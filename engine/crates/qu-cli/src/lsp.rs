//! `qu lsp` — a Language Server Protocol server on stdin/stdout.
//!
//! One server so that every editor with an LSP client (VS Code, Sublime LSP,
//! Neovim, Helix, Kate, JetBrains via LSP4IJ, ...) gets the same answers,
//! from the same parser and the same builtin table the `qu` binary itself
//! uses, instead of each plugin re-deriving them.
//!
//! What it serves:
//!
//! * diagnostics  — `qu_lexer::lex_checked` + `qu_syntax::parse`, i.e. the
//!   errors `qu run` would report, published on open and on every change;
//! * hover        — a builtin's signature and one-line summary
//!   (`qu_interp::builtin_docs`), or a user function's header line;
//! * completion   — builtins, the document's own functions and variables,
//!   keywords;
//! * signatureHelp — parameters from the documented signature, the active
//!   one found by counting top-level commas (or by a `name=` argument);
//! * documentSymbol — functions, `#%%` cells, `# --- title` sections: the
//!   rules of `qu-ui-components/src/utils/outline.ts`, ported;
//! * definition   — a user function (or a variable's first assignment) in
//!   the same file.
//!
//! Transport is JSON-RPC 2.0 with `Content-Length` framing, written here
//! against `serde_json` (already a dependency of this crate): a framing
//! loop and a `match` do not justify an LSP framework.
//!
//! Positions. LSP counts columns in UTF-16 code units unless negotiated
//! otherwise, the lexer's `Span` carries byte offsets (and a column in
//! `char`s). Everything here converts through byte offsets: see [`Doc`].
//! A line ends at `\n`; a trailing `\r` is not part of the line. A lone
//! `\r` is NOT treated as a line break, because the lexer does not treat
//! it as one either and the two must agree on line numbers.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use qu_interp::builtin_docs::{builtin_doc, BuiltinDoc, BUILTIN_DOCS};
use qu_lexer::{Span, Tok, Token};
use serde_json::{json, Value};

/// A frame larger than this is taken to be a corrupt header rather than a
/// document (256 MiB): allocating whatever a garbled length says would
/// abort the process instead of reporting the problem.
const MAX_FRAME: usize = 256 * 1024 * 1024;

/// Statement words the parser treats as keywords in context but that are
/// not in `qu_lexer::KEYWORDS` (which lists only the always-reserved ones).
const EXTRA_KEYWORDS: &[&str] = &["break", "continue", "export"];

// ------------------------------------------------------------------ framing

/// Read one `Content-Length`-framed message body. `Ok(None)` is a clean end
/// of input (the client closed the pipe).
pub fn read_message<R: BufRead>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len: Option<usize> = None;
    let mut saw_header = false;
    loop {
        let mut raw = Vec::new();
        if r.read_until(b'\n', &mut raw)? == 0 {
            return Ok(None);
        }
        let line = String::from_utf8_lossy(&raw);
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if saw_header {
                break;
            }
            // Blank lines before any header: tolerated, not a message.
            continue;
        }
        saw_header = true;
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().ok();
            }
        }
    }
    let len = len.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "message header has no Content-Length"))?;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Content-Length {len} is implausibly large")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write one framed message.
pub fn write_message<W: Write>(w: &mut W, msg: &Value) -> io::Result<()> {
    let body = msg.to_string();
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}

/// `qu lsp [--stdio]`.
pub fn cmd_lsp(args: &[String]) -> Result<(), String> {
    for a in args {
        match a.as_str() {
            // What vscode-languageclient and several other clients append
            // for a stdio server. stdio is the only transport there is.
            "--stdio" => {}
            "--help" | "-h" => {
                println!(
                    "qu lsp [--stdio]\n\n\
                     Language Server Protocol server on stdin/stdout (JSON-RPC, Content-Length framing).\n\
                     Serves diagnostics, hover, completion, signature help, document symbols and\n\
                     go-to-definition for .qu files. Started by an editor, not by hand:\n\
                     see editors/README.md for the per-editor configuration."
                );
                return Ok(());
            }
            other => return Err(format!("qu lsp: unknown option `{other}` (the only option is --stdio)")),
        }
    }

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut server = Server::default();
    loop {
        let body = match read_message(&mut input) {
            Ok(Some(b)) => b,
            // The client went away without `exit`. Nothing left to serve.
            Ok(None) => return Ok(()),
            Err(e) => return Err(format!("qu lsp: {e}")),
        };
        let msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                let reply = rpc_error(Value::Null, -32700, &format!("parse error: {e}"));
                write_message(&mut output, &reply).map_err(|e| format!("qu lsp: stdout: {e}"))?;
                continue;
            }
        };
        let (replies, exit) = server.handle(&msg);
        for r in &replies {
            write_message(&mut output, r).map_err(|e| format!("qu lsp: stdout: {e}"))?;
        }
        if let Some(code) = exit {
            drop(output);
            std::process::exit(code);
        }
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

// ------------------------------------------------------------------ documents

/// An open document and the byte offset each of its lines starts at.
pub struct Doc {
    text: String,
    version: Option<i64>,
    line_starts: Vec<usize>,
}

impl Doc {
    pub fn new(text: String, version: Option<i64>) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Doc { text, version, line_starts }
    }

    /// The text of 0-based `line`, without its terminator.
    fn line(&self, line: usize) -> &str {
        let Some(&start) = self.line_starts.get(line) else { return "" };
        let end = self.line_starts.get(line + 1).map(|e| e - 1).unwrap_or(self.text.len());
        let s = &self.text[start..end];
        s.strip_suffix('\r').unwrap_or(s)
    }

    fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Byte offset -> (0-based line, UTF-16 column).
    fn position(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_starts.partition_point(|s| *s <= offset) - 1;
        let start = self.line_starts[line];
        // An offset on the `\r` or `\n` itself reads as the end of the line.
        let within = (offset - start).min(self.line(line).len());
        let col = self.text[start..start + within].encode_utf16().count();
        (line, col)
    }

    fn position_json(&self, offset: usize) -> Value {
        let (line, character) = self.position(offset);
        json!({"line": line, "character": character})
    }

    /// (0-based line, UTF-16 column) -> byte offset. A column past the end
    /// of the line means the end of the line, a line past the end of the
    /// document means the end of the document, as the protocol specifies.
    fn offset(&self, line: usize, character: usize) -> usize {
        let Some(&start) = self.line_starts.get(line) else { return self.text.len() };
        let mut units = 0;
        for (i, c) in self.line(line).char_indices() {
            if units >= character {
                return start + i;
            }
            units += c.len_utf16();
        }
        start + self.line(line).len()
    }

    fn offset_of(&self, position: &Value) -> usize {
        let line = position.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
        let character = position.get("character").and_then(Value::as_u64).unwrap_or(0) as usize;
        self.offset(line, character)
    }

    fn range_json(&self, start: usize, end: usize) -> Value {
        json!({"start": self.position_json(start), "end": self.position_json(end)})
    }
}

// ------------------------------------------------------------------ diagnostics

