# 🏘️ Tinytown

> **Redis-native multi-agent orchestration** built for fast feedback, direct control, and practical coordination.

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Redis](https://img.shields.io/badge/Redis-8.0+-red?logo=redis)](https://redis.io/downloads/)
[![Docs](https://img.shields.io/badge/docs-mdBook-blue)](https://redis-field-engineering.github.io/tinytown/)

*Named after [Tinytown, Colorado](https://en.wikipedia.org/wiki/Tiny_Town,_Colorado) — a miniature village with big charm.*

📚 **[Read the Documentation](https://redis-field-engineering.github.io/tinytown/)** | 🚀 **[Getting Started Guide](https://redis-field-engineering.github.io/tinytown/getting-started/quickstart.html)** | 🔄 **[Coming from Gastown?](https://redis-field-engineering.github.io/tinytown/gastown/migration.html)**

## What is Tinytown?

Tinytown is a **compact, fast multi-agent orchestration system** that lets you coordinate AI agents with Redis. It started as a deliberately small alternative to larger orchestration systems, and it is still fast because Redis keeps the runtime simple and responsive.

Think of it as:
- **A smaller, easier-to-inspect orchestration stack**
- **Temporal, but for humans** 🧠
- **Airflow, but actually fun to use** 🎉

Tinytown is no longer a tiny prototype, and the repo should not pretend otherwise. Building real agent-to-agent coding workflows turned out to require some durable state, recovery paths, scheduling, and coordination logic. The project goal is not "no complexity"; it is keeping the necessary complexity visible, local, and understandable.

### Why Tinytown?

| Feature | Tinytown | Larger Systems |
|---------|----------|-----------------|
| **Setup time** | 30 seconds | Hours |
| **Config files** | 1 TOML | 10+ YAML files |
| **Core concepts** | 7 core concepts | 50+ concepts |
| **CLI commands** | 30+ | 50+ |
| **Message latency** | <1ms (Unix socket) | 10-100ms |
| **Production Rust code** | ~15,000 lines | 50,000+ |

Current repo size, as of March 27, 2026:
- ~15.4K lines of Rust code in `src/`
- ~18.7K lines of Rust code across `src/` and `tests/`
- 173 tests

## 📦 Install

```bash
cargo install tinytown
```

**Prerequisites:** [Rust 1.85+](https://rustup.rs/) and [Redis 8.0+](https://redis.io/downloads/) (or use `tt bootstrap` below).

## 🚀 Quick Start

```bash
# 0. Bootstrap Redis (one-time setup, uses AI to download & build)
tt bootstrap
export PATH="$HOME/.tt/bin:$PATH"

# 1. Initialize a new town (auto-names from git repo+branch)
tt init
# Creates town "my-repo-feature-branch"

# 2. Spawn an agent (uses default CLI from config)
tt spawn worker-1

# 3. Assign a task
tt assign worker-1 "Fix the bug in auth.rs"

# 3b. Add unassigned work to backlog (optional)
tt backlog add "Review auth error messages" --tags backend,review
tt backlog list

# 4. Or use the conductor - an AI that orchestrates for you
tt conductor
# Conductor: "I'll spawn agents and assign tasks. What do you want to build?"
```

That's it! Your agents are now coordinating via Redis.

> **Note:** `tt bootstrap` delegates to an AI agent to download Redis from GitHub and compile it for your machine. Alternatively: `brew install redis` (macOS) or `apt install redis-server` (Ubuntu).

## 🎯 Mission Mode

Start an autonomous mission that handles multiple GitHub issues with dependency-aware scheduling:

```bash
# Start a mission spanning multiple issues
tt mission start --issue 23 --issue 24 --issue 25

# Check mission status and work items
tt mission status --work

# List all missions
tt mission list
```

Mission mode provides:
- **Durable scheduling** — State persisted in Redis, survives restarts
- **Dependency tracking** — Work items execute in DAG order
- **PR/CI monitoring** — Automatic watch loops for CI status, Bugbot, reviews
- **Agent routing** — Work assigned to best-fit agents by role

## 🌐 Programmatic Interfaces

Tinytown also ships a `townhall` control plane binary with both REST and MCP interfaces.

```bash
# REST API (default: 127.0.0.1:8080)
townhall rest

# MCP over stdio
townhall mcp-stdio

# MCP over HTTP/SSE (default: REST port + 1)
townhall mcp-http
```

REST OpenAPI spec: `docs/openapi/townhall-v1.yaml`

## 🏗️ Architecture

Tinytown is built on **7 core concepts**:

| Concept | Purpose |
|---------|---------|
| **Conductor** 🚂 | AI orchestrator that manages agents and assigns tasks |
| **Town** 🏘️ | Project workspace with Redis state and agent registry |
| **Agent** 🤖 | AI workers (Claude, Auggie, Codex, Gemini, Copilot, Aider, Cursor) |
| **Task** 📋 | Units of work with state tracking |
| **Message** 💬 | Inter-agent communication with priorities |
| **Channel** 📡 | Redis-based message passing (<1ms latency) |
| **Mission** 🎯 | Autonomous multi-issue execution with durable scheduling |

```
        ┌─────────────────┐
        │   Conductor 🚂  │  (You talk to this)
        └────────┬────────┘
                 │ spawns, assigns, coordinates
        ┌────────▼────────┐
        │    Town 🏘️     │
        └────────┬────────┘
                 │
        ┌────────▼────────────────┐
        │   Redis (Unix Socket)   │  <1ms latency
        └────────┬────────────────┘
                 │
    ┌────────────┼────────────┐
    ▼            ▼            ▼
┌────────┐  ┌────────┐   ┌────────┐
│ Agent1 │  │ Agent2 │ … │ AgentN │
└────────┘  └────────┘   └────────┘
```

## 🎮 CLI Commands

| Command | Description |
|---------|-------------|
| `tt bootstrap [version]` | Download & build Redis (uses AI agent) |
| `tt init` | Initialize a new town |
| `tt spawn <name>` | Create a new agent (starts AI process!) |
| `tt assign <agent> <task>` | Assign a task |
| `tt send <agent> <msg>` | Send message to agent (`--query`, `--info`, `--urgent`) |
| `tt list` | List all agents |
| `tt status [--deep] [--tasks]` | Show town status with agent labels (`Nickname [role]`) |
| `tt inbox [agent] [--all]` | Check agent message inbox(es) |
| `tt history [-n N] [--agent <name>]` | Show recent communication history |
| `tt events [--count N] [--follow]` | Tail the event stream |
| `tt kill <agent>` | Stop an agent gracefully |
| `tt wait <agent> [--timeout]` | Wait for agent to finish |
| `tt restart <agent>` | Restart a stopped agent |
| `tt interrupt <agent>` | Pause a running agent |
| `tt resume <agent>` | Resume a paused agent |
| `tt close <agent>` | Drain current work then stop |
| `tt prune [--all]` | Remove stopped/stale agents |
| `tt recover` | Detect and clean up crashed agents |
| `tt reclaim` | Recover orphaned tasks |
| `tt task <action>` | Manage tasks (show, list, complete, current) |
| `tt backlog <subcommand>` | Manage unassigned task backlog |
| `tt conductor` | 🚂 AI orchestrator mode |
| `tt mission <subcommand>` | Mission mode (start, dispatch, status, note, stop) |
| `tt plan --init` | Create tasks.toml for planning |
| `tt sync [push\|pull]` | Sync tasks.toml ↔ Redis |
| `tt save` / `tt restore` | Save/restore Redis state to AOF (for git) |
| `tt reset [--force]` | Reset all town state |
| `tt config [key] [value]` | View or set global config |
| `tt towns` | List all registered towns |

## 🏛️ Townhall (HTTP Control Plane)

Townhall exposes Tinytown operations via REST API and MCP (Model Context Protocol):

```bash
# Start REST API server (default port 8080)
townhall

# Start MCP server for Claude Desktop integration
townhall mcp-stdio
```

See [Townhall Documentation](https://redis-field-engineering.github.io/tinytown/advanced/townhall.html) for API reference and authentication options.

## 🤖 Supported Agent CLIs

Built-in presets for popular AI coding agents (with correct non-interactive flags):

| CLI | Command |
|-----|---------|
| `claude` | `claude --print --dangerously-skip-permissions` |
| `auggie` | `auggie --print` |
| `codex` | `codex exec --dangerously-bypass-approvals-and-sandbox` |
| `codex-mini` | `codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort="medium"` |
| `aider` | `aider --yes --no-auto-commits --message` |
| `gemini` | `gemini` |
| `copilot` | `gh copilot` |
| `cursor` | `cursor` |

```bash
# Spawn uses default CLI from config (or override)
tt spawn worker-1
tt spawn worker-2 --cli auggie
tt spawn worker-3 --cli codex-mini
```

## ⚙️ Configuration

Single `tinytown.toml` file:

```toml
name = "my-town"
default_cli = "claude"
max_agents = 10

[redis]
use_socket = true
socket_path = "redis.sock"
```

### Setting the Default CLI

Change `default_cli` in `tinytown.toml` to set which AI CLI is used when spawning agents:

```toml
default_cli = "auggie"
```

Available options: `claude`, `auggie`, `codex`, `codex-mini`, `aider`, `gemini`, `copilot`, `cursor`

Or override per-agent:

```bash
tt spawn backend              # Uses default_cli from config
tt spawn frontend --cli auggie     # Override for this agent
```

## 🎯 Design Philosophy

**Simplicity over features.** We include only what you need:

✅ Agent spawning & lifecycle  
✅ Task assignment & tracking  
✅ Redis message passing  
✅ Priority queues  
❌ No workflow DAGs  
❌ No distributed transactions  
❌ No complex scheduling  

If you need more, add it yourself in 10 lines.

## 🔧 Redis Installation

**Redis 8.0+** is required. Tinytown will check your Redis version on startup.

| Platform | Command |
|----------|---------|
| **macOS** | `brew install redis` |
| **Ubuntu/Debian** | See [Redis downloads](https://redis.io/downloads/) |
| **Any** | `tt bootstrap` (uses AI to build from source) |

## 🔧 Development

```bash
cargo build          # Build
cargo test           # Run tests
cargo clippy         # Lint
```

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

**Made with ❤️ by [Jeremy Plichta](https://github.com/jeremyplichta)**

*Tinytown: Simple multi-agent orchestration for humans.*
