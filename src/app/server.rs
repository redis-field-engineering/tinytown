/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Townhall REST API server components.
//!
//! This module provides the shared server infrastructure for the townhall daemon,
//! making it accessible for integration testing.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::app::audit::audit_middleware;
use crate::app::auth::{AuthState, auth_middleware, require_scope, route_scopes};
use crate::app::services::messages::MessageKind;
use crate::config::AuthConfig;
use crate::{AgentService, BacklogService, MessageService, RecoveryService, TaskService, Town};

/// Application state shared across all routes.
pub struct AppState {
    pub town: Town,
    /// Authentication configuration (optional, defaults to None mode)
    pub auth_config: Arc<AuthConfig>,
}

/// RFC 7807 Problem Details for error responses.
#[derive(Debug, Serialize, Clone)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub error_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
}

impl ProblemDetails {
    pub fn new(status: StatusCode, title: &str, detail: &str) -> Self {
        Self {
            error_type: format!("https://tinytown.dev/errors/{}", status.as_u16()),
            title: title.to_string(),
            status: status.as_u16(),
            detail: detail.to_string(),
        }
    }

    pub fn not_found(detail: &str) -> (StatusCode, Json<Self>) {
        (
            StatusCode::NOT_FOUND,
            Json(Self::new(StatusCode::NOT_FOUND, "Not Found", detail)),
        )
    }

    pub fn internal_error(detail: &str) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                detail,
            )),
        )
    }

    pub fn bad_request(detail: &str) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self::new(StatusCode::BAD_REQUEST, "Bad Request", detail)),
        )
    }
}

type ApiResult<T> = std::result::Result<T, (StatusCode, Json<ProblemDetails>)>;

