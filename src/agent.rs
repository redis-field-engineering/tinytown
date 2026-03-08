/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Agent definitions and lifecycle management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(Uuid);

impl AgentId {
    /// Create a new random agent ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Create a well-known ID for the supervisor.
    #[must_use]
    pub fn supervisor() -> Self {
        // Fixed UUID for supervisor
        Self(Uuid::from_bytes([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]))
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AgentId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Agent types in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    /// Supervisor agent - coordinates workers
    Supervisor,
    /// Worker agent - executes tasks
    #[default]
    Worker,
}

/// Agent lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    /// Agent is starting up
    #[default]
    Starting,
    /// Agent is idle, waiting for work
    Idle,
    /// Agent is working on a task
    Working,
    /// Agent is paused
    Paused,
    /// Agent has stopped
    Stopped,
    /// Agent encountered an error
    Error,
}

impl AgentState {
    /// Check if agent is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Error)
    }

    /// Check if agent can accept new work.
    #[must_use]
    pub fn can_accept_work(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// Configuration for an agent CLI (e.g., claude, auggie, codex).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCli {
    /// CLI name (e.g., "claude", "auggie", "codex")
    pub name: String,
    /// Command to run the agent CLI
    pub command: String,
    /// Working directory
    pub workdir: Option<String>,
    /// Environment variables
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

impl AgentCli {
    /// Create a new agent CLI configuration.
    #[must_use]
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            workdir: None,
            env: std::collections::HashMap::new(),
        }
    }
}

/// An agent in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Unique agent identifier
    pub id: AgentId,
    /// Human-readable name
    pub name: String,
    /// Agent type
    pub agent_type: AgentType,
    /// Current state
    pub state: AgentState,
    /// CLI being used (e.g., "claude", "auggie")
    pub cli: String,
    /// Current task (if working)
    pub current_task: Option<crate::task::TaskId>,
    /// When agent was created
    pub created_at: DateTime<Utc>,
    /// Last heartbeat timestamp
    pub last_heartbeat: DateTime<Utc>,
    /// Number of tasks completed
    pub tasks_completed: u64,
    /// Number of rounds completed
    #[serde(default)]
    pub rounds_completed: u64,
}

impl Agent {
    /// Create a new agent.
    #[must_use]
    pub fn new(name: impl Into<String>, cli: impl Into<String>, agent_type: AgentType) -> Self {
        let now = Utc::now();
        Self {
            id: AgentId::new(),
            name: name.into(),
            agent_type,
            state: AgentState::Starting,
            cli: cli.into(),
            current_task: None,
            created_at: now,
            last_heartbeat: now,
            tasks_completed: 0,
            rounds_completed: 0,
        }
    }

    /// Create a supervisor agent.
    #[must_use]
    pub fn supervisor(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AgentId::supervisor(),
            name: name.into(),
            agent_type: AgentType::Supervisor,
            state: AgentState::Starting,
            cli: "supervisor".into(),
            current_task: None,
            created_at: now,
            last_heartbeat: now,
            tasks_completed: 0,
            rounds_completed: 0,
        }
    }
}
