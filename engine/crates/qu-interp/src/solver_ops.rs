//! Solver builtins (added in v0.4.14): `fsolve`, `linprog`, `quadprog`, and the
//! `method="nelder_mead"` branch of `minimize`.
//!
//! The numerics live in `qu_core::{fsolve, lp, qp, nelder_mead}` with their
//! own unit tests; this file turns `Value`s into slices and back, words the
//! errors and keeps every user-function callback bounded (`max_iter`).
//!
//! Keywords are read through `style_*` accessors, so the interpreter's
//! unread-keyword check rejects a misspelt option instead of ignoring it.
//! `Aeq`, `beq`, `lb`, `ub` of `linprog`/`quadprog` are accepted both as
//! keywords and, in MATLAB's order, as positional arguments 4-7 (3-... for
//! `linprog`); giving the same one both ways is an error.

use super::{arg_get, e, style_entry, style_num, text_arg, to_vec, EvalError, Interp, ModelHandle, Value, R};
use qu_core::fsolve as core_fsolve;
use qu_core::lp::{self, Lp};
use qu_core::nelder_mead as core_nm;
use qu_core::qp::{self, Qp};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

pub const NAMES: &[&str] = &["fsolve", "linprog", "quadprog"];

fn msg<T>(r: Result<T, String>) -> R<T> {
    r.map_err(|m| EvalError { msg: m })
}

fn vec_val(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

fn is_complex(v: &Value) -> bool {
    matches!(v, Value::Complex(_) | Value::CVec(_) | Value::CMat(_))
}

fn real_vec(f: &str, what: &str, v: &Value) -> R<Vec<f64>> {
    if is_complex(v) {
        return e(format!("{f}: {what} must be real"));
    }
    if matches!(v, Value::Mat(m) if m.rows() > 1 && m.cols() > 1) {
        return e(format!("{f}: {what} must be a vector, not a matrix"));
    }
    to_vec(v).map_err(|m| EvalError { msg: format!("{f}: {what} must be a numeric vector ({m})") })
}

/// `[]`, an empty vector or a 0-column/0-row matrix: "not given".
fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Vec(x) => x.is_empty(),
        Value::Mat(m) => m.rows() == 0 || m.cols() == 0,
        Value::List(l) => l.is_empty(),
        Value::Nothing => true,
        _ => false,
    }
}

/// A constraint matrix with `n` columns as rows. A plain vector of length `n`
/// is one row; for `n == 1` a vector is a column of one-entry rows.
fn rows_of(f: &str, name: &str, v: &Value, n: usize) -> R<Vec<Vec<f64>>> {
    if is_complex(v) {
        return e(format!("{f}: {name} must be real"));
    }
    match v {
        Value::Mat(m) => {
            if m.cols() != n {
                return e(format!(
                    "{f}: {name} has {} columns but there are {n} variables (one column per variable)",
                    m.cols()
                ));
            }
            (0..m.rows())
                .map(|i| m.row_vec(i).map_err(|er| EvalError { msg: format!("{f}: {name}: {er}") }))
                .collect()
        }
        Value::Num(x) if n == 1 => Ok(vec![vec![*x]]),
        Value::Vec(x) if x.len() == n => Ok(vec![x.to_vec()]),
        Value::Vec(x) if n == 1 => Ok(x.iter().map(|v| vec![*v]).collect()),
        Value::Vec(x) => e(format!("{f}: {name} has {} entries but there are {n} variables", x.len())),
        other => e(format!("{f}: {name} must be a numeric matrix, found {}", other.type_name())),
    }
}

/// A named option that may also be given positionally.
fn opt<'a>(f: &str, args: &'a [Value], style: &'a [(String, Value)], pos: usize, key: &str) -> R<Option<&'a Value>> {
    let kw = style_entry(style, key).map(|(_, v)| v);
    let ps = arg_get(args, pos);
    match (kw, ps) {
        (Some(_), Some(_)) => e(format!("{f}: {key} was given both positionally and as {key}=")),
        (Some(v), None) | (None, Some(v)) => Ok(if is_empty_value(v) { None } else { Some(v) }),
        (None, None) => Ok(None),
    }
}

