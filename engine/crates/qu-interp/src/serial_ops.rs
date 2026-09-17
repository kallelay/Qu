//! `serial_open(port, baud_rate, [data_bits=8, stop_bits=1, parity="none",
//! timeout_ms=1000])` / `serial_ports()` / `.read_line()` / `.read_bytes(n)`
//! / `.write(data)` / `.available()` / `.close()` (§ real serial port I/O,
//! 2026-09-01 — Ahmed asked directly for genuine UART/RS-232/USB-serial
//! support, with a real real-time demo built on top of it; grepped first
//! and confirmed zero serial port support existed anywhere in Qu —
//! `tcp_listen`/`tcp_connect`/etc, `qu-interp`'s only prior socket library,
//! are raw TCP sockets with no baud-rate/parity/stop-bit concept at all).
//!
//! Uses the `serialport` crate (v4 — see `Cargo.toml`'s own comment for why
//! it was picked): a thin, actively-maintained, cross-platform wrapper over
//! each OS's native serial API (`CreateFile`+`DCB`/`COMMTIMEOUTS` on
//! Windows, termios on Unix), not a hand-rolled FFI layer.
//!
//! # Method names: shared with `Value::File` where the semantics genuinely
//! match, deliberately NOT where they don't
//!
//! - `read_line`/`close`/`write` ARE shared with `Value::File`/
//!   `Value::UrlStream` — guarded arms in `lib.rs`'s `call_builtin`
//!   (`"read_line" if matches!(arg0(&args)?, Value::Serial(_)) => ...`),
//!   the same "dispatch on arg0's type" pattern `write`'s own
//!   `Value::File` arm already uses.
//! - `read_all` is deliberately NOT implemented for a serial handle: "read
//!   everything remaining" has no honest meaning against a device with no
//!   fixed length and no natural EOF — unlike `Value::File` (a known byte
//!   count from `File::metadata()`) or `Value::UrlStream` (the whole HTTP
//!   body, fetched eagerly up front before the stream object even exists).
//!   Calling `read_all(serial_handle)` isn't specially guarded here, so it
//!   falls through to `lib.rs`'s plain `Value::File` arm, whose own
//!   `as_file(...)` check already produces a clear "expected a file handle
//!   (from fopen), found serial" error — exactly the right answer, not
//!   something this module needs to duplicate.
//! - `eof` is likewise NOT implemented, for the same reason (falls through
//!   to `fs_ops::call`'s own `as_file` check, same clear error).
//! - `read_bytes(n)` and `available()` are NEW method names, unique to a
//!   serial handle today (no other `Value` kind answers to them, so they're
//!   dispatched unconditionally to this module rather than needing a
//!   guarded arm).
//!
//! # Timeout, not EOF — the fundamental difference from a file/URL stream
//!
//! A file has a byte length known up front; an HTTP response (fetched
//! eagerly by `StreamURL`) does too, once fetched. A live serial device has
//! neither — data arrives whenever the far end sends it, or never. Every
//! blocking read below is bounded by the handle's own `timeout_ms` (set at
//! `serial_open` time, `serialport::SerialPort::set_timeout` under the
//! hood via the builder's `.timeout(...)`) rather than running forever
//! waiting for a `\n`/`n` bytes that may never come.
//!
//! Confirmed directly in the vendored crate source
//! (`serialport-4.10.0/src/windows/com.rs`'s and `.../posix/tty.rs`'s own
//! `Read` impls) that a timed-out read surfaces as
//! `io::ErrorKind::TimedOut` on BOTH platforms, never `Ok(0)` — this module
//! treats exactly that error kind as "no data (yet)", not a hard failure,
//! so a slow/idle device is never indistinguishable from a real I/O fault:
//! - `read_line`: a timeout with NOTHING collected yet returns
//!   `Value::Nothing` (same "absent" marker `read_line` on a file/stream
//!   uses at EOF); a timeout after SOME bytes arrived returns that partial
//!   line as-is (no trailing `\n` to strip, since there wasn't one) —
//!   there is no clean EOF to keep waiting for, so "the device went quiet"
//!   is treated as "this line is what you get for now," not an error.
//! - `read_bytes(n)`: returns however many bytes were actually collected
//!   before the timeout elapsed, which may be FEWER than `n` — an honest
//!   short read, not a hard failure or a hang.
//!
//! # Real-hardware vs structural verification
//!
//! See this feature's test module (`lib.rs`'s `#[cfg(test)] mod tests`) and
//! the final task report for the exact split of what was verified against
//! real hardware (an actual USB-serial adapter present on the dev machine)
//! versus what could only be verified via the error path (no loopback pair
//! was installed — see the report for why).

