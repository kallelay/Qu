//! `import sparse` -- sparse matrices in compressed-sparse-row (CSR) form.
//!
//! A sparse matrix is an IMMUTABLE VALUE: every operation returns a new one.
//! It travels as a `Value::Model` of kind `"sparse"` whose read-only fields
//! (`rows`, `cols`, `nnz`, `indptr`, `indices`, `data`) are the CSR arrays
//! themselves, so there is no interpreter-side handle table to leak and a
//! sparse matrix can be copied, passed to a function or stored in a record
//! like any other value. The price is that each call decodes the index
//! arrays (O(nnz)); `get` skips the decode and binary-searches in place.
//!
//! **Indexing is zero-based**, matching base Qu (spec §34.C.6): `i` and `j`
//! in `from_triplets`, `get` and `triplets` count from 0.
//!
//! Limits (every constructor and every fill-producing operation checks them
//! and refuses with an error naming the function and the sizes, rather than
//! attempting the allocation): rows and columns each at most [`MAX_DIM`],
//! stored entries at most [`MAX_NNZ`], `to_dense` at most [`MAX_DENSE`]
//! elements; `rows * cols` is computed with checked arithmetic.
//!
//! Solvers (see `solve_lu`, `cg`, `bicgstab`): sparse LU is a left-looking
//! Gilbert-Peierls factorisation with threshold partial pivoting (diagonal
//! preferred while within 10x of the largest candidate). The fill-reducing
//! COLUMN ORDERING is reverse Cuthill-McKee on the pattern of A + A^T --
//! NOT COLAMD/AMD: it is excellent for banded/mesh-like matrices and can be
//! poor on irregular unsymmetric patterns. CG (optional Jacobi
//! preconditioner) is for SPD matrices; BiCGSTAB for nonsymmetric ones.
//! `eigs` is not implemented.

use crate::{arg_get, e, style_entry, style_num, style_str, text_arg, to_vec, EvalError, Interp, ModelHandle, Rng, Value, R};
use qu_core::matrix::Matrix;
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "from_triplets", "from_dense", "eye", "diag", "random", "size", "nnz", "density", "to_dense", "get", "triplets", "transpose", "add",
    "sub", "scale", "mul", "hadamard", "solve",
];

/// Largest row or column count.
pub const MAX_DIM: usize = 10_000_000;
/// Largest number of stored entries in any one sparse matrix.
pub const MAX_NNZ: usize = 20_000_000;
/// Largest `rows * cols` that `to_dense`/`from_dense` will materialise.
pub const MAX_DENSE: usize = 25_000_000;

// ------------------------------------------------------------------ core

#[derive(Clone, Debug, PartialEq)]
pub struct Csr {
    pub m: usize,
    pub n: usize,
    pub indptr: Vec<usize>,
    pub indices: Vec<usize>,
    pub data: Vec<f64>,
}

type CR<T> = Result<T, String>;

fn check_dims(m: usize, n: usize) -> CR<()> {
    if m > MAX_DIM || n > MAX_DIM {
        return Err(format!("{m}x{n} exceeds the limit of {MAX_DIM} rows/columns"));
    }
    if m.checked_mul(n).is_none() {
        return Err(format!("{m}x{n} overflows"));
    }
    Ok(())
}

fn check_nnz(nnz: usize) -> CR<()> {
    if nnz > MAX_NNZ {
        return Err(format!("{nnz} stored entries exceeds the limit of {MAX_NNZ}"));
    }
    Ok(())
}

impl Csr {
    pub fn empty(m: usize, n: usize) -> CR<Csr> {
        check_dims(m, n)?;
        Ok(Csr { m, n, indptr: vec![0; m + 1], indices: vec![], data: vec![] })
    }

    pub fn nnz(&self) -> usize {
        self.data.len()
    }

    /// Build from (row, col, value) triplets; duplicates are summed and
    /// entries that are exactly zero afterwards are not stored.
    pub fn from_triplets(m: usize, n: usize, ri: &[usize], ci: &[usize], v: &[f64]) -> CR<Csr> {
        check_dims(m, n)?;
        if ri.len() != ci.len() || ri.len() != v.len() {
            return Err(format!("i, j and v must have the same length, found {}, {} and {}", ri.len(), ci.len(), v.len()));
        }
        check_nnz(ri.len())?;
        for k in 0..ri.len() {
            if ri[k] >= m || ci[k] >= n {
                return Err(format!("entry {k} is at ({}, {}), outside the {m}x{n} matrix (indices are zero-based)", ri[k], ci[k]));
            }
            if !v[k].is_finite() {
                return Err(format!("entry {k} has a non-finite value ({})", v[k]));
            }
        }
        let mut cnt = vec![0usize; m + 1];
        for &r in ri {
            cnt[r + 1] += 1;
        }
        for i in 0..m {
            cnt[i + 1] += cnt[i];
        }
        let mut pos = cnt.clone();
        let mut tmp: Vec<(usize, f64)> = vec![(0, 0.0); ri.len()];
        for k in 0..ri.len() {
            tmp[pos[ri[k]]] = (ci[k], v[k]);
            pos[ri[k]] += 1;
        }
        let mut indptr = vec![0usize; m + 1];
        let mut indices = Vec::with_capacity(ri.len());
        let mut data = Vec::with_capacity(ri.len());
        for i in 0..m {
            let row = &mut tmp[cnt[i]..cnt[i + 1]];
            row.sort_by_key(|p| p.0);
            let mut k = 0;
            while k < row.len() {
                let c = row[k].0;
                let mut s = 0.0;
                while k < row.len() && row[k].0 == c {
                    s += row[k].1;
                    k += 1;
                }
                if s != 0.0 {
                    indices.push(c);
                    data.push(s);
                }
            }
            indptr[i + 1] = indices.len();
        }
        Ok(Csr { m, n, indptr, indices, data })
    }

