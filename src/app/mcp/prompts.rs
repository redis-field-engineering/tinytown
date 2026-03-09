/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! MCP prompt definitions for Tinytown context generation.

use std::collections::HashMap;
use std::sync::Arc;
use tower_mcp::protocol::GetPromptResult;
use tower_mcp::{Prompt, PromptBuilder};

use super::McpState;

/// Create the conductor.startup_context prompt.
pub fn conductor_startup_context_prompt(state: Arc<McpState>) -> Prompt {
    let s = state.clone();
    PromptBuilder::new("conductor.startup_context")
        .description("Generate startup context for the conductor agent")
        .handler(move |_args: HashMap<String, String>| {
            let state = s.clone();
            async move {
                use crate::AgentService;
                use crate::BacklogService;

                let status = AgentService::status(&state.town).await.ok();
                let backlog = BacklogService::list(state.town.channel()).await.ok();

                let mut context = String::new();
                context.push_str("# Tinytown Conductor Startup Context\n\n");

                if let Some(s) = status {
                    context.push_str(&format!("## Town: {}\n", s.name));
                    context.push_str(&format!("- Root: {}\n", s.root));
                    context.push_str(&format!("- Agent count: {}\n\n", s.agent_count));

                    if !s.agents.is_empty() {
                        context.push_str("## Active Agents\n");
                        for agent in &s.agents {
                            context.push_str(&format!(
                                "- **{}** ({}): {:?}, {} rounds, {} inbox\n",
                                agent.name,
                                agent.cli,
                                agent.state,
                                agent.rounds_completed,
                                agent.inbox_len
                            ));
                        }
                        context.push('\n');
                    }
                }

                if let Some(items) = backlog
                    && !items.is_empty()
                {
                    context.push_str("## Backlog\n");
                    for item in &items {
                        let tags = if item.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", item.tags.join(", "))
                        };
                        context.push_str(&format!("- {}{}\n", item.description, tags));
                    }
                    context.push('\n');
                }

                context.push_str("## Available MCP Tools\n");
                context.push_str("- `town.get_status` - Get town status\n");
                context.push_str("- `agent.list` - List all agents\n");
                context.push_str("- `agent.spawn` - Spawn new agent\n");
                context.push_str("- `agent.kill` - Stop an agent\n");
                context.push_str("- `task.assign` - Assign task to agent\n");
                context.push_str("- `message.send` - Send message to agent\n");
                context.push_str("- `backlog.list/add/claim` - Manage backlog\n");
                context.push_str("- `recovery.*` - Recovery operations\n");

                Ok(GetPromptResult::builder()
                    .description(
                        "Conductor startup context with town state and available operations",
                    )
                    .user(context)
                    .build())
            }
        })
        .build()
}

/// Create the agent.role_hint prompt.
/// This prompt doesn't require town state, so it doesn't need the state parameter.
pub fn agent_role_hint_prompt() -> Prompt {
    PromptBuilder::new("agent.role_hint")
        .description("Generate role hints for an agent based on its name and tags")
        .required_arg("agent_name", "Name of the agent to generate hints for")
        .optional_arg("tags", "Comma-separated tags for role matching")
        .handler(|args: HashMap<String, String>| async move {
            let agent_name = args
                .get("agent_name")
                .map(|s| s.as_str())
                .unwrap_or("agent");
            let tags = args
                .get("tags")
                .map(|s| s.split(',').map(|t| t.trim()).collect::<Vec<_>>())
                .unwrap_or_default();

            let mut hint = String::new();
            hint.push_str(&format!("# Role Hint for Agent: {}\n\n", agent_name));

            // Generate role hints based on agent name patterns
            let name_lower = agent_name.to_lowercase();
            if name_lower.contains("backend") || name_lower.contains("api") {
                hint.push_str("## Suggested Focus: Backend Development\n");
                hint.push_str("- Prioritize tasks tagged: backend, api, server, database\n");
                hint.push_str("- Handle: REST endpoints, data models, business logic\n");
            } else if name_lower.contains("frontend") || name_lower.contains("ui") {
                hint.push_str("## Suggested Focus: Frontend Development\n");
                hint.push_str("- Prioritize tasks tagged: frontend, ui, web, components\n");
                hint.push_str("- Handle: UI components, styling, user interactions\n");
            } else if name_lower.contains("test") || name_lower.contains("qa") {
                hint.push_str("## Suggested Focus: Testing & QA\n");
                hint.push_str("- Prioritize tasks tagged: test, qa, validation\n");
                hint.push_str("- Handle: Unit tests, integration tests, test coverage\n");
            } else if name_lower.contains("review") {
                hint.push_str("## Suggested Focus: Code Review\n");
                hint.push_str("- Prioritize tasks tagged: review, pr, feedback\n");
                hint.push_str("- Handle: PR reviews, code quality, suggestions\n");
            } else if name_lower.contains("supervisor") || name_lower.contains("conductor") {
                hint.push_str("## Suggested Focus: Orchestration\n");
                hint.push_str("- Coordinate other agents\n");
                hint.push_str("- Manage task distribution and progress tracking\n");
            } else {
                hint.push_str("## General Worker\n");
                hint.push_str("- Handle any assigned tasks\n");
                hint.push_str("- Claim backlog tasks matching your capabilities\n");
            }

            if !tags.is_empty() {
                hint.push_str(&format!("\n## Tag Filters: {}\n", tags.join(", ")));
                hint.push_str("- Prioritize backlog tasks matching these tags\n");
            }

            Ok(GetPromptResult::builder()
                .description(format!("Role hints for agent {}", agent_name))
                .user(hint)
                .build())
        })
        .build()
}

/// Return all prompts.
pub fn all_prompts(state: Arc<McpState>) -> Vec<Prompt> {
    vec![
        conductor_startup_context_prompt(state),
        agent_role_hint_prompt(),
    ]
}
