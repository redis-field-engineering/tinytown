/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Townhall - Tower-based REST control plane for Tinytown.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

use tinytown::{AppState, Town, create_router};

#[derive(Parser)]
#[command(
    name = "townhall",
    author,
    version,
    about = "Townhall - REST control plane for Tinytown"
)]
struct Cli {
    #[arg(short, long, default_value = ".")]
    town: PathBuf,
    #[arg(short, long)]
    bind: Option<String>,
    #[arg(short, long)]
    port: Option<u16>,
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> tinytown::Result<()> {
    let cli = Cli::parse();
    let filter = if cli.verbose {
        EnvFilter::new("townhall=debug,tinytown=debug,tower_http=debug")
    } else {
        EnvFilter::new("townhall=info,tinytown=info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let town = Town::connect(&cli.town).await?;
    let config = town.config().clone();
    info!("🏛️  Townhall starting for town: {}", config.name);
    let bind = cli
        .bind
        .clone()
        .unwrap_or_else(|| config.townhall.bind.clone());
    let port = cli.port.unwrap_or(config.townhall.rest_port);
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
    // Configure trace layer with sensitive header redaction
    // Never log Authorization, X-API-Key, or other sensitive headers
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            // Create span without sensitive headers - only log method, path, version
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
    info!("🚀 Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
