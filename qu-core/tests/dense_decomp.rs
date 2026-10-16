//! Contract tests for the blocked `qr` / `svd` / `eig` (v0.4.10), through the
//! public `qu_core::linalg` API, at sizes below, at and above the thresholds
//! where the blocked kernels take over from nalgebra's.
//!
//! The contract: reconstruction error, orthogonality, shapes and (for
//! well-conditioned input) agreement with the previous nalgebra implementation
//! to 1e-10 relative; identical error behaviour; deterministic results; and
//! correct handling of rank-deficient, repeated-eigenvalue and exactly-zero
//! inputs.

use nalgebra::DMatrix;
use qu_core::linalg::{self, LinalgError, BLOCKED_EIGGEN_MIN, BLOCKED_EIGSYM_MIN, BLOCKED_QR_MIN, BLOCKED_SVD_MIN};
use qu_core::matrix::Matrix;

fn pseudo(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
        })
        .collect()
}

fn mat(rows: usize, cols: usize, seed: u64) -> Matrix {
    Matrix::from_col_major(rows, cols, pseudo(rows * cols, seed))
}

fn dm(m: &Matrix) -> DMatrix<f64> {
    DMatrix::from_column_slice(m.rows(), m.cols(), m.as_slice())
}

fn sym(n: usize, seed: u64) -> Matrix {
    let r = pseudo(n * n, seed);
    let mut a = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            a[i + j * n] = r[i + j * n] + r[j + i * n];
        }
    }
    Matrix::from_col_major(n, n, a)
}

fn hilbert(n: usize) -> Matrix {
    let mut h = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            h[i + j * n] = 1.0 / ((i + j + 1) as f64);
        }
    }
    Matrix::from_col_major(n, n, h)
}

fn identity(n: usize) -> Matrix {
    let mut h = vec![0.0; n * n];
    for i in 0..n {
        h[i + i * n] = 1.0;
    }
    Matrix::from_col_major(n, n, h)
}

fn rank_one(m: usize, n: usize) -> Matrix {
    let u = pseudo(m, 5);
    let v = pseudo(n, 6);
    let mut a = vec![0.0; m * n];
    for j in 0..n {
        for i in 0..m {
            a[i + j * m] = u[i] * v[j];
        }
    }
    Matrix::from_col_major(m, n, a)
}

/// Sizes straddling every threshold.
fn sizes() -> Vec<usize> {
    let mut v = vec![1usize, 2, 3, 5, 31, 32, 33, 64, 100];
    for t in [BLOCKED_QR_MIN, BLOCKED_SVD_MIN, BLOCKED_EIGSYM_MIN, BLOCKED_EIGGEN_MIN] {
        v.extend([t - 1, t, t + 1]);
    }
    v.extend([150, 200, 257]);
    v.sort_unstable();
    v.dedup();
    v
}

fn check_qr(a: &Matrix, compare_to_nalgebra: bool) {
    let (m, n) = a.shape();
    let k = m.min(n);
    let f = linalg::qr(a).unwrap();
    assert_eq!(f.q.shape(), (m, k));
    assert_eq!(f.r.shape(), (k, n));
    let (q, r, am) = (dm(&f.q), dm(&f.r), dm(a));
    let scale = am.amax().max(1e-300);
    assert!((&q * &r - &am).amax() / scale < 1e-13 * (k as f64).max(1.0), "QR reconstruction {m}x{n}");
    assert!((q.transpose() * &q - DMatrix::identity(k, k)).amax() < 1e-13 * (m as f64).max(1.0), "Q orthogonality {m}x{n}");
    for c in 0..n {
        for rr in (c + 1)..k {
            assert_eq!(r[(rr, c)], 0.0, "R must be upper triangular");
        }
    }
    for i in 0..k {
        assert!(r[(i, i)] >= 0.0, "R diagonal must be non-negative");
    }
    if compare_to_nalgebra {
        let d = am.clone().qr();
        assert!((&q - d.q()).amax() < 1e-10, "Q vs nalgebra {m}x{n}");
        assert!((&r - d.r()).amax() / scale < 1e-10, "R vs nalgebra {m}x{n}");
    }
}