use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use crate::{arg0, as_serial, display_value, e, serial_check_open, text_arg, EvalError, SerialState, Value, R};

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "serial_open" => serial_open(args, style),
        "serial_ports" => serial_ports(),
        "read_bytes" => read_bytes_dispatch(args),
        "available" => available_dispatch(args),
        other => e(format!("serial_ops: internal dispatch error, unhandled `{other}`")),
    }
}

/// `serial_open(port, baud_rate, [data_bits=8, stop_bits=1, parity="none",
/// timeout_ms=1000])` — opens a real OS serial port. `data_bits` accepts
/// 5/6/7/8, `stop_bits` accepts 1/2 (no 1.5 — the `serialport` crate's own
/// `StopBits` enum only has `One`/`Two`, so a script asking for 1.5 gets a
/// clear "must be 1 or 2" error rather than silently rounding), `parity`
/// accepts `"none"`/`"even"`/`"odd"` (case-insensitive). `timeout_ms` sets
/// the bound every blocking read below waits for new data before giving up
/// — see this module's own doc comment for why that's the serial-specific
/// replacement for a file/stream's "clean EOF."
fn serial_open(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let port_name = text_arg(args, 0)?;
    // `arg0` only for its "no args at all" error message, matching
    // `http_get`/`stream_url`'s own use of the same pairing.
    let _ = arg0(args)?;
    let baud_f = args
        .get(1)
        .ok_or_else(|| EvalError {
            msg: "serial_open: expected a baud rate as the second argument".to_string(),
        })?
        .as_num()
        .map_err(|m| EvalError {
            msg: format!("serial_open: baud rate {m}"),
        })?;
    if !baud_f.is_finite() || baud_f <= 0.0 || baud_f.fract() != 0.0 {
        return e(format!("serial_open: baud_rate must be a positive whole number, got {baud_f}"));
    }
    let baud = baud_f as u32;

    let data_bits = match crate::style_entry(style, "data_bits") {
        Some((_, v)) => {
            let n = v.as_num().map_err(|m| EvalError {
                msg: format!("serial_open: data_bits {m}"),
            })?;
            match n as i64 {
                5 => serialport::DataBits::Five,
                6 => serialport::DataBits::Six,
                7 => serialport::DataBits::Seven,
                8 => serialport::DataBits::Eight,
                other => return e(format!("serial_open: data_bits must be 5, 6, 7, or 8, got {other}")),
            }
        }
        None => serialport::DataBits::Eight,
    };

    let stop_bits = match crate::style_entry(style, "stop_bits") {
        Some((_, v)) => {
            let n = v.as_num().map_err(|m| EvalError {
                msg: format!("serial_open: stop_bits {m}"),
            })?;
            match n as i64 {
                1 => serialport::StopBits::One,
                2 => serialport::StopBits::Two,
                other => {
                    return e(format!(
                        "serial_open: stop_bits must be 1 or 2 (the underlying `serialport` crate has \
                         no 1.5-stop-bit option), got {other}"
                    ))
                }
            }
        }
        None => serialport::StopBits::One,
    };

    let parity = match crate::style_entry(style, "parity") {
        Some((_, v)) => {
            let s = display_value(v).to_lowercase();
            match s.as_str() {
                "none" => serialport::Parity::None,
                "even" => serialport::Parity::Even,
                "odd" => serialport::Parity::Odd,
                other => {
                    return e(format!(
                        "serial_open: parity must be \"none\", \"even\", or \"odd\", got \"{other}\""
                    ))
                }
            }
        }
        None => serialport::Parity::None,
    };

    let timeout_ms = match crate::style_entry(style, "timeout_ms") {
        Some((_, v)) => {
            let n = v.as_num().map_err(|m| EvalError {
                msg: format!("serial_open: timeout_ms {m}"),
            })?;
            if !n.is_finite() || n < 0.0 {
                return e(format!("serial_open: timeout_ms must be a non-negative number, got {n}"));
            }
            n
        }
        None => 1000.0,
    };

    let port = serialport::new(port_name.clone(), baud)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .timeout(Duration::from_millis(timeout_ms as u64))
        .open()
        .map_err(|err| EvalError {
            msg: format!(
                "serial_open: could not open `{port_name}` at {baud} baud: {err} (check the port \
                 name is correct — `serial_ports()` lists what's currently available on this \
                 machine — and that no other program already has it open)"
            ),
        })?;

    Ok(Value::Serial(Arc::new(StdMutex::new(SerialState {
        port_name,
        baud_rate: baud,
        port: Some(port),
        closed: false,
    }))))
}

