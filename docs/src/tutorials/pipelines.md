# Tutorial: Task Pipelines

Build structured workflows with task dependencies and hierarchies.

## What We'll Build

A code review pipeline:
1. Developer writes code
2. Linter checks style
3. Tester writes tests
4. Reviewer approves
5. Merger deploys

## Pipeline with tasks.toml

Define your pipeline in `tasks.toml`:

```toml
[meta]
description = "User Profile Feature Pipeline"

# Epic (parent task)
[[tasks]]
id = "profile-epic"
description = "Implement user profile feature"
status = "pending"
tags = ["epic", "q1-2024"]

# Subtasks under the epic
[[tasks]]
id = "design"
description = "Design profile API schema"
agent = "architect"
status = "pending"
parent = "profile-epic"
tags = ["design"]

[[tasks]]
id = "implement"
description = "Implement profile endpoints"
agent = "developer"
status = "pending"
parent = "profile-epic"
tags = ["backend"]

[[tasks]]
id = "test"
description = "Write profile API tests"
agent = "tester"
status = "pending"
parent = "profile-epic"
tags = ["testing"]

[[tasks]]
id = "review"
description = "Review profile implementation"
agent = "reviewer"
status = "pending"
parent = "profile-epic"
tags = ["review"]
```

Run the pipeline:
```bash
# Initialize the plan
tt plan --init

# Spawn the team
tt spawn architect --model claude
tt spawn developer --model auggie
tt spawn tester --model codex
tt spawn reviewer --model claude

# Push tasks to Redis
tt sync push

# Start the conductor to orchestrate
tt conductor
```

## Sequential Pipeline via CLI

For simple sequential workflows:

```bash
# Stage 1: Design
tt assign architect "Design the feature architecture"
# Wait for completion, then...

# Stage 2: Implement
tt assign developer "Implement the feature"
# Wait for completion, then...

# Stage 3: Test
tt assign tester "Write tests for the feature"
# Wait for completion, then...

# Stage 4: Review
tt assign reviewer "Review the implementation"
```

Use `tt status` to monitor progress between stages.

## Multi-Stage Pipeline Example

A complete `tasks.toml` for a CI/CD-like pipeline:

```toml
[meta]
description = "Code Review Pipeline"
default_agent = "developer"

[[tasks]]
id = "lint"
description = "Run linting on src/"
agent = "linter"
status = "pending"

[[tasks]]
id = "build"
description = "Build the project"
agent = "builder"
status = "pending"
parent = "lint"

[[tasks]]
id = "test"
description = "Run test suite"
agent = "tester"
status = "pending"
parent = "build"

[[tasks]]
id = "review"
description = "Code review"
agent = "reviewer"
status = "pending"
parent = "test"

[[tasks]]
id = "deploy"
description = "Deploy to staging"
agent = "deployer"
status = "pending"
parent = "review"
```

## Best Practices

1. **Use parent tasks** for grouping related work
2. **Tag tasks** for easy filtering and reporting
3. **Keep stages small** — easier to retry and debug
4. **Log stage transitions** — helps troubleshooting
5. **Handle failures gracefully** — don't crash the whole pipeline

## Next Steps

- [Error Handling & Recovery](./recovery.md)
- [Coming from Gastown: Convoy Mapping](../gastown/concepts.md)

