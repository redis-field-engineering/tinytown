/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! MCP tool definitions for Tinytown orchestration.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_mcp::protocol::CallToolResult;
use tower_mcp::{Tool, ToolBuilder};

use super::McpState;

// ============================================================================
// Tool Input Types
// ============================================================================

/// Input for spawning an agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpawnAgentInput {
    /// Name for the new agent
    pub name: String,
    /// CLI to use (optional, defaults to town config)
    #[serde(default)]
    pub cli: Option<String>,
}

/// Input for agent operations by name.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentNameInput {
    /// Agent name
    pub agent: String,
}

/// Input for task assignment.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignTaskInput {
    /// Agent to assign the task to
    pub agent: String,
    /// Task description
    pub description: String,
}

/// Input for sending a message.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendMessageInput {
    /// Target agent name
    pub to: String,
    /// Message content
    pub message: String,
    /// Message kind: "task", "query", "info", or "ack"
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Whether this is an urgent message
    #[serde(default)]
    pub urgent: bool,
}

fn default_kind() -> String {
    "task".to_string()
}

/// Input for adding to backlog.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddBacklogInput {
    /// Task description
    pub description: String,
    /// Optional tags for the task
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Input for claiming a backlog task.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClaimBacklogInput {
    /// Task ID to claim
    pub task_id: String,
    /// Agent name to assign to
    pub agent: String,
}

/// Input for assigning all backlog tasks.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignAllBacklogInput {
    /// Agent name to assign all tasks to
    pub agent: String,
}

/// Input for recovery operations.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReclaimTasksInput {
    /// Send reclaimed tasks to backlog
    #[serde(default)]
    pub to_backlog: bool,
    /// Send reclaimed tasks to a specific agent
    #[serde(default)]
    pub to_agent: Option<String>,
    /// Only reclaim from a specific agent
    #[serde(default)]
    pub from_agent: Option<String>,
}

// ============================================================================
// Tool Result Types
// ============================================================================

/// Serializable result type for JSON responses.
#[derive(Debug, Serialize)]
struct ToolResponse<T: Serialize> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> ToolResponse<T> {
    fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
}

fn error_response(msg: String) -> CallToolResult {
    let resp: ToolResponse<()> = ToolResponse {
        success: false,
        data: None,
        error: Some(msg.clone()),
    };
    CallToolResult::text(serde_json::to_string_pretty(&resp).unwrap_or(msg))
}

fn json_result<T: Serialize>(data: T) -> CallToolResult {
    let resp = ToolResponse::ok(data);
    CallToolResult::text(serde_json::to_string_pretty(&resp).unwrap_or_default())
}

// ============================================================================
// Read-Only Tools
// ============================================================================

