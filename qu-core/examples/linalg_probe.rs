//! Scratch probe for the dense eigenvalue iteration.
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
fn main() {
    let n: usize = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(90);
    let which = std::env::args().nth(2).unwrap_or_else(|| "nalgebra".into());
    let mut perm = vec![0.0; n * n];
    for i in 0..n {
        perm[((i + 1) % n) + i * n] = 1.0;
    }
    if which == "hash" {
        // Bitwise fingerprint of the blocked results: must not depend on the thread count.
        use qu_core::linalg;
        use qu_core::matrix::Matrix;
        let h = |v: &[f64]| v.iter().fold(0u64, |acc, x| acc.rotate_left(7) ^ x.to_bits());
        let a = Matrix::from_col_major(n, n, pseudo(n * n, 5));
        let mut s = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                s[i + j * n] = a.as_slice()[i + j * n] + a.as_slice()[j + i * n];
            }
        }
        let sm = Matrix::from_col_major(n, n, s);
        let q = linalg::qr(&a).unwrap();
        let sv = linalg::svd(&a).unwrap();
        let es = linalg::eig(&sm).unwrap();
        let eg = linalg::eig(&a).unwrap();
        println!(
            "qr {:016x} {:016x} svd {:016x} {:016x} {:016x} eigsym {:016x} {:016x} eiggen {:016x} {:016x}",
            h(q.q.as_slice()),
            h(q.r.as_slice()),
            h(sv.u.as_slice()),
            h(&sv.singular_values),
            h(sv.v_t.as_slice()),
            h(&es.values_re),
            h(es.vectors.as_ref().unwrap().as_slice()),
            h(&eg.values_re),
            h(&eg.values_im)
        );
        return;
    }
    if which == "nalgebra" {
        let m = DMatrix::from_column_slice(n, n, &perm);
        let ev = m.schur().complex_eigenvalues();
        println!("nalgebra converged, first eigenvalue {:?}", ev[0]);
    } else if which == "newrand" {
        let a = pseudo(n * n, 42);
        let t = std::time::Instant::now();
        let (re, im) = qu_core::dense_eiggen::eigenvalues(&a, n).expect("converges");
        println!("new random n={n}: {:.1} ms first ({}, {})", t.elapsed().as_secs_f64() * 1e3, re[0], im[0]);
    } else {
        let (re, im) = qu_core::dense_eiggen::eigenvalues(&perm, n).expect("converges");
        println!("new converged, first eigenvalue ({}, {})", re[0], im[0]);
    }
}
