//! § distributions (normcdf/norminv/tinv/chi2inv/...) and the multivariate
//! models (lda/qda/pls/ica), exercised through the language itself --
//! argument order, keyword names, method sugar and error text are what a
//! script sees, and the unit tests inside the modules cannot see them.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    match it.run(src) {
        Ok(()) => panic!("expected an error from:\n{src}"),
        Err(e) => e.to_string(),
    }
}

fn num(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

fn nums(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("`{name}` is {other:?}, expected numbers"),
    }
}

#[test]
fn critical_values_a_statistics_table_would_give() {
    let it = run(
        "z = norminv(0.975)\n\
         t = tinv(0.975, 10)\n\
         c = chi2inv(0.95, 1)\n\
         f = finv(0.95, 5, 10)\n\
         back = normcdf(z)\n\
         shifted = norminv(0.5, 10, 2)\n",
    );
    assert!((num(&it, "z") - 1.959963984540054).abs() < 1e-12);
    assert!((num(&it, "t") - 2.2281388519649385).abs() < 1e-9);
    assert!((num(&it, "c") - 3.841458820694124).abs() < 1e-9);
    assert!((num(&it, "f") - 3.325834530413011).abs() < 1e-8);
    assert!((num(&it, "back") - 0.975).abs() < 1e-14);
    assert_eq!(num(&it, "shifted"), 10.0);
}

