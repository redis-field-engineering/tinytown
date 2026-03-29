/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Integration tests for the townhall MCP (Model Context Protocol) interface (Issue #17).
//!
//! These tests verify the MCP router, tools, resources, and prompts can be constructed
//! and the underlying services work correctly.
//!
//! Note: Full end-to-end MCP testing with TestClient requires careful API alignment
//! with tower-mcp's testing module. These tests focus on verifying the router
//! construction and service integration work correctly.

use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::TempDir;
use tinytown::{McpState, Town, create_mcp_router};
use tower_mcp::TestClient;
use uuid::Uuid;

// ============================================================================
// TEST FIXTURES AND HELPERS
// ============================================================================

/// Test fixture that creates a town with MCP router configured.
pub struct McpTestContext {
    /// The underlying town with Redis connection
    pub town: Town,
    /// Temp directory for the town (cleaned up on drop)
    pub temp_dir: TempDir,
    /// The MCP state (shared with router)
    pub mcp_state: Arc<McpState>,
}

impl McpTestContext {
    /// Create a new MCP test context with a fresh town and Redis instance.
    pub async fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let town = Town::init(temp_dir.path(), name).await?;
        let mcp_state = Arc::new(McpState::new(town.clone()));

        Ok(Self {
            town,
            temp_dir,
            mcp_state,
        })
    }

    /// Spawn a test agent in the town for use in MCP tests.
    pub async fn spawn_test_agent(&self, name: &str) -> Result<(), tinytown::Error> {
        self.town.spawn_agent(name, "test-cli").await?;
        Ok(())
    }
}

fn unique_town_name(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

impl Drop for McpTestContext {
    fn drop(&mut self) {
        // Clean up Redis when test ends
        let pid_file = self.temp_dir.path().join(".tt/redis.pid");
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
            && let Ok(pid) = pid_str.trim().parse::<i32>()
        {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

// ============================================================================
// MCP ROUTER TESTS
// ============================================================================

/// Test that MCP router can be created successfully.
#[tokio::test]
async fn test_mcp_router_creation() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-router-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Create the MCP router - this verifies all tools, resources, and prompts register correctly
    let _router = create_mcp_router(ctx.mcp_state.clone(), "test-server", "0.1.0");

    // If we get here without panicking, the router was created successfully
    Ok(())
}

/// Test that MCP router can be created with agents present.
#[tokio::test]
async fn test_mcp_router_with_agents() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-router-agents-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Spawn some agents
    ctx.spawn_test_agent("worker-1").await?;
    ctx.spawn_test_agent("worker-2").await?;

    // Create the MCP router
    let _router = create_mcp_router(ctx.mcp_state.clone(), "test-server", "0.1.0");

    // Verify agents exist via service
    let agents = tinytown::AgentService::list(&ctx.town).await?;
    assert_eq!(agents.len(), 2);

    Ok(())
}

// ============================================================================
// MCP SERVICE INTEGRATION TESTS
// ============================================================================

/// Test that AgentService (used by MCP tools) works correctly.
#[tokio::test]
async fn test_mcp_service_agent_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-agent-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Test status (used by town.get_status tool)
    let status = tinytown::AgentService::status(&ctx.town).await?;
    assert_eq!(status.name, town_name);
    assert_eq!(status.agent_count, 0);

    // Test spawn (used by agent.spawn tool)
    let spawn_result =
        tinytown::AgentService::spawn(&ctx.town, "test-worker", Some("test-cli")).await?;
    assert_eq!(spawn_result.name, "test-worker");
    assert_eq!(spawn_result.cli, "test-cli");

    // Test list (used by agent.list tool)
    let agents = tinytown::AgentService::list(&ctx.town).await?;
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "test-worker");

    // Test stop_all (used by `tt stop` semantics)
    let stopped = tinytown::AgentService::stop_all(&ctx.town).await?;
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].name, "test-worker");
    assert!(
        ctx.town
            .channel()
            .should_stop(spawn_result.agent_id)
            .await?
    );

    Ok(())
}

/// Test that TaskService (used by MCP tools) works correctly.
#[tokio::test]
async fn test_mcp_service_task_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-task-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Spawn an agent
    ctx.spawn_test_agent("worker").await?;

    // Test assign (used by task.assign tool)
    let assign_result =
        tinytown::TaskService::assign(&ctx.town, "worker", "Implement feature").await?;
    assert_eq!(assign_result.agent_name, "worker");

    let inbox = ctx
        .town
        .channel()
        .peek_inbox(assign_result.agent_id, 10)
        .await?;
    assert_eq!(inbox.len(), 1);
    match &inbox[0].msg_type {
        tinytown::MessageType::TaskAssign { task_id } => {
            assert_eq!(task_id, &assign_result.task_id.to_string());
        }
        other => panic!("expected TaskAssign, got {:?}", other),
    }

    // Test list_pending
    let pending = tinytown::TaskService::list_pending(&ctx.town).await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].task_id, assign_result.task_id);
    assert_eq!(pending[0].description, "Implement feature");

    Ok(())
}

