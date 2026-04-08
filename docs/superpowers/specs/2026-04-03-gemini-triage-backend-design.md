# Gemini Triage Backend Design

## Goal

Add support for running `armitage triage classify` through the Google Gemini CLI so users can explicitly choose Gemini Flash models such as `gemini-2.5-flash` and concrete versioned variants.

## Scope

In scope:
- Add `gemini` as a supported triage backend.
- Require explicit backend and model selection for Gemini resolution.
- Allow CLI flags to satisfy that explicit selection requirement.
- Invoke the `gemini` CLI in headless mode for classification.
- Parse Gemini CLI JSON output into the existing classification pipeline.
- Update tests and CLI help text.

Out of scope:
- Direct Gemini API integration.
- New auth/config management for Gemini credentials.
- Backend-specific prompt changes beyond what is needed to parse Gemini output.

## User Experience

Users can select Gemini either in `armitage.toml` or on the command line.

Example config:

```toml
[triage]
backend = "gemini"
model = "gemini-2.5-flash"
```

Example CLI override:

```bash
armitage triage classify --backend gemini --model gemini-2.5-flash
```

Resolution rules:
- `backend` resolves from `--backend` first, then `[triage].backend`.
- `model` resolves from `--model` first, then `[triage].model`.
- If the resolved backend is `gemini` and no model resolves, the command fails with a clear error.
- If no backend resolves at all, the command fails with a clear error instead of silently defaulting.
- Existing `claude` and `codex` behavior remains unchanged once explicitly selected.

## CLI and Config Changes

`triage classify` already accepts `--backend` and `--model`, so the main change is validation and help text:
- Update help text to list `gemini` as a supported backend.
- Update backend parse errors to include `gemini`.
- Replace the current implicit backend default with explicit resolution so users always know which provider is being used.

This keeps the command line surface area stable while making backend/model selection intentional.

## Execution Design

Add a new `LlmBackend::Gemini` variant in `src/triage/llm.rs`.

Gemini execution path:
- Run the `gemini` executable.
- Use headless mode with `-p` so the prompt can be sent non-interactively.
- Pass `--model <resolved-model>` through verbatim.
- Request structured output with `--output-format json`.
- Send the prompt over stdin and read stdout.

Armitage should not translate aliases or normalize Gemini model names. The model string supplied by the user is forwarded as-is so version-specific choices remain explicit and visible.

## Output Parsing

Gemini CLI wraps model output in JSON rather than returning the raw classification text directly.

The Gemini backend adapter should:
- Parse the JSON wrapper returned by `gemini --output-format json`.
- Extract the assistant text payload.
- Pass that extracted text into the existing `parse_classification()` / `parse_batch_classifications()` flow.

If the wrapper is malformed or the expected text field is missing, return an `Error::LlmInvocation` explaining that Gemini output could not be parsed.

## Error Handling

Expected user-facing failures:
- Unknown backend: include `claude`, `codex`, and `gemini` in the error.
- Missing backend after CLI/config resolution: return an explicit configuration error.
- Missing model when backend resolves to `gemini`: return an explicit configuration error.
- Gemini CLI spawn failure: return the existing LLM invocation error with backend name.
- Gemini CLI non-zero exit: return stderr in the invocation error, consistent with other backends.
- Gemini JSON wrapper parse failure: return an invocation error that identifies the Gemini response as invalid.

## Testing

Add or update tests for:
- Backend parsing accepts `gemini`.
- Unknown backend errors mention `gemini`.
- Triaging with no resolved backend fails.
- Triaging with `backend=gemini` and no resolved model fails.
- CLI flags override config for backend/model resolution.
- Gemini JSON wrapper extraction returns the inner text payload.
- Existing Claude wrapper parsing remains intact.

The implementation should preserve the existing test structure in `src/triage/llm.rs` and `src/cli/triage.rs`, adding focused unit tests instead of broad integration coverage.

## References

- Gemini CLI authentication and headless guidance: <https://geminicli.com/docs/get-started/authentication/>
- Gemini CLI command reference: <https://geminicli.com/docs/cli/cli-reference/>
- Gemini model naming and stable/versioned model guidance: <https://ai.google.dev/gemini-api/docs/models>
