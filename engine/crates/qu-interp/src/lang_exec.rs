//! `matlab_exec(code, [vars=])` / `js_exec(code, [vars=])` -- running other
//! languages inline from a Qu script, with real data crossing the boundary
//! in both directions.
//!
//! Qu already had exactly one of these (`python_exec`, see `py_exec.rs`), and
//! its shape turned out to be the general one: write the code to a temp
//! file, prepend a preamble that loads a JSON blob of caller-supplied
//! variables into the guest language's own namespace, hand the guest a path
//! to write its result to, run it as a subprocess, then read that file back.
//! Nothing about that is Python-specific -- only three things are:
//!
//!   1. the file extension,
//!   2. the preamble that loads vars and defines the output path,
//!   3. how the interpreter is invoked.
//!
//! So this module is that shape with those three lifted into a table
//! (`LANGS`). Adding a language is an entry, not an implementation.
//!
//! ## Why these languages
//! Chosen by what a Qu user can actually run rather than by what would look
//! impressive in a list: MATLAB, because this project benchmarks against it
//! constantly and its toolboxes are the reason people keep a MATLAB licence
//! at all; and JavaScript, because Node is near-universal and starts in
//! milliseconds, which makes it the practical choice for small glue.
//!
//! Deliberately NOT here (yet), and each for a real reason rather than
//! oversight:
//!   - Julia / Octave / R: same interpreter+JSON shape, so each is one
//!     `LANGS` entry away, but none of the three is on this machine's PATH
//!     to verify against. Adding an entry that has never once been run is
//!     how you ship something that does not work.
//!   - Rust / VB.NET / assembly: these COMPILE. That is a different
//!     lifecycle (toolchain discovery, a build step that can fail with its
//!     own diagnostics, artifact cleanup) and does not belong bolted onto a
//!     module whose entire premise is "hand a script to an interpreter".
//!
//! ## Contract
//! Identical to `python_exec`'s, deliberately -- one convention to learn:
//!   - `vars=<record>` fields arrive as ordinary variables in the guest.
//!   - a path to write results to is always predefined; write JSON there and
//!     it comes back as the returned record's `.result`. Don't, and
//!     `.result` is `none` (the common case -- most scripts just print).
//!   - the call returns `{stdout, stderr, success, exit_code, result}`.
//!
//! ## Verification status, honestly
//! `js_exec` is verified end to end: Qu variables arrive as JS globals, the
//! script writes JSON to the injected path, and it comes back as real Qu
//! values.
//!
//! `matlab_exec` is verified end to end too, but NOT against MathWorks
//! MATLAB, whose licence on this machine has expired -- confirmed by MATLAB
//! itself answering "Licensing Error 10". What it is verified against is
//! Matty, the MATLAB/Octave interpreter in the sibling repo: the generated
//! script runs there unmodified, `vars=` arrive as workspace variables of
//! the right types, the injected `qu_output_path` is writable, and the
//! results file comes back and decodes into Qu values.
//!
//! That verification is what drove the design. The original preamble used
//! `jsondecode`/`fileread`/`fieldnames`, none of which Matty has -- and
//! `jsonencode`/`jsondecode` only reached MathWorks MATLAB in R2016b and
//! need a package in GNU Octave. So `vars=` for MATLAB is now emitted as
//! plain assignments (`v = [2 4 6 8];`), which every MATLAB-family
//! interpreter has understood forever. The generated script depends on
//! nothing but assignment; what the user's own code calls is their choice.
//!
//! The remaining gap is small and worth naming: whether MathWorks MATLAB
//! behaves identically to Matty on this path. The generated code uses only
//! assignment, so the surface for disagreement is about as small as it gets.
//!
//! The one unavoidable difference is the name of that output-path variable.
//! Python and JavaScript take `_qu_output_path`; MATLAB **cannot** -- an
//! identifier there may not begin with an underscore -- so MATLAB uses
//! `qu_output_path`. Papering over that with a rename would be worse: the
//! guest code is written in the guest language, and it should obey the
//! guest language's rules.

use crate::py_exec::{plain_json_to_value, run_with, unique_temp_path, value_to_plain_json, RunResult};
use crate::{arg0, e, EvalError, Value, R};
use std::sync::Arc;

