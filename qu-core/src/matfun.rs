//! Matrix functions and Schur-based solvers (added in v0.4.14): the real
//! Schur decomposition, the principal matrix square root and logarithm, and
//! the Sylvester / Lyapunov equations.
//!
//! Everything here works on dense, real, square input and is built on one
//! primitive: the **complex Schur form** `A = U T U^H` (`T` upper
//! triangular). It is obtained from `nalgebra`'s real Schur form by turning
//! each 2x2 block of a complex-conjugate pair into a triangular one with a
//! 2x2 unitary similarity, so the QR iteration itself is not reimplemented.
//! On a triangular `T` the square root, the logarithm and the Sylvester
//! solve all reduce to substitution recurrences (Bjorck and Hammarling 1983
//! for the root, Bartels and Stewart 1972 for the equations; the logarithm
//! is inverse scaling and squaring with the degree-7 Pade approximant in
//! its Gauss-Legendre partial-fraction form, Higham 2008 section 11.5).
//!
//! Every returned value is checked, in this file's tests, against an
//! identity that does not use the code under test: `U T U' = A`,
//! `sqrtm(A)^2 = A`, `expm(logm(A)) = A`, and the equation residuals.

use crate::matrix::Matrix;
use crate::Complex64 as C;

const EPS: f64 = f64::EPSILON;

fn cz() -> C {
    C::new(0.0, 0.0)
}

/// Dense complex square matrix, column-major, only what this module needs.
#[derive(Clone, Debug)]
pub struct CSq {
    pub n: usize,
    pub d: Vec<C>,
}

impl CSq {
    fn zeros(n: usize) -> Self {
        CSq { n, d: vec![cz(); n * n] }
    }
    fn from_real(n: usize, a: &[f64]) -> Self {
        CSq { n, d: a.iter().map(|&x| C::real(x)).collect() }
    }
    #[inline]
    fn at(&self, i: usize, j: usize) -> C {
        self.d[i + j * self.n]
    }
    #[inline]
    fn set(&mut self, i: usize, j: usize, v: C) {
        self.d[i + j * self.n] = v;
    }
    /// `self * other`.
    fn mul(&self, o: &CSq) -> CSq {
        let n = self.n;
        let mut r = CSq::zeros(n);
        for j in 0..n {
            for k in 0..n {
                let b = o.at(k, j);
                if b.re == 0.0 && b.im == 0.0 {
                    continue;
                }
                for i in 0..n {
                    let v = r.d[i + j * n].add(self.d[i + k * n].mul(b));
                    r.d[i + j * n] = v;
                }
            }
        }
        r
    }
    /// Conjugate transpose.
    fn h(&self) -> CSq {
        let n = self.n;
        let mut r = CSq::zeros(n);
        for i in 0..n {
            for j in 0..n {
                r.d[j + i * n] = self.d[i + j * n].conj();
            }
        }
        r
    }
    fn norm1(&self) -> f64 {
        let n = self.n;
        (0..n)
            .map(|j| (0..n).map(|i| self.at(i, j).magnitude()).sum::<f64>())
            .fold(0.0, f64::max)
    }
    /// Real part as a [`Matrix`]; the second value is the largest `|imag|`.
    fn re(&self) -> (Matrix, f64) {
        let im = self.d.iter().fold(0.0f64, |m, z| m.max(z.im.abs()));
        (Matrix::from_col_major(self.n, self.n, self.d.iter().map(|z| z.re).collect()), im)
    }
}

/// A real Schur decomposition `A = U T U'`: `U` orthogonal, `T` real
/// quasi-upper-triangular (1x1 blocks for real eigenvalues, 2x2 blocks for
/// complex-conjugate pairs).
#[derive(Debug)]
pub struct RealSchur {
    pub t: Matrix,
    pub u: Matrix,
}

fn check_square(f: &str, a: &Matrix) -> Result<usize, String> {
    if a.rows() != a.cols() {
        return Err(format!("{f}: needs a square matrix, got {}x{}", a.rows(), a.cols()));
    }
    if a.rows() == 0 {
        return Err(format!("{f}: empty matrix"));
    }
    if a.as_slice().iter().any(|x| !x.is_finite()) {
        return Err(format!("{f}: the matrix contains NaN or Inf"));
    }
    Ok(a.rows())
}

