//! Matrix properties and norms (added in v0.4.14): machine epsilon, Hilbert
//! matrices, structure predicates (`issymmetric`, `isposdef`, `isdiag`,
//! `istril`, `istriu`, `isbanded`, `bandwidth`), the reciprocal condition
//! number `rcond`, and the `p`-norms behind `norm(x, p)` and `cond(A, p)`.
//!
//! Everything works on dense, real, column-major matrices. The predicates
//! are structural: they compare entries with `0.0` and with each other and
//! never look at rounding, apart from the explicit `tol` of `issymmetric`
//! and the symmetry slack `isposdef` documents.
//!
//! `rcond` has two paths. Up to [`RCOND_EXACT_MAX`] rows it computes the
//! inverse from an LU factorisation and takes its 1-norm, so the result is
//! `1 / (norm1(A) * norm1(inv(A)))` to the rounding of that inverse. Above
//! it, it runs the Hager / Higham 1-norm estimator (Higham, "FORTRAN codes
//! for estimating the one-norm of a real or complex matrix", 1988), which
//! needs a handful of triangular solves instead of an `O(n^3)` inverse and
//! returns a lower bound for `norm1(inv(A))` that is within a small factor
//! of it in practice (the tests check a factor of 3 on a few hundred random
//! and Hilbert-like matrices).

use crate::dense_lu::{self, DenseLu};
use crate::linalg::{self, LinalgError};
use crate::matrix::Matrix;

/// Largest order for which `rcond` forms the inverse exactly.
pub const RCOND_EXACT_MAX: usize = 300;
/// Largest order `hilb` will build (a 5000 x 5000 matrix is 200 MB).
pub const HILB_MAX: usize = 5000;
/// Largest order `invhilb` supports: every entry of the inverse Hilbert
/// matrix is an integer, and from `n = 13` some exceed `2^53`, which a double
/// can no longer hold exactly.
pub const INVHILB_MAX: usize = 12;

/// Which norm `norm(x, p)` / `cond(A, p)` asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NormP {
    /// A number: `1`, `2`, `inf`, `-inf`, `0` or any `p >= 1` for a vector.
    P(f64),
    /// The Frobenius norm (`"fro"`).
    Fro,
}

fn at(a: &Matrix, i: usize, j: usize) -> f64 {
    a.as_slice()[i + j * a.rows()]
}

// ---------------------------------------------------------------- eps ----

/// Spacing of doubles at `|x|` (MATLAB's `eps(x)`): the distance from `|x|`
/// to the next larger double. `eps_at(1.0) = 2^-52`, `eps_at(0.0)` is the
/// smallest subnormal, and `NaN` for a `NaN` or infinite `x`. At the largest
/// finite double there is no larger one, so the spacing below it is returned.
pub fn eps_at(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let a = x.abs();
    if a == f64::MAX {
        return a - f64::from_bits(a.to_bits() - 1);
    }
    f64::from_bits(a.to_bits() + 1) - a
}

// ------------------------------------------------------------- hilbert ----

/// The `n x n` Hilbert matrix, `H[i][j] = 1 / (i + j + 1)` (0-based).
pub fn hilb(n: usize) -> Result<Matrix, String> {
    if n == 0 || n > HILB_MAX {
        return Err(format!("hilb: n must be between 1 and {HILB_MAX}, got {n}"));
    }
    let mut d = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            d[i + j * n] = 1.0 / (i + j + 1) as f64;
        }
    }
    Ok(Matrix::from_col_major(n, n, d))
}

fn binom(n: u128, k: u128) -> u128 {
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        r = r * (n - i) / (i + 1);
    }
    r
}

