# tt assign

Assign a task to an agent.

## Synopsis

```bash
tt assign <AGENT> <TASK>
```

## Description

Creates a new task record and sends it to the specified agent's inbox as a semantic `task` message.

`tt assign` sends a semantic **task** message and is the right command for actionable work. Use [`tt send`](./send.md) for non-task communication such as queries, informational updates, or confirmations.

## Arguments

| Argument | Description |
|----------|-------------|
| `<AGENT>` | Agent name to assign to |
| `<TASK>` | Task description (quoted string) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Basic Assignment

```bash
tt assign worker-1 "Implement the login API"
```

### Multi-line Task

```bash
tt assign backend "Create a REST API endpoint POST /users that:
- Accepts {email, password, name}
- Validates email format
- Hashes password with bcrypt
- Returns {id, email, name, created_at}"
```

### Assign to Multiple Agents

```bash
tt assign frontend "Build the login form"
tt assign backend "Build the auth API"
tt assign tester "Write integration tests"
```

## Output

```
📋 Assigned task 550e8400-e29b-41d4-a716-446655440000 to agent 'worker-1'
```

## What Happens

1. **Task created** with state `Pending`
2. **Task stored** in Redis at `tt:task:<id>`
3. **Message sent** to agent's inbox at `tt:inbox:<agent-id>`
4. **Task state** updated to `Assigned`

## Task Lifecycle After Assignment

```
Pending → Assigned → Running → Completed
                           └─→ Failed
                           └─→ Cancelled
```

## Viewing Assigned Tasks

Check what's in an agent's inbox:

```bash
# Using redis-cli
redis-cli -s ./redis.sock LLEN tt:inbox:<agent-id>
redis-cli -s ./redis.sock LRANGE tt:inbox:<agent-id> 0 -1
```

Or check status:
```bash
tt status
```

## Errors

### Agent Not Found

```
Error: Agent not found: nonexistent
```

**Solution:** Spawn the agent first with `tt spawn`.

### Town Not Initialized

```
Error: Town not initialized at . Run 'tt init' first.
```

## Task Description Tips

Good task descriptions:
- Be specific about what to build
- Include acceptance criteria
- Mention relevant files/paths
- Specify output format if needed

```bash
# ✅ Good
tt assign backend "Create POST /api/users endpoint in src/routes/users.rs. 
Accept JSON body {email, password}. Return 201 with {id, email}."

# ❌ Too vague
tt assign backend "Build API"
```

Use `tt assign` when the recipient should do concrete work, not just acknowledge or discuss.

## See Also

- [tt spawn](./spawn.md) — Create agents
- [tt status](./status.md) — Check task status
- [Tasks Concept](../concepts/tasks.md)