fn bound_vec(f: &str, name: &str, v: Option<&Value>, n: usize, default: f64) -> R<Vec<f64>> {
    match v {
        None => Ok(vec![default; n]),
        Some(Value::Num(x)) => Ok(vec![*x; n]),
        Some(v) => {
            let b = real_vec(f, name, v)?;
            if b.len() != n {
                return e(format!("{f}: {name} has {} entries but there are {n} variables (or pass one number for all)", b.len()));
            }
            Ok(b)
        }
    }
}

fn pos_int(f: &str, key: &str, default: usize, style: &[(String, Value)]) -> R<usize> {
    match style_num(style, key) {
        None => {
            if let Some((_, v)) = style_entry(style, key) {
                return e(format!("{f}: {key} must be a number, found {}", v.type_name()));
            }
            Ok(default)
        }
        Some(x) if x.is_finite() && x >= 1.0 && x.fract() == 0.0 => Ok(x as usize),
        Some(x) => e(format!("{f}: {key} must be a positive whole number, found {x}")),
    }
}

/// Constraint data shared by `linprog` and `quadprog`; `first` is the
/// positional index of `A`.
struct Constraints {
    a: Vec<Vec<f64>>,
    b: Vec<f64>,
    aeq: Vec<Vec<f64>>,
    beq: Vec<f64>,
    lb: Vec<f64>,
    ub: Vec<f64>,
}

fn constraints(f: &str, n: usize, args: &[Value], style: &[(String, Value)], first: usize) -> R<Constraints> {
    let a_v = arg_get(args, first).filter(|v| !is_empty_value(v));
    let b_v = arg_get(args, first + 1).filter(|v| !is_empty_value(v));
    let (a, b) = match (a_v, b_v) {
        (None, None) => (vec![], vec![]),
        (Some(a), Some(b)) => (rows_of(f, "A", a, n)?, real_vec(f, "b", b)?),
        (Some(_), None) => return e(format!("{f}: A was given without b (A x <= b needs both)")),
        (None, Some(_)) => return e(format!("{f}: b was given without A (A x <= b needs both)")),
    };
    let aeq_v = opt(f, args, style, first + 2, "Aeq")?;
    let beq_v = opt(f, args, style, first + 3, "beq")?;
    let (aeq, beq) = match (aeq_v, beq_v) {
        (None, None) => (vec![], vec![]),
        (Some(a), Some(b)) => (rows_of(f, "Aeq", a, n)?, real_vec(f, "beq", b)?),
        (Some(_), None) => return e(format!("{f}: Aeq was given without beq")),
        (None, Some(_)) => return e(format!("{f}: beq was given without Aeq")),
    };
    let lb = bound_vec(f, "lb", opt(f, args, style, first + 4, "lb")?, n, f64::NEG_INFINITY)?;
    let ub = bound_vec(f, "ub", opt(f, args, style, first + 5, "ub")?, n, f64::INFINITY)?;
    Ok(Constraints { a, b, aeq, beq, lb, ub })
}

fn status_fields(x: Vec<f64>, fval: f64, status: &str, iterations: usize) -> Vec<(String, Value)> {
    vec![
        ("x".to_string(), vec_val(x)),
        ("fval".to_string(), Value::Num(fval)),
        ("status".to_string(), Value::Str(status.to_string())),
        ("iterations".to_string(), Value::Num(iterations as f64)),
    ]
}

