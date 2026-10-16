//! Symmetric eigendecomposition for large dense matrices, v0.4.10.
//!
//! The same algorithm as the `nalgebra::SymmetricEigen` this replaces above
//! the size threshold -- Householder tridiagonalisation, then implicit
//! symmetric QR with Wilkinson shifts on the tridiagonal -- restructured so
//! the cost moves off memory-bound rank-1 updates:
//!
//! * the tridiagonal reduction runs its matrix-vector product and rank-2
//!   update column-parallel (rayon, when the `parallel` feature is on);
//! * `Q` is formed with compact-WY blocks on the GEMM (`dense_qr::orgqr`);
//! * the QR iteration only ever touches `diag`/`off_diag` (O(n^2)); the
//!   Givens rotations it generates are *recorded* and applied to `Q` later in
//!   cache-sized row blocks, one block per task (`RotTarget`). Each row of
//!   `Q` sees exactly the rotations, in exactly the order, that the in-line
//!   application would give it, so the result is the same as applying them
//!   one at a time.
//!
//! Conventions match nalgebra: the matrix is scaled by its largest entry
//! first, only the lower triangle is read, the tridiagonal has non-negative
//! off-diagonals with the sign carried by `Q`, and the eigenvalues come out in
//! the order the QR iteration leaves them (not sorted).

use crate::dense_kernels::{avx2_fn, dot, fill_with, for_each_col_group, for_each_col_mut, gemm_serial, RotTarget};
use crate::dense_qr::{larfg, orgqr};

/// Panel width of the blocked reduction.
const NBT: usize = 32;
/// Below this order the unblocked reduction is used (the panel bookkeeping
/// does not pay for itself).
const BLOCKED_MIN: usize = 96;
/// The matrix-vector product of one reduction step runs in parallel once the
/// trailing block is at least this many columns wide.
const PAR_SYMV_M: usize = 320;

