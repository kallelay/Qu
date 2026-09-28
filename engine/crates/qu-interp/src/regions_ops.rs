//! § image measurement in physical units (2026-09-16).
//!
//! `image_regions(labeled, [pixel_size=], [unit=])` — the image spec's §5,
//! which the gap analysis measured as the highest-value image gap:
//! `regionprops` returns a **list** of records in **pixels**, so a
//! measurement cannot be filtered, grouped, joined or plotted with the
//! language's ordinary verbs, and an area is a number whose unit lives in
//! the caller's head.
//!
//! `image_regions` returns a `Table` and scales by a stated pixel size.
//!
//! **Why a new name rather than changing `regionprops`.** Changing what
//! `regionprops` returns would silently change the output of every script
//! that already indexes its list — the failure mode being that nothing
//! errors, it just returns something shaped differently.
//!
//! **One measurement, three shapes (decided 2026-09-28).** `regionprops`/
//! `blob_stats` (list of records), `image_regions` (7-column Table) and
//! `image.regions` (full Table) keep their names and shapes, but all three
//! now measure through `qu_image::blob_boxes`/`qu_image::regions` -- they
//! had been three independent copies of the same pixel loop.
//!
//! **Why `image_regions`, not the originally-written `regions`.** A signal-
//! toolkit accessor named `regions(signal)` (calibration/markers metadata,
//! `add_region`'s getter) landed on master the same night this module was
//! written, off a different branch -- neither author could have known. Both
//! match arms compiled clean and silently shadowed each other; only the
//! compiler's "unreachable pattern" warning at merge time caught it. Renamed
//! this one since the signal accessor is the older, already-public name.
//!
//! **Why the unit is in the column name.** A `Table` column is `Num(Vec<f64>)`
//! or `Str(Vec<String>)` — there is nowhere to hang a unit tag on a column,
//! so `area` in µm² is reported as `area_um2` and the pixel case as
//! `area_px`. That is less than the spec asks for (`r.area == 412 um^2`,
//! a real `Quantity`) and it is chosen over the alternative of a column
//! named `area` whose unit depends on an argument the reader cannot see
//! from the result. The name changes when the meaning changes.

use crate::table::{Column, Table};
use crate::{arg0, e, style_num, style_str, EvalError, Value, R};
use std::sync::Arc;

pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "image_regions" => regions(args, style),
        other => e(format!("regions_ops: unknown function `{other}`")),
    }
}

fn regions(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let Value::Model(m) = arg0(args)? else {
        return e(format!(
            "image_regions(labeled) needs a label_blobs/bwlabel result, found {}",
            arg0(args)?.type_name()
        ));
    };
    if m.kind != "blobs" {
        return e(format!(
            "image_regions(labeled) needs a label_blobs/bwlabel result, found a `{}` model",
            m.kind
        ));
    }
    let Some(Value::Mat(labels)) = m.field("labels") else {
        return e("image_regions: malformed blob model (missing `labels`)".to_string());
    };
    let count = match m.field("count") {
        Some(v) => v.as_num().map_err(|msg| EvalError { msg })? as usize,
        None => return e("image_regions: malformed blob model (missing `count`)".to_string()),
    };

    let (h, w) = labels.shape();
    let flat: Vec<f64> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (y, x)))
        .map(|(y, x)| labels.get(y, x).unwrap_or(0.0))
        .collect();
    // The measurement itself is `image.regions`'s (`qu_image::regions`),
    // so the two can never disagree; this name keeps the seven columns it
    // has always returned. Its unit checks are that function's too.
    let rt = qu_image::regions(
        &flat,
        h,
        w,
        count,
        style_num(style, "pixel_size"),
        style_str(style, "unit").as_deref(),
        None,
    )
    .map_err(|msg| EvalError {
        msg: match msg.strip_prefix("regions:") {
            Some(rest) => format!("image_regions:{rest}"),
            None => msg,
        },
    })?;
    let keep = |name: &str| {
        name == "label"
            || name == "extent"
            || ["area_", "centroid_x_", "centroid_y_", "bbox_width_", "bbox_height_"]
                .iter()
                .any(|p| name.starts_with(p))
    };
    let t = Table::from_columns(
        rt.columns
            .into_iter()
            .filter(|(name, _)| keep(name))
            .map(|(name, vals)| (name, Column::Num(vals)))
            .collect(),
    )
    .map_err(|msg| EvalError {
        msg: format!("image_regions: {msg}"),
    })?;
    Ok(Value::Table(Arc::new(t)))
}
