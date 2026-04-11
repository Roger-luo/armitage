use std::fmt::Write as _;
use std::io::Write;

use rusqlite::Connection;

use crate::db::{self, StoredIssue};
use crate::error::{Error, Result};
use crate::examples::TriageExample;
use armitage_core::tree::NodeEntry;
use armitage_labels::def::LabelsFile;
use armitage_labels::schema::LabelSchema;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LlmBackend {
    Claude,
    Codex,
    Gemini,
    GeminiApi,
}

impl std::str::FromStr for LlmBackend {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "gemini-api" => Ok(Self::GeminiApi),
            other => Err(Error::Other(format!(
                "unknown LLM backend: '{other}' (expected 'claude', 'codex', 'gemini', or 'gemini-api')"
            ))),
        }
    }
}

impl std::fmt::Display for LlmBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl LlmBackend {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::GeminiApi => "gemini-api",
        }
    }
}

/// Resolved LLM configuration after merging CLI flags with config file defaults.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Env var name for API key (gemini-api backend). Defaults to GEMINI_API_KEY.
    pub api_key_env: Option<String>,
    /// Thinking budget for gemini-api (token count).
    pub thinking_budget: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub struct PromptCatalog<'a> {
    pub label_schema: &'a LabelSchema,
    pub curated_labels: &'a LabelsFile,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LlmClassification {
    pub suggested_node: Option<String>,
    pub suggested_labels: Vec<String>,
    pub confidence: f64,
    pub reasoning: String,
    #[serde(default)]
    pub is_tracking_issue: bool,
    /// When no existing node fits, the LLM may suggest up to 3 new category names.
    #[serde(default)]
    pub suggested_new_categories: Vec<String>,
    /// True if the issue references features/APIs/components that no longer exist.
    #[serde(default)]
    pub is_stale: bool,
}

// ---------------------------------------------------------------------------
// Question generation for "inquire" decisions
// ---------------------------------------------------------------------------

fn build_question_prompt(
    issue: &StoredIssue,
    nodes: &[NodeEntry],
    catalog: PromptCatalog<'_>,
) -> String {
    let mut prompt = String::from(
        "You are a project triager reviewing GitHub issues. An issue lacks enough information \
         to classify it into the project roadmap or plan it effectively.\n\n\
         Given the roadmap tree and label schema below, generate a short, friendly comment \
         to post on the issue asking the author for the minimum information needed to:\n\
         1. Classify the issue into the right project area\n\
         2. Assess priority and timeline\n\n\
         Keep the question concise (2-4 sentences). Be specific about what's missing — \
         don't ask generic questions. If the issue body is empty or very short, ask for a \
         description of the problem/feature. If the area is ambiguous, ask which component \
         is affected. Only ask about priority/timeline if the issue gives no hints.\n\n",
    );
    prompt.push_str(&build_roadmap_section(nodes));
    prompt.push('\n');
    prompt.push_str(&build_label_schema_section(catalog.label_schema));
    prompt.push('\n');
    prompt.push_str(&build_issue_section(issue));
    prompt.push_str(
        "\n## Task\n\
         Respond with ONLY the comment text to post on the issue. \
         No JSON, no markdown fences, no preamble — just the comment body.\n",
    );
    prompt
}

/// Generate a clarification question for an issue using the LLM.
pub fn generate_question(
    issue: &StoredIssue,
    nodes: &[NodeEntry],
    catalog: PromptCatalog<'_>,
    config: &LlmConfig,
) -> Result<String> {
    let prompt = build_question_prompt(issue, nodes, catalog);
    let raw = invoke_llm(config, &prompt)?;
    Ok(raw.trim().to_string())
}

fn build_stale_question_prompt(issue: &StoredIssue, reasoning: &str) -> String {
    let mut prompt = String::from(
        "You are a project maintainer checking whether a GitHub issue is still relevant.\n\n\
         The issue was flagged as potentially stale during triage — it may reference features, \
         APIs, or components that have been removed, deprecated, or substantially changed.\n\n\
         Generate a short, friendly comment (2-3 sentences) to post on the issue asking the \
         author whether it is still relevant or can be closed. Be specific about *why* it \
         looks stale based on the triage reasoning below — don't just say \"is this still \
         relevant?\". If the issue might still be useful as backlog, acknowledge that.\n\n",
    );
    prompt.push_str(&build_issue_section(issue));
    let _ = writeln!(prompt, "\n## Triage reasoning\n{reasoning}");
    prompt.push_str(
        "\n## Task\n\
         Respond with ONLY the comment text to post on the issue. \
         No JSON, no markdown fences, no preamble — just the comment body.\n",
    );
    prompt
}

/// Generate a staleness inquiry for an issue using the LLM.
pub fn generate_stale_question(
    issue: &StoredIssue,
    reasoning: &str,
    config: &LlmConfig,
) -> Result<String> {
    let prompt = build_stale_question_prompt(issue, reasoning);
    let raw = invoke_llm(config, &prompt)?;
    Ok(raw.trim().to_string())
}

// ---------------------------------------------------------------------------
// Prompt building
// ---------------------------------------------------------------------------

fn build_roadmap_section(nodes: &[NodeEntry]) -> String {
    use std::collections::HashSet;

    // Pre-compute parent set in O(N) instead of O(N²) per-entry scanning.
    let parent_set: HashSet<&str> = nodes
        .iter()
        .filter_map(|e| e.path.rsplit_once('/').map(|(parent, _)| parent))
        .collect();

    let mut s = String::from("## Roadmap Tree\n");
    for entry in nodes {
        let kind = if parent_set.contains(entry.path.as_str()) {
            "parent"
        } else {
            "leaf"
        };
        let _ = write!(
            s,
            "- {} [{}] ({}) — {}: {}",
            entry.path, entry.node.status, kind, entry.node.name, entry.node.description
        );
        if !entry.node.repos.is_empty() {
            let _ = write!(s, "  [repos: {}]", entry.node.repos.join(", "));
        }
        if let Some(ref hint) = entry.node.triage_hint {
            let _ = write!(s, "  [hint: {hint}]");
        }
        s.push('\n');
    }
    s
}

fn build_label_schema_section(schema: &LabelSchema) -> String {
    let mut s = String::from("## Label Schema\n");
    if schema.prefixes.is_empty() {
        s.push_str("No label prefixes defined.\n");
    } else {
        for prefix in &schema.prefixes {
            let _ = writeln!(
                s,
                "- \"{}\" ({}): {}",
                prefix.prefix,
                prefix.category,
                prefix.examples.join(", "),
            );
        }
    }
    s
}

fn build_curated_labels_section(labels: &LabelsFile) -> String {
    let mut s = String::from("## Curated Labels\n");
    if labels.labels.is_empty() {
        s.push_str("No curated labels defined.\n");
    } else {
        for label in &labels.labels {
            let _ = writeln!(s, "- {}: {}", label.name, label.description);
        }
    }
    s
}

fn build_issue_section(issue: &StoredIssue) -> String {
    let labels = if issue.labels.is_empty() {
        "none".to_string()
    } else {
        issue.labels.join(", ")
    };

    let body = if issue.body.len() > 4000 {
        // Find a valid char boundary at or before 4000
        let mut end = 4000;
        while !issue.body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &issue.body[..end])
    } else {
        issue.body.clone()
    };

    let mut section = format!(
        "## Issue\nRepo: {}  Number: #{}\nState: {}\nTitle: {}\nCurrent Labels: {}\n",
        issue.repo, issue.number, issue.state, issue.title, labels,
    );
    if issue.sub_issues_count > 0 {
        let _ = writeln!(
            section,
            "Sub-issues: {} (this is likely a tracking/epic issue)",
            issue.sub_issues_count,
        );
    }
    let _ = writeln!(section, "Body:\n{body}");
    section
}

