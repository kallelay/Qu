//! Local, in-process LLM inference for Qu's planned "AI core" (§ mascot/
//! autocomplete/fix-button/generate-button, all deliberately OUT of scope
//! for this crate -- see IMPL.md's dated entry for the full brief). This
//! crate is only the inference primitive those UI features will eventually
//! sit on top of: load a small chat model once, run greedy text generation
//! on it, entirely on-device, in-process (no shelling out, no network calls
//! at generation time, no cloud API/user API key).
//!
//! **Why `candle`, not `llama-cpp-rs`/`tch`.** Qu already learned the hard
//! way (shelling out to Python for PyTorch interop) that spawning an
//! external process per call is a non-starter (~2.9s just for `import
//! torch`). `llama-cpp-rs` (bindings to llama.cpp) needs a C++/cmake
//! toolchain to build -- exactly the kind of Windows build friction this
//! repo has hit before with `hdf5-metno` (see `qu-interp`'s own Cargo.toml
//! comment on `h5-models`: MSVC/cmake-generator issues even with vcvars
//! applied). `candle` (HuggingFace's ML framework) is pure Rust end to end;
//! `cargo build` alone pulled it and its quantized-GGUF loader in cleanly on
//! this Windows dev machine with zero extra toolchain setup -- confirmed by
//! actually building it, not assumed.
//!
//! **Why NOT the originally-proposed Bonsai model.** Bonsai (PrismML/
//! deepgrove) is a ternary/1.58-bit-weight Llama-architecture model,
//! distributed as GGUF. Before writing any integration code, this crate's
//! development checked candle's actual GGUF quant-type support directly in
//! its source (`candle_core::quantized::GgmlDType`, both v0.8.4 and the
//! latest v0.11.0 at the time): it lists exactly `F32/F16/BF16/Q4_0/Q4_1/
//! Q5_0/Q5_1/Q8_0/Q8_1/Q2K..Q8K` -- the standard GGML k-quant family. There
//! is no `TQ1_0`/`TQ2_0` (the ternary/BitNet quant types llama.cpp itself
//! added), and no ternary-packing support anywhere in the quantized module.
//! candle categorically cannot load Bonsai's GGUF files. Per this task's own
//! contingency plan, this crate falls back to a small, standard 4-bit-
//! quantized instruct model instead, picked by what actually loads and
//! generates real text rather than by spec sheet: **TinyLlama-1.1B-Chat-v1.0**
//! (`TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF`, `Q4_K_M` quant, ~669MB) --
//! `candle-transformers`'s own `quantized_llama` module is the exact code
//! path the upstream candle repo's own `quantized` example exercises against
//! this same model family, and a real end-to-end run during this crate's
//! development (prompt "The capital of France is") produced "Paris." as the
//! very next word, confirmed by literally printing the generated text, not
//! guessed. A `Qwen2.5`-class GGUF would work identically through
//! `quantized_qwen2` (also present in `candle-transformers` 0.11) if a
//! different size/license tradeoff is wanted later -- swapping
//! [`KnownModel`] entries is the only change needed, no architecture work.
//!
//! **The hf-hub redirect bug worked around here.** `hf-hub` 0.3.2's own
//! `Api::get`/`download` path failed with "relative URL without a base" --
//! NOT for the big GGUF model file (which downloaded fine) but for the
//! small `tokenizer.json` fetched from a separate, non-GGUF repo. Traced by
//! hand (`curl -sI` on the real resolve URL) to a genuine difference in how
//! the HF Hub serves the two file classes: large LFS blobs 307-redirect to
//! a fully-qualified CloudFront URL, while small non-LFS files now
//! redirect to a *relative* `/api/resolve-cache/...` path. hf-hub's own
//! metadata-probing code hand-parses the `Location` header as if it were
//! already absolute and chokes on the relative case; a plain `ureq::get`
//! (which resolves the redirect itself instead of hand-parsing the header)
//! has no such bug -- confirmed by running both against the real network
//! before picking this design (see [`fetch_tokenizer_json`]'s own doc
//! comment). So: the big model file goes through `hf_hub::api::sync::Api`
//! as normal; the tokenizer file is fetched by hand and cached next to
//! hf-hub's own cache root.
//!
//! **Lazy, on-demand loading.** Nothing in this crate touches the network,
//! the filesystem beyond a cache-dir check, or allocates any model memory
//! until [`load`] is actually called -- matching `qu-interp`'s
//! `gpu_probe.rs` precedent (lazy, `OnceLock`-free here only because this
//! crate has no reason to cache a *result* process-globally the way the GPU
//! probe does; `qu-interp`'s own builtin wiring is what decides whether to
//! keep a loaded model alive, via its own `Interp`-scoped table -- see that
//! crate's `llm_bridge.rs`). A machine with no model downloaded yet, or
//! this crate simply not linked in (default `qu-interp` build has the `llm`
//! feature OFF), pays zero cost.

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_llama::ModelWeights;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use tokenizers::Tokenizer;

