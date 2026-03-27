# tt interrupt

Pause a running agent.

## Synopsis

```bash
tt interrupt <AGENT>
```

## Description

Pauses an agent immediately. The agent transitions to `Paused` state and stops processing messages until resumed with `tt resume`.

Unlike `tt kill`, the agent process **stays alive** — it enters a sleep loop, checking every 5 seconds for a resume signal. This is useful for temporarily halting an agent without losing its process or context.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to pause |

## Examples

### Pause an Agent

```bash
tt interrupt backend
```

Output:
```
⏸️  Interrupted agent 'backend'
   Agent is now paused. Use 'tt resume backend' to continue.
```

### Pause and Resume Workflow

```bash
tt interrupt backend          # Pause
# ... investigate an issue, send new instructions ...
tt send backend --urgent "Change approach: use Redis Streams instead"
tt resume backend             # Continue with new instructions
```

## How It Works

1. Sets agent state to `Paused` in Redis
2. Emits an `AgentInterrupted` structured event
3. The agent loop detects `Paused` state at the top of each iteration
4. Agent sleeps (5s intervals) until state changes back to `Idle` via `tt resume`
5. All state transitions to `Idle` are guarded — they won't overwrite `Paused`

## When to Use

- **Redirect work**: Pause an agent to change its instructions before it picks up the next message
- **Resource management**: Temporarily free up CPU/API quota without killing the process
- **Debugging**: Pause an agent to inspect its state or inbox
- **Coordination**: Hold an agent while waiting for a dependency from another agent

## See Also

- [tt resume](./resume.md) — Resume a paused agent
- [tt close](./close.md) — Gracefully drain and stop
- [tt kill](./kill.md) — Stop an agent entirely
- [Agents Concept](../concepts/agents.md)
