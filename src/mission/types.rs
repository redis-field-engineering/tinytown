/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission mode core types.
//!
//! Defines the data model for autonomous multi-issue execution:
//! - `MissionRun` - top-level orchestration record
//! - `WorkItem` - individual work unit in the DAG
//! - `WatchItem` - PR/CI monitoring task

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentId;

// ==================== ID Types ====================

/// Unique identifier for a mission run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionId(Uuid);

impl MissionId {
    /// Create a new random mission ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for MissionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MissionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MissionId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Unique identifier for a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkItemId(Uuid);

impl WorkItemId {
    /// Create a new random work item ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for WorkItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WorkItemId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Unique identifier for a watch item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WatchId(Uuid);

impl WatchId {
    /// Create a new random watch ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for WatchId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WatchId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

// ==================== Enums ====================

/// Mission execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MissionState {
    /// Compiling work graph from objectives
    #[default]
    Planning,
    /// Active execution
    Running,
    /// Waiting on external event
    Blocked,
    /// All objectives completed
    Completed,
    /// Unrecoverable error
    Failed,
}

/// Type of work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WorkKind {
    /// Design/planning work
    Design,
    /// Implementation work
    #[default]
    Implement,
    /// Testing work
    Test,
    /// Code review
    Review,
    /// Merge gate (waiting for approval)
    MergeGate,
    /// Follow-up work (bug fixes, improvements)
    Followup,
}

/// Work item execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WorkStatus {
    /// Dependencies not satisfied
    #[default]
    Pending,
    /// Can be assigned (dependencies done)
    Ready,
    /// Agent selected, not yet started
    Assigned,
    /// In progress
    Running,
    /// Waiting on fix/external
    Blocked,
    /// Completed successfully
    Done,
}

impl WorkStatus {
    /// Check if work item is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done)
    }

    /// Check if work item can be assigned.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Type of watch item for monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// Monitor PR CI checks
    #[default]
    PrChecks,
    /// Monitor Bugbot comments
    BugbotComments,
    /// Monitor review comments
    ReviewComments,
    /// Monitor PR mergeability
    Mergeability,
}

/// Watch item status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WatchStatus {
    /// Actively being monitored
    #[default]
    Active,
    /// Temporarily paused
    Snoozed,
    /// Monitoring complete
    Done,
}

/// Action to take when watch triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TriggerAction {
    /// Create a task to fix the issue
    #[default]
    CreateFixTask,
    /// Notify the reviewer
    NotifyReviewer,
    /// Advance the pipeline
    AdvancePipeline,
}

// ==================== Reference Types ====================

/// Reference to an objective (issue or document).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ObjectiveRef {
    /// GitHub issue reference
    Issue {
        /// Repository owner
        owner: String,
        /// Repository name
        repo: String,
        /// Issue number
        number: u64,
    },
    /// Document path reference
    Doc {
        /// Path to document
        path: String,
    },
}

impl std::fmt::Display for ObjectiveRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectiveRef::Issue {
                owner,
                repo,
                number,
            } => {
                write!(f, "{}/{}#{}", owner, repo, number)
            }
            ObjectiveRef::Doc { path } => write!(f, "{}", path),
        }
    }
}

// ==================== Policy ====================

/// Execution policy for a mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionPolicy {
    /// Maximum parallel work items (default: 2)
    pub max_parallel_items: u32,
    /// Reviewer required for implement/test items (default: true)
    pub reviewer_required: bool,
    /// Auto-merge PRs when approved (default: false)
    pub auto_merge: bool,
    /// Watch interval in seconds (default: 180)
    pub watch_interval_secs: u64,
}

impl Default for MissionPolicy {
    fn default() -> Self {
        Self {
            max_parallel_items: 2,
            reviewer_required: true,
            auto_merge: false,
            watch_interval_secs: 180,
        }
    }
}

// ==================== Main Structs ====================

