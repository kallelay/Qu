//! `qu editors check`: are the embedded plugin files valid, and do their
//! hand-written keyword lists still match the language?
//!
//! Every file is parsed by a real parser (JSON via serde_json; a small XML
//! well-formedness checker; a structural YAML check for the .sublime-syntax;
//! line checks for the Vim files). The drift check compares each grammar's
//! highlighted words with `qu_lexer::KEYWORDS` and its unit list with
//! `qu_lexer::UNITS`, and finally the real lexer, parser and interpreter run a
//! sample `.qu` file to prove the sample used for the comparison is valid Qu.

use crate::editors_assets::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sev {
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub sev: Sev,
    pub detail: String,
}

fn ck(name: &str, r: Result<String, String>) -> Check {
    match r {
        Ok(d) => Check { name: name.to_string(), sev: Sev::Ok, detail: d },
        Err(d) => Check { name: name.to_string(), sev: Sev::Fail, detail: d },
    }
}

fn asset(list: &'static [Asset], rel: &str) -> &'static str {
    list.iter().find(|a| a.rel == rel).map(|a| a.data).unwrap_or("")
}

// ---------------------------------------------------------------- XML

/// Minimal XML well-formedness: balanced and matching tags, quoted
/// attributes, legal entity references, comments without `--`, one root.
pub fn xml_wellformed(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let line_of = |i: usize| s[..i.min(s.len())].matches('\n').count() + 1;
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();
    let mut roots = 0;
    let check_entities = |text: &str, at: usize| -> Result<(), String> {
        let mut rest = text;
        while let Some(p) = rest.find('&') {
            let after = &rest[p + 1..];
            let Some(semi) = after.find(';') else {
                return Err(format!("line {}: '&' without ';'", line_of(at)));
            };
            let ent = &after[..semi];
            let ok = matches!(ent, "amp" | "lt" | "gt" | "quot" | "apos")
                || (ent.starts_with("#x") && ent.len() > 2 && ent[2..].chars().all(|c| c.is_ascii_hexdigit()))
                || (ent.starts_with('#') && ent.len() > 1 && ent[1..].chars().all(|c| c.is_ascii_digit()));
            if !ok {
                return Err(format!("line {}: unknown entity &{ent};", line_of(at)));
            }
            rest = &after[semi + 1..];
        }
        Ok(())
    };
    while i < b.len() {
        if b[i] != b'<' {
            let next = s[i..].find('<').map(|p| i + p).unwrap_or(b.len());
            let text = &s[i..next];
            if stack.is_empty() && !text.trim().is_empty() {
                return Err(format!("line {}: text outside the root element", line_of(i)));
            }
            check_entities(text, i)?;
            i = next;
            continue;
        }
        if s[i..].starts_with("<!--") {
            let Some(end) = s[i + 4..].find("-->") else {
                return Err(format!("line {}: unterminated comment", line_of(i)));
            };
            if s[i + 4..i + 4 + end].contains("--") {
                return Err(format!("line {}: '--' inside a comment", line_of(i)));
            }
            i += 4 + end + 3;
        } else if s[i..].starts_with("<?") {
            let Some(end) = s[i..].find("?>") else {
                return Err(format!("line {}: unterminated <? ?>", line_of(i)));
            };
            i += end + 2;
        } else if s[i..].starts_with("<![CDATA[") {
            let Some(end) = s[i..].find("]]>") else {
                return Err(format!("line {}: unterminated CDATA", line_of(i)));
            };
            i += end + 3;
        } else if s[i..].starts_with("<!") {
            let Some(end) = s[i..].find('>') else {
                return Err(format!("line {}: unterminated <!", line_of(i)));
            };
            i += end + 1;
        } else if s[i..].starts_with("</") {
            let Some(end) = s[i..].find('>') else {
                return Err(format!("line {}: unterminated closing tag", line_of(i)));
            };
            let name = s[i + 2..i + end].trim();
            match stack.pop() {
                Some(open) if open == name => {}
                Some(open) => return Err(format!("line {}: </{name}> closes <{open}>", line_of(i))),
                None => return Err(format!("line {}: </{name}> with nothing open", line_of(i))),
            }
            i += end + 1;
        } else {
            // open tag
            let start = i;
            i += 1;
            let ns = i;
            while i < b.len() && !(b[i] as char).is_whitespace() && b[i] != b'>' && b[i] != b'/' {
                i += 1;
            }
            let name = s[ns..i].to_string();
            if name.is_empty() {
                return Err(format!("line {}: '<' not followed by a tag name", line_of(start)));
            }
            let self_close;
            loop {
                while i < b.len() && (b[i] as char).is_whitespace() {
                    i += 1;
                }
                if i >= b.len() {
                    return Err(format!("line {}: unterminated tag <{name}", line_of(start)));
                }
                if b[i] == b'>' {
                    self_close = false;
                    i += 1;
                    break;
                }
                if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'>' {
                    self_close = true;
                    i += 2;
                    break;
                }
                let an = i;
                while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                if an == i {
                    return Err(format!("line {}: bad attribute in <{name}>", line_of(i)));
                }
                while i < b.len() && (b[i] as char).is_whitespace() {
                    i += 1;
                }
                if i >= b.len() || b[i] != b'=' {
                    return Err(format!("line {}: attribute {} without '='", line_of(i), &s[an..i.min(s.len())]));
                }
                i += 1;
                while i < b.len() && (b[i] as char).is_whitespace() {
                    i += 1;
                }
                if i >= b.len() || (b[i] != b'"' && b[i] != b'\'') {
                    return Err(format!("line {}: attribute value in <{name}> is not quoted", line_of(i)));
                }
                let q = b[i];
                let vs = i + 1;
                let Some(ve) = s[vs..].find(q as char) else {
                    return Err(format!("line {}: unterminated attribute value", line_of(i)));
                };
                let val = &s[vs..vs + ve];
                if val.contains('<') {
                    return Err(format!("line {}: '<' inside an attribute value", line_of(i)));
                }
                check_entities(val, vs)?;
                i = vs + ve + 1;
            }
            if stack.is_empty() {
                roots += 1;
                if roots > 1 {
                    return Err(format!("line {}: more than one root element", line_of(start)));
                }
            }
            if !self_close {
                stack.push(name);
            }
        }
    }
    if let Some(open) = stack.pop() {
        return Err(format!("<{open}> is never closed"));
    }
    if roots == 0 {
        return Err("no root element".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- YAML (.sublime-syntax)

/// Structural check of a .sublime-syntax: header, 2-space context names
/// under `contexts:`, a `main` context, and every `include:`/`push:` naming a
/// context that exists.
pub fn sublime_syntax_check(text: &str) -> Result<String, String> {
    if !text.starts_with("%YAML") {
        return Err("does not start with %YAML".into());
    }
    let mut contexts: BTreeSet<String> = BTreeSet::new();
    let mut in_contexts = false;
    let mut has = (false, false, false); // name, scope, file_extensions
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        if line.starts_with('\t') || line.trim_start_matches(' ').starts_with('\t') {
            return Err(format!("line {n}: tab in indentation"));
        }
        if line.starts_with("name:") {
            has.0 = true;
        }
        if line.starts_with("scope:") {
            has.1 = true;
        }
        if line.starts_with("file_extensions:") {
            has.2 = true;
        }
        if line.starts_with("contexts:") {
            in_contexts = true;
            continue;
        }
        if in_contexts && !line.starts_with(' ') && !line.trim().is_empty() && !line.starts_with('#') {
            in_contexts = false;
        }
        if in_contexts && line.starts_with("  ") && !line.starts_with("   ") {
            let t = line.trim();
            if let Some(name) = t.strip_suffix(':') {
                if !name.starts_with('#') && !name.starts_with('-') {
                    contexts.insert(name.to_string());
                }
            }
        }
        if let Some(rest) = line.trim_start().strip_prefix("- match:") {
            let v = rest.trim();
            let q = v.chars().next().unwrap_or(' ');
            if (q == '\'' || q == '"') && (v.len() < 2 || !v.ends_with(q)) {
                return Err(format!("line {n}: unbalanced quote in match: {v}"));
            }
        }
    }
    if !(has.0 && has.1 && has.2) {
        return Err("missing name:, scope: or file_extensions:".into());
    }
    if !contexts.contains("main") {
        return Err("no `contexts: main:`".into());
    }
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start().trim_start_matches("- ");
        for key in ["include:", "push:"] {
            if let Some(v) = t.strip_prefix(key) {
                let v = v.trim().trim_matches(|c| c == '\'' || c == '"');
                if v.is_empty() || v.starts_with('[') || v.contains('.') || v.contains('/') {
                    continue; // list push / external syntax reference
                }
                if !contexts.contains(v) {
                    return Err(format!("line {}: {key} {v} names no context", n + 1));
                }
            }
        }
    }
    Ok(format!("{} contexts, main present", contexts.len()))
}

// ---------------------------------------------------------------- word extraction

fn alt_words(re: &str) -> Vec<String> {
    let (Some(a), Some(b)) = (re.find('('), re.rfind(')')) else { return Vec::new() };
    if b <= a {
        return Vec::new();
    }
    let inner = re[a + 1..b].trim_start_matches("?:");
    inner.split('|').filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).map(String::from).collect()
}

pub struct Extracted {
    pub words: BTreeSet<String>,
    pub units: Option<BTreeSet<String>>,
}

pub fn tm_words(text: &str) -> Result<Extracted, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut words = BTreeSet::new();
    for rule in ["keywords", "constants"] {
        let pats = v.pointer(&format!("/repository/{rule}/patterns")).and_then(|p| p.as_array()).ok_or_else(|| format!("no repository.{rule}.patterns"))?;
        for p in pats {
            if let Some(m) = p.get("match").and_then(|m| m.as_str()) {
                words.extend(alt_words(m));
            }
        }
    }
    let units = text.find("(Hz|").and_then(|a| {
        let tail = &text[a + 1..];
        tail.find(')').map(|e| tail[..e].split('|').map(String::from).collect::<BTreeSet<_>>())
    });
    Ok(Extracted { words, units })
}

