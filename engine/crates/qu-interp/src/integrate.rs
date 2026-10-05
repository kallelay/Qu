//! Numerical integration and ordinary differential equations.
//!
//! Flat builtins (core numerics are not namespaced): `trapz`, `cumtrapz`,
//! `simpson`, `quad`, `quad_info`, `ode45`, `ode23`, `ode_stiff`, `rk4`.
//!
//! The numerics (everything above the `impl Interp` block) know nothing about
//! the interpreter: they take Rust closures, so they are unit-tested against
//! analytic solutions here and the interpreter layer only converts values.
//!
//! Conventions worth knowing:
//! * Sums are Neumaier-compensated (`crate::ksum`), like every other
//!   reduction in the engine.
//! * `quad` is adaptive Gauss-Kronrod (G7K15, QUADPACK's constants and error
//!   scaling) with global bisection of the worst interval. Infinite limits
//!   are mapped to a finite interval by a change of variable.
//! * `ode45` is Dormand-Prince 5(4) with the 4th-order continuous extension
//!   (MATLAB's `ntrp45`), `ode23` is Bogacki-Shampine 3(2) with cubic Hermite
//!   output, `ode_stiff` is the Shampine-Reichelt Rosenbrock 2(3) pair
//!   (MATLAB's `ode23s`) with a forward-difference Jacobian.
//! * Events are located on the dense output by Illinois' method; at most one
//!   sign change per step is detected.

use crate::{arg_get, e, ksum, style_entry, text_arg, to_vec, EvalError, Interp, Value, R};
use qu_core::matrix::Matrix;
use std::sync::Arc;

// ====================================================================
// Trapezoid and Simpson on sampled data
// ====================================================================

/// Integral of `y` by the trapezoid rule; `x = None` means unit spacing.
pub(crate) fn trapz_slice(x: Option<&[f64]>, y: &[f64]) -> f64 {
    if y.len() < 2 {
        return 0.0;
    }
    ksum((0..y.len() - 1).map(|i| {
        let dx = x.map_or(1.0, |x| x[i + 1] - x[i]);
        0.5 * dx * (y[i] + y[i + 1])
    }))
}

/// Running trapezoid integral, starting at 0, same length as `y`.
pub(crate) fn cumtrapz_slice(x: Option<&[f64]>, y: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(y.len());
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for i in 0..y.len() {
        if i > 0 {
            let dx = x.map_or(1.0, |x| x[i] - x[i - 1]);
            let v = 0.5 * dx * (y[i - 1] + y[i]);
            let t = s + v;
            if s.abs() >= v.abs() {
                c += (s - t) + v;
            } else {
                c += (v - t) + s;
            }
            s = t;
        }
        out.push(s + c);
    }
    out
}

/// Composite Simpson's rule on a (possibly non-uniform) grid.
///
/// Intervals are taken in pairs with the generalised three-point formula
/// (which reduces to `h/3 (y0 + 4 y1 + y2)` on a uniform grid). With an ODD
/// number of intervals one is left over; it is integrated with the quadratic
/// through the last three points (Cartwright's correction), so the result is
/// still exact for quadratics. Two points fall back to the trapezoid.
pub(crate) fn simpson_slice(x: Option<&[f64]>, y: &[f64]) -> Result<f64, String> {
    let n = y.len();
    if n < 2 {
        return Ok(0.0);
    }
    let xs = |i: usize| x.map_or(i as f64, |x| x[i]);
    if n == 2 {
        return Ok(0.5 * (xs(1) - xs(0)) * (y[0] + y[1]));
    }
    let intervals = n - 1;
    let pairs = intervals / 2;
    let mut terms = Vec::with_capacity(pairs + 1);
    for p in 0..pairs {
        let i = 2 * p;
        let h0 = xs(i + 1) - xs(i);
        let h1 = xs(i + 2) - xs(i + 1);
        if h0 == 0.0 || h1 == 0.0 {
            return Err("simpson: repeated x values (zero-width interval)".into());
        }
        let hs = h0 + h1;
        terms.push(
            hs / 6.0
                * ((2.0 - h1 / h0) * y[i] + hs * hs / (h0 * h1) * y[i + 1] + (2.0 - h0 / h1) * y[i + 2]),
        );
    }
    if intervals % 2 == 1 {
        let h0 = xs(n - 2) - xs(n - 3);
        let h1 = xs(n - 1) - xs(n - 2);
        if h0 == 0.0 || h1 == 0.0 {
            return Err("simpson: repeated x values (zero-width interval)".into());
        }
        let alpha = (2.0 * h1 * h1 + 3.0 * h0 * h1) / (6.0 * (h0 + h1));
        let beta = (h1 * h1 + 3.0 * h0 * h1) / (6.0 * h0);
        let eta = h1 * h1 * h1 / (6.0 * h0 * (h0 + h1));
        terms.push(alpha * y[n - 1] + beta * y[n - 2] - eta * y[n - 3]);
    }
    Ok(ksum(terms))
}

// ====================================================================
// Adaptive Gauss-Kronrod (G7K15)
// ====================================================================

const XGK: [f64; 8] = [
    0.991455371120812639206854697526329,
    0.949107912342758524526189684047851,
    0.864864423359769072789712788640926,
    0.741531185599394439863864773280788,
    0.586087235467691130294144838258730,
    0.405845151377397166906606412076961,
    0.207784955007898467600689403773245,
    0.0,
];
const WGK: [f64; 8] = [
    0.022935322010529224963732008058970,
    0.063092092629978553290700663189204,
    0.104790010322250183839876322541518,
    0.140653259715525918745189590510238,
    0.169004726639267902826583426598550,
    0.190350578064785409913256402421014,
    0.204432940075298892414161999234649,
    0.209482141084727828012999174891714,
];
const WG: [f64; 4] = [
    0.129484966168869693270611432679082,
    0.279705391489276667901467771423780,
    0.381830050505118944950369775488975,
    0.417959183673469387755102040816327,
];

type ScalarFn<'a> = &'a mut dyn FnMut(f64) -> Result<f64, String>;

/// One 15-point Kronrod rule (with its embedded 7-point Gauss rule) on
/// `[a, b]`. Returns `(result, abserr)` with QUADPACK's error scaling.
fn qk15(f: ScalarFn, a: f64, b: f64) -> Result<(f64, f64), String> {
    let centr = 0.5 * (a + b);
    let hlgth = 0.5 * (b - a);
    let dhlgth = hlgth.abs();
    let fc = f(centr)?;
    let mut resg = fc * WG[3];
    let mut resk = fc * WGK[7];
    let mut resabs = resk.abs();
    let mut fv1 = [0.0f64; 7];
    let mut fv2 = [0.0f64; 7];
    for j in 0..3 {
        let jtw = 2 * j + 1;
        let absc = hlgth * XGK[jtw];
        let (f1, f2) = (f(centr - absc)?, f(centr + absc)?);
        fv1[jtw] = f1;
        fv2[jtw] = f2;
        resg += WG[j] * (f1 + f2);
        resk += WGK[jtw] * (f1 + f2);
        resabs += WGK[jtw] * (f1.abs() + f2.abs());
    }
    for j in 0..4 {
        let jtwm1 = 2 * j;
        let absc = hlgth * XGK[jtwm1];
        let (f1, f2) = (f(centr - absc)?, f(centr + absc)?);
        fv1[jtwm1] = f1;
        fv2[jtwm1] = f2;
        resk += WGK[jtwm1] * (f1 + f2);
        resabs += WGK[jtwm1] * (f1.abs() + f2.abs());
    }
    let reskh = 0.5 * resk;
    let mut resasc = WGK[7] * (fc - reskh).abs();
    for j in 0..7 {
        resasc += WGK[j] * ((fv1[j] - reskh).abs() + (fv2[j] - reskh).abs());
    }
    let result = resk * hlgth;
    resabs *= dhlgth;
    resasc *= dhlgth;
    let mut abserr = ((resk - resg) * hlgth).abs();
    if resasc != 0.0 && abserr != 0.0 {
        abserr = resasc * (1.0f64).min((200.0 * abserr / resasc).powf(1.5));
    }
    if resabs > f64::MIN_POSITIVE / (50.0 * f64::EPSILON) {
        abserr = abserr.max(f64::EPSILON * 50.0 * resabs);
    }
    Ok((result, abserr))
}

