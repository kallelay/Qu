//! `qu` — the reference command-line driver.
//!
//! Usage:
//!   qu run <file.qu> [--emit-figure <out.svg>] [--emit-vars <out.json>]
//!                       [--emit-data <out.json>]
//!                       parse + execute a script, print its output; with
//!                       --emit-figure, also render whatever figure state
//!                       the script ended up with (even if it never called
//!                       `savefig` itself) to the given SVG path -- see
//!                       `cmd_run`'s doc comment for why this exists; with
//!                       --emit-vars, also dump the script's final top-level
//!                       variable bindings (name/type/short preview) as JSON
//!                       to the given path -- see `cmd_run`'s doc comment;
//!                       with --emit-data, also dump every numeric top-level
//!                       binding (`number`/`bool`/`vector`/`matrix`) as
//!                       UNTRUNCATED JSON to the given path -- see
//!                       `data_to_json`'s doc comment for why this is a
//!                       separate channel from --emit-vars
//!   qu run <file.qu> [--max-time <seconds>] [--max-memory <MB>]
//!                       hard resource caps for this run: exceeding either
//!                       one hard-kills the process (`std::process::exit`,
//!                       not a normal error return) with a message naming
//!                       which limit was hit and the actual measured value
//!                       at the time -- see `resource.rs`'s module doc
//!                       comment for the external-watchdog-thread design
//!                       and its precision/limitations
//!   qu run <file.qu> --sandbox
//!                       denies a fixed capability deny-list (network,
//!                       `python_exec`, file writes) at call time instead
//!                       of running it -- see `qu_interp::Interp::
//!                       check_sandbox`'s doc comment for the exact list
//!                       and why this is NOT a full security boundary on
//!                       its own (pair with --max-time/--max-memory)
//!   qu run <file.qu> --profile [--profile-output <path>]
//!                       reports per-function call count/total wall time/
//!                       time% (aggregated across all calls to that
//!                       function name) plus peak RSS for the run, to
//!                       stderr by default or to --profile-output's path
//!                       -- see `render_profile_report`'s doc comment
//!   qu run <file.qu> --report <path.html>
//!                       ADDITIVE, like --emit-figure/--profile-output --
//!                       normal stdout/exit behavior is unchanged, and
//!                       this additionally writes a self-contained HTML
//!                       report bundling the script's own source, its
//!                       captured output, and any figure it produced,
//!                       styled with the retro window-chrome look (see
//!                       `docs/design/retro-window-chrome.md`). One
//!                       code+output panel pair per `#%%` cell if the
//!                       script uses them, else one pair for the whole
//!                       file; add --profile on the same invocation for
//!                       an optional timing section -- see `report.rs`'s
//!                       module doc comment. The exact same report can
//!                       also be generated FROM INSIDE a running script
//!                       via the `write_report(path)` builtin (see
//!                       `qu_interp::report`'s module doc comment for how
//!                       the two relate and where they differ).
//!   qu parse <file.qu> [--json]
//!                       parse only; report OK or the first error (acceptance).
//!                       With --json, print a machine-readable
//!                       `{"ok":..,"diagnostics":[{"message","line","column"}]}`
//!                       document instead (always exits 0 in this mode; the
//!                       `ok`/`diagnostics` fields carry success/failure) --
//!                       meant for editor integrations (e.g. the VS Code
//!                       extension's inline diagnostics) that need
//!                       structured output rather than the human-readable
//!                       "parse error at LINE:COL: msg" string; see
//!                       `parse_diagnostics_json`'s doc comment
//!   qu run <file.qu> -- <args...>
//!                       everything after a bare `--` is the SCRIPT's, not
//!                       qu's, and is readable inside it as `argv()`. Pair
//!                       with `getenv(name)` and `now()` for scripts driven
//!                       by a cron job, a Makefile or a parameter sweep
//!   qu build <file.qu> [-o <output>]
//!                       "hard compile" (see BACKLOG.md's soft/hard compile
//!                       distinction) — produces a standalone executable
//!                       that runs the script with no `qu` install or
//!                       source file needed at distribution time. Built by
//!                       copying the CURRENTLY RUNNING `qu` binary
//!                       (`std::env::current_exe()`) and appending the
//!                       script's bytes plus a small magic+length footer;
//!                       at startup `qu` (any `qu`, this file's own `run()`)
//!                       checks its own executable for that footer before
//!                       doing anything else, and if present runs the
//!                       embedded script directly instead of parsing argv
//!                       as a normal `qu` invocation. This is the standard
//!                       self-extracting-binary trick (same idea as an NSIS/
//!                       WinRAR SFX installer): appending bytes after a
//!                       PE/ELF image doesn't touch either format's own
//!                       header, so the OS loader ignores the trailer and
//!                       loads/runs the binary exactly as it would without
//!                       it. Every argv entry after the executable name is
//!                       forwarded to the embedded script's `argv()`, the
//!                       same as `qu run script.qu -- args...`'s args (see
//!                       `run_embedded_script`'s doc comment) — a built
//!                       binary needs no `--` since it has no `qu` flags of
//!                       its own to disambiguate against.
//!   qu tokens <file.qu>  dump the token stream (lexer debugging)
//!   qu ast <file.qu>     dump the parsed AST (parser debugging)
//!   qu repl               read-eval-print loop over stdin
//!   qu repl <file.qu>     run the script, keep its interpreter and top-level
//!                       bindings alive, then drop into the same REPL loop
//!                       -- Python's `python -i script.py`, or a notebook's
//!                       "run all cells, kernel stays alive". Plain `qu run
//!                       <file.qu>` still exits the moment the script ends;
//!                       this is the one command that keeps going.
//!   qu eval "<src>"      run a one-liner
//!   qu diary <file.qu> [-o out.html]   run a script, export a Jupyter-style
//!                       HTML transcript (code block + printed text/plot per
//!                       top-level statement, in source order)
//!   qu docs --json      dump the builtin reference table as JSON, for a
//!                       host (e.g. QuStudio) that wants it without linking
//!                       qu-interp or running an interpreter
//!   qu kernel            persistent-interpreter mode for a programmatic
//!                       host (QuStudio's Code editor "Run" button): reads
//!                       one line-delimited JSON request per line from
//!                       stdin (`{"op":"run","code":".."}` or
//!                       `{"op":"restart"}`), keeps ONE `qu_interp::Interp`
//!                       alive across requests (so state persists across
//!                       "run" calls the way a Jupyter kernel's does), and
//!                       writes exactly one JSON response line per request
//!                       to stdout -- see `cmd_kernel`'s doc comment for the
//!                       full protocol. Strictly additive: no existing
//!                       subcommand's behavior changes, and nothing but
//!                       `cmd_kernel` itself reads or writes this protocol.
//!
//! Kept intentionally thin: everything real lives in the library crates so the
//! same core can later be driven from a WASM/browser front-end.

mod diary;
mod gui;
mod mcp;
mod report;
mod resource;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // `qu-interp`'s `call_builtin` is one enormous match (hundreds of
    // builtin arms) that recurses into itself for several tensor-aware
    // ops (`relu`/`tensor_math1`/`tensor_reduce` all call back into
    // `call_builtin` on the untracked inner value — see their own doc
    // comments; each call terminates in one extra frame, this is not an
    // infinite loop). In an unoptimized debug build that one function's
    // own stack frame is large enough that even a SINGLE such
    // recursion, on top of the tree-walking evaluator's ordinary
    // expression/statement recursion, can exhaust the OS's default 1MB
    // main-thread stack on Windows — reproduced by a two-line script
    // (`sequential(...)` + one `.forward(x)`/`.fit(...)` call) while
    // verifying the model-zoo constructors (`mlp_classifier`/
    // `simple_cnn`/`simple_rnn_classifier`, 2026-08-25), which hit this
    // through the just-landed `sequential`/`forward`/`fit` machinery.
    // Release builds inline/optimize this away, but debug builds (the
    // default for `cargo run`/local iteration) shouldn't crash on
    // ordinary, small scripts. Fixed the standard way: run the actual
    // work on a dedicated thread with a much larger (64 MiB) stack,
    // rather than trying to slim down `call_builtin` itself (a much
    // bigger, riskier refactor of shared, actively-edited code).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .expect("failed to spawn main worker thread")
        .join()
        .expect("main worker thread panicked")
}

fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `qu build` appends a script + footer to a copy of this very binary
    // (see `cmd_build`'s doc comment). Checking for that footer here, before
    // any normal argv dispatch, is what makes the copy self-running: a
    // built binary's argv never contains `run`/`parse`/etc — every entry is
    // the SCRIPT's own argument. This costs one small seek+read at the tail
    // of the current executable on every ordinary `qu` invocation too (a
    // plain `qu` binary has no footer, so this is a fast negative), which is
    // cheap next to the process startup it already pays.
    if let Ok(exe_path) = std::env::current_exe() {
        // Bundle format first (what `qu build` writes today), then the
        // older single-script footer, so a binary built by an earlier `qu`
        // keeps running unchanged.
        let embedded = read_embedded_bundle(&exe_path)
            .map(Embedded::Bundle)
            .or_else(|| read_embedded_script(&exe_path).map(Embedded::Script));
        if let Some(embedded) = embedded {
            let result = match embedded {
                Embedded::Bundle(b) => run_embedded_bundle(&exe_path, b, args),
                Embedded::Script(s) => run_embedded_script(&exe_path, s, args),
            };
            return match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("qu: {msg}");
                    ExitCode::FAILURE
                }
            };
        }
    }

    let cmd = args.first().map(String::as_str).unwrap_or("help");

    let result = match cmd {
        "run" => cmd_run(&args[1..]),
        "build" => cmd_build(&args[1..]),
        "gui" => return gui::run(&args[1..]),
        "parse" => cmd_parse(&args[1..]),
        "tokens" => cmd_tokens(arg(&args, 1)),
        "ast" => cmd_ast(arg(&args, 1)),
        "eval" => cmd_eval(arg(&args, 1)),
        "diary" => cmd_diary(&args[1..]),
        "docs" => cmd_docs(&args[1..]),
        "repl" => cmd_repl(&args[1..]),
        "kernel" => cmd_kernel(),
        "mcp" => mcp::cmd_mcp(&args[1..]),
        "version" | "--version" | "-V" => {
            println!("qu {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        // Printing the banner is the right answer to "no arguments" and
        // to an explicit request, and both must keep exiting 0.
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        // Anything else is a MISTAKE, and used to be answered with the
        // banner and `Ok(())` -- exit 0. So `qu -e '...'`, `qu frobnicate`
        // and a bare `qu script.qu` (forgetting `run`) all passed green.
        // In a Makefile or a CI step that is a command that did nothing
        // and reported success, which is worse than a crash: nothing
        // downstream has any reason to look.
        other => {
            let hint = if other.ends_with(".qu") || std::path::Path::new(other).is_file() {
                // Not a typo -- a real file and a real intention, so name
                // the command they meant rather than listing all of them.
                format!("did you mean `qu run {other}`?")
            } else {
                "`qu help` lists the commands".to_string()
            };
            Err(format!("unknown command `{other}` -- {hint}"))
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("qu: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// What a built binary found appended to itself, if anything.
enum Embedded {
    Bundle(Vec<(String, String)>),
    Script(Vec<u8>),
}

/// Magic marker for a `qu build` BUNDLE footer — the current format.
///
/// A bundle carries the entry script *and every `.qu` file it transitively
/// imports*, so the built binary is self-contained. `BUILD_FOOTER_MAGIC`
/// below is the older single-script format, still read (never written) so
/// a binary built by an older `qu` keeps running.
const BUNDLE_FOOTER_MAGIC: &[u8; 8] = b"QUBUNDL\0";

/// Serialise `files` as the bundle payload: a count, then per entry a
/// length-prefixed forward-slash key and a length-prefixed source.
///
/// Hand-rolled rather than JSON because the reader runs on EVERY ordinary
/// `qu` startup (the footer probe below), and because sources are
/// arbitrary UTF-8 that a JSON escape pass would have to rewrite twice for
/// no gain. All lengths are little-endian and fixed-width, so the format
/// is byte-identical whatever platform builds it — which is what lets a
/// bundle built on Windows be read by a Linux stub later.
fn encode_bundle(files: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(files.len() as u64).to_le_bytes());
    for (key, src) in files {
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&(src.len() as u64).to_le_bytes());
        out.extend_from_slice(src.as_bytes());
    }
    out
}

/// Inverse of `encode_bundle`. Returns `None` for anything malformed
/// rather than panicking: this parses bytes found at the tail of an
/// executable, which may be a foreign file that merely happens to end in
/// the magic. Every length is bounds-checked against what is actually
/// left, so a truncated or hostile payload cannot over-read.
fn decode_bundle(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    fn take<'a>(b: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        if b.len() < n {
            return None;
        }
        let (head, rest) = b.split_at(n);
        *b = rest;
        Some(head)
    }
    fn take_u64(b: &mut &[u8]) -> Option<u64> {
        Some(u64::from_le_bytes(take(b, 8)?.try_into().ok()?))
    }
    let mut b = bytes;
    let count = take_u64(&mut b)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let klen = take_u64(&mut b)? as usize;
        let key = String::from_utf8(take(&mut b, klen)?.to_vec()).ok()?;
        let slen = take_u64(&mut b)? as usize;
        let src = String::from_utf8(take(&mut b, slen)?.to_vec()).ok()?;
        out.push((key, src));
    }
    Some(out)
}

/// Walk the transitive `import "..."` graph from `entry`, returning the
/// entry script FIRST and then every file it reaches, each keyed relative
/// to the entry's own directory.
///
/// Uses the real lexer and matches exactly what `qu-syntax` accepts as a
/// file import — `import` (a true keyword) immediately followed by a
/// `Str` token. Deliberately NOT an AST walk: `import` is an ordinary
/// statement, so it can sit inside an `if` or a loop body, and a
/// top-level-only walk would silently under-bundle exactly the case this
/// whole change exists to fix. Scanning tokens over-approximates instead
/// (it would also follow an import in dead code), and over-bundling is
/// harmless where under-bundling ships a broken binary.
///
/// `RawStr` is not matched, because the parser does not accept it either:
/// `import r"x.qu"` is read as a *native module name* and errors. Matching
/// it here would bundle a file the interpreter will never ask for.
fn collect_bundle(entry: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    use qu_lexer::Tok;
    let root = entry
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let entry_key = entry
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("{} has no file name", entry.display()))?;

    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // (path on disk, key, directory its own imports resolve against)
    let mut queue: Vec<(std::path::PathBuf, String)> = vec![(entry.to_path_buf(), entry_key)];

    while let Some((path, key)) = queue.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let toks = qu_lexer::lex(&src);
        for w in toks.windows(2) {
            let imported = match (&w[0].tok, &w[1].tok) {
                (Tok::Keyword("import"), Tok::Str(p)) => p,
                _ => continue,
            };
            let asked = std::path::Path::new(imported);
            let full = if asked.is_relative() { dir.join(asked) } else { asked.to_path_buf() };
            // The SAME function the interpreter will use at run time to
            // turn a resolved path back into a key. Sharing it is the
            // point: a build-time key and a run-time key computed by two
            // different rules would miss, and miss silently.
            let Some(k) = qu_interp::Interp::bundle_key(&root, &full) else {
                return Err(format!(
                    "import \"{imported}\" in {} resolves to {}, which is not reachable \
                     from the project directory {} -- move it under the project, or \
                     build from a directory that contains both",
                    path.display(),
                    full.display(),
                    root.display()
                ));
            };
            queue.push((full, k));
        }
        out.push((key, src));
    }

    // `out` came off a stack, so the entry script is not necessarily first
    // -- and the runtime reads entry-first. Put it back where it belongs.
    let entry_pos = out
        .iter()
        .position(|(k, _)| Some(k.as_str()) == entry.file_name().and_then(|n| n.to_str()))
        .unwrap_or(0);
    out.swap(0, entry_pos);
    Ok(out)
}

/// Magic marker for a `qu build` footer — see `cmd_build`'s doc comment.
/// 8 bytes, so it sits at a fixed offset from EOF alongside the following
/// 8-byte little-endian script length (16-byte footer total).
const BUILD_FOOTER_MAGIC: &[u8; 8] = b"QUBUILD\0";

/// If `exe_path` ends with a `qu build` footer, returns the embedded
/// script's source bytes. Reads only the trailing 16 bytes first (magic +
/// length) and, on a match, seeks back exactly `length` more bytes to read
/// the script — never reads a non-footer file's contents beyond that
/// initial 16-byte probe. Returns `None` (not an error) for anything that
/// doesn't look like a built binary: too short, wrong magic, or a length
/// that doesn't fit before the footer — a plain `qu` executable and a
/// corrupt/foreign file are treated the same way, since neither is a
/// footer this build produced.
fn read_embedded_script(exe_path: &std::path::Path) -> Option<Vec<u8>> {
    read_footer_payload(exe_path, BUILD_FOOTER_MAGIC)
}

/// The bundle `qu build` appended to this binary, if it is a built one.
/// Same 16-byte footer shape as the legacy single-script format, just a
/// different magic and a structured payload -- see `decode_bundle`.
fn read_embedded_bundle(exe_path: &std::path::Path) -> Option<Vec<(String, String)>> {
    decode_bundle(&read_footer_payload(exe_path, BUNDLE_FOOTER_MAGIC)?)
}

fn read_footer_payload(exe_path: &std::path::Path, magic: &[u8; 8]) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(exe_path).ok()?;
    let file_len = f.metadata().ok()?.len();
    const FOOTER_LEN: u64 = 16;
    if file_len < FOOTER_LEN {
        return None;
    }
    f.seek(SeekFrom::End(-(FOOTER_LEN as i64))).ok()?;
    let mut footer = [0u8; FOOTER_LEN as usize];
    f.read_exact(&mut footer).ok()?;
    if &footer[0..8] != magic {
        return None;
    }
    let script_len = u64::from_le_bytes(footer[8..16].try_into().ok()?);
    if script_len > file_len - FOOTER_LEN {
        return None;
    }
    f.seek(SeekFrom::End(-((FOOTER_LEN + script_len) as i64))).ok()?;
    let mut script = vec![0u8; script_len as usize];
    f.read_exact(&mut script).ok()?;
    Some(script)
}

