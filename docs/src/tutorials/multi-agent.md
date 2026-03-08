# Tutorial: Multi-Agent Coordination

Let's coordinate multiple agents working together on a feature.

## The Scenario

We'll build a system where:
1. **Architect** designs the API
2. **Developer** implements it
3. **Tester** writes tests
4. **Reviewer** reviews everything

## Setup

```bash
mkdir multi-agent-demo && cd multi-agent-demo
tt init --name multi-demo
```

## Spawning the Team

```bash
tt spawn architect --model claude
tt spawn developer --model auggie
tt spawn tester --model codex
tt spawn reviewer --model claude
```

Check your team:
```bash
tt list
```

## Sequential Pipeline with tasks.toml

Define your workflow in `tasks.toml`:

```toml
[meta]
description = "Auth API Pipeline"

[[tasks]]
id = "design"
description = "Design a REST API for user authentication with JWT tokens"
agent = "architect"
status = "pending"

[[tasks]]
id = "implement"
description = "Implement the auth API from the architect's design"
agent = "developer"
status = "pending"
parent = "design"

[[tasks]]
id = "test"
description = "Write comprehensive tests for the auth API"
agent = "tester"
status = "pending"
parent = "implement"
```

Then sync to Redis and let the conductor orchestrate:
```bash
tt sync push
tt conductor
```

## Parallel Execution

When tasks are independent, assign them all at once:

```bash
# Spawn agents
tt spawn frontend --model claude
tt spawn backend --model auggie
tt spawn docs --model codex

# Assign tasks (they run in parallel)
tt assign frontend "Build the login UI"
tt assign backend "Build the auth API"
tt assign docs "Write API documentation"

# Monitor progress
tt status
```

## Fan-Out / Fan-In

Use the conductor for complex workflows:

```bash
# Spawn workers
tt spawn worker-1 --model claude
tt spawn worker-2 --model claude
tt spawn worker-3 --model claude
tt spawn reviewer --model claude

# Assign work to all workers
tt assign worker-1 "Implement module A"
tt assign worker-2 "Implement module B"
tt assign worker-3 "Implement module C"

# Monitor until all complete
tt status

# Then aggregate with reviewer
tt assign reviewer "Review modules A, B, and C for consistency"
```

## Agent-to-Agent Communication

Agents can send messages to each other:

```bash
# Send a message to another agent
tt send reviewer "Auth API implementation complete. Ready for review."

# Send an urgent message
tt send reviewer --urgent "Critical bug found in module A!"
```

## Comparison with Gastown

| Pattern | Tinytown | Gastown |
|---------|----------|---------|
| Sequential | `wait_for_idle()` loop | Convoy + Beads events |
| Parallel | `tokio::join!` | Mayor distributes |
| Fan-out/in | Manual coordination | Convoy tracking |
| Messaging | Direct `channel.send()` | Mail protocol |

Tinytown is more explicit—you write the coordination logic. Gastown abstracts it with Convoys and the Mayor. Choose based on your needs.

## Next Steps

- [Task Pipelines](./pipelines.md) — Build complex workflows
- [Error Handling](./recovery.md) — Handle failures gracefully