/// The range a diagnostic underlines for `span`. Never empty (an empty
/// range draws nothing in most editors) and never past the span's own
/// first line (an unterminated string would otherwise paint the rest of
/// the file).
///
/// One case moves the START: an error at the end of input ("unexpected
/// Eof", "expected `end`") has its span on the empty line after the final
/// newline, where there is nothing to underline. It is drawn on the last
/// character of actual text instead, which is where the missing piece
/// belongs.
fn diagnostic_range(doc: &Doc, span: Span) -> Value {
    let len = doc.text.len();
    let mut start = span.start.min(len);
    while !doc.text.is_char_boundary(start) {
        start -= 1;
    }
    let (line, _) = doc.position(start);
    let line_start = doc.line_starts[line];
    let line_end = line_start + doc.line(line).len();
    let mut end = span.end.min(line_end).max(start);
    if end == start {
        match doc.text[start..line_end].chars().next() {
            Some(c) => end = start + c.len_utf8(),
            // At the end of a line: underline the last character before
            // it, on this line if it has one, else on an earlier line.
            None => {
                let before = doc.text[..start].trim_end();
                match before.chars().next_back() {
                    Some(c) => {
                        end = before.len();
                        start = end - c.len_utf8();
                    }
                    // An empty document: (0,0)-(0,0) is all there is.
                    None => {}
                }
            }
        }
    }
    doc.range_json(start, end)
}

fn diagnostic(doc: &Doc, span: Span, message: &str) -> Value {
    json!({
        "range": diagnostic_range(doc, span),
        "severity": 1,
        "source": "qu",
        "message": message,
    })
}

/// Every lexical error if there are any (the lexer keeps going after one),
/// otherwise the parser's first error (it stops there) — the same order
/// `qu_syntax::parse` itself reports them in.
pub fn diagnostics(doc: &Doc) -> Vec<Value> {
    let (_, lex_errors) = qu_lexer::lex_checked(&doc.text);
    if !lex_errors.is_empty() {
        return lex_errors.iter().map(|e| diagnostic(doc, e.span, &e.msg)).collect();
    }
    // A parser panic on some half-typed input must not take the server
    // (and with it every other feature) down mid-keystroke.
    match std::panic::catch_unwind(|| qu_syntax::parse(&doc.text)) {
        Ok(Ok(_)) => Vec::new(),
        Ok(Err(e)) => vec![diagnostic(doc, e.span, &e.msg)],
        Err(_) => vec![json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "severity": 1,
            "source": "qu",
            "message": "internal error: the parser panicked on this input (please report it)",
        })],
    }
}

// ------------------------------------------------------------------ outline

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymKind {
    Function,
    Cell,
    Section,
}

/// One outline entry. Lines are 0-based here (`outline.ts` is 1-based).
#[derive(Clone, Debug)]
pub struct Sym {
    pub kind: SymKind,
    pub name: String,
    /// Parameter list for a function (`(a, b)`), otherwise empty.
    pub detail: String,
    pub line: usize,
    /// Inclusive last line.
    pub end_line: usize,
    /// Byte column of `name` within its line.
    pub col: usize,
}

fn ws_len(s: &str) -> usize {
    s.len() - s.trim_start_matches([' ', '\t']).len()
}

fn ident_len(s: &str) -> usize {
    let b = s.as_bytes();
    if b.is_empty() || !(b[0].is_ascii_alphabetic() || b[0] == b'_') {
        return 0;
    }
    b.iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count()
}

/// `[export] function name[(args]` -> (indent, name column, name, detail).
fn block_fn(line: &str) -> Option<(&str, usize, &str, &str)> {
    let indent = ws_len(line);
    let mut p = indent;
    if line[p..].starts_with("export") {
        let w = ws_len(&line[p + 6..]);
        if w > 0 {
            p += 6 + w;
        }
    }
    if !line[p..].starts_with("function") {
        return None;
    }
    p += 8;
    let w = ws_len(&line[p..]);
    if w == 0 {
        return None;
    }
    p += w;
    let n = ident_len(&line[p..]);
    if n == 0 {
        return None;
    }
    let (col, name) = (p, &line[p..p + n]);
    p += n;
    p += ws_len(&line[p..]);
    let detail = if line[p..].starts_with('(') {
        match line[p..].find(')') {
            Some(e) => &line[p..=p + e],
            None => line[p..].trim_end(),
        }
    } else {
        ""
    };
    Some((&line[..indent], col, name, detail))
}

/// `name(args) := expr` -> (name column, name, detail).
fn inline_fn(line: &str) -> Option<(usize, &str, &str)> {
    let indent = ws_len(line);
    let n = ident_len(&line[indent..]);
    if n == 0 {
        return None;
    }
    let mut p = indent + n;
    p += ws_len(&line[p..]);
    if !line[p..].starts_with('(') {
        return None;
    }
    let close = line[p + 1..].find([')', '('])?;
    if line.as_bytes()[p + 1 + close] != b')' {
        return None;
    }
    let detail = &line[p..=p + 1 + close];
    let rest = &line[p + 2 + close..];
    if !rest[ws_len(rest)..].starts_with(":=") {
        return None;
    }
    Some((indent, &line[indent..indent + n], detail))
}

fn is_end_function(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("end") else { return false };
    let w = ws_len(rest);
    if w == 0 {
        return false;
    }
    match rest[w..].strip_prefix("function") {
        Some(tail) => !tail.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'),
        None => false,
    }
}

/// A bare `end` (optionally followed by a comment): its indent, if so.
fn bare_end(line: &str) -> Option<&str> {
    let indent = ws_len(line);
    let rest = line[indent..].strip_prefix("end")?;
    let rest = rest.trim_start();
    (rest.is_empty() || rest.starts_with('#')).then(|| &line[..indent])
}

/// `# --- title ---` / `# === title` -> the title (possibly empty: a bare rule).
fn section_title(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let run = rest.len() - rest.trim_start_matches(['-', '=']).len();
    if run < 3 {
        return None;
    }
    Some(rest[run..].trim().trim_end_matches(['-', '=']).trim_end())
}

