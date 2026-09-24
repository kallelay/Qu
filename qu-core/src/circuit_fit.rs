//! Fitting an equivalent circuit to measured impedance (§48).
//!
//! [`crate::circuit`] evaluates a circuit whose parameters you already know.
//! This is the inverse problem: the spectrum is measured, the topology is
//! chosen, and the parameters are what the fit has to recover.
//!
//! The four decisions that make this work at all, all of them forced by what
//! impedance data actually looks like rather than by taste:
//!
//! 1. **Magnitude parameters are fitted as `log10` of themselves.** A cell's
//!    resistances are O(1e2) and its double-layer capacitance O(1e-6). One
//!    step size cannot serve both in linear space -- a step that moves `Rct`
//!    by a percent moves `Cdl` by a hundred million percent -- so the fit
//!    either crawls on one parameter or diverges on the other. Logging them
//!    makes every magnitude O(1) and, incidentally, makes a negative
//!    capacitance unrepresentable rather than merely discouraged. Exponents
//!    (`n`, `a`, `g`) are already O(1) and stay linear; see [`ParamRole`].
//! 2. **The residual is weighted by a noise level**, not by the raw
//!    difference. An unweighted complex residual is dominated by whichever
//!    end of the sweep has the largest `|Z|`, which on a typical cell is the
//!    low-frequency tail -- so an unweighted fit optimises the diffusion
//!    branch and ignores the high-frequency arc the measurement was taken
//!    for. With a true per-point sigma, `chi2 ~ 2N` becomes an absolute
//!    standard for "fitted as well as the noise permits" rather than a
//!    number only comparable against itself.
//! 3. **Multiple starts.** The objective is not convex and the standard
//!    failure is a local minimum that looks converged. Restarts are drawn
//!    from a seeded generator, so a fit is reproducible.
//! 4. **Standard errors are reported per parameter**, from the Jacobian at
//!    the solution. A circuit can always be over-specified for the data it
//!    is fitted to -- two time constants too close to resolve, a Warburg
//!    below the lowest measured frequency -- and the symptom is not a bad
//!    chi-squared but an unbounded error bar on the parameters the data does
//!    not constrain. A fit that does not report that is hiding it.

use crate::circuit::{Circuit, ParamRole};
use crate::linalg;
use crate::matrix::Matrix;
use crate::Complex64;

/// Knobs for [`fit`]. [`Default`] is the "no opinion supplied" fit.
#[derive(Clone, Debug)]
pub struct FitOptions {
    pub max_iter: usize,
    pub tol: f64,
    /// Total starts, including the supplied initial guess (which is always
    /// start 1). `1` disables multi-start.
    pub n_starts: usize,
    /// Seeds the restart perturbations, so the whole fit is reproducible.
    pub seed: u64,
    /// Per-parameter bounds in LINEAR space, `None` for the role defaults.
    pub lower: Option<Vec<f64>>,
    pub upper: Option<Vec<f64>>,
}

impl Default for FitOptions {
    fn default() -> Self {
        FitOptions {
            max_iter: 400,
            tol: 1e-12,
            n_starts: 8,
            seed: 0x5eed_c1c1,
            lower: None,
            upper: None,
        }
    }
}

/// What a fit recovered, and how well the data pinned it down.
#[derive(Clone, Debug)]
pub struct FitResult {
    /// Fitted values in LINEAR space, flat left-to-right leaf order.
    pub params: Vec<f64>,
    /// The weighted residual at the solution, interleaved `re, im` per
    /// frequency -- length `2 * freqs.len()`.
    pub residual: Vec<f64>,
    /// Sum of squared weighted residuals. With a true sigma this is the
    /// chi-squared whose expectation is `2N`.
    pub chi2: f64,
    /// `chi2 / (2N - p)`; ~1 when the fit is as good as the noise allows.
    pub chi2_red: f64,
    /// One-sigma error on each parameter, in linear space. `inf` marks a
    /// parameter this data does not constrain.
    pub stderr: Vec<f64>,
    pub iterations: usize,
    pub converged: bool,
    /// How many starts were actually run.
    pub starts: usize,
}

