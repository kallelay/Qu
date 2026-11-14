//! Filter-design helpers that sit beside [`crate::filter`]:
//!
//! - minimum-order estimators for the classical IIR families
//!   ([`iir_order`]: `buttord`/`cheb1ord`/`cheb2ord`/`ellipord`),
//! - Kaiser's FIR length estimate ([`kaiserord`]),
//! - Parks-McClellan equiripple FIR design ([`remez`]),
//! - the window functions that are not plain raised cosines
//!   ([`general_cosine_window`], [`bartlett_window`], [`tukey_window`],
//!   [`gaussian_window`]).
//!
//! Errors are plain messages (`Result<_, String>`): everything that can go
//! wrong here is a specification the caller wrote, and the message has to
//! name it.

use crate::filter::{ellip_k, ellip_k_comp, FilterKind};
use std::f64::consts::PI;

// ---------------------------------------------------------------- IIR order

/// Which classical IIR family an order estimate is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IirFamily {
    Butter,
    Cheby1,
    Cheby2,
    Ellip,
}

/// The result of an order estimate: the lowest order meeting the
/// specification, the band shape read from the edges, and the frequency (Hz,
/// one value for low/high, two for band/stop) to hand the matching design
/// function together with that order.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderEstimate {
    pub order: usize,
    pub kind: FilterKind,
    pub wn: Vec<f64>,
}

/// The order a family needs, as a real number, for a lowpass prototype whose
/// passband ends at 1 and whose stopband starts at `nat` (> 1).
///
/// With `gp = 10^(rp/10) - 1` and `gs = 10^(rs/10) - 1`:
/// - Butterworth: `log10(gs/gp) / (2 log10(nat))`
/// - Chebyshev I and II: `acosh(sqrt(gs/gp)) / acosh(nat)`
/// - elliptic: `K(k) K'(k1) / (K'(k) K(k1))`, `k = 1/nat`, `k1 = sqrt(gp/gs)`
///   (the degree equation).
fn real_order(family: IirFamily, nat: f64, rp: f64, rs: f64) -> f64 {
    let gp = 10f64.powf(0.1 * rp) - 1.0;
    let gs = 10f64.powf(0.1 * rs) - 1.0;
    match family {
        IirFamily::Butter => (gs / gp).log10() / (2.0 * nat.log10()),
        IirFamily::Cheby1 | IirFamily::Cheby2 => (gs / gp).sqrt().acosh() / nat.acosh(),
        IirFamily::Ellip => {
            let m0 = 1.0 / (nat * nat);
            let m1 = gp / gs;
            ellip_k(m0) * ellip_k_comp(m1) / (ellip_k_comp(m0) * ellip_k(m1))
        }
    }
}

/// Stopband edge of the lowpass prototype for a bandstop specification with
/// (prewarped) passband edges `p` and stopband edges `s`.
fn bandstop_nat(p: [f64; 2], s: [f64; 2]) -> f64 {
    let one = |si: f64| (si * (p[0] - p[1]) / (si * si - p[0] * p[1])).abs();
    one(s[0]).min(one(s[1]))
}

/// Golden-section minimum of `f` on `[a, b]`.
fn golden_min(mut a: f64, mut b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let mut c = b - g * (b - a);
    let mut d = a + g * (b - a);
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..80 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    let m = 0.5 * (a + b);
    // The interval ends are legal answers and the objective is often
    // monotonic; never return something worse than an end.
    let (fa, fb, fm) = (f(a), f(b), f(m));
    if fa <= fm && fa <= fb {
        a
    } else if fb <= fm {
        b
    } else {
        m
    }
}

/// Lowest order of a digital `family` filter that loses at most `rp` dB over
/// the passband and at least `rs` dB over the stopband, and the frequency to
/// design it with (MATLAB's/SciPy's `buttord`, `cheb1ord`, `cheb2ord`,
/// `ellipord`).
///
/// `wp`/`ws` are the passband and stopband edges in Hz at sample rate `fs`.
/// One value each is a lowpass (`wp < ws`) or a highpass (`wp > ws`); two
/// values each is a bandpass (stopband outside the passband) or a bandstop
/// (passband outside the stopband).
///
/// Method: prewarp every edge (`tan(pi f / fs)`), map the specification onto
/// the lowpass prototype, apply the family's order formula (see
/// [`real_order`]) and round up. For a bandstop the two passband edges are
/// first moved into the transition bands as far as lowers the order (one
/// after the other, as SciPy does), so the returned `wn` can lie inside the
/// transition bands; the original passband is still inside the new one.
///
/// `wn` is what the design function of the same family expects:
/// - Butterworth: the -3 dB frequency that makes the passband edge lose
///   exactly `rp` dB (the stopband then has margin),
/// - Chebyshev I and elliptic: the passband edge(s),
/// - Chebyshev II: the stopband edge that makes the passband edge lose
///   exactly `rp` dB.
pub fn iir_order(family: IirFamily, wp: &[f64], ws: &[f64], rp: f64, rs: f64, fs: f64) -> Result<OrderEstimate, String> {
    if !(fs.is_finite() && fs > 0.0) {
        return Err(format!("the sample rate fs must be a positive number, got {fs}"));
    }
    if wp.len() != ws.len() || !(wp.len() == 1 || wp.len() == 2) {
        return Err(format!(
            "wp and ws must both be one frequency (low/high) or both be [low, high] (band/stop), got {} and {} values",
            wp.len(),
            ws.len()
        ));
    }
    if !(rp.is_finite() && rp > 0.0) {
        return Err(format!("the passband ripple rp must be above 0 dB, got {rp}"));
    }
    if !(rs.is_finite() && rs > rp) {
        return Err(format!("the stopband attenuation rs must be larger than the passband ripple rp ({rp} dB), got {rs}"));
    }
    let nyquist = fs / 2.0;
    for &f in wp.iter().chain(ws) {
        if !(f.is_finite() && f > 0.0 && f < nyquist) {
            return Err(format!("every edge must lie strictly between 0 and fs/2 = {nyquist} Hz, got {f}"));
        }
    }
    let warp = |f: f64| (PI * f / fs).tan();
    let unwarp = |w: f64| fs / PI * w.atan();
    let gp = 10f64.powf(0.1 * rp) - 1.0;
    let gs = 10f64.powf(0.1 * rs) - 1.0;

    let (kind, mut passb, stopb) = if wp.len() == 1 {
        if wp[0] == ws[0] {
            return Err(format!("wp and ws are the same frequency ({}); there is no transition band", wp[0]));
        }
        let kind = if wp[0] < ws[0] { FilterKind::Low } else { FilterKind::High };
        (kind, vec![warp(wp[0])], vec![warp(ws[0])])
    } else {
        if !(wp[0] < wp[1] && ws[0] < ws[1]) {
            return Err("band edges must be given as [low, high] with low < high".to_string());
        }
        let kind = if ws[0] < wp[0] && wp[1] < ws[1] {
            FilterKind::Band
        } else if wp[0] < ws[0] && ws[1] < wp[1] {
            FilterKind::Stop
        } else {
            return Err(format!(
                "the edges do not describe a bandpass (ws[0] < wp[0] < wp[1] < ws[1]) or a bandstop (wp[0] < ws[0] < ws[1] < wp[1]): wp = [{}, {}], ws = [{}, {}]",
                wp[0], wp[1], ws[0], ws[1]
            ));
        };
        (kind, vec![warp(wp[0]), warp(wp[1])], vec![warp(ws[0]), warp(ws[1])])
    };

    let nat = match kind {
        FilterKind::Low => stopb[0] / passb[0],
        FilterKind::High => passb[0] / stopb[0],
        FilterKind::Band => {
            let one = |si: f64| ((si * si - passb[0] * passb[1]) / (si * (passb[0] - passb[1]))).abs();
            one(stopb[0]).min(one(stopb[1]))
        }
        FilterKind::Stop => {
            let s = [stopb[0], stopb[1]];
            let p1 = passb[1];
            let p0 = golden_min(passb[0], stopb[0] * (1.0 - 1e-9), |x| real_order(family, bandstop_nat([x, p1], s), rp, rs));
            let p1 = golden_min(stopb[1] * (1.0 + 1e-9), passb[1], |x| real_order(family, bandstop_nat([p0, x], s), rp, rs));
            passb = vec![p0, p1];
            bandstop_nat([p0, p1], s)
        }
    };
    if !(nat.is_finite() && nat > 1.0) {
        return Err("the specification leaves no transition band".to_string());
    }
    let exact = real_order(family, nat, rp, rs);
    if !exact.is_finite() {
        return Err("the order formula did not produce a finite order for this specification".to_string());
    }
    // A specification that lands on an integer up to rounding must not be
    // pushed one order higher by the last bit.
    let order = ((exact - 1e-12).ceil().max(1.0)) as usize;
    let n = order as f64;

    let mut wn: Vec<f64> = match family {
        IirFamily::Cheby1 | IirFamily::Ellip => passb.iter().map(|&p| unwarp(p)).collect(),
        IirFamily::Butter => {
            // Prototype -3 dB point that puts exactly `rp` dB at the
            // passband edge: |H|^2 = 1/(1 + (W/W0)^(2n)).
            let w0 = gp.powf(-1.0 / (2.0 * n));
            let natural: Vec<f64> = match kind {
                FilterKind::Low => vec![w0 * passb[0]],
                FilterKind::High => vec![passb[0] / w0],
                FilterKind::Stop => {
                    let d = passb[1] - passb[0];
                    let disc = (d * d + 4.0 * w0 * w0 * passb[0] * passb[1]).sqrt();
                    vec![(-d + disc) / (2.0 * w0), (d + disc) / (2.0 * w0)]
                }
                FilterKind::Band => {
                    let d = passb[1] - passb[0];
                    let root = (w0 * w0 / 4.0 * d * d + passb[0] * passb[1]).sqrt();
                    vec![(-w0 * d / 2.0 + root).abs(), (w0 * d / 2.0 + root).abs()]
                }
            };
            natural.into_iter().map(unwarp).collect()
        }
        IirFamily::Cheby2 => {
            // Ratio stopband edge / passband edge of the order-n prototype
            // that exactly meets `rp` at the passband edge.
            let ratio = ((gs / gp).sqrt().acosh() / n).cosh();
            let nf = 1.0 / ratio;
            let natural: Vec<f64> = match kind {
                FilterKind::Low => vec![passb[0] / nf],
                FilterKind::High => vec![passb[0] * nf],
                FilterKind::Stop => {
                    let a = nf / 2.0 * (passb[0] - passb[1]) + (nf * nf * (passb[1] - passb[0]).powi(2) / 4.0 + passb[0] * passb[1]).sqrt();
                    vec![a, passb[0] * passb[1] / a]
                }
                FilterKind::Band => {
                    let a = (passb[0] - passb[1]) / (2.0 * nf) + ((passb[1] - passb[0]).powi(2) / (4.0 * nf * nf) + passb[0] * passb[1]).sqrt();
                    vec![a, passb[0] * passb[1] / a]
                }
            };
            natural.into_iter().map(unwarp).collect()
        }
    };
    wn.sort_by(|a, b| a.total_cmp(b));
    Ok(OrderEstimate { order, kind, wn })
}

