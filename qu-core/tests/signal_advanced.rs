use std::f64::consts::TAU;
use std::time::{Duration, Instant};

use qu_core::fft_real;
use qu_core::signal::{
    crest_factor, snap_frequencies_to_bins, synthesize_cpu, synthesize_direct, MultisinePlan,
};

fn close(left: f64, right: f64, tolerance: f64) {
    assert!(
        (left - right).abs() <= tolerance,
        "{left} != {right} (tol={tolerance})"
    );
}

fn next_random(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 11) as f64) / ((1_u64 << 53) as f64)
}

fn battery_plan() -> (MultisinePlan, Vec<f64>, Vec<usize>) {
    let fs = 2.0e6;
    let samples = 2048;
    let requested: Vec<_> = (0..15)
        .map(|index| 2.0e3 * (200.0e3_f64 / 2.0e3).powf(index as f64 / 14.0))
        .collect();
    let snapped = snap_frequencies_to_bins(&requested, fs, samples).unwrap();
    let amplitude = vec![1.0 / (2.0 * (snapped.bins.len() as f64).sqrt()); snapped.bins.len()];
    let phases: Vec<_> = (0..snapped.bins.len())
        .map(|index| (index as f64 * 1.618).rem_euclid(TAU))
        .collect();
    (
        MultisinePlan::new(fs, samples, snapped.frequencies_hz, amplitude).unwrap(),
        phases,
        snapped.bins,
    )
}

#[test]
fn differential_randomized_cpu_kernel_matches_scalar_oracle() {
    let mut random = 0x5155_1a1_u64;
    for case in 0..64 {
        let sample_count = 64 << (case % 4);
        let tone_count = 1 + case % 17;
        let sample_rate = 48_000.0;
        let frequencies: Vec<_> = (0..tone_count)
            .map(|tone| (tone + 1) as f64 * sample_rate / sample_count as f64)
            .collect();
        let amplitudes: Vec<_> = (0..tone_count)
            .map(|_| 0.01 + next_random(&mut random))
            .collect();
        let phases: Vec<_> = (0..tone_count)
            .map(|_| next_random(&mut random) * TAU)
            .collect();
        let plan = MultisinePlan::new(sample_rate, sample_count, frequencies, amplitudes).unwrap();
        let oracle = synthesize_direct(&plan, &phases).unwrap();
        let accelerated = synthesize_cpu(&plan, &phases).unwrap();
        let max_error = oracle
            .iter()
            .zip(&accelerated)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(max_error < 2.0e-11, "case {case}: max error {max_error}");
    }
}

#[test]
fn snapped_battery_multisine_has_energy_only_in_requested_bins() {
    let (plan, phases, bins) = battery_plan();
    let signal = synthesize_cpu(&plan, &phases).unwrap();
    let spectrum = fft_real(&signal).unwrap();
    let magnitudes = spectrum.magnitudes();
    let tone_floor = bins
        .iter()
        .map(|&bin| magnitudes[bin])
        .fold(f64::INFINITY, f64::min);
    let leakage = magnitudes
        .iter()
        .take(plan.sample_count() / 2)
        .enumerate()
        .filter(|(bin, _)| !bins.contains(bin))
        .map(|(_, &magnitude)| magnitude)
        .fold(0.0, f64::max);
    assert!(tone_floor > 1.0);
    assert!(
        leakage / tone_floor < 1.0e-11,
        "relative off-bin leakage {}",
        leakage / tone_floor
    );
}

#[test]
fn synthesis_is_deterministic_and_crest_factor_is_scale_invariant() {
    let (plan, phases, _) = battery_plan();
    let first = synthesize_cpu(&plan, &phases).unwrap();
    let second = synthesize_cpu(&plan, &phases).unwrap();
    assert_eq!(first, second);
    let scaled: Vec<_> = first.iter().map(|sample| sample * -7.25).collect();
    close(
        crest_factor(&first).unwrap(),
        crest_factor(&scaled).unwrap(),
        1.0e-12,
    );
}

#[test]
fn accelerated_cpu_kernel_is_fast_and_faster_than_oracle() {
    let (plan, phases, _) = battery_plan();
    let rounds = if cfg!(debug_assertions) { 20 } else { 200 };
    let _ = synthesize_cpu(&plan, &phases).unwrap();

    let start = Instant::now();
    for _ in 0..rounds {
        std::hint::black_box(synthesize_direct(&plan, &phases).unwrap());
    }
    let direct = start.elapsed();

    let start = Instant::now();
    for _ in 0..rounds {
        std::hint::black_box(synthesize_cpu(&plan, &phases).unwrap());
    }
    let accelerated = start.elapsed();

    let per_waveform = accelerated / rounds;
    eprintln!(
        "CPU multisine: direct={:.3} ms, accelerated={:.3} ms, speedup={:.2}x, accelerated/waveform={:.3} ms",
        direct.as_secs_f64() * 1e3,
        accelerated.as_secs_f64() * 1e3,
        direct.as_secs_f64() / accelerated.as_secs_f64(),
        per_waveform.as_secs_f64() * 1e3,
    );
    assert!(
        accelerated < direct,
        "optimized recurrence must beat the scalar cosine oracle"
    );
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(20)
    } else {
        Duration::from_millis(1)
    };
    assert!(
        per_waveform < budget,
        "CPU synthesis {:?} exceeded {:?} budget",
        per_waveform,
        budget
    );
}
