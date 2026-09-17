//! `zip`/`map`/`dict` (§47.4, scoped/shipped 2026-08-26 — see BACKLOG.md
//! "`for` over collections, `zip`, `map`, `dict`"). `for x in expr` real
//! grammar and `Table`/`.=` bracket-indexing are OUT of scope here —
//! separate concurrent work owns those, both also touching `lib.rs`, which
//! is exactly why this pass lives in its own module instead of growing
//! `lib.rs`'s builtin-dispatch match further.
//!
//! `lib.rs`'s own dispatch (`"zip" | "map" | "dict" | "get" | "set" |
//! "keys" | "values"` match arms) calls straight into the functions below;
//! see those arms' doc comments for the exact calling convention each
//! builtin exposes to Qu source.

use crate::{arg0, collection_elems, e, fmt_num, text_arg, EvalError, Interp, Value, BUILTIN_NAMES, R};
use std::sync::Arc;

/// Read the "which function" argument of a higher-order builtin.
///
/// Two spellings reach here, and they are held to different rules:
///
///   * a NAME STRING (`map("dbl", xs)`) -- the only spelling the language
///     had before function values existed. Still restricted to functions
///     the script defined, because a bare string is indistinguishable
///     from a typo and `map("sin", xs)` silently meaning the builtin
///     `sin` was never the intent.
///   * a FUNCTION VALUE (`map(dbl, xs)`, `map(sqrt, xs)`, `map((x) := x*2,
///     xs)`) -- unambiguous by construction: whatever it names was
///     resolved when the value was made. So a builtin is allowed here,
///     and a lambda arrives already registered under its own name.
///
/// Same string, different permission, decided by how it was written.
fn callable_arg(interp: &Interp, args: &[Value], idx: usize, who: &str, verb: &str) -> R<String> {
    let by_value = matches!(args.get(idx), Some(Value::Func(_)));
    let name = text_arg(args, idx)?;
    if interp.has_user_fn(&name) || (by_value && BUILTIN_NAMES.contains(&name.as_str())) {
        return Ok(name);
    }
    // Something that could never have been meant as a function at all --
    // a list, a number, a table. Naming its TYPE is the useful thing to
    // say; the old message stringified the value and produced "no user
    // function named `[3, 4]`", which reads as if a list were a plausible
    // spelling of a name.
    if !by_value && !matches!(args.get(idx), Some(Value::Str(_))) {
        return e(format!(
            "{who}({}) needs a function, found {}",
            if idx == 0 { "f, xs" } else { "xs, f" },
            args.get(idx).map_or("nothing", Value::type_name)
        ));
    }
    e(format!(
        "{who}: no user function named `{name}` (only functions you defined can be {verb}, \
         not builtins -- pass the builtin itself, without quotes, if that is what you meant)"
    ))
}

/// Would this argument be accepted as "the function to call"?
///
/// Deliberately NOT "is it a string" -- a `Str` only counts when it names
/// a function that exists. That is what keeps `map(names, "upper")`
/// (a list of strings and a builtin's name) from being read as a
/// collection of one string mapped over a list.
fn is_callable_arg(interp: &Interp, v: Option<&Value>) -> bool {
    match v {
        Some(Value::Func(_)) => true,
        Some(Value::Str(s)) => interp.has_user_fn(s),
        _ => false,
    }
}