/// One inline language: everything that differs from `python_exec`.
struct Lang {
    /// Builtin name, used in every error message so a failure names the
    /// call the user actually wrote.
    builtin: &'static str,
    /// Temp-file extension. MATLAB in particular insists on `.m`.
    ext: &'static str,
    /// Executables to try, in order.
    candidates: &'static [&'static str],
    /// What to say when none of them is on PATH -- specific enough to act
    /// on, rather than a generic "not found".
    missing: &'static str,
    /// Name the guest sees for the results path. See the module note on why
    /// MATLAB's differs.
    output_var: &'static str,
    /// Builds the preamble: defines the output path, and (when `vars_path`
    /// is given) loads that JSON into the guest's own namespace.
    preamble: fn(output_path: &str, vars_path: Option<&str>) -> String,
    /// How to invoke the interpreter on a script file.
    argv: fn(script: &str) -> Vec<String>,
    /// Write `vars=` as guest SOURCE, inline, instead of as a JSON file the
    /// guest has to parse. `None` keeps the JSON-file route.
    ///
    /// MATLAB needs this. Its JSON functions (`jsonencode`/`jsondecode`)
    /// only arrived in R2016b, GNU Octave needs a package for them, and
    /// Matty -- the MATLAB interpreter this project benchmarks against --
    /// has neither, nor `fileread` nor `fieldnames`. Depending on them
    /// meant `vars=` silently required a recent MATLAB. Plain assignments
    /// depend on nothing, are faster, and are readable if anyone dumps the
    /// temp script to see what ran.
    inline_vars: Option<fn(&[(String, Value)]) -> R<String>>,
}

/// Render a Qu value as a MATLAB literal.
///
/// Deliberately narrow: numbers, booleans, strings, vectors and matrices
/// are what crosses this boundary in practice. Anything else errors by
/// NAME rather than being silently coerced into something MATLAB will
/// misread -- a wrong number is worse than a refusal.
fn matlab_literal(v: &Value) -> R<String> {
    Ok(match v {
        Value::Num(n) if n.is_nan() => "NaN".to_string(),
        Value::Num(n) if n.is_infinite() => {
            if *n > 0.0 { "Inf".to_string() } else { "-Inf".to_string() }
        }
        Value::Num(n) => format!("{n}"),
        Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        // MATLAB single quotes are escaped by doubling them.
        Value::Str(s) => format!("'{}'", s.replace('\'', "''")),
        Value::Vec(xs) => {
            let inner: Vec<String> = xs.iter().map(|x| matlab_literal(&Value::Num(*x))).collect::<R<_>>()?;
            format!("[{}]", inner.join(" "))
        }
        Value::Mat(m) => {
            // MATLAB separates rows with `;`.
            let mut rows = Vec::with_capacity(m.rows());
            for r in 0..m.rows() {
                let mut cells = Vec::with_capacity(m.cols());
                for c in 0..m.cols() {
                    let v = m.get(r, c).map_err(|err| EvalError {
                        msg: format!("matlab_exec: vars= matrix read failed: {err}"),
                    })?;
                    cells.push(format!("{v}"));
                }
                rows.push(cells.join(" "));
            }
            format!("[{}]", rows.join("; "))
        }
        other => {
            return e(format!(
                "matlab_exec: vars= cannot carry a {} -- pass numbers, booleans, strings, vectors or matrices",
                other.type_name()
            ))
        }
    })
}

fn matlab_vars(vars: &[(String, Value)]) -> R<String> {
    let mut out = String::new();
    for (name, value) in vars {
        out.push_str(&format!("{name} = {};\n", matlab_literal(value)?));
    }
    Ok(out)
}

fn matlab_preamble(output_path: &str, _vars_path: Option<&str>) -> String {
    // Vars arrive as plain assignments through `inline_vars`, so nothing
    // here needs `jsondecode` / `fileread` / `fieldnames`. See that field's
    // doc comment for why depending on them was a mistake.
    format!("qu_output_path = '{}';\n", output_path.replace('\'', "''"))
}

