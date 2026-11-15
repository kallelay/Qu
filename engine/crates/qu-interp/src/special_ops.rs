//! Special-function builtins (added in v0.4.14): `erfinv`, `erfcinv`,
//! `gammainc`, `betainc`, `beta`, `besselj`, `bessely`, `besseli`, `besselk`,
//! `expint`, `ellipke`, `legendre` and `zeta`.
//!
//! The numerics are in `qu_core::special_fns`. Like `gamma` and `erf`, the
//! scalar functions work elementwise on a number, `Vec`, `Mat` or `Signal`;
//! with several arguments each one is a number or an array, and all the
//! arrays must have the same shape (a number pairs with every element).
//! Real input only; out-of-domain elements give `NaN`.

use super::{arg_get, e, map1, mat_value, EvalError, Value, R};
use qu_core::matrix::Matrix;
use qu_core::special_fns::{self as sf, Tail};
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "besseli", "besselj", "besselk", "bessely", "beta", "betainc", "ellipke", "erfcinv", "erfinv",
    "expint", "gammainc", "legendre", "zeta",
];

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn check_real(f: &str, name: &str, v: &Value) -> R<()> {
    match v {
        Value::Complex(_) | Value::CVec(_) | Value::CMat(_) => {
            e(format!("{f}: {name} must be real (complex arguments are not supported)"))
        }
        Value::Num(_) | Value::Bool(_) | Value::Vec(_) | Value::Mat(_) | Value::Signal(..) => Ok(()),
        other => e(format!("{f}: {name} must be a number, vector or matrix, found {}", other.type_name())),
    }
}

fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::Num(_) | Value::Bool(_))
}

fn flat(v: &Value) -> Vec<f64> {
    match v {
        Value::Num(x) => vec![*x],
        Value::Bool(b) => vec![*b as u8 as f64],
        Value::Vec(x) => x.to_vec(),
        Value::Mat(m) => m.as_slice().to_vec(),
        Value::Signal(x, _, _) => x.to_vec(),
        _ => Vec::new(),
    }
}

