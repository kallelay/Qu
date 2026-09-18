// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::State;

mod llm_providers;
mod llm_bridge;
mod gui_bridge;
use gui_bridge::{gui_start, gui_event, gui_stop, gui_snapshot};
mod serial_protocol;
mod seriplot;
use seriplot::{seriplot_ports, seriplot_start, seriplot_stop, seriplot_poll, seriplot_record, seriplot_buffer};
mod repl_bridge;
use repl_bridge::{repl_run, repl_restart, ReplState};
use llm_bridge::{
    llm_chat, llm_complete, llm_fix_error, llm_transform_code, get_llm_provider_config,
    set_llm_provider_config, test_llm_provider,
};

#[cfg(windows)]
const QU_EXE_NAME: &str = "qu.exe";
#[cfg(not(windows))]
const QU_EXE_NAME: &str = "qu";

/// Locate the `qu` CLI binary that `execute_code` shells out to, for use as
/// a *fallback* when the Tauri sidecar (see `run_qu_sidecar`) isn't
/// available -- e.g. a dev checkout where `binaries/qu-<target-triple>.exe`
/// was never populated.
///
/// Checked in order:
/// 1. Right next to this binary (the layout after `cargo tauri build` /
///    `cargo install`, where both end up in the same directory). This is
///    also where the sidecar ends up once `tauri-build`'s `externalBin`
///    copy step has run, so in practice this check alone covers most
///    sidecar-configured builds too.
/// 2. The Qu engine's own dev-tree build output. This crate lives at
///    `<repo>/qu-studio-tauri/src-tauri`, with its own independent
///    Cargo workspace (see the empty `[workspace]` table in Cargo.toml),
///    so `cargo tauri dev` builds this binary into
///    `qu-studio-tauri/src-tauri/target/{debug,release}/`, entirely
///    separate from `qu-cli`'s own build at `engine/target/{debug,release}/`.
///    Without this fallback, the Run button can never find `qu.exe` in a
///    normal dev checkout, even one where `engine` has been fully built but
///    `binaries/` hasn't been populated for the sidecar.
/// 3. Anywhere on PATH.
fn find_qu_executable() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| find_qu_executable_near(&exe))
}

fn find_qu_executable_near(exe: &Path) -> Option<PathBuf> {
    if let Some(dir) = exe.parent() {
        let candidate = dir.join(QU_EXE_NAME);
        if candidate.exists() {
            return Some(candidate);
        }

        // dir is normally .../qu-studio-tauri/src-tauri/target/<profile>;
        // walk up 4 levels to the repo root, then down into engine's own
        // target dir.
        for profile in ["debug", "release"] {
            let candidate = dir
                .join("../../../../engine/target")
                .join(profile)
                .join(QU_EXE_NAME);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(QU_EXE_NAME))
            .find(|p| p.exists())
    })
}

// ============ Simple Request/Response Types ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub code: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub elapsed_ms: f64,
    /// Any figures produced by this run, as ready-to-display `data:` URIs
    /// (`data:image/svg+xml;base64,...` or `data:image/png;base64,...`), in
    /// the order they were discovered. See `collect_plot_outputs` for what
    /// counts as "produced": the auto-rendered final figure state (so plain
    /// `plot(x, y)` with no `savefig` call still shows something, matching
    /// what a user coming from MATLAB/Jupyter expects) plus any files the
    /// script itself wrote via explicit `savefig(...)` calls.
    pub plots: Vec<String>,
    /// The script's final top-level variable bindings, from `--emit-vars`
    /// (see `read_vars_output`). Empty whenever the emitted file is missing,
    /// unreadable, or fails to parse -- a variables-panel miss is never
    /// worth turning a successful run into a reported failure over.
    pub variables: Vec<VariableInfo>,
    /// The script's final `number`/`bool`/`vector`/`matrix` bindings, as
    /// UNTRUNCATED numeric data (from `--emit-data`, see
    /// `read_data_output`/`qu-cli`'s `data_to_json`). Requested on every run
    /// (not just Interactive Mode's) for the same reason `plots`/`variables`
    /// already are unconditionally: the extra `qu run` flag is cheap next to
    /// the process-spawn cost this command already pays, so there is no
    /// real gain from threading an opt-in flag through `ExecuteRequest` --
    /// QuStudio's normal Code/DSP/ML tabs simply never read this field.
    pub data: Vec<PlotVar>,
}

/// One numeric top-level binding, in full, for QuStudio's Interactive Mode
/// plotting -- as opposed to `VariableInfo`'s truncated human-readable
/// `preview`. `shape` is `[rows, cols]`; `data` is that many `f64`s in
/// `qu-core::matrix`'s own column-major order for a `matrix`-typed binding
/// (`shape == [len, 1]` and row-major-equals-column-major for a `vector`),
/// so a consumer reading a matrix back must index it the same way.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotVar {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub shape: Vec<usize>,
    /// `Option<f64>`, not `f64`, and that distinction is the whole reason
    /// Interactive Mode ever plotted anything.
    ///
    /// JSON has no way to spell infinity or NaN, so `qu run --emit-data`
    /// writes them as `null` -- and Qu binds `inf`/`nan` as ordinary
    /// globals, so EVERY run's data document contains at least two of
    /// them, before the script's own values are considered. With
    /// `Vec<f64>` here, serde could not deserialize those nulls, the whole
    /// `Vec<PlotVar>` parse failed, and `read_data_output`'s deliberate
    /// "a data miss is never worth failing a good run over" fallback
    /// turned that into an empty list. The run looked completely
    /// successful -- output present, `variables` populated, no error --
    /// with `data` silently empty, so Interactive Mode had nothing to plot
    /// and rendered its empty state forever. Found 2026-09-04 by driving
    /// the real shipped window over CDP and calling `execute_code`
    /// directly; it reported `success: true` with zero data.
    ///
    /// Keeping the nulls (rather than dropping or zeroing those entries)
    /// is also the honest representation: they reach the frontend as
    /// `null`, which Plotly already renders as a gap in a line rather than
    /// a fake zero.
    pub data: Vec<Option<f64>>,
}

/// One binding shown in QuStudio's Variables panel, mirroring the frontend's
/// own `Variable` interface in `qu-studio-tauri/src/App.tsx`
/// (`{name, type, value}`) so no translation is needed on the TS side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
}

/// Shape of the JSON `qu run --emit-vars` writes (see `qu-cli`'s
/// `vars_to_json`): `[{"name":.., "type":.., "preview":..}, ...]`. Kept as a
/// separate struct from `VariableInfo` because the CLI's field is named
/// `preview`, not `value` -- `read_vars_output` renames it on the way in
/// rather than making the two crates agree on a field name they have no
/// other reason to share.
#[derive(Debug, Clone, Deserialize)]
struct RawVariableJson {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    preview: String,
}

/// One parse-error diagnostic for QuStudio's inline (squiggle) diagnostics
/// feature, mirroring the frontend's `QuDiagnostic` interface in
/// `qu-ui-components`'s `CodeEditor.tsx` (`{message, line, column}`) so no
/// field renaming is needed on the TS side. `line`/`column` are carried
/// straight through from `qu_syntax::ParseError`'s own `Span` (1-based,
/// already matching Monaco's marker coordinate convention).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub path: String,
}

