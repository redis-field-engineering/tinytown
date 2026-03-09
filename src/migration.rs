/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Migration support for town isolation.
//!
//! This module handles migrating old Redis key formats (without town namespacing)
//! to the new format with town name prefixes for isolation.
//!
//! Old format: `tt:agent:<uuid>`, `tt:inbox:<uuid>`, `tt:task:<uuid>`
//! New format: `tt:<town_name>:agent:<uuid>`, `tt:<town_name>:inbox:<uuid>`, etc.

use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

/// Statistics from a migration operation.
#[derive(Debug, Default, Clone)]
pub struct MigrationStats {
    /// Number of agent keys migrated
    pub agents_migrated: usize,
    /// Number of inbox keys migrated  
    pub inboxes_migrated: usize,
    /// Number of urgent inbox keys migrated
    pub urgent_migrated: usize,
    /// Number of task keys migrated
    pub tasks_migrated: usize,
    /// Number of activity keys migrated
    pub activity_migrated: usize,
    /// Number of stop keys migrated
    pub stop_migrated: usize,
    /// Number of backlog items migrated
    pub backlog_migrated: usize,
    /// Keys that failed to migrate
    pub errors: Vec<String>,
}

impl MigrationStats {
    /// Total number of keys migrated successfully.
    pub fn total_migrated(&self) -> usize {
        self.agents_migrated
            + self.inboxes_migrated
            + self.urgent_migrated
            + self.tasks_migrated
            + self.activity_migrated
            + self.stop_migrated
            + self.backlog_migrated
    }

    /// Check if any migration occurred.
    pub fn has_changes(&self) -> bool {
        self.total_migrated() > 0
    }
}

