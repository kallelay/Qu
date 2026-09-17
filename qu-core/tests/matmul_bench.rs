//! Re-runnable version of the "Multi-core matmul via rayon" measurement
//! (BACKLOG.md, 2026-08-25 dev session: 600x600, 0.0352s -> 0.0084s, a real
//! 4.2x) — that number was a one-off manual timing, not captured in any
//! test, so it couldn't be reproduced without redoing the work by hand.
//! This pins it down as a permanent, printed-timing regression check,
//! mirroring the existing `qu-core/tests/signal_advanced.rs` pattern
//! (`accelerated_cpu_kernel_is_fast_and_faster_than_oracle`).
//!
//! Run: `cargo test -p qu-core --release --features parallel --test matmul_bench -- --nocapture`

use std::time::Instant;

use qu_core::matrix::Matrix;

fn naive_matmul(a: &Matrix, b: &Matrix, n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n * n];
    for r in 0..n {
        for c in 0..n {
            let mut acc = 0.0;
            for p in 0..n {
                acc += a.get(r, p).unwrap() * b.get(p, c).unwrap();
            }
            out[r + c * n] = acc;
        }
    }
    out
}

#[test]
#[cfg(feature = "parallel")]
fn parallel_matmul_beats_a_naive_reference_at_scale() {
    // Debug builds run this same triple loop 10-50x slower than release, so
    // the size shrinks in debug — still comfortably above
    // `Matrix::PARALLEL_MATMUL_THRESHOLD` (m*n*k = 1<<16), just not the full
    // 600x600 BACKLOG.md's number used.
    let n = if cfg!(debug_assertions) { 150 } else { 600 };
    let a = Matrix::from_col_major(n, n, (0..n * n).map(|i| ((i % 13) as f64 - 6.0) * 0.1).collect());
    let b = Matrix::from_col_major(n, n, (0..n * n).map(|i| ((i % 11) as f64 - 5.0) * 0.1).collect());

    let start = Instant::now();
    let naive = naive_matmul(&a, &b, n);
    let naive_time = start.elapsed();

    let start = Instant::now();
    let parallel = a.matmul(&b).unwrap();
    let parallel_time = start.elapsed();

    for (i, &want) in naive.iter().enumerate() {
        let (r, c) = (i % n, i / n);
        let got = parallel.get(r, c).unwrap();
        assert!((got - want).abs() < 1e-6, "mismatch at ({r},{c}): got {got}, want {want}");
    }

    eprintln!(
        "matmul {n}x{n}: naive={:.4} s, parallel={:.4} s, speedup={:.2}x",
        naive_time.as_secs_f64(),
        parallel_time.as_secs_f64(),
        naive_time.as_secs_f64() / parallel_time.as_secs_f64(),
    );
    // Debug-build timing is dominated by lack of optimization, not by
    // whether the parallel path helps — same reasoning `signal_advanced.rs`
    // uses for its own speedup assertion.
    if !cfg!(debug_assertions) {
        assert!(
            parallel_time < naive_time,
            "expected the rayon-parallel path to beat a naive triple loop at n={n}"
        );
    }
}