/// Work out which argument is the function and which is the data, for a
/// higher-order builtin that takes one of each.
///
/// **The canonical order is the collection first**, everywhere:
/// `map(xs, f)`, `filter(xs, p)`, `reduce(xs, f)`, `all(xs, p)`,
/// `apply(x, f)`, `pmap(xs, f)`, `find(xs, p)`. Two reasons, neither of
/// them taste:
///
///  1. It is what all of them except `map` already did, so it is the
///     smaller change to the language and to every script already written.
///  2. `recv.method(args)` means `method(recv, args)` throughout Qu, so
///     `xs.map(f)` only works if the collection is the first argument.
///     `apply` was already written this way for exactly that reason (see
///     its own doc comment); `map` was the one that wasn't.
///
/// `map` and `pmap` disagreeing with each other was the sharp edge:
/// `map(f, xs)` and `pmap(xs, f)`, same word, same job, opposite order.
///
/// The OTHER order is still accepted, because a language that breaks
/// working scripts to tidy a signature has made a bad trade -- and the
/// two arguments can always be told apart, since a function value (or the
/// name of one that exists) is never a collection. It warns once per
/// builtin per run, naming the fix, so a script converges instead of
/// silently keeping the old shape forever.
///
/// Returns `(function name, index of the data argument)`.
pub(crate) fn callable_pair(
    interp: &mut Interp,
    args: &[Value],
    data_idx: usize,
    fn_idx: usize,
    who: &str,
    verb: &str,
) -> R<(String, usize)> {
    if is_callable_arg(interp, args.get(fn_idx)) {
        let name = callable_arg(interp, args, fn_idx, who, verb)?;
        return Ok((name, data_idx));
    }
    if is_callable_arg(interp, args.get(data_idx)) {
        interp.warn_once(
            &format!("argorder:{who}"),
            &format!(
                "warning: `{who}` takes the collection first now -- write \
                 `{who}(xs, f)`, not `{who}(f, xs)`. The old order still works. \
                 Every other higher-order builtin already took the collection \
                 first, and `xs.{who}(f)` needs it there",
            ),
        );
        let name = callable_arg(interp, args, data_idx, who, verb)?;
        return Ok((name, fn_idx));
    }
    // Nothing here is callable, so this call is going to fail. Which slot
    // the complaint names still matters: `map("sin", xs)` is somebody
    // reaching for a builtin by its quoted name in the OLD argument order,
    // and telling them "needs a function, found vector" about the other
    // slot answers a question they did not ask. Report on whichever slot
    // holds a string, since that is the one they meant as the function.
    let blamed = if matches!(args.get(fn_idx), Some(Value::Str(_))) {
        fn_idx
    } else if matches!(args.get(data_idx), Some(Value::Str(_))) {
        data_idx
    } else {
        fn_idx
    };
    let name = callable_arg(interp, args, blamed, who, verb)?;
    Ok((name, if blamed == fn_idx { data_idx } else { fn_idx }))
}

/// `zip(a, b, ...)` — combine 2+ sequences elementwise. Qu has no tuple
/// type (`IMPL.md` flags this gap directly elsewhere), so each output row
/// is its own small `Value::List` (`[a[i], b[i], ...]`), not a tuple.
/// Mismatched lengths truncate to the shortest input (Python's own `zip`
/// convention) rather than erroring or padding with `Nothing`.
///
/// The return shape is deliberately UNIFORM regardless of argument count —
/// always a `List` of row-`List`s, for 2 args or 20 — never a `dict` for
/// exactly 2 args. An implicit arity-triggered type switch was explicitly
/// rejected while scoping this feature (this codebase's "flag it, don't
/// fake it" philosophy already avoids argument-count-triggered silent type
/// changes everywhere else). `dict(keys, values)` below is the explicit,
/// separate way to play Python's `dict(zip(...))` role.
pub fn zip_values(args: &[Value]) -> R<Value> {
    if args.len() < 2 {
        return e("zip needs at least 2 sequence arguments (a, b, ...)");
    }
    let mut cols: Vec<Vec<Value>> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let elems = collection_elems(a).map_err(|err| EvalError {
            msg: format!("zip: argument {} - {}", i + 1, err.msg),
        })?;
        cols.push(elems);
    }
    let n = cols.iter().map(Vec::len).min().unwrap_or(0);
    let rows: Vec<Value> = (0..n)
        .map(|i| Value::List(Arc::new(cols.iter().map(|c| c[i].clone()).collect())))
        .collect();
    Ok(Value::List(Arc::new(rows)))
}

