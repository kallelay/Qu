//! Nelder-Mead simplex minimiser: derivative-free, so it works on noisy or
//! non-smooth objectives where a gradient method stalls. Uses the adaptive
//! coefficients of Gao and Han (2012), which keep the method effective as the
//! dimension grows (the classic fixed 1/2/0.5/0.5 degrades past ~5 variables);
//! in one dimension the classic coefficients are used because the adaptive
//! shrink factor `1 - 1/n` would be zero there.
//!
//! The initial simplex perturbs each coordinate of `x0` by 5% (or by 0.00025
//! when it is zero), the same convention as SciPy and MATLAB's `fminsearch`.
//!
//! Converged means BOTH the simplex has shrunk (every vertex within `tol` of
//! the best in each coordinate) and the function values agree to `tol`.
//! An objective that returns NaN is treated as +infinity, i.e. a point the
//! simplex steps away from.

#[derive(Debug, Clone)]
pub struct NmResult {
    pub params: Vec<f64>,
    pub value: f64,
    pub iterations: usize,
    pub evaluations: usize,
    pub converged: bool,
}

fn key(v: f64) -> f64 {
    if v.is_nan() {
        f64::INFINITY
    } else {
        v
    }
}

pub fn nelder_mead(
    f: &mut dyn FnMut(&[f64]) -> f64,
    x0: &[f64],
    max_iter: usize,
    tol: f64,
) -> Result<NmResult, String> {
    let n = x0.len();
    if n == 0 {
        return Err("nelder_mead: x0 must have at least one element".into());
    }
    if x0.iter().any(|v| !v.is_finite()) {
        return Err("nelder_mead: x0 must be finite (it contains NaN or Inf)".into());
    }
    if !(tol > 0.0) || !tol.is_finite() {
        return Err(format!("nelder_mead: tol must be a positive number, found {tol}"));
    }
    let nf = n as f64;
    let (alpha, beta, gamma, delta) = if n == 1 {
        (1.0, 2.0, 0.5, 0.5)
    } else {
        (1.0, 1.0 + 2.0 / nf, 0.75 - 1.0 / (2.0 * nf), 1.0 - 1.0 / nf)
    };
    let evals = std::cell::Cell::new(0usize);
    let eval = |x: &[f64], f: &mut dyn FnMut(&[f64]) -> f64| -> f64 {
        evals.set(evals.get() + 1);
        key(f(x))
    };
    let f0 = f(x0);
    if !f0.is_finite() {
        return Err("nelder_mead: f(x0) is not finite (it is NaN or Inf); choose a different starting point".into());
    }
    evals.set(evals.get() + 1);
    let mut sim: Vec<Vec<f64>> = vec![x0.to_vec()];
    let mut fv: Vec<f64> = vec![f0];
    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] = if p[i] != 0.0 { p[i] * 1.05 } else { 0.00025 };
        fv.push(eval(&p, f));
        sim.push(p);
    }
    let sort = |sim: &mut Vec<Vec<f64>>, fv: &mut Vec<f64>| {
        let mut idx: Vec<usize> = (0..sim.len()).collect();
        idx.sort_by(|&a, &b| fv[a].partial_cmp(&fv[b]).unwrap());
        *sim = idx.iter().map(|&i| sim[i].clone()).collect();
        *fv = idx.iter().map(|&i| fv[i]).collect();
    };
    sort(&mut sim, &mut fv);
    let mut iterations = 0usize;
    let mut converged = false;
    while iterations < max_iter {
        let dx = sim[1..].iter().flat_map(|p| p.iter().zip(&sim[0]).map(|(a, b)| (a - b).abs())).fold(0.0f64, f64::max);
        let df = fv[1..].iter().map(|v| (v - fv[0]).abs()).fold(0.0f64, f64::max);
        if dx <= tol && df <= tol {
            converged = true;
            break;
        }
        iterations += 1;
        let xc: Vec<f64> = (0..n).map(|j| sim[..n].iter().map(|p| p[j]).sum::<f64>() / nf).collect();
        let xw = sim[n].clone();
        let along = |t: f64| -> Vec<f64> { (0..n).map(|j| xc[j] + t * (xc[j] - xw[j])).collect() };
        let xr = along(alpha);
        let fr = eval(&xr, f);
        let mut shrink = false;
        if fr < fv[0] {
            let xe = along(alpha * beta);
            let fe = eval(&xe, f);
            if fe < fr {
                sim[n] = xe;
                fv[n] = fe;
            } else {
                sim[n] = xr;
                fv[n] = fr;
            }
        } else if fr < fv[n - 1] {
            sim[n] = xr;
            fv[n] = fr;
        } else if fr < fv[n] {
            let xoc = along(alpha * gamma);
            let foc = eval(&xoc, f);
            if foc <= fr {
                sim[n] = xoc;
                fv[n] = foc;
            } else {
                shrink = true;
            }
        } else {
            let xic = along(-gamma);
            let fic = eval(&xic, f);
            if fic < fv[n] {
                sim[n] = xic;
                fv[n] = fic;
            } else {
                shrink = true;
            }
        }
        if shrink {
            let best = sim[0].clone();
            for i in 1..=n {
                for j in 0..n {
                    sim[i][j] = best[j] + delta * (sim[i][j] - best[j]);
                }
                let p = sim[i].clone();
                fv[i] = eval(&p, f);
            }
        }
        sort(&mut sim, &mut fv);
    }
    Ok(NmResult { params: sim[0].clone(), value: fv[0], iterations, evaluations: evals.get(), converged })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rosenbrock() {
        let mut f = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let r = nelder_mead(&mut f, &[-1.2, 1.0], 800, 1e-10).unwrap();
        assert!(r.converged, "{r:?}");
        assert!((r.params[0] - 1.0).abs() < 1e-4 && (r.params[1] - 1.0).abs() < 1e-4, "{:?}", r.params);
        assert!(r.value < 1e-8);
    }

    #[test]
    fn shifted_bowl_in_five_dimensions() {
        let target = [1.0, -2.0, 3.0, 0.5, -0.25];
        let mut f = |p: &[f64]| p.iter().zip(&target).map(|(a, b)| (a - b) * (a - b)).sum::<f64>();
        let r = nelder_mead(&mut f, &[0.0; 5], 5000, 1e-10).unwrap();
        assert!(r.converged);
        for (a, b) in r.params.iter().zip(target) {
            assert!((a - b).abs() < 1e-4, "{:?}", r.params);
        }
    }

    #[test]
    fn non_smooth_objective() {
        // |x - 1| + |y + 2|: no gradient at the minimum
        let mut f = |p: &[f64]| (p[0] - 1.0).abs() + (p[1] + 2.0).abs();
        let r = nelder_mead(&mut f, &[4.0, 3.0], 2000, 1e-9).unwrap();
        assert!((r.params[0] - 1.0).abs() < 1e-5 && (r.params[1] + 2.0).abs() < 1e-5, "{:?}", r.params);
    }

    #[test]
    fn one_dimensional() {
        let mut f = |p: &[f64]| (p[0] - 3.0).powi(2) + 1.0;
        let r = nelder_mead(&mut f, &[0.0], 500, 1e-12).unwrap();
        assert!((r.params[0] - 3.0).abs() < 1e-5 && (r.value - 1.0).abs() < 1e-9);
    }

    #[test]
    fn iteration_limit_is_not_converged() {
        let mut f = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let r = nelder_mead(&mut f, &[-1.2, 1.0], 5, 1e-10).unwrap();
        assert!(!r.converged);
        assert!(r.iterations <= 5);
    }

    #[test]
    fn nan_region_is_avoided() {
        // sqrt of a negative number is NaN outside x >= 0.5; the minimum of
        // (x - 0.6)^2 + sqrt(x - 0.5) is at the edge but finite values only
        let mut f = |p: &[f64]| (p[0] - 2.0).powi(2) + (p[0] - 0.5).sqrt();
        let r = nelder_mead(&mut f, &[3.0], 500, 1e-10).unwrap();
        assert!(r.value.is_finite() && r.params[0] >= 0.5);
    }

    #[test]
    fn bad_start_is_an_error() {
        let mut f = |p: &[f64]| p[0];
        assert!(nelder_mead(&mut f, &[], 10, 1e-8).is_err());
        assert!(nelder_mead(&mut f, &[f64::NAN], 10, 1e-8).is_err());
        let mut nanf = |_: &[f64]| f64::NAN;
        assert!(nelder_mead(&mut nanf, &[1.0], 10, 1e-8).unwrap_err().contains("not finite"));
    }
}
