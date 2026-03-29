# tt recover

Detect and clean up orphaned agents.

## Synopsis

```bash
tt recover [OPTIONS]
```

## Description

Scans for agents that appear to be running (Working, Starting, Idle, or Draining state) but whose processes have actually crashed or been killed. Marks these orphaned agents as Stopped so they can be pruned or restarted.

An agent is considered orphaned if:
1. It's in Working, Starting, Idle, or Draining state
2. Its log file hasn't been modified in 2+ minutes, OR
3. Its last heartbeat was 2+ minutes ago

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Scan for Orphaned Agents

```bash
tt recover
```

Output when orphans found:
```
🔍 Scanning for orphaned agents...
   🔄 Recovered 'worker-1' (Working) - last heartbeat 5m ago
   🔄 Recovered 'worker-2' (Working) - last heartbeat 3m ago

✨ Recovered 2 orphaned agent(s) (4 total checked)
   Run 'tt prune' to remove them from Redis
```

Output when no orphans:
```
🔍 Scanning for orphaned agents...

✨ No orphaned agents found (3 agents checked)
```

## Common Workflow

After a crash or system restart:

```bash
# 1. Recover orphaned agents (marks them stopped)
tt recover

# 2. Optional: Reclaim tasks from dead agents
tt reclaim --to-backlog

# 3. Clean up stopped agents
tt prune

# 4. Restart needed agents
tt restart worker-1
```

## When to Use

- After system restart or crash
- When agents appear "stuck" in Working, Idle, or Draining state
- Before reclaiming tasks from dead agents

## See Also

- [tt prune](./prune.md) — Remove stopped agents
- [tt reclaim](./reclaim.md) — Recover orphaned tasks
- [tt restart](./restart.md) — Restart stopped agents
- [Error Handling & Recovery](../tutorials/recovery.md) — Recovery tutorial
