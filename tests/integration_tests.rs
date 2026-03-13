/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Integration tests for the tinytown orchestration system.
//!
//! These tests verify the core functionality of tinytown including:
//! - Town initialization and configuration
//! - Agent creation and state management
//! - Message passing through Redis channels
//! - Task assignment and lifecycle management

use std::time::Duration;
use tempfile::TempDir;
use tinytown::message::MessageType;
use tinytown::{
    Agent, AgentId, AgentState, AgentType, Message, Priority, Task, TaskId, TaskState, Town,
};

/// Wrapper that holds both Town and TempDir, cleaning up Redis on drop
struct TownGuard {
    town: Town,
    temp_dir: TempDir,
}

impl Drop for TownGuard {
    fn drop(&mut self) {
        cleanup_redis(&self.temp_dir);
    }
}

impl std::ops::Deref for TownGuard {
    type Target = Town;
    fn deref(&self) -> &Self::Target {
        &self.town
    }
}

/// Helper function to create a temporary town for testing.
/// Returns a TownGuard that cleans up Redis when dropped.
/// Uses TT_USE_SOCKET=1 to ensure tests use isolated per-town Redis instances.
async fn create_test_town(name: &str) -> Result<TownGuard, Box<dyn std::error::Error>> {
    // Force Unix socket mode for test isolation
    // Safety: Tests are run serially via serial_test where this matters
    unsafe {
        std::env::set_var("TT_USE_SOCKET", "1");
    }

    let temp_dir = TempDir::new()?;
    let town = Town::init(temp_dir.path(), name).await?;
    Ok(TownGuard { town, temp_dir })
}

/// Helper to kill Redis when test ends (only for per-town Redis, not central)
fn cleanup_redis(temp_dir: &TempDir) {
    let pid_file = temp_dir.path().join(".tt/redis.pid");
    if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        unsafe {
            // Use SIGKILL to ensure Redis dies immediately
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

// ============================================================================
// TOWN INITIALIZATION AND CONFIGURATION TESTS
// ============================================================================

/// Test that a town can be initialized with proper directory structure.
#[tokio::test]
async fn test_town_initialization() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let town_path = temp_dir.path();
    let town = Town::init(town_path, "test-town").await?;

    // All runtime artifacts go under .tt/
    assert!(town_path.join(".tt").exists());
    assert!(town_path.join(".tt/agents").exists());
    assert!(town_path.join(".tt/logs").exists());
    assert!(town_path.join(".tt/tasks").exists());
    assert!(town_path.join("tinytown.toml").exists());

    let config = town.config();
    assert_eq!(config.name, "test-town");
    assert_eq!(config.root, town_path);

    drop(town);
    cleanup_redis(&temp_dir);
    Ok(())
}

/// Test that a town can be connected to after initialization.
#[tokio::test]
async fn test_town_connect() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let town_path = temp_dir.path();

    let _town1 = Town::init(town_path, "connect-test").await?;
    let town2 = Town::connect(town_path).await?;

    let config = town2.config();
    assert_eq!(config.name, "connect-test");

    drop(town2);
    drop(_town1);
    cleanup_redis(&temp_dir);
    Ok(())
}

// ============================================================================
// AGENT CREATION AND STATE MANAGEMENT TESTS
// ============================================================================

/// Test that an agent can be spawned and has correct initial state.
#[tokio::test]
async fn test_agent_spawn() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("agent-spawn-test").await?;

    let agent_handle = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent_handle.id();
    assert_ne!(agent_id, AgentId::supervisor());

    let agent_state = agent_handle.state().await?;
    assert!(agent_state.is_some());

    let agent = agent_state.unwrap();
    assert_eq!(agent.name, "worker-1");
    assert_eq!(agent.cli, "claude");
    assert_eq!(agent.agent_type, AgentType::Worker);
    assert_eq!(agent.state, AgentState::Starting);
    assert_eq!(agent.tasks_completed, 0);

    Ok(())
}

/// Test that multiple agents can be spawned independently.
#[tokio::test]
async fn test_multiple_agents_spawn() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("multi-agent-test").await?;

    let agent1 = town.spawn_agent("worker-1", "claude").await?;
    let agent2 = town.spawn_agent("worker-2", "gemini").await?;
    let agent3 = town.spawn_agent("worker-3", "claude").await?;

    assert_ne!(agent1.id(), agent2.id());
    assert_ne!(agent2.id(), agent3.id());
    assert_ne!(agent1.id(), agent3.id());

    let state1 = agent1.state().await?;
    let state2 = agent2.state().await?;
    let state3 = agent3.state().await?;

    assert!(state1.is_some());
    assert!(state2.is_some());
    assert!(state3.is_some());

    Ok(())
}

/// Test that agent state can be updated and persisted.
#[tokio::test]
async fn test_agent_state_update() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("agent-state-test").await?;

    let _agent_handle = town.spawn_agent("worker-1", "claude").await?;

    let mut agent = Agent::new("worker-1", "claude", AgentType::Worker);
    agent.state = AgentState::Idle;
    agent.tasks_completed = 5;

    town.channel().set_agent_state(&agent).await?;

    let retrieved = town.channel().get_agent_state(agent.id).await?;
    assert!(retrieved.is_some());

    let retrieved_agent = retrieved.unwrap();
    assert_eq!(retrieved_agent.state, AgentState::Idle);
    assert_eq!(retrieved_agent.tasks_completed, 5);

    Ok(())
}

// ============================================================================
// MESSAGE PASSING THROUGH REDIS CHANNELS TESTS
// ============================================================================

/// Test that a message can be sent to an agent's inbox.
#[tokio::test]
async fn test_message_send() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-send-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    let msg = Message::new(AgentId::supervisor(), agent_id, MessageType::Ping);

    town.channel().send(&msg).await?;

    let inbox_len = agent.inbox_len().await?;
    assert_eq!(inbox_len, 1);

    Ok(())
}

/// Test that messages can be received from an agent's inbox.
#[tokio::test]
async fn test_message_receive() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-receive-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    let original_msg = Message::new(AgentId::supervisor(), agent_id, MessageType::Ping);
    town.channel().send(&original_msg).await?;

    // Use try_receive instead of blocking receive
    let received = town.channel().try_receive(agent_id).await?;

    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.id, original_msg.id);
    assert_eq!(msg.from, AgentId::supervisor());
    assert_eq!(msg.to, agent_id);

    Ok(())
}

/// Test that message priority affects queue ordering.
#[tokio::test]
async fn test_message_priority() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-priority-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    let low_msg = Message::new(AgentId::supervisor(), agent_id, MessageType::Ping)
        .with_priority(Priority::Low);

    let high_msg = Message::new(AgentId::supervisor(), agent_id, MessageType::Pong)
        .with_priority(Priority::High);

    town.channel().send(&low_msg).await?;
    town.channel().send(&high_msg).await?;

    // High priority messages are pushed to front (lpush), so try_receive gets them first
    let first = town.channel().try_receive(agent_id).await?.unwrap();

    assert_eq!(first.id, high_msg.id);

    Ok(())
}

/// Test that non-blocking message receive works correctly.
#[tokio::test]
async fn test_message_try_receive() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-try-receive-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    let empty = town.channel().try_receive(agent_id).await?;
    assert!(empty.is_none());

    let msg = Message::new(AgentId::supervisor(), agent_id, MessageType::Ping);
    town.channel().send(&msg).await?;

    let received = town.channel().try_receive(agent_id).await?;
    assert!(received.is_some());
    assert_eq!(received.unwrap().id, msg.id);

    Ok(())
}

/// Test that message correlation IDs work for request/response patterns.
#[tokio::test]
async fn test_message_correlation() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-correlation-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    let request = Message::new(AgentId::supervisor(), agent_id, MessageType::StatusRequest);
    let request_id = request.id;

    let response = Message::new(
        agent_id,
        AgentId::supervisor(),
        MessageType::StatusResponse {
            state: "idle".to_string(),
            current_task: None,
        },
    )
    .with_correlation(request_id);

    assert_eq!(response.correlation_id, Some(request_id));

    Ok(())
}

// ============================================================================
// TASK ASSIGNMENT AND LIFECYCLE TESTS
// ============================================================================

/// Test that a task can be created with proper initial state.
#[tokio::test]
async fn test_task_creation() -> Result<(), Box<dyn std::error::Error>> {
    let task = Task::new("Fix the bug in auth.rs");

    assert_eq!(task.description, "Fix the bug in auth.rs");
    assert_eq!(task.state, TaskState::Pending);
    assert!(task.assigned_to.is_none());
    assert!(task.result.is_none());
    assert!(task.completed_at.is_none());

    Ok(())
}

/// Test that a task can be assigned to an agent.
#[tokio::test]
async fn test_task_assignment() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("task-assignment-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let mut task = Task::new("Implement feature X");
    task.assign(agent.id());

    let task_id = agent.assign(task).await?;

    let stored_task = town.channel().get_task(task_id).await?;
    assert!(stored_task.is_some());

    let stored = stored_task.unwrap();
    assert_eq!(stored.description, "Implement feature X");
    assert_eq!(stored.state, TaskState::Assigned);
    assert_eq!(stored.assigned_to, Some(agent.id()));

    Ok(())
}