fn build_classification_guidelines() -> String {
    "\n## Classification Guidelines\n\
     - Prefer the MOST SPECIFIC (leaf) node that matches the issue.\n\
     - Parent nodes should only be used when an issue genuinely spans multiple children \
       or no leaf node is a good match.\n\
     - Issues with sub-issues listed above are tracking/epic issues — these typically belong \
       at a parent (non-leaf) level since they coordinate work across sub-areas.\n\
     - Set is_tracking_issue to true if the issue is an epic, tracking issue, or \
       umbrella issue that coordinates sub-tasks.\n\
     - **Repo and branch affinity:** Nodes may list associated repos (with optional `@branch` \
       qualifiers, e.g. `owner/repo@main`, `owner/repo@feature`). When classifying, strongly \
       prefer nodes whose associated repos match the issue's repo. If multiple nodes share \
       the same repo but differ by branch, use the issue content to determine which codebase \
       (branch) the issue is about. A node scoped to a specific branch covers only code on \
       that branch. A node without a branch qualifier covers the repo's default branch.\n\
     - **Distinguish active work from planned/new projects.** Nodes covering new or planned work \
       (e.g. a rewrite or new implementation) should only match issues that explicitly reference \
       that new work. Issues about existing behavior almost certainly belong to nodes covering \
       the current/legacy codebase, even when keyword overlap with a new-project node is high.\n\
     - If no existing node fits well, set suggested_node to null AND suggest up to 3 \
       potential new category names in suggested_new_categories. Use path-style names \
       (e.g. \"ops/ci\", \"compiler/optimizations\", \"docs\"). Leave empty if an existing node fits.\n\
     - Set is_stale to true if the issue references features, APIs, modules, or components \
       that have been removed, deprecated, or no longer exist in the project. Stale issues \
       are outdated and should typically have suggested_node set to null.\n\
     - **Labels are additive only.** The issue's Current Labels were applied by humans and are \
       authoritative. In suggested_labels, include ONLY labels that are missing and should be \
       added. Do NOT repeat labels the issue already has. If the existing labels are already \
       sufficient, return an empty suggested_labels array.\n\
     - **Cite examples in reasoning.** If your classification is informed by any of the \
       Classification Examples above, mention which example(s) by their issue reference \
       (e.g. \"similar to owner/repo#42\") in your reasoning text. This helps reviewers \
       understand whether the classification was based on precedent.\n\
     - **When uncertain, prefer inquiry over guessing.** If no Classification Example \
       provides clear precedent and you are unsure about the correct node or labels, \
       set confidence low and explain what information is missing in the reasoning. \
       The preference order for low-confidence situations is:\n\
       1. **Inquiry** — if the issue lacks enough context to classify confidently, set \
          confidence below 0.4 and state in reasoning what clarification is needed from \
          the issue author.\n\
       2. **Stale** — if the issue appears outdated (references removed features/APIs), \
          set is_stale to true.\n\
       3. **Best guess** — only guess a node when you have some signal, even if weak. \
          Use a moderate confidence (0.4–0.6) and explain the uncertainty in reasoning.\n\
       Do NOT assign a high confidence score when there is no precedent or clear match.\n"
        .to_string()
}

fn build_prompt(
    issue: &StoredIssue,
    nodes: &[NodeEntry],
    catalog: PromptCatalog<'_>,
    examples: &[TriageExample],
) -> String {
    let mut prompt = String::from(
        "You are classifying GitHub issues into a project roadmap.\n\n\
         Given the roadmap tree and label schema below, determine which node this issue belongs to \
         and what labels it should have.\n\n",
    );
    prompt.push_str(&build_roadmap_section(nodes));
    prompt.push('\n');
    prompt.push_str(&build_label_schema_section(catalog.label_schema));
    prompt.push('\n');
    prompt.push_str(&build_curated_labels_section(catalog.curated_labels));
    prompt.push('\n');
    let examples_section = crate::examples::build_examples_section(examples);
    if !examples_section.is_empty() {
        prompt.push_str(&examples_section);
        prompt.push('\n');
    }
    prompt.push_str(&build_issue_section(issue));
    prompt.push_str(&build_classification_guidelines());
    prompt.push_str(
        "\n## Task\n\
         Respond with ONLY valid JSON (no markdown fences, no extra text):\n\
         {\"suggested_node\": \"path/to/node or null\", \"suggested_labels\": [\"label1\"], \
         \"confidence\": 0.85, \"is_tracking_issue\": false, \"is_stale\": false, \
         \"suggested_new_categories\": [], \"reasoning\": \"...\"}\n\
         \n\
         If the issue does not belong to any node, set suggested_node to null.\n\
         Confidence should be 0.0 to 1.0.\n",
    );
    prompt
}

fn build_batch_prompt(
    issues: &[StoredIssue],
    nodes: &[NodeEntry],
    catalog: PromptCatalog<'_>,
    examples: &[TriageExample],
) -> String {
    let mut prompt = String::from(
        "You are classifying GitHub issues into a project roadmap.\n\n\
         Given the roadmap tree and label schema below, determine which node each issue belongs to \
         and what labels it should have.\n\n",
    );
    prompt.push_str(&build_roadmap_section(nodes));
    prompt.push('\n');
    prompt.push_str(&build_label_schema_section(catalog.label_schema));
    prompt.push('\n');
    prompt.push_str(&build_curated_labels_section(catalog.curated_labels));
    prompt.push('\n');
    let examples_section = crate::examples::build_examples_section(examples);
    if !examples_section.is_empty() {
        prompt.push_str(&examples_section);
        prompt.push('\n');
    }

    for (i, issue) in issues.iter().enumerate() {
        let _ = writeln!(prompt, "## Issue {}", i + 1);
        prompt.push_str(&build_issue_section(issue));
        prompt.push('\n');
    }

    prompt.push_str(&build_classification_guidelines());
    prompt.push_str(
        "\n## Task\n\
         Respond with ONLY a valid JSON array (no markdown fences, no extra text).\n\
         One object per issue, in the same order:\n\
         [{\"suggested_node\": \"path/to/node or null\", \"suggested_labels\": [\"label1\"], \
         \"confidence\": 0.85, \"is_tracking_issue\": false, \"is_stale\": false, \
         \"suggested_new_categories\": [], \"reasoning\": \"...\"}]\n\
         \n\
         If an issue does not belong to any node, set suggested_node to null.\n\
         Confidence should be 0.0 to 1.0.\n",
    );
    prompt
}

// ---------------------------------------------------------------------------
// LLM invocation
// ---------------------------------------------------------------------------

// Suppresses the standalone LLM spinner when an outer progress bar is active
// (e.g. `triage_issues` with its own MultiProgress).
thread_local! {
    static SUPPRESS_SPINNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Create a spinner for LLM wait time. Returns `None` if stderr is not a terminal,
/// in non-interactive mode, or when an outer progress bar is managing the display.
fn llm_spinner(backend: &str) -> Option<indicatif::ProgressBar> {
    if SUPPRESS_SPINNER.with(std::cell::Cell::get) {
        return None;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        return None;
    }
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg} [{elapsed}]")
            .unwrap(),
    );
    pb.set_message(format!("Waiting for {backend}..."));
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    Some(pb)
}

fn invoke_gemini_api(config: &LlmConfig, prompt: &str) -> Result<String> {
    let env_var = config.api_key_env.as_deref().unwrap_or("GEMINI_API_KEY");

    // Resolve API key: .armitage/secrets.toml > env var
    let api_key = armitage_core::tree::find_org_root(&std::env::current_dir()?)
        .ok()
        .and_then(|root| {
            armitage_core::secrets::read_secret(&root, "gemini-api-key")
                .ok()
                .flatten()
        })
        .or_else(|| std::env::var(env_var).ok())
        .ok_or_else(|| {
            Error::LlmInvocation(format!(
                "Gemini API key not found. Run `armitage config set-secret gemini-api-key` \
                 or set {env_var} env var"
            ))
        })?;

    let model = config.model.as_deref().unwrap_or("gemini-2.5-flash");
    let url =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");

    let mut gen_config = serde_json::json!({});
    if let Some(budget) = config.thinking_budget {
        gen_config["thinkingConfig"] = serde_json::json!({ "thinkingBudget": budget });
    }

    let body = serde_json::json!({
        "contents": [{ "parts": [{ "text": prompt }] }],
        "generationConfig": gen_config,
    });

    tracing::debug!(
        model = model,
        url = url.as_str(),
        api_key_env = env_var,
        thinking_budget = config.thinking_budget,
        prompt_bytes = prompt.len(),
        "invoking Gemini API"
    );
    tracing::trace!(prompt = prompt, "full LLM prompt");

    let spinner = llm_spinner(model);
    let start = std::time::Instant::now();
    let body_str = serde_json::to_string(&body)
        .map_err(|e| Error::LlmInvocation(format!("failed to serialize request: {e}")))?;

    let resp = ureq::post(&url)
        .header("x-goog-api-key", &api_key)
        .content_type("application/json")
        .send(&body_str)
        .map_err(|e| Error::LlmInvocation(format!("Gemini API request failed: {e}")))?;

    let status = resp.status().as_u16();
    let raw = std::io::read_to_string(resp.into_body().as_reader())
        .map_err(|e| Error::LlmInvocation(format!("failed to read Gemini API response: {e}")))?;
    let elapsed = start.elapsed();

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    tracing::debug!(
        status = status,
        response_bytes = raw.len(),
        elapsed_ms = elapsed.as_millis() as u64,
        "Gemini API response"
    );

    if status != 200 {
        return Err(Error::LlmInvocation(format!(
            "Gemini API returned {status}: {raw}"
        )));
    }

    // Extract text from candidates[0].content.parts[0].text
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| Error::LlmInvocation(format!("invalid Gemini API JSON: {e}")))?;

    // Collect text from all parts, skipping thought parts
    let parts = parsed
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            Error::LlmInvocation("Gemini API response missing candidates[0].content.parts".into())
        })?;

    let text: String = parts
        .iter()
        .filter(|p| {
            !p.get("thought")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        return Err(Error::LlmInvocation(
            "Gemini API response contained no text".into(),
        ));
    }

    Ok(text)
}

