//! § file metadata (2026-09-23) — `toolkit-file.md` §1's "metadata reads
//! beyond mtime" gap, confirmed genuinely missing by the same toolkit audit
//! that found `path_ops.rs`'s gap (see `BACKLOG.md`'s "Toolkit spec-vs-
//! reality audit" entry): nothing in the language could answer "when was
//! this created" or "is this read-only" before this module.
//!
//! Every timestamp here is Unix epoch **seconds** as an ordinary `f64`
//! (sub-second precision where the OS provides it, in the fractional part)
//! — the same representation `time()`/`now()` already use elsewhere in this
//! interpreter, so a script can compare a file's timestamp against
//! `time()` directly without a units mismatch.
//!
//! **`created_at` is not available everywhere.** Some POSIX filesystems
//! genuinely do not track file-creation time at all (`ctime` there means
//! "last metadata change", a different thing `std::fs::Metadata::created()`
//! correctly refuses to conflate with birth time). `created_at` surfaces
//! that refusal as a clear Qu error naming the reason, rather than silently
//! returning the modified time instead and giving a wrong answer that
//! merely looks right.

use std::time::UNIX_EPOCH;

use crate::{e, text_arg, EvalError, Value, R};

pub fn call(f: &str, args: &[Value], _style: &[(String, Value)]) -> R<Value> {
    match f {
        "created_at" => created_at(args),
        "accessed_at" => accessed_at(args),
        "modified_at" => modified_at(args),
        "is_readonly" => is_readonly(args),
        "file_info" => file_info(args),
        other => e(format!("file_meta: unknown function `{other}`")),
    }
}

fn epoch_secs(t: std::time::SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        // A timestamp before 1970 (some filesystems allow it) is still a
        // real answer -- negative seconds, not an error.
        Err(e) => -e.duration().as_secs_f64(),
    }
}

fn metadata_of(path: &str) -> R<std::fs::Metadata> {
    std::fs::metadata(path).map_err(|err| EvalError {
        msg: format!("`{path}`: could not read metadata: {err}"),
    })
}

fn created_at(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let md = metadata_of(&path)?;
    let t = md.created().map_err(|err| EvalError {
        msg: format!(
            "created_at: `{path}`: this filesystem does not report a creation time ({err})"
        ),
    })?;
    Ok(Value::Num(epoch_secs(t)))
}

fn accessed_at(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let md = metadata_of(&path)?;
    let t = md.accessed().map_err(|err| EvalError {
        msg: format!("accessed_at: `{path}`: could not read the access time ({err})"),
    })?;
    Ok(Value::Num(epoch_secs(t)))
}

fn modified_at(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let md = metadata_of(&path)?;
    let t = md.modified().map_err(|err| EvalError {
        msg: format!("modified_at: `{path}`: could not read the modified time ({err})"),
    })?;
    Ok(Value::Num(epoch_secs(t)))
}

fn is_readonly(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let md = metadata_of(&path)?;
    Ok(Value::Bool(md.permissions().readonly()))
}

/// `file_info(path)` — every metadata field this module exposes, bundled
/// into one Record, so a script that wants several of them makes one
/// filesystem call instead of four. `created_at` is `Nothing`, not a raised
/// error, when the filesystem doesn't support it here — a bundled call
/// failing outright over the one field that's genuinely platform-dependent
/// would defeat the point of bundling; the individual `created_at` builtin
/// above is where that case is still a real error, for a caller who
/// specifically wants to know.
fn file_info(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let md = metadata_of(&path)?;
    let created = md.created().ok().map(epoch_secs).map(Value::Num).unwrap_or(Value::Nothing);
    let accessed = md.accessed().ok().map(epoch_secs).map(Value::Num).unwrap_or(Value::Nothing);
    let modified = md.modified().ok().map(epoch_secs).map(Value::Num).unwrap_or(Value::Nothing);
    let fields: Vec<(String, Value)> = vec![
        ("path".to_string(), Value::Str(path.clone())),
        ("size".to_string(), Value::Num(md.len() as f64)),
        ("is_file".to_string(), Value::Bool(md.is_file())),
        ("is_dir".to_string(), Value::Bool(md.is_dir())),
        ("is_symlink".to_string(), Value::Bool(md.is_symlink())),
        ("readonly".to_string(), Value::Bool(md.permissions().readonly())),
        ("created_at".to_string(), created),
        ("accessed_at".to_string(), accessed),
        ("modified_at".to_string(), modified),
    ];
    Ok(Value::Record(std::sync::Arc::new(fields)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// `label` must be unique per call site -- tests in this module run in
    /// parallel within the same process, so sharing one filename across two
    /// tests (as an earlier version of this helper did, keyed only on the
    /// process id) is a real race: one test's `remove_file` can delete the
    /// path out from under another test still reading it.
    fn temp_file(label: &str, contents: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("qu_file_meta_ops_test_{}_{label}.tmp", std::process::id()));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents).unwrap();
        p
    }

    #[test]
    fn file_info_reports_size_and_kind_for_a_real_file() {
        let p = temp_file("info", b"hello world");
        let a = [Value::Str(p.to_string_lossy().to_string())];
        let Value::Record(fields) = file_info(&a).unwrap() else { panic!("expected a record") };
        let get = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        assert!(matches!(get("size"), Some(Value::Num(n)) if n == 11.0));
        assert!(matches!(get("is_file"), Some(Value::Bool(true))));
        assert!(matches!(get("is_dir"), Some(Value::Bool(false))));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn modified_at_is_a_recent_unix_timestamp() {
        let p = temp_file("mtime", b"x");
        let a = [Value::Str(p.to_string_lossy().to_string())];
        let Value::Num(secs) = modified_at(&a).unwrap() else { panic!("expected a number") };
        let now = epoch_secs(std::time::SystemTime::now());
        assert!((now - secs).abs() < 60.0, "expected a timestamp within the last minute, got {secs}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn a_missing_file_reports_a_clear_error() {
        let a = [Value::Str("this/path/does/not/exist/at/all.txt".to_string())];
        let err = file_info(&a).unwrap_err();
        assert!(err.msg.contains("could not read metadata"), "got: {}", err.msg);
    }
}
