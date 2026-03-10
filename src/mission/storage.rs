/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission mode Redis storage layer.
//!
//! Provides persistence for MissionRun, WorkItem, and WatchItem records.
//! Uses Redis Hashes for efficient field-level updates.
//!
//! Key Schema:
//! - `tt:{town}:mission:{run_id}` - MissionRun metadata (Hash)
//! - `tt:{town}:mission:{run_id}:work` - WorkItems (Hash: id -> JSON)
//! - `tt:{town}:mission:{run_id}:watch` - WatchItems (Hash: id -> JSON)
//! - `tt:{town}:mission:{run_id}:events` - Activity log (List, bounded)
//! - `tt:{town}:mission:active` - Active mission IDs (Set)

use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use tracing::{debug, instrument};

use crate::error::Result;
use crate::mission::types::{
    MissionId, MissionRun, WatchId, WatchItem, WorkItem, WorkItemId,
};

/// Maximum events to keep in the activity log.
const MAX_EVENTS: isize = 100;

/// Mission storage operations.
///
/// Wraps a Redis connection with town-namespaced key generation.
#[derive(Clone)]
pub struct MissionStorage {
    conn: ConnectionManager,
    town_name: String,
}

impl MissionStorage {
    /// Create a new MissionStorage.
    pub fn new(conn: ConnectionManager, town_name: impl Into<String>) -> Self {
        Self {
            conn,
            town_name: town_name.into(),
        }
    }

    // ==================== Key Generation ====================

