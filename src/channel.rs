/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Redis-based message passing channels.
//!
//! Uses Redis Lists for reliable message queues and Pub/Sub for broadcasts.

use std::time::Duration;

use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use tracing::{debug, instrument};

use crate::agent::AgentId;
use crate::error::Result;
use crate::events::{EventStream, TownEvent};
use crate::keys::RedisKeys;
use crate::message::{Message, Priority};

/// TTL for activity logs (1 hour)
const ACTIVITY_TTL_SECS: u64 = 3600;

/// Max activity entries per agent
const ACTIVITY_MAX_ENTRIES: isize = 10;

/// Redis-based communication channel.
///
/// All Redis keys are namespaced by town name to ensure isolation when
/// multiple towns share the same Redis instance.
/// Key format: `tt:{town_name}:{type}:{id}`
#[derive(Clone)]
pub struct Channel {
    conn: ConnectionManager,
    /// Centralized key generator for this town.
    keys: RedisKeys,
}

impl Channel {
    /// Create a new channel from a Redis connection with town namespacing.
    ///
    /// All Redis keys will be prefixed with `tt:{town_name}:` to ensure
    /// isolation between different towns sharing the same Redis instance.
    pub fn new(conn: ConnectionManager, town_name: impl Into<String>) -> Self {
        let town_name = town_name.into();
        Self {
            conn,
            keys: RedisKeys::new(town_name),
        }
    }

    /// Get the town name for this channel.
    pub fn town_name(&self) -> &str {
        self.keys.town_name()
    }

    /// Get a clone of the underlying Redis connection manager.
    ///
    /// This is useful for creating specialized storage layers (like MissionStorage)
    /// that need direct Redis access while maintaining the same connection pool.
    pub fn conn(&self) -> &ConnectionManager {
        &self.conn
    }

    /// Get a reference to the Redis key generator.
    pub fn keys(&self) -> &RedisKeys {
        &self.keys
    }

    // ==================== Key Generation (delegates to RedisKeys) ====================

    fn inbox_key(&self, agent_id: AgentId) -> String {
        self.keys.agent_inbox(agent_id)
    }

    fn urgent_key(&self, agent_id: AgentId) -> String {
        self.keys.agent_urgent(agent_id)
    }

    fn state_key(&self, agent_id: AgentId) -> String {
        self.keys.agent_state(agent_id)
    }

    fn task_key(&self, task_id: crate::task::TaskId) -> String {
        self.keys.task(task_id)
    }

    fn activity_key(&self, agent_id: AgentId) -> String {
        self.keys.agent_activity(agent_id)
    }

    fn stop_key(&self, agent_id: AgentId) -> String {
        self.keys.agent_stop(agent_id)
    }

    fn backlog_key(&self) -> String {
        self.keys.backlog()
    }

    fn docket_tasks_key(&self) -> String {
        self.keys.docket_tasks()
    }

    fn docket_events_key(&self) -> String {
        self.keys.docket_events()
    }

    fn broadcast_channel(&self) -> String {
        self.keys.broadcast()
    }

    fn town_key_pattern(&self) -> String {
        self.keys.pattern_all()
    }

    fn agent_key_pattern(&self) -> String {
        self.keys.pattern_agents()
    }

    fn inbox_key_pattern(&self) -> String {
        self.keys.pattern_inboxes()
    }

    fn stop_key_pattern(&self) -> String {
        self.keys.pattern_stops()
    }

    fn activity_key_pattern(&self) -> String {
        self.keys.pattern_activities()
    }

    fn urgent_key_pattern(&self) -> String {
        self.keys.pattern_urgents()
    }

    fn task_key_pattern(&self) -> String {
        self.keys.pattern_tasks()
    }

    /// Send a message to an agent's inbox.
    #[instrument(skip(self, message), fields(to = %message.to, msg_type = ?message.msg_type))]
    pub async fn send(&self, message: &Message) -> Result<()> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(message.to);
        let serialized = serde_json::to_string(message)?;

        // Use priority queues: high priority goes to front
        match message.priority {
            Priority::Urgent | Priority::High => {
                let _: () = conn.lpush(&inbox_key, &serialized).await?;
            }
            Priority::Normal | Priority::Low => {
                let _: () = conn.rpush(&inbox_key, &serialized).await?;
            }
        }

