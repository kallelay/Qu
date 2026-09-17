//! `llm_load(...)`/`.generate(...)` bridge to the `qu-llm` crate (feature
//! `llm`, see `Cargo.toml`'s own comment) -- the "AI core" inference
//! primitive (§ mascot/autocomplete/fix-button/generate-button, all
//! deliberately OUT of scope here; see IMPL.md's dated entry for the full
//! brief this crate's own module doc comment also carries).
//!
//! **Why a live handle lives HERE, not inside a `Value`.** A loaded
//! `qu_llm::LlmModel` owns real native state (candle's quantized weights
//! plus a per-layer KV cache, wrapped in a `Mutex` since `forward` takes
//! `&mut self`) -- exactly the same "can't live inside a `Clone`-able
//! `Value` directly" situation `spawn(...)`'s `Value::Worker(u64)` already
//! solved (see that variant's own doc comment in `lib.rs`): the real
//! resource lives in an `Interp`-scoped side table (`Interp::llm_models`,
//! mirroring `Interp::workers`/`Interp::pool_registry`'s own established
//! shape exactly), and the `Value` a script actually holds only names an
//! id. This crate's own `Value` enum is NOT touched at all by this feature
//! -- adding a new variant would mean updating every one of the many
//! exhaustive matches over `Value` in `lib.rs` (`type_name`, `truthy`,
//! display, ...) even for builds with `llm` OFF, for a feature whose whole
//! point is to cost nothing when unused. Instead, `llm_load` returns an
//! ordinary `Value::Model` (kind `"llm"`, following the EXACT same
//! protocol `qr`/`svd`/`sequential`/... already use -- see `ModelHandle`'s
//! own doc comment) with one field, `"id"`, naming the row in this table.
//! `.generate(...)` is an ordinary method call on that handle via the
//! pre-existing `recv.method(args)` sugar -- no new syntax.
//!
//! **Scope.** Greedy generation only, no streaming, no chat templating
//! beyond whatever the tokenizer's own `encode` does with the raw prompt
//! string handed to it -- see `qu-llm`'s own module doc comment for the
//! full model-choice investigation and what's deferred.

use crate::model::ModelHandle;
use crate::{EvalError, Value, R};
use std::sync::Arc;

/// `llm_load(name_or_path)` -- see `qu_llm::load`'s own doc comment for
/// what `name_or_path` accepts (a short known-model name like `"tinyllama"`,
/// empty string for that same default, or a local `.gguf` path with a
/// `tokenizer.json` next to it). Registers the loaded model in `interp`'s
/// own `llm_models` table and hands back a `kind="llm"` `Value::Model`
/// naming its id -- see this module's own doc comment for why the live
/// handle isn't carried in the `Value` itself.
pub fn llm_load(interp: &mut crate::Interp, name_or_path: &str) -> R<Value> {
    let model = qu_llm::load(name_or_path).map_err(|msg| EvalError { msg })?;
    let name = model.name().to_string();
    let id = interp.next_llm_id;
    interp.next_llm_id += 1;
    interp.llm_models.insert(id, Arc::new(model));
    Ok(Value::Model(Arc::new(ModelHandle::new(
        "llm",
        vec![("id".to_string(), Value::Num(id as f64)), ("name".to_string(), Value::Str(name))],
    ))))
}

/// `recv.generate(prompt, [max_tokens])` -- `recv` must be a `kind="llm"`
/// `Value::Model` from [`llm_load`]. `max_tokens` defaults to 64 (a short,
/// fast-enough-on-CPU completion, plenty to prove the pipeline end to end;
/// a script that wants more just passes a bigger number, no cap enforced
/// here beyond what generation itself takes time-wise).
pub fn generate(interp: &crate::Interp, model: &ModelHandle, prompt: &str, max_tokens: usize) -> R<Value> {
    if model.kind != "llm" {
        return Err(EvalError {
            msg: format!("generate: expected a model from llm_load(...), got a `{}` model", model.kind),
        });
    }
    let id = match model.field("id") {
        Some(Value::Num(n)) => *n as u64,
        _ => return Err(EvalError { msg: "generate: this llm model handle is missing its `id` field".into() }),
    };
    let handle = interp.llm_models.get(&id).ok_or_else(|| EvalError {
        msg: "generate: this llm model handle is no longer valid (from a different Interp?)".into(),
    })?;
    let text = handle.generate(prompt, max_tokens).map_err(|msg| EvalError { msg })?;
    Ok(Value::Str(text))
}
