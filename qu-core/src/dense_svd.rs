//! Thin SVD for large dense matrices, v0.4.10.
//!
//! Same algorithm and conventions as the `nalgebra::SVD` it replaces above the
//! size threshold -- Householder bidiagonalisation, then the implicit-shift
//! QR iteration on the bidiagonal -- restructured for speed:
//!
//! * the bidiagonalisation is blocked (LAPACK `gebrd`/`labrd`): within a panel
//!   of `NBB` columns each step makes one pass over the stale trailing
//!   matrix for the two matrix-vector products, and the trailing matrix gets
//!   one rank-`2*NBB` update by GEMM per panel;
//! * `U` and `V` are formed from the reflectors with compact-WY blocks on the
//!   GEMM (`dense_qr::orgqr`);
//! * the QR iteration works on the `d`/`e` vectors only; every rotation it
//!   makes is recorded and applied to `U`/`V` later, in cache-sized row
//!   blocks that run in parallel (`RotTarget`). Each row sees the same
//!   rotations in the same order as the in-line application would give it.
//!
//! Wide matrices are handled through the transpose (a lower bidiagonal form
//! of `A` is the transpose of the upper bidiagonal form of `A^T`), keeping the
//! iteration's `is_upper_diagonal` bookkeeping that decides which of the two
//! factors each rotation lands on.
//!
//! Conventions match nalgebra: the matrix is scaled by its largest entry
//! first, the bidiagonal carries non-negative entries (signs go to `U`/`V`),
//! singular values are returned in descending order.

use crate::dense_kernels::{avx2_fn, dot, for_each_col_group, gemm_serial, transpose, RotTarget};
use crate::dense_qr::{larfg, orgqr};

/// Panel width of the blocked bidiagonalisation.
const NBB: usize = 32;
/// Hard lower bound for the blocked path (smaller trailing blocks use the
/// unblocked reduction).
const BLOCKED_MIN: usize = 2 * NBB;

/// Result of the thin SVD `A = U diag(s) V^T`.
pub struct SvdOut {
    /// `m x k`, column-major.
    pub u: Vec<f64>,
    pub s: Vec<f64>,
    /// `k x n`, column-major.
    pub vt: Vec<f64>,
}

