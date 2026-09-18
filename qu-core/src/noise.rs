//! Noise, interference and distortion: putting them in, and taking them
//! out.
//!
//! # Why noise is specified in decibels
//!
//! "Add Gaussian noise of amplitude 0.1" is not a statement about a
//! measurement — it depends entirely on how big the signal is. "Add
//! Gaussian noise at 3 dB SNR" is, and it is what a datasheet, a standard
//! and a reviewer all speak. So [`add_noise`] takes a target SNR by
//! default and works out the amplitude from the signal it is given:
//!
//! ```text
//! SNR_dB = 10 * log10(P_signal / P_noise)
//!   =>  P_noise = P_signal / 10^(SNR_dB / 10)
//! ```
//!
//! A raw amplitude is still available, for when that genuinely is what you
//! have — a specified 2 mV of pickup, say.
//!
//! # The kinds, and what each one models
//!
//! Noise is not one thing, and using white noise for all of it is how a
//! method comes to look robust in simulation and fail on the bench.
//!
//! - **Gaussian / white** — thermal (Johnson-Nyquist) noise, and the
//!   central-limit sum of many small independent contributions. Flat
//!   spectrum. The default, and the right one surprisingly often.
//! - **Pink**, `1/f` — flicker noise in semiconductors, drift in sensors,
//!   `1/f` everywhere in nature. Dominates at low frequencies, which is
//!   why a slow measurement is harder than a fast one.
//! - **Brown**, `1/f²` — a random walk, the integral of white noise.
//!   Baseline wander.
//! - **Blue**, `f` — what dithering and noise-shaping produce, pushing
//!   noise power up where the ear and the passband are not.
//! - **Shot** — Poisson arrival of discrete quanta: photons, electrons
//!   across a junction. Its variance equals its mean, so it grows with
//!   signal, unlike everything above.
//! - **Salt and pepper** — dead and stuck pixels, bit errors. Not additive
//!   at all: it REPLACES samples, which is why a linear filter cannot
//!   remove it and a median filter can.
//! - **Impulse** — sparse large spikes: switching transients, ESD events.
//! - **Quantisation** — the error an ADC makes. Uniform over one LSB, and
//!   correlated with the signal unless dithered, which is why it sounds
//!   worse than its power suggests.
//!
//! # Interference is not noise
//!
//! Mains hum and EMI are *deterministic* — they have structure, and that
//! structure is what lets you remove them. [`hum`] puts in a mains tone
//! with harmonics, because real hum is never a pure sine: the pickup comes
//! through a nonlinearity somewhere and arrives with a third and fifth.
//! [`emf`] adds narrowband pickup plus impulsive bursts, which is what a
//! switching supply near a sensitive input actually looks like on a trace.

/// What went wrong, in terms of the measurement rather than the arithmetic.
#[derive(Debug, Clone, PartialEq)]
pub enum NoiseError {
    Empty,
    /// A parameter outside the range that means anything.
    BadParameter(String),
    /// A named kind that is not one of the ones implemented.
    UnknownKind(String),
    /// A window that is even, zero, or longer than the data.
    BadWindow { window: usize, len: usize, why: &'static str },
}

impl std::fmt::Display for NoiseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "the signal is empty"),
            Self::BadParameter(m) => write!(f, "{m}"),
            Self::UnknownKind(k) => write!(
                f,
                "`{k}` is not a kind of noise here -- gaussian, pink, brown, blue, uniform, \
                 shot, salt_pepper, impulse, quantize"
            ),
            Self::BadWindow { window, len, why } => {
                write!(f, "a window of {window} over {len} samples: {why}")
            }
        }
    }
}

impl std::error::Error for NoiseError {}

/// A small, seeded generator, so a noisy signal is reproducible.
///
/// xorshift64* rather than anything cryptographic: this is for making
/// figures and tests repeatable, and a generator whose stream depends on
/// the platform's is exactly what makes a bug irreproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // A zero state is a fixed point of xorshift, so it is mapped away
        // rather than left to produce an all-zero stream in silence.
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal, by Box-Muller.
    ///
    /// The polar form would avoid the trig, but this one is exact rather
    /// than rejection-based, so a given seed always consumes the same
    /// number of draws -- which is what keeps a seeded figure identical
    /// when an unrelated parameter changes.
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// Mean power, `E[x²]`.
///
/// Power, not variance: a signal with a DC offset carries that offset as
/// power, and an SNR measured against the variance would quietly ignore it.
pub fn power(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64
}

/// Signal-to-noise ratio in dB between a clean signal and a noisy version
/// of it.
///
/// The loop-closer: it lets "add noise at 3 dB" be checked rather than
/// trusted. The noise is taken as the difference, so this measures what
/// actually happened rather than what was asked for.
pub fn snr_db(clean: &[f64], noisy: &[f64]) -> Result<f64, NoiseError> {
    if clean.is_empty() || noisy.is_empty() {
        return Err(NoiseError::Empty);
    }
    if clean.len() != noisy.len() {
        return Err(NoiseError::BadParameter(format!(
            "snr: {} clean samples against {} noisy ones -- they must be the same signal",
            clean.len(),
            noisy.len()
        )));
    }
    let ps = power(clean);
    let diff: Vec<f64> = clean.iter().zip(noisy).map(|(c, n)| n - c).collect();
    let pn = power(&diff);
    if pn <= 0.0 {
        return Ok(f64::INFINITY);
    }
    if ps <= 0.0 {
        return Ok(f64::NEG_INFINITY);
    }
    Ok(10.0 * (ps / pn).log10())
}

/// How much noise to add, expressed either way.
#[derive(Debug, Clone, Copy)]
pub enum Level {
    /// Target signal-to-noise ratio in dB. The amplitude is worked out from
    /// the signal.
    SnrDb(f64),
    /// A raw standard deviation, for when that is what you were given.
    Amplitude(f64),
}

impl Level {
    /// The noise standard deviation this level asks for, on this signal.
    fn sigma(&self, x: &[f64]) -> f64 {
        match self {
            Level::Amplitude(a) => *a,
            Level::SnrDb(db) => {
                let ps = power(x);
                // P_noise = P_signal / 10^(SNR/10); sigma is its root.
                (ps / 10f64.powf(db / 10.0)).sqrt()
            }
        }
    }
}

/// Shape a white sequence to a `1/f^alpha` spectrum.
///
/// Voss-McCartney would be cheaper for pink specifically, but this handles
/// every exponent with one piece of code and is exact rather than an
/// approximation built from octave-spaced random holds — which matters
/// when the point of the noise is to test something that looks at the
/// spectrum.
///
/// Done by direct summation rather than an FFT, because the sequences here
/// are the length of a measurement record, not an image.
fn shaped(n: usize, alpha: f64, rng: &mut Rng) -> Vec<f64> {
    if n < 2 {
        return vec![rng.normal(); n];
    }
    // A white sequence, then re-weighted per frequency bin and inverted by
    // summation. O(n * bins) with bins capped, which is what keeps this
    // honest for a long record.
    let bins = (n / 2).clamp(1, 512);
    let mut out = vec![0.0; n];
    for k in 1..=bins {
        let f = k as f64 / n as f64;
        let gain = 1.0 / f.powf(alpha / 2.0);
        let phase = rng.uniform() * std::f64::consts::TAU;
        let amp = rng.normal() * gain;
        for (i, o) in out.iter_mut().enumerate() {
            *o += amp * (std::f64::consts::TAU * f * i as f64 + phase).cos();
        }
    }
    normalise(&mut out);
    out
}

/// Scale to unit standard deviation, so a shaped sequence can then be set
/// to whatever level was asked for.
fn normalise(v: &mut [f64]) {
    let n = v.len() as f64;
    if n == 0.0 {
        return;
    }
    let mean = v.iter().sum::<f64>() / n;
    let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    if sd > 0.0 {
        for x in v.iter_mut() {
            *x = (*x - mean) / sd;
        }
    }
}

/// Add noise of the named kind at the given level.
///
/// Works the same on a 1-D record and on the flattened pixels of an image:
/// every kind here is per-sample, and none of them needs to know the
/// neighbours. That is why one function covers both, rather than a `1d`
/// and a `2d` version that drift apart.
pub fn add_noise(
    x: &[f64],
    kind: &str,
    level: Level,
    seed: u64,
) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    // Four kinds are not linear in their parameter, so the closed-form
    // sigma that works for additive noise does not reach the target SNR
    // for them: shot noise scales with the signal, quantisation error is
    // bounded by the step, and salt-and-pepper's parameter is a corruption
    // RATE. Rather than let `snr = 3` mean three different things
    // depending on the kind, those are solved for by bisection -- the
    // achieved SNR is monotonic in the parameter for all four, so it
    // converges in a few dozen cheap evaluations.
    if matches!(level, Level::SnrDb(_))
        && matches!(
            kind,
            "shot" | "poisson" | "quantize" | "quantise" | "adc"
                | "salt_pepper" | "saltpepper" | "impulsive" | "impulse" | "spike"
        )
    {
        return solve_for_snr(x, kind, level, seed);
    }
    let mut rng = Rng::new(seed);
    let sigma = level.sigma(x);

    let out = match kind {
        "gaussian" | "white" | "normal" => {
            x.iter().map(|v| v + sigma * rng.normal()).collect()
        }
        "uniform" => {
            // Uniform on [-a, a] has variance a²/3, so a = sigma*sqrt(3)
            // gives the same POWER as a Gaussian of that sigma -- which is
            // what makes an SNR comparison between the two meaningful.
            let a = sigma * 3f64.sqrt();
            x.iter().map(|v| v + a * (2.0 * rng.uniform() - 1.0)).collect()
        }
        "pink" | "flicker" => {
            let n = shaped(x.len(), 1.0, &mut rng);
            x.iter().zip(&n).map(|(v, e)| v + sigma * e).collect()
        }
        "brown" | "brownian" | "red" => {
            let n = shaped(x.len(), 2.0, &mut rng);
            x.iter().zip(&n).map(|(v, e)| v + sigma * e).collect()
        }
        "blue" => {
            let n = shaped(x.len(), -1.0, &mut rng);
            x.iter().zip(&n).map(|(v, e)| v + sigma * e).collect()
        }
        // Shot noise is the one whose size depends on the signal: Poisson
        // arrivals, variance equal to mean. `sigma` sets the scale, i.e.
        // how many quanta one unit of signal is worth.
        "shot" | "poisson" => {
            let scale = if sigma > 0.0 { 1.0 / (sigma * sigma) } else { 1.0 };
            x.iter()
                .map(|v| {
                    let lambda = (v.abs() * scale).max(0.0);
                    // Gaussian approximation above 30, which is exact
                    // enough there and avoids a loop of thousands.
                    let draw = if lambda > 30.0 {
                        lambda + lambda.sqrt() * rng.normal()
                    } else {
                        poisson(lambda, &mut rng) as f64
                    };
                    let signed = if *v < 0.0 { -draw } else { draw };
                    signed / scale
                })
                .collect()
        }
        // Salt and pepper REPLACES samples rather than adding to them,
        // which is precisely why a linear filter cannot remove it. `sigma`
        // is read as the fraction affected, clamped to a probability.
        "salt_pepper" | "saltpepper" | "impulsive" => {
            let p = sigma.clamp(0.0, 1.0);
            let lo = x.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            x.iter()
                .map(|v| {
                    if rng.uniform() < p {
                        if rng.uniform() < 0.5 {
                            lo
                        } else {
                            hi
                        }
                    } else {
                        *v
                    }
                })
                .collect()
        }
        // Sparse large spikes: a switching transient, an ESD event. One in
        // a hundred samples by default, at several times the signal's own
        // amplitude -- the shape that defeats an average and not a median.
        "impulse" | "spike" => {
            let p = 0.01;
            // The scale IS the parameter. An earlier version floored it at
            // six times the signal RMS to guarantee the spikes looked
            // impulsive -- which made the level saturate, so a requested
            // SNR could not be reached. It was also unnecessary: with one
            // sample in a hundred affected, hitting even 6 dB requires each
            // spike to be about five times the RMS, so the arithmetic
            // produces large spikes on its own.
            let scale = sigma;
            x.iter()
                .map(|v| {
                    if rng.uniform() < p {
                        v + scale * if rng.uniform() < 0.5 { -1.0 } else { 1.0 }
                    } else {
                        *v
                    }
                })
                .collect()
        }
        // Quantisation: `sigma` is the step. The error is uniform over one
        // LSB and CORRELATED with the signal, which is why it sounds worse
        // than its power suggests and why dither exists.
        "quantize" | "quantise" | "adc" => {
            let step = if sigma > 0.0 { sigma } else { 1.0 };
            x.iter().map(|v| (v / step).round() * step).collect()
        }
        other => return Err(NoiseError::UnknownKind(other.to_string())),
    };
    Ok(out)
}