        debug!("Sent message {} to {}", message.id, message.to);
        Ok(())
    }

    /// Send an urgent message to an agent's priority inbox.
    ///
    /// Urgent messages are checked before regular inbox at the start of each round.
    #[instrument(skip(self, message))]
    pub async fn send_urgent(&self, message: &Message) -> Result<()> {
        let mut conn = self.conn.clone();
        let urgent_key = self.urgent_key(message.to);

        let data = serde_json::to_string(message)?;
        let _: () = conn.lpush(&urgent_key, &data).await?;

        debug!("Sent URGENT message {} to {}", message.id, message.to);
        Ok(())
    }

    /// Check and receive urgent messages (non-blocking).
    ///
    /// Returns all urgent messages, emptying the urgent inbox.
    #[instrument(skip(self))]
    pub async fn receive_urgent(&self, agent_id: AgentId) -> Result<Vec<Message>> {
        let mut conn = self.conn.clone();
        let urgent_key = self.urgent_key(agent_id);

        let mut messages = Vec::new();
        loop {
            let result: Option<String> = conn.lpop(&urgent_key, None).await?;
            match result {
                Some(data) => {
                    let message: Message = serde_json::from_str(&data)?;
                    messages.push(message);
                }
                None => break,
            }
        }

        if !messages.is_empty() {
            debug!(
                "Received {} urgent messages for {}",
                messages.len(),
                agent_id
            );
        }
        Ok(messages)
    }

    /// Check urgent inbox length.
    pub async fn urgent_len(&self, agent_id: AgentId) -> Result<usize> {
        let mut conn = self.conn.clone();
        let urgent_key = self.urgent_key(agent_id);
        let len: usize = conn.llen(&urgent_key).await?;
        Ok(len)
    }

    /// Request an agent to stop gracefully.
    ///
    /// Sets a stop flag that the agent checks at the start of each round.
    #[instrument(skip(self))]
    pub async fn request_stop(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let stop_key = self.stop_key(agent_id);

        // Set flag with 1-hour TTL (cleanup if agent already dead)
        let _: () = conn.set_ex(&stop_key, "1", 3600).await?;

        debug!("Requested stop for agent {}", agent_id);
        Ok(())
    }

    /// Check if stop has been requested for an agent.
    #[instrument(skip(self))]
    pub async fn should_stop(&self, agent_id: AgentId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let stop_key = self.stop_key(agent_id);

        let exists: bool = conn.exists(&stop_key).await?;
        Ok(exists)
    }

    /// Clear the stop flag (called when agent stops).
    pub async fn clear_stop(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let stop_key = self.stop_key(agent_id);

        let _: () = conn.del(&stop_key).await?;
        Ok(())
    }

    /// Receive a message from an agent's inbox (blocking with timeout).
    #[instrument(skip(self))]
    pub async fn receive(&self, agent_id: AgentId, timeout: Duration) -> Result<Option<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(agent_id);

        let result: Option<String> = conn.blpop(&inbox_key, timeout.as_secs_f64()).await?;

        match result {
            Some(data) => {
                let message: Message = serde_json::from_str(&data)?;
                debug!("Received message {} from inbox", message.id);
                Ok(Some(message))
            }
            None => Ok(None),
        }
    }

    /// Receive a message without blocking.
    pub async fn try_receive(&self, agent_id: AgentId) -> Result<Option<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(agent_id);

        let result: Option<String> = conn.lpop(&inbox_key, None).await?;

        match result {
            Some(data) => {
                let message: Message = serde_json::from_str(&data)?;
                Ok(Some(message))
            }
            None => Ok(None),
        }
    }

    /// Get the number of messages in an agent's inbox.
    pub async fn inbox_len(&self, agent_id: AgentId) -> Result<usize> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(agent_id);
        let len: usize = conn.llen(&inbox_key).await?;
        Ok(len)
    }

    /// Peek at inbox messages without removing them.
    pub async fn peek_inbox(&self, agent_id: AgentId, count: isize) -> Result<Vec<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(agent_id);
        let items: Vec<String> = conn.lrange(&inbox_key, 0, count - 1).await?;

        let mut messages = Vec::new();
        for item in items {
            if let Ok(msg) = serde_json::from_str::<Message>(&item) {
                messages.push(msg);
            }
        }
        Ok(messages)
    }

    /// Broadcast a message to all agents.
    pub async fn broadcast(&self, message: &Message) -> Result<()> {
        let mut conn = self.conn.clone();
        let serialized = serde_json::to_string(message)?;
        let broadcast_channel = self.broadcast_channel();
        let _: () = conn.publish(broadcast_channel, &serialized).await?;
        Ok(())
    }

    /// Store agent state in Redis using Hash for atomic field updates.
    ///
    /// Uses a Redis pipeline to atomically perform HDEL (for None fields) and HSET
    /// operations, preventing race conditions where concurrent readers might see
    /// partially-updated state.
    pub async fn set_agent_state(&self, agent: &crate::agent::Agent) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent.id);

        // Convert agent fields to hash entries
        let mut fields: Vec<(String, String)> = vec![
            ("id".to_string(), agent.id.to_string()),
            ("name".to_string(), agent.name.clone()),
            (
                "agent_type".to_string(),
                serde_json::to_string(&agent.agent_type)?
                    .trim_matches('"')
                    .to_string(),
            ),
            (
                "state".to_string(),
                serde_json::to_string(&agent.state)?
                    .trim_matches('"')
                    .to_string(),
            ),
            ("cli".to_string(), agent.cli.clone()),
            ("created_at".to_string(), agent.created_at.to_rfc3339()),
            (
                "last_heartbeat".to_string(),
                agent.last_heartbeat.to_rfc3339(),
            ),
            (
                "tasks_completed".to_string(),
                agent.tasks_completed.to_string(),
            ),
            (
                "rounds_completed".to_string(),
                agent.rounds_completed.to_string(),
            ),
            (
                "last_active_at".to_string(),
                agent.last_active_at.to_rfc3339(),
            ),
            (
                "spawn_mode".to_string(),
                serde_json::to_string(&agent.spawn_mode)?
                    .trim_matches('"')
                    .to_string(),
            ),
        ];

        // Collect fields to delete when None
        let mut fields_to_delete: Vec<&str> = Vec::new();

        // Handle optional current_task
        if let Some(ref task_id) = agent.current_task {
            fields.push(("current_task".to_string(), task_id.to_string()));
        } else {
            fields_to_delete.push("current_task");
        }

        // Handle optional nickname
        if let Some(ref nickname) = agent.nickname {
            fields.push(("nickname".to_string(), nickname.clone()));
        } else {
            fields_to_delete.push("nickname");
        }

        // Handle optional role_id
        if let Some(ref role_id) = agent.role_id {
            fields.push(("role_id".to_string(), role_id.clone()));
        } else {
            fields_to_delete.push("role_id");
        }

        // Handle optional parent_agent_id
        if let Some(ref parent_id) = agent.parent_agent_id {
            fields.push(("parent_agent_id".to_string(), parent_id.to_string()));
        } else {
            fields_to_delete.push("parent_agent_id");
        }

        // Use a pipeline to make HDEL + HSET atomic
        let mut pipe = redis::pipe();
        if !fields_to_delete.is_empty() {
            pipe.hdel(&key, &fields_to_delete);
        }
        pipe.hset_multiple(&key, &fields);
        let _: () = pipe.query_async(&mut conn).await?;

        Ok(())
    }

    /// Get agent state from Redis using Hash.
    ///
    /// Uses HGETALL to retrieve all agent fields from the Hash.
    pub async fn get_agent_state(&self, agent_id: AgentId) -> Result<Option<crate::agent::Agent>> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent_id);

        let fields: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;

        if fields.is_empty() {
            return Ok(None);
        }

        // Parse fields back into Agent struct
        let agent = Self::parse_agent_from_hash(fields)?;
        Ok(Some(agent))
    }

    /// Parse an Agent from a Redis Hash field map.
    fn parse_agent_from_hash(
        fields: std::collections::HashMap<String, String>,
    ) -> Result<crate::agent::Agent> {
        use chrono::DateTime;

        let id: AgentId = fields
            .get("id")
            .ok_or_else(|| crate::error::Error::AgentNotFound("Missing id field".to_string()))?
            .parse()
            .map_err(|e| crate::error::Error::AgentNotFound(format!("Invalid agent id: {}", e)))?;

        let name = fields.get("name").cloned().unwrap_or_default();

        let agent_type: crate::agent::AgentType = fields
            .get("agent_type")
            .map(|s| serde_json::from_str(&format!("\"{}\"", s)).unwrap_or_default())
            .unwrap_or_default();

        let state: crate::agent::AgentState = fields
            .get("state")
            .map(|s| serde_json::from_str(&format!("\"{}\"", s)).unwrap_or_default())
            .unwrap_or_default();

        let cli = fields.get("cli").cloned().unwrap_or_default();

        let current_task = fields.get("current_task").and_then(|s| s.parse().ok());

        let created_at = fields
            .get("created_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let last_heartbeat = fields
            .get("last_heartbeat")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let tasks_completed = fields
            .get("tasks_completed")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let rounds_completed = fields
            .get("rounds_completed")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let last_active_at = fields
            .get("last_active_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or(created_at); // Default to created_at for backward compat

        let nickname = fields.get("nickname").cloned();
        let role_id = fields.get("role_id").cloned();
        let parent_agent_id = fields.get("parent_agent_id").and_then(|s| s.parse().ok());
        let spawn_mode: crate::agent::SpawnMode = fields
            .get("spawn_mode")
            .map(|s| serde_json::from_str(&format!("\"{}\"", s)).unwrap_or_default())
            .unwrap_or_default();

        Ok(crate::agent::Agent {
            id,
            name,
            nickname,
            role_id,
            parent_agent_id,
            spawn_mode,
            agent_type,
            state,
            cli,
            current_task,
            created_at,
            last_heartbeat,
            tasks_completed,
            rounds_completed,
            last_active_at,
        })
    }

    /// List all agents from Redis.
    pub async fn list_agents(&self) -> Result<Vec<crate::agent::Agent>> {
        let mut conn = self.conn.clone();
        let pattern = self.agent_key_pattern();
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        let mut agents = Vec::new();
        for key in keys {
            let fields: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;
            if !fields.is_empty()
                && let Ok(agent) = Self::parse_agent_from_hash(fields)
            {
                agents.push(agent);
            }
        }
        Ok(agents)
    }

    /// Get agent by name from Redis.
    pub async fn get_agent_by_name(&self, name: &str) -> Result<Option<crate::agent::Agent>> {
        let agents = self.list_agents().await?;
        Ok(agents.into_iter().find(|a| a.name == name))
    }

    /// Delete an agent from Redis.
    pub async fn delete_agent(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent_id);
        let _: () = conn.del(&key).await?;
        // Also clean up related keys
        let inbox_key = self.inbox_key(agent_id);
        let urgent_key = self.urgent_key(agent_id);
        let activity_key = self.activity_key(agent_id);
        let stop_key = self.stop_key(agent_id);
        let _: () = conn.del(&inbox_key).await?;
        let _: () = conn.del(&urgent_key).await?;
        let _: () = conn.del(&activity_key).await?;
        let _: () = conn.del(&stop_key).await?;
        Ok(())
    }

    /// Atomically increment an agent's rounds_completed counter.
    ///
    /// Uses HINCRBY for atomic increment without read-modify-write race conditions.
    /// Per Redis best practices, this avoids rewriting the entire agent object.
    ///
    /// Note: Reserved for future use when we optimize agent state updates.
    #[allow(dead_code)]
    #[instrument(skip(self))]
    pub async fn increment_agent_rounds(&self, agent_id: AgentId) -> Result<u64> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent_id);
        let new_value: i64 = conn.hincr(&key, "rounds_completed", 1).await?;
        debug!(
            "Agent {} rounds_completed incremented to {}",
            agent_id, new_value
        );
        Ok(new_value as u64)
    }

    /// Atomically increment an agent's tasks_completed counter.
    ///
    /// Uses HINCRBY for atomic increment without read-modify-write race conditions.
    ///
    /// Note: Reserved for future use when we optimize agent state updates.
    #[allow(dead_code)]
    #[instrument(skip(self))]
    pub async fn increment_agent_tasks_completed(&self, agent_id: AgentId) -> Result<u64> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent_id);
        let new_value: i64 = conn.hincr(&key, "tasks_completed", 1).await?;
        debug!(
            "Agent {} tasks_completed incremented to {}",
            agent_id, new_value
        );
        Ok(new_value as u64)
    }

    /// Atomically update an agent's heartbeat timestamp.
    ///
    /// Uses HSET on a single field for efficient heartbeat updates.
    ///
    /// Note: Reserved for future use when we optimize heartbeat updates.
    #[allow(dead_code)]
    #[instrument(skip(self))]
    pub async fn update_agent_heartbeat(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.state_key(agent_id);
        let now = chrono::Utc::now().to_rfc3339();
        let _: () = conn.hset(&key, "last_heartbeat", &now).await?;
        Ok(())
    }

    /// Store a task in Redis using Hash for atomic field updates.
    ///
    /// Uses a Redis pipeline to atomically perform HDEL (for None fields) and HSET
    /// operations, preventing race conditions where concurrent readers might see
    /// partially-updated state.
    pub async fn set_task(&self, task: &crate::task::Task) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.task_key(task.id);

        // Convert task fields to hash entries
        let mut fields: Vec<(String, String)> = vec![
            ("id".to_string(), task.id.to_string()),
            ("description".to_string(), task.description.clone()),
            (
                "state".to_string(),
                serde_json::to_string(&task.state)?
                    .trim_matches('"')
                    .to_string(),
            ),
            ("created_at".to_string(), task.created_at.to_rfc3339()),
            ("updated_at".to_string(), task.updated_at.to_rfc3339()),
            ("tags".to_string(), serde_json::to_string(&task.tags)?),
        ];

        // Collect fields to delete when None
        let mut fields_to_delete: Vec<&str> = Vec::new();

        // Handle optional fields
        if let Some(ref agent_id) = task.assigned_to {
            fields.push(("assigned_to".to_string(), agent_id.to_string()));
        } else {
            fields_to_delete.push("assigned_to");
        }
        if let Some(ref started_at) = task.started_at {
            fields.push(("started_at".to_string(), started_at.to_rfc3339()));
        } else {
            fields_to_delete.push("started_at");
        }
        if let Some(ref completed_at) = task.completed_at {
            fields.push(("completed_at".to_string(), completed_at.to_rfc3339()));
        } else {
            fields_to_delete.push("completed_at");
        }
        if let Some(ref result) = task.result {
            fields.push(("result".to_string(), result.clone()));
        } else {
            fields_to_delete.push("result");
        }
        if let Some(ref parent_id) = task.parent_id {
            fields.push(("parent_id".to_string(), parent_id.to_string()));
        } else {
            fields_to_delete.push("parent_id");
        }

        // Use a pipeline to make HDEL + HSET atomic
        let mut pipe = redis::pipe();
        if !fields_to_delete.is_empty() {
            pipe.hdel(&key, &fields_to_delete);
        }
        pipe.hset_multiple(&key, &fields);
        let _: () = pipe.query_async(&mut conn).await?;

        Ok(())
    }

    /// Get a task from Redis using Hash.
    ///
    /// Uses HGETALL to retrieve all task fields from the Hash.
    pub async fn get_task(
        &self,
        task_id: crate::task::TaskId,
    ) -> Result<Option<crate::task::Task>> {
        let mut conn = self.conn.clone();
        let key = self.task_key(task_id);

        let fields: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;

        if fields.is_empty() {
            return Ok(None);
        }

        // Parse fields back into Task struct
        let task = Self::parse_task_from_hash(fields)?;
        Ok(Some(task))
    }

    /// Delete a task from Redis.
    ///
    /// Removes the task hash at `tt:{town}:task:{task_id}`.
    /// Returns true if the task existed and was deleted, false if it didn't exist.
    pub async fn delete_task(&self, task_id: crate::task::TaskId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let key = self.task_key(task_id);
        let deleted: i64 = conn.del(&key).await?;
        if deleted > 0 {
            debug!("Deleted task {}", task_id);
        }
        Ok(deleted > 0)
    }

    /// Parse a Task from a Redis Hash field map.
    fn parse_task_from_hash(
        fields: std::collections::HashMap<String, String>,
    ) -> Result<crate::task::Task> {
        use chrono::DateTime;

        let id: crate::task::TaskId = fields
            .get("id")
            .ok_or_else(|| crate::error::Error::TaskNotFound("Missing id field".to_string()))?
            .parse()
            .map_err(|e| crate::error::Error::TaskNotFound(format!("Invalid task id: {}", e)))?;

        let description = fields.get("description").cloned().unwrap_or_default();

        let state: crate::task::TaskState = fields
            .get("state")
            .map(|s| {
                // State is stored without quotes, so wrap it for JSON parsing
                serde_json::from_str(&format!("\"{}\"", s)).unwrap_or_default()
            })
            .unwrap_or_default();

        let created_at = fields
            .get("created_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = fields
            .get("updated_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let assigned_to = fields.get("assigned_to").and_then(|s| s.parse().ok());

        let started_at = fields
            .get("started_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let completed_at = fields
            .get("completed_at")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let result = fields.get("result").cloned();

        let parent_id = fields.get("parent_id").and_then(|s| s.parse().ok());

        let tags: Vec<String> = fields
            .get("tags")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        Ok(crate::task::Task {
            id,
            description,
            state,
            assigned_to,
            created_at,
            updated_at,
            started_at,
            completed_at,
            result,
            parent_id,
            tags,
        })
    }

    /// List all tasks in Redis.
    ///
    /// Scans all `tt:{town}:task:*` keys and returns the tasks.
    pub async fn list_tasks(&self) -> Result<Vec<crate::task::Task>> {
        let mut conn = self.conn.clone();
        let pattern = self.task_key_pattern();
        let mut tasks = Vec::new();

        // Use KEYS to find all task keys (consider SCAN for larger datasets)
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        for key in keys {
            let fields: std::collections::HashMap<String, String> = conn.hgetall(&key).await?;
            if !fields.is_empty()
                && let Ok(task) = Self::parse_task_from_hash(fields)
            {
                tasks.push(task);
            }
        }

        Ok(tasks)
    }

    /// Log agent activity (bounded, with TTL).
    ///
    /// Stores recent activity in Redis list, trimmed to ACTIVITY_MAX_ENTRIES.
    /// TTL ensures cleanup even if agent dies.
    #[instrument(skip(self, activity))]
    pub async fn log_agent_activity(&self, agent_id: AgentId, activity: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.activity_key(agent_id);

        // Prepend to list (newest first)
        let _: () = conn.lpush(&key, activity).await?;

        // Trim to max entries
        let _: () = conn.ltrim(&key, 0, ACTIVITY_MAX_ENTRIES - 1).await?;

        // Set/refresh TTL
        let _: () = conn.expire(&key, ACTIVITY_TTL_SECS as i64).await?;

        debug!("Logged activity for agent {}", agent_id);
        Ok(())
    }

    /// Get recent agent activity.
    ///
    /// Returns the last N activity entries, newest first.
    #[instrument(skip(self))]
    pub async fn get_agent_activity(&self, agent_id: AgentId) -> Result<Option<String>> {
        let mut conn = self.conn.clone();
        let key = self.activity_key(agent_id);

        let entries: Vec<String> = conn.lrange(&key, 0, 4).await?; // Get last 5

        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(entries.join("\n")))
        }
    }

    // ==================== Event Stream Methods ====================

    /// Create an EventStream for this channel's town.
    pub fn event_stream(&self) -> EventStream {
        EventStream::new(self.conn.clone(), self.town_name())
    }

    /// Emit a structured event to Redis Streams.
    ///
    /// Best-effort: logs a warning on failure but does not propagate errors,
    /// ensuring event emission never breaks core workflows.
    pub async fn emit_event(&self, event: &TownEvent) {
        let es = self.event_stream();
        if let Err(e) = es.emit(event).await {
            debug!("Failed to emit event: {}", e);
        }
    }

    // ==================== Backlog Methods ====================

    /// Add a task ID to the town's backlog queue.
    pub async fn backlog_push(&self, task_id: crate::task::TaskId) -> Result<()> {
        let mut conn = self.conn.clone();
        let backlog_key = self.backlog_key();
        let _: () = conn.rpush(&backlog_key, task_id.to_string()).await?;
        debug!("Added task {} to backlog", task_id);
        Ok(())
    }

    /// List all task IDs in the backlog.
    pub async fn backlog_list(&self) -> Result<Vec<crate::task::TaskId>> {
        let mut conn = self.conn.clone();
        let backlog_key = self.backlog_key();
        let items: Vec<String> = conn.lrange(&backlog_key, 0, -1).await?;

        let mut task_ids = Vec::new();
        for item in items {
            if let Ok(task_id) = item.parse() {
                task_ids.push(task_id);
            }
        }
        Ok(task_ids)
    }

    /// Get the number of tasks in the backlog.
    pub async fn backlog_len(&self) -> Result<usize> {
        let mut conn = self.conn.clone();
        let backlog_key = self.backlog_key();
        let len: usize = conn.llen(&backlog_key).await?;
        Ok(len)
    }

    /// Pop a task from the front of the backlog (FIFO).
    pub async fn backlog_pop(&self) -> Result<Option<crate::task::TaskId>> {
        let mut conn = self.conn.clone();
        let backlog_key = self.backlog_key();
        let result: Option<String> = conn.lpop(&backlog_key, None).await?;

        match result {
            Some(id) => {
                let task_id = id.parse().map_err(|e| {
                    crate::error::Error::TaskNotFound(format!("Invalid task ID: {}", e))
                })?;
                debug!("Popped task {} from backlog", task_id);
                Ok(Some(task_id))
            }
            None => Ok(None),
        }
    }

    /// Remove a specific task from the backlog.
    pub async fn backlog_remove(&self, task_id: crate::task::TaskId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let backlog_key = self.backlog_key();
        let removed: i64 = conn.lrem(&backlog_key, 1, task_id.to_string()).await?;
        if removed > 0 {
            debug!("Removed task {} from backlog", task_id);
        }
        Ok(removed > 0)
    }

    // ==================== Reclaim Methods ====================

    /// Drain all messages from an agent's inbox (non-blocking).
    ///
    /// Returns all messages that were in the inbox.
    pub async fn drain_inbox(&self, agent_id: AgentId) -> Result<Vec<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = self.inbox_key(agent_id);

        let mut messages = Vec::new();
        loop {
            let result: Option<String> = conn.lpop(&inbox_key, None).await?;
            match result {
                Some(data) => {
                    if let Ok(msg) = serde_json::from_str::<Message>(&data) {
                        messages.push(msg);
                    }
                }
                None => break,
            }
        }

        debug!(
            "Drained {} messages from agent {}",
            messages.len(),
            agent_id
        );
        Ok(messages)
    }

    /// Move a message to another agent's inbox.
    pub async fn move_message_to_inbox(&self, message: &Message, to_agent: AgentId) -> Result<()> {
        // Create a new message with updated recipient
        let mut new_msg = message.clone();
        new_msg.to = to_agent;
        self.send(&new_msg).await
    }

    // ==================== Reset Methods ====================

    /// Scan for keys matching a pattern using SCAN (production-safe, non-blocking).
    ///
    /// Unlike KEYS, SCAN is incremental and doesn't block the Redis server.
    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let mut cursor: u64 = 0;
        let mut all_keys = Vec::new();

        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await?;

            all_keys.extend(keys);
            cursor = next_cursor;

            if cursor == 0 {
                break;
            }
        }

        Ok(all_keys)
    }

    /// Delete all town state from Redis.
    ///
    /// This removes all agents, tasks, inboxes, activity logs, stop flags, and the backlog
    /// for this specific town only. Other towns sharing the same Redis instance are not affected.
    /// Returns the number of keys deleted.
    ///
    /// Uses SCAN instead of KEYS for production safety (non-blocking).
    pub async fn reset_all(&self) -> Result<usize> {
        let mut conn = self.conn.clone();

        // Find all tt:{town_name}:* keys using SCAN (production-safe)
        let pattern = self.town_key_pattern();
        let keys = self.scan_keys(&pattern).await?;

        if keys.is_empty() {
            return Ok(0);
        }

        let count = keys.len();

        // Delete all keys
        let _: () = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await?;

        debug!(
            "Reset: deleted {} keys for town '{}'",
            count,
            self.town_name()
        );
        Ok(count)
    }

    /// Delete only agent-related state from Redis.
    ///
    /// This removes agents and their inboxes, but preserves tasks and backlog.
    /// Only affects this town's agents; other towns are not affected.
    /// Returns the number of keys deleted.
    ///
    /// Uses SCAN instead of KEYS for production safety (non-blocking).
    pub async fn reset_agents_only(&self) -> Result<usize> {
        let mut conn = self.conn.clone();

        // Find agent and inbox keys using SCAN (production-safe)
        // All patterns are town-namespaced
        let mut keys = Vec::new();
        keys.extend(self.scan_keys(&self.agent_key_pattern()).await?);
        keys.extend(self.scan_keys(&self.inbox_key_pattern()).await?);
        keys.extend(self.scan_keys(&self.urgent_key_pattern()).await?);
        keys.extend(self.scan_keys(&self.stop_key_pattern()).await?);
        keys.extend(self.scan_keys(&self.activity_key_pattern()).await?);

        if keys.is_empty() {
            return Ok(0);
        }

        let count = keys.len();

        // Delete all agent-related keys
        let _: () = redis::cmd("DEL").arg(&keys).query_async(&mut conn).await?;

        debug!(
            "Reset agents only: deleted {} keys for town '{}'",
            count,
            self.town_name()
        );
        Ok(count)
    }

    // ==================== Docket Stream Methods ====================
    // Redis Streams-based task dispatch (Docket pattern from RAK/RAR).
    // Provides at-least-once delivery, crash recovery, and consumer groups.

    /// Consumer group name for the docket task stream.
    const DOCKET_GROUP: &'static str = "workers";

    /// Ensure the docket consumer group exists.
    ///
    /// Creates the stream and consumer group if they don't already exist.
    /// Uses `$ MKSTREAM` to start reading only new entries.
    pub async fn docket_ensure_group(&self) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        let result: redis::RedisResult<()> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&key)
            .arg(Self::DOCKET_GROUP)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut conn)
            .await;

        match result {
            Ok(()) => {
                debug!("Created docket consumer group on {}", key);
                Ok(())
            }
            Err(e) if e.to_string().contains("BUSYGROUP") => {
                // Group already exists — not an error
                debug!("Docket consumer group already exists on {}", key);
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Add a task to the docket stream (XADD).
    ///
    /// The stream entry format is RAK-compatible:
    /// ```text
    /// task_id, type, message, priority, from, to, timestamp, correlation_id
    /// ```
    ///
    /// Returns the stream entry ID assigned by Redis.
    pub async fn docket_push(
        &self,
        task_id: crate::task::TaskId,
        description: &str,
        priority: &str,
        from: &str,
        to: &str,
    ) -> Result<String> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();
        let now = chrono::Utc::now().to_rfc3339();

        let entry_id: String = redis::cmd("XADD")
            .arg(&key)
            .arg("*") // auto-generate ID
            .arg("task_id")
            .arg(task_id.to_string())
            .arg("type")
            .arg("task_assign")
            .arg("message")
            .arg(description)
            .arg("priority")
            .arg(priority)
            .arg("from")
            .arg(from)
            .arg("to")
            .arg(to)
            .arg("timestamp")
            .arg(&now)
            .query_async(&mut conn)
            .await?;

        debug!("Docket XADD task {} -> entry {}", task_id, entry_id);
        Ok(entry_id)
    }

    /// Read one task from the docket stream as a consumer (XREADGROUP).
    ///
    /// Reads the next undelivered entry from the consumer group.
    /// Returns `None` if no entries are available within the timeout.
    ///
    /// The returned tuple contains `(stream_entry_id, fields)`.
    pub async fn docket_read(
        &self,
        consumer_name: &str,
        block_ms: usize,
    ) -> Result<Option<(String, std::collections::HashMap<String, String>)>> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        // XREADGROUP GROUP workers <consumer> COUNT 1 BLOCK <ms> STREAMS <key> >
        let result: redis::Value = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(Self::DOCKET_GROUP)
            .arg(consumer_name)
            .arg("COUNT")
            .arg(1)
            .arg("BLOCK")
            .arg(block_ms)
            .arg("STREAMS")
            .arg(&key)
            .arg(">")
            .query_async(&mut conn)
            .await?;

        Self::parse_xread_single(result)
    }

    /// Acknowledge a docket task as processed (XACK).
    ///
    /// Must be called after successfully processing a task to remove it
    /// from the pending entries list (PEL).
    pub async fn docket_ack(&self, entry_id: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        let _: i64 = redis::cmd("XACK")
            .arg(&key)
            .arg(Self::DOCKET_GROUP)
            .arg(entry_id)
            .query_async(&mut conn)
            .await?;

        debug!("Docket XACK entry {}", entry_id);
        Ok(())
    }

    /// Get the number of entries in the docket task stream (XLEN).
    pub async fn docket_len(&self) -> Result<usize> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        let len: usize = redis::cmd("XLEN").arg(&key).query_async(&mut conn).await?;
        Ok(len)
    }

    /// Get pending (unacknowledged) entries in the docket stream (XPENDING summary).
    ///
    /// Returns the count of pending entries. Useful for crash recovery
    /// and visibility into in-flight work.
    pub async fn docket_pending_count(&self) -> Result<usize> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        // XPENDING returns [count, min-id, max-id, [[consumer, count], ...]]
        let result: redis::Value = redis::cmd("XPENDING")
            .arg(&key)
            .arg(Self::DOCKET_GROUP)
            .query_async(&mut conn)
            .await?;

        // First element is the pending count
        match result {
            redis::Value::Array(ref items) if !items.is_empty() => match &items[0] {
                redis::Value::Int(n) => Ok(*n as usize),
                _ => Ok(0),
            },
            _ => Ok(0),
        }
    }

    /// Get the unread backlog for the docket consumer group from XINFO GROUPS lag.
    ///
    /// Unlike XLEN, lag excludes acknowledged entries that remain in the stream
    /// for audit/history purposes.
    pub async fn docket_group_lag(&self) -> Result<usize> {
        let mut conn = self.conn.clone();
        let key = self.docket_tasks_key();

        let result: redis::Value = redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        for group in Self::parse_xinfo_groups(result) {
            let Some(name) = group.get("name") else {
                continue;
            };
            if name == Self::DOCKET_GROUP
                && let Some(lag) = group
                    .get("lag")
                    .and_then(|value| value.parse::<usize>().ok())
            {
                return Ok(lag);
            }
        }

        Ok(0)
    }

    /// Log a task lifecycle event to the docket events stream.
    ///
    /// Used for task progress tracking (started, completed, failed, etc.).
    pub async fn docket_log_event(
        &self,
        task_id: crate::task::TaskId,
        event_type: &str,
        detail: &str,
    ) -> Result<String> {
        let mut conn = self.conn.clone();
        let key = self.docket_events_key();
        let now = chrono::Utc::now().to_rfc3339();

        let entry_id: String = redis::cmd("XADD")
            .arg(&key)
            .arg("*")
            .arg("task_id")
            .arg(task_id.to_string())
            .arg("event")
            .arg(event_type)
            .arg("detail")
            .arg(detail)
            .arg("timestamp")
            .arg(&now)
            .query_async(&mut conn)
            .await?;

        debug!(
            "Docket event {} for task {} -> {}",
            event_type, task_id, entry_id
        );
        Ok(entry_id)
    }

    /// Parse a single entry from an XREADGROUP response.
    ///
    /// XREADGROUP returns: [[stream_name, [[id, [field, value, ...]], ...]]]
    /// We expect at most one entry from COUNT 1.
    fn parse_xread_single(
        value: redis::Value,
    ) -> Result<Option<(String, std::collections::HashMap<String, String>)>> {
        use redis::Value;

        // Nil means no data (timeout)
        let streams = match value {
            Value::Nil => return Ok(None),
            Value::Array(s) => s,
            _ => return Ok(None),
        };

        // streams[0] = [stream_name, entries]
        let stream = match streams.into_iter().next() {
            Some(Value::Array(s)) => s,
            _ => return Ok(None),
        };

        // stream[1] = entries array
        let entries = match stream.into_iter().nth(1) {
            Some(Value::Array(e)) => e,
            _ => return Ok(None),
        };

        // entries[0] = [entry_id, [field, value, ...]]
        let entry = match entries.into_iter().next() {
            Some(Value::Array(e)) => e,
            _ => return Ok(None),
        };

        let mut entry_iter = entry.into_iter();

        // entry_id
        let entry_id = match entry_iter.next() {
            Some(Value::BulkString(b)) => String::from_utf8_lossy(&b).to_string(),
            _ => return Ok(None),
        };

        // fields = [key, val, key, val, ...]
        let fields_raw = match entry_iter.next() {
            Some(Value::Array(f)) => f,
            _ => return Ok(None),
        };

        let mut fields = std::collections::HashMap::new();
        let mut field_iter = fields_raw.into_iter();
        while let (Some(k), Some(v)) = (field_iter.next(), field_iter.next()) {
            if let (Value::BulkString(kb), Value::BulkString(vb)) = (k, v) {
                fields.insert(
                    String::from_utf8_lossy(&kb).to_string(),
                    String::from_utf8_lossy(&vb).to_string(),
                );
            }
        }

        Ok(Some((entry_id, fields)))
    }

    fn parse_xinfo_groups(value: redis::Value) -> Vec<std::collections::HashMap<String, String>> {
        use redis::Value;

        fn value_to_string(value: Value) -> Option<String> {
            match value {
                Value::BulkString(bytes) => Some(String::from_utf8_lossy(&bytes).to_string()),
                Value::SimpleString(text) => Some(text),
                Value::Int(number) => Some(number.to_string()),
                Value::Nil => None,
                _ => None,
            }
        }

        let groups = match value {
            Value::Array(groups) => groups,
            _ => return Vec::new(),
        };

        groups
            .into_iter()
            .filter_map(|group| match group {
                Value::Array(fields) => {
                    let mut parsed = std::collections::HashMap::new();
                    let mut iter = fields.into_iter();
                    while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                        if let (Some(key), Some(value)) =
                            (value_to_string(key), value_to_string(value))
                        {
                            parsed.insert(key, value);
                        }
                    }
                    Some(parsed)
                }
                Value::Map(entries) => {
                    let mut parsed = std::collections::HashMap::new();
                    for (key, value) in entries {
                        if let (Some(key), Some(value)) =
                            (value_to_string(key), value_to_string(value))
                        {
                            parsed.insert(key, value);
                        }
                    }
                    Some(parsed)
                }
                _ => None,
            })
            .collect()
    }
}
