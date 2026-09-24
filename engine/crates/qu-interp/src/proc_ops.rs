//! § process execution (2026-09-09) — `exec` and `shell`.
//!
//! Qu could already run *code* in another language (`python_exec`,
//! `js_exec`, `matlab_exec`, `lang_exec.rs`) but could not run a *program*.
//! That is a strange hole for a language whose own tooling is written in
//! itself: `tools/build_function_pages.sh` exists as a shell script mostly
//! because the Qu tool inside it cannot ask git a question or list a
//! directory, and the same is true of every script anyone else writes.
//!
//! Two builtins, because there are two genuinely different jobs and one
//! call cannot do both well:
//!
//!   - `exec(program, [args], ...)` — run it, WAIT, and hand back what it
//!     printed. This is the one a script wants: `exec("git", ("rev-parse",
//!     "HEAD")).stdout`. Output is captured, so nothing appears on the
//!     terminal unless the caller prints it.
//!   - `shell(command, [style])` — hand a command line to the OS shell and
//!     DO NOT wait. This is Visual Basic's `Shell`, which Qu's `import`
//!     already borrows from, down to the window style: normal, hidden,
//!     minimized, maximized. It returns a process id, not output, because
//!     there is no output yet when it returns.
//!
//! Both are on the sandbox deny-list (`check_sandbox` in `lib.rs`).
//! Spawning a process is the single most capability-bearing thing a script
//! can do, and `--sandbox` would be a lie if it let one through.
//!
//! Kept in its own module for the same reason `fs_ops.rs` is: `lib.rs` is
//! enormous and concurrently edited, so this touches it at exactly two
//! points (a `mod` line and one match arm).
//!
//! # `process_spawn` and friends (§ async/monitored process primitive,
//! 2026-09-23 — a direct follow-up to the bash/shell toolkit audit that
//! added `env=`/`timeout=` to `exec` above): the gap neither `exec` nor
//! `shell` closes. `exec` blocks until the program exits — no way to do
//! anything else meanwhile. `shell` never blocks, but its doc comment above
//! is explicit that its pipes are null-redirected ON PURPOSE, so there is
//! no way to *ever* read what it printed. Neither lets a script launch a
//! long-running program and then poll its output/exit status over time,
//! the way VB.NET's `Process` class or Python's `subprocess.Popen` do.
//!
//! `process_spawn(program, [args], [cwd=], [env=], [stdin=])` launches
//! immediately, like `shell`, but — unlike `shell` — keeps stdout/stderr
//! open and readable. `std::process::Child`'s own pipes are blocking, so
//! reading them from whichever thread later calls `process_poll`/
//! `process_read` would defeat the whole "don't block" point; instead, two
//! background reader threads are started at spawn time (one per pipe) that
//! continuously drain into shared `Arc<Mutex<Vec<u8>>>` buffers. Every
//! other builtin here just reads whatever has accumulated so far — never
//! blocking, never touching the pipes directly. See `ProcessHandleState`'s
//! own doc comment for the full representation and the reasoning behind a
//! `Mutex<Vec<u8>>` over an `mpsc` channel.
//!
//! Returns a `Value::Process` handle (`lib.rs`'s `Value` enum, right next
//! to `Value::File` — same `Arc<Mutex<...>>` "shared handle to live
//! internal state" shape, for the same reason: this mutates across calls).
//!
//! - `process_poll(handle)` — non-blocking. `Nothing` while still running,
//!   or the same `{stdout, stderr, exit_code, success, timed_out}` Record
//!   `exec` returns once it has exited (`timed_out` is always `false` here
//!   — nothing about a poll can time out; the field exists purely so the
//!   two Record shapes are drop-in identical for code that already knows
//!   how to read one).
//! - `process_wait(handle, [timeout=])` — blocks until exit, or until
//!   `timeout=` seconds elapse. On a real exit: the same Record
//!   `process_poll` returns, with `exit_code` a `Value::Num` (matching
//!   `exec`'s own convention, including `-1` for "killed by signal, no
//!   code"). On a timeout: a Record with `timed_out = true` and
//!   `exit_code = Value::Nothing` — deliberately NOT a number, so the two
//!   outcomes can never be confused by a script that only checks
//!   `exit_code`: a real exit always has a numeric one, a timeout never
//!   does. `stdout`/`stderr` on a timeout are whatever has accumulated so
//!   far (the process is still running and still writable — this does NOT
//!   kill it, unlike `exec`'s own `timeout=`, which does).
//! - `process_read(handle)` / `process_read_stderr(handle)` — drains and
//!   returns whatever NEW output text has arrived on that stream since the
//!   LAST `process_read`/`process_read_stderr` call on this same handle (a
//!   streaming "what's new" cursor, tracked per-handle, independent of the
//!   full buffer `process_poll`/`process_wait` still see) — not "everything
//!   since the process started". Empty string, never `Nothing`, when
//!   nothing new has arrived (there is nothing exceptional about that,
//!   same "absent vs merely empty" distinction `read_all`'s own doc
//!   comment draws elsewhere in this crate).
//! - `process_is_running(handle)` — non-blocking bool.
//! - `process_kill(handle)` — terminates the process THIS interpreter
//!   spawned and is still holding a live `Child` handle to, so (unlike the
//!   plain `kill(pid)` above, which shells out to `taskkill`/`kill` because
//!   it must work on an arbitrary external pid) this can just call
//!   `Child::kill()` directly.
//! - `process_pid(handle)` — the numeric process id, so a script can still
//!   hand it to `kill(pid)` or correlate it with `shell()`'s own
//!   pid-returning convention.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{arg0, display_value, e, int_arg, style_entry, style_num, style_str, text_arg, truthy, EvalError, Value, R};

