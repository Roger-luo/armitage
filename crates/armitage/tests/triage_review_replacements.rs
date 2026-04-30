//! Integration tests verifying that `triage suggestions --status pending` and
//! `triage decide --all-pending --decision approve --min-confidence ...` cover the
//! workflows previously served by `triage review --list` and `triage review --auto-approve`.
//!
//! Tests run sequentially because the CLI runners resolve the org via the process
//! current working directory.

use std::sync::Mutex;

use armitage_triage::db::{self, StoredIssue, TriageSuggestion};
use tempfile::TempDir;

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn make_issue(repo: &str, number: u64, title: &str, labels: &[&str]) -> StoredIssue {
    StoredIssue {
        id: 0,
        repo: repo.to_string(),
        number,
        title: title.to_string(),
        body: format!("Body for {repo}#{number}"),
        state: "open".to_string(),
        labels: labels.iter().map(|s| (*s).to_string()).collect(),
        updated_at: "2026-04-01T00:00:00Z".to_string(),
        fetched_at: "2026-04-01T00:00:00Z".to_string(),
        sub_issues_count: 0,
        author: "tester".to_string(),
        assignees: vec![],
        is_pr: false,
        comment_count: 0,
    }
}

fn make_suggestion(
    issue_id: i64,
    node: Option<&str>,
    labels: &[&str],
    confidence: f64,
) -> TriageSuggestion {
    TriageSuggestion {
        id: 0,
        issue_id,
        suggested_node: node.map(str::to_string),
        suggested_labels: labels.iter().map(|s| (*s).to_string()).collect(),
        confidence: Some(confidence),
        reasoning: format!("synthetic suggestion at confidence {confidence}"),
        llm_backend: "test".to_string(),
        created_at: "2026-04-01T00:00:00Z".to_string(),
        is_tracking_issue: false,
        suggested_new_categories: vec![],
        is_stale: false,
        is_inactive: false,
        needs_followup: false,
        followup_reason: String::new(),
    }
}

fn setup_org() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let org = tmp.path().join("org");
    armitage::cli::init::init_at(&org, "testorg", &["acme".to_string()], None).unwrap();
    armitage::cli::node::create_node(
        &org,
        "widget",
        Some("Widget"),
        Some("Widget initiative"),
        None,
        None,
        "active",
    )
    .unwrap();

    // Pre-populate DB with a couple of issues + suggestions.
    let conn = db::open_db(&org).unwrap();

    let issue1_id = db::upsert_issue(
        &conn,
        &make_issue("acme/widget", 101, "High-conf issue", &["area: core"]),
    )
    .unwrap();
    let issue2_id = db::upsert_issue(
        &conn,
        &make_issue("acme/widget", 102, "Medium-conf issue", &[]),
    )
    .unwrap();
    let issue3_id = db::upsert_issue(
        &conn,
        &make_issue("acme/widget", 103, "Low-conf issue", &[]),
    )
    .unwrap();

    db::upsert_suggestion(
        &conn,
        &make_suggestion(issue1_id, Some("widget"), &["priority: high"], 0.95),
    )
    .unwrap();
    db::upsert_suggestion(
        &conn,
        &make_suggestion(issue2_id, Some("widget"), &["priority: medium"], 0.65),
    )
    .unwrap();
    db::upsert_suggestion(
        &conn,
        &make_suggestion(issue3_id, Some("widget"), &["priority: low"], 0.30),
    )
    .unwrap();

    tmp
}

/// Run a closure with the process cwd temporarily set to `dir`.
fn with_cwd<R>(dir: &std::path::Path, f: impl FnOnce() -> R) -> R {
    let _g = CWD_LOCK.lock().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = f();
    std::env::set_current_dir(prev).unwrap();
    result
}

#[test]
fn decide_all_pending_approve_min_confidence_replaces_auto_approve() {
    let tmp = setup_org();
    let org = tmp.path().join("org");

    // Bulk-approve everything at >= 0.8 confidence (replaces `review --auto-approve 0.8`).
    with_cwd(&org, || {
        armitage::cli::triage::run_decide(
            vec![],
            "approve".to_string(),
            true,      // all_pending
            Some(0.8), // min_confidence
            None,      // max_confidence
            None,
            None,
            None,
            None,
        )
        .unwrap();
    });

    // Verify: only the 0.95 suggestion was approved; the others remain pending.
    let conn = db::open_db(&org).unwrap();
    let pending = db::get_pending_suggestions(&conn).unwrap();
    let pending_numbers: Vec<u64> = pending.iter().map(|(i, _)| i.number).collect();
    assert_eq!(pending_numbers, vec![102, 103]);

    // Verify: existing labels were merged with suggested labels (set union, order preserved).
    let approved_decisions = conn
        .prepare(
            "SELECT rd.final_node, rd.final_labels FROM review_decisions rd \
             JOIN triage_suggestions ts ON ts.id = rd.suggestion_id \
             JOIN issues i ON i.id = ts.issue_id WHERE i.number = 101",
        )
        .unwrap()
        .query_map([], |row| {
            let node: Option<String> = row.get(0)?;
            let labels: String = row.get(1)?;
            Ok((node, labels))
        })
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(approved_decisions.0.as_deref(), Some("widget"));
    let final_labels: Vec<String> = serde_json::from_str(&approved_decisions.1).unwrap();
    assert_eq!(final_labels, vec!["area: core", "priority: high"]);
}

#[test]
fn suggestions_status_pending_json_replaces_review_list() {
    let tmp = setup_org();
    let org = tmp.path().join("org");

    // Capture stdout from `triage suggestions --status pending --min-confidence 0.5 --format json`.
    // The CLI runner writes to println! so we run it and then re-query the DB to assert the
    // filter logic matches what JSON output would contain.
    with_cwd(&org, || {
        armitage::cli::triage::run_suggestions(
            vec![],
            None,
            None,
            Some(0.5),
            None,
            Some("pending".to_string()),
            false,
            false,
            false,
            "confidence".to_string(),
            50,
            "json".to_string(),
            500,
        )
        .unwrap();
    });

    // Verify the same filter via the DB layer (this is what `suggestions` reads).
    let conn = db::open_db(&org).unwrap();
    let filters = db::SuggestionFilters {
        issue_numbers: vec![],
        node_prefix: None,
        repo: None,
        min_confidence: Some(0.5),
        max_confidence: None,
        status: Some(db::SuggestionStatus::Pending),
        tracking_only: false,
        unclassified: false,
        stale_only: false,
        sort: db::SuggestionSort::Confidence,
        limit: 50,
    };
    let rows = db::get_suggestions_filtered(&conn, &filters).unwrap();
    let numbers: Vec<u64> = rows.iter().map(|(i, _)| i.number).collect();
    // 0.95 and 0.65 pass the >= 0.5 filter; 0.30 does not.
    assert_eq!(numbers, vec![101, 102]);
}
