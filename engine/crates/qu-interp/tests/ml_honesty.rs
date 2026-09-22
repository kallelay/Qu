//! § the ML honesty layer (2026-09-16).
//!
//! The metrics are pinned to values computable by hand, and the two
//! statistical guarantees — conformal coverage and grouped folds — are
//! tested by the property they promise rather than by a recorded number.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    format!("{}", it.run(src).unwrap_err())
}

// ------------------------------------------------------------- metrics

#[test]
fn r2_is_one_for_a_perfect_fit_and_zero_for_the_mean() {
    let it = run(
        r#"
perfect = r2([1, 2, 3, 4], [1, 2, 3, 4])
mean_only = r2([1, 2, 3, 4], [2.5, 2.5, 2.5, 2.5])
"#,
    );
    assert_eq!(num_of(&it, "perfect"), 1.0);
    assert!(num_of(&it, "mean_only").abs() < 1e-12);
}

#[test]
fn r2_refuses_a_constant_target_instead_of_answering() {
    // ss_tot is 0, so every answer is a claim the data cannot support.
    // Returning 0 or 1 here is the quiet wrong answer.
    let msg = err("x = r2([3, 3, 3], [3, 3, 3])");
    assert!(msg.contains("no variance"), "got: {msg}");
}

#[test]
fn roc_auc_matches_hand_computable_cases() {
    let it = run(
        r#"
perfect  = roc_auc([0, 0, 1, 1], [0.1, 0.2, 0.8, 0.9])
inverted = roc_auc([0, 0, 1, 1], [0.9, 0.8, 0.2, 0.1])
tied     = roc_auc([0, 1], [0.5, 0.5])
"#,
    );
    assert_eq!(num_of(&it, "perfect"), 1.0);
    assert_eq!(num_of(&it, "inverted"), 0.0);
    assert_eq!(
        num_of(&it, "tied"),
        0.5,
        "a tie must count as half, which a sort-and-trapezoid implementation gets wrong"
    );
}

#[test]
fn roc_auc_refuses_one_class_and_non_binary_labels() {
    assert!(err("x = roc_auc([1, 1], [0.2, 0.8])").contains("undefined without both"));
    let msg = err("x = roc_auc([0, 2], [0.2, 0.8])");
    assert!(
        msg.contains("must be 0 or 1"),
        "treating any non-zero as positive would score a different problem, got: {msg}"
    );
}

#[test]
fn brier_rewards_calibration_not_just_correctness() {
    // Both predictions are on the right side of 0.5, so accuracy is 1.0 for
    // each; only Brier separates the confident one from the hedged one.
    let it = run(
        r#"
sure  = brier([1, 0], [0.99, 0.01])
hedge = brier([1, 0], [0.6, 0.4])
acc_s = accuracy([1, 0], [0.99, 0.01])
acc_h = accuracy([1, 0], [0.6, 0.4])
"#,
    );
    assert_eq!(num_of(&it, "acc_s"), 1.0);
    assert_eq!(num_of(&it, "acc_h"), 1.0);
    assert!(num_of(&it, "sure") < num_of(&it, "hedge"));
}

#[test]
fn a_length_mismatch_is_refused_rather_than_scored_pairwise() {
    let msg = err("x = r2([1, 2, 3], [1, 2])");
    assert!(msg.contains("3 elements") && msg.contains("2"), "got: {msg}");
}

// -------------------------------------------------------------- splits

#[test]
fn kfold_partitions_every_row_exactly_once_across_test_sets() {
    let it = run(
        r#"
folds = kfold(10, 5, seed=1)
total = 0
for f in folds
    total = total + len(f.test)
end
k = len(folds)
"#,
    );
    assert_eq!(num_of(&it, "k"), 5.0);
    assert_eq!(
        num_of(&it, "total"),
        10.0,
        "the test sets must tile the rows -- overlap or omission both inflate a score"
    );
}

#[test]
fn kfold_keeps_a_group_whole() {
    // This is the entire point of `group=`. Five groups of two rows: no
    // fold may contain one member of a group without the other, or a
    // repeated measurement leaks from train into test.
    let it = run(
        r#"
g = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4]
folds = kfold(10, 5, group=g, seed=7)
split_groups = 0
for f in folds
    for gid in [0, 1, 2, 3, 4]
        in_test = 0
        for i in f.test
            if g[i] == gid
                in_test = in_test + 1
            end
        end
        if in_test == 1
            split_groups = split_groups + 1
        end
    end
end
"#,
    );
    assert_eq!(
        num_of(&it, "split_groups"),
        0.0,
        "a group was split across train and test -- this is the leak group= exists to prevent"
    );
}