fn js_preamble(output_path: &str, vars_path: Option<&str>) -> String {
    let mut p = String::from("const _qu_fs = require('fs');\n");
    p.push_str(&format!(
        "const _qu_output_path = {};\n",
        serde_json::to_string(output_path).unwrap_or_else(|_| "\"\"".into())
    ));
    if let Some(vp) = vars_path {
        p.push_str(&format!(
            "Object.assign(globalThis, JSON.parse(_qu_fs.readFileSync({}, 'utf8')));\n",
            serde_json::to_string(vp).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    p
}

const LANGS: &[Lang] = &[
    Lang {
        builtin: "matlab_exec",
        ext: "m",
        // `-batch` is the non-interactive entry point: it runs the code,
        // sends everything to stdout/stderr, and exits with a real status.
        // The older `-r` needs an explicit `exit` and will happily hang a
        // build server forever if the script errors first.
        candidates: &["matlab"],
        missing: "matlab_exec: `matlab` was not found on PATH -- install MATLAB, or add its \
                  `bin` directory to PATH, to run MATLAB from Qu",
        output_var: "qu_output_path",
        preamble: matlab_preamble,
        argv: |script| vec!["-batch".to_string(), format!("run('{}')", script.replace('\'', "''"))],
        inline_vars: Some(matlab_vars),
    },
    Lang {
        builtin: "js_exec",
        ext: "js",
        candidates: &["node", "nodejs"],
        missing: "js_exec: neither `node` nor `nodejs` was found on PATH -- install Node.js to \
                  run JavaScript from Qu",
        output_var: "_qu_output_path",
        preamble: js_preamble,
        argv: |script| vec![script.to_string()],
        // Node parses JSON natively, and faster than it would parse
        // generated source; the file route is right there.
        inline_vars: None,
    },
];

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let lang = LANGS
        .iter()
        .find(|l| l.builtin == f)
        .ok_or_else(|| EvalError { msg: format!("lang_exec: internal dispatch error, unhandled `{f}`") })?;
    exec(lang, args, style)
}

fn exec(lang: &Lang, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let code = match arg0(args)? {
        Value::Str(s) => s.clone(),
        other => {
            return e(format!(
                "{}: expected a string of source code, found {}",
                lang.builtin,
                other.type_name()
            ))
        }
    };

    let script_path = unique_temp_path("script", lang.ext);
    let output_path = unique_temp_path("out", "json");

    // `vars=` is serialized to the same plain, untagged JSON `python_exec`
    // uses -- the shape an ordinary script in the guest language expects,
    // not Qu's internal tagged round-trip format.
    let vars: Option<Vec<(String, Value)>> = match crate::style_entry(style, "vars").map(|(_, v)| v) {
        Some(Value::Record(fields)) => Some(fields.as_ref().clone()),
        Some(other) => {
            return e(format!(
                "{}: vars= must be a record of name->value pairs, found {}",
                lang.builtin,
                other.type_name()
            ))
        }
        None => None,
    };

    // Two routes into the guest: source the guest already speaks, or a
    // JSON file it parses. Which one is a property of the language.
    let mut inline_prelude = String::new();
    let mut vars_path: Option<std::path::PathBuf> = None;
    if let Some(vars) = &vars {
        match lang.inline_vars {
            Some(write) => inline_prelude = write(vars)?,
            None => {
                let mut obj = serde_json::Map::new();
                for (k, val) in vars.iter() {
                    obj.insert(k.clone(), value_to_plain_json(val)?);
                }
                let vp = unique_temp_path("vars", "json");
                let text = serde_json::to_string(&serde_json::Value::Object(obj))
                    .map_err(|err| EvalError { msg: format!("{}: could not serialize vars=: {err}", lang.builtin) })?;
                std::fs::write(&vp, text).map_err(|err| EvalError {
                    msg: format!("{}: could not write vars temp file `{}`: {err}", lang.builtin, vp.display()),
                })?;
                vars_path = Some(vp);
            }
        }
    }

    let preamble = (lang.preamble)(
        &output_path.display().to_string(),
        vars_path.as_ref().map(|p| p.display().to_string()).as_deref(),
    );
    let full = format!("{preamble}{inline_prelude}\n{code}\n");
    std::fs::write(&script_path, &full).map_err(|err| EvalError {
        msg: format!("{}: could not write temp script `{}`: {err}", lang.builtin, script_path.display()),
    })?;

    let argv = (lang.argv)(&script_path.display().to_string());
    let run_result: R<RunResult> = run_with(lang.candidates, &argv, lang.builtin, lang.missing);

    // Best-effort cleanup either way: a leftover temp file on a failed run
    // isn't worth erroring over, but isn't worth keeping either.
    // `QU_KEEP_TEMP=1` leaves the generated script on disk. The only way
    // to check what a guest language was actually handed is to read it,
    // and reconstructing it by hand is exactly how a preamble bug survives.
    if std::env::var_os("QU_KEEP_TEMP").is_none() {
        let _ = std::fs::remove_file(&script_path);
    } else {
        eprintln!("qu: kept {}", script_path.display());
    }
    if let Some(vp) = &vars_path {
        let _ = std::fs::remove_file(vp);
    }
    let run = run_result?;

    let result_val = if output_path.exists() {
        let text = std::fs::read_to_string(&output_path).unwrap_or_default();
        let _ = std::fs::remove_file(&output_path);
        if text.trim().is_empty() {
            Value::Nothing
        } else {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(j) => plain_json_to_value(&j),
                Err(err) => {
                    return e(format!(
                        "{}: script wrote an output file that isn't valid JSON: {err}",
                        lang.builtin
                    ))
                }
            }
        }
    } else {
        Value::Nothing
    };

    Ok(Value::Record(Arc::new(vec![
        ("stdout".to_string(), Value::Str(run.stdout)),
        ("stderr".to_string(), Value::Str(run.stderr)),
        ("success".to_string(), Value::Bool(run.success)),
        ("exit_code".to_string(), Value::Num(run.exit_code as f64)),
        ("result".to_string(), result_val),
    ])))
}

/// The variable name a given inline language exposes its output path under.
/// Public so error messages and docs elsewhere can name it without
/// duplicating the table.
pub fn output_var_for(builtin: &str) -> Option<&'static str> {
    LANGS.iter().find(|l| l.builtin == builtin).map(|l| l.output_var)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
    }

    // The generated MATLAB must depend on nothing but assignment. The
    // original preamble used `jsondecode`/`fileread`/`fieldnames`, which
    // MATLAB only gained in R2016b, Octave needs a package for, and Matty
    // does not have at all -- so `vars=` silently required a recent MATLAB.
    #[test]
    fn matlab_vars_are_plain_assignments() {
        let out = matlab_vars(&vars(&[
            ("v", Value::Vec(Arc::new(vec![2.0, 4.0, 6.0, 8.0]))),
            ("n", Value::Num(4.0)),
            ("tag", Value::Str("run-A".into())),
            ("ok", Value::Bool(true)),
        ]))
        .unwrap();
        assert_eq!(out, "v = [2 4 6 8];\nn = 4;\ntag = 'run-A';\nok = true;\n");
        for forbidden in ["jsondecode", "fileread", "fieldnames", "eval"] {
            assert!(!out.contains(forbidden), "generated code uses {forbidden}");
        }
    }

    #[test]
    fn matlab_strings_escape_their_quotes() {
        // MATLAB doubles a single quote inside a single-quoted string;
        // getting this wrong ends the literal early and the rest of the
        // value becomes code.
        let out = matlab_vars(&vars(&[("s", Value::Str("it's".into()))])).unwrap();
        assert_eq!(out, "s = 'it''s';\n");
    }

    #[test]
    fn matlab_non_finite_numbers_use_matlab_spelling() {
        // `inf`/`NaN` in Rust's Display are not MATLAB literals.
        let out = matlab_vars(&vars(&[
            ("a", Value::Num(f64::INFINITY)),
            ("b", Value::Num(f64::NEG_INFINITY)),
            ("c", Value::Num(f64::NAN)),
        ]))
        .unwrap();
        assert_eq!(out, "a = Inf;\nb = -Inf;\nc = NaN;\n");
    }

    #[test]
    fn an_unsupported_var_type_is_refused_by_name() {
        // Better a refusal than a value MATLAB silently misreads.
        let err = matlab_vars(&vars(&[("r", Value::Record(Arc::new(vec![])))]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("matlab_exec"), "error should name the call: {err}");
        assert!(err.contains("vars="), "error should name the argument: {err}");
    }

    #[test]
    fn javascript_still_takes_the_json_file_route() {
        // Node parses JSON natively and faster than generated source; only
        // MATLAB needs the assignment route.
        let js = LANGS.iter().find(|l| l.builtin == "js_exec").unwrap();
        assert!(js.inline_vars.is_none());
        let ml = LANGS.iter().find(|l| l.builtin == "matlab_exec").unwrap();
        assert!(ml.inline_vars.is_some());
    }

    #[test]
    fn the_matlab_preamble_only_defines_the_output_path() {
        let p = matlab_preamble("C:/tmp/out.json", None);
        assert!(p.starts_with("qu_output_path = '"));
        assert_eq!(p.lines().count(), 1, "preamble should be one line: {p:?}");
    }
}
