//! "Open a file by path": `Qu Studio.exe "<path>"` (what the Windows file
//! association runs, and what `qu-studio <path>` does on Linux/macOS).
//!
//! The contract with the frontend has no race in it:
//!
//!  * `setup()` reads `std::env::args()`, validates the path and parks it in
//!    [`StartupFile`].
//!  * The frontend calls `get_startup_file` once on mount (after it has
//!    registered its own `qu-open-file` listener). The command hands the path
//!    over exactly once.
//!  * As a belt-and-braces fallback, a watcher thread emits `qu-open-file`
//!    if nobody has claimed the path after a few seconds (a frontend that
//!    mounted but whose poll failed). Opening is idempotent on the
//!    frontend (the tab id is the path), so a double delivery is harmless.
//!
//! Single instance: Tauri 1.8 has no single-instance plugin in this repo and
//! we add no git dependencies, so a second launch simply opens a second
//! window with its own file.
//!
//! macOS "open with" / dock-drop arrives as an Apple Event, not argv.
//! `tauri::RunEvent::Opened` does not exist in Tauri 1.8.3 (it was added in
//! Tauri 2), so that path is NOT supported here; launching the binary with a
//! path argument is.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct StartupFile {
    path: Mutex<Option<Result<String, String>>>,
    claimed: AtomicBool,
}

impl StartupFile {
    pub fn set(&self, p: String) {
        *self.path.lock().unwrap() = Some(Ok(p));
    }
    /// Records that the argument was unusable, so the frontend can say so.
    pub fn set_error(&self, e: String) {
        *self.path.lock().unwrap() = Some(Err(e));
    }
    pub fn peek(&self) -> Option<String> {
        self.path.lock().unwrap().clone().and_then(|r| r.ok())
    }
    pub fn is_claimed(&self) -> bool {
        self.claimed.load(Ordering::SeqCst)
    }
}

/// First command-line argument (after argv[0]) that is not a flag.
/// Flags (`-x`, `--foo`) are skipped so `tauri dev`/WebView flags passed
/// through do not get mistaken for a file.
pub fn pick_startup_arg<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && !a.trim().is_empty())
}

/// `\\?\C:\x` -> `C:\x`, `\\?\UNC\srv\share\x` -> `\\srv\share\x`.
/// `std::fs::canonicalize` returns verbatim paths on Windows; they are
/// valid but ugly in tab titles and break some string comparisons.
pub fn strip_verbatim(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        p.to_string()
    }
}

/// Validates a path argument: strips stray quotes, requires that it exists
/// and is a regular file, and returns the canonical, non-verbatim form.
pub fn validate_open_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return Err("empty path".into());
    }
    let canon: PathBuf = std::fs::canonicalize(Path::new(trimmed))
        .map_err(|e| format!("cannot open \"{}\": {}", trimmed, e))?;
    if !canon.is_file() {
        return Err(format!("\"{}\" is not a file", trimmed));
    }
    Ok(strip_verbatim(&canon.to_string_lossy()))
}

/// What `get_startup_file` returns: exactly one of the two fields is set.
#[derive(serde::Serialize)]
pub struct StartupReply {
    pub path: Option<String>,
    pub error: Option<String>,
}

/// Hands the startup path (or the reason it was unusable) to the frontend
/// exactly once.
#[tauri::command]
pub fn get_startup_file(state: tauri::State<'_, StartupFile>) -> Option<StartupReply> {
    let r = state.path.lock().unwrap().take()?;
    state.claimed.store(true, Ordering::SeqCst);
    Some(match r {
        Ok(path) => StartupReply { path: Some(path), error: None },
        Err(error) => StartupReply { path: None, error: Some(error) },
    })
}

/// Opens a .pdf/.svg (the only things the viewers hand over) in the OS
/// default application. A dedicated command rather than the shell plugin's
/// `open`, because that one validates against a URL-only regex and refuses
/// file paths unless `tauri.conf.json`'s shell scope is loosened.
#[tauri::command]
pub fn open_in_system(path: String) -> Result<(), String> {
    let p = validate_open_path(&path)?;
    let ext = Path::new(&p)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext != "pdf" && ext != "svg" {
        return Err(format!("refusing to open a .{} file externally", ext));
    }
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer.exe").arg(&p).spawn();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&p).spawn();
    status.map(|_| ()).map_err(|e| format!("could not launch the system viewer: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_argv0_and_flags() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_startup_arg(a(&["qu-studio.exe"])), None);
        assert_eq!(
            pick_startup_arg(a(&["qu-studio.exe", "C:\\My Files\\a.qu"])),
            Some("C:\\My Files\\a.qu".to_string())
        );
        assert_eq!(
            pick_startup_arg(a(&["x", "--remote-debugging-port=1", "ü ä.svg", "b.pdf"])),
            Some("ü ä.svg".to_string())
        );
        assert_eq!(pick_startup_arg(a(&["x", "", "  "])), None);
    }

    #[test]
    fn strips_verbatim_prefixes() {
        assert_eq!(strip_verbatim(r"\\?\C:\a b\c.qu"), r"C:\a b\c.qu");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\c.qu"), r"\\srv\share\c.qu");
        assert_eq!(strip_verbatim("/home/u/c.qu"), "/home/u/c.qu");
    }

    #[test]
    fn validates_existing_files_with_spaces_and_unicode() {
        let dir = std::env::temp_dir().join(format!("qu_startup_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("mein Skript ä.qu");
        std::fs::write(&f, "x = 1").unwrap();
        let got = validate_open_path(&format!("\"{}\"", f.display())).unwrap();
        assert!(!got.starts_with(r"\\?\"));
        assert!(got.ends_with("mein Skript ä.qu"));
        assert!(validate_open_path(&dir.to_string_lossy()).is_err(), "dirs are refused");
        assert!(validate_open_path(&dir.join("nope.qu").to_string_lossy()).is_err());
        assert!(validate_open_path("").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn startup_file_is_handed_over_once() {
        let s = StartupFile::default();
        s.set("p".into());
        assert_eq!(s.peek(), Some("p".into()));
        assert!(!s.is_claimed());
    }
}
