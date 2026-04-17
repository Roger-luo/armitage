use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::error::Result;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sub_issues: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoCache {
    pub repo: String,
    pub cached_at: String,
    pub open_count: usize,
    pub triaged_count: usize,
    #[serde(default)]
    pub issues: Vec<CachedIssue>,
}

// ---------------------------------------------------------------------------
// Build cache from DB
// ---------------------------------------------------------------------------

/// Query all open issues for a repo, left-joining with triage suggestions.
pub fn build_repo_cache(conn: &Connection, repo: &str) -> Result<RepoCache> {
    let now = chrono::Utc::now().to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT i.number, i.title, i.state, i.labels_json, i.sub_issues_count,
                ts.suggested_node, ts.confidence
         FROM issues i
         LEFT JOIN triage_suggestions ts ON ts.issue_id = i.id
         WHERE i.repo = ?1 AND LOWER(i.state) = 'open'
         ORDER BY i.number",
    )?;

    let mut issues = Vec::new();
    let mut triaged = 0usize;

    let rows = stmt.query_map(params![repo], |row| {
        let labels_json: String = row.get(3)?;
        let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let node: Option<String> = row.get(5)?;
        let confidence: Option<f64> = row.get(6)?;
        Ok(CachedIssue {
            number: row.get::<_, i64>(0)? as u64,
            title: row.get(1)?,
            state: row.get(2)?,
            labels,
            node,
            confidence,
            sub_issues: row.get::<_, i64>(4)? as u64,
        })
    })?;

    for row in rows {
        let issue = row?;
        if issue.node.is_some() {
            triaged += 1;
        }
        issues.push(issue);
    }

    let open_count = issues.len();

    Ok(RepoCache {
        repo: repo.to_string(),
        cached_at: now,
        open_count,
        triaged_count: triaged,
        issues,
    })
}

