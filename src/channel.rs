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

/// Key prefix for agent inboxes
const INBOX_PREFIX: &str = "tt:inbox:";

/// Key prefix for agent state
const STATE_PREFIX: &str = "tt:agent:";

/// Key prefix for tasks
const TASK_PREFIX: &str = "tt:task:";

/// Key prefix for agent activity logs
const ACTIVITY_PREFIX: &str = "tt:activity:";

/// Key prefix for urgent inbox
const URGENT_PREFIX: &str = "tt:urgent:";

/// Key prefix for stop flags
const STOP_PREFIX: &str = "tt:stop:";

/// Key for global task backlog
const BACKLOG_KEY: &str = "tt:backlog";

/// TTL for activity logs (1 hour)
const ACTIVITY_TTL_SECS: u64 = 3600;

/// Max activity entries per agent
const ACTIVITY_MAX_ENTRIES: isize = 10;

/// Pub/sub channel for broadcasts
const BROADCAST_CHANNEL: &str = "tt:broadcast";

/// Redis-based communication channel.
#[derive(Clone)]
pub struct Channel {
    conn: ConnectionManager,
}

impl Channel {
    /// Create a new channel from a Redis connection.
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    /// Send a message to an agent's inbox.
    #[instrument(skip(self, message), fields(to = %message.to, msg_type = ?message.msg_type))]
    pub async fn send(&self, message: &Message) -> Result<()> {
        let mut conn = self.conn.clone();
        let inbox_key = format!("{}{}", INBOX_PREFIX, message.to);
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
        let urgent_key = format!("{}{}", URGENT_PREFIX, message.to);

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
        let urgent_key = format!("{}{}", URGENT_PREFIX, agent_id);

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
        let urgent_key = format!("{}{}", URGENT_PREFIX, agent_id);
        let len: usize = conn.llen(&urgent_key).await?;
        Ok(len)
    }

    /// Request an agent to stop gracefully.
    ///
    /// Sets a stop flag that the agent checks at the start of each round.
    #[instrument(skip(self))]
    pub async fn request_stop(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let stop_key = format!("{}{}", STOP_PREFIX, agent_id);

        // Set flag with 1-hour TTL (cleanup if agent already dead)
        let _: () = conn.set_ex(&stop_key, "1", 3600).await?;

        debug!("Requested stop for agent {}", agent_id);
        Ok(())
    }

    /// Check if stop has been requested for an agent.
    #[instrument(skip(self))]
    pub async fn should_stop(&self, agent_id: AgentId) -> Result<bool> {
        let mut conn = self.conn.clone();
        let stop_key = format!("{}{}", STOP_PREFIX, agent_id);

        let exists: bool = conn.exists(&stop_key).await?;
        Ok(exists)
    }

    /// Clear the stop flag (called when agent stops).
    pub async fn clear_stop(&self, agent_id: AgentId) -> Result<()> {
        let mut conn = self.conn.clone();
        let stop_key = format!("{}{}", STOP_PREFIX, agent_id);

        let _: () = conn.del(&stop_key).await?;
        Ok(())
    }

