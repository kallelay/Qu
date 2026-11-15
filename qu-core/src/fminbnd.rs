//! `fminbnd`: Brent's method (golden section plus parabolic interpolation) for
//! a scalar function on a closed interval, after Brent's `fmin`
//! (Forsythe, Malcolm and Moler). It never evaluates the interval's ends, so
//! a minimum at an endpoint is returned as a point within about `tol` of it.

#[derive(Debug, Clone, Copy)]
pub struct FminbndResult {
    pub x: f64,
    pub fx: f64,
    pub iterations: usize,
    pub converged: bool,
}

/// Minimise `f` on `[a, b]`. `tol` is the absolute tolerance on `x`; the
/// attainable accuracy is limited to about `sqrt(eps) * |x|` (the function is
/// flat to first order at a smooth minimum). At most `max_iter` evaluations
/// after the first. A non-finite value of `f` is an error.
pub fn fminbnd(f: &mut dyn FnMut(f64) -> Result<f64, String>, a: f64, b: f64, tol: f64, max_iter: usize) -> Result<FminbndResult, String> {
    let c = 0.5 * (3.0 - 5.0f64.sqrt());
    let sqrt_eps = f64::EPSILON.sqrt();
    let (mut a, mut b) = (a, b);
    let mut eval = |x: f64| -> Result<f64, String> {
        let v = f(x)?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(format!("fminbnd: f({x}) is {v}; the objective must be finite on [a, b]"))
        }
    };
    let mut x = a + c * (b - a);
    let (mut v, mut w) = (x, x);
    let (mut e, mut d) = (0.0f64, 0.0f64);
    let mut fx = eval(x)?;
    let (mut fv, mut fw) = (fx, fx);
    let mut it = 0;
    while it < max_iter {
        let xm = 0.5 * (a + b);
        let tol1 = sqrt_eps * x.abs() + tol / 3.0;
        let tol2 = 2.0 * tol1;
        if (x - xm).abs() <= tol2 - 0.5 * (b - a) {
            return Ok(FminbndResult { x, fx, iterations: it, converged: true });
        }
        let mut golden = true;
        if e.abs() > tol1 {
            let mut r = (x - w) * (fx - fv);
            let mut q = (x - v) * (fx - fw);
            let mut p = (x - v) * q - (x - w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            }
            q = q.abs();
            r = e;
            e = d;
            if p.abs() < (0.5 * q * r).abs() && p > q * (a - x) && p < q * (b - x) {
                d = p / q;
                let u = x + d;
                if u - a < tol2 || b - u < tol2 {
                    d = if xm >= x { tol1 } else { -tol1 };
                }
                golden = false;
            }
        }
        if golden {
            e = if x >= xm { a - x } else { b - x };
            d = c * e;
        }
        let u = if d.abs() >= tol1 { x + d } else { x + if d >= 0.0 { tol1 } else { -tol1 } };
        let fu = eval(u)?;
        it += 1;
        if fu <= fx {
            if u >= x {
                a = x;
            } else {
                b = x;
            }
            v = w;
            fv = fw;
            w = x;
            fw = fx;
            x = u;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                v = w;
                fv = fw;
                w = u;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }
    Ok(FminbndResult { x, fx, iterations: it, converged: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_and_cosine() {
        let r = fminbnd(&mut |x| Ok((x - 2.0) * (x - 2.0)), 0.0, 5.0, 1e-8, 500).unwrap();
        assert!(r.converged && (r.x - 2.0).abs() < 1e-7);
        let r = fminbnd(&mut |x| Ok(x.cos()), 3.0, 4.0, 1e-8, 500).unwrap();
        assert!((r.x - std::f64::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn boundary_minimum_and_cap() {
        let r = fminbnd(&mut |x| Ok(x), 1.0, 3.0, 1e-8, 500).unwrap();
        assert!(r.converged && (r.x - 1.0).abs() < 1e-6, "{r:?}");
        let r = fminbnd(&mut |x| Ok(x * x), -1.0, 2.0, 1e-8, 3).unwrap();
        assert!(!r.converged && r.iterations == 3);
    }
}
