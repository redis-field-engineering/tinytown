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

use super::{McpState, mission_storage};

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

fn default_max_parallel() -> u32 {
    2
}

fn default_event_count() -> isize {
    20
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

/// Input for listing missions.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionListInput {
    /// Include completed and failed missions
    #[serde(default)]
    pub all: bool,
    /// Optional mission state filter
    #[serde(default)]
    pub status: Option<String>,
}

/// Input for addressing a mission by ID.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionIdInput {
    /// Mission ID
    pub mission_id: String,
}

/// Input for starting a mission.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionStartInput {
    /// GitHub issue numbers or refs (e.g. "23" or "owner/repo#23")
    #[serde(default)]
    pub issues: Vec<String>,
    /// Document paths to include as objectives
    #[serde(default)]
    pub docs: Vec<String>,
    /// Maximum parallel work items
    #[serde(default = "default_max_parallel")]
    pub max_parallel: u32,
    /// Disable reviewer requirement
    #[serde(default)]
    pub no_reviewer: bool,
}

/// Input for getting detailed mission status.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionStatusInput {
    /// Mission ID
    pub mission_id: String,
    /// Include work item details
    #[serde(default)]
    pub include_work: bool,
    /// Include watch item details
    #[serde(default)]
    pub include_watch: bool,
    /// Include recent events
    #[serde(default)]
    pub include_events: bool,
    /// Include dispatcher/control details
    #[serde(default)]
    pub include_dispatcher: bool,
}

/// Input for running a dispatcher tick.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionDispatchInput {
    /// Optional mission ID filter
    #[serde(default)]
    pub mission_id: Option<String>,
}

/// Input for sending an operator note to a mission dispatcher.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionNoteInput {
    /// Mission ID
    pub mission_id: String,
    /// Note or directive body
    pub message: String,
    /// Sender label stored with the note
    #[serde(default)]
    pub sender: Option<String>,
}

/// Input for stopping a mission.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionStopInput {
    /// Mission ID
    pub mission_id: String,
    /// Force stop without graceful cleanup
    #[serde(default)]
    pub force: bool,
}

/// Input for listing mission work items.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionWorkListInput {
    /// Mission ID
    pub mission_id: String,
    /// Optional status filter
    #[serde(default)]
    pub status: Option<String>,
}

/// Input for listing mission watch items.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionWatchListInput {
    /// Mission ID
    pub mission_id: String,
    /// Optional status filter
    #[serde(default)]
    pub status: Option<String>,
}

/// Input for retrieving mission events.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionEventsInput {
    /// Mission ID
    pub mission_id: String,
    /// Number of recent events to return
    #[serde(default = "default_event_count")]
    pub count: isize,
}

/// Input for approving a mission work item.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionApproveInput {
    /// Mission ID
    pub mission_id: String,
    /// Optional work item ID. If omitted, Tinytown auto-selects the single reviewable blocked item.
    #[serde(default)]
    pub work_item_id: Option<String>,
}

/// Input for rejecting a mission work item.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionRejectInput {
    /// Mission ID
    pub mission_id: String,
    /// Work item ID
    pub work_item_id: String,
    /// Rejection reason / requested changes
    pub reason: String,
}

/// Input for providing human input to a mission work item.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissionInputResponseInput {
    /// Mission ID
    pub mission_id: String,
    /// Work item ID
    pub work_item_id: String,
    /// Human response or operator guidance
    pub response: String,
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

fn parse_mission_id(value: &str) -> std::result::Result<crate::mission::MissionId, String> {
    value
        .parse()
        .map_err(|_| format!("Invalid mission ID: {value}"))
}

fn parse_work_item_id(value: &str) -> std::result::Result<crate::mission::WorkItemId, String> {
    value
        .parse()
        .map_err(|_| format!("Invalid work item ID: {value}"))
}

fn parse_mission_state(value: &str) -> std::result::Result<crate::mission::MissionState, String> {
    match value {
        "planning" => Ok(crate::mission::MissionState::Planning),
        "running" => Ok(crate::mission::MissionState::Running),
        "blocked" => Ok(crate::mission::MissionState::Blocked),
        "completed" => Ok(crate::mission::MissionState::Completed),
        "failed" => Ok(crate::mission::MissionState::Failed),
        _ => Err(format!("Invalid mission status: {value}")),
    }
}