// ============ Application State ============

pub struct AppState {
    pub workspace_path: Arc<Mutex<Option<String>>>,
    pub execution_count: Arc<Mutex<usize>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            workspace_path: Arc::new(Mutex::new(None)),
            execution_count: Arc::new(Mutex::new(0)),
        }
    }
}

// ============ Tauri Commands ============

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Qu Studio!", name)
}

/// Result of trying to run `qu run <temp_path>` via one particular
/// mechanism: `Ok((success, stdout, stderr))` on a successful *spawn*
/// (regardless of the child's own exit code -- that's the `success` bool),
/// `Err` only when the mechanism itself couldn't launch a process at all
/// (binary missing, sidecar not configured, etc.), so the caller knows to
/// try the next fallback.
type SpawnResult = Result<(bool, String, String), String>;

/// Drives a Tauri `process::Command` to completion WITHOUT blocking the
/// calling thread -- the async-native replacement for `Command::output()`.
///
/// Real bug found and fixed live (2026-09-04), reported as "when running
/// program, the UI hangs": `Command::output()` (used here previously)
/// internally calls `tauri::async_runtime::safe_block_on(...)` to wait for
/// the child process (see that method's own source in the `tauri` crate) --
/// a genuine thread-blocking call. `execute_code` (below) was a plain,
/// non-`async` `#[tauri::command]` fn, and Tauri v1 dispatches non-async
/// commands on the MAIN thread by default (the same thread that pumps the
/// webview's event loop) -- so calling `.output()` from it froze the ENTIRE
/// UI for as long as the spawned `qu.exe` process took to finish. A script
/// that runs for more than an instant (or, worse, an accidental infinite
/// loop in a demo) made the whole app unresponsive, not just the Run
/// button. This is the standard failure mode for any GUI that shells out
/// to an interpreter synchronously on its own UI thread -- the standard
/// fix (and the standard this project should hold to for any future
/// command that spawns a process or does other blocking I/O) is: make the
/// command `async fn`, and drive the child process via a real async
/// event stream (`Command::spawn()`'s own `Receiver<CommandEvent>`,
/// awaited directly) so the Tokio runtime backing Tauri can interleave it
/// with UI event processing instead of the calling thread parking on it.
async fn run_command_async(command: tauri::api::process::Command) -> SpawnResult {
    use tauri::api::process::CommandEvent;
    let (mut rx, _child) = command
        .spawn()
        .map_err(|e| format!("failed to spawn qu: {}", e))?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code: Option<i32> = None;
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(line) => {
                stdout.push_str(&line);
                stdout.push('\n');
            }
            CommandEvent::Stderr(line) => {
                stderr.push_str(&line);
                stderr.push('\n');
            }
            CommandEvent::Terminated(payload) => exit_code = payload.code,
            CommandEvent::Error(e) => stderr.push_str(&e),
            _ => {}
        }
    }
    Ok((exit_code == Some(0), stdout, stderr))
}

/// Preferred path: run `qu` as a Tauri sidecar (`externalBin` in
/// `tauri.conf.json`). `tauri::api::process::Command::new_sidecar` resolves
/// to `<dir of this running exe>/qu.exe` -- exactly where `tauri-build`'s
/// externalBin copy step (see `build.rs`) places it on *every* `cargo
/// build`/`cargo tauri dev`/`cargo tauri build`, as long as
/// `src-tauri/binaries/qu-<target-triple>.exe` exists when this crate is
/// built. This is the only mechanism guaranteed to work in a real installed
/// build, since it doesn't assume anything about where `qu-cli` was built
/// from or whether the repo tree still exists on disk.
async fn run_qu_sidecar(
    temp_path: &Path,
    emit_figure_path: &Path,
    emit_vars_path: &Path,
    emit_data_path: &Path,
) -> SpawnResult {
    let command = tauri::api::process::Command::new_sidecar("qu")
        .map_err(|e| format!("sidecar not configured: {}", e))?
        .args([
            "run".to_string(),
            temp_path.to_string_lossy().into_owned(),
            "--emit-figure".to_string(),
            emit_figure_path.to_string_lossy().into_owned(),
            "--emit-vars".to_string(),
            emit_vars_path.to_string_lossy().into_owned(),
            "--emit-data".to_string(),
            emit_data_path.to_string_lossy().into_owned(),
        ]);
    run_command_async(command).await
}

/// Dev-mode / non-sidecar fallback: search for `qu(.exe)` at the relative
/// locations `find_qu_executable` knows about, then fall back to bare PATH
/// lookup. Kept so `cargo tauri dev` (and any environment where the sidecar
/// binary hasn't been placed under `binaries/`) still works without
/// requiring a full sidecar rebuild on every engine change. Uses the same
/// `tauri::api::process::Command` (not `std::process::Command`) as the
/// sidecar path above specifically so it can share `run_command_async` --
/// one non-blocking process-execution path, not two.
async fn run_qu_fallback(
    temp_path: &Path,
    emit_figure_path: &Path,
    emit_vars_path: &Path,
    emit_data_path: &Path,
) -> SpawnResult {
    let program = find_qu_executable()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| QU_EXE_NAME.to_string());

    let command = tauri::api::process::Command::new(program).args([
        "run".to_string(),
        temp_path.to_string_lossy().into_owned(),
        "--emit-figure".to_string(),
        emit_figure_path.to_string_lossy().into_owned(),
        "--emit-vars".to_string(),
        emit_vars_path.to_string_lossy().into_owned(),
        "--emit-data".to_string(),
        emit_data_path.to_string_lossy().into_owned(),
    ]);

    run_command_async(command).await.map_err(|e| {
        format!(
            "Failed to execute Qu code: {}\n\n\
             Make sure the Qu CLI is installed:\n\
             cd engine/crates/qu-cli\n\
             cargo install --path .\n\
             \n\
             Or add qu.exe to your PATH.",
            e
        )
    })
}

/// `qu docs --json` sidecar/fallback pair, same shape as `run_qu_sidecar`/
/// `run_qu_fallback` above but for the builtin-reference lookup instead of
/// running a script -- no temp files needed, just the two args.
async fn run_qu_docs_sidecar() -> SpawnResult {
    let command = tauri::api::process::Command::new_sidecar("qu")
        .map_err(|e| format!("sidecar not configured: {}", e))?
        .args(["docs".to_string(), "--json".to_string()]);
    run_command_async(command).await
}

async fn run_qu_docs_fallback() -> SpawnResult {
    let program = find_qu_executable()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| QU_EXE_NAME.to_string());
    let command =
        tauri::api::process::Command::new(program).args(["docs".to_string(), "--json".to_string()]);
    run_command_async(command).await
}

/// Backs the Help Browser panel. Returns the raw JSON array `qu docs
/// --json` prints (see `qu-cli/src/main.rs`'s `builtin_docs_json`) --
/// passed through rather than re-typed into a matching Rust struct, since
/// this command only ever forwards it to the frontend, which already knows
/// the shape it wants (`name`/`signature`/`summary`/`chapter`).
#[tauri::command]
async fn list_builtin_docs() -> Result<serde_json::Value, String> {
    let (ok, stdout, stderr) = match run_qu_docs_sidecar().await {
        Ok(r) => r,
        Err(_) => run_qu_docs_fallback().await?,
    };
    if !ok {
        return Err(format!("qu docs --json failed: {}", stderr.trim()));
    }
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse builtin docs JSON: {}", e))
}

