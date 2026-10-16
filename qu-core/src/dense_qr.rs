//! Blocked Householder QR (compact-WY), v0.4.10.
//!
//! LAPACK `geqrf` / `orgqr` structure in safe Rust: factor a narrow panel with
//! unblocked Householder reflections, build the triangular factor `T` of the
//! block reflector `H = I - V T V^T` (`larft`), and apply it to the trailing
//! columns with three GEMM calls (`larfb`). The thin `Q` is formed the same
//! way, block by block from the last, so almost all the O(n^3) work runs on
//! the SIMD/rayon GEMM in `dense_kernels` instead of memory-bound rank-1
//! updates.
//!
//! Convention (identical to the nalgebra routine this replaces): the diagonal
//! of `R` is non-negative, with the sign carried by `Q`. For a full-column-rank
//! input the QR factorisation with a positive `R` diagonal is unique, so the
//! two implementations agree to rounding, not merely up to sign.

use crate::dense_kernels::{avx2_fn, dot, for_each_col_group, gemm_serial, norm2};

/// Panel width. 32 keeps the unblocked panel work small and the GEMM
/// depth large enough to run near the kernel's speed.
pub const NB: usize = 32;

/// `larfg`: generate the reflector for the column `x` (`x[0]` is the pivot).
/// Overwrites `x[1..]` with `v[1..]` (`v[0] == 1` implied), `x[0]` with `beta`,
/// and returns `tau`. `H = I - tau v v^T` maps `x` to `beta e1`,
/// `beta = -sign(x0) * ||x||` (so `sign(x0) >= 0` -> `beta <= 0`).
#[inline(always)]
pub fn larfg(x: &mut [f64]) -> f64 {
    let alpha = x[0];
    let xnorm = norm2(&x[1..]);
    if xnorm == 0.0 {
        return 0.0;
    }
    let norm = (alpha * alpha + xnorm * xnorm).sqrt();
    let beta = if alpha >= 0.0 { -norm } else { norm };
    let tau = (beta - alpha) / beta;
    let scale = 1.0 / (alpha - beta);
    for v in &mut x[1..] {
        *v *= scale;
    }
    x[0] = beta;
    tau
}

/// Apply `H = I - tau v v^T` (v = [1, a[1..]]) from the left to the `c`
/// columns that start at `cols` (each `len` rows, leading dimension `ld`),
/// where the reflector itself lives in `a[v0..v0+len]`.
#[inline(always)]
fn apply_reflector_cols(a: &mut [f64], ld: usize, v_col: usize, row0: usize, len: usize, tau: f64, c0: usize, c1: usize) {
    if tau == 0.0 {
        return;
    }
    // Copy the (short) reflector once so the target columns can be borrowed
    // mutably.
    let mut v = a[row0 + v_col * ld..row0 + v_col * ld + len].to_vec();
    v[0] = 1.0;
    for c in c0..c1 {
        let col = &mut a[row0 + c * ld..row0 + c * ld + len];
        let w = tau * dot(&v, col);
        if w != 0.0 {
            for (x, vi) in col.iter_mut().zip(&v) {
                *x -= w * vi;
            }
        }
    }
}

/// Explicit `mr x nb` unit-lower-trapezoidal `V` from reflector storage whose
/// `j`-th reflector sits in column `col0 + j` with its unit entry at row
/// `row0 + j`.
#[inline(always)]
fn extract_v(a: &[f64], ld: usize, row0: usize, col0: usize, mr: usize, nb: usize) -> Vec<f64> {
    let mut v = vec![0.0; mr * nb];
    for j in 0..nb {
        let src = &a[row0 + j + (col0 + j) * ld..row0 + mr + (col0 + j) * ld];
        let dst = &mut v[j * mr + j..(j + 1) * mr];
        dst.copy_from_slice(src);
        dst[0] = 1.0;
    }
    v
}

/// `larft` (forward, columnwise): the `nb x nb` upper-triangular `T` with
/// `H_0 H_1 ... H_{nb-1} = I - V T V^T`.
#[inline(always)]
fn build_t(v: &[f64], mr: usize, nb: usize, tau: &[f64]) -> Vec<f64> {
    let mut t = vec![0.0; nb * nb];
    for i in 0..nb {
        if tau[i] == 0.0 {
            continue;
        }
        t[i + i * nb] = tau[i];
        if i == 0 {
            continue;
        }
        // t[0..i, i] = -tau_i * V[:, 0..i]^T v_i  (rows i.. only: v_i is zero above)
        let vi = &v[i * mr + i..(i + 1) * mr];
        let mut w = vec![0.0; i];
        for (j, wj) in w.iter_mut().enumerate() {
            *wj = -tau[i] * dot(&v[j * mr + i..(j + 1) * mr], vi);
        }
        // t[0..i, i] = T[0..i, 0..i] * w  (upper triangular)
        for r in 0..i {
            let mut s = 0.0;
            for c in r..i {
                s += t[r + c * nb] * w[c];
            }
            t[r + i * nb] = s;
        }
    }
    t
}

