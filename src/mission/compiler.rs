/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Work Graph Compiler: transforms issues/docs into a dependency-aware DAG.
//!
//! This module parses GitHub issue bodies and documents to extract work items
//! and their dependencies, building a directed acyclic graph for execution.
//!
//! # Dependency Detection
//!
//! The compiler recognizes these dependency patterns in issue bodies:
//! - `depends on #123` / `depends on #123, #456`
//! - `after #123`
//! - `blocked by #123`
//! - `requires #123`
//!
//! # Manual Override
//!
//! A mission manifest file can override or supplement detected dependencies:
//! ```toml
//! [[work_items]]
//! issue = 23
//! kind = "implement"
//! depends_on = [22, 21]
//! owner_role = "backend"
//! ```

use std::collections::{HashMap, HashSet, VecDeque};

use regex::Regex;
use serde::Deserialize;
use tracing::{debug, instrument, warn};

use crate::error::{Error, Result};
use crate::mission::types::{
    MissionId, ObjectiveRef, WorkItem, WorkItemId, WorkKind,
};

// ==================== Compiled Work Graph ====================

/// Result of compiling objectives into a work graph.
#[derive(Debug, Clone)]
pub struct WorkGraph {
    /// Work items in topological order (respecting dependencies).
    pub items: Vec<WorkItem>,
    /// Map from issue number to work item ID for cross-referencing.
    pub issue_map: HashMap<u64, WorkItemId>,
}

impl WorkGraph {
    /// Get items that are ready to execute (no dependencies).
    #[must_use]
    pub fn ready_items(&self) -> Vec<&WorkItem> {
        self.items.iter().filter(|item| item.depends_on.is_empty()).collect()
    }

    /// Check if the graph is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of work items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

// ==================== Parsed Issue ====================

/// Parsed information from a GitHub issue.
#[derive(Debug, Clone)]
pub struct ParsedIssue {
    /// Issue number
    pub number: u64,
    /// Issue title
    pub title: String,
    /// Issue body
    pub body: String,
    /// Repository owner
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Detected dependency issue numbers
    pub depends_on: Vec<u64>,
    /// Detected work kind
    pub kind: WorkKind,
    /// Suggested owner role
    pub owner_role: Option<String>,
}

// ==================== Manifest ====================

/// Manual mission manifest for dependency overrides.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MissionManifest {
    /// Work item overrides
    #[serde(default)]
    pub work_items: Vec<ManifestWorkItem>,
}

/// Work item definition in manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestWorkItem {
    /// Issue number this applies to
    pub issue: u64,
    /// Override work kind
    pub kind: Option<String>,
    /// Override dependencies (issue numbers)
    #[serde(default)]
    pub depends_on: Vec<u64>,
    /// Override owner role
    pub owner_role: Option<String>,
    /// Skip this issue
    #[serde(default)]
    pub skip: bool,
}

// ==================== Compiler ====================

/// Work Graph Compiler transforms objectives into executable work items.
///
/// The compiler parses issue bodies to detect dependencies, applies manifest
/// overrides, and produces a topologically sorted work graph.
pub struct WorkGraphCompiler {
    /// Regex patterns for dependency detection
    dependency_patterns: Vec<Regex>,
}

impl Default for WorkGraphCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkGraphCompiler {
    /// Create a new compiler with default dependency patterns.
    #[must_use]
    pub fn new() -> Self {
        // Compile regex patterns for dependency detection
        // These patterns match the keyword followed by issue references
        // We'll extract all #N numbers from the matched portion
        //
        // Pattern structure: keyword + issue list
        // Issue list: #N optionally followed by more #N with separators (comma, "and", space)
        // Using non-greedy matching and explicit delimiters to avoid false matches
        let patterns = vec![
            // "depends on #123" or "depends on #123, #456" or "depends on #123 and #456"
            r"(?i)depends?\s+on\s+(#\d+(?:\s*[,&]\s*#\d+|\s+and\s+#\d+)*)",
            // "after #123" or "after #123, #456"
            r"(?i)after\s+(#\d+(?:\s*[,&]\s*#\d+|\s+and\s+#\d+)*)",
            // "blocked by #123"
            r"(?i)blocked\s+by\s+(#\d+(?:\s*[,&]\s*#\d+|\s+and\s+#\d+)*)",
            // "requires #123"
            r"(?i)requires?\s+(#\d+(?:\s*[,&]\s*#\d+|\s+and\s+#\d+)*)",
        ];

        let dependency_patterns = patterns
            .into_iter()
            .map(|p| Regex::new(p).expect("Invalid regex pattern"))
            .collect();

        Self { dependency_patterns }
    }

