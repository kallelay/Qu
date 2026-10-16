//! `qu test [paths...] [--filter substr] [--fail-fast] [--json] [--timeout secs]`
//!
//! `--timeout` (default 60; 0 = none) runs every test in its own child
//! `qu` process (`qu test --child <file> --only <test>`, internal) so a hung
//! test is killed alone and reported `FAILED (timeout)`; it also means each
//! test starts from a fresh interpreter (the file's top level is re-run per
//! test). `--timeout 0` runs in-process with one interpreter per file, which
//! is replaced after an interpreter panic.
//!
//! # Convention
//!
//! A test is a top-level, zero-parameter function whose name starts with
//! `test_`. It PASSES if calling it returns normally and FAILS if it raises
//! an error. Qu has no `assert` builtin (probed 2026-10: only `error(msg)`
//! raises), so the idiom is
//!
//! ```text
//! function test_sum()
//!     if sum([1, 2, 3]) != 6
//!         error("sum([1,2,3]) was not 6")
//!     end if
//! end function
//! ```
//!
//! A file with no `test_` functions is a SCRIPT: it passes if it runs to
//! the end without an error. This is how the repo's `tests/*.qu` style of
//! "run it, it must not blow up" files are covered.
//!
//! # Isolation
//!
//! Each file gets a FRESH interpreter, so state (variables, user functions,
//! figures, RNG seed) cannot leak from one file into the next. Inside one
//! file the interpreter is shared: top-level code runs first (it defines
//! the functions and any fixtures), then each `test_` function is called in
//! source order. A top-level error is reported as one failure
//! (`<file> (load)`) and the file's tests are not run.
//!
//! # Discovery
//!
//! Explicit files are used as given; explicit directories are searched
//! recursively for `*.qu`. With no paths: every `*.qu` under `./tests`, plus
//! `test_*.qu` and `*_test.qu` directly in the current directory.
//!
//! # Exit status
//!
//! 0 all passed; 1 any failure; 2 nothing to run.

use crate::fmt_cmd::top_level_test_functions;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Outcome {
    file: String,
    /// `None` for a whole-file script run.
    name: Option<String>,
    line: u32,
    ok: bool,
    ms: f64,
    error: String,
    trace: String,
    output: String,
}

fn collect_qu(dir: &Path, out: &mut Vec<PathBuf>) {
    collect_qu_depth(dir, out, 0);
}

fn collect_qu_depth(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > crate::fmt_cmd::MAX_WALK_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") || crate::fmt_cmd::is_link_or_reparse(&p) {
                continue;
            }
            collect_qu_depth(&p, out, depth + 1);
        } else if name.ends_with(".qu") {
            out.push(p);
        }
    }
}

fn discover(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if paths.is_empty() {
        collect_qu(Path::new("tests"), &mut files);
        if let Ok(rd) = std::fs::read_dir(".") {
            let mut here: Vec<_> = rd.filter_map(|e| e.ok()).collect();
            here.sort_by_key(|e| e.file_name());
            for e in here {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_file() && ((name.starts_with("test_") && name.ends_with(".qu")) || name.ends_with("_test.qu")) {
                    files.push(e.path());
                }
            }
        }
    } else {
        for p in paths {
            let pb = Path::new(p);
            if pb.is_dir() {
                collect_qu(pb, &mut files);
            } else if pb.is_file() {
                files.push(pb.to_path_buf());
            } else {
                return Err(format!("test: no such file or directory `{p}`"));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| seen.insert(f.to_string_lossy().replace('\\', "/")));
    Ok(files)
}

/// Run `src` (or one call) and turn an error into `(message, trace)`.
fn run_unit(it: &mut qu_interp::Interp, code: &str, file: &str, synthetic: bool) -> Result<(), (String, String)> {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| it.run(code)));
    match r {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            let mut trace = it.error_trace(&e.msg, file).unwrap_or_default();
            if synthetic {
                // The last line names the synthetic `test_x()` call we made,
                // which is not a place in the user's file.
                let mut lines: Vec<&str> = trace.lines().collect();
                if lines.last().is_some_and(|l| l.contains("(top level)")) {
                    lines.pop();
                }
                trace = lines.join("\n");
            }
            Err((e.msg, trace))
        }
        Err(p) => {
            let m = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "interpreter panic".into());
            Err((format!("interpreter panic: {m}"), String::new()))
        }
    }
}

