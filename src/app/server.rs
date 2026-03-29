/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Townhall REST API server components.
//!
//! This module provides the shared server infrastructure for the townhall daemon,
//! making it accessible for integration testing.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

use crate::app::audit::audit_middleware;
use crate::app::auth::{AuthState, auth_middleware, require_scope, route_scopes};
use crate::app::services::messages::MessageKind;
use crate::config::AuthConfig;
use crate::mission::{MissionState, MissionStorage};
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

static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);
const DISPATCHER_STALE_SECS: i64 = 90;

/// Create the townhall router with all routes and authentication.
///
/// Routes are organized by required scope:
/// - `/health`, `/healthz`, `/ready`, `/readyz`, `/metrics` - No auth required
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
    let public_routes = Router::new()
        .route("/health", get(health))
        .route("/healthz", get(health))
        .route("/ready", get(readiness))
        .route("/readyz", get(readiness))
        .route("/metrics", get(metrics));

    // Read-only routes (town.read scope)
    let read_routes = Router::new()
        .route("/v1/town", get(get_town))
        .route("/v1/status", get(get_status))
        .route("/v1/agents", get(list_agents))
        .route("/v1/tasks/pending", get(list_pending_tasks))
        .route("/v1/backlog", get(list_backlog))
        .route("/v1/agents/{agent}/inbox", get(get_inbox).post(get_inbox))
        .route_layer(middleware::from_fn(move |req, next| {
            require_scope(route_scopes::READ_OPS, req, next)
        }));

    // Write routes (town.write scope)
    let write_routes = Router::new()
        .route("/v1/tasks/assign", post(assign_task))
        .route("/v1/backlog", post(add_backlog))
        .route("/v1/backlog/{task_id}/claim", post(claim_backlog))
        .route("/v1/backlog/assign-all", post(assign_all_backlog))
        .route("/v1/backlog/{task_id}", delete(remove_backlog))
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
    Json(HealthResponse {
        status: "ok",
        uptime_secs: PROCESS_START.elapsed().as_secs(),
    })
}

async fn readiness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match readiness_snapshot(&state).await {
        Ok(snapshot) => (StatusCode::OK, Json(ReadinessResponse::ready(&snapshot))).into_response(),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadinessResponse::not_ready(
                state.town.channel().town_name().to_string(),
                detail,
            )),
        )
            .into_response(),
    }
}

async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (status, body) = match gather_metrics(&state).await {
        Ok(snapshot) => (StatusCode::OK, render_metrics(&snapshot, None)),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            render_unavailable_metrics(&detail),
        ),
    };

    (
        status,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    uptime_secs: u64,
}

