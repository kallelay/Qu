//! Compressed sensing: recovering a sparse signal from far fewer
//! measurements than Nyquist would demand.
//!
//! # The problem
//!
//! You measure `y = A·x`, where `A` is `m × n` with `m << n`. That system is
//! massively underdetermined — infinitely many `x` explain any `y` — so on
//! its own it says nothing. Compressed sensing adds one assumption and gets
//! a unique answer back: **`x` is sparse**, with at most `k` non-zero
//! entries, and `k` is small compared with `m`.
//!
//! The assumption is not a trick. Almost everything a laboratory measures
//! is sparse in *some* basis: a spectrum is a handful of tones among
//! thousands of bins, an impedance sweep is a few relaxation processes, an
//! image is sparse in wavelets. If you know the basis, you can measure far
//! below Nyquist and lose nothing.
//!
//! # When it works
//!
//! Two conditions on `A`, in decreasing order of usefulness:
//!
//! - **Coherence.** `mu(A)` is the largest absolute inner product between
//!   two different unit-normalised columns. Exact recovery is guaranteed
//!   whenever `k < (1 + 1/mu) / 2`. This is checkable in `O(n²)` time —
//!   [`coherence`] does it — and it is pessimistic: matrices routinely
//!   recover far more than the bound promises. Use it as a sanity check,
//!   never as a budget.
//!
//! - **The restricted isometry property.** `A` satisfies RIP of order `k`
//!   with constant `d` when `(1-d)‖x‖² ≤ ‖Ax‖² ≤ (1+d)‖x‖²` for every
//!   `k`-sparse `x`. This is the condition the strong theorems are stated
//!   in, and it is NP-hard to verify for a given matrix — you get it from
//!   *construction* instead. A random Gaussian or Bernoulli matrix
//!   satisfies RIP of order `k` with high probability once
//!   `m ≳ k·log(n/k)`, which is where the practical rule of thumb
//!   `m ≈ 4k` comes from. [`rip_bound_from_coherence`] gives the
//!   coherence-derived certificate, which is the only one that is
//!   computable, and says so.
//!
//! # The methods, and why there are several
//!
//! Recovering the sparsest `x` is combinatorial (an `l0` problem, NP-hard),
//! so every method here is a tractable stand-in. They divide into two
//! families that fail differently, which is the reason to have both.
//!
//! **Greedy** methods build the support one decision at a time. They are
//! fast, they need `k` up front, and they are brittle: a wrong column
//! chosen early is a wrong answer.
//!
//! - [`omp`] — orthogonal matching pursuit. Add the column most correlated
//!   with the residual, re-solve least squares on the whole support, repeat.
//!   The baseline: simple, `k` iterations, and hard to beat when `A` is
//!   well conditioned and `k` is genuinely small.
//! - [`cosamp`] — takes `2k` columns at once, solves, prunes back to `k`.
//!   Can *undo* a bad early choice, which OMP cannot, at a constant-factor
//!   cost. Reach for it when OMP stalls.
//! - [`subspace_pursuit`] — the same idea with a `k`-sized working set
//!   rather than `2k`. Cheaper than CoSaMP, similar robustness.
//! - [`iht`] — iterative hard thresholding. Gradient step, keep the `k`
//!   largest, repeat. No least-squares solve at all, so it is the one that
//!   scales to a problem too large to factorise; it needs the step size
//!   under control, which [`niht`] does automatically.
//!
//! **Convex** methods replace the `l0` count with the `l1` norm, which is
//! the tightest convex relaxation of it, and solve that exactly. Slower,
//! and they do not need `k`.
//!
//! - [`ista`] / [`fista`] — proximal gradient on
//!   `½‖Ax-y‖² + λ‖x‖₁` (the LASSO / basis-pursuit-denoising problem).
//!   FISTA is ISTA with Nesterov momentum: same iteration cost, `O(1/t²)`
//!   convergence against `O(1/t)`. There is no reason to run ISTA except to
//!   see the difference, and it is here for that.
//! - [`admm_bp`] — basis pursuit, `min ‖x‖₁ s.t. Ax = y`, by alternating
//!   direction method of multipliers. The one to use when the measurements
//!   are clean and you want the constraint satisfied exactly rather than
//!   penalised.
//!
//! And one that is not sparsity in a basis at all:
//!
//! - [`tv_denoise`] — total variation. Penalises `‖∇x‖₁` instead of
//!   `‖x‖₁`, i.e. assumes the signal is piecewise constant rather than
//!   spiky. This is what recovers a step response or a segmented image
//!   where an `l1` method would flatten everything.
//!
//! # Choosing
//!
//! Clean measurements, `k` known, `A` well conditioned: [`omp`]. Noisy, or
//! `k` only estimated: [`fista`]. OMP giving the wrong support: [`cosamp`].
//! Too large to factorise: [`niht`]. Piecewise-constant signal:
//! [`tv_denoise`].

use crate::matrix::Matrix;

