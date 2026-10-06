//! Blocked dense LU with partial pivoting, plus blocked solves and inverse.
//!
//! Written for v0.4.9 because the previous path (`nalgebra`'s unblocked LU,
//! and an SVD-based `pinv` for `inv`) is memory-bound: at n = 1000 `inv`
//! took 2.1 s and `lu`/`solve` 0.09 s. Here the O(n^3) work is a sequence
//! of panel factorisations (unblocked, `NB` columns wide) whose trailing
//! updates and triangular solves are expressed as [`Matrix::matmul`] calls,
//! which run on the SIMD/rayon GEMM kernel.
//!
//! Pivoting is the same rule as `nalgebra` and LAPACK `getrf`: the first
//! entry of maximal absolute value in the column, rows swapped across the
//! whole matrix. Everything is safe, plain-slice Rust, column-major.

use crate::matrix::Matrix;

/// Panel width. 64 keeps the unblocked panel (O(n * NB^2) work per panel)
/// small against the GEMM update while the update still has depth 64.
const NB: usize = 64;

/// A computed `P A = L U` of a square matrix, packed LAPACK-style: the strict
/// lower triangle of `lu` is `L` (unit diagonal implied), the upper triangle
/// including the diagonal is `U`.
pub struct DenseLu {
    pub n: usize,
    /// Column-major `n x n` packed factors.
    pub lu: Vec<f64>,
    /// `piv[j]`: at step `j` row `j` was swapped with row `piv[j]`.
    pub piv: Vec<usize>,
    /// `true` if any pivot was exactly zero (the factorisation still ran to
    /// completion, skipping the elimination of that column, as `nalgebra`'s does).
    pub singular: bool,
}

/// Factorise the square column-major matrix `a` (`n x n`).
pub fn factor(a: &[f64], n: usize) -> DenseLu {
    assert_eq!(a.len(), n * n);
    let mut a = a.to_vec();
    let mut piv = vec![0usize; n];
    let mut singular = false;
    let mut k = 0;
    while k < n {
        let b = NB.min(n - k);
        // ---- panel: columns k..k+b, unblocked, swaps applied to every column.
        for j in k..k + b {
            let mut p = j;
            let mut max = a[j + j * n].abs();
            for i in j + 1..n {
                let v = a[i + j * n].abs();
                if v > max {
                    max = v;
                    p = i;
                }
            }
            piv[j] = p;
            if p != j {
                for c in 0..n {
                    a.swap(j + c * n, p + c * n);
                }
            }
            let d = a[j + j * n];
            if d == 0.0 {
                singular = true;
                continue;
            }
            let (left, right) = a.split_at_mut((j + 1) * n);
            let colj = &mut left[j * n..(j + 1) * n];
            for v in colj[j + 1..].iter_mut() {
                *v /= d;
            }
            let colj = &*colj;
            for c in j + 1..k + b {
                let colc = &mut right[(c - j - 1) * n..(c - j) * n];
                let f = colc[j];
                if f != 0.0 {
                    for i in j + 1..n {
                        colc[i] -= colj[i] * f;
                    }
                }
            }
        }
        let e = k + b;
        if e < n {
            // ---- U12 = L11^-1 A12 (unit lower, rows k..e, columns e..n).
            for c in e..n {
                for j in k..e {
                    let f = a[j + c * n];
                    if f != 0.0 {
                        for i in j + 1..e {
                            a[i + c * n] -= a[i + j * n] * f;
                        }
                    }
                }
            }
            // ---- A22 -= L21 * U12 through the GEMM kernel.
            let m2 = n - e;
            let mut l21 = Vec::with_capacity(m2 * b);
            for j in k..e {
                l21.extend_from_slice(&a[e + j * n..(j + 1) * n]);
            }
            let mut u12 = Vec::with_capacity(b * m2);
            for c in e..n {
                u12.extend_from_slice(&a[k + c * n..e + c * n]);
            }
            let prod = Matrix::from_col_major(m2, b, l21)
                .matmul(&Matrix::from_col_major(b, m2, u12))
                .expect("shapes agree by construction");
            let p = prod.as_slice();
            for c in 0..m2 {
                let dst = &mut a[e + (e + c) * n..(e + c + 1) * n];
                let src = &p[c * m2..(c + 1) * m2];
                for (d, s) in dst.iter_mut().zip(src) {
                    *d -= *s;
                }
            }
        }
        k = e;
    }
    DenseLu { n, lu: a, piv, singular }
}

