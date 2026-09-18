//! Tauri-side bridge for QuStudio's "AI Assist" features -- the mascot chat
//! panel (`llm_chat`), inline-completion autocomplete (`llm_complete`),
//! "Fix with AI" (`llm_fix_error`), and "Generate/Transform" (`llm_transform_code`)
//! -- plus the hosted-provider settings commands (`get_llm_provider_config`,
//! `set_llm_provider_config`, `test_llm_provider`). Entirely separate from
//! `qu-interp`'s own `llm_load(...)`/`.generate(...)` script builtins
//! (`llm_bridge.rs` in that crate) -- this module never touches `qu-interp`
//! or its `Value` type at all.
//!
//! **Provider abstraction (added on top of the original local-only
//! design).** All four AI-feature commands used to talk directly to
//! `qu_llm::LlmModel`. They now build a backend-agnostic
//! `llm_providers::GenRequest` (system prompt + user message + max_tokens,
//! same Qu-syntax-priming discipline as before -- see `QU_SYNTAX_PRIMER`
//! below, now folded into `system` instead of a hand-formatted Zephyr
//! string) and hand it to `dispatch_with_settings`, which picks the
//! backend the user configured (Local/OpenAI/Anthropic -- see
//! `llm_providers::ProviderKind`) and falls back to the local model on any
//! hosted-provider failure (network error, bad key, rate limit -- see that
//! function's own doc comment). `llm_providers.rs` owns the actual
//! `LlmProvider` trait and its three implementations; this module only
//! orchestrates.
//!
//! **Why this whole module compiles either way, `llm` feature on or off.**
//! `tauri::generate_handler!` in `main.rs` is a plain macro-rules-style
//! list of command names with no support for a per-item `#[cfg(...)]`
//! (confirmed by actually trying it: `error: expected identifier`) -- every
//! name listed there must exist under every build config `main.rs` itself
//! builds under. So `llm_chat`/`llm_complete`/etc. are always defined; the
//! `llm` feature gate now lives inside `run_local` (see below) instead of
//! inside each command body, since a hosted-provider call needs no local
//! model at all and should keep working in a build without `llm`.
//!
//! **Lazy, one-instance-per-process loading.** Unchanged from before this
//! module gained hosted providers: nothing here loads the local model at
//! app startup (`LlmState::default()` is just an empty `Mutex<None>`,
//! managed in `main.rs` unconditionally since it's free) -- the first call
//! that actually reaches `run_local` (either because Local is the
//! configured provider, or because a hosted call failed and this is the
//! fallback) pays the real load cost (parsing the ~669MB GGUF file into
//! `candle` tensors); every call after that reuses the same warm
//! `Arc<LlmModel>` from `tauri::State`.
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::llm_providers::{self, GenRequest, LlmProvider, ProviderKind};

#[cfg(feature = "llm")]
use std::sync::{Arc, Mutex};

/// Lives in `tauri::State` (see `main.rs`'s `.manage(LlmState::default())`)
/// -- managed unconditionally (it's an empty `Mutex` either way) but its
/// `model` field only exists, and is only ever populated, in an
/// `llm`-enabled build. `None` until the first call that actually needs
/// the local model (see `run_local`).
#[derive(Default)]
pub struct LlmState {
    #[cfg(feature = "llm")]
    model: Mutex<Option<Arc<qu_llm::LlmModel>>>,
}

#[cfg(feature = "llm")]
fn get_or_load_model(state: &LlmState) -> Result<Arc<qu_llm::LlmModel>, String> {
    let mut guard = state
        .model
        .lock()
        .map_err(|_| "llm bridge: model lock was poisoned by an earlier panic".to_string())?;
    if let Some(model) = guard.as_ref() {
        return Ok(model.clone());
    }
    let loaded = qu_llm::load("")?;
    let arc = Arc::new(loaded);
    *guard = Some(arc.clone());
    Ok(arc)
}

/// Runs a `GenRequest` against the local candle model, loading it first if
/// needed. The ONE place the `llm` feature gate lives now -- every other
/// function in this module (including the four command handlers) compiles
/// and runs identically either way; only this function's body differs.
#[cfg(feature = "llm")]
fn run_local(state: &LlmState, req: &GenRequest) -> Result<String, String> {
    let model = get_or_load_model(state)?;
    let provider = llm_providers::LocalProvider { model: &model };
    provider.generate(req)
}