/// What went wrong, in terms of the problem rather than the arithmetic.
#[derive(Debug, Clone, PartialEq)]
pub enum CsError {
    /// `A` has `m` rows and `y` has a different length.
    Mismatch { rows: usize, measurements: usize },
    /// A sparsity larger than the number of columns, or zero.
    BadSparsity { k: usize, n: usize },
    /// A parameter outside the range that means anything.
    BadParameter(String),
    /// An empty matrix or measurement vector.
    Empty,
    /// A least-squares solve on the working support failed.
    Singular(&'static str),
}

impl std::fmt::Display for CsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch { rows, measurements } => write!(
                f,
                "the sensing matrix has {rows} rows but {measurements} measurements were given"
            ),
            Self::BadSparsity { k, n } => write!(
                f,
                "sparsity {k} is impossible for {n} unknowns -- it must be between 1 and {n}"
            ),
            Self::BadParameter(m) => write!(f, "{m}"),
            Self::Empty => write!(f, "compressed sensing needs a non-empty matrix and measurements"),
            Self::Singular(w) => write!(
                f,
                "the least-squares solve on the working support failed ({w}) -- the chosen \
                 columns are linearly dependent, which usually means the sparsity asked for is \
                 larger than the measurements can support"
            ),
        }
    }
}

impl std::error::Error for CsError {}

/// What a recovery did, alongside what it found.
///
/// The diagnostics are not decoration. A greedy method that ran out of
/// iterations and one that converged return the same shape of answer and
/// are told apart only by `converged` and `iterations`; a caller that
/// ignores them will eventually publish a result that never converged.
#[derive(Debug, Clone)]
pub struct Recovery {
    /// The recovered signal, length `n`.
    pub x: Vec<f64>,
    /// Indices of the non-zero entries, ascending.
    pub support: Vec<usize>,
    /// `‖A·x - y‖₂` at the answer.
    pub residual: f64,
    /// Iterations actually run.
    pub iterations: usize,
    /// Whether the stopping tolerance was met, as opposed to the iteration
    /// cap being hit.
    pub converged: bool,
}

// ── helpers ────────────────────────────────────────────────────────────

fn check(a: &Matrix, y: &[f64]) -> Result<(), CsError> {
    if a.is_empty() || y.is_empty() {
        return Err(CsError::Empty);
    }
    if a.rows() != y.len() {
        return Err(CsError::Mismatch { rows: a.rows(), measurements: y.len() });
    }
    Ok(())
}

fn check_k(k: usize, n: usize) -> Result<(), CsError> {
    if k == 0 || k > n {
        return Err(CsError::BadSparsity { k, n });
    }
    Ok(())
}

/// `A·x` for a dense column vector held as a slice.
fn mul(a: &Matrix, x: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.rows()];
    for c in 0..a.cols() {
        let xc = x[c];
        if xc == 0.0 {
            continue;
        }
        for r in 0..a.rows() {
            out[r] += a.as_slice()[c * a.rows() + r] * xc;
        }
    }
    out
}

/// `Aᵀ·r`.
fn mul_t(a: &Matrix, r: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.cols()];
    for c in 0..a.cols() {
        let col = &a.as_slice()[c * a.rows()..(c + 1) * a.rows()];
        out[c] = col.iter().zip(r).map(|(v, ri)| v * ri).sum();
    }
    out
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Least squares on a chosen subset of columns, by normal equations with a
/// Cholesky solve and a small ridge for safety.
///
/// The normal equations square the condition number, which is the textbook
/// objection to them — but the working set here is `k` columns of a matrix
/// that was chosen to be incoherent, so `AᵀA` on that subset is close to
/// the identity and the objection does not bite. The ridge catches the case
/// where a greedy step picked a near-duplicate column anyway.
fn lstsq_on_support(a: &Matrix, y: &[f64], support: &[usize]) -> Result<Vec<f64>, CsError> {
    let k = support.len();
    if k == 0 {
        return Ok(Vec::new());
    }
    let m = a.rows();
    let cols: Vec<&[f64]> =
        support.iter().map(|&c| &a.as_slice()[c * m..(c + 1) * m]).collect();

    // Gram matrix and right-hand side.
    let mut g = vec![0.0; k * k];
    for i in 0..k {
        for j in i..k {
            let v: f64 = cols[i].iter().zip(cols[j]).map(|(x, z)| x * z).sum();
            g[i * k + j] = v;
            g[j * k + i] = v;
        }
    }
    let scale = (0..k).map(|i| g[i * k + i]).fold(0.0f64, f64::max).max(1.0);
    for i in 0..k {
        g[i * k + i] += scale * 1e-12;
    }
    let mut b: Vec<f64> = cols.iter().map(|c| c.iter().zip(y).map(|(x, z)| x * z).sum()).collect();

    // Cholesky, in place.
    for i in 0..k {
        for j in 0..=i {
            let mut sum = g[i * k + j];
            for p in 0..j {
                sum -= g[i * k + p] * g[j * k + p];
            }
            if i == j {
                if sum <= 0.0 {
                    return Err(CsError::Singular("Gram matrix not positive definite"));
                }
                g[i * k + i] = sum.sqrt();
            } else {
                g[i * k + j] = sum / g[j * k + j];
            }
        }
    }
    // Forward then back substitution.
    for i in 0..k {
        let mut sum = b[i];
        for p in 0..i {
            sum -= g[i * k + p] * b[p];
        }
        b[i] = sum / g[i * k + i];
    }
    for i in (0..k).rev() {
        let mut sum = b[i];
        for p in i + 1..k {
            sum -= g[p * k + i] * b[p];
        }
        b[i] = sum / g[i * k + i];
    }
    Ok(b)
}

