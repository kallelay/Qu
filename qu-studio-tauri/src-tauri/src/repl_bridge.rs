//! Persistent-interpreter bridge for QuStudio's Code editor "Run" button.
//!
//! `execute_code` (see `main.rs`) writes the whole buffer to a fresh temp
//! file and spawns a brand-new one-shot `qu run <file>` process on every
//! click -- no state survives between runs, which is exactly right for
//! `qu run`/`qu diary` (real scripts/CI depend on that one-shot semantics
//! staying untouched) but wrong for an editor "Run" button, where a user
//! expects the same MATLAB/Jupyter-style persistence a notebook cell gives:
//! define `x` in one Run, use it in the next.
//!
//! This module is a SEPARATE, additive path next to `execute_code`: it
//! spawns `qu kernel` (see `qu-cli/src/main.rs`'s `cmd_kernel` doc comment
//! for the full line-delimited-JSON protocol) and keeps that ONE process
//! alive across `repl_run` calls, using its own `ReplState` rather than
//! touching `AppState`/`LiveRunState`. Nothing here changes `execute_code`
//! itself or any surface that still calls it (DSP Workbench, etc. -- see
//! this crate's `main.rs` `invoke_handler` list, where `execute_code`
//! remains registered unmodified).
//!
//! **Sessions.** Every call names a session key (`session`, default
//! `"shared"`), and each key gets its own `qu kernel` process. The frontend
//! decides the key from its "REPL: shared / per file" setting: shared mode
//! sends one key for the whole window (the original behaviour -- define `x`
//! in one file's Run, use it from another or from the terminal prompt);
//! per-file mode sends the tab's path or id, so each file runs in its own
//! interpreter and the terminal prompt talks to the active file's one.
//!
//! **Restart.** `repl_restart` kills the live child outright (rather than
//! sending it a `{"op":"restart"}` request and trusting the still-running
//! process to reset itself) so a restart can never be foiled by a wedged
//! or partially-hung interpreter -- the next `repl_run` lazily spawns a
//! completely fresh `qu kernel` process, which is as close to a guaranteed
//! clean slate as this design can get.
//!
//! **Locking.** `ReplState.inner` is a plain `std::sync::Mutex`, not an
//! async-aware one -- deliberately: every call removes its session's kernel
//! from the map and drops the guard BEFORE doing anything that awaits (the
//! write + the response read), then re-inserts it when done. The lock is
//! therefore never held across an `.await` point. Two calls racing for the
//! SAME session would find it missing and spawn a second kernel, so the
//! frontend allows one request per session at a time: Run and the terminal
//! prompt are both disabled while either is in flight (see `App.tsx`).

use crate::{find_qu_executable, ExecuteResponse, PlotVar, VariableInfo, QU_EXE_NAME};
use base64::Engine as _;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tauri::api::process::{CommandChild, CommandEvent};
use tauri::async_runtime::Receiver;
use tauri::{Manager, State};

struct ReplKernel {
    child: CommandChild,
    rx: Receiver<CommandEvent>,
}

/// Holds the live `qu kernel` child process for this QuStudio session, if
/// one has been spawned yet -- see this module's own doc comment for why
/// one per session (not per tab) and why a plain `std::sync::Mutex`.
#[derive(Default)]
pub struct ReplState {
    inner: Mutex<HashMap<String, ReplKernel>>,
    /// Process id of each session's kernel WHILE a run is in flight. The
    /// kernel itself is out of `inner` for that time (see the locking note
    /// in the module doc), so `repl_stop` could not reach it there; this is
    /// how it finds the process to end.
    running: Mutex<HashMap<String, u32>>,
    /// Sessions whose run `repl_stop` ended on purpose, so the run's
    /// "the kernel exited" failure is reported as a stop, not an error.
    stopped: Mutex<HashSet<String>>,
}

/// The session a call without one belongs to: everything, in shared mode.
const SHARED_SESSION: &str = "shared";