/// Test that multiple tasks can be assigned to an agent.
#[tokio::test]
async fn test_multiple_task_assignment() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("multi-task-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let task1 = Task::new("Task 1");
    let task2 = Task::new("Task 2");
    let task3 = Task::new("Task 3");

    let id1 = agent.assign(task1).await?;
    let id2 = agent.assign(task2).await?;
    let id3 = agent.assign(task3).await?;

    let stored1 = town.channel().get_task(id1).await?;
    let stored2 = town.channel().get_task(id2).await?;
    let stored3 = town.channel().get_task(id3).await?;

    assert!(stored1.is_some());
    assert!(stored2.is_some());
    assert!(stored3.is_some());

    assert_eq!(stored1.unwrap().description, "Task 1");
    assert_eq!(stored2.unwrap().description, "Task 2");
    assert_eq!(stored3.unwrap().description, "Task 3");

    Ok(())
}

/// Test that task state transitions work correctly.
#[tokio::test]
async fn test_task_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let _town = create_test_town("task-state-test").await?;

    let mut task = Task::new("Test task");

    assert_eq!(task.state, TaskState::Pending);

    let agent_id = AgentId::new();
    task.assign(agent_id);
    assert_eq!(task.state, TaskState::Assigned);
    assert_eq!(task.assigned_to, Some(agent_id));

    task.start();
    assert_eq!(task.state, TaskState::Running);
    assert!(task.started_at.is_some()); // Verify started_at is set when task becomes in-flight

    task.complete("Task completed successfully");
    assert_eq!(task.state, TaskState::Completed);
    assert_eq!(task.result, Some("Task completed successfully".to_string()));
    assert!(task.completed_at.is_some());

    Ok(())
}

/// Test that task failure state works correctly.
#[tokio::test]
async fn test_task_failure() -> Result<(), Box<dyn std::error::Error>> {
    let mut task = Task::new("Failing task");

    task.assign(AgentId::new());
    task.start();
    task.fail("Connection timeout");

    assert_eq!(task.state, TaskState::Failed);
    assert_eq!(task.result, Some("Connection timeout".to_string()));
    assert!(task.completed_at.is_some());

    Ok(())
}

/// Test that tasks can have tags for filtering.
#[tokio::test]
async fn test_task_tags() -> Result<(), Box<dyn std::error::Error>> {
    let task = Task::new("Implement API endpoint").with_tags(vec!["backend", "api", "urgent"]);

    assert_eq!(task.tags.len(), 3);
    assert!(task.tags.contains(&"backend".to_string()));
    assert!(task.tags.contains(&"api".to_string()));
    assert!(task.tags.contains(&"urgent".to_string()));

    Ok(())
}

/// Test that tasks can have parent tasks for hierarchical organization.
#[tokio::test]
async fn test_task_hierarchy() -> Result<(), Box<dyn std::error::Error>> {
    let parent_id = TaskId::new();
    let child_task = Task::new("Subtask").with_parent(parent_id);

    assert_eq!(child_task.parent_id, Some(parent_id));

    Ok(())
}

/// Test that task state is persisted in Redis.
#[tokio::test]
async fn test_task_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("task-persistence-test").await?;

    let mut task = Task::new("Persistent task");
    let agent_id = AgentId::new();
    task.assign(agent_id);

    town.channel().set_task(&task).await?;

    let retrieved = town.channel().get_task(task.id).await?;
    assert!(retrieved.is_some());

    let retrieved_task = retrieved.unwrap();
    assert_eq!(retrieved_task.description, "Persistent task");
    assert_eq!(retrieved_task.state, TaskState::Assigned);
    assert_eq!(retrieved_task.assigned_to, Some(agent_id));

    Ok(())
}

// ============================================================================
// INTEGRATION TESTS - COMBINED WORKFLOWS
// ============================================================================

/// Test a complete workflow: spawn agent, assign task, send messages.
#[tokio::test]
async fn test_complete_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("complete-workflow-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let mut task = Task::new("Implement new feature");
    task.assign(agent.id());
    let task_id = agent.assign(task).await?;

    let stored_task = town.channel().get_task(task_id).await?;
    assert!(stored_task.is_some());
    assert_eq!(stored_task.unwrap().state, TaskState::Assigned);

    agent.send(MessageType::StatusRequest).await?;

    // assign() sends a TaskAssign message, and send() sends a StatusRequest message
    let inbox_len = agent.inbox_len().await?;
    assert_eq!(inbox_len, 2);

    Ok(())
}

/// Test agent state transitions through message handling.
#[tokio::test]
async fn test_agent_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("agent-transitions-test").await?;

    let agent_handle = town.spawn_agent("worker-1", "claude").await?;

    let initial = agent_handle.state().await?;
    assert_eq!(initial.unwrap().state, AgentState::Starting);

    let mut agent = Agent::new("worker-1", "claude", AgentType::Worker);
    agent.id = agent_handle.id();
    agent.state = AgentState::Idle;
    town.channel().set_agent_state(&agent).await?;

    let idle = agent_handle.state().await?;
    assert_eq!(idle.unwrap().state, AgentState::Idle);

    agent.state = AgentState::Working;
    town.channel().set_agent_state(&agent).await?;

    let working = agent_handle.state().await?;
    assert_eq!(working.unwrap().state, AgentState::Working);

    Ok(())
}

/// Test task lifecycle with agent interaction.
#[tokio::test]
async fn test_task_lifecycle_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("task-lifecycle-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let mut task = Task::new("Complete task");
    assert_eq!(task.state, TaskState::Pending);

    task.assign(agent.id());
    assert_eq!(task.state, TaskState::Assigned);

    town.channel().set_task(&task).await?;

    task.start();
    town.channel().set_task(&task).await?;

    let running = town.channel().get_task(task.id).await?;
    assert_eq!(running.unwrap().state, TaskState::Running);

    task.complete("Successfully completed");
    town.channel().set_task(&task).await?;

    let completed = town.channel().get_task(task.id).await?;
    let completed_task = completed.unwrap();
    assert_eq!(completed_task.state, TaskState::Completed);
    assert_eq!(
        completed_task.result,
        Some("Successfully completed".to_string())
    );

    Ok(())
}

/// Test message inbox behavior with multiple messages.
#[tokio::test]
async fn test_message_inbox_ordering() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("inbox-ordering-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    for i in 0..3 {
        let msg = Message::new(
            AgentId::supervisor(),
            agent_id,
            MessageType::Custom {
                kind: "test".to_string(),
                payload: format!("message-{}", i),
            },
        );
        town.channel().send(&msg).await?;
    }

    let inbox_len = agent.inbox_len().await?;
    assert_eq!(inbox_len, 3);

    let _msg1 = town.channel().try_receive(agent_id).await?.unwrap();
    let _msg2 = town.channel().try_receive(agent_id).await?.unwrap();
    let _msg3 = town.channel().try_receive(agent_id).await?.unwrap();

    let final_len = agent.inbox_len().await?;
    assert_eq!(final_len, 0);

    Ok(())
}

/// Test that agent wait functionality works (with timeout).
#[tokio::test]
async fn test_agent_wait_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("agent-wait-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let mut agent_state = Agent::new("worker-1", "claude", AgentType::Worker);
    agent_state.id = agent.id();
    agent_state.state = AgentState::Idle;
    town.channel().set_agent_state(&agent_state).await?;

    let start = std::time::Instant::now();
    agent.wait().await?;
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(1));

    Ok(())
}

// ============================================================================
// ERROR HANDLING TESTS
// ============================================================================

/// Test that invalid task retrieval returns None.
#[tokio::test]
async fn test_task_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("task-not-found-test").await?;

    let fake_id = TaskId::new();
    let result = town.channel().get_task(fake_id).await?;

    assert!(result.is_none());

    Ok(())
}

/// Test that invalid agent retrieval returns None.
#[tokio::test]
async fn test_agent_state_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("agent-state-not-found-test").await?;

    let fake_id = AgentId::new();
    let result = town.channel().get_agent_state(fake_id).await?;

    assert!(result.is_none());

    Ok(())
}

/// Test that message receive timeout works correctly.
#[tokio::test]
async fn test_message_receive_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("message-timeout-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let start = std::time::Instant::now();
    let result = town
        .channel()
        .receive(agent.id(), Duration::from_millis(100))
        .await?;
    let elapsed = start.elapsed();

    assert!(result.is_none());
    assert!(elapsed >= Duration::from_millis(100));

    Ok(())
}

// ============================================================================
// EDGE CASES AND STRESS TESTS
// ============================================================================

/// Test that task state is terminal when completed.
#[tokio::test]
async fn test_task_terminal_states() -> Result<(), Box<dyn std::error::Error>> {
    let mut task1 = Task::new("Task 1");
    task1.complete("Done");
    assert!(task1.state.is_terminal());

    let mut task2 = Task::new("Task 2");
    task2.fail("Error");
    assert!(task2.state.is_terminal());

    let task3 = Task::new("Task 3");
    assert!(!task3.state.is_terminal());

    Ok(())
}

/// Test that agent state can be checked for work acceptance.
#[tokio::test]
async fn test_agent_can_accept_work() -> Result<(), Box<dyn std::error::Error>> {
    assert!(AgentState::Idle.can_accept_work());
    assert!(!AgentState::Working.can_accept_work());
    assert!(!AgentState::Paused.can_accept_work());
    assert!(!AgentState::Starting.can_accept_work());

    Ok(())
}

