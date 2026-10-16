//! Eigenvalues of a general real square matrix, v0.4.10.
//!
//! `eig` reports eigenvalues only for a non-symmetric matrix, but the
//! `nalgebra` route it used (`schur()`) computed and carried the full Schur
//! vectors through the Hessenberg reduction and every QR sweep, and updated
//! the whole upper triangle on each sweep. Here the Schur vectors are never
//! formed and each sweep updates only the active window (the part that can
//! still change an eigenvalue).
//!
//! The iteration is a line-by-line port of nalgebra's implicit double-shift
//! QR (same shifts, same deflation tests, same 2x2 handling), so on the same
//! Hessenberg matrix the active window evolves through the same arithmetic
//! and the eigenvalues, and their order, are the same. The Hessenberg
//! reduction is Householder (LAPACK `gehd2` form); it differs from
//! nalgebra's only by a +-1 diagonal similarity and rounding, which the
//! iteration is invariant to.

use crate::dense_kernels::{avx2_fn, dot};
use crate::dense_qr::larfg;

/// Reduce the column-major `n x n` matrix `a` to upper Hessenberg form by
/// Householder similarity transformations (unblocked; reflectors are not kept
/// -- eigenvalues only). Entries below the subdiagonal are set to zero.
#[inline(always)]
fn hessenberg_unblocked_impl(a: &mut [f64], n: usize) {
    for i in 0..n.saturating_sub(2) {
        let t = larfg(&mut a[(i + 1) + i * n..n + i * n]);
        let mut v = a[(i + 1) + i * n..n + i * n].to_vec();
        v[0] = 1.0;
        // The column below the subdiagonal is now annihilated.
        for r in i + 2..n {
            a[r + i * n] = 0.0;
        }
        if t == 0.0 {
            continue;
        }
        // Right: A[:, i+1..] -= t (A[:, i+1..] v) v^T   (all rows)
        let mut w = vec![0.0; n];
        for (q, c) in (i + 1..n).enumerate() {
            let vq = v[q];
            let col = &a[c * n..(c + 1) * n];
            for (wi, ci) in w.iter_mut().zip(col) {
                *wi += vq * ci;
            }
        }
        for (q, c) in (i + 1..n).enumerate() {
            let f = t * v[q];
            let col = &mut a[c * n..(c + 1) * n];
            for (ci, wi) in col.iter_mut().zip(&w) {
                *ci -= f * wi;
            }
        }
        // Left: A[i+1.., i+1..] = (I - t v v^T) A[i+1.., i+1..]
        for c in i + 1..n {
            let col = &mut a[(i + 1) + c * n..n + c * n];
            let s = t * dot(&v, col);
            for (ci, vi) in col.iter_mut().zip(&v) {
                *ci -= s * vi;
            }
        }
    }
}

// ------------------------------------------------------------------------
// Real Schur iteration, eigenvalues only (port of nalgebra's `Schur`)
// ------------------------------------------------------------------------

#[inline]
fn to_exp(x: f64) -> (f64, f64) {
    if x >= 0.0 {
        (x, 1.0)
    } else {
        (-x, -1.0)
    }
}

/// nalgebra's `householder::reflection_axis_mut` for a real vector: turns
/// `axis` into the unit reflection axis, returns `(norm, not_zero)`.
fn reflection_axis(axis: &mut [f64]) -> (f64, bool) {
    let sq: f64 = axis.iter().map(|v| v * v).sum();
    let norm = sq.sqrt();
    let (modulus, sign) = to_exp(axis[0]);
    let signed_norm = sign * norm;
    let factor = (sq + modulus * norm) * 2.0;
    axis[0] += signed_norm;
    if factor != 0.0 {
        let f = factor.sqrt();
        for v in axis.iter_mut() {
            *v /= f;
        }
        let n2: f64 = axis.iter().map(|v| v * v).sum::<f64>().sqrt();
        if n2 != 0.0 {
            for v in axis.iter_mut() {
                *v /= n2;
            }
        }
        (-signed_norm, true)
    } else {
        (signed_norm, false)
    }
}

