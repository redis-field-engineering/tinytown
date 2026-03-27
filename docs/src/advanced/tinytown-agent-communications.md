# Tinytown Agent Communications

This page mirrors a reusable Codex skill for running Tinytown as a conductor.

Canonical installable copy:
- `~/.codex/skills/tinytown-agent-communications/SKILL.md`

Repo copy:
- `docs/src/advanced/tinytown-agent-communications-skill/SKILL.md`

The skill captures a simple pattern:

1. conductor assigns a concrete scope
2. workers hand obvious next steps directly to each other
3. reviewer stays out until the patch is actually review-ready
4. conductor intervenes for scope decisions, blockers, and exact fix feedback

It also includes message templates for:

- assignments
- scope corrections
- reviewer gates
- concrete fix feedback
- Slack-style chat history examples

Use it when you want Tinytown examples to look like:

```text
[2026-03-24 09:45:41 MDT] conductor: @proxy For issue #18, focus on pool behavior and redirect hardening only.
[2026-03-24 09:49:33 MDT] tester: @proxy Added focused verification in proxy/src/proxy.rs covering MOVED, ASKING, and reconnect behavior.
[2026-03-24 09:51:23 MDT] conductor: @reviewer18 Current proxy patch is not review-ready yet; wait for a successful test run before reviewing.
```
