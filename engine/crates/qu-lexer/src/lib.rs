//! Qu lexer.
//!
//! Turns Qu source text into a flat token stream, following the "Lexical
//! classes" section of `docs/qu-grammar.ebnf`. Design notes:
//!
//! * Statements are newline-terminated (spec §5). We emit an explicit
//!   [`Tok::Newline`] token, but *suppress* it when a line clearly continues:
//!   the previous significant token is a binary operator, an opening bracket,
//!   or a comma (so `f(a,\n b)` and `x = a +\n b` are single statements).
//! * `#` starts a line comment.
//! * Numbers cover Integer, Float (with `e`/`E` exponent) and Imaginary
//!   (`2i`). A number *immediately* followed by a known unit name becomes a
//!   [`Tok::UnitLit`] (e.g. `5 kHz`, `35 mOhm`); see [`is_unit`].
//! * Strings keep their raw inner text (interpolation `{...}` is resolved by
//!   the interpreter, not the lexer).
//! * Operators are matched longest-first so `:=`, `.*`, `<=`, `|>`, `->`,
//!   `+=` win over their single-char prefixes.
//!
//! The lexer is dependency-free (std only) and never panics on bad input: an
//! unrecognized character becomes [`Tok::Unknown`] and lexing continues, so a
//! caller can report every problem in one pass.

use std::fmt;

/// A source span as a half-open byte range `[start, end)`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Token kinds. `String` payloads keep the source spelling where it matters
/// (identifiers, raw string bodies); numbers are pre-parsed for convenience.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    // literals
    Int(i64),
    Float(f64),
    Imag(f64),
    /// value + unit name, e.g. `5 kHz` -> `UnitLit(5.0, "kHz")`.
    UnitLit(f64, String),
    /// raw inner text of a string literal (no surrounding quotes, unescaped
    /// lazily by the interpreter).
    Str(String),
    /// Body of an `r"..."` literal — reaches the interpreter verbatim, with
    /// neither escape decoding nor `{}` interpolation applied.
    ///
    /// A separate token rather than a flag on `Str` because the two are not
    /// one literal seen twice: `Str` is a template the interpreter still has
    /// to process, `RawStr` is finished text.
    RawStr(String),

    Ident(String),
    Keyword(&'static str),

    // punctuation / operators (see `OPERATORS`)
    Op(&'static str),

    Newline,
    Eof,

    /// An unrecognized character; carries it for diagnostics.
    Unknown(char),
}

/// A token together with its source span.
#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// Reserved words that are keywords in *any* position. Per the grammar's
/// austerity rule most domain vocabulary is *contextual* (plain idents that
/// only act as keywords in statement/clause-head position), so this list is
/// deliberately small — the parser promotes contextual words itself.
pub const KEYWORDS: &[&str] = &[
    "and", "or", "not", "mod", "in", "as", "to", "step", "skip", "by",
    "if", "then", "else", "elseif", "end", "for", "while", "do", "repeat",
    "until", "return", "function", "sub", "class", "new", "true", "false",
    "none", "backend", "with", "import", "const", "constant", "try", "catch",
    "unsafe", "global",
];

/// Multi-character and single-character operators, **longest first** so the
/// scanner takes the maximal munch (`:=` before `:`, `.*` before `.`,
/// `.*=` before `.*`). `match_operator` below checks up to 3 characters,
/// so any 3-char operator (`.*=`, `./=`) MUST be listed before whichever
/// shorter operator shares its prefix, or the shorter one would win.
pub const OPERATORS: &[&str] = &[
    // 3-char
    ".*=", "./=",
    // 2-char
    ":=", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", ".=", "^=", "=>",
    ".*", "./", ".\\", ".^", "|>", "->", "**", ".'", "??",
    // 1-char
    "+", "-", "*", "/", "\\", "^", "=", "<", ">", "|", "&",
    "(", ")", "[", "]", "{", "}", ",", ";", ":", ".", "'", "~", "@", "?",
];

/// Known unit names (grammar `UnitName`). A number immediately followed by one
/// of these becomes a [`Tok::UnitLit`]. We intentionally restrict to the known
/// set (not arbitrary idents) to avoid swallowing `2 x`-style adjacency.
pub const UNITS: &[&str] = &[
    "Hz", "kHz", "MHz", "GHz", "s", "ms", "us", "ns", "V", "mV", "kV",
    "A", "mA", "uA", "Ohm", "mOhm", "kOhm", "MOhm", "ohm", "kohm", "Mohm", "F", "uF", "nF", "pF",
    "H", "mH", "uH", "W", "mW", "kW", "dB", "dBm", "degC", "degF", "rad", "deg",
    "cycles", "samples", "MiB", "GiB", "KiB",
    // Length, SI base metre. Bare `m` is NOT here as a length: it is the
    // milli prefix below and has been since before lengths existed, so
    // `5 m` cannot silently change meaning under anyone's feet. See
    // `Interp::meter_mode` for how a script says which it wants.
    "km", "cm", "mm", "um", "nm",
    // bare SI magnitude prefixes (no base unit): pure numeric scaling, e.g.
    // `f = 50m` == `f = 0.05`, `n = 2k` == `n = 2000`.
    "k", "M", "G", "m", "u", "p", "a",
    // Derived units, nameable now that phase 3 (design doc §8) lets `*`/`/`
    // derive a dimension instead of erroring: energy (`J`, `Ws`, `kWh`) and
    // charge (`C`, `As`). `As` is safe as a unit spelling even though `as`
    // is a keyword: the lexer only ever emits a unit as a plain `Ident`
    // (case-sensitive), so `As` and `as` never collide.
    "J", "Ws", "kWh", "C", "As",
];

pub fn is_unit(s: &str) -> bool {
    UNITS.contains(&s)
}

fn keyword(s: &str) -> Option<&'static str> {
    KEYWORDS.iter().find(|k| **k == s).copied()
}

