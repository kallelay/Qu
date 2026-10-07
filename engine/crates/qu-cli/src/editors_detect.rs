//! Where are the editors, and where would their plugin go? The single source
//! of truth the installers call (`qu editors detect`).
//!
//! Detection goes beyond default folders: on Windows the registry is asked
//! first (`App Paths`, then the `Uninstall` keys, both registry views, via
//! `reg.exe` -- no extra crate), then `PATH`, then the known folders; on
//! macOS the `.app` bundles in `/Applications` and `~/Applications`; on Linux
//! `PATH`, the usual `/usr`, `/opt`, `/snap` locations and Flatpak data dirs.
//! Every result records WHICH method found it.
//!
//! The plugin target directory comes from the editor's real data location:
//! a portable VS Code (`data/` beside the executable), a portable Notepad++
//! (`doLocalConf.xml` beside the executable), a portable Sublime Text
//! (`Data/`), Flatpak sandboxes, `VSCODE_EXTENSIONS`.
//!
//! Everything environment-dependent lives in [`Env`], so tests build a fake
//! root and never touch the real profile, `PATH` or registry.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
    pub fn parse(s: &str) -> Option<Platform> {
        match s.to_ascii_lowercase().as_str() {
            "windows" | "win" | "win32" => Some(Platform::Windows),
            "macos" | "mac" | "darwin" => Some(Platform::MacOs),
            "linux" => Some(Platform::Linux),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::MacOs => "macos",
            Platform::Linux => "linux",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Editor {
    VsCode,
    VsCodium,
    Cursor,
    Windsurf,
    NotepadPp,
    Sublime,
    Vim,
    Nvim,
    Jupyter,
}

impl Editor {
    pub const ALL: [Editor; 9] = [
        Editor::VsCode,
        Editor::VsCodium,
        Editor::Cursor,
        Editor::Windsurf,
        Editor::NotepadPp,
        Editor::Sublime,
        Editor::Vim,
        Editor::Nvim,
        Editor::Jupyter,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Editor::VsCode => "vscode",
            Editor::VsCodium => "vscodium",
            Editor::Cursor => "cursor",
            Editor::Windsurf => "windsurf",
            Editor::NotepadPp => "notepadpp",
            Editor::Sublime => "sublime",
            Editor::Vim => "vim",
            Editor::Nvim => "nvim",
            Editor::Jupyter => "jupyter",
        }
    }
    pub fn display(self) -> &'static str {
        match self {
            Editor::VsCode => "VS Code",
            Editor::VsCodium => "VSCodium",
            Editor::Cursor => "Cursor",
            Editor::Windsurf => "Windsurf",
            Editor::NotepadPp => "Notepad++",
            Editor::Sublime => "Sublime Text",
            Editor::Vim => "Vim",
            Editor::Nvim => "Neovim",
            Editor::Jupyter => "Jupyter",
        }
    }
    pub fn from_name(s: &str) -> Option<Editor> {
        let s = s.to_ascii_lowercase();
        Editor::ALL.iter().copied().find(|e| e.name() == s).or(match s.as_str() {
            "code" => Some(Editor::VsCode),
            "codium" => Some(Editor::VsCodium),
            "notepad++" | "npp" => Some(Editor::NotepadPp),
            "subl" | "sublimetext" => Some(Editor::Sublime),
            "neovim" => Some(Editor::Nvim),
            _ => None,
        })
    }
    pub fn is_vscode_family(self) -> bool {
        matches!(self, Editor::VsCode | Editor::VsCodium | Editor::Cursor | Editor::Windsurf)
    }
    /// The command name of the editor's CLI (`code --list-extensions`).
    pub fn cli_name(self) -> &'static str {
        match self {
            Editor::VsCode => "code",
            Editor::VsCodium => "codium",
            Editor::Cursor => "cursor",
            Editor::Windsurf => "windsurf",
            Editor::NotepadPp => "notepad++",
            Editor::Sublime => "subl",
            Editor::Vim => "vim",
            Editor::Nvim => "nvim",
            Editor::Jupyter => "jupyter",
        }
    }
    /// Name of the dot-folder under the home directory that holds
    /// `extensions/` (VS Code family only).
    fn dot_dir(self) -> &'static str {
        match self {
            Editor::VsCode => ".vscode",
            Editor::VsCodium => ".vscode-oss",
            Editor::Cursor => ".cursor",
            Editor::Windsurf => ".windsurf",
            _ => "",
        }
    }
}

