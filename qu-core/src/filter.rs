//! Digital IIR filter design (Butterworth) and application.
//!
//! Pipeline (the standard textbook/production approach, matching MATLAB's
//! `butter`/SciPy's `scipy.signal.butter`): design an analog Butterworth
//! lowpass prototype, apply the requested frequency transformation
//! (lowpass/highpass/bandpass/bandstop) in the analog `s`-domain, map to
//! the digital `z`-domain via the bilinear transform (with frequency
//! pre-warping so the requested cutoff lands exactly where asked), then
//! group the resulting poles/zeros into second-order sections (SOS) —
//! cascaded biquads are the numerically stable, production-standard way
//! to realize a filter of any order; a single high-order transfer
//! function's coefficients become numerically unstable past roughly
//! order 4. Every step here was verified against `scipy.signal.butter`'s
//! own numeric output before being ported to Rust.

use crate::linalg;
use crate::matrix::Matrix;
use crate::transforms::{conv, ConvMode};
use crate::NumericError;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
    fn scale(self, k: f64) -> Self {
        Self::new(self.re * k, self.im * k)
    }
    fn neg(self) -> Self {
        Self::new(-self.re, -self.im)
    }
    fn sqrt(self) -> Self {
        let r = (self.re * self.re + self.im * self.im).sqrt();
        let re = ((r + self.re) / 2.0).max(0.0).sqrt();
        let im_mag = ((r - self.re) / 2.0).max(0.0).sqrt();
        Self::new(re, if self.im < 0.0 { -im_mag } else { im_mag })
    }
    /// Principal complex natural log: `ln|z| + i*arg(z)` -- only needed by
    /// [`Complex::asin`] below, which [`arc_jac_sn`]'s Landen recursion uses.
    fn ln(self) -> Self {
        let r = (self.re * self.re + self.im * self.im).sqrt();
        Self::new(r.ln(), self.im.atan2(self.re))
    }
    /// Principal complex arcsine via `asin(z) = -i * ln(i*z + sqrt(1-z^2))`
    /// -- the standard identity, built entirely from [`Complex::sqrt`]/
    /// [`Complex::ln`] above so it shares their same principal-branch
    /// convention. [`arc_jac_sn`] only ever evaluates this at a purely
    /// imaginary `z` (the Landen-transformed argument stays on the
    /// imaginary axis throughout its recursion, verified numerically
    /// against `scipy`'s own `_arc_jac_sn` before trusting this), where
    /// there is no branch-cut ambiguity to worry about.
    fn asin(self) -> Self {
        let i = Self::new(0.0, 1.0);
        let inner = real(1.0).sub(self.mul(self)).sqrt().add(i.mul(self));
        i.neg().mul(inner.ln())
    }
}

fn real(x: f64) -> Complex {
    Complex::new(x, 0.0)
}

fn prod(xs: &[Complex]) -> Complex {
    xs.iter().fold(real(1.0), |acc, &x| acc.mul(x))
}

/// The `n` poles of a normalized (cutoff at 1 rad/s, unity DC gain)
/// analog Butterworth lowpass prototype.
fn analog_prototype_poles(n: usize) -> Vec<Complex> {
    (1..=n)
        .map(|k| {
            let theta = PI * (2.0 * k as f64 + n as f64 - 1.0) / (2.0 * n as f64);
            Complex::new(theta.cos(), theta.sin())
        })
        .collect()
}

struct Zpk {
    zeros: Vec<Complex>,
    poles: Vec<Complex>,
    gain: f64,
}

fn lp2lp(z: Zpk, wc: f64) -> Zpk {
    let degree = z.poles.len() as i32 - z.zeros.len() as i32;
    Zpk {
        zeros: z.zeros.iter().map(|&v| v.scale(wc)).collect(),
        poles: z.poles.iter().map(|&v| v.scale(wc)).collect(),
        gain: z.gain * wc.powi(degree),
    }
}

fn lp2hp(z: Zpk, wc: f64) -> Zpk {
    let degree = z.poles.len() - z.zeros.len();
    let new_zeros: Vec<Complex> = z.zeros.iter().map(|&v| real(wc).div(v)).collect();
    let new_poles: Vec<Complex> = z.poles.iter().map(|&v| real(wc).div(v)).collect();
    // `k_hp = k * real(prod(-z) / prod(-p))` -- the standard formula
    // (matching `scipy.signal.lp2hp_zpk`) that cancels the gain change the
    // `wc/s` inversion introduces. `prod` of an empty zero list is `1`
    // (the fold's own identity element), so this needs no separate empty-
    // zeros case. The previous version of this formula (`(Zc/Pc).recip() *
    // Pc.re / Zc.re`, i.e. effectively `(Pc/Zc)^2` when zeros were
    // present) was wrong whenever `prod(-p)` wasn't already real-valued-
    // and-close-to-unit-magnitude -- invisible for Butterworth specifically
    // (its normalized prototype's poles sit exactly on the unit circle, so
    // `prod(-p) == 1` and both formulas coincide), but produces a wildly
    // wrong passband gain for a prototype like elliptic's whose pole
    // product isn't unit magnitude (caught by `ellip`'s own highpass test).
    let neg_zeros_prod = prod(&z.zeros.iter().map(|&v| v.neg()).collect::<Vec<_>>());
    let neg_poles_prod = prod(&z.poles.iter().map(|&v| v.neg()).collect::<Vec<_>>());
    let gain = z.gain * neg_zeros_prod.div(neg_poles_prod).re;
    let mut zeros = new_zeros;
    zeros.extend(std::iter::repeat_n(real(0.0), degree));
    Zpk { zeros, poles: new_poles, gain }
}

fn lp2bp(z: Zpk, wo: f64, bw: f64) -> Zpk {
    let degree = z.poles.len() - z.zeros.len();
    let scaled_poles: Vec<Complex> = z.poles.iter().map(|&v| v.scale(bw / 2.0)).collect();
    let scaled_zeros: Vec<Complex> = z.zeros.iter().map(|&v| v.scale(bw / 2.0)).collect();
    let wo2 = real(-wo * wo);
    let mut poles = Vec::with_capacity(scaled_poles.len() * 2);
    for p in &scaled_poles {
        let disc = p.mul(*p).add(wo2).sqrt();
        poles.push(p.add(disc));
        poles.push(p.sub(disc));
    }
    let mut zeros = Vec::with_capacity(scaled_zeros.len() * 2 + degree);
    for zr in &scaled_zeros {
        let disc = zr.mul(*zr).add(wo2).sqrt();
        zeros.push(zr.add(disc));
        zeros.push(zr.sub(disc));
    }
    zeros.extend(std::iter::repeat_n(real(0.0), degree));
    Zpk { zeros, poles, gain: z.gain * bw.powi(degree as i32) }
}

fn lp2bs(z: Zpk, wo: f64, bw: f64) -> Zpk {
    let degree = z.poles.len() - z.zeros.len();
    let half_bw = real(bw / 2.0);
    let hp_poles: Vec<Complex> = z.poles.iter().map(|&v| half_bw.div(v)).collect();
    let hp_zeros: Vec<Complex> = z.zeros.iter().map(|&v| half_bw.div(v)).collect();
    let wo2 = real(-wo * wo);
    let mut poles = Vec::with_capacity(hp_poles.len() * 2);
    for p in &hp_poles {
        let disc = p.mul(*p).add(wo2).sqrt();
        poles.push(p.add(disc));
        poles.push(p.sub(disc));
    }
    let mut zeros = Vec::with_capacity(hp_zeros.len() * 2 + 2 * degree);
    for zr in &hp_zeros {
        let disc = zr.mul(*zr).add(wo2).sqrt();
        zeros.push(zr.add(disc));
        zeros.push(zr.sub(disc));
    }
    for _ in 0..degree {
        zeros.push(Complex::new(0.0, wo));
        zeros.push(Complex::new(0.0, -wo));
    }
    // `k_bs = k * real(prod(-z) / prod(-p))` (matching `scipy.signal.
    // lp2bs_zpk`) -- the same formula, and the same latent Butterworth-
    // only-masked bug, as [`lp2hp`]'s own gain above (this previous
    // version had the ratio inverted: `prod(-p)/prod(-z)`, or plain
    // `prod(-p)` when zeros were empty -- again coinciding with the
    // correct answer only because Butterworth's prototype has `prod(-p) ==
    // 1`, exposed by `ellip`'s non-unit-magnitude prototype poles).
    let neg_poles: Vec<Complex> = z.poles.iter().map(|&v| v.neg()).collect();
    let neg_zeros: Vec<Complex> = z.zeros.iter().map(|&v| v.neg()).collect();
    let gain = z.gain * prod(&neg_zeros).div(prod(&neg_poles)).re;
    Zpk { zeros, poles, gain }
}

/// The bilinear transform `s = 2*fs*(z-1)/(z+1)`, mapping an analog
/// design to its digital equivalent. `fs` here is the *sample rate*
/// (already folded the standard `2*fs` prewarping factor in, so the
/// caller's analog design must already be frequency-prewarped to match).
fn bilinear(z: Zpk, fs: f64) -> Zpk {
    let degree = z.poles.len() - z.zeros.len();
    let fs2 = real(2.0 * fs);
    let map = |v: Complex| fs2.add(v).div(fs2.sub(v));
    let new_zeros: Vec<Complex> = z.zeros.iter().map(|&v| map(v)).collect();
    let new_poles: Vec<Complex> = z.poles.iter().map(|&v| map(v)).collect();
    let num: Complex = z.zeros.iter().fold(real(1.0), |acc, &v| acc.mul(fs2.sub(v)));
    let den: Complex = z.poles.iter().fold(real(1.0), |acc, &v| acc.mul(fs2.sub(v)));
    let gain = if z.zeros.is_empty() { z.gain / den.re } else { z.gain * num.div(den).re };
    let mut zeros = new_zeros;
    zeros.extend(std::iter::repeat_n(real(-1.0), degree));
    Zpk { zeros, poles: new_poles, gain }
}

/// One second-order section: `H(z) = (b0 + b1*z^-1 + b2*z^-2) / (1 + a1*z^-1 + a2*z^-2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

/// A digital filter as a cascade of second-order sections — the
/// numerically stable representation this module always designs into,
/// regardless of overall filter order.
#[derive(Clone, Debug, PartialEq)]
pub struct Sos {
    pub sections: Vec<Biquad>,
}

/// Groups poles and zeros (which always arrive in equal counts, and each
/// real-coefficient-preserving — real values or complex-conjugate pairs)
/// into second-order sections. Pairing order is simply "as encountered"
/// rather than SciPy's numerical-conditioning-optimized pairing — this
/// produces a mathematically equivalent filter (same transfer function,
/// same frequency response), just not necessarily the best-conditioned
/// section ordering for extreme orders.
fn zpk_to_sos(zpk: Zpk) -> Sos {
    let mut poles = zpk.poles;
    let mut zeros = zpk.zeros;
    // Pull out one pole/zero from a conjugate pair (or a lone real one)
    // at a time, forming a section from each. `im.abs() < tol` treats a
    // tiny imaginary residue (floating noise) as real.
    let tol = 1e-9;
    let mut sections = Vec::new();
    let mut first = true;
    while !poles.is_empty() {
        let p1 = poles.remove(0);
        let (p2, _) = if p1.im.abs() > tol {
            // Find `p1`'s actual conjugate partner wherever it landed --
            // does NOT assume conjugate pairs arrive adjacent in the
            // list (Butterworth's own pole generator happens to produce
            // that ordering, but a design with finite zeros/a lone real
            // pole placed elsewhere in the array, e.g. `ellip`'s analog
            // prototype, does not).
            match poles.iter().position(|c| (c.re - p1.re).abs() < 1e-6 && (c.im + p1.im).abs() < 1e-6) {
                Some(idx) => (poles.remove(idx), true),
                // No conjugate found (shouldn't happen for a genuine
                // real-coefficient design) -- degrade to a first-order
                // section rather than silently pairing with an unrelated
                // pole and corrupting the filter.
                None => (real(0.0), false),
            }
        } else {
            // `p1` is real: pair it with *another real* pole if one is
            // still available, wherever it is in the list -- pairing a
            // real pole with an unrelated complex one (by just grabbing
            // "whatever's next") would silently discard that complex
            // pole's imaginary part and corrupt the filter. If no other
            // real pole remains (the usual odd-order case: exactly one
            // real pole total), this pole stands alone as its own
            // first-order section (`a2 = 0`, via the `real(0.0)` partner).
            match poles.iter().position(|c| c.im.abs() <= tol) {
                Some(idx) => (poles.remove(idx), false),
                None => (real(0.0), false),
            }
        };
        let z1 = if !zeros.is_empty() { zeros.remove(0) } else { real(0.0) };
        let (z2, _) = if z1.im.abs() > tol {
            match zeros.iter().position(|c| (c.re - z1.re).abs() < 1e-6 && (c.im + z1.im).abs() < 1e-6) {
                Some(idx) => (zeros.remove(idx), true),
                None => (real(0.0), false),
            }
        } else {
            match zeros.iter().position(|c| c.im.abs() <= tol) {
                Some(idx) => (zeros.remove(idx), false),
                None => (real(0.0), false),
            }
        };
        let a1 = -(p1.re + p2.re);
        let a2 = p1.mul(p2).re;
        let mut b0 = 1.0;
        let mut b1 = -(z1.re + z2.re);
        let mut b2 = z1.mul(z2).re;
        if first {
            b0 *= zpk.gain;
            b1 *= zpk.gain;
            b2 *= zpk.gain;
            first = false;
        }
        sections.push(Biquad { b0, b1, b2, a1, a2 });
    }
    if sections.is_empty() {
        sections.push(Biquad { b0: zpk.gain, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 });
    }
    Sos { sections }
}

/// Which frequency band a Butterworth filter passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Low,
    High,
    Band,
    Stop,
}

