//! Script-facing builtins for scientific binary-format interop:
//! `read_npy`/`write_npy`/`read_npz`/`write_npz` (NumPy, see `npy.rs`) and
//! `h5read`/`h5info` (HDF5, see `hdf5.rs`).
//!
//! This file is only the seam between those plain format readers and the
//! interpreter's `Value`. Qu has no N-d array value and no integer element
//! types, so the mapping is: scalar -> number/bool/complex, 1-d -> vector,
//! 2-d -> matrix (complex likewise), every integer/float dtype -> `f64`.
//! Anything that does not fit (an array of rank 3 or more, an int64 beyond
//! 2^53) is an error that says so, never a silent reshape or rounding.

use crate::hdf5::{self, H5Value};
use crate::npy::{self, NpyArray, NpyData};
use crate::{Value, CMatrix, Complex64, Matrix};
use std::sync::Arc;

type R<T> = Result<T, String>;

fn path_arg<'a>(f: &str, args: &'a [Value], i: usize) -> R<&'a str> {
    match crate::arg_get(args, i) {
        Some(Value::Str(s)) => Ok(s.as_str()),
        Some(other) => Err(format!("{f}: argument {} must be a string, found {}", i + 1, other.type_name())),
        None => Err(format!("{f}: missing argument {}", i + 1)),
    }
}

fn read_file(f: &str, path: &str) -> R<Vec<u8>> {
    if let Ok(md) = std::fs::metadata(path) {
        if md.len() > npy::MAX_INPUT_FILE_BYTES {
            return Err(format!(
                "{f}: `{path}` is {} bytes; files over {} GiB are refused",
                md.len(),
                npy::MAX_INPUT_FILE_BYTES >> 30
            ));
        }
    }
    std::fs::read(path).map_err(|e| format!("{f}: could not read `{path}`: {e}"))
}

/// Row-major (C order) -> column-major, for a 2-d array.
fn c_to_f<T: Clone>(data: &[T], rows: usize, cols: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(data.len());
    for c in 0..cols {
        for r in 0..rows {
            out.push(data[r * cols + c].clone());
        }
    }
    out
}

fn real_value(shape: &[usize], data: Vec<f64>, what: &str) -> R<Value> {
    match shape {
        [] => Ok(Value::Num(data[0])),
        [_] => Ok(Value::Vec(Arc::new(data))),
        [r, c] => Ok(Value::Mat(Arc::new(Matrix::from_col_major(*r, *c, c_to_f(&data, *r, *c))))),
        _ => Err(format!(
            "{what} has shape {shape:?}; Qu has no arrays above two dimensions, so reshape or slice it to 2-d first"
        )),
    }
}

fn to_value(a: NpyArray) -> R<Value> {
    let shape = a.shape;
    match a.data {
        NpyData::Real(d) => real_value(&shape, d, "array"),
        NpyData::Bool(d) => {
            if shape.is_empty() {
                return Ok(Value::Bool(d[0]));
            }
            real_value(&shape, d.into_iter().map(|b| b as u8 as f64).collect(), "array")
        }
        NpyData::Complex(d) => {
            let z: Vec<Complex64> = d.into_iter().map(|(re, im)| Complex64::new(re, im)).collect();
            match shape.as_slice() {
                [] => Ok(Value::Complex(z[0])),
                [_] => Ok(Value::CVec(Arc::new(z))),
                [r, c] => Ok(Value::CMat(Arc::new(CMatrix::from_col_major(*r, *c, c_to_f(&z, *r, *c))))),
                _ => Err(format!(
                    "complex array has shape {shape:?}; Qu has no arrays above two dimensions"
                )),
            }
        }
    }
}

fn from_value(v: &Value, what: &str) -> R<NpyArray> {
    let cplx = |z: &Complex64| (z.re, z.im);
    Ok(match v {
        Value::Num(x) => NpyArray { shape: vec![], data: NpyData::Real(vec![*x]) },
        Value::Bool(b) => NpyArray { shape: vec![], data: NpyData::Bool(vec![*b]) },
        Value::Complex(z) => NpyArray { shape: vec![], data: NpyData::Complex(vec![cplx(z)]) },
        Value::Vec(xs) => NpyArray { shape: vec![xs.len()], data: NpyData::Real(xs.as_ref().clone()) },
        Value::Signal(xs, _, _) => {
            NpyArray { shape: vec![xs.len()], data: NpyData::Real(xs.as_ref().clone()) }
        }
        Value::CVec(zs) => NpyArray {
            shape: vec![zs.len()],
            data: NpyData::Complex(zs.iter().map(cplx).collect()),
        },
        Value::Mat(m) => {
            let (r, c) = (m.rows(), m.cols());
            let s = m.as_slice();
            let mut out = Vec::with_capacity(s.len());
            for i in 0..r {
                for j in 0..c {
                    out.push(s[j * r + i]);
                }
            }
            NpyArray { shape: vec![r, c], data: NpyData::Real(out) }
        }
        Value::CMat(m) => {
            let (r, c) = (m.rows(), m.cols());
            let s = m.as_slice();
            let mut out = Vec::with_capacity(s.len());
            for i in 0..r {
                for j in 0..c {
                    out.push(cplx(&s[j * r + i]));
                }
            }
            NpyArray { shape: vec![r, c], data: NpyData::Complex(out) }
        }
        other => {
            return Err(format!(
                "{what}: cannot store a {} as a NumPy array (numbers, booleans, vectors, matrices and their complex forms only)",
                other.type_name()
            ))
        }
    })
}