    /// Generate mission key: tt:{town}:mission:{run_id}
    fn mission_key(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}", self.town_name, id)
    }

    /// Generate work items key: tt:{town}:mission:{run_id}:work
    fn work_key(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:work", self.town_name, id)
    }

    /// Generate watch items key: tt:{town}:mission:{run_id}:watch
    fn watch_key(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:watch", self.town_name, id)
    }

    /// Generate events key: tt:{town}:mission:{run_id}:events
    fn events_key(&self, id: MissionId) -> String {
        format!("tt:{}:mission:{}:events", self.town_name, id)
    }

    /// Generate active missions set key: tt:{town}:mission:active
    fn active_key(&self) -> String {
        format!("tt:{}:mission:active", self.town_name)
    }

    /// Generate mission key pattern for scanning.
    fn mission_pattern(&self) -> String {
        format!("tt:{}:mission:*", self.town_name)
    }

    // ==================== MissionRun Operations ====================

    /// Save a mission run to Redis.
    #[instrument(skip(self, mission), fields(mission_id = %mission.id))]
    pub async fn save_mission(&self, mission: &MissionRun) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.mission_key(mission.id);

        // Serialize to JSON for now (can optimize to hash fields later)
        let json = serde_json::to_string(mission)?;
        let _: () = conn.set(&key, &json).await?;

        debug!("Saved mission {}", mission.id);
        Ok(())
    }

    /// Get a mission run by ID.
    #[instrument(skip(self))]
    pub async fn get_mission(&self, id: MissionId) -> Result<Option<MissionRun>> {
        let mut conn = self.conn.clone();
        let key = self.mission_key(id);

        let json: Option<String> = conn.get(&key).await?;
        match json {
            Some(data) => {
                let mission: MissionRun = serde_json::from_str(&data)?;
                Ok(Some(mission))
            }
            None => Ok(None),
        }
    }

    /// Delete a mission and all its related data.
    #[instrument(skip(self))]
    pub async fn delete_mission(&self, id: MissionId) -> Result<bool> {
        let mut conn = self.conn.clone();

        // Delete all related keys
        let keys = vec![
            self.mission_key(id),
            self.work_key(id),
            self.watch_key(id),
            self.events_key(id),
        ];

        let deleted: i64 = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await?;

        // Remove from active set
        let _: () = conn.srem(self.active_key(), id.to_string()).await?;

        debug!("Deleted mission {} ({} keys)", id, deleted);
        Ok(deleted > 0)
    }

    // ==================== Active Missions ====================

    /// Add a mission to the active set.
    #[instrument(skip(self))]
    pub async fn add_active(&self, id: MissionId) -> Result<()> {
        let mut conn = self.conn.clone();
        let _: () = conn.sadd(self.active_key(), id.to_string()).await?;
        debug!("Added mission {} to active set", id);
        Ok(())
    }

    /// Remove a mission from the active set.
    #[instrument(skip(self))]
    pub async fn remove_active(&self, id: MissionId) -> Result<()> {
        let mut conn = self.conn.clone();
        let _: () = conn.srem(self.active_key(), id.to_string()).await?;
        debug!("Removed mission {} from active set", id);
        Ok(())
    }

    /// Get all active mission IDs.
    #[instrument(skip(self))]
    pub async fn list_active(&self) -> Result<Vec<MissionId>> {
        let mut conn = self.conn.clone();
        let ids: Vec<String> = conn.smembers(self.active_key()).await?;

        let mut missions = Vec::new();
        for id_str in ids {
            if let Ok(id) = id_str.parse() {
                missions.push(id);
            }
        }
        Ok(missions)
    }

    // ==================== WorkItem Operations ====================

    /// Save a work item.
    #[instrument(skip(self, item), fields(work_id = %item.id))]
    pub async fn save_work_item(&self, item: &WorkItem) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.work_key(item.mission_id);

        let json = serde_json::to_string(item)?;
        let _: () = conn.hset(&key, item.id.to_string(), &json).await?;

        debug!("Saved work item {}", item.id);
        Ok(())
    }

    /// Get a work item by ID.
    #[instrument(skip(self))]
    pub async fn get_work_item(
        &self,
        mission_id: MissionId,
        id: WorkItemId,
    ) -> Result<Option<WorkItem>> {
        let mut conn = self.conn.clone();
        let key = self.work_key(mission_id);

        let json: Option<String> = conn.hget(&key, id.to_string()).await?;
        match json {
            Some(data) => {
                let item: WorkItem = serde_json::from_str(&data)?;
                Ok(Some(item))
            }
            None => Ok(None),
        }
    }

    /// Get all work items for a mission.
    #[instrument(skip(self))]
    pub async fn list_work_items(&self, mission_id: MissionId) -> Result<Vec<WorkItem>> {
        let mut conn = self.conn.clone();
        let key = self.work_key(mission_id);

        let items: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;

        let mut work_items = Vec::new();
        for (_id, json) in items {
            if let Ok(item) = serde_json::from_str::<WorkItem>(&json) {
                work_items.push(item);
            }
        }
        Ok(work_items)
    }

    /// Delete a work item.
    #[instrument(skip(self))]
    pub async fn delete_work_item(&self, mission_id: MissionId, id: WorkItemId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.work_key(mission_id);

        let deleted: i64 = conn.hdel(&key, id.to_string()).await?;
        debug!("Deleted work item {}", id);
        Ok(deleted > 0)
    }

    // ==================== WatchItem Operations ====================

    /// Save a watch item.
    #[instrument(skip(self, item), fields(watch_id = %item.id))]
    pub async fn save_watch_item(&self, item: &WatchItem) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.watch_key(item.mission_id);

        let json = serde_json::to_string(item)?;
        let _: () = conn.hset(&key, item.id.to_string(), &json).await?;

        debug!("Saved watch item {}", item.id);
        Ok(())
    }

    /// Get a watch item by ID.
    #[instrument(skip(self))]
    pub async fn get_watch_item(
        &self,
        mission_id: MissionId,
        id: WatchId,
    ) -> Result<Option<WatchItem>> {
        let mut conn = self.conn.clone();
        let key = self.watch_key(mission_id);

        let json: Option<String> = conn.hget(&key, id.to_string()).await?;
        match json {
            Some(data) => {
                let item: WatchItem = serde_json::from_str(&data)?;
                Ok(Some(item))
            }
            None => Ok(None),
        }
    }

    /// Get all watch items for a mission.
    #[instrument(skip(self))]
    pub async fn list_watch_items(&self, mission_id: MissionId) -> Result<Vec<WatchItem>> {
        let mut conn = self.conn.clone();
        let key = self.watch_key(mission_id);

        let items: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;

        let mut watch_items = Vec::new();
        for (_id, json) in items {
            if let Ok(item) = serde_json::from_str::<WatchItem>(&json) {
                watch_items.push(item);
            }
        }
        Ok(watch_items)
    }

    /// Get due watch items across all active missions.
    #[instrument(skip(self))]
    pub async fn list_due_watches(&self) -> Result<Vec<WatchItem>> {
        let active_ids = self.list_active().await?;
        let mut due_watches = Vec::new();

        for mission_id in active_ids {
            let watches = self.list_watch_items(mission_id).await?;
            for watch in watches {
                if watch.is_due() {
                    due_watches.push(watch);
                }
            }
        }
        Ok(due_watches)
    }

    /// Delete a watch item.
    #[instrument(skip(self))]
    pub async fn delete_watch_item(&self, mission_id: MissionId, id: WatchId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.watch_key(mission_id);

        let deleted: i64 = conn.hdel(&key, id.to_string()).await?;
        debug!("Deleted watch item {}", id);
        Ok(deleted > 0)
    }

    // ==================== Events ====================

    /// Log an event for a mission.
    #[instrument(skip(self, event))]
    pub async fn log_event(&self, mission_id: MissionId, event: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.events_key(mission_id);

        // Add timestamp to event
        let timestamped = format!("[{}] {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), event);

        // Push to list, trim to max
        let _: () = conn.lpush(&key, &timestamped).await?;
        let _: () = conn.ltrim(&key, 0, MAX_EVENTS - 1).await?;

        debug!("Logged event for mission {}", mission_id);
        Ok(())
    }

    /// Get recent events for a mission.
    #[instrument(skip(self))]
    pub async fn get_events(&self, mission_id: MissionId, count: isize) -> Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let key = self.events_key(mission_id);

        let events: Vec<String> = conn.lrange(&key, 0, count - 1).await?;
        Ok(events)
    }

    // ==================== Bulk Operations ====================

    /// List all missions (active and inactive).
    #[instrument(skip(self))]
    pub async fn list_all_missions(&self) -> Result<Vec<MissionRun>> {
        let mut conn = self.conn.clone();
        let pattern = self.mission_pattern();

        // Find mission keys (not sub-keys like :work, :watch, :events)
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        let mut missions = Vec::new();
        for key in keys {
            // Skip sub-keys
            if key.contains(":work") || key.contains(":watch")
                || key.contains(":events") || key.contains(":active") {
                continue;
            }

            let json: Option<String> = conn.get(&key).await?;
            if let Some(data) = json
                && let Ok(mission) = serde_json::from_str::<MissionRun>(&data)
            {
                missions.push(mission);
            }
        }
        Ok(missions)
    }
}
