//! End-to-end test that verifies the canonical "milestone as sub-node with
//! its own [timeline]" pattern works through the CLI.

use std::process::Command;
use tempfile::TempDir;

fn armitage_bin() -> &'static str {
    env!("CARGO_BIN_EXE_armitage")
}

fn run(args: &[&str], cwd: &std::path::Path) -> std::process::Output {
    Command::new(armitage_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn armitage")
}

#[test]
fn sub_node_with_timeline_acts_as_milestone() {
    let tmp = TempDir::new().unwrap();
    let org = tmp.path().join("acmeorg");

    // Init the org via the library API.
    armitage::cli::init::init_at(&org, "acmeorg", &["acme".to_string()], None).unwrap();

    // Parent project node with its own timeline (full year).
    let out = run(
        &[
            "node",
            "new",
            "widget",
            "--name",
            "Widget",
            "--description",
            "Widget initiative",
            "--timeline",
            "2026-01-01 to 2026-12-31",
            "--status",
            "active",
        ],
        &org,
    );
    assert!(
        out.status.success(),
        "node new widget failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Child milestone node with a tighter timeline contained within the parent's.
    let out = run(
        &[
            "node",
            "new",
            "widget/mvp",
            "--name",
            "Widget MVP",
            "--description",
            "Milestone modeled as a child node with its own [timeline].",
            "--timeline",
            "2026-02-01 to 2026-03-31",
            "--status",
            "active",
        ],
        &org,
    );
    assert!(
        out.status.success(),
        "node new widget/mvp failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // `armitage node tree` should show both nodes.
    let tree = run(&["node", "tree"], &org);
    assert!(tree.status.success(), "node tree failed: {tree:?}");
    let tree_out = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree_out.contains("widget"),
        "tree missing parent: {tree_out}"
    );
    assert!(
        tree_out.contains("mvp") || tree_out.contains("Widget MVP"),
        "tree missing milestone-style child: {tree_out}"
    );

    // `armitage node tree --depth 1` should still work and include the parent.
    let depth1 = run(&["node", "tree", "--depth", "1"], &org);
    assert!(
        depth1.status.success(),
        "node tree --depth 1 failed: {depth1:?}"
    );
    let depth1_out = String::from_utf8_lossy(&depth1.stdout);
    assert!(
        depth1_out.contains("widget") || depth1_out.contains("Widget"),
        "depth 1 tree missing top-level: {depth1_out}"
    );

    // `armitage okr show --period 2026-Q1` should run successfully against the
    // synthetic org. With no GitHub-tracked issues, the rendered list will be
    // empty — but the command must still exit 0 and emit a header (table) or
    // an empty JSON array, proving the milestone-style sub-node integrates with
    // the OKR derivation pipeline.
    let okr = run(&["okr", "show", "--period", "2026-Q1"], &org);
    assert!(
        okr.status.success(),
        "okr show failed: stdout={} stderr={}",
        String::from_utf8_lossy(&okr.stdout),
        String::from_utf8_lossy(&okr.stderr)
    );
    let okr_out = String::from_utf8_lossy(&okr.stdout);
    assert!(
        okr_out.contains("OKRs"),
        "okr show output missing header: {okr_out}"
    );

    let okr_json = run(
        &["okr", "show", "--period", "2026-Q1", "--format", "json"],
        &org,
    );
    assert!(
        okr_json.status.success(),
        "okr show --format json failed: stderr={}",
        String::from_utf8_lossy(&okr_json.stderr)
    );
    let okr_json_out = String::from_utf8_lossy(&okr_json.stdout);
    assert!(
        okr_json_out.trim().starts_with('['),
        "okr show --format json should emit a JSON array, got: {okr_json_out}"
    );

    // The `milestone` subcommand should no longer exist.
    let removed = run(&["milestone", "--help"], &org);
    assert!(
        !removed.status.success(),
        "milestone subcommand should be gone"
    );
    let stderr = String::from_utf8_lossy(&removed.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") || stderr.contains("error"),
        "expected unrecognized-subcommand error, got: {stderr}"
    );
}
