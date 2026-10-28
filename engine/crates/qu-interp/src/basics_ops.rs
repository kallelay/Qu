//! The everyday numeric basics that every MATLAB/NumPy user reaches for
//! first: `atan2`, `hypot`, `fix`, `rem`, `maximum`/`minimum`, `isnan`/
//! `isinf`/`isfinite`, `cumprod`, `repmat`, `kron`, `outer`, `cross`,
//! `tril`/`triu`, `nnz`, `lstsq` and `expm`.
//!
//! They are one module with one dispatch arm in `lib.rs` (`basics_ops::call`)
//! so the footprint in that very large, concurrently edited file stays at a
//! `mod` line, the `BUILTIN_NAMES` entries and a single `match` arm.
//!
//! Conventions shared by the whole file (they match the neighbours -- `max`,
//! `cumsum`, `diag`, `hstack` -- rather than inventing new ones):
//!
//! * Elementwise two-argument functions go through `Interp::binop_ew_named`,
//!   i.e. exactly the broadcasting `max(a, b)` already has: scalar/scalar,
//!   scalar/vector, vector/vector of equal length, matrix/matrix with the
//!   usual row/column broadcast. A length mismatch is an error, never a
//!   silent truncation.
//! * A bare `Vec` is a column when it takes part in linear algebra (`kron`,
//!   `lstsq`, `tril`) and a *row* when it is tiled (`repmat`), the same
//!   split `hstack`/`vstack` and `x * A` already make.
//! * Complex arguments are refused with a message that says so, except
//!   where the complex support already existed (`lstsq` reuses
//!   `cmatrix::lstsq`) or is trivial (`isnan`/`isinf`/`isfinite`/`nnz`).

use super::{
    arg0, arg_get, e, mat_value, to_cow, to_vec, EvalError, Interp, Value, R,
};
use qu_core::cmatrix::CMatrix;
use qu_core::matrix::Matrix;
use qu_core::Complex64;
use std::sync::Arc;

pub(crate) fn call(interp: &Interp, f: &str, args: &[Value]) -> R<Value> {
    match f {
        "atan2" => {
            let (a, b) = two(f, "atan2(y, x)", args)?;
            interp.binop_ew_named(f64::atan2, a.clone(), b.clone())
        }
        "hypot" => {
            let (a, b) = two(f, "hypot(x, y)", args)?;
            interp.binop_ew_named(f64::hypot, a.clone(), b.clone())
        }
        // `%` on f64 is C `fmod`: the result takes the sign of the DIVIDEND,
        // which is MATLAB's `rem`. (The `mod` operator is the other one --
        // sign of the divisor, `rem_euclid`.) `rem(x, 0)` is NaN.
        "rem" => {
            let (a, b) = two(f, "rem(a, b)", args)?;
            interp.binop_ew_named(|x, y| x % y, a.clone(), b.clone())
        }
        // Same function `max(a, b)` / `min(a, b)` already use, so NaN is
        // treated identically: a NaN operand loses to the number.
        "maximum" => {
            let (a, b) = two(f, "maximum(a, b)", args)?;
            interp.binop_ew_named(f64::max, a.clone(), b.clone())
        }
        "minimum" => {
            let (a, b) = two(f, "minimum(a, b)", args)?;
            interp.binop_ew_named(f64::min, a.clone(), b.clone())
        }
        "fix" => super::map1(one(f, args)?.clone(), f64::trunc),
        "isnan" => pred(f, one(f, args)?, f64::is_nan, |c| c.re.is_nan() || c.im.is_nan()),
        "isinf" => pred(f, one(f, args)?, f64::is_infinite, |c| {
            c.re.is_infinite() || c.im.is_infinite()
        }),
        "isfinite" => pred(f, one(f, args)?, f64::is_finite, |c| {
            c.re.is_finite() && c.im.is_finite()
        }),
        "cumprod" => cumprod(one(f, args)?),
        "repmat" => repmat(args),
        "kron" => {
            let (a, b) = two(f, "kron(A, B)", args)?;
            kron(a, b)
        }
        "outer" => {
            let (a, b) = two(f, "outer(u, v)", args)?;
            outer(a, b)
        }
        "cross" => {
            let (a, b) = two(f, "cross(a, b)", args)?;
            cross(a, b)
        }
        "tril" | "triu" => tri(f, args),
        "nnz" => nnz(one(f, args)?),
        "lstsq" => lstsq(args),
        "expm" => expm_value(one(f, args)?),
        other => e(format!("basics_ops: `{other}` is not handled here")),
    }
}

