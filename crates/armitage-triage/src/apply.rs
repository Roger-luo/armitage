use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::Connection;

use crate::db;
use crate::error::Result;
use armitage_core::node::IssueRef;
use armitage_labels::rename::LabelRenameLedger;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ApplyStats {
    pub applied: usize,
    pub failed: usize,
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Push all approved, unapplied label changes to GitHub.
pub fn apply_all(
    gh: &armitage_github::Gh,
    conn: &Connection,
    org_root: &Path,
    dry_run: bool,
) -> Result<ApplyStats> {
    let ledger = armitage_labels::rename::read_rename_ledger(org_root)?;
    let decisions = db::get_unapplied_decisions(conn)?;
    if decisions.is_empty() {
        println!("No approved changes to apply.");
        return Ok(ApplyStats::default());
    }

    // Pre-create any labels that decisions need but don't exist on the repos yet.
    // This avoids per-issue "label not found" failures and is much cheaper than
    // pushing the entire labels.toml catalog to every repo.
    if !dry_run {
        ensure_labels_exist(gh, &decisions)?;
    }

    let mut stats = ApplyStats::default();
    let now = chrono::Utc::now().to_rfc3339();

    for (issue, decision) in &decisions {
        let issue_ref_str = format!("{}#{}", issue.repo, issue.number);
        let issue_ref = IssueRef::parse(&issue_ref_str)?;

        // Inquired / stale-with-question decisions: post question as a comment
        if decision.decision == "inquired"
            || (decision.decision == "stale" && !decision.question.is_empty())
        {
            let label = if decision.decision == "stale" {
                "staleness inquiry"
            } else {
                "question"
            };
            if dry_run {
                println!("  {issue_ref_str}: would post {label}:");
                for line in decision.question.lines() {
                    println!("    {line}");
                }
                stats.applied += 1;
                continue;
            }
            match armitage_github::issue::add_comment(gh, &issue_ref, &decision.question) {
                Ok(()) => {
                    db::mark_applied(conn, decision.id, &now)?;
                    println!("  {issue_ref_str}: posted {label}");
                    stats.applied += 1;
                }
                Err(e) => {
                    eprintln!("  {issue_ref_str}: error: {e}");
                    stats.failed += 1;
                }
            }
            continue;
        }

        // Stale without question: just mark as applied (internal-only)
        if decision.decision == "stale" {
            if !dry_run {
                db::mark_applied(conn, decision.id, &now)?;
            }
            println!("  {issue_ref_str}: stale (no action)");
            stats.skipped += 1;
            continue;
        }

        // Label-change decisions: compute diff and apply
        let LabelDiff {
            add: add_labels,
            remove: remove_labels,
        } = compute_label_diff(&issue.labels, &decision.final_labels, &ledger);

        if add_labels.is_empty() && remove_labels.is_empty() {
            if !dry_run {
                db::mark_applied(conn, decision.id, &now)?;
            }
            println!("  {issue_ref_str}: no label changes needed");
            stats.skipped += 1;
            continue;
        }

        if dry_run {
            println!("  {issue_ref_str}:");
            if !add_labels.is_empty() {
                println!("    + {}", add_labels.join(", "));
            }
            if !remove_labels.is_empty() {
                println!("    - {}", remove_labels.join(", "));
            }
            stats.applied += 1;
            continue;
        }

        match armitage_github::issue::update_issue(
            gh,
            &issue_ref,
            None, // no title change
            None, // no body change
            &add_labels,
            &remove_labels,
        ) {
            Ok(()) => {
                db::mark_applied(conn, decision.id, &now)?;
                println!("  {issue_ref_str}: applied");
                if !add_labels.is_empty() {
                    println!("    + {}", add_labels.join(", "));
                }
                if !remove_labels.is_empty() {
                    println!("    - {}", remove_labels.join(", "));
                }
                stats.applied += 1;
            }
            Err(e) => {
                eprintln!("  {issue_ref_str}: error: {e}");
                stats.failed += 1;
            }
        }
    }

    if dry_run {
        println!("\nDry run: {} changes would be applied", stats.applied);
    } else {
        println!(
            "\nApplied: {}, Failed: {}, Skipped: {}",
            stats.applied, stats.failed, stats.skipped
        );
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Label diff with rename-ledger awareness
// ---------------------------------------------------------------------------

pub struct LabelDiff {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Compute which labels to add/remove for an issue, consulting the rename
/// ledger so that old-format labels whose new-format equivalents are already
/// in `desired` get removed automatically.
pub fn compute_label_diff(
    current_labels: &[String],
    desired_labels: &[String],
    ledger: &LabelRenameLedger,
) -> LabelDiff {
    let current: BTreeSet<&str> = current_labels.iter().map(|s| s.as_str()).collect();
    let desired: BTreeSet<&str> = desired_labels.iter().map(|s| s.as_str()).collect();

    let add: Vec<String> = desired
        .difference(&current)
        .map(|s| s.to_string())
        .collect();

    let mut remove: BTreeSet<&str> = current.difference(&desired).copied().collect();

    // Check each label on the issue: if the ledger maps it to a new name
    // that is already in the desired set, schedule the old label for removal.
    for label in &current {
        if let Some(rename) = ledger.renames.iter().find(|r| r.old_name == *label)
            && desired.contains(rename.new_name.as_str())
        {
            remove.insert(label);
        }
    }

    LabelDiff {
        add,
        remove: remove.into_iter().map(|s| s.to_string()).collect(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// For each repo that has pending decisions, fetch existing labels once, then
/// create only the labels that decisions actually need but don't exist yet.
fn ensure_labels_exist(
    gh: &armitage_github::Gh,
    decisions: &[(db::StoredIssue, db::ReviewDecision)],
) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};

    // Collect needed labels per repo (only additions -- labels to be added).
    let mut needed_per_repo: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (issue, decision) in decisions {
        // Skip stale/inquired -- they don't change labels
        if decision.decision == "stale" || decision.decision == "inquired" {
            continue;
        }
        let current: BTreeSet<&str> = issue.labels.iter().map(|s| s.as_str()).collect();
        let desired: BTreeSet<&str> = decision.final_labels.iter().map(|s| s.as_str()).collect();
        let to_add: Vec<&str> = desired.difference(&current).copied().collect();
        if !to_add.is_empty() {
            let entry = needed_per_repo.entry(issue.repo.as_str()).or_default();
            entry.extend(to_add);
        }
    }

    if needed_per_repo.is_empty() {
        return Ok(());
    }

    for (repo, needed_labels) in &needed_per_repo {
        let remote = armitage_github::issue::fetch_repo_labels(gh, repo)?;
        let existing: BTreeSet<&str> = remote.iter().map(|l| l.name.as_str()).collect();

        let missing: Vec<&&str> = needed_labels
            .iter()
            .filter(|l| !existing.contains(**l))
            .collect();

        if !missing.is_empty() {
            println!("  Creating {} missing label(s) on {repo}...", missing.len());
            for label_name in missing {
                armitage_github::issue::create_label(gh, repo, label_name, "", None)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use armitage_labels::rename::LabelRename;

    fn empty_ledger() -> LabelRenameLedger {
        LabelRenameLedger { renames: vec![] }
    }

    fn ledger_with(renames: Vec<(&str, &str)>) -> LabelRenameLedger {
        LabelRenameLedger {
            renames: renames
                .into_iter()
                .map(|(old, new)| LabelRename {
                    old_name: old.to_string(),
                    new_name: new.to_string(),
                    recorded_at: "2026-01-01T00:00:00Z".to_string(),
                    synced_repos: vec![],
                })
                .collect(),
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn basic_diff_no_ledger() {
        let current = s(&["bug"]);
        let desired = s(&["bug", "area: stdlib"]);
        let diff = compute_label_diff(&current, &desired, &empty_ledger());
        assert_eq!(diff.add, vec!["area: stdlib"]);
        assert!(diff.remove.is_empty());
    }

    #[test]
    fn basic_removal_no_ledger() {
        let current = s(&["bug", "wontfix"]);
        let desired = s(&["bug"]);
        let diff = compute_label_diff(&current, &desired, &empty_ledger());
        assert!(diff.add.is_empty());
        assert_eq!(diff.remove, vec!["wontfix"]);
    }

    /// The core bug: an issue has old-format label `A-StandardLib` and the
    /// triage decision adds `area: stdlib`. Without rename-ledger awareness
    /// the old label survives because it is in both current and desired
    /// (via merge_labels union). With the fix, the ledger causes the old
    /// label to be scheduled for removal.
    #[test]
    fn old_format_label_removed_via_ledger() {
        let ledger = ledger_with(vec![("A-StandardLib", "area: stdlib")]);

        // After merge_labels, desired = union(current, suggested) which
        // includes both old and new format labels.
        let current = s(&["A-StandardLib", "category: bug"]);
        let desired = s(&["A-StandardLib", "category: bug", "area: stdlib"]);

        let diff = compute_label_diff(&current, &desired, &ledger);
        assert_eq!(diff.add, vec!["area: stdlib"]);
        assert_eq!(diff.remove, vec!["A-StandardLib"]);
    }

    #[test]
    fn multiple_old_format_labels_removed() {
        let ledger = ledger_with(vec![
            ("A-StandardLib", "area: stdlib"),
            ("Perf-Optimization", "performance: optimization"),
            ("R-prototyping", "research: prototyping"),
        ]);

        let current = s(&["A-StandardLib", "Perf-Optimization", "R-prototyping"]);
        let desired = s(&[
            "A-StandardLib",
            "Perf-Optimization",
            "R-prototyping",
            "area: stdlib",
            "performance: optimization",
            "research: prototyping",
        ]);

        let diff = compute_label_diff(&current, &desired, &ledger);
        assert_eq!(
            diff.add,
            vec![
                "area: stdlib",
                "performance: optimization",
                "research: prototyping",
            ]
        );
        assert_eq!(
            diff.remove,
            vec!["A-StandardLib", "Perf-Optimization", "R-prototyping",]
        );
    }

    /// Old label exists on the issue but its new-format equivalent is NOT in
    /// the desired set -- the ledger should not remove it (no replacement present).
    #[test]
    fn old_format_label_kept_when_new_not_in_desired() {
        let ledger = ledger_with(vec![("A-StandardLib", "area: stdlib")]);

        let current = s(&["A-StandardLib", "bug"]);
        let desired = s(&["A-StandardLib", "bug"]);

        let diff = compute_label_diff(&current, &desired, &ledger);
        assert!(diff.add.is_empty());
        assert!(diff.remove.is_empty());
    }

    /// No-op when issue already has correct labels and no renames apply.
    #[test]
    fn no_changes_when_already_correct() {
        let ledger = ledger_with(vec![("A-StandardLib", "area: stdlib")]);
        let current = s(&["area: stdlib", "category: bug"]);
        let desired = s(&["area: stdlib", "category: bug"]);

        let diff = compute_label_diff(&current, &desired, &ledger);
        assert!(diff.add.is_empty());
        assert!(diff.remove.is_empty());
    }
}
