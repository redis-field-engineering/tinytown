/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Message service.
//!
//! Provides operations for sending messages between agents.

use crate::agent::AgentId;
use crate::error::Result;
use crate::message::{ConfirmationType, Message, MessageId, MessageType};
use crate::town::Town;

/// Kind of message to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// A task assignment
    Task,
    /// A query requiring a response
    Query,
    /// Informational message (FYI)
    Info,
    /// Acknowledgment/confirmation
    Ack,
}

/// Result of a send operation.
#[derive(Debug, Clone)]
pub struct SendResult {
    pub message_id: MessageId,
    pub to_agent: AgentId,
    pub urgent: bool,
    pub kind: MessageKind,
}

/// Inbox summary for an agent.
#[derive(Debug, Clone)]
pub struct InboxSummary {
    pub agent_id: AgentId,
    pub agent_name: String,
    pub total_messages: usize,
    pub urgent_messages: usize,
    pub messages: Vec<MessageInfo>,
}

/// Information about a message.
#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub id: MessageId,
    pub from: AgentId,
    pub msg_type: String,
    pub summary: String,
    pub urgent: bool,
}

/// Service for messaging operations.
pub struct MessageService;

impl MessageService {
    /// Send a message to an agent with `from = AgentId::supervisor()`.
    ///
    /// This is a thin wrapper over [`Self::send_as`] preserved for existing callers
    /// that don't need to attribute the message to a specific sender.
    pub async fn send(
        town: &Town,
        to: &str,
        content: &str,
        kind: MessageKind,
        urgent: bool,
    ) -> Result<SendResult> {
        Self::send_as(town, None, to, content, kind, urgent).await
    }

    /// Send a message to an agent, optionally attributing it to a specific sender.
    ///
    /// `from` may be an agent name, UUID string, or the alias "supervisor"/"conductor".
    /// When `None`, the sender defaults to [`AgentId::supervisor`].
    pub async fn send_as(
        town: &Town,
        from: Option<&str>,
        to: &str,
        content: &str,
        kind: MessageKind,
        urgent: bool,
    ) -> Result<SendResult> {
        let from_id = match from {
            None => AgentId::supervisor(),
            Some(raw) => Self::resolve_agent_id(town, raw).await?,
        };

        let to_handle = town.agent(to).await?;
        let to_id = to_handle.id();
        let channel = town.channel();

        let msg_type = match kind {
            MessageKind::Task => MessageType::Task {
                description: content.to_string(),
            },
            MessageKind::Query => MessageType::Query {
                question: content.to_string(),
            },
            MessageKind::Info => MessageType::Informational {
                summary: content.to_string(),
            },
            MessageKind::Ack => MessageType::Confirmation {
                ack_type: Self::parse_confirmation_type(content),
            },
        };

        let msg = Message::new(from_id, to_id, msg_type);
        let message_id = msg.id;

        if urgent {
            channel.send_urgent(&msg).await?;
        } else {
            channel.send(&msg).await?;
        }

        Ok(SendResult {
            message_id,
            to_agent: to_id,
            urgent,
            kind,
        })
    }

    /// Resolve a user-supplied sender reference to an [`AgentId`].
    ///
    /// Accepts (in order): a full UUID, a registered agent name, or the
    /// "supervisor"/"conductor" aliases via [`Town::agent`].
    async fn resolve_agent_id(town: &Town, raw: &str) -> Result<AgentId> {
        if let Ok(id) = raw.parse::<AgentId>() {
            return Ok(id);
        }
        let handle = town.agent(raw).await?;
        Ok(handle.id())
    }

    /// Get inbox summary for an agent.
    pub async fn get_inbox(town: &Town, agent_name: &str) -> Result<InboxSummary> {
        let handle = town.agent(agent_name).await?;
        let agent_id = handle.id();
        let channel = town.channel();

        let total = channel.inbox_len(agent_id).await?;
        let urgent = channel.urgent_len(agent_id).await?;
        let messages = channel.peek_inbox(agent_id, 100).await.unwrap_or_default();

        let message_infos: Vec<MessageInfo> = messages
            .iter()
            .map(|m| MessageInfo {
                id: m.id,
                from: m.from,
                msg_type: Self::msg_type_name(&m.msg_type),
                summary: Self::summarize_message(&m.msg_type),
                urgent: false, // Can't tell from peek
            })
            .collect();

        Ok(InboxSummary {
            agent_id,
            agent_name: agent_name.to_string(),
            total_messages: total,
            urgent_messages: urgent,
            messages: message_infos,
        })
    }

    fn parse_confirmation_type(message: &str) -> ConfirmationType {
        let lower = message.trim().to_lowercase();
        if lower.starts_with("rejected:") {
            let reason = message
                .split_once(':')
                .map(|(_, r)| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "No reason provided".to_string());
            ConfirmationType::Rejected { reason }
        } else if lower.starts_with("received") {
            ConfirmationType::Received
        } else if lower.starts_with("approved") {
            ConfirmationType::Approved
        } else if lower.contains("thanks") || lower.contains("thank you") {
            ConfirmationType::Thanks
        } else {
            ConfirmationType::Acknowledged
        }
    }

    fn msg_type_name(msg_type: &MessageType) -> String {
        match msg_type {
            MessageType::TaskAssign { .. } => "task_assign".to_string(),
            MessageType::Task { .. } => "task".to_string(),
            MessageType::Query { .. } => "query".to_string(),
            MessageType::Informational { .. } => "info".to_string(),
            MessageType::Confirmation { .. } => "confirmation".to_string(),
            MessageType::TaskDone { .. } => "task_done".to_string(),
            MessageType::TaskFailed { .. } => "task_failed".to_string(),
            MessageType::StatusRequest => "status_request".to_string(),
            MessageType::StatusResponse { .. } => "status_response".to_string(),
            MessageType::Ping => "ping".to_string(),
            MessageType::Pong => "pong".to_string(),
            MessageType::Shutdown => "shutdown".to_string(),
            MessageType::Custom { kind, .. } => format!("custom:{}", kind),
        }
    }

    fn summarize_message(msg_type: &MessageType) -> String {
        match msg_type {
            MessageType::TaskAssign { task_id } => format!("task assignment {}", task_id),
            MessageType::Task { description } => description.clone(),
            MessageType::Query { question } => question.clone(),
            MessageType::Informational { summary } => summary.clone(),
            MessageType::Confirmation { ack_type } => format!("{:?}", ack_type),
            MessageType::TaskDone { task_id, result } => format!("done {}: {}", task_id, result),
            MessageType::TaskFailed { task_id, error } => format!("failed {}: {}", task_id, error),
            MessageType::StatusRequest => "status request".to_string(),
            MessageType::StatusResponse { state, .. } => format!("status: {}", state),
            MessageType::Ping => "ping".to_string(),
            MessageType::Pong => "pong".to_string(),
            MessageType::Shutdown => "shutdown".to_string(),
            MessageType::Custom { kind, payload } => format!("[{}] {}", kind, payload),
        }
    }
}
