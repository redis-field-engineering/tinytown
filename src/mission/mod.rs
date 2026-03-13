/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission mode: autonomous multi-issue execution.
//!
//! This module provides durable, dependency-aware orchestration of multiple
//! GitHub issues with automatic PR/CI monitoring.
//!
//! # Core Types
//!
//! - [`MissionRun`] - Top-level orchestration record
//! - [`WorkItem`] - Individual work unit in the DAG
//! - [`WatchItem`] - PR/CI monitoring task
//!
//! # Compiler
//!
//! - [`WorkGraphCompiler`] - Transforms issues/docs into a dependency-aware DAG
//! - [`WorkGraph`] - Compiled work graph with topological ordering
//!
//! # Scheduler
//!
//! - [`MissionScheduler`] - Periodic tick-based work orchestration
//! - [`SchedulerConfig`] - Scheduler configuration (tick interval, max parallel, etc.)
//! - [`SchedulerTickResult`] - Result of a scheduler tick
//!
//! # Storage
//!
//! - [`MissionStorage`] - Redis persistence layer
//!
//! # Example
//!
//! ```no_run
//! use tinytown::mission::{MissionRun, ObjectiveRef, MissionStorage, WorkGraphCompiler};
//!
//! # async fn example() -> tinytown::Result<()> {
//! // Create a mission with objectives
//! let mission = MissionRun::new(vec![
//!     ObjectiveRef::Issue {
//!         owner: "redis-field-engineering".into(),
//!         repo: "tinytown".into(),
//!         number: 23,
//!     },
//! ]);
//!
//! // Compile work graph from parsed issues
//! let compiler = WorkGraphCompiler::new();
//! // let graph = compiler.compile(mission.id, parsed_issues, None)?;
//!
//! // Save to Redis via storage
//! // let storage = MissionStorage::new(conn, "my-town");
//! // storage.save_mission(&mission).await?;
//! # Ok(())
//! # }
//! ```

pub mod compiler;
pub mod scheduler;
pub mod storage;
pub mod types;
pub mod watch;

// Re-export commonly used types
pub use compiler::{MissionManifest, ParsedIssue, WorkGraph, WorkGraphCompiler};
pub use scheduler::{
    AgentMatchScore, MissionScheduler, MissionTickResult, SchedulerConfig, SchedulerTickResult,
    WorkItemCompletion,
};
pub use storage::MissionStorage;
pub use types::{
    MissionId, MissionPolicy, MissionRun, MissionState, ObjectiveRef, TriggerAction, WatchId,
    WatchItem, WatchKind, WatchStatus, WorkItem, WorkItemId, WorkKind, WorkStatus,
};
pub use watch::{
    BugbotComment, CheckDetail, CheckStatus, GitHubClient, MockGitHubClient, PrCheckResult,
    ReviewComment, ReviewState, WatchEngine, WatchEngineConfig, WatchEngineTickResult,
    WatchTickResult,
};
