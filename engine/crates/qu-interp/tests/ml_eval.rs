//! `cross_val`, `grid_search`, `roc_curve`, `pr_curve`: run as Qu source, with
//! numbers a reader can check by hand. Every assertion is on a value an empty
//! or wrong result could not have.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("script failed: {}\n{src}", e.msg));
    it
}

fn run_err(src: &str) -> String {
    let mut it = Interp::new();
    match it.run(src) {
        Ok(()) => panic!("expected an error, but this ran cleanly:\n{src}"),
        Err(e) => e.msg,
    }
}

fn num(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(x)) => *x,
        other => panic!("`{name}` is not a number: {other:?}"),
    }
}

fn vec_of(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(v)) => v.to_vec(),
        other => panic!("`{name}` is not a vector: {other:?}"),
    }
}

/// Two well-separated blobs of 10 points each (class 0 near the origin, class 1
/// near (10, 10)), as literals so no randomness is involved.
const DATA: &str = "\
X = [0, 0; 0.1, -0.1; -0.1, 0.1; 0.2, 0.05; 0.05, 0.2; -0.2, -0.05; 0.15, 0.1; -0.05, -0.15; 0.1, 0.1; 0, -0.2; 10, 10; 10.1, 9.9; 9.9, 10.1; 10.2, 10.05; 10.05, 10.2; 9.8, 9.95; 10.15, 10.1; 9.95, 9.85; 10.1, 10.1; 10, 9.8]\n\
y = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]\n";

#[test]
fn cross_val_scores_a_builtin_model_builder_passed_by_name() {
    let it = run(&format!(
        "{DATA}r = cross_val(knn_model, X, y, 3, folds=5, seed=7)\nm = r.mean\ns = r.scores\nf = r.folds"
    ));
    assert_eq!(num(&it, "f"), 5.0);
    assert_eq!(vec_of(&it, "s").len(), 5);
    assert!(num(&it, "m") > 0.99, "separable clusters should score ~1, got {}", num(&it, "m"));
}

#[test]
fn cross_val_calls_a_function_you_wrote() {
    let it = run(&format!(
        "{DATA}function fit(Xtr, ytr)\n  return knn_model(Xtr, ytr, 1, kind=\"classification\")\nend function\n\
         r = cross_val(fit, X, y, folds=4, seed=3)\nm = r.mean\nn = length(r.scores)"
    ));
    assert_eq!(num(&it, "n"), 4.0);
    assert!(num(&it, "m") > 0.99);
}

#[test]
fn the_same_seed_gives_the_same_folds_and_scores() {
    let it = run(&format!(
        "{DATA}a = cross_val(knn_model, X, y, 5, folds=6, seed=11)\n\
         b = cross_val(knn_model, X, y, 5, folds=6, seed=11)\nsa = a.scores\nsb = b.scores"
    ));
    assert_eq!(vec_of(&it, "sa"), vec_of(&it, "sb"));
}

#[test]
fn a_named_metric_scores_the_predictions() {
    let it = run(&format!(
        "{DATA}function fit(Xtr, ytr)\n  return knn_model(Xtr, ytr, 3, kind=\"classification\")\nend function\n\
         r = cross_val(fit, X, y, folds=5, seed=2, metric=\"f1\")\nm = r.mean\nname = r.metric"
    ));
    assert!(num(&it, "m") > 0.99);
    assert!(matches!(it.get("name"), Some(Value::Str(s)) if s == "f1"));
}

#[test]
fn cross_val_errors_say_what_to_change() {
    let base = DATA;
    assert!(run_err(&format!("{base}r = cross_val(knn_model, X, y, 3, folds=1)")).contains("between 2 and"));
    assert!(run_err(&format!("{base}r = cross_val(knn_model, X, y, 3, metric=\"bogus\")")).contains("unknown metric"));
    assert!(run_err(&format!("{base}r = cross_val(knn_model, X, y[0:8], 3)")).contains("X has 20 rows but y has 9"));
    assert!(run_err("r = cross_val(3, [1, 2], [1, 2])").contains("needs a function"));
}

#[test]
fn grid_search_tries_every_combination_and_reports_the_best() {
    let it = run(&format!(
        "{DATA}function fit(Xtr, ytr, p)\n  return knn_model(Xtr, ytr, p.k, kind=\"classification\")\nend function\n\
         g = grid_search(fit, X, y, {{k = [1, 3, 5, 7]}}, folds=5, seed=4)\n\
         best = g.best_score\nbi = g.best_index\nsc = g.scores\nn = length(g.results)\nk = g.best_params.k"
    ));
    let scores = vec_of(&it, "sc");
    assert_eq!(scores.len(), 4);
    assert_eq!(num(&it, "n"), 4.0);
    let max = scores.iter().cloned().fold(f64::MIN, f64::max);
    assert!((num(&it, "best") - max).abs() < 1e-15);
    assert!([1.0, 3.0, 5.0, 7.0].contains(&num(&it, "k")));
    assert!(num(&it, "best") > 0.99);
}

#[test]
fn grid_search_over_two_names_covers_the_product_with_the_last_varying_fastest() {
    let it = run(&format!(
        "{DATA}function fit(Xtr, ytr, p)\n  return knn_model(Xtr, ytr, p.k, kind=\"classification\")\nend function\n\
         g = grid_search(fit, X, y, {{k = [1, 3], tag = [10, 20, 30]}}, folds=3, seed=1)\n\
         n = length(g.results)\nfirst = g.results[0].params.tag\nsecond = g.results[1].params.tag\n\
         third = g.results[2].params.tag\nfourth_k = g.results[3].params.k"
    ));
    assert_eq!(num(&it, "n"), 6.0);
    assert_eq!((num(&it, "first"), num(&it, "second"), num(&it, "third")), (10.0, 20.0, 30.0));
    assert_eq!(num(&it, "fourth_k"), 3.0);
}

