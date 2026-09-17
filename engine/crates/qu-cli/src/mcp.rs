//! `qu mcp` — a Model Context Protocol server over stdio, so an assistant
//! can run Qu instead of guessing at it.
//!
//! The failure mode this exists to remove: a model writes Qu from memory,
//! the code is subtly wrong (a half-open range, a function that does not
//! exist, a keyword argument spelled the Python way), and nobody finds out
//! until a human runs it. Every tool here closes one of those loops —
//! `qu_check` says whether it parses, `qu_eval` says what it prints,
//! `qu_builtins` says whether a name exists at all, and it answers from the
//! interpreter's own table rather than from prose that can drift.
//!
//! **Why each call is a child process.** The server could evaluate
//! in-process and save a few milliseconds. It does not, because the three
//! properties that matter for a tool a model drives cannot be had in
//! process: a hard wall-clock limit (a model writes `while true` sooner or
//! later), a hard memory limit, and surviving a script that aborts. `qu
//! run` already implements all three as flags, so the server spends a
//! process per call and gets them for free. Startup is a few milliseconds;
//! there is no interpreter warm-up to amortise.
//!
//! **Sandboxed by default.** Calls run under `--sandbox`, which denies the
//! network, process-spawning and file-writing builtins at the moment they
//! are called. Start the server with `--allow-write` to lift that, which
//! is needed only if you want scripts to save their own files — figures
//! come back through `qu_figure` without it, because the figure is written
//! by the CLI rather than by the script.
//!
//! Framing is one JSON-RPC message per line on stdin, one per line on
//! stdout. Nothing else may ever be written to stdout; diagnostics go to
//! stderr.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Versions this server speaks. The client's request is echoed back when it
/// names one of these; otherwise it is answered with the newest, which is
/// what the specification asks for.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Tool output is capped so a runaway `print` in a loop cannot fill the
/// model's context. The cap is generous enough that no honest example
/// reaches it.
const MAX_OUTPUT: usize = 60_000;

/// The documentation, compiled in. An installed `qu` is usually nowhere
/// near a checkout, and a documentation tool that only works inside the
/// repository would be useless exactly where an agent needs it.
const DOCS: &[(&str, &str, &str)] = &[
    (
        "agent-guide",
        "Where Qu differs from what you would assume",
        include_str!("../../../../docs/llms-agent-guide.md"),
    ),
    (
        "builtin-index",
        "Every name the engine answers to, generated from its own table",
        include_str!("../../../../book/src/stdlib/builtin-index.md"),
    ),
    (
        "from-zero",
        "A complete first session that runs as written",
        include_str!("../../../../book/src/getting-started/from-zero.md"),
    ),
    (
        "fundamentals",
        "Types, contracts, control flow, error handling",
        include_str!("../../../../book/src/getting-started/book1-fundamentals.md"),
    ),
    (
        "stdlib-overview",
        "The standard library by area",
        include_str!("../../../../book/src/stdlib/overview.md"),
    ),
    (
        "core-math",
        "Core maths and linear algebra",
        include_str!("../../../../book/src/stdlib/core-math.md"),
    ),
    (
        "signal-processing",
        "Spectra, filters, windows, resampling",
        include_str!("../../../../book/src/stdlib/signal-processing.md"),
    ),
    (
        "statistics-ml",
        "Statistics, regression, models",
        include_str!("../../../../book/src/stdlib/statistics-ml.md"),
    ),
    (
        "plotting",
        "Figures, axes, annotation, export",
        include_str!("../../../../book/src/stdlib/plotting.md"),
    ),
    (
        "file-io",
        "Reading and writing files, including instrument captures",
        include_str!("../../../../book/src/stdlib/file-io.md"),
    ),
    (
        "collections-strings",
        "Vectors, tables, strings, dictionaries",
        include_str!("../../../../book/src/stdlib/collections-strings.md"),
    ),
    (
        "concurrency",
        "Parallel evaluation and pools",
        include_str!("../../../../book/src/stdlib/concurrency.md"),
    ),
    (
        "images",
        "Loading, transforming and measuring images",
        include_str!("../../../../book/src/stdlib/images.md"),
    ),
    (
        "repl-diagnostics",
        "The REPL, profiling, and reading an error",
        include_str!("../../../../book/src/stdlib/repl-diagnostics.md"),
    ),
    (
        "tour",
        "The language end to end, with examples",
        include_str!("../../../../docs/qu-language-tour.md"),
    ),
    (
        "spec",
        "The normative specification (ahead of the engine in places)",
        include_str!("../../../../docs/qu-language-spec.md"),
    ),
];