    /// `colmajor` is `rows x cols`, column-major.
    pub fn from_dense(rows: usize, cols: usize, colmajor: &[f64]) -> CR<Csr> {
        check_dims(rows, cols)?;
        let nz = colmajor.iter().filter(|v| **v != 0.0).count();
        check_nnz(nz)?;
        let mut indptr = vec![0usize; rows + 1];
        let mut indices = Vec::with_capacity(nz);
        let mut data = Vec::with_capacity(nz);
        for i in 0..rows {
            for j in 0..cols {
                let v = colmajor[j * rows + i];
                if v != 0.0 {
                    indices.push(j);
                    data.push(v);
                }
            }
            indptr[i + 1] = indices.len();
        }
        Ok(Csr { m: rows, n: cols, indptr, indices, data })
    }

    pub fn eye(n: usize) -> CR<Csr> {
        check_dims(n, n)?;
        Ok(Csr { m: n, n, indptr: (0..=n).collect(), indices: (0..n).collect(), data: vec![1.0; n] })
    }

    pub fn diag(v: &[f64]) -> CR<Csr> {
        let n = v.len();
        check_dims(n, n)?;
        let mut indptr = vec![0usize; n + 1];
        let mut indices = vec![];
        let mut data = vec![];
        for i in 0..n {
            if v[i] != 0.0 {
                indices.push(i);
                data.push(v[i]);
            }
            indptr[i + 1] = indices.len();
        }
        Ok(Csr { m: n, n, indptr, indices, data })
    }

    pub fn to_dense(&self) -> CR<Matrix> {
        let total = self.m.checked_mul(self.n).ok_or("size overflows")?;
        if total > MAX_DENSE {
            return Err(format!("a {}x{} matrix has {total} elements; to_dense is limited to {MAX_DENSE}", self.m, self.n));
        }
        let mut d = vec![0.0; total];
        for i in 0..self.m {
            for p in self.indptr[i]..self.indptr[i + 1] {
                d[self.indices[p] * self.m + i] = self.data[p];
            }
        }
        Ok(Matrix::from_col_major(self.m, self.n, d))
    }

    pub fn get(&self, i: usize, j: usize) -> f64 {
        let (a, b) = (self.indptr[i], self.indptr[i + 1]);
        match self.indices[a..b].binary_search(&j) {
            Ok(k) => self.data[a + k],
            Err(_) => 0.0,
        }
    }

    pub fn transpose(&self) -> Csr {
        let mut cnt = vec![0usize; self.n + 1];
        for &c in &self.indices {
            cnt[c + 1] += 1;
        }
        for j in 0..self.n {
            cnt[j + 1] += cnt[j];
        }
        let mut pos = cnt.clone();
        let mut indices = vec![0usize; self.nnz()];
        let mut data = vec![0.0; self.nnz()];
        for i in 0..self.m {
            for p in self.indptr[i]..self.indptr[i + 1] {
                let c = self.indices[p];
                indices[pos[c]] = i;
                data[pos[c]] = self.data[p];
                pos[c] += 1;
            }
        }
        Csr { m: self.n, n: self.m, indptr: cnt, indices, data }
    }

    pub fn scale(&self, k: f64) -> Csr {
        let mut out = self.clone();
        for v in &mut out.data {
            *v *= k;
        }
        if k == 0.0 {
            return Csr { m: self.m, n: self.n, indptr: vec![0; self.m + 1], indices: vec![], data: vec![] };
        }
        out
    }

    /// `alpha*A + beta*B` (same shape); exact cancellations are dropped.
    pub fn lincomb(&self, alpha: f64, other: &Csr, beta: f64) -> CR<Csr> {
        check_nnz(self.nnz() + other.nnz())?;
        let mut indptr = vec![0usize; self.m + 1];
        let mut indices = Vec::with_capacity(self.nnz() + other.nnz());
        let mut data = Vec::with_capacity(self.nnz() + other.nnz());
        for i in 0..self.m {
            let (mut p, pe) = (self.indptr[i], self.indptr[i + 1]);
            let (mut q, qe) = (other.indptr[i], other.indptr[i + 1]);
            while p < pe || q < qe {
                let (c, v);
                if q >= qe || (p < pe && self.indices[p] < other.indices[q]) {
                    c = self.indices[p];
                    v = alpha * self.data[p];
                    p += 1;
                } else if p >= pe || other.indices[q] < self.indices[p] {
                    c = other.indices[q];
                    v = beta * other.data[q];
                    q += 1;
                } else {
                    c = self.indices[p];
                    v = alpha * self.data[p] + beta * other.data[q];
                    p += 1;
                    q += 1;
                }
                if v != 0.0 {
                    indices.push(c);
                    data.push(v);
                }
            }
            indptr[i + 1] = indices.len();
        }
        Ok(Csr { m: self.m, n: self.n, indptr, indices, data })
    }

    pub fn hadamard(&self, other: &Csr) -> Csr {
        let mut indptr = vec![0usize; self.m + 1];
        let mut indices = vec![];
        let mut data = vec![];
        for i in 0..self.m {
            let (mut p, pe) = (self.indptr[i], self.indptr[i + 1]);
            let (mut q, qe) = (other.indptr[i], other.indptr[i + 1]);
            while p < pe && q < qe {
                if self.indices[p] < other.indices[q] {
                    p += 1;
                } else if other.indices[q] < self.indices[p] {
                    q += 1;
                } else {
                    let v = self.data[p] * other.data[q];
                    if v != 0.0 {
                        indices.push(self.indices[p]);
                        data.push(v);
                    }
                    p += 1;
                    q += 1;
                }
            }
            indptr[i + 1] = indices.len();
        }
        Csr { m: self.m, n: self.n, indptr, indices, data }
    }

