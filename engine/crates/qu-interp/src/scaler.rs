//! Per-column feature-scaling math shared by the four one-shot verbs
//! (`normalize`/`standardize`/`robust_scale`/`quantile_normalize`) and the
//! reusable `fit_scaler(X, method=)` / `.transform` / `.inverse_transform`
//! path (`Value::Model` kind `"scaler"`, dispatched in `qu-interp/src/
//! lib.rs`). Deliberately independent of `Value`: every caller — a one-shot
//! verb fitting fresh on `X`, or `fit_scaler` storing the fit for reuse on a
//! *different* `newX` later — funnels through exactly these two functions
//! (`fit_column`/`transform_column`), so `standardize(X) ==
//! fit_scaler(X, method="standard").transform(X)` holds because it is
//! literally the same code path, not because two independent
//! implementations were kept in sync by hand.
//!
//! A `Mat` scales **per column** (rows are samples, columns are features —
//! the standard ML convention, matching `pca`'s own column-mean-centering);
//! a plain `Vec`/`Signal`/scalar scales as a single feature (one column of
//! length 1..N). `qu-interp/src/lib.rs` owns that `Value`-shape dispatch;
//! this module only ever sees one column (`&[f64]`) at a time.

use crate::{e, quantile_of, std_dev, R};

/// One column's fitted parameters for one scaling method. `Quantile` stores
/// the *whole* sorted reference column (the empirical CDF's support), not a
/// handful of summary numbers — a quantile transform interpolates through
/// the entire fitted distribution, not just two endpoints like the other
/// three methods.
#[derive(Clone, Debug)]
pub enum ColumnParams {
    Standard { mean: f64, std: f64 },
    MinMax { min: f64, max: f64 },
    Robust { median: f64, iqr: f64 },
    /// Sorted ascending. `quantile_normalize`'s target distribution is
    /// **uniform on `[0, 1]`** (each value replaced by its empirical-CDF
    /// rank), not a normal-quantile (`QuantileTransformer(output_
    /// distribution="normal")`'s inverse-erf mapping) — documented default
    /// per the design note: the uniform target is the well-established
    /// simpler option, and adding an inverse-normal-CDF on top would be a
    /// second numerical approximation (no closed form) for a target this
    /// feature wasn't specifically asked to hit.
    Quantile { reference: Vec<f64> },
}

pub fn method_name(p: &ColumnParams) -> &'static str {
    match p {
        ColumnParams::Standard { .. } => "standard",
        ColumnParams::MinMax { .. } => "minmax",
        ColumnParams::Robust { .. } => "robust",
        ColumnParams::Quantile { .. } => "quantile",
    }
}

/// Fits one column under `method` (`"standard"`/`"minmax"`/`"robust"`/
/// `"quantile"`). `col` must be non-empty (checked by every caller before
/// this — an empty `Vec`/an empty matrix column has no distribution to fit).
pub fn fit_column(method: &str, col: &[f64]) -> R<ColumnParams> {
    if col.is_empty() {
        return e("fit_scaler: empty column has no distribution to fit");
    }
    match method {
        "standard" => {
            let mean = col.iter().sum::<f64>() / col.len() as f64;
            Ok(ColumnParams::Standard { mean, std: std_dev(col) })
        }
        "minmax" => {
            let min = col.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = col.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            Ok(ColumnParams::MinMax { min, max })
        }
        "robust" => {
            let median = quantile_of(col, 0.5)?;
            let q1 = quantile_of(col, 0.25)?;
            let q3 = quantile_of(col, 0.75)?;
            Ok(ColumnParams::Robust { median, iqr: q3 - q1 })
        }
        "quantile" => {
            let mut reference = col.to_vec();
            reference.sort_by(f64::total_cmp);
            Ok(ColumnParams::Quantile { reference })
        }
        other => e(format!(
            "fit_scaler: unknown method `{other}` (expected \"standard\", \"minmax\", \"robust\", or \"quantile\")"
        )),
    }
}