#[inline]
fn sgn(x: f64) -> f64 {
    if x >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

pub struct Bidiag {
    pub w: Vec<f64>,
    pub d: Vec<f64>,
    pub e: Vec<f64>,
    pub tauq: Vec<f64>,
    pub taup: Vec<f64>,
}

/// Unblocked reduction (LAPACK `gebd2`) of the trailing block starting at
/// `(i0, i0)`; `m >= n`.
#[inline(always)]
fn gebd2(w: &mut [f64], m: usize, n: usize, i0: usize, bd: &mut Bidiag) {
    let ld = m;
    for i in i0..n {
        // ---- H(i) annihilates w[i+1.., i]
        let t = larfg(&mut w[i + i * ld..m + i * ld]);
        bd.tauq[i] = t;
        bd.d[i] = w[i + i * ld];
        if i + 1 < n {
            if t != 0.0 {
                let mut v = w[i + i * ld..m + i * ld].to_vec();
                v[0] = 1.0;
                for c in i + 1..n {
                    let col = &mut w[i + c * ld..m + c * ld];
                    let s = t * dot(&v, col);
                    for (x, vi) in col.iter_mut().zip(&v) {
                        *x -= s * vi;
                    }
                }
            }
            // ---- G(i) annihilates w[i, i+2..]
            let mut row: Vec<f64> = (i + 1..n).map(|c| w[i + c * ld]).collect();
            let tp = larfg(&mut row);
            bd.taup[i] = tp;
            bd.e[i] = row[0];
            for (q, c) in (i + 1..n).enumerate() {
                if q > 0 {
                    w[i + c * ld] = row[q];
                }
            }
            if tp != 0.0 {
                let mut v = row.clone();
                v[0] = 1.0;
                // z = A[i+1.., i+1..] v ; A -= tp z v^T
                let mrows = m - (i + 1);
                if mrows > 0 {
                    let mut z = vec![0.0; mrows];
                    for (q, c) in (i + 1..n).enumerate() {
                        let col = &w[i + 1 + c * ld..m + c * ld];
                        let vq = v[q];
                        for (zi, ci) in z.iter_mut().zip(col) {
                            *zi += ci * vq;
                        }
                    }
                    for (q, c) in (i + 1..n).enumerate() {
                        let col = &mut w[i + 1 + c * ld..m + c * ld];
                        let f = tp * v[q];
                        for (ci, zi) in col.iter_mut().zip(&z) {
                            *ci -= f * zi;
                        }
                    }
                }
            }
        }
    }
}

/// Blocked panel (LAPACK `labrd`, `m >= n`) for the block starting at
/// `(i0, i0)`: reduces `nb` columns/rows, then updates the trailing matrix.
#[inline(always)]
fn labrd_panel(w: &mut [f64], m: usize, n: usize, i0: usize, nb: usize, bd: &mut Bidiag) {
    let ld = m;
    let mm = m - i0; // local rows
    let nn = n - i0; // local cols
    let mut x = vec![0.0; mm * nb]; // X: mm x nb
    let mut y = vec![0.0; nn * nb]; // Y: nn x nb
    let ai = |r: usize, c: usize| (i0 + r) + (i0 + c) * ld;
    for i in 0..nb {
        // ---- (1) bring column i up to date.
        let mut ci: Vec<f64> = w[ai(i, i)..ai(mm, i)].to_vec();
        for k in 0..i {
            let coef = y[i + k * nn];
            if coef != 0.0 {
                let col = &w[ai(i, k)..ai(mm, k)];
                for (c, a) in ci.iter_mut().zip(col) {
                    *c -= coef * a;
                }
            }
        }
        for k in 0..i {
            let coef = w[ai(k, i)];
            if coef != 0.0 {
                let xc = &x[i + k * mm..mm + k * mm];
                for (c, a) in ci.iter_mut().zip(xc) {
                    *c -= coef * a;
                }
            }
        }
        // ---- (2) H(i)
        let tq = larfg(&mut ci);
        bd.tauq[i0 + i] = tq;
        bd.d[i0 + i] = ci[0];
        ci[0] = 1.0;
        w[ai(i, i)..ai(mm, i)].copy_from_slice(&ci);
        // i+1 < nn always holds in the blocked path (panels leave columns over).
        // ---- (3) Y[i+1.., i]
        let ntail = nn - (i + 1);
        let mut yt = vec![0.0; ntail];
        for (q, c) in (i + 1..nn).enumerate() {
            yt[q] = dot(&w[ai(i, c)..ai(mm, c)], &ci);
        }
        let mut tmp1 = vec![0.0; i];
        let mut tmp2 = vec![0.0; i];
        for k in 0..i {
            tmp1[k] = dot(&w[ai(i, k)..ai(mm, k)], &ci);
            tmp2[k] = dot(&x[i + k * mm..mm + k * mm], &ci);
        }
        for k in 0..i {
            let coef = tmp1[k];
            let yc = &y[(i + 1) + k * nn..nn + k * nn];
            for (a, b) in yt.iter_mut().zip(yc) {
                *a -= coef * b;
            }
        }
        for (q, c) in (i + 1..nn).enumerate() {
            if i > 0 {
                yt[q] -= dot(&w[ai(0, c)..ai(i, c)], &tmp2);
            }
        }
        for v in yt.iter_mut() {
            *v *= tq;
        }
        y[(i + 1) + i * nn..nn + i * nn].copy_from_slice(&yt);
        // ---- update row i: A[i, i+1..nn]
        let mut row: Vec<f64> = (i + 1..nn).map(|c| w[ai(i, c)]).collect();
        for k in 0..=i {
            let coef = w[ai(i, k)];
            if coef != 0.0 {
                let yc = &y[(i + 1) + k * nn..nn + k * nn];
                for (a, b) in row.iter_mut().zip(yc) {
                    *a -= coef * b;
                }
            }
        }
        if i > 0 {
            let xr: Vec<f64> = (0..i).map(|k| x[i + k * mm]).collect();
            for (q, c) in (i + 1..nn).enumerate() {
                row[q] -= dot(&w[ai(0, c)..ai(i, c)], &xr);
            }
        }
        // ---- G(i)
        let tp = larfg(&mut row);
        bd.taup[i0 + i] = tp;
        bd.e[i0 + i] = row[0];
        row[0] = 1.0;
        for (q, c) in (i + 1..nn).enumerate() {
            w[ai(i, c)] = row[q];
        }
        // ---- X[i+1.., i]
        let rt = mm - (i + 1);
        let mut xt = vec![0.0; rt];
        for (q, c) in (i + 1..nn).enumerate() {
            let col = &w[ai(i + 1, c)..ai(mm, c)];
            let rq = row[q];
            for (a, b) in xt.iter_mut().zip(col) {
                *a += rq * b;
            }
        }
        let mut tmp3 = vec![0.0; i + 1];
        for k in 0..=i {
            tmp3[k] = dot(&y[(i + 1) + k * nn..nn + k * nn], &row);
        }
        for k in 0..=i {
            let coef = tmp3[k];
            let col = &w[ai(i + 1, k)..ai(mm, k)];
            for (a, b) in xt.iter_mut().zip(col) {
                *a -= coef * b;
            }
        }
        let mut tmp4 = vec![0.0; i];
        if i > 0 {
            for (q, c) in (i + 1..nn).enumerate() {
                let col = &w[ai(0, c)..ai(i, c)];
                let rq = row[q];
                for (a, b) in tmp4.iter_mut().zip(col) {
                    *a += rq * b;
                }
            }
            for k in 0..i {
                let coef = tmp4[k];
                let xc = &x[(i + 1) + k * mm..mm + k * mm];
                for (a, b) in xt.iter_mut().zip(xc) {
                    *a -= coef * b;
                }
            }
        }
        for v in xt.iter_mut() {
            *v *= tp;
        }
        x[(i + 1) + i * mm..mm + i * mm].copy_from_slice(&xt);
    }
    // ---- trailing update: A22 -= V Y2^T + X2 U, rows nb..mm, cols nb..nn.
    let t_rows = mm - nb;
    let t_cols = nn - nb;
    if t_rows > 0 && t_cols > 0 {
        let (vs, ucols) = {
            // V block: rows nb.. of columns 0..nb; U block: rows 0..nb of columns nb..
            (ai(nb, 0), ai(0, nb))
        };
        let a22_off = ai(nb, nb);
        // Split the borrow: V and U live in `w` too, so take copies of the
        // (small) factor panels.
        let vmat: Vec<f64> = {
            let mut v = vec![0.0; t_rows * nb];
            for k in 0..nb {
                v[k * t_rows..(k + 1) * t_rows].copy_from_slice(&w[vs + k * ld..vs + k * ld + t_rows]);
            }
            v
        };
        let umat: Vec<f64> = {
            let mut u = vec![0.0; nb * t_cols];
            for c in 0..t_cols {
                u[c * nb..(c + 1) * nb].copy_from_slice(&w[ucols + c * ld..ucols + c * ld + nb]);
            }
            u
        };
        let y2 = &y[nb..];
        let x2 = &x[nb..];
        let a22 = &mut w[a22_off..];
        let work = t_rows * t_cols * nb;
        for_each_col_group(a22, ld, t_rows, t_cols, 16, work >= (1 << 23), |j0, ncols, chunk| {
            gemm_serial(t_rows, ncols, nb, -1.0, &vmat, t_rows, false, &y2[j0..], nn, true, 1.0, chunk, ld);
            gemm_serial(t_rows, ncols, nb, -1.0, x2, mm, false, &umat[j0 * nb..], nb, false, 1.0, chunk, ld);
        });
    }
}

#[inline(always)]
fn bidiagonalize_impl(a: &[f64], m: usize, n: usize) -> Bidiag {
    assert!(m >= n && n >= 2);
    let mut bd = Bidiag {
        w: a.to_vec(),
        d: vec![0.0; n],
        e: vec![0.0; n - 1],
        tauq: vec![0.0; n],
        taup: vec![0.0; n - 1],
    };
    let mut w = std::mem::take(&mut bd.w);
    let mut i0 = 0;
    while n - i0 >= BLOCKED_MIN {
        labrd_panel(&mut w, m, n, i0, NBB, &mut bd);
        i0 += NBB;
    }
    gebd2(&mut w, m, n, i0, &mut bd);
    bd.w = w;
    bd
}

avx2_fn! {
    /// Householder bidiagonalisation (blocked, `m >= n >= 2`), AVX2 where available.
    pub fn bidiagonalize(a: &[f64], m: usize, n: usize) -> Bidiag {
        bidiagonalize_impl(a, m, n)
    }
}

// ------------------------------------------------------------------------
// The QR iteration on the bidiagonal (port of nalgebra's, rotations recorded)
// ------------------------------------------------------------------------

struct Iter<'a> {
    upper: bool,
    u: &'a mut RotTarget,
    v: &'a mut RotTarget,
}

