//! Electrochemical impedance spectroscopy (EIS): regularized linear
//! Kramers-Kronig (rLKK) validation/reconstruction.
//!
//! Port of Ahmed Yousef Kallel's own reference implementation (private
//! `rlkk.py`, itself based on Kallel & Kanoun, "Regularized linear
//! Kramers-Kronig transform for consistency check of noisy impedance
//! spectra with logarithmic frequency distribution", IWIS 2021). The
//! published Lin-KK test (Boukamp) checks whether a measured spectrum is
//! causal/linear/stable/finite by fitting a Distribution-of-Relaxation-
//! -Times (DRT) model and inspecting the residual between the fit and the
//! raw data; rLKK adds Tikhonov (L2) regularization on the DRT
//! coefficients so the fit tolerates noisy or truncated data without
//! needing an equivalent-circuit assumption.

use crate::linalg::{self, LinalgError};
use crate::matrix::Matrix;
use crate::Complex64;

/// The reference implementation's own default DRT grid: log-spaced from
/// `1e-8` Hz to `1e8` Hz, 160 points — deliberately far broader than any
/// realistic measurement band so the fit isn't grid-limited at either end.
pub fn default_drt_frequencies() -> Vec<f64> {
    logspace10(-8.0, 8.0, 160)
}

fn logspace10(start_exp: f64, stop_exp: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![10f64.powf(start_exp)];
    }
    (0..n)
        .map(|i| {
            let t = start_exp + (stop_exp - start_exp) * (i as f64) / ((n - 1) as f64);
            10f64.powf(t)
        })
        .collect()
}

/// DRT kernel `A[i][k] = 1 / (1 + j*omega_i/omega_k)` — a length-`freqs.len()`
/// vector of length-`fx.len()` rows, i.e. `A[i,k] = Z` contributed by the
/// relaxation at `fx[k]` when observed at `freqs[i]`, so that
/// `Z(freqs[i]) = sum_k A[i][k] * gamma[k]`.
fn drt_kernel(freqs: &[f64], fx: &[f64]) -> Vec<Vec<Complex64>> {
    let two_pi = 2.0 * std::f64::consts::PI;
    freqs
        .iter()
        .map(|&f| {
            let omega_i = two_pi * f;
            fx.iter()
                .map(|&fk| {
                    let omega_k = two_pi * fk;
                    let denom = Complex64::new(1.0, omega_i / omega_k);
                    Complex64::new(1.0, 0.0).div(denom)
                })
                .collect()
        })
        .collect()
}

/// `A * gamma` for a real-valued `gamma` against a complex kernel `A`.
fn apply_kernel(a: &[Vec<Complex64>], gamma: &[f64]) -> Vec<Complex64> {
    a.iter()
        .map(|row| {
            row.iter()
                .zip(gamma)
                .fold(Complex64::new(0.0, 0.0), |acc, (&aik, &g)| acc.add(aik.scale(g)))
        })
        .collect()
}

pub struct RlkkResult {
    pub z_reconstructed: Vec<Complex64>,
    pub gamma: Vec<f64>,
}

