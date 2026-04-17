use serde::Deserialize;

use crate::error::Result;
use armitage_core::node::IssueRef;

// ---------------------------------------------------------------------------
// GitHub API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<GitHubLabel>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubLabel {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepoLabel {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedIssue {
    pub number: u64,
    pub url: String,
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Fetch a GitHub issue by its reference.
pub fn fetch_issue(gh: &ionem::shell::gh::Gh, issue_ref: &IssueRef) -> Result<GitHubIssue> {
    tracing::debug!(repo = %issue_ref.repo_full(), number = issue_ref.number, "gh issue view");
    let json = gh.run(&[
        "issue",
        "view",
        &issue_ref.number.to_string(),
        "--repo",
        &issue_ref.repo_full(),
        "--json",
        "number,title,body,state,labels,updatedAt",
    ])?;
    let issue: GitHubIssue = serde_json::from_str(&json)?;
    Ok(issue)
}

/// Fetch all labels defined on a repository.
pub fn fetch_repo_labels(gh: &ionem::shell::gh::Gh, repo: &str) -> Result<Vec<GitHubRepoLabel>> {
    tracing::debug!(repo = repo, "gh label list");
    let json = gh.run(&[
        "label",
        "list",
        "--repo",
        repo,
        "--json",
        "name,description,color",
    ])?;
    let labels: Vec<GitHubRepoLabel> = serde_json::from_str(&json)?;
    tracing::debug!(repo = repo, count = labels.len(), "fetched labels");
    Ok(labels)
}

/// Create a new GitHub issue and return the created issue details.
pub fn create_issue(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<CreatedIssue> {
    tracing::debug!(repo = repo, labels = labels.len(), "gh issue create");
    let mut args = vec![
        "issue",
        "create",
        "--repo",
        repo,
        "--title",
        title,
        "--body",
        body,
        "--json",
        "number,url",
    ];

    // Build label args (we need owned strings alive for the duration)
    let label_args: Vec<String> = labels
        .iter()
        .flat_map(|l| vec!["--label".to_string(), l.clone()])
        .collect();
    let label_refs: Vec<&str> = label_args.iter().map(std::string::String::as_str).collect();
    args.extend_from_slice(&label_refs);

    let json = gh.run(&args)?;
    let created: CreatedIssue = serde_json::from_str(&json)?;
    Ok(created)
}

/// Update an existing GitHub issue (title, body, and/or labels).
pub fn update_issue(
    gh: &ionem::shell::gh::Gh,
    issue_ref: &IssueRef,
    title: Option<&str>,
    body: Option<&str>,
    add_labels: &[String],
    remove_labels: &[String],
) -> Result<()> {
    tracing::debug!(
        repo = %issue_ref.repo_full(),
        number = issue_ref.number,
        title_changed = title.is_some(),
        body_changed = body.is_some(),
        add_labels = add_labels.len(),
        remove_labels = remove_labels.len(),
        "gh issue edit"
    );
    let number_str = issue_ref.number.to_string();
    let repo_full = issue_ref.repo_full();

    let mut args = vec!["issue", "edit", &number_str, "--repo", &repo_full];

    if let Some(t) = title {
        args.push("--title");
        args.push(t);
    }

    if let Some(b) = body {
        args.push("--body");
        args.push(b);
    }

    let add_label_args: Vec<String> = add_labels
        .iter()
        .flat_map(|l| vec!["--add-label".to_string(), l.clone()])
        .collect();
    let add_label_refs: Vec<&str> = add_label_args
        .iter()
        .map(std::string::String::as_str)
        .collect();
    args.extend_from_slice(&add_label_refs);

    let remove_label_args: Vec<String> = remove_labels
        .iter()
        .flat_map(|l| vec!["--remove-label".to_string(), l.clone()])
        .collect();
    let remove_label_refs: Vec<&str> = remove_label_args
        .iter()
        .map(std::string::String::as_str)
        .collect();
    args.extend_from_slice(&remove_label_refs);

    gh.run(&args)?;
    Ok(())
}

/// Open or close a GitHub issue.
pub fn set_issue_state(gh: &ionem::shell::gh::Gh, issue_ref: &IssueRef, open: bool) -> Result<()> {
    tracing::debug!(
        repo = %issue_ref.repo_full(),
        number = issue_ref.number,
        state = if open { "open" } else { "closed" },
        "gh issue state change"
    );
    let number_str = issue_ref.number.to_string();
    let repo_full = issue_ref.repo_full();

    if open {
        gh.run(&["issue", "reopen", &number_str, "--repo", &repo_full])?;
    } else {
        gh.run(&["issue", "close", &number_str, "--repo", &repo_full])?;
    }
    Ok(())
}

/// Rename a label on a repository. Atomically updates all issues.
pub fn rename_label(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    old_name: &str,
    new_name: &str,
) -> Result<()> {
    tracing::debug!(
        repo = repo,
        old_name = old_name,
        new_name = new_name,
        "gh label edit (rename)"
    );
    gh.run(&[
        "label", "edit", old_name, "--name", new_name, "--repo", repo,
    ])?;
    Ok(())
}

/// Create a new label on a repository.
pub fn create_label(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    name: &str,
    description: &str,
    color: Option<&str>,
) -> Result<()> {
    tracing::debug!(repo = repo, name = name, "gh label create");
    let mut args = vec![
        "label",
        "create",
        name,
        "--description",
        description,
        "--force",
        "--repo",
        repo,
    ];
    if let Some(c) = color {
        args.push("--color");
        args.push(c);
    }
    gh.run(&args)?;
    Ok(())
}

/// Update a label's description and/or color on a repository.
pub fn update_label_metadata(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    name: &str,
    description: &str,
    color: Option<&str>,
) -> Result<()> {
    tracing::debug!(repo = repo, name = name, "gh label edit (metadata)");
    let mut args = vec![
        "label",
        "edit",
        name,
        "--description",
        description,
        "--repo",
        repo,
    ];
    if let Some(c) = color {
        args.push("--color");
        args.push(c);
    }
    gh.run(&args)?;
    Ok(())
}

/// Delete a label from a repository.
pub fn delete_label(gh: &ionem::shell::gh::Gh, repo: &str, label_name: &str) -> Result<()> {
    tracing::debug!(repo = repo, label_name = label_name, "gh label delete");
    gh.run(&["label", "delete", label_name, "--yes", "--repo", repo])?;
    Ok(())
}

/// Add a comment to a GitHub issue.
pub fn add_comment(gh: &ionem::shell::gh::Gh, issue_ref: &IssueRef, body: &str) -> Result<()> {
    tracing::debug!(
        repo = %issue_ref.repo_full(),
        number = issue_ref.number,
        body_len = body.len(),
        "gh issue comment"
    );
    let number_str = issue_ref.number.to_string();
    let repo_full = issue_ref.repo_full();
    gh.run(&[
        "issue",
        "comment",
        &number_str,
        "--repo",
        &repo_full,
        "--body",
        body,
    ])?;
    Ok(())
}

/// List all issue numbers that have a specific label (all states).
pub fn list_issues_with_label(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    label: &str,
) -> Result<Vec<u64>> {
    #[derive(serde::Deserialize)]
    struct IssueNum {
        number: u64,
    }

    tracing::debug!(repo = repo, label = label, "gh issue list --label");
    let json = gh.run(&[
        "issue", "list", "--label", label, "--state", "all", "--limit", "9999", "--repo", repo,
        "--json", "number",
    ])?;
    let issues: Vec<IssueNum> = serde_json::from_str(&json)?;
    Ok(issues.into_iter().map(|i| i.number).collect())
}

/// List all non-archived, non-fork repos in a GitHub org (returns "owner/name" strings).
pub fn list_org_repos(gh: &ionem::shell::gh::Gh, org: &str) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Repo {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }

    tracing::debug!(org = org, "gh repo list");
    let json = gh.run(&[
        "repo",
        "list",
        org,
        "--no-archived",
        "--source",
        "--limit",
        "1000",
        "--json",
        "nameWithOwner",
    ])?;
    let repos: Vec<Repo> = serde_json::from_str(&json)?;
    Ok(repos.into_iter().map(|r| r.name_with_owner).collect())
}

