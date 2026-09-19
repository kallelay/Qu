//! DSP transforms beyond the raw FFT kernel: real-optimized FFT, DCT, the
//! Haar wavelet, and the short-time Fourier transform, plus the everyday
//! signal-analysis primitives (convolution, correlation, the analytic
//! signal, peak detection) that sit alongside them. Each is a portable
//! reference kernel — correct and dependency-free, not yet vendor-accelerated
//! (see `IMPL.md` M3 for the provider-swap plan every numeric primitive here
//! eventually gets).

use std::f64::consts::{PI, TAU};

use crate::cmatrix::CMatrix;
use crate::linalg::{self, LinalgError};
use crate::matrix::Matrix;
use crate::{fft_complex, fft_real, ifft_complex, irfft_real, rfft_real, Complex64, NumericError};

/// Real-input FFT: returns only the non-negative-frequency half
/// (`floor(n/2)+1` bins). A real signal's spectrum is Hermitian-symmetric
/// (`X[n-k] = conj(X[k])`), so the negative-frequency half is redundant —
/// this is `numpy.fft.rfft`/MATLAB's one-sided spectrum convention.
/// Delegates to [`crate::rfft_real`] (the `realfft`-backed kernel — see its
/// own doc comment for why this does roughly half the work of the
/// previous `fft_real` + slice approach).
pub fn rfft(input: &[f64]) -> Result<Vec<Complex64>, NumericError> {
    rfft_real(input)
}

/// Inverse of [`rfft`]: reconstructs a length-`n` real signal from its
/// non-negative-frequency half-spectrum. Delegates to
/// [`crate::irfft_real`] (the `realfft`-backed kernel, which reconstructs
/// directly rather than mirroring into a full-length complex buffer first).
pub fn irfft(half_spectrum: &[Complex64], n: usize) -> Result<Vec<f64>, NumericError> {
    irfft_real(half_spectrum, n)
}

/// Orthonormal DCT-II (MATLAB `dct`/`scipy.fft.dct(norm='ortho')` convention).
/// A portable `O(n^2)` reference kernel — correct by construction, since an
/// orthonormal basis's inverse is exactly its transpose: [`idct`] is that
/// transpose transform, so `idct(dct(x)) == x` to floating tolerance without
/// any separate scaling convention to get right.
pub fn dct(input: &[f64]) -> Vec<f64> {
    let n = input.len();
    if n == 0 {
        return Vec::new();
    }
    let alpha0 = (1.0 / n as f64).sqrt();
    let alphak = (2.0 / n as f64).sqrt();
    (0..n)
        .map(|k| {
            let alpha = if k == 0 { alpha0 } else { alphak };
            let sum: f64 = input
                .iter()
                .enumerate()
                .map(|(m, &x)| x * (PI * (2 * m + 1) as f64 * k as f64 / (2.0 * n as f64)).cos())
                .sum();
            alpha * sum
        })
        .collect()
}

/// Inverse of the orthonormal [`dct`] (DCT-III with the same normalization).
pub fn idct(coeffs: &[f64]) -> Vec<f64> {
    let n = coeffs.len();
    if n == 0 {
        return Vec::new();
    }
    let alpha0 = (1.0 / n as f64).sqrt();
    let alphak = (2.0 / n as f64).sqrt();
    (0..n)
        .map(|m| {
            coeffs
                .iter()
                .enumerate()
                .map(|(k, &xk)| {
                    let alpha = if k == 0 { alpha0 } else { alphak };
                    alpha * xk * (PI * (2 * m + 1) as f64 * k as f64 / (2.0 * n as f64)).cos()
                })
                .sum()
        })
        .collect()
}

/// One level of the orthogonal Haar discrete wavelet transform. Input length
/// must be even. Returns `(approximation, detail)`, each half the input
/// length; [`idwt_haar`] reconstructs exactly (Haar is the simplest wavelet
/// with an exact, dependency-free implementation — other wavelet families,
/// e.g. Daubechies, are future work, not silently approximated here).
pub fn dwt_haar(input: &[f64]) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    let n = input.len();
    if n == 0 || n % 2 != 0 {
        return Err(NumericError::OddLength { length: n });
    }
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut approx = Vec::with_capacity(n / 2);
    let mut detail = Vec::with_capacity(n / 2);
    for pair in input.chunks_exact(2) {
        approx.push((pair[0] + pair[1]) * s);
        detail.push((pair[0] - pair[1]) * s);
    }
    Ok((approx, detail))
}

/// Inverse of [`dwt_haar`].
pub fn idwt_haar(approx: &[f64], detail: &[f64]) -> Result<Vec<f64>, NumericError> {
    if approx.len() != detail.len() {
        return Err(NumericError::ShapeMismatch { expected: approx.len(), found: detail.len() });
    }
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut out = Vec::with_capacity(approx.len() * 2);
    for (&a, &d) in approx.iter().zip(detail.iter()) {
        out.push((a + d) * s);
        out.push((a - d) * s);
    }
    Ok(out)
}

/// Periodic Hann window of length `n` (`scipy.signal.windows.hann(n,
/// sym=False)` / MATLAB's periodic `hann` convention).
pub fn hann_window(n: usize) -> Vec<f64> {
    match n {
        0 => Vec::new(),
        1 => vec![1.0],
        _ => (0..n)
            .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
            .collect(),
    }
}

/// Short-time Fourier transform: frame `input` into overlapping `nfft`-sample
/// windows advancing by `hop`, apply a periodic Hann window to each, and FFT
/// each frame. Returns an `(nfft, n_frames)` complex matrix — one column per
/// frame, the full (not one-sided) spectrum. `abs(stft(x, ...))` is a
/// spectrogram.
pub fn stft(input: &[f64], nfft: usize, hop: usize) -> Result<CMatrix, NumericError> {
    if nfft == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    if hop == 0 {
        return Err(NumericError::ElementLimit { requested: 0, limit: 1 });
    }
    if input.len() < nfft {
        return Err(NumericError::FftLength { length: input.len() });
    }
    let window = hann_window(nfft);
    let n_frames = (input.len() - nfft) / hop + 1;
    let mut data = vec![Complex64::new(0.0, 0.0); nfft * n_frames];
    for frame in 0..n_frames {
        let start = frame * hop;
        let windowed: Vec<Complex64> = (0..nfft)
            .map(|i| Complex64::real(input[start + i] * window[i]))
            .collect();
        let spectrum = fft_complex(&windowed)?;
        for (k, bin) in spectrum.into_iter().enumerate() {
            data[k + frame * nfft] = bin;
        }
    }
    Ok(CMatrix::from_col_major(nfft, n_frames, data))
}

/// Short-time energy (STE): windowed sum-of-squares over sliding,
/// overlapping frames — the windowed extension of [`signal_energy`], and
/// the standard way to track how a signal's power evolves over time (e.g.
/// locating muscle-activation bursts in an EMG signal, or voiced/silent
/// segments in speech). Each frame is Hann-windowed before squaring and
/// summing, the same windowing [`stft`] uses, so a script computing both
/// gets frames that line up.
pub fn short_time_energy(x: &[f64], win: usize, hop: usize) -> Result<Vec<f64>, NumericError> {
    if win == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    if hop == 0 {
        return Err(NumericError::ElementLimit { requested: 0, limit: 1 });
    }
    if x.len() < win {
        return Err(NumericError::FftLength { length: x.len() });
    }
    let window = hann_window(win);
    let n_frames = (x.len() - win) / hop + 1;
    let mut out = Vec::with_capacity(n_frames);
    for frame in 0..n_frames {
        let start = frame * hop;
        let e: f64 = (0..win)
            .map(|i| {
                let v = x[start + i] * window[i];
                v * v
            })
            .sum();
        out.push(e);
    }
    Ok(out)
}

/// Spectral entropy: the Shannon entropy of each [`stft`] frame's
/// normalized power spectrum, base 2 and divided by `log2(n_bins)` so it
/// always lands in `[0, 1]` regardless of `nfft` — low when a frame's
/// energy concentrates in a few frequency bins (a clean tone), high when
/// it's spread across many (broadband noise or a complex signal). Reuses
/// `stft` directly rather than a separate windowing/FFT path, so its frame
/// boundaries always agree with `stft`'s own. Verified against an
/// independent Python/numpy implementation before this was written: a
/// 50 Hz tone gives entropy around 0.18, white noise around 0.92 — the
/// expected "concentrated vs. spread out" contrast, not just "some number
/// between 0 and 1".
pub fn spectral_entropy(x: &[f64], nfft: usize, hop: usize) -> Result<Vec<f64>, NumericError> {
    let spec = stft(x, nfft, hop)?;
    let (_, n_frames) = spec.shape();
    let n_bins = nfft / 2 + 1;
    let h_max = (n_bins as f64).log2();
    let mut out = Vec::with_capacity(n_frames);
    for c in 0..n_frames {
        let power: Vec<f64> = (0..n_bins)
            .map(|b| {
                let mag = spec.get(b, c).expect("index within bounds by construction").magnitude();
                mag * mag
            })
            .collect();
        let total: f64 = power.iter().sum();
        let h = if total > 0.0 && h_max > 0.0 {
            -power
                .iter()
                .filter(|&&p| p > 0.0)
                .map(|&p| {
                    let pn = p / total;
                    pn * pn.log2()
                })
                .sum::<f64>()
                / h_max
        } else {
            0.0
        };
        out.push(h);
    }
    Ok(out)
}

/// Welch's method power spectral density estimate (Welch, 1967): segment
/// `x` into overlapping `nperseg`-length frames advancing by
/// `nperseg - noverlap` samples (exactly [`stft`]'s own framing scheme,
/// generalized to an arbitrary caller-supplied `window` instead of `stft`'s
/// hardcoded periodic Hann), take each windowed frame's one-sided spectrum
/// via [`rfft`], and average the squared magnitudes across frames — trading
/// frequency resolution for lower variance versus a single [`periodogram`]
/// on the whole signal. Returns `nperseg/2 + 1` bins, DC through Nyquist
/// inclusive (the same one-sided convention `rfft` and `spectrogram` already
/// use; the caller reconstructs the matching frequency axis via
/// `linspace(0, fs/2, length(psd))`, the same convention already documented
/// for `freqz`/`group_delay`).
///
/// Scaled in SciPy's `scaling="density"` convention (units:
/// signal-units² / Hz — `scipy.signal.welch`'s own default, not
/// `"spectrum"`'s units²): each bin is `|X[k]|² / (fs * sum(window²))`,
/// doubled for every bin except DC (and Nyquist, when `nperseg` is even)
/// to fold the negative-frequency half's power into the one-sided result.
/// Integrating the returned PSD (`sum(psd) * (fs / nperseg)`) approximates
/// the signal's own mean-square power by Parseval's theorem — exact for a
/// rectangular (all-ones) window, and a close approximation for tapered
/// windows since `sum(window²)` corrects for the window's own energy loss
/// rather than the signal's local energy under it.
///
/// Preconditions (the caller — `qu-interp`'s `welch` builtin — is expected
/// to validate these against a user-facing message before calling; this
/// function only guards the numerical work itself): `window.len()` must
/// equal `nperseg`, `nperseg` must be at least 1 and no more than
/// `x.len()`, and `noverlap` must be strictly less than `nperseg` (so the
/// frame hop is at least 1 sample).
pub fn welch(x: &[f64], fs: f64, nperseg: usize, noverlap: usize, window: &[f64]) -> Result<Vec<f64>, NumericError> {
    if nperseg == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    if window.len() != nperseg {
        return Err(NumericError::ShapeMismatch { expected: nperseg, found: window.len() });
    }
    if x.len() < nperseg {
        return Err(NumericError::FftLength { length: x.len() });
    }
    if noverlap >= nperseg {
        return Err(NumericError::ElementLimit { requested: noverlap, limit: nperseg - 1 });
    }
    let hop = nperseg - noverlap;
    let n_bins = nperseg / 2 + 1;
    let s2: f64 = window.iter().map(|w| w * w).sum();
    let scale = fs * s2;
    let is_even = nperseg % 2 == 0;
    let n_frames = (x.len() - nperseg) / hop + 1;
    let mut acc = vec![0.0; n_bins];
    for frame in 0..n_frames {
        let start = frame * hop;
        let windowed: Vec<f64> = (0..nperseg).map(|i| x[start + i] * window[i]).collect();
        let spectrum = rfft(&windowed)?;
        for (k, bin) in spectrum.iter().enumerate().take(n_bins) {
            let mag = bin.magnitude();
            let mut p = (mag * mag) / scale;
            if k != 0 && !(is_even && k == n_bins - 1) {
                p *= 2.0;
            }
            acc[k] += p;
        }
    }
    for v in acc.iter_mut() {
        *v /= n_frames as f64;
    }
    Ok(acc)
}

/// One-shot PSD estimate: [`welch`] with a single segment spanning the
/// whole signal (`nperseg == x.len()`, `noverlap = 0`) — a plain windowed
/// periodogram, `|FFT(window .* x)|²` scaled identically to `welch`'s own
/// `"density"` convention (see its doc comment) so the two remain directly
/// comparable: the same signal's `periodogram` and `welch` PSDs integrate to
/// the same approximate mean-square power, `welch` just trades this
/// function's full frequency resolution for lower per-bin variance by
/// averaging shorter, overlapping segments instead.
pub fn periodogram(x: &[f64], fs: f64, window: &[f64]) -> Result<Vec<f64>, NumericError> {
    welch(x, fs, x.len(), 0, window)
}

/// Cross-spectral density `Pxy` of `x` and `y` (Welch's method, `x`'s own
/// framing/windowing/averaging exactly mirrored so `csd(x, x, ...) ==
/// welch(x, ...)` up to `Pxy` being complex where `welch` is already the
/// magnitude-squared): each frame's real spectra are combined as
/// `conj(X_k) * Y_k`, scaled by the same window-power/`fs` factor and
/// one-sided doubling `welch` uses, then averaged across frames. `x` and
/// `y` must be the same length and share `nperseg`/`noverlap`/`window` —
/// there is no meaningful per-signal framing otherwise. Returns the
/// one-sided complex CSD (`nperseg/2 + 1` bins, DC to Nyquist); `coherence`
/// below and `abs`/`angle` on the result are how a caller gets magnitude or
/// phase out of it.
pub fn csd(
    x: &[f64],
    y: &[f64],
    fs: f64,
    nperseg: usize,
    noverlap: usize,
    window: &[f64],
) -> Result<Vec<Complex64>, NumericError> {
    if nperseg == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    if x.len() != y.len() {
        return Err(NumericError::ShapeMismatch { expected: x.len(), found: y.len() });
    }
    if window.len() != nperseg {
        return Err(NumericError::ShapeMismatch { expected: nperseg, found: window.len() });
    }
    if x.len() < nperseg {
        return Err(NumericError::FftLength { length: x.len() });
    }
    if noverlap >= nperseg {
        return Err(NumericError::ElementLimit { requested: noverlap, limit: nperseg - 1 });
    }
    let hop = nperseg - noverlap;
    let n_bins = nperseg / 2 + 1;
    let s2: f64 = window.iter().map(|w| w * w).sum();
    let scale = fs * s2;
    let is_even = nperseg % 2 == 0;
    let n_frames = (x.len() - nperseg) / hop + 1;
    let mut acc = vec![Complex64::real(0.0); n_bins];
    for frame in 0..n_frames {
        let start = frame * hop;
        let wx: Vec<f64> = (0..nperseg).map(|i| x[start + i] * window[i]).collect();
        let wy: Vec<f64> = (0..nperseg).map(|i| y[start + i] * window[i]).collect();
        let sx = rfft(&wx)?;
        let sy = rfft(&wy)?;
        for k in 0..n_bins {
            let mut p = sx[k].conj().mul(sy[k]).scale(1.0 / scale);
            if k != 0 && !(is_even && k == n_bins - 1) {
                p = p.scale(2.0);
            }
            acc[k] = acc[k].add(p);
        }
    }
    for v in acc.iter_mut() {
        *v = v.scale(1.0 / n_frames as f64);
    }
    Ok(acc)
}

