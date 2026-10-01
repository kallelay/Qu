//! § file history / versioning (2026-10-01).
//!
//! With history on (`file_versioning(true)`, or `QU_FILE_HISTORY=1` in the
//! environment), every builtin that is about to replace or rewrite an
//! existing file first copies its current content into a version history,
//! so a script that overwrites the wrong file -- or the right file with
//! the wrong data -- can be undone: `file_history(path)` lists the
//! versions, `read_version(path, v)` reads one, `restore_version(path, v)`
//! puts one back (itself snapshotting first, so a restore is undoable too).
//!
//! **Same store as Qu Studio.** Studio's editor already versions every
//! save into a sibling `.qu-versions/<file name>/<nanoseconds>.qu` folder
//! (`qu-studio-tauri/src-tauri/src/main.rs`, `snapshot_before_overwrite`).
//! This writes the identical layout -- including the `.qu` suffix on every
//! snapshot whatever the file type, and the 50-version cap -- so a version
//! a script made shows up in Studio's history panel and vice versa.
//!
//! **Where it hooks.** One table, [`WRITERS`], names every path-writing
//! builtin and which argument is the target; `dispatch_builtin` consults it
//! before running the builtin. Handle-based writes (`write_line`,
//! `write_int32`, ...) all start from `fopen(path, "w"/"a")`, which is
//! hooked instead. Off by default: a script that never asked for history
//! never grows `.qu-versions` folders.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{arg0, civil_from_unix, e, table, text_arg, EvalError, Value, R};

pub const HISTORY_DIR: &str = ".qu-versions";
pub const MAX_VERSIONS: usize = 50;
/// Files larger than this are not copied on every write -- a multi-GB
/// data file rewritten in a loop would fill the disk with history.
pub const MAX_SNAPSHOT_BYTES: u64 = 100 * 1024 * 1024;

/// `(builtin, index of its target-path argument)` for every builtin that
/// replaces or rewrites a file's content. `fopen` is handled separately
/// (only its write/append modes count).
pub const WRITERS: &[(&str, usize)] = &[
    ("write_text", 0),
    ("write_all", 0),
    ("append_text", 0),
    ("append_all", 0),
    ("write_csv", 1),
    ("save", 0),
    ("save_all", 0),
    ("save_image", 0),
    ("save_model", 1),
    ("save_svg", 0),
    ("savefig", 0),
    ("write_report", 0),
    ("copy_file", 1),
    ("move_file", 1),
    ("rename_file", 1),
    ("create_file", 0),
    ("make_file", 0),
    ("remove_file", 0),
    ("codec::write_wav", 0),
    ("pdf::write_merge", 0),
    ("pdf::write_pages", 0),
    ("xlsx::write", 0),
    ("docx::save_as", 1),
    ("pptx::save_as", 1),
    ("xlsx::save_as", 1),
    ("docx::to_pdf", 1),
    ("pptx::to_pdf", 1),
    ("xlsx::to_pdf", 1),
    ("docx::to_latex", 1),
];

pub const NAMES: &[&str] = &["file_versioning", "file_history", "read_version", "restore_version"];

/// Whether history starts on, from `QU_FILE_HISTORY` (`1`/`true`/`on`/`yes`).
pub fn enabled_from_env() -> bool {
    std::env::var("QU_FILE_HISTORY")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false)
}

/// The target path a writer builtin is about to touch, if `f` is one and
/// the call would actually change an existing file.
pub fn target_of(f: &str, args: &[Value], style: &[(String, Value)]) -> Option<String> {
    if f == "fopen" {
        let mode = args.get(1).map(crate::display_value).unwrap_or_default();
        return if mode.starts_with('w') || mode.starts_with('a') { path_arg(args, 0) } else { None };
    }
    // A move/rename/copy onto an existing name only replaces it with
    // `on_exists="overwrite"`; otherwise it errors or skips, and a
    // snapshot would be a version of nothing that happened.
    if matches!(f, "copy_file" | "move_file" | "rename_file" | "create_file") {
        let overwrite = style
            .iter()
            .any(|(k, v)| k == "on_exists" && matches!(v, Value::Str(s) if s == "overwrite"));
        if !overwrite {
            return None;
        }
    }
    // `remove_file` to the recycle bin is already recoverable; only a
    // permanent delete needs a version kept.
    if f == "remove_file" && !crate::fs_ops::remove_is_permanent(style).unwrap_or(false) {
        return None;
    }
    let (_, idx) = WRITERS.iter().find(|(name, _)| *name == f)?;
    path_arg(args, *idx)
}

