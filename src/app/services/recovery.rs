/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Recovery and reclaim service.
//!
//! Provides operations for recovering orphaned agents and reclaiming tasks.

use std::path::Path;

use crate::agent::{Agent, AgentId, AgentState};
use crate::channel::Channel;
use crate::error::Result;
use crate::message::MessageType;
use crate::task::{Task, TaskId};
use crate::town::Town;

/// Result of a recover operation.
#[derive(Debug, Clone)]
pub struct RecoverResult {
    pub agents_checked: usize,
    pub agents_recovered: usize,
    pub recovered_agents: Vec<Agent>,
}

/// Result of a reclaim operation.
#[derive(Debug, Clone)]
pub struct ReclaimResult {
    pub tasks_reclaimed: usize,
    pub destination: ReclaimDestination,
}

/// Where reclaimed tasks were sent.
#[derive(Debug, Clone)]
pub enum ReclaimDestination {
    Backlog,
    Agent(String),
    Listed,
}

/// Action to take on each orphan task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanAction {
    /// Just return the list without mutating anything.
    List,
    /// Reset to pending, clear assigned_to, push onto backlog.
    ToBacklog,
    /// Mark as cancelled with a reason.
    Cancel,
    /// Delete the task hash entirely.
    Delete,
}

/// Result of an orphan-task reclaim.
#[derive(Debug, Clone)]
pub struct OrphanReclaimResult {
    pub action: OrphanAction,
    pub orphans: Vec<OrphanTaskInfo>,
}

/// Minimal snapshot of an orphan task for reporting.
#[derive(Debug, Clone)]
pub struct OrphanTaskInfo {
    pub task_id: TaskId,
    pub assigned_to: AgentId,
    pub state: crate::task::TaskState,
    pub reason: OrphanReason,
}

/// Why a task was flagged as orphaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanReason {
    /// Assigned agent is not present in the agent registry at all.
    AgentMissing,
    /// Assigned agent exists but is in a terminal state (stopped/error).
    AgentTerminal,
}

/// Service for recovery operations.
pub struct RecoveryService;

impl RecoveryService {
    /// Recover orphaned agents (mark stale active agents as stopped).
    pub async fn recover(town: &Town, town_path: &Path) -> Result<RecoverResult> {
        let agents = town.list_agents().await;
        let channel = town.channel();

        let mut recovered_agents = Vec::new();
        let checked = agents.len();

        for agent in agents {
            // Match the CLI recover behavior so reboot-driven recovery and
            // service-backed recovery paths classify orphaned agents the same way.
            if !Self::is_recoverable_state(agent.state) {
                continue;
            }

            // Check if agent is stale
            let log_file = town_path.join(format!(".tt/logs/{}.log", agent.name));
            let is_stale = Self::is_agent_stale(&agent, &log_file);

            if is_stale {
                // Update agent state to stopped
                if let Some(mut agent_state) = channel.get_agent_state(agent.id).await? {
                    agent_state.state = AgentState::Stopped;
                    channel.set_agent_state(&agent_state).await?;
                }

                // Log activity
                channel
                    .log_agent_activity(agent.id, "🔄 Recovered (orphaned)")
                    .await?;

                recovered_agents.push(agent);
            }
        }

        Ok(RecoverResult {
            agents_checked: checked,
            agents_recovered: recovered_agents.len(),
            recovered_agents,
        })
    }

    /// Reclaim tasks from dead agents.
    pub async fn reclaim(
        town: &Town,
        to_backlog: bool,
        to_agent: Option<&str>,
        from_agent: Option<&str>,
    ) -> Result<ReclaimResult> {
        let agents = town.list_agents().await;
        let channel = town.channel();

        // Find dead agents
        let dead_agents: Vec<_> = agents
            .iter()
            .filter(|a| a.state.is_terminal())
            .filter(|a| from_agent.is_none() || from_agent == Some(&a.name))
            .collect();

        // Get target agent if specified
        let target_id = if let Some(target_name) = to_agent {
            Some(town.agent(target_name).await?.id())
        } else {
            None
        };

        let mut total_reclaimed = 0;

        for agent in dead_agents {
            // Drain both regular and urgent inboxes to ensure no task messages are lost
            let regular_messages = channel.drain_inbox(agent.id).await?;
            let urgent_messages = channel.receive_urgent(agent.id).await?;
            let messages: Vec<_> = urgent_messages
                .into_iter()
                .chain(regular_messages)
                .collect();

            for msg in messages {
                match &msg.msg_type {
                    MessageType::TaskAssign { task_id } => {
                        if let Ok(tid) = task_id.parse::<TaskId>() {
                            total_reclaimed +=
                                Self::handle_reclaim(channel, tid, to_backlog, target_id, &msg)
                                    .await?;
                        }
                    }
                    MessageType::Task { description } => {
                        if to_backlog {
                            let task = Task::new(description.clone());
                            let task_id = task.id;
                            channel.set_task(&task).await?;
                            channel.backlog_push(task_id).await?;
                            total_reclaimed += 1;
                        } else if let Some(target) = target_id {
                            channel.move_message_to_inbox(&msg, target).await?;
                            total_reclaimed += 1;
                        } else {
                            total_reclaimed += 1;
                        }
                    }
                    _ => {
                        // Non-task messages - move to target if specified
                        if let Some(target) = target_id {
                            channel.move_message_to_inbox(&msg, target).await?;
                        }
                    }
                }
            }
        }

        let destination = if to_backlog {
            ReclaimDestination::Backlog
        } else if let Some(name) = to_agent {
            ReclaimDestination::Agent(name.to_string())
        } else {
            ReclaimDestination::Listed
        };

        Ok(ReclaimResult {
            tasks_reclaimed: total_reclaimed,
            destination,
        })
    }