/// Find the parameter that reaches a requested SNR, for the kinds where it
/// cannot be computed in closed form.
///
/// Bisection on the internal amplitude/rate. The achieved SNR falls
/// monotonically as the parameter grows for every kind this is used on, so
/// the bracket is found by doubling and then halved 60 times -- which is
/// far more precision than the target is ever specified to, and still
/// costs less than a millisecond on a record of any sane length.
fn solve_for_snr(
    x: &[f64],
    kind: &str,
    level: Level,
    seed: u64,
) -> Result<Vec<f64>, NoiseError> {
    let Level::SnrDb(target) = level else {
        unreachable!("only called for an SNR target");
    };
    let achieved = |p: f64| -> Result<f64, NoiseError> {
        let y = add_noise(x, kind, Level::Amplitude(p), seed)?;
        snr_db(x, &y)
    };

    // A bracket: `lo` gives more SNR than asked, `hi` gives less.
    let mut lo = 1e-9;
    let mut hi = 1.0;
    let mut guard = 0;
    while achieved(hi)? > target && guard < 60 {
        hi *= 2.0;
        guard += 1;
    }
    while achieved(lo)? < target && guard < 120 {
        lo /= 2.0;
        guard += 1;
        if lo < 1e-300 {
            break;
        }
    }

    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if achieved(mid)? > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    add_noise(x, kind, Level::Amplitude(0.5 * (lo + hi)), seed)
}

/// A Poisson draw by Knuth's method. Only used below 30, where the loop is
/// short; above that the caller uses the Gaussian approximation.
fn poisson(lambda: f64, rng: &mut Rng) -> u32 {
    let l = (-lambda).exp();
    let mut k = 0u32;
    let mut p = 1.0;
    loop {
        p *= rng.uniform();
        if p <= l || k > 1000 {
            return k;
        }
        k += 1;
    }
}


/// What a converter did to a signal.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// The quantised signal.
    pub x: Vec<f64>,
    /// One least-significant bit, in the signal's own units.
    pub lsb: f64,
    /// How many samples fell outside the reference range and were clipped.
    ///
    /// Not a warning to be ignored: a converter that clips is not measuring
    /// any more, and a script that does not look at this number will
    /// happily report a flat-topped waveform as data.
    pub clipped: usize,
    /// The SNR an ideal converter of this word length would give:
    /// `6.02 * bits + 1.76` dB, for a full-scale sine.
    ///
    /// The number to compare a measurement against. Falling short of it
    /// means something else is dominating; beating it means the signal is
    /// not full-scale, or the arithmetic is wrong.
    pub snr_ideal_db: f64,
}

/// One converter code, in the signal's own units: `(vmax - vmin) / 2^bits`.
///
/// Pulled out as its own function so the converter *simulator* ([`adc`])
/// and the converter *detector* ([`crate::diagnostics::saturation`]) cannot
/// drift into two slightly different converters. The divisor is `2^bits`,
/// the number of codes — not `2^bits - 1`, which is the off-by-one that
/// puts every reading half an LSB out.
///
/// The code geometry that follows from it: codes run `0 ..= 2^bits - 1`, so
/// the bottom code is `vmin` and the **top code is `vmax - lsb`**, not
/// `vmax`. A saturation check that looks for samples at `vmax` finds none,
/// ever.
pub fn code_step(bits: u32, vmin: f64, vmax: f64) -> f64 {
    (vmax - vmin) / 2f64.powi(bits as i32)
}

/// Quantise a signal the way an instrument does: a reference range and a
/// word length, not a step size.
///
/// `add_noise(x, "quantize", ...)` takes a raw step, which is the
/// mathematician's parameter. A converter has a **reference range** and a
/// **resolution**, and the difference is not cosmetic: outside the range it
/// CLIPS, and a step-based model cannot express that at all — it will
/// happily quantise a signal ten times the converter's range and report no
/// problem.
///
/// ```text
/// LSB = (vmax - vmin) / 2^bits
/// ```
///
/// A `bits`-bit converter has `2^bits` codes across the range, so the LSB
/// is the range divided by that — not by `2^bits - 1`, which is the
/// off-by-one that puts every reading half an LSB out.
///
/// **Dither** adds a triangular (TPDF) noise of two LSB peak-to-peak before
/// quantising. It raises the noise floor slightly and, in exchange,
/// decorrelates the error from the signal: undithered quantisation error is
/// a *function* of the signal, so it appears as harmonics rather than as
/// noise, and no amount of averaging removes it. Dithered, it becomes
/// genuine noise, and averaging then buys resolution — which is how a
/// 12-bit converter is made to resolve better than 12 bits.
pub fn adc(
    x: &[f64],
    bits: u32,
    vmin: f64,
    vmax: f64,
    dither: bool,
    seed: u64,
) -> Result<Conversion, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if !(1..=32).contains(&bits) {
        return Err(NoiseError::BadParameter(format!(
            "adc: {bits} bits is outside 1 to 32 -- beyond that the step is below what a \
             double can represent against the reference"
        )));
    }
    if !(vmax > vmin) {
        return Err(NoiseError::BadParameter(format!(
            "adc: the reference range is empty -- vmin {vmin} is not below vmax {vmax}"
        )));
    }
    let levels = 2f64.powi(bits as i32);
    let lsb = code_step(bits, vmin, vmax);
    let mut rng = Rng::new(seed);
    let mut clipped = 0usize;

    let out: Vec<f64> = x
        .iter()
        .map(|v| {
            // Dither goes in BEFORE the converter, because that is where it
            // goes in a real instrument -- it is added to the analogue
            // signal, not to the codes.
            let sample = if dither {
                // TPDF: the sum of two independent uniforms, which is the
                // distribution that makes both the mean AND the variance of
                // the error independent of the signal. A single uniform
                // fixes the mean only.
                v + lsb * (rng.uniform() - 0.5 + rng.uniform() - 0.5)
            } else {
                *v
            };
            if sample < vmin || sample > vmax {
                clipped += 1;
            }
            let clamped = sample.clamp(vmin, vmax);
            // Round to the nearest code, then back to a voltage. The top
            // code is `levels - 1`, so a full-scale input lands on it
            // rather than one past the end.
            let code = ((clamped - vmin) / lsb).round().min(levels - 1.0).max(0.0);
            vmin + code * lsb
        })
        .collect();

    Ok(Conversion {
        x: out,
        lsb,
        clipped,
        // 6.02 dB per bit plus 1.76, the standard result for a full-scale
        // sine against uniform quantisation error.
        snr_ideal_db: 6.02 * bits as f64 + 1.76,
    })
}

/// Mains hum: a tone at `freq` with its harmonics.
///
/// Real hum is never a pure sine. The pickup arrives through a
/// nonlinearity somewhere — a rectifier, a saturating core — so it carries
/// odd harmonics, and a notch at 50 Hz alone leaves the 150 and the 250
/// behind. That is why `harmonics` defaults to 3 rather than 1: a model
/// that omits them makes every notch filter look better than it is.
///
/// Amplitude falls as `1/h`, which is the shape a clipped or rectified
/// waveform actually has.
pub fn hum(
    x: &[f64],
    fs: f64,
    freq: f64,
    harmonics: usize,
    level: Level,
    seed: u64,
) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if !(fs > 0.0) {
        return Err(NoiseError::BadParameter(format!(
            "hum: the sample rate must be positive, found {fs}"
        )));
    }
    if !(freq > 0.0) || freq >= fs / 2.0 {
        return Err(NoiseError::BadParameter(format!(
            "hum: {freq} Hz is not below the Nyquist limit of {} Hz",
            fs / 2.0
        )));
    }
    let mut rng = Rng::new(seed);
    // Build the interference at unit power first, then scale it, so the
    // level means the same whatever the harmonic content.
    let mut inter = vec![0.0; x.len()];
    // ODD harmonics: 1st, 3rd, 5th. Mains pickup arrives through a
    // symmetric nonlinearity -- a rectifier, a saturating core -- and a
    // symmetric nonlinearity generates odd harmonics only. `harmonics = 3`
    // therefore means 50, 150 and 250 Hz, not 50, 100 and 150.
    for k in 0..harmonics.max(1) {
        let h = 2 * k + 1;
        let f = freq * h as f64;
        if f >= fs / 2.0 {
            break;
        }
        let amp = 1.0 / h as f64;
        let phase = rng.uniform() * std::f64::consts::TAU;
        for (i, v) in inter.iter_mut().enumerate() {
            *v += amp * (std::f64::consts::TAU * f * i as f64 / fs + phase).sin();
        }
    }
    normalise(&mut inter);
    let sigma = level.sigma(x);
    Ok(x.iter().zip(&inter).map(|(v, e)| v + sigma * e).collect())
}

