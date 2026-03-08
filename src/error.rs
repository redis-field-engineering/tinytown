/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Error types for tinytown.

use thiserror::Error;

/// Main error type for tinytown operations.
#[derive(Error, Debug)]
pub enum Error {
    /// Redis connection or operation failed
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// Configuration error
    #[error("Config error: {0}")]
    Config(String),

    /// Agent not found
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    /// Task not found  
    #[error("Task not found: {0}")]
    TaskNotFound(String),

    /// Agent already exists
    #[error("Agent already exists: {0}")]
    AgentExists(String),

    /// Task assignment failed
    #[error("Failed to assign task: {0}")]
    AssignmentFailed(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Town not initialized
    #[error("Town not initialized at {0}. Run 'tt init' first.")]
    NotInitialized(String),

    /// Redis not running
    #[error("Redis not running. Start with 'tt start' or ensure Redis is available.")]
    RedisNotRunning,

    /// Redis not installed
    #[error(
        "Redis not found. Run 'tt bootstrap' to download and build Redis automatically.\n\nAlternatives:\n  - macOS: brew install redis\n  - Ubuntu: sudo apt install redis-server\n  - Manual: https://redis.io/downloads/"
    )]
    RedisNotInstalled,

    /// Redis version too old
    #[error(
        "Redis version {0} is too old. Tinytown requires Redis 8.0+.\n\nRun 'tt bootstrap' to install the latest version.\n\nAlternatives:\n  - macOS: brew upgrade redis\n  - Ubuntu: See https://redis.io/downloads/"
    )]
    RedisVersionTooOld(String),

    /// Timeout waiting for operation
    #[error("Operation timed out: {0}")]
    Timeout(String),
}

/// Convenience Result type for tinytown.
pub type Result<T> = std::result::Result<T, Error>;
