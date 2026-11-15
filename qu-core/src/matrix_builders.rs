//! Structured-matrix builders ([`toeplitz`], [`hankel`], [`vander`]), the
//! SVD-based subspace bases ([`null_space`], [`orth`]) and the numerical
//! derivative of sampled data ([`gradient`]).
//!
//! Errors are plain messages (`Result<_, String>`).

use crate::linalg;
use crate::matrix::Matrix;

/// Toeplitz matrix with first column `c` and first row `r` (`r[0]` is
/// ignored in favour of `c[0]`, as MATLAB does). `None` for `r` means the
/// symmetric Toeplitz matrix of `c`.
pub fn toeplitz(c: &[f64], r: Option<&[f64]>) -> Result<Matrix, String> {
    let r = r.unwrap_or(c);
    if c.is_empty() || r.is_empty() {
        return Err("toeplitz: the column and row vectors must be non-empty".to_string());
    }
    let (m, n) = (c.len(), r.len());
    let mut d = vec![0.0; m * n];
    for j in 0..n {
        for i in 0..m {
            d[i + j * m] = if i >= j { c[i - j] } else { r[j - i] };
        }
    }
    Ok(Matrix::from_col_major(m, n, d))
}

/// Hankel matrix with first column `c`; below the anti-diagonal it is filled
/// from `r` (the last row, `r[0]` ignored in favour of `c`'s last element), or
/// with zeros when `r` is `None`.
pub fn hankel(c: &[f64], r: Option<&[f64]>) -> Result<Matrix, String> {
    if c.is_empty() {
        return Err("hankel: the first-column vector must be non-empty".to_string());
    }
    let m = c.len();
    let n = match r {
        Some(r) if r.is_empty() => {
            return Err("hankel: the last-row vector must be non-empty".to_string())
        }
        Some(r) => r.len(),
        None => m,
    };
    let mut d = vec![0.0; m * n];
    for j in 0..n {
        for i in 0..m {
            let k = i + j;
            d[i + j * m] = if k < m {
                c[k]
            } else {
                match r {
                    Some(r) => r[k - m + 1],
                    None => 0.0,
                }
            };
        }
    }
    Ok(Matrix::from_col_major(m, n, d))
}

/// Vandermonde matrix: row `i` is `[v_i^(n-1), ..., v_i, 1]`; `n` defaults to
/// `len(v)`. Powers are built by repeated multiplication from the right, so
/// small-integer inputs give exact entries and `0^0 = 1`.
pub fn vander(v: &[f64], n: Option<usize>) -> Result<Matrix, String> {
    let m = v.len();
    let n = n.unwrap_or(m);
    if m == 0 {
        return Err("vander: the input vector is empty".to_string());
    }
    if n == 0 {
        return Err("vander: the number of columns must be at least 1".to_string());
    }
    let mut d = vec![0.0; m * n];
    for i in 0..m {
        let mut p = 1.0;
        for j in (0..n).rev() {
            d[i + j * m] = p;
            p *= v[i];
        }
    }
    Ok(Matrix::from_col_major(m, n, d))
}

/// The two bases an SVD gives: `(left_basis, null_basis, rank)`.
struct Subspaces {
    orth: Matrix,
    null: Matrix,
}

fn subspaces(a: &Matrix, who: &str) -> Result<Subspaces, String> {
    if a.is_empty() {
        return Err(format!("{who}: the matrix is empty"));
    }
    if a.as_slice().iter().any(|v| !v.is_finite()) {
        return Err(format!("{who}: the matrix must be finite"));
    }
    let (m, n) = a.shape();
    // Pad with zero rows to at least n rows: that adds nothing to the singular
    // values or the right singular vectors, and makes the thin SVD return a
    // FULL n x n V, whose trailing columns span the null space when m < n.
    let mm = m.max(n);
    let mut data = vec![0.0; mm * n];
    for j in 0..n {
        for i in 0..m {
            data[i + j * mm] = a.as_slice()[i + j * m];
        }
    }
    let padded = Matrix::from_col_major(mm, n, data);
    let svd = linalg::svd(&padded).map_err(|e| format!("{who}: {e}"))?;
    let s = &svd.singular_values;
    let smax = s.iter().cloned().fold(0.0f64, f64::max);
    let tol = m.max(n) as f64 * f64::EPSILON * smax;
    let rank = s.iter().filter(|&&x| x > tol).count();
    // v_t is (k, n) with k = n here: row i is right singular vector i.
    let vt = &svd.v_t;
    let k = vt.rows();
    let mut nd = Vec::with_capacity(n * (n - rank));
    for i in rank..n.min(k) {
        for j in 0..n {
            nd.push(vt.as_slice()[i + j * k]);
        }
    }
    let null = Matrix::from_col_major(n, nd.len() / n.max(1), nd);
    // U is (mm, k); keep the first `rank` columns and the first m rows (the
    // padded rows of those columns are zero: sigma > 0 forces u = A v / sigma).
    let ur = svd.u.rows();
    let mut od = Vec::with_capacity(m * rank);
    for j in 0..rank {
        for i in 0..m {
            od.push(svd.u.as_slice()[i + j * ur]);
        }
    }
    let orth = Matrix::from_col_major(m, rank, od);
    Ok(Subspaces { orth, null })
}