pub(crate) struct QuadOut {
    pub value: f64,
    pub error: f64,
    pub nfev: usize,
    pub intervals: usize,
    /// `"ok"`, `"max_depth"`, `"max_intervals"` or `"roundoff"`.
    pub status: &'static str,
}

const MAX_INTERVALS: usize = 2000;

/// Adaptive G7K15 on `[a, b]` (either end may be infinite).
///
/// Converged when the summed error estimate is at most `tol * max(1, |I|)`.
pub(crate) fn quad_adaptive(
    f: ScalarFn,
    a: f64,
    b: f64,
    tol: f64,
    maxdepth: usize,
) -> Result<QuadOut, String> {
    if a.is_nan() || b.is_nan() {
        return Err("quad: limits must not be NaN".into());
    }
    if a == b {
        return Ok(QuadOut { value: 0.0, error: 0.0, nfev: 0, intervals: 0, status: "ok" });
    }
    if a > b {
        let mut o = quad_adaptive(f, b, a, tol, maxdepth)?;
        o.value = -o.value;
        return Ok(o);
    }
    let mut nfev = 0usize;
    let mut counted = |x: f64, f: ScalarFn| -> Result<f64, String> {
        nfev += 1;
        let v = f(x)?;
        if !v.is_finite() {
            return Err(format!("quad: f returned {v} at x = {x} (the integrand must be finite on the whole range)"));
        }
        Ok(v)
    };
    let (lo, hi, out): (f64, f64, Box<dyn FnMut(f64) -> Result<f64, String> + '_>) = match (a.is_finite(), b.is_finite()) {
        (true, true) => (a, b, Box::new(|x| counted(x, &mut *f))),
        (true, false) => (
            0.0,
            1.0,
            Box::new(move |t| {
                let u = 1.0 - t;
                Ok(counted(a + t / u, &mut *f)? / (u * u))
            }),
        ),
        (false, true) => (
            0.0,
            1.0,
            Box::new(move |t| {
                let u = 1.0 - t;
                Ok(counted(b - t / u, &mut *f)? / (u * u))
            }),
        ),
        (false, false) => (
            -1.0,
            1.0,
            Box::new(move |t| {
                let u = 1.0 - t * t;
                Ok(counted(t / u, &mut *f)? * (1.0 + t * t) / (u * u))
            }),
        ),
    };
    let mut g = out;
    struct Iv {
        a: f64,
        b: f64,
        r: f64,
        e: f64,
        d: usize,
    }
    let (r0, e0) = qk15(&mut *g, lo, hi)?;
    let mut ivs = vec![Iv { a: lo, b: hi, r: r0, e: e0, d: 0 }];
    let mut status = "ok";
    loop {
        let total = ksum(ivs.iter().map(|i| i.r));
        let err = ksum(ivs.iter().map(|i| i.e));
        if err <= tol * total.abs().max(1.0) {
            break;
        }
        let mut w = 0;
        for (k, iv) in ivs.iter().enumerate() {
            if iv.e > ivs[w].e {
                w = k;
            }
        }
        if ivs[w].d >= maxdepth {
            status = "max_depth";
            break;
        }
        if ivs.len() >= MAX_INTERVALS {
            status = "max_intervals";
            break;
        }
        let (ia, ib, d) = (ivs[w].a, ivs[w].b, ivs[w].d);
        let m = 0.5 * (ia + ib);
        if !(m > ia && m < ib) {
            status = "roundoff";
            break;
        }
        let (rl, el) = qk15(&mut *g, ia, m)?;
        let (rr, er) = qk15(&mut *g, m, ib)?;
        ivs[w] = Iv { a: ia, b: m, r: rl, e: el, d: d + 1 };
        ivs.push(Iv { a: m, b: ib, r: rr, e: er, d: d + 1 });
    }
    let value = ksum(ivs.iter().map(|i| i.r));
    let error = ksum(ivs.iter().map(|i| i.e));
    let intervals = ivs.len();
    drop(g);
    Ok(QuadOut { value, error, nfev, intervals, status })
}

// ====================================================================
// ODE solvers
// ====================================================================

type RhsFn<'a> = &'a mut dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, String>;
type EventFn<'a> = &'a mut dyn FnMut(f64, &[f64]) -> Result<f64, String>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Method {
    Dp45,
    Bs23,
    Stiff,
}

pub(crate) struct OdeOpts {
    pub rtol: f64,
    pub atol: f64,
    pub max_step: f64,
    pub max_steps: usize,
}

pub(crate) struct OdeOut {
    pub t: Vec<f64>,
    pub y: Vec<Vec<f64>>,
    pub steps: usize,
    pub rejected: usize,
    pub nfev: usize,
    /// `"ok"`, `"event"`, `"max_steps"` or `"step_too_small"`.
    pub status: &'static str,
    pub t_events: Vec<f64>,
    pub y_events: Vec<Vec<f64>>,
}

struct Ctx<'a> {
    rhs: RhsFn<'a>,
    nfev: usize,
}
impl Ctx<'_> {
    fn f(&mut self, t: f64, y: &[f64]) -> Result<Vec<f64>, String> {
        self.nfev += 1;
        (self.rhs)(t, y)
    }
}

/// Dense output for one accepted step.
enum Dense {
    Dp { t0: f64, h: f64, y0: Vec<f64>, k: Vec<Vec<f64>> },
    Hermite { t0: f64, h: f64, y0: Vec<f64>, y1: Vec<f64>, f0: Vec<f64>, f1: Vec<f64> },
    Rosen { t0: f64, h: f64, y0: Vec<f64>, k1: Vec<f64>, k2: Vec<f64> },
}

/// MATLAB's `ntrp45` coefficients: stage i contributes
/// `sum_j BI[i][j] s^(j+1)` to the interpolant.
const BI: [[f64; 4]; 7] = [
    [1.0, -183.0 / 64.0, 37.0 / 12.0, -145.0 / 128.0],
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 1500.0 / 371.0, -1000.0 / 159.0, 1000.0 / 371.0],
    [0.0, -125.0 / 32.0, 125.0 / 12.0, -375.0 / 64.0],
    [0.0, 9477.0 / 3392.0, -729.0 / 106.0, 25515.0 / 6784.0],
    [0.0, -11.0 / 7.0, 11.0 / 3.0, -55.0 / 28.0],
    [0.0, 3.0 / 2.0, -4.0, 5.0 / 2.0],
];
const ROSEN_D: f64 = 0.292_893_218_813_452_5; // 1 / (2 + sqrt 2)

impl Dense {
    fn eval(&self, t: f64) -> Vec<f64> {
        match self {
            Dense::Dp { t0, h, y0, k } => {
                let s = (t - t0) / h;
                let pw = [s, s * s, s * s * s, s * s * s * s];
                let mut y = y0.clone();
                for (i, ki) in k.iter().enumerate() {
                    let c: f64 = (0..4).map(|j| BI[i][j] * pw[j]).sum();
                    if c != 0.0 {
                        for (yv, kv) in y.iter_mut().zip(ki) {
                            *yv += h * c * kv;
                        }
                    }
                }
                y
            }
            Dense::Hermite { t0, h, y0, y1, f0, f1 } => {
                let s = (t - t0) / h;
                let (s2, s3) = (s * s, s * s * s);
                let (h00, h10, h01, h11) = (2.0 * s3 - 3.0 * s2 + 1.0, s3 - 2.0 * s2 + s, -2.0 * s3 + 3.0 * s2, s3 - s2);
                (0..y0.len()).map(|i| h00 * y0[i] + h * h10 * f0[i] + h01 * y1[i] + h * h11 * f1[i]).collect()
            }
            Dense::Rosen { t0, h, y0, k1, k2 } => {
                let s = (t - t0) / h;
                let den = 1.0 - 2.0 * ROSEN_D;
                let c1 = s * (1.0 - s) / den;
                let c2 = s * (s - 2.0 * ROSEN_D) / den;
                (0..y0.len()).map(|i| y0[i] + h * (c1 * k1[i] + c2 * k2[i])).collect()
            }
        }
    }
}

