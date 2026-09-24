//! Authoritative numerical primitives for the Qu runtime.
//!
//! The crate deliberately starts small. Every primitive here must behave the
//! same in native builds and `wasm32`; higher layers may accelerate an operation
//! only when they preserve these reference semantics. Portable-by-default no
//! longer means dependency-free across the board — `rustfft` (pure Rust,
//! MIT/Apache-2.0, no OS-thread requirement) backs the FFT below because it
//! is *faster on every target this crate must build for*, `qu-wasm`
//! included, which a C library (FFTW) is not.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::cell::RefCell;

use realfft::RealFftPlanner;
use rustfft::{num_complex::Complex as RustComplex, FftPlanner};

pub mod animation;
pub mod circuit;
pub mod circuit_fit;
pub mod cmatrix;
/// Compressed sensing: recovering a sparse signal from far fewer
/// measurements than Nyquist demands. Greedy methods (OMP, CoSaMP,
/// subspace pursuit, IHT/NIHT), convex ones (ISTA/FISTA, basis pursuit by
/// ADMM), total variation, and the coherence diagnostics that say whether
/// a sensing matrix is any good. See the module doc for how to choose.
pub mod cs;
pub mod decompose;
/// Measurement diagnostics — clipping, converter saturation and structural
/// integrity. Cheap checks on whether a record is worth analysing at all,
/// as opposed to metrics computed from one that is.
pub mod diagnostics;
pub mod eis;
pub mod estimation;
pub mod filter;
pub mod geometry;
pub mod linalg;
pub mod lse;
/// SINAD/SNR/THD/SFDR/ENOB from a sampled record — the bridge from a
/// crest factor to the converter resolution it costs.
pub mod sinad;
/// Noise, interference and distortion: putting them in, and taking them
/// out. Noise is specified in decibels because an amplitude is not a
/// statement about a measurement and an SNR is. See the module's own doc
/// comment for what each kind models and why they are not interchangeable.
pub mod noise;
pub mod matrix;
pub mod optimize;
pub mod rf;
pub mod selection;
pub mod signal;
pub mod special;
pub mod threshold;
pub mod transforms;

/// Conservative guard used by interactive runners before allocating a range.
pub const DEFAULT_ELEMENT_LIMIT: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex64 {
    pub re: f64,
    pub im: f64,
}

impl Complex64 {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// A real number with zero imaginary part.
    pub const fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    /// A complex number from magnitude and phase (radians).
    pub fn from_polar(magnitude: f64, phase: f64) -> Self {
        Self {
            re: magnitude * phase.cos(),
            im: magnitude * phase.sin(),
        }
    }

    pub fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Phase angle in radians, in `(-pi, pi]` (`atan2(im, re)`).
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    /// `e^z`, by Euler: `e^(a+bi) = e^a (cos b + i sin b)`.
    ///
    /// The single most-used complex function there is -- a phasor, a
    /// design matrix of tones, a transfer function are all `exp` of an
    /// imaginary argument -- and it was the one Qu could not evaluate. A
    /// script that needed `exp(-2i*pi*f*t)` had to spell out
    /// `cos(...) - i*sin(...)` by hand.
    pub fn exp(self) -> Self {
        Self::from_polar(self.re.exp(), self.im)
    }

    /// Principal branch of `ln z`: `ln|z| + i*arg z`, with the imaginary
    /// part in `(-pi, pi]`.
    pub fn ln(self) -> Self {
        Self::new(self.magnitude().ln(), self.arg())
    }

    /// Principal square root, the branch with non-negative real part.
    pub fn sqrt(self) -> Self {
        if self.re == 0.0 && self.im == 0.0 {
            return Self::new(0.0, 0.0);
        }
        Self::from_polar(self.magnitude().sqrt(), self.arg() / 2.0)
    }

    /// `tanh z`, by the real-arithmetic identity
    ///
    /// ```text
    /// tanh(a + bi) = (sinh 2a + i sin 2b) / (cosh 2a + cos 2b)
    /// ```
    ///
    /// Written out rather than as `(e^z - e^-z)/(e^z + e^-z)` because the
    /// exponential form overflows to `inf/inf = NaN` well before `tanh`
    /// itself stops being representable.
    ///
    /// Past `|2a| = 40` even `cosh 2a` overflows, and there `tanh z` is
    /// `sign(a)` to far better than a double can express (`1 - tanh 20`
    /// is ~8e-18, below the last bit of 1.0), so that regime saturates
    /// exactly instead of returning NaN. It is not an edge case: the
    /// finite-length Warburg and de Levie porous-electrode impedances are
    /// `tanh`/`coth` of `sqrt(jwT)` swept across nine decades of
    /// frequency, so real fits reach it on every run.
    ///
    /// Qu had no complex `tanh` at all before this -- `sqrt`, `exp` and
    /// `^` all accepted complex, so nothing about the neighbours hinted
    /// that this one did not; `papers/ecm-pf` had to spell the identity
    /// out in Qu (`ctanh_scalar`), saturation branch and all.
    pub fn tanh(self) -> Self {
        let a = 2.0 * self.re;
        let b = 2.0 * self.im;
        if a.abs() > 40.0 {
            return Self::new(if a.is_sign_negative() { -1.0 } else { 1.0 }, 0.0);
        }
        let denom = a.cosh() + b.cos();
        Self::new(a.sinh() / denom, b.sin() / denom)
    }

