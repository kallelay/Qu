//! What a `Signal` knows about itself beyond its samples and its rate
//! (`docs/design/toolkit-signal.md` §10, and contract point 1: "Rate, time
//! origin, unit and calibration travel with the samples").
//!
//! `Value::Signal` carried exactly two things until now -- the buffer and
//! `Fs` -- so every other property of a measurement lived in the
//! programmer's head, which is the bug class §0 of that spec exists to
//! eliminate. This module is the third field: a time origin, a sample unit,
//! a calibration, the named metadata field set, and markers/regions.
//!
//! THE WHOLE STRUCT IS `Default`-EMPTY AND `Arc`-SHARED. That matters for
//! two reasons. A signal that nobody annotated costs one refcounted pointer
//! to a shared empty value, so the ~100 existing `Value::Signal`
//! construction sites pay nothing; and `t0` defaulting to `0.0` means every
//! origin-aware accessor added here (`.t`, `timestamps`, time slicing)
//! returns exactly what it returned before for every signal that never set
//! an origin. There is no "old behaviour" to preserve separately.
//!
//! WHY EAGER, NOT LAZY. `calibrate`/`apply_gain`/`convert_unit` rescale the
//! SAMPLES and record what they did; they do not stash a factor to be
//! applied later by whoever remembers to. So after `calibrate(s,
//! sensitivity = 12.3)` the samples really are Pascals: `rms(s)` is a
//! pressure, `max(s)` is a pressure, and a plot of `s` is labelled in the
//! unit it is actually drawn in. A lazy calibration would make every one of
//! those silently wrong unless it went through a calibration-aware wrapper,
//! and the wrappers are exactly what nobody writes. The `Calibration`
//! record is kept as PROVENANCE -- how the raw sample became this one --
//! and is composed forward by later gains/offsets so it stays true rather
//! than becoming a stale label.

use crate::{e, EvalError, Value, R};
use std::sync::Arc;

/// How a raw sample value became a physically meaningful one.
///
/// `physical = slope * raw + offset`, in `unit`. `reference` is the
/// zero-dB level for that unit when one is conventionally defined (20 µPa
/// for sound pressure), which is what lets `spl` answer at all and what its
/// absence is what makes `spl` refuse.
#[derive(Clone, Debug)]
pub struct Calibration {
    pub slope: f64,
    pub offset: f64,
    pub unit: String,
    pub reference: Option<f64>,
    /// How the calibration was established, for `metadata()` to report.
    /// `"sensitivity"`, `"tone"`, `"linear"` or `"curve"`.
    pub source: &'static str,
}

#[derive(Clone, Debug)]
pub struct Marker {
    pub time: f64,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub start: f64,
    pub end: f64,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct SigMeta {
    /// Time of sample 0, in seconds. `0.0` for a signal nobody dated, which
    /// is why every origin-aware accessor is a no-op by default.
    pub t0: f64,
    /// The unit the SAMPLES are in right now -- not the unit they were
    /// recorded in. `convert_unit` changes both together, on purpose.
    pub unit: Option<String>,
    pub cal: Option<Calibration>,
    /// The named field set, canonicalised. Values are restricted to
    /// `Num`/`Str`/`Bool` by `set_metadata`.
    pub fields: Vec<(String, Value)>,
    pub markers: Vec<Marker>,
    pub regions: Vec<Region>,
}

impl SigMeta {
    /// The shared empty metadata every plain `Value::Signal(xs, fs)` gets.
    ///
    /// One process-wide allocation rather than one per signal: a `OnceLock`
    /// rather than a `thread_local!` because `Value` crosses threads here
    /// (`Worker`/`Pool`) and a per-thread empty would make two "identical"
    /// signals hold different pointers for no reason.
    pub fn none() -> Arc<SigMeta> {
        static EMPTY: std::sync::OnceLock<Arc<SigMeta>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(|| Arc::new(SigMeta::default())).clone()
    }