pub(crate) fn invoke_llm(config: &LlmConfig, prompt: &str) -> Result<String> {
    use std::process::{Command, Stdio};

    if matches!(config.backend, LlmBackend::GeminiApi) {
        return invoke_gemini_api(config, prompt);
    }

    // Whether the prompt is passed via stdin (true) or as a CLI argument (false).
    let uses_stdin;

    let mut cmd = match config.backend {
        LlmBackend::Claude => {
            // claude -p - : reads prompt from stdin
            uses_stdin = true;
            let mut c = Command::new("claude");
            c.args(["-p", "-", "--output-format", "json"]);
            if let Some(model) = &config.model {
                c.args(["--model", model]);
            }
            if let Some(effort) = &config.effort {
                c.args(["--effort", effort]);
            }
            c
        }
        LlmBackend::Codex => {
            // codex exec - : reads prompt from stdin
            uses_stdin = true;
            let mut c = Command::new("codex");
            c.args(["exec", "-"]);
            if let Some(model) = &config.model {
                c.args(["-c", &format!("model=\"{model}\"")]);
            }
            if let Some(effort) = &config.effort {
                c.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
            }
            c
        }
        LlmBackend::Gemini => {
            // gemini -p <prompt> : prompt passed as argument value
            // (no stdin, no effort flag)
            uses_stdin = false;
            let mut c = Command::new("gemini");
            c.args(["-p", prompt, "--output-format", "json"]);
            if let Some(model) = &config.model {
                c.args(["--model", model]);
            }
            c
        }
        LlmBackend::GeminiApi => unreachable!("handled above"),
    };

    tracing::debug!(
        backend = config.backend.name(),
        model = config.model.as_deref().unwrap_or("default"),
        effort = config.effort.as_deref().unwrap_or("default"),
        prompt_bytes = prompt.len(),
        uses_stdin = uses_stdin,
        command = ?cmd,
        "invoking LLM via shell CLI"
    );
    tracing::trace!(prompt = prompt, "full LLM prompt");

    if uses_stdin {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let spinner = llm_spinner(config.backend.name());
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().map_err(|e| {
        Error::LlmInvocation(format!("failed to spawn {}: {e}", config.backend.name()))
    })?;
    tracing::debug!(
        pid = child.id(),
        "LLM process spawned, waiting for response..."
    );

    if uses_stdin && let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| Error::LlmInvocation(format!("failed to write to stdin: {e}")))?;
    }

    let output = child.wait_with_output().map_err(|e| {
        Error::LlmInvocation(format!("failed to wait for {}: {e}", config.backend.name()))
    })?;
    let elapsed = start.elapsed();

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    tracing::debug!(
        backend = config.backend.name(),
        exit_code = output.status.code(),
        stdout_bytes = output.stdout.len(),
        stderr_bytes = output.stderr.len(),
        elapsed_ms = elapsed.as_millis() as u64,
        "LLM process finished"
    );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!(stderr = %stderr, "LLM stderr output");
        return Err(Error::LlmInvocation(format!(
            "{} exited with {}: {stderr}",
            config.backend.name(),
            output.status
        )));
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| Error::LlmInvocation(format!("invalid UTF-8 in output: {e}")))?;

    unwrap_cli_output(&config.backend, &raw)
}

fn unwrap_cli_output(backend: &LlmBackend, raw: &str) -> Result<String> {
    match backend {
        LlmBackend::Claude => {
            // Claude CLI with --output-format json wraps the response in a metadata
            // object: {"type":"result", "result":"<actual LLM text>", ...}.
            // Extract the inner `result` string so callers get the raw LLM output.
            if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(raw)
                && let Some(inner) = wrapper.get("result").and_then(|v| v.as_str())
            {
                return Ok(inner.to_string());
            }
            Ok(raw.to_string())
        }
        LlmBackend::Gemini => {
            let wrapper: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
                Error::LlmInvocation(format!("invalid Gemini CLI JSON output: {e}"))
            })?;
            wrapper
                .get("response")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
                .ok_or_else(|| {
                    Error::LlmInvocation(
                        "Gemini CLI returned JSON without a response field".to_string(),
                    )
                })
        }
        LlmBackend::Codex | LlmBackend::GeminiApi => Ok(raw.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

fn parse_classification(raw: &str) -> Result<LlmClassification> {
    let trimmed = raw.trim();

    // Try direct parse
    if let Ok(c) = serde_json::from_str::<LlmClassification>(trimmed) {
        return Ok(c);
    }

    // Try extracting from markdown code fence
    if let Some(json) = extract_json_block(trimmed)
        && let Ok(c) = serde_json::from_str::<LlmClassification>(&json)
    {
        return Ok(c);
    }

    // Try finding a JSON object in the text
    if let Some(json) = extract_json_object(trimmed)
        && let Ok(c) = serde_json::from_str::<LlmClassification>(&json)
    {
        return Ok(c);
    }

    Err(Error::LlmParse(format!(
        "could not parse LLM response as classification: {trimmed}"
    )))
}

fn parse_batch_classifications(raw: &str) -> Result<Vec<LlmClassification>> {
    let trimmed = raw.trim();

    if let Ok(v) = serde_json::from_str::<Vec<LlmClassification>>(trimmed) {
        return Ok(v);
    }

    if let Some(json) = extract_json_block(trimmed)
        && let Ok(v) = serde_json::from_str::<Vec<LlmClassification>>(&json)
    {
        return Ok(v);
    }

    Err(Error::LlmParse(format!(
        "could not parse LLM batch response: {trimmed}"
    )))
}

pub(crate) fn extract_json_block(text: &str) -> Option<String> {
    // Look for ```json ... ``` or ``` ... ```
    let start_markers = ["```json\n", "```json\r\n", "```\n", "```\r\n"];
    for marker in &start_markers {
        if let Some(start) = text.find(marker) {
            let content_start = start + marker.len();
            if let Some(end) = text[content_start..].find("```") {
                return Some(text[content_start..content_start + end].to_string());
            }
        }
    }
    None
}

pub(crate) fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=(start + i)].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Label reconciliation (LLM-driven merge suggestions)
// ---------------------------------------------------------------------------

