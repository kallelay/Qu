//! § file/directory I/O extensions + type casts (2026-08-26). Builtins added
//! on top of the existing `fopen`/`read_*`/`write_*`/`close` library
//! (`FileHandleState`, `qu-interp/src/lib.rs`, § file I/O 2026-08-24): file
//! **position control** (`seek`/`tell`/`rewind`/`eof`), **generic typed
//! binary I/O** (`read_int`/`write_int`, parametrized by `bytes=`/`signed=`/
//! `endian=`, alongside the pre-existing fixed-width `read_int16`/
//! `write_int32`/etc. family, which is untouched), **raw byte-buffer I/O**
//! (`read_bin`/`write_bin`), **directory/filesystem** builtins (`cd`/`pwd`/
//! `mkdir`/`file_exists`/`dir_exists`/`make_file`/`tmp_file`/`list_dir`), and
//! **type casts** (`to_int`/`to_float`/`to_bool`).
//!
//! **§ fileops (2026-09-16)**: `remove_file`/`remove_dir` (with an optional
//! `recycle_bin=true` — the one thing `std::fs::remove_*` genuinely cannot
//! do, hence the `trash` crate dependency, see this crate's `Cargo.toml`),
//! `rename_file`/`move_file`/`copy_file`/`create_file` (all four sharing an
//! `on_exists="error"|"overwrite"|"skip"` collision policy). `mkdir`/
//! `make_file` above are UNCHANGED — kept at their existing, relied-on
//! always-succeeds contract rather than retrofitted with a policy that
//! would silently change what an existing caller's script does.
//!
//! Kept in its own module (rather than inserted directly into `lib.rs`'s
//! already-enormous builtin-dispatch `match`) specifically because another
//! concurrent pass is actively editing `lib.rs`/adding `queue_pool.rs` at
//! the same time — this file only touches `lib.rs` at a handful of minimal,
//! additive points (one new `mod` line, two new `FileHandleState` fields,
//! a few lines inside the existing `fopen`/`read_all` arms, and one new
//! match arm dispatching every name below to `fs_ops::call`), everything
//! else lives here.
//!
//! Every helper below reaches back into `lib.rs` via `crate::…` for shared
//! plumbing (`Value`, `EvalError`/`R`, `arg0`/`int_arg`/`text_arg`/
//! `style_str`/`style_num`/`to_vec`/`truthy`/`display_value`, and the file-
//! handle internals `as_file`/`file_check_open`/`file_check_readable`/
//! `file_check_writable`/`file_align_to_byte`/`file_remaining`/
//! `file_read_exact`). None of those are `pub` in `lib.rs` — they don't need
//! to be: Rust's own privacy rule already makes a private item of a module
//! visible to every *descendant* module, and `fs_ops` is declared as a
//! child of the crate root (`pub mod fs_ops;` in `lib.rs`), so this file is
//! exactly such a descendant. This matches `scaler.rs`'s own
//! `use crate::{e, quantile_of, std_dev, R};` pattern — nothing new to
//! learn here, just following precedent.
//!
//! **Recovery note (2026-08-26)**: this module (and the matching `lib.rs`
//! edits/tests) had to be rewritten once mid-task after an external
//! `git reset --hard HEAD` (run by a concurrent session sharing this same
//! working directory — see reflog, not something this task ran) wiped every
//! uncommitted change in the tree, this untracked file included. Rebuilt
//! from scratch to the identical design below; flagged prominently in this
//! task's final report since it's a real, unusual repo-safety incident
//! worth the user knowing about, not something to quietly paper over.

use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::{
    arg0, as_file, as_mmap, display_value, e, file_align_to_byte, file_check_open, file_check_readable,
    file_check_writable, file_read_byte_raw, file_read_char_lossy, file_read_exact, file_read_upto,
    file_remaining, file_seek_to, int_arg, style_entry, style_num, style_str, text_arg, to_vec,
    truthy, EvalError, FileHandleState, MmapState, Value, R,
};

/// Single dispatch entry point, called from `lib.rs`'s big builtin match
/// with one combined arm listing every name below (see that arm's own
/// comment for why it's one arm, not one per name — same "reduce the diff
/// footprint in a file another pass is editing" reasoning as this module's
/// own existence).
pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "seek" => seek(args, style),
        "tell" => tell(args),
        "rewind" => rewind(args),
        "eof" => eof(args),
        "read_int" => read_int(args, style),
        "write_int" => write_int(args, style),
        "read_bin" => read_bin(args),
        "write_bin" => write_bin(args),
        "cd" => cd(args),
        // `cur_dir` says what it gives back; `pwd` is what a shell user
        // will type first. Both, rather than a rename that breaks scripts.
        "pwd" | "cur_dir" => pwd(),
        "file_exists" => file_exists(args),
        "dir_exists" => dir_exists(args),
        "mkdir" => mkdir(args),
        "make_file" => make_file(args),
        "tmp_file" => tmp_file(),
        // § fileops (2026-09-16, Ahmed's direct go-ahead): a real
        // remove/rename/move/copy/create surface with recycle-bin and
        // collision-policy support, none of which `std` alone provides
        // portably (see each function's own doc comment). `move` itself
        // was NOT reused for `move_file` — that name is already a
        // `state.move(dt)` particle-tracker method, and overloading it by
        // arg shape would be exactly the kind of same-name-different-
        // behavior trap this codebase's own kwarg-validation policy exists
        // to avoid elsewhere.
        "remove_file" => remove_file(args, style),
        "remove_dir" => remove_dir(args, style),
        "rename_file" => rename_file(args, style),
        "move_file" => move_file(args, style),
        "copy_file" => copy_file(args, style),
        "create_file" => create_file(args, style),
        "list_dir" | "listdir" | "dir" => list_dir(args),
        "list_files" => list_files(args, style),
        // `glob(pattern)` is `list_files(pattern)` with no filters -- the
        // name every other language uses for exactly this, kept so a
        // reader looking for it finds it.
        "glob" => list_files(&args[..args.len().min(1)], &[]),
        "read_array" => read_array(args, style),
        "write_array" => write_array(args, style),
        "file_size" => file_size(args),
        "to_int" => to_int(args),
        "to_float" => to_float(args),
        "to_bool" => to_bool(args),
        "touch" => touch(args),
        "mmap_open" => mmap_open(args),
        "mmap_read" => mmap_read(args),
        "mmap_len" => mmap_len(args),
        "peek_line" => peek_line(args),
        "peek_char" => peek_char(args),
        "peek_byte" => peek_byte(args),
        other => e(format!("fs_ops: internal dispatch error, unhandled `{other}`")),
    }
}

// ================= touch =================

/// `touch(path)` (§ file I/O extensions, 2026-08-31) — Unix `touch`
/// semantics: create an empty file if `path` doesn't exist; if it already
/// exists, update its modification time WITHOUT altering its content.
///
/// The classic "open for append then close" trick does NOT reliably bump
/// mtime on every platform (some filesystems/OSes only update mtime on an
/// actual write, and an empty append writes zero bytes) — this uses the
/// `filetime` crate to set the mtime directly, a real, portable mtime-set
/// syscall wrapper (`utimensat`/`SetFileTime` under the hood) rather than
/// a half-working workaround. Sets both mtime AND atime to "now" (matching
/// real `touch`'s own default behavior with no `-m`/`-a` flag).
fn touch(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let p = std::path::Path::new(&path);
    if !p.exists() {
        std::fs::File::create(p).map_err(|err| EvalError {
            msg: format!("touch: could not create `{path}`: {err}"),
        })?;
    }
    let now = filetime::FileTime::now();
    filetime::set_file_times(p, now, now).map_err(|err| EvalError {
        msg: format!("touch: could not update the modification time of `{path}`: {err}"),
    })?;
    Ok(Value::Nothing)
}

// ================= memory-mapped file access: mmap_open/mmap_read/mmap_len =================
//
// § memory-mapped file access, 2026-08-31 — read-only random access to a
// file's bytes without loading it into a `Vec` up front, via the `memmap2`
// crate (the standard, widely-used safe(r) wrapper around the OS's own
// `mmap`/`MapViewOfFile`). Read-only by deliberate scope cut: a writable
// mapping was not asked for and is real extra surface (flushing, torn-write
// safety across concurrent mappings) this pass doesn't take on. See
// `Value::Mmap`/`MmapState`'s own doc comments in `lib.rs` for the handle
// shape.