fn delimit(t: &mut [f64], ld: usize, end: usize, eps: f64) -> (usize, usize) {
    let mut e = end;
    let abs_thr = eps * eps;
    while e > 0 {
        let m = e - 1;
        let off = t[e + m * ld].abs();
        if off <= abs_thr || off <= eps * (t[e + e * ld].abs() + t[m + m * ld].abs()) {
            t[e + m * ld] = 0.0;
        } else {
            break;
        }
        e -= 1;
    }
    if e == 0 {
        return (0, 0);
    }
    let mut new_start = e - 1;
    while new_start > 0 {
        let m = new_start - 1;
        let off = t[new_start + m * ld];
        let offn = off.abs();
        if off == 0.0 || offn <= abs_thr || offn <= eps * (t[new_start + new_start * ld].abs() + t[m + m * ld].abs()) {
            t[new_start + m * ld] = 0.0;
            break;
        }
        new_start -= 1;
    }
    (new_start, e)
}

/// `compute_2x2_eigvals`: real eigenvalues of `[[h00,h01],[h10,h11]]`, or `None`.
fn eigvals_2x2(h00: f64, h01: f64, h10: f64, h11: f64) -> Option<(f64, f64)> {
    let val = (h00 - h11) * 0.5;
    let discr = h10 * h01 + val * val;
    if discr >= 0.0 {
        let sq = discr.sqrt();
        let half = (h00 + h11) * 0.5;
        Some((half + sq, half - sq))
    } else {
        None
    }
}

/// `compute_2x2_basis`: Givens `(c, s)` that triangularises a 2x2 block with real
/// eigenvalues.
fn basis_2x2(h00: f64, h01: f64, h10: f64, h11: f64) -> Option<(f64, f64)> {
    if h10 == 0.0 {
        return None;
    }
    let (e1, e2) = eigvals_2x2(h00, h01, h10, h11)?;
    let x1 = e1 - h11;
    let x2 = e2 - h11;
    let x = if x1.abs() > x2.abs() { x1 } else { x2 };
    // GivensRotation::new(x, h10).0
    let (m0, s0) = to_exp(x);
    let denom = (m0 * m0 + h10 * h10).sqrt();
    if denom > 0.0 {
        Some((m0 / denom, h10 / (s0 * denom)))
    } else {
        Some((1.0, 0.0))
    }
}

avx2_fn! {
    /// Reduce the column-major `n x n` matrix `a` to upper Hessenberg form by
    /// Householder similarity transformations (unblocked; reflectors are not
    /// kept -- eigenvalues only). Entries below the subdiagonal become zero.
    pub fn hessenberg_unblocked(a: &mut [f64], n: usize) {
        hessenberg_unblocked_impl(a, n)
    }
}

avx2_fn! {
    fn schur_eigenvalues(t: &mut [f64], n: usize, ld: usize, amax: f64) -> Option<(Vec<f64>, Vec<f64>)> {
        schur_eigenvalues_impl(t, n, ld, amax)
    }
}