/// The exact inverse of the `n x n` Hilbert matrix. The entries are the
/// integers `(-1)^(i+j) (i+j-1) C(n+i-1, n-j) C(n+j-1, n-i) C(i+j-2, i-1)^2`
/// (1-based), computed in 128-bit integer arithmetic and only then turned
/// into doubles, so they carry no rounding at all. Limited to
/// `n <= INVHILB_MAX`, where every entry is below `2^53`.
pub fn invhilb(n: usize) -> Result<Matrix, String> {
    if n == 0 || n > INVHILB_MAX {
        return Err(format!(
            "invhilb: n must be between 1 and {INVHILB_MAX} (from 13 the entries exceed 2^53 and a \
             double cannot hold them exactly), got {n}"
        ));
    }
    let nn = n as u128;
    let mut d = vec![0.0; n * n];
    for j in 1..=n {
        for i in 1..=n {
            let (iu, ju) = (i as u128, j as u128);
            let mag = (iu + ju - 1)
                * binom(nn + iu - 1, nn - ju)
                * binom(nn + ju - 1, nn - iu)
                * binom(iu + ju - 2, iu - 1).pow(2);
            debug_assert!(mag < (1u128 << 53));
            let v = mag as f64;
            d[(i - 1) + (j - 1) * n] = if (i + j) % 2 == 0 { v } else { -v };
        }
    }
    Ok(Matrix::from_col_major(n, n, d))
}

// ---------------------------------------------------------- predicates ----

/// `A == A'`. Exact when `tol == 0`; otherwise
/// `max|A_ij - A_ji| <= tol * max|A_ij|`. A non-square matrix is not
/// symmetric. A `NaN` entry never compares equal, so it is not symmetric.
pub fn is_symmetric(a: &Matrix, tol: f64) -> bool {
    let (r, c) = a.shape();
    if r != c {
        return false;
    }
    let mut maxdiff = 0.0f64;
    let mut maxabs = 0.0f64;
    for j in 0..c {
        for i in 0..r {
            let x = at(a, i, j);
            let y = at(a, j, i);
            if tol == 0.0 {
                if x != y {
                    return false;
                }
            } else {
                let d = (x - y).abs();
                if d.is_nan() {
                    return false;
                }
                maxdiff = maxdiff.max(d);
                maxabs = maxabs.max(x.abs());
            }
        }
    }
    tol == 0.0 || maxdiff <= tol * maxabs
}

/// Symmetric and positive definite: symmetric to within `10 n eps` of the
/// largest entry (a product like `B'*B` is symmetric only up to rounding) and
/// the Cholesky factorisation succeeds. Errors on an empty, non-square or
/// non-finite matrix.
pub fn is_posdef(a: &Matrix) -> Result<bool, String> {
    let (r, c) = a.shape();
    if a.is_empty() {
        return Err("isposdef: the matrix is empty".to_string());
    }
    if r != c {
        return Err(format!("isposdef: expected a square matrix, got {r}x{c}"));
    }
    if a.as_slice().iter().any(|v| !v.is_finite()) {
        return Err("isposdef: the matrix contains NaN or infinity".to_string());
    }
    if !is_symmetric(a, 10.0 * r as f64 * f64::EPSILON) {
        return Ok(false);
    }
    Ok(linalg::cholesky(a).is_ok())
}

pub fn is_diag(a: &Matrix) -> bool {
    bandwidth(a) == (0, 0)
}

/// Lower triangular: nothing nonzero above the diagonal.
pub fn is_tril(a: &Matrix) -> bool {
    bandwidth(a).1 == 0
}

/// Upper triangular: nothing nonzero below the diagonal.
pub fn is_triu(a: &Matrix) -> bool {
    bandwidth(a).0 == 0
}

/// `(lower, upper)`: the farthest nonzero below and above the diagonal
/// (`(0, 0)` for a diagonal or all-zero matrix). A `NaN` counts as nonzero.
pub fn bandwidth(a: &Matrix) -> (usize, usize) {
    let (r, c) = a.shape();
    let (mut lo, mut up) = (0usize, 0usize);
    for j in 0..c {
        for i in 0..r {
            if at(a, i, j) != 0.0 {
                if i > j {
                    lo = lo.max(i - j);
                } else {
                    up = up.max(j - i);
                }
            }
        }
    }
    (lo, up)
}

pub fn is_banded(a: &Matrix, lower: usize, upper: usize) -> bool {
    let (lo, up) = bandwidth(a);
    lo <= lower && up <= upper
}

// --------------------------------------------------------------- rcond ----