/// `mmap_open(path)` — maps `path` read-only and returns a `Value::Mmap`
/// handle. Errors clearly if the file doesn't exist or can't be opened.
/// Mapping a zero-length file is explicitly rejected up front with a clear
/// message rather than left to `memmap2`'s own error (mapping an empty file
/// is undefined/rejected by the underlying OS call on every platform this
/// targets, and "the file is empty" is a far more actionable message than
/// whatever raw OS error that produces).
fn mmap_open(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let file = std::fs::File::open(&path).map_err(|err| EvalError {
        msg: format!("mmap_open: could not open `{path}`: {err}"),
    })?;
    let len = file
        .metadata()
        .map_err(|err| EvalError {
            msg: format!("mmap_open: could not read metadata for `{path}`: {err}"),
        })?
        .len();
    if len == 0 {
        return e(format!("mmap_open: `{path}` is empty — mapping a zero-length file is not supported"));
    }
    // Safety: memory-mapping a file is inherently unsafe in the sense that
    // the mapped bytes can change out from under the mapping if another
    // process/thread modifies the file concurrently (this crate does not,
    // and cannot in general, guard against that — same caveat every
    // `memmap2` consumer accepts). Read-only mapping of a plain, already-
    // opened, real filesystem file — no unusual fd tricks — is the
    // conventional, well-understood use `memmap2`'s own docs describe.
    let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|err| EvalError {
        msg: format!("mmap_open: could not memory-map `{path}`: {err}"),
    })?;
    Ok(Value::Mmap(Arc::new(MmapState { path, mmap })))
}

/// `mmap_len(handle)` — the mapped file's total length in bytes.
fn mmap_len(args: &[Value]) -> R<Value> {
    let m = as_mmap(arg0(args)?)?;
    Ok(Value::Num(m.mmap.len() as f64))
}

/// `mmap_read(handle, start, len)` — reads `len` raw bytes starting at byte
/// offset `start`, returned the same way `read_bin` returns bytes (a
/// `Value::Vec` of numbers 0-255 — matching that existing convention rather
/// than inventing a second "bytes" representation, per the brief's own
/// instruction to check `read_bin` and match it). A range that runs past
/// the end of the file is a CLEAR ERROR (not silently truncated) — the
/// documented choice for this builtin: unlike `read_bin`'s "graceful
/// partial read" (which makes sense for a sequential stream cursor that
/// doesn't know its own remaining length without an extra call), `mmap_len`
/// is always available up front here, so a range past EOF is far more
/// likely a caller bug (an off-by-one, a stale cached length) than a
/// deliberate "read whatever's left" — surfacing it beats silently handing
/// back fewer bytes than asked for.
fn mmap_read(args: &[Value]) -> R<Value> {
    let m = as_mmap(arg0(args)?)?;
    let start = int_arg(args, 1)?;
    let len = int_arg(args, 2)?;
    if start < 0 {
        return e(format!("mmap_read: start must be non-negative, got {start}"));
    }
    if len < 0 {
        return e(format!("mmap_read: len must be non-negative, got {len}"));
    }
    let (start, len) = (start as usize, len as usize);
    let total = m.mmap.len();
    let end = start.checked_add(len).ok_or_else(|| EvalError {
        msg: format!("mmap_read: start ({start}) + len ({len}) overflows"),
    })?;
    if end > total {
        return e(format!(
            "mmap_read: range {start}..{end} runs past the end of `{}` ({total} byte(s) total)",
            m.path
        ));
    }
    let bytes = &m.mmap[start..end];
    Ok(Value::Vec(Arc::new(bytes.iter().map(|&b| b as f64).collect())))
}

// ================= position control: seek/tell/rewind/eof =================
//
// Deliberately restricted to READABLE handles (`"r"`/`"rb"`) — a write-only
// handle gets a clear, explanatory error rather than a half-working or
// platform-inconsistent seek. Reasoning, spelled out once here rather than
// re-derived at each call site:
//
// Read-mode files are backed by a real streaming `BufReader<File>` plus an
// explicit `read_pos` byte cursor and a cached `file_len` (§ real streaming
// reads, 2026-08-31 — see `FileHandleState`'s own doc comment in `lib.rs`
// for the full story, including why this used to say "loaded whole into
// `read_buf`") — an absolute/relative/from-end seek is well-defined against
// `file_len` and cheap (`file_seek_to` performs a real `Seek::seek` on the
// underlying reader, not just a variable update).
//
// Write-mode files (`"w"`/`"a"`/`"wb"`/`"ab"`) are backed by a *live*
// `std::fs::File`, written incrementally via `write_all`, with NO tracked
// write-position field anywhere in `FileHandleState` — every existing write
// builtin (`write_line`/`write_byte`/`write_int32`/…) just calls
// `write_all` and trusts the OS cursor. Worse, append mode (`"a"`/`"ab"`) is
// opened via `OpenOptions::append(true)`, which is POSIX `O_APPEND`: on most
// platforms **every write is forced to the current end of file regardless
// of any prior seek**, so a `seek` on an append handle would either
// silently do nothing useful or behave inconsistently across platforms —
// exactly the kind of silent surprise this library's own doc comments
// promise not to introduce. Bolting on a `write_pos` field just for `tell`/
// `seek` would only be updated by the handful of write builtins this pass
// touches, silently going stale the moment a script mixes in `write_line`/
// `write_byte`/`write_int32`/etc. — a much easier way to ship a quietly
// wrong answer than a clear upfront error. So: position control is read-
// only, full stop, with the restriction explained in the error message
// every time it's hit.
fn require_positionable(st: &FileHandleState, op: &str) -> R<()> {
    file_check_open(st)?;
    if !st.readable {
        return e(format!(
            "{op}: file `{}` was opened for writing only (mode `{}`) — position control ({op}) \
             is only supported on readable handles in this file I/O library: write-mode files \
             are written sequentially through a live OS file handle with no tracked cursor, and \
             append mode ignores seeks on most platforms. Open the file in a mode that includes \
             `r` if you need positioned access.",
            st.path, st.mode_str
        ));
    }
    Ok(())
}

/// `seek(f, offset, [whence="start"|"current"|"end"])` — moves the read
/// cursor. `whence` follows C's own `fseek`/`SEEK_SET`/`SEEK_CUR`/`SEEK_END`
/// naming (spelled as strings, matching this codebase's `endian=`/`sep=`-
/// style kwarg convention rather than introducing bare symbolic constants):
/// `"start"` (default) is today's plain absolute-offset behavior, `"current"`
/// is relative to the position `tell(f)` would report right now (so
/// `seek(f, -n, whence="current")` reads as "go back `n` bytes" without a
/// separate `rewind(f, n)` function), `"end"` is relative to EOF (typically
/// a zero-or-negative offset). A single `whence`-aware `seek` was chosen
/// over a second `rewind(f, n)`-style function specifically to avoid two
/// overlapping, similarly-named position primitives — `rewind(f)` still
/// exists below as the documented `seek(f, 0)` convenience, not as a
/// separate implementation. Errors clearly if the resulting position would
/// land outside `[0, file length]` — there is no "clamp silently" fallback,
/// consistent with every other file I/O error in this library.
fn seek(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let offset_v = args.get(1).ok_or_else(|| EvalError {
        msg: "seek(file, offset, [whence=]) needs an offset argument".to_string(),
    })?;
    let offset = offset_v.as_num().map_err(|m| EvalError { msg: m })?;
    if !offset.is_finite() || offset.fract() != 0.0 {
        return e(format!("seek: offset must be a whole number of bytes, got {offset}"));
    }
    let offset = offset as i64;
    let whence = style_str(style, "whence").unwrap_or_else(|| "start".to_string());
    let mut st = fh.lock().unwrap();
    require_positionable(&st, "seek")?;
    let len = st.file_len as i64;
    let base = match whence.as_str() {
        "start" => 0i64,
        "current" => st.read_pos as i64,
        "end" => len,
        other => return e(format!("seek: whence must be \"start\", \"current\", or \"end\", got \"{other}\"")),
    };
    let target = base + offset;
    if target < 0 || target > len {
        return e(format!(
            "seek: position {target} (whence=\"{whence}\", offset={offset}) is out of range for \
             `{}` — the file is {len} byte(s), so a valid position is 0..={len}",
            st.path
        ));
    }
    // § real streaming reads (2026-08-31): repositions the ACTUAL
    // underlying reader (not just the `read_pos` tracking variable) —
    // `file_seek_to` also resets `read_bit_pos`/`pending_bit_byte`/
    // `pushback`, replacing this function's own old direct `read_bit_pos =
    // 0` line.
    file_seek_to(&mut st, target as usize)?;
    Ok(Value::Nothing)
}

/// `tell(f)` — the current (byte-aligned) read position, i.e. how many
/// bytes have been consumed from the start of the file so far.
fn tell(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let st = fh.lock().unwrap();
    require_positionable(&st, "tell")?;
    Ok(Value::Num(st.read_pos as f64))
}

/// `rewind(f)` — `seek(f, 0)` (equivalently `seek(f, 0, whence="start")`),
/// spelled out directly here rather than literally calling `seek` so the
/// out-of-range branch (never reachable for a fixed `target = 0`) doesn't
/// need to exist on this path at all.
fn rewind(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let mut st = fh.lock().unwrap();
    require_positionable(&st, "rewind")?;
    file_seek_to(&mut st, 0)?;
    Ok(Value::Nothing)
}

/// `eof(f)` — `true` iff no bytes remain to read from the current
/// (byte-aligned) position.
fn eof(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let mut st = fh.lock().unwrap();
    require_positionable(&st, "eof")?;
    file_align_to_byte(&mut st);
    Ok(Value::Bool(file_remaining(&st) == 0))
}

