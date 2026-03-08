/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Configuration management for tinytown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::AgentModel;
use crate::error::{Error, Result};

/// Default Redis socket path within a town.
pub const DEFAULT_SOCKET_NAME: &str = "redis.sock";

/// Default config file name.
pub const CONFIG_FILE: &str = "tinytown.json";

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

    /// Available agent models
    #[serde(default)]
    pub models: HashMap<String, AgentModel>,

    /// Default model to use
    #[serde(default = "default_model")]
    pub default_model: String,

    /// Maximum concurrent agents
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,
}

fn default_model() -> String {
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

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            use_socket: true,
            socket_path: DEFAULT_SOCKET_NAME.to_string(),
            host: "127.0.0.1".to_string(),
            port: 6379,
        }
    }
}

impl Config {
    /// Create a new configuration with defaults.
    #[must_use]
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let mut models = HashMap::new();
        
        // Built-in model presets
        models.insert(
            "claude".to_string(),
            AgentModel::new("claude", "claude --print"),
        );
        models.insert(
            "gemini".to_string(),
            AgentModel::new("gemini", "gemini"),
        );
        models.insert(
            "auggie".to_string(),
            AgentModel::new("auggie", "augment"),
        );
        models.insert(
            "codex".to_string(),
            AgentModel::new("codex", "codex"),
        );
        models.insert(
            "copilot".to_string(),
            AgentModel::new("copilot", "gh copilot"),
        );
        models.insert(
            "aider".to_string(),
            AgentModel::new("aider", "aider"),
        );
        models.insert(
            "cursor".to_string(),
            AgentModel::new("cursor", "cursor"),
        );

        Self {
            name: name.into(),
            root: root.into(),
            redis: RedisConfig::default(),
            models,
            default_model: "claude".to_string(),
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
        let mut config: Config = serde_json::from_str(&content)?;
        config.root = root.to_path_buf();
        
        Ok(config)
    }

    /// Save configuration to the town directory.
    pub fn save(&self) -> Result<()> {
        let config_path = self.root.join(CONFIG_FILE);
        let content = serde_json::to_string_pretty(self)?;
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
            std::env::current_dir()
                .unwrap_or_default()
                .join(&self.root)
        };
        base.join(&self.redis.socket_path)
    }

    /// Get Redis connection URL.
    #[must_use]
    pub fn redis_url(&self) -> String {
        if self.redis.use_socket {
            format!("unix://{}", self.socket_path().display())
        } else {
            format!("redis://{}:{}", self.redis.host, self.redis.port)
        }
    }
}