fn build_reconcile_prompt(
    local: &LabelsFile,
    session: &crate::label_import::LabelImportSession,
    schema: &LabelSchema,
) -> String {
    use std::collections::BTreeSet;
    use std::fmt::Write;

    let mut prompt = String::new();
    writeln!(
        prompt,
        "You are analyzing a label catalog for a project management system. \
         Below are all labels (existing local labels and newly fetched remote labels). \
         Do two things:\n\
         \n\
         1. Identify groups of 2 or more labels that are semantically similar or overlapping \
         and should be consolidated into a single label. Only suggest merges where labels \
         genuinely overlap in meaning — do not merge labels that are merely related but \
         serve distinct purposes.\n\
         \n\
         2. Identify individual labels whose name or description does not follow the naming \
         convention below, and suggest reformatted versions. Include these as single-label \
         groups in the same merge_groups array.\n"
    )
    .unwrap();

    writeln!(prompt, "## Labels\n").unwrap();

    let mut seen = BTreeSet::new();
    let pinned_names: BTreeSet<&str> = local
        .labels
        .iter()
        .filter(|l| l.pinned)
        .map(|l| l.name.as_str())
        .collect();
    for label in &local.labels {
        seen.insert(label.name.clone());
        if label.pinned {
            // Pinned labels are excluded from reconciliation
            continue;
        }
        writeln!(prompt, "- {} — {} [local]", label.name, label.description).unwrap();
    }

    for candidate in &session.candidates {
        if candidate.status == crate::label_import::CandidateStatus::Unchanged {
            continue;
        }
        if pinned_names.contains(candidate.name.as_str()) {
            continue;
        }
        if let Some(v) = candidate.remote_variants.first() {
            if seen.contains(&candidate.name) {
                // Drifted — show remote version alongside local
                writeln!(
                    prompt,
                    "- {} — {} [remote, differs from local]",
                    candidate.name, v.description
                )
                .unwrap();
            } else {
                seen.insert(candidate.name.clone());
                writeln!(prompt, "- {} — {} [remote]", candidate.name, v.description).unwrap();
            }
        }
    }

    writeln!(
        prompt,
        "\nFor each group of similar labels, explain why they overlap and suggest 2-3 \
         consolidated label alternatives (each with a name AND description). \
         Also pick one recommended label (from your suggestions or the existing labels) \
         that best represents the merged concept."
    )
    .unwrap();

    // Include structured prefix categories so the LLM knows the taxonomy
    if !schema.prefixes.is_empty() {
        writeln!(prompt, "\n## Label categories (prefixes)\n").unwrap();
        writeln!(
            prompt,
            "Labels are organized into these categories. Do NOT merge labels across \
             different categories unless they are truly duplicates:"
        )
        .unwrap();
        for prefix in &schema.prefixes {
            writeln!(
                prompt,
                "- \"{}\" ({}): e.g. {}",
                prefix.prefix,
                prefix.category,
                prefix.examples.join(", ")
            )
            .unwrap();
        }
    }

    // Label naming convention from config, or default
    if let Some(style) = &schema.style {
        writeln!(
            prompt,
            "\nLabel naming convention — follow this style strictly:"
        )
        .unwrap();
        writeln!(prompt, "{}", style.convention).unwrap();
        if !style.examples.is_empty() {
            writeln!(prompt, "Examples:").unwrap();
            for ex in &style.examples {
                writeln!(prompt, "  - {} → \"{}\"", ex.name, ex.description).unwrap();
            }
        }
    } else {
        writeln!(
            prompt,
            "\nLabel naming convention — follow this style strictly:\n\
             - Name format: <Prefix>-<Name> where the prefix is an uppercase abbreviation \
             and the name is capitalized or hyphenated lowercase.\n\
             - Description format: <Expanded prefix>: <description>.\n\
             Examples:\n\
               - A-Circuit → \"Area: quantum circuit related issues\"\n\
               - P-high → \"Priority: high priority issues\"\n\
               - C-bug → \"Category: this is a bug\""
        )
        .unwrap();
    }

    writeln!(
        prompt,
        "\nRespond with JSON only:\n\
         {{\n  \
           \"merge_groups\": [\n    \
             {{\n      \
               \"labels\": [\"label-a\", \"label-b\"],\n      \
               \"reason\": \"Why these labels overlap\",\n      \
               \"suggestions\": [\n        \
                 {{\"name\": \"A-Example\", \"description\": \"Area: clear combined description\"}}\n      \
               ],\n      \
               \"recommended\": \"A-Example\"\n    \
             }}\n  \
           ]\n\
         }}\n\
         \n\
         If no labels should be merged, return: {{\"merge_groups\": []}}"
    )
    .unwrap();

    prompt
}

fn parse_reconcile_response(raw: &str) -> Result<crate::label_import::ReconcileResponse> {
    let trimmed = raw.trim();

    if let Ok(r) = serde_json::from_str::<crate::label_import::ReconcileResponse>(trimmed) {
        return Ok(r);
    }

    if let Some(json) = extract_json_block(trimmed)
        && let Ok(r) = serde_json::from_str::<crate::label_import::ReconcileResponse>(&json)
    {
        return Ok(r);
    }

    if let Some(json) = extract_json_object(trimmed)
        && let Ok(r) = serde_json::from_str::<crate::label_import::ReconcileResponse>(&json)
    {
        return Ok(r);
    }

    Err(Error::LlmParse(format!(
        "could not parse reconcile response: {trimmed}"
    )))
}

pub fn reconcile_labels(
    local: &LabelsFile,
    session: &crate::label_import::LabelImportSession,
    config: &LlmConfig,
    schema: &LabelSchema,
) -> Result<crate::label_import::ReconcileResponse> {
    let prompt = build_reconcile_prompt(local, session, schema);
    tracing::info!(
        local_labels = local.labels.len(),
        session_candidates = session.candidates.len(),
        "reconciling labels via LLM"
    );
    let raw = invoke_llm(config, &prompt)?;
    let mut response = parse_reconcile_response(&raw)?;

    // Augment LLM results with deterministic near-duplicate detection.
    // Catches remote labels whose bare name matches a local prefixed label
    // (e.g. "flux" → "area: FLUX") that the LLM may have missed.
    let extra = find_prefix_duplicates(local, session, &response);
    if !extra.is_empty() {
        tracing::debug!(
            count = extra.len(),
            "found additional prefix-match duplicates"
        );
        response.merge_groups.extend(extra);
    }

    tracing::debug!(
        merge_groups = response.merge_groups.len(),
        "reconciliation complete"
    );
    Ok(response)
}

/// Find remote-only labels whose bare name (case-insensitive) matches the suffix
/// of a local prefixed label. For example, remote "flux" matches local "area: FLUX".
/// Only returns groups not already covered by the LLM response.
fn find_prefix_duplicates(
    local: &LabelsFile,
    session: &crate::label_import::LabelImportSession,
    llm_response: &crate::label_import::ReconcileResponse,
) -> Vec<crate::label_import::MergeGroup> {
    use std::collections::BTreeSet;

    // Collect all labels already mentioned in LLM merge groups
    let already_covered: BTreeSet<String> = llm_response
        .merge_groups
        .iter()
        .flat_map(|g| g.labels.iter().cloned())
        .collect();

    // Build a map: lowercase suffix → local label name
    // e.g. "flux" → "area: FLUX", "bug" → "category: bug"
    let mut suffix_to_local: std::collections::BTreeMap<String, &str> =
        std::collections::BTreeMap::new();
    for label in &local.labels {
        if label.pinned {
            continue;
        }
        // Extract suffix after "prefix: " pattern
        if let Some((_prefix, suffix)) = label.name.split_once(": ") {
            suffix_to_local
                .entry(suffix.to_lowercase())
                .or_insert(label.name.as_str());
        }
    }

    let mut groups = Vec::new();
    for candidate in &session.candidates {
        // Only look at remote-only labels (new ones not already in local)
        if local.labels.iter().any(|l| l.name == candidate.name) {
            continue;
        }
        if already_covered.contains(&candidate.name) {
            continue;
        }
        // Check if the candidate's name matches a suffix of a local label
        let bare_lower = candidate.name.to_lowercase();
        if let Some(&local_name) = suffix_to_local.get(&bare_lower) {
            let local_label = local.labels.iter().find(|l| l.name == local_name);
            groups.push(crate::label_import::MergeGroup {
                labels: vec![candidate.name.clone(), local_name.to_string()],
                reason: format!(
                    "Remote label '{}' is an unprefixed duplicate of local '{}'.",
                    candidate.name, local_name
                ),
                suggestions: vec![crate::label_import::LabelSuggestion {
                    name: local_name.to_string(),
                    description: local_label
                        .map(|l| l.description.clone())
                        .unwrap_or_default(),
                }],
                recommended: Some(local_name.to_string()),
            });
        }
    }

    groups
}