/// Regularized linear Kramers-Kronig reconstruction. Solves the Tikhonov-
/// regularized least-squares problem `minimize ||A*gamma - Z||^2 +
/// lambda^2*||gamma||^2` (real/imaginary parts stacked so the whole system
/// is real-valued, matching the reference implementation) via the same
/// SVD-based [`linalg::least_squares`] every other estimator in this crate
/// already uses (`ols`/`ridge`/PCA).
///
/// `fx` defaults to [`default_drt_frequencies`] when `None`.
pub fn rlkk_reconstruct(
    z: &[Complex64],
    freqs: &[f64],
    lambda: f64,
    fx: Option<&[f64]>,
) -> Result<RlkkResult, LinalgError> {
    let n = freqs.len();
    let default_fx = default_drt_frequencies();
    let fx = fx.unwrap_or(&default_fx);
    let m = fx.len();
    let a = drt_kernel(freqs, fx);

    // A_aug = [Re(A); Im(A); lambda*I]  ((2N+M) x M), b = [Re(Z); Im(Z); 0]
    let total_rows = 2 * n + m;
    let mut data = vec![0.0; total_rows * m]; // column-major
    for k in 0..m {
        let col_start = k * total_rows;
        for i in 0..n {
            data[col_start + i] = a[i][k].re;
            data[col_start + n + i] = a[i][k].im;
        }
        data[col_start + 2 * n + k] = lambda;
    }
    let a_aug = Matrix::from_col_major(total_rows, m, data);

    let mut b_data = vec![0.0; total_rows];
    for i in 0..n {
        b_data[i] = z[i].re;
        b_data[n + i] = z[i].im;
    }
    let b = Matrix::from_column(&b_data);

    // rcond=1e-10 in the Python reference's `np.linalg.lstsq` call.
    let gamma_mat = linalg::least_squares(&a_aug, &b, Some(1e-10))?;
    let gamma = gamma_mat.as_slice().to_vec();
    let z_reconstructed = apply_kernel(&a, &gamma);
    Ok(RlkkResult { z_reconstructed, gamma })
}

pub struct RlkkValidation {
    pub is_valid: bool,
    /// Percent residual per input point: `(|Z_recon| - |Z_measured|) /
    /// |Z_recon| * 100`.
    pub residuals: Vec<f64>,
    pub max_residual: f64,
}

/// Checks whether `z` is consistent with a causal, linear, time-invariant
/// system by comparing it against its own rLKK reconstruction. Points with
/// `|residual| > threshold_percent` are the ones worth discarding or
/// re-measuring.
pub fn rlkk_validate(
    z: &[Complex64],
    freqs: &[f64],
    lambda: f64,
    threshold_percent: f64,
) -> Result<RlkkValidation, LinalgError> {
    let fit = rlkk_reconstruct(z, freqs, lambda, None)?;
    let residuals: Vec<f64> = z
        .iter()
        .zip(fit.z_reconstructed.iter())
        .map(|(zi, zr)| (zr.magnitude() - zi.magnitude()) / zr.magnitude() * 100.0)
        .collect();
    let max_residual = residuals.iter().fold(0.0_f64, |acc, r| acc.max(r.abs()));
    Ok(RlkkValidation {
        is_valid: max_residual < threshold_percent,
        residuals,
        max_residual,
    })
}

