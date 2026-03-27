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
4. Executes the selected AI CLI (claude, auggie, etc.) for each task
5. Stops after `--max-rounds` iterations

## Arguments

| Argument | Description |
|----------|-------------|
| `<NAME>` | Human-readable agent name (e.g., `worker-1`, `backend`, `reviewer`) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--cli <CLI>` | `-m` | AI CLI to use (default: from `tinytown.toml`) |
| `--max-rounds <N>` | | Maximum iterations before stopping (default: 10) |
| `--foreground` | | Run in foreground instead of background |
| `--role <ROLE>` | | Explicit role ID for routing (e.g., `worker`, `reviewer`, `researcher`) |
| `--nickname <NAME>` | | Human-facing display name (separate from canonical name) |
| `--parent <AGENT>` | | Parent agent name or ID (for delegated subtasks) |
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

### Role-Based Routing

When you set `--role`, the mission scheduler uses it for matching work items to agents instead of inferring roles from the agent name. This is more reliable:

```bash
# Without --role: scheduler guesses from name "backend" → probably a worker
tt spawn backend

# With --role: scheduler knows this is explicitly a reviewer
tt spawn backend --role reviewer
```

Built-in roles: `worker`, `reviewer`, `researcher`, `architect`, `tester`, `devops`.

## Setting the Default CLI

Edit `tinytown.toml` to change which AI CLI is used by default:

```toml
name = "my-town"
default_cli = "auggie"
```

Then all `tt spawn` commands use that CLI:

```bash
tt spawn backend              # Uses auggie (from config)
tt spawn frontend --cli codex-mini   # Override to use codex-mini
```

## Built-in Agent CLIs

| CLI | Command (non-interactive) |
|-----|---------------------------|
| `claude` | `claude --print --dangerously-skip-permissions` |
| `auggie` | `auggie --print` |
| `codex` | `codex exec --dangerously-bypass-approvals-and-sandbox` |
| `codex-mini` | `codex exec --dangerously-bypass-approvals-and-sandbox -m gpt-5.4-mini -c model_reasoning_effort="medium"` |
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

### Spawn with Role and Nickname

```bash
tt spawn backend --role worker --nickname "API Developer"
tt spawn qa --role reviewer --nickname "Quality Gate"
```

### Spawn with Parent (Delegated Subtasks)

```bash
tt spawn subtask-1 --role worker --parent backend
# Creates a child agent linked to 'backend' in the agent hierarchy
```

### Spawn Multiple Agents (Parallel!)

```bash
tt spawn backend --role worker &
tt spawn frontend --role worker &
tt spawn tester --role tester &
tt spawn reviewer --role reviewer &
# All four run in parallel with explicit roles
```

## Output

```
🤖 Spawned agent 'backend' using CLI 'auggie'
   ID: 550e8400-e29b-41d4-a716-446655440000
🔄 Starting agent loop in background (max 10 rounds)...
   Logs: ./logs/backend.log
   Agent running in background. Check status with 'tt status'
```

## What Happens

1. **Agent registered** in Redis (`tt:<town>:agent:<id>`)
2. **Background process** started running `tt agent-loop`
3. **Agent loop**:
   - Checks inbox for messages
   - If messages: builds prompt, runs the selected CLI
   - CLI output logged to `logs/<name>_round_<n>.log`
   - Repeats until `--max-rounds` reached
4. **Agent stops** with state `Stopped`

## Agent Naming

Choose descriptive names. With `--role`, names no longer need to describe the role:

| Good Names | Why |
|------------|-----|
| `backend` | Describes the work area |
| `worker-1` | Simple numbered workers |
| `alice` | Personality names work — use `--role` for routing |

| Avoid | Why |
|-------|-----|
| `agent` | Too generic |
| `a` | Not descriptive |
| Spaces | Use hyphens instead |

> **Tip:** Use `--nickname` for human-friendly display names (e.g., `--nickname "Quality Gate"`) while keeping canonical names short for CLI usage.

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