impl Interp {
    pub(crate) fn solver_call(&mut self, f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        match f {
            "fsolve" => self.fsolve_builtin(args, style),
            "linprog" => {
                let c = real_vec(f, "c", arg_get(args, 0).ok_or_else(|| EvalError {
                    msg: "linprog(c, A, b, [Aeq=], [beq=], [lb=], [ub=]) needs at least c".into(),
                })?)?;
                let k = constraints(f, c.len(), args, style, 1)?;
                let default_iter = 20_000 + 100 * (c.len() + k.a.len() + k.aeq.len());
                let max_iter = pos_int(f, "max_iter", default_iter, style)?;
                let r = msg(lp::linprog(
                    &Lp { c: &c, a: &k.a, b: &k.b, aeq: &k.aeq, beq: &k.beq, lb: &k.lb, ub: &k.ub },
                    max_iter,
                ))?;
                Ok(Value::Model(Arc::new(ModelHandle::new(
                    "linprog",
                    status_fields(r.x, r.fval, r.status.name(), r.iterations),
                ))))
            }
            "quadprog" => {
                let n_f = arg_get(args, 1).ok_or_else(|| EvalError {
                    msg: "quadprog(H, f, [A], [b], [Aeq=], [beq=], [lb=], [ub=]) needs at least H and f".into(),
                })?;
                let fv = real_vec(f, "f", n_f)?;
                let n = fv.len();
                let h = match arg_get(args, 0) {
                    Some(Value::Num(x)) if n == 1 => vec![vec![*x]],
                    Some(v @ Value::Mat(_)) => {
                        let rows = rows_of(f, "H", v, n)?;
                        if rows.len() != n {
                            return e(format!("quadprog: H must be {n}x{n} to match f, found {}x{n}", rows.len()));
                        }
                        rows
                    }
                    Some(other) => {
                        return e(format!("quadprog: H must be a square matrix, found {}", other.type_name()))
                    }
                    None => return e("quadprog: missing H"),
                };
                let k = constraints(f, n, args, style, 2)?;
                let default_iter = 100 + 2 * (n + k.a.len());
                let max_iter = pos_int(f, "max_iter", default_iter.max(100), style)?;
                let r = msg(qp::quadprog(
                    &Qp { h: &h, f: &fv, a: &k.a, b: &k.b, aeq: &k.aeq, beq: &k.beq, lb: &k.lb, ub: &k.ub },
                    max_iter,
                ))?;
                Ok(Value::Model(Arc::new(ModelHandle::new(
                    "quadprog",
                    status_fields(r.x, r.fval, r.status.name(), r.iterations),
                ))))
            }
            other => e(format!("solver_ops: `{other}` is not handled here")),
        }
    }