fn path_arg(args: &[Value], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn versions_dir(path: &Path) -> Option<PathBuf> {
    Some(path.parent()?.join(HISTORY_DIR).join(path.file_name()?))
}

/// Copies `path`'s current content into its history before it is
/// overwritten. A missing file, a directory, or a file inside a history
/// folder has nothing to keep and is `Ok(None)`; `Ok(Some(msg))` is a
/// one-off note (a file too large to version) for the caller to print.
pub fn snapshot(path: &str) -> std::io::Result<Option<String>> {
    let p = Path::new(path);
    if !p.is_file() || p.components().any(|c| c.as_os_str() == HISTORY_DIR) {
        return Ok(None);
    }
    let len = p.metadata()?.len();
    if len > MAX_SNAPSHOT_BYTES {
        return Ok(Some(format!(
            "note: `{path}` is {} MB, over the {} MB file-history limit, so this write keeps no version of it",
            len / (1024 * 1024),
            MAX_SNAPSHOT_BYTES / (1024 * 1024)
        )));
    }
    let Some(dir) = versions_dir(p) else { return Ok(None) };
    std::fs::create_dir_all(&dir)?;
    // Nanoseconds, bumped past any name already there: two writes in the
    // same clock tick must not overwrite each other's snapshot (Studio's
    // own store learnt this from a test that counted survivors).
    let mut stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    while dir.join(format!("{stamp}.qu")).exists() {
        stamp += 1;
    }
    std::fs::copy(p, dir.join(format!("{stamp}.qu")))?;
    prune(&dir)?;
    Ok(None)
}

fn prune(dir: &Path) -> std::io::Result<()> {
    let mut v = list(dir)?;
    if v.len() > MAX_VERSIONS {
        v.sort_by_key(|(n, _)| *n);
        for (_, stale) in &v[..v.len() - MAX_VERSIONS] {
            let _ = std::fs::remove_file(stale);
        }
    }
    Ok(())
}

/// `(stamp, snapshot path)` for every version, unsorted.
fn list(dir: &Path) -> std::io::Result<Vec<(u64, PathBuf)>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(n) = name.strip_suffix(".qu").and_then(|s| s.parse::<u64>().ok()) {
            out.push((n, entry.path()));
        }
    }
    Ok(out)
}

/// Newest first.
fn versions_of(path: &str) -> R<Vec<(u64, PathBuf)>> {
    let Some(dir) = versions_dir(Path::new(path)) else { return Ok(Vec::new()) };
    let mut v = list(&dir).map_err(|err| EvalError { msg: format!("file_history: cannot read `{}`: {err}", dir.display()) })?;
    v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    Ok(v)
}

/// A version by its id (the string `file_history` lists) or by position
/// (`1` = the most recent, `2` the one before, ...).
fn pick(f: &str, path: &str, which: &Value) -> R<PathBuf> {
    let v = versions_of(path)?;
    if v.is_empty() {
        return e(format!("{f}: `{path}` has no saved versions (turn history on with file_versioning(true))"));
    }
    match which {
        Value::Num(k) if *k >= 1.0 && k.fract() == 0.0 => match v.get(*k as usize - 1) {
            Some((_, p)) => Ok(p.clone()),
            None => e(format!("{f}: `{path}` has {} saved version(s); there is no version {k}", v.len())),
        },
        Value::Str(id) => match v.iter().find(|(n, _)| n.to_string() == *id) {
            Some((_, p)) => Ok(p.clone()),
            None => e(format!("{f}: `{path}` has no version `{id}` -- file_history(path) lists them")),
        },
        other => e(format!(
            "{f}: a version is its id from file_history(path) or its position (1 = newest), found {}",
            crate::display_value(other)
        )),
    }
}

fn iso_utc(nanos: u64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_unix((nanos / 1_000_000_000) as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

/// `file_versioning`, `file_history`, `read_version`; `restore_version`
/// needs the interpreter's own snapshot (it is undoable) and is passed it.
pub fn call(f: &str, args: &[Value], enabled: &mut bool) -> R<Value> {
    match f {
        "file_versioning" => {
            let before = *enabled;
            if let Some(v) = args.first() {
                *enabled = crate::truthy(v);
            }
            Ok(Value::Bool(before))
        }
        "file_history" => {
            let path = text_arg(args, 0)?;
            let v = versions_of(&path)?;
            let mut ids = Vec::new();
            let mut times = Vec::new();
            let mut sizes = Vec::new();
            for (n, p) in &v {
                ids.push(n.to_string());
                times.push(iso_utc(*n));
                sizes.push(p.metadata().map(|m| m.len() as f64).unwrap_or(f64::NAN));
            }
            let t = table::Table::from_columns(vec![
                ("version".into(), table::Column::Str(ids)),
                ("saved".into(), table::Column::Str(times)),
                ("bytes".into(), table::Column::Num(sizes)),
            ])
            .map_err(|msg| EvalError { msg })?;
            Ok(Value::Table(Arc::new(t)))
        }
        "read_version" => {
            let path = text_arg(args, 0)?;
            let which = args.get(1).ok_or_else(|| EvalError { msg: "read_version(path, version) needs a version".into() })?;
            let snap = pick(f, &path, which)?;
            let bytes = std::fs::read(&snap).map_err(|err| EvalError { msg: format!("read_version: {err}") })?;
            Ok(Value::Str(String::from_utf8_lossy(&bytes).into_owned()))
        }
        "restore_version" => {
            let path = text_arg(args, 0)?;
            let which = args.get(1).ok_or_else(|| EvalError { msg: "restore_version(path, version) needs a version".into() })?;
            let snap = pick(f, &path, which)?;
            // Read before snapshotting: version 1 is about to stop being
            // the newest, and its file must not be pruned under us.
            let bytes = std::fs::read(&snap).map_err(|err| EvalError { msg: format!("restore_version: {err}") })?;
            // Always undoable, history on or not.
            snapshot(&path).map_err(|err| EvalError { msg: format!("restore_version: could not keep the current version: {err}") })?;
            std::fs::write(&path, bytes).map_err(|err| EvalError { msg: format!("restore_version: could not write `{path}`: {err}") })?;
            Ok(Value::Nothing)
        }
        _ => {
            let _ = arg0(args)?;
            e(format!("file_history: unknown function `{f}`"))
        }
    }
}