/// Runs a script embedded in the currently-running executable by `qu
/// build` — the same execution path `qu run` uses (`Interp::new` + `run`),
/// just fed source bytes read out of this binary's own trailer instead of
/// a `--emit-figure`/`--profile`/etc-aware argv parse. A built binary keeps
/// none of `qu run`'s own flags: every argv entry is the script's, exactly
/// as if it had been passed after `qu run script.qu --`, since a built
/// binary has no need to disambiguate its own options from the script's
/// (it doesn't parse `--emit-figure` and friends at all). `set_script_path`
/// is given the EXE's own path, not a source file's — imports in a built
/// script resolve next to the binary, which is the only "beside the
/// script" location that still exists once distributed.
fn run_embedded_bundle(
    exe_path: &std::path::Path,
    bundle: Vec<(String, String)>,
    script_args: Vec<String>,
) -> Result<(), String> {
    let mut files = bundle;
    if files.is_empty() {
        return Err("embedded bundle is empty".into());
    }
    // Entry first, by construction in `collect_bundle`.
    let (_, entry_src) = files.remove(0);
    let root = exe_path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut it = qu_interp::Interp::new();
    it.script_args = script_args;
    // Still the exe's own directory: an `import` that is NOT in the bundle
    // (added beside the binary after the fact) keeps resolving from disk
    // exactly as before. The bundle is an overlay, not a replacement.
    it.set_script_path(exe_path);
    it.set_bundled_sources(&root, files.into_iter().collect());
    it.run(&entry_src).map_err(|e| e.to_string())?;
    print!("{}", it.out);
    io::stdout().flush().ok();
    Ok(())
}

fn run_embedded_script(exe_path: &std::path::Path, script_bytes: Vec<u8>, script_args: Vec<String>) -> Result<(), String> {
    let src = String::from_utf8(script_bytes)
        .map_err(|e| format!("embedded script is not valid UTF-8: {e}"))?;
    let mut it = qu_interp::Interp::new();
    it.script_args = script_args;
    it.set_script_path(exe_path);
    it.run(&src).map_err(|e| e.to_string())?;
    print!("{}", it.out);
    io::stdout().flush().ok();
    Ok(())
}

/// `qu build <file.qu> [-o <output>]` — see this file's module doc comment
/// for the overall self-extracting-binary design. Copies the CURRENTLY
/// RUNNING `qu` executable (not a rebuild — no cargo/toolchain needed at
/// distribution time, which is the whole point next to a per-script
/// wrapper crate) and appends the script plus a magic+length footer that
/// `run()`'s `read_embedded_script` check recognizes on the built binary's
/// own next startup.
fn cmd_build(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                output = args.get(i).map(String::as_str);
                if output.is_none() {
                    return Err("-o/--output expects a path argument".into());
                }
            }
            other => {
                if path.is_none() {
                    path = Some(other);
                }
            }
        }
        i += 1;
    }
    let path = path.ok_or("expected a file path")?;
    // Every `.qu` the entry script transitively imports travels WITH it.
    // Embedding only the entry script (the previous behaviour) produced a
    // binary that ran correctly in its own source directory and failed
    // anywhere else -- a build that reports success on the build machine
    // and breaks on the target machine.
    let bundle = collect_bundle(std::path::Path::new(path))?;
    let module_count = bundle.len() - 1;
    let payload = encode_bundle(&bundle);

    // `current_exe()` here is always a plain (footer-less) `qu` binary:
    // `run()` checks for a footer before argv dispatch ever reaches this
    // function, so a built binary can never invoke `cmd_build` on itself —
    // its own argv is entirely the embedded script's, not `qu`'s subcommand
    // dispatch. Nothing to guard against here as a result.
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("cannot locate the running qu executable: {e}"))?;
    let stub = std::fs::read(&exe_path)
        .map_err(|e| format!("cannot read {}: {e}", exe_path.display()))?;

    let output_path = output.map(String::from).unwrap_or_else(|| default_build_output_path(path));

    let mut out_bytes = stub;
    out_bytes.extend_from_slice(&payload);
    out_bytes.extend_from_slice(BUNDLE_FOOTER_MAGIC);
    out_bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());

    std::fs::write(&output_path, &out_bytes)
        .map_err(|e| format!("cannot write {output_path}: {e}"))?;

    // The copied bytes came from an existing exec bit on `exe_path` on
    // Unix, but `fs::write` creates the new file with the process's default
    // (umask-limited) mode, which usually lacks +x — set it explicitly so
    // the freshly built binary is runnable without a manual `chmod`.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&output_path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o755);
            let _ = std::fs::set_permissions(&output_path, perms);
        }
    }

    // Naming the module count is not decoration: it is the one number
    // that tells the user whether their `import`s were actually picked up.
    match module_count {
        0 => println!("wrote {output_path}"),
        1 => println!("wrote {output_path} (1 imported module bundled)"),
        n => println!("wrote {output_path} ({n} imported modules bundled)"),
    }

    // Say WHAT was embedded, not just that something was. The stub is a
    // copy of the running `qu`, so the artifact silently inherits this
    // binary's optimisation level and compiled-in features -- two choices
    // the user never made here and cannot see by looking at the result.
    println!("  interpreter: {}", stub_profile());
    if cfg!(debug_assertions) {
        // A warning rather than a note: the difference is large, it is
        // invisible in the artifact, and it is inherited from whichever
        // `qu` happened to be on PATH rather than chosen.
        //
        // Measured 2026-09-16, SAME tree and commit, the two binaries
        // built minutes apart, interleaved in one window, best-of-3 on a
        // 4M-iteration loop: total wall 664 ms debug vs 392 ms release
        // (~1.7x); with process startup subtracted, the loop work itself
        // is 519 ms vs 83 ms. Quote the range, not a single figure --
        // release's own startup measured consistently HIGHER (259-308 ms
        // vs 144-179 ms), which is unexplained and makes the subtracted
        // number the noisier of the two. An earlier cross-vintage
        // measurement of the same thing gave 438/237 and is superseded by
        // this one; on this shared box absolute timings only mean
        // anything against a baseline taken in the same window.
        eprintln!("  warning: this is a DEBUG build of qu, so the artifact ships a debug");
        eprintln!("           interpreter and will run markedly slower. Build with a");
        eprintln!("           release qu (bash tools/build_qu.sh) to avoid this.");
    }
    Ok(())
}

/// Human-readable description of the stub this `qu` would copy: its
/// optimisation level and the optional modules compiled into it.
///
/// `qu build` copies `std::env::current_exe()`, so these are properties of
/// the RUNNING binary, fixed at its own compile time -- which is why
/// `cfg!` answers them correctly here, and why the artifact cannot differ
/// from what this reports.
fn stub_profile() -> String {
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let mut features: Vec<&str> = Vec::new();
    if cfg!(feature = "xlsx") {
        features.push("xlsx");
    }
    if cfg!(feature = "codec") {
        features.push("codec");
    }
    if cfg!(feature = "gpu") {
        features.push("gpu");
    }
    if cfg!(feature = "backend-torch") {
        features.push("backend-torch");
    }
    if cfg!(feature = "h5-models") {
        features.push("h5-models");
    }
    if cfg!(feature = "llm") {
        features.push("llm");
    }
    let feat = if features.is_empty() { "none".to_string() } else { features.join(", ") };
    // Naming an ABSENT capability that people actually reach for matters
    // as much as naming the present ones: `gpu` is off by default, so a
    // user wondering why their built tool has no GPU path needs to see it
    // was never compiled in, rather than assume a runtime fallback.
    let gpu_note = if cfg!(feature = "gpu") { "" } else { " (no gpu)" };
    format!("qu {} {profile}, features: {feat}{gpu_note}", env!("CARGO_PKG_VERSION"))
}

/// Default `qu build` output path: `<dir>/name.qu` -> `<dir>/name.exe` on
/// Windows, `<dir>/name` elsewhere — mirrors `default_diary_path`'s same
/// dir-preserving, extension-swapping shape.
fn default_build_output_path(input: &str) -> String {
    let p = std::path::Path::new(input);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("a");
    let base = match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => format!("{}/{stem}", dir.display()),
        _ => stem.to_string(),
    };
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base
    }
}

fn arg(args: &[String], i: usize) -> Option<&str> {
    args.get(i).map(String::as_str)
}

