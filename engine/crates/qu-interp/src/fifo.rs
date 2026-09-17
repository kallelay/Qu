//! `fifo(capacity)` (§ data structures pass, 2026-09-01) — a bounded,
//! circular FIFO/ring buffer: a plain first-in-first-out DATA container a
//! script pushes/pops values into directly.
//!
//! Deliberately distinct from the existing `queue()` (`Value::Queue`,
//! §47.3): that one is a queue of DEFERRED JOBS for a worker pool to drain
//! later (`.push(fnName, args...)`, run via `run q on <pool>`) — no plain
//! value is ever stored in it, only `(fnName, args)` job specs. This is the
//! ordinary "buffer of values" collection §47.4 separately named `queue()`
//! for (never implemented under that name — see `Value::Queue`'s own doc
//! comment in `lib.rs`), shipped here under the name the brief actually
//! asked for (`fifo`) specifically so it doesn't collide with the
//! already-shipped job queue.
//!
//! **Full-buffer behavior — decided and documented here, since both are
//! legitimate but different data structures**: `push` on a full `fifo`
//! evicts the OLDEST element to make room for the new one (real ring-
//! buffer/circular-queue semantics — the task's own name for this feature
//! is literally "circular queue"), rather than rejecting the new push with
//! an error. This is the more broadly useful shape for what a fixed-size
//! buffer is actually for in practice — a bounded sliding window of the
//! most recent N samples/events/log lines, the classic ring-buffer role —
//! and Qu already has real reject/back-pressure primitives elsewhere for
//! the other case (`semaphore`'s `acquire`/`release` genuinely blocks a
//! producer; `channel_send` is an unbounded MPMC queue), so a second
//! bounded-and-rejecting container would just duplicate what already
//! exists, whereas nothing in Qu today gives a fixed-size "keep only the
//! most recent N" buffer. `is_full()`/`len()` let a script detect that an
//! eviction is imminent if it needs to react before pushing.
//!
//! `Arc<StdMutex<FifoState>>` — same "live handle, shared on clone, mutates
//! across calls" convention `Value::Queue`/`Value::Channel` already use.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use crate::{arg0, e, EvalError, Value, R};

pub struct FifoState {
    capacity: usize,
    data: VecDeque<Value>,
}

impl FifoState {
    /// Used by `lib.rs`'s `value_len` so `len(f)`/`length(f)`/`numel(f)`
    /// (the existing generic length dispatch every collection already
    /// shares) work for a `fifo` too, with no fifo-specific `"len"` match
    /// arm needed.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Used by `lib.rs`'s `display_value` to show occupancy/capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl std::fmt::Debug for FifoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FifoState")
            .field("capacity", &self.capacity)
            .field("len", &self.data.len())
            .finish()
    }
}

fn as_fifo(v: &Value) -> R<&Arc<StdMutex<FifoState>>> {
    match v {
        Value::Fifo(f) => Ok(f),
        other => e(format!("expected a fifo (from fifo(capacity)), found {}", other.type_name())),
    }
}

/// `fifo(capacity)` — a new, empty ring buffer of the given fixed capacity.
/// The capacity is set once at construction; there is no resize builtin —
/// a script that needs a different size makes a new `fifo`.
fn new(args: &[Value]) -> R<Value> {
    let cap = crate::int_arg(args, 0)?;
    if cap <= 0 {
        return e(format!("fifo(capacity): capacity must be a positive integer, got {cap}"));
    }
    Ok(Value::Fifo(Arc::new(StdMutex::new(FifoState {
        capacity: cap as usize,
        data: VecDeque::new(),
    }))))
}

/// `.push(value)` — appends at the back; evicts the oldest element first if
/// already at capacity (see this module's own doc comment for why).
fn push(args: &[Value]) -> R<Value> {
    let f = as_fifo(arg0(args)?)?;
    let value = args.get(1).cloned().ok_or_else(|| EvalError {
        msg: "push(fifo, value) needs a value".into(),
    })?;
    let mut st = f.lock().unwrap();
    if st.data.len() >= st.capacity {
        st.data.pop_front();
    }
    st.data.push_back(value);
    Ok(Value::Nothing)
}

/// `.pop()` — removes and returns the oldest element; a clear error (not
/// `Nothing`) on an empty fifo, matching this codebase's "flag it, don't
/// fake it" convention for an operation with no sensible empty-input
/// result (as opposed to e.g. `read_line`'s `Nothing`-at-EOF, which IS a
/// sensible, expected outcome of ordinary use).
fn pop(args: &[Value]) -> R<Value> {
    let f = as_fifo(arg0(args)?)?;
    let mut st = f.lock().unwrap();
    st.data.pop_front().ok_or_else(|| EvalError {
        msg: "pop: fifo is empty".into(),
    })
}

/// `.peek()` — the oldest element, WITHOUT removing it. Same "clear error
/// on empty" convention as `.pop()`.
fn peek(args: &[Value]) -> R<Value> {
    let f = as_fifo(arg0(args)?)?;
    let st = f.lock().unwrap();
    st.data.front().cloned().ok_or_else(|| EvalError {
        msg: "peek: fifo is empty".into(),
    })
}

/// `.is_full()` — `true` iff the next `.push` would evict.
fn is_full(args: &[Value]) -> R<Value> {
    let f = as_fifo(arg0(args)?)?;
    let st = f.lock().unwrap();
    Ok(Value::Bool(st.data.len() >= st.capacity))
}

/// `.is_empty()` — `true` iff `.pop()`/`.peek()` would error right now.
fn is_empty(args: &[Value]) -> R<Value> {
    let f = as_fifo(arg0(args)?)?;
    let st = f.lock().unwrap();
    Ok(Value::Bool(st.data.is_empty()))
}

/// Single dispatch entry point, called from `lib.rs`'s big builtin match —
/// see that match's own comment for why this is one (mostly) combined arm
/// rather than one per name (same "reduce the diff footprint in a file
/// another pass is editing" reasoning `fs_ops::call`/`py_exec::call`
/// already established).
pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "fifo" => new(args),
        "push" => push(args),
        "pop" => pop(args),
        "peek" => peek(args),
        "is_full" => is_full(args),
        "is_empty" => is_empty(args),
        other => e(format!("fifo: internal dispatch error, unhandled `{other}`")),
    }
}
