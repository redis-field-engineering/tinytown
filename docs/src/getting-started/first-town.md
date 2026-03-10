# Your First Town

Let's build a real workflow: coordinating agents to implement and review a feature.

## The Scenario

You want to:
1. Have one agent implement a feature
2. Have another agent write tests
3. Have a third agent review both

## Project Setup

```bash
# Create and enter your project
mkdir feature-builder && cd feature-builder

# Initialize git (optional but recommended)
git init

# Initialize tinytown
tt init --name feature-builder
```

## Understanding the Config

Open `tinytown.toml`:

```toml
name = "feature-builder"
default_cli = "claude"
max_agents = 10

[redis]
use_socket = true
socket_path = "redis.sock"
```

Key settings:
- **`use_socket = true`** — Uses Unix socket for ~10x faster communication than TCP
- **`default_cli`** — Agent CLI when `--model` isn't specified
- **`max_agents`** — Prevents accidentally spawning too many

## Create Your Team

```bash
# The implementer
tt spawn dev --model claude

# The tester  
tt spawn tester --model auggie

# The reviewer
tt spawn reviewer --model codex
```

Check your team:
```bash
tt list
```

```
Agents:
  dev (550e8400-...) - Starting
  tester (6ba7b810-...) - Starting
  reviewer (6ba7b811-...) - Starting
```

## Assign the Work

```bash
# Implementation task
tt assign dev "Create a REST API endpoint POST /users that:
- Accepts {email, password, name}
- Validates email format
- Hashes password with bcrypt
- Returns {id, email, name, created_at}"

# Testing task
tt assign tester "Write integration tests for POST /users:
- Test successful creation
- Test duplicate email rejection
- Test invalid email format
- Test missing required fields"

# Review task
tt assign reviewer "Review the implementation and tests when ready:
- Check for security issues
- Verify error handling
- Ensure tests cover edge cases"
```

## Monitor Progress

```bash
# See overall status
tt status

# Watch for changes (re-run periodically)
watch -n 5 tt status
```

## What Happens Behind the Scenes

1. **Task Creation**: Each `tt assign` creates a `Task` with a unique ID
2. **Message Sending**: A `Message` of type `TaskAssign` is sent to the agent's inbox
3. **Redis Queue**: Messages are stored in town-isolated Redis lists (`tt:<town>:inbox:<agent-id>`)
4. **Agent Pickup**: Agents receive messages via `BLPOP` (blocking pop)
5. **State Tracking**: Agent and task states are stored in Redis

## Connecting Real Agents

Tinytown creates the infrastructure, but you need to connect actual AI agents. The spawn command prepares the configuration; you then run the agent:

```bash
# Example: Run Claude CLI pointing at your town
cd agents/dev
claude --print  # Uses the model command from config
```

Or with Augment:
```bash
cd agents/tester
augment  # Uses the model command from config
```

## Cleanup

When you're done:

```bash
# Stop the town's agents
tt stop

# Or just Ctrl+C if running `tt start`
```

## Next Steps

- **[Core Concepts](../concepts/overview.md)** — Deep dive into Towns, Agents, Tasks
- **[Multi-Agent Tutorial](../tutorials/multi-agent.md)** — More complex coordination patterns