// --------------------------------------------------------------- kaiserord

/// Kaiser's window-shape formula: the `beta` giving `atten_db` dB of
/// stopband attenuation (equivalently a passband ripple of the same size).
///
/// `0.1102 (A - 8.7)` above 50 dB, `0.5842 (A - 21)^0.4 + 0.07886 (A - 21)`
/// from 21 to 50 dB, `0` (a rectangular window) below 21 dB.
pub fn kaiser_beta(atten_db: f64) -> f64 {
    if atten_db > 50.0 {
        0.1102 * (atten_db - 8.7)
    } else if atten_db >= 21.0 {
        0.5842 * (atten_db - 21.0).powf(0.4) + 0.07886 * (atten_db - 21.0)
    } else {
        0.0
    }
}

/// The result of [`kaiserord`].
#[derive(Clone, Debug, PartialEq)]
pub struct KaiserEstimate {
    pub order: usize,
    pub beta: f64,
    pub kind: FilterKind,
    /// Cutoff(s) in Hz: the middle of each transition band.
    pub wn: Vec<f64>,
}

/// Kaiser's estimate of the FIR order and window `beta` for a windowed-sinc
/// design (MATLAB's `kaiserord`).
///
/// `freqs` are the transition-band edges in Hz, two per transition
/// (`[pass_end, stop_start]` for a lowpass); `amps` is the desired amplitude
/// of each band (one more band than transitions; zero or nonzero), `dev` the
/// allowed deviation per band (one value, or one per band; relative for a
/// passband, absolute for a stopband).
///
/// With `delta` the smallest deviation, `A = -20 log10(delta)` and `dw` the
/// narrowest transition in rad/sample, the order is
/// `ceil((A - 7.95) / (2.285 dw))` (`ceil(5.79 / dw)` below 21 dB) and
/// `beta` is [`kaiser_beta`]`(A)`. The order is raised to the next even
/// number when the last band is a passband (an odd order has a zero at
/// Nyquist). The formula is an estimate for a single transition: a lowpass
/// or highpass designed with it usually meets the deviation; in a bandpass
/// or bandstop the ripples of the two transitions add, so the passband can
/// exceed it (by up to a factor of two). Ask for half the `dev` then.
pub fn kaiserord(freqs: &[f64], amps: &[f64], dev: &[f64], fs: f64) -> Result<KaiserEstimate, String> {
    if !(fs.is_finite() && fs > 0.0) {
        return Err(format!("the sample rate fs must be a positive number, got {fs}"));
    }
    let nbands = amps.len();
    if !(nbands == 2 || nbands == 3) {
        return Err(format!("amps must name 2 bands (low/high) or 3 bands (band/stop), got {nbands}"));
    }
    if freqs.len() != 2 * (nbands - 1) {
        return Err(format!(
            "{nbands} bands need {} transition edges in freqs (two per transition), got {}",
            2 * (nbands - 1),
            freqs.len()
        ));
    }
    let nyquist = fs / 2.0;
    for pair in freqs.windows(2) {
        if !(pair[0] < pair[1]) {
            return Err("freqs must be strictly increasing".to_string());
        }
    }
    if freqs.iter().any(|&f| !(f.is_finite() && f > 0.0 && f < nyquist)) {
        return Err(format!("every edge in freqs must lie strictly between 0 and fs/2 = {nyquist}"));
    }
    if !(dev.len() == 1 || dev.len() == nbands) {
        return Err(format!("dev must be one number or one per band ({nbands}), got {}", dev.len()));
    }
    if dev.iter().any(|&d| !(d.is_finite() && d > 0.0)) {
        return Err("every deviation in dev must be above 0".to_string());
    }
    let pass: Vec<bool> = amps.iter().map(|&a| a != 0.0).collect();
    let kind = match pass.as_slice() {
        [true, false] => FilterKind::Low,
        [false, true] => FilterKind::High,
        [false, true, false] => FilterKind::Band,
        [true, false, true] => FilterKind::Stop,
        _ => return Err("amps must alternate between passbands (nonzero) and stopbands (0)".to_string()),
    };
    let delta = (0..nbands)
        .map(|i| {
            let d = if dev.len() == 1 { dev[0] } else { dev[i] };
            if pass[i] { d / amps[i].abs() } else { d }
        })
        .fold(f64::INFINITY, f64::min);
    let atten = -20.0 * delta.log10();
    let width = freqs.chunks(2).map(|c| c[1] - c[0]).fold(f64::INFINITY, f64::min);
    let dw = 2.0 * PI * width / fs;
    let exact = if atten > 21.0 { (atten - 7.95) / (2.285 * dw) } else { 5.79 / dw };
    let mut order = (exact.ceil().max(1.0)) as usize;
    if pass[nbands - 1] && order % 2 == 1 {
        order += 1;
    }
    let wn = freqs.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect();
    Ok(KaiserEstimate { order, beta: kaiser_beta(atten), kind, wn })
}