/// Ask LLM to refine label suggestions based on user feedback.
/// Returns new suggestions (name + description pairs).
pub fn refine_label_suggestions(
    config: &LlmConfig,
    schema: &LabelSchema,
    labels: &[String],
    previous_suggestions: &[crate::label_import::LabelSuggestion],
    user_feedback: &str,
) -> Result<Vec<crate::label_import::LabelSuggestion>> {
    use std::fmt::Write;

    #[derive(serde::Deserialize)]
    struct RefineResponseInner {
        suggestions: Vec<crate::label_import::LabelSuggestion>,
    }

    let mut prompt = String::new();
    writeln!(prompt, "You previously suggested merging these labels:").unwrap();
    for name in labels {
        writeln!(prompt, "  - {name}").unwrap();
    }
    writeln!(prompt, "\nYour previous suggestions were:").unwrap();
    for s in previous_suggestions {
        writeln!(prompt, "  - {} — {}", s.name, s.description).unwrap();
    }
    writeln!(
        prompt,
        "\nThe user wants changes: {user_feedback}\n\n\
         Provide 2-3 new label suggestions (each with name and description)."
    )
    .unwrap();

    if let Some(style) = &schema.style {
        writeln!(prompt, "\nLabel naming convention:").unwrap();
        writeln!(prompt, "{}", style.convention).unwrap();
        if !style.examples.is_empty() {
            writeln!(prompt, "Examples:").unwrap();
            for ex in &style.examples {
                writeln!(prompt, "  - {} → \"{}\"", ex.name, ex.description).unwrap();
            }
        }
    }

    writeln!(
        prompt,
        "\nRespond with JSON only:\n\
         {{\"suggestions\": [{{\"name\": \"X\", \"description\": \"Y\"}}]}}"
    )
    .unwrap();

    tracing::debug!(
        user_feedback = user_feedback,
        "refining label suggestions via LLM"
    );
    let raw = invoke_llm(config, &prompt)?;
    let trimmed = raw.trim();

    // Parse: try direct, then code fence, then embedded object
    let parsed = serde_json::from_str::<RefineResponseInner>(trimmed)
        .ok()
        .or_else(|| {
            extract_json_block(trimmed)
                .and_then(|j| serde_json::from_str::<RefineResponseInner>(&j).ok())
        })
        .or_else(|| {
            extract_json_object(trimmed)
                .and_then(|j| serde_json::from_str::<RefineResponseInner>(&j).ok())
        });

    match parsed {
        Some(r) => Ok(r.suggestions),
        None => Err(Error::LlmParse(format!(
            "could not parse refinement response: {trimmed}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Category refinement (LLM-driven consolidation of raw suggestions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RefinedGroup {
    pub raw_suggestions: Vec<String>,
    pub covered_by: Option<String>,
    pub proposed_path: Option<String>,
    pub proposed_name: Option<String>,
    pub proposed_description: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RefineResponse {
    pub groups: Vec<RefinedGroup>,
}

/// Intermediate struct for deserialization — accepts both `String` and `[String]`
/// for proposed_path/name/description, then normalizes to individual `RefinedGroup`s.
#[derive(Debug, Clone, serde::Deserialize)]
struct RawRefinedGroup {
    raw_suggestions: Vec<String>,
    covered_by: Option<String>,
    #[serde(deserialize_with = "deserialize_string_or_vec_opt")]
    proposed_path: Option<Vec<String>>,
    #[serde(deserialize_with = "deserialize_string_or_vec_opt")]
    proposed_name: Option<Vec<String>>,
    #[serde(deserialize_with = "deserialize_string_or_vec_opt")]
    proposed_description: Option<Vec<String>>,
    reason: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RawRefineResponse {
    groups: Vec<RawRefinedGroup>,
}

fn deserialize_string_or_vec_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrVecVisitor;
    impl<'de> de::Visitor<'de> for StringOrVecVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("null, a string, or an array of strings")
        }

        fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(vec![v.to_owned()]))
        }

        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
            Ok(Some(vec![v]))
        }

        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s);
            }
            if v.is_empty() { Ok(None) } else { Ok(Some(v)) }
        }
    }

    deserializer.deserialize_any(StringOrVecVisitor)
}

/// Expand a `RawRefinedGroup` with array fields into one `RefinedGroup` per element.
fn normalize_raw_groups(raw: RawRefineResponse) -> RefineResponse {
    let mut groups = Vec::new();
    for rg in raw.groups {
        let paths = rg.proposed_path.unwrap_or_default();
        let names = rg.proposed_name.unwrap_or_default();
        let descs = rg.proposed_description.unwrap_or_default();
        let n = paths.len().max(names.len()).max(descs.len()).max(1);

        if n <= 1 {
            // Single proposal (or covered_by) — emit one group
            groups.push(RefinedGroup {
                raw_suggestions: rg.raw_suggestions,
                covered_by: rg.covered_by,
                proposed_path: paths.into_iter().next(),
                proposed_name: names.into_iter().next(),
                proposed_description: descs.into_iter().next(),
                reason: rg.reason,
            });
        } else {
            // Multiple proposals — split into separate groups, one per index
            let paths: Vec<_> = paths.into_iter().collect();
            let names: Vec<_> = names.into_iter().collect();
            let descs: Vec<_> = descs.into_iter().collect();
            for i in 0..n {
                let raw_suggestion = if i < rg.raw_suggestions.len() {
                    vec![rg.raw_suggestions[i].clone()]
                } else {
                    rg.raw_suggestions.clone()
                };
                groups.push(RefinedGroup {
                    raw_suggestions: raw_suggestion,
                    covered_by: rg.covered_by.clone(),
                    proposed_path: paths.get(i).cloned(),
                    proposed_name: names.get(i).cloned(),
                    proposed_description: descs.get(i).cloned(),
                    reason: rg.reason.clone(),
                });
            }
        }
    }
    RefineResponse { groups }
}

fn build_refine_prompt(nodes: &[NodeEntry], votes: &[db::CategoryVote]) -> String {
    use std::fmt::Write;

    let mut prompt = String::new();
    writeln!(
        prompt,
        "You are consolidating suggested new categories for a project roadmap.\n"
    )
    .unwrap();

    prompt.push_str(&build_roadmap_section(nodes));

    writeln!(prompt, "\n## Raw Category Suggestions\n").unwrap();
    writeln!(
        prompt,
        "Each line shows a suggested category, vote count, and example issues.\n"
    )
    .unwrap();
    for vote in votes {
        let refs: Vec<&str> = vote
            .issue_refs
            .iter()
            .take(5)
            .map(std::string::String::as_str)
            .collect();
        writeln!(
            prompt,
            "  {:<40} {} votes  {}",
            vote.category,
            vote.vote_count,
            refs.join(", ")
        )
        .unwrap();
    }

    writeln!(
        prompt,
        "\n## Instructions\n\
         1. Group suggestions that refer to the same concept (e.g. \"circuit/synthesis\" and \
         \"research/circuit-synthesis\")\n\
         2. For each group, propose a single node: path (must be a valid child of an existing \
         node or a new top-level node), name, and description\n\
         3. If a suggestion is already covered by an existing roadmap node, mark it as \
         \"covered\" with the existing node path — set covered_by to the node path and leave \
         proposed_path/name/description as null\n\
         4. Only propose nodes that would meaningfully organize 2+ issues\n\
         \n\
         Respond with JSON only:\n\
         {{\n\
           \"groups\": [\n\
             {{\n\
               \"raw_suggestions\": [\"category-a\", \"category-b\"],\n\
               \"covered_by\": null,\n\
               \"proposed_path\": \"parent/child\",\n\
               \"proposed_name\": \"Display Name\",\n\
               \"proposed_description\": \"What this node covers\",\n\
               \"reason\": \"Why these are grouped\"\n\
             }}\n\
           ]\n\
         }}"
    )
    .unwrap();

    prompt
}

fn parse_refine_response(raw: &str) -> Result<RefineResponse> {
    let trimmed = raw.trim();

    // Try flexible format first (handles string-or-array fields), then strict.
    for candidate in [
        Some(trimmed.to_owned()),
        extract_json_block(trimmed),
        extract_json_object(trimmed),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(r) = serde_json::from_str::<RawRefineResponse>(&candidate) {
            return Ok(normalize_raw_groups(r));
        }
        if let Ok(r) = serde_json::from_str::<RefineResponse>(&candidate) {
            return Ok(r);
        }
    }

    Err(Error::LlmParse(format!(
        "could not parse category refine response: {trimmed}"
    )))
}

/// Ask LLM to consolidate raw category suggestions into grouped proposals.
pub fn refine_categories(
    nodes: &[NodeEntry],
    votes: &[db::CategoryVote],
    config: &LlmConfig,
) -> Result<RefineResponse> {
    let prompt = build_refine_prompt(nodes, votes);
    tracing::info!(
        categories = votes.len(),
        "refining category suggestions via LLM"
    );
    let raw = invoke_llm(config, &prompt)?;
    let response = parse_refine_response(&raw)?;
    tracing::debug!(
        groups = response.groups.len(),
        "category refinement complete"
    );
    Ok(response)
}

