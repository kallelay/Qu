//! ML and data-prep builtins (v0.4.15), through the language: `silhouette`,
//! `silhouette_score`, `learning_curve`, `one_hot`, `label_encode`,
//! `label_decode`, `fillna`, `impute` and `sortperm`.
//!
//! Every expected value is worked out by hand in a comment next to it (or
//! is a property that holds by definition), never read back from the code
//! under test.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

fn nums(it: &Interp, n: &str) -> Vec<f64> {
    match it.get(n) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Num(x)) => vec![*x],
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    nums(it, n)[0]
}

fn strs(it: &Interp, n: &str) -> Vec<String> {
    match it.get(n) {
        Some(Value::List(l)) => l
            .iter()
            .map(|v| match v {
                Value::Str(s) => s.clone(),
                other => panic!("`{n}` holds {other:?}"),
            })
            .collect(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn shape(it: &Interp, n: &str) -> (usize, usize) {
    match it.get(n) {
        Some(Value::Mat(m)) => (m.rows(), m.cols()),
        other => panic!("`{n}` is not a matrix: {other:?}"),
    }
}

fn close_all(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= 1e-12 * w.abs().max(1.0), "{got:?} vs {want:?}");
    }
}

// ------------------------------------------------------------- silhouette

#[test]
fn silhouette_1d_by_hand() {
    // X = [0, 1, 10, 11], clusters {0, 1} and {10, 11}.
    //   x=0 : a = 1, b = (10 + 11) / 2 = 10.5, s = 9.5 / 10.5
    //   x=1 : a = 1, b = (9 + 10) / 2  = 9.5,  s = 8.5 / 9.5
    //   x=10: a = 1, b = (10 + 9) / 2  = 9.5,  s = 8.5 / 9.5
    //   x=11: a = 1, b = (11 + 10) / 2 = 10.5, s = 9.5 / 10.5
    let it = run("s = silhouette([0, 1, 10, 11], [0, 0, 1, 1])\nm = silhouette_score([0, 1, 10, 11], [0, 0, 1, 1])");
    let (p, q) = (9.5 / 10.5, 8.5 / 9.5);
    close_all(&nums(&it, "s"), &[p, q, q, p]);
    assert!((num(&it, "m") - (2.0 * p + 2.0 * q) / 4.0).abs() < 1e-12);
}

#[test]
fn silhouette_2d_by_hand() {
    // A = {(0,0), (2,0)}, B = {(0,5), (2,5)}. For (0,0): a = 2,
    // b = (5 + sqrt(2^2 + 5^2)) / 2, s = (b - a) / b = 1 - 2 / b.
    // The four points are mirror images of each other, so all four agree.
    let it = run("X = [0, 0; 2, 0; 0, 5; 2, 5]\ns = silhouette(X, [1, 1, 2, 2])");
    let b = (5.0 + 29.0f64.sqrt()) / 2.0;
    close_all(&nums(&it, "s"), &[1.0 - 2.0 / b; 4]);
}

#[test]
fn silhouette_singleton_scores_zero() {
    // X = [0, 1, 10], labels [0, 0, 1]: the point at 10 is alone (0).
    //   x=0: a = 1, b = 10, s = 0.9        x=1: a = 1, b = 9, s = 8 / 9
    let it = run("s = silhouette([0, 1, 10], [0, 0, 1])");
    close_all(&nums(&it, "s"), &[0.9, 8.0 / 9.0, 0.0]);
}

#[test]
fn silhouette_perfect_overlap_and_mislabelled() {
    // well separated: near 1
    let it = run("m = silhouette_score([0, 0.1, 0.2, 1000, 1000.1, 1000.2], [0, 0, 0, 1, 1, 1])");
    assert!(num(&it, "m") > 0.999, "{}", num(&it, "m"));

    // A = {0, 1, 2}, B = {1, 2, 3} (overlapping). Per point (a, b):
    //   A: x=0 (1.5, 2) -> 0.25;  x=1 (1, 1) -> 0;  x=2 (1.5, 2/3) -> -5/9
    //   B: x=1 (1.5, 2/3) -> -5/9; x=2 (1, 1) -> 0; x=3 (1.5, 2) -> 0.25
    // mean = (0.5 - 10/9) / 6 = -11/108, near 0.
    let it = run("m = silhouette_score([0, 1, 2, 1, 2, 3], [0, 0, 0, 1, 1, 1])");
    assert!((num(&it, "m") + 11.0 / 108.0).abs() < 1e-12, "{}", num(&it, "m"));

    // the right clusters {0, 1} {10, 11} labelled crosswise: {0, 10} {1, 11}.
    //   x=0 : a = 10, b = (1 + 11) / 2 = 6 -> -0.4     x=1 : a = 10, b = (0+... = 5 -> -0.5
    //   x=10: a = 10, b = (9 + 1) / 2 = 5 -> -0.5      x=11: a = 10, b = 6 -> -0.4
    let it = run("s = silhouette([0, 1, 10, 11], [0, 1, 0, 1])");
    close_all(&nums(&it, "s"), &[-0.4, -0.5, -0.5, -0.4]);
}

#[test]
fn silhouette_string_labels_match_numeric() {
    let it = run(
        "a = silhouette([0, 1, 10, 11], (\"cat\", \"cat\", \"dog\", \"dog\"))\nb = silhouette([0, 1, 10, 11], [0, 0, 1, 1])",
    );
    assert_eq!(nums(&it, "a"), nums(&it, "b"));
}

#[test]
fn silhouette_errors_in_words() {
    assert!(err("silhouette([0, 1, 2], [5, 5, 5])").contains("at least 2 clusters"));
    assert!(err("silhouette_score([0, 1, 2, 3], [0, 0, 1])").contains("4 rows but labels has 3"));
    assert!(err("silhouette([0, nan, 10, 11], [0, 0, 1, 1])").contains("NaN"));
    assert!(err("silhouette([0, 1, 2], [0, 1, 2])").contains("own cluster"));
    assert!(err("silhouette([0, 1, 10, 11])").contains("missing argument"));
}

// ------------------------------------------------- one_hot, label_encode

#[test]
fn one_hot_numbers_by_hand() {
    // labels [3, 1, 2, 1] -> classes [1, 2, 3]; row i has a 1 in the column
    // of its class. Column-major: col 1 = [0,1,0,1], col 2 = [0,0,1,0],
    // col 3 = [1,0,0,0].
    let it = run("r = one_hot([3, 1, 2, 1])\nm = r.matrix\nc = r.classes");
    assert_eq!(shape(&it, "m"), (4, 3));
    assert_eq!(nums(&it, "m"), vec![0., 1., 0., 1., 0., 0., 1., 0., 1., 0., 0., 0.]);
    assert_eq!(nums(&it, "c"), vec![1.0, 2.0, 3.0]);
}

#[test]
fn one_hot_strings_by_hand() {
    // ("b","a","c","a") -> classes a, b, c
    let it = run("r = one_hot((\"b\", \"a\", \"c\", \"a\"))\nm = r.matrix\nc = r.classes");
    assert_eq!(shape(&it, "m"), (4, 3));
    assert_eq!(nums(&it, "m"), vec![0., 1., 0., 1., 1., 0., 0., 0., 0., 0., 1., 0.]);
    assert_eq!(strs(&it, "c"), vec!["a", "b", "c"]);
}

#[test]
fn label_encode_decode_round_trip() {
    let it = run(
        "e = label_encode([30, 10, 20, 10])\ncodes = e.codes\ncls = e.classes\nback = label_decode(codes, cls)",
    );
    assert_eq!(nums(&it, "codes"), vec![2., 0., 1., 0.]);
    assert_eq!(nums(&it, "cls"), vec![10., 20., 30.]);
    assert_eq!(nums(&it, "back"), vec![30., 10., 20., 10.]);

    let it = run(
        "e = label_encode((\"pear\", \"fig\", \"apple\", \"fig\"))\ncodes = e.codes\ncls = e.classes\nback = label_decode(codes, cls)",
    );
    assert_eq!(nums(&it, "codes"), vec![2., 1., 0., 1.]);
    assert_eq!(strs(&it, "cls"), vec!["apple", "fig", "pear"]);
    assert_eq!(strs(&it, "back"), vec!["pear", "fig", "apple", "fig"]);

    // decoding a one_hot matrix row by row recovers the labels too
    let it = run("r = one_hot([5, 7, 5, 9])\nm = r.matrix\nc = r.classes");
    let (m, c) = (nums(&it, "m"), nums(&it, "c"));
    let decoded: Vec<f64> = (0..4).map(|i| c[(0..3).position(|j| m[j * 4 + i] == 1.0).unwrap()]).collect();
    assert_eq!(decoded, vec![5., 7., 5., 9.]);
}

#[test]
fn label_errors_in_words() {
    assert!(err("one_hot([1, nan, 2])").contains("NaN"));
    assert!(err("one_hot((\"a\", 1))").contains("mixes strings"));
    assert!(err("label_decode([0, 3], [10, 20, 30])").contains("outside 0..2"));
    assert!(err("label_decode([0.5], [10, 20])").contains("not a whole number"));
    assert!(err("label_decode([-1], [10, 20])").contains("outside"));
    assert!(err("one_hot()").contains("missing argument"));
}

// ---------------------------------------------------------- fillna, impute

#[test]
fn fillna_vector_and_matrix() {
    let it = run("a = fillna([1, nan, 3, nan], 0)\nb = fillna([inf, nan, -2], 7)");
    assert_eq!(nums(&it, "a"), vec![1., 0., 3., 0.]);
    assert_eq!(nums(&it, "b"), vec![f64::INFINITY, 7., -2.]);

    // column-major: col 0 = [1, NaN, 5], col 1 = [NaN, 4, 6]
    let it = run("M = [1, nan; nan, 4; 5, 6]\na = fillna(M, 9)\nb = fillna(M, [10, 20])");
    assert_eq!(nums(&it, "a"), vec![1., 9., 5., 9., 4., 6.]);
    assert_eq!(nums(&it, "b"), vec![1., 10., 5., 20., 4., 6.]);
}

#[test]
fn fillna_errors_in_words() {
    assert!(err("fillna([1, nan], [1, 2])").contains("single number"));
    assert!(err("fillna([1, nan; 2, 3], [1, 2, 3])").contains("2 columns but value has 3"));
    assert!(err("fillna([1, nan])").contains("missing argument"));
}

#[test]
fn impute_mean_median_by_hand() {
    // observed values of [1, _, 2, _, 10] are 1, 2, 10: mean 13/3, median 2 (odd count)
    let it = run("a = impute([1, nan, 2, nan, 10], \"mean\")\nb = impute([1, nan, 2, nan, 10], \"median\")");
    let m = 13.0 / 3.0;
    close_all(&nums(&it, "a"), &[1., m, 2., m, 10.]);
    assert_eq!(nums(&it, "b"), vec![1., 2., 2., 2., 10.]);
    // observed values of [1, _, 2, _, 3, 10] are 1, 2, 3, 10: mean 4, median (2 + 3) / 2 = 2.5 (even count)
    let it = run("a = impute([1, nan, 2, nan, 3, 10], \"mean\")\nb = impute([1, nan, 2, nan, 3, 10], \"median\")");
    assert_eq!(nums(&it, "a"), vec![1., 4., 2., 4., 3., 10.]);
    assert_eq!(nums(&it, "b"), vec![1., 2.5, 2., 2.5, 3., 10.]);
}

#[test]
fn impute_most_frequent_and_zero() {
    // 7 occurs twice, 1 and 2 once: 7 wins
    let it = run("a = impute([7, 7, nan, 1, 2], \"most_frequent\")");
    assert_eq!(nums(&it, "a"), vec![7., 7., 7., 1., 2.]);
    // tie rule: 2 and 5 both occur twice, the SMALLEST (2) is used
    let it = run("a = impute([5, 2, nan, 2, 5, 9], \"most_frequent\")");
    assert_eq!(nums(&it, "a"), vec![5., 2., 2., 2., 5., 9.]);
    let it = run("a = impute([4, nan, 6], \"zero\")");
    assert_eq!(nums(&it, "a"), vec![4., 0., 6.]);
}

#[test]
fn impute_matrix_is_column_wise() {
    // col 0 observed 1, 3, 5 (mean 3); col 1 observed 10, 20 (mean 15); col 2 has no NaN
    let it = run("M = [1, nan, 7; nan, 10, 8; 3, 20, 9; 5, nan, 6]\na = impute(M, \"mean\")");
    assert_eq!(nums(&it, "a"), vec![1., 3., 3., 5., 15., 10., 20., 15., 7., 8., 9., 6.]);
    // a matrix with nothing missing is returned unchanged
    let it = run("a = impute([1, 2; 3, 4], \"mean\")");
    assert_eq!(nums(&it, "a"), vec![1., 3., 2., 4.]);
}

#[test]
fn impute_errors_in_words() {
    let m = err("impute([1, nan; 2, nan], \"mean\")");
    assert!(m.contains("column 2") && m.contains("no observed values"), "{m}");
    assert!(err("impute([nan, nan], \"median\")").contains("no observed values"));
    assert!(err("impute([nan, nan], \"most_frequent\")").contains("no observed values"));
    // zero needs no observed values
    let it = run("a = impute([nan, nan], \"zero\")");
    assert_eq!(nums(&it, "a"), vec![0., 0.]);
    assert!(err("impute([1, nan], \"mode\")").contains("unknown strategy"));
    assert!(err("impute([1, nan], 3)").contains("strategy must be a string"));
}

// ---------------------------------------------------------------- sortperm

#[test]
fn sortperm_is_stable_by_hand() {
    // x = [3, 1, 2, 1, 3, 1]
    // ascending: the 1s (indices 1, 3, 5 in original order), then 2 (2), then the 3s (0, 4)
    // descending: the 3s (0, 4), then 2 (2), then the 1s (1, 3, 5): ties keep original order
    let it = run("x = [3, 1, 2, 1, 3, 1]\na = sortperm(x)\nb = sortperm(x, true)\nc = argsort(x, true)");
    assert_eq!(nums(&it, "a"), vec![1., 3., 5., 2., 0., 4.]);
    assert_eq!(nums(&it, "b"), vec![0., 4., 2., 1., 3., 5.]);
    // the older argsort(x, true) just reverses the ascending order, so it reverses ties
    assert_eq!(nums(&it, "c"), vec![4., 0., 2., 5., 3., 1.]);
}

#[test]
fn sortperm_strings_nan_and_zero() {
    let it = run("a = sortperm((\"b\", \"a\", \"b\", \"a\"))\nb = sortperm((\"b\", \"a\", \"b\", \"a\"), true)");
    assert_eq!(nums(&it, "a"), vec![1., 3., 0., 2.]);
    assert_eq!(nums(&it, "b"), vec![0., 2., 1., 3.]);
    // NaN counts as the largest: last ascending, first descending
    let it = run("a = sortperm([2, nan, 1])\nb = sortperm([2, nan, 1], true)");
    assert_eq!(nums(&it, "a"), vec![2., 0., 1.]);
    assert_eq!(nums(&it, "b"), vec![1., 0., 2.]);
    // -0 and 0 are equal, so they keep their original order
    let it = run("a = sortperm([0, -0, 0, -1])");
    assert_eq!(nums(&it, "a"), vec![3., 0., 1., 2.]);
    // the permutation sorts: x[p] is non-decreasing
    let it = run("x = [0.5, -2, 9, 0.5, 3]\np = sortperm(x)\ny = x[p]");
    assert_eq!(nums(&it, "y"), vec![-2., 0.5, 0.5, 3., 9.]);
}

#[test]
fn sortperm_errors_in_words() {
    assert!(err("sortperm([3, 1], \"yes\")").contains("desc must be true or false"));
    assert!(err("sortperm([1, 2; 3, 4])").contains("matrix"));
    assert!(err("sortperm([1, 2], true, 5)").contains("sortperm"));
}

// ----------------------------------------------------------- learning_curve

const LINE: &str = "x = linspace(0, 10, 60)\ny = 3 * x + 1\n";

#[test]
fn learning_curve_by_hand_deterministic() {
    // x = 0..9, y = x, 1-nearest-neighbour, 2 folds, no shuffle: fold 0 tests
    // rows 0-4 and trains on rows 5-9, fold 1 the reverse; m = 5 rows.
    // sizes [2, 5] are row counts (2 > 1). SS_tot of five consecutive values = 10.
    //   fold 0, size 2 (train rows 5, 6): every test row's neighbour is 5,
    //     residuals 5,4,3,2,1 -> SS 55 -> R2 = 1 - 55/10 = -4.5
    //   fold 0, size 5: neighbour still 5 -> -4.5
    //   fold 1, size 2 (train rows 0, 1): neighbour is 1 for rows 5..9,
    //     residuals 4..8 -> SS 190 -> -18
    //   fold 1, size 5 (train rows 0..4): neighbour 4, residuals 1..5 -> -4.5
    // training rows are always predicted by themselves: R2 = 1.
    let it = run(
        "x = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]\ny = x\nr = learning_curve(knn_model, x, y, 1, sizes = [2, 5], folds = 2, shuffle = false)\n\
         v = r.val_scores\nt = r.train_scores\nsz = r.sizes\nvm = r.val_mean\nvs = r.val_std\ntm = r.train_mean",
    );
    assert_eq!(nums(&it, "sz"), vec![2., 5.]);
    // column-major: [s0f0, s1f0, s0f1, s1f1]
    close_all(&nums(&it, "v"), &[-4.5, -4.5, -18., -4.5]);
    close_all(&nums(&it, "t"), &[1., 1., 1., 1.]);
    // mean over folds: (-4.5 - 18) / 2 = -11.25 and -4.5; population std 6.75 and 0
    close_all(&nums(&it, "vm"), &[-11.25, -4.5]);
    close_all(&nums(&it, "vs"), &[6.75, 0.0]);
    close_all(&nums(&it, "tm"), &[1., 1.]);
}

#[test]
fn learning_curve_ols_is_exact_on_noiseless_line() {
    // y = 3x + 1 exactly, so an OLS fit on any 2 or more points is exact:
    // both scores are R2 = 1 at every size.
    let it = run(&format!("{LINE}r = learning_curve(ols_model, x, y, seed = 4)\ntm = r.train_mean\nvm = r.val_mean\nvs = r.val_std"));
    close_all(&nums(&it, "tm"), &[1.0; 5]);
    close_all(&nums(&it, "vm"), &[1.0; 5]);
    for s in nums(&it, "vs") {
        assert!(s < 1e-9, "{s}");
    }
}

#[test]
fn learning_curve_validation_rises_with_data() {
    // 1-NN memorises the training rows (train R2 = 1) but interpolates a
    // line more crudely the fewer points it has: validation climbs.
    let it = run(&format!(
        "{LINE}r = learning_curve(knn_model, x, y, 1, seed = 11)\ntm = r.train_mean\nvm = r.val_mean\nsz = r.sizes"
    ));
    let (tm, vm, sz) = (nums(&it, "tm"), nums(&it, "vm"), nums(&it, "sz"));
    // default sizes 0.2 .. 1.0 of the smallest training fold (60 - 12 = 48 rows)
    assert_eq!(sz, vec![10., 19., 29., 38., 48.]);
    for t in &tm {
        assert!((t - 1.0).abs() < 1e-12, "{tm:?}");
    }
    assert!(vm[4] > vm[0], "validation did not improve: {vm:?}");
    assert!(vm[4] > 0.99, "{vm:?}");
    assert!(vm[0] < vm[4] - 1e-4, "{vm:?}");
}

#[test]
fn learning_curve_shapes_sizes_and_seeding() {
    let it = run(&format!(
        "{LINE}r = learning_curve(knn_model, x, y, 1, sizes = [0.5, 1.0], folds = 4, seed = 5)\n\
         tsc = r.train_scores\nvsc = r.val_scores\nsz = r.sizes\nf = r.folds\nm = r.metric\nts = r.train_std\nvm = r.val_mean"
    ));
    // m = 60 - 15 = 45 rows: 0.5 -> round(22.5) = 23 (round half away from zero), 1.0 -> 45
    assert_eq!(nums(&it, "sz"), vec![23., 45.]);
    assert_eq!(shape(&it, "tsc"), (2, 4));
    assert_eq!(shape(&it, "vsc"), (2, 4));
    assert_eq!(num(&it, "f"), 4.0);
    assert_eq!(nums(&it, "ts").len(), 2);
    assert!(matches!(it.get("m"), Some(Value::Str(s)) if s == "score"));
    // explicit row counts
    let it = run(&format!("{LINE}r = learning_curve(ols_model, x, y, sizes = [5, 20, 48])\nsz = r.sizes"));
    assert_eq!(nums(&it, "sz"), vec![5., 20., 48.]);
    // a named metric
    let it = run(&format!("{LINE}r = learning_curve(ols_model, x, y, metric = \"rmse\", seed = 1)\nvm = r.val_mean\nm = r.metric"));
    assert!(nums(&it, "vm").iter().all(|v| v.abs() < 1e-9));
    assert!(matches!(it.get("m"), Some(Value::Str(s)) if s == "rmse"));

    // same seed, bit-identical; different seed, different folds
    let src = |seed: u32| {
        format!("{LINE}r = learning_curve(knn_model, x, y, 1, seed = {seed})\nv = r.val_scores\nt = r.train_scores")
    };
    let bits = |it: &Interp| nums(it, "v").iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    let (a, b, c) = (run(&src(7)), run(&src(7)), run(&src(8)));
    assert_eq!(bits(&a), bits(&b));
    assert_ne!(bits(&a), bits(&c));
}

#[test]
fn learning_curve_works_with_a_user_function_and_matrix_x() {
    let it = run(
        "function fit(X, y, k)\n    return knn_model(X, y, k)\nend function\n\
         X = [0, 0; 1, 1; 2, 2; 3, 3; 4, 4; 5, 5; 6, 6; 7, 7; 8, 8; 9, 9]\ny = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]\n\
         r = learning_curve(fit, X, y, 1, sizes = [2, 5], folds = 2, shuffle = false)\nv = r.val_scores",
    );
    close_all(&nums(&it, "v"), &[-4.5, -4.5, -18., -4.5]);
}

#[test]
fn learning_curve_errors_in_words() {
    let pre = LINE;
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, sizes = [100])")).contains("larger than"));
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, sizes = [0.5, 10.5])")).contains("mixes fractions"));
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, sizes = [0])")).contains("positive"));
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, folds = 1)")).contains("folds=1"));
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, metric = \"nope\")")).contains("unknown metric"));
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y[1:10])")).contains("rows"));
    assert!(err("learning_curve(ols_model)").contains("needs a fit function"));
    // an unread keyword is refused, not ignored
    assert!(err(&format!("{pre}learning_curve(ols_model, x, y, sizess = [0.5])")).contains("sizess"));
    assert!(err("silhouette([0, 1, 10, 11], [0, 0, 1, 1], bogus = 1)").contains("bogus"));
}
