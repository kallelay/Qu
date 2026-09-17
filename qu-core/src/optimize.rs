//! General-purpose numerical optimization: root-finding (Brent's method,
//! Newton's method), nonlinear least-squares curve fitting
//! (Levenberg-Marquardt), a quasi-Newton minimizer (L-BFGS), and basin
//! hopping (global optimization via random restarts of a local
//! minimizer).
//!
//! Every function here is generic over a plain closure for the
//! objective/residual function, so this module has no idea how that
//! function is actually evaluated — `qu-interp` supplies closures that
//! call back into a user-defined Qu function, the same "function by
//! name" bridge `pmap`/`spawn` already use elsewhere. Gradients and
//! Jacobians are always computed by finite differences internally
//! (central differences) since Qu has no automatic differentiation —
//! a real, documented limitation, not a hidden approximation.

use crate::linalg::{self, LinalgError};
use crate::matrix::Matrix;
use crate::NumericError;

/// Brent's method: combines bisection, the secant method, and inverse
/// quadratic interpolation, guaranteeing convergence (unlike Newton's
/// method, which can diverge) while converging faster than plain
/// bisection once it's close to the root. Requires `f(a)` and `f(b)` to
/// have opposite signs (a bracketed root) — the standard robust-default
/// choice for 1-D root finding, "something better than plain bisection".
pub fn brent_root(mut f: impl FnMut(f64) -> f64, a: f64, b: f64, tol: f64, max_iter: usize) -> Result<f64, NumericError> {
    let (mut a, mut b) = (a, b);
    let (mut fa, mut fb) = (f(a), f(b));
    if fa == 0.0 {
        return Ok(a);
    }
    if fb == 0.0 {
        return Ok(b);
    }
    if fa.signum() == fb.signum() {
        return Err(NumericError::EmptyInput("brent_root: f(a) and f(b) must have opposite signs"));
    }
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a;
    let mut fc = fa;
    let mut mflag = true;
    let mut d = a;
    for _ in 0..max_iter {
        if fb == 0.0 || (b - a).abs() < tol {
            return Ok(b);
        }
        let mut s = if fa != fc && fb != fc {
            // inverse quadratic interpolation
            a * fb * fc / ((fa - fb) * (fa - fc)) + b * fa * fc / ((fb - fa) * (fb - fc)) + c * fa * fb / ((fc - fa) * (fc - fb))
        } else {
            // secant method
            b - fb * (b - a) / (fb - fa)
        };
        let cond1 = (s - b) * (s - (3.0 * a + b) / 4.0) > 0.0;
        let cond2 = mflag && (s - b).abs() >= (b - c).abs() / 2.0;
        let cond3 = !mflag && (s - b).abs() >= (c - d).abs() / 2.0;
        let cond4 = mflag && (b - c).abs() < tol;
        let cond5 = !mflag && (c - d).abs() < tol;
        if cond1 || cond2 || cond3 || cond4 || cond5 {
            s = (a + b) / 2.0; // bisection fallback
            mflag = true;
        } else {
            mflag = false;
        }
        let fs = f(s);
        d = c;
        c = b;
        fc = fb;
        if fa.signum() != fs.signum() {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    let _ = d;
    Ok(b)
}

/// Newton's method: `x_{n+1} = x_n - f(x_n)/f'(x_n)`. Fast (quadratic
/// convergence) when it works, but can diverge or cycle from a poor
/// starting point or near a flat derivative — unlike [`brent_root`], it
/// needs no bracket, only a starting guess and a derivative.
pub fn newton_root(mut f: impl FnMut(f64) -> f64, mut fprime: impl FnMut(f64) -> f64, x0: f64, tol: f64, max_iter: usize) -> Result<f64, NumericError> {
    let mut x = x0;
    for _ in 0..max_iter {
        let fx = f(x);
        if fx.abs() < tol {
            return Ok(x);
        }
        let dfx = fprime(x);
        if dfx.abs() < 1e-300 {
            return Err(NumericError::EmptyInput("newton_root: derivative vanished"));
        }
        let x_new = x - fx / dfx;
        if (x_new - x).abs() < tol {
            return Ok(x_new);
        }
        x = x_new;
    }
    Ok(x)
}

fn central_diff_gradient(f: &mut impl FnMut(&[f64]) -> f64, x: &[f64], h: f64) -> Vec<f64> {
    let n = x.len();
    let mut grad = vec![0.0; n];
    let mut xp = x.to_vec();
    let mut xm = x.to_vec();
    for i in 0..n {
        let step = h * x[i].abs().max(1.0);
        xp[i] = x[i] + step;
        xm[i] = x[i] - step;
        grad[i] = (f(&xp) - f(&xm)) / (2.0 * step);
        xp[i] = x[i];
        xm[i] = x[i];
    }
    grad
}

/// Numerical Jacobian of a vector-valued residual function: row `i`,
/// column `j` is `d(residuals[i])/d(params[j])`, via forward differences
/// (cheaper than central differences — `n+1` evaluations instead of
/// `2n` — which [`levenberg_marquardt`] calls every iteration).
fn forward_diff_jacobian(f: &mut impl FnMut(&[f64]) -> Vec<f64>, x: &[f64], f0: &[f64], h: f64) -> Matrix {
    let n = x.len();
    let m = f0.len();
    let mut jac = Matrix::zeros(m, n);
    let mut xp = x.to_vec();
    for j in 0..n {
        let step = h * x[j].abs().max(1.0);
        xp[j] = x[j] + step;
        let fp = f(&xp);
        for i in 0..m {
            let _ = jac.set(i, j, (fp[i] - f0[i]) / step);
        }
        xp[j] = x[j];
    }
    jac
}

pub struct LmResult {
    pub params: Vec<f64>,
    /// `sum(residuals^2)` at the returned parameters.
    pub cost: f64,
    pub iterations: usize,
    pub converged: bool,
}

/// Levenberg-Marquardt: nonlinear least squares, minimizing
/// `sum(residuals(params)^2)`. Damped Gauss-Newton — at each step, solves
/// `(J^T J + lambda*diag(J^T J)) * delta = J^T * r` for the parameter
/// update `delta`, growing `lambda` (falling back toward gradient
/// descent) when a step doesn't reduce the cost, shrinking it (moving
/// toward the fast but less stable pure Gauss-Newton step) when it does
/// — the standard adaptive damping strategy. `residuals` is evaluated
/// through a numerical (forward-difference) Jacobian, since Qu has no
/// automatic differentiation.
pub fn levenberg_marquardt(
    mut residuals: impl FnMut(&[f64]) -> Vec<f64>,
    x0: &[f64],
    max_iter: usize,
    tol: f64,
) -> Result<LmResult, NumericError> {
    if x0.is_empty() {
        return Err(NumericError::EmptyInput("levenberg_marquardt: needs at least one parameter"));
    }
    let n = x0.len();
    let mut x = x0.to_vec();
    let mut r = residuals(&x);
    let mut cost = r.iter().map(|v| v * v).sum::<f64>();
    let mut lambda = 1e-3;
    let mut converged = false;
    let mut iterations = 0;
    for _iter in 0..max_iter {
        iterations += 1;
        let jac = forward_diff_jacobian(&mut residuals, &x, &r, 1e-7);
        let jt = jac.transpose();
        let jtj = jt.matmul(&jac).map_err(|_| NumericError::EmptyInput("levenberg_marquardt: Jacobian shape error"))?;
        let r_mat = Matrix::from_column(&r);
        let jtr = jt.matmul(&r_mat).map_err(|_| NumericError::EmptyInput("levenberg_marquardt: Jacobian shape error"))?;

        let mut step_accepted = false;
        for _ in 0..30 {
            let mut damped = jtj.clone();
            for i in 0..n {
                let diag = damped.get(i, i).unwrap_or(1.0).max(1e-12);
                let _ = damped.set(i, i, diag * (1.0 + lambda));
            }
            let delta = match linalg::least_squares(&damped, &jtr, None) {
                Ok(d) => d,
                Err(LinalgError::Decomposition(_)) | Err(_) => {
                    lambda *= 10.0;
                    continue;
                }
            };
            // `delta` solves `(damped) * delta = J^T*r`, i.e. delta points
            // in the direction of *increasing* cost (the cost gradient is
            // `+2*J^T*r`) — the step that actually reduces the cost is
            // `x - delta`, not `x + delta`.
            let x_new: Vec<f64> = x.iter().zip(delta.as_slice()).map(|(&xi, &di)| xi - di).collect();
            let r_new = residuals(&x_new);
            let cost_new = r_new.iter().map(|v| v * v).sum::<f64>();
            if cost_new < cost {
                let step_norm: f64 = delta.as_slice().iter().map(|v| v * v).sum::<f64>().sqrt();
                x = x_new;
                let cost_improved = cost - cost_new;
                r = r_new;
                cost = cost_new;
                lambda = (lambda / 10.0).max(1e-12);
                step_accepted = true;
                if step_norm < tol || cost_improved < tol * (1.0 + cost) {
                    converged = true;
                }
                break;
            } else {
                lambda *= 10.0;
            }
        }
        if !step_accepted || converged {
            break;
        }
    }
    Ok(LmResult { params: x, cost, iterations, converged })
}

pub struct MinimizeResult {
    pub params: Vec<f64>,
    pub value: f64,
    pub iterations: usize,
    pub converged: bool,
}

/// Evaluates the gradient at `x`: the user-supplied analytical gradient if
/// one was given, otherwise a central-difference approximation. Factored out
/// so [`lbfgs`] has one place that decides "numerical or analytical" instead
/// of repeating the branch at every call site.
fn eval_gradient(f: &mut impl FnMut(&[f64]) -> f64, grad_fn: &mut Option<&mut dyn FnMut(&[f64]) -> Vec<f64>>, x: &[f64]) -> Vec<f64> {
    match grad_fn {
        Some(g) => g(x),
        None => central_diff_gradient(f, x, 1e-6),
    }
}

/// L-BFGS: a quasi-Newton ("pseudo-Newton" — it approximates the inverse
/// Hessian from recent gradient history instead of computing it outright)
/// multivariate minimizer, using the standard two-loop recursion with a
/// backtracking (Armijo) line search. `grad_fn`, if given, is called for the
/// gradient at each point instead of the default central-difference
/// approximation — an analytical gradient is both cheaper (one call instead
/// of `2n`) and exact, where finite differences are only an approximation.
pub fn lbfgs(
    mut f: impl FnMut(&[f64]) -> f64,
    x0: &[f64],
    max_iter: usize,
    tol: f64,
    mut grad_fn: Option<&mut dyn FnMut(&[f64]) -> Vec<f64>>,
) -> Result<MinimizeResult, NumericError> {
    if x0.is_empty() {
        return Err(NumericError::EmptyInput("lbfgs: needs at least one parameter"));
    }
    const MEMORY: usize = 10;
    let n = x0.len();
    let mut x = x0.to_vec();
    let mut fx = f(&x);
    let mut grad = eval_gradient(&mut f, &mut grad_fn, &x);
    let mut s_history: Vec<Vec<f64>> = Vec::new();
    let mut y_history: Vec<Vec<f64>> = Vec::new();
    let mut converged = false;
    let mut iterations = 0;

    for _iter in 0..max_iter {
        iterations += 1;
        let grad_norm: f64 = grad.iter().map(|g| g * g).sum::<f64>().sqrt();
        if grad_norm < tol {
            converged = true;
            break;
        }
        // two-loop recursion for the search direction
        let mut q = grad.clone();
        let m = s_history.len();
        let mut alpha = vec![0.0; m];
        let mut rho = vec![0.0; m];
        for i in (0..m).rev() {
            let sy: f64 = s_history[i].iter().zip(&y_history[i]).map(|(s, y)| s * y).sum();
            rho[i] = if sy.abs() > 1e-300 { 1.0 / sy } else { 0.0 };
            let sq: f64 = s_history[i].iter().zip(&q).map(|(s, qi)| s * qi).sum();
            alpha[i] = rho[i] * sq;
            for j in 0..n {
                q[j] -= alpha[i] * y_history[i][j];
            }
        }
        let gamma = if m > 0 {
            let sy: f64 = s_history[m - 1].iter().zip(&y_history[m - 1]).map(|(s, y)| s * y).sum();
            let yy: f64 = y_history[m - 1].iter().map(|y| y * y).sum();
            if yy.abs() > 1e-300 { sy / yy } else { 1.0 }
        } else {
            1.0
        };
        let mut z: Vec<f64> = q.iter().map(|qi| gamma * qi).collect();
        for i in 0..m {
            let yz: f64 = y_history[i].iter().zip(&z).map(|(y, zi)| y * zi).sum();
            let beta = rho[i] * yz;
            for j in 0..n {
                z[j] += s_history[i][j] * (alpha[i] - beta);
            }
        }
        let direction: Vec<f64> = z.iter().map(|zi| -zi).collect();

        // backtracking line search (Armijo condition)
        let directional_derivative: f64 = grad.iter().zip(&direction).map(|(g, d)| g * d).sum();
        if directional_derivative >= 0.0 {
            // not a descent direction (can happen after a bad curvature
            // update) -- fall back to plain steepest descent this step.
            let sd: Vec<f64> = grad.iter().map(|g| -g).collect();
            let mut step = 1.0 / grad_norm.max(1.0);
            let mut x_new = x.clone();
            for _ in 0..40 {
                for j in 0..n {
                    x_new[j] = x[j] + step * sd[j];
                }
                let fx_new = f(&x_new);
                if fx_new < fx {
                    break;
                }
                step *= 0.5;
            }
            let grad_new = eval_gradient(&mut f, &mut grad_fn, &x_new);
            s_history.clear();
            y_history.clear();
            x = x_new;
            fx = f(&x);
            grad = grad_new;
            continue;
        }
        let mut step = 1.0;
        let mut x_new = x.clone();
        let mut fx_new = fx;
        for _ in 0..40 {
            for j in 0..n {
                x_new[j] = x[j] + step * direction[j];
            }
            fx_new = f(&x_new);
            if fx_new <= fx + 1e-4 * step * directional_derivative {
                break;
            }
            step *= 0.5;
        }
        let grad_new = eval_gradient(&mut f, &mut grad_fn, &x_new);
        let s: Vec<f64> = x_new.iter().zip(&x).map(|(a, b)| a - b).collect();
        let y: Vec<f64> = grad_new.iter().zip(&grad).map(|(a, b)| a - b).collect();
        s_history.push(s);
        y_history.push(y);
        if s_history.len() > MEMORY {
            s_history.remove(0);
            y_history.remove(0);
        }
        x = x_new;
        fx = fx_new;
        grad = grad_new;
    }
    Ok(MinimizeResult { params: x, value: fx, iterations, converged })
}

/// Basin hopping (Wales & Doye, 1997): global optimization by repeatedly
/// perturbing the current best point with random noise and re-running a
/// local minimizer ([`lbfgs`]) from there, accepting the new local
/// minimum outright if it's better, or with Metropolis probability
/// `exp(-(new-old)/temperature)` if it's worse — letting the search
/// occasionally escape a local basin instead of getting stuck in the
/// first one found. A splitmix64-style counter-based PRNG keeps the
/// search reproducible from `seed`, matching the rest of this crate's own
/// RNG discipline. `grad_fn`, if given, is forwarded to every local
/// [`lbfgs`] search in place of its default finite-difference gradient.
pub fn basin_hopping(
    mut f: impl FnMut(&[f64]) -> f64,
    x0: &[f64],
    n_iter: usize,
    step_size: f64,
    temperature: f64,
    seed: u64,
    mut grad_fn: Option<impl FnMut(&[f64]) -> Vec<f64>>,
) -> Result<MinimizeResult, NumericError> {
    if x0.is_empty() {
        return Err(NumericError::EmptyInput("basin_hopping: needs at least one parameter"));
    }
    let mut rng_state = seed ^ 0x9E3779B97F4A7C15;
    let mut next_u64 = move || {
        rng_state = rng_state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    };
    let mut next_uniform = move || (next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);

    // `lbfgs` takes its optional gradient as a trait object so it can be
    // reborrowed fresh on every one of the many local searches below,
    // instead of being consumed by the first one.
    macro_rules! grad_ref {
        () => {
            grad_fn.as_mut().map(|g| g as &mut dyn FnMut(&[f64]) -> Vec<f64>)
        };
    }

    let n = x0.len();
    let initial = lbfgs(&mut f, x0, 200, 1e-9, grad_ref!())?;
    let mut best_x = initial.params;
    let mut best_value = initial.value;
    let mut current_x = best_x.clone();
    let mut current_value = best_value;

    for _ in 0..n_iter {
        let mut candidate = current_x.clone();
        for v in candidate.iter_mut().take(n) {
            *v += step_size * (2.0 * next_uniform() - 1.0);
        }
        let local = lbfgs(&mut f, &candidate, 200, 1e-9, grad_ref!())?;
        let accept = if local.value < current_value {
            true
        } else if temperature > 0.0 {
            let p = (-(local.value - current_value) / temperature).exp();
            next_uniform() < p
        } else {
            false
        };
        if accept {
            current_x = local.params.clone();
            current_value = local.value;
        }
        if local.value < best_value {
            best_value = local.value;
            best_x = local.params;
        }
    }
    Ok(MinimizeResult { params: best_x, value: best_value, iterations: n_iter, converged: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "{a} != {b}");
    }

    #[test]
    fn brent_root_finds_sqrt_two() {
        let root = brent_root(|x| x * x - 2.0, 0.0, 2.0, 1e-12, 100).unwrap();
        close(root, std::f64::consts::SQRT_2, 1e-9);
    }

    #[test]
    fn brent_root_rejects_a_non_bracketing_interval() {
        assert!(brent_root(|x| x * x + 1.0, 0.0, 2.0, 1e-9, 100).is_err());
    }

    #[test]
    fn newton_root_finds_a_cube_root() {
        // f(x) = x^3 - 27, root at x=3.
        let root = newton_root(|x| x.powi(3) - 27.0, |x| 3.0 * x * x, 1.0, 1e-12, 100).unwrap();
        close(root, 3.0, 1e-9);
    }

    #[test]
    fn levenberg_marquardt_fits_a_known_exponential_decay() {
        // y = a*exp(-b*x), true a=5, b=0.5, no noise -- LM should recover
        // the exact parameters from a reasonable starting guess.
        let (a_true, b_true) = (5.0, 0.5);
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.2).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| a_true * (-b_true * x).exp()).collect();
        let residuals = move |p: &[f64]| -> Vec<f64> {
            xs.iter().zip(&ys).map(|(&x, &y)| p[0] * (-p[1] * x).exp() - y).collect()
        };
        let result = levenberg_marquardt(residuals, &[1.0, 1.0], 200, 1e-12).unwrap();
        close(result.params[0], a_true, 1e-4);
        close(result.params[1], b_true, 1e-4);
        assert!(result.cost < 1e-8, "cost = {}", result.cost);
    }

    #[test]
    fn levenberg_marquardt_fits_noisy_data_closely() {
        let (a_true, b_true, c_true) = (2.0, 1.5, 0.3);
        let xs: Vec<f64> = (0..30).map(|i| i as f64 * 0.1).collect();
        // deterministic "noise" (no RNG dependency in this test) via a
        // fixed small perturbation pattern.
        let ys: Vec<f64> = xs
            .iter()
            .enumerate()
            .map(|(i, &x)| a_true * (b_true * x).sin() + c_true + 0.01 * (i as f64 % 3.0 - 1.0))
            .collect();
        let residuals = move |p: &[f64]| -> Vec<f64> {
            xs.iter().zip(&ys).map(|(&x, &y)| p[0] * (p[1] * x).sin() + p[2] - y).collect()
        };
        let result = levenberg_marquardt(residuals, &[1.0, 1.0, 0.0], 200, 1e-10).unwrap();
        close(result.params[0], a_true, 0.05);
        close(result.params[1], b_true, 0.05);
        close(result.params[2], c_true, 0.05);
    }

    #[test]
    fn lbfgs_minimizes_a_simple_quadratic_bowl() {
        // f(x,y) = (x-3)^2 + (y+2)^2, minimum at (3,-2), value 0.
        let f = |p: &[f64]| (p[0] - 3.0).powi(2) + (p[1] + 2.0).powi(2);
        let result = lbfgs(f, &[0.0, 0.0], 200, 1e-10, None).unwrap();
        close(result.params[0], 3.0, 1e-3);
        close(result.params[1], -2.0, 1e-3);
        assert!(result.value < 1e-6, "value = {}", result.value);
    }

    #[test]
    fn lbfgs_minimizes_the_rosenbrock_function() {
        // The classic hard-to-optimize test function; global minimum at
        // (1,1), value 0 -- a real test of the quasi-Newton machinery,
        // not just a bowl any descent method would solve trivially.
        let f = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let result = lbfgs(f, &[-1.2, 1.0], 2000, 1e-10, None).unwrap();
        close(result.params[0], 1.0, 1e-2);
        close(result.params[1], 1.0, 1e-2);
    }

    #[test]
    fn lbfgs_matches_finite_differences_when_given_the_exact_analytical_gradient() {
        // Same Rosenbrock problem, but with an analytical gradient supplied
        // -- should converge to the same minimum (and, since the gradient is
        // now exact rather than approximated, at least as accurately).
        let f = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let mut grad = |p: &[f64]| -> Vec<f64> {
            vec![
                -2.0 * (1.0 - p[0]) - 400.0 * p[0] * (p[1] - p[0] * p[0]),
                200.0 * (p[1] - p[0] * p[0]),
            ]
        };
        let result = lbfgs(f, &[-1.2, 1.0], 2000, 1e-10, Some(&mut grad)).unwrap();
        close(result.params[0], 1.0, 1e-2);
        close(result.params[1], 1.0, 1e-2);
    }

    #[test]
    fn basin_hopping_escapes_a_local_minimum_a_pure_local_search_would_not() {
        // f has a shallow local minimum at x=-2 (value -0.5) and a deeper
        // global minimum at x=3 (value -2.0); starting exactly at the
        // shallow one, a pure local minimizer (lbfgs alone) stays stuck
        // there, but basin hopping's random restarts should find the
        // better one.
        let f = |p: &[f64]| {
            let x = p[0];
            -0.5 * (-(x + 2.0).powi(2)).exp() - 2.0 * (-(x - 3.0).powi(2) / 2.0).exp()
        };
        let stuck = lbfgs(f, &[-2.0], 200, 1e-9, None).unwrap();
        close(stuck.params[0], -2.0, 0.1); // confirms it really is stuck locally

        let hopped = basin_hopping(f, &[-2.0], 60, 3.0, 0.5, 42, None::<fn(&[f64]) -> Vec<f64>>).unwrap();
        assert!(hopped.value < stuck.value - 0.5, "basin hopping should beat the stuck local search: {} vs {}", hopped.value, stuck.value);
        close(hopped.params[0], 3.0, 0.2);
    }

    #[test]
    fn basin_hopping_is_deterministic_for_a_fixed_seed() {
        let f = |p: &[f64]| (p[0] - 1.0).powi(2) + (p[1] - 2.0).powi(2);
        let a = basin_hopping(f, &[0.0, 0.0], 20, 1.0, 0.5, 7, None::<fn(&[f64]) -> Vec<f64>>).unwrap();
        let b = basin_hopping(f, &[0.0, 0.0], 20, 1.0, 0.5, 7, None::<fn(&[f64]) -> Vec<f64>>).unwrap();
        assert_eq!(a.params, b.params);
    }

    #[test]
    fn basin_hopping_accepts_an_analytical_gradient_for_its_local_searches() {
        let f = |p: &[f64]| (p[0] - 1.0).powi(2) + (p[1] - 2.0).powi(2);
        let mut grad = |p: &[f64]| vec![2.0 * (p[0] - 1.0), 2.0 * (p[1] - 2.0)];
        let result = basin_hopping(f, &[0.0, 0.0], 20, 1.0, 0.5, 7, Some(&mut grad)).unwrap();
        close(result.params[0], 1.0, 1e-3);
        close(result.params[1], 2.0, 1e-3);
    }
}
