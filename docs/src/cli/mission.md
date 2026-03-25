# tt mission

Autonomous multi-issue mission mode commands.

## Synopsis

```bash
tt mission start [OPTIONS]
tt mission status [OPTIONS]
tt mission resume <RUN_ID>
tt mission dispatch [--run <RUN_ID>] [--once]
tt mission stop <RUN_ID> [OPTIONS]
tt mission list [OPTIONS]
```

## Description

Mission mode enables durable, dependency-aware orchestration of multiple GitHub issues with automatic PR/CI monitoring. Use these commands to start, monitor, and control missions.

`tt mission start` bootstraps the mission graph and performs an initial scheduling pass. `tt mission dispatch` is the persistent runtime loop that keeps the mission moving, monitors watches, and assigns follow-up work.

## Subcommands

### tt mission start

Start a new mission with one or more objectives.

```bash
tt mission start --issue <ISSUE>... [--doc <PATH>...] [OPTIONS]
```

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--issue <ISSUE>` | `-i` | GitHub issue number or URL (repeatable) |
| `--doc <PATH>` | `-d` | Document path as objective (repeatable) |
| `--max-parallel <N>` | | Max parallel work items (default: 2) |
| `--no-reviewer` | | Disable reviewer requirement |

**Issue Formats:**
- `23` — Issue #23 in current repo
- `owner/repo#23` — Fully qualified issue
- `https://github.com/owner/repo/issues/23` — Full URL

**Examples:**

```bash
# Start with single issue
tt mission start --issue 23

# Multiple issues
tt mission start -i 23 -i 24 -i 25

# Cross-repo issues
tt mission start --issue my-org/other-repo#42

# Include a design doc
tt mission start --issue 23 --doc docs/design.md

# Allow more parallelism
tt mission start -i 23 -i 24 --max-parallel 4

# Skip reviewer gate
tt mission start -i 23 --no-reviewer
```

### tt mission status

Show status of missions.

```bash
tt mission status [--run <ID>] [--work] [--watch]
```

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--run <ID>` | `-r` | Show specific mission by ID |
| `--work` | | Show detailed work item status |
| `--watch` | | Show watch items (PR/CI monitors) |

**Examples:**

```bash
# Show all active missions
tt mission status

# Specific mission with work items
tt mission status --run abc123 --work

# Include watch items
tt mission status -r abc123 --work --watch
```

**Output:**

```
🎯 Mission Status
   ID: abc123-def456-...
   State: 🚀 Running
   Created: 2024-01-15 10:30:00 UTC
   Updated: 2024-01-15 11:45:00 UTC

📋 Objectives: 3
   - redis-field-engineering/tinytown#23
   - redis-field-engineering/tinytown#24
   - redis-field-engineering/tinytown#25

⚙️  Policy:
   Max parallel: 2
   Reviewer required: true
   Auto-merge: false
   Watch interval: 180s

⏰ Next Wake: 2024-01-15 11:48:00 UTC

📦 Work Items: 5
   🔵 ready    Issue #23: Implement auth flow
   🔄 running  Issue #24: Add rate limiting (→ backend)
   ⏳ pending  Issue #25: Write tests
```

### tt mission resume

Resume a stopped or blocked mission.

```bash
tt mission resume <RUN_ID>
```

**Examples:**

```bash
tt mission resume abc123-def456-...
```

### tt mission dispatch

Run the persistent dispatcher loop that owns mission progression.

```bash
tt mission dispatch [--run <RUN_ID>] [--once]
```

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--run <RUN_ID>` | `-r` | Restrict dispatch to one mission |
| `--once` | | Run a single dispatcher tick and exit |

**Examples:**

```bash
# Run dispatcher for all active missions
tt mission dispatch

# Single mission only
tt mission dispatch --run abc123-def456-...

# One-shot tick for debugging/tests
tt mission dispatch --run abc123-def456-... --once
```

### tt mission stop

Stop an active mission.

```bash
tt mission stop <RUN_ID> [--force]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--force` | Force stop without graceful cleanup |

**Examples:**

```bash
# Graceful stop (can be resumed)
tt mission stop abc123

# Force stop (cannot be resumed)
tt mission stop abc123 --force
```

### tt mission list

List all missions.

```bash
tt mission list [--all]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--all` | Include completed/failed missions |

**Examples:**

```bash
# Active missions only
tt mission list

# All missions including completed
tt mission list --all
```

## Work Item States

| State | Emoji | Description |
|-------|-------|-------------|
| Pending | ⏳ | Waiting for dependencies |
| Ready | 🔵 | Dependencies satisfied, can be assigned |
| Assigned | 📌 | Assigned to an agent |
| Running | 🔄 | Agent is actively working |
| Blocked | 🚧 | Waiting on external event |
| Done | ✅ | Completed successfully |

## See Also

- [Mission Mode Concept](../concepts/missions.md)
- [Mission Mode Tutorial](../tutorials/mission-mode.md)
- [tt status](./status.md)
- [tt conductor](./conductor.md)