/// Base name `--emit-figure` writes the auto-rendered final-figure SVG to,
/// inside each run's own temp directory (see `execute_code`). Chosen to be
/// unlikely to collide with a filename a real script's own `savefig(...)`
/// call would use, so `collect_plot_outputs`'s "any other image file in the
/// run dir came from the script's own savefig call" scan doesn't confuse
/// the two.
const AUTO_FIGURE_NAME: &str = "__qu_studio_auto_figure.svg";

/// Base name `--emit-vars` writes the final-bindings JSON to, inside each
/// run's own temp directory -- same rationale as `AUTO_FIGURE_NAME`.
const AUTO_VARS_NAME: &str = "__qu_studio_auto_vars.json";

/// Base name `--emit-data` writes the full-fidelity numeric-bindings JSON
/// to, inside each run's own temp directory -- same rationale as
/// `AUTO_FIGURE_NAME`/`AUTO_VARS_NAME`.
const AUTO_DATA_NAME: &str = "__qu_studio_auto_data.json";

/// Reads and parses `--emit-vars`'s JSON output (see `qu-cli`'s
/// `vars_to_json`) from `dir`, if it's there. Missing file (e.g. `qu run`
/// hit a parse error before ever producing output) or malformed JSON both
/// just yield an empty list rather than an error -- a blank Variables panel
/// is the right degraded behavior for a run that otherwise succeeded or
/// already reported its own error through `stderr`.
fn read_vars_output(dir: &Path) -> Vec<VariableInfo> {
    let path = dir.join(AUTO_VARS_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<RawVariableJson>>(&text) {
        Ok(raw) => raw
            .into_iter()
            .map(|r| VariableInfo {
                name: r.name,
                kind: r.kind,
                value: r.preview,
            })
            .collect(),
        Err(e) => {
            eprintln!("qu-studio: warning: could not parse --emit-vars output: {}", e);
            Vec::new()
        }
    }
}

/// Reads and parses `--emit-data`'s JSON output (see `qu-cli`'s
/// `data_to_json`) from `dir`, if it's there. Same degraded-empty-list
/// behavior as `read_vars_output` on a missing/malformed file: a script
/// whose run already failed (or that produced no numeric bindings) simply
/// has nothing to plot in Interactive Mode, which isn't itself an error.
/// `PlotVar`'s fields already match `data_to_json`'s JSON shape exactly
/// (`name`/`type`/`shape`/`data`), so this deserializes straight into it --
/// no separate `Raw*Json` field-renaming struct needed here, unlike
/// `RawVariableJson`.
fn read_data_output(dir: &Path) -> Vec<PlotVar> {
    let path = dir.join(AUTO_DATA_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<PlotVar>>(&text) {
        Ok(vars) => vars,
        Err(e) => {
            eprintln!("qu-studio: warning: could not parse --emit-data output: {}", e);
            Vec::new()
        }
    }
}

/// Reads every image file (`.svg`/`.png`) written into `dir` during a run --
/// the `--emit-figure` auto-render plus anything the script itself wrote via
/// explicit `savefig(...)` calls -- as `data:` URIs, in modified-time order
/// (oldest first; ties broken by name) so the auto-rendered figure and any
/// `savefig` output land in a stable, predictable order for the frontend.
///
/// Deliberately does NOT try to deduplicate the auto-rendered figure against
/// a same-content `savefig(...)` output: a script that calls both `plot()`
/// and its own `savefig("x.svg")` will show two very similar images. That's
/// a documented minor redundancy (see BACKLOG.md), not a correctness bug --
/// silently guessing "these two files are the same plot" from content alone
/// would risk hiding a real second figure that just happens to render
/// similarly (e.g. the same axes re-plotted with one more series).
/// Compare two paths treating digit runs as numbers, so `fig_2` sorts
/// before `fig_10` the way a human reading a figure list expects.
fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    // Compare the whole digit runs as integers. Parsed as
                    // u128 with a saturating fallback so an absurdly long
                    // run of digits in some unrelated filename orders
                    // consistently instead of panicking.
                    let take_num = |it: &mut std::iter::Peekable<std::str::Chars>| -> u128 {
                        let mut s = String::new();
                        while let Some(c) = it.peek().copied() {
                            if !c.is_ascii_digit() {
                                break;
                            }
                            s.push(c);
                            it.next();
                        }
                        s.parse().unwrap_or(u128::MAX)
                    };
                    match take_num(&mut ai).cmp(&take_num(&mut bi)) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
                match ca.cmp(&cb) {
                    std::cmp::Ordering::Equal => {
                        ai.next();
                        bi.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

fn collect_plot_outputs(dir: &Path) -> Vec<String> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
                    Some("svg") | Some("png")
                )
            })
            .map(|p| {
                let modified = std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (modified, p)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    // Oldest first, so figures appear in the order the script drew them.
    // Ties (several figures written in the same instant, which is the norm
    // -- they are all emitted at the end of one run) fall back to the name,
    // compared NUMERICALLY: plain lexicographic ordering puts
    // `..._10.svg` before `..._2.svg`, which would silently scramble the
    // numbering of any script producing ten or more figures.
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| natural_cmp(&a.1, &b.1)));

    files
        .into_iter()
        .filter_map(|(_, path)| {
            let bytes = std::fs::read(&path).ok()?;
            let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
                Some("png") => "image/png",
                _ => "image/svg+xml",
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Some(format!("data:{mime};base64,{encoded}"))
        })
        .collect()
}


/// One figure to write: a filename and its bytes as a `data:` URI.
#[derive(Debug, Clone, Deserialize)]
pub struct FigureFile {
    pub name: String,
    /// `data:<mime>;base64,<payload>` -- the same form the Figures panel
    /// already holds, so nothing has to be re-rendered to save it.
    pub data_uri: String,
}

