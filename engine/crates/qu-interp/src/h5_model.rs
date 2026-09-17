//! `save_model(net, path)` / `load_model(path)` (2026-08-26) —
//! docs/qu-language-spec.md §38.4's own example (`save_model(net,
//! "mlp.qm")` / `net2 = load_model("mlp.qm")`) already named this API;
//! nothing implemented it until now. Persists/restores a `kind="sequential"`
//! `Value::Model` (see `model.rs`'s own doc comment for the general
//! `ModelHandle` shape, and `lib.rs`'s `"sequential"`/`"compile"` builtin
//! arms for exactly which fields a Sequential model carries: `"layers"`
//! (the ordered layer-spec `Record` list — `{kind:"dense", in_dim, out_dim,
//! activation}` or `{kind:"dropout", rate}`), `"params"` (the parallel
//! per-layer weight `Record` list — `{w, b}` for `"dense"`, empty for
//! `"dropout"`), and optionally `"optimizer"`/`"loss"` if the model came
//! from `compile(...)`).
//!
//! This whole module is gated behind the `h5-models` feature (see
//! `Cargo.toml`'s own comment for the full reasoning and the exact
//! build-environment note); `lib.rs` only declares `mod h5_model;` under
//! the same `#[cfg(feature = "h5-models")]`, so nothing here needs its own
//! per-item `cfg` — the whole file simply isn't compiled into a default
//! build.
//!
//! ## On-disk layout — Keras-*dataset-naming*-compatible, not Keras-*file*-compatible
//!
//! Weighted (`"dense"`) layers are written using the SAME group/dataset
//! naming and shape convention Keras' own legacy `model.save("x.h5")`
//! format uses for a `Dense` layer:
//!
//! ```text
//! /model_weights/<name>/<name>/kernel:0   (shape (in_dim, out_dim), f64)
//! /model_weights/<name>/<name>/bias:0     (shape (out_dim,), f64)
//! /model_weights  [attr] layer_names = [<name>, ...]   (weighted layers only, in order)
//! ```
//!
//! (`<name>` is `dense_<i>`/`dropout_<i>`, `i` counted independently per
//! kind in layer order — `dense_0`, `dropout_0`, `dense_1`, ... — the same
//! "auto-numbered by kind" convention Keras itself uses.) That naming and
//! shape match is the real interop win: a saved `.h5` is inspectable and
//! its weights are readable with `h5py`/`h5dump`/any generic HDF5 tool,
//! verified in this change by an actual `h5py` cross-check (see
//! `IMPL.md`'s dated entry), not just by Qu reading its own file back.
//!
//! What is DELIBERATELY NOT Keras-compatible: real Keras also writes a
//! root `model_config` attr (a full Keras-internal JSON *graph*, layer
//! classes and all) and, if the model was compiled, `training_config` +
//! `optimizer_weights`. Reproducing that JSON schema exactly is out of
//! scope here, so the full architecture — including unweighted `"dropout"`
//! layers, which have no equivalent slot in `model_weights` — plus the
//! optional optimizer/loss are stored under Qu's OWN root attribute names
//! instead (`qu_format`, `qu_architecture`, deliberately NOT
//! `model_config`, so nothing here is mistaken for, or collides with, a
//! real Keras `model_config`/`training_config` by any other tool that
//! opens this file):
//!
//! ```text
//! [attr] qu_format = "qu-sequential-h5-v1"
//! [attr] qu_architecture = <JSON, see `architecture_json` below>
//! ```
//!
//! `qu_architecture`'s JSON shape:
//!
//! ```json
//! {
//!   "format": "qu-sequential-h5-v1",
//!   "layers": [
//!     {"kind": "dense", "name": "dense_0", "in_dim": 20, "out_dim": 64, "activation": "relu"},
//!     {"kind": "dropout", "name": "dropout_0", "rate": 0.5},
//!     {"kind": "dense", "name": "dense_1", "in_dim": 64, "out_dim": 4, "activation": "none"}
//!   ],
//!   "optimizer": {"kind": "adam", "lr": 0.001, "beta1": 0.9, "beta2": 0.999, "eps": 1e-8},
//!   "loss": "cross_entropy"
//! }
//! ```
//!
//! (`"optimizer"`/`"loss"` only appear if the saved model had them, i.e.
//! it came from `compile(...)` rather than the bare `sequential(...)`
//! builtin.)
//!
//! Net effect: a real win over a fully custom binary format (any HDF5
//! tool can inspect the weights right now) but NOT literally loadable via
//! Python's `keras.models.load_model(...)` — that would need reproducing
//! Keras' own `model_config`/`training_config` JSON schema exactly, out of
//! scope for this change.
//!
//! `Matrix` (`qu_core::matrix::Matrix`) is stored column-major internally;
//! HDF5 datasets are conceptually row-major (C order), same as Keras'
//! numpy-backed `kernel:0`. `to_row_major`/`from_row_major` below convert
//! between the two explicitly (verified against both a Qu round-trip and
//! an `h5py` read — see `IMPL.md`) rather than relying on any
//! implicit/undocumented ordering.