// ------------------------------------------------------------------- remez

/// Barycentric evaluation of the polynomial through `(xs[i], ys[i])` with
/// barycentric weights `gam`.
fn barycentric(x: f64, xs: &[f64], gam: &[f64], ys: &[f64]) -> f64 {
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..xs.len() {
        let d = x - xs[i];
        if d.abs() < 1e-15 {
            return ys[i];
        }
        let t = gam[i] / d;
        num += t * ys[i];
        den += t;
    }
    num / den
}

/// Barycentric weights `1 / prod_{j != i} (x_i - x_j)`, scaled by a common
/// factor (they only ever appear in ratios). Accumulated in logarithms so a
/// long filter cannot overflow or underflow the product.
fn barycentric_weights(xs: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut logs = vec![0.0; n];
    let mut signs = vec![1.0; n];
    for i in 0..n {
        for j in 0..n {
            if i != j {
                let d = xs[i] - xs[j];
                logs[i] -= d.abs().ln();
                if d < 0.0 {
                    signs[i] = -signs[i];
                }
            }
        }
    }
    let top = logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (0..n).map(|i| signs[i] * (logs[i] - top).exp()).collect()
}

/// Parks-McClellan optimal equiripple linear-phase FIR design (MATLAB's
/// `firpm`, SciPy's `remez`), by the Remez exchange algorithm.
///
/// Returns the `order + 1` symmetric taps minimizing the largest weighted
/// error `max |W(f) (D(f) - A(f))|` over the bands, `A` being the filter's
/// zero-phase response.
///
/// - `bands`: band edges, two per band, increasing, in `[0, 1]` with `1` =
///   Nyquist. The gaps between bands are transition bands (unconstrained).
/// - `desired`: the desired amplitude at each band edge (same length as
///   `bands`), linear across a band.
/// - `weights`: one positive weight per band (default 1 each); a band with
///   weight 10 ends up with a tenth of the ripple.
///
/// An even `order` gives a Type I filter, an odd `order` a Type II filter
/// (which has a zero at Nyquist, so its desired response must be 0 there).
/// The antisymmetric types III and IV (Hilbert transformers,
/// differentiators) are not built.
///
/// Method. `A(f)` is `P(cos(pi f))` for Type I and `cos(pi f / 2) P(cos(pi
/// f))` for Type II, `P` a polynomial with `r = order/2 + 1` (Type I) or
/// `(order+1)/2` (Type II) coefficients; Type II is reduced to the same
/// polynomial problem by dividing `D` and multiplying `W` by `cos(pi f /
/// 2)`. Starting from `r + 1` equally spaced points: solve for the deviation
/// `delta` that makes the error alternate `+-delta` on the current points
/// (closed form with barycentric weights), interpolate `P` through them,
/// find the local extrema of the weighted error (on a search grid, each then
/// refined between grid points by a parabola), keep the `r + 1` largest that
/// alternate in sign, and repeat until the largest error equals `delta`. By
/// the alternation theorem that `P` is the unique minimax solution. The taps
/// are read off `P` with an exact inverse DFT.
pub fn remez(order: usize, bands: &[f64], desired: &[f64], weights: Option<&[f64]>) -> Result<Vec<f64>, String> {
    if order == 0 {
        return Err("the order must be at least 1".to_string());
    }
    if bands.len() < 2 || bands.len() % 2 != 0 {
        return Err("bands must be given as [low, high] pairs, one pair per band".to_string());
    }
    if desired.len() != bands.len() {
        return Err(format!(
            "desired needs one amplitude per band edge ({}), got {}",
            bands.len(),
            desired.len()
        ));
    }
    let nbands = bands.len() / 2;
    if bands.iter().any(|f| !f.is_finite()) || desired.iter().any(|d| !d.is_finite()) {
        return Err("bands and desired must be finite numbers".to_string());
    }
    if bands[0] < 0.0 || bands[bands.len() - 1] > 1.0 + 1e-12 {
        return Err("band edges must lie between 0 and the Nyquist frequency".to_string());
    }
    for b in 0..nbands {
        if !(bands[2 * b] < bands[2 * b + 1]) {
            return Err(format!("band {} is empty or reversed: [{}, {}]", b + 1, bands[2 * b], bands[2 * b + 1]));
        }
        if b > 0 && bands[2 * b] < bands[2 * b - 1] {
            return Err("bands must not overlap and must be in increasing order".to_string());
        }
    }
    let wts: Vec<f64> = match weights {
        Some(w) => {
            if w.len() != nbands {
                return Err(format!("weights needs one value per band ({nbands}), got {}", w.len()));
            }
            if w.iter().any(|&v| !(v.is_finite() && v > 0.0)) {
                return Err("every weight must be above 0".to_string());
            }
            w.to_vec()
        }
        None => vec![1.0; nbands],
    };
    let odd = order % 2 == 1;
    if odd && bands[bands.len() - 1] >= 1.0 - 1e-12 && desired[desired.len() - 1] != 0.0 {
        return Err(format!(
            "an odd order gives a filter with a zero at the Nyquist frequency, so the response there cannot be {}; use order {}",
            desired[desired.len() - 1],
            order + 1
        ));
    }
    // Number of polynomial coefficients.
    let r = if odd { (order + 1) / 2 } else { order / 2 + 1 };
    let m = r + 1;

    // Desired value and weight of the (reduced) problem at frequency `f` of
    // band `b`.
    let target = |b: usize, f: f64| -> (f64, f64) {
        let (lo, hi) = (bands[2 * b], bands[2 * b + 1]);
        let t = (f - lo) / (hi - lo);
        let (d, w) = (desired[2 * b] + t * (desired[2 * b + 1] - desired[2 * b]), wts[b]);
        if odd {
            let c = (PI * f / 2.0).cos();
            (d / c, w * c)
        } else {
            (d, w)
        }
    };

    // Search grid over the bands. It only has to be dense enough to see
    // every lobe of the error; each extremum is then located between grid
    // points (see `Ext` below), so the result does not depend on the grid.
    let mut density = 20.0;
    let (gf, band_of) = loop {
        let delf = 1.0 / (density * r as f64);
        let (mut gf, mut band_of) = (Vec::new(), Vec::new());
        for b in 0..nbands {
            // A Type II filter is zero at Nyquist whatever the taps: keep
            // the grid (and the division by cos(pi f / 2)) away from it.
            let (lo, hi) = (bands[2 * b], if odd { bands[2 * b + 1].min(1.0 - 0.5 * delf) } else { bands[2 * b + 1].min(1.0) });
            if hi <= lo {
                continue;
            }
            let npts = (((hi - lo) / delf).ceil() as usize + 1).max(2);
            for k in 0..npts {
                gf.push(lo + (hi - lo) * k as f64 / (npts - 1) as f64);
                band_of.push(b);
            }
        }
        if gf.len() >= 2 * m || density > 1e6 {
            break (gf, band_of);
        }
        density *= 2.0;
    };
    let ng = gf.len();
    if ng < m {
        return Err("the bands are too narrow for this order".to_string());
    }

    /// One point of the reference set: frequency, band, and the weighted
    /// error there in the last pass.
    #[derive(Clone, Copy)]
    struct Ext {
        f: f64,
        band: usize,
        e: f64,
    }
    let mut ext: Vec<Ext> = (0..m)
        .map(|i| {
            let g = i * (ng - 1) / (m - 1);
            Ext { f: gf[g], band: band_of[g], e: 0.0 }
        })
        .collect();
    let mut err = vec![0.0; ng];
    let mut xs = vec![0.0; m];
    let mut ys = vec![0.0; m];
    let mut gam = Vec::new();
    let mut converged = false;
    let mut spread = f64::INFINITY;
    for _iter in 0..200 {
        // Deviation that makes the error alternate +-delta on the reference
        // set, and the polynomial through the implied values.
        let tw: Vec<(f64, f64)> = ext.iter().map(|p| target(p.band, p.f)).collect();
        for i in 0..m {
            xs[i] = (PI * ext[i].f).cos();
        }
        gam = barycentric_weights(&xs);
        let mut num = 0.0;
        let mut den = 0.0;
        for i in 0..m {
            let s = if i % 2 == 0 { 1.0 } else { -1.0 };
            num += gam[i] * tw[i].0;
            den += gam[i] * s / tw[i].1;
        }
        let delta = num / den;
        if !delta.is_finite() {
            return Err("the exchange broke down (non-finite deviation); the specification may be degenerate".to_string());
        }
        for i in 0..m {
            let s = if i % 2 == 0 { 1.0 } else { -1.0 };
            ys[i] = tw[i].0 - s * delta / tw[i].1;
        }
        let error_at = |b: usize, f: f64| -> f64 {
            let (d, w) = target(b, f);
            w * (d - barycentric((PI * f).cos(), &xs, &gam, &ys))
        };
        for g in 0..ng {
            err[g] = error_at(band_of[g], gf[g]);
        }

        // Local extrema of the error, band by band (band ends count). An
        // interior one is moved to the vertex of the parabola through it and
        // its two neighbours when the error is larger there.
        let mut cand: Vec<Ext> = Vec::with_capacity(2 * m);
        for g in 0..ng {
            let e = err[g];
            let left = if g > 0 && band_of[g - 1] == band_of[g] { Some(err[g - 1]) } else { None };
            let right = if g + 1 < ng && band_of[g + 1] == band_of[g] { Some(err[g + 1]) } else { None };
            let is_ext = if e > 0.0 {
                left.map_or(true, |l| e >= l) && right.map_or(true, |rr| e > rr)
            } else if e < 0.0 {
                left.map_or(true, |l| e <= l) && right.map_or(true, |rr| e < rr)
            } else {
                false
            };
            if !is_ext {
                continue;
            }
            let mut point = Ext { f: gf[g], band: band_of[g], e };
            if let (Some(l), Some(rr)) = (left, right) {
                let curv = l - 2.0 * e + rr;
                if curv != 0.0 {
                    let shift = (0.5 * (l - rr) / curv).clamp(-0.5, 0.5);
                    let f = gf[g] + shift * if shift < 0.0 { gf[g] - gf[g - 1] } else { gf[g + 1] - gf[g] };
                    let ef = error_at(band_of[g], f);
                    if ef.abs() > e.abs() && (ef > 0.0) == (e > 0.0) {
                        point = Ext { f, band: band_of[g], e: ef };
                    }
                }
            }
            cand.push(point);
        }
        // Keep r+1 that alternate in sign, preferring the largest.
        loop {
            let mut merged: Vec<Ext> = Vec::with_capacity(cand.len());
            for &p in &cand {
                match merged.last_mut() {
                    Some(last) if (last.e > 0.0) == (p.e > 0.0) => {
                        if p.e.abs() > last.e.abs() {
                            *last = p;
                        }
                    }
                    _ => merged.push(p),
                }
            }
            cand = merged;
            if cand.len() <= m {
                break;
            }
            let last = cand.len() - 1;
            if cand.len() == m + 1 {
                if cand[0].e.abs() < cand[last].e.abs() {
                    cand.remove(0);
                } else {
                    cand.pop();
                }
                break;
            }
            let (mut at, mut smallest) = (0, f64::INFINITY);
            for (k, p) in cand.iter().enumerate() {
                if p.e.abs() < smallest {
                    smallest = p.e.abs();
                    at = k;
                }
            }
            cand.remove(at);
        }
        if cand.len() < m {
            return Err("the exchange did not find enough alternations; the specification may be degenerate (try a different order or wider bands)".to_string());
        }
        // Converged when the largest error anywhere is the deviation the
        // reference set was solved for: the error is then equiripple.
        let peak = cand.iter().fold(0.0f64, |a, p| a.max(p.e.abs()));
        spread = (peak - delta.abs()) / peak;
        if spread <= 1e-9 {
            converged = true;
            break;
        }
        ext = cand;
    }
    if !converged && !(spread <= 1e-5) {
        return Err(format!(
            "the exchange did not converge (the error extrema still differ by {:.1e} of the ripple)",
            spread
        ));
    }

    // P has `r` cosine coefficients; sample it at 2r-1 DFT points and invert.
    // c[n] = (1/N)(P(0) + 2 sum_j P(w_j) cos(w_j n)), w_j = 2 pi j / N, so
    // that P(w) = c[0] + 2 sum_{n>=1} c[n] cos(n w).
    let nn = 2 * r - 1;
    let samples: Vec<f64> = (0..r).map(|j| barycentric((2.0 * PI * j as f64 / nn as f64).cos(), &xs, &gam, &ys)).collect();
    let c: Vec<f64> = (0..r)
        .map(|n| {
            let mut acc = samples[0];
            for j in 1..r {
                acc += 2.0 * samples[j] * (2.0 * PI * (j * n) as f64 / nn as f64).cos();
            }
            acc / nn as f64
        })
        .collect();
    let taps = order + 1;
    let mut h = vec![0.0; taps];
    if !odd {
        let mid = order / 2;
        for n in 0..r {
            h[mid + n] = c[n];
            h[mid - n] = c[n];
        }
    } else {
        // A(w) = cos(w/2) (p0 + sum_k p_k cos(k w)) = 2 sum_m h[L-1-m] cos((m + 1/2) w)
        // with p0 = c[0], p_k = 2 c[k]:
        //   2 h[L-1]   = p0 + p1/2
        //   2 h[L-1-m] = (p_m + p_{m+1}) / 2,  m >= 1   (p_L = 0)
        let l = r;
        let p = |k: usize| -> f64 {
            if k == 0 {
                c[0]
            } else if k < l {
                2.0 * c[k]
            } else {
                0.0
            }
        };
        for mm in 0..l {
            let v = if mm == 0 { 0.5 * (p(0) + 0.5 * p(1)) } else { 0.25 * (p(mm) + p(mm + 1)) };
            h[l - 1 - mm] = v;
            h[l + mm] = v;
        }
    }
    if h.iter().any(|v| !v.is_finite()) {
        return Err("the design produced non-finite taps; the specification may be degenerate".to_string());
    }
    Ok(h)
}

