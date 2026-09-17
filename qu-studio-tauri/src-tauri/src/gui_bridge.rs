use std::sync::Mutex;
use tauri::{Manager, State};
use tauri::api::process::{Command, CommandChild, CommandEvent};

/// One live `qu gui` child and the last thing it told us.
///
/// `last_packet` exists because of a delivery race the native runner
/// window introduced (2026-09-16): `qu gui` prints its ENTIRE node tree
/// as a single packet the instant the script finishes running, and then
/// says nothing further until an event arrives on its stdin. `gui_start`
/// broadcasts that packet over `qu-gui` immediately -- which is before
/// the runner window that `GuiPanel` opens on the very next line has
/// loaded `index.html`, booted React and registered its own listener.
/// The window therefore missed the only packet it was ever going to get,
/// and since a blank window offers no widget to click, no event could be
/// generated to produce a second one: permanently empty, forever. Caching
/// the packet lets that window ask for it (`gui_snapshot`) once its
/// listener is up. Packets are full snapshots, so re-delivery is
/// idempotent and the two paths cannot disagree.
pub struct Session {
    id: String,
    child: CommandChild,
    last_packet: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct GuiState(Mutex<Option<Session>>);
impl Drop for GuiState {
    fn drop(&mut self) { if let Ok(state) = self.0.get_mut() { if let Some(session) = state.take() { let _ = session.child.kill(); } } }
}
#[tauri::command]
pub async fn gui_start(id: String, code: String, app: tauri::AppHandle, state: State<'_, GuiState>) -> Result<(), String> {
    if id.is_empty() || code.len() > 2_000_000 { return Err("Invalid GUI session or script too large".into()); }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(old) = guard.take() { let _ = old.child.kill(); }
    let path = std::env::temp_dir().join(format!("qu_gui_{}_{}.qu", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::write(&path, code).map_err(|error| format!("Could not write the GUI script to {}: {error}", path.display()))?;
    let args = vec!["gui".to_owned(), path.to_string_lossy().into_owned()];
    // Keep the engine's location for the failure message: a bare
    // `error.to_string()` here reports Windows spawn failures as just
    // "Access is denied. (os error 5)", which names neither the binary it
    // could not start nor anything actionable -- the one field report of
    // this said only "error 5", and that was the whole of what the user
    // could see.
    let (command, source) = if cfg!(debug_assertions) {
        let exe = crate::find_qu_executable().ok_or("Build the Qu CLI before starting a GUI")?;
        (Command::new(exe.to_string_lossy().into_owned()).args(args), exe.display().to_string())
    } else { (Command::new_sidecar("qu").map_err(|e| e.to_string())?.args(args), "the bundled qu sidecar".to_owned()) };
    let (mut receiver, child) = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            let denied = error.to_string().contains("os error 5");
            let hint = if denied { " — access denied usually means the binary is locked by a rebuild in progress, quarantined by antivirus, or blocked by Windows after being copied; rebuild or unblock it, then retry" } else { "" };
            return Err(format!("Could not start {source}: {error}{hint}"));
        }
    };
    *guard = Some(Session { id: id.clone(), child, last_packet: None });
    // Drop the lock before the reader task starts competing for it: the
    // task caches every packet into this same mutex (see `Session`).
    drop(guard);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let message = match event {
                CommandEvent::Stdout(line) => match serde_json::from_str::<serde_json::Value>(&line) {
                    Ok(packet) => {
                        // Cache BEFORE emitting, so a `gui_snapshot` racing
                        // the broadcast can only ever be too early (the
                        // caller's own listener then catches the live one),
                        // never too late.
                        if let Some(state) = app.try_state::<GuiState>() {
                            if let Ok(mut guard) = state.0.lock() {
                                if let Some(session) = guard.as_mut().filter(|session| session.id == id) {
                                    session.last_packet = Some(packet.clone());
                                }
                            }
                        }
                        serde_json::json!({"id":id,"packet":packet})
                    }
                    Err(_) => serde_json::json!({"id":id,"error":"Invalid GUI protocol output. Rebuild the Qu CLI."}),
                },
                CommandEvent::Stderr(error) | CommandEvent::Error(error) => serde_json::json!({"id":id,"error":error}),
                CommandEvent::Terminated(_) => { let _ = app.emit_all("qu-gui", serde_json::json!({"id":id,"done":true})); break; },
                _ => continue,
            };
            let _ = app.emit_all("qu-gui", message);
        }
        let _ = std::fs::remove_file(path);
    });
    Ok(())
}

/// The current session's most recent packet, for a host window whose
/// `qu-gui` listener came up after that packet was already broadcast.
/// See `Session`'s comment for why that is the normal case, not an edge
/// one. `None` means the session is over, unknown, or has not printed
/// yet -- in all three the caller's live listener is the right fallback.
#[tauri::command]
pub async fn gui_snapshot(id: String, state: State<'_, GuiState>) -> Result<Option<serde_json::Value>, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    Ok(guard.as_ref().filter(|session| session.id == id).and_then(|session| session.last_packet.clone()))
}
#[tauri::command]
pub async fn gui_event(id: String, target: String, event: String, value: serde_json::Value, state: State<'_, GuiState>) -> Result<(), String> {
    let bytes = format!("{}\n", serde_json::json!({"target":target,"event":event,"value":value})).into_bytes();
    if bytes.len() > 1_000_000 { return Err("GUI event is too large".into()); }
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().filter(|session| session.id == id).ok_or("GUI session ended")?;
    session.child.write(&bytes).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn gui_stop(id: String, state: State<'_, GuiState>) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    if guard.as_ref().is_some_and(|session| session.id == id) {
        if let Some(session) = guard.take() { session.child.kill().map_err(|e| e.to_string())?; }
    }
    Ok(())
}