    /// True when nothing has been set -- used to keep `save`'s JSON (and
    /// `print`'s rendering) identical to what it was for un-annotated
    /// signals, so this change adds no noise to existing output.
    pub fn is_empty(&self) -> bool {
        self.t0 == 0.0
            && self.unit.is_none()
            && self.cal.is_none()
            && self.fields.is_empty()
            && self.markers.is_empty()
            && self.regions.is_empty()
    }

    /// The annotations that survive an operation which keeps every sample
    /// where it was in time but changes what the numbers MEAN -- a
    /// threshold, a quantiser, a standardiser, an arbitrary elementwise
    /// function.
    ///
    /// This is the one rule the whole file follows, stated once:
    ///
    ///   * the TIME AXIS (origin, markers, regions, free-form fields) is a
    ///     statement about *when* sample `i` happened, and a per-sample
    ///     function does not move it, so it survives;
    ///   * the UNIT and the CALIBRATION are statements about what sample
    ///     `i` *is*, and an arbitrary function falsifies both, so they do
    ///     not. `sqrt` of a signal in pascals is not in pascals.
    ///
    /// The linear operations that genuinely do preserve a unit -- a gain,
    /// an offset, a unit conversion -- go through [`rescale`] instead,
    /// which composes the calibration forward rather than dropping it.
    /// That split is why `gain` does not reach `map1`.
    pub fn axis_only(&self) -> SigMeta {
        SigMeta {
            t0: self.t0,
            unit: None,
            cal: None,
            fields: self.fields.clone(),
            markers: self.markers.clone(),
            regions: self.regions.clone(),
        }
    }

    /// The mirror image of [`SigMeta::axis_only`], for an operation that
    /// keeps what the numbers MEAN but moves them along the axis -- `delay`
    /// is the one that matters.
    ///
    /// A delayed volt signal is still in volts and still calibrated, so the
    /// unit and calibration ride along. Its MARKERS do not: a marker saying
    /// "the impact is at 1.2 s" named a feature that has just moved, and a
    /// marker that keeps its old time after the feature under it slid is a
    /// silently wrong answer rather than a missing one. They are dropped.
    ///
    /// Shifting them by the delay instead would be the better answer and is
    /// the obvious follow-up; it is not done here because `delay`
    /// zero-fills one end and truncates the other, so a marker near either
    /// edge has no well-defined new home, and picking one quietly is how
    /// this would become wrong again.
    pub fn values_only(&self) -> SigMeta {
        SigMeta {
            t0: self.t0,
            unit: self.unit.clone(),
            cal: self.cal.clone(),
            fields: self.fields.clone(),
            markers: Vec::new(),
            regions: Vec::new(),
        }
    }

    pub fn field(&self, key: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set_field(&mut self, key: &str, v: Value) {
        match self.fields.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = v,
            None => self.fields.push((key.to_string(), v)),
        }
    }
}

// ---------------------------------------------------------------- metadata

/// The named metadata field set, adopted verbatim from `toolkit-signal.md`
/// §10 (itself from specs.md §40) rather than reinvented -- the spec's own
/// words are "a real schema worth adopting as-is".
///
/// Spelled snake_case because every other identifier in this language is;
/// `canonical_field` below accepts the spec's own camelCase
/// (`samplingRate`, `startTime`) and maps it here, so a reader following
/// the spec literally is not punished for it.
///
/// SORTED, because `metadata()` reports in this order and an arbitrary
/// order would make two dumps of the same signal diff against each other.
pub const METADATA_FIELDS: &[&str] = &[
    "channel",
    "comment",
    "experiment",
    "gain",
    "instrument",
    "offset",
    "operator",
    "sample",
    "sampling_rate",
    "sensor",
    "start_time",
    "temperature",
    "unit",
];

/// The three fields of that set that are NOT free-form: they are properties
/// the `Signal` itself already carries, so `metadata()` reads them through
/// from the value and `set_metadata` refuses to write them.
///
/// This is the whole reason the schema is worth having. A `unit` field that
/// says "Pa" while the samples are volts, or a `sampling_rate` field that
/// says 48000 next to an `Fs` of 44100, is not metadata -- it is a second,
/// competing answer to a question the value already answers, and the one
/// that gets believed is whichever the reader happened to look at. So there
/// is exactly one answer and `set_metadata` points at the real setter.
pub const DERIVED_FIELDS: &[&str] = &["sampling_rate", "start_time", "unit"];

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Map any accepted spelling of a metadata key onto its canonical one.
/// `None` for a key outside the schema -- callers refuse rather than
/// storing it, because a typo'd key that silently becomes a new field is a
/// field nobody will ever read back.
pub fn canonical_field(key: &str) -> Option<&'static str> {
    let k = squash(key.trim());
    METADATA_FIELDS.iter().copied().find(|f| squash(f) == k)
}

