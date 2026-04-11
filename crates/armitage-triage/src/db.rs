use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Schema versioning
// ---------------------------------------------------------------------------

/// Current schema version. Bump this when adding migrations.
const SCHEMA_VERSION: u32 = 7;

/// Full schema for a fresh database (always represents the latest version).
const SCHEMA_V6: &str = "
CREATE TABLE IF NOT EXISTS issues (
    id                INTEGER PRIMARY KEY,
    repo              TEXT NOT NULL,
    number            INTEGER NOT NULL,
    title             TEXT NOT NULL,
    body              TEXT NOT NULL DEFAULT '',
    state             TEXT NOT NULL,
    labels_json       TEXT NOT NULL DEFAULT '[]',
    updated_at        TEXT NOT NULL,
    fetched_at        TEXT NOT NULL,
    sub_issues_count  INTEGER NOT NULL DEFAULT 0,
    author            TEXT NOT NULL DEFAULT '',
    UNIQUE(repo, number)
);

CREATE INDEX IF NOT EXISTS idx_issues_repo ON issues(repo);
CREATE INDEX IF NOT EXISTS idx_issues_updated ON issues(updated_at);

CREATE TABLE IF NOT EXISTS triage_suggestions (
    id                INTEGER PRIMARY KEY,
    issue_id          INTEGER NOT NULL REFERENCES issues(id),
    suggested_node    TEXT,
    suggested_labels  TEXT NOT NULL DEFAULT '[]',
    confidence        REAL,
    reasoning         TEXT NOT NULL DEFAULT '',
    llm_backend       TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    is_tracking_issue          INTEGER NOT NULL DEFAULT 0,
    suggested_new_categories   TEXT NOT NULL DEFAULT '[]',
    is_stale                   INTEGER NOT NULL DEFAULT 0,
    UNIQUE(issue_id)
);

CREATE INDEX IF NOT EXISTS idx_triage_issue ON triage_suggestions(issue_id);

CREATE TABLE IF NOT EXISTS review_decisions (
    id            INTEGER PRIMARY KEY,
    suggestion_id INTEGER NOT NULL REFERENCES triage_suggestions(id),
    decision      TEXT NOT NULL,
    final_node    TEXT,
    final_labels  TEXT NOT NULL DEFAULT '[]',
    decided_at    TEXT NOT NULL,
    applied_at    TEXT,
    question      TEXT NOT NULL DEFAULT '',
    UNIQUE(suggestion_id)
);

CREATE INDEX IF NOT EXISTS idx_decisions_applied ON review_decisions(applied_at);

CREATE TABLE IF NOT EXISTS issue_project_items (
    id           INTEGER PRIMARY KEY,
    issue_id     INTEGER NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    project_url  TEXT NOT NULL,
    target_date  TEXT,
    start_date     TEXT,
    status       TEXT,
    fetched_at   TEXT NOT NULL,
    UNIQUE(issue_id, project_url)
);

CREATE INDEX IF NOT EXISTS idx_project_items_issue ON issue_project_items(issue_id);
";

