# tt status

Show town status.

## Synopsis

```bash
tt status [OPTIONS]
```

## Description

Displays comprehensive status of the town including:
- Town name and location
- Redis connection info
- All agents with their states and pending messages
- Message type breakdown for pending inbox items (tasks, queries, informational, confirmations)
- **With `--deep`**: Recent activity from each agent

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--deep` | | Show recent agent activity (stored in Redis) |
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Basic Status

```bash
tt status
```

Output:
```
🏘️  Town: my-project
📂 Root: /Users/you/projects/my-project
📡 Redis: unix:///Users/you/projects/my-project/redis.sock
🤖 Agents: 3
   backend (Working) - 0 messages pending
   frontend (Idle) - 2 messages pending (tasks: 1, queries: 1, informational: 0, confirmations: 0)
   reviewer (Idle) - 1 messages pending (tasks: 0, queries: 0, informational: 1, confirmations: 0)
```

### Deep Status (with stats and activity)

```bash
tt status --deep
```

Output:
```
🏘️  Town: my-project
📂 Root: /Users/you/projects/my-project
📡 Redis: unix:///Users/you/projects/my-project/redis.sock
🤖 Agents: 3
   backend (Working) - 0 pending, 12 rounds, uptime 1h 23m
      └─ Round 12: ✅ completed
      └─ Round 11: ✅ completed
   frontend (Idle) - 2 pending (tasks: 1, queries: 1, informational: 0, confirmations: 0), 5 rounds, uptime 45m 12s
      └─ Round 5: ✅ completed
   reviewer (Idle) - 1 pending (tasks: 0, queries: 0, informational: 1, confirmations: 0), 2 rounds, uptime 30m 5s
      └─ Round 2: ⚠️ model error

📊 Stats: rounds completed, uptime since spawn
```

## Stats Shown

| Stat | Description |
|------|-------------|
| **Rounds** | Number of agent loop iterations completed |
| **Uptime** | Time since agent was spawned |
| **Pending** | Messages waiting in inbox |
| **Message Types** | Pending breakdown: tasks, queries, informational, confirmations |
| **Activity** | Recent round results (last 5) |

## Output Fields

| Field | Description |
|-------|-------------|
| Town | Name from `tinytown.toml` |
| Root | Absolute path to town directory |
| Redis | Connection URL (socket or TCP) |
| Agents | Count and details |

## Agent Details

For each agent:
- **Name** — Human-readable identifier
- **State** — Current lifecycle state
- **Messages** — Number of pending inbox messages
- **Type Breakdown** — Pending messages grouped as tasks, queries, informational, confirmations

## Interpreting Status

| Situation | Meaning | Action |
|-----------|---------|--------|
| Agent `Idle` + 0 messages | Ready for work | Assign a task |
| Agent `Idle` + N messages | Messages waiting | Agent should process |
| Agent `Working` | Busy with task | Wait or check progress |
| Agent `Error` | Something failed | Check logs, respawn |

## Related Commands

| Command | When to Use |
|---------|-------------|
| `tt status` | Overview of everything |
| `tt list` | Just agent names and states |

## Direct Redis Inspection

For more detail:

```bash
# Connect to Redis
redis-cli -s ./redis.sock

# List all keys
KEYS tt:*

# Check specific inbox
LLEN tt:inbox:550e8400-e29b-41d4-a716-446655440000

# View agent state
GET tt:agent:550e8400-e29b-41d4-a716-446655440000
```

## See Also

- [tt list](./list.md) — Simple agent list
- [tt start](./status.md) — Keep town running
- [Towns Concept](../concepts/towns.md)
