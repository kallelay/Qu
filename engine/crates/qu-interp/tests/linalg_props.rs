//! Matrix-property builtins (v0.4.14), through the language: `eps`, `hilb`,
//! `invhilb`, `issymmetric`, `isposdef`, `isdiag`, `istril`, `istriu`,
//! `isbanded`, `bandwidth`, `rcond`, `svdvals` and the two-argument forms of
//! `norm` and `cond`.
//!
//! Expected values come from definitions and hand-built matrices (1/(i+j-1),
//! the closed-form inverse Hilbert matrix, a 2x2 norm formula, the identity
//! `norm1(A) * norm1(inv(A))`), never from the code under test.

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

fn truth(it: &Interp, n: &str) -> bool {
    match it.get(n) {
        Some(Value::Bool(b)) => *b,
        other => panic!("`{n}` is not a bool: {other:?}"),
    }
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
}

#[test]
fn eps_forms() {
    let it = run("a = eps()\nb = eps(1)\nc = eps(1e10)\nd = eps(0)\ne2 = eps(-8)\nv = eps([1, 2, 4])");
    assert_eq!(num(&it, "a"), 2.220446049250313e-16);
    assert_eq!(num(&it, "b"), 2.220446049250313e-16);
    assert_eq!(num(&it, "c"), 2.0f64.powi(-19));
    assert_eq!(num(&it, "d"), 5e-324);
    assert_eq!(num(&it, "e2"), 2.0f64.powi(-49));
    assert_eq!(nums(&it, "v"), vec![2.0f64.powi(-52), 2.0f64.powi(-51), 2.0f64.powi(-50)]);
    // `eps` stays usable as an ordinary variable name
    let it = run("eps = 1e-6\nx = eps * 2");
    assert_eq!(num(&it, "x"), 2e-6);
    assert!(err("eps(1, 2)").contains("eps"));
    assert!(err("eps(\"a\")").contains("eps"));
}

#[test]
fn hilbert_matrices() {
    let it = run("H = hilb(3)");
    let h = nums(&it, "H"); // column-major, symmetric
    for i in 0..3 {
        for j in 0..3 {
            assert_eq!(h[i + 3 * j], 1.0 / (i + j + 1) as f64);
        }
    }
    let it = run("W = invhilb(4)\nr0 = W[0, :]\nP = W * hilb(4)\nd = norm(P - eye(4))");
    assert_eq!(nums(&it, "r0"), vec![16.0, -120.0, 240.0, -140.0]);
    assert!(num(&it, "d") < 1e-11);
    let it = run("s = sum(sum(invhilb(12)))\nx = invhilb(1)");
    assert_eq!(num(&it, "s"), 144.0);
    assert_eq!(num(&it, "x"), 1.0);
    assert!(err("invhilb(13)").contains("2^53"));
    assert!(err("invhilb(0)").contains("invhilb"));
    assert!(err("hilb(2.5)").contains("whole"));
    assert!(err("hilb(0)").contains("hilb"));
    assert!(err("hilb()").contains("hilb"));
    assert!(err("hilb(3, 4)").contains("hilb"));
    assert!(err("hilb(\"a\")").contains("hilb"));
}

#[test]
fn symmetry_and_definiteness() {
    let it = run(
        "a = issymmetric([2, 1; 1, 3])\n\
         b = issymmetric([2, 1; 1.000001, 3])\n\
         c = issymmetric([2, 1; 1.000001, 3], 1e-4)\n\
         d = issymmetric([2, 1; 1.000001, 3], 1e-9)\n\
         e1 = issymmetric([1, 2, 3; 2, 1, 0])\n\
         f1 = isposdef([2, 1; 1, 3])\n\
         g = isposdef([1, 2; 2, 1])\n\
         h = isposdef([2, 5; 1, 3])\n\
         k = isposdef([1, 1; 1, 1])\n\
         m = isposdef(hilb(5))\n\
         B = [1, 2, 0; 0, 1, 3; 4, 0, 1]\n\
         n = isposdef(B' * B)",
    );
    assert!(truth(&it, "a") && !truth(&it, "b") && truth(&it, "c") && !truth(&it, "d"));
    assert!(!truth(&it, "e1"));
    assert!(truth(&it, "f1") && !truth(&it, "g") && !truth(&it, "h") && !truth(&it, "k"));
    assert!(truth(&it, "m") && truth(&it, "n"));
    assert!(err("isposdef([1, 2, 3; 2, 1, 0])").contains("square"));
    assert!(err("isposdef([1, 2; nan, 1])").contains("NaN"));
    assert!(err("issymmetric()").contains("issymmetric"));
    assert!(err("issymmetric([1, 2; 2, 1], -1)").contains("tol"));
    assert!(err("issymmetric([1, 2; 2, 1], \"x\")").contains("tol"));
    assert!(err("issymmetric(\"abc\")").contains("issymmetric"));
    assert!(err("isposdef([1, 2; 2, 1], 3)").contains("isposdef"));
}

