//! Hosted-API "AI Assist" backends (OpenAI, Anthropic) plus the local-vs-
//! hosted provider abstraction `llm_bridge.rs`'s four Tauri commands
//! dispatch through. Everything in `llm_bridge.rs` up to this point talked
//! to exactly one backend -- the local, CPU-only `qu_llm::LlmModel` -- see
//! that module's own top-level doc comment for the full picture. This
//! module adds two more backends behind the SAME shape (the `LlmProvider`
//! trait below) so `llm_bridge.rs`'s commands never need an `if provider ==
//! ...` ladder: they build one `GenRequest`, hand it to whichever
//! `LlmProvider` the caller picked (or the fallback chain -- see
//! `llm_bridge.rs`'s `answer_with_fallback`), and get back plain text plus
//! the name of whichever backend actually answered.
//!
//! **Why this module (not `qu-llm`) owns the hosted backends.** `qu-llm` is
//! specifically the local-candle-inference crate -- see its own Cargo.toml
//! doc comment ("Local, in-process LLM inference"). An HTTP call to
//! api.openai.com has nothing to do with candle/GGUF/tokenizers, and adding
//! it there would make a crate whose whole reason to exist is "run a model
//! on this machine" also respons­ible for talking to the network. This
//! module lives in `qu-studio-tauri` instead, alongside the other
//! Tauri-only glue (`llm_bridge.rs`, `serial_protocol.rs`, ...).
//!
//! **HTTP client: `ureq`, not a new dependency.** `qu-llm` already pulls in
//! `ureq` (sync, no async runtime, see its own Cargo.toml comment on why --
//! it's the same client `hf-hub`'s default `online` feature uses). Adding
//! `reqwest` here would mean two HTTP stacks in the same process for no
//! benefit: these are simple, one-shot POST-and-read-the-body calls, no
//! streaming, no connection pooling story either client would meaningfully
//! outperform the other on. `ureq` also has no async runtime to drag in,
//! which matters here specifically because `qu-studio-tauri` otherwise has
//! none either (`tauri::command` functions here are plain sync fns).
//! `ureq` is added as a normal (non-optional) dependency of this crate --
//! unlike `qu-llm`, gated behind the `llm` Cargo feature -- because hosted
//! providers should work in a build that never turns `llm` on at all: they
//! need no candle/GGUF weight, so there is no reason to force a user who
//! only wants OpenAI/Anthropic support to pay for the 669MB local model's
//! dependency tree.
//!
//! **Why not the `keyring` crate for API-key storage.** Investigated first,
//! per the brief's own preference. `cargo add keyring@2 --dry-run` (see
//! this task's own board2.txt entry for the exact numbers) pulled in
//! ~150 new transitive crates even on a Windows-only build target --
//! `zbus`, `secret-service`, `openssl-src`, `dbus`-adjacent async-io/polling
//! machinery -- all of it Linux Secret-Service plumbing that would never
//! even compile on this platform, dragged in purely because Cargo.lock
//! resolves the full cross-platform graph regardless of which target is
//! actually being built. That is exactly the "too heavy" case the brief's
//! own fallback clause anticipates, so this module uses that fallback
//! instead: a local JSON file under Tauri's `app_config_dir()` (see
//! `settings_path`), OUTSIDE the git-tracked repo tree entirely (that
//! directory is something like `%APPDATA%\com.qu.studio\` on Windows,
//! `~/Library/Application Support/com.qu.studio/` on macOS, or
//! `~/.config/com.qu.studio/` on Linux -- never anywhere under this
//! checkout). Best-effort `0600` permissions on Unix (see `save_settings`);
//! Windows relies on the user's own profile-directory ACLs, same trust
//! boundary every other per-user credential file on that platform already
//! sits inside. Documented again, with the actual dependency-count numbers,
//! in board2.txt.
//!
//! **The one hard rule in this module: an API key never appears in a
//! `Result::Err` string.** Every error path below is built from a fixed,
//! hand-written message plus (at most) the HTTP status code and the
//! provider's own JSON error message -- never the request we sent, never
//! the `Authorization`/`x-api-key` header, never the raw `ureq::Error`
//! (which can echo request metadata). See `http_post_json`'s doc comment.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------
// Provider selection + the abstraction itself
// ---------------------------------------------------------------------

