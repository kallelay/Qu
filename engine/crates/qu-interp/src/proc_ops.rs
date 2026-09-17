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

use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::{display_value, e, style_entry, style_str, text_arg, truthy, EvalError, Value, R};

/// Single dispatch entry point, called from `lib.rs`'s builtin match.
pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "exec" => exec(args, style),
        "shell" => shell(args, style),
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
    let out = child.wait_with_output().map_err(|err| EvalError {
        msg: format!("exec: `{program}` could not be waited on: {err}"),
    })?;

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
    ])))
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
    spawn_styled(&command, mode)
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
fn spawn_styled(command: &str, mode: Mode) -> R<Value> {
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
fn spawn_styled(command: &str, mode: Mode) -> R<Value> {
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
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().map_err(|err| EvalError {
        msg: format!("shell: could not start `{command}`: {err}"),
    })?;
    Ok(Value::Num(child.id() as f64))
}
