//! Trace utilities for back-to-back comparison (model vs. model, model vs.
//! host-compiled C, a run vs. a recorded run): `trace_diff`, `trace_save`,
//! `trace_load`. Added in v0.4.14.
//!
//! A *trace* is a set of named signals of equal length: a record of vectors
//! or a table of numeric columns. `trace_diff` also takes a bare vector or a
//! matrix whose columns are the signals.
//!
//! What is reused and what is new:
//! * The file formats are not new. `.csv` goes through `Table::to_csv` /
//!   `Table::from_csv` (what `write_csv` / `read_csv` use; numbers are written
//!   in the shortest form that reads back to the same `f64`), `.npz` through
//!   `npy::write_npz` / `npy::read_npz` (what `write_npz` / `read_npz` use;
//!   raw little-endian `f64`).
//! * New is the trace contract on top: one call for a record OR a table, a
//!   check that every signal is a real vector of the same length with a name
//!   both formats can hold, the same record of plain vectors back from either
//!   format in the saved order, and a refusal to load something that is not a
//!   trace (a text column, a matrix member).
//!
//! Round trip: every finite value, both zeros and both infinities come back
//! bit for bit from either format. A NaN comes back as a NaN; only `.npz`
//! also keeps its sign and payload bits.

use crate::npy::{self, NpyArray, NpyData};
use crate::table::{Column, Table};
use crate::{arg_get, e, style_entry, EvalError, Value, R};
use std::sync::Arc;

pub const NAMES: &[&str] = &["trace_diff", "trace_load", "trace_save"];

// ====================================================================
// Numerics (no interpreter types)
// ====================================================================

/// Result of comparing two equally long sample sequences.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffOut {
    /// Largest `|a - b|` (infinite if a NaN or an infinity faces a number).
    pub max_abs: f64,
    /// Largest `|a - b| / max(|a|, |b|)`.
    pub max_rel: f64,
    /// Where `max_abs` is, `None` if the two are identical.
    pub worst: Option<usize>,
    /// First index outside the tolerance.
    pub first_bad: Option<usize>,
    pub n: usize,
    pub n_bad: usize,
}

/// Absolute and relative error of one pair, and whether it is inside
/// `atol + rtol * max(|a|, |b|)`.
///
/// Equal values (including `0` vs `-0` and equal infinities) and a NaN facing
/// a NaN are an exact match. A NaN facing a number, or an infinity facing
/// anything else, is an infinite error.
fn pair_error(a: f64, b: f64, rtol: f64, atol: f64) -> (f64, f64, bool) {
    if a == b || (a.is_nan() && b.is_nan()) {
        return (0.0, 0.0, true);
    }
    if !a.is_finite() || !b.is_finite() {
        return (f64::INFINITY, f64::INFINITY, false);
    }
    let d = (a - b).abs();
    let scale = a.abs().max(b.abs());
    (d, d / scale, d <= atol + rtol * scale)
}

/// Compares `a` and `b` element by element. The lengths must already agree.
pub(crate) fn diff_slices(a: &[f64], b: &[f64], rtol: f64, atol: f64) -> DiffOut {
    let mut o = DiffOut { max_abs: 0.0, max_rel: 0.0, worst: None, first_bad: None, n: a.len(), n_bad: 0 };
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        let (d, r, ok) = pair_error(x, y, rtol, atol);
        if d > o.max_abs {
            o.max_abs = d;
            o.worst = Some(i);
        }
        if r > o.max_rel {
            o.max_rel = r;
        }
        if !ok {
            o.n_bad += 1;
            if o.first_bad.is_none() {
                o.first_bad = Some(i);
            }
        }
    }
    o
}

/// Checks a set of named signals and returns their common length.
pub(crate) fn check_trace(who: &str, signals: &[(String, Vec<f64>)]) -> Result<usize, String> {
    if signals.is_empty() {
        return Err(format!("{who}: the trace has no signals"));
    }
    let n = signals[0].1.len();
    for (i, (name, v)) in signals.iter().enumerate() {
        if name.is_empty() || name.trim() != name {
            return Err(format!("{who}: signal name `{name}` is empty or has spaces around it"));
        }
        if name.chars().any(|c| matches!(c, ',' | '"' | '\n' | '\r' | '/' | '\\' | '\0')) {
            return Err(format!(
                "{who}: signal name `{name}` contains a comma, quote, slash or line break, which a .csv header or an .npz member name cannot hold"
            ));
        }
        if signals[..i].iter().any(|(other, _)| other == name) {
            return Err(format!("{who}: signal name `{name}` appears twice"));
        }
        if v.len() != n {
            return Err(format!(
                "{who}: signal `{name}` has {} samples but `{}` has {n}; every signal of a trace must have the same length",
                v.len(),
                signals[0].0
            ));
        }
    }
    Ok(n)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Format {
    Csv,
    Npz,
}

pub(crate) fn format_of(who: &str, path: &str) -> Result<Format, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        Ok(Format::Csv)
    } else if lower.ends_with(".npz") {
        Ok(Format::Npz)
    } else {
        Err(format!("{who}: `{path}` must end in .csv (text) or .npz (binary); the extension selects the format"))
    }
}

