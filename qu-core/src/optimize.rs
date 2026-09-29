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

/// Clamp `x` into the box `[lower, upper]` (either side optional, and
/// either vector may be shorter than `x` -- missing entries are unbounded).
fn project_into_box(x: &mut [f64], lower: Option<&[f64]>, upper: Option<&[f64]>) {
    for (j, v) in x.iter_mut().enumerate() {
        if let Some(lo) = lower.and_then(|l| l.get(j)) {
            if *v < *lo {
                *v = *lo;
            }
        }
        if let Some(hi) = upper.and_then(|u| u.get(j)) {
            if *v > *hi {
                *v = *hi;
            }
        }
    }
}

fn sum_sq(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// Finite-difference step for a parameter currently at `v`: relative to the
/// parameter's own size (an ohm and a farad in one vector cannot share an
/// absolute step), falling back to `scale` itself at exactly zero.
fn fd_step(v: f64, scale: f64) -> f64 {
    let h = scale * v.abs();
    if h > 0.0 && h.is_finite() {
        h
    } else {
        scale
    }
}

/// `cbrt(eps)`: the step that balances truncation against rounding error
/// for a central difference, leaving ~`eps^(2/3)` (~1e-11) relative error.
const CENTRAL_STEP: f64 = 6.055_454_452_393_343e-6;
/// `sqrt(eps)`, the same balance for a forward difference (~1e-8 error).
const FORWARD_STEP: f64 = 1.490_116_119_384_765_6e-8;

/// Numerical Jacobian of `residuals` at `x` (where `r0 = residuals(x)`),
/// never evaluated outside the box: central differences when `central`
/// and both neighbours are inside the box, otherwise a one-sided
/// difference pointing INTO the box. The step actually taken
/// (`(x + h) - x`, after rounding) is the one divided by.
fn box_jacobian(
    residuals: &mut dyn FnMut(&[f64]) -> Vec<f64>,
    x: &[f64],
    r0: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    central: bool,
) -> Result<Matrix, NumericError> {
    let n = x.len();
    let m = r0.len();
    let mut data = vec![0.0; m * n]; // column-major: column j = d r / d x_j
    let mut eval = |xp: &[f64]| -> Result<Vec<f64>, NumericError> {
        let r = residuals(xp);
        if r.len() != m {
            return Err(NumericError::ShapeMismatch { expected: m, found: r.len() });
        }
        Ok(r)
    };
    for j in 0..n {
        let h = fd_step(x[j], if central { CENTRAL_STEP } else { FORWARD_STEP });
        let lo = lower.and_then(|l| l.get(j).copied()).unwrap_or(f64::NEG_INFINITY);
        let hi = upper.and_then(|u| u.get(j).copied()).unwrap_or(f64::INFINITY);
        let fwd_ok = x[j] + h <= hi;
        let bwd_ok = x[j] - h >= lo;
        let col = &mut data[j * m..(j + 1) * m];
        let mut xp = x.to_vec();
        if central && fwd_ok && bwd_ok {
            xp[j] = x[j] + h;
            let hp = xp[j] - x[j];
            let rp = eval(&xp)?;
            xp[j] = x[j] - h;
            let hm = x[j] - xp[j];
            let rm = eval(&xp)?;
            for i in 0..m {
                col[i] = (rp[i] - rm[i]) / (hp + hm);
            }
        } else if fwd_ok {
            xp[j] = x[j] + h;
            let hp = xp[j] - x[j];
            let rp = eval(&xp)?;
            for i in 0..m {
                col[i] = (rp[i] - r0[i]) / hp;
            }
        } else if bwd_ok {
            xp[j] = x[j] - h;
            let hm = x[j] - xp[j];
            let rm = eval(&xp)?;
            for i in 0..m {
                col[i] = (r0[i] - rm[i]) / hm;
            }
        }
        // Neither: the box is narrower than the step. The column stays
        // zero -- the parameter is pinned, and the caller sees it as
        // unidentifiable rather than as a derivative invented from a
        // point outside the box.
    }
    Ok(Matrix::from_col_major(m, n, data))
}

/// `J^T r` for a column-major `(m, n)` Jacobian.
fn jt_times(jac: &Matrix, r: &[f64]) -> Vec<f64> {
    let m = jac.rows();
    let d = jac.as_slice();
    (0..jac.cols()).map(|j| d[j * m..(j + 1) * m].iter().zip(r).map(|(a, b)| a * b).sum()).collect()
}

fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Plain Gauss-Newton with step-halving line search: nonlinear least
/// squares on `sum(residuals(params)^2)`, optionally inside a box (every
/// trial point is projected into `[lower, upper]` before it is
/// evaluated, the same contract as [`linalg::nonlinear_least_squares`]).
///
/// Each iteration solves the LINEARIZED problem `min ||J d - r||` (through
/// an SVD pseudo-inverse, so a rank-deficient `J` gives the minimum-norm
/// step rather than a failure), then tries `x - t d` for `t = 1, 1/2,
/// 1/4, ...` until the cost decreases. No damping: near a solution with
/// small residuals it converges quadratically, like the Newton method it
/// approximates; far from one, or on a large-residual problem, it can
/// crawl where [`levenberg_marquardt`] would not.
///
/// Converged when an accepted step is small relative to `x`
/// (`||dx|| <= tol (1 + ||x||)`), or a FULL step (`t = 1`) improves the
/// cost by less than `tol` relative. When no fraction of the step
/// decreases the cost at all, the point is accepted as converged only if
/// the proposed step was already negligible (`<= 1e-6 (1 + ||x||)`) --
/// otherwise `converged = false`: a stalled Gauss-Newton is a stalled
/// Gauss-Newton, not a solution.
pub fn gauss_newton(
    mut residuals: impl FnMut(&[f64]) -> Vec<f64>,
    x0: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    max_iter: usize,
    tol: f64,
) -> Result<LmResult, NumericError> {
    if x0.is_empty() {
        return Err(NumericError::EmptyInput("gauss_newton: needs at least one parameter"));
    }
    let mut x = x0.to_vec();
    project_into_box(&mut x, lower, upper);
    let mut r = residuals(&x);
    if r.is_empty() {
        return Err(NumericError::EmptyInput("gauss_newton: the residual"));
    }
    let mut cost = sum_sq(&r);
    let mut converged = false;
    let mut iterations = 0;
    for _ in 0..max_iter {
        iterations += 1;
        let jac = box_jacobian(&mut residuals, &x, &r, lower, upper, false)?;
        let delta = linalg::least_squares(&jac, &Matrix::from_column(&r), Some(1e-10))
            .map_err(|e| NumericError::Decomposition(format!("gauss_newton: {e}")))?;
        let d = delta.as_slice();
        let xnorm = norm2(&x);
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..40 {
            let mut trial: Vec<f64> = x.iter().zip(d).map(|(xi, di)| xi - t * di).collect();
            project_into_box(&mut trial, lower, upper);
            let rt = residuals(&trial);
            let ct = sum_sq(&rt);
            if rt.len() == r.len() && ct.is_finite() && ct < cost {
                accepted = Some((trial, rt, ct));
                break;
            }
            t *= 0.5;
        }
        match accepted {
            None => {
                converged = norm2(d) <= 1e-6 * (1.0 + xnorm);
                break;
            }
            Some((trial, rt, ct)) => {
                let dx: Vec<f64> = trial.iter().zip(&x).map(|(a, b)| a - b).collect();
                let rel = (cost - ct) / cost.max(f64::MIN_POSITIVE);
                x = trial;
                r = rt;
                cost = ct;
                if norm2(&dx) <= tol * (1.0 + norm2(&x)) || (t == 1.0 && rel < tol) || cost == 0.0 {
                    converged = true;
                    break;
                }
            }
        }
    }
    Ok(LmResult { params: x, cost, iterations, converged })
}

/// Steepest descent on `0.5 * ||residuals(params)||^2`, with a projected
/// Armijo backtracking line search (optionally inside a box, like
/// [`gauss_newton`]). The gradient `J^T r` uses a CENTRAL-difference
/// Jacobian: the stopping rule below needs a gradient accurate near zero,
/// which forward differences cannot give on a problem with nonzero
/// residuals at the solution.
///
/// Honest about being slow. Its rate is linear with ratio
/// `~(kappa - 1)/(kappa + 1)` in the condition number of `J^T J`, so a
/// well-conditioned problem converges in tens of iterations and a curved
/// valley (Rosenbrock) takes thousands. It declares `converged` only on
/// first-order optimality, never on "the cost stopped moving much":
///
/// * every gradient component, as a cosine between that Jacobian column and
///   the residual (`|J_j . r| / (||J_j|| ||r||)`, scale-free and `0`
///   exactly at a least-squares solution), is below `max(tol, 1e-8)`; or
/// * the cost has fallen to `tol` times its starting value (a zero-residual
///   problem, where the residual direction never becomes orthogonal).
///
/// Running out of `max_iter`, or a line search that finds no decrease,
/// returns `converged = false` with the best point reached.
pub fn gradient_descent(
    mut residuals: impl FnMut(&[f64]) -> Vec<f64>,
    x0: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
    max_iter: usize,
    tol: f64,
) -> Result<LmResult, NumericError> {
    if x0.is_empty() {
        return Err(NumericError::EmptyInput("gradient_descent: needs at least one parameter"));
    }
    let mut x = x0.to_vec();
    project_into_box(&mut x, lower, upper);
    let mut r = residuals(&x);
    if r.is_empty() {
        return Err(NumericError::EmptyInput("gradient_descent: the residual"));
    }
    let mut cost = sum_sq(&r);
    let cost0 = cost;
    let gtol = tol.max(1e-8);
    let mut step: Option<f64> = None;
    let mut converged = false;
    let mut iterations = 0;
    let optimal = |jac: &Matrix, g: &[f64], r: &[f64], cost: f64, x: &[f64]| -> bool {
        if cost == 0.0 || cost <= tol * cost0 {
            return true;
        }
        let m = jac.rows();
        let rn = norm2(r);
        g.iter().enumerate().all(|(j, gj)| {
            // A component pinned at a bound, with the gradient pushing it
            // further out, is optimal in that coordinate.
            let at_lo = lower.and_then(|l| l.get(j)).is_some_and(|lo| x[j] <= *lo);
            let at_hi = upper.and_then(|u| u.get(j)).is_some_and(|hi| x[j] >= *hi);
            if (at_lo && *gj > 0.0) || (at_hi && *gj < 0.0) {
                return true;
            }
            let cn = norm2(&jac.as_slice()[j * m..(j + 1) * m]);
            cn == 0.0 || gj.abs() <= gtol * cn * rn
        })
    };
    loop {
        let jac = box_jacobian(&mut residuals, &x, &r, lower, upper, true)?;
        let g = jt_times(&jac, &r);
        if optimal(&jac, &g, &r, cost, &x) {
            converged = true;
            break;
        }
        if iterations >= max_iter {
            break;
        }
        iterations += 1;
        let gn = norm2(&g);
        if gn == 0.0 || !gn.is_finite() {
            break;
        }
        // Grow the previous accepted step a little each time, so a step
        // that had to be cut once is not stuck small forever.
        let mut t = step.map(|s| 2.0 * s).unwrap_or(1.0 / gn);
        let mut accepted = None;
        for _ in 0..60 {
            let mut trial: Vec<f64> = x.iter().zip(&g).map(|(xi, gi)| xi - t * gi).collect();
            project_into_box(&mut trial, lower, upper);
            let moved: f64 = trial.iter().zip(&x).map(|(a, b)| (a - b) * (a - b)).sum();
            let rt = residuals(&trial);
            let ct = sum_sq(&rt);
            // Projected Armijo on f = 0.5 ||r||^2:
            //   f(x_t) <= f(x) - (c / t) ||x_t - x||^2.
            if moved > 0.0 && rt.len() == r.len() && ct.is_finite() && 0.5 * ct <= 0.5 * cost - 1e-4 / t * moved {
                accepted = Some((trial, rt, ct));
                break;
            }
            t *= 0.5;
        }
        match accepted {
            None => break,
            Some((trial, rt, ct)) => {
                x = trial;
                r = rt;
                cost = ct;
                step = Some(t);
            }
        }
    }
    Ok(LmResult { params: x, cost, iterations, converged })
}

/// Parameter covariance of a least-squares fit, linearized at the solution.
pub struct FitCovariance {
    /// Number of residuals.
    pub n: usize,
    /// Number of parameters.
    pub p: usize,
    /// `n - p`; zero or negative means there is no residual variance to
    /// estimate, and every statistic below is `NaN`.
    pub dof: i64,
    /// Residual sum of squares at the solution.
    pub rss: f64,
    /// The residual at the solution.
    pub residual: Vec<f64>,
    /// `(n, p)` central-difference Jacobian at the solution.
    pub jacobian: Matrix,
    /// `rss / dof` (`NaN` when `dof <= 0`).
    pub s2: f64,
    /// `s2 * (J^T J)^+`, `(p, p)`. Rows and columns of an unidentifiable
    /// parameter are `NaN`, with `inf` on the diagonal.
    pub cov: Matrix,
    /// `sqrt(diag(cov))`; `inf` for an unidentifiable parameter.
    pub stderr: Vec<f64>,
    /// `cov[i,j] / (stderr[i] stderr[j])`, `NaN` wherever undefined.
    pub correlation: Matrix,
    /// Numerical rank of the (column-scaled) Jacobian.
    pub rank: usize,
    /// `false` for a parameter the data does not determine (it loads on a
    /// direction of `J` whose singular value has collapsed).
    pub identifiable: Vec<bool>,
    /// `true` for a parameter sitting on its `lower`/`upper` bound, where
    /// the linearized standard error does not describe the uncertainty
    /// (the estimate's distribution is truncated there).
    pub at_bound: Vec<bool>,
    /// Condition number of the column-scaled Jacobian (`inf` when
    /// rank-deficient).
    pub condition_number: f64,
}

/// Relative singular-value cutoff below which a direction of the scaled
/// Jacobian counts as unidentifiable. Far above `eps`, on purpose: the
/// Jacobian is a central difference with ~`1e-11` relative error, so an
/// exactly redundant pair of parameters shows up as a singular value near
/// `1e-11 * s_max`, not zero -- an `eps`-sized cutoff would miss it and
/// report a finite, meaningless standard error instead.
const RANK_CUTOFF: f64 = 1e-9;

/// Parameter uncertainty for a least-squares fit: the Jacobian `J` of
/// `residuals` at `params` (central differences, one-sided inside
/// `lower`/`upper` at a bound), and `cov = s^2 (J^T J)^+` with
/// `s^2 = RSS / (n - p)` -- the standard linearized (Gauss-Newton)
/// covariance that scipy's `curve_fit` and statsmodels report. For a
/// model linear in its parameters it is exactly the OLS covariance.
///
/// Computed through the SVD of `J` with each column scaled by its
/// parameter's magnitude, rather than by forming and inverting `J^T J`:
/// forming `J^T J` squares the condition number, and scaling removes the
/// fake ill-conditioning of parameters in different units. When `J` is
/// rank-deficient the pseudo-inverse is used, and every parameter that
/// loads on a collapsed direction is marked unidentifiable with `inf`
/// standard error -- the pseudo-inverse alone would report a small,
/// meaningless number for exactly those parameters.
pub fn fit_covariance(
    residuals: &mut dyn FnMut(&[f64]) -> Vec<f64>,
    params: &[f64],
    lower: Option<&[f64]>,
    upper: Option<&[f64]>,
) -> Result<FitCovariance, NumericError> {
    let p = params.len();
    if p == 0 {
        return Err(NumericError::EmptyInput("fit_covariance: needs at least one parameter"));
    }
    let residual = residuals(params);
    let n = residual.len();
    if n == 0 {
        return Err(NumericError::EmptyInput("fit_covariance: the residual"));
    }
    let jacobian = box_jacobian(residuals, params, &residual, lower, upper, true)?;
    let rss = sum_sq(&residual);
    let dof = n as i64 - p as i64;
    let s2 = if dof > 0 { rss / dof as f64 } else { f64::NAN };

    let at_bound: Vec<bool> = (0..p)
        .map(|j| {
            let x = params[j];
            let near = |b: f64| x == b || (x - b).abs() <= 1e-10 * b.abs().max(x.abs());
            lower.and_then(|l| l.get(j)).is_some_and(|b| near(*b))
                || upper.and_then(|u| u.get(j)).is_some_and(|b| near(*b))
        })
        .collect();

    if jacobian.as_slice().iter().any(|v| !v.is_finite()) {
        return Err(NumericError::Decomposition(
            "the Jacobian at the solution is not finite (the model returns NaN/inf next to the fitted parameters)".into(),
        ));
    }
    // Column scaling by the parameter's own magnitude.
    let scale: Vec<f64> = params.iter().map(|v| if *v != 0.0 && v.is_finite() { v.abs() } else { 1.0 }).collect();
    let mut scaled = jacobian.as_slice().to_vec();
    for j in 0..p {
        for v in &mut scaled[j * n..(j + 1) * n] {
            *v *= scale[j];
        }
    }
    let svd = linalg::svd(&Matrix::from_col_major(n, p, scaled))
        .map_err(|e| NumericError::Decomposition(format!("fit_covariance: {e}")))?;
    let s = &svd.singular_values;
    let smax = s.iter().cloned().fold(0.0f64, f64::max);
    let keep: Vec<bool> = s.iter().map(|&v| smax > 0.0 && v > RANK_CUTOFF * smax).collect();
    let rank = keep.iter().filter(|k| **k).count();
    let k_rows = svd.v_t.rows();
    let vt = svd.v_t.as_slice();
    let v = |kk: usize, j: usize| vt[j * k_rows + kk];
    // A parameter is identifiable when its unit vector lies in the row space
    // spanned by the kept directions.
    let identifiable: Vec<bool> = (0..p)
        .map(|j| {
            let in_span: f64 = (0..s.len().min(k_rows)).filter(|&kk| keep[kk]).map(|kk| v(kk, j) * v(kk, j)).sum();
            in_span >= 1.0 - 1e-6
        })
        .collect();
    let smin_kept = s.iter().zip(&keep).filter(|(_, k)| **k).map(|(v, _)| *v).fold(f64::INFINITY, f64::min);
    let condition_number = if rank == p { smax / smin_kept } else { f64::INFINITY };

    let mut cov = vec![0.0; p * p];
    for a in 0..p {
        for b in 0..p {
            let val = if !identifiable[a] || !identifiable[b] {
                if a == b { f64::INFINITY } else { f64::NAN }
            } else {
                let mut acc = 0.0;
                for kk in 0..s.len().min(k_rows) {
                    if keep[kk] {
                        acc += v(kk, a) * v(kk, b) / (s[kk] * s[kk]);
                    }
                }
                acc * scale[a] * scale[b] * s2
            };
            cov[b * p + a] = if dof > 0 { val } else { f64::NAN };
        }
    }
    let stderr: Vec<f64> = (0..p).map(|j| cov[j * p + j].sqrt()).collect();
    let mut corr = vec![f64::NAN; p * p];
    for a in 0..p {
        for b in 0..p {
            if stderr[a].is_finite() && stderr[b].is_finite() {
                corr[b * p + a] = if a == b { 1.0 } else { cov[b * p + a] / (stderr[a] * stderr[b]) };
            }
        }
    }
    Ok(FitCovariance {
        n,
        p,
        dof,
        rss,
        residual,
        jacobian,
        s2,
        cov: Matrix::from_col_major(p, p, cov),
        stderr,
        correlation: Matrix::from_col_major(p, p, corr),
        rank,
        identifiable,
        at_bound,
        condition_number,
    })
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

    // ---- Gauss-Newton / gradient descent / fit covariance ----------------

    /// y = 5 exp(-0.5 x) on 0..3.8, noiseless.
    fn decay_residuals() -> impl FnMut(&[f64]) -> Vec<f64> {
        let xs: Vec<f64> = (0..20).map(|i| i as f64 * 0.2).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| 5.0 * (-0.5 * x).exp()).collect();
        move |p: &[f64]| xs.iter().zip(&ys).map(|(&x, &y)| p[0] * (-p[1] * x).exp() - y).collect()
    }

    #[test]
    fn gauss_newton_fits_an_exponential_decay() {
        let r = gauss_newton(decay_residuals(), &[1.0, 1.0], None, None, 100, 1e-12).unwrap();
        assert!(r.converged);
        close(r.params[0], 5.0, 1e-7);
        close(r.params[1], 0.5, 1e-7);
        assert!(r.iterations < 30, "{} iterations", r.iterations);
    }

    #[test]
    fn gauss_newton_solves_rosenbrock_residuals() {
        // r = (10 (y - x^2), 1 - x): a zero-residual problem, GN territory.
        let f = |p: &[f64]| vec![10.0 * (p[1] - p[0] * p[0]), 1.0 - p[0]];
        let r = gauss_newton(f, &[-1.2, 1.0], None, None, 100, 1e-12).unwrap();
        assert!(r.converged);
        close(r.params[0], 1.0, 1e-8);
        close(r.params[1], 1.0, 1e-8);
    }

    #[test]
    fn gauss_newton_stays_inside_its_box() {
        // Unconstrained optimum a = 5; upper bound 4 must hold.
        let r = gauss_newton(decay_residuals(), &[1.0, 1.0], None, Some(&[4.0, 10.0]), 100, 1e-12).unwrap();
        assert!(r.params[0] <= 4.0);
        close(r.params[0], 4.0, 1e-12);
    }

    #[test]
    fn gradient_descent_converges_on_a_well_conditioned_problem() {
        // Linear residual A p - b with cond(A^T A) = 4.
        let f = |p: &[f64]| vec![p[0] - 1.0, 2.0 * p[1] + 3.0, 0.1];
        let r = gradient_descent(f, &[0.0, 0.0], None, None, 500, 1e-10).unwrap();
        assert!(r.converged, "iterations = {}", r.iterations);
        close(r.params[0], 1.0, 1e-6);
        close(r.params[1], -1.5, 1e-6);
    }

    #[test]
    fn gradient_descent_says_so_when_it_has_not_converged() {
        // Rosenbrock's curved valley: steepest descent zig-zags for
        // thousands of iterations. Fifty is nowhere near enough, and the
        // result must say so rather than claim the point it stopped at.
        let f = |p: &[f64]| vec![10.0 * (p[1] - p[0] * p[0]), 1.0 - p[0]];
        let r = gradient_descent(f, &[-1.2, 1.0], None, None, 50, 1e-10).unwrap();
        assert!(!r.converged);
        assert_eq!(r.iterations, 50);
        assert!((r.params[0] - 1.0).abs() > 1e-3);
    }

    #[test]
    fn gradient_descent_respects_bounds() {
        let f = |p: &[f64]| vec![p[0] - 3.0, p[1] + 1.0];
        let r = gradient_descent(f, &[0.0, 0.0], Some(&[-10.0, 0.0]), Some(&[2.0, 10.0]), 500, 1e-10).unwrap();
        assert!(r.converged);
        close(r.params[0], 2.0, 1e-12);
        close(r.params[1], 0.0, 1e-12);
    }

    #[test]
    fn fit_covariance_is_the_ols_covariance_for_a_linear_model() {
        // y = b0 + b1 x with a fixed perturbation; closed form
        // cov = s^2 (X^T X)^-1, s^2 = RSS / (n - 2).
        let xs: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs
            .iter()
            .enumerate()
            .map(|(i, &x)| 1.5 + 0.7 * x + [0.3, -0.2, 0.1, -0.4, 0.25][i % 5])
            .collect();
        let n = xs.len() as f64;
        let (sx, sy) = (xs.iter().sum::<f64>(), ys.iter().sum::<f64>());
        let sxx: f64 = xs.iter().map(|x| x * x).sum();
        let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| x * y).sum();
        let det = n * sxx - sx * sx;
        let b1 = (n * sxy - sx * sy) / det;
        let b0 = (sy - b1 * sx) / n;
        let rss: f64 = xs.iter().zip(&ys).map(|(x, y)| (y - b0 - b1 * x).powi(2)).sum();
        let s2 = rss / (n - 2.0);
        let (v00, v11, v01) = (s2 * sxx / det, s2 * n / det, -s2 * sx / det);
        let mut f = move |p: &[f64]| -> Vec<f64> { xs.iter().zip(&ys).map(|(x, y)| p[0] + p[1] * x - y).collect() };
        let c = fit_covariance(&mut f, &[b0, b1], None, None).unwrap();
        assert_eq!(c.dof, 8);
        assert_eq!(c.rank, 2);
        let rel = |a: f64, b: f64| (a - b).abs() / b.abs();
        assert!(rel(c.cov.get(0, 0).unwrap(), v00) < 1e-8);
        assert!(rel(c.cov.get(1, 1).unwrap(), v11) < 1e-8);
        assert!(rel(c.cov.get(0, 1).unwrap(), v01) < 1e-8);
        assert!(rel(c.stderr[1], v11.sqrt()) < 1e-8);
        close(c.correlation.get(0, 1).unwrap(), v01 / (v00 * v11).sqrt(), 1e-8);
    }

    #[test]
    fn fit_covariance_marks_an_unidentifiable_parameter() {
        // y = a * b * x: only the product is determined.
        let xs: Vec<f64> = (1..=8).map(|i| i as f64).collect();
        let ys: Vec<f64> =
            xs.iter().enumerate().map(|(i, x)| 6.0 * x + if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        let mut f = move |p: &[f64]| -> Vec<f64> { xs.iter().zip(&ys).map(|(x, y)| p[0] * p[1] * x - y).collect() };
        let c = fit_covariance(&mut f, &[2.0, 3.0], None, None).unwrap();
        assert_eq!(c.rank, 1);
        assert_eq!(c.identifiable, vec![false, false]);
        assert!(c.stderr.iter().all(|s| s.is_infinite()));
        assert!(c.condition_number.is_infinite());
    }

    #[test]
    fn fit_covariance_without_degrees_of_freedom_is_nan_not_a_division_by_zero() {
        let mut f = |p: &[f64]| vec![p[0] - 1.0, p[1] - 2.0];
        let c = fit_covariance(&mut f, &[1.0, 2.0], None, None).unwrap();
        assert_eq!(c.dof, 0);
        assert!(c.s2.is_nan());
        assert!(c.stderr.iter().all(|s| s.is_nan()));
    }

    #[test]
    fn fit_covariance_flags_a_parameter_on_its_bound() {
        let mut f = |p: &[f64]| vec![p[0] - 3.0, p[0] - 2.0, p[1] + 1.0, p[1] + 2.0];
        let c = fit_covariance(&mut f, &[2.0, -1.5], None, Some(&[2.0, 5.0])).unwrap();
        assert_eq!(c.at_bound, vec![true, false]);
        assert!(c.stderr[0].is_finite(), "one-sided difference keeps the column");
    }
}