/// Magnitude-squared coherence `Cxy = |Pxy|^2 / (Pxx * Pyy)`, `x` and `y`'s
/// spectral correlation at each frequency, in `[0, 1]` (`1` at a frequency
/// where `y` is an exact linear/time-invariant function of `x`'s content
/// there, `0` where the two are spectrally unrelated). Built directly on
/// [`welch`] (for `Pxx`/`Pyy`) and [`csd`] (for `Pxy`), sharing the exact
/// same framing, so the three PSDs line up bin-for-bin with no separate
/// scaling to reconcile.
pub fn coherence(
    x: &[f64],
    y: &[f64],
    fs: f64,
    nperseg: usize,
    noverlap: usize,
    window: &[f64],
) -> Result<Vec<f64>, NumericError> {
    let pxx = welch(x, fs, nperseg, noverlap, window)?;
    let pyy = welch(y, fs, nperseg, noverlap, window)?;
    let pxy = csd(x, y, fs, nperseg, noverlap, window)?;
    Ok(pxx
        .iter()
        .zip(pyy.iter())
        .zip(pxy.iter())
        .map(|((&sxx, &syy), sxy)| {
            let denom = sxx * syy;
            if denom > 1e-300 {
                (sxy.re * sxy.re + sxy.im * sxy.im) / denom
            } else {
                0.0
            }
        })
        .collect())
}

/// Which of the three standard noise-weighted transfer-function estimators
/// [`transfer_function`] should compute.
///
/// The choice is a statement about WHERE THE NOISE IS, and it is not a
/// detail: on a noisy measurement the three disagree by exactly the
/// coherence, and picking one silently is how a biased FRF gets published.
/// `toolkit-signal.md` §11 calls this out as "a real, well-known choice
/// that should be a named parameter, not a hidden default".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfEstimator {
    /// `H1 = Pxy / Pxx` — assumes the noise is on the OUTPUT `y` and the
    /// input `x` is clean. Biased LOW (towards zero) by output noise.
    /// The right default for a controlled-excitation measurement, where
    /// the drive is known and the response is what the sensor sees.
    H1,
    /// `H2 = Pyy / Pyx` — assumes the noise is on the INPUT `x`. Biased
    /// HIGH by input noise. The estimator to reach for at a resonance,
    /// where the response is large and the drive is comparatively noisy.
    H2,
    /// `Hv = sqrt(H1 * H2)` — the geometric-mean compromise, between the
    /// two bounds whichever way the noise actually falls.
    ///
    /// This is the SISO (single-input single-output) definition. The
    /// general `Hv` is the principal eigenvector of the full spectral
    /// matrix and only coincides with the geometric mean in the
    /// one-input-one-output case, which is the only case this function
    /// computes.
    Hv,
}

/// Frequency response `H` of the system taking `x` to `y`, plus the
/// magnitude-squared coherence at every bin — Welch's method throughout,
/// sharing [`welch`]/[`csd`]'s exact framing (this is deliberately the same
/// call pattern [`coherence`] already uses, so `Pxx`, `Pyy` and `Pxy` line
/// up bin-for-bin with no scaling to reconcile).
///
/// Returns `(H, coherence)`, both `nperseg/2 + 1` bins, DC to Nyquist.
///
/// The three estimators share one phase and differ only in magnitude.
/// Because [`csd`] forms `Pxy = conj(X) * Y`, `arg(H1) = arg(Pxy)` and
/// `arg(H2) = -arg(conj(Pxy)) = arg(Pxy)` alike, so the estimator choice
/// can bias a magnitude but can never flip a phase. Their magnitudes
/// stand in a fixed ratio set by the coherence `g2`:
/// `|H1| = g2 * |H2|` and `|Hv| = sqrt(|H1| * |H2|) = sqrt(Pyy/Pxx)`,
/// which is why `Hv`'s magnitude needs no cross-spectrum at all.
///
/// A bin whose denominator underflows yields `0` rather than `inf`/`NaN`:
/// an unexcited bin has no measurable response, and propagating a NaN
/// through a plot hides that where a zero shows it.
pub fn transfer_function(
    x: &[f64],
    y: &[f64],
    fs: f64,
    nperseg: usize,
    noverlap: usize,
    window: &[f64],
    estimator: TfEstimator,
) -> Result<(Vec<Complex64>, Vec<f64>), NumericError> {
    let pxx = welch(x, fs, nperseg, noverlap, window)?;
    let pyy = welch(y, fs, nperseg, noverlap, window)?;
    let pxy = csd(x, y, fs, nperseg, noverlap, window)?;
    let mut h = Vec::with_capacity(pxy.len());
    let mut gamma2 = Vec::with_capacity(pxy.len());
    for k in 0..pxy.len() {
        let (sxx, syy, sxy) = (pxx[k], pyy[k], pxy[k]);
        // |Pxy|^2, reused by both the coherence and the H2 division.
        let cross2 = sxy.re * sxy.re + sxy.im * sxy.im;
        let denom = sxx * syy;
        // Clamped: round-off on a perfectly coherent bin can land a hair
        // above 1, and a coherence above 1 is not a thing a caller should
        // ever have to reason about.
        gamma2.push(if denom > 1e-300 { (cross2 / denom).min(1.0) } else { 0.0 });
        h.push(match estimator {
            TfEstimator::H1 => {
                if sxx > 1e-300 {
                    sxy.scale(1.0 / sxx)
                } else {
                    Complex64::real(0.0)
                }
            }
            // Pyy / conj(Pxy), written as Pyy * Pxy / |Pxy|^2 since
            // 1/conj(z) == z/|z|^2 — one division instead of a complex one.
            TfEstimator::H2 => {
                if cross2 > 1e-300 {
                    sxy.scale(syy / cross2)
                } else {
                    Complex64::real(0.0)
                }
            }
            TfEstimator::Hv => {
                if sxx > 1e-300 {
                    Complex64::from_polar((syy / sxx).max(0.0).sqrt(), sxy.arg())
                } else {
                    Complex64::real(0.0)
                }
            }
        });
    }
    Ok((h, gamma2))
}

/// Direct `O(n^2)` discrete Fourier transform — the textbook definition
/// `X[k] = sum_n x[n] * exp(-2*pi*i*k*n/n_total)`, not the fast
/// (`fft_complex`) algorithm. An independent reference to validate the FFT
/// against, and the direct ancestor of [`goertzel`] below (Goertzel is
/// this same sum's recurrence form, evaluated at one `k`).
pub fn dft(input: &[Complex64]) -> Result<Vec<Complex64>, NumericError> {
    let n = input.len();
    if n == 0 {
        return Err(NumericError::EmptyInput("dft"));
    }
    Ok((0..n)
        .map(|k| {
            input
                .iter()
                .enumerate()
                .fold(Complex64::new(0.0, 0.0), |acc, (t, &x)| {
                    let angle = -TAU * (k * t) as f64 / n as f64;
                    acc.add(x.mul(Complex64::from_polar(1.0, angle)))
                })
        })
        .collect())
}

/// Inverse of [`dft`] — the same direct `O(n^2)` definition with a
/// positive-exponent sum scaled by `1/n`.
pub fn idft(input: &[Complex64]) -> Result<Vec<Complex64>, NumericError> {
    let n = input.len();
    if n == 0 {
        return Err(NumericError::EmptyInput("idft"));
    }
    Ok((0..n)
        .map(|t| {
            let sum = input
                .iter()
                .enumerate()
                .fold(Complex64::new(0.0, 0.0), |acc, (k, &x)| {
                    let angle = TAU * (k * t) as f64 / n as f64;
                    acc.add(x.mul(Complex64::from_polar(1.0, angle)))
                });
            sum.scale(1.0 / n as f64)
        })
        .collect())
}

/// The Goertzel algorithm: the DFT value at a single "bin" `k` in `O(n)`
/// instead of the `O(n^2)`/`O(n log n)` cost of computing (or extracting
/// one value from) a full transform. The recursion only ever needs
/// `cos(2*pi*k/n)`, so `k` need not be an integer — evaluating at a
/// non-integer `k` gives the DFT's value at an arbitrary frequency point
/// between the FFT's own grid bins, not just a fast way to recover an
/// on-grid one. This is what makes Goertzel a practical "K-point DFT"
/// primitive: computing a handful of specific frequencies (DTMF tone
/// detection, a single carrier) costs `O(n)` per frequency rather than
/// `O(n log n)` for the whole spectrum.
pub fn goertzel(input: &[f64], k: f64) -> Result<Complex64, NumericError> {
    let n = input.len();
    if n == 0 {
        return Err(NumericError::EmptyInput("goertzel"));
    }
    let omega = TAU * k / n as f64;
    let coeff = 2.0 * omega.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64); // s[n-1], s[n-2]
    for &x in input {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let twiddle = Complex64::from_polar(1.0, -omega);
    let raw = Complex64::new(s1, 0.0).sub(Complex64::new(s2, 0.0).mul(twiddle));
    // `s[N-1] - e^{-j*omega}*s[N-2]` equals `X(k) * e^{j*(N-1)*omega}`, not
    // `X(k)` outright — a phase term the classic integer-bin derivation
    // usually hides by running the recursion for an extra all-zero sample
    // (which only works when `k` is an integer, since it relies on
    // `e^{-j*omega*N} = 1`). Correcting for that phase directly here, by
    // exact derivation from the resonator's impulse response, is what lets
    // this same routine stay exact for a real-valued (non-integer) `k` too.
    let correction = Complex64::from_polar(1.0, -(n as f64 - 1.0) * omega);
    Ok(raw.mul(correction))
}

/// [`goertzel`] parameterized by a real frequency in Hz rather than a raw
/// bin index — the common case (a known tone frequency, a fixed sample
/// rate) doesn't require the caller to do the `k = freq * n / fs`
/// conversion by hand.
pub fn goertzel_freq(input: &[f64], fs: f64, freq: f64) -> Result<Complex64, NumericError> {
    let n = input.len();
    goertzel(input, freq * n as f64 / fs)
}

/// Vaníček's Least-Squares Spectral Analysis (LSSA) — P. Vaníček, "Further
/// development and properties of the spectral analysis by least-squares"
/// (1971). Unlike `dft`/`fft`, this does not require uniform sampling: at
/// each candidate frequency `f`, it fits `x(t) ~= a*cos(2*pi*f*t) +
/// b*sin(2*pi*f*t)` by ordinary least squares (via the same
/// `linalg::least_squares` every estimator in this crate already uses) and
/// reports `power(f) = (a^2+b^2) / var(x)` — the fraction of the data's
/// variance a pure sinusoid at `f` would explain. A single full-strength
/// sinusoid at its own frequency scores `power ~= 2.0` (since
/// `var(A*cos(wt+phi)) = A^2/2` over many periods, while the fit recovers
/// `a^2+b^2 = A^2`); noise and off-frequency points score far lower.
pub fn vanicek(t: &[f64], x: &[f64], freqs: &[f64]) -> Result<Vec<f64>, LinalgError> {
    if t.is_empty() || x.is_empty() {
        return Err(LinalgError::EmptyMatrix);
    }
    if t.len() != x.len() {
        return Err(LinalgError::Shape(crate::matrix::ShapeError::Broadcast {
            left: (t.len(), 1),
            right: (x.len(), 1),
        }));
    }
    let n = x.len();
    let mean = x.iter().sum::<f64>() / n as f64;
    let variance = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n as f64;
    let observations = Matrix::from_column(x);
    freqs
        .iter()
        .map(|&f| {
            let mut design = vec![0.0; n * 2];
            for (i, &ti) in t.iter().enumerate() {
                let angle = TAU * f * ti;
                design[i] = angle.cos(); // column 0
                design[n + i] = angle.sin(); // column 1
            }
            let a_matrix = Matrix::from_col_major(n, 2, design);
            let coeffs = linalg::least_squares(&a_matrix, &observations, None)?;
            let (a, b) = (coeffs.as_slice()[0], coeffs.as_slice()[1]);
            let power = if variance > 0.0 { (a * a + b * b) / variance } else { 0.0 };
            Ok(power)
        })
        .collect()
}

/// `conv`'s output-length convention (MATLAB `conv`/NumPy `convolve`'s own
/// naming): `Full` returns every nonzero-overlap sample (the default,
/// length `len(x)+len(h)-1`); `Same` returns the middle `len(x)` samples
/// (aligned with the input, the usual choice for "filter this signal in
/// place"); `Valid` returns only the samples where `h` fully overlaps `x`
/// (length `len(x)-len(h)+1`, requires `len(x) >= len(h)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvMode {
    Full,
    Same,
    Valid,
}

/// Below this, `full_conv` uses the direct `O(n*m)` loop; at or above it,
/// the FFT path. Chosen from an actual benchmark (`conv` vs. a zero-padded
/// `rfft`/`irfft` round trip, release build), not guessed: `min(n, m) >=
/// 512` was the exact dividing line across every one of ~22 measured
/// `(n, m)` pairs spanning short-kernel-over-long-signal (FFT never won —
/// a single whole-signal FFT is the wrong tool there; the actual fix is
/// overlap-add, not implemented) through comparable-length pairs (FFT won
/// decisively, e.g. 50000x50000: 453ms direct vs. 5.4ms FFT). Gating on
/// `min(n, m)` rather than `n*m` matters: a long signal against a *short*
/// kernel can have large `n*m` yet FFT still loses, because its cost is
/// set by the padded length `~n+m`, which a short `m` barely reduces —
/// exactly the `min(n,m)` term this threshold checks instead.
const CONV_FFT_MIN_DIM: usize = 512;

fn next_pow2(n: usize) -> usize {
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p
}

/// `full_conv`'s FFT-accelerated path: zero-pad both inputs to the same
/// power-of-two length (`rustfft` — see `fft_dispatch` — is dramatically
/// faster on composite/power-of-two lengths than the arbitrary lengths
/// Bluestein's algorithm has to fall back to), multiply their `rfft`
/// spectra elementwise, and `irfft` back — the standard "convolution
/// theorem" construction, real-only throughout since both inputs are real.
fn conv_fft(x: &[f64], h: &[f64]) -> Result<Vec<f64>, NumericError> {
    let full_len = x.len() + h.len() - 1;
    let fft_len = next_pow2(full_len);
    let mut xp = vec![0.0; fft_len];
    xp[..x.len()].copy_from_slice(x);
    let mut hp = vec![0.0; fft_len];
    hp[..h.len()].copy_from_slice(h);
    let xf = rfft(&xp)?;
    let hf = rfft(&hp)?;
    let yf: Vec<Complex64> = xf.iter().zip(&hf).map(|(a, b)| a.mul(*b)).collect();
    let padded = irfft(&yf, fft_len)?;
    Ok(padded[..full_len].to_vec())
}