// ---------------------------------------------------------------- registry

#[derive(Clone, Debug, Default)]
pub struct UninstallEntry {
    pub display_name: String,
    pub install_location: Option<String>,
    pub display_icon: Option<String>,
    pub display_version: Option<String>,
}

/// The Windows registry, as little of it as detection needs. A trait so tests
/// stub it and never read the real registry.
pub trait Registry {
    /// `App Paths\<exe_name>` default values (HKCU and HKLM, both views).
    fn app_path(&self, exe_name: &str) -> Vec<PathBuf>;
    /// Entries under the `...\Uninstall\*` keys (HKCU, HKLM 64- and 32-bit).
    fn uninstall_entries(&self) -> Vec<UninstallEntry>;
}

pub struct NullRegistry;
impl Registry for NullRegistry {
    fn app_path(&self, _: &str) -> Vec<PathBuf> {
        Vec::new()
    }
    fn uninstall_entries(&self) -> Vec<UninstallEntry> {
        Vec::new()
    }
}

/// Real implementation: shells out to `reg.exe query`.
pub struct RegExe {
    cache: RefCell<Option<Vec<UninstallEntry>>>,
}

impl RegExe {
    pub fn new() -> RegExe {
        RegExe { cache: RefCell::new(None) }
    }
}

fn reg_query(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("reg")
        .arg("query")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The data of the `(Default)` value in `reg query <key> /ve` output.
pub fn parse_reg_default(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        if let Some((_, rest)) = t.split_once("    REG_") {
            let value = rest.split_once("    ").map(|(_, v)| v).unwrap_or("");
            let v = value.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Parse `reg query <Uninstall key> /s` output into entries.
pub fn parse_reg_uninstall(text: &str) -> Vec<UninstallEntry> {
    let mut out = Vec::new();
    let mut cur: Option<UninstallEntry> = None;
    for line in text.lines() {
        if line.starts_with("HKEY_") {
            if let Some(e) = cur.take() {
                if !e.display_name.is_empty() {
                    out.push(e);
                }
            }
            cur = Some(UninstallEntry::default());
            continue;
        }
        let t = line.trim_start();
        let Some((name, rest)) = t.split_once("    REG_") else { continue };
        let value = rest.split_once("    ").map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        if let Some(e) = cur.as_mut() {
            match name.trim() {
                "DisplayName" => e.display_name = value,
                "InstallLocation" if !value.is_empty() => e.install_location = Some(value),
                "DisplayIcon" if !value.is_empty() => e.display_icon = Some(value),
                "DisplayVersion" if !value.is_empty() => e.display_version = Some(value),
                _ => {}
            }
        }
    }
    if let Some(e) = cur.take() {
        if !e.display_name.is_empty() {
            out.push(e);
        }
    }
    out
}

impl Registry for RegExe {
    fn app_path(&self, exe_name: &str) -> Vec<PathBuf> {
        let tail = format!("\\Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe_name}");
        let mut found = Vec::new();
        for (hive, view) in [("HKCU", None), ("HKLM", Some("/reg:64")), ("HKLM", Some("/reg:32"))] {
            let key = format!("{hive}{tail}");
            let mut args = vec![key.as_str(), "/ve"];
            if let Some(v) = view {
                args.push(v);
            }
            if let Some(text) = reg_query(&args) {
                if let Some(p) = parse_reg_default(&text) {
                    let p = PathBuf::from(p.trim_matches('"'));
                    if !found.contains(&p) {
                        found.push(p);
                    }
                }
            }
        }
        found
    }

    fn uninstall_entries(&self) -> Vec<UninstallEntry> {
        if let Some(c) = self.cache.borrow().as_ref() {
            return c.clone();
        }
        let tail = "\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
        let mut all = Vec::new();
        for (hive, view) in [("HKCU", None), ("HKLM", Some("/reg:64")), ("HKLM", Some("/reg:32"))] {
            let key = format!("{hive}{tail}");
            let mut args = vec![key.as_str(), "/s"];
            if let Some(v) = view {
                args.push(v);
            }
            if let Some(text) = reg_query(&args) {
                all.extend(parse_reg_uninstall(&text));
            }
        }
        *self.cache.borrow_mut() = Some(all.clone());
        all
    }
}

// ---------------------------------------------------------------- env

pub struct Env {
    pub platform: Platform,
    pub home: PathBuf,
    pub appdata: PathBuf,
    pub localappdata: PathBuf,
    pub programdata: PathBuf,
    pub xdg_config: PathBuf,
    pub xdg_data: PathBuf,
    pub program_files: Vec<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    /// What `/` is: the real root, or `<root>/fs` in a sandbox.
    pub fs_root: PathBuf,
    /// `--root` / `QU_EDITORS_ROOT`: nothing outside the root is probed
    /// (no registry, no real PATH, no running of editor CLIs).
    pub sandboxed: bool,
    pub registry: Box<dyn Registry>,
    /// `VSCODE_EXTENSIONS`, honoured for VS Code only.
    pub vscode_extensions_env: Option<PathBuf>,
    /// `JUPYTER_DATA_DIR`.
    pub jupyter_data_dir: Option<PathBuf>,
    /// `JUPYTER_PATH` entries (read-only search path for kernelspecs).
    pub jupyter_path: Vec<PathBuf>,
    /// Directory holding the running `qu` (looked at for `qu-jupyter`).
    pub qu_exe: Option<PathBuf>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

impl Env {
    /// The real machine.
    pub fn real() -> Env {
        let platform = std::env::var("QU_EDITORS_PLATFORM").ok().and_then(|s| Platform::parse(&s)).unwrap_or_else(Platform::current);
        let home = env_path("USERPROFILE")
            .filter(|_| platform == Platform::Windows)
            .or_else(|| env_path("HOME"))
            .or_else(|| env_path("USERPROFILE"))
            .unwrap_or_else(|| PathBuf::from("."));
        let appdata = env_path("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let localappdata = env_path("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
        let programdata = env_path("PROGRAMDATA").unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        let xdg_config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let xdg_data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
        let mut program_files = Vec::new();
        for v in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(p) = env_path(v) {
                if !program_files.contains(&p) {
                    program_files.push(p);
                }
            }
        }
        let path_dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        let registry: Box<dyn Registry> =
            if platform == Platform::Windows && cfg!(windows) { Box::new(RegExe::new()) } else { Box::new(NullRegistry) };
        Env {
            platform,
            home,
            appdata,
            localappdata,
            programdata,
            xdg_config,
            xdg_data,
            program_files,
            path_dirs,
            fs_root: PathBuf::from("/"),
            sandboxed: false,
            registry,
            vscode_extensions_env: env_path("VSCODE_EXTENSIONS"),
            jupyter_data_dir: env_path("JUPYTER_DATA_DIR"),
            jupyter_path: std::env::var_os("JUPYTER_PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            qu_exe: std::env::current_exe().ok(),
        }
    }

    /// A fake machine under `root`: every location, including `PATH`, is
    /// inside it. `QU_EDITORS_PATH` (a PATH-style list) adds `PATH` entries.
    pub fn sandbox(root: &Path, platform: Platform) -> Env {
        let home = root.join("home");
        let path_dirs = std::env::var_os("QU_EDITORS_PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        Env {
            platform,
            appdata: root.join("AppData").join("Roaming"),
            localappdata: root.join("AppData").join("Local"),
            programdata: root.join("ProgramData"),
            xdg_config: home.join(".config"),
            xdg_data: home.join(".local").join("share"),
            program_files: vec![root.join("Program Files"), root.join("Program Files (x86)")],
            path_dirs,
            fs_root: root.join("fs"),
            sandboxed: true,
            registry: Box::new(NullRegistry),
            vscode_extensions_env: None,
            jupyter_data_dir: None,
            jupyter_path: Vec::new(),
            qu_exe: std::env::current_exe().ok(),
            home,
        }
    }

    /// `--root`, else `QU_EDITORS_ROOT`, else the real machine.
    pub fn from_args(root: Option<&str>) -> Env {
        let platform = std::env::var("QU_EDITORS_PLATFORM").ok().and_then(|s| Platform::parse(&s)).unwrap_or_else(Platform::current);
        let root = root.map(PathBuf::from).or_else(|| env_path("QU_EDITORS_ROOT"));
        match root {
            Some(r) => Env::sandbox(&r, platform),
            None => Env::real(),
        }
    }

    /// Jupyter's per-user data dir (`<this>/kernels/<name>/kernel.json`).
    pub fn jupyter_user_dir(&self) -> PathBuf {
        if let Some(d) = &self.jupyter_data_dir {
            return d.clone();
        }
        match self.platform {
            Platform::Windows => self.appdata.join("jupyter"),
            Platform::MacOs => self.home.join("Library").join("Jupyter"),
            Platform::Linux => self.xdg_data.join("jupyter"),
        }
    }

    /// Every directory that may hold `kernels/` (user dir first).
    pub fn jupyter_kernel_roots(&self) -> Vec<PathBuf> {
        let mut v = vec![self.jupyter_user_dir()];
        v.extend(self.jupyter_path.iter().cloned());
        match self.platform {
            Platform::Windows => v.push(self.programdata.join("jupyter")),
            Platform::MacOs => {
                v.push(self.fs_root.join("usr/local/share/jupyter"));
                v.push(self.fs_root.join("Library/Jupyter"));
            }
            Platform::Linux => {
                v.push(self.fs_root.join("usr/local/share/jupyter"));
                v.push(self.fs_root.join("usr/share/jupyter"));
            }
        }
        v
    }
}

// ---------------------------------------------------------------- results

#[derive(Clone, Debug)]
pub struct Target {
    /// Human label ("Sublime Text 4", or the editor name).
    pub label: String,
    /// `ext-dir`, `udl-dir`, `packages-dir`, `pack-dir`, `kernels-dir`.
    pub kind: &'static str,
    pub dir: PathBuf,
    pub exists: bool,
}

#[derive(Clone, Debug)]
pub struct Detection {
    pub editor: Editor,
    pub found: bool,
    pub exe: Option<PathBuf>,
    pub cli: Option<PathBuf>,
    pub method: String,
    pub version: Option<String>,
    pub portable: bool,
    pub targets: Vec<Target>,
    pub note: Option<String>,
}

// ---------------------------------------------------------------- specs

struct Spec {
    win_exes: &'static [&'static str],
    uninstall_prefix: &'static [&'static str],
    path_cmds: &'static [&'static str],
    win_dirs: &'static [&'static str],
    mac_apps: &'static [&'static str],
    linux_paths: &'static [&'static str],
    flatpak: Option<&'static str>,
}

