# Design: TCP Redis Migration with Security

**Author:** architect2
**Date:** 2026-03-08
**Status:** Approved (with minor recommendations addressed)

## Overview

This design documents the migration from Unix socket-only Redis to support TCP connections with security features, enabling remote Redis servers and Docker/container deployments.

## Current State

- Redis uses Unix socket (`redis.sock`) by default
- TCP is disabled (`--port 0`)
- Config supports `use_socket` toggle, but TCP mode is **not implemented** in `start_redis()`
- No authentication or TLS support

## Goals

1. **Support TCP connections** for remote Redis, Docker, and cloud deployments
2. **Implement authentication** via Redis AUTH (password or ACLs)
3. **Optional TLS encryption** for secure connections over untrusted networks
4. **Backward compatible** - Unix socket remains default for local development

## Design

### 1. Configuration Changes (`src/config.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    // Existing fields...
    pub use_socket: bool,
    pub socket_path: String,
    pub host: String,
    pub port: u16,
    pub persist: bool,
    pub aof_path: String,
    
    // NEW: Security fields
    /// Redis password (AUTH command)
    #[serde(default)]
    pub password: Option<String>,
    
    /// Enable TLS encryption
    #[serde(default)]
    pub tls_enabled: bool,
    
    /// Path to TLS certificate file (PEM)
    #[serde(default)]
    pub tls_cert: Option<String>,
    
    /// Path to TLS private key file (PEM)
    #[serde(default)]
    pub tls_key: Option<String>,
    
    /// Path to CA certificate for verification
    #[serde(default)]
    pub tls_ca_cert: Option<String>,
    
    /// Bind address for managed Redis (0.0.0.0 for remote, 127.0.0.1 for local only)
    #[serde(default = "default_bind")]
    pub bind: String,
}