/// `y = B v` for the symmetric `len x len` matrix `B` whose LOWER triangle is
/// stored (column-major, leading dimension `ld`, starting at `b[0]`). Each
/// stored element is read once and used twice.
#[inline(always)]
fn symv_lower(b: &[f64], ld: usize, len: usize, v: &[f64], y: &mut [f64]) {
    for yi in y.iter_mut() {
        *yi = 0.0;
    }
    let mut c = 0;
    // Four columns at a time: one pass over y and v serves four columns.
    while c + 4 <= len {
        let c0 = &b[c * ld..c * ld + len];
        let c1 = &b[(c + 1) * ld..(c + 1) * ld + len];
        let c2 = &b[(c + 2) * ld..(c + 2) * ld + len];
        let c3 = &b[(c + 3) * ld..(c + 3) * ld + len];
        let (v0, v1, v2, v3) = (v[c], v[c + 1], v[c + 2], v[c + 3]);
        // The 4x4 diagonal block (lower triangle valid), handled directly.
        let blk = [
            [c0[c], c0[c + 1], c0[c + 2], c0[c + 3]],
            [c0[c + 1], c1[c + 1], c1[c + 2], c1[c + 3]],
            [c0[c + 2], c1[c + 2], c2[c + 2], c2[c + 3]],
            [c0[c + 3], c1[c + 3], c2[c + 3], c3[c + 3]],
        ];
        let vv = [v0, v1, v2, v3];
        for (r, row) in blk.iter().enumerate() {
            let mut s = 0.0;
            for (q, bq) in row.iter().enumerate() {
                s += bq * vv[q];
            }
            y[c + r] += s;
        }
        let r0 = c + 4;
        // Lane-wise partial sums: a sequential `acc += ...` chain cannot be
        // vectorised (floating-point addition is not reassociated), four
        // independent lanes can.
        let mut l0 = [0.0f64; 4];
        let mut l1 = [0.0f64; 4];
        let mut l2 = [0.0f64; 4];
        let mut l3 = [0.0f64; 4];
        let (mut t0, mut t1, mut t2, mut t3) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        {
            let (x0, x1, x2, x3) = (&c0[r0..len], &c1[r0..len], &c2[r0..len], &c3[r0..len]);
            let vs = &v[r0..len];
            let ys = &mut y[r0..len];
            let mut y4 = ys.chunks_exact_mut(4);
            let mut v4 = vs.chunks_exact(4);
            let mut p0c = x0.chunks_exact(4);
            let mut p1c = x1.chunks_exact(4);
            let mut p2c = x2.chunks_exact(4);
            let mut p3c = x3.chunks_exact(4);
            for ((((yq, vq), p0), p1), (p2, p3)) in (&mut y4)
                .zip(&mut v4)
                .zip(&mut p0c)
                .zip(&mut p1c)
                .zip((&mut p2c).zip(&mut p3c))
            {
                for l in 0..4 {
                    yq[l] += p0[l] * v0 + p1[l] * v1 + p2[l] * v2 + p3[l] * v3;
                    l0[l] += p0[l] * vq[l];
                    l1[l] += p1[l] * vq[l];
                    l2[l] += p2[l] * vq[l];
                    l3[l] += p3[l] * vq[l];
                }
            }
            let yr = y4.into_remainder();
            for q in 0..yr.len() {
                let vi = v4.remainder()[q];
                let (p0, p1, p2, p3) = (p0c.remainder()[q], p1c.remainder()[q], p2c.remainder()[q], p3c.remainder()[q]);
                yr[q] += p0 * v0 + p1 * v1 + p2 * v2 + p3 * v3;
                t0 += p0 * vi;
                t1 += p1 * vi;
                t2 += p2 * vi;
                t3 += p3 * vi;
            }
        }
        y[c] += (l0[0] + l0[1]) + (l0[2] + l0[3]) + t0;
        y[c + 1] += (l1[0] + l1[1]) + (l1[2] + l1[3]) + t1;
        y[c + 2] += (l2[0] + l2[1]) + (l2[2] + l2[3]) + t2;
        y[c + 3] += (l3[0] + l3[1]) + (l3[2] + l3[3]) + t3;
        c += 4;
    }
    while c < len {
        let col = &b[c * ld..c * ld + len];
        let vc = v[c];
        let mut acc = [0.0f64; 4];
        let cs = &col[c + 1..];
        let ys = &mut y[c + 1..len];
        let vs = &v[c + 1..len];
        let mut cc = cs.chunks_exact(4);
        let mut yc = ys.chunks_exact_mut(4);
        let mut vcn = vs.chunks_exact(4);
        for ((cq, yq), vq) in (&mut cc).zip(&mut yc).zip(&mut vcn) {
            for l in 0..4 {
                yq[l] += cq[l] * vc;
                acc[l] += cq[l] * vq[l];
            }
        }
        let mut tail = 0.0;
        for ((cq, yq), vq) in cc.remainder().iter().zip(yc.into_remainder()).zip(vcn.remainder()) {
            *yq += cq * vc;
            tail += cq * vq;
        }
        y[c] += col[c] * vc + ((acc[0] + acc[1]) + (acc[2] + acc[3])) + tail;
        c += 1;
    }
}

/// Unblocked reduction (LAPACK `sytd2`, lower) on the full symmetric `w`:
/// one matrix-vector product and one rank-2 update per column.
fn reduce_unblocked(w: &mut [f64], n: usize, d: &mut [f64], e: &mut [f64], tau: &mut [f64]) {
    for i in 0..n.saturating_sub(2) {
        let m = n - 1 - i;
        d[i] = w[i + i * n];
        let t = larfg(&mut w[(i + 1) + i * n..n + i * n]);
        tau[i] = t;
        e[i] = w[(i + 1) + i * n];
        if t == 0.0 {
            continue;
        }
        let mut v = w[(i + 1) + i * n..n + i * n].to_vec();
        v[0] = 1.0;
        let par = m >= PAR_SYMV_M;
        let mut p = vec![0.0; m];
        {
            let wr = &*w;
            let vr = &v;
            fill_with(&mut p, par, |r| {
                let col = i + 1 + r;
                t * dot(&wr[(i + 1) + col * n..n + col * n], vr)
            });
        }
        let alpha = -0.5 * t * dot(&p, &v);
        for (pi, vi) in p.iter_mut().zip(&v) {
            *pi += alpha * vi;
        }
        let (pr, vr) = (&p, &v);
        for_each_col_mut(w, n, i + 1, n, par, |c, col| {
            let r = c - (i + 1);
            let (a_, b_) = (pr[r], vr[r]);
            let seg = &mut col[i + 1..n];
            for ((s, vq), pq) in seg.iter_mut().zip(vr).zip(pr) {
                *s -= vq * a_ + pq * b_;
            }
        });
    }
    d[n - 2] = w[(n - 2) + (n - 2) * n];
    e[n - 2] = w[(n - 1) + (n - 2) * n];
    d[n - 1] = w[(n - 1) + (n - 1) * n];
}

