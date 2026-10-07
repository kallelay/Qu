//! Install / uninstall / inspect the embedded plugin files at a [`Target`].
//!
//! Safety rules, all enforced here and tested against temp directories:
//! * every write goes through [`write_file`]; with `dry` nothing is touched;
//! * a file that exists, differs from what we would write, and is not exactly
//!   what we wrote last time (manifest hash) is copied to `<name>.bak` first;
//! * a `.qu-editors-manifest.json` in each install root records the hash of
//!   every file we wrote, and uninstall removes only files that still match;
//! * VS Code's `extensions.json` is only edited if it already exists, and is
//!   backed up once to `extensions.json.bak`.

use crate::editors_assets::{files_for, fnv64_hex, marker_version, shipped_version, PluginKind, VSCODE_ID};
use crate::editors_detect::{Editor, Target};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = ".qu-editors-manifest.json";

pub fn plugin_kind(ed: Editor) -> Option<PluginKind> {
    match ed {
        e if e.is_vscode_family() => Some(PluginKind::VsCode),
        Editor::NotepadPp => Some(PluginKind::NotepadPp),
        Editor::Sublime => Some(PluginKind::Sublime),
        Editor::Vim | Editor::Nvim => Some(PluginKind::Vim),
        _ => None,
    }
}

pub fn vscode_folder_name(version: &str) -> String {
    format!("{VSCODE_ID}-{version}")
}

pub fn install_root(kind: PluginKind, t: &Target) -> PathBuf {
    match kind {
        PluginKind::VsCode => t.dir.join(vscode_folder_name(&shipped_version(kind))),
        PluginKind::NotepadPp => t.dir.clone(),
        PluginKind::Sublime => t.dir.join("Qu"),
        PluginKind::Vim => t.dir.join("pack").join("qu").join("start").join("qu"),
    }
}

fn rel_path(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, s| p.join(s))
}

fn read_manifest(root: &Path) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(root.join(MANIFEST)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let files = v.get("files")?.as_object()?;
    Some(files.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
}

fn backup_name(path: &Path) -> PathBuf {
    let mut n = path.as_os_str().to_os_string();
    n.push(".bak");
    let first = PathBuf::from(&n);
    if !first.exists() {
        return first;
    }
    for i in 1..1000 {
        let mut m = path.as_os_str().to_os_string();
        m.push(format!(".bak{i}"));
        let p = PathBuf::from(m);
        if !p.exists() {
            return p;
        }
    }
    first
}

#[derive(PartialEq, Debug)]
enum Outcome {
    Created,
    Updated,
    Unchanged,
    BackedUp,
}

fn write_file(path: &Path, data: &[u8], known: Option<&str>, dry: bool, log: &mut Vec<String>) -> Result<Outcome, String> {
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    match std::fs::read(path) {
        Ok(cur) => {
            if cur == data {
                log.push(format!("unchanged  {}", path.display()));
                return Ok(Outcome::Unchanged);
            }
            let ours = known.map(|h| h == fnv64_hex(&cur)).unwrap_or(false);
            if ours {
                log.push(format!("{} {}", if dry { "would update" } else { "updated   " }, path.display()));
                if !dry {
                    std::fs::write(path, data).map_err(io)?;
                }
                Ok(Outcome::Updated)
            } else {
                let bak = backup_name(path);
                log.push(format!(
                    "{} {} (differs from ours: kept as {})",
                    if dry { "would back up" } else { "backed up  " },
                    path.display(),
                    bak.display()
                ));
                if !dry {
                    std::fs::copy(path, &bak).map_err(io)?;
                    std::fs::write(path, data).map_err(io)?;
                }
                Ok(Outcome::BackedUp)
            }
        }
        Err(_) => {
            log.push(format!("{} {}", if dry { "would create" } else { "created   " }, path.display()));
            if !dry {
                if let Some(d) = path.parent() {
                    std::fs::create_dir_all(d).map_err(io)?;
                }
                std::fs::write(path, data).map_err(io)?;
            }
            Ok(Outcome::Created)
        }
    }
}

/// `file:///c:/Users/x/...` style path VS Code stores in extensions.json.
fn vscode_uri_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &s[1..])
    } else {
        s
    }
}

fn is_ours_entry(e: &Value) -> bool {
    let id = e.pointer("/identifier/id").and_then(|v| v.as_str()).unwrap_or("");
    let rel = e.get("relativeLocation").and_then(|v| v.as_str()).unwrap_or("");
    id.eq_ignore_ascii_case(VSCODE_ID)
        || rel.to_ascii_lowercase().starts_with(&format!("{}-", VSCODE_ID))
        || rel.to_ascii_lowercase().starts_with("qu-language-")
}

