/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission dispatcher runtime.
//!
//! The dispatcher owns the autonomous mission loop: acquire leases, process due
//! watches, run scheduler ticks, and persist waiting state until the mission
//! completes or is manually stopped.

use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::agent::AgentId;
use crate::channel::Channel;
use crate::error::Result;
use crate::message::{Message, MessageType};
use crate::mission::scheduler::{MissionScheduler, SchedulerTickResult};
use crate::mission::storage::MissionStorage;
use crate::mission::types::{MissionId, MissionState, WatchStatus};
use crate::mission::watch::{GitHubClient, WatchEngine, WatchEngineTickResult};

/// Dispatcher configuration.
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// Base polling interval for the dispatcher loop.
    pub tick_interval_secs: u64,
    /// Redis lease TTL for per-mission dispatcher ownership.
    pub lock_ttl_secs: u64,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            tick_interval_secs: 30,
            lock_ttl_secs: 90,
        }
    }
}

/// Result of a dispatcher tick.
#[derive(Debug, Clone, Default)]
pub struct DispatcherTickResult {
    /// Missions whose leases were acquired for this tick.
    pub claimed_missions: Vec<MissionId>,
    /// Watch processing result.
    pub watch_result: WatchEngineTickResult,
    /// Scheduler result.
    pub scheduler_result: SchedulerTickResult,
}

/// Persistent dispatcher loop for mission orchestration.
pub struct MissionDispatcher<G: GitHubClient> {
    storage: MissionStorage,
    channel: Channel,
    scheduler: MissionScheduler,
    watch_engine: WatchEngine<G>,
    config: DispatcherConfig,
    owner_token: String,
}

impl<G: GitHubClient> MissionDispatcher<G> {
    /// Create a new dispatcher runtime.
    pub fn new(
        storage: MissionStorage,
        channel: Channel,
        github: G,
        config: DispatcherConfig,
    ) -> Self {
        let scheduler_channel = channel.clone();
        let watch_channel = channel.clone();
        let scheduler = MissionScheduler::new(
            storage.clone(),
            scheduler_channel,
            crate::mission::scheduler::SchedulerConfig {
                tick_interval_secs: config.tick_interval_secs,
                ..Default::default()
            },
        );
        let watch_engine = WatchEngine::with_defaults(storage.clone(), watch_channel, github);
        Self {
            storage,
            channel,
            scheduler,
            watch_engine,
            config,
            owner_token: Uuid::new_v4().to_string(),
        }
    }

