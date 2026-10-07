//! Tests for `qu editors`. Every filesystem test runs inside its own temp
//! directory; nothing here reads or writes a real editor profile, `PATH` or
//! the registry (the registry is a stub behind `Registry`).

use crate::editors_assets::*;
use crate::editors_check::*;
use crate::editors_detect::*;
use crate::editors_install::*;
use crate::editors_jupyter as jup;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Tmp {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("qu-editors-test-{}-{}-{tag}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn p(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

fn target(dir: &Path, kind: &'static str) -> Target {
    Target { label: "test".into(), kind, dir: dir.to_path_buf(), exists: dir.is_dir() }
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    fn walk(d: &Path, v: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, v);
                } else {
                    v.push(p);
                }
            }
        }
    }
    walk(root, &mut v);
    v.sort();
    v
}

fn count_entries(root: &Path) -> usize {
    std::fs::read_dir(root).map(|r| r.count()).unwrap_or(0)
}

// ------------------------------------------------------------ drift

#[test]
fn drift_every_grammar_covers_every_lexer_keyword() {
    for d in drift_reports() {
        assert!(d.word_count > 0, "{}: no words extracted (parser of the grammar broke?)", d.grammar);
        assert!(
            d.missing.is_empty(),
            "{}: lexer keywords not highlighted: {:?} (qu_lexer::KEYWORDS changed or the grammar drifted)",
            d.grammar,
            d.missing
        );
    }
}

#[test]
fn drift_every_grammar_has_exactly_the_lexer_units() {
    for d in drift_reports() {
        assert!(d.unit_missing.is_empty(), "{}: units missing {:?}", d.grammar, d.unit_missing);
        assert!(d.unit_extra.is_empty(), "{}: units not in lexer {:?}", d.grammar, d.unit_extra);
    }
}

#[test]
fn drift_detects_a_missing_keyword() {
    // The check must be able to fail: remove `none` from a copy of the UDL.
    let udl = NOTEPADPP_FILES[0].data.replace("true false none", "true false");
    let d = drift_of("udl-without-none", &udl_words(&udl));
    assert_eq!(d.missing, vec!["none".to_string()]);
}

#[test]
fn drift_detects_a_missing_unit() {
    let tm = VSCODE_FILES.iter().find(|a| a.rel == "syntaxes/qu.tmLanguage.json").unwrap().data.replace("(Hz|kHz|", "(Hz|");
    let d = drift_of("tm-without-kHz", &tm_words(&tm).unwrap());
    assert!(d.unit_missing.contains(&"kHz".to_string()));
}

#[test]
fn embedded_checks_have_no_failures() {
    let checks = embedded_checks();
    assert!(checks.len() > 15);
    let bad: Vec<String> = checks.iter().filter(|c| c.sev == Sev::Fail).map(|c| format!("{}: {}", c.name, c.detail)).collect();
    assert!(bad.is_empty(), "failed checks:\n{}", bad.join("\n"));
}

#[test]
fn sample_runs_through_the_real_engine() {
    let c = sample_check();
    let bad: Vec<String> = c.iter().filter(|c| c.sev == Sev::Fail).map(|c| format!("{}: {}", c.name, c.detail)).collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(c.iter().any(|c| c.name == "sample .qu: run"));
}

#[test]
fn xml_checker_accepts_and_rejects() {
    assert!(xml_wellformed("<?xml version=\"1.0\"?><a x=\"1\"><b/><!-- c --></a>").is_ok());
    assert!(xml_wellformed("<a><b></a></b>").is_err());
    assert!(xml_wellformed("<a x=1></a>").is_err());
    assert!(xml_wellformed("<a>&bogus;</a>").is_err());
    assert!(xml_wellformed("<a><!-- no -- good --></a>").is_err());
    assert!(xml_wellformed("<a></a><b></b>").is_err());
    assert!(xml_wellformed("<a>").is_err());
    assert!(xml_wellformed(NOTEPADPP_FILES[0].data).is_ok());
}

#[test]
fn sublime_checker_rejects_an_unknown_context_and_missing_main() {
    assert!(sublime_syntax_check(SUBLIME_FILES[0].data).is_ok());
    let broken = SUBLIME_FILES[0].data.replace("include: comments", "include: nope");
    assert!(sublime_syntax_check(&broken).unwrap_err().contains("nope"));
    let nomain = SUBLIME_FILES[0].data.replace("  main:", "  other:");
    assert!(sublime_syntax_check(&nomain).is_err());
}

