//! State estimation: the parts of the Kalman-filter family and the
//! particle filter that are pure numerics, with no idea how a process or
//! observation model is actually evaluated — that's problem-specific (an
//! arbitrary nonlinear function, in general) and lives in `qu-interp` as
//! calls back into user-defined Qu functions, the same "function by name"
//! boundary `qu_core::optimize` already crosses.
//!
//! [`kalman_correct`] and [`propagate_covariance`] are the one measurement-
//! update and one time-update formula shared, in substance, by the linear
//! Kalman filter, the extended Kalman filter, and the unscented Kalman
//! filter — the three variants differ only in *how* they arrive at a
//! Jacobian/cross-covariance and predicted state, not in what they do with
//! them once they have them. [`sigma_points`]/[`unscented_transform`]/
//! [`cross_covariance`] are the UKF's own machinery for getting there
//! without a Jacobian at all. [`systematic_resample`]/
//! [`effective_sample_size`]/[`weighted_mean`] are the particle filter's.

use crate::matrix::Matrix;
use crate::NumericError;

/// The Kalman-family measurement-update step, shared by the linear,
/// extended, and unscented Kalman filters: given the Kalman gain's two
/// ingredients — the state/measurement cross-covariance `cross_cov`
/// (`P*H^T` for the linear/extended filter, or a sigma-point
/// [`cross_covariance`] for the unscented one) and the innovation
/// covariance `s` (`H*P*H^T + R`, or its unscented-transform equivalent) —
/// computes the gain `K = cross_cov * S^-1` (via [`crate::linalg::pseudo_inverse`],
/// robust for the small measurement-dimension `S` this is always solving),
/// then `x' = x + K*innovation`, `P' = P - K*S*K^T`. This form (as opposed
/// to the algebraically-equivalent `P' = (I - K*H)*P`) needs no `H`
/// matrix at all, which is exactly why it's the one form all three filter
/// variants can share — the unscented filter never has an `H` to begin
/// with.
pub fn kalman_correct(x: &[f64], p: &Matrix, innovation: &[f64], cross_cov: &Matrix, s: &Matrix) -> Result<(Vec<f64>, Matrix), String> {
    let s_inv = crate::linalg::pseudo_inverse(s, None).map_err(|le| format!("kalman_correct: {le}"))?.matrix;
    let gain = cross_cov.matmul(&s_inv).map_err(|se| format!("kalman_correct: {se}"))?;
    let correction = gain.matmul(&Matrix::from_column(innovation)).map_err(|se| format!("kalman_correct: {se}"))?;
    let x_new: Vec<f64> = x.iter().zip(correction.as_slice()).map(|(a, b)| a + b).collect();
    let ks = gain.matmul(s).map_err(|se| format!("kalman_correct: {se}"))?;
    let ksk_t = ks.matmul(&gain.transpose()).map_err(|se| format!("kalman_correct: {se}"))?;
    let p_new = p.broadcast(&ksk_t, |a, b| a - b).map_err(|se| format!("kalman_correct: {se}"))?;
    Ok((x_new, p_new))
}

/// The Kalman-family time-update covariance formula, shared by the linear
/// and extended Kalman filters: `P' = F*P*F^T + Q`, where `F` is either
/// the (constant) state-transition matrix or the process Jacobian
/// evaluated at the current estimate. The unscented filter doesn't use
/// this — its own predict step goes through [`sigma_points`]/
/// [`unscented_transform`] instead, since it never linearizes anything.
pub fn propagate_covariance(f_jac: &Matrix, p: &Matrix, q: &Matrix) -> Result<Matrix, String> {
    let fp = f_jac.matmul(p).map_err(|se| format!("propagate_covariance: {se}"))?;
    let fpf_t = fp.matmul(&f_jac.transpose()).map_err(|se| format!("propagate_covariance: {se}"))?;
    fpf_t.broadcast(q, |a, b| a + b).map_err(|se| format!("propagate_covariance: {se}"))
}