fn check_svd(a: &Matrix, compare_to_nalgebra: bool) {
    let (m, n) = a.shape();
    let k = m.min(n);
    let s = linalg::svd(a).unwrap();
    assert_eq!(s.u.shape(), (m, k));
    assert_eq!(s.v_t.shape(), (k, n));
    assert_eq!(s.singular_values.len(), k);
    let (u, vt, am) = (dm(&s.u), dm(&s.v_t), dm(a));
    let scale = am.amax().max(1e-300);
    let mut us = u.clone();
    for j in 0..k {
        for i in 0..m {
            us[(i, j)] *= s.singular_values[j];
        }
    }
    assert!((&us * &vt - &am).amax() / scale < 1e-12 * (k as f64).max(1.0), "SVD reconstruction {m}x{n}");
    assert!((u.transpose() * &u - DMatrix::identity(k, k)).amax() < 1e-12 * (m as f64).max(1.0), "U orthogonality {m}x{n}");
    assert!((&vt * vt.transpose() - DMatrix::identity(k, k)).amax() < 1e-12 * (n as f64).max(1.0), "V orthogonality {m}x{n}");
    for w in s.singular_values.windows(2) {
        assert!(w[0] >= w[1], "singular values must be descending");
    }
    assert!(s.singular_values.iter().all(|v| *v >= 0.0));
    if compare_to_nalgebra {
        let d = am.clone().svd(true, true);
        for (x, y) in s.singular_values.iter().zip(d.singular_values.iter()) {
            assert!((x - y).abs() <= 1e-10 * scale, "singular values vs nalgebra {m}x{n}");
        }
    }
}

#[test]
fn qr_square_tall_wide_across_thresholds() {
    for &n in &sizes() {
        check_qr(&mat(n, n, n as u64), n >= 3);
    }
    for &(m, n) in &[(300usize, 150usize), (150, 300), (400, BLOCKED_QR_MIN), (BLOCKED_QR_MIN, 400), (1000, 200)] {
        check_qr(&mat(m, n, (m * 7 + n) as u64), true);
    }
}

#[test]
fn qr_degenerate_inputs() {
    let n = BLOCKED_QR_MIN + 8;
    check_qr(&identity(n), true);
    check_qr(&Matrix::from_col_major(n, n, vec![0.0; n * n]), true);
    check_qr(&rank_one(n, n), false);
    check_qr(&hilbert(n), false);
    // exact zero columns and a zero row
    let mut z = pseudo(n * n, 3);
    for i in 0..n {
        z[i + 4 * n] = 0.0;
        z[i + 90 * n] = 0.0;
    }
    for j in 0..n {
        z[17 + j * n] = 0.0;
    }
    check_qr(&Matrix::from_col_major(n, n, z), false);
    // 1x1 and a single column
    check_qr(&Matrix::from_col_major(1, 1, vec![-3.0]), true);
    check_qr(&mat(300, 1, 9), true);
    check_qr(&mat(1, 300, 9), true);
}

#[test]
fn svd_square_tall_wide_across_thresholds() {
    for &n in &sizes() {
        check_svd(&mat(n, n, n as u64 + 1), n >= 3);
    }
    for &(m, n) in &[(300usize, 140usize), (140, 300), (500, BLOCKED_SVD_MIN), (BLOCKED_SVD_MIN, 500), (1000, 130)] {
        check_svd(&mat(m, n, (m * 3 + n) as u64), true);
    }
}

#[test]
fn svd_degenerate_inputs() {
    let n = BLOCKED_SVD_MIN + 8;
    check_svd(&identity(n), false);
    check_svd(&Matrix::from_col_major(n, n, vec![0.0; n * n]), false);
    let r1 = rank_one(n, n);
    check_svd(&r1, false);
    let s = linalg::svd(&r1).unwrap();
    assert!(s.singular_values[1] < 1e-14 * s.singular_values[0], "rank-1 must have one non-zero singular value");
    check_svd(&hilbert(n), false);
    // diagonal with mixed signs and repeated magnitudes
    let mut d = vec![0.0; n * n];
    for i in 0..n {
        d[i + i * n] = ((i % 7) as f64) - 3.0;
    }
    check_svd(&Matrix::from_col_major(n, n, d), false);
    // exact zero rows/cols
    let mut z = pseudo(n * n, 11);
    for k in [3usize, 70] {
        for i in 0..n {
            z[i + k * n] = 0.0;
            z[k + i * n] = 0.0;
        }
    }
    check_svd(&Matrix::from_col_major(n, n, z), false);
    check_svd(&Matrix::from_col_major(1, 1, vec![2.5]), true);
    check_svd(&mat(300, 1, 4), true);
}

