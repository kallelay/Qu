//! § text: codepoints, distance, folding and layout (2026-09-16).
//!
//! The string half of `docs/design/toolkit-text.md` that the gap analysis
//! found missing. It is deliberately **not** the whole of §2: see "what is
//! not here" below, which matters more than the list of what is.
//!
//! What ships:
//!   `codepoints`, `nbytes`, `casefold`, `levenshtein`, `similar`,
//!   `word_wrap`, `dedent`, `indent`, `strip_ansi`.
//!
//! Named `word_wrap`, not `wrap`: `wrap(x, [lo=], [hi=])` already exists
//! for phase/angle wrapping (modular arithmetic on numeric data), an
//! established DSP term, and the two are unrelated operations that
//! happened to want the same short name -- a genuine collision found
//! only at merge time (the two match arms shadowed each other and the
//! compiler's own "unreachable pattern" warning caught it).
//!
//! **What is not here, on purpose.** The spec asks for `graphemes` — index
//! by grapheme cluster, so `len(s)` counts what a human calls a character.
//! Correct clustering is UAX #29 (combining marks, regional-indicator pairs,
//! ZWJ emoji sequences, Hangul jamo) and any short approximation gets
//! flag emoji and family emoji wrong while looking right for Latin text.
//! Shipping an approximation under the name `graphemes` would be a function
//! whose label is true and whose value is not, which is the defect class the
//! gap analysis exists to document. It stays absent until it can be done
//! against the real tables — an absent function makes the caller look; a
//! plausible wrong one does not.
//!
//! For the same reason `word_wrap` documents itself as wrapping on **codepoints**,
//! not display columns: East Asian Wide and combining marks make those
//! differ, and the honest name for what this does is not "columns".

use crate::{e, style_num, style_str, text_arg, Value, R};

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "codepoints" => codepoints(args),
        "nbytes" => nbytes(args),
        "casefold" => casefold(args),
        "levenshtein" => levenshtein_fn(args),
        "similar" => similar(args, style),
        "word_wrap" => wrap(args, style),
        "dedent" => dedent(args),
        "indent" => indent(args, style),
        "strip_ansi" => strip_ansi(args),
        other => e(format!("text_ops: unknown function `{other}`")),
    }
}

/// Unicode scalar values, one per element. Distinct from `nbytes` (UTF-8
/// storage) and from a grapheme count (see the module doc); a string of one
/// emoji with a skin-tone modifier is 2 codepoints, several bytes, and one
/// thing a reader would call a character.
fn codepoints(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    Ok(Value::Vec(
        s.chars().map(|c| c as u32 as f64).collect::<Vec<f64>>().into(),
    ))
}

fn nbytes(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    Ok(Value::Num(s.len() as f64))
}

/// Unicode simple case folding — the correct primitive for caseless
/// comparison, which `lower` is not: `lower("ß")` is `"ß"`, while folding it
/// gives `"ss"`, so `casefold(a) == casefold(b)` matches "STRASSE" against
/// "straße" where a lowercase comparison does not.
fn casefold(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ß' => out.push_str("ss"),
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ı' => out.push('i'),
            'ſ' => out.push('s'),
            _ => {
                for l in c.to_lowercase() {
                    out.push(l);
                }
            }
        }
    }
    Ok(Value::Str(out))
}

