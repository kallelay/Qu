//! In-process timing of the nalgebra reference kernels against the blocked
//! ones: `cargo run --release -p qu-core --features parallel,fast-matmul
//! --example linalg_bench -- [qr|eigsym|svd|eiggen] [n ...]`.
use nalgebra::DMatrix;
use qu_core::*;
use std::time::Instant;

fn pseudo(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
        })
        .collect()
}

fn time<F: FnMut()>(mut f: F, reps: usize) -> (f64, f64, f64) {
    let mut t: Vec<f64> = (0..reps)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed().as_secs_f64()
        })
        .collect();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (t[t.len() / 2], t[0], t[t.len() - 1])
}

fn sweep(op: &str) {
    for &n in &[24usize, 32, 48, 64, 80, 96, 128, 160, 200, 256] {
        let a = pseudo(n * n, 42);
        let reps = if n <= 96 { 60 } else { 15 };
        let mut s = vec![0.0; n * n];
        for j in 0..n { for i in 0..n { s[i + j * n] = a[i + j * n] + a[j + i * n]; } }
        let am = DMatrix::from_column_slice(n, n, &a);
        let sm = DMatrix::from_column_slice(n, n, &s);
        let (old, new) = match op {
            "qr" => (
                time(|| { let d = am.clone().qr(); std::hint::black_box((d.q(), d.r())); }, reps),
                time(|| { std::hint::black_box(dense_qr::qr(&a, n, n)); }, reps),
            ),
            "svd" => (
                time(|| { std::hint::black_box(am.clone().svd(true, true)); }, reps),
                time(|| { std::hint::black_box(dense_svd::svd(&a, n, n)); }, reps),
            ),
            "eigsym" if n >= 3 => (
                time(|| { std::hint::black_box(sm.clone().symmetric_eigen()); }, reps),
                time(|| { std::hint::black_box(dense_eigsym::eigh(&s, n)); }, reps),
            ),
            "eiggen" => (
                time(|| { std::hint::black_box(am.clone().schur().complex_eigenvalues()); }, reps),
                time(|| { std::hint::black_box(dense_eiggen::eigenvalues(&a, n)); }, reps),
            ),
            _ => panic!("op"),
        };
        println!("{op} n={n}: old {:.3} ms  new {:.3} ms  (min-ratio {:.2}x)", old.1 * 1e3, new.1 * 1e3, old.1 / new.1);
    }
}

