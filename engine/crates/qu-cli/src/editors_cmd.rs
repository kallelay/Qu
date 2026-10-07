//! `qu editors` -- detect, install, check and remove the editor plugins
//! (VS Code, VSCodium, Cursor, Windsurf, Notepad++, Sublime Text, Vim,
//! Neovim) and the Jupyter kernel. See `qu editors --help`.
//!
//! The plugin files are embedded in the binary (`editors_assets`), detection
//! lives in `editors_detect`, file operations in `editors_install`, the
//! validators and the keyword drift check in `editors_check`, and the Jupyter
//! kernelspec in `editors_jupyter`.

use crate::editors_assets::{PluginKind, VSCODE_ID};
use crate::editors_check::{embedded_checks, Check, Sev};
use crate::editors_detect::{detect, detect_all, qu_on_path, Detection, Editor, Env, Target};
use crate::editors_install::{install_target, plugin_kind, plugin_state, uninstall_target, PluginState};
use crate::editors_jupyter as jup;
use serde_json::{json, Value};
use std::time::Duration;

const HELP: &str = "\
qu editors -- editor plugins and the Jupyter kernel

  qu editors detect [<editor>] [--json]
        where each editor is and where its plugin would go; with an editor
        name, exit 0 if found and 1 if not (for installers to branch on)
  qu editors status [--json] [--no-cli]
        detection + is the plugin installed, which version, are its files
        intact, does the editor's CLI list it, is qu itself on PATH
  qu editors install [--editor auto|all|NAME[,NAME]] [--root DIR] [--dry-run]
        copy the embedded plugin files into place (default: auto = every
        detected editor; all = every known location even if not detected)
  qu editors uninstall [--editor ...] [--root DIR] [--dry-run]
        remove only what install wrote (default: all)
  qu editors check [--json] [--no-cli]
        validate every embedded file, compare the grammars' keyword lists with
        the lexer, and check what is installed; exit 0 only if healthy

editors: vscode vscodium cursor windsurf notepadpp sublime vim nvim jupyter
--root DIR (or QU_EDITORS_ROOT) pretends DIR is the whole machine: nothing
outside it is read or written. QU_EDITORS_PLATFORM=windows|macos|linux picks
the layout, QU_EDITORS_PATH adds PATH entries inside a sandbox.
";

#[derive(Default)]
struct Opts {
    editors: Vec<String>,
    root: Option<String>,
    dry: bool,
    json: bool,
    no_cli: bool,
    positional: Vec<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        let mut value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            *i += 1;
            args.get(*i).cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--editor" | "-e" => {
                let v = value(&mut i)?;
                o.editors.extend(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            }
            "--root" => o.root = Some(value(&mut i)?),
            "--dry-run" | "-n" => o.dry = true,
            "--json" => o.json = true,
            "--no-cli" => o.no_cli = true,
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            f if f.starts_with("--") => return Err(format!("unknown option {f} (see `qu editors --help`)")),
            _ => o.positional.push(a.to_string()),
        }
        i += 1;
    }
    Ok(o)
}

enum Sel {
    Auto,
    All,
    Named(Vec<Editor>),
}

fn parse_sel(names: &[String], default_all: bool) -> Result<Sel, String> {
    if names.is_empty() {
        return Ok(if default_all { Sel::All } else { Sel::Auto });
    }
    let mut v = Vec::new();
    for n in names {
        match n.to_ascii_lowercase().as_str() {
            "auto" => return Ok(Sel::Auto),
            "all" => return Ok(Sel::All),
            _ => match Editor::from_name(n) {
                Some(e) => {
                    if !v.contains(&e) {
                        v.push(e)
                    }
                }
                None => {
                    return Err(format!(
                        "unknown editor {n:?}; expected auto, all or one of: {}",
                        Editor::ALL.iter().map(|e| e.name()).collect::<Vec<_>>().join(" ")
                    ))
                }
            },
        }
    }
    Ok(Sel::Named(v))
}

