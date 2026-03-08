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

/// Global configuration that applies across all towns.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    /// Default CLI to use when spawning agents (e.g., "claude", "auggie")
    #[serde(default = "default_cli")]
    pub default_cli: String,

    /// Custom CLI definitions (name -> command)
    #[serde(default)]
    pub agent_clis: std::collections::HashMap<String, String>,
}

fn default_cli() -> String {
    "claude".to_string()
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

    /// Set a config value by key
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "default_cli" => {
                self.default_cli = value.to_string();
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
            _ if key.starts_with("agent_clis.") => {
                let cli_name = key.strip_prefix("agent_clis.").unwrap();
                self.agent_clis.get(cli_name).cloned()
            }
            _ => None,
        }
    }
}
