# Townhall MCP Server

Tinytown includes an MCP server in the `townhall` binary for LLM/tooling integrations.

## Start MCP

```bash
# MCP over stdio (for local MCP clients)
townhall mcp-stdio

# MCP over HTTP/SSE
townhall mcp-http

# Override bind/port (default port = rest_port + 1)
townhall mcp-http --bind 127.0.0.1 --port 8081
```

## Registered MCP Tools

Read tools:

- `town.get_status`
- `agent.list`
- `agent.inbox`
- `task.list_pending`
- `backlog.list`

Write tools:

- `task.assign`
- `message.send`
- `backlog.add`
- `backlog.claim`
- `backlog.assign_all`
- `backlog.remove`

Agent-management/recovery tools:

- `agent.spawn`
- `agent.kill`
- `agent.restart`
- `agent.prune`
- `recovery.recover_agents`
- `recovery.reclaim_tasks`

Tool responses are JSON payloads wrapped as:

```json
{
  "success": true,
  "data": {},
  "error": null
}
```

## Registered MCP Resources

Static resources:

- `tinytown://town/current`
- `tinytown://agents`
- `tinytown://backlog`

Resource templates:

- `tinytown://agents/{agent_name}`
- `tinytown://tasks/{task_id}`

## Registered MCP Prompts

- `conductor.startup_context`
- `agent.role_hint` (`agent_name` required, `tags` optional)

## Notes

- MCP tools call the same Tinytown service layer used by CLI and REST.
- `mcp-http` uses Tower MCP's HTTP/SSE transport and follows standard MCP message semantics.