fn parse_work_status(value: &str) -> std::result::Result<crate::mission::WorkStatus, String> {
    match value {
        "pending" => Ok(crate::mission::WorkStatus::Pending),
        "ready" => Ok(crate::mission::WorkStatus::Ready),
        "assigned" => Ok(crate::mission::WorkStatus::Assigned),
        "running" => Ok(crate::mission::WorkStatus::Running),
        "blocked" => Ok(crate::mission::WorkStatus::Blocked),
        "done" => Ok(crate::mission::WorkStatus::Done),
        _ => Err(format!("Invalid work status: {value}")),
    }
}

fn parse_watch_status(value: &str) -> std::result::Result<crate::mission::WatchStatus, String> {
    match value {
        "active" => Ok(crate::mission::WatchStatus::Active),
        "snoozed" => Ok(crate::mission::WatchStatus::Snoozed),
        "done" => Ok(crate::mission::WatchStatus::Done),
        _ => Err(format!("Invalid watch status: {value}")),
    }
}

fn work_item_completion_label(value: crate::mission::WorkItemCompletion) -> &'static str {
    match value {
        crate::mission::WorkItemCompletion::Completed => "completed",
        crate::mission::WorkItemCompletion::WaitingForReview => "waiting_for_review",
        crate::mission::WorkItemCompletion::WaitingForExternal => "waiting_for_external",
        crate::mission::WorkItemCompletion::MissionNotFound => "mission_not_found",
        crate::mission::WorkItemCompletion::WorkItemNotFound => "work_item_not_found",
        crate::mission::WorkItemCompletion::ReviewerApprovalRequired => {
            "reviewer_approval_required"
        }
    }
}

fn work_item_completion_error(value: crate::mission::WorkItemCompletion) -> Option<&'static str> {
    match value {
        crate::mission::WorkItemCompletion::MissionNotFound => Some("Mission not found"),
        crate::mission::WorkItemCompletion::WorkItemNotFound => Some("Work item not found"),
        crate::mission::WorkItemCompletion::ReviewerApprovalRequired => {
            Some("Reviewer approval is still required")
        }
        crate::mission::WorkItemCompletion::Completed
        | crate::mission::WorkItemCompletion::WaitingForReview
        | crate::mission::WorkItemCompletion::WaitingForExternal => None,
    }
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

/// Create the mission.list tool.
pub fn mission_list_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.list")
        .description("List missions tracked by Tinytown, optionally filtered by mission state")
        .read_only()
        .handler(move |input: MissionListInput| {
            let state = s.clone();
            async move {
                let storage = mission_storage(&state);
                let mut missions = if input.all || input.status.is_some() {
                    match storage.list_all_missions().await {
                        Ok(missions) => missions,
                        Err(e) => return Ok(error_response(e.to_string())),
                    }
                } else {
                    let active_ids = match storage.list_active().await {
                        Ok(ids) => ids,
                        Err(e) => return Ok(error_response(e.to_string())),
                    };

                    let mut missions = Vec::new();
                    for mission_id in active_ids {
                        match storage.get_mission(mission_id).await {
                            Ok(Some(mission)) => missions.push(mission),
                            Ok(None) => {}
                            Err(e) => return Ok(error_response(e.to_string())),
                        }
                    }
                    missions
                };

                if let Some(status) = input.status.as_deref() {
                    let status = match parse_mission_state(status) {
                        Ok(status) => status,
                        Err(msg) => return Ok(error_response(msg)),
                    };
                    missions.retain(|mission| mission.state == status);
                }

                Ok(json_result(serde_json::json!({
                    "missions": missions,
                    "count": missions.len()
                })))
            }
        })
        .build()
}

/// Create the mission.get_status tool.
pub fn mission_get_status_tool(state: Arc<McpState>) -> Tool {
    build_mission_status_tool(
        state,
        "mission.get_status",
        "Get detailed mission status, optionally including work, watches, events, and dispatcher details",
        false,
    )
}