/// Save a collection of figures to a directory.
///
/// The Figures panel can keep figures across runs, which is how you
/// compare a change against what you had. Until now that collection lived
/// only in the frontend's memory: the run directory each figure came from
/// is deleted as soon as its bytes are read, so closing the app lost
/// everything. A kept collection that cannot be kept is half a feature.
///
/// Writes are confined to one directory the caller names, filenames are
/// stripped to a bare name, and an existing file is never silently
/// overwritten -- a numeric suffix is added instead. Saving figures should
/// not be able to destroy anything.
#[tauri::command]
async fn save_figures(dir: String, files: Vec<FigureFile>) -> Result<Vec<String>, String> {
    let dir = PathBuf::from(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create `{}`: {e}", dir.display()))?;

    let mut written = Vec::new();
    for file in files {
        // Take only the final component, so a crafted name cannot escape
        // the directory the user chose.
        let stem = Path::new(&file.name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("figure")
            .to_string();
        let Some((_, payload)) = file.data_uri.split_once(";base64,") else {
            return Err(format!("`{stem}` is not a base64 data URI"));
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|e| format!("`{stem}` is not valid base64: {e}"))?;

        // Never clobber. A figure collection is usually saved more than
        // once, and the second save must not eat the first.
        let mut path = dir.join(&stem);
        if path.exists() {
            let (base, ext) = match stem.rsplit_once('.') {
                Some((b, e)) => (b.to_string(), format!(".{e}")),
                None => (stem.clone(), String::new()),
            };
            for n in 2..1000 {
                let candidate = dir.join(format!("{base}-{n}{ext}"));
                if !candidate.exists() {
                    path = candidate;
                    break;
                }
            }
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("could not write `{}`: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

#[tauri::command]
async fn execute_code(
    request: ExecuteRequest,
    _state: State<'_, AppState>,
) -> Result<ExecuteResponse, String> {
    let start = std::time::Instant::now();

    // Each run gets its own throwaway directory rather than one fixed
    // `qu_temp.qu` path -- besides being where `--emit-figure`'s output and
    // any `savefig(...)` file the script itself writes actually land for
    // `collect_plot_outputs` to find afterward, a shared fixed filename was
    // also a latent bug on its own: two overlapping "Run" clicks (or a
    // future concurrent-execution feature) would race to write/read/delete
    // the same `qu_temp.qu`.
    let run_dir = std::env::temp_dir().join(format!(
        "qu_studio_run_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(e) = std::fs::create_dir_all(&run_dir) {
        return Ok(ExecuteResponse {
            success: false,
            output: String::new(),
            error: Some(format!("Failed to create run directory: {}", e)),
            elapsed_ms: 0.0,
            plots: Vec::new(),
            variables: Vec::new(),
            data: Vec::new(),
        });
    }

    let temp_path = run_dir.join("script.qu");
    if let Err(e) = std::fs::write(&temp_path, &request.code) {
        let _ = std::fs::remove_dir_all(&run_dir);
        return Ok(ExecuteResponse {
            success: false,
            output: String::new(),
            error: Some(format!("Failed to write temp file: {}", e)),
            elapsed_ms: 0.0,
            plots: Vec::new(),
            variables: Vec::new(),
            data: Vec::new(),
        });
    }
    let emit_figure_path = run_dir.join(AUTO_FIGURE_NAME);
    let emit_vars_path = run_dir.join(AUTO_VARS_NAME);
    let emit_data_path = run_dir.join(AUTO_DATA_NAME);

    // Prefer the bundled sidecar; fall back to a dev-tree/PATH search so
    // `cargo tauri dev` keeps working without a sidecar rebuild. Both are
    // real `.await`s (see `run_command_async`'s own doc comment for why
    // that matters) rather than the blocking `.output()` calls this used
    // to chain via `.or_else`.
    let sidecar_result =
        run_qu_sidecar(&temp_path, &emit_figure_path, &emit_vars_path, &emit_data_path).await;
    let outcome = match sidecar_result {
        Ok(r) => Ok(r),
        Err(_) => {
            run_qu_fallback(&temp_path, &emit_figure_path, &emit_vars_path, &emit_data_path).await
        }
    };

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let plots = collect_plot_outputs(&run_dir);
    let variables = read_vars_output(&run_dir);
    let data = read_data_output(&run_dir);
    let _ = std::fs::remove_dir_all(&run_dir);

    match outcome {
        Ok((success, stdout, stderr)) => Ok(ExecuteResponse {
            success,
            output: format!("{}\n{}", stdout, stderr),
            error: if !success { Some(stderr) } else { None },
            elapsed_ms,
            plots,
            variables,
            data,
        }),
        Err(e) => Ok(ExecuteResponse {
            success: false,
            output: String::new(),
            error: Some(e),
            elapsed_ms,
            plots: Vec::new(),
            variables: Vec::new(),
            data: Vec::new(),
        }),
    }
}

/// Holds the currently-running live script's child process, if any --
/// see `run_live_start`'s own doc comment. A plain `Mutex<Option<...>>`
/// (not per-run-id) because only one live run makes sense at a time from
/// one editor: starting a new one always supersedes whatever was running
/// before, the same "newest wins" semantics `InteractiveModePanel.tsx`
/// already uses for its own debounced re-runs (there, superseded runs are
/// merely ignored on arrival since the whole run is cheap and short-lived;
/// here, a live run can be a genuine infinite loop, so the old process
/// must be actually killed, not just have its result discarded).
#[derive(Default)]
pub struct LiveRunState {
    child: Mutex<Option<tauri::api::process::CommandChild>>,
}

/// Starts a script in **live** mode (`qu run --live`, see that flag's own
/// doc comment in `qu-cli/src/main.rs`) — for a script that streams output
/// as it goes rather than running to completion and returning everything
/// at once, most importantly QuStudio's live serial-plotter feature (read
/// off a real port via `serial_open`/`read_bytes` in a loop, `print` each
/// sample). Each line the script prints arrives at the frontend as a
/// `"qu-live-line"` event the moment it's flushed (not batched), via the
/// same non-blocking `Command::spawn()` + `Receiver<CommandEvent>` stream
/// `run_command_async` uses for `execute_code` — except here the events
/// are forwarded to the frontend as they arrive instead of being collected
/// into one final `Output`, since a live run may never "finish" in the
/// `execute_code` sense at all (an intentionally infinite reading loop,
/// stopped by the user via `run_live_stop`, not by the script returning).
#[tauri::command]
async fn run_live_start(
    code: String,
    app: tauri::AppHandle,
    state: State<'_, LiveRunState>,
) -> Result<(), String> {
    use tauri::api::process::CommandEvent;
    use tauri::Manager;

    // Only one live run at a time -- starting a new one kills whatever was
    // already running, the same "starting a new thing supersedes the old
    // one" semantics as everywhere else a script can be (re-)run in this
    // app. A live run can be a genuine infinite loop, so this MUST
    // actually kill the process, not just drop our handle to it (which
    // would leak it running in the background forever).
    if let Some(old_child) = state.child.lock().unwrap().take() {
        let _ = old_child.kill();
    }

    let run_dir = std::env::temp_dir().join(format!(
        "qu_studio_live_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&run_dir).map_err(|e| format!("Failed to create run directory: {}", e))?;
    let temp_path = run_dir.join("live_script.qu");
    std::fs::write(&temp_path, &code).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let args = [
        "run".to_string(),
        temp_path.to_string_lossy().into_owned(),
        "--live".to_string(),
    ];
    // Prefer the bundled sidecar; fall back to a dev-tree/PATH search --
    // same reasoning and the same two candidates as `execute_code`'s own
    // `run_qu_sidecar`/`run_qu_fallback`, just not sharing that exact code
    // since a live run's args (`--live`, no `--emit-*` files) differ and
    // its lifecycle (spawn-and-stream-until-killed, not spawn-and-await)
    // is a genuinely different shape from `run_command_async`.
    let sidecar = tauri::api::process::Command::new_sidecar("qu").map(|c| c.args(args.clone()));
    let command = match sidecar {
        Ok(c) => c,
        Err(_) => {
            let program = find_qu_executable()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| QU_EXE_NAME.to_string());
            tauri::api::process::Command::new(program).args(args)
        }
    };

    let (mut rx, child) = command
        .spawn()
        .map_err(|e| format!("failed to spawn qu: {}", e))?;
    *state.child.lock().unwrap() = Some(child);

    // Streams events for the lifetime of the child process -- this task
    // outlives the `run_live_start` call itself (which returns as soon as
    // the process is spawned, not when it finishes), by design: the
    // frontend gets an immediate response confirming the run started, and
    // then a live stream of `"qu-live-line"`/`"qu-live-error"` events
    // until either the script actually finishes on its own or
    // `run_live_stop` kills it (which ends this loop by closing `rx`).
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let _ = app.emit_all("qu-live-line", line);
                }
                CommandEvent::Stderr(line) => {
                    let _ = app.emit_all("qu-live-error", line);
                }
                CommandEvent::Terminated(payload) => {
                    let _ = app.emit_all("qu-live-done", payload.code);
                }
                CommandEvent::Error(e) => {
                    let _ = app.emit_all("qu-live-error", e);
                }
                _ => {}
            }
        }
        let _ = std::fs::remove_dir_all(&run_dir);
    });

    Ok(())
}

/// Kills whatever script `run_live_start` has running, if any. A no-op
/// (not an error) when nothing is running -- mirrors `execute_code`'s own
/// "there's nothing to do" tolerance rather than making the frontend track
/// whether a live run is currently active just to avoid calling this.
#[tauri::command]
fn run_live_stop(state: State<LiveRunState>) -> Result<(), String> {
    if let Some(child) = state.child.lock().unwrap().take() {
        child.kill().map_err(|e| format!("failed to stop the live run: {}", e))?;
    }
    Ok(())
}

/// Inline diagnostics for CodeEditor.tsx's debounced-on-keystroke syntax
/// check. Pure parsing (`qu_syntax::parse`, linked directly into this
/// binary -- see the `qu-syntax` dependency's own Cargo.toml comment for
/// why that's safe/cheap to do in-process rather than shelling out like
/// `execute_code` does for `qu run`), no execution: fast enough (expected
/// milliseconds) to run on every debounced keystroke, unlike the `llm_*`
/// commands' real generation latency.
///
/// Mirrors `qu-cli`'s own `cmd_parse` (`main.rs`, the `qu parse`/`qu check`
/// invocation pattern this is based on) in calling `qu_syntax::parse`
/// directly, but reports a structured diagnostic instead of printing
/// `OK`/an error string to stdout. A clean parse returns an empty
/// `Vec` -- CodeEditor.tsx clears all markers when it sees one -- and a
/// parse error returns exactly one diagnostic built from that error's own
/// `ParseError { msg, span }`. This command itself always resolves `Ok`;
/// there's no failure mode in `qu_syntax::parse` (it never panics, per its
/// own module doc comment) that should surface as an `Err` to the
/// frontend rather than as a diagnostic.
#[tauri::command]
fn check_syntax(code: String) -> Result<Vec<Diagnostic>, String> {
    match qu_syntax::parse(&code) {
        Ok(_) => Ok(Vec::new()),
        Err(e) => Ok(vec![Diagnostic {
            message: e.msg,
            line: e.span.line,
            column: e.span.col,
        }]),
    }
}

#[tauri::command]
fn open_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to open file: {}", e))
}

#[tauri::command]
fn save_file(path: String, content: String) -> Result<(), String> {
    snapshot_before_overwrite(&path).map_err(|e| format!("Failed to save a version snapshot: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to save file: {}", e))
}

/// One saved file's version history sits in a sibling `.qu-versions/<file
/// name>/` folder, keyed by the moment it was superseded -- not a `.git`
/// repo, not a database, so it works the same whether or not the workspace
/// happens to be one. Kept OUT of `list_directory`'s results (filtered
/// there) and out of git (`.gitignore`), same reasoning as `.qu-versions`
/// itself: this is edit history, not project content.
const MAX_VERSIONS_PER_FILE: usize = 50;

fn versions_dir_for(path: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(path.parent()?.join(".qu-versions").join(path.file_name()?))
}

/// Copies the file's CURRENT on-disk content into its version history
/// before `save_file` overwrites it. A file that doesn't exist yet has
/// nothing to preserve -- `Ok(())`, not an error, so creating a new file
/// is never blocked by this.
fn snapshot_before_overwrite(path: &str) -> std::io::Result<()> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Ok(());
    }
    let existing = std::fs::read(path)?;
    let Some(dir) = versions_dir_for(path) else { return Ok(()) };
    std::fs::create_dir_all(&dir)?;
    // Millisecond timestamps collide for real: a caught bug, not a
    // hypothetical -- a tight loop of saves in a test landed 55 saves in
    // 19 distinct milliseconds, and each collision silently OVERWRITES an
    // earlier snapshot (same filename) rather than erroring, so it was
    // invisible without a test that actually counted survivors. Nanoseconds
    // narrow the window a great deal but Windows' clock resolution can
    // still be coarser than a save takes, so the timestamp is ALSO bumped
    // past any name already on disk -- the only way to guarantee a new
    // snapshot never silently replaces an old one, regardless of clock
    // granularity. Still exposed to the frontend as `timestamp` (the field
    // name), just nanoseconds rather than milliseconds now -- see
    // `VersionHistoryPanel.tsx`'s `relativeTime`, which knows this.
    let mut timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    while dir.join(format!("{timestamp}.qu")).exists() {
        timestamp += 1;
    }
    std::fs::write(dir.join(format!("{timestamp}.qu")), &existing)?;
    prune_old_versions(&dir)
}

