/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Integration tests for the townhall daemon REST API (Issue #15).
//!
//! These tests verify the townhall REST API endpoints including:
//! - Health endpoints (/healthz, /v1/status)
//! - Agent management (/v1/agents)
//! - Task assignment and backlog (/v1/tasks, /v1/backlog)
//! - Messaging (/v1/messages)
//! - Recovery operations (/v1/recover, /v1/reclaim)
//!
//! Test infrastructure includes:
//! - `TownhallTestServer`: Wrapper for testing townhall with a real Redis backend
//! - `TestTownhall`: Test fixture providing full E2E testing capabilities
//! - Helper functions for common test scenarios

use tempfile::TempDir;
use tinytown::town::AgentHandle;
use tinytown::{Task, Town};

// ============================================================================
// TEST FIXTURES AND HELPERS
// ============================================================================

/// Test server wrapper that manages a townhall instance for testing.
/// Includes the underlying Town (with Redis) and provides HTTP client access.
pub struct TownhallTestServer {
    /// The underlying town with Redis connection
    pub town: Town,
    /// Temp directory for the town (cleaned up on drop)
    pub temp_dir: TempDir,
    /// Base URL for the townhall REST API (when server is running)
    pub base_url: Option<String>,
}

impl TownhallTestServer {
    /// Create a new test server with a fresh town and Redis instance.
    /// Uses Unix socket mode for test isolation.
    pub async fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Force Unix socket mode for test isolation
        unsafe {
            std::env::set_var("TT_USE_SOCKET", "1");
        }

        let temp_dir = TempDir::new()?;
        let town = Town::init(temp_dir.path(), name).await?;

        Ok(Self {
            town,
            temp_dir,
            base_url: None,
        })
    }

    /// Get the town's channel for direct Redis operations
    pub fn channel(&self) -> &tinytown::Channel {
        self.town.channel()
    }

    /// Get the town's config
    pub fn config(&self) -> &tinytown::Config {
        self.town.config()
    }

    /// Create a test agent in the town
    pub async fn spawn_test_agent(&self, name: &str) -> Result<AgentHandle, tinytown::Error> {
        self.town.spawn_agent(name, "test-cli").await
    }

    /// Add a task to the backlog
    pub async fn add_backlog_task(
        &self,
        description: &str,
    ) -> Result<tinytown::TaskId, tinytown::Error> {
        let task = Task::new(description);
        let task_id = task.id;
        self.channel().set_task(&task).await?;
        self.channel().backlog_push(task_id).await?;
        Ok(task_id)
    }
}

impl Drop for TownhallTestServer {
    fn drop(&mut self) {
        // Clean up Redis when test ends
        let pid_file = self.temp_dir.path().join(".tt/redis.pid");
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
            && let Ok(pid) = pid_str.trim().parse::<i32>()
        {
            // SAFETY: This kills our test Redis process, which is safe to do.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

// ============================================================================
// EXPECTED API RESPONSE TYPES (for deserializing townhall responses)
// ============================================================================

/// Standard RFC 7807 error response format
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ApiError {
    pub r#type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Health check response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Town status response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TownStatusResponse {
    pub name: String,
    pub agent_count: usize,
    pub backlog_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redis_connected: Option<bool>,
}

/// Agent list response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AgentListResponse {
    pub agents: Vec<AgentInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Agent info in list response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub state: String,
    pub cli: String,
}

/// Backlog task entry
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BacklogEntry {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Backlog list response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BacklogListResponse {
    pub tasks: Vec<BacklogEntry>,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Message send request
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SendMessageRequest {
    pub to: String,
    pub message: String,
    #[serde(default)]
    pub kind: String, // "task" | "query" | "info" | "ack"
    #[serde(default)]
    pub urgent: bool,
}

/// Message send response
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SendMessageResponse {
    pub message_id: String,
    pub delivered: bool,
}

// ============================================================================
// PLACEHOLDER TESTS - These will test townhall when it's implemented
// ============================================================================

/// Test that the test infrastructure itself works correctly.
#[tokio::test]
async fn test_townhall_test_server_creation() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-infra-test").await?;

    // Verify town was created
    assert_eq!(server.config().name, "townhall-infra-test");

    // Verify we can spawn agents through the test server
    let agent = server.spawn_test_agent("test-worker").await?;
    let state = agent.state().await?;
    assert!(state.is_some());

    Ok(())
}

/// Test that backlog operations work through the test server.
#[tokio::test]
async fn test_townhall_test_server_backlog() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-backlog-infra-test").await?;

    // Add tasks to backlog
    let task1_id = server.add_backlog_task("Task 1 for testing").await?;
    let task2_id = server.add_backlog_task("Task 2 for testing").await?;

    // Verify backlog has the tasks
    let backlog = server.channel().backlog_list().await?;
    assert_eq!(backlog.len(), 2);
    assert_eq!(backlog[0], task1_id);
    assert_eq!(backlog[1], task2_id);

    Ok(())
}

/// Test agent spawn and list through test infrastructure.
#[tokio::test]
async fn test_townhall_test_server_agents() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-agents-infra-test").await?;

    // Spawn multiple agents
    let _agent1 = server.spawn_test_agent("worker-1").await?;
    let _agent2 = server.spawn_test_agent("worker-2").await?;
    let _agent3 = server.spawn_test_agent("reviewer").await?;

    // List agents
    let agents = server.town.list_agents().await;
    assert_eq!(agents.len(), 3);

    // Verify agent names
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"worker-1"));
    assert!(names.contains(&"worker-2"));
    assert!(names.contains(&"reviewer"));

    Ok(())
}

