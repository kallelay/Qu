//! `quadprog`: convex quadratic programming.
//!
//! ```text
//! minimise  0.5 x'Hx + f'x   subject to   A x <= b,  Aeq x = beq,  lb <= x <= ub
//! ```
//!
//! `H` must be symmetric positive SEMIdefinite (so the problem is convex and
//! every local solution is global); a clearly indefinite or asymmetric `H` is
//! an error. Bounds default to `-inf`/`+inf`, the same convention as
//! `linprog`.
//!
//! Method (chosen over a primal active set because it stays robust when `H`
//! is singular and when constraints are degenerate):
//!
//! 1. exact classification first: infeasibility is decided by the phase-I
//!    simplex of [`crate::lp`]; unboundedness (a feasible recession direction
//!    in the null space of `H` along which the objective falls) by one small
//!    LP over that null space;
//! 2. a Mehrotra predictor-corrector primal-dual interior point method with a
//!    free `x` (no splitting) and the inequality slacks, one dense LU of the
//!    reduced KKT matrix per iteration;
//! 3. a polish step: the active set read off the interior-point solution is
//!    solved exactly as an equality-constrained KKT system, and kept only if
//!    it is feasible and has non-negative multipliers. That turns the
//!    interior-point's ~1e-9 answer into the vertex/face solution to rounding.

use crate::lp::{linprog, validate_constraints, Lp, LpStatus};
use crate::small_dense::{dot, norm_inf, Lu};

#[derive(Debug, Clone)]
pub struct QpResult {
    /// The minimiser; all NaN unless `status` is `Optimal`.
    pub x: Vec<f64>,
    /// `0.5 x'Hx + f'x`; NaN when infeasible or at the iteration limit,
    /// `-inf` when unbounded.
    pub fval: f64,
    pub status: LpStatus,
    pub iterations: usize,
}

pub struct Qp<'a> {
    pub h: &'a [Vec<f64>],
    pub f: &'a [f64],
    pub a: &'a [Vec<f64>],
    pub b: &'a [f64],
    pub aeq: &'a [Vec<f64>],
    pub beq: &'a [f64],
    pub lb: &'a [f64],
    pub ub: &'a [f64],
}

fn bad(status: LpStatus, n: usize, iterations: usize) -> QpResult {
    QpResult {
        x: vec![f64::NAN; n],
        fval: if status == LpStatus::Unbounded { f64::NEG_INFINITY } else { f64::NAN },
        status,
        iterations,
    }
}

fn objective(h: &[Vec<f64>], f: &[f64], x: &[f64]) -> f64 {
    let mut q = 0.0;
    for (i, row) in h.iter().enumerate() {
        q += x[i] * dot(row, x);
    }
    0.5 * q + dot(f, x)
}

fn mat_vec(rows: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    rows.iter().map(|r| dot(r, x)).collect()
}

fn mat_t_vec(rows: &[Vec<f64>], n: usize, y: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; n];
    for (r, &yi) in rows.iter().zip(y) {
        for j in 0..n {
            out[j] += r[j] * yi;
        }
    }
    out
}

fn max_step(v: &[f64], dv: &[f64]) -> f64 {
    let mut a = 1.0f64;
    for (x, d) in v.iter().zip(dv) {
        if *d < 0.0 {
            a = a.min(-x / d);
        }
    }
    a
}

