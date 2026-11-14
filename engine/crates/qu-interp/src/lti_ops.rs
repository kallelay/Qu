//! `lti_*` builtins: linear time-invariant system objects, v0.4.13.
//!
//! A system is a `Value::Model` of kind `"lti"` whose fields are a
//! state-space realization (`A`, `B`, `C`, `D`, always matrices, and the
//! sample time `dt`, 0 for continuous time) plus the description it was built
//! from: `form` is `"ss"`, `"tf"` (with `num`, `den`) or `"zpk"` (with `z`,
//! `p`, `k`). The numerics are in `qu_core::lti`; this file only moves values
//! in and out.
//!
//! The names carry an `lti_` prefix because `tf`, `zpk`, `step`, `simulate`,
//! `bode` and `impulse` already mean other things in Qu.
//!
//! A filter model (`butter`, `fir1`, ...) is accepted wherever a system is:
//! it is read as the discrete transfer function `tf(filt)` with `dt = 1/fs`.

use crate::{arg_get, e, filter_repr_from_model, style_entry, to_vec, EvalError, FilterRepr, ModelHandle, Value, R};
use qu_core::lti::{self, Dense, Form, Hold, Lti, Method, Ss};
use qu_core::Complex64;
use std::sync::Arc;

pub const NAMES: &[&str] = &[
    "lti_bode",
    "lti_c2d",
    "lti_d2c",
    "lti_dcgain",
    "lti_feedback",
    "lti_freqresp",
    "lti_impulse",
    "lti_is_stable",
    "lti_lsim",
    "lti_margin",
    "lti_parallel",
    "lti_poles",
    "lti_series",
    "lti_ss",
    "lti_step",
    "lti_tf",
    "lti_to_ss",
    "lti_to_tf",
    "lti_to_zpk",
    "lti_zeros",
    "lti_zpk",
];

/// Points in the default time grid of `lti_step`/`lti_impulse` for a
/// continuous system.
const DEFAULT_POINTS: usize = 501;

fn fail<T>(f: &str, msg: impl std::fmt::Display) -> R<T> {
    e(format!("{f}: {msg}"))
}

fn need<'a>(f: &str, sig: &str, args: &'a [Value], idx: usize) -> R<&'a Value> {
    match arg_get(args, idx) {
        Some(v) => Ok(v),
        None => e(format!("{f}: missing argument {} of {sig}", idx + 1)),
    }
}

fn num_of(f: &str, name: &str, v: &Value) -> R<f64> {
    match v {
        Value::Num(x) => Ok(*x),
        other => fail(f, format!("{name} must be a number, found {}", other.type_name())),
    }
}

/// Positional argument `idx`, or the keyword `key=`, as a number.
fn opt_num(f: &str, args: &[Value], idx: usize, style: &[(String, Value)], key: &str) -> R<Option<f64>> {
    if let Some((_, v)) = style_entry(style, key) {
        return num_of(f, key, v).map(Some);
    }
    arg_get(args, idx).map(|v| num_of(f, key, v)).transpose()
}

/// Positional argument `idx`, or the keyword `key=`, as a string.
fn opt_str(f: &str, args: &[Value], idx: usize, style: &[(String, Value)], key: &str) -> R<Option<String>> {
    let v = match style_entry(style, key) {
        Some((_, v)) => Some(v),
        None => arg_get(args, idx),
    };
    match v {
        None => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.to_string())),
        Some(other) => fail(f, format!("{key} must be a string, found {}", other.type_name())),
    }
}

/// A real vector argument (a number counts as one element).
fn real_vec(f: &str, name: &str, v: &Value) -> R<Vec<f64>> {
    match v {
        Value::Num(_) | Value::Vec(_) | Value::Mat(_) | Value::Signal(..) => to_vec(v),
        other => fail(f, format!("{name} must be a numeric vector, found {}", other.type_name())),
    }
}

/// A list of complex numbers: real or complex, scalar or vector.
fn complex_vec(f: &str, name: &str, v: &Value) -> R<Vec<Complex64>> {
    match v {
        Value::Complex(c) => Ok(vec![*c]),
        Value::CVec(c) => Ok(c.to_vec()),
        Value::CMat(c) => Ok(c.as_slice().to_vec()),
        Value::Num(_) | Value::Vec(_) | Value::Mat(_) => Ok(to_vec(v)?.into_iter().map(Complex64::real).collect()),
        other => fail(f, format!("{name} must be a vector of real or complex numbers, found {}", other.type_name())),
    }
}

/// What a matrix argument was written as: a matrix keeps its shape, a
/// number is 1x1, and a plain vector is a row or a column depending on which
/// one fits (decided by the caller).
enum Shape {
    Mat(Dense),
    Flat(Vec<f64>),
}

fn shape_of(f: &str, name: &str, v: &Value) -> R<Shape> {
    match v {
        Value::Num(x) => Ok(Shape::Flat(vec![*x])),
        Value::Vec(xs) => Ok(Shape::Flat(xs.to_vec())),
        Value::Mat(m) => Ok(Shape::Mat(Dense::from_matrix(m))),
        other => fail(f, format!("{name} must be a number, a vector or a matrix, found {}", other.type_name())),
    }
}

fn column(v: Vec<f64>) -> Dense {
    Dense { r: v.len(), c: 1, d: v }
}

fn row(v: Vec<f64>) -> Dense {
    Dense { r: 1, c: v.len(), d: v }
}

fn mat(d: &Dense) -> Value {
    Value::Mat(Arc::new(d.to_matrix()))
}

fn cvec(v: Vec<Complex64>) -> Value {
    Value::CVec(Arc::new(v))
}

fn vec_value(v: Vec<f64>) -> Value {
    Value::Vec(Arc::new(v))
}