/// Top-level mission orchestration record.
///
/// A MissionRun is the durable representation of an autonomous multi-issue
/// execution. It owns WorkItems and WatchItems and is the single source of
/// truth for "what's next."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionRun {
    /// Unique mission identifier
    pub id: MissionId,
    /// Objective references (issues, docs)
    pub objective_refs: Vec<ObjectiveRef>,
    /// Current mission state
    pub state: MissionState,
    /// Execution policy
    pub policy: MissionPolicy,
    /// When mission was created
    pub created_at: DateTime<Utc>,
    /// When mission was last updated
    pub updated_at: DateTime<Utc>,
    /// Next scheduler wake-up time
    pub next_wake_at: Option<DateTime<Utc>>,
    /// Reason if blocked
    pub blocked_reason: Option<String>,
    /// Last time the dispatcher ticked this mission
    pub dispatcher_last_tick_at: Option<DateTime<Utc>>,
    /// Last time the dispatcher observed progress on this mission
    pub dispatcher_last_progress_at: Option<DateTime<Utc>>,
    /// Last time the dispatcher asked the conductor for help
    pub dispatcher_last_help_request_at: Option<DateTime<Utc>>,
    /// Most recent help-request reason sent to the conductor
    pub dispatcher_last_help_request_reason: Option<String>,
}

impl MissionRun {
    /// Create a new mission with the given objectives.
    #[must_use]
    pub fn new(objectives: Vec<ObjectiveRef>) -> Self {
        let now = Utc::now();
        Self {
            id: MissionId::new(),
            objective_refs: objectives,
            state: MissionState::Planning,
            policy: MissionPolicy::default(),
            created_at: now,
            updated_at: now,
            next_wake_at: None,
            blocked_reason: None,
            dispatcher_last_tick_at: None,
            dispatcher_last_progress_at: None,
            dispatcher_last_help_request_at: None,
            dispatcher_last_help_request_reason: None,
        }
    }