fn one<'a>(f: &str, args: &'a [Value]) -> R<&'a Value> {
    match arg_get(args, 0) {
        Some(v) => Ok(v),
        None => e(format!("{f}(x) needs an argument")),
    }
}

fn two<'a>(_f: &str, sig: &str, args: &'a [Value]) -> R<(&'a Value, &'a Value)> {
    match (arg_get(args, 0), arg_get(args, 1)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => e(format!("{sig} needs two arguments")),
    }
}

fn is_complex(v: &Value) -> bool {
    matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_))
}

fn no_complex<T>(f: &str, v: &Value) -> R<T> {
    e(format!(
        "{f}: complex {} is not supported (real arguments only)",
        match v {
            Value::Complex(_) => "numbers",
            Value::CVec(_) => "vectors",
            _ => "matrices",
        }
    ))
}

// ---------------------------------------------------------------- isnan etc.

/// scalar -> bool, vector -> mask (so `x[isnan(x)]` indexes), matrix -> a
/// flat column-major mask (the way every reduction already reads a matrix;
/// `any(isnan(M))` and `sum(isinf(M))` are the uses).
fn pred(
    f: &str,
    v: &Value,
    p: fn(f64) -> bool,
    pc: fn(Complex64) -> bool,
) -> R<Value> {
    match v {
        Value::Num(x) => Ok(Value::Bool(p(*x))),
        Value::Bool(b) => Ok(Value::Bool(p(if *b { 1.0 } else { 0.0 }))),
        Value::Complex(c) => Ok(Value::Bool(pc(*c))),
        Value::CVec(xs) => Ok(Value::Mask(xs.iter().map(|c| pc(*c)).collect())),
        Value::CMat(m) => Ok(Value::Mask(m.as_slice().iter().map(|c| pc(*c)).collect())),
        Value::Vec(_) | Value::Mat(_) | Value::Mask(_) | Value::Signal(..) => {
            let xs = to_cow(v)?;
            Ok(Value::Mask(xs.iter().map(|x| p(*x)).collect()))
        }
        other => e(format!(
            "{f}: expected a number, vector or matrix, found {}",
            other.type_name()
        )),
    }
}

// ------------------------------------------------------------------ cumprod

fn cumprod(src: &Value) -> R<Value> {
    // Flat running product, same shape rules as `cumsum` (a matrix is read
    // column-major; a signal stays a signal at the same Fs).
    let xs = to_vec(src)?;
    let mut acc = 1.0f64;
    let out: Vec<f64> = xs
        .iter()
        .map(|&x| {
            acc *= x;
            acc
        })
        .collect();
    match src {
        Value::Signal(_, fs, m) => Ok(Value::Signal(out.into(), *fs, Arc::new(m.axis_only()))),
        _ => Ok(Value::Vec(Arc::new(out))),
    }
}

// ------------------------------------------------------------------- counts

fn count_arg(f: &str, what: &str, v: &Value) -> R<usize> {
    let n = v.as_num().map_err(|m| EvalError { msg: format!("{f}: {what}: {m}") })?;
    if !n.is_finite() || n.fract() != 0.0 || n < 1.0 || n > 1e9 {
        return e(format!("{f}: {what} must be a positive whole number, got {n}"));
    }
    Ok(n as usize)
}

/// `(m, n)` from `repmat(A, m, n)`, `repmat(A, n)` (= `n, n`) or `repmat(A, [m, n])`.
fn rep_counts(args: &[Value]) -> R<(usize, usize)> {
    let Some(c1) = arg_get(args, 1) else {
        return e("repmat(A, m, n) needs a repeat count");
    };
    if let Some(c2) = arg_get(args, 2) {
        return Ok((count_arg("repmat", "m", c1)?, count_arg("repmat", "n", c2)?));
    }
    match c1 {
        Value::Vec(v) if v.len() == 2 => Ok((
            count_arg("repmat", "m", &Value::Num(v[0]))?,
            count_arg("repmat", "n", &Value::Num(v[1]))?,
        )),
        Value::Vec(v) if v.len() == 1 => {
            let n = count_arg("repmat", "n", &Value::Num(v[0]))?;
            Ok((n, n))
        }
        Value::Vec(v) => e(format!(
            "repmat: the repeat count vector must be [m, n], got {} elements",
            v.len()
        )),
        other => {
            let n = count_arg("repmat", "n", other)?;
            Ok((n, n))
        }
    }
}

