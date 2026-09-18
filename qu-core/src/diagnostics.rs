//! Measurement diagnostics: is this record trustworthy *before* it is
//! analysed at all.
//!
//! Everything here answers a question about the recording rather than
//! about the phenomenon. A clipped record still has a spectrum, a mean and
//! a THD figure — every one of them wrong, and none of them says so. These
//! are the cheap checks that do.
//!
//! # Why clipping detection is run-based, not threshold-based
//!
//! The naive test — "does any sample reach the maximum?" — fires on every
//! signal ever recorded, because *some* sample is always the largest one.
//! Tightened to "within x% of full scale" it still fires on any healthy
//! recording that uses its headroom, which is exactly the well-made
//! recording you least want flagged. A sine sampled anywhere near its peak
//! legitimately touches the top of its range.
//!
//! What a clipped record actually has, and an unclipped one does not, is a
//! **flat run**: several consecutive samples holding the *same* value at an
//! extreme. That is the converter (or the limiter) emitting the same code
//! over and over because the input went somewhere it cannot follow. A real
//! sine's samples near its peak are not equal to each other — consecutive
//! samples differ by roughly `A * 2*pi^2 / N^2` for `N` samples per cycle —
//! so requiring equality separates the two cases without needing to guess
//! where "full scale" was.
//!
//! This detects **hard** clipping, which is what digital clipping and a
//! converter hitting its rail both are. Soft/analogue saturation rounds the
//! shoulder over instead of flattening it, and will not produce exactly
//! equal samples; catching that needs an explicit `threshold` plus a looser
//! `tol`, which is why both are parameters rather than constants.
//!
//! **Known false-positive case, stated rather than hidden:** a heavily
//! oversampled record that has *already* been quantised can hold the same
//! code for several consecutive samples near its peak without being
//! clipped, once the per-sample change near the peak falls below one LSB.
//! Raising `min_run` past the expected dwell, or passing the converter's
//! real `threshold`, is the answer; there is no way to tell the two apart
//! from the samples alone.

/// One maximal run of consecutive samples held at the same extreme value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRun {
    /// Index of the first sample in the run.
    pub start: usize,
    /// How many consecutive samples the run covers (always `>= min_run`).
    pub len: usize,
    /// The value they are all held at, signed — so the caller can tell the
    /// positive rail from the negative one.
    pub value: f64,
}

impl ClipRun {
    /// Index of the last sample in the run, inclusive.
    pub fn end(&self) -> usize {
        self.start + self.len - 1
    }
}

/// What [`find_clipping`] found, including the parameters it actually used.
///
/// `threshold` is echoed back deliberately: when it was inferred from the
/// record rather than supplied, a caller reading only `runs` has no way to
/// know what level the answer was relative to.
#[derive(Clone, Debug)]
pub struct ClipReport {
    /// The runs, in order of position.
    pub runs: Vec<ClipRun>,
    /// Total number of samples inside those runs.
    pub samples: usize,
    /// The magnitude a sample had to reach to be a candidate.
    pub threshold: f64,
    /// How nearly equal consecutive samples had to be.
    pub tol: f64,
    /// How long a run had to be to count.
    pub min_run: usize,
}

/// Find the flat runs at or beyond `threshold`.
///
/// A run is a maximal stretch of consecutive samples that are all at least
/// `threshold` in magnitude and all within `tol` of the run's first sample.
/// Runs shorter than `min_run` are discarded.
///
/// `min_run = 1` degrades this to the plain "any sample at or beyond the
/// threshold" test, which is occasionally what a caller with a *known*
/// converter rail actually wants — see [`saturation`], which is told the
/// rail instead of inferring it and therefore does not need a run at all.
pub fn find_clipping(x: &[f64], threshold: f64, tol: f64, min_run: usize) -> ClipReport {
    let min_run = min_run.max(1);
    // Compare with a relative slack so that a threshold taken from the
    // record's own peak matches that peak exactly, and a threshold that
    // came out of arithmetic (a converter rail computed from an LSB) is not
    // missed by one ulp.
    let gate = threshold * (1.0 - 1e-12);
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < x.len() {
        if !(x[i].abs() >= gate) || !x[i].is_finite() {
            i += 1;
            continue;
        }
        let anchor = x[i];
        let mut j = i + 1;
        while j < x.len() && x[j].abs() >= gate && (x[j] - anchor).abs() <= tol {
            j += 1;
        }
        let len = j - i;
        if len >= min_run {
            runs.push(ClipRun { start: i, len, value: anchor });
        }
        // Continue from the end of this run, never re-entering it: runs are
        // maximal, so restarting inside one would report the same clipping
        // twice at different lengths.
        i = j;
    }
    let samples = runs.iter().map(|r| r.len).sum();
    ClipReport { runs, samples, threshold, tol, min_run }
}