/// Role defaults, in linear space.
///
/// The magnitude window is deliberately enormous (24 decades): it exists to
/// stop the fit walking to zero or to infinity, not to encode a prior about
/// what a resistance can be. An exponent is capped at 1 because above 1 a
/// CPE is not a physical element, and floored just above 0 because at 0 it
/// is a resistor and the branch degenerates.
fn default_bounds(role: ParamRole) -> (f64, f64) {
    match role {
        ParamRole::Magnitude => (1e-12, 1e12),
        ParamRole::Exponent => (1e-3, 1.0),
    }
}

/// Linear -> fit space.
fn to_x(p: f64, role: ParamRole) -> f64 {
    match role {
        ParamRole::Magnitude => p.log10(),
        ParamRole::Exponent => p,
    }
}

/// Fit space -> linear.
fn to_p(x: f64, role: ParamRole) -> f64 {
    match role {
        ParamRole::Magnitude => 10f64.powf(x),
        ParamRole::Exponent => x,
    }
}

/// A small deterministic generator for the restarts.
///
/// Deliberately self-contained rather than reaching for the engine's RNG: a
/// fit has to be reproducible from its `seed` alone, and sharing a global
/// stream with whatever else the caller is doing would make it depend on
/// call order instead.
struct Lcg(u64);

impl Lcg {
    /// Uniform in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// Uniform in `[-span, span]`.
    fn jitter(&mut self, span: f64) -> f64 {
        (2.0 * self.next_f64() - 1.0) * span
    }
}

