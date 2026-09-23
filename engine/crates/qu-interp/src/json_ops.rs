//! § JSON dotted-path access (2026-09-23) — `toolkit-file.md` §2's
//! `json.get("user.name")`/`json.set`/`json.delete`, confirmed genuinely
//! missing by the toolkit audit: `parse_json`/`jsonify` exist, but nothing
//! reaches into a JSON document by a dotted path without a full parse into
//! a Qu value and manual field/index walking first.
//!
//! **Deliberately operates on the raw JSON text, not on `parse_json`'s
//! result.** `parse_json` does content-shape inference (a same-shaped
//! array of objects becomes a `Table`, a numeric array becomes a `Vec`)
//! specifically so a script gets the language's own ergonomic types back
//! — exactly the wrong property for a dotted-path walker, which needs one
//! predictable tree shape to walk regardless of what the data happens to
//! look like. `json_get`/`json_set`/`json_delete` each re-parse the string
//! fresh with `serde_json` directly, walk the RAW JSON tree, and only
//! convert to a `Value` at the one leaf `json_get` returns — using a
//! simple, non-heuristic mapping (object -> `Dict`, array -> `List`,
//! never a `Table`), so the same path always means the same thing.
//!
//! **Path syntax**: dot-separated segments; a segment that parses as a
//! whole number is an array index, everything else is an object key --
//! `"items.0.name"` is `items` (array) -> index 0 -> `name` (object key).
//! No escaping for a literal `.` in a key name; none of this project's
//! own JSON documents need one, and adding escape syntax for a case that
//! hasn't come up is exactly the kind of complexity this module's own
//! existence is meant to avoid, not add.
//!
//! **`json_set` auto-creates missing OBJECT segments** on the way down
//! (so `json_set("{}", "a.b.c", 1)` produces `{"a":{"b":{"c":1}}}`), but
//! never auto-creates or grows an ARRAY -- indexing past an array's
//! current length, or expecting an array where an object (or nothing) is
//! there, is a clear error instead of a guessed-at empty-fill, matching
//! this codebase's `pack`/`unpack` stance on "a wrong answer that looks
//! right is worse than an error."
//!
//! **`json_delete` on an absent path is a no-op**, returning the document
//! unchanged -- matching how `dict.pop`/JS `delete` treat removing
//! something that was never there. A path whose PARENT doesn't resolve to
//! an object/array at all is still an error; only the final segment being
//! absent is forgiven.

use crate::{e, text_arg, EvalError, Value, R};

pub fn call(f: &str, args: &[Value], _style: &[(String, Value)]) -> R<Value> {
    match f {
        "json_get" => json_get(args),
        "json_set" => json_set(args),
        "json_delete" => json_delete(args),
        other => e(format!("json_ops: unknown function `{other}`")),
    }
}

fn parse(s: &str, who: &str) -> R<serde_json::Value> {
    serde_json::from_str(s).map_err(|err| EvalError {
        msg: format!("{who}: not valid JSON: {err}"),
    })
}

/// A dotted path split into segments; an all-digit segment is an array
/// index, everything else an object key. See this module's own doc
/// comment for the full syntax.
enum Seg {
    Key(String),
    Index(usize),
}

fn split_path(path: &str) -> Vec<Seg> {
    path.split('.')
        .filter(|s| !s.is_empty())
        .map(|s| match s.parse::<usize>() {
            Ok(i) => Seg::Index(i),
            Err(_) => Seg::Key(s.to_string()),
        })
        .collect()
}

/// The simple, non-heuristic JSON -> `Value` mapping `json_get` uses for
/// its one returned leaf -- deliberately NOT `parse_json`'s own converter,
/// see this module's doc comment for why.
fn simple_to_value(j: &serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Value::Nothing,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
        J::String(s) => Value::Str(s.clone()),
        J::Array(items) => Value::List(std::sync::Arc::new(items.iter().map(simple_to_value).collect())),
        J::Object(map) => Value::Dict(std::sync::Arc::new(
            map.iter().map(|(k, v)| (k.clone(), simple_to_value(v))).collect(),
        )),
    }
}