/// Single dispatch entry point, called from `lib.rs`'s builtin match.
pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "exec" => exec(args, style),
        "shell" => shell(args, style),
        "kill" => kill(args),
        "process_spawn" => process_spawn(args, style),
        "process_poll" => process_poll(args),
        "process_wait" => process_wait(args, style),
        "process_read" => process_read(args),
        "process_read_stderr" => process_read_stderr(args),
        "process_is_running" => process_is_running(args),
        "process_kill" => process_kill(args),
        "process_pid" => process_pid(args),
        other => e(format!("proc_ops: internal dispatch error, unhandled `{other}`")),
    }
}

/// The shell this platform hands a command line to, and the flag that
/// makes it run one: `cmd /C ...` on Windows, `sh -c ...` everywhere else.
fn shell_prefix() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

/// `exec(program, [args], [cwd=], [stdin=], [shell=])` — run a program to
/// completion and return what it produced.
///
/// `program` is the executable to run and `args` an optional `List` of
/// arguments, passed to the OS as separate items — **not** joined into a
/// command line and re-split. That is the whole reason this is two
/// arguments rather than one string: a path with a space in it
/// (`C:/Program Files/...`) is a single argument here, and no quoting rule
/// has to be guessed at. `exec("git", ("log", "--oneline", "-1"))`.
///
/// `shell = true` instead reads `program` as a whole command line and hands
/// it to `cmd /C` (Windows) or `sh -c`, so pipes, redirection and `&&` work:
/// `exec("git status | head -3", shell = true)`. Convenient, and the usual
/// caveat applies — anything interpolated into that string is interpreted
/// by the shell, so build the argument list instead when the pieces come
/// from data.
///
/// `cwd=` runs it in that directory without disturbing the caller's own
/// (unlike `cd`, which changes it for the whole process). `stdin=` is
/// written to the program's standard input, which is then closed — a
/// program that reads until EOF gets exactly that text and no hang.
///
/// Returns a `Record` with `stdout`, `stderr` (strings, decoded lossily so
/// a program that emits non-UTF-8 bytes cannot make this error),
/// `exit_code` (a number) and `success` (a bool). The SAME four fields
/// `python_exec`/`js_exec`/`matlab_exec` already return, deliberately —
/// they are all "run something and tell me how it went", and a reader who
/// has used one should not have to learn a second shape. There is no
/// `result` field here: those three inject an output path into the guest
/// and parse JSON back, which a general program knows nothing about.
///
/// **A program that fails is not a Qu error.** A non-zero exit is reported
/// as `success = false`, not raised — `exec("git", ("diff", "--quiet"))`
/// uses the exit code as its answer, and a language that threw on it would
/// make that unwritable. What DOES error is being unable to start the
/// program at all, which is a different thing and worth a stack trace.
fn exec(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let program = text_arg(args, 0)?;
    let use_shell = style_entry(style, "shell")
        .map(|(_, v)| truthy(v))
        .unwrap_or(false);

    let mut cmd = if use_shell {
        let (sh, flag) = shell_prefix();
        let mut c = Command::new(sh);
        c.arg(flag).arg(&program);
        c
    } else {
        let mut c = Command::new(&program);
        if let Some(list) = args.get(1) {
            for a in as_arg_list(list)? {
                c.arg(a);
            }
        }
        c
    };

    if let Some(dir) = style_str(style, "cwd") {
        if !std::path::Path::new(&dir).is_dir() {
            return e(format!("exec: cwd=`{dir}` does not exist or is not a directory"));
        }
        cmd.current_dir(dir);
    }
    // `env=` (§ bash/shell toolkit audit, 2026-09-23): a Dict/Record of
    // extra/overriding variables for the CHILD only -- the caller's own
    // environment (and a later `setenv`) is untouched. Added on top of
    // whatever the process already inherited, not a replacement of it:
    // `Command::env` layers rather than resets, matching how every shell's
    // own `VAR=x program` prefix behaves.
    if let Some((_, v)) = style_entry(style, "env") {
        for (k, val) in env_pairs(v, "exec")? {
            cmd.env(k, val);
        }
    }
    let stdin_text = style_str(style, "stdin");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin_text.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = cmd.spawn().map_err(|err| EvalError {
        msg: format!("exec: could not start `{program}`: {err}"),
    })?;
    if let Some(text) = stdin_text {
        use std::io::Write as _;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(text.as_bytes()).map_err(|err| EvalError {
                msg: format!("exec: could not write stdin to `{program}`: {err}"),
            })?;
            // Dropped here rather than at the end of the call: a program
            // reading until EOF would otherwise wait forever for a pipe
            // this side is still holding open, and `wait_with_output`
            // below would wait with it.
        }
    }

    // `timeout=` seconds (§ bash/shell toolkit audit, 2026-09-23): without
    // it, `exec` waits forever, and a hung external tool hangs the whole
    // script with no escape. Polls `try_wait` against a deadline rather
    // than blocking in `wait_with_output`, since `Child` has no native
    // timeout; on expiry the child is killed and `timed_out=true` is the
    // one new field on the returned Record (`success`/`exit_code` still
    // report the killed process's own status, not a lie).
    let timed_out;
    let out = if let Some(secs) = style_num(style, "timeout") {
        if secs <= 0.0 {
            return e(format!("exec: `timeout={secs}` must be a positive number of seconds"));
        }
        let deadline = Instant::now() + Duration::from_secs_f64(secs);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    timed_out = false;
                    break;
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        // `kill_tree`, not `child.kill()` -- see that
                        // function's own doc comment for why a bare
                        // Child::kill() here was found, live, to leave a
                        // shell-wrapped command's actual grandchild running
                        // and holding the stdout pipe open, defeating the
                        // whole point of a timeout.
                        kill_tree(child.id());
                        let _ = child.wait(); // reap, so it doesn't linger as a zombie
                        timed_out = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(err) => {
                    return e(format!("exec: `{program}` could not be polled: {err}"));
                }
            }
        }
        // Either way the child has already exited by this point (normal
        // completion, or killed-and-reaped above) -- `wait_with_output`
        // just drains the now-finished stdout/stderr pipes and returns
        // immediately, the same call every non-timeout `exec` uses.
        child.wait_with_output().map_err(|err| EvalError {
            msg: format!("exec: `{program}` could not be waited on: {err}"),
        })?
    } else {
        timed_out = false;
        child.wait_with_output().map_err(|err| EvalError {
            msg: format!("exec: `{program}` could not be waited on: {err}"),
        })?
    };

    Ok(Value::Record(Arc::new(vec![
        (
            "stdout".to_string(),
            Value::Str(String::from_utf8_lossy(&out.stdout).into_owned()),
        ),
        (
            "stderr".to_string(),
            Value::Str(String::from_utf8_lossy(&out.stderr).into_owned()),
        ),
        ("success".to_string(), Value::Bool(out.status.success())),
        (
            "exit_code".to_string(),
            // A process killed by a signal has no exit code at all
            // (`status.code()` is `None` on Unix). `-1` rather than
            // `Nothing` keeps the field a number in every case, which is
            // what `success` is there to disambiguate.
            Value::Num(out.status.code().unwrap_or(-1) as f64),
        ),
        ("timed_out".to_string(), Value::Bool(timed_out)),
    ])))
}