struct Lexer<'a> {
    src: &'a [u8],
    chars: Vec<(usize, char)>,
    i: usize,
    line: u32,
    col: u32,
    line_start: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            chars: src.char_indices().collect(),
            i: 0,
            line: 1,
            col: 1,
            line_start: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).map(|&(_, c)| c)
    }
    fn peek2(&self) -> Option<char> {
        self.chars.get(self.i + 1).map(|&(_, c)| c)
    }
    fn peek3(&self) -> Option<char> {
        self.chars.get(self.i + 2).map(|&(_, c)| c)
    }
    fn byte_pos(&self) -> usize {
        self.chars
            .get(self.i)
            .map(|&(b, _)| b)
            .unwrap_or(self.src.len())
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
            self.line_start = self.byte_pos();
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn span(&self, start_byte: usize, start_line: u32, start_col: u32) -> Span {
        Span {
            start: start_byte,
            end: self.byte_pos(),
            line: start_line,
            col: start_col,
        }
    }

    fn rest_str(&self, from: usize, to: usize) -> String {
        String::from_utf8_lossy(&self.src[from..to]).into_owned()
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}
fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Should a newline be suppressed (treated as continuation) given the last
/// significant token emitted?
fn suppress_newline_after(t: &Tok) -> bool {
    match t {
        Tok::Op(op) => matches!(
            *op,
            "+" | "-" | "*" | "/" | "\\" | "^" | "=" | "<" | ">" | "|" | "&"
                | ":=" | "==" | "!=" | "<=" | ">=" | "+=" | "-=" | "*=" | "/=" | ".=" | "^=" | "=>"
                | ".*" | "./" | ".\\" | ".^" | ".*=" | "./=" | "|>" | "->" | "**" | "..." | "," | "(" | "[" | "{" | ":"
        ),
        Tok::Keyword(k) => matches!(
            *k, "and" | "or" | "not" | "to" | "step" | "in" | "as" | "by" | "then" | "do"
        ),
        _ => false,
    }
}

/// Tokenize `src` into a vector ending in [`Tok::Eof`]. Never fails; lexical
/// problems surface as [`Tok::Unknown`] tokens.
pub fn lex(src: &str) -> Vec<Token> {
    let mut lx = Lexer::new(src);
    let mut out: Vec<Token> = Vec::new();

    // Track the last non-newline token to decide on continuations.
    let mut last_significant: Option<Tok> = None;

    loop {
        // skip spaces/tabs and carriage returns (not newlines)
        while matches!(lx.peek(), Some(' ') | Some('\t') | Some('\r')) {
            lx.bump();
        }

        let Some(c) = lx.peek() else { break };
        let (sb, sl, sc) = (lx.byte_pos(), lx.line, lx.col);

        // line comment
        if c == '#' {
            while let Some(ch) = lx.peek() {
                if ch == '\n' {
                    break;
                }
                lx.bump();
            }
            continue;
        }

        // newline
        if c == '\n' {
            lx.bump();
            let suppress = last_significant
                .as_ref()
                .map(suppress_newline_after)
                .unwrap_or(true); // suppress leading blank lines
            if !suppress {
                out.push(Token {
                    tok: Tok::Newline,
                    span: lx.span(sb, sl, sc),
                });
                last_significant = Some(Tok::Newline);
            }
            continue;
        }

        // number
        if c.is_ascii_digit() || (c == '.' && lx.peek2().is_some_and(|d| d.is_ascii_digit())) {
            let tok = lex_number(&mut lx);
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // ellipsis (comma-ellipsis ranges, `1, 2, ..., 50`): three dots.
        if c == '.' && lx.peek2() == Some('.') && lx.peek3() == Some('.') {
            lx.bump();
            lx.bump();
            lx.bump();
            let tok = Tok::Op("...");
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // raw string: `r"..."`, verbatim to the interpreter.
        //
        // Double quote only. `r'` is already the ctranspose of a variable
        // named `r`, which is an ordinary thing to have in a language with
        // `A'` — not worth breaking to buy a second spelling of the same
        // literal.
        //
        // Only fires at a token boundary and only with the quote directly
        // after the `r`, so `rate` and `r = 1` still reach the identifier
        // branch below.
        if c == 'r' && lx.peek2() == Some('"') {
            lx.bump(); // the `r`
            let tok = lex_raw_string(&mut lx);
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // string
        if c == '"' || c == '\'' {
            // a lone `'` after a value is ctranspose, not a string. Decide by
            // the previous token: if it could end an expression, treat `'` as op.
            if c == '\'' && ends_expr(last_significant.as_ref()) {
                lx.bump();
                let tok = Tok::Op("'");
                let span = lx.span(sb, sl, sc);
                last_significant = Some(tok.clone());
                out.push(Token { tok, span });
                continue;
            }
            let tok = lex_string(&mut lx, c);
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // identifier / keyword
        if is_ident_start(c) {
            let start = lx.i;
            while lx.peek().is_some_and(is_ident_continue) {
                lx.bump();
            }
            let text = lx.rest_str(sb, lx.byte_pos());
            let _ = start;
            let tok = match keyword(&text) {
                Some(k) => Tok::Keyword(k),
                None => Tok::Ident(text),
            };
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // operator (longest match)
        if let Some(op) = match_operator(&lx) {
            for _ in 0..op.chars().count() {
                lx.bump();
            }
            let tok = Tok::Op(op);
            let span = lx.span(sb, sl, sc);
            last_significant = Some(tok.clone());
            out.push(Token { tok, span });
            continue;
        }

        // unknown
        lx.bump();
        let tok = Tok::Unknown(c);
        let span = lx.span(sb, sl, sc);
        last_significant = Some(tok.clone());
        out.push(Token { tok, span });
    }

    out.push(Token {
        tok: Tok::Eof,
        span: lx.span(lx.byte_pos(), lx.line, lx.col),
    });
    out
}

/// Does the last token end an expression (so a following `'` is transpose)?
fn ends_expr(t: Option<&Tok>) -> bool {
    match t {
        Some(Tok::Int(_)) | Some(Tok::Float(_)) | Some(Tok::Imag(_))
        | Some(Tok::UnitLit(_, _)) | Some(Tok::Ident(_)) | Some(Tok::Str(_))
        | Some(Tok::RawStr(_)) => true,
        Some(Tok::Op(op)) => matches!(*op, ")" | "]" | "}" | "'"),
        _ => false,
    }
}

fn match_operator(lx: &Lexer) -> Option<&'static str> {
    let a = lx.peek()?;
    let b = lx.peek2();
    let c = lx.peek3();
    for op in OPERATORS {
        let mut it = op.chars();
        let o0 = it.next().unwrap();
        if o0 != a {
            continue;
        }
        let o1 = match it.next() {
            None => return Some(op),
            Some(o1) => o1,
        };
        if Some(o1) != b {
            continue;
        }
        match it.next() {
            None => return Some(op),
            // A genuine 3-char operator (`.*=`, `./=`): only matches if the
            // third character is present too — without this arm, `match_operator`
            // used to only ever compare the first two characters of ANY
            // operator, so a 3-char op would incorrectly fire on just its
            // first two (e.g. `.*` alone would have matched `.*=`'s slot).
            Some(o2) => {
                if Some(o2) == c {
                    return Some(op);
                }
            }
        }
    }
    None
}

fn lex_number(lx: &mut Lexer) -> Tok {
    let start = lx.byte_pos();
    let mut is_float = false;

    // hex literal: `0x`/`0X` followed by at least one hex digit. Checked
    // before the decimal path below so a bare `0` isn't consumed first.
    if lx.peek() == Some('0')
        && matches!(lx.peek2(), Some('x') | Some('X'))
        && lx.peek3().is_some_and(|c| c.is_ascii_hexdigit())
    {
        lx.bump(); // 0
        lx.bump(); // x/X
        while lx.peek().is_some_and(|c| c.is_ascii_hexdigit() || c == '_') {
            lx.bump();
        }
        let digits = lx.rest_str(start + 2, lx.byte_pos()).replace('_', "");
        return match i64::from_str_radix(&digits, 16) {
            Ok(n) => Tok::Int(n),
            Err(_) => Tok::Unknown('x'),
        };
    }

    // integer part
    while lx.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
        lx.bump();
    }
    // fraction
    if lx.peek() == Some('.') && lx.peek2().is_some_and(|c| c.is_ascii_digit()) {
        is_float = true;
        lx.bump(); // .
        while lx.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            lx.bump();
        }
    } else if lx.peek() == Some('.') && !lx.peek2().is_some_and(is_ident_start) {
        // trailing dot like `2.` (but not `2.field` / `2.method`)
        if lx.peek2() != Some('.') {
            is_float = true;
            lx.bump();
            while lx.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                lx.bump();
            }
        }
    }
    // exponent
    if matches!(lx.peek(), Some('e') | Some('E')) {
        let save = lx.i;
        lx.bump();
        if matches!(lx.peek(), Some('+') | Some('-')) {
            lx.bump();
        }
        if lx.peek().is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            while lx.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                lx.bump();
            }
        } else {
            // not an exponent after all (e.g. `2eq`), roll back
            lx.i = save;
        }
    }

    let raw = lx.rest_str(start, lx.byte_pos()).replace('_', "");

    // imaginary suffix: `i` (math) or `j` (engineering/Python) are both accepted
    if matches!(lx.peek(), Some('i') | Some('j')) && !lx.peek2().is_some_and(is_ident_continue) {
        lx.bump();
        let v = raw.parse::<f64>().unwrap_or(f64::NAN);
        return Tok::Imag(v);
    }

    // Note: unit suffixes (`5 kHz`, `35 mOhm`) are recognized by the *parser*
    // from a numeric literal followed by a known unit name (see qu-syntax),
    // so that spacing is irrelevant and `2 x` is never mis-read as a unit.

    if is_float {
        Tok::Float(raw.parse::<f64>().unwrap_or(f64::NAN))
    } else {
        match raw.parse::<i64>() {
            Ok(n) => Tok::Int(n),
            Err(_) => Tok::Float(raw.parse::<f64>().unwrap_or(f64::NAN)),
        }
    }
}