pub fn is_derived_field(key: &str) -> bool {
    DERIVED_FIELDS.contains(&key)
}

/// Where a derived field's real setter lives, for `set_metadata`'s refusal.
pub fn derived_field_owner(key: &str) -> &'static str {
    match key {
        "sampling_rate" => "the signal's own Fs -- rebuild it with `signal(x, fs)` or `resample_to`",
        "start_time" => "`set_start_time(s, t)`",
        "unit" => "`signal_unit(s, \"V\")` or `convert_unit(s, \"mV\")`",
        _ => "the signal itself",
    }
}

// ------------------------------------------------------------------- units

/// The canonical sample-unit set from `toolkit-signal.md` §10 -- "(V, mV, A,
/// mA, Ω, °C, Pa, g, m/s², N, strain)" -- plus the obvious prefixed
/// siblings of each, as `(spelling, family, scale to the family's base)`.
///
/// SEPARATE FROM `apply_unit`/`unit_family` in `lib.rs`, deliberately.
/// Those answer "what does the literal `1 kHz` evaluate to", are reachable
/// from the lexer's fixed `UNITS` table, and cover none of Pa/N/strain/g.
/// These are STRING arguments (`convert_unit(s, "m/s²")`), so they are not
/// bound by what the lexer will tokenise -- which is the only reason the
/// spec's own `Ω`, `°C` and `m/s²` spellings can be supported exactly as
/// written rather than transliterated.
///
/// `g` IS THE ACCELERATION g (9.80665 m/s²), NOT THE GRAM. That is what the
/// spec's list means -- it sits between `Pa` and `m/s²`, and the gram has no
/// business in an accelerometer channel. Prefixed `mg` is deliberately
/// ABSENT rather than defined as milli-g: it reads as "milligram" to at
/// least as many people, and a unit that half the readers get backwards is
/// worse than one that errors.
const UNIT_TABLE: &[(&str, &str, f64)] = &[
    // voltage, base V
    ("V", "V", 1.0),
    ("mV", "V", 1e-3),
    ("uV", "V", 1e-6),
    ("\u{b5}V", "V", 1e-6),
    ("kV", "V", 1e3),
    // current, base A
    ("A", "A", 1.0),
    ("mA", "A", 1e-3),
    ("uA", "A", 1e-6),
    ("\u{b5}A", "A", 1e-6),
    // resistance, base Ohm
    ("Ohm", "Ohm", 1.0),
    ("ohm", "Ohm", 1.0),
    ("\u{3a9}", "Ohm", 1.0),
    ("mOhm", "Ohm", 1e-3),
    ("kOhm", "Ohm", 1e3),
    ("MOhm", "Ohm", 1e6),
    // pressure, base Pa
    ("Pa", "Pa", 1.0),
    ("hPa", "Pa", 1e2),
    ("kPa", "Pa", 1e3),
    ("mbar", "Pa", 1e2),
    ("bar", "Pa", 1e5),
    // acceleration, base m/s^2
    ("m/s^2", "m/s^2", 1.0),
    ("m/s\u{b2}", "m/s^2", 1.0),
    ("g", "m/s^2", 9.806_65),
    // force, base N
    ("N", "N", 1.0),
    ("mN", "N", 1e-3),
    ("kN", "N", 1e3),
    // strain, base strain (dimensionless ratio, but its own family so it
    // cannot be silently swapped with a bare number)
    ("strain", "strain", 1.0),
    ("ustrain", "strain", 1e-6),
    ("\u{b5}strain", "strain", 1e-6),
    ("microstrain", "strain", 1e-6),
];

