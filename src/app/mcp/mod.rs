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
//! - `agent.spawn` - Spawn a new agent
//! - `agent.kill` - Kill (stop) an agent
//! - `agent.restart` - Restart a stopped agent
//! - `task.assign` - Assign a task to an agent
//! - `message.send` - Send a message to an agent
//! - `backlog.add` - Add a task to the backlog
//! - `backlog.list` - List backlog tasks
//! - `backlog.claim` - Claim a backlog task for an agent
//! - `backlog.assign_all` - Assign all backlog tasks to agents
//! - `recovery.recover_agents` - Recover orphaned agents
//! - `recovery.reclaim_tasks` - Reclaim tasks from dead agents
//!
//! ## Resources
//! - `tinytown://town/current` - Current town state
//! - `tinytown://agents` - List of all agents
//! - `tinytown://agents/{agent_name}` - Specific agent details
//! - `tinytown://backlog` - Current backlog
//! - `tinytown://tasks/{task_id}` - Specific task details
//!
//! ## Prompts
//! - `conductor.startup_context` - Context for conductor startup
//! - `agent.role_hint` - Role hints for agents

pub mod prompts;
pub mod resources;
pub mod router;
pub mod tools;

pub use router::{McpState, create_mcp_router};