pub fn sublime_words(text: &str) -> Extracted {
    let mut words = BTreeSet::new();
    let mut ctx = String::new();
    for line in text.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            ctx = line.trim().trim_end_matches(':').to_string();
        }
        if (ctx == "keywords" || ctx == "constants") && line.contains("- match: '") {
            if let (Some(a), Some(b)) = (line.find('\''), line.rfind('\'')) {
                if b > a {
                    words.extend(alt_words(&line[a + 1..b].replace("''", "'")));
                }
            }
        }
    }
    let units = text.find("(Hz|").and_then(|a| {
        let tail = &text[a + 1..];
        tail.find(')').map(|e| tail[..e].split('|').map(String::from).collect::<BTreeSet<_>>())
    });
    Extracted { words, units }
}

fn udl_list(text: &str, name: &str) -> Vec<String> {
    let key = format!("<Keywords name=\"{name}\">");
    let Some(a) = text.find(&key) else { return Vec::new() };
    let rest = &text[a + key.len()..];
    let Some(b) = rest.find("</Keywords>") else { return Vec::new() };
    rest[..b].split_whitespace().map(String::from).collect()
}

pub fn udl_words(text: &str) -> Extracted {
    let mut words = BTreeSet::new();
    for n in ["Keywords1", "Keywords2", "Keywords4"] {
        words.extend(udl_list(text, n));
    }
    let u: BTreeSet<String> = udl_list(text, "Numbers, suffix2").into_iter().collect();
    Extracted { words, units: if u.is_empty() { None } else { Some(u) } }
}