#[cfg(not(feature = "llm"))]
fn run_local(_state: &LlmState, _req: &GenRequest) -> Result<String, String> {
    let _ = _state;
    Err("the local model needs a build with the `llm` Cargo feature enabled \
         (`cargo tauri dev --features llm`) -- this build doesn't have it."
        .to_string())
}

/// Compact, concrete primer on Qu's REAL syntax, injected into every
/// system prompt (mascot chat, fix-error, transform/generate) for every
/// backend, local or hosted. TinyLlama-Chat has seen vastly more
/// Python/MATLAB/Julia in training than Qu, so left unprimed it confidently
/// produces plausible-looking code in THOSE languages' syntax instead --
/// e.g. 1-indexing, `end`-less indentation blocks, tuple-unpacking
/// `[a, b] = f(x)`. Hosted frontier models know MUCH less about Qu specifically
/// than about mainstream languages too (it's not a widely-known language),
/// so the same priming discipline matters for them as well -- this constant
/// is shared by every backend rather than being a local-model-only
/// workaround. Every line below is a real, verified-working Qu construct,
/// pulled from `catalog/*.qu` (`qu_peak_finding.qu`,
/// `qu_multiple_dispatch.qu`, `qu_qr_svd.qu`) and confirmed directly
/// against `qu-syntax`/`qu-interp` source and their own test suite for the
/// two easy-to-misremember operators (`|>`, `@`) rather than written from
/// memory.
///
/// Deliberately short: on the local CPU-only greedy model prompt
/// *processing* time scales with token count same as generation does, so a
/// page of prose here would slow down every single call, not just make it
/// more correct -- and on hosted providers, every token is also literal
/// billed cost. A handful of real, dense examples beats a long abstract
/// description at the same cost either way.
const QU_SYNTAX_PRIMER: &str = "\
Real Qu syntax (distinct from MATLAB/Python/Julia -- follow exactly):
- 0-indexed arrays: x[0] is the first element; x[0:5] is a 0-based slice.
- Blocks close with `end`/`end function`, never indentation:
    function describe(x: vec)
        print \"length {length(x)}\"
    end function
- `x: vec` / `x: mat` / `x: num` param tags define separate overloads,
  dispatched on the caller's actual argument type.
- Keyword args: findpeaks(x, min_peak_height=0.5, min_peak_distance=10)
- Multi-result builtins (qr, svd, findpeaks, sequential(...).fit(...))
  return one Model with named fields, not a tuple: r = qr(A); r.q; r.r
- `|>` pipes the left value in as the first argument: a |> f(b) = f(a, b)
- `@expr` reassigns the root variable to the whole expression's result:
  @x.append(4) means x = x.append(4)
- String interpolation with format specs: print(\"loss={loss:.4f}\")
- A lone `#%%` marks a runnable cell boundary, like a Jupyter cell.
";

/// Cap on how much of `context`/`prefix` (current buffer + last error,
/// already concatenated by the frontend -- see `App.tsx`'s
/// `buildMascotContext`) gets folded into a prompt, or sent to a hosted
/// API. Keeps the TAIL, not the head: the shape of a Qu script that errors,
/// and of a Qu error message itself, both put the actually-relevant line
/// near the end.
const CONTEXT_CHAR_BUDGET: usize = 2000;

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }
    let skip = char_count - max_chars;
    format!("...(truncated)...{}", s.chars().skip(skip).collect::<String>())
}

const CHAT_MAX_TOKENS: usize = 200;

/// Every AI-feature command's actual return type -- what backend answered,
/// not just the text. Making the fallback visible (per the brief's "make
/// the fallback visible to the user, not silent") means the frontend can
/// show something like "answered by OpenAI" or "OpenAI failed, answered by
/// Local model instead" rather than the user having no idea their hosted
/// call silently didn't happen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmAnswer {
    pub text: String,
    /// Human-readable backend name, e.g. `"OpenAI"`, `"Anthropic"`,
    /// `"Local (TinyLlama)"` -- see `ProviderKind::label`.
    pub backend: String,
    /// True when the configured provider failed and this answer actually
    /// came from the local-model fallback instead.
    pub fell_back: bool,
    /// Set only when `fell_back` is true (or provider selection itself hit
    /// a snag) -- a short, user-facing explanation, e.g. "OpenAI failed
    /// (HTTP 401: ...); used the local model instead."
    pub note: Option<String>,
}

