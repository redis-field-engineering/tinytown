/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Town - the central orchestration hub.

use std::collections::HashMap;
use std::path::Path;

use std::sync::Arc;

use redis::Client;
use redis::aio::ConnectionManager;
use tokio::process::Child;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::agent::{Agent, AgentId, AgentState, AgentType};
use crate::channel::Channel;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::global_config::GlobalConfig;
use crate::message::{Message, MessageType};
use crate::task::{Task, TaskId};

/// Town directory structure
const AGENTS_DIR: &str = "agents";
const LOGS_DIR: &str = "logs";
const TASKS_DIR: &str = "tasks";

/// Minimum required Redis version
const MIN_REDIS_VERSION: (u32, u32) = (8, 0);

/// The Town orchestrates agents and message passing.
pub struct Town {
    config: Config,
    channel: Channel,
    agents: Arc<RwLock<HashMap<AgentId, Agent>>>,
    #[expect(dead_code)]
    processes: Arc<RwLock<HashMap<AgentId, Child>>>,
}

/// PID file name for tracking Redis process
const REDIS_PID_FILE: &str = "redis.pid";

/// Find the redis-server binary, preferring ~/.tt/bin over PATH.
fn find_redis_server() -> std::path::PathBuf {
    // First, check ~/.tt/bin/redis-server (bootstrapped version)
    if let Some(home) = dirs::home_dir() {
        let tt_redis = home.join(".tt/bin/redis-server");
        if tt_redis.exists() {
            debug!("Using bootstrapped Redis: {}", tt_redis.display());
            return tt_redis;
        }
    }
    // Fall back to PATH
    std::path::PathBuf::from("redis-server")
}

impl Town {
    /// Check that Redis is installed and meets minimum version requirements.
    fn check_redis_version() -> Result<()> {
        use std::process::Command as StdCommand;

        let redis_bin = find_redis_server();

        // Check if redis-server is available
        let output = StdCommand::new(&redis_bin)
            .arg("--version")
            .output()
            .map_err(|_| Error::RedisNotInstalled)?;

        if !output.status.success() {
            return Err(Error::RedisNotInstalled);
        }

        let version_str = String::from_utf8_lossy(&output.stdout);

        // Parse version from output like: "Redis server v=8.0.0 sha=..."
        let version = Self::parse_redis_version(&version_str)?;

        if version < MIN_REDIS_VERSION {
            return Err(Error::RedisVersionTooOld(format!(
                "{}.{}",
                version.0, version.1
            )));
        }

        info!("Redis version {}.{} detected ✓", version.0, version.1);
        Ok(())
    }

    /// Parse Redis version from --version output.
    fn parse_redis_version(version_str: &str) -> Result<(u32, u32)> {
        // Format: "Redis server v=8.0.0 sha=..." or "Redis server v=7.2.4 sha=..."
        let version_part = version_str
            .split("v=")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .ok_or_else(|| Error::RedisVersionTooOld("unknown".to_string()))?;

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() < 2 {
            return Err(Error::RedisVersionTooOld(version_part.to_string()));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| Error::RedisVersionTooOld(version_part.to_string()))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| Error::RedisVersionTooOld(version_part.to_string()))?;

