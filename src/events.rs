/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Redis Stream-based event log for real-time progress.
//!
//! Provides structured events emitted to Redis Streams for real-time
//! SSE/WebSocket feeds and cross-agent event correlation.
//!
//! # Stream Keys
//!
//! - `tt:{town}:events` — town-wide event feed
//! - `tt:{town}:events:agent:{agent_id}` — per-agent events
//! - `tt:{town}:events:mission:{mission_id}` — per-mission events
//!
//! # Retention
//!
//! - Town-wide stream: MAXLEN ~1000 (rolling window)
//! - Per-agent streams: MAXLEN ~100
//! - Per-mission streams: kept until mission archived (MAXLEN ~500)

use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::agent::{AgentId, SpawnMode};
use crate::error::Result;
use crate::mission::types::MissionId;
use crate::task::TaskId;

const TOWN_STREAM_MAXLEN: usize = 1000;
const AGENT_STREAM_MAXLEN: usize = 100;
const MISSION_STREAM_MAXLEN: usize = 500;

/// Event types emitted to Redis Streams (RAR-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    AgentStateChanged,
    AgentSpawned,
    AgentStopped,
    TaskAssigned,
    TaskCompleted,
    TaskFailed,
    MissionStateChanged,
    MissionWorkPromoted,
    MissionWorkAssigned,
    MissionWorkCompleted,
    MissionWorkBlocked,
    MissionHelpNeeded,
    MissionWatchTriggered,
    MissionEvent,
    // ── Collaboration events ──
    /// A task was delegated from one agent to another.
    TaskDelegated,
    /// An agent was interrupted (e.g., preempted by higher-priority work).
    AgentInterrupted,
    /// A previously interrupted agent was resumed.
    AgentResumed,
    /// An agent completed its assigned scope successfully.
    AgentCompleted,
    /// An agent failed its assigned scope.
    AgentFailed,
    /// Work was handed off to a reviewer agent.
    ReviewerHandoff,
    /// A reviewer approved the work under review.
    ReviewerApproval,
    /// An agent escalated to the conductor for guidance or priority change.
    ConductorEscalation,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_default();
        write!(f, "{}", s.trim_matches('"'))
    }
}

/// A structured event record (RAR-compatible schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TownEvent {
    pub event_type: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<AgentId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<MissionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_state: Option<String>,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    // ── Collaboration metadata ──
    /// Parent agent that spawned or delegated to this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<AgentId>,
    /// Child agent that was spawned or delegated to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_agent_id: Option<AgentId>,
    /// Human-readable scope description for the agent's current assignment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Spawn mode used when creating a child agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_mode: Option<SpawnMode>,
}

impl TownEvent {
    #[must_use]
    pub fn new(event_type: EventType, message: impl Into<String>) -> Self {
        Self {
            event_type,
            agent_id: None,
            task_id: None,
            mission_id: None,
            old_state: None,
            new_state: None,
            message: message.into(),
            timestamp: Utc::now(),
            parent_agent_id: None,
            child_agent_id: None,
            scope: None,
            spawn_mode: None,
        }
    }
    #[must_use]
    pub fn with_agent(mut self, id: AgentId) -> Self {
        self.agent_id = Some(id);
        self
    }
    #[must_use]
    pub fn with_task(mut self, id: TaskId) -> Self {
        self.task_id = Some(id);
        self
    }
    #[must_use]
    pub fn with_mission(mut self, id: MissionId) -> Self {
        self.mission_id = Some(id);
        self
    }
    #[must_use]
    pub fn with_transition(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.old_state = Some(old.into());
        self.new_state = Some(new.into());
        self
    }
    // ── Collaboration builder methods ──
    /// Set the parent agent (the agent that spawned or delegated).
    #[must_use]
    pub fn with_parent_agent(mut self, id: AgentId) -> Self {
        self.parent_agent_id = Some(id);
        self
    }
    /// Set the child agent (the agent that was spawned or delegated to).
    #[must_use]
    pub fn with_child_agent(mut self, id: AgentId) -> Self {
        self.child_agent_id = Some(id);
        self
    }
    /// Set the human-readable scope description for this event.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }
    /// Set the spawn mode for agent-spawn events.
    #[must_use]
    pub fn with_spawn_mode(mut self, mode: SpawnMode) -> Self {
        self.spawn_mode = Some(mode);
        self
    }
}

/// Redis Stream-backed event emitter.
#[derive(Clone)]
pub struct EventStream {
    conn: ConnectionManager,
    town_name: String,
}

impl EventStream {
    pub fn new(conn: ConnectionManager, town_name: impl Into<String>) -> Self {
        Self {
            conn,
            town_name: town_name.into(),
        }
    }

