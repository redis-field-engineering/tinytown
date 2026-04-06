# Townhall REST API

`townhall` is Tinytown's REST control plane daemon. It exposes the same orchestration services used by the `tt` CLI over HTTP.

## Start the Server

```bash
# From a town directory
townhall

# Explicit REST mode
townhall rest

# Override bind/port
townhall rest --bind 127.0.0.1 --port 8080
```

Defaults come from `tinytown.toml`:

```toml
[townhall]
bind = "127.0.0.1"
rest_port = 8080
request_timeout_ms = 30000
```

## Endpoint Groups

The router is split into public/read/write/management groups:

- Public: `GET /health`, `GET /ready`, `GET /metrics`, `GET /api/scaling`
- Read (`town.read`): `GET /v1/town`, `GET /v1/status`, `GET /v1/agents`, `GET /v1/tasks/pending`, `GET /v1/backlog`, `GET /v1/agents/{agent}/inbox`
- Write (`town.write`): `POST /v1/tasks/assign`, `POST /v1/backlog`, `POST /v1/backlog/{task_id}/claim`, `POST /v1/backlog/assign-all`, `DELETE /v1/backlog/{task_id}`, `POST /v1/messages/send`
- Agent management (`agent.manage`): `POST /v1/agents`, `POST /v1/agents/{agent}/kill`, `POST /v1/agents/{agent}/restart`, `POST /v1/agents/prune`, `POST /v1/recover`, `POST /v1/reclaim`

The public probes have distinct purposes:

- `/health`: lightweight process liveness with `uptime_secs`
- `/ready`: verifies townhall can still reach Redis, reporting Redis latency, town name, and dispatcher heartbeat state
- `/metrics`: Prometheus-style text metrics for agent counts by state, task queue depth, completed tasks, active missions, and Redis latency
- `/api/scaling`: JSON scaling signal for autoscalers, including queue depth, in-flight work, desired worker count, and a scaling recommendation

Compatibility aliases `/healthz` and `/readyz` remain available for existing deployments.

## Scaling Signal API

`GET /api/scaling` exposes an autoscaler-friendly snapshot:

```json
{
  "town": "mytown",
  "timestamp": "2026-04-04T18:00:00Z",
  "queue_depth": 3,
  "pending_tasks": 2,
  "in_flight_tasks": 1,
  "active_agents": 1,
  "cold_agents": 0,
  "desired_agents": 3,
  "max_agents": 10,
  "scaling_recommendation": "scale_up"
}
```

`scaling_recommendation` values:

- `scale_up`: pending + in-flight work exceeds current active workers
- `steady`: current active workers match demand
- `scale_down`: extra workers are running and there is no queued work
- `scale_to_zero`: no queued work, no in-flight work, and all active workers have been idle past the configured timeout

The worker idle timeout is configured in `tinytown.toml`:

```toml
[agent]
idle_timeout_secs = 300
```

When the timeout expires, the worker transitions `Idle -> Draining -> Stopped` and exits cleanly with status code `0`. That gives an external autoscaler a stable signal for both scale-down and full scale-to-zero.

When `use_streams = true`, the scaling endpoint reports `pending_tasks` from the consumer-group unread lag, `in_flight_tasks` from `XPENDING`, and `queue_depth` as the sum of unread plus in-flight work. This avoids counting acknowledged stream entries that are retained for audit/history.

## Autoscaler Integration

Cloud Run:

- Set `min-instances=0`
- Poll `/api/scaling`
- Use `desired_agents` or `queue_depth` to drive instance count

KEDA:

```yaml
apiVersion: keda.sh/v1alpha1
kind: ScaledObject
metadata:
  name: tinytown-agents
spec:
  scaleTargetRef:
    name: tinytown-agent-worker
  minReplicaCount: 0
  maxReplicaCount: 10
  triggers:
    - type: metrics-api
      metadata:
        url: "http://townhall:8080/api/scaling"
        valueLocation: "queue_depth"
        targetValue: "1"
```

Custom scaler:

- Poll `/api/scaling` on a short interval
- Start workers when `scaling_recommendation` is `scale_up`
- Remove workers when `scaling_recommendation` is `scale_down` or `scale_to_zero`
- Prefer `desired_agents` as the authoritative target replica count

## Authentication

`townhall` supports three auth modes in config:

- `none` (default): local/no-auth mode
- `api_key`: API key via `Authorization: Bearer <key>` or `X-API-Key`
- `oidc`: declared in config, not yet implemented in middleware

Generate an API key + Argon2 hash:

```bash
tt auth gen-key
```

Then configure:

```toml
[townhall.auth]
mode = "api_key"
api_key_hash = "$argon2id$..."
api_key_scopes = ["town.read", "town.write", "agent.manage"]
```

Example request:

```bash
curl -H "Authorization: Bearer $TOWNHALL_API_KEY" \
  http://127.0.0.1:8080/v1/status
```

## Startup Safety Rules

At startup, `townhall` fails fast when:

- Binding to a non-loopback address with `auth.mode = "none"`
- TLS is enabled but `cert_file`/`key_file` are missing or invalid
- mTLS is required but `ca_file` is missing or invalid

## OpenAPI Spec

The REST contract is documented in:

- `docs/openapi/townhall-v1.yaml`

You can load this file in Swagger UI, Stoplight, or Redoc for interactive exploration.