    /// Run a single dispatcher tick.
    pub async fn tick(&self, mission_filter: Option<MissionId>) -> Result<DispatcherTickResult> {
        let candidate_ids = if let Some(mission_id) = mission_filter {
            vec![mission_id]
        } else {
            self.storage.list_active().await?
        };

        let mut claimed_missions = Vec::new();
        for mission_id in candidate_ids {
            if self
                .storage
                .try_acquire_dispatch_lock(mission_id, &self.owner_token, self.config.lock_ttl_secs)
                .await?
            {
                claimed_missions.push(mission_id);
            }
        }

        let mut control_progress = Vec::new();
        for mission_id in &claimed_missions {
            if self.process_control_messages(*mission_id).await? {
                control_progress.push(*mission_id);
            }
        }

        let watch_result = if claimed_missions.is_empty() {
            WatchEngineTickResult::default()
        } else {
            self.watch_engine.tick_missions(&claimed_missions).await?
        };

        let scheduler_result = if claimed_missions.is_empty() {
            SchedulerTickResult::default()
        } else {
            self.scheduler.tick_missions(&claimed_missions).await?
        };

        for mission_id in &claimed_missions {
            if let Some(mut mission) = self.storage.get_mission(*mission_id).await? {
                mission.record_dispatch_tick();

                let scheduler_progress = scheduler_result
                    .missions
                    .iter()
                    .find(|result| result.mission_id == *mission_id)
                    .is_some_and(|result| {
                        result.state_changed
                            || !result.promoted.is_empty()
                            || !result.assigned.is_empty()
                            || !result.completed.is_empty()
                            || !result.blocked.is_empty()
                    });
                let watch_progress = watch_result.results.iter().any(|result| {
                    result.mission_id == *mission_id
                        && (result.triggered
                            || result.new_status != crate::mission::WatchStatus::Active)
                });
                let made_progress =
                    scheduler_progress || watch_progress || control_progress.contains(mission_id);
                if made_progress {
                    mission.record_dispatch_progress();
                }

                if let Some(reason) = self.assess_help_needed(&mission).await? {
                    let should_send = mission
                        .dispatcher_last_help_request_at
                        .map(|sent_at| {
                            mission.dispatcher_last_help_request_reason.as_deref()
                                != Some(reason.as_str())
                                || Utc::now() - sent_at
                                    >= Duration::seconds(
                                        (self.config.tick_interval_secs * 6) as i64,
                                    )
                        })
                        .unwrap_or(true);
                    if should_send {
                        self.send_help_request(*mission_id, &reason).await?;
                        mission.record_help_request(reason);
                    }
                }

                self.storage.save_mission(&mission).await?;
            }
        }

        for mission_id in &claimed_missions {
            let _ = self
                .storage
                .release_dispatch_lock(*mission_id, &self.owner_token)
                .await;
        }

        Ok(DispatcherTickResult {
            claimed_missions,
            watch_result,
            scheduler_result,
        })
    }

    /// Run the dispatcher loop until interrupted.
    pub async fn run(&self, mission_filter: Option<MissionId>) -> Result<()> {
        let sleep = StdDuration::from_secs(self.config.tick_interval_secs.max(1));
        loop {
            self.tick(mission_filter).await?;
            tokio::time::sleep(sleep).await;
        }
    }

    async fn process_control_messages(&self, mission_id: MissionId) -> Result<bool> {
        let mut progressed = false;
        let messages = self
            .storage
            .list_pending_control_messages(mission_id)
            .await?;
        if messages.is_empty() {
            return Ok(false);
        }

        let Some(mut mission) = self.storage.get_mission(mission_id).await? else {
            return Ok(false);
        };

        for mut message in messages {
            let body = message.body.trim();
            let lower = body.to_ascii_lowercase();
            if lower.starts_with("resume") || lower.starts_with("retry") {
                if mission.state.can_resume() {
                    let completed_watches = self.force_complete_blocking_watches(mission_id).await?;
                    mission.start();
                    mission.set_next_wake_at(None);
                    progressed = true;
                    self.storage
                        .log_event(
                            mission_id,
                            &format!(
                                "Dispatcher received resume directive from {}: {}{}",
                                message.sender,
                                body,
                                if completed_watches > 0 {
                                    format!(
                                        " (force-completed {} blocking watch(es))",
                                        completed_watches
                                    )
                                } else {
                                    String::new()
                                }
                            ),
                        )
                        .await?;
                } else {
                    self.storage
                        .log_event(
                            mission_id,
                            &format!(
                                "Dispatcher ignored resume directive from {} while mission was {:?}: {}",
                                message.sender, mission.state, body
                            ),
                        )
                        .await?;
                }
            } else if lower.starts_with("pause") || lower.starts_with("hold") {
                if mission.state.can_pause() {
                    mission.block(format!("Paused by {}: {}", message.sender, body));
                    progressed = true;
                    self.storage
                        .log_event(
                            mission_id,
                            &format!(
                                "Dispatcher received pause directive from {}: {}",
                                message.sender, body
                            ),
                        )
                        .await?;
                } else {
                    self.storage
                        .log_event(
                            mission_id,
                            &format!(
                                "Dispatcher ignored pause directive from {} while mission was {:?}: {}",
                                message.sender, mission.state, body
                            ),
                        )
                        .await?;
                }
            } else {
                self.storage
                    .log_event(
                        mission_id,
                        &format!(
                            "Dispatcher received operator note from {}: {}",
                            message.sender, body
                        ),
                    )
                    .await?;
            }

            message.mark_processed();
            self.storage.save_control_message(&message).await?;
        }

        self.storage.save_mission(&mission).await?;
        Ok(progressed)
    }

