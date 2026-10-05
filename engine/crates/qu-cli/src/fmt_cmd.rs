//! `qu fmt` -- a conservative, lexer-driven source formatter.
//!
//! # What it is (and is not)
//!
//! It does NOT parse and re-print the AST. The AST has lost the comments,
//! the spelling of every literal and the user's line breaks, so printing
//! from it would rewrite the user's file rather than tidy it. Instead the
//! formatter walks the *token stream* (`qu_lexer::lex`) alongside the
//! original text and changes only the whitespace BETWEEN tokens and at the
//! start of lines. Every token's bytes -- strings, raw strings, `{...}`
//! interpolation bodies, numbers, comments -- are copied through untouched.
//! That is also why the result is checkable: stripping all whitespace from
//! the input and the output must give identical text, and the token stream
//! must be identical. `run_fmt` verifies the second before writing a file,
//! and refuses (exit 2) rather than write something that differs.
//!
//! # What it normalises
//!
//! * Indentation of statement lines: 4 spaces per block level (the repo's own
//!   .qu files use 4: 3842 lines at 4 against 2 at 2). Block
//!   structure comes from statement-head keywords (`if`/`for`/`while`/
//!   `function`/`try`/`select`/`do`/`repeat`/`with`/`unsafe`/`enum`/
//!   `parallel for`/`pool`/`every|after|at`/`on elapsed`/`watch`/
//!   `wait until`, closed by `end`, `loop`, `until`). `else`/`elseif`/
//!   `catch`/`case` sit at their opener's level. Tabs in indentation become
//!   spaces. A heuristic miss (an opener the formatter does not know) can
//!   only mis-indent -- never change meaning, because indentation is not
//!   significant in Qu.
//! * Continuation lines (inside brackets, or after a trailing operator or
//!   comma) keep their own relative indentation: they are shifted by the
//!   same amount their statement's first line moved. Continuation lines
//!   indented with tabs are left alone.
//! * Comment-only lines take the indentation of the current block.
//! * Trailing whitespace is removed; the file ends with exactly one line
//!   terminator; trailing blank lines are dropped. Runs of blank lines in
//!   the middle of a file are left as the author wrote them.
//! * Spacing, only where unambiguous: one space after every `,` (unless a
//!   closing bracket follows); one space on each side of the comparison /
//!   compound-assignment operators `== != <= >= := += -= *= /= ^= .*= ./=
//!   |>`, and of `=` at bracket depth 0 (so a keyword argument `f(x=1)`
//!   and a default parameter are left exactly as written). Nothing else
//!   (`+ - * / < >`, ranges, unary minus, `'`) is touched: `-` and `'` are
//!   ambiguous between binary/unary and transpose/quote.
//!
//! # Line endings
//!
//! Preserved per line: a line that ended `\r\n` still ends `\r\n`. The
//! added final terminator is `\r\n` if the file contains any `\r\n`, else
//! `\n`.
//!
//! # Multi-line string literals
//!
//! Every line after the first line of a string/raw-string that spans lines
//! is copied byte for byte (trailing whitespace and all), as is the rest of
//! the line the literal starts on.

use qu_lexer::{lex, Tok, Token};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Block {
    Plain,
    Select,
    Case,
    /// `do` / `repeat`: closed by `end`, `loop`, `until` or a post-test `while`.
    Loop,
}

/// Block structure of a source file, from the token stream alone.
pub struct Analysis {
    pub toks: Vec<Token>,
    /// Per token: the block depth BEFORE the token.
    pub depth_before: Vec<usize>,
    /// Per token: first token of a statement (start of a line, or after `;`)
    /// at bracket depth 0.
    pub stmt_head: Vec<bool>,
    /// Per 0-based source line: indentation level (in blocks) for a
    /// statement that starts on it, or the level a comment would take.
    pub line_level: Vec<usize>,
    /// Per line: the line begins a new statement.
    pub line_stmt_start: Vec<bool>,
    /// Per line: it continues a statement begun earlier (bracket depth > 0,
    /// or after a continuation operator).
    pub line_cont: Vec<bool>,
    /// Per line: the line is the 2nd+ line of a multi-line string literal.
    pub line_verbatim: Vec<bool>,
    /// Byte offset where each line begins.
    pub line_starts: Vec<usize>,
}

