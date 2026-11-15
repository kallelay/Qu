//! Computational-numerics builtins (added in v0.4.14): polynomials (`roots`,
//! `poly`, `deconv`, `polyder`, `polyint`), integer number theory (`gcd`,
//! `lcm`, `primes`, `isprime`, `factor`, `nchoosek`, `factorial`), structured
//! matrices and subspaces (`toeplitz`, `hankel`, `vander`, `null`, `orth`) and
//! `gradient`.
//!
//! The numerics live in `qu_core::{poly_ops, number_theory, matrix_builders}`
//! with their own unit tests; this file only turns `Value`s into slices and
//! back and words the errors. None of these take keyword arguments, so any
//! `name=value` is rejected by the interpreter's unread-keyword check.

use super::{arg_get, e, mat_value, to_cow, EvalError, Value, R};
use qu_core::matrix_builders::{self, Spacing};
use qu_core::number_theory as nt;
use qu_core::poly_ops;
use qu_core::Complex64;
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "deconv", "factor", "factorial", "gcd", "gradient", "hankel", "isprime", "lcm", "nchoosek",
    "null", "orth", "poly", "polyder", "polyint", "primes", "roots", "toeplitz", "vander",
];

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn msg<T>(f: &str, r: Result<T, String>) -> R<T> {
    // core errors that already start with the function name are kept as is
    r.map_err(|m| EvalError {
        msg: if m.starts_with(&format!("{f}:")) { m } else { format!("{f}: {m}") },
    })
}

fn real_vec(f: &str, what: &str, v: &Value) -> R<Vec<f64>> {
    if matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_)) {
        return e(format!("{f}: {what} must be real (complex coefficients are not supported)"));
    }
    if matches!(v, Value::Mat(m) if m.rows() > 1 && m.cols() > 1) {
        return e(format!("{f}: {what} must be a vector, not a matrix"));
    }
    match to_cow(v) {
        Ok(c) => Ok(c.into_owned()),
        Err(_) => e(format!("{f}: {what} must be a numeric vector, found {}", v.type_name())),
    }
}