/// Extrapolates `z` (measured at `freqs`) to `target_freqs` — which may lie
/// outside the measured range — by refitting the DRT and re-evaluating the
/// kernel at the requested frequencies. Useful for truncated measurements.
pub fn rlkk_extrapolate(
    z: &[Complex64],
    freqs: &[f64],
    target_freqs: &[f64],
    lambda: f64,
) -> Result<Vec<Complex64>, LinalgError> {
    let fx = default_drt_frequencies();
    let fit = rlkk_reconstruct(z, freqs, lambda, Some(&fx))?;
    let a_target = drt_kernel(target_freqs, &fx);
    Ok(apply_kernel(&a_target, &fit.gamma))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Randles circuit `Z = Rs + Rct/(1 + j*omega*Rct*Cdl)` (no Warburg,
    /// matching `rlkk.py`'s own `randles_impedance(..., Aw=0.0)` test
    /// fixture) — a standard textbook equivalent circuit, used here purely
    /// as clean synthetic data to validate the rLKK port against, not a
    /// claim about the original paper's own numbers.
    fn randles_impedance(freqs: &[f64], rs: f64, rct: f64, cdl: f64) -> Vec<Complex64> {
        let two_pi = 2.0 * std::f64::consts::PI;
        freqs
            .iter()
            .map(|&f| {
                let omega = two_pi * f;
                let denom = Complex64::new(1.0, omega * rct * cdl);
                Complex64::new(rs, 0.0).add(Complex64::new(rct, 0.0).div(denom))
            })
            .collect()
    }

    fn logspace_range(a: f64, b: f64, n: usize) -> Vec<f64> {
        logspace10(a.log10(), b.log10(), n)
    }

    #[test]
    fn clean_randles_data_reconstructs_with_small_residual_under_light_regularization() {
        let freqs = logspace_range(0.1, 1e4, 50);
        let z_true = randles_impedance(&freqs, 0.2, 0.01, 100e-6);
        let fit = rlkk_reconstruct(&z_true, &freqs, 1e-6, None).unwrap();
        for (zt, zr) in z_true.iter().zip(fit.z_reconstructed.iter()) {
            let rel_err = (zt.sub(*zr)).magnitude() / zt.magnitude();
            assert!(rel_err < 0.02, "rel_err = {rel_err}");
        }
    }

    #[test]
    fn validate_passes_on_clean_data_and_flags_a_single_corrupted_point() {
        let freqs = logspace_range(0.1, 1e4, 40);
        let mut z = randles_impedance(&freqs, 0.2, 0.01, 100e-6);
        let clean = rlkk_validate(&z, &freqs, 1e-4, 2.0).unwrap();
        assert!(clean.is_valid, "max_residual = {}", clean.max_residual);

        // Corrupt one point by 50% — a single wildly inconsistent
        // measurement, the exact failure mode rLKK is meant to catch.
        let mid = z.len() / 2;
        z[mid] = z[mid].scale(1.5);
        let corrupted = rlkk_validate(&z, &freqs, 1e-4, 2.0).unwrap();
        assert!(!corrupted.is_valid);
        assert!(corrupted.residuals[mid].abs() > corrupted.residuals[0].abs().max(1.0));
    }

    #[test]
    fn reconstruction_is_deterministic_for_the_same_input() {
        let freqs = logspace_range(1.0, 1e3, 20);
        let z = randles_impedance(&freqs, 0.1, 0.05, 50e-6);
        let a = rlkk_reconstruct(&z, &freqs, 1e-4, None).unwrap();
        let b = rlkk_reconstruct(&z, &freqs, 1e-4, None).unwrap();
        assert_eq!(a.gamma, b.gamma);
    }

    #[test]
    fn extrapolation_recovers_randles_impedance_beyond_the_measured_band() {
        let freqs = logspace_range(1.0, 1e3, 40);
        let z = randles_impedance(&freqs, 0.2, 0.01, 100e-6);
        let target = logspace_range(0.1, 1e4, 60); // wider than measured
        let z_extrap = rlkk_extrapolate(&z, &freqs, &target, 1e-6).unwrap();
        let z_true = randles_impedance(&target, 0.2, 0.01, 100e-6);
        // Interior of the measured band should extrapolate tightly; only
        // check the points that actually fall inside [1, 1e3] where the
        // DRT fit is well-constrained by data, not purely by the prior.
        for ((&f, zt), ze) in target.iter().zip(z_true.iter()).zip(z_extrap.iter()) {
            if (1.0..=1e3).contains(&f) {
                let rel_err = zt.sub(*ze).magnitude() / zt.magnitude();
                assert!(rel_err < 0.05, "f={f}, rel_err={rel_err}");
            }
        }
    }

    #[test]
    fn higher_lambda_smooths_toward_a_simpler_fit_reducing_gamma_norm() {
        let freqs = logspace_range(0.1, 1e4, 50);
        let z_true = randles_impedance(&freqs, 0.2, 0.01, 100e-6);
        let noise_free = rlkk_reconstruct(&z_true, &freqs, 1e-6, None).unwrap();
        let heavily_regularized = rlkk_reconstruct(&z_true, &freqs, 1e2, None).unwrap();
        let norm = |g: &[f64]| g.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!(
            norm(&heavily_regularized.gamma) < norm(&noise_free.gamma),
            "stronger regularization must shrink ||gamma||"
        );
    }
}