/// Indices of the `k` largest entries by absolute value, ascending.
fn largest_k(v: &[f64], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&i, &j| v[j].abs().total_cmp(&v[i].abs()));
    idx.truncate(k);
    idx.sort_unstable();
    idx
}

fn scatter(values: &[f64], support: &[usize], n: usize) -> Vec<f64> {
    let mut x = vec![0.0; n];
    for (&v, &i) in values.iter().zip(support) {
        x[i] = v;
    }
    x
}

fn finish(a: &Matrix, y: &[f64], x: Vec<f64>, iterations: usize, converged: bool) -> Recovery {
    let r = mul(a, &x);
    let residual = norm(&y.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>());
    let support: Vec<usize> = x.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, _)| i).collect();
    Recovery { x, support, residual, iterations, converged }
}

// ── diagnostics ────────────────────────────────────────────────────────

/// Mutual coherence: the largest absolute inner product between two
/// different unit-normalised columns.
///
/// Small is good. A zero column is skipped rather than producing a NaN,
/// because a sensing matrix with an unused column is a design mistake worth
/// surviving long enough to be told about.
pub fn coherence(a: &Matrix) -> f64 {
    let m = a.rows();
    let n = a.cols();
    let cols: Vec<Vec<f64>> = (0..n)
        .map(|c| {
            let col = &a.as_slice()[c * m..(c + 1) * m];
            let nrm = norm(col);
            if nrm > 0.0 {
                col.iter().map(|v| v / nrm).collect()
            } else {
                vec![0.0; m]
            }
        })
        .collect();
    let mut worst: f64 = 0.0;
    for i in 0..n {
        for j in i + 1..n {
            let d: f64 = cols[i].iter().zip(&cols[j]).map(|(x, z)| x * z).sum();
            worst = worst.max(d.abs());
        }
    }
    worst
}

/// The largest sparsity for which exact recovery is *guaranteed* by
/// coherence alone: `k < (1 + 1/mu)/2`.
///
/// This is the only recovery certificate that is actually computable — RIP
/// constants are NP-hard — and it is badly pessimistic. A random Gaussian
/// `100 × 500` matrix has coherence around 0.4, so this returns 1 or 2,
/// while the same matrix recovers `k = 20` reliably. Treat a pass as proof
/// and a failure as no information.
pub fn rip_bound_from_coherence(a: &Matrix) -> usize {
    let mu = coherence(a);
    if mu <= 0.0 {
        return a.cols();
    }
    (((1.0 + 1.0 / mu) / 2.0).floor() as usize).max(1)
}

// ── greedy ─────────────────────────────────────────────────────────────

/// Orthogonal matching pursuit.
///
/// Each iteration adds the single column most correlated with the current
/// residual, then re-solves least squares over the *whole* accumulated
/// support — the "orthogonal" part, and the difference from plain matching
/// pursuit, which never revisits a coefficient and consequently converges
/// far more slowly.
///
/// Stops after `k` columns, or earlier if the residual falls under `tol`.
pub fn omp(a: &Matrix, y: &[f64], k: usize, tol: f64) -> Result<Recovery, CsError> {
    check(a, y)?;
    check_k(k, a.cols())?;
    let n = a.cols();
    let mut support: Vec<usize> = Vec::with_capacity(k);
    let mut residual = y.to_vec();
    let mut coeffs = Vec::new();

    for it in 0..k {
        let corr = mul_t(a, &residual);
        // The best column not already chosen.
        let mut best = None;
        let mut best_abs = 0.0;
        for (i, c) in corr.iter().enumerate() {
            if support.contains(&i) {
                continue;
            }
            if c.abs() > best_abs {
                best_abs = c.abs();
                best = Some(i);
            }
        }
        let Some(pick) = best else { break };
        support.push(pick);
        support.sort_unstable();
        coeffs = lstsq_on_support(a, y, &support)?;
        let x = scatter(&coeffs, &support, n);
        let approx = mul(a, &x);
        residual = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        if norm(&residual) <= tol {
            return Ok(finish(a, y, x, it + 1, true));
        }
    }
    let x = scatter(&coeffs, &support, n);
    Ok(finish(a, y, x, support.len(), norm(&residual) <= tol))
}

/// CoSaMP — compressive sampling matching pursuit.
///
/// Where OMP commits to one column per step and never reconsiders, CoSaMP
/// merges the `2k` best new candidates with the current support each
/// iteration, solves on the union, and prunes back to `k`. That prune is
/// the whole point: a column wrongly chosen early can be dropped later,
/// which OMP structurally cannot do.
pub fn cosamp(a: &Matrix, y: &[f64], k: usize, iters: usize, tol: f64) -> Result<Recovery, CsError> {
    check(a, y)?;
    check_k(k, a.cols())?;
    let n = a.cols();
    let mut x = vec![0.0; n];
    let mut residual = y.to_vec();

    for it in 0..iters {
        let proxy = mul_t(a, &residual);
        let mut merged = largest_k(&proxy, (2 * k).min(n));
        for (i, v) in x.iter().enumerate() {
            if *v != 0.0 && !merged.contains(&i) {
                merged.push(i);
            }
        }
        merged.sort_unstable();
        let solved = lstsq_on_support(a, y, &merged)?;
        let full = scatter(&solved, &merged, n);
        let keep = largest_k(&full, k);
        x = vec![0.0; n];
        for &i in &keep {
            x[i] = full[i];
        }
        let approx = mul(a, &x);
        residual = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        if norm(&residual) <= tol {
            return Ok(finish(a, y, x, it + 1, true));
        }
    }
    Ok(finish(a, y, x, iters, false))
}