    pub fn matvec(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; self.m];
        for i in 0..self.m {
            let mut s = 0.0;
            for p in self.indptr[i]..self.indptr[i + 1] {
                s += self.data[p] * x[self.indices[p]];
            }
            y[i] = s;
        }
        y
    }

    /// Sparse * sparse (Gustavson), rows sorted, exact zeros dropped.
    pub fn matmul(&self, b: &Csr) -> CR<Csr> {
        let mut indptr = vec![0usize; self.m + 1];
        let mut indices: Vec<usize> = vec![];
        let mut data: Vec<f64> = vec![];
        let mut acc = vec![0.0; b.n];
        let mut mark = vec![usize::MAX; b.n];
        let mut cols: Vec<usize> = vec![];
        for i in 0..self.m {
            cols.clear();
            for p in self.indptr[i]..self.indptr[i + 1] {
                let k = self.indices[p];
                let a = self.data[p];
                for q in b.indptr[k]..b.indptr[k + 1] {
                    let j = b.indices[q];
                    if mark[j] != i {
                        mark[j] = i;
                        acc[j] = 0.0;
                        cols.push(j);
                    }
                    acc[j] += a * b.data[q];
                }
            }
            cols.sort_unstable();
            for &j in &cols {
                if acc[j] != 0.0 {
                    indices.push(j);
                    data.push(acc[j]);
                }
            }
            check_nnz(indices.len())?;
            indptr[i + 1] = indices.len();
        }
        Ok(Csr { m: self.m, n: b.n, indptr, indices, data })
    }

    pub fn diagonal(&self) -> Vec<f64> {
        (0..self.m.min(self.n)).map(|i| self.get(i, i)).collect()
    }

    pub fn max_abs(&self) -> f64 {
        self.data.iter().fold(0.0, |m, v| m.max(v.abs()))
    }
}

// ------------------------------------------------------------- ordering

/// Reverse Cuthill-McKee ordering of the pattern of A + A^T. `q[k]` is the
/// original index placed at position `k`.
pub fn rcm(a: &Csr) -> Vec<usize> {
    let n = a.n;
    let at = a.transpose();
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for i in 0..n.min(a.m) {
        for &j in &a.indices[a.indptr[i]..a.indptr[i + 1]] {
            if j != i {
                adj[i].push(j);
            }
        }
    }
    for j in 0..n.min(at.m) {
        for &i in &at.indices[at.indptr[j]..at.indptr[j + 1]] {
            if i != j && i < n {
                adj[j].push(i);
            }
        }
    }
    for l in &mut adj {
        l.sort_unstable();
        l.dedup();
    }
    let deg: Vec<usize> = adj.iter().map(|l| l.len()).collect();
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut starts: Vec<usize> = (0..n).collect();
    starts.sort_by_key(|&i| deg[i]);
    for &s in &starts {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let mut head = order.len();
        order.push(s);
        while head < order.len() {
            let v = order[head];
            head += 1;
            let mut nb: Vec<usize> = adj[v].iter().copied().filter(|&w| !seen[w]).collect();
            nb.sort_by_key(|&w| deg[w]);
            for w in nb {
                seen[w] = true;
                order.push(w);
            }
        }
    }
    order.reverse();
    order
}

// ------------------------------------------------------------------ LU

pub struct Lu {
    n: usize,
    q: Vec<usize>,
    pinv: Vec<usize>,
    lp: Vec<usize>,
    li: Vec<usize>,
    lx: Vec<f64>,
    up: Vec<usize>,
    ui: Vec<usize>,
    ux: Vec<f64>,
}