fn read_file(path: Option<&str>) -> Result<String, String> {
    let path = path.ok_or("expected a file path")?;
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

/// `fig.svg` + 2 -> `fig_2.svg`. Keeps the extension where it belongs
/// rather than appending to the end, so the result is still recognisably
/// an SVG to everything downstream (the app globs by extension).
/// A path with no extension just gets the suffix.
fn numbered_path(path: &str, n: usize) -> String {
    match path.rfind('.') {
        // `rfind` on the whole path would treat `./out/fig` as having the
        // extension `/fig`; only a dot in the final component counts.
        Some(dot) if !path[dot..].contains(['/', '\\']) => {
            format!("{}_{}{}", &path[..dot], n, &path[dot..])
        }
        _ => format!("{path}_{n}"),
    }
}

/// `qu run <file.qu> [--emit-figure <out.svg>]`.
///
/// Plain `qu run` never wrote anything to disk for `plot`/`scatter`/etc —
/// only an explicit `savefig(path)` call does that (verified directly: a
/// script that calls `plot()`/`show` and nothing else produces zero files,
/// see BACKLOG.md). That's a deliberate, honest default for the CLI: a
/// batch script shouldn't silently start writing image files nobody asked
/// for just because it happened to call `plot`.
///
/// QuStudio (the Tauri IDE) needs different behavior: a user who writes
/// `plot(x, y)` and hits Run expects to *see* a plot, the same way MATLAB's
/// desktop or a Jupyter cell would, without also having to remember a
/// trailing `savefig(...)`. Rather than have the IDE's execution wrapper
/// inject a fabricated `savefig(...)` call into the user's source (fragile:
/// wrong if the real last statement is inside a block, a comment, or after
/// a `return`, and surprising if the script legitimately ends with its own
/// unrelated trailing expression), `--emit-figure <path>` asks the
/// interpreter itself, after running, whether *any* figure was touched
/// (`it.figures > 0` -- incremented by `plot`/`scatter`/`bar`/`imshow`/...
/// at their own call sites) and if so renders its current state with the
/// exact same `plotting::render_svg` used by `savefig(".svg")` and by the
/// `qu diary` command (see `diary.rs`, which established this "figure
/// changed => render it" convention first, per-statement, for the HTML
/// transcript feature). This is opt-in and additive: plain `qu run` with no
/// flag keeps its exact prior behavior (checked by the existing
/// `cmd_run`/acceptance tests), so nothing changes for non-Studio callers.
fn cmd_run(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut emit_figure: Option<&str> = None;
    let mut emit_vars: Option<&str> = None;
    let mut emit_data: Option<&str> = None;
    let mut max_time: Option<f64> = None;
    let mut max_memory: Option<u64> = None;
    let mut sandbox = false;
    let mut profile = false;
    let mut profile_output: Option<&str> = None;
    let mut report_path: Option<&str> = None;
    let mut live = false;
    let mut emit_ui: Option<&str> = None;
    let mut ui_values: Option<&str> = None;
    // Everything after a bare `--`, handed to the script as `argv()`.
    let mut script_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        // `--` ends qu's own options: everything after it belongs to the
        // script. Using the standard separator means a script argument can
        // safely be named `--emit-figure` or start with a dash at all,
        // which a positional-only scheme could not allow.
        if args[i] == "--" {
            script_args = args[i + 1..].to_vec();
            break;
        }
        match args[i].as_str() {
            "--emit-figure" => {
                i += 1;
                emit_figure = args.get(i).map(String::as_str);
                if emit_figure.is_none() {
                    return Err("--emit-figure expects a path argument".into());
                }
            }
            "--emit-vars" => {
                i += 1;
                emit_vars = args.get(i).map(String::as_str);
                if emit_vars.is_none() {
                    return Err("--emit-vars expects a path argument".into());
                }
            }
            "--emit-data" => {
                i += 1;
                emit_data = args.get(i).map(String::as_str);
                if emit_data.is_none() {
                    return Err("--emit-data expects a path argument".into());
                }
            }
            "--max-time" => {
                i += 1;
                let raw = args.get(i).ok_or("--max-time expects a number of seconds")?;
                max_time = Some(
                    raw.parse::<f64>()
                        .map_err(|_| format!("--max-time expects a number of seconds, got `{raw}`"))?,
                );
            }
            "--max-memory" => {
                i += 1;
                let raw = args.get(i).ok_or("--max-memory expects a number of megabytes")?;
                max_memory = Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("--max-memory expects a number of megabytes, got `{raw}`"))?,
                );
            }
            "--sandbox" => sandbox = true,
            "--profile" => profile = true,
            "--profile-output" => {
                i += 1;
                profile_output = args.get(i).map(String::as_str);
                if profile_output.is_none() {
                    return Err("--profile-output expects a path argument".into());
                }
            }
            "--report" => {
                i += 1;
                report_path = args.get(i).map(String::as_str);
                if report_path.is_none() {
                    return Err("--report expects a path argument".into());
                }
            }
            // `--live` -- stream each `print`/`disp`/`writeline`/`echo`/
            // `write` call to real stdout THE MOMENT it happens, instead of
            // the normal "accumulate into `it.out`, print it all at once
            // after `run` returns" behavior (see `Interp::on_print`'s own
            // doc comment for why that distinction matters: a script with a
            // real infinite loop -- e.g. `serial_open(...)` + `while true`
            // + `print(sample)`, QuStudio's live serial-plotter feature --
            // would otherwise never produce any visible output at all,
            // since `run` never returns). Skips the final bulk `print!(
            // "{}", it.out)` below to avoid printing everything twice.
            "--live" => live = true,
            // `--emit-ui <path>` / `--ui-values <path>` -- the two halves of
            // the immediate-mode GUI loop (see `qu_interp::UiWidget`): the
            // script DECLARES its controls by calling `ui_*` builtins as it
            // runs, `--emit-ui` writes those declarations out for a host to
            // render, and `--ui-values` feeds the host's current values back
            // in on the next run. A script run with neither still works --
            // every control just returns its own default.
            "--emit-ui" => {
                i += 1;
                emit_ui = args.get(i).map(String::as_str);
                if emit_ui.is_none() {
                    return Err("--emit-ui expects a path argument".into());
                }
            }
            "--ui-values" => {
                i += 1;
                ui_values = args.get(i).map(String::as_str);
                if ui_values.is_none() {
                    return Err("--ui-values expects a path argument".into());
                }
            }
            other => {
                if path.is_none() {
                    path = Some(other);
                }
            }
        }
        i += 1;
    }

    let src = read_file(path)?;

    // Start the time/memory watchdog (see `resource.rs`'s module doc
    // comment for why this is an external polling thread) before the
    // script runs at all. `track_peak` also samples RSS purely for
    // `--profile`'s report even when neither hard limit is set. When none
    // of the three apply, `ResourceMonitor::start` spawns no thread and
    // this whole feature costs nothing.
    let monitor = resource::ResourceMonitor::start(max_time, max_memory, profile);

    let mut it = qu_interp::Interp::new();
    it.script_args = script_args;
    if sandbox {
        it.set_sandboxed(true);
    }
    if profile {
        it.set_profiling(true);
    }
    if live {
        it.on_print = Some(Box::new(|s: &str| {
            print!("{s}");
            io::stdout().flush().ok();
        }));
    }
    if let Some(path) = ui_values {
        // Must land BEFORE the run: the `ui_*` builtins read these as they
        // execute. A missing/malformed file is a warning, not a failure --
        // the script then simply runs on its own declared defaults, which
        // is the same thing that happens with no host at all.
        match std::fs::read_to_string(path) {
            Ok(text) => {
                for (k, v) in parse_ui_values(&text) {
                    it.ui_values.insert(k, v);
                }
            }
            Err(e) => eprintln!("qu: warning: --ui-values could not read `{path}`: {e}"),
        }
    }

    let run_start = std::time::Instant::now();
    // `--report` needs real per-`#%%`-cell output attribution, which
    // requires controlling execution cell-by-cell from the start (see
    // `report.rs`'s module doc comment) rather than one `it.run(&src)`
    // call. Without `--report` (the overwhelming majority of `qu run`
    // invocations), behavior is byte-identical to before this feature
    // existed — a single `it.run(&src)` call, no extra allocation or
    // parsing pass.
    // The script's own imports resolve beside the script, so a tool can be
    // run from any directory and still find the module next to it.
    if let Some(script) = path {
        it.set_script_path(std::path::Path::new(script));
    }
    let mut report_cells: Vec<qu_interp::report::ReportCell> = Vec::new();
    let run_result: Result<(), String> = if report_path.is_some() {
        let (cells, res) = report::run_cells(&mut it, &src);
        report_cells = cells;
        res
    } else {
        it.run(&src).map_err(|e| e.to_string())
    };
    let run_elapsed = run_start.elapsed();
    let peak_rss = monitor.stop_and_join();

    if !live {
        print!("{}", it.out);
        io::stdout().flush().ok();
    }

    // Dispatch-phase breakdown (`QU_DPROF=1`, see `qu_interp::dprof`).
    // stderr, so it can never contaminate a script's own stdout; the
    // empty string when the variable is unset.
    eprint!("{}", qu_interp::dprof::report());

    if let Some(fig_path) = emit_figure {
        // Every figure the script produced, not just the last one. A script
        // that draws, says `show`, then draws again has made TWO figures;
        // emitting only the live one meant the earlier ones were computed
        // and then silently dropped. Finalized figures come from
        // `figure_history` (pushed by `show` and by the `figure()`
        // builtin); the still-live one is emitted too when it has content.
        //
        // The first file keeps the exact path asked for, so every existing
        // caller that passes `fig.svg` and then reads `fig.svg` is
        // unaffected. Subsequent ones get `fig_2.svg`, `fig_3.svg`, ...
        let mut figures: Vec<&qu_interp::plotting::Figure> = it.figure_history.iter().collect();
        if !it.figure.is_pristine() {
            figures.push(&it.figure);
        }
        // `it.figures` counts `show` commands; a script can also plot
        // without ever saying `show`, which is why the emptiness check
        // above is what actually decides, not the counter.
        for (n, fig) in figures.iter().enumerate() {
            let path = if n == 0 {
                fig_path.to_string()
            } else {
                numbered_path(fig_path, n + 1)
            };
            // Publication figures embed their fonts (see the `savefig`
            // builtin for why): otherwise the LaTeX typeface only shows up
            // on machines that happen to have Latin Modern installed.
            let svg = qu_interp::plotting::render_svg(fig, fig.width, fig.height, fig.publication);
            if let Err(e) = std::fs::write(&path, svg) {
                eprintln!("qu: warning: --emit-figure could not write `{path}`: {e}");
            }
        }
    }

    if let Some(vars_path) = emit_vars {
        let json = vars_to_json(&it);
        if let Err(e) = std::fs::write(vars_path, json) {
            eprintln!("qu: warning: --emit-vars could not write `{vars_path}`: {e}");
        }
    }

    if let Some(data_path) = emit_data {
        let json = data_to_json(&it);
        if let Err(e) = std::fs::write(data_path, json) {
            eprintln!("qu: warning: --emit-data could not write `{data_path}`: {e}");
        }
    }

    if let Some(ui_path) = emit_ui {
        let json = ui_to_json(&it);
        if let Err(e) = std::fs::write(ui_path, json) {
            eprintln!("qu: warning: --emit-ui could not write `{ui_path}`: {e}");
        }
    }

    if profile {
        let report = render_profile_report(&it, run_elapsed, peak_rss);
        match profile_output {
            Some(out_path) => {
                if let Err(e) = std::fs::write(out_path, &report) {
                    eprintln!("qu: warning: --profile-output could not write `{out_path}`: {e}");
                }
            }
            None => {
                eprint!("{report}");
            }
        }
    }

    if let Some(out_path) = report_path {
        let title = report::script_title(path.unwrap_or("script"));
        if let Err(e) = report::write_report(
            out_path,
            &title,
            &report_cells,
            &it,
            profile,
            run_elapsed,
            peak_rss,
            run_result.as_ref().err().map(String::as_str),
        ) {
            eprintln!("qu: warning: --report could not write `{out_path}`: {e}");
        }
    }

    run_result
}