impl Iter<'_> {
    /// `u_vecs.rotate_rows(rot)` with the stored sign `s` as given.
    #[inline]
    fn u_push(&mut self, a: usize, b: usize, c: f64, s: f64) {
        self.u.push_pair(a, b, c, s);
    }
    /// `v_t.rotate(rows)` with rotation `(c, s)` == column op with `-s`.
    #[inline]
    fn v_push_rotate(&mut self, a: usize, b: usize, c: f64, s: f64) {
        self.v.push_pair(a, b, c, -s);
    }
}

#[inline]
fn cancel_y(x: f64, y: f64) -> Option<(f64, f64, f64)> {
    if y != 0.0 {
        let (m0, s0) = if x >= 0.0 { (x, 1.0) } else { (-x, -1.0) };
        let denom = (m0 * m0 + y * y).sqrt();
        Some((m0 / denom, -y / (s0 * denom), s0 * denom))
    } else {
        None
    }
}

#[inline]
fn cancel_x(x: f64, y: f64) -> Option<(f64, f64, f64)> {
    if x != 0.0 {
        let (m1, s1) = if y >= 0.0 { (y, 1.0) } else { (-y, -1.0) };
        let denom = (m1 * m1 + x * x).sqrt();
        Some((m1 / denom, (x * s1) / denom, s1 * denom))
    } else {
        None
    }
}