/// One entry in the small, hardcoded set of models this crate knows how to
/// fetch by a short friendly name (`llm_load("tinyllama")`, or `llm_load("")`
/// for the default). Deliberately not a config file or a registry a script
/// can extend -- growing this list is a one-line, reviewed change, matching
/// the "prove the pipeline, minimal surface" scope of this whole crate.
struct KnownModel {
    key: &'static str,
    gguf_repo: &'static str,
    gguf_file: &'static str,
    tokenizer_repo: &'static str,
    tokenizer_file: &'static str,
}

const KNOWN_MODELS: &[KnownModel] = &[KnownModel {
    key: "tinyllama",
    gguf_repo: "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
    gguf_file: "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf",
    tokenizer_repo: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
    tokenizer_file: "tokenizer.json",
}];

/// A loaded, ready-to-generate model handle. `qu-interp`'s own `llm_bridge`
/// module wraps this in an `Arc` and hands scripts back an id into its own
/// `Interp`-scoped table (the SAME "opaque native resource lives in a side
/// table, `Value` only ever holds a small `Value::Model` handle naming an
/// id" shape `Value::Worker`/`WorkerHandle` already established for
/// `spawn(...)` -- see that type's own doc comment in `qu-interp/src/
/// lib.rs`) -- this crate itself has no notion of `Value` at all, kept
/// completely independent of `qu-interp`'s own types.
pub struct LlmModel {
    /// `candle`'s per-layer KV cache lives INSIDE `ModelWeights` and
    /// `forward` takes `&mut self` -- a `Mutex` (not `RefCell`) so this type
    /// stays `Sync` and a `LlmModel` can be shared across `spawn(...)`
    /// worker threads the same way any other read-mostly Qu resource is.
    weights: Mutex<ModelWeights>,
    tokenizer: Tokenizer,
    /// From the GGUF's own `tokenizer.ggml.eos_token_id` metadata key, when
    /// present -- lets [`LlmModel::generate`] stop early instead of always
    /// running the full `max_tokens` budget. `None` (never a hardcoded
    /// guess) if the file didn't carry that key; generation just runs to
    /// `max_tokens` in that case.
    eos_token_id: Option<u32>,
    device: Device,
    name: String,
}

// `ModelWeights`/`Tokenizer` don't implement `Debug`, so this is written by
// hand rather than derived -- only needed at all so `Result<LlmModel, _>`
// satisfies `unwrap_err`'s `Debug` bound in this module's own tests.
impl std::fmt::Debug for LlmModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmModel").field("name", &self.name).finish_non_exhaustive()
    }
}

