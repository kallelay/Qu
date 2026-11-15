//! Matrix-function builtins (added in v0.4.14): `schur`, `sqrtm`, `logm`,
//! `sylvester` and `lyap`. The numerics are in `qu_core::matfun`; this file
//! turns `Value`s into matrices and back and words the errors.
//!
//! Dense, real, square input only. A complex matrix is an error.

use super::{arg_get, e, mat_value, EvalError, Value, R};
use qu_core::cmatrix::CMatrix;
use qu_core::matfun::{self, MatFun};
use qu_core::matrix::Matrix;
use std::sync::Arc;

pub const NAMES: &[&str] = &["logm", "lyap", "schur", "sqrtm", "sylvester"];

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn real_matrix(f: &str, what: &str, v: &Value) -> R<Matrix> {
    if matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_)) {
        return e(format!("{f}: {what} must be real (complex matrices are not supported)"));
    }
    v.to_matrix().map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") })
}

fn msg<T>(r: Result<T, String>) -> R<T> {
    r.map_err(|m| EvalError { msg: m })
}

fn fun_value(r: MatFun) -> Value {
    match r {
        MatFun::Real(m) => mat_value(m),
        MatFun::Complex(n, d) => Value::CMat(Arc::new(CMatrix::from_col_major(n, n, d))),
    }
}

pub(crate) fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "schur" => {
            let a = real_matrix(f, "A", need(f, "schur(A)", args, 0)?)?;
            let s = msg(matfun::schur(&a))?;
            Ok(Value::Record(Arc::new(vec![
                ("T".to_string(), mat_value(s.t)),
                ("U".to_string(), mat_value(s.u)),
            ])))
        }
        "sqrtm" => {
            let a = real_matrix(f, "A", need(f, "sqrtm(A)", args, 0)?)?;
            Ok(fun_value(msg(matfun::sqrtm(&a))?))
        }
        "logm" => {
            let a = real_matrix(f, "A", need(f, "logm(A)", args, 0)?)?;
            Ok(fun_value(msg(matfun::logm(&a))?))
        }
        "sylvester" => {
            let sig = "sylvester(A, B, C)";
            let a = real_matrix(f, "A", need(f, sig, args, 0)?)?;
            let b = real_matrix(f, "B", need(f, sig, args, 1)?)?;
            let c = real_matrix(f, "C", need(f, sig, args, 2)?)?;
            Ok(mat_value(msg(matfun::sylvester(&a, &b, &c))?))
        }
        "lyap" => {
            let a = real_matrix(f, "A", need(f, "lyap(A, Q)", args, 0)?)?;
            let q = real_matrix(f, "Q", need(f, "lyap(A, Q)", args, 1)?)?;
            Ok(mat_value(msg(matfun::lyap(&a, &q))?))
        }
        other => e(format!("matfun_ops: `{other}` is not handled here")),
    }
}
