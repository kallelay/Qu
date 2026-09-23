//! § path manipulation (2026-09-23) — `toolkit-file.md` §9's `path()` value
//! type, the doc's own flagged "single highest-value, zero-coverage" gap:
//! nothing in the language guarded against a script getting `/` vs `\`
//! wrong when building a path by hand. Confirmed absent by a full toolkit
//! audit the same day (see `BACKLOG.md`'s "Toolkit spec-vs-reality audit"
//! entry) before this module was written.
//!
//! Shipped as plain string-in/string-out flat builtins, not a new `Value`
//! variant — every existing path-taking builtin (`file_exists`, `fopen`,
//! `load_image`, ...) already takes a plain string, so a real `Path` type
//! would need those call sites to accept both forms or the whole surface to
//! migrate at once; neither is this module's job to decide (that's
//! `toolkit-file.md` §15's still-open object-model question). Strings that
//! happen to look like paths, manipulated by functions that know the shape,
//! is the smaller, backward-compatible slice of the same idea.
//!
//! **Separator policy**: every function here normalizes `\` to `/` on the
//! way in and returns `/`-separated results, on every platform, always —
//! not the platform-native separator `std::path::Path` would give back.
//! A script's own string comparisons and `split("/")` calls need a
//! predictable answer regardless of what OS `qu` happens to be running on;
//! silently returning `\`-separated results on Windows would just relocate
//! the exact bug this module exists to prevent.
//!
//! No filesystem access anywhere in this file except `path_absolute`'s
//! current-directory lookup — every other function is pure string algebra,
//! deliberately: a path function that stats the disk becomes wrong the
//! moment a caller wants to reason about a path that doesn't exist yet
//! (a save target, a path from a manifest, ...).

use crate::{e, text_arg, EvalError, Value, R};

pub fn call(f: &str, args: &[Value], _style: &[(String, Value)]) -> R<Value> {
    match f {
        "path_name" => path_name(args),
        "path_stem" => path_stem(args),
        "path_extension" => path_extension(args),
        "path_parent" => path_parent(args),
        "path_join" => path_join(args),
        "path_normalize" => path_normalize(args),
        "path_absolute" => path_absolute(args),
        "path_relative_to" => path_relative_to(args),
        other => e(format!("path: unknown function `{other}`")),
    }
}

