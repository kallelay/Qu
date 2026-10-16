//! Behaviour pins for the v0.4.10 blocked `qr` / `svd` / `eig`: through the
//! language, at sizes above the thresholds where the blocked kernels take
//! over, the answers must satisfy the decomposition identities and the
//! error behaviour must be what it was.

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
fn qr_of_large_square_tall_and_wide_matrices() {
    let it = run(
        "seed(11)\n\
         A = randn(220, 220)\nf = qr(A)\ne1 = max(abs(A - f.q * f.r))\no1 = max(abs(transpose(f.q) * f.q - eye(220)))\n\
         B = randn(400, 160)\ng = qr(B)\ne2 = max(abs(B - g.q * g.r))\nn2 = len(g.q[0, :])\n\
         C = randn(160, 400)\nh = qr(C)\ne3 = max(abs(C - h.q * h.r))\n\
         dmin = 1.0\nfor i = 0 to 219\n    dmin = min(dmin, f.r[i, i])\nend for\n\
         low = 0.0\nfor i = 1 to 219\n    for j = 0 to i - 1\n        low = low + abs(f.r[i, j])\n    end for\nend for",
    );
    assert!(num(&it, "e1") < 1e-12, "qr reconstruction {}", num(&it, "e1"));
    assert!(num(&it, "o1") < 1e-12, "Q orthogonality {}", num(&it, "o1"));
    assert!(num(&it, "e2") < 1e-12 && num(&it, "e3") < 1e-12);
    assert_eq!(num(&it, "n2"), 160.0, "thin Q has min(m, n) columns");
    assert!(num(&it, "dmin") >= 0.0, "R keeps a non-negative diagonal");
    assert_eq!(num(&it, "low"), 0.0, "R is upper triangular");
}

#[test]
fn svd_of_large_matrices_reconstructs_and_is_ordered() {
    let it = run(
        "seed(5)\n\
         A = randn(210, 210)\ns = svd(A)\n\
         e = max(abs(A - s.u * diag(s.s) * s.vt))\n\
         ou = max(abs(transpose(s.u) * s.u - eye(210)))\n\
         ov = max(abs(s.vt * transpose(s.vt) - eye(210)))\n\
         ordered = 1\nfor i = 1 to 209\n    if s.s[i] > s.s[i - 1]\n        ordered = 0\n    end if\nend for\n\
         B = randn(300, 140)\nt = svd(B)\neb = max(abs(B - t.u * diag(t.s) * t.vt))\n\
         W = randn(140, 300)\nw = svd(W)\newide = max(abs(W - w.u * diag(w.s) * w.vt))",
    );
    assert!(num(&it, "e") < 1e-11, "svd reconstruction {}", num(&it, "e"));
    assert!(num(&it, "ou") < 1e-12 && num(&it, "ov") < 1e-12);
    assert_eq!(num(&it, "ordered"), 1.0, "singular values must be descending");
    assert!(num(&it, "eb") < 1e-11 && num(&it, "ewide") < 1e-11);
}

#[test]
fn symmetric_eig_gives_orthonormal_eigenvectors_that_satisfy_av_eq_vd() {
    let it = run(
        "seed(2)\nM = randn(200, 200)\nS = M + transpose(M)\ne = eig(S)\n\
         V = e.vectors\nlam = e.values\n\
         r = max(abs(S * V - V * diag(lam)))\n\
         o = max(abs(transpose(V) * V - eye(200)))\n\
         tr = abs(sum(lam) - sum(diag(S)))",
    );
    assert!(num(&it, "r") < 1e-11, "A V - V D {}", num(&it, "r"));
    assert!(num(&it, "o") < 1e-12, "V orthogonality {}", num(&it, "o"));
    assert!(num(&it, "tr") < 1e-9);
}

#[test]
fn general_eig_reports_eigenvalues_only_and_they_are_right() {
    let it = run(
        "seed(4)\nA = randn(150, 150)\ne = eig(A)\nlam = e.values\n\
         tr = abs(real(sum(lam)) - sum(diag(A)))\n\
         tr2 = abs(real(sum(lam .* lam)) - sum(diag(A * A)))\n\
         nv = e.vectors == none\n\
         T = randn(120, 120)\nfor i = 1 to 119\n    for j = 0 to i - 1\n        T[i, j] = 0\n    end for\nend for\ntl = sort(real(eig(T).values))\ntd = sort(diag(T))\nterr = max(abs(tl - td))",
    );
    assert!(num(&it, "tr") < 1e-9, "trace {}", num(&it, "tr"));
    assert!(num(&it, "tr2") < 1e-7, "trace(A^2) {}", num(&it, "tr2"));
    assert_eq!(num(&it, "nv"), 1.0, "a non-symmetric matrix has no eigenvectors");
    assert!(num(&it, "terr") < 1e-9, "triangular matrix eigenvalues are its diagonal: {}", num(&it, "terr"));
}

#[test]
fn eig_of_a_cyclic_permutation_terminates_with_the_roots_of_unity() {
    // The nalgebra Schur iteration this replaces never terminates on this input.
    let it = run(
        "n = 64\nP = zeros(n, n)\nfor i = 0 to n - 1\n    P[(i + 1) mod n, i] = 1\nend for\n\
         e = eig(P)\nlam = e.values\nm = max(abs(abs(lam) - 1))",
    );
    assert!(num(&it, "m") < 1e-8, "{}", num(&it, "m"));
}

#[test]
fn error_behaviour_for_empty_nan_and_non_square_input_is_unchanged() {
    let e = err("A = randn(200, 200)\nA[3, 4] = nan\nf = svd(A)");
    assert!(e.contains("NaN") || e.contains("infinity"), "{e}");
    let e = err("A = randn(200, 200)\nA[3, 4] = nan\nf = qr(A)");
    assert!(e.contains("NaN") || e.contains("infinity"), "{e}");
    let e = err("A = randn(200, 200)\nA[3, 4] = nan\nf = eig(A)");
    assert!(e.contains("NaN") || e.contains("infinity"), "{e}");
    let e = err("A = randn(150, 200)\nf = eig(A)");
    assert!(e.contains("square"), "{e}");
}
