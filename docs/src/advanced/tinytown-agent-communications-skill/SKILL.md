---
name: tinytown-agent-communications
description: Use when coordinating Tinytown agents, running mission mode, or showing example conductor/worker/reviewer message flows. Covers task assignment, worker handoffs, reviewer gating, and Slack-style communication examples.
---

# Tinytown Agent Communications

Use this skill when the user wants help with Tinytown orchestration patterns rather than only code changes:

- starting or steering a multi-agent run
- assigning work to workers and reviewers
- showing example `tt send` / inbox message flows
- formatting example communication history as chat or Slack logs

## Default Pattern

1. Conductor assigns a concrete task to the owning worker.
2. Sidecar workers send focused verification or research back to the owner.
3. Reviewer waits until the work is actually review-ready.
4. Conductor steps in for scope decisions, blockers, and concrete correction.

Prefer direct worker-to-worker handoffs when the next step is obvious.

## Message Types

- `task`: use for new owned work
- `info`: use for progress, findings, or concrete fix feedback
- `query`: use only for real blockers or scope decisions
- `ack`: use for lightweight confirmation when needed

## Good Message Shape

Keep messages short and actionable:

- identify the issue / mission / task id
- say exactly what the recipient owns
- name the file or behavior to inspect
- say what to do next

## Recommended Templates

### Assignment

```text
@worker Mission <run-id>, issue #<n>.
Own <specific scope>. Preserve <constraints>.
When done, send reviewer a concrete handoff naming files changed and send supervisor an info update.
```

### Scope correction

```text
@worker For issue #<n>, focus on <scope>.
Do not expand into <out-of-scope area>; that belongs to issue #<m>.
```

### Test handoff

```text
@owner Added verification in <file> covering <cases>.
Remaining gap: <specific missing case>.
```

### Reviewer gate

```text
@reviewer Wait for a review-ready handoff, then review <files/behavior>.
Focus on <risk areas>. Route concrete fixes back to the owner.
```

### Concrete fix feedback

```text
@owner I ran <command> and the current patch fails on:
1. <specific defect>
2. <specific defect>
After fixing, rerun <command> and then hand off to reviewer.
```

## Slack-Style Transcript Format

When the user asks for examples, prefer this format:

```text
[YYYY-MM-DD HH:MM:SS TZ] sender: @recipient message
```

Example:

```text
[2026-03-24 09:45:41 MDT] conductor: @proxy For issue #18, focus on pool behavior and redirect hardening only.
[2026-03-24 09:49:33 MDT] tester: @proxy Added focused verification in proxy/src/proxy.rs covering MOVED, ASKING, and reconnect behavior.
[2026-03-24 09:51:23 MDT] conductor: @reviewer18 Current proxy patch is not review-ready yet; wait for a successful test run before reviewing.
```

## Practical Rules

- Do not send the reviewer in early.
- Do not let stale informational messages pile up without summarizing them.
- If a worker asks a scope question, answer narrowly and tie it back to issue ownership.
- If you verify locally and find defects, send exact failures, not vague “please fix” notes.
- If two agents touch the same file, watch for overlap and explicitly route ownership.