/// `map(xs, f)` — the serial (non-parallel) counterpart to
/// `pmap(xs, f)`, for per-element work too cheap to
/// justify `pmap`'s thread/isolation overhead (fresh `Interp` clone +
/// dedicated RNG stream per element). A plain sequential loop calling
/// `fnName` once per element — no `rayon`, no per-element panic catching,
/// no env/funcs snapshot, since there's only ever one interpreter involved.
///
/// Argument order is `map(xs, f)` — the COLLECTION first, like every other
/// higher-order builtin in the language (§ argument order, 2026-09-09).
///
/// It used to be `map(f, xs)`, deliberately, to match Python's
/// `map(function, iterable)`. That was a bad trade and the reason is
/// visible one line up in this file's own history: `pmap(xs, f)` and
/// `map(f, xs)` are the same word doing the same job in opposite orders,
/// and nothing about a call tells a reader which one they are in. The
/// three things that decided it Qu's way rather than Python's:
///
///  * `recv.method(args)` means `method(recv, args)` here, so `xs.map(f)`
///    only exists if the collection is first.
///  * `|>` puts the piped value in the first slot, so `signals |> map(f)`
///    only composes if the collection is first — the exact reason `pmap`
///    was written this way to begin with.
///  * every other one of them (`fold`, `reduce`, `all`, `any`, `filter`,
///    `find`, `apply`, `pmap`) already was.
///
/// `map(f, xs)` still runs — see `callable_pair` for why, and for the
/// once-per-run notice that tells a script how to converge.
///
/// Accepts EITHER `Value::Vec` or `Value::List` (via `collection_elems`,
/// the same shared entry point `contains`/`indexof`/`find` use) — a real
/// superset of `pmap`, which only ever accepts a numeric `Vec`-like input
/// (`to_cow`) because every `pmap` worker's result must reduce to a plain
/// `f64`. `map` has no such restriction (it runs serially, in this same
/// interpreter, so an arbitrary `Value` result is fine), which is also why
/// it always returns a `Value::List` — never a narrower `Value::Vec` — even
/// when every result happens to be numeric: the return type must be
/// knowable without inspecting the function's actual results.
pub fn map_serial(interp: &mut Interp, args: &[Value]) -> R<Value> {
    if args.len() < 2 {
        return e("map(xs, f) needs a collection and a function");
    }
    let (fn_name, data_idx) = callable_pair(interp, args, 0, 1, "map", "map'd")?;
    let xs = &args[data_idx];
    let elems = collection_elems(xs)?;
    let mut out = Vec::with_capacity(elems.len());
    for elem in elems {
        out.push(interp.apply(&fn_name, vec![elem], Vec::new())?);
    }
    Ok(Value::List(Arc::new(out)))
}

