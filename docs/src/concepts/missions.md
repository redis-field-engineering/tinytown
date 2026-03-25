# Mission Mode

**Mission Mode** enables autonomous, dependency-aware orchestration of multiple GitHub issues with automatic PR/CI monitoring.

## Overview

While regular Tinytown tasks are great for individual work items, mission mode is designed for larger objectives that span multiple issues, require dependency tracking, and need ongoing monitoring of external events like CI status and code reviews.

Think of a mission as a "project manager" that:
- Accepts multiple GitHub issues as objectives
- Builds a dependency-aware execution plan (DAG)
- Delegates work to best-fit agents automatically
- Monitors PRs, CI, and review status
- Persists state for restart/resume capability

In practice, missions are progressed by the `tt mission dispatch` loop. That dispatcher is the autonomous runtime: it processes due watches, assigns ready work, and advances missions without requiring the conductor to keep re-prompting the system.

## Core Concepts

### MissionRun

The top-level orchestration record. A MissionRun owns:
- **Objectives**: GitHub issues or documents to complete
- **Work Items**: Individual tasks extracted from objectives
- **Watch Items**: Monitoring tasks for PRs/CI
- **Policy**: Execution rules (parallelism, review gates, etc.)

### Mission States

```
┌──────────┐
│ Planning │ ── Compiling work graph from objectives
└────┬─────┘
     │
     ▼
┌──────────┐
│ Running  │ ── Active execution
└────┬─────┘
     │
     ├──► ┌──────────┐
     │    │ Blocked  │ ── Waiting on external event
     │    └──────────┘
     │
     ├──► ┌───────────┐
     │    │ Completed │ ✓ All objectives done
     │    └───────────┘
     │
     └──► ┌────────┐
          │ Failed │ ✗ Unrecoverable error
          └────────┘
```

### Work Items

Individual units of work in the mission DAG. Each work item:
- Has a status: `pending` → `ready` → `assigned` → `running` → `done`
- May depend on other work items
- Gets assigned to an agent based on role fit

### Watch Items

Scheduled monitoring tasks that poll for external events:
- **PR Checks**: CI pass/fail status
- **Reviews**: Human review comments
- **Bugbot**: Automated security reports
- **Mergeability**: Conflict and merge status

## Dependency Detection

The mission compiler parses issue bodies for dependency markers:

```markdown
<!-- In your GitHub issue body -->
This feature depends on #42.
After #41 is complete, we can start this.
Blocked by #40.
```

Supported patterns:
- `depends on #N`
- `after #N`
- `blocked by #N`
- `requires #N`

## Mission Policy

Control execution behavior with policy settings:

| Setting | Default | Description |
|---------|---------|-------------|
| `max_parallel_items` | 2 | Max concurrent work items |
| `reviewer_required` | true | Require review before merge |
| `auto_merge` | false | Merge PRs automatically on approval |
| `watch_interval_secs` | 180 | How often to poll PR/CI status |

## Example Workflow

```bash
# Start a mission from multiple issues
tt mission start --issue 23 --issue 24 --issue 25

# Run the autonomous dispatcher
tt mission dispatch

# Check mission status
tt mission status

# View detailed work items
tt mission status --work

# Stop a mission gracefully
tt mission stop <run-id>

# Resume a stopped mission
tt mission resume <run-id>
```

## Dispatcher Loop

The mission dispatcher runs every 30 seconds (configurable) and:
1. Loads active missions from Redis
2. Checks due watch items, executes triggers
3. Promotes pending work items to ready when dependencies satisfied
4. Matches ready items to idle agents by role fit
5. Enforces reviewer gates before advancing
6. Marks mission completed when no items remain

## Agent Routing

Work items are matched to agents using role-fit scoring:
1. **Exact match**: `owner_role: "backend"` → agent with backend role
2. **Generic fallback**: Any idle worker
3. **Load balancing**: Avoid assigning too many items to one agent
4. **Reviewer reservation**: Keep reviewer available for gates

## Redis Storage

Missions persist in Redis with these keys:

```
tt:{town}:mission:{run_id}          # MissionRun metadata
tt:{town}:mission:{run_id}:work     # WorkItem collection
tt:{town}:mission:{run_id}:watch    # WatchItem collection
tt:{town}:mission:{run_id}:events   # Activity log (last 100)
tt:{town}:mission:active            # Set of active MissionIds
```

## See Also

- [tt mission CLI Reference](../cli/mission.md)
- [Mission Mode Tutorial](../tutorials/mission-mode.md)
- [Tasks Concept](./tasks.md)
- [Coordination](./coordination.md)
