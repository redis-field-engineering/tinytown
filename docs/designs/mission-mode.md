# Autonomous Multi-Issue Mission Mode Design

## Overview

This document defines the architecture for Issue #23: Autonomous Multi-Issue Mission Mode. The system enables durable, dependency-aware orchestration of multiple GitHub issues with automatic PR/CI monitoring.

## Goals

1. Accept multiple issue/doc inputs as a single objective set
2. Build and maintain a dependency-aware execution plan (DAG)
3. Delegate tasks to best-fit agents automatically
4. Treat PR/CI/Bugbot monitoring as first-class scheduled work
5. Persist orchestration memory so restart/resume continues cleanly
6. Keep design minimal, inspectable, and deterministic

## Non-Goals

- Full event-sourced distributed scheduler
- Complex multi-tenant planning logic
- Replacing manual conductor workflows

## Data Model

### MissionRun (top-level orchestration record)

```rust
pub struct MissionRun {
    pub run_id: MissionId,           // UUID
    pub objective_refs: Vec<ObjectiveRef>,  // issue URLs, doc paths
    pub state: MissionState,         // planning | running | blocked | completed | failed
    pub policy: MissionPolicy,       // max parallelism, reviewer required, auto-merge
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub next_wake_at: Option<DateTime<Utc>>,  // scheduler wake-up time
    pub blocked_reason: Option<String>,
}

pub enum MissionState {
    Planning,   // Compiling work graph
    Running,    // Active execution
    Blocked,    // Waiting on external event
    Completed,  // All objectives done
    Failed,     // Unrecoverable error
}

pub struct MissionPolicy {
    pub max_parallel_items: u32,     // Default: 2
    pub reviewer_required: bool,     // Default: true
    pub auto_merge: bool,            // Default: false
    pub watch_interval_secs: u64,    // Default: 180
}
```

### WorkItem (individual work unit)

```rust
pub struct WorkItem {
    pub work_id: WorkItemId,
    pub mission_id: MissionId,
    pub title: String,
    pub kind: WorkKind,              // design | implement | test | review | merge_gate | followup
    pub depends_on: Vec<WorkItemId>,
    pub owner_role: Option<String>,  // "backend", "tester", "reviewer"
    pub status: WorkStatus,          // pending | ready | assigned | running | blocked | done
    pub assigned_to: Option<AgentId>,
    pub artifact_refs: Vec<String>,  // PR url, commit sha, doc path
    pub source_ref: Option<String>,  // Original issue/doc this came from
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum WorkKind {
    Design,
    Implement,
    Test,
    Review,
    MergeGate,
    Followup,
}

pub enum WorkStatus {
    Pending,    // Dependencies not satisfied
    Ready,      // Can be assigned
    Assigned,   // Agent selected, not yet started
    Running,    // In progress
    Blocked,    // Waiting on fix/external
    Done,       // Completed successfully
}
```

### WatchItem (PR/CI monitoring)

```rust
pub struct WatchItem {
    pub watch_id: WatchId,
    pub mission_id: MissionId,
    pub work_item_id: WorkItemId,    // Parent work item
    pub kind: WatchKind,             // pr_checks | bugbot | review_comments | mergeability
    pub target_ref: String,          // PR number/url
    pub interval_secs: u64,
    pub next_due_at: DateTime<Utc>,
    pub status: WatchStatus,         // active | snoozed | done
    pub on_trigger: TriggerAction,   // create_fix_task | notify_reviewer | advance_pipeline
    pub last_check_at: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
}

pub enum WatchKind {
    PrChecks,
    BugbotComments,
    ReviewComments,
    Mergeability,
}

pub enum WatchStatus {
    Active,
    Snoozed,
    Done,
}

pub enum TriggerAction {
    CreateFixTask,
    NotifyReviewer,
    AdvancePipeline,
}
```

## Redis Key Schema

All keys namespaced under `tt:{town}:mission:*`:

```
tt:{town}:mission:{run_id}              # Hash: MissionRun metadata
tt:{town}:mission:{run_id}:work         # Hash: WorkItemId -> WorkItem JSON
tt:{town}:mission:{run_id}:watch        # Hash: WatchId -> WatchItem JSON  
tt:{town}:mission:{run_id}:events       # List: bounded activity log (100 entries)
tt:{town}:mission:active                # Set: active MissionIds for scheduler
```

