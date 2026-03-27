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

use crate::agent::AgentId;
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