pub fn quadprog(p: &Qp, max_iter: usize) -> Result<QpResult, String> {
    let n = p.f.len();
    if n == 0 {
        return Err("quadprog: f must have at least one element".into());
    }
    if p.h.len() != n || p.h.iter().any(|r| r.len() != n) {
        return Err(format!(
            "quadprog: H must be a {n}x{n} matrix to match f (it has {} rows)",
            p.h.len()
        ));
    }
    if p.h.iter().flatten().any(|v| !v.is_finite()) {
        return Err("quadprog: H must be finite (it contains NaN or Inf)".into());
    }
    if p.f.iter().any(|v| !v.is_finite()) {
        return Err("quadprog: f must be finite (it contains NaN or Inf)".into());
    }
    validate_constraints("quadprog", n, p.a, p.b, p.aeq, p.beq, p.lb, p.ub)?;
    let hmax = p.h.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
    let mut asym = 0.0f64;
    for i in 0..n {
        for j in 0..i {
            asym = asym.max((p.h[i][j] - p.h[j][i]).abs());
        }
    }
    if asym > 1e-8 * hmax.max(1.0) {
        return Err(format!(
            "quadprog: H must be symmetric (the largest |H - H'| entry is {asym:e}); pass (H + H') / 2 if the symmetric part is what you mean"
        ));
    }
    // symmetrised copy used from here on
    let h: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| 0.5 * (p.h[i][j] + p.h[j][i])).collect()).collect();
    let flat: Vec<f64> = h.iter().flatten().copied().collect();
    let eig = nalgebra::DMatrix::from_row_slice(n, n, &flat).symmetric_eigen();
    let lmax = eig.eigenvalues.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let lmin = eig.eigenvalues.iter().fold(f64::INFINITY, |m, v| m.min(*v));
    if lmin < -1e-8 * lmax.max(1.0) {
        return Err(format!(
            "quadprog: H is not positive semidefinite (its smallest eigenvalue is {lmin:e}); a non-convex quadratic program is not supported"
        ));
    }
    let null_tol = 1e-10 * lmax.max(1.0);
    let null_cols: Vec<usize> = (0..n).filter(|&i| eig.eigenvalues[i] <= null_tol).collect();

    // inequality rows G x <= hh: A x <= b plus the finite bounds
    let mut g: Vec<Vec<f64>> = p.a.to_vec();
    let mut hh: Vec<f64> = p.b.to_vec();
    for i in 0..n {
        if p.lb[i].is_finite() {
            let mut r = vec![0.0; n];
            r[i] = -1.0;
            g.push(r);
            hh.push(-p.lb[i]);
        }
        if p.ub[i].is_finite() {
            let mut r = vec![0.0; n];
            r[i] = 1.0;
            g.push(r);
            hh.push(p.ub[i]);
        }
    }
    let (e, d) = (p.aeq, p.beq);
    let mi = g.len();
    let pe = e.len();

    // 1a. feasibility, decided exactly
    if mi > 0 || pe > 0 {
        let zero = vec![0.0; n];
        let feas = linprog(
            &Lp { c: &zero, a: p.a, b: p.b, aeq: p.aeq, beq: p.beq, lb: p.lb, ub: p.ub },
            50_000 + 200 * (mi + pe + n),
        )?;
        match feas.status {
            LpStatus::Infeasible => return Ok(bad(LpStatus::Infeasible, n, 0)),
            LpStatus::Optimal => {}
            other => return Ok(bad(other, n, 0)),
        }
    }
    // 1b. unbounded below along a feasible direction in null(H)?
    if !null_cols.is_empty() {
        let k = null_cols.len();
        let basis: Vec<Vec<f64>> =
            null_cols.iter().map(|&c| (0..n).map(|i| eig.eigenvectors[(i, c)]).collect()).collect();
        let proj = |rows: &[Vec<f64>]| -> Vec<Vec<f64>> {
            rows.iter().map(|r| basis.iter().map(|v| dot(r, v)).collect()).collect()
        };
        let gn = proj(&g);
        let en = proj(e);
        let fnull: Vec<f64> = basis.iter().map(|v| dot(p.f, v)).collect();
        let zeros_g = vec![0.0; gn.len()];
        let zeros_e = vec![0.0; en.len()];
        let lbw = vec![-1.0; k];
        let ubw = vec![1.0; k];
        let rec = linprog(
            &Lp { c: &fnull, a: &gn, b: &zeros_g, aeq: &en, beq: &zeros_e, lb: &lbw, ub: &ubw },
            50_000,
        )?;
        let fscale = 1.0 + norm_inf(p.f);
        if rec.status == LpStatus::Optimal && rec.fval < -1e-9 * fscale {
            return Ok(bad(LpStatus::Unbounded, n, 0));
        }
    }

    // 2. solve
    let mut iterations = 0usize;
    let mut sol: Option<(Vec<f64>, Vec<f64>)> = None; // (x, z)
    if mi == 0 {
        // pure equality-constrained: one exact KKT solve when it is nonsingular
        if let Some((x, _)) = kkt_solve(&h, p.f, &[], &[], e, d, n) {
            sol = Some((x, vec![]));
            iterations = 1;
        }
    }
    let mut converged = sol.is_some();
    if sol.is_none() {
        let r = ipm(&h, p.f, &g, &hh, e, d, n, max_iter);
        iterations = r.iterations;
        converged = r.converged;
        sol = Some((r.x, r.z.clone()));
        // polish with the active set read off the multipliers
        if converged {
            let (x_ipm, z) = sol.clone().unwrap();
            let s: Vec<f64> = (0..mi).map(|i| hh[i] - dot(&g[i], &x_ipm)).collect();
            let active: Vec<usize> = (0..mi).filter(|&i| z[i] > s[i]).collect();
            let ga: Vec<Vec<f64>> = active.iter().map(|&i| g[i].clone()).collect();
            let ha: Vec<f64> = active.iter().map(|&i| hh[i]).collect();
            if let Some((xp, za)) = kkt_solve(&h, p.f, &ga, &ha, e, d, n) {
                let gscale = 1.0 + norm_inf(&hh);
                let feas = (0..mi).all(|i| dot(&g[i], &xp) <= hh[i] + 1e-9 * gscale);
                let mult_ok = za.iter().all(|v| *v >= -1e-9 * (1.0 + norm_inf(p.f)));
                if feas && mult_ok {
                    sol = Some((xp, z));
                }
            }
        }
    }
    let (x, _) = sol.unwrap();
    if !converged || x.iter().any(|v| !v.is_finite()) {
        return Ok(bad(LpStatus::IterationLimit, n, iterations));
    }
    let fval = objective(&h, p.f, &x);
    Ok(QpResult { x, fval, status: LpStatus::Optimal, iterations })
}