fn finite_square(f: &str, a: &Matrix) -> Result<usize, String> {
    let (r, c) = a.shape();
    if a.is_empty() {
        return Err(format!("{f}: the matrix is empty"));
    }
    if r != c {
        return Err(format!("{f}: expected a square matrix, got {r}x{c}"));
    }
    if a.as_slice().iter().any(|v| !v.is_finite()) {
        return Err(format!("{f}: the matrix contains NaN or infinity"));
    }
    Ok(r)
}

fn norm1_cols(d: &[f64], n: usize) -> f64 {
    dense_lu::norm1(d, n)
}

fn has_zero_pivot(lu: &DenseLu) -> bool {
    let n = lu.n;
    lu.singular || (0..n).any(|j| lu.lu[j + j * n] == 0.0)
}

/// `norm1(inv(A))` from the explicit inverse, `None` if `A` is singular.
pub fn inv_norm1_exact(a: &[f64], n: usize) -> Option<f64> {
    let lu = dense_lu::factor(a, n);
    if has_zero_pivot(&lu) {
        return None;
    }
    let mut ident = vec![0.0; n * n];
    for i in 0..n {
        ident[i + i * n] = 1.0;
    }
    let inv = lu.solve(&ident, n)?;
    if inv.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(norm1_cols(&inv, n))
}

/// Hager / Higham estimate of `norm1(inv(A))`: a lower bound, `None` if `A`
/// is singular. Needs solves with `A` and with `A'`, so both are factored.
pub fn inv_norm1_estimate(a: &[f64], n: usize) -> Option<f64> {
    if n < 2 {
        return inv_norm1_exact(a, n);
    }
    let lu = dense_lu::factor(a, n);
    if has_zero_pivot(&lu) {
        return None;
    }
    let mut at_ = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            at_[j + i * n] = a[i + j * n];
        }
    }
    let lut = dense_lu::factor(&at_, n);
    if has_zero_pivot(&lut) {
        return None;
    }
    let l1 = |v: &[f64]| v.iter().map(|x| x.abs()).sum::<f64>();
    let mut x = vec![1.0 / n as f64; n];
    let mut est = 0.0f64;
    for it in 0..5 {
        let y = lu.solve(&x, 1)?;
        if y.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let new = l1(&y);
        if it > 0 && new <= est {
            break;
        }
        est = new;
        let xi: Vec<f64> = y.iter().map(|&v| if v >= 0.0 { 1.0 } else { -1.0 }).collect();
        let z = lut.solve(&xi, 1)?;
        let (mut j, mut zmax) = (0usize, 0.0f64);
        for (k, &v) in z.iter().enumerate() {
            if v.abs() > zmax {
                zmax = v.abs();
                j = k;
            }
        }
        let ztx: f64 = z.iter().zip(&x).map(|(p, q)| p * q).sum();
        if zmax <= ztx {
            break;
        }
        x = vec![0.0; n];
        x[j] = 1.0;
    }
    // Higham's alternating-sign vector guards against the iteration
    // converging to a local maximum.
    let alt: Vec<f64> = (0..n)
        .map(|i| {
            let s = if i % 2 == 0 { 1.0 } else { -1.0 };
            s * (1.0 + i as f64 / (n - 1) as f64)
        })
        .collect();
    let y = lu.solve(&alt, 1)?;
    let est2 = 2.0 * l1(&y) / (3.0 * n as f64);
    let r = est.max(est2);
    if r.is_finite() {
        Some(r)
    } else {
        None
    }
}

/// Reciprocal condition number in the 1-norm, `1 / (norm1(A) * norm1(inv(A)))`;
/// `0` for a singular matrix (an exactly zero LU pivot). See the module
/// documentation for the exact and estimated paths.
pub fn rcond(a: &Matrix) -> Result<f64, String> {
    let n = finite_square("rcond", a)?;
    let na = norm1_cols(a.as_slice(), n);
    if na == 0.0 {
        return Ok(0.0);
    }
    let inv = if n <= RCOND_EXACT_MAX {
        inv_norm1_exact(a.as_slice(), n)
    } else {
        inv_norm1_estimate(a.as_slice(), n)
    };
    Ok(match inv {
        Some(v) if v > 0.0 && (na * v).is_finite() => 1.0 / (na * v),
        _ => 0.0,
    })
}