/// A zero (or near-zero) spread denominator would divide every value into
/// `NaN`/`Inf`; every method treats a constant/degenerate column as "already
/// centered, nothing to scale" instead — the numerator (`x - center`) is
/// already `0` for every sample in that case, so dividing by `1` instead of
/// `0` yields a well-defined all-zero output rather than propagating a NaN.
fn safe_scale(spread: f64) -> f64 {
    if spread == 0.0 {
        1.0
    } else {
        spread
    }
}

/// Applies already-fitted parameters to `col` (which may be different data
/// than what was fit — that reuse is `fit_scaler`'s entire point).
pub fn transform_column(p: &ColumnParams, col: &[f64]) -> Vec<f64> {
    match p {
        ColumnParams::Standard { mean, std } => {
            let s = safe_scale(*std);
            col.iter().map(|x| (x - mean) / s).collect()
        }
        ColumnParams::MinMax { min, max } => {
            let s = safe_scale(max - min);
            col.iter().map(|x| (x - min) / s).collect()
        }
        ColumnParams::Robust { median, iqr } => {
            let s = safe_scale(*iqr);
            col.iter().map(|x| (x - median) / s).collect()
        }
        ColumnParams::Quantile { reference } => {
            col.iter().map(|&x| empirical_rank(reference, x)).collect()
        }
    }
}

/// Undoes `transform_column`. For `Quantile`, mapping a rank back to a
/// value is exactly `quantile_of(reference, rank)` — the same linear-
/// interpolation-between-order-statistics quantile function every other
/// `qu-interp` quantile builtin already uses, which is precisely the
/// mathematical inverse of `empirical_rank`'s forward mapping when
/// `reference` has no repeated values. Repeated values in the fitted
/// reference make the forward map many-to-one (several tied inputs share
/// one rank) — a real, documented precision limit: the inverse can only
/// recover *a* value consistent with that rank, not necessarily the exact
/// original one, in that case. `rank` is clamped to `[0, 1]` first so an
/// out-of-range input (a rank that was never produced by `transform_column`)
/// still returns the nearest in-range value instead of erroring.
pub fn inverse_transform_column(p: &ColumnParams, col: &[f64]) -> R<Vec<f64>> {
    match p {
        ColumnParams::Standard { mean, std } => Ok(col.iter().map(|x| x * std + mean).collect()),
        ColumnParams::MinMax { min, max } => Ok(col.iter().map(|x| x * (max - min) + min).collect()),
        ColumnParams::Robust { median, iqr } => Ok(col.iter().map(|x| x * iqr + median).collect()),
        ColumnParams::Quantile { reference } => col
            .iter()
            .map(|&r| quantile_of(reference, r.clamp(0.0, 1.0)))
            .collect(),
    }
}

