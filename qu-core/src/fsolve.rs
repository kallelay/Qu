//! `fsolve`: solve the nonlinear system `f(x) = 0` by Levenberg-Marquardt
//! with Nielsen's damping update and Marquardt scaling.
//!
//! LM is the damped-Newton / trust-region hybrid: far from the root the
//! damping makes it a (scaled) gradient step, close to it the damping decays
//! and it becomes Newton with quadratic convergence. It needs no square
//! Jacobian, so under- and over-determined systems are solved in the least
//! squares sense with no extra code path.
//!
//! The Jacobian is a forward difference unless the caller supplies one.
//! Everything is bounded: each loop pass (accepted or rejected trial step)
//! counts as an iteration, and a pass costs at most `n + 7` calls of `f`.

use crate::small_dense::{dot, norm2, solve};

#[derive(Debug, Clone)]
pub struct FsolveResult {
    pub x: Vec<f64>,
    pub fval: Vec<f64>,
    pub norm: f64,
    pub iterations: usize,
    pub converged: bool,
}

pub type VecFn<'a> = &'a mut dyn FnMut(&[f64]) -> Result<Vec<f64>, String>;

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Forward-difference Jacobian, row-major `m x n`.
fn fd_jacobian(f: &mut dyn FnMut(&[f64]) -> Result<Vec<f64>, String>, x: &[f64], r: &[f64]) -> Result<Vec<f64>, String> {
    let (m, n) = (r.len(), x.len());
    let mut jac = vec![0.0; m * n];
    let mut xp = x.to_vec();
    for j in 0..n {
        let h = 1.490_116_119_384_765_6e-8 * x[j].abs().max(1.0);
        xp[j] = x[j] + h;
        let step = xp[j] - x[j];
        let rp = f(&xp)?;
        xp[j] = x[j];
        if rp.len() != m {
            return Err(format!("fsolve: f returned {} values, but at the start it returned {m}", rp.len()));
        }
        for i in 0..m {
            jac[i * n + j] = (rp[i] - r[i]) / step;
        }
    }
    Ok(jac)
}

