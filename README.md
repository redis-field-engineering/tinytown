# 🏘️ Tinytown

> **Simple multi-agent orchestration using Redis** — All the power of complex systems, none of the complexity.

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Redis](https://img.shields.io/badge/Redis-8.0+-red?logo=redis)](https://redis.io/downloads/)
[![Docs](https://img.shields.io/badge/docs-mdBook-blue)](https://jeremyplichta.github.io/tinytown/)

*Named after [Tinytown, Colorado](https://en.wikipedia.org/wiki/Tiny_Town,_Colorado) — a miniature village with big charm.*

📚 **[Read the Documentation](https://jeremyplichta.github.io/tinytown/)** | 🚀 **[Getting Started Guide](https://jeremyplichta.github.io/tinytown/getting-started/quickstart.html)** | 🔄 **[Coming from Gastown?](https://jeremyplichta.github.io/tinytown/gastown/migration.html)**

## What is Tinytown?

Tinytown is a **minimal, blazing-fast multi-agent orchestration system** that lets you coordinate AI agents with Redis. It's designed for developers who want agent orchestration **without the bloat**.

Think of it as:
- **Gastown, but 100x simpler** ✨
- **Temporal, but for humans** 🧠
- **Airflow, but actually fun to use** 🎉

### Why Tinytown?

| Feature | Tinytown | Complex Systems |
|---------|----------|-----------------|
| **Setup time** | 30 seconds | Hours |
| **Config files** | 1 JSON | 10+ YAML files |
| **Core concepts** | 5 types | 50+ concepts |
| **CLI commands** | 13 | 50+ |
| **Message latency** | <1ms (Unix socket) | 10-100ms |
| **Lines of code** | ~2,600 | 50,000+ |

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

# 4. Or use the conductor - an AI that orchestrates for you
tt conductor
# Conductor: "I'll spawn agents and assign tasks. What do you want to build?"
```

That's it! Your agents are now coordinating via Redis.

> **Note:** `tt bootstrap` delegates to an AI agent to download Redis from GitHub and compile it for your machine. Alternatively: `brew install redis` (macOS) or `apt install redis-server` (Ubuntu).

## 💻 Code Example

```rust
use tinytown::{Town, Task, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to town (auto-starts Redis if needed)
    let town = Town::connect("./mytown").await?;
    
    // Spawn an agent
    let agent = town.spawn_agent("worker-1", "claude").await?;
    
    // Assign a task
    let task = Task::new("Implement the new API endpoint");
    agent.assign(task).await?;
    
    // Wait for completion
    agent.wait().await?;
    
    println!("✅ Task completed!");
    Ok(())
}
```

## 🏗️ Architecture

Tinytown is built on **5 core types**:

| Type | Purpose |
|------|---------|
| **Town** 🏘️ | Central orchestration hub, manages Redis & agents |
| **Agent** 🤖 | Workers that execute tasks (Claude, Auggie, Codex, Gemini, Copilot, Aider, Cursor, or custom) |
| **Task** 📋 | Units of work with state tracking |
| **Message** 💬 | Inter-agent communication with priorities |
| **Channel** 📡 | Redis-based message passing (<1ms latency) |

```
┌─────────────────────────────────────────┐
│           Your Application              │
└──────────────┬──────────────────────────┘
               │
        ┌──────▼───────┐
        │    Town      │  (Orchestrator)
        └──────┬───────┘
               │
        ┌──────▼──────────────────┐
        │   Redis (Unix Socket)   │  <1ms latency
        └──────┬──────────────────┘
               │
    ┌──────────┼──────────────┐
    ▼          ▼              ▼
┌────────┐ ┌────────┐    ┌────────┐
│ Agent1 │ │ Agent2 │ .. │ AgentN │
└────────┘ └────────┘    └────────┘
```

## 🎮 CLI Commands

| Command | Description |
|---------|-------------|
| `tt bootstrap [version]` | Download & build Redis (uses AI agent) |
| `tt init` | Initialize a new town |
| `tt spawn <name>` | Create a new agent (starts AI process!) |
| `tt assign <agent> <task>` | Assign a task |
| `tt list` | List all agents |
| `tt status [--deep]` | Show town status (--deep for activity) |
| `tt kill <agent>` | Stop an agent gracefully |
| `tt inbox <agent>` | Check agent's message inbox |
| `tt send [--urgent] <agent> <msg>` | Send message to agent |
| `tt conductor` | 🚂 AI orchestrator mode |
| `tt plan --init` | Create tasks.toml for planning |
| `tt sync [push\|pull]` | Sync tasks.toml ↔ Redis |
| `tt save` | Save Redis state to AOF (for git) |
| `tt restore` | Restore Redis state from AOF |

## 🤖 Supported Agent CLIs

Built-in presets for popular AI coding agents (with correct non-interactive flags):

| CLI | Command |
|-----|---------|
| `claude` | `claude --print --dangerously-skip-permissions` |
| `auggie` | `auggie --print` |
| `codex` | `codex exec --dangerously-bypass-approvals-and-sandbox` |
| `aider` | `aider --yes --no-auto-commits --message` |
| `gemini` | `gemini` |
| `copilot` | `gh copilot` |
| `cursor` | `cursor` |

```bash
# Spawn uses default CLI from config (or override)
tt spawn worker-1
tt spawn worker-2 --model auggie
tt spawn worker-3 --model codex
```

## ⚙️ Configuration

Single `tinytown.json` file:

```json
{
  "name": "my-town",
  "redis": {
    "use_socket": true,
    "socket_path": "redis.sock"
  },
  "default_model": "claude",
  "max_agents": 10
}
```

### Setting the Default CLI

Change `default_model` in `tinytown.json` to set which AI CLI is used when spawning agents:

```json
{
  "default_model": "auggie"
}
```

Available options: `claude`, `auggie`, `codex`, `aider`, `gemini`, `copilot`, `cursor`

Or override per-agent:

```bash
tt spawn backend              # Uses default_model from config
tt spawn frontend --model auggie   # Override for this agent
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

## 📦 Installation

### Prerequisites

**Redis 8.0+** is required. Tinytown will check your Redis version on startup.

#### macOS
```bash
brew install redis
```

#### Linux (Ubuntu/Debian)
```bash
# Add Redis repository and install
curl -fsSL https://packages.redis.io/gpg | sudo gpg --dearmor -o /usr/share/keyrings/redis-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/redis-archive-keyring.gpg] https://packages.redis.io/deb $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/redis.list
sudo apt-get update
sudo apt-get install redis
```

#### From Source
```bash
# Build from source - see https://redis.io/downloads/ for details
curl -O https://download.redis.io/redis-stable.tar.gz
tar xzf redis-stable.tar.gz
cd redis-stable && make && sudo make install
```

For more options, see the [official Redis downloads page](https://redis.io/downloads/).

### Install Tinytown

```bash
git clone https://github.com/jeremyplichta/tinytown.git
cd tinytown
cargo build --release
cargo install --path .
```

**Requirements:** Rust 1.85+, Redis 8.0+

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