/// Test that agent state is terminal when stopped or errored.
#[tokio::test]
async fn test_agent_terminal_states() -> Result<(), Box<dyn std::error::Error>> {
    assert!(AgentState::Stopped.is_terminal());
    assert!(AgentState::Error.is_terminal());
    assert!(!AgentState::Idle.is_terminal());
    assert!(!AgentState::Working.is_terminal());

    Ok(())
}

/// Test creating many agents in sequence.
#[tokio::test]
async fn test_many_agents() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("many-agents-test").await?;

    let mut agent_ids = Vec::new();

    for i in 0..10 {
        let agent = town.spawn_agent(&format!("worker-{}", i), "claude").await?;
        agent_ids.push(agent.id());
    }

    let unique_count = agent_ids
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, 10);

    Ok(())
}

/// Test creating many tasks in sequence.
#[tokio::test]
async fn test_many_tasks() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("many-tasks-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;

    let mut task_ids = Vec::new();

    for i in 0..20 {
        let task = Task::new(format!("Task {}", i));
        let task_id = agent.assign(task).await?;
        task_ids.push(task_id);
    }

    let unique_count = task_ids
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, 20);

    Ok(())
}

/// Test sending many messages in sequence.
#[tokio::test]
async fn test_many_messages() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("many-messages-test").await?;

    let agent = town.spawn_agent("worker-1", "claude").await?;
    let agent_id = agent.id();

    for i in 0..50 {
        let msg = Message::new(
            AgentId::supervisor(),
            agent_id,
            MessageType::Custom {
                kind: "test".to_string(),
                payload: format!("msg-{}", i),
            },
        );
        town.channel().send(&msg).await?;
    }

    let inbox_len = agent.inbox_len().await?;
    assert_eq!(inbox_len, 50);

    Ok(())
}

// ============================================================================
// TASK PLANNING DSL TESTS
// ============================================================================

/// Test that tasks.toml can be initialized.
#[tokio::test]
async fn test_plan_init_tasks_file() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;

    // Initialize tasks file
    tinytown::plan::init_tasks_file(temp_dir.path())?;

    // Check file exists
    let tasks_file = temp_dir.path().join("tasks.toml");
    assert!(tasks_file.exists());

    // Load and verify structure
    let tasks = tinytown::plan::load_tasks_file(temp_dir.path())?;
    assert_eq!(tasks.meta.description, "Task plan for this project");
    assert_eq!(tasks.tasks.len(), 1);
    assert_eq!(tasks.tasks[0].id, "example-1");
    assert_eq!(tasks.tasks[0].status, "pending");

    Ok(())
}

/// Test loading and saving tasks file.
#[tokio::test]
async fn test_plan_load_save_tasks_file() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::plan::{TaskEntry, TasksFile, TasksMeta};

    let temp_dir = TempDir::new()?;

    // Create a custom tasks file
    let tasks = TasksFile {
        meta: TasksMeta {
            description: "Test plan".to_string(),
            default_agent: Some("developer".to_string()),
        },
        tasks: vec![
            TaskEntry {
                id: "task-1".to_string(),
                description: "Build the API".to_string(),
                agent: Some("backend".to_string()),
                status: "pending".to_string(),
                tags: vec!["api".to_string(), "backend".to_string()],
                parent: None,
            },
            TaskEntry {
                id: "task-2".to_string(),
                description: "Write tests".to_string(),
                agent: Some("tester".to_string()),
                status: "pending".to_string(),
                tags: vec!["tests".to_string()],
                parent: Some("task-1".to_string()),
            },
        ],
    };

    // Save
    tinytown::plan::save_tasks_file(temp_dir.path(), &tasks)?;

    // Load back
    let loaded = tinytown::plan::load_tasks_file(temp_dir.path())?;

    assert_eq!(loaded.meta.description, "Test plan");
    assert_eq!(loaded.meta.default_agent, Some("developer".to_string()));
    assert_eq!(loaded.tasks.len(), 2);
    assert_eq!(loaded.tasks[0].id, "task-1");
    assert_eq!(loaded.tasks[0].agent, Some("backend".to_string()));
    assert_eq!(loaded.tasks[1].parent, Some("task-1".to_string()));

    Ok(())
}

/// Test pushing tasks from file to Redis.
#[tokio::test]
async fn test_plan_push_to_redis() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("plan-push-test").await?;
    let town_path = town.config().root.clone();

    // Initialize and modify tasks file
    tinytown::plan::init_tasks_file(&town_path)?;

    let tasks = tinytown::plan::TasksFile {
        meta: tinytown::plan::TasksMeta {
            description: "Push test".to_string(),
            default_agent: None,
        },
        tasks: vec![tinytown::plan::TaskEntry {
            id: "push-task-1".to_string(),
            description: "Task to push".to_string(),
            agent: None,
            status: "pending".to_string(),
            tags: vec!["test".to_string()],
            parent: None,
        }],
    };
    tinytown::plan::save_tasks_file(&town_path, &tasks)?;

    // Push to Redis
    let count = tinytown::plan::push_tasks_to_redis(&town_path, town.channel()).await?;
    assert_eq!(count, 1);

    Ok(())
}

/// Test that default_cli is used when spawning without --cli.
#[tokio::test]
async fn test_default_cli_config() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::GlobalConfig;

    let town = create_test_town("default-cli-test").await?;

    // Config should have a default_cli that matches global config
    let config = town.config();
    let global = GlobalConfig::load().unwrap_or_default();
    assert!(!config.default_cli.is_empty());
    assert_eq!(config.default_cli, global.default_cli); // Should match global config

    // Agent CLIs should include built-in presets
    assert!(config.agent_clis.contains_key("claude"));
    assert!(config.agent_clis.contains_key("auggie"));
    assert!(config.agent_clis.contains_key("codex"));

    Ok(())
}

// ============================================================================
// RECOVERY FEATURE TESTS (tt recover)
// ============================================================================

/// Test that orphaned agents (in Working state with old heartbeat) can be detected.
#[tokio::test]
async fn test_detect_orphaned_agents_working_state() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("recovery-working-test").await?;

    // Create an agent and set it to Working state
    let agent_handle = town.spawn_agent("orphaned-worker", "claude").await?;
    let agent_id = agent_handle.id();

    // Get the agent and set it to Working state with old heartbeat
    let mut agent = Agent::new("orphaned-worker", "claude", AgentType::Worker);
    agent.id = agent_id;
    agent.state = AgentState::Working;
    // Set heartbeat to 3 minutes ago (stale - over 2 min threshold)
    agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(3);
    town.channel().set_agent_state(&agent).await?;

    // Verify the agent is in Working state
    let agents = town.list_agents().await;
    let orphaned = agents.iter().find(|a| a.id == agent_id);
    assert!(orphaned.is_some());
    assert_eq!(orphaned.unwrap().state, AgentState::Working);

    // The agent should be considered stale (heartbeat > 2 min ago)
    let heartbeat_age = chrono::Utc::now() - orphaned.unwrap().last_heartbeat;
    assert!(heartbeat_age.num_seconds() > 120);

    Ok(())
}

/// Test that agents in Starting state with old heartbeat are considered orphaned.
#[tokio::test]
async fn test_detect_orphaned_agents_starting_state() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("recovery-starting-test").await?;

    // Create an agent in Starting state with old heartbeat
    let agent_handle = town.spawn_agent("stuck-starting", "auggie").await?;
    let agent_id = agent_handle.id();

    let mut agent = Agent::new("stuck-starting", "auggie", AgentType::Worker);
    agent.id = agent_id;
    agent.state = AgentState::Starting;
    agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(5);
    town.channel().set_agent_state(&agent).await?;

    // Verify state is as expected
    let state = agent_handle.state().await?;
    assert!(state.is_some());
    assert_eq!(state.unwrap().state, AgentState::Starting);

    Ok(())
}

/// Test that agents in non-active states are not considered orphaned.
#[tokio::test]
async fn test_non_active_agents_not_orphaned() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("recovery-non-active-test").await?;

    // Create agents in various non-active states
    let idle_handle = town.spawn_agent("idle-agent", "claude").await?;
    let stopped_handle = town.spawn_agent("stopped-agent", "claude").await?;
    let paused_handle = town.spawn_agent("paused-agent", "claude").await?;

    // Set states
    let mut idle_agent = Agent::new("idle-agent", "claude", AgentType::Worker);
    idle_agent.id = idle_handle.id();
    idle_agent.state = AgentState::Idle;
    idle_agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(10);
    town.channel().set_agent_state(&idle_agent).await?;

    let mut stopped_agent = Agent::new("stopped-agent", "claude", AgentType::Worker);
    stopped_agent.id = stopped_handle.id();
    stopped_agent.state = AgentState::Stopped;
    stopped_agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(10);
    town.channel().set_agent_state(&stopped_agent).await?;

    let mut paused_agent = Agent::new("paused-agent", "claude", AgentType::Worker);
    paused_agent.id = paused_handle.id();
    paused_agent.state = AgentState::Paused;
    paused_agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(10);
    town.channel().set_agent_state(&paused_agent).await?;

    // Verify states - none of these should be considered "active" (Working or Starting)
    let agents = town.list_agents().await;

    for agent in &agents {
        let is_active_state = matches!(agent.state, AgentState::Working | AgentState::Starting);
        assert!(
            !is_active_state,
            "Agent {} should not be in an active state",
            agent.name
        );
    }

    Ok(())
}

