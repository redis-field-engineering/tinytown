/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Task assignment service.
//!
//! Provides operations for assigning tasks to agents.

use crate::agent::AgentId;
use crate::channel::Channel;
use crate::error::Result;
use crate::message::MessageType;
use crate::task::{Task, TaskId};
use crate::town::Town;

/// Result of an assign operation.
#[derive(Debug, Clone)]
pub struct AssignResult {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub agent_name: String,
}

/// Information about a pending task.
#[derive(Debug, Clone)]
pub struct PendingTask {
    pub task_id: TaskId,
    pub description: String,
    pub agent_id: AgentId,
    pub agent_name: String,
}

/// Service for task-related operations.
pub struct TaskService;

impl TaskService {
    /// Assign a task to an agent.
    pub async fn assign(town: &Town, agent_name: &str, description: &str) -> Result<AssignResult> {
        let handle = town.agent(agent_name).await?;

        // Create a persisted Task record for tracking
        let mut task_record = Task::new(description);
        task_record.assign(handle.id());
        let task_id = handle.assign(task_record).await?;

        Ok(AssignResult {
            task_id,
            agent_id: handle.id(),
            agent_name: agent_name.to_string(),
        })
    }

    /// List pending tasks across all agents.
    pub async fn list_pending(town: &Town) -> Result<Vec<PendingTask>> {
        let agents = town.list_agents().await;
        let channel = town.channel();
        let mut pending = Vec::new();

        for agent in agents {
            let messages = channel.peek_inbox(agent.id, 100).await.unwrap_or_default();

            for msg in messages {
                match &msg.msg_type {
                    MessageType::TaskAssign { task_id } => {
                        if let Ok(tid) = task_id.parse::<TaskId>()
                            && let Ok(Some(task)) = channel.get_task(tid).await
                        {
                            pending.push(PendingTask {
                                task_id: tid,
                                description: task.description,
                                agent_id: agent.id,
                                agent_name: agent.name.clone(),
                            });
                        }
                    }
                    MessageType::Task { description } => {
                        // Generate a temporary ID for non-persisted tasks
                        pending.push(PendingTask {
                            task_id: TaskId::new(),
                            description: description.clone(),
                            agent_id: agent.id,
                            agent_name: agent.name.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        Ok(pending)
    }

    /// Get a task by ID.
    ///
    /// Reserved for future use (e.g., task status endpoint).
    #[allow(dead_code)]
    pub async fn get(channel: &Channel, task_id: TaskId) -> Result<Option<Task>> {
        channel.get_task(task_id).await
    }
}