/// `serial_ports()` — lists the short names of every serial device the OS
/// currently reports (e.g. `"COM3"` on Windows, `"/dev/ttyUSB0"` on Linux).
/// A machine with no serial hardware attached honestly returns an empty
/// list — this is a correct result, not treated as an error, per
/// `serialport::available_ports()`'s own documented contract ("not
/// guaranteed that these ports exist or are available even if returned").
fn serial_ports() -> R<Value> {
    match serialport::available_ports() {
        Ok(ports) => {
            let names: Vec<Value> = ports.into_iter().map(|p| Value::Str(p.port_name)).collect();
            Ok(Value::List(Arc::new(names)))
        }
        Err(err) => e(format!("serial_ports: could not enumerate serial ports on this machine: {err}")),
    }
}

fn read_bytes_dispatch(args: &[Value]) -> R<Value> {
    let recv = arg0(args)?;
    if !matches!(recv, Value::Serial(_)) {
        return e(format!(
            "read_bytes: expected a serial port handle (from serial_open), found {}",
            recv.type_name()
        ));
    }
    let n_f = args
        .get(1)
        .ok_or_else(|| EvalError {
            msg: "read_bytes: expected a byte count as the second argument".to_string(),
        })?
        .as_num()
        .map_err(|m| EvalError {
            msg: format!("read_bytes: byte count {m}"),
        })?;
    if !n_f.is_finite() || n_f < 0.0 || n_f.fract() != 0.0 {
        return e(format!("read_bytes: byte count must be a non-negative whole number, got {n_f}"));
    }
    read_bytes(recv, n_f as usize)
}

/// `read_bytes(serial, n)` — blocks (bounded by the handle's `timeout_ms`)
/// collecting up to `n` bytes. May return FEWER than `n` bytes if the
/// timeout elapses first — an honest short read, not a hard failure — see
/// this module's own doc comment. Bytes come back as a `Value::Vec` of
/// 0-255 values, the same representation `read_bin`/binary-mode `read_all`
/// already use for "raw bytes" (Qu has no separate bytes type — see
/// `net_ops.rs`'s own doc comment for the same point about `http_get`).
fn read_bytes(v: &Value, n: usize) -> R<Value> {
    let s = as_serial(v)?;
    let mut st = s.lock().unwrap();
    serial_check_open(&st)?;
    let mut buf = vec![0u8; n];
    let mut got = 0usize;
    let port_name = st.port_name.clone();
    let port = st.port.as_mut().unwrap();
    while got < n {
        match port.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
            Err(err) => {
                return e(format!("read_bytes: error reading from serial port `{port_name}`: {err}"))
            }
        }
    }
    buf.truncate(got);
    Ok(Value::Vec(Arc::new(buf.into_iter().map(|b| b as f64).collect())))
}

