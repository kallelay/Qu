//! Curves and summaries used when judging a classifier: ROC, precision-recall,
//! and the mean/spread of cross-validation scores. Plain `f64` slices in,
//! numbers out, so the arithmetic is tested without a model or an interpreter.

/// A binary label: `0`/`1` (anything else is an error the caller reports).
fn label(v: f64) -> Option<bool> {
    if v == 1.0 {
        Some(true)
    } else if v == 0.0 {
        Some(false)
    } else {
        None
    }
}

/// Scores sorted high to low, each with its label, then grouped by distinct
/// score: for every distinct threshold the running (true positives, false
/// positives) once every sample scoring at least that much is predicted
/// positive. Equal scores move together, so a tie never invents a point.
struct Sweep {
    thresholds: Vec<f64>,
    tp: Vec<f64>,
    fp: Vec<f64>,
    positives: f64,
    negatives: f64,
}

fn sweep(actual: &[f64], score: &[f64]) -> Result<Sweep, String> {
    if actual.len() != score.len() {
        return Err(format!(
            "actual has {} labels but score has {} values",
            actual.len(),
            score.len()
        ));
    }
    if actual.is_empty() {
        return Err("needs at least one sample".to_string());
    }
    let mut pairs: Vec<(f64, bool)> = Vec::with_capacity(actual.len());
    for (&a, &s) in actual.iter().zip(score) {
        let l = label(a).ok_or_else(|| format!("labels must be 0 or 1, found {a}"))?;
        if s.is_nan() {
            return Err("a score is NaN".to_string());
        }
        pairs.push((s, l));
    }
    let positives = pairs.iter().filter(|p| p.1).count() as f64;
    let negatives = pairs.len() as f64 - positives;
    if positives == 0.0 || negatives == 0.0 {
        return Err("needs both classes (found only one)".to_string());
    }
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let (mut thresholds, mut tp, mut fp) = (Vec::new(), Vec::new(), Vec::new());
    let (mut t, mut f) = (0.0, 0.0);
    let mut i = 0;
    while i < pairs.len() {
        let s = pairs[i].0;
        while i < pairs.len() && pairs[i].0 == s {
            if pairs[i].1 {
                t += 1.0;
            } else {
                f += 1.0;
            }
            i += 1;
        }
        thresholds.push(s);
        tp.push(t);
        fp.push(f);
    }
    Ok(Sweep { thresholds, tp, fp, positives, negatives })
}

/// ROC curve of `score` against 0/1 `actual`: false-positive rate, true-
/// positive rate, the threshold of each point (the first point, rate (0, 0),
/// has threshold `+inf`), and the area under it by the trapezoid rule.
pub fn roc_curve(actual: &[f64], score: &[f64]) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>, f64), String> {
    let s = sweep(actual, score)?;
    let mut fpr = vec![0.0];
    let mut tpr = vec![0.0];
    let mut thr = vec![f64::INFINITY];
    for k in 0..s.thresholds.len() {
        fpr.push(s.fp[k] / s.negatives);
        tpr.push(s.tp[k] / s.positives);
        thr.push(s.thresholds[k]);
    }
    let auc = fpr
        .windows(2)
        .zip(tpr.windows(2))
        .map(|(x, y)| (x[1] - x[0]) * (y[0] + y[1]) / 2.0)
        .sum();
    Ok((fpr, tpr, thr, auc))
}

/// Precision-recall curve: precision and recall at each distinct threshold
/// (high to low), and the average precision `sum (R_n - R_{n-1}) * P_n`.
pub fn pr_curve(actual: &[f64], score: &[f64]) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>, f64), String> {
    let s = sweep(actual, score)?;
    let mut precision = Vec::new();
    let mut recall = Vec::new();
    let mut ap = 0.0;
    let mut prev_recall = 0.0;
    for k in 0..s.thresholds.len() {
        let p = s.tp[k] / (s.tp[k] + s.fp[k]);
        let r = s.tp[k] / s.positives;
        ap += (r - prev_recall) * p;
        prev_recall = r;
        precision.push(p);
        recall.push(r);
    }
    Ok((precision, recall, s.thresholds, ap))
}

