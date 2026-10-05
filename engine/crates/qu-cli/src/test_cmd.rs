//! `qu test [paths...] [--filter substr] [--fail-fast] [--json]`
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
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
                continue;
            }
            collect_qu(&p, out);
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

fn run_file(path: &Path, filter: Option<&str>, fail_fast: bool, stop: &mut bool) -> Vec<Outcome> {
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
        .collect();
    if !tests.is_empty() && selected.is_empty() {
        return results; // filtered out entirely
    }
    if tests.is_empty() && filter.is_some_and(|f| !file.contains(f)) {
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
        let (ok, error, trace) = match r {
            Ok(()) => (true, String::new(), String::new()),
            Err((m, tr)) => (false, m, tr),
        };
        if !ok {
            *stop = true;
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
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" => {
                i += 1;
                filter = Some(args.get(i).ok_or("test: --filter expects a substring")?.clone());
            }
            "--fail-fast" => fail_fast = true,
            "--json" => json = true,
            s if s.starts_with("--") => return Err(format!("test: unknown option `{s}`")),
            s => paths.push(s.to_string()),
        }
        i += 1;
    }
    let files = discover(&paths)?;
    let started = Instant::now();
    let mut all: Vec<Outcome> = Vec::new();
    let mut stop = false;
    for f in &files {
        if fail_fast && stop {
            break;
        }
        all.extend(run_file(f, filter.as_deref(), fail_fast, &mut stop));
    }
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let passed = all.iter().filter(|o| o.ok).count();
    let failed = all.len() - passed;

    if json {
        let results: Vec<serde_json::Value> = all
            .iter()
            .map(|o| {
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
            })
            .collect();
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
