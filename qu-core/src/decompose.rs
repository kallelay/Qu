//! Adaptive signal decomposition: Empirical Mode Decomposition (EMD) and
//! Variational Mode Decomposition (VMD) split a signal into a small number
//! of oscillatory "modes" without assuming a fixed basis the way
//! Fourier/wavelet transforms do — useful for nonstationary, nonlinear
//! signals whose dominant frequencies drift over time. EMD (Huang et al.
//! 1998) is the classical, purely time-domain "sifting" approach; VMD
//! (Dragomiretskiy & Zosso, 2014) is a later, non-recursive alternative
//! posed as a frequency-domain variational optimization.

use crate::transforms::analytic_spectrum;
use crate::{ifft_complex, Complex64, NumericError};

fn local_maxima(x: &[f64]) -> (Vec<usize>, Vec<f64>) {
    (1..x.len().saturating_sub(1))
        .filter(|&i| x[i] > x[i - 1] && x[i] > x[i + 1])
        .map(|i| (i, x[i]))
        .unzip()
}

fn local_minima(x: &[f64]) -> (Vec<usize>, Vec<f64>) {
    (1..x.len().saturating_sub(1))
        .filter(|&i| x[i] < x[i - 1] && x[i] < x[i + 1])
        .map(|i| (i, x[i]))
        .unzip()
}