/// The document outline, in document order. A line scan rather than a
/// parse, for the reason `outline.ts` gives: it has to keep working on a
/// buffer that does not currently parse, which is most of the time while
/// typing.
pub fn scan_symbols(text: &str) -> Vec<Sym> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut symbols: Vec<Sym> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("#%%") {
            let title = rest.trim();
            symbols.push(Sym {
                kind: SymKind::Cell,
                name: if title.is_empty() { "Cell".to_string() } else { title.to_string() },
                detail: String::new(),
                line: i,
                end_line: i,
                col: 0,
            });
            continue;
        }
        if let Some(title) = section_title(line) {
            // A bare rule (`# ----------`) separates, but names nothing.
            if !title.is_empty() {
                symbols.push(Sym {
                    kind: SymKind::Section,
                    name: title.to_string(),
                    detail: String::new(),
                    line: i,
                    end_line: i,
                    col: line.find(title).unwrap_or(0),
                });
            }
            continue;
        }
        if let Some((indent, col, name, detail)) = block_fn(line) {
            let mut end = i;
            for (j, later) in lines.iter().enumerate().skip(i + 1) {
                // Another header before any `end function` means this one
                // is unterminated (mid-edit): it must not swallow the next.
                if block_fn(later).is_some() {
                    break;
                }
                if is_end_function(later) || bare_end(later) == Some(indent) {
                    end = j;
                    break;
                }
            }
            symbols.push(Sym {
                kind: SymKind::Function,
                name: name.to_string(),
                detail: detail.to_string(),
                line: i,
                end_line: end,
                col,
            });
            continue;
        }
        if let Some((col, name, detail)) = inline_fn(line) {
            symbols.push(Sym {
                kind: SymKind::Function,
                name: name.to_string(),
                detail: detail.to_string(),
                line: i,
                end_line: i,
                col,
            });
        }
    }

    // A cell or section runs until the next cell/section marker (a section
    // also stops at the next cell: cells are the coarser unit).
    let last = lines.len().saturating_sub(1);
    for k in 0..symbols.len() {
        if symbols[k].kind == SymKind::Function {
            continue;
        }
        let mut end = last;
        for next in &symbols[k + 1..] {
            if next.kind == SymKind::Cell || (symbols[k].kind == SymKind::Section && next.kind == SymKind::Section) {
                end = next.line.saturating_sub(1);
                break;
            }
        }
        symbols[k].end_line = end.max(symbols[k].line);
    }
    symbols
}

fn find_function<'a>(symbols: &'a [Sym], name: &str) -> Option<&'a Sym> {
    symbols.iter().find(|s| s.kind == SymKind::Function && s.name == name)
}

/// LSP `SymbolKind`: Function = 12, Module = 2 (a cell), Namespace = 3 (a section).
fn symbol_kind(kind: SymKind) -> u32 {
    match kind {
        SymKind::Function => 12,
        SymKind::Cell => 2,
        SymKind::Section => 3,
    }
}

fn symbol_ranges(doc: &Doc, s: &Sym) -> (Value, Value) {
    let start = doc.line_starts.get(s.line).copied().unwrap_or(0);
    let end_line = s.end_line.min(doc.line_count().saturating_sub(1));
    let end = doc.line_starts[end_line] + doc.line(end_line).len();
    let name_start = start + s.col;
    let name_end = match s.kind {
        SymKind::Function => name_start + s.name.len(),
        // A cell's or section's "name" is its whole header line.
        _ => start + doc.line(s.line).len(),
    };
    (doc.range_json(start, end), doc.range_json(name_start, name_end.max(name_start)))
}

/// Hierarchical `DocumentSymbol[]`: nested by containment
/// (cell > section > function), order preserved.
fn document_symbols_nested(doc: &Doc, symbols: &[Sym]) -> Value {
    struct Node {
        sym: Sym,
        children: Vec<Node>,
    }
    fn insert(nodes: &mut Vec<Node>, mut sym: Sym) {
        if let Some(parent) = nodes.last_mut() {
            if parent.sym.end_line >= sym.line && parent.sym.line < sym.line {
                // A child may never outrun its parent: clients reject such trees.
                sym.end_line = sym.end_line.min(parent.sym.end_line);
                insert(&mut parent.children, sym);
                return;
            }
        }
        nodes.push(Node { sym, children: Vec::new() });
    }
    fn render(doc: &Doc, nodes: &[Node]) -> Value {
        Value::Array(
            nodes
                .iter()
                .map(|n| {
                    let (range, selection) = symbol_ranges(doc, &n.sym);
                    let mut v = json!({
                        "name": n.sym.name,
                        "kind": symbol_kind(n.sym.kind),
                        "range": range,
                        "selectionRange": selection,
                    });
                    if !n.sym.detail.is_empty() {
                        v["detail"] = json!(n.sym.detail);
                    }
                    if !n.children.is_empty() {
                        v["children"] = render(doc, &n.children);
                    }
                    v
                })
                .collect(),
        )
    }
    let mut roots = Vec::new();
    for s in symbols {
        insert(&mut roots, s.clone());
    }
    render(doc, &roots)
}

/// Flat `SymbolInformation[]`, for a client that did not declare
/// `hierarchicalDocumentSymbolSupport`.
fn document_symbols_flat(doc: &Doc, uri: &str, symbols: &[Sym]) -> Value {
    Value::Array(
        symbols
            .iter()
            .map(|s| {
                let (range, _) = symbol_ranges(doc, s);
                json!({"name": s.name, "kind": symbol_kind(s.kind), "location": {"uri": uri, "range": range}})
            })
            .collect(),
    )
}

// ------------------------------------------------------------------ tokens

fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// The identifier or keyword token under (or ending exactly at) `offset`.
fn token_at(tokens: &[Token], offset: usize) -> Option<usize> {
    let named = |t: &Token| matches!(t.tok, Tok::Ident(_) | Tok::Keyword(_));
    tokens
        .iter()
        .position(|t| named(t) && t.span.start <= offset && offset < t.span.end)
        .or_else(|| tokens.iter().position(|t| named(t) && t.span.end == offset))
}

fn token_name(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Ident(s) => Some(s.as_str()),
        Tok::Keyword(k) => Some(k),
        _ => None,
    }
}

/// `module` when token `i` is the `name` of an unspaced `module.name`.
fn qualifier(tokens: &[Token], i: usize) -> Option<&str> {
    if i < 2 {
        return None;
    }
    let (dot, owner) = (&tokens[i - 1], &tokens[i - 2]);
    let adjacent = dot.span.end == tokens[i].span.start && owner.span.end == dot.span.start;
    match (&dot.tok, &owner.tok) {
        (Tok::Op("."), Tok::Ident(m)) if adjacent => Some(m.as_str()),
        _ => None,
    }
}

/// Is `offset` inside a comment or a string literal? Decided from the token
/// stream, so it follows the lexer's own idea of both: text on the line
/// that no token covers and that contains `#` can only be a comment.
fn in_comment_or_string(doc: &Doc, tokens: &[Token], offset: usize) -> bool {
    if tokens
        .iter()
        .any(|t| matches!(t.tok, Tok::Str(_) | Tok::RawStr(_)) && t.span.start < offset && offset < t.span.end)
    {
        return true;
    }
    let (line, _) = doc.position(offset);
    let line_start = doc.line_starts[line];
    let covered_to = tokens
        .iter()
        .filter(|t| !matches!(t.tok, Tok::Newline | Tok::Eof) && t.span.end <= offset && t.span.end > line_start)
        .map(|t| t.span.end)
        .max()
        .unwrap_or(line_start);
    doc.text[covered_to.min(offset)..offset].contains('#')
}

