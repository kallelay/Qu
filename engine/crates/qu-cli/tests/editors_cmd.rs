//! `qu editors` end to end, through the real binary, always with `--root
//! <temp dir>` so nothing outside the temp dir is read or written.

use std::path::{Path, PathBuf};
use std::process::Command;

fn qu() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_qu"));
    // a developer's real environment must not leak into the sandbox
    for v in ["QU_EDITORS_ROOT", "QU_EDITORS_PATH", "QU_EDITORS_PLATFORM", "QU_EDITORS_JUPYTER_BIN"] {
        c.env_remove(v);
    }
    c
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("qu-editors-it-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn run(root: &Path, args: &[&str]) -> (i32, String) {
    let mut c = qu();
    c.arg("editors").args(args).arg("--root").arg(root).env("QU_EDITORS_PLATFORM", "windows");
    let out = c.output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

#[test]
fn detect_exit_code_is_the_answer() {
    let r = tmp("detect");
    assert_eq!(run(&r, &["detect", "vscode"]).0, 1);
    touch(&r.join("Program Files/Microsoft VS Code/Code.exe"));
    let (code, out) = run(&r, &["detect", "vscode"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("vscode"), "{out}");
    assert!(out.contains("found") && out.contains("ext-dir="), "{out}");
    let (code, out) = run(&r, &["detect", "vscode", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["found"], true);
    assert_eq!(v["method"], "known-folder");
    let (code, _) = run(&r, &["detect", "nosuchthing"]);
    assert_ne!(code, 0);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn install_auto_installs_only_what_is_detected_and_dry_run_writes_nothing() {
    let r = tmp("auto");
    let (code, out) = run(&r, &["install"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no supported editor was detected"), "{out}");
    touch(&r.join("Program Files/Notepad++/notepad++.exe"));
    let before: Vec<_> = walk(&r);
    let (code, out) = run(&r, &["install", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("would install"), "{out}");
    assert_eq!(before, walk(&r), "--dry-run changed the disk");
    let (code, out) = run(&r, &["install"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.join("AppData/Roaming/Notepad++/userDefineLangs/Qu.udl.xml").is_file(), "{out}");
    assert!(!r.join("home/.vscode").exists(), "an undetected editor must not be touched in auto mode");
    // check passes, and sees the install as healthy
    let (code, out) = run(&r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("notepadpp plugin"), "{out}");
    // uninstall
    let (code, out) = run(&r, &["uninstall"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.join("AppData/Roaming/Notepad++/userDefineLangs/Qu.udl.xml").exists());
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn install_all_forces_every_default_location_and_check_notices_an_outdated_one() {
    let r = tmp("all");
    let (code, out) = run(&r, &["install", "--editor", "all"]);
    // jupyter has no qu-jupyter beside this test binary's qu? it does not: in
    // `all` mode that is a reported failure, everything else must succeed.
    assert!(out.contains("vscode: installed"), "{out}");
    assert!(out.contains("nvim: installed"), "{out}");
    assert!(out.contains("sublime: installed"), "{out}");
    assert!(out.contains("cursor: installed") && out.contains("windsurf: installed"), "{out}");
    let _ = code;
    assert!(r.join("home/.vscode/extensions").is_dir());
    assert!(r.join("home/.cursor/extensions").is_dir());
    assert!(r.join("home/vimfiles/pack/qu/start/qu/syntax/qu.vim").is_file());
    assert!(r.join("AppData/Local/nvim/pack/qu/start/qu/ftplugin/qu.vim").is_file());
    assert!(r.join("AppData/Roaming/Sublime Text/Packages/User/Qu/Qu.sublime-syntax").is_file());
    let (code, out) = run(&r, &["check"]);
    assert_eq!(code, 0, "{out}");

    // make the VS Code copy look like an older version
    let ext = r.join("home/.vscode/extensions");
    let dir = std::fs::read_dir(&ext).unwrap().flatten().find(|d| d.file_name().to_string_lossy().starts_with("qu-project.qu-language-")).unwrap().path();
    let pj = dir.join("package.json");
    let t = std::fs::read_to_string(&pj).unwrap();
    let v = t.split("\"version\": \"").nth(1).unwrap().split('"').next().unwrap().to_string();
    std::fs::write(&pj, t.replace(&format!("\"version\": \"{v}\""), "\"version\": \"0.0.1\"")).unwrap();
    let (code, out) = run(&r, &["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL") && out.contains("0.0.1"), "{out}");
    // reinstall repairs it
    run(&r, &["install", "--editor", "vscode"]);
    assert_eq!(run(&r, &["check"]).0, 0);

    let (_, out) = run(&r, &["uninstall"]);
    assert!(!out.contains("FAILED"), "{out}");
    assert!(!r.join("home/vimfiles/pack").exists());
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn status_json_reports_qu_path_state_and_plugin_versions() {
    let r = tmp("status");
    run(&r, &["install", "--editor", "vim"]);
    let (code, out) = run(&r, &["status", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["qu"]["on_path"], serde_json::Value::Null, "sandbox PATH is empty");
    let vim = v["editors"].as_array().unwrap().iter().find(|e| e["name"] == "vim").unwrap();
    assert_eq!(vim["plugin"][0]["installed"], true);
    assert_eq!(vim["plugin"][0]["up_to_date"], true);
    let vs = v["editors"].as_array().unwrap().iter().find(|e| e["name"] == "vscode").unwrap();
    assert_eq!(vs["plugin"][0]["installed"], false);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn unknown_editor_and_option_are_errors() {
    let r = tmp("bad");
    assert_ne!(run(&r, &["install", "--editor", "emacs"]).0, 0);
    assert_ne!(run(&r, &["install", "--bogus"]).0, 0);
    let _ = std::fs::remove_dir_all(&r);
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            v.push(p.clone());
            if p.is_dir() {
                v.extend(walk(&p));
            }
        }
    }
    v.sort();
    v
}