/// Check if there are old-format keys that need migration.
///
/// Old format keys match patterns like:
/// - `tt:agent:<uuid>` (not `tt:<town>:agent:<uuid>`)
/// - `tt:inbox:<uuid>` (not `tt:<town>:inbox:<uuid>`)
pub async fn needs_migration(conn: &mut ConnectionManager) -> Result<bool> {
    let old_patterns = [
        "tt:agent:*",
        "tt:inbox:*",
        "tt:urgent:*",
        "tt:task:*",
        "tt:activity:*",
        "tt:stop:*",
        "tt:backlog",
        "tt:broadcast",
    ];

    for pattern in old_patterns {
        let keys: Vec<String> = redis::cmd("KEYS").arg(pattern).query_async(conn).await?;

        // Filter out keys that are already namespaced (have 4 parts)
        for key in keys {
            let parts: Vec<&str> = key.split(':').collect();
            // Old format: tt:type:uuid (3 parts) or tt:type (2 parts like tt:backlog, tt:broadcast)
            // New format: tt:town:type:uuid (4 parts) or tt:town:type (3 parts)
            if parts[0] == "tt" && (parts.len() == 2 || parts.len() == 3) {
                debug!("Found old-format key: {}", key);
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Scan for old-format keys matching a pattern.
///
/// Old format keys have 2 or 3 colon-separated segments:
/// - 2 parts: `tt:backlog`, `tt:broadcast`
/// - 3 parts: `tt:agent:<uuid>`, `tt:inbox:<uuid>`, etc.
///
/// New format keys have 3 or 4 segments (with town name):
/// - 3 parts: `tt:<town>:backlog`
/// - 4 parts: `tt:<town>:agent:<uuid>`
async fn scan_old_keys(conn: &mut ConnectionManager, pattern: &str) -> Result<Vec<String>> {
    let mut cursor: u64 = 0;
    let mut all_keys = Vec::new();

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(conn)
            .await?;

        // Filter to only old-format keys (2 or 3 parts)
        for key in keys {
            let parts: Vec<&str> = key.split(':').collect();
            // Old format: tt:type (2 parts) or tt:type:uuid (3 parts)
            // New format: tt:town:type (3 parts) or tt:town:type:uuid (4 parts)
            if parts[0] == "tt" && (parts.len() == 2 || parts.len() == 3) {
                all_keys.push(key);
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    Ok(all_keys)
}

/// Migrate a single key to the new format.
///
/// Handles both 2-part and 3-part old-format keys:
/// - 2 parts: `tt:backlog` -> `tt:<town>:backlog`
/// - 3 parts: `tt:agent:<uuid>` -> `tt:<town>:agent:<uuid>`
async fn migrate_key(
    conn: &mut ConnectionManager,
    old_key: &str,
    town_name: &str,
) -> Result<String> {
    let parts: Vec<&str> = old_key.split(':').collect();
    if parts[0] != "tt" || (parts.len() != 2 && parts.len() != 3) {
        return Err(Error::Migration(format!(
            "Invalid old-format key: {}",
            old_key
        )));
    }

    let new_key = if parts.len() == 2 {
        // 2-part key like tt:backlog -> tt:<town>:backlog
        let key_type = parts[1];
        format!("tt:{}:{}", town_name, key_type)
    } else {
        // 3-part key like tt:agent:<uuid> -> tt:<town>:agent:<uuid>
        let key_type = parts[1];
        let id = parts[2];
        format!("tt:{}:{}:{}", town_name, key_type, id)
    };

    // Rename the key atomically
    let _: () = conn.rename(old_key, &new_key).await?;
    debug!("Migrated {} -> {}", old_key, new_key);

    Ok(new_key)
}

/// Migrate all old-format keys to the new town-namespaced format.
///
/// This function:
/// 1. Scans for old-format keys (tt:type:uuid)
/// 2. Renames them to new format (tt:town:type:uuid)
/// 3. Returns statistics about the migration
///
/// This is idempotent - running it multiple times is safe.
pub async fn migrate_to_town_isolation(
    conn: &mut ConnectionManager,
    town_name: &str,
) -> Result<MigrationStats> {
    let mut stats = MigrationStats::default();

    info!("Starting migration to town isolation for '{}'", town_name);

    // Migrate agent keys
    let agent_keys = scan_old_keys(conn, "tt:agent:*").await?;
    for key in agent_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.agents_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate inbox keys
    let inbox_keys = scan_old_keys(conn, "tt:inbox:*").await?;
    for key in inbox_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.inboxes_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate urgent inbox keys
    let urgent_keys = scan_old_keys(conn, "tt:urgent:*").await?;
    for key in urgent_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.urgent_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate task keys
    let task_keys = scan_old_keys(conn, "tt:task:*").await?;
    for key in task_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.tasks_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate activity keys
    let activity_keys = scan_old_keys(conn, "tt:activity:*").await?;
    for key in activity_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.activity_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate stop keys
    let stop_keys = scan_old_keys(conn, "tt:stop:*").await?;
    for key in stop_keys {
        match migrate_key(conn, &key, town_name).await {
            Ok(_) => stats.stop_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate backlog (single key)
    let backlog_exists: bool = conn.exists("tt:backlog").await?;
    if backlog_exists {
        let new_key = format!("tt:{}:backlog", town_name);
        let result: redis::RedisResult<()> = conn.rename("tt:backlog", &new_key).await;
        match result {
            Ok(_) => {
                debug!("Migrated tt:backlog -> {}", new_key);
                stats.backlog_migrated = 1;
            }
            Err(e) => {
                warn!("Failed to migrate tt:backlog: {}", e);
                stats.errors.push("tt:backlog".to_string());
            }
        }
    }

    info!(
        "Migration complete: {} keys migrated, {} errors",
        stats.total_migrated(),
        stats.errors.len()
    );

    Ok(stats)
}

// =============================================================================
// JSON String to Redis Hash Migration
// =============================================================================

/// Statistics from a JSON-to-Hash migration operation.
#[derive(Debug, Default, Clone)]
pub struct HashMigrationStats {
    /// Number of agent keys migrated from JSON to Hash
    pub agents_migrated: usize,
    /// Number of task keys migrated from JSON to Hash
    pub tasks_migrated: usize,
    /// Keys that were already Hash type (skipped)
    pub already_hash: usize,
    /// Keys that failed to migrate
    pub errors: Vec<String>,
}

impl HashMigrationStats {
    /// Total number of keys migrated successfully.
    pub fn total_migrated(&self) -> usize {
        self.agents_migrated + self.tasks_migrated
    }

    /// Check if any migration occurred.
    pub fn has_changes(&self) -> bool {
        self.total_migrated() > 0
    }
}

/// Check if there are JSON string keys that need migration to Hash.
///
/// Scans for agent and task keys that are stored as strings instead of hashes.
pub async fn needs_hash_migration(conn: &mut ConnectionManager, town_name: &str) -> Result<bool> {
    // Check agent keys
    let agent_pattern = format!("tt:{}:agent:*", town_name);
    let agent_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&agent_pattern)
        .query_async(conn)
        .await?;

    for key in agent_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "string" {
            debug!("Found JSON string agent key: {}", key);
            return Ok(true);
        }
    }

    // Check task keys
    let task_pattern = format!("tt:{}:task:*", town_name);
    let task_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&task_pattern)
        .query_async(conn)
        .await?;

    for key in task_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "string" {
            debug!("Found JSON string task key: {}", key);
            return Ok(true);
        }
    }

    Ok(false)
}

/// Migrate a single agent key from JSON string to Hash.
async fn migrate_agent_to_hash(conn: &mut ConnectionManager, key: &str) -> Result<()> {
    // Get the JSON string
    let json_str: String = conn.get(key).await?;

    // Parse the JSON into agent fields
    let agent: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| Error::Migration(format!("Failed to parse agent JSON: {}", e)))?;

    // Build hash fields from JSON
    let mut fields: Vec<(String, String)> = Vec::new();

    if let Some(id) = agent.get("id").and_then(|v| v.as_str()) {
        fields.push(("id".to_string(), id.to_string()));
    }
    if let Some(name) = agent.get("name").and_then(|v| v.as_str()) {
        fields.push(("name".to_string(), name.to_string()));
    }
    if let Some(agent_type) = agent.get("agent_type").and_then(|v| v.as_str()) {
        fields.push(("agent_type".to_string(), agent_type.to_string()));
    }
    if let Some(state) = agent.get("state").and_then(|v| v.as_str()) {
        fields.push(("state".to_string(), state.to_string()));
    }
    if let Some(cli) = agent.get("cli").and_then(|v| v.as_str()) {
        fields.push(("cli".to_string(), cli.to_string()));
    }
    if let Some(current_task) = agent.get("current_task").and_then(|v| v.as_str()) {
        fields.push(("current_task".to_string(), current_task.to_string()));
    }
    if let Some(created_at) = agent.get("created_at").and_then(|v| v.as_str()) {
        fields.push(("created_at".to_string(), created_at.to_string()));
    }
    if let Some(last_heartbeat) = agent.get("last_heartbeat").and_then(|v| v.as_str()) {
        fields.push(("last_heartbeat".to_string(), last_heartbeat.to_string()));
    }
    if let Some(tasks_completed) = agent.get("tasks_completed") {
        let val = if tasks_completed.is_u64() {
            tasks_completed.as_u64().unwrap().to_string()
        } else {
            tasks_completed.to_string()
        };
        fields.push(("tasks_completed".to_string(), val));
    }
    if let Some(rounds_completed) = agent.get("rounds_completed") {
        let val = if rounds_completed.is_u64() {
            rounds_completed.as_u64().unwrap().to_string()
        } else {
            rounds_completed.to_string()
        };
        fields.push(("rounds_completed".to_string(), val));
    }

    if fields.is_empty() {
        return Err(Error::Migration(format!(
            "No valid fields found in agent JSON for key: {}",
            key
        )));
    }

    // Delete old string key and set hash atomically via pipeline
    let mut pipe = redis::pipe();
    pipe.del(key);
    pipe.hset_multiple(key, &fields);
    let _: () = pipe.query_async(conn).await?;

    debug!("Migrated agent {} from JSON to Hash", key);
    Ok(())
}

/// Migrate a single task key from JSON string to Hash.
async fn migrate_task_to_hash(conn: &mut ConnectionManager, key: &str) -> Result<()> {
    // Get the JSON string
    let json_str: String = conn.get(key).await?;

    // Parse the JSON into task fields
    let task: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| Error::Migration(format!("Failed to parse task JSON: {}", e)))?;

    // Build hash fields from JSON
    let mut fields: Vec<(String, String)> = Vec::new();

    if let Some(id) = task.get("id").and_then(|v| v.as_str()) {
        fields.push(("id".to_string(), id.to_string()));
    }
    if let Some(description) = task.get("description").and_then(|v| v.as_str()) {
        fields.push(("description".to_string(), description.to_string()));
    }
    if let Some(state) = task.get("state").and_then(|v| v.as_str()) {
        fields.push(("state".to_string(), state.to_string()));
    }
    if let Some(assigned_to) = task.get("assigned_to").and_then(|v| v.as_str()) {
        fields.push(("assigned_to".to_string(), assigned_to.to_string()));
    }
    if let Some(created_at) = task.get("created_at").and_then(|v| v.as_str()) {
        fields.push(("created_at".to_string(), created_at.to_string()));
    }
    if let Some(updated_at) = task.get("updated_at").and_then(|v| v.as_str()) {
        fields.push(("updated_at".to_string(), updated_at.to_string()));
    }
    if let Some(started_at) = task.get("started_at").and_then(|v| v.as_str()) {
        fields.push(("started_at".to_string(), started_at.to_string()));
    }
    if let Some(completed_at) = task.get("completed_at").and_then(|v| v.as_str()) {
        fields.push(("completed_at".to_string(), completed_at.to_string()));
    }
    if let Some(result) = task.get("result").and_then(|v| v.as_str()) {
        fields.push(("result".to_string(), result.to_string()));
    }
    if let Some(parent_id) = task.get("parent_id").and_then(|v| v.as_str()) {
        fields.push(("parent_id".to_string(), parent_id.to_string()));
    }
    // Tags remain as JSON array string
    if let Some(tags) = task.get("tags")
        && tags.is_array() {
            fields.push((
                "tags".to_string(),
                serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string()),
            ));
        }

    if fields.is_empty() {
        return Err(Error::Migration(format!(
            "No valid fields found in task JSON for key: {}",
            key
        )));
    }

    // Delete old string key and set hash atomically via pipeline
    let mut pipe = redis::pipe();
    pipe.del(key);
    pipe.hset_multiple(key, &fields);
    let _: () = pipe.query_async(conn).await?;

    debug!("Migrated task {} from JSON to Hash", key);
    Ok(())
}

/// Migrate all JSON string keys to Redis Hashes for a town.
///
/// This function:
/// 1. Scans for agent and task keys with string type
/// 2. Parses the JSON and converts to Hash fields
/// 3. Atomically replaces the string key with a hash key
///
/// This is idempotent - running it multiple times is safe (already-migrated keys are skipped).
pub async fn migrate_json_to_hash(
    conn: &mut ConnectionManager,
    town_name: &str,
) -> Result<HashMigrationStats> {
    let mut stats = HashMigrationStats::default();

    info!("Starting JSON-to-Hash migration for town '{}'", town_name);

    // Migrate agent keys
    let agent_pattern = format!("tt:{}:agent:*", town_name);
    let agent_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&agent_pattern)
        .query_async(conn)
        .await?;

    for key in agent_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "hash" {
            stats.already_hash += 1;
            continue;
        }
        if key_type != "string" {
            warn!("Unexpected key type '{}' for {}, skipping", key_type, key);
            continue;
        }

        match migrate_agent_to_hash(conn, &key).await {
            Ok(_) => stats.agents_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate agent {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    // Migrate task keys
    let task_pattern = format!("tt:{}:task:*", town_name);
    let task_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&task_pattern)
        .query_async(conn)
        .await?;

    for key in task_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "hash" {
            stats.already_hash += 1;
            continue;
        }
        if key_type != "string" {
            warn!("Unexpected key type '{}' for {}, skipping", key_type, key);
            continue;
        }

        match migrate_task_to_hash(conn, &key).await {
            Ok(_) => stats.tasks_migrated += 1,
            Err(e) => {
                warn!("Failed to migrate task {}: {}", key, e);
                stats.errors.push(key);
            }
        }
    }

    info!(
        "JSON-to-Hash migration complete: {} agents, {} tasks migrated, {} already hash, {} errors",
        stats.agents_migrated,
        stats.tasks_migrated,
        stats.already_hash,
        stats.errors.len()
    );

    Ok(stats)
}

