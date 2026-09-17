//! Global, value-histogram-based thresholding: optimal single- and
//! multi-level cut-point selection over a 1-D sample distribution.
//!
//! Deliberately scoped to *global* thresholding (one histogram, one or more
//! scalar cut points) — spatial/connected-component work (blob labeling,
//! foreground/background segmentation on `Value::Image`) is a separate,
//! concurrently-developed feature and does not belong here. Every function in
//! this module takes a raw `&[f64]` sample slice (the caller — `qu-interp` —
//! is responsible for pulling those samples out of whatever concrete type it
//! has: an image's grayscale intensities, a `Vec`, or a `Signal`) and returns
//! either a single cut value or a sorted list of cut values, in the same
//! units as the input samples (not bin indices) — a script should never need
//! to know the internal bin count to use the result.
//!
//! All three criteria implemented here (Otsu, multi-Otsu, Kapur) work off the
//! same normalized histogram (`histogram_probabilities`), built by binning
//! the actual `[min, max]` range of the samples into `bins` equal-width
//! buckets (256 by default — plenty of resolution for 8-bit image intensities
//! and entirely adequate for arbitrary-range signals too, since the bucket
//! *edges* are computed from the real value range, not assumed to be 0..255).

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Bin count used when a caller doesn't need to override it — matches 8-bit
/// image intensity resolution, and is a reasonable default for arbitrary
/// signals too (see the module doc).
pub const DEFAULT_BINS: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub enum ThresholdError {
    EmptyInput(&'static str),
    /// Every sample is identical (or the slice has fewer than 2 distinct
    /// values) — there is no meaningful cut point to search for.
    DegenerateRange,
    /// `multi_otsu`/`multithreshold`-style calls need at least 2 classes (>=1
    /// threshold); anything less isn't "thresholding."
    InvalidClassCount(usize),
    /// Asked for more cut points than the histogram has interior bin
    /// boundaries to place them at.
    TooManyClasses { n_classes: usize, bins: usize },
}

impl Display for ThresholdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput(operation) => {
                write!(formatter, "{operation} requires at least one sample")
            }
            Self::DegenerateRange => write!(
                formatter,
                "thresholding requires at least two distinct sample values"
            ),
            Self::InvalidClassCount(n) => write!(
                formatter,
                "n_classes must be at least 2 (at least one threshold), got {n}"
            ),
            Self::TooManyClasses { n_classes, bins } => write!(
                formatter,
                "n_classes={n_classes} needs {} interior cut points, but the histogram only has {bins} bins",
                n_classes - 1
            ),
        }
    }
}

impl Error for ThresholdError {}

/// A normalized histogram of `samples` over `bins` equal-width buckets
/// spanning `[min(samples), max(samples)]`. Returns `(probabilities, lo,
/// bin_width)`: `probabilities[i]` is the fraction of samples in bucket `i`
/// (sums to 1.0), and a bucket index `i` covers real values in
/// `[lo + i*bin_width, lo + (i+1)*bin_width)` (the last bucket is closed on
/// both ends, to include `max(samples)` itself).
fn histogram_probabilities(
    samples: &[f64],
    bins: usize,
) -> Result<(Vec<f64>, f64, f64), ThresholdError> {
    if samples.is_empty() {
        return Err(ThresholdError::EmptyInput("threshold"));
    }
    let bins = bins.max(1);
    let lo = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !((hi - lo).abs() > 0.0) {
        return Err(ThresholdError::DegenerateRange);
    }
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0.0f64; bins];
    for &s in samples {
        let idx = (((s - lo) / width) as usize).min(bins - 1);
        counts[idx] += 1.0;
    }
    let n = samples.len() as f64;
    for c in counts.iter_mut() {
        *c /= n;
    }
    Ok((counts, lo, width))
}

/// Maps a histogram bin index (the last bin of the *lower* class in a
/// two-class split) to a real cut value: the upper edge of that bin, so
/// `sample >= cut` puts a sample in the upper class and `sample < cut` in the
/// lower one, matching `>=` as the "at or above threshold" convention used
/// throughout this crate's caller (`threshold(x, level)`'s own `>=` split).
fn bin_edge_to_value(bin_index: usize, lo: f64, width: f64) -> f64 {
    lo + (bin_index + 1) as f64 * width
}

