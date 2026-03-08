# API Reference

The Tinytown Rust API for programmatic control.

## Quick Links

- [Full rustdoc](https://jeremyplichta.github.io/tinytown/tinytown/) — Complete API documentation

## Core Types

### Town

```rust
use tinytown::Town;

// Initialize new town
let town = Town::init("./path", "name").await?;

// Connect to existing
let town = Town::connect("./path").await?;

// Operations
let agent = town.spawn_agent("name", "cli").await?;
let agent = town.agent("name").await?;
let agents = town.list_agents().await;
let channel = town.channel();
let config = town.config();
let root = town.root();
```

### Agent

```rust
use tinytown::{Agent, AgentId, AgentType, AgentState};

// Create agent
let agent = Agent::new("name", "cli", AgentType::Worker);

// Supervisor (well-known ID)
let supervisor = Agent::supervisor("coordinator");

// Check state
if agent.state.is_terminal() { /* stopped or error */ }
if agent.state.can_accept_work() { /* idle */ }
```

### AgentHandle

```rust
// Get handle from town
let handle = town.spawn_agent("worker", "claude").await?;

// Operations
let id = handle.id();
let task_id = handle.assign(task).await?;
handle.send(MessageType::StatusRequest).await?;
let len = handle.inbox_len().await?;
let state = handle.state().await?;
handle.wait().await?;
```

### Task

```rust
use tinytown::{Task, TaskId, TaskState};

// Create
let task = Task::new("description");
let task = Task::new("desc").with_tags(["tag1", "tag2"]);
let task = Task::new("desc").with_parent(parent_id);

// Lifecycle
task.assign(agent_id);
task.start();
task.complete("result");
task.fail("error");

// Check state
if task.state.is_terminal() { /* completed, failed, or cancelled */ }
```

### Message

```rust
use tinytown::{Message, MessageId, MessageType, Priority};

// Create
let msg = Message::new(from, to, MessageType::TaskAssign { 
    task_id: "abc".into() 
});

// With options
let msg = msg.with_priority(Priority::Urgent);
let msg = msg.with_correlation(other_msg.id);
```

### MessageType

```rust
pub enum MessageType {
    TaskAssign { task_id: String },
    TaskDone { task_id: String, result: String },
    TaskFailed { task_id: String, error: String },
    StatusRequest,
    StatusResponse { state: String, current_task: Option<String> },
    Ping,
    Pong,
    Shutdown,
    Custom { kind: String, payload: String },
}
```

### Channel

```rust
use tinytown::Channel;
use std::time::Duration;

let channel = town.channel();

// Messages
channel.send(&msg).await?;
let msg = channel.receive(agent_id, Duration::from_secs(30)).await?;
let msg = channel.try_receive(agent_id).await?;
let len = channel.inbox_len(agent_id).await?;
channel.broadcast(&msg).await?;

// State
channel.set_agent_state(&agent).await?;
let agent = channel.get_agent_state(agent_id).await?;
channel.set_task(&task).await?;
let task = channel.get_task(task_id).await?;
```

## Error Handling

```rust
use tinytown::{Error, Result};

match result {
    Ok(value) => { /* success */ }
    Err(Error::Redis(e)) => { /* redis error */ }
    Err(Error::AgentNotFound(name)) => { /* agent missing */ }
    Err(Error::TaskNotFound(id)) => { /* task missing */ }
    Err(Error::NotInitialized(path)) => { /* town not init */ }
    Err(Error::RedisNotInstalled) => { /* redis missing */ }
    Err(Error::RedisVersionTooOld(ver)) => { /* upgrade redis */ }
    Err(Error::Timeout(msg)) => { /* operation timed out */ }
    Err(e) => { /* other error */ }
}
```

## Example: Complete Workflow

```rust
use tinytown::{Town, Task, AgentState, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect
    let town = Town::connect(".").await?;
    
    // Spawn agents
    let dev = town.spawn_agent("dev", "claude").await?;
    let reviewer = town.spawn_agent("reviewer", "codex").await?;
    
    // Assign work
    dev.assign(Task::new("Build the feature")).await?;
    
    // Wait for completion
    loop {
        if let Some(agent) = dev.state().await? {
            if matches!(agent.state, AgentState::Idle) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    
    // Send for review
    reviewer.assign(Task::new("Review the feature")).await?;
    
    Ok(())
}
```

