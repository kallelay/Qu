//! Numbers a filter designer shows next to its plots: step-response timing
//! (rise time, settling time, overshoot, the time constant tau), the -3 dB
//! cutoff, and how long the impulse response rings.
//!
//! Everything here is plain `f64` slices in, numbers out, so the arithmetic
//! can be tested without building a filter. `filter_info` in `lib.rs` feeds
//! it the impulse response, the magnitude response and the poles.

/// Timing of a step response `y` sampled every `1/fs` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct StepInfo {
    /// Value the response settles to (mean of the last 10 % of the samples).
    pub final_value: f64,
    /// Time to go from 10 % to 90 % of the final value, seconds.
    pub rise_time: f64,
    /// Time after which the response stays within 2 % of the final value.
    pub settling_time: f64,
    /// Peak above the final value, percent of the final value (0 if none).
    pub overshoot_pct: f64,
    pub peak: f64,
    pub peak_time: f64,
    /// Time to first reach 63.2 % of the final value: the time constant tau.
    pub tau: f64,
    /// Time to first reach 50 % of the final value.
    pub delay_time: f64,
}

/// Time (seconds) at which `y` first reaches `level` (rising towards
/// `final_value`), linearly interpolated between samples. `NaN` when it never
/// does. `y[0]` is time zero.
fn first_crossing(y: &[f64], level: f64, rising: bool, fs: f64) -> f64 {
    for i in 0..y.len() {
        let hit = if rising { y[i] >= level } else { y[i] <= level };
        if hit {
            if i == 0 {
                return 0.0;
            }
            let (a, b) = (y[i - 1], y[i]);
            let frac = if b == a { 0.0 } else { (level - a) / (b - a) };
            return ((i - 1) as f64 + frac) / fs;
        }
    }
    f64::NAN
}

/// `None` when `y` is empty or settles to (almost) zero, where "percent of
/// the final value" means nothing.
pub fn step_info(y: &[f64], fs: f64) -> Option<StepInfo> {
    step_info_impl(y, fs, None)
}

/// Like `step_info`, but measured against a known final value (a filter's DC
/// gain) instead of the mean of the response's tail.
pub fn step_info_to(y: &[f64], fs: f64, final_value: f64) -> Option<StepInfo> {
    step_info_impl(y, fs, Some(final_value))
}

fn step_info_impl(y: &[f64], fs: f64, known_final: Option<f64>) -> Option<StepInfo> {
    if y.is_empty() || !(fs > 0.0) {
        return None;
    }
    let tail = (y.len() / 10).max(1);
    let final_value = known_final.unwrap_or_else(|| y[y.len() - tail..].iter().sum::<f64>() / tail as f64);
    let scale = y.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    if !final_value.is_finite() || final_value.abs() <= 1e-9 * scale.max(1e-300) {
        return None;
    }
    let rising = final_value > 0.0;
    let at = |frac: f64| first_crossing(y, frac * final_value, rising, fs);
    let (mut peak, mut peak_i) = (y[0], 0usize);
    for (i, &v) in y.iter().enumerate() {
        if (rising && v > peak) || (!rising && v < peak) {
            peak = v;
            peak_i = i;
        }
    }
    let overshoot_pct = (((peak - final_value) / final_value) * 100.0).max(0.0);
    let band = 0.02 * final_value.abs();
    let settling_time = match y.iter().rposition(|v| (v - final_value).abs() > band) {
        None => 0.0,
        Some(i) if i + 1 < y.len() => (i + 1) as f64 / fs,
        Some(_) => f64::NAN,
    };
    Some(StepInfo {
        final_value,
        rise_time: at(0.9) - at(0.1),
        settling_time,
        overshoot_pct,
        peak,
        peak_time: peak_i as f64 / fs,
        tau: at(1.0 - (-1.0_f64).exp()),
        delay_time: at(0.5),
    })
}

/// Samples until the impulse response has delivered all but `tail_fraction`
/// of its total energy (e.g. 0.001 for 99.9 %): how long the filter "rings".
pub fn impulse_length(h: &[f64], tail_fraction: f64) -> usize {
    let total: f64 = h.iter().map(|v| v * v).sum();
    if total <= 0.0 {
        return 0;
    }
    let mut acc = 0.0;
    for (i, v) in h.iter().enumerate() {
        acc += v * v;
        if acc >= total * (1.0 - tail_fraction) {
            return i + 1;
        }
    }
    h.len()
}