/// Migrations from version N to N+1. Index 0 = v0→v1, index 1 = v1→v2, etc.
/// A migration of `None` means "breaking change — drop and recreate".
const MIGRATIONS: &[Option<&str>] = &[
    // v0 → v1: initial schema (no-op, v0 means fresh DB)
    Some(""),
    // v1 → v2: add sub_issues_count and is_tracking_issue
    Some(
        "ALTER TABLE issues ADD COLUMN sub_issues_count INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE triage_suggestions ADD COLUMN is_tracking_issue INTEGER NOT NULL DEFAULT 0;",
    ),
    // v2 → v3: add suggested_new_categories to triage_suggestions
    Some(
        "ALTER TABLE triage_suggestions ADD COLUMN suggested_new_categories TEXT NOT NULL DEFAULT '[]';",
    ),
    // v3 → v4: add is_stale to triage_suggestions
    Some("ALTER TABLE triage_suggestions ADD COLUMN is_stale INTEGER NOT NULL DEFAULT 0;"),
    // v4 → v5: add question to review_decisions (for "inquired" decision type)
    Some("ALTER TABLE review_decisions ADD COLUMN question TEXT NOT NULL DEFAULT '';"),
    // v5 → v6: add issue_project_items table for GitHub Projects v2 metadata
    Some(
        "CREATE TABLE IF NOT EXISTS issue_project_items (
            id           INTEGER PRIMARY KEY,
            issue_id     INTEGER NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
            project_url  TEXT NOT NULL,
            target_date  TEXT,
            start_date     TEXT,
            status       TEXT,
            fetched_at   TEXT NOT NULL,
            UNIQUE(issue_id, project_url)
        );
        CREATE INDEX IF NOT EXISTS idx_project_items_issue ON issue_project_items(issue_id);",
    ),
    // v6 → v7: add author column to issues
    Some("ALTER TABLE issues ADD COLUMN author TEXT NOT NULL DEFAULT '';"),
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct StoredIssue {
    pub id: i64,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<String>,
    pub updated_at: String,
    pub fetched_at: String,
    pub sub_issues_count: u64,
    pub author: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageSuggestion {
    pub id: i64,
    pub issue_id: i64,
    pub suggested_node: Option<String>,
    pub suggested_labels: Vec<String>,
    pub confidence: Option<f64>,
    pub reasoning: String,
    pub llm_backend: String,
    pub created_at: String,
    pub is_tracking_issue: bool,
    pub suggested_new_categories: Vec<String>,
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewDecision {
    pub id: i64,
    pub suggestion_id: i64,
    pub decision: String,
    pub final_node: Option<String>,
    pub final_labels: Vec<String>,
    pub decided_at: String,
    pub applied_at: Option<String>,
    /// Clarification question text (non-empty only for "inquired" decisions).
    pub question: String,
}

/// A row from `issue_project_items` — GitHub Projects v2 metadata for an issue.
#[derive(Debug, Clone, Serialize)]
pub struct IssueProjectItem {
    pub id: i64,
    pub issue_id: i64,
    pub project_url: String,
    pub target_date: Option<String>,
    pub start_date: Option<String>,
    pub status: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PipelineCounts {
    pub total_fetched: usize,
    pub untriaged: usize,
    pub pending_review: usize,
    pub approved_unapplied: usize,
    pub applied: usize,
    pub stale: usize,
}

// ---------------------------------------------------------------------------
// Suggestion filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionSort {
    #[default]
    Confidence,
    Node,
    Repo,
}

#[derive(Debug, Clone, Default)]
pub struct SuggestionFilters {
    pub issue_numbers: Vec<i64>,
    pub node_prefix: Option<String>,
    pub repo: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_confidence: Option<f64>,
    pub status: Option<SuggestionStatus>,
    pub tracking_only: bool,
    pub unclassified: bool,
    pub stale_only: bool,
    pub sort: SuggestionSort,
    pub limit: usize, // 0 = unlimited
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

pub fn open_db(org_root: &Path) -> Result<Connection> {
    let dir = org_root.join(".armitage").join("triage");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("triage.db");
    open_db_from_path(&path)
}

/// Open a DB connection from an explicit path (for second connections in WAL mode).
pub fn open_db_from_path(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current == SCHEMA_VERSION {
        return Ok(());
    }

    if current > SCHEMA_VERSION {
        return Err(Error::Other(format!(
            "database schema version ({current}) is newer than this CLI supports ({SCHEMA_VERSION}); \
             please update armitage or delete .armitage/triage/triage.db to re-sync"
        )));
    }

    if current == 0 {
        // Fresh database (or pre-versioning) — check if tables already exist
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='issues'",
            [],
            |row| row.get(0),
        )?;
        if table_exists {
            // Pre-versioning DB (v1 schema without user_version set).
            // Run incremental migrations from v1 onward.
            run_migrations(conn, 1)?;
        } else {
            // Truly fresh — create everything at latest version.
            conn.execute_batch(SCHEMA_V6)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
    } else {
        // Existing versioned DB — run incremental migrations.
        run_migrations(conn, current)?;
    }

    Ok(())
}

fn run_migrations(conn: &Connection, from_version: u32) -> Result<()> {
    for version in from_version..SCHEMA_VERSION {
        let idx = version as usize;
        match MIGRATIONS.get(idx) {
            Some(Some(sql)) => {
                if !sql.is_empty() {
                    // Run each statement individually so we can tolerate
                    // "duplicate column name" errors from partial upgrades
                    // (e.g. old code ran ALTER TABLE but didn't set user_version).
                    for stmt in sql.split(';') {
                        let stmt = stmt.trim();
                        if stmt.is_empty() {
                            continue;
                        }
                        match conn.execute_batch(stmt) {
                            Ok(()) => {}
                            Err(rusqlite::Error::SqliteFailure(_, Some(ref msg)))
                                if msg.contains("duplicate column name") => {}
                            Err(e) => return Err(e.into()),
                        }
                    }
                }
            }
            Some(None) => {
                // Breaking migration — drop all tables and recreate.
                eprintln!(
                    "Schema upgrade v{version} → v{} requires a full re-sync. Resetting database...",
                    version + 1
                );
                conn.execute_batch(
                    "DROP TABLE IF EXISTS review_decisions;
                     DROP TABLE IF EXISTS triage_suggestions;
                     DROP TABLE IF EXISTS issues;",
                )?;
                conn.execute_batch(SCHEMA_V6)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                return Ok(());
            }
            None => {
                return Err(Error::Other(format!(
                    "missing migration for v{version} → v{}",
                    version + 1
                )));
            }
        }
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue CRUD
// ---------------------------------------------------------------------------

pub fn upsert_issue(conn: &Connection, issue: &StoredIssue) -> Result<i64> {
    let labels_json = serde_json::to_string(&issue.labels).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO issues (repo, number, title, body, state, labels_json, updated_at, fetched_at, sub_issues_count, author)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(repo, number) DO UPDATE SET
            title = excluded.title,
            body = excluded.body,
            state = excluded.state,
            labels_json = excluded.labels_json,
            updated_at = excluded.updated_at,
            fetched_at = excluded.fetched_at,
            sub_issues_count = excluded.sub_issues_count,
            author = excluded.author",
        params![
            issue.repo,
            issue.number as i64,
            issue.title,
            issue.body,
            issue.state,
            labels_json,
            issue.updated_at,
            issue.fetched_at,
            issue.sub_issues_count as i64,
            issue.author,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_untriaged_issues(conn: &Connection) -> Result<Vec<StoredIssue>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json, i.updated_at, i.fetched_at, i.sub_issues_count, i.author
         FROM issues i
         LEFT JOIN triage_suggestions ts ON ts.issue_id = i.id
         WHERE ts.id IS NULL AND LOWER(i.state) = 'open'
         ORDER BY i.repo, i.number",
    )?;
    let rows = stmt.query_map([], row_to_issue)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_untriaged_issues_by_repo(conn: &Connection, repo: &str) -> Result<Vec<StoredIssue>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json, i.updated_at, i.fetched_at, i.sub_issues_count, i.author
         FROM issues i
         LEFT JOIN triage_suggestions ts ON ts.issue_id = i.id
         WHERE ts.id IS NULL AND LOWER(i.state) = 'open' AND i.repo = ?1
         ORDER BY i.number",
    )?;
    let rows = stmt.query_map(params![repo], row_to_issue)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_issues_by_repo(conn: &Connection, repo: &str) -> Result<Vec<StoredIssue>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo, number, title, body, state, labels_json, updated_at, fetched_at, sub_issues_count, author
         FROM issues WHERE repo = ?1 ORDER BY number",
    )?;
    let rows = stmt.query_map(params![repo], row_to_issue)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_latest_updated_at(conn: &Connection, repo: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT MAX(updated_at) FROM issues WHERE repo = ?1")?;
    let result: Option<String> = stmt.query_row(params![repo], |row| row.get(0))?;
    Ok(result)
}

fn row_to_issue(row: &rusqlite::Row) -> rusqlite::Result<StoredIssue> {
    let labels_json: String = row.get(6)?;
    let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
    Ok(StoredIssue {
        id: row.get(0)?,
        repo: row.get(1)?,
        number: row.get::<_, i64>(2)? as u64,
        title: row.get(3)?,
        body: row.get(4)?,
        state: row.get(5)?,
        labels,
        updated_at: row.get(7)?,
        fetched_at: row.get(8)?,
        sub_issues_count: row.get::<_, i64>(9)? as u64,
        author: row.get(10)?,
    })
}

// ---------------------------------------------------------------------------
// Project Item CRUD
// ---------------------------------------------------------------------------

/// Insert or update a project-item row for an issue.
pub fn upsert_project_item(conn: &Connection, item: &IssueProjectItem) -> Result<i64> {
    conn.execute(
        "INSERT INTO issue_project_items (issue_id, project_url, target_date, start_date, status, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(issue_id, project_url) DO UPDATE SET
            target_date = excluded.target_date,
            start_date    = excluded.start_date,
            status      = excluded.status,
            fetched_at  = excluded.fetched_at",
        params![
            item.issue_id,
            item.project_url,
            item.target_date,
            item.start_date,
            item.status,
            item.fetched_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Look up an issue's internal DB id by repo and number.
/// Returns `None` if the issue is not in the database.
pub fn lookup_issue_id(conn: &Connection, repo: &str, number: u64) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM issues WHERE repo = ?1 AND number = ?2")?;
    let result = stmt
        .query_row(params![repo, number as i64], |row| row.get(0))
        .optional()?;
    Ok(result)
}

/// Get project metadata for an issue by repo and number.
pub fn get_project_items_for_issue(
    conn: &Connection,
    repo: &str,
    number: u64,
) -> Result<Vec<IssueProjectItem>> {
    let Some(issue_id) = lookup_issue_id(conn, repo, number)? else {
        return Ok(vec![]);
    };
    let mut stmt = conn.prepare(
        "SELECT id, issue_id, project_url, target_date, start_date, status, fetched_at
         FROM issue_project_items WHERE issue_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![issue_id], |row| {
            Ok(IssueProjectItem {
                id: row.get(0)?,
                issue_id: row.get(1)?,
                project_url: row.get(2)?,
                target_date: row.get(3)?,
                start_date: row.get(4)?,
                status: row.get(5)?,
                fetched_at: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Triage Suggestion CRUD
// ---------------------------------------------------------------------------

pub fn upsert_suggestion(conn: &Connection, s: &TriageSuggestion) -> Result<i64> {
    let labels_json =
        serde_json::to_string(&s.suggested_labels).unwrap_or_else(|_| "[]".to_string());
    let categories_json =
        serde_json::to_string(&s.suggested_new_categories).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO triage_suggestions (issue_id, suggested_node, suggested_labels, confidence, reasoning, llm_backend, created_at, is_tracking_issue, suggested_new_categories, is_stale)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(issue_id) DO UPDATE SET
            suggested_node = excluded.suggested_node,
            suggested_labels = excluded.suggested_labels,
            confidence = excluded.confidence,
            reasoning = excluded.reasoning,
            llm_backend = excluded.llm_backend,
            created_at = excluded.created_at,
            is_tracking_issue = excluded.is_tracking_issue,
            suggested_new_categories = excluded.suggested_new_categories,
            is_stale = excluded.is_stale",
        params![
            s.issue_id,
            s.suggested_node,
            labels_json,
            s.confidence,
            s.reasoning,
            s.llm_backend,
            s.created_at,
            i64::from(s.is_tracking_issue),
            categories_json,
            i64::from(s.is_stale),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_pending_suggestions(conn: &Connection) -> Result<Vec<(StoredIssue, TriageSuggestion)>> {
    get_pending_suggestions_filtered(conn, None, None)
}

pub fn get_pending_suggestions_filtered(
    conn: &Connection,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
) -> Result<Vec<(StoredIssue, TriageSuggestion)>> {
    let mut sql = String::from(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json, i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels, ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at, ts.is_tracking_issue, ts.suggested_new_categories, ts.is_stale
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE rd.id IS NULL",
    );
    if min_confidence.is_some() {
        sql.push_str(" AND COALESCE(ts.confidence, 0.0) >= ?1");
    }
    if max_confidence.is_some() {
        let param = if min_confidence.is_some() { "?2" } else { "?1" };
        let _ = write!(sql, " AND COALESCE(ts.confidence, 0.0) <= {param}");
    }
    sql.push_str(" ORDER BY ts.confidence DESC, i.repo, i.number");

    let mut stmt = conn.prepare(&sql)?;

    let row_mapper = |row: &rusqlite::Row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(14)?;
        let suggested_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let suggestion = TriageSuggestion {
            id: row.get(11)?,
            issue_id: row.get(12)?,
            suggested_node: row.get(13)?,
            suggested_labels,
            confidence: row.get(15)?,
            reasoning: row.get(16)?,
            llm_backend: row.get(17)?,
            created_at: row.get(18)?,
            is_tracking_issue: row.get::<_, i64>(19)? != 0,
            suggested_new_categories: {
                let json: String = row.get(20)?;
                serde_json::from_str(&json).unwrap_or_default()
            },
            is_stale: row.get::<_, i64>(21)? != 0,
        };
        Ok((issue, suggestion))
    };

    let rows = match (min_confidence, max_confidence) {
        (Some(min), Some(max)) => stmt.query_map(params![min, max], row_mapper)?,
        (Some(min), None) => stmt.query_map(params![min], row_mapper)?,
        (None, Some(max)) => stmt.query_map(params![max], row_mapper)?,
        (None, None) => stmt.query_map([], row_mapper)?,
    };
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Delete suggestions with confidence below the threshold (and their review decisions),
/// making those issues "untriaged" again. Returns the number of suggestions deleted.
pub fn delete_suggestions_below_confidence(conn: &Connection, threshold: f64) -> Result<usize> {
    // First delete any review decisions referencing these suggestions
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT id FROM triage_suggestions WHERE COALESCE(confidence, 0.0) < ?1
        )",
        params![threshold],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions WHERE COALESCE(confidence, 0.0) < ?1",
        params![threshold],
    )?;
    Ok(deleted)
}

/// Delete suggestions where suggested_node matches the given path or is a child of it
/// (and their review decisions), making those issues untriaged. Returns count deleted.
pub fn delete_suggestions_by_node_prefix(conn: &Connection, prefix: &str) -> Result<usize> {
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT id FROM triage_suggestions
            WHERE suggested_node = ?1 OR suggested_node LIKE ?2
        )",
        params![prefix, format!("{prefix}/%")],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions
         WHERE suggested_node = ?1 OR suggested_node LIKE ?2",
        params![prefix, format!("{prefix}/%")],
    )?;
    Ok(deleted)
}

/// Reassign suggestions from one node (and its descendants) to another node.
/// Also updates any review decisions that reference the old node.
/// Returns the number of suggestions reassigned.
pub fn reassign_suggestions(conn: &Connection, from_prefix: &str, to_node: &str) -> Result<usize> {
    // Update review decisions that point to the old node
    conn.execute(
        "UPDATE review_decisions SET final_node = ?3
         WHERE suggestion_id IN (
             SELECT id FROM triage_suggestions
             WHERE suggested_node = ?1 OR suggested_node LIKE ?2
         ) AND (final_node = ?1 OR final_node LIKE ?2)",
        params![from_prefix, format!("{from_prefix}/%"), to_node],
    )?;
    // Update the suggestions themselves
    let updated = conn.execute(
        "UPDATE triage_suggestions SET suggested_node = ?3
         WHERE suggested_node = ?1 OR suggested_node LIKE ?2",
        params![from_prefix, format!("{from_prefix}/%"), to_node],
    )?;
    Ok(updated)
}

/// Delete suggestions for reclassification: all null-node issues plus issues that
/// voted for the given category. Also deletes associated review decisions.
pub fn delete_suggestions_for_reclassify(conn: &Connection, category: &str) -> Result<usize> {
    let category_pattern = format!("%\"{category}\"%");
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT id FROM triage_suggestions
            WHERE suggested_node IS NULL
               OR suggested_new_categories LIKE ?1
        )",
        params![category_pattern],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions
         WHERE suggested_node IS NULL
            OR suggested_new_categories LIKE ?1",
        params![category_pattern],
    )?;
    Ok(deleted)
}

/// Delete a single issue's suggestion (and its review decision) by repo and issue number,
/// making the issue "untriaged" again. Returns the number of suggestions deleted (0 or 1).
pub fn delete_suggestion_by_issue(conn: &Connection, repo: &str, number: u64) -> Result<usize> {
    let number = number as i64;
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT ts.id FROM triage_suggestions ts
            JOIN issues i ON i.id = ts.issue_id
            WHERE i.repo = ?1 AND i.number = ?2
        )",
        params![repo, number],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions WHERE issue_id IN (
            SELECT id FROM issues WHERE repo = ?1 AND number = ?2
        )",
        params![repo, number],
    )?;
    Ok(deleted)
}

/// Delete ALL suggestions and their review decisions. Returns count deleted.
pub fn delete_all_suggestions(conn: &Connection) -> Result<usize> {
    conn.execute("DELETE FROM review_decisions", [])?;
    let deleted = conn.execute("DELETE FROM triage_suggestions", [])?;
    Ok(deleted)
}

/// Delete suggestions that have not been approved or modified — i.e. unreviewed
/// suggestions and rejected ones. Their review decisions are also removed.
/// Returns the number of suggestions deleted.
pub fn delete_unreviewed_suggestions(conn: &Connection) -> Result<usize> {
    // Delete review decisions for rejected suggestions first
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT ts.id FROM triage_suggestions ts
            LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
            WHERE rd.id IS NULL OR rd.decision = 'rejected'
        )",
        [],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions WHERE id NOT IN (
            SELECT suggestion_id FROM review_decisions
            WHERE decision IN ('approved', 'modified')
        )",
        [],
    )?;
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Review Decision CRUD
// ---------------------------------------------------------------------------

pub fn insert_decision(conn: &Connection, d: &ReviewDecision) -> Result<()> {
    let labels_json = serde_json::to_string(&d.final_labels).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO review_decisions (suggestion_id, decision, final_node, final_labels, decided_at, applied_at, question)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(suggestion_id) DO UPDATE SET
            decision = excluded.decision,
            final_node = excluded.final_node,
            final_labels = excluded.final_labels,
            decided_at = excluded.decided_at,
            applied_at = excluded.applied_at,
            question = excluded.question",
        params![
            d.suggestion_id,
            d.decision,
            d.final_node,
            labels_json,
            d.decided_at,
            d.applied_at,
            d.question,
        ],
    )?;
    Ok(())
}

/// Delete a review decision by its suggestion_id, making the suggestion "pending" again.
pub fn delete_decision_by_suggestion_id(conn: &Connection, suggestion_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id = ?1",
        params![suggestion_id],
    )?;
    Ok(())
}

/// Look up a single issue's suggestion and optional decision by repo and issue number.
/// Returns `None` if the issue has no suggestion.
pub fn get_suggestion_by_issue(
    conn: &Connection,
    repo: &str,
    number: u64,
) -> Result<Option<(StoredIssue, TriageSuggestion, Option<ReviewDecision>)>> {
    let number = number as i64;
    let mut stmt = conn.prepare(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels,
                ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at,
                ts.is_tracking_issue, ts.suggested_new_categories, ts.is_stale,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels,
                rd.decided_at, rd.applied_at, rd.question
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE i.repo = ?1 AND i.number = ?2",
    )?;

    let mut rows = stmt.query_map(params![repo, number], |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(14)?;
        let suggested_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let suggestion = TriageSuggestion {
            id: row.get(11)?,
            issue_id: row.get(12)?,
            suggested_node: row.get(13)?,
            suggested_labels,
            confidence: row.get(15)?,
            reasoning: row.get(16)?,
            llm_backend: row.get(17)?,
            created_at: row.get(18)?,
            is_tracking_issue: row.get::<_, i64>(19)? != 0,
            suggested_new_categories: {
                let json: String = row.get(20)?;
                serde_json::from_str(&json).unwrap_or_default()
            },
            is_stale: row.get::<_, i64>(21)? != 0,
        };

        let decision = if row.get::<_, Option<i64>>(22)?.is_some() {
            let final_labels_json: String = row.get(26)?;
            Some(ReviewDecision {
                id: row.get(22)?,
                suggestion_id: row.get(23)?,
                decision: row.get(24)?,
                final_node: row.get(25)?,
                final_labels: serde_json::from_str(&final_labels_json).unwrap_or_default(),
                decided_at: row.get(27)?,
                applied_at: row.get(28)?,
                question: row.get(29)?,
            })
        } else {
            None
        };

        Ok((issue, suggestion, decision))
    })?;

    match rows.next() {
        Some(Ok(row)) => Ok(Some(row)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_unapplied_decisions(conn: &Connection) -> Result<Vec<(StoredIssue, ReviewDecision)>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json, i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels, rd.decided_at, rd.applied_at, rd.question
         FROM review_decisions rd
         JOIN triage_suggestions ts ON ts.id = rd.suggestion_id
         JOIN issues i ON i.id = ts.issue_id
         WHERE rd.applied_at IS NULL AND rd.decision IN ('approved', 'modified', 'inquired', 'stale')
         ORDER BY i.repo, i.number",
    )?;
    let rows = stmt.query_map([], |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(15)?;
        let final_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let decision = ReviewDecision {
            id: row.get(11)?,
            suggestion_id: row.get(12)?,
            decision: row.get(13)?,
            final_node: row.get(14)?,
            final_labels,
            decided_at: row.get(16)?,
            applied_at: row.get(17)?,
            question: row.get(18)?,
        };
        Ok((issue, decision))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn mark_applied(conn: &Connection, decision_id: i64, applied_at: &str) -> Result<()> {
    conn.execute(
        "UPDATE review_decisions SET applied_at = ?1 WHERE id = ?2",
        params![applied_at, decision_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pipeline counts
// ---------------------------------------------------------------------------

pub fn get_pipeline_counts(conn: &Connection) -> Result<PipelineCounts> {
    let total_fetched: i64 = conn.query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
    let untriaged: i64 = conn.query_row(
        "SELECT COUNT(*) FROM issues i
         LEFT JOIN triage_suggestions ts ON ts.issue_id = i.id
         WHERE ts.id IS NULL AND LOWER(i.state) = 'open'",
        [],
        |r| r.get(0),
    )?;
    let pending_review: i64 = conn.query_row(
        "SELECT COUNT(*) FROM triage_suggestions ts
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE rd.id IS NULL",
        [],
        |r| r.get(0),
    )?;
    let approved_unapplied: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_decisions
         WHERE applied_at IS NULL AND decision IN ('approved', 'modified')",
        [],
        |r| r.get(0),
    )?;
    let applied: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_decisions WHERE applied_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let stale: i64 = conn.query_row(
        "SELECT COUNT(*) FROM triage_suggestions WHERE is_stale = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(PipelineCounts {
        total_fetched: total_fetched as usize,
        untriaged: untriaged as usize,
        pending_review: pending_review as usize,
        approved_unapplied: approved_unapplied as usize,
        applied: applied as usize,
        stale: stale as usize,
    })
}

// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ConfidenceBand {
    pub label: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeBreakdown {
    pub node: Option<String>,
    pub count: usize,
    pub avg_confidence: f64,
    pub min_confidence: f64,
    pub max_confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryVote {
    pub category: String,
    pub vote_count: usize,
    pub issue_refs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Summary queries
// ---------------------------------------------------------------------------

pub fn get_confidence_distribution(
    conn: &Connection,
    repo: Option<&str>,
) -> Result<Vec<ConfidenceBand>> {
    let bands: &[(&str, f64, f64)] = &[
        ("<0.5", 0.0, 0.5),
        ("0.5-0.7", 0.5, 0.7),
        ("0.7-0.8", 0.7, 0.8),
        ("0.8-0.9", 0.8, 0.9),
        ("0.9-1.0", 0.9, 1.01), // slightly above 1.0 to include exactly 1.0
    ];

    // Get total count first for percentage calculation
    let total: i64 = if let Some(repo) = repo {
        conn.query_row(
            "SELECT COUNT(*) FROM triage_suggestions ts
             JOIN issues i ON i.id = ts.issue_id
             WHERE i.repo = ?1",
            params![repo],
            |r| r.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM triage_suggestions", [], |r| r.get(0))?
    };

    let mut result = Vec::with_capacity(5);
    for &(label, lo, hi) in bands {
        let count: i64 = if let Some(repo) = repo {
            conn.query_row(
                "SELECT COUNT(*) FROM triage_suggestions ts
                 JOIN issues i ON i.id = ts.issue_id
                 WHERE i.repo = ?1
                   AND COALESCE(ts.confidence, 0.0) >= ?2
                   AND COALESCE(ts.confidence, 0.0) < ?3",
                params![repo, lo, hi],
                |r| r.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM triage_suggestions
                 WHERE COALESCE(confidence, 0.0) >= ?1
                   AND COALESCE(confidence, 0.0) < ?2",
                params![lo, hi],
                |r| r.get(0),
            )?
        };
        let percentage = if total > 0 {
            (count as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        result.push(ConfidenceBand {
            label: label.to_string(),
            count: count as usize,
            percentage,
        });
    }
    Ok(result)
}

pub fn get_node_breakdown(conn: &Connection, repo: Option<&str>) -> Result<Vec<NodeBreakdown>> {
    let sql = if repo.is_some() {
        "SELECT ts.suggested_node, COUNT(*) as cnt,
                AVG(COALESCE(ts.confidence, 0.0)),
                MIN(COALESCE(ts.confidence, 0.0)),
                MAX(COALESCE(ts.confidence, 0.0))
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE i.repo = ?1
         GROUP BY ts.suggested_node
         ORDER BY cnt DESC"
    } else {
        "SELECT suggested_node, COUNT(*) as cnt,
                AVG(COALESCE(confidence, 0.0)),
                MIN(COALESCE(confidence, 0.0)),
                MAX(COALESCE(confidence, 0.0))
         FROM triage_suggestions
         GROUP BY suggested_node
         ORDER BY cnt DESC"
    };

    let mut stmt = conn.prepare(sql)?;
    let row_mapper = |row: &rusqlite::Row| {
        Ok(NodeBreakdown {
            node: row.get(0)?,
            count: row.get::<_, i64>(1)? as usize,
            avg_confidence: row.get(2)?,
            min_confidence: row.get(3)?,
            max_confidence: row.get(4)?,
        })
    };

    let rows: Vec<NodeBreakdown> = if let Some(repo) = repo {
        stmt.query_map(params![repo], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

pub fn get_new_category_votes(conn: &Connection, repo: Option<&str>) -> Result<Vec<CategoryVote>> {
    let sql = if repo.is_some() {
        "SELECT ts.suggested_new_categories, i.repo, i.number
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE ts.suggested_new_categories != '[]' AND i.repo = ?1"
    } else {
        "SELECT ts.suggested_new_categories, i.repo, i.number
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE ts.suggested_new_categories != '[]'"
    };

    let mut stmt = conn.prepare(sql)?;
    let row_mapper = |row: &rusqlite::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?));

    let rows: Vec<(String, String, i64)> = if let Some(repo) = repo {
        stmt.query_map(params![repo], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (json, repo_name, number) in &rows {
        let categories: Vec<String> = serde_json::from_str(json).unwrap_or_default();
        let issue_ref = format!("{repo_name}#{number}");
        for cat in categories {
            map.entry(cat).or_default().push(issue_ref.clone());
        }
    }

    let mut votes: Vec<CategoryVote> = map
        .into_iter()
        .map(|(category, issue_refs)| CategoryVote {
            vote_count: issue_refs.len(),
            category,
            issue_refs,
        })
        .collect();
    votes.sort_by_key(|v| std::cmp::Reverse(v.vote_count));
    Ok(votes)
}

// ---------------------------------------------------------------------------
// Decision filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct DecisionFilters {
    pub status: Option<String>, // approved, rejected, modified, applied
    pub unapplied: bool,        // shorthand: approved+modified, applied_at IS NULL
    pub node_prefix: Option<String>,
    pub repo: Option<String>,
    pub limit: usize, // 0 = unlimited
}

pub fn get_decisions_filtered(
    conn: &Connection,
    f: &DecisionFilters,
) -> Result<Vec<(StoredIssue, ReviewDecision)>> {
    let mut sql = String::from(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels,
                rd.decided_at, rd.applied_at, rd.question
         FROM review_decisions rd
         JOIN triage_suggestions ts ON ts.id = rd.suggestion_id
         JOIN issues i ON i.id = ts.issue_id
         WHERE 1=1",
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if f.unapplied {
        sql.push_str(" AND rd.decision IN ('approved', 'modified') AND rd.applied_at IS NULL");
    } else if let Some(ref status) = f.status {
        if status == "applied" {
            sql.push_str(" AND rd.applied_at IS NOT NULL");
        } else {
            let _ = write!(sql, " AND rd.decision = ?{param_idx}");
            params.push(Box::new(status.clone()));
            param_idx += 1;
        }
    }

    if let Some(ref prefix) = f.node_prefix {
        let p = param_idx;
        let p2 = param_idx + 1;
        let _ = write!(
            sql,
            " AND (rd.final_node = ?{p} OR rd.final_node LIKE ?{p2})"
        );
        params.push(Box::new(prefix.clone()));
        params.push(Box::new(format!("{prefix}/%")));
        param_idx += 2;
    }

    if let Some(ref repo) = f.repo {
        let _ = write!(sql, " AND i.repo = ?{param_idx}");
        params.push(Box::new(repo.clone()));
        param_idx += 1;
    }

    sql.push_str(" ORDER BY rd.decided_at DESC");

    if f.limit > 0 {
        let _ = write!(sql, " LIMIT {}", f.limit);
    }

    let _ = param_idx;
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(std::convert::AsRef::as_ref).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(15)?;
        let final_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let decision = ReviewDecision {
            id: row.get(11)?,
            suggestion_id: row.get(12)?,
            decision: row.get(13)?,
            final_node: row.get(14)?,
            final_labels,
            decided_at: row.get(16)?,
            applied_at: row.get(17)?,
            question: row.get(18)?,
        };
        Ok((issue, decision))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Get decisions with the original suggestion's suggested_node, for example export.
pub fn get_decisions_with_original(
    conn: &Connection,
    statuses: &[&str],
    limit: usize,
) -> Result<Vec<(StoredIssue, TriageSuggestion, ReviewDecision)>> {
    let placeholders: Vec<String> = (1..=statuses.len()).map(|i| format!("?{i}")).collect();
    let mut sql = format!(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels,
                ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at,
                ts.is_tracking_issue, ts.suggested_new_categories, ts.is_stale,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels,
                rd.decided_at, rd.applied_at, rd.question
         FROM review_decisions rd
         JOIN triage_suggestions ts ON ts.id = rd.suggestion_id
         JOIN issues i ON i.id = ts.issue_id
         WHERE rd.decision IN ({})
         ORDER BY rd.decided_at DESC",
        placeholders.join(", ")
    );

    if limit > 0 {
        let _ = write!(sql, " LIMIT {limit}");
    }

    let params: Vec<&dyn rusqlite::types::ToSql> = statuses
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        let issue = row_to_issue(row)?;

        let sug_labels_json: String = row.get(14)?;
        let sug_labels: Vec<String> = serde_json::from_str(&sug_labels_json).unwrap_or_default();
        let sug_cats_json: String = row.get(20)?;
        let sug_cats: Vec<String> = serde_json::from_str(&sug_cats_json).unwrap_or_default();
        let suggestion = TriageSuggestion {
            id: row.get(11)?,
            issue_id: row.get(12)?,
            suggested_node: row.get(13)?,
            suggested_labels: sug_labels,
            confidence: row.get(15)?,
            reasoning: row.get(16)?,
            llm_backend: row.get(17)?,
            created_at: row.get(18)?,
            is_tracking_issue: row.get::<_, i32>(19)? != 0,
            suggested_new_categories: sug_cats,
            is_stale: row.get::<_, i32>(21)? != 0,
        };

        let dec_labels_json: String = row.get(26)?;
        let dec_labels: Vec<String> = serde_json::from_str(&dec_labels_json).unwrap_or_default();
        let decision = ReviewDecision {
            id: row.get(22)?,
            suggestion_id: row.get(23)?,
            decision: row.get(24)?,
            final_node: row.get(25)?,
            final_labels: dec_labels,
            decided_at: row.get(27)?,
            applied_at: row.get(28)?,
            question: row.get(29)?,
        };

        Ok((issue, suggestion, decision))
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Filtered suggestion query
// ---------------------------------------------------------------------------

pub fn get_suggestions_filtered(
    conn: &Connection,
    f: &SuggestionFilters,
) -> Result<Vec<(StoredIssue, TriageSuggestion)>> {
    let mut sql = String::from(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count, i.author,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels,
                ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at,
                ts.is_tracking_issue, ts.suggested_new_categories, ts.is_stale,
                rd.id AS rd_id, rd.decision, rd.applied_at
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE 1=1",
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if !f.issue_numbers.is_empty() {
        let placeholders: Vec<String> = f
            .issue_numbers
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", param_idx + i))
            .collect();
        let _ = write!(sql, " AND i.number IN ({})", placeholders.join(","));
        for n in &f.issue_numbers {
            params.push(Box::new(*n));
        }
        param_idx += f.issue_numbers.len();
    }

    if let Some(ref prefix) = f.node_prefix {
        let p = param_idx;
        let p2 = param_idx + 1;
        let _ = write!(
            sql,
            " AND (ts.suggested_node = ?{p} OR ts.suggested_node LIKE ?{p2})"
        );
        params.push(Box::new(prefix.clone()));
        params.push(Box::new(format!("{prefix}/%")));
        param_idx += 2;
    }

    if let Some(ref repo) = f.repo {
        let _ = write!(sql, " AND i.repo = ?{param_idx}");
        params.push(Box::new(repo.clone()));
        param_idx += 1;
    }

    if let Some(min) = f.min_confidence {
        let _ = write!(sql, " AND COALESCE(ts.confidence, 0.0) >= ?{param_idx}");
        params.push(Box::new(min));
        param_idx += 1;
    }

    if let Some(max) = f.max_confidence {
        let _ = write!(sql, " AND COALESCE(ts.confidence, 0.0) <= ?{param_idx}");
        params.push(Box::new(max));
        param_idx += 1;
    }

    match f.status {
        Some(SuggestionStatus::Pending) => sql.push_str(" AND rd.id IS NULL"),
        Some(SuggestionStatus::Approved) => {
            sql.push_str(" AND rd.decision IN ('approved', 'modified') AND rd.applied_at IS NULL");
        }
        Some(SuggestionStatus::Rejected) => sql.push_str(" AND rd.decision = 'rejected'"),
        Some(SuggestionStatus::Applied) => sql.push_str(" AND rd.applied_at IS NOT NULL"),
        None => {}
    }

    if f.tracking_only {
        sql.push_str(" AND ts.is_tracking_issue = 1");
    }
    if f.unclassified {
        sql.push_str(" AND ts.suggested_node IS NULL");
    }
    if f.stale_only {
        sql.push_str(" AND ts.is_stale = 1");
    }

    let order = match f.sort {
        SuggestionSort::Confidence => "ts.confidence DESC, i.repo, i.number",
        SuggestionSort::Node => "ts.suggested_node, i.repo, i.number",
        SuggestionSort::Repo => "i.repo, i.number",
    };
    let _ = write!(sql, " ORDER BY {order}");

    if f.limit > 0 {
        let _ = write!(sql, " LIMIT {}", f.limit);
    }

    let _ = param_idx;
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(std::convert::AsRef::as_ref).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(14)?;
        let suggested_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let suggestion = TriageSuggestion {
            id: row.get(11)?,
            issue_id: row.get(12)?,
            suggested_node: row.get(13)?,
            suggested_labels,
            confidence: row.get(15)?,
            reasoning: row.get(16)?,
            llm_backend: row.get(17)?,
            created_at: row.get(18)?,
            is_tracking_issue: row.get::<_, i64>(19)? != 0,
            suggested_new_categories: {
                let json: String = row.get(20)?;
                serde_json::from_str(&json).unwrap_or_default()
            },
            is_stale: row.get::<_, i64>(21)? != 0,
        };
        Ok((issue, suggestion))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn sample_issue(repo: &str, number: u64) -> StoredIssue {
        StoredIssue {
            id: 0,
            repo: repo.to_string(),
            number,
            title: format!("Issue #{number}"),
            body: "Some body".to_string(),
            state: "OPEN".to_string(),
            labels: vec!["bug".to_string()],
            updated_at: "2026-01-15T10:00:00Z".to_string(),
            fetched_at: "2026-04-01T00:00:00Z".to_string(),
            sub_issues_count: 0,
            author: String::new(),
        }
    }

    fn sample_suggestion(issue_id: i64, node: &str, confidence: f64) -> TriageSuggestion {
        TriageSuggestion {
            id: 0,
            issue_id,
            suggested_node: Some(node.to_string()),
            is_stale: false,
            suggested_labels: vec![],
            confidence: Some(confidence),
            reasoning: String::new(),
            llm_backend: "claude".to_string(),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            is_tracking_issue: false,
            suggested_new_categories: vec![],
        }
    }

    #[test]
    fn upsert_and_query_issues() {
        let conn = memory_db();
        let issue = sample_issue("owner/repo", 1);
        let id = upsert_issue(&conn, &issue).unwrap();
        assert!(id > 0);

        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "Issue #1");
        assert_eq!(issues[0].labels, vec!["bug"]);
    }

    #[test]
    fn upsert_replaces_on_conflict() {
        let conn = memory_db();
        let mut issue = sample_issue("owner/repo", 1);
        upsert_issue(&conn, &issue).unwrap();

        issue.title = "Updated title".to_string();
        issue.labels = vec!["enhancement".to_string()];
        upsert_issue(&conn, &issue).unwrap();

        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "Updated title");
        assert_eq!(issues[0].labels, vec!["enhancement"]);
    }

    #[test]
    fn untriaged_issues() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();

        let untriaged = get_untriaged_issues(&conn).unwrap();
        assert_eq!(untriaged.len(), 2);

        // Add a suggestion for issue 1
        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        upsert_suggestion(
            &conn,
            &TriageSuggestion {
                id: 0,
                issue_id: issues[0].id,
                suggested_node: Some("project/auth".to_string()),
                suggested_labels: vec!["team:alpha".to_string()],
                confidence: Some(0.9),
                reasoning: "Auth related".to_string(),
                llm_backend: "claude".to_string(),
                created_at: "2026-04-01T00:00:00Z".to_string(),
                is_tracking_issue: false,
                suggested_new_categories: vec![],
                is_stale: false,
            },
        )
        .unwrap();

        let untriaged = get_untriaged_issues(&conn).unwrap();
        assert_eq!(untriaged.len(), 1);
        assert_eq!(untriaged[0].number, 2);
    }

    #[test]
    fn untriaged_excludes_closed_issues() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();

        let mut closed = sample_issue("owner/repo", 2);
        closed.state = "closed".to_string();
        upsert_issue(&conn, &closed).unwrap();

        let mut closed_upper = sample_issue("owner/repo", 3);
        closed_upper.state = "CLOSED".to_string();
        upsert_issue(&conn, &closed_upper).unwrap();

        let untriaged = get_untriaged_issues(&conn).unwrap();
        assert_eq!(untriaged.len(), 1);
        assert_eq!(untriaged[0].number, 1);

        let untriaged_repo = get_untriaged_issues_by_repo(&conn, "owner/repo").unwrap();
        assert_eq!(untriaged_repo.len(), 1);
        assert_eq!(untriaged_repo[0].number, 1);
    }

    #[test]
    fn pending_suggestions_and_decisions() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();

        let sug_id = upsert_suggestion(
            &conn,
            &TriageSuggestion {
                id: 0,
                issue_id: issues[0].id,
                suggested_node: Some("project/auth".to_string()),
                suggested_labels: vec!["team:alpha".to_string()],
                confidence: Some(0.85),
                reasoning: "Auth related".to_string(),
                llm_backend: "claude".to_string(),
                created_at: "2026-04-01T00:00:00Z".to_string(),
                is_tracking_issue: false,
                suggested_new_categories: vec![],
                is_stale: false,
            },
        )
        .unwrap();

        let pending = get_pending_suggestions(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].1.suggested_node,
            Some("project/auth".to_string())
        );

        // Approve it
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: sug_id,
                decision: "approved".to_string(),
                final_node: Some("project/auth".to_string()),
                final_labels: vec!["team:alpha".to_string()],
                decided_at: "2026-04-01T01:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        // No longer pending
        let pending = get_pending_suggestions(&conn).unwrap();
        assert_eq!(pending.len(), 0);

        // Should show as unapplied
        let unapplied = get_unapplied_decisions(&conn).unwrap();
        assert_eq!(unapplied.len(), 1);

        // Mark applied
        mark_applied(&conn, unapplied[0].1.id, "2026-04-01T02:00:00Z").unwrap();
        let unapplied = get_unapplied_decisions(&conn).unwrap();
        assert_eq!(unapplied.len(), 0);
    }

    #[test]
    fn pipeline_counts() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();

        let counts = get_pipeline_counts(&conn).unwrap();
        assert_eq!(counts.total_fetched, 3);
        assert_eq!(counts.untriaged, 3);
        assert_eq!(counts.pending_review, 0);
        assert_eq!(counts.approved_unapplied, 0);
        assert_eq!(counts.applied, 0);

        // Triage issue 1
        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        let sug_id = upsert_suggestion(
            &conn,
            &TriageSuggestion {
                id: 0,
                issue_id: issues[0].id,
                suggested_node: Some("a".to_string()),
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

        let counts = get_pipeline_counts(&conn).unwrap();
        assert_eq!(counts.untriaged, 2);
        assert_eq!(counts.pending_review, 1);

        // Approve
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: sug_id,
                decision: "approved".to_string(),
                final_node: Some("a".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-01T01:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        let counts = get_pipeline_counts(&conn).unwrap();
        assert_eq!(counts.pending_review, 0);
        assert_eq!(counts.approved_unapplied, 1);
    }

    #[test]
    fn filtered_pending_suggestions() {
        let conn = memory_db();
        let issues = vec![
            sample_issue("owner/repo", 1),
            sample_issue("owner/repo", 2),
            sample_issue("owner/repo", 3),
        ];
        for issue in &issues {
            upsert_issue(&conn, issue).unwrap();
        }
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();

        // Create suggestions with varying confidence
        for (i, confidence) in [0.3, 0.6, 0.9].iter().enumerate() {
            upsert_suggestion(&conn, &sample_suggestion(stored[i].id, "a", *confidence)).unwrap();
        }

        // No filter -> all 3
        let all = get_pending_suggestions_filtered(&conn, None, None).unwrap();
        assert_eq!(all.len(), 3);

        // min only
        let high = get_pending_suggestions_filtered(&conn, Some(0.5), None).unwrap();
        assert_eq!(high.len(), 2);

        // max only
        let low = get_pending_suggestions_filtered(&conn, None, Some(0.5)).unwrap();
        assert_eq!(low.len(), 1);

        // both
        let mid = get_pending_suggestions_filtered(&conn, Some(0.5), Some(0.8)).unwrap();
        assert_eq!(mid.len(), 1);
        assert!((mid[0].1.confidence.unwrap() - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn delete_suggestions_below_threshold() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();

        upsert_suggestion(&conn, &sample_suggestion(stored[0].id, "a", 0.3)).unwrap();
        upsert_suggestion(&conn, &sample_suggestion(stored[1].id, "b", 0.8)).unwrap();

        let deleted = delete_suggestions_below_confidence(&conn, 0.5).unwrap();
        assert_eq!(deleted, 1);

        // Issue 1 is now untriaged again
        let untriaged = get_untriaged_issues(&conn).unwrap();
        assert_eq!(untriaged.len(), 1);
        assert_eq!(untriaged[0].number, 1);

        // Issue 2 still has its suggestion
        let pending = get_pending_suggestions(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0.number, 2);
    }

    #[test]
    fn latest_updated_at() {
        let conn = memory_db();
        let mut issue1 = sample_issue("owner/repo", 1);
        issue1.updated_at = "2026-01-01T00:00:00Z".to_string();
        upsert_issue(&conn, &issue1).unwrap();

        let mut issue2 = sample_issue("owner/repo", 2);
        issue2.updated_at = "2026-03-15T12:00:00Z".to_string();
        upsert_issue(&conn, &issue2).unwrap();

        let latest = get_latest_updated_at(&conn, "owner/repo").unwrap();
        assert_eq!(latest, Some("2026-03-15T12:00:00Z".to_string()));

        let latest_empty = get_latest_updated_at(&conn, "other/repo").unwrap();
        assert_eq!(latest_empty, None);
    }

    #[test]
    fn delete_suggestions_by_node_prefix_test() {
        let conn = memory_db();
        for i in 1..=4 {
            upsert_issue(&conn, &sample_issue("owner/repo", i)).unwrap();
        }
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();

        upsert_suggestion(&conn, &sample_suggestion(stored[0].id, "backend/auth", 0.9)).unwrap();
        upsert_suggestion(
            &conn,
            &sample_suggestion(stored[1].id, "backend/auth/oauth", 0.8),
        )
        .unwrap();
        upsert_suggestion(
            &conn,
            &sample_suggestion(stored[2].id, "backend/authorize", 0.7),
        )
        .unwrap();
        upsert_suggestion(&conn, &sample_suggestion(stored[3].id, "frontend", 0.6)).unwrap();

        // Reset "backend/auth" subtree — should match exact + children, NOT "backend/authorize"
        let deleted = delete_suggestions_by_node_prefix(&conn, "backend/auth").unwrap();
        assert_eq!(deleted, 2);

        let untriaged = get_untriaged_issues(&conn).unwrap();
        assert_eq!(untriaged.len(), 2);
        assert_eq!(untriaged[0].number, 1);
        assert_eq!(untriaged[1].number, 2);

        // "backend/authorize" and "frontend" still have suggestions
        let pending = get_pending_suggestions(&conn).unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn delete_all_suggestions_test() {
        let conn = memory_db();
        for i in 1..=3 {
            upsert_issue(&conn, &sample_issue("owner/repo", i)).unwrap();
        }
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();
        for (i, s) in stored.iter().enumerate() {
            upsert_suggestion(
                &conn,
                &sample_suggestion(s.id, "a", (i as f64).mul_add(0.1, 0.5)),
            )
            .unwrap();
        }

        assert_eq!(get_pending_suggestions(&conn).unwrap().len(), 3);

        let deleted = delete_all_suggestions(&conn).unwrap();
        assert_eq!(deleted, 3);

        assert_eq!(get_untriaged_issues(&conn).unwrap().len(), 3);
        assert_eq!(get_pending_suggestions(&conn).unwrap().len(), 0);
    }

    #[test]
    fn delete_unreviewed_suggestions_test() {
        let conn = memory_db();
        for i in 1..=4 {
            upsert_issue(&conn, &sample_issue("owner/repo", i)).unwrap();
        }
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();

        // Create suggestions for all 4 issues
        for s in &stored {
            upsert_suggestion(&conn, &sample_suggestion(s.id, "node/a", 0.7)).unwrap();
        }

        let suggestions = get_pending_suggestions(&conn).unwrap();
        assert_eq!(suggestions.len(), 4);

        // Issue #1: approved — should be kept
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: suggestions[0].1.id,
                decision: "approved".to_string(),
                final_node: Some("node/a".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-01T01:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        // Issue #2: rejected — should be deleted
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: suggestions[1].1.id,
                decision: "rejected".to_string(),
                final_node: None,
                final_labels: vec![],
                decided_at: "2026-04-01T01:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        // Issue #3: modified — should be kept
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: suggestions[2].1.id,
                decision: "modified".to_string(),
                final_node: Some("node/b".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-01T01:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        // Issue #4: unreviewed — should be deleted

        // Delete unreviewed + rejected
        let deleted = delete_unreviewed_suggestions(&conn).unwrap();
        assert_eq!(deleted, 2); // #2 (rejected) + #4 (unreviewed)

        // Approved (#1) and modified (#3) suggestions still exist
        assert_eq!(get_untriaged_issues(&conn).unwrap().len(), 2); // #2 and #4
    }

    #[test]
    fn sub_issues_count_roundtrip() {
        let conn = memory_db();
        let mut issue = sample_issue("owner/repo", 1);
        issue.sub_issues_count = 5;
        upsert_issue(&conn, &issue).unwrap();

        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        assert_eq!(issues[0].sub_issues_count, 5);
    }

    #[test]
    fn is_tracking_issue_roundtrip() {
        let conn = memory_db();
        upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let stored = get_issues_by_repo(&conn, "owner/repo").unwrap();

        let mut sug = sample_suggestion(stored[0].id, "a", 0.9);
        sug.is_tracking_issue = true;
        upsert_suggestion(&conn, &sug).unwrap();

        let pending = get_pending_suggestions(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].1.is_tracking_issue);
    }

    #[test]
    fn fresh_db_gets_latest_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn pre_versioning_db_gets_migrated() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        // Simulate a pre-versioning v1 DB (tables exist but user_version = 0)
        conn.execute_batch(
            "CREATE TABLE issues (
                id INTEGER PRIMARY KEY, repo TEXT NOT NULL, number INTEGER NOT NULL,
                title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL,
                labels_json TEXT NOT NULL DEFAULT '[]', updated_at TEXT NOT NULL,
                fetched_at TEXT NOT NULL, UNIQUE(repo, number)
            );
            CREATE TABLE triage_suggestions (
                id INTEGER PRIMARY KEY, issue_id INTEGER NOT NULL REFERENCES issues(id),
                suggested_node TEXT, suggested_labels TEXT NOT NULL DEFAULT '[]',
                confidence REAL, reasoning TEXT NOT NULL DEFAULT '',
                llm_backend TEXT NOT NULL, created_at TEXT NOT NULL, UNIQUE(issue_id)
            );
            CREATE TABLE review_decisions (
                id INTEGER PRIMARY KEY, suggestion_id INTEGER NOT NULL REFERENCES triage_suggestions(id),
                decision TEXT NOT NULL, final_node TEXT, final_labels TEXT NOT NULL DEFAULT '[]',
                decided_at TEXT NOT NULL, applied_at TEXT, UNIQUE(suggestion_id)
            );",
        ).unwrap();

        // Insert a row before migration
        conn.execute(
            "INSERT INTO issues (repo, number, title, state, updated_at, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["owner/repo", 1i64, "Test", "OPEN", "2026-01-01T00:00:00Z", "2026-04-01T00:00:00Z"],
        ).unwrap();

        migrate(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        // The old row should still exist, with new columns at defaults
        let issues = get_issues_by_repo(&conn, "owner/repo").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].sub_issues_count, 0);
    }

    #[test]
    fn future_version_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999u32).unwrap();
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn pipeline_counts_serializes_to_json() {
        let counts = PipelineCounts {
            total_fetched: 100,
            untriaged: 20,
            pending_review: 30,
            approved_unapplied: 10,
            applied: 40,
            stale: 5,
        };
        let json = serde_json::to_string(&counts).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total_fetched"], 100);
        assert_eq!(parsed["applied"], 40);
    }

    #[test]
    fn suggestions_filtered_by_status_and_node() {
        let conn = memory_db();

        let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();

        let s1 = upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.9)).unwrap();
        upsert_suggestion(&conn, &sample_suggestion(id2, "flair/rust", 0.7)).unwrap();
        upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.5)).unwrap();

        // Approve suggestion 1
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: s1,
                decision: "approved".to_string(),
                final_node: Some("flair".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-01T00:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        // Filter: pending only
        let filters = SuggestionFilters {
            status: Some(SuggestionStatus::Pending),
            ..Default::default()
        };
        let results = get_suggestions_filtered(&conn, &filters).unwrap();
        assert_eq!(results.len(), 2);

        // Filter: node prefix "flair"
        let filters = SuggestionFilters {
            node_prefix: Some("flair".to_string()),
            ..Default::default()
        };
        let results = get_suggestions_filtered(&conn, &filters).unwrap();
        assert_eq!(results.len(), 2); // flair and flair/rust

        // Filter: min confidence
        let filters = SuggestionFilters {
            min_confidence: Some(0.8),
            ..Default::default()
        };
        let results = get_suggestions_filtered(&conn, &filters).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.suggested_node.as_deref(), Some("flair"));
    }

    #[test]
    fn decisions_filtered_by_status() {
        let conn = memory_db();
        let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        let s1 = upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.9)).unwrap();
        let s2 = upsert_suggestion(&conn, &sample_suggestion(id2, "devops", 0.8)).unwrap();

        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: s1,
                decision: "approved".to_string(),
                final_node: Some("flair".to_string()),
                final_labels: vec!["area: FLAIR".to_string()],
                decided_at: "2026-04-01T00:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: s2,
                decision: "rejected".to_string(),
                final_node: None,
                final_labels: vec![],
                decided_at: "2026-04-01T00:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        let results = get_decisions_filtered(&conn, &DecisionFilters::default()).unwrap();
        assert_eq!(results.len(), 2);

        let results = get_decisions_filtered(
            &conn,
            &DecisionFilters {
                unapplied: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.decision, "approved");

        let results = get_decisions_filtered(
            &conn,
            &DecisionFilters {
                status: Some("rejected".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.decision, "rejected");
    }

    #[test]
    fn delete_suggestions_for_reclassify_test() {
        let conn = memory_db();
        let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();
        let id4 = upsert_issue(&conn, &sample_issue("owner/repo", 4)).unwrap();

        // id1: null node (should be reset)
        upsert_suggestion(
            &conn,
            &TriageSuggestion {
                suggested_node: None,
                ..sample_suggestion(id1, "", 0.3)
            },
        )
        .unwrap();

        // id2: has the target category in suggestions (should be reset)
        upsert_suggestion(
            &conn,
            &TriageSuggestion {
                suggested_new_categories: vec!["circuit/emulator".to_string()],
                ..sample_suggestion(id2, "circuit", 0.7)
            },
        )
        .unwrap();

        // id3: classified normally (should NOT be reset)
        upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.9)).unwrap();

        // id4: null node AND voted for category (should be reset, counted once)
        upsert_suggestion(
            &conn,
            &TriageSuggestion {
                suggested_node: None,
                suggested_new_categories: vec!["circuit/emulator".to_string()],
                ..sample_suggestion(id4, "", 0.25)
            },
        )
        .unwrap();

        let deleted = delete_suggestions_for_reclassify(&conn, "circuit/emulator").unwrap();
        assert_eq!(deleted, 3); // id1, id2, id4

        let remaining = get_suggestions_filtered(&conn, &SuggestionFilters::default()).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1.suggested_node.as_deref(), Some("devops"));
    }

    #[test]
    fn summary_confidence_distribution_and_node_breakdown() {
        let conn = memory_db();
        let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
        let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
        let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();
        let id4 = upsert_issue(&conn, &sample_issue("other/repo", 4)).unwrap();

        upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.95)).unwrap();
        upsert_suggestion(&conn, &sample_suggestion(id2, "flair", 0.85)).unwrap();
        upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.6)).unwrap();
        upsert_suggestion(
            &conn,
            &TriageSuggestion {
                suggested_node: None,
                suggested_new_categories: vec!["circuit/emulator".to_string()],
                ..sample_suggestion(id4, "", 0.3)
            },
        )
        .unwrap();

        let dist = get_confidence_distribution(&conn, None).unwrap();
        assert_eq!(dist.len(), 5);
        let total: usize = dist.iter().map(|d| d.count).sum();
        assert_eq!(total, 4);

        let nodes = get_node_breakdown(&conn, None).unwrap();
        assert!(nodes.len() >= 2);
        assert_eq!(nodes[0].node.as_deref(), Some("flair"));
        assert_eq!(nodes[0].count, 2);

        let dist = get_confidence_distribution(&conn, Some("owner/repo")).unwrap();
        let total: usize = dist.iter().map(|d| d.count).sum();
        assert_eq!(total, 3);

        let votes = get_new_category_votes(&conn, None).unwrap();
        assert_eq!(votes.len(), 1);
        assert_eq!(votes[0].category, "circuit/emulator");
        assert_eq!(votes[0].vote_count, 1);
    }

    #[test]
    fn get_suggestion_by_issue_test() {
        let conn = memory_db();

        // No suggestion -> None
        let result = get_suggestion_by_issue(&conn, "owner/repo", 999).unwrap();
        assert!(result.is_none());

        // Insert issue + suggestion, no decision
        let issue = sample_issue("owner/repo", 42);
        let issue_id = upsert_issue(&conn, &issue).unwrap();
        let sug = sample_suggestion(issue_id, "some/node", 0.85);
        upsert_suggestion(&conn, &sug).unwrap();

        let result = get_suggestion_by_issue(&conn, "owner/repo", 42).unwrap();
        assert!(result.is_some());
        let (i, s, d) = result.unwrap();
        assert_eq!(i.number, 42);
        assert_eq!(s.suggested_node, Some("some/node".to_string()));
        assert!(d.is_none()); // no decision yet

        // Add a decision -> should appear
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: s.id,
                decision: "approved".to_string(),
                final_node: Some("some/node".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-07T00:00:00Z".to_string(),
                applied_at: None,
                question: String::new(),
            },
        )
        .unwrap();

        let (_, _, d) = get_suggestion_by_issue(&conn, "owner/repo", 42)
            .unwrap()
            .unwrap();
        assert!(d.is_some());
        assert_eq!(d.unwrap().decision, "approved");
    }

    #[test]
    fn decide_approve_does_not_create_decision_for_applied() {
        let conn = memory_db();

        let issue = sample_issue("owner/repo", 50);
        let issue_id = upsert_issue(&conn, &issue).unwrap();
        let sug = sample_suggestion(issue_id, "some/node", 0.9);
        upsert_suggestion(&conn, &sug).unwrap();

        // Look up and get suggestion id
        let (_, s, _) = get_suggestion_by_issue(&conn, "owner/repo", 50)
            .unwrap()
            .unwrap();

        // Insert an applied decision
        insert_decision(
            &conn,
            &ReviewDecision {
                id: 0,
                suggestion_id: s.id,
                decision: "approved".to_string(),
                final_node: Some("some/node".to_string()),
                final_labels: vec![],
                decided_at: "2026-04-07T00:00:00Z".to_string(),
                applied_at: Some("2026-04-07T01:00:00Z".to_string()),
                question: String::new(),
            },
        )
        .unwrap();

        // Verify applied_at is visible
        let (_, _, d) = get_suggestion_by_issue(&conn, "owner/repo", 50)
            .unwrap()
            .unwrap();
        assert!(d.is_some());
        assert!(d.unwrap().applied_at.is_some());
    }
}