/// Designs an order-`n` digital Butterworth filter at sample rate `fs`.
/// `cutoff` is one frequency (Hz) for `Low`/`High`, or `[low, high]` for
/// `Band`/`Stop`.
pub fn butter(n: usize, kind: FilterKind, cutoff: &[f64], fs: f64) -> Result<Sos, NumericError> {
    if n == 0 {
        return Err(NumericError::EmptyInput("butter: order must be at least 1"));
    }
    let prewarp = |fc: f64| 2.0 * fs * (PI * fc / fs).tan();
    let prototype = Zpk { zeros: Vec::new(), poles: analog_prototype_poles(n), gain: 1.0 };
    let analog = match kind {
        FilterKind::Low => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("butter: needs a cutoff frequency"))?);
            lp2lp(prototype, wc)
        }
        FilterKind::High => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("butter: needs a cutoff frequency"))?);
            lp2hp(prototype, wc)
        }
        FilterKind::Band | FilterKind::Stop => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("butter: band/stop needs [low, high] cutoffs"));
            }
            let w1 = prewarp(cutoff[0]);
            let w2 = prewarp(cutoff[1]);
            let wo = (w1 * w2).sqrt();
            let bw = w2 - w1;
            if kind == FilterKind::Band {
                lp2bp(prototype, wo, bw)
            } else {
                lp2bs(prototype, wo, bw)
            }
        }
    };
    Ok(zpk_to_sos(bilinear(analog, fs)))
}

// ---- Elliptic (Cauer) filter design ----
//
// An elliptic filter is equiripple in *both* the passband (like Chebyshev
// I) and the stopband (like Chebyshev II) simultaneously -- the steepest
// possible rolloff of any of the classical analog-prototype IIR families
// at a given order. That equiripple-in-both-bands behavior is what forces
// the harder math: the analog prototype's transfer function is built from
// the Jacobi elliptic functions (`sn`/`cn`/`dn`) rather than the plain
// trigonometric/hyperbolic pole placement Butterworth/Chebyshev use, and
// finding the prototype's poles *and* its finite transmission zeros (the
// zeros are what give the stopband its ripple, instead of monotonically
// decaying to zero the way Butterworth/Chebyshev-I do) requires solving
// the elliptic "degree equation" via nomes and a Landen transformation.
//
// This whole prototype construction (`ellip_analog_prototype` below) was
// ported from and cross-checked line-by-line against `scipy.signal.
// ellipap`'s own algorithm (itself citing Orfanidis, "Lecture Notes on
// Elliptic Filter Design", and Lutovac/Tosic/Evans, "Filter Design for
// Signal Processing") -- ellipap's approach was fetched and read directly
// rather than half-remembered, since getting this wrong silently produces
// a filter that merely runs without ever being the equiripple design it
// claims to be. It was then independently re-derived and validated in a
// standalone script against `scipy.signal.ellipap`/`scipy.signal.ellip`
// across many `(order, ripple, attenuation, band kind)` combinations
// (lowpass/highpass/bandpass/bandstop, matching to ~1e-14 on the finished
// digital filter's own frequency response) before being trusted here --
// this file's own unit tests below check the same equiripple properties
// (not a byte-for-byte port comparison) via `freqz`, matching the rest of
// this module's "verify the actual behavior" discipline.
//
// Everything past the prototype (frequency transform, bilinear transform,
// grouping into SOS) reuses `lp2lp`/`lp2hp`/`lp2bp`/`lp2bs`/`bilinear`/
// `zpk_to_sos` completely unmodified -- those are already generic over
// finite zeros (Butterworth just never exercises that path, having none).

/// Arithmetic-geometric mean, the standard fast (quadratically converging)
/// way to evaluate the complete elliptic integral of the first kind.
fn agm(a0: f64, b0: f64) -> f64 {
    let (mut a, mut b) = (a0, b0);
    for _ in 0..64 {
        if (a - b).abs() <= 1e-15 * a.abs() {
            break;
        }
        let (a_next, b_next) = (0.5 * (a + b), (a * b).sqrt());
        a = a_next;
        b = b_next;
    }
    a
}

/// Complete elliptic integral of the first kind `K(m)`, parameterized by
/// `m = k^2` (SciPy's own `scipy.special.ellipk` convention, not the
/// modulus `k`) via `K(m) = pi / (2*AGM(1, sqrt(1-m)))`.
fn ellip_k(m: f64) -> f64 {
    PI / (2.0 * agm(1.0, (1.0 - m).sqrt()))
}

/// The complementary integral `K'(m) = K(1-m)`, computed directly from `m`
/// (`agm(1, sqrt(m))`) rather than by first forming `1-m` and calling
/// [`ellip_k`] on that -- avoids the catastrophic cancellation that would
/// otherwise show up whenever `m` is very small (SciPy's own `ellipkm1`
/// exists for exactly this reason).
fn ellip_k_comp(m: f64) -> f64 {
    PI / (2.0 * agm(1.0, m.sqrt()))
}

/// Jacobi elliptic functions `sn(u,m)`, `cn(u,m)`, `dn(u,m)` (again `m =
/// k^2`) via the descending Landen/AGM transformation (Abramowitz & Stegun
/// 16.4): iterate `(a,b,c)` by the AGM step until `c` underflows, unwind
/// through the accumulated half-angle substitutions to get `sn`/`cn`
/// directly, then `dn = sqrt(1 - m*sn^2)` from its own defining identity
/// (always the non-negative root for real `u`, `0 <= m < 1`). Matched to
/// `scipy.special.ellipj` to full `f64` precision across a spread of
/// `(u, m)` pairs before being trusted (see this section's own doc
/// comment for the verification approach).
fn ellipj(u: f64, m: f64) -> (f64, f64, f64) {
    if m <= 0.0 {
        return (u.sin(), u.cos(), 1.0);
    }
    if m >= 1.0 {
        let sech = 1.0 / u.cosh();
        return (u.tanh(), sech, sech);
    }
    let mut a = vec![1.0];
    let mut b = vec![(1.0 - m).sqrt()];
    let mut c = vec![m.sqrt()];
    let mut n = 0usize;
    while c[n].abs() > 1e-15 && n < 50 {
        let (an, bn, cn) = (0.5 * (a[n] + b[n]), (a[n] * b[n]).sqrt(), 0.5 * (a[n] - b[n]));
        a.push(an);
        b.push(bn);
        c.push(cn);
        n += 1;
    }
    let mut phi = 2f64.powi(n as i32) * a[n] * u;
    for i in (1..=n).rev() {
        phi = 0.5 * (phi + (c[i] / a[i] * phi.sin()).asin());
    }
    let (sn, cn) = (phi.sin(), phi.cos());
    let dn = (1.0 - m * sn * sn).max(0.0).sqrt();
    (sn, cn, dn)
}

/// Inverse Jacobi `sn`: solve `w = sn(z, m)` for `z`, `w` possibly complex,
/// via the ascending Landen transformation (Orfanidis's algorithm, Eq. 56;
/// the same one `scipy.signal._filter_design._arc_jac_sn` implements).
/// Only used to locate `v0` in [`ellip_analog_prototype`] below, always at
/// a purely imaginary `w` in practice -- not a general-purpose complex
/// arcsn.
fn arc_jac_sn(w: Complex, m: f64) -> Result<Complex, NumericError> {
    let k = m.sqrt();
    if k > 1.0 {
        return Err(NumericError::Decomposition("ellip: modulus out of range while solving for the prototype's pole shift".to_string()));
    }
    if (k - 1.0).abs() < 1e-14 {
        // atanh(w) = 0.5 * ln((1+w)/(1-w)).
        return Ok(real(1.0).add(w).div(real(1.0).sub(w)).ln().scale(0.5));
    }
    let mut ks = vec![k];
    let mut niter = 0usize;
    loop {
        let last = *ks.last().expect("ks always has at least one element");
        if last.abs() <= 1e-300 {
            break;
        }
        let kp = ((1.0 - last) * (1.0 + last)).sqrt();
        ks.push((1.0 - kp) / (1.0 + kp));
        niter += 1;
        if niter > 60 {
            return Err(NumericError::Decomposition("ellip: Landen transformation did not converge".to_string()));
        }
    }
    let capk = ks[1..].iter().fold(1.0_f64, |acc, &kk| acc * (1.0 + kk)) * PI / 2.0;
    let mut wn = w;
    for idx in 0..ks.len() - 1 {
        let (kn, knext) = (ks[idx], ks[idx + 1]);
        let scaled = wn.scale(kn);
        let complement = real(1.0).sub(scaled.mul(scaled)).sqrt();
        let denom = real(1.0 + knext).mul(real(1.0).add(complement));
        wn = wn.scale(2.0).div(denom);
    }
    Ok(wn.asin().scale(2.0 / PI).scale(capk))
}

/// Real inverse Jacobi `sc` with complementary modulus: solve `w = sc(z,
/// 1-m)` for real `z`, via `sc(z,m) = -i*sn(i*z, 1-m)` (so `z` is the
/// imaginary part of `arc_jac_sn(i*w, m)`, which must come back with a
/// (numerically) zero real part).
fn arc_jac_sc1(w: f64, m: f64) -> Result<f64, NumericError> {
    let z = arc_jac_sn(Complex::new(0.0, w), m)?;
    if z.re.abs() > 1e-8 {
        return Err(NumericError::Decomposition("ellip: unexpected residual while solving for the prototype's pole shift".to_string()));
    }
    Ok(z.im)
}

/// Solves the elliptic "degree equation" for the prototype's modulus
/// parameter `m` (`= k^2`) given the order `n` and the discrimination
/// parameter `m1` (`= k1^2 = eps^2 / eps1^2`, `eps`/`eps1` the passband/
/// stopband ripple factors) -- Orfanidis Eq. (49): compute the nome `q1`
/// of `m1`, take its `n`-th root `q = q1^(1/n)`, then invert back to a
/// modulus via the standard theta-function series in `q` (Abramowitz &
/// Stegun 17.3.17, truncated at `MMAX=7` terms -- matching SciPy's own
/// `_ellipdeg`, whose comment notes this many terms is already generous).
fn ellip_deg(n: usize, m1: f64) -> f64 {
    let k1 = ellip_k(m1);
    let k1p = ellip_k_comp(m1);
    let q1 = (-PI * k1p / k1).exp();
    let q = q1.powf(1.0 / n as f64);
    const MMAX: usize = 7;
    let num: f64 = (0..=MMAX).map(|mm| q.powf(mm as f64 * (mm as f64 + 1.0))).sum();
    let den: f64 = 1.0 + 2.0 * (1..=MMAX + 1).map(|mm| q.powf((mm * mm) as f64)).sum::<f64>();
    16.0 * q * (num / den).powi(4)
}

/// The zeros/poles/gain of a normalized (cutoff at 1 rad/s, the point
/// where the gain first drops below `-rp` dB) analog elliptic lowpass
/// prototype with `rp` dB of passband ripple and at least `rs` dB of
/// stopband attenuation. Unlike Butterworth/Chebyshev, this has genuine
/// finite zeros (on the imaginary axis) in addition to poles -- the
/// "Cauer" zeros responsible for the equiripple stopband.
fn ellip_analog_prototype(n: usize, rp: f64, rs: f64) -> Result<Zpk, NumericError> {
    if n == 0 {
        return Err(NumericError::EmptyInput("ellip: order must be at least 1"));
    }
    if rp <= 0.0 || rs <= 0.0 {
        return Err(NumericError::EmptyInput("ellip: ripple_db and atten_db must both be positive"));
    }
    if n == 1 {
        // Degenerates to a single real pole -- ripple/equiripple stopband
        // behavior needs at least one complex-conjugate pole pair to show
        // up at all, so order 1 is governed by `rp` alone, same as
        // Chebyshev I's own order-1 special case.
        let p = -(1.0 / (10f64.powf(0.1 * rp) - 1.0)).sqrt();
        return Ok(Zpk { zeros: Vec::new(), poles: vec![real(p)], gain: -p });
    }
    let eps_sq = 10f64.powf(0.1 * rp) - 1.0;
    let eps = eps_sq.sqrt();
    let stop_term = 10f64.powf(0.1 * rs) - 1.0;
    if stop_term <= 0.0 {
        return Err(NumericError::Decomposition("ellip: cannot meet the requested ripple/attenuation specification".to_string()));
    }
    let ck1_sq = eps_sq / stop_term;
    if !(0.0..1.0).contains(&ck1_sq) {
        return Err(NumericError::Decomposition(
            "ellip: cannot meet the requested specification (ripple_db must be small relative to atten_db)".to_string(),
        ));
    }

    let m = ellip_deg(n, ck1_sq);
    let capk = ellip_k(m);
    let k1 = ellip_k(ck1_sq);

    // `np.arange(1 - n%2, n, 2)`: odd indices for even `n`, even indices
    // (including 0) for odd `n`.
    let start = if n % 2 == 0 { 1usize } else { 0usize };
    let js: Vec<usize> = (0..n).map(|i| start + 2 * i).filter(|&j| j < n).collect();
    let jj = js.len();
    let mut s = Vec::with_capacity(jj);
    let mut c = Vec::with_capacity(jj);
    let mut d = Vec::with_capacity(jj);
    for &j in &js {
        let (sj, cj, dj) = ellipj(j as f64 * capk / n as f64, m);
        s.push(sj);
        c.push(cj);
        d.push(dj);
    }

    const EPSILON: f64 = 2e-16;
    let sqrt_m = m.sqrt();
    let mut zeros: Vec<Complex> = s.iter().filter(|&&sj| sj.abs() > EPSILON).map(|&sj| Complex::new(0.0, 1.0 / (sqrt_m * sj))).collect();
    if zeros.is_empty() {
        return Err(NumericError::Decomposition("ellip: degenerate prototype (no finite transmission zeros found)".to_string()));
    }
    let conj_zeros: Vec<Complex> = zeros.iter().map(|z| Complex::new(z.re, -z.im)).collect();
    zeros.extend(conj_zeros);

    let r = arc_jac_sc1(1.0 / eps, ck1_sq)?;
    let v0 = capk * r / (n as f64 * k1);
    let (sv, cv, dv) = ellipj(v0, 1.0 - m);

    let mut poles: Vec<Complex> = (0..jj)
        .map(|i| {
            let denom = 1.0 - (d[i] * sv).powi(2);
            Complex::new(-(c[i] * d[i] * sv * cv) / denom, -(s[i] * dv) / denom)
        })
        .collect();

    if n % 2 == 1 {
        let norm = poles.iter().map(|p| p.re * p.re + p.im * p.im).sum::<f64>().sqrt();
        let conj_extra: Vec<Complex> = poles.iter().filter(|p| p.im.abs() > EPSILON * norm).map(|p| Complex::new(p.re, -p.im)).collect();
        poles.extend(conj_extra);
    } else {
        let conj_all: Vec<Complex> = poles.iter().map(|p| Complex::new(p.re, -p.im)).collect();
        poles.extend(conj_all);
    }

    let neg_poles: Vec<Complex> = poles.iter().map(|p| p.neg()).collect();
    let neg_zeros: Vec<Complex> = zeros.iter().map(|z| z.neg()).collect();
    let mut gain = prod(&neg_poles).div(prod(&neg_zeros)).re;
    if n % 2 == 0 {
        gain /= (1.0 + eps_sq).sqrt();
    }

    Ok(Zpk { zeros, poles, gain })
}

