# tt close

Gracefully close an agent (drain current work, then stop).

## Synopsis

```bash
tt close <AGENT>
```

## Description

Requests an agent to finish its current work and then stop cleanly. The agent transitions through `Draining` → `Cold` states:

1. **Draining**: Agent finishes processing its current message/task
2. **Cold**: Agent has completed draining and is shut down

This is a gentler alternative to `tt kill`, which stops the agent at the start of its next round without waiting for current work to complete.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to close |

## Examples

```bash
tt close backend
```

Output:
```
🔻 Closing agent 'backend' (draining current work, then stopping)
```

## When to Use

- **Clean shutdown**: When you want the agent to finish its current task before stopping
- **End of session**: Gracefully wind down agents at the end of a work session
- **Differs from `tt kill`**: `kill` stops at the next round boundary; `close` lets current work complete first

## See Also

- [tt kill](./kill.md) — Stop an agent at the next round boundary
- [tt interrupt](./interrupt.md) — Pause without stopping
- [Agents Concept](../concepts/agents.md)