fn default_bind() -> String {
    "127.0.0.1".to_string()  // Local-only by default for security
}
```

### 2. Connection URL Generation (`src/config.rs`)

```rust
impl Config {
    /// Returns the Redis connection URL.
    ///
    /// ⚠️ WARNING: When password is set, this URL contains credentials.
    /// Do NOT log the full URL. Use `redis_url_redacted()` for logging.
    pub fn redis_url(&self) -> String {
        if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            let scheme = if self.redis.tls_enabled { "rediss" } else { "redis" };
            // Check env var override for password
            let password = std::env::var("TINYTOWN_REDIS_PASSWORD")
                .ok()
                .or_else(|| self.redis.password.clone());
            match password {
                Some(pass) => format!("{}://:{}@{}:{}", scheme, pass, self.redis.host, self.redis.port),
                None => format!("{}://{}:{}", scheme, self.redis.host, self.redis.port),
            }
        }
    }

    /// Returns a redacted version of the Redis URL safe for logging.
    pub fn redis_url_redacted(&self) -> String {
        if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            let scheme = if self.redis.tls_enabled { "rediss" } else { "redis" };
            let has_password = std::env::var("TINYTOWN_REDIS_PASSWORD").is_ok()
                || self.redis.password.is_some();
            if has_password {
                format!("{}://:****@{}:{}", scheme, self.redis.host, self.redis.port)
            } else {
                format!("{}://{}:{}", scheme, self.redis.host, self.redis.port)
            }
        }
    }
}
```

### 3. TCP-Enabled Redis Start (`src/town.rs`)

```rust
async fn start_redis(config: &Config) -> Result<()> {
    if !config.redis.use_socket && !config.redis.host.contains("localhost") 
       && config.redis.host != "127.0.0.1" {
        // External Redis - don't start a local server
        info!("Using external Redis at {}:{}", config.redis.host, config.redis.port);
        return Ok(());
    }
    
    let redis_bin = find_redis_server();
    let pid_file = config.root.join(REDIS_PID_FILE);
    
    let mut args = vec![
        "--daemonize", "yes",
        "--pidfile", pid_file.to_str().unwrap(),
        "--loglevel", "warning",
    ];
    
    if config.redis.use_socket {
        // Unix socket mode (current behavior)
        let socket_path = config.socket_path();
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }
        args.extend(["--unixsocket", socket_path.to_str().unwrap()]);
        args.extend(["--unixsocketperm", "700"]);
        args.extend(["--port", "0"]);
    } else {
        // TCP mode with security
        let port_str = config.redis.port.to_string();
        args.extend(["--bind", &config.redis.bind]);

        // Protected mode handling:
        // - For 127.0.0.1: explicitly disable (safe local access)
        // - For other bindings: keep enabled (default) to require auth
        if config.redis.bind == "127.0.0.1" {
            args.extend(["--protected-mode", "no"]);
        }
        // Note: protected-mode defaults to "yes" in Redis, so we don't need
        // to explicitly set it for non-localhost bindings

        // Authentication (check env var first)
        let password = std::env::var("TINYTOWN_REDIS_PASSWORD")
            .ok()
            .or_else(|| config.redis.password.clone());
        if let Some(ref pass) = password {
            args.extend(["--requirepass", pass]);
        }

        // TLS configuration
        if config.redis.tls_enabled {
            // When TLS is enabled, use tls-port and disable plain port
            args.extend(["--tls-port", &port_str]);
            args.extend(["--port", "0"]); // Disable non-TLS port
            if let Some(ref cert) = config.redis.tls_cert {
                args.extend(["--tls-cert-file", cert]);
            }
            if let Some(ref key) = config.redis.tls_key {
                args.extend(["--tls-key-file", key]);
            }
            // Note: If tls_ca_cert is None, Redis uses system CA certificates
            // for client certificate validation (if enabled)
            if let Some(ref ca) = config.redis.tls_ca_cert {
                args.extend(["--tls-ca-cert-file", ca]);
            }
        } else {
            // Non-TLS TCP mode
            args.extend(["--port", &port_str]);
        }
    }
    // ... rest of start logic
}
```

### 4. Config Examples

**Local TCP (dev/testing):**
```toml
[redis]
use_socket = false
host = "127.0.0.1"
port = 6380
```

**Remote Redis (no TLS):**
```toml
[redis]
use_socket = false
host = "redis.example.com"
port = 6379
password = "secret123"
```

**Production with TLS:**
```toml
[redis]
use_socket = false
host = "redis.example.com"
port = 6379
password = "secret123"
tls_enabled = true
tls_cert = "/etc/ssl/redis.crt"
tls_key = "/etc/ssl/redis.key"
tls_ca_cert = "/etc/ssl/ca.crt"
```

## Security Considerations

1. **Default to localhost binding** - Prevents accidental exposure
2. **Password required for remote** - Protected mode enforcement
3. **TLS recommended for production** - Encrypt data in transit
4. **Environment variable support** - `TINYTOWN_REDIS_PASSWORD` env var takes precedence over config file password
5. **TLS CA certificate handling** - When `tls_ca_cert` is not specified but TLS is enabled, Redis uses the system's default CA certificates for verification
6. **Log redaction** - Use `redis_url_redacted()` for logging to avoid exposing credentials

## Implementation Order

1. Add new config fields to `RedisConfig` (backward compatible with defaults)
2. Update `redis_url()` to support password and TLS schemes
3. Modify `start_redis()` to handle TCP mode with full args
4. Update `connect_redis()` to support TLS client configuration
5. Add documentation and examples
6. Add integration tests for TCP mode

## Testing Plan

1. Unit tests for URL generation with various config combinations
2. Integration test: TCP mode with password
3. Integration test: External Redis connection
4. Manual test: TLS with self-signed certificates

## Dependencies

- Redis crate already supports TLS via `redis/tokio-native-tls` or `redis/tokio-rustls`
- May need to add feature flag to Cargo.toml for TLS support