/// Subspace pursuit: CoSaMP's structure with a `k`-sized candidate set
/// rather than `2k`. Cheaper per iteration, comparable robustness.
pub fn subspace_pursuit(
    a: &Matrix,
    y: &[f64],
    k: usize,
    iters: usize,
    tol: f64,
) -> Result<Recovery, CsError> {
    check(a, y)?;
    check_k(k, a.cols())?;
    let n = a.cols();
    let mut support = largest_k(&mul_t(a, y), k);
    let mut x = vec![0.0; n];

    for it in 0..iters {
        let solved = lstsq_on_support(a, y, &support)?;
        let approx = mul(a, &scatter(&solved, &support, n));
        let residual: Vec<f64> = y.iter().zip(&approx).map(|(a, b)| a - b).collect();

        let mut merged = support.clone();
        for i in largest_k(&mul_t(a, &residual), k) {
            if !merged.contains(&i) {
                merged.push(i);
            }
        }
        merged.sort_unstable();
        let full = scatter(&lstsq_on_support(a, y, &merged)?, &merged, n);
        let next = largest_k(&full, k);

        let refined = lstsq_on_support(a, y, &next)?;
        x = scatter(&refined, &next, n);
        let approx = mul(a, &x);
        let r = norm(&y.iter().zip(&approx).map(|(a, b)| a - b).collect::<Vec<_>>());
        if next == support || r <= tol {
            return Ok(finish(a, y, x, it + 1, r <= tol));
        }
        support = next;
    }
    Ok(finish(a, y, x, iters, false))
}

/// Iterative hard thresholding: `x ← H_k(x + step·Aᵀ(y - Ax))`.
///
/// No least-squares solve anywhere, so the cost per iteration is two
/// matrix-vector products and a partial sort. That is what makes it the
/// method for a problem too large to factorise — and also what makes it
/// fragile: it converges only when `step < 1/‖A‖₂²`, and diverges
/// spectacularly otherwise. [`niht`] picks the step for you.
pub fn iht(
    a: &Matrix,
    y: &[f64],
    k: usize,
    step: f64,
    iters: usize,
    tol: f64,
) -> Result<Recovery, CsError> {
    check(a, y)?;
    check_k(k, a.cols())?;
    if !(step > 0.0) || !step.is_finite() {
        return Err(CsError::BadParameter(format!(
            "iht: the step must be a positive number, found {step}"
        )));
    }
    let n = a.cols();
    let mut x = vec![0.0; n];

    for it in 0..iters {
        let approx = mul(a, &x);
        let residual: Vec<f64> = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        if norm(&residual) <= tol {
            return Ok(finish(a, y, x, it, true));
        }
        let grad = mul_t(a, &residual);
        let proposal: Vec<f64> = x.iter().zip(&grad).map(|(xi, g)| xi + step * g).collect();
        let keep = largest_k(&proposal, k);
        let mut next = vec![0.0; n];
        for &i in &keep {
            next[i] = proposal[i];
        }
        x = next;
    }
    Ok(finish(a, y, x, iters, false))
}

/// Normalised IHT: the same iteration with the step recomputed each time
/// from the gradient restricted to the current support.
///
/// The step that makes IHT converge depends on `‖A‖₂`, which you would have
/// to estimate; NIHT sidesteps that by choosing the step that minimises the
/// residual along the gradient direction, which is a closed form. Slower
/// per iteration by one matrix-vector product, and it does not diverge.
pub fn niht(a: &Matrix, y: &[f64], k: usize, iters: usize, tol: f64) -> Result<Recovery, CsError> {
    check(a, y)?;
    check_k(k, a.cols())?;
    let n = a.cols();
    let mut support = largest_k(&mul_t(a, y), k);
    let mut x = vec![0.0; n];

    for it in 0..iters {
        let approx = mul(a, &x);
        let residual: Vec<f64> = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        if norm(&residual) <= tol {
            return Ok(finish(a, y, x, it, true));
        }
        let grad = mul_t(a, &residual);
        // Restrict to the support, then take the exact minimising step.
        let mut g_s = vec![0.0; n];
        for &i in &support {
            g_s[i] = grad[i];
        }
        let ag = mul(a, &g_s);
        let num: f64 = g_s.iter().map(|v| v * v).sum();
        let den: f64 = ag.iter().map(|v| v * v).sum();
        if den <= 0.0 {
            return Ok(finish(a, y, x, it, false));
        }
        let step = num / den;
        let proposal: Vec<f64> = x.iter().zip(&grad).map(|(xi, g)| xi + step * g).collect();
        support = largest_k(&proposal, k);
        let mut next = vec![0.0; n];
        for &i in &support {
            next[i] = proposal[i];
        }
        x = next;
    }
    Ok(finish(a, y, x, iters, false))
}

