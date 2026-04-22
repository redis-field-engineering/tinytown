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

pub mod bootstrap;
pub mod compiler;
pub mod dispatcher;
pub mod scheduler;
pub mod storage;
pub mod types;
pub mod watch;

use crate::agent::AgentId;
use crate::channel::Channel;
use crate::error::Result;
use crate::message::{Message, MessageType};

// Re-export commonly used types
pub use bootstrap::{build_mission_work_items, parse_issue_ref};
pub use compiler::{MissionManifest, ParsedIssue, WorkGraph, WorkGraphCompiler};
pub use dispatcher::{DispatcherConfig, DispatcherTickResult, MissionDispatcher};
pub use scheduler::{
    AgentMatchScore, MissionScheduler, MissionTickResult, SchedulerConfig, SchedulerTickResult,
    WorkItemCompletion,
};
pub use storage::MissionStorage;
pub use types::{
    MissionControlMessage, MissionId, MissionPolicy, MissionRun, MissionState, ObjectiveRef,
    TriggerAction, WatchId, WatchItem, WatchKind, WatchStatus, WorkItem, WorkItemId, WorkKind,
    WorkStatus,
};
pub use watch::{
    BugbotComment, CheckDetail, CheckStatus, GhCliGitHubClient, GitHubClient, MockGitHubClient,
    PrCheckResult, ReviewComment, ReviewState, WatchEngine, WatchEngineConfig,
    WatchEngineTickResult, WatchTickResult,
};

/// Returns true when a conductor inbox message is a dispatcher help request for `mission_id`.
#[must_use]
pub fn is_help_request_message_for_mission(message: &Message, mission_id: MissionId) -> bool {
    matches!(
        &message.msg_type,
        MessageType::Query { question }
            if question.contains(&format!("[Mission Help Needed] Mission {}", mission_id))
    )
}

/// Remove stale dispatcher help-request prompts for a mission from the conductor mailbox.
pub async fn retire_help_requests_for_mission(
    channel: &Channel,
    mission_id: MissionId,
) -> Result<usize> {
    channel
        .remove_inbox_messages_matching(AgentId::supervisor(), |message| {
            is_help_request_message_for_mission(message, mission_id)
        })
        .await
}
