// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ReviewStats {
    pub approved: usize,
    pub rejected: usize,
    pub modified: usize,
    pub stale: usize,
    pub inquired: usize,
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Merge existing issue labels with LLM-suggested labels (set union, preserving order).
pub fn merge_labels(existing: &[String], suggested: &[String]) -> Vec<String> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut merged = Vec::new();
    for label in existing.iter().chain(suggested.iter()) {
        if seen.insert(label.as_str()) {
            merged.push(label.clone());
        }
    }
    merged
}
