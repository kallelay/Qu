//! `bvp`: two-point boundary value problems `y' = f(x, y)`, `bc(y(a), y(b)) = 0`.
//!
//! The scheme is the one behind SciPy's `solve_bvp` and MATLAB's `bvp4c`: a
//! fourth-order collocation method (Lobatto IIIA, equivalently cubic Hermite
//! collocation at the interval midpoints). On every mesh interval `[x_i, x_i+1]`
//! of width `h` the collocation equation is
//!
//! ```text
//! y_{i+1} - y_i - h/6 (f_i + 4 f_mid + f_{i+1}) = 0
//! y_mid = (y_i + y_{i+1})/2 - h/8 (f_{i+1} - f_i)
//! ```
//!
//! and together with the `n` boundary conditions gives `n * N` equations for
//! the `n * N` unknowns. They are solved by a damped Newton iteration (Armijo
//! backtracking) with a finite-difference Jacobian. The mesh is refined from
//! an estimate of the true residual `y_S'(x) - f(x, y_S(x))` of the cubic
//! Hermite interpolant `y_S` (5-point Lobatto quadrature of the squared,
//! `1 + |f|` normalised residual), exactly as SciPy does, so the stopping test
//! `max residual < tol` is a statement about the continuous solution and not
//! only about the algebraic system.
//!
//! The collocation Jacobian is almost block diagonal with the boundary rows
//! coupling the two ends of the mesh. Unknowns are therefore ordered
//! `y_0, y_{N-1}, y_1, y_{N-2}, ...` ("folded"), which makes the whole matrix
//! banded with a half-bandwidth of a few `n`, and it is solved by a banded LU
//! with partial pivoting. A solve costs `O(N n^3)`, so a mesh of thousands of
//! nodes is cheap.
//!
//! Every loop is bounded: Newton does at most [`NEWTON_MAX_ITER`] iterations
//! per mesh, the mesh grows by at least one node per pass up to `max_nodes`,
//! and the number of calls of `f` is capped by [`MAX_FEV`].

pub type RhsFn<'a> = &'a mut dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, String>;
pub type BcFn<'a> = &'a mut dyn FnMut(&[f64], &[f64]) -> Result<Vec<f64>, String>;

/// Newton iterations per mesh.
pub const NEWTON_MAX_ITER: usize = 12;
/// Cap on calls of the right-hand side over the whole solve.
pub const MAX_FEV: usize = 20_000_000;
/// Cap on refinement passes.
const MAX_PASSES: usize = 200;
/// A converged-looking solution whose collocation Jacobian has a pivot below
/// this fraction of its largest entry is reported as singular. A problem with
/// no solution (`y'' + y = 0`, `y(0) = 0`, `y(pi) = 1`) has a discrete
/// "solution" of size `h^-4` whose normalised residual is nonetheless tiny;
/// well-posed problems measured here stay above 0.1, stiff ones that resolve
/// their layers above 1e-2.
const SINGULAR_PIVOT_RATIO: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BvpStatus {
    Converged,
    /// The mesh would have to exceed `max_nodes`.
    MaxNodes,
    /// The collocation Jacobian was (numerically) singular.
    Singular,
    /// Newton or the refinement loop did not reach the tolerance.
    NoConvergence,
}

impl BvpStatus {
    pub fn name(self) -> &'static str {
        match self {
            BvpStatus::Converged => "converged",
            BvpStatus::MaxNodes => "max_nodes",
            BvpStatus::Singular => "singular",
            BvpStatus::NoConvergence => "no_convergence",
        }
    }
}

/// Solution on the final mesh. `y` and `yp` are node-major: entry `k` of
/// node `i` is `y[i * n + k]` (which is a column-major `n x N` matrix).
#[derive(Debug, Clone)]
pub struct BvpResult {
    pub n: usize,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub yp: Vec<f64>,
    pub status: BvpStatus,
    /// Largest normalised residual of the interpolant on the final mesh
    /// (`NaN` if it could not be computed).
    pub max_residual: f64,
}

