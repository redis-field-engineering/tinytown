# Messages

**Messages** are how agents communicate. They're the envelopes that carry task assignments, status updates, and coordination signals.

## Message Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | UUID | Unique message identifier |
| `from` | AgentId | Sender |
| `to` | AgentId | Recipient |
| `msg_type` | Enum | Type and payload |
| `priority` | Enum | Processing order |
| `created_at` | DateTime | When sent |
| `correlation_id` | Option | For request/response |

## Message Types

```rust
pub enum MessageType {
    // Semantic message types (for inter-agent communication)
    Task { description: String },           // Actionable work request
    Query { question: String },             // Question expecting response
    Informational { summary: String },      // FYI/context update
    Confirmation { ack_type: ConfirmationType }, // Receipt/acknowledgment

    // Task management
    TaskAssign { task_id: String },
    TaskDone { task_id: String, result: String },
    TaskFailed { task_id: String, error: String },

    // Status
    StatusRequest,
    StatusResponse { state: String, current_task: Option<String> },

    // Lifecycle
    Ping,
    Pong,
    Shutdown,

    // Extensibility
    Custom { kind: String, payload: String },
}

pub enum ConfirmationType {
    Received,              // Message was received
    Acknowledged,          // Message was acknowledged
    Thanks,                // Message expressing thanks
    Approved,              // Approval confirmation
    Rejected { reason: String }, // Rejection with reason
}
```

### Semantic Message Classification

Messages are classified as either **actionable** or **informational**:

| Actionable (require work) | Informational (context only) |
|---------------------------|------------------------------|
| `Task`, `Query`, `TaskAssign` | `Informational`, `Confirmation` |
| `StatusRequest`, `Ping` | `TaskDone`, `TaskFailed` |
| `Shutdown`, `Custom` | `StatusResponse`, `Pong` |

Use `msg.is_actionable()` and `msg.is_informational_or_confirmation()` helpers to classify.
```

## Priority Levels

Messages are processed by priority:

| Priority | Behavior |
|----------|----------|
| `Urgent` | Goes to front of queue, interrupt current work |
| `High` | Goes to front of queue |
| `Normal` | Goes to back of queue (default) |
| `Low` | Goes to back, processed when idle |

```rust
let msg = Message::new(from, to, MessageType::Shutdown)
    .with_priority(Priority::Urgent);
```

## Creating Messages

```rust
use tinytown::{Message, MessageType, AgentId, Priority};

// Basic message
let msg = Message::new(
    AgentId::supervisor(),  // from
    worker_id,              // to
    MessageType::TaskAssign { task_id: "abc123".into() }
);

// With priority
let urgent = Message::new(from, to, MessageType::Shutdown)
    .with_priority(Priority::Urgent);

// With correlation (for request/response)
let request = Message::new(from, to, MessageType::StatusRequest);
let response = Message::new(to, from, MessageType::StatusResponse { 
    state: "working".into(),
    current_task: Some("task-123".into())
}).with_correlation(request.id);
```

## Sending Messages

Via Channel:
```rust
let channel = town.channel();
channel.send(&message).await?;
```

Via AgentHandle:
```rust
let handle = town.agent("worker-1").await?;
handle.send(MessageType::StatusRequest).await?;
```

## Receiving Messages

```rust
use std::time::Duration;

// Blocking receive (waits up to timeout)
if let Some(msg) = channel.receive(agent_id, Duration::from_secs(30)).await? {
    match msg.msg_type {
        MessageType::TaskAssign { task_id } => {
            println!("Got task: {}", task_id);
        }
        MessageType::Shutdown => {
            println!("Shutting down");
            break;
        }
        _ => {}
    }
}

// Non-blocking receive
if let Some(msg) = channel.try_receive(agent_id).await? {
    // Process message
}
```

## Broadcasting

Send to all agents:

```rust
let broadcast = Message::new(
    AgentId::supervisor(),
    AgentId::supervisor(),  // Placeholder, broadcast ignores this
    MessageType::Shutdown
);
channel.broadcast(&broadcast).await?;
```

## Custom Messages

For application-specific communication:

```rust
// Define your payload as JSON
let payload = serde_json::json!({
    "pr_url": "https://github.com/...",
    "files_changed": ["src/auth.rs", "tests/auth_test.rs"]
});

let msg = Message::new(from, to, MessageType::Custom {
    kind: "pr_ready".into(),
    payload: payload.to_string()
});
```

## Message Flow Example

```
Supervisor                    Worker
    │                           │
    │  TaskAssign{task-123}     │
    │ ─────────────────────────►│
    │                           │
    │                           │ (working...)
    │                           │
    │    TaskDone{task-123}     │
    │◄───────────────────────── │
    │                           │
```

## Comparison with Gastown Mail

| Feature | Tinytown Messages | Gastown Mail |
|---------|-------------------|--------------|
| Transport | Redis lists | Beads (git-backed) |
| Persistence | Redis persistence | Git commits |
| Priority | 4 levels | Yes |
| Routing | Direct to inbox | Complex routing |
| Recovery | Redis replays | Event replay |

Tinytown messages live in Redis, and Tinytown-managed local Redis uses the default RDB snapshot flow so message state is typically restored after a restart. If you want stronger write-by-write durability, enable AOF as well. With Redis Cloud, persistence and backup policy are handled by the managed service.