fn session_key(session: Option<String>) -> String {
    session.filter(|s| !s.is_empty()).unwrap_or_else(|| SHARED_SESSION.to_string())
}

/// Spawns a fresh `qu kernel` child process -- same sidecar-then-PATH-
/// fallback search `execute_code`'s own `run_qu_sidecar`/`run_qu_fallback`
/// use, just with `kernel` instead of `run <file> --emit-*` as the argv.
fn spawn_kernel() -> Result<ReplKernel, String> {
    let sidecar = tauri::api::process::Command::new_sidecar("qu")
        .map(|c| c.args(["kernel".to_string()]));
    let command = match sidecar {
        Ok(c) => c,
        Err(_) => {
            let program = find_qu_executable()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| QU_EXE_NAME.to_string());
            tauri::api::process::Command::new(program).args(["kernel".to_string()])
        }
    };
    let (rx, child) = command
        .spawn()
        .map_err(|e| format!("failed to spawn `qu kernel`: {}", e))?;
    Ok(ReplKernel { child, rx })
}

/// Sends one request object to `kernel`'s stdin (as a single JSON line) and
/// waits for its one JSON response line back -- see `cmd_kernel`'s doc
/// comment in `qu-cli` for why exactly one response line always follows
/// one request line. `Stderr` lines (e.g. a Rust panic message, which goes
/// to stderr not the JSON protocol) are logged and otherwise ignored while
/// still waiting for the real response; `Terminated`/`Error`/a closed
/// stream are reported as an `Err` so the caller can drop the dead kernel
/// and let the next call respawn a clean one.
async fn kernel_request(
    kernel: &mut ReplKernel,
    request: &serde_json::Value,
    live: Option<(&tauri::AppHandle, &str)>,
) -> Result<serde_json::Value, String> {
    let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
    line.push('\n');
    kernel
        .child
        .write(line.as_bytes())
        .map_err(|e| format!("failed to write to `qu kernel`: {}", e))?;

    loop {
        match kernel.rx.recv().await {
            Some(CommandEvent::Stdout(resp_line)) => {
                let v: serde_json::Value = serde_json::from_str(&resp_line).map_err(|e| {
                    format!("bad response from `qu kernel`: {} (line was: {:?})", e, resp_line)
                })?;
                // A streamed run sends its output as `{"op":"out","text":..}`
                // lines before the one real response. Each goes to the window
                // now -- but whatever is already queued behind it is gathered
                // into the SAME event, so a script that prints thousands of
                // lines a second costs a handful of events, not thousands
                // (one per line froze the window).
                if v.get("op").and_then(|o| o.as_str()) == Some("out") {
                    let mut text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    let mut ended: Option<Result<serde_json::Value, String>> = None;
                    loop {
                        match kernel.rx.try_recv() {
                            Ok(CommandEvent::Stdout(l)) => match serde_json::from_str::<serde_json::Value>(&l) {
                                Ok(n) if n.get("op").and_then(|o| o.as_str()) == Some("out") => {
                                    text.push_str(n.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                                }
                                Ok(n) => {
                                    ended = Some(Ok(n));
                                    break;
                                }
                                Err(e) => {
                                    ended = Some(Err(format!("bad response from `qu kernel`: {} (line was: {:?})", e, l)));
                                    break;
                                }
                            },
                            Ok(CommandEvent::Stderr(err_line)) => {
                                eprintln!("qu-studio: qu kernel stderr: {}", err_line);
                            }
                            Ok(CommandEvent::Error(e)) => {
                                ended = Some(Err(format!("`qu kernel` process error: {}", e)));
                                break;
                            }
                            Ok(CommandEvent::Terminated(payload)) => {
                                ended = Some(Err(format!(
                                    "`qu kernel` process exited unexpectedly (code {:?})",
                                    payload.code
                                )));
                                break;
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                    if let Some((app, session)) = live {
                        if !text.is_empty() {
                            let _ = app.emit_all(
                                "qu-run-output",
                                serde_json::json!({"session": session, "text": text}),
                            );
                        }
                    }
                    match ended {
                        Some(r) => return r,
                        None => continue,
                    }
                }
                return Ok(v);
            }
            Some(CommandEvent::Stderr(err_line)) => {
                eprintln!("qu-studio: qu kernel stderr: {}", err_line);
            }
            Some(CommandEvent::Error(e)) => {
                return Err(format!("`qu kernel` process error: {}", e));
            }
            Some(CommandEvent::Terminated(payload)) => {
                return Err(format!(
                    "`qu kernel` process exited unexpectedly (code {:?})",
                    payload.code
                ));
            }
            Some(_) => {}
            None => return Err("`qu kernel` process's output stream closed".to_string()),
        }
    }
}

fn svg_to_data_uri(svg: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
    format!("data:image/svg+xml;base64,{}", encoded)
}

/// Converts a `{"op":"run",...}` response (see `cmd_kernel`'s doc comment
/// for the exact shape) into the same `ExecuteResponse` the frontend
/// already knows how to render from `execute_code` -- so `App.tsx` needs no
/// new response type, just a different Tauri command to call.
fn parse_run_response(v: serde_json::Value, elapsed_ms: f64) -> ExecuteResponse {
    let success = v.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
    let output = v
        .get("output")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let error = v
        .get("error")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let plots = v
        .get("plots")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str()).map(svg_to_data_uri).collect())
        .unwrap_or_default();
    let variables: Vec<VariableInfo> = v
        .get("variables")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(VariableInfo {
                        name: item.get("name")?.as_str()?.to_string(),
                        kind: item.get("type")?.as_str()?.to_string(),
                        value: item.get("preview")?.as_str()?.to_string(),
                        system: item.get("system").and_then(|b| b.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let data: Vec<PlotVar> = v
        .get("data")
        .cloned()
        .and_then(|d| serde_json::from_value(d).ok())
        .unwrap_or_default();
    ExecuteResponse {
        success,
        output,
        error,
        elapsed_ms,
        plots,
        variables,
        data,
        // The persistent REPL kernel has no scratch run directory to scan
        // (`collect_report_outputs` needs one) -- reports are a one-shot
        // `qu run` artifact, not something the kernel protocol emits.
        // Interactive Mode's Run button has no report-print surface yet.
        reports: Vec::new(),
    }
}

/// Runs `code` against this session's persistent `qu kernel` interpreter --
/// the Code editor Run button's Tauri entry point, replacing `execute_code`
/// for that one surface. Spawns the kernel lazily on the first call, and
/// keeps it alive (state included) for every call after, until
/// `repl_restart` kills it or the whole Tauri process exits.
///
/// On any transport-level failure (bad JSON back, the process died, etc.)
/// the dead kernel is dropped and the failure is reported as an ordinary
/// unsuccessful `ExecuteResponse` rather than a hard command error -- the
/// SAME degrade-gracefully convention `execute_code` already uses, so the
/// frontend's existing error-rendering path (`error: Some(..)`) keeps
/// working unchanged. The next Run attempt transparently spawns a fresh
/// kernel (losing session state, same as an explicit Restart would) since
/// there is nothing left worth keeping once the transport itself is
/// broken.
#[tauri::command]
pub async fn repl_run(
    app: tauri::AppHandle,
    code: String,
    session: Option<String>,
    cwd: Option<String>,
    file: Option<String>,
    state: State<'_, ReplState>,
) -> Result<ExecuteResponse, String> {
    let start = std::time::Instant::now();
    let key = session_key(session);

    let mut kernel = state.inner.lock().unwrap().remove(&key);
    if kernel.is_none() {
        kernel = Some(spawn_kernel()?);
    }
    let mut kernel = kernel.expect("just ensured Some above");

    // `"stream": true`: output reaches the window while the script runs
    // (`qu-run-output` events), so a long or endless script is not silent.
    // `cwd`/`file`: the folder and name of the file being run, so relative
    // paths in the script resolve next to it and a trace names the file.
    let request = serde_json::json!({
        "op": "run", "code": code, "stream": true,
        "cwd": cwd.unwrap_or_default(), "file": file.unwrap_or_default(),
    });
    state.running.lock().unwrap().insert(key.clone(), kernel.child.pid());
    let result = kernel_request(&mut kernel, &request, Some((&app, key.as_str()))).await;
    state.running.lock().unwrap().remove(&key);
    let was_stopped = state.stopped.lock().unwrap().remove(&key);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(resp) => {
            // Success -- hand the (still-alive) kernel back for the next call.
            state.inner.lock().unwrap().insert(key, kernel);
            Ok(parse_run_response(resp, elapsed_ms))
        }
        Err(e) => {
            // Transport failure: the kernel is unusable, so it is
            // deliberately NOT put back, and the next `repl_run` for this
            // session spawns a fresh process from scratch.
            let _ = kernel.child.kill();
            Ok(ExecuteResponse {
                success: false,
                output: String::new(),
                error: Some(if was_stopped {
                    "Stopped. The interpreter was ended, so variables from earlier runs are gone.".to_string()
                } else {
                    e
                }),
                elapsed_ms,
                plots: Vec::new(),
                variables: Vec::new(),
                data: Vec::new(),
                reports: Vec::new(),
            })
        }
    }
}

/// Ends the run that is in flight for `session`, if any, by ending its
/// kernel process (an interpreter stuck in a loop cannot be asked nicely).
/// The pending `repl_run` then returns a "Stopped" response, and the next
/// Run starts a fresh interpreter. A no-op when nothing is running.
#[tauri::command]
pub fn repl_stop(session: Option<String>, state: State<ReplState>) -> Result<bool, String> {
    let key = session_key(session);
    let pid = state.running.lock().unwrap().get(&key).copied();
    let Some(pid) = pid else { return Ok(false) };
    state.stopped.lock().unwrap().insert(key);
    // The pid is the kernel this module itself spawned for the run.
    // Windows: end it directly with the API (no helper process, no tree
    // walk -- `taskkill /T` took over five seconds from inside the app).
    #[cfg(windows)]
    let status: std::io::Result<std::process::ExitStatus> = {
        use std::os::windows::process::ExitStatusExt;
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn TerminateProcess(handle: *mut std::ffi::c_void, code: u32) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        const PROCESS_TERMINATE: u32 = 0x0001;
        // Safety: plain Win32 calls on a handle opened here and closed here.
        let ok = unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                false
            } else {
                let r = TerminateProcess(h, 1) != 0;
                CloseHandle(h);
                r
            }
        };
        Ok(std::process::ExitStatus::from_raw(if ok { 0 } else { 1 }))
    };
    #[cfg(not(windows))]
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    match status {
        Ok(s) if s.success() => Ok(true),
        Ok(s) => Err(format!("could not end the running script (exit {s})")),
        Err(e) => Err(format!("could not end the running script: {e}")),
    }
}

/// Kills this session's live `qu kernel` process, if any, discarding all
/// its state. A no-op (not an error) when nothing is running yet -- mirrors
/// `run_live_stop`'s own "nothing to do" tolerance. The next `repl_run`
/// lazily spawns a brand new process, so the effect a caller actually
/// observes is: every variable/function/figure defined so far is gone, and
/// the next Run starts from a completely clean interpreter.
#[tauri::command]
pub fn repl_restart(session: Option<String>, state: State<ReplState>) -> Result<(), String> {
    if let Some(kernel) = state.inner.lock().unwrap().remove(&session_key(session)) {
        let _ = kernel.child.kill();
    }
    Ok(())
}

/// Ends a session for good (its tab was closed): same as `repl_restart`,
/// named for what the caller means.
#[tauri::command]
pub fn repl_close(session: String, state: State<ReplState>) -> Result<(), String> {
    repl_restart(Some(session), state)
}