/// List distinct repos present in the issues table.
pub fn list_repos(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT repo FROM issues ORDER BY repo")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect::<std::result::Result<Vec<String>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

fn cache_dir(org_root: &Path) -> PathBuf {
    org_root.join(".armitage").join("triage").join("repo-cache")
}

fn repo_cache_path(org_root: &Path, repo: &str) -> PathBuf {
    // owner/repo -> owner--repo.toml
    let filename = repo.replace('/', "--");
    cache_dir(org_root).join(format!("{filename}.toml"))
}

pub fn write_repo_cache(org_root: &Path, cache: &RepoCache) -> Result<()> {
    let dir = cache_dir(org_root);
    std::fs::create_dir_all(&dir)?;
    let path = repo_cache_path(org_root, &cache.repo);
    let content = toml::to_string(cache)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn read_repo_cache(org_root: &Path, repo: &str) -> Result<RepoCache> {
    let path = repo_cache_path(org_root, repo);
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|source| crate::error::Error::TomlParse { path, source })
}

/// Rebuild and write cache files for all repos in the DB.
pub fn refresh_all(conn: &Connection, org_root: &Path) -> Result<usize> {
    let repos = list_repos(conn)?;
    for repo in &repos {
        let cache = build_repo_cache(conn, repo)?;
        write_repo_cache(org_root, &cache)?;
    }
    Ok(repos.len())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, StoredIssue, TriageSuggestion};
    use tempfile::TempDir;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        db::migrate(&conn).unwrap();
        conn
    }

    fn issue(repo: &str, number: u64, title: &str) -> StoredIssue {
        StoredIssue {
            id: 0,
            repo: repo.to_string(),
            number,
            title: title.to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            labels: vec!["bug".to_string()],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            fetched_at: "2026-04-01T00:00:00Z".to_string(),
            sub_issues_count: 0,
            author: String::new(),
            assignees: vec![],
            is_pr: false,
        }
    }

    #[test]
    fn build_cache_includes_triage_info() {
        let conn = setup_db();
        db::upsert_issue(&conn, &issue("owner/repo", 1, "Bug A")).unwrap();
        db::upsert_issue(&conn, &issue("owner/repo", 2, "Bug B")).unwrap();

        let stored = db::get_issues_by_repo(&conn, "owner/repo").unwrap();
        db::upsert_suggestion(
            &conn,
            &TriageSuggestion {
                id: 0,
                issue_id: stored[0].id,
                suggested_node: Some("project/auth".to_string()),
                suggested_labels: vec![],
                confidence: Some(0.9),
                reasoning: String::new(),
                llm_backend: "claude".to_string(),
                created_at: "2026-04-01T00:00:00Z".to_string(),
                is_tracking_issue: false,
                suggested_new_categories: vec![],
                is_stale: false,
            },
        )
        .unwrap();

        let cache = build_repo_cache(&conn, "owner/repo").unwrap();
        assert_eq!(cache.open_count, 2);
        assert_eq!(cache.triaged_count, 1);
        assert_eq!(cache.issues[0].node, Some("project/auth".to_string()));
        assert!((cache.issues[0].confidence.unwrap() - 0.9).abs() < f64::EPSILON);
        assert_eq!(cache.issues[1].node, None);
    }

    #[test]
    fn cache_excludes_closed_issues() {
        let conn = setup_db();
        db::upsert_issue(&conn, &issue("owner/repo", 1, "Open")).unwrap();
        let mut closed = issue("owner/repo", 2, "Closed");
        closed.state = "closed".to_string();
        db::upsert_issue(&conn, &closed).unwrap();

        let cache = build_repo_cache(&conn, "owner/repo").unwrap();
        assert_eq!(cache.open_count, 1);
        assert_eq!(cache.issues[0].number, 1);
    }

    #[test]
    fn write_and_read_cache_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = RepoCache {
            repo: "owner/repo".to_string(),
            cached_at: "2026-04-06T00:00:00Z".to_string(),
            open_count: 1,
            triaged_count: 0,
            issues: vec![CachedIssue {
                number: 42,
                title: "Fix bug".to_string(),
                state: "OPEN".to_string(),
                labels: vec!["bug".to_string()],
                node: None,
                confidence: None,
                sub_issues: 0,
            }],
        };

        write_repo_cache(tmp.path(), &cache).unwrap();
        let loaded = read_repo_cache(tmp.path(), "owner/repo").unwrap();

        assert_eq!(loaded.repo, "owner/repo");
        assert_eq!(loaded.issues.len(), 1);
        assert_eq!(loaded.issues[0].number, 42);
        assert_eq!(loaded.issues[0].title, "Fix bug");
    }

    #[test]
    fn refresh_all_writes_per_repo_files() {
        let tmp = TempDir::new().unwrap();
        let conn = setup_db();
        db::upsert_issue(&conn, &issue("owner/alpha", 1, "A")).unwrap();
        db::upsert_issue(&conn, &issue("owner/beta", 2, "B")).unwrap();

        let count = refresh_all(&conn, tmp.path()).unwrap();
        assert_eq!(count, 2);

        let alpha = read_repo_cache(tmp.path(), "owner/alpha").unwrap();
        assert_eq!(alpha.issues.len(), 1);
        assert_eq!(alpha.issues[0].title, "A");

        let beta = read_repo_cache(tmp.path(), "owner/beta").unwrap();
        assert_eq!(beta.issues.len(), 1);
        assert_eq!(beta.issues[0].title, "B");
    }

    #[test]
    fn list_repos_returns_distinct_sorted() {
        let conn = setup_db();
        db::upsert_issue(&conn, &issue("owner/beta", 1, "B")).unwrap();
        db::upsert_issue(&conn, &issue("owner/alpha", 1, "A")).unwrap();
        db::upsert_issue(&conn, &issue("owner/beta", 2, "B2")).unwrap();

        let repos = list_repos(&conn).unwrap();
        assert_eq!(repos, vec!["owner/alpha", "owner/beta"]);
    }

    #[test]
    fn cached_issue_skips_empty_fields_in_toml() {
        let issue = CachedIssue {
            number: 1,
            title: "Test".to_string(),
            state: "OPEN".to_string(),
            labels: vec![],
            node: None,
            confidence: None,
            sub_issues: 0,
        };
        let toml = toml::to_string(&issue).unwrap();
        assert!(!toml.contains("labels"));
        assert!(!toml.contains("node"));
        assert!(!toml.contains("confidence"));
        assert!(!toml.contains("sub_issues"));
    }
}