// ----------------------------------------------------------------- windows

/// Symmetric generalized cosine window `sum_k (-1)^k a_k cos(2 pi k i /
/// (n-1))`. `n = 1` is `[1]`, `n = 0` is empty.
pub fn general_cosine_window(n: usize, coeffs: &[f64]) -> Vec<f64> {
    match n {
        0 => return Vec::new(),
        1 => return vec![1.0],
        _ => {}
    }
    let denom = (n - 1) as f64;
    (0..n)
        .map(|i| {
            let phase = 2.0 * PI * i as f64 / denom;
            coeffs
                .iter()
                .enumerate()
                .map(|(k, &a)| if k % 2 == 0 { a * (k as f64 * phase).cos() } else { -a * (k as f64 * phase).cos() })
                .sum()
        })
        .collect()
}

/// Four-term minimum Blackman-Harris coefficients (Harris 1978).
pub const BLACKMANHARRIS: [f64; 4] = [0.35875, 0.48829, 0.14128, 0.01168];
/// Nuttall's minimum four-term Blackman-Harris coefficients (Nuttall 1981),
/// as used by MATLAB's `nuttallwin` and SciPy's `nuttall`.
pub const NUTTALL: [f64; 4] = [0.3635819, 0.4891775, 0.1365995, 0.0106411];
/// Five-term flat-top coefficients (D'Antona & Ferrero), as used by MATLAB's
/// `flattopwin` and SciPy's `flattop`.
pub const FLATTOP: [f64; 5] = [0.21557895, 0.41663158, 0.277263158, 0.083578947, 0.006947368];