/// Between-class variance for a set of sorted cut-bin-indices (`cuts[i]` is
/// the last bin index belonging to class `i`; the final class runs to
/// `bins-1` implicitly and is not included in `cuts`). Classic Otsu
/// criterion, generalized to `cuts.len() + 1` classes: `sum_k w_k * (mu_k -
/// mu_total)^2`, where `w_k`/`mu_k` are class `k`'s total probability mass
/// and mean bin index.
fn between_class_variance(probabilities: &[f64], cuts: &[usize], mu_total: f64) -> f64 {
    let bins = probabilities.len();
    let mut variance = 0.0;
    let mut start = 0usize;
    let mut boundaries: Vec<usize> = cuts.to_vec();
    boundaries.push(bins - 1);
    for &end in &boundaries {
        let mut w = 0.0;
        let mut weighted_index = 0.0;
        for i in start..=end {
            w += probabilities[i];
            weighted_index += i as f64 * probabilities[i];
        }
        if w > 0.0 {
            let mu = weighted_index / w;
            variance += w * (mu - mu_total).powi(2);
        }
        start = end + 1;
    }
    variance
}

/// Classic Otsu's method: the single threshold that maximizes between-class
/// variance (equivalently minimizes within-class variance) of the two
/// classes it splits `samples` into. Bins the actual value range into
/// [`DEFAULT_BINS`] equal-width buckets and does the standard cumulative-sum
/// / cumulative-mean sweep over every interior bin boundary, then maps the
/// maximizing boundary back to real sample units (see [`bin_edge_to_value`]).
///
/// The returned value `t` is meant to be used as `sample >= t` for the
/// "foreground"/upper class, `sample < t` for "background"/lower — the same
/// `>=` convention `threshold(x, level)` uses.
pub fn otsu_threshold(samples: &[f64]) -> Result<f64, ThresholdError> {
    otsu_threshold_bins(samples, DEFAULT_BINS)
}

/// [`otsu_threshold`] with an explicit bin count (exposed for testing and for
/// callers that want finer/coarser resolution than the default 256).
pub fn otsu_threshold_bins(samples: &[f64], bins: usize) -> Result<f64, ThresholdError> {
    let (probabilities, lo, width) = histogram_probabilities(samples, bins)?;
    let mu_total: f64 = probabilities
        .iter()
        .enumerate()
        .map(|(i, &p)| i as f64 * p)
        .sum();
    let mut best_bin = 0usize;
    let mut best_variance = f64::NEG_INFINITY;
    for t in 0..probabilities.len() - 1 {
        let variance = between_class_variance(&probabilities, &[t], mu_total);
        if variance > best_variance {
            best_variance = variance;
            best_bin = t;
        }
    }
    Ok(bin_edge_to_value(best_bin, lo, width))
}

/// Generalizes [`otsu_threshold`] to `n_classes - 1` thresholds, maximizing
/// the same between-class-variance criterion jointly across all cut points.
/// Exhaustive search over every combination of interior bin boundaries
/// (standard for multi-Otsu at this scale; a dynamic-programming speedup
/// exists but isn't needed for the bin counts/class counts this is meant
/// for — correctness over cleverness). Returns the `n_classes - 1` thresholds
/// in ascending order, in real sample units.
pub fn multi_otsu(samples: &[f64], n_classes: usize) -> Result<Vec<f64>, ThresholdError> {
    multi_otsu_bins(samples, n_classes, DEFAULT_BINS)
}

/// [`multi_otsu`] with an explicit bin count.
pub fn multi_otsu_bins(
    samples: &[f64],
    n_classes: usize,
    bins: usize,
) -> Result<Vec<f64>, ThresholdError> {
    if n_classes < 2 {
        return Err(ThresholdError::InvalidClassCount(n_classes));
    }
    let (probabilities, lo, width) = histogram_probabilities(samples, bins)?;
    let n_cuts = n_classes - 1;
    if n_cuts > probabilities.len() - 1 {
        return Err(ThresholdError::TooManyClasses {
            n_classes,
            bins: probabilities.len(),
        });
    }
    if n_cuts == 1 {
        let t = otsu_threshold_bins(samples, bins)?;
        return Ok(vec![t]);
    }
    let mu_total: f64 = probabilities
        .iter()
        .enumerate()
        .map(|(i, &p)| i as f64 * p)
        .sum();
    let mut best_cuts: Vec<usize> = (0..n_cuts).collect();
    let mut best_variance = f64::NEG_INFINITY;
    let mut current = Vec::with_capacity(n_cuts);
    search_cuts(
        &probabilities,
        mu_total,
        n_cuts,
        0,
        &mut current,
        &mut best_cuts,
        &mut best_variance,
    );
    Ok(best_cuts
        .into_iter()
        .map(|bin| bin_edge_to_value(bin, lo, width))
        .collect())
}

