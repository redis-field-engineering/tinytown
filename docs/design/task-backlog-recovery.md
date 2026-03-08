# Task Backlog & Recovery System Design

## Overview

This document describes the design for a task backlog and recovery system that addresses:
1. **Orphaned Tasks Bug** — Tasks stranded in dead agent inboxes
2. **Task Backlog System** — Global queue for unassigned/reclaimable tasks
3. **Agent Restart** — Restart stopped agents with fresh rounds

## Problem Statement

### Current Issues

1. **Lost Work**: When an agent stops/dies, messages in its inbox are orphaned with no way to reclaim them
2. **No Global Queue**: Tasks must be assigned directly to agents; no "pool" for available work
3. **No Restart**: Stopped agents must be pruned and respawned, losing their identity

## Architecture

### Redis Key Structure (New)

```
tt:backlog           # LIST - global task backlog queue
tt:orphaned:<id>     # LIST - tasks orphaned from dead agent <id>
```

### Data Flow

```
                    ┌─────────────────┐
                    │  tasks.toml     │
                    │  (source)       │
                    └────────┬────────┘
                             │ tt sync push
                             ▼
┌─────────────┐      ┌───────────────┐      ┌─────────────┐
│ tt backlog  │◄────►│   BACKLOG     │◄────►│ tt reclaim  │
│ add/list    │      │  (Redis)      │      │             │
└─────────────┘      └───────────────┘      └──────┬──────┘
                             │                     │
                             │ tt backlog claim    │ (dead agent)
                             ▼                     │
                     ┌───────────────┐             │
                     │ Agent Inbox   │◄────────────┘
                     │  (Redis)      │
                     └───────────────┘
```

## New CLI Commands

### 1. `tt backlog` — Manage the Global Task Backlog

```bash
# Add a task to the backlog (not assigned to any agent)
tt backlog add "Implement new feature"
tt backlog add "Fix bug in parser" --tags "bug,P1"

# List all tasks in backlog
tt backlog list

# Claim a task from backlog (assign to agent)
tt backlog claim <task-id> <agent-name>

# Move all backlog tasks to an agent
tt backlog assign-all <agent-name>
```

### 2. `tt reclaim` — Recover Tasks from Dead Agents

```bash
# Show orphaned tasks from stopped/error agents
tt reclaim list

# Move orphaned tasks to backlog
tt reclaim --to-backlog

# Move orphaned tasks to a specific agent
tt reclaim --to <agent-name>

# Reclaim from a specific dead agent
tt reclaim --from <dead-agent> --to <alive-agent>
```

### 3. `tt restart` — Restart a Stopped Agent

```bash
# Restart agent with fresh rounds
tt restart <agent-name>

# Restart with different max rounds
tt restart <agent-name> --rounds 10
```

## Implementation Details

### Backlog Storage

The backlog is a Redis LIST at key `tt:backlog`:

```rust
// In channel.rs
const BACKLOG_KEY: &str = "tt:backlog";

impl Channel {
    /// Add a task to the global backlog.
    pub async fn backlog_push(&self, task_id: TaskId) -> Result<()>;
    
    /// Get all tasks in the backlog.
    pub async fn backlog_list(&self) -> Result<Vec<TaskId>>;
    
    /// Remove and return a task from the backlog.
    pub async fn backlog_pop(&self) -> Result<Option<TaskId>>;
    
    /// Remove a specific task from the backlog.
    pub async fn backlog_remove(&self, task_id: TaskId) -> Result<bool>;
}
```

### Reclaim Logic

When reclaiming orphaned tasks:

```rust
// Pseudo-code for tt reclaim
async fn reclaim_tasks(channel: &Channel, dead_agents: Vec<Agent>) -> Result<Vec<Task>> {
    let mut orphaned = Vec::new();
    
    for agent in dead_agents {
        if !agent.state.is_terminal() { continue; }
        
        // Drain inbox
        while let Some(msg) = channel.receive(agent.id).await? {
            if let MessageType::Task(task_id) = msg.msg_type {
                orphaned.push(task_id);
            }
        }
    }
    
    orphaned
}
```

### Restart Logic

```rust
// Pseudo-code for tt restart
async fn restart_agent(town: &Town, name: &str, rounds: Option<u32>) -> Result<()> {
    let agent = town.channel().get_agent_by_name(name).await?;
    
    if !agent.state.is_terminal() {
        return Err("Agent is still running");
    }
    
    // Reset state but keep inbox
    agent.state = AgentState::Idle;
    agent.rounds_completed = 0;
    agent.last_heartbeat = Utc::now();
    
    town.channel().set_agent_state(&agent).await?;
    
    // Spawn CLI process
    spawn_agent_cli(town, &agent, rounds).await?;
}
```

### Integration with tasks.toml

Add optional `backlog = true` field:

```toml
[[tasks]]
id = "unassigned-task"
description = "Task for whoever is available"
backlog = true   # Goes to backlog, not to specific agent
tags = ["available"]
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Reclaim from working agent | Error: "Agent is still active" |
| Claim from empty backlog | Error: "Backlog is empty" |
| Restart working agent | Error: "Agent is not stopped" |
| Task already assigned | Warning, skip task |

## Testing Strategy

1. **Unit tests** in `tests/integration_tests.rs`:
   - `test_backlog_add_list_claim`
   - `test_reclaim_from_dead_agent`
   - `test_restart_stopped_agent`

2. **Integration tests**:
   - Agent dies → reclaim → new agent gets tasks
   - Multiple agents claim from shared backlog

## Migration

No migration needed — new Redis keys only. Existing towns continue to work.

## Future Enhancements

- Priority-based backlog (use sorted set)
- Task expiry/TTL
- Auto-reclaim on `tt recover`
- Backlog webhooks for external integrations