// ============================================================================
// TOWNHALL REST API TESTS
// ============================================================================

// Import townhall router creation - note: this requires the bin to expose create_router
// For now, we test via the services layer which is what townhall uses

/// Test GET /healthz equivalent via service layer
#[tokio::test]
async fn test_services_status() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-status-test").await?;

    // Test AgentService::status (what /v1/status uses)
    let status = tinytown::AgentService::status(&server.town).await?;
    assert_eq!(status.name, "townhall-status-test");
    assert_eq!(status.agent_count, 0);

    Ok(())
}

/// Test agent spawn via service layer (what POST /v1/agents uses)
#[tokio::test]
async fn test_services_spawn_agent() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-spawn-test").await?;

    let result =
        tinytown::AgentService::spawn(&server.town, "test-worker", Some("test-cli")).await?;
    assert_eq!(result.name, "test-worker");
    assert_eq!(result.cli, "test-cli");

    // Verify agent exists
    let agents = tinytown::AgentService::list(&server.town).await?;
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "test-worker");

    Ok(())
}

/// Test backlog operations via service layer (what /v1/backlog uses)
#[tokio::test]
async fn test_services_backlog() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-backlog-test").await?;

    // Add to backlog
    let result = tinytown::BacklogService::add(
        server.channel(),
        "Test task",
        Some(vec!["test".to_string()]),
    )
    .await?;
    assert_eq!(result.description, "Test task");

    // List backlog
    let items = tinytown::BacklogService::list(server.channel()).await?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].description, "Test task");
    assert_eq!(items[0].tags, vec!["test"]);

    Ok(())
}

/// Test task assignment via service layer (what POST /v1/tasks/assign uses)
#[tokio::test]
async fn test_services_assign_task() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-assign-test").await?;

    // First spawn an agent
    let _agent = server.spawn_test_agent("worker").await?;

    // Assign a task
    let result = tinytown::TaskService::assign(&server.town, "worker", "Do something").await?;
    assert_eq!(result.agent_name, "worker");

    let inbox = server
        .town
        .channel()
        .peek_inbox(result.agent_id, 10)
        .await?;
    assert_eq!(inbox.len(), 1);
    match &inbox[0].msg_type {
        tinytown::MessageType::TaskAssign { task_id } => {
            assert_eq!(task_id, &result.task_id.to_string());
        }
        other => panic!("expected TaskAssign, got {:?}", other),
    }

    // Verify task is pending
    let pending = tinytown::TaskService::list_pending(&server.town).await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].task_id, result.task_id);
    assert_eq!(pending[0].description, "Do something");

    Ok(())
}