/// Handed to the client in the `initialize` response. A model reads this
/// once, before it writes anything, so it holds the rules that make the
/// difference between code that runs and code that looks right — not a
/// description of the product.
const INSTRUCTIONS: &str = "\
Qu is an array language for measurement science. You can run it here rather \
than guessing: `qu_check` parses without executing, `qu_eval` runs and \
returns what was printed, `qu_figure` returns the figure a script draws, and \
`qu_builtins` answers from the interpreter's own name table -- if a name is \
not there, the engine does not have it.

Six rules cover most of what a model gets wrong, because they are the places \
Qu differs from Python and MATLAB rather than the places it is unusual:

1. EVERY range includes both ends, slices too. `0 to 9` is ten values, `v[0:4]` \
   is five elements, and `M[0:1, :]` is two rows. `v[0:n]` is n+1 elements; the \
   first n are `v[0:n - 1]`, the same way you walk them with `0 to n - 1`. \
   `()` is the empty list -- there is no empty slice.
2. Indices start at zero. The last element of a thousand is `v[999]`.
3. `*` and `^` are matrix multiply and matrix power. Element-by-element is \
   `.*` and `.^`. On two vectors of the same length `*` is an error, not a \
   product.
4. `as matrix(r, c)` fills by COLUMN. This one is silent: you get a matrix, \
   just not the one you meant.
5. A keyword argument a builtin does not read is an ERROR, with the nearest \
   real name suggested. `colour=` does not quietly do nothing.
6. Blocks close with a word: `end if`, `end for`, `end function`. Indentation \
   carries no meaning.

Assignment inside a function is local unless the function says `global name`. \
Reads still see the enclosing scope.

Start from the `agent-guide` document (`qu_docs`) if you have not written Qu \
before; it is short, and it is entirely about these differences.";

// ── configuration ──────────────────────────────────────────────────────

struct Config {
    allow_write: bool,
    timeout_s: f64,
    memory_mb: u64,
    /// When set, `qu_run_file` refuses paths outside this directory.
    root: Option<PathBuf>,
    exe: PathBuf,
}

impl Config {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut cfg = Config {
            allow_write: false,
            timeout_s: 30.0,
            memory_mb: 2048,
            root: None,
            exe: std::env::current_exe()
                .map_err(|e| format!("cannot locate the qu executable: {e}"))?,
        };
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--allow-write" => cfg.allow_write = true,
                "--timeout" => {
                    i += 1;
                    let raw = args.get(i).ok_or("--timeout expects a number of seconds")?;
                    cfg.timeout_s = raw
                        .parse()
                        .map_err(|_| format!("--timeout expects a number of seconds, got `{raw}`"))?;
                }
                "--memory" => {
                    i += 1;
                    let raw = args.get(i).ok_or("--memory expects a number of megabytes")?;
                    cfg.memory_mb = raw
                        .parse()
                        .map_err(|_| format!("--memory expects a number of megabytes, got `{raw}`"))?;
                }
                "--root" => {
                    i += 1;
                    let raw = args.get(i).ok_or("--root expects a directory")?;
                    cfg.root = Some(
                        std::fs::canonicalize(raw)
                            .map_err(|e| format!("--root `{raw}`: {e}"))?,
                    );
                }
                other => return Err(format!("qu mcp: unknown option `{other}`")),
            }
            i += 1;
        }
        Ok(cfg)
    }
}

// ── the loop ───────────────────────────────────────────────────────────

pub fn cmd_mcp(args: &[String]) -> Result<(), String> {
    let cfg = Config::parse(args)?;
    eprintln!(
        "qu mcp {} — {} execution, {}s / {} MB per call",
        env!("CARGO_PKG_VERSION"),
        if cfg.allow_write { "unrestricted" } else { "sandboxed" },
        cfg.timeout_s,
        cfg.memory_mb
    );

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err(format!("stdin: {e}")),
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // A malformed frame has no id to answer against, so the
                // reply carries a null one, as JSON-RPC requires.
                send(&mut stdout, &rpc_error(Value::Null, -32700, &format!("parse error: {e}")))?;
                continue;
            }
        };
        if let Some(reply) = handle(&cfg, &msg) {
            send(&mut stdout, &reply)?;
        }
    }
    Ok(())
}