    fn is_agent_stale(agent: &Agent, log_file: &Path) -> bool {
        if log_file.exists()
            && let Ok(metadata) = std::fs::metadata(log_file)
            && let Ok(modified) = metadata.modified()
        {
            let elapsed = std::time::SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            return elapsed.as_secs() > 120;
        }
        // Fallback to heartbeat check
        let heartbeat_age = chrono::Utc::now() - agent.last_heartbeat;
        heartbeat_age.num_seconds() > 120
    }

    fn is_recoverable_state(state: AgentState) -> bool {
        matches!(
            state,
            AgentState::Working | AgentState::Starting | AgentState::Idle | AgentState::Draining
        )
    }

    async fn handle_reclaim(
        channel: &Channel,
        task_id: TaskId,
        to_backlog: bool,
        target_id: Option<AgentId>,
        msg: &crate::message::Message,
    ) -> Result<usize> {
        if to_backlog {
            channel.backlog_push(task_id).await?;
            Ok(1)
        } else if let Some(target) = target_id {
            channel.move_message_to_inbox(msg, target).await?;
            Ok(1)
        } else {
            Ok(1)
        }
    }

    /// Reclaim tasks stranded on agents that no longer exist in the registry
    /// (fully pruned) or whose registered state is terminal. Unlike
    /// [`Self::reclaim`], which drains dead agents' inboxes, this scans the
    /// task table directly so it catches tasks that were already picked up
    /// out of the inbox before the agent died.
    pub async fn reclaim_orphan_tasks(
        town: &Town,
        action: OrphanAction,
    ) -> Result<OrphanReclaimResult> {
        let channel = town.channel();
        let agents = town.list_agents().await;

        let mut agents_by_id: std::collections::HashMap<AgentId, &Agent> =
            std::collections::HashMap::with_capacity(agents.len());
        for a in &agents {
            agents_by_id.insert(a.id, a);
        }

        let tasks = channel.list_tasks().await?;
        let mut orphans = Vec::new();

        for t in tasks {
            if t.state.is_terminal() {
                continue;
            }
            let Some(assignee_id) = t.assigned_to else {
                continue;
            };

            let reason = match agents_by_id.get(&assignee_id) {
                None => Some(OrphanReason::AgentMissing),
                Some(a) if a.state.is_terminal() => Some(OrphanReason::AgentTerminal),
                Some(_) => None,
            };

            let Some(reason) = reason else { continue };

            orphans.push(OrphanTaskInfo {
                task_id: t.id,
                assigned_to: assignee_id,
                state: t.state,
                reason,
            });
        }

        for info in &orphans {
            match action {
                OrphanAction::List => {}
                OrphanAction::ToBacklog => {
                    if let Some(mut task) = channel.get_task(info.task_id).await? {
                        task.requeue();
                        channel.set_task(&task).await?;
                        channel.backlog_push(task.id).await?;
                    }
                }
                OrphanAction::Cancel => {
                    if let Some(mut task) = channel.get_task(info.task_id).await? {
                        let reason = match info.reason {
                            OrphanReason::AgentMissing => "Cancelled: assignee no longer exists",
                            OrphanReason::AgentTerminal => {
                                "Cancelled: assignee is in a terminal state"
                            }
                        };
                        task.cancel(reason);
                        channel.set_task(&task).await?;
                    }
                }
                OrphanAction::Delete => {
                    channel.delete_task(info.task_id).await?;
                }
            }
        }

        Ok(OrphanReclaimResult { action, orphans })
    }
}