#[test]
fn pinv_and_least_squares_use_the_same_svd() {
    // pseudo-inverse of a tall full-rank matrix: A+ A = I
    let a = mat(220, 140, 21);
    let p = linalg::pseudo_inverse(&a, None).unwrap();
    assert_eq!(p.rank, 140);
    let prod = dm(&p.matrix) * dm(&a);
    assert!((prod - DMatrix::identity(140, 140)).amax() < 1e-9);
    // rank-deficient: 1 non-zero singular value
    let r = linalg::pseudo_inverse(&rank_one(150, 150), None).unwrap();
    assert_eq!(r.rank, 1);
}

fn check_sym_eig(a: &Matrix, compare_to_nalgebra: bool) {
    let n = a.rows();
    let e = linalg::eig(a).unwrap();
    assert!(e.values_im.iter().all(|v| *v == 0.0));
    let v = dm(e.vectors.as_ref().expect("symmetric input has eigenvectors"));
    let am = dm(a);
    let scale = am.amax().max(1e-300);
    assert!((v.transpose() * &v - DMatrix::identity(n, n)).amax() < 1e-12 * (n as f64).max(1.0), "eigenvector orthogonality n={n}");
    let mut av = &am * &v;
    for j in 0..n {
        for i in 0..n {
            av[(i, j)] -= v[(i, j)] * e.values_re[j];
        }
    }
    assert!(av.amax() / scale < 1e-12 * (n as f64).max(1.0), "A V - V D residual n={n}");
    if compare_to_nalgebra {
        // The order of the eigenvalues is whatever the QR iteration leaves (it
        // is not sorted, and the docs promise no order), so compare as sets.
        let d = am.clone().symmetric_eigen();
        let mut got = e.values_re.clone();
        let mut want: Vec<f64> = d.eigenvalues.iter().copied().collect();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (x, y) in got.iter().zip(&want) {
            assert!((x - y).abs() <= 1e-10 * scale, "eigenvalue vs nalgebra n={n}: {x} vs {y}");
        }
    }
}

#[test]
fn symmetric_eig_across_thresholds() {
    for &n in &sizes() {
        if n < 2 {
            continue;
        }
        check_sym_eig(&sym(n, n as u64 + 2), n >= 3);
    }
}

#[test]
fn symmetric_eig_degenerate_inputs() {
    let n = BLOCKED_EIGSYM_MIN + 8;
    check_sym_eig(&identity(n), false);
    check_sym_eig(&Matrix::from_col_major(n, n, vec![0.0; n * n]), false);
    // rank one symmetric
    let u = pseudo(n, 31);
    let mut r1 = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            r1[i + j * n] = u[i] * u[j];
        }
    }
    check_sym_eig(&Matrix::from_col_major(n, n, r1), false);
    // repeated eigenvalues: three copies of one block, zero elsewhere
    let blk = sym(8, 3);
    let mut rep = vec![0.0; n * n];
    for b in 0..3 {
        for j in 0..8 {
            for i in 0..8 {
                rep[(b * 8 + i) + (b * 8 + j) * n] = blk.as_slice()[i + j * 8];
            }
        }
    }
    check_sym_eig(&Matrix::from_col_major(n, n, rep), false);
    // Hilbert matrix: backward-stable even though cond ~ 1e20
    check_sym_eig(&hilbert(n), false);
    // exact zeros row/col, diagonal
    let mut z = sym(n, 41).as_slice().to_vec();
    for k in [2usize, 60] {
        for i in 0..n {
            z[i + k * n] = 0.0;
            z[k + i * n] = 0.0;
        }
    }
    check_sym_eig(&Matrix::from_col_major(n, n, z), false);
    // eigenvalue multiplicity is reported
    let e = linalg::eig(&identity(n)).unwrap();
    assert!(e.values_re.iter().all(|v| (v - 1.0).abs() < 1e-14));
    // Slightly asymmetric input (within the 1e-9 tolerance that selects the
    // symmetric path): the decomposition is of the lower triangle, exactly as
    // with the previous implementation.
    let mut s = sym(n, 51).as_slice().to_vec();
    for j in 1..n {
        for i in 0..j {
            s[i + j * n] += 5e-12;
        }
    }
    let m = Matrix::from_col_major(n, n, s);
    let e = linalg::eig(&m).unwrap();
    let d = dm(&m).symmetric_eigen();
    for (x, y) in e.values_re.iter().zip(d.eigenvalues.iter()) {
        assert!((x - y).abs() <= 1e-10 * 10.0);
    }
}

