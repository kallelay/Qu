//! Boundary value problems, PDE building blocks and scalar/2-D numerics
//! (added in v0.4.14): `bvp`, `bvp_eval`, `chebdiff`, `integral2`, `fminbnd`,
//! and the sparse operators `sparse.diffmat` and `sparse.laplacian`.
//!
//! The numerics live in `qu_core::{bvp, fdops, fminbnd}` (with their own unit
//! tests) and in `integrate::quad_adaptive` (the 1-D engine `integral2`
//! nests). This file turns `Value`s into slices and back, words the errors and
//! keeps every user-function callback bounded: `bvp` by its node and call
//! caps, `integral2` by `max_evals`, `fminbnd` by `max_iter`.
//!
//! Keywords are read through `style_*` accessors, so the interpreter's
//! unread-keyword check rejects a misspelt option instead of ignoring it.

use super::{arg_get, e, style_entry, to_vec, EvalError, Interp, ModelHandle, Value, R};
use crate::integrate::quad_adaptive;
use crate::sparse_ops::{self, Csr};
use qu_core::bvp as core_bvp;
use qu_core::fdops::{self, Boundary};
use qu_core::fminbnd as core_fminbnd;
use qu_core::matrix::Matrix;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

pub const NAMES: &[&str] = &["bvp", "bvp_eval", "chebdiff", "fminbnd", "integral2"];

/// Most points `chebdiff` will build (a dense `n x n` matrix).
const MAX_CHEB: usize = 3000;
/// Default cap on evaluations of the integrand in `integral2`.
const DEFAULT_MAX_EVALS: usize = 5_000_000;

fn num_of(who: &str, what: &str, v: &Value) -> R<f64> {
    match v {
        Value::Num(x) => Ok(*x),
        other => e(format!("{who}: {what} must be a number, found {}", other.type_name())),
    }
}

/// `key=` or positional `idx`, never both.
fn opt<'a>(who: &str, args: &'a [Value], style: &'a [(String, Value)], idx: usize, key: &str) -> R<Option<&'a Value>> {
    let kw = style_entry(style, key).map(|(_, v)| v);
    let ps = arg_get(args, idx);
    match (kw, ps) {
        (Some(_), Some(_)) => e(format!("{who}: {key} was given both positionally and as {key}=")),
        (Some(v), None) | (None, Some(v)) => Ok(Some(v)),
        (None, None) => Ok(None),
    }
}

fn opt_num(who: &str, args: &[Value], style: &[(String, Value)], idx: usize, key: &str, default: f64) -> R<f64> {
    match opt(who, args, style, idx, key)? {
        None => Ok(default),
        Some(v) => num_of(who, key, v),
    }
}

fn opt_count(who: &str, args: &[Value], style: &[(String, Value)], idx: usize, key: &str, default: usize, min: usize) -> R<usize> {
    let x = opt_num(who, args, style, idx, key, default as f64)?;
    if x.is_finite() && x >= min as f64 && x.fract() == 0.0 && x < 1e15 {
        Ok(x as usize)
    } else {
        e(format!("{who}: {key} must be a whole number of at least {min}, found {x}"))
    }
}

fn opt_tol(who: &str, args: &[Value], style: &[(String, Value)], idx: usize, default: f64) -> R<f64> {
    let t = opt_num(who, args, style, idx, "tol", default)?;
    if t.is_finite() && t > 0.0 {
        Ok(t)
    } else {
        e(format!("{who}: tol must be a positive number, found {t}"))
    }
}

fn real_vec(who: &str, what: &str, v: &Value) -> R<Vec<f64>> {
    if matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_)) {
        return e(format!("{who}: {what} must be real"));
    }
    if matches!(v, Value::Mat(m) if m.rows() > 1 && m.cols() > 1) {
        return e(format!("{who}: {what} must be a vector, not a matrix"));
    }
    to_vec(v).map_err(|er| EvalError { msg: format!("{who}: {what} must be a numeric vector ({})", er.msg) })
}

