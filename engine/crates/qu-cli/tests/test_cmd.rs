//! `qu test`: discovery, per-test PASS/FAIL, error locations, fresh
//! interpreter per file, and the exit-status contract (0 / 1 / 2).

use std::path::{Path, PathBuf};
use std::process::Command;

fn qu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_qu")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qu_test_cmd_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn qu(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(qu_bin()).current_dir(dir).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const PASSING: &str = "function test_add()\n  if 1 + 1 != 2\n    error(\"math broke\")\n  end if\nend function\n";
const FAILING: &str = "function test_good()\n  x = 1\nend function\n\nfunction test_bad()\n  print(\"before\")\n  error(\"boom here\")\nend function\n";

#[test]
fn all_pass_exits_0_and_names_each_test() {
    let d = scratch("pass");
    std::fs::write(d.join("a.qu"), PASSING).unwrap();
    let (code, out, _) = qu(&d, &["test", "a.qu"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PASS a.qu:1 test_add"), "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_failing_test_exits_1_with_file_line_and_function() {
    let d = scratch("fail");
    std::fs::write(d.join("b.qu"), FAILING).unwrap();
    let (code, out, _) = qu(&d, &["test", "b.qu"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("PASS b.qu:1 test_good"), "{out}");
    assert!(out.contains("FAIL b.qu:5 test_bad"), "{out}");
    assert!(out.contains("boom here"), "{out}");
    assert!(out.contains("b.qu:7 in test_bad()"), "location of the error is shown:\n{out}");
    assert!(out.contains("before"), "captured output shown for a failure:\n{out}");
    assert!(out.contains("1 passed, 1 failed"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn no_tests_exits_2() {
    let d = scratch("none");
    let (code, _, err) = qu(&d, &["test"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("no tests found"), "{err}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_file_without_test_functions_runs_as_a_script() {
    let d = scratch("script");
    std::fs::write(d.join("ok.qu"), "x = 1 + 1\n").unwrap();
    std::fs::write(d.join("bad.qu"), "x = 1\nerror(\"script died\")\n").unwrap();
    let (code, out, _) = qu(&d, &["test", "ok.qu"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PASS ok.qu (script)"), "{out}");
    let (code, out, _) = qu(&d, &["test", "bad.qu"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL bad.qu:2 (script)"), "{out}");
    assert!(out.contains("script died"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn each_file_gets_a_fresh_interpreter() {
    let d = scratch("fresh");
    std::fs::write(d.join("one.qu"), "leaked = 42\nfunction test_sets()\n  y = leaked\nend function\n").unwrap();
    // `leaked` must NOT exist in the second file.
    std::fs::write(
        d.join("two.qu"),
        "function test_sees_nothing()\n  try\n    z = leaked\n  catch e\n    return\n  end try\n  error(\"state leaked between files\")\nend function\n",
    )
    .unwrap();
    let (code, out, _) = qu(&d, &["test", "one.qu", "two.qu"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn filter_and_fail_fast() {
    let d = scratch("filter");
    std::fs::write(d.join("c.qu"), FAILING).unwrap();
    let (code, out, _) = qu(&d, &["test", "c.qu", "--filter", "good"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("test_bad"), "{out}");
    let src = "function test_a()\n  error(\"a\")\nend function\nfunction test_b()\n  error(\"b\")\nend function\n";
    std::fs::write(d.join("d.qu"), src).unwrap();
    let (code, out, _) = qu(&d, &["test", "d.qu", "--fail-fast"]);
    assert_eq!(code, 1);
    assert!(out.contains("test_a") && !out.contains("test_b"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn default_discovery_finds_tests_dir_and_test_named_files() {
    let d = scratch("disc");
    std::fs::create_dir_all(d.join("tests/sub")).unwrap();
    std::fs::write(d.join("tests/sub/x.qu"), PASSING).unwrap();
    std::fs::write(d.join("test_top.qu"), PASSING).unwrap();
    std::fs::write(d.join("thing_test.qu"), PASSING).unwrap();
    std::fs::write(d.join("ignored.qu"), "error(\"must not run\")\n").unwrap();
    let (code, out, _) = qu(&d, &["test"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("3 passed"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn json_output_is_structured() {
    let d = scratch("json");
    std::fs::write(d.join("e.qu"), FAILING).unwrap();
    let (code, out, _) = qu(&d, &["test", "e.qu", "--json"]);
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).expect("stdout is JSON only");
    assert_eq!(v["passed"], 1);
    assert_eq!(v["failed"], 1);
    let r = &v["results"][1];
    assert_eq!(r["name"], "test_bad");
    assert_eq!(r["status"], "fail");
    assert_eq!(r["line"], 5);
    assert!(r["ms"].as_f64().unwrap() >= 0.0);
    assert!(r["error"].as_str().unwrap().contains("boom here"));
    let _ = std::fs::remove_dir_all(&d);
}
