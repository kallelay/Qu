//! Tauri-side bridge to `qu-llm` (feature `llm`, off by default -- see
//! `Cargo.toml`'s own comment) for two QuStudio UI features: the mascot
//! chat panel (`llm_chat`) and inline-completion autocomplete
//! (`llm_complete`). Entirely separate from `qu-interp`'s own
//! `llm_load(...)`/`.generate(...)` script builtins (`llm_bridge.rs` in
//! that crate) -- this module never touches `qu-interp` or its `Value`
//! type at all, it drives `qu_llm::LlmModel` directly.
//!
//! **Why this whole module compiles either way, `llm` feature on or off.**
//! `tauri::generate_handler!` in `main.rs` is a plain macro-rules-style
//! list of command names with no support for a per-item `#[cfg(...)]`
//! (confirmed by actually trying it: `error: expected identifier`) -- every
//! name listed there must exist under every build config `main.rs` itself
//! builds under. So `llm_chat`/`llm_complete` are always defined, and the
//! feature gate lives INSIDE their bodies instead: with `llm` off, both
//! just return a friendly "not built with this feature" `Err`, never
//! touching `qu_llm` (which isn't even a resolvable crate in that build --
//! it's an optional dependency, so `#[cfg(feature = "llm")] use qu_llm...`
//! is the only place its name appears at all).
//!
//! **Lazy, one-instance-per-process loading.** Nothing here loads the
//! model at app startup (`LlmState::default()` is just an empty
//! `Mutex<None>`, managed in `main.rs` unconditionally since it's free) --
//! the first `llm_chat` or `llm_complete` call in an `llm`-enabled build
//! pays the real load cost (parsing the ~669MB GGUF file into `candle`
//! tensors); every call after that reuses the same warm `Arc<LlmModel>`
//! from `tauri::State`. This matches the brief's explicit requirement:
//! normal QuStudio startup must stay fast for users who never touch the
//! mascot or autocomplete.
//!
//! **Why the outer `Mutex` guard is dropped before `generate` runs.**
//! `get_or_load_model` only holds `LlmState`'s own lock long enough to
//! either return the already-loaded `Arc` or load-and-cache a new one --
//! the guard goes out of scope the instant this function returns, well
//! before the caller invokes `model.generate(...)`. `LlmModel::generate`
//! has its OWN internal `Mutex` around the candle weights/KV-cache (see
//! `qu-llm`'s own doc comment on that field), so two overlapping
//! `llm_chat`/`llm_complete` calls just serialize on that inner lock
//! instead of blocking each other out here on a completely unrelated
//! "is the model loaded yet" check.
use serde::Deserialize;
use tauri::State;

#[cfg(feature = "llm")]
use std::sync::{Arc, Mutex};

/// Lives in `tauri::State` (see `main.rs`'s `.manage(LlmState::default())`)
/// -- managed unconditionally (it's an empty `Mutex` either way) but its
/// `model` field only exists, and is only ever populated, in an
/// `llm`-enabled build. `None` until the first real `llm_chat`/
/// `llm_complete` call.
#[derive(Default)]
pub struct LlmState {
    #[cfg(feature = "llm")]
    model: Mutex<Option<Arc<qu_llm::LlmModel>>>,
}

/// Returns the already-loaded model, or loads the default known model
/// (`qu_llm::load("")` -- TinyLlama-1.1B-Chat-v1.0, see that function's own
/// doc comment) and caches it in `state` first. The load itself (first call
/// only) can take real time -- parsing the GGUF file and building
/// `candle`'s tensors -- so this is deliberately NOT called from app
/// startup, only from inside a Tauri command a user action actually
/// triggered.
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

