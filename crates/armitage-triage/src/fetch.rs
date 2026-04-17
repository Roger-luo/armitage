use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::Connection;

use crate::db::{self, StoredIssue};
use crate::error::{Error, Result};
use armitage_core::tree::walk_nodes;

// ---------------------------------------------------------------------------
// GitHub API response types (from `gh api`)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct SubIssuesSummary {
    total: u64,
}

#[derive(Debug, serde::Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, serde::Deserialize)]
struct ApiIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<ApiLabel>,
    updated_at: String,
    #[serde(default)]
    sub_issues_summary: Option<SubIssuesSummary>,
    #[serde(default)]
    user: Option<ApiUser>,
    #[serde(default)]
    assignees: Vec<ApiUser>,
    // Present on PR entries returned by the issues API
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct ApiLabel {
    name: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch all issues from a single repo into the database.
/// Returns the number of issues upserted.
pub fn fetch_repo_issues(
    gh: &armitage_github::Gh,
    conn: &Connection,
    repo: &str,
    since: Option<&str>,
) -> Result<usize> {
    // Build the API endpoint with query params
    use std::fmt::Write as _;
    let mut endpoint = format!("repos/{repo}/issues?state=all&per_page=100");
    if let Some(since) = since {
        let _ = write!(endpoint, "&since={since}");
    }

    let json = gh
        .run(&["api", &endpoint, "--paginate"])
        .map_err(armitage_github::error::Error::from)?;

    // gh api --paginate concatenates JSON arrays, producing e.g. [{...}][{...}]
    // We need to handle this by parsing each array separately.
    let issues = parse_paginated_json(&json)?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut count = 0;
    for api_issue in &issues {
        let labels: Vec<String> = api_issue.labels.iter().map(|l| l.name.clone()).collect();
        let sub_issues_count = api_issue.sub_issues_summary.as_ref().map_or(0, |s| s.total);
        let stored = StoredIssue {
            id: 0,
            repo: repo.to_string(),
            number: api_issue.number,
            title: api_issue.title.clone(),
            body: api_issue.body.clone().unwrap_or_default(),
            state: api_issue.state.clone(),
            labels,
            updated_at: api_issue.updated_at.clone(),
            fetched_at: now.clone(),
            sub_issues_count,
            author: api_issue
                .user
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_default(),
            assignees: api_issue
                .assignees
                .iter()
                .map(|u| u.login.clone())
                .collect(),
            is_pr: api_issue.pull_request.is_some(),
        };
        db::upsert_issue(conn, &stored)?;
        count += 1;
    }

    Ok(count)
}

/// Fetch issues from multiple repos.
/// If `repos` is empty, collects repos from node.toml files.
/// The org's `default_repo` is always included if set.
pub fn fetch_all(
    gh: &armitage_github::Gh,
    conn: &Connection,
    org_root: &Path,
    repos: &[String],
    default_repo: Option<&str>,
    since: Option<&str>,
) -> Result<usize> {
    let mut repos: BTreeSet<String> = if repos.is_empty() {
        collect_repos_from_nodes(org_root)?.into_iter().collect()
    } else {
        repos.iter().cloned().collect()
    };

    // Always include the default repo
    if let Some(dr) = default_repo {
        repos.insert(dr.to_string());
    }

    if repos.is_empty() {
        println!(
            "No repos found. Specify --repo, add repos to node.toml files, or set org.default_repo in armitage.toml."
        );
        return Ok(0);
    }

    let repos: Vec<String> = repos.into_iter().collect();

    let mut total = 0;
    for repo in &repos {
        // Use incremental fetch: auto-detect since from DB if not specified
        let effective_since = match since {
            Some(s) => Some(s.to_string()),
            None => db::get_latest_updated_at(conn, repo)?,
        };

        println!("Fetching {repo}...");
        match fetch_repo_issues(gh, conn, repo, effective_since.as_deref()) {
            Ok(n) => {
                println!("  {n} issues fetched");
                total += n;
            }
            Err(e) => {
                eprintln!("  error fetching {repo}: {e}");
            }
        }
    }

    Ok(total)
}

/// Collect unique repos referenced by all nodes in the org tree.
pub fn collect_repos_from_nodes(org_root: &Path) -> Result<Vec<String>> {
    let nodes = walk_nodes(org_root)?;
    let mut repos = BTreeSet::new();
    for entry in &nodes {
        for repo in &entry.node.repos {
            repos.insert(strip_repo_qualifier(repo));
        }
    }
    Ok(repos.into_iter().collect())
}

/// Strip optional `@qualifier` suffix from a repo string.
///
/// Node repos may use `owner/repo@branch` or `owner/repo@workspace` qualifiers
/// for triage classification affinity. For GitHub API operations (labels, issues)
/// we need the bare `owner/repo` name.
pub fn strip_repo_qualifier(repo: &str) -> String {
    repo.split_once('@')
        .map_or(repo, |(base, _)| base)
        .to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse the concatenated JSON arrays that `gh api --paginate` produces.
/// gh outputs `[...][...]` (arrays concatenated without separator).
fn parse_paginated_json(raw: &str) -> Result<Vec<ApiIssue>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }

    // First try: it might be a single clean array
    if let Ok(issues) = serde_json::from_str::<Vec<ApiIssue>>(trimmed) {
        return Ok(issues);
    }

    // Otherwise, split concatenated arrays: find each top-level [...] block
    let mut results = Vec::new();
    let mut depth = 0;
    let mut start = None;

    for (i, ch) in trimmed.char_indices() {
        match ch {
            '[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let chunk = &trimmed[s..=i];
                        let issues: Vec<ApiIssue> = serde_json::from_str(chunk)
                            .map_err(|e| Error::Other(format!("JSON parse error: {e}")))?;
                        results.extend(issues);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_array() {
        let json = r#"[{"number":1,"title":"A","body":"b","state":"open","labels":[],"updated_at":"2026-01-01T00:00:00Z"}]"#;
        let issues = parse_paginated_json(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 1);
    }

    #[test]
    fn parse_concatenated_arrays() {
        let json = r#"[{"number":1,"title":"A","body":"","state":"open","labels":[],"updated_at":"2026-01-01T00:00:00Z"}][{"number":2,"title":"B","body":"","state":"closed","labels":[{"name":"bug"}],"updated_at":"2026-02-01T00:00:00Z"}]"#;
        let issues = parse_paginated_json(json).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[1].number, 2);
        assert_eq!(issues[1].labels[0].name, "bug");
    }

    #[test]
    fn parse_empty() {
        let issues = parse_paginated_json("").unwrap();
        assert!(issues.is_empty());

        let issues = parse_paginated_json("[]").unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn strip_repo_qualifier_removes_at_suffix() {
        assert_eq!(strip_repo_qualifier("acme/atlas@rust"), "acme/atlas");
    }

    #[test]
    fn strip_repo_qualifier_no_op_without_at() {
        assert_eq!(strip_repo_qualifier("acme/atlas"), "acme/atlas");
    }
}
