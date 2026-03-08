# Concept Mapping: Gastown → Tinytown

A detailed translation guide for Gastown users.

## Agent Taxonomy

### Gastown's Agent Zoo

Gastown has **8 agent types** across two levels:

**Town-Level:**
| Agent | Role | Tinytown Equivalent |
|-------|------|---------------------|
| Mayor | Global coordinator | Your orchestration code |
| Deacon | Daemon, health monitoring | Your process + monitoring |
| Boot | Deacon watchdog | External health check |
| Dogs | Infrastructure helpers | Background tasks |

**Rig-Level:**
| Agent | Role | Tinytown Equivalent |
|-------|------|---------------------|
| Witness | Monitors polecats | Status polling loop |
| Refinery | Merge queue processor | CI/CD integration |
| Polecats | Workers | Agents |
| Crew | Human workspaces | N/A (you're the human) |

### Tinytown's Simplicity

Tinytown has **2 agent types**:

| Agent | Role |
|-------|------|
| Supervisor | Well-known ID for coordination |
| Worker | Does the actual work |

Everything else? You write it explicitly.

## Work Tracking

### Gastown Beads

Beads are git-backed structured records:

```
ID: gt-abc12
Type: task
Title: Implement login API
Status: in_progress
Priority: P1
Created: 2024-03-01
Assigned: gastown/polecats/Toast
Parent: gt-xyz99 (epic)
Dependencies: [gt-def34, gt-ghi56]
```

Features:
- Stored in Dolt SQL
- Version controlled
- Two-level (Town + Rig)
- Rich schema with dependencies
- Prefix-based namespacing

### Tinytown Tasks

Tasks are defined in `tasks.toml`:

```toml
[[tasks]]
id = "login-api"
description = "Implement login API"
agent = "backend"
status = "pending"
tags = ["auth", "api"]
```

Or assigned via CLI:

```bash
tt assign backend "Implement login API"
```

Features:
- Stored in Redis
- Defined in TOML (version-controlled)
- Single level
- Minimal schema
- Tags for organization

### Translation

| Beads Feature | Tinytown Approach |
|---------------|-------------------|
| Priority (P0-P4) | Use tags: `["P1"]` |
| Type (task/bug/feature) | Use tags: `["bug"]` |
| Dependencies | Manual coordination |
| Parent/child | `parent_id` field |
| Status history | Not built-in (log it yourself) |

## Coordination Mechanisms

### Gastown: Convoys

Convoys track batches of related work:

```bash
gt convoy create "User Auth Feature" gt-abc12 gt-def34 gt-ghi56
gt convoy status hq-cv-xyz
```

Features:
- Auto-created by Mayor
- Tracks multiple beads
- Lifecycle: OPEN → LANDED → CLOSED
- Event-driven completion detection

### Tinytown: Manual Grouping

Use parent tasks or tags in `tasks.toml`:

```toml
# Option 1: Parent tasks
[[tasks]]
id = "auth-feature"
description = "User Auth Feature"
status = "pending"

[[tasks]]
id = "login"
description = "Login flow"
parent = "auth-feature"
agent = "backend"
status = "pending"

[[tasks]]
id = "signup"
description = "Signup flow"
parent = "auth-feature"
agent = "backend"
status = "pending"

# Option 2: Tags for grouping
[[tasks]]
id = "login-tagged"
description = "Login flow"
tags = ["auth-feature"]
agent = "backend"
status = "pending"
```

### Gastown: Hooks

Hooks are the assignment mechanism:

```
Polecat has hook → Hook has pinned bead → Polecat MUST work on it
```

The "GUPP Principle": If work is on your hook, you run it immediately.

### Tinytown: Inboxes

Messages go to agent inboxes (Redis lists):

```
Agent has inbox → Messages queued → Agent polls/blocks for messages
```

You control when and how agents process work.

## Communication

### Gastown: Mail Protocol

Messages are beads of type `message`:

```bash
# Check mail
gt mail check

# Types: POLECAT_DONE, MERGE_READY, REWORK_REQUEST, etc.
```

Complex routing through beads system.

### Tinytown: Direct Messages

Messages are transient, stored in Redis. Send via CLI:

```bash
# Send a message to another agent
tt send reviewer "Task complete. Ready for review."

# Send an urgent message
tt send reviewer --urgent "Critical issue found!"
```

Direct, simple, explicit.

## State Persistence

### Gastown: Multi-Layer

1. **Git worktrees** - Sandbox persistence
2. **Beads ledger** - Work state (Dolt SQL)
3. **Hooks** - Work assignment
4. **State files** - Runtime state (JSON)

### Tinytown: Redis

Everything in Redis:
- `tt:agent:<id>` - Agent state
- `tt:task:<id>` - Task state
- `tt:inbox:<id>` - Message queues

Enable Redis persistence (RDB/AOF) for durability.

## Recovery

### Gastown: Automatic

- Witness patrols detect stalled polecats
- Deacon monitors system health
- Boot watches Deacon
- Hooks ensure work resumes on restart

### Tinytown: Manual

You implement recovery via CLI:

```bash
# Check agent health
tt status
tt list

# If an agent is in error state, respawn it
tt prune
tt spawn worker-1 --model claude

# Reassign failed tasks
tt assign worker-1 "Retry the failed operation"
```

## When Tinytown Falls Short

Gastown features you might miss:

| Feature | Why It's Useful | Tinytown Workaround |
|---------|-----------------|---------------------|
| Automatic recovery | Hands-off operation | Write recovery loops |
| Git-backed history | Audit trail | Log to files |
| Dependency graphs | Complex workflows | Manual ordering |
| Cross-rig work | Multi-repo coordination | Run multiple towns |
| Dashboard | Visual monitoring | CLI + custom tooling |

If you find yourself building these features, consider whether Gastown's complexity is justified for your use case.

