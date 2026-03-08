/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Message types for inter-agent communication.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentId;

/// Unique identifier for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Create a new random message ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Message priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Low priority - processed when idle
    Low,
    /// Normal priority - standard processing
    #[default]
    Normal,
    /// High priority - processed before normal
    High,
    /// Urgent - interrupt current work
    Urgent,
}

/// Message types for agent communication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageType {
    /// Generic semantic task message
    Task { description: String },
    /// Generic semantic query message
    Query { question: String },
    /// Generic semantic informational message
    Informational { summary: String },
    /// Semantic confirmation/acknowledgement message
    Confirmation { ack_type: ConfirmationType },
    /// Task assignment from supervisor to worker
    TaskAssign { task_id: String },
    /// Task completion notification  
    TaskDone { task_id: String, result: String },
    /// Task failure notification
    TaskFailed { task_id: String, error: String },
    /// Status request
    StatusRequest,
    /// Status response
    StatusResponse {
        state: String,
        current_task: Option<String>,
    },
    /// Heartbeat ping
    Ping,
    /// Heartbeat pong
    Pong,
    /// Shutdown request
    Shutdown,
    /// Custom message with arbitrary payload
    Custom { kind: String, payload: String },
}

/// Semantic confirmation categories.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfirmationType {
    /// Message was received
    Received,
    /// Message was acknowledged
    Acknowledged,
    /// Message expressing thanks
    Thanks,
    /// Approval confirmation
    Approved,
    /// Rejection confirmation with reason
    Rejected { reason: String },
}

impl MessageType {
    /// Returns true when this message likely requires work/action.
    #[must_use]
    pub fn is_actionable(&self) -> bool {
        matches!(
            self,
            Self::Task { .. }
                | Self::Query { .. }
                | Self::TaskAssign { .. }
                | Self::StatusRequest
                | Self::Ping
                | Self::Shutdown
                | Self::Custom { .. }
        )
    }

    /// Returns true when this message is informational or a confirmation.
    #[must_use]
    pub fn is_informational_or_confirmation(&self) -> bool {
        !self.is_actionable()
    }

    /// Produce a short summary suitable for logs and compact UI displays.
    #[must_use]
    pub fn compact_summary(&self) -> String {
        match self {
            Self::Task { description } => format!("task: {}", Self::compact_text(description, 72)),
            Self::Query { question } => format!("query: {}", Self::compact_text(question, 72)),
            Self::Informational { summary } => format!("info: {}", Self::compact_text(summary, 72)),
            Self::Confirmation { ack_type } => match ack_type {
                ConfirmationType::Received => "confirmation: received".to_string(),
                ConfirmationType::Acknowledged => "confirmation: acknowledged".to_string(),
                ConfirmationType::Thanks => "confirmation: thanks".to_string(),
                ConfirmationType::Approved => "confirmation: approved".to_string(),
                ConfirmationType::Rejected { reason } => {
                    format!(
                        "confirmation: rejected ({})",
                        Self::compact_text(reason, 56)
                    )
                }
            },
            Self::TaskAssign { task_id } => {
                format!("task_assign: {}", Self::compact_text(task_id, 40))
            }
            Self::TaskDone { task_id, .. } => {
                format!("task_done: {}", Self::compact_text(task_id, 40))
            }
            Self::TaskFailed { task_id, .. } => {
                format!("task_failed: {}", Self::compact_text(task_id, 40))
            }
            Self::StatusRequest => "status_request".to_string(),
            Self::StatusResponse {
                state,
                current_task,
            } => match current_task {
                Some(task) => format!(
                    "status_response: {} ({})",
                    Self::compact_text(state, 32),
                    Self::compact_text(task, 32)
                ),
                None => format!("status_response: {}", Self::compact_text(state, 32)),
            },
            Self::Ping => "ping".to_string(),
            Self::Pong => "pong".to_string(),
            Self::Shutdown => "shutdown".to_string(),
            Self::Custom { kind, .. } => format!("custom: {}", Self::compact_text(kind, 40)),
        }
    }

    fn compact_text(value: &str, max_chars: usize) -> String {
        let mut chars = value.chars();
        let compact: String = chars.by_ref().take(max_chars).collect();
        if chars.next().is_some() {
            format!("{compact}...")
        } else {
            compact
        }
    }
}

