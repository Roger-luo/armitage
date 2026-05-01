//! Verifies the `sync` namespace exposes pull/push/resolve and that the
//! pre-rename top-level forms are no longer recognized.

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
fn sync_namespace_replaces_top_level_pull_push_resolve() {
    let tmp = TempDir::new().unwrap();
    let org = tmp.path().join("acmeorg");
    armitage::cli::init::init_at(&org, "acmeorg", &["acme".to_string()], None).unwrap();

    // New forms succeed at --help.
    for sub in ["pull", "push", "resolve"] {
        let out = run(&["sync", sub, "--help"], &org);
        assert!(
            out.status.success(),
            "sync {sub} --help failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // sync push --dry-run runs cleanly on a fresh org with no nodes.
    let out = run(&["sync", "push", "--dry-run"], &org);
    assert!(
        out.status.success(),
        "sync push --dry-run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Old top-level commands are rejected.
    for old in ["pull", "push", "resolve"] {
        let out = run(&[old, "--help"], &org);
        assert!(
            !out.status.success(),
            "old top-level `{old}` should be rejected after sync namespace move"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("unrecognized subcommand") || stderr.contains("invalid"),
            "expected unrecognized-subcommand error for `{old}`, got: {stderr}"
        );
    }
}