/// Blocked reduction (LAPACK `sytrd` / `latrd`, lower): within a panel of
/// `NBT` columns each step needs only ONE pass over the (stale) trailing
/// matrix, for the product `A v`; the rank-2 updates of the previous
/// reflectors in the panel are folded in through the small `V`/`W` panels,
/// and the trailing matrix itself is updated once per panel by two GEMMs.
#[inline(always)]
fn reduce_blocked_impl(w: &mut [f64], n: usize, d: &mut [f64], e: &mut [f64], tau: &mut [f64]) {
    let nref = n - 2; // reflectors with a tail
    let mut i0 = 0;
    while i0 < nref {
        let nbp = NBT.min(nref - i0);
        let pr = n - i0 - 1; // panel rows: global rows i0+1 .. n-1
        let mut vmat = vec![0.0; pr * nbp];
        let mut wmat = vec![0.0; pr * nbp];
        for j in 0..nbp {
            let i = i0 + j;
            // ---- column i of the (stale) matrix, rows i..n, corrected by the
            // reflectors of this panel so far.
            let mut c = w[i + i * n..n + i * n].to_vec();
            if j > 0 {
                // local row of global row i is j-1
                for k in 0..j {
                    let wik = wmat[(j - 1) + k * pr];
                    let vik = vmat[(j - 1) + k * pr];
                    let vcol = &vmat[(j - 1) + k * pr..(k + 1) * pr];
                    let wcol = &wmat[(j - 1) + k * pr..(k + 1) * pr];
                    for ((ci, vv), ww) in c.iter_mut().zip(vcol).zip(wcol) {
                        *ci -= vv * wik + ww * vik;
                    }
                }
            }
            d[i] = c[0];
            let t = larfg(&mut c[1..]);
            tau[i] = t;
            e[i] = c[1];
            // store the reflector tail in the matrix (for forming Q) and in V
            for (r, val) in c[2..].iter().enumerate() {
                w[(i + 2 + r) + i * n] = *val;
            }
            let len = n - 1 - i; // reflector length; local rows j..pr
            debug_assert_eq!(len, pr - j);
            vmat[j + j * pr] = 1.0;
            for (r, val) in c[2..].iter().enumerate() {
                vmat[(j + 1 + r) + j * pr] = *val;
            }
            if t == 0.0 {
                continue; // W column stays zero
            }
            let v: Vec<f64> = vmat[j + j * pr..(j + 1) * pr].to_vec();
            // ---- y = A_stale[i+1.., i+1..] v   (one pass over the trailing block)
            let mut y = vec![0.0; len];
            symv_lower(&w[(i + 1) + (i + 1) * n..], n, len, &v, &mut y);
            // ---- fold in the earlier reflectors of this panel:
            // y -= V (W^T v) + W (V^T v)
            if j > 0 {
                let mut wtv = vec![0.0; j];
                let mut vtv = vec![0.0; j];
                for k in 0..j {
                    wtv[k] = dot(&wmat[j + k * pr..(k + 1) * pr], &v);
                    vtv[k] = dot(&vmat[j + k * pr..(k + 1) * pr], &v);
                }
                for k in 0..j {
                    let (a_, b_) = (wtv[k], vtv[k]);
                    let vcol = &vmat[j + k * pr..(k + 1) * pr];
                    let wcol = &wmat[j + k * pr..(k + 1) * pr];
                    for ((yi, vv), ww) in y.iter_mut().zip(vcol).zip(wcol) {
                        *yi -= vv * a_ + ww * b_;
                    }
                }
            }
            for yi in &mut y {
                *yi *= t;
            }
            let alpha = -0.5 * t * dot(&y, &v);
            for (yi, vi) in y.iter_mut().zip(&v) {
                *yi += alpha * vi;
            }
            wmat[j + j * pr..(j + 1) * pr].copy_from_slice(&y);
        }
        // ---- trailing update A22 -= V2 W2^T + W2 V2^T over global rows/cols
        // i0+nbp .. n-1 (local rows nbp-1 ..).
        let t0 = i0 + nbp;
        let tdim = n - t0;
        if tdim > 0 {
            let off = nbp - 1;
            let (v2, w2) = (&vmat[off..], &wmat[off..]);
            let a22 = &mut w[t0 + t0 * n..];
            // Both products land in the same output; one parallel region.
            let work = tdim * tdim * nbp;
            // Only the lower triangle is maintained: a column group starting at
            // column j0 updates rows j0.. only.
            for_each_col_group(a22, n, tdim, tdim, 16, work >= (1 << 24), |j0, ncols, chunk| {
                let rows = tdim - j0;
                // B^T operands: column block j0.. of W2^T / V2^T = rows j0.. of W2 / V2
                gemm_serial(rows, ncols, nbp, -1.0, &v2[j0..], pr, false, &w2[j0..], pr, true, 1.0, &mut chunk[j0..], n);
                gemm_serial(rows, ncols, nbp, -1.0, &w2[j0..], pr, false, &v2[j0..], pr, true, 1.0, &mut chunk[j0..], n);
            });
        }
        i0 += nbp;
    }
    d[n - 2] = w[(n - 2) + (n - 2) * n];
    e[n - 2] = w[(n - 1) + (n - 2) * n];
    d[n - 1] = w[(n - 1) + (n - 1) * n];
}

