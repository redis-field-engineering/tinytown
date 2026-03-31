# Channels

The **Channel** is Tinytown's message transport layer. It's a thin wrapper around Redis that provides queues, pub/sub, and state storage.

## Why Redis?

Redis is perfect for agent orchestration:

| Feature | Benefit |
|---------|---------|
| **Unix sockets** | Sub-millisecond latency |
| **Lists** | Perfect for message queues |
| **BLPOP** | Efficient blocking receive |
| **Pub/Sub** | Broadcast to all agents |
| **Persistence** | Survives crashes |
| **Simple** | No complex setup |

## Channel Operations

### Send a Message

```rust
channel.send(&message).await?;
```

Messages go to the recipient's inbox (`tt:<town>:inbox:<agent-id>`).

Priority handling:
- `Urgent` / `High` → `LPUSH` (front of queue)
- `Normal` / `Low` → `RPUSH` (back of queue)

### Receive a Message

```rust
// Blocking (waits up to timeout)
let msg = channel.receive(agent_id, Duration::from_secs(30)).await?;

// Non-blocking
let msg = channel.try_receive(agent_id).await?;
```

Uses `BLPOP` for efficient waiting without polling.

### Check Inbox Length

```rust
let pending = channel.inbox_len(agent_id).await?;
println!("{} messages waiting", pending);
```

### Broadcast

```rust
channel.broadcast(&message).await?;
```

Uses Redis Pub/Sub (`PUBLISH tt:broadcast`).

## State Storage

The channel also stores agent and task state:

### Agent State

```rust
// Store
channel.set_agent_state(&agent).await?;

// Retrieve
let agent = channel.get_agent_state(agent_id).await?;
```

Stored at: `tt:<town>:agent:<uuid>`

### Task State

```rust
// Store
channel.set_task(&task).await?;

// Retrieve
let task = channel.get_task(task_id).await?;
```

Stored at: `tt:<town>:task:<uuid>`

## Redis Key Patterns

Keys are town-isolated to allow multiple towns to share the same Redis instance:

| Pattern | Type | Purpose |
|---------|------|---------|
| `tt:<town>:inbox:<uuid>` | List | Agent message queue |
| `tt:<town>:agent:<uuid>` | String | Agent state (JSON) |
| `tt:<town>:task:<uuid>` | String | Task state (JSON) |
| `tt:broadcast` | Pub/Sub | Broadcast channel |

See [tt migrate](../cli/migrate.md) for upgrading from older key formats.

## Direct Redis Access

Sometimes you want to query Redis directly:

```bash
# Connect to town's Redis
redis-cli -s ./redis.sock

# List all agent inboxes for your town
KEYS tt:<town_name>:inbox:*

# Check inbox length
LLEN tt:<town_name>:inbox:550e8400-e29b-41d4-a716-446655440000

# View agent state
GET tt:<town_name>:agent:550e8400-e29b-41d4-a716-446655440000

# Monitor all messages
MONITOR
```

## Performance

Unix socket performance is excellent:

| Operation | Latency |
|-----------|---------|
| Send message | ~0.1ms |
| Receive (cached) | ~0.1ms |
| State get/set | ~0.1ms |
| TCP equivalent | ~1-2ms |

For local development, this means near-instant coordination.

## Persistence

Tinytown's managed local Redis uses Redis's default RDB snapshot behavior, so channel state is normally recovered from the local snapshot after a restart instead of starting empty.

For tighter durability guarantees, or when you want every write appended to disk, add AOF on top of the default snapshots:

### Option 1: RDB Snapshots

```bash
redis-cli -s ./redis.sock CONFIG SET save "60 1"
```

Saves every 60 seconds if at least 1 key changed.

### Option 2: AOF (Append Only File)

```bash
redis-cli -s ./redis.sock CONFIG SET appendonly yes
```

Logs every write for full durability.

## Creating a Channel

Usually you don't create channels directly—the Town does it:

```rust
// Get channel from town
let channel = town.channel();

// Or create manually (advanced)
use redis::aio::ConnectionManager;
let client = redis::Client::open("unix:///path/to/redis.sock")?;
let conn = ConnectionManager::new(client).await?;
let channel = Channel::new(conn);
```
