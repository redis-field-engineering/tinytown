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
use redis::Script;
use redis::aio::ConnectionManager;
use tracing::{debug, instrument};

use crate::error::Result;
use crate::events::{EventStream, EventType, TownEvent};
use crate::keys::RedisKeys;
use crate::mission::types::{
    MissionControlMessage, MissionId, MissionRun, WatchId, WatchItem, WorkItem, WorkItemId,
};

/// Maximum events to keep in the activity log.
const MAX_EVENTS: isize = 100;

/// Mission storage operations.
///
/// Wraps a Redis connection with town-namespaced key generation.
/// Optionally holds an `EventStream` for emitting structured events
/// to Redis Streams alongside the existing bounded List activity log.
#[derive(Clone)]
pub struct MissionStorage {
    conn: ConnectionManager,
    keys: RedisKeys,
    event_stream: Option<EventStream>,
}

impl MissionStorage {
    /// Create a new MissionStorage.
    pub fn new(conn: ConnectionManager, town_name: impl Into<String>) -> Self {
        let town = town_name.into();
        let event_stream = EventStream::new(conn.clone(), &town);
        Self {
            conn,
            keys: RedisKeys::new(town),
            event_stream: Some(event_stream),
        }
    }

    /// Get a reference to the event stream (if available).
    pub fn event_stream(&self) -> Option<&EventStream> {
        self.event_stream.as_ref()
    }

    // ==================== Key Generation (delegates to RedisKeys) ====================

    fn mission_key(&self, id: MissionId) -> String {
        self.keys.mission(id)
    }

    fn work_key(&self, id: MissionId) -> String {
        self.keys.mission_work(id)
    }

    fn watch_key(&self, id: MissionId) -> String {
        self.keys.mission_watch(id)
    }

    fn events_key(&self, id: MissionId) -> String {
        self.keys.mission_events(id)
    }

    fn control_key(&self, id: MissionId) -> String {
        self.keys.mission_control(id)
    }

    fn active_key(&self) -> String {
        self.keys.mission_active()
    }

    fn dispatch_lock_key(&self, id: MissionId) -> String {
        self.keys.mission_dispatch_lock(id)
    }

    fn mission_pattern(&self) -> String {
        self.keys.pattern_missions()
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
            self.control_key(id),
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

    /// Try to acquire a dispatcher lease for a mission.
    #[instrument(skip(self))]
    pub async fn try_acquire_dispatch_lock(
        &self,
        mission_id: MissionId,
        owner_token: &str,
        ttl_secs: u64,
    ) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.dispatch_lock_key(mission_id);
        let acquired: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(owner_token)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await?;
        Ok(acquired.is_some())
    }