/// `qu run --profile`'s report: total wall-clock time and peak RSS for the
/// whole run, then a table of every user function `call_tracked` timed
/// (see `qu_interp::Interp::profile_stats`'s own doc comment for exactly
/// what's measured — inclusive wall time per function name, aggregated
/// across every call to that name; sorted here by total time descending so
/// the hottest function is always first, matching "call count/total time/
/// time %" being the ask, not a flame graph or any deeper attribution).
/// `peak_rss` is `None` when `current_rss_bytes` isn't supported on this
/// platform (see `resource.rs`) — reported honestly as "unavailable"
/// rather than a fabricated `0 MB`.
fn render_profile_report(it: &qu_interp::Interp, total_elapsed: std::time::Duration, peak_rss: Option<u64>) -> String {
    let mut stats = it.profile_stats();
    stats.sort_by(|a, b| b.2.cmp(&a.2));

    let mut out = String::new();
    out.push_str("qu profile\n");
    out.push_str(&format!("  wall-clock time : {:.3}s\n", total_elapsed.as_secs_f64()));
    match peak_rss {
        Some(bytes) => out.push_str(&format!("  peak RSS        : {} MB\n", bytes / (1024 * 1024))),
        None => out.push_str("  peak RSS        : unavailable on this platform\n"),
    }
    out.push('\n');

    if stats.is_empty() {
        out.push_str("  (no user-function calls were tracked)\n");
        return out;
    }

    let total_secs = total_elapsed.as_secs_f64().max(1e-12);
    out.push_str(&format!(
        "  {:<28} {:>8} {:>12} {:>8}\n",
        "function", "calls", "total time", "time %"
    ));
    for (name, calls, dur) in stats {
        let secs = dur.as_secs_f64();
        let pct = 100.0 * secs / total_secs;
        out.push_str(&format!(
            "  {:<28} {:>8} {:>11.3}s {:>7.1}%\n",
            name, calls, secs, pct
        ));
    }
    out
}

/// Renders `it`'s final top-level variable bindings (see
/// `Interp::global_bindings`, the same accessor the REPL's `:vars`/`:whos`
/// meta-command already uses) as a JSON array of `{name, type, preview}`
/// objects, for `--emit-vars`. `type` is `Value::type_name()` verbatim
/// (already a short, stable word like "number"/"vector"/"matrix"/"table"/
/// "model" — no separate tagging scheme invented here) and `preview` is
/// `display_value()` (the same summarizer the REPL already prints), capped
/// at a further byte length here purely as a defensive backstop in case a
/// future `Value` variant's `display_value` arm doesn't already truncate
/// the way `Vec`/`Mat`/`CVec`/`CMat` do today.
fn vars_to_json(it: &qu_interp::Interp) -> String {
    const MAX_PREVIEW: usize = 200;
    let mut bindings: Vec<(&str, &qu_interp::Value)> = it.global_bindings().collect();
    bindings.sort_by(|a, b| a.0.cmp(b.0));

    let mut out = String::from("[");
    for (i, (name, v)) in bindings.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let mut preview = qu_interp::display_value(v);
        if preview.len() > MAX_PREVIEW {
            preview.truncate(MAX_PREVIEW);
            preview.push_str("...");
        }
        out.push_str(&format!(
            "{{\"name\":{},\"type\":{},\"preview\":{}}}",
            json_string(name),
            json_string(v.type_name()),
            json_string(&preview)
        ));
    }
    out.push(']');
    out
}

/// Renders `it`'s final top-level `number`/`bool`/`vector`/`matrix`
/// bindings as UNTRUNCATED JSON (`[{"name":.., "type":.., "shape":[r,c],
/// "data":[...]}, ...]`), for `--emit-data`. This exists alongside
/// `vars_to_json` rather than extending it, because the two calls have
/// fundamentally different consumers: `--emit-vars`'s `preview` is
/// `display_value()`, meant for a human to skim in a variables panel, and
/// is deliberately capped/truncated; QuStudio's Interactive Mode instead
/// plots real data pulled straight out of a run (feeding it to
/// `react-plotly.js`), so every element has to survive the round trip. A
/// `Matrix` is column-major (`data[row + col*rows]`, matching
/// `qu-core::matrix`'s own storage), same convention `Matrix::as_slice`
/// already documents — this is NOT re-ordered to row-major here, so a
/// consumer must know to read it back the same way. Every other `Value`
/// variant (tables, strings, models, complex, ...) is silently skipped: a
/// script's non-numeric bindings simply aren't something a plot could use,
/// same non-error "just isn't in the list" behavior `vars_to_json` doesn't
/// need since it lists everything.
/// Parses `--ui-values`' flat `{"id": <number|bool|string>}` document into
/// interpreter values. Hand-rolled for the same reason as `ui_to_json`
/// (no `serde_json` in this crate), and deliberately forgiving: anything
/// it can't make sense of is skipped rather than failing the run, since a
/// control whose value doesn't arrive simply falls back to the default the
/// script itself declared.
fn parse_ui_values(text: &str) -> Vec<(String, qu_interp::Value)> {
    let mut out = Vec::new();
    let body = text.trim().trim_start_matches('{').trim_end_matches('}');
    for entry in split_top_level(body) {
        let Some((raw_key, raw_val)) = entry.split_once(':') else { continue };
        let key = raw_key.trim().trim_matches('"').to_string();
        if key.is_empty() {
            continue;
        }
        let val = raw_val.trim();
        let value = if val.starts_with('"') {
            qu_interp::Value::Str(val.trim_matches('"').to_string())
        } else if val == "true" || val == "false" {
            qu_interp::Value::Bool(val == "true")
        } else if let Ok(n) = val.parse::<f64>() {
            qu_interp::Value::Num(n)
        } else {
            continue;
        };
        out.push((key, value));
    }
    out
}

/// Splits a JSON object body on commas that aren't inside a string.
fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for ch in body.chars() {
        match ch {
            '"' if !escaped => {
                in_str = !in_str;
                cur.push(ch);
            }
            '\\' if in_str => {
                escaped = true;
                cur.push(ch);
                continue;
            }
            ',' if !in_str => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => cur.push(ch),
        }
        escaped = false;
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

/// Serializes the controls a script declared this run (see
/// `qu_interp::UiWidget`) for `--emit-ui`. Deliberately a flat array in
/// declaration order: that order IS the layout -- an immediate-mode script
/// declares its controls top to bottom as it runs, so a host renders them
/// in the order it receives them and needs no separate layout tree.
///
/// Hand-built like `vars_to_json`/`data_to_json` rather than via serde:
/// this crate has no `serde_json` dependency, and one more small writer is
/// a better trade than adding one just for six fields.
fn ui_to_json(it: &qu_interp::Interp) -> String {
    fn num_or_null(v: Option<f64>) -> String {
        v.map(|n| format!("{n}")).unwrap_or_else(|| "null".to_string())
    }
    let mut out = String::from("[");
    for (i, w) in it.ui_widgets.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let options: Vec<String> = w.options.iter().map(|o| json_string(o)).collect();
        out.push_str(&format!(
            "{{\"id\":{},\"kind\":{},\"label\":{},\"min\":{},\"max\":{},\"step\":{},\"options\":[{}],\"default\":{},\"fill\":{},\"size\":{}}}",
            json_string(&w.id),
            json_string(&w.kind),
            json_string(&w.label),
            num_or_null(w.min),
            num_or_null(w.max),
            num_or_null(w.step),
            options.join(","),
            json_string(&w.default),
            w.fill,
            num_or_null(w.size),
        ));
    }
    out.push(']');
    out
}

fn data_to_json(it: &qu_interp::Interp) -> String {
    let bindings: Vec<(&str, &qu_interp::Value)> = it.global_bindings().collect();

    // A record's numeric fields are emitted as `name.field`, one level
    // deep.
    //
    // Several builtins hand back a record because the pieces only mean
    // something together -- `spectrogram` returns its matrix with the
    // frequency and time axes that label it, and `eig` its values with its
    // vectors. Skipping records entirely (which is what `_ => continue`
    // below did to them) made all of that unreachable to anything reading
    // this dump: QuStudio's Interactive Mode plots workspace variables, so
    // a script whose whole result was a record had nothing to plot and no
    // indication why.
    //
    // One level, not recursive: it is enough for every builtin that
    // returns one, and a nested walk would flood the list from a single
    // deep structure -- `read_mat` on the crest-factor data would emit
    // several hundred entries from one variable.
    let mut flat: Vec<(String, &qu_interp::Value)> = Vec::new();
    for (name, v) in bindings {
        match v {
            qu_interp::Value::Record(fields) => {
                for (field, fv) in fields.iter() {
                    flat.push((format!("{name}.{field}"), fv));
                }
            }
            _ => flat.push((name.to_string(), v)),
        }
    }
    flat.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from("[");
    let mut first = true;
    for (name, v) in &flat {
        let (kind, shape, data): (&str, (usize, usize), Vec<f64>) = match v {
            qu_interp::Value::Num(n) => ("number", (1, 1), vec![*n]),
            qu_interp::Value::Bool(b) => ("bool", (1, 1), vec![if *b { 1.0 } else { 0.0 }]),
            qu_interp::Value::Vec(xs) => ("vector", (xs.len(), 1), xs.as_ref().clone()),
            // A `Signal` is a vector plus its sample rate, and for a dump
            // of numeric data it is simply the vector. Skipping it meant
            // every signal-processing script's main variable was absent:
            // `x = chirp(...)` produced one, so Interactive Mode had
            // nothing to plot from the very scripts it exists for.
            qu_interp::Value::Signal(xs, _, _) => ("vector", (xs.len(), 1), xs.as_ref().clone()),
            qu_interp::Value::Mat(m) => (
                "matrix",
                m.shape(),
                m.as_slice().to_vec(),
            ),
            _ => continue,
        };
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&format!(
            "{{\"name\":{},\"type\":{},\"shape\":[{},{}],\"data\":[",
            json_string(name.as_str()),
            json_string(kind),
            shape.0,
            shape.1
        ));
        for (j, x) in data.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&json_num(*x));
        }
        out.push_str("]}");
    }
    out.push(']');
    out
}

