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
async fn create_test_town(name: &str) -> Result<TownGuard, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let town = Town::init(temp_dir.path(), name).await?;
    Ok(TownGuard { town, temp_dir })
}

/// Helper to kill Redis when test ends
fn cleanup_redis(temp_dir: &TempDir) {
    let pid_file = temp_dir.path().join("redis.pid");
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

    assert!(town_path.join("agents").exists());
    assert!(town_path.join("logs").exists());
    assert!(town_path.join("tasks").exists());
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
    let town = create_test_town("default-cli-test").await?;

    // Config should have a default_cli
    let config = town.config();
    assert!(!config.default_cli.is_empty());
    assert_eq!(config.default_cli, "claude"); // Default is claude

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

    // Verify expected directories exist
    assert!(town_path.join("agents").exists());
    assert!(town_path.join("logs").exists());
    assert!(town_path.join("tasks").exists());

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

    let temp_dir = TempDir::new()?;
    let config = Config::new("test-town", temp_dir.path());

    // Default config should use Unix socket
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

    // Unix socket is default
    assert!(config.use_socket, "Default should use Unix socket");
    assert_eq!(config.socket_path, "redis.sock");

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
