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
   frontend (Idle) - 2 messages pending
   reviewer (Idle) - 1 messages pending
```

### Deep Status (with activity)

```bash
tt status --deep
```

Output:
```
🏘️  Town: my-project
📂 Root: /Users/you/projects/my-project
📡 Redis: unix:///Users/you/projects/my-project/redis.sock
🤖 Agents: 3
   backend (Working) - 0 messages pending
      └─ Round 3: ✅ completed
      └─ Round 2: ✅ completed
      └─ Round 1: ✅ completed
   frontend (Idle) - 2 messages pending
      └─ Round 5: ✅ completed
   reviewer (Idle) - 1 messages pending
      └─ Round 2: ⚠️ model error

📊 Deep status shows last activity from each agent.
   Activity is stored in Redis with 1-hour TTL.
```

## Output Fields

| Field | Description |
|-------|-------------|
| Town | Name from `tinytown.json` |
| Root | Absolute path to town directory |
| Redis | Connection URL (socket or TCP) |
| Agents | Count and details |

## Agent Details

For each agent:
- **Name** — Human-readable identifier
- **State** — Current lifecycle state
- **Messages** — Number of pending inbox messages

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
KEYS mt:*

# Check specific inbox
LLEN mt:inbox:550e8400-e29b-41d4-a716-446655440000

# View agent state
GET mt:agent:550e8400-e29b-41d4-a716-446655440000
```

## See Also

- [tt list](./list.md) — Simple agent list
- [tt start](./status.md) — Keep town running
- [Towns Concept](../concepts/towns.md)