/// Encodes one `f64` as a JSON number token. JSON has no `NaN`/`Infinity`
/// literal (unlike Rust's own `Display` for `f64`, which prints `NaN`/`inf`
/// — invalid JSON if written verbatim), so a non-finite value becomes
/// `null` instead; the frontend treats that as "no data point" for that
/// sample rather than crashing on unparseable JSON. Finite values use
/// Rust's default `f64` `Display`, which already produces the shortest
/// decimal that round-trips exactly back to the same bits — full precision
/// preserved, unlike `display_value`'s human-oriented rounding.
fn json_num(n: f64) -> String {
    if n.is_finite() {
        format!("{n}")
    } else {
        "null".to_string()
    }
}

/// Minimal JSON string encoder (qu-cli has no serde dependency, and this is
/// the only place that needs one) -- escapes the characters JSON requires
/// (`"`, `\`, and control characters) and leaves everything else, including
/// all non-ASCII text, untouched (valid JSON strings are UTF-8 already).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn cmd_parse(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut json = false;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            other => {
                if path.is_none() {
                    path = Some(other);
                }
            }
        }
    }
    let src = read_file(path)?;

    if json {
        println!("{}", parse_diagnostics_json(&src));
        return Ok(());
    }

    match qu_syntax::parse(&src) {
        Ok(p) => {
            println!("OK — {} statement(s) parsed", p.stmts.len());
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// `qu parse --json`'s payload: `{"ok":bool,"diagnostics":[{"message","line","column"}]}`.
///
/// Mirrors `qu-studio-tauri`'s own `check_syntax` Tauri command (`main.rs`,
/// `Diagnostic { message, line, column }`) field-for-field, so the same
/// shape editors already consume there (QuStudio's CodeEditor.tsx debounced
/// inline-diagnostics feature) is reusable by an external editor extension
/// shelling out to this CLI instead of linking `qu_syntax` in-process. A
/// clean parse reports `"ok":true` with an empty `diagnostics` array; a
/// parse failure reports `"ok":false` with exactly one diagnostic built
/// from that error's own `ParseError { msg, span }` — `qu_syntax::parse`
/// stops at the first error, so there is at most one to report, same as
/// `check_syntax`. `line`/`column` are `Span`'s own fields verbatim (1-based,
/// per `qu-lexer`'s span convention), not re-based here.
fn parse_diagnostics_json(src: &str) -> String {
    match qu_syntax::parse(src) {
        Ok(_) => "{\"ok\":true,\"diagnostics\":[]}".to_string(),
        Err(e) => format!(
            "{{\"ok\":false,\"diagnostics\":[{{\"message\":{},\"line\":{},\"column\":{}}}]}}",
            json_string(&e.msg),
            e.span.line,
            e.span.col
        ),
    }
}

fn cmd_tokens(path: Option<&str>) -> Result<(), String> {
    let src = read_file(path)?;
    for t in qu_lexer::lex(&src) {
        println!("{:>3}:{:<3} {:?}", t.span.line, t.span.col, t.tok);
    }
    Ok(())
}

fn cmd_ast(path: Option<&str>) -> Result<(), String> {
    let src = read_file(path)?;
    match qu_syntax::parse(&src) {
        Ok(p) => {
            for s in &p.stmts {
                println!("{s:#?}");
            }
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn cmd_eval(src: Option<&str>) -> Result<(), String> {
    let src = src.ok_or("expected a source string")?;
    let mut it = qu_interp::Interp::new();
    it.run(src).map_err(|e| e.to_string())?;
    print!("{}", it.out);
    io::stdout().flush().ok();
    Ok(())
}

/// `qu docs --json` — dumps the compiled-in `BUILTIN_DOCS` table (name,
/// signature, one-line summary, chapter) as a single JSON array on stdout.
/// Exists so a host that cannot link `qu-interp` as a library (QuStudio's
/// Tauri backend deliberately doesn't, to keep autocomplete/build latency
/// down — see that crate's Cargo.toml) can still show the same reference
/// data a running interpreter's `help("name")` already serves, without
/// spawning one interpreter per lookup. `--json` is required, not a
/// default, so a bare `qu docs` stays a predictable no-op rather than
/// dumping 800+ lines at a terminal by accident.
fn cmd_docs(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|a| a == "--json") {
        return Err("qu docs requires --json (the only supported output today)".to_string());
    }
    println!("{}", builtin_docs_json());
    Ok(())
}

/// The pure half of `cmd_docs` -- kept separate from the `println!` so the
/// actual serialization is testable without capturing stdout.
fn builtin_docs_json() -> String {
    let entries: Vec<_> = qu_interp::builtin_docs::BUILTIN_DOCS
        .iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "signature": d.signature,
                "summary": d.summary,
                "chapter": d.chapter,
            })
        })
        .collect();
    serde_json::Value::Array(entries).to_string()
}

/// `qu diary <file.qu> [-o out.html]` — parses/executes the script exactly
/// like `qu run`, but renders a self-contained HTML transcript (one code
/// block + its printed output/plot per top-level statement, in source
/// order) instead of just dumping stdout. See `diary.rs` for the approach.
fn cmd_diary(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                out = args.get(i).cloned();
                if out.is_none() {
                    return Err("-o/--out expects a path argument".into());
                }
            }
            other => {
                if path.is_none() {
                    path = Some(other);
                }
            }
        }
        i += 1;
    }
    let path = path.ok_or("expected a file path")?;
    let src = read_file(Some(path))?;
    let out_path = out.unwrap_or_else(|| default_diary_path(path));

    let report = diary::run_diary(&src);
    std::fs::write(&out_path, &report.html)
        .map_err(|e| format!("cannot write {out_path}: {e}"))?;
    println!("wrote {out_path}");

    match report.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Default diary output path for `<dir>/name.qu` is `<dir>/name.html`.
fn default_diary_path(input: &str) -> String {
    let p = std::path::Path::new(input);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("diary");
    match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => {
            format!("{}/{stem}.html", dir.display())
        }
        _ => format!("{stem}.html"),
    }
}

/// Moved to `qu_interp::input_looks_complete` so a Jupyter kernel (or any
/// other host feeding Qu source incrementally) can share the exact same
/// block-completion heuristic instead of re-deriving it. Brought into scope
/// here so every existing call site (including this module's own tests)
/// keeps working unqualified.
use qu_interp::input_looks_complete;

/// Built-in constants the interpreter seeds every session with. They are
/// bindings like any other, but listing them under "your variables" is
/// noise you scan past on every `:vars` in a long session.
const BUILTIN_CONSTANTS: &[&str] = &["pi", "e", "tau", "inf", "nan", "none"];

fn print_vars(it: &qu_interp::Interp) {
    let mut bindings: Vec<(&str, &qu_interp::Value)> = it
        .global_bindings()
        .filter(|(name, _)| !BUILTIN_CONSTANTS.contains(name))
        .collect();
    bindings.sort_by(|a, b| a.0.cmp(b.0));
    if bindings.is_empty() {
        println!("  (no variables bound)");
        return;
    }
    for (name, v) in bindings {
        println!("  {:<12} {:<14} {}", name, v.type_name(), qu_interp::display_value(v));
    }
}

fn print_repl_help() {
    println!(
        "REPL meta-commands (not part of the Qu language):\n  \
         :vars, :whos   list your variables, their types and a summary\n  \
         :clear         reset the session (drop all variables/functions)\n  \
         :cancel        discard a half-typed block and start over\n  \
         :help          show this message\n  \
         :quit, :exit   leave the REPL (Ctrl+D / EOF also works)\n\n\
         `help` and `quit` also work without the colon.\n\n\
         A line not ending in `;` echoes its result (`ans = ...` for a bare\n\
         expression, `name = ...` for an assignment); end it with `;` to stay\n\
         quiet. An unfinished block (`for`/`while`/`if`/`function`/`try`/\n\
         `unsafe ... end`, or an open bracket) keeps prompting with `...>`\n\
         until it's complete -- closed by `end`, or by `end for` / `end if` /\n\
         `end while` / `end function`. `:cancel` throws one away."
    );
}

