use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for the triage pipeline, read from the `[triage]` section
/// of `armitage.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageConfig {
    /// LLM backend: "claude", "codex", "gemini", or "gemini-api"
    #[serde(default)]
    pub backend: Option<String>,
    /// Model to use (e.g. "sonnet", "o3", "gemini-2.5-flash")
    #[serde(default)]
    pub model: Option<String>,
    /// Effort level (claude: low/medium/high/max, codex: low/medium/high/xhigh)
    #[serde(default)]
    pub effort: Option<String>,
    /// Env var name holding the Gemini API key (default: GEMINI_API_KEY)
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Thinking budget for gemini-api (token count, 0-32768)
    #[serde(default)]
    pub thinking_budget: Option<i64>,
    /// Per-command LLM overrides for `triage labels merge`
    #[serde(default)]
    pub labels: Option<TriageLlmOverride>,
    /// Labels implied by each repo. When applying node labels, labels listed
    /// here for the issue's repo are skipped (they are redundant because the
    /// repo itself already implies them).
    ///
    /// Example in armitage.toml:
    /// ```toml
    /// [triage.repo_labels]
    /// "owner/repo" = ["area: circuit"]
    /// ```
    #[serde(default)]
    pub repo_labels: HashMap<String, Vec<String>>,
    /// GitHub Projects v2 configuration for fetching project metadata
    /// (target dates, status, etc.) into the triage DB.
    ///
    /// Example in armitage.toml:
    /// ```toml
    /// [triage.project]
    /// url = "https://github.com/orgs/MyOrg/projects/42"
    /// [triage.project.fields]
    /// target_date = "Target date"
    /// start_date = "Start date"
    /// status = "Status"
    /// ```
    #[serde(default)]
    pub project: Option<ProjectConfig>,
}

/// Configuration for a GitHub Projects v2 board.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Full URL of the project, e.g. `https://github.com/orgs/MyOrg/projects/42`.
    pub url: String,
    /// Map from DB column names (`target_date`, `start_date`, `status`) to the
    /// display names of the corresponding project fields.
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// Optional per-command LLM config that overrides `[triage]` defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageLlmOverride {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub thinking_budget: Option<i64>,
}