fn repmat(args: &[Value]) -> R<Value> {
    let src = arg0(args)?;
    let (m, n) = rep_counts(args)?;
    if let Value::Str(s) = src {
        // Strings are a row of characters: only the horizontal count means
        // anything, so `repmat("ab", 3)` is "ababab" (not a 3x3 block).
        let times = if arg_get(args, 2).is_some() || matches!(arg_get(args, 1), Some(Value::Vec(_))) {
            if m != 1 {
                return e("repmat: a string can only be repeated along one row (use m = 1)");
            }
            n
        } else {
            m
        };
        return Ok(Value::Str(s.repeat(times)));
    }
    if is_complex(src) {
        return no_complex("repmat", src);
    }
    let (rows, cols, data, vec_like): (usize, usize, Vec<f64>, bool) = match src {
        Value::Num(x) => (1, 1, vec![*x], true),
        Value::Bool(b) => (1, 1, vec![if *b { 1.0 } else { 0.0 }], true),
        // a bare Vec tiles as a ROW, like hstack/vstack and `[v; v]`
        Value::Vec(v) => (1, v.len(), v.as_ref().clone(), true),
        Value::Mat(mm) => (mm.rows(), mm.cols(), mm.as_slice().to_vec(), false),
        other => {
            return e(format!(
                "repmat: expected a number, vector, matrix or string, found {}",
                other.type_name()
            ))
        }
    };
    if rows == 0 || cols == 0 {
        return e("repmat: cannot tile an empty array");
    }
    let (orows, ocols) = (rows * m, cols * n);
    let mut out = Vec::with_capacity(orows * ocols);
    for jc in 0..ocols {
        let sc = jc % cols;
        for ir in 0..orows {
            out.push(data[(ir % rows) + sc * rows]);
        }
    }
    if vec_like && (orows == 1 || ocols == 1) {
        if out.len() == 1 {
            return Ok(Value::Num(out[0]));
        }
        return Ok(Value::Vec(Arc::new(out)));
    }
    Ok(mat_value(Matrix::from_col_major(orows, ocols, out)))
}

// --------------------------------------------------------- kron / outer / cross

fn real_matrix(f: &str, v: &Value) -> R<Matrix> {
    if is_complex(v) {
        return no_complex(f, v);
    }
    v.to_matrix().map_err(|m| EvalError { msg: format!("{f}: {m}") })
}

fn is_vec_like(v: &Value) -> bool {
    matches!(v, Value::Vec(_) | Value::Num(_) | Value::Bool(_))
}

fn kron(a: &Value, b: &Value) -> R<Value> {
    let am = real_matrix("kron", a)?;
    let bm = real_matrix("kron", b)?;
    let (ar, ac) = am.shape();
    let (br, bc) = bm.shape();
    if am.is_empty() || bm.is_empty() {
        return e("kron: empty argument");
    }
    let (orows, ocols) = (ar * br, ac * bc);
    let mut out = vec![0.0; orows * ocols];
    for j in 0..ac {
        for i in 0..ar {
            let av = am.as_slice()[i + j * ar];
            for l in 0..bc {
                for k in 0..br {
                    out[(i * br + k) + (j * bc + l) * orows] = av * bm.as_slice()[k + l * br];
                }
            }
        }
    }
    // the Kronecker product of two vectors is a vector
    if is_vec_like(a) && is_vec_like(b) {
        return Ok(if out.len() == 1 { Value::Num(out[0]) } else { Value::Vec(Arc::new(out)) });
    }
    Ok(mat_value(Matrix::from_col_major(orows, ocols, out)))
}

fn flat_real(f: &str, v: &Value) -> R<Vec<f64>> {
    if is_complex(v) {
        return no_complex(f, v);
    }
    match v {
        Value::Num(_) | Value::Bool(_) | Value::Vec(_) | Value::Mat(_) | Value::Signal(..) => {
            Ok(to_cow(v)?.into_owned())
        }
        other => e(format!("{f}: expected numbers, found {}", other.type_name())),
    }
}

