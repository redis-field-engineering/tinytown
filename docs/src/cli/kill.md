# tt kill

Stop an agent gracefully.

## Synopsis

```bash
tt kill <AGENT>
```

## Description

Requests an agent to stop gracefully. The agent will:

1. Finish its current CLI run (if any)
2. Check the stop flag at the start of next round
3. Exit cleanly with state `Stopped`

**This is a graceful stop**, not an immediate kill. The agent completes its current work before stopping.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to stop |

## Examples

### Stop a Single Agent

```bash
tt kill backend
```

Output:
```
🛑 Requested stop for agent 'backend'
   Agent will stop at the start of its next round.
```

### Stop All Agents (Cleanup)

```bash
tt kill backend
tt kill frontend
tt kill reviewer
```

### Check That Agent Stopped

```bash
tt status
```

Output:
```
🤖 Agents: 3
   backend (Stopped) - 0 messages pending
   frontend (Idle) - 0 messages pending
   reviewer (Working) - 1 messages pending
```

## How It Works

1. Sets a stop flag in Redis: `tt:stop:<agent-id>`
2. Agent checks this flag at start of each round
3. If flag is set, agent exits loop gracefully
4. Flag has 1-hour TTL (auto-cleanup if agent already dead)

## When to Use

- **Work complete**: All tasks finished, clean up agents
- **Stuck agent**: Agent not making progress, stop and respawn
- **Resource cleanup**: Free up system resources
- **Reconfigure**: Stop agent to change CLI or settings

## See Also

- [tt spawn](./spawn.md) — Start new agents
- [tt status](./status.md) — Check agent states
- [Coordination](../concepts/coordination.md)