fn record(fields: Vec<(&str, Value)>) -> Value {
    Value::Record(Arc::new(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

/// The model value of a system.
pub(crate) fn to_value(sys: &Lti) -> Value {
    let ss = &sys.ss;
    let mut fields: Vec<(String, Value)> = vec![
        (
            "form".to_string(),
            Value::Str(
                match sys.form {
                    Form::Ss => "ss",
                    Form::Tf { .. } => "tf",
                    Form::Zpk { .. } => "zpk",
                }
                .into(),
            ),
        ),
        ("A".to_string(), mat(&ss.a)),
        ("B".to_string(), mat(&ss.b)),
        ("C".to_string(), mat(&ss.c)),
        ("D".to_string(), mat(&ss.d)),
        ("dt".to_string(), Value::Num(ss.dt)),
    ];
    match &sys.form {
        Form::Ss => {}
        Form::Tf { num, den } => {
            fields.push(("num".to_string(), vec_value(num.clone())));
            fields.push(("den".to_string(), vec_value(den.clone())));
        }
        Form::Zpk { z, p, k } => {
            fields.push(("z".to_string(), cvec(z.clone())));
            fields.push(("p".to_string(), cvec(p.clone())));
            fields.push(("k".to_string(), Value::Num(*k)));
        }
    }
    Value::Model(Arc::new(ModelHandle::new("lti", fields)))
}

fn field_dense(f: &str, m: &ModelHandle, name: &str) -> R<Dense> {
    match m.field(name) {
        Some(Value::Mat(x)) => Ok(Dense::from_matrix(x)),
        Some(Value::Num(x)) => Ok(Dense::from_rows(&[&[*x]])),
        _ => fail(f, format!("the lti system has no matrix field `{name}`")),
    }
}

/// Read a system back out of a value. A filter model is read as its discrete
/// transfer function; a number as a static gain when `gain_ok`.
fn system(f: &str, what: &str, v: &Value, gain_ok: bool) -> R<Lti> {
    let m = match v {
        Value::Model(m) => m,
        Value::Num(k) if gain_ok => {
            return Lti::from_tf(&[*k], &[1.0], 0.0).map_err(|msg| EvalError { msg: format!("{f}: {what}: {msg}") });
        }
        other => {
            return fail(
                f,
                format!("{what} must be an lti system (from lti_ss, lti_tf or lti_zpk), found {}", other.type_name()),
            );
        }
    };
    if m.kind == "filter" {
        let (b, a) = match filter_repr_from_model(m)? {
            FilterRepr::Iir(sos) => qu_core::filter::sos_to_tf(&sos),
            FilterRepr::Fir(b) => (b, vec![1.0]),
        };
        // b and a are in ascending powers of z^-1; multiplied through by
        // z^N they are polynomials in z, highest power first.
        let len = b.len().max(a.len());
        let pad = |mut c: Vec<f64>| {
            c.resize(len, 0.0);
            c
        };
        let dt = match m.field("fs") {
            Some(Value::Num(fs)) if *fs > 0.0 => 1.0 / fs,
            _ => 1.0,
        };
        return Lti::from_tf(&pad(b), &pad(a), dt).map_err(|msg| EvalError { msg: format!("{f}: {what}: {msg}") });
    }
    if m.kind != "lti" {
        return fail(f, format!("{what} must be an lti system (from lti_ss, lti_tf or lti_zpk), found a `{}` model", m.kind));
    }
    let dt = match m.field("dt") {
        Some(Value::Num(dt)) => *dt,
        _ => return fail(f, "the lti system has no `dt` field"),
    };
    let ss = Ss::new(field_dense(f, m, "A")?, field_dense(f, m, "B")?, field_dense(f, m, "C")?, field_dense(f, m, "D")?, dt)
        .map_err(|msg| EvalError { msg: format!("{f}: {msg}") })?;
    let form = match m.field("form") {
        Some(Value::Str(s)) if s.as_str() == "tf" => match (m.field("num"), m.field("den")) {
            (Some(n), Some(d)) => Form::Tf { num: to_vec(n)?, den: to_vec(d)? },
            _ => Form::Ss,
        },
        Some(Value::Str(s)) if s.as_str() == "zpk" => match (m.field("z"), m.field("p"), m.field("k")) {
            (Some(z), Some(p), Some(Value::Num(k))) => {
                Form::Zpk { z: complex_vec(f, "z", z)?, p: complex_vec(f, "p", p)?, k: *k }
            }
            _ => Form::Ss,
        },
        _ => Form::Ss,
    };
    Ok(Lti { ss, form })
}

fn lift<T>(f: &str, r: Result<T, String>) -> R<T> {
    r.map_err(|msg| EvalError { msg: format!("{f}: {msg}") })
}

fn method_of(f: &str, name: Option<String>) -> R<Method> {
    match name.as_deref() {
        None | Some("zoh") => Ok(Method::Zoh),
        Some("tustin") | Some("bilinear") => Ok(Method::Tustin),
        Some("euler") | Some("forward") => Ok(Method::Euler),
        Some("backward") => Ok(Method::Backward),
        Some(other) => fail(f, format!("unknown method \"{other}\" (use \"zoh\", \"tustin\", \"euler\" or \"backward\")")),
    }
}

/// `{t, y, x}`: `y` is a vector for one output and a `len(t) x outputs`
/// matrix otherwise; `x` is always `len(t) x states`.
fn sim_value(sim: lti::Sim) -> Value {
    let y = if sim.y.c == 1 { vec_value(sim.y.d.clone()) } else { mat(&sim.y) };
    let x = if sim.x.c == 0 { vec_value(Vec::new()) } else { mat(&sim.x) };
    record(vec![("t", vec_value(sim.t)), ("y", y), ("x", x)])
}

/// The time vector of `lti_step`/`lti_impulse`: an end time (a number) or
/// the sample times themselves.
fn times(f: &str, sys: &Lti, v: &Value, style: &[(String, Value)]) -> R<Vec<f64>> {
    let points = match style_entry(style, "n") {
        Some((_, v)) => {
            let n = num_of(f, "n", v)?;
            if !(n >= 2.0 && n.fract() == 0.0 && n <= 1e7) {
                return fail(f, format!("n must be a whole number of points, at least 2, got {n}"));
            }
            Some(n as usize)
        }
        None => None,
    };
    match v {
        Value::Num(t_end) => {
            if points.is_some() && sys.ss.is_discrete() {
                return fail(f, "n= does not apply to a discrete system, which is sampled every dt");
            }
            lift(f, lti::time_grid(&sys.ss, *t_end, points.unwrap_or(DEFAULT_POINTS)))
        }
        _ => {
            if points.is_some() {
                return fail(f, "n= sets the number of points up to an end time; it does not apply when a time vector is given");
            }
            real_vec(f, "t", v)
        }
    }
}

fn input_index(f: &str, style: &[(String, Value)]) -> R<usize> {
    match style_entry(style, "input") {
        None => Ok(0),
        Some((_, v)) => {
            let k = num_of(f, "input", v)?;
            if !(k >= 0.0 && k.fract() == 0.0) {
                return fail(f, format!("input must be a whole number, counted from 0, got {k}"));
            }
            Ok(k as usize)
        }
    }
}

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "lti_ss" => {
            let sig = "lti_ss(A, B, C, D, [dt])";
            let a = match shape_of(f, "A", need(f, sig, args, 0)?)? {
                Shape::Mat(m) => m,
                Shape::Flat(v) if v.len() == 1 => column(v),
                Shape::Flat(v) => {
                    return fail(f, format!("A must be a square matrix, found a vector of {} elements (write [a, b; c, d])", v.len()));
                }
            };
            let n = a.r;
            let b = match shape_of(f, "B", need(f, sig, args, 1)?)? {
                Shape::Mat(m) => m,
                Shape::Flat(v) if n == 1 && v.len() != 1 => row(v),
                Shape::Flat(v) => column(v),
            };
            let c = match shape_of(f, "C", need(f, sig, args, 2)?)? {
                Shape::Mat(m) => m,
                Shape::Flat(v) if n == 1 && v.len() != 1 => column(v),
                Shape::Flat(v) => row(v),
            };
            let (p, m) = (c.r, b.c);
            let d = match shape_of(f, "D", need(f, sig, args, 3)?)? {
                Shape::Mat(d) => d,
                Shape::Flat(v) if v.len() == 1 && v[0] == 0.0 => Dense::zeros(p, m),
                Shape::Flat(v) if m == 1 && v.len() == p => column(v),
                Shape::Flat(v) => row(v),
            };
            let dt = opt_num(f, args, 4, style, "dt")?.unwrap_or(0.0);
            Ok(to_value(&Lti::from_ss(lift(f, Ss::new(a, b, c, d, dt))?)))
        }
        "lti_tf" => {
            let sig = "lti_tf(num, den, [dt])";
            let num = real_vec(f, "num", need(f, sig, args, 0)?)?;
            let den = real_vec(f, "den", need(f, sig, args, 1)?)?;
            let dt = opt_num(f, args, 2, style, "dt")?.unwrap_or(0.0);
            Ok(to_value(&lift(f, Lti::from_tf(&num, &den, dt))?))
        }
        "lti_zpk" => {
            let sig = "lti_zpk(z, p, k, [dt])";
            let z = complex_vec(f, "z", need(f, sig, args, 0)?)?;
            let p = complex_vec(f, "p", need(f, sig, args, 1)?)?;
            let k = num_of(f, "k", need(f, sig, args, 2)?)?;
            let dt = opt_num(f, args, 3, style, "dt")?.unwrap_or(0.0);
            Ok(to_value(&lift(f, Lti::from_zpk(&z, &p, k, dt))?))
        }
        "lti_to_ss" => {
            let sys = system(f, "the argument", need(f, "lti_to_ss(sys)", args, 0)?, false)?;
            Ok(to_value(&Lti::from_ss(sys.ss)))
        }
        "lti_to_tf" => {
            let sys = system(f, "the argument", need(f, "lti_to_tf(sys)", args, 0)?, false)?;
            let (num, den) = lift(f, sys.tf())?;
            Ok(to_value(&Lti { ss: sys.ss, form: Form::Tf { num, den } }))
        }
        "lti_to_zpk" => {
            let sys = system(f, "the argument", need(f, "lti_to_zpk(sys)", args, 0)?, false)?;
            let (z, p, k) = lift(f, sys.zpk())?;
            Ok(to_value(&Lti { ss: sys.ss, form: Form::Zpk { z, p, k } }))
        }
        "lti_poles" => {
            let sys = system(f, "the argument", need(f, "lti_poles(sys)", args, 0)?, false)?;
            Ok(cvec(lift(f, sys.poles())?))
        }
        "lti_zeros" => {
            let sys = system(f, "the argument", need(f, "lti_zeros(sys)", args, 0)?, false)?;
            Ok(cvec(lift(f, sys.zeros())?))
        }
        "lti_is_stable" => {
            let sys = system(f, "the argument", need(f, "lti_is_stable(sys)", args, 0)?, false)?;
            Ok(Value::Bool(lift(f, sys.is_stable())?))
        }
        "lti_dcgain" => {
            let sys = system(f, "the argument", need(f, "lti_dcgain(sys)", args, 0)?, false)?;
            let g = lift(f, sys.dcgain())?;
            Ok(if g.d.len() == 1 { Value::Num(g.d[0]) } else { mat(&g) })
        }
        "lti_freqresp" => {
            let sig = "lti_freqresp(sys, w)";
            let sys = system(f, "the first argument", need(f, sig, args, 0)?, false)?;
            let wv = need(f, sig, args, 1)?;
            let h = lift(f, sys.freqresp(&real_vec(f, "w", wv)?))?;
            Ok(match wv {
                Value::Num(_) => Value::Complex(h[0]),
                _ => cvec(h),
            })
        }
        "lti_bode" => {
            let sys = system(f, "the first argument", need(f, "lti_bode(sys, [w])", args, 0)?, false)?;
            let w = match arg_get(args, 1) {
                Some(v) => real_vec(f, "w", v)?,
                None => lift(f, lti::default_frequencies(&sys))?,
            };
            let b = lift(f, lti::bode(&sys, &w))?;
            Ok(record(vec![("w", vec_value(b.w)), ("mag_db", vec_value(b.mag_db)), ("phase_deg", vec_value(b.phase_deg))]))
        }
        "lti_margin" => {
            let sys = system(f, "the argument", need(f, "lti_margin(sys)", args, 0)?, false)?;
            let m = lift(f, lti::margins(&sys))?;
            Ok(record(vec![
                ("gm", Value::Num(m.gm)),
                ("gm_db", Value::Num(20.0 * m.gm.log10())),
                ("pm", Value::Num(m.pm)),
                ("w_gm", Value::Num(m.w_gm)),
                ("w_pm", Value::Num(m.w_pm)),
                ("stable", Value::Bool(m.stable)),
            ]))
        }
        "lti_step" | "lti_impulse" => {
            let sig = if f == "lti_step" { "lti_step(sys, t)" } else { "lti_impulse(sys, t)" };
            let sys = system(f, "the first argument", need(f, sig, args, 0)?, false)?;
            let t = times(f, &sys, need(f, sig, args, 1)?, style)?;
            let input = input_index(f, style)?;
            let sim = if f == "lti_step" { lti::step(&sys.ss, &t, input) } else { lti::impulse(&sys.ss, &t, input) };
            Ok(sim_value(lift(f, sim)?))
        }
        "lti_lsim" => {
            let sig = "lti_lsim(sys, u, t, [x0])";
            let sys = system(f, "the first argument", need(f, sig, args, 0)?, false)?;
            let t = real_vec(f, "t", need(f, sig, args, 2)?)?;
            let m = sys.ss.inputs();
            let u = match shape_of(f, "u", need(f, sig, args, 1)?)? {
                Shape::Flat(v) if m == 1 => column(v),
                Shape::Mat(d) if m == 1 && (d.r == 1 || d.c == 1) => column(d.d),
                Shape::Mat(d) => d,
                Shape::Flat(v) => {
                    return fail(
                        f,
                        format!("the system has {m} inputs, so u must be a {}x{m} matrix (one column per input), found a vector of {} elements", t.len(), v.len()),
                    );
                }
            };
            let x0 = match style_entry(style, "x0") {
                Some((_, v)) => Some(real_vec(f, "x0", v)?),
                None => arg_get(args, 3).map(|v| real_vec(f, "x0", v)).transpose()?,
            };
            let hold = match opt_str(f, &[], 0, style, "method")?.as_deref() {
                None | Some("foh") | Some("linear") => Hold::Foh,
                Some("zoh") => Hold::Zoh,
                Some(other) => return fail(f, format!("unknown method \"{other}\" (use \"foh\" or \"zoh\")")),
            };
            Ok(sim_value(lift(f, lti::lsim(&sys.ss, &u, &t, x0.as_deref(), hold))?))
        }
        "lti_series" | "lti_parallel" => {
            let sig = if f == "lti_series" { "lti_series(a, b)" } else { "lti_parallel(a, b)" };
            let a = system(f, "the first argument", need(f, sig, args, 0)?, true)?;
            let b = system(f, "the second argument", need(f, sig, args, 1)?, true)?;
            let ss = if f == "lti_series" { lti::series(&a.ss, &b.ss) } else { lti::parallel(&a.ss, &b.ss) };
            Ok(to_value(&Lti::from_ss(lift(f, ss)?)))
        }
        "lti_feedback" => {
            let sig = "lti_feedback(plant, [controller], [sign=-1])";
            let g = system(f, "the plant", need(f, sig, args, 0)?, true)?;
            let h = match arg_get(args, 1) {
                Some(v) => system(f, "the controller", v, true)?,
                None => {
                    if g.ss.inputs() != g.ss.outputs() {
                        return fail(
                            f,
                            format!(
                                "unity feedback needs as many outputs as inputs; the plant has {} input(s) and {} output(s)",
                                g.ss.inputs(),
                                g.ss.outputs()
                            ),
                        );
                    }
                    Lti::from_ss(Ss::gain(Dense::eye(g.ss.outputs()), g.ss.dt))
                }
            };
            let sign = opt_num(f, args, 2, style, "sign")?.unwrap_or(-1.0);
            Ok(to_value(&Lti::from_ss(lift(f, lti::feedback(&g.ss, &h.ss, sign))?)))
        }
        "lti_c2d" => {
            let sig = "lti_c2d(sys, dt, [method=\"zoh\"])";
            let sys = system(f, "the first argument", need(f, sig, args, 0)?, false)?;
            let dt = num_of(f, "dt", need(f, sig, args, 1)?)?;
            let method = method_of(f, opt_str(f, args, 2, style, "method")?)?;
            Ok(to_value(&Lti::from_ss(lift(f, lti::c2d(&sys.ss, dt, method))?)))
        }
        "lti_d2c" => {
            let sig = "lti_d2c(sys, [method=\"zoh\"])";
            let sys = system(f, "the first argument", need(f, sig, args, 0)?, false)?;
            let method = method_of(f, opt_str(f, args, 1, style, "method")?)?;
            Ok(to_value(&Lti::from_ss(lift(f, lti::d2c(&sys.ss, method))?)))
        }
        other => e(format!("lti_ops: `{other}` is not handled here")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[f64]) -> Value {
        Value::Vec(Arc::new(xs.to_vec()))
    }

    fn m(rows: &[&[f64]]) -> Value {
        mat(&Dense::from_rows(rows))
    }

    fn go(f: &str, args: &[Value]) -> Value {
        call(f, args, &[]).unwrap_or_else(|err| panic!("{f}: {}", err.msg))
    }

    fn go_kw(f: &str, args: &[Value], style: &[(&str, Value)]) -> R<Value> {
        let style: Vec<(String, Value)> = style.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        call(f, args, &style)
    }

    fn msg(f: &str, args: &[Value]) -> String {
        match call(f, args, &[]) {
            Ok(_) => panic!("{f} should have failed"),
            Err(err) => err.msg,
        }
    }

    fn field(val: &Value, name: &str) -> Value {
        match val {
            Value::Model(h) => h.field(name).unwrap_or_else(|| panic!("no field {name}")).clone(),
            Value::Record(r) => r.iter().find(|(k, _)| k == name).unwrap_or_else(|| panic!("no field {name}")).1.clone(),
            other => panic!("not a model or record: {other:?}"),
        }
    }

    fn form(val: &Value) -> String {
        match field(val, "form") {
            Value::Str(s) => s.to_string(),
            other => panic!("form is {other:?}"),
        }
    }

    fn reals(val: &Value) -> Vec<f64> {
        match val {
            Value::Vec(x) => x.to_vec(),
            Value::Mat(x) => Dense::from_matrix(x).d,
            Value::Num(x) => vec![*x],
            other => panic!("not real: {other:?}"),
        }
    }

    fn complexes(val: &Value) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = match val {
            Value::CVec(x) => x.iter().map(|c| (c.re, c.im)).collect(),
            Value::Complex(c) => vec![(c.re, c.im)],
            other => panic!("not complex: {other:?}"),
        };
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        out
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    fn lag(tau: f64) -> Value {
        go("lti_tf", &[v(&[1.0]), v(&[tau, 1.0])])
    }

    #[test]
    fn names_are_sorted_and_all_dispatch() {
        let mut sorted = NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, NAMES);
        for name in NAMES {
            // every name reaches its own arm: with no arguments each one
            // reports a missing argument, not "not handled here"
            let text = msg(name, &[]);
            assert!(text.starts_with(&format!("{name}: missing argument 1")), "{name}: {text}");
        }
    }

    #[test]
    fn lti_ss_shapes_vectors_and_validates() {
        let sys = go("lti_ss", &[m(&[&[0.0, 1.0], &[-2.0, -3.0]]), v(&[0.0, 1.0]), v(&[1.0, 0.0]), Value::Num(0.0)]);
        assert!(matches!(&sys, Value::Model(h) if h.kind == "lti"));
        assert_eq!(form(&sys), "ss");
        // a plain vector became a column for B and a row for C
        assert!(matches!(field(&sys, "B"), Value::Mat(b) if b.shape() == (2, 1)));
        assert!(matches!(field(&sys, "C"), Value::Mat(c) if c.shape() == (1, 2)));
        assert!(matches!(field(&sys, "D"), Value::Mat(d) if d.shape() == (1, 1)));
        assert_eq!(reals(&field(&sys, "dt")), vec![0.0]);
        // scalars make a first-order system; dt positional or named
        let d = go("lti_ss", &[Value::Num(0.5), Value::Num(1.0), Value::Num(1.0), Value::Num(0.0), Value::Num(0.1)]);
        assert_eq!(reals(&field(&d, "dt")), vec![0.1]);
        let d = go_kw("lti_ss", &[Value::Num(0.5), Value::Num(1.0), Value::Num(1.0), Value::Num(0.0)], &[("dt", Value::Num(0.2))]).unwrap();
        assert_eq!(reals(&field(&d, "dt")), vec![0.2]);
        // one state, two inputs, two outputs
        let mimo = go("lti_ss", &[Value::Num(-1.0), v(&[1.0, 2.0]), v(&[3.0, 4.0]), Value::Num(0.0)]);
        assert!(matches!(field(&mimo, "B"), Value::Mat(b) if b.shape() == (1, 2)));
        assert!(matches!(field(&mimo, "C"), Value::Mat(c) if c.shape() == (2, 1)));
        assert!(matches!(field(&mimo, "D"), Value::Mat(d) if d.shape() == (2, 2)));
        // errors name the offending matrix
        let a = m(&[&[0.0, 1.0], &[-2.0, -3.0]]);
        assert!(msg("lti_ss", &[v(&[1.0, 2.0, 3.0]), v(&[1.0]), v(&[1.0]), Value::Num(0.0)]).contains("A must be a square matrix"));
        assert!(msg("lti_ss", &[m(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]), v(&[1.0]), v(&[1.0]), Value::Num(0.0)]).contains("A must be square, got 2x3"));
        assert!(msg("lti_ss", &[a.clone(), v(&[1.0, 2.0, 3.0]), v(&[1.0, 0.0]), Value::Num(0.0)]).contains("B must have 2 row"));
        assert!(msg("lti_ss", &[a.clone(), v(&[0.0, 1.0]), v(&[1.0, 0.0, 0.0]), Value::Num(0.0)]).contains("C must have 2 column"));
        assert!(msg("lti_ss", &[a.clone(), v(&[0.0, 1.0]), v(&[1.0, 0.0]), v(&[1.0, 2.0])]).contains("D must be 1x1"));
        assert!(msg("lti_ss", &[a.clone(), v(&[0.0, 1.0]), v(&[1.0, 0.0])]).contains("missing argument 4"));
        assert!(msg("lti_ss", &[a, v(&[0.0, 1.0]), v(&[1.0, 0.0]), Value::Num(0.0), Value::Num(-1.0)]).contains("dt"));
        assert!(msg("lti_ss", &[Value::Str("a".into()), v(&[1.0]), v(&[1.0]), Value::Num(0.0)]).contains("found string"));
    }

    #[test]
    fn lti_tf_and_lti_zpk_keep_their_original_form() {
        let sys = go("lti_tf", &[v(&[2.0, 3.0]), v(&[1.0, 4.0, 5.0])]);
        assert_eq!(form(&sys), "tf");
        assert_eq!(reals(&field(&sys, "num")), vec![2.0, 3.0]);
        assert_eq!(reals(&field(&sys, "den")), vec![1.0, 4.0, 5.0]);
        assert_eq!(reals(&field(&sys, "A")), vec![0.0, 1.0, -5.0, -4.0]);
        assert!(msg("lti_tf", &[v(&[1.0, 0.0, 0.0]), v(&[1.0, 1.0])]).contains("improper"));
        assert!(msg("lti_tf", &[v(&[1.0]), v(&[0.0])]).contains("denominator is zero"));
        let z = go("lti_zpk", &[v(&[-3.0]), cvec(vec![Complex64::new(-1.0, 2.0), Complex64::new(-1.0, -2.0)]), Value::Num(5.0)]);
        assert_eq!(form(&z), "zpk");
        assert_eq!(complexes(&field(&z, "p")), vec![(-1.0, -2.0), (-1.0, 2.0)]);
        assert_eq!(reals(&field(&z, "k")), vec![5.0]);
        assert!(msg("lti_zpk", &[v(&[]), cvec(vec![Complex64::new(-1.0, 2.0)]), Value::Num(1.0)]).contains("conjugate pairs"));
        assert!(msg("lti_zpk", &[v(&[1.0, 2.0]), v(&[-1.0]), Value::Num(1.0)]).contains("improper"));
        assert!(msg("lti_zpk", &[v(&[]), v(&[-1.0]), v(&[1.0, 2.0])]).contains("k must be a number"));
    }

    #[test]
    fn conversions() {
        let z = go("lti_zpk", &[v(&[-3.0]), v(&[-1.0, -2.0]), Value::Num(2.0)]);
        let t = go("lti_to_tf", &[z.clone()]);
        assert_eq!(form(&t), "tf");
        assert_eq!(reals(&field(&t, "num")), vec![2.0, 6.0]);
        assert_eq!(reals(&field(&t, "den")), vec![1.0, 3.0, 2.0]);
        let s = go("lti_to_ss", &[t.clone()]);
        assert_eq!(form(&s), "ss");
        assert!(matches!(&s, Value::Model(h) if h.field("num").is_none()));
        // back from the bare realization
        let t2 = go("lti_to_tf", &[s.clone()]);
        assert_eq!(reals(&field(&t2, "num")), vec![2.0, 6.0]);
        assert_eq!(reals(&field(&t2, "den")), vec![1.0, 3.0, 2.0]);
        let z2 = go("lti_to_zpk", &[s]);
        assert_eq!(form(&z2), "zpk");
        let zs = complexes(&field(&z2, "z"));
        close(zs[0].0, -3.0, 1e-12);
        close(reals(&field(&z2, "k"))[0], 2.0, 1e-14);
        // MIMO has no single transfer function
        let mimo = go("lti_ss", &[Value::Num(-1.0), v(&[1.0, 2.0]), v(&[3.0, 4.0]), Value::Num(0.0)]);
        assert!(msg("lti_to_tf", &[mimo.clone()]).contains("2 input(s) and 2 output(s)"));
        assert!(msg("lti_to_zpk", &[mimo.clone()]).contains("2 input(s)"));
        assert!(matches!(go("lti_to_ss", &[mimo]), Value::Model(_)));
        assert!(msg("lti_to_tf", &[Value::Num(3.0)]).contains("must be an lti system"));
    }

    #[test]
    fn analysis_builtins() {
        let sys = go("lti_tf", &[v(&[1.0, 5.0]), v(&[1.0, 3.0, 2.0])]);
        let p = complexes(&go("lti_poles", &[sys.clone()]));
        close(p[0].0, -2.0, 1e-12);
        close(p[1].0, -1.0, 1e-12);
        let z = complexes(&go("lti_zeros", &[sys.clone()]));
        assert_eq!(z.len(), 1);
        close(z[0].0, -5.0, 1e-12);
        assert!(matches!(go("lti_is_stable", &[sys.clone()]), Value::Bool(true)));
        assert!(matches!(go("lti_is_stable", &[go("lti_tf", &[v(&[1.0]), v(&[1.0, -1.0])])]), Value::Bool(false)));
        // discrete: |p| < 1
        assert!(matches!(go("lti_is_stable", &[go("lti_tf", &[v(&[1.0]), v(&[1.0, 0.9]), Value::Num(1.0)])]), Value::Bool(true)));
        assert!(matches!(go("lti_is_stable", &[go("lti_tf", &[v(&[1.0]), v(&[1.0, 1.1]), Value::Num(1.0)])]), Value::Bool(false)));
        close(reals(&go("lti_dcgain", &[sys.clone()]))[0], 2.5, 1e-14);
        // H(j) = (5 + j) / (1 + 3j)
        let h = go("lti_freqresp", &[sys.clone(), v(&[1.0])]);
        let h = complexes(&h)[0];
        close(h.0, 0.8, 1e-14);
        close(h.1, -1.4, 1e-14);
        // a scalar frequency gives a scalar
        assert!(matches!(go("lti_freqresp", &[sys.clone(), Value::Num(1.0)]), Value::Complex(_)));
        let mimo = go("lti_ss", &[Value::Num(-1.0), v(&[1.0, 2.0]), v(&[3.0, 4.0]), Value::Num(0.0)]);
        assert!(msg("lti_zeros", &[mimo.clone()]).contains("one input and one output"));
        assert!(msg("lti_freqresp", &[mimo.clone(), v(&[1.0])]).contains("one input and one output"));
        // MIMO dc gain is a matrix: C B / 1
        assert_eq!(reals(&go("lti_dcgain", &[mimo])), vec![3.0, 6.0, 4.0, 8.0]);
    }

    #[test]
    fn simulation_builtins() {
        let tau = 0.5;
        let r = go("lti_step", &[lag(tau), Value::Num(2.0)]);
        let (t, y) = (reals(&field(&r, "t")), reals(&field(&r, "y")));
        assert_eq!(t.len(), 501);
        assert_eq!(*t.last().unwrap(), 2.0);
        for (tk, yk) in t.iter().zip(&y) {
            close(*yk, 1.0 - (-tk / tau).exp(), 1e-14);
        }
        assert!(matches!(field(&r, "x"), Value::Mat(x) if x.shape() == (501, 1)));
        // n= picks the number of points; a vector is used as given
        let r = go_kw("lti_step", &[lag(tau), Value::Num(2.0)], &[("n", Value::Num(5.0))]).unwrap();
        assert_eq!(reals(&field(&r, "t")), vec![0.0, 0.5, 1.0, 1.5, 2.0]);
        let r = go("lti_step", &[lag(tau), v(&[0.0, 0.1, 0.7])]);
        close(reals(&field(&r, "y"))[2], 1.0 - (-0.7f64 / tau).exp(), 1e-15);
        let r = go("lti_impulse", &[lag(tau), v(&[0.0, 0.3])]);
        close(reals(&field(&r, "y"))[1], (-0.3f64 / tau).exp() / tau, 1e-14);
        // lsim: ramp into an integrator, exact under the default first-order hold
        let integ = go("lti_tf", &[v(&[1.0]), v(&[1.0, 0.0])]);
        let t = v(&[0.0, 0.5, 1.0, 1.5]);
        let r = go("lti_lsim", &[integ.clone(), t.clone(), t.clone()]);
        assert_eq!(reals(&field(&r, "y")), vec![0.0, 0.125, 0.5, 1.125]);
        let r = go_kw("lti_lsim", &[integ.clone(), t.clone(), t.clone()], &[("method", Value::Str("zoh".into()))]).unwrap();
        assert_eq!(reals(&field(&r, "y")), vec![0.0, 0.0, 0.25, 0.75]);
        // x0 positional and named
        let zero = v(&[0.0, 0.0, 0.0, 0.0]);
        let r = go("lti_lsim", &[integ.clone(), zero.clone(), t.clone(), v(&[3.0])]);
        assert_eq!(reals(&field(&r, "y")), vec![3.0; 4]);
        let r = go_kw("lti_lsim", &[integ.clone(), zero, t.clone()], &[("x0", Value::Num(4.0))]).unwrap();
        assert_eq!(reals(&field(&r, "y")), vec![4.0; 4]);
        assert!(msg("lti_lsim", &[integ.clone(), v(&[1.0, 2.0]), t.clone()]).contains("one row per time sample"));
        assert!(go_kw("lti_lsim", &[integ, t.clone(), t], &[("method", Value::Str("rk4".into()))]).unwrap_err().msg.contains("unknown method"));
        // discrete accumulator
        let acc = go("lti_tf", &[v(&[1.0]), v(&[1.0, -1.0]), Value::Num(1.0)]);
        let r = go("lti_step", &[acc.clone(), Value::Num(4.0)]);
        assert_eq!(reals(&field(&r, "y")), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        assert!(go_kw("lti_step", &[acc, Value::Num(4.0)], &[("n", Value::Num(9.0))]).unwrap_err().msg.contains("discrete"));
        // two inputs: pick one with input=
        let mimo = go("lti_ss", &[Value::Num(-1.0), v(&[1.0, 2.0]), Value::Num(1.0), v(&[0.0, 0.0])]);
        let r0 = go("lti_step", &[mimo.clone(), v(&[0.0, 50.0])]);
        let r1 = go_kw("lti_step", &[mimo.clone(), v(&[0.0, 50.0])], &[("input", Value::Num(1.0))]).unwrap();
        close(reals(&field(&r0, "y"))[1], 1.0, 1e-12);
        close(reals(&field(&r1, "y"))[1], 2.0, 1e-12);
        assert!(go_kw("lti_step", &[mimo, v(&[0.0, 1.0])], &[("input", Value::Num(2.0))]).unwrap_err().msg.contains("out of range"));
    }

    #[test]
    fn connection_builtins() {
        let g1 = go("lti_tf", &[v(&[1.0]), v(&[1.0, 1.0])]);
        let g2 = go("lti_tf", &[v(&[2.0]), v(&[1.0, 3.0])]);
        let s = go("lti_to_tf", &[go("lti_series", &[g1.clone(), g2.clone()])]);
        assert_eq!((reals(&field(&s, "num")), reals(&field(&s, "den"))), (vec![2.0], vec![1.0, 4.0, 3.0]));
        let p = go("lti_to_tf", &[go("lti_parallel", &[g1.clone(), g2.clone()])]);
        assert_eq!((reals(&field(&p, "num")), reals(&field(&p, "den"))), (vec![3.0, 5.0], vec![1.0, 4.0, 3.0]));
        // a number is a static gain
        let k = go("lti_to_tf", &[go("lti_series", &[Value::Num(5.0), g1.clone()])]);
        assert_eq!(reals(&field(&k, "num")), vec![5.0]);
        // feedback: unity, with a return path, positive sign
        let u = go("lti_to_tf", &[go("lti_feedback", &[g1.clone()])]);
        assert_eq!(reals(&field(&u, "den")), vec![1.0, 2.0]);
        let fb = go("lti_to_tf", &[go("lti_feedback", &[g1.clone(), g2.clone()])]);
        assert_eq!((reals(&field(&fb, "num")), reals(&field(&fb, "den"))), (vec![1.0, 3.0], vec![1.0, 4.0, 5.0]));
        let pos = go_kw("lti_feedback", &[g1.clone(), g2.clone()], &[("sign", Value::Num(1.0))]).unwrap();
        assert_eq!(reals(&field(&go("lti_to_tf", &[pos]), "den")), vec![1.0, 4.0, 1.0]);
        let pos = go("lti_feedback", &[g1.clone(), g2.clone(), Value::Num(1.0)]);
        assert_eq!(reals(&field(&go("lti_to_tf", &[pos]), "den")), vec![1.0, 4.0, 1.0]);
        assert!(go_kw("lti_feedback", &[g1.clone()], &[("sign", Value::Num(2.0))]).unwrap_err().msg.contains("sign must be"));
        let d = go("lti_tf", &[v(&[1.0]), v(&[1.0, -0.5]), Value::Num(0.1)]);
        assert!(msg("lti_series", &[g1.clone(), d]).contains("continuous system with a discrete"));
        assert!(msg("lti_parallel", &[g1, Value::Str("x".into())]).contains("the second argument must be an lti system"));
    }

    #[test]
    fn discretization_builtins() {
        let (tau, h) = (0.4f64, 0.1f64);
        let d = go("lti_c2d", &[lag(tau), Value::Num(h)]);
        close(reals(&field(&d, "A"))[0], (-h / tau).exp(), 2e-16);
        assert_eq!(reals(&field(&d, "dt")), vec![h]);
        // method by position or by name
        let t1 = go("lti_c2d", &[lag(tau), Value::Num(h), Value::Str("tustin".into())]);
        let t2 = go_kw("lti_c2d", &[lag(tau), Value::Num(h)], &[("method", Value::Str("tustin".into()))]).unwrap();
        let want = (1.0 - h / (2.0 * tau)) / (1.0 + h / (2.0 * tau));
        close(reals(&field(&t1, "A"))[0], want, 1e-15);
        close(reals(&field(&t2, "A"))[0], want, 1e-15);
        let eu = go("lti_c2d", &[lag(tau), Value::Num(h), Value::Str("euler".into())]);
        close(reals(&field(&eu, "A"))[0], 1.0 - h / tau, 1e-15);
        assert!(msg("lti_c2d", &[lag(tau), Value::Num(h), Value::Str("magic".into())]).contains("unknown method"));
        assert!(msg("lti_c2d", &[d.clone(), Value::Num(h)]).contains("already discrete"));
        // and back
        let c = go("lti_d2c", &[d.clone()]);
        close(reals(&field(&c, "A"))[0], -1.0 / tau, 1e-12);
        assert_eq!(reals(&field(&c, "dt")), vec![0.0]);
        let c = go("lti_d2c", &[t1, Value::Str("tustin".into())]);
        close(reals(&field(&c, "A"))[0], -1.0 / tau, 1e-12);
        assert!(msg("lti_d2c", &[d, Value::Str("euler".into())]).contains("only \"zoh\" and \"tustin\""));
        assert!(msg("lti_d2c", &[lag(tau)]).contains("already continuous"));
    }

    #[test]
    fn bode_and_margin_builtins() {
        let l = go("lti_tf", &[v(&[1.0]), v(&[1.0, 3.0, 2.0, 0.0])]);
        let b = go("lti_bode", &[l.clone(), v(&[1.0])]);
        // |L(j)| = 1/sqrt(10), phase = -90 - 45 - atan(1/2)
        close(reals(&field(&b, "mag_db"))[0], -10.0, 1e-12);
        close(reals(&field(&b, "phase_deg"))[0], -135.0 - 0.5f64.atan().to_degrees(), 1e-12);
        let auto = go("lti_bode", &[l.clone()]);
        let (w, ph) = (reals(&field(&auto, "w")), reals(&field(&auto, "phase_deg")));
        assert!(w.len() >= 200 && w.len() == ph.len());
        assert!(ph.iter().all(|p| *p < -89.0 && *p > -271.0), "unwrapped phase stays between -90 and -270");
        let m = go("lti_margin", &[l]);
        close(reals(&field(&m, "gm"))[0], 6.0, 1e-12);
        close(reals(&field(&m, "gm_db"))[0], 15.563025007672874, 1e-10);
        close(reals(&field(&m, "w_gm"))[0], 2f64.sqrt(), 1e-12);
        close(reals(&field(&m, "pm"))[0], 53.4, 0.1);
        assert!(matches!(field(&m, "stable"), Value::Bool(true)));
        let mimo = go("lti_ss", &[Value::Num(-1.0), v(&[1.0, 2.0]), v(&[3.0, 4.0]), Value::Num(0.0)]);
        assert!(msg("lti_margin", &[mimo.clone()]).contains("one input and one output"));
        assert!(msg("lti_bode", &[mimo]).contains("one input and one output"));
    }
}