/// Which backend answers a given `GenRequest`. `Local` is always available
/// (in an `llm`-enabled build); `Openai`/`Anthropic` need a saved API key.
/// `Default` is `Local` -- a fresh install with no settings file yet must
/// keep working exactly like QuStudio did before this module existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Local,
    Openai,
    Anthropic,
}

impl Default for ProviderKind {
    fn default() -> Self {
        ProviderKind::Local
    }
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Local => "Local (TinyLlama)",
            ProviderKind::Openai => "OpenAI",
            ProviderKind::Anthropic => "Anthropic",
        }
    }
}

/// One generation request, backend-agnostic. Built by `llm_bridge.rs`'s
/// four command handlers from exactly the same prompt pieces they already
/// assembled for the local model (see e.g. `build_chat_prompt`) -- the
/// Qu-syntax priming discipline documented there is NOT duplicated here;
/// `llm_bridge.rs` folds `QU_SYNTAX_PRIMER` into `system` before this
/// struct is ever built, so every backend (local or hosted) sees it.
pub struct GenRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: usize,
    /// Set ONLY by `llm_complete` (inline autocomplete). When present, the
    /// LOCAL backend uses this raw string as the entire prompt, bypassing
    /// the system/user chat template entirely -- this preserves
    /// `llm_complete`'s pre-existing behavior (a bare prefix continuation,
    /// not a chat turn) unchanged, per the brief's "don't touch the local
    /// TinyLlama path's own logic beyond what's needed" instruction. Hosted
    /// providers have no raw-continuation endpoint to speak of, so they
    /// always use `system`/`user` regardless of this field -- see
    /// `OpenAiProvider::generate`/`AnthropicProvider::generate`. Only ever
    /// READ by `LocalProvider`, which is itself `#[cfg(feature = "llm")]`
    /// -- so in a build without that feature nothing reads this field at
    /// all; that's expected (there's no local backend to read it FOR in
    /// that build), not a real bug to warn about.
    #[cfg_attr(not(feature = "llm"), allow(dead_code))]
    pub local_raw_prompt: Option<String>,
}

/// The abstraction every backend implements, so `llm_bridge.rs` never
/// branches on "which provider" itself -- it just calls `.generate()` on
/// whichever `&dyn LlmProvider` the fallback chain picked.
/// The one method every backend implements. `llm_bridge.rs` gets the
/// user-facing backend name (surfaced as `LlmAnswer::backend`, "answered
/// by ...") from `ProviderKind::label()` instead of a method on this trait
/// -- it already knows which `ProviderKind` it picked before constructing
/// whichever provider struct, so a second name source on the trait itself
/// would just be a redundant thing to keep in sync.
pub trait LlmProvider {
    fn generate(&self, req: &GenRequest) -> Result<String, String>;
}

// ---------------------------------------------------------------------
// Settings + key storage (outside the repo, see this module's doc comment)
// ---------------------------------------------------------------------

/// Lives in Tauri's `app_config_dir()`, never inside this checkout. See
/// this module's top-level doc comment for the concrete per-OS paths and
/// why a plain JSON file was chosen over the `keyring` crate.
pub const SETTINGS_FILE_NAME: &str = "llm_provider_settings.json";

fn default_openai_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_anthropic_model() -> String {
    "claude-haiku-4-5".to_string()
}

/// The full on-disk settings shape, API keys included. Never sent to the
/// frontend as-is -- see `PublicProviderSettings` for the redacted DTO
/// `get_llm_provider_config` actually returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    #[serde(default)]
    pub provider: ProviderKind,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    #[serde(default = "default_anthropic_model")]
    pub anthropic_model: String,
    /// `None` until the user saves one via the settings panel. Read ONLY by
    /// `llm_bridge.rs`'s dispatch code and this module's own HTTP calls --
    /// never logged (see `env_logger`'s call sites elsewhere in this crate;
    /// none of them touch this struct), never included in any `Err` string.
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        ProviderSettings {
            provider: ProviderKind::default(),
            openai_model: default_openai_model(),
            anthropic_model: default_anthropic_model(),
            openai_api_key: None,
            anthropic_api_key: None,
        }
    }
}

