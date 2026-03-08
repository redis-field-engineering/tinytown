# Tutorial: Error Handling & Recovery

Things go wrong. Agents crash, tasks fail, Redis restarts. Here's how to handle it.

## Checking Agent Health

Use CLI commands to monitor agent state:

```bash
# Check all agents
tt status

# List agents and their states
tt list

# Check pending tasks
tt tasks
```

Agent states to watch for:
- **Idle** — Ready for work ✓
- **Working** — Busy but healthy ✓
- **Error** — Something went wrong ✗
- **Stopped** — Agent terminated ✗

## Checking Task State

View task status with the CLI:

```bash
# See all pending tasks by agent
tt tasks

# Check a specific agent's inbox
tt inbox <agent-name>
```

## Respawning Failed Agents

If an agent dies, spawn a new one:

```bash
# Check if agent exists
tt list

# If stopped or missing, respawn it
tt spawn worker-1 --model claude

# Or prune stale agents first
tt prune
tt spawn worker-1 --model claude
```

## Graceful Shutdown

To stop agents gracefully:

```bash
# Stop a specific agent
tt kill worker-1

# Stop the entire town (saves state first)
tt save
tt stop
```

## Recovery Checklist

When things go wrong:

1. **Check Redis** — Is `redis-server` running?
   ```bash
   redis-cli -s ./redis.sock PING
   ```

2. **Check agent state** — What state is it in?
   ```bash
   tt list
   tt status
   ```

3. **Check inbox** — Are messages stuck?
   ```bash
   tt inbox <agent-name>
   ```

4. **Check tasks** — What tasks are pending?
   ```bash
   tt tasks
   ```

5. **Check logs** — Look in `logs/` directory

## Comparison with Gastown Recovery

| Feature | Tinytown | Gastown |
|---------|----------|---------|
| Auto-recovery | Manual (you write it) | Witness patrol |
| State persistence | Redis | Git-backed beads |
| Crash detection | Check agent state | Boot/Deacon monitors |
| Work resumption | Reassign tasks | Hook-based (automatic) |

Tinytown puts you in control. Gastown automates more but is more complex. Choose based on your reliability requirements.