/// Compact, concrete primer on Qu's REAL syntax, injected into every
/// chat-style system prompt (mascot chat, fix-error, transform/generate).
/// TinyLlama-Chat has seen vastly more Python/MATLAB/Julia in training than
/// Qu, so left unprimed it confidently produces plausible-looking code in
/// THOSE languages' syntax instead -- e.g. 1-indexing, `end`-less
/// indentation blocks, tuple-unpacking `[a, b] = f(x)`. Every line below is
/// a real, verified-working Qu construct, pulled from `catalog/*.qu`
/// (`qu_peak_finding.qu`, `qu_multiple_dispatch.qu`, `qu_qr_svd.qu`) and
/// confirmed directly against `qu-syntax`/`qu-interp` source and their own
/// test suite for the two easy-to-misremember operators (`|>`, `@`) rather
/// than written from memory -- an initial guess that `@` was a
/// self-mutating *method-call* prefix (`@sort()`) was wrong; it's actually
/// a whole-statement "reassign the root variable" desugar, confirmed at
/// `qu-syntax/src/lib.rs`'s statement parser and exercised in
/// `qu-interp/tests/acceptance.rs`.
///
/// Deliberately short: this is a CPU-only, greedy 1.1B model where prompt
/// *processing* time scales with token count same as generation does (see
/// this module's own top-level doc comment) -- a page of prose here would
/// slow down every single mascot/fix/transform call, not just make them
/// more correct. A handful of real, dense examples beats a long abstract
/// description at the same token cost.
#[cfg(feature = "llm")]
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

/// Builds the TinyLlama-Chat prompt (its own Zephyr-style chat template --
/// `<|system|>`/`<|user|>`/`<|assistant|>` turns separated by `</s>`; this
/// is the exact format `TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF`'s model
/// card documents, not a guess) for the mascot chat feature. `context`, when
/// present, is folded into the system turn as extra grounding -- e.g. the
/// current editor buffer and/or the last error message, per the brief's
/// "basic awareness of context that's cheap to provide" scope (deliberately
/// NOT a RAG pipeline: just the obviously relevant text, capped so a whole
/// large file doesn't blow up prompt-processing time on a CPU-only model).
#[cfg(feature = "llm")]
fn build_chat_prompt(question: &str, context: Option<&str>) -> String {
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
    format!("<|system|>\n{system}</s>\n<|user|>\n{question}</s>\n<|assistant|>\n")
}

/// Cap on how much of `context`/`prefix` (current buffer + last error,
/// already concatenated by the frontend -- see `App.tsx`'s
/// `buildMascotContext`) gets folded into a prompt. This is a CPU-only,
/// greedy-decoding 1.1B model: prompt *processing* time scales with token
/// count same as generation does, so handing it an entire multi-thousand-
/// line file would make every mascot question/completion slow regardless
/// of how short the answer is. A few thousand characters is enough to cover
/// "why did this small script fail" without that blowup -- keeps the TAIL,
/// not the head: the shape of a Qu script that errors, and of a Qu error
/// message itself, both put the actually-relevant line near the end.
#[cfg(feature = "llm")]
const CONTEXT_CHAR_BUDGET: usize = 2000;

#[cfg(feature = "llm")]
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }
    let skip = char_count - max_chars;
    format!("...(truncated)...{}", s.chars().skip(skip).collect::<String>())
}

/// Chat responses get a real budget -- a mascot reply is meant to be a
/// short paragraph, not one line. Still deliberately far short of
/// "unbounded": see this module's own doc comment / IMPL.md for the
/// measured tokens/sec this is based on.
#[cfg(feature = "llm")]
const CHAT_MAX_TOKENS: usize = 200;

// `#[allow(dead_code)]` on both fields: in a build WITHOUT `llm`, `llm_chat`
// never reads either field (see its `not(feature = "llm")` branch below) --
// that's correct, expected behavior for that build, not a real bug to warn
// about.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
pub struct LlmChatRequest {
    pub prompt: String,
    /// Already-assembled context string (current buffer + last error, or
    /// either alone) -- see `App.tsx`'s `buildMascotContext`. `None`/empty
    /// when there's nothing relevant yet (e.g. no run has happened).
    pub context: Option<String>,
}

/// `llm_chat(request: { prompt, context })` -- the mascot chat panel's one
/// command. Loads the model on first call (see `get_or_load_model`), then
/// runs one greedy completion of the Zephyr-style chat prompt built by
/// `build_chat_prompt`. Errors (model failed to load, generation failed,
/// or this build simply doesn't have the `llm` feature) come back as
/// `Err(String)`, which Tauri surfaces to the JS side as a rejected
/// promise -- the frontend shows this as a mascot error bubble rather than
/// a silent failure.
#[tauri::command]
pub fn llm_chat(request: LlmChatRequest, state: State<LlmState>) -> Result<String, String> {
    chat_with_state(&state, request)
}

