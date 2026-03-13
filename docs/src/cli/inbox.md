# tt inbox

Check agent inbox(es).

## Synopsis

```bash
tt inbox <AGENT>         # Check specific agent
tt inbox --all           # Show all agents' inboxes
```

## Description

Shows pending messages in agent inboxes.

`supervisor` and `conductor` are aliases for the same well-known mailbox, so `tt inbox supervisor` and `tt inbox conductor` inspect the conductor-facing queue even when no agent with that literal name exists.

When used with `--all`, displays a summary of pending messages for all agents, categorized by type:
- **[T]** Tasks requiring action
- **[Q]** Queries awaiting response
- **[I]** Informational messages (FYI)
- **[C]** Confirmations/acknowledgments

Messages are added by:
- `tt assign` — Creates a task and sends TaskAssign message
- `tt send` — Sends a custom message

## Arguments

| Argument | Description |
|----------|-------------|
| `[AGENT]` | Agent name to check (optional with `--all`) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--all` | `-a` | Show pending messages for all agents |
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Check Specific Agent

```bash
tt inbox backend
tt inbox supervisor
```

Output:
```
📬 Inbox for 'backend': 3 messages
   [T] 2 tasks requiring action
   [Q] 1 queries awaiting response
   [I] 0 informational
   [C] 0 confirmations

   [T] task 1234: Fix authentication bug in login endpoint
   [Q] question: Should this support OAuth fallback?
```

### View All Agents' Inboxes

```bash
tt inbox --all
```

Output:
```
📋 Pending Messages by Agent:

  backend (Working):
    [T] 2 tasks requiring action
    [Q] 1 queries awaiting response
    [I] 3 informational
    [C] 0 confirmations
    • Fix authentication bug in login endpoint
    • Update database schema for new fields

  reviewer (Idle):
    [T] 1 tasks requiring action
    [Q] 0 queries awaiting response
    [I] 0 informational
    [C] 2 confirmations
    • [T] Review PR #42: Add user validation

  supervisor/conductor (well-known mailbox):
    [T] 0 tasks requiring action
    [Q] 1 queries awaiting response
    [I] 2 informational
    [C] 0 confirmations
    • [Q] question: Product decision needed for rollout timing

Total: 4 actionable message(s)
```

## Comparison with tt status

- `tt status` shows agent states and total inbox counts
- `tt inbox --all` shows message breakdown and previews

## See Also

- [tt send](./send.md) — Send messages to agents
- [tt assign](./assign.md) — Assign tasks (sends TaskAssign message)
- [tt task](./task.md) — Manage individual tasks
- [Messages Concept](../concepts/messages.md)