// --------------------------------------------------------------- norms ----

fn scaled_pnorm(x: &[f64], p: f64) -> f64 {
    let m = x.iter().fold(0.0f64, |acc, v| acc.max(v.abs()));
    if m == 0.0 || m.is_infinite() {
        return m;
    }
    let s: f64 = x.iter().map(|v| (v.abs() / m).powf(p)).sum();
    m * s.powf(1.0 / p)
}

/// Vector `p`-norm: `1`, `2`, `inf` (max), `-inf` (min), `0` (number of
/// nonzeros), any `p >= 1`, or `Fro` (the 2-norm). `NaN` input is an error.
pub fn vector_norm(x: &[f64], p: NormP) -> Result<f64, String> {
    if x.iter().any(|v| v.is_nan()) {
        return Err("norm: the input contains NaN".to_string());
    }
    let p = match p {
        NormP::Fro => 2.0,
        NormP::P(p) => p,
    };
    if p.is_nan() {
        return Err("norm: p is NaN".to_string());
    }
    if p == f64::NEG_INFINITY {
        return x
            .iter()
            .map(|v| v.abs())
            .reduce(f64::min)
            .ok_or_else(|| "norm: p = -inf of an empty vector is undefined".to_string());
    }
    if p == 0.0 {
        return Ok(x.iter().filter(|&&v| v != 0.0).count() as f64);
    }
    if p == f64::INFINITY {
        return Ok(x.iter().fold(0.0f64, |acc, v| acc.max(v.abs())));
    }
    if p == 1.0 {
        return Ok(x.iter().map(|v| v.abs()).sum());
    }
    if p >= 1.0 {
        return Ok(scaled_pnorm(x, p));
    }
    Err(format!("norm: a vector norm needs p = 0, p >= 1, inf or -inf, got {p}"))
}

/// Matrix norm: `1` (max column sum), `2` (largest singular value), `inf`
/// (max row sum) or `Fro`. `NaN` input is an error.
pub fn matrix_norm(a: &Matrix, p: NormP) -> Result<f64, String> {
    if a.as_slice().iter().any(|v| v.is_nan()) {
        return Err("norm: the matrix contains NaN".to_string());
    }
    let (r, c) = a.shape();
    match p {
        NormP::Fro => Ok(scaled_pnorm(a.as_slice(), 2.0)),
        NormP::P(p) if p == 1.0 => Ok((0..c)
            .map(|j| (0..r).map(|i| at(a, i, j).abs()).sum::<f64>())
            .fold(0.0, f64::max)),
        NormP::P(p) if p == f64::INFINITY => Ok((0..r)
            .map(|i| (0..c).map(|j| at(a, i, j).abs()).sum::<f64>())
            .fold(0.0, f64::max)),
        NormP::P(p) if p == 2.0 => {
            if a.is_empty() {
                return Ok(0.0);
            }
            let s = linalg::singular_values(a).map_err(|e: LinalgError| format!("norm: {e}"))?;
            Ok(s[0])
        }
        NormP::P(p) => Err(format!(
            "norm: a matrix norm needs p = 1, 2, inf or \"fro\", got {p}"
        )),
    }
}