fn lex_string(lx: &mut Lexer, quote: char) -> Tok {
    lx.bump(); // opening quote
    let start = lx.byte_pos();
    let mut end = start;
    while let Some(c) = lx.peek() {
        if c == '\\' {
            lx.bump();
            lx.bump();
            end = lx.byte_pos();
            continue;
        }
        if c == quote {
            end = lx.byte_pos();
            lx.bump();
            break;
        }
        lx.bump();
        end = lx.byte_pos();
    }
    Tok::Str(lx.rest_str(start, end))
}

/// `r"..."` — everything up to the next `"`, with no escape handling at all.
///
/// Note the missing `if c == '\\'` arm that `lex_string` has: that arm is
/// the entire difference, and skipping it is what lets `r"C:\temp\new"`
/// arrive as a path rather than as a tab and a newline. It also means a raw
/// string cannot contain a double quote, and that a trailing backslash is
/// just a character — `r"C:\dir\"` is a path ending in a separator, where
/// Rust and Python both make that spelling a syntax error.
fn lex_raw_string(lx: &mut Lexer) -> Tok {
    lx.bump(); // opening quote
    let start = lx.byte_pos();
    let mut end = start;
    while let Some(c) = lx.peek() {
        if c == '"' {
            end = lx.byte_pos();
            lx.bump();
            break;
        }
        lx.bump();
        end = lx.byte_pos();
    }
    Tok::RawStr(lx.rest_str(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn arithmetic() {
        let k = kinds("x = 2 + 3*4");
        assert_eq!(
            k,
            vec![
                Tok::Ident("x".into()),
                Tok::Op("="),
                Tok::Int(2),
                Tok::Op("+"),
                Tok::Int(3),
                Tok::Op("*"),
                Tok::Int(4),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn floats_and_exponent() {
        assert_eq!(kinds("5e3")[0], Tok::Float(5000.0));
        assert_eq!(kinds("1.5")[0], Tok::Float(1.5));
        assert_eq!(kinds(".25")[0], Tok::Float(0.25));
        assert_eq!(kinds("42")[0], Tok::Int(42));
    }

    #[test]
    fn hex_literals() {
        assert_eq!(kinds("0xFF")[0], Tok::Int(255));
        assert_eq!(kinds("0xff")[0], Tok::Int(255));
        assert_eq!(kinds("0X1a")[0], Tok::Int(26));
        assert_eq!(kinds("0x00")[0], Tok::Int(0));
        assert_eq!(kinds("0xDEAD_BEEF")[0], Tok::Int(0xDEADBEEFu32 as i64));
        // a bare `0` followed by an identifier starting with `x` is NOT a hex
        // literal without a hex digit right after the `x` (e.g. `0 xhat`-style
        // adjacency stays two tokens, matching the unit-suffix precedent).
        let k = kinds("0xhat");
        assert_eq!(k[0], Tok::Int(0));
        assert_eq!(k[1], Tok::Ident("xhat".into()));
    }

    #[test]
    fn number_then_unit_word_are_separate_tokens() {
        // The lexer keeps them separate; the parser fuses number + unit name.
        let k = kinds("5 kHz");
        assert_eq!(k[0], Tok::Int(5));
        assert_eq!(k[1], Tok::Ident("kHz".into()));
        // adjacency without a space tokenizes the same way
        let k2 = kinds("200ms");
        assert_eq!(k2[0], Tok::Int(200));
        assert_eq!(k2[1], Tok::Ident("ms".into()));
        assert!(is_unit("kHz") && is_unit("mOhm") && !is_unit("x"));
    }

    #[test]
    fn raw_strings_keep_backslashes_and_braces_as_written() {
        assert_eq!(kinds(r#"r"C:\temp\new""#)[0], Tok::RawStr(r"C:\temp\new".into()));
        assert_eq!(kinds(r#"r"{a}""#)[0], Tok::RawStr("{a}".into()));
        // A trailing backslash cannot escape the closing quote, because no
        // escape handling runs inside a raw string at all.
        assert_eq!(kinds(r#"r"C:\dir\""#)[0], Tok::RawStr(r"C:\dir\".into()));
    }

    #[test]
    fn an_identifier_starting_with_r_is_not_a_raw_string() {
        // The raw-string branch fires only at a token boundary and only
        // with the quote directly after the `r`.
        assert_eq!(kinds("rate")[0], Tok::Ident("rate".into()));
        assert_eq!(kinds("r = 1")[0], Tok::Ident("r".into()));
        // `r'` stays transpose: raw strings are double-quote only.
        assert_eq!(kinds("r'")[1], Tok::Op("'"));
    }

    #[test]
    fn multichar_operators() {
        assert_eq!(kinds("a := b")[1], Tok::Op(":="));
        assert_eq!(kinds("a .* b")[1], Tok::Op(".*"));
        assert_eq!(kinds("a |> b")[1], Tok::Op("|>"));
        assert_eq!(kinds("a <= b")[1], Tok::Op("<="));
    }

    #[test]
    fn elementwise_and_power_compound_assign_operators() {
        // `.*=`/`./=` are genuine 3-char tokens — `match_operator` used to
        // only ever compare the first two characters of any candidate
        // operator, so before its fix these would have matched the
        // shorter `.*`/`./` slot instead (see `match_operator`'s own
        // doc comment). Checking these lex as ONE token (not `.*` then a
        // separate `=`) is the real regression coverage.
        assert_eq!(kinds("a .*= b")[1], Tok::Op(".*="));
        assert_eq!(kinds("a ./= b")[1], Tok::Op("./="));
        assert_eq!(kinds("a ^= b")[1], Tok::Op("^="));
        // and the plain (non-assign) elementwise/power operators are
        // still unaffected, still exactly 1 token each.
        assert_eq!(kinds("a .* b")[1], Tok::Op(".*"));
        assert_eq!(kinds("a ./ b")[1], Tok::Op("./"));
        assert_eq!(kinds("a ^ b")[1], Tok::Op("^"));
    }

    #[test]
    fn newline_continuation() {
        // binary operator at end of line suppresses the newline
        let k = kinds("a = 1 +\n2");
        assert!(!k.contains(&Tok::Newline));
        // a complete statement keeps its newline
        let k2 = kinds("a = 1\nb = 2");
        assert!(k2.contains(&Tok::Newline));
    }

    #[test]
    fn string_and_transpose() {
        assert_eq!(kinds("\"hi {x}\"")[0], Tok::Str("hi {x}".into()));
        // trailing quote after an identifier is transpose, not a string
        let k = kinds("M'");
        assert_eq!(k[0], Tok::Ident("M".into()));
        assert_eq!(k[1], Tok::Op("'"));
    }

    #[test]
    fn comments_skipped() {
        let k = kinds("x = 1 # trailing\ny = 2");
        assert!(k.contains(&Tok::Ident("y".into())));
    }

    #[test]
    fn comma_ellipsis_range_tokens() {
        // ported from the retired root `qu-parser` scaffold's `test_ellipsis`
        // (see IMPL.md C1 gate item 4) — the one scenario it covered that
        // this crate's own suite didn't yet have a dedicated lexer-level test
        // for.
        let k = kinds("1, 2, ..., 50");
        assert_eq!(
            k,
            vec![
                Tok::Int(1),
                Tok::Op(","),
                Tok::Int(2),
                Tok::Op(","),
                Tok::Op("..."),
                Tok::Op(","),
                Tok::Int(50),
                Tok::Eof,
            ]
        );
    }
}
