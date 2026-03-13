/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Tinytown CLI - Simple multi-agent orchestration.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use tinytown::{GlobalConfig, Result, Task, Town, plan};

const TT_AGENT_ID_ENV: &str = "TINYTOWN_AGENT_ID";
const TT_AGENT_NAME_ENV: &str = "TINYTOWN_AGENT_NAME";

/// Build a shell command to run an agent CLI with a prompt/instruction file.
/// Different CLIs have different ways to accept input:
/// - auggie: uses --instruction-file flag
/// - claude, codex, etc.: accept input via stdin (pipe or redirect)
fn build_cli_command(cli_name: &str, cli_cmd: &str, prompt_file: &std::path::Path) -> String {
    if cli_name == "auggie" {
        // Auggie uses --instruction-file flag
        format!("{} --instruction-file '{}'", cli_cmd, prompt_file.display())
    } else {
        // Other CLIs accept input via stdin
        format!("cat '{}' | {}", prompt_file.display(), cli_cmd)
    }
}

async fn resolve_agent_id_for_current_task(
    town: &Town,
    agent: Option<&str>,
) -> Result<tinytown::AgentId> {
    if let Some(agent_ref) = agent {
        if let Ok(agent_id) = agent_ref.parse::<tinytown::AgentId>()
            && town.channel().get_agent_state(agent_id).await?.is_some()
        {
            return Ok(agent_id);
        }

        return Ok(town.agent(agent_ref).await?.id());
    }

    if let Ok(agent_id) = std::env::var(TT_AGENT_ID_ENV)
        && let Ok(parsed_id) = agent_id.parse::<tinytown::AgentId>()
        && town.channel().get_agent_state(parsed_id).await?.is_some()
    {
        return Ok(parsed_id);
    }

    if let Ok(agent_name) = std::env::var(TT_AGENT_NAME_ENV) {
        return Ok(town.agent(&agent_name).await?.id());
    }

    Err(tinytown::Error::AgentNotFound(
        "No current agent context found. Pass an agent name/id or run this from an agent loop."
            .to_string(),
    ))
}

fn is_supervisor_alias(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "supervisor" | "conductor")
}

fn validate_spawn_agent_name(name: &str) -> Result<()> {
    if is_supervisor_alias(name) {
        return Err(tinytown::Error::Config(format!(
            "'{}' is reserved for the well-known supervisor/conductor mailbox",
            name
        )));
    }

    Ok(())
}

fn inbox_preview_prefix(msg_type: &tinytown::MessageType) -> &'static str {
    match classify_message(msg_type) {
        MessageCategory::Task => "[T]",
        MessageCategory::Query => "[Q]",
        MessageCategory::Informational => "[I]",
        MessageCategory::Confirmation => "[C]",
        MessageCategory::OtherActionable => "[!]",
    }
}

async fn sampled_inbox(
    channel: &tinytown::Channel,
    agent_id: tinytown::AgentId,
    sample_limit: usize,
) -> Result<(usize, Vec<tinytown::Message>, MessageBreakdown)> {
    let inbox_len = channel.inbox_len(agent_id).await?;
    if inbox_len == 0 {
        return Ok((0, Vec::new(), MessageBreakdown::default()));
    }

    let messages = channel
        .peek_inbox(agent_id, std::cmp::min(inbox_len, sample_limit) as isize)
        .await?;
    let mut breakdown = MessageBreakdown::default();
    for msg in &messages {
        breakdown.count(&msg.msg_type);
    }

    Ok((inbox_len, messages, breakdown))
}

async fn print_all_inbox_section(
    channel: &tinytown::Channel,
    heading: &str,
    inbox_len: usize,
    messages: &[tinytown::Message],
    breakdown: MessageBreakdown,
) {
    info!("  {}:", heading);
    info!(
        "    [T] {} tasks requiring action",
        breakdown.tasks + breakdown.other_actionable
    );
    info!("    [Q] {} queries awaiting response", breakdown.queries);
    info!("    [I] {} informational", breakdown.informational);
    info!("    [C] {} confirmations", breakdown.confirmations);

    let mut shown = 0;
    for msg in messages {
        if !matches!(
            classify_message(&msg.msg_type),
            MessageCategory::Task | MessageCategory::Query | MessageCategory::OtherActionable
        ) {
            continue;
        }
        if shown >= 5 {
            break;
        }

        let summary = describe_message(channel, &msg.msg_type).await;
        info!(
            "    • {} {}",
            inbox_preview_prefix(&msg.msg_type),
            truncate_summary(&summary, 90)
        );
        shown += 1;
    }

    if shown == 0 {
        for msg in messages.iter().take(3) {
            let summary = describe_message(channel, &msg.msg_type).await;
            info!(
                "    • {} {}",
                inbox_preview_prefix(&msg.msg_type),
                truncate_summary(&summary, 90)
            );
            shown += 1;
        }
    }

    if inbox_len > shown {
        info!("    …plus {} more message(s)", inbox_len - shown);
    }

    info!("");
}

async fn track_current_task_for_round(
    channel: &tinytown::Channel,
    agent_id: tinytown::AgentId,
    actionable_messages: &[(tinytown::Message, bool)],
) -> Result<()> {
    let task_ids: Vec<_> = actionable_messages
        .iter()
        .filter_map(|(msg, _)| match &msg.msg_type {
            tinytown::MessageType::TaskAssign { task_id } => task_id.parse().ok(),
            _ => None,
        })
        .collect();

    if task_ids.len() != 1 {
        return Ok(());
    }

    tinytown::TaskService::set_current_for_agent(channel, agent_id, task_ids[0]).await
}

async fn format_actionable_section(
    channel: &tinytown::Channel,
    actionable_messages: &[(tinytown::Message, bool)],
) -> String {
    let mut section = String::from("## Actionable Messages (already popped)\n\n");

    for (idx, (msg, urgent)) in actionable_messages.iter().enumerate() {
        let priority = if *urgent { "URGENT" } else { "normal" };
        match &msg.msg_type {
            tinytown::MessageType::TaskAssign { task_id } => {
                let description = if let Ok(tid) = task_id.parse::<tinytown::TaskId>() {
                    match channel.get_task(tid).await {
                        Ok(Some(task)) => truncate_summary(&task.description, 160),
                        _ => "Task details unavailable".to_string(),
                    }
                } else {
                    "Task details unavailable".to_string()
                };
                section.push_str(&format!(
                    "{}. [{}] task assignment from {}\n   Task ID: {}\n   Description: {}\n   Complete with: tt task complete {} --result \"what was done\"\n   Ignore any mission/work-item UUIDs in the description; the Task ID above is the real Tinytown task id.\n",
                    idx + 1,
                    priority,
                    msg.from,
                    task_id,
                    description,
                    task_id
                ));
            }
            _ => {
                let summary =
                    truncate_summary(&describe_message(channel, &msg.msg_type).await, 120);
                section.push_str(&format!(
                    "{}. [{}] from {}: {}\n",
                    idx + 1,
                    priority,
                    msg.from,
                    summary
                ));
            }
        }
    }

    section
}

#[derive(Parser)]
#[command(name = "tt")]
#[command(author, version, about = "Tinytown - Simple multi-agent orchestration using Redis", long_about = None)]
struct Cli {
    /// Town directory (defaults to current directory)
    #[arg(short, long, global = true, default_value = ".")]
    town: PathBuf,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap: Download and build Redis (delegates to an AI agent)
    Bootstrap {
        /// Redis version to install (default: latest)
        #[arg(default_value = "latest")]
        version: String,

        /// Agent CLI to use for bootstrapping (uses default_cli from global config if not specified)
        #[arg(short, long)]
        cli: Option<String>,
    },

    /// Initialize a new town
    Init {
        /// Town name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Spawn a new agent
    Spawn {
        /// Agent name
        name: String,

        /// CLI to use (uses default_cli from config if not specified)
        #[arg(short, long)]
        cli: Option<String>,

        /// Maximum rounds before agent stops (default: runs until done)
        #[arg(long, default_value = "10")]
        max_rounds: u32,

        /// Run in foreground (don't background the process)
        #[arg(long)]
        foreground: bool,
    },

    /// Run agent loop (internal - called by spawn)
    #[command(hide = true)]
    AgentLoop {
        /// Agent name
        name: String,

        /// Agent ID
        id: String,

        /// Maximum rounds
        max_rounds: u32,
    },

    /// List all agents
    List,

    /// Assign a task to an agent
    Assign {
        /// Agent name
        agent: String,

        /// Task description
        task: String,
    },

    /// Show town status
    Status {
        /// Show deep status with recent agent activity
        #[arg(long)]
        deep: bool,

        /// Show detailed task breakdown by state and agent
        #[arg(long)]
        tasks: bool,
    },

    /// Keep a connection open to the town
    Start,

    /// Request all agents in the town to stop gracefully
    Stop,

    /// Reset all town state (clear all agents, tasks, messages)
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,

        /// Only reset agent-related state (agents and inboxes), preserving tasks
        #[arg(long)]
        agents_only: bool,
    },

    /// Stop a specific agent gracefully
    Kill {
        /// Agent name to stop
        agent: String,
    },

    /// Remove stopped/stale agents from Redis
    Prune {
        /// Remove ALL agents (not just stopped ones)
        #[arg(long)]
        all: bool,
    },

    /// Manage individual tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// Check agent inbox(es)
    Inbox {
        /// Agent name (optional with --all)
        agent: Option<String>,

        /// Show pending messages for all agents
        #[arg(long, short)]
        all: bool,
    },

    /// Send a message to an agent
    Send {
        /// Target agent name
        to: String,

        /// Message content
        message: String,

        /// Mark message as a query requiring a response
        #[arg(long, conflicts_with_all = ["info", "ack"])]
        query: bool,

        /// Mark message as informational (FYI)
        #[arg(long, conflicts_with_all = ["query", "ack"])]
        info: bool,

        /// Mark message as an acknowledgment
        #[arg(long, conflicts_with_all = ["query", "info"])]
        ack: bool,

        /// Send as urgent (processed before regular inbox)
        #[arg(long)]
        urgent: bool,
    },

    /// Start the conductor (interactive orchestration mode)
    Conductor,

    /// Plan tasks without starting agents (edit tasks.toml)
    Plan {
        /// Initialize a new tasks.toml file
        #[arg(short, long)]
        init: bool,
    },

    /// Sync tasks.toml with Redis
    Sync {
        /// Direction: 'push' (file→Redis) or 'pull' (Redis→file)
        #[arg(default_value = "push")]
        direction: String,
    },

    /// Save Redis state to AOF file (for version control)
    Save,

    /// Restore Redis state from AOF file
    Restore,

    /// View or set global configuration (~/.tt/config.toml)
    Config {
        /// Config key to get or set (e.g., default_cli)
        key: Option<String>,

        /// Value to set (if omitted, shows current value)
        value: Option<String>,
    },

    /// Detect and clean up crashed/orphaned agents
    Recover,

    /// List all registered towns
    Towns,

    /// Manage the global task backlog
    Backlog {
        #[command(subcommand)]
        action: BacklogAction,
    },

    /// Recover orphaned tasks from dead agents
    Reclaim {
        /// Move orphaned tasks to the backlog
        #[arg(long)]
        to_backlog: bool,

        /// Move orphaned tasks to a specific agent
        #[arg(long, value_name = "AGENT")]
        to: Option<String>,

        /// Reclaim only from a specific dead agent
        #[arg(long, value_name = "AGENT")]
        from: Option<String>,
    },

    /// Restart a stopped agent with fresh rounds
    Restart {
        /// Agent name to restart
        agent: String,

        /// Maximum rounds for restarted agent
        #[arg(long, default_value = "10")]
        rounds: u32,

        /// Run in foreground (don't background the process)
        #[arg(long)]
        foreground: bool,
    },

    /// Authentication management for townhall
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Migrate old Redis keys to town-isolated format
    Migrate {
        /// Preview migration without making changes
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,

        /// Migrate JSON string storage to Redis Hash format
        #[arg(long)]
        hash: bool,
    },

    /// Autonomous multi-issue mission mode
    Mission {
        #[command(subcommand)]
        action: MissionAction,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Generate a new API key and its hash
    GenKey,
}

#[derive(Subcommand)]
enum MissionAction {
    /// Start a new mission with one or more GitHub issues
    Start {
        /// GitHub issue numbers or URLs (e.g., "23" or "owner/repo#23")
        #[arg(long = "issue", short = 'i', value_name = "ISSUE")]
        issues: Vec<String>,

        /// Document paths to include as objectives
        #[arg(long = "doc", short = 'd', value_name = "PATH")]
        docs: Vec<String>,

        /// Maximum parallel work items (default: 2)
        #[arg(long, default_value = "2")]
        max_parallel: u32,

        /// Disable reviewer requirement
        #[arg(long)]
        no_reviewer: bool,
    },

    /// Show status of active missions
    Status {
        /// Specific mission ID to show
        #[arg(long, short = 'r')]
        run: Option<String>,

        /// Show detailed work item status
        #[arg(long)]
        work: bool,

        /// Show watch items
        #[arg(long)]
        watch: bool,
    },

    /// Resume a stopped or blocked mission
    Resume {
        /// Mission run ID to resume
        run_id: String,
    },

    /// Stop an active mission
    Stop {
        /// Mission run ID to stop
        run_id: String,

        /// Force stop without graceful cleanup
        #[arg(long)]
        force: bool,
    },