fn cmd_repl(args: &[String]) -> Result<(), String> {
    let mut it = qu_interp::Interp::new();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut buf = String::new();
    // A banner, because the meta-command system was previously reachable
    // only by typing something wrong and reading the error.
    println!("Qu {} — `:help` for commands, `:quit` to leave", env!("CARGO_PKG_VERSION"));

    // `qu repl <file.qu>` -- run the file into THIS SAME interpreter before
    // the first prompt, so its top-level bindings are already live when the
    // user starts typing. This is the one command that bridges "one-shot
    // run" and "REPL from nothing" (see the `qu repl` doc comment at the top
    // of this file for why that gap mattered). A script error here is fatal
    // -- same as `qu run` -- rather than silently dropping into an empty
    // REPL, since a half-run script's partial bindings would be a confusing
    // starting point, not a useful one.
    if let Some(path) = args.first() {
        let src = read_file(Some(path.as_str()))?;
        it.set_script_path(std::path::Path::new(path));
        it.run(&src).map_err(|e| e.to_string())?;
        print!("{}", it.out);
        it.out.clear();
        io::stdout().flush().ok();
    }

    print!("qu> ");
    io::stdout().flush().ok();
    while let Some(line) = lines.next() {
        let line = line.map_err(|e| e.to_string())?;

        // REPL meta-commands. Recognized even INSIDE a pending multi-line
        // block: previously they were only read when the buffer was empty,
        // so an unclosed block swallowed `:quit`, `:clear`, blank lines and
        // everything else, and the only escape was killing the process.
        // Nothing a user types should be able to trap them.
        //
        // `:` can never collide with real Qu syntax -- it is not a usable
        // leading character for any statement -- so this is unambiguous.
        //
        // Bare `help`, `quit`, `exit` and `?` are accepted too, without the
        // colon. They are what everyone types first, and answering
        // "`help` is not defined" while a perfectly good help system sits
        // behind an undiscoverable prefix is a bad first thirty seconds.
        let bare = line.trim();
        let meta = line
            .trim_start()
            .strip_prefix(':')
            .map(str::trim)
            .or_else(|| match bare {
                "help" | "?" => Some("help"),
                "quit" | "exit" => Some("quit"),
                _ => None,
            });
        if let Some(rest) = meta {
            match rest {
                "vars" | "whos" => print_vars(&it),
                "clear" => {
                    it = qu_interp::Interp::new();
                    buf.clear();
                    println!("  (session cleared)");
                }
                // An explicit way out of a half-typed block, which is what
                // you want when you have realised the line above was wrong.
                "cancel" | "abort" => {
                    if buf.is_empty() {
                        println!("  (nothing to cancel)");
                    } else {
                        buf.clear();
                        println!("  (discarded the unfinished block)");
                    }
                }
                "help" => print_repl_help(),
                "quit" | "exit" => break,
                other => println!("  unknown command `:{other}` — try `:help`"),
            }
            // Informational commands leave a pending block alone; `:clear`
            // and `:cancel` have already emptied it.
            print!("{}", if buf.is_empty() { "qu> " } else { "...> " });
            io::stdout().flush().ok();
            continue;
        }

        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&line);

        if !input_looks_complete(&buf) {
            print!("...> ");
            io::stdout().flush().ok();
            continue;
        }

        let src = std::mem::take(&mut buf);
        let before = it.out.len();
        match it.run_repl_line(&src) {
            Ok(echo) => {
                print!("{}", &it.out[before..]);
                if let Some((name, v)) = echo {
                    println!("{name} = {}", qu_interp::display_value(&v));
                }
            }
            Err(e) => {
                println!("  {e}");
            }
        }
        print!("qu> ");
        io::stdout().flush().ok();
    }
    println!();
    Ok(())
}

/// `qu kernel` — the persistent-interpreter backend for QuStudio's Code
/// editor "Run" button (see `qu-studio-tauri/src-tauri/src/repl_bridge.rs`
/// on the Tauri side). Unlike `qu repl`, this is not meant for a human at a
/// terminal: no banner, no prompt, no `:`-prefixed meta-commands, no
/// partial/multi-line-continuation buffering (the host sends one already-
/// complete submission per request, exactly what the editor buffer held
/// when Run was clicked — there's no line-at-a-time typing to buffer).
///
/// **Protocol.** Line-delimited JSON in both directions, one object per
/// line, flushed after every response so a host reading the child's stdout
/// asynchronously never blocks waiting for a line that's sitting in a
/// buffer:
///
/// Request (stdin):
///   `{"op":"run","code":"<source>"}` — parse+execute `code` against the
///   ONE `Interp` this process keeps alive for its whole lifetime (created
///   once, above the loop), so every binding a previous "run" made is still
///   in scope. Statement-level atomicity comes straight from
///   `Interp::run`/`exec`: it walks the parsed program's top-level
///   statements in a plain `for` loop bailing out on the first `Err` (see
///   `exec_block`), so whatever ran before the failing statement in THIS
///   submission stays applied, the failing statement's own effect doesn't
///   half-apply (each statement's `exec` either fully completes or returns
///   before mutating further), and nothing from an earlier "run" is
///   touched at all — this is exactly the semantics `qu repl` already
///   relies on for a human typing line by line, just handed a whole
///   submission's source at once instead of one line.
///   `{"op":"restart"}` — drops the live `Interp` and replaces it with a
///   fresh one, discarding all state. `{"op":"vars"}` reports the current
///   bindings without executing anything (e.g. for a panel refresh that
///   isn't tied to a run).
///
/// Response (stdout), one per request, in the same order:
///   `{"op":"run","success":bool,"output":"..","error":string|null,
///     "plots":["<svg>..",..],"variables":[{"name","type","preview"},..],
///     "data":[{"name","type","shape","data"},..]}` — `output` is only the
///   text `code` itself produced (the `it.out` growth since before this
///   call, matching `cmd_repl`'s own `before`/slice pattern), NOT the
///   whole session's output so far. `variables`/`data` ARE the whole
///   session's current bindings (via `vars_to_json`/`data_to_json` against
///   the live `it`, the same functions `qu run --emit-vars`/`--emit-data`
///   use) — that's deliberate: the host's Variables/Figures panels are
///   meant to show accumulated session state, not just this call's delta.
///   `plots` is every figure in `it.figure_history` plus the current live
///   one if it has content, rendered fresh each call — also accumulated
///   session state, not just what THIS run touched.
///   `{"op":"restart","success":true}` / `{"op":"vars","success":true,
///     "variables":[..],"data":[..]}`.
///   A line that isn't valid JSON, or whose `"op"` isn't recognized, gets
///   `{"op":"error","success":false,"error":".."}` — the process itself
///   never exits over a bad request, since one malformed message shouldn't
///   kill a session the user has state in.
fn cmd_kernel() -> Result<(), String> {
    let mut it = qu_interp::Interp::new();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let stdout = io::stdout();

    while let Some(line) = lines.next() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_kernel_response(
                    &stdout,
                    &serde_json::json!({
                        "op": "error",
                        "success": false,
                        "error": format!("bad request JSON: {e}"),
                    }),
                );
                continue;
            }
        };
        let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");
        match op {
            "run" => {
                let code = req.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let before = it.out.len();
                let run_result = it.run(code);
                let output = it.out[before..].to_string();
                let (success, error) = match &run_result {
                    Ok(()) => (true, None),
                    Err(e) => (false, Some(e.to_string())),
                };
                write_kernel_response(&stdout, &kernel_run_response(&it, success, output, error));
            }
            "restart" => {
                it = qu_interp::Interp::new();
                write_kernel_response(
                    &stdout,
                    &serde_json::json!({"op": "restart", "success": true}),
                );
            }
            "vars" => {
                write_kernel_response(
                    &stdout,
                    &serde_json::json!({
                        "op": "vars",
                        "success": true,
                        "variables": kernel_vars_value(&it),
                        "data": kernel_data_value(&it),
                    }),
                );
            }
            other => {
                write_kernel_response(
                    &stdout,
                    &serde_json::json!({
                        "op": "error",
                        "success": false,
                        "error": format!("unknown op `{other}`"),
                    }),
                );
            }
        }
    }
    Ok(())
}

/// Writes one JSON response line and flushes immediately — the host reads
/// this stream asynchronously and must see each response the moment it's
/// ready, not whenever a stdio buffer happens to fill.
fn write_kernel_response(mut stdout: &std::io::Stdout, resp: &serde_json::Value) {
    let _ = writeln!(stdout, "{resp}");
    let _ = stdout.flush();
}

/// `vars_to_json`'s output re-parsed as a `serde_json::Value` for embedding
/// in a `qu kernel` response — reusing the exact same text builder as `qu
/// run --emit-vars` rather than a second implementation, at the cost of one
/// parse-back per call (negligible next to the interpreter run it follows).
fn kernel_vars_value(it: &qu_interp::Interp) -> serde_json::Value {
    serde_json::from_str(&vars_to_json(it)).unwrap_or_else(|_| serde_json::json!([]))
}

/// Same idea as `kernel_vars_value`, for `data_to_json`.
fn kernel_data_value(it: &qu_interp::Interp) -> serde_json::Value {
    serde_json::from_str(&data_to_json(it)).unwrap_or_else(|_| serde_json::json!([]))
}

/// Every figure `it` currently holds, rendered to SVG text — the session's
/// full accumulated figure state, not just what the most recent "run"
/// touched. Same source selection `cmd_run`'s `--emit-figure` handling uses
/// (`figure_history` plus the live figure if it's non-pristine), kept as an
/// independent copy here rather than a shared helper so this new, additive
/// `kernel` path can never change `cmd_run`'s own behavior by editing code
/// `cmd_run` also calls.
fn kernel_figures_svg(it: &qu_interp::Interp) -> Vec<String> {
    let mut figures: Vec<&qu_interp::plotting::Figure> = it.figure_history.iter().collect();
    if !it.figure.is_pristine() {
        figures.push(&it.figure);
    }
    figures
        .iter()
        .map(|fig| qu_interp::plotting::render_svg(fig, fig.width, fig.height, fig.publication))
        .collect()
}

