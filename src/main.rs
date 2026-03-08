/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Tinytown CLI - Simple multi-agent orchestration.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use tinytown::{Result, Task, Town, plan};

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

        /// Model to use (uses default_model from config if not specified)
        #[arg(short, long)]
        model: Option<String>,

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
        Commands::Init { name } => {
            let name = name.unwrap_or_else(|| {
                cli.town
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("tinytown")
                    .to_string()
            });

            let town = Town::init(&cli.town, &name).await?;
            info!("✨ Initialized town '{}' at {}", name, cli.town.display());
            info!("📡 Redis running with Unix socket for fast message passing");
            info!("🚀 Run 'tt spawn <name>' to create agents");

            // Keep town alive briefly to show it's working
            drop(town);
        }

        Commands::Spawn {
            name,
            model,
            max_rounds,
            foreground,
        } => {
            let town = Town::connect(&cli.town).await?;
            let model = model.unwrap_or_else(|| town.config().default_model.clone());
            let agent = town.spawn_agent(&name, &model).await?;
            let agent_id = agent.id().to_string();

            info!("🤖 Spawned agent '{}' using model '{}'", name, model);
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
                info!("   Logs: {}/logs/{}.log", town_path.display(), name);

                let log_dir = town_path.join("logs");
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
            let handle = town.agent(&agent).await?;
            let task = Task::new(&task);
            let task_id = handle.assign(task).await?;
            info!("📋 Assigned task {} to agent '{}'", task_id, agent);
        }

        Commands::Status { deep } => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();

            info!("🏘️  Town: {}", config.name);
            info!("📂 Root: {}", town.root().display());
            info!("📡 Redis: {}", config.redis_url());

            let agents = town.list_agents().await;
            info!("🤖 Agents: {}", agents.len());

            for agent in agents {
                let inbox_len = town.channel().inbox_len(agent.id).await.unwrap_or(0);
                info!(
                    "   {} ({:?}) - {} messages pending",
                    agent.name, agent.state, inbox_len
                );

                if deep {
                    // Get recent activity from Redis
                    if let Ok(Some(activity)) = town.channel().get_agent_activity(agent.id).await {
                        for line in activity.lines().take(5) {
                            info!("      └─ {}", line);
                        }
                    }
                }
            }

            if deep {
                info!("");
                info!("📊 Deep status shows last activity from each agent.");
                info!("   Activity is stored in Redis with 1-hour TTL.");
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
            urgent,
        } => {
            use tinytown::{AgentId, Message, MessageType};

            let town = Town::connect(&cli.town).await?;
            let to_handle = town.agent(&to).await?;
            let to_id = to_handle.id();

            // Create a custom message
            let msg = Message::new(
                AgentId::supervisor(), // From conductor/supervisor
                to_id,
                MessageType::Custom {
                    kind: if urgent {
                        "urgent".to_string()
                    } else {
                        "task".to_string()
                    },
                    payload: message.clone(),
                },
            );

            if urgent {
                town.channel().send_urgent(&msg).await?;
                info!("🚨 Sent URGENT message to '{}': {}", to, message);
            } else {
                town.channel().send(&msg).await?;
                info!("📤 Sent message to '{}': {}", to, message);
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

            // Get model command
            let agent_state = channel.get_agent_state(agent_id).await?;
            let model_name = agent_state
                .as_ref()
                .map(|a| a.model.clone())
                .unwrap_or_else(|| config.default_model.clone());
            let model_cmd = config
                .models
                .get(&model_name)
                .map(|m| m.command.clone())
                .unwrap_or_else(|| model_name.clone());

            info!(
                "🔄 Agent '{}' starting loop (max {} rounds)",
                name, max_rounds
            );
            info!("   Model: {} ({})", model_name, model_cmd);

            for round in 1..=max_rounds {
                info!("\n📍 Round {}/{}", round, max_rounds);

                // Check if stop has been requested
                if channel.should_stop(agent_id).await? {
                    info!("   🛑 Stop requested, exiting gracefully...");
                    channel
                        .log_agent_activity(
                            agent_id,
                            &format!("Round {}: 🛑 stopped by request", round),
                        )
                        .await?;
                    channel.clear_stop(agent_id).await?;
                    break;
                }

                // Check URGENT inbox first (priority messages)
                let urgent_messages = channel.receive_urgent(agent_id).await?;
                if !urgent_messages.is_empty() {
                    info!("   🚨 {} URGENT messages!", urgent_messages.len());
                    for msg in &urgent_messages {
                        if let tinytown::MessageType::Custom { kind, payload } = &msg.msg_type {
                            info!("      └─ [{}] {}", kind, payload);
                        }
                    }
                    // Log that we processed urgent messages
                    channel
                        .log_agent_activity(
                            agent_id,
                            &format!(
                                "Round {}: 🚨 processed {} urgent",
                                round,
                                urgent_messages.len()
                            ),
                        )
                        .await?;
                }

                // Check regular inbox for messages
                let inbox_len = channel.inbox_len(agent_id).await?;
                if inbox_len == 0 && urgent_messages.is_empty() {
                    info!("   📭 Inbox empty, waiting...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                info!("   📬 {} messages in inbox", inbox_len);

                // Build urgent messages section
                let urgent_section = if urgent_messages.is_empty() {
                    String::new()
                } else {
                    let mut section = String::from("\n## 🚨 URGENT MESSAGES (handle first!)\n\n");
                    for msg in &urgent_messages {
                        if let tinytown::MessageType::Custom { payload, .. } = &msg.msg_type {
                            section.push_str(&format!("- {}\n", payload));
                        }
                    }
                    section
                };

                // Build prompt with agent context
                let prompt = format!(
                    r#"# Agent: {name}

You are agent "{name}" in Tinytown "{town_name}".
{urgent_section}
## Available Commands

```bash
tt status                    # Check town status and all agents
tt inbox {name}              # Check YOUR inbox for messages
tt assign <agent> "task"     # Send task to another agent
tt send <agent> "message"    # Send message to another agent
tt send <agent> --urgent "!" # Send urgent message
```

## Current State
- Round: {round}/{max_rounds}
- Messages waiting: {inbox_len}
- Urgent messages: {urgent_count}

## Your Workflow

1. **Handle URGENT messages first** (if any above)
2. **Check your inbox**: `tt inbox {name}`
3. **Do the work** requested in messages
4. **Check for more work**: `tt inbox {name}` again
5. **If more messages**, continue working on them
6. **If inbox empty**, you can finish this round
7. **If blocked**, send message to conductor or another agent

**Don't just exit** - keep checking `tt inbox {name}` and working until your inbox is empty!

## Hand-offs

If you need another agent to do something:
```bash
tt assign reviewer "Please review src/auth.rs for security issues"
```

If you're done and want to notify someone:
```bash
tt send conductor "Auth API complete. Ready for review."
```

Begin work. Check your inbox and keep working until it's empty.
"#,
                    name = name,
                    town_name = config.name,
                    urgent_section = urgent_section,
                    round = round,
                    max_rounds = max_rounds,
                    inbox_len = inbox_len,
                    urgent_count = urgent_messages.len(),
                );

                // Write prompt to temp file
                let prompt_file = cli.town.join(format!(".agent_{}_prompt.md", name));
                std::fs::write(&prompt_file, &prompt)?;

                // Update agent state to working
                if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                    agent.state = AgentState::Working;
                    channel.set_agent_state(&agent).await?;
                }

                // Run the AI model
                info!("   🤖 Running {}...", model_name);
                let output_file = cli.town.join(format!("logs/{}_round_{}.log", name, round));
                let output = std::fs::File::create(&output_file)?;

                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("cat '{}' | {}", prompt_file.display(), model_cmd))
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
                        info!("   ✅ Round {} complete", round);
                        format!("Round {}: ✅ completed", round)
                    }
                    Ok(_) => {
                        info!("   ⚠️ Model exited with error");
                        format!("Round {}: ⚠️ model error", round)
                    }
                    Err(e) => {
                        info!("   ❌ Failed to run model: {}", e);
                        format!("Round {}: ❌ failed: {}", round, e)
                    }
                };

                // Store activity in Redis (bounded, TTL'd)
                channel.log_agent_activity(agent_id, &activity_msg).await?;

                if status.is_err() {
                    break;
                }

                // Update agent state back to idle
                if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                    agent.state = AgentState::Idle;
                    channel.set_agent_state(&agent).await?;
                }

                // Small delay between rounds
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // Mark agent as stopped
            if let Some(mut agent) = channel.get_agent_state(agent_id).await? {
                agent.state = AgentState::Stopped;
                channel.set_agent_state(&agent).await?;
            }

            info!("🏁 Agent '{}' finished after {} rounds", name, max_rounds);
        }

        Commands::Conductor => {
            let town = Town::connect(&cli.town).await?;
            let config = town.config();

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

## Your Capabilities

You have access to the `tt` CLI tool. Run these commands in your shell to orchestrate:

### Spawn agents (starts actual AI process!)
```bash
tt spawn <name>                    # Spawn agent with default model (backgrounds)
tt spawn <name> --foreground       # Run in foreground (see output)
tt spawn <name> --max-rounds 5     # Limit iterations (default: 10)
```

### Assign tasks
```bash
tt assign <agent> "<task description>"
```

### Send messages between agents
```bash
tt send <agent> "message"          # Send message to agent's inbox
tt send <agent> --urgent "msg"     # URGENT: processed first next round
tt inbox <agent>                   # Check agent's inbox
```

### Check status
```bash
tt status         # Overview of town and agents
tt status --deep  # Deep status with recent agent activity
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
5. **Monitor** progress with `tt status` (use `--deep` to see recent activity)
6. **Coordinate** handoffs between agents
7. **Check with reviewer** to decide when work is complete
8. **Cleanup**: When done, stop agents with `tt kill <agent>`

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
            );

            // Write context to a temp file for the model
            let context_file = cli.town.join(".conductor_context.md");
            std::fs::write(&context_file, &context)?;

            // Get the model command
            let model = &config.default_model;
            let model_config = config.models.get(model);

            info!("🚂 Starting conductor with {} model...", model);
            info!("   Context: {}", context_file.display());
            info!("");

            // Get the command for the model
            let command = if let Some(m) = model_config {
                m.command.clone()
            } else {
                model.clone() // Fallback to model name as command
            };

            // Launch the model with the context
            // Claude CLI: cat context | claude
            // Most AI CLIs accept input from stdin or can read files
            let shell_cmd = format!(
                "cat '{}' && echo '' && echo '---' && echo '' && {}",
                context_file.display(),
                command
            );

            info!("   Running: {}", command);
            info!("");

            // Execute the AI model interactively
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&shell_cmd)
                .current_dir(&cli.town)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()?;

            if !status.success() {
                info!("❌ Conductor exited with error");
            } else {
                info!("👋 Conductor signing off!");
            }

            // Cleanup context file
            let _ = std::fs::remove_file(&context_file);
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
    }

    Ok(())
}