    /// Parse dependencies from issue body text.
    #[instrument(skip(self, body), fields(body_len = body.len()))]
    pub fn parse_dependencies(&self, body: &str) -> Vec<u64> {
        let mut deps = HashSet::new();

        // Simple pattern: find all #N references after dependency keywords
        let number_pattern = Regex::new(r"#(\d+)").expect("Invalid regex");

        for pattern in &self.dependency_patterns {
            for cap in pattern.captures_iter(body) {
                // Extract the matched portion and find all issue numbers
                if let Some(m) = cap.get(0) {
                    for num_cap in number_pattern.captures_iter(m.as_str()) {
                        if let Some(num_match) = num_cap.get(1)
                            && let Ok(num) = num_match.as_str().parse::<u64>()
                        {
                            deps.insert(num);
                        }
                    }
                }
            }
        }

        let result: Vec<u64> = deps.into_iter().collect();
        debug!("Found {} dependencies", result.len());
        result
    }

    /// Infer work kind from issue title and body.
    #[must_use]
    pub fn infer_work_kind(&self, title: &str, body: &str) -> WorkKind {
        let text = format!("{} {}", title, body).to_lowercase();

        if text.contains("design") || text.contains("rfc") || text.contains("proposal") {
            WorkKind::Design
        } else if text.contains("test") || text.contains("spec") {
            WorkKind::Test
        } else if text.contains("review") {
            WorkKind::Review
        } else if text.contains("merge") {
            WorkKind::MergeGate
        } else if text.contains("followup") || text.contains("follow-up") || text.contains("fix") {
            WorkKind::Followup
        } else {
            WorkKind::Implement
        }
    }

    /// Infer owner role from issue labels or content.
    #[must_use]
    pub fn infer_owner_role(&self, title: &str, body: &str) -> Option<String> {
        let text = format!("{} {}", title, body).to_lowercase();

        if text.contains("backend") || text.contains("api") || text.contains("server") {
            Some("backend".to_string())
        } else if text.contains("frontend") || text.contains("ui") || text.contains("web") {
            Some("frontend".to_string())
        } else if text.contains("test") || text.contains("qa") {
            Some("tester".to_string())
        } else if text.contains("review") {
            Some("reviewer".to_string())
        } else if text.contains("devops") || text.contains("infrastructure") || text.contains("deploy") {
            Some("devops".to_string())
        } else {
            None
        }
    }

    /// Parse a GitHub issue into a ParsedIssue structure.
    #[must_use]
    pub fn parse_issue(
        &self,
        number: u64,
        title: String,
        body: String,
        owner: String,
        repo: String,
    ) -> ParsedIssue {
        let depends_on = self.parse_dependencies(&body);
        let kind = self.infer_work_kind(&title, &body);
        let owner_role = self.infer_owner_role(&title, &body);

        ParsedIssue {
            number,
            title,
            body,
            owner,
            repo,
            depends_on,
            kind,
            owner_role,
        }
    }

    /// Apply manifest overrides to a parsed issue.
    fn apply_manifest_overrides(
        &self,
        issue: &mut ParsedIssue,
        manifest: &MissionManifest,
    ) -> bool {
        if let Some(override_item) = manifest.work_items.iter().find(|w| w.issue == issue.number) {
            if override_item.skip {
                debug!("Skipping issue #{} per manifest", issue.number);
                return false;
            }

            if let Some(ref kind_str) = override_item.kind {
                issue.kind = match kind_str.to_lowercase().as_str() {
                    "design" => WorkKind::Design,
                    "implement" => WorkKind::Implement,
                    "test" => WorkKind::Test,
                    "review" => WorkKind::Review,
                    "merge_gate" => WorkKind::MergeGate,
                    "followup" => WorkKind::Followup,
                    _ => issue.kind,
                };
            }

            if !override_item.depends_on.is_empty() {
                issue.depends_on = override_item.depends_on.clone();
            }

            if override_item.owner_role.is_some() {
                issue.owner_role = override_item.owner_role.clone();
            }
        }
        true
    }

