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
    /// Explicit role ID (e.g., "worker", "reviewer", "researcher")
    #[serde(default)]
    pub role_id: Option<String>,
    /// Human-facing nickname (separate from canonical name)
    #[serde(default)]
    pub nickname: Option<String>,
    /// Parent agent name or ID (for delegation / child spawning)
    #[serde(default)]
    pub parent_agent: Option<String>,
}

/// Input for agent operations by name.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentNameInput {
    /// Agent name
    pub agent: String,
}

/// Input for waiting on an agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WaitAgentInput {
    /// Agent name
    pub agent: String,
    /// Timeout in seconds (optional; waits forever if omitted)
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Input for pruning agents.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PruneAgentsInput {
    /// Remove all agents, not just stopped/error agents
    #[serde(default)]
    pub all: bool,
}

/// Input for task assignment.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignTaskInput {
    /// Agent to assign the task to
    pub agent: String,
    /// Task description
    pub description: String,
}

/// Input for completing a task.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteTaskInput {
    /// Task ID to mark as completed
    pub task_id: String,
    /// Optional result/summary message
    #[serde(default)]
    pub result: Option<String>,
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

/// Input for removing a backlog task.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveBacklogInput {
    /// Task ID to remove
    pub task_id: String,
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
                            "nickname": a.nickname,
                            "role_id": a.role_id,
                            "parent_agent_id": a.parent_agent_id.map(|id| id.to_string()),
                            "spawn_mode": format!("{}", a.spawn_mode),
                            "cli": a.cli,
                            "state": format!("{:?}", a.state),
                            "rounds_completed": a.rounds_completed,
                            "tasks_completed": a.tasks_completed,
                            "inbox_len": a.inbox_len,
                            "urgent_len": a.urgent_len,
                            "current_scope": a.current_scope
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
                                    "nickname": a.nickname,
                                    "role_id": a.role_id,
                                    "parent_agent_id": a.parent_agent_id.map(|id| id.to_string()),
                                    "spawn_mode": format!("{}", a.spawn_mode),
                                    "cli": a.cli,
                                    "state": format!("{:?}", a.state),
                                    "rounds_completed": a.rounds_completed,
                                    "tasks_completed": a.tasks_completed,
                                    "inbox_len": a.inbox_len,
                                    "urgent_len": a.urgent_len,
                                    "current_scope": a.current_scope
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