impl DenseLu {
    /// Determinant: sign of the permutation times the product of `diag(U)`.
    pub fn det(&self) -> f64 {
        let n = self.n;
        let mut d = 1.0;
        for j in 0..n {
            d *= self.lu[j + j * n];
            if self.piv[j] != j {
                d = -d;
            }
        }
        d
    }

    /// Row permutation as a vector: row `i` of `P A` is row `perm[i]` of `A`.
    pub fn permutation(&self) -> Vec<usize> {
        let mut perm: Vec<usize> = (0..self.n).collect();
        for (j, &p) in self.piv.iter().enumerate() {
            perm.swap(j, p);
        }
        perm
    }

    /// Solve `A X = B` for the column-major `n x m` block `b`. `None` when `U`
    /// has an exactly zero pivot (the same condition `nalgebra`'s `solve`
    /// reports).
    pub fn solve(&self, b: &[f64], m: usize) -> Option<Vec<f64>> {
        let n = self.n;
        assert_eq!(b.len(), n * m);
        if self.singular || (0..n).any(|j| self.lu[j + j * n] == 0.0) {
            return None;
        }
        let mut x = b.to_vec();
        // Apply P to the right-hand side, in the order the swaps were made.
        for (j, &p) in self.piv.iter().enumerate() {
            if p != j {
                for c in 0..m {
                    x.swap(j + c * n, p + c * n);
                }
            }
        }
        let lu = &self.lu;
        // ---- forward: L y = Pb (unit lower), block by block.
        let mut k = 0;
        while k < n {
            let bb = NB.min(n - k);
            let e = k + bb;
            for c in 0..m {
                let col = &mut x[c * n..(c + 1) * n];
                for j in k..e {
                    let f = col[j];
                    if f != 0.0 {
                        for i in j + 1..e {
                            col[i] -= lu[i + j * n] * f;
                        }
                    }
                }
            }
            if e < n {
                self.gemm_sub(&mut x, m, e, n, k, e);
            }
            k = e;
        }
        // ---- backward: U x = y, blocks from the last one up.
        let mut e = n;
        while e > 0 {
            let bb = NB.min(e);
            let k = e - bb;
            for c in 0..m {
                let col = &mut x[c * n..(c + 1) * n];
                for j in (k..e).rev() {
                    col[j] /= lu[j + j * n];
                    let f = col[j];
                    if f != 0.0 {
                        for i in k..j {
                            col[i] -= lu[i + j * n] * f;
                        }
                    }
                }
            }
            if k > 0 {
                self.gemm_sub(&mut x, m, 0, k, k, e);
            }
            e = k;
        }
        Some(x)
    }

    /// `x[r0..r1, :] -= LU[r0..r1, c0..c1] * x[c0..c1, :]` (the strictly
    /// lower part of the packed factor for the forward sweep, the upper part
    /// for the backward one — in both cases the referenced block lies wholly
    /// on one side of the diagonal, so no masking is needed).
    fn gemm_sub(&self, x: &mut [f64], m: usize, r0: usize, r1: usize, c0: usize, c1: usize) {
        let n = self.n;
        // For the forward sweep the target rows are below the solved block
        // (r0 = e, r1 = n, solved rows c0 = k..c1 = e); for the backward
        // sweep they are above it (r0 = 0, r1 = k).
        let (rows, inner) = (r1 - r0, c1 - c0);
        let mut a = Vec::with_capacity(rows * inner);
        for j in c0..c1 {
            a.extend_from_slice(&self.lu[r0 + j * n..r1 + j * n]);
        }
        let mut bmat = Vec::with_capacity(inner * m);
        for c in 0..m {
            bmat.extend_from_slice(&x[c0 + c * n..c1 + c * n]);
        }
        let prod = Matrix::from_col_major(rows, inner, a)
            .matmul(&Matrix::from_col_major(inner, m, bmat))
            .expect("shapes agree by construction");
        let p = prod.as_slice();
        for c in 0..m {
            let dst = &mut x[r0 + c * n..r1 + c * n];
            for (d, s) in dst.iter_mut().zip(&p[c * rows..(c + 1) * rows]) {
                *d -= *s;
            }
        }
    }
}

/// 1-norm (maximum absolute column sum) of a column-major `n x n` matrix.
pub fn norm1(a: &[f64], n: usize) -> f64 {
    (0..n)
        .map(|c| a[c * n..(c + 1) * n].iter().map(|v| v.abs()).sum::<f64>())
        .fold(0.0, f64::max)
}

