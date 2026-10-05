//! `qu fmt`: the formatter must be IDEMPOTENT and SEMANTICS-PRESERVING.
//!
//! The corpus test pushes every `.qu` file in the repository through the
//! real binary (`qu fmt --stdin`) and checks, for each:
//!   (a) fmt(fmt(x)) == fmt(x)
//!   (b) the token stream (kinds + text, whitespace/comments ignored) is
//!       unchanged
//!   (c) the formatted file still parses (when the original did)
//!   (d) stripping ALL whitespace from input and output gives identical
//!       text -- which also proves comments and string contents survived.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn fmt_stdin(src: &str) -> (i32, String) {
    let mut child = Command::new(qu_bin())
        .args(["fmt", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qu");
    let mut sin = child.stdin.take().unwrap();
    let data = src.as_bytes().to_vec();
    let h = std::thread::spawn(move || {
        let _ = sin.write_all(&data);
    });
    let out = child.wait_with_output().expect("wait");
    h.join().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

fn tokens(src: &str) -> Vec<String> {
    let mut v: Vec<String> = qu_lexer::lex(src).iter().map(|t| format!("{:?}", t.tok)).collect();
    // The closing Newline exists only if the file ends in one; fmt adds it.
    if v.len() >= 2 && v[v.len() - 2] == "Newline" {
        v.remove(v.len() - 2);
    }
    v
}

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "private") {
                continue;
            }
            walk(&p, out);
        } else if name.ends_with(".qu") {
            out.push(p);
        }
    }
}

#[test]
fn every_repo_qu_file_formats_idempotently_and_preserves_tokens() {
    let mut files = Vec::new();
    walk(&repo_root(), &mut files);
    assert!(files.len() > 20, "corpus walk found only {} files -- wrong root?", files.len());
    let mut changed = 0;
    let mut checked = 0;
    let mut failures = Vec::new();
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        checked += 1;
        let name = f.display().to_string();
        let (code, once) = fmt_stdin(&src);
        if code != 0 {
            failures.push(format!("{name}: fmt exited {code}"));
            continue;
        }
        if once != src {
            changed += 1;
        }
        let (code2, twice) = fmt_stdin(&once);
        if code2 != 0 || twice != once {
            failures.push(format!("{name}: not idempotent"));
        }
        if tokens(&src) != tokens(&once) {
            failures.push(format!("{name}: token stream changed"));
        }
        if strip_ws(&src) != strip_ws(&once) {
            failures.push(format!("{name}: non-whitespace text changed"));
        }
        if !once.ends_with('\n') && !once.is_empty() {
            failures.push(format!("{name}: no final newline"));
        }
        // (c) still parses, if it parsed before. Deep recursion: big stack.
        let (s1, s2) = (src.clone(), once.clone());
        let parsed = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || (qu_syntax::parse(&s1).is_ok(), qu_syntax::parse(&s2).is_ok()))
            .unwrap()
            .join()
            .unwrap();
        if parsed.0 && !parsed.1 {
            failures.push(format!("{name}: parsed before formatting, fails to parse after"));
        }
    }
    eprintln!("fmt corpus: {checked} files checked, {changed} would change");
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qu_fmt_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn check_exits_1_and_lists_files_that_would_change() {
    let d = scratch("check");
    std::fs::write(d.join("messy.qu"), "if x\nprint(1)  \nend if").unwrap();
    std::fs::write(d.join("clean.qu"), "x = 1\n").unwrap();
    let out = Command::new(qu_bin()).args(["fmt", "--check"]).arg(&d).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("messy.qu"), "{text}");
    assert!(!text.contains("clean.qu"), "{text}");
    // --check must not write
    assert_eq!(std::fs::read_to_string(d.join("messy.qu")).unwrap(), "if x\nprint(1)  \nend if");
    // formatting in place, then --check passes
    let out = Command::new(qu_bin()).arg("fmt").arg(&d).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(d.join("messy.qu")).unwrap(), "if x\n    print(1)\nend if\n");
    let out = Command::new(qu_bin()).args(["fmt", "--check"]).arg(&d).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn stdin_mode_formats() {
    let (code, out) = fmt_stdin("x=f(a,b)\n");
    assert_eq!(code, 0);
    assert_eq!(out, "x = f(a, b)\n");
}

#[test]
fn comments_and_strings_survive_byte_for_byte() {
    let src = "s = \"a,b=c   {x ,y}\"   # keep,  this=\n  t = r\"raw,  x=1\"\n";
    let (_, out) = fmt_stdin(src);
    assert!(out.contains("\"a,b=c   {x ,y}\""), "{out}");
    assert!(out.contains("# keep,  this="), "{out}");
    assert!(out.contains("r\"raw,  x=1\""), "{out}");
}

#[test]
fn crlf_files_stay_crlf() {
    let (_, out) = fmt_stdin("if a\r\nb = 1\r\nend if\r\n");
    assert_eq!(out, "if a\r\n    b = 1\r\nend if\r\n");
}