    fn town_stream_key(&self) -> String {
        format!("tt:{}:events", self.town_name)
    }
    fn agent_stream_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:events:agent:{}", self.town_name, agent_id)
    }
    fn mission_stream_key(&self, mission_id: MissionId) -> String {
        format!("tt:{}:events:mission:{}", self.town_name, mission_id)
    }

    #[instrument(skip(self, event), fields(event_type = %event.event_type))]
    pub async fn emit(&self, event: &TownEvent) -> Result<()> {
        let json = serde_json::to_string(event)?;
        let et = event.event_type.to_string();
        let fields: &[(&str, &str)] = &[("event_type", &et), ("data", &json)];
        self.xadd(&self.town_stream_key(), TOWN_STREAM_MAXLEN, fields)
            .await?;
        if let Some(aid) = event.agent_id {
            self.xadd(&self.agent_stream_key(aid), AGENT_STREAM_MAXLEN, fields)
                .await?;
        }
        // Also route collaboration events to parent/child agent streams
        if let Some(pid) = event.parent_agent_id {
            // Avoid duplicate if parent == agent_id
            if event.agent_id != Some(pid) {
                self.xadd(&self.agent_stream_key(pid), AGENT_STREAM_MAXLEN, fields)
                    .await?;
            }
        }
        if let Some(cid) = event.child_agent_id
            && event.agent_id != Some(cid)
            && event.parent_agent_id != Some(cid)
        {
            self.xadd(&self.agent_stream_key(cid), AGENT_STREAM_MAXLEN, fields)
                .await?;
        }
        if let Some(mid) = event.mission_id {
            self.xadd(&self.mission_stream_key(mid), MISSION_STREAM_MAXLEN, fields)
                .await?;
        }
        debug!("Emitted event: {}", event.event_type);
        Ok(())
    }

    async fn xadd(&self, key: &str, maxlen: usize, fields: &[(&str, &str)]) -> Result<()> {
        let mut conn = self.conn.clone();
        let mut cmd = redis::cmd("XADD");
        cmd.arg(key).arg("MAXLEN").arg("~").arg(maxlen).arg("*");
        for (k, v) in fields {
            cmd.arg(*k).arg(*v);
        }
        let _: String = cmd.query_async(&mut conn).await?;
        Ok(())
    }

    // ==================== Read ====================

    /// Read events from the town-wide stream (use "0-0" for all).
    #[instrument(skip(self))]
    pub async fn read_town_events(
        &self,
        last_id: &str,
        count: usize,
    ) -> Result<Vec<(String, TownEvent)>> {
        self.xrange(&self.town_stream_key(), last_id, count).await
    }

    /// Read events from a per-agent stream.
    #[instrument(skip(self))]
    pub async fn read_agent_events(
        &self,
        agent_id: AgentId,
        last_id: &str,
        count: usize,
    ) -> Result<Vec<(String, TownEvent)>> {
        self.xrange(&self.agent_stream_key(agent_id), last_id, count)
            .await
    }

    /// Read events from a per-mission stream.
    #[instrument(skip(self))]
    pub async fn read_mission_events(
        &self,
        mission_id: MissionId,
        last_id: &str,
        count: usize,
    ) -> Result<Vec<(String, TownEvent)>> {
        self.xrange(&self.mission_stream_key(mission_id), last_id, count)
            .await
    }

    /// Read the most recent N events from the town-wide stream (newest first,
    /// then reversed so the caller gets chronological order).
    pub async fn read_recent_town_events(&self, count: usize) -> Result<Vec<(String, TownEvent)>> {
        let mut results = self.xrevrange(&self.town_stream_key(), count).await?;
        results.reverse(); // oldest → newest for display
        Ok(results)
    }

    async fn xrange(
        &self,
        key: &str,
        start: &str,
        count: usize,
    ) -> Result<Vec<(String, TownEvent)>> {
        let mut conn = self.conn.clone();
        let raw: Vec<redis::Value> = redis::cmd("XRANGE")
            .arg(key)
            .arg(start)
            .arg("+")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await?;
        let mut results = Vec::new();
        for entry in raw {
            if let Some(pair) = Self::parse_stream_entry(&entry) {
                results.push(pair);
            }
        }
        Ok(results)
    }

    async fn xrevrange(&self, key: &str, count: usize) -> Result<Vec<(String, TownEvent)>> {
        let mut conn = self.conn.clone();
        let raw: Vec<redis::Value> = redis::cmd("XREVRANGE")
            .arg(key)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await?;
        let mut results = Vec::new();
        for entry in raw {
            if let Some(pair) = Self::parse_stream_entry(&entry) {
                results.push(pair);
            }
        }
        Ok(results)
    }

    fn parse_stream_entry(value: &redis::Value) -> Option<(String, TownEvent)> {
        let arr = match value {
            redis::Value::Array(a) => a,
            _ => return None,
        };
        if arr.len() < 2 {
            return None;
        }
        let id = match &arr[0] {
            redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
            _ => return None,
        };
        let fields = match &arr[1] {
            redis::Value::Array(a) => a,
            _ => return None,
        };
        let mut i = 0;
        while i + 1 < fields.len() {
            let key = match &fields[i] {
                redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                _ => {
                    i += 2;
                    continue;
                }
            };
            if key == "data" {
                let val = match &fields[i + 1] {
                    redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                    _ => return None,
                };
                return serde_json::from_str::<TownEvent>(&val)
                    .ok()
                    .map(|e| (id, e));
            }
            i += 2;
        }
        None
    }
}
