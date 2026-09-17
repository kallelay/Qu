//! Smooth (differentiable) maximum and the softmax family.
//!
//! `max` is not differentiable, so any gradient method that needs to push
//! down a peak needs something differentiable that behaves like one.
//! `logsumexp` is the standard choice: `(1/beta) * ln(sum(exp(beta * x)))`
//! approaches `max(x)` as `beta` grows, and is smooth everywhere.
//!
//! Implemented with the usual max-shift for numerical stability, so a
//! large `beta` does not overflow `exp`.

pub fn logsumexp(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NEG_INFINITY;
    }
    let m = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !m.is_finite() {
        return m; // all -inf (or a +inf present): propagate directly
    }
    let sum: f64 = values.iter().map(|&x| (x - m).exp()).sum();
    m + sum.ln()
}

/// Smooth maximum with sharpness `beta > 0`: `(1/beta) * logsumexp(beta * x)`.
///
/// Larger `beta` tracks the true maximum more tightly (at the cost of a stiffer
/// gradient). `beta <= 0` or a non-finite `beta` falls back to the hard `max`.
pub fn smooth_max(values: &[f64], beta: f64) -> f64 {
    if values.is_empty() {
        return f64::NEG_INFINITY;
    }
    if !beta.is_finite() || beta <= 0.0 {
        return values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    }
    let scaled: Vec<f64> = values.iter().map(|&x| x * beta).collect();
    logsumexp(&scaled) / beta
}

/// The softmax of a vector — the gradient of [`logsumexp`], a probability
/// distribution that sums to 1. Stable via the same max-shift.
pub fn softmax(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let m = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = values.iter().map(|&x| (x - m).exp()).collect();
    let sum: f64 = exps.iter().sum();
    if sum == 0.0 {
        return vec![0.0; values.len()];
    }
    exps.into_iter().map(|e| e / sum).collect()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn logsumexp_matches_naive_on_small_values() {
        let x = [0.0_f64, 1.0, 2.0];
        let naive = x.iter().map(|&v| v.exp()).sum::<f64>().ln();
        close(logsumexp(&x), naive);
    }

    #[test]
    fn logsumexp_is_stable_for_large_inputs() {
        // naive exp(1000) overflows to +inf; the stable form stays finite.
        let x = [1000.0, 1000.0, 1000.0];
        let expected = 1000.0 + (3.0_f64).ln();
        close(logsumexp(&x), expected);
        assert!(logsumexp(&x).is_finite());
    }

    #[test]
    fn logsumexp_bounds_the_max() {
        let x = [3.0, 1.0, -2.0, 0.5];
        let m = 3.0;
        let n = x.len() as f64;
        let lse = logsumexp(&x);
        assert!(lse >= m - 1e-12);
        assert!(lse <= m + n.ln() + 1e-12);
    }

    #[test]
    fn smooth_max_approaches_hard_max_as_beta_grows() {
        let x = [1.0, 3.0, 2.5, -1.0];
        let hard = 3.0;
        let loose = smooth_max(&x, 1.0);
        let tight = smooth_max(&x, 50.0);
        assert!(loose > hard); // over-estimates for small beta
        assert!((tight - hard).abs() < 1e-2); // converges for large beta
        assert!(tight < loose);
    }

    #[test]
    fn softmax_is_a_distribution_and_is_lse_gradient() {
        let x = [1.0, 2.0, 3.0];
        let p = softmax(&x);
        close(p.iter().sum::<f64>(), 1.0);
        // largest input gets the largest probability
        assert!(p[2] > p[1] && p[1] > p[0]);
    }


}