pub fn vim_words(text: &str) -> Extracted {
    let mut words = BTreeSet::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some("syn") && it.next() == Some("keyword") {
            if let Some(g) = it.next() {
                if matches!(g, "quControl" | "quKeyword" | "quBoolean") {
                    words.extend(it.map(String::from));
                }
            }
        }
    }
    let units = text.find("\\%(Hz\\|").and_then(|a| {
        let tail = &text[a + 3..];
        tail.find("\\)").map(|e| tail[..e].split("\\|").map(String::from).collect::<BTreeSet<_>>())
    });
    Extracted { words, units }
}

#[derive(Debug, Clone)]
pub struct Drift {
    pub grammar: &'static str,
    /// Lexer keywords the grammar does not highlight.
    pub missing: Vec<String>,
    /// Highlighted as keywords but are builtin FUNCTION names (so `read(...)`
    /// would be coloured as a keyword).
    pub builtin_as_keyword: Vec<String>,
    /// Highlighted words that are neither lexer keywords nor builtins
    /// (contextual statement words the parser promotes: informational).
    pub contextual_extra: Vec<String>,
    pub unit_missing: Vec<String>,
    pub unit_extra: Vec<String>,
    pub word_count: usize,
}

pub fn drift_of(grammar: &'static str, ex: &Extracted) -> Drift {
    let kw: BTreeSet<&str> = qu_lexer::KEYWORDS.iter().copied().collect();
    let builtins: BTreeSet<&str> = qu_interp::BUILTIN_NAMES.iter().copied().collect();
    let missing = kw.iter().filter(|k| !ex.words.contains(**k)).map(|k| k.to_string()).collect();
    let mut builtin_as_keyword = Vec::new();
    let mut contextual_extra = Vec::new();
    for w in &ex.words {
        if kw.contains(w.as_str()) {
            continue;
        }
        if builtins.contains(w.as_str()) {
            builtin_as_keyword.push(w.clone());
        } else {
            contextual_extra.push(w.clone());
        }
    }
    let want: BTreeSet<&str> = qu_lexer::UNITS.iter().copied().collect();
    let (unit_missing, unit_extra) = match &ex.units {
        Some(u) => (
            want.iter().filter(|w| !u.contains(**w)).map(|w| w.to_string()).collect(),
            u.iter().filter(|w| !want.contains(w.as_str())).cloned().collect(),
        ),
        None => (vec!["(no unit list found)".to_string()], Vec::new()),
    };
    Drift { grammar, missing, builtin_as_keyword, contextual_extra, unit_missing, unit_extra, word_count: ex.words.len() }
}

