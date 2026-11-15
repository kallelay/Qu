//! `linprog`: linear programming by the two-phase tableau simplex method.
//!
//! ```text
//! minimise  c'x   subject to   A x <= b,  Aeq x = beq,  lb <= x <= ub
//! ```
//!
//! Bounds default to `lb = -inf`, `ub = +inf` (MATLAB's convention), so the
//! textbook standard form `x >= 0` needs `lb = 0` to be passed explicitly.
//!
//! The problem is first rewritten as `min c'z, A z = b, z >= 0`:
//! a variable with a finite lower bound becomes `x = lb + z`, one with only an
//! upper bound `x = ub - z`, a free one `x = z+ - z-`; a finite pair adds the
//! row `z <= ub - lb`. Inequalities get slack variables. Phase I minimises
//! the sum of artificial variables to find a feasible basis (or prove there
//! is none), phase II optimises.
//!
//! Pivoting uses Dantzig's rule (most negative reduced cost) and switches to
//! Bland's smallest-index rule whenever the objective has stopped improving
//! for a while, which is what makes cycling on degenerate vertices
//! impossible; it returns to Dantzig as soon as progress resumes. The
//! iteration count is capped (`max_iter`), never unbounded.
//!
//! Dense tableau, `O(rows * columns)` per pivot: made for the small and
//! medium problems a lab script poses (hundreds of variables), not for
//! thousand-by-thousand sparse LPs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LpStatus {
    Optimal,
    Infeasible,
    Unbounded,
    IterationLimit,
}

impl LpStatus {
    pub fn name(self) -> &'static str {
        match self {
            LpStatus::Optimal => "optimal",
            LpStatus::Infeasible => "infeasible",
            LpStatus::Unbounded => "unbounded",
            LpStatus::IterationLimit => "iteration_limit",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LpResult {
    /// The minimiser; all NaN unless `status` is `Optimal`.
    pub x: Vec<f64>,
    /// `c'x`; NaN when infeasible or at the iteration limit, `-inf` when unbounded.
    pub fval: f64,
    pub status: LpStatus,
    pub iterations: usize,
}

pub struct Lp<'a> {
    pub c: &'a [f64],
    pub a: &'a [Vec<f64>],
    pub b: &'a [f64],
    pub aeq: &'a [Vec<f64>],
    pub beq: &'a [f64],
    /// length `n`; `-inf` allowed
    pub lb: &'a [f64],
    /// length `n`; `+inf` allowed
    pub ub: &'a [f64],
}

/// Check shapes and finiteness of an `A x <= b` / `Aeq x = beq` / bounds
/// problem of `n` variables. Shared with `quadprog`.
pub fn validate_constraints(
    who: &str,
    n: usize,
    a: &[Vec<f64>],
    b: &[f64],
    aeq: &[Vec<f64>],
    beq: &[f64],
    lb: &[f64],
    ub: &[f64],
) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{who}: A has {} rows but b has {} entries", a.len(), b.len()));
    }
    if aeq.len() != beq.len() {
        return Err(format!("{who}: Aeq has {} rows but beq has {} entries", aeq.len(), beq.len()));
    }
    for (name, rows) in [("A", a), ("Aeq", aeq)] {
        for (i, r) in rows.iter().enumerate() {
            if r.len() != n {
                return Err(format!("{who}: {name} row {} has {} columns but there are {n} variables", i + 1, r.len()));
            }
            if r.iter().any(|v| !v.is_finite()) {
                return Err(format!("{who}: {name} must be finite (row {} has NaN or Inf)", i + 1));
            }
        }
    }
    for (name, v) in [("b", b), ("beq", beq)] {
        if v.iter().any(|x| !x.is_finite()) {
            return Err(format!("{who}: {name} must be finite (it contains NaN or Inf)"));
        }
    }
    if lb.len() != n || ub.len() != n {
        return Err(format!("{who}: lb and ub must have {n} entries (one per variable)"));
    }
    if lb.iter().chain(ub).any(|v| v.is_nan()) {
        return Err(format!("{who}: lb and ub must not contain NaN"));
    }
    if lb.iter().any(|v| *v == f64::INFINITY) || ub.iter().any(|v| *v == f64::NEG_INFINITY) {
        return Err(format!("{who}: lb may not be +Inf and ub may not be -Inf"));
    }
    Ok(())
}

