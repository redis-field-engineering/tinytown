# Quick Start

Let's get a multi-agent workflow running in under 5 minutes.

> **First time?** Make sure you've [installed Tinytown](./installation.md) first.

## Step 1: Initialize a Town

A **Town** is your orchestration workspace. It manages Redis, agents, and message passing.

```bash
# Create a project directory
mkdir my-project && cd my-project

# Initialize the town
tt init --name my-project
```

You'll see:
```
✨ Initialized town 'my-project' at .
📡 Redis running with Unix socket for fast message passing
🚀 Run 'tt spawn <name>' to create agents
```

This creates:
- `tinytown.toml` — Configuration file
- `agents/` — Agent working directories
- `logs/` — Activity logs
- `tasks/` — Task storage
- `redis.sock` — Unix socket for fast Redis communication

## Step 2: Spawn an Agent

Agents are workers that execute tasks. Spawn one:

```bash
tt spawn worker-1
```

Output:
```
🤖 Spawned agent 'worker-1' using CLI 'claude'
   ID: 550e8400-e29b-41d4-a716-446655440000
```

The agent uses `default_cli` from your config. Spawn more:
```bash
tt spawn worker-2
tt spawn reviewer
```

Or override with `--cli`:
```bash
tt spawn specialist --cli codex-mini
```

## Step 3: Assign Tasks

Give your agents something to do:

```bash
tt assign worker-1 "Implement the user login API endpoint"
tt assign worker-2 "Write tests for the login API"
tt assign reviewer "Review the login implementation when ready"
```

## Step 4: Check Status

See what's happening in your town:

```bash
tt status
```

Output:
```
🏘️  Town: my-project
📂 Root: /path/to/my-project
📡 Redis: unix:///path/to/my-project/redis.sock
🤖 Agents: 3
   worker-1 (Working) - 0 messages pending
   worker-2 (Idle) - 1 messages pending
   reviewer (Idle) - 1 messages pending
```

List all agents:
```bash
tt list
```

## Step 5: Keep It Running

To keep a town connection open during development:

```bash
tt start
```

Press `Ctrl+C` to stop gracefully.

## What Just Happened?

You created a **Town** with three **Agents**. Each agent received a **Task** via a **Message** sent through a Redis **Channel**.

That's the entire Tinytown model:

```
Town → spawns → Agents
       ↓
     Channel (Redis)
       ↓
     Messages → contain → Tasks
```

## Bonus: Planning with tasks.toml

For larger projects, define tasks in a file instead of CLI commands:

```bash
# Initialize a task plan
tt plan --init
```

Edit `tasks.toml`:

```toml
[[tasks]]
id = "login-api"
description = "Implement the user login API endpoint"
agent = "worker-1"
status = "pending"

[[tasks]]
id = "login-tests"
description = "Write tests for the login API"
agent = "worker-2"
status = "pending"
parent = "login-api"
```

Sync to Redis:

```bash
tt sync push
```

Now your tasks are version-controlled and can be reviewed in PRs. See [tt plan](../cli/plan.md) for more.

## Next Steps

- **[Your First Town](./first-town.md)** — Deeper dive into town setup
- **[Core Concepts](../concepts/overview.md)** — Understand the 5 core types
- **[Single Agent Workflow](../tutorials/single-agent.md)** — Complete tutorial