/// Keeps only the newest `MAX_VERSIONS_PER_FILE` snapshots -- an
/// unbounded history for a file edited all day would otherwise grow
/// forever and nobody would notice until the disk did.
fn prune_old_versions(dir: &std::path::Path) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "qu"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    if entries.len() > MAX_VERSIONS_PER_FILE {
        for stale in &entries[..entries.len() - MAX_VERSIONS_PER_FILE] {
            let _ = std::fs::remove_file(stale.path());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct FileVersion {
    /// Nanoseconds since the Unix epoch (bumped past any collision -- see
    /// `snapshot_before_overwrite`), as a STRING -- also the snapshot's
    /// filename stem, so a version is addressed by this same value end to
    /// end. A `u64` here would round-trip through JSON as a JS `number`,
    /// which cannot represent a nanosecond epoch value exactly (it needs
    /// ~61 bits; `Number` only carries 53 safely) -- `read_file_version`
    /// would then be asked for a timestamp that never matches any real
    /// filename on disk, silently rounded by the frontend's own JSON
    /// parser. A string is exact both ways.
    pub timestamp: String,
    pub size: u64,
}

#[tauri::command]
fn list_file_versions(path: String) -> Result<Vec<FileVersion>, String> {
    let target = std::path::Path::new(&path);
    let Some(dir) = versions_dir_for(target) else { return Ok(Vec::new()) };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(stem) = name.strip_suffix(".qu") else { continue };
        // Parsed only to validate and to sort numerically (a string sort
        // would put "9" after "10") -- the stored/returned value is still
        // the original string, not a reformatted number.
        let Ok(numeric) = stem.parse::<u64>() else { continue };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        versions.push((numeric, FileVersion { timestamp: stem.to_string(), size }));
    }
    // Newest first -- that's the order a history panel reads naturally.
    versions.sort_by_key(|(numeric, _)| std::cmp::Reverse(*numeric));
    Ok(versions.into_iter().map(|(_, v)| v).collect())
}

#[tauri::command]
fn read_file_version(path: String, timestamp: String) -> Result<String, String> {
    if timestamp.is_empty() || !timestamp.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid version id".into());
    }
    let target = std::path::Path::new(&path);
    let dir = versions_dir_for(target).ok_or("This file has no version history")?;
    std::fs::read_to_string(dir.join(format!("{timestamp}.qu")))
        .map_err(|e| format!("Failed to read that version: {}", e))
}

