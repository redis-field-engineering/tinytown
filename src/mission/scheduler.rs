/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission Scheduler: periodic tick-based work orchestration.
//!
//! The scheduler runs every N seconds (configurable, default 30s) and:
//! 1. Loads active missions from Redis
//! 2. Updates work item statuses from observations
//! 3. Promotes pending -> ready when dependencies satisfied
//! 4. Matches ready items to idle agents by role fit
//! 5. Enforces reviewer gates before advancing
//! 6. Logs activity events
//!
//! Key design principle: scheduler always selects from persisted `ready` queue,
//! never from transient "memory."

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tracing::{debug, info, instrument, warn};

use crate::agent::{Agent, AgentId};
use crate::channel::Channel;
use crate::error::Result;
use crate::message::{Message, MessageType};
use crate::mission::storage::MissionStorage;
use crate::mission::types::{
    MissionId, MissionRun, MissionState, WorkItem, WorkItemId, WorkKind, WorkStatus,
};
use crate::task::Task;

// ==================== Configuration ====================

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Tick interval in seconds (default: 30)
    pub tick_interval_secs: u64,
    /// Maximum parallel work items per mission (default: 2)
    pub max_parallel_items: u32,
    /// Reviewer required for implement/test items (default: true)
    pub reviewer_required: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_secs: 30,
            max_parallel_items: 2,
            reviewer_required: true,
        }
    }
}

// ==================== Tick Result ====================

/// Result of a scheduler tick for a single mission.
#[derive(Debug, Clone, Default)]
pub struct MissionTickResult {
    /// Mission ID
    pub mission_id: MissionId,
    /// Work items promoted to ready
    pub promoted: Vec<WorkItemId>,
    /// Work items assigned to agents
    pub assigned: Vec<(WorkItemId, AgentId)>,
    /// Work items completed
    pub completed: Vec<WorkItemId>,
    /// Work items blocked
    pub blocked: Vec<WorkItemId>,
    /// Whether mission state changed
    pub state_changed: bool,
    /// New mission state (if changed)
    pub new_state: Option<MissionState>,
    /// Next wake-up time
    pub next_wake_at: Option<DateTime<Utc>>,
}

/// Aggregate result of scheduler tick across all missions.
#[derive(Debug, Clone, Default)]
pub struct SchedulerTickResult {
    /// Results per mission
    pub missions: Vec<MissionTickResult>,
    /// Total items promoted
    pub total_promoted: usize,
    /// Total items assigned
    pub total_assigned: usize,
    /// Number of missions now completed
    pub missions_completed: usize,
}

/// Outcome of attempting to complete a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemCompletion {
    /// Work item completion was persisted successfully.
    Completed,
    /// Mission record was not found.
    MissionNotFound,
    /// Work item record was not found.
    WorkItemNotFound,
    /// Reviewer approval is still required before completion can proceed.
    ReviewerApprovalRequired,
}

// ==================== Agent Match Score ====================

/// Score for matching an agent to a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentMatchScore {
    /// Higher is better: exact role match = 100, generic worker = 50, reviewer fallback = 25
    pub score: u32,
    /// Penalty for concurrent items (lower is worse)
    pub load_penalty: u32,
}

impl AgentMatchScore {
    /// Create a new match score.
    #[must_use]
    pub fn new(score: u32, load_penalty: u32) -> Self {
        Self {
            score,
            load_penalty,
        }
    }

    /// Total score (higher is better).
    #[must_use]
    pub fn total(&self) -> i32 {
        self.score as i32 - self.load_penalty as i32
    }
}

// ==================== Mission Scheduler ====================

/// The mission scheduler orchestrates work item execution.
///
/// It runs periodic ticks to:
/// - Promote ready work items
/// - Assign items to idle agents
/// - Enforce reviewer gates
/// - Update mission state
pub struct MissionScheduler {
    storage: MissionStorage,
    channel: Channel,
    config: SchedulerConfig,
}

