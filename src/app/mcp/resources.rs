/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! MCP resource definitions for Tinytown state discovery.

use std::sync::Arc;
use tower_mcp::protocol::ReadResourceResult;
use tower_mcp::{Resource, ResourceBuilder, ResourceTemplate, ResourceTemplateBuilder};

use super::McpState;

// ============================================================================
// Static Resources
// ============================================================================

/// Create the tinytown://town/current resource.
pub fn town_current_resource(state: Arc<McpState>) -> Resource {
    let s = state.clone();
    ResourceBuilder::new("tinytown://town/current")
        .name("Current Town")
        .description("Current town state including configuration and summary")
        .handler(move || {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::status(&state.town).await {
                    Ok(s) => {
                        let json = serde_json::json!({
                            "name": s.name,
                            "root": s.root,
                            "redis_url": s.redis_url,
                            "agent_count": s.agent_count,
                            "agents": s.agents.iter().map(|a| serde_json::json!({
                                "id": a.id.to_string(),
                                "name": a.name,
                                "cli": a.cli,
                                "state": format!("{:?}", a.state),
                                "rounds_completed": a.rounds_completed,
                                "tasks_completed": a.tasks_completed,
                                "inbox_len": a.inbox_len,
                                "urgent_len": a.urgent_len
                            })).collect::<Vec<_>>()
                        });
                        Ok(ReadResourceResult::text(
                            "tinytown://town/current",
                            serde_json::to_string_pretty(&json).unwrap_or_default(),
                        ))
                    }
                    Err(e) => Ok(ReadResourceResult::text(
                        "tinytown://town/current",
                        format!("Error: {}", e),
                    )),
                }
            }
        })
        .build()
}

/// Create the tinytown://agents resource.
pub fn agents_resource(state: Arc<McpState>) -> Resource {
    let s = state.clone();
    ResourceBuilder::new("tinytown://agents")
        .name("All Agents")
        .description("List of all agents in the town")
        .handler(move || {
            let state = s.clone();
            async move {
                use crate::AgentService;
                match AgentService::list(&state.town).await {
                    Ok(agents) => {
                        let json: Vec<_> = agents
                            .iter()
                            .map(|a| {
                                serde_json::json!({
                                    "id": a.id.to_string(),
                                    "name": a.name,
                                    "cli": a.cli,
                                    "state": format!("{:?}", a.state),
                                    "rounds_completed": a.rounds_completed,
                                    "tasks_completed": a.tasks_completed,
                                    "inbox_len": a.inbox_len,
                                    "urgent_len": a.urgent_len
                                })
                            })
                            .collect();
                        Ok(ReadResourceResult::text(
                            "tinytown://agents",
                            serde_json::to_string_pretty(&json).unwrap_or_default(),
                        ))
                    }
                    Err(e) => Ok(ReadResourceResult::text(
                        "tinytown://agents",
                        format!("Error: {}", e),
                    )),
                }
            }
        })
        .build()
}

/// Create the tinytown://backlog resource.
pub fn backlog_resource(state: Arc<McpState>) -> Resource {
    let s = state.clone();
    ResourceBuilder::new("tinytown://backlog")
        .name("Backlog")
        .description("Current task backlog")
        .handler(move || {
            let state = s.clone();
            async move {
                use crate::BacklogService;
                match BacklogService::list(state.town.channel()).await {
                    Ok(items) => {
                        let json: Vec<_> = items
                            .iter()
                            .map(|i| {
                                serde_json::json!({
                                    "task_id": i.task_id.to_string(),
                                    "description": i.description,
                                    "tags": i.tags
                                })
                            })
                            .collect();
                        Ok(ReadResourceResult::text(
                            "tinytown://backlog",
                            serde_json::to_string_pretty(&json).unwrap_or_default(),
                        ))
                    }
                    Err(e) => Ok(ReadResourceResult::text(
                        "tinytown://backlog",
                        format!("Error: {}", e),
                    )),
                }
            }
        })
        .build()
}

// ============================================================================
// Resource Templates
// ============================================================================

/// Create the tinytown://agents/{agent_name} resource template.
pub fn agent_by_name_template(state: Arc<McpState>) -> ResourceTemplate {
    let s = state.clone();
    ResourceTemplateBuilder::new("tinytown://agents/{agent_name}")
        .name("Agent Details")
        .description("Details for a specific agent by name")
        .handler(
            move |uri: String, vars: std::collections::HashMap<String, String>| {
                let state = s.clone();
                async move {
                    use crate::AgentService;
                    let agent_name = vars.get("agent_name").cloned().unwrap_or_default();
                    match AgentService::list(&state.town).await {
                        Ok(agents) => {
                            if let Some(agent) = agents.iter().find(|a| a.name == agent_name) {
                                let json = serde_json::json!({
                                    "id": agent.id.to_string(),
                                    "name": agent.name,
                                    "cli": agent.cli,
                                    "state": format!("{:?}", agent.state),
                                    "rounds_completed": agent.rounds_completed,
                                    "tasks_completed": agent.tasks_completed,
                                    "inbox_len": agent.inbox_len,
                                    "urgent_len": agent.urgent_len
                                });
                                Ok(ReadResourceResult::text(
                                    uri,
                                    serde_json::to_string_pretty(&json).unwrap_or_default(),
                                ))
                            } else {
                                Ok(ReadResourceResult::text(
                                    uri,
                                    format!("Agent not found: {}", agent_name),
                                ))
                            }
                        }
                        Err(e) => Ok(ReadResourceResult::text(uri, format!("Error: {}", e))),
                    }
                }
            },
        )
}

/// Create the tinytown://tasks/{task_id} resource template.
pub fn task_by_id_template(state: Arc<McpState>) -> ResourceTemplate {
    let s = state.clone();
    ResourceTemplateBuilder::new("tinytown://tasks/{task_id}")
        .name("Task Details")
        .description("Details for a specific task by ID")
        .handler(
            move |uri: String, vars: std::collections::HashMap<String, String>| {
                let state = s.clone();
                async move {
                    use crate::BacklogService;
                    use crate::TaskId;
                    let task_id_str = vars.get("task_id").cloned().unwrap_or_default();
                    let task_id: TaskId = match task_id_str.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Ok(ReadResourceResult::text(
                                uri,
                                format!("Invalid task ID: {}", task_id_str),
                            ));
                        }
                    };
                    match BacklogService::list(state.town.channel()).await {
                        Ok(items) => {
                            if let Some(item) = items.iter().find(|i| i.task_id == task_id) {
                                let json = serde_json::json!({
                                    "task_id": item.task_id.to_string(),
                                    "description": item.description,
                                    "tags": item.tags
                                });
                                Ok(ReadResourceResult::text(
                                    uri,
                                    serde_json::to_string_pretty(&json).unwrap_or_default(),
                                ))
                            } else {
                                Ok(ReadResourceResult::text(
                                    uri,
                                    format!("Task not found: {}", task_id_str),
                                ))
                            }
                        }
                        Err(e) => Ok(ReadResourceResult::text(uri, format!("Error: {}", e))),
                    }
                }
            },
        )
}

// ============================================================================
// Resource Registration
// ============================================================================

/// Return all static resources.
pub fn all_resources(state: Arc<McpState>) -> Vec<Resource> {
    vec![
        town_current_resource(state.clone()),
        agents_resource(state.clone()),
        backlog_resource(state),
    ]
}

/// Return all resource templates.
pub fn all_templates(state: Arc<McpState>) -> Vec<ResourceTemplate> {
    vec![
        agent_by_name_template(state.clone()),
        task_by_id_template(state),
    ]
}