/// What the editor believes is on disk for an open file, so an external
/// edit can be *detected* without a filesystem watcher.
///
/// Deliberately not `notify`-based: a watcher would add a crate, a
/// background thread and a stream of events per keystroke from whatever
/// tool is writing, and the question this app actually needs answered is
/// only ever asked at two moments -- when the window regains focus, and
/// on a slow poll while it has focus. A stat is cheap enough to just ask
/// then. `exists: false` is a normal answer (the file was deleted or
/// renamed out from under us), not an error, because the banner needs to
/// say so rather than swallow it.
#[derive(Debug, Clone, Serialize)]
struct FileStat {
    exists: bool,
    /// Milliseconds since the Unix epoch. `None` when the platform or
    /// filesystem does not report a modification time -- callers must
    /// fall back to `len`, never treat `None` as "unchanged".
    mtime_ms: Option<u64>,
    len: u64,
}

#[tauri::command]
fn file_stat(path: String) -> Result<FileStat, String> {
    match std::fs::metadata(&path) {
        Ok(m) => Ok(FileStat {
            exists: true,
            mtime_ms: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64),
            len: m.len(),
        }),
        // A missing file is a real, reportable state -- not a failure to
        // answer the question.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileStat {
            exists: false,
            mtime_ms: None,
            len: 0,
        }),
        Err(e) => Err(format!("Failed to stat file: {}", e)),
    }
}

#[tauri::command]
fn list_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let entries = std::fs::read_dir(&path)
        .map_err(|e| format!("Failed to read directory: {}", e))?;
    
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        // Version history, not project content -- never shown in the file
        // tree, same as it's excluded from git (see `.gitignore`).
        if name == ".qu-versions" {
            continue;
        }

        let is_dir = path.is_dir();
        let size = if !is_dir {
            path.metadata().ok().map(|m| m.len())
        } else {
            None
        };
        
        files.push(FileInfo {
            name,
            is_dir,
            size,
            path: path.to_string_lossy().to_string(),
        });
    }
    
    Ok(files)
}

#[tauri::command]
fn get_examples_dir(app: tauri::AppHandle) -> Result<String, String> {
    // Real bug found and fixed live (2026-09-04): this used to ONLY check
    // paths relative to the running exe (`exe_dir.join("catalog")`, or a
    // dev-tree-shaped `../../catalog` fallback) -- neither of which ever
    // exists for an actually-installed copy of Qu Studio, since `catalog/`
    // was never declared as a bundled resource (`tauri.bundle.resources`
    // was `[]`). The Catalog sidebar section therefore silently rendered
    // nothing for every real install (it degrades to an empty list rather
    // than an error -- see the frontend's own `catch` around this call),
    // which is exactly what got reported: "there is no 'demos' catalog".
    //
    // `path_resolver().resolve_resource(...)` -- NOT `resource_dir().join(...)`
    // -- is the correct way to find a bundled resource declared with a `..`
    // component, per Tauri's own doc comment on `App::resolve_resource`:
    // "Tauri replaces [a `..` component] with a parent folder, so simply
    // using resource_dir() and joining the path won't work." Confirmed by
    // inspecting a real built output: `tauri.conf.json`'s
    // `"../../catalog/**/*"` resource pattern actually lands at
    // `release/_up_/_up_/catalog/*.qu`, not `release/resources/catalog/`
    // as the more obvious (and, on a first pass here, wrong) reading of
    // resource_dir() would suggest -- resolve_resource handles that `_up_`
    // translation internally instead of needing it hardcoded here.
    if let Some(bundled) = app.path_resolver().resolve_resource("../../catalog") {
        if bundled.exists() {
            return Ok(bundled.to_string_lossy().to_string());
        }
    }

    // Get the directory where the executable is located
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed to get exe path: {}", e))?;

    let exe_dir = exe_path.parent()
        .ok_or("No parent directory")?;

    // Try to find catalog directory
    let examples_path = exe_dir.join("catalog");
    if examples_path.exists() {
        return Ok(examples_path.to_string_lossy().to_string());
    }

    // Fallback to workspace catalog
    let workspace_examples = exe_dir.join("../../catalog");
    if workspace_examples.exists() {
        return Ok(workspace_examples.to_string_lossy().to_string());
    }

    Err("Catalog directory not found".to_string())
}

// ============ Main Entry Point ============