#[test]
fn kfold_is_reproducible_from_its_seed() {
    let it = run(
        r#"
a = kfold(20, 4, seed=42)
b = kfold(20, 4, seed=42)
c = kfold(20, 4, seed=43)
same = sum(abs(a[0].test - b[0].test))
diff = sum(abs(a[0].test - c[0].test))
"#,
    );
    // Compared by summed absolute difference, not `==`: on vectors `==`
    // returns an elementwise mask, so `assert(a == b)` would be testing a
    // mask for truthiness rather than the vectors for equality.
    assert_eq!(num_of(&it, "same"), 0.0, "the same seed must give the same folds");
    assert!(
        num_of(&it, "diff") > 0.0,
        "different seeds must actually shuffle differently, or seed= is decorative --          the first draft of this module accepted seed= and ignored it entirely"
    );
}

#[test]
fn kfold_refuses_impossible_splits() {
    assert!(err("x = kfold(3, 5)").contains("cannot be split"));
    assert!(err("x = kfold(10, 1)").contains("at least 2"));
    let msg = err("x = kfold(10, 5, group=[0,0,0,0,0,1,1,1,1,1])");
    assert!(msg.contains("distinct group"), "got: {msg}");
}

#[test]
fn split_time_holds_out_the_end_in_order() {
    let it = run(
        r#"
s = split_time(10, 3)
first_test = s.test[0]
last_train = s.train[len(s.train) - 1]
ntrain = len(s.train)
"#,
    );
    assert_eq!(num_of(&it, "ntrain"), 7.0);
    assert_eq!(num_of(&it, "first_test"), 7.0);
    assert_eq!(
        num_of(&it, "last_train"),
        6.0,
        "training must end before testing begins, or the model trains on the future"
    );
}

// --------------------------------------------------------- uncertainty

#[test]
fn conformal_half_width_covers_at_the_stated_rate() {
    // The guarantee, tested as a property: with 99 calibration residuals
    // drawn from the same process as 200 fresh ones, a 90% interval should
    // cover about 90% of the fresh points -- and never far below it.
    let it = run(
        r#"
seed(11)
calib = randn(99)
fresh = randn(200)
h = conformal(calib, level=0.90)
lo = fresh * 0 - h
hi = fresh * 0 + h
cov = coverage(fresh, lo, hi)
"#,
    );
    let cov = num_of(&it, "cov");
    assert!(
        cov >= 0.80,
        "a 90% conformal interval covered only {cov} of fresh draws"
    );
    assert!(cov <= 1.0);
}

#[test]
fn conformal_refuses_a_level_its_sample_cannot_support() {
    // 5 residuals cannot justify a 99% interval: ceil((n+1)*level) = 6 > 5.
    // Silently returning the largest residual would report a weaker
    // guarantee under the name of the stronger one.
    let msg = err("x = conformal([1, 2, 3, 4, 5], level=0.99)");
    assert!(
        msg.contains("cannot support") && msg.contains("calibration points"),
        "got: {msg}"
    );
}

#[test]
fn conformal_takes_residuals_or_a_pair_and_agrees() {
    let it = run(
        r#"
a = conformal([1, -2, 3, -4, 5, 6, 7, 8, 9, 10], level=0.8)
b = conformal([1, 0, 3, 0, 5, 6, 7, 8, 9, 10], [0, 2, 0, 4, 0, 0, 0, 0, 0, 0], level=0.8)
"#,
    );
    assert_eq!(
        num_of(&it, "a"),
        num_of(&it, "b"),
        "the two call forms must produce the same half-width; the pair form exists so a \
         caller cannot hand in signed residuals by accident"
    );
}

#[test]
fn coverage_reports_the_shortfall_rather_than_hiding_it() {
    let it = run(
        r#"
c = coverage([1, 2, 3, 100], [0, 0, 0, 0], [5, 5, 5, 5])
"#,
    );
    assert_eq!(num_of(&it, "c"), 0.75);
    assert!(err("x = coverage([1], [5], [0])").contains("above upper bound"));
}