fn lev(a: &str, b: &str) -> usize {
    let x: Vec<char> = a.chars().collect();
    let y: Vec<char> = b.chars().collect();
    if x.is_empty() {
        return y.len();
    }
    if y.is_empty() {
        return x.len();
    }
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    let mut cur = vec![0usize; y.len() + 1];
    for i in 1..=x.len() {
        cur[0] = i;
        for j in 1..=y.len() {
            let cost = if x[i - 1] == y[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[y.len()]
}

/// Edit distance in **codepoints**, not bytes — so a two-byte character
/// counts as one edit, which is what a caller comparing names expects.
fn levenshtein_fn(args: &[Value]) -> R<Value> {
    let a = text_arg(args, 0)?;
    let b = text_arg(args, 1)?;
    Ok(Value::Num(lev(&a, &b) as f64))
}

/// Similarity in `0..1`. `metric="levenshtein"` (default) is
/// `1 - distance/max_len`; `metric="dice"` is the Sørensen–Dice coefficient
/// over character bigrams, which is better for reordered words.
fn similar(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let a = text_arg(args, 0)?;
    let b = text_arg(args, 1)?;
    let metric = style_str(style, "metric").unwrap_or_else(|| "levenshtein".to_string());
    let v = match metric.as_str() {
        "levenshtein" | "lev" => {
            let n = a.chars().count().max(b.chars().count());
            if n == 0 {
                1.0
            } else {
                1.0 - (lev(&a, &b) as f64 / n as f64)
            }
        }
        "dice" => {
            let bigrams = |s: &str| -> Vec<(char, char)> {
                let cs: Vec<char> = s.chars().collect();
                cs.windows(2).map(|w| (w[0], w[1])).collect()
            };
            let (x, mut y) = (bigrams(&a), bigrams(&b));
            if x.is_empty() && y.is_empty() {
                1.0
            } else if x.is_empty() || y.is_empty() {
                0.0
            } else {
                let total = x.len() + y.len();
                let mut hits = 0usize;
                for g in &x {
                    if let Some(p) = y.iter().position(|h| h == g) {
                        y.remove(p);
                        hits += 1;
                    }
                }
                2.0 * hits as f64 / total as f64
            }
        }
        other => {
            return e(format!(
                "similar: `metric=\"{other}\"` is not a metric -- use \"levenshtein\" or \"dice\""
            ))
        }
    };
    Ok(Value::Num(v))
}

/// Greedy word wrap at `width` **codepoints** (see the module doc on why
/// this is not called columns). Existing newlines are paragraph breaks and
/// are preserved; a word longer than the width is left intact on its own
/// line rather than broken mid-word.
fn wrap(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let width = match args.get(1) {
        Some(Value::Num(n)) => *n,
        _ => style_num(style, "width").unwrap_or(72.0),
    };
    if width < 1.0 || width.fract() != 0.0 {
        return e(format!("wrap: `width={width}` must be a whole number >= 1"));
    }
    let width = width as usize;
    let mut out = String::new();
    for (i, para) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut col = 0usize;
        let mut first = true;
        for word in para.split_whitespace() {
            let wlen = word.chars().count();
            if !first && col + 1 + wlen > width {
                out.push('\n');
                col = 0;
                first = true;
            }
            if !first {
                out.push(' ');
                col += 1;
            }
            out.push_str(word);
            col += wlen;
            first = false;
        }
    }
    Ok(Value::Str(out))
}

/// Remove the longest common leading whitespace from every non-blank line —
/// the inverse of `indent`, and what a heredoc-shaped string literal needs.
fn dedent(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let mut common: Option<String> = None;
    for line in s.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let ws: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        common = Some(match common {
            None => ws,
            Some(prev) => {
                let n = prev
                    .chars()
                    .zip(ws.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                prev.chars().take(n).collect()
            }
        });
    }
    let strip = common.unwrap_or_default();
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.strip_prefix(strip.as_str()).unwrap_or(line));
    }
    if s.ends_with('\n') {
        out.push('\n');
    }
    Ok(Value::Str(out))
}

fn indent(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let pad = match args.get(1) {
        Some(Value::Str(p)) => p.clone(),
        Some(Value::Num(n)) => " ".repeat(*n as usize),
        _ => match style_str(style, "with") {
            Some(p) => p,
            None => "    ".to_string(),
        },
    };
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !line.trim().is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    if s.ends_with('\n') {
        out.push('\n');
    }
    Ok(Value::Str(out))
}

/// Strip ANSI CSI/OSC escape sequences — what you need before measuring or
/// storing text captured from a terminal, where an invisible colour code
/// otherwise counts toward every length.
fn strip_ansi(args: &[Value]) -> R<Value> {
    let s = text_arg(args, 0)?;
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < cs.len() {
        if cs[i] == '\u{1b}' && i + 1 < cs.len() {
            match cs[i + 1] {
                '[' => {
                    // CSI: ends at the first byte in 0x40..=0x7e
                    i += 2;
                    while i < cs.len() && !('\u{40}'..='\u{7e}').contains(&cs[i]) {
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                ']' => {
                    // OSC: ends at BEL or ESC \
                    i += 2;
                    while i < cs.len() && cs[i] != '\u{7}' {
                        if cs[i] == '\u{1b}' && i + 1 < cs.len() && cs[i + 1] == '\\' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    Ok(Value::Str(out))
}
