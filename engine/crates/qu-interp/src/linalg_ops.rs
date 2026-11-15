//! Matrix-property builtins (added in v0.4.14): `eps`, `hilb`, `invhilb`,
//! `issymmetric`, `isposdef`, `isdiag`, `istril`, `istriu`, `isbanded`,
//! `bandwidth`, `rcond`, `svdvals`, plus the two-argument forms of `norm`
//! and `cond` (`norm_p`, `cond_p`, called from their arms in `lib.rs`).
//! The numerics are in `qu_core::matprops`; this file turns `Value`s into
//! matrices and back and words the errors.
//!
//! Real input only. None of these take keyword arguments, so any
//! `name=value` is rejected by the interpreter's unread-keyword check, and
//! a surplus positional argument by its arity check.

use super::{arg_get, e, mat_value, to_cow, EvalError, Value, R};
use qu_core::linalg;
use qu_core::matprops::{self as mp, NormP};
use qu_core::matrix::Matrix;
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "bandwidth", "eps", "hilb", "invhilb", "isbanded", "isdiag", "isposdef", "issymmetric",
    "istril", "istriu", "rcond", "svdvals",
];

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn msg<T>(r: Result<T, String>) -> R<T> {
    r.map_err(|m| EvalError { msg: m })
}

fn reject_complex(f: &str, what: &str, v: &Value) -> R<()> {
    if matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_)) {
        return e(format!("{f}: {what} must be real (complex input is not supported)"));
    }
    Ok(())
}

fn real_matrix(f: &str, what: &str, v: &Value) -> R<Matrix> {
    reject_complex(f, what, v)?;
    v.to_matrix().map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") })
}

/// A whole number `>= min` as `usize`.
fn count(f: &str, what: &str, v: &Value, min: usize) -> R<usize> {
    match v {
        Value::Num(x) if x.is_finite() && *x == x.trunc() && *x >= min as f64 && *x < 1e9 => {
            Ok(*x as usize)
        }
        Value::Num(x) => e(format!("{f}: {what} must be a whole number >= {min}, got {x}")),
        other => e(format!("{f}: {what} must be a whole number, found {}", other.type_name())),
    }
}

fn parse_p(f: &str, v: &Value) -> R<NormP> {
    match v {
        Value::Num(p) => Ok(NormP::P(*p)),
        Value::Str(s) if s == "fro" => Ok(NormP::Fro),
        Value::Str(s) => e(format!("{f}: unknown norm \"{s}\" -- p is a number or \"fro\"")),
        other => e(format!("{f}: p must be a number or \"fro\", found {}", other.type_name())),
    }
}

fn eps_map(f: &str, v: &Value) -> R<Value> {
    reject_complex(f, "x", v)?;
    match v {
        Value::Num(x) => Ok(Value::Num(mp::eps_at(*x))),
        Value::Bool(b) => Ok(Value::Num(mp::eps_at(*b as u8 as f64))),
        Value::Vec(xs) => Ok(Value::Vec(Arc::new(xs.iter().map(|&x| mp::eps_at(x)).collect()))),
        Value::Mat(m) => Ok(Value::Mat(Arc::new(m.map(mp::eps_at)))),
        other => e(format!("eps: x must be a number, Vec or Mat, found {}", other.type_name())),
    }
}