/// First frequency (Hz) where the magnitude falls 3 dB below its peak, for a
/// response sampled at `freqs`; for a highpass the first rise to within 3 dB
/// of the peak. `None` when it never crosses (an allpass, or a response that
/// stays within 3 dB of its peak). Linearly interpolated.
pub fn cutoff_3db(freqs: &[f64], mag: &[f64]) -> Option<f64> {
    if freqs.len() != mag.len() || mag.len() < 2 {
        return None;
    }
    let peak = mag.iter().cloned().fold(0.0_f64, f64::max);
    if peak <= 0.0 {
        return None;
    }
    let level = peak * std::f64::consts::FRAC_1_SQRT_2;
    let below_at_start = mag[0] < level;
    for i in 1..mag.len() {
        let crossed = if below_at_start { mag[i] >= level } else { mag[i] < level };
        if crossed {
            let (a, b) = (mag[i - 1], mag[i]);
            let frac = if b == a { 0.0 } else { (level - a) / (b - a) };
            return Some(freqs[i - 1] + frac * (freqs[i] - freqs[i - 1]));
        }
    }
    None
}

/// Time constant (seconds) of the slowest pole, `-1 / (fs * ln r)` for a pole
/// of radius `r` (`0 < r < 1`). `None` with no pole inside the unit circle
/// that decays at all (an FIR filter, whose poles all sit at the origin).
pub fn dominant_tau(pole_radii: &[f64], fs: f64) -> Option<f64> {
    let r = pole_radii.iter().cloned().filter(|r| *r > 1e-12 && *r < 1.0).fold(0.0_f64, f64::max);
    if r <= 0.0 {
        None
    } else {
        Some(-1.0 / (fs * r.ln()))
    }
}