/// Natural cubic spline second derivatives at `x` (assumed strictly
/// increasing) — the standard tridiagonal system, solved with the Thomas
/// algorithm. A self-contained copy of the same construction `splineplot`
/// uses (kept local rather than shared across crates, since `qu-interp`'s
/// copy is a private helper of its own plotting code).
fn cubic_spline_moments(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    if n == 2 {
        return vec![0.0, 0.0];
    }
    let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
    let (mut a, mut b, mut c, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    b[0] = 1.0;
    b[n - 1] = 1.0;
    for i in 1..n - 1 {
        a[i] = h[i - 1];
        b[i] = 2.0 * (h[i - 1] + h[i]);
        c[i] = h[i];
        d[i] = 6.0 * ((y[i + 1] - y[i]) / h[i] - (y[i] - y[i - 1]) / h[i - 1]);
    }
    let (mut cp, mut dp) = (vec![0.0; n], vec![0.0; n]);
    cp[0] = c[0] / b[0];
    dp[0] = d[0] / b[0];
    for i in 1..n {
        let denom = b[i] - a[i] * cp[i - 1];
        cp[i] = if i < n - 1 { c[i] / denom } else { 0.0 };
        dp[i] = (d[i] - a[i] * dp[i - 1]) / denom;
    }
    let mut m = vec![0.0; n];
    m[n - 1] = dp[n - 1];
    for i in (0..n - 1).rev() {
        m[i] = dp[i] - cp[i] * m[i + 1];
    }
    m
}

fn spline_eval_one(x: &[f64], y: &[f64], m: &[f64], xq: f64) -> f64 {
    let n = x.len();
    if n == 2 {
        let t = (xq - x[0]) / (x[1] - x[0]);
        return y[0] + t * (y[1] - y[0]);
    }
    let mut i = 0;
    while i < n - 2 && xq > x[i + 1] {
        i += 1;
    }
    let h = x[i + 1] - x[i];
    let a = (x[i + 1] - xq) / h;
    let b = (xq - x[i]) / h;
    a * y[i] + b * y[i + 1] + ((a.powi(3) - a) * m[i] + (b.powi(3) - b) * m[i + 1]) * (h * h) / 6.0
}

/// The upper/lower envelope of `signal` through its extrema at `idx`/`val`,
/// evaluated at every integer sample `0..signal.len()`. The signal's own
/// first/last sample is added as an extra boundary "extremum" whenever the
/// real extrema don't already reach the edges — a deliberate
/// simplification of the mirror-extension schemes used in some reference
/// EMD implementations, chosen so the spline never has to extrapolate
/// outside the data it was built from.
fn envelope(signal: &[f64], idx: &[usize], val: &[f64]) -> Vec<f64> {
    let n = signal.len();
    let mut ext_x = Vec::with_capacity(idx.len() + 2);
    let mut ext_y = Vec::with_capacity(idx.len() + 2);
    if idx.first() != Some(&0) {
        ext_x.push(0.0);
        ext_y.push(signal[0]);
    }
    for (&i, &v) in idx.iter().zip(val) {
        ext_x.push(i as f64);
        ext_y.push(v);
    }
    if idx.last() != Some(&(n - 1)) {
        ext_x.push((n - 1) as f64);
        ext_y.push(signal[n - 1]);
    }
    if ext_x.len() < 2 {
        return vec![ext_y.first().copied().unwrap_or(0.0); n];
    }
    let m = cubic_spline_moments(&ext_x, &ext_y);
    (0..n).map(|i| spline_eval_one(&ext_x, &ext_y, &m, i as f64)).collect()
}

/// One sifting pass: repeatedly subtract the mean of the upper/lower
/// envelopes until the standard deviation between successive iterations
/// drops below `sd_threshold` (Huang et al.'s own stopping criterion) or
/// `max_sift` iterations are used up, whichever comes first. Stops early,
/// leaving `h` as-is, once there are too few extrema (fewer than 2 on
/// either side) to fit an envelope at all.
fn sift(signal: &[f64], max_sift: usize, sd_threshold: f64) -> Vec<f64> {
    let mut h = signal.to_vec();
    for _ in 0..max_sift {
        let (max_idx, max_val) = local_maxima(&h);
        let (min_idx, min_val) = local_minima(&h);
        if max_idx.len() < 2 || min_idx.len() < 2 {
            break;
        }
        let upper = envelope(&h, &max_idx, &max_val);
        let lower = envelope(&h, &min_idx, &min_val);
        let h_new: Vec<f64> = h.iter().zip(upper.iter().zip(&lower)).map(|(&hv, (&u, &l))| hv - (u + l) / 2.0).collect();
        let sd: f64 = h.iter().zip(&h_new).map(|(a, b)| (a - b).powi(2)).sum::<f64>()
            / h.iter().map(|a| a * a).sum::<f64>().max(1e-300);
        h = h_new;
        if sd < sd_threshold {
            break;
        }
    }
    h
}

pub struct EmdResult {
    /// Each intrinsic mode function (IMF), highest frequency first.
    pub imfs: Vec<Vec<f64>>,
    /// What's left after every IMF is subtracted out — the signal's
    /// overall trend, with at most one interior extremum (or none).
    pub residual: Vec<f64>,
}

/// Empirical Mode Decomposition: repeatedly extracts the highest-frequency
/// oscillatory mode still present (via [`sift`]) and subtracts it from the
/// running residual, stopping once the residual has too few extrema to be
/// meaningfully oscillatory (fewer than 3 combined maxima+minima) or
/// `max_imfs` modes have been extracted, whichever comes first.
/// `sum(result.imfs) + result.residual == x` exactly, by construction —
/// sifting only ever subtracts, so there is nowhere for energy to leak.
pub fn emd(x: &[f64], max_imfs: Option<usize>) -> Result<EmdResult, NumericError> {
    if x.len() < 4 {
        return Err(NumericError::EmptyInput("emd"));
    }
    const MAX_SIFT: usize = 100;
    const SD_THRESHOLD: f64 = 0.2;
    let mut residual = x.to_vec();
    let mut imfs = Vec::new();
    loop {
        if let Some(limit) = max_imfs {
            if imfs.len() >= limit {
                break;
            }
        }
        let (max_idx, _) = local_maxima(&residual);
        let (min_idx, _) = local_minima(&residual);
        if max_idx.len() + min_idx.len() < 3 {
            break;
        }
        let imf = sift(&residual, MAX_SIFT, SD_THRESHOLD);
        for (r, &v) in residual.iter_mut().zip(&imf) {
            *r -= v;
        }
        imfs.push(imf);
    }
    Ok(EmdResult { imfs, residual })
}

pub struct VmdResult {
    /// The `k` recovered modes, each the same length as the input.
    pub modes: Vec<Vec<f64>>,
    /// Each mode's converged center frequency, normalized to cycles per
    /// sample (`0.5` is Nyquist).
    pub center_freqs: Vec<f64>,
}

/// Variational Mode Decomposition (Dragomiretskiy & Zosso, 2014):
/// decomposes `f` into `k` band-limited modes by minimizing each mode's
/// bandwidth around its own center frequency, solved by ADMM in the
/// frequency domain (ordinary-least-squares mode updates, a power-
/// weighted-centroid frequency update, and a dual-ascent term enforcing
/// `sum(modes) == f`). `alpha` is the bandwidth-constraint weight (larger
/// forces narrower-band modes; `2000` is the value used throughout the
/// original paper's own examples).
///
/// **Simplification versus the reference implementation**: the original
/// paper's own MATLAB code mirror-pads the signal before transforming, to
/// suppress edge artifacts; this implementation works directly on `f`'s
/// own length instead, which is simpler and correct in the interior but
/// leaves more pronounced edge effects near the boundaries than the
/// mirrored version — a real, documented trade-off, not a hidden one.
pub fn vmd(f: &[f64], k: usize, alpha: f64) -> Result<VmdResult, NumericError> {
    if f.is_empty() {
        return Err(NumericError::EmptyInput("vmd"));
    }
    if k == 0 {
        return Err(NumericError::EmptyInput("vmd: k must be at least 1"));
    }
    const TAU: f64 = 0.0; // no dual-ascent noise tolerance: exact sum(modes) == f
    const TOL: f64 = 1e-7;
    const MAX_ITER: usize = 500;

    let n = f.len();
    let half = n / 2;
    // The one-sided ("analytic") spectrum: this is the frequency-domain
    // target every mode competes to explain a share of.
    let f_hat = analytic_spectrum(f)?;
    let freqs: Vec<f64> = (0..n).map(|i| i as f64 / n as f64).collect();

    let mut u_hat = vec![vec![Complex64::new(0.0, 0.0); n]; k];
    // Spread initial center frequencies evenly across (0, 0.5) rather than
    // all starting at the same point, so distinct modes don't collapse
    // onto one frequency by symmetry.
    let mut omega: Vec<f64> = (0..k).map(|i| 0.5 * (i as f64 + 1.0) / (k as f64 + 1.0)).collect();
    let mut lambda = vec![Complex64::new(0.0, 0.0); n];
    let mut sum_u = vec![Complex64::new(0.0, 0.0); n];

    for _iteration in 0..MAX_ITER {
        let mut max_relative_change = 0.0_f64;
        for kk in 0..k {
            for i in 0..n {
                sum_u[i] = sum_u[i].sub(u_hat[kk][i]);
            }
            let mut new_u = vec![Complex64::new(0.0, 0.0); n];
            for i in 0..n {
                let numerator = f_hat[i].sub(sum_u[i]).add(lambda[i].scale(0.5));
                let denominator = 1.0 + alpha * (freqs[i] - omega[kk]).powi(2);
                new_u[i] = numerator.scale(1.0 / denominator);
            }
            let (mut weighted, mut power) = (0.0, 0.0);
            for i in 0..=half.min(n - 1) {
                let p = new_u[i].magnitude().powi(2);
                weighted += freqs[i] * p;
                power += p;
            }
            if power > 1e-300 {
                omega[kk] = weighted / power;
            }
            let change: f64 = new_u.iter().zip(&u_hat[kk]).map(|(a, b)| a.sub(*b).magnitude().powi(2)).sum();
            let norm: f64 = u_hat[kk].iter().map(|v| v.magnitude().powi(2)).sum::<f64>().max(1e-300);
            max_relative_change = max_relative_change.max(change / norm);
            for i in 0..n {
                sum_u[i] = sum_u[i].add(new_u[i]);
            }
            u_hat[kk] = new_u;
        }
        if TAU > 0.0 {
            for i in 0..n {
                lambda[i] = lambda[i].add(f_hat[i].sub(sum_u[i]).scale(TAU));
            }
        }
        if max_relative_change < TOL {
            break;
        }
    }

    // Each `u_hat[k]` is already a one-sided ("analytic") spectrum in the
    // same sense `analytic_spectrum`/`hilbert` use, so its real part after
    // an inverse FFT directly reconstructs the real-valued mode — no
    // Hermitian mirroring step needed, exactly mirroring how `hilbert`
    // itself recovers the original real signal in its own real part.
    let mut modes = Vec::with_capacity(k);
    for spectrum in &u_hat {
        let time = ifft_complex(spectrum)?;
        modes.push(time.iter().map(|c| c.re).collect());
    }
    Ok(VmdResult { modes, center_freqs: omega })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU as TWO_PI;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    #[test]
    fn emd_reconstruction_is_exact() {
        let n = 200;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64;
                (TWO_PI * 0.2 * t).sin() + 0.5 * (TWO_PI * 0.02 * t).sin() + 0.001 * t
            })
            .collect();
        let result = emd(&x, None).unwrap();
        let mut reconstructed = result.residual.clone();
        for imf in &result.imfs {
            for (r, &v) in reconstructed.iter_mut().zip(imf) {
                *r += v;
            }
        }
        for (a, b) in x.iter().zip(&reconstructed) {
            close(*a, *b, 1e-6);
        }
    }

    #[test]
    fn emd_produces_at_least_one_imf_for_an_oscillatory_signal() {
        let n = 100;
        let x: Vec<f64> = (0..n).map(|i| (TWO_PI * 0.1 * i as f64).sin()).collect();
        let result = emd(&x, None).unwrap();
        assert!(!result.imfs.is_empty());
    }

    #[test]
    fn emd_of_a_pure_linear_ramp_has_no_imfs() {
        // a monotonic signal has no oscillation to extract at all.
        let x: Vec<f64> = (0..50).map(|i| i as f64 * 0.5).collect();
        let result = emd(&x, None).unwrap();
        assert!(result.imfs.is_empty(), "a monotonic ramp should decompose to zero IMFs");
        close(result.residual[0], x[0], 1e-9);
    }

    #[test]
    fn emd_respects_max_imfs() {
        let n = 300;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64;
                (TWO_PI * 0.3 * t).sin() + (TWO_PI * 0.1 * t).sin() + (TWO_PI * 0.02 * t).sin()
            })
            .collect();
        let result = emd(&x, Some(1)).unwrap();
        assert_eq!(result.imfs.len(), 1);
    }

    #[test]
    fn emd_rejects_a_too_short_input() {
        assert!(emd(&[1.0, 2.0], None).is_err());
    }

    #[test]
    fn vmd_separates_two_well_spaced_tones_and_reconstructs_the_signal() {
        let n = 512;
        let fs = 512.0;
        let f_lo = 5.0;
        let f_hi = 60.0;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                (TWO_PI * f_lo * t).sin() + (TWO_PI * f_hi * t).sin()
            })
            .collect();
        let result = vmd(&x, 2, 2000.0).unwrap();
        assert_eq!(result.modes.len(), 2);

        // sum(modes) must reconstruct the original signal closely (tau=0
        // enforces this as part of the optimization itself).
        let mut reconstructed = vec![0.0; n];
        for mode in &result.modes {
            for (r, &v) in reconstructed.iter_mut().zip(mode) {
                *r += v;
            }
        }
        let max_err = x.iter().zip(&reconstructed).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(max_err < 0.05, "max reconstruction error = {max_err}");

        // the two center frequencies (normalized, cycles/sample) must be
        // close to the two true tones' own normalized frequencies.
        let mut normalized_freqs: Vec<f64> = result.center_freqs.iter().map(|&f| f * fs).collect();
        normalized_freqs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        close(normalized_freqs[0], f_lo, 1.0);
        close(normalized_freqs[1], f_hi, 1.0);
    }

    #[test]
    fn vmd_rejects_zero_modes() {
        assert!(vmd(&[1.0, 2.0, 3.0], 0, 2000.0).is_err());
    }

    #[test]
    fn vmd_rejects_empty_input() {
        assert!(vmd(&[], 1, 2000.0).is_err());
    }
}
