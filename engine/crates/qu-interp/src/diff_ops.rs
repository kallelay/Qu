//! `diff_lines(a, b, [changed_only=false])` -- a line diff of two texts.
//!
//! Qu's numeric `diff` only takes numbers, and `pdf.text_diff` only diffs
//! PDF pages. This is the general one: two `List`s of strings (or two
//! strings, split on line breaks) in, a `List` of `Record`s out, one per
//! line of the merged result:
//!
//!   op    "equal" | "insert" | "delete"
//!   old   1-based line number in `a`  (`none` for an insert)
//!   new   1-based line number in `b`  (`none` for a delete)
//!   text  the line
//!
//! Line numbers are 1-based because they are what an editor shows, unlike
//! Qu's 0-based list indices. The algorithm is the textbook longest common
//! subsequence on the lines left after trimming the common prefix and
//! suffix (the usual case -- a few edits in a long file -- leaves a small
//! middle). The middle is capped at `MAX_CELLS` table cells so a huge,
//! wholly different pair gives a clear error rather than exhausting memory.

use std::sync::Arc;

use crate::{e, style_entry, truthy, Value, R};

/// A list of strings, or one string split into lines (CRLF or LF).
fn text_lines_arg(v: &Value, f: &str, which: &str) -> R<Vec<String>> {
    match v {
        Value::Str(s) => Ok(s.lines().map(str::to_string).collect()),
        Value::List(items) => items
            .iter()
            .enumerate()
            .map(|it| match it.1 {
                Value::Str(s) => Ok(s.clone()),
                other => e(format!(
                    "{f}: the {which} list must hold only strings; element {} is {}",
                    it.0,
                    other.type_name()
                )),
            })
            .collect(),
        other => e(format!("{f}: the {which} argument must be a list of strings or a string, found {}", other.type_name())),
    }
}

/// Cells of the LCS table (`(n + 1) * (m + 1)`, 4 bytes each): ~100 MB.
const MAX_CELLS: usize = 25_000_000;

pub fn call(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    if args.len() != 2 {
        return e("diff_lines(a, b, [changed_only=]) takes two lists of strings (or two strings)");
    }
    let a = text_lines_arg(&args[0], "diff_lines", "first")?;
    let b = text_lines_arg(&args[1], "diff_lines", "second")?;
    let changed_only = style_entry(style, "changed_only").map(|(_, v)| truthy(v)).unwrap_or(false);

    let ops = diff(&a, &b)?;
    let mut out: Vec<Value> = Vec::new();
    for (op, i, j) in ops {
        if changed_only && op == Op::Equal {
            continue;
        }
        let (name, old, new, text) = match op {
            Op::Equal => ("equal", Value::Num((i + 1) as f64), Value::Num((j + 1) as f64), &a[i]),
            Op::Delete => ("delete", Value::Num((i + 1) as f64), Value::Nothing, &a[i]),
            Op::Insert => ("insert", Value::Nothing, Value::Num((j + 1) as f64), &b[j]),
        };
        out.push(Value::Record(Arc::new(vec![
            ("op".to_string(), Value::Str(name.to_string())),
            ("old".to_string(), old),
            ("new".to_string(), new),
            ("text".to_string(), Value::Str(text.clone())),
        ])));
    }
    Ok(Value::List(Arc::new(out)))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Equal,
    Delete,
    Insert,
}