/// Symmetric Bartlett (triangular, zero at both ends) window.
pub fn bartlett_window(n: usize) -> Vec<f64> {
    match n {
        0 => return Vec::new(),
        1 => return vec![1.0],
        _ => {}
    }
    let denom = (n - 1) as f64;
    (0..n).map(|i| 1.0 - (2.0 * i as f64 / denom - 1.0).abs()).collect()
}

/// Symmetric Tukey (tapered cosine) window: a cosine taper over the fraction
/// `alpha` of the window (half at each end), flat in between. `alpha <= 0`
/// is rectangular, `alpha >= 1` is a Hann window.
pub fn tukey_window(n: usize, alpha: f64) -> Vec<f64> {
    match n {
        0 => return Vec::new(),
        1 => return vec![1.0],
        _ => {}
    }
    if alpha <= 0.0 {
        return vec![1.0; n];
    }
    let alpha = alpha.min(1.0);
    let span = alpha * (n - 1) as f64;
    let width = (span / 2.0).floor() as usize;
    (0..n)
        .map(|i| {
            let k = i.min(n - 1 - i);
            if k <= width {
                0.5 * (1.0 + (PI * (-1.0 + 2.0 * k as f64 / span)).cos())
            } else {
                1.0
            }
        })
        .collect()
}