struct StepOut {
    ynew: Vec<f64>,
    fnew: Vec<f64>,
    err: Vec<f64>,
    dense: Dense,
}

fn axpy(y: &[f64], a: f64, k: &[f64]) -> Vec<f64> {
    y.iter().zip(k).map(|(y, k)| y + a * k).collect()
}

fn dp_step(ctx: &mut Ctx, t: f64, y: &[f64], f0: &[f64], h: f64) -> Result<StepOut, String> {
    let n = y.len();
    let comb = |ks: &[&Vec<f64>], cs: &[f64]| -> Vec<f64> {
        (0..n).map(|i| y[i] + h * ks.iter().zip(cs).map(|(k, c)| c * k[i]).sum::<f64>()).collect()
    };
    let k1 = f0.to_vec();
    let k2 = ctx.f(t + h / 5.0, &comb(&[&k1], &[1.0 / 5.0]))?;
    let k3 = ctx.f(t + 3.0 * h / 10.0, &comb(&[&k1, &k2], &[3.0 / 40.0, 9.0 / 40.0]))?;
    let k4 = ctx.f(t + 4.0 * h / 5.0, &comb(&[&k1, &k2, &k3], &[44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0]))?;
    let k5 = ctx.f(
        t + 8.0 * h / 9.0,
        &comb(
            &[&k1, &k2, &k3, &k4],
            &[19372.0 / 6561.0, -25360.0 / 2187.0, 64448.0 / 6561.0, -212.0 / 729.0],
        ),
    )?;
    let k6 = ctx.f(
        t + h,
        &comb(
            &[&k1, &k2, &k3, &k4, &k5],
            &[9017.0 / 3168.0, -355.0 / 33.0, 46732.0 / 5247.0, 49.0 / 176.0, -5103.0 / 18656.0],
        ),
    )?;
    let b = [35.0 / 384.0, 0.0, 500.0 / 1113.0, 125.0 / 192.0, -2187.0 / 6784.0, 11.0 / 84.0];
    let ynew = comb(&[&k1, &k2, &k3, &k4, &k5, &k6], &b);
    let k7 = ctx.f(t + h, &ynew)?;
    let e = [71.0 / 57600.0, 0.0, -71.0 / 16695.0, 71.0 / 1920.0, -17253.0 / 339200.0, 22.0 / 525.0, -1.0 / 40.0];
    let ks = [&k1, &k2, &k3, &k4, &k5, &k6, &k7];
    let err: Vec<f64> = (0..n).map(|i| h * ks.iter().zip(e).map(|(k, c)| c * k[i]).sum::<f64>()).collect();
    let dense = Dense::Dp { t0: t, h, y0: y.to_vec(), k: vec![k1, k2, k3, k4, k5, k6, k7.clone()] };
    Ok(StepOut { ynew, fnew: k7, err, dense })
}

fn bs_step(ctx: &mut Ctx, t: f64, y: &[f64], f0: &[f64], h: f64) -> Result<StepOut, String> {
    let n = y.len();
    let k1 = f0.to_vec();
    let k2 = ctx.f(t + 0.5 * h, &axpy(y, 0.5 * h, &k1))?;
    let k3 = ctx.f(t + 0.75 * h, &axpy(y, 0.75 * h, &k2))?;
    let ynew: Vec<f64> = (0..n).map(|i| y[i] + h * (2.0 / 9.0 * k1[i] + 1.0 / 3.0 * k2[i] + 4.0 / 9.0 * k3[i])).collect();
    let k4 = ctx.f(t + h, &ynew)?;
    let err: Vec<f64> = (0..n)
        .map(|i| h * (-5.0 / 72.0 * k1[i] + 1.0 / 12.0 * k2[i] + 1.0 / 9.0 * k3[i] - 0.125 * k4[i]))
        .collect();
    let dense = Dense::Hermite { t0: t, h, y0: y.to_vec(), y1: ynew.clone(), f0: k1, f1: k4.clone() };
    Ok(StepOut { ynew, fnew: k4, err, dense })
}

/// LU factorisation with partial pivoting (row-major `n x n`).
fn lu_factor(mut a: Vec<f64>, n: usize) -> Option<(Vec<f64>, Vec<usize>)> {
    let mut piv: Vec<usize> = (0..n).collect();
    for c in 0..n {
        let (mut p, mut best) = (c, a[c * n + c].abs());
        for r in c + 1..n {
            if a[r * n + c].abs() > best {
                best = a[r * n + c].abs();
                p = r;
            }
        }
        if !(best > 0.0) || !best.is_finite() {
            return None;
        }
        if p != c {
            for j in 0..n {
                a.swap(c * n + j, p * n + j);
            }
            piv.swap(c, p);
        }
        for r in c + 1..n {
            let m = a[r * n + c] / a[c * n + c];
            a[r * n + c] = m;
            for j in c + 1..n {
                a[r * n + j] -= m * a[c * n + j];
            }
        }
    }
    Some((a, piv))
}

fn lu_solve(lu: &(Vec<f64>, Vec<usize>), b: &[f64]) -> Vec<f64> {
    let (a, piv) = lu;
    let n = b.len();
    let mut x: Vec<f64> = piv.iter().map(|&p| b[p]).collect();
    for r in 1..n {
        for j in 0..r {
            x[r] -= a[r * n + j] * x[j];
        }
    }
    for r in (0..n).rev() {
        for j in r + 1..n {
            x[r] -= a[r * n + j] * x[j];
        }
        x[r] /= a[r * n + r];
    }
    x
}

/// Jacobian and time-derivative cached at the last accepted point.
struct JacT {
    j: Vec<f64>,
    tderiv: Vec<f64>,
}

fn jac_t(ctx: &mut Ctx, t: f64, y: &[f64], f0: &[f64], h: f64, ythresh: f64) -> Result<JacT, String> {
    let n = y.len();
    let mut j = vec![0.0; n * n];
    let sq = f64::EPSILON.sqrt();
    let mut yp = y.to_vec();
    for c in 0..n {
        let del = sq * y[c].abs().max(ythresh);
        yp[c] = y[c] + del;
        let fp = ctx.f(t, &yp)?;
        yp[c] = y[c];
        for r in 0..n {
            j[r * n + c] = (fp[r] - f0[r]) / del;
        }
    }
    let dt = sq * t.abs().max(h.abs());
    let dt = if h < 0.0 { -dt } else { dt };
    let ft = ctx.f(t + dt, y)?;
    let tderiv = (0..n).map(|i| (ft[i] - f0[i]) / dt).collect();
    Ok(JacT { j, tderiv })
}

/// Shampine-Reichelt Rosenbrock 2(3) step (MATLAB's `ode23s`).
fn rosen_step(ctx: &mut Ctx, t: f64, y: &[f64], f0: &[f64], h: f64, jt: &JacT) -> Result<Option<StepOut>, String> {
    let n = y.len();
    let d = ROSEN_D;
    let e32 = 6.0 + std::f64::consts::SQRT_2;
    let mut w = vec![0.0; n * n];
    for r in 0..n {
        for c in 0..n {
            w[r * n + c] = (if r == c { 1.0 } else { 0.0 }) - h * d * jt.j[r * n + c];
        }
    }
    let lu = match lu_factor(w, n) {
        Some(l) => l,
        None => return Ok(None),
    };
    let rhs1: Vec<f64> = (0..n).map(|i| f0[i] + h * d * jt.tderiv[i]).collect();
    let k1 = lu_solve(&lu, &rhs1);
    let f1 = ctx.f(t + 0.5 * h, &axpy(y, 0.5 * h, &k1))?;
    let r2: Vec<f64> = (0..n).map(|i| f1[i] - k1[i]).collect();
    let s2 = lu_solve(&lu, &r2);
    let k2: Vec<f64> = (0..n).map(|i| s2[i] + k1[i]).collect();
    let ynew = axpy(y, h, &k2);
    let f2 = ctx.f(t + h, &ynew)?;
    let r3: Vec<f64> = (0..n)
        .map(|i| f2[i] - e32 * (k2[i] - f1[i]) - 2.0 * (k1[i] - f0[i]) + h * d * jt.tderiv[i])
        .collect();
    let k3 = lu_solve(&lu, &r3);
    let err: Vec<f64> = (0..n).map(|i| h / 6.0 * (k1[i] - 2.0 * k2[i] + k3[i])).collect();
    let dense = Dense::Rosen { t0: t, h, y0: y.to_vec(), k1, k2 };
    Ok(Some(StepOut { ynew, fnew: f2, err, dense }))
}