// ── convex ─────────────────────────────────────────────────────────────

fn soft_threshold(v: f64, t: f64) -> f64 {
    if v > t {
        v - t
    } else if v < -t {
        v + t
    } else {
        0.0
    }
}

/// A safe gradient step for the proximal methods: `1/‖A‖₂²`, with `‖A‖₂`
/// estimated by a few power iterations on `AᵀA`.
fn lipschitz_step(a: &Matrix) -> f64 {
    let n = a.cols();
    let mut v: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64) * 1e-3).collect();
    let mut lambda = 1.0;
    for _ in 0..50 {
        let w = mul_t(a, &mul(a, &v));
        let nrm = norm(&w);
        if nrm <= 0.0 {
            return 1.0;
        }
        v = w.iter().map(|x| x / nrm).collect();
        lambda = nrm;
    }
    if lambda > 0.0 {
        1.0 / lambda
    } else {
        1.0
    }
}

/// ISTA — iterative soft thresholding, proximal gradient descent on
/// `½‖Ax-y‖² + λ‖x‖₁`.
///
/// Present mostly so the difference from [`fista`] can be seen: same cost
/// per iteration, `O(1/t)` convergence against FISTA's `O(1/t²)`. In
/// practice use FISTA.
pub fn ista(a: &Matrix, y: &[f64], lambda: f64, iters: usize, tol: f64) -> Result<Recovery, CsError> {
    check(a, y)?;
    if !(lambda >= 0.0) {
        return Err(CsError::BadParameter(format!(
            "ista: lambda must be non-negative, found {lambda}"
        )));
    }
    let n = a.cols();
    let step = lipschitz_step(a);
    let mut x = vec![0.0; n];

    for it in 0..iters {
        let approx = mul(a, &x);
        let residual: Vec<f64> = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        let grad = mul_t(a, &residual);
        let next: Vec<f64> = x
            .iter()
            .zip(&grad)
            .map(|(xi, g)| soft_threshold(xi + step * g, step * lambda))
            .collect();
        let change = norm(&next.iter().zip(&x).map(|(a, b)| a - b).collect::<Vec<_>>());
        x = next;
        if change <= tol {
            return Ok(finish(a, y, x, it + 1, true));
        }
    }
    Ok(finish(a, y, x, iters, false))
}

/// FISTA — ISTA with Nesterov momentum.
///
/// The extrapolation `z = x_t + ((t-1)/(t+2))·(x_t - x_{t-1})` costs one
/// vector operation and turns `O(1/t)` convergence into `O(1/t²)`. It is
/// the default convex solver here for that reason.
pub fn fista(
    a: &Matrix,
    y: &[f64],
    lambda: f64,
    iters: usize,
    tol: f64,
) -> Result<Recovery, CsError> {
    check(a, y)?;
    if !(lambda >= 0.0) {
        return Err(CsError::BadParameter(format!(
            "fista: lambda must be non-negative, found {lambda}"
        )));
    }
    let n = a.cols();
    let step = lipschitz_step(a);
    let mut x = vec![0.0; n];
    let mut prev = x.clone();
    let mut t = 1.0f64;

    for it in 0..iters {
        let momentum = (t - 1.0) / (t + 2.0);
        let z: Vec<f64> =
            x.iter().zip(&prev).map(|(xi, pi)| xi + momentum * (xi - pi)).collect();
        let approx = mul(a, &z);
        let residual: Vec<f64> = y.iter().zip(&approx).map(|(a, b)| a - b).collect();
        let grad = mul_t(a, &residual);
        let next: Vec<f64> = z
            .iter()
            .zip(&grad)
            .map(|(zi, g)| soft_threshold(zi + step * g, step * lambda))
            .collect();
        let change = norm(&next.iter().zip(&x).map(|(a, b)| a - b).collect::<Vec<_>>());
        prev = std::mem::replace(&mut x, next);
        t += 1.0;
        if change <= tol {
            return Ok(finish(a, y, x, it + 1, true));
        }
    }
    Ok(finish(a, y, x, iters, false))
}

