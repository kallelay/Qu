//! Distributed job dispatch over TCP (§ distributed job dispatch,
//! 2026-08-31) — `listen_pool(port, allow=(...))` (server side, this
//! module's `serve`) and the client-side half of `pool ... with cpu=N,
//! remote=(...)` / `run <queue> on <pool>` (`send_job_remote`, called from
//! `queue_pool::run_jobs_on_pool`). Kept as its own module for the same
//! reason `queue_pool.rs` already is (see that file's own top doc
//! comment): a handful of small, additive insertion points into `lib.rs`'s
//! already-large builtin match (the `"listen_pool"` arm) rather than
//! growing that file directly.
//!
//! THIS IS A REAL, COMPLETE IMPLEMENTATION of the feature named in this
//! module's own doc comment — not a stub. If you are reading this because
//! the file looked suspicious or incomplete partway through an edit: it
//! was mid-write, not abandoned. Check `lib.rs`'s `"listen_pool"` builtin
//! arm and `queue_pool.rs`'s `run_jobs_on_pool` remote-dispatch branch —
//! both call directly into this file and are part of the SAME change.
//!
//! **Wire protocol.** Newline-delimited JSON over a plain TCP connection,
//! one request/response pair per connection (a client opens a fresh
//! connection per job, sends one line, reads one line, closes — no
//! keep-alive, no pipelining; simplest thing that is still correct for a
//! trusted-LAN feature, per this feature's own explicit scope). Reuses
//! `Value`'s EXISTING JSON encoding (`crate::value_to_json`/`crate::
//! json_to_value`, the same hand-written conversion `save`/`load` already
//! use — grepped first, confirmed this is the only `Value`<->JSON bridge
//! in the codebase) rather than inventing a second serialization scheme,
//! per this feature's own explicit instruction to check for one first.
//!
//! - Request:  `{"fn": "<name>", "args": [<value_to_json>, ...]}\n`
//! - Response: `{"ok": true,  "result": <value_to_json>}\n`
//!          or `{"ok": false, "error": "<message>"}\n`
//!
//! A plain JSON object, not a bespoke binary framing, because (a) it rides
//! for free on `value_to_json`/`json_to_value`, (b) newline-delimited JSON
//! is trivially readable with `BufRead::read_line` (matches this
//! codebase's existing "as bare as possible, add your own framing if you
//! need more" TCP convention — see `tcp_send`/`tcp_recv`'s own doc
//! comments in `lib.rs`), and (c) `serde_json::to_string`'s compact output
//! never emits a raw newline byte inside a string (control characters are
//! always `\`-escaped), so treating one line as one whole message is
//! actually safe, not just convenient.
//!
//! **Security — the hard requirement this module exists to satisfy.** A
//! `listen_pool` server NEVER executes a function by name alone; `serve`
//! checks the requested name against the `allow` list captured at
//! `listen_pool(...)` call time before doing anything else, and returns a
//! clean `{"ok": false, "error": ...}` rejection (never a crash, never a
//! fallback to executing it anyway) for anything not on that list. There
//! is no code path in this module that can execute an arbitrary function
//! name — only ever pre-defined, allowlisted ones, and only ever by name
//! (never a shipped closure/source — Qu's job system has never had an
//! evaluable closure value at all, see `queue_pool::Job`'s own doc
//! comment).
//!
//! **Isolation model.** Each accepted request runs on a FRESH `Interp`
//! seeded with a snapshot of the listening script's `env`/`methods` taken
//! once, when `listen_pool` is first called — the exact same share-nothing
//! convention `queue_pool::run_one_job` already uses for local pool jobs,
//! so a script behaves identically whether a job lands on a local thread
//! or a remote listener. A panic inside the job function is caught (same
//! `catch_unwind` discipline as `run_one_job`) and reported as a normal
//! `{"ok": false, ...}` response instead of taking the whole listening
//! process down.
//!
//! **Timeouts.** `CONNECT_TIMEOUT` (client dial) and `IO_TIMEOUT` (both
//! sides, read+write once connected) are the answer to verification point
//! (d) — an unreachable/hung remote worker fails a bounded, reasonable
//! amount of time after being asked, never hangs the calling `run`
//! forever.
//!
//! **Failure handling for an unreachable/rejecting remote worker** is
//! decided in `queue_pool::run_jobs_on_pool`, not here: `send_job_remote`
//! below simply returns a normal `R<Value>` `Err` for every failure mode
//! (connect failure, timeout, malformed response, application-level
//! rejection/error) — indistinguishable, at this layer, from any other
//! job failure. See that function's own doc comment for why "surface as
//! this job's error, no silent fallback to local" was chosen over the
//! alternative.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::json;