/// Symmetric Gaussian window `exp(-((i - (n-1)/2) / std)^2 / 2)`; `std` in
/// samples.
pub fn gaussian_window(n: usize, std: f64) -> Vec<f64> {
    match n {
        0 => return Vec::new(),
        1 => return vec![1.0],
        _ => {}
    }
    let mid = (n - 1) as f64 / 2.0;
    (0..n).map(|i| (-0.5 * ((i as f64 - mid) / std).powi(2)).exp()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{butter, cheby1, cheby2, ellip, fir1, fir_freqz, freqz, Sos};

    fn design(family: IirFamily, est: &OrderEstimate, rp: f64, rs: f64, fs: f64) -> Sos {
        match family {
            IirFamily::Butter => butter(est.order, est.kind, &est.wn, fs),
            IirFamily::Cheby1 => cheby1(est.order, rp, est.kind, &est.wn, fs),
            IirFamily::Cheby2 => cheby2(est.order, rs, est.kind, &est.wn, fs),
            IirFamily::Ellip => ellip(est.order, rp, rs, est.kind, &est.wn, fs),
        }
        .unwrap()
    }

    /// Worst passband loss and best stopband attenuation (both positive dB)
    /// of `sos` measured with `freqz` on 8001 points.
    fn measure(sos: &Sos, wp: &[f64], ws: &[f64], kind: FilterKind, fs: f64) -> (f64, f64) {
        let n = 8001;
        let h = freqz(sos, n);
        let mut worst_pass: f64 = 0.0;
        let mut worst_stop = f64::INFINITY;
        let mut peak: f64 = 0.0;
        for (k, c) in h.iter().enumerate() {
            let f = fs / 2.0 * k as f64 / (n - 1) as f64;
            let db = 20.0 * c.magnitude().max(1e-300).log10();
            peak = peak.max(db);
            let (in_pass, in_stop) = match kind {
                FilterKind::Low => (f <= wp[0], f >= ws[0]),
                FilterKind::High => (f >= wp[0], f <= ws[0]),
                FilterKind::Band => (f >= wp[0] && f <= wp[1], f <= ws[0] || f >= ws[1]),
                FilterKind::Stop => (f <= wp[0] || f >= wp[1], f >= ws[0] && f <= ws[1]),
            };
            if in_pass {
                worst_pass = worst_pass.max(-db);
            }
            if in_stop {
                worst_stop = worst_stop.min(-db);
            }
        }
        assert!(peak < 1e-6, "gain above 0 dB: {peak}");
        (worst_pass, worst_stop)
    }

    const FAMILIES: [IirFamily; 4] = [IirFamily::Butter, IirFamily::Cheby1, IirFamily::Cheby2, IirFamily::Ellip];

    fn specs() -> Vec<(Vec<f64>, Vec<f64>, FilterKind)> {
        // fs = 2, so these are the normalized edges of the SciPy reference run.
        vec![
            (vec![0.2], vec![0.3], FilterKind::Low),
            (vec![0.3], vec![0.2], FilterKind::High),
            (vec![0.2, 0.5], vec![0.1, 0.6], FilterKind::Band),
            (vec![0.1, 0.6], vec![0.2, 0.5], FilterKind::Stop),
        ]
    }

    #[test]
    fn a_design_at_the_estimated_order_meets_the_specification() {
        let (rp, rs, fs) = (1.0, 40.0, 2.0);
        for family in FAMILIES {
            for (wp, ws, kind) in specs() {
                let est = iir_order(family, &wp, &ws, rp, rs, fs).unwrap();
                assert_eq!(est.kind, kind);
                let sos = design(family, &est, rp, rs, fs);
                let (pass, stop) = measure(&sos, &wp, &ws, kind, fs);
                assert!(pass <= rp + 1e-6, "{family:?} {kind:?}: passband loses {pass} dB, allowed {rp}");
                assert!(stop >= rs - 1e-6, "{family:?} {kind:?}: stopband only {stop} dB down, wanted {rs}");
            }
        }
    }

    #[test]
    fn one_order_less_does_not_meet_the_specification() {
        // The estimate is the MINIMUM order: for lowpass and highpass, where
        // `wn` leaves no freedom beyond the family's own convention, the
        // next lower order must fail one of the two bands.
        let (rp, rs, fs) = (1.0, 40.0, 2.0);
        for family in FAMILIES {
            for (wp, ws, kind) in specs().into_iter().take(2) {
                let est = iir_order(family, &wp, &ws, rp, rs, fs).unwrap();
                // Re-derive wn for the lower order the way the estimator would.
                let lower = OrderEstimate { order: est.order - 1, ..est.clone() };
                let mut ok_somewhere = false;
                // Slide the design frequency across the whole transition band.
                for step in 0..=200 {
                    let t = step as f64 / 200.0;
                    let f = wp[0] + t * (ws[0] - wp[0]);
                    let trial = OrderEstimate { wn: vec![f], ..lower.clone() };
                    let sos = design(family, &trial, rp, rs, fs);
                    let (pass, stop) = measure(&sos, &wp, &ws, kind, fs);
                    if pass <= rp && stop >= rs {
                        ok_somewhere = true;
                    }
                }
                assert!(!ok_somewhere, "{family:?} {kind:?}: order {} already meets the spec", est.order - 1);
            }
        }
    }

    #[test]
    fn orders_and_frequencies_match_scipy() {
        // scipy.signal.{buttord,cheb1ord,cheb2ord,ellipord}(wp, ws, 1, 40),
        // SciPy 1.17.1, same four specifications as `specs()`.
        let reference: [(IirFamily, [(usize, &[f64]); 4]); 4] = [
            (IirFamily::Butter, [
                (12, &[0.21077527313622169]),
                (12, &[0.28574364670293406]),
                (11, &[0.19408840718550224, 0.5101870368546043]),
                (11, &[0.15378706346220905, 0.5869714708500121]),
            ]),
            (IirFamily::Cheby1, [
                (6, &[0.2]),
                (6, &[0.3]),
                (6, &[0.2, 0.5]),
                (6, &[0.14758232569947782, 0.5999987080915622]),
            ]),
            (IirFamily::Cheby2, [
                (6, &[0.2950241106106145]),
                (6, &[0.20366468429474507]),
                (6, &[0.15706857725752188, 0.580219828322454]),
                (6, &[0.19093195995113274, 0.515729618134045]),
            ]),
            (IirFamily::Ellip, [
                (4, &[0.2]),
                (4, &[0.3]),
                (4, &[0.2, 0.5]),
                (4, &[0.14758235223075672, 0.5999987080915622]),
            ]),
        ];
        for (family, rows) in reference {
            for ((wp, ws, kind), (order, wn)) in specs().into_iter().zip(rows) {
                let est = iir_order(family, &wp, &ws, 1.0, 40.0, 2.0).unwrap();
                assert_eq!(est.order, order, "{family:?} {kind:?}");
                // The bandstop edges come out of a 1-D search on both sides
                // (SciPy's own tolerance there is 1e-5).
                let tol = if kind == FilterKind::Stop { 2e-4 } else { 1e-12 };
                for (a, b) in est.wn.iter().zip(wn) {
                    assert!((a - b).abs() < tol, "{family:?} {kind:?}: wn {a} vs SciPy {b}");
                }
            }
        }
    }

    #[test]
    fn buttord_matches_the_textbook_formula_by_hand() {
        // MATLAB's documentation example: 40 Hz pass, 150 Hz stop, 3 dB, 60 dB
        // at 1000 Hz. tan(pi*0.04) = 0.126329, tan(pi*0.15) = 0.509525, ratio
        // 4.03332; log10((10^6 - 1)/(10^0.3 - 1)) / (2 log10(4.03332)) = 4.955,
        // so the order is 5.
        let est = iir_order(IirFamily::Butter, &[40.0], &[150.0], 3.0, 60.0, 1000.0).unwrap();
        assert_eq!(est.order, 5);
        let by_hand = (999_999.0f64 / (10f64.powf(0.3) - 1.0)).log10()
            / (2.0 * ((PI * 0.15).tan() / (PI * 0.04).tan()).log10());
        assert!((by_hand - 4.955).abs() < 1e-3, "{by_hand}");
        // SciPy: wn = 0.08003760149689168 (normalized) -> 40.0188 Hz.
        assert!((est.wn[0] - 0.08003760149689168 * 500.0).abs() < 1e-9, "{}", est.wn[0]);
    }

    #[test]
    fn order_estimates_reject_bad_specifications() {
        let f = IirFamily::Butter;
        assert!(iir_order(f, &[0.2], &[0.2], 1.0, 40.0, 2.0).is_err());
        assert!(iir_order(f, &[0.2], &[0.3, 0.4], 1.0, 40.0, 2.0).is_err());
        assert!(iir_order(f, &[0.2], &[1.3], 1.0, 40.0, 2.0).is_err());
        assert!(iir_order(f, &[0.2], &[0.3], 40.0, 1.0, 2.0).is_err());
        assert!(iir_order(f, &[0.2, 0.5], &[0.3, 0.6], 1.0, 40.0, 2.0).is_err());
    }

    // ---- kaiserord

    #[test]
    fn kaiser_beta_matches_kaisers_formula() {
        assert_eq!(kaiser_beta(15.0), 0.0);
        // 0.5842*(19)^0.4 + 0.07886*19, scipy.signal.kaiser_beta(40)
        assert!((kaiser_beta(40.0) - 3.3953210522614574).abs() < 1e-12);
        // 0.1102*(60-8.7), scipy.signal.kaiser_beta(60)
        assert!((kaiser_beta(60.0) - 5.65326).abs() < 1e-12);
    }

    #[test]
    fn kaiserord_matches_scipy_and_the_formula() {
        // scipy.signal.kaiserord(60, 0.1) -> (74 taps, beta 5.65326): 60 dB,
        // transition 0.1 of Nyquist. (60 - 7.95)/(2.285 * 0.1 pi) = 72.5 -> 73.
        let est = kaiserord(&[0.25, 0.35], &[1.0, 0.0], &[0.001], 2.0).unwrap();
        assert_eq!(est.order, 73);
        assert!((est.beta - 5.65326).abs() < 1e-9);
        assert_eq!(est.kind, FilterKind::Low);
        assert!((est.wn[0] - 0.3).abs() < 1e-12);
        // A highpass needs an even order.
        let hp = kaiserord(&[0.25, 0.35], &[0.0, 1.0], &[0.001], 2.0).unwrap();
        assert_eq!(hp.order, 74);
        assert_eq!(hp.kind, FilterKind::High);
    }

    fn kaiser_window(n: usize, beta: f64) -> Vec<f64> {
        let denom = (n - 1) as f64;
        let i0b = crate::special::bessel_i0(beta);
        (0..n)
            .map(|i| {
                let r = 2.0 * i as f64 / denom - 1.0;
                crate::special::bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0b
            })
            .collect()
    }

    /// Largest |A - 1| over `pass` and largest |A| over `stop` (ranges in
    /// units of Nyquist) for the taps `b`.
    fn fir_deviation(b: &[f64], pass: &[(f64, f64)], stop: &[(f64, f64)]) -> (f64, f64) {
        let n = 8001;
        let h = fir_freqz(b, n);
        let (mut dp, mut ds): (f64, f64) = (0.0, 0.0);
        for (k, c) in h.iter().enumerate() {
            let f = k as f64 / (n - 1) as f64;
            if pass.iter().any(|&(a, z)| f >= a && f <= z) {
                dp = dp.max((c.magnitude() - 1.0).abs());
            }
            if stop.iter().any(|&(a, z)| f >= a && f <= z) {
                ds = ds.max(c.magnitude());
            }
        }
        (dp, ds)
    }

    #[test]
    fn a_kaiser_design_at_the_estimated_order_meets_the_ripple_specification() {
        // Lowpass: pass to 1000 Hz, stop from 1500 Hz at 8 kHz, 1 % passband
        // ripple and 0.1 % (60 dB) stopband.
        let fs = 8000.0;
        let est = kaiserord(&[1000.0, 1500.0], &[1.0, 0.0], &[0.01, 0.001], fs).unwrap();
        let b = fir1(est.order, est.kind, &est.wn, fs, &kaiser_window(est.order + 1, est.beta)).unwrap();
        let (dp, ds) = fir_deviation(&b, &[(0.0, 0.25)], &[(0.375, 1.0)]);
        assert!(dp <= 0.01, "passband deviation {dp}");
        assert!(ds <= 0.001 * 1.02, "stopband deviation {ds}");
        // Highpass, same edges mirrored.
        let est = kaiserord(&[2500.0, 3000.0], &[0.0, 1.0], &[0.001, 0.01], fs).unwrap();
        assert_eq!((est.kind, est.order % 2), (FilterKind::High, 0));
        let b = fir1(est.order, est.kind, &est.wn, fs, &kaiser_window(est.order + 1, est.beta)).unwrap();
        let (dp, ds) = fir_deviation(&b, &[(0.75, 1.0)], &[(0.0, 0.625)]);
        assert!(dp <= 0.01 && ds <= 0.001 * 1.02, "deviations {dp} {ds}");
        // Bandpass, 40 dB. Kaiser's formula is for one transition. In a band
        // filter the ripples of the two transitions add (and `fir1` then
        // scales the band centre to exactly 1), so the passband can exceed
        // the requested deviation: here 1.13 %, against a bound of twice the
        // request. The stopbands meet it. Checked for what it is: an estimate.
        let est = kaiserord(&[500.0, 800.0, 2000.0, 2300.0], &[0.0, 1.0, 0.0], &[0.01], fs).unwrap();
        assert_eq!(est.kind, FilterKind::Band);
        let b = fir1(est.order, est.kind, &est.wn, fs, &kaiser_window(est.order + 1, est.beta)).unwrap();
        let (dp, ds) = fir_deviation(&b, &[(0.2, 0.5)], &[(0.0, 0.125), (0.575, 1.0)]);
        assert!(ds <= 0.01, "stopband deviation {ds}");
        assert!(dp <= 0.02, "passband deviation {dp}");
    }

    // ---- remez

    /// Zero-phase response of symmetric taps at `f` (units of Nyquist).
    fn amplitude(h: &[f64], f: f64) -> f64 {
        let mid = (h.len() - 1) as f64 / 2.0;
        h.iter().enumerate().map(|(k, &v)| v * (PI * f * (k as f64 - mid)).cos()).sum()
    }

    /// Signed weighted-error extrema over the bands, on a fine grid.
    fn error_extrema(h: &[f64], bands: &[f64], desired: &[f64], weights: &[f64]) -> Vec<f64> {
        let mut out = Vec::new();
        for b in 0..bands.len() / 2 {
            let (lo, hi) = (bands[2 * b], bands[2 * b + 1]);
            let n = 20001;
            let e: Vec<f64> = (0..n)
                .map(|k| {
                    let t = k as f64 / (n - 1) as f64;
                    let f = lo + t * (hi - lo);
                    weights[b] * (desired[2 * b] + t * (desired[2 * b + 1] - desired[2 * b]) - amplitude(h, f))
                })
                .collect();
            for k in 0..n {
                let l = if k > 0 { e[k - 1] } else { 0.0 };
                let r = if k + 1 < n { e[k + 1] } else { 0.0 };
                let v = e[k];
                let peak = if k == 0 { v.abs() > r.abs() } else if k + 1 == n { v.abs() > l.abs() } else { (v > 0.0 && v >= l && v > r) || (v < 0.0 && v <= l && v < r) };
                if peak {
                    out.push(v);
                }
            }
        }
        out
    }

    /// The alternation theorem, checked on the finished taps: at least `r+1`
    /// error extrema of equal size and alternating sign. Returns the ripple.
    fn assert_equiripple(h: &[f64], bands: &[f64], desired: &[f64], weights: &[f64], r: usize) -> f64 {
        let ext = error_extrema(h, bands, desired, weights);
        let delta = ext.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
        // extrema that reach the full ripple
        let full: Vec<f64> = ext.iter().cloned().filter(|v| v.abs() > 0.999 * delta).collect();
        assert!(full.len() >= r + 1, "only {} full-size extrema, need {}", full.len(), r + 1);
        for v in &full {
            assert!((v.abs() - delta).abs() <= 1e-3 * delta, "extremum {v} vs ripple {delta}");
        }
        let alternations = full.windows(2).filter(|p| (p[0] > 0.0) != (p[1] > 0.0)).count();
        assert!(alternations >= r, "only {alternations} sign alternations, need {r}");
        delta
    }

    #[test]
    fn remez_lowpass_matches_scipy_and_is_equiripple() {
        // scipy.signal.remez(21, [0, 0.3, 0.4, 1], [1, 0], fs=2,
        // grid_density=4096, maxiter=500), SciPy 1.17.1: first 11 taps. (At
        // SciPy's default grid_density=16 its own taps move in the fifth
        // decimal; 1024 and 4096 agree to 1e-8.)
        let reference = [
            -0.03796584539600822, -0.0084514392873488, 0.02065528411215688, 0.03928173761604945,
            0.0167372292746837, -0.03805178424910088, -0.06841126830904648, -0.01362634272418903,
            0.12901107625335648, 0.2848114706977272, 0.3523612852077771,
        ];
        let bands = [0.0, 0.3, 0.4, 1.0];
        let desired = [1.0, 1.0, 0.0, 0.0];
        let h = remez(20, &bands, &desired, None).unwrap();
        assert_eq!(h.len(), 21);
        for k in 0..11 {
            assert!((h[k] - reference[k]).abs() < 1e-6, "tap {k}: {} vs SciPy {}", h[k], reference[k]);
            assert!((h[k] - h[20 - k]).abs() < 1e-14, "not symmetric at {k}");
        }
        let delta = assert_equiripple(&h, &bands, &desired, &[1.0, 1.0], 11);
        // SciPy's own design measures 0.0555130 in both bands.
        assert!((delta - 0.0555130).abs() < 1e-6, "ripple {delta}");
    }

    #[test]
    fn remez_type_two_with_weights_matches_scipy() {
        // scipy.signal.remez(22, [0, 0.3, 0.4, 1], [1, 0], weight=[1, 10],
        // fs=2, grid_density=4096, maxiter=500): order 21, a Type II filter.
        let reference = [
            -0.00791203496986663, 0.01388934381237734, 0.03970498016748691, 0.05496990880666811,
            0.03432542213495737, -0.02021341142659564, -0.06680190955704801, -0.04841625545999969,
            0.05811719089048771, 0.2107493232799366, 0.32231923269519575,
        ];
        let bands = [0.0, 0.3, 0.4, 1.0];
        let desired = [1.0, 1.0, 0.0, 0.0];
        let h = remez(21, &bands, &desired, Some(&[1.0, 10.0])).unwrap();
        assert_eq!(h.len(), 22);
        for k in 0..11 {
            assert!((h[k] - reference[k]).abs() < 1e-6, "tap {k}: {} vs SciPy {}", h[k], reference[k]);
            assert!((h[k] - h[21 - k]).abs() < 1e-14);
        }
        // Weighted error is equiripple: passband ripple is ten times the
        // stopband ripple (SciPy: 0.1814636 and 0.01814636).
        let (dp, ds) = fir_deviation(&h, &[(0.0, 0.3)], &[(0.4, 1.0)]);
        assert!((dp - 0.1814636).abs() < 1e-6 && (ds - 0.01814636).abs() < 1e-7, "{dp} {ds}");
        assert_equiripple(&h, &bands, &desired, &[1.0, 10.0], 11);
        assert!(amplitude(&h, 1.0).abs() < 1e-12, "a Type II filter is zero at Nyquist");
    }

    #[test]
    fn remez_bandpass_matches_scipy() {
        // scipy.signal.remez(31, [0, 0.1, 0.2, 0.5, 0.6, 1], [0, 1, 0], fs=2,
        // grid_density=4096, maxiter=500): first 16 taps.
        let reference = [
            -4.6119459883052971e-03, -7.8213227915919937e-03, -9.0443864035621678e-03, 2.9330251134870124e-02,
            1.8912340583493940e-02, 3.7629607661612201e-04, 2.8812590781443100e-02, 4.8608553580982802e-02,
            -1.0621258450603580e-02, -5.0508367993420934e-02, 9.8873830509928944e-04, -2.7286639323599236e-02,
            -1.9303718610170792e-01, -1.7530991345376257e-01, 1.6860110727414276e-01, 3.9869566921549254e-01,
        ];
        let bands = [0.0, 0.1, 0.2, 0.5, 0.6, 1.0];
        let desired = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
        let h = remez(30, &bands, &desired, None).unwrap();
        for k in 0..16 {
            assert!((h[k] - reference[k]).abs() < 1e-6, "tap {k}: {} vs SciPy {}", h[k], reference[k]);
        }
        assert_equiripple(&h, &bands, &desired, &[1.0, 1.0, 1.0], 16);
    }

    #[test]
    fn remez_beats_a_hamming_window_design_of_the_same_order_in_the_stopband() {
        let order = 40;
        let bands = [0.0, 0.3, 0.4, 1.0];
        let desired = [1.0, 1.0, 0.0, 0.0];
        // Weight the stopband so the comparison is on attenuation.
        let h = remez(order, &bands, &desired, Some(&[1.0, 10.0])).unwrap();
        let delta = assert_equiripple(&h, &bands, &desired, &[1.0, 10.0], order / 2 + 1);
        let hamming: Vec<f64> = (0..=order).map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / order as f64).cos()).collect();
        let win = fir1(order, FilterKind::Low, &[0.35], 2.0, &hamming).unwrap();
        let (_, stop_remez) = fir_deviation(&h, &[(0.0, 0.3)], &[(0.4, 1.0)]);
        let (pass_win, stop_win) = fir_deviation(&win, &[(0.0, 0.3)], &[(0.4, 1.0)]);
        let (db_remez, db_win) = (-20.0 * stop_remez.log10(), -20.0 * stop_win.log10());
        assert!((stop_remez - delta / 10.0).abs() < 1e-3 * delta, "stopband ripple is the weighted deviation");
        assert!(db_remez > db_win + 10.0, "remez {db_remez} dB vs Hamming fir1 {db_win} dB");
        // ... and its passband is no worse than the window design's either.
        let (pass_remez, _) = fir_deviation(&h, &[(0.0, 0.3)], &[(0.4, 1.0)]);
        assert!(pass_remez < pass_win, "{pass_remez} vs {pass_win}");
    }

    #[test]
    fn remez_handles_a_long_filter_and_a_sloped_band() {
        let bands = [0.0, 0.2, 0.22, 1.0];
        let desired = [1.0, 1.0, 0.0, 0.0];
        let h = remez(400, &bands, &desired, None).unwrap();
        assert_equiripple(&h, &bands, &desired, &[1.0, 1.0], 201);
        // desired is linear across a band: a ramp from 0 to 1.
        let bands = [0.0, 0.6, 0.7, 1.0];
        let desired = [0.0, 1.0, 0.0, 0.0];
        let h = remez(24, &bands, &desired, None).unwrap();
        let delta = assert_equiripple(&h, &bands, &desired, &[1.0, 1.0], 13);
        assert!((amplitude(&h, 0.3) - 0.5).abs() <= delta * 1.001);
    }

    #[test]
    fn remez_rejects_what_it_cannot_build() {
        let lp = ([0.0, 0.3, 0.4, 1.0], [1.0, 1.0, 0.0, 0.0]);
        assert!(remez(0, &lp.0, &lp.1, None).is_err());
        assert!(remez(20, &[0.0, 0.3, 0.4], &[1.0, 1.0, 0.0], None).is_err());
        assert!(remez(20, &[0.0, 0.4, 0.3, 1.0], &lp.1, None).is_err());
        assert!(remez(20, &lp.0, &lp.1, Some(&[1.0])).is_err());
        assert!(remez(20, &lp.0, &lp.1, Some(&[1.0, 0.0])).is_err());
        // odd order with a passband reaching Nyquist
        let err = remez(21, &[0.0, 0.3, 0.4, 1.0], &[0.0, 0.0, 1.0, 1.0], None).unwrap_err();
        assert!(err.contains("Nyquist") && err.contains("order 22"), "{err}");
        assert!(remez(22, &[0.0, 0.3, 0.4, 1.0], &[0.0, 0.0, 1.0, 1.0], None).is_ok());
    }

    // ---- windows (references: scipy.signal.get_window(..., 9, fftbins=False))

    fn assert_close(a: &[f64], b: &[f64], tol: f64) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < tol, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn windows_match_their_definitions_and_scipy() {
        assert_close(&bartlett_window(5), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
        assert_close(&tukey_window(9, 0.5), &[0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0], 1e-15);
        assert_close(&tukey_window(6, 0.0), &[1.0; 6], 1e-15);
        // alpha = 1 is a Hann window
        assert_close(&tukey_window(5, 1.0), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
        // exp(-x^2/2) at x = 2, 1, 0
        let g = gaussian_window(5, 1.0);
        assert_close(&g, &[(-2.0f64).exp(), (-0.5f64).exp(), 1.0, (-0.5f64).exp(), (-2.0f64).exp()], 1e-15);
        assert_close(
            &gaussian_window(9, 1.5)[..5],
            &[0.02856550078455038, 0.1353352832366127, 0.41111229050718745, 0.8007374029168081, 1.0],
            1e-15,
        );
        assert_close(
            &general_cosine_window(9, &BLACKMANHARRIS)[..5],
            &[6.0e-05, 2.1735837018679628e-02, 2.1747e-01, 6.9576416298132038e-01, 1.0],
            1e-14,
        );
        assert_close(
            &general_cosine_window(9, &NUTTALL)[..5],
            &[3.628e-04, 2.5205566515401824e-02, 2.269824e-01, 7.0195823348459818e-01, 1.0],
            1e-14,
        );
        assert_close(
            &general_cosine_window(9, &FLATTOP)[..5],
            &[-4.21051e-04, -2.6872193286334545e-02, -5.473684e-02, 4.4413535728633452e-01, 1.000000003],
            1e-14,
        );
        // Hann through the general form: [0, 0.5, 1, 0.5, 0]
        assert_close(&general_cosine_window(5, &[0.5, 0.5]), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
        for w in [bartlett_window(1), tukey_window(1, 0.5), gaussian_window(1, 2.0), general_cosine_window(1, &NUTTALL)] {
            assert_eq!(w, vec![1.0]);
        }
        assert!(bartlett_window(0).is_empty());
    }
}