/// Fit `template`'s parameters to `z` measured at `freqs` (Hz).
///
/// `template` supplies the topology; its own parameter values are ignored
/// (pass them as `p0` if they are the initial guess). `sigma` is the
/// per-point noise level the residual is divided by, one entry per
/// frequency. `p0` is the starting point, in linear space.
pub fn fit(
    template: &Circuit,
    freqs: &[f64],
    z: &[Complex64],
    sigma: &[f64],
    p0: &[f64],
    opts: &FitOptions,
) -> Result<FitResult, String> {
    let np = template.nparam();
    if np == 0 {
        return Err("circuit_fit: the circuit has no parameters to fit".into());
    }
    if freqs.is_empty() {
        return Err("circuit_fit: no data points".into());
    }
    if freqs.len() != z.len() {
        return Err(format!(
            "circuit_fit: {} frequency/frequencies but {} impedance value(s)",
            freqs.len(),
            z.len()
        ));
    }
    if sigma.len() != freqs.len() {
        return Err(format!(
            "circuit_fit: {} weight(s) for {} data point(s)",
            sigma.len(),
            freqs.len()
        ));
    }
    if p0.len() != np {
        return Err(format!(
            "circuit_fit: the circuit has {np} parameter(s) but the initial guess has {}",
            p0.len()
        ));
    }
    for (i, f) in freqs.iter().enumerate() {
        if !f.is_finite() || *f <= 0.0 {
            return Err(format!(
                "circuit_fit: frequency {} is {f}; frequencies must be positive (Hz)",
                i + 1
            ));
        }
    }
    for (i, s) in sigma.iter().enumerate() {
        if !s.is_finite() || *s <= 0.0 {
            return Err(format!(
                "circuit_fit: the noise level at point {} is {s}; it must be positive",
                i + 1
            ));
        }
    }

    let roles = template.param_roles();
    let names = template.param_names();

    // Bounds, then the starting point, both into fit space. A magnitude that
    // starts at zero or below has no log, and defaulting it silently would
    // fit a circuit the caller did not ask for.
    let mut lo_x = Vec::with_capacity(np);
    let mut hi_x = Vec::with_capacity(np);
    for j in 0..np {
        let (mut lo, mut hi) = default_bounds(roles[j]);
        if let Some(l) = &opts.lower {
            if l[j].is_finite() {
                lo = l[j];
            }
        }
        if let Some(h) = &opts.upper {
            if h[j].is_finite() {
                hi = h[j];
            }
        }
        if roles[j] == ParamRole::Magnitude && lo <= 0.0 {
            lo = default_bounds(roles[j]).0;
        }
        if lo >= hi {
            return Err(format!(
                "circuit_fit: bounds for `{}` are [{lo}, {hi}], which is empty",
                names[j]
            ));
        }
        lo_x.push(to_x(lo, roles[j]));
        hi_x.push(to_x(hi, roles[j]));
    }

    let mut x0 = Vec::with_capacity(np);
    for j in 0..np {
        let p = p0[j];
        if !p.is_finite() {
            return Err(format!(
                "circuit_fit: the initial guess for `{}` is {p}",
                names[j]
            ));
        }
        if roles[j] == ParamRole::Magnitude && p <= 0.0 {
            return Err(format!(
                "circuit_fit: the initial guess for `{}` is {p}; magnitude parameters are fitted in log space and must be positive",
                names[j]
            ));
        }
        x0.push(to_x(p, roles[j]).clamp(lo_x[j], hi_x[j]));
    }

    let residual_at = |x: &[f64]| -> Vec<f64> {
        let p: Vec<f64> = (0..np).map(|j| to_p(x[j], roles[j])).collect();
        let c = match template.with_params(&p) {
            Ok(c) => c,
            // Unreachable: `p` is built to length `np` on every call.
            Err(_) => return vec![BLOWUP; 2 * freqs.len()],
        };
        let model = c.spectrum_hz(freqs);
        let mut out = Vec::with_capacity(2 * freqs.len());
        for i in 0..freqs.len() {
            for d in [
                (model[i].re - z[i].re) / sigma[i],
                (model[i].im - z[i].im) / sigma[i],
            ] {
                // A single NaN anywhere in the residual makes the whole
                // step NaN and the fit dies silently at the start point.
                // A large finite value is a bad fit, which is what an
                // element evaluated outside its domain actually is.
                out.push(if d.is_finite() { d } else { BLOWUP });
            }
        }
        out
    };

    let mut rng = Lcg(opts.seed);
    let starts = opts.n_starts.max(1);
    let mut best: Option<(Vec<f64>, Vec<f64>, f64, usize, bool)> = None;

    for s in 0..starts {
        let mut x = x0.clone();
        if s > 0 {
            for j in 0..np {
                // One decade either way for a magnitude, a modest nudge for
                // an exponent -- restarts should sample the basin structure,
                // not abandon the caller's guess entirely.
                let span = match roles[j] {
                    ParamRole::Magnitude => 1.0,
                    ParamRole::Exponent => 0.15,
                };
                x[j] = (x[j] + rng.jitter(span)).clamp(lo_x[j], hi_x[j]);
            }
        }
        let r = linalg::nonlinear_least_squares(
            &residual_at,
            &x,
            Some(&lo_x),
            Some(&hi_x),
            opts.max_iter,
            opts.tol,
        );
        let r = match r {
            Ok(r) => r,
            // One start failing (a degenerate Jacobian at a silly
            // perturbation, say) is not the fit failing; only every start
            // failing is.
            Err(_) => continue,
        };
        if !r.cost.is_finite() {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, _, c, _, _)) => r.cost < *c,
        };
        if better {
            best = Some((
                r.parameters,
                r.residual,
                r.cost,
                r.iterations,
                r.converged,
            ));
        }
    }

    let (x_best, residual, chi2, iterations, converged) = best.ok_or_else(|| {
        "circuit_fit: no start converged -- check the initial guess and the topology".to_string()
    })?;

    let params: Vec<f64> = (0..np).map(|j| to_p(x_best[j], roles[j])).collect();
    let dof = 2 * freqs.len();
    let chi2_red = if dof > np {
        chi2 / (dof - np) as f64
    } else {
        f64::NAN
    };
    let stderr = standard_errors(&residual_at, &x_best, &params, &roles, chi2_red);

    Ok(FitResult {
        params,
        residual,
        chi2,
        chi2_red,
        stderr,
        iterations,
        converged,
        starts,
    })
}

/// The residual value substituted for a non-finite one. Large enough that no
/// real fit reaches it, small enough that its square is nowhere near
/// overflow.
const BLOWUP: f64 = 1e8;

