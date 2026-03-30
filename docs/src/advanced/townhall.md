# Townhall Control Plane

Townhall is the HTTP control plane for Tinytown. It exposes all Tinytown operations via REST API and MCP (Model Context Protocol), enabling remote management from web clients, mobile apps, and AI tools.

## Quick Start

```bash
# Start townhall daemon (REST API on port 8787)
townhall

# With verbose logging
townhall --verbose

# Custom port
townhall rest --port 9000

# For a specific town
townhall --town /path/to/project
```

## Modes

Townhall supports three modes:

| Mode | Command | Transport | Use Case |
|------|---------|-----------|----------|
| REST API | `townhall rest` | HTTP/JSON | Web/mobile clients, scripts |
| MCP stdio | `townhall mcp-stdio` | stdin/stdout | IDE extensions, Claude Desktop |
| MCP HTTP | `townhall mcp-http` | HTTP/SSE | Browser MCP clients |

### REST API (Default)

```bash
townhall rest --bind 127.0.0.1 --port 8787
```

All CLI operations are available as HTTP endpoints:

```bash
# Get status
curl http://localhost:8787/v1/status

# List agents
curl http://localhost:8787/v1/agents

# Spawn agent
curl -X POST http://localhost:8787/v1/agents \
  -H "Content-Type: application/json" \
  -d '{"name": "worker-1", "cli": "claude"}'

# Assign task
curl -X POST http://localhost:8787/v1/tasks/assign \
  -H "Content-Type: application/json" \
  -d '{"agent": "worker-1", "task": "Fix the bug"}'
```

### MCP Mode

For AI tool integration (Claude Desktop, VS Code, etc.):

```bash
# stdio transport (for Claude Desktop)
townhall mcp-stdio

# HTTP/SSE transport (for web clients)
townhall mcp-http --port 8788
```

See [MCP Interface](./mcp.md) for detailed MCP documentation.

## API Reference

### Endpoints

| Endpoint | Method | Scope | Description |
|----------|--------|-------|-------------|
| `/health` | GET | *public* | Liveness check with process uptime |
| `/ready` | GET | *public* | Readiness check against Redis-backed town state |
| `/metrics` | GET | *public* | Prometheus text metrics |
| `/v1/town` | GET | `town.read` | Get town info |
| `/v1/status` | GET | `town.read` | Get full status |
| `/v1/agents` | GET | `town.read` | List agents |
| `/v1/agents` | POST | `agent.manage` | Spawn agent |
| `/v1/agents/{agent}/kill` | POST | `agent.manage` | Stop agent |
| `/v1/agents/{agent}/restart` | POST | `agent.manage` | Restart agent |
| `/v1/agents/prune` | POST | `agent.manage` | Prune dead agents |
| `/v1/tasks/assign` | POST | `town.write` | Assign task |
| `/v1/tasks/pending` | GET | `town.read` | List pending tasks |
| `/v1/backlog` | GET | `town.read` | List backlog |
| `/v1/backlog` | POST | `town.write` | Add to backlog |
| `/v1/backlog/{task_id}/claim` | POST | `town.write` | Claim backlog task |
| `/v1/backlog/assign-all` | POST | `town.write` | Assign all backlog |
| `/v1/backlog/{task_id}` | DELETE | `town.write` | Remove backlog task |
| `/v1/messages/send` | POST | `town.write` | Send message |
| `/v1/agents/{agent}/inbox` | GET | `town.read` | Get inbox |

Compatibility aliases `/healthz` and `/readyz` are also available.
| `/v1/recover` | POST | `agent.manage` | Recover orphaned agents |
| `/v1/reclaim` | POST | `agent.manage` | Reclaim tasks |

See the [OpenAPI spec](https://github.com/redis-field-engineering/tinytown/blob/main/docs/openapi/townhall-v1.yaml) for complete API documentation.

### Error Handling

Errors follow [RFC 7807 Problem Details](https://datatracker.ietf.org/doc/html/rfc7807):

```json
{
  "type": "https://tinytown.dev/errors/404",
  "title": "Not Found",
  "status": 404,
  "detail": "Agent 'worker-99' not found"
}
```

## Configuration

Configure townhall in `tinytown.toml`:

```toml
[townhall]
bind = "127.0.0.1"      # Bind address (default: 127.0.0.1)
rest_port = 8080        # REST API port (default: 8080)
request_timeout_ms = 30000  # Request timeout (default: 30s)
```

For production deployments, enable [Authentication](./auth.md):

```toml
[townhall.auth]
mode = "api_key"
api_key_hash = "$argon2id$v=19$..."  # Use: tt generate-api-key
```

## Security

### Startup Safety Rules

Townhall enforces security by default:

1. **Non-loopback binding requires authentication** - Cannot bind to `0.0.0.0` with `auth.mode = "none"`
2. **Warnings for API key on non-loopback** - Recommends OIDC for production
3. **TLS/mTLS validation** - Fails fast on invalid certificate configuration

### Best Practices

- Use `127.0.0.1` for local development
- Enable API key or OIDC authentication for any network exposure
- Enable TLS for production deployments
- Use mTLS for service-to-service communication