/// The inverse, for `json_set`'s new leaf value -- a Qu `Value` written
/// into the JSON tree. `Table`/`Signal`/etc have no JSON shape and are a
/// clear error rather than a silently-wrong `{}`.
fn value_to_json(v: &Value) -> R<serde_json::Value> {
    use serde_json::Value as J;
    Ok(match v {
        Value::Nothing => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Num(n) => serde_json::Number::from_f64(*n).map(J::Number).unwrap_or(J::Null),
        Value::Str(s) => J::String(s.clone()),
        Value::Vec(xs) => J::Array(xs.iter().map(|x| J::Number(serde_json::Number::from_f64(*x).unwrap_or(0.into()))).collect()),
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items.iter() {
                out.push(value_to_json(it)?);
            }
            J::Array(out)
        }
        Value::Dict(pairs) | Value::Record(pairs) => {
            let mut map = serde_json::Map::with_capacity(pairs.len());
            for (k, val) in pairs.iter() {
                map.insert(k.clone(), value_to_json(val)?);
            }
            J::Object(map)
        }
        other => return e(format!(
            "json_set: a {} has no JSON representation -- expected a number, string, boolean, \
             nothing, vector, list, dict or record",
            other.type_name()
        )),
    })
}

/// `json_get(json_str, path)` — the value at `path`, or an error naming
/// exactly where the path stopped resolving (which segment, and what was
/// found there instead).
fn json_get(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let path = text_arg(args, 1)?;
    let root = parse(&s, "json_get")?;
    let segs = split_path(&path);
    let mut cur = &root;
    let mut walked = String::new();
    for seg in &segs {
        match seg {
            Seg::Key(k) => {
                let obj = cur.as_object().ok_or_else(|| EvalError {
                    msg: format!(
                        "json_get: `{path}` -- at `{walked}` found {}, not an object, so `.{k}` doesn't apply",
                        json_kind(cur)
                    ),
                })?;
                cur = obj.get(k).ok_or_else(|| EvalError {
                    msg: format!("json_get: `{path}` -- no key `{k}` at `{walked}`"),
                })?;
                walked = if walked.is_empty() { k.clone() } else { format!("{walked}.{k}") };
            }
            Seg::Index(i) => {
                let arr = cur.as_array().ok_or_else(|| EvalError {
                    msg: format!(
                        "json_get: `{path}` -- at `{walked}` found {}, not an array, so `.{i}` doesn't apply",
                        json_kind(cur)
                    ),
                })?;
                cur = arr.get(*i).ok_or_else(|| EvalError {
                    msg: format!("json_get: `{path}` -- index {i} is out of range at `{walked}` ({} elements)", arr.len()),
                })?;
                walked = format!("{walked}.{i}");
            }
        }
    }
    Ok(simple_to_value(cur))
}