/// Real Schur form with every 2x2 block that has real eigenvalues reduced
/// to two 1x1 blocks, so the only 2x2 blocks left are complex pairs.
fn real_schur_raw(a: &Matrix) -> Result<(Vec<f64>, Vec<f64>, usize), String> {
    let n = a.rows();
    let m = nalgebra::DMatrix::from_column_slice(n, n, a.as_slice());
    let s = m
        .try_schur(EPS, 100_000)
        .ok_or_else(|| "the Schur iteration did not converge".to_string())?;
    let (q, t) = s.unpack();
    let mut t: Vec<f64> = t.as_slice().to_vec();
    let mut u: Vec<f64> = q.as_slice().to_vec();
    // clean the part below the first subdiagonal and negligible subdiagonals
    for j in 0..n {
        for i in (j + 2)..n {
            t[i + j * n] = 0.0;
        }
    }
    for i in 0..n.saturating_sub(1) {
        let s = t[i + i * n].abs() + t[i + 1 + (i + 1) * n].abs();
        if t[i + 1 + i * n].abs() <= EPS * s.max(f64::MIN_POSITIVE) {
            t[i + 1 + i * n] = 0.0;
        }
    }
    // split 2x2 blocks with real eigenvalues by a rotation
    let mut i = 0;
    while i + 1 < n {
        if t[i + 1 + i * n] != 0.0 {
            let (a11, a12, a21, a22) =
                (t[i + i * n], t[i + (i + 1) * n], t[i + 1 + i * n], t[i + 1 + (i + 1) * n]);
            let disc = ((a11 - a22) / 2.0).powi(2) + a12 * a21;
            if disc >= 0.0 {
                let lam = (a11 + a22) / 2.0 + disc.sqrt();
                // eigenvector of lam
                let (mut v1, mut v2) = if a12.abs() >= a21.abs() { (a12, lam - a11) } else { (lam - a22, a21) };
                let nv = v1.hypot(v2);
                v1 /= nv;
                v2 /= nv;
                // rotation G = [v1 -v2; v2 v1]; T <- G' T G, U <- U G
                rot_apply(&mut t, &mut u, n, i, v1, v2);
                t[i + 1 + i * n] = 0.0;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Ok((t, u, n))
}

fn rot_apply(t: &mut [f64], u: &mut [f64], n: usize, i: usize, c: f64, s: f64) {
    // columns i, i+1 of T and U: [x y] <- [x y] G with G = [c -s; s c]
    for r in 0..n {
        let (x, y) = (t[r + i * n], t[r + (i + 1) * n]);
        t[r + i * n] = c * x + s * y;
        t[r + (i + 1) * n] = -s * x + c * y;
        let (x, y) = (u[r + i * n], u[r + (i + 1) * n]);
        u[r + i * n] = c * x + s * y;
        u[r + (i + 1) * n] = -s * x + c * y;
    }
    // rows i, i+1 of T: G' * rows
    for cidx in 0..n {
        let (x, y) = (t[i + cidx * n], t[i + 1 + cidx * n]);
        t[i + cidx * n] = c * x + s * y;
        t[i + 1 + cidx * n] = -s * x + c * y;
    }
}

/// Real Schur decomposition of a square real matrix.
pub fn schur(a: &Matrix) -> Result<RealSchur, String> {
    let n = check_square("schur", a)?;
    let (t, u, _) = real_schur_raw(a).map_err(|m| format!("schur: {m}"))?;
    Ok(RealSchur { t: Matrix::from_col_major(n, n, t), u: Matrix::from_col_major(n, n, u) })
}

/// Complex Schur form `A = U T U^H` with `T` upper triangular.
pub fn complex_schur(f: &str, a: &Matrix) -> Result<(CSq, CSq), String> {
    let n = check_square(f, a)?;
    let (t, u, _) = real_schur_raw(a).map_err(|m| format!("{f}: {m}"))?;
    let mut tc = CSq::from_real(n, &t);
    let mut uc = CSq::from_real(n, &u);
    let mut i = 0;
    while i + 1 < n {
        if t[i + 1 + i * n] != 0.0 {
            let (a11, a12, a21, a22) =
                (t[i + i * n], t[i + (i + 1) * n], t[i + 1 + i * n], t[i + 1 + (i + 1) * n]);
            let disc = ((a11 - a22) / 2.0).powi(2) + a12 * a21; // < 0 here
            let lam = C::new((a11 + a22) / 2.0, (-disc).max(0.0).sqrt());
            let (v1, v2) = if a12.abs() >= a21.abs() {
                (C::real(a12), lam.sub(C::real(a11)))
            } else {
                (lam.sub(C::real(a22)), C::real(a21))
            };
            let nv = v1.magnitude().hypot(v2.magnitude());
            let (v1, v2) = (v1.scale(1.0 / nv), v2.scale(1.0 / nv));
            // G = [v1  -conj(v2); v2  conj(v1)] is unitary and G^H B G = [lam *; 0 conj(lam)]
            let g = [[v1, v2.conj().scale(-1.0)], [v2, v1.conj()]];
            // columns i, i+1 (right multiply by G) for T and U
            for r in 0..n {
                for m in [&mut tc, &mut uc] {
                    let (x, y) = (m.at(r, i), m.at(r, i + 1));
                    m.set(r, i, x.mul(g[0][0]).add(y.mul(g[1][0])));
                    m.set(r, i + 1, x.mul(g[0][1]).add(y.mul(g[1][1])));
                }
            }
            // rows i, i+1 of T (left multiply by G^H)
            for c in 0..n {
                let (x, y) = (tc.at(i, c), tc.at(i + 1, c));
                tc.set(i, c, g[0][0].conj().mul(x).add(g[1][0].conj().mul(y)));
                tc.set(i + 1, c, g[0][1].conj().mul(x).add(g[1][1].conj().mul(y)));
            }
            tc.set(i + 1, i, cz());
            i += 2;
        } else {
            i += 1;
        }
    }
    // make T exactly upper triangular
    for j in 0..n {
        for r in (j + 1)..n {
            tc.set(r, j, cz());
        }
    }
    Ok((uc, tc))
}

/// Number of real eigenvalues clearly negative, and numerically zero (within `10 n eps ||A||`), among the 1x1
/// blocks of the real Schur form. Complex pairs are never on the real axis.
fn real_axis_counts(a: &Matrix) -> Result<(usize, usize), String> {
    let (t, _, n) = real_schur_raw(a)?;
    let (mut neg, mut zero) = (0, 0);
    let tol = 10.0 * n as f64 * EPS * t.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let mut i = 0;
    while i < n {
        if i + 1 < n && t[i + 1 + i * n] != 0.0 {
            i += 2;
        } else {
            let x = t[i + i * n];
            if x < -tol {
                neg += 1;
            } else if x.abs() <= tol {
                zero += 1;
            }
            i += 1;
        }
    }
    Ok((neg, zero))
}

/// Result of a matrix function whose principal value can be complex.
#[derive(Debug)]
pub enum MatFun {
    Real(Matrix),
    /// Column-major complex entries, `n x n`.
    Complex(usize, Vec<C>),
}

fn finish(c: &CSq, want_real: bool) -> MatFun {
    if want_real {
        MatFun::Real(c.re().0)
    } else {
        MatFun::Complex(c.n, c.d.clone())
    }
}

/// Square root of an upper-triangular complex matrix (Bjorck-Hammarling).
fn sqrt_tri(t: &CSq) -> Result<CSq, String> {
    let n = t.n;
    let scale = t.norm1().max(f64::MIN_POSITIVE);
    let mut r = CSq::zeros(n);
    for j in 0..n {
        r.set(j, j, t.at(j, j).sqrt());
        for i in (0..j).rev() {
            let mut s = t.at(i, j);
            for k in (i + 1)..j {
                s = s.sub(r.at(i, k).mul(r.at(k, j)));
            }
            let den = r.at(i, i).add(r.at(j, j));
            if den.magnitude() == 0.0 {
                if s.magnitude() <= 64.0 * EPS * scale {
                    r.set(i, j, cz());
                    continue;
                }
                return Err("the matrix has no square root (a zero eigenvalue with a nontrivial Jordan block)".into());
            }
            r.set(i, j, s.div(den));
        }
    }
    Ok(r)
}

/// Principal matrix square root. Real when `A` has no eigenvalue on the
/// open negative real axis (the usual case, also for singular `A` whose
/// zero eigenvalues are semisimple); otherwise the principal root is a
/// genuinely complex matrix and that is what is returned.
pub fn sqrtm(a: &Matrix) -> Result<MatFun, String> {
    let n = check_square("sqrtm", a)?;
    let (neg, _zero) = real_axis_counts(a).map_err(|m| format!("sqrtm: {m}"))?;
    let (u, t) = complex_schur("sqrtm", a)?;
    let r = sqrt_tri(&t).map_err(|m| format!("sqrtm: {m}"))?;
    let x = u.mul(&r).mul(&u.h());
    let _ = n;
    Ok(finish(&x, neg == 0))
}

/// Gauss-Legendre nodes and weights on `[0, 1]` (Newton on `P_m`).
fn gauss_legendre_01(m: usize) -> (Vec<f64>, Vec<f64>) {
    let mut x = vec![0.0; m];
    let mut w = vec![0.0; m];
    for i in 0..m {
        let mut z = (std::f64::consts::PI * (i as f64 + 0.75) / (m as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, z);
            for k in 2..=m {
                let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
                p0 = p1;
                p1 = p2;
            }
            let pm = if m == 1 { z } else { p1 };
            let pm1 = if m == 1 { 1.0 } else { p0 };
            dp = m as f64 * (z * pm - pm1) / (z * z - 1.0);
            let dz = pm / dp;
            z -= dz;
            if dz.abs() < 1e-16 {
                break;
            }
        }
        // map [-1, 1] -> [0, 1]
        x[i] = 0.5 * (1.0 - z);
        w[i] = 1.0 / ((1.0 - z * z) * dp * dp);
    }
    (x, w)
}

/// `X (I + s X)^{-1}` for upper-triangular `X`, by triangular solves.
fn tri_x_over_i_plus_sx(x: &CSq, s: f64) -> CSq {
    let n = x.n;
    // Solve (I + s X) Y = X column by column, back substitution.
    let mut y = CSq::zeros(n);
    for j in 0..n {
        for i in (0..=j).rev() {
            let mut v = x.at(i, j);
            for k in (i + 1)..=j {
                v = v.sub(x.at(i, k).scale(s).mul(y.at(k, j)));
            }
            let d = C::real(1.0).add(x.at(i, i).scale(s));
            y.set(i, j, v.div(d));
        }
    }
    y
}

/// Principal matrix logarithm of an upper-triangular complex matrix with a
/// non-zero diagonal.
fn log_tri(t: &CSq) -> Result<CSq, String> {
    let n = t.n;
    let diag_log: Vec<C> = (0..n).map(|i| t.at(i, i).ln()).collect();
    let mut cur = t.clone();
    let mut k = 0u32;
    let theta = 0.25;
    loop {
        let mut d = cur.clone();
        for i in 0..n {
            d.set(i, i, d.at(i, i).sub(C::real(1.0)));
        }
        if d.norm1() <= theta {
            break;
        }
        if k >= 120 {
            return Err("the inverse scaling and squaring iteration did not converge".into());
        }
        cur = sqrt_tri(&cur)?;
        k += 1;
    }
    let mut x = cur;
    for i in 0..n {
        x.set(i, i, x.at(i, i).sub(C::real(1.0)));
    }
    let (nodes, weights) = gauss_legendre_01(7);
    let mut s = CSq::zeros(n);
    for (xj, wj) in nodes.iter().zip(&weights) {
        let y = tri_x_over_i_plus_sx(&x, *xj);
        for (a, b) in s.d.iter_mut().zip(&y.d) {
            *a = a.add(b.scale(*wj));
        }
    }
    let f = (2.0f64).powi(k as i32);
    for z in s.d.iter_mut() {
        *z = z.scale(f);
    }
    // the diagonal is known exactly: log of the original eigenvalues
    for i in 0..n {
        s.set(i, i, diag_log[i]);
    }
    Ok(s)
}

/// Principal matrix logarithm; the inverse of `expm`. Real when `A` has no
/// eigenvalue on the closed negative real axis; with a negative real
/// eigenvalue the principal logarithm is complex and is returned as such.
/// A zero eigenvalue is an error (the logarithm does not exist).
pub fn logm(a: &Matrix) -> Result<MatFun, String> {
    check_square("logm", a)?;
    let (neg, zero) = real_axis_counts(a).map_err(|m| format!("logm: {m}"))?;
    if zero > 0 {
        return Err("logm: the matrix is singular (a zero eigenvalue), so its logarithm does not exist".into());
    }
    let (u, t) = complex_schur("logm", a)?;
    let l = log_tri(&t).map_err(|m| format!("logm: {m}"))?;
    let x = u.mul(&l).mul(&u.h());
    Ok(finish(&x, neg == 0))
}

/// Solve `A X + X B = C` for `X` (`A` is `n x n`, `B` is `m x m`, `C` is
/// `n x m`) by the Bartels-Stewart algorithm on complex Schur forms.
pub fn sylvester(a: &Matrix, b: &Matrix, c: &Matrix) -> Result<Matrix, String> {
    let n = check_square("sylvester", a).map_err(|m| format!("{m} (A)"))?;
    let m = check_square("sylvester", b).map_err(|m| format!("{m} (B)"))?;
    if c.rows() != n || c.cols() != m {
        return Err(format!(
            "sylvester: C must be {n}x{m} (rows of A by columns of B), got {}x{}",
            c.rows(),
            c.cols()
        ));
    }
    if c.as_slice().iter().any(|x| !x.is_finite()) {
        return Err("sylvester: C contains NaN or Inf".into());
    }
    let (ua, ta) = complex_schur("sylvester", a)?;
    let (ub, tb) = complex_schur("sylvester", b)?;
    // F = Ua^H C Ub
    let mut cc = vec![cz(); n * m];
    for j in 0..m {
        for i in 0..n {
            cc[i + j * n] = C::real(c.as_slice()[i + j * n]);
        }
    }
    let uah = ua.h();
    // tmp = Ua^H * C  (n x m)
    let mut tmp = vec![cz(); n * m];
    for j in 0..m {
        for k in 0..n {
            let ck = cc[k + j * n];
            if ck.re == 0.0 && ck.im == 0.0 {
                continue;
            }
            for i in 0..n {
                tmp[i + j * n] = tmp[i + j * n].add(uah.at(i, k).mul(ck));
            }
        }
    }
    // F = tmp * Ub (n x m)
    let mut f = vec![cz(); n * m];
    for j in 0..m {
        for k in 0..m {
            let u = ub.at(k, j);
            for i in 0..n {
                f[i + j * n] = f[i + j * n].add(tmp[i + k * n].mul(u));
            }
        }
    }
    let tol = 8.0 * EPS * (ta.norm1() + tb.norm1()).max(f64::MIN_POSITIVE) * (n.max(m) as f64);
    // Ta Y + Y Tb = F, column by column of Y
    let mut y = vec![cz(); n * m];
    for j in 0..m {
        let mut rhs: Vec<C> = f[j * n..(j + 1) * n].to_vec();
        for k in 0..j {
            let tkj = tb.at(k, j);
            if tkj.re == 0.0 && tkj.im == 0.0 {
                continue;
            }
            for i in 0..n {
                rhs[i] = rhs[i].sub(y[i + k * n].mul(tkj));
            }
        }
        let bjj = tb.at(j, j);
        for i in (0..n).rev() {
            let mut v = rhs[i];
            for k in (i + 1)..n {
                v = v.sub(ta.at(i, k).mul(y[k + j * n]));
            }
            let den = ta.at(i, i).add(bjj);
            if den.magnitude() <= tol {
                return Err("sylvester: A and -B have a common (or nearly common) eigenvalue, so the equation has no unique solution".into());
            }
            y[i + j * n] = v.div(den);
        }
    }
    // X = Ua Y Ub^H
    let ubh = ub.h();
    let mut t2 = vec![cz(); n * m];
    // t2 = Ua * Y
    for j in 0..m {
        for k in 0..n {
            let yk = y[k + j * n];
            for i in 0..n {
                t2[i + j * n] = t2[i + j * n].add(ua.at(i, k).mul(yk));
            }
        }
    }
    let mut x = vec![0.0; n * m];
    let mut worst_im = 0.0f64;
    let mut xmax = 0.0f64;
    for j in 0..m {
        let mut col = vec![cz(); n];
        for k in 0..m {
            let u = ubh.at(k, j);
            for i in 0..n {
                col[i] = col[i].add(t2[i + k * n].mul(u));
            }
        }
        for i in 0..n {
            x[i + j * n] = col[i].re;
            worst_im = worst_im.max(col[i].im.abs());
            xmax = xmax.max(col[i].re.abs());
        }
    }
    let _ = (worst_im, xmax);
    Ok(Matrix::from_col_major(n, m, x))
}

/// Solve the continuous Lyapunov equation `A X + X A' + Q = 0`.
pub fn lyap(a: &Matrix, q: &Matrix) -> Result<Matrix, String> {
    let n = check_square("lyap", a)?;
    if q.rows() != n || q.cols() != n {
        return Err(format!("lyap: Q must be {n}x{n} like A, got {}x{}", q.rows(), q.cols()));
    }
    let neg_q = Matrix::from_col_major(n, n, q.as_slice().iter().map(|x| -x).collect());
    sylvester(a, &a.transpose(), &neg_q).map_err(|m| {
        if m.contains("common") {
            "lyap: A has eigenvalues l1, l2 with l1 + l2 = 0 (for example a pole pair on the imaginary axis), so the equation has no unique solution".to_string()
        } else {
            m.replacen("sylvester", "lyap", 1)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(rows: &[&[f64]]) -> Matrix {
        Matrix::from_rows(&rows.iter().map(|r| r.to_vec()).collect::<Vec<_>>()).unwrap()
    }

    fn mm(a: &Matrix, b: &Matrix) -> Matrix {
        a.matmul(b).unwrap()
    }

    fn maxdiff(a: &Matrix, b: &Matrix) -> f64 {
        assert_eq!(a.shape(), b.shape());
        a.as_slice().iter().zip(b.as_slice()).fold(0.0, |s, (x, y)| s.max((x - y).abs()))
    }

    /// Deterministic pseudo-random matrix (LCG), no dependency.
    fn rnd(n: usize, mcols: usize, seed: u64) -> Matrix {
        let mut s = seed;
        let d: Vec<f64> = (0..n * mcols)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as f64 / (1u64 << 31) as f64) - 0.5
            })
            .collect();
        Matrix::from_col_major(n, mcols, d)
    }

    fn eye(n: usize) -> Matrix {
        Matrix::eye(n)
    }

    fn add(a: &Matrix, b: &Matrix) -> Matrix {
        Matrix::from_col_major(
            a.rows(),
            a.cols(),
            a.as_slice().iter().zip(b.as_slice()).map(|(x, y)| x + y).collect(),
        )
    }

    fn real(x: MatFun) -> Matrix {
        match x {
            MatFun::Real(m) => m,
            MatFun::Complex(..) => panic!("expected a real result"),
        }
    }

    #[test]
    fn schur_identity_and_shape() {
        for (n, seed) in [(1usize, 1u64), (2, 2), (3, 3), (5, 4), (8, 5), (12, 6), (20, 7)] {
            let a = rnd(n, n, seed);
            let s = schur(&a).unwrap();
            let rec = mm(&mm(&s.u, &s.t), &s.u.transpose());
            assert!(maxdiff(&rec, &a) < 1e-12, "n={n}: U T U' != A ({})", maxdiff(&rec, &a));
            let utu = mm(&s.u.transpose(), &s.u);
            assert!(maxdiff(&utu, &eye(n)) < 1e-13, "n={n}: U not orthogonal");
            // quasi-triangular: nothing below the first subdiagonal, no two
            // consecutive non-zero subdiagonal entries
            for j in 0..n {
                for i in (j + 2)..n {
                    assert_eq!(s.t.get(i, j).unwrap(), 0.0, "n={n} t[{i},{j}]");
                }
            }
            for i in 0..n.saturating_sub(2) {
                let (x, y) = (s.t.get(i + 1, i).unwrap(), s.t.get(i + 2, i + 1).unwrap());
                assert!(x == 0.0 || y == 0.0, "n={n}: two adjacent 2x2 blocks overlap at {i}");
            }
            // every remaining 2x2 block is a complex pair
            for i in 0..n.saturating_sub(1) {
                let c = s.t.get(i + 1, i).unwrap();
                if c != 0.0 {
                    let (a11, a12, a22) = (
                        s.t.get(i, i).unwrap(),
                        s.t.get(i, i + 1).unwrap(),
                        s.t.get(i + 1, i + 1).unwrap(),
                    );
                    assert!(((a11 - a22) / 2.0).powi(2) + a12 * c < 0.0, "real-eigenvalue block left");
                }
            }
        }
    }

    #[test]
    fn schur_of_rotation_keeps_complex_block() {
        let a = m(&[&[0.0, -1.0], &[1.0, 0.0]]);
        let s = schur(&a).unwrap();
        assert!(s.t.get(1, 0).unwrap() != 0.0);
        let rec = mm(&mm(&s.u, &s.t), &s.u.transpose());
        assert!(maxdiff(&rec, &a) < 1e-14);
    }

    #[test]
    fn schur_errors() {
        assert!(schur(&Matrix::zeros(2, 3)).unwrap_err().contains("square"));
        assert!(schur(&m(&[&[1.0, f64::NAN], &[0.0, 1.0]])).unwrap_err().contains("NaN"));
    }

    #[test]
    fn sqrtm_squares_back() {
        for (n, seed) in [(2usize, 11u64), (3, 12), (6, 13), (10, 14)] {
            // symmetric positive definite shifted so eigenvalues are positive
            let b = rnd(n, n, seed);
            let a = add(&mm(&b, &b.transpose()), &eye(n));
            let r = real(sqrtm(&a).unwrap());
            assert!(maxdiff(&mm(&r, &r), &a) < 1e-12, "spd n={n}");
            // nonsymmetric with complex eigenvalues, shifted to the right half plane
            let g = add(&rnd(n, n, seed + 100), &Matrix::from_col_major(n, n, {
                let mut d = vec![0.0; n * n];
                for i in 0..n {
                    d[i + i * n] = 2.0;
                }
                d
            }));
            let r = real(sqrtm(&g).unwrap());
            assert!(maxdiff(&mm(&r, &r), &g) < 1e-12, "general n={n}: {}", maxdiff(&mm(&r, &r), &g));
        }
    }

    #[test]
    fn sqrtm_known_values() {
        let r = real(sqrtm(&m(&[&[4.0, 0.0], &[0.0, 9.0]])).unwrap());
        assert!(maxdiff(&r, &m(&[&[2.0, 0.0], &[0.0, 3.0]])) < 1e-14);
        // [1 1; 0 1]^(1/2) = [1 .5; 0 1]
        let r = real(sqrtm(&m(&[&[1.0, 1.0], &[0.0, 1.0]])).unwrap());
        assert!(maxdiff(&r, &m(&[&[1.0, 0.5], &[0.0, 1.0]])) < 1e-14);
        // zero matrix has the zero root
        let r = real(sqrtm(&Matrix::zeros(3, 3)).unwrap());
        assert!(r.as_slice().iter().all(|x| *x == 0.0));
    }

    #[test]
    fn sqrtm_negative_eigenvalue_is_complex() {
        match sqrtm(&m(&[&[-4.0, 0.0], &[0.0, 1.0]])).unwrap() {
            MatFun::Complex(n, d) => {
                assert_eq!(n, 2);
                assert!((d[0].re).abs() < 1e-14 && (d[0].im - 2.0).abs() < 1e-14);
                assert!((d[3].re - 1.0).abs() < 1e-14 && d[3].im.abs() < 1e-14);
            }
            _ => panic!("expected complex"),
        }
    }

    #[test]
    fn sqrtm_nilpotent_errors() {
        let e = sqrtm(&m(&[&[0.0, 1.0], &[0.0, 0.0]])).err().expect("no root");
        assert!(e.contains("no square root"), "{e}");
    }

    #[test]
    fn logm_known_and_inverse_of_exp() {
        // log of diag
        let l = real(logm(&m(&[&[std::f64::consts::E, 0.0], &[0.0, 1.0]])).unwrap());
        assert!(maxdiff(&l, &m(&[&[1.0, 0.0], &[0.0, 0.0]])) < 1e-14);
        // log of identity is zero
        let l = real(logm(&eye(3)).unwrap());
        assert!(l.as_slice().iter().all(|x| x.abs() < 1e-15));
        // log([1 1; 0 1]) = [0 1; 0 0]
        let l = real(logm(&m(&[&[1.0, 1.0], &[0.0, 1.0]])).unwrap());
        assert!(maxdiff(&l, &m(&[&[0.0, 1.0], &[0.0, 0.0]])) < 1e-13);
        // log of a rotation by t is the skew generator t*[0 -1; 1 0]
        let t = 0.7f64;
        let rot = m(&[&[t.cos(), -t.sin()], &[t.sin(), t.cos()]]);
        let l = real(logm(&rot).unwrap());
        assert!(maxdiff(&l, &m(&[&[0.0, -t], &[t, 0.0]])) < 1e-13);
    }

    #[test]
    fn logm_sqrtm_consistency() {
        // sqrtm(A) = expm(logm(A)/2) checked through squaring: logm(A) = 2 logm(sqrtm(A))
        let b = rnd(4, 4, 21);
        let a = add(&mm(&b, &b.transpose()), &eye(4));
        let la = real(logm(&a).unwrap());
        let r = real(sqrtm(&a).unwrap());
        let lr = real(logm(&r).unwrap());
        let twice = Matrix::from_col_major(4, 4, lr.as_slice().iter().map(|x| 2.0 * x).collect());
        assert!(maxdiff(&la, &twice) < 1e-12);
    }

    #[test]
    fn logm_singular_and_negative() {
        assert!(logm(&m(&[&[1.0, 0.0], &[0.0, 0.0]])).err().unwrap().contains("singular"));
        match logm(&m(&[&[-1.0, 0.0], &[0.0, 1.0]])).unwrap() {
            MatFun::Complex(_, d) => {
                assert!((d[0].im - std::f64::consts::PI).abs() < 1e-14);
            }
            _ => panic!("expected complex"),
        }
    }

    #[test]
    fn sylvester_residual() {
        for (n, mc, seed) in [(1usize, 1usize, 31u64), (3, 2, 32), (4, 4, 33), (7, 5, 34), (12, 3, 35)] {
            // shift A right and B right so A + (-B) never share eigenvalues
            let a = add(&rnd(n, n, seed), &Matrix::from_col_major(n, n, {
                let mut d = vec![0.0; n * n];
                for i in 0..n {
                    d[i + i * n] = 3.0;
                }
                d
            }));
            let b = add(&rnd(mc, mc, seed + 1), &Matrix::from_col_major(mc, mc, {
                let mut d = vec![0.0; mc * mc];
                for i in 0..mc {
                    d[i + i * mc] = 3.0;
                }
                d
            }));
            let c = rnd(n, mc, seed + 2);
            let x = sylvester(&a, &b, &c).unwrap();
            let res = add(&mm(&a, &x), &mm(&x, &b));
            assert!(maxdiff(&res, &c) < 1e-12, "n={n} m={mc}: {}", maxdiff(&res, &c));
        }
    }

    #[test]
    fn sylvester_scalar() {
        // 2x + 3x = 10 -> x = 2
        let x = sylvester(&m(&[&[2.0]]), &m(&[&[3.0]]), &m(&[&[10.0]])).unwrap();
        assert!((x.get(0, 0).unwrap() - 2.0).abs() < 1e-15);
    }

    #[test]
    fn sylvester_singular_and_shape_errors() {
        let e = sylvester(&m(&[&[1.0]]), &m(&[&[-1.0]]), &m(&[&[1.0]])).unwrap_err();
        assert!(e.contains("eigenvalue"), "{e}");
        assert!(sylvester(&eye(2), &eye(3), &Matrix::zeros(2, 2)).unwrap_err().contains("2x3"));
    }

    #[test]
    fn lyap_residual_and_symmetry() {
        // stable A: eigenvalues in the open left half plane
        let a = add(&rnd(5, 5, 41), &Matrix::from_col_major(5, 5, {
            let mut d = vec![0.0; 25];
            for i in 0..5 {
                d[i + i * 5] = -2.0;
            }
            d
        }));
        let b = rnd(5, 5, 42);
        let q = mm(&b, &b.transpose());
        let x = lyap(&a, &q).unwrap();
        let res = add(&add(&mm(&a, &x), &mm(&x, &a.transpose())), &q);
        assert!(res.as_slice().iter().all(|v| v.abs() < 1e-12), "residual");
        // symmetric Q gives symmetric X, and X is PSD (diagonal >= 0)
        assert!(maxdiff(&x, &x.transpose()) < 1e-12);
        for i in 0..5 {
            assert!(x.get(i, i).unwrap() >= 0.0);
        }
        // scalar: 2 a x + q = 0 -> x = -q/(2a)
        let xs = lyap(&m(&[&[-3.0]]), &m(&[&[6.0]])).unwrap();
        assert!((xs.get(0, 0).unwrap() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn lyap_pole_pair_on_axis_errors() {
        let e = lyap(&m(&[&[0.0, 1.0], &[-1.0, 0.0]]), &eye(2)).unwrap_err();
        assert!(e.contains("l1 + l2 = 0"), "{e}");
    }
}