// ------------------------------------------------------------ install layouts

fn vscode_target(t: &Tmp) -> Target {
    target(&t.p().join("ext"), "ext-dir")
}

fn vs_folder() -> String {
    format!("qu-project.qu-language-{}", vscode_version())
}

#[test]
fn vscode_fresh_install_writes_every_file_and_a_manifest() {
    let t = Tmp::new("vs-fresh");
    let tg = vscode_target(&t);
    let log = install_target(Editor::VsCode, &tg, false).unwrap();
    let root = tg.dir.join(vs_folder());
    for a in VSCODE_FILES {
        assert_eq!(std::fs::read_to_string(root.join(a.rel.replace('/', std::path::MAIN_SEPARATOR_STR))).unwrap(), a.data, "{}", a.rel);
    }
    assert!(root.join(MANIFEST).is_file());
    assert!(!tg.dir.join("extensions.json").exists(), "extensions.json must not be invented");
    assert!(log.iter().any(|l| l.starts_with("created")));
    let st = plugin_state(Editor::VsCode, &tg);
    assert!(st.installed && st.managed && st.up_to_date && st.modified.is_empty(), "{st:?}");
}

#[test]
fn vscode_updates_extensions_json_keeping_other_entries() {
    let t = Tmp::new("vs-json");
    let tg = vscode_target(&t);
    write(
        &tg.dir.join("extensions.json"),
        r#"[{"identifier":{"id":"other.ext"},"version":"1.0.0","relativeLocation":"other.ext-1.0.0"}]"#,
    );
    install_target(Editor::VsCode, &tg, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(tg.dir.join("extensions.json")).unwrap()).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["identifier"]["id"], "other.ext");
    assert_eq!(arr[1]["identifier"]["id"], VSCODE_ID);
    assert_eq!(arr[1]["relativeLocation"], vs_folder());
    assert!(tg.dir.join("extensions.json.bak").is_file(), "extensions.json must be backed up once");
    let st = plugin_state(Editor::VsCode, &tg);
    assert_eq!(st.in_extensions_json, Some(true));
}

#[test]
fn vscode_reinstall_is_idempotent() {
    let t = Tmp::new("vs-idem");
    let tg = vscode_target(&t);
    write(&tg.dir.join("extensions.json"), "[]");
    install_target(Editor::VsCode, &tg, false).unwrap();
    let before: Vec<(PathBuf, Vec<u8>)> = all_files(t.p()).into_iter().map(|p| { let d = std::fs::read(&p).unwrap(); (p, d) }).collect();
    let log = install_target(Editor::VsCode, &tg, false).unwrap();
    let after: Vec<(PathBuf, Vec<u8>)> = all_files(t.p()).into_iter().map(|p| { let d = std::fs::read(&p).unwrap(); (p, d) }).collect();
    assert_eq!(before, after, "a second install changed bytes on disk");
    assert!(log.iter().all(|l| l.starts_with("unchanged")), "{log:?}");
}