/// Electromagnetic interference: narrowband pickup plus impulsive bursts.
///
/// What a switching supply near a sensitive input actually does, and it is
/// two things at once, which is the part a single-tone model misses:
///
///   - a carrier at the switching frequency with sidebands, from
///     capacitive and inductive coupling;
///   - short bursts at the switching edges, from the `di/dt` of the
///     commutation itself.
///
/// A method tested only against the carrier will fail on the bursts, and
/// vice versa, which is the reason to model both.
pub fn emf(
    x: &[f64],
    fs: f64,
    carrier: f64,
    burst_rate: f64,
    level: Level,
    seed: u64,
) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if !(fs > 0.0) {
        return Err(NoiseError::BadParameter(format!(
            "emf: the sample rate must be positive, found {fs}"
        )));
    }
    if !(carrier > 0.0) || carrier >= fs / 2.0 {
        return Err(NoiseError::BadParameter(format!(
            "emf: a {carrier} Hz carrier is not below the Nyquist limit of {} Hz",
            fs / 2.0
        )));
    }
    let mut rng = Rng::new(seed);
    let n = x.len();
    let mut inter = vec![0.0; n];

    // The carrier, amplitude-modulated slowly: coupling is never constant,
    // because the loop area and the load both move.
    let phase = rng.uniform() * std::f64::consts::TAU;
    let mod_f = carrier / 137.0;
    for (i, v) in inter.iter_mut().enumerate() {
        let t = i as f64 / fs;
        let envelope = 0.7 + 0.3 * (std::f64::consts::TAU * mod_f * t).sin();
        *v += envelope * (std::f64::consts::TAU * carrier * t + phase).sin();
    }

    // The bursts: a damped ring at each switching edge, which is what the
    // parasitic inductance and capacitance of the loop actually produce.
    if burst_rate > 0.0 {
        let period = (fs / burst_rate).max(1.0) as usize;
        let ring_f = carrier * 7.0;
        let decay = fs / (carrier * 0.8);
        let mut at = (rng.uniform() * period as f64) as usize;
        while at < n {
            let amp = 2.0 + 2.0 * rng.uniform();
            for j in 0..(decay as usize * 3).min(n - at) {
                let t = j as f64 / fs;
                let env = (-(j as f64) / decay).exp();
                if env < 1e-3 {
                    break;
                }
                if ring_f < fs / 2.0 {
                    inter[at + j] += amp * env * (std::f64::consts::TAU * ring_f * t).sin();
                }
            }
            at += period;
        }
    }

    normalise(&mut inter);
    let sigma = level.sigma(x);
    Ok(x.iter().zip(&inter).map(|(v, e)| v + sigma * e).collect())
}

/// Distortion: the signal changed by a nonlinearity, not something added
/// to it.
///
/// The distinction matters. Noise is independent of the signal and
/// averaging reduces it; distortion is a *function* of the signal, so
/// averaging does nothing at all and the only cure is not to distort.
///
/// `amount` runs 0 to 1 for every kind, so they can be compared.
pub fn distort(x: &[f64], kind: &str, amount: f64) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if !(0.0..=1.0).contains(&amount) {
        return Err(NoiseError::BadParameter(format!(
            "distort: the amount runs 0 to 1, found {amount}"
        )));
    }
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if peak == 0.0 {
        return Ok(x.to_vec());
    }

    let out = match kind {
        // Hard clipping: the amplifier ran out of rail. Generates odd
        // harmonics abruptly, which is why it sounds harsh and shows as a
        // comb in the spectrum.
        "clip" | "hard" => {
            let limit = peak * (1.0 - 0.9 * amount);
            x.iter().map(|v| v.clamp(-limit, limit)).collect()
        }
        // Soft saturation: a tanh curve, what a transformer or a valve
        // does. The same odd harmonics, arriving gradually.
        "soft" | "saturate" | "tanh" => {
            let drive = 1.0 + 9.0 * amount;
            let norm = (drive).tanh();
            x.iter().map(|v| (v / peak * drive).tanh() / norm * peak).collect()
        }
        // Crossover: a dead zone at zero, from a class-B output stage
        // whose two halves do not meet. Worst on SMALL signals, which is
        // the opposite of clipping and the reason it is measured
        // separately.
        "crossover" => {
            let dead = peak * 0.25 * amount;
            x.iter()
                .map(|v| {
                    if v.abs() < dead {
                        0.0
                    } else if *v > 0.0 {
                        v - dead
                    } else {
                        v + dead
                    }
                })
                .collect()
        }
        // Harmonic distortion by a cubic term: the classic weakly
        // nonlinear amplifier. Produces a third harmonic whose amplitude
        // is what THD measures.
        "harmonic" | "cubic" => {
            let k = 0.5 * amount;
            x.iter().map(|v| {
                let u = v / peak;
                (u - k * u * u * u) / (1.0 - k) * peak
            }).collect()
        }
        // Quantisation as a distortion rather than as noise: `amount`
        // chooses the word length, 16 bits down to 2.
        "quantize" | "quantise" => {
            let bits = (16.0 - 14.0 * amount).round().max(2.0);
            let levels = 2f64.powf(bits) - 1.0;
            let step = 2.0 * peak / levels;
            x.iter().map(|v| (v / step).round() * step).collect()
        }
        other => {
            return Err(NoiseError::UnknownKind(format!(
                "{other}` is not a distortion here -- clip, soft, crossover, harmonic, quantize`"
            )))
        }
    };
    Ok(out)
}

// ── removal ────────────────────────────────────────────────────────────

/// Median filter, 1-D.
///
/// The right answer to salt-and-pepper, and the reason is worth stating: an
/// average is pulled by an outlier in proportion to how far out it is,
/// while a median ignores it entirely as long as fewer than half the window
/// is corrupt. That is also why it preserves an edge where a moving average
/// smears it — a step is not an outlier to a median.
///
/// The window must be odd, so there is a middle sample; an even window is
/// refused rather than silently rounded, because which way it rounds
/// changes the answer.
pub fn medfilt(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: x.len(),
            why: "the window must be odd, so there is a middle sample",
        });
    }
    let half = window / 2;
    let mut out = Vec::with_capacity(x.len());
    let mut buf = Vec::with_capacity(window);
    for i in 0..x.len() {
        buf.clear();
        // Edges reflect rather than zero-pad: padding with zeros invents a
        // step at each end, and the filter then faithfully preserves it.
        for k in 0..window {
            let j = (i + k) as isize - half as isize;
            let j = if j < 0 {
                (-j) as usize
            } else if j as usize >= x.len() {
                2 * x.len() - 2 - j as usize
            } else {
                j as usize
            };
            buf.push(x[j.min(x.len() - 1)]);
        }
        buf.sort_by(f64::total_cmp);
        out.push(buf[half]);
    }
    Ok(out)
}

/// Median filter, 2-D, over a square window.
///
/// Separable it is not: the median of medians is not the median, so this
/// does the real thing over the whole window. `window` is the side length
/// and must be odd.
pub fn medfilt2(
    pixels: &[f64],
    width: usize,
    height: usize,
    window: usize,
) -> Result<Vec<f64>, NoiseError> {
    if pixels.is_empty() || width == 0 || height == 0 {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: pixels.len(),
            why: "the window must be odd, so there is a middle sample",
        });
    }
    let half = (window / 2) as isize;
    let mut out = vec![0.0; pixels.len()];
    let mut buf = Vec::with_capacity(window * window);
    for y in 0..height as isize {
        for x in 0..width as isize {
            buf.clear();
            for dy in -half..=half {
                for dx in -half..=half {
                    // Clamped at the border, which is the convention every
                    // image library uses and the only one that cannot
                    // invent an edge.
                    let sy = (y + dy).clamp(0, height as isize - 1) as usize;
                    let sx = (x + dx).clamp(0, width as isize - 1) as usize;
                    buf.push(pixels[sy * width + sx]);
                }
            }
            buf.sort_by(f64::total_cmp);
            out[y as usize * width + x as usize] = buf[buf.len() / 2];
        }
    }
    Ok(out)
}

/// Savitzky-Golay smoothing: a least-squares polynomial fit over a sliding
/// window.
///
/// The smoother to use when the shape matters. A moving average flattens a
/// peak — it is a low-pass filter and a peak is high-frequency — while this
/// fits a polynomial locally and so preserves the height and width of a
/// peak while removing the noise around it. That is why it is the default
/// in every spectroscopy package.
///
/// `order` must be less than `window`, or the fit is underdetermined.
pub fn savgol(x: &[f64], window: usize, order: usize) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: x.len(),
            why: "the window must be odd, so it is centred on a sample",
        });
    }
    if order >= window {
        return Err(NoiseError::BadParameter(format!(
            "savgol: order {order} needs a window longer than {order}, and this one is {window} \
             -- the polynomial would pass through every point and smooth nothing"
        )));
    }
    let half = (window / 2) as isize;
    // The convolution coefficients: row 0 of (A'A)^-1 A', where A is the
    // Vandermonde matrix of the window offsets. Computed once.
    let m = order + 1;
    let mut ata = vec![0.0; m * m];
    for i in 0..m {
        for j in 0..m {
            ata[i * m + j] = (-half..=half).map(|t| (t as f64).powi((i + j) as i32)).sum();
        }
    }
    // Solve (A'A) c = e0 by Gaussian elimination with partial pivoting.
    let mut aug = vec![0.0; m * (m + 1)];
    for i in 0..m {
        for j in 0..m {
            aug[i * (m + 1) + j] = ata[i * m + j];
        }
        aug[i * (m + 1) + m] = if i == 0 { 1.0 } else { 0.0 };
    }
    for col in 0..m {
        let pivot = (col..m)
            .max_by(|a, b| {
                aug[a * (m + 1) + col].abs().total_cmp(&aug[b * (m + 1) + col].abs())
            })
            .unwrap_or(col);
        if aug[pivot * (m + 1) + col].abs() < 1e-300 {
            return Err(NoiseError::BadParameter(
                "savgol: the fit is singular for this window and order".into(),
            ));
        }
        for k in 0..=m {
            aug.swap(col * (m + 1) + k, pivot * (m + 1) + k);
        }
        let d = aug[col * (m + 1) + col];
        for k in col..=m {
            aug[col * (m + 1) + k] /= d;
        }
        for row in 0..m {
            if row != col {
                let f = aug[row * (m + 1) + col];
                for k in col..=m {
                    aug[row * (m + 1) + k] -= f * aug[col * (m + 1) + k];
                }
            }
        }
    }
    let c: Vec<f64> = (0..m).map(|i| aug[i * (m + 1) + m]).collect();
    let coeffs: Vec<f64> = (-half..=half)
        .map(|t| (0..m).map(|p| c[p] * (t as f64).powi(p as i32)).sum())
        .collect();

    let mut out = Vec::with_capacity(x.len());
    for i in 0..x.len() {
        let mut acc = 0.0;
        for (k, w) in coeffs.iter().enumerate() {
            let j = (i + k) as isize - half;
            let j = j.clamp(0, x.len() as isize - 1) as usize;
            acc += w * x[j];
        }
        out.push(acc);
    }
    Ok(out)
}