/// Basis pursuit by ADMM: `min ‖x‖₁ subject to Ax = y`.
///
/// The equality-constrained problem, for when the measurements are clean
/// and you want `Ax = y` satisfied rather than traded off against sparsity.
/// Splits into an `l1` proximal step (soft thresholding) and a projection
/// onto the affine set `{x : Ax = y}`, alternated with a dual update.
///
/// The projection needs `Aᵀ(AAᵀ)⁻¹`, which is factored once and reused —
/// that is what makes the iteration cheap despite the constraint.
pub fn admm_bp(a: &Matrix, y: &[f64], rho: f64, iters: usize, tol: f64) -> Result<Recovery, CsError> {
    check(a, y)?;
    if !(rho > 0.0) {
        return Err(CsError::BadParameter(format!(
            "admm_bp: rho must be positive, found {rho}"
        )));
    }
    let m = a.rows();
    let n = a.cols();

    // (AAᵀ + eps·I)⁻¹ by Cholesky, formed once.
    let mut g = vec![0.0; m * m];
    for i in 0..m {
        for j in i..m {
            let v: f64 = (0..n)
                .map(|c| a.as_slice()[c * m + i] * a.as_slice()[c * m + j])
                .sum();
            g[i * m + j] = v;
            g[j * m + i] = v;
        }
    }
    let scale = (0..m).map(|i| g[i * m + i]).fold(0.0f64, f64::max).max(1.0);
    for i in 0..m {
        g[i * m + i] += scale * 1e-10;
    }
    for i in 0..m {
        for j in 0..=i {
            let mut sum = g[i * m + j];
            for p in 0..j {
                sum -= g[i * m + p] * g[j * m + p];
            }
            if i == j {
                if sum <= 0.0 {
                    return Err(CsError::Singular("A·Aᵀ not positive definite"));
                }
                g[i * m + i] = sum.sqrt();
            } else {
                g[i * m + j] = sum / g[j * m + j];
            }
        }
    }
    let solve_gram = |rhs: &[f64]| -> Vec<f64> {
        let mut b = rhs.to_vec();
        for i in 0..m {
            let mut sum = b[i];
            for p in 0..i {
                sum -= g[i * m + p] * b[p];
            }
            b[i] = sum / g[i * m + i];
        }
        for i in (0..m).rev() {
            let mut sum = b[i];
            for p in i + 1..m {
                sum -= g[p * m + i] * b[p];
            }
            b[i] = sum / g[i * m + i];
        }
        b
    };

    let mut x = vec![0.0; n];
    let mut z = vec![0.0; n];
    let mut u = vec![0.0; n];

    for it in 0..iters {
        // x-update: project z - u onto {x : Ax = y}.
        let v: Vec<f64> = z.iter().zip(&u).map(|(zi, ui)| zi - ui).collect();
        let av = mul(a, &v);
        let corr: Vec<f64> = y.iter().zip(&av).map(|(yi, ai)| yi - ai).collect();
        let lam = solve_gram(&corr);
        let at_lam = mul_t(a, &lam);
        x = v.iter().zip(&at_lam).map(|(vi, ai)| vi + ai).collect();

        // z-update: soft threshold.
        let z_old = z.clone();
        z = x
            .iter()
            .zip(&u)
            .map(|(xi, ui)| soft_threshold(xi + ui, 1.0 / rho))
            .collect();

        // dual update.
        for i in 0..n {
            u[i] += x[i] - z[i];
        }

        let primal = norm(&x.iter().zip(&z).map(|(a, b)| a - b).collect::<Vec<_>>());
        let dual = rho * norm(&z.iter().zip(&z_old).map(|(a, b)| a - b).collect::<Vec<_>>());
        if primal <= tol && dual <= tol {
            return Ok(finish(a, y, z, it + 1, true));
        }
    }
    Ok(finish(a, y, z, iters, false))
}

/// One-dimensional total variation denoising: minimise
/// `½‖x - y‖² + λ‖∇x‖₁`.
///
/// A different sparsity assumption from everything above: not that the
/// signal has few non-zero samples, but that it has few *changes* — a
/// staircase, a segmented profile, a step response. An `l1` method applied
/// to such a signal flattens it; this preserves the edges exactly, which is
/// the property TV is famous for.
///
/// Condat's direct algorithm: one pass, `O(n)`, exact rather than iterative.
pub fn tv_denoise(y: &[f64], lambda: f64) -> Result<Vec<f64>, CsError> {
    if y.is_empty() {
        return Err(CsError::Empty);
    }
    if !(lambda >= 0.0) {
        return Err(CsError::BadParameter(format!(
            "tv_denoise: lambda must be non-negative, found {lambda}"
        )));
    }
    let n = y.len();
    if lambda == 0.0 || n == 1 {
        return Ok(y.to_vec());
    }
    let mut x = vec![0.0; n];

    let (mut k, mut k0, mut kmin, mut kplus) = (0usize, 0usize, 0usize, 0usize);
    let (mut vmin, mut vmax) = (y[0] - lambda, y[0] + lambda);
    let (mut umin, mut umax) = (lambda, -lambda);

    loop {
        if k == n - 1 {
            if umin < 0.0 {
                for i in k0..=kmin {
                    x[i] = vmin;
                }
                k = kmin + 1;
                k0 = k;
                kmin = k;
                vmin = y[k];
                umin = lambda;
                umax = y[k] + lambda - vmax;
                continue;
            } else if umax > 0.0 {
                for i in k0..=kplus {
                    x[i] = vmax;
                }
                k = kplus + 1;
                k0 = k;
                kplus = k;
                vmax = y[k];
                umax = -lambda;
                umin = y[k] - lambda - vmin;
                continue;
            } else {
                let v = vmin + umin / ((k - k0 + 1) as f64);
                for i in k0..n {
                    x[i] = v;
                }
                break;
            }
        }

        umin += y[k + 1] - vmin;
        umax += y[k + 1] - vmax;

        if umin < -lambda {
            for i in k0..=kmin {
                x[i] = vmin;
            }
            k = kmin + 1;
            k0 = k;
            kmin = k;
            kplus = k;
            vmin = y[k];
            vmax = y[k] + 2.0 * lambda;
            umin = lambda;
            umax = -lambda;
        } else if umax > lambda {
            for i in k0..=kplus {
                x[i] = vmax;
            }
            k = kplus + 1;
            k0 = k;
            kmin = k;
            kplus = k;
            vmin = y[k] - 2.0 * lambda;
            vmax = y[k];
            umin = lambda;
            umax = -lambda;
        } else {
            k += 1;
            if umin >= lambda {
                kmin = k;
                vmin += (umin - lambda) / ((kmin - k0 + 1) as f64);
                umin = lambda;
            }
            if umax <= -lambda {
                kplus = k;
                vmax += (umax + lambda) / ((kplus - k0 + 1) as f64);
                umax = -lambda;
            }
        }
    }
    Ok(x)
}