/// Test that MessageService (used by MCP tools) works correctly.
#[tokio::test]
async fn test_mcp_service_message_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-message-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Spawn an agent
    ctx.spawn_test_agent("receiver").await?;

    // Test send (used by message.send tool)
    let send_result = tinytown::MessageService::send(
        &ctx.town,
        "receiver",
        "Hello from MCP!",
        tinytown::app::services::messages::MessageKind::Info,
        false,
    )
    .await?;
    assert!(!send_result.urgent);

    // Test get_inbox
    let inbox = tinytown::MessageService::get_inbox(&ctx.town, "receiver").await?;
    assert_eq!(inbox.total_messages, 1);

    Ok(())
}

/// Test that BacklogService (used by MCP tools) works correctly.
#[tokio::test]
async fn test_mcp_service_backlog_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-backlog-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Test add (used by backlog.add tool)
    let add_result = tinytown::BacklogService::add(
        ctx.town.channel(),
        "Review the code",
        Some(vec!["review".to_string(), "code".to_string()]),
    )
    .await?;
    assert_eq!(add_result.description, "Review the code");

    // Test list (used by backlog.list tool)
    let items = tinytown::BacklogService::list(ctx.town.channel()).await?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].description, "Review the code");
    assert_eq!(items[0].tags, vec!["review", "code"]);

    // Test claim (used by backlog.claim tool)
    ctx.spawn_test_agent("worker").await?;
    let claim_result =
        tinytown::BacklogService::claim(&ctx.town, add_result.task_id, "worker").await?;
    assert_eq!(claim_result.agent_name, "worker");

    // Verify backlog is now empty
    let items_after = tinytown::BacklogService::list(ctx.town.channel()).await?;
    assert!(items_after.is_empty());

    // Test remove (used by backlog.remove tool)
    let removable =
        tinytown::BacklogService::add(ctx.town.channel(), "Remove this task", None).await?;
    let removed = tinytown::BacklogService::remove(ctx.town.channel(), removable.task_id).await?;
    assert!(removed);
    assert!(
        ctx.town
            .channel()
            .get_task(removable.task_id)
            .await?
            .is_none()
    );

    Ok(())
}

/// Test that RecoveryService (used by MCP tools) works correctly.
#[tokio::test]
async fn test_mcp_service_recovery_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-recovery-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Test recover (used by recovery.recover_agents tool)
    let recover_result = tinytown::RecoveryService::recover(&ctx.town, ctx.town.root()).await?;
    // No agents to recover in a fresh town
    assert_eq!(recover_result.agents_recovered, 0);

    // Test reclaim (used by recovery.reclaim_tasks tool)
    let reclaim_result = tinytown::RecoveryService::reclaim(&ctx.town, true, None, None).await?;
    // No tasks to reclaim in a fresh town
    assert_eq!(reclaim_result.tasks_reclaimed, 0);

    Ok(())
}

/// Test that mission storage and bootstrap helpers used by MCP tools work correctly.
#[tokio::test]
async fn test_mcp_service_mission_operations() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-svc-mission-test");
    let ctx = McpTestContext::new(&town_name).await?;

    let storage =
        tinytown::mission::MissionStorage::new(ctx.town.channel().conn().clone(), &town_name);

    let mut mission =
        tinytown::mission::MissionRun::new(vec![tinytown::mission::ObjectiveRef::Doc {
            path: "docs/design.md".to_string(),
        }]);
    mission.start();

    storage.save_mission(&mission).await?;
    storage.add_active(mission.id).await?;
    storage
        .log_event(mission.id, "Mission started in test")
        .await?;

    let work_items = tinytown::mission::build_mission_work_items(
        ctx.town.root(),
        mission.id,
        &mission.objective_refs,
    )?;
    assert_eq!(work_items.len(), 1);
    storage.save_work_item(&work_items[0]).await?;

    let note =
        tinytown::mission::MissionControlMessage::new(mission.id, "tester", "resume and retry");
    storage.save_control_message(&note).await?;

    let stored = storage.get_mission(mission.id).await?;
    assert!(stored.is_some());

    let active = storage.list_active().await?;
    assert!(active.contains(&mission.id));

    let stored_work = storage.list_work_items(mission.id).await?;
    assert_eq!(stored_work.len(), 1);
    assert_eq!(stored_work[0].title, "docs/design.md");

    let controls = storage.list_control_messages(mission.id).await?;
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].body, "resume and retry");

    let events = storage.get_events(mission.id, 5).await?;
    assert_eq!(events.len(), 1);

    let all_missions = storage.list_all_missions().await?;
    assert_eq!(all_missions.len(), 1);
    assert_eq!(all_missions[0].id, mission.id);

    Ok(())
}

