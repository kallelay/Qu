//! `qu run --sandbox` must deny EVERY file-writing builtin, and `--dry-run`
//! must skip the same set. An internal audit (2026-10-06) found that
//! `write_text`, `mkdir` and friends were not on the deny list: a sandboxed
//! script could still write files. The claim under test is what happens on
//! disk, so these spawn the real binary and look at the filesystem.

use std::process::Command;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qu_sbx_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_with(flag: &str, dir: &std::path::Path, src: &str) -> (i32, String) {
    let script = dir.join("s.qu");
    std::fs::write(&script, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qu"))
        .current_dir(dir)
        .args(["run", flag])
        .arg(&script)
        .output()
        .expect("spawn qu");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

const WRITES: &str = "try\n  write_text(\"a.txt\", \"x\")\ncatch e\n  print(\"denied write_text\")\nend\n\
try\n  append_text(\"b.txt\", \"x\")\ncatch e\n  print(\"denied append_text\")\nend\n\
try\n  mkdir(\"sub_dir\")\ncatch e\n  print(\"denied mkdir\")\nend\n\
try\n  touch(\"c.txt\")\ncatch e\n  print(\"denied touch\")\nend\n";

#[test]
fn sandbox_denies_the_file_writing_builtins() {
    let dir = scratch("sandbox");
    let (_code, text) = run_with("--sandbox", &dir, WRITES);
    for name in ["write_text", "append_text", "mkdir", "touch"] {
        assert!(text.contains(&format!("denied {name}")), "{name} was not denied: {text}");
    }
    for leftover in ["a.txt", "b.txt", "c.txt", "sub_dir"] {
        assert!(!dir.join(leftover).exists(), "{leftover} was created under --sandbox");
    }
}

#[test]
fn dry_run_skips_the_same_set_and_reports_each() {
    let dir = scratch("dry");
    let (code, text) = run_with("--dry-run", &dir, WRITES);
    assert_eq!(code, 0, "{text}");
    for name in ["write_text", "append_text", "mkdir", "touch"] {
        assert!(text.contains(&format!("dry-run: skipped {name}")), "{name} not reported: {text}");
    }
    for leftover in ["a.txt", "b.txt", "c.txt", "sub_dir"] {
        assert!(!dir.join(leftover).exists(), "{leftover} was created under --dry-run");
    }
}

#[test]
fn dry_run_refuses_a_write_mode_fopen_instead_of_faking_a_handle() {
    let dir = scratch("dryfopen");
    let (code, text) = run_with("--dry-run", &dir, "f = fopen(\"o.txt\", \"w\")\n");
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("cannot be rehearsed"), "{text}");
    assert!(!dir.join("o.txt").exists());
}
