/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Agent management service.
//!
//! Provides operations for spawning, listing, killing, and managing agents.

use crate::agent::{Agent, AgentId, AgentState};
use crate::channel::Channel;
use crate::error::Result;
use crate::events::{EventType, TownEvent};
use crate::task::Task;
use crate::town::Town;

/// Result type for spawn operation.
#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub agent_id: AgentId,
    pub name: String,
    pub cli: String,
    pub role_id: Option<String>,
    pub nickname: Option<String>,
    pub parent_agent_id: Option<AgentId>,
}

/// Result type for list operation.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub id: AgentId,
    pub name: String,
    pub nickname: Option<String>,
    pub role_id: Option<String>,
    pub parent_agent_id: Option<AgentId>,
    pub spawn_mode: crate::agent::SpawnMode,
    pub cli: String,
    pub state: AgentState,
    pub rounds_completed: u64,
    pub tasks_completed: u64,
    pub inbox_len: usize,
    pub urgent_len: usize,
    /// Human-readable scope description for the agent's current assignment.
    pub current_scope: Option<String>,
}

/// Result type for status operation.
#[derive(Debug, Clone)]
pub struct TownStatus {
    pub name: String,
    pub root: String,
    pub redis_url: String,
    pub agent_count: usize,
    pub agents: Vec<AgentInfo>,
}

/// Service for agent-related operations.
pub struct AgentService;

impl AgentService {
    /// Spawn a new agent.
    pub async fn spawn(town: &Town, name: &str, cli: Option<&str>) -> Result<SpawnResult> {
        let config = town.config();
        let cli_name = cli
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.default_cli.clone());

        let agent = town.spawn_agent(name, &cli_name).await?;