/// Below this kernel length, `full_conv` never routes to overlap-add no
/// matter how long the signal gets — direct convolution's inner loop is
/// only `m` multiply-adds, and a benchmark (throwaway release-mode binary,
/// same discipline as `CONV_FFT_MIN_DIM`'s) found that's already cheaper
/// than the per-block FFT overhead at every kernel length tried at or below
/// this: `m=16` lost to direct by 1.3-1.6x all the way out to `n=100000`,
/// `m=32`/`m=48` hovered at break-even (0.9-1.3x, noise-dominated, no
/// reliable win at any `n`). The win becomes real and consistent starting
/// at `m=64` (0.55-0.86x direct's time, every `n` from 500 up) — so 64 is
/// the measured floor, not a round-number guess.
const OVERLAP_ADD_MIN_KERNEL: usize = 64;

/// Below this total size (`max(n,h)`), `full_conv` skips overlap-add even
/// if the kernel clears `OVERLAP_ADD_MIN_KERNEL` — every absolute time in
/// this regime is a fraction of a millisecond already (direct or
/// overlap-add), so there's nothing to gain by taking the more complex path,
/// and it keeps tiny/degenerate inputs (e.g. two comparably-short arrays
/// both under `CONV_FFT_MIN_DIM`) on the simple direct loop rather than
/// letting overlap-add's block-length heuristic degenerate into an
/// oversized single-block FFT. `m=64, n=1000` was the smallest case in the
/// benchmark with a clean, reproducible win (0.56x direct); `n=500` at the
/// same `m` was still technically faster but only by noise-level margins
/// (sub-4-microsecond absolute times).
const OVERLAP_ADD_MIN_SIGNAL: usize = 1000;

/// `full_conv`'s overlap-add path: the standard fix for "short FIR kernel
/// over a long signal" that a single whole-signal FFT (`conv_fft`) does not
/// help (see `CONV_FFT_MIN_DIM`'s doc comment) — chunk the longer input
/// into non-overlapping blocks, FFT the kernel once, FFT+multiply+IFFT each
/// block against it at a small fixed size, and add the overlapping tails
/// into a shared output buffer. Cost is `O(n/L * L*log(L))` for a signal of
/// length `n` and block length `L`, independent of how long `n` gets for a
/// fixed `L` — the reason this beats both direct (`O(n*m)`) and a
/// whole-signal FFT (`O((n+m)*log(n+m))`, all-at-once) once `n` is large
/// enough relative to `m`.
///
/// `x`/`h` may be given in either length order — full convolution is
/// commutative (`conv(x,h) == conv(h,x)`), so this treats whichever input
/// is longer as the "signal" to chunk and the shorter as the "kernel" to
/// FFT once, regardless of which the caller called `x` and which `h`.
///
/// Block length `L = next_pow2(8 * kernel_len)`, capped at the signal
/// length itself (so a near-comparable pair degenerates to one block
/// spanning the whole signal, rather than padding past it for no reason).
/// The `8x` multiplier is the standard "several times the kernel length"
/// heuristic, confirmed by the same benchmark that picked
/// `OVERLAP_ADD_MIN_KERNEL`/`OVERLAP_ADD_MIN_SIGNAL`: sweeping multipliers
/// 2/4/8/16/32/64 per `(n, m)` pair, the true optimum varied case to case
/// (anywhere from 2x to 16x depending on `m`), but a fixed `8x` landed
/// within about 10-35% of that per-case optimum everywhere, and — the part
/// that actually matters for picking a single constant — never lost to
/// direct convolution anywhere `full_conv` now dispatches to this path.
fn overlap_add(x: &[f64], h: &[f64]) -> Result<Vec<f64>, NumericError> {
    let (sig, ker) = if x.len() >= h.len() { (x, h) } else { (h, x) };
    let (n, m) = (sig.len(), ker.len());
    let block_len = (8 * m).clamp(1, n);
    let fft_len = next_pow2(block_len + m - 1);

    let mut kp = vec![0.0; fft_len];
    kp[..m].copy_from_slice(ker);
    let kf = rfft(&kp)?;

    let mut out = vec![0.0; n + m - 1];
    let mut start = 0;
    while start < n {
        let end = (start + block_len).min(n);
        let mut cp = vec![0.0; fft_len];
        cp[..end - start].copy_from_slice(&sig[start..end]);
        let cf = rfft(&cp)?;
        let yf: Vec<Complex64> = cf.iter().zip(&kf).map(|(a, b)| a.mul(*b)).collect();
        let block_out = irfft(&yf, fft_len)?;
        let seg_len = (end - start) + m - 1;
        for (i, &v) in block_out.iter().enumerate().take(seg_len) {
            out[start + i] += v;
        }
        start = end;
    }
    Ok(out)
}

/// Three-way dispatch: direct `O(n*m)` for small work, overlap-add for a
/// long signal against a short-to-medium kernel, whole-signal FFT
/// (`conv_fft`) for comparable-length pairs both at or above
/// `CONV_FFT_MIN_DIM` — see that constant's and `OVERLAP_ADD_MIN_KERNEL`/
/// `OVERLAP_ADD_MIN_SIGNAL`'s doc comments for the measurements behind each
/// boundary. The three ranges are mutually exclusive by construction
/// (overlap-add only fires when `min(n,m) < CONV_FFT_MIN_DIM`), so there's
/// no ambiguity about which path a given `(n, m)` takes.
fn full_conv(x: &[f64], h: &[f64]) -> Vec<f64> {
    let (n, m) = (x.len(), h.len());
    let min_dim = n.min(m);
    let max_dim = n.max(m);
    if min_dim >= CONV_FFT_MIN_DIM {
        if let Ok(v) = conv_fft(x, h) {
            return v;
        }
        // `conv_fft` can only fail on an internal FFT-length mismatch,
        // which `next_pow2`'s construction rules out — this is an
        // unreachable-in-practice safety net, not an expected path.
    } else if min_dim >= OVERLAP_ADD_MIN_KERNEL && max_dim >= OVERLAP_ADD_MIN_SIGNAL {
        if let Ok(v) = overlap_add(x, h) {
            return v;
        }
        // same unreachable-in-practice safety net as `conv_fft` above.
    }
    let mut full = vec![0.0; n + m - 1];
    for (i, &xi) in x.iter().enumerate() {
        for (j, &hj) in h.iter().enumerate() {
            full[i + j] += xi * hj;
        }
    }
    full
}

/// Discrete convolution `(x * h)[n] = sum_m x[m] * h[n-m]`. Three-way
/// dispatch in `full_conv`: the direct `O(n*m)` definition for small work,
/// overlap-add for a long signal against a short-to-medium kernel, a
/// whole-signal `rfft`/`irfft` round trip for comparable-length pairs both
/// at or above `CONV_FFT_MIN_DIM` — see `full_conv`'s own doc comment and
/// `CONV_FFT_MIN_DIM`/`OVERLAP_ADD_MIN_KERNEL`/`OVERLAP_ADD_MIN_SIGNAL` for
/// the measured thresholds.
pub fn conv(x: &[f64], h: &[f64], mode: ConvMode) -> Result<Vec<f64>, NumericError> {
    if x.is_empty() || h.is_empty() {
        return Err(NumericError::EmptyInput("conv"));
    }
    let (n, m) = (x.len(), h.len());
    let full = full_conv(x, h);
    match mode {
        ConvMode::Full => Ok(full),
        ConvMode::Same => {
            let start = (m - 1) / 2;
            Ok(full[start..start + n].to_vec())
        }
        ConvMode::Valid => {
            if n < m {
                return Err(NumericError::ShapeMismatch { expected: m, found: n });
            }
            let start = m - 1;
            Ok(full[start..start + (n - m + 1)].to_vec())
        }
    }
}

/// Cross-correlation `xcorr(x, y)[k] = sum_n x[n] * y[n-k]`, computed via
/// the standard identity `xcorr(x, y) == conv(x, reverse(y))` (every lag
/// with nonzero overlap, `len(x)+len(y)-1` values — MATLAB's/SciPy's own
/// default `mode='full'`). Autocorrelation is `xcorr(x, x)`.
pub fn xcorr(x: &[f64], y: &[f64]) -> Result<Vec<f64>, NumericError> {
    let mut reversed: Vec<f64> = y.to_vec();
    reversed.reverse();
    conv(x, &reversed, ConvMode::Full)
}

/// The analytic signal `x_a = x + j*H[x]`, via the standard FFT-based
/// construction (SciPy's `scipy.signal.hilbert`): zero the negative-
/// frequency half of `x`'s spectrum, double the positive-frequency half,
/// leave DC (and Nyquist, for even-length input) alone, then inverse-
/// transform. The imaginary part of the result is the Hilbert transform of
/// `x` itself; `abs(hilbert(x))` is `x`'s envelope.
pub fn hilbert(x: &[f64]) -> Result<Vec<Complex64>, NumericError> {
    ifft_complex(&analytic_spectrum(x)?)
}

/// The one-sided ("analytic") spectrum construction [`hilbert`] is built
/// on: DC (and Nyquist, for even-length input) unchanged, the positive-
/// frequency half doubled, the negative-frequency half zeroed. Exposed
/// separately because [`vmd`] needs the same one-sided spectrum as its
/// per-mode working representation, not just the time-domain analytic
/// signal `hilbert` itself returns.
pub(crate) fn analytic_spectrum(x: &[f64]) -> Result<Vec<Complex64>, NumericError> {
    if x.is_empty() {
        return Err(NumericError::EmptyInput("hilbert"));
    }
    let n = x.len();
    let complex_x: Vec<Complex64> = x.iter().map(|&v| Complex64::real(v)).collect();
    let mut spectrum = fft_complex(&complex_x)?;
    let half = n / 2;
    let even = n % 2 == 0;
    for (k, bin) in spectrum.iter_mut().enumerate().skip(1) {
        let factor = if even {
            if k < half { 2.0 } else if k == half { 1.0 } else { 0.0 }
        } else if k <= half {
            2.0
        } else {
            0.0
        };
        *bin = bin.scale(factor);
    }
    Ok(spectrum)
}

/// Pearson correlation coefficient between two equal-length samples,
/// `cov(x,y) / (std(x)*std(y))`. `corr(x, x) == 1` for any non-constant
/// `x`; a `NaN` denominator (a genuinely constant input) is guarded to
/// `0.0` rather than propagating `NaN` outright, matching the reference
/// convention `qu-interp` already used internally for `corr_heatmap`.
pub fn corr(x: &[f64], y: &[f64]) -> Result<f64, NumericError> {
    if x.is_empty() || y.is_empty() {
        return Err(NumericError::EmptyInput("corr"));
    }
    if x.len() != y.len() {
        return Err(NumericError::ShapeMismatch { expected: x.len(), found: y.len() });
    }
    let n = x.len() as f64;
    let mean_x = x.iter().sum::<f64>() / n;
    let mean_y = y.iter().sum::<f64>() / n;
    let cov: f64 = x.iter().zip(y).map(|(a, b)| (a - mean_x) * (b - mean_y)).sum();
    let var_x: f64 = x.iter().map(|a| (a - mean_x).powi(2)).sum();
    let var_y: f64 = y.iter().map(|b| (b - mean_y).powi(2)).sum();
    let denom = (var_x * var_y).sqrt();
    Ok(if denom > 1e-300 { cov / denom } else { 0.0 })
}

/// Total signal energy, `sum(x^2)` — the discrete analogue of `integral
/// x(t)^2 dt`, with no time localization (see [`tkeo`] for a per-sample
/// instantaneous estimate instead).
pub fn signal_energy(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}

/// The Teager-Kaiser Energy Operator: `psi[x(n)] = x(n)^2 - x(n-1)*x(n+1)`
/// — a nonlinear, near-instantaneous energy tracker (single-sample time
/// resolution, three multiplies per sample) that approximates the energy
/// needed to produce an AM-FM oscillation as amplitude^2 * frequency^2,
/// far cheaper than a full Hilbert-envelope estimate. Returns
/// `length(x)-2` values (the interior; the definition needs a neighbor on
/// each side, so — like `diff` — this doesn't pad to preserve length).
pub fn tkeo(x: &[f64]) -> Result<Vec<f64>, NumericError> {
    if x.len() < 3 {
        return Err(NumericError::EmptyInput("tkeo"));
    }
    Ok((1..x.len() - 1).map(|n| x[n] * x[n] - x[n - 1] * x[n + 1]).collect())
}

/// The prominence of the local maximum at `x[p]`: its height above the
/// higher of (a) the lowest point between it and the nearest point to the
/// left that exceeds it (or the left edge, if none does), and (b) the
/// same scan to the right. The standard peak-prominence definition (as in
/// SciPy's `scipy.signal.peak_prominences`, without the `wlen` window
/// restriction).
fn prominence(x: &[f64], p: usize) -> f64 {
    prominence_and_bases(x, p).0
}

/// Public entry point for [`prominence`] — `find_peaks`'s own `min_prominence`
/// filter already computes this internally, but callers that already have a
/// peak index (e.g. `findpeaks`'s enriched output, or `peak_width`'s own
/// caller) need the bare value too.
pub fn peak_prominence(x: &[f64], p: usize) -> f64 {
    prominence(x, p)
}

/// [`prominence`]'s full computation, also returning the two "bases" — the
/// index of the lowest point found on each side before the scan hit a
/// higher sample (or an edge) — since [`peak_width`] needs the exact same
/// bounding region SciPy's default (`wlen=None`) uses to find its
/// half-prominence crossings, not the whole signal. `prominence` above is a
/// thin wrapper that only keeps the height.
fn prominence_and_bases(x: &[f64], p: usize) -> (f64, usize, usize) {
    let height = x[p];
    let mut left_min = height;
    let mut left_base = p;
    let mut i = p;
    while i > 0 {
        i -= 1;
        if x[i] > height {
            break;
        }
        if x[i] < left_min {
            left_min = x[i];
            left_base = i;
        }
    }
    let mut right_min = height;
    let mut right_base = p;
    let mut j = p;
    while j + 1 < x.len() {
        j += 1;
        if x[j] > height {
            break;
        }
        if x[j] < right_min {
            right_min = x[j];
            right_base = j;
        }
    }
    (height - left_min.max(right_min), left_base, right_base)
}