/// Create the task.list_pending tool.
pub fn task_list_pending_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("task.list_pending")
        .description("List all pending tasks across agent inboxes")
        .read_only()
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::TaskService;
                match TaskService::list_pending(&state.town).await {
                    Ok(tasks) => Ok(json_result(
                        tasks
                            .iter()
                            .map(|t| {
                                serde_json::json!({
                                    "task_id": t.task_id.to_string(),
                                    "description": t.description,
                                    "agent_id": t.agent_id.to_string(),
                                    "agent_name": t.agent_name
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

/// Create the agent.inbox tool.
pub fn agent_inbox_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.inbox")
        .description("Inspect an agent inbox")
        .read_only()
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::MessageService;
                match MessageService::get_inbox(&state.town, &input.agent).await {
                    Ok(inbox) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "agent_id": inbox.agent_id.to_string(),
                        "total_messages": inbox.total_messages,
                        "urgent_messages": inbox.urgent_messages,
                        "messages": inbox.messages.iter().map(|m| serde_json::json!({
                            "id": m.id.to_string(),
                            "from": m.from.to_string(),
                            "type": m.msg_type,
                            "summary": m.summary,
                            "urgent": m.urgent
                        })).collect::<Vec<_>>()
                    }))),
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
        .description("Spawn a new agent with optional role, nickname, and parent metadata")
        .handler(move |input: SpawnAgentInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;

                // Resolve parent agent by name or ID
                let parent_id = if let Some(ref parent) = input.parent_agent {
                    if let Ok(pid) = parent.parse::<crate::AgentId>() {
                        Some(pid)
                    } else {
                        match state.town.agent(parent).await {
                            Ok(h) => Some(h.id()),
                            Err(_) => {
                                return Ok(error_response(format!(
                                    "Parent agent '{}' not found",
                                    parent
                                )));
                            }
                        }
                    }
                } else {
                    None
                };

                match AgentService::spawn_with_metadata(
                    &state.town,
                    &input.name,
                    input.cli.as_deref(),
                    input.role_id.as_deref(),
                    input.nickname.as_deref(),
                    parent_id,
                    None,
                )
                .await
                {
                    Ok(r) => Ok(json_result(serde_json::json!({
                        "agent_id": r.agent_id.to_string(),
                        "name": r.name,
                        "cli": r.cli,
                        "role_id": r.role_id,
                        "nickname": r.nickname,
                        "parent_agent_id": r.parent_agent_id.map(|id| id.to_string())
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

/// Create the task.complete tool.
pub fn task_complete_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("task.complete")
        .description("Mark a task as completed")
        .handler(move |input: CompleteTaskInput| {
            let state = s.clone();
            async move {
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
                let channel = state.town.channel();
                match crate::TaskService::complete(channel, task_id, input.result).await {
                    Ok(Some(completed)) => Ok(json_result(serde_json::json!({
                        "task_id": task_id.to_string(),
                        "description": completed.task.description,
                        "result": completed.result,
                        "status": "completed",
                        "cleared_current_task": completed.cleared_current_task,
                        "tasks_completed": completed.tasks_completed
                    }))),
                    Ok(None) => Ok(error_response(format!("Task {} not found", task_id))),
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

/// Create the backlog.remove tool.
pub fn backlog_remove_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("backlog.remove")
        .description("Remove a backlog task without assigning it")
        .handler(move |input: RemoveBacklogInput| {
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
                match BacklogService::remove(state.town.channel(), task_id).await {
                    Ok(true) => Ok(json_result(serde_json::json!({
                        "task_id": task_id.to_string(),
                        "removed": true
                    }))),
                    Ok(false) => Ok(error_response(format!(
                        "Task {} not found in backlog",
                        task_id
                    ))),
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
        .description("Recover orphaned agents (mark stale active agents as stopped)")
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

/// Create the agent.prune tool.
pub fn agent_prune_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.prune")
        .description("Remove stopped or stale agents from the town")
        .handler(move |input: PruneAgentsInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::prune(&state.town, input.all).await {
                    Ok(removed) => Ok(json_result(serde_json::json!({
                        "removed": removed.len(),
                        "agents": removed.iter().map(|a| serde_json::json!({
                            "id": a.id.to_string(),
                            "name": a.name,
                            "state": format!("{:?}", a.state)
                        })).collect::<Vec<_>>()
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.interrupt tool.
pub fn agent_interrupt_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.interrupt")
        .description("Interrupt (pause) a running agent")
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::interrupt(state.town.channel(), handle.id()).await {
                    Ok(()) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "status": "paused"
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.wait tool.
pub fn agent_wait_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.wait")
        .description("Wait for an agent to reach a terminal state")
        .read_only()
        .handler(move |input: WaitAgentInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                let timeout = input.timeout_secs.map(std::time::Duration::from_secs);
                match AgentService::wait(state.town.channel(), handle.id(), timeout).await {
                    Ok(agent) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "state": format!("{:?}", agent.state),
                        "rounds_completed": agent.rounds_completed,
                        "tasks_completed": agent.tasks_completed
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.resume tool.
pub fn agent_resume_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.resume")
        .description("Resume a paused agent")
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::resume(state.town.channel(), handle.id()).await {
                    Ok(()) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "status": "resumed"
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.close tool.
pub fn agent_close_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.close")
        .description("Close an agent gracefully (drain current work, then stop)")
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::close(state.town.channel(), handle.id()).await {
                    Ok(()) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "status": "draining"
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the agent.list_open tool.
pub fn agent_list_open_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.list_open")
        .description("List all agents that are not in a terminal state")
        .read_only()
        .no_params_handler(move || {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::list_open(&state.town).await {
                    Ok(agents) => Ok(json_result(
                        agents
                            .iter()
                            .map(|a| {
                                serde_json::json!({
                                    "id": a.id.to_string(),
                                    "name": a.name,
                                    "nickname": a.nickname,
                                    "role_id": a.role_id,
                                    "parent_agent_id": a.parent_agent_id.map(|id| id.to_string()),
                                    "spawn_mode": format!("{}", a.spawn_mode),
                                    "cli": a.cli,
                                    "state": format!("{:?}", a.state),
                                    "rounds_completed": a.rounds_completed,
                                    "tasks_completed": a.tasks_completed,
                                    "inbox_len": a.inbox_len,
                                    "urgent_len": a.urgent_len,
                                    "current_scope": a.current_scope
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

/// Create the agent.get_result tool.
pub fn agent_get_result_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("agent.get_result")
        .description("Get the result of an agent's most recently completed task")
        .read_only()
        .handler(move |input: AgentNameInput| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                let handle = match state.town.agent(&input.agent).await {
                    Ok(h) => h,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                match AgentService::get_result(state.town.channel(), handle.id()).await {
                    Ok(Some(task)) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "task_id": task.id.to_string(),
                        "description": task.description,
                        "result": task.result,
                        "completed_at": task.completed_at.map(|t| t.to_string())
                    }))),
                    Ok(None) => Ok(json_result(serde_json::json!({
                        "agent": input.agent,
                        "result": null,
                        "message": "No completed tasks found for this agent"
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
        agent_list_open_tool(state.clone()),
        agent_inbox_tool(state.clone()),
        task_list_pending_tool(state.clone()),
        backlog_list_tool(state),
    ]
}

/// Return all mutating tools (town.write scope).
pub fn write_tools(state: Arc<McpState>) -> Vec<Tool> {
    vec![
        task_assign_tool(state.clone()),
        task_complete_tool(state.clone()),
        message_send_tool(state.clone()),
        backlog_add_tool(state.clone()),
        backlog_claim_tool(state.clone()),
        backlog_assign_all_tool(state.clone()),
        backlog_remove_tool(state),
    ]
}

/// Return all agent management tools (agent.manage scope).
pub fn agent_manage_tools(state: Arc<McpState>) -> Vec<Tool> {
    vec![
        agent_spawn_tool(state.clone()),
        agent_kill_tool(state.clone()),
        agent_interrupt_tool(state.clone()),
        agent_wait_tool(state.clone()),
        agent_resume_tool(state.clone()),
        agent_close_tool(state.clone()),
        agent_restart_tool(state.clone()),
        agent_prune_tool(state.clone()),
        agent_get_result_tool(state.clone()),
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
