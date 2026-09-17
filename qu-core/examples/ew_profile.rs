//! Diagnostic, not a test: how much of `element_wise_ops`'s gap is the
//! numerical kernel itself vs. everything wrapping it. Run with
//! `cargo run --release --features "parallel,fast-matmul" --example ew_profile`
//! from `qu-core/`. Calls `Matrix::map`/`Matrix::broadcast` directly --
//! the SAME functions `qu-interp`'s elementwise builtins call -- so this
//! measures the real kernel, not a reimplementation of it.
//!
//! Compare its "FULL CHAIN" number against the same chain run through
//! `qu.exe` (`A .* B + sin(A) .* cos(B); abs(...)`, matching
//! `benchmarks/matty_suite/bench_fair.qu`'s `element_wise_ops` kernel
//! exactly): the gap between the two is Qu's own interpreter/dispatch
//! overhead, separate from whatever the kernel itself costs.
use qu_core::matrix::Matrix;
use std::time::Instant;

fn best_of<F: FnMut() -> Matrix>(reps: usize, mut f: F) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..reps {
        let t0 = Instant::now();
        let out = f();
        std::hint::black_box(&out);
        let dt = t0.elapsed().as_secs_f64();
        if dt < best { best = dt; }
    }
    best
}

fn main() {
    let n = 3000;
    let a = Matrix::filled(n, n, 1.7);
    let b = Matrix::filled(n, n, 2.3);
    // warm-up (persistent rayon pool spins up here, same as Qu's process does)
    let _ = a.broadcast(&b, |x, y| x * y).unwrap();

    let reps = 5;
    let t_mul1 = best_of(reps, || a.broadcast(&b, |x, y| x * y).unwrap());
    println!("broadcast .* (real kernel)      : {t_mul1:.5} s");

    let t_sin = best_of(reps, || a.map(f64::sin));
    println!("map sin (real kernel)           : {t_sin:.5} s");

    let t_cos = best_of(reps, || b.map(f64::cos));
    println!("map cos (real kernel)           : {t_cos:.5} s");

    let s1 = a.map(f64::sin);
    let c1 = b.map(f64::cos);
    let t_mul2 = best_of(reps, || s1.broadcast(&c1, |x, y| x * y).unwrap());
    println!("broadcast s.*c (real kernel)     : {t_mul2:.5} s");

    let m1 = a.broadcast(&b, |x, y| x * y).unwrap();
    let m2 = s1.broadcast(&c1, |x, y| x * y).unwrap();
    let t_add = best_of(reps, || m1.broadcast(&m2, |x, y| x + y).unwrap());
    println!("broadcast + (real kernel)        : {t_add:.5} s");

    let c2 = m1.broadcast(&m2, |x, y| x + y).unwrap();
    let t_abs = best_of(reps, || c2.map(f64::abs));
    println!("map abs (real kernel)            : {t_abs:.5} s");

    let sum6 = t_mul1 + t_sin + t_cos + t_mul2 + t_add + t_abs;
    println!("SUM of 6 real-kernel ops         : {sum6:.5} s");

    let t_chain = best_of(reps, || {
        let m1 = a.broadcast(&b, |x, y| x * y).unwrap();
        let s1 = a.map(f64::sin);
        let c1 = b.map(f64::cos);
        let m2 = s1.broadcast(&c1, |x, y| x * y).unwrap();
        let c2 = m1.broadcast(&m2, |x, y| x + y).unwrap();
        c2.map(f64::abs)
    });
    println!("FULL CHAIN, real kernel, one tic  : {t_chain:.5} s");
}
