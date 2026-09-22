//! The kernel's socket set and message dispatch. One `qu_interp::Interp`
//! lives for the whole process lifetime (mirrors `qu-cli`'s own REPL: see
//! `Interp::run_repl_line`'s doc comment) so a notebook's later cells see
//! earlier cells' variables — that persistence is the entire reason a real
//! kernel process exists instead of `qu run`-ing a temp file per cell (which
//! is what QuStudio's own cell UI does today, and explicitly documents as a
//! deliberate simplification it accepted, not a limitation of Qu itself).

use crate::connection::ConnectionInfo;
use crate::protocol::{Header, Message};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{json, Value as Json};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;
use zeromq::{PubSocket, RepSocket, RouterSocket, Socket, SocketEvent, SocketRecv, SocketSend};

type IoPub = Arc<Mutex<PubSocket>>;

pub async fn run(conn: ConnectionInfo) -> Result<(), String> {
    let key = conn.key.as_bytes().to_vec();
    let session = uuid::Uuid::new_v4().to_string();

    let mut shell = RouterSocket::new();
    shell.bind(&conn.endpoint(conn.shell_port)).await.map_err(|e| e.to_string())?;
    let mut control = RouterSocket::new();
    control.bind(&conn.endpoint(conn.control_port)).await.map_err(|e| e.to_string())?;
    let mut stdin_sock = RouterSocket::new();
    stdin_sock.bind(&conn.endpoint(conn.stdin_port)).await.map_err(|e| e.to_string())?;
    let mut hb = RepSocket::new();
    hb.bind(&conn.endpoint(conn.hb_port)).await.map_err(|e| e.to_string())?;
    let mut iopub_sock = PubSocket::new();
    iopub_sock.bind(&conn.endpoint(conn.iopub_port)).await.map_err(|e| e.to_string())?;
    let iopub: IoPub = Arc::new(Mutex::new(iopub_sock));

    eprintln!("qu-jupyter: bound shell={} iopub={} on {}", conn.shell_port, conn.iopub_port, conn.ip);

    // Heartbeat: Jupyter's liveness check is "echo whatever you were sent,"
    // nothing more — a REP socket is exactly that shape already.
    tokio::spawn(async move {
        loop {
            match hb.recv().await {
                Ok(msg) => {
                    if hb.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // stdin: backs `read_input()` (see qu-interp's `on_input` hook). A
    // frontend's stdin DEALER never sends anything unprompted — the kernel
    // always speaks first with `input_request` — so there is no message to
    // learn its ROUTER peer identity from (the identity `RouterSocket::recv`
    // would normally hand us). `monitor()`'s `SocketEvent::Accepted` gives
    // it to us at CONNECT time instead, which is why this needs its own
    // small tracking task rather than just waiting for a `recv()`.
    let mut stdin_events = stdin_sock.monitor();
    let last_stdin_identity: Arc<StdMutex<Option<Bytes>>> = Arc::new(StdMutex::new(None));
    {
        let last_stdin_identity = last_stdin_identity.clone();
        tokio::spawn(async move {
            while let Some(event) = stdin_events.next().await {
                if let SocketEvent::Accepted(_, peer_id) = event {
                    *last_stdin_identity.lock().unwrap() = Some(peer_id.into());
                }
            }
        });
    }

    let (input_req_tx, mut input_req_rx) = tokio::sync::mpsc::unbounded_channel::<InputRequest>();
    {
        let session = session.clone();
        let key = key.clone();
        tokio::spawn(async move {
            while let Some(req) = input_req_rx.recv().await {
                let identity = last_stdin_identity.lock().unwrap().clone();
                let Some(identity) = identity else {
                    eprintln!("qu-jupyter: read_input() called but no frontend is connected on stdin");
                    let _ = req.reply_tx.send(String::new());
                    continue;
                };
                let out = Message {
                    identities: vec![identity],
                    header: Header::new(&session, "input_request"),
                    parent_header: serde_json::to_value(&req.parent).unwrap_or(Json::Null),
                    metadata: json!({}),
                    content: json!({"prompt": req.prompt, "password": false}),
                    buffers: vec![],
                };
                if stdin_sock.send(out.into_zmq(&key)).await.is_err() {
                    let _ = req.reply_tx.send(String::new());
                    continue;
                }
                // Ignore anything that isn't the reply we're waiting for
                // (there shouldn't be anything else on this channel, but a
                // stray/duplicate message must not desynchronize the next
                // real request).
                loop {
                    match stdin_sock.recv().await {
                        Ok(raw) => match Message::parse(raw, &key) {
                            Ok(reply) if reply.header.msg_type == "input_reply" => {
                                let value =
                                    reply.content.get("value").and_then(Json::as_str).unwrap_or("").to_string();
                                let _ = req.reply_tx.send(value);
                                break;
                            }
                            _ => continue,
                        },
                        Err(_) => {
                            let _ = req.reply_tx.send(String::new());
                            break;
                        }
                    }
                }
            }
        });
    }

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Shared with the `Interp` itself (`qu_interp::Interp::interrupt`,
    // checked once per statement in `exec_block`) so `interrupt_request` on
    // the control channel can actually stop a running cell, not just
    // acknowledge the request — see that field's own doc comment for why
    // this is safe to check that cheaply.
    let interrupt = Arc::new(AtomicBool::new(false));

    // Control channel runs as its own task specifically so shutdown_request
    // (and now interrupt_request) can be answered even while the shell task
    // is deep inside a long `execute_request` (see `handle_execute`'s doc
    // comment for why that call is on its own blocking thread, not this
    // task).
    {
        let iopub = iopub.clone();
        let interrupt = interrupt.clone();
        let key = key.clone();
        let session = session.clone();
        tokio::spawn(async move {
            let mut shutdown_tx = Some(shutdown_tx);
            loop {
                let raw = match control.recv().await {
                    Ok(m) => m,
                    Err(_) => break,
                };
                let msg = match Message::parse(raw, &key) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("qu-jupyter: dropping malformed control message: {e}");
                        continue;
                    }
                };
                match msg.header.msg_type.as_str() {
                    "kernel_info_request" => {
                        send_reply(&mut control, &msg, &session, &key, "kernel_info_reply", kernel_info_content())
                            .await;
                    }
                    "shutdown_request" => {
                        let restart =
                            msg.content.get("restart").and_then(Json::as_bool).unwrap_or(false);
                        send_reply(
                            &mut control,
                            &msg,
                            &session,
                            &key,
                            "shutdown_reply",
                            json!({"restart": restart}),
                        )
                        .await;
                        if let Some(tx) = shutdown_tx.take() {
                            let _ = tx.send(());
                        }
                    }
                    "interrupt_request" => {
                        // Checked once per statement by `exec_block`
                        // (`qu_interp::Interp::interrupt`) — reset to
                        // `false` before each cell in `handle_execute`, so
                        // this can't leak into stopping some LATER cell
                        // that hasn't started yet.
                        interrupt.store(true, AtomicOrdering::Relaxed);
                        send_reply(&mut control, &msg, &session, &key, "interrupt_reply", json!({})).await;
                        let _ = &iopub; // reserved: future interrupt-status broadcast
                    }
                    _ => {}
                }
            }
        });
    }

    // Real kernels broadcast this once at boot so a frontend's IOPub
    // subscriber has *something* to see immediately. It can still be lost
    // to PUB/SUB's slow-joiner race (sent before the frontend's SUB socket
    // finishes connecting) — `dispatch`'s own busy/idle-per-request wrapper
    // is what actually guarantees `wait_for_ready` unblocks even then, by
    // giving every retried `kernel_info_request` its own fresh iopub pair.
    send_iopub(&iopub, "status", None, json!({"execution_state": "starting"}), &session, &key).await;

    let mut interp = qu_interp::Interp::new();
    interp.interrupt = Some(interrupt.clone());
    let mut kernel = KernelState {
        interp: Some(interp),
        execution_count: 0,
        iopub,
        session,
        key,
        interrupt,
        input_req_tx,
    };

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => {
                eprintln!("qu-jupyter: shutdown requested, exiting");
                break;
            }
            raw = shell.recv() => {
                let raw = match raw {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("qu-jupyter: shell socket closed: {e}");
                        break;
                    }
                };
                let msg = match Message::parse(raw, &kernel.key) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("qu-jupyter: dropping malformed shell message: {e}");
                        continue;
                    }
                };
                if kernel.dispatch(&mut shell, msg).await == Control::Shutdown {
                    break;
                }
            }
        }
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum Control {
    Continue,
    Shutdown,
}

struct KernelState {
    /// `Option` so a cell's execution can `take()` it into a blocking
    /// worker thread (see `handle_execute`) and put it back afterward —
    /// `Interp` itself has no async awareness, so it must never be held
    /// across an `.await` while a synchronous call into it is in flight.
    interp: Option<qu_interp::Interp>,
    execution_count: u32,
    iopub: IoPub,
    session: String,
    key: Vec<u8>,
    /// Shared with the `Interp`'s own `interrupt` field; see where it's
    /// constructed in `run` for why it lives outside `Interp` too (the
    /// control-channel task that flips it never touches `Interp` itself).
    interrupt: Arc<AtomicBool>,
    /// Channel into the stdin-handling task spawned in `run`; see
    /// `InputRequest`'s own doc comment.
    input_req_tx: tokio::sync::mpsc::UnboundedSender<InputRequest>,
}

/// One `read_input(...)` call's round trip, sent from the cell's blocking
/// worker thread (see `handle_execute`) to the async task that owns the
/// stdin socket. `reply_tx` is a plain `std::sync::mpsc::Sender` (not
/// tokio's) because the receiving end — inside the synchronous `on_input`
/// closure, itself inside `Interp::run_repl_line`, on a `spawn_blocking`
/// thread — blocks on it with an ordinary synchronous `recv()`, which is
/// exactly what that channel type is for and none of what an async mutex
/// or tokio channel would want to be awaited from that context.
struct InputRequest {
    prompt: String,
    parent: Header,
    reply_tx: std::sync::mpsc::Sender<String>,
}

impl KernelState {
    /// Every shell request gets a `busy`/`idle` iopub status pair around it
    /// — required by the messaging spec for any request, not just
    /// `execute_request` (`handle_execute` publishes its own richer status
    /// history *inside* that window: `execute_input`, `stream`,
    /// `display_data`/`execute_result`, all between this `busy` and
    /// `idle`). Beyond spec compliance, this is what makes
    /// `wait_for_ready()` work at all: a frontend's IOPub SUB socket can
    /// join *after* the kernel has already sent its one-time startup
    /// status, so `wait_for_ready` treats "no iopub traffic yet" as
    /// ambiguous and keeps retrying `kernel_info_request` until it
    /// observes *any* iopub message — which, without this wrapper, would
    /// never come until the notebook's first cell runs.
    async fn dispatch(&mut self, shell: &mut RouterSocket, msg: Message) -> Control {
        send_status(&self.iopub, "busy", &msg.header, &self.session, &self.key).await;
        let control = self.dispatch_inner(shell, &msg).await;
        send_status(&self.iopub, "idle", &msg.header, &self.session, &self.key).await;
        control
    }

    async fn dispatch_inner(&mut self, shell: &mut RouterSocket, msg: &Message) -> Control {
        match msg.header.msg_type.as_str() {
            "kernel_info_request" => {
                send_reply(shell, msg, &self.session, &self.key, "kernel_info_reply", kernel_info_content())
                    .await;
            }
            "execute_request" => self.handle_execute(shell, msg).await,
            "is_complete_request" => self.handle_is_complete(shell, msg).await,
            "complete_request" => self.handle_complete(shell, msg).await,
            "history_request" => {
                send_reply(shell, msg, &self.session, &self.key, "history_reply", json!({"history": []}))
                    .await;
            }
            "comm_info_request" => {
                send_reply(shell, msg, &self.session, &self.key, "comm_info_reply", json!({"comms": {}}))
                    .await;
            }
            "shutdown_request" => {
                let restart = msg.content.get("restart").and_then(Json::as_bool).unwrap_or(false);
                send_reply(shell, msg, &self.session, &self.key, "shutdown_reply", json!({"restart": restart}))
                    .await;
                return Control::Shutdown;
            }
            other => {
                eprintln!("qu-jupyter: ignoring unsupported shell message type `{other}`");
            }
        }
        Control::Continue
    }

    async fn handle_is_complete(&mut self, shell: &mut RouterSocket, msg: &Message) {
        let code = msg.content.get("code").and_then(Json::as_str).unwrap_or("");
        let content = if code.trim().is_empty() || qu_interp::input_looks_complete(code) {
            json!({"status": "complete"})
        } else {
            json!({"status": "incomplete", "indent": "  "})
        };
        send_reply(shell, msg, &self.session, &self.key, "is_complete_reply", content).await;
    }

    async fn handle_complete(&mut self, shell: &mut RouterSocket, msg: &Message) {
        let code = msg.content.get("code").and_then(Json::as_str).unwrap_or("");
        let cursor =
            (msg.content.get("cursor_pos").and_then(Json::as_u64).unwrap_or(code.len() as u64) as usize)
                .min(code.len());
        let prefix_start = code[..cursor]
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &code[prefix_start..cursor];
        let matches: Vec<String> = self
            .interp
            .as_ref()
            .map(|it| {
                it.global_bindings()
                    .map(|(name, _)| name.to_string())
                    .filter(|name| name.starts_with(prefix))
                    .collect()
            })
            .unwrap_or_default();
        send_reply(
            shell,
            msg,
            &self.session,
            &self.key,
            "complete_reply",
            json!({
                "matches": matches,
                "cursor_start": prefix_start,
                "cursor_end": cursor,
                "metadata": {},
                "status": "ok",
            }),
        )
        .await;
    }

    /// Runs one cell. Two things drive the shape of this function:
    ///
    /// 1. `Interp::run_repl_line` is a plain synchronous call with no yield
    ///    points, and a Qu script can run arbitrarily long (or loop
    ///    forever). Calling it directly on this task would block the whole
    ///    async runtime thread — including this kernel's own heartbeat and
    ///    control-channel handling on a single-threaded runtime, and at
    ///    minimum this task's own ability to notice a shutdown. So the
    ///    actual call happens on a `spawn_blocking` worker thread, and
    ///    `self.interp` is temporarily `take()`n to hand it over (see the
    ///    field's own doc comment).
    /// 2. Printed output should appear in the notebook as it happens, not
    ///    only after the whole cell finishes (a `for`-loop with `print`
    ///    inside it should stream, the way it does in a real terminal REPL).
    ///    `Interp::on_print` (`qu-interp`'s own hook, added for QuStudio's
    ///    live-run panel) fires synchronously from the worker thread; its
    ///    closure forwards each chunk over a `tokio::sync::mpsc` unbounded
    ///    channel, whose `send` is a plain non-async call and so is safe to
    ///    call from inside that synchronous callback. A second task drains
    ///    the channel and publishes each chunk as an iopub `stream` message
    ///    while the cell is still running.
    async fn handle_execute(&mut self, shell: &mut RouterSocket, msg: &Message) {
        let code = msg.content.get("code").and_then(Json::as_str).unwrap_or("").to_string();
        let silent = msg.content.get("silent").and_then(Json::as_bool).unwrap_or(false);

        if !silent {
            self.execution_count += 1;
        }
        let execution_count = self.execution_count;

        // Clear any interrupt left over from a PRIOR cell (e.g. one that
        // arrived just as that cell finished on its own) before this one
        // starts — otherwise it would immediately kill a cell it was never
        // meant for.
        self.interrupt.store(false, AtomicOrdering::Relaxed);

        // `dispatch` already sent the enclosing `busy` status for this
        // request; this is the richer per-cell timeline that goes inside it.
        send_iopub(
            &self.iopub,
            "execute_input",
            Some(&msg.header),
            json!({"code": code, "execution_count": execution_count}),
            &self.session,
            &self.key,
        )
        .await;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let stream_iopub = self.iopub.clone();
        let stream_parent = msg.header.clone();
        let stream_session = self.session.clone();
        let stream_key = self.key.clone();
        let stream_task = tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                send_iopub(
                    &stream_iopub,
                    "stream",
                    Some(&stream_parent),
                    json!({"name": "stdout", "text": chunk}),
                    &stream_session,
                    &stream_key,
                )
                .await;
            }
        });

        let input_req_tx = self.input_req_tx.clone();
        let input_parent = msg.header.clone();
        let mut interp = self.interp.take().expect("qu-jupyter: interp missing between cells");
        let (interp, run_result, fig_svgs, echo) = tokio::task::spawn_blocking(move || {
            let fig_hist_before = interp.figure_history.len();
            let live_svg_before = live_figure_svg(&interp);

            interp.on_print = Some(Box::new(move |s: &str| {
                let _ = tx.send(s.to_string());
            }));
            interp.on_input = Some(Box::new(move |prompt: &str| {
                let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                let req = InputRequest { prompt: prompt.to_string(), parent: input_parent.clone(), reply_tx };
                if input_req_tx.send(req).is_err() {
                    return String::new();
                }
                reply_rx.recv().unwrap_or_default()
            }));
            let run_result = interp.run_repl_line(&code);
            interp.on_print = None; // drops the sender, which is what lets `stream_task` finish
            interp.on_input = None;

            let fig_svgs = cell_figures(&interp, fig_hist_before, live_svg_before.as_deref());

            // Qu's own REPL echoes a bare expression (bound to `ans`) or an
            // un-suppressed assignment as `name = <value>` — kept identical
            // here rather than adopting Python/Jupyter's "only a trailing
            // bare expression echoes" convention, so a script behaves the
            // same whether it's typed at `qu repl` or run as a notebook
            // cell. See `run_repl_line`'s own doc comment.
            let echo = match &run_result {
                Ok(Some((name, value))) => Some(format!("{name} = {}", qu_interp::display_value(value))),
                _ => None,
            };

            (interp, run_result, fig_svgs, echo)
        })
        .await
        .expect("qu-jupyter: interpreter worker thread panicked");
        self.interp = Some(interp);

        // `stream_task`'s sender was dropped inside the closure above
        // (`on_print = None`); awaiting it here guarantees every buffered
        // chunk has actually been published before execute_reply goes out,
        // so a frontend never sees the reply before the cell's own output.
        let _ = stream_task.await;

        match run_result {
            Ok(_) => {
                for svg in &fig_svgs {
                    send_iopub(
                        &self.iopub,
                        "display_data",
                        Some(&msg.header),
                        json!({
                            "data": {"image/svg+xml": svg, "text/plain": "<Qu figure>"},
                            "metadata": {},
                            "transient": {},
                        }),
                        &self.session,
                        &self.key,
                    )
                    .await;
                }
                if let Some(text) = &echo {
                    send_iopub(
                        &self.iopub,
                        "execute_result",
                        Some(&msg.header),
                        json!({
                            "execution_count": execution_count,
                            "data": {"text/plain": text},
                            "metadata": {},
                        }),
                        &self.session,
                        &self.key,
                    )
                    .await;
                }
                send_reply(
                    shell,
                    msg,
                    &self.session,
                    &self.key,
                    "execute_reply",
                    json!({
                        "status": "ok",
                        "execution_count": execution_count,
                        "user_expressions": {},
                        "payload": [],
                    }),
                )
                .await;
            }
            Err(e) => {
                let evalue = e.to_string();
                // `exec_block`'s interrupt check raises exactly this message
                // (see `qu_interp::Interp::interrupt`'s doc comment) — named
                // `KeyboardInterrupt` here to match the convention every
                // Jupyter frontend already recognizes (dims/greys the
                // traceback differently from an ordinary error).
                let ename = if evalue.ends_with("interrupted") { "KeyboardInterrupt" } else { "QuError" };
                send_iopub(
                    &self.iopub,
                    "error",
                    Some(&msg.header),
                    json!({"ename": ename, "evalue": evalue, "traceback": [evalue.clone()]}),
                    &self.session,
                    &self.key,
                )
                .await;
                send_reply(
                    shell,
                    msg,
                    &self.session,
                    &self.key,
                    "execute_reply",
                    json!({
                        "status": "error",
                        "execution_count": execution_count,
                        "ename": ename,
                        "evalue": evalue,
                        "traceback": [evalue],
                    }),
                )
                .await;
            }
        }
    }
}