#[derive(Debug, Serialize)]
struct ReadinessResponse {
    status: &'static str,
    redis: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    redis_latency_ms: Option<f64>,
    dispatcher: String,
    town: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl ReadinessResponse {
    fn ready(snapshot: &ReadinessSnapshot) -> Self {
        Self {
            status: "ready",
            redis: "connected",
            redis_latency_ms: Some(snapshot.redis_latency_secs * 1000.0),
            dispatcher: snapshot.dispatcher_status.clone(),
            town: snapshot.town_name.clone(),
            detail: None,
        }
    }

    fn not_ready(town: String, detail: String) -> Self {
        Self {
            status: "not_ready",
            redis: "disconnected",
            redis_latency_ms: None,
            dispatcher: "unknown".to_string(),
            town,
            detail: Some(detail),
        }
    }
}

#[derive(Debug)]
struct ReadinessSnapshot {
    town_name: String,
    redis_latency_secs: f64,
    dispatcher_status: String,
}

#[derive(Debug)]
struct MetricsSnapshot {
    task_queue_depth: usize,
    backlog_count: usize,
    assigned_pending_task_count: usize,
    completed_task_count: usize,
    active_mission_count: usize,
    redis_latency_secs: f64,
    urgent_message_count: usize,
    agent_states: BTreeMap<String, usize>,
}

async fn probe_redis_latency(state: &AppState) -> std::result::Result<f64, String> {
    let mut conn = state.town.channel().conn().clone();
    let start = Instant::now();
    let response: String = redis::cmd("PING")
        .query_async(&mut conn)
        .await
        .map_err(|e| format!("Redis ping failed: {}", e))?;

    if response == "PONG" {
        Ok(start.elapsed().as_secs_f64())
    } else {
        Err(format!("Unexpected Redis PING response: {}", response))
    }
}

async fn readiness_snapshot(state: &AppState) -> std::result::Result<ReadinessSnapshot, String> {
    let redis_latency_secs = probe_redis_latency(state).await?;
    let storage = MissionStorage::new(
        state.town.channel().conn().clone(),
        state.town.channel().town_name(),
    );
    let missions = storage.list_all_missions().await.unwrap_or_default();

    Ok(ReadinessSnapshot {
        town_name: state.town.channel().town_name().to_string(),
        redis_latency_secs,
        dispatcher_status: dispatcher_status(&missions),
    })
}

async fn gather_metrics(state: &AppState) -> std::result::Result<MetricsSnapshot, String> {
    let redis_latency_secs = probe_redis_latency(state).await?;

    let agents = AgentService::list(&state.town)
        .await
        .map_err(|e| format!("Failed to list agents: {}", e))?;
    let backlog_count = state
        .town
        .channel()
        .backlog_len()
        .await
        .map_err(|e| format!("Failed to read backlog length: {}", e))?;
    let pending_task_count = TaskService::list_pending(&state.town)
        .await
        .map_err(|e| format!("Failed to list pending tasks: {}", e))?
        .len();
    let completed_task_count = state
        .town
        .channel()
        .list_tasks()
        .await
        .map_err(|e| format!("Failed to list tasks: {}", e))?
        .into_iter()
        .filter(|task| task.state == crate::task::TaskState::Completed)
        .count();
    let storage = MissionStorage::new(
        state.town.channel().conn().clone(),
        state.town.channel().town_name(),
    );
    let active_mission_count = storage
        .list_active()
        .await
        .map_err(|e| format!("Failed to list active missions: {}", e))?
        .len();

    let urgent_message_count = agents.iter().map(|agent| agent.urgent_len).sum();
    let mut agent_states = BTreeMap::new();
    for agent in &agents {
        *agent_states
            .entry(format!("{:?}", agent.state).to_lowercase())
            .or_insert(0) += 1;
    }

    Ok(MetricsSnapshot {
        task_queue_depth: backlog_count + pending_task_count,
        backlog_count,
        assigned_pending_task_count: pending_task_count,
        completed_task_count,
        active_mission_count,
        redis_latency_secs,
        urgent_message_count,
        agent_states,
    })
}

fn render_metrics(snapshot: &MetricsSnapshot, scrape_error: Option<&str>) -> String {
    let agent_lines: Vec<String> = snapshot
        .agent_states
        .iter()
        .map(|(state, count)| format!("tinytown_agents_total{{state=\"{}\"}} {}", state, count))
        .collect();

    let mut lines = vec![
        "# HELP tinytown_up Whether the townhall process is running.".to_string(),
        "# TYPE tinytown_up gauge".to_string(),
        "tinytown_up 1".to_string(),
        "# HELP tinytown_ready Whether townhall can reach its Redis-backed town state.".to_string(),
        "# TYPE tinytown_ready gauge".to_string(),
        "tinytown_ready 1".to_string(),
        "# HELP tinytown_agents_total Number of registered agents by state.".to_string(),
        "# TYPE tinytown_agents_total gauge".to_string(),
    ];
    lines.extend(agent_lines);
    lines.extend([
        "# HELP tinytown_tasks_pending Number of queued tasks across backlog and agent inboxes."
            .to_string(),
        "# TYPE tinytown_tasks_pending gauge".to_string(),
        format!("tinytown_tasks_pending {}", snapshot.task_queue_depth),
        "# HELP tinytown_tasks_completed_total Total completed tasks persisted in town state."
            .to_string(),
        "# TYPE tinytown_tasks_completed_total counter".to_string(),
        format!(
            "tinytown_tasks_completed_total {}",
            snapshot.completed_task_count
        ),
        "# HELP tinytown_missions_active Number of active missions.".to_string(),
        "# TYPE tinytown_missions_active gauge".to_string(),
        format!("tinytown_missions_active {}", snapshot.active_mission_count),
        "# HELP tinytown_redis_latency_seconds Redis round-trip latency for this scrape."
            .to_string(),
        "# TYPE tinytown_redis_latency_seconds gauge".to_string(),
        format!(
            "tinytown_redis_latency_seconds {:.6}",
            snapshot.redis_latency_secs
        ),
        "# HELP tinytown_backlog_tasks Tasks currently in the global backlog.".to_string(),
        "# TYPE tinytown_backlog_tasks gauge".to_string(),
        format!("tinytown_backlog_tasks {}", snapshot.backlog_count),
        "# HELP tinytown_tasks_assigned_pending Pending assigned tasks across all agents."
            .to_string(),
        "# TYPE tinytown_tasks_assigned_pending gauge".to_string(),
        format!(
            "tinytown_tasks_assigned_pending {}",
            snapshot.assigned_pending_task_count
        ),
        "# HELP tinytown_urgent_messages Urgent inbox messages across all agents.".to_string(),
        "# TYPE tinytown_urgent_messages gauge".to_string(),
        format!("tinytown_urgent_messages {}", snapshot.urgent_message_count),
    ]);

    if let Some(error) = scrape_error {
        lines.push(format!(
            "# tinytown_metrics_error {}",
            sanitize_metrics_comment(error)
        ));
    }

    lines.join("\n") + "\n"
}

fn render_unavailable_metrics(detail: &str) -> String {
    let snapshot = MetricsSnapshot {
        task_queue_depth: 0,
        backlog_count: 0,
        assigned_pending_task_count: 0,
        completed_task_count: 0,
        active_mission_count: 0,
        redis_latency_secs: 0.0,
        urgent_message_count: 0,
        agent_states: BTreeMap::new(),
    };
    let mut body = render_metrics(&snapshot, Some(detail));
    body = body.replacen("tinytown_ready 1", "tinytown_ready 0", 1);
    body
}

fn sanitize_metrics_comment(detail: &str) -> String {
    detail.replace(['\n', '\r'], " ")
}

fn dispatcher_status(missions: &[crate::mission::MissionRun]) -> String {
    let active: Vec<_> = missions
        .iter()
        .filter(|mission| {
            !matches!(
                mission.state,
                MissionState::Completed | MissionState::Failed
            )
        })
        .collect();

    if active.is_empty() {
        return "idle".to_string();
    }

    let now = chrono::Utc::now();
    let has_fresh_tick = active.iter().any(|mission| {
        mission.dispatcher_last_tick_at.is_some_and(|ts| {
            now.signed_duration_since(ts) <= chrono::Duration::seconds(DISPATCHER_STALE_SECS)
        })
    });

    if has_fresh_tick {
        "running".to_string()
    } else {
        "stalled".to_string()
    }
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
        "id": a.id.to_string(), "name": a.name, "nickname": a.nickname, "role_id": a.role_id,
        "parent_agent_id": a.parent_agent_id.map(|id| id.to_string()),
        "spawn_mode": format!("{}", a.spawn_mode),
        "cli": a.cli, "state": format!("{:?}", a.state),
        "rounds_completed": a.rounds_completed, "tasks_completed": a.tasks_completed,
        "inbox_len": a.inbox_len, "urgent_len": a.urgent_len,
        "current_scope": a.current_scope
    })).collect();
    Ok(Json(
        serde_json::json!({ "name": s.name, "root": s.root, "redis_url": s.redis_url, "agent_count": s.agent_count, "agents": agents }),
    ))
}