/// Test that agent state can be transitioned from Working to Stopped (recovery action).
#[tokio::test]
async fn test_recover_agent_to_stopped() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("recovery-transition-test").await?;

    let agent_handle = town.spawn_agent("recoverable", "claude").await?;
    let agent_id = agent_handle.id();

    // Set to Working state
    let mut agent = Agent::new("recoverable", "claude", AgentType::Worker);
    agent.id = agent_id;
    agent.state = AgentState::Working;
    agent.last_heartbeat = chrono::Utc::now() - chrono::Duration::minutes(3);
    town.channel().set_agent_state(&agent).await?;

    // Verify initial state
    let state_before = agent_handle.state().await?;
    assert_eq!(state_before.unwrap().state, AgentState::Working);

    // Simulate recovery action: change state to Stopped
    agent.state = AgentState::Stopped;
    town.channel().set_agent_state(&agent).await?;

    // Verify recovery worked
    let state_after = agent_handle.state().await?;
    assert_eq!(state_after.unwrap().state, AgentState::Stopped);

    Ok(())
}

/// Test that no agents are orphaned when all are healthy.
#[tokio::test]
async fn test_no_orphans_when_healthy() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("recovery-healthy-test").await?;

    // Create agents with recent heartbeats
    let handle1 = town.spawn_agent("healthy-1", "claude").await?;
    let handle2 = town.spawn_agent("healthy-2", "auggie").await?;

    // Set to Working with recent heartbeat (within 2 min threshold)
    let mut agent1 = Agent::new("healthy-1", "claude", AgentType::Worker);
    agent1.id = handle1.id();
    agent1.state = AgentState::Working;
    agent1.last_heartbeat = chrono::Utc::now() - chrono::Duration::seconds(30);
    town.channel().set_agent_state(&agent1).await?;

    let mut agent2 = Agent::new("healthy-2", "auggie", AgentType::Worker);
    agent2.id = handle2.id();
    agent2.state = AgentState::Idle;
    agent2.last_heartbeat = chrono::Utc::now();
    town.channel().set_agent_state(&agent2).await?;

    // Check all agents - count orphaned (Working/Starting with stale heartbeat)
    let agents = town.list_agents().await;
    let mut orphan_count = 0;

    for agent in &agents {
        let is_active = matches!(agent.state, AgentState::Working | AgentState::Starting);
        if is_active {
            let heartbeat_age = chrono::Utc::now() - agent.last_heartbeat;
            if heartbeat_age.num_seconds() > 120 {
                orphan_count += 1;
            }
        }
    }

    assert_eq!(
        orphan_count, 0,
        "No agents should be orphaned when heartbeats are recent"
    );

    Ok(())
}

// ============================================================================
// TOWNS REGISTRY TESTS (tt towns, tt init registration)
// ============================================================================

/// Test that towns.toml format is valid.
#[tokio::test]
async fn test_towns_toml_format() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that the towns.toml format can be parsed
    let toml_content = r#"
[[towns]]
path = "/path/to/town1"
name = "my-town"

[[towns]]
path = "/path/to/town2"
name = "another-town"
"#;

    #[derive(Debug, Clone, serde::Deserialize)]
    struct TownEntry {
        path: String,
        name: String,
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    struct TownsFile {
        towns: Vec<TownEntry>,
    }

    let parsed: TownsFile = toml::from_str(toml_content)?;
    assert_eq!(parsed.towns.len(), 2);
    assert_eq!(parsed.towns[0].name, "my-town");
    assert_eq!(parsed.towns[0].path, "/path/to/town1");
    assert_eq!(parsed.towns[1].name, "another-town");
    assert_eq!(parsed.towns[1].path, "/path/to/town2");

    Ok(())
}

/// Test that empty towns.toml is valid.
#[tokio::test]
async fn test_empty_towns_toml() -> Result<(), Box<dyn std::error::Error>> {
    #[derive(Debug, Clone, serde::Deserialize, Default)]
    struct TownsFile {
        #[serde(default)]
        towns: Vec<TownEntry>,
    }

    #[allow(dead_code)]
    #[derive(Debug, Clone, serde::Deserialize)]
    struct TownEntry {
        path: String,
        name: String,
    }

    // Empty file or just whitespace should parse to default
    let empty_content = "";
    let parsed: TownsFile = toml::from_str(empty_content).unwrap_or_default();
    assert_eq!(parsed.towns.len(), 0);

    // File with just towns = [] should also work
    let explicit_empty = "towns = []";
    let parsed2: TownsFile = toml::from_str(explicit_empty)?;
    assert_eq!(parsed2.towns.len(), 0);

    Ok(())
}

/// Test that global config directory constant is accessible.
#[tokio::test]
async fn test_global_config_dir_constant() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::global_config::GLOBAL_CONFIG_DIR;

    assert_eq!(GLOBAL_CONFIG_DIR, ".tt");

    Ok(())
}

/// Test that GlobalConfig Default trait works (note: serde defaults are separate).
#[tokio::test]
async fn test_global_config_defaults() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::global_config::GlobalConfig;

    // Default trait gives empty strings (Rust default)
    let config = GlobalConfig::default();
    assert!(config.agent_clis.is_empty());

    // The load() method uses serde defaults when file doesn't exist
    // We can test that GlobalConfig can be serialized/deserialized with defaults
    let toml_str = r#"
default_cli = "claude"
"#;
    let parsed: GlobalConfig = toml::from_str(toml_str)?;
    assert_eq!(parsed.default_cli, "claude");

    Ok(())
}

/// Test that town initialization creates expected directories.
#[tokio::test]
async fn test_town_init_creates_structure() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let town_path = temp_dir.path();

    let _town = Town::init(town_path, "init-structure-test").await?;

    // Verify expected directories exist (all under .tt/)
    assert!(town_path.join(".tt").exists());
    assert!(town_path.join(".tt/agents").exists());
    assert!(town_path.join(".tt/logs").exists());
    assert!(town_path.join(".tt/tasks").exists());

    // Verify config file exists (note: uses .toml now, not .json)
    let toml_config = town_path.join("tinytown.toml");
    let json_config = town_path.join("tinytown.json");
    assert!(toml_config.exists() || json_config.exists());

    drop(_town);
    cleanup_redis(&temp_dir);
    Ok(())
}

/// Test that towns can be connected to and have proper status.
#[tokio::test]
async fn test_town_status_info() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("status-info-test").await?;

    // Spawn some agents
    let _agent1 = town.spawn_agent("worker-1", "claude").await?;
    let _agent2 = town.spawn_agent("worker-2", "auggie").await?;

    // List agents should return them
    let agents = town.list_agents().await;
    assert_eq!(agents.len(), 2);

    // Config should have expected values
    let config = town.config();
    assert_eq!(config.name, "status-info-test");

    Ok(())
}

/// Test that town can report agent activity states for recovery.
#[tokio::test]
async fn test_town_agent_activity_report() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("activity-report-test").await?;

    // Create agents in different states
    let active_handle = town.spawn_agent("active-worker", "claude").await?;
    let idle_handle = town.spawn_agent("idle-worker", "claude").await?;

    // Set up states
    let mut active_agent = Agent::new("active-worker", "claude", AgentType::Worker);
    active_agent.id = active_handle.id();
    active_agent.state = AgentState::Working;
    active_agent.last_heartbeat = chrono::Utc::now();
    town.channel().set_agent_state(&active_agent).await?;

    let mut idle_agent = Agent::new("idle-worker", "claude", AgentType::Worker);
    idle_agent.id = idle_handle.id();
    idle_agent.state = AgentState::Idle;
    idle_agent.last_heartbeat = chrono::Utc::now();
    town.channel().set_agent_state(&idle_agent).await?;

    // Get agents and count by state
    let agents = town.list_agents().await;
    let working_count = agents
        .iter()
        .filter(|a| a.state == AgentState::Working)
        .count();
    let idle_count = agents
        .iter()
        .filter(|a| a.state == AgentState::Idle)
        .count();

    assert_eq!(working_count, 1);
    assert_eq!(idle_count, 1);

    Ok(())
}

// ============================================================================
// BACKLOG AND RECOVERY TESTS
// ============================================================================

/// Test that tasks can be added, listed, and claimed from the backlog.
#[tokio::test]
async fn test_backlog_add_list_claim() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("backlog-test").await?;

    let channel = town.channel();

    // Initially, backlog should be empty
    let backlog = channel.backlog_list().await?;
    assert!(backlog.is_empty());
    assert_eq!(channel.backlog_len().await?, 0);

    // Add tasks to the backlog
    let task1_id = TaskId::new();
    let task2_id = TaskId::new();
    let task3_id = TaskId::new();

    channel.backlog_push(task1_id).await?;
    channel.backlog_push(task2_id).await?;
    channel.backlog_push(task3_id).await?;

    // Verify backlog has 3 tasks
    assert_eq!(channel.backlog_len().await?, 3);
    let backlog = channel.backlog_list().await?;
    assert_eq!(backlog.len(), 3);
    assert_eq!(backlog[0], task1_id);
    assert_eq!(backlog[1], task2_id);
    assert_eq!(backlog[2], task3_id);

    // Pop (claim) a task from the backlog (FIFO)
    let claimed = channel.backlog_pop().await?;
    assert!(claimed.is_some());
    assert_eq!(claimed.unwrap(), task1_id);
    assert_eq!(channel.backlog_len().await?, 2);

    // Remove a specific task
    let removed = channel.backlog_remove(task3_id).await?;
    assert!(removed);
    assert_eq!(channel.backlog_len().await?, 1);

    // Verify only task2 remains
    let backlog = channel.backlog_list().await?;
    assert_eq!(backlog.len(), 1);
    assert_eq!(backlog[0], task2_id);

    // Pop remaining task
    let claimed = channel.backlog_pop().await?;
    assert_eq!(claimed, Some(task2_id));

    // Backlog should be empty now
    assert_eq!(channel.backlog_len().await?, 0);
    let empty_pop = channel.backlog_pop().await?;
    assert!(empty_pop.is_none());

    Ok(())
}