fn err_norm(err: &[f64], y: &[f64], ynew: &[f64], o: &OdeOpts) -> f64 {
    if err.is_empty() {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..err.len() {
        let sc = o.atol + o.rtol * y[i].abs().max(ynew[i].abs());
        let r = err[i] / sc;
        acc += r * r;
    }
    let v = (acc / err.len() as f64).sqrt();
    if v.is_finite() {
        v
    } else {
        f64::INFINITY
    }
}

fn rms(v: &[f64], y: &[f64], o: &OdeOpts) -> f64 {
    let acc: f64 = v.iter().zip(y).map(|(v, y)| (v / (o.atol + o.rtol * y.abs())).powi(2)).sum();
    (acc / v.len().max(1) as f64).sqrt()
}

/// Checks `t_eval`-style output times against the span.
pub(crate) fn check_outputs(who: &str, outs: &[f64], t0: f64, tf: f64) -> Result<(), String> {
    let dir = if tf > t0 { 1.0 } else { -1.0 };
    let (lo, hi) = (t0.min(tf), t0.max(tf));
    let tol = 1e-12 * (hi - lo).max(f64::MIN_POSITIVE);
    for (i, &t) in outs.iter().enumerate() {
        if !t.is_finite() {
            return Err(format!("{who}: output times must be finite"));
        }
        if t < lo - tol || t > hi + tol {
            return Err(format!("{who}: output time {t} lies outside the span [{t0}, {tf}]"));
        }
        if i > 0 && dir * (t - outs[i - 1]) <= 0.0 {
            return Err(format!("{who}: output times must be strictly monotone in the direction of integration"));
        }
    }
    Ok(())
}

/// Adaptive solver driver shared by `ode45`, `ode23` and `ode_stiff`.
///
/// `outputs`: times to report (strictly monotone, inside the span); `None`
/// reports every accepted step. `event`: `(g, terminal)`.
pub(crate) fn solve_adaptive(
    method: Method,
    rhs: RhsFn,
    t0: f64,
    tf: f64,
    y0: &[f64],
    outputs: Option<&[f64]>,
    opts: &OdeOpts,
    mut event: Option<(EventFn, bool)>,
) -> Result<OdeOut, String> {
    let n = y0.len();
    let dir = if tf > t0 { 1.0 } else { -1.0 };
    let span = (tf - t0).abs();
    let pexp = match method {
        Method::Dp45 => 1.0 / 5.0,
        _ => 1.0 / 3.0,
    };
    let mut ctx = Ctx { rhs, nfev: 0 };
    let mut t = t0;
    let mut y = y0.to_vec();
    let mut f0 = ctx.f(t, &y)?;
    if f0.iter().any(|v| !v.is_finite()) {
        return Err("f(t0, y0) is not finite".into());
    }
    // initial step (Hairer, Norsett & Wanner)
    let mut h = {
        let d0 = rms(&y, &y, opts);
        let d1 = rms(&f0, &y, opts);
        let h0 = if d0 < 1e-5 || d1 < 1e-5 { 1e-6 } else { 0.01 * d0 / d1 };
        let y1 = axpy(&y, dir * h0, &f0);
        let f1 = ctx.f(t + dir * h0, &y1)?;
        let df: Vec<f64> = (0..n).map(|i| f1[i] - f0[i]).collect();
        let d2 = rms(&df, &y, opts) / h0;
        let m = d1.max(d2);
        let h1 = if m <= 1e-15 { (h0 * 1e-3).max(1e-6) } else { (0.01 / m).powf(pexp) };
        (100.0 * h0).min(h1)
    };
    h = h.min(span).min(opts.max_step);
    let mut out = OdeOut {
        t: vec![],
        y: vec![],
        steps: 0,
        rejected: 0,
        nfev: 0,
        status: "ok",
        t_events: vec![],
        y_events: vec![],
    };
    let mut oi = 0usize;
    match outputs {
        None => {
            out.t.push(t0);
            out.y.push(y.clone());
        }
        Some(o) => {
            while oi < o.len() && o[oi] == t0 {
                out.t.push(t0);
                out.y.push(y.clone());
                oi += 1;
            }
        }
    }
    let terminal_evt = event.as_ref().map_or(false, |x| x.1);
    let mut g_prev = match event.as_mut() {
        Some((g, _)) => Some(g(t, &y)?),
        None => None,
    };
    let ythresh = (opts.atol / opts.rtol).max(1e-12);
    let mut jt: Option<JacT> = None;
    let mut prev_rejected = false;
    'main: while dir * (tf - t) > 0.0 {
        if out.steps + out.rejected >= opts.max_steps {
            out.status = "max_steps";
            break;
        }
        let mut h_abs = h.min(opts.max_step);
        let remaining = (tf - t).abs();
        let mut last = false;
        if h_abs >= remaining {
            h_abs = remaining;
            last = true;
        }
        let hs = dir * h_abs;
        if t + hs == t {
            out.status = "step_too_small";
            break;
        }
        let step = match method {
            Method::Dp45 => Some(dp_step(&mut ctx, t, &y, &f0, hs)?),
            Method::Bs23 => Some(bs_step(&mut ctx, t, &y, &f0, hs)?),
            Method::Stiff => {
                if jt.is_none() {
                    jt = Some(jac_t(&mut ctx, t, &y, &f0, hs, ythresh)?);
                }
                rosen_step(&mut ctx, t, &y, &f0, hs, jt.as_ref().unwrap())?
            }
        };
        let (en, step) = match step {
            Some(s) => (err_norm(&s.err, &y, &s.ynew, opts), Some(s)),
            None => (f64::INFINITY, None), // singular W: shrink and retry
        };
        let fac_raw = if en == 0.0 { 10.0 } else { 0.9 * en.powf(-pexp) };
        if en > 1.0 || step.is_none() {
            out.rejected += 1;
            prev_rejected = true;
            h = h_abs * fac_raw.clamp(0.2, 0.9);
            continue;
        }
        let s = step.unwrap();
        let tnew = if last { tf } else { t + hs };
        // event detection on this step
        let mut hit: Option<(f64, Vec<f64>)> = None;
        if let (Some((g, _)), Some(gp)) = (event.as_mut(), g_prev) {
            let gn = g(tnew, &s.ynew)?;
            if gp * gn < 0.0 || (gn == 0.0 && gp != 0.0) {
                let (mut a, mut ga, mut b, mut gb) = (t, gp, tnew, gn);
                let mut te = tnew;
                for _ in 0..200 {
                    if gb == 0.0 {
                        te = b;
                        break;
                    }
                    let c = b - gb * (b - a) / (gb - ga);
                    let c = if c.is_finite() && (c - a) * (c - b) < 0.0 { c } else { 0.5 * (a + b) };
                    let gc = g(c, &s.dense.eval(c))?;
                    te = c;
                    if gc == 0.0 || (b - a).abs() <= 1e-14 * b.abs().max(1.0) {
                        break;
                    }
                    if gc * gb < 0.0 {
                        a = b;
                        ga = gb;
                    } else {
                        ga *= 0.5;
                    }
                    b = c;
                    gb = gc;
                }
                let ye = if te == tnew { s.ynew.clone() } else { s.dense.eval(te) };
                out.t_events.push(te);
                out.y_events.push(ye.clone());
                if terminal_evt {
                    hit = Some((te, ye));
                }
            }
            g_prev = Some(gn);
        }
        let limit = hit.as_ref().map_or(tnew, |h| h.0);
        match outputs {
            None => {
                if let Some((te, ye)) = &hit {
                    out.t.push(*te);
                    out.y.push(ye.clone());
                } else {
                    out.t.push(tnew);
                    out.y.push(s.ynew.clone());
                }
            }
            Some(o) => {
                while oi < o.len() && dir * (o[oi] - limit) <= 1e-13 * span.max(limit.abs()) && dir * (o[oi] - t) > 0.0 {
                    out.t.push(o[oi]);
                    out.y.push(if o[oi] == tnew { s.ynew.clone() } else { s.dense.eval(o[oi]) });
                    oi += 1;
                }
                if let Some((te, ye)) = &hit {
                    if out.t.last() != Some(te) {
                        out.t.push(*te);
                        out.y.push(ye.clone());
                    }
                }
            }
        }
        out.steps += 1;
        if hit.is_some() {
            out.status = "event";
            break 'main;
        }
        t = tnew;
        y = s.ynew;
        f0 = s.fnew;
        jt = None;
        let fac = if prev_rejected { fac_raw.min(1.0) } else { fac_raw.min(10.0) };
        h = h_abs * fac.max(0.2);
        prev_rejected = false;
    }
    out.nfev = ctx.nfev;
    Ok(out)
}

