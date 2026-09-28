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