/// `apply(x, f)` (§ signal builtins, 2026-09) — elementwise map that
/// PRESERVES `x`'s own container type, unlike `map(xs, f)` above (which
/// always returns a plain `Value::List`, discarding any `Fs`/type
/// information on purpose — see its own doc comment). Argument order is
/// `(x, f)` so the receiver lands in `args[0]` and the generic
/// `recv.method(args) == method(recv, args)` call sugar (`eval_call`) gives
/// `sig.apply(f)` for free. `map` disagreed with this until 2026-09-09 and
/// now matches it; this doc comment used to explain why they differed, and
/// the explanation did not survive contact with anyone using both.
///
/// Deliberately generalized to every container `map1` (this crate's
/// existing Rust-closure elementwise-preserving-type helper, used for
/// `sin`/`cos`/unary `-`/... on `Num`/`Bool`/`Vec`/`Mat`/`Signal`) already
/// covers, rather than staying `Signal`-only: this is the SAME "elementwise,
/// keep the container" shape the codebase already established for math
/// builtins, just with a named Qu function in place of a Rust closure, so
/// matching `map1`'s exact type coverage is the more consistent choice, not
/// scope creep. `Signal(xs, fs)` comes back as a NEW `Signal` at the SAME
/// `fs`; `Vec`/`Mat` similarly keep their own shape; a bare `Num`/`Bool`
/// applies the function once. Every other type (`List`, `Table`, `Str`, ...)
/// is a clear error naming the type — `map`/`pmap` already own the
/// arbitrary-`Value`-result, `List`-returning case, so `apply` doesn't need
/// to reach for it.
///
/// Each element's result is required to be numeric (`as_num`, matching how
/// `resample_to`/`sosfilt`/every other `Fs`-preserving builtin already
/// assumes a `Signal`'s samples stay real numbers) — a function returning
/// something else is a clear error citing the offending element, never a
/// silent `NaN`/coercion.
pub fn apply_named(interp: &mut Interp, args: &[Value]) -> R<Value> {
    if args.len() < 2 {
        return e("apply(x, f) needs a value and a function");
    }
    let (fn_name, data_idx) = callable_pair(interp, args, 0, 1, "apply", "apply'd")?;
    let v = args[data_idx].clone();
    let call_one = |interp: &mut Interp, x: f64| -> R<f64> {
        interp
            .apply(&fn_name, vec![Value::Num(x)], Vec::new())?
            .as_num()
            .map_err(|msg| EvalError {
                msg: format!("apply: function `{fn_name}` must return a number for every element - {msg}"),
            })
    };
    match v {
        Value::Num(x) => Ok(Value::Num(call_one(interp, x)?)),
        Value::Bool(b) => Ok(Value::Num(call_one(interp, if b { 1.0 } else { 0.0 })?)),
        Value::Vec(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for &x in xs.iter() {
                out.push(call_one(interp, x)?);
            }
            Ok(Value::Vec(Arc::new(out)))
        }
        Value::Signal(xs, fs) => {
            let mut out = Vec::with_capacity(xs.len());
            for &x in xs.iter() {
                out.push(call_one(interp, x)?);
            }
            Ok(Value::Signal(Arc::new(out), fs))
        }
        Value::Mat(m) => {
            let (rows, cols) = m.shape();
            let mut out = Vec::with_capacity(rows * cols);
            for &x in m.as_slice() {
                out.push(call_one(interp, x)?);
            }
            Ok(Value::Mat(Arc::new(qu_core::matrix::Matrix::from_col_major(rows, cols, out))))
        }
        other => e(format!(
            "apply(x, f): cannot apply a numeric function to {} (expected a num, bool, vec, matrix, or signal)",
            other.type_name()
        )),
    }
}

/// Normalize a dict key to `Value::Dict`'s internal `String` representation
/// (§47.4: "string and number keys"). Numbers are stringified via
/// `fmt_num` — the same canonical formatting `display_value` uses
/// elsewhere — so `1` and `1.0` collide on the same key, matching how a
/// script would expect numeric keys to behave. Deliberately NOT a generic
/// hashable-key system: any other value type (bool, list, dict, ...) has
/// no sensible canonical key spelling and is rejected outright rather than
/// silently coerced.
fn dict_key(v: &Value) -> R<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        Value::Num(n) => Ok(fmt_num(*n)),
        other => e(format!(
            "dict: keys must be strings or numbers, found {}",
            other.type_name()
        )),
    }
}

/// Shared `arg0`-as-dict extractor for `get`/`set`/`keys`/`values` below —
/// same "expected an X, found Y" error phrasing every other builtin in
/// this codebase uses for a type mismatch.
/// A `Dict` or a `Record`, which are the same shape.
///
/// `Value::Record` (`{a = 1, b = 2}`) and `Value::Dict` (`dict(k, v)`) are
/// both `Arc<Vec<(String, Value)>>`, and had completely disjoint access:
/// a record answered only to `.field`, a dict only to `get`/`keys`/
/// `values`. So `keys(r)` on a record -- the obvious way to ask what
/// fields a result has -- failed with "expected a dict, found record",
/// naming a type the script never mentioned and offering no way forward.
///
/// Reading is the same operation on both, so it now works on both.
/// Writing keeps the variant it was given (see `rebuild`), because the two
/// are not interchangeable everywhere yet -- `.field` access is a record's
/// alone -- and quietly handing back the other one would lose that.
fn as_pairs(v: &Value, who: &str) -> R<Arc<Vec<(String, Value)>>> {
    match v {
        Value::Dict(pairs) | Value::Record(pairs) => Ok(pairs.clone()),
        other => e(format!(
            "{who}: expected a dict or a record, found {}",
            other.type_name()
        )),
    }
}