fn build_mission_status_tool(
    state: Arc<McpState>,
    name: &'static str,
    description: &'static str,
    include_work_and_watch_by_default: bool,
) -> Tool {
    let s = state.clone();
    ToolBuilder::new(name)
        .description(description)
        .read_only()
        .handler(move |input: MissionStatusInput| {
            let state = s.clone();
            async move {
                let effective = if include_work_and_watch_by_default {
                    MissionStatusInput {
                        mission_id: input.mission_id,
                        include_work: true,
                        include_watch: true,
                        include_events: input.include_events,
                        include_dispatcher: true,
                    }
                } else {
                    input
                };

                let mission_id = match parse_mission_id(&effective.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                let Some(mission) = (match storage.get_mission(mission_id).await {
                    Ok(mission) => mission,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Mission {} not found",
                        effective.mission_id
                    )));
                };

                let work_items = if effective.include_work {
                    Some(match storage.list_work_items(mission_id).await {
                        Ok(items) => items,
                        Err(e) => return Ok(error_response(e.to_string())),
                    })
                } else {
                    None
                };
                let watch_items = if effective.include_watch {
                    Some(match storage.list_watch_items(mission_id).await {
                        Ok(items) => items,
                        Err(e) => return Ok(error_response(e.to_string())),
                    })
                } else {
                    None
                };
                let work_item_count = match &work_items {
                    Some(items) => items.len(),
                    None => match storage.count_work_items(mission_id).await {
                        Ok(count) => count,
                        Err(e) => return Ok(error_response(e.to_string())),
                    },
                };
                let watch_item_count = match &watch_items {
                    Some(items) => items.len(),
                    None => match storage.count_watch_items(mission_id).await {
                        Ok(count) => count,
                        Err(e) => return Ok(error_response(e.to_string())),
                    },
                };

                let events = if effective.include_events {
                    match storage.get_events(mission_id, 10).await {
                        Ok(events) => Some(events),
                        Err(e) => return Ok(error_response(e.to_string())),
                    }
                } else {
                    None
                };

                let dispatcher = if effective.include_dispatcher {
                    let control_messages = match storage.list_control_messages(mission_id).await {
                        Ok(messages) => messages,
                        Err(e) => return Ok(error_response(e.to_string())),
                    };
                    let pending_control_messages: Vec<_> = control_messages
                        .iter()
                        .filter(|message| message.is_pending())
                        .cloned()
                        .collect();
                    Some(serde_json::json!({
                        "last_tick_at": mission.dispatcher_last_tick_at,
                        "last_progress_at": mission.dispatcher_last_progress_at,
                        "last_help_request_at": mission.dispatcher_last_help_request_at,
                        "last_help_request_reason": mission.dispatcher_last_help_request_reason,
                        "control_messages": control_messages,
                        "pending_control_messages": pending_control_messages
                    }))
                } else {
                    None
                };

                Ok(json_result(serde_json::json!({
                    "mission": mission,
                    "work_item_count": work_item_count,
                    "watch_item_count": watch_item_count,
                    "work_items": work_items,
                    "watch_items": watch_items,
                    "events": events,
                    "dispatcher": dispatcher
                })))
            }
        })
        .build()
}

/// Create the mission.list_work tool.
pub fn mission_list_work_tool(state: Arc<McpState>) -> Tool {
    build_mission_work_list_tool(state, "mission.list_work")
}

fn build_mission_work_list_tool(state: Arc<McpState>, name: &'static str) -> Tool {
    let s = state.clone();
    ToolBuilder::new(name)
        .description("List work items for a mission")
        .read_only()
        .handler(move |input: MissionWorkListInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                let mut items = match storage.list_work_items(mission_id).await {
                    Ok(items) => items,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                if let Some(status) = input.status.as_deref() {
                    let status = match parse_work_status(status) {
                        Ok(status) => status,
                        Err(msg) => return Ok(error_response(msg)),
                    };
                    items.retain(|item| item.status == status);
                }

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "count": items.len(),
                    "items": items
                })))
            }
        })
        .build()
}