/// Width of the peak at `x[p]`, at `rel_height` (SciPy's own default:
/// `0.5`, "full width at half prominence") of the way down from the peak to
/// its prominence-derived base: the reference level is
/// `x[p] - rel_height * prominence(x, p)`. Returns `(width, left_ip,
/// right_ip)` — the width in fractional samples, and the two interpolated
/// crossing positions it was measured between (useful for plotting the
/// width markers the way SciPy's own `peak_widths` return does).
///
/// Matches `scipy.signal.peak_widths`'s default (`wlen=None`) algorithm
/// exactly: starting at the peak, walk outward on each side while the
/// signal is still above the reference level, then linearly interpolate
/// between the last sample above it and the first sample at-or-below it to
/// locate the fractional crossing. The walk is bounded by the same
/// left/right bases `prominence` itself found, so a taller neighboring
/// peak's terrain cannot be mistaken for this peak's own.
pub fn peak_width(x: &[f64], p: usize, rel_height: f64) -> (f64, f64, f64) {
    let (prom, left_base, right_base) = prominence_and_bases(x, p);
    let ref_level = x[p] - rel_height * prom;

    let mut i = p;
    while i > left_base && ref_level < x[i] {
        i -= 1;
    }
    let mut left_ip = i as f64;
    if x[i] < ref_level && i + 1 <= p {
        let denom = x[i + 1] - x[i];
        if denom.abs() > 1e-300 {
            left_ip += (ref_level - x[i]) / denom;
        }
    }

    let mut j = p;
    while j < right_base && ref_level < x[j] {
        j += 1;
    }
    let mut right_ip = j as f64;
    if x[j] < ref_level && j >= 1 {
        let denom = x[j - 1] - x[j];
        if denom.abs() > 1e-300 {
            right_ip -= (ref_level - x[j]) / denom;
        }
    }

    (right_ip - left_ip, left_ip, right_ip)
}

/// Local-maximum peak detection: a strict local max (`x[i] > x[i-1]` and
/// `x[i] > x[i+1]`), OR a flat-topped plateau bounded by strictly lower
/// neighbors on both sides (reported at the plateau's midpoint, rounded
/// down on an even-width plateau — the same "lower index on a tie"
/// convention `argmedian` already uses), that also clears every supplied
/// filter. `min_distance` (in samples) is enforced greedily by height —
/// the tallest surviving peak is accepted first, then each next-tallest
/// candidate is accepted only if it isn't within `min_distance` of an
/// already-accepted one (SciPy's own `find_peaks` strategy) — so a
/// min-distance cluster keeps its tallest member, not just the first one
/// scanned.
pub fn find_peaks(
    x: &[f64],
    min_height: Option<f64>,
    min_distance: Option<usize>,
    min_prominence: Option<f64>,
) -> Vec<usize> {
    if x.len() < 3 {
        return Vec::new();
    }
    let n = x.len();
    let mut raw_candidates: Vec<usize> = Vec::new();
    let mut i = 1;
    while i < n - 1 {
        if x[i] > x[i - 1] {
            let mut j = i;
            while j + 1 < n && x[j + 1] == x[i] {
                j += 1;
            }
            if j < n - 1 && x[j + 1] < x[i] {
                raw_candidates.push(i + (j - i) / 2);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    let candidates: Vec<usize> = raw_candidates
        .into_iter()
        .filter(|&i| min_height.is_none_or(|h| x[i] >= h))
        .filter(|&i| min_prominence.is_none_or(|p| prominence(x, i) >= p))
        .collect();
    match min_distance {
        Some(dist) if dist > 0 => select_by_peak_distance(&candidates, x, dist),
        _ => candidates,
    }
}

/// Greedy tallest-first distance suppression: sort candidates by height
/// descending (stable, so an exact height tie keeps the earlier-index
/// candidate first — same rule scipy's priority-order removal produces),
/// then accept a candidate only if it is at least `dist` away from every
/// already-accepted one.
///
/// This is `O(n log n)`, not the `O(candidates × accepted)` a naive
/// "scan every accepted peak for every candidate" loop costs. The trick:
/// `candidates` arrives already sorted by POSITION (ascending), so anyone
/// within `dist` of a given candidate forms a contiguous run in that
/// order. A doubly linked list over `candidates` (by position) lets us
/// walk outward from a newly-accepted candidate and unlink every neighbor
/// within `dist` in O(1) per unlink — each candidate is unlinked at most
/// once, so the whole sweep phase is O(n) after the initial sort.
///
/// Correctness (why skipping already-removed neighbors via the linked
/// list — rather than rescanning every accepted peak — still finds
/// exactly the same accepted set): process candidates in tallest-first
/// order. By induction, whenever a candidate reaches its own turn without
/// having been removed, it cannot be within `dist` of any already-accepted
/// candidate — if it were, the taller accepted candidate would have swept
/// and removed it during its own (earlier) turn, since at that time this
/// candidate was still present in the list. So the two algorithms (scan
/// all accepted vs. sweep-and-unlink) always agree on both the accepted
/// set and the tie-break.
fn select_by_peak_distance(candidates: &[usize], x: &[f64], dist: usize) -> Vec<usize> {
    let m = candidates.len();
    if m == 0 {
        return Vec::new();
    }
    let mut prev: Vec<Option<usize>> = (0..m).map(|i| i.checked_sub(1)).collect();
    let mut next: Vec<Option<usize>> = (0..m).map(|i| if i + 1 < m { Some(i + 1) } else { None }).collect();
    let mut removed = vec![false; m];

    // Tallest first; a stable sort on a Vec already in ascending-index
    // order keeps exact-height ties in that same ascending-index order.
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| x[candidates[b]].partial_cmp(&x[candidates[a]]).unwrap_or(std::cmp::Ordering::Equal));

    let mut accepted: Vec<usize> = Vec::with_capacity(m);
    for idx in order {
        if removed[idx] {
            continue;
        }
        accepted.push(candidates[idx]);

        // Sweep left, unlinking every remaining neighbor within `dist`.
        let mut p = prev[idx];
        while let Some(pp) = p {
            if candidates[idx] - candidates[pp] >= dist {
                break;
            }
            removed[pp] = true;
            p = prev[pp];
        }
        prev[idx] = p;
        if let Some(pp) = p {
            next[pp] = Some(idx);
        }

        // Sweep right symmetrically.
        let mut nx = next[idx];
        while let Some(nn) = nx {
            if candidates[nn] - candidates[idx] >= dist {
                break;
            }
            removed[nn] = true;
            nx = next[nn];
        }
        next[idx] = nx;
        if let Some(nn) = nx {
            prev[nn] = Some(idx);
        }
    }
    accepted.sort_unstable();
    accepted
}

// =====================================================================
// Oscilloscope-style pulse and edge measurements
// (`docs/design/toolkit-signal.md` §6 "Events", §8 "pulse measurements")
//
// Everything below is built on ONE primitive, `find_trigger`, so that
// "where does this signal cross a level" has exactly one definition in
// the engine rather than five subtly different ones. The measurement
// layer (`rise_time`, `pulse_period`, ...) adds sub-sample linear
// interpolation on top of the integer indices the locator layer reports;
// see `crossing_position`.
// =====================================================================

/// Which way a threshold crossing goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Rising,
    Falling,
    Both,
}

/// Sample indices where `x` crosses `level` in the requested direction.
///
/// The crossing is defined on the BOOLEAN state `x[i] >= level`, and the
/// index reported is the first sample of the new state — so a rising
/// crossing at `i` means `x[i-1] < level <= x[i]`, and a falling one
/// means `x[i-1] >= level > x[i]`.
///
/// Treating "exactly at the level" as belonging to the HIGH side is not
/// cosmetic: because the state is a single boolean, rising and falling
/// crossings are guaranteed to strictly ALTERNATE, which is what lets
/// `find_pulses` pair them without a re-scan and without ever emitting a
/// pulse whose end precedes its start. The symmetric-looking alternative
/// (`prev < level && cur >= level` for rising, `prev > level && cur <=
/// level` for falling) loses that property: a sample landing exactly ON
/// the level produces a rising crossing with no matching falling one.
///
/// A transition involving a NaN is skipped rather than reported. NaN
/// compares false against everything, so it would otherwise read as
/// "low" and manufacture a pair of edges out of a dropout — inventing
/// pulses where the record merely has missing samples.
pub fn find_trigger(x: &[f64], level: f64, edge: Edge) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 1..x.len() {
        if x[i - 1].is_nan() || x[i].is_nan() {
            continue;
        }
        let was_high = x[i - 1] >= level;
        let is_high = x[i] >= level;
        if was_high == is_high {
            continue;
        }
        let wanted = match edge {
            Edge::Rising => is_high,
            Edge::Falling => !is_high,
            Edge::Both => true,
        };
        if wanted {
            out.push(i);
        }
    }
    out
}

/// The sub-sample position, in fractional samples, at which the segment
/// ending at index `i` crosses `level` — linear interpolation between
/// `x[i-1]` and `x[i]`, the same thing a scope does when it reports a
/// time between two samples.
///
/// Returns `i` itself when there is no segment to interpolate over
/// (`i == 0`) or when the two samples are equal (a vertical step gives
/// no information about where inside the interval the crossing fell).
pub fn crossing_position(x: &[f64], i: usize, level: f64) -> f64 {
    if i == 0 || i >= x.len() {
        return i as f64;
    }
    let (a, b) = (x[i - 1], x[i]);
    let denom = b - a;
    if !denom.is_finite() || denom.abs() < 1e-300 {
        return i as f64;
    }
    (i - 1) as f64 + (level - a) / denom
}

/// Walk an already-detected rising crossing back to where the signal
/// actually passed `level`, and interpolate there.
///
/// Without hysteresis this is a no-op: the detected index already IS the
/// mid-threshold crossing. It matters when `find_edges` used a Schmitt
/// scheme, where the edge is reported at the far (high) threshold — the
/// measurement must still be taken at the nominal threshold, or every
/// width and period would carry the hysteresis band's own slew time.
fn refine_rising(x: &[f64], i: usize, level: f64) -> f64 {
    let mut j = i;
    while j > 0 && x[j - 1] >= level {
        j -= 1;
    }
    crossing_position(x, j, level)
}

fn refine_falling(x: &[f64], i: usize, level: f64) -> f64 {
    let mut j = i;
    while j > 0 && x[j - 1] < level {
        j -= 1;
    }
    crossing_position(x, j, level)
}

/// Midpoint of `x`'s range, `(min + max) / 2` over the finite samples —
/// the default threshold when a caller does not name one. `None` when
/// there is no finite sample to work from.
pub fn midpoint_level(x: &[f64]) -> Option<f64> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in x.iter().filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo > hi { None } else { Some((lo + hi) / 2.0) }
}

/// A detected set of edges: integer sample indices, their directions,
/// and each one's interpolated sub-sample position at the NOMINAL
/// threshold.
pub struct EdgeSet {
    pub indices: Vec<usize>,
    pub rising: Vec<bool>,
    pub positions: Vec<f64>,
}

/// Rising and falling edges of `x` about `threshold`.
///
/// With `hysteresis <= 0` this is exactly `find_trigger(.., Edge::Both)`.
/// With a positive `hysteresis` it is a Schmitt trigger: the signal must
/// reach `threshold + hysteresis/2` to be called high and fall to
/// `threshold - hysteresis/2` to be called low, so noise riding on a slow
/// edge produces one edge instead of a burst of them. The reported
/// POSITIONS are still taken at `threshold` either way (see
/// `refine_rising`), so turning hysteresis on to reject glitches does not
/// move the measurements it is protecting.
pub fn find_edges(x: &[f64], threshold: f64, hysteresis: f64) -> EdgeSet {
    let mut set = EdgeSet { indices: Vec::new(), rising: Vec::new(), positions: Vec::new() };
    if hysteresis <= 0.0 || !hysteresis.is_finite() {
        for i in find_trigger(x, threshold, Edge::Both) {
            let up = x[i] >= threshold;
            set.indices.push(i);
            set.rising.push(up);
            set.positions
                .push(if up { refine_rising(x, i, threshold) } else { refine_falling(x, i, threshold) });
        }
        return set;
    }

    let hi = threshold + hysteresis / 2.0;
    let lo = threshold - hysteresis / 2.0;
    // Seed the state from the first finite sample, so a record that
    // starts high does not report a spurious first rising edge.
    let mut state = match x.iter().find(|v| !v.is_nan()) {
        Some(&v) => v >= threshold,
        None => return set,
    };
    for (i, &v) in x.iter().enumerate() {
        if v.is_nan() {
            continue;
        }
        if !state && v >= hi {
            state = true;
            set.indices.push(i);
            set.rising.push(true);
            set.positions.push(refine_rising(x, i, threshold));
        } else if state && v <= lo {
            state = false;
            set.indices.push(i);
            set.rising.push(false);
            set.positions.push(refine_falling(x, i, threshold));
        }
    }
    set
}

/// A pulse: a rising edge and the next falling one (or the reverse, for
/// `positive == false`), with the width measured between their
/// interpolated positions.
pub struct Pulse {
    pub start: usize,
    pub end: usize,
    pub width: f64,
}

/// Pairs of consecutive opposite-going edges.
///
/// A partial pulse at either end of the record — a signal that is
/// already high when capture starts, or still high when it stops — has
/// only one of its two edges present and is therefore NOT reported. That
/// is deliberate: its width is unknown, and reporting a truncated one is
/// the kind of believable-but-wrong number this codebase keeps getting
/// bitten by.
pub fn find_pulses(x: &[f64], threshold: f64, hysteresis: f64, positive: bool) -> Vec<Pulse> {
    let set = find_edges(x, threshold, hysteresis);
    let mut out = Vec::new();
    for k in 0..set.indices.len().saturating_sub(1) {
        if set.rising[k] == positive && set.rising[k + 1] != positive {
            out.push(Pulse {
                start: set.indices[k],
                end: set.indices[k + 1],
                width: set.positions[k + 1] - set.positions[k],
            });
        }
    }
    out
}

/// Mean interval, in samples, between successive rising crossings of
/// `level` — the pulse train's period. `None` when fewer than two rising
/// crossings exist, i.e. when the record does not contain a full cycle.
pub fn pulse_period(x: &[f64], level: f64, hysteresis: f64) -> Option<f64> {
    let set = find_edges(x, level, hysteresis);
    let ups: Vec<f64> = set
        .positions
        .iter()
        .zip(set.rising.iter())
        .filter(|(_, &r)| r)
        .map(|(&p, _)| p)
        .collect();
    if ups.len() < 2 {
        return None;
    }
    // Mean of the consecutive differences telescopes to the span over
    // the gap count -- computed that way so a long record does not
    // accumulate rounding across every individual difference.
    Some((ups[ups.len() - 1] - ups[0]) / (ups.len() - 1) as f64)
}

/// Mean width, in samples, of the pulses of the requested polarity.
pub fn mean_pulse_width(x: &[f64], level: f64, hysteresis: f64, positive: bool) -> Option<f64> {
    let pulses = find_pulses(x, level, hysteresis, positive);
    if pulses.is_empty() {
        return None;
    }
    Some(pulses.iter().map(|p| p.width).sum::<f64>() / pulses.len() as f64)
}

/// Duty cycle as a FRACTION in `[0, 1]`: mean high time over mean period.
pub fn duty_cycle(x: &[f64], level: f64, hysteresis: f64) -> Option<f64> {
    let width = mean_pulse_width(x, level, hysteresis, true)?;
    let period = pulse_period(x, level, hysteresis)?;
    if !period.is_finite() || period == 0.0 {
        return None;
    }
    Some(width / period)
}