/// Drift report for every embedded grammar.
pub fn drift_reports() -> Vec<Drift> {
    let mut v = Vec::new();
    match tm_words(asset(VSCODE_FILES, "syntaxes/qu.tmLanguage.json")) {
        Ok(ex) => v.push(drift_of("vscode tmLanguage", &ex)),
        Err(e) => v.push(Drift {
            grammar: "vscode tmLanguage",
            missing: vec![format!("unreadable: {e}")],
            builtin_as_keyword: vec![],
            contextual_extra: vec![],
            unit_missing: vec![],
            unit_extra: vec![],
            word_count: 0,
        }),
    }
    v.push(drift_of("sublime syntax", &sublime_words(asset(SUBLIME_FILES, "Qu.sublime-syntax"))));
    v.push(drift_of("notepad++ udl", &udl_words(asset(NOTEPADPP_FILES, "Qu.udl.xml"))));
    v.push(drift_of("vim syntax", &vim_words(asset(VIM_FILES, "syntax/qu.vim"))));
    v
}

fn drift_check(d: &Drift) -> Check {
    let name = format!("keywords vs lexer: {}", d.grammar);
    let mut problems = Vec::new();
    if !d.missing.is_empty() {
        problems.push(format!("lexer keywords not highlighted: {}", d.missing.join(" ")));
    }
    if !d.unit_missing.is_empty() {
        problems.push(format!("units missing: {}", d.unit_missing.join(" ")));
    }
    if !d.unit_extra.is_empty() {
        problems.push(format!("units not in the lexer: {}", d.unit_extra.join(" ")));
    }
    if !problems.is_empty() {
        return Check { name, sev: Sev::Fail, detail: problems.join("; ") };
    }
    let mut detail = format!("{} words, all {} lexer keywords and {} units covered", d.word_count, qu_lexer::KEYWORDS.len(), qu_lexer::UNITS.len());
    let mut sev = Sev::Ok;
    if !d.builtin_as_keyword.is_empty() {
        sev = Sev::Warn;
        detail.push_str(&format!("; also coloured as keywords but are builtin functions: {}", d.builtin_as_keyword.join(" ")));
    }
    Check { name, sev, detail }
}

