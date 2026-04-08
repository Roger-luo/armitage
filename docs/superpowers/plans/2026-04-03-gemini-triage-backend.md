# Gemini Triage Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Gemini CLI-backed triage backend with explicit backend/model resolution and focused tests for parsing and validation.

**Architecture:** Keep the existing triage pipeline intact and extend only the backend resolution and CLI adapter seams. Backend/model validation will live in the triage command path, while Gemini-specific process launch and JSON unwrapping will live in the LLM adapter alongside Claude/Codex behavior.

**Tech Stack:** Rust, clap, rusqlite, cargo-nextest

---

### Task 1: Add failing tests for backend parsing and explicit config resolution

**Files:**
- Modify: `src/triage/llm.rs`
- Modify: `src/cli/triage.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parse_accepts_gemini_backend() {
    let backend = LlmBackend::parse("gemini").unwrap();
    assert_eq!(backend.name(), "gemini");
}

#[test]
fn parse_unknown_backend_lists_gemini() {
    let err = LlmBackend::parse("other").unwrap_err().to_string();
    assert!(err.contains("claude"));
    assert!(err.contains("codex"));
    assert!(err.contains("gemini"));
}
```

```rust
#[test]
fn resolve_config_requires_backend() {
    let err = resolve_classify_config(None, None, None, &TriageConfig::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("backend"));
}

#[test]
fn resolve_config_requires_model_for_gemini() {
    let triage = TriageConfig {
        backend: Some("gemini".to_string()),
        model: None,
        effort: None,
    };
    let err = resolve_classify_config(None, None, None, &triage)
        .unwrap_err()
        .to_string();
    assert!(err.contains("model"));
    assert!(err.contains("gemini"));
}

#[test]
fn resolve_config_uses_cli_overrides() {
    let triage = TriageConfig {
        backend: Some("claude".to_string()),
        model: Some("sonnet".to_string()),
        effort: Some("medium".to_string()),
    };
    let cfg = resolve_classify_config(
        Some("gemini".to_string()),
        Some("gemini-2.5-flash".to_string()),
        None,
        &triage,
    )
    .unwrap();

    assert_eq!(cfg.backend.name(), "gemini");
    assert_eq!(cfg.model.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(cfg.effort.as_deref(), Some("medium"));
}
```

- [ ] **Step 2: Run targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(parse_accepts_gemini_backend|parse_unknown_backend_lists_gemini|resolve_config_requires_backend|resolve_config_requires_model_for_gemini|resolve_config_uses_cli_overrides)'`

Expected: FAIL because `gemini` is not yet accepted and the classify config resolver does not yet exist.

- [ ] **Step 3: Implement the minimal code**

```rust
pub enum LlmBackend {
    Claude,
    Codex,
    Gemini,
}
```

```rust
fn resolve_classify_config(
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    triage: &crate::model::org::TriageConfig,
) -> Result<llm::LlmConfig> {
    let backend_str = backend
        .or_else(|| triage.backend.clone())
        .ok_or_else(|| Error::Other("triage backend must be set via --backend or [triage].backend".to_string()))?;
    let model = model.or_else(|| triage.model.clone());
    let effort = effort.or_else(|| triage.effort.clone());
    let backend = llm::LlmBackend::parse(&backend_str)?;

    if matches!(backend, llm::LlmBackend::Gemini) && model.is_none() {
        return Err(Error::Other(
            "triage model must be set via --model or [triage].model when backend is gemini"
                .to_string(),
        ));
    }

    Ok(llm::LlmConfig { backend, model, effort })
}
```

- [ ] **Step 4: Run targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(parse_accepts_gemini_backend|parse_unknown_backend_lists_gemini|resolve_config_requires_backend|resolve_config_requires_model_for_gemini|resolve_config_uses_cli_overrides)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs src/cli/triage.rs
git commit -m "feat: add explicit triage backend resolution"
```

### Task 2: Add failing tests for Gemini CLI JSON unwrapping

**Files:**
- Modify: `src/triage/llm.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn unwrap_gemini_cli_wrapper_extracts_text() {
    let raw = r#"{"response":"{\"suggested_node\":null,\"suggested_labels\":[],\"confidence\":0.5,\"reasoning\":\"none\"}"}"#;
    let text = unwrap_cli_output(LlmBackend::Gemini, raw).unwrap();
    let parsed = parse_classification(&text).unwrap();
    assert_eq!(parsed.suggested_node, None);
}