use crate::{ModelHandle, Value};
use hdf5_metno as hdf5;
use hdf5_metno::types::VarLenUnicode;
use qu_core::matrix::Matrix;
use std::str::FromStr;
use std::sync::Arc;

const FORMAT: &str = "qu-sequential-h5-v1";

/// `Matrix::as_slice()` is column-major (`data[c * rows + r]`); HDF5/Keras
/// datasets are row-major (`data[r * cols + c]`). Explicit element-by-
/// element transpose, not a reinterpret — see this module's own doc
/// comment.
fn to_row_major(m: &Matrix) -> Vec<f64> {
    let (rows, cols) = m.shape();
    let src = m.as_slice();
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[r * cols + c] = src[c * rows + r];
        }
    }
    out
}

/// Inverse of [`to_row_major`]: a flat row-major buffer (as read back from
/// an HDF5 dataset) plus its declared `(rows, cols)` shape, into a
/// column-major `Matrix`.
fn from_row_major(rows: usize, cols: usize, row_major: &[f64]) -> Matrix {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = row_major[r * cols + c];
        }
    }
    Matrix::from_col_major(rows, cols, out)
}

fn record_field<'a>(fields: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn record_str(fields: &[(String, Value)], name: &str) -> Option<String> {
    match record_field(fields, name) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn record_num(fields: &[(String, Value)], name: &str) -> Option<f64> {
    match record_field(fields, name) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    }
}

fn write_str_attr(loc: &hdf5::Group, name: &str, value: &str) -> Result<(), String> {
    let v = VarLenUnicode::from_str(value)
        .map_err(|err| format!("save_model: attribute `{name}` isn't valid text: {err}"))?;
    loc.new_attr::<VarLenUnicode>()
        .create(name)
        .and_then(|attr| attr.write_scalar(&v))
        .map_err(|err| format!("save_model: writing attribute `{name}`: {err}"))
}

fn read_str_attr(loc: &hdf5::Group, name: &str) -> Result<String, String> {
    let attr = loc
        .attr(name)
        .map_err(|err| format!("load_model: missing attribute `{name}`: {err}"))?;
    let v: VarLenUnicode = attr
        .read_scalar()
        .map_err(|err| format!("load_model: reading attribute `{name}`: {err}"))?;
    Ok(v.as_str().to_string())
}

fn write_str_list_attr(loc: &hdf5::Group, name: &str, values: &[String]) -> Result<(), String> {
    let items: Vec<VarLenUnicode> = values
        .iter()
        .map(|s| VarLenUnicode::from_str(s))
        .collect::<Result<_, _>>()
        .map_err(|err| format!("save_model: attribute `{name}` isn't valid text: {err}"))?;
    let attr = loc
        .new_attr::<VarLenUnicode>()
        .shape([items.len()])
        .create(name)
        .map_err(|err| format!("save_model: creating attribute `{name}`: {err}"))?;
    attr.write_raw(&items[..])
        .map_err(|err| format!("save_model: writing attribute `{name}`: {err}"))
}