enum Map {
    Shift(usize, f64),
    Neg(usize, f64),
    Free(usize, usize),
}

enum Outcome {
    Optimal,
    Unbounded,
    Limit,
}

fn pivot(t: &mut [Vec<f64>], basis: &mut [usize], r: usize, j: usize) {
    let piv = t[r][j];
    for v in t[r].iter_mut() {
        *v /= piv;
    }
    t[r][j] = 1.0;
    let pr = t[r].clone();
    for (i, row) in t.iter_mut().enumerate() {
        if i == r {
            continue;
        }
        let f = row[j];
        if f != 0.0 {
            for (v, p) in row.iter_mut().zip(&pr) {
                *v -= f * p;
            }
            row[j] = 0.0;
        }
    }
    basis[r] = j;
}

fn simplex(
    t: &mut [Vec<f64>],
    ntot: usize,
    basis: &mut [usize],
    cost: &[f64],
    allowed: usize,
    max_iter: usize,
    iters: &mut usize,
) -> Outcome {
    const DTOL: f64 = 1e-9;
    const PTOL: f64 = 1e-9;
    let m = t.len();
    let mut last_obj = f64::INFINITY;
    let mut stall = 0usize;
    loop {
        let mut is_basic = vec![false; ntot];
        for &bj in basis.iter() {
            is_basic[bj] = true;
        }
        let mut obj = 0.0;
        for i in 0..m {
            obj += cost[basis[i]] * t[i][ntot];
        }
        if obj < last_obj - 1e-12 * (1.0 + obj.abs()) {
            last_obj = obj;
            stall = 0;
        } else {
            stall += 1;
        }
        let bland = stall > 10 + m;
        // entering column
        let mut enter: Option<usize> = None;
        let mut best = -DTOL;
        for j in 0..allowed {
            if is_basic[j] {
                continue;
            }
            let mut d = cost[j];
            for i in 0..m {
                d -= cost[basis[i]] * t[i][j];
            }
            if bland {
                if d < -DTOL {
                    enter = Some(j);
                    break;
                }
            } else if d < best {
                best = d;
                enter = Some(j);
            }
        }
        let j = match enter {
            Some(j) => j,
            None => return Outcome::Optimal,
        };
        // ratio test
        let mut minratio = f64::INFINITY;
        for i in 0..m {
            if t[i][j] > PTOL {
                let r = t[i][ntot].max(0.0) / t[i][j];
                if r < minratio {
                    minratio = r;
                }
            }
        }
        if !minratio.is_finite() {
            return Outcome::Unbounded;
        }
        let cutoff = minratio + 1e-12 * (1.0 + minratio);
        let mut leave: Option<usize> = None;
        for i in 0..m {
            if t[i][j] > PTOL && t[i][ntot].max(0.0) / t[i][j] <= cutoff {
                leave = match leave {
                    None => Some(i),
                    Some(cur) => {
                        let better = if bland { basis[i] < basis[cur] } else { t[i][j] > t[cur][j] };
                        if better {
                            Some(i)
                        } else {
                            Some(cur)
                        }
                    }
                };
            }
        }
        if *iters >= max_iter {
            return Outcome::Limit;
        }
        *iters += 1;
        pivot(t, basis, leave.unwrap(), j);
    }
}

fn nan_result(status: LpStatus, n: usize, iterations: usize) -> LpResult {
    LpResult {
        x: vec![f64::NAN; n],
        fval: if status == LpStatus::Unbounded { f64::NEG_INFINITY } else { f64::NAN },
        status,
        iterations,
    }
}

