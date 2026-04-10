use std::collections::BTreeSet;

use armitage_core::node::{Node, NodeStatus, Timeline};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum MergeResult {
    Clean(Node),
    Conflict {
        merged: Node,
        conflicts: Vec<FieldConflict>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldConflict {
    pub field: String,
    pub local_value: String,
    pub remote_value: String,
}

#[derive(Debug, Clone)]
pub enum BodyMergeResult {
    Clean(String),
    Conflict { local: String, remote: String },
}

// ---------------------------------------------------------------------------
// Three-way merge for Node
// ---------------------------------------------------------------------------

/// Perform a three-way merge of node fields.
///
/// For each field:
/// - If neither side changed, keep base.
/// - If only local changed, take local.
/// - If only remote changed, take remote.
/// - If both changed to the same value, take it (no conflict).
/// - If both changed to different values, record a conflict (use local as provisional winner).
///
/// Labels use set-based merging (union of additions, propagate removals).
pub fn merge_nodes(base: &Node, local: &Node, remote: &Node) -> MergeResult {
    let mut conflicts: Vec<FieldConflict> = Vec::new();

    // --- name ---
    let name = merge_string_field(
        "name",
        &base.name,
        &local.name,
        &remote.name,
        &mut conflicts,
    );

    // --- description ---
    let description = merge_string_field(
        "description",
        &base.description,
        &local.description,
        &remote.description,
        &mut conflicts,
    );

    // --- github_issue ---
    let github_issue = merge_option_string_field(
        "github_issue",
        base.github_issue.as_deref(),
        local.github_issue.as_deref(),
        remote.github_issue.as_deref(),
        &mut conflicts,
    );

    // --- status ---
    let status = merge_status_field(
        "status",
        &base.status,
        &local.status,
        &remote.status,
        &mut conflicts,
    );

    // --- labels (set-based) ---
    let labels = merge_labels(&base.labels, &local.labels, &remote.labels, &mut conflicts);

    // --- repos ---
    let repos = merge_string_vec_field(
        "repos",
        &base.repos,
        &local.repos,
        &remote.repos,
        &mut conflicts,
    );

    // --- owners ---
    let owners = merge_string_vec_field(
        "owners",
        &base.owners,
        &local.owners,
        &remote.owners,
        &mut conflicts,
    );

    // --- triage_hint ---
    let triage_hint = merge_option_string_field(
        "triage_hint",
        base.triage_hint.as_deref(),
        local.triage_hint.as_deref(),
        remote.triage_hint.as_deref(),
        &mut conflicts,
    );

    // --- team ---
    let team = merge_option_string_field(
        "team",
        base.team.as_deref(),
        local.team.as_deref(),
        remote.team.as_deref(),
        &mut conflicts,
    );

    // --- timeline ---
    let timeline = merge_timeline_field(
        "timeline",
        base.timeline.as_ref(),
        local.timeline.as_ref(),
        remote.timeline.as_ref(),
        &mut conflicts,
    );

    let merged = Node {
        name,
        description,
        triage_hint,
        github_issue,
        labels,
        repos,
        owners,
        team,
        timeline,
        status,
    };

    if conflicts.is_empty() {
        MergeResult::Clean(merged)
    } else {
        MergeResult::Conflict { merged, conflicts }
    }
}

// ---------------------------------------------------------------------------
// Three-way merge for issue body text
// ---------------------------------------------------------------------------

/// Three-way merge of issue body text.
///
/// - If neither changed: clean with base.
/// - If only one changed: clean with that version.
/// - If both changed to the same text: clean.
/// - If both changed differently: conflict.
pub fn merge_issue_body(base: &str, local: &str, remote: &str) -> BodyMergeResult {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => BodyMergeResult::Clean(base.to_string()),
        (true, false) => BodyMergeResult::Clean(local.to_string()),
        (false, true) => BodyMergeResult::Clean(remote.to_string()),
        (true, true) => {
            if local == remote {
                BodyMergeResult::Clean(local.to_string())
            } else {
                BodyMergeResult::Conflict {
                    local: local.to_string(),
                    remote: remote.to_string(),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Field merge helpers
// ---------------------------------------------------------------------------

fn merge_string_field(
    field: &str,
    base: &str,
    local: &str,
    remote: &str,
    conflicts: &mut Vec<FieldConflict>,
) -> String {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => base.to_string(),
        (true, false) => local.to_string(),
        (false, true) => remote.to_string(),
        (true, true) => {
            if local != remote {
                conflicts.push(FieldConflict {
                    field: field.to_string(),
                    local_value: local.to_string(),
                    remote_value: remote.to_string(),
                });
            }
            // Use local as provisional (or agreed value)
            local.to_string()
        }
    }
}

fn merge_option_string_field(
    field: &str,
    base: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    conflicts: &mut Vec<FieldConflict>,
) -> Option<String> {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => base.map(std::string::ToString::to_string),
        (true, false) => local.map(std::string::ToString::to_string),
        (false, true) => remote.map(std::string::ToString::to_string),
        (true, true) => {
            if local != remote {
                conflicts.push(FieldConflict {
                    field: field.to_string(),
                    local_value: local.unwrap_or("(none)").to_string(),
                    remote_value: remote.unwrap_or("(none)").to_string(),
                });
            }
            local.map(std::string::ToString::to_string)
        }
    }
}

fn merge_status_field(
    field: &str,
    base: &NodeStatus,
    local: &NodeStatus,
    remote: &NodeStatus,
    conflicts: &mut Vec<FieldConflict>,
) -> NodeStatus {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => base.clone(),
        (true, false) => local.clone(),
        (false, true) => remote.clone(),
        (true, true) => {
            if local != remote {
                conflicts.push(FieldConflict {
                    field: field.to_string(),
                    local_value: local.to_string(),
                    remote_value: remote.to_string(),
                });
            }
            local.clone()
        }
    }
}

fn merge_string_vec_field(
    field: &str,
    base: &[String],
    local: &[String],
    remote: &[String],
    conflicts: &mut Vec<FieldConflict>,
) -> Vec<String> {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => base.to_vec(),
        (true, false) => local.to_vec(),
        (false, true) => remote.to_vec(),
        (true, true) => {
            if local != remote {
                conflicts.push(FieldConflict {
                    field: field.to_string(),
                    local_value: local.join(", "),
                    remote_value: remote.join(", "),
                });
            }
            local.to_vec()
        }
    }
}

fn merge_timeline_field(
    field: &str,
    base: Option<&Timeline>,
    local: Option<&Timeline>,
    remote: Option<&Timeline>,
    conflicts: &mut Vec<FieldConflict>,
) -> Option<Timeline> {
    let timeline_eq = |a: Option<&Timeline>, b: Option<&Timeline>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.start == b.start && a.end == b.end,
        _ => false,
    };

    let local_changed = !timeline_eq(local, base);
    let remote_changed = !timeline_eq(remote, base);

    match (local_changed, remote_changed) {
        (false, false) => base.cloned(),
        (true, false) => local.cloned(),
        (false, true) => remote.cloned(),
        (true, true) => {
            if !timeline_eq(local, remote) {
                let fmt_tl = |t: Option<&Timeline>| {
                    t.map_or_else(
                        || "(none)".to_string(),
                        |tl| format!("{} — {}", tl.start, tl.end),
                    )
                };
                conflicts.push(FieldConflict {
                    field: field.to_string(),
                    local_value: fmt_tl(local),
                    remote_value: fmt_tl(remote),
                });
            }
            local.cloned()
        }
    }
}

/// Set-based label merge.
///
/// Rules:
/// - Additions on one side that are not removed on the other are applied.
/// - Removals on one side that are not added on the other are applied.
/// - If a label is added by one side and removed by the other: conflict.
/// - Union of non-conflicting additions is the result.
fn merge_labels(
    base: &[String],
    local: &[String],
    remote: &[String],
    conflicts: &mut Vec<FieldConflict>,
) -> Vec<String> {
    let base_set: BTreeSet<&str> = base.iter().map(std::string::String::as_str).collect();
    let local_set: BTreeSet<&str> = local.iter().map(std::string::String::as_str).collect();
    let remote_set: BTreeSet<&str> = remote.iter().map(std::string::String::as_str).collect();

    // Compute changes
    let local_added: BTreeSet<&str> = local_set.difference(&base_set).copied().collect();
    let local_removed: BTreeSet<&str> = base_set.difference(&local_set).copied().collect();
    let remote_added: BTreeSet<&str> = remote_set.difference(&base_set).copied().collect();
    let remote_removed: BTreeSet<&str> = base_set.difference(&remote_set).copied().collect();

    // Conflict: a label was added by one side and removed by the other
    let add_remove_conflicts: BTreeSet<&str> = local_added
        .intersection(&remote_removed)
        .copied()
        .collect::<BTreeSet<_>>()
        .union(
            &remote_added
                .intersection(&local_removed)
                .copied()
                .collect::<BTreeSet<_>>(),
        )
        .copied()
        .collect();

    if !add_remove_conflicts.is_empty() {
        let conflict_labels: Vec<&str> = add_remove_conflicts.iter().copied().collect();
        conflicts.push(FieldConflict {
            field: "labels".to_string(),
            local_value: local.join(", "),
            remote_value: remote.join(", "),
        });
        // Fall back to union for the non-conflicting labels, plus keep local for conflicted ones
        let _ = conflict_labels; // acknowledged
    }

    // Build result set:
    // Start with base, apply non-conflicting changes
    let mut result: BTreeSet<&str> = base_set.clone();

    // Apply local removals (that aren't add-remove conflicts with remote)
    for label in &local_removed {
        if !remote_added.contains(*label) {
            result.remove(*label);
        }
    }

    // Apply remote removals (that aren't add-remove conflicts with local)
    for label in &remote_removed {
        if !local_added.contains(*label) {
            result.remove(*label);
        }
    }

    // Apply local additions
    for label in &local_added {
        result.insert(*label);
    }

    // Apply remote additions
    for label in &remote_added {
        result.insert(*label);
    }

    result
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    fn base_node() -> Node {
        Node {
            name: "test-node".to_string(),
            description: "A test node".to_string(),
            github_issue: Some("owner/repo#1".to_string()),
            labels: vec!["team:alpha".to_string(), "priority:low".to_string()],
            repos: vec!["owner/repo".to_string()],
            owners: vec![],
            team: None,
            triage_hint: None,
            timeline: None,
            status: NodeStatus::Active,
        }
    }

    #[test]
    fn no_changes_returns_clean() {
        let base = base_node();
        let local = base.clone();
        let remote = base.clone();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert_eq!(n.name, "test-node");
        }
    }

    #[test]
    fn local_only_change_takes_local() {
        let base = base_node();
        let mut local = base.clone();
        local.description = "Updated by local".to_string();
        let remote = base.clone();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert_eq!(n.description, "Updated by local");
        }
    }

    #[test]
    fn remote_only_change_takes_remote() {
        let base = base_node();
        let local = base.clone();
        let mut remote = base.clone();
        remote.description = "Updated by remote".to_string();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert_eq!(n.description, "Updated by remote");
        }
    }

    #[test]
    fn both_changed_different_fields_merges() {
        let base = base_node();
        let mut local = base.clone();
        local.description = "Local description".to_string();
        let mut remote = base.clone();
        remote.name = "remote-name".to_string();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert_eq!(n.description, "Local description");
            assert_eq!(n.name, "remote-name");
        }
    }

    #[test]
    fn both_changed_same_field_conflicts() {
        let base = base_node();
        let mut local = base.clone();
        local.description = "Local description".to_string();
        let mut remote = base.clone();
        remote.description = "Remote description".to_string();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Conflict { .. }));
        if let MergeResult::Conflict { conflicts, merged } = result {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].field, "description");
            assert_eq!(conflicts[0].local_value, "Local description");
            assert_eq!(conflicts[0].remote_value, "Remote description");
            // Provisional value should be local
            assert_eq!(merged.description, "Local description");
        }
    }

    #[test]
    fn labels_union_non_conflicting() {
        let base = base_node();
        let mut local = base.clone();
        local.labels = vec![
            "team:alpha".to_string(),
            "priority:low".to_string(),
            "local-only".to_string(),
        ];
        let mut remote = base.clone();
        remote.labels = vec![
            "team:alpha".to_string(),
            "priority:low".to_string(),
            "remote-only".to_string(),
        ];

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert!(n.labels.contains(&"local-only".to_string()));
            assert!(n.labels.contains(&"remote-only".to_string()));
            assert!(n.labels.contains(&"team:alpha".to_string()));
        }
    }

    #[test]
    fn label_removed_one_side_takes_removal() {
        let base = base_node(); // has team:alpha and priority:low
        let mut local = base.clone();
        // local removes priority:low
        local.labels = vec!["team:alpha".to_string()];
        let remote = base.clone(); // remote unchanged

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
        if let MergeResult::Clean(n) = result {
            assert!(n.labels.contains(&"team:alpha".to_string()));
            assert!(!n.labels.contains(&"priority:low".to_string()));
        }
    }

    #[test]
    fn merge_issue_body_both_changed_conflicts() {
        let base = "# Title\n\nOriginal content.";
        let local = "# Title\n\nLocal changes.";
        let remote = "# Title\n\nRemote changes.";

        let result = merge_issue_body(base, local, remote);
        assert!(matches!(result, BodyMergeResult::Conflict { .. }));
        if let BodyMergeResult::Conflict {
            local: lv,
            remote: rv,
        } = result
        {
            assert_eq!(lv, local);
            assert_eq!(rv, remote);
        }
    }

    #[test]
    fn merge_issue_body_one_side_changed() {
        let base = "# Title\n\nOriginal content.";
        let local = "# Title\n\nLocal changes.";
        let remote = base; // unchanged

        let result = merge_issue_body(base, local, remote);
        assert!(matches!(result, BodyMergeResult::Clean(_)));
        if let BodyMergeResult::Clean(body) = result {
            assert_eq!(body, local);
        }
    }
}
