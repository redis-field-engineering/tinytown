# Agents

An **Agent** is a worker that executes tasks. Agents can use AI coding CLIs (Claude, Auggie, Codex) or custom processes.

## Agent Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | UUID | Unique identifier |
| `name` | String | Human-readable canonical name |
| `nickname` | String | Human-facing display name — auto-assigned from 1920s-era names if not provided |
| `role_id` | Option\<String\> | Explicit role for routing (e.g., `"worker"`, `"reviewer"`, `"researcher"`) |
| `agent_type` | Enum | `Worker` or `Supervisor` |
| `state` | Enum | Current lifecycle state |
| `cli` | String | CLI being used (claude, auggie, etc.) |
| `current_task` | Option | Task being worked on |
| `parent_agent_id` | Option\<AgentId\> | Parent agent for delegated subtasks |
| `spawn_mode` | SpawnMode | How the session was created: `Fresh`, `ForkedContext`, or `Resumed` |
| `current_scope` | Option\<String\> | Free-text description of current assigned scope |
| `created_at` | DateTime | When agent was created |
| `last_heartbeat` | DateTime | Last activity timestamp |
| `tasks_completed` | u64 | Count of completed tasks |

### Roles

Roles are **explicit metadata** rather than inferred from agent names. The mission scheduler prefers `role_id` for routing work to agents, falling back to name-based matching only when no role is set.

Built-in roles: `worker`, `reviewer`, `researcher`, `architect`, `tester`, `devops`.

```bash
# Spawn with an explicit role
tt spawn backend --role worker
tt spawn qa --role reviewer --nickname "Quality Gate"
tt spawn alice --role researcher --parent backend
```

### Nicknames

Every agent automatically gets a nickname from the 1920s — the decade Tiny Town, Colorado was founded. Names like **Robert**, **Dorothy**, **Helen**, **James**, and **Margaret** are deterministically assigned based on the agent's UUID.

You can override the auto-nickname with `--nickname`:

```bash
tt spawn backend                           # Gets a 1920s name like "Dorothy"
tt spawn backend --nickname "The Builder"  # Overrides to "The Builder"
```

### Spawn Modes

| Mode | Description |
|------|-------------|
| `Fresh` | Default. New agent with no inherited context. |
| `ForkedContext` | Agent forked from a parent, inheriting its context with boundary markers. |
| `Resumed` | Agent resumed from a previous session. |

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
      ├─────────────────────┐
      ▼                     ▼
┌───────────┐     ┌───────────┐     ┌───────────┐
│  Paused   │     │ Draining  │     │   Error   │
└─────┬─────┘     └─────┬─────┘     └───────────┘
      │                 │
      ▼                 ▼
┌───────────┐     ┌───────────┐
│  Stopped  │     │   Cold    │
└───────────┘     └───────────┘
```

| State | Description |
|-------|-------------|
| `Starting` | Agent is initializing |
| `Idle` | Ready to accept work |
| `Working` | Executing a task |
| `Paused` | Temporarily paused via `tt interrupt` — resumes with `tt resume` |
| `Draining` | Finishing current work before stopping (via `tt close`) |
| `Cold` | Gracefully shut down after draining |
| `Error` | Something went wrong |
| `Stopped` | Agent has terminated |

### Control-Plane Operations

| Command | Effect |
|---------|--------|
| `tt interrupt <agent>` | Pauses the agent — it stops processing messages until resumed |
| `tt resume <agent>` | Resumes a paused agent |
| `tt close <agent>` | Gracefully drains current work, then transitions to `Cold` |
| `tt wait <agent> [--timeout N]` | Blocks until the agent reaches a terminal state |
| `tt kill <agent>` | Requests the agent to stop at the start of its next round |

## Creating Agents

### CLI

```bash
# Basic spawn
tt spawn worker-1
tt spawn worker-1 --cli claude

# With role and nickname
tt spawn backend --role worker --nickname "API Developer"

# With parent (for delegated subtasks)
tt spawn subtask-1 --role worker --parent backend

# With all metadata
tt spawn qa --role reviewer --nickname "Quality Gate" --cli auggie
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

## Built-in CLIs

Tinytown comes with presets for popular AI coding agents:

| CLI | Command | Agent |
|-------|---------|-------|
| `claude` | `claude --print --dangerously-skip-permissions` | Anthropic Claude |
| `auggie` | `auggie --print` | Augment Code |
| `codex` | `codex exec --dangerously-bypass-approvals-and-sandbox` | OpenAI Codex |
| `codex-mini` | `codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort="medium"` | OpenAI Codex |
| `gemini` | `gemini` | Google Gemini |
| `copilot` | `gh copilot` | GitHub Copilot |
| `aider` | `aider --yes --no-auto-commits --message` | Aider |
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

Agents are persisted in Redis using town-isolated keys:

```
tt:<town_name>:agent:<uuid>  →  JSON serialized Agent struct
tt:<town_name>:inbox:<uuid>  →  List of pending messages
```

This town-isolated format allows multiple Tinytown projects to share the same Redis instance without key conflicts. See [tt migrate](../cli/migrate.md) for upgrading from older key formats.

This means:
- Agent state survives Redis restarts (with persistence)
- Multiple processes can coordinate via the same town
- Multiple towns can share the same Redis instance
- You can inspect state with `redis-cli`