#[test]
fn bootstrap_ci_brackets_the_point_estimate_and_is_reproducible() {
    let it = run(
        r#"
seed(3)
y = randn(60)
yhat = y * 0.8 + randn(60) * 0.3
a = bootstrap_ci(y, yhat, metric="r2", level=0.95, n=400, seed=5)
b = bootstrap_ci(y, yhat, metric="r2", level=0.95, n=400, seed=5)
same_lo = a.lo == b.lo
point = a.value
lo = a.lo
hi = a.hi
direct = r2(y, yhat)
"#,
    );
    assert!(matches!(it.get("same_lo"), Some(Value::Bool(true))));
    assert_eq!(
        num_of(&it, "point"),
        num_of(&it, "direct"),
        "the reported point estimate must be the metric itself, not a bootstrap mean"
    );
    assert!(num_of(&it, "lo") <= num_of(&it, "point"));
    assert!(num_of(&it, "hi") >= num_of(&it, "point"));
    assert!(
        num_of(&it, "hi") > num_of(&it, "lo"),
        "a zero-width interval would mean the resampling did nothing"
    );
}

#[test]
fn bootstrap_ci_refuses_an_unknown_metric_and_too_few_resamples() {
    assert!(err("x = bootstrap_ci([1,2,3], [1,2,3], metric=\"auc\")").contains("not supported"));
    assert!(err("x = bootstrap_ci([1,2,3], [1,2,3], n=10)").contains("too few"));
}

// ------------------------------------------- exchangeability (2026-09-16)

#[test]
fn conformal_refuses_correlated_residuals_rather_than_narrowing() {
    // Added after the ECM lane measured lag-1 residual autocorrelation on a
    // CNLS impedance fit: -0.088 with the correct circuit, +0.885 with the
    // wrong one. Correlated residuals under-represent a fresh draw, so the
    // conformal band gets MORE confident as the model gets more wrong --
    // the band is least justified exactly where it looks best.
    let msg = err(
        r#"
r = [0]
for i in 1 to 60
    r = append(r, r[i - 1] * 0.95 + randn() * 0.1)
end
h = conformal(r, level=0.9)
"#,
    );
    assert!(
        msg.contains("not exchangeable") && msg.contains("lag-1"),
        "got: {msg}"
    );
}

#[test]
fn an_explicit_override_is_available_but_must_be_stated() {
    let it = run(
        r#"
seed(5)
r = [0]
for i in 1 to 60
    r = append(r, r[i - 1] * 0.95 + randn() * 0.1)
end
h = conformal(r, level=0.9, assume_exchangeable=true)
"#,
    );
    assert!(num_of(&it, "h") > 0.0);
}

#[test]
fn independent_residuals_pass_the_check() {
    // The guard must not fire on the case it is meant to allow, or it is a
    // refusal that makes the function unusable rather than honest.
    let it = run("seed(2)\nr = randn(200)\nh = conformal(r, level=0.9)\n");
    assert!(num_of(&it, "h") > 0.0);
}

#[test]
fn residual_acf_separates_a_correct_fit_from_a_wrong_one() {
    let it = run(
        r#"
seed(4)
indep = residual_acf(randn(300))
c = [0]
for i in 1 to 300
    c = append(c, c[i - 1] * 0.9 + randn() * 0.1)
end
corr = residual_acf(c)
"#,
    );
    assert!(
        num_of(&it, "indep").abs() < 0.3,
        "independent draws must read near zero, got {}",
        num_of(&it, "indep")
    );
    assert!(
        num_of(&it, "corr") > 0.5,
        "a strongly correlated sequence must read high, got {}",
        num_of(&it, "corr")
    );
}

#[test]
fn bootstrap_ci_keeps_the_asymmetry_of_a_skewed_metric() {
    // Percentiles of the resampling distribution, NOT value +/- k*sd.
    // Raised by the ECM lane: for a log-scaled or bounded quantity the two
    // tails genuinely differ, and a symmetric +/- summary describes it
    // badly -- their example was a magnitude parameter where "861%" stood
    // in for a 14.66-decade interval. R2 near its ceiling is the same
    // shape: the distribution piles up against 1 and has a long left tail.
    //
    // This test exists to stop a later "simplification" to value +/- 1.96*sd,
    // which would pass every other test in this file.
    let it = run(
        r#"
seed(9)
y = randn(80)
yhat = y * 0.99 + randn(80) * 0.05
ci = bootstrap_ci(y, yhat, metric="r2", level=0.95, n=800, seed=1)
point = ci.value
up = ci.hi - ci.value
down = ci.value - ci.lo
"#,
    );
    let up = num_of(&it, "up");
    let down = num_of(&it, "down");
    assert!(up >= 0.0 && down >= 0.0, "the interval must bracket the estimate");
    assert!(
        num_of(&it, "point") > 0.9,
        "the fixture must actually sit near the ceiling for the skew to exist"
    );
    assert!(
        down > up * 1.2,
        "a near-ceiling R2 must have a longer lower tail (down={down}, up={up}); \
         equal tails would mean the interval was built symmetrically rather than \
         from the resampling distribution"
    );
}