// ---------------------------------------------------------------- Hermite

/// Cubic Hermite interpolant (and derivative) through `y`, `yp` on the mesh
/// `x`, evaluated at `xq` (clamped to the mesh range). Returns `n` values.
pub fn hermite_eval(x: &[f64], y: &[f64], yp: &[f64], n: usize, xq: f64, deriv: bool) -> Vec<f64> {
    let nn = x.len();
    // interval index: largest i with x[i] <= xq, capped at nn-2
    let i = match x.partition_point(|&v| v <= xq) {
        0 => 0,
        p => (p - 1).min(nn - 2),
    };
    let h = x[i + 1] - x[i];
    let t = (xq - x[i]) / h;
    let (t2, t3) = (t * t, t * t * t);
    let mut out = vec![0.0; n];
    for k in 0..n {
        let (y0, y1) = (y[i * n + k], y[(i + 1) * n + k]);
        let (f0, f1) = (yp[i * n + k], yp[(i + 1) * n + k]);
        out[k] = if deriv {
            ((6.0 * t2 - 6.0 * t) * y0 + (-6.0 * t2 + 6.0 * t) * y1) / h + (3.0 * t2 - 4.0 * t + 1.0) * f0 + (3.0 * t2 - 2.0 * t) * f1
        } else {
            (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * f0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * f1
        };
    }
    out
}

// ------------------------------------------------------------- banded LU

struct BandMatrix {
    m: usize,
    kl: usize,
    ku: usize,
    ldab: usize,
    ab: Vec<f64>,
}

impl BandMatrix {
    fn new(m: usize, kl: usize, ku: usize) -> BandMatrix {
        let ldab = 2 * kl + ku + 1;
        BandMatrix { m, kl, ku, ldab, ab: vec![0.0; ldab * m] }
    }

    #[inline]
    fn idx(&self, i: usize, j: usize) -> usize {
        (self.kl + self.ku + i) - j + j * self.ldab
    }

    fn add(&mut self, i: usize, j: usize, v: f64) {
        debug_assert!(i + self.ku >= j && i <= j + self.kl);
        let k = self.idx(i, j);
        self.ab[k] += v;
    }

    /// LU with partial pivoting in place (LAPACK `dgbtf2`), then solve
    /// `A z = b`. `None` when a pivot is numerically zero.
    fn solve(mut self, b: &mut [f64]) -> Option<f64> {
        let (m, kl, ku, ldab) = (self.m, self.kl, self.ku, self.ldab);
        let kv = kl + ku;
        let scale = self.ab.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        if !(scale > 0.0) || !scale.is_finite() {
            return None;
        }
        let tiny = 1e-14 * scale;
        let mut ipiv = vec![0usize; m];
        let mut ju = 0usize;
        let ab = &mut self.ab;
        for j in 0..m {
            let km = kl.min(m - 1 - j);
            let mut jp = 0usize;
            let mut best = ab[kv + j * ldab].abs();
            for t in 1..=km {
                let v = ab[kv + t + j * ldab].abs();
                if v > best {
                    best = v;
                    jp = t;
                }
            }
            ipiv[j] = j + jp;
            if !(best > tiny) {
                return None;
            }
            ju = ju.max((j + ku + jp).min(m - 1));
            if jp != 0 {
                for c in j..=ju {
                    let a = kv + j + jp - c + c * ldab;
                    let bidx = kv + j - c + c * ldab;
                    ab.swap(a, bidx);
                }
            }
            if km > 0 {
                let piv = ab[kv + j * ldab];
                for t in 1..=km {
                    ab[kv + t + j * ldab] /= piv;
                }
                for c in j + 1..=ju {
                    let ujc = ab[kv + j - c + c * ldab];
                    if ujc != 0.0 {
                        for t in 1..=km {
                            let l = ab[kv + t + j * ldab];
                            ab[kv + j + t - c + c * ldab] -= l * ujc;
                        }
                    }
                }
            }
        }
        for j in 0..m.saturating_sub(1) {
            let p = ipiv[j];
            if p != j {
                b.swap(j, p);
            }
            let km = kl.min(m - 1 - j);
            let bj = b[j];
            for t in 1..=km {
                b[j + t] -= ab[kv + t + j * ldab] * bj;
            }
        }
        for j in (0..m).rev() {
            b[j] /= ab[kv + j * ldab];
            let bj = b[j];
            for i in j.saturating_sub(kv)..j {
                b[i] -= ab[kv + i - j + j * ldab] * bj;
            }
        }
        if !b.iter().all(|v| v.is_finite()) {
            return None;
        }
        let min_pivot = (0..m).map(|j| ab[kv + j * ldab].abs()).fold(f64::INFINITY, f64::min);
        Some(min_pivot / scale)
    }
}

// ------------------------------------------------------------ collocation

struct Ctx<'a, 'b> {
    f: RhsFn<'a>,
    bc: BcFn<'b>,
    n: usize,
    nfev: usize,
    /// Smallest LU pivot over the largest Jacobian entry at the last Newton
    /// step: close to zero means the collocation system is nearly singular.
    pivot_ratio: f64,
}

struct State {
    f: Vec<f64>,   // f at nodes, node-major
    fm: Vec<f64>,  // f at the midpoints
    col: Vec<f64>, // collocation residuals per interval
    bc: Vec<f64>,
    cost: f64,
}

impl<'a, 'b> Ctx<'a, 'b> {
    fn fcall(&mut self, x: f64, y: &[f64]) -> Result<Vec<f64>, String> {
        self.nfev += 1;
        if self.nfev > MAX_FEV {
            return Err(format!("bvp: f was called more than {MAX_FEV} times without converging; the problem may be too hard for this guess or tolerance"));
        }
        let v = (self.f)(x, y)?;
        if v.len() != self.n {
            return Err(format!("bvp: f(x, y) returned {} value(s) but y has {} component(s)", v.len(), self.n));
        }
        Ok(v)
    }

    fn bccall(&mut self, ya: &[f64], yb: &[f64]) -> Result<Vec<f64>, String> {
        let v = (self.bc)(ya, yb)?;
        if v.len() != self.n {
            return Err(format!(
                "bvp: bc(ya, yb) returned {} value(s) but there are {} unknown(s); it must return one residual per component of y",
                v.len(),
                self.n
            ));
        }
        Ok(v)
    }

    fn eval_state(&mut self, x: &[f64], y: &[f64]) -> Result<State, String> {
        let n = self.n;
        let nn = x.len();
        let mut f = Vec::with_capacity(nn * n);
        for i in 0..nn {
            f.extend(self.fcall(x[i], &y[i * n..(i + 1) * n])?);
        }
        let mut fm = Vec::with_capacity((nn - 1) * n);
        let mut col = Vec::with_capacity((nn - 1) * n);
        let mut ym = vec![0.0; n];
        for i in 0..nn - 1 {
            let h = x[i + 1] - x[i];
            for k in 0..n {
                ym[k] = 0.5 * (y[i * n + k] + y[(i + 1) * n + k]) - h / 8.0 * (f[(i + 1) * n + k] - f[i * n + k]);
            }
            let fmid = self.fcall(x[i] + 0.5 * h, &ym)?;
            for k in 0..n {
                col.push(y[(i + 1) * n + k] - y[i * n + k] - h / 6.0 * (f[i * n + k] + 4.0 * fmid[k] + f[(i + 1) * n + k]));
            }
            fm.extend(fmid);
        }
        let bc = self.bccall(&y[0..n], &y[(nn - 1) * n..nn * n])?;
        let cost = col.iter().chain(bc.iter()).map(|v| v * v).sum::<f64>();
        Ok(State { f, fm, col, bc, cost })
    }

    /// Forward-difference Jacobian `df/dy` at `(x, y)` given `f0 = f(x, y)`,
    /// row-major `n x n`.
    fn fd_jac(&mut self, x: f64, y: &[f64], f0: &[f64]) -> Result<Vec<f64>, String> {
        let n = self.n;
        let mut jac = vec![0.0; n * n];
        let mut yp = y.to_vec();
        for j in 0..n {
            let h = 1.490_116_119_384_765_6e-8 * y[j].abs().max(1.0);
            yp[j] = y[j] + h;
            let step = yp[j] - y[j];
            let f1 = self.fcall(x, &yp)?;
            yp[j] = y[j];
            for i in 0..n {
                jac[i * n + j] = (f1[i] - f0[i]) / step;
            }
        }
        Ok(jac)
    }

    fn fd_bc_jac(&mut self, ya: &[f64], yb: &[f64], r0: &[f64]) -> Result<(Vec<f64>, Vec<f64>), String> {
        let n = self.n;
        let mut ja = vec![0.0; n * n];
        let mut jb = vec![0.0; n * n];
        let mut p = ya.to_vec();
        for j in 0..n {
            let h = 1.490_116_119_384_765_6e-8 * ya[j].abs().max(1.0);
            p[j] = ya[j] + h;
            let step = p[j] - ya[j];
            let r = self.bccall(&p, yb)?;
            p[j] = ya[j];
            for i in 0..n {
                ja[i * n + j] = (r[i] - r0[i]) / step;
            }
        }
        let mut p = yb.to_vec();
        for j in 0..n {
            let h = 1.490_116_119_384_765_6e-8 * yb[j].abs().max(1.0);
            p[j] = yb[j] + h;
            let step = p[j] - yb[j];
            let r = self.bccall(ya, &p)?;
            p[j] = yb[j];
            for i in 0..n {
                jb[i * n + j] = (r[i] - r0[i]) / step;
            }
        }
        Ok((ja, jb))
    }
}

fn matmul_nn(a: &[f64], b: &[f64], n: usize) -> Vec<f64> {
    let mut c = vec![0.0; n * n];
    for i in 0..n {
        for k in 0..n {
            let aik = a[i * n + k];
            if aik != 0.0 {
                for j in 0..n {
                    c[i * n + j] += aik * b[k * n + j];
                }
            }
        }
    }
    c
}

/// Folded node order `0, N-1, 1, N-2, ...`; returns `pos[node]`.
fn folded_positions(nn: usize) -> Vec<usize> {
    let mut pos = vec![0usize; nn];
    let (mut lo, mut hi, mut p) = (0usize, nn - 1, 0usize);
    while lo <= hi {
        pos[lo] = p;
        p += 1;
        if lo < hi {
            pos[hi] = p;
            p += 1;
        }
        lo += 1;
        if hi == 0 {
            break;
        }
        hi -= 1;
    }
    pos
}

/// Newton step: solves `J dy = -res`. `Ok(None)` for a singular Jacobian.
fn newton_step(ctx: &mut Ctx, x: &[f64], y: &[f64], st: &State) -> Result<Option<Vec<f64>>, String> {
    let n = ctx.n;
    let nn = x.len();
    let pos = folded_positions(nn);
    // interval rows are placed in order of their leftmost column block
    let mut order: Vec<usize> = (0..nn - 1).collect();
    order.sort_by_key(|&i| (pos[i].min(pos[i + 1]), i));
    let mut row_block = vec![0usize; nn - 1];
    for (r, &i) in order.iter().enumerate() {
        row_block[i] = r + 1; // block 0 is the boundary conditions
    }
    // bandwidth from the block pattern
    let (mut kl, mut ku) = (0usize, 0usize);
    let mut note = |r: usize, c: usize| {
        let (r0, r1) = (r * n, r * n + n - 1);
        let (c0, c1) = (c * n, c * n + n - 1);
        kl = kl.max(r1.saturating_sub(c0));
        ku = ku.max(c1.saturating_sub(r0));
    };
    for i in 0..nn - 1 {
        note(row_block[i], pos[i]);
        note(row_block[i], pos[i + 1]);
    }
    note(0, pos[0]);
    note(0, pos[nn - 1]);
    let m = nn * n;
    let mut band = BandMatrix::new(m, kl, ku);

    // node Jacobians
    let mut jn: Vec<Vec<f64>> = Vec::with_capacity(nn);
    for i in 0..nn {
        let j = ctx.fd_jac(x[i], &y[i * n..(i + 1) * n], &st.f[i * n..(i + 1) * n])?;
        jn.push(j);
    }
    let mut ym = vec![0.0; n];
    for i in 0..nn - 1 {
        let h = x[i + 1] - x[i];
        for k in 0..n {
            ym[k] = 0.5 * (y[i * n + k] + y[(i + 1) * n + k]) - h / 8.0 * (st.f[(i + 1) * n + k] - st.f[i * n + k]);
        }
        let jm = ctx.fd_jac(x[i] + 0.5 * h, &ym, &st.fm[i * n..(i + 1) * n])?;
        // dymid/dyi = I/2 + h/8 Ji ; dymid/dyi+1 = I/2 - h/8 Ji+1
        let mut d0 = vec![0.0; n * n];
        let mut d1 = vec![0.0; n * n];
        for a in 0..n {
            for b in 0..n {
                let id = if a == b { 0.5 } else { 0.0 };
                d0[a * n + b] = id + h / 8.0 * jn[i][a * n + b];
                d1[a * n + b] = id - h / 8.0 * jn[i + 1][a * n + b];
            }
        }
        let jm_d0 = matmul_nn(&jm, &d0, n);
        let jm_d1 = matmul_nn(&jm, &d1, n);
        let r = row_block[i];
        for a in 0..n {
            for b in 0..n {
                let id = if a == b { 1.0 } else { 0.0 };
                let left = -id - h / 6.0 * (jn[i][a * n + b] + 4.0 * jm_d0[a * n + b]);
                let right = id - h / 6.0 * (jn[i + 1][a * n + b] + 4.0 * jm_d1[a * n + b]);
                band.add(r * n + a, pos[i] * n + b, left);
                band.add(r * n + a, pos[i + 1] * n + b, right);
            }
        }
    }
    let (ja, jb) = ctx.fd_bc_jac(&y[0..n], &y[(nn - 1) * n..nn * n], &st.bc)?;
    for a in 0..n {
        for b in 0..n {
            band.add(a, pos[0] * n + b, ja[a * n + b]);
            band.add(a, pos[nn - 1] * n + b, jb[a * n + b]);
        }
    }
    // right-hand side -res in row order
    let mut rhs = vec![0.0; m];
    for a in 0..n {
        rhs[a] = -st.bc[a];
    }
    for i in 0..nn - 1 {
        for a in 0..n {
            rhs[row_block[i] * n + a] = -st.col[i * n + a];
        }
    }
    match band.solve(&mut rhs) {
        None => return Ok(None),
        Some(r) => ctx.pivot_ratio = r,
    }
    let mut dy = vec![0.0; m];
    for i in 0..nn {
        for k in 0..n {
            dy[i * n + k] = rhs[pos[i] * n + k];
        }
    }
    Ok(Some(dy))
}

enum NewtonEnd {
    Converged,
    NotConverged,
    Singular,
}

fn newton(ctx: &mut Ctx, x: &[f64], y: &mut Vec<f64>, tol: f64) -> Result<(State, NewtonEnd), String> {
    let n = ctx.n;
    let nn = x.len();
    let mut st = ctx.eval_state(x, y)?;
    if !st.cost.is_finite() {
        return Err("bvp: f or bc is not finite at the initial guess (check y0 and the mesh)".into());
    }
    for _ in 0..NEWTON_MAX_ITER {
        // converged?
        let mut ok = st.bc.iter().all(|v| v.abs() <= 0.1 * tol);
        if ok {
            for i in 0..nn - 1 {
                let h = x[i + 1] - x[i];
                for k in 0..n {
                    if st.col[i * n + k].abs() > 0.02 * tol * h * (1.0 + st.fm[i * n + k].abs()) {
                        ok = false;
                    }
                }
            }
        }
        if ok {
            return Ok((st, NewtonEnd::Converged));
        }
        let dy = match newton_step(ctx, x, y, &st)? {
            Some(d) => d,
            None => return Ok((st, NewtonEnd::Singular)),
        };
        // Armijo backtracking (SciPy: sigma 0.2, tau 0.5, 4 reductions)
        let mut alpha = 1.0;
        let mut ynew = vec![0.0; y.len()];
        let mut stnew: Option<State> = None;
        for trial in 0..=4 {
            for j in 0..y.len() {
                ynew[j] = y[j] + alpha * dy[j];
            }
            let s = ctx.eval_state(x, &ynew)?;
            let good = s.cost < (1.0 - 2.0 * alpha * 0.2) * st.cost;
            stnew = Some(s);
            if good {
                break;
            }
            if trial < 4 {
                alpha *= 0.5;
            }
        }
        let s = stnew.unwrap();
        if !s.cost.is_finite() {
            // the step took us somewhere f is not defined: stay where we were
            return Ok((st, NewtonEnd::NotConverged));
        }
        let step = (0..y.len()).map(|j| (alpha * dy[j]).abs() / (1.0 + y[j].abs())).fold(0.0f64, f64::max);
        y.copy_from_slice(&ynew);
        st = s;
        if step < 1e-13 {
            return Ok((st, NewtonEnd::Converged));
        }
    }
    Ok((st, NewtonEnd::NotConverged))
}

/// Normalised RMS residual of the interpolant on each interval.
fn rms_residuals(ctx: &mut Ctx, x: &[f64], y: &[f64], st: &State) -> Result<Vec<f64>, String> {
    let n = ctx.n;
    let nn = x.len();
    let mut out = Vec::with_capacity(nn - 1);
    for i in 0..nn - 1 {
        let h = x[i + 1] - x[i];
        let xm = x[i] + 0.5 * h;
        let s = 0.5 * h * (3.0f64 / 7.0).sqrt();
        let mut rr = [0.0f64; 3]; // middle, x1, x2
        // middle
        for k in 0..n {
            let r = 1.5 * st.col[i * n + k] / h / (1.0 + st.fm[i * n + k].abs());
            rr[0] += r * r;
        }
        for (slot, xq) in [(1usize, xm + s), (2usize, xm - s)] {
            let yq = hermite_eval(x, y, &st.f, n, xq, false);
            let ypq = hermite_eval(x, y, &st.f, n, xq, true);
            let fq = ctx.fcall(xq, &yq)?;
            for k in 0..n {
                let r = (ypq[k] - fq[k]) / (1.0 + fq[k].abs());
                rr[slot] += r * r;
            }
        }
        out.push((0.5 * (32.0 / 45.0 * rr[0] + 49.0 / 90.0 * (rr[1] + rr[2]))).sqrt());
    }
    Ok(out)
}

/// Solve the boundary value problem.
///
/// `x` is the initial mesh (strictly increasing, at least two nodes), `y0`
/// the initial guess, node-major (`y0[i * n + k]`). `tol` bounds the
/// normalised residual of the interpolant (and, as an absolute tolerance, the
/// boundary condition residual). Errors are only bad input or a failure
/// inside a callback; a hard problem returns a status instead.
pub fn solve_bvp(f: RhsFn, bc: BcFn, x: &[f64], n: usize, y0: &[f64], tol: f64, max_nodes: usize) -> Result<BvpResult, String> {
    if n == 0 || x.len() < 2 || y0.len() != n * x.len() {
        return Err("bvp: bad problem dimensions".into());
    }
    if x.iter().any(|v| !v.is_finite()) || x.windows(2).any(|w| !(w[1] > w[0])) {
        return Err("bvp: the mesh x must be finite and strictly increasing".into());
    }
    let mut ctx = Ctx { f, bc, n, nfev: 0, pivot_ratio: f64::INFINITY };
    let mut x = x.to_vec();
    let mut y = y0.to_vec();
    let mut last: Option<(State, f64)> = None;
    let mut status = BvpStatus::NoConvergence;
    for _pass in 0..MAX_PASSES {
        let (st, end) = newton(&mut ctx, &x, &mut y, tol)?;
        if matches!(end, NewtonEnd::Singular) {
            last = Some((st, f64::NAN));
            status = BvpStatus::Singular;
            break;
        }
        let rms = rms_residuals(&mut ctx, &x, &y, &st)?;
        let max_rms = rms.iter().cloned().fold(0.0f64, f64::max);
        let bc_ok = st.bc.iter().all(|v| v.abs() <= tol);
        let finite = rms.iter().all(|v| v.is_finite()) && y.iter().all(|v| v.is_finite());
        if !finite {
            last = Some((st, f64::NAN));
            status = BvpStatus::NoConvergence;
            break;
        }
        if max_rms < tol {
            status = if !bc_ok {
                BvpStatus::NoConvergence
            } else if ctx.pivot_ratio < SINGULAR_PIVOT_RATIO {
                // the residual is small only relative to a huge solution
                BvpStatus::Singular
            } else {
                BvpStatus::Converged
            };
            last = Some((st, max_rms));
            break;
        }
        let ins1: Vec<usize> = (0..rms.len()).filter(|&i| rms[i] > tol && rms[i] < 100.0 * tol).collect();
        let ins2: Vec<usize> = (0..rms.len()).filter(|&i| rms[i] >= 100.0 * tol).collect();
        let added = ins1.len() + 2 * ins2.len();
        if x.len() + added > max_nodes {
            status = BvpStatus::MaxNodes;
            last = Some((st, max_rms));
            break;
        }
        // new mesh
        let mut nx = Vec::with_capacity(x.len() + added);
        for i in 0..x.len() - 1 {
            nx.push(x[i]);
            let h = x[i + 1] - x[i];
            if rms[i] >= 100.0 * tol {
                nx.push(x[i] + h / 3.0);
                nx.push(x[i] + 2.0 * h / 3.0);
            } else if rms[i] > tol {
                nx.push(x[i] + 0.5 * h);
            }
        }
        nx.push(*x.last().unwrap());
        // a refinement that cannot be represented (interval already at machine width)
        if nx.windows(2).any(|w| !(w[1] > w[0])) {
            status = BvpStatus::NoConvergence;
            last = Some((st, max_rms));
            break;
        }
        let mut ny = Vec::with_capacity(nx.len() * n);
        for &xq in &nx {
            ny.extend(hermite_eval(&x, &y, &st.f, n, xq, false));
        }
        x = nx;
        y = ny;
    }
    let (st, max_residual) = match last {
        Some(v) => v,
        None => (ctx.eval_state(&x, &y)?, f64::NAN),
    };
    Ok(BvpResult { n, x, y, yp: st.f, status, max_residual })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve(
        f: &mut dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, String>,
        bc: &mut dyn FnMut(&[f64], &[f64]) -> Result<Vec<f64>, String>,
        x: &[f64],
        y0: &[f64],
        n: usize,
        tol: f64,
    ) -> BvpResult {
        solve_bvp(f, bc, x, n, y0, tol, 10_000).unwrap()
    }

    #[test]
    fn sine_from_second_order_system() {
        // y'' + y = 0, y(0) = 0, y(pi/2) = 1  ->  y = sin x
        let nn = 11;
        let b = std::f64::consts::FRAC_PI_2;
        let x: Vec<f64> = (0..nn).map(|i| b * i as f64 / (nn - 1) as f64).collect();
        let y0 = vec![0.0; 2 * nn];
        let r = solve(
            &mut |_x, y| Ok(vec![y[1], -y[0]]),
            &mut |ya, yb| Ok(vec![ya[0], yb[0] - 1.0]),
            &x,
            &y0,
            2,
            1e-9,
        );
        assert_eq!(r.status, BvpStatus::Converged);
        let err = r.x.iter().enumerate().map(|(i, &xi)| (r.y[i * 2] - xi.sin()).abs()).fold(0.0, f64::max);
        assert!(err < 1e-8, "err {err}");
    }

    #[test]
    fn band_solver_matches_dense() {
        // tridiagonal-with-pivoting system against a hand solve
        let m = 6;
        let mut a = vec![vec![0.0; m]; m];
        for i in 0..m {
            a[i][i] = if i % 2 == 0 { 0.1 } else { 3.0 };
            if i + 1 < m {
                a[i][i + 1] = 1.0;
                a[i + 1][i] = 2.0;
            }
        }
        let xs: Vec<f64> = (0..m).map(|i| 1.0 + i as f64).collect();
        let mut b: Vec<f64> = (0..m).map(|i| (0..m).map(|j| a[i][j] * xs[j]).sum()).collect();
        let mut band = BandMatrix::new(m, 1, 1);
        for i in 0..m {
            for j in 0..m {
                if a[i][j] != 0.0 {
                    band.add(i, j, a[i][j]);
                }
            }
        }
        band.solve(&mut b).unwrap();
        for i in 0..m {
            assert!((b[i] - xs[i]).abs() < 1e-12, "{b:?}");
        }
    }

    #[test]
    fn folded_positions_is_a_permutation() {
        for nn in 2..9 {
            let mut p = folded_positions(nn);
            p.sort();
            assert_eq!(p, (0..nn).collect::<Vec<_>>());
        }
    }

    #[test]
    fn problem_without_a_solution_is_not_reported_converged() {
        // y'' + y = 0, y(0) = 0, y(pi) = 1: no solution (resonance). The
        // discrete system is nearly singular and its "solution" is ~1e8.
        let nn = 11;
        let x: Vec<f64> = (0..nn).map(|i| std::f64::consts::PI * i as f64 / (nn - 1) as f64).collect();
        let r = solve(&mut |_x, y| Ok(vec![y[1], -y[0]]), &mut |ya, yb| Ok(vec![ya[0], yb[0] - 1.0]), &x, &vec![0.0; 2 * nn], 2, 1e-6);
        assert_eq!(r.status, BvpStatus::Singular);
        // the same equation on [0, 1.5] is fine
        let x: Vec<f64> = (0..nn).map(|i| 1.5 * i as f64 / (nn - 1) as f64).collect();
        let r = solve(&mut |_x, y| Ok(vec![y[1], -y[0]]), &mut |ya, yb| Ok(vec![ya[0], yb[0] - 1.0]), &x, &vec![0.0; 2 * nn], 2, 1e-6);
        assert_eq!(r.status, BvpStatus::Converged);
    }

    #[test]
    fn singular_periodic_problem_is_reported() {
        // y' = 0 with y(0) = y(1): every constant solves it
        let x: Vec<f64> = (0..6).map(|i| i as f64 / 5.0).collect();
        let r = solve(&mut |_x, _y| Ok(vec![0.0]), &mut |ya, yb| Ok(vec![ya[0] - yb[0]]), &x, &x.clone(), 1, 1e-6);
        assert_eq!(r.status, BvpStatus::Singular);
    }
}