/// Rebuild the same kind of container the caller passed in.
fn rebuild(like: &Value, pairs: Vec<(String, Value)>) -> Value {
    match like {
        Value::Record(_) => Value::Record(Arc::new(pairs)),
        _ => Value::Dict(Arc::new(pairs)),
    }
}

/// `dict()` / `dict(keys, values)` (§47.4) — `dict()` with no arguments
/// makes an empty dict (the spec's own bare `dict()` example); the
/// 2-argument form zips `keys`/`values` row-wise, explicitly playing the
/// role Python's `dict(zip(...))` idiom would — kept a separate, explicit
/// constructor rather than an arity-branch on `zip` itself (see
/// `zip_values`'s own doc comment for why an implicit 2-arg-means-dict
/// switch was rejected).
///
/// A `keys`/`values` length mismatch is a hard ERROR, not a truncation:
/// unlike `zip`'s own elementwise combine (where two independently-sized
/// streams truncating silently is the normal, expected case), a
/// keys/values length mismatch here is far more likely a genuine mistake
/// worth surfacing ("flag it, don't fake it") than a normal use pattern.
/// Repeated keys keep their FIRST position but take the LAST value —
/// ordinary "last write wins" dict semantics.
pub fn dict_new(args: &[Value]) -> R<Value> {
    if args.is_empty() {
        return Ok(Value::Dict(Arc::new(Vec::new())));
    }
    if args.len() != 2 {
        return e("dict(...) needs 0 arguments (empty dict) or 2 (keys, values)");
    }
    let keys = collection_elems(&args[0]).map_err(|err| EvalError {
        msg: format!("dict: keys - {}", err.msg),
    })?;
    let values = collection_elems(&args[1]).map_err(|err| EvalError {
        msg: format!("dict: values - {}", err.msg),
    })?;
    if keys.len() != values.len() {
        return e(format!(
            "dict(keys, values): keys has {} element(s) but values has {} — lengths must match",
            keys.len(),
            values.len()
        ));
    }
    let mut pairs: Vec<(String, Value)> = Vec::with_capacity(keys.len());
    for (k, v) in keys.into_iter().zip(values.into_iter()) {
        let key = dict_key(&k)?;
        match pairs.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, slot)) => *slot = v,
            None => pairs.push((key, v)),
        }
    }
    Ok(Value::Dict(Arc::new(pairs)))
}

/// `get(d, key)` — looks up `key` (string or number, normalized the same
/// way `dict`/`set` do), returning `Value::Nothing` if absent. Matches
/// `indexof`/`find`'s existing "missing means `Nothing`" convention, not a
/// thrown error — composes with `??` the same way (`get(d, "x") ?? 0`)
/// instead of forcing every lookup through `try`/`catch`.
pub fn dict_get(args: &[Value]) -> R<Value> {
    let d = as_pairs(arg0(args)?, "get")?;
    let key = dict_key(args.get(1).ok_or_else(|| EvalError {
        msg: "get(d, key) needs a key".into(),
    })?)?;
    Ok(d.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or(Value::Nothing))
}