/// A message passed between agents via Redis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier
    pub id: MessageId,
    /// Sender agent ID  
    pub from: AgentId,
    /// Recipient agent ID
    pub to: AgentId,
    /// Message type and payload
    #[serde(flatten)]
    pub msg_type: MessageType,
    /// Priority level
    pub priority: Priority,
    /// Timestamp when created
    pub created_at: DateTime<Utc>,
    /// Optional correlation ID for request/response
    pub correlation_id: Option<MessageId>,
}

impl Message {
    /// Create a new message.
    #[must_use]
    pub fn new(from: AgentId, to: AgentId, msg_type: MessageType) -> Self {
        Self {
            id: MessageId::new(),
            from,
            to,
            msg_type,
            priority: Priority::Normal,
            created_at: Utc::now(),
            correlation_id: None,
        }
    }

    /// Set message priority.
    #[must_use]
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set correlation ID for request/response tracking.
    #[must_use]
    pub fn with_correlation(mut self, id: MessageId) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfirmationType, MessageType};

    #[test]
    fn message_type_roundtrip_new_semantic_variants() {
        let cases = vec![
            MessageType::Task {
                description: "Refactor scheduler".to_string(),
            },
            MessageType::Query {
                question: "Can you deploy this now?".to_string(),
            },
            MessageType::Informational {
                summary: "Build completed successfully".to_string(),
            },
            MessageType::Confirmation {
                ack_type: ConfirmationType::Received,
            },
            MessageType::Confirmation {
                ack_type: ConfirmationType::Rejected {
                    reason: "Missing approval from owner".to_string(),
                },
            },
        ];

        for case in cases {
            let serialized = serde_json::to_string(&case).expect("serialize message type");
            let deserialized: MessageType =
                serde_json::from_str(&serialized).expect("deserialize message type");
            assert_eq!(deserialized, case);
        }
    }

    #[test]
    fn classification_helpers_distinguish_actionable_vs_informational() {
        let actionable = [
            MessageType::Task {
                description: "Fix flake".to_string(),
            },
            MessageType::Query {
                question: "What is blocked?".to_string(),
            },
            MessageType::TaskAssign {
                task_id: "task-1".to_string(),
            },
            MessageType::StatusRequest,
            MessageType::Ping,
            MessageType::Shutdown,
            MessageType::Custom {
                kind: "needs_attention".to_string(),
                payload: "{}".to_string(),
            },
        ];

        for msg in actionable {
            assert!(msg.is_actionable());
            assert!(!msg.is_informational_or_confirmation());
        }

        let informational = [
            MessageType::Informational {
                summary: "Everything is green".to_string(),
            },
            MessageType::Confirmation {
                ack_type: ConfirmationType::Acknowledged,
            },
            MessageType::TaskDone {
                task_id: "task-2".to_string(),
                result: "done".to_string(),
            },
            MessageType::TaskFailed {
                task_id: "task-3".to_string(),
                error: "timeout".to_string(),
            },
            MessageType::StatusResponse {
                state: "idle".to_string(),
                current_task: None,
            },
            MessageType::Pong,
        ];

        for msg in informational {
            assert!(!msg.is_actionable());
            assert!(msg.is_informational_or_confirmation());
        }
    }

    #[test]
    fn compact_summary_is_short_and_readable() {
        let task = MessageType::Task {
            description:
                "Implement semantic message typing groundwork for issue nine in worker pipeline"
                    .to_string(),
        };
        assert_eq!(
            task.compact_summary(),
            "task: Implement semantic message typing groundwork for issue nine in worker pi..."
        );

        let rejected = MessageType::Confirmation {
            ack_type: ConfirmationType::Rejected {
                reason: "missing tests for classification edge cases and long payload formatting"
                    .to_string(),
            },
        };
        assert_eq!(
            rejected.compact_summary(),
            "confirmation: rejected (missing tests for classification edge cases and long pay...)"
        );

        let status = MessageType::StatusResponse {
            state: "busy".to_string(),
            current_task: Some("task-42".to_string()),
        };
        assert_eq!(status.compact_summary(), "status_response: busy (task-42)");
    }
}