/// Test message sending via service layer (what POST /v1/messages/send uses)
#[tokio::test]
async fn test_services_send_message() -> Result<(), Box<dyn std::error::Error>> {
    let server = TownhallTestServer::new("townhall-message-test").await?;

    // Spawn an agent
    let _agent = server.spawn_test_agent("receiver").await?;

    // Send a message
    let result = tinytown::MessageService::send(
        &server.town,
        "receiver",
        "Hello!",
        tinytown::app::services::messages::MessageKind::Task,
        false,
    )
    .await?;
    assert!(!result.urgent);

    // Check inbox
    let inbox = tinytown::MessageService::get_inbox(&server.town, "receiver").await?;
    assert_eq!(inbox.total_messages, 1);

    Ok(())
}

/// Test that the inbox endpoint supports GET for read semantics while keeping POST compatibility.
#[tokio::test]
async fn test_townhall_inbox_endpoint_supports_get_and_post()
-> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, create_router};

    let server = TownhallTestServer::new("townhall-inbox-route-test").await?;
    server.spawn_test_agent("receiver").await?;
    tinytown::MessageService::send(
        &server.town,
        "receiver",
        "Hello over townhall",
        tinytown::app::services::messages::MessageKind::Info,
        false,
    )
    .await?;

    let auth_config = Arc::new(AuthConfig::default());
    let state = Arc::new(AppState {
        town: server.town.clone(),
        auth_config,
    });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    test_server
        .get("/v1/agents/receiver/inbox")
        .await
        .assert_status_ok()
        .assert_json_contains(&serde_json::json!({
            "agent": "receiver",
            "total": 1
        }));

    test_server
        .post("/v1/agents/receiver/inbox")
        .await
        .assert_status_ok()
        .assert_json_contains(&serde_json::json!({
            "agent": "receiver",
            "total": 1
        }));

    Ok(())
}

/// Test that backlog removal is exposed through the REST router and deletes task data.
#[tokio::test]
async fn test_townhall_delete_backlog_endpoint() -> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, BacklogService, create_router};

    let server = TownhallTestServer::new("townhall-backlog-delete-route-test").await?;
    let added = BacklogService::add(server.channel(), "Remove me", None).await?;

    let auth_config = Arc::new(AuthConfig::default());
    let state = Arc::new(AppState {
        town: server.town.clone(),
        auth_config,
    });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    test_server
        .delete(&format!("/v1/backlog/{}", added.task_id))
        .await
        .assert_status_ok()
        .assert_json_contains(&serde_json::json!({
            "removed": true,
            "task_id": added.task_id.to_string()
        }));

    assert!(BacklogService::list(server.channel()).await?.is_empty());
    assert!(server.channel().get_task(added.task_id).await?.is_none());

    Ok(())
}

// ============================================================================
// AUTHENTICATION TESTS (Issue #16)
// ============================================================================

/// Test that auth module functions work correctly.
#[tokio::test]
async fn test_auth_api_key_generation_and_verification() {
    let (raw_key, hash) = tinytown::generate_api_key();

    // Key should be a long hex string
    assert!(raw_key.len() >= 32);

    // Hash should be Argon2id format
    assert!(hash.starts_with("$argon2"));

    // Verification should work
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let parsed_hash = PasswordHash::new(&hash).expect("valid hash");
    assert!(
        Argon2::default()
            .verify_password(raw_key.as_bytes(), &parsed_hash)
            .is_ok()
    );

    // Wrong key should fail
    assert!(
        Argon2::default()
            .verify_password(b"wrong-key", &parsed_hash)
            .is_err()
    );
}

/// Test principal scope checking.
#[tokio::test]
async fn test_principal_scopes() {
    use std::collections::HashSet;
    use tinytown::{Principal, Scope};

    // Local admin has all scopes
    let admin = Principal::local_admin();
    assert!(admin.has_scope(Scope::TownRead));
    assert!(admin.has_scope(Scope::TownWrite));
    assert!(admin.has_scope(Scope::AgentManage));
    assert!(admin.has_scope(Scope::Admin));

    // Principal with only TownRead
    let mut scopes = HashSet::new();
    scopes.insert(Scope::TownRead);
    let reader = tinytown::Principal {
        id: "reader".to_string(),
        scopes,
    };
    assert!(reader.has_scope(Scope::TownRead));
    assert!(!reader.has_scope(Scope::TownWrite));
    assert!(!reader.has_scope(Scope::AgentManage));
    assert!(!reader.has_scope(Scope::Admin));
}

