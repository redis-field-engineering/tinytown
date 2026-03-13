# tt backlog

Manage the global backlog of unassigned tasks.

## Synopsis

```bash
tt backlog <SUBCOMMAND> [OPTIONS]
```

## Description

Use backlog when work should exist in Tinytown but should not be assigned immediately.

Backlog tasks are stored in Redis, can be tagged, and can be claimed later by the right agent.

## Subcommands

### Add

```bash
tt backlog add "<TASK DESCRIPTION>" [--tags tag1,tag2]
```

Creates a new task and places it in the global backlog queue.

### List

```bash
tt backlog list
```

Shows all backlog task IDs with a short description and tags.

### Claim

```bash
tt backlog claim <TASK_ID> <AGENT>
```

Removes a task from backlog, assigns it to `<AGENT>`, and sends a semantic `TaskAssign` message to that agent.

### Assign All

```bash
tt backlog assign-all <AGENT>
```

Bulk-assigns every backlog task to one agent (useful for manual catch-up or handoff).

### Remove

```bash
tt backlog remove <TASK_ID>
```

Removes a task from the backlog without assigning it to any agent. Useful for cleaning up tasks that are no longer needed or were added by mistake.
The task record is deleted as part of the removal.

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Examples

### Park Work in Backlog

```bash
tt backlog add "Investigate flaky auth integration test" --tags test,auth,backend
tt backlog add "Document token refresh behavior" --tags docs,api
```

### Review and Claim by Role

```bash
# Backend agent role
tt backlog list
tt backlog claim 550e8400-e29b-41d4-a716-446655440000 backend

# Docs agent role
tt backlog claim 550e8400-e29b-41d4-a716-446655440111 docs
```

## Role-Based Claiming Pattern

When agents are idle, have them:

1. Run `tt backlog list`
2. Claim one task matching their role/tags
3. Skip backlog claiming if nothing clearly matches their role
4. Work claimed tasks to completion, then repeat

This keeps specialists busy without over-assigning work up front.

Tinytown's worker loop follows the same rule: idle agents should only auto-claim backlog work that matches their role hint. A `reviewer` should not pick up implementation-tagged backlog by default, and implementation agents should leave review/security backlog to the appropriate specialist unless a human explicitly redirects the work.

## See Also

- [tt assign](./assign.md) — Directly assign new work
- [tt conductor](./conductor.md) — Orchestrate agents interactively
- [Tasks Concept](../concepts/tasks.md)