/// `/`-normalized, with a run of consecutive slashes collapsed to one and
/// any trailing slash (besides a bare root `/`) stripped. The one thing
/// deliberately preserved: a leading `//` (rare, but meaningful on some
/// systems as a distinct root) collapses like any other run here — this
/// module targets the common case, not every UNC/POSIX edge case.
fn normalize_seps(p: &str) -> String {
    let unified = p.replace('\\', "/");
    let mut out = String::with_capacity(unified.len());
    let mut last_was_slash = false;
    for c in unified.chars() {
        if c == '/' {
            if !last_was_slash {
                out.push('/');
            }
            last_was_slash = true;
        } else {
            out.push(c);
            last_was_slash = false;
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// A leading `/` (POSIX/UNC-ish) or a drive letter (`C:`, `c:`) — the two
/// shapes `path_absolute`/`path_join`/`path_relative_to` need to recognize
/// to behave sensibly on a checkout that moves between Windows and POSIX
/// machines (this project's own working practice — see `board2.txt`).
fn is_absolute(p: &str) -> bool {
    if p.starts_with('/') {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// The final path component, keeping its extension — `path_name("a/b/c.txt")
/// == "c.txt"`. Empty for a path with no component at all (`""`, `"/"`).
fn path_name(args: &[Value]) -> R<Value> {
    let p = normalize_seps(&text_arg(args, 0)?);
    let name = p.rsplit('/').next().unwrap_or("").to_string();
    Ok(Value::Str(name))
}

/// The final component with its extension removed — matching the common
/// "a leading dot alone isn't an extension separator" convention, so
/// `path_stem(".gitignore") == ".gitignore"`, not `""`.
fn path_stem(args: &[Value]) -> R<Value> {
    let Value::Str(name) = path_name(args)? else { unreachable!() };
    Ok(Value::Str(split_ext(&name).0.to_string()))
}

/// The extension without its leading dot — `path_extension("c.tar.gz") ==
/// "gz"` (the LAST extension, matching every mainstream language's own
/// `.extension`, not a `.tar.gz`-aware multi-extension guess). `""` when
/// there is no extension, including the leading-dot-only case above.
fn path_extension(args: &[Value]) -> R<Value> {
    let Value::Str(name) = path_name(args)? else { unreachable!() };
    Ok(Value::Str(split_ext(&name).1.to_string()))
}

/// Splits a bare file name (no `/` in it) into `(stem, extension)`, the
/// shared logic `path_stem`/`path_extension` both need. A dot at position 0
/// starts the "dotfile" special case, `path_stem`'s own doc comment above.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) => (name, ""),
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    }
}

/// Everything before the final component — `path_parent("a/b/c.txt") ==
/// "a/b"`. `""` for a path with no parent (a bare name, or a root already).
fn path_parent(args: &[Value]) -> R<Value> {
    let p = normalize_seps(&text_arg(args, 0)?);
    match p.rfind('/') {
        Some(0) => Ok(Value::Str("/".to_string())), // parent of "/x" is the root "/"
        Some(i) => Ok(Value::Str(p[..i].to_string())),
        None => Ok(Value::Str(String::new())),
    }
}

/// Joins every argument left to right with `/`, the way `os.path.join`
/// does in every language that has one: a later argument that is itself
/// absolute discards everything accumulated before it, rather than being
/// appended under it (joining `"/etc"` onto `"config.d"` should not
/// silently produce `"config.d/etc"`).
fn path_join(args: &[Value]) -> R<Value> {
    if args.is_empty() {
        return e("path_join: needs at least one argument".to_string());
    }
    let mut acc = String::new();
    for (i, a) in args.iter().enumerate() {
        let piece = normalize_seps(&crate::display_value(a));
        if piece.is_empty() {
            continue;
        }
        if is_absolute(&piece) || i == 0 {
            acc = piece;
        } else if acc.is_empty() || acc.ends_with('/') {
            acc.push_str(&piece);
        } else {
            acc.push('/');
            acc.push_str(&piece);
        }
    }
    Ok(Value::Str(acc))
}

/// Resolves `.`/`..` segments and normalizes separators, purely as string
/// algebra (no filesystem access, so this works for a path that doesn't
/// exist yet). A leading `..` that would walk above an absolute path's root
/// is dropped rather than erroring, matching what `std::fs::canonicalize`'s
/// callers usually want out of a *lexical* normalize (this is not that —
/// it never touches the disk, so symlinks are not resolved).
fn path_normalize(args: &[Value]) -> R<Value> {
    let p = normalize_seps(&text_arg(args, 0)?);
    let abs = is_absolute(&p);
    let (prefix, rest) = if abs && p.starts_with('/') {
        ("/", p.as_str().trim_start_matches('/'))
    } else if abs {
        // "C:/rest" -- keep "C:/" as the prefix, walk the rest.
        let split_at = p.find('/').map(|i| i + 1).unwrap_or(p.len());
        (&p[..split_at], &p[split_at..])
    } else {
        ("", p.as_str())
    };
    let mut stack: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if stack.last().map(|s| *s != "..").unwrap_or(false) {
                    stack.pop();
                } else if !abs {
                    stack.push("..");
                }
                // an absolute path's ".." above its root is simply dropped
            }
            s => stack.push(s),
        }
    }
    let joined = stack.join("/");
    let out = if prefix.is_empty() {
        if joined.is_empty() { ".".to_string() } else { joined }
    } else if prefix == "/" {
        format!("/{joined}")
    } else {
        format!("{prefix}{joined}")
    };
    Ok(Value::Str(out))
}

/// `path_absolute(path)` — `path` unchanged (just normalized) if already
/// absolute, otherwise joined onto the current working directory. The one
/// function in this module that touches the OS, for that reason alone.
fn path_absolute(args: &[Value]) -> R<Value> {
    let p = text_arg(args, 0)?;
    if is_absolute(&normalize_seps(&p)) {
        return path_normalize(args);
    }
    let cwd = std::env::current_dir().map_err(|err| EvalError {
        msg: format!("path_absolute: could not determine the current directory: {err}"),
    })?;
    let joined = path_join(&[Value::Str(cwd.display().to_string()), Value::Str(p)])?;
    path_normalize(&[joined])
}