/// Create the mission.list_watches tool.
pub fn mission_list_watches_tool(state: Arc<McpState>) -> Tool {
    build_mission_watch_list_tool(state, "mission.list_watches")
}

fn build_mission_watch_list_tool(state: Arc<McpState>, name: &'static str) -> Tool {
    let s = state.clone();
    ToolBuilder::new(name)
        .description("List watch items for a mission")
        .read_only()
        .handler(move |input: MissionWatchListInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                let mut items = match storage.list_watch_items(mission_id).await {
                    Ok(items) => items,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                if let Some(status) = input.status.as_deref() {
                    let status = match parse_watch_status(status) {
                        Ok(status) => status,
                        Err(msg) => return Ok(error_response(msg)),
                    };
                    items.retain(|item| item.status == status);
                }

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "count": items.len(),
                    "items": items
                })))
            }
        })
        .build()
}

/// Create the mission.get_events tool.
pub fn mission_get_events_tool(state: Arc<McpState>) -> Tool {
    build_mission_events_tool(state, "mission.get_events")
}

fn build_mission_events_tool(state: Arc<McpState>, name: &'static str) -> Tool {
    let s = state.clone();
    ToolBuilder::new(name)
        .description("Get recent mission activity events")
        .read_only()
        .handler(move |input: MissionEventsInput| {
            let state = s.clone();
            async move {
                if input.count <= 0 {
                    return Ok(error_response(
                        "Event count must be greater than zero".to_string(),
                    ));
                }

                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                match storage.get_events(mission_id, input.count).await {
                    Ok(events) => Ok(json_result(serde_json::json!({
                        "mission_id": mission_id,
                        "count": events.len(),
                        "events": events
                    }))),
                    Err(e) => Ok(error_response(e.to_string())),
                }
            }
        })
        .build()
}

/// Create the mission.status tool.
pub fn mission_status_tool(state: Arc<McpState>) -> Tool {
    build_mission_status_tool(
        state,
        "mission.status",
        "Get detailed mission status including work items, watches, and dispatcher details",
        true,
    )
}

/// Create the mission.work_items tool.
pub fn mission_work_items_tool(state: Arc<McpState>) -> Tool {
    build_mission_work_list_tool(state, "mission.work_items")
}

/// Create the mission.watches tool.
pub fn mission_watches_tool(state: Arc<McpState>) -> Tool {
    build_mission_watch_list_tool(state, "mission.watches")
}

