//! `linked_list()` (§ data structures pass, 2026-09-01) — a double-ended
//! list of arbitrary `Value`s with O(1) push/pop at BOTH ends.
//!
//! **Deliberately backed by `std::collections::VecDeque<Value>`, not a real
//! chain of individually heap-allocated nodes.** The traditional reason a
//! linked list's node-chain layout exists at all is O(1) insertion/removal
//! at an arbitrary position a caller already holds a direct reference/
//! pointer to, splicing without shifting the rest of the collection. Qu has
//! no such concept anywhere: there is no way for a script to obtain or hold
//! a reference to "the third node" and later splice next to it — every
//! operation this feature asks for (`push_front`/`push_back`/`pop_front`/
//! `pop_back`/`len`/`to_vec`) only ever touches the two ends or walks the
//! whole thing, and `VecDeque` gives IDENTICAL O(1) amortized complexity
//! for every one of those, backed by one growable ring-buffer allocation
//! with real cache locality, instead of one scattered heap allocation per
//! element plus pointer-chasing on every traversal. A genuine boxed-node
//! chain here would be strictly worse for every actual caller (slower
//! iteration/`to_vec`, more allocations, more `unsafe` surface for
//! self-referential-adjacent code) purely to satisfy "is this REALLY a
//! linked list" — the more honest, useful choice is the one that matches
//! how this is actually ever called from Qu script code, which is this.
//!
//! `Arc<StdMutex<VecDeque<Value>>>` directly AS `Value::LinkedList`'s own
//! payload — no separate wrapper state struct, since there is nothing to
//! track beyond the deque itself. Same "live, shared-on-clone, mutates
//! across calls" handle convention `Value::Queue`/`Value::Channel` already
//! use.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use crate::{arg0, e, EvalError, Value, R};

pub fn as_linked_list(v: &Value) -> R<&Arc<StdMutex<VecDeque<Value>>>> {
    match v {
        Value::LinkedList(l) => Ok(l),
        other => e(format!("expected a linked list (from linked_list()), found {}", other.type_name())),
    }
}

/// `linked_list()` — a new, empty list.
fn new() -> Value {
    Value::LinkedList(Arc::new(StdMutex::new(VecDeque::new())))
}

/// `.push_front(v)` — O(1) amortized, prepends.
fn push_front(args: &[Value]) -> R<Value> {
    let l = as_linked_list(arg0(args)?)?;
    let value = args.get(1).cloned().ok_or_else(|| EvalError {
        msg: "push_front(list, value) needs a value".into(),
    })?;
    l.lock().unwrap().push_front(value);
    Ok(Value::Nothing)
}

/// `.push_back(v)` — O(1) amortized, appends.
fn push_back(args: &[Value]) -> R<Value> {
    let l = as_linked_list(arg0(args)?)?;
    let value = args.get(1).cloned().ok_or_else(|| EvalError {
        msg: "push_back(list, value) needs a value".into(),
    })?;
    l.lock().unwrap().push_back(value);
    Ok(Value::Nothing)
}

/// `.pop_front()` — removes and returns the front element; a clear error
/// (not `Nothing`) on an empty list, matching `fifo`'s own `.pop()`
/// convention for the same reason (see `fifo.rs`'s doc comment).
fn pop_front(args: &[Value]) -> R<Value> {
    let l = as_linked_list(arg0(args)?)?;
    l.lock().unwrap().pop_front().ok_or_else(|| EvalError {
        msg: "pop_front: linked list is empty".into(),
    })
}

/// `.pop_back()` — removes and returns the back element.
fn pop_back(args: &[Value]) -> R<Value> {
    let l = as_linked_list(arg0(args)?)?;
    l.lock().unwrap().pop_back().ok_or_else(|| EvalError {
        msg: "pop_back: linked list is empty".into(),
    })
}

/// `.to_vec()` — materializes to a plain `Value::List`, front to back, for
/// interop with the rest of Qu (numeric indexing, `length`, `print`, ...).
/// Named `to_vec` (matching the brief) even though the result is a
/// `Value::List`, not a `Value::Vec` — a `linked_list` is heterogeneous
/// (any `Value` element), so `List` (Qu's general ordered collection) is
/// the only faithful target, the same reasoning `map`'s own doc comment in
/// `collections.rs` already spells out for why IT always returns `List`.
fn to_vec(args: &[Value]) -> R<Value> {
    let l = as_linked_list(arg0(args)?)?;
    let items: Vec<Value> = l.lock().unwrap().iter().cloned().collect();
    Ok(Value::List(Arc::new(items)))
}

/// Single dispatch entry point, called from `lib.rs`'s big builtin match —
/// see that match's own comment for why this is a combined arm. None of
/// these five names collide with any existing builtin, so every arm here
/// is unconditional (no `Value`-type guard needed, unlike `fifo`'s `push`
/// or `double_buffer`'s `write`).
pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "linked_list" => Ok(new()),
        "push_front" => push_front(args),
        "push_back" => push_back(args),
        "pop_front" => pop_front(args),
        "pop_back" => pop_back(args),
        "to_vec" => to_vec(args),
        other => e(format!("linked_list: internal dispatch error, unhandled `{other}`")),
    }
}
