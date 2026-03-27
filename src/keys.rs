/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Redis key conventions for Tinytown.
//!
//! Single source of truth for all Redis key generation. All keys are namespaced
//! by town name to ensure isolation when multiple towns share the same Redis instance.
//!
//! # Key Schema
//!
//! ## Agent Keys
//! - `tt:{town}:agent:{agent_id}` — Agent state (Hash)
//! - `tt:{town}:inbox:{agent_id}` — Agent message queue (List)
//! - `tt:{town}:urgent:{agent_id}` — High-priority messages (List)
//! - `tt:{town}:activity:{agent_id}` — Activity log (List, bounded, 1hr TTL)
//! - `tt:{town}:stop:{agent_id}` — Stop signal flag (String)
//!
//! ## Task Keys
//! - `tt:{town}:task:{task_id}` — Task state (Hash)
//! - `tt:{town}:backlog` — Unassigned task queue (List)
//!
//! ## Docket Keys (Redis Streams)
//! - `tt:{town}:docket:tasks` — Task dispatch stream
//! - `tt:{town}:docket:events` — Task lifecycle events stream
//!
//! ## Mission Keys
//! - `tt:{town}:mission:{run_id}` — Mission metadata (String/JSON)
//! - `tt:{town}:mission:{run_id}:work` — WorkItem collection (Hash)
//! - `tt:{town}:mission:{run_id}:watch` — WatchItem collection (Hash)
//! - `tt:{town}:mission:{run_id}:events` — Activity log (List, bounded)
//! - `tt:{town}:mission:{run_id}:control` — Control messages (Hash)
//! - `tt:{town}:mission:{run_id}:dispatch_lock` — Dispatcher lease (String)
//! - `tt:{town}:mission:active` — Active mission IDs (Set)
//!
//! ## Broadcast
//! - `tt:{town}:broadcast` — Pub/Sub broadcast channel

use std::fmt;

use crate::agent::AgentId;
use crate::mission::types::MissionId;
use crate::task::TaskId;

/// Centralized Redis key generator for a town.
///
/// All keys follow the pattern `tt:{town}:{type}:{id}` to ensure namespace
/// isolation between towns sharing the same Redis instance.
#[derive(Clone, Debug)]
pub struct RedisKeys {
    /// Town name used as the namespace prefix.
    town_name: String,
}

impl RedisKeys {
    /// Create a new key generator for the given town.
    pub fn new(town_name: impl Into<String>) -> Self {
        Self {
            town_name: town_name.into(),
        }
    }

    /// Get the town name.
    pub fn town_name(&self) -> &str {
        &self.town_name
    }

    // ==================== Agent Keys ====================

    /// Agent state hash: `tt:{town}:agent:{agent_id}`
    pub fn agent_state(&self, agent_id: AgentId) -> String {
        format!("tt:{}:agent:{}", self.town_name, agent_id)
    }

    /// Agent message inbox: `tt:{town}:inbox:{agent_id}`
    pub fn agent_inbox(&self, agent_id: AgentId) -> String {
        format!("tt:{}:inbox:{}", self.town_name, agent_id)
    }

    /// Agent urgent inbox: `tt:{town}:urgent:{agent_id}`
    pub fn agent_urgent(&self, agent_id: AgentId) -> String {
        format!("tt:{}:urgent:{}", self.town_name, agent_id)
    }

    /// Agent activity log: `tt:{town}:activity:{agent_id}`
    pub fn agent_activity(&self, agent_id: AgentId) -> String {
        format!("tt:{}:activity:{}", self.town_name, agent_id)
    }

    /// Agent stop flag: `tt:{town}:stop:{agent_id}`
    pub fn agent_stop(&self, agent_id: AgentId) -> String {
        format!("tt:{}:stop:{}", self.town_name, agent_id)
    }

    // ==================== Task Keys ====================

    /// Task state hash: `tt:{town}:task:{task_id}`
    pub fn task(&self, task_id: TaskId) -> String {
        format!("tt:{}:task:{}", self.town_name, task_id)
    }

    /// Backlog queue: `tt:{town}:backlog`
    pub fn backlog(&self) -> String {
        format!("tt:{}:backlog", self.town_name)
    }

