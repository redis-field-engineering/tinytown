/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Global Redis instance manager for Tinytown.

use std::path::{Path, PathBuf};
use std::process::Command;

use redis::Client;
use redis::aio::ConnectionManager;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

const MIN_REDIS_VERSION: (u32, u32) = (8, 0);

/// Global Redis manager that handles a single shared Redis instance.
pub struct RedisManager {
    socket_path: PathBuf,
    pid_file: PathBuf,
    data_dir: PathBuf,
}

impl RedisManager {
    /// Create a RedisManager for the global ~/.tt directory.
    pub fn global() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| Error::Config("No home directory".into()))?;
        let tt_dir = home.join(".tt");
        std::fs::create_dir_all(&tt_dir)?;
        Ok(Self {
            socket_path: tt_dir.join("redis.sock"),
            pid_file: tt_dir.join("redis.pid"),
            data_dir: tt_dir,
        })
    }

    /// Create a RedisManager for testing in a temporary directory.
    pub fn for_testing(temp_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(temp_dir)?;
        Ok(Self {
            socket_path: temp_dir.join("redis.sock"),
            pid_file: temp_dir.join("redis.pid"),
            data_dir: temp_dir.to_path_buf(),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
    pub fn pid_file(&self) -> &Path {
        &self.pid_file
    }
    pub fn redis_url(&self) -> String {
        format!("unix://{}", self.socket_path.display())
    }

    /// Ensure Redis is running. Starts it if not already running.
    pub async fn ensure_running(&self) -> Result<()> {
        if self.socket_path.exists() {
            if self.connect().await.is_ok() {
                debug!("Redis already running at {}", self.socket_path.display());
                return Ok(());
            }
            warn!("Stale Redis socket, removing and restarting");
            std::fs::remove_file(&self.socket_path).ok();
        }
        self.start().await
    }

    async fn start(&self) -> Result<()> {
        Self::check_redis_version()?;
        let redis_bin = Self::find_redis_server();
        info!("Starting Redis at {}", self.socket_path.display());

        let status = Command::new(&redis_bin)
            .args([
                "--unixsocket",
                self.socket_path.to_str().unwrap(),
                "--unixsocketperm",
                "700",
                "--port",
                "0",
                "--daemonize",
                "yes",
                "--pidfile",
                self.pid_file.to_str().unwrap(),
                "--loglevel",
                "warning",
                "--dir",
                self.data_dir.to_str().unwrap(),
            ])
            .status()?;

        if !status.success() {
            return Err(Error::Timeout("Redis failed to start".into()));
        }

        for _ in 0..50 {
            if self.socket_path.exists() {
                debug!("Redis socket ready");
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(Error::Timeout(
            "Redis socket not ready after 5 seconds".into(),
        ))
    }

    pub async fn connect(&self) -> Result<ConnectionManager> {
        let client = Client::open(self.redis_url())?;
        let conn = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            ConnectionManager::new(client),
        )
        .await
        .map_err(|_| Error::Timeout("Redis connection timed out".into()))??;
        Ok(conn)
    }

    pub async fn stop(&self) -> Result<()> {
        if let Ok(pid_str) = std::fs::read_to_string(&self.pid_file)
            && let Ok(pid) = pid_str.trim().parse::<i32>()
        {
            info!("Stopping Redis (PID {})", pid);
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        std::fs::remove_file(&self.socket_path).ok();
        std::fs::remove_file(&self.pid_file).ok();
        Ok(())
    }

    fn find_redis_server() -> PathBuf {
        if let Some(home) = dirs::home_dir() {
            let tt_redis = home.join(".tt/bin/redis-server");
            if tt_redis.exists() {
                debug!("Using bootstrapped Redis: {}", tt_redis.display());
                return tt_redis;
            }
        }
        PathBuf::from("redis-server")
    }

    fn check_redis_version() -> Result<()> {
        let redis_bin = Self::find_redis_server();
        let output = Command::new(&redis_bin)
            .arg("--version")
            .output()
            .map_err(|_| Error::Config("redis-server not found".into()))?;
        let version_str = String::from_utf8_lossy(&output.stdout);

        if let Some(v_start) = version_str.find("v=") {
            let v_part = &version_str[v_start + 2..];
            let parts: Vec<&str> = v_part.split('.').collect();
            if parts.len() >= 2 {
                let major: u32 = parts[0].parse().unwrap_or(0);
                let minor: u32 = parts[1]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if (major, minor) >= MIN_REDIS_VERSION {
                    info!("Redis version {}.{} detected ✓", major, minor);
                    return Ok(());
                }
                return Err(Error::Config(format!(
                    "Redis {}.{} required, found {}.{}",
                    MIN_REDIS_VERSION.0, MIN_REDIS_VERSION.1, major, minor
                )));
            }
        }
        warn!("Could not parse Redis version, proceeding anyway");
        Ok(())
    }
}