    /// List all missions (including completed)
    List {
        /// Include completed/failed missions
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum BacklogAction {
    /// Add a task to the backlog
    Add {
        /// Task description
        description: String,

        /// Optional tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
    },

    /// List all tasks in the backlog
    List,

    /// Claim a task from the backlog and assign to an agent
    Claim {
        /// Task ID to claim
        task_id: String,

        /// Agent name to assign the task to
        agent: String,
    },

    /// Assign all backlog tasks to an agent
    AssignAll {
        /// Agent name to assign all tasks to
        agent: String,
    },

    /// Remove a task from the backlog
    Remove {
        /// Task ID to remove
        task_id: String,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    /// Mark a task as completed
    Complete {
        /// Task ID to mark as completed
        task_id: String,

        /// Optional result/summary message
        #[arg(long)]
        result: Option<String>,
    },

    /// Show details of a specific task
    Show {
        /// Task ID to show
        task_id: String,
    },

    /// Show the tracked current task for an agent
    Current {
        /// Agent name or ID (optional inside an agent loop)
        agent: Option<String>,
    },

    /// List all tasks
    List {
        /// Filter by state (pending, assigned, running, completed, failed, cancelled)
        #[arg(long)]
        state: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageCategory {
    Task,
    Query,
    Informational,
    Confirmation,
    OtherActionable,
}

#[derive(Debug, Default, Clone, Copy)]
struct MessageBreakdown {
    tasks: usize,
    queries: usize,
    informational: usize,
    confirmations: usize,
    other_actionable: usize,
}

impl MessageBreakdown {
    fn count(&mut self, msg_type: &tinytown::MessageType) {
        match classify_message(msg_type) {
            MessageCategory::Task => self.tasks += 1,
            MessageCategory::Query => self.queries += 1,
            MessageCategory::Informational => self.informational += 1,
            MessageCategory::Confirmation => self.confirmations += 1,
            MessageCategory::OtherActionable => self.other_actionable += 1,
        }
    }

    fn actionable_count(&self) -> usize {
        self.tasks + self.queries + self.other_actionable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BacklogRole {
    Frontend,
    Backend,
    Tester,
    Reviewer,
    Docs,
    Devops,
    Security,
    General,
}

fn classify_backlog_role(agent_name: &str) -> BacklogRole {
    let role = agent_name.to_lowercase();
    if role.contains("front")
        || role.contains("ui")
        || role.contains("web")
        || role.contains("client")
    {
        BacklogRole::Frontend
    } else if role.contains("back") || role.contains("api") || role.contains("server") {
        BacklogRole::Backend
    } else if role.contains("test") || role.contains("qa") {
        BacklogRole::Tester
    } else if role.contains("review") || role.contains("audit") {
        BacklogRole::Reviewer
    } else if role.contains("doc") || role.contains("writer") {
        BacklogRole::Docs
    } else if role.contains("devops")
        || role.contains("ops")
        || role.contains("infra")
        || role.contains("deploy")
    {
        BacklogRole::Devops
    } else if role.contains("security") || role == "sec" {
        BacklogRole::Security
    } else {
        BacklogRole::General
    }
}

fn backlog_role_keywords(agent_name: &str) -> &'static [&'static str] {
    match classify_backlog_role(agent_name) {
        BacklogRole::Frontend => &["frontend", "ui", "web", "client", "ux"],
        BacklogRole::Backend => &["backend", "api", "server", "database", "data"],
        BacklogRole::Tester => &["test", "qa", "validation", "regression"],
        BacklogRole::Reviewer => &[
            "review",
            "reviewer",
            "qa",
            "security",
            "audit",
            "validation",
        ],
        BacklogRole::Docs => &["docs", "doc", "documentation", "spec", "readme"],
        BacklogRole::Devops => &["devops", "ops", "infra", "deploy", "ci", "reliability"],
        BacklogRole::Security => &["security", "sec", "vulnerability", "hardening", "audit"],
        BacklogRole::General => &[],
    }
}

fn classify_custom_message(kind: &str, payload: &str) -> MessageCategory {
    let kind = kind.to_lowercase();
    let payload = payload.to_lowercase();
    let token = format!("{} {}", kind, payload);

    if token.contains("ack")
        || token.contains("thanks")
        || token.contains("thank you")
        || token.contains("received")
        || token.contains("approved")
    {
        return MessageCategory::Confirmation;
    }

    if token.contains("info")
        || token.contains("fyi")
        || token.contains("status")
        || token.contains("update")
    {
        return MessageCategory::Informational;
    }

    if token.contains("query") || token.contains("question") {
        return MessageCategory::Query;
    }

    MessageCategory::Task
}

fn classify_message(msg_type: &tinytown::MessageType) -> MessageCategory {
    match msg_type {
        tinytown::MessageType::TaskAssign { .. } | tinytown::MessageType::Task { .. } => {
            MessageCategory::Task
        }
        tinytown::MessageType::Query { .. } | tinytown::MessageType::StatusRequest => {
            MessageCategory::Query
        }
        tinytown::MessageType::Informational { .. }
        | tinytown::MessageType::TaskDone { .. }
        | tinytown::MessageType::TaskFailed { .. }
        | tinytown::MessageType::StatusResponse { .. }
        | tinytown::MessageType::Ping
        | tinytown::MessageType::Pong => MessageCategory::Informational,
        tinytown::MessageType::Confirmation { .. } => MessageCategory::Confirmation,
        tinytown::MessageType::Custom { kind, payload } => classify_custom_message(kind, payload),
        tinytown::MessageType::Shutdown => MessageCategory::OtherActionable,
    }
}

fn parse_confirmation_type(message: &str) -> tinytown::ConfirmationType {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();

    if lower.starts_with("rejected:") {
        let reason = trimmed
            .split_once(':')
            .map(|(_, reason)| reason.trim().to_string())
            .filter(|reason| !reason.is_empty())
            .unwrap_or_else(|| "No reason provided".to_string());
        return tinytown::ConfirmationType::Rejected { reason };
    }

    if lower.starts_with("received") {
        return tinytown::ConfirmationType::Received;
    }

    if lower.starts_with("approved") {
        return tinytown::ConfirmationType::Approved;
    }

    if lower.contains("thanks") || lower.contains("thank you") {
        return tinytown::ConfirmationType::Thanks;
    }

    tinytown::ConfirmationType::Acknowledged
}

fn summarize_message(msg_type: &tinytown::MessageType) -> String {
    match msg_type {
        tinytown::MessageType::TaskAssign { task_id } => format!("task assignment {}", task_id),
        tinytown::MessageType::Task { description } => description.clone(),
        tinytown::MessageType::Query { question } => format!("question: {}", question),
        tinytown::MessageType::Informational { summary } => summary.clone(),
        tinytown::MessageType::Confirmation { ack_type } => match ack_type {
            tinytown::ConfirmationType::Received => "received".to_string(),
            tinytown::ConfirmationType::Acknowledged => "acknowledged".to_string(),
            tinytown::ConfirmationType::Thanks => "thanks".to_string(),
            tinytown::ConfirmationType::Approved => "approved".to_string(),
            tinytown::ConfirmationType::Rejected { reason } => {
                format!("rejected: {}", reason)
            }
        },
        tinytown::MessageType::TaskDone { task_id, result } => {
            format!("task {} done: {}", task_id, result)
        }
        tinytown::MessageType::TaskFailed { task_id, error } => {
            format!("task {} failed: {}", task_id, error)
        }
        tinytown::MessageType::StatusRequest => "status requested".to_string(),
        tinytown::MessageType::StatusResponse {
            state,
            current_task,
        } => {
            if let Some(task) = current_task {
                format!("status {} ({})", state, task)
            } else {
                format!("status {}", state)
            }
        }
        tinytown::MessageType::Ping => "ping".to_string(),
        tinytown::MessageType::Pong => "pong".to_string(),
        tinytown::MessageType::Shutdown => "shutdown requested".to_string(),
        tinytown::MessageType::Custom { kind, payload } => format!("[{}] {}", kind, payload),
    }
}

async fn describe_message(channel: &tinytown::Channel, msg_type: &tinytown::MessageType) -> String {
    match msg_type {
        tinytown::MessageType::TaskAssign { task_id } => {
            if let Ok(tid) = task_id.parse::<tinytown::TaskId>()
                && let Ok(Some(task)) = channel.get_task(tid).await
            {
                format!("task {}: {}", task_id, task.description)
            } else {
                format!("task {}", task_id)
            }
        }
        _ => summarize_message(msg_type),
    }
}

fn mission_task_binding(
    tags: &[String],
) -> Option<(tinytown::mission::MissionId, tinytown::mission::WorkItemId)> {
    let mission_id = tags
        .iter()
        .find_map(|tag| tag.strip_prefix("mission:"))
        .and_then(|value| value.parse().ok())?;
    let work_item_id = tags
        .iter()
        .find_map(|tag| tag.strip_prefix("work-item:"))
        .and_then(|value| value.parse().ok())?;
    Some((mission_id, work_item_id))
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    let first_line = text.lines().next().unwrap_or(text).trim();
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let truncated: String = first_line
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect();
        format!("{}...", truncated)
    }
}

/// Clean up a raw log line for display in `tt status --deep`.
///
/// Extracts meaningful content from tracing-formatted logs like:
/// `[2m2026-03-09T20:03:14.667655Z[0m [32m INFO[0m [2mtt[0m[2m:[0m    ✅ Round 1 complete`
///
/// Returns None if the line should be skipped (e.g., internal waiting loops).
fn clean_log_line(line: &str) -> Option<String> {
    // Strip ANSI escape codes
    let stripped = strip_ansi_codes(line);

    // Skip empty lines
    if stripped.trim().is_empty() {
        return None;
    }

    // Skip internal waiting loop messages (noise)
    if stripped.contains("Inbox empty, waiting") {
        return None;
    }

    // Skip Redis version messages
    if stripped.contains("Redis version") && stripped.contains("detected") {
        return None;
    }

    // Skip repetitive round marker lines (just noise in logs)
    // Pattern: "📍 Round X/Y" without any other content
    let trimmed = stripped.trim();
    if trimmed.starts_with("📍 Round ") && !trimmed.contains("complete") {
        return None;
    }

    // Skip standalone "tt:" lines (empty log content)
    if trimmed == "tt:" || trimmed.ends_with(" tt:") {
        return None;
    }

    // Skip "Running auggie..." lines (repetitive, expected behavior)
    if trimmed.contains("Running auggie") {
        return None;
    }

    // Skip "Rounds completed:" status lines (redundant with round complete messages)
    if trimmed.contains("📊 Rounds completed:") {
        return None;
    }

    // Skip batching status lines (low-value noise)
    if trimmed.contains("📬 batched:") {
        return None;
    }

    // Skip repetitive backlog prompting messages (consolidate with round info)
    if trimmed.contains("prompting backlog review") || trimmed.contains("prompting claim review") {
        return None;
    }

    // Try to extract the actual message content from tracing format
    // Format: "2026-03-09T20:03:14.667655Z  INFO tt:    ✅ Round 1 complete"
    // Or: "📍 Round 2/15"
    let content = extract_log_content(&stripped);

    if content.is_empty() {
        return None;
    }

    // Skip if extracted content is just "tt:" (sometimes logs have empty content)
    if content == "tt:" {
        return None;
    }

    Some(content)
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence: ESC [ ... m
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit 'm' (end of color code)
                for ch in chars.by_ref() {
                    if ch == 'm' {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Clean up old round log files for an agent.
///
/// Removes all files matching pattern: `{agent_name}_round_{N}.log`
/// Returns the number of files deleted.
fn clean_agent_round_logs(log_dir: &std::path::Path, agent_name: &str) -> usize {
    let prefix = format!("{}_round_", agent_name);
    let mut deleted = 0;

    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str())
                && filename.starts_with(&prefix)
                && filename.ends_with(".log")
                && std::fs::remove_file(&path).is_ok()
            {
                deleted += 1;
            }
        }
    }

    deleted
}

/// Find the latest round log file for an agent.
///
/// Searches for files matching pattern: `{agent_name}_round_{N}.log`
/// Returns the path and round number of the file with the highest round number.
fn find_latest_round_log(
    log_dir: &std::path::Path,
    agent_name: &str,
) -> Option<(u32, std::path::PathBuf)> {
    let prefix = format!("{}_round_", agent_name);
    let mut latest: Option<(u32, std::path::PathBuf)> = None;

    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str())
                && filename.starts_with(&prefix)
                && filename.ends_with(".log")
            {
                // Extract round number from filename
                let num_part = &filename[prefix.len()..filename.len() - 4]; // Remove prefix and ".log"
                if let Ok(round_num) = num_part.parse::<u32>() {
                    match &latest {
                        Some((current_max, _)) if round_num > *current_max => {
                            latest = Some((round_num, path));
                        }
                        None => {
                            latest = Some((round_num, path));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    latest
}

/// Extract the meaningful content from a tracing log line.
fn extract_log_content(line: &str) -> String {
    let trimmed = line.trim();

    // If it starts with an emoji or marker, it's already clean content
    if trimmed.starts_with('📍')
        || trimmed.starts_with('✅')
        || trimmed.starts_with('❌')
        || trimmed.starts_with('🔄')
        || trimmed.starts_with('📬')
        || trimmed.starts_with('🤖')
        || trimmed.starts_with('📊')
        || trimmed.starts_with('🛑')
        || trimmed.starts_with('📋')
    {
        return trimmed.to_string();
    }

    // Try to find the message after the log level indicator
    // Patterns: "INFO tt:" or "WARN tt:" or just the timestamp pattern
    if let Some(pos) = trimmed.find(" INFO ") {
        let after_info = &trimmed[pos + 6..];
        // Skip module path like "tt:" or "tinytown::town:"
        if let Some(colon_pos) = after_info.find(':') {
            let content = after_info[colon_pos + 1..].trim();
            if !content.is_empty() {
                return content.to_string();
            }
        }
        return after_info.trim().to_string();
    }

    if let Some(pos) = trimmed.find(" WARN ") {
        let after_warn = &trimmed[pos + 6..];
        if let Some(colon_pos) = after_warn.find(':') {
            let content = after_warn[colon_pos + 1..].trim();
            if !content.is_empty() {
                return format!("⚠️ {}", content);
            }
        }
        return format!("⚠️ {}", after_warn.trim());
    }

    if let Some(pos) = trimmed.find(" ERROR ") {
        let after_error = &trimmed[pos + 7..];
        if let Some(colon_pos) = after_error.find(':') {
            let content = after_error[colon_pos + 1..].trim();
            if !content.is_empty() {
                return format!("❌ {}", content);
            }
        }
        return format!("❌ {}", after_error.trim());
    }

    // If it looks like a timestamp at the start, try to skip it
    // Pattern: "2026-03-09T20:03:14.667655Z ..."
    if trimmed.len() > 27
        && trimmed.chars().nth(4) == Some('-')
        && trimmed.chars().nth(10) == Some('T')
    {
        let rest = trimmed[27..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }

    // Return as-is if we couldn't parse it
    trimmed.to_string()
}

fn backlog_role_hint(agent_name: &str) -> &'static str {
    match classify_backlog_role(agent_name) {
        BacklogRole::Frontend => "Prioritize tasks tagged frontend/ui/web/client.",
        BacklogRole::Backend => "Prioritize tasks tagged backend/api/server/database.",
        BacklogRole::Tester => "Prioritize tasks tagged test/qa/validation/regression.",
        BacklogRole::Reviewer => "Prioritize review/quality/security validation tasks.",
        BacklogRole::Docs => "Prioritize documentation/spec/readme tasks.",
        BacklogRole::Devops => "Prioritize infrastructure/ci/deploy/reliability tasks.",
        BacklogRole::Security => "Prioritize security/vulnerability/hardening tasks.",
        BacklogRole::General => {
            "Prioritize tasks matching your current specialization and capabilities."
        }
    }
}

fn backlog_task_matches_role(task: &tinytown::Task, agent_name: &str) -> bool {
    let keywords = backlog_role_keywords(agent_name);
    if keywords.is_empty() {
        return true;
    }

    let normalized_tags: Vec<String> = task.tags.iter().map(|tag| tag.to_lowercase()).collect();
    if normalized_tags
        .iter()
        .any(|tag| keywords.iter().any(|keyword| tag == keyword))
    {
        return true;
    }

    let description = task.description.to_lowercase();
    keywords.iter().any(|keyword| description.contains(keyword))
}

struct BacklogSnapshot {
    total_backlog: usize,
    total_matching: usize,
    tasks: Vec<(tinytown::TaskId, Task)>,
}

async fn backlog_snapshot_for_agent(
    channel: &tinytown::Channel,
    agent_name: &str,
    limit: usize,
) -> Result<BacklogSnapshot> {
    let backlog_ids = channel.backlog_list().await?;
    let mut tasks = Vec::new();
    let mut total_matching = 0usize;

    for task_id in backlog_ids {
        if let Some(task) = channel.get_task(task_id).await?
            && backlog_task_matches_role(&task, agent_name)
        {
            total_matching += 1;
            if tasks.len() < limit {
                tasks.push((task_id, task));
            }
        }
    }

    Ok(BacklogSnapshot {
        total_backlog: channel.backlog_len().await?,
        total_matching,
        tasks,
    })
}

/// Bootstrap Redis by delegating to an AI coding agent.
///
/// The agent fetches the release from GitHub, downloads source, and builds it.
fn bootstrap_redis(version: &str, cli: &str) -> Result<()> {
    use std::process::Command;

    let tt_dir = dirs::home_dir()
        .map(|h| h.join(".tt"))
        .unwrap_or_else(|| std::path::PathBuf::from(".tt"));

    info!("🚀 Bootstrapping Redis {} to {}", version, tt_dir.display());
    info!("   Using {} to download and build Redis...", cli);
    info!("");

    // Create .tt directory
    std::fs::create_dir_all(&tt_dir)?;

    let version_instruction = if version == "latest" {
        "Find the latest stable release version number from https://github.com/redis/redis/releases (e.g., 8.0.2)".to_string()
    } else {
        format!("Use Redis version {}", version)
    };

    let prompt = format!(
        r#"# Task: Download and Build Redis

{version_instruction}

## Steps

1. Go to https://github.com/redis/redis/releases
2. Find the release version (e.g., 8.0.2)
3. Download the source tarball (.tar.gz) to {tt_dir}/versions/
4. Extract it to {tt_dir}/versions/redis-<version>/ (e.g., redis-8.0.2)
5. cd into the extracted directory and run `make` to build Redis
6. Create {tt_dir}/bin/ directory if it doesn't exist
7. Create symlinks in {tt_dir}/bin/ pointing to the built binaries:
   - ln -sf {tt_dir}/versions/redis-<version>/src/redis-server {tt_dir}/bin/redis-server
   - ln -sf {tt_dir}/versions/redis-<version>/src/redis-cli {tt_dir}/bin/redis-cli

## Target Directory

Base directory: {tt_dir}
Version directory: {tt_dir}/versions/redis-<version>/
Symlinks: {tt_dir}/bin/redis-server, {tt_dir}/bin/redis-cli

## Important

- Use curl or wget to download
- The source URL format is: https://github.com/redis/redis/archive/refs/tags/<version>.tar.gz
- After building, verify with: {tt_dir}/bin/redis-server --version
- The symlinks allow easy switching between versions

## When Done

Print the installed version and confirm the symlinks are working.
"#,
        version_instruction = version_instruction,
        tt_dir = tt_dir.display()
    );

    // Write prompt to temp file
    let prompt_file = tt_dir.join("bootstrap_prompt.md");
    std::fs::write(&prompt_file, &prompt)?;

    // Get the CLI command
    let cli_cmd = match cli {
        "claude" => "claude --print --dangerously-skip-permissions",
        "auggie" => "auggie --print",
        "codex" => "codex exec --dangerously-bypass-approvals-and-sandbox",
        "aider" => "aider --yes --no-auto-commits --message",
        _ => cli, // Allow custom commands
    };

    let shell_cmd = build_cli_command(cli, cli_cmd, &prompt_file);
    info!("📋 Running: {}", shell_cmd);
    info!("   (This may take a few minutes to download and compile)");
    info!("");

    // Run the AI agent
    let status = Command::new("sh")
        .args(["-c", &shell_cmd])
        .current_dir(&tt_dir)
        .status()?;

    // Clean up prompt file
    let _ = std::fs::remove_file(&prompt_file);

    if status.success() {
        let redis_bin = tt_dir.join("bin/redis-server");
        if redis_bin.exists() {
            // Get version from the installed binary
            let version_output = Command::new(&redis_bin)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();

            info!("");
            info!("✅ Redis installed successfully!");
            info!("   Location: {}", redis_bin.display());
            if !version_output.is_empty() {
                info!("   {}", version_output.trim());
            }

            // Initialize global config with password if not already set
            match GlobalConfig::load_or_init() {
                Ok(config) => {
                    info!("");
                    info!("📋 Global config initialized:");
                    info!("   Config: ~/.tt/config.toml");
                    info!("   Default CLI: {}", config.default_cli);
                    info!(
                        "   Central Redis: {}:{} (password protected)",
                        config.redis.host, config.redis.port
                    );
                }
                Err(e) => {
                    warn!("⚠️  Could not initialize global config: {}", e);
                }
            }

            info!("");
            info!("   Tinytown will automatically use this Redis.");
            info!("   Run: tt init");
        } else {
            info!("");
            info!("⚠️  Agent finished but redis-server not found at expected location.");
            info!("   Expected: {}", redis_bin.display());
            info!(
                "   Check {}/versions/ for build artifacts.",
                tt_dir.display()
            );
            info!("   You may need to run 'tt bootstrap' again or build manually.");
        }
    } else {
        info!("");
        info!("❌ Bootstrap failed. Check the output above for errors.");
        info!("   You can also install Redis manually:");
        info!("   - macOS: brew install redis");
        info!("   - Ubuntu: sudo apt install redis-server");
        info!("   - From source: https://redis.io/docs/latest/operate/oss_and_stack/install/");
    }

    Ok(())
}

/// Derive a town name from git repo and branch, or fall back to directory name.
///
/// Format: `<repo>-<branch>` (e.g., `redisearch-feature-auth`)
fn derive_town_name(town_path: &std::path::Path) -> String {
    use std::process::Command;

    // Try to get git repo name and branch
    let repo_name = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(town_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .and_then(|path| {
            std::path::Path::new(path.trim())
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        });

    let branch_name = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(town_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    match (repo_name, branch_name) {
        (Some(repo), Some(branch)) => {
            // Sanitize branch name (replace / with -)
            let branch = branch.replace('/', "-");
            format!("{}-{}", repo, branch)
        }
        (Some(repo), None) => repo,
        _ => {
            // Fall back to directory name
            town_path
                .canonicalize()
                .ok()
                .and_then(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "tinytown".to_string())
        }
    }
}

/// Register a town in ~/.tt/towns.toml
fn register_town(town_path: &std::path::Path, name: &str) -> Result<()> {
    use tinytown::global_config::GLOBAL_CONFIG_DIR;

    let tt_dir = dirs::home_dir()
        .map(|h| h.join(GLOBAL_CONFIG_DIR))
        .ok_or_else(|| {
            tinytown::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find home directory",
            ))
        })?;

    // Ensure ~/.tt exists
    std::fs::create_dir_all(&tt_dir)?;

    let towns_path = tt_dir.join("towns.toml");
    let abs_path = town_path
        .canonicalize()
        .unwrap_or_else(|_| town_path.to_path_buf());
    let path_str = abs_path.to_string_lossy().to_string();

    // Load existing towns or create new
    let mut towns_file: TownsFile = if towns_path.exists() {
        let content = std::fs::read_to_string(&towns_path)?;
        toml::from_str(&content).unwrap_or_default()
    } else {
        TownsFile::default()
    };

    // Check if already registered (by path)
    if towns_file.towns.iter().any(|t| t.path == path_str) {
        // Update name if different
        for town in &mut towns_file.towns {
            if town.path == path_str && town.name != name {
                town.name = name.to_string();
            }
        }
    } else {
        // Add new entry
        towns_file.towns.push(TownEntry {
            path: path_str,
            name: name.to_string(),
        });
    }

    // Save
    let content = toml::to_string_pretty(&towns_file).map_err(|e| {
        tinytown::Error::Io(std::io::Error::other(format!(
            "Failed to serialize towns.toml: {}",
            e
        )))
    })?;
    std::fs::write(&towns_path, content)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Commands::Bootstrap {
            version,
            cli: cli_arg,
        } => {
            // Use CLI arg, or fall back to global config default_cli
            let cli_name = cli_arg.unwrap_or_else(|| {
                GlobalConfig::load()
                    .map(|c| c.default_cli)
                    .unwrap_or_else(|_| "claude".to_string())
            });
            bootstrap_redis(&version, &cli_name)?;
        }

        Commands::Init { name } => {
            let name = name.unwrap_or_else(|| derive_town_name(&cli.town));

            // Initialize global config if needed (ensures password is set)
            let global = GlobalConfig::load_or_init().unwrap_or_default();

            let town = Town::init(&cli.town, &name).await?;
            info!("✨ Initialized town '{}' at {}", name, cli.town.display());

            // Update .gitignore to exclude .tt directory (runtime artifacts)
            let gitignore_path = cli.town.join(".gitignore");
            let tt_entry = ".tt";
            let needs_update = if gitignore_path.exists() {
                let content = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
                !content.lines().any(|line| line.trim() == tt_entry)
            } else {
                true
            };
            if needs_update {
                let mut content = if gitignore_path.exists() {
                    std::fs::read_to_string(&gitignore_path).unwrap_or_default()
                } else {
                    String::new()
                };
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str("\n# Tinytown runtime artifacts\n.tt\n");
                std::fs::write(&gitignore_path, content)?;
                info!("📝 Added .tt to .gitignore");
            }

            // Show appropriate message based on Redis mode
            if global.redis.use_central {
                info!(
                    "📡 Using central Redis on {}:{} (shared across towns)",
                    global.redis.host, global.redis.port
                );
            } else {
                info!("📡 Redis running with Unix socket for fast message passing");
            }
            info!("🚀 Run 'tt spawn <name>' to create agents");

            // Register town in ~/.tt/towns.toml
            if let Err(e) = register_town(&cli.town, &name) {
                info!("⚠️  Could not register town in ~/.tt/towns.toml: {}", e);
            }

            // Keep town alive briefly to show it's working
            drop(town);
        }

        Commands::Spawn {
            name,
            cli: cli_arg,
            max_rounds,
            foreground,
        } => {
            let town = Town::connect(&cli.town).await?;
            validate_spawn_agent_name(&name)?;
            // Priority: CLI arg > town config > global config
            let cli_name = cli_arg.unwrap_or_else(|| {
                let town_cli = &town.config().default_cli;
                if !town_cli.is_empty() {
                    town_cli.clone()
                } else {
                    GlobalConfig::load()
                        .map(|c| c.default_cli)
                        .unwrap_or_else(|_| "claude".to_string())
                }
            });
            let agent = town.spawn_agent(&name, &cli_name).await?;
            let agent_id = agent.id().to_string();

            info!("🤖 Spawned agent '{}' using CLI '{}'", name, cli_name);
            info!("   ID: {}", agent_id);

            // Get the path to this executable
            let exe = std::env::current_exe()?;
            let town_path = cli.town.canonicalize().unwrap_or(cli.town.clone());

            // Clean up old round log files to prevent stale data in 'tt status --deep'
            // This handles the case where an agent is respawned with the same name
            let log_dir = town_path.join(".tt/logs");
            if log_dir.exists() {
                let cleaned = clean_agent_round_logs(&log_dir, &name);
                if cleaned > 0 {
                    info!("   Cleaned {} old round log file(s)", cleaned);
                }
            }

            if foreground {
                // Run agent loop in foreground
                info!("🔄 Running agent loop (max {} rounds)...", max_rounds);
                drop(town); // Release connection before running loop

                let status = std::process::Command::new(&exe)
                    .arg("--town")
                    .arg(&town_path)
                    .arg("agent-loop")
                    .arg(&name)
                    .arg(&agent_id)
                    .arg(max_rounds.to_string())
                    .stdin(std::process::Stdio::inherit())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .status()?;

                if status.success() {
                    info!("✅ Agent '{}' completed", name);
                } else {
                    info!("❌ Agent '{}' exited with error", name);
                }
            } else {
                // Background the agent process
                info!(
                    "🔄 Starting agent loop in background (max {} rounds)...",
                    max_rounds
                );
                info!("   Logs: {}/.tt/logs/{}.log", town_path.display(), name);

                std::fs::create_dir_all(&log_dir)?;
                let log_file = std::fs::File::create(log_dir.join(format!("{}.log", name)))?;

                std::process::Command::new(&exe)
                    .arg("--town")
                    .arg(&town_path)
                    .arg("agent-loop")
                    .arg(&name)
                    .arg(&agent_id)
                    .arg(max_rounds.to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(log_file.try_clone()?)
                    .stderr(log_file)
                    .spawn()?;

                info!("   Agent running in background. Check status with 'tt status'");
            }
        }

        Commands::List => {
            let town = Town::connect(&cli.town).await?;
            let agents = town.list_agents().await;

            if agents.is_empty() {
                info!("No agents. Run 'tt spawn <name>' to create one.");
            } else {
                info!("Agents:");
                for agent in agents {
                    info!("  {} ({}) - {:?}", agent.name, agent.id, agent.state);
                }
            }
        }

        Commands::Assign { agent, task } => {
            let town = Town::connect(&cli.town).await?;
            let result = tinytown::TaskService::assign(&town, &agent, &task).await?;

            info!("📋 Assigned task {} to agent '{}'", result.task_id, agent);
        }

        Commands::Status {
            deep,
            tasks: show_tasks,
        } => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();

            info!("🏘️  Town: {}", config.name);
            info!("📂 Root: {}", town.root().display());
            info!("📡 Redis: {}", config.redis_url_redacted());

            let agents = town.list_agents().await;
            info!("🤖 Agents: {}", agents.len());

            // Fetch tasks once before the agent loop to avoid N+1 Redis calls
            let all_tasks = town.channel().list_tasks().await.unwrap_or_default();

            for agent in &agents {
                let inbox_len = town.channel().inbox_len(agent.id).await.unwrap_or(0);
                let peek_count = std::cmp::min(inbox_len, 200) as isize;
                let inbox_messages = if peek_count > 0 {
                    town.channel()
                        .peek_inbox(agent.id, peek_count)
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut breakdown = MessageBreakdown::default();
                for msg in &inbox_messages {
                    breakdown.count(&msg.msg_type);
                }
                let sampled_note = if inbox_len > inbox_messages.len() {
                    format!(" (sampled first {})", inbox_messages.len())
                } else {
                    String::new()
                };

                // Calculate uptime
                let uptime = chrono::Utc::now() - agent.created_at;
                let uptime_str = if uptime.num_hours() > 0 {
                    format!("{}h {}m", uptime.num_hours(), uptime.num_minutes() % 60)
                } else if uptime.num_minutes() > 0 {
                    format!("{}m {}s", uptime.num_minutes(), uptime.num_seconds() % 60)
                } else {
                    format!("{}s", uptime.num_seconds())
                };

                // Get running tasks assigned to this agent (using pre-fetched all_tasks)
                let running_tasks: Vec<_> = all_tasks
                    .iter()
                    .filter(|t| {
                        t.assigned_to == Some(agent.id) && t.state == tinytown::TaskState::Running
                    })
                    .collect();

                if deep {
                    info!(
                        "   {} ({:?}) - {} pending, {} rounds, uptime {}",
                        agent.name, agent.state, inbox_len, agent.rounds_completed, uptime_str
                    );
                    // Build a more readable pending breakdown with labels
                    let task_count = breakdown.tasks + breakdown.other_actionable;
                    let mut pending_parts = Vec::new();
                    if task_count > 0 {
                        pending_parts.push(format!(
                            "{} task{}",
                            task_count,
                            if task_count == 1 { "" } else { "s" }
                        ));
                    }
                    if breakdown.queries > 0 {
                        pending_parts.push(format!(
                            "{} quer{}",
                            breakdown.queries,
                            if breakdown.queries == 1 { "y" } else { "ies" }
                        ));
                    }
                    if breakdown.informational > 0 {
                        pending_parts.push(format!("{} info", breakdown.informational));
                    }
                    if breakdown.confirmations > 0 {
                        pending_parts.push(format!(
                            "{} ack{}",
                            breakdown.confirmations,
                            if breakdown.confirmations == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ));
                    }
                    if pending_parts.is_empty() {
                        pending_parts.push("no pending messages".to_string());
                    }
                    info!("      └─ 📬 {}{}", pending_parts.join(", "), sampled_note);
                    // Show running tasks assigned to this agent
                    if !running_tasks.is_empty() {
                        for task in &running_tasks {
                            let desc = if task.description.len() > 55 {
                                format!(
                                    "{}...",
                                    &task.description.chars().take(52).collect::<String>()
                                )
                            } else {
                                task.description.clone()
                            };
                            let started = task
                                .started_at
                                .map(|t| {
                                    let elapsed = chrono::Utc::now() - t;
                                    if elapsed.num_hours() > 0 {
                                        format!(
                                            "{}h {}m ago",
                                            elapsed.num_hours(),
                                            elapsed.num_minutes() % 60
                                        )
                                    } else if elapsed.num_minutes() > 0 {
                                        format!("{}m ago", elapsed.num_minutes())
                                    } else {
                                        "just now".to_string()
                                    }
                                })
                                .unwrap_or_default();
                            info!(
                                "      └─ 🔄 {}: {} (started {})",
                                task.id.to_string().chars().take(8).collect::<String>(),
                                desc,
                                started
                            );
                        }
                    }
                    // Get recent activity from Redis
                    if let Ok(Some(activity)) = town.channel().get_agent_activity(agent.id).await {
                        for line in activity.lines().take(5) {
                            info!("      └─ {}", line);
                        }
                    }
                } else {
                    // Show current task indicator for working agents in non-deep mode
                    info!(
                        "   {} ({:?}) - {} pending (T:{} Q:{} I:{} C:{})",
                        agent.name,
                        agent.state,
                        inbox_len,
                        breakdown.tasks + breakdown.other_actionable,
                        breakdown.queries,
                        breakdown.informational,
                        breakdown.confirmations
                    );
                    // Show running tasks for this agent
                    if !running_tasks.is_empty() {
                        let task = &running_tasks[0];
                        let desc = if task.description.len() > 50 {
                            format!(
                                "{}...",
                                &task.description.chars().take(47).collect::<String>()
                            )
                        } else {
                            task.description.clone()
                        };
                        info!("      └─ Working: {}", desc);
                    }
                }
            }

            // Task summary section (reuse pre-fetched all_tasks)
            let tasks = &all_tasks;
            let backlog_count = town.channel().backlog_len().await.unwrap_or(0);

            // Count by state
            let mut pending = 0usize;
            let mut assigned = 0usize;
            let mut running = 0usize;
            let mut completed = 0usize;
            let mut failed = 0usize;
            let mut cancelled = 0usize;

            for task in tasks {
                match task.state {
                    tinytown::TaskState::Pending => pending += 1,
                    tinytown::TaskState::Assigned => assigned += 1,
                    tinytown::TaskState::Running => running += 1,
                    tinytown::TaskState::Completed => completed += 1,
                    tinytown::TaskState::Failed => failed += 1,
                    tinytown::TaskState::Cancelled => cancelled += 1,
                }
            }

            let total = tasks.len();
            let in_flight = assigned + running;
            let done = completed + failed + cancelled;
            // Note: backlog items are already counted in `pending` (they have TaskState::Pending)
            // so we don't add backlog_count again to avoid double-counting
            let pending_total = pending;

            info!(
                "📋 Tasks: {} total ({} pending, {} in-flight, {} done)",
                total, pending_total, in_flight, done
            );

            // Show detailed task breakdown when --tasks flag is passed
            if show_tasks {
                info!("");
                info!("📊 Task Breakdown by State:");
                info!("   ⏳ Pending:   {}", pending);
                info!("   📌 Assigned:  {}", assigned);
                info!("   🔄 Running:   {}", running);
                info!("   ✅ Completed: {}", completed);
                info!("   ❌ Failed:    {}", failed);
                info!("   🚫 Cancelled: {}", cancelled);
                info!("   📋 Backlog:   {}", backlog_count);

                // Group tasks by agent
                let mut tasks_by_agent: std::collections::HashMap<String, Vec<&tinytown::Task>> =
                    std::collections::HashMap::new();
                let mut unassigned_tasks: Vec<&tinytown::Task> = Vec::new();

                for task in tasks {
                    if let Some(agent_id) = task.assigned_to {
                        // Find agent name (reusing pre-fetched agents list)
                        let agent_name = agents
                            .iter()
                            .find(|a| a.id == agent_id)
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| agent_id.to_string());
                        tasks_by_agent.entry(agent_name).or_default().push(task);
                    } else {
                        unassigned_tasks.push(task);
                    }
                }

                // Show tasks by agent
                info!("");
                info!("📋 Tasks by Agent:");
                for (agent_name, agent_tasks) in &tasks_by_agent {
                    let active_count = agent_tasks
                        .iter()
                        .filter(|t| !t.state.is_terminal())
                        .count();
                    let done_count = agent_tasks.iter().filter(|t| t.state.is_terminal()).count();
                    info!(
                        "   {} ({} active, {} done):",
                        agent_name, active_count, done_count
                    );
                    for task in agent_tasks.iter().take(5) {
                        let state_icon = match task.state {
                            tinytown::TaskState::Pending => "⏳",
                            tinytown::TaskState::Assigned => "📌",
                            tinytown::TaskState::Running => "🔄",
                            tinytown::TaskState::Completed => "✅",
                            tinytown::TaskState::Failed => "❌",
                            tinytown::TaskState::Cancelled => "🚫",
                        };
                        let desc = task.description.chars().take(50).collect::<String>();
                        let truncated = if task.description.chars().count() > 50 {
                            "..."
                        } else {
                            ""
                        };
                        info!("      {} {} {}{}", state_icon, task.id, desc, truncated);
                    }
                    if agent_tasks.len() > 5 {
                        info!("      ... and {} more task(s)", agent_tasks.len() - 5);
                    }
                }

                if !unassigned_tasks.is_empty() {
                    info!("   (unassigned) ({} tasks):", unassigned_tasks.len());
                    for task in unassigned_tasks.iter().take(5) {
                        let desc = task.description.chars().take(50).collect::<String>();
                        let truncated = if task.description.chars().count() > 50 {
                            "..."
                        } else {
                            ""
                        };
                        info!("      ⏳ {} {}{}", task.id, desc, truncated);
                    }
                    if unassigned_tasks.len() > 5 {
                        info!("      ... and {} more task(s)", unassigned_tasks.len() - 5);
                    }
                }
            }

            if deep {
                info!("");
                info!("📊 Stats: rounds completed, uptime since spawn");

                // Show recent logs from each agent
                info!("");
                info!("📜 Recent Agent Activity:");
                let log_dir = cli.town.join(".tt/logs");
                if log_dir.exists() {
                    let mut shown_logs = std::collections::HashSet::new();
                    // Reuse the agents variable from earlier to avoid redundant Redis call
                    for agent in &agents {
                        let log_file = log_dir.join(format!("{}.log", agent.name));
                        if log_file.exists() && !shown_logs.contains(&agent.name) {
                            shown_logs.insert(agent.name.clone());
                            info!("");
                            info!("--- {} ---", agent.name);
                            if let Ok(content) = std::fs::read_to_string(&log_file) {
                                let lines: Vec<&str> = content.lines().collect();
                                let start = lines.len().saturating_sub(50);
                                let mut shown = 0;
                                let mut consecutive_rounds: Vec<u32> = Vec::new();
                                let mut last_line: Option<String> = None;

                                for line in &lines[start..] {
                                    if shown >= 15 {
                                        break;
                                    }
                                    // Parse and clean up log lines for better UX
                                    if let Some(cleaned) = clean_log_line(line) {
                                        if cleaned.is_empty() {
                                            continue;
                                        }

                                        // Detect round completion patterns:
                                        // "✅ Round N complete" or "Round N: ✅ completed"
                                        let is_round_complete = (cleaned.contains("Round ")
                                            && cleaned.contains("complete"))
                                            && (cleaned.contains("✅")
                                                || cleaned.contains("completed"));

                                        if is_round_complete {
                                            // Extract round number - try both formats
                                            if let Some(round_str) = cleaned.split("Round ").nth(1)
                                            {
                                                // Handle both "Round N complete" and "Round N:"
                                                let num_part = round_str
                                                    .split_whitespace()
                                                    .next()
                                                    .or_else(|| round_str.split(':').next())
                                                    .unwrap_or("");
                                                if let Ok(round_num) =
                                                    num_part.trim().parse::<u32>()
                                                {
                                                    consecutive_rounds.push(round_num);
                                                    continue;
                                                }
                                            }
                                        }

                                        // Before showing a non-round line, flush any accumulated rounds
                                        if !consecutive_rounds.is_empty() {
                                            if consecutive_rounds.len() == 1 {
                                                info!(
                                                    "  ✅ Round {} completed",
                                                    consecutive_rounds[0]
                                                );
                                            } else {
                                                let min_round =
                                                    consecutive_rounds.iter().min().unwrap_or(&0);
                                                let max_round =
                                                    consecutive_rounds.iter().max().unwrap_or(&0);
                                                info!(
                                                    "  ✅ Rounds {}-{} completed ({} rounds)",
                                                    min_round,
                                                    max_round,
                                                    consecutive_rounds.len()
                                                );
                                            }
                                            shown += 1;
                                            consecutive_rounds.clear();
                                        }

                                        // Skip duplicate consecutive lines
                                        if Some(&cleaned) == last_line.as_ref() {
                                            continue;
                                        }

                                        info!("  {}", cleaned);
                                        last_line = Some(cleaned);
                                        shown += 1;
                                    }
                                }

                                // Flush any remaining accumulated rounds
                                if !consecutive_rounds.is_empty() {
                                    if consecutive_rounds.len() == 1 {
                                        info!("  ✅ Round {} completed", consecutive_rounds[0]);
                                    } else {
                                        let min_round =
                                            consecutive_rounds.iter().min().unwrap_or(&0);
                                        let max_round =
                                            consecutive_rounds.iter().max().unwrap_or(&0);
                                        info!(
                                            "  ✅ Rounds {}-{} completed ({} rounds)",
                                            min_round,
                                            max_round,
                                            consecutive_rounds.len()
                                        );
                                    }
                                }
                            }

                            // Show last lines from most recent round log file
                            // These files show what the AI is actually doing
                            if let Some((round_num, round_log_path)) =
                                find_latest_round_log(&log_dir, &agent.name)
                            {
                                info!("");
                                info!("  📋 Latest Round {} Activity:", round_num);
                                if let Ok(round_content) = std::fs::read_to_string(&round_log_path)
                                {
                                    let round_lines: Vec<&str> = round_content.lines().collect();
                                    // Show last 8 meaningful lines
                                    let mut meaningful_lines: Vec<&str> = Vec::new();
                                    for line in round_lines.iter().rev() {
                                        let trimmed = line.trim();
                                        // Skip empty lines, ANSI-only lines, and noise
                                        if trimmed.is_empty() {
                                            continue;
                                        }
                                        // Skip lines that are mostly ANSI codes
                                        let stripped = strip_ansi_codes(trimmed);
                                        if stripped.is_empty() {
                                            continue;
                                        }
                                        meaningful_lines.push(trimmed);
                                        if meaningful_lines.len() >= 8 {
                                            break;
                                        }
                                    }
                                    // Display in chronological order
                                    meaningful_lines.reverse();
                                    for line in meaningful_lines {
                                        // Clean up and truncate for display (use chars to avoid UTF-8 panic)
                                        let display_line = strip_ansi_codes(line);
                                        let truncated = if display_line.chars().count() > 80 {
                                            format!(
                                                "{}...",
                                                display_line.chars().take(77).collect::<String>()
                                            )
                                        } else {
                                            display_line
                                        };
                                        info!("     {}", truncated);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Commands::Kill { agent } => {
            let town = Town::connect(&cli.town).await?;
            let handle = town.agent(&agent).await?;
            tinytown::AgentService::kill(town.channel(), handle.id()).await?;

            info!("🛑 Requested stop for agent '{}'", agent);
            info!("   Agent will stop at the start of its next round.");
        }

        Commands::Prune { all } => {
            let town = Town::connect(&cli.town).await?;
            let removed = tinytown::AgentService::prune(&town, all).await?;

            for agent in &removed {
                info!(
                    "🗑️  Removed {} ({}) - {:?}",
                    agent.name, agent.id, agent.state
                );
            }

            if removed.is_empty() {
                info!("No agents to prune.");
            } else {
                info!("✨ Pruned {} agent(s)", removed.len());
            }
        }

        Commands::Task { action } => {
            let town = Town::connect(&cli.town).await?;

            match action {
                TaskAction::Complete { task_id, result } => {
                    // Parse task ID
                    let tid: tinytown::TaskId = task_id.parse().map_err(|e| {
                        tinytown::Error::TaskNotFound(format!("Invalid task ID: {}", e))
                    })?;

                    if let Some(completed) =
                        tinytown::TaskService::complete(town.channel(), tid, result).await?
                    {
                        let task = completed.task;
                        let result_msg = completed.result;

                        if let Some((mission_id, work_item_id)) = mission_task_binding(&task.tags) {
                            use tinytown::mission::{MissionScheduler, MissionStorage};

                            let storage = MissionStorage::new(
                                town.channel().conn().clone(),
                                &town.config().name,
                            );
                            let scheduler = MissionScheduler::with_defaults(
                                storage.clone(),
                                town.channel().clone(),
                            );
                            let completion = scheduler
                                .complete_work_item(
                                    mission_id,
                                    work_item_id,
                                    vec![format!("task:{}", tid)],
                                    false,
                                )
                                .await?;
                            match completion {
                                tinytown::mission::WorkItemCompletion::Completed => {
                                    let tick_result = scheduler.tick().await?;
                                    info!(
                                        "   Mission sync: work item completed; scheduler promoted {} and assigned {}",
                                        tick_result.total_promoted, tick_result.total_assigned
                                    );
                                }
                                tinytown::mission::WorkItemCompletion::ReviewerApprovalRequired => {
                                    info!(
                                        "   Mission sync: reviewer approval is still required before the work item can be completed"
                                    );
                                }
                                tinytown::mission::WorkItemCompletion::MissionNotFound => {
                                    warn!(
                                        "   Mission sync: mission {} no longer exists; skipping work item completion sync",
                                        mission_id
                                    );
                                }
                                tinytown::mission::WorkItemCompletion::WorkItemNotFound => {
                                    warn!(
                                        "   Mission sync: work item {} was not found in mission {}; skipping completion sync",
                                        work_item_id, mission_id
                                    );
                                }
                            }
                        }

                        info!("✅ Task {} marked as completed", task_id);
                        info!(
                            "   Description: {}",
                            truncate_summary(&task.description, 60)
                        );
                        info!("   Result: {}", truncate_summary(&result_msg, 60));
                        if completed.cleared_current_task {
                            info!("   Cleared current assignment pointer for agent");
                        }
                        if let Some(tasks_completed) = completed.tasks_completed {
                            info!("   Agent tasks completed: {}", tasks_completed);
                        }
                    } else {
                        info!("❌ Task {} not found", task_id);
                    }
                }

                TaskAction::Show { task_id } => {
                    // Parse task ID
                    let tid: tinytown::TaskId = task_id.parse().map_err(|e| {
                        tinytown::Error::TaskNotFound(format!("Invalid task ID: {}", e))
                    })?;

                    // Get agents for name lookup
                    let agents = town.list_agents().await;

                    if let Some(task) = town.channel().get_task(tid).await? {
                        info!("📋 Task: {}", task.id);
                        info!("   Description: {}", task.description);
                        info!("   State: {:?}", task.state);
                        if let Some(agent_id) = task.assigned_to {
                            // Look up agent name
                            let agent_name = agents
                                .iter()
                                .find(|a| a.id == agent_id)
                                .map(|a| a.name.clone())
                                .unwrap_or_else(|| agent_id.to_string());
                            info!("   Assigned to: {}", agent_name);
                        }
                        info!("   Created: {}", task.created_at);
                        info!("   Updated: {}", task.updated_at);
                        if let Some(started) = task.started_at {
                            info!("   Started: {}", started);
                        }
                        if let Some(completed) = task.completed_at {
                            info!("   Completed: {}", completed);
                        }
                        if let Some(result) = task.result {
                            info!("   Result: {}", result);
                        }
                        if !task.tags.is_empty() {
                            info!("   Tags: {}", task.tags.join(", "));
                        }
                    } else {
                        info!("❌ Task {} not found", task_id);
                    }
                }

                TaskAction::Current { agent } => {
                    let agent_id =
                        resolve_agent_id_for_current_task(&town, agent.as_deref()).await?;
                    let agents = town.list_agents().await;
                    let agent_name = agents
                        .iter()
                        .find(|candidate| candidate.id == agent_id)
                        .map(|candidate| candidate.name.clone())
                        .unwrap_or_else(|| agent_id.to_string());

                    if let Some(task) =
                        tinytown::TaskService::current_for_agent(town.channel(), agent_id).await?
                    {
                        info!("📋 Current task for '{}': {}", agent_name, task.id);
                        info!("   Description: {}", task.description);
                        info!("   State: {:?}", task.state);
                        info!(
                            "   Complete with: tt task complete {} --result \"what was done\"",
                            task.id
                        );
                        if !task.tags.is_empty() {
                            info!("   Tags: {}", task.tags.join(", "));
                        }
                    } else {
                        info!("📭 No current task tracked for '{}'", agent_name);
                    }
                }

                TaskAction::List { state } => {
                    let tasks = town.channel().list_tasks().await?;
                    // Get agents for name lookup
                    let agents = town.list_agents().await;

                    if tasks.is_empty() {
                        info!("📋 No tasks found");
                    } else {
                        // Filter by state if provided
                        let filtered: Vec<_> = if let Some(ref state_filter) = state {
                            let target_state: tinytown::TaskState = match state_filter
                                .to_lowercase()
                                .as_str()
                            {
                                "pending" => tinytown::TaskState::Pending,
                                "assigned" => tinytown::TaskState::Assigned,
                                "running" => tinytown::TaskState::Running,
                                "completed" => tinytown::TaskState::Completed,
                                "failed" => tinytown::TaskState::Failed,
                                "cancelled" => tinytown::TaskState::Cancelled,
                                _ => {
                                    info!(
                                        "❌ Unknown state filter: {}. Valid: pending, assigned, running, completed, failed, cancelled",
                                        state_filter
                                    );
                                    return Ok(());
                                }
                            };
                            tasks
                                .into_iter()
                                .filter(|t| t.state == target_state)
                                .collect()
                        } else {
                            tasks
                        };

                        if filtered.is_empty() {
                            info!(
                                "📋 No tasks found with state '{}'",
                                state.unwrap_or_default()
                            );
                        } else {
                            info!("📋 Tasks ({}):", filtered.len());
                            for task in &filtered {
                                let status_icon = match task.state {
                                    tinytown::TaskState::Pending => "⏳",
                                    tinytown::TaskState::Assigned => "📌",
                                    tinytown::TaskState::Running => "🔄",
                                    tinytown::TaskState::Completed => "✅",
                                    tinytown::TaskState::Failed => "❌",
                                    tinytown::TaskState::Cancelled => "🚫",
                                };
                                // Look up agent name instead of showing UUID
                                let agent = task
                                    .assigned_to
                                    .and_then(|agent_id| {
                                        agents
                                            .iter()
                                            .find(|a| a.id == agent_id)
                                            .map(|a| a.name.clone())
                                    })
                                    .unwrap_or_else(|| "unassigned".to_string());
                                info!(
                                    "   {} {} - {} [{}]",
                                    status_icon,
                                    task.id,
                                    truncate_summary(&task.description, 50),
                                    agent
                                );
                            }
                        }
                    }
                }
            }
        }

        Commands::Start => {
            let _town = Town::connect(&cli.town).await?;
            info!("🚀 Town connection open");
            // Keep running until Ctrl+C
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for ctrl-c");
            info!("👋 Closing town connection...");
        }

        Commands::Stop => {
            let town = Town::connect(&cli.town).await?;
            let requested = tinytown::AgentService::stop_all(&town).await?;

            if requested.is_empty() {
                info!(
                    "👋 No active agents to stop in town '{}'",
                    town.config().name
                );
            } else {
                info!(
                    "🛑 Requested graceful stop for {} agent(s) in town '{}'",
                    requested.len(),
                    town.config().name
                );
                info!("   Agents will stop at the start of their next round.");
            }

            info!("   Central Redis remains available to other towns.");
        }

        Commands::Reset { force, agents_only } => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();

            // Show what will be deleted
            let agents = town.list_agents().await;

            if agents_only {
                info!("🗑️  Resetting agents in town '{}'", config.name);
                info!("   This will delete:");
                info!("   - {} agent(s) and their inboxes", agents.len());
                info!("   Tasks and backlog will be preserved.");

                if !force {
                    info!("");
                    info!("⚠️  This action cannot be undone!");
                    info!("   Run with --force to confirm: tt reset --agents-only --force");
                    return Ok(());
                }

                // Perform agents-only reset
                let deleted = town.channel().reset_agents_only().await?;

                info!("");
                info!(
                    "✅ Reset complete: deleted {} Redis keys (agents only)",
                    deleted
                );
                info!("   Run 'tt spawn <name>' to create new agents");
            } else {
                let tasks = town.channel().list_tasks().await.unwrap_or_default();
                let backlog_len = town.channel().backlog_len().await.unwrap_or(0);

                info!("🗑️  Resetting town '{}'", config.name);
                info!("   This will delete:");
                info!("   - {} agent(s)", agents.len());
                info!("   - {} task(s)", tasks.len());
                info!("   - {} backlog item(s)", backlog_len);

                if !force {
                    info!("");
                    info!("⚠️  This action cannot be undone!");
                    info!("   Run with --force to confirm: tt reset --force");
                    return Ok(());
                }

                // Perform the full reset
                let deleted = town.channel().reset_all().await?;

                info!("");
                info!("✅ Reset complete: deleted {} Redis keys", deleted);
                info!("   Run 'tt spawn <name>' to create new agents");
            }
        }

        Commands::Inbox { agent, all } => {
            let town = Town::connect(&cli.town).await?;

            if all {
                // Show pending messages for all agents (replaces old 'tt tasks' command)
                let agents = town.list_agents().await;
                let supervisor_inbox =
                    sampled_inbox(town.channel(), tinytown::AgentId::supervisor(), 100)
                        .await
                        .unwrap_or((0, Vec::new(), MessageBreakdown::default()));

                if agents.is_empty() && supervisor_inbox.0 == 0 {
                    info!("No agents. Run 'tt spawn <name>' to create one.");
                } else {
                    info!("📋 Pending Messages by Agent:");
                    info!("");

                    let mut total_actionable = 0;
                    let mut printed_any = false;
                    for agent in &agents {
                        let (inbox_len, messages, breakdown) =
                            sampled_inbox(town.channel(), agent.id, 100)
                                .await
                                .unwrap_or((0, Vec::new(), MessageBreakdown::default()));
                        if inbox_len == 0 {
                            continue;
                        }

                        printed_any = true;
                        let heading = format!("{} ({:?})", agent.name, agent.state);
                        print_all_inbox_section(
                            town.channel(),
                            &heading,
                            inbox_len,
                            &messages,
                            breakdown,
                        )
                        .await;
                        total_actionable += breakdown.actionable_count();
                    }

                    if supervisor_inbox.0 > 0 {
                        printed_any = true;
                        let (inbox_len, messages, breakdown) = supervisor_inbox;
                        print_all_inbox_section(
                            town.channel(),
                            "supervisor/conductor (well-known mailbox)",
                            inbox_len,
                            &messages,
                            breakdown,
                        )
                        .await;
                        total_actionable += breakdown.actionable_count();
                    }

                    if !printed_any {
                        info!("  (no pending messages)");
                    } else {
                        info!("Total: {} actionable message(s)", total_actionable);
                    }
                }
            } else if let Some(agent_name) = agent {
                // Show inbox for a specific agent
                let handle = town.agent(&agent_name).await?;
                let agent_id = handle.id();
                let display_name = if is_supervisor_alias(&agent_name) {
                    format!("{} (well-known supervisor/conductor mailbox)", agent_name)
                } else {
                    agent_name.clone()
                };

                let (inbox_len, messages, breakdown) =
                    sampled_inbox(town.channel(), agent_id, 100).await?;
                info!("📬 Inbox for '{}': {} messages", display_name, inbox_len);

                if inbox_len > 0 {
                    info!(
                        "   [T] {} tasks requiring action",
                        breakdown.tasks + breakdown.other_actionable
                    );
                    info!("   [Q] {} queries awaiting response", breakdown.queries);
                    info!("   [I] {} informational", breakdown.informational);
                    info!("   [C] {} confirmations", breakdown.confirmations);
                    info!("");

                    let preview_limit = 10;
                    let shown = std::cmp::min(messages.len(), preview_limit);
                    for msg in messages.iter().take(preview_limit) {
                        let summary = describe_message(town.channel(), &msg.msg_type).await;
                        info!(
                            "   {} {}",
                            inbox_preview_prefix(&msg.msg_type),
                            truncate_summary(&summary, 120)
                        );
                    }

                    if inbox_len > shown {
                        info!("   …plus {} more message(s)", inbox_len - shown);
                    }
                }
            } else {
                info!("Usage: tt inbox <AGENT> or tt inbox --all");
                info!("  tt inbox <agent>  - Show inbox for a specific agent");
                info!("  tt inbox --all    - Show pending messages for all agents");
            }
        }

        Commands::Send {
            to,
            message,
            query,
            info: informational,
            ack,
            urgent,
        } => {
            use tinytown::{AgentId, Message, MessageType};

            let town = Town::connect(&cli.town).await?;
            let to_handle = town.agent(&to).await?;
            let to_id = to_handle.id();

            let (msg_type, label) = if query {
                (MessageType::Query { question: message }, "query")
            } else if informational {
                (
                    MessageType::Informational { summary: message },
                    "informational",
                )
            } else if ack {
                (
                    MessageType::Confirmation {
                        ack_type: parse_confirmation_type(&message),
                    },
                    "confirmation",
                )
            } else {
                (
                    MessageType::Task {
                        description: message,
                    },
                    "task",
                )
            };

            let msg = Message::new(AgentId::supervisor(), to_id, msg_type);

            if urgent {
                town.channel().send_urgent(&msg).await?;
                info!("🚨 Sent URGENT {} message to '{}'", label, to);
            } else {
                town.channel().send(&msg).await?;
                info!("📤 Sent {} message to '{}'", label, to);
            }
        }

        Commands::AgentLoop {
            name,
            id,
            max_rounds,
        } => {
            // This is the actual agent worker loop
            // It runs the AI model repeatedly, checking inbox for tasks

            use std::time::Duration;
            use tinytown::{AgentId, AgentState};

            let town = Town::connect(&cli.town).await?;
            let config = town.config();
            let channel = town.channel();

            // Parse agent ID
            let agent_id: AgentId = id
                .parse()
                .map_err(|_| tinytown::Error::AgentNotFound(format!("Invalid agent ID: {}", id)))?;

            // Get CLI command
            let agent_state = channel.get_agent_state(agent_id).await?;
            let cli_name = agent_state
                .as_ref()
                .map(|a| a.cli.clone())
                .unwrap_or_else(|| config.default_cli.clone());
            let cli_cmd = config
                .agent_clis
                .get(&cli_name)
                .map(|c| c.command.clone())
                .unwrap_or_else(|| cli_name.clone());

            info!(
                "🔄 Agent '{}' starting loop (max {} rounds)",
                name, max_rounds
            );
            info!("   CLI: {} ({})", cli_name, cli_cmd);

            // Use manual counter - only increment AFTER CLI execution (fixes round-burning bug)
            let mut round: u32 = 0;

            loop {
                // Check if we've hit max rounds
                if round >= max_rounds {
                    break;
                }

                info!("\n📍 Round {}/{}", round + 1, max_rounds);

                // Check if stop has been requested
                if channel.should_stop(agent_id).await? {
                    info!("   🛑 Stop requested, exiting gracefully...");
                    channel
                        .log_agent_activity(
                            agent_id,
                            &format!("Round {}: 🛑 stopped by request", round + 1),
                        )
                        .await?;
                    channel.clear_stop(agent_id).await?;
                    break;
                }

                let display_round = round + 1;
                let urgent_messages = channel.receive_urgent(agent_id).await?;
                let mut regular_messages = channel.drain_inbox(agent_id).await?;
                let backlog_snapshot = backlog_snapshot_for_agent(channel, &name, 8).await?;

                if regular_messages.is_empty() && urgent_messages.is_empty() {
                    if backlog_snapshot.total_matching > 0 {
                        info!(
                            "   📋 Inbox empty, but {} backlog task(s) match this role; prompting backlog review",
                            backlog_snapshot.total_matching
                        );
                        regular_messages.push(tinytown::Message::new(
                            AgentId::supervisor(),
                            agent_id,
                            tinytown::MessageType::Query {
                                question: format!(
                                    "Backlog has {} role-matching task(s). Review `tt backlog list` and claim one using `tt backlog claim <task-id> {}`.",
                                    backlog_snapshot.total_matching, name
                                ),
                            },
                        ));
                    } else if backlog_snapshot.total_backlog > 0 {
                        info!(
                            "   📋 Backlog has {} task(s), but none match this role hint; waiting",
                            backlog_snapshot.total_backlog
                        );
                        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                            agent.state = AgentState::Idle;
                            agent.last_heartbeat = chrono::Utc::now();
                            channel.set_agent_state(&agent).await?;
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    } else {
                        info!("   📭 Inbox empty, waiting...");
                        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                            agent.state = AgentState::Idle;
                            agent.last_heartbeat = chrono::Utc::now();
                            channel.set_agent_state(&agent).await?;
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }

                let mut breakdown = MessageBreakdown::default();
                let mut actionable_messages: Vec<(tinytown::Message, bool)> = Vec::new();
                let mut informational_summaries: Vec<String> = Vec::new();
                let mut confirmation_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();

                for msg in urgent_messages {
                    breakdown.count(&msg.msg_type);
                    match classify_message(&msg.msg_type) {
                        MessageCategory::Task
                        | MessageCategory::Query
                        | MessageCategory::OtherActionable => {
                            actionable_messages.push((msg, true));
                        }
                        MessageCategory::Informational => {
                            informational_summaries
                                .push(truncate_summary(&summarize_message(&msg.msg_type), 100));
                        }
                        MessageCategory::Confirmation => {
                            let key = truncate_summary(&summarize_message(&msg.msg_type), 60);
                            *confirmation_counts.entry(key).or_insert(0) += 1;
                        }
                    }
                }

                for msg in regular_messages {
                    breakdown.count(&msg.msg_type);
                    match classify_message(&msg.msg_type) {
                        MessageCategory::Task
                        | MessageCategory::Query
                        | MessageCategory::OtherActionable => {
                            actionable_messages.push((msg, false));
                        }
                        MessageCategory::Informational => {
                            informational_summaries
                                .push(truncate_summary(&summarize_message(&msg.msg_type), 100));
                        }
                        MessageCategory::Confirmation => {
                            let key = truncate_summary(&summarize_message(&msg.msg_type), 60);
                            *confirmation_counts.entry(key).or_insert(0) += 1;
                        }
                    }
                }

                info!(
                    "   📬 batched: {} actionable, {} informational, {} confirmations",
                    actionable_messages.len(),
                    informational_summaries.len(),
                    breakdown.confirmations
                );

                if actionable_messages.is_empty() {
                    if backlog_snapshot.total_matching > 0 {
                        info!(
                            "   📋 No direct actionable messages; {} backlog task(s) match this role, prompting claim review",
                            backlog_snapshot.total_matching
                        );
                        actionable_messages.push((
                            tinytown::Message::new(
                                AgentId::supervisor(),
                                agent_id,
                                tinytown::MessageType::Query {
                                    question: format!(
                                        "No direct assignments right now. Backlog has {} role-matching task(s): review and claim one with `tt backlog claim <task-id> {}`.",
                                        backlog_snapshot.total_matching, name
                                    ),
                                },
                            ),
                            false,
                        ));
                    } else if backlog_snapshot.total_backlog > 0 {
                        let summary = format!(
                            "Round {}: ⏭️ no direct work and {} backlog task(s) did not match role hint",
                            display_round, backlog_snapshot.total_backlog
                        );
                        info!("   {}", summary);
                        channel.log_agent_activity(agent_id, &summary).await?;

                        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                            agent.state = AgentState::Idle;
                            agent.last_heartbeat = chrono::Utc::now();
                            channel.set_agent_state(&agent).await?;
                        }

                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    } else {
                        let summary = format!(
                            "Round {}: ⏭️ auto-handled {} informational, {} confirmations",
                            display_round,
                            informational_summaries.len(),
                            breakdown.confirmations
                        );
                        info!("   {}", summary);
                        channel.log_agent_activity(agent_id, &summary).await?;

                        if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                            agent.state = AgentState::Idle;
                            agent.last_heartbeat = chrono::Utc::now();
                            channel.set_agent_state(&agent).await?;
                        }

                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                }

                let urgent_actionable = actionable_messages
                    .iter()
                    .filter(|(_, urgent)| *urgent)
                    .count();
                track_current_task_for_round(channel, agent_id, &actionable_messages).await?;
                let actionable_section =
                    format_actionable_section(channel, &actionable_messages).await;

                let informational_section = if informational_summaries.is_empty() {
                    String::new()
                } else {
                    let mut section = String::from("\n## Informational (batched summary)\n\n");
                    for summary in informational_summaries.iter().take(8) {
                        section.push_str(&format!("- {}\n", summary));
                    }
                    if informational_summaries.len() > 8 {
                        section.push_str(&format!(
                            "- ...and {} more informational message(s)\n",
                            informational_summaries.len() - 8
                        ));
                    }
                    section
                };

                let confirmation_section = if confirmation_counts.is_empty() {
                    String::new()
                } else {
                    let mut section = String::from("\n## Confirmations (auto-dismissed)\n\n");
                    for (kind, count) in &confirmation_counts {
                        section.push_str(&format!("- {} x{}\n", kind, count));
                    }
                    section
                };

                let role_hint = backlog_role_hint(&name);
                let backlog_section = {
                    let mut section = format!(
                        "\n## Backlog Snapshot\n\n- Total backlog tasks: {}\n- Role-matching backlog tasks: {}\n- Role match hint: {}\n",
                        backlog_snapshot.total_backlog, backlog_snapshot.total_matching, role_hint
                    );
                    if backlog_snapshot.total_matching > 0 {
                        section.push_str("\nReview and claim role-matching items:\n");
                        for (task_id, task) in &backlog_snapshot.tasks {
                            let tags = if task.tags.is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", task.tags.join(", "))
                            };
                            section.push_str(&format!(
                                "- {} - {}{}\n",
                                task_id,
                                truncate_summary(&task.description, 90),
                                tags
                            ));
                        }
                        if backlog_snapshot.total_matching > backlog_snapshot.tasks.len() {
                            section.push_str(&format!(
                                "- ...and {} more role-matching backlog task(s)\n",
                                backlog_snapshot.total_matching - backlog_snapshot.tasks.len()
                            ));
                        }
                    } else if backlog_snapshot.total_backlog > 0 {
                        section.push_str(
                            "\nNo backlog tasks currently match your role hint. Do not claim unrelated work by default.\n",
                        );
                    }
                    section
                };

                let prompt = format!(
                    r#"# Agent: {name}

You are agent "{name}" in Tinytown "{town_name}".

{actionable_section}{informational_section}{confirmation_section}
## Available Commands

```bash
tt status                              # Check town status and all agents
tt assign <agent> "task"               # Assign actionable work
tt backlog list                        # Review unassigned backlog tasks
tt backlog claim <task_id> {agent_name}   # Claim a backlog task for yourself
tt send <agent> --query "question"     # Ask for a response
tt send <agent> --info "update"        # Send FYI update
tt send <agent> --ack "received"       # Send acknowledgment
tt send <agent> --urgent --query "..." # Priority message for next round
tt task current                        # Show your tracked current assignment
tt task complete <task_id> --result "summary"  # Mark a task as done
```

{backlog_section}
## Current State
- Round: {display_round}/{max_rounds}
- Actionable messages: {actionable_count}
- Urgent actionable: {urgent_actionable}
- Batched informational: {info_count}
- Auto-dismissed confirmations: {confirmation_count}

## Your Workflow

1. Handle all actionable messages listed above.
2. If you have no direct assignment or extra capacity, review backlog and claim one role-matching task.
3. Claim only work that matches your role hint; do not claim unrelated tasks.
4. Prefer direct agent-to-agent messages for concrete execution handoffs, review requests, and unblock checks.
5. Use `supervisor` / `conductor` when you need human guidance, priority changes, broader sequencing, escalation, or town-wide visibility.
6. If blocked, send a query with specific unblock needs.
7. Use `tt task current` to confirm the real Tinytown task id before completing work; never use mission/work-item UUIDs from the description as the task id.
8. When finished with a task, mark it complete: `tt task complete <task_id> --result "what was done"`
9. Send informational updates or confirmations as appropriate, including FYI summaries to supervisor/conductor when the conductor should stay informed.

Only run commands needed to complete listed work; inbox messages for this round are already provided above.
"#,
                    name = name,
                    agent_name = name,
                    town_name = config.name,
                    actionable_section = actionable_section,
                    informational_section = informational_section,
                    confirmation_section = confirmation_section,
                    backlog_section = backlog_section,
                    display_round = display_round,
                    max_rounds = max_rounds,
                    actionable_count = actionable_messages.len(),
                    urgent_actionable = urgent_actionable,
                    info_count = informational_summaries.len(),
                    confirmation_count = breakdown.confirmations,
                );

                // Write prompt to temp file (under .tt/)
                let prompt_file = cli.town.join(format!(".tt/agent_{}_prompt.md", name));
                std::fs::write(&prompt_file, &prompt)?;

                // Update agent state to working
                if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                    agent.state = AgentState::Working;
                    channel.set_agent_state(&agent).await?;
                }

                // Run the agent CLI
                info!("   🤖 Running {}...", cli_name);
                let output_file = cli
                    .town
                    .join(format!(".tt/logs/{}_round_{}.log", name, display_round));
                let output = std::fs::File::create(&output_file)?;

                let shell_cmd = build_cli_command(&cli_name, &cli_cmd, &prompt_file);
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&shell_cmd)
                    .current_dir(&cli.town)
                    .env(TT_AGENT_ID_ENV, agent_id.to_string())
                    .env(TT_AGENT_NAME_ENV, &name)
                    .stdin(std::process::Stdio::null())
                    .stdout(output.try_clone()?)
                    .stderr(output)
                    .status();

                // Clean up prompt file
                let _ = std::fs::remove_file(&prompt_file);

                // Log activity and result
                let activity_msg = match &status {
                    Ok(s) if s.success() => {
                        info!("   ✅ Round {} complete", display_round);
                        format!("Round {}: ✅ completed", display_round)
                    }
                    Ok(_) => {
                        info!("   ⚠️ CLI exited with error");
                        format!("Round {}: ⚠️ CLI error", display_round)
                    }
                    Err(e) => {
                        info!("   ❌ Failed to run CLI: {}", e);
                        format!("Round {}: ❌ failed: {}", display_round, e)
                    }
                };

                // Store activity in Redis (bounded, TTL'd)
                channel.log_agent_activity(agent_id, &activity_msg).await?;

                let should_requeue = match &status {
                    Ok(s) => !s.success(),
                    Err(_) => true,
                };
                if should_requeue {
                    info!(
                        "   ↩️ Re-queueing {} actionable message(s)",
                        actionable_messages.len()
                    );
                    for (msg, was_urgent) in &actionable_messages {
                        if *was_urgent {
                            channel.send_urgent(msg).await?;
                        } else {
                            channel.send(msg).await?;
                        }
                    }
                }

                if status.is_err() {
                    break;
                }

                // Increment round counter AFTER successful CLI execution (fixes round-burning bug)
                round += 1;

                // Update agent state back to idle and increment stats
                if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                    agent.state = AgentState::Idle;
                    agent.rounds_completed += 1;
                    agent.last_heartbeat = chrono::Utc::now();
                    channel.set_agent_state(&agent).await?;
                    info!("   📊 Rounds completed: {}", agent.rounds_completed);
                } else {
                    warn!("   ⚠️ Could not update agent state - agent not found in Redis");
                }

                // Small delay between rounds
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // Mark agent as stopped with final stats
            if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                agent.state = AgentState::Stopped;
                agent.last_heartbeat = chrono::Utc::now();
                channel.set_agent_state(&agent).await?;
                info!(
                    "🏁 Agent '{}' finished: {} rounds, {} tasks",
                    name, agent.rounds_completed, agent.tasks_completed
                );
            } else {
                info!("🏁 Agent '{}' finished after {} rounds", name, max_rounds);
            }
        }

        Commands::Conductor => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();
            let backlog_count = town.channel().backlog_len().await.unwrap_or(0);

            // Build conductor context with current state
            let agents = town.list_agents().await;
            let mut agent_status = String::new();
            for agent in &agents {
                let inbox = town.channel().inbox_len(agent.id).await.unwrap_or(0);
                agent_status.push_str(&format!(
                    "  - {} ({:?}) - {} messages pending\n",
                    agent.name, agent.state, inbox
                ));
            }

            // Detect if this is a fresh start or resuming
            let is_fresh_start = agents.is_empty();
            let startup_mode = if is_fresh_start {
                format!(
                    r#"## 🆕 Fresh Start

This is a new town with no agents yet. Your first job is to help the user:

1. **Understand their goal**: What do they want to build or accomplish?
2. **Analyze the project**: Look at the codebase, README, or any design docs
3. **Suggest team roles**: Based on the project, recommend which agents would help:

### Common Team Roles

| Role | When to Use |
|------|-------------|
| `backend` | API development, server-side logic |
| `frontend` | UI/UX implementation |
| `tester` | Writing and running tests |
| `reviewer` | **Always include** - quality gate for all work |
| `devops` | CI/CD, deployment, infrastructure |
| `security` | Security review, vulnerability analysis |
| `docs` | Documentation, API specs, README updates |
| `architect` | System design, code structure decisions |

4. **Break down the work**: Help decompose their idea into specific, assignable tasks
5. **Use backlog for unassigned work**: If ownership is unclear, park tasks in backlog and let role-matched agents claim them

### First Interaction Template

Ask the user:
> "I'm ready to help orchestrate your project! To get started:
> 1. What are you trying to build or accomplish?
> 2. Is there a design doc, README, or existing code I should analyze?
> 3. Based on that, I'll suggest which agents to spawn and how to break down the work."

If they provide a design or task, analyze it and propose:
- Which agents to spawn (always include reviewer!)
- Task breakdown with assignments
- Suggested order of execution

Backlog currently has **{backlog_count}** task(s)."#,
                    backlog_count = backlog_count
                )
            } else {
                format!(
                    r#"## 🔄 Resuming Session

You have existing agents running:
{agent_status}
Check their status with `tt status --deep` to see progress, then continue coordinating.
Backlog currently has **{backlog_count}** task(s).

If work is stalled or you need to pivot, you can:
- `tt kill <agent>` to stop agents
- Spawn new agents for different roles
- Reassign tasks as needed
- Use `tt backlog list` and `tt backlog claim <task-id> <agent>` for unassigned tasks"#,
                    agent_status = agent_status,
                    backlog_count = backlog_count
                )
            };

            if agent_status.is_empty() {
                agent_status = "  (no agents spawned yet)\n".to_string();
            }

            let context = format!(
                r#"# Tinytown Conductor

You are the **conductor** of Tinytown "{name}" - like the train conductor guiding the miniature train through Tiny Town, Colorado, you coordinate AI agents working on this project.

## Current Town State

**Town:** {name}
**Location:** {root}
**Agents ({agent_count}):**
{agent_status}
**Backlog tasks:** {backlog_count}

{startup_mode}

## Your Capabilities

You have access to the `tt` CLI tool. Run these commands in your shell to orchestrate:

### Spawn agents (starts actual AI process!)
```bash
tt spawn <name>                    # Spawn agent with default CLI (backgrounds)
tt spawn <name> --foreground       # Run in foreground (see output)
tt spawn <name> --max-rounds 5     # Limit iterations (default: 10)
```

### Assign tasks
```bash
tt assign <agent> "<task description>"
```

### Manage backlog (unassigned tasks)
```bash
tt backlog add "<task description>" --tags backend,api
tt backlog list
tt backlog claim <task_id> <agent>
tt backlog assign-all <agent>
```

### Send messages between agents
```bash
tt send <agent> "task"             # Send actionable task message
tt send <agent> --query "question" # Ask for a response
tt send <agent> --info "update"    # Send FYI update
tt send <agent> --ack "received"   # Send acknowledgment
tt send <agent> --urgent --query "msg" # URGENT: processed first next round
tt inbox <agent>                   # Check agent's inbox
```

### Check status and stats
```bash
tt status         # Overview of town and agents
tt status --deep  # Stats: rounds completed, uptime, recent activity
tt list           # List all agents
```

### Stop agents
```bash
tt kill <agent>   # Request agent to stop gracefully (at start of next round)
```

### Plan and persist tasks
```bash
tt plan --init              # Create tasks.toml for planning
tt plan                     # View planned tasks
tt sync push                # Send tasks.toml to Redis
tt sync pull                # Save Redis state to tasks.toml (for git)
tt save                     # Save Redis AOF snapshot (for version control)
```

## Your Role

1. **Understand** what the user wants to accomplish
2. **Break down** complex requests into discrete tasks
3. **Spawn** appropriate agents including a **reviewer** for quality control
4. **Assign** tasks to agents with clear, actionable descriptions
5. **Use backlog** for unassigned work and role-based claiming
6. **Monitor** progress with `tt status --deep` (shows rounds, uptime, activity)
7. **Coordinate** handoffs between agents without becoming the bottleneck
8. **Use reviewer outcomes** to decide when work is complete
9. **Cleanup**: When done, stop agents with `tt kill <agent>`

## The Reviewer Pattern

Always spawn a **reviewer** agent. This agent decides when work is satisfactorily done, but the next execution step should usually flow directly to the owning worker:

1. Worker completes task → worker or conductor routes review to reviewer
2. Reviewer checks the work → approves or sends concrete fixes directly to the owning worker
3. Reviewer or worker sends `--info` to supervisor/conductor when visibility matters
4. You step in for human decisions, priority changes, cross-team sequencing, or escalation

This keeps execution flowing: agents hand off obvious next steps directly, reviewer remains the quality gate, and you stay focused on higher-level orchestration.

## Guidelines

- **Always spawn a reviewer** - they're your quality gate
- Be proactive: spawn agents and assign tasks without waiting to be told exactly how
- Be specific: task descriptions should be clear and actionable
- Be efficient: parallelize independent work across multiple agents
- Prefer direct worker/reviewer/worker coordination when the next handoff is obvious
- Keep the conductor in the loop with `tt send supervisor --info ...` when humans need visibility without blocking execution
- Check `tt status` frequently to monitor progress
- Keep backlog flowing: if an agent goes idle, have it review backlog and claim role-matching work
- **Save state to git**: Run `tt sync pull` periodically to save task state to tasks.toml, then suggest committing it

## Example Workflow

User: "Build a user authentication system"

You:
1. `tt spawn backend` - for implementation
2. `tt spawn tester` - for tests
3. `tt spawn reviewer` - for quality control (ALWAYS include this)
4. `tt assign backend "Implement REST API for user auth: POST /signup, POST /login, POST /logout, POST /reset-password. Use bcrypt for passwords."`
5. `tt assign tester "Write integration tests for auth API: test signup, login, logout, password reset. Cover success and error cases."`
6. Monitor with `tt status`
7. When backend is ready: backend or conductor notifies reviewer directly with `tt send reviewer "Auth API implementation complete. Review src/auth.rs and route fixes back to backend if needed."`
8. If reviewer finds concrete issues → reviewer sends them directly to backend and copies supervisor/conductor with `--info`
9. If reviewer approves → done! If broader coordination is needed → you step in and reassign or reprioritize.
10. Save state: `tt sync pull` to save tasks to tasks.toml
11. Suggest: "Run `git add tasks.toml && git commit -m 'Update task state'` to persist"

Now, help the user orchestrate their project!
"#,
                name = config.name,
                root = cli.town.display(),
                agent_count = agents.len(),
                agent_status = agent_status,
                backlog_count = backlog_count,
                startup_mode = startup_mode,
            );

            // Write context to a temp file for the CLI (under .tt/)
            let context_file = cli.town.join(".tt/conductor_context.md");
            std::fs::write(&context_file, &context)?;

            // Get the CLI name (conductor runs interactively, not in --print mode)
            let cli_name = &config.default_cli;

            info!("🚂 Starting conductor with {} CLI...", cli_name);
            info!("   Context: {}", context_file.display());
            info!("");

            // Build the interactive command (no --print flag)
            // For conductor, we want full interactive mode
            let exec_cmd = match cli_name.as_str() {
                "auggie" => format!(
                    "exec auggie --instruction-file '{}'",
                    context_file.display()
                ),
                "claude" => format!("exec claude --resume '{}'", context_file.display()),
                "aider" => format!("exec aider --message-file '{}'", context_file.display()),
                _ => {
                    // For unknown CLIs, try piping the context
                    format!("cat '{}' | exec {}", context_file.display(), cli_name)
                }
            };

            info!("   Running: {}", exec_cmd);
            info!("");

            // Use exec to replace this process with the CLI
            // This gives full interactive control (stdin/stdout/stderr)
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new("sh")
                .arg("-c")
                .arg(&exec_cmd)
                .current_dir(&cli.town)
                .exec();

            // If we get here, exec failed
            eprintln!("❌ Failed to exec conductor: {}", err);
            std::process::exit(1);
        }

        Commands::Plan { init } => {
            if init {
                plan::init_tasks_file(&cli.town)?;
                info!("📝 Created tasks.toml - edit it to plan your work!");
            } else {
                // Open tasks.toml in editor
                let tasks_file = cli.town.join("tasks.toml");
                if !tasks_file.exists() {
                    info!("No tasks.toml found. Run 'tt plan --init' first.");
                } else {
                    let tasks = plan::load_tasks_file(&cli.town)?;
                    info!("📋 Tasks in plan ({}):", tasks_file.display());
                    for task in &tasks.tasks {
                        let status_icon = match task.status.as_str() {
                            "pending" => "⏳",
                            "assigned" => "📌",
                            "running" => "🔄",
                            "completed" => "✅",
                            "failed" => "❌",
                            _ => "❓",
                        };
                        let agent = task.agent.as_deref().unwrap_or("unassigned");
                        info!(
                            "  {} [{}] {} - {}",
                            status_icon, agent, task.id, task.description
                        );
                    }
                }
            }
        }

        Commands::Sync { direction } => {
            let town = Town::connect(&cli.town).await?;
            match direction.as_str() {
                "push" => {
                    let count = plan::push_tasks_to_redis(&cli.town, town.channel()).await?;
                    info!("⬆️  Pushed {} tasks from tasks.toml to Redis", count);
                }
                "pull" => {
                    let count = plan::pull_tasks_from_redis(&cli.town, town.channel()).await?;
                    info!("⬇️  Pulled {} tasks from Redis to tasks.toml", count);
                }
                _ => {
                    info!("Usage: tt sync [push|pull]");
                    info!("  push - Send tasks.toml to Redis");
                    info!("  pull - Save Redis tasks to tasks.toml");
                }
            }
        }

        Commands::Save => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();
            let aof_path = cli.town.join(&config.redis.aof_path);

            // Trigger Redis BGREWRITEAOF to compact and save
            info!("💾 Saving Redis state...");

            let redis_url = config.redis_url();
            let client = redis::Client::open(redis_url)?;
            let mut conn = client.get_multiplexed_async_connection().await?;

            // Trigger background rewrite
            let _: () = redis::cmd("BGREWRITEAOF").query_async(&mut conn).await?;

            info!("   AOF rewrite triggered. File: {}", aof_path.display());
            info!("");
            info!("   To version control Redis state:");
            info!("   git add {}", config.redis.aof_path);
            info!("   git commit -m 'Save town state'");
        }

        Commands::Restore => {
            let config = tinytown::Config::load(&cli.town)?;
            let aof_path = cli.town.join(&config.redis.aof_path);

            if !aof_path.exists() {
                info!("❌ No AOF file found at: {}", aof_path.display());
                info!("   Run 'tt save' first to create one.");
            } else {
                info!("📂 AOF file found: {}", aof_path.display());
                info!("");
                info!("   To restore from AOF:");
                info!("   1. Stop Redis if running");
                info!(
                    "   2. Start Redis with: redis-server --appendonly yes --appendfilename {}",
                    config.redis.aof_path
                );
                info!("   3. Redis will replay the AOF and restore state");
                info!("");
                info!("   Or just run 'tt init' - it will use existing AOF if present.");
            }
        }

        Commands::Config { key, value } => {
            let config_path = GlobalConfig::config_path()?;

            match (key, value) {
                // No args: show all config
                (None, None) => {
                    let config = GlobalConfig::load()?;
                    info!("⚙️  Global config: {}", config_path.display());
                    info!("");
                    info!("default_cli = \"{}\"", config.default_cli);
                    if !config.agent_clis.is_empty() {
                        info!("");
                        info!("[agent_clis]");
                        for (name, cmd) in &config.agent_clis {
                            info!("{} = \"{}\"", name, cmd);
                        }
                    }
                    info!("");
                    info!("Available CLIs: claude, auggie, codex, aider, gemini, copilot, cursor");
                }
                // Key only: show that value
                (Some(key), None) => {
                    let config = GlobalConfig::load()?;
                    if let Some(val) = config.get(&key) {
                        println!("{}", val);
                    } else {
                        info!("❌ Unknown config key: {}", key);
                        info!("   Available keys: default_cli, agent_clis.<name>");
                    }
                }
                // Key and value: set it
                (Some(key), Some(value)) => {
                    let mut config = GlobalConfig::load()?;
                    config.set(&key, &value)?;
                    config.save()?;
                    info!("✅ Set {} = \"{}\"", key, value);
                    info!("   Saved to: {}", config_path.display());
                }
                // Value without key (shouldn't happen due to clap)
                (None, Some(_)) => {
                    info!("❌ Please specify a key");
                }
            }
        }

        Commands::Recover => {
            use tinytown::AgentState;

            let town = Town::connect(&cli.town).await?;
            let agents = town.list_agents().await;

            let mut recovered = 0;
            let mut checked = 0;

            info!("🔍 Scanning for orphaned agents...");

            for agent in agents {
                checked += 1;

                // Check if agent is in a "working" or "starting" state (should be active)
                let is_active_state =
                    matches!(agent.state, AgentState::Working | AgentState::Starting);

                if !is_active_state {
                    continue;
                }

                // Check if the agent's process is still running by looking for its log file
                // and checking if it was recently modified (within last 2 minutes)
                let log_file = cli.town.join(format!(".tt/logs/{}.log", agent.name));
                let is_stale = if log_file.exists() {
                    if let Ok(metadata) = std::fs::metadata(&log_file) {
                        if let Ok(modified) = metadata.modified() {
                            let elapsed = std::time::SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or_default();
                            // If log hasn't been modified in 2 minutes, consider stale
                            elapsed.as_secs() > 120
                        } else {
                            // Can't get modified time, assume stale if old heartbeat
                            let heartbeat_age = chrono::Utc::now() - agent.last_heartbeat;
                            heartbeat_age.num_seconds() > 120
                        }
                    } else {
                        true
                    }
                } else {
                    // No log file and agent claims to be working - likely orphaned
                    let heartbeat_age = chrono::Utc::now() - agent.last_heartbeat;
                    heartbeat_age.num_seconds() > 120
                };

                if is_stale {
                    // Update agent state to stopped
                    if let Some(mut agent_state) = town.channel().get_agent_state(agent.id).await? {
                        agent_state.state = AgentState::Stopped;
                        town.channel().set_agent_state(&agent_state).await?;
                    }

                    // Log activity
                    town.channel()
                        .log_agent_activity(agent.id, "🔄 Recovered by tt recover (orphaned)")
                        .await?;

                    info!(
                        "   🔄 Recovered '{}' ({:?}) - last heartbeat {:?} ago",
                        agent.name,
                        agent.state,
                        chrono::Utc::now() - agent.last_heartbeat
                    );
                    recovered += 1;
                }
            }

            info!("");
            if recovered == 0 {
                info!("✨ No orphaned agents found ({} agents checked)", checked);
            } else {
                info!(
                    "✨ Recovered {} orphaned agent(s) ({} total checked)",
                    recovered, checked
                );
                info!("   Run 'tt prune' to remove them from Redis");
            }
        }

        Commands::Towns => {
            use tinytown::global_config::GLOBAL_CONFIG_DIR;

            let towns_path = dirs::home_dir()
                .map(|h| h.join(GLOBAL_CONFIG_DIR).join("towns.toml"))
                .ok_or_else(|| {
                    tinytown::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Could not find home directory",
                    ))
                })?;

            if !towns_path.exists() {
                info!("📍 No towns registered yet.");
                info!("   Run 'tt init' in a directory to register a town.");
                return Ok(());
            }

            // Parse towns.toml
            let content = std::fs::read_to_string(&towns_path)?;
            let towns_file: TownsFile = toml::from_str(&content).map_err(|e| {
                tinytown::Error::Io(std::io::Error::other(format!("Invalid towns.toml: {}", e)))
            })?;

            info!("🏘️  Registered Towns ({}):", towns_file.towns.len());
            info!("");

            for town_entry in &towns_file.towns {
                let path = std::path::Path::new(&town_entry.path);

                // Try to connect to the town's Redis
                let status = if path.exists() {
                    // Check if tinytown.toml exists
                    let config_file = path.join("tinytown.toml");
                    if !config_file.exists() {
                        "⚠️  (no config)".to_string()
                    } else {
                        // Try to connect
                        match Town::connect(path).await {
                            Ok(t) => {
                                let agents = t.list_agents().await;
                                let active = agents
                                    .iter()
                                    .filter(|a| {
                                        matches!(
                                            a.state,
                                            tinytown::AgentState::Working
                                                | tinytown::AgentState::Starting
                                                | tinytown::AgentState::Idle
                                        )
                                    })
                                    .count();
                                format!("[OK] {} agents ({} active)", agents.len(), active)
                            }
                            Err(_) => "[OFFLINE]".to_string(),
                        }
                    }
                } else {
                    "❌ (path not found)".to_string()
                };

                info!("   {} - {}", town_entry.name, status);
                info!("      📂 {}", town_entry.path);
            }
        }

        Commands::Backlog { action } => {
            let town = Town::connect(&cli.town).await?;

            match action {
                BacklogAction::Add { description, tags } => {
                    let mut task = Task::new(&description);
                    if let Some(tag_str) = tags {
                        let tag_list: Vec<String> =
                            tag_str.split(',').map(|s| s.trim().to_string()).collect();
                        task = task.with_tags(tag_list);
                    }

                    // Store task and add to backlog
                    town.channel().set_task(&task).await?;
                    town.channel().backlog_push(task.id).await?;

                    info!("📋 Added task to backlog: {}", task.id);
                    info!("   Description: {}", description);
                }

                BacklogAction::List => {
                    let task_ids = town.channel().backlog_list().await?;

                    if task_ids.is_empty() {
                        info!("📋 Backlog is empty");
                    } else {
                        info!("📋 Backlog ({} tasks):", task_ids.len());
                        info!("");
                        for task_id in task_ids {
                            if let Ok(Some(task)) = town.channel().get_task(task_id).await {
                                let tags = if task.tags.is_empty() {
                                    String::new()
                                } else {
                                    format!(" [{}]", task.tags.join(", "))
                                };
                                info!(
                                    "   {} - {}{}",
                                    task_id,
                                    task.description.chars().take(60).collect::<String>(),
                                    tags
                                );
                            } else {
                                info!("   {} - (task not found)", task_id);
                            }
                        }
                    }
                }

                BacklogAction::Claim { task_id, agent } => {
                    // Parse task ID
                    let tid: tinytown::TaskId = task_id.parse().map_err(|e| {
                        tinytown::Error::TaskNotFound(format!("Invalid task ID: {}", e))
                    })?;

                    // Check task exists in backlog
                    let removed = town.channel().backlog_remove(tid).await?;
                    if !removed {
                        info!("❌ Task {} not found in backlog", task_id);
                        return Ok(());
                    }

                    // Get agent
                    let agent_handle = town.agent(&agent).await?;

                    // Assign the task (consistent with tt assign - agent will start() when working)
                    if let Some(mut task) = town.channel().get_task(tid).await? {
                        task.assign(agent_handle.id());
                        town.channel().set_task(&task).await?;

                        // Send assignment message
                        use tinytown::agent::AgentId;
                        use tinytown::message::{Message, MessageType};
                        let msg = Message::new(
                            AgentId::supervisor(),
                            agent_handle.id(),
                            MessageType::TaskAssign {
                                task_id: tid.to_string(),
                            },
                        );
                        town.channel().send(&msg).await?;

                        info!("✅ Claimed task {} and assigned to '{}'", task_id, agent);
                    } else {
                        info!("❌ Task {} not found", task_id);
                    }
                }

                BacklogAction::AssignAll { agent } => {
                    let agent_handle = town.agent(&agent).await?;
                    let mut count = 0;

                    while let Some(tid) = town.channel().backlog_pop().await? {
                        if let Some(mut task) = town.channel().get_task(tid).await? {
                            // Consistent with tt assign - agent will call start() when working
                            task.assign(agent_handle.id());
                            town.channel().set_task(&task).await?;

                            use tinytown::agent::AgentId;
                            use tinytown::message::{Message, MessageType};
                            let msg = Message::new(
                                AgentId::supervisor(),
                                agent_handle.id(),
                                MessageType::TaskAssign {
                                    task_id: tid.to_string(),
                                },
                            );
                            town.channel().send(&msg).await?;
                            count += 1;
                        }
                    }

                    if count == 0 {
                        info!("📋 Backlog is empty, no tasks to assign");
                    } else {
                        info!("✅ Assigned {} task(s) from backlog to '{}'", count, agent);
                    }
                }

                BacklogAction::Remove { task_id } => {
                    // Parse task ID
                    let tid: tinytown::TaskId = task_id.parse().map_err(|e| {
                        tinytown::Error::TaskNotFound(format!("Invalid task ID: {}", e))
                    })?;

                    // Remove from backlog
                    let removed = tinytown::BacklogService::remove(town.channel(), tid).await?;
                    if removed {
                        info!("✅ Removed task {} from backlog", task_id);
                    } else {
                        info!("❌ Task {} not found in backlog", task_id);
                    }
                }
            }
        }

        Commands::Reclaim {
            to_backlog,
            to,
            from,
        } => {
            let town = Town::connect(&cli.town).await?;
            let agents = town.list_agents().await;

            // Find dead agents (stopped or error state)
            let dead_agents: Vec<_> = agents
                .iter()
                .filter(|a| a.state.is_terminal())
                .filter(|a| from.as_ref().is_none_or(|f| &a.name == f))
                .collect();

            if dead_agents.is_empty() {
                if let Some(f) = &from {
                    info!("❌ Agent '{}' not found or not in terminal state", f);
                } else {
                    info!("✨ No dead agents found with tasks to reclaim");
                }
                return Ok(());
            }

            let mut total_reclaimed = 0;

            // Get target agent if specified
            let target_agent = if let Some(target_name) = &to {
                Some(town.agent(target_name).await?)
            } else {
                None
            };

            info!("🔄 Reclaiming orphaned tasks...");

            for agent in dead_agents {
                let messages = town.channel().drain_inbox(agent.id).await?;

                if messages.is_empty() {
                    continue;
                }

                info!(
                    "   {} ({:?}): {} message(s)",
                    agent.name,
                    agent.state,
                    messages.len()
                );

                for msg in messages {
                    // Check if it's a task assignment message
                    if let tinytown::message::MessageType::TaskAssign { task_id } = &msg.msg_type {
                        if let Ok(tid) = task_id.parse::<tinytown::TaskId>() {
                            if to_backlog {
                                // Move to backlog
                                town.channel().backlog_push(tid).await?;
                                info!("      → backlog: {}", task_id);
                            } else if let Some(ref target) = target_agent {
                                // Move to target agent
                                town.channel()
                                    .move_message_to_inbox(&msg, target.id())
                                    .await?;
                                info!("      → {}: {}", to.as_ref().unwrap(), task_id);
                            } else {
                                // Just list what we found (no destination specified)
                                info!("      task: {}", task_id);
                            }
                            total_reclaimed += 1;
                        }
                    } else if let tinytown::message::MessageType::Task { description } =
                        &msg.msg_type
                    {
                        if to_backlog {
                            let task = tinytown::Task::new(description.clone());
                            let task_id = task.id;
                            town.channel().set_task(&task).await?;
                            town.channel().backlog_push(task_id).await?;
                            info!("      → backlog: {}", task_id);
                        } else if let Some(ref target) = target_agent {
                            town.channel()
                                .move_message_to_inbox(&msg, target.id())
                                .await?;
                            info!(
                                "      → {}: {}",
                                to.as_ref().unwrap(),
                                truncate_summary(description, 60)
                            );
                        } else {
                            info!("      task: {}", truncate_summary(description, 60));
                        }
                        total_reclaimed += 1;
                    } else {
                        // Non-task message - move to target or discard
                        if let Some(ref target) = target_agent {
                            town.channel()
                                .move_message_to_inbox(&msg, target.id())
                                .await?;
                        }
                    }
                }
            }

            info!("");
            if total_reclaimed == 0 {
                info!("📋 No tasks found in dead agent inboxes");
            } else if to_backlog {
                info!("✅ Moved {} task(s) to backlog", total_reclaimed);
            } else if let Some(target_name) = &to {
                info!("✅ Moved {} task(s) to '{}'", total_reclaimed, target_name);
            } else {
                info!("📋 Found {} orphaned task(s)", total_reclaimed);
                info!("   Use --to-backlog or --to <agent> to reclaim them");
            }
        }

        Commands::Restart {
            agent,
            rounds,
            foreground,
        } => {
            use tinytown::AgentState;

            let town = Town::connect(&cli.town).await?;

            // Get the agent
            let Some(mut agent_state) = town.channel().get_agent_by_name(&agent).await? else {
                info!("❌ Agent '{}' not found", agent);
                return Ok(());
            };

            // Check if agent is in terminal state
            if !agent_state.state.is_terminal() {
                info!(
                    "❌ Agent '{}' is still active ({:?})",
                    agent, agent_state.state
                );
                info!("   Use 'tt kill {}' to stop it first", agent);
                return Ok(());
            }

            // Reset agent state
            agent_state.state = AgentState::Idle;
            agent_state.rounds_completed = 0;
            agent_state.last_heartbeat = chrono::Utc::now();
            town.channel().set_agent_state(&agent_state).await?;

            // Clear any stop flags
            town.channel().clear_stop(agent_state.id).await?;

            // Log activity
            town.channel()
                .log_agent_activity(
                    agent_state.id,
                    &format!("🔄 Restarted with {} rounds", rounds),
                )
                .await?;

            info!("🔄 Restarting agent '{}'...", agent);
            info!("   Rounds: {}", rounds);

            // Spawn the agent loop process
            let logs_dir = cli.town.join(".tt/logs");
            std::fs::create_dir_all(&logs_dir)?;

            // Clean up old round log files to prevent stale data in 'tt status --deep'
            let cleaned = clean_agent_round_logs(&logs_dir, &agent);
            if cleaned > 0 {
                info!("   Cleaned {} old round log file(s)", cleaned);
            }

            let log_file = logs_dir.join(format!("{}.log", agent));

            let agent_loop_cmd = format!(
                "tt -t '{}' agent-loop '{}' '{}' {}",
                cli.town.display(),
                agent,
                agent_state.id,
                rounds
            );

            if foreground {
                // Run in foreground
                std::process::Command::new("sh")
                    .args(["-c", &agent_loop_cmd])
                    .status()?;
            } else {
                // Run in background with logging
                let full_cmd = format!(
                    "nohup {} >> '{}' 2>&1 &",
                    agent_loop_cmd,
                    log_file.display()
                );
                std::process::Command::new("sh")
                    .args(["-c", &full_cmd])
                    .spawn()?;

                info!("   Log: {}", log_file.display());
                info!("");
                info!("✅ Agent '{}' restarted", agent);
            }
        }

        Commands::Auth { action } => match action {
            AuthAction::GenKey => {
                use tinytown::generate_api_key;

                let (raw_key, hash) = generate_api_key();

                info!("🔐 Generated new API key");
                info!("");
                info!("API Key (store securely, shown only once):");
                println!("{}", raw_key);
                info!("");
                info!("API Key Hash (add to tinytown.toml):");
                println!("{}", hash);
                info!("");
                info!("Add to your tinytown.toml:");
                info!("");
                info!("  [townhall.auth]");
                info!("  mode = \"api_key\"");
                info!("  api_key_hash = \"{}\"", hash);
                info!("");
                info!("Then use the API key with townhall:");
                info!(
                    "  curl -H 'Authorization: Bearer {}' http://localhost:8080/v1/status",
                    &raw_key[..8]
                );
            }
        },

        Commands::Migrate {
            dry_run,
            force,
            hash,
        } => {
            use tinytown::{
                migrate_json_to_hash, migrate_to_town_isolation, needs_hash_migration,
                needs_migration, preview_hash_migration, preview_migration,
            };

            let town = Town::connect(&cli.town).await?;
            let config = town.config();
            let town_name = &config.name;

            // Get a connection for migration
            let redis_url = config.redis_url();
            let client = redis::Client::open(redis_url)?;
            let mut conn = redis::aio::ConnectionManager::new(client).await?;

            if hash {
                // JSON-to-Hash migration
                let needs_mig = needs_hash_migration(&mut conn, town_name).await?;
                if !needs_mig {
                    info!(
                        "✅ No JSON-to-Hash migration needed - all keys already use Hash storage"
                    );
                    info!("   Town: {}", town_name);
                    return Ok(());
                }

                if dry_run {
                    info!("🔍 JSON-to-Hash Migration Preview (dry run)");
                    info!("   Town: {}", town_name);
                    info!("");

                    let preview = preview_hash_migration(&mut conn, town_name).await?;
                    if preview.is_empty() {
                        info!("   No JSON string keys found.");
                    } else {
                        info!("   Keys to convert to Hash:");
                        for key in &preview {
                            info!("   {} (string → hash)", key);
                        }
                        info!("");
                        info!("   Total: {} key(s) would be migrated", preview.len());
                        info!("");
                        info!(
                            "   Run 'tt migrate --hash' (without --dry-run) to perform migration."
                        );
                    }
                } else {
                    if !force {
                        info!("⚠️  JSON-to-Hash Migration Warning");
                        info!("");
                        info!("   This will convert JSON string storage to Redis Hash format.");
                        info!(
                            "   Benefits: atomic field updates, memory efficiency, partial reads."
                        );
                        info!("");
                        info!("   This operation cannot be undone.");
                        info!("");
                        info!("   Run with --force to skip this prompt, or --dry-run to preview.");
                        info!("");

                        eprint!("   Continue? [y/N]: ");
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if !input.trim().eq_ignore_ascii_case("y") {
                            info!("   Migration cancelled.");
                            return Ok(());
                        }
                    }

                    info!("🔄 Migrating JSON strings to Redis Hashes...");
                    info!("   Town: {}", town_name);

                    let stats = migrate_json_to_hash(&mut conn, town_name).await?;

                    info!("");
                    info!("✅ JSON-to-Hash migration complete!");
                    info!("   Agents migrated: {}", stats.agents_migrated);
                    info!("   Tasks migrated:  {}", stats.tasks_migrated);
                    info!("   Already hash:    {}", stats.already_hash);

                    if !stats.errors.is_empty() {
                        warn!("");
                        warn!("   ⚠️  {} key(s) failed to migrate:", stats.errors.len());
                        for key in &stats.errors {
                            warn!("      - {}", key);
                        }
                    }
                }
            } else {
                // Town isolation migration (existing behavior)
                let needs_mig = needs_migration(&mut conn).await?;
                if !needs_mig {
                    info!("✅ No migration needed - all keys already use town isolation format");
                    info!("   Town: {}", town_name);
                    return Ok(());
                }

                if dry_run {
                    info!("🔍 Migration Preview (dry run)");
                    info!("   Town: {}", town_name);
                    info!("");

                    let preview = preview_migration(&mut conn).await?;
                    if preview.is_empty() {
                        info!("   No old-format keys found.");
                    } else {
                        info!("   Keys to migrate:");
                        for (old_key, new_pattern) in &preview {
                            let new_key = new_pattern.replace("<town>", town_name);
                            info!("   {} → {}", old_key, new_key);
                        }
                        info!("");
                        info!("   Total: {} key(s) would be migrated", preview.len());
                        info!("");
                        info!("   Run 'tt migrate' (without --dry-run) to perform migration.");
                    }
                } else {
                    if !force {
                        info!("⚠️  Migration Warning");
                        info!("");
                        info!(
                            "   This will migrate old Redis keys to the new town-isolated format:"
                        );
                        info!("   tt:type:id → tt:{}:type:id", town_name);
                        info!("");
                        info!("   This operation cannot be undone.");
                        info!("");
                        info!("   Run with --force to skip this prompt, or --dry-run to preview.");
                        info!("");

                        eprint!("   Continue? [y/N]: ");
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if !input.trim().eq_ignore_ascii_case("y") {
                            info!("   Migration cancelled.");
                            return Ok(());
                        }
                    }

                    info!("🔄 Migrating to town isolation...");
                    info!("   Town: {}", town_name);

                    let stats = migrate_to_town_isolation(&mut conn, town_name).await?;

                    info!("");
                    info!("✅ Migration complete!");
                    info!("   Agents migrated:  {}", stats.agents_migrated);
                    info!("   Inboxes migrated: {}", stats.inboxes_migrated);
                    info!("   Tasks migrated:   {}", stats.tasks_migrated);
                    info!(
                        "   Other keys:       {}",
                        stats.urgent_migrated
                            + stats.activity_migrated
                            + stats.stop_migrated
                            + stats.backlog_migrated
                    );

                    if !stats.errors.is_empty() {
                        warn!("");
                        warn!("   ⚠️  {} key(s) failed to migrate:", stats.errors.len());
                        for key in &stats.errors {
                            warn!("      - {}", key);
                        }
                    }
                }
            }
        }

        Commands::Mission { action } => {
            use tinytown::mission::{
                MissionId, MissionPolicy, MissionRun, MissionScheduler, MissionState,
                MissionStorage, ObjectiveRef,
            };

            let town = Town::connect(&cli.town).await?;
            let config = town.config();
            let storage = MissionStorage::new(town.channel().conn().clone(), &config.name);

            match action {
                MissionAction::Start {
                    issues,
                    docs,
                    max_parallel,
                    no_reviewer,
                } => {
                    if issues.is_empty() && docs.is_empty() {
                        info!("❌ At least one --issue or --doc is required");
                        return Ok(());
                    }

                    // Parse objectives
                    let mut objectives = Vec::new();

                    for issue in &issues {
                        if let Some(obj) = parse_issue_ref(issue, &config.name, town.root()) {
                            objectives.push(obj);
                        } else {
                            warn!("⚠️  Could not parse issue: {}", issue);
                        }
                    }

                    for doc in &docs {
                        objectives.push(ObjectiveRef::Doc { path: doc.clone() });
                    }

                    if objectives.is_empty() {
                        info!("❌ No valid objectives found");
                        return Ok(());
                    }

                    // Create mission with policy
                    let policy = MissionPolicy {
                        max_parallel_items: max_parallel,
                        reviewer_required: !no_reviewer,
                        ..Default::default()
                    };

                    let mut mission = MissionRun::new(objectives.clone()).with_policy(policy);
                    mission.start();

                    // Save to Redis
                    storage.save_mission(&mission).await?;
                    storage.add_active(mission.id).await?;
                    storage
                        .log_event(mission.id, "Mission started via CLI")
                        .await?;

                    let work_items =
                        build_mission_work_items(town.root(), mission.id, &objectives)?;
                    let work_item_count = work_items.len();
                    for item in &work_items {
                        storage.save_work_item(item).await?;
                    }
                    storage
                        .log_event(
                            mission.id,
                            &format!(
                                "Bootstrapped {} work item(s) from mission objectives",
                                work_item_count
                            ),
                        )
                        .await?;

                    let scheduler =
                        MissionScheduler::with_defaults(storage.clone(), town.channel().clone());
                    let tick_result = scheduler.tick().await?;

                    info!("🚀 Mission started!");
                    info!("   ID: {}", mission.id);
                    info!("   Objectives: {}", objectives.len());
                    info!("   Work items: {}", work_item_count);
                    for obj in &objectives {
                        info!("      - {}", obj);
                    }
                    info!("   Max parallel: {}", max_parallel);
                    info!("   Reviewer required: {}", !no_reviewer);
                    info!(
                        "   Scheduler bootstrap: {} promoted, {} assigned",
                        tick_result.total_promoted, tick_result.total_assigned
                    );
                    info!("");
                    info!(
                        "   Check status with: tt mission status --run {}",
                        mission.id
                    );
                }

                MissionAction::Status { run, work, watch } => {
                    if let Some(run_id) = run {
                        // Show specific mission
                        let mission_id: MissionId = run_id
                            .parse()
                            .map_err(|_| tinytown::Error::Config("Invalid mission ID".into()))?;

                        let Some(mission) = storage.get_mission(mission_id).await? else {
                            info!("❌ Mission {} not found", run_id);
                            return Ok(());
                        };

                        print_mission_status(&storage, &mission, work, watch).await?;
                    } else {
                        // Show all active missions
                        let active_ids = storage.list_active().await?;

                        if active_ids.is_empty() {
                            info!("📋 No active missions");
                            info!("   Start one with: tt mission start --issue <N>");
                            return Ok(());
                        }

                        info!("📋 Active Missions: {}", active_ids.len());
                        info!("");

                        for mission_id in active_ids {
                            if let Some(mission) = storage.get_mission(mission_id).await? {
                                print_mission_summary(&mission);
                            }
                        }
                    }
                }

                MissionAction::Resume { run_id } => {
                    let mission_id: MissionId = run_id
                        .parse()
                        .map_err(|_| tinytown::Error::Config("Invalid mission ID".into()))?;

                    let Some(mut mission) = storage.get_mission(mission_id).await? else {
                        info!("❌ Mission {} not found", run_id);
                        return Ok(());
                    };

                    if mission.state == MissionState::Running {
                        info!("ℹ️  Mission {} is already running", run_id);
                        return Ok(());
                    }

                    if mission.state == MissionState::Completed {
                        info!("ℹ️  Mission {} is already completed", run_id);
                        return Ok(());
                    }

                    mission.start();
                    storage.save_mission(&mission).await?;
                    storage.add_active(mission_id).await?;
                    storage
                        .log_event(mission_id, "Mission resumed via CLI")
                        .await?;

                    info!("▶️  Mission {} resumed", run_id);
                }

                MissionAction::Stop { run_id, force } => {
                    let mission_id: MissionId = run_id
                        .parse()
                        .map_err(|_| tinytown::Error::Config("Invalid mission ID".into()))?;

                    let Some(mut mission) = storage.get_mission(mission_id).await? else {
                        info!("❌ Mission {} not found", run_id);
                        return Ok(());
                    };

                    if force {
                        mission.fail("Stopped by user (forced)");
                    } else {
                        mission.block("Stopped by user");
                    }

                    storage.save_mission(&mission).await?;
                    storage.remove_active(mission_id).await?;
                    storage
                        .log_event(mission_id, &format!("Mission stopped (force={})", force))
                        .await?;

                    info!("⏹️  Mission {} stopped", run_id);
                }

                MissionAction::List { all } => {
                    let missions = if all {
                        storage.list_all_missions().await?
                    } else {
                        let active_ids = storage.list_active().await?;
                        let mut missions = Vec::new();
                        for id in active_ids {
                            if let Some(m) = storage.get_mission(id).await? {
                                missions.push(m);
                            }
                        }
                        missions
                    };

                    if missions.is_empty() {
                        info!("📋 No missions found");
                        return Ok(());
                    }

                    info!("📋 Missions: {}", missions.len());
                    info!("");

                    for mission in missions {
                        print_mission_summary(&mission);
                    }
                }
            }
        }
    }

    Ok(())
}

// ==================== Mission Helper Functions ====================

/// Derive GitHub owner and repo from git remote URL.
///
/// Parses remote URLs in formats:
/// - `git@github.com:owner/repo.git`
/// - `https://github.com/owner/repo.git`
/// - `https://github.com/owner/repo`
fn derive_git_remote_info(town_path: &std::path::Path) -> Option<(String, String)> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(town_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();

    // Parse SSH format: git@github.com:owner/repo.git
    if url.starts_with("git@github.com:") {
        let path = url
            .strip_prefix("git@github.com:")?
            .trim_end_matches(".git");
        let (owner, repo) = path.split_once('/')?;
        return Some((owner.to_string(), repo.to_string()));
    }

    // Parse HTTPS format: https://github.com/owner/repo.git
    if url.contains("github.com/") {
        let path = url.split("github.com/").nth(1)?.trim_end_matches(".git");
        let (owner, repo) = path.split_once('/')?;
        return Some((owner.to_string(), repo.to_string()));
    }

    None
}

/// Parse an issue reference (e.g., "23", "owner/repo#23", or URL).
fn parse_issue_ref(
    input: &str,
    default_repo: &str,
    town_path: &std::path::Path,
) -> Option<tinytown::mission::ObjectiveRef> {
    use tinytown::mission::ObjectiveRef;

    // Try as plain number first
    if let Ok(number) = input.parse::<u64>() {
        // Try to derive owner/repo from git remote
        if let Some((owner, repo)) = derive_git_remote_info(town_path) {
            return Some(ObjectiveRef::Issue {
                owner,
                repo,
                number,
            });
        }

        // Fall back to deriving repo from town name (format: repo-branch)
        let repo_part = default_repo.split('-').next().unwrap_or("tinytown");
        warn!(
            "Could not derive GitHub owner from git remote; using repo '{}' with unknown owner",
            repo_part
        );
        return None;
    }

    // Try as owner/repo#number
    if let Some((repo_part, num_part)) = input.split_once('#')
        && let Ok(number) = num_part.parse::<u64>()
        && let Some((owner, repo)) = repo_part.split_once('/')
    {
        return Some(ObjectiveRef::Issue {
            owner: owner.into(),
            repo: repo.into(),
            number,
        });
    }

    // Try as GitHub URL
    if input.contains("github.com") && input.contains("/issues/") {
        let parts: Vec<&str> = input.split('/').collect();
        if parts.len() >= 4 {
            let owner = parts[parts.len() - 4].to_string();
            let repo = parts[parts.len() - 3].to_string();
            if let Ok(number) = parts[parts.len() - 1].parse::<u64>() {
                return Some(ObjectiveRef::Issue {
                    owner,
                    repo,
                    number,
                });
            }
        }
    }

    None
}

#[derive(serde::Deserialize)]
struct GitHubIssueView {
    title: String,
    body: Option<String>,
}

fn fetch_issue_view(
    town_path: &std::path::Path,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Option<GitHubIssueView>> {
    let output = std::process::Command::new("gh")
        .args([
            "issue",
            "view",
            &number.to_string(),
            "--repo",
            &format!("{owner}/{repo}"),
            "--json",
            "title,body",
        ])
        .current_dir(town_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let issue = serde_json::from_slice::<GitHubIssueView>(&output.stdout)?;
            Ok(Some(issue))
        }
        Ok(output) => {
            warn!(
                "⚠️  Could not fetch issue {}/{}#{} via gh: {}",
                owner,
                repo,
                number,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(None)
        }
        Err(err) => {
            warn!(
                "⚠️  Could not run gh for issue {}/{}#{}: {}",
                owner, repo, number, err
            );
            Ok(None)
        }
    }
}

fn build_mission_work_items(
    town_path: &std::path::Path,
    mission_id: tinytown::mission::MissionId,
    objectives: &[tinytown::mission::ObjectiveRef],
) -> Result<Vec<tinytown::mission::WorkItem>> {
    use tinytown::mission::{ObjectiveRef, ParsedIssue, WorkGraphCompiler, WorkItem, WorkKind};

    let compiler = WorkGraphCompiler::new();
    let mut parsed_issues: Vec<ParsedIssue> = Vec::new();
    let mut doc_items = Vec::new();

    for objective in objectives {
        match objective {
            ObjectiveRef::Issue {
                owner,
                repo,
                number,
            } => {
                let issue = fetch_issue_view(town_path, owner, repo, *number)?;
                let title = issue
                    .as_ref()
                    .map(|data| data.title.clone())
                    .unwrap_or_else(|| format!("Issue #{}", number));
                let body = issue.and_then(|data| data.body).unwrap_or_default();
                parsed_issues.push(compiler.parse_issue(
                    *number,
                    title,
                    body,
                    owner.clone(),
                    repo.clone(),
                ));
            }
            ObjectiveRef::Doc { path } => {
                doc_items.push(
                    WorkItem::new(mission_id, path.clone(), WorkKind::Design)
                        .with_source_ref(path.clone()),
                );
            }
        }
    }

    let mut work_items = if parsed_issues.is_empty() {
        Vec::new()
    } else {
        compiler.compile(mission_id, parsed_issues, None)?.items
    };
    work_items.extend(doc_items);
    Ok(work_items)
}

/// Print a summary of a mission.
fn print_mission_summary(mission: &tinytown::mission::MissionRun) {
    let state_emoji = match mission.state {
        tinytown::mission::MissionState::Planning => "📝",
        tinytown::mission::MissionState::Running => "🚀",
        tinytown::mission::MissionState::Blocked => "🚧",
        tinytown::mission::MissionState::Completed => "✅",
        tinytown::mission::MissionState::Failed => "❌",
    };

    let objectives_str: Vec<String> = mission
        .objective_refs
        .iter()
        .map(|o| o.to_string())
        .collect();
    let objectives_short = if objectives_str.len() > 2 {
        format!(
            "{}, {} +{} more",
            objectives_str[0],
            objectives_str[1],
            objectives_str.len() - 2
        )
    } else {
        objectives_str.join(", ")
    };

    let age = chrono::Utc::now() - mission.created_at;
    let age_str = if age.num_hours() > 24 {
        format!("{}d ago", age.num_days())
    } else if age.num_hours() > 0 {
        format!("{}h ago", age.num_hours())
    } else {
        format!("{}m ago", age.num_minutes())
    };

    info!(
        "   {} {} ({:?}) - {} - {}",
        state_emoji,
        mission.id.to_string().chars().take(8).collect::<String>(),
        mission.state,
        objectives_short,
        age_str
    );

    if let Some(reason) = &mission.blocked_reason {
        info!("      └─ Blocked: {}", reason);
    }
}

/// Print detailed mission status.
async fn print_mission_status(
    storage: &tinytown::mission::MissionStorage,
    mission: &tinytown::mission::MissionRun,
    show_work: bool,
    show_watch: bool,
) -> tinytown::Result<()> {
    let state_emoji = match mission.state {
        tinytown::mission::MissionState::Planning => "📝",
        tinytown::mission::MissionState::Running => "🚀",
        tinytown::mission::MissionState::Blocked => "🚧",
        tinytown::mission::MissionState::Completed => "✅",
        tinytown::mission::MissionState::Failed => "❌",
    };

    info!("🎯 Mission Status");
    info!("   ID: {}", mission.id);
    info!("   State: {} {:?}", state_emoji, mission.state);
    info!(
        "   Created: {}",
        mission.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    info!(
        "   Updated: {}",
        mission.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    info!("");

    info!("📋 Objectives: {}", mission.objective_refs.len());
    for obj in &mission.objective_refs {
        info!("   - {}", obj);
    }
    info!("");

    info!("⚙️  Policy:");
    info!("   Max parallel: {}", mission.policy.max_parallel_items);
    info!("   Reviewer required: {}", mission.policy.reviewer_required);
    info!("   Auto-merge: {}", mission.policy.auto_merge);
    info!("   Watch interval: {}s", mission.policy.watch_interval_secs);
    info!("");

    if let Some(reason) = &mission.blocked_reason {
        info!("🚧 Blocked Reason: {}", reason);
        info!("");
    }

    // Work items
    let work_items = storage.list_work_items(mission.id).await?;
    info!("📦 Work Items: {}", work_items.len());

    if show_work || work_items.len() <= 5 {
        for item in &work_items {
            let status_emoji = match item.status {
                tinytown::mission::WorkStatus::Pending => "⏳",
                tinytown::mission::WorkStatus::Ready => "🔵",
                tinytown::mission::WorkStatus::Assigned => "📌",
                tinytown::mission::WorkStatus::Running => "🔄",
                tinytown::mission::WorkStatus::Blocked => "🚧",
                tinytown::mission::WorkStatus::Done => "✅",
            };
            info!(
                "   {} {} ({:?}) - {:?}",
                status_emoji, item.title, item.kind, item.status
            );
            if let Some(agent) = item.assigned_to {
                info!("      └─ Assigned to: {}", agent);
            }
        }
    } else {
        // Count by status
        let pending = work_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WorkStatus::Pending)
            .count();
        let ready = work_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WorkStatus::Ready)
            .count();
        let running = work_items
            .iter()
            .filter(|w| {
                w.status == tinytown::mission::WorkStatus::Running
                    || w.status == tinytown::mission::WorkStatus::Assigned
            })
            .count();
        let done = work_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WorkStatus::Done)
            .count();
        let blocked = work_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WorkStatus::Blocked)
            .count();

        info!("   ⏳ Pending: {}", pending);
        info!("   🔵 Ready: {}", ready);
        info!("   🔄 Running: {}", running);
        info!("   ✅ Done: {}", done);
        info!("   🚧 Blocked: {}", blocked);
        info!("   (use --work for full list)");
    }
    info!("");

    // Watch items
    let watch_items = storage.list_watch_items(mission.id).await?;
    info!("👁️  Watch Items: {}", watch_items.len());

    if show_watch {
        for item in &watch_items {
            let status_emoji = match item.status {
                tinytown::mission::WatchStatus::Active => "🟢",
                tinytown::mission::WatchStatus::Snoozed => "😴",
                tinytown::mission::WatchStatus::Done => "✅",
            };
            info!(
                "   {} {:?} - {} ({:?})",
                status_emoji, item.kind, item.target_ref, item.status
            );
            info!(
                "      └─ Next check: {}",
                item.next_due_at.format("%H:%M:%S")
            );
        }
    } else if !watch_items.is_empty() {
        let active = watch_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WatchStatus::Active)
            .count();
        let done = watch_items
            .iter()
            .filter(|w| w.status == tinytown::mission::WatchStatus::Done)
            .count();
        info!("   🟢 Active: {}", active);
        info!("   ✅ Done: {}", done);
        info!("   (use --watch for full list)");
    }
    info!("");

    // Recent events
    let events = storage.get_events(mission.id, 5).await?;
    if !events.is_empty() {
        info!("📜 Recent Events:");
        for event in events {
            info!("   {}", event);
        }
    }

    Ok(())
}

/// Town registry entry for ~/.tt/towns.toml
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TownEntry {
    path: String,
    name: String,
}

/// Towns file format
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct TownsFile {
    #[serde(default)]
    towns: Vec<TownEntry>,
}

#[cfg(test)]
mod tests {
    use super::{backlog_task_matches_role, is_supervisor_alias, validate_spawn_agent_name};
    use tinytown::Task;

    #[test]
    fn reviewer_does_not_match_implementation_backlog_tags() {
        let task = Task::new("Add demo data mode").with_tags(["backend", "frontend", "data"]);
        assert!(!backlog_task_matches_role(&task, "reviewer"));
    }

    #[test]
    fn reviewer_matches_review_or_security_tags() {
        let review_task = Task::new("Review auth flow").with_tags(["review", "security"]);
        assert!(backlog_task_matches_role(&review_task, "reviewer"));
    }

    #[test]
    fn backend_matches_backend_and_data_tags() {
        let task = Task::new("Implement importer").with_tags(["backend", "data"]);
        assert!(backlog_task_matches_role(&task, "backend"));
    }

    #[test]
    fn generalist_roles_can_match_generic_backlog() {
        let task = Task::new("Pick up the next general task");
        assert!(backlog_task_matches_role(&task, "worker"));
        assert!(backlog_task_matches_role(&task, "agent"));
    }

    #[test]
    fn supervisor_aliases_are_reserved_spawn_names() {
        assert!(is_supervisor_alias("supervisor"));
        assert!(is_supervisor_alias("Conductor"));
        assert!(validate_spawn_agent_name("supervisor").is_err());
        assert!(validate_spawn_agent_name("conductor").is_err());
        assert!(validate_spawn_agent_name("supervisor-2").is_ok());
        assert!(validate_spawn_agent_name("backend").is_ok());
    }
}