/// Condition number in the `p`-norm: `2` (ratio of extreme singular values,
/// any shape), or `1`, `inf`, `Fro` (`norm(A, p) * norm(inv(A), p)`, square
/// only). Infinite for a singular matrix.
pub fn cond(a: &Matrix, p: NormP) -> Result<f64, String> {
    if a.as_slice().iter().any(|v| v.is_nan()) {
        return Err("cond: the matrix contains NaN".to_string());
    }
    if a.is_empty() {
        return Err("cond: the matrix is empty".to_string());
    }
    if p == NormP::P(2.0) {
        let s = linalg::singular_values(a).map_err(|e| format!("cond: {e}"))?;
        let (hi, lo) = (s[0], *s.last().unwrap());
        return Ok(if lo > 0.0 { hi / lo } else { f64::INFINITY });
    }
    let ok = matches!(p, NormP::Fro) || p == NormP::P(1.0) || p == NormP::P(f64::INFINITY);
    if !ok {
        let NormP::P(q) = p else { unreachable!() };
        return Err(format!("cond: p must be 1, 2, inf or \"fro\", got {q}"));
    }
    let n = finite_square("cond", a)?;
    let lu = dense_lu::factor(a.as_slice(), n);
    if has_zero_pivot(&lu) {
        return Ok(f64::INFINITY);
    }
    let mut ident = vec![0.0; n * n];
    for i in 0..n {
        ident[i + i * n] = 1.0;
    }
    let inv = match lu.solve(&ident, n) {
        Some(v) if v.iter().all(|x| x.is_finite()) => v,
        _ => return Ok(f64::INFINITY),
    };
    let inv = Matrix::from_col_major(n, n, inv);
    Ok(matrix_norm(a, p)? * matrix_norm(&inv, p)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(rows: &[&[f64]]) -> Matrix {
        let r: Vec<Vec<f64>> = rows.iter().map(|x| x.to_vec()).collect();
        Matrix::from_rows(&r).unwrap()
    }

    fn rnd(n: usize, seed: u64) -> Matrix {
        let mut s = seed;
        let d = (0..n * n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect();
        Matrix::from_col_major(n, n, d)
    }

    #[test]
    fn eps_spacing_matches_the_power_of_two_definition() {
        assert_eq!(eps_at(1.0), 2.0f64.powi(-52));
        assert_eq!(eps_at(1.0), 2.220446049250313e-16);
        assert_eq!(eps_at(f64::EPSILON), 2.0f64.powi(-104));
        // 2^33 <= 1e10 < 2^34, so the spacing is 2^(33-52)
        assert_eq!(eps_at(1e10), 2.0f64.powi(-19));
        assert_eq!(eps_at(-1e10), 2.0f64.powi(-19));
        assert_eq!(eps_at(0.5), 2.0f64.powi(-53));
        assert_eq!(eps_at(9007199254740992.0), 2.0); // 2^53
        assert_eq!(eps_at(0.0), f64::from_bits(1));
        assert_eq!(eps_at(0.0), 5e-324);
        assert_eq!(eps_at(f64::MAX), 2.0f64.powi(971));
        assert!(eps_at(f64::INFINITY).is_nan() && eps_at(f64::NAN).is_nan());
        // the defining property: x + eps(x) is the next double, x + eps(x)/4 rounds back
        for &x in &[1.0, 3.7, 1e-5, 123456.789] {
            assert!(x + eps_at(x) > x);
            assert_eq!(x + eps_at(x) / 4.0, x);
        }
    }

    #[test]
    fn hilb_entries_and_limits() {
        let h = hilb(3).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(at(&h, i, j), 1.0 / (i + j + 1) as f64);
            }
        }
        assert_eq!(at(&h, 2, 2), 0.2);
        assert!(hilb(0).is_err() && hilb(HILB_MAX + 1).is_err());
    }

    #[test]
    fn invhilb_known_entries_and_exact_identities() {
        let w = invhilb(4).unwrap();
        assert_eq!(
            (0..4).map(|j| at(&w, 0, j)).collect::<Vec<_>>(),
            vec![16.0, -120.0, 240.0, -140.0]
        );
        assert_eq!(at(&w, 1, 1), 1200.0);
        assert_eq!(at(&w, 3, 3), 2800.0);
        assert_eq!(at(&w, 1, 3), 1680.0);
        assert_eq!(at(&w, 2, 2), 6480.0);
        // inv(hilb(n)) times hilb(n) is the identity to the rounding of the product
        let (h, w) = (hilb(4).unwrap(), invhilb(4).unwrap());
        let p = w.matmul(&h).unwrap();
        for i in 0..4 {
            for j in 0..4 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((at(&p, i, j) - want).abs() < 1e-11, "{i} {j}");
            }
        }
        // For every supported n: symmetric integer entries whose sum is n^2
        // (a classical identity for the inverse Hilbert matrix), no rounding.
        for n in 1..=INVHILB_MAX {
            let w = invhilb(n).unwrap();
            assert!(is_symmetric(&w, 0.0));
            let mut sum: i128 = 0;
            for &v in w.as_slice() {
                assert_eq!(v, v.trunc());
                assert!(v.abs() < 9007199254740992.0);
                sum += v as i128;
            }
            assert_eq!(sum, (n * n) as i128, "n = {n}");
        }
        assert!(invhilb(13).is_err() && invhilb(0).is_err());
    }

    #[test]
    fn symmetry_predicates() {
        let s = m(&[&[2.0, 1.0], &[1.0, 3.0]]);
        assert!(is_symmetric(&s, 0.0));
        let near = m(&[&[2.0, 1.0], &[1.0 + 1e-9, 3.0]]);
        assert!(!is_symmetric(&near, 0.0));
        assert!(is_symmetric(&near, 1e-6));
        assert!(!is_symmetric(&near, 1e-12));
        assert!(!is_symmetric(&m(&[&[1.0, 2.0, 3.0], &[2.0, 1.0, 0.0]]), 0.0));
        assert!(!is_symmetric(&m(&[&[f64::NAN]]), 0.0));
        assert!(is_symmetric(&Matrix::zeros(3, 3), 1e-3));
        assert!(is_posdef(&s).unwrap());
        // symmetric indefinite: eigenvalues 3 and -1
        assert!(!is_posdef(&m(&[&[1.0, 2.0], &[2.0, 1.0]])).unwrap());
        // not symmetric at all, though its lower triangle alone would pass Cholesky
        assert!(!is_posdef(&m(&[&[2.0, 5.0], &[1.0, 3.0]])).unwrap());
        assert!(!is_posdef(&near).unwrap());
        // singular PSD is not positive definite
        assert!(!is_posdef(&m(&[&[1.0, 1.0], &[1.0, 1.0]])).unwrap());
        // B'B is symmetric only to rounding, and positive definite
        let b = rnd(6, 7);
        let g = b.transpose().matmul(&b).unwrap();
        assert!(is_posdef(&g).unwrap());
        assert!(is_posdef(&hilb(5).unwrap()).unwrap());
        assert!(is_posdef(&m(&[&[1.0, 2.0, 3.0], &[2.0, 1.0, 0.0]])).is_err());
        assert!(is_posdef(&m(&[&[f64::NAN]])).is_err());
    }

    #[test]
    fn structure_predicates_and_bandwidth() {
        let tri = m(&[
            &[2.0, -1.0, 0.0, 0.0],
            &[-1.0, 2.0, -1.0, 0.0],
            &[0.0, -1.0, 2.0, -1.0],
            &[0.0, 0.0, -1.0, 2.0],
        ]);
        assert_eq!(bandwidth(&tri), (1, 1));
        assert!(is_banded(&tri, 1, 1) && is_banded(&tri, 2, 3));
        assert!(!is_banded(&tri, 0, 1) && !is_banded(&tri, 1, 0));
        assert!(!is_diag(&tri) && !is_tril(&tri) && !is_triu(&tri));
        let full = m(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0], &[7.0, 8.0, 9.0]]);
        assert_eq!(bandwidth(&full), (2, 2));
        assert!(is_banded(&full, 2, 2) && !is_banded(&full, 1, 2));
        let up = m(&[&[1.0, 2.0], &[0.0, 3.0]]);
        let lo = up.transpose();
        assert!(is_triu(&up) && !is_tril(&up) && is_tril(&lo) && !is_triu(&lo));
        assert_eq!(bandwidth(&up), (0, 1));
        assert_eq!(bandwidth(&lo), (1, 0));
        assert!(is_diag(&m(&[&[1.0, 0.0], &[0.0, 5.0]])));
        assert!(is_diag(&Matrix::zeros(2, 2)) && is_tril(&Matrix::zeros(2, 2)));
        assert_eq!(bandwidth(&Matrix::zeros(2, 2)), (0, 0));
        // rectangular: a 2x3 with a nonzero at (0, 2) has upper bandwidth 2
        let rect = m(&[&[1.0, 0.0, 4.0], &[0.0, 2.0, 0.0]]);
        assert_eq!(bandwidth(&rect), (0, 2));
        assert!(is_triu(&rect) && !is_diag(&rect));
        // NaN counts as nonzero
        assert!(!is_diag(&m(&[&[1.0, f64::NAN], &[0.0, 1.0]])));
    }

    #[test]
    fn rcond_references() {
        assert_eq!(rcond(&Matrix::eye(5)).unwrap(), 1.0);
        let d = m(&[&[1.0, 0.0], &[0.0, 10.0]]);
        assert!((rcond(&d).unwrap() - 0.1).abs() < 1e-16);
        // singular: exact zero pivot, and an all-zero matrix
        assert_eq!(rcond(&m(&[&[1.0, 2.0], &[2.0, 4.0]])).unwrap(), 0.0);
        assert_eq!(rcond(&Matrix::zeros(3, 3)).unwrap(), 0.0);
        // hilb(8): the reference uses the exact integer inverse, not an LU
        for n in [3usize, 5, 8] {
            let h = hilb(n).unwrap();
            let w = invhilb(n).unwrap();
            let reference = 1.0
                / (matrix_norm(&h, NormP::P(1.0)).unwrap() * matrix_norm(&w, NormP::P(1.0)).unwrap());
            let got = rcond(&h).unwrap();
            assert!((got - reference).abs() <= 1e-3 * reference, "n = {n}: {got} vs {reference}");
        }
        assert!(rcond(&Matrix::zeros(2, 3)).is_err());
        assert!(rcond(&m(&[&[f64::NAN]])).is_err());
        assert!(rcond(&m(&[&[f64::INFINITY]])).is_err());
    }

    #[test]
    fn estimator_is_a_lower_bound_within_a_factor_of_three() {
        let mut worst = 1.0f64;
        let mut cases: Vec<Matrix> = (0..60).map(|k| rnd(4 + 5 * (k % 14), 100 + k as u64)).collect();
        cases.push(hilb(8).unwrap());
        cases.push(hilb(11).unwrap());
        for a in &cases {
            let n = a.rows();
            let exact = inv_norm1_exact(a.as_slice(), n).unwrap();
            let est = inv_norm1_estimate(a.as_slice(), n).unwrap();
            assert!(est <= exact * (1.0 + 1e-9), "estimate above the true value: {est} > {exact}");
            assert!(est * 3.0 >= exact, "n = {n}: estimate {est} vs exact {exact}");
            worst = worst.max(exact / est);
        }
        eprintln!("worst exact/estimate ratio: {worst}");
        // large matrix: the public rcond takes the estimator path
        let n = RCOND_EXACT_MAX + 40;
        let mut a = rnd(n, 9);
        let mut d = a.as_slice().to_vec();
        for i in 0..n {
            d[i + i * n] += 3.0;
        }
        a = Matrix::from_col_major(n, n, d);
        let exact = 1.0 / (norm1_cols(a.as_slice(), n) * inv_norm1_exact(a.as_slice(), n).unwrap());
        let got = rcond(&a).unwrap();
        assert!(got >= exact * (1.0 - 1e-9) && got <= 3.0 * exact, "{got} vs {exact}");
        // singular large matrix: zero column
        let mut d = a.as_slice().to_vec();
        for i in 0..n {
            d[i + 5 * n] = 0.0;
        }
        assert_eq!(rcond(&Matrix::from_col_major(n, n, d)).unwrap(), 0.0);
    }

    #[test]
    fn singular_values_agree_with_svd() {
        let d = m(&[&[3.0, 0.0, 0.0], &[0.0, 2.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_eq!(linalg::singular_values(&d).unwrap(), vec![3.0, 2.0, 1.0]);
        for (n, seed) in [(5usize, 1u64), (17, 2), (130, 3)] {
            let a = rnd(n, seed);
            let s = linalg::singular_values(&a).unwrap();
            let f = linalg::svd(&a).unwrap().singular_values;
            assert_eq!(s.len(), f.len());
            for (x, y) in s.iter().zip(&f) {
                assert!((x - y).abs() <= 1e-12 * f[0], "{x} vs {y}");
            }
            assert!(s.windows(2).all(|w| w[0] >= w[1]));
        }
        assert!(linalg::singular_values(&m(&[&[f64::NAN]])).is_err());
    }

    #[test]
    fn matrix_and_vector_norms() {
        let a = m(&[&[1.0, 2.0], &[3.0, 4.0]]);
        assert_eq!(matrix_norm(&a, NormP::P(1.0)).unwrap(), 6.0);
        assert_eq!(matrix_norm(&a, NormP::P(f64::INFINITY)).unwrap(), 7.0);
        assert!((matrix_norm(&a, NormP::Fro).unwrap() - 30.0f64.sqrt()).abs() < 1e-15);
        // the 2-norm of a 2x2 in closed form: sqrt((F + sqrt(F^2 - 4 det^2)) / 2), F = ||A||_F^2
        let closed = ((30.0 + (900.0f64 - 16.0).sqrt()) / 2.0).sqrt();
        let two = matrix_norm(&a, NormP::P(2.0)).unwrap();
        assert!((two - closed).abs() < 1e-14 && (two - 5.464985704219043).abs() < 1e-14, "{two}");
        assert!(matrix_norm(&a, NormP::P(3.0)).is_err());
        assert!(matrix_norm(&m(&[&[f64::NAN, 1.0], &[1.0, 1.0]]), NormP::P(1.0)).is_err());

        let v = [3.0, -4.0, 0.0, 12.0];
        assert_eq!(vector_norm(&v, NormP::P(1.0)).unwrap(), 19.0);
        assert_eq!(vector_norm(&v, NormP::P(2.0)).unwrap(), 13.0);
        assert_eq!(vector_norm(&v, NormP::Fro).unwrap(), 13.0);
        assert_eq!(vector_norm(&v, NormP::P(f64::INFINITY)).unwrap(), 12.0);
        assert_eq!(vector_norm(&v, NormP::P(f64::NEG_INFINITY)).unwrap(), 0.0);
        assert_eq!(vector_norm(&v, NormP::P(0.0)).unwrap(), 3.0);
        let p3 = vector_norm(&v, NormP::P(3.0)).unwrap();
        assert!((p3 - (27.0f64 + 64.0 + 1728.0).powf(1.0 / 3.0)).abs() < 1e-12);
        assert!(vector_norm(&v, NormP::P(0.5)).is_err());
        assert!(vector_norm(&v, NormP::P(-1.0)).is_err());
        assert!(vector_norm(&[1.0, f64::NAN], NormP::P(2.0)).is_err());
        // overflow-safe: naive squares of 1e200 overflow
        let big = vector_norm(&[3e200, 4e200], NormP::P(2.0)).unwrap();
        assert!((big - 5e200).abs() < 1e186);
    }

    #[test]
    fn condition_numbers() {
        let a = m(&[&[1.0, 2.0], &[3.0, 4.0]]);
        // inv(A) = [-2 1; 1.5 -0.5]: norm1 3.5, norm_inf 3, fro sqrt(7.5)
        assert!((cond(&a, NormP::P(1.0)).unwrap() - 21.0).abs() < 1e-12);
        assert!((cond(&a, NormP::P(f64::INFINITY)).unwrap() - 21.0).abs() < 1e-12);
        assert!((cond(&a, NormP::Fro).unwrap() - 15.0).abs() < 1e-12);
        let s = linalg::svd(&a).unwrap().singular_values;
        assert!((cond(&a, NormP::P(2.0)).unwrap() - s[0] / s[1]).abs() < 1e-12);
        let sing = m(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert_eq!(cond(&sing, NormP::P(1.0)).unwrap(), f64::INFINITY);
        assert!(cond(&m(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]), NormP::P(1.0)).is_err());
        assert!(cond(&m(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]), NormP::P(2.0)).is_ok());
        assert!(cond(&a, NormP::P(3.0)).is_err());
        assert!(cond(&m(&[&[f64::NAN]]), NormP::P(1.0)).is_err());
        // consistency with rcond: cond_1 = 1 / rcond
        let h = hilb(6).unwrap();
        let c1 = cond(&h, NormP::P(1.0)).unwrap();
        assert!((c1 * rcond(&h).unwrap() - 1.0).abs() < 1e-9);
    }
}