/// Metadata fetched from GitHub for a single repo.
#[derive(Debug, Clone)]
pub struct RepoMetadata {
    /// Canonical `owner/repo` name as reported by GitHub.
    pub name_with_owner: String,
    pub is_archived: bool,
}

/// Fetch archived status and canonical name for a single repo.
///
/// Returns `None` when the repo does not exist or the `gh` call fails (e.g.
/// network unavailable), so callers can skip rather than hard-error.
pub fn fetch_repo_metadata(gh: &ionem::shell::gh::Gh, repo: &str) -> Option<RepoMetadata> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
        #[serde(rename = "isArchived")]
        is_archived: bool,
    }

    tracing::debug!(repo = repo, "gh repo view metadata");
    let json = gh
        .run(&["repo", "view", repo, "--json", "isArchived,nameWithOwner"])
        .ok()?;
    let raw: Raw = serde_json::from_str(&json).ok()?;
    Some(RepoMetadata {
        name_with_owner: raw.name_with_owner,
        is_archived: raw.is_archived,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_issue_json() {
        let json = r#"{
            "number": 42,
            "title": "Fix the thing",
            "body": "Some description",
            "state": "OPEN",
            "labels": [
                {"name": "bug"},
                {"name": "priority:high"}
            ],
            "updatedAt": "2026-01-15T10:30:00Z"
        }"#;

        let issue: GitHubIssue = serde_json::from_str(json).expect("parse github issue");
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Fix the thing");
        assert_eq!(issue.body, "Some description");
        assert_eq!(issue.state, "OPEN");
        assert_eq!(issue.labels.len(), 2);
        assert_eq!(issue.labels[0].name, "bug");
        assert_eq!(issue.labels[1].name, "priority:high");
        assert_eq!(issue.updated_at, "2026-01-15T10:30:00Z");
    }

    #[test]
    fn parse_created_issue_url() {
        let json = r#"{
            "number": 99,
            "url": "https://github.com/owner/repo/issues/99"
        }"#;

        let created: CreatedIssue = serde_json::from_str(json).expect("parse created issue");
        assert_eq!(created.number, 99);
        assert_eq!(created.url, "https://github.com/owner/repo/issues/99");
    }

    #[test]
    fn parse_github_label_list_json() {
        let json = r#"[
            {"name":"bug","description":"Broken behavior","color":"D73A4A"},
            {"name":"priority:high","description":"Needs prompt attention","color":"B60205"}
        ]"#;

        let labels: Vec<GitHubRepoLabel> = serde_json::from_str(json).expect("parse repo labels");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].name, "bug");
        assert_eq!(labels[0].description.as_deref(), Some("Broken behavior"));
        assert_eq!(labels[0].color, "D73A4A");
    }
}