/// First `file:LINE` mentioned in a trace.
fn first_line_in_trace(trace: &str, file: &str) -> Option<u32> {
    let pat = format!("{file}:");
    let at = trace.find(&pat)?;
    let digits: String = trace[at + pat.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn outcome_json(o: &Outcome) -> serde_json::Value {
    serde_json::json!({
        "file": o.file,
        "name": o.name,
        "line": o.line,
        "status": if o.ok { "pass" } else { "fail" },
        "ms": (o.ms * 1000.0).round() / 1000.0,
        "error": if o.ok { serde_json::Value::Null } else { serde_json::Value::String(o.error.clone()) },
        "trace": if o.trace.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(o.trace.clone()) },
        "output": o.output,
    })
}

fn outcome_from_json(v: &serde_json::Value) -> Outcome {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Outcome {
        file: s("file"),
        name: v["name"].as_str().map(|x| x.to_string()),
        line: v["line"].as_u64().unwrap_or(0) as u32,
        ok: v["status"] == "pass",
        ms: v["ms"].as_f64().unwrap_or(0.0),
        error: s("error"),
        trace: s("trace"),
        output: s("output"),
    }
}

const CHILD_MARK: &str = "@@QU_TEST_RESULT@@ ";

/// Run one test (or a whole script file) in a child `qu test --child`
/// process so a hang can be abandoned by killing ONLY that child. Threads
/// cannot be killed safely; processes can. `None` from the child's report
/// means it died without reporting (crash / abort).
fn run_in_child(file: &str, only: Option<&str>, timeout: f64) -> Result<Vec<Outcome>, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own executable: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("test").arg("--child").arg(file);
    if let Some(n) = only {
        cmd.arg("--only").arg(n);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start child: {e}"))?;
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let h_out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let h_err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + std::time::Duration::from_secs_f64(timeout);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(format!("waiting for child: {e}")),
        }
    };
    let out = h_out.join().unwrap_or_default();
    let err = h_err.join().unwrap_or_default();
    if timed_out {
        return Err(format!("FAILED (timeout): exceeded {timeout} s and was killed"));
    }
    for l in out.lines() {
        if let Some(j) = l.strip_prefix(CHILD_MARK) {
            if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(j) {
                return Ok(a.iter().map(outcome_from_json).collect());
            }
        }
    }
    let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
    Err(format!("child process died without reporting ({:?}) {tail}", status.map(|s| s.code())))
}

/// `run_file`, but each test runs in its own child process under a
/// deadline. The child re-runs the file's top-level code, so every test
/// starts from a pristine interpreter and a hang or crash costs one test.
fn run_file_isolated(path: &Path, filter: Option<&str>, fail_fast: bool, stop: &mut bool, timeout: f64) -> Vec<Outcome> {
    let file = path.to_string_lossy().replace('\\', "/");
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return run_file(path, filter, fail_fast, stop, None), // reports the read error
    };
    let tests = top_level_test_functions(&src);
    let selected: Vec<&(String, u32)> = tests.iter().filter(|(n, _)| filter.map_or(true, |f| n.contains(f))).collect();
    if !tests.is_empty() && selected.is_empty() {
        return Vec::new();
    }
    if tests.is_empty() && filter.is_some_and(|f| !file.contains(f)) {
        return Vec::new();
    }
    let fail = |name: Option<String>, line: u32, error: String| Outcome {
        file: file.clone(),
        name,
        line,
        ok: false,
        ms: 0.0,
        error,
        trace: String::new(),
        output: String::new(),
    };
    let mut results = Vec::new();
    if tests.is_empty() {
        match run_in_child(&file, None, timeout) {
            Ok(v) => results.extend(v),
            Err(m) => results.push(fail(None, 0, m)),
        }
        return results;
    }
    for (name, line) in selected {
        if fail_fast && *stop {
            break;
        }
        match run_in_child(&file, Some(name), timeout) {
            Ok(v) => {
                let load_failed = v.iter().any(|o| o.name.as_deref() == Some("(load)"));
                if v.iter().any(|o| !o.ok) {
                    *stop = true;
                }
                results.extend(v);
                if load_failed {
                    break; // the file does not load: one (load) failure, as in-process
                }
            }
            Err(m) => {
                *stop = true;
                results.push(fail(Some(name.clone()), *line, m));
            }
        }
    }
    results
}