/// Moving average over an odd window, reflecting at the edges.
pub fn moving_average(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: x.len(),
            why: "the window must be odd, so it is centred on a sample",
        });
    }
    let half = (window / 2) as isize;
    let mut out = Vec::with_capacity(x.len());
    for i in 0..x.len() {
        let mut acc = 0.0;
        for k in -half..=half {
            let j = (i as isize + k).clamp(0, x.len() as isize - 1) as usize;
            acc += x[j];
        }
        out.push(acc / window as f64);
    }
    Ok(out)
}

/// Hampel: replace a sample by the local median when it lies more than
/// `n_sigma` robust deviations from it.
///
/// An outlier remover rather than a smoother. Everything the test does not
/// flag is left exactly as it was, which is the difference from a median
/// filter — that one rewrites every sample whether it needed it or not.
///
/// The scale is `1.4826 * MAD`, the constant that makes the median absolute
/// deviation an unbiased estimate of the standard deviation for Gaussian
/// data, so `n_sigma` means what it says.
pub fn hampel(
    x: &[f64],
    window: usize,
    n_sigma: f64,
) -> Result<(Vec<f64>, usize), NoiseError> {
    let (flagged, local_median) = hampel_flags(x, window, n_sigma)?;
    let mut out = x.to_vec();
    let mut replaced = 0usize;
    for i in 0..x.len() {
        if flagged[i] {
            out[i] = local_median[i];
            replaced += 1;
        }
    }
    Ok((out, replaced))
}

/// The Hampel *test*, without the replacement: which samples the criterion
/// flags, and the local median each was compared against.
///
/// Split out of `hampel` so `find_outliers`/`remove_outliers`/
/// `replace_outliers` can offer `method="hampel"` without a second copy of
/// the test drifting away from this one -- `hampel` above is now a thin
/// caller of it, so the two cannot disagree about what an outlier is.
///
/// Returns `(flagged, local_median)`, both length-N. Unlike the global
/// criteria in `outlier_flags`, this one is *local*: it compares each
/// sample against a window centred on it, so it flags a spike riding on a
/// baseline that itself drifts far further than the spike does.
pub fn hampel_flags(
    x: &[f64],
    window: usize,
    n_sigma: f64,
) -> Result<(Vec<bool>, Vec<f64>), NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: x.len(),
            why: "the window must be odd, so it is centred on a sample",
        });
    }
    if !(n_sigma > 0.0) {
        return Err(NoiseError::BadParameter(format!(
            "n_sigma must be positive, found {n_sigma}"
        )));
    }
    let half = (window / 2) as isize;
    let mut flagged = vec![false; x.len()];
    let mut medians = vec![0.0; x.len()];
    let mut buf = Vec::with_capacity(window);
    for i in 0..x.len() {
        buf.clear();
        for k in -half..=half {
            let j = (i as isize + k).clamp(0, x.len() as isize - 1) as usize;
            buf.push(x[j]);
        }
        buf.sort_by(f64::total_cmp);
        let med = buf[buf.len() / 2];
        let mut dev: Vec<f64> = buf.iter().map(|v| (v - med).abs()).collect();
        dev.sort_by(f64::total_cmp);
        let mad = dev[dev.len() / 2];
        let sigma = 1.4826 * mad;
        medians[i] = med;
        flagged[i] = sigma > 0.0 && (x[i] - med).abs() > n_sigma * sigma;
    }
    Ok((flagged, medians))
}

/// Remove a least-squares polynomial trend.
///
/// `order` 0 removes the mean, 1 a linear drift, 2 a bow. Baseline wander
/// is what brown noise looks like after the fact, and removing it before
/// anything else is usually the first step of an analysis.
pub fn detrend(x: &[f64], order: usize) -> Result<Vec<f64>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if order >= x.len() {
        return Err(NoiseError::BadParameter(format!(
            "detrend: order {order} over {} samples would fit them exactly and leave nothing",
            x.len()
        )));
    }
    let n = x.len();
    let m = order + 1;
    // Normal equations on the Vandermonde matrix of the sample index,
    // scaled to [-1, 1] so a high order does not overflow.
    let t: Vec<f64> = (0..n)
        .map(|i| if n > 1 { 2.0 * i as f64 / (n - 1) as f64 - 1.0 } else { 0.0 })
        .collect();
    let mut ata = vec![0.0; m * m];
    let mut atb = vec![0.0; m];
    for i in 0..m {
        for j in 0..m {
            ata[i * m + j] = t.iter().map(|v| v.powi((i + j) as i32)).sum();
        }
        atb[i] = t.iter().zip(x).map(|(v, y)| v.powi(i as i32) * y).sum();
    }
    // Gaussian elimination.
    let mut aug = vec![0.0; m * (m + 1)];
    for i in 0..m {
        for j in 0..m {
            aug[i * (m + 1) + j] = ata[i * m + j];
        }
        aug[i * (m + 1) + m] = atb[i];
    }
    for col in 0..m {
        let pivot = (col..m)
            .max_by(|a, b| {
                aug[a * (m + 1) + col].abs().total_cmp(&aug[b * (m + 1) + col].abs())
            })
            .unwrap_or(col);
        if aug[pivot * (m + 1) + col].abs() < 1e-300 {
            return Err(NoiseError::BadParameter("detrend: the fit is singular".into()));
        }
        for k in 0..=m {
            aug.swap(col * (m + 1) + k, pivot * (m + 1) + k);
        }
        let d = aug[col * (m + 1) + col];
        for k in col..=m {
            aug[col * (m + 1) + k] /= d;
        }
        for row in 0..m {
            if row != col {
                let f = aug[row * (m + 1) + col];
                for k in col..=m {
                    aug[row * (m + 1) + k] -= f * aug[col * (m + 1) + k];
                }
            }
        }
    }
    let c: Vec<f64> = (0..m).map(|i| aug[i * (m + 1) + m]).collect();
    Ok(x
        .iter()
        .zip(&t)
        .map(|(y, v)| y - (0..m).map(|p| c[p] * v.powi(p as i32)).sum::<f64>())
        .collect())
}

// ── rolling statistics ─────────────────────────────────────────────────
//
// A rolling statistic and a smoothing filter look alike and are not the
// same thing, and the difference decides the edge convention.
//
// `medfilt`/`savgol`/`moving_average` above are FILTERS: they answer "what
// does this signal look like with the noise taken out", and they pad at the
// edges (reflect, or clamp) because a filter has to produce an output for
// every input sample and padding is the least-bad way to invent the
// neighbours it does not have.
//
// These are STATISTICS: they answer "what was the mean/spread/range of the
// data actually in this window". Padding would answer that question with
// fabricated samples, and for the spread statistics it does not merely add
// a little edge error -- it biases them the wrong way, hard. Clamping
// repeats the endpoint, and repeated identical values have zero variance,
// so a clamp-padded `rolling_std` reports the signal getting *quieter* at
// exactly the two places nothing is known about it. Reflecting is no better
// (a mirrored sample is perfectly correlated with its original).
//
// So the window SHRINKS at the edges: near an endpoint the statistic is
// taken over however much of the window actually overlaps the data, and
// nothing is invented. This is MATLAB's `movmean`/`movstd`/`movmin`/
// `movmax` default (`Endpoints="shrink"`) and pandas' `min_periods=1`, i.e.
// the convention the rolling-statistic family has everywhere else, and the
// deliberate, documented divergence from the filters just above.
//
// The guarantee this buys, and the one the tests check: for every index i,
// `rolling_<stat>(x, w)[i]` is exactly `<stat>` of the sub-slice of `x` that
// the window covers -- including the shrunken ones. There is no index at
// which the answer is a statistic of something other than real data.
//
// The window must be odd, same as every other windowed function in this
// file and for the same reason: an even window has no centre sample, and
// which way it rounds changes the answer.

/// The half-open sub-slice of `x` that a `window`-wide window centred on
/// `i` actually covers, shrunk at the edges rather than padded.
fn window_slice(x: &[f64], i: usize, half: usize) -> &[f64] {
    let lo = i.saturating_sub(half);
    let hi = (i + half + 1).min(x.len());
    &x[lo..hi]
}

/// Shared validation for the rolling family; returns the window's half-width.
fn check_rolling(x: &[f64], window: usize) -> Result<usize, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if window == 0 || window % 2 == 0 {
        return Err(NoiseError::BadWindow {
            window,
            len: x.len(),
            why: "the window must be odd, so it is centred on a sample",
        });
    }
    Ok(window / 2)
}

/// Rolling mean over a centred, odd window; the window shrinks at the edges.
pub fn rolling_mean(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    let half = check_rolling(x, window)?;
    Ok((0..x.len())
        .map(|i| {
            let w = window_slice(x, i, half);
            w.iter().sum::<f64>() / w.len() as f64
        })
        .collect())
}

/// Rolling root-mean-square over a centred, odd window.
///
/// Note this is the RMS of the samples themselves, *not* of their deviation
/// from the local mean -- a moving-RMS trigger level wants the signal's
/// actual magnitude, DC included. Subtract the baseline first (`detrend`)
/// if the AC part is what is wanted; `rolling_std` is the already-centred
/// counterpart.
pub fn rolling_rms(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    let half = check_rolling(x, window)?;
    Ok((0..x.len())
        .map(|i| {
            let w = window_slice(x, i, half);
            (w.iter().map(|v| v * v).sum::<f64>() / w.len() as f64).sqrt()
        })
        .collect())
}

