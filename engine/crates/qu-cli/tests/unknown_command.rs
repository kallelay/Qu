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
fn a_bare_script_path_suggests_run() {
    // Forgetting `run` is the commonest version of this, and the argument
    // is not a typo -- the user has a real file and a real intention, so
    // the message can be specific rather than generic.
    let (code, text) = run(&["some_script.qu"]);
    assert_ne!(code, 0, "output was:\n{text}");
    assert!(
        text.contains("qu run some_script.qu"),
        "should suggest the command they meant:\n{text}"
    );
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