/// Temperature is affine, not a scale factor, so it cannot live in
/// `UNIT_TABLE` -- 20 °C is not 20/273.15 K. Handled as its own family
/// throughout.
/// Bare `C` and `F` are deliberately absent. They are Coulomb and Farad in
/// this engine's own literal-unit table (`apply_unit`), and a spelling that
/// means charge in one half of the language and temperature in the other is
/// a trap regardless of which reading is "more useful here". `degC`/`°C`
/// are unambiguous and cost one keystroke.
const TEMP_UNITS: &[(&str, TempScale)] = &[
    ("degC", TempScale::C),
    ("\u{b0}C", TempScale::C),
    ("degF", TempScale::F),
    ("\u{b0}F", TempScale::F),
    ("K", TempScale::K),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempScale {
    C,
    F,
    K,
}

impl TempScale {
    fn to_kelvin(self, v: f64) -> f64 {
        match self {
            TempScale::C => v + 273.15,
            TempScale::F => (v - 32.0) * 5.0 / 9.0 + 273.15,
            TempScale::K => v,
        }
    }
    fn from_kelvin(self, k: f64) -> f64 {
        match self {
            TempScale::C => k - 273.15,
            TempScale::F => (k - 273.15) * 9.0 / 5.0 + 32.0,
            TempScale::K => k,
        }
    }
}

fn temp_scale(u: &str) -> Option<TempScale> {
    TEMP_UNITS.iter().find(|(n, _)| *n == u).map(|(_, s)| *s)
}

fn linear_unit(u: &str) -> Option<(&'static str, f64)> {
    UNIT_TABLE
        .iter()
        .find(|(n, _, _)| *n == u)
        .map(|(_, fam, sc)| (*fam, *sc))
}

/// The family a unit spelling belongs to, for error messages and for the
/// same-family check conversion depends on.
pub fn unit_family_of(u: &str) -> Option<&'static str> {
    if temp_scale(u).is_some() {
        return Some("temperature");
    }
    linear_unit(u).map(|(f, _)| f)
}

pub fn known_unit(u: &str) -> bool {
    unit_family_of(u).is_some()
}

/// Every accepted spelling, for the "unknown unit" message. A list of what
/// WOULD have worked is the difference between a dead end and a typo fixed
/// on the next line.
pub fn known_units_list() -> String {
    let mut names: Vec<&str> = UNIT_TABLE.iter().map(|(n, _, _)| *n).collect();
    names.extend(TEMP_UNITS.iter().map(|(n, _)| *n));
    names.join(", ")
}

/// The affine map taking a value in `from` to the same physical quantity in
/// `to`, as `(scale, offset)` so a whole buffer can be rescaled with one
/// multiply-add per sample and a `Calibration` can compose it.
///
/// Refuses across families by NAMING BOTH, which is the §1 "mismatched
/// rates error, they never silently reinterpret" rule applied to the
/// vertical axis: `convert_unit(s, "Pa")` on a volt signal is not a
/// conversion anybody can do without a sensitivity, and answering it with a
/// number would be inventing one.
pub fn conversion(f: &str, from: &str, to: &str) -> R<(f64, f64)> {
    if from == to {
        return Ok((1.0, 0.0));
    }
    match (temp_scale(from), temp_scale(to)) {
        (Some(a), Some(b)) => {
            // Compose the two affine maps into one so the caller still gets
            // a single (scale, offset): K = a_s*v + a_o, then out = b from K.
            let k0 = a.to_kelvin(0.0);
            let k1 = a.to_kelvin(1.0);
            let o0 = b.from_kelvin(k0);
            let o1 = b.from_kelvin(k1);
            Ok((o1 - o0, o0))
        }
        (Some(_), None) | (None, Some(_)) => e(format!(
            "{f}: cannot convert between {from} and {to} -- one is a temperature and the other \
             is not"
        )),
        (None, None) => {
            let (fa, fs) = linear_unit(from).ok_or_else(|| EvalError {
                msg: format!(
                    "{f}: unknown unit \"{from}\" -- known units are {}",
                    known_units_list()
                ),
            })?;
            let (ta, ts) = linear_unit(to).ok_or_else(|| EvalError {
                msg: format!(
                    "{f}: unknown unit \"{to}\" -- known units are {}",
                    known_units_list()
                ),
            })?;
            if fa != ta {
                return e(format!(
                    "{f}: cannot convert {from} ({fa}) to {to} ({ta}) -- these are different \
                     physical quantities. A conversion between them needs a calibration, not a \
                     unit change: see calibrate(s, sensitivity = ...)."
                ));
            }
            Ok((fs / ts, 0.0))
        }
    }
}