    /// Compile parsed issues into a work graph.
    ///
    /// This is the main compilation entry point. It:
    /// 1. Applies manifest overrides
    /// 2. Creates work items with proper dependencies
    /// 3. Performs topological sort
    /// 4. Returns error if cycle detected
    #[instrument(skip(self, issues, manifest))]
    pub fn compile(
        &self,
        mission_id: MissionId,
        mut issues: Vec<ParsedIssue>,
        manifest: Option<&MissionManifest>,
    ) -> Result<WorkGraph> {
        let manifest = manifest.cloned().unwrap_or_default();

        // Apply manifest overrides and filter skipped issues
        issues.retain_mut(|issue| self.apply_manifest_overrides(issue, &manifest));

        // Create work items and build issue -> WorkItemId map
        let mut issue_map: HashMap<u64, WorkItemId> = HashMap::new();
        let mut work_items: HashMap<WorkItemId, WorkItem> = HashMap::new();

        for issue in &issues {
            let mut item = WorkItem::new(
                mission_id,
                issue.title.clone(),
                issue.kind,
            );
            item = item.with_source_ref(format!("{}/#{}",
                ObjectiveRef::Issue {
                    owner: issue.owner.clone(),
                    repo: issue.repo.clone(),
                    number: issue.number,
                },
                issue.number
            ));

            if let Some(ref role) = issue.owner_role {
                item = item.with_owner_role(role.clone());
            }

            issue_map.insert(issue.number, item.id);
            work_items.insert(item.id, item);
        }

        // Resolve dependencies from issue numbers to WorkItemIds
        for issue in &issues {
            if let Some(&work_id) = issue_map.get(&issue.number) {
                let dep_ids: Vec<WorkItemId> = issue
                    .depends_on
                    .iter()
                    .filter_map(|dep_num| issue_map.get(dep_num).copied())
                    .collect();

                if !dep_ids.is_empty()
                    && let Some(item) = work_items.get_mut(&work_id)
                {
                    item.depends_on = dep_ids;
                }
            }
        }

        // Topological sort
        let sorted_items = self.topological_sort(work_items)?;

        debug!("Compiled {} work items", sorted_items.len());

        Ok(WorkGraph {
            items: sorted_items,
            issue_map,
        })
    }

    /// Perform Kahn's algorithm for topological sorting.
    /// Returns error if a cycle is detected.
    fn topological_sort(
        &self,
        items: HashMap<WorkItemId, WorkItem>,
    ) -> Result<Vec<WorkItem>> {
        let mut in_degree: HashMap<WorkItemId, usize> = HashMap::new();
        let mut dependents: HashMap<WorkItemId, Vec<WorkItemId>> = HashMap::new();

        // Initialize in-degrees
        for (id, item) in &items {
            in_degree.entry(*id).or_insert(0);
            for dep in &item.depends_on {
                *in_degree.entry(*id).or_insert(0) += 1;
                dependents.entry(*dep).or_default().push(*id);
            }
        }

        // Start with items that have no dependencies
        let mut queue: VecDeque<WorkItemId> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = Vec::new();

        while let Some(id) = queue.pop_front() {
            if let Some(item) = items.get(&id).cloned() {
                sorted.push(item);

                // Reduce in-degree for dependents
                if let Some(deps) = dependents.get(&id) {
                    for dep_id in deps {
                        if let Some(deg) = in_degree.get_mut(dep_id) {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.push_back(*dep_id);
                            }
                        }
                    }
                }
            }
        }

        // Check for cycles
        if sorted.len() != items.len() {
            let cycle_items: Vec<String> = items
                .iter()
                .filter(|(id, _)| in_degree.get(id).is_some_and(|&d| d > 0))
                .map(|(_, item)| item.title.clone())
                .collect();
            warn!("Cycle detected in work graph: {:?}", cycle_items);
            return Err(Error::Config(format!(
                "Cycle detected in work graph involving: {}",
                cycle_items.join(", ")
            )));
        }