// ================= non-advancing peeks: peek_line/peek_char/peek_byte =================
//
// § peek without advancing (2026-09-01) — reads the next unit exactly the
// way `read_line`/`read_char`/`read_byte` would, WITHOUT moving the
// handle's read cursor: a second `peek_*` call, or the next real `read_*`
// call, sees the identical data again. Three distinct names, matching
// `read_line`/`read_char`/`read_byte`'s own existing three granularities
// one-for-one, rather than a single generic `peek()` — this codebase
// already made that exact naming choice for reads (there is no bare
// `read()`), and a single `peek()` would additionally collide with
// `fifo`'s own `.peek()` method name (§ data structures pass, same date),
// which answers a completely different question (the oldest queued value,
// not "the next unit of a file").
//
// Implemented via a real save-position/read/restore-position round trip
// around the existing buffered reader — `tell`'s own `read_pos` field for
// "save", the corresponding `read_*` logic for "read", and `file_seek_to`
// (the same real `Seek::seek` `seek`/`rewind` already use) for "restore" —
// rather than consuming and needing to un-consume: a genuine reposition of
// the underlying `BufReader` is simpler AND more obviously correct than
// hand-rolling a one-item lookahead buffer that every OTHER read builtin
// would then also need to check on every call.
//
// Deliberately restricted to READABLE handles, same as `seek`/`tell`/
// `rewind`/`eof` — `file_check_readable` is the same check `read_line`/
// `read_char`/`read_byte` themselves already use (not `require_positionable`'s
// own wording, which talks specifically about *why seek is restricted*;
// peeking's read-only restriction is just "this is a read", not a separate
// user-facing position-control concept).

/// `peek_line(f)` — exactly `read_line(f)`'s result (one line, without its
/// trailing newline, or `Nothing` at EOF), but the read cursor ends up
/// back where it started: the very next `read_line(f)` (or another
/// `peek_line(f)`) returns the SAME line again.
fn peek_line(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let mut st = fh.lock().unwrap();
    file_check_readable(&st)?;
    file_align_to_byte(&mut st);
    let start = st.read_pos;
    if file_remaining(&st) == 0 {
        return Ok(Value::Nothing);
    }
    let mut line_bytes: Vec<u8> = Vec::new();
    loop {
        match file_read_byte_raw(&mut st) {
            Some(b'\n') => break,
            Some(b) => line_bytes.push(b),
            None => break,
        }
    }
    if line_bytes.last() == Some(&b'\r') {
        line_bytes.pop();
    }
    file_seek_to(&mut st, start)?;
    Ok(Value::Str(String::from_utf8_lossy(&line_bytes).into_owned()))
}

/// `peek_char(f)` — exactly `read_char(f)`'s result (one UTF-8 character,
/// or `Nothing` at EOF), cursor restored afterward.
fn peek_char(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let mut st = fh.lock().unwrap();
    file_check_readable(&st)?;
    file_align_to_byte(&mut st);
    let start = st.read_pos;
    if file_remaining(&st) == 0 {
        return Ok(Value::Nothing);
    }
    let result = match file_read_char_lossy(&mut st) {
        Some(c) => Value::Str(c.to_string()),
        None => Value::Nothing,
    };
    file_seek_to(&mut st, start)?;
    Ok(result)
}

/// `peek_byte(f)` — exactly `read_byte(f)`'s result (one raw byte 0-255, or
/// `Nothing` at EOF), cursor restored afterward.
fn peek_byte(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let mut st = fh.lock().unwrap();
    file_check_readable(&st)?;
    file_align_to_byte(&mut st);
    let start = st.read_pos;
    match file_read_exact(&mut st, 1) {
        Some(b) => {
            file_seek_to(&mut st, start)?;
            Ok(Value::Num(b[0] as f64))
        }
        None => Ok(Value::Nothing),
    }
}

// ================= generic typed binary I/O: read_int/write_int =================
//
// Distinct from the pre-existing fixed-width `read_int16`/`read_uint16`/
// `read_int32`/`read_uint32`/`read_float`/`read_double` family (and their
// `write_*` mirrors) in `lib.rs`: those encode width+signedness in the
// function NAME (one name per (width, signed) pair) and always resolve
// `endian=` against a hardcoded `"little"` default when the kwarg is
// omitted. `read_int`/`write_int` instead take `bytes=`/`signed=` as
// parameters of ONE function — more convenient for parsing a binary format
// whose field widths are looked up at runtime (a header byte saying "this
// field is 2 bytes," say) rather than known at script-writing time.
//
// Per-call `signed=`/`endian=` kwargs are optional and fall back to
// *per-handle* defaults set once at `fopen(path, mode, [signed=], [endian=])`
// time (`FileHandleState::default_signed`/`default_little_endian`, parsed in
// `lib.rs`'s `fopen` arm) — deliberately scoped to the one file it
// naturally belongs to, not a global/sticky interpreter setting, so two
// files with different native endianness open at the same time don't step
// on each other. An explicit per-call kwarg still overrides the handle
// default for that one call, same precedence order any keyword-argument-
// with-a-default convention uses. The pre-existing `read_int16`/etc. family
// deliberately does NOT consult these new handle defaults — it keeps its
// original hardcoded-little-endian-unless-overridden behavior exactly as
// shipped 2026-08-24, so this pass changes zero already-tested behavior;
// only the new `bytes=`-parametrized functions are default-aware.
//
// Both `read_int` and `write_int` consume/produce EXACTLY `bytes` bytes
// per call, advancing the handle's position by exactly that many bytes —
// load-bearing for sequential binary parsing (read a fixed-width header
// field, then immediately read the next one, or a length-prefixed payload)
// — `file_read_exact` (read side) and a single `write_all` of exactly
// `bytes` bytes (write side) both already guarantee this; there is no
// partial-consume path.

fn valid_int_width(bytes: i64) -> bool {
    matches!(bytes, 1 | 2 | 4 | 8)
}

fn resolve_signed(style: &[(String, Value)], default_signed: bool) -> bool {
    crate::style_entry(style, "signed")
        .map(|(_, v)| truthy(v))
        .unwrap_or(default_signed)
}

fn resolve_endian(style: &[(String, Value)], default_little: bool) -> R<bool> {
    match style_str(style, "endian").as_deref() {
        None => Ok(default_little),
        Some("little") => Ok(true),
        Some("big") => Ok(false),
        Some(other) => e(format!("endian must be \"little\" or \"big\", got \"{other}\"")),
    }
}

/// `read_int(f, [bytes=4], [signed=], [endian=])` — reads `bytes` bytes
/// from the current position (1/2/4/8 only) and decodes them as a signed
/// or unsigned integer of that width in the given byte order, advancing the
/// position by exactly `bytes`. `Value::Nothing` if fewer than `bytes`
/// bytes remain (same "exhausted" convention as `read_int16`/etc. and every
/// other typed read). Values are always returned as `Value::Num` (`f64`) —
/// this codebase has no separate integer type — so an 8-byte unsigned
/// read above 2^53 already loses exact precision inside the `f64`; that is
/// an inherent `Value::Num` limitation shared by `read_double`/`write_double`
/// and the pre-existing `read_uint32`, not something new introduced here.
/// The 8-byte width exists for byte-LAYOUT fidelity (matching a protocol's
/// or C struct's declared field width) more than for full 64-bit numeric
/// range.
fn read_int(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let bytes = style_num(style, "bytes").map(|n| n as i64).unwrap_or(4);
    if !valid_int_width(bytes) {
        return e(format!("read_int: bytes must be 1, 2, 4, or 8, got {bytes}"));
    }
    let mut st = fh.lock().unwrap();
    file_check_readable(&st)?;
    let signed = resolve_signed(style, st.default_signed);
    let little = resolve_endian(style, st.default_little_endian)?;
    let Some(raw) = file_read_exact(&mut st, bytes as usize) else {
        return Ok(Value::Nothing);
    };
    let n = match (bytes, signed) {
        (1, true) => raw[0] as i8 as f64,
        (1, false) => raw[0] as f64,
        (2, true) => {
            let b: [u8; 2] = raw.try_into().unwrap();
            (if little { i16::from_le_bytes(b) } else { i16::from_be_bytes(b) }) as f64
        }
        (2, false) => {
            let b: [u8; 2] = raw.try_into().unwrap();
            (if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }) as f64
        }
        (4, true) => {
            let b: [u8; 4] = raw.try_into().unwrap();
            (if little { i32::from_le_bytes(b) } else { i32::from_be_bytes(b) }) as f64
        }
        (4, false) => {
            let b: [u8; 4] = raw.try_into().unwrap();
            (if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }) as f64
        }
        (8, true) => {
            let b: [u8; 8] = raw.try_into().unwrap();
            (if little { i64::from_le_bytes(b) } else { i64::from_be_bytes(b) }) as f64
        }
        (8, false) => {
            let b: [u8; 8] = raw.try_into().unwrap();
            (if little { u64::from_le_bytes(b) } else { u64::from_be_bytes(b) }) as f64
        }
        _ => unreachable!("bytes validated to be 1/2/4/8 above"),
    };
    Ok(Value::Num(n))
}