fn update_extensions_json(ext_dir: &Path, folder: &str, version: &str, add: bool, dry: bool, log: &mut Vec<String>) -> Result<(), String> {
    let path = ext_dir.join("extensions.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(()) };
    let mut v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            log.push(format!("warning: {} is not valid JSON ({e}); left alone, restart VS Code to pick up the folder", path.display()));
            return Ok(());
        }
    };
    let Some(arr) = v.as_array_mut() else {
        log.push(format!("warning: {} is not a JSON array; left alone", path.display()));
        return Ok(());
    };
    let existing: Vec<usize> = arr.iter().enumerate().filter(|(_, e)| is_ours_entry(e)).map(|(i, _)| i).collect();
    if add {
        if existing.len() == 1 {
            let e = &arr[existing[0]];
            if e.get("relativeLocation").and_then(|x| x.as_str()) == Some(folder)
                && e.get("version").and_then(|x| x.as_str()) == Some(version)
            {
                log.push(format!("unchanged  {} (entry for {folder})", path.display()));
                return Ok(());
            }
        }
        let full = ext_dir.join(folder);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let entry = json!({
            "identifier": {"id": VSCODE_ID},
            "version": version,
            "location": {"$mid": 1, "path": vscode_uri_path(&full), "scheme": "file"},
            "relativeLocation": folder,
            "metadata": {"installedTimestamp": now, "pinned": false}
        });
        for i in existing.iter().rev() {
            arr.remove(*i);
        }
        arr.push(entry);
        log.push(format!("{} {} (entry for {folder})", if dry { "would update" } else { "updated   " }, path.display()));
    } else {
        if existing.is_empty() {
            return Ok(());
        }
        for i in existing.iter().rev() {
            arr.remove(*i);
        }
        log.push(format!("{} entry for {VSCODE_ID} in {}", if dry { "would remove" } else { "removed   " }, path.display()));
    }
    if !dry {
        let bak = path.with_extension("json.bak");
        if !bak.exists() {
            std::fs::copy(&path, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
        }
        std::fs::write(&path, serde_json::to_string(&v).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Old `qu-language-*` / `qu-project.qu-language-*` folders (the hand-copied
/// 0.1.0 one, previous versions of this plugin) other than `keep`.
fn old_vscode_folders(ext_dir: &Path, keep: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(ext_dir) {
        for d in rd.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            let low = name.to_ascii_lowercase();
            if name != keep
                && (low.starts_with(&format!("{}-", VSCODE_ID)) || low.starts_with("qu-language-"))
                && d.path().is_dir()
            {
                v.push(d.path());
            }
        }
    }
    v.sort();
    v
}

fn owns_vscode_folder(p: &Path) -> bool {
    std::fs::read_to_string(p.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str().map(String::from)))
        .map(|n| n == "qu-language")
        .unwrap_or(false)
}

pub fn install_target(ed: Editor, t: &Target, dry: bool) -> Result<Vec<String>, String> {
    let kind = plugin_kind(ed).ok_or_else(|| format!("{} has no plugin files to install", ed.name()))?;
    let version = shipped_version(kind);
    let root = install_root(kind, t);
    let known = read_manifest(&root).unwrap_or_default();
    let mut log = Vec::new();

    if kind == PluginKind::VsCode {
        let keep = vscode_folder_name(&version);
        for old in old_vscode_folders(&t.dir, &keep) {
            if owns_vscode_folder(&old) {
                log.push(format!("{} older copy {}", if dry { "would remove" } else { "removed   " }, old.display()));
                if !dry {
                    std::fs::remove_dir_all(&old).map_err(|e| format!("{}: {e}", old.display()))?;
                }
            } else {
                log.push(format!("warning: {} looks like qu-language but its package.json name differs; left alone", old.display()));
            }
        }
    }

    let mut manifest_files = BTreeMap::new();
    for a in files_for(kind) {
        let dest = rel_path(&root, a.rel);
        write_file(&dest, a.data.as_bytes(), known.get(a.rel).map(String::as_str), dry, &mut log)?;
        manifest_files.insert(a.rel.to_string(), fnv64_hex(a.data.as_bytes()));
    }
    let manifest = json!({
        "tool": "qu editors",
        "qu_version": env!("CARGO_PKG_VERSION"),
        "plugin_version": version,
        "files": manifest_files,
    });
    let mtext = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    // The manifest is ours by definition: no backup dance.
    let mpath = root.join(MANIFEST);
    if std::fs::read_to_string(&mpath).ok().as_deref() != Some(mtext.as_str()) {
        log.push(format!("{} {}", if dry { "would write" } else { "wrote     " }, mpath.display()));
        if !dry {
            std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
            std::fs::write(&mpath, mtext).map_err(|e| format!("{}: {e}", mpath.display()))?;
        }
    }

    if kind == PluginKind::VsCode {
        update_extensions_json(&t.dir, &vscode_folder_name(&version), &version, true, dry, &mut log)?;
    }
    Ok(log)
}

pub fn uninstall_target(ed: Editor, t: &Target, dry: bool) -> Result<Vec<String>, String> {
    let kind = plugin_kind(ed).ok_or_else(|| format!("{} has no plugin files to remove", ed.name()))?;
    let root = install_root(kind, t);
    let mut log = Vec::new();
    let Some(files) = read_manifest(&root) else {
        if root.exists() {
            log.push(format!("{} exists but was not written by `qu editors install` (no manifest); left alone", root.display()));
        } else {
            log.push(format!("nothing at {}", root.display()));
        }
        return Ok(log);
    };
    for (rel, hash) in &files {
        let p = rel_path(&root, rel);
        match std::fs::read(&p) {
            Err(_) => {}
            Ok(cur) if fnv64_hex(&cur) == *hash => {
                log.push(format!("{} {}", if dry { "would remove" } else { "removed   " }, p.display()));
                if !dry {
                    std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                }
            }
            Ok(_) => log.push(format!("kept       {} (modified since install)", p.display())),
        }
    }
    if !dry {
        let _ = std::fs::remove_file(root.join(MANIFEST));
        // prune the directories we created, deepest first
        let mut dirs: Vec<PathBuf> = files.keys().filter_map(|r| rel_path(&root, r).parent().map(Path::to_path_buf)).collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        dirs.dedup();
        for d in dirs {
            let mut cur = d;
            // (Notepad++'s userDefineLangs is Notepad++'s own folder: never pruned.)
            while cur.starts_with(&root) && !(kind == PluginKind::NotepadPp && cur == root) {
                if std::fs::remove_dir(&cur).is_err() {
                    break;
                }
                if !cur.pop() {
                    break;
                }
            }
        }
        if kind == PluginKind::Vim {
            // pack/qu/start/qu -> start, qu, pack
            let mut cur = root.clone();
            for _ in 0..3 {
                if !cur.pop() || !cur.starts_with(&t.dir) || cur == t.dir {
                    break;
                }
                if std::fs::remove_dir(&cur).is_err() {
                    break;
                }
            }
        }
    }
    if kind == PluginKind::VsCode {
        update_extensions_json(&t.dir, &vscode_folder_name(&shipped_version(kind)), &shipped_version(kind), false, dry, &mut log)?;
    }
    Ok(log)
}

#[derive(Debug, Default, Clone)]
pub struct PluginState {
    pub installed: bool,
    /// Written by `qu editors install` (manifest present).
    pub managed: bool,
    pub version: Option<String>,
    pub shipped: String,
    pub up_to_date: bool,
    /// Files whose content differs from the embedded copy (same version).
    pub modified: Vec<String>,
    pub missing: Vec<String>,
    pub notes: Vec<String>,
    pub root: Option<PathBuf>,
    /// VS Code family: is it registered in extensions.json (None: no such file).
    pub in_extensions_json: Option<bool>,
}

pub fn plugin_state(ed: Editor, t: &Target) -> PluginState {
    let Some(kind) = plugin_kind(ed) else { return PluginState::default() };
    let shipped = shipped_version(kind);
    let mut st = PluginState { shipped: shipped.clone(), ..Default::default() };

    let root = if kind == PluginKind::VsCode {
        let want = t.dir.join(vscode_folder_name(&shipped));
        if want.is_dir() {
            Some(want)
        } else {
            let olds = old_vscode_folders(&t.dir, "");
            if olds.len() > 1 {
                st.notes.push(format!(
                    "{} qu-language folders present ({}); VS Code may load the wrong one",
                    olds.len(),
                    olds.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
            olds.into_iter().next()
        }
    } else {
        let r = install_root(kind, t);
        if files_for(kind).iter().any(|a| rel_path(&r, a.rel).is_file()) {
            Some(r)
        } else {
            None
        }
    };
    let Some(root) = root else { return st };
    st.installed = true;
    st.managed = read_manifest(&root).is_some();
    if !st.managed {
        st.notes.push("not installed by `qu editors install` (hand-copied or from an installer); `qu editors install` will take it over".into());
    }
    st.version = if kind == PluginKind::VsCode {
        std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("version").and_then(|s| s.as_str().map(String::from)))
    } else {
        files_for(kind)
            .first()
            .and_then(|a| std::fs::read_to_string(rel_path(&root, a.rel)).ok())
            .and_then(|t| marker_version(&t))
    };
    let same_version = st.version.as_deref() == Some(shipped.as_str());
    for a in files_for(kind) {
        let p = rel_path(&root, a.rel);
        match std::fs::read(&p) {
            Err(_) => st.missing.push(a.rel.to_string()),
            Ok(cur) => {
                if same_version && cur != a.data.as_bytes() {
                    st.modified.push(a.rel.to_string());
                }
            }
        }
    }
    st.up_to_date = same_version && st.missing.is_empty();
    if st.version.is_none() {
        st.notes.push("no version marker found (installed by an older qu)".into());
    }
    if kind == PluginKind::VsCode {
        if let Ok(text) = std::fs::read_to_string(t.dir.join("extensions.json")) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                let folder = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                st.in_extensions_json = Some(
                    v.as_array()
                        .map(|a| a.iter().any(|e| is_ours_entry(e) && e.get("relativeLocation").and_then(|r| r.as_str()) == Some(folder.as_str())))
                        .unwrap_or(false),
                );
            }
        }
    }
    st.root = Some(root);
    st
}
