# tt wait

Wait for an agent to reach a terminal state.

## Synopsis

```bash
tt wait <AGENT> [--timeout <SECONDS>]
```

## Description

Blocks until the specified agent reaches a terminal state (`Stopped`, `Cold`, or `Error`). Useful for scripting sequential workflows where you need to wait for an agent to finish before proceeding.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to wait for |

## Options

| Option | Description |
|--------|-------------|
| `--timeout <SECONDS>` | Maximum seconds to wait (default: wait forever) |

## Examples

### Wait Forever

```bash
tt wait backend
```

Output:
```
⏳ Waiting for agent 'backend' to finish...
   Agent 'backend' reached state: 🛑 Stopped
```

### Wait with Timeout

```bash
tt wait backend --timeout 300
# Waits up to 5 minutes
```

### Scripted Workflow

```bash
tt spawn backend --role worker
tt assign backend "Build the API"
tt wait backend --timeout 600
tt spawn reviewer --role reviewer
tt assign reviewer "Review backend's work"
tt wait reviewer
echo "All done!"
```

## See Also

- [tt kill](./kill.md) — Stop an agent
- [tt close](./close.md) — Gracefully drain and stop
- [Agents Concept](../concepts/agents.md)