pub(crate) fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "eps" => match arg_get(args, 0) {
            None => Ok(Value::Num(f64::EPSILON)),
            Some(v) => eps_map(f, v),
        },
        "hilb" => {
            let n = count(f, "n", need(f, "hilb(n)", args, 0)?, 1)?;
            Ok(mat_value(msg(mp::hilb(n))?))
        }
        "invhilb" => {
            let n = count(f, "n", need(f, "invhilb(n)", args, 0)?, 1)?;
            Ok(mat_value(msg(mp::invhilb(n))?))
        }
        "issymmetric" => {
            let a = real_matrix(f, "A", need(f, "issymmetric(A, [tol])", args, 0)?)?;
            let tol = match arg_get(args, 1) {
                None => 0.0,
                Some(Value::Num(t)) if t.is_finite() && *t >= 0.0 => *t,
                Some(other) => {
                    return e(format!(
                        "issymmetric: tol must be a non-negative number, found {}",
                        match other {
                            Value::Num(t) => t.to_string(),
                            o => o.type_name().to_string(),
                        }
                    ))
                }
            };
            Ok(Value::Bool(mp::is_symmetric(&a, tol)))
        }
        "isposdef" => {
            let a = real_matrix(f, "A", need(f, "isposdef(A)", args, 0)?)?;
            Ok(Value::Bool(msg(mp::is_posdef(&a))?))
        }
        "isdiag" | "istril" | "istriu" => {
            let a = real_matrix(f, "A", need(f, &format!("{f}(A)"), args, 0)?)?;
            Ok(Value::Bool(match f {
                "isdiag" => mp::is_diag(&a),
                "istril" => mp::is_tril(&a),
                _ => mp::is_triu(&a),
            }))
        }
        "isbanded" => {
            let sig = "isbanded(A, lower, upper)";
            let a = real_matrix(f, "A", need(f, sig, args, 0)?)?;
            let lo = count(f, "lower", need(f, sig, args, 1)?, 0)?;
            let up = count(f, "upper", need(f, sig, args, 2)?, 0)?;
            Ok(Value::Bool(mp::is_banded(&a, lo, up)))
        }
        "bandwidth" => {
            let a = real_matrix(f, "A", need(f, "bandwidth(A)", args, 0)?)?;
            let (lo, up) = mp::bandwidth(&a);
            Ok(Value::Record(Arc::new(vec![
                ("lower".to_string(), Value::Num(lo as f64)),
                ("upper".to_string(), Value::Num(up as f64)),
            ])))
        }
        "rcond" => {
            let a = real_matrix(f, "A", need(f, "rcond(A)", args, 0)?)?;
            Ok(Value::Num(msg(mp::rcond(&a))?))
        }
        "svdvals" => {
            let a = real_matrix(f, "A", need(f, "svdvals(A)", args, 0)?)?;
            let s = linalg::singular_values(&a)
                .map_err(|er| EvalError { msg: format!("svdvals: {er}") })?;
            Ok(Value::Vec(Arc::new(s)))
        }
        other => e(format!("linalg_ops: `{other}` is not handled here")),
    }
}

/// `norm(x, p)`: a vector (`Vec`, a row or column `Mat`, a number) takes
/// `p` = 1, 2, inf, -inf, 0, any `p >= 1` or `"fro"`; a matrix takes
/// 1, 2, inf or `"fro"`.
pub(crate) fn norm_p(args: &[Value]) -> R<Value> {
    let f = "norm";
    let x = need(f, "norm(x, p)", args, 0)?;
    let p = parse_p(f, need(f, "norm(x, p)", args, 1)?)?;
    reject_complex(f, "x", x)?;
    if let Value::Mat(m) = x {
        if m.rows() > 1 && m.cols() > 1 {
            return Ok(Value::Num(msg(mp::matrix_norm(m, p))?));
        }
    }
    let xs = to_cow(x).map_err(|er| EvalError { msg: format!("norm: {}", er.msg) })?;
    Ok(Value::Num(msg(mp::vector_norm(&xs, p))?))
}

/// `cond(A, p)` for `p` = 1, 2, inf or `"fro"`.
pub(crate) fn cond_p(args: &[Value]) -> R<Value> {
    let f = "cond";
    let a = real_matrix(f, "A", need(f, "cond(A, p)", args, 0)?)?;
    let p = parse_p(f, need(f, "cond(A, p)", args, 1)?)?;
    Ok(Value::Num(msg(mp::cond(&a, p))?))
}