fn line_starts_of(src: &str) -> Vec<usize> {
    let mut v = vec![0];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn is_assign_like(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<=" | ">=" | ":=" | "+=" | "-=" | "*=" | "/=" | "^=" | ".*=" | "./=" | "|>")
}

pub fn analyse(src: &str) -> Analysis {
    let toks = lex(src);
    let line_starts = line_starts_of(src);
    let nlines = line_starts.len();
    let line_of = |byte: usize| line_starts.partition_point(|&s| s <= byte) - 1;

    let mut line_verbatim = vec![false; nlines];
    for t in &toks {
        if matches!(t.tok, Tok::Eof | Tok::Newline) || t.span.end <= t.span.start {
            continue;
        }
        let a = line_of(t.span.start);
        let b = line_of(t.span.end - 1);
        for l in a + 1..=b {
            line_verbatim[l] = true;
        }
    }

    let mut depth_before = vec![0; toks.len()];
    let mut stmt_head = vec![false; toks.len()];
    let mut line_level = vec![0; nlines];
    let mut line_stmt_start = vec![false; nlines];
    let mut line_cont = vec![false; nlines];

    let mut stack: Vec<Block> = Vec::new();
    let mut bracket: i64 = 0;
    let mut prev_is_newline = true; // start of file counts as a statement boundary
    let mut skip_tail = false; // token directly after `end` / `loop` is its tail word
    let mut at_head = true; // next token begins a statement
    let mut cur_line_seen: Option<usize> = None;
    let mut next_line_to_fill = 0usize;

    // Fill defaults for lines with no tokens up to (not including) `upto`.
    macro_rules! fill_lines_before {
        ($upto:expr, $stack:expr, $bracket:expr, $prev_nl:expr) => {
            while next_line_to_fill < $upto {
                line_level[next_line_to_fill] = $stack.len();
                line_cont[next_line_to_fill] = $bracket > 0 || !$prev_nl;
                next_line_to_fill += 1;
            }
        };
    }

    for (i, t) in toks.iter().enumerate() {
        if matches!(t.tok, Tok::Eof) {
            break;
        }
        if matches!(t.tok, Tok::Newline) {
            prev_is_newline = true;
            at_head = true;
            skip_tail = false;
            continue;
        }
        let line = line_of(t.span.start);
        if cur_line_seen != Some(line) {
            // First token on this line.
            fill_lines_before!(line, stack, bracket, prev_is_newline);
            cur_line_seen = Some(line);
            next_line_to_fill = line + 1;
            let starts = prev_is_newline && bracket <= 0;
            if starts {
                at_head = true;
            }
            line_stmt_start[line] = starts;
            line_cont[line] = !starts;
            // Indent for this line (computed from the stack BEFORE the head
            // token mutates it).
            let top = stack.last().copied();
            let len = stack.len();
            let lvl = match &t.tok {
                Tok::Keyword("end") => len.saturating_sub(if top == Some(Block::Case) { 2 } else { 1 }),
                Tok::Keyword("else") | Tok::Keyword("elseif") | Tok::Keyword("catch") => len.saturating_sub(1),
                Tok::Keyword("until") if top == Some(Block::Loop) => len.saturating_sub(1),
                Tok::Keyword("while") if top == Some(Block::Loop) => len.saturating_sub(1),
                Tok::Ident(w) if w == "loop" && top == Some(Block::Loop) => len.saturating_sub(1),
                Tok::Ident(w) if w == "case" && top == Some(Block::Case) => len.saturating_sub(1),
                _ => len,
            };
            line_level[line] = lvl;
        }
        depth_before[i] = stack.len();
        let head = at_head && bracket <= 0;
        stmt_head[i] = head;
        at_head = false;
        prev_is_newline = false;

        let next = toks.get(i + 1).map(|t| &t.tok);
        let next2 = toks.get(i + 2).map(|t| &t.tok);

        if skip_tail {
            skip_tail = false;
            // The word right after `end`/`loop` names what is closed.
            if matches!(t.tok, Tok::Keyword(_) | Tok::Ident(_)) {
                continue;
            }
        }

        match &t.tok {
            Tok::Op("(") | Tok::Op("[") | Tok::Op("{") => bracket += 1,
            Tok::Op(")") | Tok::Op("]") | Tok::Op("}") => bracket = (bracket - 1).max(0),
            Tok::Op(";") if bracket <= 0 => at_head = true,
            Tok::Keyword("end") => {
                if stack.last() == Some(&Block::Case) {
                    stack.pop();
                }
                stack.pop();
                skip_tail = true;
            }
            Tok::Keyword("function") => stack.push(Block::Plain),
            Tok::Keyword(k) if head => match *k {
                "if" | "for" | "try" | "unsafe" | "with" => stack.push(Block::Plain),
                "while" => {
                    if stack.last() == Some(&Block::Loop) {
                        stack.pop();
                    } else {
                        stack.push(Block::Plain);
                    }
                }
                "until" => {
                    if stack.last() == Some(&Block::Loop) {
                        stack.pop();
                    }
                }
                "do" | "repeat" => stack.push(Block::Loop),
                _ => {}
            },
            Tok::Ident(w) if head => match w.as_str() {
                "select" if matches!(next, Some(Tok::Ident(c)) if c == "case") => stack.push(Block::Select),
                "case" if matches!(stack.last(), Some(Block::Select) | Some(Block::Case)) => {
                    if stack.last() == Some(&Block::Case) {
                        stack.pop();
                    }
                    stack.push(Block::Case);
                }
                "loop" if stack.last() == Some(&Block::Loop) => {
                    stack.pop();
                    skip_tail = true;
                }
                "parallel" if matches!(next, Some(Tok::Keyword("for"))) => {
                    stack.push(Block::Plain);
                    // the `for` belongs to this opener
                    skip_tail = true;
                }
                "pool" if matches!(next, Some(Tok::Ident(_))) && matches!(next2, Some(Tok::Keyword("with"))) => {
                    stack.push(Block::Plain)
                }
                "enum" if matches!(next, Some(Tok::Ident(_))) => stack.push(Block::Plain),
                "wait" if matches!(next, Some(Tok::Keyword("until"))) => {
                    stack.push(Block::Plain);
                    skip_tail = true;
                }
                "every" | "after" | "at"
                    if !matches!(
                        next,
                        Some(Tok::Op("=" | ":=" | "+=" | "-=" | "*=" | "/=" | ".=" | ".*=" | "./=" | "^="))
                    ) =>
                {
                    stack.push(Block::Plain)
                }
                "on" if matches!(next, Some(Tok::Ident(e)) if e == "elapsed" || e == "elapsedOnce")
                    && matches!(next2, Some(Tok::Op("("))) =>
                {
                    stack.push(Block::Plain)
                }
                "watch" if matches!(next, Some(Tok::Ident(k)) if k == "file" || k == "url") && matches!(next2, Some(Tok::Op("("))) => {
                    stack.push(Block::Plain)
                }
                "watch"
                    if matches!(next, Some(Tok::Ident(_)))
                        && matches!(next2, Some(Tok::Keyword("do") | Tok::Newline | Tok::Eof | Tok::Op(";"))) =>
                {
                    stack.push(Block::Plain)
                }
                _ => {}
            },
            _ => {}
        }
        // The lexer swallows the newline after `then`/`do`, but the next line
        // still begins a statement.
        if matches!(t.tok, Tok::Keyword("then") | Tok::Keyword("do")) && bracket <= 0 {
            prev_is_newline = true;
        }
    }
    fill_lines_before!(nlines, stack, bracket, prev_is_newline);

    Analysis { toks, depth_before, stmt_head, line_level, line_stmt_start, line_cont, line_verbatim, line_starts }
}

/// Zero-argument `test_*` functions defined at the top level of `src`, as
/// `(name, 1-based line)`. Used by `qu test`.
pub fn top_level_test_functions(src: &str) -> Vec<(String, u32)> {
    let a = analyse(src);
    let t = &a.toks;
    let mut out = Vec::new();
    for i in 0..t.len() {
        if !a.stmt_head[i] || a.depth_before[i] != 0 {
            continue;
        }
        let (name, rest) = match (&t[i].tok, t.get(i + 1).map(|x| &x.tok)) {
            (Tok::Keyword("function"), Some(Tok::Ident(n))) => (n.clone(), i + 2),
            (Tok::Ident(n), Some(Tok::Op("("))) => (n.clone(), i + 1),
            _ => continue,
        };
        if !name.starts_with("test_") {
            continue;
        }
        // `(` `)` -- zero parameters
        if !(matches!(t.get(rest).map(|x| &x.tok), Some(Tok::Op("(")))
            && matches!(t.get(rest + 1).map(|x| &x.tok), Some(Tok::Op(")"))))
        {
            continue;
        }
        // the short form is only a definition when `:=` follows the `)`
        if matches!(t[i].tok, Tok::Ident(_)) && !matches!(t.get(rest + 2).map(|x| &x.tok), Some(Tok::Op(":="))) {
            continue;
        }
        out.push((name, t[i].span.line));
    }
    out
}

fn gap_is_blank(g: &str) -> bool {
    g.chars().all(|c| c == ' ' || c == '\t')
}

/// Spaces per block level.
const INDENT: usize = 4;

fn indent_width(ws: &str) -> i64 {
    ws.chars().map(|c| if c == '\t' { INDENT as i64 } else { 1 }).sum()
}

/// Format `src`. Pure and total: it never fails, it just returns the input
/// unchanged where it does not know better.
pub fn format_source(src: &str) -> String {
    let a = analyse(src);
    let lines: Vec<&str> = src.split('\n').collect();
    let nlines = lines.len();
    debug_assert_eq!(nlines, a.line_starts.len());

    // Tokens grouped by the line they start on.
    let mut by_line: Vec<Vec<usize>> = vec![Vec::new(); nlines];
    for (i, t) in a.toks.iter().enumerate() {
        if matches!(t.tok, Tok::Eof | Tok::Newline) {
            continue;
        }
        let l = a.line_starts.partition_point(|&s| s <= t.span.start) - 1;
        by_line[l].push(i);
    }

    let mut bracket: i64 = 0;
    let mut delta: i64 = 0;
    // (text, verbatim)
    let mut out: Vec<(String, bool)> = Vec::with_capacity(nlines);

    for l in 0..nlines {
        let raw = lines[l];
        if a.line_verbatim[l] {
            out.push((raw.to_string(), true));
            // brackets inside the tail of a string line still count
            for &i in &by_line[l] {
                match &a.toks[i].tok {
                    Tok::Op("(") | Tok::Op("[") | Tok::Op("{") => bracket += 1,
                    Tok::Op(")") | Tok::Op("]") | Tok::Op("}") => bracket = (bracket - 1).max(0),
                    _ => {}
                }
            }
            continue;
        }
        let cr = raw.ends_with('\r');
        let body = raw.trim_end_matches(['\r', ' ', '\t']);
        let eol = if cr { "\r" } else { "" };
        let lead_len = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let lead = &raw[..lead_len.min(raw.len())];

        if body.trim().is_empty() {
            out.push((eol.to_string(), false));
            continue;
        }

        let toks_here = &by_line[l];
        let new_indent: usize;
        if a.line_cont[l] && !a.line_stmt_start[l] {
            // continuation (or a comment inside one): shift by the statement's delta
            if lead.contains('\t') {
                new_indent = usize::MAX; // sentinel: keep verbatim
            } else {
                new_indent = (indent_width(lead) + delta).max(0) as usize;
            }
        } else {
            new_indent = a.line_level[l] * INDENT;
            if a.line_stmt_start[l] {
                delta = new_indent as i64 - indent_width(lead);
            }
        }
        let mut line = String::new();
        if new_indent == usize::MAX {
            line.push_str(lead);
        } else {
            line.push_str(&" ".repeat(new_indent));
        }

        if toks_here.is_empty() {
            // comment-only line
            line.push_str(body.trim_start());
            line.push_str(eol);
            out.push((line, false));
            continue;
        }

        let mut prev_end: Option<usize> = None;
        let mut prev_tok: Option<&Tok> = None;
        let mut prev_spaced = false;
        let line_end_byte = a.line_starts[l] + raw.len();
        let mut verbatim_tail_from: Option<usize> = None;
        for (k, &i) in toks_here.iter().enumerate() {
            let t = &a.toks[i];
            let multiline = t.span.end > a.line_starts.get(l + 1).copied().unwrap_or(usize::MAX);
            let depth0 = bracket <= 0;
            let spaced = match &t.tok {
                Tok::Op(op) => is_assign_like(op) || (*op == "=" && depth0),
                _ => false,
            };
            if k > 0 {
                let gap = &src[prev_end.unwrap()..t.span.start];
                let want_space = gap_is_blank(gap)
                    && (matches!(prev_tok, Some(Tok::Op(",")))
                        && !matches!(&t.tok, Tok::Op(")") | Tok::Op("]") | Tok::Op("}"))
                        || spaced
                        || prev_spaced);
                if want_space {
                    line.push(' ');
                } else {
                    line.push_str(gap);
                }
            }
            if multiline {
                verbatim_tail_from = Some(t.span.start);
                break;
            }
            line.push_str(&src[t.span.start..t.span.end]);
            match &t.tok {
                Tok::Op("(") | Tok::Op("[") | Tok::Op("{") => bracket += 1,
                Tok::Op(")") | Tok::Op("]") | Tok::Op("}") => bracket = (bracket - 1).max(0),
                _ => {}
            }
            prev_end = Some(t.span.end);
            prev_tok = Some(&t.tok);
            prev_spaced = spaced;
        }
        if let Some(from) = verbatim_tail_from {
            line.push_str(&src[from..line_end_byte]);
            // account for brackets the multi-line token's line still opens
            for &i in toks_here {
                let t = &a.toks[i];
                if t.span.start < from {
                    continue;
                }
                match &t.tok {
                    Tok::Op("(") | Tok::Op("[") | Tok::Op("{") => bracket += 1,
                    Tok::Op(")") | Tok::Op("]") | Tok::Op("}") => bracket = (bracket - 1).max(0),
                    _ => {}
                }
            }
            out.push((line, true));
            continue;
        }
        let tail = &src[prev_end.unwrap()..a.line_starts[l] + raw.len()];
        line.push_str(tail.trim_end_matches(['\r', ' ', '\t']));
        line.push_str(eol);
        out.push((line, false));
    }

    // Trailing blank lines go; one final terminator stays.
    while let Some((text, verb)) = out.last() {
        if !*verb && text.trim().is_empty() {
            out.pop();
        } else {
            break;
        }
    }
    if out.is_empty() {
        return String::new();
    }
    let crlf = src.contains("\r\n");
    let (last, last_verb) = out.last_mut().unwrap();
    let last_verb = &last_verb.clone();
    if !*last_verb {
        while last.ends_with('\r') {
            last.pop();
        }
    }
    let mut result = String::with_capacity(src.len() + 16);
    for (n, (text, _)) in out.iter().enumerate() {
        if n > 0 {
            result.push('\n');
        }
        result.push_str(text);
    }
    if crlf {
        // the last (non-verbatim) line had its CR stripped above
        if !*last_verb {
            result.push_str("\r\n");
        } else if result.ends_with('\r') {
            result.push('\n');
        } else {
            result.push_str("\r\n");
        }
    } else {
        result.push('\n');
    }
    result
}

/// Token kinds+text of `src` ignoring whitespace/comments: the identity a
/// formatter must preserve.
fn token_fingerprint(src: &str) -> Vec<String> {
    let mut v: Vec<String> = lex(src).iter().map(|t| format!("{:?}", t.tok)).collect();
    // A file with no final newline lexes without the closing `Newline`;
    // adding the final newline is the point, so that token is not identity.
    if v.len() >= 2 && v[v.len() - 2] == "Newline" {
        v.remove(v.len() - 2);
    }
    v
}

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `Ok(formatted)` when the result provably only differs by whitespace.
pub fn format_checked(src: &str) -> Result<String, String> {
    let out = format_source(src);
    if strip_ws(&out) != strip_ws(src) {
        return Err("internal error: formatting would change non-whitespace text; refusing".into());
    }
    if token_fingerprint(&out) != token_fingerprint(src) {
        return Err("internal error: formatting would change the token stream; refusing".into());
    }
    Ok(out)
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
                continue;
            }
            collect(&p, out);
        } else if name.ends_with(".qu") {
            out.push(p);
        }
    }
}

