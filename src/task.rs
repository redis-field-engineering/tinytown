/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Task definitions and state management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentId;

/// Unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    /// Create a new random task ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaskId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Task execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    /// Task is waiting to be assigned
    #[default]
    Pending,
    /// Task is assigned to an agent
    Assigned,
    /// Task is being worked on
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed
    Failed,
    /// Task was cancelled
    Cancelled,
}

impl TaskState {
    /// Check if task is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// A task that can be assigned to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier
    pub id: TaskId,
    /// Human-readable task description
    pub description: String,
    /// Current state
    pub state: TaskState,
    /// Assigned agent (if any)
    pub assigned_to: Option<AgentId>,
    /// When the task was created
    pub created_at: DateTime<Utc>,
    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
    /// When the task was completed (if applicable)
    pub completed_at: Option<DateTime<Utc>>,
    /// Result or error message
    pub result: Option<String>,
    /// Optional parent task for hierarchical tasks
    pub parent_id: Option<TaskId>,
    /// Optional tags for filtering
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Task {
    /// Create a new task with the given description.
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            description: description.into(),
            state: TaskState::Pending,
            assigned_to: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            result: None,
            parent_id: None,
            tags: Vec::new(),
        }
    }

    /// Add tags to the task.
    #[must_use]
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Set parent task ID.
    #[must_use]
    pub fn with_parent(mut self, parent: TaskId) -> Self {
        self.parent_id = Some(parent);
        self
    }

    /// Mark task as assigned to an agent.
    pub fn assign(&mut self, agent: AgentId) {
        self.assigned_to = Some(agent);
        self.state = TaskState::Assigned;
        self.updated_at = Utc::now();
    }

    /// Mark task as running.
    pub fn start(&mut self) {
        self.state = TaskState::Running;
        self.updated_at = Utc::now();
    }

    /// Mark task as completed with a result.
    pub fn complete(&mut self, result: impl Into<String>) {
        self.state = TaskState::Completed;
        self.result = Some(result.into());
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark task as failed with an error message.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.state = TaskState::Failed;
        self.result = Some(error.into());
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }
}