/// `set(d, key, value)` — returns a NEW dict with `key` bound to `value`
/// (updating in place if `key` already existed, appended at the end
/// otherwise); `d` itself is never mutated. Matches every other compound
/// Qu value's "operations return a new value" convention (`List`'s own
/// `insert`/`append`/`remove` in `lib.rs` follow the identical rule).
pub fn dict_set(args: &[Value]) -> R<Value> {
    let like = arg0(args)?;
    let d = as_pairs(like, "set")?;
    let key = dict_key(args.get(1).ok_or_else(|| EvalError {
        msg: "set(d, key, value) needs a key".into(),
    })?)?;
    let value = args.get(2).cloned().ok_or_else(|| EvalError {
        msg: "set(d, key, value) needs a value".into(),
    })?;
    let mut pairs = d.as_ref().clone();
    match pairs.iter_mut().find(|(k, _)| *k == key) {
        Some((_, slot)) => *slot = value,
        None => pairs.push((key, value)),
    }
    Ok(rebuild(like, pairs))
}

/// `keys(d)` — every key, in insertion order, as a `List` of `Str`s.
pub fn dict_keys(args: &[Value]) -> R<Value> {
    let d = as_pairs(arg0(args)?, "keys")?;
    Ok(Value::List(Arc::new(
        d.iter().map(|(k, _)| Value::Str(k.clone())).collect(),
    )))
}

/// `values(d)` — every value, in the same order as `keys(d)`, as a `List`.
pub fn dict_values(args: &[Value]) -> R<Value> {
    let d = as_pairs(arg0(args)?, "values")?;
    Ok(Value::List(Arc::new(d.iter().map(|(_, v)| v.clone()).collect())))
}

// ---------------------------------------------------------------- sequences
//
// `Value::List` is what `split`, `zip`, `keys`/`values` and a `(a, b, c)`
// tuple literal all return, and it had the thinnest surface of any
// container in the language: `length`, `reverse`, `zip`, `contains`,
// `index_of`, `map` and `[i]`, and nothing else. `sum`, `sort`, `unique`
// and `count` all rejected it ("expected a numeric vector, found list"),
// `filter` was table-and-mask only, and there was no way to say "the first
// one" without writing `[0]`.
//
// So `"hello".split("").first()` -- which is how anyone would write it --
// failed with a suggestion list of `fit, fir1, hist, firls`. These fill
// that in.
//
// Argument order is RECEIVER-FIRST (`any(xs, "pred")`), not `map`'s
// function-first. That is deliberate and it is the majority convention
// here: `eval_call`'s generic `recv.method(args) == method(recv, args)`
// sugar means receiver-first is what makes `xs.any("big")` work at all,
// and `apply`/`pmap` already chose it for exactly that reason. `map` is
// the documented exception, matching Python's `map(f, xs)`.

/// The elements of any sequence, plus a name for the error message.
fn elems_of(v: &Value, who: &str) -> R<Vec<Value>> {
    collection_elems(v).map_err(|err| EvalError {
        msg: format!("{who}: {}", err.msg),
    })
}

/// `first(xs)` / `last(xs)` — the ends of a sequence.
///
/// Empty is `Nothing` rather than an error, matching `get`/`index_of`'s
/// existing "missing means `Nothing`" convention, so `first(xs) ?? 0`
/// composes instead of forcing a `try`/`catch` around a lookup.
pub fn seq_first(args: &[Value]) -> R<Value> {
    let xs = elems_of(arg0(args)?, "first")?;
    Ok(xs.into_iter().next().unwrap_or(Value::Nothing))
}

pub fn seq_last(args: &[Value]) -> R<Value> {
    let xs = elems_of(arg0(args)?, "last")?;
    Ok(xs.into_iter().next_back().unwrap_or(Value::Nothing))
}

/// `take(xs, n)` / `skip(xs, n)` — a prefix, and everything after one.
///
/// Both clamp rather than erroring: taking 10 from a 3-element list gives
/// the 3, and skipping 10 gives the empty list. That is what every
/// language with these does, and the alternative -- an error -- makes the
/// commonest use (`take(xs, 5)` for a preview) fail exactly when the data
/// is smaller than expected, which is when you least want it to.
pub fn seq_take(args: &[Value]) -> R<Value> {
    let xs = elems_of(arg0(args)?, "take")?;
    let n = count_arg(args, "take")?;
    Ok(Value::List(Arc::new(xs.into_iter().take(n).collect())))
}