## Scheduler Loop

The scheduler runs every 30s (configurable):

```
1. Load active mission IDs from tt:{town}:mission:active
2. For each active mission:
   a. Refresh mission state
   b. Check due watch items, execute triggers
   c. Update work item statuses from observations
   d. Promote pending -> ready when dependencies satisfied
   e. Match ready items to idle agents by role fit
   f. Enforce reviewer gate before advancing
   g. Log activity events
3. If no ready items and no active watches: mark completed
4. If blocked: set next_wake_at based on watch intervals
```

## Agent Routing

Simple role-fit scoring:
1. Exact role/tag match (e.g., `owner_role: "backend"` -> agent with `backend` role)
2. Generic worker fallback
3. Avoid assigning same agent too many concurrent items
4. Keep reviewer reserved for review gates unless idle

## CLI Commands

```bash
tt mission start --issue <n|url>... --doc <path>...   # Start new mission
tt mission status [--run <id>]                         # Show mission status
tt mission resume <run_id>                             # Resume stopped mission
tt mission stop <run_id>                               # Stop mission gracefully
```

## MCP Tools

```
mission.start     # Start mission with objectives
mission.status    # Get mission state and work graph
mission.resume    # Resume stopped mission
mission.stop      # Stop mission
mission.list      # List active missions
watch.list        # List active watches for a mission
watch.snooze      # Temporarily pause a watch
```

## Module Structure

```
src/
├── mission/
│   ├── mod.rs           # Public API exports
│   ├── types.rs         # MissionRun, WorkItem, WatchItem, IDs
│   ├── storage.rs       # Redis persistence layer
│   ├── compiler.rs      # Work graph compiler (issues/docs -> DAG)
│   ├── scheduler.rs     # Scheduler loop and ready-queue logic
│   ├── router.rs        # Agent routing/matching
│   └── watch.rs         # Watch engine for PR/CI monitoring
├── app/
│   ├── mcp/
│   │   └── mission_tools.rs  # MCP tool definitions
│   └── services/
│       └── mission.rs   # MissionService (business logic)
```

## Implementation Phases

### Phase 1: Core Types & Storage (src/mission/types.rs, storage.rs)
- Define `MissionId`, `WorkItemId`, `WatchId` ID types
- Define `MissionRun`, `WorkItem`, `WatchItem` structs
- Implement `MissionStorage` trait for Redis persistence
- Add key generation and CRUD operations

### Phase 2: Work Graph Compiler (src/mission/compiler.rs)
- Parse GitHub issue bodies for dependency markers (`depends on #X`, `after #X`)
- Build WorkItem DAG from issues/docs
- Support manual manifest override file
- Calculate topological order

### Phase 3: Scheduler Loop (src/mission/scheduler.rs)
- Implement periodic scheduler tick (30s default)
- Ready-queue management (pending -> ready promotion)
- Agent assignment with role matching
- Reviewer gate enforcement
- State machine transitions

### Phase 4: Watch Engine (src/mission/watch.rs)
- PR check polling via GitHub API
- Bugbot/review comment detection
- Trigger action execution (create fix task, notify)
- Backoff and retry logic

### Phase 5: CLI & MCP Surface (main.rs, mcp/mission_tools.rs)
- `tt mission start/status/resume/stop` commands
- MCP tools for mission lifecycle
- Status output with work graph visualization

### Phase 6: Integration & Testing
- End-to-end test: plan -> delegate -> PR watch -> fix -> advance
- Resume test: restart mid-run
- Unit tests: dependency resolution, ready-queue, watch scheduling

## Handoff Protocol

When a work item reaches done:
1. Mark item done with evidence (PR url, commit sha)
2. Recompute DAG readiness
3. Automatically activate next ready items
4. If no ready items and watches active: stay `running` with next wake-up
5. If no ready items and no active watches: set run `completed`

## Execution Policy Defaults

- Reviewer gate required for all implement/test items
- Max active implementation items = 2 (configurable)
- Watch interval default = 180s for PR/CI/Bugbot
- Retry failed watch checks with backoff (1m, 2m, 5m), then mark blocked

## Error Handling

- Transient GitHub API errors: retry with backoff
- Persistent failures: mark watch as blocked, log reason
- Agent crashes: reclaim tasks via existing recovery service
- Mission failure: preserve state for debugging, allow manual resume