/// `C -= V (T or T^T) V^T C` for `C` (`mr x nc`, leading dimension `c_ld`),
/// `V` explicit `mr x nb`. `transpose_t == true` applies `H^T` (what
/// factorisation needs); `false` applies `H` (what forming `Q` needs).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn larfb_left(v: &[f64], mr: usize, nb: usize, t: &[f64], transpose_t: bool, c: &mut [f64], c_ld: usize, nc: usize) {
    if nc == 0 || mr == 0 {
        return;
    }
    let work = 4 * mr * nb * nc;
    // One parallel region: each group of columns does its own V^T C, T-multiply
    // and rank-nb update, so the columns stay hot in cache across the three
    // steps and there is a single fork/join per block.
    for_each_col_group(c, c_ld, mr, nc, 16, work >= (1 << 25), |_j0, ncols, chunk| {
        // The products are formed transposed (output `ncols x nb`) because the
        // GEMM kernel runs about twice as fast with the long dimension as the
        // row count than with a 32-row output.
        // Wt = C^T V   (ncols x nb)
        let mut wt = vec![0.0; ncols * nb];
        gemm_serial(ncols, nb, mr, 1.0, chunk, c_ld, true, v, mr, false, 0.0, &mut wt, ncols);
        // W2t = Wt op(T)^T  (= (op(T) W)^T; T is upper triangular, stored dense)
        let mut w2t = vec![0.0; ncols * nb];
        gemm_serial(ncols, nb, nb, 1.0, &wt, ncols, false, t, nb, !transpose_t, 0.0, &mut w2t, ncols);
        // C -= V W2 = V W2t^T
        gemm_serial(mr, ncols, nb, -1.0, v, mr, false, &w2t, ncols, true, 1.0, chunk, c_ld);
    });
}

/// Result of the in-place factorisation.
pub struct Factored {
    /// `m x n` column-major: `R` in the upper triangle, reflectors below.
    pub a: Vec<f64>,
    pub tau: Vec<f64>,
}

/// Blocked QR factorisation in place (`geqrf`).
#[inline(always)]
fn geqrf_impl(mut a: Vec<f64>, m: usize, n: usize) -> Factored {
    let k = m.min(n);
    let mut tau = vec![0.0; k];
    let mut j = 0;
    while j < k {
        let jb = NB.min(k - j);
        // ---- panel: columns j..j+jb, rows j..m, unblocked.
        for jj in 0..jb {
            let col = j + jj;
            let len = m - col;
            let t = larfg(&mut a[col + col * m..m + col * m]);
            tau[col] = t;
            if jj + 1 < jb {
                apply_reflector_cols(&mut a, m, col, col, len, t, col + 1, j + jb);
            }
        }
        // ---- trailing update with the block reflector H^T.
        if j + jb < n {
            let mr = m - j;
            let v = extract_v(&a, m, j, j, mr, jb);
            let t = build_t(&v, mr, jb, &tau[j..j + jb]);
            let nc = n - (j + jb);
            let c = &mut a[j + (j + jb) * m..];
            larfb_left(&v, mr, jb, &t, true, c, m, nc);
        }
        j += jb;
    }
    Factored { a, tau }
}

/// Form the thin `Q` (`m x kq`, `kq <= k`) from reflector storage `a`
/// (leading dimension `ld`) and `tau` (`orgqr`). Reflector `i` has its unit
/// entry at row `i` of column `i`.
#[inline(always)]
fn orgqr_impl(a: &[f64], ld: usize, m: usize, k: usize, kq: usize, tau: &[f64]) -> Vec<f64> {
    let mut q = vec![0.0; m * kq];
    for i in 0..kq {
        q[i + i * m] = 1.0;
    }
    // Block starts, applied from the last block to the first.
    let mut starts = Vec::new();
    let mut j = 0;
    while j < k {
        starts.push(j);
        j += NB;
    }
    for &j in starts.iter().rev() {
        let jb = NB.min(k - j);
        let mr = m - j;
        let v = extract_v(a, ld, j, j, mr, jb);
        let t = build_t(&v, mr, jb, &tau[j..j + jb]);
        if j >= kq {
            continue;
        }
        let nc = kq - j;
        let c = &mut q[j + j * m..];
        larfb_left(&v, mr, jb, &t, false, c, m, nc);
    }
    q
}

avx2_fn! {
    /// Blocked QR factorisation in place (`geqrf`), AVX2 where available.
    pub fn geqrf(a: Vec<f64>, m: usize, n: usize) -> Factored {
        geqrf_impl(a, m, n)
    }
}

