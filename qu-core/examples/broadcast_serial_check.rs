//! Isolates whether the per-element branching or the PARALLEL dispatch
//! itself is the real cost, by comparing at a size BELOW
//! PARALLEL_ELEMENTWISE_THRESHOLD (1<<20), where both old and new code
//! run serially -- if serial old vs serial new still differ, branching is
//! real; if they match, the earlier parallel-vs-Vec-zip finding was
//! measuring dispatch overhead, not branching.
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
    let (r, c) = (1000, 1000); // 1,000,000 -- under the 1<<20 threshold, forces serial
    let a_data: Vec<f64> = (0..r * c).map(|i| (i as f64) * 0.0001).collect();
    let b_data: Vec<f64> = (0..r * c).map(|i| ((i as f64) * 0.0003).sin()).collect();
    let a = Matrix::from_col_major_checked(r, c, a_data.clone()).unwrap();
    let b = Matrix::from_col_major_checked(r, c, b_data.clone()).unwrap();

    let t_vec_zip = best_of(7, || -> Vec<f64> {
        a_data.iter().zip(b_data.iter()).map(|(&x, &y)| x * y).collect()
    });
    println!("Vec serial zip                    : {t_vec_zip:.5} s");

    let t_broadcast = best_of(7, || a.broadcast(&b, |x, y| x * y).unwrap());
    println!("Matrix::broadcast (SERIAL, r*c<threshold): {t_broadcast:.5} s");
}