/// The actual `llm_chat` logic, factored out from the `#[tauri::command]`
/// wrapper above so it can be exercised by a real test (see
/// `real_chat_produces_a_coherent_reply` below) without needing to
/// construct a real `tauri::State` -- `&LlmState` is all this needs, and a
/// plain `LlmState::default()` is one to make. The command wrapper itself
/// is trivial argument/return marshaling on top of this, already exercised
/// implicitly by every other command in this crate using the same
/// `#[tauri::command]` macro.
fn chat_with_state(state: &LlmState, request: LlmChatRequest) -> Result<String, String> {
    #[cfg(feature = "llm")]
    {
        let model = get_or_load_model(state)?;
        let prompt = build_chat_prompt(&request.prompt, request.context.as_deref());
        let reply = model.generate(&prompt, CHAT_MAX_TOKENS)?;
        Ok(reply.trim().to_string())
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (request, state);
        Err("Qu Studio's mascot needs a build with the `llm` Cargo feature enabled \
             (`cargo tauri dev --features llm`) -- this build doesn't have it."
            .to_string())
    }
}

/// Inline-completion `max_tokens` cap, independent of whatever the caller
/// asks for. A completion is a few tokens or one line, never a paragraph --
/// see the brief's explicit "keep max_tokens small for completions
/// specifically" scope. Also directly protects editor latency: on a
/// CPU-only greedy model every extra token is roughly another fixed
/// per-token cost (see this module's own doc comment for the measured
/// figure), and ghost text arriving well after the user kept typing is
/// worse than a shorter, faster suggestion.
#[cfg(feature = "llm")]
const COMPLETE_MAX_TOKENS_CAP: usize = 32;
#[cfg(feature = "llm")]
const COMPLETE_DEFAULT_MAX_TOKENS: usize = 16;

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
pub struct LlmCompleteRequest {
    pub prefix: String,
    pub max_tokens: Option<usize>,
}

/// `llm_complete(request: { prefix, max_tokens? })` -- Monaco's inline
/// ghost-text provider (see `CodeEditor.tsx`'s
/// `registerInlineCompletionsProvider`) calls this with the code before the
/// cursor as a raw completion prompt (no chat template -- this is plain
/// code continuation, not a conversation turn). `max_tokens` is clamped to
/// `COMPLETE_MAX_TOKENS_CAP` regardless of what the frontend passes, since
/// this is the one call site latency work built around most closely.
#[tauri::command]
pub fn llm_complete(request: LlmCompleteRequest, state: State<LlmState>) -> Result<String, String> {
    complete_with_state(&state, request)
}

/// The actual `llm_complete` logic -- see `chat_with_state`'s own doc
/// comment for why this is factored out from the `#[tauri::command]`
/// wrapper (testability without a real `tauri::State`).
fn complete_with_state(state: &LlmState, request: LlmCompleteRequest) -> Result<String, String> {
    #[cfg(feature = "llm")]
    {
        let model = get_or_load_model(state)?;
        let max_tokens = request
            .max_tokens
            .unwrap_or(COMPLETE_DEFAULT_MAX_TOKENS)
            .min(COMPLETE_MAX_TOKENS_CAP)
            .max(1);
        // Prefix-only completion (v1, per the brief) -- the tail of the
        // buffer before the cursor is the prompt, no fill-in-middle suffix
        // handling. Same tail-truncation as the chat prompt, for the same
        // reason: prompt-processing time scales with input token count on
        // this CPU-only model, and the END of the prefix (right before the
        // cursor) is what actually matters for a completion.
        let prefix = truncate_chars(&request.prefix, CONTEXT_CHAR_BUDGET);
        let completion = model.generate(&prefix, max_tokens)?;
        Ok(completion)
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (request, state);
        Err("Qu Studio's inline autocomplete needs a build with the `llm` Cargo feature \
             enabled (`cargo tauri dev --features llm`) -- this build doesn't have it."
            .to_string())
    }
}