/// `GivensRotation::new(c, s)` (eps = 0): `(c', s', norm)`.
#[inline]
fn givens_new(c: f64, s: f64) -> (f64, f64, f64) {
    let (m0, s0) = if c >= 0.0 { (c, 1.0) } else { (-c, -1.0) };
    let denom = (m0 * m0 + s * s).sqrt();
    if denom > 0.0 {
        let norm = s0 * denom;
        (m0 / denom, s / norm, norm)
    } else {
        (1.0, 0.0, 0.0)
    }
}

fn wilkinson_shift(tmm: f64, tnn: f64, tmn: f64) -> f64 {
    let sq_tmn = tmn * tmn;
    if sq_tmn != 0.0 {
        let d = (tmm - tnn) * 0.5;
        tnn - sq_tmn / (d + d.signum() * (d * d + sq_tmn).sqrt())
    } else {
        tnn
    }
}

fn compute_2x2_uptrig_svd(m11: f64, m12: f64, m22: f64) -> ((f64, f64), [f64; 2], (f64, f64)) {
    let denom = (m11 + m22).hypot(m12) + (m11 - m22).hypot(m12);
    let mut v1 = m11 * m22 * 2.0 / denom;
    let mut v2 = 0.5 * denom;
    let (csv_c, csv_s, sgn_v) = givens_new(m11 * m12, v1 * v1 - m11 * m11);
    v1 *= sgn_v;
    v2 *= sgn_v;
    let cu = (m11 * csv_c + m12 * csv_s) / v1;
    let su = (m22 * csv_s) / v1;
    let (csu_c, csu_s, sgn_u) = givens_new(cu, su);
    v1 *= sgn_u;
    v2 *= sgn_u;
    ((csu_c, csu_s), [v1, v2], (csv_c, csv_s))
}