    fn fsolve_builtin(&mut self, args: &[Value], style: &[(String, Value)]) -> R<Value> {
        let fn_name = text_arg(args, 0)?;
        if !self.has_user_fn(&fn_name) {
            return e(format!("fsolve: no user function named `{fn_name}`"));
        }
        let x0 = real_vec(
            "fsolve",
            "x0",
            arg_get(args, 1).ok_or_else(|| EvalError { msg: "fsolve(f, x0) needs at least 2 arguments".into() })?,
        )?;
        let tol = match style_num(style, "tol") {
            Some(t) => t,
            None => {
                if let Some((_, v)) = style_entry(style, "tol") {
                    return e(format!("fsolve: tol must be a number, found {}", v.type_name()));
                }
                1e-10
            }
        };
        let max_iter = pos_int("fsolve", "max_iter", 200, style)?;
        let jac_name = match style_entry(style, "jac") {
            None => None,
            Some((_, Value::Str(s))) => Some(s.clone()),
            Some((_, Value::Func(fv))) => match fv.call_name() {
                Some(n) => Some(n.to_string()),
                None => return e("fsolve: jac= is a function with no name to call it by; define it with `function` or pass its name"),
            },
            Some((_, other)) => {
                return e(format!("fsolve: jac= must be a function (or its name), found {}", other.type_name()))
            }
        };
        if let Some(name) = &jac_name {
            if !self.has_user_fn(name) {
                return e(format!("fsolve: no user function named `{name}` (jac=)"));
            }
        }
        let n = x0.len();
        let m_seen = Cell::new(0usize);
        let cell = RefCell::new(self);
        let mut ff = |x: &[f64]| -> Result<Vec<f64>, String> {
            let arg = Value::Vec(Arc::new(x.to_vec()));
            let out = cell.borrow_mut().apply(&fn_name, vec![arg], Vec::new()).map_err(|er| er.msg)?;
            if is_complex(&out) {
                return Err("fsolve: f must return real values".into());
            }
            let v = to_vec(&out).map_err(|er| format!("fsolve: f must return a numeric vector ({})", er.msg))?;
            if m_seen.get() == 0 {
                m_seen.set(v.len());
            }
            Ok(v)
        };
        let mut jj = jac_name.as_ref().map(|name| {
            let cell = &cell;
            let m_seen = &m_seen;
            move |x: &[f64]| -> Result<Vec<f64>, String> {
                let arg = Value::Vec(Arc::new(x.to_vec()));
                let out = cell.borrow_mut().apply(name, vec![arg], Vec::new()).map_err(|er| er.msg)?;
                let m = m_seen.get();
                match &out {
                    Value::Mat(mat) => {
                        if mat.rows() != m || mat.cols() != n {
                            return Err(format!(
                                "fsolve: jac must return a {m}x{n} matrix (rows = equations, columns = unknowns), got {}x{}",
                                mat.rows(),
                                mat.cols()
                            ));
                        }
                        let mut rm = Vec::with_capacity(m * n);
                        for i in 0..m {
                            for j in 0..n {
                                rm.push(mat.get(i, j).map_err(|er| er.to_string())?);
                            }
                        }
                        Ok(rm)
                    }
                    Value::Vec(v) if v.len() == m * n && (m == 1 || n == 1) => Ok(v.to_vec()),
                    Value::Num(x) if m == 1 && n == 1 => Ok(vec![*x]),
                    other => Err(format!(
                        "fsolve: jac must return a {m}x{n} matrix, found {}",
                        other.type_name()
                    )),
                }
            }
        });
        let jac_ref: Option<&mut dyn FnMut(&[f64]) -> Result<Vec<f64>, String>> =
            jj.as_mut().map(|j| j as &mut dyn FnMut(&[f64]) -> Result<Vec<f64>, String>);
        let r = msg(core_fsolve::fsolve(&mut ff, jac_ref, &x0, tol, max_iter))?;
        Ok(Value::Model(Arc::new(ModelHandle::new(
            "fsolve",
            vec![
                ("x".to_string(), vec_val(r.x)),
                ("fval".to_string(), vec_val(r.fval)),
                ("norm".to_string(), Value::Num(r.norm)),
                ("iterations".to_string(), Value::Num(r.iterations as f64)),
                ("converged".to_string(), Value::Bool(r.converged)),
            ],
        ))))
    }

    /// `minimize(f, x0, method="nelder_mead", [max_iter=200*n], [tol=1e-8])`.
    pub(crate) fn minimize_nelder_mead(
        &mut self,
        fn_name: &str,
        x0: &[f64],
        style: &[(String, Value)],
    ) -> R<Value> {
        if style_entry(style, "grad").is_some() {
            return e("minimize: grad= is only used by method=\"bfgs\"; nelder_mead needs no gradient");
        }
        let max_iter = pos_int("minimize", "max_iter", 200 * x0.len().max(1), style)?;
        let tol = style_num(style, "tol").unwrap_or(1e-8);
        let err_slot: std::rc::Rc<RefCell<Option<String>>> = std::rc::Rc::new(RefCell::new(None));
        let cell = RefCell::new(self);
        let mut obj = |p: &[f64]| cell.borrow_mut().call_scalar_of_vec(fn_name, p, &err_slot);
        let r = core_nm::nelder_mead(&mut obj, x0, max_iter, tol);
        if let Some(m) = err_slot.borrow_mut().take() {
            return e(m);
        }
        let r = msg(r)?;
        Ok(Value::Model(Arc::new(ModelHandle::new(
            "minimize",
            vec![
                ("params".to_string(), vec_val(r.params)),
                ("value".to_string(), Value::Num(r.value)),
                ("converged".to_string(), Value::Bool(r.converged)),
            ],
        ))))
    }
}