/// The bytes of a trace file. Deterministic: the same signals give the same
/// bytes (no timestamps; signals in the given order).
pub(crate) fn encode(who: &str, signals: &[(String, Vec<f64>)], format: Format) -> Result<Vec<u8>, String> {
    check_trace(who, signals)?;
    match format {
        Format::Csv => {
            let cols = signals.iter().map(|(k, v)| (k.clone(), Column::Num(v.clone()))).collect();
            Ok(Table::from_columns(cols).map_err(|m| format!("{who}: {m}"))?.to_csv().into_bytes())
        }
        Format::Npz => {
            let arrays: Vec<(String, NpyArray)> = signals
                .iter()
                .map(|(k, v)| (k.clone(), NpyArray { shape: vec![v.len()], data: NpyData::Real(v.clone()) }))
                .collect();
            npy::write_npz(&arrays).map_err(|m| format!("{who}: {m}"))
        }
    }
}

pub(crate) fn decode(who: &str, path: &str, bytes: &[u8], format: Format) -> Result<Vec<(String, Vec<f64>)>, String> {
    let signals: Vec<(String, Vec<f64>)> = match format {
        Format::Csv => {
            let text = std::str::from_utf8(bytes).map_err(|_| format!("{who}: `{path}` is not UTF-8 text"))?;
            let t = Table::from_csv(text).map_err(|m| format!("{who}: `{path}`: {}", m.trim_start_matches("read_csv: ")))?;
            let mut out = Vec::with_capacity(t.ncols());
            for name in t.column_names() {
                match t.col(name) {
                    Some(Column::Num(v)) => out.push((name.to_string(), v.clone())),
                    _ => return Err(format!("{who}: `{path}`: column `{name}` is not numeric, so this is not a trace")),
                }
            }
            out
        }
        Format::Npz => {
            let arrays = npy::read_npz(bytes).map_err(|m| format!("{who}: `{path}`: {m}"))?;
            let mut out = Vec::with_capacity(arrays.len());
            for (name, a) in arrays {
                match (a.shape.len(), a.data) {
                    (1, NpyData::Real(v)) => out.push((name, v)),
                    (rank, _) => {
                        return Err(format!(
                            "{who}: `{path}`: member `{name}` is not a real 1-d array (rank {rank}), so this is not a trace; read_npz reads general archives"
                        ))
                    }
                }
            }
            out
        }
    };
    check_trace(who, &signals).map_err(|m| format!("{m} (in `{path}`)"))?;
    Ok(signals)
}

// ====================================================================
// Interpreter layer
// ====================================================================

fn fail<T>(msg: String) -> R<T> {
    Err(EvalError { msg })
}

/// What `trace_diff` was handed.
enum Traces {
    Flat(Vec<f64>),
    /// Column-major columns of a matrix with more than one row and column.
    Cols(Vec<Vec<f64>>, usize),
    Named(Vec<(String, Vec<f64>)>),
}

fn real_vector(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Num(x) => Some(vec![*x]),
        Value::Vec(x) => Some(x.to_vec()),
        Value::Signal(x, _, _) => Some(x.to_vec()),
        Value::Mat(m) if m.rows() == 1 || m.cols() == 1 => Some(m.as_slice().to_vec()),
        _ => None,
    }
}

/// The named signals of a record or a table.
fn named_signals(who: &str, what: &str, v: &Value) -> R<Vec<(String, Vec<f64>)>> {
    match v {
        Value::Record(fields) => {
            let mut out = Vec::with_capacity(fields.len());
            for (k, f) in fields.iter() {
                match real_vector(f) {
                    Some(x) => out.push((k.clone(), x)),
                    None => {
                        return fail(format!(
                            "{who}: {what}: field `{k}` is a {}, but every signal of a trace must be a real vector",
                            f.type_name()
                        ))
                    }
                }
            }
            Ok(out)
        }
        Value::Table(t) => {
            let mut out = Vec::with_capacity(t.ncols());
            for name in t.column_names() {
                match t.col(name) {
                    Some(Column::Num(x)) => out.push((name.to_string(), x.clone())),
                    _ => return fail(format!("{who}: {what}: column `{name}` is text, but every signal of a trace must be numeric")),
                }
            }
            Ok(out)
        }
        other => fail(format!("{who}: {what} must be a record of vectors or a table, found {}", other.type_name())),
    }
}

