/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Watch Engine: PR/CI/Bugbot monitoring for mission mode.
//!
//! The watch engine polls GitHub for external events and triggers
//! actions when conditions are met:
//! - PR check status (CI pass/fail)
//! - Bugbot comments (automated vulnerability reports)
//! - Review comments (human review feedback)
//! - Mergeability status
//!
//! # Design
//!
//! Watch items are scheduled checks with:
//! - Configurable polling interval (default 180s)
//! - Backoff on failures (1m, 2m, 5m, then stay at 5m)
//! - Trigger actions (create fix task, notify reviewer, advance pipeline)

use std::collections::HashMap;

use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, info, instrument, warn};

use crate::agent::AgentId;
use crate::channel::Channel;
use crate::error::Result;
use crate::message::{Message, MessageType};
use crate::mission::storage::MissionStorage;
use crate::mission::types::{MissionId, TriggerAction, WatchId, WatchItem, WatchKind, WatchStatus};
use crate::task::Task;

// ==================== Watch Check Results ====================

/// Result of checking a PR's status.
#[derive(Debug, Clone)]
pub struct PrCheckResult {
    /// PR number
    pub pr_number: u64,
    /// Owner/repo string
    pub repo: String,
    /// Overall status (success/failure/pending)
    pub status: CheckStatus,
    /// Individual check details
    pub checks: Vec<CheckDetail>,
    /// Whether PR is mergeable
    pub mergeable: bool,
    /// Review status
    pub review_state: ReviewState,
    /// Any blocking comments (bugbot, review requests)
    pub blocking_comments: Vec<String>,
}

/// Status of CI checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// All checks passed
    Success,
    /// One or more checks failed
    Failure,
    /// Checks still running
    Pending,
    /// No checks found
    Unknown,
}

/// Individual check detail.
#[derive(Debug, Clone)]
pub struct CheckDetail {
    /// Check name
    pub name: String,
    /// Check status
    pub status: CheckStatus,
    /// Failure details (if any)
    pub details: Option<String>,
}

/// Review state of a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// Approved by reviewers
    Approved,
    /// Changes requested
    ChangesRequested,
    /// Pending review
    Pending,
    /// No review required
    NotRequired,
}

/// Result of processing a watch item.
#[derive(Debug, Clone)]
pub struct WatchTickResult {
    /// Watch item ID
    pub watch_id: WatchId,
    /// Mission ID
    pub mission_id: MissionId,
    /// Whether the watch triggered an action
    pub triggered: bool,
    /// Action taken (if triggered)
    pub action_taken: Option<TriggerAction>,
    /// New status of the watch
    pub new_status: WatchStatus,
    /// Error message (if check failed)
    pub error: Option<String>,
}

/// Aggregate result of watch engine tick.
#[derive(Debug, Clone, Default)]
pub struct WatchEngineTickResult {
    /// Total watches processed
    pub watches_processed: usize,
    /// Watches that triggered actions
    pub watches_triggered: usize,
    /// Watches that completed
    pub watches_completed: usize,
    /// Watches that failed (with backoff)
    pub watches_failed: usize,
    /// Individual results
    pub results: Vec<WatchTickResult>,
}

// ==================== GitHub Client Interface ====================

/// Trait for GitHub API operations.
///
/// This trait allows mocking GitHub calls in tests.
#[async_trait::async_trait]
pub trait GitHubClient: Send + Sync {
    /// Get PR check status.
    async fn get_pr_checks(&self, owner: &str, repo: &str, pr_number: u64)
    -> Result<PrCheckResult>;

    /// Get PR review comments.
    async fn get_pr_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<ReviewComment>>;