fn kernel_info_content() -> Json {
    json!({
        "status": "ok",
        "protocol_version": crate::protocol::PROTOCOL_VERSION,
        "implementation": "qu-jupyter",
        "implementation_version": env!("CARGO_PKG_VERSION"),
        "language_info": {
            "name": "qu",
            "version": env!("CARGO_PKG_VERSION"),
            "mimetype": "text/x-qu",
            "file_extension": ".qu",
            "pygments_lexer": "text",
            "codemirror_mode": "text",
        },
        "banner": format!("Qu {} — Jupyter kernel (engine/crates/qu-jupyter)", env!("CARGO_PKG_VERSION")),
        "help_links": [],
    })
}

async fn send_status(iopub: &IoPub, state: &str, parent: &Header, session: &str, key: &[u8]) {
    send_iopub(iopub, "status", Some(parent), json!({"execution_state": state}), session, key).await;
}

/// Publishes on iopub (PUB — no reply expected). The topic frame is the
/// `msg_type`, matching real kernels' convention; most frontends subscribe
/// to everything (`""`) so this is belt-and-suspenders rather than load-
/// bearing, but it costs nothing and helps a picky subscriber.
///
/// `parent` is `None` only for the one-time startup `status: starting`
/// broadcast, which isn't a reply to anything a frontend sent.
async fn send_iopub(
    iopub: &IoPub,
    msg_type: &str,
    parent: Option<&Header>,
    content: Json,
    session: &str,
    key: &[u8],
) {
    let out = Message {
        identities: vec![Bytes::copy_from_slice(msg_type.as_bytes())],
        header: Header::new(session, msg_type),
        parent_header: parent.and_then(|p| serde_json::to_value(p).ok()).unwrap_or(json!({})),
        metadata: json!({}),
        content,
        buffers: vec![],
    };
    let zmsg = out.into_zmq(key);
    let mut sock = iopub.lock().await;
    if let Err(e) = sock.send(zmsg).await {
        eprintln!("qu-jupyter: iopub send failed: {e}");
    }
}