/// Inverse of a square matrix through the blocked LU, guarded so that it
/// only ever answers when the matrix is *clearly* nonsingular by the same
/// standard the SVD-based `pinv` uses (`sigma_min > n * eps * sigma_max`).
///
/// `cond_2 <= n * cond_1` for an `n x n` matrix, and `cond_1` is known
/// exactly once the inverse is in hand, so `n * cond_1 * n * eps < 1/4`
/// proves `cond_2 < 1 / (4 n eps)`: the SVD route would report full rank.
/// Anything else (exact zero pivot, borderline conditioning, a non-finite
/// result) returns `None` and the caller keeps its SVD path, which decides
/// those cases exactly as before.
pub fn inverse_if_well_conditioned(a: &[f64], n: usize) -> Option<Vec<f64>> {
    if n == 0 {
        return None;
    }
    let f = factor(a, n);
    if f.singular {
        return None;
    }
    let mut ident = vec![0.0; n * n];
    for i in 0..n {
        ident[i + i * n] = 1.0;
    }
    let inv = f.solve(&ident, n)?;
    if inv.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let cond1 = norm1(a, n) * norm1(&inv, n);
    let nf = n as f64;
    if cond1.is_finite() && cond1 * nf * nf * f64::EPSILON < 0.25 {
        Some(inv)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random fill (no external RNG needed).
    fn fill(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..n * n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect()
    }

    fn matvec(a: &[f64], n: usize, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; n];
        for c in 0..n {
            for r in 0..n {
                y[r] += a[r + c * n] * x[c];
            }
        }
        y
    }

    #[test]
    fn solve_residual_is_tiny_across_block_boundaries() {
        for &n in &[1usize, 2, 3, 17, 63, 64, 65, 130, 200] {
            let a = fill(n, n as u64 + 7);
            let xs: Vec<f64> = (0..n).map(|i| (i as f64 * 0.37).sin()).collect();
            let b = matvec(&a, n, &xs);
            let f = factor(&a, n);
            let x = f.solve(&b, 1).expect("random matrix is nonsingular");
            let err = x.iter().zip(&xs).map(|(p, q)| (p - q).abs()).fold(0.0, f64::max);
            assert!(err < 1e-8, "n={n} err={err}");
        }
    }

    #[test]
    fn inverse_times_matrix_is_identity() {
        for &n in &[1usize, 5, 64, 100, 150] {
            let a = fill(n, 99 + n as u64);
            let inv = inverse_if_well_conditioned(&a, n).expect("well conditioned");
            let prod = Matrix::from_col_major(n, n, a)
                .matmul(&Matrix::from_col_major(n, n, inv))
                .unwrap();
            for c in 0..n {
                for r in 0..n {
                    let want = if r == c { 1.0 } else { 0.0 };
                    assert!((prod.as_slice()[r + c * n] - want).abs() < 1e-9, "n={n}");
                }
            }
        }
    }

    #[test]
    fn determinant_matches_a_known_value() {
        // [[2,1],[1,3]] col-major, det = 5; with a row swap forced: [[0,1],[1,0]] det = -1.
        assert!((factor(&[2.0, 1.0, 1.0, 3.0], 2).det() - 5.0).abs() < 1e-14);
        assert!((factor(&[0.0, 1.0, 1.0, 0.0], 2).det() + 1.0).abs() < 1e-14);
    }

    #[test]
    fn singular_and_borderline_matrices_decline() {
        // exact zero pivot
        assert!(inverse_if_well_conditioned(&[1.0, 2.0, 2.0, 4.0], 2).is_none());
        // numerically singular: second column is 1 + 1e-17 times the first
        let a = [1.0, 1.0, 1.0, 1.0 + 1e-15];
        assert!(inverse_if_well_conditioned(&a, 2).is_none());
    }

    #[test]
    fn permutation_reproduces_pa_equals_lu() {
        let n = 90;
        let a = fill(n, 5);
        let f = factor(&a, n);
        let perm = f.permutation();
        for c in 0..n {
            for r in 0..n {
                // (L U)[r,c] = sum_k L[r,k] U[k,c]
                let mut s = 0.0;
                for k in 0..=r.min(c) {
                    let l = if k == r { 1.0 } else { f.lu[r + k * n] };
                    s += l * f.lu[k + c * n];
                }
                assert!((s - a[perm[r] + c * n]).abs() < 1e-12);
            }
        }
    }
}