fn send(out: &mut io::Stdout, msg: &Value) -> Result<(), String> {
    writeln!(out, "{msg}").map_err(|e| format!("stdout: {e}"))?;
    out.flush().map_err(|e| format!("stdout: {e}"))
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn handle(cfg: &Config, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    // No id means a notification: act on it, answer nothing. Replying to
    // one is a protocol violation that some clients treat as fatal.
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_specs() })),
        "tools/call" => tools_call(cfg, &params),
        "resources/list" => Ok(resources_list()),
        "resources/read" => resources_read(&params),
        // Declared empty rather than unimplemented: a client that asks and
        // gets "method not found" logs an error every session.
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err((-32601, format!("unknown method `{method}`"))),
    };

    let id = id?;
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, message)) => rpc_error(id, code, &message),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = match asked {
        Some(v) if PROTOCOL_VERSIONS.contains(&v) => v,
        _ => PROTOCOL_VERSIONS[0],
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {}, "resources": {} },
        "serverInfo": { "name": "qu", "title": "Qu", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

// ── tools ──────────────────────────────────────────────────────────────

fn tool_specs() -> Value {
    json!([
        {
            "name": "qu_eval",
            "title": "Run Qu",
            "description":
                "Run Qu source and return everything it printed. Sandboxed and \
                 time-limited. Use this to check an answer rather than asserting \
                 one: printing the shape or the first few values of a result costs \
                 one call and settles the question.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Qu source to run." }
                },
                "required": ["code"]
            }
        },
        {
            "name": "qu_check",
            "title": "Parse Qu",
            "description":
                "Parse Qu source WITHOUT running it and return any diagnostics with \
                 line and column. Cheaper than qu_eval and safe on code with side \
                 effects; it catches the structural mistakes (a missing `end for`, a \
                 Python-style colon) but not undefined names, which are a runtime \
                 error in Qu.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Qu source to parse." }
                },
                "required": ["code"]
            }
        },
        {
            "name": "qu_builtins",
            "title": "Search builtin names",
            "description":
                "Search the engine's own table of builtin names. This is the \
                 authority on whether a function exists: the table is the one the \
                 interpreter dispatches on, not a list in a document. Call it before \
                 using a name you are not certain of.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description":
                            "Substring to match, case-insensitive. Omit to list every name."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum names to return (default 80)."
                    }
                }
            }
        },
        {
            "name": "qu_docs",
            "title": "Read documentation",
            "description":
                "Read a Qu documentation page. Start with `agent-guide`, which is \
                 short and is entirely about where Qu differs from what you would \
                 assume from Python or MATLAB.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page": {
                        "type": "string",
                        "description": "Page name.",
                        "enum": DOCS.iter().map(|(n, _, _)| *n).collect::<Vec<_>>()
                    },
                    "search": {
                        "type": "string",
                        "description":
                            "Return only the sections whose heading or body contains \
                             this text. Worth using on `spec` and `tour`, which are large."
                    }
                },
                "required": ["page"]
            }
        },
        {
            "name": "qu_figure",
            "title": "Draw a figure",
            "description":
                "Run Qu that draws, and return the figure as SVG source. The script \
                 does not need to call `savefig`; every figure it produced is \
                 returned. Use it to verify a plot actually contains what you \
                 intended -- the SVG carries the axis labels, tick values and legend \
                 text as text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Qu source that draws." }
                },
                "required": ["code"]
            }
        },
        {
            "name": "qu_run_file",
            "title": "Run a Qu script",
            "description":
                "Run a .qu file from disk and return what it printed. Arguments after \
                 the path reach the script as `argv()`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the script." },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments passed to the script as `argv()`."
                    }
                },
                "required": ["path"]
            }
        }
    ])
}

fn tools_call(cfg: &Config, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "tools/call requires a tool name".to_string()))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    let need_str = |key: &str| -> Result<String, (i64, String)> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or((-32602, format!("`{name}` requires a string `{key}`")))
    };

    match name {
        "qu_eval" => Ok(run_source(cfg, &need_str("code")?, None)),
        "qu_check" => Ok(check_source(cfg, &need_str("code")?)),
        "qu_builtins" => Ok(builtins(&args)),
        "qu_docs" => docs(&args),
        "qu_figure" => Ok(figure(cfg, &need_str("code")?)),
        "qu_run_file" => run_file(cfg, &args),
        other => Err((-32602, format!("unknown tool `{other}`"))),
    }
}