/// Designs an order-`n` digital elliptic (Cauer) filter at sample rate
/// `fs`, equiripple in both the passband (within `ripple_db` dB of 0dB)
/// and the stopband (at least `atten_db` dB down) simultaneously -- the
/// steepest transition band of any of the classical IIR families at a
/// given order. `cutoff` is the point in the transition band where the
/// gain first drops below `-ripple_db` (the same ripple-boundary
/// convention Chebyshev I uses, not Butterworth's -3dB point).
/// `cutoff`/`fs`/`kind` otherwise follow the same convention as [`butter`];
/// this reuses [`butter`]'s exact prewarp/frequency-transform/bilinear/SOS
/// pipeline, only the analog prototype itself differs.
pub fn ellip(n: usize, ripple_db: f64, atten_db: f64, kind: FilterKind, cutoff: &[f64], fs: f64) -> Result<Sos, NumericError> {
    let prewarp = |fc: f64| 2.0 * fs * (PI * fc / fs).tan();
    let prototype = ellip_analog_prototype(n, ripple_db, atten_db)?;
    let analog = match kind {
        FilterKind::Low => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("ellip: needs a cutoff frequency"))?);
            lp2lp(prototype, wc)
        }
        FilterKind::High => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("ellip: needs a cutoff frequency"))?);
            lp2hp(prototype, wc)
        }
        FilterKind::Band | FilterKind::Stop => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("ellip: band/stop needs [low, high] cutoffs"));
            }
            let w1 = prewarp(cutoff[0]);
            let w2 = prewarp(cutoff[1]);
            let wo = (w1 * w2).sqrt();
            let bw = w2 - w1;
            if kind == FilterKind::Band {
                lp2bp(prototype, wo, bw)
            } else {
                lp2bs(prototype, wo, bw)
            }
        }
    };
    Ok(zpk_to_sos(bilinear(analog, fs)))
}

// ---- Chebyshev filter design (Type I and Type II / inverse Chebyshev) ----
//
// Chebyshev Type I is equiripple in the passband (bounded by `ripple_db`)
// and monotonic in the stopband -- steeper rolloff than Butterworth at the
// same order, at the cost of passband ripple. No finite zeros (all zeros
// at infinity), so structurally it's the simplest of the four IIR families
// here: just a different analog-prototype pole placement, reusing
// `butter`'s prewarp/transform/bilinear/SOS pipeline unmodified.
//
// Chebyshev Type II (inverse Chebyshev) is the mirror image: monotonic
// passband, equiripple stopband (guaranteed at least `atten_db` down
// starting at the stopband edge), flatter passband than Type I at the cost
// of a wider transition band. Unlike Type I, it has genuine finite zeros
// on the imaginary axis (from the reciprocal-of-Chebyshev-poles
// construction) -- the property flagged up front as most likely to
// exercise the same pole/zero-pairing and finite-zero gain-formula code
// paths `ellip` needed fixed (see that function's own doc comment above),
// so its own tests below check the equiripple property exhaustively rather
// than assuming the shared pipeline is now bug-free.
//
// Both prototypes were ported from the *current* `scipy.signal.cheb1ap`/
// `cheb2ap` source (fetched directly, scipy 1.17.1 installed locally,
// rather than reconstructed from a half-remembered textbook formula) --
// their compact `p = -sinh(mu + i*theta)`/`p = -1/sinh(mu + i*theta)` form
// is algebraically identical to (but easier to get right than) the
// classical `p = -sinh(mu)*sin(theta) + i*cosh(mu)*cos(theta)`
// presentation some textbooks use. Both prototypes, and the full
// prototype -> frequency-transform -> bilinear -> SOS pipeline built on
// top of them, were independently re-derived in a standalone Python script
// and cross-checked against real `scipy.signal.cheb1ap`/`cheb2ap` (poles/
// zeros/gain, exact to 1e-9) and the full `scipy.signal.cheby1`/`cheby2`
// digital design (frequency response, exact to floating-point noise
// ~1e-9..1e-14) across a spread of `(order, ripple/attenuation, band
// kind)` combinations -- including the odd-order Type II case specifically
// (real pole + finite-zero-deficit relative to pole count, the structural
// case most likely to trip the shared pipeline) -- before any of it was
// trusted here.

/// Inverse hyperbolic sine, `asinh(x) = ln(x + sqrt(x^2+1))` -- only ever
/// evaluated here at a positive real argument (`1/eps`, `1/de`), so this
/// plain real-valued form is all either prototype below needs.
fn asinh(x: f64) -> f64 {
    (x + (x * x + 1.0).sqrt()).ln()
}

/// The poles/zeros/gain of a normalized (cutoff at 1 rad/s, the point where
/// the gain first drops below `-rp` dB) analog Chebyshev Type I lowpass
/// prototype. No finite zeros -- all zeros at infinity, unlike Type II.
fn cheby1_analog_prototype(n: usize, rp: f64) -> Result<Zpk, NumericError> {
    if n == 0 {
        return Err(NumericError::EmptyInput("cheby1: order must be at least 1"));
    }
    if rp <= 0.0 {
        return Err(NumericError::EmptyInput("cheby1: ripple_db must be positive"));
    }
    let eps = (10f64.powf(0.1 * rp) - 1.0).sqrt();
    let mu = asinh(1.0 / eps) / n as f64;
    let (sinh_mu, cosh_mu) = (mu.sinh(), mu.cosh());
    // `m` ranges over the `n` odd (if `n` even) or even (if `n` odd)
    // integers from `-(n-1)` to `n-1` in steps of 2 -- `scipy.signal.
    // cheb1ap`'s own `np.arange(-N+1, N, 2)`.
    let poles: Vec<Complex> = (0..n)
        .map(|k| {
            let m = -(n as f64 - 1.0) + 2.0 * k as f64;
            let theta = PI * m / (2.0 * n as f64);
            // `p = -sinh(mu + i*theta)`, expanded via `sinh(a+ib) =
            // sinh(a)cos(b) + i*cosh(a)sin(b)`.
            Complex::new(-sinh_mu * theta.cos(), -cosh_mu * theta.sin())
        })
        .collect();
    let neg_poles: Vec<Complex> = poles.iter().map(|p| p.neg()).collect();
    let mut gain = prod(&neg_poles).re;
    if n % 2 == 0 {
        // Even order has no pole landing exactly on the real axis, so the
        // plain `prod(-poles)` isn't yet normalized to the `-rp` dB
        // passband-ripple convention -- matches `scipy.signal.cheb1ap`'s
        // own even-order correction.
        gain /= (1.0 + eps * eps).sqrt();
    }
    Ok(Zpk { zeros: Vec::new(), poles, gain })
}

/// The poles/zeros/gain of a normalized (cutoff at 1 rad/s, the point where
/// the attenuation first reaches `rs` dB) analog Chebyshev Type II (inverse
/// Chebyshev) lowpass prototype. Has genuine finite zeros on the imaginary
/// axis: `n-1` of them for odd `n`, `n` for even `n` -- the "missing" zero
/// at `m=0` would need `1/sin(0)`, undefined, so for odd `n` (whose `m`
/// range always includes 0) that one term is simply dropped, the standard
/// inverse-Chebyshev fact that an odd-order design has one pole with no
/// finite-zero partner (that pole ends up alone, at the real axis).
fn cheby2_analog_prototype(n: usize, rs: f64) -> Result<Zpk, NumericError> {
    if n == 0 {
        return Err(NumericError::EmptyInput("cheby2: order must be at least 1"));
    }
    if rs <= 0.0 {
        return Err(NumericError::EmptyInput("cheby2: atten_db must be positive"));
    }
    let de = 1.0 / (10f64.powf(0.1 * rs) - 1.0).sqrt();
    let mu = asinh(1.0 / de) / n as f64;
    let (sinh_mu, cosh_mu) = (mu.sinh(), mu.cosh());
    let ms: Vec<f64> = (0..n).map(|k| -(n as f64 - 1.0) + 2.0 * k as f64).collect();
    let zeros: Vec<Complex> = ms
        .iter()
        .filter(|&&m| m != 0.0)
        .map(|&m| {
            let theta = PI * m / (2.0 * n as f64);
            Complex::new(0.0, 1.0 / theta.sin())
        })
        .collect();
    let poles: Vec<Complex> = ms
        .iter()
        .map(|&m| {
            let theta = PI * m / (2.0 * n as f64);
            // `p = -1/sinh(mu + i*theta)`.
            let denom = Complex::new(sinh_mu * theta.cos(), cosh_mu * theta.sin());
            real(-1.0).div(denom)
        })
        .collect();
    let neg_poles: Vec<Complex> = poles.iter().map(|p| p.neg()).collect();
    let neg_zeros: Vec<Complex> = zeros.iter().map(|z| z.neg()).collect();
    let gain = prod(&neg_poles).div(prod(&neg_zeros)).re;
    Ok(Zpk { zeros, poles, gain })
}

/// Designs an order-`n` digital Chebyshev Type I filter at sample rate
/// `fs`, equiripple in the passband (within `ripple_db` dB of 0dB) and
/// monotonic in the stopband. `cutoff` is the point in the transition band
/// where the gain first drops below `-ripple_db` (matching MATLAB's/
/// SciPy's own `cheby1(n, Rp, Wn)`), not Butterworth's -3dB point.
/// `cutoff`/`fs`/`kind` otherwise follow the same convention as [`butter`];
/// this reuses [`butter`]'s exact prewarp/frequency-transform/bilinear/SOS
/// pipeline, only the analog prototype itself differs.
pub fn cheby1(n: usize, ripple_db: f64, kind: FilterKind, cutoff: &[f64], fs: f64) -> Result<Sos, NumericError> {
    let prewarp = |fc: f64| 2.0 * fs * (PI * fc / fs).tan();
    let prototype = cheby1_analog_prototype(n, ripple_db)?;
    let analog = match kind {
        FilterKind::Low => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("cheby1: needs a cutoff frequency"))?);
            lp2lp(prototype, wc)
        }
        FilterKind::High => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("cheby1: needs a cutoff frequency"))?);
            lp2hp(prototype, wc)
        }
        FilterKind::Band | FilterKind::Stop => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("cheby1: band/stop needs [low, high] cutoffs"));
            }
            let w1 = prewarp(cutoff[0]);
            let w2 = prewarp(cutoff[1]);
            let wo = (w1 * w2).sqrt();
            let bw = w2 - w1;
            if kind == FilterKind::Band {
                lp2bp(prototype, wo, bw)
            } else {
                lp2bs(prototype, wo, bw)
            }
        }
    };
    Ok(zpk_to_sos(bilinear(analog, fs)))
}

/// Designs an order-`n` digital Chebyshev Type II (inverse Chebyshev)
/// filter at sample rate `fs`, monotonic in the passband and equiripple in
/// the stopband (at least `atten_db` dB down starting at the stopband
/// edge). `cutoff` is the point in the transition band where the
/// attenuation first reaches `atten_db` (matching MATLAB's/SciPy's own
/// `cheby2(n, Rs, Wn)`), the stopband-edge convention (the mirror of
/// `cheby1`'s passband-edge one). `cutoff`/`fs`/`kind` otherwise follow the
/// same convention as [`butter`]; this reuses [`butter`]'s exact prewarp/
/// frequency-transform/bilinear/SOS pipeline, only the analog prototype
/// itself differs.
pub fn cheby2(n: usize, atten_db: f64, kind: FilterKind, cutoff: &[f64], fs: f64) -> Result<Sos, NumericError> {
    let prewarp = |fc: f64| 2.0 * fs * (PI * fc / fs).tan();
    let prototype = cheby2_analog_prototype(n, atten_db)?;
    let analog = match kind {
        FilterKind::Low => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("cheby2: needs a cutoff frequency"))?);
            lp2lp(prototype, wc)
        }
        FilterKind::High => {
            let wc = prewarp(*cutoff.first().ok_or(NumericError::EmptyInput("cheby2: needs a cutoff frequency"))?);
            lp2hp(prototype, wc)
        }
        FilterKind::Band | FilterKind::Stop => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("cheby2: band/stop needs [low, high] cutoffs"));
            }
            let w1 = prewarp(cutoff[0]);
            let w2 = prewarp(cutoff[1]);
            let wo = (w1 * w2).sqrt();
            let bw = w2 - w1;
            if kind == FilterKind::Band {
                lp2bp(prototype, wo, bw)
            } else {
                lp2bs(prototype, wo, bw)
            }
        }
    };
    Ok(zpk_to_sos(bilinear(analog, fs)))
}

/// Per-section running state for streaming (sample-at-a-time) filtering —
/// Direct Form II Transposed, the standard low-latency, numerically
/// well-behaved realization.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BiquadState {
    pub z1: f64,
    pub z2: f64,
}