/// The edit script as `(op, index in a, index in b)` triples, in order.
/// For `Delete` the `b` index is where the deletion sits; for `Insert` the
/// `a` index is -- callers read only the index that applies to the op.
pub fn diff(a: &[String], b: &[String]) -> R<Vec<(Op, usize, usize)>> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] {
        suf += 1;
    }
    let am = &a[pre..a.len() - suf];
    let bm = &b[pre..b.len() - suf];
    let (n, m) = (am.len(), bm.len());
    let cells = (n + 1).checked_mul(m + 1);
    if cells.is_none_or(|c| c > MAX_CELLS) {
        return e(format!(
            "diff_lines: the differing middle is {n} x {m} lines, too large to compare (limit about {MAX_CELLS} \
             table cells); diff smaller pieces"
        ));
    }

    let w = m + 1;
    let mut dp = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * w + j] = if am[i] == bm[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }

    let mut out: Vec<(Op, usize, usize)> = Vec::with_capacity(pre + n + m + suf);
    for k in 0..pre {
        out.push((Op::Equal, k, k));
    }
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if am[i] == bm[j] {
            out.push((Op::Equal, pre + i, pre + j));
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            out.push((Op::Delete, pre + i, pre + j));
            i += 1;
        } else {
            out.push((Op::Insert, pre + i, pre + j));
            j += 1;
        }
    }
    while i < n {
        out.push((Op::Delete, pre + i, pre + j));
        i += 1;
    }
    while j < m {
        out.push((Op::Insert, pre + i, pre + j));
        j += 1;
    }
    for k in 0..suf {
        out.push((Op::Equal, a.len() - suf + k, b.len() - suf + k));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    fn render(a: &str, b: &str) -> Vec<String> {
        let (a, b) = (lines(a), lines(b));
        diff(&a, &b)
            .unwrap()
            .into_iter()
            .map(|(op, i, j)| match op {
                Op::Equal => format!("= {}", a[i]),
                Op::Delete => format!("- {}", a[i]),
                Op::Insert => format!("+ {}", b[j]),
            })
            .collect()
    }

    #[test]
    fn identical_texts_are_all_equal() {
        assert_eq!(render("a\nb\nc", "a\nb\nc"), vec!["= a", "= b", "= c"]);
    }

    #[test]
    fn a_changed_line_is_a_delete_then_an_insert() {
        assert_eq!(render("a\nb\nc", "a\nX\nc"), vec!["= a", "- b", "+ X", "= c"]);
    }

    #[test]
    fn inserts_and_deletes_at_the_ends() {
        assert_eq!(render("b\nc", "a\nb\nc\nd"), vec!["+ a", "= b", "= c", "+ d"]);
        assert_eq!(render("a\nb\nc\nd", "b\nc"), vec!["- a", "= b", "= c", "- d"]);
    }

    #[test]
    fn empty_sides() {
        assert_eq!(render("", "x\ny"), vec!["+ x", "+ y"]);
        assert_eq!(render("x\ny", ""), vec!["- x", "- y"]);
        assert!(render("", "").is_empty());
    }

    #[test]
    fn applying_the_script_to_a_reproduces_b() {
        // property check on a handful of scrambled pairs
        let pairs = [
            ("a\nb\nc\nd\ne", "e\nd\nc\nb\na"),
            ("1\n2\n3\n4\n5\n6", "1\n3\n4\n7\n6"),
            ("x\nx\nx", "x\nx"),
            ("a\nb\nb\na", "b\na\na\nb"),
        ];
        for (sa, sb) in pairs {
            let (a, b) = (lines(sa), lines(sb));
            let rebuilt: Vec<String> = diff(&a, &b)
                .unwrap()
                .into_iter()
                .filter_map(|(op, i, j)| match op {
                    Op::Equal => Some(a[i].clone()),
                    Op::Insert => Some(b[j].clone()),
                    Op::Delete => None,
                })
                .collect();
            assert_eq!(rebuilt, b, "{sa:?} -> {sb:?}");
            let kept: Vec<String> = diff(&a, &b)
                .unwrap()
                .into_iter()
                .filter_map(|(op, i, _)| match op {
                    Op::Equal | Op::Delete => Some(a[i].clone()),
                    Op::Insert => None,
                })
                .collect();
            assert_eq!(kept, a, "a is recoverable from the script");
        }
    }

    #[test]
    fn a_huge_wholly_different_pair_is_refused_not_allocated() {
        let a: Vec<String> = (0..6000).map(|i| format!("a{i}")).collect();
        let b: Vec<String> = (0..6000).map(|i| format!("b{i}")).collect();
        let err = diff(&a, &b).unwrap_err().to_string();
        assert!(err.contains("too large to compare"), "{err}");
    }
}