pub fn seq_drop(args: &[Value]) -> R<Value> {
    let xs = elems_of(arg0(args)?, "drop")?;
    let n = count_arg(args, "drop")?;
    Ok(Value::List(Arc::new(xs.into_iter().skip(n).collect())))
}

fn count_arg(args: &[Value], who: &str) -> R<usize> {
    let n = args
        .get(1)
        .ok_or_else(|| EvalError { msg: format!("{who}(xs, n) needs a count") })?
        .as_num()
        .map_err(|msg| EvalError { msg: format!("{who}: {msg}") })?;
    if n < 0.0 || n.fract() != 0.0 {
        return e(format!("{who}: the count must be a whole number, got {}", fmt_num(n)));
    }
    Ok(n as usize)
}

/// `distinct(xs)` — the elements of `xs`, first occurrence kept.
///
/// `unique` already does this for a numeric vector and additionally SORTS
/// (MATLAB's own behaviour). This one keeps order and works on anything
/// `values_equal` can compare, which is the LINQ/`Distinct` meaning and
/// the one you want for a list of strings.
pub fn seq_distinct(args: &[Value]) -> R<Value> {
    let xs = elems_of(arg0(args)?, "distinct")?;
    let mut out: Vec<Value> = Vec::new();
    for x in xs {
        if !out.iter().any(|seen| crate::values_equal(seen, &x)) {
            out.push(x);
        }
    }
    Ok(Value::List(Arc::new(out)))
}

/// `flatten(xs)` — one level of nesting removed, which is the level `zip`
/// and `group_by`-shaped results actually add. Deliberately NOT recursive:
/// a fully-recursive flatten on a list of lists of lists is rarely what
/// was meant, and `flatten(flatten(x))` says the other thing explicitly.
pub fn seq_flatten(args: &[Value]) -> R<Value> {
    // A matrix flattens to a VECTOR, not a list: the result is numeric and
    // the caller almost always wants to keep doing maths with it.
    //
    // Column-major, which is `A(:)` in MATLAB and matches how `as
    // matrix(r, c)` FILLS -- `[1,2,3,4] as matrix(2,2)` is `[1 3; 2 4]`,
    // so flattening it must give back `[1,2,3,4]` rather than `[1,3,2,4]`.
    // Round-tripping is the property worth having: flatten then reshape is
    // the identity.
    if let Value::Mat(m) = arg0(args)? {
        return Ok(Value::Vec(Arc::new(m.as_slice().to_vec())));
    }
    let xs = elems_of(arg0(args)?, "flatten")?;
    let mut out = Vec::new();
    for x in xs {
        match collection_elems(&x) {
            Ok(inner) => out.extend(inner),
            Err(_) => out.push(x),
        }
    }
    Ok(Value::List(Arc::new(out)))
}

/// `items(d)` — a dict or record as a list of `(key, value)` pairs, so it
/// can be walked with `for` the way `keys`/`values` already allow one side
/// of it to be.
pub fn dict_items(args: &[Value]) -> R<Value> {
    let d = as_pairs(arg0(args)?, "items")?;
    Ok(Value::List(Arc::new(
        d.iter()
            .map(|(k, v)| Value::List(Arc::new(vec![Value::Str(k.clone()), v.clone()])))
            .collect(),
    )))
}

/// `has_key(d, key)` — whether a dict or record has that key at all.
///
/// Distinct from `get(d, k) != none`, which cannot tell a missing key from
/// one whose value IS `none`, and distinct from `contains`, which searches
/// VALUES. Calling this `contains` would have made one name mean both.
pub fn dict_has_key(args: &[Value]) -> R<Value> {
    let d = as_pairs(arg0(args)?, "has_key")?;
    let key = dict_key(args.get(1).ok_or_else(|| EvalError {
        msg: "has_key(d, key) needs a key".into(),
    })?)?;
    Ok(Value::Bool(d.iter().any(|(k, _)| *k == key)))
}

