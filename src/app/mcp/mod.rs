/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! MCP (Model Context Protocol) interface for Tinytown.
//!
//! This module provides an MCP server interface that exposes Tinytown orchestration
//! operations as MCP tools, resources, and prompts. It supports both stdio and HTTP/SSE
//! transports.
//!
//! ## Tools
//! - `town.get_status` - Get town status including all agents
//! - `agent.list` - List all agents
//! - `agent.inbox` - Inspect an agent inbox
//! - `agent.spawn` - Spawn a new agent
//! - `agent.kill` - Kill (stop) an agent
//! - `agent.restart` - Restart a stopped agent
//! - `agent.prune` - Remove stopped or stale agents
//! - `task.assign` - Assign a task to an agent
//! - `task.list_pending` - List pending tasks across inboxes
//! - `message.send` - Send a message to an agent
//! - `backlog.add` - Add a task to the backlog
//! - `backlog.list` - List backlog tasks
//! - `backlog.claim` - Claim a backlog task for an agent
//! - `backlog.assign_all` - Assign all backlog tasks to agents
//! - `backlog.remove` - Remove a task from the backlog
//! - `mission.list` - List missions
//! - `mission.get_status` - Get detailed mission status
//! - `mission.status` - Alias for detailed mission status
//! - `mission.list_work` - List mission work items
//! - `mission.work_items` - Alias for mission work items
//! - `mission.list_watches` - List mission watch items
//! - `mission.watches` - Alias for mission watch items
//! - `mission.get_events` - Get recent mission events
//! - `mission.events` - Alias for recent mission events
//! - `mission.start` - Start a new mission
//! - `mission.approve` - Approve a mission work item review gate
//! - `mission.reject` - Reject a mission work item review gate
//! - `mission.pause` - Pause a mission through the dispatcher
//! - `mission.resume` - Resume a mission
//! - `mission.dispatch` - Run a single mission dispatcher tick
//! - `mission.note` - Queue an operator note for the dispatcher
//! - `mission.input` - Provide human input to a mission work item
//! - `mission.stop` - Stop a mission
//! - `recovery.recover_agents` - Recover orphaned agents
//! - `recovery.reclaim_tasks` - Reclaim tasks from dead agents
//!
//! ## Resources
//! - `tinytown://town/current` - Current town state
//! - `tinytown://agents` - List of all agents
//! - `tinytown://agents/{agent_name}` - Specific agent details
//! - `tinytown://backlog` - Current backlog
//! - `tinytown://tasks/{task_id}` - Specific task details
//! - `tinytown://missions` - All missions
//! - `tinytown://missions/{mission_id}` - Specific mission details
//!
//! ## Prompts
//! - `conductor.startup_context` - Context for conductor startup
//! - `agent.role_hint` - Role hints for agents

pub mod prompts;
pub mod resources;
pub mod router;
pub mod tools;

pub use router::{McpState, create_mcp_router};

pub(crate) fn mission_storage(state: &McpState) -> crate::mission::MissionStorage {
    let config = state.town.config();
    crate::mission::MissionStorage::new(state.town.channel().conn().clone(), &config.name)
}