/// Orthonormal basis of the null space of `A` (`n x (n - rank)`), from the
/// SVD with MATLAB's rank tolerance `max(size(A)) * eps * sigma_max`. The
/// matrix has zero columns when `A` has full column rank.
pub fn null_space(a: &Matrix) -> Result<Matrix, String> {
    subspaces(a, "null").map(|s| s.null)
}

/// Orthonormal basis of the range of `A` (`m x rank`), same tolerance.
pub fn orth(a: &Matrix) -> Result<Matrix, String> {
    subspaces(a, "orth").map(|s| s.orth)
}

/// Numerical gradient of samples `v` at unit spacing, uniform spacing `h`, or
/// the coordinates `x` (one per sample): central differences in the interior
/// (`(v[i+1]-v[i-1]) / (x[i+1]-x[i-1])`, exact for quadratics on a uniform
/// grid) and one-sided differences at the two ends.
pub fn gradient(v: &[f64], spacing: Spacing) -> Result<Vec<f64>, String> {
    let n = v.len();
    if n < 2 {
        return Err("gradient: needs at least two samples".to_string());
    }
    let x_at = |i: usize| -> f64 {
        match &spacing {
            Spacing::Uniform(h) => *h * i as f64,
            Spacing::Coords(x) => x[i],
        }
    };
    match &spacing {
        Spacing::Uniform(h) => {
            if !(h.is_finite() && *h != 0.0) {
                return Err("gradient: the spacing must be finite and non-zero".to_string());
            }
        }
        Spacing::Coords(x) => {
            if x.len() != n {
                return Err(format!(
                    "gradient: the coordinate vector has {} entries but there are {n} samples",
                    x.len()
                ));
            }
            if x.windows(2).any(|w| !(w[1] != w[0]) || !w[0].is_finite() || !w[1].is_finite()) {
                return Err(
                    "gradient: coordinates must be finite and consecutive values distinct"
                        .to_string(),
                );
            }
        }
    }
    let mut g = vec![0.0; n];
    g[0] = (v[1] - v[0]) / (x_at(1) - x_at(0));
    g[n - 1] = (v[n - 1] - v[n - 2]) / (x_at(n - 1) - x_at(n - 2));
    for i in 1..n - 1 {
        g[i] = (v[i + 1] - v[i - 1]) / (x_at(i + 1) - x_at(i - 1));
    }
    Ok(g)
}

/// Sample spacing for [`gradient`].
pub enum Spacing {
    Uniform(f64),
    Coords(Vec<f64>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(m: &Matrix) -> Vec<Vec<f64>> {
        (0..m.rows()).map(|i| m.row_vec(i).unwrap()).collect()
    }

    #[test]
    fn toeplitz_cases() {
        let t = toeplitz(&[1.0, 2.0, 3.0], None).unwrap();
        assert_eq!(rows(&t), vec![vec![1.0, 2.0, 3.0], vec![2.0, 1.0, 2.0], vec![3.0, 2.0, 1.0]]);
        let t = toeplitz(&[1.0, 2.0, 3.0], Some(&[1.0, 4.0, 5.0, 6.0])).unwrap();
        assert_eq!(
            rows(&t),
            vec![vec![1.0, 4.0, 5.0, 6.0], vec![2.0, 1.0, 4.0, 5.0], vec![3.0, 2.0, 1.0, 4.0]]
        );
        assert!(toeplitz(&[], None).is_err());
    }

    #[test]
    fn hankel_cases() {
        let h = hankel(&[1.0, 2.0, 3.0], None).unwrap();
        assert_eq!(rows(&h), vec![vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 0.0], vec![3.0, 0.0, 0.0]]);
        let h = hankel(&[1.0, 2.0, 3.0], Some(&[3.0, 7.0, 8.0, 9.0])).unwrap();
        assert_eq!(
            rows(&h),
            vec![vec![1.0, 2.0, 3.0, 7.0], vec![2.0, 3.0, 7.0, 8.0], vec![3.0, 7.0, 8.0, 9.0]]
        );
    }