/// Test that mission pause/resume tools mutate mission state via the MCP router and
/// reject invalid pause attempts once a mission is already blocked.
#[tokio::test]
async fn test_mcp_mission_pause_resume_tools_enforce_state_guards()
-> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-mission-pause-resume");
    let ctx = McpTestContext::new(&town_name).await?;
    let storage =
        tinytown::mission::MissionStorage::new(ctx.town.channel().conn().clone(), &town_name);

    let mut mission =
        tinytown::mission::MissionRun::new(vec![tinytown::mission::ObjectiveRef::Doc {
            path: "docs/design.md".to_string(),
        }]);
    mission.start();
    storage.save_mission(&mission).await?;
    storage.add_active(mission.id).await?;

    let router = create_mcp_router(ctx.mcp_state.clone(), "tinytown-mcp", "0.5.0");
    let mut client = TestClient::from_router(router);
    client.initialize().await;

    let pause_result = client
        .call_tool_json(
            "mission.pause",
            json!({ "mission_id": mission.id.to_string() }),
        )
        .await;
    assert_eq!(pause_result["success"], true);
    assert_eq!(pause_result["data"]["directive"], "pause");

    let paused = storage.get_mission(mission.id).await?.unwrap();
    assert_eq!(paused.state, tinytown::mission::MissionState::Blocked);

    let pause_again = client
        .call_tool_json(
            "mission.pause",
            json!({ "mission_id": mission.id.to_string() }),
        )
        .await;
    assert_eq!(pause_again["success"], false);
    assert!(
        pause_again["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not running")
    );

    let resume_result = client
        .call_tool_json(
            "mission.resume",
            json!({ "mission_id": mission.id.to_string() }),
        )
        .await;
    assert_eq!(resume_result["success"], true);
    assert_eq!(resume_result["data"]["directive"], "resume");

    let resumed = storage.get_mission(mission.id).await?.unwrap();
    assert_eq!(resumed.state, tinytown::mission::MissionState::Running);

    Ok(())
}

/// Test that mission.resume rejects failed missions before queueing dispatcher work.
#[tokio::test]
async fn test_mcp_mission_resume_tool_rejects_failed_missions()
-> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-mission-resume-failed");
    let ctx = McpTestContext::new(&town_name).await?;
    let storage =
        tinytown::mission::MissionStorage::new(ctx.town.channel().conn().clone(), &town_name);

    let mut mission =
        tinytown::mission::MissionRun::new(vec![tinytown::mission::ObjectiveRef::Doc {
            path: "docs/design.md".to_string(),
        }]);
    mission.fail("Unrecoverable error");
    storage.save_mission(&mission).await?;

    let router = create_mcp_router(ctx.mcp_state.clone(), "tinytown-mcp", "0.5.0");
    let mut client = TestClient::from_router(router);
    client.initialize().await;

    let resume_result = client
        .call_tool_json(
            "mission.resume",
            json!({ "mission_id": mission.id.to_string() }),
        )
        .await;
    assert_eq!(resume_result["success"], false);
    assert!(
        resume_result["error"]
            .as_str()
            .unwrap_or_default()
            .contains("cannot be resumed")
    );

    let updated = storage.get_mission(mission.id).await?.unwrap();
    assert_eq!(updated.state, tinytown::mission::MissionState::Failed);
    assert!(storage.list_control_messages(mission.id).await?.is_empty());

    Ok(())
}

