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

        // Filter to only old-format keys (3 parts)
        for key in keys {
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() == 3 && parts[0] == "tt" {
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
async fn migrate_key(
    conn: &mut ConnectionManager,
    old_key: &str,
    town_name: &str,
) -> Result<String> {
    let parts: Vec<&str> = old_key.split(':').collect();
    if parts.len() != 3 || parts[0] != "tt" {
        return Err(Error::Migration(format!(
            "Invalid old-format key: {}",
            old_key
        )));
    }

    let key_type = parts[1];
    let id = parts[2];
    let new_key = format!("tt:{}:{}:{}", town_name, key_type, id);

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
            if parts.len() == 3 {
                let key_type = parts[1];
                let id = parts[2];
                preview.push((key.clone(), format!("tt:<town>:{}:{}", key_type, id)));
            }
        }
    }

    // Check backlog
    let backlog_exists: bool = conn.exists("tt:backlog").await?;
    if backlog_exists {
        preview.push(("tt:backlog".to_string(), "tt:<town>:backlog".to_string()));
    }

    Ok(preview)
}