/// Solve the equality-constrained KKT system
/// `[H E' Ga'; E 0 0; Ga 0 0] [x; y; za] = [-f; d; ha]`; `None` if singular.
/// Returns `(x, za)`.
fn kkt_solve(
    h: &[Vec<f64>],
    f: &[f64],
    ga: &[Vec<f64>],
    ha: &[f64],
    e: &[Vec<f64>],
    d: &[f64],
    n: usize,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let (pe, ma) = (e.len(), ga.len());
    let sz = n + pe + ma;
    let mut k = vec![0.0; sz * sz];
    for i in 0..n {
        for j in 0..n {
            k[i * sz + j] = h[i][j];
        }
    }
    for (r, row) in e.iter().enumerate() {
        for j in 0..n {
            k[(n + r) * sz + j] = row[j];
            k[j * sz + n + r] = row[j];
        }
    }
    for (r, row) in ga.iter().enumerate() {
        for j in 0..n {
            k[(n + pe + r) * sz + j] = row[j];
            k[j * sz + n + pe + r] = row[j];
        }
    }
    let mut rhs = vec![0.0; sz];
    for i in 0..n {
        rhs[i] = -f[i];
    }
    rhs[n..n + pe].copy_from_slice(d);
    rhs[n + pe..].copy_from_slice(ha);
    let lu = Lu::new(&k, sz)?;
    let sol = lu.solve(&rhs);
    // an LU can "succeed" on a near-singular matrix; check the residual
    let mut worst = 0.0f64;
    for i in 0..sz {
        let mut s = -rhs[i];
        for j in 0..sz {
            s += k[i * sz + j] * sol[j];
        }
        worst = worst.max(s.abs());
    }
    if !(worst <= 1e-8 * (1.0 + norm_inf(&rhs) + norm_inf(&sol))) {
        return None;
    }
    Some((sol[..n].to_vec(), sol[n + pe..].to_vec()))
}

struct IpmOut {
    x: Vec<f64>,
    z: Vec<f64>,
    iterations: usize,
    converged: bool,
}

