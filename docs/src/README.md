# Tinytown

> **Redis-native multi-agent orchestration** built for fast feedback, direct control, and practical coordination.

Welcome to Tinytown! 🏘️

Tinytown is a compact, fast multi-agent orchestration system. It lets you coordinate AI coding agents (Claude, Augment, Codex, and more) using Redis for message passing.

## Why Tinytown?

Tinytown started as a quick, intentionally small alternative to larger orchestration systems. It is still fast because Redis keeps the core runtime lightweight, but real multi-agent coding workflows have required some scheduling, recovery, and state-management complexity.

| What you want | Complex systems | Tinytown |
|---------------|-----------------|----------|
| **Get started** | Hours of setup | 30 seconds |
| **Understand it** | 50+ concepts | 7 core concepts |
| **Configure it** | 10+ config files | 1 TOML file |
| **Debug it** | Navigate 300K+ lines | Work in ~15K lines of production Rust |

## Core Philosophy

**Simplicity is still a feature, but this project is no longer pretending orchestration is trivial.**

Tinytown still tries to keep the core model tight. We include the pieces that have proven necessary in practice:

✅ Spawn and manage agents  
✅ Assign tasks and track state  
✅ Keep unassigned work in a shared backlog  
✅ Pass messages between agents  
✅ Persist work in Redis  

And we still avoid a lot of heavier machinery:

❌ Complex workflow DAGs  
❌ Distributed transactions  
❌ Recovery daemons  
❌ Multi-layer databases  

Today the repo is roughly 15K lines of production Rust, about 19K including tests, with 173 tests. That is no longer "tiny," but it is still small enough for one team to understand end to end.

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
