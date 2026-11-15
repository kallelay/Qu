//! Finite-difference and spectral operators for the method of lines:
//! `diffmat` (central first/second derivative on a uniform grid), `laplacian`
//! (1-D and 2-D discrete `-d^2/dx^2`, `-(d^2/dx^2 + d^2/dy^2)`), and
//! `chebdiff` (Chebyshev differentiation matrix, the `cheb` of Trefethen's
//! "Spectral Methods in MATLAB").
//!
//! The sparse operators come back as triplets `(row, col, value)` so the
//! interpreter can build its own CSR type from them.

#[derive(Debug, Clone, PartialEq)]
pub struct Triplets {
    pub m: usize,
    pub n: usize,
    pub i: Vec<usize>,
    pub j: Vec<usize>,
    pub v: Vec<f64>,
}

impl Triplets {
    fn new(m: usize, n: usize) -> Triplets {
        Triplets { m, n, i: vec![], j: vec![], v: vec![] }
    }
    fn push(&mut self, i: usize, j: usize, v: f64) {
        self.i.push(i);
        self.j.push(j);
        self.v.push(v);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// One-sided second-order stencils at the two ends (`diffmat` only).
    None,
    Periodic,
    /// The grid holds the interior unknowns; the value just outside is zero.
    Dirichlet,
    /// Zero flux through the ends of the cell-centred grid (`laplacian` only).
    Neumann,
}

fn check_h(f: &str, h: f64) -> Result<(), String> {
    if h.is_finite() && h > 0.0 {
        Ok(())
    } else {
        Err(format!("{f}: h must be a positive finite number, found {h}"))
    }
}

/// Central first (`order = 1`) or second (`order = 2`) derivative on `n`
/// equally spaced points `h` apart.
pub fn diffmat(n: usize, h: f64, order: usize, bc: Boundary) -> Result<Triplets, String> {
    check_h("sparse.diffmat", h)?;
    if order != 1 && order != 2 {
        return Err(format!("sparse.diffmat: order must be 1 or 2, found {order}"));
    }
    let need = match (bc, order) {
        (Boundary::Periodic, _) => 3,
        (Boundary::None, 1) => 3,
        (Boundary::None, _) => 4,
        (Boundary::Dirichlet, _) => 2,
        (Boundary::Neumann, _) => return Err("sparse.diffmat: bc must be \"none\", \"periodic\" or \"dirichlet\"".into()),
    };
    if n < need {
        return Err(format!("sparse.diffmat: order {order} with this bc needs at least {need} points, found {n}"));
    }
    let mut t = Triplets::new(n, n);
    let (c1, c2) = (1.0 / (2.0 * h), 1.0 / (h * h));
    for r in 0..n {
        let interior = r > 0 && r + 1 < n;
        if interior || bc == Boundary::Periodic {
            let (l, rt) = ((r + n - 1) % n, (r + 1) % n);
            if order == 1 {
                t.push(r, l, -c1);
                t.push(r, rt, c1);
            } else {
                t.push(r, l, c2);
                t.push(r, r, -2.0 * c2);
                t.push(r, rt, c2);
            }
        } else if bc == Boundary::Dirichlet {
            // the neighbour outside the grid is zero
            if order == 1 {
                if r == 0 {
                    t.push(r, 1, c1);
                } else {
                    t.push(r, n - 2, -c1);
                }
            } else {
                t.push(r, r, -2.0 * c2);
                t.push(r, if r == 0 { 1 } else { n - 2 }, c2);
            }
        } else {
            // one-sided, second-order accurate
            let cols: Vec<usize> = if r == 0 { (0..4).collect() } else { (0..4).map(|k| n - 1 - k).collect() };
            if order == 1 {
                let sign = if r == 0 { 1.0 } else { -1.0 };
                let coef = [-3.0, 4.0, -1.0];
                for k in 0..3 {
                    t.push(r, cols[k], sign * coef[k] * c1);
                }
            } else {
                let coef = [2.0, -5.0, 4.0, -1.0];
                for k in 0..4 {
                    t.push(r, cols[k], coef[k] * c2);
                }
            }
        }
    }
    Ok(t)
}

fn lap1(n: usize, h: f64, bc: Boundary) -> Result<Triplets, String> {
    check_h("sparse.laplacian", h)?;
    let need = match bc {
        Boundary::Dirichlet => 1,
        Boundary::Neumann => 2,
        Boundary::Periodic => 3,
        Boundary::None => return Err("sparse.laplacian: bc must be \"dirichlet\", \"neumann\" or \"periodic\"".into()),
    };
    if n < need {
        return Err(format!("sparse.laplacian: this bc needs at least {need} points per direction, found {n}"));
    }
    let c = 1.0 / (h * h);
    let mut t = Triplets::new(n, n);
    for r in 0..n {
        let has_l = r > 0;
        let has_r = r + 1 < n;
        match bc {
            Boundary::Dirichlet => {
                t.push(r, r, 2.0 * c);
                if has_l {
                    t.push(r, r - 1, -c);
                }
                if has_r {
                    t.push(r, r + 1, -c);
                }
            }
            Boundary::Neumann => {
                // zero-flux ends: the missing neighbour is dropped from the diagonal too
                t.push(r, r, (has_l as usize + has_r as usize) as f64 * c);
                if has_l {
                    t.push(r, r - 1, -c);
                }
                if has_r {
                    t.push(r, r + 1, -c);
                }
            }
            _ => {
                t.push(r, r, 2.0 * c);
                t.push(r, (r + n - 1) % n, -c);
                t.push(r, (r + 1) % n, -c);
            }
        }
    }
    Ok(t)
}

/// 1-D (`shape = [n]`) or 2-D (`shape = [nx, ny]`) discrete `-Laplacian`.
/// The 2-D node `(i, j)` (`i` along x, `0 <= i < nx`) is unknown number
/// `i + nx * j`: x runs fastest, so a column-major `nx x ny` array flattens
/// to the unknown vector in order. `h` has one spacing per direction.
pub fn laplacian(shape: &[usize], h: &[f64], bc: Boundary) -> Result<Triplets, String> {
    match shape.len() {
        1 => lap1(shape[0], h[0], bc),
        2 => {
            let (nx, ny) = (shape[0], shape[1]);
            let lx = lap1(nx, h[0], bc)?;
            let ly = lap1(ny, h[1], bc)?;
            let mut t = Triplets::new(nx * ny, nx * ny);
            for j in 0..ny {
                for k in 0..lx.v.len() {
                    t.push(lx.i[k] + nx * j, lx.j[k] + nx * j, lx.v[k]);
                }
            }
            for i in 0..nx {
                for k in 0..ly.v.len() {
                    t.push(i + nx * ly.i[k], i + nx * ly.j[k], ly.v[k]);
                }
            }
            Ok(t)
        }
        _ => Err("sparse.laplacian: shape must be n or [nx, ny] (3-D is not supported)".into()),
    }
}

/// Chebyshev differentiation matrix on the `n` Gauss-Lobatto points
/// `x_j = cos(j pi / (n - 1))`, `j = 0..n-1` (so from `1` down to `-1`).
/// Returns `(D, x)` with `D` row-major `n x n`. The diagonal is the negative
/// row sum (so a constant differentiates to exactly zero), the numerically
/// stable form.
pub fn chebdiff(n: usize) -> Result<(Vec<f64>, Vec<f64>), String> {
    if n < 2 {
        return Err(format!("chebdiff: n must be at least 2 points, found {n}"));
    }
    let big_n = n - 1;
    let x: Vec<f64> = (0..n).map(|j| (std::f64::consts::PI * j as f64 / big_n as f64).cos()).collect();
    let c: Vec<f64> = (0..n)
        .map(|j| (if j == 0 || j == big_n { 2.0 } else { 1.0 }) * if j % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let mut d = vec![0.0; n * n];
    for i in 0..n {
        let mut s = 0.0;
        for j in 0..n {
            if i != j {
                let v = c[i] / c[j] / (x[i] - x[j]);
                d[i * n + j] = v;
                s += v;
            }
        }
        d[i * n + i] = -s;
    }
    Ok((d, x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dense(t: &Triplets) -> Vec<Vec<f64>> {
        let mut m = vec![vec![0.0; t.n]; t.m];
        for k in 0..t.v.len() {
            m[t.i[k]][t.j[k]] += t.v[k];
        }
        m
    }

    fn apply(m: &[Vec<f64>], u: &[f64]) -> Vec<f64> {
        m.iter().map(|r| r.iter().zip(u).map(|(a, b)| a * b).sum()).collect()
    }

    #[test]
    fn second_order_convergence_of_every_variant() {
        for &(order, bc) in &[(1, Boundary::None), (2, Boundary::None), (1, Boundary::Periodic), (2, Boundary::Periodic)] {
            let mut errs = vec![];
            for &npts in &[40usize, 80] {
                let len = 2.0 * std::f64::consts::PI;
                let per = bc == Boundary::Periodic;
                let h = if per { len / npts as f64 } else { len / (npts - 1) as f64 };
                let u: Vec<f64> = (0..npts).map(|i| (i as f64 * h + 0.5).sin()).collect();
                let exact: Vec<f64> = (0..npts).map(|i| if order == 1 { (i as f64 * h + 0.5).cos() } else { -(i as f64 * h + 0.5).sin() }).collect();
                let d = dense(&diffmat(npts, h, order, bc).unwrap());
                let got = apply(&d, &u);
                errs.push(got.iter().zip(&exact).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max));
            }
            let slope = (errs[0] / errs[1]).log2();
            assert!((slope - 2.0).abs() < 0.25, "order {order} {bc:?} slope {slope}");
        }
    }

    #[test]
    fn neumann_rows_sum_to_zero() {
        let d = dense(&laplacian(&[5], &[0.5], Boundary::Neumann).unwrap());
        for r in &d {
            assert!(r.iter().sum::<f64>().abs() < 1e-12);
        }
    }

    #[test]
    fn chebdiff_is_exact_on_polynomials() {
        let n = 8;
        let (d, x) = chebdiff(n).unwrap();
        let u: Vec<f64> = x.iter().map(|v| v.powi(5)).collect();
        for i in 0..n {
            let got: f64 = (0..n).map(|j| d[i * n + j] * u[j]).sum();
            assert!((got - 5.0 * x[i].powi(4)).abs() < 1e-11);
        }
    }
}