#[allow(clippy::too_many_arguments)]
fn ipm(
    h: &[Vec<f64>],
    f: &[f64],
    g: &[Vec<f64>],
    hh: &[f64],
    e: &[Vec<f64>],
    d: &[f64],
    n: usize,
    max_iter: usize,
) -> IpmOut {
    let (mi, pe) = (g.len(), e.len());
    let tol = 1e-9;
    let hnorm = h.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
    let reg_x = 1e-11 * (1.0 + hnorm);
    let reg_y = 1e-10 * (1.0 + hnorm);
    let mut x = vec![0.0; n];
    let mut y = vec![0.0; pe];
    let gx0 = mat_vec(g, &x);
    let mut s: Vec<f64> = (0..mi).map(|i| (hh[i] - gx0[i]).max(1.0)).collect();
    let mut z = vec![1.0; mi];
    let sz = n + pe;
    let fscale = 1.0 + norm_inf(f);
    let mut iterations = 0usize;
    let mut converged = false;
    let mut near = false;
    for _ in 0..max_iter {
        let hx = mat_vec(h, &x);
        let gx = mat_vec(g, &x);
        let ex = mat_vec(e, &x);
        let ety = mat_t_vec(e, n, &y);
        let gtz = mat_t_vec(g, n, &z);
        let rd: Vec<f64> = (0..n).map(|i| hx[i] + f[i] + ety[i] + gtz[i]).collect();
        let re: Vec<f64> = (0..pe).map(|i| ex[i] - d[i]).collect();
        let rp: Vec<f64> = (0..mi).map(|i| gx[i] + s[i] - hh[i]).collect();
        let mu = if mi > 0 { dot(&s, &z) / mi as f64 } else { 0.0 };
        let (nd, ne, np) = (norm_inf(&rd), norm_inf(&re), norm_inf(&rp));
        let pscale = 1.0 + norm_inf(hh);
        let escale = 1.0 + norm_inf(d);
        let dscale = fscale + norm_inf(&hx);
        near = nd <= 1e-6 * dscale && ne <= 1e-6 * escale && np <= 1e-6 * pscale && mu <= 1e-6;
        if nd <= tol * dscale && ne <= tol * escale && np <= tol * pscale && mu <= tol {
            converged = true;
            break;
        }
        iterations += 1;
        // reduced KKT matrix [H + G'(Z/S)G + rho I, E'; E, -delta I]
        let mut k = vec![0.0; sz * sz];
        for i in 0..n {
            for j in 0..n {
                k[i * sz + j] = h[i][j];
            }
            k[i * sz + i] += reg_x;
        }
        for (r, row) in g.iter().enumerate() {
            let w = z[r] / s[r];
            for i in 0..n {
                if row[i] != 0.0 {
                    for j in 0..n {
                        k[i * sz + j] += w * row[i] * row[j];
                    }
                }
            }
        }
        for (r, row) in e.iter().enumerate() {
            for j in 0..n {
                k[(n + r) * sz + j] = row[j];
                k[j * sz + n + r] = row[j];
            }
            k[(n + r) * sz + n + r] = -reg_y;
        }
        let lu = match Lu::new(&k, sz) {
            Some(l) => l,
            None => break,
        };
        let solve_dir = |rc: &[f64]| -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
            // w = (-rc + Z rp) / S
            let w: Vec<f64> = (0..mi).map(|i| (-rc[i] + z[i] * rp[i]) / s[i]).collect();
            let gtw = mat_t_vec(g, n, &w);
            let mut rhs = vec![0.0; sz];
            for i in 0..n {
                rhs[i] = -rd[i] - gtw[i];
            }
            for i in 0..pe {
                rhs[n + i] = -re[i];
            }
            let sol = lu.solve(&rhs);
            let dx = sol[..n].to_vec();
            let dy = sol[n..].to_vec();
            let gdx = mat_vec(g, &dx);
            let ds: Vec<f64> = (0..mi).map(|i| -rp[i] - gdx[i]).collect();
            let dz: Vec<f64> = (0..mi).map(|i| w[i] + z[i] / s[i] * gdx[i]).collect();
            (dx, dy, ds, dz)
        };
        let rc_aff: Vec<f64> = (0..mi).map(|i| s[i] * z[i]).collect();
        let (dx, dy, ds, dz) = if mi == 0 {
            solve_dir(&rc_aff)
        } else {
            let (_, _, dsa, dza) = solve_dir(&rc_aff);
            let ap = max_step(&s, &dsa);
            let ad = max_step(&z, &dza);
            let mu_aff = (0..mi).map(|i| (s[i] + ap * dsa[i]) * (z[i] + ad * dza[i])).sum::<f64>() / mi as f64;
            let sigma = if mu > 0.0 { (mu_aff / mu).powi(3).clamp(0.0, 1.0) } else { 0.0 };
            let rc: Vec<f64> = (0..mi).map(|i| s[i] * z[i] + dsa[i] * dza[i] - sigma * mu).collect();
            solve_dir(&rc)
        };
        let alpha = if mi == 0 { 1.0 } else { (0.995 * max_step(&s, &ds).min(max_step(&z, &dz))).min(1.0) };
        if !(alpha > 0.0) || dx.iter().chain(&dy).any(|v| !v.is_finite()) {
            break;
        }
        for i in 0..n {
            x[i] += alpha * dx[i];
        }
        for i in 0..pe {
            y[i] += alpha * dy[i];
        }
        for i in 0..mi {
            s[i] += alpha * ds[i];
            z[i] += alpha * dz[i];
            // keep strictly interior in the face of rounding
            s[i] = s[i].max(1e-300);
            z[i] = z[i].max(1e-300);
        }
        if norm_inf(&x) > 1e15 {
            break;
        }
    }
    IpmOut { x, z, iterations, converged: converged || near }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::small_dense::solve;

    const INF: f64 = f64::INFINITY;

    #[allow(clippy::too_many_arguments)]
    fn run(
        h: &[Vec<f64>],
        f: &[f64],
        a: &[Vec<f64>],
        b: &[f64],
        aeq: &[Vec<f64>],
        beq: &[f64],
        lb: &[f64],
        ub: &[f64],
    ) -> QpResult {
        quadprog(&Qp { h, f, a, b, aeq, beq, lb, ub }, 200).unwrap()
    }

    fn free(n: usize) -> (Vec<f64>, Vec<f64>) {
        (vec![-INF; n], vec![INF; n])
    }

    #[test]
    fn unconstrained_is_minus_h_inverse_f() {
        let h = vec![vec![4.0, 1.0], vec![1.0, 3.0]];
        let f = [1.0, 2.0];
        let (lb, ub) = free(2);
        let r = run(&h, &f, &[], &[], &[], &[], &lb, &ub);
        assert_eq!(r.status, LpStatus::Optimal);
        // independent: solve H x = -f by hand: det = 11, x = [-(3*1-1*2)/11, -(4*2-1*1)/11]
        assert!((r.x[0] + 1.0 / 11.0).abs() < 1e-12, "{:?}", r.x);
        assert!((r.x[1] + 7.0 / 11.0).abs() < 1e-12);
        assert!((r.fval - (0.5 * dot(&r.x, &[4.0 * r.x[0] + r.x[1], r.x[0] + 3.0 * r.x[1]]) + dot(&f, &r.x))).abs() < 1e-12);
    }

    #[test]
    fn box_constrained() {
        // min (x-3)^2 + (y+1)^2 over [0,2]x[0,2]: clamp -> (2, 0)
        let h = vec![vec![2.0, 0.0], vec![0.0, 2.0]];
        let f = [-6.0, 2.0];
        let r = run(&h, &f, &[], &[], &[], &[], &[0.0, 0.0], &[2.0, 2.0]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 2.0).abs() < 1e-9 && r.x[1].abs() < 1e-9, "{:?}", r.x);
        assert!((r.fval - (0.5 * (2.0 * 4.0) - 12.0)).abs() < 1e-8);
    }

    #[test]
    fn nocedal_wright_example_16_4() {
        // min (x1-1)^2 + (x2-2.5)^2 with five linear inequalities: (1.4, 1.7)
        let h = vec![vec![2.0, 0.0], vec![0.0, 2.0]];
        let f = [-2.0, -5.0];
        let a = vec![
            vec![-1.0, 2.0],  // x1 - 2 x2 + 2 >= 0
            vec![1.0, 2.0],   // -x1 - 2 x2 + 6 >= 0
            vec![1.0, -2.0],  // -x1 + 2 x2 + 2 >= 0
        ];
        let b = [2.0, 6.0, 2.0];
        let r = run(&h, &f, &a, &b, &[], &[], &[0.0, 0.0], &[INF, INF]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 1.4).abs() < 1e-8 && (r.x[1] - 1.7).abs() < 1e-8, "{:?}", r.x);
    }

    #[test]
    fn portfolio_with_known_inverse_variance_weights() {
        // min x' S x, S = diag(1, 4, 9), sum x = 1, x >= 0:  x_i ~ 1/S_ii
        let h = vec![vec![2.0, 0.0, 0.0], vec![0.0, 8.0, 0.0], vec![0.0, 0.0, 18.0]];
        let f = [0.0; 3];
        let r = run(&h, &f, &[], &[], &[vec![1.0; 3]], &[1.0], &[0.0; 3], &[INF; 3]);
        assert_eq!(r.status, LpStatus::Optimal);
        let tot = 1.0 + 0.25 + 1.0 / 9.0;
        for (xi, w) in r.x.iter().zip([1.0, 0.25, 1.0 / 9.0]) {
            assert!((xi - w / tot).abs() < 1e-9, "{:?}", r.x);
        }
        assert!((r.fval - 1.0 / tot).abs() < 1e-9);
    }

    #[test]
    fn portfolio_with_a_binding_long_only_constraint() {
        // asset 3 has such a high return term that the budget forces x3 = 0 at
        // the optimum: min x'Sx - 3 x1 with S = I (3 assets), sum x = 1, x >= 0
        // -> x = (1, 0, 0)  (unconstrained shift would be x1 = 1 + t, others negative)
        let h = vec![vec![2.0, 0.0, 0.0], vec![0.0, 2.0, 0.0], vec![0.0, 0.0, 2.0]];
        let f = [-3.0, 0.0, 0.0];
        let r = run(&h, &f, &[], &[], &[vec![1.0; 3]], &[1.0], &[0.0; 3], &[INF; 3]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 1.0).abs() < 1e-9 && r.x[1].abs() < 1e-9 && r.x[2].abs() < 1e-9, "{:?}", r.x);
    }

    #[test]
    fn equality_constrained_satisfies_the_kkt_system() {
        let h = vec![vec![2.0, 0.5, 0.0], vec![0.5, 3.0, 0.2], vec![0.0, 0.2, 1.0]];
        let f = [1.0, -2.0, 0.5];
        let aeq = vec![vec![1.0, 1.0, 1.0], vec![1.0, -1.0, 0.0]];
        let beq = [1.0, 0.25];
        let (lb, ub) = free(3);
        let r = run(&h, &f, &[], &[], &aeq, &beq, &lb, &ub);
        assert_eq!(r.status, LpStatus::Optimal);
        // constraints hold
        for (row, bi) in aeq.iter().zip(&beq) {
            assert!((dot(row, &r.x) - bi).abs() < 1e-10);
        }
        // KKT: H x + f lies in the row space of Aeq  <=>  H x + f = -Aeq' lambda
        let hx: Vec<f64> = h.iter().map(|row| dot(row, &r.x)).collect();
        let grad: Vec<f64> = (0..3).map(|i| hx[i] + f[i]).collect();
        // least-squares lambda via 2x2 normal equations, then residual ~ 0
        let g11 = dot(&aeq[0], &aeq[0]);
        let g12 = dot(&aeq[0], &aeq[1]);
        let g22 = dot(&aeq[1], &aeq[1]);
        let rhs = [-dot(&aeq[0], &grad), -dot(&aeq[1], &grad)];
        let lam = solve(&[g11, g12, g12, g22], 2, &rhs).unwrap();
        for i in 0..3 {
            assert!((grad[i] + aeq[0][i] * lam[0] + aeq[1][i] * lam[1]).abs() < 1e-10);
        }
    }

    #[test]
    fn singular_h_with_a_bounded_problem() {
        // H = diag(1, 0), f = (-1, 1), 0 <= x2 <= 1: x = (1, 0), value -0.5
        let h = vec![vec![1.0, 0.0], vec![0.0, 0.0]];
        let r = run(&h, &[-1.0, 1.0], &[], &[], &[], &[], &[-INF, 0.0], &[INF, 1.0]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 1.0).abs() < 1e-8 && r.x[1].abs() < 1e-8, "{:?}", r);
        assert!((r.fval + 0.5).abs() < 1e-8);
    }

    #[test]
    fn unbounded_along_the_null_space() {
        // H = diag(1, 0), f = (0, -1), x2 >= 0: x2 -> inf lowers the objective forever
        let h = vec![vec![1.0, 0.0], vec![0.0, 0.0]];
        let r = run(&h, &[0.0, -1.0], &[], &[], &[], &[], &[-INF, 0.0], &[INF, INF]);
        assert_eq!(r.status, LpStatus::Unbounded);
        // ...but not once x2 is capped
        let r = run(&h, &[0.0, -1.0], &[], &[], &[], &[], &[-INF, 0.0], &[INF, 5.0]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[1] - 5.0).abs() < 1e-8);
    }

    #[test]
    fn infeasible_constraints() {
        let h = vec![vec![2.0, 0.0], vec![0.0, 2.0]];
        // x1 + x2 <= -1 with x >= 0
        let r = run(&h, &[0.0, 0.0], &[vec![1.0, 1.0]], &[-1.0], &[], &[], &[0.0, 0.0], &[INF, INF]);
        assert_eq!(r.status, LpStatus::Infeasible);
        assert!(r.x.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn indefinite_and_asymmetric_h_are_errors() {
        let (lb, ub) = free(2);
        let ind = vec![vec![1.0, 0.0], vec![0.0, -1.0]];
        let e = quadprog(&Qp { h: &ind, f: &[0.0, 0.0], a: &[], b: &[], aeq: &[], beq: &[], lb: &lb, ub: &ub }, 50)
            .unwrap_err();
        assert!(e.contains("not positive semidefinite"), "{e}");
        let asym = vec![vec![1.0, 2.0], vec![0.0, 1.0]];
        let e = quadprog(&Qp { h: &asym, f: &[0.0, 0.0], a: &[], b: &[], aeq: &[], beq: &[], lb: &lb, ub: &ub }, 50)
            .unwrap_err();
        assert!(e.contains("symmetric"), "{e}");
        let wrong = vec![vec![1.0]];
        let e = quadprog(&Qp { h: &wrong, f: &[0.0, 0.0], a: &[], b: &[], aeq: &[], beq: &[], lb: &lb, ub: &ub }, 50)
            .unwrap_err();
        assert!(e.contains("2x2"), "{e}");
    }

    #[test]
    fn random_general_qps_satisfy_the_kkt_conditions() {
        // Random convex QPs (a third with a rank-deficient H) with general
        // inequality rows, built feasible by construction. The answer is
        // verified by KKT conditions computed here: feasibility, and that the
        // gradient is a NON-NEGATIVE combination of the active rows
        // (multipliers from a non-negative least-squares fit).
        use crate::linalg::nnls;
        use crate::matrix::Matrix;
        let mut seed = 777u64;
        let mut lcg = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut with_active = 0;
        for case in 0..90 {
            let n = 3 + case % 3;
            let rank = if case % 3 == 2 { n - 1 } else { n };
            let mm: Vec<Vec<f64>> = (0..rank).map(|_| (0..n).map(|_| lcg() - 0.5).collect()).collect();
            let h: Vec<Vec<f64>> = (0..n)
                .map(|i| (0..n).map(|j| (0..rank).map(|k| mm[k][i] * mm[k][j]).sum::<f64>()).collect())
                .collect();
            let f: Vec<f64> = (0..n).map(|_| (lcg() - 0.5) * 4.0).collect();
            let m = 3 + case % 4;
            let a: Vec<Vec<f64>> = (0..m).map(|_| (0..n).map(|_| lcg() * 2.0 - 1.0).collect()).collect();
            let xf: Vec<f64> = (0..n).map(|_| lcg() - 0.5).collect();
            let b: Vec<f64> = a.iter().map(|r| dot(r, &xf) + 0.05 + lcg()).collect();
            // keep it bounded even when H is singular
            let lb = vec![-3.0; n];
            let ub = vec![3.0; n];
            let r = quadprog(&Qp { h: &h, f: &f, a: &a, b: &b, aeq: &[], beq: &[], lb: &lb, ub: &ub }, 200).unwrap();
            assert_eq!(r.status, LpStatus::Optimal, "case {case}");
            for (row, bi) in a.iter().zip(&b) {
                assert!(dot(row, &r.x) <= bi + 1e-7, "case {case}: infeasible");
            }
            assert!(r.x.iter().all(|v| *v >= -3.0 - 1e-7 && *v <= 3.0 + 1e-7));
            // gradient g = H x + f; KKT: g = -sum lambda_i * row_i, lambda >= 0
            let g: Vec<f64> = (0..n).map(|i| dot(&h[i], &r.x) + f[i]).collect();
            let mut rows: Vec<Vec<f64>> = Vec::new();
            for (row, bi) in a.iter().zip(&b) {
                if bi - dot(row, &r.x) < 1e-6 {
                    rows.push(row.clone());
                }
            }
            for i in 0..n {
                let mut e = vec![0.0; n];
                if r.x[i] <= -3.0 + 1e-6 {
                    e[i] = -1.0;
                    rows.push(e);
                } else if r.x[i] >= 3.0 - 1e-6 {
                    e[i] = 1.0;
                    rows.push(e);
                }
            }
            if rows.is_empty() {
                let gn = g.iter().fold(0.0f64, |mx, v| mx.max(v.abs()));
                assert!(gn < 1e-7, "case {case}: interior point with gradient {gn}");
                continue;
            }
            with_active += 1;
            // columns = active rows; solve  [rows'] lambda ~= -g
            let mut cols = Vec::new();
            for r_ in &rows {
                cols.extend_from_slice(r_);
            }
            let am = Matrix::from_col_major(n, rows.len(), cols);
            let neg_g: Vec<f64> = g.iter().map(|v| -v).collect();
            let sol = nnls(&am, &neg_g, None).unwrap();
            let lam = sol.x.as_slice().to_vec();
            let mut resid = 0.0f64;
            for i in 0..n {
                let s: f64 = (0..rows.len()).map(|k| rows[k][i] * lam[k]).sum();
                resid = resid.max((s + g[i]).abs());
            }
            assert!(resid < 1e-6, "case {case}: KKT residual {resid}");
        }
        assert!(with_active > 30, "only {with_active} cases had an active constraint");
    }

    #[test]
    fn random_box_qps_match_projected_gradient_optimality() {
        // For min 0.5 x'Hx + f'x on a box the solution is characterised by
        // x = clamp(x - t (Hx + f)) for any t > 0 (first-order optimality).
        let mut seed = 99u64;
        let mut lcg = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        for case in 0..60 {
            let n = 2 + case % 4;
            let m: Vec<Vec<f64>> = (0..n).map(|_| (0..n).map(|_| lcg() - 0.5).collect()).collect();
            // H = M'M + 0.1 I  (symmetric, PD)
            let h: Vec<Vec<f64>> = (0..n)
                .map(|i| {
                    (0..n)
                        .map(|j| (0..n).map(|k| m[k][i] * m[k][j]).sum::<f64>() + if i == j { 0.1 } else { 0.0 })
                        .collect()
                })
                .collect();
            let f: Vec<f64> = (0..n).map(|_| (lcg() - 0.5) * 6.0).collect();
            let lb = vec![-1.0; n];
            let ub = vec![1.0; n];
            let r = quadprog(&Qp { h: &h, f: &f, a: &[], b: &[], aeq: &[], beq: &[], lb: &lb, ub: &ub }, 200).unwrap();
            assert_eq!(r.status, LpStatus::Optimal, "case {case}");
            let grad: Vec<f64> = (0..n).map(|i| dot(&h[i], &r.x) + f[i]).collect();
            for i in 0..n {
                let proj = (r.x[i] - 0.7 * grad[i]).clamp(-1.0, 1.0);
                assert!((proj - r.x[i]).abs() < 1e-7, "case {case}: x={:?} grad={grad:?}", r.x);
            }
        }
    }
}