/// Create the mission.events tool.
pub fn mission_events_tool(state: Arc<McpState>) -> Tool {
    build_mission_events_tool(state, "mission.events")
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

/// Create the mission.start tool.
pub fn mission_start_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.start")
        .description("Start a new mission from GitHub issues and/or document objectives")
        .handler(move |input: MissionStartInput| {
            let state = s.clone();
            async move {
                if input.issues.is_empty() && input.docs.is_empty() {
                    return Ok(error_response(
                        "At least one issue or doc objective is required".to_string(),
                    ));
                }

                let config = state.town.config();
                let mut objectives = Vec::new();
                let mut invalid_issues = Vec::new();

                for issue in &input.issues {
                    if let Some(objective) =
                        crate::mission::parse_issue_ref(issue, &config.name, state.town.root())
                    {
                        objectives.push(objective);
                    } else {
                        invalid_issues.push(issue.clone());
                    }
                }

                for doc in &input.docs {
                    objectives.push(crate::mission::ObjectiveRef::Doc { path: doc.clone() });
                }

                if objectives.is_empty() {
                    return Ok(error_response(
                        "No valid mission objectives found".to_string(),
                    ));
                }

                let policy = crate::mission::MissionPolicy {
                    max_parallel_items: input.max_parallel,
                    reviewer_required: !input.no_reviewer,
                    ..Default::default()
                };

                let mut mission =
                    crate::mission::MissionRun::new(objectives.clone()).with_policy(policy);
                mission.start();

                let storage = mission_storage(&state);
                if let Err(e) = storage.save_mission(&mission).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage.add_active(mission.id).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage
                    .log_event(mission.id, "Mission started via MCP")
                    .await
                {
                    return Ok(error_response(e.to_string()));
                }

                let work_items = match crate::mission::build_mission_work_items(
                    state.town.root(),
                    mission.id,
                    &objectives,
                ) {
                    Ok(items) => items,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                for item in &work_items {
                    if let Err(e) = storage.save_work_item(item).await {
                        return Ok(error_response(e.to_string()));
                    }
                }
                if let Err(e) = storage
                    .log_event(
                        mission.id,
                        &format!(
                            "Bootstrapped {} work item(s) from mission objectives",
                            work_items.len()
                        ),
                    )
                    .await
                {
                    return Ok(error_response(e.to_string()));
                }

                let scheduler = crate::mission::MissionScheduler::with_defaults(
                    storage.clone(),
                    state.town.channel().clone(),
                );
                let tick_result = match scheduler.tick().await {
                    Ok(result) => result,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                Ok(json_result(serde_json::json!({
                    "mission": mission,
                    "invalid_issues": invalid_issues,
                    "objective_count": objectives.len(),
                    "work_item_count": work_items.len(),
                    "scheduler_bootstrap": {
                        "total_promoted": tick_result.total_promoted,
                        "total_assigned": tick_result.total_assigned,
                        "missions_completed": tick_result.missions_completed,
                    }
                })))
            }
        })
        .build()
}

/// Create the mission.approve tool.
pub fn mission_approve_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.approve")
        .description("Approve a reviewer gate for a mission work item")
        .handler(move |input: MissionApproveInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                let Some(mission) = (match storage.get_mission(mission_id).await {
                    Ok(mission) => mission,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Mission {} not found",
                        input.mission_id
                    )));
                };

                let work_item_id = if let Some(work_item_id) = input.work_item_id.as_deref() {
                    match parse_work_item_id(work_item_id) {
                        Ok(id) => id,
                        Err(msg) => return Ok(error_response(msg)),
                    }
                } else {
                    let work_items = match storage.list_work_items(mission_id).await {
                        Ok(items) => items,
                        Err(e) => return Ok(error_response(e.to_string())),
                    };
                    let mut candidates = work_items
                        .into_iter()
                        .filter(|item| {
                            item.status == crate::mission::WorkStatus::Blocked
                                && !item.reviewer_approved
                                && !item.artifact_refs.is_empty()
                                && mission.policy.reviewer_required
                                && matches!(
                                    item.kind,
                                    crate::mission::WorkKind::Implement
                                        | crate::mission::WorkKind::Test
                                )
                        })
                        .map(|item| item.id);
                    let Some(candidate) = candidates.next() else {
                        return Ok(error_response(
                            "No reviewable blocked work item found; provide work_item_id explicitly"
                                .to_string(),
                        ));
                    };
                    if candidates.next().is_some() {
                        return Ok(error_response(
                            "Multiple reviewable blocked work items found; provide work_item_id explicitly"
                                .to_string(),
                        ));
                    }
                    candidate
                };

                let scheduler = crate::mission::MissionScheduler::with_defaults(
                    storage.clone(),
                    state.town.channel().clone(),
                );
                let completion = match scheduler
                    .approve_submission(
                        mission_id,
                        work_item_id,
                        vec!["mcp:mission.approve".to_string()],
                    )
                    .await
                {
                    Ok(completion) => completion,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                if let Some(message) = work_item_completion_error(completion) {
                    return Ok(error_response(message.to_string()));
                }
                let tick_result = match scheduler.tick().await {
                    Ok(result) => result,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "status": work_item_completion_label(completion),
                    "scheduler_result": {
                        "total_promoted": tick_result.total_promoted,
                        "total_assigned": tick_result.total_assigned,
                        "missions_completed": tick_result.missions_completed,
                    }
                })))
            }
        })
        .build()
}

