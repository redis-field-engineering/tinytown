/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! # Tinytown
//!
//! A simple, fast multi-agent orchestration system using Redis for message passing.
//!
//! Tinytown takes the best ideas from complex orchestration systems and distills them
//! into a minimal, fast, and easy-to-use library. It uses Redis with Unix socket
//! communication for blazing-fast local message passing between agents.
//!
//! ## Key Features
//!
//! - **Simple**: 5 core types, 1 config file, 3 commands
//! - **Fast**: Redis with Unix socket for sub-millisecond message passing
//! - **Reliable**: Agents persist work in git worktrees, survive crashes
//! - **Observable**: Built-in activity logging and status monitoring
//!
//! ## Quick Example
//!
//! ```no_run
//! use tinytown::{Town, Agent, Task, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Connect to town (auto-starts Redis if needed)
//!     let town = Town::connect("./mytown").await?;
//!
//!     // Create an agent
//!     let agent = town.spawn_agent("worker-1", "claude").await?;
//!
//!     // Assign a task
//!     let task = Task::new("Fix the bug in auth.rs");
//!     agent.assign(task).await?;
//!
//!     // Wait for completion
//!     agent.wait().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod agent;
pub mod app;
pub mod channel;
pub mod config;
pub mod error;
pub mod events;
pub mod global_config;
pub mod keys;
pub mod message;
pub mod migration;
pub mod mission;
pub mod plan;
pub mod redis_manager;
pub mod task;
pub mod town;

pub use agent::{
    Agent, AgentId, AgentState, AgentType, RoleDefinition, SpawnMode, builtin_roles, roles,
};
pub use app::audit::{AuditEvent, AuditResult, audit_middleware};
pub use app::auth::{AuthError, AuthState, Principal, auth_middleware, generate_api_key};
pub use app::mcp::{McpState, create_mcp_router};
pub use app::rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
pub use app::server::{AppState, ProblemDetails, create_router};
pub use app::services::{
    AgentService, BacklogService, MessageService, RecoveryService, TaskService,
};
pub use channel::Channel;
pub use config::{AuthConfig, AuthMode, Config, MtlsConfig, Scope, TlsConfig, TownhallConfig};
pub use error::{Error, Result};
pub use events::{EventStream, EventType, TownEvent};
pub use global_config::GlobalConfig;
pub use keys::RedisKeys;
pub use message::{ConfirmationType, Message, MessageId, MessageType, Priority};
pub use migration::{
    HashMigrationStats, MigrationStats, migrate_json_to_hash, migrate_to_town_isolation,
    needs_hash_migration, needs_migration, preview_hash_migration, preview_migration,
};
pub use plan::{TaskEntry, TasksFile, TasksMeta};
pub use redis_manager::RedisManager;
pub use task::{Task, TaskId, TaskState};
pub use town::{TT_DIR, Town};