// ---------------------------------------------------------------- sample

pub const SAMPLE_QU: &str = r#"#%% cell one
x = 5 mV
raw = r"""a "quoted" {braces}"""
m = [1, 2; 3, 4]'
function twice(a)
    return a * 2
end
total = 0
for i in 1:3
    total = total + twice(i)
end
msg = "total {total}"
if total > 5 and not false
    ok = true
else
    ok = false
end
nothing_here = none
"#;

/// Run the REAL lexer, parser and interpreter on the sample, and check every
/// keyword token it produced is highlighted by every grammar.
pub fn sample_check() -> Vec<Check> {
    let mut out = Vec::new();
    let toks = qu_lexer::lex(SAMPLE_QU);
    let unknown: Vec<String> = toks
        .iter()
        .filter_map(|t| if let qu_lexer::Tok::Unknown(c) = &t.tok { Some(c.to_string()) } else { None })
        .collect();
    out.push(ck(
        "sample .qu: lexer",
        if unknown.is_empty() { Ok(format!("{} tokens, no unknown characters", toks.len())) } else { Err(format!("unknown characters: {}", unknown.join(" "))) },
    ));
    out.push(ck(
        "sample .qu: parser",
        match qu_syntax::parse(SAMPLE_QU) {
            Ok(p) => Ok(format!("{} statements", p.stmts.len())),
            Err(e) => Err(format!("{} (at {}:{})", e.msg, e.span.line, e.span.col)),
        },
    ));
    out.push(ck("sample .qu: run", {
        let mut it = qu_interp::Interp::new();
        match it.run(SAMPLE_QU) {
            Ok(()) => Ok("ran to completion".to_string()),
            Err(e) => Err(e.to_string()),
        }
    }));
    let used: BTreeSet<&'static str> = toks.iter().filter_map(|t| if let qu_lexer::Tok::Keyword(k) = &t.tok { Some(*k) } else { None }).collect();
    for d in drift_reports() {
        // words highlighted by this grammar = KEYWORDS minus missing, plus extras: reuse `missing`
        let miss: Vec<&&str> = used.iter().filter(|k| d.missing.iter().any(|m| m == **k)).collect();
        out.push(ck(
            &format!("sample .qu keywords highlighted: {}", d.grammar),
            if miss.is_empty() { Ok(format!("{} keyword kinds used by the sample, all highlighted", used.len())) } else { Err(format!("not highlighted: {}", miss.iter().map(|k| **k).collect::<Vec<_>>().join(" "))) },
        ));
    }
    out
}

// ---------------------------------------------------------------- embedded files

fn parse_json(name: &str, text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("{name}: {e}"))
}