#[test]
fn nonsymmetric_eig_values_only_and_across_thresholds() {
    for &n in &sizes() {
        if n < 2 {
            continue;
        }
        let a = mat(n, n, n as u64 + 3);
        let e = linalg::eig(&a).unwrap();
        assert!(e.vectors.is_none(), "non-symmetric eig reports eigenvalues only");
        assert_eq!(e.values_re.len(), n);
        // trace and sum of squares of eigenvalues = trace(A) and trace(A^2)
        let am = dm(&a);
        let tr: f64 = (0..n).map(|i| am[(i, i)]).sum();
        let s: f64 = e.values_re.iter().sum();
        let scale = am.amax().max(1e-300);
        assert!((tr - s).abs() < 1e-9 * scale * n as f64, "trace n={n}");
        let a2 = &am * &am;
        let tr2: f64 = (0..n).map(|i| a2[(i, i)]).sum();
        let s2: f64 = e.values_re.iter().zip(&e.values_im).map(|(r, i)| r * r - i * i).sum();
        assert!((tr2 - s2).abs() < 1e-8 * scale * scale * n as f64, "trace(A^2) n={n}");
        if n >= 3 {
            let reference = am.clone().schur().complex_eigenvalues();
            for i in 0..n {
                // same set (compare unsorted multiset by greedy matching)
                let _ = i;
            }
            let mut pool: Vec<(f64, f64)> = reference.iter().map(|c| (c.re, c.im)).collect();
            for i in 0..n {
                let p = pool
                    .iter()
                    .position(|c| (c.0 - e.values_re[i]).abs() < 1e-8 * scale && (c.1 - e.values_im[i]).abs() < 1e-8 * scale)
                    .unwrap_or_else(|| panic!("eigenvalue {i} of n={n} missing from the reference"));
                pool.swap_remove(p);
            }
        }
    }
}

#[test]
fn nonsymmetric_eig_special_inputs() {
    let n = BLOCKED_EIGGEN_MIN + 40;
    // upper triangular: eigenvalues are the diagonal
    let mut up = pseudo(n * n, 61);
    for j in 0..n {
        for i in j + 1..n {
            up[i + j * n] = 0.0;
        }
    }
    let e = linalg::eig(&Matrix::from_col_major(n, n, up.clone())).unwrap();
    let mut want: Vec<f64> = (0..n).map(|i| up[i + i * n]).collect();
    let mut got = e.values_re.clone();
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-10);
    }
    // rotation-like block: complex pairs
    let mut rot = vec![0.0; n * n];
    for b in 0..n / 2 {
        let (i, j) = (2 * b, 2 * b + 1);
        rot[i + i * n] = 0.0;
        rot[j + j * n] = 0.0;
        rot[i + j * n] = -2.0;
        rot[j + i * n] = 2.0;
    }
    if n % 2 == 1 {
        rot[(n - 1) + (n - 1) * n] = 1.0;
    }
    // make it non-symmetric overall so the general path is taken
    let e = linalg::eig(&Matrix::from_col_major(n, n, rot)).unwrap();
    let complex = e.values_im.iter().filter(|v| v.abs() > 1.0).count();
    assert_eq!(complex, n / 2 * 2, "every 2x2 rotation block gives a conjugate pair +-2i");
    // a cyclic permutation matrix has the n-th roots of unity; the reference
    // iteration never terminates on it
    for &m in &[5usize, BLOCKED_EIGGEN_MIN + 7, 90] {
        let mut perm = vec![0.0; m * m];
        for i in 0..m {
            perm[((i + 1) % m) + i * m] = 1.0;
        }
        let e = linalg::eig(&Matrix::from_col_major(m, m, perm)).unwrap();
        for i in 0..m {
            let r = (e.values_re[i].powi(2) + e.values_im[i].powi(2)).sqrt();
            assert!((r - 1.0).abs() < 1e-8, "|root| = {r} (m={m})");
        }
    }
    // zero matrix and identity are symmetric -> symmetric path; a nilpotent shift is not
    let mut shift = vec![0.0; n * n];
    for i in 0..n - 1 {
        shift[(i + 1) + i * n] = 1.0;
    }
    let e = linalg::eig(&Matrix::from_col_major(n, n, shift)).unwrap();
    assert!(e.values_re.iter().all(|v| v.abs() < 1e-2) && e.values_im.iter().all(|v| v.abs() < 1e-2));
}

