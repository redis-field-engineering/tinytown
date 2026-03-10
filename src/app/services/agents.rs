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
use crate::town::Town;

/// Result type for spawn operation.
#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub agent_id: AgentId,
    pub name: String,
    pub cli: String,
}

/// Result type for list operation.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub id: AgentId,
    pub name: String,
    pub cli: String,
    pub state: AgentState,
    pub rounds_completed: u64,
    pub tasks_completed: u64,
    pub inbox_len: usize,
    pub urgent_len: usize,
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

            result.push(AgentInfo {
                id: agent.id,
                name: agent.name.clone(),
                cli: agent.cli.clone(),
                state: agent.state,
                rounds_completed: agent.rounds_completed,
                tasks_completed: agent.tasks_completed,
                inbox_len,
                urgent_len,
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
}