avx2_fn! {
    fn reduce_blocked(w: &mut [f64], n: usize, d: &mut [f64], e: &mut [f64], tau: &mut [f64]) {
        reduce_blocked_impl(w, n, d, e, tau)
    }
}

/// Householder reduction `Q^T A Q = T` (tridiagonal) reading the lower
/// triangle of the column-major `n x n` `a`. Returns `(d, e, q)` where `e`
/// are the *signed* sub-diagonals and `q` is `n x n` (only when `want_q`).
pub fn tridiagonalize(a: &[f64], n: usize, want_q: bool) -> (Vec<f64>, Vec<f64>, Option<Vec<f64>>) {
    assert!(n >= 3);
    let mut w = a.to_vec();
    let mut d = vec![0.0; n];
    let mut e = vec![0.0; n - 1];
    let mut tau = vec![0.0; n - 1];
    if n >= BLOCKED_MIN {
        // Only the lower triangle is read and maintained.
        reduce_blocked(&mut w, n, &mut d, &mut e, &mut tau);
    } else {
        // Mirror the lower triangle into the upper one: the unblocked
        // reduction keeps the whole trailing block symmetric.
        for j in 0..n {
            for i in j + 1..n {
                w[j + i * n] = w[i + j * n];
            }
        }
        reduce_unblocked(&mut w, n, &mut d, &mut e, &mut tau);
    }

    if !want_q {
        return (d, e, None);
    }
    // Q = diag(1, Q') with Q' = H_0 ... H_{n-3} from the stored reflectors.
    let mp = n - 1;
    let mut ap = vec![0.0; mp * mp];
    for i in 0..mp {
        for r in i..mp {
            ap[r + i * mp] = w[(r + 1) + i * n];
        }
    }
    let qq = orgqr(&ap, mp, mp, mp, mp, &tau);
    let mut q = vec![0.0; n * n];
    q[0] = 1.0;
    // Column scaling that makes the off-diagonals non-negative.
    let mut s = 1.0f64;
    for c in 0..mp {
        if e[c] < 0.0 {
            s = -s;
        }
        let dst = &mut q[(c + 1) * n + 1..(c + 2) * n];
        let src = &qq[c * mp..(c + 1) * mp];
        if s > 0.0 {
            dst.copy_from_slice(src);
        } else {
            for (x, y) in dst.iter_mut().zip(src) {
                *x = -y;
            }
        }
    }
    (d, e, Some(q))
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

/// `GivensRotation::cancel_y` for real data: `(c, s, r)` or `None` when `y == 0`.
#[inline]
fn cancel_y(x: f64, y: f64) -> Option<(f64, f64, f64)> {
    if y != 0.0 {
        let (m0, s0) = if x >= 0.0 { (x, 1.0) } else { (-x, -1.0) };
        let denom = (m0 * m0 + y * y).sqrt();
        let c = m0 / denom;
        let s = -y / (s0 * denom);
        Some((c, s, s0 * denom))
    } else {
        None
    }
}

fn delimit_subproblem(diag: &[f64], off: &mut [f64], end: usize, eps: f64) -> (usize, usize) {
    let mut n = end;
    while n > 0 {
        let m = n - 1;
        if off[m].abs() > eps * (diag[n].abs() + diag[m].abs()) {
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
        if off[m] == 0.0 || off[m].abs() <= eps * (diag[new_start].abs() + diag[m].abs()) {
            off[m] = 0.0;
            break;
        }
        new_start -= 1;
    }
    (new_start, n)
}

/// Implicit symmetric QR on the tridiagonal `(diag, off)`, recording the
/// rotations into `q` when given. `off` may carry any signs. Eigenvalues are
/// left in `diag` in the order the iteration produces.
pub fn tridiagonal_ql(diag: &mut [f64], off: &mut [f64], mut q: Option<&mut RotTarget>) {
    let dim = diag.len();
    if dim <= 1 {
        return;
    }
    let eps = f64::EPSILON;
    let (mut start, mut end) = delimit_subproblem(diag, off, dim - 1, eps);
    while end != start {
        let subdim = end - start + 1;
        if subdim > 2 {
            let m = end - 1;
            let n = end;
            let mut vx = diag[start] - wilkinson_shift(diag[m], diag[n], off[m]);
            let mut vy = off[start];
            for i in start..n {
                let j = i + 1;
                match cancel_y(vx, vy) {
                    Some((c, s, norm)) => {
                        if i > start {
                            off[i - 1] = norm;
                        }
                        let mii = diag[i];
                        let mjj = diag[j];
                        let mij = off[i];
                        let cc = c * c;
                        let ss = s * s;
                        let cs = c * s;
                        let b = cs * 2.0 * mij;
                        diag[i] = (cc * mii + ss * mjj) - b;
                        diag[j] = (ss * mii + cc * mjj) + b;
                        off[i] = cs * (mii - mjj) + mij * (cc - ss);
                        if i != n - 1 {
                            vx = off[i];
                            vy = -s * off[i + 1];
                            off[i + 1] *= c;
                        }
                        if let Some(q) = q.as_deref_mut() {
                            q.push(i, c, -s);
                        }
                    }
                    None => break,
                }
            }
            if off[m].abs() <= eps * (diag[m].abs() + diag[n].abs()) {
                end -= 1;
            }
        } else if subdim == 2 {
            let h00 = diag[start];
            let h11 = diag[start + 1];
            let h10 = off[start];
            let val = (h00 - h11) * 0.5;
            let discr = h10 * h10 + val * val;
            let sq = discr.sqrt();
            let half_tra = (h00 + h11) * 0.5;
            let ev0 = half_tra + sq;
            let ev1 = half_tra - sq;
            let (bx, by) = if (ev0 - h11).abs() > (ev0 - h00).abs() {
                (ev0 - h11, off[start])
            } else {
                (off[start], ev0 - h00)
            };
            diag[start] = ev0;
            diag[start + 1] = ev1;
            if let Some(q) = q.as_deref_mut() {
                // GivensRotation::try_new(bx, by, eps)
                let (m0, s0) = if bx >= 0.0 { (bx, 1.0) } else { (-bx, -1.0) };
                let denom = (m0 * m0 + by * by).sqrt();
                if denom > eps {
                    let norm = s0 * denom;
                    q.push(start, m0 / denom, by / norm);
                }
            }
            end -= 1;
        }
        let sub = delimit_subproblem(diag, off, end, eps);
        start = sub.0;
        end = sub.1;
    }
}

/// Eigenvalues (in iteration order) and orthonormal eigenvectors (columns)
/// of the symmetric matrix whose lower triangle is the column-major `n x n`
/// `a` (`n >= 3`).
pub fn eigh(a: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let amax = a.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let scaled: Vec<f64>;
    let src: &[f64] = if amax != 0.0 {
        scaled = a.iter().map(|v| v / amax).collect();
        &scaled
    } else {
        a
    };
    let (mut d, e, q) = tridiagonalize(src, n, true);
    let q = q.expect("requested");
    let mut off: Vec<f64> = e.iter().map(|v| v.abs()).collect();
    let mut target = RotTarget::new(&q, n, n);
    tridiagonal_ql(&mut d, &mut off, Some(&mut target));
    let vectors = target.finish();
    if amax != 0.0 {
        for v in &mut d {
            *v *= amax;
        }
    }
    (d, vectors)
}

/// Eigenvalues only (same order as [`eigh`]).
pub fn eigvalsh(a: &[f64], n: usize) -> Vec<f64> {
    let amax = a.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let scaled: Vec<f64>;
    let src: &[f64] = if amax != 0.0 {
        scaled = a.iter().map(|v| v / amax).collect();
        &scaled
    } else {
        a
    };
    let (mut d, e, _) = tridiagonalize(src, n, false);
    let mut off: Vec<f64> = e.iter().map(|v| v.abs()).collect();
    tridiagonal_ql(&mut d, &mut off, None);
    if amax != 0.0 {
        for v in &mut d {
            *v *= amax;
        }
    }
    d
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

    fn sym(n: usize, seed: u64) -> Vec<f64> {
        let r = pseudo(n * n, seed);
        let mut a = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                a[i + j * n] = r[i + j * n] + r[j + i * n];
            }
        }
        a
    }

    fn check(n: usize, a: Vec<f64>, compare: bool) {
        let (vals, vecs) = eigh(&a, n);
        let am = DMatrix::from_column_slice(n, n, &a);
        let vm = DMatrix::from_column_slice(n, n, &vecs);
        let scale = am.amax().max(1e-300);
        let orth = (vm.transpose() * &vm - DMatrix::identity(n, n)).amax();
        assert!(orth < 1e-12 * n as f64, "orth {orth} n={n}");
        let mut av = &am * &vm;
        for j in 0..n {
            for i in 0..n {
                av[(i, j)] -= vecs[i + j * n] * vals[j];
            }
        }
        let resid = av.amax() / scale;
        assert!(resid < 1e-12 * n as f64, "residual {resid} n={n}");
        if compare {
            let d = am.clone().symmetric_eigen();
            for (x, y) in vals.iter().zip(d.eigenvalues.iter()) {
                assert!((x - y).abs() < 1e-10 * scale, "value order/size differs: {x} vs {y}");
            }
            // eigenvectors element-wise (sign included) when eigenvalues are well separated
            let dv = (&vm - &d.eigenvectors).amax();
            assert!(dv < 1e-6, "vectors differ from nalgebra by {dv} (n={n})");
        }
        let vo = eigvalsh(&a, n);
        for (x, y) in vo.iter().zip(&vals) {
            assert!((x - y).abs() < 1e-10 * scale);
        }
    }

    #[test]
    fn matches_nalgebra_and_satisfies_the_decomposition() {
        for &n in &[3usize, 4, 5, 17, 33, 64, 65, 95, 96, 97, 100, 128, 129, 200, 301] {
            check(n, sym(n, n as u64), true);
        }
    }

    #[test]
    fn identity_diagonal_repeated_and_zero_structure() {
        let n = 80;
        let mut id = vec![0.0; n * n];
        for i in 0..n {
            id[i + i * n] = 1.0;
        }
        check(n, id, false);
        let mut diag = vec![0.0; n * n];
        for i in 0..n {
            diag[i + i * n] = (i as f64 * 0.37).sin();
        }
        check(n, diag, false);
        // repeated eigenvalues: block diagonal with three copies of the same block
        let mut rep = vec![0.0; n * n];
        let blk = sym(8, 3);
        for b in 0..3 {
            for j in 0..8 {
                for i in 0..8 {
                    rep[(b * 8 + i) + (b * 8 + j) * n] = blk[i + j * 8];
                }
            }
        }
        check(n, rep, false);
        // rank one
        let u = pseudo(n, 11);
        let mut r1 = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                r1[i + j * n] = u[i] * u[j];
            }
        }
        check(n, r1, false);
        // zero matrix
        check(n, vec![0.0; n * n], false);
        // matrix with exact zero rows/columns
        let mut z = sym(n, 17);
        for k in [5usize, 40] {
            for i in 0..n {
                z[i + k * n] = 0.0;
                z[k + i * n] = 0.0;
            }
        }
        check(n, z, false);
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
        // Hilbert is symmetric positive definite with huge conditioning: the
        // decomposition is still backward stable.
        let (vals, vecs) = eigh(&h, n);
        let am = DMatrix::from_column_slice(n, n, &h);
        let vm = DMatrix::from_column_slice(n, n, &vecs);
        let mut av = &am * &vm;
        for j in 0..n {
            for i in 0..n {
                av[(i, j)] -= vecs[i + j * n] * vals[j];
            }
        }
        assert!(av.amax() < 1e-13);
        let orth = (vm.transpose() * &vm - DMatrix::identity(n, n)).amax();
        assert!(orth < 1e-12);
        let big = vals.iter().cloned().fold(0.0, f64::max);
        let reference = am.clone().symmetric_eigen().eigenvalues.iter().cloned().fold(0.0, f64::max);
        assert!((big - reference).abs() < 1e-12, "largest eigenvalue {big} vs {reference}");
    }
}
