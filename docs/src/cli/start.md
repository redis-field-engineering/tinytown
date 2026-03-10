# tt start

Keep a connection open to the current town.

## Synopsis

```bash
tt start [OPTIONS]
```

## Description

Connects to an existing town and keeps the process running until Ctrl+C. This is useful for:

1. Keeping an active town session open during development
2. Maintaining a persistent connection for debugging
3. Watching a town without spawning a new agent

Note: Towns automatically connect to central Redis when you run `tt init` or any command that needs it. This command is mainly for explicitly keeping the town connection open.

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Keep Town Running

```bash
tt start
```

Output:
```
🚀 Town connection open
^C
👋 Closing town connection...
```

### With Specific Town

```bash
tt start --town ~/git/my-project
```

## When to Use

Most operations don't require `tt start` because:
- `tt init` provisions the town and connects as needed
- `tt spawn` connects and stays alive
- `tt status` connects temporarily

Use `tt start` when you want to:
- Keep a town session open without spawning agents
- Debug connection issues
- Manually control the town lifecycle

## See Also

- [tt stop](./stop.md) — Stop the town
- [tt init](./init.md) — Initialize a new town
- [tt status](./status.md) — Check town status
