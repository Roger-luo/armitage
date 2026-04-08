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