    async fn force_complete_blocking_watches(&self, mission_id: MissionId) -> Result<usize> {
        let watches = self.storage.list_watch_items(mission_id).await?;
        let mut completed = 0;

        for mut watch in watches {
            if watch.status == WatchStatus::Done {
                continue;
            }

            watch.complete();
            self.storage.save_watch_item(&watch).await?;
            completed += 1;
        }

        Ok(completed)
    }

    async fn assess_help_needed(
        &self,
        mission: &crate::mission::MissionRun,
    ) -> Result<Option<String>> {
        if mission.state == MissionState::Completed || mission.state == MissionState::Failed {
            return Ok(None);
        }

        let work_items = self.storage.list_work_items(mission.id).await?;
        let ready_items: Vec<_> = work_items
            .iter()
            .filter(|item| item.status == crate::mission::WorkStatus::Ready)
            .collect();
        let idle_agents: Vec<_> = self
            .channel
            .list_agents()
            .await?
            .into_iter()
            .filter(|agent| agent.state.can_accept_work())
            .collect();

        if !ready_items.is_empty() && idle_agents.is_empty() {
            return Ok(Some(format!(
                "Mission {} has {} ready work item(s) but no idle agents are available",
                mission.id,
                ready_items.len()
            )));
        }

        let stalled_for = mission
            .dispatcher_last_progress_at
            .map(|ts| Utc::now() - ts)
            .unwrap_or_else(|| Duration::seconds((self.config.tick_interval_secs * 2) as i64));
        let stuck_threshold_secs = std::cmp::max(self.config.tick_interval_secs * 6, 180) as i64;

        if stalled_for >= Duration::seconds(stuck_threshold_secs) {
            let tasks = self.channel.list_tasks().await?;
            let stale_tasks: Vec<_> = tasks
                .into_iter()
                .filter(|task| {
                    !task.state.is_terminal()
                        && task
                            .tags
                            .iter()
                            .any(|tag| tag == &format!("mission:{}", mission.id))
                        && Utc::now() - task.updated_at >= Duration::seconds(stuck_threshold_secs)
                })
                .collect();
            if let Some(task) = stale_tasks.first() {
                return Ok(Some(format!(
                    "Mission {} appears stuck; task {} has not changed since {}",
                    mission.id, task.id, task.updated_at
                )));
            }

            if let Some(reason) = &mission.blocked_reason {
                // Don't escalate when the mission is simply waiting on active
                // watches – that is normal async operation, not a stall.
                let watches = self.storage.list_watch_items(mission.id).await?;
                let has_active_watches = watches.iter().any(|w| {
                    w.status == crate::mission::WatchStatus::Active
                        || w.status == crate::mission::WatchStatus::Snoozed
                });
                if has_active_watches {
                    return Ok(None);
                }

                return Ok(Some(format!(
                    "Mission {} is still blocked: {}",
                    mission.id, reason
                )));
            }
        }

        Ok(None)
    }

    async fn send_help_request(&self, mission_id: MissionId, reason: &str) -> Result<()> {
        let message = Message::new(
            AgentId::supervisor(),
            AgentId::supervisor(),
            MessageType::Query {
                question: format!(
                    "[Mission Help Needed] Mission {}\n\n{}\n\nReply with `tt mission note {} \"resume ...\"`, `tt mission note {} \"pause ...\"`, or another operator note.",
                    mission_id, reason, mission_id, mission_id
                ),
            },
        );
        self.channel.send(&message).await?;
        self.storage
            .log_event(
                mission_id,
                &format!("Dispatcher asked conductor for help: {}", reason),
            )
            .await?;
        Ok(())
    }
}
