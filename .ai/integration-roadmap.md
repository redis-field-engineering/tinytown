# Tinytown × RAK/RAR Integration Roadmap

## Date: 2025-03-25

## Decision: Adopt Concepts, Not Code Directly

RAK and RAR are Python. Tinytown is Rust. Direct code reuse isn't practical, but the
**architectural patterns and Redis conventions** are highly valuable. The goal is to make
tinytown a first-class citizen in the RAR ecosystem by speaking the same Redis dialect.

## Phase 1: Cloud-Ready Foundation (Pre-requisites)

### 1.1 Containerize Tinytown
- [ ] Multi-stage Dockerfile (build Rust, slim runtime image)
- [ ] Townhall container (API + MCP + dispatcher in one process)
- [ ] Agent worker container (tt agent loop + coding CLI)
- [ ] docker-compose for local dev (Redis + townhall + N agents)

### 1.2 Health & Observability
- [ ] `/health` and `/ready` endpoints on townhall
- [ ] Structured JSON logging (already has tracing)
- [ ] Agent heartbeat mechanism (Redis key with TTL)
- [ ] Metrics endpoint (Prometheus-compatible)

### 1.3 Remote Redis Support
- [ ] Support Redis Cloud connection strings (TLS, auth)
- [ ] Remove hard dependency on Unix socket for cloud mode
- [ ] Connection pooling for remote Redis

## Phase 2: Adopt RAR Patterns in Rust

### 2.1 Docket Streams (Replace List-based backlog)
**Why**: Lists lose messages if consumer crashes before processing. Streams provide:
- Consumer groups (multiple workers, exactly-once delivery)
- Acknowledgment (XACK after completion)
- Replay (re-read failed messages)
- Visibility (XPENDING shows in-flight work)

**Migration**:
```
BEFORE: tt:{town}:backlog              (List, RPUSH/BLPOP)
AFTER:  tt:{town}:docket:tasks         (Stream, XADD/XREADGROUP)
        tt:{town}:docket:tasks:events  (Stream, progress/results)
```

### 2.2 Worker Lifecycle State Machine
Adopt RAR's worker states in tinytown agents:
```
cold → starting → idle → busy → draining → stopped → failed
```
Store in Redis: `tt:{town}:agent:{id}:lifecycle` (String with TTL)

### 2.3 Queue-Depth-Driven Scaling Signal
- Townhall exposes queue depth via API: `GET /api/scaling`
- Returns: `{ "pending_tasks": N, "active_agents": M, "desired_agents": K }`
- External scaler (KEDA, Cloud Run, custom) uses this to scale agents

### 2.4 Namespace Alignment
Optionally support RAR-compatible key prefixes:
```
rar:tenant:{tid}:project:{pid}:agent:tinytown:deployment:{did}:tasks:commands
```
This lets RAR's orchestrator discover and manage tinytown deployments.

## Phase 3: Mobile App Communication Layer

### 3.1 Real-Time Event Streaming
- [ ] SSE endpoint: `GET /api/events/stream` (mission status, agent activity)
- [ ] WebSocket endpoint: `GET /api/events/ws` (bidirectional)
- [ ] Redis Pub/Sub → SSE bridge (subscribe to `tt:{town}:events`)

### 3.2 Mobile-Friendly API
- [ ] `GET /api/missions` — list missions with summary status
- [ ] `GET /api/missions/{id}/timeline` — event timeline for a mission
- [ ] `POST /api/missions/{id}/approve` — human-in-the-loop approval
- [ ] `POST /api/missions/{id}/pause` — pause from phone
- [ ] Push notification webhooks (mission complete, needs approval, failure)

### 3.3 Authentication
- [ ] Bearer token auth (already partially implemented)
- [ ] OAuth2/OIDC for mobile app login
- [ ] API key management for programmatic access

## Phase 4: Full RAR Integration (Optional)

### 4.1 Register as RAR Agent Type
- Tinytown townhall registers itself with RAR control plane
- RAR manages tinytown deployments alongside Python agents
- Unified dashboard shows all agent types

### 4.2 Cross-Framework Communication
- Tinytown agents can dispatch sub-tasks to RAK Python agents
- RAK agents can trigger tinytown missions
- Shared Docket streams as the interop layer

## Redis Data Structure Comparison

| Concern | Tinytown (Now) | RAK | RAR | Tinytown (Target) |
|---------|---------------|-----|-----|-------------------|
| Task queue | List (BLPOP) | Docket Stream | Docket Stream | **Docket Stream** |
| Agent state | Hash | String/JSON | Hash | Hash (no change) |
| Task state | Hash | String/JSON | Hash+Stream | Hash + Event Stream |
| Status index | Set | Set | Stream events | Set (no change) |
| Broadcast | Pub/Sub | Pub/Sub | Pub/Sub | Pub/Sub (no change) |
| Activity log | List (bounded) | — | Stream | **Stream** |
| Mission state | String/JSON | — | — | String/JSON (no change) |
| Scaling signal | — | — | Stream depth | **API endpoint** |

## Phase 5: A2A + MCP as Standard Contracts (Andrew's RAR Direction)

Per Andrew (RAR author): the two standard contracts for agents in RAR are:
- **A2A** = conversation protocol ("talk to the agent")
- **MCP** = tool protocol ("call a function on the agent")

### A2A on Townhall (#62)
- `/.well-known/agent-card.json` — agent discovery
- `POST /message:send` — send message, get task back
- `POST /message:stream` — SSE streaming updates
- Push notification configs — A2A-native push
- Maps tinytown task states to A2A states (working, input-required, completed)

### Mission MCP Tools (#63)
- `mission.start`, `mission.status`, `mission.list`, `mission.stop`
- `mission.approve`, `mission.reject`, `mission.pause`, `mission.resume`
- `mission.input` — human-in-the-loop responses
- Complements existing agent/task/backlog MCP tools

### Mobile App = A2A + MCP Client
- Chat with conductor via A2A ("what's happening?", "approve the merge")
- Structured actions via MCP tools (start mission, pause, approve)
- Push notifications via A2A push notification protocol
- Dashboard via simple REST endpoint (convenience aggregation)

## Key Insight

The biggest win isn't code reuse — it's **protocol alignment**. Two layers:

**Redis layer**: If tinytown speaks Docket Streams like RAK/RAR, then:
1. RAR's orchestrator can manage tinytown workers
2. RAR's dashboard can show tinytown status
3. Tinytown and Python agents can share task queues

**API layer**: If tinytown speaks A2A + MCP, then:
4. `rak chat --target tinytown` just works
5. The mobile app is a standard A2A/MCP client, not a custom REST client
6. Other agents can delegate work to tinytown via A2A
7. Any A2A-compatible tool can interact with tinytown
