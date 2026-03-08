# tt send

Send a message to an agent.

## Synopsis

```bash
tt send <TO> <MESSAGE> [OPTIONS]
```

## Description

Sends a semantic message to an agent's inbox. The agent will receive it on their next inbox check.

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
# Developer finishes, notifies reviewer
tt send reviewer "Implementation complete. Please review src/auth.rs"

# Critical bug found - urgent interrupt
tt send developer --urgent "Critical: SQL injection in login. Fix immediately."
```

## How It Works

### Regular Messages
1. Goes to `tt:inbox:<id>` (Redis list)
2. Processed in order with other messages
3. Agent sees it when they check inbox
4. Semantic type is attached as `task`, `query`, `info`, or `ack`

### Urgent Messages
1. Goes to `tt:urgent:<id>` (separate priority queue)
2. Agent checks urgent queue FIRST at start of each round
3. Urgent messages injected into agent's prompt with 🚨 marker
4. Processed before regular inbox
5. Keeps its semantic type (`task`, `query`, `info`, or `ack`)

## See Also

- [tt inbox](./inbox.md) — Check agent's inbox
- [tt assign](./assign.md) — Assign tasks (more structured)
- [Coordination](../concepts/coordination.md)