impl Iter<'_> {
    fn cancel_horizontal(&mut self, diag: &mut [f64], off: &mut [f64], i: usize, end: usize) {
        let (mut vx, mut vy) = (off[i], diag[i + 1]);
        off[i] = 0.0;
        for k in i..end {
            match cancel_x(vx, vy) {
                Some((c, s, norm)) => {
                    diag[k + 1] = norm;
                    if self.upper {
                        // u: rot.inverse().rotate_rows(columns i, k+1)
                        self.u_push(i, k + 1, c, -s);
                    } else {
                        // v_t: rot.rotate(rows i, k+1)
                        self.v_push_rotate(i, k + 1, c, s);
                    }
                    if k + 1 != end {
                        vx = -s * off[k + 1];
                        vy = diag[k + 2];
                        off[k + 1] *= c;
                    }
                }
                None => break,
            }
        }
    }

    fn cancel_vertical(&mut self, diag: &mut [f64], off: &mut [f64], i: usize) {
        let (mut vx, mut vy) = (diag[i], off[i]);
        off[i] = 0.0;
        for k in (0..i + 1).rev() {
            match cancel_y(vx, vy) {
                Some((c, s, norm)) => {
                    diag[k] = norm;
                    if self.upper {
                        // v_t: rot.rotate(rows k, i+1)
                        self.v_push_rotate(k, i + 1, c, s);
                    } else {
                        // u: rot.inverse().rotate_rows(columns k, i+1)
                        self.u_push(k, i + 1, c, -s);
                    }
                    if k > 0 {
                        vx = diag[k - 1];
                        vy = s * off[k - 1];
                        off[k - 1] *= c;
                    }
                }
                None => break,
            }
        }
    }

    fn delimit(&mut self, diag: &mut [f64], off: &mut [f64], end: usize, eps: f64) -> (usize, usize) {
        let mut n = end;
        while n > 0 {
            let m = n - 1;
            if off[m] == 0.0 || off[m].abs() <= eps * (diag[n].abs() + diag[m].abs()) {
                off[m] = 0.0;
            } else if diag[m].abs() <= eps {
                diag[m] = 0.0;
                self.cancel_horizontal(diag, off, m, m + 1);
                if m != 0 {
                    self.cancel_vertical(diag, off, m - 1);
                }
            } else if diag[n].abs() <= eps {
                diag[n] = 0.0;
                self.cancel_vertical(diag, off, m);
            } else {
                break;
            }
            n -= 1;
        }
        if n == 0 {
            return (0, 0);
        }
        let mut new_start = n - 1;
        while new_start > 0 {
            let m = new_start - 1;
            if off[m].abs() <= eps * (diag[new_start].abs() + diag[m].abs()) {
                off[m] = 0.0;
                break;
            } else if diag[m].abs() <= eps {
                diag[m] = 0.0;
                self.cancel_horizontal(diag, off, m, n);
                if m != 0 {
                    self.cancel_vertical(diag, off, m - 1);
                }
                break;
            }
            new_start -= 1;
        }
        (new_start, n)
    }

    fn run(&mut self, diag: &mut [f64], off: &mut [f64]) {
        let dim = diag.len();
        let eps = f64::EPSILON * 5.0;
        let (mut start, mut end) = self.delimit(diag, off, dim - 1, eps);
        while end != start {
            let subdim = end - start + 1;
            if subdim > 2 {
                let m = end - 1;
                let n = end;
                let (mut vx, mut vy);
                {
                    let dm = diag[m];
                    let dn = diag[n];
                    let fm = off[m];
                    let tmm = dm * dm + off[m - 1] * off[m - 1];
                    let tmn = dm * fm;
                    let tnn = dn * dn + fm * fm;
                    let shift = wilkinson_shift(tmm, tnn, tmn);
                    vx = diag[start] * diag[start] - shift;
                    vy = diag[start] * off[start];
                }
                for k in start..n {
                    let m12 = if k == n - 1 { 0.0 } else { off[k + 1] };
                    // subm = [[d_k, e_k, 0], [0, d_{k+1}, m12]]
                    let mut s00 = diag[k];
                    let mut s01 = off[k];
                    let mut s02 = 0.0;
                    let mut s10 = 0.0;
                    let mut s11 = diag[k + 1];
                    let mut s12 = m12;
                    match cancel_y(vx, vy) {
                        Some((c1, sn1, norm1)) => {
                            // rot1.inverse().rotate_rows(subm columns 0,1): s' = -sn1
                            {
                                let sp = -sn1;
                                let (a0, b0) = (s00, s01);
                                s00 = a0 * c1 + sp * b0;
                                s01 = -sp * a0 + b0 * c1;
                                let (a1, b1) = (s10, s11);
                                s10 = a1 * c1 + sp * b1;
                                s11 = -sp * a1 + b1 * c1;
                            }
                            if k > start {
                                off[k - 1] = norm1;
                            }
                            let (c2, sn2, norm2) = cancel_y(s00, s10).unwrap_or((1.0, 0.0, s00));
                            // rot2.rotate(subm columns 1,2): rows 0,1
                            {
                                let (a, b) = (s01, s11);
                                s01 = a * c2 - sn2 * b;
                                s11 = sn2 * a + b * c2;
                                let (a, b) = (s02, s12);
                                s02 = a * c2 - sn2 * b;
                                s12 = sn2 * a + b * c2;
                            }
                            s00 = norm2;
                            let _ = s10;
                            if self.upper {
                                self.v_push_rotate(k, k + 1, c1, sn1);
                                self.u_push(k, k + 1, c2, -sn2);
                            } else {
                                self.v_push_rotate(k, k + 1, c2, sn2);
                                self.u_push(k, k + 1, c1, -sn1);
                            }
                            diag[k] = s00;
                            diag[k + 1] = s11;
                            off[k] = s01;
                            if k != n - 1 {
                                off[k + 1] = s12;
                            }
                            vx = s01;
                            vy = s02;
                        }
                        None => break,
                    }
                }
            } else if subdim == 2 {
                let ((cu, su), s, (cv, sv)) =
                    compute_2x2_uptrig_svd(diag[start], off[start], diag[start + 1]);
                // u2 = rotation producing the left factor, v2 the right one (both computed)
                let (u2, v2) = ((cu, su), (cv, sv));
                diag[start] = s[0];
                diag[start + 1] = s[1];
                off[start] = 0.0;
                let urot = if self.upper { u2 } else { v2 };
                self.u_push(start, start + 1, urot.0, urot.1);
                let vrot = if self.upper { v2 } else { u2 };
                // rot.inverse().rotate(v_t rows) == v_push_rotate with -s
                self.v_push_rotate(start, start + 1, vrot.0, -vrot.1);
                end -= 1;
            }
            let sub = self.delimit(diag, off, end, eps);
            start = sub.0;
            end = sub.1;
        }
    }
}

