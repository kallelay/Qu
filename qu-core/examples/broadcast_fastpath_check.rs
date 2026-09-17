//! Verifies the `broadcast` same-shape fast path is mathematically
//! identical to the general path (not just faster), then times it against
//! the bar QuMaster set: a plain serial Vec zip over the same 4M-element
//! data, in wall-clock TIME (not GB/s -- a rate figure's convention does
//! not travel, a time comparison on the same box needs none).
use qu_core::matrix::Matrix;
use std::time::Instant;

fn best_of<F: FnMut() -> T, T>(reps: usize, mut f: F) -> f64 {
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
    // ---- correctness: build a matrix big enough to force the parallel
    // path, and confirm every element matches a hand-computed reference. ----
    let (r, c) = (2000, 2000); // 4,000,000 -- over PARALLEL_ELEMENTWISE_THRESHOLD
    let a_data: Vec<f64> = (0..r * c).map(|i| (i as f64) * 0.0001).collect();
    let b_data: Vec<f64> = (0..r * c).map(|i| ((i as f64) * 0.0003).sin()).collect();
    let a = Matrix::from_col_major_checked(r, c, a_data.clone()).unwrap();
    let b = Matrix::from_col_major_checked(r, c, b_data.clone()).unwrap();

    let got = a.broadcast(&b, |x, y| x * y).unwrap();
    let mut worst = 0.0f64;
    for i in 0..r * c {
        let want = a_data[i] * b_data[i];
        worst = worst.max((got.as_slice()[i] - want).abs());
    }
    println!("correctness: worst abs error over {} elements = {worst:e}", r * c);
    assert!(worst < 1e-12, "fast path diverged from the reference computation");

    // ---- the time bar: plain serial Vec zip over the same data ----
    let t_vec_zip = best_of(5, || -> Vec<f64> {
        a_data.iter().zip(b_data.iter()).map(|(&x, &y)| x * y).collect()
    });
    println!("Vec serial zip (the bar)          : {t_vec_zip:.5} s");

    let t_broadcast = best_of(5, || a.broadcast(&b, |x, y| x * y).unwrap());
    println!("Matrix::broadcast, same-shape fast path : {t_broadcast:.5} s");

    println!(
        "-> {}",
        if t_broadcast <= t_vec_zip * 1.15 {
            "MEETS the bar (within 15%)"
        } else {
            "BELOW the bar"
        }
    );
}