/// Names the document binds: assignment targets, loop variables and
/// function parameters, each with the span of its first binding. No
/// scoping: a completion list wants every name in the file.
fn variables(tokens: &[Token]) -> Vec<(String, Span)> {
    const ASSIGN: &[&str] = &["=", ":=", "+=", "-=", "*=", "/=", ".=", "^=", ".*=", "./="];
    let mut out: Vec<(String, Span)> = Vec::new();
    let mut add = |name: &str, span: Span| {
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_string(), span));
        }
    };
    let ident = |i: usize| match tokens.get(i).map(|t| &t.tok) {
        Some(Tok::Ident(s)) => Some(s.as_str()),
        _ => None,
    };
    let op = |i: usize, o: &str| matches!(tokens.get(i).map(|t| &t.tok), Some(Tok::Op(x)) if *x == o);

    for i in 0..tokens.len() {
        // Parameters: `function f(a, b)` and `f(a, b) := ...`.
        let params_from = match &tokens[i].tok {
            Tok::Keyword("function") if ident(i + 1).is_some() && op(i + 2, "(") => Some(i + 3),
            _ => None,
        };
        if let Some(mut j) = params_from {
            while let Some(name) = ident(j) {
                add(name, tokens[j].span);
                j += if op(j + 1, ",") { 2 } else { 1 };
            }
        }
        if matches!(tokens[i].tok, Tok::Keyword("for")) {
            if let Some(name) = ident(i + 1) {
                add(name, tokens[i + 1].span);
            }
        }

        let line_start = i == 0 || tokens[i - 1].span.line != tokens[i].span.line || matches!(tokens[i - 1].tok, Tok::Newline);
        if !line_start {
            continue;
        }
        let mut j = i;
        while matches!(tokens.get(j).map(|t| &t.tok), Some(Tok::Keyword("const" | "constant" | "global")))
            || ident(j) == Some("export")
        {
            j += 1;
        }
        // `a = ...`, `a, b = ...`
        let mut targets = Vec::new();
        let mut k = j;
        while ident(k).is_some() {
            targets.push(k);
            if op(k + 1, ",") {
                k += 2;
            } else {
                k += 1;
                break;
            }
        }
        if !targets.is_empty() && ASSIGN.iter().any(|a| op(k, a)) {
            for t in targets {
                add(ident(t).unwrap(), tokens[t].span);
            }
            continue;
        }
        // `f(a, b) := ...`: the parameters (the name itself is a function).
        if ident(j).is_some() && op(j + 1, "(") {
            let mut k = j + 2;
            let mut params = Vec::new();
            while ident(k).is_some() {
                params.push(k);
                k += if op(k + 1, ",") { 2 } else { 1 };
            }
            if op(k, ")") && op(k + 1, ":=") {
                for p in params {
                    add(ident(p).unwrap(), tokens[p].span);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ signatures

/// One call form parsed out of a documented signature.
#[derive(Debug, PartialEq)]
pub struct CallForm {
    pub label: String,
    /// Byte ranges of each parameter within `label`.
    pub params: Vec<(usize, usize)>,
}

/// Split at top level (outside brackets and string literals) on `sep`.
fn split_top_level(s: &str, sep: &str) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start, mut i) = (0i32, None::<u8>, 0usize, 0usize);
    let b = s.as_bytes();
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' {
            quote = Some(c);
        } else if matches!(c, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(c, b')' | b']' | b'}') {
            depth -= 1;
        } else if depth == 0 && s[i..].starts_with(sep) {
            parts.push((start, i));
            i += sep.len();
            start = i;
            continue;
        }
        i += 1;
    }
    parts.push((start, s.len()));
    parts
}

/// The call forms in a documented signature. `plot(y) / plot(x, y)` is two
/// forms; `a and b` (no parentheses) is none.
pub fn call_forms(signature: &str) -> Vec<CallForm> {
    let mut forms = Vec::new();
    for (s, e) in split_top_level(signature, " / ") {
        let label = signature[s..e].trim();
        let Some(open) = label.find('(') else { continue };
        // The matching close of that first paren.
        let mut depth = 0i32;
        let mut close = None;
        for (i, c) in label.char_indices().skip_while(|(i, _)| *i < open) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { continue };
        let inner = &label[open + 1..close];
        let mut params = Vec::new();
        for (ps, pe) in split_top_level(inner, ",") {
            let raw = &inner[ps..pe];
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lead = raw.len() - raw.trim_start().len();
            let start = open + 1 + ps + lead;
            params.push((start, start + trimmed.len()));
        }
        forms.push(CallForm { label: label.to_string(), params });
    }
    forms
}

/// The bare name of a documented parameter: `[bits=12]` -> `bits`.
fn param_name(label: &str) -> &str {
    let s = label.trim_start_matches(['[', ' ']);
    let end = s.find(|c: char| !is_word_char(c)).unwrap_or(s.len());
    &s[..end]
}

/// The call the cursor is inside, found by walking the tokens backwards.
struct CallSite {
    /// Callee name; `module.name` when written that way.
    name: String,
    bare: String,
    /// Top-level commas between the opening paren and the cursor.
    commas: usize,
    /// `name` of a `name=` argument the cursor is in, if any.
    named: Option<String>,
}

fn call_site(tokens: &[Token], offset: usize) -> Option<CallSite> {
    let before: Vec<usize> = (0..tokens.len())
        .filter(|i| tokens[*i].span.start < offset && !matches!(tokens[*i].tok, Tok::Eof))
        .collect();
    let (mut depth, mut commas) = (0usize, 0usize);
    // Index of the first token of the argument the cursor is in.
    let mut arg_start: Option<usize> = before.last().map(|l| l + 1);
    for &i in before.iter().rev() {
        match &tokens[i].tok {
            Tok::Op(")" | "]" | "}") => depth += 1,
            Tok::Op(open @ ("(" | "[" | "{")) => {
                if depth > 0 {
                    depth -= 1;
                    continue;
                }
                let callee = i.checked_sub(1).map(|p| &tokens[p]);
                let is_call = *open == "("
                    && callee.is_some_and(|c| matches!(c.tok, Tok::Ident(_)) && c.span.end == tokens[i].span.start);
                if !is_call {
                    // A grouping paren or a list literal: look further out.
                    commas = 0;
                    arg_start = None;
                    continue;
                }
                let bare = token_name(&tokens[i - 1]).unwrap_or("").to_string();
                let name = match qualifier(tokens, i - 1) {
                    Some(m) => format!("{m}.{bare}"),
                    None => bare.clone(),
                };
                let first = if commas == 0 { Some(i + 1) } else { arg_start };
                let named = first.and_then(|f| match (tokens.get(f).map(|t| &t.tok), tokens.get(f + 1).map(|t| &t.tok)) {
                    (Some(Tok::Ident(n)), Some(Tok::Op("="))) if tokens[f + 1].span.start < offset => Some(n.clone()),
                    _ => None,
                });
                return Some(CallSite { name, bare, commas, named });
            }
            Tok::Op(",") if depth == 0 => {
                if commas == 0 && arg_start.is_some() {
                    arg_start = Some(i + 1);
                }
                commas += 1;
            }
            // A statement boundary outside any bracket: not in a call.
            Tok::Newline if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

// ------------------------------------------------------------------ builtins

/// Every completable builtin name: `BUILTIN_NAMES` plus documented names
/// that are not in it (module members such as `xlsx.read`), each once.
fn builtin_names() -> impl Iterator<Item = &'static str> {
    qu_interp::BUILTIN_NAMES
        .iter()
        .copied()
        .chain(BUILTIN_DOCS.iter().map(|d| d.name).filter(|n| !qu_interp::BUILTIN_NAMES.contains(n)))
}

fn is_keyword(name: &str) -> bool {
    qu_lexer::KEYWORDS.contains(&name) || EXTRA_KEYWORDS.contains(&name)
}

fn builtin_markdown(name: &str, doc: &BuiltinDoc) -> String {
    let mut md = String::new();
    // Aliases share an entry (`acos` is documented as `asin(x)`): say which
    // name was asked about, so the signature is not read as a mismatch.
    if !doc.signature.contains(name) {
        md.push_str(&format!("**{name}** — documented with:\n\n"));
    }
    md.push_str(&format!("```qu\n{}\n```\n\n{}\n\n*Qu builtin · {}*", doc.signature, doc.summary, doc.chapter));
    md
}

/// LSP `CompletionItemKind`: Function = 3, Variable = 6, Keyword = 14, Constant = 21.
fn builtin_completion(label: &str, name: &str) -> Value {
    match builtin_doc(name) {
        Some(d) => json!({
            "label": label,
            "kind": if is_keyword(name) { 14 } else if d.signature.contains('(') { 3 } else { 21 },
            "detail": d.signature,
            "documentation": {"kind": "markdown", "value": format!("{}\n\n*Qu builtin · {}*", d.summary, d.chapter)},
            "sortText": format!("1{label}"),
        }),
        None => json!({"label": label, "kind": 3, "detail": "Qu builtin", "sortText": format!("1{label}")}),
    }
}

// ------------------------------------------------------------------ server

#[derive(Default)]
pub struct Server {
    docs: HashMap<String, Doc>,
    initialized: bool,
    shutdown: bool,
    hierarchical_symbols: bool,
    label_offsets: bool,
}

impl Server {
    /// Handle one incoming message. Returns the messages to send (a reply
    /// and/or notifications) and, after `exit`, the process exit code.
    pub fn handle(&mut self, msg: &Value) -> (Vec<Value>, Option<i32>) {
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response to something we sent. We send no requests.
            return (Vec::new(), None);
        };
        let id = msg.get("id").cloned();
        let null = Value::Null;
        let params = msg.get("params").unwrap_or(&null);
        let mut out = Vec::new();

        if method == "exit" {
            // The protocol's own rule: 0 only if `shutdown` came first.
            return (out, Some(if self.shutdown { 0 } else { 1 }));
        }

        let Some(id) = id else {
            if self.initialized && !self.shutdown {
                self.notification(method, params, &mut out);
            }
            return (out, None);
        };

        let result: Result<Value, (i64, String)> = if self.shutdown {
            Err((-32600, "the server has been shut down".to_string()))
        } else if method == "initialize" {
            Ok(self.initialize(params))
        } else if !self.initialized {
            Err((-32002, "the server has not been initialized".to_string()))
        } else {
            self.request(method, params)
        };
        out.push(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => rpc_error(id, code, &message),
        });
        (out, None)
    }

    fn initialize(&mut self, params: &Value) -> Value {
        self.initialized = true;
        let td = &params["capabilities"]["textDocument"];
        self.hierarchical_symbols = td["documentSymbol"]["hierarchicalDocumentSymbolSupport"].as_bool().unwrap_or(false);
        self.label_offsets =
            td["signatureHelp"]["signatureInformation"]["parameterInformation"]["labelOffsetSupport"].as_bool().unwrap_or(false);
        // Clients that only ask for completion on trigger characters (not on
        // every word character, as VS Code does) need the identifier
        // characters listed, plus `.` for `module.member`.
        let mut triggers: Vec<String> = ('a'..='z').chain('A'..='Z').map(String::from).collect();
        triggers.push("_".to_string());
        triggers.push(".".to_string());
        json!({
            "capabilities": {
                "positionEncoding": "utf-16",
                // 1 = full: every change carries the whole document.
                "textDocumentSync": {"openClose": true, "change": 1},
                "hoverProvider": true,
                "completionProvider": {"triggerCharacters": triggers},
                "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
                "documentSymbolProvider": true,
                "definitionProvider": true,
            },
            "serverInfo": {"name": "qu lsp", "version": env!("CARGO_PKG_VERSION")},
        })
    }

    fn publish(&self, uri: &str, out: &mut Vec<Value>) {
        let Some(doc) = self.docs.get(uri) else { return };
        let mut params = json!({"uri": uri, "diagnostics": diagnostics(doc)});
        if let Some(v) = doc.version {
            params["version"] = json!(v);
        }
        out.push(json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params}));
    }

    fn notification(&mut self, method: &str, params: &Value, out: &mut Vec<Value>) {
        let td = &params["textDocument"];
        let Some(uri) = td["uri"].as_str() else { return };
        match method {
            "textDocument/didOpen" => {
                let text = td["text"].as_str().unwrap_or("").to_string();
                self.docs.insert(uri.to_string(), Doc::new(text, td["version"].as_i64()));
                self.publish(uri, out);
            }
            "textDocument/didChange" => {
                // Full sync: the last change is the whole new text.
                let Some(text) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str())
                else {
                    return;
                };
                self.docs.insert(uri.to_string(), Doc::new(text.to_string(), td["version"].as_i64()));
                self.publish(uri, out);
            }
            "textDocument/didClose" => {
                self.docs.remove(uri);
                // Clear what was published, or the errors of a closed file
                // stay in the client's problem list.
                out.push(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {"uri": uri, "diagnostics": []},
                }));
            }
            // didSave, $/setTrace, $/cancelRequest, workspace/*: nothing to do.
            _ => {}
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        if method == "shutdown" {
            self.shutdown = true;
            return Ok(Value::Null);
        }
        let handler: fn(&Server, &str, &Doc, &Value) -> Value = match method {
            "textDocument/hover" => Server::hover,
            "textDocument/completion" => Server::completion,
            "textDocument/signatureHelp" => Server::signature_help,
            "textDocument/documentSymbol" => Server::document_symbol,
            "textDocument/definition" => Server::definition,
            _ => return Err((-32601, format!("method `{method}` is not supported"))),
        };
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or_else(|| (-32602, "params.textDocument.uri is missing".to_string()))?;
        let doc = self.docs.get(uri).ok_or_else(|| (-32602, format!("document `{uri}` is not open")))?;
        Ok(handler(self, uri, doc, params))
    }

    fn hover(&self, _uri: &str, doc: &Doc, params: &Value) -> Value {
        let offset = doc.offset_of(&params["position"]);
        let tokens = qu_lexer::lex(&doc.text);
        let Some(i) = token_at(&tokens, offset) else { return Value::Null };
        let name = token_name(&tokens[i]).unwrap_or("");
        let module = qualifier(&tokens, i);
        let range = doc.range_json(tokens[i].span.start, tokens[i].span.end);

        // A user function shadows a builtin of the same name, so it wins.
        if module.is_none() {
            let symbols = scan_symbols(&doc.text);
            if let Some(f) = find_function(&symbols, name) {
                let mut md = format!("```qu\n{}\n```", doc.line(f.line).trim());
                // The comment block directly above the header is its doc.
                let mut above: Vec<&str> = Vec::new();
                let mut l = f.line;
                while l > 0 && above.len() < 12 {
                    let t = doc.line(l - 1).trim();
                    if !t.starts_with('#') || t.starts_with("#%%") || section_title(t).is_some() {
                        break;
                    }
                    above.push(t.trim_start_matches('#').trim());
                    l -= 1;
                }
                if !above.is_empty() {
                    above.reverse();
                    md.push_str("\n\n");
                    md.push_str(&above.join("\n"));
                }
                md.push_str(&format!("\n\n*Defined in this file, line {}*", f.line + 1));
                return json!({"contents": {"kind": "markdown", "value": md}, "range": range});
            }
        }

        let qualified = module.map(|m| format!("{m}.{name}"));
        let found = qualified
            .as_deref()
            .and_then(|q| builtin_doc(q).map(|d| (q.to_string(), d)))
            .or_else(|| builtin_doc(name).map(|d| (name.to_string(), d)));
        match found {
            Some((shown, d)) => json!({"contents": {"kind": "markdown", "value": builtin_markdown(&shown, d)}, "range": range}),
            None if module.is_none() && qu_interp::BUILTIN_NAMES.contains(&name) => json!({
                "contents": {"kind": "markdown", "value": format!("`{name}` — Qu builtin (no reference entry yet)")},
                "range": range,
            }),
            None => Value::Null,
        }
    }

    fn completion(&self, _uri: &str, doc: &Doc, params: &Value) -> Value {
        let offset = doc.offset_of(&params["position"]);
        let empty = json!({"isIncomplete": false, "items": []});
        let tokens = qu_lexer::lex(&doc.text);
        if in_comment_or_string(doc, &tokens, offset) {
            return empty;
        }
        let head = &doc.text[..offset];
        let prefix_start = head.char_indices().rev().take_while(|(_, c)| is_word_char(*c)).last().map(|(i, _)| i).unwrap_or(offset);
        let prefix = &head[prefix_start..];
        // `5k`, `3.14`: a number (or a unit suffix on one), not a name.
        if prefix.starts_with(|c: char| c.is_ascii_digit()) {
            return empty;
        }
        let module = head[..prefix_start].strip_suffix('.').map(|before| {
            let start = before.char_indices().rev().take_while(|(_, c)| is_word_char(*c)).last().map(|(i, _)| i).unwrap_or(before.len());
            &before[start..]
        });
        if module.is_some_and(|m| m.is_empty() || m.starts_with(|c: char| c.is_ascii_digit())) {
            return empty;
        }
        let lower = prefix.to_lowercase();
        let matches = |name: &str| name.to_lowercase().starts_with(&lower);
        let mut items: Vec<Value> = Vec::new();

        if let Some(m) = module {
            // `xlsx.re|` -> the members of that module, labelled by member.
            let scope = format!("{m}.");
            items.extend(
                builtin_names()
                    .filter_map(|n| n.strip_prefix(scope.as_str()).filter(|member| matches(member)).map(|member| (member, n)))
                    .map(|(member, full)| builtin_completion(member, full)),
            );
            if items.is_empty() {
                // Not a module: `obj.me|` is a method call, and methods are
                // documented under their bare names.
                items.extend(builtin_names().filter(|n| !n.contains('.') && !is_keyword(n) && matches(n)).map(|n| builtin_completion(n, n)));
            }
            return json!({"isIncomplete": !prefix.is_empty(), "items": items});
        }

        let symbols = scan_symbols(&doc.text);
        let mut seen: Vec<&str> = Vec::new();
        for f in symbols.iter().filter(|s| s.kind == SymKind::Function && matches(&s.name)) {
            if seen.contains(&f.name.as_str()) {
                continue;
            }
            seen.push(&f.name);
            items.push(json!({
                "label": f.name,
                "kind": 3,
                "detail": format!("{}{}", f.name, f.detail),
                "documentation": {"kind": "markdown", "value": format!("Defined in this file, line {}", f.line + 1)},
                "sortText": format!("0{}", f.name),
            }));
        }
        let vars = variables(&tokens);
        for (name, span) in &vars {
            // The word being typed is not yet a variable to offer.
            let being_typed = span.start == prefix_start && span.end == offset;
            if being_typed || !matches(name) || seen.contains(&name.as_str()) {
                continue;
            }
            seen.push(name);
            items.push(json!({"label": name, "kind": 6, "detail": "variable", "sortText": format!("0{name}")}));
        }
        for name in builtin_names().filter(|n| !n.contains('.') && matches(n) && !seen.contains(n)) {
            items.push(builtin_completion(name, name));
        }
        for kw in qu_lexer::KEYWORDS.iter().chain(EXTRA_KEYWORDS).filter(|k| matches(k)) {
            if builtin_doc(kw).is_none() && !qu_interp::BUILTIN_NAMES.contains(kw) {
                items.push(json!({"label": kw, "kind": 14, "detail": "keyword", "sortText": format!("2{kw}")}));
            }
        }
        // Filtered by the prefix typed so far, so a longer list exists for
        // a shorter prefix: tell the client to ask again rather than
        // narrow this one.
        json!({"isIncomplete": !prefix.is_empty(), "items": items})
    }

    fn signature_help(&self, _uri: &str, doc: &Doc, params: &Value) -> Value {
        let offset = doc.offset_of(&params["position"]);
        let tokens = qu_lexer::lex(&doc.text);
        let Some(site) = call_site(&tokens, offset) else { return Value::Null };

        let symbols = scan_symbols(&doc.text);
        let user = if site.name == site.bare { find_function(&symbols, &site.bare) } else { None };
        let (forms, documentation) = match user {
            Some(f) if !f.detail.is_empty() => {
                (call_forms(&format!("{}{}", f.name, f.detail)), format!("Defined in this file, line {}", f.line + 1))
            }
            _ => match builtin_doc(&site.name).or_else(|| builtin_doc(&site.bare)) {
                Some(d) => (call_forms(d.signature), d.summary.to_string()),
                None => return Value::Null,
            },
        };
        if forms.is_empty() {
            return Value::Null;
        }

        let variadic = |f: &CallForm| f.params.last().is_some_and(|(s, e)| f.label[*s..*e].contains("..."));
        let named_index =
            |f: &CallForm| site.named.as_deref().and_then(|n| f.params.iter().position(|(s, e)| param_name(&f.label[*s..*e]) == n));
        // The first form that has room for the argument being typed.
        let active_form = forms
            .iter()
            .position(|f| named_index(f).is_some() || site.commas < f.params.len() || variadic(f))
            .unwrap_or(0);
        let form = &forms[active_form];
        let active_param = named_index(form).unwrap_or(site.commas).min(form.params.len().saturating_sub(1));

        let signatures: Vec<Value> = forms
            .iter()
            .map(|f| {
                let parameters: Vec<Value> = f
                    .params
                    .iter()
                    .map(|(s, e)| {
                        if self.label_offsets {
                            let a = f.label[..*s].encode_utf16().count();
                            let b = a + f.label[*s..*e].encode_utf16().count();
                            json!({"label": [a, b]})
                        } else {
                            json!({"label": &f.label[*s..*e]})
                        }
                    })
                    .collect();
                json!({
                    "label": f.label,
                    "documentation": {"kind": "markdown", "value": documentation},
                    "parameters": parameters,
                })
            })
            .collect();
        json!({"signatures": signatures, "activeSignature": active_form, "activeParameter": active_param})
    }

    fn document_symbol(&self, uri: &str, doc: &Doc, _params: &Value) -> Value {
        let symbols = scan_symbols(&doc.text);
        if self.hierarchical_symbols {
            document_symbols_nested(doc, &symbols)
        } else {
            document_symbols_flat(doc, uri, &symbols)
        }
    }

    fn definition(&self, uri: &str, doc: &Doc, params: &Value) -> Value {
        let offset = doc.offset_of(&params["position"]);
        let tokens = qu_lexer::lex(&doc.text);
        let Some(i) = token_at(&tokens, offset) else { return Value::Null };
        let Tok::Ident(name) = &tokens[i].tok else { return Value::Null };
        if qualifier(&tokens, i).is_some() {
            return Value::Null;
        }
        let symbols = scan_symbols(&doc.text);
        if let Some(f) = find_function(&symbols, name) {
            let (_, selection) = symbol_ranges(doc, f);
            return json!({"uri": uri, "range": selection});
        }
        match variables(&tokens).into_iter().find(|(n, _)| n == name) {
            Some((_, span)) => json!({"uri": uri, "range": doc.range_json(span.start, span.end)}),
            None => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "file:///t.qu";

    fn server_with(text: &str) -> (Server, Vec<Value>) {
        let mut s = Server::default();
        s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {"textDocument": {
            "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
            "signatureHelp": {"signatureInformation": {"parameterInformation": {"labelOffsetSupport": true}}},
        }}}}));
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {"uri": URI, "languageId": "qu", "version": 1, "text": text},
        }}));
        (s, out)
    }

    fn ask(s: &mut Server, method: &str, line: usize, character: usize) -> Value {
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 9, "method": method, "params": {
            "textDocument": {"uri": URI}, "position": {"line": line, "character": character},
        }}));
        out[0]["result"].clone()
    }

    #[test]
    fn framing_round_trips_and_reads_two_messages_back_to_back() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"a": "é"})).unwrap();
        write_message(&mut buf, &json!({"b": 2})).unwrap();
        let mut r = io::BufReader::new(&buf[..]);
        assert_eq!(serde_json::from_slice::<Value>(&read_message(&mut r).unwrap().unwrap()).unwrap(), json!({"a": "é"}));
        assert_eq!(serde_json::from_slice::<Value>(&read_message(&mut r).unwrap().unwrap()).unwrap(), json!({"b": 2}));
        assert!(read_message(&mut r).unwrap().is_none());
    }

    #[test]
    fn a_header_without_content_length_is_an_error_not_a_hang() {
        let mut r = io::BufReader::new(&b"Content-Type: x\r\n\r\n{}"[..]);
        assert!(read_message(&mut r).is_err());
    }

    #[test]
    fn positions_are_utf16_columns() {
        // `𝛼` is one char, four UTF-8 bytes, two UTF-16 units.
        let doc = Doc::new("a = \"𝛼\" + b\r\nsecond".to_string(), None);
        let b = doc.text.find(" + b").unwrap() + 3;
        assert_eq!(doc.position(b), (0, 11));
        assert_eq!(doc.offset(0, 11), b);
        assert_eq!(doc.offset(0, 999), doc.text.find('\r').unwrap(), "past the end clamps to the line end, before \\r");
        assert_eq!(doc.position(doc.text.find("second").unwrap()), (1, 0));
        assert_eq!(doc.offset(7, 0), doc.text.len());
    }

    #[test]
    fn a_clean_file_publishes_an_empty_list_and_a_broken_one_the_error_line() {
        let (_, out) = server_with("x = 1\ny = x + 2\n");
        assert_eq!(out[0]["method"], "textDocument/publishDiagnostics");
        assert_eq!(out[0]["params"]["diagnostics"], json!([]));

        let (mut s, out) = server_with("x = 1\ny = (2 +\nz = 3\n");
        let d = &out[0]["params"]["diagnostics"];
        assert_eq!(d.as_array().unwrap().len(), 1, "{d}");
        assert!(d[0]["range"]["start"]["line"].as_u64().unwrap() >= 1, "{d}");
        let (r, e) = (&d[0]["range"]["start"], &d[0]["range"]["end"]);
        assert!(r != e, "an empty range draws nothing: {d}");

        // An error at end of input is drawn on the last real character,
        // not on the empty line after it.
        let (_, out) = server_with("x = 1 +\n");
        let r = &out[0]["params"]["diagnostics"][0]["range"];
        assert_eq!(r["start"], json!({"line": 0, "character": 6}), "{r}");
        assert_eq!(r["end"], json!({"line": 0, "character": 7}), "{r}");
        let (_, out) = server_with("y = \"abc\nz = 2\n");
        let r = &out[0]["params"]["diagnostics"][0]["range"];
        assert_eq!(r["start"], json!({"line": 0, "character": 4}), "{r}");
        assert_eq!(r["end"]["line"], 0, "an unterminated string is underlined on its own line only: {r}");

        // Fixing the file clears it.
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
            "textDocument": {"uri": URI, "version": 2}, "contentChanges": [{"text": "x = 1\n"}],
        }}));
        assert_eq!(out[0]["params"]["diagnostics"], json!([]));
        assert_eq!(out[0]["params"]["version"], 2);
    }

    #[test]
    fn hover_shows_a_builtin_signature_and_a_user_function_header() {
        let src = "# Doubles it.\nfunction twice(x)\n    return 2 * x\nend function\ny = twice(butter(2, \"low\", 10, 100))\n# butter in a comment\n";
        let (mut s, _) = server_with(src);
        let h = ask(&mut s, "textDocument/hover", 4, 12);
        let md = h["contents"]["value"].as_str().unwrap();
        assert!(md.contains("butter(order, kind, cutoff, fs)"), "{md}");
        assert_eq!(h["range"]["start"]["character"], 10);
        assert_eq!(h["range"]["end"]["character"], 16);

        let h = ask(&mut s, "textDocument/hover", 4, 5);
        let md = h["contents"]["value"].as_str().unwrap();
        assert!(md.contains("function twice(x)") && md.contains("Doubles it."), "{md}");

        assert_eq!(ask(&mut s, "textDocument/hover", 5, 4), Value::Null, "a word in a comment is not code");
        assert_eq!(ask(&mut s, "textDocument/hover", 4, 2), Value::Null, "whitespace");
    }

    #[test]
    fn completion_offers_builtins_user_names_and_keywords_but_not_in_comments() {
        let src = "buffer_len = 4\nfunction build(x)\n    return x\nend function\ny = bu\n# bu\nwh\n";
        let (mut s, _) = server_with(src);
        let c = ask(&mut s, "textDocument/completion", 4, 6);
        let labels: Vec<&str> = c["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"butter"), "{labels:?}");
        assert!(labels.contains(&"buffer_len"), "{labels:?}");
        assert!(labels.contains(&"build"), "{labels:?}");
        assert!(labels.iter().all(|l| l.to_lowercase().starts_with("bu")), "{labels:?}");
        let butter = c["items"].as_array().unwrap().iter().find(|i| i["label"] == "butter").unwrap();
        assert_eq!(butter["detail"], "butter(order, kind, cutoff, fs)");
        assert!(butter["documentation"]["value"].as_str().unwrap().contains("Model"));

        assert_eq!(ask(&mut s, "textDocument/completion", 5, 4)["items"], json!([]));
        let c = ask(&mut s, "textDocument/completion", 6, 2);
        assert!(c["items"].as_array().unwrap().iter().any(|i| i["label"] == "while" && i["kind"] == 14), "{c}");
    }

    #[test]
    fn completion_after_a_module_dot_lists_that_modules_members() {
        let (mut s, _) = server_with("import codec\nw = codec.dec\n");
        let c = ask(&mut s, "textDocument/completion", 1, 13);
        let labels: Vec<&str> = c["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"decode_wav"), "{labels:?}");
        assert!(labels.iter().all(|l| l.starts_with("dec")), "{labels:?}");
    }

    #[test]
    fn signature_help_counts_top_level_commas_only() {
        let (mut s, _) = server_with("f = butter(2, max([1, 2]), \n");
        let h = ask(&mut s, "textDocument/signatureHelp", 0, 27);
        assert_eq!(h["signatures"][0]["label"], "butter(order, kind, cutoff, fs)");
        assert_eq!(h["activeParameter"], 2, "{h}");
        // Offsets of `cutoff` in the label.
        assert_eq!(h["signatures"][0]["parameters"][2]["label"], json!([20, 26]));

        // Inside the nested call it is that call's help.
        let h = ask(&mut s, "textDocument/signatureHelp", 0, 18);
        assert!(h["signatures"][0]["label"].as_str().unwrap().starts_with("max("), "{h}");

        // Outside any call: nothing.
        let (mut s, _) = server_with("x = 1 + 2\n");
        assert_eq!(ask(&mut s, "textDocument/signatureHelp", 0, 6), Value::Null);
    }

    #[test]
    fn signature_help_follows_a_named_argument_and_a_user_function() {
        let (mut s, _) = server_with("function mix(a, b, gain)\nend function\ny = mix(1, 2, \nz = adc(x, vmax=\n");
        let h = ask(&mut s, "textDocument/signatureHelp", 2, 14);
        assert_eq!(h["signatures"][0]["label"], "mix(a, b, gain)");
        assert_eq!(h["activeParameter"], 2);

        let h = ask(&mut s, "textDocument/signatureHelp", 3, 16);
        let sig = &h["signatures"][h["activeSignature"].as_u64().unwrap() as usize];
        let label = sig["label"].as_str().unwrap();
        let p = &sig["parameters"][h["activeParameter"].as_u64().unwrap() as usize]["label"];
        let (a, b) = (p[0].as_u64().unwrap() as usize, p[1].as_u64().unwrap() as usize);
        assert_eq!(&label[a..b], "[vmax=]", "{h}");
    }

    #[test]
    fn call_forms_split_alternatives_and_keep_bracketed_defaults_whole() {
        let forms = call_forms("plot(y) / plot(x, y) / plot(x, y, \"o\")");
        assert_eq!(forms.len(), 3);
        assert_eq!(forms[2].params.len(), 3);
        let f = &call_forms("imshow(m, [cmap=\"a,b\"], [lim=(0, 1)])")[0];
        let labels: Vec<&str> = f.params.iter().map(|(s, e)| &f.label[*s..*e]).collect();
        assert_eq!(labels, ["m", "[cmap=\"a,b\"]", "[lim=(0, 1)]"]);
        assert!(call_forms("a and b").is_empty());
    }

    #[test]
    fn the_outline_nests_cell_section_function() {
        let src = "#%% Setup\n# --- Filters ---\nfunction lp(x)\n    if x > 0\n        y = 1\n    end\n    return x\nend\ng(t) := t * 2\n#%% Plot\n# ----------\nplot(1)\n";
        let syms = scan_symbols(src);
        let got: Vec<(SymKind, &str, usize, usize)> = syms.iter().map(|s| (s.kind, s.name.as_str(), s.line, s.end_line)).collect();
        assert_eq!(
            got,
            [
                (SymKind::Cell, "Setup", 0, 8),
                (SymKind::Section, "Filters", 1, 8),
                // The nested `end` at a deeper indent does not close it.
                (SymKind::Function, "lp", 2, 7),
                (SymKind::Function, "g", 8, 8),
                (SymKind::Cell, "Plot", 9, 12),
            ]
        );
        let (mut s, _) = server_with(src);
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/documentSymbol", "params": {"textDocument": {"uri": URI}}}));
        let r = &out[0]["result"];
        assert_eq!(r.as_array().unwrap().len(), 2, "{r}");
        assert_eq!(r[0]["children"][0]["name"], "Filters");
        assert_eq!(r[0]["children"][0]["children"][0]["name"], "lp");
        assert_eq!(r[0]["children"][0]["children"][0]["detail"], "(x)");
        assert_eq!(r[0]["children"][0]["children"][0]["selectionRange"]["start"], json!({"line": 2, "character": 9}));
    }

    #[test]
    fn definition_jumps_to_a_function_and_to_a_first_assignment() {
        let (mut s, _) = server_with("gain = 3\nfunction amp(x)\n    return gain * x\nend function\ny = amp(gain)\n");
        let d = ask(&mut s, "textDocument/definition", 4, 5);
        assert_eq!(d["uri"], URI);
        assert_eq!(d["range"]["start"], json!({"line": 1, "character": 9}));
        let d = ask(&mut s, "textDocument/definition", 4, 9);
        assert_eq!(d["range"]["start"], json!({"line": 0, "character": 0}));
        assert_eq!(ask(&mut s, "textDocument/definition", 2, 5), Value::Null, "`return` is not a definition");
    }

    #[test]
    fn the_lifecycle_rules_hold() {
        let mut s = Server::default();
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/hover", "params": {}}));
        assert_eq!(out[0]["error"]["code"], -32002);
        let (_, exit) = Server::default().handle(&json!({"jsonrpc": "2.0", "method": "exit"}));
        assert_eq!(exit, Some(1), "exit without shutdown is an abnormal exit");

        let (mut s, _) = server_with("x = 1\n");
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "workspace/symbol", "params": {}}));
        assert_eq!(out[0]["error"]["code"], -32601);
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": 2}}));
        assert!(out.is_empty(), "a notification is never answered");
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"}));
        assert_eq!(out[0]["result"], Value::Null);
        assert!(out[0].get("result").is_some());
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "textDocument/hover", "params": {}}));
        assert_eq!(out[0]["error"]["code"], -32600);
        let (_, exit) = s.handle(&json!({"jsonrpc": "2.0", "method": "exit"}));
        assert_eq!(exit, Some(0));
    }

    #[test]
    fn closing_a_document_clears_its_diagnostics() {
        let (mut s, _) = server_with("x = (\n");
        let (out, _) = s.handle(&json!({"jsonrpc": "2.0", "method": "textDocument/didClose", "params": {"textDocument": {"uri": URI}}}));
        assert_eq!(out[0]["params"]["diagnostics"], json!([]));
    }
}