/// Thin SVD of the column-major `m x n` finite matrix `a` (`min(m, n) >= 2`).
pub fn svd(a: &[f64], m: usize, n: usize) -> SvdOut {
    let k = m.min(n);
    assert!(k >= 2);
    let amax = a.iter().fold(0.0f64, |mx, v| mx.max(v.abs()));
    let scaled: Vec<f64>;
    let src: &[f64] = if amax != 0.0 {
        scaled = a.iter().map(|v| v / amax).collect();
        &scaled
    } else {
        a
    };
    // Tall (or square): bidiagonalise A. Wide: bidiagonalise A^T; the lower
    // bidiagonal form of A is its transpose and the roles of the two
    // orthogonal factors swap.
    let wide = m < n;
    let (rows, cols) = if wide { (n, m) } else { (m, n) };
    let tr;
    let work: &[f64] = if wide {
        tr = transpose(src, m, n);
        &tr
    } else {
        src
    };
    let bd = bidiagonalize(work, rows, cols);
    // ---- orthogonal factors of the bidiagonalisation
    let q0 = orgqr(&bd.w, rows, rows, cols, cols, &bd.tauq); // rows x cols
    let mp = cols - 1;
    let mut ap = vec![0.0; mp * mp];
    for i in 0..mp {
        for r in i..mp {
            ap[r + i * mp] = bd.w[i + (r + 1) * rows];
        }
    }
    let p0 = orgqr(&ap, mp, mp, mp, mp, &bd.taup); // mp x mp
    // V0 = diag(1, P0), cols x cols
    let mut v0 = vec![0.0; cols * cols];
    v0[0] = 1.0;
    for c in 0..mp {
        v0[(c + 1) * cols + 1..(c + 2) * cols].copy_from_slice(&p0[c * mp..(c + 1) * mp]);
    }
    // ---- sign normalisation making the bidiagonal non-negative
    let mut su = vec![1.0; cols];
    let mut sv = vec![1.0; cols];
    for i in 0..cols {
        su[i] = sgn(bd.d[i] * sv[i]);
        if i + 1 < cols {
            sv[i + 1] = sgn(su[i] * bd.e[i]);
        }
    }
    let mut q = q0;
    for (j, s) in su.iter().enumerate() {
        if *s < 0.0 {
            for v in &mut q[j * rows..(j + 1) * rows] {
                *v = -*v;
            }
        }
    }
    for (j, s) in sv.iter().enumerate() {
        if *s < 0.0 {
            for v in &mut v0[j * cols..(j + 1) * cols] {
                *v = -*v;
            }
        }
    }
    let mut diag: Vec<f64> = bd.d.iter().map(|v| v.abs()).collect();
    let mut off: Vec<f64> = bd.e.iter().map(|v| v.abs()).collect();
    // upper case: U-side factor = q (rows x k), V-side = v0 (cols x k)
    // wide case:  A = V_B S U_B^T  => U_A = v0, V_A = q
    let (u_init, u_rows, v_init, v_rows) = if wide {
        (v0, cols, q, rows)
    } else {
        (q, rows, v0, cols)
    };
    let mut ut = RotTarget::new(&u_init, u_rows, k);
    let mut vtg = RotTarget::new(&v_init, v_rows, k);
    drop(u_init);
    drop(v_init);
    {
        let mut it = Iter { upper: !wide, u: &mut ut, v: &mut vtg };
        it.run(&mut diag, &mut off);
    }
    for v in &mut diag {
        *v *= amax;
    }
    // Non-negative singular values: flip the U column otherwise.
    for (i, v) in diag.iter_mut().enumerate() {
        if *v < 0.0 {
            *v = -*v;
            ut.negate_column(i);
        }
    }
    let u_fin = ut.finish(); // u_rows x k
    let v_fin = vtg.finish(); // v_rows x k
    // ---- descending order
    let mut perm: Vec<usize> = (0..k).collect();
    perm.sort_by(|&a, &b| diag[b].partial_cmp(&diag[a]).unwrap_or(std::cmp::Ordering::Equal));
    let s: Vec<f64> = perm.iter().map(|&i| diag[i]).collect();
    let mut u = vec![0.0; u_rows * k];
    for (jn, &jo) in perm.iter().enumerate() {
        u[jn * u_rows..(jn + 1) * u_rows].copy_from_slice(&u_fin[jo * u_rows..(jo + 1) * u_rows]);
    }
    // V^T: k x v_rows
    let mut vt = vec![0.0; k * v_rows];
    for (jn, &jo) in perm.iter().enumerate() {
        for r in 0..v_rows {
            vt[jn + r * k] = v_fin[r + jo * v_rows];
        }
    }
    SvdOut { u, s, vt }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::DMatrix;

    fn pseudo(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect()
    }

    fn check(m: usize, n: usize, a: Vec<f64>, compare: bool) {
        let k = m.min(n);
        let r = svd(&a, m, n);
        let am = DMatrix::from_column_slice(m, n, &a);
        let um = DMatrix::from_column_slice(m, k, &r.u);
        let vtm = DMatrix::from_column_slice(k, n, &r.vt);
        let scale = am.amax().max(1e-300);
        let mut us = um.clone();
        for j in 0..k {
            for i in 0..m {
                us[(i, j)] *= r.s[j];
            }
        }
        let recon = (&us * &vtm - &am).amax() / scale;
        assert!(recon < 1e-12 * (k as f64), "recon {recon} for {m}x{n}");
        let ou = (um.transpose() * &um - DMatrix::identity(k, k)).amax();
        let ov = (&vtm * vtm.transpose() - DMatrix::identity(k, k)).amax();
        assert!(ou < 1e-12 * (m as f64) && ov < 1e-12 * (n as f64), "orth {ou} {ov} for {m}x{n}");
        for w in r.s.windows(2) {
            assert!(w[0] >= w[1]);
        }
        assert!(r.s.iter().all(|v| *v >= 0.0));
        if compare {
            let d = am.clone().svd(true, true);
            for (x, y) in r.s.iter().zip(d.singular_values.iter()) {
                assert!((x - y).abs() < 1e-10 * scale, "singular value {x} vs {y} ({m}x{n})");
            }
            let du = (&um - d.u.as_ref().unwrap()).amax();
            let dv = (&vtm - d.v_t.as_ref().unwrap()).amax();
            assert!(du < 1e-6 && dv < 1e-6, "vectors differ from nalgebra: u {du} vt {dv} ({m}x{n})");
        }
    }

    #[test]
    fn matches_nalgebra_tall_wide_square() {
        for &(m, n) in &[
            (4, 4),
            (5, 3),
            (3, 5),
            (31, 31),
            (64, 64),
            (65, 63),
            (63, 65),
            (100, 100),
            (130, 70),
            (70, 130),
            (200, 200),
            (250, 90),
            (90, 250),
            (301, 301),
        ] {
            check(m, n, pseudo(m * n, (m * 1000 + n) as u64), true);
        }
    }

    #[test]
    fn rank_deficient_identity_diagonal_zero_and_repeated() {
        let n = 80;
        let mut id = vec![0.0; n * n];
        for i in 0..n {
            id[i + i * n] = 1.0;
        }
        check(n, n, id, false);
        let mut diag = vec![0.0; n * n];
        for i in 0..n {
            diag[i + i * n] = ((i * 7 % 13) as f64) - 6.0;
        }
        check(n, n, diag, false);
        let u = pseudo(n, 5);
        let v = pseudo(n, 6);
        let mut r1 = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                r1[i + j * n] = u[i] * v[j];
            }
        }
        check(n, n, r1.clone(), false);
        let r = svd(&r1, n, n);
        assert!(r.s[1] < 1e-14 * r.s[0], "rank-1 matrix: s1/s0 = {}", r.s[1] / r.s[0]);
        check(n, n, vec![0.0; n * n], false);
        let mut z = pseudo(n * n, 17);
        for k in [5usize, 40] {
            for i in 0..n {
                z[i + k * n] = 0.0;
                z[k + i * n] = 0.0;
            }
        }
        check(n, n, z, false);
        // tall rank-deficient (duplicate columns)
        let (m2, n2) = (120, 70);
        let mut dup = pseudo(m2 * n2, 23);
        for i in 0..m2 {
            dup[i + 10 * m2] = dup[i + 3 * m2];
            dup[i + 50 * m2] = 2.0 * dup[i + 3 * m2];
        }
        check(m2, n2, dup, false);
    }

    #[test]
    fn hilbert_matrix() {
        let n = 90;
        let mut h = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                h[i + j * n] = 1.0 / ((i + j + 1) as f64);
            }
        }
        check(n, n, h, false);
    }
}