/// Re-invoke LLM to refine a single category group based on user feedback.
pub fn refine_category_group(
    nodes: &[NodeEntry],
    group: &RefinedGroup,
    user_feedback: &str,
    config: &LlmConfig,
) -> Result<RefinedGroup> {
    use std::fmt::Write;

    let mut prompt = String::new();
    writeln!(prompt, "You previously proposed this for a category group:").unwrap();
    writeln!(
        prompt,
        "  Raw suggestions: {}",
        group.raw_suggestions.join(", ")
    )
    .unwrap();
    if let Some(ref path) = group.proposed_path {
        writeln!(prompt, "  Proposed path: {path}").unwrap();
    }
    if let Some(ref name) = group.proposed_name {
        writeln!(prompt, "  Proposed name: {name}").unwrap();
    }
    if let Some(ref desc) = group.proposed_description {
        writeln!(prompt, "  Proposed description: {desc}").unwrap();
    }
    if let Some(ref covered) = group.covered_by {
        writeln!(prompt, "  Covered by: {covered}").unwrap();
    }
    writeln!(prompt, "  Reason: {}", group.reason).unwrap();

    writeln!(prompt, "\n{}", build_roadmap_section(nodes)).unwrap();

    writeln!(
        prompt,
        "The user wants changes: {user_feedback}\n\n\
         Respond with an updated JSON proposal:\n\
         {{\n\
           \"covered_by\": null,\n\
           \"proposed_path\": \"...\",\n\
           \"proposed_name\": \"...\",\n\
           \"proposed_description\": \"...\",\n\
           \"reason\": \"...\"\n\
         }}"
    )
    .unwrap();

    tracing::debug!(user_feedback, "refining category group via LLM");
    let raw = invoke_llm(config, &prompt)?;
    let trimmed = raw.trim();

    let parsed = serde_json::from_str::<RefinedGroup>(trimmed)
        .ok()
        .or_else(|| {
            extract_json_block(trimmed).and_then(|j| serde_json::from_str::<RefinedGroup>(&j).ok())
        })
        .or_else(|| {
            extract_json_object(trimmed).and_then(|j| serde_json::from_str::<RefinedGroup>(&j).ok())
        });

    match parsed {
        Some(mut g) => {
            g.raw_suggestions.clone_from(&group.raw_suggestions);
            Ok(g)
        }
        None => Err(Error::LlmParse(format!(
            "could not parse category refinement response: {trimmed}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Terminal integration (Ghostty, iTerm2, kitty, WezTerm, etc.)
// ---------------------------------------------------------------------------

/// Set terminal tab/titlebar progress indicator via ConEmu OSC 9;4 protocol.
/// state: 1=normal, 2=error, 3=paused, 0=remove
fn set_terminal_progress(state: u8, percent: u8) {
    if state == 0 {
        eprint!("\x1b]9;4;0\x07");
    } else {
        eprint!("\x1b]9;4;{state};{percent}\x07");
    }
}

/// Set terminal title via OSC 2.
fn set_terminal_title(title: &str) {
    eprint!("\x1b]2;{title}\x07");
}

/// Clear terminal title and progress indicator.
fn clear_terminal_status() {
    set_terminal_progress(0, 0);
    // Reset title to empty (terminal will revert to default)
    set_terminal_title("");
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max.saturating_sub(3);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Run triage on untriaged issues. Returns number of issues classified.
#[allow(clippy::too_many_arguments)]
pub fn triage_issues(
    conn: &Connection,
    nodes: &[NodeEntry],
    catalog: PromptCatalog<'_>,
    examples: &[TriageExample],
    config: &LlmConfig,
    batch_size: usize,
    parallel: usize,
    limit: Option<usize>,
    repo: Option<&str>,
) -> Result<usize> {
    use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
    use std::sync::{Arc, Mutex};
    use std::thread;

    struct WorkItem {
        prompt: String,
        is_batch: bool,
        issue_ids: Vec<i64>,
        issue_refs: Vec<String>,
        issue_titles: Vec<String>,
        /// Existing labels per issue (for filtering out duplicates from LLM output).
        existing_labels: Vec<std::collections::BTreeSet<String>>,
    }

    let mut issues = match repo {
        Some(r) => db::get_untriaged_issues_by_repo(conn, r)?,
        None => db::get_untriaged_issues(conn)?,
    };

    if issues.is_empty() {
        println!("No untriaged issues found.");
        return Ok(0);
    }

    if let Some(limit) = limit {
        let total_untriaged = issues.len();
        issues.truncate(limit);
        if issues.len() < total_untriaged {
            println!(
                "Limiting to {} of {total_untriaged} untriaged issues",
                issues.len()
            );
        }
    }

    let backend_desc = config.model.as_ref().map_or_else(
        || config.backend.name().to_string(),
        |m| format!("{} (model: {m})", config.backend.name()),
    );

    let parallel = parallel.max(1);
    let batch_size = batch_size.min(issues.len()).max(1);
    let total_issues = issues.len();

    println!(
        "Classifying {total_issues} issues with {backend_desc} (batch_size={batch_size}, parallel={parallel})"
    );

    // Build work units
    let work_units: Vec<&[StoredIssue]> = if batch_size <= 1 {
        issues.iter().map(std::slice::from_ref).collect()
    } else {
        issues.chunks(batch_size).collect()
    };

    let now = chrono::Utc::now().to_rfc3339();
    let backend_name = config.backend.name().to_string();

    // Set up progress bars
    let mp = MultiProgress::new();

    // Main progress bar (always at the bottom)
    let main_pb = mp.add(ProgressBar::new(total_issues as u64));
    main_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} issues ({eta} remaining)")
            .unwrap()
            .progress_chars("=>-"),
    );
    main_pb.enable_steady_tick(std::time::Duration::from_millis(200));

    // Worker spinners (stacked above the main bar in parallel mode)
    let worker_pbs: Vec<ProgressBar> = if parallel > 1 {
        (0..parallel)
            .map(|i| {
                let pb = mp.insert_before(&main_pb, ProgressBar::new_spinner());
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template(&format!("  {{spinner:.dim}} worker {}: {{msg}}", i + 1))
                        .unwrap(),
                );
                pb.set_message("idle");
                pb.enable_steady_tick(std::time::Duration::from_millis(150));
                pb
            })
            .collect()
    } else {
        vec![]
    };

    let items: Vec<WorkItem> = work_units
        .iter()
        .map(|unit| {
            let (prompt, is_batch) = if batch_size <= 1 {
                (build_prompt(&unit[0], nodes, catalog, examples), false)
            } else {
                (build_batch_prompt(unit, nodes, catalog, examples), true)
            };
            WorkItem {
                prompt,
                is_batch,
                issue_ids: unit.iter().map(|i| i.id).collect(),
                issue_refs: unit
                    .iter()
                    .map(|i| format!("{}#{}", i.repo, i.number))
                    .collect(),
                issue_titles: unit.iter().map(|i| i.title.clone()).collect(),
                existing_labels: unit
                    .iter()
                    .map(|i| i.labels.iter().cloned().collect())
                    .collect(),
            }
        })
        .collect();

    let queue = Arc::new(Mutex::new(items));
    let err_count = Arc::new(Mutex::new(0usize));
    let classified_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Valid node set for validation inside workers
    let valid_nodes: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.path.clone()).collect();
    let valid_nodes = Arc::new(valid_nodes);

    // Open a second connection for writes (WAL mode allows concurrent reader+writer)
    let db_path = conn.path().unwrap().to_string();
    let write_conn = Arc::new(Mutex::new(db::open_db_from_path(std::path::Path::new(
        &db_path,
    ))?));

    let config = Arc::new(config.clone());
    let backend_name = Arc::new(backend_name);
    let now = Arc::new(now);
    let main_pb = Arc::new(main_pb);
    let total_issues = total_issues as u64;

    // Set initial terminal progress
    set_terminal_progress(1, 0);
    set_terminal_title(&format!("armitage classify: 0/{total_issues}"));

    let num_workers = parallel;
    let mut handles = Vec::new();

    for worker_id in 0..num_workers {
        let queue = Arc::clone(&queue);
        let config = Arc::clone(&config);
        let err_count = Arc::clone(&err_count);
        let bn = Arc::clone(&backend_name);
        let now = Arc::clone(&now);
        let main_pb = Arc::clone(&main_pb);
        let valid_nodes = Arc::clone(&valid_nodes);
        let write_conn = Arc::clone(&write_conn);
        let classified_count = Arc::clone(&classified_count);
        let worker_pb = if worker_id < worker_pbs.len() {
            Some(worker_pbs[worker_id].clone())
        } else {
            None
        };

        handles.push(thread::spawn(move || {
            SUPPRESS_SPINNER.with(|s| s.set(true));
            loop {
                let wi = { queue.lock().unwrap().pop() };
                let Some(wi) = wi else { break };

                let desc = if wi.is_batch {
                    format!("{} (+{} more)", wi.issue_refs[0], wi.issue_refs.len() - 1)
                } else {
                    format!("{} {}", wi.issue_refs[0], truncate(&wi.issue_titles[0], 40))
                };

                if let Some(ref pb) = worker_pb {
                    pb.set_message(desc.clone());
                }

                let llm_result = invoke_llm(&config, &wi.prompt);
                match llm_result {
                    Ok(raw) => {
                        let parsed = if wi.is_batch {
                            parse_batch_classifications(&raw)
                        } else {
                            parse_classification(&raw).map(|c| vec![c])
                        };
                        match parsed {
                            Ok(cs) => {
                                let conn_guard = write_conn.lock().unwrap();
                                for (i, c) in cs.into_iter().enumerate() {
                                    if i < wi.issue_ids.len() {
                                        // Validate node exists
                                        let validated_node =
                                            c.suggested_node.as_deref().and_then(|n| {
                                                if valid_nodes.contains(n) {
                                                    Some(n.to_string())
                                                } else {
                                                    main_pb.println(format!(
                                                        "  WARN {}: non-existent node '{}', set to null",
                                                        wi.issue_refs[i], n,
                                                    ));
                                                    None
                                                }
                                            });

                                        let node_str =
                                            validated_node.as_deref().unwrap_or("none");
                                        let conf = c.confidence * 100.0;
                                        let title = truncate(&wi.issue_titles[i], 50);
                                        main_pb.println(format!(
                                            "  {} {title} -> {node_str} ({conf:.0}%)",
                                            wi.issue_refs[i],
                                        ));

                                        // Only keep labels the issue doesn't already have
                                        let new_labels: Vec<String> = c
                                            .suggested_labels
                                            .iter()
                                            .filter(|l| !wi.existing_labels[i].contains(l.as_str()))
                                            .cloned()
                                            .collect();

                                        let sug = db::TriageSuggestion {
                                            id: 0,
                                            issue_id: wi.issue_ids[i],
                                            suggested_node: validated_node,
                                            suggested_labels: new_labels,
                                            confidence: Some(c.confidence),
                                            reasoning: c.reasoning.clone(),
                                            llm_backend: bn.to_string(),
                                            created_at: now.to_string(),
                                            is_tracking_issue: c.is_tracking_issue,
                                            suggested_new_categories: c
                                                .suggested_new_categories
                                                .clone(),
                                            is_stale: c.is_stale,
                                        };
                                        if let Err(e) =
                                            db::upsert_suggestion(&conn_guard, &sug)
                                        {
                                            main_pb.println(format!(
                                                "  ERROR {}: DB write: {e}",
                                                wi.issue_refs[i]
                                            ));
                                        } else {
                                            classified_count.fetch_add(
                                                1,
                                                std::sync::atomic::Ordering::Relaxed,
                                            );
                                        }

                                        main_pb.inc(1);
                                        let pct = (main_pb.position() * 100 / total_issues) as u8;
                                        set_terminal_progress(1, pct);
                                        set_terminal_title(&format!(
                                            "armitage classify: {}/{total_issues}",
                                            main_pb.position()
                                        ));
                                    }
                                }
                                drop(conn_guard);
                            }
                            Err(e) => {
                                main_pb.println(format!("  ERROR {desc}: parse error: {e}"));
                                *err_count.lock().unwrap() += 1;
                                main_pb.inc(wi.issue_ids.len() as u64);
                                set_terminal_progress(
                                    2,
                                    (main_pb.position() * 100 / total_issues) as u8,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        main_pb.println(format!("  ERROR {desc}: {e}"));
                        *err_count.lock().unwrap() += 1;
                        main_pb.inc(wi.issue_ids.len() as u64);
                        set_terminal_progress(2, (main_pb.position() * 100 / total_issues) as u8);
                    }
                }

                if let Some(ref pb) = worker_pb {
                    pb.set_message("idle");
                }
            }
            if let Some(ref pb) = worker_pb {
                pb.finish_and_clear();
            }
        }));
    }

    for h in handles {
        h.join().expect("worker thread panicked");
    }

    main_pb.finish_with_message("done");
    clear_terminal_status();

    let errors = *err_count.lock().unwrap();
    let classified = classified_count.load(std::sync::atomic::Ordering::Relaxed);

    if errors > 0 {
        eprintln!("{errors} issue(s) failed to classify.");
    }

    // Post-run: collect new category votes from DB for summary output
    let category_votes = db::get_new_category_votes(conn, None)?;
    if !category_votes.is_empty() {
        println!("\n--- Suggested new categories ---");
        for vote in &category_votes {
            println!("  {} ({} issue(s))", vote.category, vote.vote_count);
            for issue_ref in vote.issue_refs.iter().take(5) {
                println!("    - {issue_ref}");
            }
        }
        println!(
            "To create a new node: armitage node new <path> --name \"...\" --description \"...\""
        );
    }

    Ok(classified)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use armitage_core::node::{Node, NodeStatus};

    #[test]
    fn parse_accepts_gemini_backend() {
        let backend: LlmBackend = "gemini".parse().unwrap();
        assert_eq!(backend.name(), "gemini");
    }

    #[test]
    fn parse_unknown_backend_lists_gemini() {
        let err = "other".parse::<LlmBackend>().unwrap_err().to_string();
        assert!(err.contains("claude"));
        assert!(err.contains("codex"));
        assert!(err.contains("gemini"));
    }

    #[test]
    fn unwrap_gemini_cli_wrapper_extracts_text() {
        let raw = r#"{"response":"{\"suggested_node\":null,\"suggested_labels\":[],\"confidence\":0.5,\"reasoning\":\"none\"}"}"#;
        let text = unwrap_cli_output(&LlmBackend::Gemini, raw).unwrap();
        let parsed = parse_classification(&text).unwrap();
        assert_eq!(parsed.suggested_node, None);
    }

    #[test]
    fn unwrap_gemini_cli_wrapper_rejects_missing_response() {
        let raw = r#"{"status":"ok"}"#;
        let err = unwrap_cli_output(&LlmBackend::Gemini, raw)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Gemini"));
    }

    #[test]
    fn curated_labels_section_lists_name_and_description_only() {
        let labels = LabelsFile {
            labels: vec![
                armitage_labels::def::LabelDef {
                    name: "bug".to_string(),
                    description: "Broken behavior".to_string(),
                    color: Some("D73A4A".to_string()),
                    repos: vec![],
                    pinned: false,
                },
                armitage_labels::def::LabelDef {
                    name: "priority:high".to_string(),
                    description: "Needs prompt attention".to_string(),
                    color: Some("B60205".to_string()),
                    repos: vec![],
                    pinned: false,
                },
            ],
        };

        let section = build_curated_labels_section(&labels);

        assert!(section.contains("- bug: Broken behavior"));
        assert!(section.contains("- priority:high: Needs prompt attention"));
        assert!(!section.contains("D73A4A"));
        assert!(!section.contains("owner/repo"));
    }

    #[test]
    fn prompt_includes_curated_labels_section() {
        let issue = StoredIssue {
            id: 1,
            repo: "owner/repo".to_string(),
            number: 42,
            title: "Fix label import".to_string(),
            body: "Need better label curation.".to_string(),
            state: "OPEN".to_string(),
            labels: vec!["bug".to_string()],
            updated_at: "2026-04-03T12:00:00Z".to_string(),
            fetched_at: "2026-04-03T12:00:00Z".to_string(),
            sub_issues_count: 0,
            author: String::new(),
            assignees: vec![],
        };
        let nodes = vec![NodeEntry {
            path: "infra".to_string(),
            dir: std::path::PathBuf::from("/tmp/infra"),
            node: Node {
                name: "Infra".to_string(),
                description: "Infrastructure work".to_string(),
                github_issue: None,
                labels: vec![],
                repos: vec![],
                owners: vec![],
                team: None,
                triage_hint: None,
                timeline: None,
                status: NodeStatus::Active,
            },
        }];
        let schema = LabelSchema::default();
        let labels = LabelsFile {
            labels: vec![armitage_labels::def::LabelDef {
                name: "bug".to_string(),
                description: "Broken behavior".to_string(),
                color: None,
                repos: vec![],
                pinned: false,
            }],
        };

        let prompt = build_prompt(
            &issue,
            &nodes,
            PromptCatalog {
                label_schema: &schema,
                curated_labels: &labels,
            },
            &[],
        );

        assert!(prompt.contains("## Curated Labels"));
        assert!(prompt.contains("- bug: Broken behavior"));
    }

    #[test]
    fn parse_claude_cli_wrapper() {
        let wrapper = r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"suggested_node\": null, \"suggested_labels\": [], \"confidence\": 0.9, \"reasoning\": \"No match\"}"}"#;
        let inner: serde_json::Value = serde_json::from_str(wrapper).unwrap();
        let result_str = inner["result"].as_str().unwrap();
        let c = parse_classification(result_str).unwrap();
        assert_eq!(c.suggested_node, None);
        assert!((c.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_clean_json() {
        let raw = r#"{"suggested_node": "project/auth", "suggested_labels": ["bug", "team:alpha"], "confidence": 0.85, "reasoning": "Auth related"}"#;
        let c = parse_classification(raw).unwrap();
        assert_eq!(c.suggested_node, Some("project/auth".to_string()));
        assert_eq!(c.suggested_labels, vec!["bug", "team:alpha"]);
        assert!((c.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_json_in_code_fence() {
        let raw = "Here is the result:\n```json\n{\"suggested_node\": \"a/b\", \"suggested_labels\": [], \"confidence\": 0.5, \"reasoning\": \"test\"}\n```\n";
        let c = parse_classification(raw).unwrap();
        assert_eq!(c.suggested_node, Some("a/b".to_string()));
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let raw = "Based on my analysis, {\"suggested_node\": null, \"suggested_labels\": [\"bug\"], \"confidence\": 0.3, \"reasoning\": \"unclear\"} is my answer.";
        let c = parse_classification(raw).unwrap();
        assert_eq!(c.suggested_node, None);
    }

    #[test]
    fn parse_batch_response() {
        let raw = r#"[{"suggested_node": "a", "suggested_labels": [], "confidence": 0.9, "reasoning": "r1"}, {"suggested_node": "b", "suggested_labels": ["bug"], "confidence": 0.7, "reasoning": "r2"}]"#;
        let v = parse_batch_classifications(raw).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].suggested_node, Some("a".to_string()));
        assert_eq!(v[1].suggested_node, Some("b".to_string()));
    }

    #[test]
    fn extract_json_block_from_markdown() {
        let text = "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text";
        let block = extract_json_block(text).unwrap();
        assert_eq!(block, "{\"key\": \"value\"}\n");
    }

    #[test]
    fn extract_json_object_from_text() {
        let text = "The answer is {\"a\": 1, \"b\": {\"c\": 2}} ok";
        let obj = extract_json_object(text).unwrap();
        assert_eq!(obj, "{\"a\": 1, \"b\": {\"c\": 2}}");
    }

    #[test]
    fn parse_refine_response_handles_array_fields() {
        let raw = r#"{"groups": [{"raw_suggestions": ["atlas/integrations", "relay/integration"], "covered_by": null, "proposed_path": ["atlas/integrations", "relay/integration"], "proposed_name": ["Integrations", "Integration"], "proposed_description": ["Atlas integrations", "Relay integration"], "reason": "Both are integration work"}]}"#;
        let r = parse_refine_response(raw).unwrap();
        assert_eq!(r.groups.len(), 2);
        assert_eq!(
            r.groups[0].proposed_path.as_deref(),
            Some("atlas/integrations")
        );
        assert_eq!(r.groups[0].proposed_name.as_deref(), Some("Integrations"));
        assert_eq!(r.groups[0].raw_suggestions, vec!["atlas/integrations"]);
        assert_eq!(
            r.groups[1].proposed_path.as_deref(),
            Some("relay/integration")
        );
        assert_eq!(r.groups[1].raw_suggestions, vec!["relay/integration"]);
    }

    #[test]
    fn parse_refine_response_handles_string_fields() {
        let raw = r#"{"groups": [{"raw_suggestions": ["a"], "covered_by": null, "proposed_path": "some/path", "proposed_name": "Name", "proposed_description": "Desc", "reason": "ok"}]}"#;
        let r = parse_refine_response(raw).unwrap();
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].proposed_path.as_deref(), Some("some/path"));
    }

    #[test]
    fn parse_refine_response_handles_code_fence_with_arrays() {
        let raw = "```json\n{\"groups\": [{\"raw_suggestions\": [\"a\", \"b\"], \"covered_by\": null, \"proposed_path\": [\"x\", \"y\"], \"proposed_name\": [\"X\", \"Y\"], \"proposed_description\": [\"desc x\", \"desc y\"], \"reason\": \"r\"}]}\n```";
        let r = parse_refine_response(raw).unwrap();
        assert_eq!(r.groups.len(), 2);
    }

    #[test]
    fn build_refine_prompt_includes_categories_and_tree() {
        let nodes = vec![NodeEntry {
            path: "prism".to_string(),
            dir: std::path::PathBuf::from("/tmp/prism"),
            node: Node {
                name: "PRISM".to_string(),
                description: "PRISM language".to_string(),
                github_issue: None,
                labels: vec![],
                repos: vec![],
                owners: vec![],
                team: None,
                triage_hint: None,
                timeline: None,
                status: NodeStatus::Active,
            },
        }];
        let votes = vec![
            db::CategoryVote {
                category: "compute/emulator".to_string(),
                vote_count: 5,
                issue_refs: vec!["owner/repo#1".to_string(), "owner/repo#2".to_string()],
            },
            db::CategoryVote {
                category: "compute/backend".to_string(),
                vote_count: 2,
                issue_refs: vec!["owner/repo#1".to_string()],
            },
        ];
        let prompt = build_refine_prompt(&nodes, &votes);
        assert!(prompt.contains("compute/emulator"));
        assert!(prompt.contains("5 votes"));
        assert!(prompt.contains("compute/backend"));
        assert!(prompt.contains("prism"));
        assert!(prompt.contains("\"groups\""));
    }

    #[test]
    fn find_prefix_duplicates_catches_bare_remote_label() {
        use crate::label_import::{
            CandidateStatus, LabelImportCandidate, LabelImportSession, ReconcileResponse,
            RemoteLabelVariant,
        };

        let local = LabelsFile {
            labels: vec![armitage_labels::def::LabelDef {
                name: "area: FLUX".to_string(),
                description: "Area: FLUX emulator integration.".to_string(),
                color: None,
                repos: vec![],
                pinned: false,
            }],
        };

        let session = LabelImportSession {
            id: "test".to_string(),
            fetched_at: "2026-04-07T00:00:00Z".to_string(),
            repos: vec!["owner/repo".to_string()],
            candidates: vec![LabelImportCandidate {
                name: "flux".to_string(),
                status: CandidateStatus::New,
                local: None,
                remote_variants: vec![RemoteLabelVariant {
                    repo: "owner/repo".to_string(),
                    description: "flux related".to_string(),
                    color: None,
                }],
            }],
        };

        let empty_llm = ReconcileResponse {
            merge_groups: vec![],
        };

        let groups = find_prefix_duplicates(&local, &session, &empty_llm);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].labels, vec!["flux", "area: FLUX"]);
        assert_eq!(groups[0].recommended, Some("area: FLUX".to_string()));
    }

    #[test]
    fn find_prefix_duplicates_skips_already_covered() {
        use crate::label_import::{
            CandidateStatus, LabelImportCandidate, LabelImportSession, MergeGroup,
            ReconcileResponse, RemoteLabelVariant,
        };

        let local = LabelsFile {
            labels: vec![armitage_labels::def::LabelDef {
                name: "category: bug".to_string(),
                description: "Bug".to_string(),
                color: None,
                repos: vec![],
                pinned: false,
            }],
        };

        let session = LabelImportSession {
            id: "test".to_string(),
            fetched_at: "2026-04-07T00:00:00Z".to_string(),
            repos: vec!["owner/repo".to_string()],
            candidates: vec![LabelImportCandidate {
                name: "bug".to_string(),
                status: CandidateStatus::New,
                local: None,
                remote_variants: vec![RemoteLabelVariant {
                    repo: "owner/repo".to_string(),
                    description: "Something broken".to_string(),
                    color: None,
                }],
            }],
        };

        // LLM already grouped "bug"
        let llm_response = ReconcileResponse {
            merge_groups: vec![MergeGroup {
                labels: vec!["bug".to_string(), "category: bug".to_string()],
                reason: "same thing".to_string(),
                suggestions: vec![],
                recommended: Some("category: bug".to_string()),
            }],
        };

        let groups = find_prefix_duplicates(&local, &session, &llm_response);
        assert!(
            groups.is_empty(),
            "should skip labels already in LLM groups"
        );
    }
}