#[test]
fn structure_predicates() {
    let it = run(
        "T = [2, -1, 0, 0; -1, 2, -1, 0; 0, -1, 2, -1; 0, 0, -1, 2]\n\
         bt = bandwidth(T)\n\
         lo = bt.lower\n\
         up = bt.upper\n\
         F = [1, 2, 3; 4, 5, 6; 7, 8, 9]\n\
         bf = bandwidth(F)\n\
         a = isbanded(T, 1, 1)\n\
         b = isbanded(T, 0, 1)\n\
         c = isbanded(F, 2, 2)\n\
         d = isbanded(F, 1, 2)\n\
         e1 = isdiag([1, 0; 0, 5])\n\
         f1 = isdiag(T)\n\
         g = istril([1, 0; 2, 3])\n\
         h = istril([1, 2; 0, 3])\n\
         k = istriu([1, 2; 0, 3])\n\
         m = istriu([1, 0; 2, 3])\n\
         n = isdiag(zeros(3, 3))\n\
         r = bandwidth([1, 0, 4; 0, 2, 0])",
    );
    assert_eq!((num(&it, "lo"), num(&it, "up")), (1.0, 1.0));
    match it.get("bf") {
        Some(Value::Record(r)) => {
            assert_eq!(r[0].0, "lower");
            assert_eq!(r[1].0, "upper");
            assert!(matches!(r[0].1, Value::Num(x) if x == 2.0));
            assert!(matches!(r[1].1, Value::Num(x) if x == 2.0));
        }
        other => panic!("{other:?}"),
    }
    assert!(truth(&it, "a") && !truth(&it, "b") && truth(&it, "c") && !truth(&it, "d"));
    assert!(truth(&it, "e1") && !truth(&it, "f1"));
    assert!(truth(&it, "g") && !truth(&it, "h") && truth(&it, "k") && !truth(&it, "m"));
    assert!(truth(&it, "n"));
    match it.get("r") {
        Some(Value::Record(r)) => assert!(matches!(r[1].1, Value::Num(x) if x == 2.0)),
        other => panic!("{other:?}"),
    }
    assert!(err("isbanded([1, 2; 3, 4], 1)").contains("isbanded"));
    assert!(err("isbanded([1, 2; 3, 4], -1, 1)").contains("lower"));
    assert!(err("isbanded([1, 2; 3, 4], 1, 1.5)").contains("upper"));
    assert!(err("bandwidth()").contains("bandwidth"));
    assert!(err("isdiag(\"x\")").contains("isdiag"));
}