async fn list_agents(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let agents = AgentService::list(&state.town)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    let list: Vec<_> = agents.iter().map(|a| serde_json::json!({
        "id": a.id.to_string(), "name": a.name, "nickname": a.nickname, "role_id": a.role_id,
        "parent_agent_id": a.parent_agent_id.map(|id| id.to_string()),
        "spawn_mode": format!("{}", a.spawn_mode),
        "cli": a.cli, "state": format!("{:?}", a.state),
        "current_scope": a.current_scope
    })).collect();
    Ok(Json(
        serde_json::json!({ "agents": list, "count": list.len() }),
    ))
}

#[derive(Deserialize)]
struct SpawnReq {
    name: String,
    cli: Option<String>,
    role_id: Option<String>,
    nickname: Option<String>,
    parent_agent_id: Option<String>,
}

async fn spawn_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpawnReq>,
) -> ApiResult<impl IntoResponse> {
    let parent_id = req
        .parent_agent_id
        .as_deref()
        .map(|s| s.parse::<crate::AgentId>())
        .transpose()
        .map_err(|e| ProblemDetails::bad_request(&format!("Invalid parent_agent_id: {}", e)))?;

    let r = AgentService::spawn_with_metadata(
        &state.town,
        &req.name,
        req.cli.as_deref(),
        req.role_id.as_deref(),
        req.nickname.as_deref(),
        parent_id,
        None,
    )
    .await
    .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "agent_id": r.agent_id.to_string(),
            "name": r.name,
            "cli": r.cli,
            "role_id": r.role_id,
            "nickname": r.nickname,
            "parent_agent_id": r.parent_agent_id.map(|id| id.to_string())
        })),
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

async fn remove_backlog(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let tid: crate::TaskId = task_id
        .parse()
        .map_err(|e| ProblemDetails::bad_request(&format!("Invalid task ID: {}", e)))?;
    let removed = BacklogService::remove(state.town.channel(), tid)
        .await
        .map_err(|e| ProblemDetails::internal_error(&e.to_string()))?;

    if !removed {
        return Err(ProblemDetails::not_found(&format!(
            "Task {} not found in backlog",
            tid
        )));
    }

    Ok(Json(
        serde_json::json!({ "removed": true, "task_id": tid.to_string() }),
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