/// Create the townhall router with all routes and authentication.
///
/// Routes are organized by required scope:
/// - `/healthz` - No auth required
/// - Read operations (town.read): GET /v1/town, /v1/status, /v1/agents, /v1/tasks/pending,
///   /v1/backlog, /v1/agents/{agent}/inbox
/// - Write operations (town.write): POST /v1/tasks/assign, /v1/backlog, /v1/backlog/{task_id}/claim,
///   /v1/backlog/assign-all, /v1/messages/send
/// - Agent management (agent.manage): POST /v1/agents, /v1/agents/{agent}/kill,
///   /v1/agents/{agent}/restart, /v1/agents/prune, /v1/recover, /v1/reclaim
pub fn create_router(state: Arc<AppState>) -> Router {
    let auth_state = AuthState {
        config: state.auth_config.clone(),
    };

    // Public routes (no auth required)
    let public_routes = Router::new().route("/healthz", get(health));

    // Read-only routes (town.read scope)
    let read_routes = Router::new()
        .route("/v1/town", get(get_town))
        .route("/v1/status", get(get_status))
        .route("/v1/agents", get(list_agents))
        .route("/v1/tasks/pending", get(list_pending_tasks))
        .route("/v1/backlog", get(list_backlog))
        .route("/v1/agents/{agent}/inbox", post(get_inbox))
        .route_layer(middleware::from_fn(move |req, next| {
            require_scope(route_scopes::READ_OPS, req, next)
        }));

    // Write routes (town.write scope)
    let write_routes = Router::new()
        .route("/v1/tasks/assign", post(assign_task))
        .route("/v1/backlog", post(add_backlog))
        .route("/v1/backlog/{task_id}/claim", post(claim_backlog))
        .route("/v1/backlog/assign-all", post(assign_all_backlog))
        .route("/v1/messages/send", post(send_message))
        .route_layer(middleware::from_fn(move |req, next| {
            require_scope(route_scopes::WRITE_OPS, req, next)
        }));

    // Agent management routes (agent.manage scope)
    let agent_mgmt_routes = Router::new()
        .route("/v1/agents", post(spawn_agent))
        .route("/v1/agents/{agent}/kill", post(kill_agent))
        .route("/v1/agents/{agent}/restart", post(restart_agent))
        .route("/v1/agents/prune", post(prune_agents))
        .route("/v1/recover", post(recover))
        .route("/v1/reclaim", post(reclaim))
        .route_layer(middleware::from_fn(move |req, next| {
            require_scope(route_scopes::AGENT_MGMT, req, next)
        }));

    // Combine all authenticated routes with audit logging
    let authenticated_routes = Router::new()
        .merge(read_routes)
        .merge(write_routes)
        .merge(agent_mgmt_routes)
        .route_layer(middleware::from_fn(audit_middleware))
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());

    // Combine public and authenticated routes
    public_routes.merge(authenticated_routes).with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn get_town(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let c = state.town.config();
    Json(
        serde_json::json!({ "name": c.name, "root": state.town.root().display().to_string(), "redis_url": c.redis_url_redacted() }),
    )
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct StatusQuery {
    #[serde(default)]
    deep: bool,
}

async fn get_status(
    State(state): State<Arc<AppState>>,
    Query(_q): Query<StatusQuery>,
) -> ApiResult<impl IntoResponse> {
    let s = AgentService::status(&state.town)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let agents: Vec<_> = s.agents.iter().map(|a| serde_json::json!({
        "id": a.id.to_string(), "name": a.name, "cli": a.cli, "state": format!("{:?}", a.state),
        "rounds_completed": a.rounds_completed, "tasks_completed": a.tasks_completed, "inbox_len": a.inbox_len, "urgent_len": a.urgent_len
    })).collect();
    Ok(Json(
        serde_json::json!({ "name": s.name, "root": s.root, "redis_url": s.redis_url, "agent_count": s.agent_count, "agents": agents }),
    ))
}

async fn list_agents(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let agents = AgentService::list(&state.town)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let list: Vec<_> = agents.iter().map(|a| serde_json::json!({ "id": a.id.to_string(), "name": a.name, "cli": a.cli, "state": format!("{:?}", a.state) })).collect();
    Ok(Json(
        serde_json::json!({ "agents": list, "count": list.len() }),
    ))
}

#[derive(Deserialize)]
struct SpawnReq {
    name: String,
    cli: Option<String>,
}

async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpawnReq>,
) -> ApiResult<impl IntoResponse> {
    let r = AgentService::spawn(&state.town, &req.name, req.cli.as_deref())
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            serde_json::json!({ "agent_id": r.agent_id.to_string(), "name": r.name, "cli": r.cli }),
        ),
    ))
}

async fn kill_agent(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let h = state
        .town
        .agent(&agent)
        .await
        .map_err(|e| ProblemDetails::not_found(&e.to_string()))?;
    AgentService::kill(state.town.channel(), h.id())
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "status": "stopped", "agent": agent }),
    ))
}

async fn restart_agent(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let h = state
        .town
        .agent(&agent)
        .await
        .map_err(|e| ProblemDetails::not_found(&e.to_string()))?;
    AgentService::restart(state.town.channel(), h.id())
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "status": "restarted", "agent": agent }),
    ))
}

#[derive(Deserialize)]
struct PruneReq {
    #[serde(default)]
    all: bool,
}

async fn prune_agents(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PruneReq>,
) -> ApiResult<impl IntoResponse> {
    let removed = AgentService::prune(&state.town, req.all)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "removed": removed.len(), "agents": removed.iter().map(|a| &a.name).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct AssignReq {
    agent: String,
    task: String,
}

async fn assign_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AssignReq>,
) -> ApiResult<impl IntoResponse> {
    let r = TaskService::assign(&state.town, &req.agent, &req.task)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            serde_json::json!({ "task_id": r.task_id.to_string(), "agent_id": r.agent_id.to_string(), "agent_name": r.agent_name }),
        ),
    ))
}