/// `save_model(net, path)` — see this module's own doc comment for the
/// full on-disk layout. Rejects anything that isn't a `kind="sequential"`
/// model (the only kind this covers, matching `docs/qu-language-spec.md`
/// §38.4's `save_model`/`load_model` example, which only talks about
/// `model`/`layer`-built networks).
pub fn save_model(m: &ModelHandle, path: &str) -> Result<(), String> {
    if m.kind != "sequential" {
        return Err(format!(
            "save_model: only sequential models can be saved (found kind=\"{}\")",
            m.kind
        ));
    }
    let layers = match m.field("layers") {
        Some(Value::List(l)) => l.clone(),
        _ => return Err("save_model: model is missing its `layers` field (corrupt model)".to_string()),
    };
    let params = match m.field("params") {
        Some(Value::List(p)) => p.clone(),
        _ => return Err("save_model: model is missing its `params` field (corrupt model)".to_string()),
    };
    if layers.len() != params.len() {
        return Err("save_model: layers/params length mismatch (corrupt model)".to_string());
    }

    let file = hdf5::File::create(path)
        .map_err(|err| format!("save_model: couldn't create \"{path}\": {err}"))?;

    let mut arch_layers = Vec::with_capacity(layers.len());
    let mut weighted_names = Vec::new();
    let mut dense_idx = 0usize;
    let mut dropout_idx = 0usize;

    for (i, (layer, p)) in layers.iter().zip(params.iter()).enumerate() {
        let fields = match layer {
            Value::Record(f) => f.as_slice(),
            other => {
                return Err(format!(
                    "save_model: layer {i} spec is not a record (corrupt model, found {})",
                    other.type_name()
                ))
            }
        };
        let kind = record_str(fields, "kind")
            .ok_or_else(|| format!("save_model: layer {i} spec is missing `kind`"))?;
        match kind.as_str() {
            "dense" => {
                let name = format!("dense_{dense_idx}");
                dense_idx += 1;
                let in_dim = record_num(fields, "in_dim")
                    .ok_or_else(|| format!("save_model: layer `{name}` is missing `in_dim`"))?
                    as usize;
                let out_dim = record_num(fields, "out_dim")
                    .ok_or_else(|| format!("save_model: layer `{name}` is missing `out_dim`"))?
                    as usize;
                let activation = record_str(fields, "activation").unwrap_or_else(|| "none".to_string());

                let pfields = match p {
                    Value::Record(f) => f.as_slice(),
                    other => {
                        return Err(format!(
                            "save_model: layer `{name}` weights are not a record (corrupt model, found {})",
                            other.type_name()
                        ))
                    }
                };
                let w = match record_field(pfields, "w") {
                    Some(Value::Mat(m)) => m.clone(),
                    _ => return Err(format!("save_model: layer `{name}` is missing weight matrix `w`")),
                };
                let b = match record_field(pfields, "b") {
                    Some(Value::Vec(v)) => v.clone(),
                    _ => return Err(format!("save_model: layer `{name}` is missing bias vector `b`")),
                };
                if w.shape() != (in_dim, out_dim) {
                    let (wr, wc) = w.shape();
                    return Err(format!(
                        "save_model: layer `{name}` weight shape ({wr}, {wc}) doesn't match its spec ({in_dim}, {out_dim})"
                    ));
                }
                if b.len() != out_dim {
                    return Err(format!(
                        "save_model: layer `{name}` bias length {} doesn't match out_dim {out_dim}",
                        b.len()
                    ));
                }

                let group = file
                    .create_group(&format!("model_weights/{name}/{name}"))
                    .map_err(|err| format!("save_model: creating group for `{name}`: {err}"))?;
                let kernel_flat = to_row_major(&w);
                group
                    .new_dataset::<f64>()
                    .shape([in_dim, out_dim])
                    .create("kernel:0")
                    .and_then(|ds| ds.write_raw(&kernel_flat[..]))
                    .map_err(|err| format!("save_model: writing `{name}/kernel:0`: {err}"))?;
                group
                    .new_dataset::<f64>()
                    .shape([out_dim])
                    .create("bias:0")
                    .and_then(|ds| ds.write_raw(&b[..]))
                    .map_err(|err| format!("save_model: writing `{name}/bias:0`: {err}"))?;
                weighted_names.push(name.clone());

                arch_layers.push(serde_json::json!({
                    "kind": "dense",
                    "name": name,
                    "in_dim": in_dim,
                    "out_dim": out_dim,
                    "activation": activation,
                }));
            }
            "dropout" => {
                let name = format!("dropout_{dropout_idx}");
                dropout_idx += 1;
                let rate = record_num(fields, "rate").unwrap_or(0.5);
                arch_layers.push(serde_json::json!({
                    "kind": "dropout",
                    "name": name,
                    "rate": rate,
                }));
            }
            other => {
                return Err(format!(
                    "save_model: layer {i} has unsupported kind `{other}` (save_model only knows \"dense\"/\"dropout\")"
                ))
            }
        }
    }

    if !weighted_names.is_empty() {
        let mw_group = file
            .group("model_weights")
            .map_err(|err| format!("save_model: {err}"))?;
        write_str_list_attr(&mw_group, "layer_names", &weighted_names)?;
    }

    let mut root = serde_json::Map::new();
    root.insert("format".to_string(), serde_json::Value::String(FORMAT.to_string()));
    root.insert("layers".to_string(), serde_json::Value::Array(arch_layers));

    if let Some(Value::Model(opt)) = m.field("optimizer") {
        let mut o = serde_json::Map::new();
        o.insert("kind".to_string(), serde_json::Value::String(opt.kind.clone()));
        for (k, v) in opt.fields.iter() {
            if let Value::Num(n) = v {
                o.insert(k.clone(), serde_json::json!(n));
            }
        }
        root.insert("optimizer".to_string(), serde_json::Value::Object(o));
    }
    if let Some(Value::Str(loss)) = m.field("loss") {
        root.insert("loss".to_string(), serde_json::Value::String(loss.clone()));
    }

    let arch_json = serde_json::to_string(&serde_json::Value::Object(root))
        .map_err(|err| format!("save_model: serializing architecture: {err}"))?;

    write_str_attr(&file, "qu_format", FORMAT)?;
    write_str_attr(&file, "qu_architecture", &arch_json)?;

    Ok(())
}

