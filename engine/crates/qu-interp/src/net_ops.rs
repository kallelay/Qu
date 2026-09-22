//! `http_get(url)` (§ HTTP fetch, 2026-08-31) — Qu had raw TCP sockets
//! (`tcp_listen`/`tcp_connect`/etc., § socket I/O 2026-08-26) but no HTTP
//! client at all (grepped first, confirmed zero hits for `http_get`/`ureq`/
//! any HTTP verb anywhere in `qu-interp`) — this is that, kept deliberately
//! minimal per the brief: GET only, no custom headers/auth/POST in this
//! pass. Uses `ureq` — the same crate `qu-llm`'s `hf-hub` dependency already
//! pulls into this workspace for its own model downloads (checked
//! `crates/qu-llm/Cargo.toml` first) — so this is not a second HTTP client
//! entering the dependency tree.
//!
//! **Return shape**: always a `Value::Str` — the response body, UTF-8
//! decoded LOSSILY (same convention `read_all_text`/text-mode `read_all`
//! already use for "just give me the text"). Qu has no separate "bytes"
//! value distinct from a plain numeric `Vec` (see `read_bin`'s own doc
//! comment in `fs_ops.rs`), and the overwhelming common case for
//! `http_get` — JSON APIs, static text/HTML files — is text; a caller
//! fetching genuinely binary content over HTTP is out of this minimal v1's
//! scope (no `Content-Type`-based branching, to keep the return type
//! predictable rather than "sometimes a string, sometimes a byte vector
//! depending on what the server said").
//!
//! **Timeout**: a fixed 30-second timeout on the whole request (connect +
//! read), per the brief's explicit "don't hang forever on an unreachable
//! host" ask — not configurable in this v1 (no `timeout=` kwarg), since
//! nothing asked for one and it keeps the one-argument signature simple.
//!
//! **Errors**: any network failure (DNS, connection refused/timeout, TLS)
//! or non-2xx HTTP response is a clear `EvalError` naming the URL and (for
//! an HTTP-level failure) the actual status code — never a silently empty
//! string or a partial body passed off as success.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use crate::{arg0, as_url_stream, e, text_arg, url_stream_check_open, EvalError, UrlStreamState, Value, R};

pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "http_get" => http_get(args),
        "StreamURL" => stream_url(args),
        other => e(format!("net_ops: internal dispatch error, unhandled `{other}`")),
    }
}

/// `http_get(url)` — see this module's own doc comment for the return
/// shape/timeout/error conventions.
fn http_get(args: &[Value]) -> R<Value> {
    let url = text_arg(args, 0)?;
    // `arg0` is only invoked for its "no args at all" error message
    // (`text_arg` already covers "arg 0 isn't a string"); kept for
    // parity with every other single-arg builtin's own error phrasing.
    let _ = arg0(args)?;
    let body = fetch_body("http_get", &url)?;
    Ok(Value::Str(String::from_utf8_lossy(&body).into_owned()))
}

/// Shared GET + error-handling body for `http_get` and `StreamURL` — same
/// `ureq` agent config (30s connect+read timeout, per `http_get`'s own doc
/// comment), same status/transport error wording, just handed back as raw
/// bytes so `StreamURL` can store them for line-splitting rather than
/// immediately lossy-decoding the whole body to one `Value::Str` the way
/// `http_get` itself does. `label` names the calling builtin in error text.
fn fetch_body(label: &str, url: &str) -> R<Vec<u8>> {
    // ureq 3's `Error::StatusCode(u16)` (replacing 2's `Error::Status(code,
    // resp)`) doesn't carry the response any more, so there's no
    // `resp.status_text()` to read from it -- `http_status_as_error(false)`
    // keeps non-2xx responses out of the `Err` path entirely, and the
    // status/reason phrase is read from the `Ok` response instead, same
    // information as before via the standard `http` crate's `StatusCode`.
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    match agent.get(url).call() {
        Ok(mut resp) => {
            let status = resp.status();
            if !status.is_success() {
                let status_text = status.canonical_reason().unwrap_or("");
                let code = status.as_u16();
                return e(format!(
                    "{label}: `{url}` returned HTTP {code} ({status_text}) — not a successful (2xx) response"
                ));
            }
            // ureq 3's `Body::read_to_vec()` caps at 10MB by default -- a
            // real, silent regression versus ureq 2 (no such cap existed),
            // caught by actually running the round-trip against a real
            // server rather than trusting the type check. Raised to 100MB:
            // generous for this builtin's stated scope (JSON APIs, static
            // text/HTML -- see this module's own doc comment) without
            // reintroducing an unbounded buffer for a user-facing script
            // builtin that can be pointed at any URL.
            resp.body_mut()
                .with_config()
                .limit(100 * 1024 * 1024)
                .read_to_vec()
                .map_err(|err| EvalError {
                    msg: format!("{label}: could not read the response body from `{url}`: {err}"),
                })
        }
        Err(err) => e(format!(
            "{label}: could not reach `{url}`: {err} (network error, DNS failure, TLS failure, or timeout — a 30s timeout applies to the whole request)"
        )),
    }
}

