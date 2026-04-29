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
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];

    for label in labels {
        args.push("--label".to_string());
        args.push(label.clone());
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // gh issue create prints the issue URL on stdout (e.g.
    // "https://github.com/owner/repo/issues/42").  Extract the number from it.
    let output = gh.run(&arg_refs)?;
    let url = output.trim();
    let number = url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| {
            crate::error::Error::Other(format!("could not parse issue number from URL: {url}"))
        })?;
    Ok(CreatedIssue {
        number,
        url: url.to_string(),
    })
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

// ---------------------------------------------------------------------------
// Sub-issues API
// ---------------------------------------------------------------------------

/// Fetch the integer database ID of a GitHub issue.
///
/// This is distinct from the issue number and is required by the sub-issues
/// REST API (`POST /repos/.../issues/{n}/sub_issues`).
pub fn fetch_issue_database_id(gh: &ionem::shell::gh::Gh, issue_ref: &IssueRef) -> Result<u64> {
    tracing::debug!(
        repo = %issue_ref.repo_full(),
        number = issue_ref.number,
        "fetch issue database id"
    );
    let path = format!(
        "/repos/{}/{}/issues/{}",
        issue_ref.owner, issue_ref.repo, issue_ref.number
    );
    let json = gh.run(&["api", &path, "--jq", ".id"])?;
    let id: u64 = json
        .trim()
        .parse()
        .map_err(|_| crate::error::Error::Other(format!("invalid database id: {json}")))?;
    Ok(id)
}

/// Return the database IDs of existing sub-issues for a parent issue.
pub fn list_sub_issue_ids(gh: &ionem::shell::gh::Gh, parent_ref: &IssueRef) -> Result<Vec<u64>> {
    #[derive(serde::Deserialize)]
    struct SubIssue {
        id: u64,
    }

    tracing::debug!(
        repo = %parent_ref.repo_full(),
        number = parent_ref.number,
        "list sub-issues"
    );
    let path = format!(
        "/repos/{}/{}/issues/{}/sub_issues",
        parent_ref.owner, parent_ref.repo, parent_ref.number
    );
    let json = gh.run(&["api", &path])?;
    let items: Vec<SubIssue> = serde_json::from_str(&json)?;
    Ok(items.into_iter().map(|s| s.id).collect())
}

/// Register `child_db_id` as a sub-issue of `parent_ref`.
///
/// `child_db_id` is the integer database ID returned by the REST API (the
/// `id` field, not the `number`). Cross-repo sub-issues are supported as long
/// as both repos belong to the same GitHub organisation.
pub fn add_sub_issue(
    gh: &ionem::shell::gh::Gh,
    parent_ref: &IssueRef,
    child_db_id: u64,
) -> Result<()> {
    tracing::debug!(
        parent_repo = %parent_ref.repo_full(),
        parent_number = parent_ref.number,
        child_db_id = child_db_id,
        "add sub-issue"
    );
    let path = format!(
        "/repos/{}/{}/issues/{}/sub_issues",
        parent_ref.owner, parent_ref.repo, parent_ref.number
    );
    let field = format!("sub_issue_id={child_db_id}");
    gh.run(&["api", "-X", "POST", &path, "--field", &field])?;
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
    pub is_private: bool,
}

/// Fetch visibility, archived status, and canonical name for a single repo.
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
        #[serde(rename = "isPrivate")]
        is_private: bool,
    }

    tracing::debug!(repo = repo, "gh repo view metadata");
    let json = gh
        .run(&[
            "repo",
            "view",
            repo,
            "--json",
            "isArchived,nameWithOwner,isPrivate",
        ])
        .ok()?;
    let raw: Raw = serde_json::from_str(&json).ok()?;
    Some(RepoMetadata {
        name_with_owner: raw.name_with_owner,
        is_archived: raw.is_archived,
        is_private: raw.is_private,
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
