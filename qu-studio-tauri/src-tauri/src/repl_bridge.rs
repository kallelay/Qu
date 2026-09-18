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
//! **One interpreter per SESSION, not per tab.** QuStudio's frontend
//! (`qu-studio-tauri/src/App.tsx`) keeps multiple open files as tabs in ONE
//! editor state, but there is exactly one Run button and one Variables/
//! Figures panel pair for the whole window -- there is no per-tab
//! execution context anywhere in the existing UI (the Interactive-Mode
//! panel, the Variables panel, the Figures panel are all singletons keyed
//! off "whatever `execute_code`/now `repl_run` last returned", not off a
//! tab id). Per-tab interpreters would need per-tab Variables/Figures
//! panels and a tab-switch story for what "Restart" even means, none of
//! which exists today -- so per-SESSION (one `qu kernel` child process for
//! the whole Tauri backend, in `ReplState`, exactly parallel to
//! `LiveRunState`'s existing single-`Mutex<Option<...>>` shape) is the
//! simpler, defensible choice: it matches how almost every Studio user
//! actually works (one primary file at a time) and how the surrounding UI
//! is already built. A user who wants a second independent session can
//! open a second QuStudio window (a second OS process, hence a second
//! `ReplState`).
//!
//! **Restart.** `repl_restart` kills the live child outright (rather than
//! sending it a `{"op":"restart"}` request and trusting the still-running
//! process to reset itself) so a restart can never be foiled by a wedged
//! or partially-hung interpreter -- the next `repl_run` lazily spawns a
//! completely fresh `qu kernel` process, which is as close to a guaranteed
//! clean slate as this design can get.
//!
//! **Locking.** `ReplState.inner` is a plain `std::sync::Mutex`, not an
//! async-aware one -- deliberately: every call `.take()`s the kernel out
//! (leaving `None` behind) and drops the guard BEFORE doing anything that
//! awaits (the write + the response read), then re-inserts it when done.
//! The lock itself is therefore never held across an `.await` point. This
//! app has exactly one Run button and one Restart action for one session,
//! so two calls racing to `.take()` the same kernel at once is not a
//! realistic concern in practice; the frontend also disables Run while a
//! request is in flight (see `App.tsx`).

use crate::{find_qu_executable, ExecuteResponse, PlotVar, VariableInfo, QU_EXE_NAME};
use base64::Engine as _;
use std::sync::Mutex;
use tauri::api::process::{CommandChild, CommandEvent};
use tauri::async_runtime::Receiver;
use tauri::State;

struct ReplKernel {
    child: CommandChild,
    rx: Receiver<CommandEvent>,
}

/// Holds the live `qu kernel` child process for this QuStudio session, if
/// one has been spawned yet -- see this module's own doc comment for why
/// one per session (not per tab) and why a plain `std::sync::Mutex`.
#[derive(Default)]
pub struct ReplState {
    inner: Mutex<Option<ReplKernel>>,
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
                return serde_json::from_str(&resp_line).map_err(|e| {
                    format!("bad response from `qu kernel`: {} (line was: {:?})", e, resp_line)
                });
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
pub async fn repl_run(code: String, state: State<'_, ReplState>) -> Result<ExecuteResponse, String> {
    let start = std::time::Instant::now();

    let mut kernel = state.inner.lock().unwrap().take();
    if kernel.is_none() {
        kernel = Some(spawn_kernel()?);
    }
    let mut kernel = kernel.expect("just ensured Some above");

    let request = serde_json::json!({ "op": "run", "code": code });
    let result = kernel_request(&mut kernel, &request).await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(resp) => {
            // Success -- hand the (still-alive) kernel back for the next call.
            *state.inner.lock().unwrap() = Some(kernel);
            Ok(parse_run_response(resp, elapsed_ms))
        }
        Err(e) => {
            // Transport failure: the kernel is unusable, so it is
            // deliberately NOT put back -- `state.inner` stays `None`, and
            // the next `repl_run` spawns a fresh process from scratch.
            let _ = kernel.child.kill();
            Ok(ExecuteResponse {
                success: false,
                output: String::new(),
                error: Some(e),
                elapsed_ms,
                plots: Vec::new(),
                variables: Vec::new(),
                data: Vec::new(),
            })
        }
    }
}

/// Kills this session's live `qu kernel` process, if any, discarding all
/// its state. A no-op (not an error) when nothing is running yet -- mirrors
/// `run_live_stop`'s own "nothing to do" tolerance. The next `repl_run`
/// lazily spawns a brand new process, so the effect a caller actually
/// observes is: every variable/function/figure defined so far is gone, and
/// the next Run starts from a completely clean interpreter.
#[tauri::command]
pub fn repl_restart(state: State<ReplState>) -> Result<(), String> {
    if let Some(kernel) = state.inner.lock().unwrap().take() {
        let _ = kernel.child.kill();
    }
    Ok(())
}