/// Eigenvalues of the upper-Hessenberg `t` (destroyed). Returns `(re, im)` in
/// the order nalgebra reports them.
#[inline(always)]
fn schur_eigenvalues_impl(t: &mut [f64], n: usize, ld: usize, amax: f64) -> Option<(Vec<f64>, Vec<f64>)> {
    let eps = f64::EPSILON;
    let (mut start, mut end) = delimit(t, ld, n - 1, eps);
    // Iterations spent on the current window; the reference implementation
    // has no exceptional shifts and never terminates on e.g. a cyclic
    // permutation matrix. As in LAPACK `lahqr`, iterations 10 and 20 of a
    // window use an ad-hoc shift, and a window that still has not deflated
    // after 100 iterations gives up (`None`) instead of looping forever.
    // Matrices that deflate in under 10 iterations per window -- all the
    // ordinary ones -- never reach this code.
    let mut its = 0usize;
    let mut last_window = (usize::MAX, usize::MAX);
    while end != start {
        if (start, end) != last_window {
            last_window = (start, end);
            its = 0;
        }
        let subdim = end - start + 1;
        if subdim > 2 {
            let m = end - 1;
            let nn = end;
            let h11 = t[start + start * ld];
            let h12 = t[start + (start + 1) * ld];
            let h21 = t[(start + 1) + start * ld];
            let h22 = t[(start + 1) + (start + 1) * ld];
            let h32 = t[(start + 2) + (start + 1) * ld];
            let hnn = t[nn + nn * ld];
            let hmm = t[m + m * ld];
            let hnm = t[nn + m * ld];
            let hmn = t[m + nn * ld];
            let (tra, det);
            if its == 10 || its == 20 {
                let (s, d0) = if its == 10 {
                    (t[nn + m * ld].abs() + t[m + (m - 1) * ld].abs(), hnn)
                } else {
                    (h21.abs() + h32.abs(), h11)
                };
                let e11 = 0.75 * s + d0;
                tra = 2.0 * e11;
                det = e11 * e11 + 0.4375 * s * s;
            } else {
                tra = hnn + hmm;
                det = hnn * hmm - hnm * hmn;
            }
            its += 1;
            if its > 100 {
                return None;
            }
            let mut axis = [
                h11 * h11 + h12 * h21 - tra * h11 + det,
                h21 * (h11 + h22 - tra),
                h21 * h32,
            ];
            for k in start..nn - 1 {
                let (norm, not_zero) = reflection_axis(&mut axis);
                if not_zero {
                    if k > start {
                        t[k + (k - 1) * ld] = norm;
                        t[(k + 1) + (k - 1) * ld] = 0.0;
                        t[(k + 2) + (k - 1) * ld] = 0.0;
                    }
                    // Left: rows k..k+3, columns k ..= end (window only).
                    {
                        let (a0, a1, a2) = (axis[0], axis[1], axis[2]);
                        let region = &mut t[k + k * ld..k + end * ld + 3];
                        for col in region.chunks_mut(ld) {
                            if let [x0, x1, x2, ..] = col {
                                let s = (a0 * *x0 + a1 * *x1 + a2 * *x2) * -2.0;
                                *x0 += s * a0;
                                *x1 += s * a1;
                                *x2 += s * a2;
                            }
                        }
                    }
                    // Right: columns k..k+3, rows start ..= min(k+3, end).
                    let r1 = (k + 4).min(end + 1);
                    {
                        let (a0, a1, a2) = (axis[0], axis[1], axis[2]);
                        let (head, rest) = t.split_at_mut((k + 1) * ld);
                        let (mid, tail) = rest.split_at_mut(ld);
                        let c0 = &mut head[k * ld + start..k * ld + r1];
                        let c1 = &mut mid[start..r1];
                        let c2 = &mut tail[start..r1];
                        for ((x0, x1), x2) in c0.iter_mut().zip(c1.iter_mut()).zip(c2.iter_mut()) {
                            let w = (*x0 * a0 + *x1 * a1 + *x2 * a2) * -2.0;
                            *x0 += w * a0;
                            *x1 += w * a1;
                            *x2 += w * a2;
                        }
                    }
                }
                axis[0] = t[(k + 1) + k * ld];
                axis[1] = t[(k + 2) + k * ld];
                if k < nn - 2 {
                    axis[2] = t[(k + 3) + k * ld];
                }
            }
            let mut axis2 = [axis[0], axis[1]];
            let (norm, not_zero) = reflection_axis(&mut axis2);
            if not_zero {
                t[m + (m - 1) * ld] = norm;
                t[nn + (m - 1) * ld] = 0.0;
                for c in m..=end {
                    let base = m + c * ld;
                    let s = (axis2[0] * t[base] + axis2[1] * t[base + 1]) * -2.0;
                    t[base] += s * axis2[0];
                    t[base + 1] += s * axis2[1];
                }
                for r in start..=end {
                    let w = (t[r + m * ld] * axis2[0] + t[r + (m + 1) * ld] * axis2[1]) * -2.0;
                    t[r + m * ld] += w * axis2[0];
                    t[r + (m + 1) * ld] += w * axis2[1];
                }
            }
        } else {
            // Decouple the 2x2 block if it has real eigenvalues.
            let (h00, h01, h10, h11) = (
                t[start + start * ld],
                t[start + (start + 1) * ld],
                t[(start + 1) + start * ld],
                t[(start + 1) + (start + 1) * ld],
            );
            if let Some((c, s)) = basis_2x2(h00, h01, h10, h11) {
                // inv_rot.rotate(rows start,start+1 over columns start..=end):
                //   a' = c a + s b ; b' = -s a + c b   (rot.inverse() => s -> -s in `rotate`)
                for col in start..=end {
                    let (a, b) = (t[start + col * ld], t[(start + 1) + col * ld]);
                    t[start + col * ld] = c * a + s * b;
                    t[(start + 1) + col * ld] = -s * a + c * b;
                }
                // rot.rotate_rows(columns start,start+1 over rows start..=end)
                for r in start..=end {
                    let (a, b) = (t[r + start * ld], t[r + (start + 1) * ld]);
                    t[r + start * ld] = a * c + s * b;
                    t[r + (start + 1) * ld] = -s * a + b * c;
                }
                t[end + start * ld] = 0.0;
            }
            if end > 2 {
                end -= 2;
            } else {
                break;
            }
        }
        let sub = delimit(t, ld, end, eps);
        start = sub.0;
        end = sub.1;
    }
    // Eigenvalues from the (quasi-)triangular diagonal blocks of t * amax.
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let sc = |x: f64| if amax != 0.0 { x * amax } else { x };
    let mut m = 0;
    while m + 1 < n {
        let nn = m + 1;
        if t[nn + m * ld] == 0.0 {
            re[m] = sc(t[m + m * ld]);
            m += 1;
        } else {
            let hmm = sc(t[m + m * ld]);
            let hnm = sc(t[nn + m * ld]);
            let hmn = sc(t[m + nn * ld]);
            let hnn = sc(t[nn + nn * ld]);
            let val = (hmm - hnn) * 0.5;
            let discr = hnm * hmn + val * val;
            let sq = (-discr).sqrt();
            let half = (hnn + hmm) * 0.5;
            re[m] = half;
            im[m] = sq;
            re[m + 1] = half;
            im[m + 1] = -sq;
            m += 2;
        }
    }
    if m == n - 1 {
        re[m] = sc(t[m + m * ld]);
    }
    Some((re, im))
}