    // ==================== Docket Keys (Streams) ====================

    /// Docket task dispatch stream: `tt:{town}:docket:tasks`
    pub fn docket_tasks(&self) -> String {
        format!("tt:{}:docket:tasks", self.town_name)
    }

    /// Docket task events stream: `tt:{town}:docket:events`
    pub fn docket_events(&self) -> String {
        format!("tt:{}:docket:events", self.town_name)
    }

    // ==================== Event Stream Keys ====================

    /// Town-wide event stream: `tt:{town}:events`
    pub fn events(&self) -> String {
        format!("tt:{}:events", self.town_name)
    }

    /// Per-agent event stream: `tt:{town}:events:agent:{agent_id}`
    pub fn events_agent(&self, agent_id: AgentId) -> String {
        format!("tt:{}:events:agent:{}", self.town_name, agent_id)
    }

    /// Per-mission event stream: `tt:{town}:events:mission:{mission_id}`
    pub fn events_mission(&self, mission_id: MissionId) -> String {
        format!("tt:{}:events:mission:{}", self.town_name, mission_id)
    }

    // ==================== Broadcast ====================

    /// Broadcast pub/sub channel: `tt:{town}:broadcast`
    pub fn broadcast(&self) -> String {
        format!("tt:{}:broadcast", self.town_name)
    }

    // ==================== Mission Keys ====================

    /// Mission metadata: `tt:{town}:mission:{run_id}`
    pub fn mission(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}", self.town_name, id)
    }

    /// Mission work items: `tt:{town}:mission:{run_id}:work`
    pub fn mission_work(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:work", self.town_name, id)
    }

    /// Mission watch items: `tt:{town}:mission:{run_id}:watch`
    pub fn mission_watch(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:watch", self.town_name, id)
    }

    /// Mission events log: `tt:{town}:mission:{run_id}:events`
    pub fn mission_events(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:events", self.town_name, id)
    }

    /// Mission control messages: `tt:{town}:mission:{run_id}:control`
    pub fn mission_control(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:control", self.town_name, id)
    }

    /// Mission dispatcher lock: `tt:{town}:mission:{run_id}:dispatch_lock`
    pub fn mission_dispatch_lock(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:dispatch_lock", self.town_name, id)
    }

    /// Active missions set: `tt:{town}:mission:active`
    pub fn mission_active(&self) -> String {
        format!("tt:{}:mission:active", self.town_name)
    }

    // ==================== Scan Patterns ====================

    /// Pattern for all keys in this town: `tt:{town}:*`
    pub fn pattern_all(&self) -> String {
        format!("tt:{}:*", self.town_name)
    }

    /// Pattern for all agent state keys: `tt:{town}:agent:*`
    pub fn pattern_agents(&self) -> String {
        format!("tt:{}:agent:*", self.town_name)
    }

    /// Pattern for all inbox keys: `tt:{town}:inbox:*`
    pub fn pattern_inboxes(&self) -> String {
        format!("tt:{}:inbox:*", self.town_name)
    }

    /// Pattern for all stop flag keys: `tt:{town}:stop:*`
    pub fn pattern_stops(&self) -> String {
        format!("tt:{}:stop:*", self.town_name)
    }

    /// Pattern for all activity log keys: `tt:{town}:activity:*`
    pub fn pattern_activities(&self) -> String {
        format!("tt:{}:activity:*", self.town_name)
    }

    /// Pattern for all urgent inbox keys: `tt:{town}:urgent:*`
    pub fn pattern_urgents(&self) -> String {
        format!("tt:{}:urgent:*", self.town_name)
    }

    /// Pattern for all task keys: `tt:{town}:task:*`
    pub fn pattern_tasks(&self) -> String {
        format!("tt:{}:task:*", self.town_name)
    }

    /// Pattern for all mission keys: `tt:{town}:mission:*`
    pub fn pattern_missions(&self) -> String {
        format!("tt:{}:mission:*", self.town_name)
    }
}

impl fmt::Display for RedisKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RedisKeys({})", self.town_name)
    }
}
