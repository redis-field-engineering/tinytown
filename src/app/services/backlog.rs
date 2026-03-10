/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Backlog management service.
//!
//! Provides operations for managing the global task backlog.

use crate::agent::AgentId;
use crate::channel::Channel;
use crate::error::Result;
use crate::message::{Message, MessageType};
use crate::task::{Task, TaskId};
use crate::town::Town;

/// Backlog item with full task details.
#[derive(Debug, Clone)]
pub struct BacklogItem {
    pub task_id: TaskId,
    pub description: String,
    pub tags: Vec<String>,
}

/// Result of adding to backlog.
#[derive(Debug, Clone)]
pub struct AddBacklogResult {
    pub task_id: TaskId,
    pub description: String,
}

/// Result of claiming from backlog.
#[derive(Debug, Clone)]
pub struct ClaimResult {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub agent_name: String,
}

/// Service for backlog operations.
pub struct BacklogService;

impl BacklogService {
    /// Add a task to the backlog.
    pub async fn add(
        channel: &Channel,
        description: &str,
        tags: Option<Vec<String>>,
    ) -> Result<AddBacklogResult> {
        let mut task = Task::new(description);
        if let Some(tag_list) = tags {
            task = task.with_tags(tag_list);
        }

        let task_id = task.id;
        channel.set_task(&task).await?;
        channel.backlog_push(task_id).await?;

        Ok(AddBacklogResult {
            task_id,
            description: description.to_string(),
        })
    }

    /// List all tasks in the backlog.
    pub async fn list(channel: &Channel) -> Result<Vec<BacklogItem>> {
        let task_ids = channel.backlog_list().await?;
        let mut items = Vec::new();

        for task_id in task_ids {
            if let Ok(Some(task)) = channel.get_task(task_id).await {
                items.push(BacklogItem {
                    task_id,
                    description: task.description,
                    tags: task.tags,
                });
            } else {
                // Task record not found - still include with empty description
                items.push(BacklogItem {
                    task_id,
                    description: "(task record not found)".to_string(),
                    tags: Vec::new(),
                });
            }
        }

        Ok(items)
    }

    /// Get the number of tasks in the backlog.
    ///
    /// Reserved for future use (e.g., backlog health checks, pagination).
    #[allow(dead_code)]
    pub async fn len(channel: &Channel) -> Result<usize> {
        channel.backlog_len().await
    }

    /// Claim a task from the backlog and assign to an agent.
    pub async fn claim(town: &Town, task_id: TaskId, agent_name: &str) -> Result<ClaimResult> {
        let channel = town.channel();

        // Remove from backlog
        let removed = channel.backlog_remove(task_id).await?;
        if !removed {
            return Err(crate::error::Error::TaskNotFound(format!(
                "Task {} not in backlog",
                task_id
            )));
        }

        // Get agent
        let agent_handle = town.agent(agent_name).await?;
        let agent_id = agent_handle.id();

        // Update task assignment (consistent with tt assign - agent will start() when working)
        if let Some(mut task) = channel.get_task(task_id).await? {
            task.assign(agent_id);
            channel.set_task(&task).await?;
        }

        // Send assignment message
        let msg = Message::new(
            AgentId::supervisor(),
            agent_id,
            MessageType::TaskAssign {
                task_id: task_id.to_string(),
            },
        );
        channel.send(&msg).await?;

        Ok(ClaimResult {
            task_id,
            agent_id,
            agent_name: agent_name.to_string(),
        })
    }

    /// Remove a task from the backlog without assigning it.
    ///
    /// This also deletes the persisted task record so all interfaces behave the
    /// same as the CLI `tt backlog remove` command.
    ///
    /// Returns true if the task was found and removed, false otherwise.
    pub async fn remove(channel: &Channel, task_id: TaskId) -> Result<bool> {
        let removed = channel.backlog_remove(task_id).await?;
        if removed {
            channel.delete_task(task_id).await?;
        }
        Ok(removed)
    }

    /// Assign all backlog tasks to an agent.
    pub async fn assign_all(town: &Town, agent_name: &str) -> Result<Vec<ClaimResult>> {
        let channel = town.channel();
        let agent_handle = town.agent(agent_name).await?;
        let agent_id = agent_handle.id();

        let mut results = Vec::new();

        while let Some(task_id) = channel.backlog_pop().await? {
            if let Some(mut task) = channel.get_task(task_id).await? {
                task.assign(agent_id);
                channel.set_task(&task).await?;

                let msg = Message::new(
                    AgentId::supervisor(),
                    agent_id,
                    MessageType::TaskAssign {
                        task_id: task_id.to_string(),
                    },
                );
                channel.send(&msg).await?;

                results.push(ClaimResult {
                    task_id,
                    agent_id,
                    agent_name: agent_name.to_string(),
                });
            }
        }

        Ok(results)
    }
}