/// `env=`'s value: a `Dict` or `Record` of name/value pairs. Every value is
/// rendered with `display_value`, matching `as_arg_list`'s own "a number
/// needs no `str` at the call site" convention.
fn env_pairs(v: &Value, who: &str) -> R<Vec<(String, String)>> {
    match v {
        Value::Dict(pairs) | Value::Record(pairs) => {
            Ok(pairs.iter().map(|(k, val)| (k.clone(), display_value(val))).collect())
        }
        other => e(format!(
            "{who}: `env=` expected a dict/record of name/value pairs, found {}",
            other.type_name()
        )),
    }
}

/// `kill(pid)` — terminates a process by id, e.g. one `shell()` returned
/// earlier. Shells out to the platform's own kill tool rather than pulling
/// in a new OS-API dependency for it: `Child::kill()` only works on a
/// process THIS interpreter spawned and is still holding a handle to,
/// which is exactly what `shell()`'s whole contract is NOT (it returns a
/// bare id and lets go). Returns `true` if the kill command itself ran and
/// reported success -- not a guarantee the process still existed to be
/// killed; killing an already-dead pid is not an error here, matching
/// `remove_file`-style "the end state is what you asked for" semantics
/// more than "prove something happened".
fn kill(args: &[Value]) -> R<Value> {
    let pid = int_arg(args, 0)?;
    if pid <= 0 {
        return e(format!("kill: `{pid}` is not a valid process id"));
    }
    Ok(Value::Bool(kill_tree(pid as u32)))
}

