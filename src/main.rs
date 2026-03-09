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
    },

    /// Start the town (Redis server)
    Start,

    /// Stop the town
    Stop,

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

    /// Show pending tasks/messages for all agents
    Tasks,

    /// Check an agent's inbox
    Inbox {
        /// Agent name
        agent: String,
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
}

#[derive(Subcommand)]
enum AuthAction {
    /// Generate a new API key and its hash
    GenKey,
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

fn backlog_role_hint(agent_name: &str) -> &'static str {
    let role = agent_name.to_lowercase();
    if role.contains("front")
        || role.contains("ui")
        || role.contains("web")
        || role.contains("client")
    {
        "Prioritize tasks tagged frontend/ui/web/client."
    } else if role.contains("back") || role.contains("api") || role.contains("server") {
        "Prioritize tasks tagged backend/api/server/database."
    } else if role.contains("test") || role.contains("qa") {
        "Prioritize tasks tagged test/qa/validation/regression."
    } else if role.contains("review") || role.contains("audit") {
        "Prioritize review/quality/security validation tasks."
    } else if role.contains("doc") || role.contains("writer") {
        "Prioritize documentation/spec/readme tasks."
    } else if role.contains("devops")
        || role.contains("ops")
        || role.contains("infra")
        || role.contains("deploy")
    {
        "Prioritize infrastructure/ci/deploy/reliability tasks."
    } else if role.contains("security") || role == "sec" {
        "Prioritize security/vulnerability/hardening tasks."
    } else {
        "Prioritize tasks matching your current specialization and capabilities."
    }
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

                let log_dir = town_path.join(".tt/logs");
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
            use tinytown::{AgentId, Message, MessageType};

            let town = Town::connect(&cli.town).await?;
            let handle = town.agent(&agent).await?;

            // Keep creating a persisted Task record for tracking/backlog operations.
            let mut task_record = Task::new(&task);
            task_record.assign(handle.id());
            let task_id = task_record.id;
            town.channel().set_task(&task_record).await?;

            // Send semantic task message for agent processing.
            let msg = Message::new(
                AgentId::supervisor(),
                handle.id(),
                MessageType::Task { description: task },
            );
            town.channel().send(&msg).await?;