/// `write_int(f, value, [bytes=4], [signed=], [endian=])` — the mirror of
/// `read_int`. Unlike the pre-existing `write_int16`/`write_int32`/etc.
/// (which just `as i16`/`as i32` cast with no range check, silently
/// wrapping an out-of-range value), `write_int` DOES validate `value` fits
/// in the requested width/signedness before writing, erroring clearly
/// otherwise. Deliberate divergence from the older functions' behavior,
/// not an oversight: those are narrower, already-shipped, per-width-named
/// functions where picking the wrong one is a script-author mistake at
/// the CALL SITE (`write_int16` vs `write_int32` is visually obvious);
/// `write_int`'s `bytes=`/`signed=` are runtime VALUES that can come from
/// anywhere (a variable, a computed field width), so a silently-wrapped
/// value here is a much easier and much harder to notice mistake — worth
/// the extra check per this library's own "specific and actionable errors,
/// never a silent wrong-value fallback" convention.
fn write_int(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let v = args.get(1).ok_or_else(|| EvalError {
        msg: "write_int(file, value, [bytes=], [signed=], [endian=]) needs a value".to_string(),
    })?;
    let n = v.as_num().map_err(|m| EvalError { msg: m })?;
    if !n.is_finite() || n.fract() != 0.0 {
        return e(format!("write_int: value must be a whole number, got {n}"));
    }
    let bytes = style_num(style, "bytes").map(|b| b as i64).unwrap_or(4);
    if !valid_int_width(bytes) {
        return e(format!("write_int: bytes must be 1, 2, 4, or 8, got {bytes}"));
    }
    let mut st = fh.lock().unwrap();
    file_check_writable(&st)?;
    let signed = resolve_signed(style, st.default_signed);
    let little = resolve_endian(style, st.default_little_endian)?;
    let (min, max): (f64, f64) = match (bytes, signed) {
        (1, true) => (i8::MIN as f64, i8::MAX as f64),
        (1, false) => (0.0, u8::MAX as f64),
        (2, true) => (i16::MIN as f64, i16::MAX as f64),
        (2, false) => (0.0, u16::MAX as f64),
        (4, true) => (i32::MIN as f64, i32::MAX as f64),
        (4, false) => (0.0, u32::MAX as f64),
        (8, true) => (i64::MIN as f64, i64::MAX as f64),
        (8, false) => (0.0, u64::MAX as f64),
        _ => unreachable!("bytes validated to be 1/2/4/8 above"),
    };
    if n < min || n > max {
        return e(format!(
            "write_int: {n} does not fit in {bytes} byte(s) {}(valid range {min}..={max})",
            if signed { "signed " } else { "unsigned " }
        ));
    }
    let out: Vec<u8> = match (bytes, signed) {
        (1, true) => vec![(n as i8) as u8],
        (1, false) => vec![n as u8],
        (2, true) => {
            let x = n as i16;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        (2, false) => {
            let x = n as u16;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        (4, true) => {
            let x = n as i32;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        (4, false) => {
            let x = n as u32;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        (8, true) => {
            let x = n as i64;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        (8, false) => {
            let x = n as u64;
            if little { x.to_le_bytes().to_vec() } else { x.to_be_bytes().to_vec() }
        }
        _ => unreachable!("bytes validated to be 1/2/4/8 above"),
    };
    // Exactly `bytes` bytes, one `write_all` — the handle's OS-level write
    // cursor advances by exactly that many bytes, matching `read_int`'s own
    // "advances by exactly `bytes`" contract on the read side.
    st.writer.as_mut().unwrap().write_all(&out).map_err(|err| EvalError {
        msg: format!("write_int: could not write `{}`: {err}", st.path),
    })?;
    Ok(Value::Nothing)
}

// ================= raw byte buffers: read_bin/write_bin =================

/// `read_bin(f, n)` — reads up to `n` raw bytes from the current position,
/// returned as a `Value::Vec` of numbers 0-255 (this codebase's plain
/// numeric-array type; there is no separate "bytes" `Value` variant, and a
/// homogeneous 0-255 numeric array is exactly what `read_byte`'s own
/// single-byte return already establishes as "how one byte looks as a
/// `Value`" — `read_bin` is just that, N at a time). Deliberately mirrors
/// `read_chars`/`read_byte`'s "graceful partial read" convention, not the
/// typed-int family's "`Nothing` unless the FULL width is available":
/// `read_bin` is a general "give me the next up-to-`n` raw bytes" op, not a
/// fixed-width typed decode where a partial value would be meaningless, so
/// hitting EOF partway through just returns however many bytes were
/// actually available (composable for "drain a file in chunks" loops
/// without a separate `eof` check on every iteration). `Value::Nothing`
/// only when zero bytes remain at all — the same "exhausted" signal every
/// other read here uses.
fn read_bin(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let n = int_arg(args, 1)?;
    if n < 0 {
        return e(format!("read_bin: n must be non-negative, got {n}"));
    }
    let mut st = fh.lock().unwrap();
    file_check_readable(&st)?;
    file_align_to_byte(&mut st);
    let avail = file_remaining(&st);
    if avail == 0 {
        return Ok(Value::Nothing);
    }
    let take = (n as usize).min(avail);
    // § real streaming reads (2026-08-31): reads exactly `take` bytes off
    // the stream rather than slicing a whole-file buffer.
    let bytes = file_read_upto(&mut st, take);
    Ok(Value::Vec(Arc::new(bytes.into_iter().map(|b| b as f64).collect())))
}

/// `write_bin(f, bytes)` — the inverse of `read_bin`/binary-mode `read_all`:
/// accepts the same representation those return (a numeric `Value::Vec`,
/// each element 0-255 — also accepts a plain `List`/`Signal`/etc. via the
/// existing `to_vec` coercion, so e.g. a `List` literal of byte values
/// works too), validates every element is an integer 0-255, and writes the
/// raw bytes.
fn write_bin(args: &[Value]) -> R<Value> {
    let fh = as_file(arg0(args)?)?;
    let v = args.get(1).ok_or_else(|| EvalError {
        msg: "write_bin(file, bytes) needs a byte array, e.g. from read_bin/read_all".to_string(),
    })?;
    let nums = to_vec(v)?;
    let mut out = Vec::with_capacity(nums.len());
    for (i, x) in nums.iter().enumerate() {
        if !x.is_finite() || x.fract() != 0.0 || *x < 0.0 || *x > 255.0 {
            return e(format!("write_bin: element {i} ({x}) is not a byte (must be a whole number 0-255)"));
        }
        out.push(*x as u8);
    }
    let mut st = fh.lock().unwrap();
    file_check_writable(&st)?;
    st.writer.as_mut().unwrap().write_all(&out).map_err(|err| EvalError {
        msg: format!("write_bin: could not write `{}`: {err}", st.path),
    })?;
    Ok(Value::Nothing)
}

// ================= directory / filesystem =================

/// `cd(path)` — changes the process's current working directory. Errors
/// clearly (checked up front, not just left to `set_current_dir`'s own OS
/// error) if `path` doesn't exist or isn't a directory.
fn cd(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let p = std::path::Path::new(&path);
    if !p.is_dir() {
        return e(format!("cd: `{path}` does not exist or is not a directory"));
    }
    std::env::set_current_dir(p).map_err(|err| EvalError {
        msg: format!("cd: could not change into `{path}`: {err}"),
    })?;
    Ok(Value::Nothing)
}

/// `pwd()` — the current working directory, as a string.
fn pwd() -> R<Value> {
    let p = std::env::current_dir().map_err(|err| EvalError {
        msg: format!("pwd: could not determine the current directory: {err}"),
    })?;
    Ok(Value::Str(p.display().to_string()))
}

/// `file_exists(path)` — `true` iff `path` exists AND is a regular file.
/// A path that doesn't exist at all returns `false`, not an error — same
/// for `dir_exists` below — matching how most languages' own `exists`-style
/// checks behave (the absence itself is the answer, not exceptional).
fn file_exists(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    Ok(Value::Bool(std::path::Path::new(&path).is_file()))
}

/// `dir_exists(path)` — `true` iff `path` exists AND is a directory.
fn dir_exists(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    Ok(Value::Bool(std::path::Path::new(&path).is_dir()))
}

/// `mkdir(path)` — creates the directory, including any missing parent
/// directories (`create_dir_all`). Idempotent: returns cleanly (not an
/// error) if the directory already exists — `create_dir_all` already has
/// exactly this behavior, matching "make sure this directory exists" more
/// than a strict "create, error if present" `mkdir -p` vs. bare `mkdir`
/// distinction most script authors don't actually want.
fn mkdir(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    std::fs::create_dir_all(&path).map_err(|err| EvalError {
        msg: format!("mkdir: could not create `{path}`: {err}"),
    })?;
    Ok(Value::Nothing)
}

/// `make_file(path)` — creates an empty file, or TRUNCATES an existing one
/// to empty. Truncate-to-empty was chosen (over "error if it already
/// exists" or "leave existing content alone") to match `fopen(path, "w")`'s
/// own already-established truncate-on-open convention — `make_file` reads
/// as an assertion "this path is now a fresh empty file," consistent with
/// the one other place this codebase already makes that same promise.
fn make_file(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    std::fs::File::create(&path).map_err(|err| EvalError {
        msg: format!("make_file: could not create `{path}`: {err}"),
    })?;
    Ok(Value::Nothing)
}

/// `on_exists=` reader shared by `rename_file`/`move_file`/`copy_file`/
/// `create_file`: validates the value up front (not just when a collision
/// actually happens) so a typo'd policy fails immediately rather than only
/// on the one run where the target happens to already exist.
fn on_exists_policy(style: &[(String, Value)], caller: &str) -> R<String> {
    let v = style_str(style, "on_exists").unwrap_or_else(|| "error".to_string());
    match v.as_str() {
        "error" | "overwrite" | "skip" => Ok(v),
        other => e(format!(
            "{caller}: on_exists must be \"error\", \"overwrite\" or \"skip\", got \"{other}\""
        )),
    }
}

/// Applies an already-validated `on_exists` policy against `target`.
/// `Ok(true)` — proceed; `Ok(false)` — target exists and the policy is
/// `"skip"`, so the caller does nothing and reports that back; `Err` —
/// target exists and the policy is `"error"` (the default: a collision is
/// a mistake to surface, not a mistake to paper over silently).
fn check_collision(target: &str, policy: &str, caller: &str) -> R<bool> {
    if !std::path::Path::new(target).exists() {
        return Ok(true);
    }
    match policy {
        "overwrite" => Ok(true),
        "skip" => Ok(false),
        _ => e(format!(
            "{caller}: `{target}` already exists (pass on_exists=\"overwrite\" or on_exists=\"skip\" to allow this)"
        )),
    }
}

/// `remove_file(path, [recycle_bin=false])` — deletes a file. Errors
/// clearly if `path` doesn't exist or isn't a regular file (checked up
/// front, matching `cd`'s own "check first, don't just surface the raw OS
/// error" convention) rather than leaving that to `remove`/`trash::delete`'s
/// own error text. `recycle_bin=true` sends it to the OS Recycle Bin/Trash
/// (via the `trash` crate — see this crate's `Cargo.toml` for why that
/// needs a crate at all) instead of a permanent delete; default `false`
/// keeps the previously-only-available behavior as the default, since a
/// script that has always run permanent-delete should not silently start
/// leaving recoverable copies behind after this builtin was added.
fn remove_file(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    if !std::path::Path::new(&path).is_file() {
        return e(format!("remove_file: `{path}` does not exist or is not a file"));
    }
    let recycle = style_entry(style, "recycle_bin")
        .map(|(_, v)| truthy(v))
        .unwrap_or(false);
    if recycle {
        trash::delete(&path).map_err(|err| EvalError {
            msg: format!("remove_file: could not send `{path}` to the Recycle Bin: {err}"),
        })?;
    } else {
        std::fs::remove_file(&path).map_err(|err| EvalError {
            msg: format!("remove_file: could not delete `{path}`: {err}"),
        })?;
    }
    Ok(Value::Nothing)
}

/// `remove_dir(path, [recursive=false], [recycle_bin=false])` — deletes a
/// directory. `recursive` gates whether a NON-EMPTY directory may be
/// removed at all: default `false` means this only succeeds on an already-
/// empty directory, checked explicitly (via `read_dir` returning nothing)
/// so the refusal names the real reason rather than surfacing
/// `remove_dir`'s own "directory not empty" OS error text verbatim. This
/// gate applies whether or not `recycle_bin` is set — recoverable is not
/// the same question as "did the caller mean to remove a whole tree,"
/// which is what `recursive` answers.
fn remove_dir(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    if !std::path::Path::new(&path).is_dir() {
        return e(format!("remove_dir: `{path}` does not exist or is not a directory"));
    }
    let recursive = style_entry(style, "recursive")
        .map(|(_, v)| truthy(v))
        .unwrap_or(false);
    let recycle = style_entry(style, "recycle_bin")
        .map(|(_, v)| truthy(v))
        .unwrap_or(false);
    if !recursive {
        let non_empty = std::fs::read_dir(&path)
            .map_err(|err| EvalError {
                msg: format!("remove_dir: could not inspect `{path}`: {err}"),
            })?
            .next()
            .is_some();
        if non_empty {
            return e(format!(
                "remove_dir: `{path}` is not empty (pass recursive=true to remove it and everything in it)"
            ));
        }
    }
    if recycle {
        trash::delete(&path).map_err(|err| EvalError {
            msg: format!("remove_dir: could not send `{path}` to the Recycle Bin: {err}"),
        })?;
    } else if recursive {
        std::fs::remove_dir_all(&path).map_err(|err| EvalError {
            msg: format!("remove_dir: could not delete `{path}`: {err}"),
        })?;
    } else {
        std::fs::remove_dir(&path).map_err(|err| EvalError {
            msg: format!("remove_dir: could not delete `{path}`: {err}"),
        })?;
    }
    Ok(Value::Nothing)
}

/// `rename_file(old, new, [on_exists="error"])` — a plain filesystem
/// rename (`std::fs::rename`, generally a same-filesystem operation).
/// Distinct from `move_file` below on purpose, matching how the two read
/// as different intents even though a same-filesystem `move_file` could
/// share the implementation: `rename_file` does not fall back to copy+
/// delete on failure, so a cross-drive `rename_file` fails with a clear
/// error naming the real reason instead of silently becoming a slower,
/// different operation than the one its name promises.
fn rename_file(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let old = text_arg(args, 0)?;
    let new = text_arg(args, 1)?;
    if !std::path::Path::new(&old).exists() {
        return e(format!("rename_file: `{old}` does not exist"));
    }
    let policy = on_exists_policy(style, "rename_file")?;
    if !check_collision(&new, &policy, "rename_file")? {
        return Ok(Value::Bool(false));
    }
    std::fs::rename(&old, &new).map_err(|err| EvalError {
        msg: format!("rename_file: could not rename `{old}` to `{new}`: {err}"),
    })?;
    Ok(Value::Bool(true))
}

/// `move_file(src, dst, [on_exists="error"])` — relocates a file, possibly
/// across drives. Tries `std::fs::rename` first (fast, atomic, the common
/// same-filesystem case), and on ANY failure from that falls back to
/// copy-then-remove-the-source instead of trying to distinguish "failed
/// because cross-device" from every other reason `rename` can fail on a
/// given platform (the exact `io::ErrorKind` for that case is not
/// consistently reported across Windows/macOS/Linux) — the fallback is
/// only ever taken after `check_collision` has already confirmed the
/// destination is clear to write, so it cannot silently overwrite
/// something the rename path would have refused.
fn move_file(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let src = text_arg(args, 0)?;
    let dst = text_arg(args, 1)?;
    if !std::path::Path::new(&src).is_file() {
        return e(format!("move_file: `{src}` does not exist or is not a file"));
    }
    let policy = on_exists_policy(style, "move_file")?;
    if !check_collision(&dst, &policy, "move_file")? {
        return Ok(Value::Bool(false));
    }
    if std::fs::rename(&src, &dst).is_err() {
        std::fs::copy(&src, &dst).map_err(|err| EvalError {
            msg: format!("move_file: could not copy `{src}` to `{dst}`: {err}"),
        })?;
        std::fs::remove_file(&src).map_err(|err| EvalError {
            msg: format!(
                "move_file: copied `{src}` to `{dst}` but could not remove the original: {err}"
            ),
        })?;
    }
    Ok(Value::Bool(true))
}

/// `copy_file(src, dst, [on_exists="error"])` — the copy `std::fs` and this
/// library both otherwise lack: `fopen`+`read_all`+`write_bin` can do it in
/// three calls, but a single builtin with the same collision policy as
/// `move_file`/`rename_file` is the obvious missing 5th operation once
/// those three exist, not a new design.
fn copy_file(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let src = text_arg(args, 0)?;
    let dst = text_arg(args, 1)?;
    if !std::path::Path::new(&src).is_file() {
        return e(format!("copy_file: `{src}` does not exist or is not a file"));
    }
    let policy = on_exists_policy(style, "copy_file")?;
    if !check_collision(&dst, &policy, "copy_file")? {
        return Ok(Value::Bool(false));
    }
    std::fs::copy(&src, &dst).map_err(|err| EvalError {
        msg: format!("copy_file: could not copy `{src}` to `{dst}`: {err}"),
    })?;
    Ok(Value::Bool(true))
}

/// `create_file(path, [on_exists="error"])` — `make_file` with a collision
/// policy. `make_file` itself stays exactly as it is (always truncates, no
/// policy) rather than gaining one: it already has an established, relied-
/// on contract, and changing its default behavior would silently change
/// what every existing caller does. This is the new, separate builtin for
/// callers that need to say what should happen when the path is already
/// there.
fn create_file(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let policy = on_exists_policy(style, "create_file")?;
    if !check_collision(&path, &policy, "create_file")? {
        return Ok(Value::Bool(false));
    }
    std::fs::File::create(&path).map_err(|err| EvalError {
        msg: format!("create_file: could not create `{path}`: {err}"),
    })?;
    Ok(Value::Bool(true))
}

static TMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `tmp_file()` — creates a new, empty file with a guaranteed-unique name
/// under the OS temp directory, and returns its path as a string (not an
/// already-open `Value::File` handle: checked `docs/qu-language-spec.md`
/// for any existing `tmp_file`/temp-file convention first — grepped "temp",
/// found none — so there's no established shape to match; returning just
/// the path and letting the caller `fopen` it themselves is the simpler,
/// more composable choice, and keeps `tmp_file` symmetric with every other
/// path-returning builtin here rather than being the one file builtin that
/// hands back a live handle instead of a name). Uniqueness comes from
/// combining the process id, a nanosecond wall-clock timestamp, AND a
/// `static AtomicU64` counter — any one alone has a plausible collision
/// window (two calls in the same process within one clock tick would tie
/// on timestamp+pid alone), the three combined do not.
fn tmp_file() -> R<Value> {
    let counter = TMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("qu_tmp_{pid}_{nanos}_{counter}.tmp");
    let path = std::env::temp_dir().join(name);
    std::fs::File::create(&path).map_err(|err| EvalError {
        msg: format!("tmp_file: could not create a temp file at `{}`: {err}", path.display()),
    })?;
    Ok(Value::Str(path.display().to_string()))
}

/// `list_files([dir], [pattern], [filter=], [contains=], [size=],
/// [recursive=])` — the files matching a wildcard pattern, as a `List` of
/// paths, sorted.
///
/// This is the WALKING counterpart of `list_dir(path, pattern)`, which can
/// only filter the bare names inside the one directory it is handed. Here a
/// pattern is a whole path, so a wildcard may appear in any segment, and
/// the answers come back as paths you can open:
///
/// ```text
/// list_files("C:/users/pc", "*.html")   a directory and a name pattern
/// list_files("*.html")                  a pattern alone, in the current directory
/// list_files("website/**/*.html")       `**` walks the whole subtree
/// list_files()                          every file here
/// list_files(filter = "*.qu", contains = "Hi!", size = ">2MB")
/// ```
///
/// **Files only**, never directories — that is the whole difference from
/// `list_dir`, and it is what makes `contains=` and `size=` mean something
/// on every entry that comes back.
///
/// Three wildcards, and the third is the reason this exists at all:
///   - `*`  — any run of characters, within one path segment
///   - `?`  — exactly one character, within one path segment
///   - `**` — zero or more whole segments, so `a/**/b.txt` matches
///            `a/b.txt`, `a/x/b.txt` and `a/x/y/b.txt` alike
///
/// `*` deliberately does NOT cross a separator, which is why `**` has to be
/// spelled differently: if it did, `website/*.html` would also match
/// `website/fn/add.html` and the common case would be wrong. Every shell
/// and every language that ships a glob draws the line in this same place.
///
/// **How the positional arguments are read.** One argument is a PATTERN if
/// it has a wildcard in it and a DIRECTORY if it does not — so
/// `list_files("*.html")` and `list_files("C:/users/pc")` both do the
/// obvious thing, and neither needs a keyword to say which it meant. Two
/// arguments are always `(dir, pattern)`. `filter=` is the keyword spelling
/// of the pattern; passing both a positional pattern and `filter=` is an
/// error rather than a silent pick between them.
///
/// **The filters** are applied in the order that reads the fewest bytes:
/// the name pattern first (no file is opened), then `size=` (a metadata
/// call), then `contains=` (which has to read the file). Pairing
/// `contains=` with a pattern or a size bound is what keeps a search over a
/// large tree cheap.
///   - `contains=` — keep a file only if its bytes contain this text.
///     Matched against the raw bytes, so it works on any file and never
///     fails on one that is not valid UTF-8. Case-sensitive.
///   - `size=` — a comparison against the file's byte size, written as a
///     string: `">2MB"`, `"<=100KB"`, `"=0"`, `"!=0"`. See `size_pred`.
///   - `recursive=` — search subdirectories too, without writing `**`
///     yourself. `list_files(d, "*.html", recursive = true)` is exactly
///     `list_files(d, "**/*.html")`.
///
/// Both `/` and `\` separate segments in the PATTERN — a Windows script
/// should not have to rewrite its paths to search them — and the paths that
/// come back are joined with the platform's own separator, the same one
/// `cur_dir` returns.
///
/// A pattern with no wildcard in it is neither an error nor a special case:
/// it matches iff that exact file exists, so `list_files(p)` on a plain
/// name is a list-shaped `file_exists(p)`. A search matching nothing gives
/// an empty `List` rather than an error — absence is the answer, the same
/// reading `file_exists`/`dir_exists` already take.
///
/// Hidden files are included: `read_dir` reports them like any other name,
/// and dropping them here would be this library inventing a convention the
/// OS does not have.
fn list_files(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let pat_arg = style_str(style, "filter");
    let recursive = style_entry(style, "recursive")
        .map(|(_, v)| truthy(v))
        .unwrap_or(false);
    let contains = style_str(style, "contains");
    let size = match style_str(style, "size") {
        Some(spec) => Some(size_pred(&spec)?),
        None => None,
    };

    // `list_files("*.html")` vs `list_files("C:/users/pc")`: a wildcard is
    // what tells them apart, so neither spelling needs a keyword to
    // disambiguate it.
    let (dir, mut name_pat) = match (args.first(), args.get(1)) {
        (None, _) => (String::new(), "*".to_string()),
        (Some(a), None) => {
            let s = display_value(a);
            if s.contains('*') || s.contains('?') {
                (String::new(), s)
            } else {
                (s, "*".to_string())
            }
        }
        (Some(a), Some(b)) => (display_value(a), display_value(b)),
    };
    if let Some(f) = pat_arg {
        // A positional pattern AND `filter=` have two honest readings and
        // this picks neither.
        if name_pat != "*" {
            return e(format!(
                "list_files: given both a positional pattern (`{name_pat}`) and \
                 filter=`{f}` -- pass one or the other"
            ));
        }
        name_pat = f;
    }
    if recursive {
        // `**/` goes in front of the NAME, not the whole pattern, so
        // `list_files("a/b/*.txt", recursive=true)` still starts at `a/b`.
        name_pat = match name_pat.rsplit_once(['/', '\\']) {
            Some((head, tail)) => format!("{head}/**/{tail}"),
            None => format!("**/{name_pat}"),
        };
    }
    let pattern = if dir.is_empty() {
        name_pat
    } else {
        format!("{}/{}", dir.trim_end_matches(['/', '\\']), name_pat)
    };

    let mut found = walk_pattern(&pattern);
    if let Some(pred) = &size {
        found.retain(|p| match std::fs::metadata(p) {
            Ok(m) => pred.holds(m.len()),
            // A file that vanished between the walk and the stat is not
            // an answer to the query.
            Err(_) => false,
        });
    }
    if let Some(needle) = &contains {
        let needle = needle.as_bytes();
        found.retain(|p| match std::fs::read(p) {
            Ok(bytes) => bytes
                .windows(needle.len().max(1))
                .any(|w| w == needle)
                || needle.is_empty(),
            Err(_) => false,
        });
    }
    Ok(Value::List(Arc::new(found.into_iter().map(Value::Str).collect())))
}

/// A parsed `size=` comparison: an operator and a byte count.
struct SizePred {
    op: String,
    bytes: u64,
}

impl SizePred {
    fn holds(&self, n: u64) -> bool {
        match self.op.as_str() {
            ">" => n > self.bytes,
            ">=" => n >= self.bytes,
            "<" => n < self.bytes,
            "<=" => n <= self.bytes,
            "!=" => n != self.bytes,
            _ => n == self.bytes,
        }
    }
}

/// `size=">2MB"` — an operator, a number, and a unit.
///
/// Operators: `>`, `>=`, `<`, `<=`, `=`, `==`, `!=`. With no operator at
/// all the comparison is equality, which is only really useful for
/// `size="0"` — but that IS the query "which of these files are empty", so
/// it earns the default over a guess like `>=`.
///
/// **`KB`/`MB`/`GB` are 1024-based here**, the way a file manager reports a
/// size, not the 1000-based SI reading. `KiB`/`MiB`/`GiB` are accepted as
/// explicit spellings of those same values, so a script that wants to be
/// unambiguous can say so. A bare number is bytes.
///
/// A unit this does not know is an error rather than a silent "assume
/// bytes": `size=">2Mb"` is a typo worth hearing about, and reading it as
/// 2 bytes would return a wrong answer that looks like a right one.
fn size_pred(spec: &str) -> R<SizePred> {
    let s = spec.trim();
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = s.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = s.strip_prefix("!=") {
        ("!=", r)
    } else if let Some(r) = s.strip_prefix("==") {
        ("=", r)
    } else if let Some(r) = s.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = s.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = s.strip_prefix('=') {
        ("=", r)
    } else {
        ("=", s)
    };
    let rest = rest.trim();
    let split = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_'))
        .unwrap_or(rest.len());
    let (num, unit) = rest.split_at(split);
    let num: f64 = num.replace('_', "").parse().map_err(|_| EvalError {
        msg: format!("size: `{spec}` has no number in it -- write it like \">2MB\" or \"=0\""),
    })?;
    let scale: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" | "byte" | "bytes" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        "t" | "tb" | "tib" => 1024u64 * 1024 * 1024 * 1024,
        other => {
            return e(format!(
                "size: `{other}` is not a unit I know -- use B, KB, MB, GB or TB \
                 (1024-based, as a file manager reports them)"
            ))
        }
    };
    if !num.is_finite() || num < 0.0 {
        return e(format!("size: `{spec}` is not a size"));
    }
    Ok(SizePred {
        op: op.to_string(),
        bytes: (num * scale as f64) as u64,
    })
}

/// Every FILE matching a whole-path wildcard pattern, unsorted.
///
/// Split out of `list_files` so the pattern walk is one thing and the
/// `size=`/`contains=` filtering another; `glob(pattern)` is this and
/// nothing else.
fn walk_pattern(pattern: &str) -> Vec<String> {
    let segs: Vec<&str> = pattern.split(['/', '\\']).collect();

    // The literal prefix is WALKED, not matched. `C:\Users\*\x` and
    // `/home/*/x` both open with a root no wildcard can sensibly apply to,
    // and a leading empty segment (from an absolute `/x`) has to become
    // part of that root rather than a segment that matches nothing.
    let mut base = std::path::PathBuf::new();
    let mut first_wild = 0usize;
    for (i, seg) in segs.iter().enumerate() {
        if seg.contains('*') || seg.contains('?') {
            break;
        }
        if i == 0 && seg.is_empty() {
            base.push(std::path::MAIN_SEPARATOR.to_string());
        } else if i == 0 && seg.len() == 2 && seg.ends_with(':')
            && seg.as_bytes()[0].is_ascii_alphabetic()
        {
            // A bare Windows drive letter ("C:") is DRIVE-RELATIVE, not a
            // root. `PathBuf::push("C:")` then `push("Users")` builds
            // "C:Users", which resolves against the current directory ON
            // that drive -- not "C:\Users" -- so `list_files("C:\\Users\\
            // ...\\*.html")` walked a path that was never the one asked
            // for and silently found nothing there, in every case, on
            // every absolute Windows path. A trailing separator turns it
            // into an actual drive root.
            base.push(format!("{seg}{}", std::path::MAIN_SEPARATOR));
        } else {
            base.push(seg);
        }
        first_wild = i + 1;
    }
    if first_wild == segs.len() {
        // No wildcard anywhere: the pattern names one path, and it is an
        // answer only if it is a file.
        let mut out = Vec::new();
        if base.is_file() {
            out.push(base.display().to_string());
        }
        return out;
    }

    // An empty base means the pattern started with a wildcard (`*.qu`),
    // which means "here" -- walked as `.`, but the pattern never asked for
    // that prefix so `render_path` takes it back off.
    let strip_dot = base.as_os_str().is_empty();
    let start = if strip_dot {
        std::path::PathBuf::from(".")
    } else {
        base
    };
    let mut found: Vec<String> = Vec::new();
    glob_walk(&start, &segs[first_wild..], strip_dot, &mut found);
    found.sort();
    found.dedup();
    found
}

/// One step of `glob`'s walk: match `segs[0]` against the entries of `at`,
/// then recurse on the rest.
///
/// `**` is the only segment that can consume more than one directory level,
/// and it does it by trying BOTH readings at every point — "match no
/// directory here, carry on with the rest of the pattern" and "swallow this
/// directory and try `**` again inside it". That is what makes `a/**/b`
/// match plain `a/b` as well as `a/x/y/b`.
///
/// A directory that cannot be read is skipped rather than fatal: a glob
/// over a tree containing one unreadable directory should still return
/// everything else, which is what a shell's own glob does.
///
/// Recursion is bounded by the tree, not by the pattern, so a symlink loop
/// under a `**` would otherwise spin forever. The descent tests
/// `entry.file_type()`, which is the entry's OWN type with no link
/// followed, so a symlinked directory is matched as an entry but never
/// walked into.
fn glob_walk(at: &std::path::Path, segs: &[&str], strip_dot: bool, out: &mut Vec<String>) {
    let Some(seg) = segs.first() else {
        return;
    };
    let rest = &segs[1..];

    if *seg == "**" {
        if rest.is_empty() {
            // A trailing `**` is the whole subtree below here.
            collect_tree(at, strip_dot, out);
            return;
        }
        // The "zero segments" reading.
        glob_walk(at, rest, strip_dot, out);
    }

    let Ok(entries) = std::fs::read_dir(at) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = at.join(&name);
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if *seg == "**" {
            if is_dir {
                glob_walk(&path, segs, strip_dot, out);
            }
            continue;
        }
        if !glob_match(seg, &name) {
            continue;
        }
        if rest.is_empty() {
            // Files only: a directory whose NAME matches the last segment
            // is not one of the files the caller asked for.
            if !is_dir {
                out.push(render_path(&path, strip_dot));
            }
        } else if is_dir {
            glob_walk(&path, rest, strip_dot, out);
        }
    }
}

/// Every file below `at`, at any depth — what a trailing `**` means.
///
/// Directories are descended into but never reported, the same "files
/// only" rule the rest of the walk follows.
fn collect_tree(at: &std::path::Path, strip_dot: bool, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(at) else {
        return;
    };
    for entry in entries.flatten() {
        let path = at.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect_tree(&path, strip_dot, out);
        } else {
            out.push(render_path(&path, strip_dot));
        }
    }
}

/// `list_files("*.qu")` is walked from `.`, so every path it builds opens with
/// `./`. The pattern did not ask for that prefix, so it does not come back
/// carrying one.
fn render_path(p: &std::path::Path, strip_dot: bool) -> String {
    let s = p.display().to_string();
    if !strip_dot {
        return s;
    }
    let lead = format!(".{}", std::path::MAIN_SEPARATOR);
    match s.strip_prefix(&lead) {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

/// Minimal shell-glob matcher for `list_dir(path, pattern)`: `*` matches any
/// run of characters (including none), `?` matches exactly one character —
/// no character classes/brace expansion/`**` recursion, since the ask was
/// specifically `*.qu`/`data_*.csv`-style single-segment filtering, not a
/// full POSIX glob. No glob crate exists in this workspace today (checked
/// `engine/crates/qu-interp/Cargo.toml` and the workspace root first), and
/// this doesn't need one — hand-rolled here instead of adding a dependency
/// for two wildcard characters. Classic DP over (pattern, text):
/// `dp[i][j]` = does `pattern[..i]` match `text[..j]`.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (plen, tlen) = (p.len(), t.len());
    let mut dp = vec![vec![false; tlen + 1]; plen + 1];
    dp[0][0] = true;
    for i in 1..=plen {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=plen {
        for j in 1..=tlen {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }
    dp[plen][tlen]
}

/// `list_dir(path, [pattern])` — an ordinary Qu `List` of filename strings
/// (bare names, e.g. `"a.qu"`, not full paths — checked `split`'s own
/// `Value::List(Arc::new(parts))` of `Value::Str` for the house "what does
/// a string-array Qu value look like" convention, matched exactly here;
/// there's no separate vector-of-strings `Value` variant, per `Table`'s own
/// doc comment). Returns BOTH files and subdirectories — the typical
/// `readdir`/`list_dir` expectation across most languages — rather than
/// picking one kind; a caller who wants only one can already filter with
/// the existing `file_exists`/`dir_exists` per entry. Sorted alphabetically
/// before returning: `std::fs::read_dir`'s own iteration order is OS/
/// filesystem-dependent and unspecified, and leaking that non-determinism
/// into script behavior would be exactly the kind of surprise this library
/// avoids elsewhere.
///
/// **`for f in list_dir(path)` works.** This comment used to say, at
/// length and with evidence, that it did not: that `for <name> in <expr>`
/// was a parse error outside `parallel for`, and that `Stmt::For` had no
/// case for `Value::List` even when spelled `for f = ...`. Both were true
/// when written (2026-08-26) and neither is true now — list iteration
/// landed in the meantime, and re-checked here 2026-09-09:
///
/// ```text
/// names = list_dir("tools")
/// for f in names
///     print(f)
/// end for
/// ```
///
/// The indexing form the old note recommended (`for i = 0 to
/// length(names) - 1`) still works and is what this function's own test
/// below exercises; it is no longer the only option. Left as a correction
/// rather than deleted, because a reader who saw the old claim elsewhere
/// should find out here that it expired.
fn list_dir(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let pattern = args.get(1).map(display_value);
    let entries = std::fs::read_dir(&path).map_err(|err| EvalError {
        msg: format!("list_dir: could not read `{path}`: {err}"),
    })?;
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| EvalError {
            msg: format!("list_dir: error reading an entry in `{path}`: {err}"),
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(pat) = &pattern {
            if !glob_match(pat, &name) {
                continue;
            }
        }
        names.push(name);
    }
    names.sort();
    Ok(Value::List(Arc::new(names.into_iter().map(Value::Str).collect())))
}

// ================= type casts: to_int/to_float/to_bool =================
//
// `str(x)` was the only cast that existed before this pass (grepped first
// to confirm — no `int(x)`/`float(x)`/etc. anywhere). Named `to_int`/
// `to_float`/`to_bool` rather than `int`/`float`/`bool` since those shorter
// names risk colliding with a future type-name/annotation keyword in this
// still-evolving, MATLAB-flavored language — `to_*` is unambiguous.

/// `to_int(x)` — numeric round-to-nearest, MATLAB `int32`-style (round,
/// not truncate): reuses `f64::round` (round-half-away-from-zero), the
/// exact same primitive the pre-existing `round` builtin already uses
/// (`"round" => e1(f64::round, ...)`), so `to_int` and `round` agree on
/// ties by construction rather than by coincidence. A string argument is
/// parsed as a number first, then rounded the same way; a string that
/// doesn't parse as a number is a CLEAR ERROR, never a silent `0` — the
/// one behavior this builtin was explicitly asked never to have. `Bool` is
/// also accepted (`true`/`false` -> `1`/`0`) as a small, harmless, and
/// clearly-documented extension beyond the two required input kinds.
fn to_int(args: &[Value]) -> R<Value> {
    let v = arg0(args)?;
    let n = match v {
        Value::Num(n) => *n,
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        Value::Str(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| EvalError { msg: format!("to_int: \"{s}\" is not a valid number") })?,
        other => return e(format!("to_int: cannot convert a {} to a number", other.type_name())),
    };
    if !n.is_finite() {
        return e(format!("to_int: cannot convert {n} to an integer (not finite)"));
    }
    Ok(Value::Num(n.round()))
}

/// `to_float(x)` — a number passes through unchanged; a string is parsed
/// as a float; anything else (including an unparseable string) is a clear
/// error, never a silent `0.0`. `Bool` accepted for the same reason as
/// `to_int` above.
fn to_float(args: &[Value]) -> R<Value> {
    let v = arg0(args)?;
    let n = match v {
        Value::Num(n) => *n,
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        Value::Str(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| EvalError { msg: format!("to_float: \"{s}\" is not a valid number") })?,
        other => return e(format!("to_float: cannot convert a {} to a number", other.type_name())),
    };
    Ok(Value::Num(n))
}

/// `to_bool(x)` — for every NON-string `x`, this is exactly `truthy(x)`
/// (the same function `if`/`and`/`or`/`while` already use — see its own
/// doc comment in `lib.rs` for the per-type rules, e.g. `to_bool(0)` is
/// `false`, `to_bool([])` is `false`, a nonempty-but-all-zero `Vec` is
/// still `true` since `truthy` on a `Vec` means "nonempty," not "any
/// nonzero element"). A bare `Value::Str`, though, is handled specially
/// rather than by calling `truthy` on it directly: `truthy`'s own string
/// rule is "nonempty string is truthy" — literally ANY non-empty text,
/// including `"nope"` or `"maybe"` — which would make `to_bool` on a
/// string never able to fail, unlike `to_int`/`to_float`'s "clear error on
/// an unparseable string" contract this cast family was explicitly asked
/// to have. So `to_bool` on a string instead requires `"true"`/`"false"`
/// (case-insensitive) or a numeric string (`n != 0`), erroring clearly on
/// anything else. A deliberate, documented deviation from "reuse `truthy`'s
/// string rule verbatim" for this one input kind — spelled out here rather
/// than silently applied, per this pass's own "flag anything deliberately
/// different, don't just do it quietly" instruction.
fn to_bool(args: &[Value]) -> R<Value> {
    let v = arg0(args)?;
    if let Value::Str(s) = v {
        let t = s.trim();
        return match t.to_ascii_lowercase().as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => match t.parse::<f64>() {
                Ok(n) => Ok(Value::Bool(n != 0.0)),
                Err(_) => e(format!(
                    "to_bool: \"{s}\" is not a recognized boolean — use \"true\"/\"false\" or a number"
                )),
            },
        };
    }
    Ok(Value::Bool(truthy(v)))
}

/// `file_size(path)` -- bytes, without opening or reading the file.
///
/// A directory of instrument captures is filtered by size before anything
/// is read: a run that was cut short is a different length, and reading it
/// to find that out costs the read. `list_dir` gives names only.
fn file_size(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let meta = std::fs::metadata(&path).map_err(|err| EvalError {
        msg: format!("file_size: could not stat `{path}`: {err}"),
    })?;
    Ok(Value::Num(meta.len() as f64))
}

/// `read_array(path, type = "f64", endian = "little")` -- a whole binary
/// file as numbers, in one call.
///
/// `read_bin` returns one element per BYTE and `read_double` returns one
/// value per call, so a raw instrument capture -- 100 000 float64 samples
/// per channel, a few hundred files -- could only be read by reassembling
/// bytes in script or by making a million builtin calls. Neither is a way
/// to read a measurement. This is the bulk form: the file is a typed
/// array, and it comes back as a vector.
///
/// A file whose length is not a whole number of elements is an error
/// rather than a truncation: a short read of a capture is a corrupt
/// capture, and silently dropping the tail would put a fractional cycle
/// into someone's FFT.
fn read_array(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let ty = crate::style_entry(style, "type")
        .map(|(_, v)| display_value(v))
        .unwrap_or_else(|| "f64".to_string());
    let little = match crate::style_entry(style, "endian") {
        Some((_, v)) => {
            let s = display_value(v);
            match s.as_str() {
                "little" | "le" => true,
                "big" | "be" => false,
                other => {
                    return e(format!(
                        "read_array: endian must be \"little\" or \"big\", got `{other}`"
                    ))
                }
            }
        }
        None => true,
    };
    // One shared table with `read_values` and the struct reads, so a type
    // cannot mean 8 bytes in one of them and 4 in another.
    let width = crate::binary_type_width(&ty)?;
    let bytes = std::fs::read(&path).map_err(|err| EvalError {
        msg: format!("read_array: could not read `{path}`: {err}"),
    })?;
    if bytes.len() % width != 0 {
        return e(format!(
            "read_array: `{path}` is {} bytes, not a whole number of {ty} values ({width} bytes each)",
            bytes.len()
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / width);
    for chunk in bytes.chunks_exact(width) {
        out.push(crate::decode_binary_value(&ty, chunk, little));
    }
    Ok(Value::Vec(std::sync::Arc::new(out)))
}

/// `write_array(path, data, [type="f64"], [endian="little"])` — the write
/// side `read_array` never had: every element of `data` (a `Vec`, `Mat`,
/// `Signal`, or bare scalar/`Bool` — anything `to_vec` already accepts,
/// the same acceptance `sum`/`mean`/etc. get for free) encoded as `type`
/// and written to `path` in one shot, overwriting whatever was there.
/// A `Mat` is written in its own column-major storage order (see
/// `Value::Mat`'s doc comment) — round-trips through `read_array` plus a
/// `reshape`/`-> (r, c)` back to the original shape, not through
/// `read_array` alone, since a flat file has no shape of its own to
/// recover.
fn write_array(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let data = args.get(1).ok_or_else(|| EvalError {
        msg: "write_array(path, data) needs a value to write".to_string(),
    })?;
    let values = to_vec(data)?;
    let ty = crate::style_entry(style, "type")
        .map(|(_, v)| display_value(v))
        .unwrap_or_else(|| "f64".to_string());
    let little = match crate::style_entry(style, "endian") {
        Some((_, v)) => {
            let s = display_value(v);
            match s.as_str() {
                "little" | "le" => true,
                "big" | "be" => false,
                other => {
                    return e(format!(
                        "write_array: endian must be \"little\" or \"big\", got `{other}`"
                    ))
                }
            }
        }
        None => true,
    };
    // Validated up front (same as `binary_type_width`'s own check) so a
    // bad `type=` fails before any byte of a possibly-large array is
    // written, not partway through.
    let _ = crate::binary_type_width(&ty)?;
    let mut bytes = Vec::with_capacity(values.len() * crate::binary_type_width(&ty)?);
    for v in &values {
        bytes.extend(crate::encode_binary_value(&ty, *v, little));
    }
    std::fs::write(&path, &bytes).map_err(|err| EvalError {
        msg: format!("write_array: could not write `{path}`: {err}"),
    })?;
    Ok(Value::Nothing)
}