/// `read_line(serial)` dispatch — see `lib.rs`'s `"read_line"` match arm,
/// which calls this when `arg0` is a `Value::Serial`. See this module's own
/// doc comment for the exact "timeout instead of EOF" contract: only a
/// timeout with ZERO bytes collected yet returns `Value::Nothing`; a
/// timeout after some bytes arrived returns that partial line.
pub fn serial_read_line(v: &Value) -> R<Value> {
    let s = as_serial(v)?;
    let mut st = s.lock().unwrap();
    serial_check_open(&st)?;
    let port_name = st.port_name.clone();
    let port = st.port.as_mut().unwrap();
    let mut line_bytes: Vec<u8> = Vec::new();
    let mut one = [0u8; 1];
    loop {
        match port.read(&mut one) {
            Ok(0) => break,
            Ok(_) => {
                if one[0] == b'\n' {
                    break;
                }
                line_bytes.push(one[0]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                if line_bytes.is_empty() {
                    return Ok(Value::Nothing);
                }
                break;
            }
            Err(err) => return e(format!("read_line: error reading from serial port `{port_name}`: {err}")),
        }
    }
    if line_bytes.last() == Some(&b'\r') {
        line_bytes.pop();
    }
    Ok(Value::Str(String::from_utf8_lossy(&line_bytes).into_owned()))
}

fn available_dispatch(args: &[Value]) -> R<Value> {
    let recv = arg0(args)?;
    if !matches!(recv, Value::Serial(_)) {
        return e(format!(
            "available: expected a serial port handle (from serial_open), found {}",
            recv.type_name()
        ));
    }
    let s = as_serial(recv)?;
    let st = s.lock().unwrap();
    serial_check_open(&st)?;
    let port = st.port.as_ref().unwrap();
    let n = port.bytes_to_read().map_err(|err| EvalError {
        msg: format!("available: could not query serial port `{}`: {err}", st.port_name),
    })?;
    Ok(Value::Num(n as f64))
}

/// `write(serial, data)` dispatch — see `lib.rs`'s `"write"` match arm,
/// which calls this when `args[0]` is a `Value::Serial` (same "dispatch on
/// arg0's type" pattern the `Value::File` arm just above it already uses).
/// `data` may be a `Value::Str` (written as UTF-8 bytes) or a `Value::Vec`
/// of raw byte values 0-255 (the same "no separate bytes type" convention
/// `read_bytes`/`read_bin` use on the read side) — anything else is a
/// clear type error rather than a silent `display_value` stringification.
pub fn serial_write(args: &[Value]) -> R<Value> {
    let recv = arg0(args)?;
    let s = as_serial(recv)?;
    let mut st = s.lock().unwrap();
    serial_check_open(&st)?;
    let data = args.get(1).ok_or_else(|| EvalError {
        msg: "write: expected a second argument (the data to write to the serial port)".to_string(),
    })?;
    let bytes: Vec<u8> = match data {
        Value::Str(text) => text.as_bytes().to_vec(),
        Value::Vec(xs) => xs.iter().map(|&x| x as u8).collect(),
        other => {
            return e(format!(
                "write: expected a string or a vector of byte values (0-255) to write to a serial \
                 port, found {}",
                other.type_name()
            ))
        }
    };
    let port_name = st.port_name.clone();
    let port = st.port.as_mut().unwrap();
    port.write_all(&bytes).map_err(|err| EvalError {
        msg: format!("write: could not write to serial port `{port_name}`: {err}"),
    })?;
    // Best-effort: flushes the OS's own output buffer where that's a real,
    // separate step (Windows `FlushFileBuffers`); harmless where it isn't.
    let _ = port.flush();
    Ok(Value::Nothing)
}

/// `close(serial)` dispatch — see `lib.rs`'s `"close"` match arm. Idempotent
/// like `Value::File`'s own `close`: closing an already-closed handle is a
/// no-op, not an error. Drops the underlying `Box<dyn SerialPort>`, which
/// releases the real OS handle (Windows `CloseHandle`/Unix `close(2)`) —
/// the same "closing genuinely releases the OS resource" contract `fopen`'s
/// own `close` gives.
pub fn serial_close(v: &Value) -> R<Value> {
    let s = as_serial(v)?;
    let mut st = s.lock().unwrap();
    st.closed = true;
    st.port = None;
    Ok(Value::Nothing)
}
