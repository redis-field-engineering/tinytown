# tt spawn

Create and start a new agent.

## Synopsis

```bash
tt spawn <NAME> [OPTIONS]
```

## Description

Spawns a new worker agent in the town. **This actually starts an AI process!**

The agent:
1. Registers in Redis with state `Starting`
2. Starts a background process (or foreground with `--foreground`)
3. Runs in a loop, checking inbox for tasks
4. Executes the AI model (claude, auggie, etc.) for each task
5. Stops after `--max-rounds` iterations

## Arguments

| Argument | Description |
|----------|-------------|
| `<NAME>` | Human-readable agent name (e.g., `worker-1`, `backend`, `reviewer`) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--model <MODEL>` | `-m` | AI CLI to use (default: from `tinytown.json`) |
| `--max-rounds <N>` | | Maximum iterations before stopping (default: 10) |
| `--foreground` | | Run in foreground instead of background |
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Setting the Default CLI

Edit `tinytown.json` to change which AI CLI is used by default:

```json
{
  "name": "my-town",
  "default_model": "auggie"
}
```

Then all `tt spawn` commands use that CLI:

```bash
tt spawn backend              # Uses auggie (from config)
tt spawn frontend --model codex   # Override to use codex
```

## Built-in Agent CLIs

| CLI | Command (non-interactive) |
|-----|---------------------------|
| `claude` | `claude --print --dangerously-skip-permissions` |
| `auggie` | `auggie --print` |
| `codex` | `codex exec --dangerously-bypass-approvals-and-sandbox` |
| `aider` | `aider --yes --no-auto-commits --message` |
| `gemini` | `gemini` |
| `copilot` | `gh copilot` |
| `cursor` | `cursor` |

These are the CLI tools that run AI coding agents, not the underlying models.

## Examples

### Spawn in Background (Default)

```bash
tt spawn worker-1
# Agent runs in background, logs to logs/worker-1.log
```

### Spawn in Foreground (See Output)

```bash
tt spawn worker-1 --foreground
# Agent runs in this terminal, you see all output
```

### Limit Iterations

```bash
tt spawn worker-1 --max-rounds 5
# Agent stops after 5 rounds (default is 10)
```

### Spawn Multiple Agents (Parallel!)

```bash
tt spawn backend &
tt spawn frontend &
tt spawn tester &
# All three run in parallel
```

## Output

```
🤖 Spawned agent 'backend' using model 'auggie'
   ID: 550e8400-e29b-41d4-a716-446655440000
🔄 Starting agent loop in background (max 10 rounds)...
   Logs: ./logs/backend.log
   Agent running in background. Check status with 'tt status'
```

## What Happens

1. **Agent registered** in Redis (`tt:agent:<id>`)
2. **Background process** started running `tt agent-loop`
3. **Agent loop**:
   - Checks inbox for messages
   - If messages: builds prompt, runs AI model
   - Model output logged to `logs/<name>_round_<n>.log`
   - Repeats until `--max-rounds` reached
4. **Agent stops** with state `Stopped`

## Agent Naming

Choose descriptive names:

| Good Names | Why |
|------------|-----|
| `backend` | Describes the work area |
| `worker-1` | Simple numbered workers |
| `reviewer` | Describes the role |
| `alice` | Personality names work too |

| Avoid | Why |
|-------|-----|
| `agent` | Too generic |
| `a` | Not descriptive |
| Spaces | Use hyphens instead |

## Agent State After Spawn

New agents start in `Starting` state, then transition to `Idle`:

```
Starting → Idle (ready for work)
```

Check state with:
```bash
tt list
```

## Errors

### Town Not Initialized

```
Error: Town not initialized at . Run 'tt init' first.
```

**Solution:** Run `tt init` or specify `--town` path.

### Agent Already Exists

Agents are tracked by name. Spawning the same name creates a new agent with a new ID.

## See Also

- [tt init](./init.md) — Initialize a town
- [tt assign](./assign.md) — Assign tasks to agents
- [tt list](./list.md) — List all agents
- [Agents Concept](../concepts/agents.md)

