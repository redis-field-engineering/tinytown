/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Townhall - Tower-based REST and MCP control plane for Tinytown.
//!
//! Supports both REST API and MCP (Model Context Protocol) interfaces.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

use tinytown::{AppState, McpState, Town, create_mcp_router, create_router};

#[derive(Parser)]
#[command(
    name = "townhall",
    author,
    version,
    about = "Townhall - REST and MCP control plane for Tinytown"
)]
struct Cli {
    #[arg(short, long, default_value = ".", global = true)]
    town: PathBuf,

    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the REST API server (default)
    Rest {
        #[arg(short, long)]
        bind: Option<String>,
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Run the MCP server over stdio
    McpStdio,
    /// Run the MCP server over HTTP/SSE
    McpHttp {
        #[arg(short, long)]
        bind: Option<String>,
        #[arg(short, long)]
        port: Option<u16>,
    },
}

#[tokio::main]
async fn main() -> tinytown::Result<()> {
    let cli = Cli::parse();
    let filter = if cli.verbose {
        EnvFilter::new("townhall=debug,tinytown=debug,tower_http=debug,tower_mcp=debug")
    } else {
        EnvFilter::new("townhall=info,tinytown=info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let town = Town::connect(&cli.town).await?;
    let config = town.config().clone();
    info!("🏛️  Townhall starting for town: {}", config.name);

    match cli.command.unwrap_or(Commands::Rest {
        bind: None,
        port: None,
    }) {
        Commands::Rest { bind, port } => run_rest_server(town, config, bind, port).await,
        Commands::McpStdio => run_mcp_stdio(town, config).await,
        Commands::McpHttp { bind, port } => run_mcp_http(town, config, bind, port).await,
    }
}

/// Run the REST API server.
async fn run_rest_server(
    town: Town,
    config: tinytown::Config,
    bind_opt: Option<String>,
    port_opt: Option<u16>,
) -> tinytown::Result<()> {
    let bind = bind_opt.unwrap_or_else(|| config.townhall.bind.clone());
    let port = port_opt.unwrap_or(config.townhall.rest_port);
    let addr: SocketAddr = format!("{}:{}", bind, port)
        .parse()
        .expect("Invalid address");

    // Startup safety rules (Issue #16)
    let is_loopback = addr.ip().is_loopback();
    let auth_mode = &config.townhall.auth.mode;

    // Fail fast: non-loopback binding without authentication
    if !is_loopback && *auth_mode == tinytown::AuthMode::None {
        tracing::error!(
            "❌ FATAL: Binding to non-loopback address {} with auth.mode=none is not allowed. \
             Configure API key or OIDC authentication, or bind to 127.0.0.1.",
            addr
        );
        std::process::exit(1);
    }

    // Warn when using API key mode on non-loopback
    if !is_loopback && *auth_mode == tinytown::AuthMode::ApiKey {
        tracing::warn!(
            "⚠️  Running API key authentication on non-loopback address {}. \
             Consider using OIDC for production deployments.",
            addr
        );
    }

    // Fail fast: TLS enabled but invalid config
    if let Some(err) = config.townhall.tls.validate() {
        tracing::error!("❌ FATAL: TLS configuration error: {}", err);
        std::process::exit(1);
    }

    // Fail fast: mTLS required but invalid config
    if let Some(err) = config.townhall.mtls.validate() {
        tracing::error!("❌ FATAL: mTLS configuration error: {}", err);
        std::process::exit(1);
    }

    info!("🔐 Auth mode: {:?}", auth_mode);
    if config.townhall.tls.enabled {
        info!("🔒 TLS enabled");
    }
    if config.townhall.mtls.enabled {
        info!(
            "🔒 mTLS enabled (required: {})",
            config.townhall.mtls.required
        );
    }

    let timeout_duration = Duration::from_millis(config.townhall.request_timeout_ms);
    let auth_config = Arc::new(config.townhall.auth.clone());
    let state = Arc::new(AppState { town, auth_config });
    #[allow(deprecated)]
    let timeout_layer = TimeoutLayer::new(timeout_duration);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri().path(),
                version = ?request.version(),
            )
        })
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    let app = create_router(state)
        .layer(trace_layer)
        .layer(CompressionLayer::new())
        .layer(timeout_layer)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    info!("🚀 REST API listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Run the MCP server over stdio.
async fn run_mcp_stdio(town: Town, _config: tinytown::Config) -> tinytown::Result<()> {
    use tower_mcp::StdioTransport;

    info!("🔌 Starting MCP server over stdio");
    let mcp_state = Arc::new(McpState::new(town));
    let router = create_mcp_router(mcp_state, "tinytown-mcp", env!("CARGO_PKG_VERSION"));

    StdioTransport::new(router)
        .run()
        .await
        .map_err(|e| tinytown::Error::Io(std::io::Error::other(format!("MCP error: {}", e))))?;

    Ok(())
}

/// Run the MCP server over HTTP/SSE.
async fn run_mcp_http(
    town: Town,
    config: tinytown::Config,
    bind_opt: Option<String>,
    port_opt: Option<u16>,
) -> tinytown::Result<()> {
    use tower_mcp::transport::HttpTransport;

    let bind = bind_opt.unwrap_or_else(|| config.townhall.bind.clone());
    // Use MCP port (default: REST port + 1)
    let port = port_opt.unwrap_or(config.townhall.rest_port + 1);
    let addr: SocketAddr = format!("{}:{}", bind, port)
        .parse()
        .expect("Invalid address");

    info!("🔌 Starting MCP server over HTTP/SSE");
    let mcp_state = Arc::new(McpState::new(town));
    let router = create_mcp_router(mcp_state, "tinytown-mcp", env!("CARGO_PKG_VERSION"));

    let transport = HttpTransport::new(router);
    let app = transport.into_router().layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    );

    info!("🚀 MCP HTTP/SSE listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