/// Loads a model by short name (currently just `"tinyllama"` -- also the
/// default when `model_name_or_path` is empty) or by an explicit local
/// `.gguf` file path (which must have a `tokenizer.json` sitting next to it
/// -- GGUF technically embeds enough vocab metadata for llama.cpp itself to
/// build a tokenizer from the file alone, but wiring that up through the
/// `tokenizers` crate is real additional work with no payoff for this
/// crate's one supported model, so it's deferred rather than half-built).
///
/// CPU-only (candle's default `Device::Cpu`) -- GPU acceleration for the
/// LLM path is explicitly out of scope for this pass (see IMPL.md's dated
/// entry); Bonsai/BitNet-style ternary models are specifically designed to
/// be fast enough on CPU alone, which was part of the original appeal, even
/// though this crate ended up on a standard quantized model instead.
pub fn load(model_name_or_path: &str) -> Result<LlmModel, String> {
    let name = if model_name_or_path.trim().is_empty() { "tinyllama" } else { model_name_or_path.trim() };

    let (gguf_path, tokenizer) = if name.ends_with(".gguf") {
        load_local(name)?
    } else if let Some(km) = KNOWN_MODELS.iter().find(|m| m.key == name) {
        load_known(km)?
    } else {
        let known: Vec<&str> = KNOWN_MODELS.iter().map(|m| m.key).collect();
        return Err(format!(
            "llm_load: unknown model `{name}` -- supported names: {} (or pass a local .gguf file path with a `tokenizer.json` next to it)",
            known.join(", ")
        ));
    };

    let device = Device::Cpu;
    let mut file = std::fs::File::open(&gguf_path)
        .map_err(|err| format!("llm_load: couldn't open `{}`: {err}", gguf_path.display()))?;
    let content = gguf_file::Content::read(&mut file)
        .map_err(|err| format!("llm_load: `{}` isn't a valid GGUF file: {err}", gguf_path.display()))?;

    // Must be read out of `content.metadata` BEFORE `ModelWeights::from_gguf`
    // below, which consumes `content` by value.
    let eos_token_id = content
        .metadata
        .get("tokenizer.ggml.eos_token_id")
        .and_then(|v| v.to_u32().ok());

    let weights = ModelWeights::from_gguf(content, &mut file, &device).map_err(|err| {
        format!("llm_load: candle couldn't build the model from `{}`: {err}", gguf_path.display())
    })?;

    Ok(LlmModel { weights: Mutex::new(weights), tokenizer, eos_token_id, device, name: name.to_string() })
}

fn load_known(km: &KnownModel) -> Result<(PathBuf, Tokenizer), String> {
    let api = hf_hub::api::sync::Api::new()
        .map_err(|err| format!("llm_load: couldn't set up the Hugging Face cache: {err}"))?;
    let gguf_path = api.model(km.gguf_repo.to_string()).get(km.gguf_file).map_err(|err| {
        format!("llm_load: couldn't download `{}/{}`: {err}", km.gguf_repo, km.gguf_file)
    })?;
    let tok_path = fetch_tokenizer_json(km.tokenizer_repo, km.tokenizer_file)?;
    let tokenizer = Tokenizer::from_file(&tok_path)
        .map_err(|err| format!("llm_load: couldn't parse tokenizer `{}`: {err}", tok_path.display()))?;
    Ok((gguf_path, tokenizer))
}

fn load_local(gguf_path_str: &str) -> Result<(PathBuf, Tokenizer), String> {
    let gguf_path = PathBuf::from(gguf_path_str);
    if !gguf_path.exists() {
        return Err(format!("llm_load: no such file `{gguf_path_str}`"));
    }
    let tok_path = gguf_path.with_file_name("tokenizer.json");
    if !tok_path.exists() {
        return Err(format!(
            "llm_load: `{gguf_path_str}` needs a `tokenizer.json` file next to it (looked for `{}`)",
            tok_path.display()
        ));
    }
    let tokenizer = Tokenizer::from_file(&tok_path)
        .map_err(|err| format!("llm_load: couldn't parse `{}`: {err}", tok_path.display()))?;
    Ok((gguf_path, tokenizer))
}