/// Test that tasks can be reclaimed (drained) from a dead agent's inbox.
#[tokio::test]
async fn test_reclaim_from_dead_agent() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("reclaim-test").await?;

    let channel = town.channel();

    // Create an agent and send some messages to it
    let agent_handle = town.spawn_agent("dead-worker", "claude").await?;
    let agent_id = agent_handle.id();

    // Send multiple messages to the agent
    let msg1 = Message::new(AgentId::supervisor(), agent_id, MessageType::Ping);
    let msg2 = Message::new(
        AgentId::supervisor(),
        agent_id,
        MessageType::TaskAssign {
            task_id: "task-1".to_string(),
        },
    );
    let msg3 = Message::new(
        AgentId::supervisor(),
        agent_id,
        MessageType::TaskAssign {
            task_id: "task-2".to_string(),
        },
    );

    channel.send(&msg1).await?;
    channel.send(&msg2).await?;
    channel.send(&msg3).await?;

    // Verify messages are in inbox
    let inbox_len = agent_handle.inbox_len().await?;
    assert_eq!(inbox_len, 3);

    // Simulate agent death by setting state to Stopped
    let mut agent = Agent::new("dead-worker", "claude", AgentType::Worker);
    agent.id = agent_id;
    agent.state = AgentState::Stopped;
    channel.set_agent_state(&agent).await?;

    // Drain the inbox (reclaim messages)
    let drained = channel.drain_inbox(agent_id).await?;
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0].id, msg1.id);
    assert_eq!(drained[1].id, msg2.id);
    assert_eq!(drained[2].id, msg3.id);

    // Inbox should be empty after drain
    let inbox_len = agent_handle.inbox_len().await?;
    assert_eq!(inbox_len, 0);

    Ok(())
}

/// Test that drained messages can be moved to the backlog.
#[tokio::test]
async fn test_reclaim_to_backlog() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("reclaim-backlog-test").await?;

    let channel = town.channel();

    // Create an agent and send task messages
    let agent_handle = town.spawn_agent("failing-worker", "claude").await?;
    let agent_id = agent_handle.id();

    // Create task IDs and send as TaskAssign messages
    let task1_id = TaskId::new();
    let task2_id = TaskId::new();
    let msg1 = Message::new(
        AgentId::supervisor(),
        agent_id,
        MessageType::TaskAssign {
            task_id: task1_id.to_string(),
        },
    );
    let msg2 = Message::new(
        AgentId::supervisor(),
        agent_id,
        MessageType::TaskAssign {
            task_id: task2_id.to_string(),
        },
    );

    channel.send(&msg1).await?;
    channel.send(&msg2).await?;

    // Simulate agent death
    let mut agent = Agent::new("failing-worker", "claude", AgentType::Worker);
    agent.id = agent_id;
    agent.state = AgentState::Error;
    channel.set_agent_state(&agent).await?;

    // Drain inbox and move task IDs to backlog
    let drained = channel.drain_inbox(agent_id).await?;
    for msg in &drained {
        if let MessageType::TaskAssign { task_id: task_str } = &msg.msg_type
            && let Ok(task_id) = task_str.parse::<TaskId>()
        {
            channel.backlog_push(task_id).await?;
        }
    }

    // Verify tasks are in backlog
    let backlog = channel.backlog_list().await?;
    assert_eq!(backlog.len(), 2);
    assert_eq!(backlog[0], task1_id);
    assert_eq!(backlog[1], task2_id);

    Ok(())
}

/// Test that a message can be moved from one agent to another.
#[tokio::test]
async fn test_move_message_to_inbox() -> Result<(), Box<dyn std::error::Error>> {
    let town = create_test_town("move-message-test").await?;

    let channel = town.channel();

    // Create two agents
    let agent1 = town.spawn_agent("worker-1", "claude").await?;
    let agent2 = town.spawn_agent("worker-2", "claude").await?;
    let agent1_id = agent1.id();
    let agent2_id = agent2.id();

    // Send a message to agent1
    let original_msg = Message::new(
        AgentId::supervisor(),
        agent1_id,
        MessageType::TaskAssign {
            task_id: "important-task".to_string(),
        },
    );
    channel.send(&original_msg).await?;

    // Verify message is in agent1's inbox
    assert_eq!(agent1.inbox_len().await?, 1);
    assert_eq!(agent2.inbox_len().await?, 0);

    // Drain from agent1 and move to agent2
    let drained = channel.drain_inbox(agent1_id).await?;
    assert_eq!(drained.len(), 1);

    // Move the message to agent2
    channel
        .move_message_to_inbox(&drained[0], agent2_id)
        .await?;

    // Verify message moved
    assert_eq!(agent1.inbox_len().await?, 0);
    assert_eq!(agent2.inbox_len().await?, 1);

    // Receive from agent2 and verify content preserved
    let received = channel.try_receive(agent2_id).await?;
    assert!(received.is_some());
    let msg = received.unwrap();
    match msg.msg_type {
        MessageType::TaskAssign { task_id } => assert_eq!(task_id, "important-task"),
        _ => panic!("Expected TaskAssign message type"),
    }

    Ok(())
}

// ============================================================================
// TCP REDIS CONFIGURATION TESTS
// ============================================================================

/// Test that redis_url() returns Unix socket URL when use_socket is true.
#[tokio::test]
async fn test_redis_url_unix_socket() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Explicitly set Unix socket mode (may not be default if global config uses central Redis)
    config.redis = RedisConfig {
        use_socket: true,
        socket_path: "redis.sock".to_string(),
        host: "127.0.0.1".to_string(),
        port: 6379,
        persist: false,
        aof_path: "redis.aof".to_string(),
        password: None,
        tls_enabled: false,
        tls_cert: None,
        tls_key: None,
        tls_ca_cert: None,
        bind: "127.0.0.1".to_string(),
    };

    assert!(config.redis.use_socket);
    let url = config.redis_url();
    assert!(
        url.starts_with("unix://"),
        "Expected unix:// URL, got: {}",
        url
    );
    assert!(
        url.contains("redis.sock"),
        "Expected socket path in URL, got: {}",
        url
    );

    Ok(())
}

/// Test that redis_url() returns TCP URL without password.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_url_tcp_no_password() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up env var first
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TCP mode without password
    config.redis = RedisConfig {
        use_socket: false,
        host: "127.0.0.1".to_string(),
        port: 6380,
        password: None,
        tls_enabled: false,
        ..Default::default()
    };

    let url = config.redis_url();
    assert_eq!(url, "redis://127.0.0.1:6380", "Unexpected URL: {}", url);

    Ok(())
}

/// Test that redis_url() returns TCP URL with password.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_url_tcp_with_password() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up env var first
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TCP mode with password
    config.redis = RedisConfig {
        use_socket: false,
        host: "localhost".to_string(),
        port: 6379,
        password: Some("secret123".to_string()),
        tls_enabled: false,
        ..Default::default()
    };

    let url = config.redis_url();
    assert_eq!(
        url, "redis://:secret123@localhost:6379",
        "Unexpected URL: {}",
        url
    );

    Ok(())
}

/// Test that redis_url() returns TLS URL (rediss scheme) when TLS is enabled.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_url_tls_enabled() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up env var first
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TLS mode
    config.redis = RedisConfig {
        use_socket: false,
        host: "redis.example.com".to_string(),
        port: 6379,
        password: Some("tls-password".to_string()),
        tls_enabled: true,
        ..Default::default()
    };

    let url = config.redis_url();
    assert!(
        url.starts_with("rediss://"),
        "Expected rediss:// scheme, got: {}",
        url
    );
    assert_eq!(url, "rediss://:tls-password@redis.example.com:6379");

    Ok(())
}

/// Test is_remote_redis() correctly identifies local vs remote Redis.
#[tokio::test]
async fn test_is_remote_redis() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Unix socket is not remote
    config.redis.use_socket = true;
    assert!(
        !config.is_remote_redis(),
        "Unix socket should not be remote"
    );

    // localhost is not remote
    config.redis = RedisConfig {
        use_socket: false,
        host: "localhost".to_string(),
        port: 6379,
        ..Default::default()
    };
    assert!(!config.is_remote_redis(), "localhost should not be remote");

    // 127.0.0.1 is not remote
    config.redis.host = "127.0.0.1".to_string();
    assert!(!config.is_remote_redis(), "127.0.0.1 should not be remote");

    // 127.0.1.1 is not remote (any 127.x.x.x)
    config.redis.host = "127.0.1.1".to_string();
    assert!(!config.is_remote_redis(), "127.x.x.x should not be remote");

    // External host IS remote
    config.redis.host = "redis.example.com".to_string();
    assert!(
        config.is_remote_redis(),
        "redis.example.com should be remote"
    );

    // IP address IS remote
    config.redis.host = "192.168.1.100".to_string();
    assert!(config.is_remote_redis(), "192.168.1.100 should be remote");

    Ok(())
}