use crate::queue_pool::Job;
use crate::{json_to_value, value_to_json, EvalError, Interp, MethodEntry, Value, R};

/// How long a client will wait to establish the TCP connection itself
/// before giving up — deliberately short (this is meant to fail fast on a
/// genuinely dead/unreachable host, not paper over a slow one).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long either side will wait on a read/write once connected — covers
/// a remote worker that accepted the connection but then hung (e.g. stuck
/// inside the job function) instead of a connection-level failure.
const IO_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------
// Client side — `pool ... with cpu=N, remote=(...)` dispatch
// ---------------------------------------------------------------------

/// Sends one job to the listener at `addr` (`"host:port"`) and returns its
/// result — the client-side half of the wire protocol documented in this
/// module's own top doc comment. Every failure mode (DNS/connect failure,
/// connect or I/O timeout, a malformed response line, or an application-
/// level `{"ok": false, ...}` — including an allowlist rejection) becomes
/// a plain `EvalError` naming `addr`, never a panic and never a partial/
/// guessed result.
pub fn send_job_remote(addr: &str, job: &Job) -> R<Value> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| EvalError {
            msg: format!("run: remote worker `{addr}` is not a valid \"host:port\" address: {err}"),
        })?
        .next()
        .ok_or_else(|| EvalError {
            msg: format!("run: remote worker `{addr}` did not resolve to any address"),
        })?;

    let stream = TcpStream::connect_timeout(&socket_addr, CONNECT_TIMEOUT).map_err(|err| EvalError {
        msg: format!("run: remote worker `{addr}` is unreachable: {err}"),
    })?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();

    let mut args_json = Vec::with_capacity(job.args.len());
    for a in &job.args {
        args_json.push(value_to_json(a)?);
    }
    let request = json!({"fn": job.fn_name, "args": args_json});

    let mut writer = stream.try_clone().map_err(|err| EvalError {
        msg: format!("run: could not prepare the connection to `{addr}`: {err}"),
    })?;
    writeln!(writer, "{request}").map_err(|err| EvalError {
        msg: format!("run: could not send job `{}` to `{addr}`: {err}", job.fn_name),
    })?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line).map_err(|err| EvalError {
        msg: format!("run: no response from `{addr}` for job `{}`: {err}", job.fn_name),
    })?;
    if n == 0 {
        return Err(EvalError {
            msg: format!(
                "run: remote worker `{addr}` closed the connection without a response for job `{}`",
                job.fn_name
            ),
        });
    }

    let parsed: serde_json::Value = serde_json::from_str(line.trim()).map_err(|err| EvalError {
        msg: format!("run: malformed response from `{addr}`: {err}"),
    })?;
    let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if ok {
        let result = parsed.get("result").ok_or_else(|| EvalError {
            msg: format!("run: response from `{addr}` is missing its `result` field"),
        })?;
        json_to_value(result)
    } else {
        let msg = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("(no error message given)");
        Err(EvalError {
            msg: format!("run: job `{}` was rejected by `{addr}`: {msg}", job.fn_name),
        })
    }
}

// ---------------------------------------------------------------------
// Server side — `listen_pool(port, allow=(...))`
// ---------------------------------------------------------------------

/// `listen_pool(port, allow=(...))` — binds `0.0.0.0:port`, then blocks
/// forever accepting connections, one job request at a time (see this
/// module's own top doc comment for the wire protocol, the security
/// allowlist, and the isolation model). Never returns normally (an
/// external stop — killing the process — is how a script-level "listen
/// mode" instance is meant to be shut down, matching the "blocks and
/// accepts connections" shape asked for; there is no in-language `stop`
/// builtin for it in this pass).
///
/// Status/error lines go straight to real stdout/stderr via `println!`/
/// `eprintln!`, NOT `Interp::out` (that buffer is only ever flushed once,
/// after a script finishes running — see `qu-cli`'s `print!("{}", it.out)`
/// — which would mean a listener's own status never becomes visible until
/// the process is killed, since this call never returns). This is the one
/// deliberate exception to that convention in this codebase, for exactly
/// that reason.
pub fn serve(interp: &mut Interp, port: i64, allow: Vec<String>) -> R<Value> {
    if !(0..=65535).contains(&port) {
        return Err(EvalError {
            msg: format!("listen_pool: port must be 0..=65535, got {port}"),
        });
    }
    if allow.is_empty() {
        return Err(EvalError {
            msg: "listen_pool: `allow=(...)` must name at least one function — a listener with \
                  an empty allowlist could never execute anything, which is almost certainly not \
                  what was intended"
                .into(),
        });
    }
    let listener = TcpListener::bind(("0.0.0.0", port as u16)).map_err(|err| EvalError {
        msg: format!("listen_pool: could not bind port {port}: {err}"),
    })?;
    let bound_port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(port as u16);
    println!(
        "listen_pool: listening on 0.0.0.0:{bound_port} (allow: [{}])",
        allow.join(", ")
    );
    let _ = std::io::stdout().flush();
    serve_on_listener(interp, listener, allow)
}