/// Solve the LP. Errors are for malformed input only; infeasible and
/// unbounded problems are reported through [`LpResult::status`].
pub fn linprog(p: &Lp, max_iter: usize) -> Result<LpResult, String> {
    let n = p.c.len();
    if n == 0 {
        return Err("linprog: c must have at least one element".into());
    }
    if p.c.iter().any(|v| !v.is_finite()) {
        return Err("linprog: c must be finite (it contains NaN or Inf)".into());
    }
    validate_constraints("linprog", n, p.a, p.b, p.aeq, p.beq, p.lb, p.ub)?;
    for i in 0..n {
        if p.lb[i] > p.ub[i] {
            return Ok(nan_result(LpStatus::Infeasible, n, 0));
        }
    }
    // --- variable substitution
    let mut maps = Vec::with_capacity(n);
    let mut nz = 0usize;
    for i in 0..n {
        if p.lb[i].is_finite() {
            maps.push(Map::Shift(nz, p.lb[i]));
            nz += 1;
        } else if p.ub[i].is_finite() {
            maps.push(Map::Neg(nz, p.ub[i]));
            nz += 1;
        } else {
            maps.push(Map::Free(nz, nz + 1));
            nz += 2;
        }
    }
    let expand = |row: &[f64]| -> (Vec<f64>, f64) {
        let mut z = vec![0.0; nz];
        let mut off = 0.0;
        for i in 0..n {
            match maps[i] {
                Map::Shift(c, l) => {
                    z[c] += row[i];
                    off += row[i] * l;
                }
                Map::Neg(c, u) => {
                    z[c] -= row[i];
                    off += row[i] * u;
                }
                Map::Free(cp, cn) => {
                    z[cp] += row[i];
                    z[cn] -= row[i];
                }
            }
        }
        (z, off)
    };
    // rows: (z coefficients, rhs, is_inequality)
    let mut rows: Vec<(Vec<f64>, f64, bool)> = Vec::new();
    for (r, &bi) in p.a.iter().zip(p.b) {
        let (z, off) = expand(r);
        rows.push((z, bi - off, true));
    }
    for (r, &bi) in p.aeq.iter().zip(p.beq) {
        let (z, off) = expand(r);
        rows.push((z, bi - off, false));
    }
    for i in 0..n {
        if let Map::Shift(c, l) = maps[i] {
            if p.ub[i].is_finite() {
                let mut z = vec![0.0; nz];
                z[c] = 1.0;
                rows.push((z, p.ub[i] - l, true));
            }
        }
    }
    let (cz, coff) = expand(p.c);
    let m = rows.len();
    let ns = rows.iter().filter(|r| r.2).count();
    let ncols = nz + ns;
    // --- tableau with artificials where no slack can serve as the basis
    let mut slack_of = vec![usize::MAX; m];
    let mut next = nz;
    for (i, r) in rows.iter().enumerate() {
        if r.2 {
            slack_of[i] = next;
            next += 1;
        }
    }
    let mut need_art = vec![false; m];
    let mut dense: Vec<Vec<f64>> = Vec::with_capacity(m);
    for (i, (z, rhs, ineq)) in rows.iter().enumerate() {
        let mut row = vec![0.0; ncols + 1];
        row[..nz].copy_from_slice(z);
        if *ineq {
            row[slack_of[i]] = 1.0;
        }
        row[ncols] = *rhs;
        if *rhs < 0.0 {
            for v in row.iter_mut() {
                *v = -*v;
            }
            need_art[i] = true;
        } else if !*ineq {
            need_art[i] = true;
        }
        dense.push(row);
    }
    let nart = need_art.iter().filter(|b| **b).count();
    let ntot = ncols + nart;
    let mut t: Vec<Vec<f64>> = Vec::with_capacity(m);
    let mut basis = vec![0usize; m];
    let mut a_idx = ncols;
    for (i, row) in dense.into_iter().enumerate() {
        let mut full = vec![0.0; ntot + 1];
        full[..ncols].copy_from_slice(&row[..ncols]);
        full[ntot] = row[ncols];
        if need_art[i] {
            full[a_idx] = 1.0;
            basis[i] = a_idx;
            a_idx += 1;
        } else {
            basis[i] = slack_of[i];
        }
        t.push(full);
    }
    let mut iters = 0usize;
    // --- phase I
    if nart > 0 {
        let mut c1 = vec![0.0; ntot];
        for v in c1.iter_mut().skip(ncols) {
            *v = 1.0;
        }
        match simplex(&mut t, ntot, &mut basis, &c1, ncols, max_iter, &mut iters) {
            Outcome::Limit => return Ok(nan_result(LpStatus::IterationLimit, n, iters)),
            Outcome::Unbounded | Outcome::Optimal => {}
        }
        let bnorm = rows.iter().fold(0.0f64, |mx, r| mx.max(r.1.abs()));
        let infeas: f64 = (0..m).filter(|&i| basis[i] >= ncols).map(|i| t[i][ntot].max(0.0)).sum();
        if infeas > 1e-8 * (1.0 + bnorm) {
            return Ok(nan_result(LpStatus::Infeasible, n, iters));
        }
        // drive remaining (zero-level) artificials out of the basis
        for i in 0..m {
            if basis[i] >= ncols {
                let mut best = 1e-9;
                let mut col = None;
                for j in 0..ncols {
                    if t[i][j].abs() > best {
                        best = t[i][j].abs();
                        col = Some(j);
                    }
                }
                if let Some(j) = col {
                    pivot(&mut t, &mut basis, i, j);
                }
                // else: the row is redundant (all zero); it stays inert
            }
        }
    }
    // --- phase II
    let mut c2 = vec![0.0; ntot];
    c2[..nz].copy_from_slice(&cz);
    match simplex(&mut t, ntot, &mut basis, &c2, ncols, max_iter, &mut iters) {
        Outcome::Limit => return Ok(nan_result(LpStatus::IterationLimit, n, iters)),
        Outcome::Unbounded => return Ok(nan_result(LpStatus::Unbounded, n, iters)),
        Outcome::Optimal => {}
    }
    let mut z = vec![0.0; ntot];
    for i in 0..m {
        z[basis[i]] = t[i][ntot].max(0.0);
    }
    let x: Vec<f64> = (0..n)
        .map(|i| match maps[i] {
            Map::Shift(c, l) => l + z[c],
            Map::Neg(c, u) => u - z[c],
            Map::Free(cp, cn) => z[cp] - z[cn],
        })
        .collect();
    let _ = coff;
    let fval: f64 = p.c.iter().zip(&x).map(|(a, b)| a * b).sum();
    Ok(LpResult { x, fval, status: LpStatus::Optimal, iterations: iters })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::small_dense::solve;

    const INF: f64 = f64::INFINITY;

    fn run(
        c: &[f64],
        a: &[Vec<f64>],
        b: &[f64],
        aeq: &[Vec<f64>],
        beq: &[f64],
        lb: &[f64],
        ub: &[f64],
    ) -> LpResult {
        linprog(&Lp { c, a, b, aeq, beq, lb, ub }, 10_000).unwrap()
    }

    #[test]
    fn textbook_production_problem() {
        // max 3x + 5y  s.t. x <= 4, 2y <= 12, 3x + 2y <= 18, x,y >= 0  -> (2, 6), 36
        let r = run(
            &[-3.0, -5.0],
            &[vec![1.0, 0.0], vec![0.0, 2.0], vec![3.0, 2.0]],
            &[4.0, 12.0, 18.0],
            &[],
            &[],
            &[0.0, 0.0],
            &[INF, INF],
        );
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 2.0).abs() < 1e-9 && (r.x[1] - 6.0).abs() < 1e-9, "{:?}", r.x);
        assert!((r.fval + 36.0).abs() < 1e-9);
    }

    #[test]
    fn beale_cycling_example_terminates_at_minus_five_quarters() {
        // Beale (1955): the textbook LP on which Dantzig's rule with a naive
        // tie-break cycles forever. Optimum -5/4 at (1, 0, 1, 0).
        let r = run(
            &[-0.75, 20.0, -0.5, 6.0],
            &[vec![0.25, -8.0, -1.0, 9.0], vec![0.5, -12.0, -0.5, 3.0], vec![0.0, 0.0, 1.0, 0.0]],
            &[0.0, 0.0, 1.0],
            &[],
            &[],
            &[0.0; 4],
            &[INF; 4],
        );
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.fval + 1.25).abs() < 1e-9, "{:?}", r);
        assert!((r.x[0] - 1.0).abs() < 1e-9 && (r.x[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn infeasible_problem() {
        // x + y <= 1 and x + y >= 3 (as -x - y <= -3)
        let r = run(
            &[1.0, 1.0],
            &[vec![1.0, 1.0], vec![-1.0, -1.0]],
            &[1.0, -3.0],
            &[],
            &[],
            &[0.0, 0.0],
            &[INF, INF],
        );
        assert_eq!(r.status, LpStatus::Infeasible);
        assert!(r.x.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn contradictory_bounds_are_infeasible() {
        let r = run(&[1.0], &[], &[], &[], &[], &[2.0], &[1.0]);
        assert_eq!(r.status, LpStatus::Infeasible);
    }

    #[test]
    fn unbounded_problem() {
        // min -x - y, x - y <= 1, x,y >= 0: ray (t, t)
        let r = run(&[-1.0, -1.0], &[vec![1.0, -1.0]], &[1.0], &[], &[], &[0.0, 0.0], &[INF, INF]);
        assert_eq!(r.status, LpStatus::Unbounded);
        assert_eq!(r.fval, f64::NEG_INFINITY);
    }

    #[test]
    fn free_variables_default_bounds_are_minus_inf() {
        // min x s.t. x >= -5 written as -x <= 5, x free: optimum -5
        let r = run(&[1.0], &[vec![-1.0]], &[5.0], &[], &[], &[-INF], &[INF]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] + 5.0).abs() < 1e-9);
        // and with no constraint at all it is unbounded
        let r = run(&[1.0], &[], &[], &[], &[], &[-INF], &[INF]);
        assert_eq!(r.status, LpStatus::Unbounded);
    }

    #[test]
    fn equality_constrained() {
        // min x + 2y + 3z  s.t. x + y + z = 10, x - y = 2, x,y,z >= 0
        // y = x - 2; 2x - 2 + z = 10 -> z = 12 - 2x >= 0 -> x <= 6, x >= 2
        // cost = x + 2x - 4 + 36 - 6x = 32 - 3x: minimum at x = 6: y = 4, z = 0, 14
        let r = run(
            &[1.0, 2.0, 3.0],
            &[],
            &[],
            &[vec![1.0, 1.0, 1.0], vec![1.0, -1.0, 0.0]],
            &[10.0, 2.0],
            &[0.0; 3],
            &[INF; 3],
        );
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.fval - 14.0).abs() < 1e-9, "{:?}", r);
        assert!((r.x[0] - 6.0).abs() < 1e-9 && (r.x[1] - 4.0).abs() < 1e-9 && r.x[2].abs() < 1e-9);
    }

    #[test]
    fn redundant_equalities_are_fine() {
        // the second equality is twice the first
        let r = run(
            &[1.0, 1.0],
            &[],
            &[],
            &[vec![1.0, 1.0], vec![2.0, 2.0]],
            &[3.0, 6.0],
            &[0.0, 0.0],
            &[INF, INF],
        );
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.fval - 3.0).abs() < 1e-9);
    }

    #[test]
    fn upper_and_negative_lower_bounds() {
        // min -x - y with -2 <= x <= 3, y <= 4 (no lower bound): optimum (3, 4)
        let r = run(&[-1.0, -1.0], &[], &[], &[], &[], &[-2.0, -INF], &[3.0, 4.0]);
        assert_eq!(r.status, LpStatus::Optimal);
        assert!((r.x[0] - 3.0).abs() < 1e-12 && (r.x[1] - 4.0).abs() < 1e-12);
        // min x + y with the same bounds: x = -2 and y unbounded below
        let r = run(&[1.0, 1.0], &[], &[], &[], &[], &[-2.0, -INF], &[3.0, 4.0]);
        assert_eq!(r.status, LpStatus::Unbounded);
    }

    #[test]
    fn bad_input_is_an_error() {
        let e = |c: &[f64], a: &[Vec<f64>], b: &[f64]| {
            let n = c.len();
            let lb = vec![0.0; n];
            let ub = vec![INF; n];
            linprog(&Lp { c, a, b, aeq: &[], beq: &[], lb: &lb, ub: &ub }, 100).unwrap_err()
        };
        assert!(e(&[1.0, 2.0], &[vec![1.0]], &[1.0]).contains("1 columns but there are 2"));
        assert!(e(&[1.0], &[vec![1.0]], &[1.0, 2.0]).contains("1 rows but b has 2"));
        assert!(e(&[f64::NAN], &[], &[]).contains("c must be finite"));
        assert!(e(&[1.0], &[vec![f64::NAN]], &[1.0]).contains("finite"));
        assert!(e(&[], &[], &[]).contains("at least one"));
    }

    #[test]
    fn iteration_limit_is_reported() {
        let r = linprog(
            &Lp {
                c: &[-3.0, -5.0],
                a: &[vec![1.0, 0.0], vec![0.0, 2.0], vec![3.0, 2.0]],
                b: &[4.0, 12.0, 18.0],
                aeq: &[],
                beq: &[],
                lb: &[0.0, 0.0],
                ub: &[INF, INF],
            },
            1,
        )
        .unwrap();
        assert_eq!(r.status, LpStatus::IterationLimit);
    }

    // --- brute-force cross-check: enumerate every vertex of a small boxed LP
    fn lcg(s: &mut u64) -> f64 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*s >> 33) as f64) / (1u64 << 31) as f64
    }

    fn brute(c: &[f64], a: &[Vec<f64>], b: &[f64], ubox: f64) -> Option<f64> {
        let n = c.len();
        // all inequality rows: A x <= b, -x_i <= 0, x_i <= ubox
        let mut g: Vec<(Vec<f64>, f64)> = a.iter().cloned().zip(b.iter().copied()).collect();
        for i in 0..n {
            let mut r = vec![0.0; n];
            r[i] = -1.0;
            g.push((r.clone(), 0.0));
            r[i] = 1.0;
            g.push((r, ubox));
        }
        let mut best: Option<f64> = None;
        let k = g.len();
        let mut idx: Vec<usize> = (0..n).collect();
        loop {
            let mut mat = Vec::new();
            let mut rhs = Vec::new();
            for &i in &idx {
                mat.extend_from_slice(&g[i].0);
                rhs.push(g[i].1);
            }
            if let Some(x) = solve(&mat, n, &rhs) {
                if g.iter().all(|(r, bi)| r.iter().zip(&x).map(|(p, q)| p * q).sum::<f64>() <= bi + 1e-9) {
                    let v: f64 = c.iter().zip(&x).map(|(p, q)| p * q).sum();
                    best = Some(best.map_or(v, |b0: f64| b0.min(v)));
                }
            }
            // next combination
            let mut p = n;
            while p > 0 && idx[p - 1] == k - n + p - 1 {
                p -= 1;
            }
            if p == 0 {
                break;
            }
            idx[p - 1] += 1;
            for q in p..n {
                idx[q] = idx[q - 1] + 1;
            }
        }
        best
    }

    #[test]
    fn random_small_lps_match_vertex_enumeration() {
        let mut seed = 12345u64;
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..400 {
            let n = 2 + (case % 3); // 2, 3, 4 variables
            let m = 2 + (case % 4);
            let ubox = 10.0;
            let c: Vec<f64> = (0..n).map(|_| (lcg(&mut seed) * 10.0).round() - 5.0).collect();
            let a: Vec<Vec<f64>> = (0..m)
                .map(|_| (0..n).map(|_| (lcg(&mut seed) * 8.0).round() - 3.0).collect())
                .collect();
            // mix of loose and tight right-hand sides, some negative
            let b: Vec<f64> = (0..m).map(|_| (lcg(&mut seed) * 30.0).round() - 4.0).collect();
            let want = brute(&c, &a, &b, ubox);
            let lb = vec![0.0; n];
            let ub = vec![ubox; n];
            let r = linprog(&Lp { c: &c, a: &a, b: &b, aeq: &[], beq: &[], lb: &lb, ub: &ub }, 10_000).unwrap();
            match want {
                None => {
                    infeasible += 1;
                    assert_eq!(r.status, LpStatus::Infeasible, "case {case}: c={c:?} a={a:?} b={b:?}");
                }
                Some(v) => {
                    feasible += 1;
                    assert_eq!(r.status, LpStatus::Optimal, "case {case}: c={c:?} a={a:?} b={b:?}");
                    assert!((r.fval - v).abs() < 1e-7 * (1.0 + v.abs()), "case {case}: {} vs {v}", r.fval);
                    // and the returned point is itself feasible
                    for (row, bi) in a.iter().zip(&b) {
                        assert!(row.iter().zip(&r.x).map(|(p, q)| p * q).sum::<f64>() <= bi + 1e-7);
                    }
                    assert!(r.x.iter().all(|v| *v >= -1e-9 && *v <= ubox + 1e-9));
                }
            }
        }
        // the sample must actually exercise both outcomes
        assert!(feasible > 100 && infeasible > 10, "feasible={feasible} infeasible={infeasible}");
    }
}