/// The -3 dB edges of the response's main passband: the widest run of samples
/// within 3 dB of the peak. `(low, high, first, last)`; `low` is `None` when the
/// run starts at the first sample (a lowpass), `high` is `None` when it reaches
/// the last (a highpass). Linearly interpolated. `first`/`last` are the run's
/// sample indices so a caller can average something over the passband.
pub fn band_edges_3db(freqs: &[f64], mag: &[f64]) -> Option<(Option<f64>, Option<f64>, usize, usize)> {
    if freqs.len() != mag.len() || mag.len() < 2 {
        return None;
    }
    let peak = mag.iter().cloned().fold(0.0_f64, f64::max);
    if peak <= 0.0 {
        return None;
    }
    let level = peak * std::f64::consts::FRAC_1_SQRT_2;
    let (mut best, mut start) = ((0usize, 0usize, 0usize), None::<usize>);
    for i in 0..=mag.len() {
        let inside = i < mag.len() && mag[i] >= level;
        match (inside, start) {
            (true, None) => start = Some(i),
            (false, Some(a)) => {
                let len = i - a;
                if len > best.0 {
                    best = (len, a, i - 1);
                }
                start = None;
            }
            _ => {}
        }
    }
    let (_, a, b) = best;
    let cross = |i0: usize, i1: usize| {
        let (m0, m1) = (mag[i0], mag[i1]);
        let frac = if m1 == m0 { 0.0 } else { (level - m0) / (m1 - m0) };
        freqs[i0] + frac * (freqs[i1] - freqs[i0])
    };
    let low = if a > 0 { Some(cross(a - 1, a)) } else { None };
    let high = if b + 1 < mag.len() { Some(cross(b, b + 1)) } else { None };
    Some((low, high, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y[n] = 1 - exp(-n / T): a first-order lag with time constant T samples.
    fn lag(t_samples: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| 1.0 - (-(i as f64) / t_samples).exp()).collect()
    }

    #[test]
    fn first_order_lag_has_the_textbook_numbers() {
        let y = lag(20.0, 600);
        let s = step_info(&y, 1.0).unwrap();
        assert!((s.final_value - 1.0).abs() < 1e-6);
        assert!((s.tau - 20.0).abs() < 0.2, "tau {}", s.tau); // 63.2 % at t = T
        assert!((s.rise_time - 20.0 * 9.0_f64.ln()).abs() < 0.3, "rise {}", s.rise_time); // T ln 9
        assert!((s.settling_time - 20.0 * 50.0_f64.ln()).abs() < 1.5, "settle {}", s.settling_time); // T ln 50
        assert!(s.overshoot_pct < 1e-6);
    }

    #[test]
    fn times_scale_with_the_sample_rate() {
        let y = lag(20.0, 600);
        let s = step_info(&y, 100.0).unwrap();
        assert!((s.tau - 0.2).abs() < 0.002);
    }

    #[test]
    fn overshoot_is_reported_as_percent_of_final() {
        let mut y = vec![0.0, 0.6, 1.2, 1.1];
        y.extend(std::iter::repeat(1.0).take(60));
        let s = step_info(&y, 1.0).unwrap();
        assert!((s.overshoot_pct - 20.0).abs() < 1e-9);
        assert_eq!(s.peak_time, 2.0);
    }

    #[test]
    fn a_response_that_settles_to_zero_has_no_step_info() {
        assert!(step_info(&[1.0, 0.5, 0.0, 0.0], 1.0).is_none());
        assert!(step_info(&[], 1.0).is_none());
    }

    #[test]
    fn impulse_length_counts_samples_to_the_energy_tail() {
        let h: Vec<f64> = (0..200).map(|i| 0.9_f64.powi(i)).collect();
        let n = impulse_length(&h, 0.001);
        // energy of 0.9^n decays as 0.81^n: 0.81^n < 0.001 -> n ~ 33
        assert!((30..=36).contains(&n), "{n}");
        assert_eq!(impulse_length(&[0.0, 0.0], 0.001), 0);
    }

    #[test]
    fn cutoff_finds_the_lowpass_and_highpass_corner() {
        let f: Vec<f64> = (0..101).map(|i| i as f64).collect();
        let low: Vec<f64> = f.iter().map(|x| 1.0 / (1.0 + (x / 30.0).powi(2)).sqrt()).collect();
        assert!((cutoff_3db(&f, &low).unwrap() - 30.0).abs() < 0.5);
        let high: Vec<f64> = f.iter().map(|x| 1.0 - 1.0 / (1.0 + (x / 30.0).powi(2)).sqrt()).collect();
        assert!(cutoff_3db(&f, &high).is_some());
        assert!(cutoff_3db(&f, &vec![1.0; 101]).is_none());
    }

    #[test]
    fn dominant_tau_comes_from_the_pole_closest_to_the_unit_circle() {
        let t = dominant_tau(&[0.5, (-1.0_f64 / 20.0).exp(), 0.0], 1.0).unwrap();
        assert!((t - 20.0).abs() < 1e-9);
        assert!(dominant_tau(&[0.0, 0.0], 1.0).is_none());
        assert!(dominant_tau(&[1.2], 1.0).is_none());
    }

    #[test]
    fn band_edges_find_both_sides_of_a_bandpass() {
        let f: Vec<f64> = (0..201).map(|i| i as f64).collect();
        let m: Vec<f64> = f.iter().map(|x| if (80.0..=120.0).contains(x) { 1.0 } else { 0.01 }).collect();
        let (lo, hi, a, b) = band_edges_3db(&f, &m).unwrap();
        assert!((lo.unwrap() - 79.5).abs() < 1.0 && (hi.unwrap() - 120.5).abs() < 1.0);
        assert_eq!((a, b), (80, 120));
        let low: Vec<f64> = f.iter().map(|x| 1.0 / (1.0 + (x / 30.0).powi(2)).sqrt()).collect();
        let (lo, hi, _, _) = band_edges_3db(&f, &low).unwrap();
        assert!(lo.is_none() && (hi.unwrap() - 30.0).abs() < 0.5);
    }

    #[test]
    fn a_known_final_value_beats_the_tail_of_a_truncated_response() {
        // a slow lag cut off long before it settles: the tail mean is far from 1
        let y = lag(1000.0, 3000);
        let wrong = step_info(&y, 1.0).unwrap();
        let right = step_info_to(&y, 1.0, 1.0).unwrap();
        assert!((right.tau - 1000.0).abs() < 10.0, "tau {}", right.tau);
        assert!((wrong.tau - 1000.0).abs() > 50.0);
    }
}