    /// Refresh a dispatcher lease if still owned by the given token.
    #[instrument(skip(self))]
    pub async fn refresh_dispatch_lock(
        &self,
        mission_id: MissionId,
        owner_token: &str,
        ttl_secs: u64,
    ) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.dispatch_lock_key(mission_id);
        let script = Script::new(
            r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
  redis.call("EXPIRE", KEYS[1], ARGV[2])
  return 1
end
return 0
"#,
        );
        let refreshed: i32 = script
            .key(&key)
            .arg(owner_token)
            .arg(ttl_secs)
            .invoke_async(&mut conn)
            .await?;
        Ok(refreshed == 1)
    }

    /// Release a dispatcher lease if still owned by the given token.
    #[instrument(skip(self))]
    pub async fn release_dispatch_lock(
        &self,
        mission_id: MissionId,
        owner_token: &str,
    ) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.dispatch_lock_key(mission_id);
        let script = Script::new(
            r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
  return redis.call("DEL", KEYS[1])
end
return 0
"#,
        );
        let released: i32 = script
            .key(&key)
            .arg(owner_token)
            .invoke_async(&mut conn)
            .await?;
        Ok(released == 1)
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

    /// Get the number of work items for a mission without loading their payloads.
    #[instrument(skip(self))]
    pub async fn count_work_items(&self, mission_id: MissionId) -> Result<usize> {
        let mut conn = self.conn.clone();
        let key = self.work_key(mission_id);
        Ok(conn.hlen(&key).await?)
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

    /// Get the number of watch items for a mission without loading their payloads.
    #[instrument(skip(self))]
    pub async fn count_watch_items(&self, mission_id: MissionId) -> Result<usize> {
        let mut conn = self.conn.clone();
        let key = self.watch_key(mission_id);
        Ok(conn.hlen(&key).await?)
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

    // ==================== Control Message Operations ====================

    /// Save a control message.
    #[instrument(skip(self, message), fields(mission_id = %message.mission_id))]
    pub async fn save_control_message(&self, message: &MissionControlMessage) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.control_key(message.mission_id);
        let json = serde_json::to_string(message)?;
        let _: () = conn.hset(&key, &message.id, json).await?;
        Ok(())
    }

    /// List all control messages for a mission.
    #[instrument(skip(self))]
    pub async fn list_control_messages(
        &self,
        mission_id: MissionId,
    ) -> Result<Vec<MissionControlMessage>> {
        let mut conn = self.conn.clone();
        let key = self.control_key(mission_id);
        let messages: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;

        let mut control_messages = Vec::new();
        for (_id, json) in messages {
            if let Ok(message) = serde_json::from_str::<MissionControlMessage>(&json) {
                control_messages.push(message);
            }
        }
        control_messages.sort_by_key(|message| message.created_at);
        Ok(control_messages)
    }

    /// List pending control messages for a mission.
    #[instrument(skip(self))]
    pub async fn list_pending_control_messages(
        &self,
        mission_id: MissionId,
    ) -> Result<Vec<MissionControlMessage>> {
        Ok(self
            .list_control_messages(mission_id)
            .await?
            .into_iter()
            .filter(MissionControlMessage::is_pending)
            .collect())
    }

    // ==================== Events ====================

    /// Log an event for a mission.
    ///
    /// Writes to both the bounded List (backward compat) and the Redis Stream
    /// (real-time feed) when an EventStream is available.
    #[instrument(skip(self, event))]
    pub async fn log_event(&self, mission_id: MissionId, event: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.events_key(mission_id);

        // Add timestamp to event
        let timestamped = format!(
            "[{}] {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            event
        );

        // Push to list, trim to max (backward compat)
        let _: () = conn.lpush(&key, &timestamped).await?;
        let _: () = conn.ltrim(&key, 0, MAX_EVENTS - 1).await?;

        // Also emit to Redis Stream for real-time consumption
        if let Some(ref es) = self.event_stream {
            let town_event =
                TownEvent::new(EventType::MissionEvent, event).with_mission(mission_id);
            // Best-effort: don't fail the whole operation if stream emit fails
            if let Err(e) = es.emit(&town_event).await {
                debug!("Failed to emit stream event: {}", e);
            }
        }

        debug!("Logged event for mission {}", mission_id);
        Ok(())
    }

    /// Emit a typed event to the Redis Stream (and also log to the bounded list).
    ///
    /// Use this for structured events with full metadata (agent_id, task_id, etc.).
    /// Falls back gracefully if no EventStream is configured.
    #[instrument(skip(self, event))]
    pub async fn emit_event(&self, mission_id: MissionId, event: TownEvent) -> Result<()> {
        // Log to bounded list for backward compat
        self.log_event(mission_id, &event.message).await?;

        // Emit typed event to stream
        if let Some(ref es) = self.event_stream
            && let Err(e) = es.emit(&event).await
        {
            debug!("Failed to emit typed stream event: {}", e);
        }
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
            if key.contains(":work")
                || key.contains(":watch")
                || key.contains(":events")
                || key.contains(":control")
                || key.contains(":dispatch_lock")
                || key.contains(":active")
            {
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