fn h5_to_value(v: H5Value) -> R<Value> {
    match v {
        H5Value::Real { shape, data } => real_value(&shape, data, "dataset"),
        H5Value::Text { shape, mut data } => match shape.as_slice() {
            [] => Ok(Value::Str(data.swap_remove(0))),
            [_] => Ok(Value::List(Arc::new(data.into_iter().map(Value::Str).collect()))),
            _ => Err(format!("string dataset has shape {shape:?}; only scalar and 1-d string datasets are supported")),
        },
    }
}

/// Entry point for the six builtins. `f` is the builtin's name.
pub(crate) fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "read_npy" => {
            let path = path_arg(f, args, 0)?;
            let bytes = read_file(f, path)?;
            let a = npy::read_npy(&bytes).map_err(|e| format!("{f}: `{path}`: {e}"))?;
            to_value(a).map_err(|e| format!("{f}: `{path}`: {e}"))
        }
        "read_npz" => {
            let path = path_arg(f, args, 0)?;
            let bytes = read_file(f, path)?;
            let arrays = npy::read_npz(&bytes).map_err(|e| format!("{f}: `{path}`: {e}"))?;
            let mut fields = Vec::with_capacity(arrays.len());
            for (k, a) in arrays {
                let v = to_value(a).map_err(|e| format!("{f}: `{path}`: array `{k}`: {e}"))?;
                fields.push((k, v));
            }
            Ok(Value::Record(Arc::new(fields)))
        }
        "write_npy" => {
            let path = path_arg(f, args, 0)?;
            let v = crate::arg_get(args, 1).ok_or("write_npy(path, x) needs the value to write")?;
            let bytes = npy::write_npy(&from_value(v, f)?);
            std::fs::write(path, bytes).map_err(|e| format!("{f}: could not write `{path}`: {e}"))?;
            Ok(Value::Nothing)
        }
        "write_npz" => {
            let path = path_arg(f, args, 0)?;
            let Some(Value::Record(fields)) = crate::arg_get(args, 1) else {
                return Err("write_npz(path, record) needs a record of arrays as its second argument".into());
            };
            let mut arrays = Vec::with_capacity(fields.len());
            for (k, v) in fields.iter() {
                if k.is_empty() || k.contains('/') || k.contains('\\') || k.contains('\0') {
                    return Err(format!("write_npz: field name `{k}` cannot be used as an archive member name"));
                }
                arrays.push((k.clone(), from_value(v, &format!("write_npz: field `{k}`"))?));
            }
            let bytes = npy::write_npz(&arrays).map_err(|e| format!("{f}: {e}"))?;
            std::fs::write(path, bytes).map_err(|e| format!("{f}: could not write `{path}`: {e}"))?;
            Ok(Value::Nothing)
        }
        "h5read" => {
            let path = path_arg(f, args, 0)?;
            let ds = path_arg(f, args, 1)?;
            let bytes = read_file(f, path)?;
            let v = hdf5::h5read(&bytes, ds).map_err(|e| format!("{f}: `{path}`: {e}"))?;
            h5_to_value(v).map_err(|e| format!("{f}: `{path}`: `{ds}`: {e}"))
        }
        "h5info" => {
            let path = path_arg(f, args, 0)?;
            let bytes = read_file(f, path)?;
            let entries = hdf5::h5info(&bytes).map_err(|e| format!("{f}: `{path}`: {e}"))?;
            let rows: Vec<Value> = entries
                .into_iter()
                .map(|e| {
                    Value::Record(Arc::new(vec![
                        ("path".to_string(), Value::Str(e.path)),
                        ("kind".to_string(), Value::Str(e.kind.to_string())),
                        ("dtype".to_string(), Value::Str(e.dtype)),
                        (
                            "shape".to_string(),
                            Value::Vec(Arc::new(e.shape.iter().map(|&d| d as f64).collect())),
                        ),
                    ]))
                })
                .collect();
            Ok(Value::List(Arc::new(rows)))
        }
        _ => Err(format!("interop_ops: unknown builtin {f}")),
    }
}
