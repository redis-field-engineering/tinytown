# Tinytown Cloud Deployment Analysis

## Date: 2025-03-25

## Current Architecture (Stateless, Redis-Backed)

Tinytown is already well-positioned for cloud deployment:
- **All state lives in Redis** — agents, tasks, missions, messages, work items
- **Lease-based locking** — safe for multiple dispatcher instances
- **Tick-based scheduling** — idempotent, crash-safe, restartable
- **No in-memory state** — any process can restart and resume

### Current Components
| Component | Binary | Role | Always-On? |
|-----------|--------|------|------------|
| Townhall | `townhall` | REST API + MCP control plane | ✅ Yes |
| Conductor | `tt conductor` | AI orchestrator | ✅ Yes (or on-demand) |
| Dispatcher | `tt mission dispatch` | Mission tick loop | ✅ Yes |
| Agents | `tt spawn` | Worker processes | ❌ Scale to 0 |

### Redis Key Schema
```
tt:{town}:inbox:{agent_id}          # List — agent message queue
tt:{town}:agent:{agent_id}          # Hash — agent state
tt:{town}:backlog                   # List — unassigned tasks
tt:{town}:mission:{run_id}          # String — mission metadata
tt:{town}:mission:{run_id}:work     # Hash — work items
tt:{town}:mission:{run_id}:watch    # Hash — watch items
tt:{town}:mission:active            # Set — active mission IDs
tt:{town}:broadcast                 # Pub/Sub — announcements
```

## Reference Frameworks

### redis-agent-kit (RAK)
Python library for building agents with Redis primitives.

**Key patterns tinytown should consider:**
- **Middleware chain** — cross-cutting concerns (logging, metrics, error handling) as composable layers
- **Docket (Redis Streams)** — reliable task dispatch with consumer groups and acknowledgment
- **TaskContext** — structured context object passed through handler chain
- **Memory management** — working memory + long-term memory with session binding
- **Protocol adapters** — REST, A2A, ACP translation layer

### redis-agent-runtime (RAR)
Operational layer for deploying and scaling agents.

**Key patterns tinytown should adopt:**
- **Scale-to-zero** — Lazy startup policy: no compute until first request
- **Docket streams** — Redis Streams as the coupling point between orchestrator and workers
- **Agent registry** — immutable versioned deployments with stable endpoints
- **Worker lifecycle state machine** — cold → starting → idle → busy → draining → stopped
- **Namespace isolation** — `{prefix}:tenant:{tid}:project:{pid}:agent:{aid}:deployment:{did}`
- **Queue-depth-driven scaling** — orchestrator updates queue depth, operator reconciles pod count

## Gap Analysis

### What Tinytown Has That Others Don't
1. **Rust performance** — sub-ms Redis latency, no GC, efficient concurrency
2. **Mission DAG orchestration** — dependency-aware work graph with watch engine
3. **Multi-CLI support** — Claude, Augment, Codex, Gemini, Copilot, Cursor, Aider
4. **Priority message queues** — LPUSH/RPUSH for urgent vs normal
5. **Conductor AI** — built-in AI orchestrator role

### What Tinytown Needs for Cloud
1. **Container packaging** — Dockerfile for townhall + agent workers
2. **Scale-to-zero agents** — queue-depth-driven pod lifecycle
3. **Durable task dispatch** — Redis Streams instead of Lists for reliability
4. **Health endpoints** — liveness/readiness probes for k8s
5. **Auth for remote access** — token-based auth for mobile/remote clients
6. **Event streaming** — WebSocket/SSE for real-time status to mobile app
7. **Namespace isolation** — multi-tenant key prefixing

### What Can Be Used Directly
1. **RAR's Kubernetes operator pattern** — AgentWorker CRD for managing tinytown agent pods
2. **RAR's scale-to-zero logic** — lazy startup policy maps directly to tinytown agents
3. **RAK's Docket streams** — drop-in replacement for tinytown's List-based backlog
4. **RAR's deployment flow** — build → register → assign endpoints → lazy start

## Recommended Cloud Architecture

```
┌──────────────────────────────────────────────────────┐
│  Mobile App / Remote Client                          │
│  (iOS/Android — mission control, status, approvals)  │
└──────────────┬───────────────────────────────────────┘
               │ HTTPS (REST + WebSocket/SSE)
┌──────────────▼───────────────────────────────────────┐
│  Townhall (Always-On, Small Instance)                │
│  ├─ REST API (task ingress, status, control)         │
│  ├─ MCP endpoint (AI tool integration)               │
│  ├─ WebSocket/SSE (real-time status to mobile)       │
│  └─ Dispatcher loop (mission tick engine)            │
└──────────────┬───────────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────────┐
│  Redis Cloud (Managed, Always-On)                    │
│  ├─ Streams (Docket — durable task queues)           │
│  ├─ Hashes (agent/task/mission state)                │
│  ├─ Sets (indices, active missions)                  │
│  └─ Pub/Sub (real-time notifications)                │
└──────────────┬───────────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────────┐
│  Agent Workers (Scale-to-Zero)                       │
│  ├─ Cloud Run / K8s pods / Lambda                    │
│  ├─ Pulled from Docket stream when work arrives      │
│  ├─ Each runs: tt agent + coding CLI (claude, etc)   │
│  └─ Idle timeout → scale to 0                        │
└──────────────────────────────────────────────────────┘
```

### Cost Model
- **Always-on**: Townhall (tiny — 256MB) + Redis Cloud ($7-30/mo)
- **Scale-to-zero**: Agent workers only run during active missions
- **Estimated idle cost**: ~$10-40/month (Redis + small VM)
- **Active mission cost**: + compute per agent-hour