/// `//` comments stripped (Sublime's .sublime-build allows them).
fn strip_line_comments(text: &str) -> String {
    text.lines()
        .map(|l| {
            let mut in_str = false;
            let mut prev = ' ';
            for (i, c) in l.char_indices() {
                if c == '"' && prev != '\\' {
                    in_str = !in_str;
                }
                if !in_str && c == '/' && prev == '/' {
                    return l[..i - 1].to_string();
                }
                prev = c;
            }
            l.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn vscode_package_check() -> Result<String, String> {
    let v = parse_json("package.json", asset(VSCODE_FILES, "package.json"))?;
    let langs = v.pointer("/contributes/languages").and_then(|l| l.as_array()).ok_or("no contributes.languages")?;
    let qu = langs.iter().find(|l| l.get("id").and_then(|i| i.as_str()) == Some("qu")).ok_or("no language with id \"qu\"")?;
    let exts = qu.get("extensions").and_then(|e| e.as_array()).ok_or("language qu has no extensions")?;
    if !exts.iter().any(|e| e.as_str() == Some(".qu")) {
        return Err("language qu does not claim .qu".into());
    }
    let cfg = qu.get("configuration").and_then(|c| c.as_str()).ok_or("language qu has no configuration")?;
    let g = v.pointer("/contributes/grammars").and_then(|g| g.as_array()).and_then(|g| g.first()).ok_or("no contributes.grammars")?;
    if g.get("scopeName").and_then(|s| s.as_str()) != Some("source.qu") || g.get("language").and_then(|s| s.as_str()) != Some("qu") {
        return Err("grammar must have language \"qu\" and scopeName \"source.qu\"".into());
    }
    for (what, p) in [("configuration", cfg), ("grammar", g.get("path").and_then(|p| p.as_str()).unwrap_or(""))] {
        let rel = p.trim_start_matches("./");
        if !VSCODE_FILES.iter().any(|a| a.rel == rel) {
            return Err(format!("{what} path {p} is not an embedded file"));
        }
    }
    let main = v.get("main").and_then(|m| m.as_str()).unwrap_or("").trim_start_matches("./").to_string();
    if !VSCODE_FILES.iter().any(|a| a.rel == main) {
        return Err(format!("main {main} is not an embedded file"));
    }
    if !v.get("activationEvents").and_then(|a| a.as_array()).map(|a| a.iter().any(|e| e.as_str() == Some("onLanguage:qu"))).unwrap_or(false) {
        return Err("activationEvents lacks onLanguage:qu".into());
    }
    if v.pointer("/contributes/configuration/properties/qu.executablePath").is_none() {
        return Err("setting qu.executablePath is not contributed".into());
    }
    let ver = v.get("version").and_then(|s| s.as_str()).unwrap_or("");
    if ver.split('.').count() != 3 || ver.split('.').any(|p| p.parse::<u32>().is_err()) {
        return Err(format!("version {ver:?} is not X.Y.Z"));
    }
    let ext = asset(VSCODE_FILES, "extension.js");
    for needle in ["qu.executablePath", "module.exports", "'parse', '--json'", "called from"] {
        if !ext.contains(needle) {
            return Err(format!("extension.js does not mention {needle}"));
        }
    }
    Ok(format!("language qu for .qu, scope source.qu, version {ver}, setting qu.executablePath"))
}

fn tm_check() -> Result<String, String> {
    let text = asset(VSCODE_FILES, "syntaxes/qu.tmLanguage.json");
    let v = parse_json("qu.tmLanguage.json", text)?;
    if v.get("scopeName").and_then(|s| s.as_str()) != Some("source.qu") {
        return Err("scopeName is not source.qu".into());
    }
    let repo = v.get("repository").and_then(|r| r.as_object()).ok_or("no repository")?;
    // every {"include": "#x"} must name a repository entry
    let mut n = 0;
    let mut stack = vec![&v];
    while let Some(x) = stack.pop() {
        match x {
            Value::Object(m) => {
                if let Some(Value::String(inc)) = m.get("include") {
                    if let Some(name) = inc.strip_prefix('#') {
                        n += 1;
                        if !repo.contains_key(name) {
                            return Err(format!("include #{name} names no repository entry"));
                        }
                    }
                }
                stack.extend(m.values());
            }
            Value::Array(a) => stack.extend(a.iter()),
            _ => {}
        }
    }
    // every begin has an end
    let mut stack = vec![&v];
    while let Some(x) = stack.pop() {
        match x {
            Value::Object(m) => {
                if m.contains_key("begin") != m.contains_key("end") {
                    return Err("a pattern has begin without end (or the reverse)".into());
                }
                stack.extend(m.values());
            }
            Value::Array(a) => stack.extend(a.iter()),
            _ => {}
        }
    }
    Ok(format!("{} repository entries, {n} includes resolve", repo.len()))
}

fn langconfig_check() -> Result<String, String> {
    let v = parse_json("language-configuration.json", asset(VSCODE_FILES, "language-configuration.json"))?;
    if v.pointer("/comments/lineComment").and_then(|c| c.as_str()) != Some("#") {
        return Err("lineComment is not \"#\"".into());
    }
    let inc = v.pointer("/indentationRules/increaseIndentPattern").and_then(|p| p.as_str()).unwrap_or("");
    if inc.contains("elif") {
        return Err("indent rules mention `elif`, which is not a Qu keyword (it is `elseif`)".into());
    }
    Ok("line comment #, brackets, indent rules use elseif".into())
}

fn build_check() -> Result<String, String> {
    let v = parse_json("Qu.sublime-build", &strip_line_comments(asset(SUBLIME_FILES, "Qu.sublime-build")))?;
    let cmd = v.get("cmd").and_then(|c| c.as_array()).ok_or("no cmd")?;
    if cmd.first().and_then(|c| c.as_str()) != Some("qu") || cmd.get(1).and_then(|c| c.as_str()) != Some("run") {
        return Err("cmd is not [\"qu\", \"run\", ...]".into());
    }
    if v.get("selector").and_then(|s| s.as_str()) != Some("source.qu") {
        return Err("selector is not source.qu".into());
    }
    Ok("cmd qu run $file, selector source.qu".into())
}

fn udl_check() -> Result<String, String> {
    let text = asset(NOTEPADPP_FILES, "Qu.udl.xml");
    xml_wellformed(text)?;
    if !text.contains("<UserLang name=\"Qu\"") {
        return Err("no <UserLang name=\"Qu\">".into());
    }
    if !text.contains("ext=\"qu\"") {
        return Err("UserLang does not claim ext=\"qu\"".into());
    }
    for n in ["Keywords1", "Keywords2", "Keywords3", "Keywords4"] {
        if udl_list(text, n).is_empty() {
            return Err(format!("keyword list {n} is empty"));
        }
    }
    Ok("well-formed, UserLang Qu, ext qu, keyword lists present".into())
}

fn vim_check() -> Result<String, String> {
    let syn = asset(VIM_FILES, "syntax/qu.vim");
    let det = asset(VIM_FILES, "ftdetect/qu.vim");
    let plug = asset(VIM_FILES, "ftplugin/qu.vim");
    if !syn.contains("let b:current_syntax = \"qu\"") || !syn.contains("syn keyword") {
        return Err("syntax/qu.vim lacks `syn keyword` or b:current_syntax".into());
    }
    let ifs = syn.lines().filter(|l| l.trim_start().starts_with("if ")).count();
    let endifs = syn.lines().filter(|l| l.trim() == "endif").count();
    if ifs != endifs {
        return Err(format!("syntax/qu.vim has {ifs} `if` and {endifs} `endif`"));
    }
    if !det.contains("*.qu") || !det.contains("setfiletype qu") {
        return Err("ftdetect/qu.vim does not map *.qu to filetype qu".into());
    }
    if !plug.contains("commentstring=#") || !plug.contains("shiftwidth=4") || !plug.contains("b:match_words") {
        return Err("ftplugin/qu.vim lacks commentstring, 4-space indent or matchit pairs".into());
    }
    for kw in ["if", "for", "while", "function"] {
        if !plug.contains(kw) {
            return Err(format!("matchit pairs do not mention {kw}"));
        }
    }
    Ok("syntax, ftdetect (*.qu), ftplugin (commentstring #, 4 spaces, matchit if/for/while/function ... end)".into())
}

/// Every check that needs no installed editor.
pub fn embedded_checks() -> Vec<Check> {
    let mut c = vec![
        ck("vscode package.json", vscode_package_check()),
        ck("vscode qu.tmLanguage.json", tm_check()),
        ck("vscode language-configuration.json", langconfig_check()),
        ck("sublime Qu.sublime-syntax", sublime_syntax_check(asset(SUBLIME_FILES, "Qu.sublime-syntax"))),
        ck("sublime Qu.sublime-build", build_check()),
        ck("notepad++ Qu.udl.xml", udl_check()),
        ck("vim files", vim_check()),
    ];
    // versions agree
    let v = vscode_version();
    let others = [
        ("sublime", shipped_version(PluginKind::Sublime)),
        ("notepad++", shipped_version(PluginKind::NotepadPp)),
        ("vim", shipped_version(PluginKind::Vim)),
    ];
    let bad: Vec<String> = others.iter().filter(|(_, ov)| *ov != v).map(|(n, ov)| format!("{n} {ov:?}")).collect();
    c.push(ck(
        "plugin versions agree",
        if bad.is_empty() { Ok(format!("all {v}")) } else { Err(format!("vscode is {v} but {}", bad.join(", "))) },
    ));
    for d in drift_reports() {
        c.push(drift_check(&d));
    }
    c.extend(sample_check());
    c
}
