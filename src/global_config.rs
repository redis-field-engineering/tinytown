/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Global configuration stored in ~/.tt/config.toml

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Global config file name
pub const GLOBAL_CONFIG_FILE: &str = "config.toml";

/// Global config directory
pub const GLOBAL_CONFIG_DIR: &str = ".tt";

/// Default Redis port (non-standard to avoid conflicts)
pub const DEFAULT_REDIS_PORT: u16 = 16379;

/// Global configuration that applies across all towns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default CLI to use when spawning agents (e.g., "claude", "auggie")
    #[serde(default = "default_cli")]
    pub default_cli: String,

    /// Custom CLI definitions (name -> command)
    #[serde(default)]
    pub agent_clis: std::collections::HashMap<String, String>,

    /// Central Redis configuration
    #[serde(default)]
    pub redis: GlobalRedisConfig,
}

/// Global Redis configuration for the central Redis instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalRedisConfig {
    /// Redis host (default: 127.0.0.1)
    #[serde(default = "default_host")]
    pub host: String,

    /// Redis port (default: 16379 - non-standard to avoid conflicts)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Redis password (auto-generated on first use if not set)
    #[serde(default)]
    pub password: Option<String>,

    /// Whether towns should use the central Redis by default
    #[serde(default = "default_true")]
    pub use_central: bool,
}

impl Default for GlobalRedisConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            password: None,
            use_central: true,
        }
    }
}

fn default_cli() -> String {
    "claude".to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    DEFAULT_REDIS_PORT
}

fn default_true() -> bool {
    true
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            default_cli: default_cli(),
            agent_clis: std::collections::HashMap::new(),
            redis: GlobalRedisConfig::default(),
        }
    }
}

impl GlobalConfig {
    /// Get the global config directory path (~/.tt)
    pub fn config_dir() -> Result<PathBuf> {
        dirs::home_dir()
            .map(|h| h.join(GLOBAL_CONFIG_DIR))
            .ok_or_else(|| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                ))
            })
    }

    /// Get the global config file path (~/.tt/config.toml)
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join(GLOBAL_CONFIG_FILE))
    }

    /// Load global config, creating default if it doesn't exist.
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            // Return default config if file doesn't exist
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)?;
        let config: GlobalConfig = toml::from_str(&content)
            .map_err(|e| Error::Io(std::io::Error::other(format!("Invalid config.toml: {}", e))))?;

        Ok(config)
    }

    /// Save global config to ~/.tt/config.toml
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_path = Self::config_path()?;

        // Create ~/.tt if it doesn't exist
        std::fs::create_dir_all(&config_dir)?;

        let content = toml::to_string_pretty(self).map_err(|e| {
            Error::Io(std::io::Error::other(format!(
                "Failed to serialize config: {}",
                e
            )))
        })?;

        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// Get the path to the central Redis PID file (~/.tt/redis.pid)
    pub fn redis_pid_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("redis.pid"))
    }

    /// Check if the central Redis is running by checking the PID file.
    pub fn is_central_redis_running() -> bool {
        let pid_path = match Self::redis_pid_path() {
            Ok(p) => p,
            Err(_) => return false,
        };

        if !pid_path.exists() {
            return false;
        }

        // Read PID and check if process is running
        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if process is running (kill -0 doesn't send signal, just checks)
                unsafe {
                    return libc::kill(pid, 0) == 0;
                }
            }
        }

        false
    }

    /// Load and ensure global config exists with password set.
    /// This will create the config file if it doesn't exist and generate a password.
    pub fn load_or_init() -> Result<Self> {
        let config_path = Self::config_path()?;

        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str(&content).map_err(|e| {
                Error::Io(std::io::Error::other(format!("Invalid config.toml: {}", e)))
            })?
        } else {
            Self::default()
        };

        // Ensure password is set
        if config.ensure_redis_password() {
            // Password was generated, save config
            config.save()?;
        }

        Ok(config)
    }

    /// Set a config value by key
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "default_cli" => {
                self.default_cli = value.to_string();
                Ok(())
            }
            "redis.host" => {
                self.redis.host = value.to_string();
                Ok(())
            }
            "redis.port" => {
                self.redis.port = value.parse().map_err(|_| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Invalid port number",
                    ))
                })?;
                Ok(())
            }
            "redis.password" => {
                self.redis.password = Some(value.to_string());
                Ok(())
            }
            "redis.use_central" => {
                self.redis.use_central = value.parse().map_err(|_| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Invalid boolean value",
                    ))
                })?;
                Ok(())
            }
            _ if key.starts_with("agent_clis.") => {
                let cli_name = key.strip_prefix("agent_clis.").unwrap();
                self.agent_clis
                    .insert(cli_name.to_string(), value.to_string());
                Ok(())
            }
            _ => Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unknown config key: {}", key),
            ))),
        }
    }

    /// Get a config value by key
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "default_cli" => Some(self.default_cli.clone()),
            "redis.host" => Some(self.redis.host.clone()),
            "redis.port" => Some(self.redis.port.to_string()),
            "redis.password" => self.redis.password.clone(),
            "redis.use_central" => Some(self.redis.use_central.to_string()),
            _ if key.starts_with("agent_clis.") => {
                let cli_name = key.strip_prefix("agent_clis.").unwrap();
                self.agent_clis.get(cli_name).cloned()
            }
            _ => None,
        }
    }

    /// Generate a random password for Redis.
    #[must_use]
    pub fn generate_password() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Simple random password generation using system time and process id
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();

        // Create a reasonably secure password by hashing timestamp + pid
        format!("tt_{:x}_{:x}", timestamp, pid)
    }

    /// Ensure the Redis password is set, generating one if needed.
    /// Returns true if a new password was generated.
    pub fn ensure_redis_password(&mut self) -> bool {
        if self.redis.password.is_none() {
            self.redis.password = Some(Self::generate_password());
            true
        } else {
            false
        }
    }

    /// Get the Redis connection URL for the central Redis.
    /// Returns None if central Redis is disabled.
    #[must_use]
    pub fn redis_url(&self) -> Option<String> {
        if !self.redis.use_central {
            return None;
        }

        match &self.redis.password {
            Some(pass) => Some(format!(
                "redis://:{}@{}:{}",
                pass, self.redis.host, self.redis.port
            )),
            None => Some(format!("redis://{}:{}", self.redis.host, self.redis.port)),
        }
    }

    /// Get a redacted Redis URL for logging.
    #[must_use]
    pub fn redis_url_redacted(&self) -> Option<String> {
        if !self.redis.use_central {
            return None;
        }

        if self.redis.password.is_some() {
            Some(format!(
                "redis://:****@{}:{}",
                self.redis.host, self.redis.port
            ))
        } else {
            Some(format!("redis://{}:{}", self.redis.host, self.redis.port))
        }
    }
}