#[test]
fn error_behaviour_is_unchanged() {
    let empty = Matrix::from_col_major(0, 0, vec![]);
    assert_eq!(linalg::qr(&empty).unwrap_err(), LinalgError::EmptyMatrix);
    assert_eq!(linalg::svd(&empty).unwrap_err(), LinalgError::EmptyMatrix);
    assert_eq!(linalg::eig(&empty).unwrap_err(), LinalgError::EmptyMatrix);
    for &n in &[4usize, BLOCKED_QR_MIN + 3] {
        let mut a = pseudo(n * n, 1);
        a[n + 2] = f64::NAN;
        let nan = Matrix::from_col_major(n, n, a.clone());
        assert_eq!(linalg::qr(&nan).unwrap_err(), LinalgError::NonFiniteInput);
        assert_eq!(linalg::svd(&nan).unwrap_err(), LinalgError::NonFiniteInput);
        assert_eq!(linalg::eig(&nan).unwrap_err(), LinalgError::NonFiniteInput);
        a[n + 2] = f64::INFINITY;
        let inf = Matrix::from_col_major(n, n, a);
        assert_eq!(linalg::eig(&inf).unwrap_err(), LinalgError::NonFiniteInput);
    }
    for &(m, n) in &[(3usize, 4usize), (BLOCKED_EIGSYM_MIN + 5, BLOCKED_EIGSYM_MIN + 6)] {
        let r = Matrix::from_col_major(m, n, pseudo(m * n, 2));
        assert_eq!(linalg::eig(&r).unwrap_err(), LinalgError::NotSquare { rows: m, cols: n });
    }
}

#[test]
fn results_are_deterministic_run_to_run() {
    let n = 260;
    let a = mat(n, n, 99);
    let s = sym(n, 98);
    let (q1, q2) = (linalg::qr(&a).unwrap(), linalg::qr(&a).unwrap());
    assert_eq!(q1, q2);
    let (v1, v2) = (linalg::svd(&a).unwrap(), linalg::svd(&a).unwrap());
    assert_eq!(v1, v2);
    let (e1, e2) = (linalg::eig(&s).unwrap(), linalg::eig(&s).unwrap());
    assert_eq!(e1, e2);
    let (g1, g2) = (linalg::eig(&a).unwrap(), linalg::eig(&a).unwrap());
    assert_eq!(g1, g2);
}

#[test]
fn eigenvalue_vector_pairing_is_consistent_in_the_reported_order() {
    // Whatever order the iteration leaves them in, column j of the eigenvector
    // matrix must belong to values_re[j].
    for &n in &[BLOCKED_EIGSYM_MIN, BLOCKED_EIGSYM_MIN + 33, 200] {
        let a = sym(n, 7 * n as u64);
        let e = linalg::eig(&a).unwrap();
        let v = dm(e.vectors.as_ref().unwrap());
        let am = dm(&a);
        for j in 0..n {
            let col = v.column(j);
            let r = (&am * col - col * e.values_re[j]).amax();
            assert!(r < 1e-11 * am.amax() * n as f64, "column {j} is not the eigenvector of values[{j}] (n={n})");
        }
    }
}