/// The standard scaled (Van der Merwe) sigma-point set for an
/// `n`-dimensional Gaussian (mean `x`, covariance `p`): `2n+1` points —
/// `x` itself, plus `x +/- ` each column of a matrix square root of
/// `(n+lambda)*P` (via [`crate::linalg::cholesky`]) — along with the mean
/// weights `Wm` and covariance weights `Wc` used to recombine them after
/// propagation through a nonlinear function ([`unscented_transform`]).
/// `lambda = alpha^2*(n+kappa) - n`; the conventional defaults are
/// `alpha=1e-3` (small and positive — how spread out the points are),
/// `beta=2.0` (optimal for Gaussian-distributed `x`), `kappa=0.0`.
/// Verified against an independent Python/numpy implementation before
/// this was written: for a *linear* function, propagating these points
/// and recombining with [`unscented_transform`] reproduces `F*x`/`F*P*F^T`
/// exactly, the standard sanity check for a sigma-point set.
pub fn sigma_points(x: &[f64], p: &Matrix, alpha: f64, beta: f64, kappa: f64) -> Result<(Matrix, Vec<f64>, Vec<f64>), String> {
    let n = x.len();
    if n == 0 {
        return Err("sigma_points: needs at least one state dimension".to_string());
    }
    if p.rows() != n || p.cols() != n {
        return Err(format!("sigma_points: state has {n} dimension(s) but P is {}x{}", p.rows(), p.cols()));
    }
    let lambda = alpha * alpha * (n as f64 + kappa) - n as f64;
    let scale = alpha * alpha * (n as f64 + kappa); // == n + lambda, without the cancellation
    if scale <= 0.0 {
        return Err(format!("sigma_points: alpha/kappa give a non-positive scale ({scale}); need alpha^2*(n+kappa) > 0"));
    }
    let scaled_p = p.map(|v| v * scale);
    let sqrt_p = crate::linalg::cholesky(&scaled_p).map_err(|le| format!("sigma_points: {le}"))?.l;

    let n_points = 2 * n + 1;
    let mut points = Matrix::zeros(n_points, n);
    for c in 0..n {
        let _ = points.set(0, c, x[c]);
    }
    for i in 0..n {
        let col = sqrt_p.col_vec(i).expect("index within bounds by construction");
        for c in 0..n {
            let _ = points.set(1 + i, c, x[c] + col[c]);
            let _ = points.set(1 + n + i, c, x[c] - col[c]);
        }
    }

    let mut wm = vec![1.0 / (2.0 * scale); n_points];
    let mut wc = wm.clone();
    wm[0] = lambda / scale;
    wc[0] = lambda / scale + (1.0 - alpha * alpha + beta);
    Ok((points, wm, wc))
}

/// Recombines a set of (already-propagated) sigma points into a weighted
/// mean and covariance — the "transform" half of the unscented transform,
/// used identically whether propagating through a process model (for the
/// predicted state) or an observation model (for the predicted
/// measurement). `points` is `(2n+1) x d`, one point per row (the layout
/// [`sigma_points`] produces) — `d` need not equal the state dimension `n`
/// the weights were generated from, since an observation model can have a
/// different output dimension.
pub fn unscented_transform(points: &Matrix, wm: &[f64], wc: &[f64]) -> (Vec<f64>, Matrix) {
    let n_points = points.rows();
    let d = points.cols();
    let mut mean = vec![0.0; d];
    for r in 0..n_points {
        let w = wm[r];
        for (c, m) in mean.iter_mut().enumerate() {
            *m += w * points.get(r, c).expect("index within bounds by construction");
        }
    }
    let mut cov = Matrix::zeros(d, d);
    for r in 0..n_points {
        let w = wc[r];
        for i in 0..d {
            let di = points.get(r, i).expect("index within bounds by construction") - mean[i];
            for j in 0..d {
                let dj = points.get(r, j).expect("index within bounds by construction") - mean[j];
                let cur = cov.get(i, j).expect("index within bounds by construction");
                let _ = cov.set(i, j, cur + w * di * dj);
            }
        }
    }
    (mean, cov)
}