/// 10%–90% (or `low`–`high`) transition time of the first complete
/// rising transition in `x`, in fractional samples.
///
/// `base`/`top` are the levels the transition runs between. The caller
/// supplies them; `rise_fall_levels` below computes the default pair.
///
/// The transition located is: the FIRST rising crossing of the high
/// level, paired with the LAST rising crossing of the low level at or
/// before it. Pairing backwards from the high crossing (rather than
/// forwards from the first low crossing) is what makes this correct on a
/// signal that wanders across the low level a few times before finally
/// committing to the transition — it measures the edge that actually
/// arrived, not the first hesitation.
pub fn rise_time(x: &[f64], base: f64, top: f64, low: f64, high: f64) -> Option<f64> {
    let span = top - base;
    let (lo_lvl, hi_lvl) = (base + low * span, base + high * span);
    let hi_idx = *find_trigger(x, hi_lvl, Edge::Rising).first()?;
    let lo_idx = *find_trigger(x, lo_lvl, Edge::Rising).iter().filter(|&&i| i <= hi_idx).next_back()?;
    Some(crossing_position(x, hi_idx, hi_lvl) - crossing_position(x, lo_idx, lo_lvl))
}

/// The mirror of `rise_time`: the first complete FALLING transition,
/// from the high level down to the low one.
pub fn fall_time(x: &[f64], base: f64, top: f64, low: f64, high: f64) -> Option<f64> {
    let span = top - base;
    let (lo_lvl, hi_lvl) = (base + low * span, base + high * span);
    let lo_idx = *find_trigger(x, lo_lvl, Edge::Falling).first()?;
    let hi_idx = *find_trigger(x, hi_lvl, Edge::Falling).iter().filter(|&&i| i <= lo_idx).next_back()?;
    Some(crossing_position(x, lo_idx, lo_lvl) - crossing_position(x, hi_idx, hi_lvl))
}

/// Default `(base, top)` for a rise/fall measurement: the signal's own
/// min and max over its finite samples.
///
/// This is the predictable choice, not the clever one, and it has a real
/// failure mode worth stating: on a step that RINGS, the max is the
/// overshoot peak rather than the settled top, which drags the 90% level
/// up and reports a rise time that is too long. Pass `base=`/`top=`
/// explicitly (or the settled value from `overshoot`'s own reckoning)
/// when the step overshoots.
pub fn rise_fall_levels(x: &[f64]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in x.iter().filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo > hi { None } else { Some((lo, hi)) }
}

/// Mean of the first / last `frac` of the finite samples (at least one
/// sample each) — the default "initial level" and "settled level" of a
/// step response.
pub fn settled_levels(x: &[f64], frac: f64) -> Option<(f64, f64)> {
    let finite: Vec<f64> = x.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return None;
    }
    let k = ((finite.len() as f64 * frac).round() as usize).clamp(1, finite.len());
    let head = finite[..k].iter().sum::<f64>() / k as f64;
    let tail = finite[finite.len() - k..].iter().sum::<f64>() / k as f64;
    Some((head, tail))
}

