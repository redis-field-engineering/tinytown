# Why Tinytown?

A practical guide to why Tinytown exists and where it fits.

## The Problem with Complex Systems

Gastown is impressive engineering. It has:
- Automatic crash recovery
- Git-backed work history  
- Multi-agent coordination
- Visual dashboards
- Sophisticated orchestration

But it also has:
- **317,898 lines of code** to understand
- **50+ concepts** to learn
- **Hours of setup** before your first task
- **Days of learning** before you're productive

## The Tinytown Starting Point

> "Make it work. Make it simple. Stop."

Tinytown started with a simple idea: keep the orchestration stack small enough that one team could understand and modify it directly.

That goal still matters, but the project has also shown that multi-agent coding systems pick up real complexity once you add durable state, backlog management, mission scheduling, recovery paths, and agent-to-agent coordination.

### 1. Start with the Core Workflow

90% of multi-agent orchestration is:
1. Create agents
2. Assign tasks
3. Wait for completion
4. Check results

Tinytown was built to make that loop fast and direct, then add the extra machinery only when repeated operational problems justified it.

### 2. Complexity Compounds

Every feature adds:
- Code to maintain
- Concepts to learn
- Bugs to fix
- Documentation to write

Tinytown has grown to roughly **15K lines of production Rust** and about **19K lines including tests**. That is no longer tiny, but it is still a size where one team can understand the whole system and keep the complexity grounded in the code instead of hiding it behind layers of infrastructure.

### 3. Explicit is Better Than Magic

Gastown's Mayor "figures things out" for you:
```bash
gt mayor attach
> Build an authentication system
# Mayor creates convoy, spawns agents, distributes work...
```

Tinytown makes you say what you want:
```rust
architect.assign(Task::new("Design auth system")).await?;
developer.assign(Task::new("Implement auth")).await?;
tester.assign(Task::new("Test auth")).await?;
```

More typing, but you know exactly what's happening.

### 4. Recovery is Your Responsibility

Gastown: Witness patrols, Deacon monitors, Boot watches Deacon...

Tinytown: You write a loop:
```rust
if agent.state == AgentState::Error {
    respawn_and_retry(agent).await?;
}
```

Is this more work? Yes. Some orchestration complexity is unavoidable. Tinytown's approach is to keep that complexity explicit instead of pretending it does not exist.

## The Tradeoffs

### What You Gain

✅ **Understanding** — You know how it works  
✅ **Speed** — Running in 30 seconds  
✅ **Debuggability** — ~15K lines of production Rust to inspect  
✅ **Control** — You decide everything  
✅ **Focused model** — 7 core concepts  

### What You Lose

❌ **Automation** — You write recovery logic  
❌ **Scale** — Designed for 1-10 agents  
❌ **History** — No git-backed audit trail  
❌ **Visualization** — No built-in dashboard  
❌ **Federation** — Single machine focus  

## When to Choose What

### Choose Tinytown If:

- You're learning agent orchestration
- You want to ship something today
- You have 1-5 agents
- You prefer explicit over magic
- You value understanding over features

### Choose Gastown If:

- You need 20+ concurrent agents
- You need audit trails
- You need automatic recovery
- You need cross-project coordination
- You have time to learn the system

### Choose Both If:

Start with Tinytown. Learn the patterns. If you outgrow it, Gastown will make more sense because you understand what problems it's solving.

## A Practical Test

Ask yourself:

1. **How many agents do I need?**
   - 1-5: Tinytown
   - 10+: Consider Gastown

2. **How important is automatic recovery?**
   - Nice to have: Tinytown
   - Critical: Gastown

3. **How much time do I have?**
   - Minutes: Tinytown
   - Days/weeks: Either

4. **Do I want to understand the system?**
   - Yes: Tinytown
   - No, just make it work: Gastown (eventually)

## The Honest Answer

Tinytown exists because there was room for a smaller, faster-to-modify orchestration system built around Redis primitives.

If you've bounced off a larger system, Tinytown may be a better place to start. If you need the broader machinery later, that does not mean Tinytown failed; it means orchestration work has real complexity and different tools make different tradeoffs.

Start with the smallest system that honestly fits the work. Add complexity when the work demands it, not because the marketing copy says you never will.

> "Perfection is achieved not when there is nothing more to add, but when there is nothing left to take away."
> — Antoine de Saint-Exupéry