async fn list_pending_tasks(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let pending = TaskService::list_pending(&state.town)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let tasks: Vec<_> = pending.iter().map(|t| serde_json::json!({ "task_id": t.task_id.to_string(), "description": t.description, "agent_id": t.agent_id.to_string(), "agent_name": t.agent_name })).collect();
    Ok(Json(
        serde_json::json!({ "tasks": tasks, "count": tasks.len() }),
    ))
}

#[derive(Deserialize)]
struct AddBacklogReq {
    description: String,
    tags: Option<Vec<String>>,
}

async fn add_backlog(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddBacklogReq>,
) -> ApiResult<impl IntoResponse> {
    let r = BacklogService::add(state.town.channel(), &req.description, req.tags)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "task_id": r.task_id.to_string(), "description": r.description })),
    ))
}

async fn list_backlog(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let items = BacklogService::list(state.town.channel())
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let list: Vec<_> = items.iter().map(|i| serde_json::json!({ "task_id": i.task_id.to_string(), "description": i.description, "tags": i.tags })).collect();
    Ok(Json(
        serde_json::json!({ "backlog": list, "count": list.len() }),
    ))
}

#[derive(Deserialize)]
struct ClaimReq {
    agent: String,
}

async fn claim_backlog(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(req): Json<ClaimReq>,
) -> ApiResult<impl IntoResponse> {
    let tid: crate::TaskId = task_id
        .parse()
        .map_err(|e| ProblemDetails::bad_request(&format!("Invalid task ID: {}", e)))?;
    let r = BacklogService::claim(&state.town, tid, &req.agent)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "task_id": r.task_id.to_string(), "agent_id": r.agent_id.to_string(), "agent_name": r.agent_name }),
    ))
}

#[derive(Deserialize)]
struct AssignAllReq {
    agent: String,
}

async fn assign_all_backlog(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AssignAllReq>,
) -> ApiResult<impl IntoResponse> {
    let results = BacklogService::assign_all(&state.town, &req.agent)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "assigned": results.len(), "tasks": results.iter().map(|r| r.task_id.to_string()).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct SendReq {
    to: String,
    message: String,
    kind: Option<String>,
    #[serde(default)]
    urgent: bool,
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendReq>,
) -> ApiResult<impl IntoResponse> {
    let kind = match req.kind.as_deref() {
        Some("query") => MessageKind::Query,
        Some("info") => MessageKind::Info,
        Some("ack") => MessageKind::Ack,
        _ => MessageKind::Task,
    };
    let r = MessageService::send(&state.town, &req.to, &req.message, kind, req.urgent)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            serde_json::json!({ "message_id": r.message_id.to_string(), "to_agent": r.to_agent.to_string(), "urgent": r.urgent }),
        ),
    ))
}

async fn get_inbox(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let inbox = MessageService::get_inbox(&state.town, &agent)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let msgs: Vec<_> = inbox.messages.iter().map(|m| serde_json::json!({ "id": m.id.to_string(), "from": m.from.to_string(), "type": m.msg_type, "summary": m.summary })).collect();
    Ok(Json(
        serde_json::json!({ "agent": agent, "total": inbox.total_messages, "urgent": inbox.urgent_messages, "messages": msgs }),
    ))
}

async fn recover(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let r = RecoveryService::recover(&state.town, state.town.root())
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "checked": r.agents_checked, "recovered": r.agents_recovered, "agents": r.recovered_agents.iter().map(|a| &a.name).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct ReclaimReq {
    #[serde(default)]
    to_backlog: bool,
    to: Option<String>,
    from: Option<String>,
}

async fn reclaim(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReclaimReq>,
) -> ApiResult<impl IntoResponse> {
    let r = RecoveryService::reclaim(
        &state.town,
        req.to_backlog,
        req.to.as_deref(),
        req.from.as_deref(),
    )
    .await
    .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "reclaimed": r.tasks_reclaimed, "destination": format!("{:?}", r.destination) }),
    ))
}