fn vec_val(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

fn mat_val(rows: usize, cols: usize, col_major: Vec<f64>) -> Value {
    Value::Mat(Arc::new(Matrix::from_col_major(rows, cols, col_major)))
}

fn record(fields: Vec<(&str, Value)>) -> Value {
    Value::Record(Arc::new(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

fn field<'a>(r: &'a [(String, Value)], k: &str) -> Option<&'a Value> {
    r.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

// ------------------------------------------------------------ sparse side

fn parse_bc(who: &str, v: Option<&Value>, allowed: &[&str], default: &str) -> R<Boundary> {
    let s = match v {
        None => default.to_string(),
        Some(Value::Str(s)) => s.clone(),
        Some(other) => return e(format!("{who}: bc must be text such as \"{default}\", found {}", other.type_name())),
    };
    if !allowed.contains(&s.as_str()) {
        let list: Vec<String> = allowed.iter().map(|a| format!("\"{a}\"")).collect();
        return e(format!("{who}: unknown bc \"{s}\" (use {})", list.join(", ")));
    }
    Ok(match s.as_str() {
        "none" => Boundary::None,
        "periodic" => Boundary::Periodic,
        "dirichlet" => Boundary::Dirichlet,
        _ => Boundary::Neumann,
    })
}

fn whole(who: &str, what: &str, x: f64) -> R<usize> {
    if x.is_finite() && x >= 1.0 && x.fract() == 0.0 && x <= 1e7 {
        Ok(x as usize)
    } else {
        e(format!("{who}: {what} must be a positive whole number, found {x}"))
    }
}

fn to_sparse(who: &str, t: fdops::Triplets) -> R<Value> {
    let c = Csr::from_triplets(t.m, t.n, &t.i, &t.j, &t.v).map_err(|m| EvalError { msg: format!("{who}: {m}") })?;
    Ok(sparse_ops::to_value(c))
}

/// `sparse.diffmat` and `sparse.laplacian`.
pub(crate) fn sparse_pde(name: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let who = format!("sparse.{name}");
    let who = who.as_str();
    if name == "diffmat" {
        let n = match arg_get(args, 0) {
            Some(v) => whole(who, "n", num_of(who, "n", v)?)?,
            None => return e("sparse.diffmat(n, [h=1], [order=1], [bc=\"none\"]) needs at least n"),
        };
        let h = opt_num(who, args, style, 1, "h", 1.0)?;
        let order = opt_num(who, args, style, 2, "order", 1.0)?;
        if order != 1.0 && order != 2.0 {
            return e(format!("{who}: order must be 1 or 2, found {order}"));
        }
        let bc = parse_bc(who, opt(who, args, style, 3, "bc")?, &["none", "periodic", "dirichlet"], "none")?;
        let t = fdops::diffmat(n, h, order as usize, bc).map_err(|m| EvalError { msg: m })?;
        to_sparse(who, t)
    } else {
        let shape: Vec<usize> = match arg_get(args, 0) {
            Some(Value::Num(x)) => vec![whole(who, "shape", *x)?],
            Some(v @ Value::Vec(_)) | Some(v @ Value::Mat(_)) => {
                let s = real_vec(who, "shape", v)?;
                if s.is_empty() || s.len() > 2 {
                    return e(format!("{who}: shape must be n or [nx, ny], found {} entries", s.len()));
                }
                s.iter().map(|x| whole(who, "shape", *x)).collect::<R<Vec<_>>>()?
            }
            Some(other) => return e(format!("{who}: shape must be n or [nx, ny], found {}", other.type_name())),
            None => return e("sparse.laplacian(shape, [h=1], [bc=\"dirichlet\"]) needs a shape"),
        };
        let total = shape.iter().try_fold(1usize, |a, &b| a.checked_mul(b));
        if total.map_or(true, |t| t > sparse_ops::MAX_DIM) {
            return e(format!("{who}: shape {shape:?} is larger than the sparse matrix limit of {} unknowns", sparse_ops::MAX_DIM));
        }
        let h: Vec<f64> = match opt(who, args, style, 1, "h")? {
            None => vec![1.0; shape.len()],
            Some(Value::Num(x)) => vec![*x; shape.len()],
            Some(v) => {
                let hv = real_vec(who, "h", v)?;
                if hv.len() != shape.len() {
                    return e(format!("{who}: h has {} entries but shape has {} direction(s) (or pass one number)", hv.len(), shape.len()));
                }
                hv
            }
        };
        let bc = parse_bc(who, opt(who, args, style, 2, "bc")?, &["dirichlet", "neumann", "periodic"], "dirichlet")?;
        let t = fdops::laplacian(&shape, &h, bc).map_err(|m| EvalError { msg: m })?;
        to_sparse(who, t)
    }
}

// --------------------------------------------------------------- interp side

enum Lim {
    Const(f64),
    Func(String),
}

impl Interp {
    pub(crate) fn numerics4_call(&mut self, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        match f {
            "bvp" => self.bvp_builtin(args, style),
            "bvp_eval" => bvp_eval(args),
            "chebdiff" => {
                let n = match arg_get(args, 0) {
                    Some(v) => num_of(f, "n", v)?,
                    None => return e("chebdiff(n) needs the number of points n"),
                };
                if !(n.is_finite() && n >= 2.0 && n.fract() == 0.0 && n <= MAX_CHEB as f64) {
                    return e(format!("chebdiff: n must be a whole number of points from 2 to {MAX_CHEB}, found {n}"));
                }
                let n = n as usize;
                let (d, x) = fdops::chebdiff(n).map_err(|m| EvalError { msg: m })?;
                let mut cm = vec![0.0; n * n];
                for i in 0..n {
                    for j in 0..n {
                        cm[j * n + i] = d[i * n + j];
                    }
                }
                Ok(record(vec![("D", mat_val(n, n, cm)), ("x", vec_val(x))]))
            }
            "fminbnd" => self.fminbnd_builtin(args, style),
            _ => self.integral2_builtin(args, style),
        }
    }

    /// The name to call a function argument by (a lambda has been given one
    /// by the time it arrives).
    fn fn_arg(&self, who: &str, what: &str, args: &[Value], idx: usize) -> R<String> {
        let name = match arg_get(args, idx) {
            Some(Value::Func(fv)) => match fv.call_name() {
                Some(n) => n.to_string(),
                None => return e(format!("{who}: {what} is a function with no name to call it by")),
            },
            Some(Value::Str(s)) => s.clone(),
            Some(other) => return e(format!("{who}: {what} must be a function, found {}", other.type_name())),
            None => return e(format!("{who}: missing {what}")),
        };
        if !self.has_user_fn(&name) {
            return e(format!("{who}: no user function named `{name}` ({what} must be a Qu function or lambda)"));
        }
        Ok(name)
    }

    fn bvp_builtin(&mut self, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        if args.len() < 4 {
            return e("bvp(f, bc, x, y0, [tol=1e-6], [max_nodes=10000]) needs at least 4 arguments");
        }
        let fname = self.fn_arg("bvp", "f", args, 0)?;
        let bcname = self.fn_arg("bvp", "bc", args, 1)?;
        let x = real_vec("bvp", "x", arg_get(args, 2).unwrap())?;
        if x.len() < 2 {
            return e("bvp: the mesh x needs at least 2 nodes");
        }
        if x.iter().any(|v| !v.is_finite()) || x.windows(2).any(|w| !(w[1] > w[0])) {
            return e("bvp: the mesh x must be finite and strictly increasing");
        }
        let tol = opt_tol("bvp", args, style, 4, 1e-6)?;
        let max_nodes = opt_count("bvp", args, style, 5, "max_nodes", 10_000, 2)?;
        let nn = x.len();
        // initial guess: a matrix with one column per node, or a function of x
        let (n, y0, mut scalar) = match arg_get(args, 3).unwrap() {
            Value::Func(_) | Value::Str(_) => {
                let gname = self.fn_arg("bvp", "y0", args, 3)?;
                let mut y0 = Vec::new();
                let mut n = 0;
                let mut scalar = false;
                for (i, &xi) in x.iter().enumerate() {
                    let r = self.apply(&gname, vec![Value::Num(xi)], Vec::new()).map_err(|er| EvalError { msg: format!("bvp: y0({xi}) failed: {}", er.msg) })?;
                    let v = real_vec("bvp", "the value of y0(x)", &r)?;
                    if i == 0 {
                        n = v.len();
                        scalar = matches!(r, Value::Num(_));
                    } else if v.len() != n {
                        return e(format!("bvp: y0(x) returned {} value(s) at x = {xi} but {n} at the first node", v.len()));
                    }
                    y0.extend(v);
                }
                (n, y0, scalar)
            }
            Value::Mat(m) if m.rows() > 1 || m.cols() > 1 => {
                if m.cols() != nn {
                    return e(format!(
                        "bvp: y0 has {} column(s) but x has {nn} nodes (y0 is one column per node, one row per unknown{})",
                        m.cols(),
                        if m.rows() == nn { "; it looks transposed" } else { "" }
                    ));
                }
                (m.rows(), m.as_slice().to_vec(), m.rows() == 1)
            }
            other => {
                let v = real_vec("bvp", "y0", other)?;
                if v.len() != nn {
                    return e(format!("bvp: y0 has {} entries but x has {nn} nodes (one guess value per node)", v.len()));
                }
                (1, v, true)
            }
        };
        if n == 0 || y0.iter().any(|v| !v.is_finite()) {
            return e("bvp: y0 must be a non-empty finite array");
        }
        if n == 1 {
            scalar = true;
        }
        let cell = RefCell::new(&mut *self);
        let to_arg = |y: &[f64]| if scalar { Value::Num(y[0]) } else { Value::Vec(Arc::new(y.to_vec())) };
        let mut rhs = |xv: f64, y: &[f64]| -> Result<Vec<f64>, String> {
            let r = cell.borrow_mut().apply(&fname, vec![Value::Num(xv), to_arg(y)], Vec::new());
            let r = r.map_err(|er| format!("bvp: f({xv}, y) failed: {}", er.msg))?;
            to_vec(&r).map_err(|er| format!("bvp: f must return a number or vector: {}", er.msg))
        };
        let mut bcf = |ya: &[f64], yb: &[f64]| -> Result<Vec<f64>, String> {
            let r = cell.borrow_mut().apply(&bcname, vec![to_arg(ya), to_arg(yb)], Vec::new());
            let r = r.map_err(|er| format!("bvp: bc(ya, yb) failed: {}", er.msg))?;
            to_vec(&r).map_err(|er| format!("bvp: bc must return a number or vector: {}", er.msg))
        };
        let r = core_bvp::solve_bvp(&mut rhs, &mut bcf, &x, n, &y0, tol, max_nodes).map_err(|m| EvalError { msg: m })?;
        let nodes = r.x.len();
        Ok(record(vec![
            ("x", vec_val(r.x)),
            ("y", mat_val(n, nodes, r.y)),
            ("yp", mat_val(n, nodes, r.yp)),
            ("status", Value::Str(r.status.name().to_string())),
            ("nodes", Value::Num(nodes as f64)),
            ("max_residual", Value::Num(r.max_residual)),
        ]))
    }

    fn fminbnd_builtin(&mut self, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        if args.len() < 3 {
            return e("fminbnd(f, a, b, [tol=1e-8], [max_iter=500]) needs at least 3 arguments");
        }
        let name = self.fn_arg("fminbnd", "f", args, 0)?;
        let a = num_of("fminbnd", "a", arg_get(args, 1).unwrap())?;
        let b = num_of("fminbnd", "b", arg_get(args, 2).unwrap())?;
        if !(a.is_finite() && b.is_finite()) {
            return e(format!("fminbnd: the interval must be finite, found [{a}, {b}]"));
        }
        if a >= b {
            return e(format!("fminbnd: a must be less than b, found a = {a} and b = {b}"));
        }
        let tol = opt_tol("fminbnd", args, style, 3, 1e-8)?;
        let max_iter = opt_count("fminbnd", args, style, 4, "max_iter", 500, 1)?;
        let mut call = |x: f64| -> Result<f64, String> {
            let v = self.apply(&name, vec![Value::Num(x)], Vec::new()).map_err(|er| format!("fminbnd: f({x}) failed: {}", er.msg))?;
            v.as_num().map_err(|_| format!("fminbnd: f must return a number, found {} at x = {x}", v.type_name()))
        };
        let r = core_fminbnd::fminbnd(&mut call, a, b, tol, max_iter).map_err(|m| EvalError { msg: m })?;
        Ok(Value::Model(Arc::new(ModelHandle::new(
            "fminbnd",
            vec![
                ("params".to_string(), Value::Num(r.x)),
                ("value".to_string(), Value::Num(r.fx)),
                ("converged".to_string(), Value::Bool(r.converged)),
                ("iterations".to_string(), Value::Num(r.iterations as f64)),
            ],
        ))))
    }

    fn integral2_builtin(&mut self, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        const SIG: &str = "integral2(f, xa, xb, ya, yb, [tol=1e-8], [max_evals=5000000])";
        if args.len() < 5 {
            return e(format!("{SIG} needs at least 5 arguments"));
        }
        let name = self.fn_arg("integral2", "f", args, 0)?;
        let xa = num_of("integral2", "xa", arg_get(args, 1).unwrap())?;
        let xb = num_of("integral2", "xb", arg_get(args, 2).unwrap())?;
        let mut lims = Vec::new();
        for (idx, what) in [(3usize, "ya"), (4usize, "yb")] {
            lims.push(match arg_get(args, idx).unwrap() {
                Value::Num(c) => Lim::Const(*c),
                Value::Func(_) | Value::Str(_) => Lim::Func(self.fn_arg("integral2", what, args, idx)?),
                other => return e(format!("integral2: {what} must be a number or a function of x, found {}", other.type_name())),
            });
        }
        let tol = opt_tol("integral2", args, style, 5, 1e-8)?;
        let max_evals = opt_count("integral2", args, style, 6, "max_evals", DEFAULT_MAX_EVALS, 1)?;
        let inner_tol = (tol * 0.1).max(1e-13);
        let count = Cell::new(0usize);
        let cell = RefCell::new(&mut *self);
        let call_f = |x: f64, y: f64| -> Result<f64, String> {
            let c = count.get() + 1;
            if c > max_evals {
                return Err(format!(
                    "integral2: f was evaluated more than max_evals={max_evals} times without reaching tol={tol:e}; \
                     loosen tol, raise max_evals, or split the region"
                ));
            }
            count.set(c);
            let v = cell.borrow_mut().apply(&name, vec![Value::Num(x), Value::Num(y)], Vec::new());
            let v = v.map_err(|er| format!("integral2: f({x}, {y}) failed: {}", er.msg))?;
            v.as_num().map_err(|_| format!("integral2: f must return a number, found {} at ({x}, {y})", v.type_name()))
        };
        let lim = |l: &Lim, x: f64| -> Result<f64, String> {
            match l {
                Lim::Const(c) => Ok(*c),
                Lim::Func(n) => {
                    let v = cell.borrow_mut().apply(n, vec![Value::Num(x)], Vec::new());
                    let v = v.map_err(|er| format!("integral2: the y limit function failed at x = {x}: {}", er.msg))?;
                    let c = v.as_num().map_err(|_| format!("integral2: a y limit must return a number, found {} at x = {x}", v.type_name()))?;
                    if c.is_nan() {
                        return Err(format!("integral2: a y limit is NaN at x = {x}"));
                    }
                    Ok(c)
                }
            }
        };
        let mut outer = |x: f64| -> Result<f64, String> {
            let lo = lim(&lims[0], x)?;
            let hi = lim(&lims[1], x)?;
            let mut inner = |y: f64| call_f(x, y);
            let o = quad_adaptive(&mut inner, lo, hi, inner_tol, 60)?;
            Ok(o.value)
        };
        let fix = |m: String| EvalError { msg: if let Some(rest) = m.strip_prefix("quad:") { format!("integral2:{rest}") } else { m } };
        let o = quad_adaptive(&mut outer, xa, xb, tol, 60).map_err(fix)?;
        if o.status != "ok" {
            return e(format!(
                "integral2: did not reach tol={tol:e} ({}): estimate {} with error estimate {:e} after {} evaluations of f; \
                 loosen tol or split the region at its singularities",
                o.status,
                o.value,
                o.error,
                count.get()
            ));
        }
        Ok(Value::Num(o.value))
    }
}

/// `bvp_eval(sol, xq)`: the cubic Hermite interpolant of a `bvp` solution.
fn bvp_eval(args: &[Value]) -> R<Value> {
    let rec = match arg_get(args, 0) {
        Some(Value::Record(r)) => r.clone(),
        Some(other) => return e(format!("bvp_eval: the first argument must be the record returned by bvp, found {}", other.type_name())),
        None => return e("bvp_eval(sol, xq) needs a solution and query points"),
    };
    let bad = || EvalError { msg: "bvp_eval: sol is missing fields x, y, yp; pass the record returned by bvp".into() };
    let x = match field(&rec, "x") {
        Some(Value::Vec(v)) => v.to_vec(),
        _ => return Err(bad()),
    };
    let (n, y) = match field(&rec, "y") {
        Some(Value::Mat(m)) => (m.rows(), m.as_slice().to_vec()),
        _ => return Err(bad()),
    };
    let yp = match field(&rec, "yp") {
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        _ => return Err(bad()),
    };
    if x.len() < 2 || y.len() != n * x.len() || yp.len() != y.len() {
        return Err(bad());
    }
    let xq_v = match arg_get(args, 1) {
        Some(v) => v,
        None => return e("bvp_eval(sol, xq) needs query points xq"),
    };
    let scalar_q = matches!(xq_v, Value::Num(_));
    let xq = real_vec("bvp_eval", "xq", xq_v)?;
    let (lo, hi) = (x[0], *x.last().unwrap());
    let slack = 1e-12 * (hi - lo).abs().max(1.0);
    let mut data = Vec::with_capacity(n * xq.len());
    for &q in &xq {
        if !(q >= lo - slack && q <= hi + slack) {
            return e(format!("bvp_eval: query point {q} is outside the solved interval [{lo}, {hi}]"));
        }
        data.extend(core_bvp::hermite_eval(&x, &y, &yp, n, q.clamp(lo, hi), false));
    }
    Ok(match (n, scalar_q) {
        (1, true) => Value::Num(data[0]),
        (1, false) => vec_val(data),
        (_, true) => vec_val(data),
        (_, false) => mat_val(n, xq.len(), data),
    })
}

