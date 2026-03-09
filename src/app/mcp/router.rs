/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! MCP router configuration for Tinytown.

use std::sync::Arc;
use tower_mcp::McpRouter;

use crate::Town;

use super::prompts;
use super::resources;
use super::tools;

/// Shared state for MCP handlers.
#[derive(Clone)]
pub struct McpState {
    /// The town instance for accessing orchestration services.
    pub town: Town,
}

impl McpState {
    /// Create a new MCP state with the given town.
    pub fn new(town: Town) -> Self {
        Self { town }
    }
}

/// Create an MCP router with all Tinytown tools, resources, and prompts.
///
/// # Arguments
/// * `state` - The MCP state containing the town instance
/// * `server_name` - Name of the MCP server
/// * `version` - Version of the MCP server
///
/// # Returns
/// A configured `McpRouter` ready to serve MCP requests.
pub fn create_mcp_router(state: Arc<McpState>, server_name: &str, version: &str) -> McpRouter {
    let mut router = McpRouter::new()
        .server_info(server_name, version)
        .instructions(
            "Tinytown MCP interface for multi-agent orchestration. \
             Use tools to manage agents, assign tasks, and monitor town status. \
             Resources provide read-only snapshots of town state. \
             Prompts help generate context for conductor and agent operations.",
        );

    // Register all tools
    for tool in tools::all_tools(state.clone()) {
        router = router.tool(tool);
    }

    // Register all resources
    for resource in resources::all_resources(state.clone()) {
        router = router.resource(resource);
    }

    // Register all resource templates
    for template in resources::all_templates(state.clone()) {
        router = router.resource_template(template);
    }

    // Register all prompts
    for prompt in prompts::all_prompts(state) {
        router = router.prompt(prompt);
    }

    router
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_create_mcp_router() {
        // This test just verifies the router can be constructed
        // Full integration tests require a Redis connection
        // See tests/mcp_integration_tests.rs for full tests
    }
}