        Ok(sorted)
    }

    /// Compile from objective references (convenience method).
    ///
    /// This method is intended for use when you have ObjectiveRefs and
    /// need to fetch issue data externally. It returns the parsed issues
    /// so the caller can populate them with actual issue data.
    #[must_use]
    pub fn extract_issue_refs(objectives: &[ObjectiveRef]) -> Vec<(String, String, u64)> {
        objectives
            .iter()
            .filter_map(|obj| match obj {
                ObjectiveRef::Issue { owner, repo, number } => {
                    Some((owner.clone(), repo.clone(), *number))
                }
                ObjectiveRef::Doc { .. } => None,
            })
            .collect()
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dependencies_depends_on() {
        let compiler = WorkGraphCompiler::new();
        let body = "This issue depends on #123 and #456";
        let deps = compiler.parse_dependencies(body);
        assert!(deps.contains(&123));
        assert!(deps.contains(&456));
    }

    #[test]
    fn test_parse_dependencies_after() {
        let compiler = WorkGraphCompiler::new();
        let body = "This should be done after #42";
        let deps = compiler.parse_dependencies(body);
        assert!(deps.contains(&42));
    }

    #[test]
    fn test_parse_dependencies_blocked_by() {
        let compiler = WorkGraphCompiler::new();
        let body = "Blocked by #99";
        let deps = compiler.parse_dependencies(body);
        assert!(deps.contains(&99));
    }

    #[test]
    fn test_parse_dependencies_requires() {
        let compiler = WorkGraphCompiler::new();
        let body = "Requires #10 to be completed first";
        let deps = compiler.parse_dependencies(body);
        assert!(deps.contains(&10));
    }

    #[test]
    fn test_parse_dependencies_case_insensitive() {
        let compiler = WorkGraphCompiler::new();
        let body = "DEPENDS ON #100";
        let deps = compiler.parse_dependencies(body);
        assert!(deps.contains(&100));
    }

    #[test]
    fn test_infer_work_kind() {
        let compiler = WorkGraphCompiler::new();
        assert_eq!(compiler.infer_work_kind("Design auth system", ""), WorkKind::Design);
        assert_eq!(compiler.infer_work_kind("Add unit tests", ""), WorkKind::Test);
        assert_eq!(compiler.infer_work_kind("Implement feature", ""), WorkKind::Implement);
    }

    #[test]
    fn test_compile_simple() {
        let compiler = WorkGraphCompiler::new();
        let mission_id = MissionId::new();

        let issues = vec![
            ParsedIssue {
                number: 1,
                title: "First issue".to_string(),
                body: "".to_string(),
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                depends_on: vec![],
                kind: WorkKind::Implement,
                owner_role: None,
            },
            ParsedIssue {
                number: 2,
                title: "Second issue".to_string(),
                body: "".to_string(),
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                depends_on: vec![1],
                kind: WorkKind::Implement,
                owner_role: None,
            },
        ];

        let graph = compiler.compile(mission_id, issues, None).unwrap();
        assert_eq!(graph.len(), 2);

        // First item should have no dependencies
        assert!(graph.items[0].depends_on.is_empty());
        // Second item should depend on first
        assert_eq!(graph.items[1].depends_on.len(), 1);
    }

    #[test]
    fn test_compile_detects_cycle() {
        let compiler = WorkGraphCompiler::new();
        let mission_id = MissionId::new();

        let issues = vec![
            ParsedIssue {
                number: 1,
                title: "First".to_string(),
                body: "".to_string(),
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                depends_on: vec![2],
                kind: WorkKind::Implement,
                owner_role: None,
            },
            ParsedIssue {
                number: 2,
                title: "Second".to_string(),
                body: "".to_string(),
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                depends_on: vec![1],
                kind: WorkKind::Implement,
                owner_role: None,
            },
        ];

        let result = compiler.compile(mission_id, issues, None);
        assert!(result.is_err());
    }
}