fn traces(who: &str, what: &str, v: &Value) -> R<Traces> {
    if let Value::Mat(m) = v {
        if m.rows() > 1 && m.cols() > 1 {
            let r = m.rows();
            return Ok(Traces::Cols(m.as_slice().chunks(r).map(|c| c.to_vec()).collect(), r));
        }
    }
    if let Some(x) = real_vector(v) {
        return Ok(Traces::Flat(x));
    }
    match v {
        Value::Record(_) | Value::Table(_) => Ok(Traces::Named(named_signals(who, what, v)?)),
        other => fail(format!(
            "{who}: {what} must be a vector, a matrix (one signal per column), a record of vectors or a table, found {}",
            other.type_name()
        )),
    }
}

fn tolerance(who: &str, args: &[Value], idx: usize, style: &[(String, Value)], key: &str, default: f64) -> R<f64> {
    let v = match style_entry(style, key) {
        Some((_, v)) => Some(v),
        None => arg_get(args, idx),
    };
    let Some(v) = v else { return Ok(default) };
    match v.as_num() {
        Ok(x) if x >= 0.0 && x.is_finite() => Ok(x),
        Ok(x) => fail(format!("{who}: {key} must be a finite number >= 0, found {x}")),
        Err(_) => fail(format!("{who}: {key} must be a number, found {}", v.type_name())),
    }
}

fn idx(v: Option<usize>) -> Value {
    Value::Num(v.map_or(-1.0, |i| i as f64))
}

/// Folds per-signal results into one: totals, the worst signal and the
/// signal holding the earliest bad sample (ties: the first signal).
fn fold(parts: &[DiffOut]) -> (DiffOut, Option<usize>, Option<usize>) {
    let mut all = DiffOut { max_abs: 0.0, max_rel: 0.0, worst: None, first_bad: None, n: 0, n_bad: 0 };
    let (mut worst_sig, mut bad_sig) = (None, None);
    for (k, p) in parts.iter().enumerate() {
        all.n += p.n;
        all.n_bad += p.n_bad;
        if p.max_abs > all.max_abs {
            all.max_abs = p.max_abs;
            all.worst = p.worst;
            worst_sig = Some(k);
        }
        if p.max_rel > all.max_rel {
            all.max_rel = p.max_rel;
        }
        if let Some(fb) = p.first_bad {
            if all.first_bad.map_or(true, |cur| fb < cur) {
                all.first_bad = Some(fb);
                bad_sig = Some(k);
            }
        }
    }
    (all, worst_sig, bad_sig)
}