#[test]
fn vscode_upgrade_replaces_the_hand_installed_old_folder() {
    let t = Tmp::new("vs-upgrade");
    let tg = vscode_target(&t);
    let old = tg.dir.join("qu-language-0.1.0");
    write(&old.join("package.json"), r#"{"name":"qu-language","version":"0.1.0"}"#);
    write(&old.join("extension.js"), "// old");
    let stranger = tg.dir.join("qu-language-tools-1.0.0");
    write(&stranger.join("package.json"), r#"{"name":"someone-elses","version":"1.0.0"}"#);
    write(
        &tg.dir.join("extensions.json"),
        r#"[{"identifier":{"id":"qu-language"},"version":"0.1.0","relativeLocation":"qu-language-0.1.0"}]"#,
    );
    let before = plugin_state(Editor::VsCode, &tg);
    assert!(before.installed && !before.managed && !before.up_to_date);
    assert_eq!(before.version.as_deref(), Some("0.1.0"));

    install_target(Editor::VsCode, &tg, false).unwrap();
    assert!(!old.exists(), "the old qu-language-0.1.0 folder must be removed");
    assert!(stranger.exists(), "a folder whose package.json is not qu-language must be left alone");
    assert!(tg.dir.join(vs_folder()).is_dir());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(tg.dir.join("extensions.json")).unwrap()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1, "old entry must be replaced, not duplicated");
    assert_eq!(v[0]["relativeLocation"], vs_folder());
    let after = plugin_state(Editor::VsCode, &tg);
    assert!(after.up_to_date && after.managed);
}

#[test]
fn user_modified_file_is_backed_up_before_overwrite() {
    let t = Tmp::new("vs-mod");
    let tg = vscode_target(&t);
    install_target(Editor::VsCode, &tg, false).unwrap();
    let f = tg.dir.join(vs_folder()).join("language-configuration.json");
    std::fs::write(&f, "{\"user\": \"edit\"}").unwrap();
    let st = plugin_state(Editor::VsCode, &tg);
    assert_eq!(st.modified, vec!["language-configuration.json".to_string()]);
    let log = install_target(Editor::VsCode, &tg, false).unwrap();
    let bak = f.with_file_name("language-configuration.json.bak");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "{\"user\": \"edit\"}");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), VSCODE_FILES.iter().find(|a| a.rel == "language-configuration.json").unwrap().data);
    assert!(log.iter().any(|l| l.contains("backed up")), "{log:?}");
}

#[test]
fn an_unmodified_file_of_an_older_install_is_replaced_without_a_backup() {
    // Simulate an older qu: a manifest whose hash matches the old content.
    let t = Tmp::new("vs-old-ours");
    let tg = vscode_target(&t);
    install_target(Editor::VsCode, &tg, false).unwrap();
    let root = tg.dir.join(vs_folder());
    let f = root.join("extension.js");
    std::fs::write(&f, "// older version of ours").unwrap();
    let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join(MANIFEST)).unwrap()).unwrap();
    m["files"]["extension.js"] = serde_json::json!(fnv64_hex(b"// older version of ours"));
    std::fs::write(root.join(MANIFEST), m.to_string()).unwrap();
    install_target(Editor::VsCode, &tg, false).unwrap();
    assert!(!root.join("extension.js.bak").exists());
    assert!(std::fs::read_to_string(&f).unwrap().contains("Qu editor plugin"));
}

#[test]
fn vscode_uninstall_removes_only_what_install_wrote() {
    let t = Tmp::new("vs-un");
    let tg = vscode_target(&t);
    write(
        &tg.dir.join("extensions.json"),
        r#"[{"identifier":{"id":"other.ext"},"version":"1.0.0","relativeLocation":"other.ext-1.0.0"}]"#,
    );
    write(&tg.dir.join("other.ext-1.0.0").join("package.json"), "{}");
    install_target(Editor::VsCode, &tg, false).unwrap();
    let root = tg.dir.join(vs_folder());
    write(&root.join("my-notes.txt"), "mine"); // not ours
    std::fs::write(root.join("README.md"), "edited by user").unwrap(); // ours but modified

    let log = uninstall_target(Editor::VsCode, &tg, false).unwrap();
    assert!(!root.join("extension.js").exists());
    assert!(!root.join(MANIFEST).exists());
    assert_eq!(std::fs::read_to_string(root.join("my-notes.txt")).unwrap(), "mine");
    assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(), "edited by user");
    assert!(log.iter().any(|l| l.contains("modified since install")), "{log:?}");
    assert!(tg.dir.join("other.ext-1.0.0").join("package.json").is_file());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(tg.dir.join("extensions.json")).unwrap()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["identifier"]["id"], "other.ext");
}

#[test]
fn uninstall_leaves_an_unmanaged_copy_alone() {
    let t = Tmp::new("vs-un-unmanaged");
    let tg = vscode_target(&t);
    let root = tg.dir.join(vs_folder());
    write(&root.join("package.json"), "{}");
    let log = uninstall_target(Editor::VsCode, &tg, false).unwrap();
    assert!(root.join("package.json").is_file());
    assert!(log[0].contains("left alone"), "{log:?}");
}

