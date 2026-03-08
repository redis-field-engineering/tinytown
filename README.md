# 🏘️ Tinytown

> **Simple multi-agent orchestration using Redis** — All the power of complex systems, none of the complexity.

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Redis](https://img.shields.io/badge/Redis-8.0+-red?logo=redis)](https://redis.io/downloads/)

*Named after [Tinytown, Colorado](https://en.wikipedia.org/wiki/Tiny_Town,_Colorado) — a miniature village with big charm.*

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
| **Message latency** | <1ms (Unix socket) | 10-100ms |
| **Lines of code** | ~1,000 | 50,000+ |

## 🚀 Quick Start

```bash
# 1. Initialize a new town
tt init --name my-town

# 2. Spawn an agent
tt spawn worker-1 --model claude

# 3. Assign a task
tt assign worker-1 "Fix the bug in auth.rs"
```

That's it! Your agents are now coordinating via Redis.

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
| `tt init` | Initialize a new town |
| `tt spawn <name>` | Create a new agent |
| `tt assign <agent> <task>` | Assign a task |
| `tt list` | List all agents |
| `tt status` | Show town status |
| `tt start` | Start the town |
| `tt stop` | Stop the town |

## 🤖 Supported Models

Built-in presets for popular AI coding agents:

| Model | Command |
|-------|---------|
| `claude` | `claude --print` |
| `auggie` | `augment` |
| `codex` | `codex` |
| `gemini` | `gemini` |
| `copilot` | `gh copilot` |
| `aider` | `aider` |
| `cursor` | `cursor` |

```bash
# Use any built-in model
tt spawn worker-1 --model claude
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