/// A tool result. Qu failing is NOT a protocol error — the model asked what
/// this code does and "it does not run, here is why" is the answer, so it
/// comes back as content with `isError` set rather than as a JSON-RPC
/// fault, which most clients hide.
fn tool_text(text: impl Into<String>, is_error: bool) -> Value {
    let mut text = text.into();
    if text.len() > MAX_OUTPUT {
        let kept = text
            .char_indices()
            .take_while(|(i, _)| *i < MAX_OUTPUT)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        let dropped = text.len() - kept;
        text.truncate(kept);
        text.push_str(&format!("\n\n[{dropped} more bytes not shown]"));
    }
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

// ── running Qu ─────────────────────────────────────────────────────────

/// A scratch directory for one call, removed when the call ends. Scripts
/// that write do so here, so a sandboxed server never leaves anything
/// behind and an unsandboxed one leaves it somewhere harmless.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "qu-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("scratch directory: {e}"))?;
        Ok(Scratch(dir))
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Everything a child `qu` run produced.
struct Outcome {
    ok: bool,
    text: String,
}

fn invoke(cfg: &Config, script: &Path, extra: &[String], cwd: &Path) -> Outcome {
    let mut cmd = Command::new(&cfg.exe);
    cmd.arg("run")
        .arg(script)
        .arg("--max-time")
        .arg(cfg.timeout_s.to_string())
        .arg("--max-memory")
        .arg(cfg.memory_mb.to_string());
    if !cfg.allow_write {
        cmd.arg("--sandbox");
    }
    for a in extra {
        cmd.arg(a);
    }
    // stdin is closed rather than inherited: this process's stdin is the
    // MCP channel, and a script that calls `input()` must see end-of-file
    // instead of eating a protocol message.
    let out = cmd
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match out {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(err.trim_end());
                text.push('\n');
            }
            Outcome { ok: o.status.success(), text }
        }
        Err(e) => Outcome { ok: false, text: format!("could not start the engine: {e}\n") },
    }
}

fn run_source(cfg: &Config, code: &str, extra: Option<&[String]>) -> Value {
    let scratch = match Scratch::new() {
        Ok(s) => s,
        Err(e) => return tool_text(e, true),
    };
    let script = scratch.path("snippet.qu");
    if let Err(e) = std::fs::write(&script, code) {
        return tool_text(format!("could not write the snippet: {e}"), true);
    }
    let out = invoke(cfg, &script, extra.unwrap_or(&[]), &scratch.0);
    let text = if out.text.trim().is_empty() {
        if out.ok {
            "(ran, printed nothing)".to_string()
        } else {
            "failed, with no message".to_string()
        }
    } else {
        out.text
    };
    tool_text(text, !out.ok)
}

fn check_source(cfg: &Config, code: &str) -> Value {
    let scratch = match Scratch::new() {
        Ok(s) => s,
        Err(e) => return tool_text(e, true),
    };
    let script = scratch.path("snippet.qu");
    if let Err(e) = std::fs::write(&script, code) {
        return tool_text(format!("could not write the snippet: {e}"), true);
    }
    let out = Command::new(&cfg.exe)
        .arg("parse")
        .arg(&script)
        .arg("--json")
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let body = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if ok {
                tool_text("parses cleanly", false)
            } else {
                let mut msg = String::from("does not parse:\n");
                if let Some(ds) = parsed.get("diagnostics").and_then(Value::as_array) {
                    for d in ds {
                        let line = d.get("line").and_then(Value::as_u64).unwrap_or(0);
                        let col = d.get("column").and_then(Value::as_u64).unwrap_or(0);
                        let m = d.get("message").and_then(Value::as_str).unwrap_or("");
                        msg.push_str(&format!("  {line}:{col}  {m}\n"));
                    }
                } else {
                    msg.push_str(&body);
                }
                tool_text(msg, true)
            }
        }
        Err(e) => tool_text(format!("could not start the engine: {e}"), true),
    }
}

