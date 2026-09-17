//! Signal-generation reference semantics and optimized portable CPU kernels.
//!
//! The direct kernel is the numerical oracle. The recurrence kernel removes one
//! transcendental `cos` call per tone/sample while preserving the same waveform.

use std::error::Error;
use std::f64::consts::TAU;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, PartialEq)]
pub enum SignalError {
    EmptyTones,
    Shape {
        frequencies: usize,
        amplitudes: usize,
        phases: usize,
    },
    InvalidSampleRate(f64),
    InvalidSampleCount(usize),
    InvalidFrequency(f64),
    NonFiniteInput,
    ZeroPeak,
    InvalidDac,
}

impl Display for SignalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTones => write!(f, "multisine requires at least one tone"),
            Self::Shape { frequencies, amplitudes, phases } => write!(
                f,
                "tone shape mismatch: {frequencies} frequencies, {amplitudes} amplitudes, {phases} phases"
            ),
            Self::InvalidSampleRate(value) => write!(f, "invalid sample rate {value}"),
            Self::InvalidSampleCount(value) => write!(f, "invalid sample count {value}"),
            Self::InvalidFrequency(value) => write!(f, "frequency {value} is outside (0, Nyquist)"),
            Self::NonFiniteInput => write!(f, "signal inputs must be finite"),
            Self::ZeroPeak => write!(f, "cannot peak-normalize an all-zero signal"),
            Self::InvalidDac => write!(f, "DAC full scale, amplitude, and maximum code must be positive"),
        }
    }
}

impl Error for SignalError {}

#[derive(Clone, Debug, PartialEq)]
pub struct MultisinePlan {
    sample_rate_hz: f64,
    sample_count: usize,
    frequencies_hz: Vec<f64>,
    amplitudes: Vec<f64>,
}