#[test]
fn rcond_and_svdvals() {
    let it = run(
        "a = rcond(eye(4))\n\
         b = rcond([1, 2; 2, 4])\n\
         c = rcond(zeros(3, 3))\n\
         H = hilb(8)\n\
         d = rcond(H)\n\
         ref = 1 / (norm(H, 1) * norm(invhilb(8), 1))\n\
         e1 = rcond([4, 3; 6, 3])\n\
         s = svdvals(diag([3, 1, 2]))\n\
         A = [1, 2; 3, 4]\n\
         v = svdvals(A)\n\
         w = svd(A)\n\
         tall = svdvals([1, 0; 0, 2; 0, 0])",
    );
    assert_eq!(num(&it, "a"), 1.0);
    assert_eq!(num(&it, "b"), 0.0);
    assert_eq!(num(&it, "c"), 0.0);
    let (d, r) = (num(&it, "d"), num(&it, "ref"));
    assert!((d - r).abs() <= 1e-3 * r, "{d} vs {r}");
    assert!(d > 1e-12 && d < 1e-9, "{d}");
    // [4 3; 6 3]: norm1 = 10 (column 1), inv = [-0.5 0.5; 1 -2/3] has norm1 1.5 -> rcond = 1/15
    close(num(&it, "e1"), 1.0 / 15.0, 1e-14);
    assert_eq!(nums(&it, "s"), vec![3.0, 2.0, 1.0]);
    assert_eq!(nums(&it, "tall"), vec![2.0, 1.0]);
    let v = nums(&it, "v");
    assert!(v[0] > v[1]);
    close(v[0], 5.464985704219043, 1e-14);
    close(v[0] * v[1], 2.0, 1e-14); // |det|
    // agrees with the singular values `svd` returns
    let it2 = run("A = [1, 2; 3, 4]
v = svdvals(A)
w = svd(A).s");
    let (v2, w2) = (nums(&it2, "v"), nums(&it2, "w"));
    assert_eq!(v2.len(), w2.len());
    for (x, y) in v2.iter().zip(&w2) {
        close(*x, *y, 1e-14);
    }
    assert!(err("rcond([1, 2, 3; 4, 5, 6])").contains("square"));
    assert!(err("rcond([1, nan; 3, 4])").contains("NaN"));
    assert!(err("rcond()").contains("rcond"));
    assert!(err("rcond(eye(2), 1)").contains("rcond"));
    assert!(err("svdvals([1, nan; 3, 4])").contains("NaN"));
    assert!(err("svdvals()").contains("svdvals"));
}

#[test]
fn norm_with_p() {
    let it = run(
        "A = [1, 2; 3, 4]\n\
         n0 = norm(A)\n\
         n1 = norm(A, 1)\n\
         n2 = norm(A, 2)\n\
         ni = norm(A, inf)\n\
         nf = norm(A, \"fro\")\n\
         v = [3, -4, 0, 12]\n\
         v0 = norm(v)\n\
         v1 = norm(v, 1)\n\
         v2 = norm(v, 2)\n\
         vi = norm(v, inf)\n\
         vm = norm(v, -inf)\n\
         vz = norm(v, 0)\n\
         v3 = norm(v, 3)\n\
         vf = norm(v, \"fro\")\n\
         s = norm(-7, 1)\n\
         c = norm([1; 2; 2], 2)",
    );
    close(num(&it, "n0"), 30.0f64.sqrt(), 1e-15);
    assert_eq!(num(&it, "n1"), 6.0);
    close(num(&it, "n2"), 5.464985704219043, 1e-14);
    assert_eq!(num(&it, "ni"), 7.0);
    close(num(&it, "nf"), 5.477225575051661, 1e-15);
    // one-argument norm unchanged: matrix is Frobenius, vector is 2-norm
    assert_eq!(num(&it, "v0"), 13.0);
    assert_eq!(num(&it, "v1"), 19.0);
    assert_eq!(num(&it, "v2"), 13.0);
    assert_eq!(num(&it, "vi"), 12.0);
    assert_eq!(num(&it, "vm"), 0.0);
    assert_eq!(num(&it, "vz"), 3.0);
    close(num(&it, "v3"), 1819.0f64.powf(1.0 / 3.0), 1e-14);
    assert_eq!(num(&it, "vf"), 13.0);
    assert_eq!(num(&it, "s"), 7.0);
    assert_eq!(num(&it, "c"), 3.0);
    assert!(err("norm([1, 2; 3, 4], 3)").contains("matrix norm"));
    assert!(err("norm([1, 2; 3, 4], 0)").contains("matrix norm"));
    assert!(err("norm([1, 2, 3], 0.5)").contains("p >= 1"));
    assert!(err("norm([1, 2, 3], \"nuc\")").contains("unknown norm"));
    assert!(err("norm([1, 2, 3], [1, 2])").contains("p must be"));
    assert!(err("norm([1, nan], 2)").contains("NaN"));
    assert!(err("norm([1, 2; 3, 4], 1, 2)").contains("norm"));
    assert!(err("norm([1, 2], p=1)").contains("p"));
}

#[test]
fn one_argument_norm_and_cond_are_unchanged() {
    // The pre-existing one-argument results, bit for bit: Frobenius for a
    // matrix, 2-norm for a vector, 2-norm condition number.
    let it = run("a = norm([1, 2; 3, 4])\nb = norm([3, 4])\nc = cond([1, 2; 3, 4])\nd = cond(eye(3))");
    assert_eq!(num(&it, "a"), 30.0f64.sqrt());
    assert_eq!(num(&it, "b"), 5.0);
    close(num(&it, "c"), 14.933034373659268, 1e-13);
    assert_eq!(num(&it, "d"), 1.0);
    assert!(err("norm()").contains("at least one argument"));
}

#[test]
fn cond_with_p() {
    let it = run(
        "A = [1, 2; 3, 4]\n\
         c1 = cond(A, 1)\n\
         c2 = cond(A, 2)\n\
         ci = cond(A, inf)\n\
         cf = cond(A, \"fro\")\n\
         c0 = cond(A)\n\
         cs = cond([1, 2; 2, 4], 1)\n\
         ct = cond([1, 2, 3; 4, 5, 6], 2)",
    );
    // inv(A) = [-2 1; 1.5 -0.5]
    close(num(&it, "c1"), 21.0, 1e-13);
    close(num(&it, "ci"), 21.0, 1e-13);
    close(num(&it, "cf"), 15.0, 1e-13);
    close(num(&it, "c2"), num(&it, "c0"), 1e-13);
    assert_eq!(num(&it, "cs"), f64::INFINITY);
    assert!(num(&it, "ct") > 1.0 && num(&it, "ct").is_finite());
    assert!(err("cond([1, 2, 3; 4, 5, 6], 1)").contains("square"));
    assert!(err("cond([1, 2; 3, 4], 3)").contains("p must be"));
    assert!(err("cond([1, 2; 3, 4], \"nuc\")").contains("unknown norm"));
    assert!(err("cond([1, nan; 3, 4], 1)").contains("NaN"));
    assert!(err("cond([1, 2; 3, 4], 1, 2)").contains("cond"));
}
