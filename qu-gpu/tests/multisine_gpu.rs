use std::time::{Duration, Instant};

use qu_core::signal::{snap_frequencies_to_bins, synthesize_direct, MultisinePlan};
use qu_gpu::{GpuContext, GpuError};

fn battery_inputs(players: usize) -> (MultisinePlan, Vec<f32>, Vec<f32>, Vec<f32>) {
    let sample_rate = 2.0e6;
    let samples = 2048;
    let requested: Vec<_> = (0..15)
        .map(|index| 2.0e3 * (100.0_f64).powf(index as f64 / 14.0))
        .collect();
    let snapped = snap_frequencies_to_bins(&requested, sample_rate, samples).unwrap();
    let amplitudes = vec![1.0 / (2.0 * (snapped.bins.len() as f64).sqrt()); snapped.bins.len()];
    let mut phases = Vec::with_capacity(players * snapped.bins.len());
    for player in 0..players {
        for tone in 0..snapped.bins.len() {
            phases
                .push(((player * 17 + tone * 31) as f64 * 0.013).rem_euclid(std::f64::consts::TAU));
        }
    }
    let plan =
        MultisinePlan::new(sample_rate, samples, snapped.frequencies_hz, amplitudes).unwrap();
    (
        plan.clone(),
        plan.frequencies_hz()
            .iter()
            .map(|&value| value as f32)
            .collect(),
        plan.amplitudes()
            .iter()
            .map(|&value| value as f32)
            .collect(),
        phases.iter().map(|&value| value as f32).collect(),
    )
}

#[test]
fn gpu_population_matches_cpu_and_meets_dispatch_budget() {
    let gpu = match GpuContext::new_blocking() {
        Ok(gpu) => gpu,
        Err(GpuError::NoAdapter) => {
            if std::env::var_os("QU_REQUIRE_GPU").is_some() {
                panic!("QU_REQUIRE_GPU is set but no compatible WebGPU adapter was found");
            }
            eprintln!("GPU SKIP: no compatible WebGPU adapter");
            return;
        }
        Err(error) => panic!("GPU initialization failed: {error}"),
    };
    let players = 80;
    let (plan, frequencies, amplitudes, phases) = battery_inputs(players);

    let first = gpu
        .synthesize_batch(2.0e6, 2048, &frequencies, &amplitudes, &phases)
        .unwrap();
    let second = gpu
        .synthesize_batch(2.0e6, 2048, &frequencies, &amplitudes, &phases)
        .unwrap();
    assert_eq!(
        first, second,
        "GPU output must be deterministic on one adapter"
    );

    let periodic_phases: Vec<_> = phases
        .iter()
        .map(|phase| phase + std::f32::consts::TAU)
        .collect();
    let periodic = gpu
        .synthesize_batch(2.0e6, 2048, &frequencies, &amplitudes, &periodic_phases)
        .unwrap();
    let periodic_error = first
        .iter()
        .zip(&periodic)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(
        periodic_error < 1.0e-4,
        "2*pi phase periodicity error {periodic_error}"
    );

    let mut reversed_frequencies = frequencies.clone();
    let mut reversed_amplitudes = amplitudes.clone();
    reversed_frequencies.reverse();
    reversed_amplitudes.reverse();
    let mut reversed_phases = Vec::with_capacity(phases.len());
    for player in 0..players {
        let mut row = phases[player * plan.tone_count()..(player + 1) * plan.tone_count()].to_vec();
        row.reverse();
        reversed_phases.extend(row);
    }
    let permuted = gpu
        .synthesize_batch(
            2.0e6,
            2048,
            &reversed_frequencies,
            &reversed_amplitudes,
            &reversed_phases,
        )
        .unwrap();
    let permutation_error = first
        .iter()
        .zip(&permuted)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(
        permutation_error < 2.0e-6,
        "tone permutation error {permutation_error}"
    );

    for player in [0, 17, 79] {
        let tone_count = plan.tone_count();
        let player_phases: Vec<_> = phases[player * tone_count..(player + 1) * tone_count]
            .iter()
            .map(|&value| value as f64)
            .collect();
        let cpu = synthesize_direct(&plan, &player_phases).unwrap();
        let gpu_row = &first[player * plan.sample_count()..(player + 1) * plan.sample_count()];
        let max_error = cpu
            .iter()
            .zip(gpu_row)
            .map(|(&a, &b)| (a - b as f64).abs())
            .fold(0.0, f64::max);
        assert!(
            max_error < 4.0e-4,
            "player {player} CPU/GPU max error {max_error}"
        );
    }

    // Shape sweep catches workgroup tails, single-tone handling, and indexing
    // mistakes hidden by production-sized divisible dimensions.
    for (tone_count, sample_count, player_count) in
        [(1, 63, 1), (3, 257, 5), (8, 511, 9), (17, 1025, 3)]
    {
        let shape_frequencies: Vec<_> = (1..=tone_count)
            .map(|tone| tone as f32 * 48_000.0 / (2 * tone_count + 3) as f32)
            .collect();
        let shape_amplitudes = vec![1.0 / tone_count as f32; tone_count];
        let shape_phases: Vec<_> = (0..player_count * tone_count)
            .map(|index| index as f32 * 0.137)
            .collect();
        let gpu_values = gpu
            .synthesize_batch(
                48_000.0,
                sample_count,
                &shape_frequencies,
                &shape_amplitudes,
                &shape_phases,
            )
            .unwrap();
        let shape_plan = MultisinePlan::new(
            48_000.0,
            sample_count,
            shape_frequencies
                .iter()
                .map(|&value| value as f64)
                .collect(),
            shape_amplitudes.iter().map(|&value| value as f64).collect(),
        )
        .unwrap();
        for player in 0..player_count {
            let player_phases: Vec<_> = shape_phases
                [player * tone_count..(player + 1) * tone_count]
                .iter()
                .map(|&value| value as f64)
                .collect();
            let cpu_values = synthesize_direct(&shape_plan, &player_phases).unwrap();
            let row = &gpu_values[player * sample_count..(player + 1) * sample_count];
            let error = cpu_values
                .iter()
                .zip(row)
                .map(|(&a, &b)| (a - b as f64).abs())
                .fold(0.0, f64::max);
            assert!(
                error < 8.0e-4,
                "shape {player_count}x{tone_count}x{sample_count}, player {player}: {error}"
            );
        }
    }

    let rounds = 5;
    let start = Instant::now();
    for _ in 0..rounds {
        std::hint::black_box(
            gpu.synthesize_batch(2.0e6, 2048, &frequencies, &amplitudes, &phases)
                .unwrap(),
        );
    }
    let elapsed = start.elapsed();
    let per_population = elapsed / rounds;
    eprintln!(
        "GPU multisine: adapter='{}' backend={:?}, 80x15x2048 population={:.3} ms ({:.3} us/waveform)",
        gpu.adapter_info().name,
        gpu.adapter_info().backend,
        per_population.as_secs_f64() * 1e3,
        per_population.as_secs_f64() * 1e6 / players as f64,
    );
    assert!(
        per_population < Duration::from_millis(250),
        "GPU population dispatch {:?} exceeded interactive budget",
        per_population
    );
}