/// Create the town.get_status tool.
pub fn town_get_status_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("town.get_status")
        .description("Get town status including all agents")
        .read_only()
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::status(&state.town).await {
                    Ok(s) => Ok(json_result(serde_json::json!({
                        "name": s.name,
                        "root": s.root,
                        "redis_url": s.redis_url,
                        "agent_count": s.agent_count,
                        "agents": s.agents.iter().map(|a| serde_json::json!({
                            "id": a.id.to_string(),
                            "name": a.name,
                            "cli": a.cli,
                            "state": format!("{:?}", a.state),
                            "rounds_completed": a.rounds_completed,
                            "tasks_completed": a.tasks_completed,
                            "inbox_len": a.inbox_len,
                            "urgent_len": a.urgent_len
                        })).collect::<Vec<_>>()
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.list tool.
pub fn agent_list_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.list")
        .description("List all agents with their current status")
        .read_only()
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::list(&state.town).await {
                    Ok(agents) => Ok(json_result(
                        agents
                            .iter()
                            .map(|a| {
                                serde_json::json!({
                                    "id": a.id.to_string(),
                                    "name": a.name,
                                    "cli": a.cli,
                                    "state": format!("{:?}", a.state),
                                    "rounds_completed": a.rounds_completed,
                                    "tasks_completed": a.tasks_completed,
                                    "inbox_len": a.inbox_len,
                                    "urgent_len": a.urgent_len
                                })
                            })
                            .collect::<Vec<_>>(),
                    )),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the backlog.list tool.
pub fn backlog_list_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("backlog.list")
        .description("List all tasks in the backlog")
        .read_only()
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::BacklogService;
                match BacklogService::list(state.town.channel()).await {
                    Ok(items) => Ok(json_result(
                        items
                            .iter()
                            .map(|i| {
                                serde_json::json!({
                                    "task_id": i.task_id.to_string(),
                                    "description": i.description,
                                    "tags": i.tags
                                })
                            })
                            .collect::<Vec<_>>(),
                    )),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

// ============================================================================
// Mutating Tools (town.write scope)
// ============================================================================

/// Create the agent.spawn tool.
pub fn agent_spawn_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.spawn")
        .description("Spawn a new agent")
        .handler(move |input: SpawnAgentInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::spawn(&state.town, &input.name, input.cli.as_deref()).await {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "agent_id": r.agent_id.to_string(),
                        "name": r.name,
                        "cli": r.cli
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.kill tool.
pub fn agent_kill_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.kill")
        .description("Kill (stop) an agent gracefully")
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::kill(state.town.channel(), handle.id()).await {
                    Ok(()) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "status": "stopped"
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.restart tool.
pub fn agent_restart_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.restart")
        .description("Restart a stopped agent")
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::restart(state.town.channel(), handle.id()).await {
                    Ok(()) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "status": "restarted"
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the task.assign tool.
pub fn task_assign_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("task.assign")
        .description("Assign a task to an agent")
        .handler(move |input: AssignTaskInput| {
            let state = s.clone();
            async move {
                use crate::TaskService;
                match TaskService::assign(&state.town, &input.agent, &input.description).await {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "task_id": r.task_id.to_string(),
                        "agent_id": r.agent_id.to_string(),
                        "agent_name": r.agent_name
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the message.send tool.
pub fn message_send_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("message.send")
        .description("Send a message to an agent")
        .handler(move |input: SendMessageInput| {
            let state = s.clone();
            async move {
                use crate::MessageService;
                use crate::app::services::messages::MessageKind;
                let kind = match input.kind.as_str() {
                    "query" => MessageKind::Query,
                    "info" => MessageKind::Info,
                    "ack" => MessageKind::Ack,
                    _ => MessageKind::Task,
                };
                match MessageService::send(
                    &state.town,
                    &input.to,
                    &input.message,
                    kind,
                    input.urgent,
                )
                .await
                {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "message_id": r.message_id.to_string(),
                        "to_agent": r.to_agent.to_string(),
                        "urgent": r.urgent
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the backlog.add tool.
pub fn backlog_add_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("backlog.add")
        .description("Add a task to the backlog")
        .handler(move |input: AddBacklogInput| {
            let state = s.clone();
            async move {
                use crate::BacklogService;
                match BacklogService::add(state.town.channel(), &input.description, input.tags)
                    .await
                {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "task_id": r.task_id.to_string(),
                        "description": r.description
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the backlog.claim tool.
pub fn backlog_claim_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("backlog.claim")
        .description("Claim a backlog task and assign it to an agent")
        .handler(move |input: ClaimBacklogInput| {
            let state = s.clone();
            async move {
                use crate::BacklogService;
                use crate::TaskId;
                let task_id: TaskId = match input.task_id.parse() {
                    Ok(id) => id,
                    Err(_) => {
                        return Ok(error_response(format!(
                            "Invalid task ID: {}",
                            input.task_id
                        )));
                    }
                };
                match BacklogService::claim(&state.town, task_id, &input.agent).await {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "task_id": r.task_id.to_string(),
                        "agent_id": r.agent_id.to_string(),
                        "agent_name": r.agent_name
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the backlog.assign_all tool.
pub fn backlog_assign_all_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("backlog.assign_all")
        .description("Assign all backlog tasks to a specific agent")
        .handler(move |input: AssignAllBacklogInput| {
            let state = s.clone();
            async move {
                use crate::BacklogService;
                match BacklogService::assign_all(&state.town, &input.agent).await {
                    Ok(assignments) => Ok(json_result(
                        assignments
                            .iter()
                            .map(|a| {
                                serde_json::json!({
                                    "task_id": a.task_id.to_string(),
                                    "agent_id": a.agent_id.to_string(),
                                    "agent_name": a.agent_name
                                })
                            })
                            .collect::<Vec<_>>(),
                    )),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

// ============================================================================
// Recovery Tools (agent.manage scope)
// ============================================================================

/// Create the recovery.recover_agents tool.
pub fn recovery_recover_agents_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("recovery.recover_agents")
        .description("Recover orphaned agents (mark stale working agents as stopped)")
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::RecoveryService;
                match RecoveryService::recover(&state.town, state.town.root()).await {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "agents_checked": r.agents_checked,
                        "agents_recovered": r.agents_recovered,
                        "recovered_agents": r.recovered_agents.iter().map(|a| serde_json::json!({
                            "id": a.id.to_string(),
                            "name": a.name
                        })).collect::<Vec<_>>()
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the recovery.reclaim_tasks tool.
pub fn recovery_reclaim_tasks_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("recovery.reclaim_tasks")
        .description("Reclaim tasks from dead agents")
        .handler(move |input: ReclaimTasksInput| {
            let state = s.clone();
            async move {
                use crate::RecoveryService;
                match RecoveryService::reclaim(
                    &state.town,
                    input.to_backlog,
                    input.to_agent.as_deref(),
                    input.from_agent.as_deref(),
                )
                .await
                {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "tasks_reclaimed": r.tasks_reclaimed,
                        "destination": format!("{:?}", r.destination)
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

// ============================================================================
// Tool Registration
// ============================================================================

/// Return all read-only tools (town.read scope).
pub fn read_tools(state: Arc<McpState>) -> Vec<Tool> {
    vec![
        town_get_status_tool(state.clone()),
        agent_list_tool(state.clone()),
        backlog_list_tool(state),
    ]
}

/// Return all mutating tools (town.write scope).
pub fn write_tools(state: Arc<McpState>) -> Vec<Tool> {
    vec![
        task_assign_tool(state.clone()),
        message_send_tool(state.clone()),
        backlog_add_tool(state.clone()),
        backlog_claim_tool(state.clone()),
        backlog_assign_all_tool(state),
    ]
}

/// Return all agent management tools (agent.manage scope).
pub fn agent_manage_tools(state: Arc<McpState>) -> Vec<Tool> {
    vec![
        agent_spawn_tool(state.clone()),
        agent_kill_tool(state.clone()),
        agent_restart_tool(state.clone()),
        recovery_recover_agents_tool(state.clone()),
        recovery_reclaim_tasks_tool(state),
    ]
}

/// Return all tools.
pub fn all_tools(state: Arc<McpState>) -> Vec<Tool> {
    let mut tools = read_tools(state.clone());
    tools.extend(write_tools(state.clone()));
    tools.extend(agent_manage_tools(state));
    tools
}