/// How saturated a record is against a converter whose range is *known*.
#[derive(Clone, Copy, Debug)]
pub struct SaturationReport {
    /// Samples sitting on (or past) the top code.
    pub high: usize,
    /// Samples sitting on (or past) the bottom code.
    pub low: usize,
    /// Fraction of the record on either rail, 0 to 1.
    pub fraction: f64,
    /// One least-significant bit, in the signal's own units.
    pub lsb: f64,
    /// The bottom of the reference range — also the bottom code.
    pub vmin: f64,
    /// The top of the reference range. The top *code* is one LSB below it.
    pub vmax: f64,
    /// The value of the top code, `vmax - lsb`.
    pub top_code: f64,
}

impl SaturationReport {
    /// Did the converter hit a rail at all.
    pub fn saturated(&self) -> bool {
        self.high > 0 || self.low > 0
    }
}

/// Has `x` hit the rails of a `bits`-bit converter spanning `[vmin, vmax]`.
///
/// This is the counterpart to [`find_clipping`] for the case where the
/// converter is known rather than guessed, and it deliberately does **not**
/// use the flat-run heuristic: when the rail is a number you were given, a
/// single sample sitting on it is already evidence, and demanding a run
/// would only lose the short excursions that matter most.
///
/// The code geometry is taken from the same place [`crate::noise::adc`]
/// takes it — `lsb = (vmax - vmin) / 2^bits`, top code at `vmax - lsb` —
/// via [`crate::noise::code_step`], so the detector and the simulator
/// cannot drift apart into two slightly different converters.
pub fn saturation(x: &[f64], bits: u32, vmin: f64, vmax: f64) -> SaturationReport {
    let lsb = crate::noise::code_step(bits, vmin, vmax);
    let top_code = vmax - lsb;
    // A hair of slack, in LSBs rather than in absolute terms, so that a
    // sample landing exactly on the rail counts whatever the range is.
    let slack = lsb * 1e-9;
    let mut high = 0usize;
    let mut low = 0usize;
    for &v in x {
        if !v.is_finite() {
            continue;
        }
        if v >= top_code - slack {
            high += 1;
        } else if v <= vmin + slack {
            low += 1;
        }
    }
    let fraction = if x.is_empty() { 0.0 } else { (high + low) as f64 / x.len() as f64 };
    SaturationReport { high, low, fraction, lsb, vmin, vmax, top_code }
}

/// Structural sanity of a record, before any semantic diagnostic is run.
#[derive(Clone, Debug, Default)]
pub struct IntegrityReport {
    /// Number of samples.
    pub n: usize,
    /// How many samples are NaN.
    pub nan: usize,
    /// How many samples are an infinity.
    pub inf: usize,
    /// True when every sample is the same value — not an error, but the
    /// usual shape of a disconnected input or a stuck channel.
    pub constant: bool,
    /// Everything that is wrong, in the order checked. Empty means OK.
    pub issues: Vec<String>,
}