fn biquad_step(bq: &Biquad, state: &mut BiquadState, x: f64) -> f64 {
    let y = bq.b0 * x + state.z1;
    state.z1 = bq.b1 * x - bq.a1 * y + state.z2;
    state.z2 = bq.b2 * x - bq.a2 * y;
    y
}

/// One sample through every section of the cascade, in place.
pub fn sos_step(sos: &Sos, states: &mut [BiquadState], x: f64) -> f64 {
    let mut v = x;
    for (bq, st) in sos.sections.iter().zip(states) {
        v = biquad_step(bq, st, v);
    }
    v
}

/// Causal (one-pass) filtering of a whole signal — has the usual IIR
/// phase distortion; see [`filtfilt`] for a zero-phase alternative.
pub fn sosfilt(sos: &Sos, x: &[f64]) -> Vec<f64> {
    let mut states = vec![BiquadState::default(); sos.sections.len()];
    x.iter().map(|&v| sos_step(sos, &mut states, v)).collect()
}

/// Zero-phase filtering: filter forward, reverse, filter again, reverse
/// again — doubles the effective order but cancels phase distortion
/// entirely (the standard offline-analysis choice over plain `sosfilt`).
/// **Simplification versus reference implementations**: no edge padding
/// is applied before the forward/backward passes, so transients at the
/// very start/end of `x` are more pronounced than in padded versions.
pub fn filtfilt(sos: &Sos, x: &[f64]) -> Vec<f64> {
    let forward = sosfilt(sos, x);
    let mut reversed: Vec<f64> = forward.into_iter().rev().collect();
    reversed = sosfilt(sos, &reversed);
    reversed.reverse();
    reversed
}

/// Frequency response `H(e^{jω})` of an SOS cascade, sampled at `n` points
/// evenly spaced from DC (`ω=0`) to Nyquist (`ω=π`), inclusive — MATLAB's/
/// SciPy's own half-spectrum convention (a real-coefficient filter's
/// response above Nyquist is the mirror image of below it, so there's
/// nothing new to show there). Each section's `H_k(z) = (b0 + b1*z^-1 +
/// b2*z^-2) / (1 + a1*z^-1 + a2*z^-2)` is evaluated directly at `z^-1 =
/// e^{-jω}` and the sections multiply, exactly mirroring [`biquad_step`]'s
/// own time-domain difference equation (same coefficients, same
/// normalization, just evaluated on the unit circle instead of stepped
/// sample-by-sample) — the standard definition, not an approximation.
pub fn freqz(sos: &Sos, n: usize) -> Vec<crate::Complex64> {
    let denom = (n.max(2) - 1) as f64;
    (0..n.max(1))
        .map(|k| {
            let omega = PI * k as f64 / denom;
            let z_inv = crate::Complex64::from_polar(1.0, -omega);
            let z_inv2 = z_inv.mul(z_inv);
            sos.sections.iter().fold(crate::Complex64::real(1.0), |h, bq| {
                let num = crate::Complex64::real(bq.b0).add(z_inv.scale(bq.b1)).add(z_inv2.scale(bq.b2));
                let den = crate::Complex64::real(1.0).add(z_inv.scale(bq.a1)).add(z_inv2.scale(bq.a2));
                h.mul(num.div(den))
            })
        })
        .collect()
}

/// Group delay (in samples), `-dφ/dω` where `φ = arg(H(e^{jω}))` — the
/// standard definition, computed practically: unwrap the phase from
/// [`freqz`] (remove spurious `±2π` jumps between adjacent samples before
/// differentiating — the classic pitfall of differentiating `atan2`'s
/// `(-π, π]`-wrapped output directly) then take a central-difference
/// derivative (forward/backward at the two endpoints), negated. An exact
/// closed-form method exists (differentiating the numerator/denominator
/// polynomials directly, avoiding unwrapping entirely) but needs per-
/// section polynomial derivatives for comparatively little practical
/// benefit — this is accurate everywhere except very close to a response
/// null, where every group-delay definition is acutely sensitive anyway.
pub fn group_delay(sos: &Sos, n: usize) -> Vec<f64> {
    group_delay_from_response(&freqz(sos, n), n)
}

/// The shared unwrap-then-differentiate machinery [`group_delay`] and
/// [`fir_group_delay`] both use, factored out so it operates on an already-
/// sampled frequency response rather than assuming an `Sos` — the same
/// general phase-derivative definition applies to any filter representation
/// (IIR or FIR), not just the biquad-cascade one, and `fir_group_delay`
/// reuses it exactly rather than duplicating the unwrap logic.
fn group_delay_from_response(h: &[crate::Complex64], n: usize) -> Vec<f64> {
    let mut phase: Vec<f64> = h.iter().map(|c| c.arg()).collect();
    for i in 1..phase.len() {
        let mut diff = phase[i] - phase[i - 1];
        while diff > PI {
            phase[i] -= 2.0 * PI;
            diff -= 2.0 * PI;
        }
        while diff < -PI {
            phase[i] += 2.0 * PI;
            diff += 2.0 * PI;
        }
    }
    let n_pts = phase.len();
    if n_pts == 0 {
        return Vec::new();
    }
    let domega = PI / (n.max(2) - 1) as f64;
    (0..n_pts)
        .map(|i| {
            let dphi = if n_pts == 1 {
                0.0
            } else if i == 0 {
                phase[1] - phase[0]
            } else if i == n_pts - 1 {
                phase[i] - phase[i - 1]
            } else {
                (phase[i + 1] - phase[i - 1]) / 2.0
            };
            -dphi / domega
        })
        .collect()
}

// ---- FIR design (windowed-sinc) and analysis ----
//
// A cascade of biquad sections *multiplies* small transfer functions
// together (convolving 3-tap responses pairwise) -- it cannot represent an
// arbitrary N-tap FIR without first factoring the FIR's transfer-function
// polynomial into quadratic roots, a real numerical step this module does
// not do. An FIR filter also fundamentally has no feedback/denominator at
// all, unlike `Sos`. So FIR gets its own parallel representation: a plain
// tap vector `b`, no `Sos` involved anywhere below.

/// Windowed-sinc FIR filter design (MATLAB's/SciPy's `fir1`): the ideal
/// (infinite) lowpass/highpass/bandpass/bandstop impulse response, truncated
/// to `n+1` taps centered at `n/2`, multiplied pointwise by the caller-
/// supplied `window` (`window.len()` must be `n+1` -- computed by the caller
/// from `hamming`/`hann`/`blackman`/`kaiser`'s own functions, not
/// reimplemented here), then normalized so the gain at the center of the
/// passband is exactly unity (DC for `Low`, Nyquist for `High`, the band
/// center `(w1+w2)/2` for `Band`, DC for `Stop` -- matching MATLAB's own
/// `fir1` normalization convention).
///
/// `cutoff`/`fs` use the same convention as [`butter`]: one frequency (Hz)
/// for `Low`/`High`, `[low, high]` for `Band`/`Stop`. Pass `fs = 2.0` for
/// MATLAB's own *normalized* convention instead, where `cutoff` is directly
/// in `[0, 1]` with `1` = Nyquist (the same default `fvtool` already falls
/// back to when a filter model has no `fs` field).
pub fn fir1(n: usize, kind: FilterKind, cutoff: &[f64], fs: f64, window: &[f64]) -> Result<Vec<f64>, NumericError> {
    let taps = n + 1;
    if window.len() != taps {
        return Err(NumericError::ShapeMismatch { expected: taps, found: window.len() });
    }
    let to_omega = |fc: f64| 2.0 * PI * fc / fs;
    let alpha = n as f64 / 2.0;
    // The ideal (unwindowed) lowpass impulse response with cutoff `omega_c`
    // (rad/sample), centered at `alpha` -- `sinc(omega_c*(k-alpha))`, with
    // the `k == alpha` singularity resolved to its limit `omega_c/pi`.
    let ideal_lp = |omega_c: f64| -> Vec<f64> {
        (0..taps)
            .map(|k| {
                let m = k as f64 - alpha;
                if m.abs() < 1e-12 { omega_c / PI } else { (omega_c * m).sin() / (PI * m) }
            })
            .collect()
    };
    let delta = |k: usize| -> f64 { if (k as f64 - alpha).abs() < 1e-12 { 1.0 } else { 0.0 } };
    let (h, w0) = match kind {
        FilterKind::Low => {
            let wc = to_omega(*cutoff.first().ok_or(NumericError::EmptyInput("fir1: needs a cutoff frequency"))?);
            (ideal_lp(wc), 0.0)
        }
        FilterKind::High => {
            // Spectral inversion: ideal highpass = allpass (a unit impulse)
            // minus the ideal lowpass at the same cutoff.
            let wc = to_omega(*cutoff.first().ok_or(NumericError::EmptyInput("fir1: needs a cutoff frequency"))?);
            let lp = ideal_lp(wc);
            let h: Vec<f64> = (0..taps).map(|k| delta(k) - lp[k]).collect();
            (h, PI)
        }
        FilterKind::Band => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("fir1: band/stop needs [low, high] cutoffs"));
            }
            let (w1, w2) = (to_omega(cutoff[0]), to_omega(cutoff[1]));
            let (lp1, lp2) = (ideal_lp(w1), ideal_lp(w2));
            let h: Vec<f64> = (0..taps).map(|k| lp2[k] - lp1[k]).collect();
            (h, (w1 + w2) / 2.0)
        }
        FilterKind::Stop => {
            if cutoff.len() != 2 {
                return Err(NumericError::EmptyInput("fir1: band/stop needs [low, high] cutoffs"));
            }
            let (w1, w2) = (to_omega(cutoff[0]), to_omega(cutoff[1]));
            let (lp1, lp2) = (ideal_lp(w1), ideal_lp(w2));
            // Ideal bandstop = allpass minus the ideal bandpass at the same edges.
            let h: Vec<f64> = (0..taps).map(|k| lp1[k] + delta(k) - lp2[k]).collect();
            (h, 0.0)
        }
    };
    let windowed: Vec<f64> = h.iter().zip(window).map(|(&hi, &wi)| hi * wi).collect();
    // Rescale so `|H(e^{j*w0})| == 1` exactly -- a plain DTFT sum evaluated
    // at the one frequency that matters, not a full `fir_freqz` sweep.
    let gain = windowed
        .iter()
        .enumerate()
        .fold(crate::Complex64::real(0.0), |acc, (k, &b)| acc.add(crate::Complex64::from_polar(b, -w0 * k as f64)))
        .magnitude();
    if gain < 1e-300 {
        return Err(NumericError::Decomposition("fir1: passband gain normalization failed (near-zero response at the design frequency)".to_string()));
    }
    Ok(windowed.iter().map(|&b| b / gain).collect())
}

