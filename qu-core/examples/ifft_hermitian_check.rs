//! Answers a concrete question rather than guessing at it: is it worth
//! detecting a Hermitian-symmetric (real-signal-derived) spectrum inside
//! the general `ifft()` path, so it can route to `irfft_real` instead of
//! the full complex inverse? Measures detection cost against the actual
//! savings, at the sizes bench_fair.qu's fft kernel and a 10x-larger case
//! use, rather than asserting either "too expensive" or "worth it".
//!
//! **THIS KERNEL-LEVEL NUMBER SAYS YES; DO NOT TRUST IT ALONE (§ 2026-09-10
//! -- built, measured end-to-end, reverted).** Isolated here it looks
//! like a clear win (26-44%, detection under 5% of the transform). Wired
//! into `eval_fft`'s actual inverse path and measured through `qu.exe` --
//! the number that matters -- it was a REGRESSION: `ifft(1000000,1)` went
//! from ~0.0120s to ~0.0134s, consistently, tight variance both sides, not
//! noise (checked at n=100000 too, where it was inconclusive -- noisy
//! enough that neither direction was trustworthy, which is itself the
//! reason to trust the n=1000000 result more, not less). Root cause not
//! pinned down -- a plausible but UNVERIFIED guess is that
//! `is_hermitian_spectrum`'s scan or the extra `Vec` clone the real code
//! needs (this file's isolated version does not) cost more inside
//! `eval_fft`'s huge translation unit than in this small, separately-
//! compiled one, where LLVM has an easier time. Reverted rather than
//! shipped, because the end-to-end measurement is the one that counts and
//! it disagreed with this file's own story. Kept as a diagnostic anyway:
//! the NEXT attempt at this should start by explaining why THIS number and
//! the real one disagree, not by re-deriving that they do.
use qu_core::{fft_complex, ifft_complex, irfft_real, Complex64};
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

/// O(n) check: does `spec` actually satisfy the Hermitian symmetry a
/// real-valued time-domain signal's spectrum always has?
fn is_hermitian(spec: &[Complex64], tol: f64) -> bool {
    let n = spec.len();
    for k in 1..n / 2 + 1 {
        let a = spec[k];
        let b = spec[n - k];
        if (a.re - b.re).abs() > tol || (a.im + b.im).abs() > tol {
            return false;
        }
    }
    true
}

fn main() {
    for n in [100_000usize, 1_000_000] {
        let real: Vec<f64> = (0..n).map(|i| (i as f64 * 0.0137).sin()).collect();
        let packed: Vec<Complex64> = real.iter().map(|&v| Complex64::real(v)).collect();
        let spectrum = fft_complex(&packed).unwrap();

        let t_full_ifft = best_of(5, || ifft_complex(&spectrum).unwrap());
        println!("n={n}: full complex ifft            : {t_full_ifft:.5} s");

        let t_detect = best_of(5, || is_hermitian(&spectrum, 1e-6));
        println!("n={n}: Hermitian detection (O(n))    : {t_detect:.5} s");

        let half = &spectrum[..n / 2 + 1];
        let t_real_ifft = best_of(5, || irfft_real(half, n).unwrap());
        println!("n={n}: irfft_real on the half-spectrum: {t_real_ifft:.5} s");

        let combined = t_detect + t_real_ifft;
        println!(
            "n={n}: detect+real-ifft = {combined:.5} s  vs  full ifft = {t_full_ifft:.5} s  -> {}",
            if combined < t_full_ifft { "WORTH IT" } else { "NOT worth it" }
        );
        println!();
    }
}