/// Elementwise `g` over several real arguments (numbers or same-shape arrays).
fn bcast(f: &str, names: &[&str], vals: &[&Value], g: impl Fn(&[f64]) -> f64) -> R<Value> {
    for (n, v) in names.iter().zip(vals) {
        check_real(f, n, v)?;
    }
    // the shape donor: the first array argument
    let Some(d) = vals.iter().position(|v| !is_scalar(v)) else {
        let a: Vec<f64> = vals.iter().map(|v| flat(v)[0]).collect();
        return Ok(Value::Num(g(&a)));
    };
    let cols: Vec<Vec<f64>> = vals.iter().map(|v| flat(v)).collect();
    let n = cols[d].len();
    for (i, v) in vals.iter().enumerate() {
        if is_scalar(v) {
            continue;
        }
        let same_kind = std::mem::discriminant(*v) == std::mem::discriminant(vals[d]);
        let same_shape = match (*v, vals[d]) {
            (Value::Mat(a), Value::Mat(b)) => a.shape() == b.shape(),
            _ => cols[i].len() == n,
        };
        if !same_kind || !same_shape {
            return e(format!(
                "{f}: {} and {} must have the same shape (or one of them a single number), got {} and {}",
                names[d],
                names[i],
                vals[d].type_name(),
                v.type_name()
            ));
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut a = vec![0.0; vals.len()];
    for k in 0..n {
        for i in 0..vals.len() {
            a[i] = if is_scalar(vals[i]) { cols[i][0] } else { cols[i][k] };
        }
        out.push(g(&a));
    }
    Ok(match vals[d] {
        Value::Mat(m) => Value::Mat(Arc::new(Matrix::from_col_major(m.rows(), m.cols(), out))),
        Value::Signal(_, fs, meta) => Value::Signal(Arc::new(out), *fs, Arc::new(meta.axis_only())),
        _ => Value::Vec(Arc::new(out)),
    })
}

pub(crate) fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "erfinv" | "erfcinv" | "expint" => {
            let sig = format!("{f}(x)");
            let x = need(f, &sig, args, 0)?;
            check_real(f, "x", x)?;
            let g: fn(f64) -> f64 = match f {
                "erfinv" => sf::erfinv,
                "erfcinv" => sf::erfcinv,
                _ => sf::expint,
            };
            map1(x.clone(), g)
        }
        "beta" => {
            let a = need(f, "beta(a, b)", args, 0)?;
            let b = need(f, "beta(a, b)", args, 1)?;
            bcast(f, &["a", "b"], &[a, b], |v| sf::beta(v[0], v[1]))
        }
        "betainc" => {
            let sig = "betainc(x, a, b)";
            let x = need(f, sig, args, 0)?;
            let a = need(f, sig, args, 1)?;
            let b = need(f, sig, args, 2)?;
            bcast(f, &["x", "a", "b"], &[x, a, b], |v| sf::betainc(v[0], v[1], v[2]))
        }
        "gammainc" => {
            let sig = "gammainc(x, a, [tail])";
            let x = need(f, sig, args, 0)?;
            let a = need(f, sig, args, 1)?;
            let tail = match arg_get(args, 2) {
                None => Tail::Lower,
                Some(Value::Str(s)) => match s.to_ascii_lowercase().as_str() {
                    "lower" => Tail::Lower,
                    "upper" => Tail::Upper,
                    other => {
                        return e(format!("gammainc: the tail must be \"lower\" or \"upper\", got \"{other}\""))
                    }
                },
                Some(other) => {
                    return e(format!(
                        "gammainc: the tail must be \"lower\" or \"upper\", found {}",
                        other.type_name()
                    ))
                }
            };
            bcast(f, &["x", "a"], &[x, a], |v| sf::gammainc(v[0], v[1], tail))
        }
        "besselj" | "bessely" | "besseli" | "besselk" => {
            let sig = format!("{f}(nu, x)");
            let nu = need(f, &sig, args, 0)?;
            let x = need(f, &sig, args, 1)?;
            let g: fn(f64, f64) -> f64 = match f {
                "besselj" => sf::bessel_j,
                "bessely" => sf::bessel_y,
                "besseli" => sf::bessel_i,
                _ => sf::bessel_k,
            };
            bcast(f, &["nu", "x"], &[nu, x], |v| g(v[0], v[1]))
        }
        "ellipke" => {
            let m = need(f, "ellipke(m)", args, 0)?;
            check_real(f, "m", m)?;
            let k = map1(m.clone(), |v| sf::ellipke(v).0)?;
            let ee = map1(m.clone(), |v| sf::ellipke(v).1)?;
            Ok(Value::Record(Arc::new(vec![("K".to_string(), k), ("E".to_string(), ee)])))
        }
        "zeta" => {
            let s = need(f, "zeta(s)", args, 0)?;
            check_real(f, "s", s)?;
            // a pole is an error, like gamma's: find it first
            if flat(s).iter().any(|&x| x == 1.0) {
                return e("zeta: pole at s = 1 (the series diverges)");
            }
            map1(s.clone(), |v| sf::zeta(v).unwrap_or(f64::NAN))
        }
        "legendre" => {
            let sig = "legendre(n, x)";
            let nv = need(f, sig, args, 0)?;
            let x = need(f, sig, args, 1)?;
            let n = match nv {
                Value::Num(v) if v.is_finite() && *v >= 0.0 && v.fract() == 0.0 && *v <= 100_000.0 => *v as usize,
                Value::Num(v) => {
                    return e(format!("legendre: n must be a non-negative whole number (at most 100000), got {v}"))
                }
                other => {
                    return e(format!("legendre: n must be a single whole number, found {}", other.type_name()))
                }
            };
            check_real(f, "x", x)?;
            if matches!(x, Value::Mat(m) if m.rows() > 1 && m.cols() > 1) {
                return e("legendre: x must be a number or a vector, not a matrix");
            }
            let xs = flat(x);
            let cols: Vec<Vec<f64>> = xs
                .iter()
                .map(|&v| sf::legendre(n, v))
                .collect::<Result<_, _>>()
                .map_err(|m| EvalError { msg: m })?;
            if is_scalar(x) {
                return Ok(Value::Vec(Arc::new(cols.into_iter().next().unwrap())));
            }
            // (n+1) x length(x): row m is P_n^m at every x
            let k = xs.len();
            let mut data = Vec::with_capacity((n + 1) * k);
            for c in &cols {
                data.extend_from_slice(c);
            }
            Ok(mat_value(Matrix::from_col_major(n + 1, k, data)))
        }
        other => e(format!("special_ops: `{other}` is not handled here")),
    }
}