/// Resolves the saved provider settings and runs `req` through whichever
/// provider is configured, falling back to the local model on any hosted-
/// provider failure. Takes `ProviderSettings` directly (not an `AppHandle`)
/// so it's callable from a plain unit test without any real Tauri runtime
/// -- `dispatch` (below) is the thin `AppHandle`-resolving wrapper the
/// actual commands call.
///
/// Fallback only ever goes hosted-provider -> local, never local -> hosted
/// (a local failure, e.g. a corrupt GGUF cache, has nothing a hosted
/// fallback could safely improvise -- and falling back TO a network call
/// the user never opted into would violate "Qu Studio should keep working
/// offline" from the other direction). If the configured provider IS Local
/// and it fails, that error is returned as-is.
fn dispatch_with_settings(
    state: &LlmState,
    settings: &llm_providers::ProviderSettings,
    req: GenRequest,
) -> Result<LlmAnswer, String> {
    let chosen = settings.provider;

    let primary: Result<String, String> = match chosen {
        ProviderKind::Local => run_local(state, &req),
        ProviderKind::Openai => match settings.openai_api_key.as_deref() {
            Some(key) if !key.is_empty() => {
                let provider = llm_providers::OpenAiProvider {
                    api_key: key.to_string(),
                    model: settings.openai_model.clone(),
                };
                provider.generate(&req)
            }
            _ => Err("OpenAI is selected in AI Provider settings but no API key is saved yet.".to_string()),
        },
        ProviderKind::Anthropic => match settings.anthropic_api_key.as_deref() {
            Some(key) if !key.is_empty() => {
                let provider = llm_providers::AnthropicProvider {
                    api_key: key.to_string(),
                    model: settings.anthropic_model.clone(),
                };
                provider.generate(&req)
            }
            _ => Err("Anthropic is selected in AI Provider settings but no API key is saved yet.".to_string()),
        },
    };

    match primary {
        Ok(text) => Ok(LlmAnswer { text, backend: chosen.label().to_string(), fell_back: false, note: None }),
        Err(primary_err) if chosen == ProviderKind::Local => Err(primary_err),
        Err(primary_err) => match run_local(state, &req) {
            Ok(text) => Ok(LlmAnswer {
                text,
                backend: ProviderKind::Local.label().to_string(),
                fell_back: true,
                note: Some(format!("{} failed ({primary_err}); used the local model instead.", chosen.label())),
            }),
            Err(local_err) => Err(format!(
                "{} failed: {primary_err}. Local fallback also failed: {local_err}",
                chosen.label()
            )),
        },
    }
}

