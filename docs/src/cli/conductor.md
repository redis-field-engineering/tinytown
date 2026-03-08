# tt conductor

Start the conductor - an AI agent that orchestrates your town.

## Synopsis

```bash
tt conductor [OPTIONS]
```

## Description

The **conductor** is an AI agent (using your default model) that coordinates your Tinytown! 🚂

Like the train conductor guiding the miniature train through Tiny Town, Colorado, it:
- Understands what you want to build
- Breaks down work into tasks
- Spawns appropriate agents
- Assigns tasks to agents
- Monitors progress
- Helps resolve blockers

The conductor knows how to use the `tt` CLI to orchestrate your project.

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--town <PATH>` | `-t` | Town directory (default: `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## How It Works

1. **Context Injection**: The conductor receives context about:
   - Current town state (agents, tasks, pending messages)
   - Whether this is a **fresh start** or **resuming** an existing session
   - Available `tt` commands and suggested team roles
   - Its role as orchestrator

2. **AI Model Launch**: Your default model (claude, auggie, etc.) starts with this context

3. **Natural Conversation**: You describe what you want, the AI orchestrates

## Fresh Start vs Resuming

The conductor behaves differently based on existing state:

### Fresh Start (No Agents)

When starting with a new town, the conductor:
1. Asks what you're trying to build
2. Offers to analyze the project (README, design docs, codebase)
3. Suggests appropriate team roles for your project
4. Helps break down your idea into tasks and agent assignments

### Resuming (Existing Agents)

When agents already exist, the conductor:
1. Shows current agent status
2. Checks progress with `tt status --deep`
3. Continues coordinating from where you left off

## Suggested Team Roles

The conductor knows about common team roles and when to suggest them:

| Role | When to Suggest |
|------|-----------------|
| `backend` | API development, server-side logic |
| `frontend` | UI/UX implementation |
| `tester` | Writing and running tests |
| `reviewer` | **Always include** - quality gate |
| `devops` | CI/CD, deployment, infrastructure |
| `security` | Security review, vulnerability analysis |
| `docs` | Documentation, API specs, README |
| `architect` | System design, code structure |

The conductor will analyze your project and suggest roles that make sense.

## Example Session

```
$ tt conductor
🚂 Starting conductor with claude model...
   Context: ./.conductor_context.md

   Running: claude --print

# Tinytown Conductor

You are the **conductor** of Tinytown "my-project"...
[context displayed]

---

User: Build a user authentication system with login, signup, and password reset.

Conductor: I'll set up a team for this. Let me spawn some agents and create a plan.

[Conductor runs: tt spawn architect]
[Conductor runs: tt spawn backend]
[Conductor runs: tt spawn tester]

I've created three agents. Now let me assign the initial work:

[Conductor runs: tt assign architect "Design REST API for user authentication..."]

The architect is working on the API design. Once complete, I'll assign implementation to the backend agent and tests to the tester.

[Conductor runs: tt status]

Current status:
- architect (Working) - designing the API
- backend (Idle) - waiting for design
- tester (Idle) - will write tests after implementation
```

## The Reviewer Pattern

The conductor always spawns a **reviewer** agent. This creates a simple completion protocol:

```
Worker completes task
       ↓
Conductor assigns review task to reviewer
       ↓
Reviewer checks work → approves or requests changes
       ↓
Conductor marks complete or assigns fixes
```

This keeps it simple:
- **Workers** do the work
- **Reviewer** decides if it's done
- **Conductor** coordinates everything

## The Conductor's Context

The conductor receives a markdown context file that includes:

```markdown
# Tinytown Conductor

You are the **conductor** of Tinytown "my-project"...

## Current Town State
- Agents: backend (Working), reviewer (Idle)
- Tasks pending: 1

## Your Capabilities
- tt spawn <name> - Create agents
- tt assign <agent> "task" - Assign work
- tt status - Check progress

## The Reviewer Pattern
Always spawn a reviewer. They decide when work is done.

## Your Role
1. Break down user requests into tasks
2. Spawn workers + reviewer
3. Assign work, then assign review
4. Coordinate until reviewer approves
5. Save state with `tt sync pull`, suggest git commit
```

## Comparison with `gt mayor attach`

| Gastown | Tinytown |
|---------|----------|
| `gt mayor attach` | `tt conductor` |
| Natural language | Natural language ✓ |
| Mayor is complex orchestrator | Conductor is simple AI + CLI |
| Hard to understand what Mayor does | You can read the context |
| Recovery daemons, convoys, beads | Just `tt` commands |

The conductor is **transparent**: you can see exactly what context it has and what commands it runs.

## See Also

- [tt status](./status.md) — Check town status
- [tt spawn](./spawn.md) — Spawn agents manually
- [tt plan](./plan.md) — Plan tasks in a file

