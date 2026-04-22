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

    /// Return the first 4 hex characters of the UUID for compact display.
    #[must_use]
    pub fn short_id(&self) -> String {
        self.0.to_string().chars().take(4).collect()
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
    /// Task is being worked on (in-flight)
    #[serde(alias = "in_flight")]
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

    /// Check if task is in-flight (being actively worked on).
    #[must_use]
    pub fn is_in_flight(&self) -> bool {
        matches!(self, Self::Running)
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
    /// When work on the task started (if applicable)
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
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
            started_at: None,
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

    /// Mark task as running (in-flight).
    pub fn start(&mut self) {
        let now = Utc::now();
        self.state = TaskState::Running;
        self.started_at = Some(now);
        self.updated_at = now;
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

    /// Mark task as cancelled with an optional reason.
    pub fn cancel(&mut self, reason: impl Into<String>) {
        self.state = TaskState::Cancelled;
        self.result = Some(reason.into());
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Reset an assigned/running task back to pending so it can be requeued.
    pub fn requeue(&mut self) {
        self.state = TaskState::Pending;
        self.assigned_to = None;
        self.started_at = None;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentId;

    #[test]
    fn cancel_sets_terminal_state_and_records_reason() {
        let mut task = Task::new("ship it");
        task.assign(AgentId::new());
        task.start();
        let before = task.updated_at;

        task.cancel("assignee gone");

        assert_eq!(task.state, TaskState::Cancelled);
        assert!(task.state.is_terminal());
        assert_eq!(task.result.as_deref(), Some("assignee gone"));
        assert!(task.completed_at.is_some());
        assert!(task.updated_at >= before);
    }

    #[test]
    fn requeue_clears_assignment_and_returns_to_pending() {
        let mut task = Task::new("ship it");
        let agent = AgentId::new();
        task.assign(agent);
        task.start();
        assert_eq!(task.state, TaskState::Running);
        assert_eq!(task.assigned_to, Some(agent));
        assert!(task.started_at.is_some());

        task.requeue();

        assert_eq!(task.state, TaskState::Pending);
        assert!(task.assigned_to.is_none());
        assert!(task.started_at.is_none());
        assert!(!task.state.is_terminal());
    }

    #[test]
    fn requeue_from_assigned_also_clears_assignee() {
        let mut task = Task::new("ship it");
        task.assign(AgentId::new());
        assert_eq!(task.state, TaskState::Assigned);

        task.requeue();

        assert_eq!(task.state, TaskState::Pending);
        assert!(task.assigned_to.is_none());
    }
}