    #[test]
    fn vander_cases() {
        let v = vander(&[1.0, 2.0, 3.0], None).unwrap();
        assert_eq!(rows(&v), vec![vec![1.0, 1.0, 1.0], vec![4.0, 2.0, 1.0], vec![9.0, 3.0, 1.0]]);
        let v = vander(&[0.0, 2.0], Some(4)).unwrap();
        assert_eq!(rows(&v), vec![vec![0.0, 0.0, 0.0, 1.0], vec![8.0, 4.0, 2.0, 1.0]]);
        assert!(vander(&[], None).is_err());
    }

    fn max_abs(m: &Matrix) -> f64 {
        m.as_slice().iter().fold(0.0f64, |a, b| a.max(b.abs()))
    }

    fn check_orthonormal(q: &Matrix) {
        let g = q.transpose().matmul(q).unwrap();
        for i in 0..g.rows() {
            for j in 0..g.cols() {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((g.get(i, j).unwrap() - want).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn null_and_orth_wide_rank_deficient() {
        // 2 x 4, rank 2 -> null space has dimension 2
        let a = Matrix::from_rows(&[vec![1.0, 2.0, 3.0, 4.0], vec![2.0, 4.0, 6.0, 8.5]]).unwrap();
        let z = null_space(&a).unwrap();
        // rank is 2 (second row is not a multiple): null dim = 2
        assert_eq!(z.shape(), (4, 2));
        assert!(max_abs(&a.matmul(&z).unwrap()) < 1e-12);
        check_orthonormal(&z);
        let o = orth(&a).unwrap();
        assert_eq!(o.shape(), (2, 2));
        check_orthonormal(&o);
    }

    #[test]
    fn null_and_orth_rank_one() {
        let a = Matrix::from_rows(&[vec![1.0, 2.0], vec![2.0, 4.0], vec![3.0, 6.0]]).unwrap();
        let z = null_space(&a).unwrap();
        assert_eq!(z.shape(), (2, 1));
        assert!(max_abs(&a.matmul(&z).unwrap()) < 1e-12);
        check_orthonormal(&z);
        let o = orth(&a).unwrap();
        assert_eq!(o.shape(), (3, 1));
        check_orthonormal(&o);
        // range of A is spanned by [1 2 3]/sqrt(14)
        let s = 14.0f64.sqrt();
        let dot = (o.get(0, 0).unwrap() + 2.0 * o.get(1, 0).unwrap() + 3.0 * o.get(2, 0).unwrap()) / s;
        assert!((dot.abs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn full_rank_has_empty_null_space() {
        let a = Matrix::from_rows(&[vec![2.0, 0.0], vec![0.0, 3.0]]).unwrap();
        assert_eq!(null_space(&a).unwrap().shape(), (2, 0));
        assert_eq!(orth(&a).unwrap().shape(), (2, 2));
        let zero = Matrix::zeros(2, 3);
        assert_eq!(null_space(&zero).unwrap().shape(), (3, 3));
        assert_eq!(orth(&zero).unwrap().shape(), (2, 0));
    }

    #[test]
    fn gradient_cases() {
        let x: Vec<f64> = (0..11).map(|i| i as f64 * 0.1).collect();
        let v: Vec<f64> = x.iter().map(|t| t * t).collect();
        let g = gradient(&v, Spacing::Uniform(0.1)).unwrap();
        for i in 1..10 {
            assert!((g[i] - 2.0 * x[i]).abs() < 1e-12, "i = {i}");
        }
        // one-sided ends
        assert!((g[0] - 0.1).abs() < 1e-12);
        assert!((g[10] - 1.9).abs() < 1e-12);
        let g2 = gradient(&v, Spacing::Coords(x.clone())).unwrap();
        for i in 0..11 {
            assert!((g2[i] - g[i]).abs() < 1e-9);
        }
        // non-uniform coordinates, linear data: exact slope everywhere
        let xs = vec![0.0, 0.5, 2.0, 2.25, 5.0];
        let lin: Vec<f64> = xs.iter().map(|t| 3.0 * t + 1.0).collect();
        let g = gradient(&lin, Spacing::Coords(xs)).unwrap();
        assert!(g.iter().all(|s| (s - 3.0).abs() < 1e-12));
        assert!(gradient(&[1.0], Spacing::Uniform(1.0)).is_err());
        assert!(gradient(&[1.0, 2.0], Spacing::Uniform(0.0)).is_err());
        assert!(gradient(&[1.0, 2.0], Spacing::Coords(vec![1.0])).is_err());
    }
}