#[test]
fn dry_run_writes_nothing_for_install_and_uninstall() {
    let t = Tmp::new("dry");
    for (ed, sub) in [
        (Editor::VsCode, "ext"),
        (Editor::Cursor, "cursor-ext"),
        (Editor::NotepadPp, "udl"),
        (Editor::Sublime, "User"),
        (Editor::Vim, "vimfiles"),
        (Editor::Nvim, "nvim"),
    ] {
        let tg = target(&t.p().join(sub), "x");
        let log = install_target(ed, &tg, true).unwrap();
        assert!(log.iter().any(|l| l.starts_with("would")), "{ed:?}: {log:?}");
        assert!(!t.p().join(sub).exists(), "{ed:?}: dry run created {sub}");
    }
    assert_eq!(count_entries(t.p()), 0);

    // dry uninstall of a real install changes nothing either
    let tg = vscode_target(&t);
    install_target(Editor::VsCode, &tg, false).unwrap();
    let before = all_files(t.p());
    uninstall_target(Editor::VsCode, &tg, true).unwrap();
    assert_eq!(before, all_files(t.p()));
}

#[test]
fn notepadpp_install_and_uninstall_keep_other_udls() {
    let t = Tmp::new("npp");
    let tg = target(&t.p().join("userDefineLangs"), "udl-dir");
    write(&tg.dir.join("Other.xml"), "<x/>");
    // an older hand-copied Qu.udl.xml (as the Studio installer left it)
    write(&tg.dir.join("Qu.udl.xml"), "<NotepadPlus><UserLang name=\"Qu\"/></NotepadPlus>");
    let log = install_target(Editor::NotepadPp, &tg, false).unwrap();
    assert!(log.iter().any(|l| l.contains("backed up")), "{log:?}");
    assert!(tg.dir.join("Qu.udl.xml.bak").is_file());
    assert_eq!(std::fs::read_to_string(tg.dir.join("Qu.udl.xml")).unwrap(), NOTEPADPP_FILES[0].data);
    uninstall_target(Editor::NotepadPp, &tg, false).unwrap();
    assert!(!tg.dir.join("Qu.udl.xml").exists());
    assert!(tg.dir.join("Other.xml").is_file());
    assert!(tg.dir.is_dir(), "Notepad++'s own folder must not be pruned");
}

#[test]
fn sublime_installs_into_a_qu_package_folder() {
    let t = Tmp::new("sub");
    let tg = target(&t.p().join("Packages").join("User"), "packages-dir");
    install_target(Editor::Sublime, &tg, false).unwrap();
    assert!(tg.dir.join("Qu").join("Qu.sublime-syntax").is_file());
    assert!(tg.dir.join("Qu").join("Qu.sublime-build").is_file());
    uninstall_target(Editor::Sublime, &tg, false).unwrap();
    assert!(!tg.dir.join("Qu").exists());
    assert!(tg.dir.is_dir());
}

#[test]
fn vim_and_nvim_use_the_native_package_layout() {
    for ed in [Editor::Vim, Editor::Nvim] {
        let t = Tmp::new("vim");
        let tg = target(&t.p().join("cfg"), "pack-dir");
        install_target(ed, &tg, false).unwrap();
        let root = tg.dir.join("pack").join("qu").join("start").join("qu");
        for rel in ["syntax/qu.vim", "ftdetect/qu.vim", "ftplugin/qu.vim"] {
            assert!(root.join(rel).is_file(), "{rel}");
        }
        let again = install_target(ed, &tg, false).unwrap();
        assert!(again.iter().all(|l| l.starts_with("unchanged")), "{again:?}");
        uninstall_target(ed, &tg, false).unwrap();
        assert!(!tg.dir.join("pack").exists(), "empty pack dirs must be pruned");
        assert!(tg.dir.is_dir());
    }
}

// ------------------------------------------------------------ detection

struct Stub {
    app_paths: Vec<(String, PathBuf)>,
    entries: Vec<UninstallEntry>,
}
impl Registry for Stub {
    fn app_path(&self, exe: &str) -> Vec<PathBuf> {
        self.app_paths.iter().filter(|(n, _)| n.eq_ignore_ascii_case(exe)).map(|(_, p)| p.clone()).collect()
    }
    fn uninstall_entries(&self) -> Vec<UninstallEntry> {
        self.entries.clone()
    }
}