fn outer(a: &Value, b: &Value) -> R<Value> {
    let u = flat_real("outer", a)?;
    let v = flat_real("outer", b)?;
    if u.is_empty() || v.is_empty() {
        return e("outer: empty argument");
    }
    let mut out = Vec::with_capacity(u.len() * v.len());
    for vj in &v {
        for ui in &u {
            out.push(ui * vj);
        }
    }
    Ok(mat_value(Matrix::from_col_major(u.len(), v.len(), out)))
}

fn cross(a: &Value, b: &Value) -> R<Value> {
    let u = flat_real("cross", a)?;
    let v = flat_real("cross", b)?;
    if u.len() != 3 || v.len() != 3 {
        return e(format!(
            "cross: needs two 3-element vectors, got {} and {} elements",
            u.len(),
            v.len()
        ));
    }
    Ok(Value::Vec(Arc::new(vec![
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ])))
}

// -------------------------------------------------------------- tril / triu

fn tri(f: &str, args: &[Value]) -> R<Value> {
    let src = arg0(args)?;
    let k: i64 = match arg_get(args, 1) {
        None => 0,
        Some(v) => {
            let x = v.as_num().map_err(|m| EvalError { msg: format!("{f}: k: {m}") })?;
            if !x.is_finite() || x.fract() != 0.0 {
                return e(format!("{f}: the diagonal offset k must be a whole number, got {x}"));
            }
            x as i64
        }
    };
    let m = real_matrix(f, src)?;
    let (rows, cols) = m.shape();
    let mut out = m.as_slice().to_vec();
    for j in 0..cols {
        for i in 0..rows {
            let d = j as i64 - i as i64; // column minus row
            let keep = if f == "tril" { d <= k } else { d >= k };
            if !keep {
                out[i + j * rows] = 0.0;
            }
        }
    }
    Ok(mat_value(Matrix::from_col_major(rows, cols, out)))
}

// --------------------------------------------------------------------- nnz

fn nnz(v: &Value) -> R<Value> {
    // NaN != 0, so a NaN counts (MATLAB's nnz does too).
    let n = match v {
        Value::Complex(c) => (c.re != 0.0 || c.im != 0.0) as usize,
        Value::CVec(xs) => xs.iter().filter(|c| c.re != 0.0 || c.im != 0.0).count(),
        Value::CMat(m) => m.as_slice().iter().filter(|c| c.re != 0.0 || c.im != 0.0).count(),
        Value::Num(_) | Value::Bool(_) | Value::Vec(_) | Value::Mat(_) | Value::Mask(_)
        | Value::Signal(..) => to_cow(v)?.iter().filter(|x| **x != 0.0).count(),
        other => {
            return e(format!(
                "nnz: expected a number, vector or matrix, found {}",
                other.type_name()
            ))
        }
    };
    Ok(Value::Num(n as f64))
}

// -------------------------------------------------------------------- lstsq

/// Minimum-norm least-squares solution `x` of `A x ~= b` via the SVD
/// pseudo-inverse, always (never the LU shortcut `solve` tries first), so a
/// rank-deficient or under-determined `A` returns the shortest solution
/// instead of an arbitrary one. `b` may be a vector or a matrix of
/// right-hand sides. Optional third argument: relative singular-value
/// cut-off (default `eps * max(rows, cols)`, NumPy's default).
fn lstsq(args: &[Value]) -> R<Value> {
    let a = arg0(args)?;
    let Some(b) = arg_get(args, 1) else {
        return e("lstsq(A, b) needs two arguments");
    };
    let tol = match arg_get(args, 2) {
        None => None,
        Some(t) => Some(t.as_num().map_err(|m| EvalError { msg: format!("lstsq: tol: {m}") })?),
    };
    let b_is_vec = matches!(b, Value::Vec(_) | Value::CVec(_) | Value::Num(_) | Value::Bool(_));
    if is_complex(a) || is_complex(b) {
        if tol.is_some() {
            return e("lstsq: the tolerance argument is not supported for complex systems");
        }
        let to_c = |v: &Value| -> R<CMatrix> {
            Ok(match v {
                Value::CMat(m) => (**m).clone(),
                Value::CVec(x) => CMatrix::from_column(x),
                Value::Complex(c) => CMatrix::from_column(&[*c]),
                other => CMatrix::from_real(&real_matrix("lstsq", other)?),
            })
        };
        let (ac, bc) = (to_c(a)?, to_c(b)?);
        if ac.rows() != bc.rows() {
            return e(format!(
                "lstsq: A has {} rows but b has {}",
                ac.rows(),
                bc.rows()
            ));
        }
        let x = qu_core::cmatrix::lstsq(&ac, &bc).map_err(|er| EvalError { msg: er.to_string() })?;
        if b_is_vec {
            return Ok(Value::CVec(Arc::new(x.as_slice().to_vec())));
        }
        return Ok(super::cmat_value(x));
    }
    let am = real_matrix("lstsq", a)?;
    let bm = real_matrix("lstsq", b)?;
    if am.rows() != bm.rows() {
        return e(format!(
            "lstsq: A has {} rows but b has {} (they must match)",
            am.rows(),
            bm.rows()
        ));
    }
    let pinv = qu_core::linalg::pseudo_inverse(&am, tol)
        .map_err(|er| EvalError { msg: format!("lstsq: {er}") })?;
    let x = pinv
        .matrix
        .matmul(&bm)
        .map_err(|er| EvalError { msg: format!("lstsq: {er}") })?;
    if b_is_vec {
        let data = x.into_vec();
        return Ok(if data.len() == 1 { Value::Num(data[0]) } else { Value::Vec(Arc::new(data)) });
    }
    Ok(mat_value(x))
}