// ================= StreamURL: polymorphic read-only stream over an HTTP body =================
//
// § StreamFile/StreamURL polymorphic streams, 2026-08-31 — Ahmed asked for
// `StreamFile(path)`/`StreamURL(url)` to be constructible objects exposing
// the SAME method names (`read_line`/`read_all`/`eof`/`close`), so code
// consuming "a stream" doesn't need to know which concrete kind it holds.
//
// **Fetch-then-wrap, not genuine incremental network streaming — a
// deliberate, investigated choice, not a shortcut.** `ureq::Response::
// into_reader()` DOES return a real `Read`-implementing body (confirmed by
// reading `http_get`'s own use of it above), so a live, byte-at-a-time
// network reader is technically available from the HTTP client. Reusing
// `fopen`'s exact streaming-reader plumbing (`file_read_byte_raw`/
// `file_read_upto`/`file_read_exact`/`file_remaining`, all in `lib.rs`)
// against that reader was the first option considered, but it doesn't fit
// without a real structural change to `FileHandleState` itself:
//   1. Every one of those helpers is hard-coded to `reader:
//      Option<BufReader<std::fs::File>>` — generalizing to also hold "or a
//      boxed `Read + Send` HTTP body" means widening that field's type
//      (and `FileHandleState` is a widely-shared struct another concurrent
//      session may be editing right now, per this repo's own shared-tree
//      caution — a bigger, riskier touch than this feature's own scope).
//   2. `file_remaining`/`eof` are defined as `file_len - read_pos`, with
//      `file_len` captured once up front from `File::metadata()`. An HTTP
//      response has no equivalent guarantee: `Content-Length` can be
//      absent (chunked transfer encoding, which `ureq` already
//      transparently decodes for the caller), so "the total length" isn't
//      always knowable before the body is fully read — a real semantic
//      gap `FileHandleState`'s model doesn't have, not just a type
//      mismatch.
// Given that, fetching the whole body up front (exactly what `http_get`
// already does — `fetch_body` above, shared verbatim) and exposing it
// through the same method names as a read-only in-memory stream is the
// right-sized answer for this pass: real incremental network streaming
// would be a genuine, separate `FileHandleState`-redesign task, not a
// reasonable extension of it. `StreamURL`'s `.eof()` is honest about this
// too — it always knows its own length (the fetch already completed), so
// there is no "unknown length" edge case to paper over.

/// `StreamURL(url)` — fetches the whole response body eagerly (same GET,
/// same 30s timeout, same error wording as `http_get`), then wraps the raw
/// bytes in a `Value::UrlStream` cursor exposing `read_line`/`read_all`/
/// `eof`/`close` — the same method names `StreamFile`'s `Value::File`
/// already has, so a script holding "a stream" of either kind can call
/// those without knowing which one it has (see this module's own doc
/// comment for the fetch-then-wrap reasoning, and `lib.rs`'s `read_line`/
/// `read_all` match arms for how they dispatch across both handle kinds).
fn stream_url(args: &[Value]) -> R<Value> {
    let url = text_arg(args, 0)?;
    let _ = arg0(args)?;
    let data = fetch_body("StreamURL", &url)?;
    Ok(Value::UrlStream(Arc::new(StdMutex::new(UrlStreamState {
        url,
        data,
        pos: 0,
        closed: false,
    }))))
}

/// `read_line` dispatch for a `Value::UrlStream` receiver — see `lib.rs`'s
/// `"read_line"` match arm, which calls this when `arg0` is a
/// `Value::UrlStream` rather than a `Value::File`. Same observable contract
/// as the file side: one line without its trailing `\n` (and a `\r`
/// immediately before it, if present), or `Value::Nothing` at EOF.
pub fn stream_read_line(v: &Value) -> R<Value> {
    let s = as_url_stream(v)?;
    let mut st = s.lock().unwrap();
    url_stream_check_open(&st)?;
    if st.pos >= st.data.len() {
        return Ok(Value::Nothing);
    }
    let start = st.pos;
    let rest = &st.data[start..];
    let (line_end, next_pos) = match rest.iter().position(|&b| b == b'\n') {
        Some(nl) => (nl, start + nl + 1),
        None => (rest.len(), st.data.len()),
    };
    let mut line_bytes = rest[..line_end].to_vec();
    if line_bytes.last() == Some(&b'\r') {
        line_bytes.pop();
    }
    st.pos = next_pos;
    Ok(Value::Str(String::from_utf8_lossy(&line_bytes).into_owned()))
}

/// `read_all` dispatch for a `Value::UrlStream` receiver — see `lib.rs`'s
/// `"read_all"` match arm. Always text (`Value::Str`), matching `http_get`'s
/// own "HTTP bodies are text" convention (see that builtin's module doc
/// comment) — there is no binary-mode `StreamURL`, unlike `StreamFile`,
/// which inherits `fopen`'s `"rb"` mode unchanged.
pub fn stream_read_all(v: &Value) -> R<Value> {
    let s = as_url_stream(v)?;
    let mut st = s.lock().unwrap();
    url_stream_check_open(&st)?;
    let rest = st.data[st.pos..].to_vec();
    st.pos = st.data.len();
    Ok(Value::Str(String::from_utf8_lossy(&rest).into_owned()))
}

/// `eof` dispatch for a `Value::UrlStream` receiver — see `lib.rs`'s `"eof"`
/// match arm.
pub fn stream_eof(v: &Value) -> R<Value> {
    let s = as_url_stream(v)?;
    let st = s.lock().unwrap();
    url_stream_check_open(&st)?;
    Ok(Value::Bool(st.pos >= st.data.len()))
}

/// `close` dispatch for a `Value::UrlStream` receiver — see `lib.rs`'s
/// `"close"` match arm. Idempotent, matching `Value::File`'s own `close`
/// convention: closing an already-closed stream is a no-op, not an error.
pub fn stream_close(v: &Value) -> R<Value> {
    let s = as_url_stream(v)?;
    let mut st = s.lock().unwrap();
    st.closed = true;
    Ok(Value::Nothing)
}