/// Weighted cross-covariance between two sigma-point sets propagated from
/// the *same* base points (e.g. a UKF's state sigma points and their
/// corresponding observation sigma points) — the piece
/// [`unscented_transform`] alone doesn't give, needed for the unscented
/// filter's measurement-update Kalman gain via [`kalman_correct`].
pub fn cross_covariance(a_points: &Matrix, a_mean: &[f64], b_points: &Matrix, b_mean: &[f64], wc: &[f64]) -> Matrix {
    let n_points = a_points.rows();
    let da = a_points.cols();
    let db = b_points.cols();
    let mut cov = Matrix::zeros(da, db);
    for r in 0..n_points {
        let w = wc[r];
        for i in 0..da {
            let di = a_points.get(r, i).expect("index within bounds by construction") - a_mean[i];
            for j in 0..db {
                let dj = b_points.get(r, j).expect("index within bounds by construction") - b_mean[j];
                let cur = cov.get(i, j).expect("index within bounds by construction");
                let _ = cov.set(i, j, cur + w * di * dj);
            }
        }
    }
    cov
}

/// Effective sample size: `1 / sum(w_i^2)` for *normalized* weights
/// (`sum(w_i) == 1`). Ranges from 1 (all weight on a single particle — the
/// filter has degenerated to one hypothesis) to `n` (perfectly uniform
/// weights — full particle diversity). The standard diagnostic for
/// deciding when to resample.
pub fn effective_sample_size(weights: &[f64]) -> f64 {
    let sum_sq: f64 = weights.iter().map(|w| w * w).sum();
    if sum_sq > 0.0 {
        1.0 / sum_sq
    } else {
        0.0
    }
}

/// Systematic resampling (Kitagawa, 1996): draws `n` new particle indices
/// from the categorical distribution defined by `weights`, using a single
/// random offset and `n` evenly-spaced strata rather than `n` independent
/// draws — lower variance than naive multinomial resampling for the same
/// particle count, the standard choice in practice. `particles` is `n x d`
/// (one particle per row); returns the resampled `n x d` particle matrix
/// alongside the corresponding uniform weights (`1/n` each) — resampling
/// always resets weights, since it's specifically the step that converts
/// "few particles carrying most of the weight" back into "many equally
/// likely particles".
pub fn systematic_resample(particles: &Matrix, weights: &[f64], seed: u64) -> Result<(Matrix, Vec<f64>), NumericError> {
    let n = weights.len();
    if n == 0 {
        return Err(NumericError::EmptyInput("systematic_resample: needs at least one particle"));
    }
    if particles.rows() != n {
        return Err(NumericError::EmptyInput("systematic_resample: particle count doesn't match weight count"));
    }
    // splitmix64-style counter PRNG, matching qu_core::optimize::basin_hopping's
    // own RNG discipline (reproducible from a seed, no external crate).
    let mut rng_state = seed ^ 0x9E3779B97F4A7C15;
    let mut next_u64 = move || {
        rng_state = rng_state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    };
    let u0 = (next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64) / n as f64;

    let mut cumulative = Vec::with_capacity(n);
    let mut running = 0.0;
    for &w in weights {
        running += w;
        cumulative.push(running);
    }
    let total = running;
    if total <= 0.0 {
        return Err(NumericError::EmptyInput("systematic_resample: weights must sum to a positive total"));
    }

    let d = particles.cols();
    let mut resampled = Matrix::zeros(n, d);
    let mut j = 0;
    for i in 0..n {
        let target = (u0 + i as f64 / n as f64) * total;
        while j < n - 1 && cumulative[j] < target {
            j += 1;
        }
        for c in 0..d {
            let _ = resampled.set(i, c, particles.get(j, c).unwrap_or(0.0));
        }
    }
    Ok((resampled, vec![1.0 / n as f64; n]))
}