pub fn cmd_editors(args: &[String]) -> Result<(), String> {
    let Some(sub) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    if matches!(sub, "help" | "--help" | "-h") {
        print!("{HELP}");
        return Ok(());
    }
    let o = parse_opts(&args[1..])?;
    let env = Env::from_args(o.root.as_deref());
    match sub {
        "detect" => cmd_detect(&env, &o),
        "status" => cmd_status(&env, &o),
        "install" => cmd_install(&env, &o),
        "uninstall" => cmd_uninstall(&env, &o),
        "check" => cmd_check(&env, &o),
        other => Err(format!("unknown subcommand {other:?} (detect, status, install, uninstall, check)")),
    }
}

fn exit_with(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

// ---------------------------------------------------------------- detect

fn det_json(d: &Detection) -> Value {
    json!({
        "name": d.editor.name(),
        "display": d.editor.display(),
        "found": d.found,
        "exe": d.exe.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "cli": d.cli.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "method": if d.found { Value::String(d.method.clone()) } else { Value::Null },
        "version": d.version,
        "portable": d.portable,
        "note": d.note,
        "targets": d.targets.iter().map(|t| json!({
            "label": t.label, "kind": t.kind, "dir": t.dir.to_string_lossy(), "exists": t.exists,
        })).collect::<Vec<_>>(),
    })
}

fn det_line(d: &Detection) -> String {
    let name = d.editor.name();
    let mut s = if d.found {
        format!("{name:<10}  found  {}", d.exe.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".into()))
    } else {
        format!("{name:<10}  not found  -")
    };
    for t in &d.targets {
        s.push_str(&format!("  {}={}", t.kind, t.dir.display()));
    }
    if !d.found && !d.targets.is_empty() {
        s.push_str("  (default location)");
    }
    if d.found {
        s.push_str(&format!("  via={}", d.method));
        if d.portable {
            s.push_str("  portable");
        }
    }
    if let Some(n) = &d.note {
        s.push_str(&format!("  note={n}"));
    }
    s
}

fn cmd_detect(env: &Env, o: &Opts) -> Result<(), String> {
    if let Some(name) = o.positional.first() {
        let ed = Editor::from_name(name).ok_or_else(|| format!("unknown editor {name:?}"))?;
        let d = detect(env, ed);
        if o.json {
            println!("{}", serde_json::to_string_pretty(&det_json(&d)).unwrap());
        } else {
            println!("{}", det_line(&d));
        }
        exit_with(if d.found { 0 } else { 1 });
    }
    let all = detect_all(env);
    if o.json {
        let v = json!({
            "qu_version": env!("CARGO_PKG_VERSION"),
            "platform": env.platform.name(),
            "editors": all.iter().map(det_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        for d in &all {
            println!("{}", det_line(d));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- install / uninstall

fn selected(env: &Env, sel: &Sel) -> (Vec<Detection>, bool) {
    match sel {
        Sel::Auto => (detect_all(env).into_iter().filter(|d| d.found).collect(), true),
        Sel::All => (detect_all(env), false),
        Sel::Named(v) => (v.iter().map(|e| detect(env, *e)).collect(), false),
    }
}

fn cmd_install(env: &Env, o: &Opts) -> Result<(), String> {
    let sel = parse_sel(&o.editors, false)?;
    let (dets, auto) = selected(env, &sel);
    let dry = if o.dry { " (dry run: nothing is written)" } else { "" };
    if dets.is_empty() {
        println!("no supported editor was detected; nothing to install{dry}");
        println!("(use `qu editors install --editor all` to write to every default location anyway)");
        return Ok(());
    }
    let mut failed = 0;
    let mut attempted = 0;
    for d in &dets {
        let ed = d.editor;
        if !d.found && matches!(sel, Sel::Named(_)) {
            println!("{}: not detected on this machine; installing to the default location", ed.name());
        }
        if ed == Editor::Jupyter {
            attempted += 1;
            match jup::install(env, o.dry) {
                Ok(lines) => {
                    println!("{}: {}{dry}", ed.name(), if o.dry { "would register the kernel" } else { "kernel registered" });
                    for l in lines {
                        println!("    {l}");
                    }
                }
                Err(e) if auto && e.starts_with("qu-jupyter not found") => {
                    attempted -= 1;
                    println!("{}: skipped: {e}", ed.name());
                }
                Err(e) => {
                    failed += 1;
                    println!("{}: FAILED: {e}", ed.name());
                }
            }
            continue;
        }
        if d.targets.is_empty() {
            println!("{}: skipped: {}", ed.name(), d.note.clone().unwrap_or_else(|| "no plugin location on this platform".into()));
            continue;
        }
        for t in &d.targets {
            attempted += 1;
            match install_target(ed, t, o.dry) {
                Ok(lines) => {
                    println!(
                        "{}: {} -> {}{dry}",
                        ed.name(),
                        if o.dry { "would install" } else { "installed" },
                        t.dir.display()
                    );
                    for l in lines {
                        println!("    {l}");
                    }
                    if plugin_kind(ed) == Some(PluginKind::VsCode) && !o.dry {
                        println!("    restart {} (or run \"Developer: Reload Window\") to load it", ed.display());
                    }
                }
                Err(e) => {
                    failed += 1;
                    println!("{}: FAILED: {e}", ed.name());
                }
            }
        }
    }
    println!("{attempted} install(s) attempted, {failed} failed");
    if failed > 0 {
        exit_with(1);
    }
    Ok(())
}

fn cmd_uninstall(env: &Env, o: &Opts) -> Result<(), String> {
    let sel = parse_sel(&o.editors, true)?;
    let (dets, _) = selected(env, &sel);
    let mut failed = 0;
    for d in &dets {
        let ed = d.editor;
        if ed == Editor::Jupyter {
            match jup::uninstall(env, o.dry) {
                Ok(lines) => lines.iter().for_each(|l| println!("{}: {l}", ed.name())),
                Err(e) => {
                    failed += 1;
                    println!("{}: FAILED: {e}", ed.name());
                }
            }
            continue;
        }
        for t in &d.targets {
            match uninstall_target(ed, t, o.dry) {
                Ok(lines) => {
                    for l in lines {
                        println!("{}: {l}", ed.name());
                    }
                }
                Err(e) => {
                    failed += 1;
                    println!("{}: FAILED: {e}", ed.name());
                }
            }
        }
    }
    if failed > 0 {
        exit_with(1);
    }
    Ok(())
}

// ---------------------------------------------------------------- status

/// `id@version` lines from `<cli> --list-extensions --show-versions`.
fn cli_extensions(env: &Env, d: &Detection, o: &Opts) -> Option<Result<Vec<String>, String>> {
    if env.sandboxed || o.no_cli {
        return None;
    }
    let cli = d.cli.as_ref()?;
    let r = jup::run_timeout(
        cli.as_os_str(),
        &["--list-extensions".into(), "--show-versions".into()],
        &[],
        Duration::from_secs(30),
    );
    Some(match r {
        Ok(c) if c.timed_out => Err("timed out after 30 s".to_string()),
        Ok(c) if c.code == Some(0) => Ok(c.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()),
        Ok(c) => Err(format!("exit {:?}: {}", c.code, c.stderr.lines().next().unwrap_or("").trim())),
        Err(e) => Err(e),
    })
}

fn listed_version(list: &[String], id: &str) -> Option<String> {
    list.iter().find_map(|l| {
        let (name, ver) = l.split_once('@').unwrap_or((l.as_str(), ""));
        if name.eq_ignore_ascii_case(id) {
            Some(ver.to_string())
        } else {
            None
        }
    })
}

/// Is Microsoft's Jupyter extension in this extensions dir? -> (installed, version)
fn jupyter_ext_in_dir(t: &Target) -> (bool, Option<String>) {
    let mut best: Option<String> = None;
    if let Ok(rd) = std::fs::read_dir(&t.dir) {
        for d in rd.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.to_ascii_lowercase().strip_prefix("ms-toolsai.jupyter-") {
                if rest.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    best = Some(v);
                }
            }
        }
    }
    (best.is_some(), best)
}

fn describe_state(ed: Editor, st: &PluginState) -> String {
    let _ = ed;
    if !st.installed {
        return format!("NOT installed (this qu ships {}) -- run `qu editors install --editor {}`", st.shipped, ed.name());
    }
    let ver = st.version.clone().unwrap_or_else(|| "unknown version".into());
    let mut s = if st.up_to_date {
        format!("installed {ver}, up to date")
    } else if !st.missing.is_empty() && st.version.as_deref() == Some(st.shipped.as_str()) {
        format!("installed {ver} but files are missing: {}", st.missing.join(", "))
    } else {
        format!("OUTDATED: installed {ver}, this qu ships {} -- run `qu editors install`", st.shipped)
    };
    if st.managed {
        s.push_str(", installed by qu editors");
    }
    if !st.modified.is_empty() {
        s.push_str(&format!("; modified: {}", st.modified.join(", ")));
    }
    s
}

fn state_json(st: &PluginState) -> Value {
    json!({
        "installed": st.installed,
        "managed": st.managed,
        "version": st.version,
        "shipped_version": st.shipped,
        "up_to_date": st.up_to_date,
        "files_intact": st.installed && st.modified.is_empty() && st.missing.is_empty(),
        "modified": st.modified,
        "missing": st.missing,
        "notes": st.notes,
        "root": st.root.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "in_extensions_json": st.in_extensions_json,
    })
}

fn qu_section(env: &Env) -> (Vec<String>, Value) {
    let me = env.qu_exe.clone();
    let onpath = qu_on_path(env);
    let mut lines = Vec::new();
    lines.push(format!(
        "qu {} at {}",
        env!("CARGO_PKG_VERSION"),
        me.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(unknown)".into())
    ));
    match &onpath {
        Some(p) => lines.push(format!("qu on PATH: {}", p.display())),
        None => lines.push(format!(
            "qu on PATH: NO -- the editor extensions will not find qu by default. Set the editor setting qu.executablePath to {} (or put its folder on PATH)",
            me.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "the full path of qu".into())
        )),
    }
    let v = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "this_binary": me.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "on_path": onpath.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "extension_will_use": onpath.as_ref().map(|p| p.to_string_lossy().into_owned()),
    });
    (lines, v)
}

fn jupyter_status(env: &Env, smoke: bool) -> (Vec<String>, Value) {
    let mut lines = Vec::new();
    let bin = jup::qu_jupyter_bin(env);
    let h = jup::kernel_health(env, smoke);
    let v = match &h {
        None => {
            lines.push(format!(
                "kernelspec `qu`: NOT registered (searched {}) -- run `qu editors install --editor jupyter`",
                env.jupyter_kernel_roots().iter().map(|p| p.join("kernels").display().to_string()).collect::<Vec<_>>().join(", ")
            ));
            json!({"kernelspec": null})
        }
        Some(h) => {
            lines.push(format!(
                "kernelspec `qu`: {} (argv[0] = {})",
                h.dir.display(),
                h.argv0.clone().unwrap_or_else(|| "?".into())
            ));
            if h.problems.is_empty() {
                lines.push("  healthy: argv[0] exists and is executable, {connection_file} present".to_string());
            }
            for p in &h.problems {
                lines.push(format!("  PROBLEM: {p}"));
            }
            for n in &h.notes {
                lines.push(format!("  note: {n}"));
            }
            json!({"kernelspec": {"dir": h.dir.to_string_lossy(), "argv0": h.argv0, "healthy": h.problems.is_empty(), "problems": h.problems, "notes": h.notes}})
        }
    };
    lines.push(format!(
        "qu-jupyter next to qu: {}",
        if bin.is_file() { bin.display().to_string() } else { format!("NOT FOUND (expected {})", bin.display()) }
    ));
    (lines, json!({"qu_jupyter": bin.to_string_lossy(), "qu_jupyter_exists": bin.is_file(), "kernel": v}))
}

fn cmd_status(env: &Env, o: &Opts) -> Result<(), String> {
    let (qlines, qjson) = qu_section(env);
    let mut text = qlines;
    let mut editors = Vec::new();
    for d in detect_all(env) {
        let ed = d.editor;
        let mut j = det_json(&d);
        text.push(String::new());
        text.push(format!(
            "{} ({}): {}",
            ed.display(),
            ed.name(),
            if d.found {
                format!(
                    "found at {} via {}{}{}",
                    d.exe.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                    d.method,
                    d.version.as_ref().map(|v| format!(", version {v}")).unwrap_or_default(),
                    if d.portable { ", portable" } else { "" }
                )
            } else {
                "not found".to_string()
            }
        ));
        if let Some(n) = &d.note {
            text.push(format!("  note: {n}"));
        }
        if let Some(c) = &d.cli {
            text.push(format!("  cli: {}", c.display()));
        }
        if ed == Editor::Jupyter {
            let (l, v) = jupyter_status(env, false);
            text.extend(l.into_iter().map(|x| format!("  {x}")));
            j["status"] = v;
            editors.push(j);
            continue;
        }
        let cli_list = if plugin_kind(ed) == Some(PluginKind::VsCode) { cli_extensions(env, &d, o) } else { None };
        let mut tj = Vec::new();
        for t in &d.targets {
            let st = plugin_state(ed, t);
            text.push(format!("  {} {}{}", t.kind, t.dir.display(), if t.exists { "" } else { "  (does not exist yet)" }));
            text.push(format!("    plugin: {}", describe_state(ed, &st)));
            for n in &st.notes {
                text.push(format!("    note: {n}"));
            }
            let mut entry = state_json(&st);
            entry["label"] = json!(t.label);
            entry["dir"] = json!(t.dir.to_string_lossy());
            if plugin_kind(ed) == Some(PluginKind::VsCode) {
                if let Some(reg) = st.in_extensions_json {
                    text.push(format!("    registered in extensions.json: {}", if reg { "yes" } else { "NO" }));
                }
                match &cli_list {
                    None => text.push(format!("    {} --list-extensions: not run", ed.cli_name())),
                    Some(Err(e)) => text.push(format!("    {} --list-extensions: failed ({e})", ed.cli_name())),
                    Some(Ok(list)) => {
                        let lv = listed_version(list, VSCODE_ID);
                        text.push(format!(
                            "    {} --list-extensions: {}",
                            ed.cli_name(),
                            match &lv {
                                Some(v) => format!("lists {VSCODE_ID}@{v}"),
                                None => format!("does NOT list {VSCODE_ID}"),
                            }
                        ));
                        entry["cli_lists_extension"] = json!(lv.is_some());
                        entry["cli_listed_version"] = json!(lv);
                    }
                }
                let (jin, jver) = match &cli_list {
                    Some(Ok(list)) => {
                        let v = listed_version(list, "ms-toolsai.jupyter");
                        (v.is_some(), v.filter(|s| !s.is_empty()))
                    }
                    _ => jupyter_ext_in_dir(t),
                };
                text.push(if jin {
                    format!("    Jupyter extension (ms-toolsai.jupyter): installed{}", jver.as_ref().map(|v| format!(" {v}")).unwrap_or_default())
                } else {
                    format!(
                        "    Jupyter extension (ms-toolsai.jupyter): NOT installed; notebooks need it. To install: {} --install-extension ms-toolsai.jupyter",
                        ed.cli_name()
                    )
                });
                entry["jupyter_extension"] = json!({"installed": jin, "version": jver, "install_command": format!("{} --install-extension ms-toolsai.jupyter", ed.cli_name())});
            }
            tj.push(entry);
        }
        j["plugin"] = json!(tj);
        editors.push(j);
    }
    if o.json {
        let v = json!({"qu": qjson, "platform": env.platform.name(), "editors": editors});
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        for l in text {
            println!("{l}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- check

fn mark(s: Sev) -> &'static str {
    match s {
        Sev::Ok => "ok  ",
        Sev::Warn => "WARN",
        Sev::Fail => "FAIL",
    }
}

fn installed_checks(env: &Env, o: &Opts) -> Vec<Check> {
    let mut out = Vec::new();
    for d in detect_all(env) {
        let ed = d.editor;
        if ed == Editor::Jupyter {
            match jup::kernel_health(env, !env.sandboxed && !o.no_cli) {
                None => out.push(Check { name: "jupyter kernel qu".into(), sev: Sev::Ok, detail: "not registered (nothing to check)".into() }),
                Some(h) => {
                    let name = "jupyter kernel qu".to_string();
                    if h.problems.is_empty() {
                        out.push(Check { name, sev: Sev::Ok, detail: format!("{}: {}", h.dir.display(), h.notes.join("; ")) });
                    } else {
                        out.push(Check { name, sev: Sev::Fail, detail: h.problems.join("; ") });
                    }
                }
            }
            continue;
        }
        let cli_list = if plugin_kind(ed) == Some(PluginKind::VsCode) { cli_extensions(env, &d, o) } else { None };
        for t in &d.targets {
            let st = plugin_state(ed, t);
            if !st.installed {
                continue;
            }
            let name = format!("{} plugin ({})", ed.name(), t.label);
            let ver = st.version.clone().unwrap_or_else(|| "?".into());
            if !st.up_to_date {
                out.push(Check {
                    name,
                    sev: Sev::Fail,
                    detail: format!("installed {ver}, this qu ships {}{}; run `qu editors install`", st.shipped, if st.missing.is_empty() { String::new() } else { format!(" (missing {})", st.missing.join(", ")) }),
                });
                continue;
            }
            if !st.modified.is_empty() {
                out.push(Check { name: name.clone(), sev: Sev::Warn, detail: format!("{ver}: modified since install: {}", st.modified.join(", ")) });
            } else {
                out.push(Check { name: name.clone(), sev: Sev::Ok, detail: format!("{ver}, files match the embedded copy") });
            }
            if plugin_kind(ed) == Some(PluginKind::VsCode) {
                if st.in_extensions_json == Some(false) {
                    out.push(Check { name: format!("{} extensions.json", ed.name()), sev: Sev::Warn, detail: "the folder is not registered in extensions.json; run `qu editors install`".into() });
                }
                match &cli_list {
                    None => out.push(Check { name: format!("{} activation", ed.name()), sev: Sev::Warn, detail: "CLI not run, so not verified that the editor loads the extension".into() }),
                    Some(Err(e)) => out.push(Check { name: format!("{} activation", ed.name()), sev: Sev::Warn, detail: format!("{} --list-extensions failed: {e}", ed.cli_name()) }),
                    Some(Ok(list)) => match listed_version(list, VSCODE_ID) {
                        Some(v) => out.push(Check { name: format!("{} activation", ed.name()), sev: Sev::Ok, detail: format!("{} lists {VSCODE_ID}@{v}", ed.cli_name()) }),
                        None => out.push(Check { name: format!("{} activation", ed.name()), sev: Sev::Fail, detail: format!("{} --list-extensions does not list {VSCODE_ID}: the editor does not see the extension", ed.cli_name()) }),
                    },
                }
                let (jin, _) = match &cli_list {
                    Some(Ok(list)) => (listed_version(list, "ms-toolsai.jupyter").is_some(), None),
                    _ => jupyter_ext_in_dir(t),
                };
                if !jin {
                    out.push(Check { name: format!("{} Jupyter extension", ed.name()), sev: Sev::Warn, detail: format!("ms-toolsai.jupyter is not installed (notebooks need it): {} --install-extension ms-toolsai.jupyter", ed.cli_name()) });
                }
            }
        }
    }
    out
}

fn cmd_check(env: &Env, o: &Opts) -> Result<(), String> {
    let mut checks = embedded_checks();
    let on_path = qu_on_path(env);
    checks.push(Check {
        name: "qu on PATH".into(),
        sev: if on_path.is_some() { Sev::Ok } else { Sev::Warn },
        detail: match &on_path {
            Some(p) => p.display().to_string(),
            None => "not on PATH: set qu.executablePath in the editor, or add qu's folder to PATH".into(),
        },
    });
    checks.extend(installed_checks(env, o));
    let fails = checks.iter().filter(|c| c.sev == Sev::Fail).count();
    let warns = checks.iter().filter(|c| c.sev == Sev::Warn).count();
    if o.json {
        let v = json!({
            "ok": fails == 0,
            "failures": fails,
            "warnings": warns,
            "checks": checks.iter().map(|c| json!({"name": c.name, "result": match c.sev { Sev::Ok => "ok", Sev::Warn => "warn", Sev::Fail => "fail" }, "detail": c.detail})).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        for c in &checks {
            println!("[{}] {}: {}", mark(c.sev), c.name, c.detail);
        }
        println!("{} checks, {fails} failed, {warns} warning(s)", checks.len());
    }
    if fails > 0 {
        exit_with(1);
    }
    Ok(())
}