/// Rolling standard deviation over a centred, odd window.
///
/// The SAMPLE (N-1, unbiased) deviation, matching the engine's own `std`/
/// `var` rather than the population form, so `rolling_std(x, w)[i]` and
/// `std(<that window>)` agree exactly -- which is a property a script can
/// check, and would quietly not hold if this used N.
///
/// A window that covers a single sample yields `0.0`, not NaN, again
/// because that is what `std` of one sample already returns here.
pub fn rolling_std(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    let half = check_rolling(x, window)?;
    Ok((0..x.len())
        .map(|i| {
            let w = window_slice(x, i, half);
            if w.len() < 2 {
                return 0.0;
            }
            let m = w.iter().sum::<f64>() / w.len() as f64;
            (w.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (w.len() as f64 - 1.0)).sqrt()
        })
        .collect())
}

/// Rolling minimum over a centred, odd window.
pub fn rolling_min(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    let half = check_rolling(x, window)?;
    Ok((0..x.len())
        .map(|i| window_slice(x, i, half).iter().copied().fold(f64::INFINITY, f64::min))
        .collect())
}

/// Rolling maximum over a centred, odd window.
pub fn rolling_max(x: &[f64], window: usize) -> Result<Vec<f64>, NoiseError> {
    let half = check_rolling(x, window)?;
    Ok((0..x.len())
        .map(|i| {
            window_slice(x, i, half)
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect())
}

// ── outliers ───────────────────────────────────────────────────────────

/// Median of a slice, by the usual "lower of the two middles for an even
/// count" order-statistic convention used elsewhere in this file.
fn median_of(sorted: &[f64]) -> f64 {
    sorted[sorted.len() / 2]
}

fn sorted_copy(x: &[f64]) -> Vec<f64> {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

/// A quantile by linear interpolation between order statistics (the
/// "type 7"/`numpy.percentile` default), so Q1/Q3 of a short vector are not
/// pinned to whichever sample happens to sit nearest the index.
fn quantile_of(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (pos - lo as f64) * (sorted[hi] - sorted[lo])
}

/// The threshold that `method` means when the caller did not name one.
///
/// Deliberately per-method rather than one shared number: 3 sigma, 3.5
/// modified-z and 1.5 IQR are the conventional cutoffs for three *different*
/// scales, and forcing them to share a default would silently make two of
/// the three mean something nobody intends.
pub fn default_outlier_threshold(method: &str) -> f64 {
    match method {
        "modified_zscore" | "modified_z" | "mzscore" => 3.5,
        "iqr" | "tukey" => 1.5,
        // zscore, hampel
        _ => 3.0,
    }
}

/// Which samples of `x` a global outlier criterion flags.
///
/// `method` is one of:
///
/// - `"zscore"` — `|x - mean| / std > threshold`. The textbook test, and
///   the one to distrust on dirty data: the outlier is itself in the mean
///   and the deviation it is being measured against, so a big enough spike
///   inflates `std` until it stops looking like a spike (the masking
///   effect). Fine for a handful of mild outliers, wrong for a stuck
///   channel reading 1e6.
///
///   It also has a HARD CEILING that catches people out and is worth
///   stating: over N samples no z-score can exceed `(N-1)/sqrt(N)`, because
///   a single sample can only be so far from a mean it is itself part of.
///   At the default threshold of 3 that means **fewer than 11 samples can
///   never produce an outlier at all** (`9/sqrt(10) = 2.85 < 3`), and at 11
///   the maximum possible score is `3.015` — so a short record answers
///   "no outliers" by arithmetic rather than by evidence. The robust
///   criteria have no such ceiling. Measured, not derived from memory:
///   an 11-sample vector with one value of 100 scores exactly 3.0.
/// - `"modified_zscore"` — `0.6745 * |x - median| / MAD > threshold`
///   (Iglewicz & Hoaglin). Same idea on robust estimators, so a spike
///   cannot hide itself. `0.6745` is the constant that makes MAD estimate
///   the standard deviation for Gaussian data.
/// - `"iqr"` — outside `[Q1 - k*IQR, Q3 + k*IQR]` (Tukey's fences). Makes
///   no distributional assumption at all; the one to reach for on skewed
///   data, where both z-scores over-flag the long tail.
///
/// `"hampel"` is handled by `hampel_flags` instead, because it is a LOCAL
/// test (a window) rather than a global one and needs a window parameter
/// these three do not have.
///
/// A NaN sample is never flagged here: a missing sample is not an outlier,
/// it is a missing sample, and conflating the two would have
/// `remove_outliers` quietly double as a NaN filter. `find_missing`/
/// `remove_nan` are the functions for that.
pub fn outlier_flags(
    x: &[f64],
    method: &str,
    threshold: f64,
) -> Result<Vec<bool>, NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    if !(threshold > 0.0) {
        return Err(NoiseError::BadParameter(format!(
            "threshold must be positive, found {threshold}"
        )));
    }
    // Only the finite samples define the criterion; a NaN would poison a
    // mean or a sort and make every sample look like an outlier.
    let finite: Vec<f64> = x.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return Ok(vec![false; x.len()]);
    }
    match method {
        "zscore" | "z" | "std" => {
            let n = finite.len() as f64;
            let mean = finite.iter().sum::<f64>() / n;
            let sd = if finite.len() < 2 {
                0.0
            } else {
                (finite.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0)).sqrt()
            };
            // A constant vector has sd 0 and no outliers -- without this
            // guard every sample divides 0/0 and comes back NaN > t, i.e.
            // false, which is the right answer by luck rather than by
            // design. Said out loud so it stays the right answer.
            if sd == 0.0 {
                return Ok(vec![false; x.len()]);
            }
            Ok(x.iter().map(|v| v.is_finite() && (v - mean).abs() / sd > threshold).collect())
        }
        "modified_zscore" | "modified_z" | "mzscore" => {
            let sorted = sorted_copy(&finite);
            let med = median_of(&sorted);
            let dev = sorted_copy(&finite.iter().map(|v| (v - med).abs()).collect::<Vec<_>>());
            let mad = median_of(&dev);
            if mad == 0.0 {
                // More than half the samples are identical, so MAD is 0 and
                // the criterion is undefined rather than "everything else is
                // an outlier". Fall back to the mean absolute deviation,
                // which is what Iglewicz & Hoaglin themselves prescribe for
                // this case, rather than flagging the whole vector.
                let mean_ad = finite.iter().map(|v| (v - med).abs()).sum::<f64>()
                    / finite.len() as f64;
                if mean_ad == 0.0 {
                    return Ok(vec![false; x.len()]);
                }
                return Ok(x
                    .iter()
                    .map(|v| v.is_finite() && (v - med).abs() / (1.253314 * mean_ad) > threshold)
                    .collect());
            }
            Ok(x.iter()
                .map(|v| v.is_finite() && 0.6745 * (v - med).abs() / mad > threshold)
                .collect())
        }
        "iqr" | "tukey" => {
            let sorted = sorted_copy(&finite);
            let q1 = quantile_of(&sorted, 0.25);
            let q3 = quantile_of(&sorted, 0.75);
            let iqr = q3 - q1;
            if iqr == 0.0 {
                return Ok(vec![false; x.len()]);
            }
            let lo = q1 - threshold * iqr;
            let hi = q3 + threshold * iqr;
            Ok(x.iter().map(|v| v.is_finite() && (*v < lo || *v > hi)).collect())
        }
        other => Err(NoiseError::BadParameter(format!(
            "`{other}` is not an outlier method -- zscore, modified_zscore, iqr, hampel"
        ))),
    }
}

// ── missing samples ────────────────────────────────────────────────────

/// Fill every non-finite sample of `x` by linear interpolation between the
/// nearest finite samples on either side.
///
/// A gap with finite data on only one side (i.e. one that runs off the
/// start or the end of the record) is filled by HOLDING that one neighbour,
/// not by extrapolating the last interior slope. `interp1` extrapolates,
/// deliberately and documented; this one does not, because the two cases
/// are not the same question. `interp1`'s caller asked for a value at a
/// named point outside the data and gets the model's honest opinion; here
/// nobody asked for anything -- a trailing dropout is being patched so the
/// downstream FFT has something to chew on, and inventing a ramp that keeps
/// climbing off the end of a record is how a dropout turns into a trend.
///
/// Returns `(filled, n_filled)`. Returns every sample NaN untouched if there
/// is no finite sample at all to interpolate from.
pub fn interpolate_missing(x: &[f64]) -> (Vec<f64>, usize) {
    let mut out = x.to_vec();
    let finite: Vec<usize> = (0..x.len()).filter(|&i| x[i].is_finite()).collect();
    if finite.is_empty() {
        return (out, 0);
    }
    let mut filled = 0usize;
    for i in 0..x.len() {
        if x[i].is_finite() {
            continue;
        }
        // Nearest finite index on each side.
        let before = finite.partition_point(|&j| j < i);
        let lo = if before == 0 { None } else { Some(finite[before - 1]) };
        let hi = finite.get(before).copied();
        out[i] = match (lo, hi) {
            (Some(a), Some(b)) => {
                let t = (i - a) as f64 / (b - a) as f64;
                x[a] + t * (x[b] - x[a])
            }
            (Some(a), None) => x[a],
            (None, Some(b)) => x[b],
            (None, None) => unreachable!("finite is non-empty"),
        };
        filled += 1;
    }
    (out, filled)
}

