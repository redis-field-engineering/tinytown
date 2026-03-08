# Agents

An **Agent** is a worker that executes tasks. Agents can be AI models (Claude, Auggie, Codex) or custom processes.

## Agent Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | UUID | Unique identifier |
| `name` | String | Human-readable name |
| `agent_type` | Enum | `Worker` or `Supervisor` |
| `state` | Enum | Current lifecycle state |
| `cli` | String | CLI being used (claude, auggie, etc.) |
| `current_task` | Option | Task being worked on |
| `created_at` | DateTime | When agent was created |
| `last_heartbeat` | DateTime | Last activity timestamp |
| `tasks_completed` | u64 | Count of completed tasks |

## Agent States

```
┌───────────┐
│  Starting │ ── Agent is initializing
└─────┬─────┘
      │
      ▼
┌───────────┐     ┌───────────┐
│   Idle    │ ◄──►│  Working  │ ── Can accept work / Executing task
└─────┬─────┘     └───────────┘
      │
      ▼
┌───────────┐     ┌───────────┐
│  Paused   │     │   Error   │ ── Temporarily paused / Something went wrong
└─────┬─────┘     └───────────┘
      │
      ▼
┌───────────┐
│  Stopped  │ ── Agent has terminated
└───────────┘
```

## Creating Agents

### CLI

```bash
# With default model
tt spawn worker-1

# With specific model
tt spawn worker-1 --model claude
tt spawn worker-2 --model auggie
tt spawn reviewer --model codex
```

### Rust API

```rust
use tinytown::{Town, Agent, AgentType};

let town = Town::connect(".").await?;

// Spawn returns a handle
let handle = town.spawn_agent("worker-1", "claude").await?;

// Handle provides operations
let id = handle.id();
let state = handle.state().await?;
let inbox_len = handle.inbox_len().await?;
```

## Built-in Models

Tinytown comes with presets for popular AI coding agents:

| Model | Command | Agent |
|-------|---------|-------|
| `claude` | `claude --print` | Anthropic Claude |
| `auggie` | `augment` | Augment Code |
| `codex` | `codex` | OpenAI Codex |
| `gemini` | `gemini` | Google Gemini |
| `copilot` | `gh copilot` | GitHub Copilot |
| `aider` | `aider` | Aider |
| `cursor` | `cursor` | Cursor |

## Agent Types

### Worker (Default)

Workers execute tasks assigned to them:

```rust
let agent = Agent::new("worker-1", "claude", AgentType::Worker);
```

### Supervisor

A special agent that coordinates workers:

```rust
let supervisor = Agent::supervisor("coordinator");
```

The supervisor has a well-known ID and can send messages to all workers.

## Working with Agent Handles

```rust
let handle = town.spawn_agent("worker-1", "claude").await?;

// Assign a task
let task_id = handle.assign(Task::new("Build the API")).await?;

// Send a message
handle.send(MessageType::StatusRequest).await?;

// Check inbox
let pending = handle.inbox_len().await?;

// Get current state
if let Some(agent) = handle.state().await? {
    println!("State: {:?}", agent.state);
    println!("Current task: {:?}", agent.current_task);
}

// Wait for completion
handle.wait().await?;
```

## Agent Storage in Redis

Agents are persisted in Redis:

```
tt:agent:<uuid>  →  JSON serialized Agent struct
tt:inbox:<uuid>  →  List of pending messages
```

This means:
- Agent state survives Redis restarts (with persistence)
- Multiple processes can coordinate via the same town
- You can inspect state with `redis-cli`