fn diff_record(o: &DiffOut) -> Vec<(String, Value)> {
    vec![
        ("ok".to_string(), Value::Bool(o.n_bad == 0)),
        ("max_abs".to_string(), Value::Num(o.max_abs)),
        ("max_rel".to_string(), Value::Num(o.max_rel)),
        ("worst_index".to_string(), idx(o.worst)),
        ("first_bad_index".to_string(), idx(o.first_bad)),
        ("n".to_string(), Value::Num(o.n as f64)),
        ("n_bad".to_string(), Value::Num(o.n_bad as f64)),
    ]
}

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "trace_diff" => {
            let sig = "trace_diff(a, b, [rtol=1e-9], [atol=1e-12])";
            let (Some(av), Some(bv)) = (arg_get(args, 0), arg_get(args, 1)) else {
                return fail(format!("{sig} needs two traces to compare"));
            };
            let rtol = tolerance(f, args, 2, style, "rtol", 1e-9)?;
            let atol = tolerance(f, args, 3, style, "atol", 1e-12)?;
            match (traces(f, "a", av)?, traces(f, "b", bv)?) {
                (Traces::Flat(a), Traces::Flat(b)) => {
                    if a.len() != b.len() {
                        return fail(format!("{f}: a has {} samples but b has {}; the traces must have the same length", a.len(), b.len()));
                    }
                    Ok(Value::Record(Arc::new(diff_record(&diff_slices(&a, &b, rtol, atol)))))
                }
                (Traces::Cols(a, ra), Traces::Cols(b, rb)) => {
                    if ra != rb || a.len() != b.len() {
                        return fail(format!(
                            "{f}: a is {ra}x{} but b is {rb}x{} (rows are samples, columns are signals); the shapes must match",
                            a.len(),
                            b.len()
                        ));
                    }
                    let parts: Vec<DiffOut> = a.iter().zip(&b).map(|(x, y)| diff_slices(x, y, rtol, atol)).collect();
                    let (all, worst, bad) = fold(&parts);
                    let mut rec = diff_record(&all);
                    rec.push(("worst_col".to_string(), idx(worst)));
                    rec.push(("first_bad_col".to_string(), idx(bad)));
                    Ok(Value::Record(Arc::new(rec)))
                }
                (Traces::Named(a), Traces::Named(b)) => {
                    let missing_in = |x: &[(String, Vec<f64>)], y: &[(String, Vec<f64>)]| -> Vec<String> {
                        x.iter().filter(|(k, _)| !y.iter().any(|(j, _)| j == k)).map(|(k, _)| format!("`{k}`")).collect()
                    };
                    let (only_a, only_b) = (missing_in(&a, &b), missing_in(&b, &a));
                    if !only_a.is_empty() || !only_b.is_empty() {
                        let mut why = Vec::new();
                        if !only_a.is_empty() {
                            why.push(format!("only in a: {}", only_a.join(", ")));
                        }
                        if !only_b.is_empty() {
                            why.push(format!("only in b: {}", only_b.join(", ")));
                        }
                        return fail(format!("{f}: the two traces do not have the same signals ({})", why.join("; ")));
                    }
                    let mut parts = Vec::with_capacity(a.len());
                    for (name, x) in &a {
                        let y = &b.iter().find(|(k, _)| k == name).unwrap().1;
                        if x.len() != y.len() {
                            return fail(format!(
                                "{f}: signal `{name}` has {} samples in a but {} in b; the traces must have the same length",
                                x.len(),
                                y.len()
                            ));
                        }
                        parts.push(diff_slices(x, y, rtol, atol));
                    }
                    let (all, worst, bad) = fold(&parts);
                    let name_of = |k: Option<usize>| Value::Str(k.map_or(String::new(), |k| a[k].0.clone()));
                    let mut rec = diff_record(&all);
                    rec.push(("worst_signal".to_string(), name_of(worst)));
                    rec.push(("first_bad_signal".to_string(), name_of(bad)));
                    Ok(Value::Record(Arc::new(rec)))
                }
                _ => fail(format!(
                    "{f}: a is a {} and b is a {}; compare two vectors, two matrices of the same shape, or two traces (records or tables)",
                    av.type_name(),
                    bv.type_name()
                )),
            }
        }
        "trace_save" => {
            let sig = "trace_save(path, trace)";
            let path = match arg_get(args, 0) {
                Some(Value::Str(s)) => s.clone(),
                Some(other) => return fail(format!("{sig}: path must be a string, found {}", other.type_name())),
                None => return fail(format!("{sig} needs a path and a record or table of signals")),
            };
            let Some(v) = arg_get(args, 1) else {
                return fail(format!("{sig} needs the record or table of signals to save"));
            };
            let format = format_of(f, &path).map_err(|msg| EvalError { msg })?;
            let signals = named_signals(f, "trace", v)?;
            let bytes = encode(f, &signals, format).map_err(|msg| EvalError { msg })?;
            std::fs::write(&path, bytes).map_err(|err| EvalError { msg: format!("{f}: could not write `{path}`: {err}") })?;
            Ok(Value::Nothing)
        }
        "trace_load" => {
            let path = match arg_get(args, 0) {
                Some(Value::Str(s)) => s.clone(),
                Some(other) => return fail(format!("trace_load(path): path must be a string, found {}", other.type_name())),
                None => return fail("trace_load(path) needs the path of a trace written by trace_save".to_string()),
            };
            let format = format_of(f, &path).map_err(|msg| EvalError { msg })?;
            let bytes = std::fs::read(&path).map_err(|err| EvalError { msg: format!("{f}: could not read `{path}`: {err}") })?;
            let signals = decode(f, &path, &bytes, format).map_err(|msg| EvalError { msg })?;
            Ok(Value::Record(Arc::new(signals.into_iter().map(|(k, v)| (k, Value::Vec(Arc::new(v)))).collect())))
        }
        _ => e(format!("{f}: not a trace builtin")),
    }
}

