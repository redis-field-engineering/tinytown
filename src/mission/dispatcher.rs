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

use uuid::Uuid;

use crate::channel::Channel;
use crate::error::Result;
use crate::mission::scheduler::{MissionScheduler, SchedulerTickResult};
use crate::mission::storage::MissionStorage;
use crate::mission::types::MissionId;
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
        let scheduler = MissionScheduler::new(
            storage.clone(),
            channel.clone(),
            crate::mission::scheduler::SchedulerConfig {
                tick_interval_secs: config.tick_interval_secs,
                ..Default::default()
            },
        );
        let watch_engine = WatchEngine::with_defaults(storage.clone(), channel, github);
        Self {
            storage,
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
                .try_acquire_dispatch_lock(
                    mission_id,
                    &self.owner_token,
                    self.config.lock_ttl_secs,
                )
                .await?
            {
                claimed_missions.push(mission_id);
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
}
