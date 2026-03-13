# tt send

Send a message to an agent.

## Synopsis

```bash
tt send <TO> <MESSAGE> [OPTIONS]
```

## Description

Sends a semantic message to an agent's inbox. The agent will receive it on their next inbox check.

`conductor` and `supervisor` are interchangeable names for the same well-known conductor mailbox, so either target works when agents need to report back to the human orchestrator.

Message semantics:
- Default (no semantic flag): Task-style/actionable message
- `--query`: Question that expects a response or decision
- `--info`: Informational update (context only)
- `--ack`: Confirmation/receipt message

**With `--urgent`**: Message goes to priority inbox, processed before regular messages!

Use this for:
- Agent-to-agent communication
- Conductor instructions
- Custom coordination
- **Urgent**: Interrupt agents with priority messages

## Arguments

| Argument | Description |
|----------|-------------|
| `<TO>` | Target agent name |
| `<MESSAGE>` | Message content |

## Options

| Option | Description |
|--------|-------------|
| `--query` | Mark message as a query (`query` semantic type) |
| `--info` | Mark message as informational (`info` semantic type) |
| `--ack` | Mark message as confirmation (`ack` semantic type) |
| `--urgent` | Send as urgent (processed first at start of next round) |

## Examples

### Send a Regular Message

```bash
tt send backend "The API spec is ready in docs/api.md"
```

Output:
```
📤 Sent task message to 'backend'
```

### Send a Query

```bash
tt send backend --query "Can you take auth token refresh next?"
```

### Send Informational Context

```bash
tt send reviewer --info "CI is green on commit a1b2c3d"
```

### Send a Confirmation

```bash
tt send conductor --ack "Received. I'll start after current task."
```

### Report Back to the Conductor

```bash
# Progress or completion notice
tt send supervisor --info "Implementation complete; ready for review"

# Blocked, needs a human decision
tt send conductor --query "Need a decision on OAuth scope naming"

# Simple receipt only
tt send supervisor --ack "Received. I will start after current task."
```

Recommended pattern:
- Use `--info` for progress updates, completion notices, or FYI visibility
- Use `--query` when blocked or when human judgment is needed
- Use `--ack` only for receipt/confirmation
- If the work corresponds to a real Tinytown task, still run `tt task complete <task_id> --result "summary"` when it is actually done

### Send an URGENT Message

```bash
tt send backend --urgent "STOP! Security vulnerability found. Do not merge."
```

Output:
```
🚨 Sent URGENT task message to 'backend'
```

The agent will see this at the start of their next round, before processing regular inbox.

### Coordination Between Agents

```bash
# Developer finishes, notifies reviewer directly
tt send reviewer "Implementation complete. Please review src/auth.rs"

# Reviewer sends concrete fixes straight back to the owner
tt send backend --query "Review found weak password hashing in src/auth.rs. Switch to bcrypt and reply when ready."

# Keep conductor informed without blocking the handoff
tt send supervisor --info "Reviewer asked backend to fix password hashing before approval."

# Critical bug found - urgent interrupt
tt send developer --urgent "Critical: SQL injection in login. Fix immediately."
```

## How It Works

### Regular Messages
1. Goes to `tt:<town>:inbox:<id>` (Redis list)
2. Processed in order with other messages
3. Agent sees it when they check inbox
4. Semantic type is attached as `task`, `query`, `info`, or `ack`

### Urgent Messages
1. Goes to `tt:<town>:urgent:<id>` (separate priority queue)
2. Agent checks urgent queue FIRST at start of each round
3. Urgent messages injected into agent's prompt with 🚨 marker
4. Processed before regular inbox
5. Keeps its semantic type (`task`, `query`, `info`, or `ack`)

## See Also

- [tt inbox](./inbox.md) — Check agent's inbox
- [tt assign](./assign.md) — Assign tasks (more structured)
- [Coordination](../concepts/coordination.md)
