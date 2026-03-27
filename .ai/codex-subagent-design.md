# Codex Subagent Design Notes

Date: 2026-03-27

Scope: research against the installed `@openai/codex` package in `/opt/homebrew/lib/node_modules/@openai/codex`, then the matching open-source source tag `rust-v0.115.0` from `https://github.com/openai/codex`.

Important framing:

- The npm package installed locally is mostly a thin Node wrapper around a native binary.
- The actual subagent implementation lives in the Rust source, not in the wrapper JS.
- These notes target Codex `0.115.0`.

## Executive Summary

Codex treats subagents as first-class threads managed by a shared `AgentControl`, not as ad hoc subprocesses. The parent model talks to them through a small control surface:

- `spawn_agent`
- `send_input`
- `wait_agent`
- `resume_agent`
- `close_agent`

Three design choices matter most for Tinytown:

1. Naming is split into a stable machine id and a human nickname.
2. Role is split into a human-facing role description and an optional hard config layer.
3. Communication is split into direct control-plane calls plus asynchronous status reflected back into the parent context.

That separation is why the system is fairly robust. The machine id is authoritative, the nickname is for humans, and the role can be either soft guidance or hard policy depending on whether it has a config file behind it.

## 1. How Codex Spawns Subagents

The `spawn_agent` tool handler parses the request, computes child depth, emits a `CollabAgentSpawnBeginEvent`, builds a child config from the live parent turn, applies requested model/reasoning overrides, applies the selected role layer, reapplies runtime policy, and finally asks `AgentControl` to create the thread. It then emits `CollabAgentSpawnEndEvent` and returns `{ agent_id, nickname }`.

Key source:

- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs:23-136`
- `codex-rs/core/src/tools/handlers/multi_agents.rs:244-369`

The child config starts from the parent turn’s effective state, including:

- model
- provider
- reasoning effort and summary
- developer instructions
- compact prompt
- approval policy
- sandbox policy
- shell environment policy
- cwd
- base instructions

This is explicit in:

- `codex-rs/core/src/tools/handlers/multi_agents.rs:251-257`
- `codex-rs/core/src/tools/handlers/multi_agents.rs:272-315`

One subtle but important detail: Codex reapplies runtime overrides after the role layer. That means role files can override model-related config, but live runtime safety and execution settings are forced back to the parent turn’s current values.

## 2. How It Names Subagents

Codex gives every spawned subagent:

- a stable thread id, used for all actual control calls
- an optional human-facing nickname, used in UI and history

The nickname is assigned inside `AgentControl`, not by the model. The parent model never invents the final nickname itself.

Key source:

- `codex-rs/core/src/agent/control.rs:97-128`
- `codex-rs/core/src/agent/guards.rs:69-153`
- `codex-rs/state/src/model/thread_metadata.rs:67-70`

Mechanics:

- If the selected role has `nickname_candidates`, Codex uses that pool.
- Otherwise it falls back to a built-in name list (`Euclid`, `Hypatia`, `Noether`, etc.).
- It randomly picks from unused names in the current session-wide pool.
- If the pool is exhausted, it resets the used-name set and starts adding ordinal suffixes like `Plato the 2nd`.
- Nickname uniqueness is session-scoped because all subagents share one `AgentControl`.

Important implementation details:

- Candidate lookup: `codex-rs/core/src/agent/control.rs:45-60`
- Reservation and uniqueness tracking: `codex-rs/core/src/agent/guards.rs:119-153`
- Ordinal suffixing after pool reset: `codex-rs/core/src/agent/guards.rs:34-50`
- Nickname persisted in thread metadata: `codex-rs/state/src/model/thread_metadata.rs:67-70`

This is a good pattern for Tinytown:

- Treat `id` as the real address.
- Treat `nickname` as a presentation affordance.
- Persist nickname separately so resume/reopen can keep continuity.

## 3. How It Defines Roles

Codex role selection is via `agent_type`. If omitted, it defaults to `default`.

Key source:

- `codex-rs/core/src/agent/role.rs:26-54`
- `codex-rs/core/src/agent/role.rs:339-416`

Built-in roles at this tag:

- `default`
- `explorer`
- `worker`

What matters is that a role has two different parts:

1. A human-facing description shown to the parent model in the `spawn_agent` tool schema.
2. An optional config file that is loaded as a high-precedence config layer for the child.

That distinction is explicit here:

- Role layer application: `codex-rs/core/src/agent/role.rs:30-110`
- Spawn tool role descriptions: `codex-rs/core/src/agent/role.rs:262-337`
- Built-in role declarations: `codex-rs/core/src/agent/role.rs:343-416`

### Built-in roles are mostly prompt-side conventions

At `rust-v0.115.0`, `explorer` has a description plus an embedded `explorer.toml`, but that file is empty in this tag. `worker` has no config file at all. So in practice:

- `explorer` and `worker` mainly shape the parent model’s delegation behavior through tool descriptions.
- They do not strongly enforce a special child prompt or different runtime behavior by default.

This is the most important nuance in the whole design.

Codex has both:

- soft roles: descriptive guidance to the parent model
- hard roles: roles backed by config files that alter child configuration

## 4. How Role Instructions Actually Reach the Child

For user-defined roles, Codex can load role TOML from either:

- `[agents.<role>]` entries in config
- discovered `agents/*.toml` files

Key source:

- `codex-rs/core/src/config/agent_roles.rs:17-105`
- `codex-rs/core/src/config/agent_roles.rs:135-291`

Important constraints:

- Role descriptions are required.
- Direct role files discovered under `agents/` must define `developer_instructions`.
- Nickname candidates are validated and normalized.

Key validation source:

- `codex-rs/core/src/config/agent_roles.rs:241-245`
- `codex-rs/core/src/config/agent_roles.rs:321-357`
- `codex-rs/core/src/config/agent_roles.rs:390-439`

The role config is loaded as a normal config layer with session-flag precedence:

- `codex-rs/core/src/agent/role.rs:56-110`
- `codex-rs/core/src/agent/role.rs:146-259`

That means a custom role can hard-set things like:

- `developer_instructions`
- `model`
- `model_reasoning_effort`
- other config values supported by Codex config

The tool schema even tells the parent model when a role has locked model/reasoning settings:

- `codex-rs/core/src/agent/role.rs:295-332`

Also, explicit `model` / `reasoning_effort` arguments on `spawn_agent` are applied before the role layer, so the role layer can override them. That is intentional, not incidental.

Relevant sources:

- Requested overrides: `codex-rs/core/src/tools/handlers/multi_agents.rs:316-369`
- Role applied after overrides: `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs:61-75`

For Tinytown, this suggests a strong design rule:

- Roles should not just be labels.
- A role should be able to carry an actual child-config layer.
- The orchestrator prompt can still describe roles, but policy should live in config when it must be enforced.

## 5. What the Parent Model Is Told About Spawning

The `spawn_agent` tool description is long and opinionated. It explicitly teaches the parent model:

- when delegation is allowed
- when not to delegate
- how to split work
- how to coordinate parallel work
- when to avoid waiting
- how to assign workers

Key source:

- `codex-rs/core/src/tools/spec.rs:1086-1123`

This means Codex does not only implement subagents in backend code. It also shapes the parent’s delegation style in the tool contract itself.

This is worth copying in Tinytown. The orchestration quality is partly coming from prompt policy, not just runtime mechanics.

## 6. How Parent and Child Communicate

### Direct communication path

Parent-to-child messaging is done through `AgentControl`, which sends typed `Op`s to the child thread:

- `Op::UserInput` for normal messages
- `Op::Interrupt` for redirection
- `Op::Shutdown` for closing

Key source:

- `codex-rs/core/src/agent/control.rs:296-331`
- `codex-rs/core/src/tools/handlers/multi_agents/send_input.rs:17-84`
- `codex-rs/core/src/tools/handlers/multi_agents/close_agent.rs:17-98`

`send_input` optionally interrupts first, then sends the new input:

- `codex-rs/core/src/tools/handlers/multi_agents/send_input.rs:36-42`

`wait_agent` subscribes to status streams and waits until any requested agent reaches a final state, subject to a timeout clamp:

- `codex-rs/core/src/tools/handlers/multi_agents/wait.rs:27-175`

This is a clean split:

- control path = explicit operations
- observation path = status subscriptions and events

### Interactive delegate plumbing

Internally, Codex subagents are wired with:

- an op channel into the child
- an event channel back out

Key source:

- `codex-rs/core/src/codex_delegate.rs:57-133`
- `codex-rs/core/src/codex_delegate.rs:400-412`

Notable details:

- Child approvals are intercepted and routed back through the parent session.
- Non-approval events are forwarded back to the consumer.
- Dynamic tools are not inherited into the child in this path (`dynamic_tools: Vec::new()`).

Key source:

- spawn args: `codex-rs/core/src/codex_delegate.rs:76-92`
- approval interception: `codex-rs/core/src/codex_delegate.rs:217-365`

### Centralized approvals

This is one of the strongest architectural decisions in the system.

The child does not own approval UX. Instead, delegated approval requests are routed back to the parent session, and optionally to a guardian subagent:

- shell command approval
- patch approval
- request permissions
- request user input

Key source:

- `codex-rs/core/src/codex_delegate.rs:257-365`
- `codex-rs/core/src/codex_delegate.rs:415-849`

Tinytown should probably copy this pattern. Centralized approval policy is much easier to reason about than per-subagent approval policy.

## 7. How Async Child State Is Reflected Back to the Parent

Codex does not rely only on `wait_agent`.

It also feeds subagent state back into the parent’s context in two ways:

1. Current open subagents are listed in the parent’s environment context.
2. Final child completion is injected as a contextual user message.

Key source:

- environment context assembly: `codex-rs/core/src/codex.rs:3529-3538`
- subagent list formatting: `codex-rs/core/src/agent/control.rs:383-415`
- completion watcher: `codex-rs/core/src/agent/control.rs:417-465`
- contextual message tags: `codex-rs/core/src/contextual_user_message.rs:12-15,82-95`

This is a very good design. It gives the parent model ambient awareness of:

- which children exist
- when a child finishes

without forcing constant explicit polling.

For Tinytown, I would copy this almost directly:

- maintain a structured “open agents” block in the orchestrator’s visible context
- inject lightweight async completion messages when children hit terminal states

## 8. How Resume and Fork Work

### Resume

Closed agents can be resumed from rollout history. On resume, Codex tries to restore the persisted nickname and role from sqlite metadata, and then reserves that nickname again if possible.

Key source:

- resume handler: `codex-rs/core/src/tools/handlers/multi_agents/resume_agent.rs:18-163`
- metadata rehydration: `codex-rs/core/src/agent/control.rs:221-294`

### Fork

If `fork_context=true`, Codex does not merely copy settings. It forks the parent rollout history and inserts a synthetic output message telling the child:

- you are newly spawned
- the forked history is background context
- the next user message is the new task

Key source:

- `codex-rs/core/src/agent/control.rs:29-30`
- `codex-rs/core/src/agent/control.rs:134-192`

This is also worth copying. A forked child needs an explicit boundary marker so it does not confuse inherited context with the new task.

## 9. How This Shows Up in UI / App Protocol

The app-server exposes collaboration actions as `collabToolCall` items with:

- tool
- sender thread id
- receiver thread id or new thread id
- prompt
- agent status

Key source:

- `codex-rs/app-server/README.md:826-833`

The spawned thread metadata also includes:

- `agent_nickname`
- `agent_role`

Key source:

- `codex-rs/app-server-protocol/src/protocol/v2.rs:3561-3566`
- `codex-rs/state/src/model/thread_metadata.rs:67-70`

This is another good pattern for Tinytown:

- keep orchestration events as first-class structured history items
- do not bury subagent lifecycle in free-form logs

## 10. Hard-Enforced vs Soft-Conventional Behavior

This distinction matters.

Hard-enforced in code:

- stable ids and session-scoped nickname assignment
- max thread count and max depth
- runtime policy inheritance
- central approval routing
- resume/fork mechanics
- async completion reflection
- role config layering when a config file exists

Soft-conventional via tool descriptions / prompting:

- when to delegate
- which role to choose
- explorer behavior
- worker coordination behavior
- ownership guidance
- “not alone in the codebase” guidance

At this tag, most built-in role behavior is soft-conventional, not hard-enforced.

## 11. Recommendations For Tinytown

### Adopt directly

1. Keep `agent_id` and `nickname` separate.
2. Store `role` and `nickname` in thread/session metadata.
3. Make roles optionally backed by config layers, not just prompt labels.
4. Reapply live runtime policy after role layering.
5. Centralize approvals in the orchestrator, not in workers.
6. Keep a structured subagent list in orchestrator-visible context.
7. Inject async completion messages back into the orchestrator thread.
8. Support both fresh-spawn and forked-context spawn modes.

### Tinytown-specific improvements

1. Make built-in Tinytown roles hard-backed by config, not only described in prompts.
2. Add per-role nickname pools so names communicate intent, for example researcher/reviewer/worker families.
3. Treat role descriptions as orchestration guidance and role config as enforcement.
4. Preserve stable human labels across resume/restart exactly as Codex does.
5. Expose collab lifecycle as structured history events so the orchestrator can reason over them.

### Suggested Tinytown role model

- `default`: no special enforcement
- `researcher` / `explorer`: read-heavy defaults, maybe reduced edit permissions
- `worker`: normal editing permissions, explicit ownership required
- `reviewer`: review-mode instructions and stricter “no implementation unless asked” defaults
- `awaiter` or `runner`: long-running verification / test watcher role

Codex already hints at this direction. Tinytown can make it more explicit and more enforceable.

## 12. Source Pointers

Primary files I used:

- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/send_input.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/resume_agent.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/close_agent.rs`
- `codex-rs/core/src/tools/handlers/multi_agents.rs`
- `codex-rs/core/src/agent/control.rs`
- `codex-rs/core/src/agent/guards.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/config/agent_roles.rs`
- `codex-rs/core/src/codex_delegate.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/contextual_user_message.rs`
- `codex-rs/app-server/README.md`
- `codex-rs/state/src/model/thread_metadata.rs`

## Bottom Line

Codex’s subagent design is not “spawn a helper and hope for the best.” It is a small thread-oriented control system with:

- stable ids
- separate human nicknames
- role-as-label and role-as-config separation
- shared session-scoped guards
- centralized approvals
- explicit async state reflection into the parent context

That combination is the part Tinytown should emulate, more than any single role name or prompt string.