/// Kills `pid` AND every process it spawned -- `taskkill /PID <pid> /T /F`
/// on Windows, `kill -9 <pid>` elsewhere. **Why `/T` (tree) matters, found
/// live while testing `exec`'s `timeout=`**: `exec("cmd", ("/C", "ping -n
/// 30 ...")); timeout=1` correctly detected the timeout and killed the
/// `cmd.exe` child at ~1s, but the whole call still blocked for the full
/// 30s anyway -- `cmd.exe`'s own child (`ping.exe`) was never touched,
/// kept running, and kept the piped stdout handle open, so
/// `wait_with_output`'s read-until-EOF never saw EOF until `ping.exe`
/// finally exited on its own. Without `/T`, a `shell=true` call or any
/// manual `cmd /C`/`sh -c` wrapping defeats `timeout=`/`kill(pid)` in
/// exactly this way. Plain POSIX `kill -9` has no tree-kill flag the way
/// `taskkill` does; the Unix side of this is a known, smaller gap (it
/// still kills the named pid correctly, just not any of ITS children) --
/// closing it properly needs spawning into a new process group at launch
/// time (`setsid`/`process_group`), a bigger change than this fix, left
/// for later if it turns out to matter in practice on that platform.
fn kill_tree(pid: u32) -> bool {
    let status = if cfg!(windows) {
        Command::new("taskkill").arg("/PID").arg(pid.to_string()).arg("/T").arg("/F").status()
    } else {
        Command::new("kill").arg("-9").arg(pid.to_string()).status()
    };
    status.map(|s| s.success()).unwrap_or(false)
}

/// A `List`/tuple of arguments, or one bare value used as a single
/// argument. Every element is rendered with `display_value`, so a number
/// argument (`exec("sleep", (5))`) needs no `str` at the call site.
fn as_arg_list(v: &Value) -> R<Vec<String>> {
    match v {
        Value::List(items) => Ok(items.iter().map(display_value).collect()),
        Value::Str(s) => Ok(vec![s.to_string()]),
        Value::Num(_) | Value::Bool(_) => Ok(vec![display_value(v)]),
        other => e(format!(
            "exec: the second argument is the argument list -- expected a list of \
             strings, got {}",
            other.type_name()
        )),
    }
}

/// `shell(command, [style])` — hand `command` to the OS shell and return
/// immediately, without waiting for it.
///
/// This is Visual Basic's `Shell` function, which is where the second
/// argument comes from too:
///
/// | `style`                  | what it does                              |
/// |--------------------------|-------------------------------------------|
/// | `"normal"` (default)     | a window, as the program would normally open |
/// | `"hidden"` / `"ghost"`   | no window at all                          |
/// | `"minimized"`            | opens minimized to the taskbar            |
/// | `"maximized"`            | opens filling the screen                  |
///
/// Returns the process id as a number, so `shell` composes with whatever
/// the caller wants to do about the process later.
///
/// **The id is not always the program's own.** `"minimized"` and
/// `"maximized"` have no API reachable from `std::process::Command`, whose
/// `STARTUPINFO` this cannot set — they go through `cmd /C start /MIN|/MAX`,
/// and the id that comes back belongs to that launcher, which exits as soon
/// as it has started the program. `"normal"` and `"hidden"` spawn the shell
/// directly and give a real, live id. Said here rather than left to be
/// discovered, because an id that is already dead is exactly the kind of
/// thing a script would use and not notice.
///
/// **On anything but Windows**, `"normal"` and `"hidden"` behave (there is
/// no window to hide, so "hidden" simply means "no terminal is borrowed"),
/// and `"minimized"`/`"maximized"` ERROR rather than being quietly ignored.
/// They are window-manager instructions with no meaning to `sh`, and
/// accepting them silently would mean a script that looks portable and is
/// not.
fn shell(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let command = text_arg(args, 0)?;
    // Positional second argument, or `style=` — a window style reads
    // naturally either way, and VB's own `Shell` takes it positionally.
    let want = match args.get(1) {
        Some(v) => display_value(v),
        None => style_str(style, "style").unwrap_or_else(|| "normal".to_string()),
    };
    let want = want.trim().to_ascii_lowercase();
    let mode = match want.as_str() {
        "normal" | "" => Mode::Normal,
        "hidden" | "hide" | "ghost" => Mode::Hidden,
        "minimized" | "min" => Mode::Minimized,
        "maximized" | "max" => Mode::Maximized,
        other => {
            return e(format!(
                "shell: `{other}` is not a window style -- use \"normal\", \"hidden\" \
                 (also spelled \"ghost\"), \"minimized\" or \"maximized\""
            ))
        }
    };
    let env = match style_entry(style, "env") {
        Some((_, v)) => env_pairs(v, "shell")?,
        None => Vec::new(),
    };
    spawn_styled(&command, mode, &env)
}