/// Fill every non-finite sample of `x` by the named strategy.
///
/// `method`:
/// - `"linear"` — `interpolate_missing` above (the default).
/// - `"previous"`/`"ffill"` — hold the last finite sample forward. A leading
///   gap, which has no previous sample, falls back to the first finite one.
/// - `"next"`/`"bfill"` — take the next finite sample backward, symmetric.
/// - `"nearest"` — whichever finite neighbour is closer (ties go to the
///   earlier one, so the result does not depend on parity).
/// - `"mean"`/`"median"` — the statistic of the finite samples. Flat, and
///   honest about being flat: it will not fabricate a trend across a gap,
///   which is what makes it the safe choice when the gap is long relative
///   to the signal's own timescale and linear interpolation would be
///   drawing a line through nothing.
///
/// Returns `(filled, n_filled)`.
pub fn fill_missing(x: &[f64], method: &str) -> Result<(Vec<f64>, usize), NoiseError> {
    if x.is_empty() {
        return Err(NoiseError::Empty);
    }
    let finite: Vec<usize> = (0..x.len()).filter(|&i| x[i].is_finite()).collect();
    match method {
        "linear" | "interp" => return Ok(interpolate_missing(x)),
        "previous" | "ffill" | "hold" | "next" | "bfill" | "nearest" | "mean" | "median" => {}
        other => {
            return Err(NoiseError::BadParameter(format!(
                "`{other}` is not a fill method -- linear, previous, next, nearest, mean, median"
            )))
        }
    }
    let mut out = x.to_vec();
    if finite.is_empty() {
        return Ok((out, 0));
    }
    let constant = match method {
        "mean" => Some(finite.iter().map(|&i| x[i]).sum::<f64>() / finite.len() as f64),
        "median" => {
            let sorted = sorted_copy(&finite.iter().map(|&i| x[i]).collect::<Vec<_>>());
            Some(median_of(&sorted))
        }
        _ => None,
    };
    let mut filled = 0usize;
    for i in 0..x.len() {
        if x[i].is_finite() {
            continue;
        }
        if let Some(c) = constant {
            out[i] = c;
            filled += 1;
            continue;
        }
        let before = finite.partition_point(|&j| j < i);
        let lo = if before == 0 { None } else { Some(finite[before - 1]) };
        let hi = finite.get(before).copied();
        out[i] = match method {
            // A leading gap has no previous sample; falling back to the next
            // one is the only alternative to leaving a NaN behind, and a
            // "fill" that leaves NaNs is a trap for whatever runs next.
            "previous" | "ffill" | "hold" => x[lo.or(hi).expect("finite is non-empty")],
            "next" | "bfill" => x[hi.or(lo).expect("finite is non-empty")],
            "nearest" => match (lo, hi) {
                (Some(a), Some(b)) => {
                    if i - a <= b - i {
                        x[a]
                    } else {
                        x[b]
                    }
                }
                (Some(a), None) => x[a],
                (None, Some(b)) => x[b],
                (None, None) => unreachable!("finite is non-empty"),
            },
            _ => unreachable!("checked above"),
        };
        filled += 1;
    }
    Ok((out, filled))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (std::f64::consts::TAU * 5.0 * i as f64 / n as f64).sin())
            .collect()
    }

    /// The headline claim: ask for 3 dB and get 3 dB. This is the whole
    /// reason `add_noise` is parameterised in decibels.
    #[test]
    fn asking_for_a_given_snr_produces_that_snr() {
        let x = tone(4096);
        for target in [20.0, 10.0, 3.0, 0.0, -6.0] {
            let y = add_noise(&x, "gaussian", Level::SnrDb(target), 7).unwrap();
            let got = snr_db(&x, &y).unwrap();
            assert!(
                (got - target).abs() < 0.5,
                "asked for {target} dB, measured {got} dB"
            );
        }
    }

    /// EVERY kind must honour the requested SNR, not just the additive
    /// ones -- otherwise `snr = 3` means three different things depending
    /// on which noise you asked for, and the kinds cannot be compared. The
    /// four that are not linear in their parameter get there by bisection.
    #[test]
    fn every_kind_honours_the_requested_snr_including_the_nonlinear_ones() {
        let x = tone(4096);
        for kind in [
            "gaussian", "uniform", "pink", "brown", "blue",
            "shot", "quantize", "salt_pepper", "impulse",
        ] {
            let y = add_noise(&x, kind, Level::SnrDb(6.0), 11).unwrap();
            let got = snr_db(&x, &y).unwrap();
            assert!((got - 6.0).abs() < 1.0, "{kind}: asked 6 dB, measured {got}");
        }
    }

    /// Salt-and-pepper REPLACES samples, so the measured SNR is not the
    /// parameter -- but the corruption rate is, and that is what the
    /// parameter means for this kind.
    #[test]
    fn salt_and_pepper_replaces_about_the_fraction_asked_for() {
        let x = tone(10_000);
        let y = add_noise(&x, "salt_pepper", Level::Amplitude(0.05), 3).unwrap();
        let changed = x.iter().zip(&y).filter(|(a, b)| a != b).count();
        let rate = changed as f64 / x.len() as f64;
        assert!((rate - 0.05).abs() < 0.01, "corrupted {rate}, asked 0.05");
    }

    /// The pairing the whole module exists for: a median filter removes
    /// salt-and-pepper that a moving average of the same width cannot.
    #[test]
    fn a_median_beats_an_average_on_salt_and_pepper_decisively() {
        let x = tone(2000);
        let y = add_noise(&x, "salt_pepper", Level::Amplitude(0.08), 5).unwrap();
        let med = medfilt(&y, 5).unwrap();
        let avg = moving_average(&y, 5).unwrap();
        let err = |v: &[f64]| -> f64 {
            (v.iter().zip(&x).map(|(a, b)| (a - b).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
        };
        let (em, ea) = (err(&med), err(&avg));
        assert!(em < ea / 2.0, "median {em} should be far better than average {ea}");
    }

    /// And the reason: a median leaves an edge alone where an average
    /// smears it. This is the property, not a side effect.
    #[test]
    fn a_median_preserves_a_step_that_an_average_smears() {
        let step: Vec<f64> = (0..200).map(|i| if i < 100 { 0.0 } else { 1.0 }).collect();
        let med = medfilt(&step, 9).unwrap();
        let avg = moving_average(&step, 9).unwrap();
        // The median reproduces the step exactly.
        assert!((med[99] - 0.0).abs() < 1e-12 && (med[100] - 1.0).abs() < 1e-12);
        // The average does not.
        assert!(avg[99] > 0.1 && avg[100] < 0.9, "avg {} {}", avg[99], avg[100]);
    }

    /// Savitzky-Golay preserves a peak's height where a moving average of
    /// the same width flattens it. That is the reason to have both.
    #[test]
    fn savgol_keeps_a_peak_that_a_moving_average_flattens() {
        let peak: Vec<f64> = (0..200)
            .map(|i| {
                let t = (i as f64 - 100.0) / 8.0;
                (-t * t).exp()
            })
            .collect();
        let sg = savgol(&peak, 21, 3).unwrap();
        let ma = moving_average(&peak, 21).unwrap();
        // Measured: Savitzky-Golay keeps 0.93 of the peak, the moving
        // average 0.63. The gap is the point; the thresholds sit either
        // side of it with room, rather than pinning the exact numbers.
        assert!(sg[100] > 0.90, "savgol kept {}", sg[100]);
        assert!(ma[100] < 0.70, "moving average flattened to {}", ma[100]);
        assert!(sg[100] > ma[100] * 1.4, "sg {} vs ma {}", sg[100], ma[100]);
    }

    /// A polynomial of the fitted order passes through Savitzky-Golay
    /// untouched -- the defining property, and the sharpest test of the
    /// coefficients.
    #[test]
    fn savgol_reproduces_a_polynomial_of_its_own_order() {
        let x: Vec<f64> = (0..100).map(|i| {
            let t = i as f64 / 10.0;
            2.0 + 3.0 * t - 0.5 * t * t
        }).collect();
        let y = savgol(&x, 11, 2).unwrap();
        // Away from the edges, where the clamp changes the fit.
        for i in 10..90 {
            assert!((y[i] - x[i]).abs() < 1e-6, "at {i}: {} vs {}", y[i], x[i]);
        }
    }

    #[test]
    fn hampel_fixes_the_spikes_and_leaves_everything_else_exactly_alone() {
        let mut x = tone(500);
        let clean = x.clone();
        x[100] += 8.0;
        x[300] -= 9.0;
        let (fixed, n) = hampel(&x, 7, 3.0).unwrap();
        assert!(n >= 2 && n <= 8, "replaced {n}, expected about 2");
        assert!((fixed[100] - clean[100]).abs() < 0.3);
        // A sample nowhere near a spike is bit-identical.
        assert_eq!(fixed[250], clean[250]);
    }

    /// The contract is that adding a trend and detrending is the same as
    /// detrending alone -- not that the underlying signal comes back
    /// untouched. It does not: a 5-cycle sine sampled over 200 points is
    /// not exactly orthogonal to a ramp, it genuinely carries a -0.19
    /// slope, and detrend correctly removes that too. Asserting otherwise
    /// would be asserting that a least-squares fit should ignore data.
    #[test]
    fn detrending_removes_exactly_the_trend_that_was_added() {
        let n = 200;
        let base = tone(n);
        let with_drift: Vec<f64> = base
            .iter()
            .enumerate()
            .map(|(i, v)| v + 3.0 + 0.02 * i as f64)
            .collect();
        let a = detrend(&base, 1).unwrap();
        let b = detrend(&with_drift, 1).unwrap();
        for (i, (p, q)) in a.iter().zip(&b).enumerate() {
            assert!((p - q).abs() < 1e-9, "at {i}: {p} vs {q}");
        }
        // And a pure polynomial of the fitted order goes to zero.
        let ramp: Vec<f64> = (0..n).map(|i| 5.0 - 0.3 * i as f64).collect();
        for v in detrend(&ramp, 1).unwrap() {
            assert!(v.abs() < 1e-9, "a pure ramp should vanish, got {v}");
        }
        // Order 0 removes the mean and nothing else.
        let centred = detrend(&with_drift, 0).unwrap();
        let mean = centred.iter().sum::<f64>() / n as f64;
        assert!(mean.abs() < 1e-9, "mean {mean}");
    }

    /// Hum carries harmonics, and that is the point: a notch at the
    /// fundamental alone leaves them behind.
    #[test]
    fn hum_puts_energy_at_the_harmonics_not_only_the_fundamental() {
        let fs = 1000.0;
        let n = 2000;
        let quiet = vec![0.0; n];
        let h = hum(&quiet, fs, 50.0, 3, Level::Amplitude(1.0), 1).unwrap();
        // Correlate against each harmonic; all three must be present.
        let energy_at = |f: f64| -> f64 {
            let c: f64 = h.iter().enumerate()
                .map(|(i, v)| v * (std::f64::consts::TAU * f * i as f64 / fs).cos()).sum();
            let s: f64 = h.iter().enumerate()
                .map(|(i, v)| v * (std::f64::consts::TAU * f * i as f64 / fs).sin()).sum();
            (c * c + s * s).sqrt() / n as f64
        };
        assert!(energy_at(50.0) > 0.2, "fundamental {}", energy_at(50.0));
        assert!(energy_at(150.0) > 0.05, "third {}", energy_at(150.0));
        assert!(energy_at(250.0) > 0.02, "fifth {}", energy_at(250.0));
        // And nothing at the EVEN harmonic, which a symmetric
        // nonlinearity does not produce.
        assert!(energy_at(100.0) < 0.02, "100 Hz should be empty: {}", energy_at(100.0));
        // And nothing at a frequency that is not a harmonic.
        assert!(energy_at(80.0) < 0.02, "80 Hz should be empty: {}", energy_at(80.0));
    }

    #[test]
    fn distortion_is_a_function_of_the_signal_so_averaging_cannot_help() {
        let x = tone(1000);
        let d = distort(&x, "clip", 0.5).unwrap();
        // Two runs are identical: no randomness anywhere.
        let d2 = distort(&x, "clip", 0.5).unwrap();
        assert_eq!(d, d2);
        // Clipping reduces the peak; that is what it is.
        let peak = |v: &[f64]| v.iter().fold(0.0f64, |m, a| m.max(a.abs()));
        assert!(peak(&d) < peak(&x) * 0.9);
    }

    #[test]
    fn crossover_hurts_small_signals_where_clipping_hurts_large_ones() {
        let small: Vec<f64> = tone(1000).iter().map(|v| v * 0.02).collect();
        let clipped = distort(&small, "clip", 0.3).unwrap();
        let cross = distort(&small, "crossover", 0.3).unwrap();
        let err = |v: &[f64]| -> f64 {
            v.iter().zip(&small).map(|(a, b)| (a - b).abs()).sum::<f64>()
        };
        // Both are scaled to the signal's own peak, so the comparison is
        // about SHAPE: crossover zeroes the middle, clipping trims the ends.
        // The dead zone is 25% of `amount` of the peak, so on a sine it
        // zeroes 2/pi * asin(0.075) of the samples -- about 4.8%, which is
        // ~48 of 1000. Worked out rather than guessed.
        let zeroed = cross.iter().filter(|v| **v == 0.0).count();
        assert!((30..80).contains(&zeroed), "crossover zeroed {zeroed}, expected about 48");
        // Clipping leaves the middle alone and trims the ends: the exact
        // opposite, which is why they are measured separately. Counted
        // against the INPUT's own zeros -- a sine starts at exactly 0.0,
        // and clipping preserving that is not clipping creating it.
        let zeros_in = small.iter().filter(|v| **v == 0.0).count();
        assert_eq!(clipped.iter().filter(|v| **v == 0.0).count(), zeros_in);
        assert!(err(&clipped) > 0.0);
    }

    /// The ideal-converter result, `6.02*bits + 1.76` dB, is the number a
    /// measurement is compared against -- so the implementation had better
    /// reach it on a full-scale sine.
    #[test]
    fn an_ideal_converter_reaches_its_theoretical_snr() {
        let x = tone(8192);
        for bits in [8u32, 10, 12, 16] {
            let c = adc(&x, bits, -1.0, 1.0, false, 1).unwrap();
            assert_eq!(c.clipped, 0, "a full-scale sine should not clip");
            let measured = snr_db(&x, &c.x).unwrap();
            assert!(
                (measured - c.snr_ideal_db).abs() < 2.0,
                "{bits} bits: ideal {:.1} dB, measured {measured:.1} dB",
                c.snr_ideal_db
            );
        }
    }

    /// The LSB is the range over 2^bits. Getting this wrong by one -- over
    /// `2^bits - 1` -- puts every reading half a step out, which is the
    /// classic converter bug.
    #[test]
    fn the_lsb_is_the_range_over_the_number_of_codes() {
        let c = adc(&[0.0], 12, 0.0, 4.096, false, 1).unwrap();
        assert!((c.lsb - 0.001).abs() < 1e-12, "a 0-4.096 V 12-bit LSB is 1 mV, got {}", c.lsb);
        let c = adc(&[0.0], 8, -5.0, 5.0, false, 1).unwrap();
        assert!((c.lsb - 10.0 / 256.0).abs() < 1e-12);
    }

    /// Clipping is what a step-based model cannot express, and it is the
    /// thing that silently ruins a measurement.
    #[test]
    fn a_signal_beyond_the_reference_clips_and_says_how_often() {
        let x = tone(1000);
        // A range of half the signal's amplitude: everything past +-0.5
        // has nowhere to go.
        let c = adc(&x, 12, -0.5, 0.5, false, 1).unwrap();
        assert!(c.clipped > 200, "clipped only {} of 1000", c.clipped);
        assert!(c.x.iter().all(|v| *v >= -0.5001 && *v <= 0.5001));
        // And inside the range, nothing clips.
        let quiet: Vec<f64> = x.iter().map(|v| v * 0.4).collect();
        assert_eq!(adc(&quiet, 12, -1.0, 1.0, false, 1).unwrap().clipped, 0);
    }

    /// Dither trades a slightly higher noise floor for an error that is
    /// noise rather than harmonics. The floor rising is the measurable
    /// half of that bargain.
    #[test]
    fn dither_costs_a_little_snr_and_buys_an_uncorrelated_error() {
        let x = tone(8192);
        let plain = adc(&x, 8, -1.0, 1.0, false, 3).unwrap();
        let dithered = adc(&x, 8, -1.0, 1.0, true, 3).unwrap();
        let a = snr_db(&x, &plain.x).unwrap();
        let b = snr_db(&x, &dithered.x).unwrap();
        assert!(b < a, "dither should cost SNR: plain {a:.1}, dithered {b:.1}");
        assert!(a - b < 6.0, "but not more than a few dB: {:.1}", a - b);
    }

    #[test]
    fn the_errors_name_the_measurement_problem() {
        assert_eq!(medfilt(&[1.0, 2.0], 4).unwrap_err(),
                   NoiseError::BadWindow { window: 4, len: 2, why: "the window must be odd, so there is a middle sample" });
        assert!(matches!(add_noise(&[1.0], "rainbow", Level::SnrDb(3.0), 1),
                         Err(NoiseError::UnknownKind(_))));
        assert!(matches!(savgol(&[1.0, 2.0, 3.0], 3, 5), Err(NoiseError::BadParameter(_))));
        assert_eq!(medfilt(&[], 3).unwrap_err(), NoiseError::Empty);
        assert!(matches!(adc(&[1.0], 40, 0.0, 1.0, false, 1), Err(NoiseError::BadParameter(_))));
        assert!(matches!(adc(&[1.0], 12, 1.0, 1.0, false, 1), Err(NoiseError::BadParameter(_))));
    }

    /// A seed makes a noisy signal reproducible, which is the difference
    /// between a figure you can regenerate and one you cannot.
    #[test]
    fn the_same_seed_gives_the_same_noise_and_a_different_one_does_not() {
        let x = tone(500);
        let a = add_noise(&x, "gaussian", Level::SnrDb(10.0), 42).unwrap();
        let b = add_noise(&x, "gaussian", Level::SnrDb(10.0), 42).unwrap();
        let c = add_noise(&x, "gaussian", Level::SnrDb(10.0), 43).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn a_two_dimensional_median_cleans_an_image_of_salt_and_pepper() {
        let (w, h) = (40usize, 30usize);
        // A smooth ramp, so any spike is unambiguous.
        let clean: Vec<f64> = (0..w * h).map(|i| (i % w) as f64 / w as f64).collect();
        let noisy = add_noise(&clean, "salt_pepper", Level::Amplitude(0.10), 9).unwrap();
        let fixed = medfilt2(&noisy, w, h, 3).unwrap();
        let err = |v: &[f64]| -> f64 {
            v.iter().zip(&clean).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt()
        };
        assert!(err(&fixed) < err(&noisy) / 3.0,
                "median2 {} should be far better than {}", err(&fixed), err(&noisy));
    }

    // ── rolling statistics ─────────────────────────────────────────────

    /// The headline guarantee, stated as a test rather than a comment: at
    /// EVERY index -- interior and shrunken-edge alike -- the answer is the
    /// statistic of the real sub-slice the window covers, with nothing
    /// invented. If a future edit swaps the shrink for padding, this fails
    /// at index 0 immediately rather than producing plausible numbers.
    #[test]
    fn every_rolling_value_is_the_statistic_of_the_window_it_actually_covers() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let w = 5usize;
        let half = w / 2;
        let rm = rolling_mean(&x, w).unwrap();
        let rr = rolling_rms(&x, w).unwrap();
        let rs = rolling_std(&x, w).unwrap();
        let rmin = rolling_min(&x, w).unwrap();
        let rmax = rolling_max(&x, w).unwrap();
        for i in 0..x.len() {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(x.len());
            let win = &x[lo..hi];
            let n = win.len() as f64;
            let mean = win.iter().sum::<f64>() / n;
            assert!((rm[i] - mean).abs() < 1e-12, "mean at {i}: {} vs {mean}", rm[i]);
            let rms = (win.iter().map(|v| v * v).sum::<f64>() / n).sqrt();
            assert!((rr[i] - rms).abs() < 1e-12, "rms at {i}: {} vs {rms}", rr[i]);
            let sd = if win.len() < 2 {
                0.0
            } else {
                (win.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
            };
            assert!((rs[i] - sd).abs() < 1e-12, "std at {i}: {} vs {sd}", rs[i]);
            let lo_v = win.iter().copied().fold(f64::INFINITY, f64::min);
            let hi_v = win.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            assert_eq!(rmin[i], lo_v, "min at {i}");
            assert_eq!(rmax[i], hi_v, "max at {i}");
        }
    }

    /// The specific bias the shrink convention exists to avoid. On a signal
    /// with constant spread everywhere, a clamp- or reflect-padded
    /// rolling_std would report the edges as markedly quieter than the
    /// middle, because a repeated (or mirrored) sample carries no variance.
    /// Shrinking keeps the edge estimate in the same ballpark as the
    /// interior -- noisier, since it is built on fewer samples, but not
    /// biased toward zero.
    #[test]
    fn rolling_std_does_not_collapse_at_the_edges() {
        // Alternating +-1: every window of odd width has real spread.
        let x: Vec<f64> = (0..41).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let rs = rolling_std(&x, 5).unwrap();
        let interior = rs[20];
        assert!(interior > 0.5, "sanity: interior spread {interior}");
        // With a clamp-pad, rs[0] would be built from [x0,x0,x0,x1,x2] and
        // come out well under the interior value. Shrinking gives the std of
        // [x0,x1,x2], which is the same order of magnitude.
        assert!(
            rs[0] > 0.5 * interior,
            "edge std {} collapsed against interior {interior} -- padding crept back in",
            rs[0]
        );
        assert!(rs[x.len() - 1] > 0.5 * interior, "trailing edge {}", rs[x.len() - 1]);
    }

    #[test]
    fn a_rolling_window_must_be_odd_like_every_other_window_here() {
        assert!(rolling_mean(&[1.0, 2.0, 3.0], 4).is_err());
        assert!(rolling_mean(&[1.0, 2.0, 3.0], 0).is_err());
        assert!(rolling_mean(&[], 3).is_err());
        // A window longer than the data is fine -- it just shrinks to the
        // whole vector at every index, which is the whole-vector statistic.
        let all = rolling_mean(&[1.0, 2.0, 3.0], 101).unwrap();
        assert!(all.iter().all(|v| (v - 2.0).abs() < 1e-12), "{all:?}");
    }

    // ── outliers ───────────────────────────────────────────────────────

    /// The masking effect, made concrete: this is the reason the default is
    /// not simply "zscore, always". One gross outlier inflates the very
    /// standard deviation it is measured against, so the plain z-score
    /// misses it while the two robust criteria do not.
    /// The masking effect, made concrete: this is the reason the default is
    /// not simply "zscore, always".
    ///
    /// THREE outliers, not one, because one is not enough to demonstrate it
    /// -- a lone gross outlier drives the z-score to its ceiling of
    /// `(N-1)/sqrt(N)`, which for N=21 is 4.36 and clears the threshold
    /// easily. It takes a few of them to inflate `std` enough that each
    /// hides behind the others: here each scores 2.39, so the plain
    /// criterion reports a clean vector. Both robust criteria see all three.
    #[test]
    fn several_gross_outliers_mask_each_other_from_the_plain_zscore() {
        let mut x: Vec<f64> = (0..18).map(|i| (i % 3) as f64).collect();
        x.extend([10_000.0, 10_000.0, 10_000.0]);
        let z = outlier_flags(&x, "zscore", 3.0).unwrap();
        assert_eq!(
            z.iter().filter(|f| **f).count(),
            0,
            "the plain z-score is expected to be masked here -- that is the point"
        );
        for m in ["modified_zscore", "iqr"] {
            let f = outlier_flags(&x, m, default_outlier_threshold(m)).unwrap();
            assert!(f[18] && f[19] && f[20], "{m} must catch all three");
            assert_eq!(f.iter().filter(|v| **v).count(), 3, "{m}: and only those three");
        }
    }

    /// The z-score's hard ceiling, pinned so the doc comment above cannot
    /// drift from the arithmetic. Over N samples no z-score can exceed
    /// `(N-1)/sqrt(N)`, so at the default threshold of 3 a short record
    /// answers "no outliers" no matter how gross the outlier is. This is a
    /// property of the criterion, not a bug -- but it is exactly the kind of
    /// silent "nothing found" that should be written down somewhere.
    #[test]
    fn the_plain_zscore_cannot_flag_anything_in_a_short_record() {
        for n in 3..=10usize {
            // A baseline with real spread, so the robust criteria are on
            // their ordinary path rather than the degenerate MAD-is-zero
            // one, and one enormous sample at the end.
            let mut x: Vec<f64> = (0..n).map(|i| (i % 3) as f64).collect();
            x[n - 1] = 1e9; // as gross as it gets
            let f = outlier_flags(&x, "zscore", 3.0).unwrap();
            assert_eq!(
                f.iter().filter(|v| **v).count(),
                0,
                "n={n}: a z-score over {n} samples caps at {:.3}, below 3",
                (n as f64 - 1.0) / (n as f64).sqrt()
            );
            // The robust criteria have no such ceiling and do catch it.
            let mz = outlier_flags(&x, "modified_zscore", 3.5).unwrap();
            assert!(mz[n - 1], "n={n}: modified z-score must still catch 1e9");
        }
    }

    /// A second, narrower ceiling, found the same way as the first (by a
    /// test failing that had no business failing) and recorded because it is
    /// genuinely surprising: the modified z-score's DEGENERATE path has a
    /// finite-sample ceiling of its own.
    ///
    /// When over half the window is identical, MAD is 0 and the criterion
    /// falls back to `1.253314 * mean absolute deviation`. On a vector of
    /// N-1 identical samples plus one outlier, that mean deviation is itself
    /// proportional to the outlier, and the score collapses to `N/1.2533`
    /// no matter how extreme the outlier is -- so it clears 3.5 only from
    /// N=5 up. On a baseline with any spread at all MAD is non-zero, the
    /// fallback never runs, and none of this applies; that is the case the
    /// test above covers.
    #[test]
    fn the_modified_zscore_fallback_has_a_ceiling_of_its_own_on_a_flat_baseline() {
        for n in 3..=8usize {
            let mut x = vec![0.0; n];
            x[n - 1] = 1e9;
            let caught = outlier_flags(&x, "modified_zscore", 3.5).unwrap()[n - 1];
            let score = n as f64 / 1.253314;
            assert_eq!(
                caught,
                score > 3.5,
                "n={n}: flat-baseline fallback scores {score:.3} against 3.5"
            );
        }
    }

    #[test]
    fn a_clean_vector_has_no_outliers_under_any_criterion() {
        let x: Vec<f64> = (0..50).map(|i| (i as f64 * 0.1).sin()).collect();
        for m in ["zscore", "modified_zscore", "iqr"] {
            let f = outlier_flags(&x, m, default_outlier_threshold(m)).unwrap();
            assert_eq!(f.iter().filter(|v| **v).count(), 0, "{m} flagged a clean sine");
        }
        // A constant vector has no spread and so no outliers -- not
        // "everything is an outlier", which a 0/0 would otherwise produce.
        let flat = vec![7.0; 10];
        for m in ["zscore", "modified_zscore", "iqr"] {
            let f = outlier_flags(&flat, m, default_outlier_threshold(m)).unwrap();
            assert_eq!(f.iter().filter(|v| **v).count(), 0, "{m} flagged a constant vector");
        }
    }

    /// A NaN is a missing sample, not an outlier. Conflating them would make
    /// `remove_outliers` quietly double as a NaN filter and hide dropouts.
    #[test]
    fn a_nan_is_never_reported_as_an_outlier() {
        let x = [1.0, 2.0, f64::NAN, 3.0, 2.0, 1.0, 500.0];
        for m in ["zscore", "modified_zscore", "iqr"] {
            let f = outlier_flags(&x, m, default_outlier_threshold(m)).unwrap();
            assert!(!f[2], "{m} flagged the NaN");
        }
    }

    #[test]
    fn an_unknown_outlier_method_names_the_ones_that_exist() {
        let err = outlier_flags(&[1.0, 2.0], "three_sigma", 3.0).unwrap_err().to_string();
        assert!(err.contains("zscore"), "{err}");
        assert!(err.contains("iqr"), "{err}");
    }

    /// `hampel` is now a caller of `hampel_flags`, so the two can never
    /// disagree about which samples are outliers. Checked rather than
    /// assumed, because the split is exactly the kind of refactor that
    /// silently changes one side.
    #[test]
    fn hampel_and_hampel_flags_agree_sample_for_sample() {
        let mut x: Vec<f64> = (0..60).map(|i| (i as f64 * 0.2).sin()).collect();
        x[17] = 9.0;
        x[42] = -9.0;
        let (cleaned, n) = hampel(&x, 7, 3.0).unwrap();
        let (flags, meds) = hampel_flags(&x, 7, 3.0).unwrap();
        assert_eq!(n, flags.iter().filter(|f| **f).count());
        assert!(flags[17] && flags[42], "both spikes must be flagged");
        for i in 0..x.len() {
            let want = if flags[i] { meds[i] } else { x[i] };
            assert_eq!(cleaned[i], want, "disagreement at {i}");
        }
    }

    /// The case the local Hampel test exists for and the global ones cannot
    /// do: a small spike riding on a baseline that itself travels much
    /// further than the spike does. Globally the spike is unremarkable.
    #[test]
    fn hampel_catches_a_spike_on_a_drifting_baseline_that_global_tests_miss() {
        let mut x: Vec<f64> = (0..200).map(|i| i as f64).collect();
        x[100] += 30.0; // tiny next to the 0..200 range the baseline covers
        let (flags, _) = hampel_flags(&x, 7, 3.0).unwrap();
        assert!(flags[100], "the local test must see the step");
        let z = outlier_flags(&x, "zscore", 3.0).unwrap();
        assert!(!z[100], "the global test is expected to miss it -- that is the point");
    }

    // ── missing samples ────────────────────────────────────────────────

    #[test]
    fn linear_interpolation_fills_an_interior_gap_exactly() {
        let x = [0.0, f64::NAN, f64::NAN, 30.0];
        let (out, n) = interpolate_missing(&x);
        assert_eq!(n, 2);
        assert!((out[1] - 10.0).abs() < 1e-12, "{out:?}");
        assert!((out[2] - 20.0).abs() < 1e-12, "{out:?}");
        assert!(out.iter().all(|v| v.is_finite()));
    }

    /// A gap at the end has finite data on one side only. Holding the last
    /// good sample is deliberate: extrapolating the interior slope turns a
    /// dropout into a trend, which is worse than a flat patch because it
    /// looks like data.
    #[test]
    fn an_edge_gap_holds_the_nearest_sample_instead_of_extrapolating() {
        let x = [f64::NAN, 5.0, 6.0, 7.0, f64::NAN, f64::NAN];
        let (out, n) = interpolate_missing(&x);
        assert_eq!(n, 3);
        assert_eq!(out[0], 5.0, "leading gap holds the first good sample");
        assert_eq!(out[4], 7.0, "trailing gap holds the last good sample");
        assert_eq!(out[5], 7.0, "and does not keep climbing at 1/sample");
    }

    #[test]
    fn an_all_nan_vector_is_left_alone_rather_than_invented() {
        let x = [f64::NAN; 4];
        let (out, n) = interpolate_missing(&x);
        assert_eq!(n, 0);
        assert!(out.iter().all(|v| v.is_nan()), "{out:?}");
    }

    #[test]
    fn each_fill_method_does_what_its_name_says() {
        let x = [1.0, f64::NAN, 9.0];
        assert_eq!(fill_missing(&x, "previous").unwrap().0[1], 1.0);
        assert_eq!(fill_missing(&x, "next").unwrap().0[1], 9.0);
        assert_eq!(fill_missing(&x, "linear").unwrap().0[1], 5.0);
        assert_eq!(fill_missing(&x, "mean").unwrap().0[1], 5.0);
        // A leading gap has no previous sample; "previous" falls back rather
        // than leaving a NaN behind for the next stage to trip over.
        let lead = [f64::NAN, 2.0, 3.0];
        assert_eq!(fill_missing(&lead, "previous").unwrap().0[0], 2.0);
        // "nearest" breaks a tie toward the earlier sample, so the answer
        // does not depend on the gap's parity.
        let tie = [0.0, f64::NAN, 100.0];
        assert_eq!(fill_missing(&tie, "nearest").unwrap().0[1], 0.0);
        let err = fill_missing(&x, "spline").unwrap_err().to_string();
        assert!(err.contains("linear"), "{err}");
    }
}