/// One-sigma parameter errors from the Jacobian at the solution.
///
/// `cov_x = chi2_red * (J^T J)^-1`, computed through the SVD of `J` rather
/// than by forming and inverting `J^T J` -- which squares the condition
/// number, and a circuit fit is exactly where that matters: two unresolvable
/// time constants make `J` numerically rank-deficient by construction.
///
/// A direction whose singular value has collapsed gets `inf`, not a large
/// number and not zero. That is the honest answer -- the data does not
/// constrain that combination at all -- and it is the opposite of what a
/// pseudo-inverse would report, which is zero error on the very parameters
/// that are unidentifiable.
fn standard_errors(
    residual_at: &dyn Fn(&[f64]) -> Vec<f64>,
    x: &[f64],
    params: &[f64],
    roles: &[ParamRole],
    chi2_red: f64,
) -> Vec<f64> {
    let np = x.len();
    let r0 = residual_at(x);
    let m = r0.len();
    if m == 0 || !chi2_red.is_finite() {
        return vec![f64::INFINITY; np];
    }
    // Column-major Jacobian: column j is d residual / d x_j.
    let mut data = Vec::with_capacity(m * np);
    for j in 0..np {
        let h = 1e-6 * x[j].abs().max(1.0);
        let mut xp = x.to_vec();
        xp[j] += h;
        let rp = residual_at(&xp);
        if rp.len() != m {
            return vec![f64::INFINITY; np];
        }
        for i in 0..m {
            data.push((rp[i] - r0[i]) / h);
        }
    }
    let jac = match Matrix::from_col_major_checked(m, np, data) {
        Ok(j) => j,
        Err(_) => return vec![f64::INFINITY; np],
    };
    let svd = match linalg::svd(&jac) {
        Ok(s) => s,
        Err(_) => return vec![f64::INFINITY; np],
    };
    let s = &svd.singular_values;
    let smax = s.iter().cloned().fold(0.0f64, f64::max);
    if smax <= 0.0 {
        return vec![f64::INFINITY; np];
    }
    let cutoff = smax * (m.max(np) as f64) * f64::EPSILON;
    let k = s.len();
    let vt = svd.v_t.as_slice(); // (k, np), column-major
    let vt_rows = svd.v_t.rows();
    let mut out = Vec::with_capacity(np);
    for j in 0..np {
        let mut var = 0.0f64;
        for (kk, sk) in s.iter().enumerate().take(k) {
            if kk >= vt_rows {
                break;
            }
            let v = vt[j * vt_rows + kk];
            if *sk <= cutoff {
                if v.abs() > 1e-12 {
                    var = f64::INFINITY;
                }
                continue;
            }
            var += (v / sk) * (v / sk);
        }
        let sd_x = (var * chi2_red).sqrt();
        // Back out of fit space: p = 10^x has dp/dx = p ln(10).
        out.push(match roles[j] {
            ParamRole::Magnitude => params[j].abs() * std::f64::consts::LN_10 * sd_x,
            ParamRole::Exponent => sd_x,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::Element;

    /// `Rs - (Rct | Cdl)`, the textbook Randles cell without diffusion.
    fn randles(rs: f64, rct: f64, cdl: f64) -> Circuit {
        Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(rs)),
            Circuit::Parallel(vec![
                Circuit::Leaf(Element::Resistor(rct)),
                Circuit::Leaf(Element::Capacitor(cdl)),
            ]),
        ])
    }

    fn logspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                10f64.powf(lo + t * (hi - lo))
            })
            .collect()
    }

    #[test]
    fn param_names_number_by_occurrence() {
        let c = Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(1.0)),
            Circuit::Parallel(vec![
                Circuit::Leaf(Element::Resistor(2.0)),
                Circuit::Leaf(Element::Cpe { q: 1e-5, n: 0.8 }),
            ]),
        ]);
        assert_eq!(c.param_names(), vec!["R1", "R2", "Q1_q", "Q1_n"]);
        assert_eq!(
            c.param_roles(),
            vec![
                ParamRole::Magnitude,
                ParamRole::Magnitude,
                ParamRole::Magnitude,
                ParamRole::Exponent
            ]
        );
    }

    #[test]
    fn with_params_preserves_topology_and_checks_length() {
        let c = randles(1.0, 2.0, 3.0);
        let d = c.with_params(&[10.0, 20.0, 30.0]).unwrap();
        assert_eq!(d, randles(10.0, 20.0, 30.0));
        assert!(c.with_params(&[1.0, 2.0]).is_err());
        assert!(c.with_params(&[1.0, 2.0, 3.0, 4.0]).is_err());
    }

    /// The test this module exists to pass: exact data from a known circuit
    /// must come back as the known parameters, from a guess that is an order
    /// of magnitude off on every one of them.
    #[test]
    fn recovers_a_randles_cell_from_clean_data() {
        let truth = randles(10.0, 100.0, 1e-5);
        let freqs = logspace(-1.0, 5.0, 60);
        let z = truth.spectrum_hz(&freqs);
        let sigma: Vec<f64> = z.iter().map(|c| c.magnitude()).collect();
        let got = fit(
            &truth,
            &freqs,
            &z,
            &sigma,
            &[1.0, 1000.0, 1e-4],
            &FitOptions::default(),
        )
        .unwrap();
        for (got, want) in got.params.iter().zip([10.0, 100.0, 1e-5]) {
            assert!(
                (got / want - 1.0).abs() < 1e-4,
                "recovered {got}, wanted {want}"
            );
        }
        assert!(got.chi2 < 1e-12, "chi2 = {}", got.chi2);
    }

    /// A CPE branch exercises the exponent role, which is the one parameter
    /// kind NOT fitted in log space.
    #[test]
    fn recovers_a_cpe_exponent() {
        let truth = Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(5.0)),
            Circuit::Parallel(vec![
                Circuit::Leaf(Element::Resistor(50.0)),
                Circuit::Leaf(Element::Cpe { q: 2e-5, n: 0.85 }),
            ]),
        ]);
        let freqs = logspace(-1.0, 5.0, 60);
        let z = truth.spectrum_hz(&freqs);
        let sigma: Vec<f64> = z.iter().map(|c| c.magnitude()).collect();
        let got = fit(
            &truth,
            &freqs,
            &z,
            &sigma,
            &[1.0, 10.0, 1e-4, 0.5],
            &FitOptions::default(),
        )
        .unwrap();
        let want = [5.0, 50.0, 2e-5, 0.85];
        for (g, w) in got.params.iter().zip(want) {
            assert!((g / w - 1.0).abs() < 1e-3, "recovered {g}, wanted {w}");
        }
    }

    /// An unidentifiable parameter has to be REPORTED as unidentifiable.
    ///
    /// Two resistors in series are one resistor: only their sum is
    /// constrained, so each one's error bar is unbounded however good the
    /// fit's chi-squared looks.
    #[test]
    fn degenerate_parameters_get_infinite_error_bars() {
        let truth = Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(30.0)),
            Circuit::Leaf(Element::Resistor(70.0)),
        ]);
        let freqs = logspace(0.0, 4.0, 30);
        let z = truth.spectrum_hz(&freqs);
        let sigma: Vec<f64> = z.iter().map(|c| c.magnitude() * 0.01).collect();
        let got = fit(
            &truth,
            &freqs,
            &z,
            &sigma,
            &[10.0, 10.0],
            &FitOptions {
                n_starts: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            (got.params[0] + got.params[1] - 100.0).abs() < 1e-6,
            "the SUM is what the data constrains: {:?}",
            got.params
        );
        assert!(
            got.stderr.iter().all(|s| !s.is_finite()),
            "both errors should be unbounded, got {:?}",
            got.stderr
        );
    }

    #[test]
    fn a_negative_initial_magnitude_is_rejected_not_defaulted() {
        let truth = randles(10.0, 100.0, 1e-5);
        let freqs = logspace(0.0, 4.0, 20);
        let z = truth.spectrum_hz(&freqs);
        let sigma = vec![1.0; freqs.len()];
        let err = fit(
            &truth,
            &freqs,
            &z,
            &sigma,
            &[-1.0, 100.0, 1e-5],
            &FitOptions::default(),
        )
        .unwrap_err();
        assert!(err.contains("R1"), "error should name the parameter: {err}");
    }

    /// Same seed, same answer -- the multi-start must not make a fit
    /// depend on anything but its inputs.
    #[test]
    fn multistart_is_reproducible() {
        let truth = randles(10.0, 100.0, 1e-5);
        let freqs = logspace(-1.0, 4.0, 40);
        let z = truth.spectrum_hz(&freqs);
        let sigma: Vec<f64> = z.iter().map(|c| c.magnitude()).collect();
        let run = || {
            fit(
                &truth,
                &freqs,
                &z,
                &sigma,
                &[1.0, 1.0, 1e-3],
                &FitOptions::default(),
            )
            .unwrap()
            .params
        };
        assert_eq!(run(), run());
    }
}
