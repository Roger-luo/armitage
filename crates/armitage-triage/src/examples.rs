use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single human-verified classification example used as a few-shot prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageExample {
    /// GitHub issue reference (e.g. "acme/widgets#231")
    pub issue_ref: String,
    /// Issue title
    pub title: String,
    /// Abbreviated issue body (enough context for the LLM)
    pub body_excerpt: String,
    /// The LLM's original suggestion (before user correction), if available
    pub original_node: Option<String>,
    /// The correct node assignment (None means unclassified)
    pub node: Option<String>,
    /// The correct labels
    pub labels: Vec<String>,
    /// Whether this is a tracking/epic issue
    #[serde(default)]
    pub is_tracking_issue: bool,
    /// Whether this issue is stale (references removed/deprecated features)
    #[serde(default)]
    pub is_stale: bool,
    /// Human note explaining *why* this classification is correct
    #[serde(default)]
    pub note: String,
}

/// Container for the TOML file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageExamplesFile {
    #[serde(default, rename = "example")]
    pub examples: Vec<TriageExample>,
}

// ---------------------------------------------------------------------------
// File path
// ---------------------------------------------------------------------------

fn examples_path(org_root: &Path) -> PathBuf {
    org_root
        .join(".armitage")
        .join("triage")
        .join("examples.toml")
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

/// Load examples from the triage examples file.
/// Returns an empty vec if the file doesn't exist.
pub fn load_examples(org_root: &Path) -> Result<Vec<TriageExample>> {
    let path = examples_path(org_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let file: TriageExamplesFile =
        toml::from_str(&content).map_err(|e| Error::Other(format!("parse examples: {e}")))?;
    Ok(file.examples)
}

/// Save examples to the triage examples file.
pub fn save_examples(org_root: &Path, examples: &[TriageExample]) -> Result<()> {
    let file = TriageExamplesFile {
        examples: examples.to_vec(),
    };
    let content = toml::to_string_pretty(&file)
        .map_err(|e| Error::Other(format!("serialize examples: {e}")))?;
    let path = examples_path(org_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}

/// Append a single example, deduplicating by issue_ref.
pub fn append_example(org_root: &Path, example: TriageExample) -> Result<()> {
    let mut examples = load_examples(org_root)?;
    // Replace existing example for same issue, or append
    if let Some(pos) = examples
        .iter()
        .position(|e| e.issue_ref == example.issue_ref)
    {
        examples[pos] = example;
    } else {
        examples.push(example);
    }
    save_examples(org_root, &examples)
}

/// Remove an example by issue_ref. Returns true if an example was found and removed.
pub fn remove_example(org_root: &Path, issue_ref: &str) -> Result<bool> {
    let mut examples = load_examples(org_root)?;
    let before = examples.len();
    examples.retain(|e| e.issue_ref != issue_ref);
    if examples.len() < before {
        save_examples(org_root, &examples)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Prompt building
// ---------------------------------------------------------------------------

/// Build an `## Examples` section for the LLM prompt from human-verified examples.
pub fn build_examples_section(examples: &[TriageExample]) -> String {
    if examples.is_empty() {
        return String::new();
    }

    let mut s = String::from(
        "## Classification Examples\n\
         The following are human-verified classifications from past reviews. \
         Use them as guidance for similar issues.\n\n",
    );

    for (i, ex) in examples.iter().enumerate() {
        s.push_str(&format!("### Example {}\n", i + 1));
        s.push_str(&format!("Issue: {} — {}\n", ex.issue_ref, ex.title));
        if !ex.body_excerpt.is_empty() {
            s.push_str(&format!("Body: {}\n", ex.body_excerpt));
        }
        if let Some(orig) = &ex.original_node {
            s.push_str(&format!("LLM originally suggested: {orig} (INCORRECT)\n"));
        }

        // Show the correct classification as the expected JSON
        let node_json = match &ex.node {
            Some(n) => format!("\"{}\"", n),
            None => "null".to_string(),
        };
        let labels_json: Vec<String> = ex.labels.iter().map(|l| format!("\"{l}\"")).collect();
        s.push_str(&format!(
            "Correct: {{\"suggested_node\": {node_json}, \"suggested_labels\": [{}], \
             \"is_tracking_issue\": {}, \"is_stale\": {}}}\n",
            labels_json.join(", "),
            ex.is_tracking_issue,
            ex.is_stale,
        ));
        if !ex.note.is_empty() {
            s.push_str(&format!("Reason: {}\n", ex.note));
        }
        s.push('\n');
    }

    s
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_examples_toml() {
        let examples = vec![
            TriageExample {
                issue_ref: "owner/repo#1".to_string(),
                title: "Fix the thing".to_string(),
                body_excerpt: "The thing is broken".to_string(),
                original_node: Some("wrong/node".to_string()),
                node: Some("correct/node".to_string()),
                labels: vec!["category: bug".to_string()],
                is_tracking_issue: false,
                is_stale: false,
                note: "This belongs in correct/node because X".to_string(),
            },
            TriageExample {
                issue_ref: "owner/repo#2".to_string(),
                title: "Unclassified issue".to_string(),
                body_excerpt: String::new(),
                original_node: None,
                node: None,
                labels: vec![],
                is_tracking_issue: false,
                is_stale: false,
                note: String::new(),
            },
        ];

        let dir = tempfile::tempdir().unwrap();
        // Create triage subdir
        std::fs::create_dir_all(dir.path().join(".armitage").join("triage")).unwrap();
        save_examples(dir.path(), &examples).unwrap();
        let loaded = load_examples(dir.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].issue_ref, "owner/repo#1");
        assert_eq!(loaded[0].original_node.as_deref(), Some("wrong/node"));
        assert_eq!(loaded[1].node, None);
    }

    #[test]
    fn append_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".armitage").join("triage")).unwrap();
        let ex1 = TriageExample {
            issue_ref: "owner/repo#1".to_string(),
            title: "Original".to_string(),
            body_excerpt: String::new(),
            original_node: None,
            node: Some("a".to_string()),
            labels: vec![],
            is_tracking_issue: false,
            is_stale: false,
            note: String::new(),
        };
        append_example(dir.path(), ex1).unwrap();

        let ex1_updated = TriageExample {
            issue_ref: "owner/repo#1".to_string(),
            title: "Updated".to_string(),
            body_excerpt: String::new(),
            original_node: None,
            node: Some("b".to_string()),
            labels: vec![],
            is_tracking_issue: false,
            is_stale: false,
            note: String::new(),
        };
        append_example(dir.path(), ex1_updated).unwrap();

        let loaded = load_examples(dir.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Updated");
        assert_eq!(loaded[0].node.as_deref(), Some("b"));
    }

    #[test]
    fn build_examples_section_empty() {
        assert_eq!(build_examples_section(&[]), "");
    }

    #[test]
    fn build_examples_section_renders_correction() {
        let examples = vec![TriageExample {
            issue_ref: "owner/repo#1".to_string(),
            title: "Fix bug".to_string(),
            body_excerpt: "broken".to_string(),
            original_node: Some("wrong/node".to_string()),
            node: Some("right/node".to_string()),
            labels: vec!["category: bug".to_string()],
            is_tracking_issue: false,
            is_stale: false,
            note: "Because reasons".to_string(),
        }];
        let section = build_examples_section(&examples);
        assert!(section.contains("Classification Examples"));
        assert!(section.contains("wrong/node"));
        assert!(section.contains("INCORRECT"));
        assert!(section.contains("\"right/node\""));
        assert!(section.contains("Because reasons"));
    }
}
