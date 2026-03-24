# Tinytown

> **Simple multi-agent orchestration using Redis** — All the power, none of the complexity.

Welcome to Tinytown! 🏘️

Tinytown is a minimal, blazing-fast multi-agent orchestration system. It lets you coordinate AI coding agents (Claude, Augment, Codex, and more) using Redis for message passing.

## Why Tinytown?

If you've tried to set up complex orchestration systems like Gastown and found yourself drowning in configuration files, agent taxonomies, and recovery mechanisms — Tinytown is for you.

| What you want | Complex systems | Tinytown |
|---------------|-----------------|----------|
| **Get started** | Hours of setup | 30 seconds |
| **Understand it** | 50+ concepts | 5 types |
| **Configure it** | 10+ config files | 1 TOML file |
| **Debug it** | Navigate 300K+ lines | Read 1,400 lines |

## Core Philosophy

**Simplicity is a feature, not a limitation.**

Tinytown does less, so you can do more. We include only what you need:

✅ Spawn and manage agents  
✅ Assign tasks and track state  
✅ Keep unassigned work in a shared backlog  
✅ Pass messages between agents  
✅ Persist work in Redis  

And we deliberately leave out:

❌ Complex workflow DAGs  
❌ Distributed transactions  
❌ Recovery daemons  
❌ Multi-layer databases  

When you need those features, you'll know — and you can add them yourself in a few lines of code, or upgrade to a more complex system.

## Quick Example

```bash
# Initialize a town
tt init --name my-project

# Spawn agents (uses the default CLI, or specify with --cli)
tt spawn frontend
tt spawn backend
tt spawn reviewer

# Assign tasks
tt assign frontend "Build the login page"
tt assign backend "Create the auth API"
tt assign reviewer "Review PRs when ready"

# Or park unassigned tasks for role-based claiming
tt backlog add "Harden auth error handling" --tags backend,security
tt backlog list

# Check status
tt status

# Or let the conductor orchestrate for you!
tt conductor
# "Build a user authentication system"
# Conductor spawns agents, breaks down tasks, and coordinates...
```

That's it. Your agents are now coordinating via Redis.

## Plan Work with tasks.toml

For complex workflows, define tasks in a file:

```bash
tt plan --init   # Creates tasks.toml
```

Edit `tasks.toml` to define your pipeline:

```toml
[[tasks]]
id = "auth-api"
description = "Build the auth API"
agent = "backend"
status = "pending"

[[tasks]]
id = "auth-tests"
description = "Write auth tests"
agent = "tester"
parent = "auth-api"
status = "pending"
```

Then sync to Redis and let agents work:

```bash
tt sync push
tt conductor
```

See [tt plan](./cli/plan.md) for the full task DSL.

## What's Next?

- **[Installation](./getting-started/installation.md)** — Get Tinytown running in 30 seconds
- **[Quick Start](./getting-started/quickstart.md)** — Your first multi-agent workflow
- **[Core Concepts](./concepts/overview.md)** — Understand Towns, Agents, Tasks, Messages, and Channels
- **[Townhall REST API](./advanced/townhall-rest.md)** — HTTP control plane for automation
- **[Townhall MCP Server](./advanced/townhall-mcp.md)** — MCP tools/resources/prompts for LLM clients
- **[Coming from Gastown?](./gastown/migration.md)** — Migration guide for Gastown users

## Named After

[Tiny Town, Colorado](https://en.wikipedia.org/wiki/Tiny_Town,_Colorado) — a miniature village with big charm, just like this project! 🏔️