fn main() {
    // Initialize logger
    env_logger::try_init().ok();

    // `LlmState` starts as an empty `Mutex<None>` regardless of build --
    // managing it here costs nothing at startup either way; the actual
    // ~669MB model load only happens lazily, inside `llm_chat`/
    // `llm_complete`'s own `get_or_load_model`, on whichever of those two
    // commands the user triggers first (mascot chat or inline
    // autocomplete), and ONLY in a build with `--features llm` -- a plain
    // build's `llm_chat`/`llm_complete` never touch `LlmState` at all, they
    // just return a friendly "not built with this feature" error (see
    // `llm_bridge.rs`). `tauri::generate_handler!` here is a plain
    // `macro_rules!`-style list with no support for per-item `#[cfg(...)]`
    // (confirmed: that syntax fails to parse) -- so both commands are
    // ALWAYS registered, with the feature gate living inside their own
    // function bodies instead of at this call site.
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(llm_bridge::LlmState::default())
        .manage(LiveRunState::default())
        .manage(gui_bridge::GuiState::default())
        .manage(seriplot::SeriPlotState::default())
        .manage(ReplState::default())
        .invoke_handler(tauri::generate_handler![
            greet,
            execute_code,
            repl_run,
            repl_restart,
            save_figures,
            check_syntax,
            open_file,
            save_file,
            list_file_versions,
            read_file_version,
            list_builtin_docs,
            list_directory,
            file_stat,
            get_examples_dir,
            run_live_start,
            gui_start,
            gui_event,
            gui_stop,
            gui_snapshot,
            seriplot_ports,
            seriplot_start,
            seriplot_stop,
            seriplot_poll,
            seriplot_record,
            seriplot_buffer,
            run_live_stop,
            llm_chat,
            llm_complete,
            llm_fix_error,
            llm_transform_code,
            get_llm_provider_config,
            set_llm_provider_config,
            test_llm_provider,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a throwaway directory tree under the OS temp dir and returns
    /// its path; caller is responsible for removing it.
    fn make_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qu_studio_test_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_qu_next_to_own_binary() {
        let dir = make_temp_dir("next_to_exe");
        let fake_exe = dir.join("qu-studio-fake-exe");
        std::fs::write(&fake_exe, b"").unwrap();
        let qu_path = dir.join(QU_EXE_NAME);
        std::fs::write(&qu_path, b"").unwrap();

        assert_eq!(find_qu_executable_near(&fake_exe), Some(qu_path));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_qu_in_sibling_engine_dev_tree() {
        // Mirrors the real repo layout: qu-studio-tauri/src-tauri/target/<profile>/<exe>
        // sitting next to engine/target/<profile>/qu(.exe).
        let repo_root = make_temp_dir("dev_tree");
        let studio_target_debug = repo_root
            .join("qu-studio-tauri")
            .join("src-tauri")
            .join("target")
            .join("debug");
        std::fs::create_dir_all(&studio_target_debug).unwrap();
        let fake_exe = studio_target_debug.join("qu-studio-fake-exe");
        std::fs::write(&fake_exe, b"").unwrap();

        let engine_target_debug = repo_root.join("engine").join("target").join("debug");
        std::fs::create_dir_all(&engine_target_debug).unwrap();
        let qu_path = engine_target_debug.join(QU_EXE_NAME);
        std::fs::write(&qu_path, b"").unwrap();

        let found = find_qu_executable_near(&fake_exe).expect("should find qu.exe in engine/target");
        // Compare canonicalized paths since the found one goes through `../../../../`.
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(&qu_path).unwrap()
        );

        std::fs::remove_dir_all(&repo_root).unwrap();
    }

    #[test]
    fn returns_none_when_not_found_anywhere() {
        let dir = make_temp_dir("not_found");
        let fake_exe = dir.join("qu-studio-fake-exe");
        std::fs::write(&fake_exe, b"").unwrap();

        // Sanity: nothing named QU_EXE_NAME exists near this fake exe, and we
        // don't touch the real PATH here, so this should only pass/fail based
        // on whether the near-exe candidates exist -- which they don't.
        let near_only = {
            if let Some(d) = fake_exe.parent() {
                d.join(QU_EXE_NAME).exists()
                    || d.join("../../../../engine/target/debug").join(QU_EXE_NAME).exists()
                    || d.join("../../../../engine/target/release").join(QU_EXE_NAME).exists()
            } else {
                false
            }
        };
        assert!(!near_only, "temp dir should not accidentally contain a qu executable");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ============ Sidecar invocation ============

    #[test]
    fn sidecar_invocation_runs_real_qu_binary_when_available() {
        // `run_qu_sidecar` calls `tauri::api::process::Command::new_sidecar`,
        // which resolves to `<dir of current_exe()>/qu.exe` (see
        // `relative_command_path` inside the tauri crate). In a real
        // packaged app that directory is where `tauri-build`'s
        // `externalBin` copy step (driven by `build.rs`) places the sidecar
        // on every build. Inside `cargo test`, `current_exe()` is the test
        // binary under `target/<profile>/deps/`, which that copy step never
        // populates -- so to exercise the *exact* code path `execute_code`
        // uses (not a mock of it), this test stages a real, already-built
        // `qu.exe` next to the *test binary's* own exe and runs it there.
        //
        // Honesty note on what this does and doesn't prove: this confirms
        // `new_sidecar("qu")` + `.args([...])` + `.output()` correctly
        // spawns a real `qu` binary and captures its stdout/exit status --
        // i.e. the command-construction and result-plumbing logic is wired
        // right. It does NOT exercise the Tauri IPC layer, the "Run" button,
        // or a real packaged `cargo tauri build` output, none of which can
        // be driven headlessly in this environment. That the sidecar is
        // correctly *placed* by a real build was verified separately by
        // running `cargo build` (with `CARGO_TARGET_DIR` pointed outside
        // the Dropbox-synced repo, to avoid the sync-lock issue) and
        // confirming `qu.exe` landed next to `qu-studio.exe` in
        // `target/debug/`.
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            manifest_dir.join("../../engine/target/release/qu.exe"),
            manifest_dir.join("../../engine/target/debug/qu.exe"),
        ];
        let Some(source_qu) = candidates.iter().find(|p| p.exists()) else {
            eprintln!(
                "skipping sidecar_invocation_runs_real_qu_binary_when_available: \
                 no prebuilt qu.exe found under engine/target/{{release,debug}}; \
                 run `cargo build --release --manifest-path engine/Cargo.toml -p qu-cli` first"
            );
            return;
        };

        let test_exe = std::env::current_exe().unwrap();
        let sidecar_dest = test_exe.parent().unwrap().join(QU_EXE_NAME);
        // Don't clobber (or later delete) a copy some other concurrent test
        // run may have staged there.
        let we_staged_it = !sidecar_dest.exists();
        if we_staged_it {
            std::fs::copy(source_qu, &sidecar_dest)
                .expect("failed to stage qu.exe next to the test binary");
        }

        let script_dir = make_temp_dir("sidecar_script");
        let script_path = script_dir.join("hello.qu");
        std::fs::write(&script_path, "print(\"{1 + 1}\")").unwrap();
        let emit_figure_path = script_dir.join("unused_figure.svg");
        let emit_vars_path = script_dir.join("unused_vars.json");
        let emit_data_path = script_dir.join("unused_data.json");

        // `run_qu_sidecar` is `async fn` (see its own doc comment for why:
        // the real bug this session found and fixed was `execute_code`
        // blocking the whole UI thread via a synchronous `.output()` call)
        // -- `block_on` drives it to completion here since a plain `#[test]`
        // fn isn't itself async.
        let result = tauri::async_runtime::block_on(run_qu_sidecar(
            &script_path,
            &emit_figure_path,
            &emit_vars_path,
            &emit_data_path,
        ));

        if we_staged_it {
            let _ = std::fs::remove_file(&sidecar_dest);
        }
        std::fs::remove_dir_all(&script_dir).unwrap();

        let (success, stdout, stderr): (bool, String, String) =
            result.expect("run_qu_sidecar should successfully spawn the staged qu.exe");
        assert!(success, "qu run should exit successfully; stderr: {stderr}");
        assert!(
            stdout.contains('2'),
            "expected qu's output to contain the computed value 2, got: {stdout:?}"
        );
    }

    // ============ Inline diagnostics (`check_syntax`) ============

    #[test]
    fn check_syntax_returns_no_diagnostics_for_valid_code() {
        let diagnostics = check_syntax("x = 1 + 2\nprint(x)".to_string())
            .expect("check_syntax should not itself error on valid input");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn check_syntax_reports_a_diagnostic_with_line_and_column_for_a_missing_end() {
        // `if` opened on line 2 with no matching `end` -- a classic case the
        // inline-diagnostics feature exists to catch as the user types.
        let src = "x = 1\nif x > 0\n  y = 2\n";
        let diagnostics = check_syntax(src.to_string())
            .expect("check_syntax should not itself error on a parse failure");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].line >= 1);
        assert!(diagnostics[0].column >= 1);
        assert!(!diagnostics[0].message.is_empty());
    }

    // ============ "Open Folder" flow: direct command-function tests ============
    //
    // These call the `list_directory` / `open_file` / `save_file` command
    // functions directly (bypassing the Tauri IPC/window layer entirely,
    // which cannot be exercised headlessly here). They stand in for the
    // frontend loop: pick folder -> list_directory populates the tree ->
    // click a .qu file -> open_file loads its content.

    #[test]
    fn list_directory_reports_files_and_subfolders() {
        let dir = make_temp_dir("list_dir");
        std::fs::write(dir.join("script.qu"), b"print 1").unwrap();
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        std::fs::create_dir_all(dir.join("subfolder")).unwrap();

        let mut entries = list_directory(dir.to_string_lossy().to_string())
            .expect("list_directory should succeed on a real directory");
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(entries.len(), 3);

        let script = entries.iter().find(|e| e.name == "script.qu").unwrap();
        assert!(!script.is_dir);
        assert_eq!(script.size, Some(7)); // "print 1" is 7 bytes

        let notes = entries.iter().find(|e| e.name == "notes.txt").unwrap();
        assert!(!notes.is_dir);

        let sub = entries.iter().find(|e| e.name == "subfolder").unwrap();
        assert!(sub.is_dir);
        assert_eq!(sub.size, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_directory_errors_on_missing_path() {
        let dir = make_temp_dir("list_dir_missing");
        let missing = dir.join("does-not-exist");

        let result = list_directory(missing.to_string_lossy().to_string());
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // `file_stat` is what the file-changed banner polls with. The two
    // cases that matter to it are "the file moved on" and "the file is
    // gone" -- and the second must NOT come back as an error, or the
    // banner cannot tell "deleted" apart from "the stat call broke".
    #[test]
    fn file_stat_reports_size_and_a_moving_mtime() {
        let dir = make_temp_dir("file_stat");
        let path = dir.join("watched.qu");
        std::fs::write(&path, b"a = 1").unwrap();

        let first = file_stat(path.to_string_lossy().to_string())
            .expect("file_stat should succeed on a real file");
        assert!(first.exists);
        assert_eq!(first.len, 5); // "a = 1"
        let first_mtime = first.mtime_ms.expect("a real file has a mtime here");

        // Write different CONTENT of a different LENGTH, so the change is
        // detectable through `len` alone even on a filesystem whose mtime
        // resolution is too coarse to have ticked yet -- which is exactly
        // the fallback the banner relies on.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&path, b"a = 1 # edited elsewhere").unwrap();

        let second = file_stat(path.to_string_lossy().to_string()).unwrap();
        assert!(second.exists);
        assert_eq!(second.len, 24);
        assert!(
            second.mtime_ms.unwrap() > first_mtime,
            "mtime should advance after a rewrite: {:?} then {:?}",
            first_mtime,
            second.mtime_ms
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_stat_reports_a_deleted_file_as_absent_not_an_error() {
        let dir = make_temp_dir("file_stat_gone");
        let path = dir.join("vanishes.qu");
        std::fs::write(&path, b"x = 1").unwrap();
        assert!(file_stat(path.to_string_lossy().to_string()).unwrap().exists);

        std::fs::remove_file(&path).unwrap();

        let stat = file_stat(path.to_string_lossy().to_string())
            .expect("a missing file is a state to report, not a failure");
        assert!(!stat.exists);
        assert_eq!(stat.mtime_ms, None);
        assert_eq!(stat.len, 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_file_reads_back_written_content() {
        let dir = make_temp_dir("open_file");
        let file_path = dir.join("script.qu");
        std::fs::write(&file_path, "x = 1\nprint x").unwrap();

        let content = open_file(file_path.to_string_lossy().to_string())
            .expect("open_file should read an existing file");
        assert_eq!(content, "x = 1\nprint x");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_file_errors_on_missing_file() {
        let dir = make_temp_dir("open_file_missing");
        let missing = dir.join("nope.qu");

        let result = open_file(missing.to_string_lossy().to_string());
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_a_new_file_creates_no_version_history() {
        // Nothing existed before this save -- there is nothing to snapshot,
        // and `.qu-versions/` should not even be created.
        let dir = make_temp_dir("versions_new_file");
        let file_path = dir.join("fresh.qu");

        save_file(file_path.to_string_lossy().to_string(), "x = 1".to_string())
            .expect("save_file should create a new file");

        let versions = list_file_versions(file_path.to_string_lossy().to_string())
            .expect("list_file_versions should not error on a file with no history");
        assert!(versions.is_empty());
        assert!(!dir.join(".qu-versions").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resaving_a_file_snapshots_the_previous_content_and_it_is_readable_back() {
        let dir = make_temp_dir("versions_resave");
        let file_path = dir.join("script.qu");

        save_file(file_path.to_string_lossy().to_string(), "x = 1".to_string()).unwrap();
        save_file(file_path.to_string_lossy().to_string(), "x = 2".to_string()).unwrap();
        save_file(file_path.to_string_lossy().to_string(), "x = 3".to_string()).unwrap();

        let versions = list_file_versions(file_path.to_string_lossy().to_string())
            .expect("list_file_versions should succeed once history exists");
        // Two resaves -> two prior versions preserved (the third save's
        // own content is the CURRENT file, not a version of itself).
        assert_eq!(versions.len(), 2);
        // Newest first -- compared numerically, not lexicographically:
        // both ids happen to be the same digit-width today, but the test
        // shouldn't rely on that coincidence to mean the same thing as
        // "sorted".
        assert!(versions[0].timestamp.parse::<u64>().unwrap() >= versions[1].timestamp.parse::<u64>().unwrap());

        let oldest = versions.last().unwrap().timestamp.clone();
        let content = read_file_version(file_path.to_string_lossy().to_string(), oldest)
            .expect("read_file_version should read a real snapshot back");
        assert_eq!(content, "x = 1");

        // `.qu-versions/` must not appear in the current directory listing --
        // it is history, not project content.
        let listing = list_directory(dir.to_string_lossy().to_string()).unwrap();
        assert!(listing.iter().all(|f| f.name != ".qu-versions"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn version_history_is_pruned_beyond_the_cap() {
        let dir = make_temp_dir("versions_pruned");
        let file_path = dir.join("churn.qu");
        let versions_dir = dir.join(".qu-versions").join("churn.qu");

        save_file(file_path.to_string_lossy().to_string(), "n = 0".to_string()).unwrap();
        for n in 1..=(MAX_VERSIONS_PER_FILE + 5) {
            save_file(file_path.to_string_lossy().to_string(), format!("n = {n}")).unwrap();
        }

        let versions = list_file_versions(file_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(versions.len(), MAX_VERSIONS_PER_FILE);
        // Confirms pruning removed files on disk, not just what's reported.
        let on_disk = std::fs::read_dir(&versions_dir).unwrap().count();
        assert_eq!(on_disk, MAX_VERSIONS_PER_FILE);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reading_an_unknown_version_errors_instead_of_panicking() {
        let dir = make_temp_dir("versions_missing");
        let file_path = dir.join("script.qu");
        save_file(file_path.to_string_lossy().to_string(), "x = 1".to_string()).unwrap();

        let result = read_file_version(file_path.to_string_lossy().to_string(), "1".to_string());
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn full_open_folder_loop_list_then_open_matches_saved_content() {
        // Mirrors the actual frontend loop end-to-end at the command level:
        // list_directory(folder) -> pick the .qu file -> open_file(path)
        // -> content matches what save_file previously wrote.
        let dir = make_temp_dir("full_loop");
        let file_path = dir.join("analysis.qu");
        save_file(file_path.to_string_lossy().to_string(), "y = 2 * 21".to_string())
            .expect("save_file should write the file");

        let entries = list_directory(dir.to_string_lossy().to_string())
            .expect("list_directory should see the saved file");
        let qu_file = entries
            .iter()
            .find(|e| e.name.ends_with(".qu"))
            .expect(".qu file should appear in the listing");

        let content = open_file(qu_file.path.clone())
            .expect("open_file should load the file found via list_directory");
        assert_eq!(content, "y = 2 * 21");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