impl MultisinePlan {
    pub fn new(
        sample_rate_hz: f64,
        sample_count: usize,
        frequencies_hz: Vec<f64>,
        amplitudes: Vec<f64>,
    ) -> Result<Self, SignalError> {
        if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
            return Err(SignalError::InvalidSampleRate(sample_rate_hz));
        }
        if sample_count == 0 {
            return Err(SignalError::InvalidSampleCount(sample_count));
        }
        if frequencies_hz.is_empty() {
            return Err(SignalError::EmptyTones);
        }
        if frequencies_hz.len() != amplitudes.len() {
            return Err(SignalError::Shape {
                frequencies: frequencies_hz.len(),
                amplitudes: amplitudes.len(),
                phases: 0,
            });
        }
        if frequencies_hz
            .iter()
            .chain(amplitudes.iter())
            .any(|value| !value.is_finite())
        {
            return Err(SignalError::NonFiniteInput);
        }
        let nyquist = sample_rate_hz / 2.0;
        if let Some(&bad) = frequencies_hz
            .iter()
            .find(|&&frequency| frequency <= 0.0 || frequency >= nyquist)
        {
            return Err(SignalError::InvalidFrequency(bad));
        }
        Ok(Self {
            sample_rate_hz,
            sample_count,
            frequencies_hz,
            amplitudes,
        })
    }

    pub fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }
    pub fn frequencies_hz(&self) -> &[f64] {
        &self.frequencies_hz
    }
    pub fn amplitudes(&self) -> &[f64] {
        &self.amplitudes
    }
    pub fn tone_count(&self) -> usize {
        self.frequencies_hz.len()
    }

    fn validate_phases(&self, phases: &[f64]) -> Result<(), SignalError> {
        if phases.len() != self.tone_count() {
            return Err(SignalError::Shape {
                frequencies: self.tone_count(),
                amplitudes: self.amplitudes.len(),
                phases: phases.len(),
            });
        }
        if phases.iter().any(|phase| !phase.is_finite()) {
            return Err(SignalError::NonFiniteInput);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnappedFrequencies {
    pub bins: Vec<usize>,
    pub frequencies_hz: Vec<f64>,
    pub bin_width_hz: f64,
}

/// Snap requested tones to unique, positive DFT bins below Nyquist.
pub fn snap_frequencies_to_bins(
    requested_hz: &[f64],
    sample_rate_hz: f64,
    sample_count: usize,
) -> Result<SnappedFrequencies, SignalError> {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return Err(SignalError::InvalidSampleRate(sample_rate_hz));
    }
    if sample_count < 2 {
        return Err(SignalError::InvalidSampleCount(sample_count));
    }
    if requested_hz.iter().any(|frequency| !frequency.is_finite()) {
        return Err(SignalError::NonFiniteInput);
    }
    let bin_width_hz = sample_rate_hz / sample_count as f64;
    let mut bins = Vec::with_capacity(requested_hz.len());
    for &frequency in requested_hz {
        let bin = (frequency / bin_width_hz).round() as isize;
        if bin <= 0 || bin as usize >= sample_count / 2 {
            return Err(SignalError::InvalidFrequency(frequency));
        }
        bins.push(bin as usize);
    }
    bins.sort_unstable();
    bins.dedup();
    if bins.is_empty() {
        return Err(SignalError::EmptyTones);
    }
    let frequencies_hz = bins.iter().map(|&bin| bin as f64 * bin_width_hz).collect();
    Ok(SnappedFrequencies {
        bins,
        frequencies_hz,
        bin_width_hz,
    })
}

/// Scalar definition of multisine synthesis. Use as a correctness oracle.
pub fn synthesize_direct(plan: &MultisinePlan, phases: &[f64]) -> Result<Vec<f64>, SignalError> {
    plan.validate_phases(phases)?;
    let mut output = vec![0.0; plan.sample_count];
    for (sample, value) in output.iter_mut().enumerate() {
        let time = sample as f64 / plan.sample_rate_hz;
        *value = plan
            .frequencies_hz
            .iter()
            .zip(plan.amplitudes.iter())
            .zip(phases.iter())
            .map(|((&frequency, &amplitude), &phase)| {
                amplitude * (TAU * frequency * time + phase).cos()
            })
            .sum();
    }
    Ok(output)
}

/// Optimized CPU synthesis using a stable per-tone oscillator recurrence.
pub fn synthesize_cpu(plan: &MultisinePlan, phases: &[f64]) -> Result<Vec<f64>, SignalError> {
    plan.validate_phases(phases)?;
    let mut output = vec![0.0; plan.sample_count];
    for ((&frequency, &amplitude), &phase) in plan
        .frequencies_hz
        .iter()
        .zip(plan.amplitudes.iter())
        .zip(phases.iter())
    {
        let delta = TAU * frequency / plan.sample_rate_hz;
        let (sin_delta, cos_delta) = delta.sin_cos();
        let (mut sin_value, mut cos_value) = phase.sin_cos();
        for (sample, value) in output.iter_mut().enumerate() {
            *value += amplitude * cos_value;
            let next_cos = cos_value * cos_delta - sin_value * sin_delta;
            let next_sin = sin_value * cos_delta + cos_value * sin_delta;
            cos_value = next_cos;
            sin_value = next_sin;
            if sample & 255 == 255 {
                let norm = cos_value.hypot(sin_value);
                cos_value /= norm;
                sin_value /= norm;
            }
        }
    }
    Ok(output)
}

pub fn crest_factor(signal: &[f64]) -> Result<f64, SignalError> {
    if signal.is_empty() || signal.iter().any(|value| !value.is_finite()) {
        return Err(SignalError::NonFiniteInput);
    }
    let peak = signal
        .iter()
        .fold(0.0_f64, |value, sample| value.max(sample.abs()));
    let rms =
        (signal.iter().map(|sample| sample * sample).sum::<f64>() / signal.len() as f64).sqrt();
    if rms == 0.0 {
        return Err(SignalError::ZeroPeak);
    }
    Ok(peak / rms)
}

pub fn normalize_peak(signal: &[f64]) -> Result<(Vec<f64>, f64), SignalError> {
    if signal.is_empty() || signal.iter().any(|value| !value.is_finite()) {
        return Err(SignalError::NonFiniteInput);
    }
    let peak = signal
        .iter()
        .fold(0.0_f64, |value, sample| value.max(sample.abs()));
    if peak == 0.0 {
        return Err(SignalError::ZeroPeak);
    }
    Ok((signal.iter().map(|sample| sample / peak).collect(), peak))
}

pub fn quantize_bipolar_dac(
    normalized: &[f64],
    signal_amplitude_volts: f64,
    full_scale_volts: f64,
    max_code: u32,
) -> Result<Vec<u32>, SignalError> {
    if signal_amplitude_volts <= 0.0 || full_scale_volts <= 0.0 || max_code == 0 {
        return Err(SignalError::InvalidDac);
    }
    if normalized.iter().any(|sample| !sample.is_finite()) {
        return Err(SignalError::NonFiniteInput);
    }
    let gain = signal_amplitude_volts / full_scale_volts * max_code as f64;
    let midpoint = max_code as f64 / 2.0;
    Ok(normalized
        .iter()
        .map(|sample| {
            (sample * gain + midpoint)
                .round()
                .clamp(0.0, max_code as f64) as u32
        })
        .collect())
}

pub fn encode_f64_be(values: &[f64]) -> Result<Vec<u8>, SignalError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(SignalError::NonFiniteInput);
    }
    Ok(values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MageRole {
    Global,
    Local,
}

pub fn mage_target(role: MageRole, global_best: usize, party_best: usize) -> usize {
    match role {
        MageRole::Global => global_best,
        MageRole::Local => party_best,
    }
}

pub fn crossover(left: &[f64], right: &[f64], mix: f64) -> Result<Vec<f64>, SignalError> {
    if left.len() != right.len() || !(0.0..=1.0).contains(&mix) {
        return Err(SignalError::Shape {
            frequencies: left.len(),
            amplitudes: right.len(),
            phases: 0,
        });
    }
    Ok(left
        .iter()
        .zip(right)
        .map(|(&a, &b)| mix * a + (1.0 - mix) * b)
        .collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerLifecycle {
    pub active: bool,
    pub party: Option<usize>,
    pub life: usize,
    pub hit_best: bool,
}

impl PlayerLifecycle {
    pub fn eliminate(&mut self) {
        self.active = false;
        self.party = None;
    }
    pub fn reset_after_merge(&mut self, party: usize) {
        if self.active {
            self.party = Some(party);
            self.life = 0;
            self.hit_best = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> MultisinePlan {
        MultisinePlan::new(
            2.0e6,
            2048,
            vec![1953.125, 9765.625, 199218.75],
            vec![0.2; 3],
        )
        .unwrap()
    }

    #[test]
    fn optimized_cpu_matches_definition() {
        let phases = [0.1, 2.0, 5.5];
        let direct = synthesize_direct(&plan(), &phases).unwrap();
        let fast = synthesize_cpu(&plan(), &phases).unwrap();
        let max_error = direct
            .iter()
            .zip(&fast)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(max_error < 2.0e-12, "maximum synthesis error {max_error}");
    }

    #[test]
    fn role_and_lifecycle_regressions_stay_fixed() {
        assert_eq!(mage_target(MageRole::Global, 7, 3), 7);
        assert_eq!(mage_target(MageRole::Local, 7, 3), 3);
        assert_eq!(
            crossover(&[0.0, 2.0], &[10.0, 6.0], 0.25).unwrap(),
            vec![7.5, 5.0]
        );
        let mut player = PlayerLifecycle {
            active: true,
            party: Some(2),
            life: 8,
            hit_best: true,
        };
        player.eliminate();
        player.reset_after_merge(1);
        assert_eq!(
            player,
            PlayerLifecycle {
                active: false,
                party: None,
                life: 8,
                hit_best: true
            }
        );
    }

    #[test]
    fn bin_snapping_is_sorted_unique_and_zero_based() {
        let snapped = snap_frequencies_to_bins(&[2000.0, 2050.0, 200_000.0], 2.0e6, 2048).unwrap();
        assert_eq!(snapped.bins, vec![2, 205]);
        assert_eq!(snapped.frequencies_hz, vec![1953.125, 200195.3125]);
    }

    #[test]
    fn normalization_dac_and_big_endian_export_are_bounded() {
        let (normalized, peak) = normalize_peak(&[-2.0, 0.0, 1.0]).unwrap();
        assert_eq!(peak, 2.0);
        assert_eq!(normalized, vec![-1.0, 0.0, 0.5]);
        let dac = quantize_bipolar_dac(&normalized, 1.6, 3.3, 4095).unwrap();
        assert!(dac.iter().all(|&code| code <= 4095));
        assert_eq!(encode_f64_be(&[1.0]).unwrap(), 1.0_f64.to_be_bytes());
    }
}
