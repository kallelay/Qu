use std::f64::consts::TAU;

use qu_core::linalg::{least_squares, pseudo_inverse};
use qu_core::matrix::Matrix;
use qu_core::signal::{synthesize_direct, MultisinePlan};
use qu_core::{fft_complex, ifft_complex, Complex64};

fn next_random(state: &mut u64) -> f64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    ((*state >> 11) as f64) / ((1_u64 << 53) as f64) * 2.0 - 1.0
}

fn naive_dft(input: &[Complex64]) -> Vec<Complex64> {
    let n = input.len() as f64;
    (0..input.len())
        .map(|bin| {
            input
                .iter()
                .enumerate()
                .fold(Complex64::default(), |sum, (sample, &value)| {
                    let angle = -TAU * bin as f64 * sample as f64 / n;
                    sum.add(value.mul(Complex64::from_polar(1.0, angle)))
                })
        })
        .collect()
}

fn complex_error(left: &[Complex64], right: &[Complex64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(&a, &b)| a.sub(b).magnitude())
        .fold(0.0, f64::max)
}

fn subtract(left: &Matrix, right: &Matrix) -> Matrix {
    left.broadcast(right, |a, b| a - b).unwrap()
}

fn frobenius(matrix: &Matrix) -> f64 {
    matrix
        .as_slice()
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
}

fn relative_matrix_error(actual: &Matrix, expected: &Matrix) -> f64 {
    frobenius(&subtract(actual, expected)) / frobenius(expected).max(f64::MIN_POSITIVE)
}

#[test]
fn fft_matches_independent_naive_dft_over_random_complex_inputs() {
    let mut random = 0xD17F_EA5E_u64;
    for length in [2, 4, 8, 16, 32, 64, 128] {
        for case in 0..8 {
            let input: Vec<_> = (0..length)
                .map(|_| Complex64::new(next_random(&mut random), next_random(&mut random)))
                .collect();
            let expected = naive_dft(&input);
            let actual = fft_complex(&input).unwrap();
            let error = complex_error(&actual, &expected);
            assert!(
                error < 2.0e-11,
                "length {length}, case {case}: DFT error {error}"
            );
        }
    }
}

#[test]
fn fft_obeys_parseval_shift_and_inverse_identities() {
    let mut random = 0xA11C_E55_u64;
    let input: Vec<_> = (0..256)
        .map(|_| Complex64::new(next_random(&mut random), next_random(&mut random)))
        .collect();
    let spectrum = fft_complex(&input).unwrap();
    let time_energy: f64 = input.iter().map(|value| value.magnitude().powi(2)).sum();
    let frequency_energy: f64 = spectrum
        .iter()
        .map(|value| value.magnitude().powi(2))
        .sum::<f64>()
        / input.len() as f64;
    assert!((time_energy - frequency_energy).abs() / time_energy < 2.0e-13);

    let restored = ifft_complex(&spectrum).unwrap();
    let round_trip_error = complex_error(&restored, &input);
    assert!(
        round_trip_error < 5.0e-14,
        "FFT/IFFT round-trip error {round_trip_error}"
    );

    let shifted: Vec<_> = (0..input.len())
        .map(|sample| input[sample].scale(if sample % 2 == 0 { 1.0 } else { -1.0 }))
        .collect();
    let shifted_spectrum = fft_complex(&shifted).unwrap();
    for bin in 0..input.len() {
        let expected = spectrum[(bin + input.len() / 2) % input.len()];
        assert!(shifted_spectrum[bin].sub(expected).magnitude() < 2.0e-13);
    }
}

#[test]
fn pseudoinverse_satisfies_all_four_moore_penrose_conditions() {
    let cases = [
        Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 5.0], vec![7.0, 11.0]]).unwrap(),
        Matrix::from_rows(&[vec![1.0, 2.0, 3.0], vec![2.0, 4.0, 6.0]]).unwrap(),
        Matrix::from_rows(&[
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0e-10, 0.0],
            vec![0.0, 0.0, 1.0e-14],
        ])
        .unwrap(),
    ];

    for (case, matrix) in cases.iter().enumerate() {
        let inverse = pseudo_inverse(matrix, Some(1.0e-12)).unwrap().matrix;
        let a_pa = matrix.matmul(&inverse).unwrap().matmul(matrix).unwrap();
        let pa_a_pa = inverse.matmul(matrix).unwrap().matmul(&inverse).unwrap();
        let a_pa_projector = matrix.matmul(&inverse).unwrap();
        let pa_a_projector = inverse.matmul(matrix).unwrap();

        assert!(
            relative_matrix_error(&a_pa, matrix) < 2.0e-10,
            "case {case}: A A+ A"
        );
        assert!(
            relative_matrix_error(&pa_a_pa, &inverse) < 2.0e-10,
            "case {case}: A+ A A+"
        );
        assert!(
            relative_matrix_error(&a_pa_projector.transpose(), &a_pa_projector) < 2.0e-12,
            "case {case}: A A+ symmetry"
        );
        assert!(
            relative_matrix_error(&pa_a_projector.transpose(), &pa_a_projector) < 2.0e-12,
            "case {case}: A+ A symmetry"
        );
    }
}

#[test]
fn least_squares_residual_is_orthogonal_to_column_space() {
    let coefficients = Matrix::from_rows(&[
        vec![1.0, 0.0, 2.0],
        vec![1.0, 1.0, 0.0],
        vec![1.0, 2.0, 1.0],
        vec![1.0, 3.0, 4.0],
        vec![1.0, 4.0, 2.0],
    ])
    .unwrap();
    let observations = Matrix::from_column(&[1.0, 2.0, 2.5, 5.0, 4.0]);
    let solution = least_squares(&coefficients, &observations, None).unwrap();
    let residual = subtract(&coefficients.matmul(&solution).unwrap(), &observations);
    let normal_residual = coefficients.transpose().matmul(&residual).unwrap();
    assert!(frobenius(&normal_residual) < 2.0e-13);
}

#[test]
fn rpg_l2p_analytic_jacobian_matches_central_difference() {
    let frequency = vec![1000.0, 3000.0, 7000.0, 11_000.0];
    let amplitude = vec![0.2, 0.15, 0.1, 0.05];
    let phase = vec![0.3, 1.0, 2.2, 5.1];
    let plan = MultisinePlan::new(48_000.0, 64, frequency.clone(), amplitude.clone()).unwrap();
    let signal = synthesize_direct(&plan, &phase).unwrap();
    let q = 4.0;
    let epsilon = 1.0e-6;

    for tone in 0..phase.len() {
        let mut plus = phase.clone();
        let mut minus = phase.clone();
        plus[tone] += epsilon;
        minus[tone] -= epsilon;
        let plus_signal = synthesize_direct(&plan, &plus).unwrap();
        let minus_signal = synthesize_direct(&plan, &minus).unwrap();

        for sample in 0..plan.sample_count() {
            let numerical =
                (plus_signal[sample].powf(q) - minus_signal[sample].powf(q)) / (2.0 * epsilon);
            let time = sample as f64 / plan.sample_rate_hz();
            let analytic = -q
                * amplitude[tone]
                * signal[sample].powf(q - 1.0)
                * (TAU * frequency[tone] * time + phase[tone]).sin();
            let scale = numerical.abs().max(analytic.abs()).max(1.0e-8);
            assert!(
                (numerical - analytic).abs() / scale < 2.0e-7,
                "tone {tone}, sample {sample}: analytic={analytic}, numerical={numerical}"
            );
        }
    }
}