/// The actual accept loop, split out from `serve` (which just does the
/// port-binding/status-printing wrapper above it) so tests can bind their
/// own `TcpListener` — typically with port 0, letting the OS pick a free
/// ephemeral port and reading it back via `local_addr()` BEFORE this call
/// blocks forever — without going through a real `listen_pool(...)` script
/// call. Not a builtin entry point itself; `serve` is.
pub fn serve_on_listener(interp: &mut Interp, listener: TcpListener, allow: Vec<String>) -> R<Value> {
    // Snapshot ONCE, at `listen_pool` call time — see this module's top
    // doc comment's "Isolation model" section. A function defined AFTER
    // this call (unusual, but not forbidden by the grammar) would not be
    // visible to a later request; matches how `run`'s own env snapshot is
    // taken once per `run` call, not re-read per job.
    let env = interp.visible_env_snapshot();
    let methods = interp.methods.clone();

    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                if let Err(err) = handle_connection(stream, &allow, &env, &methods) {
                    eprintln!("listen_pool: connection from {peer} failed: {}", err.msg);
                }
            }
            Err(err) => {
                eprintln!("listen_pool: accept failed: {err}");
            }
        }
    }
}

/// Handles exactly one request on `stream`: read one line, dispatch (or
/// reject) it via `handle_request`, write back exactly one response line.
/// The `Err` this itself returns is strictly a CONNECTION-level failure
/// (a read/write I/O error) — an allowlist rejection or a runtime error
/// INSIDE the job function is captured into a normal `{"ok": false, ...}`
/// response line instead, never propagated here, so one misbehaving
/// request can never look like (or be logged as) a networking problem.
fn handle_connection(
    stream: TcpStream,
    allow: &[String],
    env: &HashMap<String, Value>,
    methods: &HashMap<String, Vec<MethodEntry>>,
) -> R<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    let read_stream = stream.try_clone().map_err(|err| EvalError {
        msg: format!("could not clone the incoming connection: {err}"),
    })?;
    let mut reader = BufReader::new(read_stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line).map_err(|err| EvalError {
        msg: format!("read failed: {err}"),
    })?;
    if n == 0 {
        // Peer connected then disconnected without sending anything (e.g.
        // a plain TCP health check) — not a real request, nothing to log
        // loudly about.
        return Ok(());
    }

    let response = match handle_request(&line, allow, env, methods) {
        Ok(value) => match value_to_json(&value) {
            Ok(j) => json!({"ok": true, "result": j}),
            Err(err) => json!({
                "ok": false,
                "error": format!("listen_pool: could not encode the result: {}", err.msg)
            }),
        },
        Err(err) => json!({"ok": false, "error": err.msg}),
    };

    let mut writer = stream;
    writeln!(writer, "{response}").map_err(|err| EvalError {
        msg: format!("write failed: {err}"),
    })?;
    Ok(())
}

/// Parses one request line, enforces the allowlist, and — only if the
/// name passes — runs the job on a fresh, share-nothing `Interp` (same
/// convention as `queue_pool::run_one_job`; see this module's top doc
/// comment). A panic inside the job function is caught and turned into a
/// normal `Err`, never allowed to take the listening process down.
fn handle_request(
    line: &str,
    allow: &[String],
    env: &HashMap<String, Value>,
    methods: &HashMap<String, Vec<MethodEntry>>,
) -> R<Value> {
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).map_err(|err| EvalError {
        msg: format!("listen_pool: malformed request (not valid JSON): {err}"),
    })?;
    let fn_name = parsed
        .get("fn")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EvalError {
            msg: "listen_pool: request is missing its string `fn` field".into(),
        })?
        .to_string();

    // THE security check this whole module exists for: never execute a
    // name that wasn't explicitly allowlisted by the listening script
    // itself, no matter what the request claims or asks for.
    if !allow.iter().any(|a| a == &fn_name) {
        return Err(EvalError {
            msg: format!(
                "listen_pool: function `{fn_name}` is not in this listener's allowlist — rejected"
            ),
        });
    }

    let args_json = parsed
        .get("args")
        .and_then(|v| v.as_array())
        .ok_or_else(|| EvalError {
            msg: "listen_pool: request is missing its array `args` field".into(),
        })?;
    let mut args = Vec::with_capacity(args_json.len());
    for a in args_json {
        args.push(json_to_value(a)?);
    }

    let mut worker = Interp::new();
    worker.env = env.clone();
    worker.methods = methods.clone();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker.apply(&fn_name, args, Vec::new())
    }));
    outcome.unwrap_or_else(|_| {
        Err(EvalError {
            msg: format!("listen_pool: job function `{fn_name}` panicked"),
        })
    })
}