pub fn cmd_fmt(args: &[String]) -> Result<(), String> {
    let mut check = false;
    let mut stdin = false;
    let mut paths: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "--check" => check = true,
            "--stdin" => stdin = true,
            s if s.starts_with("--") => return Err(format!("fmt: unknown option `{s}`")),
            s => paths.push(s.to_string()),
        }
    }
    if stdin {
        let mut src = String::new();
        std::io::stdin().read_to_string(&mut src).map_err(|e| format!("fmt: cannot read stdin: {e}"))?;
        let out = format_checked(&src).map_err(|e| format!("fmt: <stdin>: {e}"))?;
        if check {
            if out != src {
                return Err("fmt: <stdin> would change".into());
            }
            return Ok(());
        }
        std::io::stdout().write_all(out.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if paths.is_empty() {
        paths.push(".".into());
    }
    let mut files = Vec::new();
    for p in &paths {
        let pb = Path::new(p);
        if !pb.exists() {
            return Err(format!("fmt: no such file or directory `{p}`"));
        }
        collect(pb, &mut files);
    }
    let mut would_change = Vec::new();
    let mut errors = 0;
    let mut changed = 0;
    for f in &files {
        let src = match std::fs::read(f) {
            Ok(b) => match String::from_utf8(b) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("qu fmt: {}: not valid UTF-8, skipped", f.display());
                    errors += 1;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("qu fmt: {}: {e}", f.display());
                errors += 1;
                continue;
            }
        };
        match format_checked(&src) {
            Ok(out) if out == src => {}
            Ok(out) => {
                if check {
                    would_change.push(f.display().to_string());
                } else if let Err(e) = std::fs::write(f, out) {
                    eprintln!("qu fmt: {}: {e}", f.display());
                    errors += 1;
                } else {
                    println!("formatted {}", f.display());
                    changed += 1;
                }
            }
            Err(e) => {
                eprintln!("qu fmt: {}: {e}", f.display());
                errors += 1;
            }
        }
    }
    if check {
        for f in &would_change {
            println!("would reformat {f}");
        }
        if errors > 0 {
            return Err(format!("fmt: {errors} file(s) could not be checked"));
        }
        if !would_change.is_empty() {
            // exit 1, distinct from an internal error (2): handled by the caller
            std::process::exit(1);
        }
        return Ok(());
    }
    if errors > 0 {
        return Err(format!("fmt: {errors} file(s) could not be formatted"));
    }
    println!("{} file(s) checked, {changed} reformatted", files.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_blocks_and_else() {
        let src = "if x\nprint(1)\nelse\nprint(2)\nend if\n";
        assert_eq!(format_source(src), "if x\n    print(1)\nelse\n    print(2)\nend if\n");
    }

    #[test]
    fn select_case_levels() {
        let src = "select case x\ncase 1\nprint(1)\ncase else\nprint(2)\nend select\n";
        assert_eq!(
            format_source(src),
            "select case x\n    case 1\n        print(1)\n    case else\n        print(2)\nend select\n"
        );
    }

    #[test]
    fn spacing_is_conservative() {
        assert_eq!(format_source("x=f(a,b,c)\n"), "x = f(a, b, c)\n");
        assert_eq!(format_source("f(x=1)\n"), "f(x=1)\n");
        assert_eq!(format_source("if a==b\nend if\n"), "if a == b\nend if\n");
        assert_eq!(format_source("y = a-b*c\n"), "y = a-b*c\n");
    }

    #[test]
    fn strings_and_comments_survive() {
        let src = "s = \"a,b=c   {x ,y}\"  # note,  here=1  \nr = r\"x,y  z\"\n";
        let out = format_source(src);
        assert!(out.contains("\"a,b=c   {x ,y}\""));
        assert!(out.contains("# note,  here=1"));
        assert!(out.contains("r\"x,y  z\""));
    }

    #[test]
    fn multiline_string_is_verbatim() {
        let src = "x = \"line1\n   keep   \n  me\"\nprint(x)  \n";
        let out = format_source(src);
        assert!(out.contains("   keep   \n  me\""));
        assert!(out.ends_with("print(x)\n"));
    }

    #[test]
    fn final_newline_and_crlf() {
        assert_eq!(format_source("x = 1"), "x = 1\n");
        assert_eq!(format_source("x = 1  \n\n\n"), "x = 1\n");
        assert_eq!(format_source("if a\r\nb = 1\r\nend if\r\n"), "if a\r\n    b = 1\r\nend if\r\n");
        assert_eq!(format_source(""), "");
    }

    #[test]
    fn continuation_lines_shift_with_their_statement() {
        let src = "if a
  x = [1, 2,
       3, 4]
end if
";
        assert_eq!(format_source(src), "if a
    x = [1, 2,
         3, 4]
end if
");
    }

    #[test]
    fn line_after_then_or_do_is_a_statement_not_a_continuation() {
        let src = "if n < 2 then\n        return n\n    else\nreturn 3\nend if\nwhile x < 3 do\nx += 1\nend while\n";
        assert_eq!(
            format_source(src),
            "if n < 2 then\n    return n\nelse\n    return 3\nend if\nwhile x < 3 do\n    x += 1\nend while\n"
        );
    }

    #[test]
    fn idempotent_on_samples() {
        for src in [
            "function f(a,b)\nreturn a+b\nend function\nx=f(1,2) # c\n",
            "do\nx=1\nloop while x<3\n",
            "repeat\nx=1\nuntil x>3\n",
            "try\nerror(\"a\")\ncatch e\nprint(e)\nend try\n",
        ] {
            let once = format_source(src);
            assert_eq!(format_source(&once), once, "not idempotent for {src:?}");
        }
    }

    #[test]
    fn finds_top_level_tests() {
        let src = "function test_a()\nend function\nfunction test_b(x)\nend function\nif 1\nfunction test_c()\nend function\nend if\ntest_d() := 1\n";
        let names: Vec<_> = top_level_test_functions(src).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["test_a", "test_d"]);
    }
}