    /// Receive a message from an agent's inbox (blocking with timeout).
    #[instrument(skip(self))]
    pub async fn receive(&self, agent_id: AgentId, timeout: Duration) -> Result<Option<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);

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
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);

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
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);
        let len: usize = conn.llen(&inbox_key).await?;
        Ok(len)
    }

    /// Peek at inbox messages without removing them.
    pub async fn peek_inbox(&self, agent_id: AgentId, count: isize) -> Result<Vec<Message>> {
        let mut conn = self.conn.clone();
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);
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
        let _: () = conn.publish(BROADCAST_CHANNEL, &serialized).await?;
        Ok(())
    }

    /// Store agent state in Redis.
    pub async fn set_agent_state(&self, agent: &crate::agent::Agent) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = format!("{}{}", STATE_PREFIX, agent.id);
        let serialized = serde_json::to_string(agent)?;
        let _: () = conn.set(&key, &serialized).await?;
        Ok(())
    }

    /// Get agent state from Redis.
    pub async fn get_agent_state(&self, agent_id: AgentId) -> Result<Option<crate::agent::Agent>> {
        let mut conn = self.conn.clone();
        let key = format!("{}{}", STATE_PREFIX, agent_id);
        let result: Option<String> = conn.get(&key).await?;

        match result {
            Some(data) => Ok(Some(serde_json::from_str(&data)?)),
            None => Ok(None),
        }
    }

    /// List all agents from Redis.
    pub async fn list_agents(&self) -> Result<Vec<crate::agent::Agent>> {
        let mut conn = self.conn.clone();
        let pattern = format!("{}*", STATE_PREFIX);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        let mut agents = Vec::new();
        for key in keys {
            if let Ok(Some(data)) = conn.get::<_, Option<String>>(&key).await
                && let Ok(agent) = serde_json::from_str::<crate::agent::Agent>(&data)
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
        let key = format!("{}{}", STATE_PREFIX, agent_id);
        let _: () = conn.del(&key).await?;
        // Also clean up related keys
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);
        let urgent_key = format!("{}{}", URGENT_PREFIX, agent_id);
        let activity_key = format!("{}{}", ACTIVITY_PREFIX, agent_id);
        let stop_key = format!("{}{}", STOP_PREFIX, agent_id);
        let _: () = conn.del(&inbox_key).await?;
        let _: () = conn.del(&urgent_key).await?;
        let _: () = conn.del(&activity_key).await?;
        let _: () = conn.del(&stop_key).await?;
        Ok(())
    }

    /// Store a task in Redis.
    pub async fn set_task(&self, task: &crate::task::Task) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = format!("{}{}", TASK_PREFIX, task.id);
        let serialized = serde_json::to_string(task)?;
        let _: () = conn.set(&key, &serialized).await?;
        Ok(())
    }

    /// Get a task from Redis.
    pub async fn get_task(
        &self,
        task_id: crate::task::TaskId,
    ) -> Result<Option<crate::task::Task>> {
        let mut conn = self.conn.clone();
        let key = format!("{}{}", TASK_PREFIX, task_id);
        let result: Option<String> = conn.get(&key).await?;

        match result {
            Some(data) => Ok(Some(serde_json::from_str(&data)?)),
            None => Ok(None),
        }
    }

    /// Log agent activity (bounded, with TTL).
    ///
    /// Stores recent activity in Redis list, trimmed to ACTIVITY_MAX_ENTRIES.
    /// TTL ensures cleanup even if agent dies.
    #[instrument(skip(self, activity))]
    pub async fn log_agent_activity(&self, agent_id: AgentId, activity: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let key = format!("{}{}", ACTIVITY_PREFIX, agent_id);

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
        let key = format!("{}{}", ACTIVITY_PREFIX, agent_id);

        let entries: Vec<String> = conn.lrange(&key, 0, 4).await?; // Get last 5

        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(entries.join("\n")))
        }
    }

    // ==================== Backlog Methods ====================

    /// Add a task ID to the global backlog queue.
    pub async fn backlog_push(&self, task_id: crate::task::TaskId) -> Result<()> {
        let mut conn = self.conn.clone();
        let _: () = conn.rpush(BACKLOG_KEY, task_id.to_string()).await?;
        debug!("Added task {} to backlog", task_id);
        Ok(())
    }

    /// List all task IDs in the backlog.
    pub async fn backlog_list(&self) -> Result<Vec<crate::task::TaskId>> {
        let mut conn = self.conn.clone();
        let items: Vec<String> = conn.lrange(BACKLOG_KEY, 0, -1).await?;

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
        let len: usize = conn.llen(BACKLOG_KEY).await?;
        Ok(len)
    }

    /// Pop a task from the front of the backlog (FIFO).
    pub async fn backlog_pop(&self) -> Result<Option<crate::task::TaskId>> {
        let mut conn = self.conn.clone();
        let result: Option<String> = conn.lpop(BACKLOG_KEY, None).await?;

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
        let removed: i64 = conn.lrem(BACKLOG_KEY, 1, task_id.to_string()).await?;
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
        let inbox_key = format!("{}{}", INBOX_PREFIX, agent_id);

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
}