/// Call a user-defined predicate once per element.
///
/// Builtins are refused for the same reason `map` refuses them: `apply`
/// dispatches on user functions only, so naming a builtin here would fail
/// deeper in with a worse message.
fn predicate<'a>(interp: &'a mut Interp, args: &[Value], who: &str) -> R<(Vec<Value>, String)> {
    if args.len() < 2 {
        return e(format!(
            "{who}(xs, predicate) needs a function -- a name you defined, or a function value"
        ));
    }
    let (fn_name, data_idx) = callable_pair(interp, args, 0, 1, who, "used as a predicate")?;
    let xs = elems_of(&args[data_idx], who)?;
    Ok((xs, fn_name))
}

fn truthy_result(v: &Value, who: &str, fn_name: &str) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Num(n) => Ok(*n != 0.0),
        other => e(format!(
            "{who}: `{fn_name}` must return true or false for every element, got {}",
            other.type_name()
        )),
    }
}

/// `where(xs, "pred")` — the elements a predicate says yes to.
///
/// Named `where` rather than `filter` because `filter` is already taken,
/// and by something genuinely different: `filter(table, mask)` selects
/// table rows by a precomputed boolean mask. Overloading one name across
/// "rows by mask" and "elements by predicate" would make the call's
/// meaning depend on its argument types, which is what this codebase
/// avoids elsewhere.
pub fn seq_where(interp: &mut Interp, args: &[Value]) -> R<Value> {
    let (xs, f) = predicate(interp, args, "where")?;
    let mut out = Vec::new();
    for x in xs {
        let keep = interp.apply(&f, vec![x.clone()], Vec::new())?;
        if truthy_result(&keep, "where", &f)? {
            out.push(x);
        }
    }
    Ok(Value::List(Arc::new(out)))
}

/// `any(xs, "pred")` / `all(xs, "pred")`.
///
/// Both short-circuit. On an EMPTY sequence `any` is false and `all` is
/// true -- the standard vacuous-truth reading, and the one that keeps
/// `all(xs, p) == !any(xs, not_p)` honest.
pub fn seq_any(interp: &mut Interp, args: &[Value]) -> R<Value> {
    let (xs, f) = predicate(interp, args, "any")?;
    for x in xs {
        let hit = interp.apply(&f, vec![x], Vec::new())?;
        if truthy_result(&hit, "any", &f)? {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

pub fn seq_all(interp: &mut Interp, args: &[Value]) -> R<Value> {
    let (xs, f) = predicate(interp, args, "all")?;
    for x in xs {
        let hit = interp.apply(&f, vec![x], Vec::new())?;
        if !truthy_result(&hit, "all", &f)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

/// `reduce(xs, "fn")` / `reduce(xs, "fn", initial)` — fold left.
///
/// Without an initial value the first element seeds the accumulator, and
/// an empty sequence is then `none` rather than an error, matching
/// `first`/`last`. With one, an empty sequence is that initial value,
/// which is what makes `reduce(xs, "add", 0)` safe on data that might be
/// empty.
pub fn seq_reduce(interp: &mut Interp, args: &[Value], who: &str) -> R<Value> {
    // `who` is the word the script actually wrote -- `fold` and `reduce`
    // share this body, and a notice about argument order that names the
    // other one is a notice about somebody else's call.
    let (xs, f) = predicate(interp, args, who)?;
    let mut it = xs.into_iter();
    let mut acc = match args.get(2) {
        Some(init) => init.clone(),
        None => match it.next() {
            Some(first) => first,
            None => return Ok(Value::Nothing),
        },
    };
    for x in it {
        acc = interp.apply(&f, vec![acc, x], Vec::new())?;
    }
    Ok(acc)
}
