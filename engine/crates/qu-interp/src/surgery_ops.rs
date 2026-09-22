//! § text surgery: absolute positions, lines, and offset-safe edits (2026-09-16).
//!
//! `docs/design/toolkit-text.md` §3.2/§3.3. Find-and-replace already exists;
//! what did not is the ability to say **where** — "the 14 characters at
//! offset 203", "line 7", "from line 3 to line 9" — and to make several such
//! changes at once without the earlier ones moving the later ones.
//!
//! ## Two coordinate systems, deliberately named apart
//!
//! * **Position** is an absolute offset in **codepoints** from the start of
//!   the text, 0-based, exactly like every other index in Qu. `pos_of`
//!   converts a line/column to one; `line_col` converts back.
//! * **Line number** is **1-based**, because that is what the rest of this
//!   system already prints: a parse error says `at 2:13` and means the
//!   second line. A `line_at(s, 1)` that returned the second line would be
//!   the same class of defect this toolkit work keeps finding — a label that
//!   is true and a value that is not.
//!
//! The two bases are a real inconsistency. It is chosen over the two
//! alternatives, both worse: 0-based lines that disagree with every error
//! message the engine emits, or 1-based positions that disagree with every
//! other index in the language. The names differ (`pos` vs `line`) so the
//! reader is told which one they are holding.
//!
//! ## Why `splice` and `apply_edits` are separate
//!
//! `splice` is one cut. `apply_edits` is several, and it exists because the
//! obvious loop is wrong: replacing at position 10 and then at position 50
//! works, but doing it in the other order — or changing a length along the
//! way — silently shifts every later position. `apply_edits` sorts, checks
//! for overlap, refuses rather than guessing, and applies in one pass
//! against the ORIGINAL coordinates. Every position you pass it means what
//! it meant when you measured it.

use crate::{e, style_str, text_arg, Value, R};

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "line_count" => line_count(args),
        "line_at" => line_at(args),
        "line_range" => line_range(args),
        "line_set" => line_set(args),
        "line_insert" => line_insert(args, style),
        "line_delete" => line_delete(args),
        "pos_of" => pos_of(args),
        "line_col" => line_col(args),
        "slice_at" => slice_at(args),
        "splice" => splice(args),
        "apply_edits" => apply_edits(args),
        other => e(format!("surgery_ops: unknown function `{other}`")),
    }
}

// ------------------------------------------------------------- helpers

/// Split preserving the information `str::lines` throws away: whether the
/// text ended with a newline. `"a\nb"` and `"a\nb\n"` are two lines either
/// way, but only one of them should get a trailing newline back when the
/// lines are rejoined.
fn split_lines(s: &str) -> (Vec<String>, bool) {
    let trailing = s.ends_with('\n');
    let body = if trailing { &s[..s.len() - 1] } else { s };
    if body.is_empty() && trailing {
        return (vec![String::new()], true);
    }
    if body.is_empty() {
        return (Vec::new(), false);
    }
    (body.split('\n').map(|l| l.to_string()).collect(), trailing)
}

fn join_lines(lines: &[String], trailing: bool) -> String {
    let mut s = lines.join("\n");
    if trailing && !lines.is_empty() {
        s.push('\n');
    }
    s
}

fn int_of(args: &[Value], i: usize, who: &str, what: &str) -> R<i64> {
    match args.get(i) {
        Some(Value::Num(n)) => {
            if n.fract() != 0.0 || !n.is_finite() {
                return e(format!("{who}: {what} must be a whole number, got `{n}`"));
            }
            Ok(*n as i64)
        }
        Some(other) => e(format!(
            "{who}: {what} must be a number, got {}",
            other.type_name()
        )),
        None => e(format!("{who}: missing {what}")),
    }
}

