//! A runtime error must say WHERE it happened: file:line, the function,
//! and the call chain up to the top level (feedback item 9 -- "it cost many
//! bisect runs"). Spawns the real binary because the claim is about what
//! the user sees on stderr.

use std::process::Command;

fn run_script(name: &str, src: &str) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("qu_errloc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qu"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn qu");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn an_error_inside_nested_functions_names_line_function_and_callers() {
    let src = "function inner(x)\n  z = sqrt(\"abc\")\n  return z\nend\n\
               function outer(x)\n  return inner(x) + 1\nend\nb = outer(1)\n";
    let (code, text) = run_script("nested.qu", src);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("nested.qu:2 in inner()"), "{text}");
    assert!(text.contains("called from") && text.contains("nested.qu:6 in outer()"), "{text}");
    assert!(text.contains("nested.qu:8 (top level)"), "{text}");
}

#[test]
fn a_top_level_error_gets_its_line() {
    let (code, text) = run_script("top.qu", "x = 1\ny = 5 + undefined_thing_xyz\n");
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("top.qu:2"), "{text}");
}

#[test]
fn a_caught_error_does_not_leave_a_stale_location_on_a_later_one() {
    let src = "function f()\n  return sqrt(\"abc\")\nend\n\
               try\n  f()\ncatch e\n  print(\"caught\")\nend\nq = 5 + undefined_thing_xyz\n";
    let (code, text) = run_script("stale.qu", src);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("stale.qu:9"), "{text}");
    assert!(!text.contains("in f()"), "stale frame leaked: {text}");
}