/// `path_relative_to(path, base)` — the relative path FROM `base` TO
/// `path`, built lexically from each side's normalized-absolute segments
/// (both are resolved with `path_absolute` first, so this works whether or
/// not the caller already passed absolute paths). `..` segments climb out
/// of `base` for however much of it `path` doesn't share.
fn path_relative_to(args: &[Value]) -> R<Value> {
    let path = text_arg(args, 0)?;
    let base = text_arg(args, 1)?;
    let Value::Str(abs_path) = path_absolute(&[Value::Str(path)])? else { unreachable!() };
    let Value::Str(abs_base) = path_absolute(&[Value::Str(base)])? else { unreachable!() };

    let path_segs: Vec<&str> = abs_path.split('/').filter(|s| !s.is_empty()).collect();
    let base_segs: Vec<&str> = abs_base.split('/').filter(|s| !s.is_empty()).collect();

    let mut common = 0;
    while common < path_segs.len()
        && common < base_segs.len()
        && path_segs[common].eq_ignore_ascii_case(base_segs[common])
    {
        common += 1;
    }

    let mut out_segs: Vec<String> = Vec::new();
    for _ in common..base_segs.len() {
        out_segs.push("..".to_string());
    }
    for seg in &path_segs[common..] {
        out_segs.push((*seg).to_string());
    }

    if out_segs.is_empty() {
        Ok(Value::Str(".".to_string()))
    } else {
        Ok(Value::Str(out_segs.join("/")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: R<Value>) -> String {
        match v.unwrap() {
            Value::Str(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    #[test]
    fn name_stem_extension_split_a_plain_file() {
        let a = [Value::Str("a/b/c.txt".to_string())];
        assert_eq!(s(path_name(&a)), "c.txt");
        assert_eq!(s(path_stem(&a)), "c");
        assert_eq!(s(path_extension(&a)), "txt");
    }

    #[test]
    fn a_dotfile_is_not_treated_as_all_extension() {
        let a = [Value::Str(".gitignore".to_string())];
        assert_eq!(s(path_stem(&a)), ".gitignore");
        assert_eq!(s(path_extension(&a)), "");
    }

    #[test]
    fn multi_dot_extension_takes_only_the_last_segment() {
        let a = [Value::Str("archive.tar.gz".to_string())];
        assert_eq!(s(path_stem(&a)), "archive.tar");
        assert_eq!(s(path_extension(&a)), "gz");
    }

    #[test]
    fn backslashes_normalize_to_forward_slashes_everywhere() {
        let a = [Value::Str("a\\b\\c.txt".to_string())];
        assert_eq!(s(path_name(&a)), "c.txt");
        assert_eq!(s(path_parent(&a)), "a/b");
    }

    #[test]
    fn parent_of_a_root_level_file_is_the_root() {
        let a = [Value::Str("/etc".to_string())];
        assert_eq!(s(path_parent(&a)), "/");
    }

    #[test]
    fn parent_of_a_bare_name_is_empty() {
        let a = [Value::Str("readme.md".to_string())];
        assert_eq!(s(path_parent(&a)), "");
    }

    #[test]
    fn join_glues_pieces_with_one_slash_each() {
        let a = [Value::Str("a".to_string()), Value::Str("b".to_string()), Value::Str("c.txt".to_string())];
        assert_eq!(s(path_join(&a)), "a/b/c.txt");
    }

    #[test]
    fn join_resets_on_a_later_absolute_piece() {
        let a = [Value::Str("config.d".to_string()), Value::Str("/etc".to_string())];
        assert_eq!(s(path_join(&a)), "/etc");
    }

    #[test]
    fn normalize_resolves_dot_and_dotdot() {
        let a = [Value::Str("a/./b/../c".to_string())];
        assert_eq!(s(path_normalize(&a)), "a/c");
    }

    #[test]
    fn normalize_keeps_a_leading_dotdot_on_a_relative_path() {
        let a = [Value::Str("../a/b".to_string())];
        assert_eq!(s(path_normalize(&a)), "../a/b");
    }

    #[test]
    fn normalize_drops_a_dotdot_above_an_absolute_root() {
        let a = [Value::Str("/../a".to_string())];
        assert_eq!(s(path_normalize(&a)), "/a");
    }

    #[test]
    fn relative_to_climbs_out_and_back_down() {
        let a = [Value::Str("/a/b/x.txt".to_string()), Value::Str("/a/c".to_string())];
        assert_eq!(s(path_relative_to(&a)), "../b/x.txt");
    }

    #[test]
    fn relative_to_a_direct_ancestor_has_no_dotdot() {
        let a = [Value::Str("/a/b/c.txt".to_string()), Value::Str("/a".to_string())];
        assert_eq!(s(path_relative_to(&a)), "b/c.txt");
    }

    #[test]
    fn relative_to_itself_is_a_single_dot() {
        let a = [Value::Str("/a/b".to_string()), Value::Str("/a/b".to_string())];
        assert_eq!(s(path_relative_to(&a)), ".");
    }
}