fn win_env(t: &Tmp) -> Env {
    Env::sandbox(t.p(), Platform::Windows)
}

#[test]
fn detect_nothing_in_an_empty_sandbox_still_reports_default_targets() {
    let t = Tmp::new("det-empty");
    let env = win_env(&t);
    for d in detect_all(&env) {
        assert!(!d.found, "{:?}", d.editor);
    }
    let vs = detect(&env, Editor::VsCode);
    assert_eq!(vs.targets[0].dir, t.p().join("home").join(".vscode").join("extensions"));
    let npp = detect(&env, Editor::NotepadPp);
    assert_eq!(npp.targets[0].dir, t.p().join("AppData/Roaming/Notepad++/userDefineLangs"));
    let nvim = detect(&env, Editor::Nvim);
    assert_eq!(nvim.targets[0].dir, t.p().join("AppData/Local/nvim"));
    let vim = detect(&env, Editor::Vim);
    assert_eq!(vim.targets[0].dir, t.p().join("home/vimfiles"));
}

#[test]
fn detect_program_files_and_localappdata_programs() {
    let t = Tmp::new("det-pf");
    let r = t.p();
    touch(&r.join("Program Files/Microsoft VS Code/Code.exe"));
    touch(&r.join("Program Files/VSCodium/VSCodium.exe"));
    touch(&r.join("Program Files/Notepad++/notepad++.exe"));
    touch(&r.join("Program Files/Sublime Text/sublime_text.exe"));
    touch(&r.join("AppData/Local/Programs/cursor/Cursor.exe"));
    touch(&r.join("Program Files/Vim/vim91/vim.exe"));
    touch(&r.join("Program Files/Neovim/bin/nvim.exe"));
    let env = win_env(&t);
    for (ed, tail) in [
        (Editor::VsCode, "Code.exe"),
        (Editor::VsCodium, "VSCodium.exe"),
        (Editor::NotepadPp, "notepad++.exe"),
        (Editor::Sublime, "sublime_text.exe"),
        (Editor::Cursor, "Cursor.exe"),
        (Editor::Vim, "vim.exe"),
        (Editor::Nvim, "nvim.exe"),
    ] {
        let d = detect(&env, ed);
        assert!(d.found, "{ed:?}");
        assert_eq!(d.method, "known-folder", "{ed:?}");
        assert!(d.exe.as_ref().unwrap().ends_with(tail), "{ed:?}: {:?}", d.exe);
    }
    assert!(!detect(&env, Editor::Windsurf).found);
}

#[test]
fn detect_via_registry_app_paths_in_a_custom_folder() {
    let t = Tmp::new("det-reg");
    let custom = t.p().join("D").join("Tools").join("Code");
    touch(&custom.join("Code.exe"));
    let mut env = win_env(&t);
    env.registry = Box::new(Stub { app_paths: vec![("Code.exe".into(), custom.join("Code.exe"))], entries: vec![] });
    let d = detect(&env, Editor::VsCode);
    assert!(d.found);
    assert_eq!(d.method, "registry:app-paths");
    assert_eq!(d.exe.unwrap(), custom.join("Code.exe"));
}

#[test]
fn detect_via_registry_uninstall_entry_and_ignore_stale_ones() {
    let t = Tmp::new("det-unin");
    let loc = t.p().join("Apps").join("Notepad++");
    touch(&loc.join("notepad++.exe"));
    let mut env = win_env(&t);
    env.registry = Box::new(Stub {
        app_paths: vec![],
        entries: vec![
            UninstallEntry { display_name: "Notepad++ (64-bit x64)".into(), install_location: Some(loc.to_string_lossy().into_owned()), display_icon: None, display_version: Some("8.6.9".into()) },
            // stale: the folder is gone
            UninstallEntry { display_name: "Sublime Text".into(), install_location: Some(t.p().join("gone").to_string_lossy().into_owned()), display_icon: None, display_version: None },
            // not an editor
            UninstallEntry { display_name: "Mouse Cursor Pack".into(), install_location: Some(loc.to_string_lossy().into_owned()), display_icon: None, display_version: None },
        ],
    });
    let n = detect(&env, Editor::NotepadPp);
    assert!(n.found);
    assert_eq!(n.method, "registry:uninstall");
    assert_eq!(n.version.as_deref(), Some("8.6.9"));
    assert!(!detect(&env, Editor::Sublime).found, "stale Uninstall entry must not count");
    assert!(!detect(&env, Editor::Cursor).found, "'Mouse Cursor Pack' is not Cursor");
}