/// Step-response overshoot and undershoot, each as a PERCENT of the
/// step's own size `|final - initial|`.
///
/// Overshoot is how far the response travels PAST its settled value, in
/// the direction the step was going; undershoot is how far it backs up
/// past where it STARTED (the pre-shoot / recoil), which is the
/// convention MATLAB's `stepinfo` uses. Both are clamped at zero — a
/// response that never exceeds either bound has 0% of that quantity, not
/// a negative amount of it. Works for a falling step as well as a rising
/// one: the roles of min and max swap with the sign of the step.
///
/// `None` when the step has no size to be a percentage of.
pub fn step_shoot(x: &[f64], initial: f64, settled: f64) -> Option<(f64, f64)> {
    let span = settled - initial;
    if !span.is_finite() || span == 0.0 {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in x.iter().filter(|v| v.is_finite()) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo > hi {
        return None;
    }
    let (beyond_settled, beyond_initial) =
        if span > 0.0 { (hi - settled, initial - lo) } else { (settled - lo, hi - initial) };
    let mag = span.abs();
    Some((100.0 * (beyond_settled / mag).max(0.0), 100.0 * (beyond_initial / mag).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    #[test]
    fn rfft_is_half_of_the_full_spectrum() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let full = fft_real(&x).unwrap();
        let half = rfft(&x).unwrap();
        assert_eq!(half.len(), 5); // 8/2 + 1
        for (a, b) in half.iter().zip(full.bins().iter()) {
            close(a.re, b.re, 1e-9);
            close(a.im, b.im, 1e-9);
        }
    }

    #[test]
    fn irfft_round_trips_rfft_for_even_and_odd_lengths() {
        for &n in &[8usize, 9, 15, 16, 50] {
            let x: Vec<f64> = (0..n).map(|k| (k as f64 * 0.37).sin()).collect();
            let half = rfft(&x).unwrap();
            let recovered = irfft(&half, n).unwrap();
            for (a, b) in x.iter().zip(recovered.iter()) {
                close(*a, *b, 1e-9);
            }
        }
    }

    #[test]
    fn irfft_rejects_a_mismatched_half_spectrum() {
        let bad = vec![Complex64::new(0.0, 0.0); 3];
        assert_eq!(
            irfft(&bad, 8),
            Err(NumericError::ShapeMismatch { expected: 5, found: 3 })
        );
    }

    #[test]
    fn dct_idct_round_trip_is_exact() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let coeffs = dct(&x);
        let recovered = idct(&coeffs);
        for (a, b) in x.iter().zip(recovered.iter()) {
            close(*a, *b, 1e-9);
        }
    }

    #[test]
    fn dct_of_a_constant_concentrates_in_bin_zero() {
        let x = [3.0; 8];
        let coeffs = dct(&x);
        // bin 0 carries all the energy for a DC input; every other basis
        // function is orthogonal to a constant, so its coefficient is ~0.
        assert!(coeffs[0].abs() > 1.0);
        for &c in &coeffs[1..] {
            close(c, 0.0, 1e-9);
        }
    }

    #[test]
    fn haar_dwt_round_trips() {
        let x = [4.0, 2.0, 5.0, 1.0, 3.0, 3.0, 8.0, 0.0];
        let (approx, detail) = dwt_haar(&x).unwrap();
        assert_eq!(approx.len(), 4);
        let recovered = idwt_haar(&approx, &detail).unwrap();
        for (a, b) in x.iter().zip(recovered.iter()) {
            close(*a, *b, 1e-9);
        }
    }

    #[test]
    fn haar_dwt_rejects_odd_length() {
        assert_eq!(dwt_haar(&[1.0, 2.0, 3.0]), Err(NumericError::OddLength { length: 3 }));
    }

    #[test]
    fn haar_dwt_is_orthogonal_energy_preserving() {
        // Parseval: sum(approx^2) + sum(detail^2) == sum(x^2) for an
        // orthogonal transform.
        let x = [4.0, 2.0, 5.0, 1.0, 3.0, 3.0, 8.0, 0.0];
        let (approx, detail) = dwt_haar(&x).unwrap();
        let energy_in: f64 = x.iter().map(|v| v * v).sum();
        let energy_out: f64 =
            approx.iter().map(|v| v * v).sum::<f64>() + detail.iter().map(|v| v * v).sum::<f64>();
        close(energy_in, energy_out, 1e-9);
    }

    #[test]
    fn stft_shape_and_reassembly() {
        let n = 512;
        let x: Vec<f64> = (0..n).map(|k| (k as f64 * 0.1).sin()).collect();
        let nfft = 64;
        let hop = 32;
        let s = stft(&x, nfft, hop).unwrap();
        let expected_frames = (n - nfft) / hop + 1;
        assert_eq!(s.shape(), (nfft, expected_frames));
    }

    #[test]
    fn stft_locates_a_tone_in_frequency() {
        let n = 1024;
        let fs = 1024.0;
        let tone_bin = 8.0;
        let x: Vec<f64> = (0..n)
            .map(|k| (TAU * tone_bin * k as f64 / fs).sin())
            .collect();
        let nfft = 128;
        let s = stft(&x, nfft, nfft / 2).unwrap();
        // every frame should show its peak magnitude at (or next to) bin 8
        // (tone_bin * nfft / fs = 8 * 128/1024 = 1... use a bin scaled to nfft)
        let bin = (tone_bin * nfft as f64 / fs).round() as usize;
        let col0 = s.col_vec(1).unwrap(); // skip frame 0 (windowing edge effects)
        let peak = (0..nfft / 2)
            .max_by(|&a, &b| col0[a].magnitude().total_cmp(&col0[b].magnitude()))
            .unwrap();
        assert_eq!(peak, bin, "expected the spectral peak at bin {bin}, got {peak}");
    }

    #[test]
    fn welch_locates_a_pure_tones_true_frequency() {
        let fs = 1000.0;
        let freq = 123.0; // not bin-aligned to any convenient nperseg, on purpose
        let n = 8000;
        let x: Vec<f64> = (0..n).map(|k| (TAU * freq * k as f64 / fs).sin()).collect();
        let nperseg = 512;
        let window = hann_window(nperseg);
        let psd = welch(&x, fs, nperseg, nperseg / 2, &window).unwrap();
        let peak_bin = (0..psd.len()).max_by(|&a, &b| psd[a].total_cmp(&psd[b])).unwrap();
        let peak_hz = peak_bin as f64 * fs / nperseg as f64;
        assert!(
            (peak_hz - freq).abs() <= fs / nperseg as f64,
            "expected the PSD peak within one bin of {freq} Hz, got {peak_hz} Hz"
        );
    }

    #[test]
    fn welch_integrates_to_roughly_the_signals_mean_square_power() {
        let fs = 1000.0;
        let freq = 50.0;
        let amplitude = 2.0;
        let n = 8000;
        let x: Vec<f64> = (0..n)
            .map(|k| amplitude * (TAU * freq * k as f64 / fs).sin())
            .collect();
        let nperseg = 256;
        let window = hann_window(nperseg);
        let psd = welch(&x, fs, nperseg, nperseg / 2, &window).unwrap();
        let df = fs / nperseg as f64;
        let integrated_power: f64 = psd.iter().sum::<f64>() * df;
        // a zero-mean sine of amplitude A has mean-square power A^2/2.
        let expected_power = amplitude * amplitude / 2.0;
        assert!(
            (integrated_power - expected_power).abs() / expected_power < 0.05,
            "expected integrated PSD near {expected_power}, got {integrated_power}"
        );
    }

    #[test]
    fn periodogram_is_welch_with_one_full_length_segment() {
        let fs = 500.0;
        let x: Vec<f64> = (0..300).map(|k| (TAU * 40.0 * k as f64 / fs).sin()).collect();
        let window = hann_window(x.len());
        let direct = periodogram(&x, fs, &window).unwrap();
        let via_welch = welch(&x, fs, x.len(), 0, &window).unwrap();
        assert_eq!(direct.len(), x.len() / 2 + 1);
        for (a, b) in direct.iter().zip(via_welch.iter()) {
            close(*a, *b, 1e-12);
        }
    }

    #[test]
    fn welch_rejects_a_window_length_mismatch() {
        let x = vec![0.0; 100];
        let window = hann_window(32); // wrong length for nperseg=64
        let err = welch(&x, 100.0, 64, 32, &window).unwrap_err();
        assert!(matches!(err, NumericError::ShapeMismatch { expected: 64, found: 32 }));
    }

    #[test]
    fn welch_rejects_noverlap_at_or_above_nperseg() {
        let x = vec![0.0; 100];
        let window = hann_window(32);
        let err = welch(&x, 100.0, 32, 32, &window).unwrap_err();
        assert!(matches!(err, NumericError::ElementLimit { .. }));
    }

    #[test]
    fn dft_matches_the_fast_fft_for_a_real_signal() {
        let x: Vec<f64> = (0..16).map(|k| (k as f64 * 0.4).sin() + 0.3 * (k as f64 * 1.1).cos()).collect();
        let complex_x: Vec<Complex64> = x.iter().map(|&v| Complex64::real(v)).collect();
        let via_fft = fft_complex(&complex_x).unwrap();
        let via_dft = dft(&complex_x).unwrap();
        for (a, b) in via_fft.iter().zip(via_dft.iter()) {
            close(a.re, b.re, 1e-9);
            close(a.im, b.im, 1e-9);
        }
    }

    #[test]
    fn idft_of_dft_round_trips() {
        let complex_x: Vec<Complex64> = (0..12).map(|k| Complex64::new(k as f64, (k as f64 * 0.5).sin())).collect();
        let spectrum = dft(&complex_x).unwrap();
        let recovered = idft(&spectrum).unwrap();
        for (a, b) in complex_x.iter().zip(recovered.iter()) {
            close(a.re, b.re, 1e-9);
            close(a.im, b.im, 1e-9);
        }
    }

    #[test]
    fn dft_rejects_empty_input() {
        assert!(matches!(dft(&[]), Err(NumericError::EmptyInput("dft"))));
    }

    #[test]
    fn goertzel_matches_the_direct_dft_at_every_integer_bin() {
        let x: Vec<f64> = (0..20).map(|k| (k as f64 * 0.3).sin() + 0.5 * (k as f64 * 0.9).cos()).collect();
        let complex_x: Vec<Complex64> = x.iter().map(|&v| Complex64::real(v)).collect();
        let spectrum = dft(&complex_x).unwrap();
        for k in 0..20 {
            let via_goertzel = goertzel(&x, k as f64).unwrap();
            close(via_goertzel.re, spectrum[k].re, 1e-9);
            close(via_goertzel.im, spectrum[k].im, 1e-9);
        }
    }

    #[test]
    fn goertzel_at_a_non_integer_bin_agrees_with_the_direct_partial_sum() {
        // the whole point of Goertzel accepting a real `k`: it must match
        // the textbook DFT sum evaluated at that same non-integer frequency,
        // not just an on-grid FFT bin.
        let x: Vec<f64> = (0..10).map(|k| (k as f64 * 0.7).cos()).collect();
        let n = x.len();
        let k = 3.37;
        let direct: Complex64 = x.iter().enumerate().fold(Complex64::new(0.0, 0.0), |acc, (t, &xt)| {
            let angle = -TAU * k * t as f64 / n as f64;
            acc.add(Complex64::real(xt).mul(Complex64::from_polar(1.0, angle)))
        });
        let via_goertzel = goertzel(&x, k).unwrap();
        close(via_goertzel.re, direct.re, 1e-9);
        close(via_goertzel.im, direct.im, 1e-9);
    }

    #[test]
    fn goertzel_freq_finds_a_known_tone() {
        let fs = 1000.0;
        let n = 200;
        let tone_hz = 60.0;
        let x: Vec<f64> = (0..n).map(|i| (TAU * tone_hz * i as f64 / fs).sin()).collect();
        let at_tone = goertzel_freq(&x, fs, tone_hz).unwrap().magnitude();
        let off_tone = goertzel_freq(&x, fs, tone_hz * 2.0).unwrap().magnitude();
        assert!(at_tone > off_tone * 5.0, "at_tone={at_tone}, off_tone={off_tone}");
    }

    #[test]
    fn vanicek_scores_a_pure_tone_near_the_textbook_value_of_two() {
        let fs = 100.0;
        let n = 300;
        let tone_hz = 5.0;
        let amplitude = 2.0;
        let t: Vec<f64> = (0..n).map(|i| i as f64 / fs).collect();
        let x: Vec<f64> = t.iter().map(|&ti| amplitude * (TAU * tone_hz * ti).sin()).collect();
        let freqs = vec![1.0, 3.0, tone_hz, 8.0, 12.0];
        let power = vanicek(&t, &x, &freqs).unwrap();
        let tone_index = 2;
        // var(A*sin) = A^2/2 over many full periods, and the LS fit
        // recovers a^2+b^2 = A^2 at the true frequency, so power ~= 2.0.
        close(power[tone_index], 2.0, 0.05);
        for (i, &p) in power.iter().enumerate() {
            if i != tone_index {
                assert!(p < power[tone_index] / 5.0, "off-frequency power {p} too close to on-frequency {}", power[tone_index]);
            }
        }
    }

    #[test]
    fn vanicek_handles_unevenly_spaced_samples() {
        // the entire point of Vaníček's method over a plain FFT: `t` need
        // not be uniformly spaced. Drop every third sample from an
        // otherwise-uniform grid to build a genuinely irregular one.
        let fs = 100.0;
        let tone_hz = 4.0;
        let amplitude = 1.5;
        let (t, x): (Vec<f64>, Vec<f64>) = (0..300)
            .filter(|i| i % 3 != 0)
            .map(|i| {
                let ti = i as f64 / fs;
                (ti, amplitude * (TAU * tone_hz * ti).sin())
            })
            .unzip();
        let power = vanicek(&t, &x, &[1.0, tone_hz, 10.0]).unwrap();
        assert!(power[1] > power[0] * 5.0 && power[1] > power[2] * 5.0, "power = {power:?}");
    }

    #[test]
    fn vanicek_rejects_mismatched_lengths() {
        let err = vanicek(&[0.0, 1.0, 2.0], &[0.0, 1.0], &[1.0]).unwrap_err();
        assert!(matches!(err, LinalgError::Shape(_)));
    }

    #[test]
    fn conv_full_matches_a_hand_computed_example() {
        // [1,2,3] * [0,1,0.5] worked out by hand: full convolution is
        // [1*0, 1*1+2*0, 1*0.5+2*1+3*0, 2*0.5+3*1, 3*0.5]
        //   = [0, 1, 2.5, 4, 1.5]
        let result = conv(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5], ConvMode::Full).unwrap();
        close_vec(&result, &[0.0, 1.0, 2.5, 4.0, 1.5]);
    }

    fn close_vec(a: &[f64], b: &[f64]) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            close(*x, *y, 1e-9);
        }
    }

    #[test]
    fn conv_same_returns_input_length_centered_on_full() {
        let full = conv(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5], ConvMode::Full).unwrap();
        let same = conv(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5], ConvMode::Same).unwrap();
        assert_eq!(same.len(), 3);
        close_vec(&same, &full[1..4]);
    }

    #[test]
    fn conv_valid_only_where_h_fully_overlaps_x() {
        let result = conv(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0], ConvMode::Valid).unwrap();
        // valid length = 4 - 2 + 1 = 3: [1+2, 2+3, 3+4]
        close_vec(&result, &[3.0, 5.0, 7.0]);
    }

    #[test]
    fn conv_valid_rejects_a_kernel_longer_than_the_signal() {
        let err = conv(&[1.0, 2.0], &[1.0, 1.0, 1.0], ConvMode::Valid).unwrap_err();
        assert_eq!(err, NumericError::ShapeMismatch { expected: 3, found: 2 });
    }

    #[test]
    fn conv_rejects_empty_input() {
        assert_eq!(conv(&[], &[1.0], ConvMode::Full), Err(NumericError::EmptyInput("conv")));
    }

    #[test]
    fn xcorr_autocorrelation_peaks_at_zero_lag() {
        let x = [1.0, 2.0, -1.0, 3.0, 0.5];
        let r = xcorr(&x, &x).unwrap();
        // zero lag sits at the middle index of a length-(2n-1) full xcorr.
        let zero_lag = r.len() / 2;
        let peak_index = r.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap().0;
        assert_eq!(peak_index, zero_lag, "autocorrelation must peak at zero lag");
        let energy: f64 = x.iter().map(|v| v * v).sum();
        close(r[zero_lag], energy, 1e-9);
    }

    #[test]
    fn xcorr_matches_convolution_with_the_reversed_kernel() {
        let x = [1.0, 2.0, 3.0];
        let y = [0.5, -1.0];
        let mut reversed_y = y.to_vec();
        reversed_y.reverse();
        let expected = conv(&x, &reversed_y, ConvMode::Full).unwrap();
        let got = xcorr(&x, &y).unwrap();
        close_vec(&got, &expected);
    }

    /// The direct `O(n*m)` definition, independent of `full_conv`/`conv_fft`
    /// — a reference to check the FFT path against, not the same code path
    /// with different plumbing.
    fn conv_direct_reference(x: &[f64], h: &[f64]) -> Vec<f64> {
        let mut full = vec![0.0; x.len() + h.len() - 1];
        for (i, &xi) in x.iter().enumerate() {
            for (j, &hj) in h.iter().enumerate() {
                full[i + j] += xi * hj;
            }
        }
        full
    }

    #[test]
    fn conv_fft_path_matches_the_direct_definition() {
        // Both inputs at/above `CONV_FFT_MIN_DIM` (512) so `conv` actually
        // dispatches to `conv_fft` here — this is the case the dual-path
        // threshold exists for (see `CONV_FFT_MIN_DIM`'s doc comment for the
        // benchmark that picked 512).
        let n = 600;
        let m = 550;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.017).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.031).cos() * 0.5).collect();
        let via_dispatch = conv(&x, &h, ConvMode::Full).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&via_dispatch, &reference);
    }

    #[test]
    fn conv_fft_path_matches_direct_for_a_non_power_of_two_length() {
        // 513 + 513 - 1 = 1025, not itself a power of two, exercising the
        // zero-padding-to-`next_pow2` logic rather than a length that
        // happens to already be one.
        let x: Vec<f64> = (0..513).map(|i| ((i as f64) * 0.013).sin()).collect();
        let h: Vec<f64> = (0..513).map(|i| ((i as f64) * 0.007).cos()).collect();
        let via_dispatch = conv(&x, &h, ConvMode::Full).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&via_dispatch, &reference);
    }

    #[test]
    fn conv_stays_on_the_direct_path_below_the_fft_threshold() {
        // A quick sanity check that the dispatch boundary itself doesn't
        // change small-input behavior — same hand-computed example as
        // `conv_full_matches_a_hand_computed_example`, well under
        // `CONV_FFT_MIN_DIM`.
        let result = conv(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5], ConvMode::Full).unwrap();
        close_vec(&result, &[0.0, 1.0, 2.5, 4.0, 1.5]);
    }

    #[test]
    fn overlap_add_matches_direct_reference_for_a_single_tap_kernel() {
        // m=1 is the degenerate edge case: `overlap_add` must still produce
        // exactly `x` scaled by the one kernel tap, not divide-by-zero or
        // misindex on a fft_len computed from `block_len + m - 1` with `m=1`.
        let x: Vec<f64> = (0..50).map(|i| (i as f64 * 0.09).sin()).collect();
        let h = [2.5];
        let got = overlap_add(&x, &h).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&got, &reference);
    }

    #[test]
    fn overlap_add_matches_direct_reference_when_the_signal_divides_evenly_into_blocks() {
        // m=10 -> block_len = next candidate is 8*10=80, uncapped since
        // n=240 > 80; 240/80 = 3 exact blocks, no partial tail.
        let n = 240;
        let m = 10;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.041).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.5).cos()).collect();
        let got = overlap_add(&x, &h).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&got, &reference);
    }

    #[test]
    fn overlap_add_matches_direct_reference_when_the_signal_does_not_divide_evenly() {
        // Same m=10 (block_len=80) but n=250: 250 = 3*80 + 10, so the last
        // block is a 10-sample partial chunk — exercises the `end.min(n)`
        // truncation and the shorter final `seg_len` add-in.
        let n = 250;
        let m = 10;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.041).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.5).cos()).collect();
        let got = overlap_add(&x, &h).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&got, &reference);
    }

    #[test]
    fn overlap_add_matches_direct_reference_when_the_kernel_forces_a_single_capped_block() {
        // m=60, n=100: the raw heuristic block length (8*60=480) is capped
        // at `n` itself, so this collapses to exactly one block spanning
        // the whole signal — the "kernel close to/exceeding block length"
        // edge case, still must match the direct definition.
        let n = 100;
        let m = 60;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.07).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.13).cos()).collect();
        let got = overlap_add(&x, &h).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&got, &reference);
    }

    #[test]
    fn overlap_add_matches_direct_reference_when_the_first_argument_is_the_shorter_one() {
        // `overlap_add(x, h)` must swap roles internally when `h` is the
        // longer input — calling it with a short `x` and a long `h`
        // (backwards from the usual "long signal, short kernel" framing)
        // must still return the correct, commutative result.
        let x: Vec<f64> = (0..20).map(|i| (i as f64 * 0.3).cos()).collect(); // short "kernel"
        let h: Vec<f64> = (0..300).map(|i| (i as f64 * 0.02).sin()).collect(); // long "signal"
        let got = overlap_add(&x, &h).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&got, &reference);
    }

    #[test]
    fn conv_dispatches_to_overlap_add_for_a_long_signal_and_a_medium_kernel() {
        // min(n,m)=100 clears `OVERLAP_ADD_MIN_KERNEL` (64) and stays under
        // `CONV_FFT_MIN_DIM` (512); max(n,m)=5000 clears
        // `OVERLAP_ADD_MIN_SIGNAL` (1000) — squarely the overlap-add tier.
        let n = 5000;
        let m = 100;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.017).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.031).cos() * 0.5).collect();
        let via_dispatch = conv(&x, &h, ConvMode::Full).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&via_dispatch, &reference);
    }

    #[test]
    fn conv_stays_off_overlap_add_when_the_kernel_is_below_its_floor() {
        // m=32 is below `OVERLAP_ADD_MIN_KERNEL` (64) even though the
        // signal is long — the benchmark found direct convolution wins at
        // this kernel length regardless of `n`, so dispatch must still land
        // on the direct path here, not overlap-add.
        let n = 5000;
        let m = 32;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.023).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.047).cos()).collect();
        let via_dispatch = conv(&x, &h, ConvMode::Full).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&via_dispatch, &reference);
    }

    #[test]
    fn conv_stays_off_overlap_add_when_the_total_size_is_below_its_floor() {
        // m=100 clears `OVERLAP_ADD_MIN_KERNEL`, but max(n,m)=500 is below
        // `OVERLAP_ADD_MIN_SIGNAL` (1000) — too small a total size for
        // overlap-add to be worth the extra machinery, so this must still
        // land on the direct path.
        let n = 500;
        let m = 100;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.023).sin()).collect();
        let h: Vec<f64> = (0..m).map(|i| (i as f64 * 0.047).cos()).collect();
        let via_dispatch = conv(&x, &h, ConvMode::Full).unwrap();
        let reference = conv_direct_reference(&x, &h);
        close_vec(&via_dispatch, &reference);
    }

    #[test]
    fn hilbert_of_a_pure_cosine_gives_a_sine_imaginary_part() {
        // cos(wt)'s Hilbert transform is sin(wt); away from the
        // boundary-effect edges the analytic signal's imaginary part
        // should track sin(wt) closely.
        let n = 256;
        let freq_bin = 8.0;
        let x: Vec<f64> = (0..n).map(|i| (TAU * freq_bin * i as f64 / n as f64).cos()).collect();
        let analytic = hilbert(&x).unwrap();
        for i in 40..n - 40 {
            let expected = (TAU * freq_bin * i as f64 / n as f64).sin();
            close(analytic[i].im, expected, 0.05);
        }
    }

    #[test]
    fn hilbert_envelope_of_an_amplitude_modulated_tone_recovers_the_envelope() {
        let n = 512;
        let fs = 512.0;
        let carrier = 40.0;
        let envelope_freq = 2.0;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let env = 1.0 + 0.5 * (TAU * envelope_freq * t).sin();
                env * (TAU * carrier * t).cos()
            })
            .collect();
        let analytic = hilbert(&x).unwrap();
        for i in 60..n - 60 {
            let t = i as f64 / fs;
            let expected_env = 1.0 + 0.5 * (TAU * envelope_freq * t).sin();
            close(analytic[i].magnitude(), expected_env, 0.1);
        }
    }

    #[test]
    fn hilbert_rejects_empty_input() {
        assert_eq!(hilbert(&[]), Err(NumericError::EmptyInput("hilbert")));
    }

    #[test]
    fn corr_of_a_signal_with_itself_is_one() {
        let x = [1.0, 4.0, 2.0, 8.0, 5.0];
        close(corr(&x, &x).unwrap(), 1.0, 1e-9);
    }

    #[test]
    fn corr_of_a_signal_with_its_negation_is_minus_one() {
        let x = [1.0, 4.0, 2.0, 8.0, 5.0];
        let neg: Vec<f64> = x.iter().map(|v| -v).collect();
        close(corr(&x, &neg).unwrap(), -1.0, 1e-9);
    }

    #[test]
    fn corr_of_unrelated_signals_matches_a_hand_computed_value() {
        // Pearson's r for x=[1,2,3,4,5], y=[2,1,4,3,5], worked out by hand:
        // mean_x=mean_y=3, dx=[-2,-1,0,1,2], dy=[-1,-2,1,0,2],
        // cov=sum(dx*dy)=8, var_x=var_y=10, r=8/sqrt(10*10)=0.8.
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 1.0, 4.0, 3.0, 5.0];
        close(corr(&x, &y).unwrap(), 0.8, 1e-9);
    }

    #[test]
    fn corr_rejects_mismatched_lengths() {
        assert_eq!(corr(&[1.0, 2.0], &[1.0]), Err(NumericError::ShapeMismatch { expected: 2, found: 1 }));
    }

    #[test]
    fn signal_energy_matches_sum_of_squares() {
        close(signal_energy(&[1.0, 2.0, 3.0]), 14.0, 1e-9);
    }

    #[test]
    fn tkeo_matches_a_hand_computed_example() {
        // psi[x(n)] = x(n)^2 - x(n-1)*x(n+1) at each interior sample.
        let x = [1.0, 2.0, 3.0, 4.0];
        let result = tkeo(&x).unwrap();
        // n=1: 2^2 - 1*3 = 1; n=2: 3^2 - 2*4 = 1
        close_vec(&result, &[1.0, 1.0]);
    }

    #[test]
    fn tkeo_of_a_constant_signal_is_zero() {
        let result = tkeo(&[3.0, 3.0, 3.0, 3.0, 3.0]).unwrap();
        for v in result {
            close(v, 0.0, 1e-9);
        }
    }

    #[test]
    fn tkeo_rejects_too_short_input() {
        assert_eq!(tkeo(&[1.0, 2.0]), Err(NumericError::EmptyInput("tkeo")));
    }

    #[test]
    fn short_time_energy_produces_the_expected_frame_count_and_a_hand_checked_value() {
        // win=4, hop=2 over 8 samples -> frames at 0,2,4 => 3 frames.
        // Frame 0 is a constant [1,1,1,1] Hann-windowed ([0, 0.5, 1, 0.5])
        // then squared and summed -- computed independently in Python
        // (numpy) as 1.5 before writing this assertion, not just "some
        // positive number".
        let x = [1.0; 8];
        let result = short_time_energy(&x, 4, 2).unwrap();
        assert_eq!(result.len(), 3);
        close(result[0], 1.5, 1e-9);
    }

    #[test]
    fn short_time_energy_rejects_input_shorter_than_the_window() {
        assert!(short_time_energy(&[1.0, 2.0], 4, 2).is_err());
    }

    #[test]
    fn spectral_entropy_is_low_for_a_pure_tone_and_high_for_white_noise() {
        // Verified against an independent Python/numpy implementation
        // first: a 50 Hz tone gives entropy around 0.18 (energy
        // concentrated in one bin), white noise around 0.92 (spread across
        // all bins) -- the real contrast this measure is supposed to
        // capture, not just "returns numbers in [0,1]".
        let fs = 1000.0;
        let n = 2000;
        let tone: Vec<f64> = (0..n).map(|i| (2.0 * std::f64::consts::PI * 50.0 * i as f64 / fs).sin()).collect();
        let tone_entropy = spectral_entropy(&tone, 256, 128).unwrap();
        let mean_tone: f64 = tone_entropy.iter().sum::<f64>() / tone_entropy.len() as f64;
        assert!(mean_tone < 0.3, "tone entropy {mean_tone} should be low");

        // Deterministic white noise via the same splitmix64-style
        // counter PRNG `basin_hopping`/`systematic_resample` already use
        // elsewhere in this crate (reproducible, no external RNG crate).
        let mut rng_state: u64 = 42u64 ^ 0x9E3779B97F4A7C15;
        let noise: Vec<f64> = (0..n)
            .map(|_| {
                rng_state = rng_state.wrapping_add(0x9E3779B97F4A7C15);
                let mut z = rng_state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
                z ^= z >> 31;
                (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64) * 2.0 - 1.0
            })
            .collect();
        let noise_entropy = spectral_entropy(&noise, 256, 128).unwrap();
        let mean_noise: f64 = noise_entropy.iter().sum::<f64>() / noise_entropy.len() as f64;
        assert!(mean_noise > 0.7, "noise entropy {mean_noise} should be high");
        assert!(mean_noise > mean_tone);
    }

    #[test]
    fn spectral_entropy_values_stay_within_zero_and_one() {
        let x: Vec<f64> = (0..1000).map(|i| (i as f64 * 0.1).sin() + 0.3 * (i as f64 * 0.37).cos()).collect();
        for v in spectral_entropy(&x, 128, 64).unwrap() {
            assert!((0.0..=1.0).contains(&v), "entropy {v} out of range");
        }
    }

    #[test]
    fn find_peaks_locates_two_well_separated_gaussians() {
        let gaussian = |x: f64, mu: f64, sigma: f64| (-0.5 * ((x - mu) / sigma).powi(2)).exp();
        let x: Vec<f64> = (0..200)
            .map(|i| {
                let t = i as f64;
                gaussian(t, 50.0, 5.0) + 0.8 * gaussian(t, 140.0, 5.0)
            })
            .collect();
        let peaks = find_peaks(&x, None, None, None);
        assert_eq!(peaks, vec![50, 140], "peaks = {peaks:?}");
    }

    #[test]
    fn find_peaks_min_height_filters_out_the_smaller_peak() {
        let x = [0.0, 1.0, 0.0, 3.0, 0.0, 1.0, 0.0];
        let peaks = find_peaks(&x, Some(2.0), None, None);
        assert_eq!(peaks, vec![3]);
    }

    #[test]
    fn find_peaks_min_distance_keeps_the_tallest_of_a_close_cluster() {
        // two close local maxima at indices 1 and 4 (heights 1.0 and 2.0),
        // 3 samples apart -- with a min_distance of 4 (stricter than their
        // actual separation), only the taller one should survive.
        let x = [0.0, 1.0, 0.0, 1.5, 2.0, 0.5, 0.0, 0.0, 0.0];
        assert_eq!(find_peaks(&x, None, None, None), vec![1, 4], "sanity check: both are peaks without a distance filter");
        let peaks = find_peaks(&x, None, Some(4), None);
        assert_eq!(peaks, vec![4]);
        // exactly at the required distance (3) both must still be kept --
        // "minimal distance" means clearing that bar counts as satisfied.
        assert_eq!(find_peaks(&x, None, Some(3), None), vec![1, 4]);
    }

    #[test]
    fn find_peaks_min_prominence_rejects_a_shoulder_on_a_bigger_peak() {
        // a small bump riding on the shoulder of a much taller peak has
        // low prominence (its "own" rise above the surrounding terrain is
        // small) even though its absolute height might clear a min_height
        // filter.
        let x = [0.0, 1.0, 2.0, 2.5, 2.3, 2.6, 2.0, 1.0, 0.0];
        let all_peaks = find_peaks(&x, None, None, None);
        assert!(all_peaks.contains(&3) && all_peaks.contains(&5));
        let prominent = find_peaks(&x, None, None, Some(0.5));
        assert!(!prominent.contains(&3), "the small shoulder bump must be filtered by prominence");
    }

    #[test]
    fn find_peaks_returns_nothing_for_a_flat_or_monotonic_signal() {
        assert!(find_peaks(&[1.0, 1.0, 1.0], None, None, None).is_empty());
        assert!(find_peaks(&[1.0, 2.0, 3.0, 4.0], None, None, None).is_empty());
        assert!(find_peaks(&[1.0, 2.0], None, None, None).is_empty());
    }

    #[test]
    fn find_peaks_reports_the_midpoint_of_a_flat_topped_plateau() {
        // A strict local-max test (`x[i] > x[i-1] && x[i] > x[i+1]`) misses
        // this entirely: index 2 fails `x[2] > x[3]` since they're equal.
        let x = [1.0, 3.0, 3.0, 3.0, 1.0];
        assert_eq!(find_peaks(&x, None, None, None), vec![2]);

        // Even-width plateau: midpoint rounds down (index 1 of the pair
        // at indices 1,2), matching argmedian's "lower index on a tie".
        let x = [1.0, 3.0, 3.0, 1.0];
        assert_eq!(find_peaks(&x, None, None, None), vec![1]);

        // An ascending run into a taller plateau is not itself a peak;
        // the real peak (the next, taller plateau) must still be found.
        let x = [1.0, 2.0, 2.0, 3.0, 2.0, 1.0];
        assert_eq!(find_peaks(&x, None, None, None), vec![3]);

        // A plateau touching the signal's right edge has no right
        // neighbor to be a peak against, so it's correctly not reported.
        let x = [1.0, 3.0, 3.0, 3.0];
        assert!(find_peaks(&x, None, None, None).is_empty());
    }

    /// The original `O(candidates x accepted)` distance-suppression loop,
    /// kept here (test-only) purely as a reference oracle to cross-check
    /// `select_by_peak_distance`'s `O(n log n)` linked-list rewrite
    /// against, across randomized cases the handful of hand-written tests
    /// above wouldn't otherwise exercise.
    fn naive_select_by_peak_distance(candidates: &[usize], x: &[f64], dist: usize) -> Vec<usize> {
        let mut sorted: Vec<usize> = candidates.to_vec();
        sorted.sort_by(|&a, &b| x[b].partial_cmp(&x[a]).unwrap_or(std::cmp::Ordering::Equal));
        let mut accepted: Vec<usize> = Vec::new();
        for c in sorted {
            if accepted.iter().all(|&a| a.abs_diff(c) >= dist) {
                accepted.push(c);
            }
        }
        accepted.sort_unstable();
        accepted
    }

    /// A tiny deterministic PRNG (splitmix64), same pattern as
    /// `threshold.rs`'s test module, so the randomized cross-checks below
    /// are reproducible without a `rand` dependency.
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        fn next_range(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    #[test]
    fn select_by_peak_distance_matches_the_naive_reference_on_random_cases() {
        let mut rng = SplitMix64(0xC0FFEE);
        for trial in 0..500 {
            // Random, strictly-increasing candidate positions (as
            // `find_peaks` always produces), random heights (with
            // deliberate low cardinality so exact ties are common), and a
            // random distance threshold -- including some clustered
            // layouts (small position range relative to distance) so
            // dense accepted-neighbor scans are exercised.
            let n = 1 + rng.next_range(40);
            let clustered = trial % 2 == 0;
            let span = if clustered { 1 + rng.next_range(6) } else { 1 + rng.next_range(200) };
            let mut positions: Vec<usize> = Vec::with_capacity(n);
            let mut pos = rng.next_range(5);
            for _ in 0..n {
                positions.push(pos);
                pos += 1 + rng.next_range(span.max(1));
            }
            // Low-cardinality heights so exact ties are frequent (tests
            // the stable "earlier index wins" tie-break specifically).
            let heights: Vec<f64> = positions.iter().map(|_| rng.next_range(4) as f64).collect();
            let mut x = vec![0.0; positions.last().map(|&p| p + 1).unwrap_or(1)];
            for (&p, &h) in positions.iter().zip(&heights) {
                x[p] = h;
            }
            let dist = 1 + rng.next_range(8);

            let expected = naive_select_by_peak_distance(&positions, &x, dist);
            let actual = select_by_peak_distance(&positions, &x, dist);
            assert_eq!(
                actual, expected,
                "trial {trial}: positions={positions:?} heights={heights:?} dist={dist}"
            );
        }
    }

    #[test]
    fn select_by_peak_distance_breaks_exact_ties_by_earlier_index() {
        // Three exactly-equal-height candidates, all mutually within
        // `dist` of each other: only the earliest-index one should
        // survive, matching the old stable-sort-by-descending-height
        // behavior (ties keep the original, ascending-index order).
        let positions = [10usize, 12, 14];
        let x_len = 15;
        let mut x = vec![0.0; x_len];
        for &p in &positions {
            x[p] = 5.0;
        }
        assert_eq!(select_by_peak_distance(&positions, &x, 10), vec![10]);

        // Two separate tied clusters far apart: each cluster keeps its
        // own earliest index independently.
        let positions2 = [0usize, 1, 100, 101];
        let mut x2 = vec![0.0; 102];
        for &p in &positions2 {
            x2[p] = 1.0;
        }
        assert_eq!(select_by_peak_distance(&positions2, &x2, 5), vec![0, 100]);
    }

    #[test]
    fn select_by_peak_distance_handles_dense_clusters_of_accepted_peaks() {
        // A long run of candidates spaced exactly at the distance
        // threshold, alternating heights so every OTHER one wins --
        // exercises many accept/remove cycles over one contiguous chain.
        let n = 200;
        let positions: Vec<usize> = (0..n).map(|i| i * 3).collect();
        let x: Vec<f64> = positions
            .iter()
            .enumerate()
            .map(|(i, _)| if i % 2 == 0 { 2.0 } else { 1.0 })
            .collect();
        let mut full = vec![0.0; positions.last().unwrap() + 1];
        for (&p, &h) in positions.iter().zip(&x) {
            full[p] = h;
        }
        let expected = naive_select_by_peak_distance(&positions, &full, 4);
        let actual = select_by_peak_distance(&positions, &full, 4);
        assert_eq!(actual, expected);
        // Every even-indexed (taller) candidate should survive since its
        // odd-indexed neighbors are within `dist` but shorter.
        assert_eq!(actual.len(), n / 2);
    }

    #[test]
    fn conv_matches_known_closed_form() {
        // conv([1,2,3],[0,1,0.5]) by hand:
        // full[0]=1*0=0, full[1]=1*1+2*0=1, full[2]=1*0.5+2*1+3*0=2.5,
        // full[3]=2*0.5+3*1=4, full[4]=3*0.5=1.5
        let out = conv(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.5], ConvMode::Full).unwrap();
        let expected = [0.0, 1.0, 2.5, 4.0, 1.5];
        assert_eq!(out.len(), expected.len());
        for (a, b) in out.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-9, "got {out:?}, expected {expected:?}");
        }
    }

    #[test]
    fn xcorr_of_a_signal_with_itself_peaks_at_zero_lag() {
        let x = [1.0, 2.0, -1.0, 0.5, 3.0];
        let r = xcorr(&x, &x).unwrap();
        // full length 2n-1, zero lag is the middle sample.
        assert_eq!(r.len(), 2 * x.len() - 1);
        let mid = x.len() - 1;
        let peak = (0..r.len()).max_by(|&a, &b| r[a].total_cmp(&r[b])).unwrap();
        assert_eq!(peak, mid, "autocorrelation should peak at zero lag");
    }

    #[test]
    fn hilbert_of_a_pure_cosine_has_near_constant_envelope_and_linear_phase() {
        let n = 512;
        let amplitude = 3.0;
        let freq = 10.0;
        let fs = 512.0;
        let x: Vec<f64> = (0..n)
            .map(|k| amplitude * (TAU * freq * k as f64 / fs).cos())
            .collect();
        let analytic = hilbert(&x).unwrap();
        // Skip the edges (Hilbert via FFT has boundary transients).
        let interior = &analytic[n / 8..n - n / 8];
        for z in interior {
            let mag = z.magnitude();
            assert!(
                (mag - amplitude).abs() / amplitude < 0.05,
                "expected envelope near {amplitude}, got {mag}"
            );
        }
        // Instantaneous phase should advance ~linearly at 2*pi*freq/fs
        // rad/sample; check the average step over an interior stretch.
        let phases: Vec<f64> = interior.iter().map(|z| z.arg()).collect();
        let mut unwrapped = vec![phases[0]];
        for i in 1..phases.len() {
            let mut d = phases[i] - phases[i - 1];
            while d > PI {
                d -= TAU;
            }
            while d < -PI {
                d += TAU;
            }
            unwrapped.push(unwrapped[i - 1] + d);
        }
        let total = unwrapped[unwrapped.len() - 1] - unwrapped[0];
        let expected_step = TAU * freq / fs;
        let avg_step = total / (unwrapped.len() - 1) as f64;
        assert!(
            (avg_step - expected_step).abs() < 0.01,
            "expected phase step {expected_step}, got {avg_step}"
        );
    }

    #[test]
    fn find_peaks_and_widths_match_a_known_triangular_peak() {
        // A symmetric triangular peak: 0,1,2,3,4,3,2,1,0 -- apex at index 4,
        // height 4, base 0 on both sides, prominence 4 (edges are the
        // bounding "higher" points, i.e. none exist, so prominence is the
        // full height above the lower of the two edges, both 0).
        let x = [0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0];
        let peaks = find_peaks(&x, None, None, None);
        assert_eq!(peaks, vec![4]);
        let prom = prominence(&x, 4);
        assert!((prom - 4.0).abs() < 1e-9, "expected prominence 4, got {prom}");
        // Half-prominence (rel_height=0.5) reference level is 4 - 0.5*4 = 2,
        // which the triangle's linear sides cross exactly at samples 2 and
        // 6 (x=2 there), so width == 4 samples exactly.
        let (width, left_ip, right_ip) = peak_width(&x, 4, 0.5);
        assert!((width - 4.0).abs() < 1e-9, "expected width 4, got {width}");
        assert!((left_ip - 2.0).abs() < 1e-9, "expected left crossing at 2, got {left_ip}");
        assert!((right_ip - 6.0).abs() < 1e-9, "expected right crossing at 6, got {right_ip}");
    }

    #[test]
    fn csd_of_a_signal_with_itself_matches_welch() {
        let fs = 1000.0;
        let freq = 77.0;
        let n = 4000;
        let x: Vec<f64> = (0..n).map(|k| (TAU * freq * k as f64 / fs).sin()).collect();
        let nperseg = 256;
        let window = hann_window(nperseg);
        let pxx = welch(&x, fs, nperseg, nperseg / 2, &window).unwrap();
        let sxx = csd(&x, &x, fs, nperseg, nperseg / 2, &window).unwrap();
        for (p, s) in pxx.iter().zip(sxx.iter()) {
            // Pxx == Sxx's magnitude (imaginary part ~0 for a signal with itself).
            assert!(s.im.abs() < 1e-6, "expected ~real Sxx, got im={}", s.im);
            assert!((p - s.re).abs() / p.max(1e-9) < 1e-6, "welch {p} vs csd.re {}", s.re);
        }
    }

    /// Deterministic broadband noise — every bin excited, no RNG crate and
    /// no cross-platform reproducibility question (a plain LCG, written out
    /// so the test means the same thing on every machine).
    fn lcg_noise(n: usize, seed: u64) -> Vec<f64> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
            })
            .collect()
    }

    /// The estimator's PHASE SIGN — which a magnitude check cannot reveal.
    ///
    /// [`csd`] forms `conj(X) * Y`, so `H1 = Pxy/Pxx` must map `x -> y`. A
    /// pure `D`-sample delay has the closed form `exp(-2i*pi*f*D/fs)`: unit
    /// magnitude at every bin and a phase ramp whose SIGN is exactly what a
    /// backwards conjugation would flip while leaving `|H| = 1` untouched.
    /// A magnitude-only assertion here would pass for both conventions,
    /// which is the whole reason this test is written against the phase.
    #[test]
    fn h1_recovers_a_pure_delays_phase_ramp_with_the_right_sign() {
        let fs = 1000.0;
        let n = 16384;
        let x = lcg_noise(n, 12345);
        let d = 3usize;
        let mut y = vec![0.0; n];
        for k in d..n {
            y[k] = x[k - d];
        }
        let nperseg = 256;
        let window = hann_window(nperseg);
        let (h, coh) =
            transfer_function(&x, &y, fs, nperseg, nperseg / 2, &window, TfEstimator::H1).unwrap();
        for k in 5..100 {
            let f = k as f64 * fs / nperseg as f64;
            let want = -TAU * f * d as f64 / fs;
            assert!(
                (h[k].magnitude() - 1.0).abs() < 0.02,
                "bin {k}: |H| = {}, expected ~1 for a pure delay",
                h[k].magnitude()
            );
            // Compared as unit phasors so the (-pi, pi] wrap never matters.
            let got = Complex64::from_polar(1.0, h[k].arg());
            let expected = Complex64::from_polar(1.0, want);
            assert!(
                got.sub(expected).magnitude() < 0.05,
                "bin {k}: phase {} but a {d}-sample delay must give {want}; \
                 a flipped conjugation would land on {}",
                h[k].arg(),
                -want
            );
            assert!(coh[k] > 0.95, "bin {k}: coherence {} on a noiseless delay", coh[k]);
        }
    }

    /// The documented bias ordering, and the exact magnitude identity that
    /// ties the three together: `|H1| = g2 * |H2|`, with `|Hv|` between.
    /// With the noise on the OUTPUT, `H1` is the estimator that is right,
    /// which is why it is the default.
    #[test]
    fn output_noise_biases_h1_low_h2_high_and_hv_between() {
        let fs = 1000.0;
        let n = 32768;
        let x = lcg_noise(n, 12345);
        let noise = lcg_noise(n, 777);
        let gain = 2.0;
        let y: Vec<f64> = x
            .iter()
            .zip(noise.iter())
            .map(|(a, e)| gain * a + 0.5 * e)
            .collect();
        let nperseg = 512;
        let window = hann_window(nperseg);
        let half = nperseg / 2;
        let (h1, coh) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::H1).unwrap();
        let (h2, _) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::H2).unwrap();
        let (hv, _) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::Hv).unwrap();
        let (mut err1, mut err2, mut bins) = (0.0, 0.0, 0.0);
        for k in 5..200 {
            let (m1, m2, mv) = (h1[k].magnitude(), h2[k].magnitude(), hv[k].magnitude());
            assert!(m1 <= m2 + 1e-9, "bin {k}: |H1| {m1} must not exceed |H2| {m2}");
            assert!(
                mv >= m1 - 1e-9 && mv <= m2 + 1e-9,
                "bin {k}: |Hv| {mv} must sit between {m1} and {m2}"
            );
            assert!(
                (m1 - coh[k] * m2).abs() < 1e-6 * m2.max(1.0),
                "bin {k}: the |H1| = g2*|H2| identity broke ({m1} vs {})",
                coh[k] * m2
            );
            err1 += (m1 - gain).abs();
            err2 += (m2 - gain).abs();
            bins += 1.0;
        }
        // The noise is on `y`, so H1 is the unbiased estimator here and H2
        // is inflated by 1/g2. That is a claim about the EXPECTATION, not
        // about any one bin: with ~127 averaged segments the per-bin random
        // error is comparable to the bias, and individual bins where H2
        // lands closer by luck are expected (bin 27 of this very record is
        // one). Asserting it per-bin would be asserting something the
        // estimator does not promise, so it is averaged across the band.
        assert!(
            err1 / bins < err2 / bins,
            "H1 should be the better estimator under output noise: \
             mean |error| {} vs H2's {}",
            err1 / bins,
            err2 / bins
        );
    }

    /// With no noise anywhere the coherence is 1, and at `g2 == 1` the
    /// three estimators are algebraically the same number — so a clean
    /// measurement cannot tell them apart, and any disagreement here would
    /// mean one of the three formulas is wrong.
    #[test]
    fn the_three_estimators_agree_when_the_measurement_is_noiseless() {
        let fs = 1000.0;
        let n = 16384;
        let x = lcg_noise(n, 4242);
        let y: Vec<f64> = x.iter().map(|a| -1.75 * a).collect();
        let nperseg = 256;
        let window = hann_window(nperseg);
        let half = nperseg / 2;
        let (h1, _) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::H1).unwrap();
        let (h2, _) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::H2).unwrap();
        let (hv, _) =
            transfer_function(&x, &y, fs, nperseg, half, &window, TfEstimator::Hv).unwrap();
        for k in 3..120 {
            assert!(
                h1[k].sub(h2[k]).magnitude() < 1e-6,
                "bin {k}: H1 {:?} and H2 {:?} disagree on a noiseless record",
                h1[k],
                h2[k]
            );
            assert!(h1[k].sub(hv[k]).magnitude() < 1e-6, "bin {k}: H1 and Hv disagree");
            // A pure sign inversion is a gain of 1.75 at a phase of pi.
            assert!((h1[k].magnitude() - 1.75).abs() < 1e-6, "bin {k}: gain");
            assert!(
                (h1[k].arg().abs() - std::f64::consts::PI).abs() < 1e-6,
                "bin {k}: an inverting gain must read as a phase of +-pi, got {}",
                h1[k].arg()
            );
        }
    }

    #[test]
    fn coherence_is_near_one_at_a_shared_tone_for_correlated_signals() {
        let fs = 1000.0;
        let freq = 60.0;
        let n = 8000;
        let x: Vec<f64> = (0..n).map(|k| (TAU * freq * k as f64 / fs).sin()).collect();
        // y is x scaled and phase-shifted -- a linear, time-invariant
        // function of x, so coherence at `freq` should be ~1.
        let y: Vec<f64> = (0..n)
            .map(|k| 2.5 * (TAU * freq * k as f64 / fs + 0.3).sin())
            .collect();
        let nperseg = 256;
        let window = hann_window(nperseg);
        let coh = coherence(&x, &y, fs, nperseg, nperseg / 2, &window).unwrap();
        let bin = (freq * nperseg as f64 / fs).round() as usize;
        assert!(
            coh[bin] > 0.95,
            "expected coherence near 1 at {freq} Hz, got {}",
            coh[bin]
        );
    }

    // --- pulse / edge measurements -----------------------------------

    /// A 0/1 pulse train: period 10 samples, high for the first 3 of
    /// every 10. Every expected number below is worked out by hand from
    /// this shape, not read back off the implementation.
    fn pulse_train() -> Vec<f64> {
        (0..50).map(|i| if i % 10 < 3 { 1.0 } else { 0.0 }).collect()
    }

    /// A step from 0 to 1 along a straight ramp of exactly 8 intervals
    /// (slope 1/8 per sample), with flat tails either side.
    fn ramp_step() -> Vec<f64> {
        let mut v = vec![0.0, 0.0, 0.0];
        for k in 0..=8 {
            v.push(k as f64 / 8.0);
        }
        v.extend_from_slice(&[1.0, 1.0, 1.0]);
        v
    }

    #[test]
    fn trigger_reports_the_first_sample_of_the_new_state() {
        let x = pulse_train();
        // Rising: x[9]=0, x[10]=1 -> index 10. The run that is already
        // high at sample 0 has no predecessor, so it is not an edge.
        assert_eq!(find_trigger(&x, 0.5, Edge::Rising), vec![10, 20, 30, 40]);
        // Falling: x[2]=1, x[3]=0 -> index 3.
        assert_eq!(find_trigger(&x, 0.5, Edge::Falling), vec![3, 13, 23, 33, 43]);
        let both = find_trigger(&x, 0.5, Edge::Both);
        assert_eq!(both, vec![3, 10, 13, 20, 23, 30, 33, 40, 43]);
    }

    #[test]
    fn rising_and_falling_crossings_strictly_alternate() {
        // The property `find_pulses` relies on, checked on a signal that
        // deliberately sits exactly ON the level (the case the symmetric
        // definition gets wrong).
        let x = [0.0, 0.5, 0.0, 0.5, 0.5, 0.0];
        let set = find_edges(&x, 0.5, 0.0);
        assert_eq!(set.indices, vec![1, 2, 3, 5]);
        assert_eq!(set.rising, vec![true, false, true, false]);
    }

    #[test]
    fn trigger_does_not_invent_edges_across_a_nan_dropout() {
        let x = [1.0, 1.0, f64::NAN, 1.0, 1.0];
        assert!(find_trigger(&x, 0.5, Edge::Both).is_empty());
    }

    #[test]
    fn crossings_interpolate_between_samples() {
        let x = pulse_train();
        // Between x[9]=0 and x[10]=1 the half-way level falls at 9.5.
        close(crossing_position(&x, 10, 0.5), 9.5, 1e-12);
        // Between x[2]=1 and x[3]=0, likewise at 2.5.
        close(crossing_position(&x, 3, 0.5), 2.5, 1e-12);
    }

    #[test]
    fn period_width_and_duty_come_back_exact_on_a_known_train() {
        let x = pulse_train();
        close(pulse_period(&x, 0.5, 0.0).unwrap(), 10.0, 1e-12);
        close(mean_pulse_width(&x, 0.5, 0.0, true).unwrap(), 3.0, 1e-12);
        close(duty_cycle(&x, 0.5, 0.0).unwrap(), 0.3, 1e-12);
        // Four complete pulses -- the high run at the very start of the
        // record is truncated and deliberately not reported.
        let pulses = find_pulses(&x, 0.5, 0.0, true);
        assert_eq!(pulses.len(), 4);
        assert_eq!(pulses[0].start, 10);
        assert_eq!(pulses[0].end, 13);
        close(pulses[0].width, 3.0, 1e-12);
    }

    #[test]
    fn rise_time_of_a_known_ramp_is_the_ramp_slope() {
        let x = ramp_step();
        let (base, top) = rise_fall_levels(&x).unwrap();
        close(base, 0.0, 1e-12);
        close(top, 1.0, 1e-12);
        // 10%->90% is 0.8 of a unit span climbed at 1/8 per sample: 6.4
        // samples, and the two interpolated crossings are 3.8 and 10.2.
        close(rise_time(&x, base, top, 0.1, 0.9).unwrap(), 6.4, 1e-12);
    }

    #[test]
    fn fall_time_mirrors_rise_time_on_the_reversed_ramp() {
        let mut x = ramp_step();
        x.reverse();
        let (base, top) = rise_fall_levels(&x).unwrap();
        close(fall_time(&x, base, top, 0.1, 0.9).unwrap(), 6.4, 1e-12);
    }

    #[test]
    fn hysteresis_rejects_a_glitch_without_moving_the_measurement() {
        // A clean edge at the same place, once with a noise spike that
        // pokes back across the threshold mid-transition.
        let clean = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let glitchy = [0.0, 0.0, 0.0, 1.0, 0.45, 1.0, 1.0, 1.0];
        assert_eq!(find_edges(&glitchy, 0.5, 0.0).indices.len(), 3); // up, down, up
        let hyst = find_edges(&glitchy, 0.5, 0.4);
        assert_eq!(hyst.indices.len(), 1); // the 0.45 dip never reaches lo=0.3
        assert!(hyst.rising[0]);
        // ...and the position is still the mid-threshold crossing, the
        // same one the clean signal reports.
        close(hyst.positions[0], find_edges(&clean, 0.5, 0.0).positions[0], 1e-12);
    }

    #[test]
    fn overshoot_and_undershoot_are_percentages_of_the_step() {
        // Settles at 1.0, peaks at 1.2, dips to -0.1 before rising.
        let x = [0.0, 0.0, -0.1, 0.0, 1.2, 1.05, 1.0, 1.0, 1.0, 1.0];
        let (initial, settled) = settled_levels(&x, 0.1).unwrap();
        close(initial, 0.0, 1e-12);
        close(settled, 1.0, 1e-12);
        let (over, under) = step_shoot(&x, initial, settled).unwrap();
        close(over, 20.0, 1e-12); // (1.2 - 1.0) / 1.0
        close(under, 10.0, 1e-12); // (0.0 - -0.1) / 1.0
    }

    #[test]
    fn a_falling_step_overshoots_downward() {
        let x = [1.0, 1.0, 1.0, -0.2, 0.0, 0.0, 0.0, 0.0];
        let (initial, settled) = settled_levels(&x, 0.125).unwrap();
        let (over, under) = step_shoot(&x, initial, settled).unwrap();
        close(over, 20.0, 1e-12); // travelled 0.2 past the settled 0.0
        close(under, 0.0, 1e-12); // never went above where it started
    }

    #[test]
    fn a_record_without_a_full_cycle_reports_nothing_rather_than_guessing() {
        let x = [0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(pulse_period(&x, 0.5, 0.0).is_none());
        assert!(mean_pulse_width(&x, 0.5, 0.0, true).is_none());
    }
}