fn json_kind(j: &serde_json::Value) -> &'static str {
    match j {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// `json_set(json_str, path, value)` — a new JSON string with `path` set
/// to `value`, creating missing OBJECT segments on the way down. Returns
/// the whole document, re-serialized -- `json_set` is not in-place, same
/// as every other Qu value operation.
fn json_set(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let path = text_arg(args, 1)?;
    let new_val = args.get(2).ok_or_else(|| EvalError {
        msg: "json_set(json, path, value): needs a third argument, the value to set".to_string(),
    })?;
    let mut root = parse(&s, "json_set")?;
    let segs = split_path(&path);
    if segs.is_empty() {
        return e("json_set: the path is empty".to_string());
    }
    set_at(&mut root, &segs, value_to_json(new_val)?, &path)?;
    Ok(Value::Str(root.to_string()))
}

fn set_at(cur: &mut serde_json::Value, segs: &[Seg], new_val: serde_json::Value, whole_path: &str) -> R<()> {
    use serde_json::Value as J;
    match &segs[0] {
        Seg::Key(k) => {
            // Auto-vivify: `null`/missing becomes `{}` here, but anything
            // else that isn't already an object is a real structural
            // conflict, not silently overwritten.
            if cur.is_null() {
                *cur = J::Object(serde_json::Map::new());
            }
            let kind = json_kind(cur);
            let obj = cur.as_object_mut().ok_or_else(|| EvalError {
                msg: format!("json_set: `{whole_path}` -- can't set key `{k}` on {kind}"),
            })?;
            if segs.len() == 1 {
                obj.insert(k.clone(), new_val);
            } else {
                let entry = obj.entry(k.clone()).or_insert(J::Null);
                set_at(entry, &segs[1..], new_val, whole_path)?;
            }
        }
        Seg::Index(i) => {
            let kind = json_kind(cur);
            let arr = cur.as_array_mut().ok_or_else(|| EvalError {
                msg: format!(
                    "json_set: `{whole_path}` -- can't index into {kind} at `.{i}` -- \
                     json_set does not create or grow arrays, only objects"
                ),
            })?;
            if *i >= arr.len() {
                return e(format!(
                    "json_set: `{whole_path}` -- index {i} is out of range ({} elements) -- \
                     json_set does not grow arrays",
                    arr.len()
                ));
            }
            if segs.len() == 1 {
                arr[*i] = new_val;
            } else {
                set_at(&mut arr[*i], &segs[1..], new_val, whole_path)?;
            }
        }
    }
    Ok(())
}

/// `json_delete(json_str, path)` — a new JSON string with the key/index at
/// `path` removed. A path whose final segment is already absent is a
/// no-op (see this module's own doc comment); a path whose PARENT doesn't
/// resolve at all is still an error.
fn json_delete(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let path = text_arg(args, 1)?;
    let mut root = parse(&s, "json_delete")?;
    let segs = split_path(&path);
    if segs.is_empty() {
        return e("json_delete: the path is empty".to_string());
    }
    delete_at(&mut root, &segs, &path)?;
    Ok(Value::Str(root.to_string()))
}

fn delete_at(cur: &mut serde_json::Value, segs: &[Seg], whole_path: &str) -> R<()> {
    if segs.len() == 1 {
        match &segs[0] {
            Seg::Key(k) => {
                if let Some(obj) = cur.as_object_mut() {
                    obj.remove(k);
                } else {
                    return e(format!(
                        "json_delete: `{whole_path}` -- can't delete key `{k}` from {}",
                        json_kind(cur)
                    ));
                }
            }
            Seg::Index(i) => {
                if let Some(arr) = cur.as_array_mut() {
                    if *i < arr.len() {
                        arr.remove(*i);
                    }
                } else {
                    return e(format!(
                        "json_delete: `{whole_path}` -- can't delete index {i} from {}",
                        json_kind(cur)
                    ));
                }
            }
        }
        return Ok(());
    }
    match &segs[0] {
        Seg::Key(k) => {
            let kind = json_kind(cur);
            let obj = cur.as_object_mut().ok_or_else(|| EvalError {
                msg: format!("json_delete: `{whole_path}` -- at `{k}` found {kind}, not an object"),
            })?;
            match obj.get_mut(k) {
                Some(next) => delete_at(next, &segs[1..], whole_path),
                None => Ok(()), // parent segment absent -> nothing to delete, matches the no-op contract
            }
        }
        Seg::Index(i) => {
            let kind = json_kind(cur);
            let arr = cur.as_array_mut().ok_or_else(|| EvalError {
                msg: format!("json_delete: `{whole_path}` -- at `.{i}` found {kind}, not an array"),
            })?;
            match arr.get_mut(*i) {
                Some(next) => delete_at(next, &segs[1..], whole_path),
                None => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: R<Value>) -> String {
        match v.unwrap() {
            Value::Str(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    #[test]
    fn get_walks_nested_objects_and_arrays() {
        let doc = r#"{"user":{"name":"Ada","tags":["x","y"]}}"#;
        let a = [Value::Str(doc.to_string()), Value::Str("user.name".to_string())];
        assert!(matches!(json_get(&a).unwrap(), Value::Str(n) if n == "Ada"));
        let a2 = [Value::Str(doc.to_string()), Value::Str("user.tags.1".to_string())];
        assert!(matches!(json_get(&a2).unwrap(), Value::Str(n) if n == "y"));
    }

    #[test]
    fn get_on_a_missing_key_names_where_it_stopped() {
        let doc = r#"{"a":{}}"#;
        let a = [Value::Str(doc.to_string()), Value::Str("a.b.c".to_string())];
        let err = json_get(&a).unwrap_err();
        assert!(err.msg.contains("no key `b`"), "got: {}", err.msg);
    }

    #[test]
    fn set_creates_missing_object_segments() {
        let a = [
            Value::Str("{}".to_string()),
            Value::Str("a.b.c".to_string()),
            Value::Num(1.0),
        ];
        let out = s(json_set(&a));
        assert_eq!(out, r#"{"a":{"b":{"c":1.0}}}"#);
    }

    #[test]
    fn set_refuses_to_grow_an_array() {
        let a = [
            Value::Str(r#"{"items":[]}"#.to_string()),
            Value::Str("items.0".to_string()),
            Value::Num(1.0),
        ];
        let err = json_set(&a).unwrap_err();
        assert!(err.msg.contains("does not grow arrays"), "got: {}", err.msg);
    }

    #[test]
    fn delete_removes_a_key() {
        let a = [Value::Str(r#"{"a":1,"b":2}"#.to_string()), Value::Str("a".to_string())];
        let out = s(json_delete(&a));
        assert_eq!(out, r#"{"b":2}"#);
    }

    #[test]
    fn delete_of_an_absent_path_is_a_no_op() {
        let a = [Value::Str(r#"{"a":1}"#.to_string()), Value::Str("x.y".to_string())];
        let out = s(json_delete(&a));
        assert_eq!(out, r#"{"a":1}"#);
    }
}
