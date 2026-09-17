//! Does the ~2.5x-vs-3.1x-behind-MATLAB FFT gap track rustfft's own
//! mixed-radix path against FFTW, or is it something Qu's wrapper adds
//! (repacking, allocation)? Two things measured per size, isolated:
//!
//!   RAW    -- rustfft's own `process()` on a pre-allocated
//!             `Vec<Complex<f64>>` buffer, cached `FftPlanner` (the
//!             exact caching Qu's own `fft_dispatch` already uses --
//!             confirmed by reading qu-core/src/lib.rs's
//!             `FFT_PLANNER` thread_local before writing this), best
//!             of 5, buffer allocated outside the timed region.
//!   WRAPPED -- the same transform through a function shaped exactly
//!             like `fft_dispatch`: input arrives as `Complex64`
//!             (this crate's own type), gets copied element-by-element
//!             into `Complex<f64>` (rustfft's type), transformed, then
//!             copied back -- the two extra O(n) passes every real
//!             `fft()` call actually pays that a microbenchmark calling
//!             rustfft directly would not see.
//!
//! Four sizes, same order of magnitude, different factorizations --
//! textbook mixed-radix stress test:
//!   1,048,576 = 2^20            (power of two)
//!     995,328 = 2^12 * 3^5      (3-smooth)
//!   1,000,000 = 2^6  * 5^6      (5-smooth -- the one already measured
//!                                 in Qu at 3.14x behind MATLAB)
//!   1,000,003                   (prime -- rustfft's worst case, no
//!                                 factorization to exploit at all)
//!
//! Run: cargo run --release --example fft_mixed_radix_probe -p qu-core

use qu_core::Complex64;
use rustfft::{num_complex::Complex as RustComplex, FftPlanner};
use std::time::Instant;

fn best_of<F: FnMut() -> T, T>(reps: usize, mut f: F) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..reps {
        let t0 = Instant::now();
        let out = f();
        std::hint::black_box(&out);
        let dt = t0.elapsed().as_secs_f64();
        if dt < best {
            best = dt;
        }
    }
    best
}

fn main() {
    let sizes: [(usize, &str); 4] = [
        (1_048_576, "2^20          (power of two)"),
        (995_328, "2^12 * 3^5    (3-smooth)"),
        (1_000_000, "2^6  * 5^6    (5-smooth)"),
        (1_000_003, "prime         (no factorization at all)"),
    ];

    println!("{:<14}{:<32}{:>12}{:>12}{:>10}", "N", "factorization", "raw (s)", "wrapped (s)", "wrap/raw");

    for (n, label) in sizes {
        // ---- RAW: rustfft directly, cached planner, no Qu type conversion ----
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(n);
        let base: Vec<RustComplex<f64>> = (0..n)
            .map(|i| RustComplex::new((i as f64 * 0.0001).sin(), 0.0))
            .collect();

        // warm-up (first call to this length plans internally, same as
        // Qu's own thread-local planner would on its own first call)
        let mut buf = base.clone();
        fft.process(&mut buf);

        let t_raw = best_of(5, || {
            let mut buf = base.clone(); // allocation itself excluded isn't possible
                                         // without changing what process() does in
                                         // place; cloning a pre-built Vec is the
                                         // fairest stand-in for "buffer already
                                         // exists", timed the same way in both arms
            fft.process(&mut buf);
            buf
        });

        // ---- WRAPPED: exactly what qu-core's fft_dispatch does ----
        let input: Vec<Complex64> = base.iter().map(|c| Complex64::new(c.re, c.im)).collect();
        let t_wrapped = best_of(5, || {
            let mut buffer: Vec<RustComplex<f64>> =
                input.iter().map(|c| RustComplex::new(c.re, c.im)).collect();
            fft.process(&mut buffer);
            let out: Vec<Complex64> =
                buffer.into_iter().map(|c| Complex64::new(c.re, c.im)).collect();
            out
        });

        println!(
            "{:<14}{:<32}{:>12.6}{:>12.6}{:>9.2}x",
            n,
            label,
            t_raw,
            t_wrapped,
            t_wrapped / t_raw
        );
    }
}