/// Weighted least-squares FIR design (MATLAB's/SciPy's `firls`): given a
/// piecewise-linear desired response specified as band edges (pairs of
/// normalized frequencies in `[0, 1]`, `1` = Nyquist, matching `fir1`'s own
/// `fs = 2.0` convention) and the desired amplitude at each edge, finds the
/// length-`n+1` symmetric (Type I linear-phase) FIR that minimizes the
/// weighted integral squared error against that desired response.
///
/// Restricted to even `n` (so `n+1` taps is odd) -- the standard Type I
/// case, same restriction SciPy's own `firls` imposes (it requires odd
/// `numtaps`); an odd `n` would need the Type II half-sample-shifted basis
/// instead, which this function doesn't build.
///
/// The classical closed-form solution (Parks & Burrus): set up a continuous
/// weighted L2 minimization over `a_0..a_M` (`M = n/2`), the cosine-series
/// coefficients of the zero-phase response `A(w) = sum_m a_m*cos(m*w)`, and
/// solve it via the analytic integrals of `cos(iw)*cos(jw)` and
/// `D(w)*cos(iw)` over each band (`D(w)` the piecewise-linear desired
/// response). Differentiating the weighted integral squared error `sum_bands
/// weight * integral (A(w)-D(w))^2 dw` with respect to each `a_i` gives the
/// normal equations `Q*a = b` directly, with (per band, `L = w2-w1`):
///
/// - `Q[i,j] = weight * 0.5*(intcos(i-j,w1,w2) + intcos(i+j,w1,w2))`, from
///   `cos(iw)cos(jw) = 0.5*(cos((i-j)w)+cos((i+j)w))`, where
///   `intcos(k,w1,w2) = integral_{w1}^{w2} cos(k*w) dw` (`= L` at `k=0`,
///   `= (sin(k*w2)-sin(k*w1))/k` otherwise).
/// - `b[i] = weight * (d1*intcos(i,w1,w2) + slope*intramp(i,w1,w2))`, from
///   `D(w) = d1 + slope*(w-w1)`, `slope = (d2-d1)/L`, where
///   `intramp(i,w1,w2) = integral_{w1}^{w2} (w-w1)*cos(i*w) dw` (`= L^2/2`
///   at `i=0`; otherwise, by parts,
///   `L*sin(i*w2)/i + (cos(i*w2)-cos(i*w1))/i^2`).
///
/// `Q` is `(M+1)x(M+1)` (a sum of a Toeplitz and a Hankel term, one pair per
/// band) -- independent of any frequency grid, unlike an earlier version of
/// this function that discretized each band into `O(taps)` sample points
/// and ran a generic overdetermined least-squares solve: that grid scaled
/// with the filter order on *both* the row and column side of the solve,
/// making the whole thing `O(taps^3)` and reaching 26-40s at 1024 taps (see
/// `benchmarks/filter_design/README.md`). Solving the compact,
/// grid-independent `Q*a=b` here instead (still via `qu_core::linalg::
/// least_squares`'s pseudo-inverse, for the same near-singular-Q robustness
/// the old grid-based system relied on) is `O(M^3)` with `M = n/2` --
/// `~n^3/8` against the old path's `~n^3` (the discretization grid alone
/// was `32*taps` points per band) -- and, being the exact closed form
/// rather than a Riemann-sum approximation of the same integral, is also
/// *more* accurate than the grid-based version it replaces, not just
/// faster. (A further Toeplitz/Levinson-Durbin-style `O(M^2)` solve was
/// considered but not implemented: `Q` here is Toeplitz-*plus*-Hankel, not
/// pure Toeplitz, so plain Levinson-Durbin does not apply, and neither
/// MATLAB's nor SciPy's own `firls` bothers with a structured solve of it
/// either -- both build this same small dense `(M+1)x(M+1)` system and pass
/// it straight to a generic solver, because `M` is already small enough
/// that `O(M^3)` costs microseconds to low milliseconds even at four-digit
/// filter orders.) Verified against the previous grid-based implementation
/// across multiple orders/band configurations to `1e-6` (see
/// `firls_matches_the_reference_grid_based_solution` below), which itself
/// was cross-checked against `scipy.signal.firls` before being trusted.
pub fn firls(n: usize, freq_bands: &[f64], desired: &[f64], weights: Option<&[f64]>) -> Result<Vec<f64>, NumericError> {
    if n == 0 || n % 2 != 0 {
        return Err(NumericError::EmptyInput("firls: order n must be even (n+1 taps, a Type I symmetric FIR)"));
    }
    if freq_bands.is_empty() || freq_bands.len() % 2 != 0 {
        return Err(NumericError::EmptyInput("firls: freq_bands must be given as [lo, hi] pairs, one pair per band"));
    }
    if desired.len() != freq_bands.len() {
        return Err(NumericError::ShapeMismatch { expected: freq_bands.len(), found: desired.len() });
    }
    let n_bands = freq_bands.len() / 2;
    let weight_vec: Vec<f64> = match weights {
        Some(w) => {
            if w.len() != n_bands {
                return Err(NumericError::ShapeMismatch { expected: n_bands, found: w.len() });
            }
            w.to_vec()
        }
        None => vec![1.0; n_bands],
    };

    let taps = n + 1;
    let half = n / 2;
    let ncoef = half + 1;

    // integral_{w1}^{w2} cos(k*w) dw -- `k == 0` handled as its own limit.
    let intcos = |k: i64, w1: f64, w2: f64| -> f64 {
        if k == 0 {
            w2 - w1
        } else {
            let kf = k as f64;
            ((kf * w2).sin() - (kf * w1).sin()) / kf
        }
    };
    // integral_{w1}^{w2} (w - w1) * cos(k*w) dw, by parts.
    let intramp = |k: i64, w1: f64, w2: f64| -> f64 {
        let l = w2 - w1;
        if k == 0 {
            0.5 * l * l
        } else {
            let kf = k as f64;
            l * (kf * w2).sin() / kf + ((kf * w2).cos() - (kf * w1).cos()) / (kf * kf)
        }
    };

    let mut q = Matrix::zeros(ncoef, ncoef);
    let mut b = vec![0.0; ncoef];

    for band in 0..n_bands {
        let (f1, f2) = (freq_bands[2 * band], freq_bands[2 * band + 1]);
        let (d1, d2) = (desired[2 * band], desired[2 * band + 1]);
        let weight = weight_vec[band].max(0.0);
        if !(0.0..=1.0).contains(&f1) || !(0.0..=1.0).contains(&f2) || f2 < f1 {
            return Err(NumericError::EmptyInput("firls: freq_bands entries must be nondecreasing pairs within [0, 1]"));
        }
        let (w1, w2) = (PI * f1, PI * f2);
        if (w2 - w1).abs() < 1e-12 {
            // Degenerate zero-width band: an exact point constraint at
            // `w1`. This is the same normal-equations contribution a
            // single least-squares row `sqrt(weight)*cos(m*w1)` (target
            // `sqrt(weight)*d1`) would make: `weight*cos(i*w1)*cos(j*w1)`
            // into `Q`, `weight*d1*cos(i*w1)` into `b`.
            let cosvec: Vec<f64> = (0..ncoef).map(|m| (m as f64 * w1).cos()).collect();
            for i in 0..ncoef {
                b[i] += weight * d1 * cosvec[i];
                for j in 0..ncoef {
                    let prior = q.get(i, j).unwrap_or(0.0);
                    let _ = q.set(i, j, prior + weight * cosvec[i] * cosvec[j]);
                }
            }
            continue;
        }
        let slope = (d2 - d1) / (w2 - w1);
        for i in 0..ncoef {
            for j in i..ncoef {
                let cij = 0.5 * (intcos(i as i64 - j as i64, w1, w2) + intcos(i as i64 + j as i64, w1, w2));
                let contribution = weight * cij;
                let prior_ij = q.get(i, j).unwrap_or(0.0);
                let _ = q.set(i, j, prior_ij + contribution);
                if i != j {
                    let prior_ji = q.get(j, i).unwrap_or(0.0);
                    let _ = q.set(j, i, prior_ji + contribution);
                }
            }
            b[i] += weight * (d1 * intcos(i as i64, w1, w2) + slope * intramp(i as i64, w1, w2));
        }
    }

    let observations = Matrix::from_col_major(ncoef, 1, b);
    let solved = linalg::least_squares(&q, &observations, None).map_err(|le| NumericError::Decomposition(le.to_string()))?;
    let a: Vec<f64> = (0..ncoef).map(|i| solved.get(i, 0).unwrap_or(0.0)).collect();

    let mut h = vec![0.0; taps];
    h[half] = a[0];
    for m in 1..=half {
        let val = a[m] / 2.0;
        h[half - m] = val;
        h[half + m] = val;
    }
    Ok(h)
}

/// FIR frequency response `H(e^{jω}) = sum_k b[k] * e^{-jkω}` -- simpler
/// than the IIR [`freqz`] case: no denominator polynomial at all, just a
/// direct evaluation of the taps on the unit circle. Same `n`-point,
/// DC-to-Nyquist-inclusive convention as [`freqz`].
pub fn fir_freqz(b: &[f64], n: usize) -> Vec<crate::Complex64> {
    let denom = (n.max(2) - 1) as f64;
    (0..n.max(1))
        .map(|k| {
            let omega = PI * k as f64 / denom;
            b.iter()
                .enumerate()
                .fold(crate::Complex64::real(0.0), |acc, (m, &bm)| acc.add(crate::Complex64::from_polar(bm, -omega * m as f64)))
        })
        .collect()
}

/// FIR group delay, sharing [`group_delay`]'s own unwrap-then-differentiate
/// machinery via `group_delay_from_response` rather than a separate
/// implementation -- the same general phase-derivative definition applies
/// regardless of representation. For a `fir1`-designed (symmetric, linear-
/// phase) filter this is exactly constant at `n/2` samples by construction;
/// this numeric path also works for a non-symmetric FIR, where it wouldn't be.
pub fn fir_group_delay(b: &[f64], n: usize) -> Vec<f64> {
    group_delay_from_response(&fir_freqz(b, n), n)
}

/// Causal FIR filtering: exactly `conv(x, b, Full)` truncated to `len(x)` --
/// full convolution's first `len(x)` samples are precisely the causal
/// direct-form output `y[n] = sum_{k=0}^{min(n,M)} b[k]*x[n-k]`, so this
/// reuses `transforms::conv`'s own direct/overlap-add/FFT dispatch for free
/// instead of a bespoke loop.
pub fn fir_filt(b: &[f64], x: &[f64]) -> Result<Vec<f64>, NumericError> {
    if x.is_empty() {
        return Ok(Vec::new());
    }
    let full = conv(x, b, ConvMode::Full)?;
    Ok(full[..x.len().min(full.len())].to_vec())
}

/// Zero-phase FIR filtering (forward, then backward), the same construction
/// as [`filtfilt`] applied to [`fir_filt`] instead of [`sosfilt`].
pub fn fir_filtfilt(b: &[f64], x: &[f64]) -> Result<Vec<f64>, NumericError> {
    let forward = fir_filt(b, x)?;
    let mut reversed: Vec<f64> = forward.into_iter().rev().collect();
    reversed = fir_filt(b, &reversed)?;
    reversed.reverse();
    Ok(reversed)
}

/// One sample through a direct-form FIR filter. `state` holds the last
/// `b.len()-1` inputs, most-recent-first -- simpler than [`BiquadState`]:
/// an FIR filter's whole "state" is a plain input delay line, no feedback
/// terms to track at all.
pub fn fir_step(b: &[f64], state: &mut Vec<f64>, x: f64) -> f64 {
    let mut y = b.first().copied().unwrap_or(0.0) * x;
    for (i, &bi) in b.iter().enumerate().skip(1) {
        y += bi * state.get(i - 1).copied().unwrap_or(0.0);
    }
    state.insert(0, x);
    state.truncate(b.len().saturating_sub(1));
    y
}

// ---- Poles, zeros, and stability (both IIR and FIR representations) ----

/// Roots of a real polynomial `c[0]*z^n + c[1]*z^(n-1) + ... + c[n] = 0` via
/// the companion-matrix eigenvalue method -- the standard, numerically
/// robust way to find polynomial roots, reusing `linalg::eig` rather than a
/// bespoke root-finder. `c[0]` must be nonzero.
pub fn polynomial_roots(c: &[f64]) -> Result<Vec<crate::Complex64>, NumericError> {
    let degree = c.len().saturating_sub(1);
    if degree == 0 {
        return Ok(Vec::new());
    }
    if c[0] == 0.0 {
        return Err(NumericError::Decomposition("polynomial_roots: leading coefficient is zero".to_string()));
    }
    let mut companion = Matrix::zeros(degree, degree);
    for i in 0..degree {
        let _ = companion.set(0, i, -c[i + 1] / c[0]);
    }
    for i in 1..degree {
        let _ = companion.set(i, i - 1, 1.0);
    }
    let result = linalg::eig(&companion).map_err(|err| NumericError::Decomposition(err.to_string()))?;
    Ok(result
        .values_re
        .iter()
        .zip(result.values_im.iter())
        .map(|(&re, &im)| crate::Complex64::new(re, im))
        .collect())
}

/// Roots of a real quadratic `a*z^2 + b*z + c = 0` -- a complex-conjugate
/// pair when the discriminant is negative, two real roots otherwise.
fn quadratic_roots(a: f64, b: f64, c: f64) -> [crate::Complex64; 2] {
    let disc = b * b - 4.0 * a * c;
    if disc >= 0.0 {
        let sq = disc.sqrt();
        [crate::Complex64::real((-b + sq) / (2.0 * a)), crate::Complex64::real((-b - sq) / (2.0 * a))]
    } else {
        let sq = (-disc).sqrt();
        let re = -b / (2.0 * a);
        let im = sq / (2.0 * a);
        [crate::Complex64::new(re, im), crate::Complex64::new(re, -im)]
    }
}

/// Poles of an SOS cascade: each section's `z^2 + a1*z + a2 = 0` roots
/// (implicit `a0 = 1`, so the quadratic's leading coefficient is always
/// exactly `1` -- never the degenerate `a=0` case), collected across every
/// section.
pub fn sos_poles(sos: &Sos) -> Vec<crate::Complex64> {
    sos.sections.iter().flat_map(|bq| quadratic_roots(1.0, bq.a1, bq.a2)).collect()
}

/// Zeros of an SOS cascade: each section's `b0*z^2 + b1*z + b2 = 0` roots.
pub fn sos_zeros(sos: &Sos) -> Vec<crate::Complex64> {
    sos.sections.iter().flat_map(|bq| quadratic_roots(bq.b0, bq.b1, bq.b2)).collect()
}

/// An FIR filter's poles are all at the origin -- `len(b)-1` of them, the
/// standard "poles at z=0" statement for a direct-form FIR viewed as a
/// proper rational function (`H(z) = z^-M * (b0*z^M + ... + bM)`,
/// `M = len(b)-1`; the `z^-M` factor needed to make it causal is exactly
/// `M` poles at the origin). No feedback anywhere, so there's nothing else
/// to compute.
pub fn fir_poles(b: &[f64]) -> Vec<crate::Complex64> {
    vec![crate::Complex64::real(0.0); b.len().saturating_sub(1)]
}

/// Zeros of an FIR filter: roots of `b0*z^M + b1*z^(M-1) + ... + bM = 0`
/// (the tap polynomial with the `z^-k` factors cleared to a plain positive-
/// power polynomial in `z`, i.e. exactly `b` itself read as descending-power
/// coefficients) via [`polynomial_roots`].
pub fn fir_zeros(b: &[f64]) -> Result<Vec<crate::Complex64>, NumericError> {
    polynomial_roots(b)
}