impl IntegrityReport {
    /// Nothing wrong was found.
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Check a record for the structural faults that make every later number
/// meaningless: nothing to measure, and non-finite contamination.
///
/// Deliberately narrow. This answers "are these samples a usable record",
/// not "is this signal any good" — a constant record is *reported* as
/// constant but is not an error, because a DC measurement is a real thing.
pub fn verify(x: &[f64]) -> IntegrityReport {
    let mut r = IntegrityReport { n: x.len(), ..Default::default() };
    for &v in x {
        if v.is_nan() {
            r.nan += 1;
        } else if v.is_infinite() {
            r.inf += 1;
        }
    }
    r.constant = !x.is_empty() && x.iter().all(|v| *v == x[0]);
    if x.is_empty() {
        r.issues.push("the record is empty -- there are no samples to check".to_string());
    }
    if r.nan > 0 {
        r.issues.push(format!(
            "{} of {} samples are NaN -- a mean, an FFT or a filter over this returns NaN \
             throughout, not just at those positions",
            r.nan, r.n
        ));
    }
    if r.inf > 0 {
        r.issues.push(format!("{} of {} samples are infinite", r.inf, r.n));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean sine must not be flagged. This is the direction that is easy
    /// to get wrong and easy to never test: a detector that always fires
    /// passes every positive-case test there is.
    #[test]
    fn a_clean_sine_has_no_flat_runs() {
        let x: Vec<f64> = (0..1000)
            .map(|i| (2.0 * std::f64::consts::PI * i as f64 / 64.0).sin())
            .collect();
        let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let r = find_clipping(&x, peak, 1e-9 * peak, 3);
        assert!(r.runs.is_empty(), "clean sine flagged: {:?}", r.runs);
        assert_eq!(r.samples, 0);
    }

    /// ...and a clipped one must be.
    #[test]
    fn a_clamped_sine_is_caught_on_both_rails() {
        let x: Vec<f64> = (0..1000)
            .map(|i| (2.0 * std::f64::consts::PI * i as f64 / 64.0).sin().clamp(-0.6, 0.6))
            .collect();
        let r = find_clipping(&x, 0.6, 1e-9 * 0.6, 3);
        assert!(r.runs.len() >= 20, "expected many runs, got {}", r.runs.len());
        assert!(r.runs.iter().any(|c| c.value > 0.0), "no positive-rail run");
        assert!(r.runs.iter().any(|c| c.value < 0.0), "no negative-rail run");
        // Every reported run really is flat and really is at the rail.
        for run in &r.runs {
            assert!(run.len >= 3);
            for k in run.start..=run.end() {
                assert!((x[k] - run.value).abs() <= 1e-9, "run {run:?} is not flat at {k}");
                assert!(x[k].abs() >= 0.6 * (1.0 - 1e-12));
            }
        }
    }

    /// Runs are maximal and non-overlapping, so the sample total cannot
    /// exceed the record.
    #[test]
    fn runs_do_not_overlap() {
        let x = vec![1.0; 10];
        let r = find_clipping(&x, 1.0, 1e-12, 3);
        assert_eq!(r.runs.len(), 1);
        assert_eq!(r.runs[0], ClipRun { start: 0, len: 10, value: 1.0 });
        assert_eq!(r.samples, 10);
    }

    /// A run shorter than `min_run` is not clipping.
    #[test]
    fn a_short_touch_is_not_clipping() {
        let mut x = vec![0.0; 20];
        x[5] = 1.0;
        x[6] = 1.0;
        assert!(find_clipping(&x, 1.0, 1e-12, 3).runs.is_empty());
        assert_eq!(find_clipping(&x, 1.0, 1e-12, 2).runs.len(), 1);
    }

    /// The rails agree with what `adc` would actually emit — the whole
    /// reason `code_step` is shared rather than re-derived here.
    #[test]
    fn saturation_rails_match_what_adc_emits() {
        let bits = 8u32;
        let (vmin, vmax) = (-1.0, 1.0);
        // Drive the converter well past its range on both sides.
        let x: Vec<f64> = (0..200).map(|i| 2.0 * (i as f64 / 100.0 - 1.0)).collect();
        let conv = crate::noise::adc(&x, bits, vmin, vmax, false, 1).unwrap();
        let r = saturation(&conv.x, bits, vmin, vmax);
        let emitted_max = conv.x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let emitted_min = conv.x.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!((emitted_max - r.top_code).abs() < r.lsb * 1e-9, "{emitted_max} vs {}", r.top_code);
        assert!((emitted_min - vmin).abs() < r.lsb * 1e-9);
        assert!(r.saturated());
        assert!(r.high > 0 && r.low > 0);
    }

    /// A signal comfortably inside the range must not read as saturated.
    #[test]
    fn an_in_range_signal_is_not_saturated() {
        let x: Vec<f64> = (0..500)
            .map(|i| 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 50.0).sin())
            .collect();
        let conv = crate::noise::adc(&x, 12, -1.0, 1.0, false, 1).unwrap();
        let r = saturation(&conv.x, 12, -1.0, 1.0);
        assert!(!r.saturated(), "half-scale sine read as saturated: {r:?}");
        assert_eq!(r.fraction, 0.0);
    }

    #[test]
    fn verify_names_what_is_wrong() {
        assert!(verify(&[1.0, 2.0, 3.0]).ok());
        assert!(!verify(&[]).ok());
        let r = verify(&[1.0, f64::NAN, f64::INFINITY]);
        assert_eq!((r.nan, r.inf), (1, 1));
        assert!(!r.ok());
        assert!(verify(&[2.0, 2.0, 2.0]).constant);
        assert!(verify(&[2.0, 2.0, 2.0]).ok(), "a DC record is odd, not invalid");
    }
}