fn spec(ed: Editor) -> Spec {
    match ed {
        Editor::VsCode => Spec {
            win_exes: &["Code.exe"],
            uninstall_prefix: &["microsoft visual studio code"],
            path_cmds: &["code"],
            win_dirs: &["Microsoft VS Code"],
            mac_apps: &["Visual Studio Code.app"],
            linux_paths: &["usr/bin/code", "usr/share/code/code", "opt/visual-studio-code/code", "snap/bin/code"],
            flatpak: Some("com.visualstudio.code"),
        },
        Editor::VsCodium => Spec {
            win_exes: &["VSCodium.exe", "codium.exe"],
            uninstall_prefix: &["vscodium"],
            path_cmds: &["codium", "vscodium"],
            win_dirs: &["VSCodium"],
            mac_apps: &["VSCodium.app"],
            linux_paths: &["usr/bin/codium", "usr/share/codium/codium", "opt/vscodium-bin/codium", "snap/bin/codium"],
            flatpak: Some("com.vscodium.codium"),
        },
        Editor::Cursor => Spec {
            win_exes: &["Cursor.exe", "cursor.exe"],
            uninstall_prefix: &["cursor"],
            path_cmds: &["cursor"],
            win_dirs: &["cursor", "Cursor"],
            mac_apps: &["Cursor.app"],
            linux_paths: &["usr/bin/cursor", "opt/Cursor/cursor", "opt/cursor/cursor"],
            flatpak: None,
        },
        Editor::Windsurf => Spec {
            win_exes: &["Windsurf.exe", "windsurf.exe"],
            uninstall_prefix: &["windsurf"],
            path_cmds: &["windsurf"],
            win_dirs: &["Windsurf"],
            mac_apps: &["Windsurf.app"],
            linux_paths: &["usr/bin/windsurf", "opt/Windsurf/windsurf", "usr/share/windsurf/windsurf"],
            flatpak: None,
        },
        Editor::NotepadPp => Spec {
            win_exes: &["notepad++.exe"],
            uninstall_prefix: &["notepad++"],
            path_cmds: &["notepad++"],
            win_dirs: &["Notepad++"],
            mac_apps: &[],
            linux_paths: &[],
            flatpak: None,
        },
        Editor::Sublime => Spec {
            win_exes: &["sublime_text.exe"],
            uninstall_prefix: &["sublime text"],
            path_cmds: &["subl", "sublime_text"],
            win_dirs: &["Sublime Text", "Sublime Text 3", "Sublime Text 4"],
            mac_apps: &["Sublime Text.app"],
            linux_paths: &["opt/sublime_text/sublime_text", "usr/bin/subl", "usr/bin/sublime_text", "snap/bin/subl"],
            flatpak: Some("com.sublimetext.three"),
        },
        Editor::Vim => Spec {
            win_exes: &["gvim.exe", "vim.exe"],
            uninstall_prefix: &["vim "],
            path_cmds: &["vim", "gvim"],
            win_dirs: &["Vim"],
            mac_apps: &["MacVim.app"],
            linux_paths: &["usr/bin/vim", "usr/bin/gvim", "usr/local/bin/vim"],
            flatpak: None,
        },
        Editor::Nvim => Spec {
            win_exes: &["nvim.exe", "nvim-qt.exe"],
            uninstall_prefix: &["neovim"],
            path_cmds: &["nvim"],
            win_dirs: &["Neovim"],
            mac_apps: &[],
            linux_paths: &["usr/bin/nvim", "usr/local/bin/nvim", "snap/bin/nvim"],
            flatpak: None,
        },
        Editor::Jupyter => Spec {
            win_exes: &["jupyter.exe"],
            uninstall_prefix: &[],
            path_cmds: &["jupyter", "jupyter-lab", "jupyter-notebook"],
            win_dirs: &[],
            mac_apps: &[],
            linux_paths: &["usr/bin/jupyter", "usr/local/bin/jupyter"],
            flatpak: None,
        },
    }
}