fn figure(cfg: &Config, code: &str) -> Value {
    let scratch = match Scratch::new() {
        Ok(s) => s,
        Err(e) => return tool_text(e, true),
    };
    let script = scratch.path("snippet.qu");
    if let Err(e) = std::fs::write(&script, code) {
        return tool_text(format!("could not write the snippet: {e}"), true);
    }
    let fig = scratch.path("figure.svg");
    let extra = vec!["--emit-figure".to_string(), fig.display().to_string()];
    let out = invoke(cfg, &script, &extra, &scratch.0);
    if !out.ok {
        return tool_text(out.text, true);
    }

    // A script that draws twice made two figures; `--emit-figure` numbers
    // them from the second on.
    let mut parts = Vec::new();
    for n in 1..100 {
        let path = if n == 1 { fig.clone() } else { scratch.path(&format!("figure_{n}.svg")) };
        match std::fs::read_to_string(&path) {
            Ok(svg) => parts.push(if n == 1 { svg } else { format!("\n<!-- figure {n} -->\n{svg}") }),
            Err(_) => break,
        }
    }
    if parts.is_empty() {
        let mut msg = out.text;
        msg.push_str("\nThe script ran but drew nothing: no plotting call produced a figure.");
        return tool_text(msg, true);
    }
    let mut text = String::new();
    if !out.text.trim().is_empty() {
        text.push_str(&out.text);
        text.push('\n');
    }
    text.push_str(&parts.join(""));
    tool_text(text, false)
}

fn run_file(cfg: &Config, args: &Value) -> Result<Value, (i64, String)> {
    let raw = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or((-32602, "`qu_run_file` requires a string `path`".to_string()))?;
    let path = std::fs::canonicalize(raw)
        .map_err(|e| (-32602, format!("`{raw}`: {e}")))?;
    // With a root set, a path outside it is refused rather than clamped:
    // silently running a different file than the one asked for is worse
    // than saying no.
    if let Some(root) = &cfg.root {
        if !path.starts_with(root) {
            return Err((
                -32602,
                format!("`{raw}` is outside the server's root `{}`", root.display()),
            ));
        }
    }
    let mut extra = vec!["--".to_string()];
    if let Some(list) = args.get("args").and_then(Value::as_array) {
        for a in list {
            if let Some(s) = a.as_str() {
                extra.push(s.to_string());
            }
        }
    }
    if extra.len() == 1 {
        extra.clear();
    }
    let cwd = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let out = invoke(cfg, &path, &extra, &cwd);
    let text = if out.text.trim().is_empty() {
        "(ran, printed nothing)".to_string()
    } else {
        out.text
    };
    Ok(tool_text(text, !out.ok))
}

// ── names and documentation ────────────────────────────────────────────

fn builtins(args: &Value) -> Value {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(80) as usize;
    let hits: Vec<&str> = qu_interp::BUILTIN_NAMES
        .iter()
        .copied()
        .filter(|n| query.is_empty() || n.to_lowercase().contains(&query))
        .collect();
    let total = hits.len();
    if total == 0 {
        return tool_text(
            format!(
                "No builtin name contains `{query}`. The engine has {} names in \
                 total; it does not have this one, so a call to it will fail with \
                 `unknown function`.",
                qu_interp::BUILTIN_NAMES.len()
            ),
            false,
        );
    }
    let shown: Vec<&str> = hits.into_iter().take(limit).collect();
    let mut text = if query.is_empty() {
        format!("{total} builtin names:\n")
    } else {
        format!("{total} of {} builtin names contain `{query}`:\n", qu_interp::BUILTIN_NAMES.len())
    };
    text.push_str(&shown.join(", "));
    if total > shown.len() {
        text.push_str(&format!("\n\n[{} more; narrow the query or raise `limit`]", total - shown.len()));
    }
    tool_text(text, false)
}

fn docs(args: &Value) -> Result<Value, (i64, String)> {
    let page = args
        .get("page")
        .and_then(Value::as_str)
        .ok_or((-32602, "`qu_docs` requires a string `page`".to_string()))?;
    let doc = DOCS.iter().find(|(n, _, _)| *n == page).ok_or_else(|| {
        (
            -32602,
            format!(
                "no page `{page}`. Available: {}",
                DOCS.iter().map(|(n, _, _)| *n).collect::<Vec<_>>().join(", ")
            ),
        )
    })?;
    match args.get("search").and_then(Value::as_str) {
        Some(needle) if !needle.trim().is_empty() => Ok(tool_text(sections_matching(doc.2, needle), false)),
        _ => Ok(tool_text(doc.2.to_string(), false)),
    }
}

