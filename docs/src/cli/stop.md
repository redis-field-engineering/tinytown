# tt stop

Request all agents in the current town to stop gracefully.

## Synopsis

```bash
tt stop [OPTIONS]
```

## Description

`tt stop` requests all agents in the current town to stop gracefully.
It does not shut down the shared central Redis instance, because that instance may be serving other towns.

Note: To fully reset a town, use `tt reset` instead.

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Stop the Town

```bash
tt stop
```

Output:
```
🛑 Requested graceful stop for 3 agent(s) in town 'my-project'
   Agents will stop at the start of their next round.
   Central Redis remains available to other towns.
```

## Related Operations

| Task | Command |
|------|---------|
| Stop one agent | `tt kill <agent>` |
| Reset all state | `tt reset` |
| Inspect remaining work | `tt inbox --all` |

## Shared Redis Lifecycle

Central Redis in Tinytown is shared across towns:
- Starts when a town first needs it
- Remains available after `tt stop`
- Should only be shut down through explicit Redis administration, not normal town cleanup

For persistent deployments, consider running Redis independently.

## See Also

- [tt start](./start.md) — Keep town alive
- [tt reset](./reset.md) — Full state reset
- [tt kill](./kill.md) — Stop specific agents