/// Test that default RedisConfig has expected defaults.
#[tokio::test]
async fn test_redis_config_defaults() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::config::RedisConfig;

    let config = RedisConfig::default();

    // Unix socket is default (under .tt/)
    assert!(config.use_socket, "Default should use Unix socket");
    assert_eq!(config.socket_path, ".tt/redis.sock");

    // TCP defaults
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 6379);

    // Security defaults - disabled by default
    assert!(
        config.password.is_none(),
        "Password should be None by default"
    );
    assert!(!config.tls_enabled, "TLS should be disabled by default");
    assert!(config.tls_cert.is_none());
    assert!(config.tls_key.is_none());
    assert!(config.tls_ca_cert.is_none());

    // Bind defaults to localhost for security
    assert_eq!(config.bind, "127.0.0.1");

    Ok(())
}

/// Test redis_url() uses env var password over config password.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_password_env_var_override() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up any existing env var first
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TCP mode with config password
    config.redis = RedisConfig {
        use_socket: false,
        host: "localhost".to_string(),
        port: 6379,
        password: Some("config-password".to_string()),
        tls_enabled: false,
        ..Default::default()
    };

    // Set env var to override
    // Safety: This is a serial test
    unsafe {
        std::env::set_var("TINYTOWN_REDIS_PASSWORD", "env-password");
    }

    // redis_password() should return env var
    assert_eq!(
        config.redis_password(),
        Some("env-password".to_string()),
        "Env var should override config password"
    );

    // URL should use env var password
    let url = config.redis_url();
    assert!(
        url.contains("env-password"),
        "URL should use env var password, got: {}",
        url
    );
    assert!(
        !url.contains("config-password"),
        "URL should NOT use config password, got: {}",
        url
    );

    // Clean up env var
    // Safety: This is a single-threaded test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    // Now it should use config password
    assert_eq!(
        config.redis_password(),
        Some("config-password".to_string()),
        "After removing env var, should use config password"
    );

    Ok(())
}

/// Test that redis_url_redacted() properly masks passwords.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_url_redacted_masks_password() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up any env var first to ensure test isolation
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TCP mode with password
    config.redis = RedisConfig {
        use_socket: false,
        host: "redis.example.com".to_string(),
        port: 6379,
        password: Some("super-secret-password".to_string()),
        tls_enabled: false,
        ..Default::default()
    };

    // redis_url() contains real password
    let real_url = config.redis_url();
    assert!(
        real_url.contains("super-secret-password"),
        "Real URL should contain password"
    );

    // redis_url_redacted() should mask it
    let redacted_url = config.redis_url_redacted();
    assert!(
        !redacted_url.contains("super-secret-password"),
        "Redacted URL should NOT contain password"
    );
    assert!(
        redacted_url.contains("****"),
        "Redacted URL should contain mask: {}",
        redacted_url
    );
    assert_eq!(redacted_url, "redis://:****@redis.example.com:6379");

    // TLS mode should also be redacted properly
    config.redis.tls_enabled = true;
    let redacted_tls = config.redis_url_redacted();
    assert!(redacted_tls.starts_with("rediss://"));
    assert!(redacted_tls.contains("****"));
    assert!(!redacted_tls.contains("super-secret-password"));

    Ok(())
}

/// Test that redis_url_redacted() returns normal URL when no password is set.
#[tokio::test]
#[serial_test::serial]
async fn test_redis_url_redacted_no_password() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::Config;
    use tinytown::config::RedisConfig;

    // Clean up any env var first to ensure test isolation
    // Safety: This is a serial test
    unsafe {
        std::env::remove_var("TINYTOWN_REDIS_PASSWORD");
    }

    let temp_dir = TempDir::new()?;
    let mut config = Config::new("test-town", temp_dir.path());

    // Configure TCP mode without password
    config.redis = RedisConfig {
        use_socket: false,
        host: "localhost".to_string(),
        port: 6379,
        password: None,
        tls_enabled: false,
        ..Default::default()
    };

    // Both should be the same when no password
    let real_url = config.redis_url();
    let redacted_url = config.redis_url_redacted();
    assert_eq!(real_url, redacted_url, "URLs should match when no password");
    assert_eq!(real_url, "redis://localhost:6379");

    Ok(())
}

// ============================================================================
// MISSION MODULE TESTS
// ============================================================================

/// Test MissionId creation and parsing.
#[test]
fn test_mission_id_creation_and_parsing() {
    use tinytown::mission::{MissionId, WatchId, WorkItemId};
    use uuid::Uuid;

    // Test MissionId
    let id1 = MissionId::new();
    let id2 = MissionId::new();
    assert_ne!(id1, id2, "Each new ID should be unique");

    // Test from_uuid
    let uuid = Uuid::new_v4();
    let id_from_uuid = MissionId::from_uuid(uuid);
    assert_eq!(format!("{}", id_from_uuid), format!("{}", uuid));

    // Test Display and FromStr roundtrip
    let id_str = id1.to_string();
    let parsed: MissionId = id_str.parse().expect("Should parse MissionId");
    assert_eq!(id1, parsed);

    // Test Default
    let default_id = MissionId::default();
    assert_ne!(default_id, id1, "Default should create new ID");

    // Test WorkItemId similarly
    let work_id = WorkItemId::new();
    let work_str = work_id.to_string();
    let parsed_work: WorkItemId = work_str.parse().expect("Should parse WorkItemId");
    assert_eq!(work_id, parsed_work);

    // Test WatchId similarly
    let watch_id = WatchId::new();
    let watch_str = watch_id.to_string();
    let parsed_watch: WatchId = watch_str.parse().expect("Should parse WatchId");
    assert_eq!(watch_id, parsed_watch);
}

/// Test ObjectiveRef display formatting.
#[test]
fn test_objective_ref_display() {
    use tinytown::mission::ObjectiveRef;

    let issue_ref = ObjectiveRef::Issue {
        owner: "redis-field-engineering".into(),
        repo: "tinytown".into(),
        number: 42,
    };
    assert_eq!(
        format!("{}", issue_ref),
        "redis-field-engineering/tinytown#42"
    );

    let doc_ref = ObjectiveRef::Doc {
        path: "docs/design.md".into(),
    };
    assert_eq!(format!("{}", doc_ref), "docs/design.md");
}

/// Test MissionRun creation and state transitions.
#[test]
fn test_mission_run_state_transitions() {
    use tinytown::mission::{MissionRun, MissionState, ObjectiveRef};

    let objectives = vec![ObjectiveRef::Issue {
        owner: "owner".into(),
        repo: "repo".into(),
        number: 1,
    }];

    let mut mission = MissionRun::new(objectives.clone());
    assert_eq!(mission.state, MissionState::Planning);
    assert!(mission.blocked_reason.is_none());
    assert_eq!(mission.objective_refs.len(), 1);

    // Test start transition
    mission.start();
    assert_eq!(mission.state, MissionState::Running);

    // Test block transition
    mission.block("Waiting for CI");
    assert_eq!(mission.state, MissionState::Blocked);
    assert_eq!(mission.blocked_reason.as_deref(), Some("Waiting for CI"));

    // Test complete transition
    mission.complete();
    assert_eq!(mission.state, MissionState::Completed);
    assert!(mission.blocked_reason.is_none());

    // Test fail transition (from fresh mission)
    let mut mission2 = MissionRun::new(objectives);
    mission2.fail("Unrecoverable error");
    assert_eq!(mission2.state, MissionState::Failed);
    assert_eq!(
        mission2.blocked_reason.as_deref(),
        Some("Unrecoverable error")
    );
}

/// Test MissionRun with custom policy.
#[test]
fn test_mission_run_with_policy() {
    use tinytown::mission::{MissionPolicy, MissionRun, ObjectiveRef};

    let objectives = vec![ObjectiveRef::Doc {
        path: "README.md".into(),
    }];
    let policy = MissionPolicy {
        max_parallel_items: 5,
        reviewer_required: false,
        auto_merge: true,
        watch_interval_secs: 60,
    };

    let mission = MissionRun::new(objectives).with_policy(policy.clone());
    assert_eq!(mission.policy.max_parallel_items, 5);
    assert!(!mission.policy.reviewer_required);
    assert!(mission.policy.auto_merge);
    assert_eq!(mission.policy.watch_interval_secs, 60);
}

/// Test WorkItem creation and state transitions.
#[test]
fn test_work_item_state_transitions() {
    use tinytown::AgentId;
    use tinytown::mission::{MissionId, WorkItem, WorkKind, WorkStatus};

    let mission_id = MissionId::new();
    let mut work_item = WorkItem::new(mission_id, "Implement feature", WorkKind::Implement);

    assert_eq!(work_item.status, WorkStatus::Pending);
    assert!(!work_item.status.is_terminal());
    assert!(!work_item.status.is_ready());
    assert!(work_item.assigned_to.is_none());
    assert!(work_item.artifact_refs.is_empty());

    // Test mark_ready
    work_item.mark_ready();
    assert_eq!(work_item.status, WorkStatus::Ready);
    assert!(work_item.status.is_ready());

    // Test assign
    let agent_id = AgentId::new();
    work_item.assign(agent_id);
    assert_eq!(work_item.status, WorkStatus::Assigned);
    assert_eq!(work_item.assigned_to, Some(agent_id));

    // Test start
    work_item.start();
    assert_eq!(work_item.status, WorkStatus::Running);

    // Test block
    work_item.block();
    assert_eq!(work_item.status, WorkStatus::Blocked);

    // Test complete
    work_item.complete(vec!["https://github.com/owner/repo/pull/1".into()]);
    assert_eq!(work_item.status, WorkStatus::Done);
    assert!(work_item.status.is_terminal());
    assert_eq!(work_item.artifact_refs.len(), 1);
}