/// Preview JSON-to-Hash migration without making changes.
///
/// Returns the list of keys that would be migrated.
pub async fn preview_hash_migration(
    conn: &mut ConnectionManager,
    town_name: &str,
) -> Result<Vec<String>> {
    let mut preview = Vec::new();

    // Check agent keys
    let agent_pattern = format!("tt:{}:agent:*", town_name);
    let agent_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&agent_pattern)
        .query_async(conn)
        .await?;

    for key in agent_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "string" {
            preview.push(key);
        }
    }

    // Check task keys
    let task_pattern = format!("tt:{}:task:*", town_name);
    let task_keys: Vec<String> = redis::cmd("KEYS")
        .arg(&task_pattern)
        .query_async(conn)
        .await?;

    for key in task_keys {
        let key_type: String = redis::cmd("TYPE").arg(&key).query_async(conn).await?;
        if key_type == "string" {
            preview.push(key);
        }
    }

    Ok(preview)
}

// =============================================================================
// Town Isolation Migration (existing code)
// =============================================================================

/// Preview migration without making changes.
///
/// Returns the list of keys that would be migrated.
pub async fn preview_migration(conn: &mut ConnectionManager) -> Result<Vec<(String, String)>> {
    let mut preview = Vec::new();

    // Check all key types
    let patterns = [
        "tt:agent:*",
        "tt:inbox:*",
        "tt:urgent:*",
        "tt:task:*",
        "tt:activity:*",
        "tt:stop:*",
    ];

    for pattern in patterns {
        let keys = scan_old_keys(conn, pattern).await?;
        for key in keys {
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() == 2 {
                // 2-part key like tt:backlog
                let key_type = parts[1];
                preview.push((key.clone(), format!("tt:<town>:{}", key_type)));
            } else if parts.len() == 3 {
                // 3-part key like tt:agent:<uuid>
                let key_type = parts[1];
                let id = parts[2];
                preview.push((key.clone(), format!("tt:<town>:{}:{}", key_type, id)));
            }
        }
    }

    // Check backlog (also check for tt:broadcast)
    let backlog_exists: bool = conn.exists("tt:backlog").await?;
    if backlog_exists {
        preview.push(("tt:backlog".to_string(), "tt:<town>:backlog".to_string()));
    }
    let broadcast_exists: bool = conn.exists("tt:broadcast").await?;
    if broadcast_exists {
        preview.push((
            "tt:broadcast".to_string(),
            "tt:<town>:broadcast".to_string(),
        ));
    }

    Ok(preview)
}