fn run_file(path: &Path, filter: Option<&str>, fail_fast: bool, stop: &mut bool, only: Option<&str>) -> Vec<Outcome> {
    let file = path.to_string_lossy().replace('\\', "/");
    let mut results = Vec::new();
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            results.push(Outcome {
                file,
                name: None,
                line: 0,
                ok: false,
                ms: 0.0,
                error: format!("cannot read file: {e}"),
                trace: String::new(),
                output: String::new(),
            });
            return results;
        }
    };
    let tests = top_level_test_functions(&src);
    let selected: Vec<&(String, u32)> = tests
        .iter()
        .filter(|(n, _)| filter.map_or(true, |f| n.contains(f)))
        .filter(|(n, _)| only.map_or(true, |o| n == o))
        .collect();
    if !tests.is_empty() && selected.is_empty() {
        return results; // filtered out entirely
    }
    if tests.is_empty() && only.is_none() && filter.is_some_and(|f| !file.contains(f)) {
        return results;
    }

    let mut it = qu_interp::Interp::new();
    it.set_script_path(path);
    let t0 = Instant::now();
    let load = run_unit(&mut it, &src, &file, false);
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let load_out = std::mem::take(&mut it.out);

    if tests.is_empty() {
        // script mode
        let (ok, error, trace) = match load {
            Ok(()) => (true, String::new(), String::new()),
            Err((m, t)) => (false, m, t),
        };
        let line = first_line_in_trace(&trace, &file).unwrap_or(0);
        results.push(Outcome { file, name: None, line, ok, ms: load_ms, error, trace, output: load_out });
        return results;
    }
    if let Err((m, t)) = load {
        let line = first_line_in_trace(&t, &file).unwrap_or(0);
        results.push(Outcome {
            file: file.clone(),
            name: Some("(load)".into()),
            line,
            ok: false,
            ms: load_ms,
            error: m,
            trace: t,
            output: load_out,
        });
        return results;
    }
    for (name, line) in selected {
        if fail_fast && *stop {
            break;
        }
        let start_len = it.out.len();
        let t = Instant::now();
        let r = run_unit(&mut it, &format!("{name}()"), &file, true);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let output = it.out.get(start_len..).unwrap_or("").to_string();
        it.out.truncate(start_len);
        let panicked = matches!(&r, Err((m, _)) if m.starts_with("interpreter panic:"));
        let (ok, error, trace) = match r {
            Ok(()) => (true, String::new(), String::new()),
            Err((m, tr)) => (false, m, tr),
        };
        if !ok {
            *stop = true;
        }
        if panicked {
            // A panic unwinds past the call-stack / frame bookkeeping
            // without popping it, so this interpreter is no longer
            // trustworthy for the NEXT test. Replace it with a pristine one
            // and re-run the file's top-level definitions.
            it = qu_interp::Interp::new();
            it.set_script_path(path);
            let _ = run_unit(&mut it, &src, &file, false);
            it.out.clear();
        }
        results.push(Outcome { file: file.clone(), name: Some(name.clone()), line: *line, ok, ms, error, trace, output });
    }
    results
}

pub fn cmd_test(args: &[String]) -> Result<(), String> {
    let mut paths = Vec::new();
    let mut filter: Option<String> = None;
    let mut fail_fast = false;
    let mut json = false;
    let mut timeout = 60.0f64;
    let mut child: Option<String> = None;
    let mut only: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                i += 1;
                filter = Some(args.get(i).ok_or("test: --filter expects a substring")?.clone());
            }
            "--fail-fast" => fail_fast = true,
            "--json" => json = true,
            "--timeout" => {
                i += 1;
                timeout = args
                    .get(i)
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|t| *t >= 0.0 && t.is_finite())
                    .ok_or("test: --timeout expects a number of seconds (0 = no limit, in-process)")?;
            }
            "--child" => {
                i += 1;
                child = Some(args.get(i).ok_or("test: --child expects a file")?.clone());
            }
            "--only" => {
                i += 1;
                only = Some(args.get(i).ok_or("test: --only expects a test name")?.clone());
            }
            s if s.starts_with("--") => return Err(format!("test: unknown option `{s}`")),
            s => paths.push(s.to_string()),
        }
        i += 1;
    }
    if let Some(file) = child {
        // Internal: run exactly one test (or one script file) in this
        // process and report it on a marked stdout line.
        let mut stop = false;
        let res = run_file(Path::new(&file), None, false, &mut stop, only.as_deref());
        let arr: Vec<serde_json::Value> = res.iter().map(outcome_json).collect();
        use std::io::Write;
        println!("{CHILD_MARK}{}", serde_json::Value::Array(arr));
        std::io::stdout().flush().ok();
        std::process::exit(0);
    }
    let files = discover(&paths)?;
    let started = Instant::now();
    let mut all: Vec<Outcome> = Vec::new();
    let mut stop = false;
    for f in &files {
        if fail_fast && stop {
            break;
        }
        if timeout > 0.0 {
            all.extend(run_file_isolated(f, filter.as_deref(), fail_fast, &mut stop, timeout));
        } else {
            all.extend(run_file(f, filter.as_deref(), fail_fast, &mut stop, None));
        }
    }
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let passed = all.iter().filter(|o| o.ok).count();
    let failed = all.len() - passed;

    if json {
        let results: Vec<serde_json::Value> = all.iter().map(outcome_json).collect();
        let doc = serde_json::json!({
            "passed": passed,
            "failed": failed,
            "total": all.len(),
            "ms": (total_ms * 1000.0).round() / 1000.0,
            "results": results,
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    } else {
        for o in &all {
            let label = if o.ok { "PASS" } else { "FAIL" };
            let loc = if o.line > 0 { format!("{}:{}", o.file, o.line) } else { o.file.clone() };
            let what = match &o.name {
                Some(n) => format!("{loc} {n}"),
                None => format!("{loc} (script)"),
            };
            println!("{label} {what}  ({:.1} ms)", o.ms);
            if !o.ok {
                for l in o.error.lines() {
                    println!("       {l}");
                }
                for l in o.trace.lines() {
                    println!("     {l}");
                }
                if !o.output.trim().is_empty() {
                    println!("     -- output --");
                    for l in o.output.lines() {
                        println!("     | {l}");
                    }
                }
            }
        }
        println!(
            "\ntest result: {} passed, {} failed ({} total) in {:.2}s",
            passed,
            failed,
            all.len(),
            total_ms / 1000.0
        );
    }
    use std::io::Write;
    std::io::stdout().flush().ok();
    if all.is_empty() {
        eprintln!("qu: no tests found");
        std::process::exit(2);
    }
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