// --------------------------------------------------------------------- expm

fn expm_value(v: &Value) -> R<Value> {
    if is_complex(v) {
        return e("expm: complex matrices are not supported yet (real square matrices only)");
    }
    match v {
        Value::Num(x) => return Ok(Value::Num(x.exp())),
        Value::Bool(b) => return Ok(Value::Num(if *b { 1f64.exp() } else { 1.0 })),
        _ => {}
    }
    let m = match v {
        Value::Mat(m) => (**m).clone(),
        Value::Vec(x) if x.len() == 1 => return Ok(Value::Num(x[0].exp())),
        Value::Vec(x) => {
            return e(format!(
                "expm: needs a square matrix, got a vector of {} elements (use exp(x) for elementwise)",
                x.len()
            ))
        }
        other => {
            return e(format!("expm: expected a square matrix, found {}", other.type_name()))
        }
    };
    if m.rows() != m.cols() {
        return e(format!("expm: needs a square matrix, got {}x{}", m.rows(), m.cols()));
    }
    if m.is_empty() {
        return e("expm: empty matrix");
    }
    if m.as_slice().iter().any(|x| !x.is_finite()) {
        return e("expm: the matrix contains NaN or Inf");
    }
    Ok(mat_value(expm_real(&m)?))
}

fn scaled_sum(n: usize, terms: &[(f64, &Matrix)]) -> Matrix {
    let mut out = vec![0.0; n * n];
    for (c, m) in terms {
        for (o, x) in out.iter_mut().zip(m.as_slice()) {
            *o += c * x;
        }
    }
    Matrix::from_col_major(n, n, out)
}

fn add_identity(m: &mut Matrix, c: f64) {
    let n = m.rows();
    let mut data = m.as_slice().to_vec();
    for i in 0..n {
        data[i + i * n] += c;
    }
    *m = Matrix::from_col_major(n, n, data);
}