/// Test WorkItem builder methods.
#[test]
fn test_work_item_builder_methods() {
    use tinytown::mission::{MissionId, WorkItem, WorkItemId, WorkKind};

    let mission_id = MissionId::new();
    let dep1 = WorkItemId::new();
    let dep2 = WorkItemId::new();

    let work_item = WorkItem::new(mission_id, "Test feature", WorkKind::Test)
        .with_dependencies(vec![dep1, dep2])
        .with_owner_role("tester")
        .with_source_ref("owner/repo#42");

    assert_eq!(work_item.depends_on.len(), 2);
    assert!(work_item.depends_on.contains(&dep1));
    assert!(work_item.depends_on.contains(&dep2));
    assert_eq!(work_item.owner_role.as_deref(), Some("tester"));
    assert_eq!(work_item.source_ref.as_deref(), Some("owner/repo#42"));
    assert_eq!(work_item.kind, WorkKind::Test);
}

/// Test WatchItem creation and check scheduling.
#[test]
fn test_watch_item_scheduling() {
    use tinytown::mission::{
        MissionId, TriggerAction, WatchItem, WatchKind, WatchStatus, WorkItemId,
    };

    let mission_id = MissionId::new();
    let work_item_id = WorkItemId::new();

    // Create watch with 1 second interval for test
    let watch = WatchItem::new(
        mission_id,
        work_item_id,
        WatchKind::PrChecks,
        "https://github.com/owner/repo/pull/1",
        1,
    );

    assert_eq!(watch.status, WatchStatus::Active);
    assert_eq!(watch.kind, WatchKind::PrChecks);
    assert!(watch.last_check_at.is_none());
    assert_eq!(watch.consecutive_failures, 0);

    // Test with_trigger
    let watch_with_trigger = WatchItem::new(
        mission_id,
        work_item_id,
        WatchKind::ReviewComments,
        "pr/1",
        60,
    )
    .with_trigger(TriggerAction::NotifyReviewer);

    assert_eq!(watch_with_trigger.on_trigger, TriggerAction::NotifyReviewer);
}

/// Test WatchItem check recording.
#[test]
fn test_watch_item_check_recording() {
    use chrono::Utc;
    use tinytown::mission::{MissionId, WatchItem, WatchKind, WorkItemId};

    let mission_id = MissionId::new();
    let work_item_id = WorkItemId::new();

    let mut watch = WatchItem::new(mission_id, work_item_id, WatchKind::PrChecks, "pr/1", 60);

    // Record successful check
    let before_check = Utc::now();
    watch.record_check();
    assert!(watch.last_check_at.is_some());
    assert!(watch.last_check_at.unwrap() >= before_check);
    assert_eq!(watch.consecutive_failures, 0);
    // Next due should be ~60 seconds from now
    assert!(watch.next_due_at > before_check);

    // Record failures with backoff
    watch.record_failure();
    assert_eq!(watch.consecutive_failures, 1);

    watch.record_failure();
    assert_eq!(watch.consecutive_failures, 2);

    watch.record_failure();
    assert_eq!(watch.consecutive_failures, 3);
}

/// Test WatchItem snooze and complete.
#[test]
fn test_watch_item_snooze_and_complete() {
    use chrono::Utc;
    use tinytown::mission::{MissionId, WatchItem, WatchKind, WatchStatus, WorkItemId};

    let mission_id = MissionId::new();
    let work_item_id = WorkItemId::new();

    let mut watch = WatchItem::new(
        mission_id,
        work_item_id,
        WatchKind::Mergeability,
        "pr/1",
        60,
    );

    // Test snooze
    watch.snooze(300);
    assert_eq!(watch.status, WatchStatus::Snoozed);
    assert!(watch.next_due_at > Utc::now());

    // Test complete
    watch.complete();
    assert_eq!(watch.status, WatchStatus::Done);
}

/// Test WorkKind and WorkStatus variants.
#[test]
fn test_work_kind_and_status_variants() {
    use tinytown::mission::{WorkKind, WorkStatus};

    // Test all WorkKind variants exist
    let kinds = [
        WorkKind::Design,
        WorkKind::Implement,
        WorkKind::Test,
        WorkKind::Review,
        WorkKind::MergeGate,
        WorkKind::Followup,
    ];
    assert_eq!(kinds.len(), 6);
    assert_eq!(WorkKind::default(), WorkKind::Implement);

    // Test all WorkStatus variants
    let statuses = [
        WorkStatus::Pending,
        WorkStatus::Ready,
        WorkStatus::Assigned,
        WorkStatus::Running,
        WorkStatus::Blocked,
        WorkStatus::Done,
    ];
    assert_eq!(statuses.len(), 6);
    assert_eq!(WorkStatus::default(), WorkStatus::Pending);

    // Test is_terminal for each status
    assert!(!WorkStatus::Pending.is_terminal());
    assert!(!WorkStatus::Ready.is_terminal());
    assert!(!WorkStatus::Assigned.is_terminal());
    assert!(!WorkStatus::Running.is_terminal());
    assert!(!WorkStatus::Blocked.is_terminal());
    assert!(WorkStatus::Done.is_terminal());

    // Test is_ready for each status
    assert!(!WorkStatus::Pending.is_ready());
    assert!(WorkStatus::Ready.is_ready());
    assert!(!WorkStatus::Assigned.is_ready());
}

/// Test MissionState, WatchKind, WatchStatus, and TriggerAction defaults.
#[test]
fn test_enum_defaults() {
    use tinytown::mission::{MissionState, TriggerAction, WatchKind, WatchStatus};

    assert_eq!(MissionState::default(), MissionState::Planning);
    assert_eq!(WatchKind::default(), WatchKind::PrChecks);
    assert_eq!(WatchStatus::default(), WatchStatus::Active);
    assert_eq!(TriggerAction::default(), TriggerAction::CreateFixTask);
}

/// Test MissionPolicy default values.
#[test]
fn test_mission_policy_defaults() {
    use tinytown::mission::MissionPolicy;

    let policy = MissionPolicy::default();
    assert_eq!(policy.max_parallel_items, 2);
    assert!(policy.reviewer_required);
    assert!(!policy.auto_merge);
    assert_eq!(policy.watch_interval_secs, 180);
}

// ============================================================================
// MISSION STORAGE TESTS
// ============================================================================

/// Test MissionStorage save and get operations for MissionRun.
#[tokio::test]
async fn test_mission_storage_save_and_get_mission() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionState, MissionStorage, ObjectiveRef};

    let town = create_test_town("mission-storage-basic").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-basic");

    let objectives = vec![ObjectiveRef::Issue {
        owner: "owner".into(),
        repo: "repo".into(),
        number: 42,
    }];

    let mission = MissionRun::new(objectives);
    let mission_id = mission.id;

    // Save mission
    storage.save_mission(&mission).await?;

    // Get mission
    let retrieved = storage.get_mission(mission_id).await?;
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, mission_id);
    assert_eq!(retrieved.state, MissionState::Planning);
    assert_eq!(retrieved.objective_refs.len(), 1);

    Ok(())
}

/// Test MissionStorage delete operation.
#[tokio::test]
async fn test_mission_storage_delete_mission() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionStorage, ObjectiveRef};

    let town = create_test_town("mission-storage-delete").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-delete");

    let mission = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "test.md".into(),
    }]);
    let mission_id = mission.id;

    storage.save_mission(&mission).await?;

    // Verify it exists
    assert!(storage.get_mission(mission_id).await?.is_some());

    // Delete
    let deleted = storage.delete_mission(mission_id).await?;
    assert!(deleted);

    // Verify it's gone
    assert!(storage.get_mission(mission_id).await?.is_none());

    // Delete non-existent should return false
    let deleted_again = storage.delete_mission(mission_id).await?;
    assert!(!deleted_again);

    Ok(())
}

/// Test MissionStorage active set operations.
#[tokio::test]
async fn test_mission_storage_active_set() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionStorage, ObjectiveRef};

    let town = create_test_town("mission-storage-active").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-active");

    // Initially empty
    let active = storage.list_active().await?;
    assert!(active.is_empty());

    // Create and add two missions to active set
    let mission1 = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "doc1.md".into(),
    }]);
    let mission2 = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "doc2.md".into(),
    }]);
    let id1 = mission1.id;
    let id2 = mission2.id;

    storage.save_mission(&mission1).await?;
    storage.save_mission(&mission2).await?;
    storage.add_active(id1).await?;
    storage.add_active(id2).await?;

    // List active
    let active = storage.list_active().await?;
    assert_eq!(active.len(), 2);
    assert!(active.contains(&id1));
    assert!(active.contains(&id2));

    // Remove one
    storage.remove_active(id1).await?;
    let active = storage.list_active().await?;
    assert_eq!(active.len(), 1);
    assert!(!active.contains(&id1));
    assert!(active.contains(&id2));

    Ok(())
}