/// Replies on a ROUTER socket (shell/control), routed back to whichever
/// peer's identity frame(s) `msg` carried in.
async fn send_reply(
    socket: &mut RouterSocket,
    msg: &Message,
    session: &str,
    key: &[u8],
    msg_type: &str,
    content: Json,
) {
    let out = msg.reply(session, msg_type, content);
    let zmsg = out.into_zmq(key);
    if let Err(e) = socket.send(zmsg).await {
        eprintln!("qu-jupyter: reply send failed ({msg_type}): {e}");
    }
}

/// SVG of the still-live (not yet `show`n/finalized) figure, or `None` if
/// nothing has been drawn into it. Pulled out to a free function purely so
/// `handle_execute` can snapshot it both before and after a cell runs with
/// the same rendering call.
fn live_figure_svg(interp: &qu_interp::Interp) -> Option<String> {
    (!interp.figure.is_pristine()).then(|| {
        qu_interp::plotting::render_svg(&interp.figure, interp.figure.width, interp.figure.height, interp.figure.publication)
    })
}

/// Every figure a cell should show as `display_data`: every figure newly
/// finalized into `figure_history` during the cell (via `show()`/`figure()`),
/// plus the still-live figure IF this cell actually changed it.
///
/// `qu run --emit-figure`'s own algorithm (main.rs) only checks
/// `!figure.is_pristine()` because it runs once at the very end of a whole
/// script — "non-pristine" there correctly means "something was drawn,
/// ever." Reused verbatim per-cell, that check is wrong: once any earlier
/// cell draws into the live figure, it stays non-pristine forever, so
/// every *later* cell — even one that never touches plotting — would
/// re-emit that same stale figure as its own display_data (confirmed live:
/// a cell doing nothing but `x = 5` after an earlier `plot(...)` showed the
/// old plot again). The fix is to compare rendered SVG before/after and
/// only emit it when THIS cell actually changed it — a render, not a cheap
/// flag, because there's no mutation counter on the live figure to compare
/// instead.
fn cell_figures(interp: &qu_interp::Interp, fig_hist_before: usize, live_svg_before: Option<&str>) -> Vec<String> {
    let mut fig_svgs: Vec<String> = interp.figure_history[fig_hist_before..]
        .iter()
        .map(|fig| qu_interp::plotting::render_svg(fig, fig.width, fig.height, fig.publication))
        .collect();
    if let Some(live_svg_after) = live_figure_svg(interp) {
        if live_svg_before != Some(live_svg_after.as_str()) {
            fig_svgs.push(live_svg_after);
        }
    }
    fig_svgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_flag_stops_an_infinite_loop_mid_run() {
        let flag = Arc::new(AtomicBool::new(false));
        let mut it = qu_interp::Interp::new();
        it.interrupt = Some(flag.clone());

        let flag_for_thread = flag.clone();
        let flipper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            flag_for_thread.store(true, AtomicOrdering::Relaxed);
        });

        let result = it.run_repl_line("n = 0\nwhile true\n  n = n + 1\nend");
        flipper.join().unwrap();

        let err = result.expect_err("an infinite loop must be stopped by the interrupt flag, not run forever");
        assert!(err.to_string().ends_with("interrupted"), "unexpected error: {err}");

        // The loop actually ran (and was actually stopped mid-flight, not
        // rejected before starting) — `n` should be some large positive
        // count, not 0 and not still incrementing.
        let n = it.global_bindings().find(|(name, _)| *name == "n").map(|(_, v)| v.clone());
        match n {
            Some(qu_interp::Value::Num(n)) => assert!(n > 0.0, "loop should have run at least once: n={n}"),
            other => panic!("expected n to be a positive number, got {other:?}"),
        }
    }

    /// Runs `code` against `it` the same way `handle_execute` does (minus
    /// the async/streaming machinery, which `cell_figures`/`live_figure_svg`
    /// don't touch), and returns the figures that cell would have shown.
    fn run_and_collect_figures(it: &mut qu_interp::Interp, code: &str) -> Vec<String> {
        let fig_hist_before = it.figure_history.len();
        let live_svg_before = live_figure_svg(it);
        it.run_repl_line(code).expect("test script should run cleanly");
        cell_figures(it, fig_hist_before, live_svg_before.as_deref())
    }

    #[test]
    fn a_later_unrelated_cell_does_not_reshow_an_earlier_plot() {
        let mut it = qu_interp::Interp::new();
        let figs = run_and_collect_figures(&mut it, "t = 0 to 9\nplot(t, t)\nxlabel(\"t\")");
        assert_eq!(figs.len(), 1, "the plotting cell itself should show exactly one figure");

        let figs = run_and_collect_figures(&mut it, "x = 5");
        assert!(figs.is_empty(), "an unrelated cell must not re-show the earlier plot: {figs:?}");

        let figs = run_and_collect_figures(&mut it, "x");
        assert!(figs.is_empty(), "a bare-expression cell must not re-show the earlier plot: {figs:?}");
    }

    #[test]
    fn a_cell_that_further_modifies_the_live_figure_reshows_the_updated_one() {
        let mut it = qu_interp::Interp::new();
        let first = run_and_collect_figures(&mut it, "t = 0 to 9\nplot(t, t)");
        assert_eq!(first.len(), 1);

        let second = run_and_collect_figures(&mut it, "ylabel(\"y\")");
        assert_eq!(second.len(), 1, "a cell that changes the still-live figure should re-show it");
        assert_ne!(first[0], second[0], "the re-shown figure should reflect the new ylabel, not be a duplicate");

        let third = run_and_collect_figures(&mut it, "y = 1");
        assert!(third.is_empty(), "and a further unrelated cell should go back to showing nothing");
    }

    #[test]
    fn a_cell_with_no_plot_at_all_shows_nothing() {
        let mut it = qu_interp::Interp::new();
        let figs = run_and_collect_figures(&mut it, "x = 5");
        assert!(figs.is_empty());
    }
}
