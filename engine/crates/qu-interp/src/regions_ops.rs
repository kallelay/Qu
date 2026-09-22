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
//! errors, it just returns something shaped differently. Errors-into-answers
//! first: this is new surface and can break nothing, so it lands now;
//! whether `regionprops` should eventually become an alias is a decision
//! that changes existing programs, and therefore Ahmed's, not this module's.
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

    // `pixel_size=` is the edge length of one pixel. Absent means the
    // measurement stays in pixels and says so in the column names, rather
    // than defaulting to 1 of some unit nobody stated -- the spectrogram
    // `fs=1` default is exactly the trap being avoided here.
    let scale = style_num(style, "pixel_size");
    if let Some(p) = scale {
        if !(p > 0.0) || !p.is_finite() {
            return e(format!(
                "image_regions: `pixel_size={p}` must be a positive, finite length per pixel"
            ));
        }
    }
    let unit = style_str(style, "unit");
    if unit.is_some() && scale.is_none() {
        return e(
            "image_regions: `unit=` was given without `pixel_size=` -- a unit name with no \
             scale would label pixel counts as though they had been converted"
                .to_string(),
        );
    }
    let unit = unit.unwrap_or_else(|| "px".to_string());
    let (len_suffix, area_suffix) = match scale {
        None => ("_px".to_string(), "_px2".to_string()),
        Some(_) => (format!("_{unit}"), format!("_{unit}2")),
    };
    let k = scale.unwrap_or(1.0);

    let (h, w) = labels.shape();
    let mut area = vec![0u64; count + 1];
    let mut sum_x = vec![0f64; count + 1];
    let mut sum_y = vec![0f64; count + 1];
    let mut min_x = vec![usize::MAX; count + 1];
    let mut max_x = vec![0usize; count + 1];
    let mut min_y = vec![usize::MAX; count + 1];
    let mut max_y = vec![0usize; count + 1];
    for y in 0..h {
        for x in 0..w {
            let lbl = labels.get(y, x).unwrap_or(0.0).round() as i64;
            if lbl <= 0 || lbl as usize > count {
                continue;
            }
            let l = lbl as usize;
            area[l] += 1;
            sum_x[l] += x as f64;
            sum_y[l] += y as f64;
            min_x[l] = min_x[l].min(x);
            max_x[l] = max_x[l].max(x);
            min_y[l] = min_y[l].min(y);
            max_y[l] = max_y[l].max(y);
        }
    }

    let mut label_c = Vec::new();
    let mut area_c = Vec::new();
    let mut cx_c = Vec::new();
    let mut cy_c = Vec::new();
    let mut bw_c = Vec::new();
    let mut bh_c = Vec::new();
    let mut ext_c = Vec::new();
    for l in 1..=count {
        if area[l] == 0 {
            continue;
        }
        let a = area[l] as f64;
        let bw = (max_x[l] - min_x[l] + 1) as f64;
        let bh = (max_y[l] - min_y[l] + 1) as f64;
        label_c.push(l as f64);
        area_c.push(a * k * k);
        cx_c.push((sum_x[l] / a) * k);
        cy_c.push((sum_y[l] / a) * k);
        bw_c.push(bw * k);
        bh_c.push(bh * k);
        // Extent is a ratio, so it is unitless and the scale cancels --
        // stated explicitly because a column that silently did NOT scale
        // would look identical to one that was forgotten.
        ext_c.push(a / (bw * bh));
    }

    let t = Table::from_columns(vec![
        ("label".to_string(), Column::Num(label_c)),
        (format!("area{area_suffix}"), Column::Num(area_c)),
        (format!("centroid_x{len_suffix}"), Column::Num(cx_c)),
        (format!("centroid_y{len_suffix}"), Column::Num(cy_c)),
        (format!("bbox_width{len_suffix}"), Column::Num(bw_c)),
        (format!("bbox_height{len_suffix}"), Column::Num(bh_c)),
        ("extent".to_string(), Column::Num(ext_c)),
    ])
    .map_err(|msg| EvalError {
        msg: format!("image_regions: {msg}"),
    })?;
    Ok(Value::Table(Arc::new(t)))
}
