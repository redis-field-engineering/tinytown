# Tutorial: Mission Mode

Orchestrate multiple GitHub issues as a single autonomous mission.

## What We'll Build

A mission that implements a user authentication feature spanning three issues:
1. Issue #1: Design auth API
2. Issue #2: Implement auth endpoints
3. Issue #3: Write auth tests

The issues have natural dependencies: design → implement → test.

## Prerequisites

- A running Tinytown instance (`tt init`, Redis available)
- GitHub issues created with dependency markers
- Multiple agents spawned with appropriate roles

## Step 1: Create Issues with Dependencies

In your GitHub issues, use dependency markers:

**Issue #1: Design auth API**
```markdown
Design the authentication API schema.

- Define login/logout endpoints
- Document token format
- Specify error responses
```

**Issue #2: Implement auth endpoints**
```markdown
Implement the authentication endpoints.

Depends on #1.

- Implement login endpoint
- Implement logout endpoint
- Add token validation
```

**Issue #3: Write auth tests**
```markdown
Write comprehensive tests for auth API.

After #2.

- Unit tests for token validation
- Integration tests for login flow
- Error case coverage
```

## Step 2: Spawn Your Team

```bash
# Spawn agents with appropriate roles
tt spawn designer --cli claude
tt spawn backend --cli auggie
tt spawn tester --cli codex-mini
```

## Step 3: Start the Mission

```bash
tt mission start --issue 1 --issue 2 --issue 3
```

Output:
```
🚀 Mission started: abc123-def456-...
📋 Objectives: 3 issues
📦 Work items: 3
   ⏳ Issue #1: Design auth API
   ⏳ Issue #2: Implement auth endpoints
   ⏳ Issue #3: Write auth tests
```

## Step 4: Monitor Progress

```bash
# Run the persistent dispatcher loop
tt mission dispatch

# Check overall status
tt mission status

# Detailed work item view
tt mission status --work

# Watch PR/CI monitors
tt mission status --work --watch
```

## Step 5: Understand the Dispatcher

The mission dispatcher runs every 30 seconds and:

1. **Promotes work items**: Issue #1 starts immediately (no deps)
2. **Assigns to agents**: Designer gets Issue #1
3. **Monitors completion**: When #1 done, #2 becomes ready
4. **Watches PRs**: Creates watch items for CI/Bugbot/review status
5. **Enforces gates**: Reviewer approval before final completion

```
Round 1: Issue #1 → ready → assigned to designer
Round 5: Issue #1 done → Issue #2 ready → assigned to backend
Round 10: Issue #2 done → Issue #3 ready → assigned to tester
Round 15: All done → Mission completed
```

## Step 6: Handle Blocking

If CI fails or review is needed:

```bash
# Check why mission is blocked
tt mission status --watch

# Output shows:
# 🚧 Watch Items: 1
#    ⚠️  PR #42 CI check: failing (retrying in 180s)
```

The mission will:
- Auto-retry CI checks
- Create persisted fix tasks if CI or Bugbot comments fail
- Create reviewer tasks and wait for approval if `reviewer_required`

## Step 7: Stop and Resume

```bash
# Pause the mission (can resume later)
tt mission stop abc123

# Resume when ready
tt mission resume abc123
```

## Advanced: Custom Policy

```bash
# More parallelism
tt mission start -i 1 -i 2 -i 3 --max-parallel 4

# Skip reviewer (for drafts/experiments)
tt mission start -i 1 --no-reviewer
```

## Advanced: Mission Manifest

For complex projects, create `mission.toml`:

```toml
# Override issue handling
[[overrides]]
issue = 1
owner_role = "architect"
priority = 10

[[overrides]]
issue = 2
depends_on = [1]
owner_role = "backend"

[[overrides]]
issue = 3
skip = true  # Exclude from mission
```

Then reference it (feature coming soon).

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Mission stuck in Planning | Check if issues are accessible |
| Work item never ready | Verify dependency markers parsed |
| Agent not assigned | Spawn agent with matching role |
| CI watch failing | Check GitHub API permissions |

## Best Practices

1. **Use clear dependency markers**: `depends on #N` in issue body
2. **Keep issues focused**: One objective per issue
3. **Role-tag your agents**: Match agent roles to work types
4. **Monitor actively**: Use `--work` flag to see progress
5. **Set appropriate parallelism**: Don't overwhelm your agents

## Next Steps

- [Mission Mode Concept](../concepts/missions.md)
- [tt mission CLI Reference](../cli/mission.md)
- [Multi-Agent Coordination](./multi-agent.md)