/// The DTO actually sent to the frontend -- booleans instead of the real
/// keys, so a saved key can never round-trip back out over the Tauri IPC
/// bridge (which, on some platforms/build configs, can end up in debug
/// logs) even by accident.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProviderSettings {
    pub provider: ProviderKind,
    pub openai_model: String,
    pub anthropic_model: String,
    pub has_openai_key: bool,
    pub has_anthropic_key: bool,
}

impl From<&ProviderSettings> for PublicProviderSettings {
    fn from(s: &ProviderSettings) -> Self {
        PublicProviderSettings {
            provider: s.provider,
            openai_model: s.openai_model.clone(),
            anthropic_model: s.anthropic_model.clone(),
            has_openai_key: s.openai_api_key.as_deref().is_some_and(|k| !k.is_empty()),
            has_anthropic_key: s.anthropic_api_key.as_deref().is_some_and(|k| !k.is_empty()),
        }
    }
}

/// What the frontend sends `set_llm_provider_config`. `openai_api_key`/
/// `anthropic_api_key`: `None` means "leave whatever's already saved
/// alone", `Some("")` means "clear it" -- lets the settings panel save a
/// provider/model change without forcing the user to re-paste a key that's
/// already stored, and lets a "Remove key" button work with the same
/// command.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSettingsUpdate {
    pub provider: ProviderKind,
    #[serde(default)]
    pub openai_model: Option<String>,
    #[serde(default)]
    pub anthropic_model: Option<String>,
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
}

fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE_NAME)
}

