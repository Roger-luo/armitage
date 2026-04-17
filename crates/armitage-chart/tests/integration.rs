//! Integration test: generates chart HTML from the mock org fixture.
//!
//! This test is used by the Playwright test harness in Rust pipeline mode.
//! Run with: cargo test -p armitage-chart --test integration

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use armitage_chart::data::{IssueDates, build_chart_data};
use armitage_chart::render_chart;
use armitage_core::tree::walk_nodes;
use rusqlite::Connection;

/// Build an IssueDates map from the mock org's SQLite database,
/// replicating the query from armitage/src/cli/chart.rs.
fn build_issue_dates(db_path: &Path) -> HashMap<String, IssueDates> {
    let mut map = HashMap::new();
    let conn = Connection::open(db_path).expect("failed to open mock triage.db");
    let mut stmt = conn
        .prepare(
            "SELECT i.repo, i.number, i.state, p.start_date, p.target_date, i.body, i.labels_json, i.author, i.assignees_json, i.is_pr
             FROM issues i
             LEFT JOIN issue_project_items p ON p.issue_id = i.id",
        )
        .expect("failed to prepare query");
    let rows = stmt
        .query_map([], |row| {
            let repo: String = row.get(0)?;
            let number: i64 = row.get(1)?;
            let state: String = row.get(2)?;
            let start_date: Option<String> = row.get(3)?;
            let target_date: Option<String> = row.get(4)?;
            let body: String = row.get(5)?;
            let labels_json: String = row.get(6)?;
            let author: String = row.get(7)?;
            let assignees_json: String = row.get(8)?;
            let is_pr: bool = row.get::<_, i64>(9)? != 0;
            Ok((
                format!("{repo}#{number}"),
                state,
                start_date,
                target_date,
                body,
                labels_json,
                author,
                assignees_json,
                is_pr,
            ))
        })
        .expect("failed to query issues");
    for row in rows.flatten() {
        let (
            issue_ref,
            state,
            start_date,
            target_date,
            body,
            labels_json,
            author,
            assignees_json,
            is_pr,
        ) = row;
        map.insert(
            issue_ref,
            IssueDates {
                start_date,
                target_date,
                state: Some(state),
                description: if body.is_empty() { None } else { Some(body) },
                labels: serde_json::from_str(&labels_json).unwrap_or_default(),
                author: if author.is_empty() {
                    None
                } else {
                    Some(author)
                },
                assignees: serde_json::from_str(&assignees_json).unwrap_or_default(),
                is_pr,
            },
        );
    }
    map
}

#[test]
fn generate_test_html() {
    let mock_org = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock-org");
    assert!(
        mock_org.exists(),
        "mock-org fixture not found at {mock_org:?}"
    );

    let db_path = mock_org.join(".armitage/triage/triage.db");
    assert!(db_path.exists(), "mock triage.db not found at {db_path:?}");

    // Walk nodes from the mock org
    let entries = walk_nodes(&mock_org).expect("failed to walk mock org nodes");
    assert!(!entries.is_empty(), "no nodes found in mock org");

    // Build issue dates from the mock DB
    let issue_dates = build_issue_dates(&db_path);

    // Build chart data
    let chart_data =
        build_chart_data(&entries, "nexus", &issue_dates).expect("failed to build chart data");

    // Verify basic structure
    assert!(
        !chart_data.nodes.is_empty(),
        "chart data has no top-level nodes"
    );
    assert!(chart_data.global_start.is_some(), "no global_start");
    assert!(chart_data.global_end.is_some(), "no global_end");

    // Render to HTML
    let html = render_chart(&chart_data, true).expect("failed to render chart HTML");
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("nexus"));

    // Write to the output directory for Playwright tests
    let output_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-results/html");
    fs::create_dir_all(&output_dir).expect("failed to create output dir");
    fs::write(output_dir.join("mock-org.html"), &html).expect("failed to write HTML");

    println!(
        "Generated mock-org chart HTML ({} bytes) with {} top-level nodes",
        html.len(),
        chart_data.nodes.len()
    );
}

#[test]
fn mock_org_has_expected_structure() {
    let mock_org = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock-org");
    let entries = walk_nodes(&mock_org).expect("failed to walk mock org nodes");

    // Should have 5 top-level initiatives
    let top_level: Vec<_> = entries.iter().filter(|e| !e.path.contains('/')).collect();
    assert_eq!(
        top_level.len(),
        5,
        "expected 5 top-level initiatives, got {}: {:?}",
        top_level.len(),
        top_level.iter().map(|e| &e.path).collect::<Vec<_>>()
    );

    // Aurora should have nested children
    let aurora_children: Vec<_> = entries
        .iter()
        .filter(|e| e.path.starts_with("aurora/"))
        .collect();
    assert!(
        aurora_children.len() >= 4,
        "aurora should have at least 4 sub-nodes"
    );

    // Delta should have no children
    let delta_children: Vec<_> = entries
        .iter()
        .filter(|e| e.path.starts_with("delta/"))
        .collect();
    assert_eq!(delta_children.len(), 0, "delta should have no children");
}