            info!("📋 Assigned task {} to agent '{}'", task_id, agent);
        }

        Commands::Status { deep } => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();

            info!("🏘️  Town: {}", config.name);
            info!("📂 Root: {}", town.root().display());
            info!("📡 Redis: {}", config.redis_url_redacted());

            let agents = town.list_agents().await;
            info!("🤖 Agents: {}", agents.len());

            for agent in agents {
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

                if deep {
                    info!(
                        "   {} ({:?}) - {} pending, {} rounds, uptime {}",
                        agent.name, agent.state, inbox_len, agent.rounds_completed, uptime_str
                    );
                    info!(
                        "      └─ 🔴 {} task  🟡 {} query  🟢 {} info  ⚪ {} confirmations{}",
                        breakdown.tasks + breakdown.other_actionable,
                        breakdown.queries,
                        breakdown.informational,
                        breakdown.confirmations,
                        sampled_note
                    );
                    // Get recent activity from Redis
                    if let Ok(Some(activity)) = town.channel().get_agent_activity(agent.id).await {
                        for line in activity.lines().take(5) {
                            info!("      └─ {}", line);
                        }
                    }
                } else {
                    info!(
                        "   {} ({:?}) - {} pending (🔴 {} 🟡 {} 🟢 {} ⚪ {})",
                        agent.name,
                        agent.state,
                        inbox_len,
                        breakdown.tasks + breakdown.other_actionable,
                        breakdown.queries,
                        breakdown.informational,
                        breakdown.confirmations
                    );
                }
            }

            if deep {
                info!("");
                info!("📊 Stats: rounds completed, uptime since spawn");

                // Show recent logs from each agent
                info!("");
                info!("📜 Recent Agent Logs (last 50 lines each):");
                let log_dir = cli.town.join(".tt/logs");
                if log_dir.exists() {
                    let mut shown_logs = std::collections::HashSet::new();
                    for agent in town.list_agents().await {
                        let log_file = log_dir.join(format!("{}.log", agent.name));
                        if log_file.exists() && !shown_logs.contains(&agent.name) {
                            shown_logs.insert(agent.name.clone());
                            info!("");
                            info!("--- {} ({}) ---", agent.name, log_file.display());
                            if let Ok(content) = std::fs::read_to_string(&log_file) {
                                let lines: Vec<&str> = content.lines().collect();
                                let start = lines.len().saturating_sub(50);
                                for line in &lines[start..] {
                                    info!("  {}", line);
                                }
                            }
                        }
                    }
                }
            }
        }

        Commands::Kill { agent } => {
            use tinytown::AgentState;

            let town = Town::connect(&cli.town).await?;
            let handle = town.agent(&agent).await?;
            let agent_id = handle.id();

            // Request the agent to stop
            town.channel().request_stop(agent_id).await?;

            // Update state to show it's stopping
            if let Some(mut agent_state) = town.channel().get_agent_state(agent_id).await? {
                agent_state.state = AgentState::Stopped;
                town.channel().set_agent_state(&agent_state).await?;
            }

            // Log activity
            town.channel()
                .log_agent_activity(agent_id, "🛑 Stop requested by user")
                .await?;

            info!("🛑 Requested stop for agent '{}'", agent);
            info!("   Agent will stop at the start of its next round.");
        }

        Commands::Prune { all } => {
            use tinytown::AgentState;

            let town = Town::connect(&cli.town).await?;
            let agents = town.list_agents().await;

            let mut removed = 0;
            for agent in agents {
                let should_remove =
                    all || matches!(agent.state, AgentState::Stopped | AgentState::Error);
                if should_remove {
                    town.channel().delete_agent(agent.id).await?;
                    info!(
                        "🗑️  Removed {} ({}) - {:?}",
                        agent.name, agent.id, agent.state
                    );
                    removed += 1;
                }
            }

            if removed == 0 {
                info!("No agents to prune.");
            } else {
                info!("✨ Pruned {} agent(s)", removed);
            }
        }

        Commands::Tasks => {
            use tinytown::TaskId;

            let town = Town::connect(&cli.town).await?;
            let agents = town.list_agents().await;

            if agents.is_empty() {
                info!("No agents. Run 'tt spawn <name>' to create one.");
            } else {
                info!("📋 Pending Messages by Agent:");
                info!("");

                let mut total_actionable = 0;
                for agent in &agents {
                    let inbox_len = town.channel().inbox_len(agent.id).await.unwrap_or(0);
                    if inbox_len == 0 {
                        continue;
                    }

                    let peek_count = std::cmp::min(inbox_len, 100) as isize;
                    let messages = town
                        .channel()
                        .peek_inbox(agent.id, peek_count)
                        .await
                        .unwrap_or_default();
                    if messages.is_empty() {
                        continue;
                    }

                    let mut breakdown = MessageBreakdown::default();
                    for msg in &messages {
                        breakdown.count(&msg.msg_type);
                    }

                    info!("  {} ({:?}):", agent.name, agent.state);
                    info!(
                        "    🔴 {} tasks requiring action",
                        breakdown.tasks + breakdown.other_actionable
                    );
                    info!("    🟡 {} queries awaiting response", breakdown.queries);
                    info!("    🟢 {} informational", breakdown.informational);
                    info!("    ⚪ {} confirmations", breakdown.confirmations);

                    let mut shown = 0;
                    for msg in &messages {
                        if !matches!(
                            classify_message(&msg.msg_type),
                            MessageCategory::Task
                                | MessageCategory::Query
                                | MessageCategory::OtherActionable
                        ) {
                            continue;
                        }
                        if shown >= 5 {
                            break;
                        }

                        let summary = match &msg.msg_type {
                            tinytown::MessageType::TaskAssign { task_id } => {
                                if let Ok(tid) = task_id.parse::<TaskId>() {
                                    if let Ok(Some(task)) = town.channel().get_task(tid).await {
                                        task.description
                                    } else {
                                        format!("Task {}", task_id)
                                    }
                                } else {
                                    format!("Task {}", task_id)
                                }
                            }
                            _ => summarize_message(&msg.msg_type),
                        };
                        info!("    • {}", truncate_summary(&summary, 90));
                        shown += 1;
                    }

                    if shown == 0 {
                        info!("    • (no actionable messages in sampled inbox)");
                    }

                    if inbox_len > messages.len() {
                        info!("    …plus {} more message(s)", inbox_len - messages.len());
                    }

                    total_actionable += breakdown.actionable_count();
                    info!("");
                }

                if total_actionable == 0 {
                    info!("  (no actionable messages)");
                } else {
                    info!("Total: {} actionable message(s)", total_actionable);
                }
            }
        }

        Commands::Start => {
            let _town = Town::connect(&cli.town).await?;
            info!("🚀 Town started");
            // Keep running until Ctrl+C
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for ctrl-c");
            info!("👋 Shutting down...");
        }

        Commands::Stop => {
            info!("👋 Town stopped (Redis will be cleaned up)");
        }

        Commands::Inbox { agent } => {
            let town = Town::connect(&cli.town).await?;
            let handle = town.agent(&agent).await?;
            let agent_id = handle.id();

            // Check inbox length
            let inbox_len = town.channel().inbox_len(agent_id).await?;
            info!("📬 Inbox for '{}': {} messages", agent, inbox_len);

            // Try to receive messages (non-blocking peek would be better, but for now show count)
            if inbox_len > 0 {
                info!("   Use the agent loop to process messages");
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

                if regular_messages.is_empty() && urgent_messages.is_empty() {
                    let backlog_count = channel.backlog_len().await?;
                    if backlog_count > 0 {
                        info!(
                            "   📋 Inbox empty, but backlog has {} task(s); prompting backlog review",
                            backlog_count
                        );
                        regular_messages.push(tinytown::Message::new(
                            AgentId::supervisor(),
                            agent_id,
                            tinytown::MessageType::Query {
                                question: format!(
                                    "Backlog has {} task(s). Review `tt backlog list` and claim one that matches your role using `tt backlog claim <task-id> {}`.",
                                    backlog_count, name
                                ),
                            },
                        ));
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
                    let backlog_count = channel.backlog_len().await?;
                    if backlog_count > 0 {
                        info!(
                            "   📋 No direct actionable messages; backlog has {} task(s), prompting claim review",
                            backlog_count
                        );
                        actionable_messages.push((
                            tinytown::Message::new(
                                AgentId::supervisor(),
                                agent_id,
                                tinytown::MessageType::Query {
                                    question: format!(
                                        "No direct assignments right now. Backlog has {} task(s): review and claim one that fits your role with `tt backlog claim <task-id> {}`.",
                                        backlog_count, name
                                    ),
                                },
                            ),
                            false,
                        ));
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
                let actionable_section = {
                    let mut section = String::from("## Actionable Messages (already popped)\n\n");
                    for (idx, (msg, urgent)) in actionable_messages.iter().enumerate() {
                        let summary = truncate_summary(&summarize_message(&msg.msg_type), 120);
                        let priority = if *urgent { "URGENT" } else { "normal" };
                        section.push_str(&format!(
                            "{}. [{}] from {}: {}\n",
                            idx + 1,
                            priority,
                            msg.from,
                            summary
                        ));
                    }
                    section
                };

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

                let backlog_ids = channel.backlog_list().await?;
                let backlog_count = backlog_ids.len();
                let role_hint = backlog_role_hint(&name);
                let backlog_section = {
                    let mut section = format!(
                        "\n## Backlog Snapshot\n\n- Total backlog tasks: {}\n- Role match hint: {}\n",
                        backlog_count, role_hint
                    );
                    if backlog_count > 0 {
                        section.push_str("\nReview and claim role-matching items:\n");
                        let mut shown = 0usize;
                        for task_id in backlog_ids.iter().take(8) {
                            if let Some(task) = channel.get_task(*task_id).await? {
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
                                shown += 1;
                            } else {
                                section.push_str(&format!(
                                    "- {} - (task record not found)\n",
                                    task_id
                                ));
                                shown += 1;
                            }
                        }
                        if backlog_count > shown {
                            section.push_str(&format!(
                                "- ...and {} more backlog task(s)\n",
                                backlog_count - shown
                            ));
                        }
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
4. Delegate or ask questions using semantic message types (`--query`, `--info`, `--ack`).
5. If blocked, send a query with specific unblock needs.
6. When finished, send informational updates or confirmations as appropriate.

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
7. **Coordinate** handoffs between agents
8. **Check with reviewer** to decide when work is complete
9. **Cleanup**: When done, stop agents with `tt kill <agent>`

## The Reviewer Pattern

Always spawn a **reviewer** agent. This agent decides when work is satisfactorily done:

1. Worker completes task → you assign review task to reviewer
2. Reviewer checks the work → reports back (approve/needs changes)
3. You either mark complete or assign fixes to worker

This keeps decisions simple: workers work, reviewer approves, you coordinate.

## Guidelines

- **Always spawn a reviewer** - they're your quality gate
- Be proactive: spawn agents and assign tasks without waiting to be told exactly how
- Be specific: task descriptions should be clear and actionable
- Be efficient: parallelize independent work across multiple agents
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
7. When backend is done: `tt assign reviewer "Review the auth API implementation. Check: security (password hashing, no secrets in logs), error handling, API consistency. Approve or list changes needed."`
8. If reviewer approves → done! If not → assign fixes to backend, repeat.
9. Save state: `tt sync pull` to save tasks to tasks.toml
10. Suggest: "Run `git add tasks.toml && git commit -m 'Update task state'` to persist"

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
                                format!("✅ {} agents ({} active)", agents.len(), active)
                            }
                            Err(_) => "🔴 offline".to_string(),
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

                    // Assign the task
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