/// Split a sensitivity spelling like `"mV/Pa"` into its electrical and
/// physical halves, and return the factor that takes a raw sample in the
/// signal's own unit to one physical unit.
///
/// A sensitivity is quoted as "how much electrical output per physical
/// input", so the calibration slope is its RECIPROCAL -- which is the step
/// that gets inverted by hand and gets it backwards. Done once, here.
pub fn parse_sensitivity_unit(f: &str, spec: &str) -> R<(String, String)> {
    let mut parts = spec.splitn(2, '/');
    let elec = parts.next().unwrap_or("").trim().to_string();
    let phys = match parts.next() {
        Some(p) => p.trim().to_string(),
        None => {
            return e(format!(
                "{f}: sensitivity_unit \"{spec}\" is not of the form \"<electrical>/<physical>\" \
                 -- e.g. \"mV/Pa\" for a microphone, \"mV/g\" for an accelerometer"
            ))
        }
    };
    if elec.is_empty() || phys.is_empty() {
        return e(format!(
            "{f}: sensitivity_unit \"{spec}\" is missing one of its two halves -- it must read \
             \"<electrical>/<physical>\", e.g. \"mV/Pa\""
        ));
    }
    if !known_unit(&elec) {
        return e(format!(
            "{f}: sensitivity_unit \"{spec}\" -- unknown electrical unit \"{elec}\". Known units \
             are {}",
            known_units_list()
        ));
    }
    if !known_unit(&phys) {
        return e(format!(
            "{f}: sensitivity_unit \"{spec}\" -- unknown physical unit \"{phys}\". Known units \
             are {}",
            known_units_list()
        ));
    }
    Ok((elec, phys))
}

/// The conventional 0 dB reference for a physical unit, when its field has
/// one everybody agrees on. `None` means `spl`-style logarithmic levels are
/// not defined for that unit and asking for one should refuse rather than
/// pick a reference the caller never named.
pub fn db_reference_for(unit: &str) -> Option<f64> {
    match unit_family_of(unit)? {
        // 20 µPa, the threshold of hearing -- the reference every dB SPL
        // number in acoustics is against.
        "Pa" => Some(20e-6),
        _ => None,
    }
}

/// RMS about the mean -- what a sound level meter measures.
///
/// AC-coupled on purpose. A DC offset in a capture is not sound (nor
/// vibration, nor strain); including it would inflate the level and, in
/// `calibrate`'s tone path, silently mis-scale every measurement made
/// afterwards by the same factor.
pub fn ac_rms(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    (xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n).sqrt()
}

/// The base unit of the family a spelling belongs to (`mV` -> `V`).
///
/// Used by `calibrate(sensitivity =)` to decide what an UNTAGGED signal is
/// assumed to be in: given `mV/Pa`, a signal with no unit of its own is
/// taken to be in volts, not millivolts. That is the near-universal case (a
/// capture scaled to +/-1 full scale is in volts), and the assumption is
/// stated here rather than left implicit in the arithmetic.
pub fn family_base_unit(u: &str) -> &'static str {
    unit_family_of(u).unwrap_or("V")
}

