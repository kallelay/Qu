//! `qu <something it does not recognise>` must FAIL, not print the banner
//! and exit 0.
//!
//! The old fallback arm did `print_help(); Ok(())` for anything unmatched,
//! so `qu -e '...'`, `qu frobnicate`, `qu --nonsense` and a bare
//! `qu script.qu` (forgetting `run`) all exited 0. A typo'd subcommand in
//! a Makefile or a CI step therefore passed green while doing nothing --
//! the failure that looks like success, which is the expensive kind.
//!
//! These spawn the real binary, because the thing under test is the
//! PROCESS EXIT STATUS. Asserting that the message mentions the bad
//! argument would not catch a regression to `Ok(())`: the message could be
//! perfect and the exit code still 0. The exit code is the claim; the
//! wording is a courtesy.

use std::process::Command;

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(qu_bin()).args(args).output().expect("spawn qu");
    let code = out.status.code().unwrap_or(-1);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (code, text)
}

#[test]
fn an_unknown_subcommand_exits_non_zero() {
    let (code, text) = run(&["frobnicate"]);
    assert_ne!(code, 0, "a typo must not pass green in CI; output was:\n{text}");
    assert!(text.contains("frobnicate"), "say what was not recognised:\n{text}");
}

#[test]
fn an_unknown_flag_exits_non_zero() {
    // `-e` is the specific one that started this: it looks like the
    // "evaluate" flag other tools have, and Qu spells it `eval`.
    for flag in ["-e", "--nonsense"] {
        let (code, text) = run(&[flag]);
        assert_ne!(code, 0, "`qu {flag}` exited 0; output was:\n{text}");
    }
}

#[test]
fn a_bare_script_path_runs_it() {
    // `qu script.qu` means `qu run script.qu` (since 0.4.5, as `python
    // script.py` does). It used to be refused with a "did you mean"; what
    // must still hold is the exit status: a script that is not there is a
    // failure that names it, never a green no-op.
    let (code, text) = run(&["no_such_script_here.qu"]);
    assert_ne!(code, 0, "output was:\n{text}");
    assert!(text.contains("no_such_script_here.qu"), "name the missing file:\n{text}");

    let dir = std::env::temp_dir().join(format!("qu_bare_run_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("hello.qu");
    std::fs::write(&script, "print(\"bare ok {1 + 1}\")\n").unwrap();
    let path = script.to_string_lossy().to_string();
    let (code, text) = run(&[&path]);
    assert_eq!(code, 0, "output was:\n{text}");
    assert!(text.contains("bare ok 2"), "{text}");
    // options before the command, or before a bare script, are passed on
    let (code, text) = run(&["--sandbox", &path]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("bare ok 2"), "{text}");
    let (code, text) = run(&["--live", "run", &path]);
    assert_eq!(code, 0, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn repl_runs_its_script_and_streams_it() {
    // `qu repl x.qu` printed the script's output only after the whole
    // script had finished. With stdout a pipe, the first line must arrive
    // before the `sleep` is over.
    use std::io::{BufRead, BufReader, Write};
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join(format!("qu_repl_live_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("slow.qu");
    std::fs::write(&script, "print(\"first\")\nsleep(1500)\nprint(\"second\")\n").unwrap();
    let mut child = Command::new(qu_bin())
        .args(["--live", "repl", &script.to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn qu repl");
    let start = Instant::now();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let mut first_at = None;
    while out.read_line(&mut line).unwrap() > 0 {
        if line.contains("first") {
            first_at = Some(start.elapsed());
            break;
        }
        line.clear();
    }
    child.stdin.as_mut().unwrap().write_all(b":quit\n").unwrap();
    let _ = child.wait();
    let first_at = first_at.expect("the script's output reached stdout");
    assert!(first_at < Duration::from_millis(1200), "first line only after {first_at:?} -- buffered, not live");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asking_for_help_still_succeeds() {
    // The other half of the property. Printing the banner is correct for
    // no arguments and for an explicit request -- the fix must not turn
    // those into failures, which is the obvious way to overshoot here.
    for args in [vec![], vec!["help"], vec!["--help"], vec!["-h"]] {
        let (code, text) = run(&args);
        assert_eq!(code, 0, "`qu {args:?}` should succeed; output was:\n{text}");
        assert!(text.contains("commands:"), "should print the banner:\n{text}");
    }
}