fn vec_val(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

/// Integer scalar or integer vector argument: `(values, was_scalar)`.
fn ints(f: &str, name: &str, v: &Value) -> R<(Vec<i64>, bool)> {
    match v {
        Value::Num(x) => Ok((vec![msg(f, nt::to_int(f, name, *x))?], true)),
        Value::Bool(b) => Ok((vec![*b as i64], true)),
        Value::Vec(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for &x in xs.iter() {
                out.push(msg(f, nt::to_int(f, name, x))?);
            }
            Ok((out, false))
        }
        other => e(format!(
            "{f}: {name} must be an integer (or a vector of integers), found {}",
            other.type_name()
        )),
    }
}

fn scalar_int(f: &str, name: &str, v: &Value) -> R<i64> {
    match v {
        Value::Num(_) | Value::Bool(_) => Ok(ints(f, name, v)?.0[0]),
        other => e(format!("{f}: {name} must be a single integer, found {}", other.type_name())),
    }
}

fn nonneg(f: &str, name: &str, x: i64) -> R<u64> {
    if x < 0 {
        return e(format!("{f}: {name} must be a non-negative integer, got {x}"));
    }
    Ok(x as u64)
}

/// `a` and `b` paired elementwise: scalar-scalar, scalar-vector (either way)
/// or two vectors of equal length.
fn pair_up(
    f: &str,
    a: (Vec<i64>, bool),
    b: (Vec<i64>, bool),
) -> R<(Vec<(i64, i64)>, bool)> {
    let (av, asc) = a;
    let (bv, bsc) = b;
    if asc && bsc {
        return Ok((vec![(av[0], bv[0])], true));
    }
    if asc {
        return Ok((bv.iter().map(|&y| (av[0], y)).collect(), false));
    }
    if bsc {
        return Ok((av.iter().map(|&x| (x, bv[0])).collect(), false));
    }
    if av.len() != bv.len() {
        return e(format!(
            "{f}: the two vectors must have the same length, got {} and {}",
            av.len(),
            bv.len()
        ));
    }
    Ok((av.into_iter().zip(bv).collect(), false))
}

fn num_or_vec(vals: Vec<f64>, scalar: bool) -> Value {
    if scalar {
        Value::Num(vals[0])
    } else {
        vec_val(vals)
    }
}

pub(crate) fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "roots" => {
            let p = real_vec(f, "the coefficient vector", need(f, "roots(p)", args, 0)?)?;
            let r = msg(f, poly_ops::roots(&p))?;
            if r.iter().all(|z| z.im == 0.0) {
                Ok(vec_val(r.iter().map(|z| z.re).collect()))
            } else {
                Ok(Value::CVec(Arc::new(r)))
            }
        }
        "poly" => {
            let v = need(f, "poly(r)", args, 0)?;
            if matches!(v, Value::Mat(m) if m.rows() > 1 && m.cols() > 1)
                || matches!(v, Value::CMat(m) if m.rows() > 1 && m.cols() > 1)
            {
                return e("poly: the characteristic polynomial of a matrix is not supported; pass a vector of roots");
            }
            let r: Vec<Complex64> = v.as_complex_flat().map_err(|m| EvalError {
                msg: format!("poly: the roots must be a numeric vector ({m})"),
            })?;
            if r.iter().any(|z| !z.re.is_finite() || !z.im.is_finite()) {
                return e("poly: the roots must be finite");
            }
            let c = poly_ops::poly_complex(&r);
            if r.iter().all(|z| z.im == 0.0) || poly_ops::is_conjugate_closed(&r) {
                Ok(vec_val(c.iter().map(|z| z.re).collect()))
            } else {
                Ok(Value::CVec(Arc::new(c)))
            }
        }
        "deconv" => {
            let b = real_vec(f, "the dividend", need(f, "deconv(b, a)", args, 0)?)?;
            let a = real_vec(f, "the divisor", need(f, "deconv(b, a)", args, 1)?)?;
            let (q, r) = msg(f, poly_ops::deconv(&b, &a))?;
            Ok(Value::Record(Arc::new(vec![
                ("q".to_string(), vec_val(q)),
                ("r".to_string(), vec_val(r)),
            ])))
        }
        "polyder" => {
            let p = real_vec(f, "the coefficient vector", need(f, "polyder(p)", args, 0)?)?;
            Ok(vec_val(msg(f, poly_ops::polyder(&p))?))
        }
        "polyint" => {
            let p = real_vec(f, "the coefficient vector", need(f, "polyint(p, [k])", args, 0)?)?;
            let k = match arg_get(args, 1) {
                Some(v) => v.as_num().map_err(|m| EvalError { msg: format!("polyint: k: {m}") })?,
                None => 0.0,
            };
            Ok(vec_val(msg(f, poly_ops::polyint(&p, k))?))
        }
        "gcd" | "lcm" => {
            let sig = if f == "gcd" { "gcd(a, b)" } else { "lcm(a, b)" };
            let a = ints(f, "a", need(f, sig, args, 0)?)?;
            let b = ints(f, "b", need(f, sig, args, 1)?)?;
            let (pairs, scalar) = pair_up(f, a, b)?;
            let mut out = Vec::with_capacity(pairs.len());
            for (x, y) in pairs {
                let r = if f == "gcd" { nt::gcd(x, y) } else { msg(f, nt::lcm(x, y))? };
                out.push(r as f64);
            }
            Ok(num_or_vec(out, scalar))
        }
        "primes" => {
            let n = scalar_int(f, "n", need(f, "primes(n)", args, 0)?)?;
            let n = if n < 0 { 0 } else { n as u64 };
            let p = msg(f, nt::primes_upto(n))?;
            Ok(vec_val(p.into_iter().map(|x| x as f64).collect()))
        }
        "isprime" => {
            let (xs, scalar) = ints(f, "n", need(f, "isprime(n)", args, 0)?)?;
            let flags: Vec<bool> = xs.iter().map(|&x| x >= 2 && nt::is_prime(x as u64)).collect();
            if scalar {
                Ok(Value::Bool(flags[0]))
            } else {
                Ok(Value::Mask(flags))
            }
        }
        "factor" => {
            let n = scalar_int(f, "n", need(f, "factor(n)", args, 0)?)?;
            if n < 1 {
                return e(format!("factor: n must be a positive integer, got {n}"));
            }
            let fs = msg(f, nt::factor(n as u64))?;
            Ok(vec_val(fs.into_iter().map(|x| x as f64).collect()))
        }
        "nchoosek" => {
            let n = scalar_int(f, "n", need(f, "nchoosek(n, k)", args, 0)?)?;
            let k = scalar_int(f, "k", need(f, "nchoosek(n, k)", args, 1)?)?;
            let (n, k) = (nonneg(f, "n", n)?, nonneg(f, "k", k)?);
            Ok(Value::Num(msg(f, nt::nchoosek(n, k))? as f64))
        }
        "factorial" => {
            let (xs, scalar) = ints(f, "n", need(f, "factorial(n)", args, 0)?)?;
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(msg(f, nt::factorial(nonneg(f, "n", x)?))?);
            }
            Ok(num_or_vec(out, scalar))
        }
        "toeplitz" | "hankel" => {
            let sig = if f == "toeplitz" { "toeplitz(c, [r])" } else { "hankel(c, [r])" };
            let c = real_vec(f, "c", need(f, sig, args, 0)?)?;
            let r = match arg_get(args, 1) {
                Some(v) => Some(real_vec(f, "r", v)?),
                None => None,
            };
            let m = if f == "toeplitz" {
                matrix_builders::toeplitz(&c, r.as_deref())
            } else {
                matrix_builders::hankel(&c, r.as_deref())
            };
            Ok(mat_value(msg(f, m)?))
        }
        "vander" => {
            let v = real_vec(f, "v", need(f, "vander(v, [n])", args, 0)?)?;
            let n = match arg_get(args, 1) {
                Some(x) => Some(x.as_index().map_err(|m| EvalError { msg: format!("vander: n: {m}") })?),
                None => None,
            };
            Ok(mat_value(msg(f, matrix_builders::vander(&v, n))?))
        }
        "null" | "orth" => {
            let a = need(f, &format!("{f}(A)"), args, 0)?;
            let m = a.to_matrix().map_err(|m| EvalError { msg: format!("{f}: {m} (real matrices only)") })?;
            let out = if f == "null" { matrix_builders::null_space(&m) } else { matrix_builders::orth(&m) };
            let out = msg(f, out)?;
            // a 1x1 result is a scalar, like every other matrix builtin; an
            // empty basis keeps its (n x 0) shape.
            Ok(mat_value(out))
        }
        "gradient" => {
            let src = need(f, "gradient(v, [h])", args, 0)?;
            if matches!(src, Value::Mat(m) if m.rows() > 1 && m.cols() > 1) {
                return e("gradient: only vectors are supported (a matrix was given)");
            }
            let v = real_vec(f, "v", src)?;
            let spacing = match arg_get(args, 1) {
                None => Spacing::Uniform(1.0),
                Some(Value::Num(h)) => Spacing::Uniform(*h),
                Some(x @ (Value::Vec(_) | Value::Mat(_))) => Spacing::Coords(real_vec(f, "h", x)?),
                Some(other) => {
                    return e(format!(
                        "gradient: h must be a spacing (number) or a coordinate vector, found {}",
                        other.type_name()
                    ))
                }
            };
            Ok(vec_val(msg(f, matrix_builders::gradient(&v, spacing))?))
        }
        other => e(format!("num_ops: `{other}` is not handled here")),
    }
}