    /// Create a mission with custom policy.
    #[must_use]
    pub fn with_policy(mut self, policy: MissionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Transition to running state.
    pub fn start(&mut self) {
        self.state = MissionState::Running;
        self.next_wake_at = None;
        self.blocked_reason = None;
        self.updated_at = Utc::now();
    }

    /// Transition to blocked state with reason.
    pub fn block(&mut self, reason: impl Into<String>) {
        self.state = MissionState::Blocked;
        self.blocked_reason = Some(reason.into());
        self.updated_at = Utc::now();
    }

    /// Update the next wake-up time without changing mission state.
    pub fn set_next_wake_at(&mut self, next_wake_at: Option<DateTime<Utc>>) {
        self.next_wake_at = next_wake_at;
        self.updated_at = Utc::now();
    }

    /// Record that the dispatcher ticked this mission.
    pub fn record_dispatch_tick(&mut self) {
        self.dispatcher_last_tick_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Record mission progress seen by the dispatcher.
    pub fn record_dispatch_progress(&mut self) {
        let now = Utc::now();
        self.dispatcher_last_tick_at = Some(now);
        self.dispatcher_last_progress_at = Some(now);
        self.updated_at = now;
    }

    /// Record that the dispatcher escalated to the conductor.
    pub fn record_help_request(&mut self, reason: impl Into<String>) {
        let now = Utc::now();
        self.dispatcher_last_help_request_at = Some(now);
        self.dispatcher_last_help_request_reason = Some(reason.into());
        self.updated_at = now;
    }

    /// Transition to completed state.
    pub fn complete(&mut self) {
        self.state = MissionState::Completed;
        self.next_wake_at = None;
        self.blocked_reason = None;
        self.updated_at = Utc::now();
    }

    /// Transition to failed state with reason.
    pub fn fail(&mut self, reason: impl Into<String>) {
        self.state = MissionState::Failed;
        self.next_wake_at = None;
        self.blocked_reason = Some(reason.into());
        self.updated_at = Utc::now();
    }
}

/// Individual work unit in the mission DAG.
///
/// WorkItems represent specific tasks that can be assigned to agents.
/// They track dependencies and can produce artifacts (PRs, commits, docs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    /// Unique work item identifier
    pub id: WorkItemId,
    /// Parent mission
    pub mission_id: MissionId,
    /// Human-readable title
    pub title: String,
    /// Type of work
    pub kind: WorkKind,
    /// Dependencies (must complete before this item)
    pub depends_on: Vec<WorkItemId>,
    /// Preferred agent role (e.g., "backend", "tester")
    pub owner_role: Option<String>,
    /// Current status
    pub status: WorkStatus,
    /// Assigned agent (if any)
    pub assigned_to: Option<AgentId>,
    /// Artifact references (PR URLs, commit SHAs)
    pub artifact_refs: Vec<String>,
    /// Whether reviewer approval has been recorded for this item
    #[serde(default)]
    pub reviewer_approved: bool,
    /// Source objective reference
    pub source_ref: Option<String>,
    /// When work item was created
    pub created_at: DateTime<Utc>,
    /// When work item was last updated
    pub updated_at: DateTime<Utc>,
}

impl WorkItem {
    /// Create a new work item.
    #[must_use]
    pub fn new(mission_id: MissionId, title: impl Into<String>, kind: WorkKind) -> Self {
        let now = Utc::now();
        Self {
            id: WorkItemId::new(),
            mission_id,
            title: title.into(),
            kind,
            depends_on: Vec::new(),
            owner_role: None,
            status: WorkStatus::Pending,
            assigned_to: None,
            artifact_refs: Vec::new(),
            reviewer_approved: false,
            source_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Add dependencies.
    #[must_use]
    pub fn with_dependencies(mut self, deps: Vec<WorkItemId>) -> Self {
        self.depends_on = deps;
        self
    }

    /// Set owner role.
    #[must_use]
    pub fn with_owner_role(mut self, role: impl Into<String>) -> Self {
        self.owner_role = Some(role.into());
        self
    }

    /// Set source reference.
    #[must_use]
    pub fn with_source_ref(mut self, source: impl Into<String>) -> Self {
        self.source_ref = Some(source.into());
        self
    }

    /// Mark as ready (dependencies satisfied).
    pub fn mark_ready(&mut self) {
        self.status = WorkStatus::Ready;
        self.updated_at = Utc::now();
    }

    /// Assign to an agent.
    pub fn assign(&mut self, agent_id: AgentId) {
        self.assigned_to = Some(agent_id);
        self.status = WorkStatus::Assigned;
        self.reviewer_approved = false;
        self.updated_at = Utc::now();
    }

    /// Mark as running.
    pub fn start(&mut self) {
        self.status = WorkStatus::Running;
        self.updated_at = Utc::now();
    }

    /// Mark as blocked with artifact reference.
    pub fn block(&mut self) {
        self.status = WorkStatus::Blocked;
        self.updated_at = Utc::now();
    }

    /// Record new evidence without completing the work item.
    pub fn record_artifacts(&mut self, artifacts: impl IntoIterator<Item = impl Into<String>>) {
        self.artifact_refs
            .extend(artifacts.into_iter().map(Into::into));
        self.updated_at = Utc::now();
    }

    /// Mark reviewer approval for the item.
    pub fn approve_review(&mut self) {
        self.reviewer_approved = true;
        self.updated_at = Utc::now();
    }

    /// Clear reviewer approval, typically when new changes are required.
    pub fn clear_review_approval(&mut self) {
        self.reviewer_approved = false;
        self.updated_at = Utc::now();
    }

    /// Mark as done with artifact references.
    pub fn complete(&mut self, artifacts: Vec<String>) {
        self.status = WorkStatus::Done;
        self.artifact_refs.extend(artifacts);
        self.updated_at = Utc::now();
    }
}

/// PR/CI monitoring task.
///
/// WatchItems are scheduled checks that monitor external systems (GitHub CI,
/// Bugbot, reviewers) and trigger actions when conditions are met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    /// Unique watch identifier
    pub id: WatchId,
    /// Parent mission
    pub mission_id: MissionId,
    /// Parent work item
    pub work_item_id: WorkItemId,
    /// Type of watch
    pub kind: WatchKind,
    /// Target reference (PR URL, number)
    pub target_ref: String,
    /// Check interval in seconds
    pub interval_secs: u64,
    /// Next due time
    pub next_due_at: DateTime<Utc>,
    /// Current status
    pub status: WatchStatus,
    /// Action on trigger
    pub on_trigger: TriggerAction,
    /// Last check time
    pub last_check_at: Option<DateTime<Utc>>,
    /// Consecutive failures
    pub consecutive_failures: u32,
}

impl WatchItem {
    /// Create a new watch item.
    #[must_use]
    pub fn new(
        mission_id: MissionId,
        work_item_id: WorkItemId,
        kind: WatchKind,
        target_ref: impl Into<String>,
        interval_secs: u64,
    ) -> Self {
        Self {
            id: WatchId::new(),
            mission_id,
            work_item_id,
            kind,
            target_ref: target_ref.into(),
            interval_secs,
            next_due_at: Utc::now(),
            status: WatchStatus::Active,
            on_trigger: TriggerAction::default(),
            last_check_at: None,
            consecutive_failures: 0,
        }
    }