/// Classical fixed-step RK4 from `t0` to `tf` with step `h` (the last step is
/// shortened to land on `tf`). Every step is reported.
pub(crate) fn rk4_fixed(rhs: RhsFn, t0: f64, tf: f64, y0: &[f64], h: f64) -> Result<OdeOut, String> {
    let span = (tf - t0).abs();
    let dir = if tf > t0 { 1.0 } else { -1.0 };
    let nsteps = (span / h - 1e-9).ceil().max(1.0);
    if nsteps > 5e7 {
        return Err(format!("{:.0} steps would be needed (h too small for the span)", nsteps));
    }
    let nsteps = nsteps as usize;
    let mut ctx = Ctx { rhs, nfev: 0 };
    let n = y0.len();
    let mut y = y0.to_vec();
    let mut out = OdeOut { t: vec![t0], y: vec![y.clone()], steps: 0, rejected: 0, nfev: 0, status: "ok", t_events: vec![], y_events: vec![] };
    let mut t = t0;
    for k in 0..nsteps {
        let tn = if k + 1 == nsteps { tf } else { t0 + dir * h * (k + 1) as f64 };
        let hh = tn - t;
        let k1 = ctx.f(t, &y)?;
        let k2 = ctx.f(t + 0.5 * hh, &axpy(&y, 0.5 * hh, &k1))?;
        let k3 = ctx.f(t + 0.5 * hh, &axpy(&y, 0.5 * hh, &k2))?;
        let k4 = ctx.f(tn, &axpy(&y, hh, &k3))?;
        for i in 0..n {
            y[i] += hh / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
        if y.iter().any(|v| !v.is_finite()) {
            out.status = "nonfinite";
            out.t.push(tn);
            out.y.push(y.clone());
            out.steps = k + 1;
            out.nfev = ctx.nfev;
            return Ok(out);
        }
        t = tn;
        out.t.push(t);
        out.y.push(y.clone());
    }
    out.steps = nsteps;
    out.nfev = ctx.nfev;
    Ok(out)
}

// ====================================================================
// Interpreter layer
// ====================================================================

fn num_arg(who: &str, what: &str, v: &Value) -> R<f64> {
    v.as_num().map_err(|_| EvalError { msg: format!("{who}: {what} must be a number, found {}", v.type_name()) })
}

/// `key=` if given, else positional `idx` if present.
fn opt_value<'a>(args: &'a [Value], idx: usize, style: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    match style_entry(style, key) {
        Some((_, v)) => Some(v),
        None => arg_get(args, idx),
    }
}

fn opt_num(who: &str, args: &[Value], idx: usize, style: &[(String, Value)], key: &str) -> R<Option<f64>> {
    match opt_value(args, idx, style, key) {
        None => Ok(None),
        Some(v) => Ok(Some(num_arg(who, key, v)?)),
    }
}

fn kw_num(who: &str, style: &[(String, Value)], key: &str) -> R<Option<f64>> {
    match style_entry(style, key) {
        None => Ok(None),
        Some((_, v)) => Ok(Some(num_arg(who, key, v)?)),
    }
}

fn positive(who: &str, key: &str, v: f64) -> R<f64> {
    if v > 0.0 {
        Ok(v)
    } else {
        e(format!("{who}: {key} must be positive, found {v}"))
    }
}

/// Columns of a sample array: a vector (or 1-row matrix) is one column.
fn columns(who: &str, v: &Value) -> R<(Vec<Vec<f64>>, bool)> {
    if let Value::Mat(m) = v {
        if m.rows() > 1 && m.cols() > 1 {
            let r = m.rows();
            return Ok((m.as_slice().chunks(r).map(|c| c.to_vec()).collect(), true));
        }
    }
    let y = to_vec(v).map_err(|er| EvalError { msg: format!("{who}: y: {}", er.msg) })?;
    Ok((vec![y], false))
}

fn rows_value(cols: Vec<Vec<f64>>) -> R<Value> {
    let r = cols.first().map_or(0, |c| c.len());
    let c = cols.len();
    let data: Vec<f64> = cols.into_iter().flatten().collect();
    Ok(Value::Mat(Arc::new(Matrix::from_col_major(r, c, data))))
}

fn row_matrix(rows: &[Vec<f64>], n: usize) -> Value {
    let data: Vec<f64> = (0..n).flat_map(|j| rows.iter().map(move |r| r[j])).collect();
    Value::Mat(Arc::new(Matrix::from_col_major(rows.len(), n, data)))
}

impl Interp {
    pub(crate) fn integrate_builtin(&mut self, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        match f {
            "trapz" | "cumtrapz" | "simpson" => self.sampled_integral(f, args),
            "quad" | "quad_info" => self.quad_builtin(f, args, style),
            _ => self.ode_builtin(f, args, style),
        }
    }

    fn sampled_integral(&mut self, who: &str, args: &[Value]) -> R<Value> {
        let (xv, yv) = match (arg_get(args, 0), arg_get(args, 1)) {
            (Some(y), None) => (None, y),
            (Some(x), Some(y)) => (Some(x), y),
            _ => return e(format!("{who}(y) or {who}(x, y) needs at least one argument")),
        };
        let x = match xv {
            Some(v) => Some(to_vec(v).map_err(|er| EvalError { msg: format!("{who}: x: {}", er.msg) })?),
            None => None,
        };
        let (cols, is_mat) = columns(who, yv)?;
        let n = cols[0].len();
        if let Some(x) = &x {
            if x.len() != n {
                return e(format!(
                    "{who}: x has {} elements but y has {n} {}",
                    x.len(),
                    if is_mat { "rows (columns integrate along dimension 1)" } else { "elements" }
                ));
            }
            if x.iter().any(|v| !v.is_finite()) {
                return e(format!("{who}: x must be finite"));
            }
        }
        let xs = x.as_deref();
        match who {
            "trapz" => {
                let r: Vec<f64> = cols.iter().map(|c| trapz_slice(xs, c)).collect();
                Ok(if is_mat { Value::Vec(Arc::new(r)) } else { Value::Num(r[0]) })
            }
            "cumtrapz" => {
                let r: Vec<Vec<f64>> = cols.iter().map(|c| cumtrapz_slice(xs, c)).collect();
                if is_mat {
                    rows_value(r)
                } else {
                    Ok(Value::Vec(Arc::new(r.into_iter().next().unwrap())))
                }
            }
            _ => {
                let mut r = Vec::new();
                for c in &cols {
                    r.push(simpson_slice(xs, c).map_err(|msg| EvalError { msg })?);
                }
                Ok(if is_mat { Value::Vec(Arc::new(r)) } else { Value::Num(r[0]) })
            }
        }
    }