/// Mean and population standard deviation (ddof = 0) of cross-validation scores.
pub fn mean_std(scores: &[f64]) -> (f64, f64) {
    let n = scores.len() as f64;
    let mean = scores.iter().sum::<f64>() / n;
    let var = scores.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// Metrics where a smaller number is the better model.
pub fn lower_is_better(metric: &str) -> bool {
    matches!(metric, "rmse" | "mae" | "mse" | "brier")
}

/// Metrics `cross_val` and `grid_search` accept besides `"auto"` (the model's
/// own `score`).
pub const METRICS: &[&str] = &["accuracy", "f1", "precision", "recall", "r2", "rmse", "mae", "mse", "roc_auc", "brier"];

#[cfg(test)]
mod tests {
    use super::*;

    // the worked example from scikit-learn's documentation
    const Y: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
    const S: [f64; 4] = [0.1, 0.4, 0.35, 0.8];

    #[test]
    fn roc_matches_the_textbook_example() {
        let (fpr, tpr, thr, auc) = roc_curve(&Y, &S).unwrap();
        assert_eq!(fpr, vec![0.0, 0.0, 0.5, 0.5, 1.0]);
        assert_eq!(tpr, vec![0.0, 0.5, 0.5, 1.0, 1.0]);
        assert_eq!(thr[1..], [0.8, 0.4, 0.35, 0.1]);
        assert!(thr[0].is_infinite());
        assert!((auc - 0.75).abs() < 1e-15);
    }

    #[test]
    fn pr_matches_the_textbook_example() {
        let (p, r, thr, ap) = pr_curve(&Y, &S).unwrap();
        assert_eq!(thr, vec![0.8, 0.4, 0.35, 0.1]);
        assert_eq!(r, vec![0.5, 0.5, 1.0, 1.0]);
        assert!((p[0] - 1.0).abs() < 1e-15 && (p[1] - 0.5).abs() < 1e-15);
        assert!((ap - 0.8333333333333333).abs() < 1e-12, "ap {ap}");
    }

    #[test]
    fn perfect_and_reversed_scores_give_auc_one_and_zero() {
        assert!((roc_curve(&[0.0, 0.0, 1.0, 1.0], &[0.1, 0.2, 0.8, 0.9]).unwrap().3 - 1.0).abs() < 1e-15);
        assert!(roc_curve(&[0.0, 0.0, 1.0, 1.0], &[0.9, 0.8, 0.2, 0.1]).unwrap().3.abs() < 1e-15);
    }

    #[test]
    fn tied_scores_move_together() {
        // all scores equal: one threshold, so the curve is the diagonal, AUC 0.5
        let (fpr, tpr, _, auc) = roc_curve(&[0.0, 1.0, 0.0, 1.0], &[0.5; 4]).unwrap();
        assert_eq!(fpr, vec![0.0, 1.0]);
        assert_eq!(tpr, vec![0.0, 1.0]);
        assert!((auc - 0.5).abs() < 1e-15);
    }

    #[test]
    fn bad_input_is_an_error_that_says_why() {
        assert!(roc_curve(&[0.0, 1.0], &[0.1]).unwrap_err().contains("2 labels but score has 1"));
        assert!(roc_curve(&[0.0, 2.0], &[0.1, 0.2]).unwrap_err().contains("0 or 1"));
        assert!(roc_curve(&[1.0, 1.0], &[0.1, 0.2]).unwrap_err().contains("both classes"));
        assert!(roc_curve(&[], &[]).unwrap_err().contains("at least one"));
        assert!(pr_curve(&[0.0, 1.0], &[f64::NAN, 0.2]).unwrap_err().contains("NaN"));
    }

    #[test]
    fn mean_std_is_population() {
        let (m, s) = mean_std(&[1.0, 2.0, 3.0, 4.0]);
        assert!((m - 2.5).abs() < 1e-15 && (s - 1.25_f64.sqrt()).abs() < 1e-15);
    }

    #[test]
    fn error_metrics_are_lower_is_better() {
        assert!(lower_is_better("rmse") && lower_is_better("mae"));
        assert!(!lower_is_better("accuracy") && !lower_is_better("f1"));
    }
}