/// Resolves the app-config directory, loads the saved provider settings,
/// and calls `dispatch_with_settings`. A missing/unresolvable config dir
/// degrades to "behave as Local" (same defaulting `ProviderSettings`
/// itself already does for a missing file) rather than hard-erroring --
/// consistent with this whole feature's "never break a working local-only
/// install" design goal.
fn dispatch(app: &tauri::AppHandle, state: &LlmState, req: GenRequest) -> Result<LlmAnswer, String> {
    let settings = match app.path_resolver().app_config_dir() {
        Some(dir) => llm_providers::load_settings(&dir),
        None => llm_providers::ProviderSettings::default(),
    };
    dispatch_with_settings(state, &settings, req)
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmChatRequest {
    pub prompt: String,
    /// Already-assembled context string (current buffer + last error, or
    /// either alone) -- see `App.tsx`'s `buildMascotContext`. `None`/empty
    /// when there's nothing relevant yet.
    pub context: Option<String>,
}

fn chat_system_prompt(context: Option<&str>) -> String {
    let mut system = format!(
        "You are Qu-bot, the friendly built-in mascot assistant for Qu Studio, an IDE for the \
         Qu scientific scripting language (a MATLAB/Julia-like language for signal processing, \
         linear algebra, and machine learning).\n\n{QU_SYNTAX_PRIMER}\n\
         Answer the user's question directly. Be as concise as possible: no preamble, don't \
         restate the question, no filler -- just the answer, in as few sentences as it takes.",
    );
    if let Some(ctx) = context {
        let trimmed = ctx.trim();
        if !trimmed.is_empty() {
            system.push_str(
                "\n\nContext that may help answer (the user's current editor buffer and/or \
                 their last run's error output):\n",
            );
            system.push_str(&truncate_chars(trimmed, CONTEXT_CHAR_BUDGET));
        }
    }
    system
}

/// `llm_chat(request: { prompt, context })` -- the mascot chat panel's one
/// command. Routes through `dispatch`: whichever provider is configured
/// (default Local) answers, with an automatic local fallback on hosted
/// failure. Errors (no provider reachable at all) come back as
/// `Err(String)`, which Tauri surfaces to the JS side as a rejected
/// promise -- the frontend shows this as a mascot error bubble.
#[tauri::command]
pub fn llm_chat(
    request: LlmChatRequest,
    app: tauri::AppHandle,
    state: State<LlmState>,
) -> Result<LlmAnswer, String> {
    let system = chat_system_prompt(request.context.as_deref());
    let user = request.prompt.clone();
    dispatch(&app, &state, GenRequest { system, user, max_tokens: CHAT_MAX_TOKENS, local_raw_prompt: None })
}

const COMPLETE_MAX_TOKENS_CAP: usize = 32;
const COMPLETE_DEFAULT_MAX_TOKENS: usize = 16;

#[derive(Debug, Clone, Deserialize)]
pub struct LlmCompleteRequest {
    pub prefix: String,
    pub max_tokens: Option<usize>,
}

/// `llm_complete(request: { prefix, max_tokens? })` -- Monaco's inline
/// ghost-text provider calls this with the code before the cursor. The
/// LOCAL backend still gets the exact pre-existing behavior (a raw prefix
/// continuation, no chat template -- see `GenRequest::local_raw_prompt`);
/// a hosted backend, which has no raw-continuation endpoint, gets a short
/// "continue this code" system/user pair instead. `max_tokens` is clamped
/// to `COMPLETE_MAX_TOKENS_CAP` regardless of what the frontend passes.
#[tauri::command]
pub fn llm_complete(
    request: LlmCompleteRequest,
    app: tauri::AppHandle,
    state: State<LlmState>,
) -> Result<LlmAnswer, String> {
    let max_tokens = request
        .max_tokens
        .unwrap_or(COMPLETE_DEFAULT_MAX_TOKENS)
        .min(COMPLETE_MAX_TOKENS_CAP)
        .max(1);
    let prefix = truncate_chars(&request.prefix, CONTEXT_CHAR_BUDGET);
    let system = format!(
        "You are completing Qu code inside Qu Studio.\n\n{QU_SYNTAX_PRIMER}\n\
         Continue the code below with a short, syntactically valid continuation. Reply with \
         ONLY the continuation text (no repetition of the prefix, no explanation, no markdown \
         fences)."
    );
    let user = format!("Continue this Qu code:\n{prefix}");
    dispatch(
        &app,
        &state,
        GenRequest { system, user, max_tokens, local_raw_prompt: Some(prefix) },
    )
}

const FIX_MAX_TOKENS: usize = 256;
const TRANSFORM_MAX_TOKENS: usize = 256;
const FIX_ERROR_CHAR_BUDGET: usize = 600;

#[derive(Debug, Clone, Deserialize)]
pub struct LlmFixErrorRequest {
    pub code: String,
    pub error: String,
}

fn fix_system_prompt() -> String {
    format!(
        "You are a Qu code-fixing assistant inside Qu Studio, an IDE for the Qu scientific \
         scripting language.\n\n{QU_SYNTAX_PRIMER}\n\
         The user's script below failed with the error shown. Reply with ONLY the corrected, \
         complete Qu script that fixes it -- no explanation, no restating the error, no \
         markdown code fences, no commentary before or after the code."
    )
}

/// `llm_fix_error(request: { code, error })` -- the "Fix with AI" button's
/// command. Returns the model's best guess at a corrected FULL script (not
/// a diff/patch -- the frontend diffs it against the current buffer itself
/// for the review-before-apply UI). Never applied automatically.
#[tauri::command]
pub fn llm_fix_error(
    request: LlmFixErrorRequest,
    app: tauri::AppHandle,
    state: State<LlmState>,
) -> Result<LlmAnswer, String> {
    let system = fix_system_prompt();
    let user = format!(
        "Script:\n{}\n\nError:\n{}",
        truncate_chars(request.code.trim(), CONTEXT_CHAR_BUDGET),
        truncate_chars(request.error.trim(), FIX_ERROR_CHAR_BUDGET)
    );
    let mut answer = dispatch(&app, &state, GenRequest { system, user, max_tokens: FIX_MAX_TOKENS, local_raw_prompt: None })?;
    answer.text = strip_code_fence(&answer.text);
    Ok(answer)
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmTransformRequest {
    pub instruction: String,
    pub code: Option<String>,
    pub selection: Option<String>,
}

fn transform_system_prompt(has_selection: bool) -> String {
    if has_selection {
        format!(
            "You are a Qu code-transformation assistant inside Qu Studio, an IDE for the Qu \
             scientific scripting language.\n\n{QU_SYNTAX_PRIMER}\n\
             The user selected a piece of Qu code and describes how to change it. Reply with \
             ONLY the rewritten Qu code for that selection -- no explanation, no markdown code \
             fences, no commentary before or after the code."
        )
    } else {
        format!(
            "You are a Qu code-generation assistant inside Qu Studio, an IDE for the Qu \
             scientific scripting language.\n\n{QU_SYNTAX_PRIMER}\n\
             The user describes Qu code they want written. Reply with ONLY the new Qu code -- \
             no explanation, no markdown code fences, no commentary before or after the code."
        )
    }
}

fn build_transform_user(instruction: &str, code_context: Option<&str>, selection: Option<&str>) -> String {
    let has_selection = selection.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let mut user = String::new();
    if let Some(ctx) = code_context {
        let trimmed = ctx.trim();
        if !trimmed.is_empty() {
            user.push_str("Current file (context only, e.g. existing variable names):\n");
            user.push_str(&truncate_chars(trimmed, CONTEXT_CHAR_BUDGET));
            user.push_str("\n\n");
        }
    }
    if has_selection {
        user.push_str("Selected code to transform:\n");
        user.push_str(&truncate_chars(selection.unwrap().trim(), CONTEXT_CHAR_BUDGET));
        user.push_str("\n\n");
    }
    user.push_str("Instruction: ");
    user.push_str(instruction.trim());
    user
}

/// `llm_transform_code(request: { instruction, code, selection })` -- the
/// "Generate/Transform" button's one command, covering both modes
/// (selection present -> rewrite it; absent -> generate new code). Same
/// review-before-apply contract as `llm_fix_error`.
#[tauri::command]
pub fn llm_transform_code(
    request: LlmTransformRequest,
    app: tauri::AppHandle,
    state: State<LlmState>,
) -> Result<LlmAnswer, String> {
    let has_selection = request.selection.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let system = transform_system_prompt(has_selection);
    let user = build_transform_user(&request.instruction, request.code.as_deref(), request.selection.as_deref());
    let mut answer = dispatch(
        &app,
        &state,
        GenRequest { system, user, max_tokens: TRANSFORM_MAX_TOKENS, local_raw_prompt: None },
    )?;
    answer.text = strip_code_fence(&answer.text);
    Ok(answer)
}

/// Extracts just the code from a model reply, defensively, on top of the
/// "no markdown fences, no commentary" prompt instruction -- see the
/// original implementation's own note (kept verbatim below) on why a
/// simple "strip fences off the whole reply" guess was wrong against a
/// real reply.
///
/// NOT a "strip fences off the whole-reply wrapper" guess -- that was
/// tried first and turned out wrong against a REAL reply:
/// `"Here's the corrected Qu script that fixes the error:\n\n\`\`\`\nx = \
/// [1, 2, 3]\nprint(length(x))\n\`\`\`\n\nThis script defines..."` -- prose
/// BEFORE the fence and prose AFTER it, not just a bare fenced-whole-reply.
/// So this scans for the first fence-opening line (\`\`\` or \`\`\`qu) and
/// the NEXT fence-closing line after it, and keeps only what's strictly
/// between them. Falls back to the trimmed whole reply when there's no
/// fence pair at all.
fn strip_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    if let Some(start) = lines.iter().position(|l| l.trim_start().starts_with("```")) {
        if let Some(end_offset) = lines[start + 1..].iter().position(|l| l.trim_start().starts_with("```")) {
            let end = start + 1 + end_offset;
            return lines[start + 1..end].join("\n").trim().to_string();
        }
    }
    trimmed.to_string()
}

// ---------------------------------------------------------------------
// Provider settings commands
// ---------------------------------------------------------------------

fn config_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path_resolver()
        .app_config_dir()
        .ok_or_else(|| "could not resolve the app config directory for this platform".to_string())
}