/// Map samples through a piecewise-linear calibration curve.
///
/// `raw` must be strictly increasing -- that is what makes the lookup
/// single-valued, and a curve that doubles back describes a sensor whose
/// reading does not determine its input, which no interpolation can rescue.
///
/// `mode` decides what happens to a sample outside `raw`'s span:
///
///   * `"error"` (the default at the call site): refuse, naming the sample
///     and the domain. The calibration says nothing about that reading, so
///     neither does this.
///   * `"clamp"`: hold the nearest end value.
///   * `"linear"`: continue the end segment's slope outwards.
///
/// The curve's own points are NOT retained on the resulting signal. The
/// samples are physical afterwards and the `Calibration` record says a
/// curve is how they got there, but the exact mapping cannot be recovered
/// from the returned value alone -- keep the curve alongside if that
/// matters. Recording it would mean a variable-size calibration record, and
/// nothing in §10 reads it back.
pub fn apply_curve(xs: &[f64], raw: &[f64], phys: &[f64], mode: &str) -> R<Vec<f64>> {
    let f = "apply_calibration_curve";
    if raw.len() != phys.len() {
        return e(format!(
            "{f}: the curve's two columns have different lengths ({} raw points, {} physical \
             points) -- they are pairs",
            raw.len(),
            phys.len()
        ));
    }
    if raw.len() < 2 {
        return e(format!(
            "{f}: a calibration curve needs at least 2 points, got {}",
            raw.len()
        ));
    }
    for w in raw.windows(2) {
        if w[1] <= w[0] {
            return e(format!(
                "{f}: the raw points must be strictly increasing, but {} is followed by {}. A \
                 curve that repeats or reverses a raw reading does not define a single \
                 physical value for it.",
                w[0], w[1]
            ));
        }
    }
    if raw.iter().chain(phys.iter()).any(|v| !v.is_finite()) {
        return e(format!("{f}: the curve contains a non-finite point"));
    }
    if !matches!(mode, "error" | "clamp" | "linear") {
        return e(format!(
            "{f}: unknown extrapolate {mode:?} -- expected \"error\", \"clamp\" or \"linear\""
        ));
    }
    let (lo, hi) = (raw[0], raw[raw.len() - 1]);
    let seg = |i: usize, x: f64| -> f64 {
        let t = (x - raw[i]) / (raw[i + 1] - raw[i]);
        phys[i] + t * (phys[i + 1] - phys[i])
    };
    let mut out = Vec::with_capacity(xs.len());
    for &x in xs {
        // A NaN sample stays NaN rather than being extrapolated from or
        // refused: it was already missing before the calibration ran, and
        // `fill_missing`/`interpolate_nan` are what address that.
        if !x.is_finite() {
            out.push(x);
            continue;
        }
        if x < lo || x > hi {
            match mode {
                "error" => {
                    return e(format!(
                        "{f}: sample {x} is outside the curve's measured range [{lo}, {hi}], so \
                         the calibration says nothing about it. Extend the curve, or pass \
                         extrapolate=\"clamp\" to hold the end value or extrapolate=\"linear\" \
                         to continue the end slope -- both are guesses, which is why neither \
                         is the default."
                    ))
                }
                "clamp" => out.push(if x < lo { phys[0] } else { phys[phys.len() - 1] }),
                _ => out.push(seg(if x < lo { 0 } else { raw.len() - 2 }, x)),
            }
            continue;
        }
        // Binary search for the segment containing `x`.
        let mut a = 0usize;
        let mut b = raw.len() - 1;
        while b - a > 1 {
            let mid = (a + b) / 2;
            if raw[mid] <= x {
                a = mid;
            } else {
                b = mid;
            }
        }
        out.push(seg(a, x));
    }
    Ok(out)
}

// -------------------------------------------------------------- value glue

/// Borrow a `Value` as a signal, or say why it is not one.
///
/// Every builtin in this module REQUIRES a real `Signal` rather than
/// accepting any numeric vector through `to_cow`. That is the opposite of
/// the convention most of this file's neighbours follow, and it is
/// deliberate: a bare vector has no rate, so a marker at "1.5 s", a
/// `timestamps()` axis, or a `spl` reading would all have to invent one.
/// Inventing 1 Hz and carrying on is exactly the `spectrogram` defect
/// `resolve_fs` was written to kill.
pub fn as_signal<'a>(f: &str, v: &'a Value) -> R<(&'a Arc<Vec<f64>>, f64, &'a Arc<SigMeta>)> {
    match v {
        Value::Signal(xs, fs, m) => Ok((xs, *fs, m)),
        other => e(format!(
            "{f}: expected a signal that carries its own sample rate, found {}. \
             Wrap the samples first -- `signal(x, fs)`.",
            other.type_name()
        )),
    }
}