/// Downloads `tokenizer_file` from `repo`'s `main` branch with a plain
/// blocking `ureq::get` instead of `hf_hub::api::sync::Api::get`.
///
/// hf-hub 0.3.2's own download path failed here with "relative URL without
/// a base" -- confirmed by running it against the real network while
/// building this crate (2026-08-27), for THIS file specifically, even
/// though the same crate's API happily downloaded the much bigger GGUF
/// model file moments earlier. `curl -sI` on the real resolve URL explained
/// why: large LFS-tracked blobs 307-redirect to a fully-qualified
/// CloudFront URL, but small, non-LFS files (like a plain `tokenizer.json`)
/// now redirect to a *relative* path (`Location: /api/resolve-cache/...`).
/// hf-hub's own redirect-metadata code calls `Url::parse` directly on that
/// header as though it were already absolute, which is exactly what
/// `RelativeUrlWithoutBase` means. A bare `ureq::get(...).call()` resolves
/// the SAME redirect correctly (confirmed against the real URL) because it
/// joins the `Location` header against the request's own base internally
/// instead of hand-parsing it -- so this function just does that, and skips
/// `hf_hub` entirely for this one file.
///
/// Cached by hand under a `qu-llm-tokenizers` directory next to hf-hub's
/// own cache root (so it still respects a custom `HF_HOME`), keyed by repo
/// name, so a second `load` call for the same known model doesn't
/// re-download it.
fn fetch_tokenizer_json(repo: &str, file: &str) -> Result<PathBuf, String> {
    let cache_root = hf_hub::Cache::default()
        .path()
        .parent()
        .map(|p| p.join("qu-llm-tokenizers"))
        .unwrap_or_else(|| PathBuf::from(".qu-llm-tokenizers"));
    let dest_dir = cache_root.join(repo.replace('/', "--"));
    let dest = dest_dir.join(file);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(&dest_dir)
        .map_err(|err| format!("llm_load: couldn't create cache dir `{}`: {err}", dest_dir.display()))?;
    let url = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let resp = ureq::get(&url).call().map_err(|err| format!("llm_load: couldn't download `{url}`: {err}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|err| format!("llm_load: couldn't read the response body from `{url}`: {err}"))?;
    std::fs::write(&dest, &bytes).map_err(|err| format!("llm_load: couldn't write `{}`: {err}", dest.display()))?;
    Ok(dest)
}

impl LlmModel {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Greedy (argmax) generation only -- no temperature/top-k/top-p
    /// sampling knob yet, and no token-by-token streaming callback; both
    /// are natural follow-ups but out of scope for this pass (see this
    /// crate's own module doc comment / IMPL.md's dated entry). Every call
    /// clears the model's KV cache first, so two `generate` calls on the
    /// same handle never see each other's context -- each call is a fresh,
    /// independent completion of `prompt`, not a running conversation.
    pub fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String, String> {
        let mut weights =
            self.weights.lock().map_err(|_| "llm generate: model lock was poisoned by an earlier panic".to_string())?;
        weights.clear_kv_cache();

        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|err| format!("llm generate: couldn't tokenize the prompt: {err}"))?;
        let mut all_tokens: Vec<u32> = encoding.get_ids().to_vec();
        if all_tokens.is_empty() {
            return Err("llm generate: the prompt tokenized to zero tokens".to_string());
        }

        let mut generated: Vec<u32> = Vec::with_capacity(max_tokens);
        for step in 0..max_tokens {
            let (context, start_pos): (&[u32], usize) =
                if step == 0 { (&all_tokens, 0) } else { (std::slice::from_ref(all_tokens.last().unwrap()), all_tokens.len() - 1) };
            let input = Tensor::new(context, &self.device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|err| format!("llm generate: couldn't build the input tensor: {err}"))?;
            let logits = weights
                .forward(&input, start_pos)
                .map_err(|err| format!("llm generate: forward pass failed: {err}"))?;
            let logits = logits.squeeze(0).map_err(|err| format!("llm generate: couldn't read the logits: {err}"))?;
            let next_token = logits
                .argmax(0)
                .and_then(|t| t.to_scalar::<u32>())
                .map_err(|err| format!("llm generate: couldn't pick the next token: {err}"))?;

            if Some(next_token) == self.eos_token_id {
                break;
            }
            all_tokens.push(next_token);
            generated.push(next_token);
        }

        // Decode every generated id TOGETHER, not one at a time: SentencePiece/
        // BPE tokenizers attach a leading-space marker to the FIRST token of a
        // word, which only round-trips correctly when neighboring tokens are
        // decoded as one batch. Decoding token-by-token and concatenating raw
        // strings (this crate's own early feasibility probe did exactly this
        // before the bug was noticed) silently drops every inter-word space.
        self.tokenizer.decode(&generated, true).map_err(|err| format!("llm generate: couldn't decode the output: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises ONLY the local-path branch's validation, no network/model
    /// download required -- keeps this crate's default `cargo test` fast
    /// and hermetic. The real generation test (load a real model, generate
    /// real text, assert it's coherent) lives in `qu-interp`'s own test
    /// suite as an `#[ignore]`d test, per this task's own "don't break the
    /// normal suite for machines without the model cached" requirement --
    /// see that crate's `llm_bridge.rs`.
    #[test]
    fn load_local_rejects_a_missing_gguf_file() {
        let err = load_local("Z:/definitely/not/a/real/path.gguf").unwrap_err();
        assert!(err.contains("no such file"), "got: {err}");
    }

    #[test]
    fn load_rejects_an_unknown_short_name() {
        let err = load("not-a-real-model-name").unwrap_err();
        assert!(err.contains("unknown model"), "got: {err}");
        assert!(err.contains("tinyllama"), "got: {err}");
    }
}