/// Re-fit the found support by plain least squares, discarding the
/// coefficient values the sparse solver produced.
///
/// The `l1` penalty that makes a convex method find the right support also
/// SHRINKS every coefficient toward zero — that is the same term doing both
/// jobs, and it is why FISTA lands on the correct support with a systematic
/// few-parts-in-ten-thousand bias. Once the support is known the bias has
/// no purpose: least squares on those columns alone gives the coefficients
/// the measurements actually imply.
///
/// Standard practice, and cheap. The only reason not to do it is if the
/// support is wrong, in which case debiasing commits to the error rather
/// than leaving it visibly shrunk.
pub fn debias(a: &Matrix, y: &[f64], x: &[f64]) -> Result<Recovery, CsError> {
    check(a, y)?;
    let support: Vec<usize> =
        x.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, _)| i).collect();
    if support.is_empty() {
        return Ok(finish(a, y, vec![0.0; a.cols()], 0, false));
    }
    let refined = lstsq_on_support(a, y, &support)?;
    Ok(finish(a, y, scatter(&refined, &support, a.cols()), 1, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pseudo-random matrix, so the tests are reproducible
    /// without pulling a generator in.
    fn sensing(m: usize, n: usize, seed: u64) -> Matrix {
        let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut data = Vec::with_capacity(m * n);
        for _ in 0..m * n {
            // xorshift, then Box-Muller-free: a sum of uniforms is close
            // enough to Gaussian for a sensing matrix.
            let mut acc = 0.0;
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                acc += (state >> 11) as f64 / (1u64 << 53) as f64;
            }
            data.push((acc - 2.0) / (4.0f64 / 12.0).sqrt() / (m as f64).sqrt());
        }
        Matrix::from_col_major(m, n, data)
    }

    fn sparse_signal(n: usize, support: &[(usize, f64)]) -> Vec<f64> {
        let mut x = vec![0.0; n];
        for &(i, v) in support {
            x[i] = v;
        }
        x
    }

    fn recovers(got: &[f64], want: &[f64], tol: f64) -> bool {
        got.iter().zip(want).all(|(a, b)| (a - b).abs() < tol)
    }

    /// The headline claim, for every greedy method: a 5-sparse signal in
    /// 200 unknowns, recovered exactly from 60 measurements. Nyquist would
    /// have needed 200.
    #[test]
    fn every_greedy_method_recovers_a_sparse_signal_from_far_too_few_measurements() {
        let (m, n) = (60, 200);
        let a = sensing(m, n, 12345);
        let x = sparse_signal(n, &[(7, 1.5), (33, -2.0), (91, 0.75), (140, 3.0), (177, -1.25)]);
        let y = mul(&a, &x);

        let got = omp(&a, &y, 5, 1e-9).unwrap();
        assert!(recovers(&got.x, &x, 1e-6), "omp: {:?}", got.support);
        assert_eq!(got.support, vec![7, 33, 91, 140, 177]);

        let got = cosamp(&a, &y, 5, 50, 1e-9).unwrap();
        assert!(recovers(&got.x, &x, 1e-6), "cosamp: {:?}", got.support);

        let got = subspace_pursuit(&a, &y, 5, 50, 1e-9).unwrap();
        assert!(recovers(&got.x, &x, 1e-6), "subspace_pursuit: {:?}", got.support);

        let got = niht(&a, &y, 5, 500, 1e-9).unwrap();
        assert!(recovers(&got.x, &x, 1e-5), "niht: {:?}", got.support);
    }

    /// FISTA finds the same support without being told `k`, which is the
    /// whole reason to prefer a convex method.
    #[test]
    fn fista_finds_the_support_without_being_told_the_sparsity() {
        let (m, n) = (60, 200);
        let a = sensing(m, n, 999);
        let x = sparse_signal(n, &[(3, 2.0), (44, -1.5), (150, 1.0)]);
        let y = mul(&a, &x);

        let got = fista(&a, &y, 1e-3, 4000, 1e-12).unwrap();
        let mut found: Vec<usize> =
            got.x.iter().enumerate().filter(|(_, v)| v.abs() > 0.1).map(|(i, _)| i).collect();
        found.sort_unstable();
        assert_eq!(found, vec![3, 44, 150], "support wrong: residual {}", got.residual);
    }

    /// FISTA reaches a given accuracy in fewer iterations than ISTA. That
    /// is the only claim momentum makes, and it should be checked rather
    /// than asserted in a comment.
    #[test]
    fn fista_converges_faster_than_ista_on_the_same_problem() {
        let (m, n) = (40, 120);
        let a = sensing(m, n, 7);
        let x = sparse_signal(n, &[(5, 1.0), (60, -1.0)]);
        let y = mul(&a, &x);

        let slow = ista(&a, &y, 1e-3, 300, 0.0).unwrap();
        let fast = fista(&a, &y, 1e-3, 300, 0.0).unwrap();
        assert!(
            fast.residual < slow.residual,
            "fista {} should beat ista {} at equal iterations",
            fast.residual,
            slow.residual
        );
    }

    /// The l1 penalty finds the support and shrinks the coefficients, both
    /// with the same term. Debiasing keeps the first and drops the second.
    #[test]
    fn debiasing_removes_the_shrinkage_a_convex_method_leaves_behind() {
        let (m, n) = (60, 200);
        let a = sensing(m, n, 999);
        let x = sparse_signal(n, &[(3, 2.0), (44, -1.5), (150, 1.0)]);
        let y = mul(&a, &x);

        let biased = fista(&a, &y, 1e-3, 4000, 1e-12).unwrap();
        // Drop the near-zero tail the penalty leaves, then re-fit.
        let cleaned: Vec<f64> =
            biased.x.iter().map(|v| if v.abs() > 0.05 { *v } else { 0.0 }).collect();
        let fixed = debias(&a, &y, &cleaned).unwrap();

        let err = |v: &[f64]| -> f64 {
            v.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max)
        };
        assert!(err(&biased.x) > 1e-4, "expected visible shrinkage, got {}", err(&biased.x));
        assert!(err(&fixed.x) < 1e-8, "debiased should be exact, got {}", err(&fixed.x));
    }

    #[test]
    fn basis_pursuit_satisfies_the_measurements_exactly() {
        let (m, n) = (50, 150);
        let a = sensing(m, n, 4242);
        let x = sparse_signal(n, &[(11, 1.0), (70, -2.0), (120, 0.5)]);
        let y = mul(&a, &x);

        let got = admm_bp(&a, &y, 1.0, 3000, 1e-8).unwrap();
        assert!(got.residual < 1e-4, "residual {}", got.residual);
    }

    /// Total variation keeps an edge sharp, which is the property it exists
    /// for and the one a smoothing filter destroys.
    #[test]
    fn total_variation_preserves_a_step_that_smoothing_would_round_off() {
        let mut y: Vec<f64> = (0..100).map(|i| if i < 50 { 0.0 } else { 1.0 }).collect();
        // A little deterministic noise.
        for (i, v) in y.iter_mut().enumerate() {
            *v += ((i * 37 % 11) as f64 - 5.0) * 0.01;
        }
        let x = tv_denoise(&y, 0.3).unwrap();

        // The step survives at full height. This is the claim: a smoothing
        // filter wide enough to remove the noise would also spread this
        // edge over its whole window.
        assert!(x[49] < 0.1, "left of the step: {}", x[49]);
        assert!(x[50] > 0.9, "right of the step: {}", x[50]);

        // And the wiggle is gone: total variation away from the edge, which
        // is the quantity being minimised, drops by an order of magnitude
        // while the edge itself contributes its full 1.0 to both.
        let tv = |v: &[f64]| -> f64 { v.windows(2).map(|w| (w[1] - w[0]).abs()).sum() };
        let before = tv(&y[..50]);
        let after = tv(&x[..50]);
        assert!(
            after < before / 10.0,
            "variation on the flat side: {before} before, {after} after"
        );
        assert!((tv(&x) - 1.0).abs() < 0.2, "the whole signal should be one step: {}", tv(&x));
    }

    #[test]
    fn coherence_is_zero_for_orthogonal_columns_and_one_for_a_duplicate() {
        let eye = Matrix::eye(4);
        assert!(coherence(&eye) < 1e-12);

        let dup = Matrix::from_col_major(2, 2, vec![1.0, 0.0, 1.0, 0.0]);
        assert!((coherence(&dup) - 1.0).abs() < 1e-12);
    }

    /// The guarantee is real but pessimistic, and the doc comment says so;
    /// this pins the arithmetic behind that claim.
    #[test]
    fn the_coherence_bound_is_conservative_and_the_test_says_by_how_much() {
        let a = sensing(60, 200, 31337);
        let guaranteed = rip_bound_from_coherence(&a);
        let x = sparse_signal(200, &[(7, 1.5), (33, -2.0), (91, 0.75), (140, 3.0), (177, -1.25)]);
        let y = mul(&a, &x);
        let got = omp(&a, &y, 5, 1e-9).unwrap();
        assert!(recovers(&got.x, &x, 1e-6));
        assert!(
            guaranteed < 5,
            "the bound guarantees {guaranteed}, yet 5 recovers -- if this ever fails the \
             bound stopped being pessimistic and the doc comment needs revisiting"
        );
    }

    #[test]
    fn the_errors_name_the_problem_rather_than_the_arithmetic() {
        let a = sensing(10, 20, 1);
        assert_eq!(
            omp(&a, &[1.0, 2.0], 3, 1e-9).unwrap_err(),
            CsError::Mismatch { rows: 10, measurements: 2 }
        );
        let y = vec![0.0; 10];
        assert_eq!(omp(&a, &y, 0, 1e-9).unwrap_err(), CsError::BadSparsity { k: 0, n: 20 });
        assert_eq!(omp(&a, &y, 99, 1e-9).unwrap_err(), CsError::BadSparsity { k: 99, n: 20 });
    }
}