/// Weighted mean of the particle set (`sum(w_i * particle_i)`, for
/// normalized weights) — the standard point estimate a particle filter
/// reports at each step.
pub fn weighted_mean(particles: &Matrix, weights: &[f64]) -> Vec<f64> {
    let d = particles.cols();
    let mut mean = vec![0.0; d];
    for r in 0..particles.rows() {
        let w = weights.get(r).copied().unwrap_or(0.0);
        for (c, m) in mean.iter_mut().enumerate() {
            *m += w * particles.get(r, c).unwrap_or(0.0);
        }
    }
    mean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalman_correct_matches_a_hand_computed_1d_example() {
        // Same worked example as the linear Kalman filter's own test:
        // x=0, P=1, H=1, R=1, z=2 -> cross_cov=P*H^T=1, S=H*P*H^T+R=2,
        // innovation=z-H*x=2 -> x'=0+0.5*2=1, P'=1-0.5*2*0.5=0.5.
        let x = [0.0];
        let p = Matrix::from_rows(&[vec![1.0]]).unwrap();
        let cross_cov = Matrix::from_rows(&[vec![1.0]]).unwrap();
        let s = Matrix::from_rows(&[vec![2.0]]).unwrap();
        let (x_new, p_new) = kalman_correct(&x, &p, &[2.0], &cross_cov, &s).unwrap();
        assert!((x_new[0] - 1.0).abs() < 1e-12);
        assert!((p_new.get(0, 0).unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn propagate_covariance_matches_f_p_f_t_plus_q() {
        let f = Matrix::from_rows(&[vec![2.0, 0.0], vec![0.0, 3.0]]).unwrap();
        let p = Matrix::from_rows(&[vec![1.0, 0.0], vec![0.0, 1.0]]).unwrap();
        let q = Matrix::from_rows(&[vec![0.1, 0.0], vec![0.0, 0.1]]).unwrap();
        let result = propagate_covariance(&f, &p, &q).unwrap();
        // F*P*F^T = diag(4, 9); + Q = diag(4.1, 9.1).
        assert!((result.get(0, 0).unwrap() - 4.1).abs() < 1e-9);
        assert!((result.get(1, 1).unwrap() - 9.1).abs() < 1e-9);
        assert!((result.get(0, 1).unwrap()).abs() < 1e-9);
    }

    #[test]
    fn sigma_points_and_unscented_transform_reproduce_a_linear_transform_exactly() {
        // The standard UKF sanity check, verified independently in Python
        // (numpy) first: for a *linear* function f(x) = F*x, propagating
        // the sigma points and recombining must exactly reproduce F*x and
        // F*P*F^T -- the property that makes sigma points a valid
        // generalization of the linear Kalman filter's own math.
        let x = [0.3, -1.2, 0.7];
        let p = Matrix::from_rows(&[
            vec![1.2, 0.1, 0.0],
            vec![0.1, 0.8, -0.05],
            vec![0.0, -0.05, 0.6],
        ])
        .unwrap();
        let f = Matrix::from_rows(&[
            vec![1.0, 0.5, 0.0],
            vec![-0.3, 1.0, 0.2],
            vec![0.1, 0.0, 0.9],
        ])
        .unwrap();
        let (points, wm, wc) = sigma_points(&x, &p, 1e-3, 2.0, 0.0).unwrap();
        assert_eq!(points.rows(), 2 * x.len() + 1);

        let mut propagated = Matrix::zeros(points.rows(), x.len());
        for r in 0..points.rows() {
            let row = points.row_vec(r).unwrap();
            let fx = f.matmul(&Matrix::from_column(&row)).unwrap();
            for c in 0..x.len() {
                let _ = propagated.set(r, c, fx.get(c, 0).unwrap());
            }
        }
        let (mean, cov) = unscented_transform(&propagated, &wm, &wc);

        let expected_mean = f.matmul(&Matrix::from_column(&x)).unwrap();
        for i in 0..x.len() {
            assert!((mean[i] - expected_mean.get(i, 0).unwrap()).abs() < 1e-6, "mean[{i}] = {}", mean[i]);
        }
        let expected_cov = f.matmul(&p).unwrap().matmul(&f.transpose()).unwrap();
        for i in 0..x.len() {
            for j in 0..x.len() {
                assert!(
                    (cov.get(i, j).unwrap() - expected_cov.get(i, j).unwrap()).abs() < 1e-6,
                    "cov[{i},{j}] = {} vs expected {}",
                    cov.get(i, j).unwrap(),
                    expected_cov.get(i, j).unwrap()
                );
            }
        }
    }

    #[test]
    fn sigma_points_rejects_a_covariance_shape_mismatch() {
        let p = Matrix::from_rows(&[vec![1.0, 0.0], vec![0.0, 1.0]]).unwrap();
        assert!(sigma_points(&[0.0], &p, 1e-3, 2.0, 0.0).is_err());
    }

    #[test]
    fn cross_covariance_of_a_point_set_with_itself_matches_its_own_variance() {
        let x = [1.0, -0.5];
        let p = Matrix::from_rows(&[vec![2.0, 0.3], vec![0.3, 1.5]]).unwrap();
        let (points, wm, wc) = sigma_points(&x, &p, 1e-3, 2.0, 0.0).unwrap();
        let (mean, cov) = unscented_transform(&points, &wm, &wc);
        let cross = cross_covariance(&points, &mean, &points, &mean, &wc);
        for i in 0..2 {
            for j in 0..2 {
                assert!((cross.get(i, j).unwrap() - cov.get(i, j).unwrap()).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn effective_sample_size_is_n_for_uniform_weights() {
        let w = vec![0.25; 4];
        assert!((effective_sample_size(&w) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn effective_sample_size_is_one_when_all_weight_is_on_one_particle() {
        let w = vec![1.0, 0.0, 0.0, 0.0];
        assert!((effective_sample_size(&w) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn systematic_resample_returns_uniform_weights() {
        let particles = Matrix::from_rows(&[vec![1.0], vec![2.0], vec![3.0]]).unwrap();
        let weights = vec![0.2, 0.3, 0.5];
        let (_, new_weights) = systematic_resample(&particles, &weights, 7).unwrap();
        for w in new_weights {
            assert!((w - 1.0 / 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn systematic_resample_draws_particles_roughly_proportional_to_weight() {
        // Particle 2 (value 3.0) carries 80% of the weight -- over many
        // resampled particles it should dominate the output roughly 4:1
        // over the other two combined, not appear only rarely or not at
        // all (the failure mode a buggy cumulative-sum walk produces).
        let particles = Matrix::from_rows(&[vec![1.0], vec![2.0], vec![3.0]]).unwrap();
        let weights = vec![0.1, 0.1, 0.8];
        let (resampled, _) = systematic_resample(&particles, &weights, 42).unwrap();
        let count_dominant = (0..resampled.rows()).filter(|&r| (resampled.get(r, 0).unwrap() - 3.0).abs() < 1e-9).count();
        assert!(count_dominant >= 2, "expected the 80%-weight particle to dominate, got {count_dominant}/3");
    }

    #[test]
    fn systematic_resample_is_deterministic_for_a_fixed_seed() {
        let particles = Matrix::from_rows(&[vec![1.0], vec![2.0], vec![3.0], vec![4.0]]).unwrap();
        let weights = vec![0.25; 4];
        let (a, _) = systematic_resample(&particles, &weights, 99).unwrap();
        let (b, _) = systematic_resample(&particles, &weights, 99).unwrap();
        assert_eq!(a.as_slice(), b.as_slice());
    }

    #[test]
    fn weighted_mean_matches_a_hand_computed_value() {
        // particles at 0 and 10, weights 0.9/0.1 -> mean should sit near 1,
        // not the unweighted midpoint 5 (the bug a plain average would give).
        let particles = Matrix::from_rows(&[vec![0.0], vec![10.0]]).unwrap();
        let weights = vec![0.9, 0.1];
        let mean = weighted_mean(&particles, &weights);
        assert!((mean[0] - 1.0).abs() < 1e-9, "mean = {}", mean[0]);
    }
}