#[test]
fn distribution_functions_broadcast_over_vectors() {
    let it = run("p = normcdf([-1, 0, 1])\nq = gaminv([0.1, 0.5, 0.9], 2, 3)\n");
    let p = nums(&it, "p");
    assert_eq!(p.len(), 3);
    assert!((p[1] - 0.5).abs() < 1e-15 && (p[0] + p[2] - 1.0).abs() < 1e-15);
    assert!(nums(&it, "q").windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn a_probability_outside_0_1_is_named_not_nan() {
    let msg = err("x = tinv(1.2, 5)\n");
    assert!(msg.contains("between 0 and 1") && msg.contains("1.2"), "{msg}");
    let msg = err("x = chi2inv(0.5, 0)\n");
    assert!(msg.contains("positive"), "{msg}");
}

const IRIS_LIKE: &str = "
X = [1.0, 1.2, 0.8, 1.1, 0.9, 5.0, 5.2, 4.8, 5.1, 4.9, 9.0, 9.2, 8.8, 9.1, 8.9, 2.0, 1.7, 2.3, 2.1, 1.9, 6.0, 6.3, 5.8, 5.6, 6.2, 1.0, 0.7, 1.3, 1.1, 0.8] as matrix(15, 2)
y = [10, 10, 10, 10, 10, 20, 20, 20, 20, 20, 30, 30, 30, 30, 30]
";

#[test]
fn lda_and_qda_predict_the_labels_they_were_given() {
    let it = run(&format!(
        "{IRIS_LIKE}\nlda = lda_model(X, y)\nqda = qda_model(X, y, reg=0.01)\n\
         a = lda.predict(X)\nb = qda.predict(X)\nz = lda.transform(X)\nn = ncol(z)\n"
    ));
    let y: Vec<f64> = [10.0; 5].into_iter().chain([20.0; 5]).chain([30.0; 5]).collect();
    assert_eq!(nums(&it, "a"), y);
    assert_eq!(nums(&it, "b"), y);
    assert_eq!(num(&it, "n"), 2.0, "3 classes, 2 features -> 2 discriminant axes");
}

#[test]
fn pls_recovers_an_exact_linear_relation() {
    let it = run(
        "X = [1, 2, 3, 4, 5, 0.5, 2, 1, 5, 3, 7, 0.2] as matrix(6, 2)\n\
         y = X[:, 0] * 2 - X[:, 1] * 3 + 1\n\
         m = pls_model(X, y, 2)\n\
         yhat = m.predict(X)\n\
         err = max(abs(yhat - y))\n\
         t = m.transform(X)\n",
    );
    assert!(num(&it, "err") < 1e-9, "err = {}", num(&it, "err"));
    assert_eq!(nums(&it, "t").len(), 12);
}

#[test]
fn ica_returns_unit_variance_sources_and_repeats_run_to_run() {
    let src = "
n = 1000
t = (0 to n - 1) * 0.01
s1 = sign(sin(t * 3))
s2 = ((t * 1.3) mod 2) - 1
# column-major: the first n values fill column 0, the next n column 1
X = [s1 + s2 * 0.6, s1 * 0.4 + s2] as matrix(n, 2)
S = ica(X, 2)
v = std(S[:, 0])
m = ica_model(X, 2, seed=3)
S2 = m.transform(X)
";
    let a = run(src);
    let b = run(src);
    assert!((num(&a, "v") - 1.0).abs() < 1e-2, "sources are scaled to unit variance: {}", num(&a, "v"));
    assert_eq!(nums(&a, "S"), nums(&b, "S"), "the default seed is fixed, so runs repeat");
}

#[test]
fn qda_names_reg_when_a_class_is_singular() {
    let msg = err(
        "X = [0, 1, 2, 5, 6, 7, 1, 1, 1, 3, 4, 6] as matrix(6, 2)\n\
         m = qda_model(X, [0, 0, 0, 1, 1, 1])\n",
    );
    assert!(msg.contains("reg="), "{msg}");
}

// ------------------------------------------------------------ native_call

#[test]
#[cfg(target_os = "linux")]
fn native_call_reaches_the_c_math_library() {
    let it = run(
        "lib = load_library(\"libm.so.6\")\n\
         c = lib.native_call(\"cos\", \"double(double)\", 0)\n\
         p = native_call(lib, \"pow\", \"double(double, double)\", 2, 10)\n",
    );
    assert_eq!(num(&it, "c"), 1.0);
    assert_eq!(num(&it, "p"), 1024.0);
}

#[test]
fn native_code_is_refused_in_the_sandbox() {
    let mut it = Interp::new();
    it.set_sandboxed(true);
    let msg = it.run("lib = load_library(\"libm.so.6\")\n").unwrap_err().to_string();
    assert!(msg.contains("load_library"), "{msg}");
}

#[test]
fn a_missing_library_or_function_is_a_named_error() {
    let msg = err("lib = load_library(\"/no/such/library.so\")\n");
    assert!(msg.contains("could not load") && msg.contains("/no/such/library.so"), "{msg}");
}

// ---- every family on the distribution chart (v0.4.4) ----

fn close(got: f64, want: f64, rel: f64, what: &str) {
    assert!(
        (got - want).abs() <= rel * want.abs().max(1e-300),
        "{what}: got {got:e}, want {want:e} (rel err {:e})",
        (got - want).abs() / want.abs().max(1e-300)
    );
}

/// Reference values from 50-digit mpmath / SciPy. The large-parameter
/// masses are the cases a difference of `lgamma`s gets wrong at 1e-12.
#[test]
fn discrete_masses_and_cdfs_are_full_precision() {
    let it = run(
        "a = binopdf(10, 1000, 0.01)\nb = binocdf(2, 1000, 0.01)\nc = poisspdf(519, 500)\n\
         d = poisscdf(481, 500)\ne = hygepdf(17, 1000, 300, 100)\nf = nbinpdf(199, 2.5, 0.05)\n\
         g = geocdf(3, 0.2)\nh = unidcdf(2.5, 6)\ni = berncdf(0.5, 0.3)\nj = binopdf(2.5, 10, 0.3)",
    );
    close(num(&it, "a"), 0.12574021112620742, 1e-14, "binopdf");
    close(num(&it, "b"), 0.00267943199379151, 3e-14, "binocdf");
    close(num(&it, "c"), 0.012258161463227256920, 1e-14, "poisspdf");
    close(num(&it, "d"), 0.20467861927485105389, 1e-14, "poisscdf");
    close(num(&it, "e"), 0.0007975737182073513, 1e-14, "hygepdf");
    close(num(&it, "f"), 4.396875865484616e-05, 1e-14, "nbinpdf");
    close(num(&it, "g"), 1.0 - 0.8f64.powi(4), 1e-15, "geocdf");
    assert_eq!(num(&it, "h"), 2.0 / 6.0);
    assert_eq!(num(&it, "i"), 0.7);
    assert_eq!(num(&it, "j"), 0.0, "a non-integer k has no mass");
}

#[test]
fn continuous_families_and_inverses_round_trip() {
    let it = run(
        "a = lognpdf(1.7734562180007067, 1.5, 0.3)\nb = logninv(logncdf(2.2, 0.4, 0.8), 0.4, 0.8)\n\
         c = wblcdf(0.7357883197815892, 2, 1.5)\nd = wblinv(0.3, 1, 0.5)\ne = unifinv(0.25, -2, 6)\n\
         f = unifpdf([-3, 0, 7], -2, 6)",
    );
    close(num(&it, "a"), 0.006328678134984466, 1e-14, "lognpdf");
    close(num(&it, "b"), 2.2, 1e-14, "logninv(logncdf)");
    close(num(&it, "c"), 0.2, 1e-14, "wblcdf");
    close(num(&it, "d"), 0.12721701563369794, 1e-14, "wblinv");
    assert_eq!(num(&it, "e"), 0.0);
    assert_eq!(nums(&it, "f"), vec![0.0, 0.125, 0.0]);
}

#[test]
fn discrete_inverse_is_the_smallest_k_reaching_p() {
    let it = run(
        "k = binoinv(0.95, 20, 0.3)\nlo = binocdf(k - 1, 20, 0.3)\nhi = binocdf(k, 20, 0.3)\n\
         back = poissinv(poisscdf(7, 3.5), 3.5)\ntop = poissinv(1, 3.5)\nh = hygeinv(0, 50, 10, 45)",
    );
    let k = num(&it, "k");
    assert!(num(&it, "lo") < 0.95 && num(&it, "hi") >= 0.95, "k = {k}");
    assert_eq!(num(&it, "back"), 7.0);
    assert!(num(&it, "top").is_infinite());
    assert_eq!(num(&it, "h"), 5.0, "the support starts at N + K - M");
}

#[test]
fn stat_results_carry_mean_and_var() {
    let it = run(
        "a = hygestat(50, 10, 12).var\nb = wblstat(2, 1.5).mean\nc = tstat(1).mean\nd = tstat(1.5).var\n\
         e = poisstat(3.5).var\nf = nbinstat(3, 0.4).mean",
    );
    close(num(&it, "a"), 1.4889795918367348, 1e-14, "hygestat");
    close(num(&it, "b"), 1.805490585901867, 1e-14, "wblstat");
    assert!(num(&it, "c").is_nan(), "the Cauchy mean does not exist");
    assert!(num(&it, "d").is_infinite());
    assert_eq!(num(&it, "e"), 3.5);
    close(num(&it, "f"), 4.5, 1e-15, "nbinstat");
}

#[test]
fn fits_find_the_likelihood_maximum() {
    // Weibull data: the fitted shape must zero its score equation -- the
    // defining property of the MLE -- to rounding.
    let it = run(
        "x = wblrnd(3, 1.8, 400, 1, seed = 5)\nw = wblfit(x)\nb = w.b\n\
         g = gamfit(gamrnd(2.2, 1.7, 3000, 1, seed = 9))\nga = g.a\ngb = g.b\n\
         n = normfit([1, 2, 3, 4, 10])\nmu = n.mu\nsd = n.sigma\nll = n.loglik\n\
         p = binofit([3, 5, 4], 10).p\nu = unifit([2, -1, 5]).params",
    );
    let x = nums(&it, "x");
    let b = num(&it, "b");
    let (s0, s1): (f64, f64) = x.iter().fold((0.0, 0.0), |(a, c), v| (a + v.powf(b), c + v.powf(b) * v.ln()));
    let mean_ln = x.iter().map(|v| v.ln()).sum::<f64>() / x.len() as f64;
    assert!((s1 / s0 - 1.0 / b - mean_ln).abs() < 1e-12, "Weibull score not zero at b = {b}");
    assert!((num(&it, "ga") - 2.2).abs() < 0.15 && (num(&it, "gb") - 1.7).abs() < 0.15);
    assert_eq!(num(&it, "mu"), 4.0);
    close(num(&it, "sd"), 12.5f64.sqrt(), 1e-15, "normfit sigma (n - 1)");
    assert!(num(&it, "ll").is_finite());
    assert_eq!(num(&it, "p"), 0.4);
    assert_eq!(nums(&it, "u"), vec![-1.0, 5.0]);
}

#[test]
fn samplers_are_seeded_shaped_and_right_on_average() {
    let it = run(
        "a = binornd(1000000, 0.3, 20000, 1, seed = 1)\nb = binornd(1000000, 0.3, 20000, 1, seed = 1)\n\
         m = mean(a)\nM = poissrnd(4, 3, 2)\nz = hygernd(50, 10, 12, 1, 1, seed = 3)\n\
         t = trnd(5, 1000, 1, seed = 2)",
    );
    assert_eq!(nums(&it, "a"), nums(&it, "b"), "same seed, same draws");
    // mean 300000, sd 458.3: 20000 draws put the sample mean within 4 se.
    assert!((num(&it, "m") - 300000.0).abs() < 4.0 * 458.26 / 20000f64.sqrt(), "{}", num(&it, "m"));
    assert!(matches!(it.get("M"), Some(Value::Mat(m)) if m.rows() == 3 && m.cols() == 2));
    let z = num(&it, "z");
    assert!(z.fract() == 0.0 && (0.0..=10.0).contains(&z));
    assert!(nums(&it, "t").iter().all(|v| v.is_finite()));
}

#[test]
fn family_errors_name_the_problem() {
    assert!(err("x = binopdf(1, 2.5, 0.3)").contains("n (trials) must be a whole number"));
    assert!(err("x = geopdf(1, 0)").contains("p must be in (0, 1]"));
    assert!(err("x = hygepdf(1, 10, 11, 3)").contains("cannot exceed M"));
    assert!(err("x = unifpdf(1, 3, 2)").contains("a < b"));
    assert!(err("x = poissinv(1.5, 2)").contains("between 0 and 1"));
    assert!(err("x = nbinfit([1, 1, 1, 2])").contains("not overdispersed"));
    assert!(err("x = gamfit([2, 2, 2])").contains("all 3 values are equal"));
    assert!(err("x = betafit([0.2, 1.0])").contains("strictly between 0 and 1"));
    assert!(err("x = normrnd(0)").contains("missing argument 2"));
}