avx2_fn! {
    /// Form the thin `Q` (`m x kq`, `kq <= k`) from reflector storage `a`
    /// (leading dimension `ld`) and `tau` (`orgqr`). Reflector `i` has its unit
    /// entry at row `i` of column `i`.
    pub fn orgqr(a: &[f64], ld: usize, m: usize, k: usize, kq: usize, tau: &[f64]) -> Vec<f64> {
        orgqr_impl(a, ld, m, k, kq, tau)
    }
}

/// Thin QR of the column-major `m x n` matrix: `Q` is `m x k`, `R` is `k x n`
/// (`k = min(m, n)`), `R` has a non-negative diagonal.
pub fn qr(a: &[f64], m: usize, n: usize) -> (Vec<f64>, Vec<f64>) {
    let k = m.min(n);
    let f = geqrf(a.to_vec(), m, n);
    let mut q = orgqr(&f.a, m, m, k, k, &f.tau);
    let mut r = vec![0.0; k * n];
    for c in 0..n {
        for rr in 0..k.min(c + 1) {
            r[rr + c * k] = f.a[rr + c * m];
        }
    }
    // Make the diagonal of R non-negative; the sign moves into Q.
    for i in 0..k {
        if r[i + i * k] < 0.0 {
            for c in i..n {
                r[i + c * k] = -r[i + c * k];
            }
            for v in &mut q[i * m..(i + 1) * m] {
                *v = -*v;
            }
        }
    }
    (q, r)
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

    fn check(m: usize, n: usize, data: Vec<f64>, compare_nalgebra: bool) {
        let k = m.min(n);
        let (q, r) = qr(&data, m, n);
        let a = DMatrix::from_column_slice(m, n, &data);
        let qm = DMatrix::from_column_slice(m, k, &q);
        let rm = DMatrix::from_column_slice(k, n, &r);
        let scale = a.amax().max(1e-300);
        let recon = (&qm * &rm - &a).amax() / scale;
        assert!(recon < 1e-13 * (k as f64).max(1.0), "recon {recon} for {m}x{n}");
        let orth = (qm.transpose() * &qm - DMatrix::identity(k, k)).amax();
        assert!(orth < 1e-13 * (m as f64).max(1.0), "orth {orth} for {m}x{n}");
        for c in 0..n {
            for rr in (c + 1)..k {
                assert_eq!(rm[(rr, c)], 0.0);
            }
        }
        for i in 0..k {
            assert!(rm[(i, i)] >= 0.0);
        }
        if compare_nalgebra {
            let d = a.clone().qr();
            let (q0, r0) = (d.q(), d.r());
            let dq = (&qm - &q0).amax();
            let dr = (&rm - &r0).amax() / scale;
            assert!(dq < 1e-9 && dr < 1e-10, "vs nalgebra dq {dq} dr {dr} for {m}x{n}");
        }
    }

    #[test]
    fn matches_nalgebra_and_reconstructs_across_block_boundaries() {
        for &(m, n) in &[
            (1, 1),
            (2, 2),
            (5, 3),
            (3, 5),
            (31, 31),
            (32, 32),
            (33, 33),
            (64, 64),
            (65, 40),
            (40, 65),
            (100, 7),
            (7, 100),
            (130, 130),
            (257, 200),
            (200, 257),
        ] {
            check(m, n, pseudo(m * n, (m * 1000 + n) as u64), true);
        }
    }

    #[test]
    fn rank_deficient_zero_columns_identity_and_exact_zeros() {
        // identity
        let n = 70;
        let mut id = vec![0.0; n * n];
        for i in 0..n {
            id[i + i * n] = 1.0;
        }
        check(n, n, id, true);
        // rank-1
        let u = pseudo(n, 5);
        let v = pseudo(n, 6);
        let mut r1 = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                r1[i + j * n] = u[i] * v[j];
            }
        }
        check(n, n, r1, false);
        // matrix with exact zero columns and a zero row block
        let mut z = pseudo(n * n, 7);
        for i in 0..n {
            z[i + 3 * n] = 0.0;
            z[i + 40 * n] = 0.0;
        }
        for j in 0..n {
            z[10 + j * n] = 0.0;
        }
        check(n, n, z, false);
        // all-zero matrix
        check(40, 40, vec![0.0; 1600], true);
        // columns that are already upper triangular with negative diagonal
        let mut tri = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..=j {
                tri[i + j * n] = if i == j { -1.5 } else { 0.25 };
            }
        }
        check(n, n, tri, true);
    }

    #[test]
    fn hilbert_matrix_reconstruction() {
        let n = 80;
        let mut h = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                h[i + j * n] = 1.0 / ((i + j + 1) as f64);
            }
        }
        check(n, n, h, false);
    }
}
