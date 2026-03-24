# Towns

A **Town** is your orchestration workspace. It's the top-level container that manages Redis, agents, and coordination.

## What a Town Contains

```
my-project/           # Town root
├── tinytown.toml     # Configuration
├── .gitignore        # Auto-updated to exclude .tt/
└── .tt/              # Runtime artifacts (gitignored)
    ├── redis.sock    # Unix socket (when running)
    ├── redis.pid     # Redis process ID
    ├── redis.aof     # Redis persistence (if enabled)
    ├── agents/       # Agent working directories
    ├── logs/         # Activity logs
    └── tasks/        # Task storage
```

All runtime artifacts are stored under `.tt/` which is automatically added to `.gitignore` during `tt init`. This keeps your repository clean and prevents accidental commits of logs, sockets, and other temporary files.

## Creating a Town

### CLI

```bash
tt init --name my-project
```

### Rust API

```rust
use tinytown::{Town, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize a new town
    let town = Town::init("./my-project", "my-project").await?;
    
    // Town is now running with Redis started
    Ok(())
}
```

## Connecting to an Existing Town

```rust
// Connect to existing town (starts Redis if needed)
let town = Town::connect("./my-project").await?;
```

## Town Configuration

The `tinytown.toml` file:

```toml
name = "my-project"
default_cli = "claude"
max_agents = 10

[redis]
use_socket = true
socket_path = ".tt/redis.sock"
host = "127.0.0.1"
port = 6379

[agent_clis.claude]
name = "claude"
command = "claude --print --dangerously-skip-permissions"

[agent_clis.auggie]
name = "auggie"
command = "auggie --print"

[agent_clis.codex-mini]
name = "codex-mini"
command = "codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort=\"medium\""
```

### Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `name` | Directory name | Human-readable town name |
| `redis.use_socket` | `true` | Use Unix socket (faster) vs TCP |
| `redis.socket_path` | `.tt/redis.sock` | Socket file path (under .tt/) |
| `redis.host` | `127.0.0.1` | TCP host (if not using socket) |
| `redis.port` | `6379` | TCP port (if not using socket) |
| `default_cli` | `claude` | Default CLI for new agents |
| `max_agents` | `10` | Maximum concurrent agents |

## Town Lifecycle

```
┌─────────────┐
│   init()    │ ── Creates directories, config, starts Redis
└──────┬──────┘
       │
       ▼
┌─────────────┐
│   running   │ ── Agents can be spawned, tasks assigned
└──────┬──────┘
       │
       ▼
┌─────────────┐
│    drop     │ ── Redis stopped, cleanup
└─────────────┘
```

## Town Methods

```rust
// Spawn a new agent
let agent = town.spawn_agent("worker-1", "claude").await?;

// Get handle to existing agent
let agent = town.agent("worker-1").await?;

// List all agents
let agents = town.list_agents().await;

// Access the channel directly
let channel = town.channel();

// Get configuration
let config = town.config();

// Get root directory
let root = town.root();
```

## Redis Management

Tinytown automatically manages Redis:

1. **On `init()`**: Starts `redis-server` with Unix socket
2. **On `connect()`**: Connects to existing, or starts if needed
3. **On `drop`**: Stops Redis gracefully

### Unix Socket vs TCP

Unix sockets are **~10x faster** for local communication:

| Mode | Latency | Use Case |
|------|---------|----------|
| Unix Socket | ~0.1ms | Local development (default) |
| TCP | ~1ms | Remote Redis, Docker |

To use TCP instead:
```toml
[redis]
use_socket = false
host = "redis.example.com"
port = 6379
```