/// Left-looking Gilbert-Peierls LU of `A*Q` with threshold partial pivoting.
pub fn lu_factor(a: &Csr) -> CR<Lu> {
    let n = a.n;
    let at = a.transpose(); // row j of `at` is column j of `a`
    let q = rcm(a);
    let amax = a.max_abs();
    let singular_tol = 1e-13 * amax;
    const NONE: usize = usize::MAX;
    let mut pinv = vec![NONE; n];
    let (mut lp, mut li, mut lx) = (vec![0usize; n + 1], Vec::<usize>::new(), Vec::<f64>::new());
    let (mut up, mut ui, mut ux) = (vec![0usize; n + 1], Vec::<usize>::new(), Vec::<f64>::new());
    let mut x = vec![0.0; n];
    let mut xi = vec![0usize; n];
    let mut mark = vec![0usize; n];
    let mut stack: Vec<(usize, usize)> = vec![];
    for k in 0..n {
        let col = q[k];
        let stamp = k + 1;
        let mut top = n;
        for p in at.indptr[col]..at.indptr[col + 1] {
            let i0 = at.indices[p];
            if mark[i0] == stamp {
                continue;
            }
            mark[i0] = stamp;
            let b0 = if pinv[i0] == NONE { 0 } else { lp[pinv[i0]] };
            stack.push((i0, b0));
            while let Some(&(j, p0)) = stack.last() {
                let jj = pinv[j];
                let end = if jj == NONE { 0 } else { lp[jj + 1] };
                if p0 < end {
                    stack.last_mut().unwrap().1 += 1;
                    let c = li[p0];
                    if mark[c] != stamp {
                        mark[c] = stamp;
                        let bc = if pinv[c] == NONE { 0 } else { lp[pinv[c]] };
                        stack.push((c, bc));
                    }
                } else {
                    top -= 1;
                    xi[top] = j;
                    stack.pop();
                }
            }
        }
        for p in at.indptr[col]..at.indptr[col + 1] {
            x[at.indices[p]] = at.data[p];
        }
        for px in top..n {
            let j = xi[px];
            let jj = pinv[j];
            if jj == NONE {
                continue;
            }
            let xj = x[j];
            for p in lp[jj]..lp[jj + 1] {
                x[li[p]] -= lx[p] * xj;
            }
        }
        let mut ipiv = NONE;
        let mut best = -1.0f64;
        for px in top..n {
            let i = xi[px];
            if pinv[i] == NONE {
                let t = x[i].abs();
                if t > best {
                    best = t;
                    ipiv = i;
                }
            } else {
                ui.push(pinv[i]);
                ux.push(x[i]);
            }
        }
        if ipiv == NONE || !(best > singular_tol) || !best.is_finite() {
            return Err(format!(
                "the matrix is singular (no usable pivot in column {col}; largest candidate {best:e}) -- the system has no unique solution"
            ));
        }
        if pinv[col] == NONE && x[col].abs() >= 0.1 * best {
            ipiv = col;
        }
        let pivot = x[ipiv];
        ui.push(k);
        ux.push(pivot);
        up[k + 1] = ui.len();
        pinv[ipiv] = k;
        for px in top..n {
            let i = xi[px];
            if pinv[i] == NONE {
                li.push(i);
                lx.push(x[i] / pivot);
            }
            x[i] = 0.0;
        }
        lp[k + 1] = li.len();
        if li.len() > MAX_NNZ || ui.len() > MAX_NNZ {
            return Err(format!("LU fill-in exceeds the limit of {MAX_NNZ} stored entries; try method=\"cg\" or \"bicgstab\""));
        }
    }
    for r in &mut li {
        *r = pinv[*r];
    }
    Ok(Lu { n, q, pinv, lp, li, lx, up, ui, ux })
}

impl Lu {
    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        let n = self.n;
        let mut y = vec![0.0; n];
        for i in 0..n {
            y[self.pinv[i]] = b[i];
        }
        for j in 0..n {
            let yj = y[j];
            if yj != 0.0 {
                for p in self.lp[j]..self.lp[j + 1] {
                    y[self.li[p]] -= self.lx[p] * yj;
                }
            }
        }
        for j in (0..n).rev() {
            y[j] /= self.ux[self.up[j + 1] - 1];
            let yj = y[j];
            if yj != 0.0 {
                for p in self.up[j]..self.up[j + 1] - 1 {
                    y[self.ui[p]] -= self.ux[p] * yj;
                }
            }
        }
        let mut x = vec![0.0; n];
        for k in 0..n {
            x[self.q[k]] = y[k];
        }
        x
    }
}

// ------------------------------------------------------------ iterative