    /// Set the trigger action.
    #[must_use]
    pub fn with_trigger(mut self, action: TriggerAction) -> Self {
        self.on_trigger = action;
        self
    }

    /// Check if watch is due.
    #[must_use]
    pub fn is_due(&self) -> bool {
        self.status == WatchStatus::Active && Utc::now() >= self.next_due_at
    }

    /// Record a successful check.
    pub fn record_check(&mut self) {
        self.last_check_at = Some(Utc::now());
        self.next_due_at = Utc::now() + chrono::Duration::seconds(self.interval_secs as i64);
        self.consecutive_failures = 0;
    }

    /// Record a failed check.
    pub fn record_failure(&mut self) {
        self.last_check_at = Some(Utc::now());
        self.consecutive_failures += 1;
        // Backoff: 1m, 2m, 5m, then stay at 5m
        let backoff_secs = match self.consecutive_failures {
            1 => 60,
            2 => 120,
            _ => 300,
        };
        self.next_due_at = Utc::now() + chrono::Duration::seconds(backoff_secs);
    }

    /// Snooze the watch.
    pub fn snooze(&mut self, duration_secs: u64) {
        self.status = WatchStatus::Snoozed;
        self.next_due_at = Utc::now() + chrono::Duration::seconds(duration_secs as i64);
    }

    /// Mark as done.
    pub fn complete(&mut self) {
        self.status = WatchStatus::Done;
    }
}

/// Operator-to-dispatcher note for a mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionControlMessage {
    /// Unique control message identifier
    pub id: String,
    /// Parent mission
    pub mission_id: MissionId,
    /// Human-readable sender label
    pub sender: String,
    /// Free-form note/directive body
    pub body: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Processing timestamp when dispatcher consumes it
    #[serde(default)]
    pub processed_at: Option<DateTime<Utc>>,
}

impl MissionControlMessage {
    /// Create a new control message.
    #[must_use]
    pub fn new(mission_id: MissionId, sender: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            mission_id,
            sender: sender.into(),
            body: body.into(),
            created_at: Utc::now(),
            processed_at: None,
        }
    }

    /// Mark the control message as processed.
    pub fn mark_processed(&mut self) {
        self.processed_at = Some(Utc::now());
    }

    /// Whether the message is still pending dispatcher handling.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.processed_at.is_none()
    }
}
