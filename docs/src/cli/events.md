# tt events

Tail the Redis Stream event log. Shows raw events from the town-wide event stream.

## Usage

```bash
tt events [--count <N>] [--agent <NAME>] [--mission <ID>] [--follow]
```

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--count <N>` | `-c` | Number of recent events to show (default: 20) |
| `--agent <NAME>` | | Filter by agent name |
| `--mission <ID>` | | Filter by mission ID |
| `--follow` | `-f` | Continuously poll for new events |

## Examples

```bash
# Show last 20 events
tt events

# Follow events in real-time
tt events --follow

# Show events for a specific agent
tt events --agent backend

# Show events for a specific mission
tt events --mission abc12345-...
```

## See Also

- [tt history](./history.md) — higher-level communication history with Slack-style formatting
- [tt status](./status.md) — agent and task status overview