/// "Fix this error" gets more headroom than a chat reply -- the output IS
/// the replacement script, not prose, so it needs to be long enough to
/// cover a whole small Qu script (the catalog examples this primer itself
/// quotes run 20-50 lines). Still bounded, same CPU-only-latency reasoning
/// as every other budget in this module.
#[cfg(feature = "llm")]
const FIX_MAX_TOKENS: usize = 256;
#[cfg(feature = "llm")]
const TRANSFORM_MAX_TOKENS: usize = 256;
/// Error messages are short; take the TAIL when truncating (same rationale
/// as `CONTEXT_CHAR_BUDGET`'s own doc comment -- Qu's own error format puts
/// the actually-useful line near the end), just with a smaller budget since
/// there's usually nowhere near this much error text to begin with.
#[cfg(feature = "llm")]
const FIX_ERROR_CHAR_BUDGET: usize = 600;

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
pub struct LlmFixErrorRequest {
    /// The full current editor buffer -- the script that produced `error`.
    pub code: String,
    /// The exact error text from that run (`ExecuteResponse.error` /
    /// `executionState.error` in `App.tsx`, i.e. the CURRENT run's error,
    /// not a stale one from an earlier edit -- see `App.tsx`'s own
    /// `executeCode`, which clears `executionState.error` back to `null`
    /// the instant a new run starts).
    pub error: String,
}

/// Builds the fix-error prompt: primer + "here's the script and the exact
/// error, reply with ONLY the corrected script." Told explicitly not to
/// wrap the reply in markdown fences or add commentary, since this output
/// goes straight into a diff-preview the user applies to their buffer --
/// chatty wrapping around the code would otherwise need to be stripped
/// before it's usable as a replacement script (see `strip_code_fence`,
/// kept as a defensive second layer since a small chat-tuned model doesn't
/// reliably follow "no markdown" instructions 100% of the time).
#[cfg(feature = "llm")]
fn build_fix_prompt(code: &str, error: &str) -> String {
    let system = format!(
        "You are a Qu code-fixing assistant inside Qu Studio, an IDE for the Qu scientific \
         scripting language.\n\n{QU_SYNTAX_PRIMER}\n\
         The user's script below failed with the error shown. Reply with ONLY the corrected, \
         complete Qu script that fixes it -- no explanation, no restating the error, no \
         markdown code fences, no commentary before or after the code."
    );
    let user = format!(
        "Script:\n{}\n\nError:\n{}",
        truncate_chars(code.trim(), CONTEXT_CHAR_BUDGET),
        truncate_chars(error.trim(), FIX_ERROR_CHAR_BUDGET)
    );
    format!("<|system|>\n{system}</s>\n<|user|>\n{user}</s>\n<|assistant|>\n")
}

/// `llm_fix_error(request: { code, error })` -- the "Fix with AI" button's
/// command, triggered from the inline error banner `App.tsx` shows next to
/// a failed run. Returns the model's best guess at a corrected FULL script
/// (not a diff/patch -- the frontend diffs it against the current buffer
/// itself for the review-before-apply UI, see `AiDiffModal`). Never applied
/// automatically: this is exactly the "destructive if wrong" action the
/// brief calls out, so the caller is expected to show it for review first.
#[tauri::command]
pub fn llm_fix_error(request: LlmFixErrorRequest, state: State<LlmState>) -> Result<String, String> {
    fix_error_with_state(&state, request)
}