pub struct Iter {
    pub x: Vec<f64>,
    pub iterations: usize,
    pub residual: f64,
    pub converged: bool,
    pub status: String,
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

fn true_residual(a: &Csr, x: &[f64], b: &[f64], bnorm: f64) -> f64 {
    let ax = a.matvec(x);
    let r: Vec<f64> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
    norm(&r) / bnorm
}

fn inv_diag(a: &Csr, jacobi: bool, spd: bool) -> CR<Option<Vec<f64>>> {
    if !jacobi {
        return Ok(None);
    }
    let d = a.diagonal();
    for (i, v) in d.iter().enumerate() {
        if *v == 0.0 || (spd && *v < 0.0) {
            return Err(format!("the Jacobi preconditioner needs a {}diagonal, but entry {i} is {v}", if spd { "positive " } else { "nonzero " }));
        }
    }
    Ok(Some(d.iter().map(|v| 1.0 / v).collect()))
}

fn apply_m(minv: &Option<Vec<f64>>, r: &[f64]) -> Vec<f64> {
    match minv {
        Some(d) => r.iter().zip(d).map(|(a, b)| a * b).collect(),
        None => r.to_vec(),
    }
}

pub fn cg(a: &Csr, b: &[f64], tol: f64, maxiter: usize, jacobi: bool) -> CR<Iter> {
    let n = a.n;
    let bnorm = norm(b);
    if bnorm == 0.0 {
        return Ok(Iter { x: vec![0.0; n], iterations: 0, residual: 0.0, converged: true, status: "converged".into() });
    }
    let minv = inv_diag(a, jacobi, true)?;
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let mut z = apply_m(&minv, &r);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let (mut converged, mut iterations) = (false, 0);
    let mut status = format!("did not converge in {maxiter} iterations");
    for it in 0..maxiter {
        let ap = a.matvec(&p);
        let pap = dot(&p, &ap);
        if !(pap > 0.0) || !pap.is_finite() {
            status = format!("breakdown at iteration {it}: p'Ap = {pap:e} is not positive, so the matrix is not symmetric positive definite -- use method=\"bicgstab\" or \"lu\"");
            iterations = it;
            break;
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        iterations = it + 1;
        if norm(&r) <= tol * bnorm {
            converged = true;
            status = "converged".into();
            break;
        }
        z = apply_m(&minv, &r);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
    let residual = true_residual(a, &x, b, bnorm);
    Ok(Iter { x, iterations, residual, converged, status })
}

pub fn bicgstab(a: &Csr, b: &[f64], tol: f64, maxiter: usize, jacobi: bool) -> CR<Iter> {
    let n = a.n;
    let bnorm = norm(b);
    if bnorm == 0.0 {
        return Ok(Iter { x: vec![0.0; n], iterations: 0, residual: 0.0, converged: true, status: "converged".into() });
    }
    let minv = inv_diag(a, jacobi, false)?;
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let rhat = r.clone();
    let (mut rho, mut alpha, mut omega) = (1.0f64, 1.0f64, 1.0f64);
    let mut v = vec![0.0; n];
    let mut p = vec![0.0; n];
    let (mut converged, mut iterations) = (false, 0);
    let mut status = format!("did not converge in {maxiter} iterations");
    for it in 0..maxiter {
        let rho_new = dot(&rhat, &r);
        if rho_new == 0.0 || !rho_new.is_finite() {
            status = format!("breakdown at iteration {it}: rho = {rho_new:e}");
            iterations = it;
            break;
        }
        if it == 0 {
            p.copy_from_slice(&r);
        } else {
            let beta = (rho_new / rho) * (alpha / omega);
            for i in 0..n {
                p[i] = r[i] + beta * (p[i] - omega * v[i]);
            }
        }
        let phat = apply_m(&minv, &p);
        v = a.matvec(&phat);
        let den = dot(&rhat, &v);
        if den == 0.0 || !den.is_finite() {
            status = format!("breakdown at iteration {it}: (rhat, A p) = {den:e}");
            iterations = it;
            break;
        }
        alpha = rho_new / den;
        let s: Vec<f64> = (0..n).map(|i| r[i] - alpha * v[i]).collect();
        iterations = it + 1;
        if norm(&s) <= tol * bnorm {
            for i in 0..n {
                x[i] += alpha * phat[i];
            }
            converged = true;
            status = "converged".into();
            break;
        }
        let shat = apply_m(&minv, &s);
        let t = a.matvec(&shat);
        let tt = dot(&t, &t);
        if tt == 0.0 || !tt.is_finite() {
            status = format!("breakdown at iteration {it}: A s is zero");
            break;
        }
        omega = dot(&t, &s) / tt;
        for i in 0..n {
            x[i] += alpha * phat[i] + omega * shat[i];
            r[i] = s[i] - omega * t[i];
        }
        if norm(&r) <= tol * bnorm {
            converged = true;
            status = "converged".into();
            break;
        }
        if omega == 0.0 {
            status = format!("breakdown at iteration {it}: omega = 0");
            break;
        }
        rho = rho_new;
    }
    let residual = true_residual(a, &x, b, bnorm);
    Ok(Iter { x, iterations, residual, converged, status })
}

// ------------------------------------------------------------ glue

fn is_sparse(v: &Value) -> bool {
    matches!(v, Value::Model(m) if m.kind == "sparse")
}

pub fn to_value(s: Csr) -> Value {
    let f = |v: Vec<usize>| Value::Vec(Arc::new(v.into_iter().map(|x| x as f64).collect()));
    Value::Model(Arc::new(ModelHandle::new(
        "sparse",
        vec![
            ("rows".into(), Value::Num(s.m as f64)),
            ("cols".into(), Value::Num(s.n as f64)),
            ("nnz".into(), Value::Num(s.data.len() as f64)),
            ("indptr".into(), f(s.indptr)),
            ("indices".into(), f(s.indices)),
            ("data".into(), Value::Vec(Arc::new(s.data))),
        ],
    )))
}

fn vec_field(m: &ModelHandle, k: &str) -> Vec<f64> {
    match m.field(k) {
        Some(Value::Vec(v)) => v.as_ref().clone(),
        _ => Vec::new(),
    }
}

fn num_field(m: &ModelHandle, k: &str) -> usize {
    match m.field(k) {
        Some(Value::Num(x)) => *x as usize,
        _ => 0,
    }
}

fn csr_arg(args: &[Value], idx: usize, f: &str) -> R<Csr> {
    match arg_get(args, idx) {
        Some(Value::Model(m)) if m.kind == "sparse" => Ok(Csr {
            m: num_field(m, "rows"),
            n: num_field(m, "cols"),
            indptr: vec_field(m, "indptr").into_iter().map(|x| x as usize).collect(),
            indices: vec_field(m, "indices").into_iter().map(|x| x as usize).collect(),
            data: vec_field(m, "data"),
        }),
        Some(other) => e(format!("{f}: argument {} must be a sparse matrix (from sparse.from_triplets, sparse.from_dense, ...), found {}", idx + 1, other.type_name())),
        None => e(format!("{f}: missing argument {} (a sparse matrix)", idx + 1)),
    }
}

fn dim_arg(args: &[Value], idx: usize, f: &str, what: &str) -> R<usize> {
    match arg_get(args, idx) {
        Some(v) => match v.as_num() {
            Ok(x) if x.is_finite() && x >= 0.0 && x.fract() == 0.0 => Ok(x as usize),
            Ok(x) => e(format!("{f}: {what} must be a non-negative integer, found {x}")),
            Err(_) => e(format!("{f}: {what} must be a number, found {}", v.type_name())),
        },
        None => e(format!("{f}: missing argument {} ({what})", idx + 1)),
    }
}

fn index_vec(args: &[Value], idx: usize, f: &str, what: &str) -> R<Vec<usize>> {
    let v = match arg_get(args, idx) {
        Some(v) => v,
        None => return e(format!("{f}: missing argument {} ({what})", idx + 1)),
    };
    let xs = to_vec(v).map_err(|msg| EvalError { msg: format!("{f}: {what}: {msg}") })?;
    xs.iter()
        .enumerate()
        .map(|(k, x)| {
            if x.is_finite() && *x >= 0.0 && x.fract() == 0.0 {
                Ok(*x as usize)
            } else {
                e(format!("{f}: {what}[{k}] = {x} is not a non-negative integer (indices are zero-based)"))
            }
        })
        .collect()
}

fn num_vec(args: &[Value], idx: usize, f: &str, what: &str) -> R<Vec<f64>> {
    match arg_get(args, idx) {
        Some(v) => to_vec(v).map_err(|msg| EvalError { msg: format!("{f}: {what}: {msg}") }),
        None => e(format!("{f}: missing argument {} ({what})", idx + 1)),
    }
}

fn shape_str(s: &Csr) -> String {
    format!("{}x{}", s.m, s.n)
}

fn vec_value(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

impl Interp {
    pub(crate) fn sparse_call(&mut self, f: &str, args: &[Value], seed: Option<u64>, style: &[(String, Value)]) -> R<Value> {
        let name = f.strip_prefix("sparse::").unwrap_or(f);
        let fq = format!("sparse.{name}");
        let fq = fq.as_str();
        let lift = |msg: String| EvalError { msg: format!("{fq}: {msg}") };
        match name {
            "from_triplets" => {
                let m = dim_arg(args, 3, fq, "rows m")?;
                let n = dim_arg(args, 4, fq, "cols n")?;
                let ri = index_vec(args, 0, fq, "i")?;
                let ci = index_vec(args, 1, fq, "j")?;
                let v = num_vec(args, 2, fq, "v")?;
                Ok(to_value(Csr::from_triplets(m, n, &ri, &ci, &v).map_err(lift)?))
            }
            "from_dense" => match arg_get(args, 0) {
                Some(Value::Mat(a)) => Ok(to_value(Csr::from_dense(a.rows(), a.cols(), a.as_slice()).map_err(lift)?)),
                Some(Value::Vec(v)) => Ok(to_value(Csr::from_dense(v.len(), 1, v).map_err(lift)?)),
                Some(Value::Num(x)) => Ok(to_value(Csr::from_dense(1, 1, &[*x]).map_err(lift)?)),
                Some(other) => e(format!("{fq}: expected a dense matrix, found {}", other.type_name())),
                None => e(format!("{fq}: missing argument 1 (a dense matrix)")),
            },
            "eye" => Ok(to_value(Csr::eye(dim_arg(args, 0, fq, "n")?).map_err(lift)?)),
            "diag" => Ok(to_value(Csr::diag(&num_vec(args, 0, fq, "v")?).map_err(lift)?)),
            "random" => {
                let m = dim_arg(args, 0, fq, "rows m")?;
                let n = dim_arg(args, 1, fq, "cols n")?;
                let density = match arg_get(args, 2).map(|v| v.as_num()) {
                    Some(Ok(d)) if (0.0..=1.0).contains(&d) => d,
                    Some(Ok(d)) => return e(format!("{fq}: density must be in [0, 1], found {d}")),
                    _ => return e(format!("{fq}: needs (m, n, density) with a numeric density in [0, 1]")),
                };
                check_dims(m, n).map_err(lift)?;
                let total = (m as u128) * (n as u128);
                let target = (density * total as f64).round();
                if target > MAX_NNZ as f64 {
                    return e(format!("{fq}: {m}x{n} at density {density} would store about {target} entries, over the limit of {MAX_NNZ}"));
                }
                let target = target as usize;
                let mut local;
                let rng: &mut Rng = match seed {
                    Some(s) => {
                        local = Rng::new(s);
                        &mut local
                    }
                    None => &mut self.rng,
                };
                let (mut ri, mut ci, mut vv) = (Vec::with_capacity(target), Vec::with_capacity(target), Vec::with_capacity(target));
                if total <= (4 * target as u128).max(64) && total <= MAX_DENSE as u128 {
                    // dense-ish: exactly `target` cells chosen by selection sampling
                    let mut need = target as u128;
                    let mut left = total;
                    for i in 0..m {
                        for j in 0..n {
                            if need > 0 && ((rng.uniform() * left as f64) as u128) < need {
                                ri.push(i);
                                ci.push(j);
                                vv.push(rng.uniform().max(f64::MIN_POSITIVE));
                                need -= 1;
                            }
                            left -= 1;
                        }
                    }
                } else {
                    let mut taken = std::collections::HashSet::with_capacity(target);
                    while taken.len() < target {
                        let key = rng.next_u64() % (total as u64);
                        if taken.insert(key) {
                            ri.push((key / n as u64) as usize);
                            ci.push((key % n as u64) as usize);
                            vv.push(rng.uniform().max(f64::MIN_POSITIVE));
                        }
                    }
                }
                Ok(to_value(Csr::from_triplets(m, n, &ri, &ci, &vv).map_err(lift)?))
            }
            "size" => {
                let s = csr_arg(args, 0, fq)?;
                Ok(vec_value(vec![s.m as f64, s.n as f64]))
            }
            "nnz" => Ok(Value::Num(csr_arg(args, 0, fq)?.nnz() as f64)),
            "density" => {
                let s = csr_arg(args, 0, fq)?;
                let total = s.m as f64 * s.n as f64;
                Ok(Value::Num(if total == 0.0 { 0.0 } else { s.nnz() as f64 / total }))
            }
            "to_dense" => Ok(Value::Mat(Arc::new(csr_arg(args, 0, fq)?.to_dense().map_err(lift)?))),
            "get" => {
                // Binary search on the stored arrays: no O(nnz) decode.
                let (i, j) = (dim_arg(args, 1, fq, "row i")?, dim_arg(args, 2, fq, "col j")?);
                match arg_get(args, 0) {
                    Some(Value::Model(m)) if m.kind == "sparse" => {
                        let (rows, cols) = (num_field(m, "rows"), num_field(m, "cols"));
                        if i >= rows || j >= cols {
                            return e(format!("{fq}: ({i}, {j}) is outside the {rows}x{cols} matrix (indices are zero-based)"));
                        }
                        let (ip, ix, dt) = match (m.field("indptr"), m.field("indices"), m.field("data")) {
                            (Some(Value::Vec(a)), Some(Value::Vec(b)), Some(Value::Vec(c))) => (a, b, c),
                            _ => return e(format!("{fq}: corrupt sparse matrix")),
                        };
                        let (a, b) = (ip[i] as usize, ip[i + 1] as usize);
                        let jf = j as f64;
                        Ok(Value::Num(match ix[a..b].binary_search_by(|c| c.partial_cmp(&jf).unwrap()) {
                            Ok(k) => dt[a + k],
                            Err(_) => 0.0,
                        }))
                    }
                    _ => {
                        csr_arg(args, 0, fq)?;
                        e(format!("{fq}: internal"))
                    }
                }
            }
            "triplets" => {
                let s = csr_arg(args, 0, fq)?;
                let mut ri = Vec::with_capacity(s.nnz());
                for i in 0..s.m {
                    for _ in s.indptr[i]..s.indptr[i + 1] {
                        ri.push(i as f64);
                    }
                }
                Ok(Value::Record(Arc::new(vec![
                    ("i".into(), vec_value(ri)),
                    ("j".into(), vec_value(s.indices.iter().map(|&c| c as f64).collect())),
                    ("v".into(), vec_value(s.data.clone())),
                ])))
            }
            "transpose" => Ok(to_value(csr_arg(args, 0, fq)?.transpose())),
            "add" | "sub" | "hadamard" => {
                let a = csr_arg(args, 0, fq)?;
                let b = csr_arg(args, 1, fq)?;
                if (a.m, a.n) != (b.m, b.n) {
                    return e(format!("{fq}: shapes differ: {} vs {}", shape_str(&a), shape_str(&b)));
                }
                Ok(to_value(match name {
                    "add" => a.lincomb(1.0, &b, 1.0).map_err(lift)?,
                    "sub" => a.lincomb(1.0, &b, -1.0).map_err(lift)?,
                    _ => a.hadamard(&b),
                }))
            }
            "scale" => {
                let a = csr_arg(args, 0, fq)?;
                let k = match arg_get(args, 1).map(|v| v.as_num()) {
                    Some(Ok(k)) if k.is_finite() => k,
                    _ => return e(format!("{fq}: needs a finite number as its second argument")),
                };
                Ok(to_value(a.scale(k)))
            }
            "mul" => {
                let a = csr_arg(args, 0, fq)?;
                match arg_get(args, 1) {
                    Some(v) if is_sparse(v) => {
                        let b = csr_arg(args, 1, fq)?;
                        if a.n != b.m {
                            return e(format!("{fq}: inner dimensions differ: {} * {}", shape_str(&a), shape_str(&b)));
                        }
                        Ok(to_value(a.matmul(&b).map_err(lift)?))
                    }
                    Some(Value::Mat(x)) => {
                        if a.n != x.rows() {
                            return e(format!("{fq}: inner dimensions differ: {} * {}x{}", shape_str(&a), x.rows(), x.cols()));
                        }
                        let total = a.m.checked_mul(x.cols()).filter(|t| *t <= MAX_DENSE);
                        if total.is_none() {
                            return e(format!("{fq}: the {}x{} dense result exceeds {MAX_DENSE} elements", a.m, x.cols()));
                        }
                        let mut out = Vec::with_capacity(a.m * x.cols());
                        for j in 0..x.cols() {
                            out.extend(a.matvec(&x.as_slice()[j * x.rows()..(j + 1) * x.rows()]));
                        }
                        Ok(Value::Mat(Arc::new(Matrix::from_col_major(a.m, x.cols(), out))))
                    }
                    Some(v @ (Value::Vec(_) | Value::Signal(..))) => {
                        let x = to_vec(v)?;
                        if a.n != x.len() {
                            return e(format!("{fq}: inner dimensions differ: {} * vector of length {}", shape_str(&a), x.len()));
                        }
                        Ok(vec_value(a.matvec(&x)))
                    }
                    Some(other) => e(format!("{fq}: second argument must be a sparse matrix, a vector or a dense matrix, found {} (use sparse.scale for a scalar)", other.type_name())),
                    None => e(format!("{fq}: needs two arguments")),
                }
            }
            "solve" => {
                let a = csr_arg(args, 0, fq)?;
                if a.m != a.n {
                    return e(format!("{fq}: needs a square matrix, found {}", shape_str(&a)));
                }
                let method = style_str(style, "method").unwrap_or_else(|| "lu".into());
                let tol = style_num(style, "tol").unwrap_or(1e-10);
                if !(tol > 0.0) || !tol.is_finite() {
                    return e(format!("{fq}: tol must be a positive number, found {tol}"));
                }
                let maxiter = match style_num(style, "maxiter") {
                    Some(m) if m >= 1.0 && m.is_finite() && m.fract() == 0.0 => m as usize,
                    Some(m) => return e(format!("{fq}: maxiter must be a positive integer, found {m}")),
                    None => (10 * a.n).max(1000),
                };
                let precond = style_str(style, "precond").unwrap_or_else(|| "none".into());
                if precond != "none" && precond != "jacobi" {
                    return e(format!("{fq}: precond must be \"none\" or \"jacobi\", found \"{precond}\""));
                }
                let jacobi = precond == "jacobi";
                let b_arg = arg_get(args, 1).ok_or_else(|| EvalError { msg: format!("{fq}: missing argument 2 (the right-hand side b)") })?;
                let size_err = |got: String| EvalError { msg: format!("{fq}: b has {got} but the matrix is {}", shape_str(&a)) };
                if method == "lu" {
                    let lu = lu_factor(&a).map_err(lift)?;
                    return match b_arg {
                        Value::Mat(bm) if bm.cols() != 1 => {
                            if bm.rows() != a.n {
                                return Err(size_err(format!("{} rows", bm.rows())));
                            }
                            let mut out = Vec::with_capacity(a.n * bm.cols());
                            for j in 0..bm.cols() {
                                out.extend(lu.solve(&bm.as_slice()[j * a.n..(j + 1) * a.n]));
                            }
                            Ok(Value::Mat(Arc::new(Matrix::from_col_major(a.n, bm.cols(), out))))
                        }
                        other => {
                            let b = to_vec(other)?;
                            if b.len() != a.n {
                                return Err(size_err(format!("length {}", b.len())));
                            }
                            Ok(vec_value(lu.solve(&b)))
                        }
                    };
                }
                if method != "cg" && method != "bicgstab" {
                    return e(format!("{fq}: unknown method \"{method}\" -- use \"lu\", \"cg\" or \"bicgstab\""));
                }
                let b = to_vec(b_arg)?;
                if b.len() != a.n {
                    return Err(size_err(format!("length {}", b.len())));
                }
                if b.iter().any(|v| !v.is_finite()) {
                    return e(format!("{fq}: b contains a non-finite value"));
                }
                let r = if method == "cg" {
                    let skew = a.lincomb(1.0, &a.transpose(), -1.0).map_err(lift)?.max_abs();
                    if skew > 1e-10 * a.max_abs() {
                        return e(format!("{fq}: method=\"cg\" needs a symmetric matrix (|A - A'| max = {skew:e}); use method=\"bicgstab\" or \"lu\""));
                    }
                    cg(&a, &b, tol, maxiter, jacobi).map_err(lift)?
                } else {
                    bicgstab(&a, &b, tol, maxiter, jacobi).map_err(lift)?
                };
                Ok(Value::Record(Arc::new(vec![
                    ("x".into(), vec_value(r.x)),
                    ("iterations".into(), Value::Num(r.iterations as f64)),
                    ("residual".into(), Value::Num(r.residual)),
                    ("converged".into(), Value::Bool(r.converged)),
                    ("status".into(), Value::Str(r.status)),
                    ("method".into(), Value::Str(method)),
                ])))
            }
            other => e(format!("sparse: unknown function `{other}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dense_of(a: &Csr) -> Vec<Vec<f64>> {
        (0..a.m).map(|i| (0..a.n).map(|j| a.get(i, j)).collect()).collect()
    }

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f64) / (1u64 << 31) as f64
    }

    #[test]
    fn triplets_sum_duplicates_and_keep_empty_rows() {
        let a = Csr::from_triplets(4, 3, &[0, 0, 2], &[1, 1, 2], &[1.0, 2.0, 5.0]).unwrap();
        assert_eq!(a.get(0, 1), 3.0);
        assert_eq!(a.get(2, 2), 5.0);
        assert_eq!(a.nnz(), 2);
        assert_eq!(a.indptr, vec![0, 1, 1, 2, 2]);
    }

    #[test]
    fn out_of_range_and_overflow_are_refused() {
        assert!(Csr::from_triplets(2, 2, &[2], &[0], &[1.0]).is_err());
        assert!(Csr::empty(usize::MAX, usize::MAX).is_err());
        assert!(Csr::empty(MAX_DIM + 1, 1).is_err());
    }

    #[test]
    fn lu_matches_dense_on_random_matrices() {
        let mut s = 7u64;
        for n in [1usize, 2, 5, 17, 40] {
            let mut ri = vec![];
            let mut ci = vec![];
            let mut vv = vec![];
            for i in 0..n {
                ri.push(i);
                ci.push(i);
                vv.push(4.0 + lcg(&mut s));
                for _ in 0..3 {
                    ri.push(i);
                    ci.push((lcg(&mut s) * n as f64) as usize % n);
                    vv.push(lcg(&mut s) - 0.5);
                }
            }
            let a = Csr::from_triplets(n, n, &ri, &ci, &vv).unwrap();
            let xs: Vec<f64> = (0..n).map(|i| i as f64 + 1.0).collect();
            let b = a.matvec(&xs);
            let x = lu_factor(&a).unwrap().solve(&b);
            for i in 0..n {
                assert!((x[i] - xs[i]).abs() < 1e-8, "n={n} i={i}: {} vs {}", x[i], xs[i]);
            }
            let t = a.transpose();
            assert_eq!(dense_of(&t.transpose()), dense_of(&a));
        }
    }

    #[test]
    fn singular_is_an_error() {
        let a = Csr::from_triplets(2, 2, &[0, 0, 1, 1], &[0, 1, 0, 1], &[1.0, 2.0, 2.0, 4.0]).unwrap();
        let err = lu_factor(&a).err().unwrap();
        assert!(err.contains("singular"), "{err}");
    }

    #[test]
    fn pivoting_handles_a_zero_diagonal() {
        let a = Csr::from_triplets(2, 2, &[0, 1], &[1, 0], &[1.0, 1.0]).unwrap();
        let x = lu_factor(&a).unwrap().solve(&[3.0, 5.0]);
        assert!((x[0] - 5.0).abs() < 1e-12 && (x[1] - 3.0).abs() < 1e-12);
    }
}