/// Test that the mission.pause MCP handler rejects terminal missions.
#[tokio::test]
async fn test_mcp_mission_pause_tool_rejects_terminal_mission()
-> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-mission-pause-terminal-test");
    let ctx = McpTestContext::new(&town_name).await?;

    let storage =
        tinytown::mission::MissionStorage::new(ctx.town.channel().conn().clone(), &town_name);
    let mut mission =
        tinytown::mission::MissionRun::new(vec![tinytown::mission::ObjectiveRef::Doc {
            path: "docs/design.md".to_string(),
        }]);
    mission.complete();
    storage.save_mission(&mission).await?;

    let tool = tinytown::app::mcp::tools::all_tools(ctx.mcp_state.clone())
        .into_iter()
        .find(|tool| tool.name == "mission.pause")
        .expect("mission.pause tool should exist");

    let result = tool
        .call(serde_json::json!({ "mission_id": mission.id.to_string() }))
        .await;

    let payload: serde_json::Value =
        serde_json::from_str(result.first_text().expect("text response"))?;
    assert_eq!(payload["success"], false);
    assert!(
        payload["error"]
            .as_str()
            .is_some_and(|text| text.contains("cannot be paused"))
    );

    Ok(())
}

/// Test that the mission.status MCP handler returns work and watch details by default.
#[tokio::test]
async fn test_mcp_mission_status_tool_returns_detailed_status()
-> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-mission-status-tool-test");
    let ctx = McpTestContext::new(&town_name).await?;

    let storage =
        tinytown::mission::MissionStorage::new(ctx.town.channel().conn().clone(), &town_name);
    let mut mission =
        tinytown::mission::MissionRun::new(vec![tinytown::mission::ObjectiveRef::Doc {
            path: "docs/design.md".to_string(),
        }]);
    mission.start();
    storage.save_mission(&mission).await?;
    storage.add_active(mission.id).await?;

    let work_items = tinytown::mission::build_mission_work_items(
        ctx.town.root(),
        mission.id,
        &mission.objective_refs,
    )?;
    storage.save_work_item(&work_items[0]).await?;

    let tool = tinytown::app::mcp::tools::all_tools(ctx.mcp_state.clone())
        .into_iter()
        .find(|tool| tool.name == "mission.status")
        .expect("mission.status tool should exist");

    let result = tool
        .call(serde_json::json!({ "mission_id": mission.id.to_string() }))
        .await;

    assert!(!result.is_error);
    let payload: serde_json::Value =
        serde_json::from_str(result.first_text().expect("text response"))?;
    let mission_id = mission.id.to_string();
    assert_eq!(
        payload["data"]["mission"]["id"].as_str(),
        Some(mission_id.as_str())
    );
    assert!(payload["data"]["work_items"].is_array());
    assert!(payload["data"]["watch_items"].is_array());

    Ok(())
}

// ============================================================================
// MCP ROUTER CREATION VERIFICATION
// ============================================================================

/// Test that the MCP router is configured correctly.
#[tokio::test]
async fn test_mcp_router_is_configured() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-config-test");
    let ctx = McpTestContext::new(&town_name).await?;

    // Create router with server info - if this doesn't panic, it's configured correctly
    let _router = create_mcp_router(ctx.mcp_state.clone(), "tinytown-mcp", "0.5.0");

    // The router creation completed successfully
    // Full testing of MCP protocol would require an MCP client connection
    Ok(())
}

/// Test that parity tools are registered in the MCP router inventory.
#[tokio::test]
async fn test_mcp_tool_inventory_includes_parity_tools() -> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-tool-inventory-test");
    let ctx = McpTestContext::new(&town_name).await?;

    let tool_names: HashSet<_> = tinytown::app::mcp::tools::all_tools(ctx.mcp_state.clone())
        .into_iter()
        .map(|tool| tool.name)
        .collect();

    for expected in [
        "agent.inbox",
        "task.list_pending",
        "agent.prune",
        "backlog.remove",
        "mission.list",
        "mission.status",
        "mission.get_status",
        "mission.work_items",
        "mission.watches",
        "mission.events",
        "mission.start",
        "mission.approve",
        "mission.reject",
        "mission.pause",
        "mission.resume",
        "mission.dispatch",
        "mission.note",
        "mission.input",
    ] {
        assert!(tool_names.contains(expected), "missing tool {expected}");
    }

    Ok(())
}

/// Test that mission resources are registered in the MCP router inventory.
#[tokio::test]
async fn test_mcp_resource_inventory_includes_mission_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let town_name = unique_town_name("mcp-resource-inventory-test");
    let ctx = McpTestContext::new(&town_name).await?;

    let resource_uris: HashSet<_> =
        tinytown::app::mcp::resources::all_resources(ctx.mcp_state.clone())
            .into_iter()
            .map(|resource| resource.uri)
            .collect();
    let template_uris: HashSet<_> =
        tinytown::app::mcp::resources::all_templates(ctx.mcp_state.clone())
            .into_iter()
            .map(|template| template.uri_template)
            .collect();

    assert!(resource_uris.contains("tinytown://missions"));
    assert!(template_uris.contains("tinytown://missions/{mission_id}"));

    Ok(())
}