/// Test that health endpoint works without auth.
#[tokio::test]
async fn test_health_endpoint_no_auth_required() -> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, create_router};

    let temp_dir = tempfile::TempDir::new()?;
    unsafe {
        std::env::set_var("TT_USE_SOCKET", "1");
    }
    let town = tinytown::Town::init(temp_dir.path(), "auth-health-test").await?;

    // Create router with API key auth mode (but health should still work)
    let auth_config = Arc::new(AuthConfig {
        mode: tinytown::AuthMode::ApiKey,
        api_key_hash: Some("$argon2id$v=19$m=19456,t=2,p=1$fake$fake".to_string()),
        ..Default::default()
    });
    let state = Arc::new(AppState { town, auth_config });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    // Health endpoint should work without auth
    test_server.get("/healthz").await.assert_status_ok();

    Ok(())
}

/// Test that protected endpoints require authentication.
#[tokio::test]
async fn test_protected_endpoints_require_auth() -> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, create_router};

    let temp_dir = tempfile::TempDir::new()?;
    unsafe {
        std::env::set_var("TT_USE_SOCKET", "1");
    }
    let town = tinytown::Town::init(temp_dir.path(), "auth-protected-test").await?;

    // Create router with API key auth mode
    let (raw_key, hash) = tinytown::generate_api_key();
    let auth_config = Arc::new(AuthConfig {
        mode: tinytown::AuthMode::ApiKey,
        api_key_hash: Some(hash),
        ..Default::default()
    });
    let state = Arc::new(AppState { town, auth_config });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    // Request without auth should return 401
    test_server
        .get("/v1/status")
        .await
        .assert_status_unauthorized();

    // Request with wrong key should return 401
    test_server
        .get("/v1/status")
        .add_header(axum_test::http::header::AUTHORIZATION, "Bearer wrong-key")
        .await
        .assert_status_unauthorized();

    // Request with correct key should succeed
    test_server
        .get("/v1/status")
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            format!("Bearer {}", raw_key),
        )
        .await
        .assert_status_ok();

    Ok(())
}

/// Test that X-API-Key header also works for authentication.
#[tokio::test]
async fn test_x_api_key_header_auth() -> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, create_router};

    let temp_dir = tempfile::TempDir::new()?;
    unsafe {
        std::env::set_var("TT_USE_SOCKET", "1");
    }
    let town = tinytown::Town::init(temp_dir.path(), "auth-x-api-key-test").await?;

    let (raw_key, hash) = tinytown::generate_api_key();
    let auth_config = Arc::new(AuthConfig {
        mode: tinytown::AuthMode::ApiKey,
        api_key_hash: Some(hash),
        ..Default::default()
    });
    let state = Arc::new(AppState { town, auth_config });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    // Request with X-API-Key header should succeed
    test_server
        .get("/v1/town")
        .add_header("x-api-key", raw_key)
        .await
        .assert_status_ok();

    Ok(())
}

/// Test that auth.mode=none allows all requests.
#[tokio::test]
async fn test_auth_mode_none_allows_all() -> Result<(), Box<dyn std::error::Error>> {
    use axum_test::TestServer;
    use std::sync::Arc;
    use tinytown::{AppState, AuthConfig, create_router};

    let temp_dir = tempfile::TempDir::new()?;
    unsafe {
        std::env::set_var("TT_USE_SOCKET", "1");
    }
    let town = tinytown::Town::init(temp_dir.path(), "auth-none-test").await?;

    // auth.mode = none (default)
    let auth_config = Arc::new(AuthConfig::default());
    let state = Arc::new(AppState { town, auth_config });
    let app = create_router(state);
    let test_server = TestServer::new(app);

    // All endpoints should work without auth
    test_server.get("/v1/status").await.assert_status_ok();
    test_server.get("/v1/agents").await.assert_status_ok();
    test_server.get("/v1/backlog").await.assert_status_ok();

    Ok(())
}
