# tt history

Show recent agent communication history from the event stream.

## Usage

```bash
tt history [-n <limit>] [--agent <name>]
```

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--limit <N>` | `-n` | Maximum number of events to show (default: 30) |
| `--agent <NAME>` | `-a` | Filter to events involving a specific agent |

## Output

Events are displayed chronologically with timestamps, icons, and agent labels in `Nickname [role]` format:

```
📜 Recent History (5 events):

  14:23:01 🐣 *Frankie [backend]* Agent spawned
  14:23:02 📌 *Frankie [backend]* Task assigned: implement auth API
  14:23:15 🔄 *Frankie [backend]* State changed: Idle → Working
  14:25:30 ✅ *Frankie [backend]* Task completed: implement auth API
  14:25:31 👀 *Martha [reviewer]* Review handoff from backend
```

## Event Icons

| Icon | Event Type |
|------|-----------|
| 🐣 | Agent spawned |
| 🏁 | Agent stopped/completed |
| 🔄 | State changed |
| 📌 | Task assigned |
| ✅ | Task/review completed |
| ❌ | Task failed |
| 🤝 | Task delegated |
| 👀 | Reviewer handoff |
| 🚨 | Conductor escalation |
| ⏸️ | Agent interrupted |
| ▶️ | Agent resumed |
| 🎯 | Mission event |

## Examples

```bash
# Show last 30 events
tt history

# Show last 50 events
tt history -n 50

# Show only events for a specific agent
tt history --agent backend
```