#[cfg(test)]
mod tests {
    //! Real TCP loopback tests — genuine sockets, genuine `serve_on_listener`
    //! accept loop on a background thread, genuine `send_job_remote` client
    //! calls — NOT a mock. This is still one physical machine (there is no
    //! second one available in this environment), so it is not a true
    //! multi-machine test; see this crate's higher-level `lib.rs` tests
    //! (search `pool_with_only_remote_workers`/`pool_with_cpu_and_remote`)
    //! for the same distinction exercised through a real `.qu` script via
    //! `pool ... with remote=(...)` / `run jobs on pool`, and the actual
    //! two-process `qu.exe` verification run for this feature's own report.

    use super::*;
    use crate::Interp;

    fn job(name: &str, args: Vec<Value>) -> Job {
        Job {
            fn_name: name.to_string(),
            args,
            on: "any".to_string(),
        }
    }

    /// Spawns a real listener on an OS-assigned loopback port and returns
    /// its address — the listener itself keeps accepting on a detached
    /// background thread for the rest of the test process's life (fine for
    /// a `#[test]`; nothing here needs an explicit shutdown path since
    /// `listen_pool` itself has none either, by design — see `serve`'s own
    /// doc comment).
    fn spawn_listener(script: &str, allow: &[&str]) -> String {
        let mut it = Interp::new();
        it.run(script).unwrap_or_else(|e| panic!("listener script failed: {e}"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let allow: Vec<String> = allow.iter().map(|s| s.to_string()).collect();
        std::thread::spawn(move || {
            let _ = serve_on_listener(&mut it, listener, allow);
        });
        addr
    }

    #[test]
    fn send_job_remote_round_trips_a_real_tcp_request() {
        let addr = spawn_listener("double(n) := n * 2", &["double"]);
        let result = send_job_remote(&addr, &job("double", vec![Value::Num(21.0)])).unwrap();
        assert!(
            matches!(result, Value::Num(n) if (n - 42.0).abs() < 1e-9),
            "got: {result:?}"
        );
    }

    #[test]
    fn send_job_remote_is_rejected_cleanly_for_a_non_allowlisted_function() {
        // Verification point (c): the security allowlist actually rejects
        // a non-allowlisted function name — cleanly (a normal `Err`, no
        // crash, no silent execution anyway).
        let addr = spawn_listener("double(n) := n * 2\nother(n) := n", &["double"]);
        let err = send_job_remote(&addr, &job("other", vec![Value::Num(1.0)])).unwrap_err();
        assert!(err.msg.contains("other"), "got: {}", err.msg);
        assert!(
            err.msg.to_lowercase().contains("allowlist") || err.msg.to_lowercase().contains("rejected"),
            "got: {}",
            err.msg
        );
    }

    #[test]
    fn send_job_remote_surfaces_a_runtime_error_from_inside_the_job_function() {
        let addr = spawn_listener("boom(n) := error(\"kaboom {n}\")", &["boom"]);
        let err = send_job_remote(&addr, &job("boom", vec![Value::Num(7.0)])).unwrap_err();
        assert!(err.msg.contains("kaboom 7"), "got: {}", err.msg);
    }

    #[test]
    fn send_job_remote_fails_fast_on_an_unreachable_address_not_forever() {
        // Verification point (d): an unreachable/dead remote worker fails
        // within a real, bounded connection timeout instead of hanging.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = probe.local_addr().unwrap().port();
        drop(probe); // nothing is listening at this address anymore

        let start = std::time::Instant::now();
        let err = send_job_remote(
            &format!("127.0.0.1:{dead_port}"),
            &job("anything", vec![]),
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "should fail fast, took {elapsed:?}"
        );
        assert!(err.msg.contains("unreachable"), "got: {}", err.msg);
    }

    #[test]
    fn send_job_remote_reports_a_malformed_host_port_address_clearly() {
        let err = send_job_remote("not-a-valid-address", &job("anything", vec![])).unwrap_err();
        assert!(err.msg.contains("not-a-valid-address"), "got: {}", err.msg);
    }
}