#[test]
fn unwrap_gemini_cli_wrapper_rejects_missing_response() {
    let raw = r#"{"status":"ok"}"#;
    let err = unwrap_cli_output(LlmBackend::Gemini, raw).unwrap_err().to_string();
    assert!(err.contains("Gemini"));
}
```

- [ ] **Step 2: Run targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(unwrap_gemini_cli_wrapper_extracts_text|unwrap_gemini_cli_wrapper_rejects_missing_response)'`

Expected: FAIL because the Gemini unwrap helper does not yet exist.

- [ ] **Step 3: Implement the minimal code**

```rust
fn unwrap_cli_output(backend: LlmBackend, raw: &str) -> Result<String> {
    match backend {
        LlmBackend::Claude => { /* existing wrapper logic */ }
        LlmBackend::Gemini => {
            let wrapper: serde_json::Value = serde_json::from_str(raw)?;
            let response = wrapper
                .get("response")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::LlmInvocation("Gemini CLI returned JSON without a response field".to_string()))?;
            Ok(response.to_string())
        }
        LlmBackend::Codex => Ok(raw.to_string()),
    }
}
```

- [ ] **Step 4: Run targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(unwrap_gemini_cli_wrapper_extracts_text|unwrap_gemini_cli_wrapper_rejects_missing_response|parse_claude_cli_wrapper)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs
git commit -m "feat: parse gemini cli responses"
```

### Task 3: Wire Gemini CLI invocation and command help

**Files:**
- Modify: `src/triage/llm.rs`
- Modify: `src/cli/triage.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn classify_help_mentions_gemini_backend() {
    const HELP: &str = include_str!("expected-help-snippet");
    assert!(HELP.contains("gemini"));
}
```

This task uses a lightweight assertion instead of process-level command tests: add or update an existing help-text-oriented assertion in the touched module if present; otherwise add a targeted unit test around the clap metadata or validate by final command output.

- [ ] **Step 2: Run targeted verification to confirm the gap**

Run: `cargo nextest run -E 'test(classify_help_mentions_gemini_backend)'`

Expected: FAIL if a concrete help-text test is added. If clap metadata is awkward here, skip adding this exact test and rely on final command verification instead.

- [ ] **Step 3: Implement the minimal code**

```rust
LlmBackend::Gemini => {
    let mut c = Command::new("gemini");
    c.arg("-p").arg("-");
    c.arg("--output-format").arg("json");
    if let Some(model) = &config.model {
        c.arg("--model").arg(model);
    }
    c
}
```

```rust
/// LLM backend: "claude", "codex", or "gemini" (overrides [triage].backend in armitage.toml)
```

- [ ] **Step 4: Run focused verification**

Run: `cargo nextest run -E 'test(parse_accepts_gemini_backend|unwrap_gemini_cli_wrapper_extracts_text|parse_claude_cli_wrapper)'`

Run: `cargo run -- triage classify --help`

Expected: Tests PASS and help output lists `gemini`.

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs src/cli/triage.rs src/cli/mod.rs
git commit -m "feat: wire gemini triage backend"
```

### Task 4: Run repository verification and summarize results

**Files:**
- Modify: `src/triage/llm.rs`
- Modify: `src/cli/triage.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Run formatting**

Run: `cargo fmt --all`

Expected: exit 0

- [ ] **Step 2: Run lint**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: exit 0

- [ ] **Step 3: Run full tests**

Run: `cargo nextest run`

Expected: PASS with 0 failures

- [ ] **Step 4: Inspect git diff**

Run: `git status --short && git diff --stat`

Expected: only the planned triage/help changes are present

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs src/cli/triage.rs src/cli/mod.rs
git commit -m "feat: support gemini flash for triage"
```