fn shape(op: &str, m: usize, n: usize) {
    let a = pseudo(m * n, 42);
    let am = DMatrix::from_column_slice(m, n, &a);
    let (old, new) = match op {
        "qr" => (
            time(|| { let d = am.clone().qr(); std::hint::black_box((d.q(), d.r())); }, 5),
            time(|| { std::hint::black_box(dense_qr::qr(&a, m, n)); }, 5),
        ),
        "svd" => (
            time(|| { std::hint::black_box(am.clone().svd(true, true)); }, 5),
            time(|| { std::hint::black_box(dense_svd::svd(&a, m, n)); }, 5),
        ),
        _ => panic!("op"),
    };
    println!("{op} {m}x{n}: old {:.2} ms  new {:.2} ms  ({:.2}x)", old.1 * 1e3, new.1 * 1e3, old.1 / new.1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("shape") {
        shape(&args[1], args[2].parse().unwrap(), args[3].parse().unwrap());
        return;
    }
    if args.first().map(|s| s.as_str()) == Some("sweep") {
        sweep(args.get(1).map(|s| s.as_str()).unwrap_or("qr"));
        return;
    }
    let what = args.first().cloned().unwrap_or_else(|| "qr".into());
    let sizes: Vec<usize> = if args.len() > 1 {
        args[1..].iter().map(|s| s.parse().unwrap()).collect()
    } else {
        vec![100, 200, 500, 1000]
    };
    for &n in &sizes {
        let a = pseudo(n * n, 42);
        let reps = if n >= 1000 { 3 } else { 5 };
        match what.as_str() {
            "qr" => {
                let am = DMatrix::from_column_slice(n, n, &a);
                let old = time(|| { let d = am.clone().qr(); std::hint::black_box((d.q(), d.r())); }, reps);
                let new = time(|| { std::hint::black_box(dense_qr::qr(&a, n, n)); }, reps);
                println!("qr n={n}: old {:.2} ms (min {:.2})  new {:.2} ms (min {:.2})  ({:.2}x med, {:.2}x min)", old.0 * 1e3, old.1 * 1e3, new.0 * 1e3, new.1 * 1e3, old.0 / new.0, old.1 / new.1);
            }
            "eigsym" => {
                let mut s = vec![0.0; n * n];
                for j in 0..n { for i in 0..n { s[i + j * n] = a[i + j * n] + a[j + i * n]; } }
                let sm = DMatrix::from_column_slice(n, n, &s);
                let old = time(|| { std::hint::black_box(sm.clone().symmetric_eigen()); }, reps);
                let new = time(|| { std::hint::black_box(dense_eigsym::eigh(&s, n)); }, reps);
                println!("eigsym n={n}: old {:.2} ms (min {:.2})  new {:.2} ms (min {:.2})  ({:.2}x med, {:.2}x min)", old.0 * 1e3, old.1 * 1e3, new.0 * 1e3, new.1 * 1e3, old.0 / new.0, old.1 / new.1);
            }
            "svd" => {
                let am = DMatrix::from_column_slice(n, n, &a);
                let old = time(|| { std::hint::black_box(am.clone().svd(true, true)); }, reps);
                let new = time(|| { std::hint::black_box(dense_svd::svd(&a, n, n)); }, reps);
                println!("svd n={n}: old {:.2} ms (min {:.2})  new {:.2} ms (min {:.2})  ({:.2}x med, {:.2}x min)", old.0 * 1e3, old.1 * 1e3, new.0 * 1e3, new.1 * 1e3, old.0 / new.0, old.1 / new.1);
            }
            "eiggen" => {
                let am = DMatrix::from_column_slice(n, n, &a);
                let old = time(|| { std::hint::black_box(am.clone().schur().complex_eigenvalues()); }, reps);
                let mid = time(|| { std::hint::black_box(am.complex_eigenvalues()); }, reps);
                let new = time(|| { std::hint::black_box(dense_eiggen::eigenvalues(&a, n)); }, reps);
                let t_h = time(|| { let mut t = a.clone(); dense_eiggen::hessenberg_unblocked(&mut t, n); std::hint::black_box(t); }, reps);
                println!("eiggen n={n}: schur+vals {:.2} ms (min {:.2})  complex_eigenvalues {:.2} ms (min {:.2})  new {:.2} ms (min {:.2}) [hessenberg part {:.2}]", old.0 * 1e3, old.1 * 1e3, mid.0 * 1e3, mid.1 * 1e3, new.0 * 1e3, new.1 * 1e3, t_h.0 * 1e3);
            }
            "gemm" => {
                use qu_core::dense_kernels::gemm_serial;
                let bm = pseudo(n * n, 7);
                for &(m, nn, k, ta, tb) in &[(32usize, n, n, true, false), (32usize, n, n, false, false), (n, 32usize, n, true, false), (n, 32usize, n, false, false), (n, n, 32, false, false), (n, n, n, false, false), (n, n, 64, false, false)] {
                    let aa = pseudo(m.max(k) * m.max(k) + 64, 1);
                    let bb = pseudo(k.max(nn) * k.max(nn) + 64, 2);
                    let mut c = vec![0.0; m * nn + 64];
                    let tm = time(|| {
                        let (alda, blda) = (if ta { k } else { m }, if tb { nn } else { k });
                        gemm_serial(m, nn, k, 1.0, &aa, alda, ta, &bb, blda, tb, 0.0, &mut c, m);
                    }, 7);
                    let _ = &bm;
                    println!("gemm {m}x{nn}x{k} (ta={ta}): {:.3} ms  {:.1} GF/s", tm.1 * 1e3, 2.0 * (m * nn * k) as f64 / tm.1 / 1e9);
                }
            }
            "svd_stages" => {
                let t_bd = time(|| { std::hint::black_box(dense_svd::bidiagonalize(&a, n, n)); }, reps);
                let bd = dense_svd::bidiagonalize(&a, n, n);
                let t_q = time(|| { std::hint::black_box(dense_qr::orgqr(&bd.w, n, n, n, n, &bd.tauq)); }, reps);
                let t_all = time(|| { std::hint::black_box(dense_svd::svd(&a, n, n)); }, reps);
                println!("svd n={n}: bidiag {:.2}  orgqr(one) {:.2}  total {:.2} ms (medians)", t_bd.0 * 1e3, t_q.0 * 1e3, t_all.0 * 1e3);
            }
            "qr_stages" => {
                let t_f = time(|| { std::hint::black_box(dense_qr::geqrf(a.clone(), n, n)); }, reps);
                let f = dense_qr::geqrf(a.clone(), n, n);
                let t_q = time(|| { std::hint::black_box(dense_qr::orgqr(&f.a, n, n, n, n, &f.tau)); }, reps);
                println!("qr n={n}: geqrf {:.2}  orgqr {:.2} ms (medians)", t_f.0 * 1e3, t_q.0 * 1e3);
            }
            "eigsym_stages" => {
                let mut s = vec![0.0; n * n];
                for j in 0..n { for i in 0..n { s[i + j * n] = a[i + j * n] + a[j + i * n]; } }
                let t_tri = time(|| { std::hint::black_box(dense_eigsym::tridiagonalize(&s, n, false)); }, reps);
                let t_tri_q = time(|| { std::hint::black_box(dense_eigsym::tridiagonalize(&s, n, true)); }, reps);
                let (d0, e0, q) = dense_eigsym::tridiagonalize(&s, n, true);
                let q = q.unwrap();
                let t_ql = time(|| {
                    let mut d = d0.clone();
                    let mut o: Vec<f64> = e0.iter().map(|v| v.abs()).collect();
                    dense_eigsym::tridiagonal_ql(&mut d, &mut o, None);
                    std::hint::black_box(d);
                }, reps);
                let t_all = time(|| {
                    let mut d = d0.clone();
                    let mut o: Vec<f64> = e0.iter().map(|v| v.abs()).collect();
                    let mut t = qu_core::dense_kernels::RotTarget::new(&q, n, n);
                    dense_eigsym::tridiagonal_ql(&mut d, &mut o, Some(&mut t));
                    std::hint::black_box(t.finish());
                }, reps);
                println!("eigsym n={n}: tri(no Q) {:.2}  tri+Q {:.2}  QL values-only {:.2}  QL+rotations {:.2} ms (medians)",
                    t_tri.0 * 1e3, t_tri_q.0 * 1e3, t_ql.0 * 1e3, t_all.0 * 1e3);
            }
            _ => panic!("unknown benchmark"),
        }
    }
}