        Ok(SpawnResult {
            agent_id: agent.id(),
            name: name.to_string(),
            cli: cli_name,
            role_id: None,
            nickname: None,
            parent_agent_id: None,
        })
    }

    /// Spawn a new agent with extended metadata (role, nickname, parent).
    pub async fn spawn_with_metadata(
        town: &Town,
        name: &str,
        cli: Option<&str>,
        role_id: Option<&str>,
        nickname: Option<&str>,
        parent_agent_id: Option<AgentId>,
        spawn_mode: Option<crate::agent::SpawnMode>,
    ) -> Result<SpawnResult> {
        let config = town.config();
        let cli_name = cli
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.default_cli.clone());

        let handle = town.spawn_agent(name, &cli_name).await?;

        // Update agent metadata in Redis — the agent must exist immediately after spawn
        let mut agent = town
            .channel()
            .get_agent_state(handle.id())
            .await?
            .ok_or_else(|| {
                crate::Error::AgentNotFound(format!(
                    "Agent {} not found in Redis immediately after spawn",
                    handle.id()
                ))
            })?;
        if let Some(role) = role_id {
            agent.role_id = Some(role.to_string());
        }
        if let Some(nick) = nickname {
            agent.nickname = Some(nick.to_string());
        }
        agent.parent_agent_id = parent_agent_id;
        if let Some(mode) = spawn_mode {
            agent.spawn_mode = mode;
        }
        town.channel().set_agent_state(&agent).await?;

        Ok(SpawnResult {
            agent_id: handle.id(),
            name: name.to_string(),
            cli: cli_name,
            role_id: role_id.map(|s| s.to_string()),
            nickname: nickname.map(|s| s.to_string()),
            parent_agent_id,
        })
    }

    /// List all agents with their current status.
    pub async fn list(town: &Town) -> Result<Vec<AgentInfo>> {
        let agents = town.list_agents().await;
        let channel = town.channel();

        let mut result = Vec::new();
        for agent in agents {
            let inbox_len = channel.inbox_len(agent.id).await.unwrap_or(0);
            let urgent_len = channel.urgent_len(agent.id).await.unwrap_or(0);

            // Derive current_scope from the agent's current task description
            let current_scope = if let Some(task_id) = agent.current_task {
                channel
                    .get_task(task_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.description)
            } else {
                None
            };

            result.push(AgentInfo {
                id: agent.id,
                name: agent.name.clone(),
                nickname: agent.nickname.clone(),
                role_id: agent.role_id.clone(),
                parent_agent_id: agent.parent_agent_id,
                spawn_mode: agent.spawn_mode,
                cli: agent.cli.clone(),
                state: agent.state,
                rounds_completed: agent.rounds_completed,
                tasks_completed: agent.tasks_completed,
                inbox_len,
                urgent_len,
                current_scope,
            });
        }

        Ok(result)
    }

    /// Get town status including all agents.
    pub async fn status(town: &Town) -> Result<TownStatus> {
        let config = town.config();
        let agents = Self::list(town).await?;

        Ok(TownStatus {
            name: config.name.clone(),
            root: town.root().display().to_string(),
            redis_url: config.redis_url_redacted(),
            agent_count: agents.len(),
            agents,
        })
    }

    /// Kill (stop) an agent gracefully.
    pub async fn kill(channel: &Channel, agent_id: AgentId) -> Result<()> {
        // Request the agent to stop
        channel.request_stop(agent_id).await?;

        // Update state to show it's stopping
        if let Some(mut agent_state) = channel.get_agent_state(agent_id).await? {
            agent_state.state = AgentState::Stopped;
            channel.set_agent_state(&agent_state).await?;
        }

        // Log activity
        channel
            .log_agent_activity(agent_id, "🛑 Stop requested")
            .await?;

        Ok(())
    }

    /// Request all non-terminal agents in a town to stop gracefully.
    pub async fn stop_all(town: &Town) -> Result<Vec<Agent>> {
        let agents = town.list_agents().await;
        let channel = town.channel();
        let mut requested = Vec::new();

        for agent in agents {
            if agent.state.is_terminal() {
                continue;
            }

            Self::kill(channel, agent.id).await?;
            requested.push(agent);
        }

        Ok(requested)
    }

    /// Restart a stopped agent.
    pub async fn restart(channel: &Channel, agent_id: AgentId) -> Result<()> {
        if let Some(mut agent_state) = channel.get_agent_state(agent_id).await? {
            agent_state.state = AgentState::Idle;
            agent_state.rounds_completed = 0;
            agent_state.last_heartbeat = chrono::Utc::now();
            channel.set_agent_state(&agent_state).await?;
        }

        // Clear any stop flags
        channel.clear_stop(agent_id).await?;

        Ok(())
    }

    /// Prune stopped/stale agents.
    pub async fn prune(town: &Town, all: bool) -> Result<Vec<Agent>> {
        let agents = town.list_agents().await;
        let channel = town.channel();

        let mut removed = Vec::new();
        for agent in agents {
            let should_remove =
                all || matches!(agent.state, AgentState::Stopped | AgentState::Error);
            if should_remove {
                channel.delete_agent(agent.id).await?;
                removed.push(agent);
            }
        }

        Ok(removed)
    }

    /// Interrupt (pause) a running agent.
    ///
    /// Sets the agent's state to Paused. The agent will not pick up new work
    /// until resumed.
    pub async fn interrupt(channel: &Channel, agent_id: AgentId) -> Result<()> {
        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
            if agent.state.is_terminal() {
                return Err(crate::Error::AgentNotFound(format!(
                    "Agent {} is in terminal state {:?} and cannot be interrupted",
                    agent_id, agent.state
                )));
            }
            let old_state = format!("{:?}", agent.state);
            agent.state = AgentState::Paused;
            channel.set_agent_state(&agent).await?;
            channel
                .log_agent_activity(agent_id, "⏸️ Interrupted (paused)")
                .await?;
            channel
                .emit_event(
                    &TownEvent::new(EventType::AgentInterrupted, "Agent interrupted (paused)")
                        .with_agent(agent_id)
                        .with_transition(old_state, "Paused"),
                )
                .await;
        } else {
            return Err(crate::Error::AgentNotFound(agent_id.to_string()));
        }
        Ok(())
    }

    /// Wait for an agent to reach a terminal state.
    ///
    /// Polls the agent state at 1-second intervals until the agent is in a
    /// terminal state or the timeout expires. Returns the final agent state.
    pub async fn wait(
        channel: &Channel,
        agent_id: AgentId,
        timeout: Option<std::time::Duration>,
    ) -> Result<Agent> {
        let deadline = timeout.map(|d| std::time::Instant::now() + d);

        loop {
            if let Some(agent) = channel.get_agent_state(agent_id).await? {
                if agent.state.is_terminal() {
                    return Ok(agent);
                }
            } else {
                return Err(crate::Error::AgentNotFound(agent_id.to_string()));
            }

            if let Some(dl) = deadline
                && std::time::Instant::now() >= dl
            {
                // Return current state on timeout
                return channel
                    .get_agent_state(agent_id)
                    .await?
                    .ok_or_else(|| crate::Error::AgentNotFound(agent_id.to_string()));
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    /// Resume a paused agent.
    ///
    /// Sets the agent's state back to Idle so it can accept new work.
    pub async fn resume(channel: &Channel, agent_id: AgentId) -> Result<()> {
        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
            if agent.state != AgentState::Paused {
                return Err(crate::Error::Config(format!(
                    "Agent {} is {:?}, not Paused — cannot resume",
                    agent_id, agent.state
                )));
            }
            agent.state = AgentState::Idle;
            channel.set_agent_state(&agent).await?;
            channel.clear_stop(agent_id).await?;
            channel.log_agent_activity(agent_id, "▶️ Resumed").await?;
            channel
                .emit_event(
                    &TownEvent::new(EventType::AgentResumed, "Agent resumed")
                        .with_agent(agent_id)
                        .with_transition("Paused", "Idle"),
                )
                .await;
        } else {
            return Err(crate::Error::AgentNotFound(agent_id.to_string()));
        }
        Ok(())
    }

    /// Close an agent: drain its current work and then stop.
    ///
    /// Sets the agent to Draining state, which means it will finish its
    /// current task but won't accept new work. Then requests a stop.
    pub async fn close(channel: &Channel, agent_id: AgentId) -> Result<()> {
        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
            if agent.state.is_terminal() {
                return Err(crate::Error::AgentNotFound(format!(
                    "Agent {} is already in terminal state {:?}",
                    agent_id, agent.state
                )));
            }
            let old_state = format!("{:?}", agent.state);
            agent.state = AgentState::Draining;
            channel.set_agent_state(&agent).await?;
            channel.request_stop(agent_id).await?;
            channel
                .log_agent_activity(agent_id, "🔻 Closing (draining then stop)")
                .await?;
            channel
                .emit_event(
                    &TownEvent::new(
                        EventType::AgentStopped,
                        "Agent closing (draining then stop)",
                    )
                    .with_agent(agent_id)
                    .with_transition(old_state, "Draining"),
                )
                .await;
        } else {
            return Err(crate::Error::AgentNotFound(agent_id.to_string()));
        }
        Ok(())
    }

    /// List all agents that are not in a terminal state.
    pub async fn list_open(town: &Town) -> Result<Vec<AgentInfo>> {
        let all = Self::list(town).await?;
        Ok(all.into_iter().filter(|a| !a.state.is_terminal()).collect())
    }

    /// Get the result of an agent's most recently completed task.
    pub async fn get_result(channel: &Channel, agent_id: AgentId) -> Result<Option<Task>> {
        let tasks = channel.list_tasks().await?;
        // Find the most recently completed task assigned to this agent
        let result = tasks
            .into_iter()
            .filter(|t| {
                t.assigned_to == Some(agent_id) && t.state == crate::task::TaskState::Completed
            })
            .max_by_key(|t| t.completed_at);
        Ok(result)
    }
}