/// The empirical-CDF rank of `x` against the fitted `reference` (sorted
/// ascending): `0.0` at or below the fitted minimum, `1.0` at or above the
/// fitted maximum, linearly interpolated between the two bracketing order
/// statistics' own ranks (`i/(n-1)`) in between. This is the forward
/// direction `quantile_of` (which maps rank -> value) inverts: transforming
/// the exact data a column was fit on reproduces each sample's own
/// `i/(n-1)` rank exactly, since `x` then equals `reference[i]` exactly.
fn empirical_rank(reference: &[f64], x: f64) -> f64 {
    let n = reference.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        // A single fitted point has no spread to place `x` within; `0.5`
        // (dead center of the target `[0,1]` range) is the only
        // non-arbitrary choice.
        return 0.5;
    }
    if x <= reference[0] {
        return 0.0;
    }
    if x >= reference[n - 1] {
        return 1.0;
    }
    // Binary search for the first index whose value is >= x.
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if reference[mid] < x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let i1 = lo;
    let i0 = lo - 1; // safe: x > reference[0] was already ruled out above
    let (x0, x1) = (reference[i0], reference[i1]);
    let r0 = i0 as f64 / (n - 1) as f64;
    let r1 = i1 as f64 / (n - 1) as f64;
    if x1 > x0 {
        r0 + (r1 - r0) * (x - x0) / (x1 - x0)
    } else {
        // A flat (tied-value) region of the fitted reference: every x in
        // this bracket maps to the same value, so there's no fractional
        // position to interpolate — split the difference.
        (r0 + r1) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variance;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }

    #[test]
    fn standard_hand_example() {
        // mean=3, population values 1,2,3,4,5 -> sample std = sqrt(2.5)
        let col = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let p = fit_column("standard", &col).unwrap();
        let ColumnParams::Standard { mean, std } = p else { panic!() };
        approx(mean, 3.0);
        approx(std, variance(&col).sqrt());
        let out = transform_column(&ColumnParams::Standard { mean, std }, &col);
        approx(out.iter().sum::<f64>() / out.len() as f64, 0.0);
    }

    #[test]
    fn minmax_hand_example() {
        let col = vec![10.0, 20.0, 30.0];
        let p = fit_column("minmax", &col).unwrap();
        let out = transform_column(&p, &col);
        approx(out[0], 0.0);
        approx(out[1], 0.5);
        approx(out[2], 1.0);
    }

    #[test]
    fn robust_hand_example() {
        let col = vec![1.0, 2.0, 3.0, 4.0, 100.0]; // outlier at the end
        let p = fit_column("robust", &col).unwrap();
        let ColumnParams::Robust { median, iqr } = p else { panic!() };
        approx(median, 3.0);
        // q1 = quantile_of(0.25) over [1,2,3,4,100] = 2, q3 = 4 -> iqr = 2
        approx(iqr, quantile_of(&col, 0.75).unwrap() - quantile_of(&col, 0.25).unwrap());
        let out = transform_column(&p, &col);
        approx(out[2], 0.0); // median maps to 0
    }

    #[test]
    fn quantile_forward_matches_fitted_ranks_exactly() {
        let col = vec![5.0, 1.0, 3.0, 2.0, 4.0];
        let p = fit_column("quantile", &col).unwrap();
        let out = transform_column(&p, &col);
        // 1 -> rank 0, 2 -> 0.25, 3 -> 0.5, 4 -> 0.75, 5 -> 1.0
        let mut paired: Vec<(f64, f64)> = col.iter().cloned().zip(out).collect();
        paired.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let expected = [0.0, 0.25, 0.5, 0.75, 1.0];
        for (i, (_, r)) in paired.iter().enumerate() {
            approx(*r, expected[i]);
        }
    }

    #[test]
    fn quantile_round_trip() {
        let col = vec![5.0, 1.0, 3.0, 2.0, 4.0, 10.0, -2.0];
        let p = fit_column("quantile", &col).unwrap();
        let out = transform_column(&p, &col);
        let back = inverse_transform_column(&p, &out).unwrap();
        for (a, b) in col.iter().zip(back.iter()) {
            approx(*a, *b);
        }
    }

    #[test]
    fn standard_round_trip() {
        let col = vec![2.0, 4.0, 6.0, 8.0];
        let p = fit_column("standard", &col).unwrap();
        let out = transform_column(&p, &col);
        let back = inverse_transform_column(&p, &out).unwrap();
        for (a, b) in col.iter().zip(back.iter()) {
            approx(*a, *b);
        }
    }

    #[test]
    fn constant_column_does_not_divide_by_zero() {
        let col = vec![7.0, 7.0, 7.0];
        for method in ["standard", "minmax", "robust"] {
            let p = fit_column(method, &col).unwrap();
            let out = transform_column(&p, &col);
            assert!(out.iter().all(|v| *v == 0.0), "{method}: {out:?}");
            let back = inverse_transform_column(&p, &out).unwrap();
            for b in back {
                approx(b, 7.0);
            }
        }
    }
}