    fn quad_builtin(&mut self, who: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        if args.len() < 3 {
            return e(format!("{who}(f, a, b, [tol=1e-10], [maxdepth=60]) needs at least 3 arguments"));
        }
        let name = text_arg(args, 0)?;
        if !self.has_user_fn(&name) {
            return e(format!("{who}: no user function named `{name}` (f must be a Qu function or lambda of one number)"));
        }
        let a = num_arg(who, "a", arg_get(args, 1).unwrap())?;
        let b = num_arg(who, "b", arg_get(args, 2).unwrap())?;
        let tol = positive(who, "tol", opt_num(who, args, 3, style, "tol")?.unwrap_or(1e-10))?;
        let md = opt_num(who, args, 4, style, "maxdepth")?.unwrap_or(60.0);
        if !(md >= 1.0 && md <= 200.0) || md.fract() != 0.0 {
            return e(format!("{who}: maxdepth must be a whole number from 1 to 200, found {md}"));
        }
        let mut call = |x: f64| -> Result<f64, String> {
            let v = self.apply(&name, vec![Value::Num(x)], Vec::new()).map_err(|er| format!("{who}: f({x}) failed: {}", er.msg))?;
            v.as_num().map_err(|_| format!("{who}: f must return a number, found {} at x = {x}", v.type_name()))
        };
        let o = quad_adaptive(&mut call, a, b, tol, md as usize).map_err(|msg| EvalError { msg })?;
        if who == "quad" {
            if o.status != "ok" {
                return e(format!(
                    "quad: did not reach tol={tol:e} ({}): estimate {} with error estimate {:e} after {} intervals; \
                     loosen tol, raise maxdepth, or call quad_info to get the estimate without this error",
                    o.status, o.value, o.error, o.intervals
                ));
            }
            return Ok(Value::Num(o.value));
        }
        Ok(Value::Record(Arc::new(vec![
            ("value".to_string(), Value::Num(o.value)),
            ("error".to_string(), Value::Num(o.error)),
            ("nfev".to_string(), Value::Num(o.nfev as f64)),
            ("intervals".to_string(), Value::Num(o.intervals as f64)),
            ("status".to_string(), Value::Str(o.status.to_string())),
        ])))
    }

    fn ode_builtin(&mut self, who: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        let fixed = who == "rk4";
        if args.len() < 3 + fixed as usize {
            return e(if fixed {
                "rk4(f, tspan, y0, h) needs 4 arguments".to_string()
            } else {
                format!("{who}(f, tspan, y0, [rtol=1e-6], [atol=1e-9], [max_step=], [t_eval=]) needs at least 3 arguments")
            });
        }
        let name = text_arg(args, 0)?;
        if !self.has_user_fn(&name) {
            return e(format!("{who}: no user function named `{name}` (f must be a Qu function or lambda f(t, y))"));
        }
        let tspan = to_vec(arg_get(args, 1).unwrap()).map_err(|er| EvalError { msg: format!("{who}: tspan: {}", er.msg) })?;
        if tspan.len() < 2 || tspan.iter().any(|v| !v.is_finite()) {
            return e(format!("{who}: tspan must be [t0, tf] (two finite numbers), found {} element(s)", tspan.len()));
        }
        let (t0, tf) = (tspan[0], *tspan.last().unwrap());
        if t0 == tf {
            return e(format!("{who}: tspan has t0 == tf == {t0}; there is nothing to integrate"));
        }
        let y0v = arg_get(args, 2).unwrap();
        let scalar = matches!(y0v, Value::Num(_));
        let y0 = to_vec(y0v).map_err(|er| EvalError { msg: format!("{who}: y0: {}", er.msg) })?;
        if y0.is_empty() || y0.iter().any(|v| !v.is_finite()) {
            return e(format!("{who}: y0 must be a non-empty finite number or vector"));
        }
        let n = y0.len();
        let cell = std::cell::RefCell::new(&mut *self);
        let mut rhs = |t: f64, y: &[f64]| -> Result<Vec<f64>, String> {
            let yv = if scalar { Value::Num(y[0]) } else { Value::Vec(Arc::new(y.to_vec())) };
            let r = cell.borrow_mut().apply(&name, vec![Value::Num(t), yv], Vec::new());
            let r = r.map_err(|er| format!("{who}: f({t}, y) failed: {}", er.msg))?;
            let v = to_vec(&r).map_err(|er| format!("{who}: f must return a number or vector: {}", er.msg))?;
            if v.len() != n {
                return Err(format!("{who}: f(t, y) returned {} value(s) but y has {n}", v.len()));
            }
            Ok(v)
        };
        let out = if fixed {
            let h = positive(who, "h", num_arg(who, "h", arg_get(args, 3).unwrap())?)?;
            rk4_fixed(&mut rhs, t0, tf, &y0, h)
        } else {
            let method = match who {
                "ode45" => Method::Dp45,
                "ode23" => Method::Bs23,
                _ => Method::Stiff,
            };
            let rtol = positive(who, "rtol", opt_num(who, args, 3, style, "rtol")?.unwrap_or(1e-6))?;
            let atol = positive(who, "atol", opt_num(who, args, 4, style, "atol")?.unwrap_or(1e-9))?;
            let max_step = positive(who, "max_step", opt_num(who, args, 5, style, "max_step")?.unwrap_or(f64::INFINITY))?;
            let max_steps = positive(who, "max_steps", kw_num(who, style, "max_steps")?.unwrap_or(500_000.0))? as usize;
            // outputs: t_eval=, or a tspan with more than two entries
            let mut outputs: Option<Vec<f64>> = None;
            if let Some(v) = opt_value(args, 6, style, "t_eval") {
                outputs = Some(to_vec(v).map_err(|er| EvalError { msg: format!("{who}: t_eval: {}", er.msg) })?);
            }
            if tspan.len() > 2 {
                if outputs.is_some() {
                    return e(format!("{who}: give either a tspan with more than two entries or t_eval=, not both"));
                }
                outputs = Some(tspan.clone());
            }
            if let Some(o) = &outputs {
                check_outputs(who, o, t0, tf).map_err(|msg| EvalError { msg })?;
            }
            let terminal = match style_entry(style, "terminal") {
                None => false,
                Some((_, Value::Bool(b))) => *b,
                Some((_, v)) => v.as_num().map(|x| x != 0.0).unwrap_or(false),
            };
            let ev_name = match style_entry(style, "events") {
                None => None,
                Some((_, v)) => Some(text_arg(std::slice::from_ref(v), 0)?),
            };
            if let Some(n) = &ev_name {
                if !cell.borrow().has_user_fn(n) {
                    return e(format!("{who}: events= names no user function `{n}` (it must be a function g(t, y) returning a number)"));
                }
            }
            let mut g = |t: f64, y: &[f64]| -> Result<f64, String> {
                let yv = if scalar { Value::Num(y[0]) } else { Value::Vec(Arc::new(y.to_vec())) };
                let nm = ev_name.as_ref().unwrap();
                let r = cell.borrow_mut().apply(nm, vec![Value::Num(t), yv], Vec::new());
                let r = r.map_err(|er| format!("{who}: events function failed at t={t}: {}", er.msg))?;
                r.as_num().map_err(|_| format!("{who}: the events function must return a number, found {}", r.type_name()))
            };
            let opts = OdeOpts { rtol, atol, max_step, max_steps };
            let evt: Option<(EventFn, bool)> = if ev_name.is_some() { Some((&mut g, terminal)) } else { None };
            solve_adaptive(method, &mut rhs, t0, tf, &y0, outputs.as_deref(), &opts, evt)
        };
        let o = out.map_err(|msg| EvalError { msg: if msg.starts_with(who) { msg } else { format!("{who}: {msg}") } })?;
        let with_events = !fixed && style_entry(style, "events").is_some();
        let mut fields = vec![
            ("t".to_string(), Value::Vec(Arc::new(o.t))),
            ("y".to_string(), row_matrix(&o.y, n)),
            ("steps".to_string(), Value::Num(o.steps as f64)),
            ("nfev".to_string(), Value::Num(o.nfev as f64)),
            ("status".to_string(), Value::Str(o.status.to_string())),
        ];
        if !fixed {
            fields.push(("rejected".to_string(), Value::Num(o.rejected as f64)));
        }
        if with_events {
            fields.push(("t_events".to_string(), Value::Vec(Arc::new(o.t_events))));
            fields.push((
                "y_events".to_string(),
                if o.y_events.is_empty() { Value::Mat(Arc::new(Matrix::zeros(0, n))) } else { row_matrix(&o.y_events, n) },
            ));
        }
        Ok(Value::Record(Arc::new(fields)))
    }
}