// ---------------------------------------------------------------- helpers

fn is_file(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
}
fn is_dir(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// First existing `dir/<exe>`, `dir/bin/<exe>`, or `dir/vim*/<exe>` (Vim keeps
/// its executable in a versioned subfolder).
fn exe_in_dir(dir: &Path, exes: &[&str]) -> Option<PathBuf> {
    for e in exes {
        for rel in ["", "bin"] {
            let p = if rel.is_empty() { dir.join(e) } else { dir.join(rel).join(e) };
            if is_file(&p) {
                return Some(p);
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .filter(|d| d.file_name().to_string_lossy().to_ascii_lowercase().starts_with("vim"))
            .map(|d| d.path())
            .collect();
        subs.sort();
        subs.reverse();
        for s in subs {
            for e in exes {
                let p = s.join(e);
                if is_file(&p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn path_exts(env: &Env) -> Vec<String> {
    if env.platform == Platform::Windows {
        vec![".exe".into(), ".cmd".into(), ".bat".into(), ".com".into()]
    } else {
        vec![String::new()]
    }
}

/// `which`: the first `cmd` on `env.path_dirs`.
pub fn which(env: &Env, cmd: &str) -> Option<PathBuf> {
    let exts = path_exts(env);
    for dir in &env.path_dirs {
        for e in &exts {
            let p = dir.join(format!("{cmd}{e}"));
            if is_file(&p) {
                return Some(p);
            }
        }
    }
    None
}

fn ancestors_up(p: &Path, n: usize) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut cur = p.parent().map(Path::to_path_buf);
    while let Some(c) = cur {
        if v.len() >= n {
            break;
        }
        cur = c.parent().map(Path::to_path_buf);
        v.push(c);
    }
    v
}

/// The VS Code-family application root for a launcher or exe: the nearest
/// ancestor (up to 4 up) holding one of the Windows exe names, else the exe's
/// own folder.
fn vscode_app_root(env: &Env, ed: Editor, p: &Path) -> PathBuf {
    if p.extension().map(|e| e == "app").unwrap_or(false) {
        return p.to_path_buf();
    }
    if env.platform == Platform::Windows {
        for a in ancestors_up(p, 5) {
            for e in spec(ed).win_exes {
                if is_file(&a.join(e)) {
                    return a;
                }
            }
        }
    }
    p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf())
}

/// A folder whose `data/` makes it a portable install, within 5 ancestors.
fn portable_root(p: &Path) -> Option<PathBuf> {
    let mut cands = vec![p.to_path_buf()];
    cands.extend(ancestors_up(p, 5));
    for a in cands {
        let d = a.join("data");
        if is_dir(&d) && (is_dir(&d.join("extensions")) || is_dir(&d.join("user-data")) || is_dir(&d.join("tmp"))) {
            return Some(a);
        }
    }
    None
}

fn read_version(p: &Path) -> Option<String> {
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("version")?.as_str().map(String::from)
}

fn vscode_version_near(env: &Env, ed: Editor, root: &Path, exe: &Path) -> Option<String> {
    let _ = ed;
    if env.platform == Platform::MacOs {
        let mut a = exe.to_path_buf();
        loop {
            if a.extension().map(|e| e == "app").unwrap_or(false) {
                return read_version(&a.join("Contents/Resources/app/package.json"));
            }
            if !a.pop() {
                break;
            }
        }
    }
    if let Some(v) = read_version(&root.join("resources/app/package.json")) {
        return Some(v);
    }
    // newer installs: <root>/<commit>/resources/app/package.json
    if let Ok(rd) = std::fs::read_dir(root) {
        for d in rd.flatten() {
            if let Some(v) = read_version(&d.path().join("resources/app/package.json")) {
                return Some(v);
            }
        }
    }
    None
}

fn find_cli(env: &Env, ed: Editor, root: &Path, launcher: Option<&Path>) -> Option<PathBuf> {
    if let Some(l) = launcher {
        return Some(l.to_path_buf());
    }
    let n = ed.cli_name();
    let cands: Vec<PathBuf> = if env.platform == Platform::Windows {
        vec![
            root.join("bin").join(format!("{n}.cmd")),
            root.join("resources/app/bin").join(format!("{n}.cmd")),
            root.join(format!("{n}.cmd")),
        ]
    } else if env.platform == Platform::MacOs {
        let mut v = vec![root.join("bin").join(n)];
        let mut a = root.to_path_buf();
        loop {
            if a.extension().map(|e| e == "app").unwrap_or(false) {
                v.push(a.join("Contents/Resources/app/bin").join(n));
                break;
            }
            if !a.pop() {
                break;
            }
        }
        v
    } else {
        vec![root.join("bin").join(n), root.join(n)]
    };
    cands.into_iter().find(|p| is_file(p))
}

// ---------------------------------------------------------------- detection

struct Found {
    exe: PathBuf,
    launcher: Option<PathBuf>,
    method: String,
    version: Option<String>,
    flatpak: bool,
}

fn find_exe(env: &Env, ed: Editor) -> Option<Found> {
    let sp = spec(ed);

    if ed == Editor::NotepadPp && env.platform != Platform::Windows {
        return None;
    }

    // 1. registry (Windows)
    if env.platform == Platform::Windows {
        for name in sp.win_exes {
            for p in env.registry.app_path(name) {
                if is_file(&p) {
                    return Some(Found { exe: p, launcher: None, method: "registry:app-paths".into(), version: None, flatpak: false });
                }
            }
        }
        if !sp.uninstall_prefix.is_empty() {
            for ent in env.registry.uninstall_entries() {
                let dn = ent.display_name.to_ascii_lowercase();
                if !sp.uninstall_prefix.iter().any(|p| dn.starts_with(p)) {
                    continue;
                }
                let mut exe = None;
                if let Some(loc) = &ent.install_location {
                    exe = exe_in_dir(Path::new(loc.trim_matches('"')), sp.win_exes);
                }
                if exe.is_none() {
                    if let Some(icon) = &ent.display_icon {
                        let icon = icon.split(',').next().unwrap_or("").trim().trim_matches('"');
                        let p = PathBuf::from(icon);
                        if is_file(&p) && p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                            exe = Some(p);
                        }
                    }
                }
                if let Some(exe) = exe {
                    return Some(Found {
                        exe,
                        launcher: None,
                        method: "registry:uninstall".into(),
                        version: ent.display_version.clone(),
                        flatpak: false,
                    });
                }
            }
        }
    }

    // 2. PATH
    for cmd in sp.path_cmds {
        if let Some(p) = which(env, cmd) {
            return Some(Found { exe: p.clone(), launcher: Some(p), method: "PATH".into(), version: None, flatpak: false });
        }
    }

    // 3. known folders
    match env.platform {
        Platform::Windows => {
            let mut bases: Vec<PathBuf> = env.program_files.clone();
            bases.push(env.localappdata.join("Programs"));
            for b in &bases {
                for d in sp.win_dirs {
                    if let Some(p) = exe_in_dir(&b.join(d), sp.win_exes) {
                        return Some(Found { exe: p, launcher: None, method: "known-folder".into(), version: None, flatpak: false });
                    }
                }
            }
        }
        Platform::MacOs => {
            for base in [env.fs_root.join("Applications"), env.home.join("Applications")] {
                for app in sp.mac_apps {
                    let p = base.join(app);
                    if is_dir(&p) {
                        return Some(Found { exe: p, launcher: None, method: "applications".into(), version: None, flatpak: false });
                    }
                }
            }
        }
        Platform::Linux => {
            for rel in sp.linux_paths {
                let p = env.fs_root.join(rel);
                if is_file(&p) {
                    return Some(Found { exe: p, launcher: None, method: "known-path".into(), version: None, flatpak: false });
                }
            }
            // 4. flatpak
            if let Some(id) = sp.flatpak {
                let d = env.home.join(".var").join("app").join(id);
                if is_dir(&d) {
                    return Some(Found { exe: d, launcher: None, method: "flatpak".into(), version: None, flatpak: true });
                }
                let sys = env.fs_root.join("var/lib/flatpak/app").join(id);
                if is_dir(&sys) {
                    return Some(Found { exe: sys, launcher: None, method: "flatpak".into(), version: None, flatpak: true });
                }
            }
        }
    }
    None
}

fn target(label: &str, kind: &'static str, dir: PathBuf) -> Target {
    let exists = is_dir(&dir);
    Target { label: label.to_string(), kind, dir, exists }
}

fn vscode_family_targets(env: &Env, ed: Editor, exe: Option<&Path>, flatpak: bool) -> (Vec<Target>, bool) {
    if let Some(e) = exe {
        if !flatpak {
            if let Some(root) = portable_root(e) {
                return (vec![target(ed.display(), "ext-dir", root.join("data").join("extensions"))], true);
            }
        }
    }
    if ed == Editor::VsCode {
        if let Some(d) = &env.vscode_extensions_env {
            return (vec![target(ed.display(), "ext-dir", d.clone())], false);
        }
    }
    if flatpak {
        let sub = if ed == Editor::VsCode { "vscode" } else { "codium" };
        let id = spec(ed).flatpak.unwrap_or("");
        return (
            vec![target(ed.display(), "ext-dir", env.home.join(".var/app").join(id).join("data").join(sub).join("extensions"))],
            false,
        );
    }
    (vec![target(ed.display(), "ext-dir", env.home.join(ed.dot_dir()).join("extensions"))], false)
}

fn sublime_targets(env: &Env, exe: Option<&Path>, flatpak: bool) -> (Vec<Target>, bool) {
    if let Some(e) = exe {
        if !flatpak {
            if let Some(dir) = e.parent() {
                let data = dir.join("Data");
                if is_dir(&data) {
                    return (vec![target("Sublime Text (portable)", "packages-dir", data.join("Packages").join("User"))], true);
                }
            }
        }
    }
    let bases: Vec<(&str, PathBuf)> = if flatpak {
        vec![("Sublime Text", env.home.join(".var/app/com.sublimetext.three/config/sublime-text"))]
    } else {
        match env.platform {
            Platform::Windows => vec![
                ("Sublime Text 4", env.appdata.join("Sublime Text")),
                ("Sublime Text 3", env.appdata.join("Sublime Text 3")),
            ],
            Platform::MacOs => vec![
                ("Sublime Text 4", env.home.join("Library/Application Support/Sublime Text")),
                ("Sublime Text 3", env.home.join("Library/Application Support/Sublime Text 3")),
            ],
            Platform::Linux => vec![
                ("Sublime Text 4", env.xdg_config.join("sublime-text")),
                ("Sublime Text 3", env.xdg_config.join("sublime-text-3")),
            ],
        }
    };
    let mut out = Vec::new();
    for (label, b) in &bases {
        if is_dir(&b.join("Packages")) {
            out.push(target(label, "packages-dir", b.join("Packages").join("User")));
        }
    }
    if out.is_empty() {
        let (label, b) = &bases[0];
        out.push(target(label, "packages-dir", b.join("Packages").join("User")));
    }
    (out, false)
}

fn vim_targets(env: &Env, ed: Editor) -> Vec<Target> {
    let dir = match (ed, env.platform) {
        (Editor::Vim, Platform::Windows) => env.home.join("vimfiles"),
        (Editor::Vim, _) => env.home.join(".vim"),
        (_, Platform::Windows) => env.localappdata.join("nvim"),
        (_, _) => env.xdg_config.join("nvim"),
    };
    vec![target(ed.display(), "pack-dir", dir)]
}

fn notepadpp_targets(env: &Env, exe: Option<&Path>) -> (Vec<Target>, bool) {
    if env.platform != Platform::Windows {
        return (Vec::new(), false);
    }
    if let Some(e) = exe {
        if let Some(dir) = e.parent() {
            if is_file(&dir.join("doLocalConf.xml")) {
                return (vec![target("Notepad++ (portable)", "udl-dir", dir.join("userDefineLangs"))], true);
            }
        }
    }
    (vec![target("Notepad++", "udl-dir", env.appdata.join("Notepad++").join("userDefineLangs"))], false)
}

pub fn jupyter_targets(env: &Env) -> Vec<Target> {
    vec![target("Jupyter", "kernels-dir", env.jupyter_user_dir().join("kernels"))]
}

/// Detect one editor.
pub fn detect(env: &Env, ed: Editor) -> Detection {
    let mut found = find_exe(env, ed);

    // Jupyter: a kernels dir that already exists is also "found" (the
    // installer wrote there, or the user did), and `python -m jupyter` is the
    // last resort. The last-resort probes run a process, so never in a sandbox.
    let mut note = None;
    if ed == Editor::Jupyter && found.is_none() {
        for r in env.jupyter_kernel_roots() {
            if is_dir(&r.join("kernels")) {
                found = Some(Found {
                    exe: r.join("kernels"),
                    launcher: None,
                    method: "kernelspec-dir".into(),
                    version: None,
                    flatpak: false,
                });
                break;
            }
        }
        if found.is_none() && !env.sandboxed {
            if let Some((cmd, how)) = crate::editors_jupyter::python_m_jupyter(env) {
                found = Some(Found { exe: cmd, launcher: None, method: how, version: None, flatpak: false });
            }
        }
    }
    if ed == Editor::NotepadPp && env.platform != Platform::Windows {
        note = Some("Notepad++ is Windows-only".to_string());
    }

    let exe_for_layout: Option<PathBuf> = found.as_ref().map(|f| f.exe.clone());
    let flatpak = found.as_ref().map(|f| f.flatpak).unwrap_or(false);

    let (targets, portable) = match ed {
        e if e.is_vscode_family() => vscode_family_targets(env, e, exe_for_layout.as_deref(), flatpak),
        Editor::NotepadPp => notepadpp_targets(env, exe_for_layout.as_deref()),
        Editor::Sublime => sublime_targets(env, exe_for_layout.as_deref(), flatpak),
        Editor::Vim | Editor::Nvim => (vim_targets(env, ed), false),
        _ => (jupyter_targets(env), false),
    };

    let mut det = Detection {
        editor: ed,
        found: found.is_some(),
        exe: None,
        cli: None,
        method: String::new(),
        version: None,
        portable,
        targets,
        note,
    };
    if let Some(f) = found {
        det.method = f.method.clone();
        det.version = f.version.clone();
        if ed.is_vscode_family() && !f.flatpak {
            let root = vscode_app_root(env, ed, &f.exe);
            let exe = if env.platform == Platform::Windows {
                spec(ed).win_exes.iter().map(|e| root.join(e)).find(|p| is_file(p)).unwrap_or_else(|| f.exe.clone())
            } else {
                f.exe.clone()
            };
            det.cli = find_cli(env, ed, &root, f.launcher.as_deref());
            if det.version.is_none() {
                det.version = vscode_version_near(env, ed, &root, &exe);
            }
            det.exe = Some(exe);
        } else {
            det.cli = f.launcher.clone();
            det.exe = Some(f.exe);
        }
    }
    det
}

pub fn detect_all(env: &Env) -> Vec<Detection> {
    Editor::ALL.iter().map(|e| detect(env, *e)).collect()
}

/// Is a `qu`/`qu.exe` on `PATH`? (Used by status.)
pub fn qu_on_path(env: &Env) -> Option<PathBuf> {
    which(env, "qu")
}