/// Create the mission.reject tool.
pub fn mission_reject_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.reject")
        .description("Reject a mission work item review gate and request changes")
        .handler(move |input: MissionRejectInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let work_item_id = match parse_work_item_id(&input.work_item_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                let scheduler = crate::mission::MissionScheduler::with_defaults(
                    storage.clone(),
                    state.town.channel().clone(),
                );
                let completion = match scheduler
                    .request_changes(mission_id, work_item_id, &input.reason)
                    .await
                {
                    Ok(completion) => completion,
                    Err(e) => return Ok(error_response(e.to_string())),
                };
                if let Some(message) = work_item_completion_error(completion) {
                    return Ok(error_response(message.to_string()));
                }
                let tick_result = match scheduler.tick().await {
                    Ok(result) => result,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "status": work_item_completion_label(completion),
                    "reason": input.reason,
                    "scheduler_result": {
                        "total_promoted": tick_result.total_promoted,
                        "total_assigned": tick_result.total_assigned,
                        "missions_completed": tick_result.missions_completed,
                    }
                })))
            }
        })
        .build()
}

/// Create the mission.pause tool.
pub fn mission_pause_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.pause")
        .description("Pause a running mission through the dispatcher control channel")
        .handler(move |input: MissionIdInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                let Some(mission) = (match storage.get_mission(mission_id).await {
                    Ok(mission) => mission,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Mission {} not found",
                        input.mission_id
                    )));
                };
                if mission.state.is_terminal() {
                    return Ok(error_response(format!(
                        "Mission {} is terminal and cannot be paused",
                        input.mission_id
                    )));
                }
                if !mission.state.can_pause() {
                    return Ok(error_response(format!(
                        "Mission {} is not running and cannot be paused",
                        input.mission_id
                    )));
                }

                let note = crate::mission::MissionControlMessage::new(
                    mission_id,
                    "mcp",
                    "pause requested via mission.pause",
                );
                if let Err(e) = storage.save_control_message(&note).await {
                    return Ok(error_response(e.to_string()));
                }
                let dispatcher = crate::mission::MissionDispatcher::new(
                    storage.clone(),
                    state.town.channel().clone(),
                    crate::mission::GhCliGitHubClient,
                    crate::mission::DispatcherConfig::default(),
                );
                if let Err(e) = dispatcher.tick(Some(mission_id)).await {
                    return Ok(error_response(e.to_string()));
                }

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "directive": "pause",
                    "status": "queued_and_processed"
                })))
            }
        })
        .build()
}

/// Create the mission.resume tool.
pub fn mission_resume_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.resume")
        .description("Resume a stopped or blocked mission through the dispatcher control channel")
        .handler(move |input: MissionIdInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                let Some(mission) = (match storage.get_mission(mission_id).await {
                    Ok(mission) => mission,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Mission {} not found",
                        input.mission_id
                    )));
                };

                if mission.state == crate::mission::MissionState::Running {
                    return Ok(error_response(format!(
                        "Mission {} is already running",
                        input.mission_id
                    )));
                }
                if mission.state == crate::mission::MissionState::Completed {
                    return Ok(error_response(format!(
                        "Mission {} is already completed",
                        input.mission_id
                    )));
                }
                if mission.state == crate::mission::MissionState::Failed {
                    return Ok(error_response(format!(
                        "Mission {} has failed and cannot be resumed",
                        input.mission_id
                    )));
                }
                if !mission.state.can_resume() {
                    return Ok(error_response(format!(
                        "Mission {} is not blocked and cannot be resumed",
                        input.mission_id
                    )));
                }

                let note = crate::mission::MissionControlMessage::new(
                    mission_id,
                    "mcp",
                    "resume requested via mission.resume",
                );
                if let Err(e) = storage.save_control_message(&note).await {
                    return Ok(error_response(e.to_string()));
                }
                let dispatcher = crate::mission::MissionDispatcher::new(
                    storage.clone(),
                    state.town.channel().clone(),
                    crate::mission::GhCliGitHubClient,
                    crate::mission::DispatcherConfig::default(),
                );
                if let Err(e) = dispatcher.tick(Some(mission_id)).await {
                    return Ok(error_response(e.to_string()));
                }
                match storage.get_mission(mission_id).await {
                    Ok(Some(updated)) if updated.state == crate::mission::MissionState::Running => {
                        if let Err(e) = storage.add_active(mission_id).await {
                            return Ok(error_response(e.to_string()));
                        }
                    }
                    Ok(Some(_)) | Ok(None) => {}
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "directive": "resume",
                    "status": "queued_and_processed"
                })))
            }
        })
        .build()
}