/// Return the `##`-delimited sections of a document that mention `needle`.
/// The specification is a quarter of a megabyte; handing all of it back to
/// answer one question about `stft` wastes the context the tool exists to
/// protect.
fn sections_matching(body: &str, needle: &str) -> String {
    let needle = needle.to_lowercase();
    let mut out = String::new();
    let mut current = String::new();
    let mut hit = false;
    let mut found = 0;
    for line in body.lines() {
        if line.starts_with("## ") {
            if hit {
                out.push_str(&current);
                out.push('\n');
                found += 1;
            }
            current.clear();
            hit = false;
        }
        if line.to_lowercase().contains(&needle) {
            hit = true;
        }
        current.push_str(line);
        current.push('\n');
    }
    if hit {
        out.push_str(&current);
        found += 1;
    }
    if found == 0 {
        return format!("Nothing in this page mentions `{needle}`.");
    }
    format!("{found} section(s) mention `{needle}`:\n\n{out}")
}

// ── resources ──────────────────────────────────────────────────────────

fn resources_list() -> Value {
    let items: Vec<Value> = DOCS
        .iter()
        .map(|(name, title, body)| {
            json!({
                "uri": format!("qu://docs/{name}"),
                "name": *name,
                "title": *title,
                "mimeType": "text/markdown",
                "size": body.len(),
            })
        })
        .collect();
    json!({ "resources": items })
}

fn resources_read(params: &Value) -> Result<Value, (i64, String)> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or((-32602, "resources/read requires a `uri`".to_string()))?;
    let name = uri
        .strip_prefix("qu://docs/")
        .ok_or_else(|| (-32602, format!("unknown resource `{uri}`")))?;
    let doc = DOCS
        .iter()
        .find(|(n, _, _)| *n == name)
        .ok_or_else(|| (-32602, format!("unknown resource `{uri}`")))?;
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": doc.2 }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            allow_write: false,
            timeout_s: 30.0,
            memory_mb: 512,
            root: None,
            exe: PathBuf::from("qu"),
        }
    }

    #[test]
    fn a_notification_gets_no_reply() {
        let msg = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle(&cfg(), &msg).is_none());
    }

    #[test]
    fn initialize_echoes_a_protocol_version_it_speaks_and_substitutes_one_it_does_not() {
        let mine = initialize(&json!({"protocolVersion": "2024-11-05"}));
        assert_eq!(mine["protocolVersion"], "2024-11-05");
        let theirs = initialize(&json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(theirs["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn every_advertised_tool_is_dispatched() {
        // A tool listed but not handled is invisible until a model calls it
        // and gets `unknown tool`, which is the worst time to find out.
        let specs = tool_specs();
        for spec in specs.as_array().unwrap() {
            let name = spec["name"].as_str().unwrap();
            let err = tools_call(&cfg(), &json!({"name": name, "arguments": {}}));
            if let Err((_, msg)) = err {
                assert!(
                    !msg.starts_with("unknown tool"),
                    "`{name}` is advertised but not dispatched"
                );
            }
        }
    }

    #[test]
    fn searching_the_builtin_table_finds_a_real_name_and_reports_a_missing_one() {
        let hit = builtins(&json!({"query": "polyfit"}));
        assert!(hit["content"][0]["text"].as_str().unwrap().contains("polyfit"));
        let miss = builtins(&json!({"query": "linspace_but_not_really"}));
        assert!(miss["content"][0]["text"].as_str().unwrap().contains("does not have"));
    }

    #[test]
    fn a_documentation_search_returns_only_the_sections_that_mention_the_needle() {
        let body = "# Doc\n\n## Alpha\nnothing here\n\n## Beta\nmentions stft\n";
        let out = sections_matching(body, "stft");
        assert!(out.contains("Beta"));
        assert!(!out.contains("Alpha"));
        assert!(sections_matching(body, "nowhere").contains("Nothing in this page"));
    }

    #[test]
    fn an_unknown_method_is_a_jsonrpc_error_not_a_silent_drop() {
        let msg = json!({"jsonrpc": "2.0", "id": 7, "method": "does/not/exist"});
        let reply = handle(&cfg(), &msg).expect("a request always gets a reply");
        assert_eq!(reply["error"]["code"], -32601);
        assert_eq!(reply["id"], 7);
    }
}