/// Reads the settings file, falling back to `ProviderSettings::default()`
/// (local-only, no keys) when it doesn't exist yet or fails to parse --
/// same "never break a working install" reasoning as `LlmState::default()`
/// in `llm_bridge.rs`: a corrupt/missing settings file must degrade to
/// "behave like before this feature existed," not to a hard error.
pub fn load_settings(config_dir: &Path) -> ProviderSettings {
    std::fs::read_to_string(settings_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Writes the settings file, creating `config_dir` first if needed. Sets
/// `0600` permissions on Unix (best-effort -- a failure here doesn't fail
/// the save, since the file still isn't inside the repo and still sits
/// under the user's own profile directory either way).
pub fn save_settings(config_dir: &Path, settings: &ProviderSettings) -> Result<(), String> {
    std::fs::create_dir_all(config_dir)
        .map_err(|e| format!("could not create settings directory: {e}"))?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("could not serialize provider settings: {e}"))?;
    let path = settings_path(config_dir);
    std::fs::write(&path, json).map_err(|e| format!("could not write settings file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Applies a `ProviderSettingsUpdate` onto whatever's already saved (see
/// `ProviderSettingsUpdate`'s own doc comment for the `None`-keeps/
/// `Some("")`-clears key semantics), then persists it.
pub fn apply_update(config_dir: &Path, update: ProviderSettingsUpdate) -> Result<ProviderSettings, String> {
    let mut settings = load_settings(config_dir);
    settings.provider = update.provider;
    if let Some(m) = update.openai_model {
        if !m.trim().is_empty() {
            settings.openai_model = m;
        }
    }
    if let Some(m) = update.anthropic_model {
        if !m.trim().is_empty() {
            settings.anthropic_model = m;
        }
    }
    if let Some(k) = update.openai_api_key {
        settings.openai_api_key = if k.is_empty() { None } else { Some(k) };
    }
    if let Some(k) = update.anthropic_api_key {
        settings.anthropic_api_key = if k.is_empty() { None } else { Some(k) };
    }
    save_settings(config_dir, &settings)?;
    Ok(settings)
}

// ---------------------------------------------------------------------
// HTTP plumbing shared by both hosted providers
// ---------------------------------------------------------------------

/// POSTs `body` as JSON to `url` with `headers`, returns the response body
/// as a string. The one and only place either hosted provider touches the
/// network, so it is also the one place that has to be careful never to
/// let an API key leak into the `Err` it returns: `ureq::Error`'s own
/// `Display` impl can include the request URL and, for a `Status` error,
/// the response body -- both are safe to surface (this app builds the URL
/// itself with no key in it, and a provider's own error response is never
/// going to contain the key WE sent, since providers don't echo the
/// `Authorization`/`x-api-key` header back) -- but headers themselves are
/// never read out of the error, so the key genuinely cannot reach this
/// function's return value even if a future `ureq` version started
/// including them.
fn http_post_json(
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
    provider_label: &str,
) -> Result<String, String> {
    let mut req = ureq::post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    match req.send_json(body.clone()) {
        Ok(resp) => resp
            .into_string()
            .map_err(|_| format!("{provider_label}: could not read response body")),
        Err(ureq::Error::Status(code, resp)) => {
            // Provider's own error message (e.g. `{"error":{"message":"Incorrect API key..."}}`)
            // -- useful for the user, never contains the key we sent.
            let text = resp.into_string().unwrap_or_default();
            let snippet: String = text.chars().take(300).collect();
            Err(format!("{provider_label}: HTTP {code}: {snippet}"))
        }
        Err(ureq::Error::Transport(_)) => {
            // Deliberately NOT formatting the `Transport` value itself --
            // it can include the target host/URL, which is fine, but this
            // keeps the contract simple: transport failures always get the
            // same generic, key-free message regardless of what ureq's
            // internals happen to include in a given version.
            Err(format!(
                "{provider_label}: network request failed (no internet connection, DNS failure, \
                 or the provider is unreachable)"
            ))
        }
    }
}

// ---------------------------------------------------------------------
// Local (candle/TinyLlama) provider -- thin wrapper, no new logic
// ---------------------------------------------------------------------

/// Adapts the already-loaded local model to `LlmProvider`. Holds only a
/// borrowed reference -- `llm_bridge.rs` still owns the `Arc<qu_llm::LlmModel>`
/// lifecycle (lazy load, caching in `LlmState`) exactly as before; this
/// struct exists purely so the SAME dispatch code path in `llm_bridge.rs`
/// can call `.generate()` on it like any other provider.
#[cfg(feature = "llm")]
pub struct LocalProvider<'a> {
    pub model: &'a qu_llm::LlmModel,
}

#[cfg(feature = "llm")]
impl LlmProvider for LocalProvider<'_> {
    fn generate(&self, req: &GenRequest) -> Result<String, String> {
        // Raw-prefix mode (`llm_complete`'s pre-existing behavior) bypasses
        // the chat template entirely -- see `GenRequest::local_raw_prompt`'s
        // own doc comment.
        if let Some(raw) = &req.local_raw_prompt {
            return self.model.generate(raw, req.max_tokens);
        }
        let prompt = format!(
            "<|system|>\n{}</s>\n<|user|>\n{}</s>\n<|assistant|>\n",
            req.system, req.user
        );
        self.model.generate(&prompt, req.max_tokens)
    }
}

// ---------------------------------------------------------------------
// OpenAI (Chat Completions API)
// ---------------------------------------------------------------------

/// `POST https://api.openai.com/v1/chat/completions` -- chosen over the
/// newer Responses API because Chat Completions is the one shape that
/// covers BOTH this app's use cases with no special-casing: multi-turn-
/// capable (mascot chat, even though today's `GenRequest` only ever sends
/// one system + one user turn -- extending to real history later is just
/// appending more entries to `messages`) AND perfectly fine for a single-
/// shot code transform (system + one user turn, same as every other
/// `llm_bridge.rs` command already does today). The Responses API's extra
/// surface (built-in tool use, server-side conversation state) has no use
/// here and would be strictly more integration work for zero benefit on
/// this app's actual request shape. Request/response field names verified
/// against OpenAI's own API reference (chat/create) during this task, not
/// recalled from training data -- see board2.txt's entry for this task.
pub const OPENAI_CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiRequestBody<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    max_tokens: usize,
}

/// Pure request-building logic, factored out from `OpenAiProvider::generate`
/// so it's unit-testable without a live network call (see the brief's own
/// "unit-testable against a mocked/fixture response body" instruction).
fn openai_request_body<'a>(model: &'a str, system: &'a str, user: &'a str, max_tokens: usize) -> serde_json::Value {
    let body = OpenAiRequestBody {
        model,
        messages: vec![
            OpenAiMessage { role: "system", content: system },
            OpenAiMessage { role: "user", content: user },
        ],
        max_tokens,
    };
    serde_json::to_value(body).expect("OpenAiRequestBody always serializes")
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}
#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