/// Solve `f(x) = 0` from `x0`.
///
/// `jac`, if given, returns the row-major `m x n` Jacobian at `x`.
/// Converged means `||f(x)|| <= tol * max(1, ||f(x0)||)`; for an
/// over-determined system (`m > n`, no exact root in general) it also means
/// that `f` is orthogonal to the Jacobian's columns to within 1e-8, i.e. the
/// least-squares minimum was reached.
pub fn fsolve(
    f: VecFn,
    mut jac: Option<VecFn>,
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<FsolveResult, String> {
    let n = x0.len();
    if n == 0 {
        return Err("fsolve: x0 must have at least one element".into());
    }
    if !finite(x0) {
        return Err("fsolve: x0 must be finite (it contains NaN or Inf)".into());
    }
    if !(tol > 0.0) || !tol.is_finite() {
        return Err(format!("fsolve: tol must be a positive number, found {tol}"));
    }
    let mut x = x0.to_vec();
    let mut r = f(&x)?;
    let m = r.len();
    if m == 0 {
        return Err("fsolve: f returned an empty vector".into());
    }
    if !finite(&r) {
        return Err("fsolve: f(x0) is not finite (it contains NaN or Inf); choose a different starting point".into());
    }
    let norm0 = norm2(&r);
    let thr = tol * norm0.max(1.0);
    let mut cost = 0.5 * dot(&r, &r);

    let mut compute_jac = |x: &[f64], r: &[f64], f: &mut dyn FnMut(&[f64]) -> Result<Vec<f64>, String>| -> Result<Vec<f64>, String> {
        match jac.as_mut() {
            Some(j) => {
                let v = j(x)?;
                if v.len() != m * n {
                    return Err(format!("fsolve: jac returned {} values, expected a {m}x{n} matrix ({} values)", v.len(), m * n));
                }
                if !finite(&v) {
                    return Err("fsolve: jac returned a non-finite value".into());
                }
                Ok(v)
            }
            None => fd_jacobian(f, x, r),
        }
    };

    let mut j = compute_jac(&x, &r, f)?;
    let mut lambda = -1.0f64; // set from the first normal matrix
    let mut nu = 2.0f64;
    let mut iterations = 0usize;
    let mut stalled = false;
    let mut try_newton = true;

    // normal matrix A = J'J (n x n, row-major) and gradient g = J'r
    let normal = |j: &[f64], r: &[f64]| -> (Vec<f64>, Vec<f64>) {
        let mut a = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            let row = &j[i * n..(i + 1) * n];
            for p in 0..n {
                g[p] += row[p] * r[i];
                if row[p] != 0.0 {
                    for q in p..n {
                        a[p * n + q] += row[p] * row[q];
                    }
                }
            }
        }
        for p in 0..n {
            for q in 0..p {
                a[p * n + q] = a[q * n + p];
            }
        }
        (a, g)
    };

    while iterations < max_iter {
        let rn = norm2(&r);
        if rn <= thr {
            break;
        }
        let (a, g) = normal(&j, &r);
        let maxdiag = (0..n).map(|p| a[p * n + p]).fold(0.0f64, f64::max);
        if lambda < 0.0 {
            lambda = 1e-3 * if maxdiag > 0.0 { maxdiag } else { 1.0 };
        }
        // scale-free first-order optimality test: cos of the angle between r
        // and the range of J. Zero gradient on a square system with a
        // non-zero residual is a local minimum of ||f||, not a root.
        let jf = norm2(&j);
        let cosine = if jf > 0.0 && rn > 0.0 { norm2(&g) / (jf * rn) } else { 0.0 };
        if cosine <= 1e-12 {
            stalled = true;
            break;
        }
        if m > n && cosine <= 1e-8 {
            break;
        }
        iterations += 1;
        // Newton first: the full Newton / Gauss-Newton step with a short
        // backtracking line search on ||f||. Near a root (and on many
        // far-from-root problems) this converges quadratically where the
        // damped step would creep; when it cannot reduce ||f|| the damped
        // (Levenberg-Marquardt) step below takes over.
        if try_newton {
            let neg_r: Vec<f64> = r.iter().map(|v| -v).collect();
            let dn = if m == n {
                solve(&j, n, &neg_r)
            } else {
                let mut mat = a.clone();
                let bump = 1e-12 * if maxdiag > 0.0 { maxdiag } else { 1.0 };
                for p in 0..n {
                    mat[p * n + p] += bump;
                }
                let neg_g: Vec<f64> = g.iter().map(|v| -v).collect();
                solve(&mat, n, &neg_g)
            };
            let mut accepted = false;
            if let Some(dn) = dn.filter(|d| finite(d)) {
                let mut alpha = 1.0f64;
                for _ in 0..6 {
                    let xt: Vec<f64> = x.iter().zip(&dn).map(|(a, b)| a + alpha * b).collect();
                    let rt = f(&xt)?;
                    if rt.len() != m {
                        return Err(format!("fsolve: f returned {} values, but at the start it returned {m}", rt.len()));
                    }
                    if finite(&rt) {
                        let nt = norm2(&rt);
                        if nt <= (1.0 - 1e-4 * alpha) * rn {
                            x = xt;
                            r = rt;
                            cost = 0.5 * nt * nt;
                            accepted = true;
                            break;
                        }
                    }
                    alpha *= 0.5;
                }
            }
            if accepted {
                j = compute_jac(&x, &r, f)?;
                continue;
            }
            try_newton = false;
        }
        let dmin = if maxdiag > 0.0 { 1e-10 * maxdiag } else { 1.0 };
        let d: Vec<f64> = (0..n).map(|p| a[p * n + p].max(dmin)).collect();
        let mut mat = a.clone();
        for p in 0..n {
            mat[p * n + p] += lambda * d[p];
        }
        let neg_g: Vec<f64> = g.iter().map(|v| -v).collect();
        let delta = match solve(&mat, n, &neg_g) {
            Some(dl) => dl,
            None => {
                lambda *= nu;
                nu *= 2.0;
                if lambda > 1e18 {
                    stalled = true;
                    break;
                }
                continue;
            }
        };
        let xn = norm2(&x);
        if norm2(&delta) <= 1e-15 * (xn + 1e-15) {
            stalled = true;
            break;
        }
        let xt: Vec<f64> = x.iter().zip(&delta).map(|(a, b)| a + b).collect();
        let rt = f(&xt)?;
        if rt.len() != m {
            return Err(format!("fsolve: f returned {} values, but at the start it returned {m}", rt.len()));
        }
        let cost_t = if finite(&rt) { 0.5 * dot(&rt, &rt) } else { f64::INFINITY };
        // predicted reduction of the quadratic model: 0.5 d'(lambda D d - g)
        let pred = 0.5 * (0..n).map(|p| delta[p] * (lambda * d[p] * delta[p] - g[p])).sum::<f64>();
        let rho = if pred > 0.0 { (cost - cost_t) / pred } else { -1.0 };
        if rho > 0.0 && cost_t.is_finite() {
            x = xt;
            r = rt;
            cost = cost_t;
            let t = 2.0 * rho - 1.0;
            lambda *= (1.0f64 / 3.0).max(1.0 - t * t * t);
            nu = 2.0;
            lambda = lambda.max(1e-30);
            if rho > 0.75 {
                try_newton = true;
            }
            j = compute_jac(&x, &r, f)?;
        } else {
            lambda *= nu;
            nu *= 2.0;
            if lambda > 1e18 {
                stalled = true;
                break;
            }
        }
    }
    let rn = norm2(&r);
    let mut converged = rn <= thr;
    if !converged && m > n {
        let (_, g) = normal(&j, &r);
        let jf = norm2(&j);
        let cosine = if jf > 0.0 && rn > 0.0 { norm2(&g) / (jf * rn) } else { 0.0 };
        converged = cosine <= if stalled { 1e-6 } else { 1e-8 };
    }
    Ok(FsolveResult { x, fval: r, norm: rn, iterations, converged })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl Fn(&[f64]) -> Vec<f64>, x0: &[f64], max_iter: usize) -> FsolveResult {
        let mut ff = |x: &[f64]| Ok(f(x));
        fsolve(&mut ff, None, x0, 1e-10, max_iter).unwrap()
    }

    #[test]
    fn rosenbrock_as_a_system() {
        // roots of [10 (y - x^2), 1 - x]: x = y = 1
        let r = run(|v| vec![10.0 * (v[1] - v[0] * v[0]), 1.0 - v[0]], &[-1.2, 1.0], 200);
        assert!(r.converged, "{r:?}");
        assert!((r.x[0] - 1.0).abs() < 1e-8 && (r.x[1] - 1.0).abs() < 1e-8, "{:?}", r.x);
    }

    #[test]
    fn circle_meets_line() {
        // x^2 + y^2 = 4, y = x: x = y = sqrt(2)
        let r = run(|v| vec![v[0] * v[0] + v[1] * v[1] - 4.0, v[0] - v[1]], &[1.0, 0.5], 200);
        assert!(r.converged);
        let s = 2.0f64.sqrt();
        assert!((r.x[0] - s).abs() < 1e-8 && (r.x[1] - s).abs() < 1e-8);
        assert!(r.norm <= 1e-9);
    }

    #[test]
    fn powell_badly_scaled() {
        // 1e4 x y = 1, exp(-x) + exp(-y) = 1.0001: x = 1.098159e-5, y = 9.106146
        let r = run(
            |v| vec![1e4 * v[0] * v[1] - 1.0, (-v[0]).exp() + (-v[1]).exp() - 1.0001],
            &[0.0, 1.0],
            2000,
        );
        assert!(r.converged, "{r:?}");
        // verify against the equations, not against a stored answer
        let (x, y) = (r.x[0], r.x[1]);
        assert!((1e4 * x * y - 1.0).abs() < 1e-8);
        assert!(((-x).exp() + (-y).exp() - 1.0001).abs() < 1e-8);
        assert!((x - 1.098159e-5).abs() < 1e-9 && (y - 9.106146).abs() < 1e-5, "{x} {y}");
    }

    #[test]
    fn three_by_three_with_known_root() {
        let r = run(
            |v| {
                vec![
                    v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - 14.0,
                    v[0] * v[1] - 2.0,
                    (v[0] - 1.0).exp() + v[1] * v[2] - 7.0,
                ]
            },
            &[0.8, 1.5, 2.5],
            200,
        );
        assert!(r.converged, "{r:?}");
        for (a, b) in r.x.iter().zip([1.0, 2.0, 3.0]) {
            assert!((a - b).abs() < 1e-7, "{:?}", r.x);
        }
    }

    #[test]
    fn analytic_jacobian_agrees_and_costs_fewer_calls() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        let mut ff = |v: &[f64]| {
            calls.set(calls.get() + 1);
            Ok(vec![v[0] * v[0] + v[1] * v[1] - 4.0, v[0] - v[1]])
        };
        let mut jj = |v: &[f64]| Ok(vec![2.0 * v[0], 2.0 * v[1], 1.0, -1.0]);
        let r = fsolve(&mut ff, Some(&mut jj), &[1.0, 0.5], 1e-12, 200).unwrap();
        assert!(r.converged);
        assert!((r.x[0] - 2.0f64.sqrt()).abs() < 1e-10);
        let analytic_calls = calls.get();
        calls.set(0);
        let mut ff2 = |v: &[f64]| {
            calls.set(calls.get() + 1);
            Ok(vec![v[0] * v[0] + v[1] * v[1] - 4.0, v[0] - v[1]])
        };
        fsolve(&mut ff2, None, &[1.0, 0.5], 1e-12, 200).unwrap();
        assert!(analytic_calls < calls.get());
    }

    #[test]
    fn no_real_root_reports_not_converged() {
        // x^2 + 1 = 0 has no real root; the iteration finds the minimum of
        // |f| at x = 0 and must say it did not solve the system.
        let r = run(|v| vec![v[0] * v[0] + 1.0], &[2.0], 500);
        assert!(!r.converged, "{r:?}");
        assert!(r.norm >= 0.99);
    }

    #[test]
    fn iteration_limit_is_reported_not_raised() {
        let r = run(|v| vec![10.0 * (v[1] - v[0] * v[0]), 1.0 - v[0]], &[-1.2, 1.0], 2);
        assert!(!r.converged);
        assert!(r.iterations <= 2);
    }

    #[test]
    fn overdetermined_is_least_squares() {
        // x = 1, x = 2, x = 6: least squares minimum at the mean, 3
        let r = run(|v| vec![v[0] - 1.0, v[0] - 2.0, v[0] - 6.0], &[0.0], 100);
        assert!(r.converged);
        assert!((r.x[0] - 3.0).abs() < 1e-7);
    }

    #[test]
    fn underdetermined_finds_a_point_on_the_solution_set() {
        // x + y + z = 3 from the origin: any root is fine, check the equation
        let r = run(|v| vec![v[0] + v[1] + v[2] - 3.0], &[0.0, 0.0, 0.0], 100);
        assert!(r.converged);
        assert!((r.x.iter().sum::<f64>() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn bad_input_and_errors() {
        let mut ff = |v: &[f64]| Ok(vec![v[0]]);
        assert!(fsolve(&mut ff, None, &[], 1e-10, 10).is_err());
        assert!(fsolve(&mut ff, None, &[f64::NAN], 1e-10, 10).is_err());
        let mut nanf = |_: &[f64]| Ok(vec![f64::NAN]);
        assert!(fsolve(&mut nanf, None, &[1.0], 1e-10, 10).is_err());
        let mut failing = |_: &[f64]| Err("boom".to_string());
        assert_eq!(fsolve(&mut failing, None, &[1.0], 1e-10, 10).unwrap_err(), "boom");
        let mut ff3 = |v: &[f64]| Ok(vec![v[0]]);
        let mut badj = |_: &[f64]| Ok(vec![1.0, 2.0, 3.0]);
        assert!(fsolve(&mut ff3, Some(&mut badj), &[1.0], 1e-10, 10).unwrap_err().contains("jac returned 3"));
    }
}