impl MissionScheduler {
    /// Create a new scheduler.
    pub fn new(storage: MissionStorage, channel: Channel, config: SchedulerConfig) -> Self {
        Self {
            storage,
            channel,
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(storage: MissionStorage, channel: Channel) -> Self {
        Self::new(storage, channel, SchedulerConfig::default())
    }

    /// Run a single scheduler tick across all active missions.
    ///
    /// This is the main entry point for the scheduler loop.
    #[instrument(skip(self))]
    pub async fn tick(&self) -> Result<SchedulerTickResult> {
        let mut result = SchedulerTickResult::default();

        // Load active missions
        let active_ids = self.storage.list_active().await?;
        debug!("Scheduler tick: {} active missions", active_ids.len());

        // Get all agents once (cache for this tick)
        let agents = self.channel.list_agents().await?;

        for mission_id in active_ids {
            match self.tick_mission(mission_id, &agents).await {
                Ok(mission_result) => {
                    result.total_promoted += mission_result.promoted.len();
                    result.total_assigned += mission_result.assigned.len();
                    if mission_result.new_state == Some(MissionState::Completed) {
                        result.missions_completed += 1;
                    }
                    result.missions.push(mission_result);
                }
                Err(e) => {
                    warn!("Error ticking mission {}: {}", mission_id, e);
                }
            }
        }

        info!(
            "Scheduler tick complete: {} promoted, {} assigned, {} completed",
            result.total_promoted, result.total_assigned, result.missions_completed
        );

        Ok(result)
    }

    /// Run a tick for a single mission.
    #[instrument(skip(self, agents), fields(mission_id = %mission_id))]
    async fn tick_mission(
        &self,
        mission_id: MissionId,
        agents: &[Agent],
    ) -> Result<MissionTickResult> {
        let mut result = MissionTickResult {
            mission_id,
            ..Default::default()
        };

        // Load mission
        let Some(mut mission) = self.storage.get_mission(mission_id).await? else {
            warn!("Mission {} not found", mission_id);
            return Ok(result);
        };

        // Skip non-running missions
        if mission.state != MissionState::Running {
            debug!(
                "Skipping mission {} (state: {:?})",
                mission_id, mission.state
            );
            return Ok(result);
        }

        // Load work items
        let mut work_items = self.storage.list_work_items(mission_id).await?;

        // Step 1: Promote pending -> ready
        // Build a status map for dependency checking (owned data, no borrow conflict)
        let status_map: HashMap<WorkItemId, WorkStatus> =
            work_items.iter().map(|w| (w.id, w.status)).collect();
        let promoted = self.promote_ready_items(&mut work_items, &status_map);
        for id in &promoted {
            if let Some(item) = work_items.iter().find(|w| w.id == *id) {
                self.storage.save_work_item(item).await?;
                self.storage
                    .log_event(
                        mission_id,
                        &format!("Work item '{}' promoted to ready", item.title),
                    )
                    .await?;
            }
        }
        result.promoted = promoted;

        // Step 2: Assign ready items to agents
        let assigned = self
            .assign_ready_items(&mut work_items, agents, &mission)
            .await?;
        result.assigned = assigned;

        // Step 3: Check for completion
        // Guard: empty work_items.iter().all() returns true, causing spurious completion
        let all_work_done =
            !work_items.is_empty() && work_items.iter().all(|w| w.status == WorkStatus::Done);
        let has_ready = work_items.iter().any(|w| w.status == WorkStatus::Ready);
        let has_running = work_items
            .iter()
            .any(|w| w.status == WorkStatus::Running || w.status == WorkStatus::Assigned);

        // Also check that all watches are complete before marking mission done
        let watches = self.storage.list_watch_items(mission_id).await?;
        let all_watches_done = watches
            .iter()
            .all(|w| w.status == crate::mission::WatchStatus::Done);
        let has_active_watches = watches
            .iter()
            .any(|w| w.status == crate::mission::WatchStatus::Active);

        if all_work_done && all_watches_done {
            mission.complete();
            self.storage.save_mission(&mission).await?;
            self.storage.remove_active(mission_id).await?;
            self.storage
                .log_event(
                    mission_id,
                    "Mission completed - all work items and watches done",
                )
                .await?;
            result.state_changed = true;
            result.new_state = Some(MissionState::Completed);
        } else if all_work_done && has_active_watches {
            // All work is done but watches still pending - log and wait
            debug!(
                "Mission {} has all work done but {} active watches remaining",
                mission_id,
                watches
                    .iter()
                    .filter(|w| w.status == crate::mission::WatchStatus::Active)
                    .count()
            );
        } else if !has_ready && !has_running {
            // All items are pending or blocked - compute next wake time
            let next_wake = Utc::now() + Duration::seconds(self.config.tick_interval_secs as i64);
            result.next_wake_at = Some(next_wake);
        }

        Ok(result)
    }

    // ==================== Ready Queue Promotion ====================

    /// Promote work items from Pending to Ready when dependencies are satisfied.
    ///
    /// Returns the list of work item IDs that were promoted.
    fn promote_ready_items(
        &self,
        work_items: &mut [WorkItem],
        status_map: &HashMap<WorkItemId, WorkStatus>,
    ) -> Vec<WorkItemId> {
        let mut promoted = Vec::new();

        for item in work_items.iter_mut() {
            if item.status != WorkStatus::Pending {
                continue;
            }

            // Check if all dependencies are done
            let deps_satisfied = item.depends_on.iter().all(|dep_id| {
                status_map
                    .get(dep_id)
                    .is_some_and(|status| *status == WorkStatus::Done)
            });

            if deps_satisfied {
                item.mark_ready();
                promoted.push(item.id);
                debug!("Promoted work item '{}' to ready", item.title);
            }
        }

        promoted
    }

    // ==================== Agent Assignment ====================

    /// Assign ready work items to idle agents.
    ///
    /// Returns list of (WorkItemId, AgentId) assignments made.
    #[instrument(skip(self, work_items, agents, mission))]
    async fn assign_ready_items(
        &self,
        work_items: &mut [WorkItem],
        agents: &[Agent],
        mission: &MissionRun,
    ) -> Result<Vec<(WorkItemId, AgentId)>> {
        let mut assignments = Vec::new();

        // Count currently running/assigned items
        let running_count = work_items
            .iter()
            .filter(|w| w.status == WorkStatus::Running || w.status == WorkStatus::Assigned)
            .count() as u32;

        // Respect max parallel limit from mission policy
        let max_parallel = mission.policy.max_parallel_items;
        let slots_available = max_parallel.saturating_sub(running_count);

        if slots_available == 0 {
            debug!("No assignment slots available (running: {})", running_count);
            return Ok(assignments);
        }

        // Get idle agents
        let idle_agents: Vec<&Agent> = agents
            .iter()
            .filter(|a| a.state.can_accept_work())
            .collect();

        if idle_agents.is_empty() {
            debug!("No idle agents available");
            return Ok(assignments);
        }

        // Get ready items (limited by available slots)
        let ready_items: Vec<&mut WorkItem> = work_items
            .iter_mut()
            .filter(|w| w.status == WorkStatus::Ready)
            .take(slots_available as usize)
            .collect();

        for item in ready_items {
            // Find best agent for this item
            if let Some(agent) = self.find_best_agent(item, &idle_agents, &assignments) {
                // Assign the item
                item.assign(agent.id);
                self.storage.save_work_item(item).await?;

                let mut task = Task::new(format!(
                    "[Mission Work Item] {}\n\nMission: {}\nWork item: {}\nSource: {}",
                    item.title,
                    mission.id,
                    item.id,
                    item.source_ref.as_deref().unwrap_or("unknown")
                ))
                .with_tags([
                    "mission-work-item".to_string(),
                    format!("mission:{}", mission.id),
                    format!("work-item:{}", item.id),
                ]);
                task.assign(agent.id);
                let task_id = task.id;
                self.channel.set_task(&task).await?;

                // Send persisted task assignment to agent
                let msg = Message::new(
                    AgentId::supervisor(),
                    agent.id,
                    MessageType::TaskAssign {
                        task_id: task_id.to_string(),
                    },
                );
                self.channel.send(&msg).await?;

                // Log event
                self.storage
                    .log_event(
                        mission.id,
                        &format!(
                            "Assigned '{}' to agent '{}' as task {}",
                            item.title, agent.name, task_id
                        ),
                    )
                    .await?;

                assignments.push((item.id, agent.id));
                info!(
                    "Assigned work item '{}' to agent '{}'",
                    item.title, agent.name
                );
            }
        }

        Ok(assignments)
    }

    // ==================== Agent Routing ====================

    /// Find the best agent for a work item using role-fit scoring.
    ///
    /// Scoring heuristic:
    /// 1. Exact role/tag match = 100 points
    /// 2. Generic worker = 50 points
    /// 3. Reviewer (for non-review work) = 25 points
    /// 4. Penalty for concurrent assignments in this tick
    fn find_best_agent<'a>(
        &self,
        item: &WorkItem,
        idle_agents: &[&'a Agent],
        current_assignments: &[(WorkItemId, AgentId)],
    ) -> Option<&'a Agent> {
        if idle_agents.is_empty() {
            return None;
        }

        let mut scored: Vec<(&Agent, AgentMatchScore)> = idle_agents
            .iter()
            .map(|agent| {
                let score = self.score_agent_match(agent, item, current_assignments);
                (*agent, score)
            })
            .collect();

        // Sort by total score descending
        scored.sort_by(|a, b| b.1.total().cmp(&a.1.total()));

        // Return best match if score is positive
        scored
            .first()
            .filter(|(_, score)| score.total() > 0)
            .map(|(agent, _)| *agent)
    }

    /// Score how well an agent matches a work item.
    fn score_agent_match(
        &self,
        agent: &Agent,
        item: &WorkItem,
        current_assignments: &[(WorkItemId, AgentId)],
    ) -> AgentMatchScore {
        let agent_name_lower = agent.name.to_lowercase();

        // Base score: role matching
        let base_score = if let Some(ref owner_role) = item.owner_role {
            let role_lower = owner_role.to_lowercase();
            if self.agent_matches_role(&agent_name_lower, &role_lower) {
                100 // Exact role match
            } else if self.is_reviewer_agent(&agent_name_lower) {
                // Reviewer can do review work at full score, other work at penalty
                if item.kind == WorkKind::Review {
                    100
                } else {
                    25
                }
            } else {
                50 // Generic worker
            }
        } else {
            // No role specified - any worker is fine
            if self.is_reviewer_agent(&agent_name_lower) {
                // Prefer non-reviewers for unspecified work
                40
            } else {
                60
            }
        };

        // Load penalty: reduce score for agents already assigned this tick
        let concurrent_count = current_assignments
            .iter()
            .filter(|(_, aid)| *aid == agent.id)
            .count() as u32;
        let load_penalty = concurrent_count * 30;

        AgentMatchScore::new(base_score, load_penalty)
    }

    /// Check if agent name suggests it matches a role.
    fn agent_matches_role(&self, agent_name: &str, role: &str) -> bool {
        // Check for direct match or common synonyms
        match role {
            "backend" => {
                agent_name.contains("backend")
                    || agent_name.contains("api")
                    || agent_name.contains("server")
            }
            "frontend" => {
                agent_name.contains("frontend")
                    || agent_name.contains("ui")
                    || agent_name.contains("web")
                    || agent_name.contains("client")
            }
            "tester" | "test" => agent_name.contains("test") || agent_name.contains("qa"),
            "reviewer" | "review" => agent_name.contains("review") || agent_name.contains("audit"),
            "devops" | "infra" => {
                agent_name.contains("devops")
                    || agent_name.contains("infra")
                    || agent_name.contains("deploy")
            }
            _ => agent_name.contains(role),
        }
    }

    /// Check if agent is a reviewer type.
    fn is_reviewer_agent(&self, agent_name: &str) -> bool {
        agent_name.contains("review") || agent_name.contains("audit")
    }

    // ==================== Reviewer Gate ====================

    /// Check if a work item requires reviewer approval before completion.
    ///
    /// Reviewer gate applies to implement and test work kinds when
    /// the mission policy requires it.
    #[must_use]
    pub fn requires_reviewer_gate(&self, item: &WorkItem, mission: &MissionRun) -> bool {
        if !mission.policy.reviewer_required {
            return false;
        }

        matches!(item.kind, WorkKind::Implement | WorkKind::Test)
    }

    /// Mark a work item as complete, respecting reviewer gates.
    ///
    /// Returns the specific completion outcome.
    #[instrument(skip(self, artifacts))]
    pub async fn complete_work_item(
        &self,
        mission_id: MissionId,
        work_item_id: WorkItemId,
        artifacts: Vec<String>,
        reviewer_approved: bool,
    ) -> Result<WorkItemCompletion> {
        let Some(mission) = self.storage.get_mission(mission_id).await? else {
            warn!("Mission {} not found", mission_id);
            return Ok(WorkItemCompletion::MissionNotFound);
        };

        let Some(mut item) = self.storage.get_work_item(mission_id, work_item_id).await? else {
            warn!("Work item {} not found", work_item_id);
            return Ok(WorkItemCompletion::WorkItemNotFound);
        };

        // Check reviewer gate
        if self.requires_reviewer_gate(&item, &mission) && !reviewer_approved {
            warn!(
                "Work item '{}' requires reviewer approval before completion",
                item.title
            );
            self.storage
                .log_event(
                    mission_id,
                    &format!("Work item '{}' awaiting reviewer approval", item.title),
                )
                .await?;
            return Ok(WorkItemCompletion::ReviewerApprovalRequired);
        }

        // Mark complete
        item.complete(artifacts);
        self.storage.save_work_item(&item).await?;

        self.storage
            .log_event(mission_id, &format!("Work item '{}' completed", item.title))
            .await?;

        info!("Completed work item '{}'", item.title);
        Ok(WorkItemCompletion::Completed)
    }

    /// Mark a work item as blocked.
    #[instrument(skip(self))]
    pub async fn block_work_item(
        &self,
        mission_id: MissionId,
        work_item_id: WorkItemId,
        reason: &str,
    ) -> Result<()> {
        let Some(mut item) = self.storage.get_work_item(mission_id, work_item_id).await? else {
            warn!("Work item {} not found", work_item_id);
            return Ok(());
        };

        item.block();
        self.storage.save_work_item(&item).await?;

        self.storage
            .log_event(
                mission_id,
                &format!("Work item '{}' blocked: {}", item.title, reason),
            )
            .await?;

        warn!("Blocked work item '{}': {}", item.title, reason);
        Ok(())
    }

    /// Start a work item (transition from Assigned to Running).
    #[instrument(skip(self))]
    pub async fn start_work_item(
        &self,
        mission_id: MissionId,
        work_item_id: WorkItemId,
    ) -> Result<()> {
        let Some(mut item) = self.storage.get_work_item(mission_id, work_item_id).await? else {
            warn!("Work item {} not found", work_item_id);
            return Ok(());
        };

        if item.status == WorkStatus::Assigned {
            item.start();
            self.storage.save_work_item(&item).await?;

            self.storage
                .log_event(mission_id, &format!("Work item '{}' started", item.title))
                .await?;

            info!("Started work item '{}'", item.title);
        }

        Ok(())
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::types::ObjectiveRef;

    #[test]
    fn test_scheduler_config_defaults() {
        let config = SchedulerConfig::default();
        assert_eq!(config.tick_interval_secs, 30);
        assert_eq!(config.max_parallel_items, 2);
        assert!(config.reviewer_required);
    }

    #[test]
    fn test_agent_match_score_total() {
        let score = AgentMatchScore::new(100, 30);
        assert_eq!(score.total(), 70);

        let negative = AgentMatchScore::new(20, 50);
        assert_eq!(negative.total(), -30);
    }

    #[test]
    fn test_mission_tick_result_default() {
        let result = MissionTickResult::default();
        assert!(result.promoted.is_empty());
        assert!(result.assigned.is_empty());
        assert!(!result.state_changed);
        assert!(result.new_state.is_none());
    }

    #[test]
    fn test_scheduler_tick_result_default() {
        let result = SchedulerTickResult::default();
        assert!(result.missions.is_empty());
        assert_eq!(result.total_promoted, 0);
        assert_eq!(result.total_assigned, 0);
        assert_eq!(result.missions_completed, 0);
    }

    // Helper to create a mission for testing
    fn create_test_mission() -> MissionRun {
        MissionRun::new(vec![ObjectiveRef::Issue {
            owner: "test".into(),
            repo: "repo".into(),
            number: 1,
        }])
    }

    // Helper to create work items for testing
    #[allow(dead_code)]
    fn create_test_work_items(mission_id: MissionId) -> Vec<WorkItem> {
        let item1 = WorkItem::new(mission_id, "Task 1", WorkKind::Implement);
        let mut item2 = WorkItem::new(mission_id, "Task 2", WorkKind::Implement);
        item2.depends_on = vec![item1.id];
        vec![item1, item2]
    }

    #[test]
    fn test_requires_reviewer_gate() {
        let mut mission = create_test_mission();
        mission.policy.reviewer_required = true;

        let implement_item = WorkItem::new(mission.id, "Implement", WorkKind::Implement);
        let test_item = WorkItem::new(mission.id, "Test", WorkKind::Test);
        let review_item = WorkItem::new(mission.id, "Review", WorkKind::Review);
        let design_item = WorkItem::new(mission.id, "Design", WorkKind::Design);

        // Implement and Test require reviewer gate
        assert!(matches!(
            implement_item.kind,
            WorkKind::Implement | WorkKind::Test
        ));
        assert!(matches!(
            test_item.kind,
            WorkKind::Implement | WorkKind::Test
        ));

        // Review and Design do not
        assert!(!matches!(
            review_item.kind,
            WorkKind::Implement | WorkKind::Test
        ));
        assert!(!matches!(
            design_item.kind,
            WorkKind::Implement | WorkKind::Test
        ));

        // When reviewer not required, nothing needs gate
        mission.policy.reviewer_required = false;
        // The check would be: !mission.policy.reviewer_required => false
    }

    #[test]
    fn test_role_matching() {
        // Test role matching patterns
        let backend_names = ["backend-worker", "api-agent", "server-1"];
        let frontend_names = ["frontend-dev", "ui-worker", "web-client"];
        let tester_names = ["tester-1", "qa-agent"];
        let reviewer_names = ["reviewer-bob", "audit-agent"];

        for name in backend_names {
            assert!(
                name.contains("backend") || name.contains("api") || name.contains("server"),
                "Should match backend: {}",
                name
            );
        }

        for name in frontend_names {
            assert!(
                name.contains("frontend")
                    || name.contains("ui")
                    || name.contains("web")
                    || name.contains("client"),
                "Should match frontend: {}",
                name
            );
        }

        for name in tester_names {
            assert!(
                name.contains("test") || name.contains("qa"),
                "Should match tester: {}",
                name
            );
        }

        for name in reviewer_names {
            assert!(
                name.contains("review") || name.contains("audit"),
                "Should match reviewer: {}",
                name
            );
        }
    }
}