/// Create the mission.dispatch tool.
pub fn mission_dispatch_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.dispatch")
        .description("Run a single mission dispatcher tick for one mission or all active missions")
        .handler(move |input: MissionDispatchInput| {
            let state = s.clone();
            async move {
                let mission_id = match input.mission_id.as_deref() {
                    Some(value) => match parse_mission_id(value) {
                        Ok(id) => {
                            let storage = mission_storage(&state);
                            match storage.get_mission(id).await {
                                Ok(Some(_)) => Some(id),
                                Ok(None) => {
                                    return Ok(error_response(format!(
                                        "Mission {} not found",
                                        value
                                    )));
                                }
                                Err(e) => return Ok(error_response(e.to_string())),
                            }
                        }
                        Err(msg) => return Ok(error_response(msg)),
                    },
                    None => None,
                };

                let storage = mission_storage(&state);
                let dispatcher = crate::mission::MissionDispatcher::new(
                    storage,
                    state.town.channel().clone(),
                    crate::mission::GhCliGitHubClient,
                    crate::mission::DispatcherConfig::default(),
                );
                let result = match dispatcher.tick(mission_id).await {
                    Ok(result) => result,
                    Err(e) => return Ok(error_response(e.to_string())),
                };

                Ok(json_result(serde_json::json!({
                    "claimed_missions": result.claimed_missions,
                    "watch_result": {
                        "watches_processed": result.watch_result.watches_processed,
                        "watches_triggered": result.watch_result.watches_triggered,
                        "watches_completed": result.watch_result.watches_completed,
                        "watches_failed": result.watch_result.watches_failed,
                        "results": result.watch_result.results.iter().map(|item| serde_json::json!({
                            "watch_id": item.watch_id,
                            "mission_id": item.mission_id,
                            "triggered": item.triggered,
                            "action_taken": item.action_taken,
                            "new_status": item.new_status,
                            "error": item.error,
                        })).collect::<Vec<_>>()
                    },
                    "scheduler_result": {
                        "total_promoted": result.scheduler_result.total_promoted,
                        "total_assigned": result.scheduler_result.total_assigned,
                        "missions_completed": result.scheduler_result.missions_completed,
                        "missions": result.scheduler_result.missions.iter().map(|mission| serde_json::json!({
                            "mission_id": mission.mission_id,
                            "promoted": mission.promoted,
                            "assigned": mission.assigned.iter().map(|(work_item_id, agent_id)| serde_json::json!({
                                "work_item_id": work_item_id,
                                "agent_id": agent_id,
                            })).collect::<Vec<_>>(),
                            "completed": mission.completed,
                            "blocked": mission.blocked,
                            "state_changed": mission.state_changed,
                            "new_state": mission.new_state,
                            "next_wake_at": mission.next_wake_at,
                        })).collect::<Vec<_>>()
                    }
                })))
            }
        })
        .build()
}

/// Create the mission.note tool.
pub fn mission_note_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.note")
        .description("Queue an operator note or directive for a mission dispatcher")
        .handler(move |input: MissionNoteInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                let note = crate::mission::MissionControlMessage::new(
                    mission_id,
                    input.sender.unwrap_or_else(|| "mcp".to_string()),
                    input.message.clone(),
                );
                if let Err(e) = storage.save_control_message(&note).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage
                    .log_event(
                        mission_id,
                        &format!("Operator note queued via MCP: {}", input.message),
                    )
                    .await
                {
                    return Ok(error_response(e.to_string()));
                }

                Ok(json_result(serde_json::json!({
                    "note": note
                })))
            }
        })
        .build()
}