// ====================================================================
// Unit tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(rtol: f64, atol: f64) -> OdeOpts {
        OdeOpts { rtol, atol, max_step: f64::INFINITY, max_steps: 2_000_000 }
    }

    #[test]
    fn kronrod_weights_sum_to_two_and_integrate_polynomials() {
        let k: f64 = 2.0 * WGK[..7].iter().sum::<f64>() + WGK[7];
        let g: f64 = 2.0 * WG[..3].iter().sum::<f64>() + WG[3];
        assert!((k - 2.0).abs() < 1e-15 && (g - 2.0).abs() < 1e-15);
        // x^14 over [-1, 1] = 2/15: exact for K15 (degree <= 22)
        let mut f = |x: f64| Ok(x.powi(14));
        let (r, _) = qk15(&mut f, -1.0, 1.0).unwrap();
        assert!((r - 2.0 / 15.0).abs() < 1e-15, "{r}");
    }

    #[test]
    fn trapz_converges_at_second_order_and_simpson_at_fourth() {
        let err = |n: usize, simpson: bool| {
            let x: Vec<f64> = (0..=n).map(|i| std::f64::consts::PI * i as f64 / n as f64).collect();
            let y: Vec<f64> = x.iter().map(|v| v.sin()).collect();
            let v = if simpson { simpson_slice(Some(&x), &y).unwrap() } else { trapz_slice(Some(&x), &y) };
            (v - 2.0).abs()
        };
        let (t1, t2) = (err(20, false), err(40, false));
        assert!((t1 / t2 - 4.0).abs() < 0.1, "trapz order: {}", t1 / t2);
        let (s1, s2) = (err(20, true), err(40, true));
        assert!((s1 / s2 - 16.0).abs() < 0.8, "simpson order: {}", s1 / s2);
    }

    #[test]
    fn simpson_is_exact_for_cubics_on_even_intervals_and_quadratics_on_odd() {
        let x: Vec<f64> = (0..=8).map(|i| i as f64 * 0.25).collect();
        let y: Vec<f64> = x.iter().map(|v| v * v * v - 2.0 * v + 1.0).collect();
        let want = 2.0f64.powi(4) / 4.0 - 4.0 + 2.0;
        assert!((simpson_slice(Some(&x), &y).unwrap() - want).abs() < 1e-13);
        // 7 intervals (odd), non-uniform grid, quadratic
        let x = [0.0, 0.3, 0.5, 1.1, 1.2, 2.0, 2.7, 3.0];
        let y: Vec<f64> = x.iter().map(|v| 3.0 * v * v - v + 2.0).collect();
        let want = 27.0 - 4.5 + 6.0;
        assert!((simpson_slice(Some(&x), &y).unwrap() - want).abs() < 1e-12);
    }

    #[test]
    fn cumtrapz_ends_at_trapz() {
        let y: Vec<f64> = (0..50).map(|i| (i as f64 * 0.1).cos()).collect();
        let c = cumtrapz_slice(None, &y);
        assert_eq!(c[0], 0.0);
        assert!((c[49] - trapz_slice(None, &y)).abs() < 1e-14);
    }

    fn q(f: impl Fn(f64) -> f64, a: f64, b: f64, tol: f64) -> QuadOut {
        let mut g = |x| Ok(f(x));
        quad_adaptive(&mut g, a, b, tol, 60).unwrap()
    }

    #[test]
    fn quad_known_integrals() {
        let o = q(|x| x.sin(), 0.0, std::f64::consts::PI, 1e-12);
        assert!((o.value - 2.0).abs() < 1e-12 && o.status == "ok");
        // reversed limits flip the sign
        assert!((q(|x| x.sin(), std::f64::consts::PI, 0.0, 1e-12).value + 2.0).abs() < 1e-12);
        // oscillatory
        let o = q(|x| (20.0 * x).cos() * x, 0.0, 3.0, 1e-11);
        let want = (3.0 * (60.0f64).sin()) / 20.0 + ((60.0f64).cos() - 1.0) / 400.0;
        assert!((o.value - want).abs() < 1e-10, "{} vs {want}", o.value);
    }

    #[test]
    fn quad_endpoint_singularities() {
        // integral of x^-1/2 on (0, 1] is 2
        let o = q(|x| 1.0 / x.sqrt(), 0.0, 1.0, 1e-9);
        assert!((o.value - 2.0).abs() < 1e-8 && o.status == "ok", "{} {}", o.value, o.status);
        // integral of ln x on (0, 1] is -1
        let o = q(|x| x.ln(), 0.0, 1.0, 1e-10);
        assert!((o.value + 1.0).abs() < 1e-9, "{}", o.value);
    }

    #[test]
    fn quad_infinite_ranges() {
        let o = q(|x| (-x * x).exp(), f64::NEG_INFINITY, f64::INFINITY, 1e-12);
        assert!((o.value - std::f64::consts::PI.sqrt()).abs() < 1e-11, "{}", o.value);
        let o = q(|x| (-x).exp(), 0.0, f64::INFINITY, 1e-12);
        assert!((o.value - 1.0).abs() < 1e-11, "{}", o.value);
        let o = q(|x| 1.0 / (1.0 + x * x), f64::NEG_INFINITY, 0.0, 1e-12);
        assert!((o.value - std::f64::consts::FRAC_PI_2).abs() < 1e-11, "{}", o.value);
        // slowly decaying: integral of 1/x^2 on [1, inf) is 1
        let o = q(|x| 1.0 / (x * x), 1.0, f64::INFINITY, 1e-12);
        assert!((o.value - 1.0).abs() < 1e-11, "{}", o.value);
    }

    #[test]
    fn quad_reports_a_non_finite_integrand() {
        let mut g = |x: f64| Ok(1.0 / x);
        let r = quad_adaptive(&mut g, -1.0, 1.0, 1e-10, 60);
        // the centre node is exactly 0
        assert!(r.is_err(), "{:?}", r.map(|o| o.value));
    }

    fn solve(m: Method, f: &mut dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, String>, tf: f64, y0: &[f64], o: &OdeOpts) -> OdeOut {
        solve_adaptive(m, f, 0.0, tf, y0, None, o, None).unwrap()
    }

    #[test]
    fn dense_output_weights_reproduce_the_step() {
        // at s = 1 the continuous extension must collapse to the 5th-order b
        let b = [35.0 / 384.0, 0.0, 500.0 / 1113.0, 125.0 / 192.0, -2187.0 / 6784.0, 11.0 / 84.0, 0.0];
        for i in 0..7 {
            let c: f64 = BI[i].iter().sum();
            assert!((c - b[i]).abs() < 1e-14, "row {i}: {c} vs {}", b[i]);
        }
    }

    #[test]
    fn exponential_decay_all_methods() {
        for (m, rt, tol) in [(Method::Dp45, 1e-9, 1e-7), (Method::Bs23, 1e-9, 1e-6), (Method::Stiff, 1e-9, 1e-5)] {
            let mut f = |_t: f64, y: &[f64]| Ok(vec![-2.0 * y[0]]);
            let o = solve(m, &mut f, 3.0, &[1.5], &opts(rt, 1e-12));
            let got = o.y.last().unwrap()[0];
            let want = 1.5 * (-6.0f64).exp();
            assert!((got - want).abs() < tol, "{m:?}: {got} vs {want}");
            assert_eq!(o.status, "ok");
            assert_eq!(*o.t.last().unwrap(), 3.0);
        }
    }

    #[test]
    fn harmonic_oscillator_energy_drift_over_many_periods() {
        // 20 periods; DP5(4) is dissipative but at rtol 1e-9 drift is tiny
        let tf = 20.0 * 2.0 * std::f64::consts::PI;
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[1], -y[0]]);
        let o = solve(Method::Dp45, &mut f, tf, &[1.0, 0.0], &opts(1e-9, 1e-12));
        let y = o.y.last().unwrap();
        let energy = 0.5 * (y[0] * y[0] + y[1] * y[1]);
        assert!((energy - 0.5).abs() < 1e-6, "energy drift {}", (energy - 0.5).abs());
        assert!((y[0] - tf.cos()).abs() < 1e-5 && (y[1] + tf.sin()).abs() < 1e-5);
        // every reported step is a real accepted step
        assert_eq!(o.t.len(), o.steps + 1);
    }

    #[test]
    fn t_eval_uses_accurate_dense_output() {
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[1], -y[0]]);
        let outs: Vec<f64> = (0..=40).map(|i| i as f64 * 0.25).collect();
        let o = solve_adaptive(Method::Dp45, &mut f, 0.0, 10.0, &[0.0, 1.0], Some(&outs), &opts(1e-8, 1e-10), None).unwrap();
        assert_eq!(o.t, outs);
        for (t, y) in o.t.iter().zip(&o.y) {
            assert!((y[0] - t.sin()).abs() < 5e-7 && (y[1] - t.cos()).abs() < 5e-7, "t={t}");
        }
        // and with outputs that fall between steps (spacing 0.37 vs step ~0.08)
        let sparse: Vec<f64> = (0..=27).map(|i| i as f64 * 0.37).collect();
        let o = solve_adaptive(Method::Dp45, &mut f, 0.0, 10.0, &[0.0, 1.0], Some(&sparse), &opts(1e-8, 1e-10), None).unwrap();
        assert_eq!(o.t.len(), sparse.len());
        assert!(o.steps > o.t.len(), "outputs should be interpolated, not forced steps");
        for (t, y) in o.t.iter().zip(&o.y) {
            assert!((y[0] - t.sin()).abs() < 5e-7, "sparse t={t}");
        }
    }

    #[test]
    fn backward_integration() {
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[0]]);
        let o = solve_adaptive(Method::Dp45, &mut f, 1.0, 0.0, &[std::f64::consts::E], None, &opts(1e-10, 1e-12), None).unwrap();
        assert!((o.y.last().unwrap()[0] - 1.0).abs() < 1e-8);
    }

    #[test]
    fn rk4_is_fourth_order() {
        let err = |h: f64| {
            let mut f = |_t: f64, y: &[f64]| Ok(vec![y[0]]);
            let o = rk4_fixed(&mut f, 0.0, 1.0, &[1.0], h).unwrap();
            (o.y.last().unwrap()[0] - std::f64::consts::E).abs()
        };
        let r = err(0.1) / err(0.05);
        assert!((r - 16.0).abs() < 1.5, "ratio {r}");
        // the last step lands exactly on tf even when h does not divide the span
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[0]]);
        let o = rk4_fixed(&mut f, 0.0, 1.0, &[1.0], 0.3).unwrap();
        assert_eq!(*o.t.last().unwrap(), 1.0);
        assert_eq!(o.steps, 4);
    }

    fn robertson(_t: f64, y: &[f64]) -> Result<Vec<f64>, String> {
        Ok(vec![
            -0.04 * y[0] + 1e4 * y[1] * y[2],
            0.04 * y[0] - 1e4 * y[1] * y[2] - 3e7 * y[1] * y[1],
            3e7 * y[1] * y[1],
        ])
    }

    #[test]
    fn stiff_solver_matches_a_reference_on_robertson() {
        // reference: explicit DP5(4) at 1e-11 over [0, 1] (stiff but short)
        let mut f = robertson;
        let r = solve(Method::Dp45, &mut f, 1.0, &[1.0, 0.0, 0.0], &opts(1e-11, 1e-14));
        let want = r.y.last().unwrap().clone();
        let mut f = robertson;
        let s = solve(Method::Stiff, &mut f, 1.0, &[1.0, 0.0, 0.0], &opts(1e-7, 1e-11));
        let got = s.y.last().unwrap();
        for i in 0..3 {
            let sc = want[i].abs().max(1e-6);
            assert!((got[i] - want[i]).abs() < 2e-4 * sc, "y[{i}]: {} vs {}", got[i], want[i]);
        }
        assert_eq!(s.status, "ok");
        // mass is conserved to rounding by the linear invariant of the scheme
        assert!((got.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn stiff_solver_reaches_the_published_robertson_values_at_t_40() {
        let mut f = robertson;
        let s = solve(Method::Stiff, &mut f, 40.0, &[1.0, 0.0, 0.0], &opts(1e-7, 1e-11));
        let y = s.y.last().unwrap();
        // Hairer & Wanner (1996), IV.1: y(40) = 0.7158270687, 9.185534764e-6, 0.2841637457
        assert!((y[0] - 0.7158270687).abs() < 1e-4, "{}", y[0]);
        assert!((y[1] - 9.185534764e-6).abs() < 5e-8, "{}", y[1]);
        assert!((y[2] - 0.2841637457).abs() < 1e-4, "{}", y[2]);
    }

    #[test]
    fn stiff_solver_beats_explicit_on_a_stiff_linear_problem() {
        // y' = -L (y - cos t), L = 1e4: explicit stability needs h < 3.3/L
        let mut f = |t: f64, y: &[f64]| Ok(vec![-1e4 * (y[0] - t.cos())]);
        let stiff = solve(Method::Stiff, &mut f, 10.0, &[0.0], &opts(1e-6, 1e-9));
        let mut f = |t: f64, y: &[f64]| Ok(vec![-1e4 * (y[0] - t.cos())]);
        let expl = solve(Method::Dp45, &mut f, 10.0, &[0.0], &opts(1e-6, 1e-9));
        assert!(stiff.nfev * 5 < expl.nfev, "stiff {} vs explicit {}", stiff.nfev, expl.nfev);
        // exact after the transient: (L^2 cos t + L sin t)/(L^2 + 1)
        let t = 10.0f64;
        let want = (1e8 * t.cos() + 1e4 * t.sin()) / (1e8 + 1.0);
        assert!((stiff.y.last().unwrap()[0] - want).abs() < 1e-5);
        assert!((expl.y.last().unwrap()[0] - want).abs() < 1e-5);
    }

    #[test]
    fn terminal_event_finds_the_zero_crossing() {
        // y'' = -g ball thrown up from 0 with v = 10: lands at t = 2 v / g
        let g = 9.81;
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[1], -g]);
        // Start slightly above 0 so the start is not a zero of the event.
        let mut ev = |_t: f64, y: &[f64]| Ok(y[0]);
        let o = solve_adaptive(Method::Dp45, &mut f, 0.0, 10.0, &[1e-3, 10.0], None, &opts(1e-10, 1e-12), Some((&mut ev, true))).unwrap();
        assert_eq!(o.status, "event");
        let want = (10.0 + (100.0f64 + 2.0 * g * 1e-3).sqrt()) / g;
        assert!((o.t_events[0] - want).abs() < 1e-9, "{} vs {want}", o.t_events[0]);
        assert_eq!(*o.t.last().unwrap(), o.t_events[0]);
    }

    #[test]
    fn non_terminal_events_count_every_crossing() {
        let mut f = |_t: f64, y: &[f64]| Ok(vec![y[1], -y[0]]);
        let mut ev = |_t: f64, y: &[f64]| Ok(y[0]);
        let o = solve_adaptive(Method::Bs23, &mut f, 0.0, 10.0, &[0.5, 1.0], None, &opts(1e-9, 1e-12), Some((&mut ev, false))).unwrap();
        assert_eq!(o.status, "ok");
        // y = 0.5 cos t + sin t crosses zero at t = atan(-0.5) + k pi
        let first = std::f64::consts::PI - 0.5f64.atan();
        assert_eq!(o.t_events.len(), 3, "{:?}", o.t_events);
        assert!((o.t_events[0] - first).abs() < 1e-6);
    }

    #[test]
    fn bad_output_times_are_refused() {
        assert!(check_outputs("ode45", &[0.0, 2.0, 1.0], 0.0, 3.0).is_err());
        assert!(check_outputs("ode45", &[0.0, 4.0], 0.0, 3.0).is_err());
        assert!(check_outputs("ode45", &[0.0, 1.0], 0.0, 3.0).is_ok());
    }
}
