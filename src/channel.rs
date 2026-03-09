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
    /// Town name used for key namespacing
    town_name: String,
}

impl Channel {
    /// Create a new channel from a Redis connection with town namespacing.
    ///
    /// All Redis keys will be prefixed with `tt:{town_name}:` to ensure
    /// isolation between different towns sharing the same Redis instance.
    pub fn new(conn: ConnectionManager, town_name: impl Into<String>) -> Self {
        Self {
            conn,
            town_name: town_name.into(),
        }
    }

    /// Get the town name for this channel.
    pub fn town_name(&self) -> &str {
        &self.town_name
    }

    // ==================== Key Generation ====================
    // All keys are namespaced by town name: tt:{town}:{type}:{id}

    /// Generate inbox key for an agent.
    fn inbox_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:inbox:{}", self.town_name, agent_id)
    }

    /// Generate urgent inbox key for an agent.
    fn urgent_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:urgent:{}", self.town_name, agent_id)
    }

    /// Generate state key for an agent.
    fn state_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:agent:{}", self.town_name, agent_id)
    }

    /// Generate task key.
    fn task_key(&self, task_id: crate::task::TaskId) -> String {
        format!("tt:{}:task:{}", self.town_name, task_id)
    }

    /// Generate activity key for an agent.
    fn activity_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:activity:{}", self.town_name, agent_id)
    }

    /// Generate stop flag key for an agent.
    fn stop_key(&self, agent_id: AgentId) -> String {
        format!("tt:{}:stop:{}", self.town_name, agent_id)
    }

    /// Generate backlog key for this town.
    fn backlog_key(&self) -> String {
        format!("tt:{}:backlog", self.town_name)
    }

    /// Generate broadcast channel name for this town.
    fn broadcast_channel(&self) -> String {
        format!("tt:{}:broadcast", self.town_name)
    }

    /// Generate key pattern for scanning all keys in this town.
    fn town_key_pattern(&self) -> String {
        format!("tt:{}:*", self.town_name)
    }

    /// Generate key pattern for scanning agent keys in this town.
    fn agent_key_pattern(&self) -> String {
        format!("tt:{}:agent:*", self.town_name)
    }

    /// Generate key pattern for scanning inbox keys in this town.
    fn inbox_key_pattern(&self) -> String {
        format!("tt:{}:inbox:*", self.town_name)
    }

    /// Generate key pattern for scanning stop keys in this town.
    fn stop_key_pattern(&self) -> String {
        format!("tt:{}:stop:*", self.town_name)
    }

    /// Generate key pattern for scanning activity keys in this town.
    fn activity_key_pattern(&self) -> String {
        format!("tt:{}:activity:*", self.town_name)
    }

    /// Generate key pattern for scanning urgent inbox keys in this town.
    fn urgent_key_pattern(&self) -> String {
        format!("tt:{}:urgent:*", self.town_name)
    }

    /// Generate key pattern for scanning task keys in this town.
    fn task_key_pattern(&self) -> String {
        format!("tt:{}:task:*", self.town_name)
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
    /// Uses HSET to store each agent field separately, enabling atomic updates
    /// to individual fields (like state, heartbeat) without rewriting the entire object.
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
        ];

        // Handle optional current_task - use HDEL to clear stale values when None
        if let Some(ref task_id) = agent.current_task {
            fields.push(("current_task".to_string(), task_id.to_string()));
        } else {
            // Delete the field if it's None to avoid stale values persisting
            let _: () = conn.hdel(&key, "current_task").await?;
        }

        // Use HSET with multiple field-value pairs
        let _: () = conn.hset_multiple(&key, &fields).await?;
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

        Ok(crate::agent::Agent {
            id,
            name,
            agent_type,
            state,
            cli,
            current_task,
            created_at,
            last_heartbeat,
            tasks_completed,
            rounds_completed,
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
            if !fields.is_empty() {
                if let Ok(agent) = Self::parse_agent_from_hash(fields) {
                    agents.push(agent);
                }
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
    /// Uses HSET to store each task field separately, enabling atomic updates
    /// to individual fields without rewriting the entire object.
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

        // Handle optional fields - use HDEL to clear stale values when None
        // This prevents old values from persisting when fields transition from Some to None
        if let Some(ref agent_id) = task.assigned_to {
            fields.push(("assigned_to".to_string(), agent_id.to_string()));
        } else {
            let _: () = conn.hdel(&key, "assigned_to").await?;
        }
        if let Some(ref started_at) = task.started_at {
            fields.push(("started_at".to_string(), started_at.to_rfc3339()));
        } else {
            let _: () = conn.hdel(&key, "started_at").await?;
        }
        if let Some(ref completed_at) = task.completed_at {
            fields.push(("completed_at".to_string(), completed_at.to_rfc3339()));
        } else {
            let _: () = conn.hdel(&key, "completed_at").await?;
        }
        if let Some(ref result) = task.result {
            fields.push(("result".to_string(), result.clone()));
        } else {
            let _: () = conn.hdel(&key, "result").await?;
        }
        if let Some(ref parent_id) = task.parent_id {
            fields.push(("parent_id".to_string(), parent_id.to_string()));
        } else {
            let _: () = conn.hdel(&key, "parent_id").await?;
        }

        // Use HSET with multiple field-value pairs
        let _: () = conn.hset_multiple(&key, &fields).await?;
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
            if !fields.is_empty() {
                if let Ok(task) = Self::parse_task_from_hash(fields) {
                    tasks.push(task);
                }
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
            count, self.town_name
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
            count, self.town_name
        );
        Ok(count)
    }
}