/// `get_llm_provider_config()` -- the settings panel's initial load. Never
/// returns the real API keys, only whether one is saved (see
/// `llm_providers::PublicProviderSettings`).
#[tauri::command]
pub fn get_llm_provider_config(app: tauri::AppHandle) -> Result<llm_providers::PublicProviderSettings, String> {
    let dir = config_dir(&app)?;
    let settings = llm_providers::load_settings(&dir);
    Ok((&settings).into())
}

/// `set_llm_provider_config(update)` -- saves the selected provider, model
/// names, and (optionally) new API keys. See
/// `llm_providers::ProviderSettingsUpdate`'s own doc comment for the
/// "`None` keeps, `Some(\"\")` clears" key semantics. Returns the same
/// redacted DTO `get_llm_provider_config` does, so the settings panel can
/// refresh its "key saved?" indicator from the response instead of a
/// second round-trip.
#[tauri::command]
pub fn set_llm_provider_config(
    update: llm_providers::ProviderSettingsUpdate,
    app: tauri::AppHandle,
) -> Result<llm_providers::PublicProviderSettings, String> {
    let dir = config_dir(&app)?;
    let settings = llm_providers::apply_update(&dir, update)?;
    Ok((&settings).into())
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestLlmProviderRequest {
    pub provider: ProviderKind,
    /// The key to test. The settings panel sends whatever's currently in
    /// the password field -- which may not be saved yet -- so "Test
    /// connection" can validate a key BEFORE the user commits to saving
    /// it. `None`/empty falls back to whatever's already saved for that
    /// provider, so re-testing an already-saved key without re-pasting it
    /// also works.
    pub api_key: Option<String>,
    pub model: Option<String>,
}

/// `test_llm_provider(request: { provider, api_key?, model? })` -- the
/// settings panel's "Test connection" button. Makes ONE real, cheap call
/// (5 max_tokens, a one-word expected reply) to confirm a key actually
/// works before the user saves it. `Local` always "succeeds" without a
/// network call -- there's nothing to test, and the button should still
/// make sense when Local is selected (a no-op success rather than a
/// disabled/hidden case the frontend would need to special-case).
#[tauri::command]
pub fn test_llm_provider(request: TestLlmProviderRequest, app: tauri::AppHandle) -> Result<(), String> {
    match request.provider {
        ProviderKind::Local => Ok(()),
        ProviderKind::Openai => {
            let dir = config_dir(&app)?;
            let saved = llm_providers::load_settings(&dir);
            let key = request
                .api_key
                .filter(|k| !k.is_empty())
                .or(saved.openai_api_key)
                .ok_or_else(|| "no OpenAI API key to test -- paste one first".to_string())?;
            let model = request.model.filter(|m| !m.is_empty()).unwrap_or(saved.openai_model);
            llm_providers::openai_test_connection(&key, &model)
        }
        ProviderKind::Anthropic => {
            let dir = config_dir(&app)?;
            let saved = llm_providers::load_settings(&dir);
            let key = request
                .api_key
                .filter(|k| !k.is_empty())
                .or(saved.anthropic_api_key)
                .ok_or_else(|| "no Anthropic API key to test -- paste one first".to_string())?;
            let model = request.model.filter(|m| !m.is_empty()).unwrap_or(saved.anthropic_model);
            llm_providers::anthropic_test_connection(&key, &model)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_providers::ProviderSettings;

    fn local_settings() -> ProviderSettings {
        ProviderSettings::default()
    }

    /// Exercises ONLY the local, `llm`-off error path -- no model, no
    /// network. Mirrors `qu-llm`'s own "cheap, hermetic tests in the
    /// default suite; real generation is `#[ignore]`d" split.
    #[cfg(not(feature = "llm"))]
    #[test]
    fn chat_without_llm_feature_returns_a_clear_error() {
        let state = LlmState::default();
        let req = GenRequest { system: "s".into(), user: "hi".into(), max_tokens: 8, local_raw_prompt: None };
        let err = dispatch_with_settings(&state, &local_settings(), req).unwrap_err();
        assert!(err.contains("llm"), "got: {err}");
    }

    #[test]
    fn strip_code_fence_extracts_code_between_prose() {
        let reply = "Here's the corrected Qu script that fixes the error:\n\n\
                     ```\nx = [1, 2, 3]\nprint(length(x))\n```\n\n\
                     This script defines the `length` function and calls it.";
        assert_eq!(strip_code_fence(reply), "x = [1, 2, 3]\nprint(length(x))");
    }

    #[test]
    fn strip_code_fence_passes_through_a_fence_free_reply() {
        assert_eq!(strip_code_fence("x = [1, 2, 3]\nprint(length(x))"), "x = [1, 2, 3]\nprint(length(x))");
    }

    #[test]
    fn strip_code_fence_handles_a_language_tagged_fence() {
        let reply = "```qu\ny = sum(x)\n```";
        assert_eq!(strip_code_fence(reply), "y = sum(x)");
    }

    /// A provider selected with no saved key must error clearly, not panic
    /// or silently fall back without telling the caller why (fallback IS
    /// still attempted -- see the next test -- this one covers the
    /// no-fallback-available case, i.e. an `llm`-off build).
    #[cfg(not(feature = "llm"))]
    #[test]
    fn openai_selected_without_key_and_no_local_fallback_available_errors_clearly() {
        let state = LlmState::default();
        let mut settings = local_settings();
        settings.provider = ProviderKind::Openai;
        let req = GenRequest { system: "s".into(), user: "hi".into(), max_tokens: 8, local_raw_prompt: None };
        let err = dispatch_with_settings(&state, &settings, req).unwrap_err();
        assert!(err.contains("OpenAI"), "got: {err}");
        assert!(err.contains("Local fallback also failed"), "got: {err}");
    }

    /// Same shape for Anthropic.
    #[cfg(not(feature = "llm"))]
    #[test]
    fn anthropic_selected_without_key_and_no_local_fallback_available_errors_clearly() {
        let state = LlmState::default();
        let mut settings = local_settings();
        settings.provider = ProviderKind::Anthropic;
        let req = GenRequest { system: "s".into(), user: "hi".into(), max_tokens: 8, local_raw_prompt: None };
        let err = dispatch_with_settings(&state, &settings, req).unwrap_err();
        assert!(err.contains("Anthropic"), "got: {err}");
    }

    /// The fallback path, exercised for real: point `provider` at a hosted
    /// backend with an obviously-invalid key so the HTTP call fails fast
    /// with a 401 (still a REAL network call -- this is intentionally not
    /// mocked, since the fallback wiring itself is what's under test, not
    /// OpenAI's response shape which `llm_providers`'s own tests already
    /// cover against fixtures). Requires network access and is skipped in
    /// the default `cargo test` run.
    ///   cargo test -p qu-studio -- --ignored --nocapture fallback_to_local
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "makes a real network call to OpenAI with a deliberately invalid key, then loads the real local model as the fallback"]
    fn fallback_to_local_on_hosted_failure_real() {
        let state = LlmState::default();
        let mut settings = local_settings();
        settings.provider = ProviderKind::Openai;
        settings.openai_api_key = Some("sk-deliberately-invalid-for-this-test".to_string());
        let req = GenRequest {
            system: "You are a helpful assistant.".into(),
            user: "Say hi in one word.".into(),
            max_tokens: 8,
            local_raw_prompt: None,
        };
        let answer = dispatch_with_settings(&state, &settings, req).expect("local fallback should succeed");
        assert!(answer.fell_back, "expected fell_back=true, got {answer:?}");
        assert_eq!(answer.backend, "Local (TinyLlama)");
        assert!(answer.note.as_deref().unwrap_or("").contains("OpenAI failed"));
    }
}