#[test]
fn detect_on_path_resolves_the_launcher_to_the_real_exe_and_cli() {
    let t = Tmp::new("det-path");
    let base = t.p().join("custom").join("VSCode");
    touch(&base.join("Code.exe"));
    touch(&base.join("bin").join("code.cmd"));
    let mut env = win_env(&t);
    env.path_dirs = vec![base.join("bin")];
    let d = detect(&env, Editor::VsCode);
    assert!(d.found);
    assert_eq!(d.method, "PATH");
    assert_eq!(d.exe.as_ref().unwrap(), &base.join("Code.exe"));
    assert_eq!(d.cli.as_ref().unwrap(), &base.join("bin").join("code.cmd"));
}

#[test]
fn detect_cursor_through_its_deep_cli_folder_and_read_the_version() {
    let t = Tmp::new("det-cursor");
    let base = t.p().join("cur");
    touch(&base.join("Cursor.exe"));
    touch(&base.join("resources/app/bin/cursor.cmd"));
    write(&base.join("resources/app/package.json"), r#"{"version":"1.2.3"}"#);
    let mut env = win_env(&t);
    env.path_dirs = vec![base.join("resources/app/bin")];
    let d = detect(&env, Editor::Cursor);
    assert_eq!(d.exe.as_ref().unwrap(), &base.join("Cursor.exe"));
    assert_eq!(d.version.as_deref(), Some("1.2.3"));
}

#[test]
fn portable_vscode_uses_the_data_folder_next_to_the_exe() {
    let t = Tmp::new("det-port-vs");
    let base = t.p().join("usb").join("VSCode-win32-x64");
    touch(&base.join("Code.exe"));
    std::fs::create_dir_all(base.join("data").join("extensions")).unwrap();
    let mut env = win_env(&t);
    env.registry = Box::new(Stub { app_paths: vec![("Code.exe".into(), base.join("Code.exe"))], entries: vec![] });
    let d = detect(&env, Editor::VsCode);
    assert!(d.portable);
    assert_eq!(d.targets[0].dir, base.join("data").join("extensions"));
    install_target(Editor::VsCode, &d.targets[0], false).unwrap();
    assert!(base.join("data/extensions").join(vs_folder()).is_dir());
    assert!(!t.p().join("home/.vscode").exists(), "portable install must not touch the profile");
}

#[test]
fn portable_notepadpp_honours_doLocalConf() {
    let t = Tmp::new("det-port-npp");
    let base = t.p().join("usb").join("npp");
    touch(&base.join("notepad++.exe"));
    touch(&base.join("doLocalConf.xml"));
    let mut env = win_env(&t);
    env.path_dirs = vec![base.clone()];
    let d = detect(&env, Editor::NotepadPp);
    assert!(d.found && d.portable);
    assert_eq!(d.targets[0].dir, base.join("userDefineLangs"));
    // without doLocalConf.xml the config is in %APPDATA%
    std::fs::remove_file(base.join("doLocalConf.xml")).unwrap();
    let d = detect(&env, Editor::NotepadPp);
    assert!(!d.portable);
    assert_eq!(d.targets[0].dir, t.p().join("AppData/Roaming/Notepad++/userDefineLangs"));
}

#[test]
fn portable_sublime_uses_its_data_folder_and_installed_sublime_finds_both_versions() {
    let t = Tmp::new("det-sub");
    let base = t.p().join("usb").join("sublime");
    touch(&base.join("sublime_text.exe"));
    std::fs::create_dir_all(base.join("Data")).unwrap();
    let mut env = win_env(&t);
    env.path_dirs = vec![base.clone()];
    env.path_dirs.push(base.clone());
    let mut e2 = win_env(&t);
    e2.registry = Box::new(Stub { app_paths: vec![("sublime_text.exe".into(), base.join("sublime_text.exe"))], entries: vec![] });
    let d = detect(&e2, Editor::Sublime);
    assert!(d.portable);
    assert_eq!(d.targets[0].dir, base.join("Data/Packages/User"));

    // installed (non-portable) Sublime with both data dirs
    let t2 = Tmp::new("det-sub2");
    touch(&t2.p().join("Program Files/Sublime Text/sublime_text.exe"));
    std::fs::create_dir_all(t2.p().join("AppData/Roaming/Sublime Text/Packages")).unwrap();
    std::fs::create_dir_all(t2.p().join("AppData/Roaming/Sublime Text 3/Packages")).unwrap();
    let d = detect(&win_env(&t2), Editor::Sublime);
    assert_eq!(d.targets.len(), 2);
    assert_eq!(d.targets[0].dir, t2.p().join("AppData/Roaming/Sublime Text/Packages/User"));
    assert_eq!(d.targets[1].dir, t2.p().join("AppData/Roaming/Sublime Text 3/Packages/User"));
}

#[test]
fn detect_linux_and_macos_layouts() {
    let t = Tmp::new("det-nix");
    touch(&t.p().join("fs/usr/bin/code"));
    std::fs::create_dir_all(t.p().join("home/.var/app/com.vscodium.codium")).unwrap();
    let env = Env::sandbox(t.p(), Platform::Linux);
    let d = detect(&env, Editor::VsCode);
    assert!(d.found);
    assert_eq!(d.method, "known-path");
    assert_eq!(d.targets[0].dir, t.p().join("home/.vscode/extensions"));
    let c = detect(&env, Editor::VsCodium);
    assert_eq!(c.method, "flatpak");
    assert_eq!(c.targets[0].dir, t.p().join("home/.var/app/com.vscodium.codium/data/codium/extensions"));
    assert_eq!(detect(&env, Editor::Nvim).targets[0].dir, t.p().join("home/.config/nvim"));
    assert_eq!(detect(&env, Editor::Vim).targets[0].dir, t.p().join("home/.vim"));
    assert!(!detect(&env, Editor::NotepadPp).found);
    assert!(detect(&env, Editor::NotepadPp).targets.is_empty());
    assert_eq!(detect(&env, Editor::Sublime).targets[0].dir, t.p().join("home/.config/sublime-text/Packages/User"));

    std::fs::create_dir_all(t.p().join("fs/Applications/Cursor.app")).unwrap();
    let mac = Env::sandbox(t.p(), Platform::MacOs);
    let d = detect(&mac, Editor::Cursor);
    assert!(d.found);
    assert_eq!(d.method, "applications");
    assert_eq!(detect(&mac, Editor::Sublime).targets[0].dir, t.p().join("home/Library/Application Support/Sublime Text/Packages/User"));
}

#[test]
fn registry_output_parsers() {
    let default = "\r\nHKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Code.exe\r\n    (Default)    REG_SZ    C:\\Program Files\\Microsoft VS Code\\Code.exe\r\n    Path    REG_SZ    C:\\Program Files\\Microsoft VS Code\r\n";
    assert_eq!(parse_reg_default(default).as_deref(), Some("C:\\Program Files\\Microsoft VS Code\\Code.exe"));
    let un = "HKEY_LOCAL_MACHINE\\...\\Uninstall\\Notepad++\r\n    DisplayName    REG_SZ    Notepad++ (64-bit x64)\r\n    InstallLocation    REG_SZ    C:\\Program Files\\Notepad++\r\n    DisplayVersion    REG_SZ    8.6.9\r\n\r\nHKEY_LOCAL_MACHINE\\...\\Uninstall\\NoName\r\n    Foo    REG_DWORD    0x1\r\n";
    let v = parse_reg_uninstall(un);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].display_name, "Notepad++ (64-bit x64)");
    assert_eq!(v[0].install_location.as_deref(), Some("C:\\Program Files\\Notepad++"));
    assert_eq!(v[0].display_version.as_deref(), Some("8.6.9"));
}