#[test]
fn grid_search_minimizes_error_metrics() {
    // a model that predicts the constant p.c: rmse is smallest at the true mean
    let it = run(
        "X = linspace(0, 9, 10)\n\
         y = X * 0 + 5\n\
         function fit(Xtr, ytr, p)\n  return ols_model(Xtr, ytr + p.shift)\nend function\n\
         g = grid_search(fit, X, y, {shift = [-2, 0, 2]}, folds=5, seed=1, metric=\"rmse\")\n\
         shift = g.best_params.shift\nbest = g.best_score",
    );
    assert_eq!(num(&it, "shift"), 0.0, "rmse is lowest with no shift");
    assert!(num(&it, "best") < 1e-6);
}

#[test]
fn grid_search_rejects_a_bad_grid() {
    let f = "function fit(Xtr, ytr, p)\n  return knn_model(Xtr, ytr, 1, kind=\"classification\")\nend function\n";
    assert!(run_err(&format!("{DATA}{f}g = grid_search(fit, X, y, [1, 2, 3])")).contains("record of lists"));
    assert!(run_err(&format!("{DATA}{f}g = grid_search(fit, X, y, {{k = []}})")).contains("no values to try"));
}

#[test]
fn roc_curve_matches_the_textbook_example() {
    let it = run(
        "r = roc_curve([0, 0, 1, 1], [0.1, 0.4, 0.35, 0.8])\nfpr = r.fpr\ntpr = r.tpr\nauc = r.auc\nthr = r.thresholds\n\
         auc2 = roc_auc([0, 0, 1, 1], [0.1, 0.4, 0.35, 0.8])",
    );
    assert_eq!(vec_of(&it, "fpr"), vec![0.0, 0.0, 0.5, 0.5, 1.0]);
    assert_eq!(vec_of(&it, "tpr"), vec![0.0, 0.5, 0.5, 1.0, 1.0]);
    assert!((num(&it, "auc") - 0.75).abs() < 1e-15);
    assert!((num(&it, "auc") - num(&it, "auc2")).abs() < 1e-12, "agrees with roc_auc");
    assert!(vec_of(&it, "thr")[0].is_infinite());
}

#[test]
fn pr_curve_matches_the_textbook_example() {
    let it = run("r = pr_curve([0, 0, 1, 1], [0.1, 0.4, 0.35, 0.8])\nap = r.ap\nrec = r.recall\nprec = r.precision");
    assert!((num(&it, "ap") - 0.8333333333333333).abs() < 1e-12);
    assert_eq!(vec_of(&it, "rec"), vec![0.5, 0.5, 1.0, 1.0]);
    assert_eq!(vec_of(&it, "prec")[0], 1.0);
}

#[test]
fn curve_errors_name_the_function_and_the_problem() {
    assert!(run_err("r = roc_curve([1, 1], [0.1, 0.2])").contains("roc_curve: needs both classes"));
    assert!(run_err("r = pr_curve([0, 2], [0.1, 0.2])").contains("pr_curve: labels must be 0 or 1"));
    assert!(run_err("r = roc_curve([0, 1], [0.1])").contains("2 labels but score has 1"));
}


#[test]
fn accuracy_scores_multiclass_labels_as_labels_not_thresholded_scores() {
    // five of six right: 5/6. Thresholding class 2 down to 1 used to give 0.5.
    let it = run("a = accuracy([0, 1, 2, 2, 1, 0], [0, 1, 2, 2, 1, 1])");
    assert!((num(&it, "a") - 5.0 / 6.0).abs() < 1e-12);
    // a perfect 3-class prediction is 1, and classes numbered from 1 work too
    let it = run("a = accuracy([1, 2, 3, 3, 2, 1], [1, 2, 3, 3, 2, 1])");
    assert_eq!(num(&it, "a"), 1.0);
    // a wrong class is a miss even when it is "closer" to the right one
    let it = run("a = accuracy([2, 2, 2, 2], [1, 2, 3, 2])");
    assert_eq!(num(&it, "a"), 0.5);
}

#[test]
fn accuracy_on_binary_labels_still_thresholds_scores() {
    // unchanged behaviour: 0/1 labels, probabilities cut at 0.5 ...
    let it = run("a = accuracy([0, 1, 1, 0], [0.2, 0.9, 0.4, 0.1])");
    assert_eq!(num(&it, "a"), 0.75);
    // ... and at a threshold the caller chooses
    let it = run("a = accuracy([0, 1, 1, 0], [0.2, 0.9, 0.4, 0.1], threshold=0.3)");
    assert_eq!(num(&it, "a"), 1.0);
    // a threshold makes no sense for multi-class labels: say so
    let msg = run_err("a = accuracy([0, 1, 2], [0, 1, 2], threshold=0.5)");
    assert!(msg.contains("more than two classes"), "{msg}");
}

#[test]
fn cross_val_accuracy_is_right_for_three_classes() {
    // three tight, far-apart blobs: every fold's kNN is right, so the CV
    // accuracy must be exactly 1 (it was 0.667 when class 2 was thresholded)
    let it = run(concat!(
        "X = [0, 0; 0.1, 0.1; -0.1, 0.1; 0.05, -0.1; 10, 0; 10.1, 0.1; 9.9, 0.1; 10.05, -0.1; 0, 10; 0.1, 10.1; -0.1, 9.9; 0.05, 10.1]\n",
        "y = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2]\n",
        "r = cross_val(knn_model, X, y, 1, folds=4, seed=3, metric=\"accuracy\")\n",
        "m = r.mean\n",
    ));
    assert!((num(&it, "m") - 1.0).abs() < 1e-12);
}