        Ok((major, minor))
    }

    /// Initialize a new town at the given path.
    pub async fn init(path: impl AsRef<Path>, name: impl Into<String>) -> Result<Self> {
        let path = path.as_ref();
        let name = name.into();

        // Check Redis version first
        Self::check_redis_version()?;

        info!("Initializing town '{}' at {}", name, path.display());

        // Create directory structure
        std::fs::create_dir_all(path)?;
        std::fs::create_dir_all(path.join(AGENTS_DIR))?;
        std::fs::create_dir_all(path.join(LOGS_DIR))?;
        std::fs::create_dir_all(path.join(TASKS_DIR))?;

        // Create config
        let config = Config::new(&name, path);
        config.save()?;

        // Start Redis (daemonized - stays running after we exit) and connect
        Self::start_redis(&config).await?;
        let channel = Self::connect_redis(&config).await?;

        Ok(Self {
            config,
            channel,
            agents: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Connect to an existing town.
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        // Check Redis version first (skip for remote Redis)
        let config = Config::load(&path)?;

        if !config.is_remote_redis() {
            Self::check_redis_version()?;
        }

        // Determine if Redis appears to be running
        let redis_appears_ready = if config.is_remote_redis() {
            // For remote Redis, always try to connect first
            true
        } else if config.redis.use_socket {
            // Unix socket mode - check if socket file exists
            config.socket_path().exists()
        } else {
            // Local TCP mode - try to connect to the port
            std::net::TcpStream::connect(format!("{}:{}", config.redis.bind, config.redis.port))
                .is_ok()
        };

        // Try to connect to Redis, start if needed
        let channel = if redis_appears_ready {
            // Redis appears to be running - try to connect
            match Self::connect_redis(&config).await {
                Ok(ch) => ch,
                Err(_) if !config.is_remote_redis() => {
                    // Local Redis not responding - restart it
                    warn!("Redis not responding, restarting...");
                    Self::start_redis(&config).await?;
                    Self::connect_redis(&config).await?
                }
                Err(e) => {
                    // Remote Redis failed - can't restart, propagate error
                    return Err(e);
                }
            }
        } else {
            // Redis not running - start it (only for local Redis)
            debug!("Redis not found, starting...");
            Self::start_redis(&config).await?;
            Self::connect_redis(&config).await?
        };

        Ok(Self {
            config,
            channel,
            agents: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start a local Redis server (daemonized).
    /// Supports both Unix socket (default) and TCP modes with security options.
    /// Redis will continue running after tinytown exits.
    async fn start_redis(config: &Config) -> Result<()> {
        // Skip starting local server for external/remote Redis
        if config.is_remote_redis() {
            info!(
                "Using external Redis at {}:{}",
                config.redis.host, config.redis.port
            );
            return Ok(());
        }

        // Check if using central Redis mode
        let is_central = config.is_central_redis();

        // For central Redis, check if already running
        if is_central && GlobalConfig::is_central_redis_running() {
            debug!("Central Redis already running");
            return Ok(());
        }

        // Determine PID file and working directory
        let (pid_file, work_dir) = if is_central {
            let global_dir = GlobalConfig::config_dir()?;
            std::fs::create_dir_all(&global_dir)?;
            (GlobalConfig::redis_pid_path()?, global_dir)
        } else {
            (config.root.join(REDIS_PID_FILE), config.root.clone())
        };

        let redis_bin = find_redis_server();

        debug!("Using Redis binary: {}", redis_bin.display());

        // Build args dynamically based on config
        let mut args: Vec<String> = vec![
            "--daemonize".to_string(),
            "yes".to_string(),
            "--pidfile".to_string(),
            pid_file.to_str().unwrap().to_string(),
            "--loglevel".to_string(),
            "warning".to_string(),
        ];

        if config.redis.use_socket {
            // Unix socket mode (default, current behavior)
            let socket_path = config.socket_path();

            // Remove stale socket if exists
            if socket_path.exists() {
                std::fs::remove_file(&socket_path)?;
            }

            info!("Starting Redis with socket: {}", socket_path.display());
            args.extend([
                "--unixsocket".to_string(),
                socket_path.to_str().unwrap().to_string(),
                "--unixsocketperm".to_string(),
                "700".to_string(),
                "--port".to_string(),
                "0".to_string(), // Disable TCP
            ]);
        } else {
            // TCP mode with security
            info!(
                "Starting Redis with TCP on {}:{}",
                config.redis.bind, config.redis.port
            );

            // TLS configuration
            if config.redis.tls_enabled {
                args.extend([
                    "--tls-port".to_string(),
                    config.redis.port.to_string(),
                    "--port".to_string(),
                    "0".to_string(), // Disable non-TLS port when TLS is enabled
                ]);

                if let Some(ref cert) = config.redis.tls_cert {
                    args.extend(["--tls-cert-file".to_string(), cert.clone()]);
                }
                if let Some(ref key) = config.redis.tls_key {
                    args.extend(["--tls-key-file".to_string(), key.clone()]);
                }
                if let Some(ref ca_cert) = config.redis.tls_ca_cert {
                    args.extend(["--tls-ca-cert-file".to_string(), ca_cert.clone()]);
                }
            } else {
                // Plain TCP
                args.extend(["--port".to_string(), config.redis.port.to_string()]);
            }

            // Bind address
            args.extend(["--bind".to_string(), config.redis.bind.clone()]);

            // Password authentication (check env var first via redis_password())
            if let Some(ref password) = config.redis_password() {
                args.extend(["--requirepass".to_string(), password.clone()]);
            }

            // Protected mode: Redis requires this when binding to non-localhost without password
            if config.redis.bind != "127.0.0.1" && config.redis_password().is_none() {
                warn!(
                    "Binding to {} without password - enabling protected mode",
                    config.redis.bind
                );
                args.extend(["--protected-mode".to_string(), "yes".to_string()]);
            }
        }

        // Start Redis daemonized
        if is_central {
            info!(
                "Starting central Redis on {}:{}",
                config.redis.host, config.redis.port
            );
        }
        let status = std::process::Command::new(&redis_bin)
            .args(&args)
            .current_dir(&work_dir)
            .status()?;

        if !status.success() {
            return Err(Error::Timeout("Redis failed to start".into()));
        }

        // Wait for Redis to be ready
        if config.redis.use_socket {
            // Wait for socket file
            let socket_path = config.socket_path();
            for _ in 0..50 {
                if socket_path.exists() {
                    debug!("Redis socket ready");
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        } else {
            // Wait for TCP port to be ready
            for _ in 0..50 {
                if std::net::TcpStream::connect(format!(
                    "{}:{}",
                    config.redis.bind, config.redis.port
                ))
                .is_ok()
                {
                    debug!("Redis TCP port ready");
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        Err(Error::Timeout("Redis failed to start".into()))
    }

    /// Connect to Redis.
    async fn connect_redis(config: &Config) -> Result<Channel> {
        let url = config.redis_url();
        // Use redacted URL for logging to avoid exposing password
        debug!("Connecting to Redis: {}", config.redis_url_redacted());

        let client = Client::open(url)?;

        // Short timeout - Redis should connect in milliseconds if healthy
        // Stale sockets can hang indefinitely, so fail fast and restart Redis
        let conn = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            ConnectionManager::new(client),
        )
        .await
        .map_err(|_| Error::Timeout("Redis connection timed out".into()))??;

        Ok(Channel::new(conn))
    }

    /// Spawn a new worker agent.
    pub async fn spawn_agent(&self, name: &str, model: &str) -> Result<AgentHandle> {
        let agent = Agent::new(name, model, AgentType::Worker);
        let id = agent.id;

        // Store agent state
        self.channel.set_agent_state(&agent).await?;
        self.agents.write().await.insert(id, agent);

        info!("Spawned agent '{}' ({})", name, id);

        Ok(AgentHandle {
            id,
            channel: self.channel.clone(),
        })
    }

    /// Get a handle to an existing agent.
    pub async fn agent(&self, name: &str) -> Result<AgentHandle> {
        // Look up agent in Redis (persisted across process restarts)
        if let Some(agent) = self.channel.get_agent_by_name(name).await? {
            return Ok(AgentHandle {
                id: agent.id,
                channel: self.channel.clone(),
            });
        }
        Err(Error::AgentNotFound(name.to_string()))
    }

    /// List all agents.
    pub async fn list_agents(&self) -> Vec<Agent> {
        // Get agents from Redis (persisted across process restarts)
        self.channel.list_agents().await.unwrap_or_default()
    }

    /// Get the communication channel.
    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    /// Get the town configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get the town root directory.
    pub fn root(&self) -> &Path {
        &self.config.root
    }
}

// Note: Redis runs daemonized and persists after Town is dropped.
// Use `tt stop` to explicitly stop Redis if needed.

/// Handle for interacting with an agent.
#[derive(Clone)]
pub struct AgentHandle {
    id: AgentId,
    channel: Channel,
}

impl AgentHandle {
    /// Get the agent ID.
    pub fn id(&self) -> AgentId {
        self.id
    }

    /// Assign a task to this agent.
    pub async fn assign(&self, task: Task) -> Result<TaskId> {
        let task_id = task.id;

        // Store task
        self.channel.set_task(&task).await?;

        // Send assignment message
        let msg = Message::new(
            AgentId::supervisor(),
            self.id,
            MessageType::TaskAssign {
                task_id: task_id.to_string(),
            },
        );
        self.channel.send(&msg).await?;

        Ok(task_id)
    }

    /// Send a message to this agent.
    pub async fn send(&self, msg_type: MessageType) -> Result<()> {
        let msg = Message::new(AgentId::supervisor(), self.id, msg_type);
        self.channel.send(&msg).await
    }

    /// Check agent's inbox length.
    pub async fn inbox_len(&self) -> Result<usize> {
        self.channel.inbox_len(self.id).await
    }

    /// Get agent state.
    pub async fn state(&self) -> Result<Option<Agent>> {
        self.channel.get_agent_state(self.id).await
    }

    /// Wait for agent to complete current task.
    pub async fn wait(&self) -> Result<()> {
        // Poll until agent is idle or error
        loop {
            if let Some(agent) = self.state().await? {
                match agent.state {
                    AgentState::Idle | AgentState::Stopped => return Ok(()),
                    AgentState::Error => {
                        return Err(Error::AgentNotFound(format!(
                            "Agent {} in error state",
                            self.id
                        )));
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