// ------------------------------------------------------------ jupyter

fn kernel_env(t: &Tmp) -> Env {
    let mut env = Env::sandbox(t.p(), Platform::Windows);
    env.jupyter_data_dir = Some(t.p().join("jdata"));
    env.qu_exe = Some(t.p().join("bin").join("qu.exe"));
    env
}

fn write_kernel(t: &Tmp, json: &str) {
    write(&t.p().join("jdata/kernels/qu/kernel.json"), json);
}

#[test]
fn jupyter_not_registered_is_none() {
    let t = Tmp::new("jup-none");
    assert!(jup::kernel_health(&kernel_env(&t), false).is_none());
}

#[test]
fn jupyter_healthy_kernelspec() {
    let t = Tmp::new("jup-ok");
    let exe = t.p().join("bin").join("qu-jupyter.exe");
    touch(&exe);
    write_kernel(&t, &serde_json::json!({"argv": [exe.to_string_lossy(), "-f", "{connection_file}"], "display_name": "Qu", "language": "qu"}).to_string());
    let h = jup::kernel_health(&kernel_env(&t), false).unwrap();
    assert!(h.problems.is_empty(), "{:?}", h.problems);
    assert_eq!(h.argv0.as_deref(), Some(exe.to_string_lossy().as_ref()));
}

