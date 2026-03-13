# Agent Coordination

How agents work together and decide when tasks are complete.

## The Simple Model

Tinytown keeps coordination simple:

1. **Conductor** orchestrates priorities, humans, and cross-team sequencing
2. **Workers** do the work and hand off obvious next steps directly
3. **Reviewer** decides when work is done and routes concrete fixes back to the owner
4. **Conductor** stays informed without becoming the bottleneck

## The Reviewer Pattern

Always include a reviewer agent. They're your quality gate:

```
┌────────────┐     work      ┌────────────┐
│  Conductor │ ────────────► │   Worker   │
└─────┬──────┘               └─────┬──────┘
      │                            │
      │                            │ completes
      │                            ▼
      │    review request    ┌────────────┐
      │ ────────────────────►│  Reviewer  │
      │                      └─────┬──────┘
      │                            │
      │◄───────────────────────────┘
      │      approve / reject
      │
      ▼
   Done (or assign fixes)
```

## Why a Reviewer?

Without a reviewer, who decides "done"?

| Approach | Problem |
|----------|---------|
| Worker decides | "I'm done" but is it good? |
| Conductor decides | Conductor may not understand domain |
| User decides | User has to check everything |
| **Reviewer decides** | ✓ Separation of concerns |

The reviewer pattern is used everywhere: code review, QA, editing. It works.

## How It Works in Practice

### 1. Conductor Spawns Team

```bash
tt spawn backend
tt spawn frontend
tt spawn reviewer  # Always include!
```

### 2. Workers Work

```bash
tt assign backend "Build the API"
tt assign frontend "Build the UI"
```

### 3. Route Review Directly When Work Is Ready

When implementation is ready, the next handoff is usually obvious:

```bash
tt send reviewer "API implementation is ready for review in src/api.rs. Route concrete fixes back to backend and copy conductor if needed."
```

### 4. Reviewer Responds

The reviewer either:
- **Approves**: "LGTM, API is solid"
- **Requests changes**: "Password hashing uses weak algorithm, fix needed"

### 5. Direct Handoffs First, Conductor for Escalation

- If approved → task is done
- If changes are concrete → reviewer sends them directly to the owning worker
- If priority, staffing, or human judgment is needed → reviewer or worker notifies conductor/supervisor

## Messages Between Agents

Agents can send messages directly via their inboxes:

```rust
// In code (for custom integrations)
let msg = Message::new(worker_id, reviewer_id, MessageType::Custom {
    kind: "ready_for_review".into(),
    payload: r#"{"files": ["src/api.rs"]}"#.into(),
});
channel.send(&msg).await?;
```

Direct agent-to-agent messaging should be the default when the next execution handoff is obvious:

- worker -> reviewer when code is ready for review
- reviewer -> worker when fixes are concrete
- worker -> worker when file ownership or sequencing is clear

Use `supervisor` / `conductor` when you need:

- human judgment
- priority changes
- cross-team sequencing
- escalation or blockers
- visibility for the broader town

## Keeping It Simple

Tinytown deliberately avoids:

- ❌ Complex state machines
- ❌ Automatic dependency resolution
- ❌ Event-driven triggers

Instead:

- ✅ Agents coordinate directly for obvious next steps
- ✅ Conductor checks `tt status` and stays informed
- ✅ Reviewer is the quality gate
- ✅ Conductor steps in for non-obvious or human decisions

This keeps execution explicit without routing every routine handoff through one inbox.

## Comparison with Gastown

| Aspect | Gastown | Tinytown |
|--------|---------|----------|
| Coordination | Mayor + Witness + Hooks | Conductor + Reviewer |
| Completion | Complex bead states | Reviewer approves |
| Automation | Event-driven | Conductor-driven |
| Complexity | High | Low |

Gastown automates more but is harder to understand. Tinytown is explicit.