    /// `sinh(a+bi) = sinh(a)cos(b) + i cosh(a)sin(b)`. Entire (no poles).
    pub fn sinh(self) -> Self {
        Self::new(
            self.re.sinh() * self.im.cos(),
            self.re.cosh() * self.im.sin(),
        )
    }

    /// `cosh(a+bi) = cosh(a)cos(b) + i sinh(a)sin(b)`. Entire (no poles).
    pub fn cosh(self) -> Self {
        Self::new(
            self.re.cosh() * self.im.cos(),
            self.re.sinh() * self.im.sin(),
        )
    }

    /// `sin(a+bi) = sin(a)cosh(b) + i cos(a)sinh(b)`. Entire (no poles).
    pub fn sin(self) -> Self {
        Self::new(
            self.re.sin() * self.im.cosh(),
            self.re.cos() * self.im.sinh(),
        )
    }

    /// `cos(a+bi) = cos(a)cosh(b) - i sin(a)sinh(b)`. Entire (no poles).
    pub fn cos(self) -> Self {
        Self::new(
            self.re.cos() * self.im.cosh(),
            -(self.re.sin() * self.im.sinh()),
        )
    }

    /// `tan(z) = sin(z)/cos(z)`. `cos(z) = 0` for complex `z` only on the
    /// real axis at `z = pi/2 + k*pi` (`cosh(b) >= 1` for every real `b`,
    /// so the imaginary part of `cos(a+bi)` forces `sin(a)sinh(b) = 0`;
    /// combined with the real part forcing `cos(a) = 0`, `sinh(b)` must
    /// also be `0`, i.e. `b = 0`) -- the same poles as real `tan`, nowhere
    /// else in the plane. `div` already yields non-finite components at
    /// those poles rather than erroring, matching the float semantics the
    /// rest of the runtime uses (see `div`'s own doc comment).
    pub fn tan(self) -> Self {
        self.sin().div(self.cos())
    }

    /// Principal value: `asin(z) = -i * ln(iz + sqrt(1 - z^2))`.
    /// Branch points at `z = +-1`; `sqrt`/`ln` above are already the
    /// principal branches, so this is the principal branch of `asin` too.
    pub fn asin(self) -> Self {
        let i = Self::new(0.0, 1.0);
        let inner = i.mul(self).add(Self::real(1.0).sub(self.mul(self)).sqrt());
        i.scale(-1.0).mul(inner.ln())
    }

    /// Principal value: `acos(z) = -i * ln(z + i*sqrt(1 - z^2))`.
    /// Branch points at `z = +-1`.
    pub fn acos(self) -> Self {
        let i = Self::new(0.0, 1.0);
        let inner = self.add(i.mul(Self::real(1.0).sub(self.mul(self)).sqrt()));
        i.scale(-1.0).mul(inner.ln())
    }

    /// Principal value: `atan(z) = (i/2) * ln((1 - iz) / (1 + iz))`.
    /// Branch points at `z = +-i`.
    pub fn atan(self) -> Self {
        let i = Self::new(0.0, 1.0);
        let iz = i.mul(self);
        let ratio = Self::real(1.0).sub(iz).div(Self::real(1.0).add(iz));
        i.scale(0.5).mul(ratio.ln())
    }