/// Create the mission.input tool.
pub fn mission_input_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.input")
        .description("Record human input for a mission work item and forward it to the current owner when possible")
        .handler(move |input: MissionInputResponseInput| {
            let state = s.clone();
            async move {
                use crate::MessageService;
                use crate::app::services::messages::MessageKind;

                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let work_item_id = match parse_work_item_id(&input.work_item_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                match storage.get_mission(mission_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(error_response(format!(
                            "Mission {} not found",
                            input.mission_id
                        )));
                    }
                    Err(e) => return Ok(error_response(e.to_string())),
                }

                let Some(work_item) = (match storage.get_work_item(mission_id, work_item_id).await {
                    Ok(item) => item,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Work item {} not found in mission {}",
                        input.work_item_id, input.mission_id
                    )));
                };

                let note = crate::mission::MissionControlMessage::new(
                    mission_id,
                    "mcp",
                    format!(
                        "operator input for work item {}: {}",
                        work_item_id, input.response
                    ),
                );
                if let Err(e) = storage.save_control_message(&note).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage
                    .log_event(
                        mission_id,
                        &format!(
                            "Operator input recorded for work item '{}': {}",
                            work_item.title, input.response
                        ),
                    )
                    .await
                {
                    return Ok(error_response(e.to_string()));
                }

                let forwarded_to = if let Some(agent_id) = work_item.assigned_to {
                    let agents = match state.town.channel().list_agents().await {
                        Ok(agents) => agents,
                        Err(e) => return Ok(error_response(e.to_string())),
                    };
                    if let Some(agent) = agents.into_iter().find(|agent| agent.id == agent_id) {
                        if let Err(e) = MessageService::send(
                            &state.town,
                            &agent.name,
                            &format!(
                                "[Mission Input] Mission {} work item {}: {}",
                                mission_id, work_item_id, input.response
                            ),
                            MessageKind::Info,
                            true,
                        )
                        .await
                        {
                            return Ok(error_response(e.to_string()));
                        }
                        Some(agent.name)
                    } else {
                        None
                    }
                } else {
                    None
                };

                Ok(json_result(serde_json::json!({
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "forwarded_to": forwarded_to,
                    "recorded": true,
                })))
            }
        })
        .build()
}

/// Create the mission.stop tool.
pub fn mission_stop_tool(state: Arc<McpState>) -> Tool {
    let s = state.clone();
    ToolBuilder::new("mission.stop")
        .description("Stop an active mission")
        .handler(move |input: MissionStopInput| {
            let state = s.clone();
            async move {
                let mission_id = match parse_mission_id(&input.mission_id) {
                    Ok(id) => id,
                    Err(msg) => return Ok(error_response(msg)),
                };
                let storage = mission_storage(&state);
                let Some(mut mission) = (match storage.get_mission(mission_id).await {
                    Ok(mission) => mission,
                    Err(e) => return Ok(error_response(e.to_string())),
                }) else {
                    return Ok(error_response(format!(
                        "Mission {} not found",
                        input.mission_id
                    )));
                };
                if mission.state.is_terminal() {
                    return Ok(error_response(format!(
                        "Mission {} is terminal and cannot be stopped",
                        input.mission_id
                    )));
                }

                if input.force {
                    mission.fail("Stopped by user (forced)");
                } else {
                    mission.block("Stopped by user");
                }

                if let Err(e) = storage.save_mission(&mission).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage.remove_active(mission_id).await {
                    return Ok(error_response(e.to_string()));
                }
                if let Err(e) = storage
                    .log_event(
                        mission_id,
                        &format!("Mission stopped via MCP (force={})", input.force),
                    )
                    .await
                {
                    return Ok(error_response(e.to_string()));
                }

                Ok(json_result(serde_json::json!({
                    "mission": mission,
                    "force": input.force
                })))
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
        backlog_list_tool(state.clone()),
        mission_list_tool(state.clone()),
        mission_get_status_tool(state.clone()),
        mission_status_tool(state.clone()),
        mission_list_work_tool(state.clone()),
        mission_work_items_tool(state.clone()),
        mission_list_watches_tool(state.clone()),
        mission_watches_tool(state.clone()),
        mission_get_events_tool(state.clone()),
        mission_events_tool(state),
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
        backlog_remove_tool(state.clone()),
        mission_start_tool(state.clone()),
        mission_approve_tool(state.clone()),
        mission_reject_tool(state.clone()),
        mission_pause_tool(state.clone()),
        mission_resume_tool(state.clone()),
        mission_dispatch_tool(state.clone()),
        mission_note_tool(state.clone()),
        mission_input_tool(state.clone()),
        mission_stop_tool(state),
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
