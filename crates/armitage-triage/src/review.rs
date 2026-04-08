use rusqlite::Connection;

use crate::db::{self, ReviewDecision};
use crate::error::Result;

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
// Auto-approve
// ---------------------------------------------------------------------------

pub fn review_auto_approve(conn: &Connection, min_confidence: f64) -> Result<ReviewStats> {
    let pending = db::get_pending_suggestions(conn)?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut stats = ReviewStats::default();

    for (issue, suggestion) in &pending {
        let confidence = suggestion.confidence.unwrap_or(0.0);
        if confidence >= min_confidence {
            let merged = merge_labels(&issue.labels, &suggestion.suggested_labels);
            db::insert_decision(
                conn,
                &ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "approved".to_string(),
                    final_node: suggestion.suggested_node.clone(),
                    final_labels: merged,
                    decided_at: now.clone(),
                    applied_at: None,
                    question: String::new(),
                },
            )?;
            println!(
                "  Auto-approved {}#{} (confidence: {:.0}%)",
                issue.repo,
                issue.number,
                confidence * 100.0
            );
            stats.approved += 1;
        } else {
            stats.skipped += 1;
        }
    }

    println!(
        "\nAuto-approved {} of {} pending suggestions (threshold: {:.0}%)",
        stats.approved,
        pending.len(),
        min_confidence * 100.0
    );
    Ok(stats)
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