    pub fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }

    pub fn add(self, rhs: Self) -> Self {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }

    pub fn sub(self, rhs: Self) -> Self {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }

    pub fn mul(self, rhs: Self) -> Self {
        multiply(self, rhs)
    }

    pub fn scale(self, k: f64) -> Self {
        Self {
            re: self.re * k,
            im: self.im * k,
        }
    }

    /// Complex division; dividing by zero yields non-finite components, matching
    /// the float semantics the rest of the runtime uses.
    pub fn div(self, rhs: Self) -> Self {
        let denom = rhs.re * rhs.re + rhs.im * rhs.im;
        Self {
            re: (self.re * rhs.re + self.im * rhs.im) / denom,
            im: (self.im * rhs.re - self.re * rhs.im) / denom,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    bins: Vec<Complex64>,
}

impl Spectrum {
    pub fn bins(&self) -> &[Complex64] {
        &self.bins
    }

    pub fn magnitudes(&self) -> Vec<f64> {
        self.bins.iter().map(|bin| bin.magnitude()).collect()
    }

    /// Returns the largest bin from the one-sided spectrum, including DC.
    pub fn peak_bin(&self) -> Option<(usize, f64)> {
        self.bins
            .iter()
            .take(self.bins.len() / 2 + 1)
            .enumerate()
            .map(|(index, bin)| (index, bin.magnitude()))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NumericError {
    EmptyInput(&'static str),
    NonFiniteRange,
    ZeroRangeStep,
    RangeDirection,
    ElementLimit { requested: usize, limit: usize },
    FftLength { length: usize },
    /// A transform (`dwt_haar`) that requires an even-length input got an odd one.
    OddLength { length: usize },
    /// A reconstruction transform (`irfft`, `idwt_haar`) got mismatched shapes.
    ShapeMismatch { expected: usize, found: usize },
    /// A numerical decomposition (e.g. `filter::polynomial_roots`'s
    /// companion-matrix eigenvalue solve) failed, or was given input it
    /// can't decompose (a zero leading coefficient, a degenerate design).
    Decomposition(String),
}

impl Display for NumericError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput(operation) => {
                write!(formatter, "{operation} requires at least one value")
            }
            Self::NonFiniteRange => write!(formatter, "range bounds and step must be finite"),
            Self::ZeroRangeStep => write!(formatter, "range step cannot be zero"),
            Self::RangeDirection => write!(formatter, "range step points away from its stop value"),
            Self::ElementLimit { requested, limit } => {
                write!(
                    formatter,
                    "range would create {requested} values; limit is {limit}"
                )
            }
            Self::FftLength { length } => {
                write!(formatter, "fft requires at least 1 sample; received {length}")
            }
            Self::OddLength { length } => {
                write!(formatter, "this transform requires an even-length input; received {length}")
            }
            Self::ShapeMismatch { expected, found } => {
                write!(formatter, "expected {expected} elements, found {found}")
            }
            Self::Decomposition(msg) => write!(formatter, "decomposition failed: {msg}"),
        }
    }
}

impl Error for NumericError {}

/// How many values `start to stop step delta` produces.
///
/// Split out of [`inclusive_range`] so a caller that only wants to COUNT a
/// range -- a `for` loop, which visits the values one at a time and never
/// needs them all at once -- gets exactly the same answer as one that
/// materialises it. Two implementations of "how long is this range" would
/// eventually disagree, and the symptom would be a loop running one
/// iteration more or less than the equivalent vector has elements.
///
/// No element limit here: counting is free. The limit belongs to
/// materialising, which is what actually costs memory.
pub fn inclusive_range_len(start: f64, stop: f64, step: f64) -> Result<u128, NumericError> {
    if !start.is_finite() || !stop.is_finite() || !step.is_finite() {
        return Err(NumericError::NonFiniteRange);
    }
    if step == 0.0 {
        return Err(NumericError::ZeroRangeStep);
    }
    let distance = (stop - start) / step;
    if distance < -1.0e-12 {
        return Err(NumericError::RangeDirection);
    }
    // A fixed absolute epsilon is too tight once `distance` is large: a step
    // like `0.01` (inexact in binary) accumulates enough floating-point drift
    // over ~2000 divisions to exceed `1e-10` and silently drop the inclusive
    // stop sample. Scale the guard with `distance` itself instead.
    let eps = (distance.abs() + 1.0) * 1.0e-9;
    Ok((distance + eps).floor() as u128 + 1)
}

/// The `k`th value of `start to stop step delta`.
///
/// `start + k * step`, NOT `previous + step`: repeated addition accumulates
/// the same drift the epsilon above exists to tolerate, so a long loop
/// would wander away from the values the equivalent vector holds. This is
/// the identical expression [`inclusive_range`] uses per element.
#[inline]
pub fn inclusive_range_nth(start: f64, step: f64, k: u128) -> f64 {
    start + k as f64 * step
}

/// Qu's `start to stop step delta` range is inclusive at the stop boundary.
pub fn inclusive_range(
    start: f64,
    stop: f64,
    step: f64,
    limit: usize,
) -> Result<Vec<f64>, NumericError> {
    let length = inclusive_range_len(start, stop, step)?;
    if length > limit as u128 {
        return Err(NumericError::ElementLimit {
            // Saturating because the count is now `u128`: a range asking
            // for more values than `usize` can index should report the
            // limit it broke, not wrap around to a small number.
            requested: length.min(usize::MAX as u128) as usize,
            limit,
        });
    }
    Ok((0..length as usize)
        .map(|index| inclusive_range_nth(start, step, index as u128))
        .collect())
}

pub fn mean(values: &[f64]) -> Result<f64, NumericError> {
    if values.is_empty() {
        return Err(NumericError::EmptyInput("mean"));
    }
    Ok(values.iter().sum::<f64>() / values.len() as f64)
}

pub fn rms(values: &[f64]) -> Result<f64, NumericError> {
    if values.is_empty() {
        return Err(NumericError::EmptyInput("rms"));
    }
    Ok((values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt())
}

/// FFT of a real sequence, any length (see [`fft_complex`] for the dispatch
/// rule). Computes the **full** spectrum (`n` bins) — for the common
/// "just the non-negative-frequency half" case, prefer [`rfft_real`]
/// below, which does roughly half the work rather than computing this and
/// discarding half the output.
pub fn fft_real(input: &[f64]) -> Result<Spectrum, NumericError> {
    let samples: Vec<_> = input.iter().copied().map(Complex64::real).collect();
    let bins = fft_dispatch(&samples, false)?;
    Ok(Spectrum { bins })
}

thread_local! {
    // Same reasoning as `FFT_PLANNER` above (cache the planned algorithm
    // per length across calls, thread-local rather than a shared `Mutex`
    // since every `spawn`/`parallel for` worker is already its own OS
    // thread) — `RealFftPlanner` is `realfft`'s own equivalent cache.
    static REAL_FFT_PLANNER: RefCell<RealFftPlanner<f64>> = RefCell::new(RealFftPlanner::new());
}

/// Real-input FFT, non-negative-frequency half only (`n/2+1` bins) — via
/// `realfft`, `rustfft`'s real-FFT companion. Was: pack `input` into
/// `Complex64` (imaginary=0), run a full-length complex FFT, then slice
/// off half the output — real work spent computing values only to discard
/// them, not just a real-optimized *output shape*. `realfft` instead packs
/// two real values into one half-length complex FFT, exploiting the
/// Hermitian symmetry a real signal's spectrum always has, for roughly
/// half the work of the old path. Unnormalized (matches `fft_complex`'s
/// own forward convention) — [`irfft_real`] applies the inverse's `1/n`
/// scale explicitly, same as [`ifft_complex`] already does.
pub fn rfft_real(input: &[f64]) -> Result<Vec<Complex64>, NumericError> {
    let n = input.len();
    if n == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    if n == 1 {
        // realfft requires length >= 2; the one-sample "transform" is
        // itself, matching `fft_dispatch`'s own length-1 short circuit.
        return Ok(vec![Complex64::real(input[0])]);
    }
    REAL_FFT_PLANNER.with(|planner| {
        let mut planner = planner.borrow_mut();
        let r2c = planner.plan_fft_forward(n);
        let mut indata = r2c.make_input_vec();
        indata.copy_from_slice(input);
        let mut spectrum = r2c.make_output_vec();
        r2c.process(&mut indata, &mut spectrum)
            .expect("realfft: process() failed despite correctly-sized make_input_vec/make_output_vec buffers");
        Ok(spectrum.iter().map(|c| Complex64::new(c.re, c.im)).collect())
    })
}

/// Inverse of [`rfft_real`]: reconstructs a length-`n` real signal from its
/// non-negative-frequency half-spectrum (`n/2+1` bins) directly via
/// `realfft`'s C2R transform — no explicit Hermitian mirroring into a
/// full-length complex buffer first (the old `irfft`'s approach), `realfft`
/// does that reconstruction internally as part of the same real-optimized
/// half-length complex FFT `rfft_real`'s forward direction uses. `1/n`
/// normalized, matching [`ifft_complex`]'s own convention (`realfft`'s raw
/// output, like `rustfft`'s, is unnormalized).
pub fn irfft_real(half_spectrum: &[Complex64], n: usize) -> Result<Vec<f64>, NumericError> {
    if n == 0 {
        return Err(NumericError::FftLength { length: 0 });
    }
    let expected = n / 2 + 1;
    if half_spectrum.len() != expected {
        return Err(NumericError::ShapeMismatch { expected, found: half_spectrum.len() });
    }
    if n == 1 {
        return Ok(vec![half_spectrum[0].re]);
    }
    REAL_FFT_PLANNER.with(|planner| {
        let mut planner = planner.borrow_mut();
        let c2r = planner.plan_fft_inverse(n);
        let mut spectrum = c2r.make_input_vec();
        for (dst, src) in spectrum.iter_mut().zip(half_spectrum) {
            *dst = RustComplex::new(src.re, src.im);
        }
        let mut outdata = c2r.make_output_vec();
        c2r.process(&mut spectrum, &mut outdata)
            .expect("realfft: process() failed despite correctly-sized make_input_vec/make_output_vec buffers");
        let scale = 1.0 / n as f64;
        Ok(outdata.iter().map(|v| v * scale).collect())
    })
}

fn multiply(left: Complex64, right: Complex64) -> Complex64 {
    Complex64::new(
        left.re * right.re - left.im * right.im,
        left.re * right.im + left.im * right.re,
    )
}

/// Forward FFT of a complex sequence, **any length**. Backed by `rustfft`'s
/// planner (mixed-radix Cooley–Tukey for composite lengths, Bluestein for
/// prime/awkward ones) instead of this crate's own hand-written radix-2 +
/// Bluestein — same semantics (forward sign convention, no scaling), real
/// throughput win from SIMD dispatch where the target supports it. See
/// `FFT_PLANNER` below for why a planner is cached per thread rather than
/// built fresh on every call.
pub fn fft_complex(input: &[Complex64]) -> Result<Vec<Complex64>, NumericError> {
    fft_dispatch(input, false)
}

/// Inverse FFT of a complex spectrum, any length, `1/N` normalized (rustfft's
/// own inverse plan is unnormalized, matching the convention this crate
/// already used — the `1/N` scale is applied explicitly below).
pub fn ifft_complex(input: &[Complex64]) -> Result<Vec<Complex64>, NumericError> {
    fft_dispatch(input, true)
}

thread_local! {
    // rustfft's `FftPlanner` caches the algorithm it builds for each length
    // it's asked to plan, so reusing one planner across calls (instead of a
    // fresh `FftPlanner::new()` per call) means a script that repeatedly
    // transforms the same length — the common case, e.g. `stft`'s per-frame
    // FFTs — only pays the planning cost once. Thread-local rather than a
    // shared `Mutex`: each `spawn`/`parallel for` worker is its own OS
    // thread already isolated by design (§ interpreter performance,
    // BOARD.md), so a lock here would only add contention with no benefit.
    static FFT_PLANNER: RefCell<FftPlanner<f64>> = RefCell::new(FftPlanner::new());
}

fn fft_dispatch(input: &[Complex64], inverse: bool) -> Result<Vec<Complex64>, NumericError> {
    let length = input.len();
    if length == 0 {
        return Err(NumericError::FftLength { length });
    }
    if length == 1 {
        // the DFT of a single sample is itself, independent of direction.
        return Ok(input.to_vec());
    }
    let mut buffer: Vec<RustComplex<f64>> =
        input.iter().map(|c| RustComplex::new(c.re, c.im)).collect();
    FFT_PLANNER.with(|planner| {
        let mut planner = planner.borrow_mut();
        let fft = if inverse {
            planner.plan_fft_inverse(length)
        } else {
            planner.plan_fft_forward(length)
        };
        fft.process(&mut buffer);
    });
    if inverse {
        let scale = 1.0 / length as f64;
        for c in buffer.iter_mut() {
            *c *= scale;
        }
    }
    Ok(buffer.into_iter().map(|c| Complex64::new(c.re, c.im)).collect())
}

/// The nearest power of two at least as large as `n` (for zero-padding to an
/// FFT-friendly length). Returns at least 2.
pub fn next_pow2(n: usize) -> usize {
    let mut p = 2usize;
    while p < n {
        p <<= 1;
    }
    p
}

/// Machine-readable enough for runners, human-readable enough for `qu --version`.
pub fn capabilities() -> &'static str {
    "qu-core/0.4 portable-cpu multisine-recurrence fft-rustfft fft-complex ifft matrix-colmajor matmul pinv-svd least-squares broadcast ranges mean rms selection mesh3 animation3 hull-box hull-trimesh native+wasm"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 1.0e-9, "{left} != {right}");
    }

    #[test]
    fn inclusive_range_preserves_the_stop_sample() {
        let values = inclusive_range(0.0, 1.0, 0.25, DEFAULT_ELEMENT_LIMIT).unwrap();
        assert_eq!(values, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn ranges_reject_explosive_allocations() {
        assert_eq!(
            inclusive_range(0.0, 10.0, 1.0, 4),
            Err(NumericError::ElementLimit {
                requested: 11,
                limit: 4
            })
        );
    }

    #[test]
    fn fft_of_an_impulse_is_flat() {
        let spectrum = fft_real(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
        for bin in spectrum.bins() {
            close(bin.re, 1.0);
            close(bin.im, 0.0);
        }
    }

    #[test]
    fn fft_locates_a_bin_centered_sine() {
        let length = 1024;
        let expected_bin = 73;
        let signal: Vec<_> = (0..length)
            .map(|sample| (TAU * expected_bin as f64 * sample as f64 / length as f64).sin())
            .collect();
        let (actual_bin, magnitude) = fft_real(&signal).unwrap().peak_bin().unwrap();
        assert_eq!(actual_bin, expected_bin);
        close(magnitude, length as f64 / 2.0);
    }

    #[test]
    fn basic_statistics_match_reference_values() {
        close(mean(&[1.0, 2.0, 3.0]).unwrap(), 2.0);
        close(rms(&[3.0, 4.0]).unwrap(), (12.5_f64).sqrt());
    }

    #[test]
    fn fft_only_rejects_an_empty_input() {
        assert_eq!(fft_real(&[]), Err(NumericError::FftLength { length: 0 }));
        // a non-power-of-two length now succeeds via Bluestein (below).
        assert!(fft_real(&[0.0; 12]).is_ok());
    }

    /// A slow, unmistakably-correct DFT used only to cross-check `rustfft`'s
    /// non-power-of-two path against an independent reference.
    fn naive_dft(input: &[Complex64], inverse: bool) -> Vec<Complex64> {
        let n = input.len();
        let sign = if inverse { 1.0 } else { -1.0 };
        (0..n)
            .map(|k| {
                let mut acc = Complex64::new(0.0, 0.0);
                for (j, &x) in input.iter().enumerate() {
                    let angle = sign * TAU * (k * j) as f64 / n as f64;
                    acc = acc.add(x.mul(Complex64::new(angle.cos(), angle.sin())));
                }
                if inverse {
                    acc.scale(1.0 / n as f64)
                } else {
                    acc
                }
            })
            .collect()
    }

    fn close_c(a: Complex64, b: Complex64, tol: f64) {
        assert!((a.re - b.re).abs() < tol && (a.im - b.im).abs() < tol, "{a:?} != {b:?}");
    }

    #[test]
    fn fft_matches_naive_dft_for_non_power_of_two_lengths() {
        for &n in &[3usize, 5, 6, 7, 9, 12, 17, 50, 100] {
            let signal: Vec<Complex64> = (0..n)
                .map(|k| Complex64::new((k as f64 * 0.7).sin(), (k as f64 * 0.3).cos()))
                .collect();
            let expected = naive_dft(&signal, false);
            let actual = fft_complex(&signal).unwrap();
            for (a, b) in actual.iter().zip(expected.iter()) {
                close_c(*a, *b, 1e-9);
            }
        }
    }

    #[test]
    fn fft_round_trips_for_non_power_of_two_lengths() {
        for &n in &[3usize, 7, 12, 50, 100] {
            let signal: Vec<Complex64> = (0..n).map(|k| Complex64::real((k as f64).sin())).collect();
            let spectrum = fft_complex(&signal).unwrap();
            let recovered = ifft_complex(&spectrum).unwrap();
            for (a, b) in signal.iter().zip(recovered.iter()) {
                close_c(*a, *b, 1e-9);
            }
        }
    }

    #[test]
    fn fft_of_length_one_is_the_identity() {
        let x = [Complex64::new(3.0, -2.0)];
        assert_eq!(fft_complex(&x).unwrap(), vec![x[0]]);
        assert_eq!(ifft_complex(&x).unwrap(), vec![x[0]]);
    }

    #[test]
    fn fft_of_n50_matches_a_known_tone() {
        // the exact case that used to error: N=50 is not a power of two.
        let n = 50;
        let signal: Vec<f64> = (0..n).map(|k| (TAU * 3.0 * k as f64 / n as f64).cos()).collect();
        let spectrum = fft_real(&signal).unwrap();
        let mags = spectrum.magnitudes();
        // a pure cosine at bin 3 puts equal energy at bins 3 and n-3=47.
        assert!(mags[3] > 20.0, "expected a strong peak at bin 3, got {}", mags[3]);
        assert!(mags[47] > 20.0, "expected a strong peak at bin 47, got {}", mags[47]);
        let peak_elsewhere = mags
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != 3 && i != 47)
            .map(|(_, &m)| m)
            .fold(0.0, f64::max);
        assert!(peak_elsewhere < 1e-6, "leakage into other bins: {peak_elsewhere}");
    }

    #[test]
    fn ifft_inverts_fft() {
        let signal: Vec<_> = (0..16)
            .map(|s| Complex64::new((s as f64 * 0.3).sin(), (s as f64 * 0.1).cos()))
            .collect();
        let round = ifft_complex(&fft_complex(&signal).unwrap()).unwrap();
        for (a, b) in signal.iter().zip(round.iter()) {
            close(a.re, b.re);
            close(a.im, b.im);
        }
    }

    #[test]
    fn complex_fft_matches_real_fft() {
        let real = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let via_real = fft_real(&real).unwrap();
        let via_complex =
            fft_complex(&real.iter().copied().map(Complex64::real).collect::<Vec<_>>()).unwrap();
        for (a, b) in via_real.bins().iter().zip(via_complex.iter()) {
            close(a.re, b.re);
            close(a.im, b.im);
        }
    }

    #[test]
    fn complex_arithmetic_reference() {
        let a = Complex64::new(1.0, 2.0);
        let b = Complex64::new(3.0, -1.0);
        close(a.mul(b).re, 5.0); // (1+2i)(3-i) = 5 + 5i
        close(a.mul(b).im, 5.0);
        let q = a.div(b);
        // (1+2i)/(3-i) = (1+7i)/10
        close(q.re, 0.1);
        close(q.im, 0.7);
        close(Complex64::from_polar(2.0, 0.0).re, 2.0);
        close(a.conj().im, -2.0);
    }

    /// `tanh` checked against a path that shares none of its arithmetic:
    /// `(e^2z - 1)/(e^2z + 1)`, built from `exp`/`sub`/`add`/`div`, which
    /// the implementation does not call. Agreeing to 1e-12 on a spread of
    /// points in all four quadrants is then evidence, not a tautology.
    #[test]
    fn complex_tanh_matches_the_exponential_definition() {
        let one = Complex64::real(1.0);
        for &(a, b) in &[
            (0.5, 0.5),
            (-0.5, 0.5),
            (0.5, -1.7),
            (-2.0, -3.0),
            (0.0, 0.75),
            (3.0, 0.0),
            (1e-8, 1e-8),
        ] {
            let z = Complex64::new(a, b);
            let e2z = z.scale(2.0).exp();
            let reference = e2z.sub(one).div(e2z.add(one));
            close_c(z.tanh(), reference, 1.0e-12);
        }
    }

    /// Three anchors the formula could not fake: the real axis must match
    /// `f64::tanh`, the imaginary axis must give `i tan b` (so `tanh(i pi/4)`
    /// is exactly `i`), and `tanh` must stay odd.
    #[test]
    fn complex_tanh_agrees_with_the_real_and_imaginary_axes() {
        for &x in &[-3.0, -0.3, 0.0, 0.3, 3.0, 12.0] {
            close_c(Complex64::real(x).tanh(), Complex64::real(x.tanh()), 1.0e-14);
        }
        close_c(
            Complex64::new(0.0, std::f64::consts::FRAC_PI_4).tanh(),
            Complex64::new(0.0, 1.0),
            1.0e-14,
        );
        let z = Complex64::new(0.8, -2.3);
        let minus = z.scale(-1.0).tanh();
        close_c(minus, z.tanh().scale(-1.0), 1.0e-14);
    }

    /// The saturation branch at `|2a| = 40`, which a happy-path test misses
    /// entirely -- and which the finite-length Warburg reaches on every
    /// nine-decade sweep.
    ///
    /// Two things are asserted, and the second is the point: that past the
    /// cut `tanh` is `+-1` exactly, AND that the unsaturated arithmetic
    /// really would have produced NaN there. Without the second assertion
    /// the first would still pass if the cut were removed.
    #[test]
    fn complex_tanh_saturates_where_cosh_stops_being_representable() {
        // Just inside the cut: the direct formula still works, and the
        // value is already indistinguishable from +-1.
        let inside = Complex64::new(19.9, 1.3).tanh();
        assert!(inside.re.is_finite() && (inside.re - 1.0).abs() < 1.0e-15, "{inside:?}");
        // Just outside it, in both directions.
        close_c(Complex64::new(20.1, 1.3).tanh(), Complex64::real(1.0), 1.0e-300);
        close_c(Complex64::new(-20.1, 1.3).tanh(), Complex64::real(-1.0), 1.0e-300);
        // Far outside, where every naive route overflows.
        close_c(Complex64::new(400.0, 0.25).tanh(), Complex64::real(1.0), 1.0e-300);
        close_c(Complex64::new(-400.0, 0.25).tanh(), Complex64::real(-1.0), 1.0e-300);
        // ... and this is what the cut is protecting against: without it,
        // both the cosh/sinh identity and the exponential form are NaN.
        let a = 2.0 * 400.0f64;
        assert!((a.sinh() / (a.cosh() + 0.5f64.cos())).is_nan());
        let e2z = Complex64::new(400.0, 0.25).scale(2.0).exp();
        let naive = e2z.sub(Complex64::real(1.0)).div(e2z.add(Complex64::real(1.0)));
        assert!(naive.re.is_nan(), "exponential form unexpectedly survived: {naive:?}");
    }

    /// `sinh`/`cosh` are entire; check the identity against exponential
    /// form and the real/imaginary axes.
    #[test]
    fn complex_sinh_cosh_match_the_exponential_definition() {
        for &(a, b) in &[(0.5, 0.5), (-0.5, 0.5), (0.5, -1.7), (-2.0, -3.0), (0.0, 0.75), (3.0, 0.0)] {
            let z = Complex64::new(a, b);
            let ez = z.exp();
            let e_neg_z = z.scale(-1.0).exp();
            close_c(z.sinh(), ez.sub(e_neg_z).scale(0.5), 1.0e-12);
            close_c(z.cosh(), ez.add(e_neg_z).scale(0.5), 1.0e-12);
        }
        for &x in &[-3.0, -0.3, 0.0, 0.3, 3.0] {
            close_c(Complex64::real(x).sinh(), Complex64::real(x.sinh()), 1.0e-14);
            close_c(Complex64::real(x).cosh(), Complex64::real(x.cosh()), 1.0e-14);
        }
        // sinh(i*pi/2) = i*sin(pi/2) = i
        close_c(
            Complex64::new(0.0, std::f64::consts::FRAC_PI_2).sinh(),
            Complex64::new(0.0, 1.0),
            1.0e-14,
        );
    }

    /// `sin`/`cos` checked against the exponential definitions
    /// (`sin z = (e^iz - e^-iz)/2i`, `cos z = (e^iz + e^-iz)/2`), which
    /// share none of the arithmetic the identity-based implementation
    /// uses.
    #[test]
    fn complex_sin_cos_match_the_exponential_definition() {
        let i = Complex64::new(0.0, 1.0);
        for &(a, b) in &[(0.5, 0.5), (-0.5, 0.5), (0.5, -1.7), (-2.0, -3.0), (0.0, 0.75), (1.3, 0.0)] {
            let z = Complex64::new(a, b);
            let e_iz = i.mul(z).exp();
            let e_neg_iz = i.mul(z).scale(-1.0).exp();
            close_c(z.sin(), e_iz.sub(e_neg_iz).div(i.scale(2.0)), 1.0e-12);
            close_c(z.cos(), e_iz.add(e_neg_iz).scale(0.5), 1.0e-12);
        }
        for &x in &[-3.0, -0.3, 0.0, 0.3, 3.0] {
            close_c(Complex64::real(x).sin(), Complex64::real(x.sin()), 1.0e-14);
            close_c(Complex64::real(x).cos(), Complex64::real(x.cos()), 1.0e-14);
        }
    }

    /// `tan`'s only poles in the complex plane are the real-axis ones real
    /// `tan` already has (proof in `Complex64::tan`'s doc comment). This is
    /// the direct test of that claim: a point right next to the real pole
    /// at `pi/2`, but with a nonzero imaginary part, must evaluate to a
    /// finite, ordinary value -- not blow up, not error.
    #[test]
    fn complex_tan_is_finite_off_the_real_axis_near_a_pole() {
        let near_pole = Complex64::new(std::f64::consts::FRAC_PI_2 + 0.1, 0.0);
        let z = Complex64::new(near_pole.re, 0.3);
        let t = z.tan();
        assert!(t.re.is_finite() && t.im.is_finite(), "{t:?} should be finite off the real axis");
        // and matches sin/cos directly, since that's the definition.
        close_c(t, z.sin().div(z.cos()), 1.0e-12);
        // on the real axis, at the exact same real part, it is the huge
        // (but still floating-point-finite, since PI/2 isn't exactly
        // representable) value real `tan` gives -- same location as the
        // real function's pole, not a different one.
        let on_axis = Complex64::real(near_pole.re).tan();
        close(on_axis.re, near_pole.re.tan());
        close(on_axis.im, 0.0);
    }

    /// `asin`/`acos`/`acos` (sic acos) checked at values with known closed
    /// forms, including the discriminating case: a REAL value outside
    /// `[-1,1]`, wrapped as an explicit complex argument. `asin(x)` for
    /// real `x > 1` is `pi/2 - i*acosh(x)` -- purely via the complex
    /// branch, since the real `f64::asin` would give NaN there. A broken
    /// implementation could easily get this one wrong while still passing
    /// the interior-of-the-disc cases.
    #[test]
    fn complex_asin_acos_atan_match_known_values_and_the_out_of_domain_case() {
        // asin(1) = pi/2, acos(1) = 0, atan(1) = pi/4 -- exactly on the
        // real axis, sanity against f64.
        close_c(Complex64::real(0.5).asin(), Complex64::real(0.5f64.asin()), 1.0e-12);
        close_c(Complex64::real(0.5).acos(), Complex64::real(0.5f64.acos()), 1.0e-12);
        close_c(Complex64::real(0.5).atan(), Complex64::real(0.5f64.atan()), 1.0e-12);

        // asin(i) = i*asinh(1) = i*ln(1+sqrt(2))
        close_c(
            Complex64::new(0.0, 1.0).asin(),
            Complex64::new(0.0, (1.0 + 2f64.sqrt()).ln()),
            1.0e-12,
        );

        // The discriminating case: asin(2+0i), a real value out of
        // [-1,1]'s range but passed as a genuine complex argument, must
        // equal pi/2 - i*acosh(2) via the complex branch, mathematically
        // total there (the branch point is at z=1, not at z=2).
        let acosh_2 = (2.0 + (2.0f64 * 2.0 - 1.0).sqrt()).ln(); // acosh(x) = ln(x + sqrt(x^2-1))
        let expected = Complex64::new(std::f64::consts::FRAC_PI_2, -acosh_2);
        close_c(Complex64::new(2.0, 0.0).asin(), expected, 1.0e-9);

        // acos(z) = pi/2 - asin(z) identity, cross-checked independently.
        let z = Complex64::new(0.6, -0.4);
        close_c(
            z.acos(),
            Complex64::real(std::f64::consts::FRAC_PI_2).sub(z.asin()),
            1.0e-12,
        );

        // atan is odd: atan(-z) = -atan(z).
        let w = Complex64::new(1.1, 0.7);
        close_c(w.scale(-1.0).atan(), w.atan().scale(-1.0), 1.0e-12);

        // atan near its branch point at z=i stays finite just off it.
        let near_branch = Complex64::new(0.05, 1.0).atan();
        assert!(near_branch.re.is_finite() && near_branch.im.is_finite(), "{near_branch:?}");
    }

    /// `real(z)` unaffected: calling with a real `Value::Num` still uses
    /// the real function, not the complex branch promoted back down --
    /// out-of-domain reals stay NaN rather than silently becoming complex
    /// (same convention as `sqrt(-1)`). This is a property of the
    /// `map1_complex` dispatch in qu-interp, not of `Complex64` itself, so
    /// it is asserted here only at the `Complex64` level: the real
    /// closed-form functions (`f64::asin` etc.) are untouched by this
    /// change.
    #[test]
    fn real_asin_out_of_domain_is_still_nan_not_complex() {
        assert!(2.0f64.asin().is_nan());
    }

    #[test]
    fn next_pow2_rounds_up() {
        assert_eq!(next_pow2(1), 2);
        assert_eq!(next_pow2(3), 4);
        assert_eq!(next_pow2(1000), 1024);
        assert_eq!(next_pow2(1024), 1024);
    }
}