/// `true` iff every pole lies strictly inside the unit circle -- the
/// standard causal-LTI stability criterion. An FIR filter's poles (all at
/// the origin, from [`fir_poles`]) trivially satisfy this, so FIR stability
/// falls straight out of this one general definition rather than needing a
/// special case.
pub fn is_stable(poles: &[crate::Complex64]) -> bool {
    poles.iter().all(|p| p.magnitude() < 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    /// `H(e^jw)` evaluated directly from the SOS cascade — used to check
    /// the actual frequency response against known Butterworth properties
    /// rather than trusting the design pipeline blindly.
    fn magnitude_db(sos: &Sos, w: f64) -> f64 {
        // H(e^jw) with z^-1 = e^{-jw}.
        let ejw_inv = Complex::new(w.cos(), -w.sin());
        let mut h = real(1.0);
        for bq in &sos.sections {
            let num = real(bq.b0).add(ejw_inv.mul(real(bq.b1))).add(ejw_inv.mul(ejw_inv).mul(real(bq.b2)));
            let den = real(1.0).add(ejw_inv.mul(real(bq.a1))).add(ejw_inv.mul(ejw_inv).mul(real(bq.a2)));
            h = h.mul(num.div(den));
        }
        20.0 * (h.re * h.re + h.im * h.im).sqrt().max(1e-300).log10()
    }

    #[test]
    fn lowpass_matches_the_known_minus_3db_cutoff() {
        let fs = 1000.0;
        let fc = 100.0;
        let sos = butter(4, FilterKind::Low, &[fc], fs).unwrap();
        close(magnitude_db(&sos, 0.0), 0.0, 1e-6);
        close(magnitude_db(&sos, 2.0 * PI * fc / fs), -3.0103, 1e-3);
        assert!(magnitude_db(&sos, PI) < -100.0, "should be heavily attenuated at Nyquist");
    }

    #[test]
    fn highpass_matches_the_known_minus_3db_cutoff() {
        let fs = 1000.0;
        let fc = 100.0;
        let sos = butter(4, FilterKind::High, &[fc], fs).unwrap();
        assert!(magnitude_db(&sos, 0.0) < -100.0, "should be heavily attenuated at DC");
        close(magnitude_db(&sos, 2.0 * PI * fc / fs), -3.0103, 1e-3);
        close(magnitude_db(&sos, PI), 0.0, 1e-6);
    }

    #[test]
    fn bandpass_matches_known_edge_and_center_gains() {
        let fs = 1000.0;
        let (f1, f2) = (50.0, 150.0);
        let sos = butter(2, FilterKind::Band, &[f1, f2], fs).unwrap();
        assert!(magnitude_db(&sos, 0.0) < -50.0);
        close(magnitude_db(&sos, 2.0 * PI * f1 / fs), -3.0103, 1e-2);
        close(magnitude_db(&sos, 2.0 * PI * f2 / fs), -3.0103, 1e-2);
        close(magnitude_db(&sos, 2.0 * PI * (f1 * f2).sqrt() / fs), 0.0, 1e-3);
    }

    #[test]
    fn bandstop_matches_known_notch_depth() {
        let fs = 1000.0;
        let (f1, f2) = (45.0, 55.0);
        let sos = butter(2, FilterKind::Stop, &[f1, f2], fs).unwrap();
        close(magnitude_db(&sos, 0.0), 0.0, 1e-6);
        close(magnitude_db(&sos, PI), 0.0, 1e-6);
        assert!(magnitude_db(&sos, 2.0 * PI * (f1 * f2).sqrt() / fs) < -50.0, "the notch itself must be deep");
    }

    #[test]
    fn sosfilt_attenuates_a_tone_above_the_lowpass_cutoff() {
        let fs = 1000.0;
        let sos = butter(4, FilterKind::Low, &[50.0], fs).unwrap();
        let n = 500;
        let low_tone: Vec<f64> = (0..n).map(|i| (2.0 * PI * 10.0 * i as f64 / fs).sin()).collect();
        let high_tone: Vec<f64> = (0..n).map(|i| (2.0 * PI * 300.0 * i as f64 / fs).sin()).collect();
        let low_out = sosfilt(&sos, &low_tone);
        let high_out = sosfilt(&sos, &high_tone);
        let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
        // A unit-amplitude sine has RMS 1/sqrt(2) ~= 0.707; a passband
        // tone should keep close to that, a stopband tone should not.
        // Skip the initial transient before measuring either.
        assert!(rms(&low_out[200..]) > 0.6, "a passband tone should survive mostly intact, got rms={}", rms(&low_out[200..]));
        assert!(rms(&high_out[200..]) < 0.1, "a stopband tone should be strongly attenuated, got rms={}", rms(&high_out[200..]));
    }

    #[test]
    fn filtfilt_has_no_net_phase_shift_on_a_pure_tone() {
        let fs = 1000.0;
        let sos = butter(4, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 1000;
        let f = 20.0;
        let x: Vec<f64> = (0..n).map(|i| (2.0 * PI * f * i as f64 / fs).sin()).collect();
        let y = filtfilt(&sos, &x);
        // in the steady interior, filtfilt output should align with the
        // input at the same sample index (zero net phase shift) -- a
        // plain sosfilt would NOT have this property (it has a phase lag).
        let mid = n / 2;
        close(y[mid], x[mid], 0.05);
    }

    #[test]
    fn streaming_sos_step_matches_batch_sosfilt() {
        let fs = 1000.0;
        let sos = butter(3, FilterKind::Low, &[80.0], fs).unwrap();
        let n = 200;
        let x: Vec<f64> = (0..n).map(|i| (2.0 * PI * 30.0 * i as f64 / fs).sin() + 0.3 * (2.0 * PI * 300.0 * i as f64 / fs).sin()).collect();
        let batch = sosfilt(&sos, &x);
        let mut states = vec![BiquadState::default(); sos.sections.len()];
        let streamed: Vec<f64> = x.iter().map(|&v| sos_step(&sos, &mut states, v)).collect();
        for (a, b) in batch.iter().zip(&streamed) {
            close(*a, *b, 1e-12);
        }
    }

    #[test]
    fn butter_rejects_zero_order() {
        assert!(butter(0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    #[test]
    fn freqz_magnitude_matches_the_independent_magnitude_db_helper() {
        // Cross-checks `freqz` against this file's own pre-existing,
        // independently-written `magnitude_db` test helper (used by the
        // cutoff tests above) rather than only checking `freqz` against
        // itself.
        let fs = 1000.0;
        let sos = butter(4, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 257;
        let h = freqz(&sos, n);
        for (k, hk) in h.iter().enumerate() {
            let omega = PI * k as f64 / (n - 1) as f64;
            let expected_db = magnitude_db(&sos, omega);
            let got_db = 20.0 * hk.magnitude().max(1e-300).log10();
            close(got_db, expected_db, 1e-9);
        }
    }

    /// A single section with `H(z) = z^-1` exactly (`b0=0, b1=1, b2=0,
    /// a1=0, a2=0`) — a pure one-sample delay. Its frequency response has
    /// unit magnitude and phase `-ω` at every frequency by construction, so
    /// `freqz`/`group_delay` can be checked against exact closed-form
    /// values instead of only cross-checking against each other.
    fn pure_delay_sos(samples: usize) -> Sos {
        Sos {
            sections: (0..samples).map(|_| Biquad { b0: 0.0, b1: 1.0, b2: 0.0, a1: 0.0, a2: 0.0 }).collect(),
        }
    }

    #[test]
    fn freqz_of_a_pure_delay_has_unit_magnitude_and_linear_phase() {
        let sos = pure_delay_sos(1);
        let n = 65;
        let h = freqz(&sos, n);
        for (k, hk) in h.iter().enumerate() {
            let omega = PI * k as f64 / (n - 1) as f64;
            close(hk.magnitude(), 1.0, 1e-9);
            // phase wraps past +-pi, so only check it away from that wrap.
            if omega < PI - 1e-6 {
                close(hk.arg(), -omega, 1e-6);
            }
        }
    }

    #[test]
    fn group_delay_of_a_pure_delay_is_exactly_one_sample_everywhere() {
        let n = 65;
        let gd = group_delay(&pure_delay_sos(1), n);
        assert_eq!(gd.len(), n);
        for &d in &gd {
            close(d, 1.0, 1e-6);
        }
    }

    #[test]
    fn group_delay_is_additive_across_cascaded_delay_sections() {
        // Two cascaded unit-delay sections form a 2-sample delay -- group
        // delay must double, a real structural property, not a coincidence
        // of the single-section case above.
        let n = 65;
        let gd = group_delay(&pure_delay_sos(2), n);
        for &d in &gd {
            close(d, 2.0, 1e-6);
        }
    }

    // ---- fir1 / FIR analysis ----

    /// A minimal, independently-written Hamming window (`qu-interp`'s own
    /// `hamming` builtin isn't visible from `qu-core`) -- just enough to
    /// exercise `fir1` without depending on the interpreter crate.
    fn test_hamming(n: usize) -> Vec<f64> {
        if n == 1 {
            return vec![1.0];
        }
        (0..n).map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / (n - 1) as f64).cos()).collect()
    }

    #[test]
    fn fir1_lowpass_passes_dc_and_rejects_a_far_stopband_tone() {
        let fs = 1000.0;
        let n = 100;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[100.0], fs, &window).unwrap();
        let h = fir_freqz(&b, 513);
        // DC gain is normalized to exactly unity by construction.
        close(h[0].magnitude(), 1.0, 1e-9);
        // 300 Hz is 3x the 100 Hz cutoff -- comfortably in the stopband for
        // a 101-tap Hamming-windowed design.
        let idx_300 = (300.0 / (fs / 2.0) * 512.0).round() as usize;
        let db_300 = 20.0 * h[idx_300].magnitude().max(1e-300).log10();
        assert!(db_300 < -40.0, "expected strong stopband attenuation at 300Hz, got {db_300} dB");
    }

    #[test]
    fn fir1_highpass_rejects_dc_and_passes_nyquist() {
        let fs = 1000.0;
        let n = 100;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::High, &[100.0], fs, &window).unwrap();
        let h = fir_freqz(&b, 513);
        let db_dc = 20.0 * h[0].magnitude().max(1e-300).log10();
        assert!(db_dc < -40.0, "expected strong DC attenuation, got {db_dc} dB");
        close(h[512].magnitude(), 1.0, 1e-9); // Nyquist normalized to unity.
    }

    #[test]
    fn fir1_bandpass_passes_the_band_center_and_rejects_dc_and_nyquist() {
        let fs = 1000.0;
        let n = 100;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Band, &[150.0, 250.0], fs, &window).unwrap();
        let h = fir_freqz(&b, 513);
        let idx_center = (200.0 / (fs / 2.0) * 512.0).round() as usize;
        // The 513-point grid doesn't necessarily land exactly on 200 Hz --
        // the true passband peak is exact (checked structurally by
        // `fir1_taps_are_symmetric...`), so a small off-bin tolerance is
        // the rounding error, not a design flaw.
        close(h[idx_center].magnitude(), 1.0, 2e-3);
        let db_dc = 20.0 * h[0].magnitude().max(1e-300).log10();
        let db_nyq = 20.0 * h[512].magnitude().max(1e-300).log10();
        assert!(db_dc < -30.0, "expected DC well outside the band, got {db_dc} dB");
        assert!(db_nyq < -30.0, "expected Nyquist well outside the band, got {db_nyq} dB");
    }

    #[test]
    fn fir1_bandstop_notches_the_band_center_and_passes_dc_and_nyquist() {
        let fs = 1000.0;
        let n = 100;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Stop, &[150.0, 250.0], fs, &window).unwrap();
        let h = fir_freqz(&b, 513);
        // Normalization targets exactly DC (w0=0 for `Stop`), so DC is
        // exact; Nyquist is only approximately unity for a truncated,
        // windowed design (not the exact normalization frequency), hence
        // the looser tolerance there.
        close(h[0].magnitude(), 1.0, 1e-6);
        close(h[512].magnitude(), 1.0, 1e-2);
        let idx_center = (200.0 / (fs / 2.0) * 512.0).round() as usize;
        // Same off-bin rounding note as the bandpass test above -- the
        // notch center is exact, this grid point is merely near it.
        let db_center = 20.0 * h[idx_center].magnitude().max(1e-300).log10();
        assert!(db_center < -30.0, "expected the notch itself to be deep, got {db_center} dB");
    }

    #[test]
    fn fir1_rejects_a_mismatched_window_length() {
        let short_window = test_hamming(10); // needs n+1 = 51
        assert!(fir1(50, FilterKind::Low, &[100.0], 1000.0, &short_window).is_err());
    }

    #[test]
    fn fir1_taps_are_symmetric_confirming_a_linear_phase_design() {
        let n = 40;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[100.0], 1000.0, &window).unwrap();
        for k in 0..=n {
            close(b[k], b[n - k], 1e-12);
        }
    }

    #[test]
    fn fir_group_delay_of_a_fir1_lowpass_matches_the_theoretical_n_over_2_in_the_deep_passband() {
        // Generalized-linear-phase FIR: the true phase response can jump by
        // exactly pi at a magnitude null (a genuine discontinuity, not a
        // wraparound artifact the unwrap step is meant to remove), which can
        // perturb the *numeric* unwrap-then-differentiate group delay right
        // at those nulls. Deep in the passband, far from any stopband
        // ripple null, there's no such crossing, so the numeric estimate
        // should match the theoretical constant `n/2` tightly there.
        let fs = 1000.0;
        let n = 40;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[100.0], fs, &window).unwrap();
        let n_freqz = 513;
        let gd = fir_group_delay(&b, n_freqz);
        let domega = PI / (n_freqz - 1) as f64;
        let wc = 2.0 * PI * 100.0 / fs;
        let theoretical = n as f64 / 2.0;
        for (k, &d) in gd.iter().enumerate() {
            let omega = domega * k as f64;
            if omega < wc * 0.5 {
                close(d, theoretical, 0.05);
            }
        }
    }

    #[test]
    fn fir_filt_matches_fir_step_streaming() {
        let fs = 1000.0;
        let n = 60;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[80.0], fs, &window).unwrap();
        let m = 200;
        let x: Vec<f64> = (0..m).map(|i| (2.0 * PI * 30.0 * i as f64 / fs).sin() + 0.3 * (2.0 * PI * 300.0 * i as f64 / fs).sin()).collect();
        let batch = fir_filt(&b, &x).unwrap();
        let mut state = Vec::new();
        let streamed: Vec<f64> = x.iter().map(|&v| fir_step(&b, &mut state, v)).collect();
        for (a, s) in batch.iter().zip(&streamed) {
            close(*a, *s, 1e-9);
        }
    }

    #[test]
    fn fir_filtfilt_has_no_net_phase_shift_on_a_pure_tone() {
        let fs = 1000.0;
        let n = 100;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[100.0], fs, &window).unwrap();
        let m = 1000;
        let f = 20.0;
        let x: Vec<f64> = (0..m).map(|i| (2.0 * PI * f * i as f64 / fs).sin()).collect();
        let y = fir_filtfilt(&b, &x).unwrap();
        let mid = m / 2;
        close(y[mid], x[mid], 0.05);
    }

    // ---- poles, zeros, stability ----

    #[test]
    fn polynomial_roots_matches_a_known_quadratic() {
        // z^2 - 3z + 2 = (z-1)(z-2), roots 1 and 2 exactly.
        let mut roots = polynomial_roots(&[1.0, -3.0, 2.0]).unwrap();
        roots.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap());
        close(roots[0].re, 1.0, 1e-9);
        close(roots[0].im, 0.0, 1e-9);
        close(roots[1].re, 2.0, 1e-9);
        close(roots[1].im, 0.0, 1e-9);
    }

    #[test]
    fn sos_zeros_of_a_hand_built_biquad_matches_the_same_known_quadratic() {
        // Same z^2 - 3z + 2 polynomial, this time as a biquad's numerator.
        let sos = Sos { sections: vec![Biquad { b0: 1.0, b1: -3.0, b2: 2.0, a1: 0.0, a2: 0.0 }] };
        let mut zeros = sos_zeros(&sos);
        zeros.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap());
        close(zeros[0].re, 1.0, 1e-9);
        close(zeros[1].re, 2.0, 1e-9);
    }

    #[test]
    fn fir_zeros_of_the_same_taps_matches_the_sos_path() {
        let mut zeros = fir_zeros(&[1.0, -3.0, 2.0]).unwrap();
        zeros.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap());
        close(zeros[0].re, 1.0, 1e-9);
        close(zeros[1].re, 2.0, 1e-9);
    }

    #[test]
    fn a_stable_butterworth_design_has_every_pole_strictly_inside_the_unit_circle() {
        let fs = 1000.0;
        let sos = butter(4, FilterKind::Low, &[100.0], fs).unwrap();
        let poles = sos_poles(&sos);
        assert_eq!(poles.len(), 4); // order 4 => 2 sections => 4 poles.
        for p in &poles {
            assert!(p.magnitude() < 1.0, "pole {:?} should be inside the unit circle", p);
        }
        assert!(is_stable(&poles));
    }

    #[test]
    fn fir_poles_are_all_at_the_origin_and_an_fir_filter_is_always_stable() {
        let fs = 1000.0;
        let n = 30;
        let window = test_hamming(n + 1);
        let b = fir1(n, FilterKind::Low, &[100.0], fs, &window).unwrap();
        let poles = fir_poles(&b);
        assert_eq!(poles.len(), n); // n+1 taps => n poles at the origin.
        for p in &poles {
            close(p.re, 0.0, 1e-15);
            close(p.im, 0.0, 1e-15);
        }
        assert!(is_stable(&poles));
    }

    #[test]
    fn an_unstable_pole_outside_the_unit_circle_is_reported_unstable() {
        // A hand-built pole at z=1.5 (outside the unit circle): a biquad
        // with a1=-1.5 (single real pole, other section coefficient zeroed
        // by using an effectively first-order section: a2=0 => the second
        // "pole" lands at z=0, which is fine, the |z|=1.5 one still fails).
        let sos = Sos { sections: vec![Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: -1.5, a2: 0.0 }] };
        let poles = sos_poles(&sos);
        assert!(!is_stable(&poles), "a pole outside the unit circle must be reported unstable");
    }

    // ---- ellip (elliptic/Cauer) design ----
    //
    // An equiripple design isn't checked the way `butter`'s tests check a
    // single -3dB-at-cutoff point above -- the whole point of an elliptic
    // filter is that the passband oscillates between 0dB and `-ripple_db`
    // dB, and the stopband oscillates below `-atten_db` dB, across the
    // *entire* band, not just at one frequency. So these tests sample
    // `freqz` densely across each band and check every sample against the
    // bound, matching the property described in this session's own task
    // (and cross-checked, before being trusted, against `scipy.signal.
    // ellip`'s actual frequency response in a standalone script -- see
    // `ellip_analog_prototype`'s doc comment).

    #[test]
    fn ellip_lowpass_meets_its_ripple_and_attenuation_spec_across_both_bands() {
        let fs = 1000.0;
        let (ripple_db, atten_db) = (1.0, 60.0);
        let sos = ellip(6, ripple_db, atten_db, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        let mut sampled_passband = false;
        let mut sampled_stopband = false;
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq <= 90.0 {
                sampled_passband = true;
                assert!(db <= 0.05, "passband must not exceed 0dB (got {db} dB at {freq} Hz)");
                assert!(db >= -ripple_db - 0.05, "passband ripple must stay within {ripple_db}dB of 0dB (got {db} dB at {freq} Hz)");
            } else if freq >= 150.0 {
                sampled_stopband = true;
                assert!(db <= -atten_db + 0.5, "stopband must reach {atten_db}dB down (got {db} dB at {freq} Hz)");
            }
        }
        assert!(sampled_passband && sampled_stopband, "test grid should have covered both bands");
    }

    #[test]
    fn ellip_highpass_meets_its_ripple_and_attenuation_spec() {
        let fs = 1000.0;
        let (ripple_db, atten_db) = (0.5, 50.0);
        let sos = ellip(5, ripple_db, atten_db, FilterKind::High, &[300.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq >= 350.0 {
                assert!(db <= 0.05 && db >= -ripple_db - 0.05, "passband ripple out of bounds: {db} dB at {freq} Hz");
            } else if freq <= 200.0 {
                assert!(db <= -atten_db + 0.5, "stopband attenuation not met: {db} dB at {freq} Hz");
            }
        }
    }

    #[test]
    fn ellip_bandpass_passes_the_center_and_attenuates_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = ellip(4, 1.0, 40.0, FilterKind::Band, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) >= -1.0 - 0.1, "band center should be in the passband, got {} dB", db_at(200.0));
        assert!(db_at(1.0) <= -40.0 + 0.5, "DC should be deep in the stopband, got {} dB", db_at(1.0));
        assert!(db_at(499.0) <= -40.0 + 0.5, "Nyquist should be deep in the stopband, got {} dB", db_at(499.0));
    }

    #[test]
    fn ellip_bandstop_notches_the_center_and_passes_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = ellip(4, 1.0, 40.0, FilterKind::Stop, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) <= -40.0 + 0.5, "band center should be notched out, got {} dB", db_at(200.0));
        assert!(db_at(1.0) >= -1.0 - 0.1, "DC should be passed, got {} dB", db_at(1.0));
        assert!(db_at(499.0) >= -1.0 - 0.1, "Nyquist should be passed, got {} dB", db_at(499.0));
    }

    #[test]
    fn ellip_design_is_stable_every_pole_strictly_inside_the_unit_circle() {
        // Even order deliberately: `zpk_to_sos` represents a lone real
        // pole (the odd-order case) as a first-order section (`a2 = 0`),
        // and `sos_poles`'s per-section `quadratic_roots(1, a1, 0)` then
        // reports that section's implicit second root at `z=0` as if it
        // were a genuine pole -- harmless for stability (`|0| < 1`) but an
        // over-count vs. the filter's true order, a pre-existing quirk of
        // `sos_poles` this test isn't the place to fix. An even order has
        // no first-order section, so the count is exact here.
        let fs = 1000.0;
        let sos = ellip(8, 1.0, 50.0, FilterKind::Low, &[100.0], fs).unwrap();
        let poles = sos_poles(&sos);
        assert_eq!(poles.len(), 8);
        assert!(is_stable(&poles), "an elliptic design must be stable");
    }

    #[test]
    fn ellip_order_one_degenerates_to_a_single_real_pole_governed_by_ripple_alone() {
        // No complex-conjugate pole pair exists yet at order 1, so there's
        // no equiripple behavior to check -- only that it's a sane, stable,
        // unity-ish-DC-gain lowpass, matching Chebyshev I's own order-1
        // special case.
        let fs = 1000.0;
        let sos = ellip(1, 1.0, 40.0, FilterKind::Low, &[100.0], fs).unwrap();
        let h = freqz(&sos, 512);
        close(h[0].magnitude(), 1.0, 1e-6);
        assert!(h[511].magnitude() < h[0].magnitude(), "should still roll off toward Nyquist");
        assert!(is_stable(&sos_poles(&sos)));
    }

    #[test]
    fn ellip_rejects_zero_order() {
        assert!(ellip(0, 1.0, 40.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    #[test]
    fn ellip_rejects_a_ripple_not_small_relative_to_the_attenuation() {
        // rp >= rs makes the degree equation's discrimination parameter
        // `ck1_sq = eps^2/eps1^2` land at or past 1 -- not a meaningful
        // filter (asking for more passband ripple than stopband
        // attenuation), and must fail cleanly rather than produce NaNs.
        assert!(ellip(4, 40.0, 1.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    // ---- cheby1 (Chebyshev Type I) ----

    #[test]
    fn cheby1_lowpass_meets_its_ripple_spec_and_rolls_off_monotonically() {
        let fs = 1000.0;
        let ripple_db = 1.0;
        let sos = cheby1(6, ripple_db, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        let mut sampled_passband = false;
        let mut sampled_stopband = false;
        let mut prev_stopband_db: Option<f64> = None;
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq <= 100.0 {
                sampled_passband = true;
                assert!(db <= 0.05, "passband must not exceed 0dB (got {db} dB at {freq} Hz)");
                assert!(db >= -ripple_db - 0.05, "passband ripple must stay within {ripple_db}dB of 0dB (got {db} dB at {freq} Hz)");
            } else if freq >= 150.0 {
                sampled_stopband = true;
                // Chebyshev I's stopband is monotonically decreasing (no
                // ripple), unlike its passband -- each sample should be no
                // louder than the previous one.
                if let Some(prev) = prev_stopband_db {
                    assert!(db <= prev + 1e-6, "stopband must decay monotonically, got {db} dB after {prev} dB at {freq} Hz");
                }
                prev_stopband_db = Some(db);
            }
        }
        assert!(sampled_passband && sampled_stopband, "test grid should have covered both bands");
        assert!(prev_stopband_db.unwrap() < -60.0, "should be heavily attenuated well past cutoff, got {} dB", prev_stopband_db.unwrap());
    }

    #[test]
    fn cheby1_highpass_meets_its_ripple_spec() {
        let fs = 1000.0;
        let ripple_db = 0.5;
        let sos = cheby1(5, ripple_db, FilterKind::High, &[300.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq >= 300.0 {
                assert!(db <= 0.05 && db >= -ripple_db - 0.05, "passband ripple out of bounds: {db} dB at {freq} Hz");
            }
        }
        assert!(20.0 * h[0].magnitude().max(1e-300).log10() < -40.0, "DC should be well into the stopband");
    }

    #[test]
    fn cheby1_bandpass_passes_the_center_and_attenuates_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = cheby1(4, 1.0, FilterKind::Band, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) >= -1.0 - 0.1, "band center should be in the passband, got {} dB", db_at(200.0));
        assert!(db_at(1.0) <= -30.0, "DC should be well into the stopband, got {} dB", db_at(1.0));
        assert!(db_at(499.0) <= -30.0, "Nyquist should be well into the stopband, got {} dB", db_at(499.0));
    }

    #[test]
    fn cheby1_bandstop_notches_the_center_and_passes_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = cheby1(4, 1.0, FilterKind::Stop, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) <= -30.0, "band center should be notched out, got {} dB", db_at(200.0));
        assert!(db_at(1.0) >= -1.0 - 0.1, "DC should be passed, got {} dB", db_at(1.0));
        assert!(db_at(499.0) >= -1.0 - 0.1, "Nyquist should be passed, got {} dB", db_at(499.0));
    }

    #[test]
    fn cheby1_design_is_stable_every_pole_strictly_inside_the_unit_circle() {
        let fs = 1000.0;
        let sos = cheby1(8, 1.0, FilterKind::Low, &[100.0], fs).unwrap();
        let poles = sos_poles(&sos);
        assert_eq!(poles.len(), 8);
        assert!(is_stable(&poles), "a Chebyshev I design must be stable");
    }

    #[test]
    fn cheby1_order_one_degenerates_to_a_single_real_pole_governed_by_ripple_alone() {
        let fs = 1000.0;
        let sos = cheby1(1, 1.0, FilterKind::Low, &[100.0], fs).unwrap();
        let h = freqz(&sos, 512);
        close(h[0].magnitude(), 1.0, 1e-6);
        assert!(h[511].magnitude() < h[0].magnitude(), "should still roll off toward Nyquist");
        assert!(is_stable(&sos_poles(&sos)));
    }

    #[test]
    fn cheby1_rejects_zero_order() {
        assert!(cheby1(0, 1.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    #[test]
    fn cheby1_rejects_a_nonpositive_ripple() {
        assert!(cheby1(4, 0.0, FilterKind::Low, &[100.0], 1000.0).is_err());
        assert!(cheby1(4, -1.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    // ---- cheby2 (Chebyshev Type II / inverse Chebyshev) ----

    #[test]
    fn cheby2_lowpass_meets_its_attenuation_spec_and_stays_flat_in_the_passband() {
        let fs = 1000.0;
        let atten_db = 60.0;
        let sos = cheby2(6, atten_db, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        let mut sampled_passband = false;
        let mut sampled_stopband = false;
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq <= 100.0 {
                sampled_passband = true;
                // Type II is monotonic (flat, no ripple) in the passband,
                // unlike Type I -- allow only a small numeric margin above
                // 0dB across the whole transition-band-inclusive range up
                // to the cutoff, and additionally check it stays close to
                // 0dB well inside the passband (not just "not blown up") --
                // the transition band itself (roughly 40-100Hz here) is
                // expected to roll off well before the stopband edge, so
                // the tight -3dB check only applies deep in the passband.
                assert!(db <= 0.05, "passband must not exceed 0dB (got {db} dB at {freq} Hz)");
                if freq <= 40.0 {
                    assert!(db >= -3.0, "flat passband should stay close to 0dB (got {db} dB at {freq} Hz)");
                }
            } else if freq >= 100.0 {
                sampled_stopband = true;
                assert!(db <= -atten_db + 0.5, "stopband must reach {atten_db}dB down starting at the edge (got {db} dB at {freq} Hz)");
            }
        }
        assert!(sampled_passband && sampled_stopband, "test grid should have covered both bands");
    }

    #[test]
    fn cheby2_stopband_is_genuinely_equiripple_not_monotonic() {
        // The actual defining property of Type II vs Type I/Butterworth:
        // the stopband must ripple back up toward (but never past)
        // -atten_db repeatedly, rather than decaying monotonically deeper
        // and deeper (the way cheby1's own stopband and butter's own
        // stopband both do). Detect this by counting local minima in the
        // sampled stopband magnitude curve -- more than one confirms
        // genuine ripple, not just numerical noise on a monotone curve.
        let fs = 1000.0;
        let atten_db = 40.0;
        let sos = cheby2(8, atten_db, FilterKind::Low, &[100.0], fs).unwrap();
        let n = 4001;
        let h = freqz(&sos, n);
        let stopband_db: Vec<f64> = (0..n)
            .filter_map(|k| {
                let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
                if freq >= 100.0 {
                    Some(20.0 * h[k].magnitude().max(1e-300).log10())
                } else {
                    None
                }
            })
            .collect();
        for &db in &stopband_db {
            assert!(db <= -atten_db + 0.5, "stopband must never exceed {atten_db}dB down, got {db} dB");
        }
        let mut local_minima = 0;
        for i in 1..stopband_db.len() - 1 {
            if stopband_db[i] < stopband_db[i - 1] && stopband_db[i] < stopband_db[i + 1] {
                local_minima += 1;
            }
        }
        assert!(local_minima >= 2, "an order-8 Type II stopband should show multiple ripple dips, found {local_minima}");
    }

    #[test]
    fn cheby2_highpass_meets_its_attenuation_spec() {
        let fs = 1000.0;
        let atten_db = 50.0;
        let sos = cheby2(5, atten_db, FilterKind::High, &[300.0], fs).unwrap();
        let n = 2001;
        let h = freqz(&sos, n);
        for (k, hk) in h.iter().enumerate() {
            let freq = k as f64 / (n - 1) as f64 * fs / 2.0;
            let db = 20.0 * hk.magnitude().max(1e-300).log10();
            if freq <= 300.0 {
                assert!(db <= -atten_db + 0.5, "stopband attenuation not met: {db} dB at {freq} Hz");
            }
        }
        close(h[n - 1].magnitude(), 1.0, 1e-6);
    }

    #[test]
    fn cheby2_bandpass_passes_the_center_and_attenuates_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = cheby2(4, 40.0, FilterKind::Band, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) >= -3.0, "band center should be in the passband, got {} dB", db_at(200.0));
        assert!(db_at(1.0) <= -39.5, "DC should be deep in the stopband, got {} dB", db_at(1.0));
        assert!(db_at(499.0) <= -39.5, "Nyquist should be deep in the stopband, got {} dB", db_at(499.0));
    }

    #[test]
    fn cheby2_bandstop_notches_the_center_and_passes_dc_and_nyquist() {
        let fs = 1000.0;
        let sos = cheby2(4, 40.0, FilterKind::Stop, &[150.0, 250.0], fs).unwrap();
        let h = freqz(&sos, 2001);
        let db_at = |freq: f64| -> f64 {
            let k = (freq / (fs / 2.0) * 2000.0).round() as usize;
            20.0 * h[k].magnitude().max(1e-300).log10()
        };
        assert!(db_at(200.0) <= -39.5, "band center should be notched out, got {} dB", db_at(200.0));
        assert!(db_at(1.0) >= -3.0, "DC should be passed, got {} dB", db_at(1.0));
        assert!(db_at(499.0) >= -3.0, "Nyquist should be passed, got {} dB", db_at(499.0));
    }

    #[test]
    fn cheby2_design_is_stable_every_pole_strictly_inside_the_unit_circle() {
        let fs = 1000.0;
        let sos = cheby2(8, 50.0, FilterKind::Low, &[100.0], fs).unwrap();
        let poles = sos_poles(&sos);
        assert_eq!(poles.len(), 8);
        assert!(is_stable(&poles), "a Chebyshev II design must be stable");
    }

    #[test]
    fn cheby2_odd_order_has_one_fewer_finite_zero_than_pole() {
        // The structural property flagged as the most likely to exercise
        // `zpk_to_sos`'s pole/zero pairing bug `ellip` needed fixed: an
        // odd-order Type II prototype has a lone real pole with no finite-
        // zero partner (`n-1` zeros vs `n` poles), the same shape as
        // `ellip`'s own odd-order prototype. Checked directly on the
        // analog prototype (not through `Sos`/`sos_poles`, which reports a
        // first-order section's implicit second root at `z=0` as if it
        // were a genuine pole -- a pre-existing `sos_poles` quirk, not
        // what this test is checking).
        let proto = cheby2_analog_prototype(5, 40.0).unwrap();
        assert_eq!(proto.poles.len(), 5);
        assert_eq!(proto.zeros.len(), 4);
        // Exactly one pole should be (numerically) real -- the one with no
        // finite-zero partner.
        let real_poles = proto.poles.iter().filter(|p| p.im.abs() < 1e-9).count();
        assert_eq!(real_poles, 1);
        // And the whole design built from this prototype must still end up
        // stable end-to-end, through the actual `zpk_to_sos` pairing.
        let fs = 1000.0;
        let sos = cheby2(5, 40.0, FilterKind::Low, &[100.0], fs).unwrap();
        assert!(is_stable(&sos_poles(&sos)));
    }

    #[test]
    fn cheby2_order_one_has_no_finite_zeros() {
        let fs = 1000.0;
        let sos = cheby2(1, 40.0, FilterKind::Low, &[100.0], fs).unwrap();
        let h = freqz(&sos, 512);
        close(h[0].magnitude(), 1.0, 1e-6);
        assert!(is_stable(&sos_poles(&sos)));
    }

    #[test]
    fn cheby2_rejects_zero_order() {
        assert!(cheby2(0, 40.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    #[test]
    fn cheby2_rejects_a_nonpositive_attenuation() {
        assert!(cheby2(4, 0.0, FilterKind::Low, &[100.0], 1000.0).is_err());
        assert!(cheby2(4, -1.0, FilterKind::Low, &[100.0], 1000.0).is_err());
    }

    // ---- firls (least-squares FIR design) ----

    #[test]
    fn firls_lowpass_approximates_the_desired_piecewise_response() {
        let n = 60;
        let b = firls(n, &[0.0, 0.3, 0.4, 1.0], &[1.0, 1.0, 0.0, 0.0], None).unwrap();
        assert_eq!(b.len(), n + 1);
        let h = fir_freqz(&b, 513);
        for (k, hk) in h.iter().enumerate() {
            let f = k as f64 / 512.0;
            if f <= 0.25 {
                close(hk.magnitude(), 1.0, 0.05);
            } else if f >= 0.45 {
                assert!(hk.magnitude() < 0.05, "expected a near-zero stopband gain at f={f}, got {}", hk.magnitude());
            }
        }
    }

    #[test]
    fn firls_taps_are_symmetric_confirming_a_linear_phase_design() {
        let n = 40;
        let b = firls(n, &[0.0, 0.5, 0.6, 1.0], &[1.0, 1.0, 0.0, 0.0], None).unwrap();
        for i in 0..=n {
            close(b[i], b[n - i], 1e-9);
        }
    }

    #[test]
    fn firls_upweighting_a_band_reduces_its_own_worst_case_error() {
        // The defining, checkable behavior of *weighted* least squares:
        // pushing more weight onto the stopband should make its own
        // worst-case error strictly smaller than the equal-weight design,
        // not just "produce a different-but-equally-valid filter".
        let n = 30;
        let bands = [0.0, 0.3, 0.4, 1.0];
        let desired = [1.0, 1.0, 0.0, 0.0];
        let b_equal = firls(n, &bands, &desired, None).unwrap();
        let b_weighted = firls(n, &bands, &desired, Some(&[1.0, 20.0])).unwrap();
        let stopband_worst_case = |b: &[f64]| -> f64 {
            fir_freqz(b, 513)
                .iter()
                .enumerate()
                .filter(|(k, _)| *k as f64 / 512.0 >= 0.45)
                .map(|(_, hk)| hk.magnitude())
                .fold(0.0, f64::max)
        };
        assert!(
            stopband_worst_case(&b_weighted) < stopband_worst_case(&b_equal),
            "upweighting the stopband should reduce its worst-case error"
        );
    }

    #[test]
    fn firls_rejects_an_odd_order() {
        assert!(firls(31, &[0.0, 0.5, 0.6, 1.0], &[1.0, 1.0, 0.0, 0.0], None).is_err());
    }

    #[test]
    fn firls_rejects_a_mismatched_weights_length() {
        assert!(firls(30, &[0.0, 0.5, 0.6, 1.0], &[1.0, 1.0, 0.0, 0.0], Some(&[1.0, 2.0, 3.0])).is_err());
    }

    #[test]
    fn firls_rejects_mismatched_band_and_desired_lengths() {
        assert!(firls(30, &[0.0, 0.5, 0.6, 1.0], &[1.0, 1.0, 0.0], None).is_err());
    }

    /// An independent, deliberately different-looking re-derivation of the
    /// same minimization problem `firls` solves, kept here purely as an
    /// accuracy reference for `firls_matches_the_reference_grid_based_
    /// solution` below. Rather than the closed-form integrals the
    /// production code now uses, this discretizes each band on a dense
    /// frequency grid and approximates the weighted continuous integral
    /// with the (quadratically convergent) **trapezoidal rule** turned into
    /// an ordinary sum-of-squares: each interior sample's row/target is
    /// scaled by `sqrt(weight * grid_spacing)`, each of the two endpoint
    /// samples by half that (the standard trapezoidal endpoint weight),
    /// and the resulting overdetermined system solved with the same
    /// `linalg::least_squares` pseudo-inverse solver `firls` itself calls.
    /// `O(1/N^2)` convergence means a modest grid (a few thousand points
    /// per band) already lands within `1e-9`-ish of the exact continuous
    /// answer, tight enough to catch a real algorithmic error in the new
    /// closed-form path without needing an impractically large grid.
    fn firls_reference_grid_based(n: usize, freq_bands: &[f64], desired: &[f64], weights: Option<&[f64]>) -> Vec<f64> {
        let n_bands = freq_bands.len() / 2;
        let weight_vec: Vec<f64> = match weights {
            Some(w) => w.to_vec(),
            None => vec![1.0; n_bands],
        };
        let taps = n + 1;
        let half = n / 2;
        let ncoef = half + 1;
        // Trapezoidal error scales like `(band_width)^3 * m_max^2 / N^2`
        // (`f'' ~ -m^2*cos(mw)` for the highest-frequency cosine term in
        // play), so the point count is tied to `taps` (`~ m_max`) rather
        // than fixed, keeping the reference's own discretization error
        // comfortably below the comparison tolerance across every order
        // this test exercises, not just the smallest one.
        let points_per_band = (40 * taps).max(2000);

        let mut rows: Vec<Vec<f64>> = Vec::new();
        let mut target: Vec<f64> = Vec::new();
        for band in 0..n_bands {
            let (f1, f2) = (freq_bands[2 * band], freq_bands[2 * band + 1]);
            let (d1, d2) = (desired[2 * band], desired[2 * band + 1]);
            let weight = weight_vec[band];
            let (w1, w2) = (PI * f1, PI * f2);
            if (w2 - w1).abs() < 1e-12 {
                let scale = weight.max(0.0).sqrt();
                rows.push((0..ncoef).map(|m| scale * (m as f64 * w1).cos()).collect());
                target.push(scale * d1);
                continue;
            }
            let domega = (w2 - w1) / (points_per_band as f64 - 1.0);
            for k in 0..points_per_band {
                let mut trap_weight = domega;
                if k == 0 || k == points_per_band - 1 {
                    trap_weight *= 0.5;
                }
                let scale = (weight.max(0.0) * trap_weight).sqrt();
                let omega = w1 + k as f64 * domega;
                let t = (omega - w1) / (w2 - w1);
                let dsamp = d1 + t * (d2 - d1);
                rows.push((0..ncoef).map(|m| scale * (m as f64 * omega).cos()).collect());
                target.push(scale * dsamp);
            }
        }
        let design = Matrix::from_rows(&rows).unwrap();
        let observations = Matrix::from_col_major(target.len(), 1, target);
        let solved = linalg::least_squares(&design, &observations, None).unwrap();
        let a: Vec<f64> = (0..ncoef).map(|i| solved.get(i, 0).unwrap_or(0.0)).collect();
        let mut h = vec![0.0; taps];
        h[half] = a[0];
        for m in 1..=half {
            let val = a[m] / 2.0;
            h[half - m] = val;
            h[half + m] = val;
        }
        h
    }

    #[test]
    fn firls_matches_the_reference_grid_based_solution() {
        let cases: Vec<(usize, Vec<f64>, Vec<f64>, Option<Vec<f64>>)> = vec![
            (8, vec![0.0, 0.3, 0.4, 1.0], vec![1.0, 1.0, 0.0, 0.0], None),
            (16, vec![0.0, 0.3, 0.4, 1.0], vec![1.0, 1.0, 0.0, 0.0], None),
            (64, vec![0.0, 0.3, 0.4, 1.0], vec![1.0, 1.0, 0.0, 0.0], None),
            (64, vec![0.0, 0.5, 0.6, 1.0], vec![1.0, 1.0, 0.0, 0.0], Some(vec![1.0, 20.0])),
            (64, vec![0.0, 0.2, 0.3, 0.6, 0.7, 1.0], vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0], None),
            (128, vec![0.0, 1.0], vec![0.0, 1.0], None), // full-band linear ramp, single band
            (256, vec![0.0, 0.08, 0.12, 1.0], vec![1.0, 1.0, 0.0, 0.0], None),
        ];
        for (n, bands, desired, weights) in cases {
            let got = firls(n, &bands, &desired, weights.as_deref()).unwrap();
            let want = firls_reference_grid_based(n, &bands, &desired, weights.as_deref());
            assert_eq!(got.len(), want.len());
            let max_abs_err = got.iter().zip(&want).map(|(g, w)| (g - w).abs()).fold(0.0, f64::max);
            assert!(
                max_abs_err < 5e-6,
                "n={n}, bands={bands:?}: max coefficient error {max_abs_err:e} exceeds tolerance\ngot:  {got:?}\nwant: {want:?}"
            );
        }
    }
}
