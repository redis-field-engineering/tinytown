/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Mission bootstrap helpers shared by CLI and MCP surfaces.

use std::path::Path;
use std::process::Command;

use tracing::warn;

use crate::Result;
use crate::mission::{MissionId, ObjectiveRef, ParsedIssue, WorkGraphCompiler, WorkItem, WorkKind};

/// Parse an issue reference (e.g., "23", "owner/repo#23", or URL).
pub fn parse_issue_ref(input: &str, default_repo: &str, town_path: &Path) -> Option<ObjectiveRef> {
    if let Ok(number) = input.parse::<u64>() {
        if let Some((owner, repo)) = derive_git_remote_info(town_path) {
            return Some(ObjectiveRef::Issue {
                owner,
                repo,
                number,
            });
        }

        let repo_part = default_repo.split('-').next().unwrap_or("tinytown");
        warn!(
            "Could not derive GitHub owner from git remote; using repo '{}' with unknown owner",
            repo_part
        );
        return None;
    }

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

/// Build initial work items for a new mission from its objectives.
pub fn build_mission_work_items(
    town_path: &Path,
    mission_id: MissionId,
    objectives: &[ObjectiveRef],
) -> Result<Vec<WorkItem>> {
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

fn derive_git_remote_info(town_path: &Path) -> Option<(String, String)> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(town_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();

    if url.starts_with("git@github.com:") {
        let path = url
            .strip_prefix("git@github.com:")?
            .trim_end_matches(".git");
        let (owner, repo) = path.split_once('/')?;
        return Some((owner.to_string(), repo.to_string()));
    }

    if url.contains("github.com/") {
        let path = url.split("github.com/").nth(1)?.trim_end_matches(".git");
        let (owner, repo) = path.split_once('/')?;
        return Some((owner.to_string(), repo.to_string()));
    }

    None
}

#[derive(serde::Deserialize)]
struct GitHubIssueView {
    title: String,
    body: Option<String>,
}

fn fetch_issue_view(
    town_path: &Path,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Option<GitHubIssueView>> {
    let output = Command::new("gh")
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
                "Could not fetch issue {}/{}#{} via gh: {}",
                owner,
                repo,
                number,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(None)
        }
        Err(err) => {
            warn!(
                "Could not run gh for issue {}/{}#{}: {}",
                owner, repo, number, err
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_mission_work_items, parse_issue_ref};
    use crate::mission::{MissionId, ObjectiveRef, WorkKind};

    #[test]
    fn parses_full_issue_refs() {
        let path = std::path::Path::new(".");

        let owner_repo =
            parse_issue_ref("redis-field-engineering/tinytown#42", "tinytown-main", path);
        assert!(matches!(
            owner_repo,
            Some(ObjectiveRef::Issue {
                owner,
                repo,
                number: 42
            }) if owner == "redis-field-engineering" && repo == "tinytown"
        ));

        let url = parse_issue_ref(
            "https://github.com/redis-field-engineering/tinytown/issues/77",
            "tinytown-main",
            path,
        );
        assert!(matches!(
            url,
            Some(ObjectiveRef::Issue {
                owner,
                repo,
                number: 77
            }) if owner == "redis-field-engineering" && repo == "tinytown"
        ));
    }

    #[test]
    fn bootstraps_doc_objectives_without_github() {
        let mission_id = MissionId::new();
        let items = build_mission_work_items(
            std::path::Path::new("."),
            mission_id,
            &[ObjectiveRef::Doc {
                path: "docs/design.md".into(),
            }],
        )
        .expect("doc-only objectives should succeed");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].mission_id, mission_id);
        assert_eq!(items[0].title, "docs/design.md");
        assert_eq!(items[0].kind, WorkKind::Design);
        assert_eq!(items[0].source_ref.as_deref(), Some("docs/design.md"));
    }
}