/// A signal's duration in seconds, from its own rate.
pub fn duration_of(n: usize, fs: f64) -> f64 {
    if fs.is_finite() && fs > 0.0 {
        n as f64 / fs
    } else {
        f64::NAN
    }
}

/// Apply `out = scale * raw + offset` to every sample, and COMPOSE the same
/// map into the calibration record.
///
/// The composition is the whole point. After `calibrate(...)` the record
/// says "physical = slope*raw + offset"; a later `apply_gain(s, 2)` changes
/// the samples, and a record left untouched would then describe a mapping
/// from the raw file to a value the signal no longer holds. Composing gives
/// `slope' = scale*slope`, `offset' = scale*offset + offset_new`, which is
/// still literally true of the samples in hand.
///
/// THE dB REFERENCE MOVES ONLY WHEN THE UNIT DOES, and that distinction is
/// the whole correctness of `spl` under these operations. A reference is a
/// fixed physical magnitude (20 µPa) that happens to be *written* in the
/// signal's current unit:
///
///   * `convert_unit(s, "kPa")` divides every sample by 1000 without making
///     the sound any quieter, so the reference must be re-expressed as
///     `20e-9 kPa` and `spl` is UNCHANGED;
///   * `apply_gain(s, 2)` leaves the unit alone and genuinely doubles the
///     pressure, so the reference must stay at `20e-6 Pa` and `spl` goes UP
///     by 6 dB.
///
/// Scaling it in both cases (the first version of this function) made a
/// gain invisible to `spl` -- the level came back identical no matter how
/// much gain was applied, which a probe caught only because it asserted the
/// expected 6 dB rather than just that the call succeeded.
pub fn rescale(
    f: &str,
    v: &Value,
    scale: f64,
    offset: f64,
    new_unit: Option<String>,
    source: Option<&'static str>,
) -> R<Value> {
    let (xs, fs, m) = as_signal(f, v)?;
    if !scale.is_finite() || !offset.is_finite() {
        return e(format!(
            "{f}: the calibration would be scale={scale}, offset={offset} -- not finite, so every \
             sample would become NaN or infinite"
        ));
    }
    let out: Vec<f64> = xs.iter().map(|x| scale * x + offset).collect();
    let mut meta = (**m).clone();
    let unit_after = new_unit.clone().or_else(|| meta.unit.clone());
    // Did this operation change the unit the samples are written in? Only
    // then is the reference re-expressed; see the doc comment above.
    let unit_changed = match (&new_unit, &meta.unit) {
        (Some(after), Some(before)) => after != before,
        (Some(_), None) => true,
        (None, _) => false,
    };
    meta.cal = match (meta.cal.take(), source) {
        (Some(c), _) => Some(Calibration {
            slope: scale * c.slope,
            offset: scale * c.offset + offset,
            unit: unit_after.clone().unwrap_or(c.unit),
            reference: c
                .reference
                .map(|r| if unit_changed { r * scale } else { r }),
            source: source.unwrap_or(c.source),
        }),
        (None, Some(src)) => Some(Calibration {
            slope: scale,
            offset,
            unit: unit_after.clone().unwrap_or_default(),
            reference: unit_after.as_deref().and_then(db_reference_for),
            source: src,
        }),
        // A plain gain on an uncalibrated signal stays uncalibrated: it
        // changed the numbers, but it did not make them Pascals, and
        // recording a "calibration" here would let `spl` answer from a
        // scaling nobody claimed was physical.
        (None, None) => None,
    };
    if let Some(u) = new_unit {
        meta.unit = Some(u);
    }
    Ok(Value::Signal(Arc::new(out), fs, Arc::new(meta)))
}