fn fix_error_with_state(state: &LlmState, request: LlmFixErrorRequest) -> Result<String, String> {
    #[cfg(feature = "llm")]
    {
        let model = get_or_load_model(state)?;
        let prompt = build_fix_prompt(&request.code, &request.error);
        let reply = model.generate(&prompt, FIX_MAX_TOKENS)?;
        Ok(strip_code_fence(&reply))
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (request, state);
        Err("Qu Studio's AI fix needs a build with the `llm` Cargo feature enabled \
             (`cargo tauri dev --features llm`) -- this build doesn't have it."
            .to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
pub struct LlmTransformRequest {
    /// What the user wants done, in plain English -- e.g. "vectorize this
    /// loop" or "add error handling" (transform mode), or "plot a damped
    /// sine wave" (generate mode, see `selection` below).
    pub instruction: String,
    /// The current whole-buffer text, for context (existing variable names,
    /// style) -- optional since a brand-new/empty buffer has none.
    pub code: Option<String>,
    /// The user's current Monaco selection, exactly as read from the editor
    /// (see `CodeEditor.tsx`'s `onSelectionChange`) -- `None`/empty means
    /// "generate new code from scratch" (Task 3a) rather than "rewrite this
    /// selection" (Task 3b).
    pub selection: Option<String>,
}

/// Builds the transform-or-generate prompt. Two modes, same shape as
/// `LlmTransformRequest.selection`'s own doc comment: with a selection,
/// asks for a rewrite of just that snippet; without one, asks for brand-new
/// code from the instruction alone. Both modes end with the same "ONLY the
/// code" instruction as `build_fix_prompt`, for the same reason (this
/// output is meant to go straight into a diff-preview/insert, not be read
/// as prose first).
#[cfg(feature = "llm")]
fn build_transform_prompt(instruction: &str, code_context: Option<&str>, selection: Option<&str>) -> String {
    let has_selection = selection.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let system = if has_selection {
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
    };

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

    format!("<|system|>\n{system}</s>\n<|user|>\n{user}</s>\n<|assistant|>\n")
}

/// `llm_transform_code(request: { instruction, code, selection })` -- the
/// "Generate/Transform" button's one command, covering both Task 3 modes
/// (selection present -> rewrite it; absent -> generate new code). Same
/// review-before-apply contract as `llm_fix_error`: this returns a
/// SUGGESTION string, never touches the caller's actual buffer itself.
#[tauri::command]
pub fn llm_transform_code(request: LlmTransformRequest, state: State<LlmState>) -> Result<String, String> {
    transform_with_state(&state, request)
}

fn transform_with_state(state: &LlmState, request: LlmTransformRequest) -> Result<String, String> {
    #[cfg(feature = "llm")]
    {
        let model = get_or_load_model(state)?;
        let prompt = build_transform_prompt(
            &request.instruction,
            request.code.as_deref(),
            request.selection.as_deref(),
        );
        let reply = model.generate(&prompt, TRANSFORM_MAX_TOKENS)?;
        Ok(strip_code_fence(&reply))
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (request, state);
        Err("Qu Studio's AI generate/transform needs a build with the `llm` Cargo feature \
             enabled (`cargo tauri dev --features llm`) -- this build doesn't have it."
            .to_string())
    }
}

/// Extracts just the code from a model reply, defensively, on top of the
/// "no markdown fences, no commentary" prompt instruction. NOT a "strip
/// fences off the whole-reply wrapper" guess -- that was tried first and
/// turned out wrong against a REAL reply (see IMPL.md's dated entry):
/// `real_fix_error_suggests_a_correction` came back as
/// `"Here's the corrected Qu script that fixes the error:\n\n\`\`\`\nx = \
/// [1, 2, 3]\nprint(length(x))\n\`\`\`\n\nThis script defines..."` -- prose
/// BEFORE the fence and prose AFTER it, not just a bare fenced-whole-reply.
/// So this scans for the first fence-opening line (\`\`\` or \`\`\`qu) and
/// the NEXT fence-closing line after it, and keeps only what's strictly
/// between them, discarding surrounding commentary either side. Falls back
/// to the trimmed whole reply when there's no fence pair at all (the model
/// DID follow the "no fences" instruction that time).
#[cfg(feature = "llm")]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises ONLY the `llm`-off error path -- no model, no network,
    /// stays in the default `cargo test` run. Mirrors `qu-llm`'s own
    /// "cheap, hermetic tests in the default suite; real generation is
    /// `#[ignore]`d" split.
    #[cfg(not(feature = "llm"))]
    #[test]
    fn chat_without_llm_feature_returns_a_clear_error() {
        let state = LlmState::default();
        let err = chat_with_state(&state, LlmChatRequest { prompt: "hi".into(), context: None }).unwrap_err();
        assert!(err.contains("llm"), "got: {err}");
    }

    /// Same `llm`-off error-path coverage as `chat_without_llm_feature_returns_a_clear_error`,
    /// for the two newer commands.
    #[cfg(not(feature = "llm"))]
    #[test]
    fn fix_error_without_llm_feature_returns_a_clear_error() {
        let state = LlmState::default();
        let err = fix_error_with_state(
            &state,
            LlmFixErrorRequest { code: "x = 1".into(), error: "boom".into() },
        )
        .unwrap_err();
        assert!(err.contains("llm"), "got: {err}");
    }

    /// Locks in `strip_code_fence`'s handling of the EXACT reply shape a
    /// real `real_fix_error_suggests_a_correction` run actually produced
    /// (see that function's own doc comment) -- prose before the fence,
    /// prose after it, no full-reply wrapping. A hermetic, no-model test:
    /// this is pure string logic, doesn't need `--ignored`.
    #[cfg(feature = "llm")]
    #[test]
    fn strip_code_fence_extracts_code_between_prose() {
        let reply = "Here's the corrected Qu script that fixes the error:\n\n\
                     ```\nx = [1, 2, 3]\nprint(length(x))\n```\n\n\
                     This script defines the `length` function and calls it.";
        assert_eq!(strip_code_fence(reply), "x = [1, 2, 3]\nprint(length(x))");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn strip_code_fence_passes_through_a_fence_free_reply() {
        assert_eq!(strip_code_fence("x = [1, 2, 3]\nprint(length(x))"), "x = [1, 2, 3]\nprint(length(x))");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn strip_code_fence_handles_a_language_tagged_fence() {
        let reply = "```qu\ny = sum(x)\n```";
        assert_eq!(strip_code_fence(reply), "y = sum(x)");
    }

    #[cfg(not(feature = "llm"))]
    #[test]
    fn transform_without_llm_feature_returns_a_clear_error() {
        let state = LlmState::default();
        let err = transform_with_state(
            &state,
            LlmTransformRequest { instruction: "do it".into(), code: None, selection: None },
        )
        .unwrap_err();
        assert!(err.contains("llm"), "got: {err}");
    }

    /// Real, end-to-end, non-mocked exercise of the EXACT logic
    /// `llm_chat`'s `#[tauri::command]` wrapper calls -- loads the real
    /// TinyLlama model (from the local Hugging Face cache; downloads it on
    /// a machine that doesn't have it yet) and runs real CPU inference.
    /// `#[ignore]`d for the same reason `qu-llm`/`qu-interp`'s own real-
    /// generation tests are: not appropriate for a default `cargo test` run
    /// on a machine/CI sandbox with no model cached. Run by hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture real_chat_produces_a_coherent_reply
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn real_chat_produces_a_coherent_reply() {
        let state = LlmState::default();
        let reply = chat_with_state(
            &state,
            LlmChatRequest {
                prompt: "In one short sentence, what is a Fourier transform?".to_string(),
                context: None,
            },
        )
        .expect("chat_with_state should succeed with a real model");
        println!("real mascot chat reply: {reply:?}");
        assert!(!reply.trim().is_empty(), "expected a non-empty reply, got: {reply:?}");
    }

    /// Same real, non-mocked exercise for `llm_complete`'s underlying
    /// logic. Run by hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture real_complete_continues_qu_code
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn real_complete_continues_qu_code() {
        let state = LlmState::default();
        let completion = complete_with_state(
            &state,
            LlmCompleteRequest {
                prefix: "x = [1, 2, 3, 4, 5]\ny = sum(x)\nprint(".to_string(),
                max_tokens: Some(12),
            },
        )
        .expect("complete_with_state should succeed with a real model");
        println!("real inline completion: {completion:?}");
        assert!(!completion.trim().is_empty(), "expected a non-empty completion, got: {completion:?}");
    }

    /// Real, non-mocked exercise of `llm_fix_error`'s underlying logic on a
    /// script with a deliberate, obvious bug (wrong builtin name). Run by
    /// hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture real_fix_error_suggests_a_correction
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn real_fix_error_suggests_a_correction() {
        let state = LlmState::default();
        let fixed = fix_error_with_state(
            &state,
            LlmFixErrorRequest {
                code: "x = [1, 2, 3]\nprint(lenght(x))".to_string(),
                error: "unknown function `lenght` (did you mean `length`?)".to_string(),
            },
        )
        .expect("fix_error_with_state should succeed with a real model");
        println!("real fix-error suggestion: {fixed:?}");
        assert!(!fixed.trim().is_empty(), "expected a non-empty suggestion, got: {fixed:?}");
    }

    /// Real, non-mocked exercise of `llm_transform_code`'s underlying logic
    /// in "transform a selection" mode. Run by hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture real_transform_rewrites_a_selection
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn real_transform_rewrites_a_selection() {
        let state = LlmState::default();
        let result = transform_with_state(
            &state,
            LlmTransformRequest {
                instruction: "add a comment above this line explaining what it does".to_string(),
                code: Some("x = [1, 2, 3]\ny = sum(x)".to_string()),
                selection: Some("y = sum(x)".to_string()),
            },
        )
        .expect("transform_with_state should succeed with a real model");
        println!("real transform suggestion: {result:?}");
        assert!(!result.trim().is_empty(), "expected a non-empty suggestion, got: {result:?}");
    }

    /// Real, non-mocked exercise of `llm_transform_code`'s underlying logic
    /// in "generate from scratch" mode (no selection). Run by hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture real_transform_generates_new_code
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn real_transform_generates_new_code() {
        let state = LlmState::default();
        let result = transform_with_state(
            &state,
            LlmTransformRequest {
                instruction: "create a vector of 10 zeros".to_string(),
                code: None,
                selection: None,
            },
        )
        .expect("transform_with_state should succeed with a real model");
        println!("real generate suggestion: {result:?}");
        assert!(!result.trim().is_empty(), "expected a non-empty suggestion, got: {result:?}");
    }

    /// Verifies the chat prompt's conciseness instruction actually changes
    /// real model output, rather than assuming a sentence added to the
    /// prompt worked. Generates the SAME question through the OLD prompt
    /// shape (primer, no "be concise" instruction) and the CURRENT
    /// `build_chat_prompt` (primer + conciseness instruction) with the same
    /// `max_tokens` budget, and prints both so a human/reviewing agent can
    /// compare them directly. Run by hand with:
    ///   cargo test -p qu-studio --features llm -- --ignored --nocapture conciseness_instruction_changes_real_output
    #[cfg(feature = "llm")]
    #[test]
    #[ignore = "downloads/loads a real ~669MB model and runs real CPU inference; run manually"]
    fn conciseness_instruction_changes_real_output() {
        let model = qu_llm::load("").expect("model should load");
        let question = "What does the |> operator do in Qu?";

        let verbose_system = format!(
            "You are Qu-bot, the friendly built-in mascot assistant for Qu Studio, an IDE for \
             the Qu scientific scripting language (a MATLAB/Julia-like language for signal \
             processing, linear algebra, and machine learning).\n\n{QU_SYNTAX_PRIMER}\n\
             Answer the user's question directly and concisely.",
        );
        let verbose_prompt =
            format!("<|system|>\n{verbose_system}</s>\n<|user|>\n{question}</s>\n<|assistant|>\n");
        let verbose_reply =
            model.generate(&verbose_prompt, CHAT_MAX_TOKENS).expect("verbose generate should succeed");

        let concise_prompt = build_chat_prompt(question, None);
        let concise_reply =
            model.generate(&concise_prompt, CHAT_MAX_TOKENS).expect("concise generate should succeed");

        println!("=== WITHOUT explicit conciseness instruction ===\n{verbose_reply}");
        println!("=== WITH explicit conciseness instruction ===\n{concise_reply}");
        println!(
            "lengths: without={} chars, with={} chars",
            verbose_reply.trim().chars().count(),
            concise_reply.trim().chars().count()
        );
    }
}