/// Pure response-parsing logic, same testability rationale as
/// `openai_request_body`. Deliberately tolerant of extra/unknown fields
/// (only the three we actually need are declared above) -- OpenAI's real
/// response includes `id`/`object`/`created`/`model`/`usage`/etc. that this
/// app has no use for.
fn openai_parse_response(body: &str) -> Result<String, String> {
    // A non-2xx response never reaches here (see `http_post_json`'s
    // `Status` branch, which returns `Err` before this function is ever
    // called) -- this only has to handle "200 OK but the JSON shape
    // surprised us" (e.g. a future API change, or `choices` empty).
    let parsed: OpenAiResponse = serde_json::from_str(body)
        .map_err(|e| format!("OpenAI: could not parse response: {e}"))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "OpenAI: response had no choices".to_string())
}

pub struct OpenAiProvider {
    pub api_key: String,
    pub model: String,
}

impl LlmProvider for OpenAiProvider {
    fn generate(&self, req: &GenRequest) -> Result<String, String> {
        let body = openai_request_body(&self.model, &req.system, &req.user, req.max_tokens);
        let auth = format!("Bearer {}", self.api_key);
        let text = http_post_json(
            OPENAI_CHAT_URL,
            &[("Authorization", &auth), ("Content-Type", "application/json")],
            &body,
            "OpenAI",
        )?;
        openai_parse_response(&text)
    }
}

/// `POST https://api.openai.com/v1/chat/completions` with `model`/`api_key`
/// -- a minimal, cheap call for the settings panel's "Test connection"
/// button (Tauri command `test_llm_provider`). One token of output is
/// enough to confirm the key + model are both valid without burning a
/// meaningful amount of the user's quota.
pub fn openai_test_connection(api_key: &str, model: &str) -> Result<(), String> {
    let req = GenRequest {
        system: "Reply with only the word OK.".to_string(),
        user: "Say OK.".to_string(),
        max_tokens: 5,
        local_raw_prompt: None,
    };
    let provider = OpenAiProvider { api_key: api_key.to_string(), model: model.to_string() };
    provider.generate(&req).map(|_| ())
}

// ---------------------------------------------------------------------
// Anthropic (Messages API)
// ---------------------------------------------------------------------

/// `POST https://api.anthropic.com/v1/messages`. Headers/fields verified
/// against Anthropic's own API reference (`/en/api/messages`) during this
/// task -- notably: `system` is a top-level request field, NOT a message
/// with `role: "system"` (unlike OpenAI's shape above), and the
/// `anthropic-version` header is required on every call.
pub const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct AnthropicRequestBody<'a> {
    model: &'a str,
    max_tokens: usize,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

fn anthropic_request_body<'a>(model: &'a str, system: &'a str, user: &'a str, max_tokens: usize) -> serde_json::Value {
    let body = AnthropicRequestBody {
        model,
        max_tokens,
        system,
        messages: vec![AnthropicMessage { role: "user", content: user }],
    };
    serde_json::to_value(body).expect("AnthropicRequestBody always serializes")
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}
#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// Same testability rationale as `openai_parse_response`. Anthropic's
/// `content` is an array of typed blocks (`{"type": "text", "text": "..."}`
/// today, but the API reserves room for other block types in principle) --
/// this concatenates every `"text"`-typed block's text, which is exactly
/// what today's plain single-turn requests (no tool use) always produce as
/// a single one-element array in practice.
fn anthropic_parse_response(body: &str) -> Result<String, String> {
    let parsed: AnthropicResponse = serde_json::from_str(body)
        .map_err(|e| format!("Anthropic: could not parse response: {e}"))?;
    let text: String = parsed
        .content
        .into_iter()
        .filter(|b| b.kind == "text")
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        Err("Anthropic: response had no text content".to_string())
    } else {
        Ok(text)
    }
}

pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
}

