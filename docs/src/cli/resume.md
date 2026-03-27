# tt resume

Resume a paused agent.

## Synopsis

```bash
tt resume <AGENT>
```

## Description

Resumes an agent that was previously paused with `tt interrupt`. The agent transitions from `Paused` back to `Idle` and immediately starts processing messages again.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to resume |

## Examples

```bash
tt resume backend
```

Output:
```
▶️  Resumed agent 'backend'
```

## How It Works

1. Sets agent state from `Paused` to `Idle` in Redis
2. Emits an `AgentResumed` structured event
3. The agent loop picks up the state change on its next 5-second check
4. Agent resumes processing inbox messages normally

## See Also

- [tt interrupt](./interrupt.md) — Pause an agent
- [tt close](./close.md) — Gracefully drain and stop
- [Agents Concept](../concepts/agents.md)
