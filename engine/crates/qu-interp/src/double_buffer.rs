//! `double_buffer(initial)` (§ data structures pass, 2026-09-01) — a real
//! double-buffering primitive, the standard real-time/embedded/graphics
//! pattern: writers write into a "back" buffer while readers keep observing
//! a stable "front" buffer, until an explicit `.swap()` atomically
//! exchanges the two. The entire reason this data structure exists (rather
//! than just a plain shared `mutex(initial)`) is that a reader's `.read()`
//! must NEVER observe a write that hasn't been swapped in yet — see the
//! acceptance test `double_buffer_read_stays_stale_until_swap_then_sees_new_value`
//! in `tests/acceptance.rs`, which checks exactly that property (not just
//! "read/write work at all").
//!
//! `Arc<StdMutex<DoubleBufferState>>` — same "shared handle to live internal
//! state" convention `Value::Timer`/`Value::File` already use: cloning a
//! `Value::DoubleBuffer` shares the SAME front/back pair. `.swap()` is a
//! single lock-held `std::mem::swap` on the two fields, so any `.read()`
//! going through the same `Mutex` always observes either the whole
//! pre-swap or whole post-swap state — never a torn, half-swapped mix.

use std::sync::{Arc, Mutex as StdMutex};

use crate::{arg0, e, EvalError, Value, R};

pub struct DoubleBufferState {
    front: Value,
    back: Value,
}

impl DoubleBufferState {
    /// Used by `lib.rs`'s `display_value` to show both current slots.
    pub fn front(&self) -> &Value {
        &self.front
    }
    pub fn back(&self) -> &Value {
        &self.back
    }
}

impl std::fmt::Debug for DoubleBufferState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleBufferState").finish_non_exhaustive()
    }
}

fn as_double_buffer(v: &Value) -> R<&Arc<StdMutex<DoubleBufferState>>> {
    match v {
        Value::DoubleBuffer(b) => Ok(b),
        other => e(format!(
            "expected a double buffer (from double_buffer(initial)), found {}",
            other.type_name()
        )),
    }
}

/// `double_buffer(initial)` — front AND back both start out equal to
/// `initial`, so the very first `.read()` (before any `.write`/`.swap`) is
/// a well-defined value rather than `Nothing`/some placeholder.
fn new(args: &[Value]) -> R<Value> {
    let initial = arg0(args)?.clone();
    Ok(Value::DoubleBuffer(Arc::new(StdMutex::new(DoubleBufferState {
        front: initial.clone(),
        back: initial,
    }))))
}

/// `.write(value)` — writes ONLY the back buffer. `.read()` (which only
/// ever looks at `front`) is completely unaffected by this until the next
/// `.swap()` — this one fact is the entire mechanism the data structure
/// exists for.
fn write(args: &[Value]) -> R<Value> {
    let b = as_double_buffer(arg0(args)?)?;
    let value = args.get(1).cloned().ok_or_else(|| EvalError {
        msg: "write(double_buffer, value) needs a value".into(),
    })?;
    b.lock().unwrap().back = value;
    Ok(Value::Nothing)
}

/// `.read()` — always returns the current FRONT buffer, never the back one.
fn read(args: &[Value]) -> R<Value> {
    let b = as_double_buffer(arg0(args)?)?;
    Ok(b.lock().unwrap().front.clone())
}

/// `.swap()` — atomically exchanges front and back (a single
/// `std::mem::swap` while holding the one lock both fields live behind).
/// After this call: `.read()` returns whatever was most recently
/// `.write()`-ten, and the OLD front becomes the new back (so a later
/// `.write()` with no intervening `.swap()` overwrites it, exactly as a
/// real double buffer's back slot should behave).
fn swap(args: &[Value]) -> R<Value> {
    let b = as_double_buffer(arg0(args)?)?;
    let mut guard = b.lock().unwrap();
    // One explicit reborrow first (`&mut *guard`), so the two field
    // borrows below are plain disjoint struct-field projections off a
    // single `&mut DoubleBufferState` — taking `&mut guard.front`/`&mut
    // guard.back` directly would instead require the compiler to prove
    // disjointness across two separate `DerefMut::deref_mut` calls on the
    // `MutexGuard`, which it cannot.
    let st: &mut DoubleBufferState = &mut guard;
    std::mem::swap(&mut st.front, &mut st.back);
    Ok(Value::Nothing)
}

/// Single dispatch entry point, called from `lib.rs`'s big builtin match —
/// see that match's own comment for why this is a combined arm. `"write"`
/// is dispatched here only when `lib.rs`'s own guard confirms `arg0` is a
/// `Value::DoubleBuffer` (plain `write(x)` on anything else keeps its
/// pre-existing "print to console" meaning, and `write(file, s)` keeps its
/// own separate meaning too) — `"double_buffer"`/`"read"`/`"swap"` have no
/// competing meaning anywhere else, so those three are unconditional.
pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "double_buffer" => new(args),
        "write" => write(args),
        "read" => read(args),
        "swap" => swap(args),
        other => e(format!("double_buffer: internal dispatch error, unhandled `{other}`")),
    }
}
