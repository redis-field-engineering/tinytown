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
const INBOX_PREFIX: &str = "mt:inbox:";

/// Key prefix for agent state
const STATE_PREFIX: &str = "mt:agent:";

/// Key prefix for tasks
const TASK_PREFIX: &str = "mt:task:";

/// Key prefix for agent activity logs
const ACTIVITY_PREFIX: &str = "mt:activity:";

/// TTL for activity logs (1 hour)
const ACTIVITY_TTL_SECS: u64 = 3600;

/// Max activity entries per agent
const ACTIVITY_MAX_ENTRIES: isize = 10;

/// Pub/sub channel for broadcasts
const BROADCAST_CHANNEL: &str = "mt:broadcast";

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
}