#[test]
fn jupyter_detects_each_kind_of_breakage() {
    let t = Tmp::new("jup-bad");
    let env = kernel_env(&t);
    write_kernel(&t, "{ not json");
    assert!(jup::kernel_health(&env, false).unwrap().problems[0].contains("not valid JSON"));
    write_kernel(&t, r#"{"language":"qu"}"#);
    assert!(jup::kernel_health(&env, false).unwrap().problems[0].contains("argv"));
    write_kernel(&t, r#"{"argv":["Z:\\nowhere\\qu-jupyter.exe","-f","{connection_file}"]}"#);
    assert!(jup::kernel_health(&env, false).unwrap().problems.iter().any(|p| p.contains("does not exist")));
    let exe = t.p().join("bin").join("qu-jupyter.exe");
    touch(&exe);
    write_kernel(&t, &serde_json::json!({"argv": [exe.to_string_lossy(), "-f", "conn.json"]}).to_string());
    assert!(jup::kernel_health(&env, false).unwrap().problems.iter().any(|p| p.contains("{connection_file}")));
}

#[test]
fn jupyter_install_without_the_sibling_binary_says_so_and_dry_run_writes_nothing() {
    let t = Tmp::new("jup-inst");
    let env = kernel_env(&t);
    let e = jup::install(&env, false).unwrap_err();
    assert!(e.starts_with("qu-jupyter not found"), "{e}");
    assert!(!t.p().join("jdata").exists());
    touch(&t.p().join("bin").join("qu-jupyter.exe"));
    let log = jup::install(&env, true).unwrap();
    assert!(log[0].starts_with("would run"));
    assert!(!t.p().join("jdata").exists());
}

#[test]
fn jupyter_uninstall_removes_only_the_qu_kernelspec() {
    let t = Tmp::new("jup-un");
    let env = kernel_env(&t);
    write_kernel(&t, r#"{"argv":["x"]}"#);
    write(&t.p().join("jdata/kernels/python3/kernel.json"), "{}");
    write(&t.p().join("jdata/kernels/qu/user-logo.png"), "png");
    jup::uninstall(&env, true).unwrap();
    assert!(t.p().join("jdata/kernels/qu/kernel.json").is_file(), "dry run");
    jup::uninstall(&env, false).unwrap();
    assert!(!t.p().join("jdata/kernels/qu/kernel.json").exists());
    assert!(t.p().join("jdata/kernels/qu/user-logo.png").is_file());
    assert!(t.p().join("jdata/kernels/python3/kernel.json").is_file());
}

#[test]
fn run_timeout_captures_a_fast_child_and_kills_a_slow_one() {
    // The test binary itself is a fast child (`--list` prints and exits).
    let me = std::env::current_exe().unwrap();
    let c = jup::run_timeout(me.as_os_str(), &["--list".into()], &[], std::time::Duration::from_secs(60)).unwrap();
    assert!(!c.timed_out);
    assert_eq!(c.code, Some(0));
    // A child that cannot start is an error, not a hang or a panic.
    assert!(jup::run_timeout(t_missing().as_os_str(), &[], &[], std::time::Duration::from_secs(1)).is_err());
}

fn t_missing() -> PathBuf {
    std::env::temp_dir().join("qu-editors-definitely-not-a-program.exe")
}
