//! Tiny dense helpers shared by the solver modules (`fsolve`, `lp`, `qp`):
//! a row-major LU with partial pivoting. Sizes here are the small systems
//! (tens to a few hundreds of unknowns) those solvers form, so clarity beats
//! blocking.

/// LU factorisation (partial pivoting) of a row-major `n x n` matrix.
pub struct Lu {
    n: usize,
    lu: Vec<f64>,
    piv: Vec<usize>,
}

impl Lu {
    /// `None` when the matrix is singular to working precision (a pivot
    /// smaller than `1e-14` times the largest entry) or has a non-finite entry.
    pub fn new(a: &[f64], n: usize) -> Option<Lu> {
        assert_eq!(a.len(), n * n);
        if a.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let scale = a.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        if scale == 0.0 {
            return if n == 0 { Some(Lu { n, lu: vec![], piv: vec![] }) } else { None };
        }
        let mut lu = a.to_vec();
        let mut piv: Vec<usize> = (0..n).collect();
        for k in 0..n {
            let mut p = k;
            let mut best = lu[k * n + k].abs();
            for i in k + 1..n {
                let v = lu[i * n + k].abs();
                if v > best {
                    best = v;
                    p = i;
                }
            }
            if best <= 1e-14 * scale {
                return None;
            }
            if p != k {
                for j in 0..n {
                    lu.swap(k * n + j, p * n + j);
                }
                piv.swap(k, p);
            }
            let d = lu[k * n + k];
            for i in k + 1..n {
                let f = lu[i * n + k] / d;
                lu[i * n + k] = f;
                if f != 0.0 {
                    for j in k + 1..n {
                        lu[i * n + j] -= f * lu[k * n + j];
                    }
                }
            }
        }
        Some(Lu { n, lu, piv })
    }

    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        let n = self.n;
        assert_eq!(b.len(), n);
        let mut x: Vec<f64> = self.piv.iter().map(|&p| b[p]).collect();
        for i in 0..n {
            let mut s = x[i];
            for j in 0..i {
                s -= self.lu[i * n + j] * x[j];
            }
            x[i] = s;
        }
        for i in (0..n).rev() {
            let mut s = x[i];
            for j in i + 1..n {
                s -= self.lu[i * n + j] * x[j];
            }
            x[i] = s / self.lu[i * n + i];
        }
        x
    }
}

/// Solve `A x = b` for a row-major `n x n` `A`; `None` if singular.
pub fn solve(a: &[f64], n: usize, b: &[f64]) -> Option<Vec<f64>> {
    Lu::new(a, n).map(|lu| lu.solve(b))
}

pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn norm2(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

pub fn norm_inf(a: &[f64]) -> f64 {
    a.iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_known_system() {
        // 2x + y = 5, x + 3y = 10  ->  x = 1, y = 3
        let x = solve(&[2.0, 1.0, 1.0, 3.0], 2, &[5.0, 10.0]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-14 && (x[1] - 3.0).abs() < 1e-14);
    }

    #[test]
    fn needs_pivoting() {
        let x = solve(&[0.0, 1.0, 1.0, 0.0], 2, &[2.0, 3.0]).unwrap();
        assert_eq!(x, vec![3.0, 2.0]);
    }

    #[test]
    fn singular_is_none() {
        assert!(solve(&[1.0, 2.0, 2.0, 4.0], 2, &[1.0, 2.0]).is_none());
    }
}
