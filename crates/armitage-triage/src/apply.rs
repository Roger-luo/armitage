use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::TriageDomain;
use crate::db;
use crate::error::Result;
use armitage_core::issues::IssuesFile;
use armitage_core::node::IssueRef;
use armitage_core::org::Org;
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_labels::rename::LabelRenameLedger;

// ---------------------------------------------------------------------------
// Decision type constants
// ---------------------------------------------------------------------------

const DECISION_STALE: &str = "stale";
const DECISION_INQUIRED: &str = "inquired";

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

/// Mark a decision as applied and record the issue in the target node's
/// `issues.toml`. Logs a warning (but does not fail) if the write fails.
fn finalize_decision(
    conn: &Connection,
    decision: &db::ReviewDecision,
    all_nodes: &[NodeEntry],
    org_root: &Path,
    issue_ref_str: &str,
    title: &str,
    now: &str,
) -> Result<()> {
    db::mark_applied(conn, decision.id, now)?;
    if let Some(node_path) = &decision.final_node
        && let Err(e) =
            record_issue_in_node(all_nodes, org_root, node_path, issue_ref_str, Some(title))
    {
        eprintln!("  {issue_ref_str}: warning: issues.toml write failed: {e}");
    }
    Ok(())
}

/// Push all approved, unapplied label changes to GitHub.
pub fn apply_all(
    gh: &armitage_github::Gh,
    conn: &Connection,
    org_root: &Path,
    dry_run: bool,
) -> Result<ApplyStats> {
    let ledger = armitage_labels::rename::read_rename_ledger(org_root)?;
    let mut decisions = db::get_unapplied_decisions(conn)?;
    if decisions.is_empty() {
        println!("No approved changes to apply.");
        return Ok(ApplyStats::default());
    }

    // Build node-label map: for each node, collect its labels + all ancestor labels.
    // These are structural labels that must be on every issue classified to the node.
    let node_labels = build_node_label_map(org_root);

    // Inject node labels into each decision's final_labels before applying.
    // Node labels are additive: they are merged with the decision's final_labels
    // (which already include the issue's existing labels from the approve step).
    // Skip labels that the repo already implies (configured in armitage.toml
    // under [triage.repo_labels]).
    let repo_labels = read_repo_implied_labels(org_root);
    for (issue, decision) in &mut decisions {
        let implied = repo_labels.get(issue.repo.as_str());

        // Inject node labels (skipping repo-implied ones)
        if let Some(node_path) = &decision.final_node
            && let Some(labels) = node_labels.get(node_path.as_str())
        {
            // Start with what's already in final_labels + current issue labels
            let mut all: BTreeSet<String> = decision.final_labels.iter().cloned().collect();
            all.extend(issue.labels.iter().cloned());
            // Add node labels, skipping those implied by the repo
            for label in labels {
                if implied.is_some_and(|set| set.contains(label.as_str())) {
                    continue;
                }
                all.insert(label.clone());
            }
            decision.final_labels = all.into_iter().collect();
        }

        // Remove repo-implied labels from final_labels (clean up redundancy)
        if let Some(implied_set) = implied {
            decision
                .final_labels
                .retain(|l| !implied_set.contains(l.as_str()));
        }
    }

    // Pre-create any labels that decisions need but don't exist on the repos yet.
    if !dry_run {
        ensure_labels_exist(gh, &decisions)?;
    }

    // Walk the org tree once for all record_issue_in_node calls.
    let all_nodes = walk_nodes(org_root)?;
    let mut stats = ApplyStats::default();
    let now = chrono::Utc::now().to_rfc3339();

    for (issue, decision) in &decisions {
        let issue_ref_str = format!("{}#{}", issue.repo, issue.number);
        let issue_ref = IssueRef::parse(&issue_ref_str)?;

        // Inquired / stale-with-question decisions: post question as a comment
        if decision.decision == DECISION_INQUIRED
            || (decision.decision == DECISION_STALE && !decision.question.is_empty())
        {
            let label = if decision.decision == DECISION_STALE {
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
                    finalize_decision(
                        conn,
                        decision,
                        &all_nodes,
                        org_root,
                        &issue_ref_str,
                        &issue.title,
                        &now,
                    )?;
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
        if decision.decision == DECISION_STALE {
            if !dry_run {
                finalize_decision(
                    conn,
                    decision,
                    &all_nodes,
                    org_root,
                    &issue_ref_str,
                    &issue.title,
                    &now,
                )?;
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
                finalize_decision(
                    conn,
                    decision,
                    &all_nodes,
                    org_root,
                    &issue_ref_str,
                    &issue.title,
                    &now,
                )?;
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
                finalize_decision(
                    conn,
                    decision,
                    &all_nodes,
                    org_root,
                    &issue_ref_str,
                    &issue.title,
                    &now,
                )?;
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
// Repo-implied labels (from [triage.repo_labels] in armitage.toml)
// ---------------------------------------------------------------------------

/// Read the `[triage.repo_labels]` config: a map from repo to labels that
/// the repo already implies. Returns an empty map on any config error.
fn read_repo_implied_labels(org_root: &Path) -> HashMap<String, HashSet<String>> {
    let org = match Org::open(org_root) {
        Ok(o) => o,
        Err(_) => return HashMap::new(),
    };
    let config = match org.domain_config::<TriageDomain>() {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    config
        .repo_labels
        .into_iter()
        .map(|(repo, labels)| (repo, labels.into_iter().collect()))
        .collect()
}

// ---------------------------------------------------------------------------
// Node label map
// ---------------------------------------------------------------------------

/// Build a map from node path → set of labels that must be present on any
/// issue classified to that node. Includes the node's own labels plus all
/// ancestor labels (e.g. `gemini/logical-mvp` inherits from `gemini/`).
fn build_node_label_map(org_root: &Path) -> HashMap<String, Vec<String>> {
    let nodes = match walk_nodes(org_root) {
        Ok(n) => n,
        Err(_) => return HashMap::new(),
    };

    // First pass: collect direct labels per node
    let mut direct: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &nodes {
        if !entry.node.labels.is_empty() {
            direct.insert(entry.path.clone(), entry.node.labels.clone());
        }
    }

    // Second pass: for each node, walk up ancestors and accumulate labels
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &nodes {
        let mut all_labels = BTreeSet::new();

        // Collect own labels
        for l in &entry.node.labels {
            all_labels.insert(l.clone());
        }

        // Walk ancestors: "a/b/c" → check "a/b", then "a"
        let mut path = entry.path.as_str();
        while let Some(pos) = path.rfind('/') {
            path = &path[..pos];
            if let Some(ancestor_labels) = direct.get(path) {
                for l in ancestor_labels {
                    all_labels.insert(l.clone());
                }
            }
        }

        if !all_labels.is_empty() {
            result.insert(entry.path.clone(), all_labels.into_iter().collect());
        }
    }

    result
}

// ---------------------------------------------------------------------------
// issues.toml tracking
// ---------------------------------------------------------------------------

/// Record an issue in the target node's `issues.toml`, removing it from any
/// other node that may have previously claimed it (e.g. after re-classification).
fn record_issue_in_node(
    all_nodes: &[NodeEntry],
    org_root: &Path,
    node_path: &str,
    issue_ref_str: &str,
    title: Option<&str>,
) -> Result<()> {
    let target_dir = org_root.join(node_path);
    if !target_dir.join("node.toml").exists() {
        eprintln!("  warning: node '{node_path}' has no node.toml, skipping issues.toml write");
        return Ok(());
    }

    // Remove from any other node that currently contains this issue.
    for entry in all_nodes {
        if entry.path == node_path {
            continue;
        }
        let mut file = IssuesFile::read(&entry.dir)?;
        if file.remove(issue_ref_str) {
            file.write(&entry.dir)?;
        }
    }

    // Add to the target node.
    let mut file = IssuesFile::read(&target_dir)?;
    file.add(issue_ref_str.to_string(), title.map(str::to_owned));
    file.write(&target_dir)?;

    Ok(())
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
        if decision.decision == DECISION_STALE || decision.decision == DECISION_INQUIRED {
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

        let missing: Vec<&str> = needed_labels
            .iter()
            .copied()
            .filter(|l| !existing.contains(l))
            .collect();

        if !missing.is_empty() {
            println!("  Creating {} missing label(s) on {repo}...", missing.len());
            for label_name in &missing {
                armitage_github::issue::create_label(gh, repo, label_name, "", None)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use armitage_core::issues::IssuesFile;
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

    // -----------------------------------------------------------------------
    // record_issue_in_node tests
    // -----------------------------------------------------------------------

    /// Helper: create a minimal org tree in a temp dir with given node paths.
    fn make_test_org(node_paths: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("armitage.toml"), "[org]\nname = \"test\"\n").unwrap();
        for p in node_paths {
            let dir = tmp.path().join(p);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("node.toml"),
                format!(
                    "name = \"{}\" \ndescription = \"test node\"\n",
                    p.replace('/', "-")
                ),
            )
            .unwrap();
        }
        tmp
    }

    #[test]
    fn record_adds_issue_to_target_node() {
        let tmp = make_test_org(&["alpha", "beta"]);
        let nodes = walk_nodes(tmp.path()).unwrap();
        record_issue_in_node(
            &nodes,
            tmp.path(),
            "alpha",
            "acme/repo#1",
            Some("First issue"),
        )
        .unwrap();

        let f = IssuesFile::read(&tmp.path().join("alpha")).unwrap();
        assert_eq!(f.len(), 1);
        assert!(f.has("acme/repo#1"));

        // beta should still be empty
        let f2 = IssuesFile::read(&tmp.path().join("beta")).unwrap();
        assert!(f2.is_empty());
    }

    #[test]
    fn record_moves_issue_on_reclassification() {
        let tmp = make_test_org(&["alpha", "beta"]);
        let nodes = walk_nodes(tmp.path()).unwrap();

        // Initially classify to alpha
        record_issue_in_node(&nodes, tmp.path(), "alpha", "acme/repo#1", Some("Issue")).unwrap();
        assert!(
            IssuesFile::read(&tmp.path().join("alpha"))
                .unwrap()
                .has("acme/repo#1")
        );

        // Re-classify to beta
        record_issue_in_node(&nodes, tmp.path(), "beta", "acme/repo#1", Some("Issue")).unwrap();

        // Should be in beta, removed from alpha
        assert!(
            IssuesFile::read(&tmp.path().join("beta"))
                .unwrap()
                .has("acme/repo#1")
        );
        assert!(
            !IssuesFile::read(&tmp.path().join("alpha"))
                .unwrap()
                .has("acme/repo#1")
        );
    }

    #[test]
    fn record_deduplicates_same_node() {
        let tmp = make_test_org(&["alpha"]);
        let nodes = walk_nodes(tmp.path()).unwrap();
        record_issue_in_node(&nodes, tmp.path(), "alpha", "acme/repo#1", Some("A")).unwrap();
        record_issue_in_node(&nodes, tmp.path(), "alpha", "acme/repo#1", Some("A")).unwrap();

        let f = IssuesFile::read(&tmp.path().join("alpha")).unwrap();
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn record_skips_invalid_node_path() {
        let tmp = make_test_org(&["alpha"]);
        let nodes = walk_nodes(tmp.path()).unwrap();
        // "nonexistent" has no node.toml — should not error
        let result = record_issue_in_node(&nodes, tmp.path(), "nonexistent", "acme/repo#1", None);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // build_node_label_map tests
    // -----------------------------------------------------------------------

    /// Helper: create a node with labels
    fn make_test_org_with_labels(nodes: &[(&str, &[&str])]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("armitage.toml"), "[org]\nname = \"test\"\n").unwrap();
        for (path, labels) in nodes {
            let dir = tmp.path().join(path);
            std::fs::create_dir_all(&dir).unwrap();
            let labels_toml = if labels.is_empty() {
                String::new()
            } else {
                let items: Vec<String> = labels.iter().map(|l| format!("\"{}\"", l)).collect();
                format!("labels = [{}]", items.join(", "))
            };
            std::fs::write(
                dir.join("node.toml"),
                format!(
                    "name = \"{}\"\ndescription = \"test\"\n{}\n",
                    path.replace('/', "-"),
                    labels_toml
                ),
            )
            .unwrap();
        }
        tmp
    }

    #[test]
    fn node_label_map_includes_own_labels() {
        let tmp = make_test_org_with_labels(&[
            ("gemini", &["hardware: Gemini"]),
            ("circuit", &["area: circuit"]),
        ]);
        let map = build_node_label_map(tmp.path());
        assert_eq!(map.get("gemini").unwrap(), &["hardware: Gemini"]);
        assert_eq!(map.get("circuit").unwrap(), &["area: circuit"]);
    }

    #[test]
    fn node_label_map_inherits_ancestor_labels() {
        let tmp = make_test_org_with_labels(&[
            ("gemini", &["hardware: Gemini"]),
            ("gemini/logical-mvp", &[]),
            ("gemini/cloud", &["project: cloud"]),
        ]);
        let map = build_node_label_map(tmp.path());

        // logical-mvp has no own labels but inherits from gemini
        let mvp_labels = map.get("gemini/logical-mvp").unwrap();
        assert!(mvp_labels.contains(&"hardware: Gemini".to_string()));

        // cloud has own label + inherits from gemini
        let cloud_labels = map.get("gemini/cloud").unwrap();
        assert!(cloud_labels.contains(&"hardware: Gemini".to_string()));
        assert!(cloud_labels.contains(&"project: cloud".to_string()));
    }

    #[test]
    fn node_label_map_no_labels_no_entry() {
        let tmp = make_test_org_with_labels(&[("plain", &[])]);
        let map = build_node_label_map(tmp.path());
        assert!(!map.contains_key("plain"));
    }
}