/// Test MissionStorage WorkItem operations.
#[tokio::test]
async fn test_mission_storage_work_items() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionStorage, ObjectiveRef, WorkItem, WorkKind};

    let town = create_test_town("mission-storage-work").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-work");

    let mission = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "test.md".into(),
    }]);
    let mission_id = mission.id;
    storage.save_mission(&mission).await?;

    // Create and save work items
    let work1 = WorkItem::new(mission_id, "Design feature", WorkKind::Design);
    let work2 = WorkItem::new(mission_id, "Implement feature", WorkKind::Implement);
    let work1_id = work1.id;
    let work2_id = work2.id;

    storage.save_work_item(&work1).await?;
    storage.save_work_item(&work2).await?;

    // Get individual work item
    let retrieved = storage.get_work_item(mission_id, work1_id).await?;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().title, "Design feature");

    // List all work items
    let items = storage.list_work_items(mission_id).await?;
    assert_eq!(items.len(), 2);

    // Delete one
    let deleted = storage.delete_work_item(mission_id, work1_id).await?;
    assert!(deleted);

    let items = storage.list_work_items(mission_id).await?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, work2_id);

    Ok(())
}

/// Test that mission scheduler assignments create persisted TaskAssign messages.
#[tokio::test]
async fn test_mission_scheduler_assigns_persisted_tasks() -> Result<(), Box<dyn std::error::Error>>
{
    use tinytown::mission::{
        MissionRun, MissionScheduler, MissionStorage, ObjectiveRef, WorkItem, WorkKind,
    };

    let town = create_test_town("mission-scheduler-task-assign").await?;
    let agent_handle = town.spawn_agent("backend-worker", "claude").await?;

    let mut agent = Agent::new("backend-worker", "claude", AgentType::Worker);
    agent.id = agent_handle.id();
    agent.state = AgentState::Idle;
    town.channel().set_agent_state(&agent).await?;

    let storage = MissionStorage::new(
        town.channel().conn().clone(),
        "mission-scheduler-task-assign",
    );

    let mut mission = MissionRun::new(vec![ObjectiveRef::Issue {
        owner: "owner".into(),
        repo: "repo".into(),
        number: 42,
    }]);
    mission.policy.reviewer_required = false;
    mission.start();
    storage.save_mission(&mission).await?;
    storage.add_active(mission.id).await?;

    let mut work_item = WorkItem::new(mission.id, "Implement feature", WorkKind::Implement);
    work_item.mark_ready();
    let work_item_id = work_item.id;
    storage.save_work_item(&work_item).await?;

    let scheduler = MissionScheduler::with_defaults(storage.clone(), town.channel().clone());
    let result = scheduler.tick().await?;
    assert_eq!(result.total_assigned, 1);

    let inbox = town.channel().peek_inbox(agent_handle.id(), 10).await?;
    assert_eq!(inbox.len(), 1);

    let task_id = match &inbox[0].msg_type {
        MessageType::TaskAssign { task_id } => task_id.parse::<TaskId>()?,
        other => panic!("expected TaskAssign, got {:?}", other),
    };

    let task = town
        .channel()
        .get_task(task_id)
        .await?
        .expect("stored task");
    assert_eq!(task.assigned_to, Some(agent_handle.id()));
    assert!(
        task.description
            .contains("[Mission Work Item] Implement feature")
    );
    assert!(task.tags.iter().any(|tag| tag == "mission-work-item"));
    assert!(
        task.tags
            .iter()
            .any(|tag| tag == &format!("mission:{}", mission.id))
    );
    assert!(
        task.tags
            .iter()
            .any(|tag| tag == &format!("work-item:{}", work_item_id))
    );

    Ok(())
}

/// Test MissionStorage WatchItem operations.
#[tokio::test]
async fn test_mission_storage_watch_items() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{
        MissionRun, MissionStorage, ObjectiveRef, WatchItem, WatchKind, WorkItem, WorkKind,
    };

    let town = create_test_town("mission-storage-watch").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-watch");

    let mission = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "test.md".into(),
    }]);
    let mission_id = mission.id;
    storage.save_mission(&mission).await?;

    let work = WorkItem::new(mission_id, "Implement", WorkKind::Implement);
    let work_id = work.id;
    storage.save_work_item(&work).await?;

    // Create and save watch item
    let watch = WatchItem::new(mission_id, work_id, WatchKind::PrChecks, "pr/123", 60);
    let watch_id = watch.id;
    storage.save_watch_item(&watch).await?;

    // Get watch item
    let retrieved = storage.get_watch_item(mission_id, watch_id).await?;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().target_ref, "pr/123");

    // List watch items
    let watches = storage.list_watch_items(mission_id).await?;
    assert_eq!(watches.len(), 1);

    // Delete watch
    let deleted = storage.delete_watch_item(mission_id, watch_id).await?;
    assert!(deleted);
    assert!(storage.list_watch_items(mission_id).await?.is_empty());

    Ok(())
}

/// Test MissionStorage event logging.
#[tokio::test]
async fn test_mission_storage_events() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionStorage, ObjectiveRef};

    let town = create_test_town("mission-storage-events").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-events");

    let mission = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "test.md".into(),
    }]);
    let mission_id = mission.id;
    storage.save_mission(&mission).await?;

    // Log some events
    storage.log_event(mission_id, "Mission started").await?;
    storage.log_event(mission_id, "Work item assigned").await?;
    storage.log_event(mission_id, "PR created").await?;

    // Get events (they should be in reverse order - newest first)
    let events = storage.get_events(mission_id, 10).await?;
    assert_eq!(events.len(), 3);

    // Events should contain timestamps and messages
    assert!(events[0].contains("PR created"));
    assert!(events[1].contains("Work item assigned"));
    assert!(events[2].contains("Mission started"));

    Ok(())
}

/// Test MissionStorage list_all_missions operation.
#[tokio::test]
async fn test_mission_storage_list_all() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{MissionRun, MissionStorage, ObjectiveRef};

    let town = create_test_town("mission-storage-list-all").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-list-all");

    // Create multiple missions
    let mission1 = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "doc1.md".into(),
    }]);
    let mission2 = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "doc2.md".into(),
    }]);
    let mission3 = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "doc3.md".into(),
    }]);

    storage.save_mission(&mission1).await?;
    storage.save_mission(&mission2).await?;
    storage.save_mission(&mission3).await?;

    // List all missions
    let all = storage.list_all_missions().await?;
    assert_eq!(all.len(), 3);

    // Verify IDs are present
    let ids: Vec<_> = all.iter().map(|m| m.id).collect();
    assert!(ids.contains(&mission1.id));
    assert!(ids.contains(&mission2.id));
    assert!(ids.contains(&mission3.id));

    Ok(())
}

/// Test MissionStorage list_due_watches across active missions.
#[tokio::test]
async fn test_mission_storage_list_due_watches() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{
        MissionRun, MissionStorage, ObjectiveRef, WatchItem, WatchKind, WorkItem, WorkKind,
    };

    let town = create_test_town("mission-storage-due-watches").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-due-watches");

    // Create mission with watch items
    let mission = MissionRun::new(vec![ObjectiveRef::Doc {
        path: "test.md".into(),
    }]);
    let mission_id = mission.id;
    storage.save_mission(&mission).await?;
    storage.add_active(mission_id).await?;

    let work = WorkItem::new(mission_id, "Implement", WorkKind::Implement);
    let work_id = work.id;
    storage.save_work_item(&work).await?;

    // Create a watch that is due (interval of 0 means immediately due)
    let watch = WatchItem::new(mission_id, work_id, WatchKind::PrChecks, "pr/123", 0);
    storage.save_watch_item(&watch).await?;

    // List due watches
    let due = storage.list_due_watches().await?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].target_ref, "pr/123");

    Ok(())
}

/// Test mission and work item update flow.
#[tokio::test]
async fn test_mission_storage_update_flow() -> Result<(), Box<dyn std::error::Error>> {
    use tinytown::mission::{
        MissionRun, MissionState, MissionStorage, ObjectiveRef, WorkItem, WorkKind, WorkStatus,
    };

    let town = create_test_town("mission-storage-update").await?;
    let storage = MissionStorage::new(town.channel().conn().clone(), "mission-storage-update");

    // Create and save mission
    let mut mission = MissionRun::new(vec![ObjectiveRef::Issue {
        owner: "owner".into(),
        repo: "repo".into(),
        number: 1,
    }]);
    let mission_id = mission.id;
    storage.save_mission(&mission).await?;

    // Update mission state
    mission.start();
    storage.save_mission(&mission).await?;

    let retrieved = storage.get_mission(mission_id).await?.unwrap();
    assert_eq!(retrieved.state, MissionState::Running);

    // Create and update work item
    let mut work = WorkItem::new(mission_id, "Task", WorkKind::Implement);
    let work_id = work.id;
    storage.save_work_item(&work).await?;

    work.mark_ready();
    storage.save_work_item(&work).await?;

    let retrieved_work = storage.get_work_item(mission_id, work_id).await?.unwrap();
    assert_eq!(retrieved_work.status, WorkStatus::Ready);

    Ok(())
}