/// The window styles `shell` accepts.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Hidden,
    Minimized,
    Maximized,
}

#[cfg(windows)]
fn spawn_styled(command: &str, mode: Mode, env: &[(String, String)]) -> R<Value> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW` — the process gets no console of its own and
    /// does not borrow the parent's. This is the real "ghost" mode;
    /// `start /B` still lets a console app paint into the caller's window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = Command::new("cmd");
    match mode {
        Mode::Normal => {
            cmd.arg("/C").arg(command);
        }
        Mode::Hidden => {
            cmd.arg("/C").arg(command).creation_flags(CREATE_NO_WINDOW);
        }
        // `start` takes a window title first, and an empty one is the
        // documented way to say "no title" -- without it, a quoted program
        // path would be READ as the title and nothing would launch.
        Mode::Minimized => {
            cmd.arg("/C").arg("start").arg("").arg("/MIN").arg(command);
        }
        Mode::Maximized => {
            cmd.arg("/C").arg("start").arg("").arg("/MAX").arg(command);
        }
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    // Detached from this process's own pipes: `shell` does not wait, so
    // there is nobody to read them and an inherited pipe that fills up
    // would block the child forever.
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().map_err(|err| EvalError {
        msg: format!("shell: could not start `{command}`: {err}"),
    })?;
    Ok(Value::Num(child.id() as f64))
}

#[cfg(not(windows))]
fn spawn_styled(command: &str, mode: Mode, env: &[(String, String)]) -> R<Value> {
    if matches!(mode, Mode::Minimized | Mode::Maximized) {
        return e(
            "shell: \"minimized\"/\"maximized\" are Windows window styles and have no \
             meaning here -- use \"normal\" or \"hidden\""
                .to_string(),
        );
    }
    let (sh, flag) = shell_prefix();
    let mut cmd = Command::new(sh);
    cmd.arg(flag).arg(command);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().map_err(|err| EvalError {
        msg: format!("shell: could not start `{command}`: {err}"),
    })?;
    Ok(Value::Num(child.id() as f64))
}

// ---------------------------------------------------------------------
// § async/monitored process primitive (2026-09-23) -- see this module's
// own doc comment above for the full design.
// ---------------------------------------------------------------------

/// Cached exit information for a `Value::Process` handle, filled in once
/// (`ProcessHandleState::refresh_exit`) by an actual `try_wait` that
/// observed the child had exited. Kept as its own small `Clone` struct
/// (rather than reaching back into `std::process::ExitStatus` every time)
/// so a snapshot can be taken while the state's mutex is held and then
/// used afterward, without re-locking.
#[derive(Clone, Copy)]
struct ProcessExitInfo {
    /// `None` exactly when `ExitStatus::code()` is (a process killed by a
    /// signal on Unix) -- `process_poll`/`process_wait` report that as `-1`,
    /// the same "no code, but not a timeout either" convention `exec`'s own
    /// `exit_code` field already uses.
    exit_code: Option<i32>,
    success: bool,
}

/// Backing state for `Value::Process` (`process_spawn` and the
/// `process_poll`/`process_wait`/`process_read`/`process_read_stderr`/
/// `process_is_running`/`process_kill`/`process_pid` family). Follows
/// `FileHandleState`'s exact `Arc<StdMutex<...>>` convention (see its own
/// doc comment in `lib.rs`) since a live process handle is genuinely
/// mutable across calls: the read cursors advance, the reader threads keep
/// appending, and the process itself transitions from running to exited.
///
/// # Why two background reader threads, and why `Mutex<Vec<u8>>` rather
/// than a channel
///
/// `Child::stdout`/`Child::stderr` are blocking pipes. Without a reader
/// thread, `process_poll` (which must never block) would have nothing safe
/// to read -- draining a blocking pipe non-blockingly isn't possible with
/// `std::process::ChildStdout` alone. So each pipe gets its own thread,
/// spawned once at `process_spawn` time, that loops `read()`ing into the
/// shared buffer until EOF (the process exited and closed the pipe) or a
/// real I/O error, then exits.
///
/// A `Mutex<Vec<u8>>` per stream, not an `mpsc` channel, because two
/// genuinely different things need to read the SAME accumulated output at
/// different granularities: `process_poll`/`process_wait` want the WHOLE
/// captured output so far (matching `exec`'s own all-of-it `stdout`
/// field), while `process_read`/`process_read_stderr` want only what's
/// NEW since their own last call. A channel gives you one or the other for
/// free (drain-to-here, or peek-without-consuming) but not both without
/// re-buffering into a `Vec` anyway -- so the `Vec` a reader thread appends
/// to directly, plus a separate read-cursor `usize` per handle for the
/// "what's new" side, does both with no duplicate storage.
pub struct ProcessHandleState {
    program: String,
    child: Child,
    stdout_buf: Arc<StdMutex<Vec<u8>>>,
    stderr_buf: Arc<StdMutex<Vec<u8>>>,
    /// `process_read`'s cursor into `stdout_buf` -- see this struct's own
    /// doc comment's "why two background reader threads" paragraph.
    stdout_read_pos: usize,
    /// `process_read_stderr`'s cursor into `stderr_buf`.
    stderr_read_pos: usize,
    /// Taken and joined (see `refresh_exit`) the first time the process is
    /// observed to have exited, so `stdout_buf`/`stderr_buf` are guaranteed
    /// complete before `exit_info` is reported as populated -- a reader
    /// thread can still have a final chunk in flight for a few instructions
    /// after `try_wait` first reports the child gone.
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    exit_info: Option<ProcessExitInfo>,
}

impl std::fmt::Debug for ProcessHandleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessHandleState")
            .field("program", &self.program)
            .field("pid", &self.child.id())
            .field("exited", &self.exit_info.is_some())
            .finish()
    }
}

impl ProcessHandleState {
    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Non-blocking: `try_wait`s the child once. If it has exited and this
    /// is the first time that's been observed, joins both reader threads
    /// (bounded -- a thread already sitting on a closed pipe's EOF returns
    /// almost immediately) so the output buffers are complete, and caches
    /// the exit status so a reaped child is never `try_wait`ed again.
    fn refresh_exit(&mut self) -> R<()> {
        if self.exit_info.is_some() {
            return Ok(());
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                if let Some(h) = self.stdout_thread.take() {
                    let _ = h.join();
                }
                if let Some(h) = self.stderr_thread.take() {
                    let _ = h.join();
                }
                self.exit_info = Some(ProcessExitInfo {
                    exit_code: status.code(),
                    success: status.success(),
                });
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => e(format!(
                "process: `{}` (pid {}) could not be polled: {err}",
                self.program,
                self.child.id()
            )),
        }
    }

    /// Whether the process has exited, without forcing a fresh `try_wait`
    /// (used by `display_value`'s `Value::Process` arm, which calls
    /// `refresh_exit` itself first) -- see `refresh_exit`'s own doc comment.
    pub fn exit_status(&self) -> Option<()> {
        self.exit_info.map(|_| ())
    }

    /// See `refresh_exit`'s own doc comment; call it first if a fresh
    /// answer (rather than the last-cached one) is needed.
    pub fn refresh_and_get_exit(&mut self) -> R<Option<(Option<i32>, bool)>> {
        self.refresh_exit()?;
        Ok(self.exit_info.map(|info| (info.exit_code, info.success)))
    }

    fn stdout_snapshot(&self) -> String {
        String::from_utf8_lossy(&self.stdout_buf.lock().unwrap()).into_owned()
    }

    fn stderr_snapshot(&self) -> String {
        String::from_utf8_lossy(&self.stderr_buf.lock().unwrap()).into_owned()
    }

    /// Drains and returns whatever NEW stdout bytes have arrived since the
    /// last call to this method on this same handle -- see this module's
    /// own doc comment for the "what's new" vs "everything so far"
    /// distinction. Decodes lossily, same convention `exec`'s own
    /// `stdout`/`stderr` fields use; a multi-byte UTF-8 sequence that
    /// happens to straddle exactly where one call ends and the next begins
    /// can show as a replacement character at the boundary -- a known,
    /// accepted edge (the same byte-level tradeoff `FileHandleState`'s own
    /// streaming reads document elsewhere in this crate), not worth a
    /// pushback buffer for a diagnostics-output stream.
    fn read_new_stdout(&mut self) -> String {
        let buf = self.stdout_buf.lock().unwrap();
        let start = self.stdout_read_pos.min(buf.len());
        let s = String::from_utf8_lossy(&buf[start..]).into_owned();
        self.stdout_read_pos = buf.len();
        s
    }

    /// `process_read_stderr`'s counterpart to `read_new_stdout`.
    fn read_new_stderr(&mut self) -> String {
        let buf = self.stderr_buf.lock().unwrap();
        let start = self.stderr_read_pos.min(buf.len());
        let s = String::from_utf8_lossy(&buf[start..]).into_owned();
        self.stderr_read_pos = buf.len();
        s
    }
}

fn as_process(v: &Value) -> R<&Arc<StdMutex<ProcessHandleState>>> {
    match v {
        Value::Process(p) => Ok(p),
        other => e(format!(
            "expected a process handle (from process_spawn), found {}",
            other.type_name()
        )),
    }
}

/// A background reader thread body: loops reading `src` into `buf` until
/// EOF (the pipe closed, i.e. the process exited) or a real I/O error, then
/// returns. See `ProcessHandleState`'s own doc comment for why this exists
/// (making a blocking pipe safe for a non-blocking `process_poll` to
/// observe) and why the destination is a plain `Mutex<Vec<u8>>`.
fn spawn_reader(mut src: impl std::io::Read + Send + 'static, buf: Arc<StdMutex<Vec<u8>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match src.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.lock().unwrap().extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
    })
}

/// Assembles the `{stdout, stderr, exit_code, success, timed_out}` Record
/// `process_poll`/`process_wait` both return on completion -- the exact
/// same four-plus-one field shape `exec` returns (see its own doc comment),
/// so a reader who already knows one shape doesn't have to learn a second.
fn exit_record(stdout: String, stderr: String, exit_code: Value, success: bool, timed_out: bool) -> Value {
    Value::Record(Arc::new(vec![
        ("stdout".to_string(), Value::Str(stdout)),
        ("stderr".to_string(), Value::Str(stderr)),
        ("success".to_string(), Value::Bool(success)),
        ("exit_code".to_string(), exit_code),
        ("timed_out".to_string(), Value::Bool(timed_out)),
    ]))
}

/// `process_spawn(program, [args], [cwd=], [env=], [stdin=])` -- launch
/// immediately, do NOT wait, return a `Value::Process` handle. See this
/// module's own doc comment for the full design; argument handling
/// (`as_arg_list`/`env_pairs`/`cwd=`/`stdin=`) deliberately mirrors `exec`'s
/// own above, reusing its helpers rather than duplicating them.
fn process_spawn(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let program = text_arg(args, 0)?;

    let mut cmd = Command::new(&program);
    if let Some(list) = args.get(1) {
        for a in as_arg_list(list)? {
            cmd.arg(a);
        }
    }

    if let Some(dir) = style_str(style, "cwd") {
        if !std::path::Path::new(&dir).is_dir() {
            return e(format!("process_spawn: cwd=`{dir}` does not exist or is not a directory"));
        }
        cmd.current_dir(dir);
    }
    if let Some((_, v)) = style_entry(style, "env") {
        for (k, val) in env_pairs(v, "process_spawn")? {
            cmd.env(k, val);
        }
    }

    let stdin_text = style_str(style, "stdin");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin_text.is_some() { Stdio::piped() } else { Stdio::null() });

    let mut child = cmd.spawn().map_err(|err| EvalError {
        msg: format!("process_spawn: could not start `{program}`: {err}"),
    })?;

    if let Some(text) = stdin_text {
        use std::io::Write as _;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(text.as_bytes()).map_err(|err| EvalError {
                msg: format!("process_spawn: could not write stdin to `{program}`: {err}"),
            })?;
            // Dropped here, not held for the life of the handle -- a
            // program reading until EOF gets exactly this text and no
            // hang, same reasoning as `exec`'s own stdin handling above.
        }
    }

    let stdout_buf = Arc::new(StdMutex::new(Vec::new()));
    let stderr_buf = Arc::new(StdMutex::new(Vec::new()));
    let stdout_thread = child.stdout.take().map(|out| spawn_reader(out, Arc::clone(&stdout_buf)));
    let stderr_thread = child.stderr.take().map(|errp| spawn_reader(errp, Arc::clone(&stderr_buf)));

    Ok(Value::Process(Arc::new(StdMutex::new(ProcessHandleState {
        program,
        child,
        stdout_buf,
        stderr_buf,
        stdout_read_pos: 0,
        stderr_read_pos: 0,
        stdout_thread,
        stderr_thread,
        exit_info: None,
    }))))
}

/// `process_poll(handle)` -- non-blocking. `Nothing` while still running,
/// else the same Record `process_wait` returns on completion.
fn process_poll(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let mut st = ph.lock().unwrap();
    match st.refresh_and_get_exit()? {
        None => Ok(Value::Nothing),
        Some((exit_code, success)) => {
            let stdout = st.stdout_snapshot();
            let stderr = st.stderr_snapshot();
            Ok(exit_record(
                stdout,
                stderr,
                Value::Num(exit_code.unwrap_or(-1) as f64),
                success,
                false,
            ))
        }
    }
}

/// `process_wait(handle, [timeout=])` -- blocks until the process exits, or
/// until `timeout=` seconds elapse. See this module's own doc comment for
/// the exact Record shape on each outcome and why a timeout's `exit_code`
/// is `Value::Nothing` rather than a number (the one thing that can never
/// also be true of a real exit, so the two outcomes can't be confused by
/// code that only checks `exit_code`). Polls `try_wait` on a short sleep
/// loop rather than a real blocking `wait()`, matching `exec`'s own
/// `timeout=` implementation above -- the only way to bound a wait against
/// a `Child` at all, which has no native timeout.
fn process_wait(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let timeout_secs = style_num(style, "timeout");
    if let Some(secs) = timeout_secs {
        if secs <= 0.0 {
            return e(format!("process_wait: `timeout={secs}` must be a positive number of seconds"));
        }
    }
    let deadline = timeout_secs.map(|secs| Instant::now() + Duration::from_secs_f64(secs));

    loop {
        let mut st = ph.lock().unwrap();
        if let Some((exit_code, success)) = st.refresh_and_get_exit()? {
            let stdout = st.stdout_snapshot();
            let stderr = st.stderr_snapshot();
            return Ok(exit_record(
                stdout,
                stderr,
                Value::Num(exit_code.unwrap_or(-1) as f64),
                success,
                false,
            ));
        }
        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                // Still running -- NOT killed (unlike `exec`'s own
                // `timeout=`, which kills on expiry: `exec` owns the whole
                // wait, `process_wait` is just one check-in on a handle the
                // script may still want to keep polling). `exit_code =
                // Nothing` is the documented, type-level distinguisher from
                // a real exit -- see this function's own doc comment.
                let stdout = st.stdout_snapshot();
                let stderr = st.stderr_snapshot();
                return Ok(exit_record(stdout, stderr, Value::Nothing, false, true));
            }
        }
        drop(st);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `process_read(handle)` -- drains and returns whatever NEW stdout text