/// Matrix exponential by scaling and squaring with the degree-13 Pade
/// approximant (Higham, "The scaling and squaring method for the matrix
/// exponential revisited", SIAM J. Matrix Anal. Appl. 2005) -- the method
/// SciPy's `expm` uses for large norms.
pub(crate) fn expm_real(a: &Matrix) -> R<Matrix> {
    const B: [f64; 14] = [
        64764752532480000.0,
        32382376266240000.0,
        7771770303897600.0,
        1187353796428800.0,
        129060195264000.0,
        10559470521600.0,
        670442572800.0,
        33522128640.0,
        1323241920.0,
        40840800.0,
        960960.0,
        16380.0,
        182.0,
        1.0,
    ];
    const THETA13: f64 = 5.371920351148152;
    let n = a.rows();
    let norm1 = (0..n)
        .map(|j| (0..n).map(|i| a.as_slice()[i + j * n].abs()).sum::<f64>())
        .fold(0.0, f64::max);
    let s: i32 = if norm1 > THETA13 { (norm1 / THETA13).log2().ceil() as i32 } else { 0 };
    let scale = 2f64.powi(-s);
    let a1 = scaled_sum(n, &[(scale, a)]);
    let mm = |x: &Matrix, y: &Matrix| -> R<Matrix> {
        x.matmul(y).map_err(|er| EvalError { msg: format!("expm: {er}") })
    };
    let a2 = mm(&a1, &a1)?;
    let a4 = mm(&a2, &a2)?;
    let a6 = mm(&a4, &a2)?;
    // U = A (A6 (b13 A6 + b11 A4 + b9 A2) + b7 A6 + b5 A4 + b3 A2 + b1 I)
    let inner_u = scaled_sum(n, &[(B[13], &a6), (B[11], &a4), (B[9], &a2)]);
    let mut u_poly = scaled_sum(n, &[(B[7], &a6), (B[5], &a4), (B[3], &a2)]);
    let t = mm(&a6, &inner_u)?;
    u_poly = scaled_sum(n, &[(1.0, &u_poly), (1.0, &t)]);
    add_identity(&mut u_poly, B[1]);
    let u = mm(&a1, &u_poly)?;
    // V = A6 (b12 A6 + b10 A4 + b8 A2) + b6 A6 + b4 A4 + b2 A2 + b0 I
    let inner_v = scaled_sum(n, &[(B[12], &a6), (B[10], &a4), (B[8], &a2)]);
    let mut v = scaled_sum(n, &[(B[6], &a6), (B[4], &a4), (B[2], &a2)]);
    let t = mm(&a6, &inner_v)?;
    v = scaled_sum(n, &[(1.0, &v), (1.0, &t)]);
    add_identity(&mut v, B[0]);
    // R = (V - U)^-1 (V + U)
    let p = scaled_sum(n, &[(1.0, &v), (1.0, &u)]);
    let q = scaled_sum(n, &[(1.0, &v), (-1.0, &u)]);
    let mut r = qu_core::linalg::least_squares(&q, &p, None)
        .map_err(|er| EvalError { msg: format!("expm: {er}") })?;
    for _ in 0..s {
        r = mm(&r, &r)?;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Interp {
        let mut it = Interp::new();
        it.run(src).unwrap_or_else(|er| panic!("run failed: {er}\n{src}"));
        it
    }
    fn err(src: &str) -> String {
        let mut it = Interp::new();
        match it.run(src) {
            Ok(()) => panic!("expected an error: {src}"),
            Err(er) => er.msg,
        }
    }
    fn nums(it: &Interp, name: &str) -> Vec<f64> {
        match it.get(name) {
            Some(Value::Num(x)) => vec![*x],
            Some(Value::Vec(v)) => v.as_ref().clone(),
            Some(Value::Mat(m)) => m.as_slice().to_vec(),
            other => panic!("`{name}` is {other:?}"),
        }
    }
    fn close(a: &[f64], b: &[f64], tol: f64) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= tol || (x.is_nan() && y.is_nan()), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn atan2_hypot_fix_rem() {
        let it = run("a = atan2(1, -1)\nb = atan2([1, -1], [1, 1])\nc = hypot(3, 4)\nd = hypot([3, 5], 4)\n\
                      f = fix([-2.7, 2.7, -0.2])\nr = rem([-7, 7, -7, 7], [3, 3, -3, -3])\nm = -7 mod 3");
        close(&nums(&it, "a"), &[3.0 * std::f64::consts::FRAC_PI_4], 1e-15);
        close(&nums(&it, "b"), &[std::f64::consts::FRAC_PI_4, -std::f64::consts::FRAC_PI_4], 1e-15);
        close(&nums(&it, "c"), &[5.0], 0.0);
        close(&nums(&it, "d"), &[5.0, 6.403124237432849], 1e-15);
        close(&nums(&it, "f"), &[-2.0, 2.0, 0.0], 0.0);
        // sign of the dividend, unlike `mod`
        close(&nums(&it, "r"), &[-1.0, 1.0, -1.0, 1.0], 0.0);
        close(&nums(&it, "m"), &[2.0], 0.0);
    }

    #[test]
    fn rem_by_zero_is_nan_and_broadcasts_matrices() {
        let it = run("a = rem(5, 0)\nM = [1, 2; 3, 4]\nb = rem(M, 2)");
        assert!(nums(&it, "a")[0].is_nan());
        close(&nums(&it, "b"), &[1.0, 1.0, 0.0, 0.0], 0.0);
        assert!(err("x = rem([1,2,3], [1,2])").contains("mismatch"));
        assert!(err("x = atan2(1)").contains("two arguments"));
    }

    #[test]
    fn maximum_minimum_match_max_min_including_nan() {
        let it = run("a = maximum([1, 5, nan], [3, 2, 4])\nb = minimum([1, 5, nan], [3, 2, 4])\n\
                      c = max([1, 5, nan], [3, 2, 4])\nd = min([1, 5, nan], [3, 2, 4])\ng = maximum([1,2,3], 2)");
        close(&nums(&it, "a"), &[3.0, 5.0, 4.0], 0.0);
        close(&nums(&it, "b"), &[1.0, 2.0, 4.0], 0.0);
        close(&nums(&it, "a"), &nums(&it, "c"), 0.0);
        close(&nums(&it, "b"), &nums(&it, "d"), 0.0);
        close(&nums(&it, "g"), &[2.0, 2.0, 3.0], 0.0);
    }

    #[test]
    fn isnan_isinf_isfinite_scalar_and_mask() {
        let it = run("x = [1, nan, inf, -inf, 2]\na = isnan(x)\nb = isinf(x)\nc = isfinite(x)\n\
                      s = isnan(nan)\nt = isfinite(3)\nclean = x[isfinite(x)]\nn = sum(isnan(x))");
        assert!(matches!(it.get("a"), Some(Value::Mask(m)) if m == &vec![false, true, false, false, false]));
        assert!(matches!(it.get("b"), Some(Value::Mask(m)) if m == &vec![false, false, true, true, false]));
        assert!(matches!(it.get("c"), Some(Value::Mask(m)) if m == &vec![true, false, false, false, true]));
        assert!(matches!(it.get("s"), Some(Value::Bool(true))));
        assert!(matches!(it.get("t"), Some(Value::Bool(true))));
        close(&nums(&it, "clean"), &[1.0, 2.0], 0.0);
        close(&nums(&it, "n"), &[1.0], 0.0);
        assert!(err("x = isnan(\"a\")").contains("isnan"));
    }

    #[test]
    fn cumprod_runs_and_keeps_signal() {
        let it = run("a = cumprod([1, 2, 3, 4])\nb = cumprod([2, nan, 3])");
        close(&nums(&it, "a"), &[1.0, 2.0, 6.0, 24.0], 0.0);
        let b = nums(&it, "b");
        assert_eq!(b[0], 2.0);
        assert!(b[1].is_nan() && b[2].is_nan());
    }

    #[test]
    fn repmat_shapes() {
        let it = run("M = [1, 2; 3, 4]\nA = repmat(M, 2, 3)\nB = repmat(M, 2)\nv = repmat([1, 2], 1, 3)\n\
                      w = repmat([1, 2], 2, 1)\ns = repmat(\"ab\", 3)\nc = repmat(7, 1, 4)");
        let a = match it.get("A") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(a.shape(), (4, 6));
        assert_eq!(a.get(2, 4).unwrap(), 1.0);
        assert_eq!(a.get(3, 5).unwrap(), 4.0);
        let b = match it.get("B") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(b.shape(), (4, 4));
        close(&nums(&it, "v"), &[1.0, 2.0, 1.0, 2.0, 1.0, 2.0], 0.0);
        let w = match it.get("w") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(w.shape(), (2, 2));
        assert!(matches!(it.get("s"), Some(Value::Str(s)) if s == "ababab"));
        close(&nums(&it, "c"), &[7.0; 4], 0.0);
        assert!(err("x = repmat([1,2], 0, 2)").contains("positive whole number"));
        assert!(err("x = repmat([1,2], 1.5, 2)").contains("whole number"));
    }

    #[test]
    fn kron_outer_cross() {
        let it = run("K = kron([1, 2; 3, 4], [1, 1; 1, 1])\nkv = kron([1, 2], [1, 10])\nO = outer([1, 2, 3], [4, 5])\n\
                      c = cross([1, 0, 0], [0, 1, 0])\nc2 = cross([1, 2, 3], [4, 5, 6])");
        let k = match it.get("K") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(k.shape(), (4, 4));
        assert_eq!(k.get(0, 2).unwrap(), 2.0);
        assert_eq!(k.get(3, 3).unwrap(), 4.0);
        close(&nums(&it, "kv"), &[1.0, 10.0, 2.0, 20.0], 0.0);
        let o = match it.get("O") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(o.shape(), (3, 2));
        assert_eq!(o.get(2, 1).unwrap(), 15.0);
        close(&nums(&it, "c"), &[0.0, 0.0, 1.0], 0.0);
        close(&nums(&it, "c2"), &[-3.0, 6.0, -3.0], 0.0);
        assert!(err("x = cross([1,2], [3,4])").contains("3-element"));
    }

    #[test]
    fn tril_triu_nnz() {
        let it = run("M = [1, 2, 3; 4, 5, 6; 7, 8, 9]\nL = tril(M)\nU = triu(M)\nL1 = tril(M, -1)\nU1 = triu(M, 1)\n\
                      n = nnz([0, 1, nan, 0, 2])\nn2 = nnz(L)");
        close(&nums(&it, "L"), &[1.0, 4.0, 7.0, 0.0, 5.0, 8.0, 0.0, 0.0, 9.0], 0.0);
        close(&nums(&it, "U"), &[1.0, 0.0, 0.0, 2.0, 5.0, 0.0, 3.0, 6.0, 9.0], 0.0);
        close(&nums(&it, "L1"), &[0.0, 4.0, 7.0, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0], 0.0);
        close(&nums(&it, "U1"), &[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 6.0, 0.0], 0.0);
        close(&nums(&it, "n"), &[3.0], 0.0);
        close(&nums(&it, "n2"), &[6.0], 0.0);
    }

    #[test]
    fn lstsq_full_rank_and_rank_deficient() {
        let it = run("A = [1, 0; 0, 1; 1, 1]\nb = [1, 2, 3.5]\nx = lstsq(A, b)\n\
                      R = [1, 1; 1, 1]\ny = lstsq(R, [2, 2])\nB = lstsq(A, [1, 2; 2, 1; 3, 3])");
        close(&nums(&it, "x"), &[1.1666666666666667, 2.1666666666666665], 1e-13);
        // min-norm solution of x1 + x2 = 2 is (1, 1)
        close(&nums(&it, "y"), &[1.0, 1.0], 1e-13);
        let bm = match it.get("B") { Some(Value::Mat(m)) => m.clone(), o => panic!("{o:?}") };
        assert_eq!(bm.shape(), (2, 2));
        assert!(err("x = lstsq([1,2;3,4], [1,2,3])").contains("rows"));
    }

    #[test]
    fn expm_known_cases() {
        // rotation generator: expm([0 -t; t 0]) = [cos -sin; sin cos]
        let it = run("t = 0.7\nR = expm([0, -t; t, 0])\nN = expm([0, 1, 0; 0, 0, 1; 0, 0, 0])\nZ = expm(zeros(3, 3))\ns = expm(2)");
        close(&nums(&it, "R"), &[0.7f64.cos(), 0.7f64.sin(), -(0.7f64.sin()), 0.7f64.cos()], 1e-14);
        close(&nums(&it, "N"), &[1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.5, 1.0, 1.0], 1e-14);
        close(&nums(&it, "Z"), &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], 0.0);
        close(&nums(&it, "s"), &[2f64.exp()], 1e-14);
        // large norm exercises the squaring loop: diag(10, -10)
        let it = run("D = expm([10, 0; 0, -10])");
        let d = nums(&it, "D");
        assert!((d[0] / 10f64.exp() - 1.0).abs() < 1e-13);
        assert!((d[3] / (-10f64).exp() - 1.0).abs() < 1e-12);
        assert!(err("x = expm([1, 2, 3])").contains("square"));
        assert!(err("x = expm([1, 2; 3, 4; 5, 6])").contains("square"));
        assert!(err("x = expm([1, nan; 0, 1])").contains("NaN"));
    }

    #[test]
    fn expm1_is_still_the_scalar_function() {
        let it = run("y = expm1(1e-15)");
        close(&nums(&it, "y"), &[1e-15], 1e-20);
    }

    #[test]
    fn json_and_csv_text_round_trip_awkward_doubles_exactly() {
        // 123456789.123456789 is one ulp off through serde_json's default
        // (fast) float parser; `float_roundtrip` makes `load`/`parse_json` exact.
        let it = run("v = [1/3, 1e-300, 123456789.123456789, 5e-324, 9007199254740992, 0.1+0.2, -0.0]
                      j = parse_json(jsonify(v))
ok_j = sum(j == v)
                      t = parse_csv(csvify(table(a=v)))
ok_c = sum(t.a == v)");
        close(&nums(&it, "ok_j"), &[7.0], 0.0);
        close(&nums(&it, "ok_c"), &[7.0], 0.0);
    }
}