// ====================================================================
// Unit tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Values chosen to break a lossy text format: subnormals, the extremes,
    /// both zeros, values needing 17 digits, both infinities.
    fn nasty() -> Vec<f64> {
        let mut v = vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.1,
            0.1 + 0.2,
            1.0 / 3.0,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            5e-324,
            -5e-324,
            f64::EPSILON,
            1e15,
            1e15 + 0.5,
            123456789012345680.0,
            9007199254740993.0,
            1e-5,
            9.999999999999999e-5,
            1e16,
            2.5e-310,
            f64::INFINITY,
            f64::NEG_INFINITY,
            std::f64::consts::PI,
            -std::f64::consts::E * 1e-200,
        ];
        // a deterministic spread of arbitrary bit patterns (finite ones only)
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        while v.len() < 600 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let x = f64::from_bits(s);
            if x.is_finite() {
                v.push(x);
            }
        }
        v
    }

    fn bits(v: &[f64]) -> Vec<u64> {
        v.iter().map(|x| x.to_bits()).collect()
    }

    #[test]
    fn both_formats_round_trip_every_bit() {
        let a = nasty();
        let b: Vec<f64> = a.iter().rev().cloned().collect();
        let trace = vec![("t".to_string(), a.clone()), ("u_ref".to_string(), b.clone())];
        for (fmt, path) in [(Format::Csv, "x.csv"), (Format::Npz, "x.npz")] {
            let bytes = encode("trace_save", &trace, fmt).unwrap();
            let back = decode("trace_load", path, &bytes, fmt).unwrap();
            assert_eq!(back.len(), 2, "{fmt:?}");
            assert_eq!((back[0].0.as_str(), back[1].0.as_str()), ("t", "u_ref"), "{fmt:?}: order kept");
            assert_eq!(bits(&back[0].1), bits(&a), "{fmt:?}");
            assert_eq!(bits(&back[1].1), bits(&b), "{fmt:?}");
            // deterministic: encoding again gives the same bytes
            assert_eq!(encode("trace_save", &back, fmt).unwrap(), bytes, "{fmt:?}");
        }
    }

    #[test]
    fn nan_survives_and_npz_keeps_its_payload() {
        let odd = f64::from_bits(0xFFF8_0000_0000_1234);
        let trace = vec![("x".to_string(), vec![1.0, f64::NAN, odd])];
        let csv = decode("trace_load", "x.csv", &encode("trace_save", &trace, Format::Csv).unwrap(), Format::Csv).unwrap();
        assert!(csv[0].1[1].is_nan() && csv[0].1[2].is_nan() && csv[0].1[0] == 1.0);
        let npz = decode("trace_load", "x.npz", &encode("trace_save", &trace, Format::Npz).unwrap(), Format::Npz).unwrap();
        assert_eq!(bits(&npz[0].1), bits(&trace[0].1));
    }

    #[test]
    fn an_empty_trace_and_a_single_sample_round_trip() {
        for fmt in [Format::Csv, Format::Npz] {
            for n in [0usize, 1] {
                let trace = vec![("a".to_string(), vec![0.25; n]), ("b".to_string(), vec![-7.0; n])];
                let back = decode("trace_load", "p", &encode("trace_save", &trace, fmt).unwrap(), fmt).unwrap();
                assert_eq!(back, trace, "{fmt:?} n={n}");
            }
        }
    }

    #[test]
    fn a_trace_must_be_well_formed() {
        let bad = |t: Vec<(&str, Vec<f64>)>| {
            let t: Vec<(String, Vec<f64>)> = t.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            check_trace("trace_save", &t).unwrap_err()
        };
        let m = bad(vec![("a", vec![1.0, 2.0]), ("b", vec![1.0])]);
        assert!(m.contains("`b` has 1 samples") && m.contains("`a` has 2"), "{m}");
        assert!(bad(vec![("a,b", vec![1.0])]).contains("comma"));
        assert!(bad(vec![("a", vec![1.0]), ("a", vec![1.0])]).contains("twice"));
        assert!(bad(vec![(" a", vec![1.0])]).contains("spaces"));
        assert!(bad(vec![]).contains("no signals"));
        assert!(format_of("trace_save", "run.txt").unwrap_err().contains(".csv"));
        assert_eq!(format_of("trace_save", "RUN.CSV").unwrap(), Format::Csv);
    }

    #[test]
    fn loading_refuses_what_is_not_a_trace() {
        let m = decode("trace_load", "x.csv", b"a,b\n1,hello\n", Format::Csv).unwrap_err();
        assert!(m.contains("`b` is not numeric"), "{m}");
        let arrays = vec![("m".to_string(), NpyArray { shape: vec![2, 2], data: NpyData::Real(vec![1.0; 4]) })];
        let m = decode("trace_load", "x.npz", &npy::write_npz(&arrays).unwrap(), Format::Npz).unwrap_err();
        assert!(m.contains("`m` is not a real 1-d array"), "{m}");
    }

    #[test]
    fn diff_of_identical_traces_is_clean() {
        let a = nasty();
        let o = diff_slices(&a, &a, 0.0, 0.0);
        assert_eq!(o, DiffOut { max_abs: 0.0, max_rel: 0.0, worst: None, first_bad: None, n: a.len(), n_bad: 0 });
        // NaN against NaN and 0 against -0 are matches too
        let o = diff_slices(&[f64::NAN, 0.0], &[f64::NAN, -0.0], 0.0, 0.0);
        assert_eq!((o.n_bad, o.max_abs), (0, 0.0));
    }

    #[test]
    fn diff_reports_the_worst_and_the_first_bad_sample_separately() {
        let a = [1.0, 2.0, 3.0, 4.0, 1000.0];
        let b = [1.0, 2.0 + 1e-6, 3.0, 4.0 + 1e-3, 1000.0 + 5e-4];
        let o = diff_slices(&a, &b, 1e-9, 1e-12);
        assert_eq!(o.first_bad, Some(1));
        assert_eq!(o.worst, Some(3));
        assert!((o.max_abs - 1e-3).abs() < 1e-12, "{}", o.max_abs);
        // the largest relative error is at index 3 too: 1e-3 / 4.001
        assert!((o.max_rel - 1e-3 / (4.0 + 1e-3)).abs() < 1e-12, "{}", o.max_rel);
        assert_eq!((o.n, o.n_bad), (5, 3));
    }

    #[test]
    fn tolerance_is_atol_plus_rtol_times_the_larger_magnitude() {
        // on a value of 1000, rtol 1e-9 allows 1e-6: 5e-7 passes, 1e-5 needs atol
        assert_eq!(diff_slices(&[1000.0], &[1000.0000005], 1e-9, 1e-12).n_bad, 0);
        assert_eq!(diff_slices(&[1000.0], &[1000.00001], 1e-9, 1e-12).n_bad, 1);
        assert_eq!(diff_slices(&[1000.0], &[1000.00001], 1e-9, 1e-4).n_bad, 0);
        // symmetric in its arguments
        let (x, y) = ([1.0, 5.0, -3.0], [1.1, 5.0, -3.2]);
        assert_eq!(diff_slices(&x, &y, 0.05, 0.0), diff_slices(&y, &x, 0.05, 0.0));
        // near zero only atol can pass
        assert_eq!(diff_slices(&[0.0], &[1e-13], 1e-9, 1e-12).n_bad, 0);
        assert_eq!(diff_slices(&[0.0], &[1e-11], 1e-9, 1e-12).n_bad, 1);
    }

    #[test]
    fn a_nan_or_infinity_facing_a_number_is_an_infinite_error() {
        let o = diff_slices(&[1.0, f64::NAN, 3.0], &[1.0, 2.0, 3.0], 1.0, 1e300);
        assert_eq!((o.n_bad, o.first_bad, o.worst), (1, Some(1), Some(1)));
        assert!(o.max_abs.is_infinite() && o.max_rel.is_infinite());
        let o = diff_slices(&[f64::INFINITY], &[f64::NEG_INFINITY], 1.0, 1.0);
        assert_eq!(o.n_bad, 1);
        assert_eq!(diff_slices(&[f64::INFINITY], &[f64::INFINITY], 0.0, 0.0).n_bad, 0);
    }

    #[test]
    fn fold_picks_the_worst_signal_and_the_earliest_bad_sample() {
        let p0 = diff_slices(&[0.0, 0.0, 0.0], &[0.0, 0.0, 0.5], 0.0, 0.0);
        let p1 = diff_slices(&[0.0, 0.0, 0.0], &[0.0, 0.1, 0.0], 0.0, 0.0);
        let (all, worst, bad) = fold(&[p0, p1]);
        assert_eq!((worst, bad), (Some(0), Some(1)));
        assert_eq!((all.worst, all.first_bad, all.n, all.n_bad), (Some(2), Some(1), 6, 2));
        assert_eq!(all.max_abs, 0.5);
    }
}
