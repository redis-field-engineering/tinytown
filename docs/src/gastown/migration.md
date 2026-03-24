# Migration Guide: From Gastown to Tinytown

Tried Gastown and found it overwhelming? You're not alone. Here's how to get the same results with Tinytown.

## Why You're Here

Gastown is powerful but complex:
- 50+ concepts to understand
- Multiple agent types (Mayor, Deacon, Witness, Polecats, etc.)
- Two-level database architecture (Town beads + Rig beads)
- Daemon processes, patrols, and recovery mechanisms
- Hours to set up, days to understand

Tinytown gives you **90% of the value with 10% of the complexity**.

## Quick Comparison

| What you wanted | Gastown way | Tinytown way |
|-----------------|-------------|--------------|
| Start orchestrating | 10+ commands | `tt init` |
| Create an agent | Complex Polecat setup | `tt spawn worker` |
| Assign work | `gt sling` + convoys | `tt assign worker "task"` |
| Check status | `gt convoy list`, `gt feed` | `tt status` |
| Understand it | Read 300K lines | Read 1,400 lines |

## Concept Mapping

### Gastown → Tinytown

| Gastown Concept | Tinytown Equivalent |
|-----------------|---------------------|
| Town | Town ✓ (same name!) |
| Mayor | You (or your code) |
| Polecat | Agent |
| Beads | Tasks (simpler) |
| Convoy | Task groups (manual) |
| Hook | Agent's inbox |
| Mail | Messages |
| Witness | Your monitoring code |
| Refinery | Your CI/CD |

### What Tinytown Doesn't Have

Deliberately omitted for simplicity:

| Gastown Feature | Tinytown Alternative |
|-----------------|---------------------|
| Dolt SQL | Redis (simpler) |
| Git-backed beads | Redis persistence |
| Two-level DB | Single Redis instance |
| Daemon processes | Your process manages |
| Auto-recovery | Manual retry logic |
| Formulas | Write code directly |
| MEOW orchestration | Direct API calls |

## Migration Steps

### Step 1: Install Tinytown

```bash
git clone https://github.com/redis-field-engineering/tinytown.git
cd tinytown
cargo install --path .
```

### Step 2: Initialize Your Project

**Gastown:**
```bash
# Multiple steps, daemon processes, config files...
gt boot
gt daemon start
# Configure rig, beads, etc.
```

**Tinytown:**
```bash
mkdir my-project && cd my-project
tt init --name my-project
# Done!
```

### Step 3: Create Agents

**Gastown:**
```bash
# Configure polecat pools, spawn through Mayor...
gt mayor attach
# "Create a polecat for backend work"
```

**Tinytown:**
```bash
tt spawn backend --cli claude
tt spawn frontend --cli auggie
tt spawn reviewer --cli codex-mini
```

### Step 4: Assign Work

**Gastown:**
```bash
# Create beads, slinging, convoys...
bd create --type task --title "Build API"
gt sling gt-abc12 gastown/polecats/Toast
gt convoy create "Feature X" gt-abc12
```

**Tinytown:**
```bash
tt assign backend "Build the REST API"
tt assign frontend "Build the UI"
```

### Step 5: Monitor Progress

**Gastown:**
```bash
gt convoy list
gt convoy status hq-cv-abc
gt feed
gt dashboard  # requires tmux
```

**Tinytown:**
```bash
tt status
tt list
```

## Code Migration

### Gastown Pattern: Tell the Mayor

```python
# Gastown: Complex orchestration
# You tell Mayor what you want, Mayor figures out the rest
gt mayor attach
> Build a user authentication system with login, signup, and password reset
# Mayor creates convoy, assigns polecats, tracks progress...
```

### Tinytown Pattern: Direct Control

```rust
// Tinytown: You're in control
let town = Town::connect(".").await?;

// Create your team
let designer = town.spawn_agent("designer", "claude").await?;
let backend = town.spawn_agent("backend", "auggie").await?;
let frontend = town.spawn_agent("frontend", "codex").await?;

// Assign work explicitly
designer.assign(Task::new("Design auth API schema")).await?;
wait_for_idle(&designer).await?;

backend.assign(Task::new("Implement auth endpoints")).await?;
frontend.assign(Task::new("Build login/signup UI")).await?;

// Wait for both
tokio::join!(
    wait_for_idle(&backend),
    wait_for_idle(&frontend)
);
```

## When to Use Tinytown vs Gastown

### Use Tinytown When:

✅ You want to understand the system  
✅ You need something working in 30 seconds  
✅ You're coordinating 1-5 agents  
✅ You want to write your own orchestration logic  
✅ Simple is better than feature-rich  

### Use Gastown When:

✅ You need 20+ concurrent agents  
✅ You need git-backed work history  
✅ You need automatic crash recovery  
✅ You need cross-project coordination  
✅ You have time to learn the system  

## Common Questions

**Q: Can I use both?**
A: Yes! Start with Tinytown for simplicity. If you outgrow it, Gastown's there.

**Q: Is Tinytown production-ready?**
A: For small teams and projects, yes. For enterprise scale, consider Gastown.

**Q: Can I migrate Tinytown work to Gastown?**
A: Tasks are JSON. You could write a converter to Beads format.

**Q: Does Tinytown support everything Gastown does?**
A: No, and that's the point. Tinytown does less, but what it does is simple.