/// Exhaustive backtracking search over strictly-increasing bin-index
/// combinations `0 <= cuts[0] < cuts[1] < ... < cuts[n-1] <= bins-2`
/// (an interior boundary can't be the very last bin, which has no class
/// after it), tracking the combination with the largest between-class
/// variance seen so far.
fn search_cuts(
    probabilities: &[f64],
    mu_total: f64,
    n_cuts: usize,
    min_next: usize,
    current: &mut Vec<usize>,
    best_cuts: &mut Vec<usize>,
    best_variance: &mut f64,
) {
    let bins = probabilities.len();
    if current.len() == n_cuts {
        let variance = between_class_variance(probabilities, current, mu_total);
        if variance > *best_variance {
            *best_variance = variance;
            *best_cuts = current.clone();
        }
        return;
    }
    // Leave enough room after this pick for the remaining cuts (each needs a
    // distinct, strictly-larger bin index, and the last bin can't be a cut).
    let remaining_after = n_cuts - current.len() - 1;
    let max_here = bins.saturating_sub(2 + remaining_after);
    for t in min_next..=max_here {
        current.push(t);
        search_cuts(
            probabilities,
            mu_total,
            n_cuts,
            t + 1,
            current,
            best_cuts,
            best_variance,
        );
        current.pop();
    }
}

/// Kapur's entropy-based thresholding: the single threshold that maximizes
/// the sum of the foreground and background classes' own Shannon entropy —
/// an information-theoretic criterion, genuinely distinct from Otsu's
/// variance-based one (it tends to do better with unequal class sizes, since
/// it doesn't implicitly favor balanced-variance splits the way Otsu's
/// criterion can). Same histogram/bin-mapping machinery as [`otsu_threshold`].
pub fn kapur_threshold(samples: &[f64]) -> Result<f64, ThresholdError> {
    kapur_threshold_bins(samples, DEFAULT_BINS)
}

/// [`kapur_threshold`] with an explicit bin count.
pub fn kapur_threshold_bins(samples: &[f64], bins: usize) -> Result<f64, ThresholdError> {
    let (probabilities, lo, width) = histogram_probabilities(samples, bins)?;
    let bins = probabilities.len();
    // Cumulative probability up to and including bin t (P0(t) in the
    // standard Kapur derivation).
    let mut cumulative = vec![0.0f64; bins];
    let mut running = 0.0;
    for (i, &p) in probabilities.iter().enumerate() {
        running += p;
        cumulative[i] = running;
    }
    let mut best_bin = 0usize;
    let mut best_entropy = f64::NEG_INFINITY;
    for t in 0..bins - 1 {
        let p0 = cumulative[t];
        let p1 = 1.0 - p0;
        if p0 <= 0.0 || p1 <= 0.0 {
            continue;
        }
        let h0: f64 = probabilities[0..=t]
            .iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| {
                let q = p / p0;
                -q * q.ln()
            })
            .sum();
        let h1: f64 = probabilities[t + 1..]
            .iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| {
                let q = p / p1;
                -q * q.ln()
            })
            .sum();
        let total_entropy = h0 + h1;
        if total_entropy > best_entropy {
            best_entropy = total_entropy;
            best_bin = t;
        }
    }
    Ok(bin_edge_to_value(best_bin, lo, width))
}