    /// Get bugbot comments on a PR.
    async fn get_bugbot_comments(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<BugbotComment>>;
}

/// A review comment on a PR.
#[derive(Debug, Clone)]
pub struct ReviewComment {
    /// Comment author
    pub author: String,
    /// Comment body
    pub body: String,
    /// Whether this is actionable (not just acknowledgment)
    pub is_actionable: bool,
}

/// A bugbot/security bot comment.
#[derive(Debug, Clone)]
pub struct BugbotComment {
    /// Bot name
    pub bot_name: String,
    /// Severity level
    pub severity: String,
    /// Issue description
    pub description: String,
    /// File path (if specific)
    pub file_path: Option<String>,
}

// ==================== Watch Engine ====================

/// Configuration for the watch engine.
#[derive(Debug, Clone)]
pub struct WatchEngineConfig {
    /// Default check interval in seconds
    pub default_interval_secs: u64,
    /// Maximum consecutive failures before marking blocked
    pub max_failures: u32,
}

impl Default for WatchEngineConfig {
    fn default() -> Self {
        Self {
            default_interval_secs: 180,
            max_failures: 5,
        }
    }
}

/// The watch engine monitors PR/CI status and triggers actions.
///
/// It processes due watch items by:
/// 1. Checking GitHub for status updates
/// 2. Evaluating trigger conditions
/// 3. Executing trigger actions (create task, notify, advance)
/// 4. Updating watch status and next check time
pub struct WatchEngine<G: GitHubClient> {
    storage: MissionStorage,
    channel: Channel,
    github: G,
    config: WatchEngineConfig,
}

impl<G: GitHubClient> WatchEngine<G> {
    /// Create a new watch engine.
    pub fn new(
        storage: MissionStorage,
        channel: Channel,
        github: G,
        config: WatchEngineConfig,
    ) -> Self {
        Self {
            storage,
            channel,
            github,
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(storage: MissionStorage, channel: Channel, github: G) -> Self {
        Self::new(storage, channel, github, WatchEngineConfig::default())
    }

    /// Run a single tick of the watch engine.
    ///
    /// Processes all due watch items across active missions.
    #[instrument(skip(self))]
    pub async fn tick(&self) -> Result<WatchEngineTickResult> {
        let due_watches = self.storage.list_due_watches().await?;
        self.process_due_watches(due_watches).await
    }

    /// Run a watch tick for the provided mission IDs only.
    #[instrument(skip(self, mission_ids))]
    pub async fn tick_missions(&self, mission_ids: &[MissionId]) -> Result<WatchEngineTickResult> {
        let mut due_watches = Vec::new();
        for mission_id in mission_ids {
            for watch in self.storage.list_watch_items(*mission_id).await? {
                if watch.is_due() {
                    due_watches.push(watch);
                }
            }
        }
        self.process_due_watches(due_watches).await
    }

    async fn process_due_watches(&self, due_watches: Vec<WatchItem>) -> Result<WatchEngineTickResult> {
        let mut result = WatchEngineTickResult::default();
        debug!("Watch engine tick: {} due watches", due_watches.len());

        for watch in due_watches {
            match self.process_watch(&watch).await {
                Ok(tick_result) => {
                    result.watches_processed += 1;
                    if tick_result.triggered {
                        result.watches_triggered += 1;
                    }
                    if tick_result.new_status == WatchStatus::Done {
                        result.watches_completed += 1;
                    }
                    if tick_result.error.is_some() {
                        result.watches_failed += 1;
                    }
                    result.results.push(tick_result);
                }
                Err(e) => {
                    warn!("Error processing watch {}: {}", watch.id, e);
                    result.watches_failed += 1;
                }
            }
        }

        info!(
            "Watch engine tick: {} processed, {} triggered, {} completed, {} failed",
            result.watches_processed,
            result.watches_triggered,
            result.watches_completed,
            result.watches_failed
        );

        Ok(result)
    }

    /// Process a single watch item.
    #[instrument(skip(self), fields(watch_id = %watch.id, kind = ?watch.kind))]
    async fn process_watch(&self, watch: &WatchItem) -> Result<WatchTickResult> {
        let mut result = WatchTickResult {
            watch_id: watch.id,
            mission_id: watch.mission_id,
            triggered: false,
            action_taken: None,
            new_status: watch.status,
            error: None,
        };

        // Parse target reference (format: owner/repo#pr_number)
        let Some((owner, repo, pr_number)) = parse_pr_ref(&watch.target_ref) else {
            warn!("Invalid PR reference: {}", watch.target_ref);
            result.error = Some(format!("Invalid PR reference: {}", watch.target_ref));
            return Ok(result);
        };

        // Check based on watch kind
        let check_result = match watch.kind {
            WatchKind::PrChecks => self.check_pr_status(&owner, &repo, pr_number).await,
            WatchKind::BugbotComments => self.check_bugbot(&owner, &repo, pr_number).await,
            WatchKind::ReviewComments => self.check_reviews(&owner, &repo, pr_number).await,
            WatchKind::Mergeability => self.check_mergeability(&owner, &repo, pr_number).await,
        };

        match check_result {
            Ok((triggered, should_complete)) => {
                let mut updated_watch = watch.clone();

                if triggered {
                    result.triggered = true;
                    result.action_taken = Some(watch.on_trigger);

                    // Execute trigger action
                    self.execute_trigger_action(watch).await?;

                    if !should_complete {
                        updated_watch.snooze(self.config.default_interval_secs);
                    }
                }

                if should_complete {
                    updated_watch.complete();
                    result.new_status = WatchStatus::Done;
                } else {
                    updated_watch.record_check();
                }

                self.storage.save_watch_item(&updated_watch).await?;
            }
            Err(e) => {
                let mut updated_watch = watch.clone();
                updated_watch.record_failure();

                if updated_watch.consecutive_failures >= self.config.max_failures {
                    // Mark work item as blocked after too many failures
                    self.mark_work_blocked(watch, &format!("Watch check failed: {}", e))
                        .await?;
                }

                self.storage.save_watch_item(&updated_watch).await?;
                result.error = Some(e.to_string());
            }
        }

        Ok(result)
    }

    /// Check PR CI status.
    async fn check_pr_status(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<(bool, bool)> {
        let pr_result = self.github.get_pr_checks(owner, repo, pr_number).await?;

        match pr_result.status {
            CheckStatus::Success => {
                // CI passed - watch complete
                Ok((false, true))
            }
            CheckStatus::Failure => {
                // CI failed - trigger action
                Ok((true, false))
            }
            CheckStatus::Pending | CheckStatus::Unknown => {
                // Still running - no action
                Ok((false, false))
            }
        }
    }

    /// Check for bugbot comments.
    async fn check_bugbot(&self, owner: &str, repo: &str, pr_number: u64) -> Result<(bool, bool)> {
        let comments = self
            .github
            .get_bugbot_comments(owner, repo, pr_number)
            .await?;

        if comments.is_empty() {
            // No bugbot issues yet - continue watching (bugbot may not have posted yet)
            // The watch should only complete when CI passes and we've confirmed no bugbot issues
            Ok((false, false))
        } else {
            // Bugbot found issues - trigger action
            Ok((true, false))
        }
    }

    /// Check for review comments.
    async fn check_reviews(&self, owner: &str, repo: &str, pr_number: u64) -> Result<(bool, bool)> {
        let reviews = self.github.get_pr_reviews(owner, repo, pr_number).await?;

        let has_actionable = reviews.iter().any(|r| r.is_actionable);
        if has_actionable {
            // Actionable review comments - trigger action
            Ok((true, false))
        } else {
            // No actionable comments - continue watching
            Ok((false, false))
        }
    }

    /// Check PR mergeability.
    async fn check_mergeability(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<(bool, bool)> {
        let pr_result = self.github.get_pr_checks(owner, repo, pr_number).await?;

        if pr_result.mergeable && pr_result.status == CheckStatus::Success {
            // Ready to merge - watch complete, trigger advance
            Ok((true, true))
        } else {
            // Not mergeable yet
            Ok((false, false))
        }
    }

    /// Execute a trigger action.
    #[instrument(skip(self))]
    async fn execute_trigger_action(&self, watch: &WatchItem) -> Result<()> {
        match watch.on_trigger {
            TriggerAction::CreateFixTask => {
                self.create_fix_task(watch).await?;
            }
            TriggerAction::NotifyReviewer => {
                self.notify_reviewer(watch).await?;
            }
            TriggerAction::AdvancePipeline => {
                self.advance_pipeline(watch).await?;
            }
        }

        self.storage
            .log_event(
                watch.mission_id,
                &format!(
                    "Watch {} triggered action {:?} for {}",
                    watch.id, watch.on_trigger, watch.target_ref
                ),
            )
            .await?;

        Ok(())
    }

    /// Create a fix task for a work item.
    async fn create_fix_task(&self, watch: &WatchItem) -> Result<()> {
        // Get the parent work item
        let Some(work_item) = self
            .storage
            .get_work_item(watch.mission_id, watch.work_item_id)
            .await?
        else {
            warn!("Work item {} not found for watch", watch.work_item_id);
            return Ok(());
        };

        // If the work item has an assigned agent, send them a fix task
        if let Some(agent_id) = work_item.assigned_to {
            let mut task = Task::new(format!(
                "[Mission Fix Required] {}\n\nWatch type: {:?}\nTarget: {}\n\nInvestigate the reported issue, update the branch/PR, and complete this task with the refreshed PR URL or ref.",
                work_item.title, watch.kind, watch.target_ref
            ))
            .with_tags([
                "mission-fix-task".to_string(),
                format!("mission:{}", watch.mission_id),
                format!("work-item:{}", watch.work_item_id),
                format!("watch:{}", watch.id),
            ]);
            task.assign(agent_id);
            let task_id = task.id;
            self.channel.set_task(&task).await?;

            let msg = Message::new(
                AgentId::supervisor(),
                agent_id,
                MessageType::TaskAssign {
                    task_id: task_id.to_string(),
                },
            );
            self.channel.send(&msg).await?;

            self.storage
                .log_event(
                    watch.mission_id,
                    &format!(
                        "Created fix task {} for work item '{}' due to {:?}",
                        task_id, work_item.title, watch.kind
                    ),
                )
                .await?;

            info!(
                "Created fix task for work item '{}' assigned to agent {:?}",
                work_item.title, agent_id
            );
        }

        Ok(())
    }

    /// Notify the reviewer about a work item.
    async fn notify_reviewer(&self, watch: &WatchItem) -> Result<()> {
        // Find reviewer agents
        let agents = self.channel.list_agents().await?;
        let reviewers: Vec<_> = agents
            .iter()
            .filter(|a| {
                let name = a.name.to_lowercase();
                name.contains("review") || name.contains("audit")
            })
            .collect();

        if reviewers.is_empty() {
            warn!("No reviewer agents found to notify");
            return Ok(());
        }

        let notification = format!(
            "[Mission Notification] Watch triggered for {}\n\nType: {:?}\nTarget: {}",
            watch.work_item_id, watch.kind, watch.target_ref
        );

        for reviewer in reviewers {
            let msg = Message::new(
                AgentId::supervisor(),
                reviewer.id,
                MessageType::Informational {
                    summary: notification.clone(),
                },
            );
            self.channel.send(&msg).await?;
        }

        info!("Notified reviewers about watch trigger");
        Ok(())
    }

    /// Advance the pipeline (mark work item ready for next step).
    async fn advance_pipeline(&self, watch: &WatchItem) -> Result<()> {
        // Get the work item and mark it as done
        if let Some(mut work_item) = self
            .storage
            .get_work_item(watch.mission_id, watch.work_item_id)
            .await?
        {
            work_item.complete(vec![watch.target_ref.clone()]);
            self.storage.save_work_item(&work_item).await?;

            self.storage
                .log_event(
                    watch.mission_id,
                    &format!(
                        "Work item '{}' completed via pipeline advance",
                        work_item.title
                    ),
                )
                .await?;

            info!(
                "Advanced pipeline: work item '{}' completed",
                work_item.title
            );
        }

        Ok(())
    }

    /// Mark a work item as blocked.
    async fn mark_work_blocked(&self, watch: &WatchItem, reason: &str) -> Result<()> {
        if let Some(mut work_item) = self
            .storage
            .get_work_item(watch.mission_id, watch.work_item_id)
            .await?
        {
            work_item.block();
            self.storage.save_work_item(&work_item).await?;

            self.storage
                .log_event(
                    watch.mission_id,
                    &format!("Work item '{}' blocked: {}", work_item.title, reason),
                )
                .await?;

            warn!("Blocked work item '{}': {}", work_item.title, reason);
        }

        Ok(())
    }
}

/// GitHub client backed by the local `gh` CLI.
///
/// The runtime uses `gh`, consistent with other mission commands, while tests
/// continue to rely on `MockGitHubClient`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GhCliGitHubClient;

#[derive(serde::Deserialize)]
struct GhPrView {
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<Value>,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
    #[serde(default)]
    reviews: Vec<GhReview>,
}

#[derive(serde::Deserialize)]
struct GhReview {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    author: Option<GhNamedAuthor>,
}

#[derive(serde::Deserialize)]
struct GhNamedAuthor {
    #[serde(default)]
    login: Option<String>,
}

#[derive(serde::Deserialize)]
struct GhIssueComment {
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    user: Option<GhNamedAuthor>,
}

impl GhCliGitHubClient {
    async fn gh_json<T: serde::de::DeserializeOwned>(args: &[String]) -> Result<T> {
        let output = Command::new("gh").args(args).output().await?;
        if !output.status.success() {
            return Err(crate::error::Error::Config(format!(
                "gh command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    fn parse_check_status(checks: &[Value]) -> (CheckStatus, Vec<CheckDetail>) {
        let mut overall = CheckStatus::Unknown;
        let mut details = Vec::new();

        for check in checks {
            let name = check
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| check.get("context").and_then(Value::as_str))
                .unwrap_or("check")
                .to_string();
            let raw = check
                .get("conclusion")
                .and_then(Value::as_str)
                .or_else(|| check.get("state").and_then(Value::as_str))
                .or_else(|| check.get("status").and_then(Value::as_str))
                .unwrap_or("unknown")
                .to_lowercase();
            let status = match raw.as_str() {
                "success" => CheckStatus::Success,
                "neutral" | "skipped" => CheckStatus::Success,
                "queued" | "pending" | "in_progress" | "expected" => CheckStatus::Pending,
                "failure" | "failed" | "timed_out" | "action_required" | "cancelled" => {
                    CheckStatus::Failure
                }
                _ => CheckStatus::Unknown,
            };

            overall = match (overall, status) {
                (_, CheckStatus::Failure) => CheckStatus::Failure,
                (CheckStatus::Unknown, other) => other,
                (CheckStatus::Success, CheckStatus::Pending)
                | (CheckStatus::Pending, CheckStatus::Success) => CheckStatus::Pending,
                (current, _) => current,
            };

            details.push(CheckDetail {
                name,
                status,
                details: None,
            });
        }

        (overall, details)
    }

    fn parse_mergeable(value: Option<String>) -> bool {
        matches!(
            value.as_deref().map(str::to_ascii_uppercase).as_deref(),
            Some("MERGEABLE") | Some("CLEAN") | Some("HAS_HOOKS") | Some("UNSTABLE")
        )
    }

    fn parse_review_state(value: Option<String>) -> ReviewState {
        match value
            .as_deref()
            .map(str::to_ascii_uppercase)
            .as_deref()
        {
            Some("APPROVED") => ReviewState::Approved,
            Some("CHANGES_REQUESTED") => ReviewState::ChangesRequested,
            Some("REVIEW_REQUIRED") => ReviewState::Pending,
            _ => ReviewState::Pending,
        }
    }
}

#[async_trait::async_trait]
impl GitHubClient for GhCliGitHubClient {
    async fn get_pr_checks(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<PrCheckResult> {
        let payload: GhPrView = Self::gh_json(&[
            "pr".to_string(),
            "view".to_string(),
            pr_number.to_string(),
            "--repo".to_string(),
            format!("{owner}/{repo}"),
            "--json".to_string(),
            "statusCheckRollup,mergeable,reviewDecision,reviews".to_string(),
        ])
        .await?;

        let (status, checks) = Self::parse_check_status(&payload.status_check_rollup);
        let review_state = Self::parse_review_state(payload.review_decision);
        let blocking_comments = payload
            .reviews
            .iter()
            .filter_map(|review| {
                let state = review.state.as_deref()?.to_ascii_uppercase();
                if state == "CHANGES_REQUESTED" {
                    review.body.clone()
                } else {
                    None
                }
            })
            .collect();

        Ok(PrCheckResult {
            pr_number,
            repo: format!("{owner}/{repo}"),
            status,
            checks,
            mergeable: Self::parse_mergeable(payload.mergeable),
            review_state,
            blocking_comments,
        })
    }

    async fn get_pr_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<ReviewComment>> {
        let payload: GhPrView = Self::gh_json(&[
            "pr".to_string(),
            "view".to_string(),
            pr_number.to_string(),
            "--repo".to_string(),
            format!("{owner}/{repo}"),
            "--json".to_string(),
            "reviews,reviewDecision".to_string(),
        ])
            .await?;

        Ok(payload
            .reviews
            .into_iter()
            .map(|review| {
                let state = review.state.unwrap_or_default().to_ascii_uppercase();
                let body = review.body.unwrap_or_default();
                ReviewComment {
                    author: review
                        .author
                        .and_then(|author| author.login)
                        .unwrap_or_else(|| "reviewer".to_string()),
                    is_actionable: state == "CHANGES_REQUESTED",
                    body,
                }
            })
            .collect())
    }

    async fn get_bugbot_comments(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<BugbotComment>> {
        let comments: Vec<GhIssueComment> = Self::gh_json(&[
            "api".to_string(),
            format!("repos/{owner}/{repo}/issues/{pr_number}/comments"),
        ])
            .await?;

        Ok(comments
            .into_iter()
            .filter_map(|comment| {
                let author = comment.user.and_then(|user| user.login)?;
                let body = comment.body.unwrap_or_default();
                let lower_author = author.to_ascii_lowercase();
                let lower_body = body.to_ascii_lowercase();
                if lower_author.contains("bugbot")
                    || lower_author.contains("cursor")
                    || lower_body.contains("bugbot")
                    || lower_body.contains("cursor bugbot")
                {
                    Some(BugbotComment {
                        bot_name: author,
                        severity: if lower_body.contains("critical") {
                            "critical".to_string()
                        } else if lower_body.contains("high") {
                            "high".to_string()
                        } else {
                            "unknown".to_string()
                        },
                        description: body,
                        file_path: None,
                    })
                } else {
                    None
                }
            })
            .collect())
    }
}

// ==================== Helper Functions ====================

/// Parse a PR reference string (owner/repo#123).
fn parse_pr_ref(target_ref: &str) -> Option<(String, String, u64)> {
    // Format: owner/repo#pr_number or https://github.com/owner/repo/pull/123
    if target_ref.contains("github.com") {
        // URL format
        let parts: Vec<&str> = target_ref.split('/').collect();
        if parts.len() >= 5 {
            let owner = parts[parts.len() - 4].to_string();
            let repo = parts[parts.len() - 3].to_string();
            let pr_number = parts[parts.len() - 1].parse().ok()?;
            return Some((owner, repo, pr_number));
        }
    } else {
        // Short format: owner/repo#123
        let (repo_part, pr_part) = target_ref.split_once('#')?;
        let (owner, repo) = repo_part.split_once('/')?;
        let pr_number = pr_part.parse().ok()?;
        return Some((owner.to_string(), repo.to_string(), pr_number));
    }
    None
}

// ==================== Mock GitHub Client ====================

/// A mock GitHub client for testing.
#[derive(Default)]
pub struct MockGitHubClient {
    /// Predefined PR check results
    pub pr_checks: HashMap<String, PrCheckResult>,
    /// Predefined review comments
    pub reviews: HashMap<String, Vec<ReviewComment>>,
    /// Predefined bugbot comments
    pub bugbot_comments: HashMap<String, Vec<BugbotComment>>,
}

impl MockGitHubClient {
    /// Create a new mock client.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set PR check result for a PR.
    pub fn set_pr_checks(&mut self, owner: &str, repo: &str, pr: u64, result: PrCheckResult) {
        self.pr_checks
            .insert(format!("{}/{}#{}", owner, repo, pr), result);
    }

    fn get_key(owner: &str, repo: &str, pr: u64) -> String {
        format!("{}/{}#{}", owner, repo, pr)
    }
}

#[async_trait::async_trait]
impl GitHubClient for MockGitHubClient {
    async fn get_pr_checks(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<PrCheckResult> {
        let key = Self::get_key(owner, repo, pr_number);
        self.pr_checks.get(&key).cloned().ok_or_else(|| {
            crate::error::Error::Config(format!("No mock PR check result for {}", key))
        })
    }

    async fn get_pr_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<ReviewComment>> {
        let key = Self::get_key(owner, repo, pr_number);
        Ok(self.reviews.get(&key).cloned().unwrap_or_default())
    }

    async fn get_bugbot_comments(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> Result<Vec<BugbotComment>> {
        let key = Self::get_key(owner, repo, pr_number);
        Ok(self.bugbot_comments.get(&key).cloned().unwrap_or_default())
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_ref_short_format() {
        let (owner, repo, pr) = parse_pr_ref("redis-field-engineering/tinytown#23").unwrap();
        assert_eq!(owner, "redis-field-engineering");
        assert_eq!(repo, "tinytown");
        assert_eq!(pr, 23);
    }

    #[test]
    fn test_parse_pr_ref_url_format() {
        let (owner, repo, pr) =
            parse_pr_ref("https://github.com/redis-field-engineering/tinytown/pull/23").unwrap();
        assert_eq!(owner, "redis-field-engineering");
        assert_eq!(repo, "tinytown");
        assert_eq!(pr, 23);
    }

    #[test]
    fn test_parse_pr_ref_invalid() {
        assert!(parse_pr_ref("invalid").is_none());
        assert!(parse_pr_ref("owner/repo").is_none());
    }

    #[test]
    fn test_watch_engine_config_defaults() {
        let config = WatchEngineConfig::default();
        assert_eq!(config.default_interval_secs, 180);
        assert_eq!(config.max_failures, 5);
    }

    #[test]
    fn test_mock_github_client() {
        let mut client = MockGitHubClient::new();

        let pr_result = PrCheckResult {
            pr_number: 1,
            repo: "test/repo".to_string(),
            status: CheckStatus::Success,
            checks: vec![],
            mergeable: true,
            review_state: ReviewState::Approved,
            blocking_comments: vec![],
        };

        client.set_pr_checks("test", "repo", 1, pr_result);
        assert!(client.pr_checks.contains_key("test/repo#1"));
    }
}