/// `load_model(path)` — the inverse of [`save_model`]. Reconstructs a
/// `kind="sequential"` `ModelHandle` field-for-field equivalent to the one
/// that was saved (same `"layers"`/`"params"` shape `sequential_forward`/
/// `.predict()`/`.fit()` already expect — see this module's own doc
/// comment), so the result is ready for `.predict()`/further `.fit()`
/// exactly like a freshly built `sequential(...)`/`compile(...)` model.
pub fn load_model(path: &str) -> Result<ModelHandle, String> {
    let file =
        hdf5::File::open(path).map_err(|err| format!("load_model: couldn't open \"{path}\": {err}"))?;

    let format = read_str_attr(&file, "qu_format").map_err(|_| {
        format!("load_model: \"{path}\" doesn't look like a Qu model file (missing `qu_format` attribute)")
    })?;
    if format != FORMAT {
        return Err(format!(
            "load_model: \"{path}\" has format `{format}`, but this build of Qu only understands `{FORMAT}`"
        ));
    }
    let arch_json = read_str_attr(&file, "qu_architecture").map_err(|_| {
        format!("load_model: \"{path}\" is missing its `qu_architecture` attribute (corrupt file)")
    })?;
    let arch: serde_json::Value = serde_json::from_str(&arch_json)
        .map_err(|err| format!("load_model: \"{path}\" has malformed `qu_architecture` JSON: {err}"))?;
    let arch_layers = arch
        .get("layers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("load_model: \"{path}\": `qu_architecture` is missing its `layers` array"))?;
    if arch_layers.is_empty() {
        return Err(format!("load_model: \"{path}\" has no layers (corrupt file)"));
    }

    let mut layers = Vec::with_capacity(arch_layers.len());
    let mut params = Vec::with_capacity(arch_layers.len());

    for (i, lj) in arch_layers.iter().enumerate() {
        let kind = lj
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("load_model: layer {i} in \"{path}\" is missing `kind`"))?;
        match kind {
            "dense" => {
                let name = lj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("load_model: layer {i} in \"{path}\" is missing `name`"))?;
                let in_dim = lj
                    .get("in_dim")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| format!("load_model: layer `{name}` is missing `in_dim`"))?
                    as usize;
                let out_dim = lj
                    .get("out_dim")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| format!("load_model: layer `{name}` is missing `out_dim`"))?
                    as usize;
                let activation = lj
                    .get("activation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none")
                    .to_string();

                let kernel_path = format!("model_weights/{name}/{name}/kernel:0");
                let ds = file
                    .dataset(&kernel_path)
                    .map_err(|err| format!("load_model: layer `{name}` is missing `{kernel_path}`: {err}"))?;
                let shape = ds.shape();
                if shape != vec![in_dim, out_dim] {
                    return Err(format!(
                        "load_model: layer `{name}` kernel shape {shape:?} doesn't match its architecture ({in_dim}, {out_dim})"
                    ));
                }
                let kernel_flat: Vec<f64> = ds
                    .read_raw()
                    .map_err(|err| format!("load_model: reading `{kernel_path}`: {err}"))?;
                let w = from_row_major(in_dim, out_dim, &kernel_flat);

                let bias_path = format!("model_weights/{name}/{name}/bias:0");
                let bds = file
                    .dataset(&bias_path)
                    .map_err(|err| format!("load_model: layer `{name}` is missing `{bias_path}`: {err}"))?;
                let b: Vec<f64> = bds
                    .read_raw()
                    .map_err(|err| format!("load_model: reading `{bias_path}`: {err}"))?;
                if b.len() != out_dim {
                    return Err(format!(
                        "load_model: layer `{name}` bias length {} doesn't match out_dim {out_dim}",
                        b.len()
                    ));
                }

                layers.push(Value::Record(Arc::new(vec![
                    ("kind".to_string(), Value::Str("dense".to_string())),
                    ("in_dim".to_string(), Value::Num(in_dim as f64)),
                    ("out_dim".to_string(), Value::Num(out_dim as f64)),
                    ("activation".to_string(), Value::Str(activation)),
                ])));
                params.push(Value::Record(Arc::new(vec![
                    ("w".to_string(), Value::Mat(Arc::new(w))),
                    ("b".to_string(), Value::Vec(Arc::new(b))),
                ])));
            }
            "dropout" => {
                let rate = lj.get("rate").and_then(|v| v.as_f64()).unwrap_or(0.5);
                layers.push(Value::Record(Arc::new(vec![
                    ("kind".to_string(), Value::Str("dropout".to_string())),
                    ("rate".to_string(), Value::Num(rate)),
                ])));
                params.push(Value::Record(Arc::new(Vec::new())));
            }
            other => {
                return Err(format!(
                    "load_model: layer {i} in \"{path}\" has unsupported kind `{other}`"
                ))
            }
        }
    }

    let mut fields = vec![
        ("layers".to_string(), Value::List(Arc::new(layers))),
        ("params".to_string(), Value::List(Arc::new(params))),
    ];

    if let Some(opt) = arch.get("optimizer").and_then(|v| v.as_object()) {
        let kind = opt
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("adam")
            .to_string();
        let mut opt_fields = Vec::new();
        for (k, v) in opt.iter() {
            if k == "kind" {
                continue;
            }
            if let Some(n) = v.as_f64() {
                opt_fields.push((k.clone(), Value::Num(n)));
            }
        }
        fields.push(("optimizer".to_string(), Value::Model(Arc::new(ModelHandle::new(kind, opt_fields)))));
    }
    if let Some(loss) = arch.get("loss").and_then(|v| v.as_str()) {
        fields.push(("loss".to_string(), Value::Str(loss.to_string())));
    }

    Ok(ModelHandle::new("sequential", fields))
}