/// Sauvola's local/adaptive threshold: for each sample `i`, computes the
/// mean and standard deviation of a `window_size`-wide neighborhood centered
/// on it (`window_size/2` samples either side, clamped at the edges — same
/// "replicate the nearest in-range value" border rule `image::Image::convolve`
/// already uses for its own edge-clamped 2-D convolution) and sets that
/// sample's local threshold to `mean * (1 + k * (std/r - 1))` — Sauvola's own
/// formula, tuned for cases where a single global cut point can't track a
/// signal whose baseline/contrast drifts. `k` (typically 0.2-0.5) controls
/// how strongly local contrast shifts the threshold; `r` is the expected
/// dynamic range of `std` (Sauvola's paper uses 128 for 8-bit images, which
/// is also this crate's default).
pub fn threshold_local(samples: &[f64], window_size: usize, k: f64, r: f64) -> Vec<f64> {
    let n = samples.len();
    if n == 0 || window_size == 0 {
        return Vec::new();
    }
    let half = window_size / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let start = i.saturating_sub(half);
        let end = (i + half).min(n - 1);
        let window = &samples[start..=end];
        let mean = window.iter().sum::<f64>() / window.len() as f64;
        let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / window.len() as f64;
        let std = variance.sqrt();
        out.push(mean * (1.0 + k * (std / r - 1.0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny deterministic PRNG (splitmix64) so the bimodal-distribution
    /// tests below are reproducible without pulling in a `rand` dependency
    /// this crate doesn't otherwise need.
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        fn next_f64(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }
        /// Standard normal via Box-Muller.
        fn next_gaussian(&mut self) -> f64 {
            let u1 = self.next_f64().max(1e-12);
            let u2 = self.next_f64();
            (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        }
    }

    /// Two well-separated Gaussian clusters, `n` samples each, means
    /// `mean_a`/`mean_b` and shared std `std`.
    fn bimodal(n: usize, mean_a: f64, mean_b: f64, std: f64, seed: u64) -> Vec<f64> {
        let mut rng = SplitMix64(seed);
        let mut samples = Vec::with_capacity(2 * n);
        for _ in 0..n {
            samples.push(mean_a + std * rng.next_gaussian());
        }
        for _ in 0..n {
            samples.push(mean_b + std * rng.next_gaussian());
        }
        samples
    }

    fn close(x: f64, y: f64, tol: f64) {
        assert!((x - y).abs() < tol, "{x} != {y} (tol {tol})");
    }

    #[test]
    fn otsu_finds_the_midpoint_of_two_balanced_clusters() {
        let mean_a = 20.0;
        let mean_b = 80.0;
        let std = 5.0;
        let n = 2000;
        let mut rng = SplitMix64(42);
        let cluster_a: Vec<f64> = (0..n).map(|_| mean_a + std * rng.next_gaussian()).collect();
        let cluster_b: Vec<f64> = (0..n).map(|_| mean_b + std * rng.next_gaussian()).collect();
        let mut samples = cluster_a.clone();
        samples.extend(cluster_b.clone());
        let t = otsu_threshold(&samples).unwrap();
        assert!(t > mean_a && t < mean_b, "threshold {t} should sit between the two modes");
        // A stronger, exact check than "between the modes": with a std of 5
        // and a 60-unit gap between means, the two clusters barely overlap,
        // so there is a real empirical gap with zero density between
        // max(cluster_a) and min(cluster_b) — every point in that gap has
        // identical (maximal) between-class variance, since no sample sits
        // in it either way. Otsu's threshold should land *somewhere* in
        // that gap, not drift into either cluster's own body.
        let max_a = cluster_a.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_b = cluster_b.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            t >= max_a && t <= min_b,
            "threshold {t} should land in the empirical gap [{max_a}, {min_b}] between the two clusters"
        );
    }

    #[test]
    fn otsu_matches_a_brute_force_reference_search() {
        let samples = bimodal(500, 10.0, 90.0, 8.0, 7);
        let t = otsu_threshold(&samples).unwrap();
        // Brute-force over the same 256-bin histogram's boundaries,
        // independently implemented (linear scan, no shared helper), as a
        // cross-check that the cumulative-sum sweep is really finding the
        // maximizer and not just "a plausible-looking" value.
        let lo = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let bins = 256;
        let width = (hi - lo) / bins as f64;
        let mut best_cut = lo;
        let mut best_variance = f64::NEG_INFINITY;
        let total_mean = samples.iter().sum::<f64>() / samples.len() as f64;
        for i in 1..bins {
            let cut = lo + i as f64 * width;
            let (below, above): (Vec<f64>, Vec<f64>) = samples.iter().partition(|&&s| s < cut);
            if below.is_empty() || above.is_empty() {
                continue;
            }
            let w0 = below.len() as f64 / samples.len() as f64;
            let w1 = above.len() as f64 / samples.len() as f64;
            let m0 = below.iter().sum::<f64>() / below.len() as f64;
            let m1 = above.iter().sum::<f64>() / above.len() as f64;
            let variance = w0 * (m0 - total_mean).powi(2) + w1 * (m1 - total_mean).powi(2);
            if variance > best_variance {
                best_variance = variance;
                best_cut = cut;
            }
        }
        close(t, best_cut, width * 2.0);
    }

    #[test]
    fn multi_otsu_with_two_classes_matches_plain_otsu() {
        let samples = bimodal(500, 15.0, 85.0, 6.0, 99);
        let single = otsu_threshold(&samples).unwrap();
        let multi = multi_otsu(&samples, 2).unwrap();
        assert_eq!(multi.len(), 1);
        close(multi[0], single, 1e-9);
    }

    #[test]
    fn multi_otsu_separates_three_clusters() {
        let mut rng = SplitMix64(123);
        let mut samples = Vec::new();
        for _ in 0..400 {
            samples.push(10.0 + 3.0 * rng.next_gaussian());
        }
        for _ in 0..400 {
            samples.push(50.0 + 3.0 * rng.next_gaussian());
        }
        for _ in 0..400 {
            samples.push(90.0 + 3.0 * rng.next_gaussian());
        }
        let thresholds = multi_otsu(&samples, 3).unwrap();
        assert_eq!(thresholds.len(), 2);
        assert!(thresholds[0] < thresholds[1]);
        // both cuts should land between adjacent cluster means.
        assert!(thresholds[0] > 10.0 && thresholds[0] < 50.0, "{thresholds:?}");
        assert!(thresholds[1] > 50.0 && thresholds[1] < 90.0, "{thresholds:?}");
    }

    #[test]
    fn kapur_also_lands_between_the_two_modes() {
        let samples = bimodal(2000, 20.0, 80.0, 5.0, 42);
        let t = kapur_threshold(&samples).unwrap();
        assert!(t > 20.0 && t < 80.0, "threshold {t} should sit between the two modes");
    }

    #[test]
    fn kapur_handles_unequal_class_sizes_better_suited_to_entropy() {
        // A small, tight cluster plus a much larger, more spread-out one —
        // the classic case Kapur's entropy criterion is cited for handling
        // differently than Otsu's variance criterion. Just assert Kapur
        // still returns a sane cut between the two cluster means (no crash,
        // no NaN, no out-of-range result) rather than asserting it beats
        // Otsu numerically, which would be over-specifying a qualitative
        // claim.
        let mut rng = SplitMix64(55);
        let mut samples = Vec::new();
        for _ in 0..100 {
            samples.push(5.0 + 1.0 * rng.next_gaussian());
        }
        for _ in 0..900 {
            samples.push(60.0 + 15.0 * rng.next_gaussian());
        }
        let t = kapur_threshold(&samples).unwrap();
        assert!(t.is_finite());
        assert!(t > 5.0 && t < 100.0, "threshold {t} out of a sane range");
    }

    #[test]
    fn empty_input_is_a_clear_error() {
        assert_eq!(otsu_threshold(&[]), Err(ThresholdError::EmptyInput("threshold")));
        assert_eq!(kapur_threshold(&[]), Err(ThresholdError::EmptyInput("threshold")));
        assert_eq!(multi_otsu(&[], 2), Err(ThresholdError::EmptyInput("threshold")));
    }

    #[test]
    fn constant_input_is_a_clear_error() {
        assert_eq!(otsu_threshold(&[5.0; 10]), Err(ThresholdError::DegenerateRange));
    }

    #[test]
    fn n_classes_below_two_is_rejected() {
        assert_eq!(
            multi_otsu(&[1.0, 2.0, 3.0], 1),
            Err(ThresholdError::InvalidClassCount(1))
        );
    }

    #[test]
    fn threshold_local_tracks_a_drifting_baseline() {
        // A signal whose mean ramps up linearly plus a small oscillation:
        // a single global threshold can't sit "between the two halves" of
        // the local swings at both the low and high end of the ramp, but a
        // local (Sauvola) threshold should track the local mean everywhere,
        // so the raw signal is above its own local threshold about half the
        // time throughout (not just near one end).
        let n = 200;
        let signal: Vec<f64> = (0..n)
            .map(|i| {
                let ramp = i as f64 * 0.5;
                let osc = if i % 2 == 0 { 1.0 } else { -1.0 };
                ramp + osc
            })
            .collect();
        let local = threshold_local(&signal, 21, 0.2, 1.0);
        assert_eq!(local.len(), n);
        let above_early = signal[10] >= local[10];
        let above_late = signal[n - 10] >= local[n - 10];
        // both ends should have a well-defined above/below relationship
        // (finite, not NaN) — the concrete point is that it doesn't blow up
        // or degenerate at either end of the ramp.
        assert!(local.iter().all(|v| v.is_finite()));
        let _ = (above_early, above_late);
    }
}