impl LlmProvider for AnthropicProvider {
    fn generate(&self, req: &GenRequest) -> Result<String, String> {
        let body = anthropic_request_body(&self.model, &req.system, &req.user, req.max_tokens);
        let text = http_post_json(
            ANTHROPIC_MESSAGES_URL,
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", ANTHROPIC_VERSION),
                ("Content-Type", "application/json"),
            ],
            &body,
            "Anthropic",
        )?;
        anthropic_parse_response(&text)
    }
}

/// Same "Test connection" role as `openai_test_connection`.
pub fn anthropic_test_connection(api_key: &str, model: &str) -> Result<(), String> {
    let req = GenRequest {
        system: "Reply with only the word OK.".to_string(),
        user: "Say OK.".to_string(),
        max_tokens: 5,
        local_raw_prompt: None,
    };
    let provider = AnthropicProvider { api_key: api_key.to_string(), model: model.to_string() };
    provider.generate(&req).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- request building -------------------------------------------------

    #[test]
    fn openai_request_body_has_expected_shape() {
        let v = openai_request_body("gpt-4o-mini", "sys prompt", "user msg", 128);
        assert_eq!(v["model"], "gpt-4o-mini");
        assert_eq!(v["max_tokens"], 128);
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "sys prompt");
        assert_eq!(v["messages"][1]["role"], "user");
        assert_eq!(v["messages"][1]["content"], "user msg");
    }

    #[test]
    fn anthropic_request_body_has_expected_shape() {
        let v = anthropic_request_body("claude-haiku-4-5", "sys prompt", "user msg", 128);
        assert_eq!(v["model"], "claude-haiku-4-5");
        assert_eq!(v["max_tokens"], 128);
        // `system` is top-level, NOT inside `messages` -- see this module's
        // doc comment on `ANTHROPIC_MESSAGES_URL`.
        assert_eq!(v["system"], "sys prompt");
        assert_eq!(v["messages"].as_array().unwrap().len(), 1);
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"], "user msg");
        assert!(v.get("role").is_none() || v["messages"][0]["role"] != "system");
    }

    // -- response parsing, against fixture bodies shaped like each
    // provider's own real documented example response (captured from their
    // public API reference during this task -- see this module's own doc
    // comments on `openai_parse_response`/`anthropic_parse_response`) ------

    #[test]
    fn openai_parse_response_extracts_message_content() {
        let fixture = r#"{
            "id": "chatcmpl-B9MBs8CjcvOU2jLn4n570S5qMJKcT",
            "object": "chat.completion",
            "created": 1741570283,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello! How can I assist?"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 19, "completion_tokens": 10, "total_tokens": 29}
        }"#;
        assert_eq!(openai_parse_response(fixture).unwrap(), "Hello! How can I assist?");
    }

    #[test]
    fn openai_parse_response_rejects_empty_choices() {
        let fixture = r#"{"id": "x", "object": "chat.completion", "choices": []}"#;
        let err = openai_parse_response(fixture).unwrap_err();
        assert!(err.contains("no choices"), "got: {err}");
    }

    #[test]
    fn openai_parse_response_rejects_garbage() {
        let err = openai_parse_response("not json").unwrap_err();
        assert!(err.contains("OpenAI"), "got: {err}");
    }

    #[test]
    fn anthropic_parse_response_extracts_text_block() {
        let fixture = r#"{
            "id": "msg_1234567890",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello! I'm Claude, an AI assistant. How can I help you today?"}
            ],
            "model": "claude-haiku-4-5",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 20}
        }"#;
        assert_eq!(
            anthropic_parse_response(fixture).unwrap(),
            "Hello! I'm Claude, an AI assistant. How can I help you today?"
        );
    }

    #[test]
    fn anthropic_parse_response_rejects_no_text_blocks() {
        let fixture = r#"{"id": "x", "type": "message", "role": "assistant", "content": []}"#;
        let err = anthropic_parse_response(fixture).unwrap_err();
        assert!(err.contains("no text"), "got: {err}");
    }

    #[test]
    fn anthropic_parse_response_rejects_garbage() {
        let err = anthropic_parse_response("not json").unwrap_err();
        assert!(err.contains("Anthropic"), "got: {err}");
    }

    // -- settings persistence ----------------------------------------------

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("qu-llm-providers-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut settings = ProviderSettings::default();
        settings.provider = ProviderKind::Openai;
        settings.openai_api_key = Some("sk-test-not-a-real-key-placeholder".to_string());
        save_settings(&dir, &settings).expect("save should succeed");

        let loaded = load_settings(&dir);
        assert_eq!(loaded.provider, ProviderKind::Openai);
        assert_eq!(loaded.openai_api_key.as_deref(), Some("sk-test-not-a-real-key-placeholder"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_settings_file_falls_back_to_local_only_defaults() {
        let dir = std::env::temp_dir().join(format!("qu-llm-providers-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let loaded = load_settings(&dir);
        assert_eq!(loaded.provider, ProviderKind::Local);
        assert!(loaded.openai_api_key.is_none());
        assert!(loaded.anthropic_api_key.is_none());
    }

    #[test]
    fn public_settings_never_exposes_the_real_key() {
        let mut settings = ProviderSettings::default();
        settings.openai_api_key = Some("sk-super-secret-placeholder-value".to_string());
        let public = PublicProviderSettings::from(&settings);
        let serialized = serde_json::to_string(&public).unwrap();
        assert!(!serialized.contains("sk-super-secret-placeholder-value"));
        assert!(public.has_openai_key);
    }

    #[test]
    fn apply_update_none_key_preserves_existing_saved_key() {
        let dir = std::env::temp_dir().join(format!("qu-llm-providers-test-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        apply_update(
            &dir,
            ProviderSettingsUpdate {
                provider: ProviderKind::Openai,
                openai_model: None,
                anthropic_model: None,
                openai_api_key: Some("sk-first-placeholder".to_string()),
                anthropic_api_key: None,
            },
        )
        .unwrap();

        // A later save that only changes `provider` (key field `None`) must
        // NOT clear the key that's already on disk.
        let after = apply_update(
            &dir,
            ProviderSettingsUpdate {
                provider: ProviderKind::Anthropic,
                openai_model: None,
                anthropic_model: None,
                openai_api_key: None,
                anthropic_api_key: None,
            },
        )
        .unwrap();
        assert_eq!(after.openai_api_key.as_deref(), Some("sk-first-placeholder"));
        assert_eq!(after.provider, ProviderKind::Anthropic);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_update_empty_string_key_clears_it() {
        let dir = std::env::temp_dir().join(format!("qu-llm-providers-test-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        apply_update(
            &dir,
            ProviderSettingsUpdate {
                provider: ProviderKind::Openai,
                openai_model: None,
                anthropic_model: None,
                openai_api_key: Some("sk-to-be-cleared-placeholder".to_string()),
                anthropic_api_key: None,
            },
        )
        .unwrap();

        let after = apply_update(
            &dir,
            ProviderSettingsUpdate {
                provider: ProviderKind::Openai,
                openai_model: None,
                anthropic_model: None,
                openai_api_key: Some(String::new()),
                anthropic_api_key: None,
            },
        )
        .unwrap();
        assert!(after.openai_api_key.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- key-never-leaks, exercised against a REAL (failing) network call --

    /// Exercises the same "key never leaks into an error" guarantee,
    /// directly against `http_post_json` with a
    /// URL that cannot resolve -- no `#[ignore]` needed, this fails fast
    /// (DNS resolution failure, not a hung connection) and needs no real
    /// provider reachability.
    #[test]
    fn key_never_leaks_into_error_via_bad_host() {
        let fake_key_marker = "sk-THIS-LOOKS-LIKE-A-REAL-OPENAI-KEY-0123456789";
        let body = openai_request_body("gpt-4o-mini", "s", "u", 5);
        let auth = format!("Bearer {fake_key_marker}");
        let err = http_post_json(
            "https://this-host-does-not-exist.invalid.example/v1/chat/completions",
            &[("Authorization", &auth), ("Content-Type", "application/json")],
            &body,
            "OpenAI",
        )
        .expect_err("a nonexistent host must fail");
        assert!(!err.contains(fake_key_marker), "key leaked into error: {err}");
        assert!(!err.contains("Bearer"), "auth scheme leaked into error: {err}");
    }
}
