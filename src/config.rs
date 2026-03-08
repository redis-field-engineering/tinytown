/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Configuration management for tinytown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::AgentCli;
use crate::error::{Error, Result};

/// Default Redis socket path within a town.
pub const DEFAULT_SOCKET_NAME: &str = "redis.sock";

/// Default config file name.
pub const CONFIG_FILE: &str = "tinytown.toml";

/// Town configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Town name
    pub name: String,

    /// Town root directory
    #[serde(skip)]
    pub root: PathBuf,

    /// Redis configuration
    #[serde(default)]
    pub redis: RedisConfig,

    /// Available agent CLIs (e.g., claude, auggie, codex)
    #[serde(default)]
    pub agent_clis: HashMap<String, AgentCli>,

    /// Default CLI to use when spawning agents
    #[serde(default = "default_cli")]
    pub default_cli: String,

    /// Maximum concurrent agents
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,
}

fn default_cli() -> String {
    "claude".to_string()
}

fn default_max_agents() -> usize {
    10
}

/// Redis connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Use Unix socket (faster) vs TCP
    #[serde(default = "default_true")]
    pub use_socket: bool,

    /// Socket path (relative to town root)
    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    /// TCP host (if not using socket)
    #[serde(default = "default_host")]
    pub host: String,

    /// TCP port (if not using socket)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Enable AOF persistence (state survives restart)
    #[serde(default)]
    pub persist: bool,

    /// AOF file path (relative to town root)
    #[serde(default = "default_aof_path")]
    pub aof_path: String,

    // Security fields for TCP mode
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

fn default_aof_path() -> String {
    "redis.aof".to_string()
}

fn default_true() -> bool {
    true
}

fn default_socket_path() -> String {
    DEFAULT_SOCKET_NAME.to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    6379
}

fn default_bind() -> String {
    "127.0.0.1".to_string()
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            use_socket: true,
            socket_path: DEFAULT_SOCKET_NAME.to_string(),
            host: "127.0.0.1".to_string(),
            port: 6379,
            persist: false,
            aof_path: default_aof_path(),
            password: None,
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca_cert: None,
            bind: default_bind(),
        }
    }
}

impl Config {
    /// Create a new configuration with defaults.
    #[must_use]
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let mut agent_clis = HashMap::new();

        // Built-in CLI presets with correct non-interactive flags

        // Claude Code: --print for non-interactive, --dangerously-skip-permissions for full access
        agent_clis.insert(
            "claude".to_string(),
            AgentCli::new("claude", "claude --print --dangerously-skip-permissions"),
        );

        // Auggie (Augment CLI): --print for non-interactive
        agent_clis.insert(
            "auggie".to_string(),
            AgentCli::new("auggie", "auggie --print"),
        );

        // Codex: exec for non-interactive, --dangerously-bypass-approvals-and-sandbox for full access
        agent_clis.insert(
            "codex".to_string(),
            AgentCli::new(
                "codex",
                "codex exec --dangerously-bypass-approvals-and-sandbox",
            ),
        );

        // Aider: --yes for auto-confirm, --no-auto-commits to not auto-commit
        agent_clis.insert(
            "aider".to_string(),
            AgentCli::new("aider", "aider --yes --no-auto-commits --message"),
        );

        // These may need updates when their CLIs are available/verified
        agent_clis.insert("gemini".to_string(), AgentCli::new("gemini", "gemini"));
        agent_clis.insert(
            "copilot".to_string(),
            AgentCli::new("copilot", "gh copilot"),
        );
        agent_clis.insert("cursor".to_string(), AgentCli::new("cursor", "cursor"));

        Self {
            name: name.into(),
            root: root.into(),
            redis: RedisConfig::default(),
            agent_clis,
            default_cli: "claude".to_string(),
            max_agents: 10,
        }
    }

    /// Load configuration from a town directory.
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let config_path = root.join(CONFIG_FILE);

        if !config_path.exists() {
            return Err(Error::NotInitialized(root.display().to_string()));
        }

        let content = std::fs::read_to_string(&config_path)?;
        let mut config: Config = toml::from_str(&content).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Invalid tinytown.toml: {}",
                e
            )))
        })?;
        config.root = root.to_path_buf();

        Ok(config)
    }

    /// Save configuration to the town directory.
    pub fn save(&self) -> Result<()> {
        let config_path = self.root.join(CONFIG_FILE);
        let content = toml::to_string_pretty(self).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to serialize config: {}",
                e
            )))
        })?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// Get the Redis socket path.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        // Ensure we have an absolute path for Redis
        let base = if self.root.is_absolute() {
            self.root.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(&self.root)
        };
        base.join(&self.redis.socket_path)
    }

    /// Get Redis connection URL.
    ///
    /// ⚠️ WARNING: When password is set, this URL contains credentials.
    /// Do NOT log the full URL. Use `redis_url_redacted()` for logging.
    #[must_use]
    pub fn redis_url(&self) -> String {
        if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            // Use rediss:// scheme for TLS, redis:// for plain TCP
            let scheme = if self.redis.tls_enabled {
                "rediss"
            } else {
                "redis"
            };

            // Check env var override for password (TINYTOWN_REDIS_PASSWORD takes precedence)
            let password = std::env::var("TINYTOWN_REDIS_PASSWORD")
                .ok()
                .or_else(|| self.redis.password.clone());

            // Include password in URL if configured
            match password {
                Some(pass) => {
                    format!(
                        "{}://:{}@{}:{}",
                        scheme, pass, self.redis.host, self.redis.port
                    )
                }
                None => format!("{}://{}:{}", scheme, self.redis.host, self.redis.port),
            }
        }
    }

    /// Get the Redis password, checking env var first.
    #[must_use]
    pub fn redis_password(&self) -> Option<String> {
        std::env::var("TINYTOWN_REDIS_PASSWORD")
            .ok()
            .or_else(|| self.redis.password.clone())
    }

    /// Get a redacted version of the Redis URL safe for logging.
    /// Masks the password with **** if one is configured.
    #[must_use]
    pub fn redis_url_redacted(&self) -> String {
        if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            let scheme = if self.redis.tls_enabled {
                "rediss"
            } else {
                "redis"
            };

            // Check if any password is set (env var or config)
            let has_password =
                std::env::var("TINYTOWN_REDIS_PASSWORD").is_ok() || self.redis.password.is_some();

            if has_password {
                format!("{}://:****@{}:{}", scheme, self.redis.host, self.redis.port)
            } else {
                format!("{}://{}:{}", scheme, self.redis.host, self.redis.port)
            }
        }
    }

    /// Check if Redis host is remote (not localhost/127.0.0.1)
    #[must_use]
    pub fn is_remote_redis(&self) -> bool {
        !self.redis.use_socket
            && self.redis.host != "127.0.0.1"
            && self.redis.host != "localhost"
            && !self.redis.host.starts_with("127.")
    }
}