fn kernel_run_response(
    it: &qu_interp::Interp,
    success: bool,
    output: String,
    error: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "op": "run",
        "success": success,
        "output": output,
        "error": error,
        "plots": kernel_figures_svg(it),
        "variables": kernel_vars_value(it),
        "data": kernel_data_value(it),
    })
}

fn print_help() {
    println!("qu {} — reference engine (specification-first)\n", env!("CARGO_PKG_VERSION"));
    print!(
        "commands:\n  \
         run <file.qu>     parse + execute a script\n  \
         run <file.qu> [--max-time <s>] [--max-memory <MB>]   hard resource caps; kills the\n  \
                           process with a clear message identifying which limit was hit\n  \
         run <file.qu> --sandbox   deny network/process/file-write builtins at call time\n  \
         run <file.qu> --profile [--profile-output <path>]   per-function time + peak RSS report\n  \
         run <file.qu> --report <path.html>   additive: also write a self-contained HTML report\n  \
                           (source + captured output + any figure, retro window-chrome styled)\n  \
         build <file.qu> [-o <output>]   produce a standalone executable that runs the script\n  \
                           with no qu install needed -- see `qu build` in the file header comment\n  \
         parse <file.qu> [--json]   parse only (corpus acceptance); --json for structured diagnostics\n  \
         tokens <file.qu>  dump the token stream\n  \
         ast <file.qu>     dump the parsed AST\n  \
         eval \"<src>\"      run a one-liner\n  \
         diary <file.qu> [-o out.html]   run + export a Jupyter-style HTML transcript\n  \
         docs --json       dump the builtin reference table (name/signature/summary/chapter) as JSON\n  \
         repl [<file.qu>]  interactive read-eval-print loop; with a file, run it first and\n  \
                           keep its bindings alive in the same session (like `python -i`)\n  \
         kernel            persistent-interpreter JSON protocol on stdin/stdout, for a\n  \
                           programmatic host (e.g. QuStudio's Run button) -- not for humans\n  \
         mcp [--allow-write] [--timeout <s>] [--memory <MB>] [--root <dir>]\n  \
                           Model Context Protocol server on stdin/stdout, so an\n  \
                           assistant can run Qu instead of guessing at it\n  \
         version           print version\n"
    );
}

#[cfg(test)]
mod tests {
    use super::{builtin_docs_json, input_looks_complete, numbered_path, parse_diagnostics_json};

    #[test]
    fn parse_diagnostics_json_reports_ok_true_and_empty_diagnostics_for_valid_source() {
        let json = parse_diagnostics_json("x = 1 + 2\nprint(x)");
        assert_eq!(json, "{\"ok\":true,\"diagnostics\":[]}");
    }

    #[test]
    fn parse_diagnostics_json_reports_one_diagnostic_with_line_and_column_on_parse_error() {
        // `if` opened with no matching `end` -- same case
        // qu-studio-tauri's `check_syntax` test exercises.
        let json = parse_diagnostics_json("x = 1\nif x > 0\n  y = 2\n");
        assert!(json.starts_with("{\"ok\":false,\"diagnostics\":[{"));
        assert!(json.contains("\"line\":"));
        assert!(json.contains("\"column\":"));
        assert!(json.contains("\"message\":"));
    }

    #[test]
    fn builtin_docs_json_is_a_nonempty_array_of_complete_entries() {
        let json: serde_json::Value = serde_json::from_str(&builtin_docs_json())
            .expect("builtin_docs_json should produce valid JSON");
        let entries = json.as_array().expect("top level should be a JSON array");
        // The table has 800+ entries as of this writing; a low bound here
        // catches "the table came back empty/truncated" without pinning an
        // exact count that would need updating every time a builtin is added.
        assert!(entries.len() > 100, "expected a real table, got {} entries", entries.len());
        let linspace = entries
            .iter()
            .find(|e| e["name"] == "linspace")
            .expect("a well-known builtin should be present");
        assert!(linspace["signature"].as_str().unwrap().contains("linspace"));
        assert!(!linspace["summary"].as_str().unwrap().is_empty());
        assert!(!linspace["chapter"].as_str().unwrap().is_empty());
    }

    #[test]
    fn single_statement_lines_are_complete() {
        assert!(input_looks_complete("x = 5"));
        assert!(input_looks_complete("3 + 4"));
        assert!(input_looks_complete(""));
    }

    #[test]
    fn open_for_while_if_function_try_unsafe_blocks_are_incomplete() {
        assert!(!input_looks_complete("for i = 1 to 3"));
        assert!(!input_looks_complete("while x < 10"));
        assert!(!input_looks_complete("if x > 0"));
        assert!(!input_looks_complete("function f(x)"));
        assert!(!input_looks_complete("try"));
        assert!(!input_looks_complete("unsafe"));
    }

    #[test]
    fn closing_end_makes_the_block_complete() {
        assert!(input_looks_complete("for i = 1 to 3\n  print(i)\nend"));
        assert!(input_looks_complete("if x > 0\n  y = 1\nend"));
        assert!(input_looks_complete("function f(x)\n  return x * 2\nend"));
    }

    #[test]
    fn nested_blocks_need_one_end_per_opener() {
        assert!(!input_looks_complete("for i = 1 to 3\n  if i == 1\n    print(i)\n  end"));
        assert!(input_looks_complete(
            "for i = 1 to 3\n  if i == 1\n    print(i)\n  end\nend"
        ));
    }

    #[test]
    fn parallel_for_closes_with_a_single_end() {
        assert!(!input_looks_complete("parallel for i in 1 to 3"));
        assert!(input_looks_complete("parallel for i in 1 to 3\n  x = i\nend parallel"));
    }

    #[test]
    fn open_brackets_across_lines_are_incomplete_until_closed() {
        assert!(!input_looks_complete("m = [1, 2,"));
        assert!(input_looks_complete("m = [1, 2,\n     3, 4]"));
        assert!(!input_looks_complete("f(1, 2"));
        assert!(input_looks_complete("f(1, 2)"));
    }

    #[test]
    fn contextual_timer_and_on_elapsed_forms_need_their_own_end() {
        assert!(!input_looks_complete("every 1 s do"));
        assert!(input_looks_complete("every 1 s do\n  tick()\nend"));
        assert!(!input_looks_complete("on elapsed(t) do"));
        assert!(input_looks_complete("on elapsed(t) do\n  tick()\nend"));
        // `every`/`on` used as a plain variable name must NOT be treated as
        // opening a block.
        assert!(input_looks_complete("every = 5"));
        assert!(input_looks_complete("on = 5"));
    }

    #[test]
    fn keywords_inside_strings_and_comments_do_not_affect_balance() {
        assert!(input_looks_complete("x = \"for i = 1 to 3 end\""));
        assert!(input_looks_complete("x = 1 # for while if unsafe end"));
    }


    // F-01: `end for` and friends used to leave the REPL at `...>` forever
    // -- the trailing keyword was counted as opening a new block, so it
    // exactly cancelled the `end`. The file parser has always accepted
    // both forms; this is the REPL agreeing with it.
    #[test]
    fn qualified_end_closes_a_block() {
        for src in [
            "for i = 0 to 2\nprint(i)\nend for",
            "if x > 1\nprint(x)\nend if",
            "while i < 3\ni = i + 1\nend while",
            "function f(a)\nreturn a\nend function",
        ] {
            assert!(input_looks_complete(src), "should be complete:\n{src}");
        }
    }

    #[test]
    fn bare_end_still_closes_a_block() {
        assert!(input_looks_complete("for i = 0 to 2\nprint(i)\nend"));
    }

    #[test]
    fn an_unclosed_block_is_still_incomplete() {
        assert!(!input_looks_complete("for i = 0 to 2\nprint(i)"));
        assert!(!input_looks_complete("if x > 1"));
    }

    #[test]
    fn nested_blocks_need_every_end() {
        assert!(!input_looks_complete("for i = 0 to 2\nif i > 1\nprint(i)\nend if"));
        assert!(input_looks_complete(
            "for i = 0 to 2\nif i > 1\nprint(i)\nend if\nend for"
        ));
    }

    #[test]
    fn a_for_after_a_completed_block_still_opens_one() {
        // Guards the fix from over-reaching: only a keyword IMMEDIATELY
        // after `end` is part of that `end`. A new loop on the next line
        // is a new block and must still be counted.
        assert!(!input_looks_complete("for i = 0 to 2\nend\nfor j = 0 to 2"));
        assert!(!input_looks_complete("for i = 0 to 2\nend for\nfor j = 0 to 2"));
    }

    #[test]
    fn brackets_still_gate_completeness() {
        assert!(!input_looks_complete("x = [1, 2,"));
        assert!(input_looks_complete("x = [1, 2]"));
    }

    #[test]
    fn numbered_path_inserts_before_the_extension() {
        assert_eq!(numbered_path("fig.svg", 2), "fig_2.svg");
        assert_eq!(numbered_path("out/deep.dir/fig.svg", 3), "out/deep.dir/fig_3.svg");
        // No extension in the final component: suffix goes on the end
        // rather than corrupting a directory name.
        assert_eq!(numbered_path("out.d/fig", 2), "out.d/fig_2");
        assert_eq!(numbered_path("fig", 2), "fig_2");
    }
}