/// has arrived since the last `process_read` call on this handle.
fn process_read(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let mut st = ph.lock().unwrap();
    Ok(Value::Str(st.read_new_stdout()))
}

/// `process_read_stderr(handle)` -- `process_read`'s counterpart for
/// stderr.
fn process_read_stderr(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let mut st = ph.lock().unwrap();
    Ok(Value::Str(st.read_new_stderr()))
}

/// `process_is_running(handle)` -- non-blocking bool.
fn process_is_running(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let mut st = ph.lock().unwrap();
    Ok(Value::Bool(st.refresh_and_get_exit()?.is_none()))
}

/// `process_kill(handle)` -- terminates the process THIS interpreter
/// spawned, via `Child::kill()` directly (see this module's own doc
/// comment for why that's simpler than the plain `kill(pid)` above, which
/// must shell out because it targets an arbitrary external pid). Killing
/// an already-exited process is not an error here -- same "the end state
/// is what you asked for" convention `kill(pid)`'s own doc comment states
/// for an already-dead pid -- it just returns `false` (nothing was left to
/// kill) instead of propagating `Child::kill()`'s `InvalidInput` error for
/// that case.
fn process_kill(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let mut st = ph.lock().unwrap();
    if st.refresh_and_get_exit()?.is_some() {
        return Ok(Value::Bool(false));
    }
    match st.child.kill() {
        Ok(()) => Ok(Value::Bool(true)),
        Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => Ok(Value::Bool(false)),
        Err(err) => e(format!(
            "process_kill: could not kill `{}` (pid {}): {err}",
            st.program,
            st.child.id()
        )),
    }
}

/// `process_pid(handle)` -- the numeric process id.
fn process_pid(args: &[Value]) -> R<Value> {
    let ph = as_process(arg0(args)?)?;
    let st = ph.lock().unwrap();
    Ok(Value::Num(st.pid() as f64))
}
