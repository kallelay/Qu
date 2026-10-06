//! Behaviour pins for the v0.4.9 performance work: every optimised path must
//! give the answer the old path did.

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

fn num(it: &Interp, n: &str) -> f64 {
    match it.get(n) {
        Some(Value::Num(x)) => *x,
        Some(Value::Bool(b)) => f64::from(*b),
        other => panic!("`{n}` is {other:?}"),
    }
}

#[test]
fn inverse_of_a_random_matrix_is_accurate_across_the_block_size() {
    // 150 > the 64-column panel width, so the blocked update is exercised.
    let it = run(
        "seed(7)\nA = randn(150, 150)\nB = inv(A)\nE = A * B - eye(150)\nerr = max(abs(E))\n\
         x = randn(150)\ny = solve(A, A * x)\nserr = max(abs(y - x))\nd = det(A)\nd2 = det(B)\nprod = d * d2",
    );
    assert!(num(&it, "err") < 1e-9, "inv residual {}", num(&it, "err"));
    assert!(num(&it, "serr") < 1e-8, "solve error {}", num(&it, "serr"));
    assert!((num(&it, "prod") - 1.0).abs() < 1e-8, "det(A)*det(inv(A)) = {}", num(&it, "prod"));
}

#[test]
fn inverse_still_reports_singular_matrices() {
    let m = err("A = [1, 2; 2, 4]\nB = inv(A)");
    assert!(m.contains("singular"), "{m}");
    // numerically singular (rank-deficient to rounding): the SVD route decides
    let m = err("A = [1, 2, 3; 4, 5, 6; 7, 8, 9]\nB = inv(A)");
    assert!(m.contains("singular"), "{m}");
}

#[test]
fn lu_factors_reproduce_the_permuted_matrix() {
    let it = run(
        "seed(3)\nA = randn(100, 100)\nf = lu(A)\nE = f.p * A - f.l * f.u\nerr = max(abs(E))\n\
         lower = 0.0\nfor i = 0 to 98\n    for j = i + 1 to 99\n        lower = lower + abs(f.l[i, j]) + abs(f.u[j, i])\n    end for\nend for\n\
         unit = 0.0\nfor i = 0 to 99\n    unit = unit + abs(f.l[i, i] - 1)\nend for",
    );
    assert!(num(&it, "err") < 1e-12, "{}", num(&it, "err"));
    assert_eq!(num(&it, "lower"), 0.0);
    assert_eq!(num(&it, "unit"), 0.0);
}

#[test]
fn large_sort_and_unique_match_the_definition() {
    // above the parallel-sort threshold (65536); includes NaN and signed zeros
    let it = run(
        "seed(5)\nx = randn(200000)\nx[10] = nan\nx[11] = -0.0\nx[12] = 0.0\ns = sort(x)\n\
         n = len(s)\nok = true\nfor i = 1 to n - 2\n    if s[i - 1] > s[i]\n        ok = false\n    end if\nend for\n\
         last = s[n - 1]
last_is_nan = last != last\nu = unique(floor(x * 10))\nsorted_u = true\n\
         for i = 1 to len(u) - 2\n    if u[i - 1] >= u[i]\n        sorted_u = false\n    end if\nend for",
    );
    assert_eq!(num(&it, "ok"), 1.0);
    assert_eq!(num(&it, "last_is_nan"), 1.0);
    assert_eq!(num(&it, "sorted_u"), 1.0);
}

#[test]
fn filter_ba_matches_the_direct_recurrence() {
    let it = run(
        "b = [0.2, 0.3, 0.1, 0.05]\na = [1, -0.5, 0.25]\nx = [1, 2, 3, 4, 5, 6, 7, 8]\ny = filter_ba(b, a, x)\n\
         r = zeros(8)\nfor n = 0 to 7\n    acc = 0.0\n    for k = 0 to 3\n        if n - k >= 0\n            acc = acc + b[k] * x[n - k]\n        end if\n    end for\n\
         for k = 1 to 2\n        if n - k >= 0\n            acc = acc - a[k] * r[n - k]\n        end if\n    end for\n    r[n] = acc\nend for\n\
         err = max(abs(y - r))\nfir = filter_ba([0.5, 0.5], [1], x)\nfe = abs(fir[3] - 3.5) + abs(fir[0] - 0.5)",
    );
    assert!(num(&it, "err") < 1e-12, "{}", num(&it, "err"));
    assert!(num(&it, "fe") < 1e-15);
}

#[test]
fn matrix_cell_assignment_semantics_are_unchanged() {
    let it = run(
        "M = zeros(3, 3)\nM[1, 2] = 5\nN = M\nM[0, 0] = 7\ns1 = M[1, 2] + M[0, 0]\ns2 = N[0, 0]\nM[2, 2] += 3\ns3 = M[2, 2]\n\
         M[1, :] = [1, 2, 3]\ns4 = M[1, 0] + M[1, 1] + M[1, 2]",
    );
    assert_eq!(num(&it, "s1"), 12.0);
    assert_eq!(num(&it, "s2"), 0.0, "an alias must not see the later write");
    assert_eq!(num(&it, "s3"), 3.0);
    assert_eq!(num(&it, "s4"), 6.0);
    let m = err("M = zeros(2, 2)\nM[2, 0] = 1");
    assert!(m.contains("out of bounds") || m.contains("outside") || m.contains("index"), "{m}");
}

#[test]
fn user_function_calls_still_work_with_shared_definitions() {
    let it = run(
        "function f(a, b = 2)\n    return a * b\nend function\ng(x) := x + 1\ns = 0\nfor i = 1 to 100\n    s = s + f(i) + g(i)\nend for\n\
         function f(a, b = 3)\n    return a * b\nend function\nt = f(2)",
    );
    assert_eq!(num(&it, "s"), 2.0 * 5050.0 + 5050.0 + 100.0);
    assert_eq!(num(&it, "t"), 6.0, "a redefinition replaces the shared entry");
}