/// A 1-based line number, validated against the actual line count so the
/// error names both the request and the range rather than clamping.
fn line_index(n: i64, count: usize, who: &str) -> R<usize> {
    if n < 1 {
        return e(format!(
            "{who}: line {n} -- line numbers are 1-based, matching the `line:col` \
             the engine prints in errors"
        ));
    }
    if n as usize > count {
        return e(format!(
            "{who}: line {n} is past the end -- the text has {count} line{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    Ok(n as usize - 1)
}

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

// --------------------------------------------------------------- lines

fn line_count(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let (lines, _) = split_lines(&s);
    Ok(Value::Num(lines.len() as f64))
}

fn line_at(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let n = int_of(args, 1, "line_at", "the line number")?;
    let (lines, _) = split_lines(&s);
    let i = line_index(n, lines.len(), "line_at")?;
    Ok(Value::Str(lines[i].clone()))
}

/// Lines `a` through `b`, **inclusive** at both ends — `line_range(s, 3, 5)`
/// is three lines, matching how a person says "lines 3 to 5" and how Qu's
/// own `3 to 5` range reads.
fn line_range(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let a = int_of(args, 1, "line_range", "the first line number")?;
    let b = int_of(args, 2, "line_range", "the last line number")?;
    let (lines, trailing) = split_lines(&s);
    let i = line_index(a, lines.len(), "line_range")?;
    let j = line_index(b, lines.len(), "line_range")?;
    if i > j {
        return e(format!(
            "line_range: first line {a} is after last line {b} -- the range is \
             inclusive and ordered, and an empty result here is more likely a \
             swapped argument than an intent"
        ));
    }
    Ok(Value::Str(join_lines(&lines[i..=j], trailing && j + 1 == lines.len())))
}

fn line_set(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let n = int_of(args, 1, "line_set", "the line number")?;
    let new = text_arg(args, 2)?;
    let (mut lines, trailing) = split_lines(&s);
    let i = line_index(n, lines.len(), "line_set")?;
    lines[i] = new;
    Ok(Value::Str(join_lines(&lines, trailing)))
}

/// Insert before line `n`. `where="after"` inserts after it instead, and
/// `n = line_count + 1` appends — the one position past the end is legal
/// here precisely because appending is the common case.
fn line_insert(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let n = int_of(args, 1, "line_insert", "the line number")?;
    let new = text_arg(args, 2)?;
    let (mut lines, trailing) = split_lines(&s);
    let after = match style_str(style, "where") {
        None => false,
        Some(w) => match w.as_str() {
            "before" => false,
            "after" => true,
            other => {
                return e(format!(
                    "line_insert: `where=\"{other}\"` -- use \"before\" (the default) or \"after\""
                ))
            }
        },
    };
    if n < 1 {
        return e(format!("line_insert: line {n} -- line numbers are 1-based"));
    }
    let at = if after { n as usize } else { n as usize - 1 };
    if at > lines.len() {
        return e(format!(
            "line_insert: line {n} is more than one past the end -- the text has {} lines, \
             so the largest insertable line number is {}",
            lines.len(),
            lines.len() + 1
        ));
    }
    for (k, part) in new.split('\n').enumerate() {
        lines.insert(at + k, part.to_string());
    }
    Ok(Value::Str(join_lines(&lines, trailing)))
}

fn line_delete(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let n = int_of(args, 1, "line_delete", "the line number")?;
    let count = match args.get(2) {
        None => 1i64,
        Some(_) => int_of(args, 2, "line_delete", "the line count")?,
    };
    if count < 0 {
        return e(format!("line_delete: count {count} must be >= 0"));
    }
    let (mut lines, trailing) = split_lines(&s);
    let i = line_index(n, lines.len(), "line_delete")?;
    let end = (i + count as usize).min(lines.len());
    lines.drain(i..end);
    Ok(Value::Str(join_lines(&lines, trailing && !lines.is_empty())))
}

// ----------------------------------------------------------- positions

/// Absolute 0-based codepoint offset of a 1-based `line` and 1-based `col`.
/// The inverse of `line_col`, and the bridge between an error message's
/// `line:col` and a position this module's cutting verbs accept.
fn pos_of(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let ln = int_of(args, 1, "pos_of", "the line number")?;
    let col = match args.get(2) {
        None => 1i64,
        Some(_) => int_of(args, 2, "pos_of", "the column")?,
    };
    if col < 1 {
        return e(format!("pos_of: column {col} -- columns are 1-based"));
    }
    let (lines, _) = split_lines(&s);
    let i = line_index(ln, lines.len(), "pos_of")?;
    let mut pos = 0usize;
    for l in lines.iter().take(i) {
        pos += l.chars().count() + 1; // +1 for the newline
    }
    let width = lines[i].chars().count();
    if col as usize > width + 1 {
        return e(format!(
            "pos_of: column {col} is past the end of line {ln}, which has {width} character{}",
            if width == 1 { "" } else { "s" }
        ));
    }
    Ok(Value::Num((pos + col as usize - 1) as f64))
}

fn line_col(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let pos = int_of(args, 1, "line_col", "the position")?;
    if pos < 0 {
        return e(format!("line_col: position {pos} must be >= 0"));
    }
    let cs = chars_of(&s);
    if pos as usize > cs.len() {
        return e(format!(
            "line_col: position {pos} is past the end -- the text has {} characters",
            cs.len()
        ));
    }
    let mut line = 1usize;
    let mut col = 1usize;
    for c in cs.iter().take(pos as usize) {
        if *c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    Ok(Value::Record(std::sync::Arc::new(vec![
        ("line".to_string(), Value::Num(line as f64)),
        ("col".to_string(), Value::Num(col as f64)),
        ("pos".to_string(), Value::Num(pos as f64)),
    ])))
}

fn slice_at(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let pos = int_of(args, 1, "slice_at", "the position")?;
    let len = int_of(args, 2, "slice_at", "the length")?;
    let cs = chars_of(&s);
    let (a, b) = span_bounds(pos, len, cs.len(), "slice_at")?;
    Ok(Value::Str(cs[a..b].iter().collect()))
}

fn span_bounds(pos: i64, len: i64, total: usize, who: &str) -> R<(usize, usize)> {
    if pos < 0 {
        return e(format!("{who}: position {pos} must be >= 0"));
    }
    if len < 0 {
        return e(format!("{who}: length {len} must be >= 0"));
    }
    let a = pos as usize;
    if a > total {
        return e(format!(
            "{who}: position {pos} is past the end -- the text has {total} characters"
        ));
    }
    let b = a + len as usize;
    if b > total {
        return e(format!(
            "{who}: the span at {pos} of length {len} runs {} character{} past the end \
             of {total} -- a truncated span is a silently different edit, so this is an error",
            b - total,
            if b - total == 1 { "" } else { "s" }
        ));
    }
    Ok((a, b))
}

/// The one surgical cut: replace `len` characters at `pos` with `new`.
/// `len = 0` inserts; `new = ""` deletes.
fn splice(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let pos = int_of(args, 1, "splice", "the position")?;
    let len = int_of(args, 2, "splice", "the length")?;
    let new = match args.get(3) {
        None => String::new(),
        Some(_) => text_arg(args, 3)?,
    };
    let cs = chars_of(&s);
    let (a, b) = span_bounds(pos, len, cs.len(), "splice")?;
    let mut out: String = cs[..a].iter().collect();
    out.push_str(&new);
    out.extend(cs[b..].iter());
    Ok(Value::Str(out))
}

/// Several cuts at once, all against the ORIGINAL positions.
///
/// Takes a list of `[pos, len, text]` triples. Overlapping spans are an
/// error rather than a resolution, because there is no order in which two
/// overlapping edits both mean what they said. This is the whole reason the
/// function exists: the hand-written loop over `splice` is correct only when
/// the edits happen to be applied back-to-front, and nothing warns when they
/// are not.
fn apply_edits(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let cs = chars_of(&s);
    let list = match args.get(1) {
        Some(Value::List(xs)) => xs.to_vec(),
        // An empty `[]` literal is an empty numeric vector, not a list --
        // so "no edits" arrives here as a Vec and must mean no edits rather
        // than a type error. A NON-empty vector is still refused below,
        // since a flat list of numbers is not a list of [pos, len, text].
        Some(Value::Vec(v)) if v.is_empty() => Vec::new(),
        Some(other) => {
            return e(format!(
                "apply_edits: expected a list of [pos, len, text] edits, got {}",
                other.type_name()
            ))
        }
        None => return e("apply_edits: expected a list of [pos, len, text] edits".to_string()),
    };

    let mut edits: Vec<(usize, usize, String)> = Vec::with_capacity(list.len());
    for (k, item) in list.iter().enumerate() {
        let parts = match item {
            Value::List(p) => p.to_vec(),
            Value::Vec(v) => v.iter().map(|x| Value::Num(*x)).collect(),
            other => {
                return e(format!(
                    "apply_edits: edit {k} is {}, expected [pos, len, text]",
                    other.type_name()
                ))
            }
        };
        if parts.len() < 2 || parts.len() > 3 {
            return e(format!(
                "apply_edits: edit {k} has {} element{}, expected [pos, len, text] \
                 (text may be omitted to delete)",
                parts.len(),
                if parts.len() == 1 { "" } else { "s" }
            ));
        }
        let pos = int_of(&parts, 0, "apply_edits", &format!("edit {k}'s position"))?;
        let len = int_of(&parts, 1, "apply_edits", &format!("edit {k}'s length"))?;
        let text = match parts.get(2) {
            None => String::new(),
            Some(Value::Str(t)) => t.clone(),
            Some(other) => {
                return e(format!(
                    "apply_edits: edit {k}'s replacement is {}, expected text",
                    other.type_name()
                ))
            }
        };
        let (a, b) = span_bounds(pos, len, cs.len(), &format!("apply_edits (edit {k})"))?;
        edits.push((a, b, text));
    }

    edits.sort_by_key(|(a, b, _)| (*a, *b));
    for w in edits.windows(2) {
        let (a0, b0, _) = &w[0];
        let (a1, _, _) = &w[1];
        // Touching is fine (b0 == a1); genuinely overlapping is not. Two
        // pure insertions at the same point are also refused, because their
        // relative order is exactly what the caller has not said.
        if a1 < b0 || (a0 == a1 && b0 == a0) {
            return e(format!(
                "apply_edits: the edit at {a0}..{b0} overlaps the one at {a1} -- \
                 there is no order in which both mean what they said, so this is \
                 an error rather than a silent choice"
            ));
        }
    }

    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    for (a, b, text) in &edits {
        out.extend(cs[cursor..*a].iter());
        out.push_str(text);
        cursor = *b;
    }
    out.extend(cs[cursor..].iter());
    Ok(Value::Str(out))
}