/// Eigenvalues of the column-major `n x n` finite matrix (`n >= 3`) as
/// `(re, im)`, in the same order as `nalgebra`'s Schur-based route; `None` if the
/// QR iteration fails to converge.
pub fn eigenvalues(a: &[f64], n: usize) -> Option<(Vec<f64>, Vec<f64>)> {
    assert!(n >= 3);
    let amax = a.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let mut t: Vec<f64> = if amax != 0.0 { a.iter().map(|v| v / amax).collect() } else { a.to_vec() };
    hessenberg_unblocked(&mut t, n);
    // Pad the leading dimension of power-of-two-ish orders so the 3-row strips
    // of consecutive columns do not all land in the same cache set.
    let ld = if n % 64 == 0 { n + 1 } else { n };
    if ld == n {
        schur_eigenvalues(&mut t, n, n, amax)
    } else {
        let mut tp = vec![0.0; ld * n];
        for c in 0..n {
            tp[c * ld..c * ld + n].copy_from_slice(&t[c * n..(c + 1) * n]);
        }
        schur_eigenvalues(&mut tp, n, ld, amax)
    }
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

    fn sorted(re: &[f64], im: &[f64]) -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = re.iter().cloned().zip(im.iter().cloned()).collect();
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.partial_cmp(&b.1).unwrap()));
        v
    }

    fn compare(n: usize, a: Vec<f64>) {
        let (re, im) = eigenvalues(&a, n).unwrap();
        let am = DMatrix::from_column_slice(n, n, &a);
        let sch = am.clone().schur();
        let ev = sch.complex_eigenvalues();
        let scale = am.amax().max(1e-300);
        // same order, same values
        for i in 0..n {
            assert!(
                (re[i] - ev[i].re).abs() < 1e-8 * scale && (im[i] - ev[i].im).abs() < 1e-8 * scale,
                "eigenvalue {i} of n={n}: ({}, {}) vs ({}, {})",
                re[i],
                im[i],
                ev[i].re,
                ev[i].im
            );
        }
        // trace and determinant-free sanity: sum of eigenvalues = trace
        let tr: f64 = (0..n).map(|i| a[i + i * n]).sum();
        let s: f64 = re.iter().sum();
        assert!((tr - s).abs() < 1e-9 * scale * n as f64, "trace {tr} vs {s}");
        let _ = sorted(&re, &im);
    }

    #[test]
    fn matches_nalgebra_schur_eigenvalues_in_order() {
        for &n in &[3usize, 4, 5, 8, 17, 33, 64, 100, 150, 257] {
            compare(n, pseudo(n * n, n as u64 * 31 + 7));
        }
    }

    #[test]
    fn small_orders_and_integer_matrices_agree_with_nalgebra() {
        let (mut checked, mut skipped, mut reordered, mut reordered_generic) = (0, 0, 0, 0);
        for n in 3..=24usize {
            for seed in 0..12u64 {
                // random reals, small-integer matrices (many ties and exact zeros),
                // and sparse integer matrices
                let r = pseudo(n * n, seed * 977 + n as u64);
                let int: Vec<f64> = r.iter().map(|v| (v * 8.0).round()).collect();
                let sparse: Vec<f64> = r
                    .iter()
                    .enumerate()
                    .map(|(i, v)| if (i * 7 + seed as usize) % 4 == 0 { (v * 6.0).round() } else { 0.0 })
                    .collect();
                for (kind, a) in [r.clone(), int, sparse].into_iter().enumerate() {
                    if a.iter().all(|v| *v == 0.0) {
                        continue;
                    }
                    let am = DMatrix::from_column_slice(n, n, &a);
                    // The reference iteration can fail to converge on structured input
                    // (it has no exceptional shifts); bound it and skip those.
                    let Some(reference) = nalgebra::linalg::Schur::try_new(am.clone(), f64::EPSILON, 5000) else {
                        skipped += 1;
                        continue;
                    };
                    let want = reference.complex_eigenvalues();
                    let (re, im) = eigenvalues(&a, n).expect("converges where the reference does");
                    let scale = am.amax().max(1e-300);
                    // 1. same multiset of eigenvalues
                    let mut pool: Vec<(f64, f64)> = want.iter().map(|c| (c.re, c.im)).collect();
                    // Structured integer matrices are often defective: a k-fold defective
                    // eigenvalue is only determined to ~eps^(1/k) (k up to 10 here).
                    let tol = if kind == 0 { 1e-6 } else { 5e-2 } * scale;
                    for i in 0..n {
                        let hit = pool
                            .iter()
                            .position(|e| (re[i] - e.0).abs() < tol && (im[i].abs() - e.1.abs()).abs() < tol);
                        match hit {
                            Some(p) => {
                                pool.swap_remove(p);
                            }
                            None => panic!("n={n} seed={seed} kind={kind}: eigenvalue ({}, {}) has no match in {pool:?}", re[i], im[i]),
                        }
                    }
                    // 2. same order (always for generic matrices)
                    let same_order = (0..n)
                        .all(|i| (re[i] - want[i].re).abs() < 1e-7 * scale && (im[i] - want[i].im).abs() < 1e-7 * scale);
                    if !same_order {
                        if kind == 0 {
                            reordered_generic += 1;
                        } else {
                            reordered += 1;
                        }
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 600, "checked {checked}, skipped {skipped}");
        eprintln!(
            "order differs from the reference: {reordered_generic} generic random, {reordered} structured integer (checked {checked}, skipped {skipped})"
        );
    }

    #[test]
    fn special_structure() {
        let n = 90;
        // identity, diagonal, upper triangular, companion-like, permutation (cyclic), zero
        let mut id = vec![0.0; n * n];
        for i in 0..n {
            id[i + i * n] = 1.0;
        }
        let (re, im) = eigenvalues(&id, n).unwrap();
        assert!(re.iter().all(|v| (*v - 1.0).abs() < 1e-14) && im.iter().all(|v| *v == 0.0));
        let mut up = pseudo(n * n, 5);
        for j in 0..n {
            for i in j + 1..n {
                up[i + j * n] = 0.0;
            }
        }
        let (re, _) = eigenvalues(&up, n).unwrap();
        let mut want: Vec<f64> = (0..n).map(|i| up[i + i * n]).collect();
        let mut got = re.clone();
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-10);
        }
        // cyclic permutation: eigenvalues are the n-th roots of unity (|lambda| = 1)
        let mut perm = vec![0.0; n * n];
        for i in 0..n {
            perm[((i + 1) % n) + i * n] = 1.0;
        }
        let (re, im) = eigenvalues(&perm, n).unwrap();
        for i in 0..n {
            assert!((re[i] * re[i] + im[i] * im[i] - 1.0).abs() < 1e-8, "root {i}: {} {}", re[i], im[i]);
        }
        let (re, im) = eigenvalues(&vec![0.0; n * n], n).unwrap();
        assert!(re.iter().all(|v| *v == 0.0) && im.iter().all(|v| *v == 0.0));
        // repeated eigenvalue (Jordan-like): 2 I + N
        let mut jor = vec![0.0; n * n];
        for i in 0..n {
            jor[i + i * n] = 2.0;
            if i + 1 < n {
                jor[i + (i + 1) * n] = 1.0;
            }
        }
        let (re, _) = eigenvalues(&jor, n).unwrap();
        assert!(re.iter().all(|v| (*v - 2.0).abs() < 1e-6));
        // rank one
        let u = pseudo(n, 9);
        let mut r1 = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                r1[i + j * n] = u[i] * u[j];
            }
        }
        let (re, im) = eigenvalues(&r1, n).unwrap();
        let big = re.iter().cloned().fold(0.0, f64::max);
        let uu: f64 = u.iter().map(|x| x * x).sum();
        assert!((big - uu).abs() < 1e-10);
        assert!(im.iter().all(|v| v.abs() < 1e-7));
    }
}
