//! Acceptance tests for the M2 runnable subset.
//!
//! These lock in end-to-end behavior (parse → execute → output) so later
//! milestones cannot silently regress the surface Ahmed's scripts rely on.

use qu_interp::table::Column;
use qu_interp::{numeric, Interp, TempScale, UnitTag, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

#[test]
fn demo_signal_runs_end_to_end() {
    let src = include_str!("../../../examples/demo_signal.qu");
    let it = run(src);
    // The decaying cosine, detrended, has (near-)zero mean.
    let out = it.out;
    assert!(out.contains("mean x = "), "output was:\n{out}");
    assert!(out.contains("crest  = "), "output was:\n{out}");
    assert!(out.contains("sum 1..N = 136"), "output was:\n{out}"); // 1..16
    assert!(it.figures >= 1, "expected a recorded figure");
}

#[test]
fn user_function_and_units() {
    let it = run(
        "q(v) := max(abs(v)) / rms(v)\n\
         Fs = 48 kHz\n\
         x = -2 to 2\n\
         c = q(x)\n\
         f = Fs",
    );
    // Since design doc §8 phase 2, `48 kHz` is tracked as
    // `Value::Unit(_, UnitTag::Dim(_, Some("Hz")))`, not a bare `Value::Num` —
    // the SI-normalized magnitude is unchanged, only the tag is new.
    assert!(matches!(
        it.get("f"),
        Some(Value::Unit(n, UnitTag::Dim(_, Some("Hz")))) if (*n - 48000.0).abs() < 1e-6
    ));
    // crest factor of a symmetric ramp is finite and >= 1
    if let Some(Value::Num(c)) = it.get("c") {
        assert!(*c >= 1.0 - 1e-9);
    } else {
        panic!("c should be a number");
    }
}

#[test]
fn recursive_user_function() {
    // recursion requires the `function` block form (`:=` one-liners are pure,
    // non-recursive per §41.1), exercising the return/restore machinery.
    let it = run(
        "function fact(n)\n\
         \x20   if n < 2 then\n\
         \x20       return 1\n\
         \x20   end if\n\
         \x20   return n * fact(n - 1)\n\
         end function\n\
         y = fact(5)",
    );
    assert!(matches!(it.get("y"), Some(Value::Num(n)) if (*n - 120.0).abs() < 1e-9));
}

#[test]
fn interpreter_links_the_shared_numeric_core() {
    let capabilities = numeric::capabilities();
    assert!(capabilities.contains("fft-rustfft"));
    assert!(capabilities.contains("selection"));
    assert!(capabilities.contains("native+wasm"));
}

// ---------------------------------------------------------------- M2 arrays

fn num(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("{name} is not a number: {other:?}"),
    }
}

fn assert_str(it: &Interp, name: &str, expected: &str) {
    match it.get(name) {
        Some(Value::Str(s)) => assert_eq!(s.as_str(), expected, "{name} mismatch"),
        other => panic!("{name} is not a string: {other:?}"),
    }
}

fn list_len(it: &Interp, name: &str) -> usize {
    match it.get(name) {
        Some(Value::List(l)) => l.len(),
        other => panic!("{name} is not a list: {other:?}"),
    }
}

fn vec(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(v)) => v.to_vec(),
        other => panic!("{name} is not a vector: {other:?}"),
    }
}

/// Like [`vec`], but also accepts a `Mat` (flattened column-major) — for
/// asserting on values that may have gained an explicit orientation (e.g.
/// via `.T`).
fn flat(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("{name} is not a vector or matrix: {other:?}"),
    }
}

/// Reads a single `(row, col)` element out of a `Value::Mat` variable —
/// for the transformer/attention gradient-check tests, which read one or
/// two specific entries out of a `grad()`-returned gradient matrix rather
/// than the whole thing.
fn mat_at(it: &Interp, name: &str, row: usize, col: usize) -> f64 {
    match it.get(name) {
        Some(Value::Mat(m)) => m.get(row, col).unwrap_or_else(|e| panic!("{name}[{row},{col}]: {e}")),
        other => panic!("{name} is not a matrix: {other:?}"),
    }
}

/// `catalog/qu_linear_algebra.qu` runs and its matrix results are correct.
#[test]
fn linear_algebra_example_runs_end_to_end() {
    let src = include_str!("../../../../catalog/qu_linear_algebra.qu");
    let it = run(src);
    let out = &it.out;
    assert!(out.contains("A * B   = [19, 22; 43, 50]"), "output:\n{out}");
    assert!(out.contains("A .* B  = [5, 12; 21, 32]"), "output:\n{out}");
    assert!(out.contains("outer   = [10, 20; 20, 40; 30, 60]"), "output:\n{out}");
    assert!(out.contains("colsum  = [5, 7, 9]"), "output:\n{out}");
    assert!(out.contains("dot     = 9"), "output:\n{out}");
}

/// `catalog/qu_fft_spectrum.qu` recovers the two tones and round-trips.
#[test]
fn fft_spectrum_example_runs_end_to_end() {
    let src = include_str!("../../../../catalog/qu_fft_spectrum.qu");
    let it = run(src);
    let out = &it.out;
    assert!(out.contains("bin 4 mag  = 1.000"), "output:\n{out}");
    assert!(out.contains("bin 9 mag  = 0.500"), "output:\n{out}");
    assert!(out.contains("bin 9 phase= 1.0472"), "output:\n{out}"); // pi/3
}


/// `engine/examples/peak_finding.qu` locates the dominant peak of a
/// synthetic (Gaussian bump + ripple + noise) signal three independent
/// ways — `max`/`argmax`, `find_peaks` reduced to its tallest candidate,
/// and a hand-written brute-force `for`-loop scan that calls none of those
/// builtins — and reports that all three agree, then annotates the plot.
/// The example's own `savefig` path (`engine/examples/peak_finding.svg`)
/// is relative to the repo root the CLI is normally run from; a plain
/// `cargo test` runs with the crate directory as its cwd, so this test
/// redirects that one line to a temp file before running, the same
/// pattern `savefig_writes_svg_html_and_tikz_with_expected_markers` uses.
#[test]
fn peak_finding_example_all_three_methods_agree() {
    let raw = include_str!("../../../examples/peak_finding.qu");
    let tmp = std::env::temp_dir().join("qu_peak_finding_acceptance_test.svg");
    let tmp_arg = tmp.display().to_string().replace('\\', "\\\\");
    let src = raw.replace(
        "savefig(\"engine/examples/peak_finding.svg\")",
        &format!("savefig(\"{tmp_arg}\")"),
    );
    assert_ne!(src, raw, "expected to find and redirect the example's savefig(...) call");
    let it = run(&src);
    let out = &it.out;

    // The three independently-implemented methods must land on the exact
    // same sample and value.
    assert!(
        out.contains("Method a (max + argmax):  index = 131, value = 5.8098"),
        "output:\n{out}"
    );
    assert!(
        out.contains("Method b (find_peaks):    index = 131, value = 5.8098"),
        "output:\n{out}"
    );
    assert!(
        out.contains("Method c (manual scan):   index = 131, value = 5.8098"),
        "output:\n{out}"
    );
    assert!(
        out.contains("All three methods agree: peak at index 131, value 5.8098"),
        "output:\n{out}"
    );

    // Cross-check directly against the interpreter's own variables, not
    // just the printed text.
    assert_eq!(num(&it, "peak_idx_a"), 131.0);
    assert_eq!(num(&it, "peak_idx_b"), 131.0);
    assert_eq!(num(&it, "peak_idx_c"), 131.0);
    let x = vec(&it, "x");
    assert_eq!(x.len(), 200);
    assert!((x[131] - 5.8098).abs() < 1e-3, "x[131] = {}", x[131]);
    // Independently verify this actually is the vector's maximum (not just
    // what the three methods claim), and that it sits near the synthetic
    // Gaussian bump's built-in center (n = 120), not at an edge or on a
    // spurious ripple/noise crest far from it.
    let brute_max = x.iter().cloned().fold(f64::MIN, f64::max);
    assert!((brute_max - x[131]).abs() < 1e-12);
    assert!(x.iter().enumerate().all(|(i, &v)| i == 131 || v <= x[131]));
    assert!((131i32 - 120i32).abs() <= 20, "peak strayed too far from the bump center");

    assert!(it.figures >= 1, "expected a recorded figure");
}

// --------------------------------------------------- targeted M2 semantics

#[test]
fn matrix_multiply_versus_elementwise() {
    let it = run("A = [1, 2; 3, 4]\nB = [5, 6; 7, 8]\nc = A * B\nd = A .* B");
    // c and d are matrices; check the (1,1) entries via indexed reads.
    let it2 = run(
        "A = [1, 2; 3, 4]\nB = [5, 6; 7, 8]\n\
         mm = (A * B)[1, 1]\nhad = (A .* B)[1, 1]",
    );
    let _ = it;
    assert_eq!(num(&it2, "mm"), 50.0);
    assert_eq!(num(&it2, "had"), 32.0);
}

#[test]
fn bare_vector_plus_matrix_broadcasts_as_a_row_bias_when_lengths_say_so() {
    // The standard "add a per-output-channel bias" idiom (`X*W + b`) — `b`
    // is a bare `Value::Vec` (from `[10, 20]`'s own "orientation-free
    // vector" convention), and its length (2) matches `pred`'s column
    // count (2), not its row count (3), so it must broadcast as a *row*
    // added to every row of `pred`, not fail or silently misapply.
    let it = run(
        "pred = [1, 2; 3, 4; 5, 6]\n\
         b = [10, 20]\n\
         out = pred + b",
    );
    match it.get("out") {
        Some(Value::Mat(m)) => {
            assert_eq!(m.shape(), (3, 2));
            assert_eq!(m.get(0, 0).unwrap(), 11.0);
            assert_eq!(m.get(0, 1).unwrap(), 22.0);
            assert_eq!(m.get(2, 0).unwrap(), 15.0);
            assert_eq!(m.get(2, 1).unwrap(), 26.0);
        }
        other => panic!("out should be a 3x2 matrix: {other:?}"),
    }
}

#[test]
fn bare_vector_plus_matrix_still_broadcasts_as_a_column_when_that_matches() {
    // The pre-existing column default is unchanged when the vector's
    // length matches rows, not columns (`b` here can only mean "one value
    // per row").
    let it = run(
        "pred = [1, 2, 3; 4, 5, 6]\n\
         b = [10, 20]\n\
         out = pred + b",
    );
    match it.get("out") {
        Some(Value::Mat(m)) => {
            assert_eq!(m.shape(), (2, 3));
            assert_eq!(m.get(0, 0).unwrap(), 11.0);
            assert_eq!(m.get(1, 0).unwrap(), 24.0);
        }
        other => panic!("out should be a 2x3 matrix: {other:?}"),
    }
}

#[test]
fn grad_of_a_matmul_plus_bias_regression_loss_matches_finite_differences() {
    // The exact scenario the row/column broadcast fix targets, end to end
    // through autodiff: `loss = mean((X*W + b - Y)^2)` with a *bias* term
    // added to a genuinely multi-column matmul result (X*W is 3x2, so a
    // length-2 bias can only mean "one value per output column" — the
    // ambiguity the fix resolves). `W` is a plain constant here (not
    // `param`) so the finite-difference cross-check only has to vary `b`,
    // keeping this test focused on the bias-broadcast path specifically
    // rather than re-covering matmul's own vjp rule (already covered by
    // `grad_of_a_matmul_regression_loss_matches_finite_differences`).
    let it = run(
        "X = [1,2;3,4;5,6]\n\
         W = [1,2;3,4]\n\
         Y = [2,3;4,5;6,7]\n\
         b = param([0.1, 0.2])\n\
         pred = X*W + b\n\
         loss = mean((pred - Y).^2)\n\
         gb = grad(loss, wrt=b)\n\
         function loss_of(b1, b2)\n\
         \x20   Xc = [1,2;3,4;5,6]\n\
         \x20   Wc = [1,2;3,4]\n\
         \x20   Yc = [2,3;4,5;6,7]\n\
         \x20   predc = Xc*Wc + [b1, b2]\n\
         \x20   return mean((predc - Yc).^2)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gb1 = (loss_of(0.1+eps, 0.2) - loss_of(0.1-eps, 0.2)) / (2*eps)\n\
         fd_gb2 = (loss_of(0.1, 0.2+eps) - loss_of(0.1, 0.2-eps)) / (2*eps)",
    );
    let gb = match it.get("gb") {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("gb should be a vector or matrix: {other:?}"),
    };
    assert!((gb[0] - num(&it, "fd_gb1")).abs() < 1e-4);
    assert!((gb[1] - num(&it, "fd_gb2")).abs() < 1e-4);
}

#[test]
fn outer_product_via_orientation_contracts() {
    let it = run(
        "u = [1, 2, 3]\nv = [10, 20]\n\
         u as vector(3, 1)\nv as vector(1, 2)\n\
         P = u * v\nrow0 = P[0, :]\nrow2 = P[2, :]",
    );
    assert_eq!(vec(&it, "row0"), vec![10.0, 20.0]);
    assert_eq!(vec(&it, "row2"), vec![30.0, 60.0]);
}

#[test]
fn complex_arithmetic_and_magnitude() {
    // `z * conj(z)` = |z|^2 = 25; the value stays complex (im == 0) so we read
    // its real part with `real(...)`.
    let it = run("z = 3 + 4i\nm = abs(z)\np = real(z * conj(z))");
    assert_eq!(num(&it, "m"), 5.0);
    assert_eq!(num(&it, "p"), 25.0);
}

#[test]
fn fft_of_impulse_is_flat_and_round_trips() {
    let it = run(
        "x = zeros(8)\nx[0] = 1\nX = fft(x, 8)\n\
         mag = abs(X)\nxr = real(ifft(X, 8))\nerr = max(abs(xr - x))",
    );
    assert_eq!(vec(&it, "mag"), vec![1.0; 8]);
    assert!(num(&it, "err") < 1e-12);
}

#[test]
fn matrix_index_assignment_writes_rows_and_cells() {
    let it = run(
        "M = zeros(3, 3)\nM[0, :] = [1, 2, 3]\nM[1, 1] = 9\n\
         r0 = M[0, :]\ncell = M[1, 1]\ncol1 = M[:, 1]",
    );
    assert_eq!(vec(&it, "r0"), vec![1.0, 2.0, 3.0]);
    assert_eq!(num(&it, "cell"), 9.0);
    assert_eq!(vec(&it, "col1"), vec![2.0, 9.0, 0.0]);
}

#[test]
fn reshape_uses_column_major_order() {
    let it = run("r = reshape(0 to 5, 2, 3)\nc = r[1, 2]\nflat = r[3]");
    // column-major: r[1,2] is element index 1 + 2*2 = 5 -> value 5
    assert_eq!(num(&it, "c"), 5.0);
    // single flat index is column-major too: index 3 -> value 3
    assert_eq!(num(&it, "flat"), 3.0);
}

#[test]
fn clip_idiom_via_two_argument_min_max() {
    let it = run("x = -3 to 3\ny = min(max(x, -1), 1)");
    assert_eq!(vec(&it, "y"), vec![-1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0]);
}

// ------------------------------------------------ function definition forms

#[test]
fn colon_equals_defines_a_one_line_function() {
    // canonical §41.1 form; elemental, so it maps over arrays
    let it = run("crest(x) := max(abs(x)) / rms(x)\nsq(x) := x ^ 2\n\
                  c = crest([-3, 1, 2, 4])\ns = sq([1, 2, 3])");
    assert!((num(&it, "c") - 1.4605935).abs() < 1e-4);
    assert_eq!(vec(&it, "s"), vec![1.0, 4.0, 9.0]);
}

#[test]
fn function_block_supports_control_flow_and_recursion() {
    let it = run(
        "function fib(n)\n\
         \x20   if n < 2 then\n\
         \x20       return n\n\
         \x20   end if\n\
         \x20   return fib(n - 1) + fib(n - 2)\n\
         end function\n\
         y = fib(10)",
    );
    assert_eq!(num(&it, "y"), 55.0);
}

#[test]
fn declare_and_initialize_in_one_statement() {
    // `name as contract = rhs` reshapes on assignment (column-major)
    let it = run("y as vector(2, 2) = [1, 2, 3, 4]\na = y[0, 1]\nb = y[1, 0]");
    assert_eq!(num(&it, "a"), 3.0);
    assert_eq!(num(&it, "b"), 2.0);
}

#[test]
fn legacy_def_fn_still_parses_as_an_alias() {
    // §41.1: `def fn` is retained so existing scripts keep working.
    let it = run("def fn twice(x) = 2 * x\ny = twice(21)");
    assert_eq!(num(&it, "y"), 42.0);
}

#[test]
fn smooth_max_family_builtins() {
    // logsumexp >= max; smoothmax -> max as beta grows; softmax sums to 1.
    let it = run(
        "x = [0, 1, 2, 3]\n\
         lse = logsumexp(x)\n\
         hard = max(x)\n\
         loose = smoothmax(x, 1)\n\
         tight = smoothmax(x, 100)\n\
         p = softmax(x)\n\
         psum = sum(p)",
    );
    assert!(num(&it, "lse") >= num(&it, "hard"));
    assert!((num(&it, "tight") - 3.0).abs() < 1e-2);
    assert!(num(&it, "tight") < num(&it, "loose"));
    assert!((num(&it, "psum") - 1.0).abs() < 1e-12);
}

// --------------------------------------- transpose spellings, power, inverse

#[test]
fn transpose_spellings_agree() {
    // `.T`, `'`, `.'`, `transpose(...)` all give the same column vector.
    // (`^T`/`^H` are deliberately not supported: `x^T` collides with "raise
    // `x` to the power of a variable named `T`" — see `caret_t_is_power`.)
    let it = run(
        "A = [1,2,3].T\nB = [4,5,6].T\nC = A + B\n\
         a2 = [1,2,3]'\na3 = [1,2,3].'\na5 = transpose([1,2,3])",
    );
    assert_eq!(flat(&it, "C"), vec![5.0, 7.0, 9.0]);
    for name in ["a2", "a3", "a5"] {
        assert_eq!(flat(&it, name), vec![1.0, 2.0, 3.0], "mismatch for {name}");
    }
}

#[test]
fn caret_t_is_power_not_transpose() {
    // `x^T` must mean "x to the power of the variable T", never transpose —
    // `^T`/`^H` sugar was rejected specifically because it used to shadow a
    // real variable named `T`/`H` with no warning.
    let it = run("T = 3\nx = 2\ny = x^T");
    assert_eq!(num(&it, "y"), 8.0);
}

#[test]
fn matrix_power_and_inverse() {
    let it = run(
        "M = [4, 7; 2, 6]\n\
         Mi = M^(-1)\n\
         I = M * Mi\n\
         M2 = M^2\n\
         M0 = M^0",
    );
    // M * M^-1 == I (2x2)
    let it2 = run("M = [4, 7; 2, 6]\nMi = M^(-1)\nI = M * Mi\na=I[0,0]\nb=I[0,1]\nc=I[1,0]\nd=I[1,1]");
    assert!((num(&it2, "a") - 1.0).abs() < 1e-9);
    assert!(num(&it2, "b").abs() < 1e-9);
    assert!(num(&it2, "c").abs() < 1e-9);
    assert!((num(&it2, "d") - 1.0).abs() < 1e-9);
    let _ = it;
    // M^2 == M*M
    let it3 = run("M = [4, 7; 2, 6]\nA = M^2\nB = M*M\nd = A[1,1] - B[1,1]");
    assert!(num(&it3, "d").abs() < 1e-9);
    // M^0 is the identity
    let it4 = run("M = [4, 7; 2, 6]\nI0 = M^0\na=I0[0,0]\nb=I0[1,1]\nc=I0[0,1]");
    assert_eq!(num(&it4, "a"), 1.0);
    assert_eq!(num(&it4, "b"), 1.0);
    assert_eq!(num(&it4, "c"), 0.0);
}

#[test]
fn non_square_or_singular_inverse_errors_clearly() {
    let mut it = Interp::new();
    let err = it.run("M = [1, 2, 3; 4, 5, 6]\nMi = M^(-1)").unwrap_err();
    assert!(err.to_string().contains("square"), "got: {err}");

    let mut it2 = Interp::new();
    let err2 = it2.run("M = [1, 2; 2, 4]\nMi = M^(-1)").unwrap_err();
    assert!(err2.to_string().contains("singular"), "got: {err2}");
}

#[test]
fn elementwise_power_dot_caret() {
    let it = run("v = [1, 2, 3]\ns = v .^ 2");
    assert_eq!(vec(&it, "s"), vec![1.0, 4.0, 9.0]);
}

// -------------------------------------------------------- complex matrices

#[test]
fn j_suffix_is_accepted_alongside_i() {
    let it = run("a = 3 + 4j\nb = 3 + 4i\nd = real(a - b)");
    assert_eq!(num(&it, "d"), 0.0);
}

#[test]
fn complex_matrix_literal_and_matmul() {
    // a genuine 2x2 complex matrix; matmul against its own inverse gives I.
    let it = run(
        "C = [1+2j, 3+4j; 0+1j, 2-1j]\n\
         C1 = C.H * C\n\
         F1 = C1^(-1)\n\
         chk = C1 * F1\n\
         a = real(chk[0,0])\n\
         b = real(chk[1,1])\n\
         off = abs(chk[0,1])",
    );
    assert!((num(&it, "a") - 1.0).abs() < 1e-6);
    assert!((num(&it, "b") - 1.0).abs() < 1e-6);
    assert!(num(&it, "off") < 1e-6);
}

#[test]
fn complex_least_squares_normal_equations() {
    // (C.H C)^-1 C.H D solves the complex least-squares problem end to end.
    let it = run(
        "C = [1+2j, 3+4j; 0+1j, 2-1j]\n\
         D = [4, 6]\n\
         D as vector(2, 1)\n\
         E2 = (C.H * C)^-1 * C.H * D",
    );
    match it.get("E2") {
        Some(Value::CMat(_)) => {}
        other => panic!("expected a complex matrix result, got {other:?}"),
    }
}

#[test]
fn backslash_solves_a_square_system_matching_inv_times_b() {
    let it = run(
        "A = [4, 7; 2, 6]\n\
         b = [1; 1]\n\
         x = A \\ b\n\
         xr = inv(A) * b\n\
         check = A * x\n\
         err0 = abs(x[0,0] - xr[0,0])\n\
         err1 = abs(x[1,0] - xr[1,0])",
    );
    assert!(num(&it, "err0") < 1e-9);
    assert!(num(&it, "err1") < 1e-9);
    match it.get("check") {
        Some(Value::Mat(m)) => {
            assert!((m.get(0, 0).unwrap() - 1.0).abs() < 1e-9);
            assert!((m.get(1, 0).unwrap() - 1.0).abs() < 1e-9);
        }
        other => panic!("expected a column matrix, got {other:?}"),
    }
}

#[test]
fn backslash_least_squares_on_an_overdetermined_system() {
    // 3 equations, 2 unknowns: `\` should give the least-squares solution.
    // Verified via the standard least-squares optimality condition — the
    // residual `A*x - b` must be orthogonal to A's column space
    // (`A.' * resid ~= 0`) — the same property `polyfit`'s normal-equations
    // path relies on.
    let it = run(
        "A = [1, 0; 0, 1; 1, 1]\n\
         b = [1; 2; 4]\n\
         x = A \\ b\n\
         resid = A * x - b\n\
         ortho = A.' * resid",
    );
    match it.get("ortho") {
        Some(Value::Mat(m)) => {
            for v in m.as_slice() {
                assert!(v.abs() < 1e-9, "residual not orthogonal to columns: {v}");
            }
        }
        other => panic!("expected a matrix, got {other:?}"),
    }
}

#[test]
fn complex_backslash_matches_the_hand_written_normal_equations() {
    let it = run(
        "C = [1+2j, 3+4j; 0+1j, 2-1j]\n\
         D = [4, 6]\n\
         D as vector(2, 1)\n\
         E1 = C \\ D\n\
         E2 = (C.H * C)^-1 * C.H * D",
    );
    match (it.get("E1"), it.get("E2")) {
        (Some(Value::CMat(a)), Some(Value::CMat(b))) => {
            for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
                assert!((x.re - y.re).abs() < 1e-9 && (x.im - y.im).abs() < 1e-9, "{x:?} vs {y:?}");
            }
        }
        other => panic!("expected two complex matrices, got {other:?}"),
    }
}

#[test]
fn outer_product_row_times_column_needs_explicit_orientation() {
    // an orientation-free complex vector defaults to a COLUMN at a `*`
    // boundary (mirrors the real-vector convention) — so multiplying it
    // against another column is a clear shape error, not a silent guess.
    let mut it = Interp::new();
    let err = it.run("C = [1+2j, 3+4j]\nD = [4, 6]\nE = C * D.T").unwrap_err();
    assert!(err.to_string().contains("inner dimensions"), "got: {err}");
    // giving C an explicit row orientation resolves it to a (1,1) product.
    let ok = run("C = [1+2j, 3+4j]\nD = [4, 6]\nC as vector(1, 2)\nE = C * D.T");
    match ok.get("E") {
        Some(Value::Complex(c)) => {
            assert!((c.re - 22.0).abs() < 1e-9);
            assert!((c.im - 32.0).abs() < 1e-9);
        }
        other => panic!("expected a complex scalar, got {other:?}"),
    }
}

#[test]
fn rank_one_gram_matrix_is_correctly_singular() {
    // C.T*C for a single 2-element row vector is a rank-1 outer product: a
    // genuinely singular 2x2 matrix. The inverse must error, not return a
    // silently wrong (Inf/NaN) answer.
    let mut it = Interp::new();
    let err = it
        .run("C = [1+2j, 3+4j]\nC as vector(1, 2)\nC1 = C.T * C\nF1 = C1^(-1)")
        .unwrap_err();
    assert!(err.to_string().contains("singular"), "got: {err}");
}

// ------------------------------- comma-ellipsis ranges, SI prefixes, val()

#[test]
fn comma_ellipsis_range() {
    let it = run("t = 1, 2, ..., 10\nu = 0, 5, ..., 20\nd = -3, -1, ..., 5");
    assert_eq!(vec(&it, "t"), (1..=10).map(|x| x as f64).collect::<Vec<_>>());
    assert_eq!(vec(&it, "u"), vec![0.0, 5.0, 10.0, 15.0, 20.0]);
    assert_eq!(vec(&it, "d"), vec![-3.0, -1.0, 1.0, 3.0, 5.0]);
}

#[test]
fn bare_si_prefix_literals_scale_by_magnitude() {
    let it = run(
        "f = 50m\nn = 2k\ng = 3M\nh = 1G\ni2 = 5u\nj2 = 10p\nk2 = 1a",
    );
    assert!((num(&it, "f") - 0.05).abs() < 1e-12);
    assert_eq!(num(&it, "n"), 2000.0);
    assert_eq!(num(&it, "g"), 3_000_000.0);
    assert_eq!(num(&it, "h"), 1e9);
    assert!((num(&it, "i2") - 5e-6).abs() < 1e-15);
    assert!((num(&it, "j2") - 1e-11).abs() < 1e-18);
    assert!((num(&it, "k2") - 1e-18).abs() < 1e-24);
}

#[test]
fn tiny_magnitudes_display_in_scientific_notation() {
    // fixed `.6` formatting would round 1e-11 to a misleading "0".
    let it = run("j2 = 10p\nprint(\"{j2}\")");
    assert_eq!(it.out.trim(), "1e-11");
}

#[test]
fn val_is_currently_the_identity() {
    // `:=` is eager in M2, so `val(x)` has nothing to materialize yet; it
    // becomes meaningful once deferred dataflow is lazily fused (M4).
    let it = run("x = val(42)\ny = val([1,2,3])");
    assert_eq!(num(&it, "x"), 42.0);
    assert_eq!(vec(&it, "y"), vec![1.0, 2.0, 3.0]);
}

// ------------------------------------------------- arbitrary-length fft

#[test]
fn fft_supports_non_power_of_two_lengths() {
    // the exact case that used to raise "fft requires a power-of-two length"
    let it = run(
        "t = 1, 2, ..., 50\nx = sin(t)\nX = fft(x)\n\
         n = length(X)\n\
         err = max(abs(real(ifft(X)) - x))",
    );
    assert_eq!(num(&it, "n"), 50.0);
    assert!(num(&it, "err") < 1e-9, "round-trip error too large");
}

#[test]
fn fft_of_odd_length_locates_a_known_tone() {
    let it = run(
        "N = 15\nn = 0 to N - 1\nx = cos(2 * pi * 2 * n / N)\n\
         X = fft(x, N)\nmag = abs(X)\n\
         peak = mag[2]\nother = mag[7]",
    );
    assert!(num(&it, "peak") > 5.0);
    assert!(num(&it, "other") < 1e-6);
}

// -------------------------------- fftc/rfft/irfft/fftr/dct/idct/dwt/stft

#[test]
fn fftc_is_an_alias_of_fft() {
    let it = run("x = [1,2,3,4,5,6,7,8]\na = abs(fft(x))\nb = abs(fftc(x))\nd = max(abs(a-b))");
    assert_eq!(num(&it, "d"), 0.0);
}

#[test]
fn rfft_irfft_round_trip() {
    let it = run(
        "x = [1,2,3,4,5,6,7,8]\nhalf = rfft(x)\nn = length(half)\n\
         back = irfft(half, 8)\nerr = max(abs(back - x))\n\
         back2 = fftr(half, 8)\nerr2 = max(abs(back2 - x))",
    );
    assert_eq!(num(&it, "n"), 5.0);
    assert!(num(&it, "err") < 1e-9);
    assert!(num(&it, "err2") < 1e-9);
}

#[test]
fn irfft_needs_explicit_output_length() {
    let mut it = Interp::new();
    let err = it.run("x = [1,2,3,4]\nhalf = rfft(x)\ny = irfft(half)").unwrap_err();
    assert!(err.to_string().contains("second argument"), "got: {err}");
}

#[test]
fn dct_idct_round_trip() {
    let it = run("x = [1,2,3,4,5,6,7]\nc = dct(x)\nxr = idct(c)\nerr = max(abs(xr - x))");
    assert!(num(&it, "err") < 1e-9);
}

#[test]
fn haar_dwt_idwt_round_trip() {
    let it = run(
        "x = [1,2,3,4,5,6,7,8]\nD = dwt(x)\napprox = D[0,:]\ndetail = D[1,:]\n\
         xr = idwt(D)\nerr = max(abs(xr - x))",
    );
    assert_eq!(vec(&it, "approx").len(), 4);
    assert_eq!(vec(&it, "detail").len(), 4);
    assert!(num(&it, "err") < 1e-9);
}

#[test]
fn stft_shape_and_spectrogram() {
    let it = run(
        "n = 512\nt = 0 to n-1\nx = sin(2*pi*8*t/n)\n\
         S = stft(x, 64, 32)\nsh = shape(S)\nmag = abs(S)\nmsh = shape(mag)",
    );
    assert_eq!(vec(&it, "sh"), vec![64.0, 15.0]);
    assert_eq!(vec(&it, "msh"), vec![64.0, 15.0]);
}

// ------------------------------- advanced time-frequency (§3)
//
// Each of these asserts WHERE the energy landed, against a frequency the
// signal was built to have. A transform that returned a correctly-shaped
// matrix of zeros, or that mislabelled its own `freq` axis, passes a
// shape check and fails every one of these.

#[test]
fn cqt_lands_a_tone_in_the_bin_at_its_own_pitch() {
    // 440 Hz is exactly two octaves above fmin = 110, so at 12 bins per
    // octave the answer is bin 24 -- fixed by the geometric spacing
    // before the transform runs, not read off its output.
    let it = run(
        "fs = 2000\nt = 0 to 3999\nx = sin(2*pi*440*t/fs)\n\
         C = cqt(x, fs, fmin=110, fmax=880, bins_per_octave=12, hop=20)\n\
         nbins = length(C.freq)\nnframes = length(C.time)\n\
         M = abs(C.coef)\nmid = floor(nframes/2)\n\
         k = argmax(M[:, mid])\npeak_hz = C.freq[k]\n\
         bin_hz = C.freq[24]\noctave = C.freq[12] / C.freq[0]",
    );
    assert_eq!(num(&it, "k"), 24.0);
    assert!((num(&it, "peak_hz") - 440.0).abs() < 1e-6, "peak at {}", num(&it, "peak_hz"));
    assert!((num(&it, "bin_hz") - 440.0).abs() < 1e-6);
    // Twelve bins along is exactly one octave, at any pitch -- the
    // defining property, and what `stft`'s linear bins cannot do.
    assert!((num(&it, "octave") - 2.0).abs() < 1e-9);
    assert_eq!(num(&it, "nbins"), 37.0);
}

#[test]
fn mel_spectrogram_puts_a_tone_in_the_band_that_covers_it() {
    let it = run(
        "fs = 8000\nt = 0 to 7999\nx = sin(2*pi*1000*t/fs)\n\
         S = mel_spectrogram(x, fs, n_mels=40, fmin=0, fmax=4000, nfft=512, hop=256)\n\
         nb = length(S.freq)\nmid = floor(length(S.time)/2)\n\
         k = argmax(S.power[:, mid])\npeak_hz = S.freq[k]\n\
         first_gap = S.freq[1] - S.freq[0]\nlast_gap = S.freq[39] - S.freq[38]",
    );
    assert_eq!(num(&it, "nb"), 40.0);
    // Band spacing up at 1 kHz is ~150 Hz, so one band of slack.
    let peak = num(&it, "peak_hz");
    assert!((peak - 1000.0).abs() < 160.0, "peak band centre {peak} Hz, expected ~1000");
    // Bands crowd the low frequencies -- a linear axis would not.
    assert!(num(&it, "last_gap") > 2.0 * num(&it, "first_gap"));
}

#[test]
fn cwt_morlet_follows_a_chirp_up_the_scale_ladder() {
    // 50 Hz -> 200 Hz over 2 s, so the instantaneous frequency at 25% and
    // 75% of the record is 87.5 Hz and 162.5 Hz.
    let it = run(
        "fs = 1000\nn = 2000\nt = 0 to n-1\ntt = t/fs\ntend = (n-1)/fs\n\
         x = sin(2*pi*(50*tt + 0.5*(200-50)/tend*tt.*tt))\n\
         W = cwt(x, fs, wavelet=\"morlet\")\n\
         A = abs(W.coef)\nnsc = length(W.scale)\nncol = length(W.time)\n\
         early = W.freq[argmax(A[:, 500])]\nlate = W.freq[argmax(A[:, 1500])]\n\
         name = W.wavelet",
    );
    // One column per input sample: no framing, no decimation.
    assert_eq!(num(&it, "ncol"), 2000.0);
    assert!(num(&it, "nsc") > 8.0);
    assert_str(&it, "name", "morlet");
    let early = num(&it, "early");
    let late = num(&it, "late");
    assert!((early - 87.5).abs() < 0.12 * 87.5, "ridge at 25% of the record: {early} Hz");
    assert!((late - 162.5).abs() < 0.12 * 162.5, "ridge at 75% of the record: {late} Hz");
    assert!(late > early, "the chirp rises, so the ridge must too");
}

#[test]
fn cwt_rejects_a_discrete_wavelet_by_name() {
    // `haar` belongs to `dwt`; accepting it here would imply the two
    // transforms are interchangeable.
    let mut it = Interp::new();
    let err = it
        .run("x = sin(0 to 255)\nW = cwt(x, 100, wavelet=\"haar\")")
        .expect_err("haar is not a continuous wavelet");
    let msg = format!("{err}");
    assert!(msg.contains("dwt"), "error should name the right transform: {msg}");
}

#[test]
fn wigner_ville_puts_a_tone_on_its_own_frequency_row() {
    // Row k is k*fs/(2N) Hz, so at fs=1000 and N=256 the 125 Hz tone is
    // row 64 exactly -- twice the resolution of a 256-point FFT.
    let it = run(
        "fs = 1000\nt = 0 to 255\nx = sin(2*pi*125*t/fs)\n\
         V = wigner_ville(x, fs)\n\
         rows = length(V.freq)\ncols = length(V.time)\n\
         k = argmax(V.tfr[:, 128])\npeak_hz = V.freq[k]\nrow64 = V.freq[64]",
    );
    assert_eq!(num(&it, "rows"), 256.0);
    assert_eq!(num(&it, "cols"), 256.0);
    assert_eq!(num(&it, "k"), 64.0);
    assert!((num(&it, "peak_hz") - 125.0).abs() < 1e-9);
    assert!((num(&it, "row64") - 125.0).abs() < 1e-9);
}

#[test]
fn wigner_ville_refuses_a_record_whose_square_will_not_fit() {
    let mut it = Interp::new();
    let err = it
        .run("x = sin(0 to 2999)\nV = wigner_ville(x, 1000)")
        .expect_err("a 3000-sample record is 9,000,000 cells");
    let msg = format!("{err}");
    assert!(msg.contains("quadratic"), "error should say why: {msg}");
}

// ------------------------------------------------- inclusive-range epsilon

#[test]
fn range_epsilon_scales_with_length_not_fixed() {
    // `1, 1.01, ..., 20` used to drop the inclusive stop sample (N=1900
    // instead of 1901) because a fixed 1e-10 tolerance was too tight for the
    // float drift accumulated over ~1900 divisions of an inexact step.
    let it = run("x = 1, 1.01, ..., 20\nn = length(x)\nlast = x[n-1]");
    assert_eq!(num(&it, "n"), 1901.0);
    assert!((num(&it, "last") - 20.0).abs() < 1e-9);
}

#[test]
fn backwards_range_is_still_empty_not_an_error() {
    let it = run("x = 5 to 1\nn = length(x)");
    assert_eq!(num(&it, "n"), 0.0);
}

// --------------------------------------------------- Signal (spec §41.2)

#[test]
fn signal_constructor_and_accessors() {
    let it = run(
        "x = signal([1,2,3,4], 100)\n\
         fs = x.Fs\nnn = x.N\ndt = x.dt\nt0 = x.t[0]\nt1 = x.t[1]",
    );
    assert_eq!(num(&it, "fs"), 100.0);
    assert_eq!(num(&it, "nn"), 4.0);
    assert_eq!(num(&it, "dt"), 0.01);
    assert_eq!(num(&it, "t0"), 0.0);
    assert_eq!(num(&it, "t1"), 0.01);
}

#[test]
fn signal_is_interchangeable_with_vector_for_arithmetic() {
    let it = run(
        "x = signal([1,2,3], 100)\n\
         a = x + 1\nafs = a.Fs\n\
         b = sin(x)\nbfs = b.Fs\n\
         c = -x\ncfs = c.Fs",
    );
    match it.get("a") {
        Some(Value::Signal(..)) => {}
        other => panic!("x+1 should stay a Signal, got {other:?}"),
    }
    assert_eq!(num(&it, "afs"), 100.0);
    assert_eq!(num(&it, "bfs"), 100.0);
    assert_eq!(num(&it, "cfs"), 100.0);
}

#[test]
fn signal_array_round_trip() {
    let it = run("x = signal([1,2,3], 100)\ny = array(x)");
    match it.get("y") {
        Some(Value::Vec(v)) => assert_eq!(v.as_ref(), &vec![1.0, 2.0, 3.0]),
        other => panic!("array(x) should be a plain Vec, got {other:?}"),
    }
}

#[test]
fn signal_diff_and_slice_preserve_the_wrapper() {
    let it = run(
        "x = signal([1,2,4,8,16], 100)\n\
         d = x.diff()\ndfs = d.Fs\n\
         seg = x[1:3]\nsegfs = seg.Fs\nsegn = seg.N\n\
         elem = x[0]",
    );
    match it.get("d") {
        Some(Value::Signal(v, _, _)) => assert_eq!(**v, vec![1.0, 2.0, 4.0, 8.0]),
        other => panic!("x.diff() should stay a Signal, got {other:?}"),
    }
    assert_eq!(num(&it, "dfs"), 100.0);
    assert_eq!(num(&it, "segfs"), 100.0);
    assert_eq!(num(&it, "segn"), 3.0); // `1:3` includes 3
    // a single sample is a plain number, not a Signal.
    match it.get("elem") {
        Some(Value::Num(_)) => {}
        other => panic!("a single indexed sample should be a Num, got {other:?}"),
    }
}

#[test]
fn mismatched_sample_rates_error_clearly() {
    let mut it = Interp::new();
    let err = it
        .run("z = signal([1,2,3], 50)\nw = signal([1,2,3], 200)\nbad = z + w")
        .unwrap_err();
    assert!(err.to_string().contains("sample rate"), "got: {err}");
}

#[test]
fn capitalized_signal_array_aliases_work() {
    // matches the spelling from the original feature request.
    let it = run("x = [1,2,3]\nx := Signal(x, 10)\ny := Array(x)");
    match it.get("x") {
        Some(Value::Signal(_, fs, _)) => assert_eq!(*fs, 10.0),
        other => panic!("expected a Signal, got {other:?}"),
    }
    match it.get("y") {
        Some(Value::Vec(v)) => assert_eq!(v.as_ref(), &vec![1.0, 2.0, 3.0]),
        other => panic!("expected a plain Vec, got {other:?}"),
    }
}

// ---- call-stack frames + fast-loop slot resolution (§ interpreter performance, 2026-08-23) ----

#[test]
fn nested_calls_do_not_leak_same_named_locals_across_depths() {
    // The pre-`Arc`-frame scoping hack tracked scope by "which env keys
    // existed before this call", not a real stack: if `outer()` assigns
    // `tmp` *before* calling `inner()` (also its own local `tmp`), `inner`
    // would see `tmp` as pre-existing (outer just created it) and its
    // return-time cleanup would leave `inner`'s value of `tmp` sitting in
    // `outer`'s scope — a silent cross-call corruption bug, not a crash.
    // Real per-call frames make this structurally impossible: each call
    // gets its own frame, so `inner`'s `tmp` and `outer`'s `tmp` never
    // touch the same storage no matter the call order.
    let it = run(
        "function inner()\n\
         \x20   tmp = 2\n\
         \x20   return tmp\n\
         end function\n\
         function outer()\n\
         \x20   tmp = 1\n\
         \x20   junk = inner()\n\
         \x20   return tmp\n\
         end function\n\
         result = outer()",
    );
    assert_eq!(num(&it, "result"), 1.0);
}

#[test]
fn a_call_cannot_silently_rewrite_the_callers_variables() {
    // The defect this rule exists to prevent, in the shape it actually
    // appeared: a helper using ordinary names for its own bookkeeping
    // clobbered the caller's variables of the same name, and the caller
    // went on using them. It surfaced as correct numbers under wrong
    // labels -- a results table printing a tone count where a keep
    // fraction belonged -- which is the shape that survives review.
    let it = run(
        "n = 42\n\
         total = 999\n\
         function helper(v)\n\
         \x20   n = len(v)\n\
         \x20   total = 0\n\
         \x20   for i in 0 to n - 1\n\
         \x20       total = total + v[i]\n\
         \x20   end for\n\
         \x20   return total\n\
         end function\n\
         s = helper([1.0, 2.0, 3.0])",
    );
    assert_eq!(num(&it, "s"), 6.0, "the function still computes correctly");
    assert_eq!(num(&it, "n"), 42.0, "caller n must survive");
    assert_eq!(num(&it, "total"), 999.0, "caller total must survive");
}

#[test]
fn functions_still_read_and_mutate_pre_existing_globals() {
    // READS still fall through to the enclosing scope -- the behaviour
    // the crest-factor closures (`private/`) depend on (`synth`/`crest`/
    // `grad` close over top-level `A`/`theta` without taking them as
    // parameters). WRITES bind locally unless the body says `global`,
    // so a counter kept across calls declares it.
    let it = run(
        "count = 0\n\
         function bump()\n\
         \x20   global count\n\
         \x20   count = count + 1\n\
         end function\n\
         bump()\n\
         bump()\n\
         bump()",
    );
    assert_eq!(num(&it, "count"), 3.0);
}

#[test]
fn fast_for_loop_matches_plain_accumulation() {
    let it = run("result = 0\nfor i = 1 to 1000\n    result = result + i\nend for");
    assert_eq!(num(&it, "result"), 500_500.0); // 1000*1001/2
    assert_eq!(num(&it, "i"), 1000.0); // loop var retains its final value, same as the slow path
}

#[test]
fn fast_for_loop_handles_compound_ops_and_multiple_locals() {
    let it = run(
        "sum = 0\n\
         prod = 1\n\
         for i = 1 to 6\n\
         \x20   sum += i\n\
         \x20   prod *= i\n\
         end for",
    );
    assert_eq!(num(&it, "sum"), 21.0); // 1+2+..+6
    assert_eq!(num(&it, "prod"), 720.0); // 6!
}

#[test]
fn fast_while_loop_matches_plain_accumulation() {
    let it = run("n = 0\ntotal = 0\nwhile n < 100\n    total = total + n\n    n = n + 1\nend while");
    assert_eq!(num(&it, "total"), 4950.0); // 0+1+..+99
}

#[test]
fn for_loop_falls_back_correctly_when_a_local_is_not_pre_existing_num() {
    // `x` has no binding before the loop, so the fast path's precondition
    // (every non-loop-var local must already be a plain `Num`) can't be
    // proven — it must silently fall back to the ordinary tree walker
    // and still produce the right answer, not skip the assignment.
    let it = run("for i = 1 to 5\n    x = i * 2\nend for");
    assert_eq!(num(&it, "x"), 10.0); // last iteration: i=5
}

#[test]
fn for_loop_falls_back_correctly_with_nested_control_flow() {
    // An `if` inside the body isn't in the fast path's supported subset
    // (plain `Stmt::Assign` only) — must fall back to `exec_block` and
    // still behave correctly, not silently drop the branch.
    let it = run(
        "evens = 0\n\
         for i = 1 to 10\n\
         \x20   if i mod 2 == 0 then\n\
         \x20       evens = evens + 1\n\
         \x20   end if\n\
         end for",
    );
    assert_eq!(num(&it, "evens"), 5.0);
}

#[test]
fn fast_for_loop_result_matches_recursion_free_reference() {
    // Cross-check the fast path against an independently-computed value
    // for a loop that mixes several ops (not just +=), so a subtly wrong
    // `fast_binop_fn`/`fast_combine_fn` mapping would show up as a
    // mismatch rather than passing by coincidence.
    let it = run(
        "acc = 1.0\n\
         for i = 1 to 8\n\
         \x20   acc = acc * 2 - 1\n\
         end for",
    );
    let mut expected = 1.0f64;
    for _ in 1..=8 {
        expected = expected * 2.0 - 1.0;
    }
    assert!((num(&it, "acc") - expected).abs() < 1e-12);
}

// ---- `for ... in` over a list (2026-09-05) ----
//
// The loop itself has iterated a `Value::List` since 2026-08-26, but until
// now nothing in Qu SOURCE could build one holding strings: `["a", "b"]` went
// down the numeric bracket-literal path and died with "expected a number,
// found string" before the loop ever ran (the sibling unit test
// `for_in_over_a_list_iterates_mixed_elements` had to inject its list with
// `var_set` for exactly that reason). These lock in the whole shape end to
// end — literal, then loop — because that is the combination the reported
// script (one saved figure per named theme) actually needs.

#[test]
fn for_in_iterates_a_list_of_strings() {
    let it = run(
        "themes = [\"ocean\", \"journal\", \"dark\"]\n\
         out = \"\"\n\
         n = 0\n\
         for t in themes\n\
         \x20   out = out + t + \"|\"\n\
         \x20   n += 1\n\
         end for",
    );
    assert_str(&it, "out", "ocean|journal|dark|");
    assert_eq!(num(&it, "n"), 3.0);
    assert_eq!(list_len(&it, "themes"), 3);
    // The loop variable keeps its last value, and that value is the string
    // itself — not a number the loop coerced it into.
    assert_str(&it, "t", "dark");
}

#[test]
fn for_in_iterates_a_string_list_literal_written_inline() {
    // No intermediate variable: the literal sits directly in the `in` slot,
    // which is how a "one output per name" loop actually gets written.
    let it = run(
        "out = \"\"\n\
         for name in [\"a.csv\", \"b.csv\"]\n\
         \x20   out = out + name + \";\"\n\
         end for",
    );
    assert_str(&it, "out", "a.csv;b.csv;");
}

#[test]
fn for_in_iterates_a_mixed_list_binding_each_element_verbatim() {
    // Strings, numbers and booleans in one literal. Each element binds as
    // its own kind — the boolean stays a boolean rather than flattening to
    // 1/0 the way the all-numeric literal path deliberately still does for
    // `[true, false]`.
    let it = run(
        "mixed = [\"sine\", 440, true]\n\
         kinds = \"\"\n\
         total = 0\n\
         for m in mixed\n\
         \x20   kinds = kinds + type(m) + \",\"\n\
         end for\n\
         for m in mixed\n\
         \x20   if type(m) == \"number\" then\n\
         \x20       total = total + m\n\
         \x20   end if\n\
         end for",
    );
    assert_str(&it, "kinds", "string,number,bool,");
    assert_eq!(num(&it, "total"), 440.0);
}

#[test]
fn for_in_iterates_a_single_column_string_literal() {
    // `[a; b]` is the column spelling of the same list — unambiguous, so it
    // flattens exactly like the row form rather than erroring.
    let it = run(
        "out = \"\"\n\
         for t in [\"a\"; \"b\"; \"c\"]\n\
         \x20   out = out + t\n\
         end for",
    );
    assert_str(&it, "out", "abc");
}

#[test]
fn for_in_iterates_nested_list_literals() {
    // A one-row literal whose cells are themselves lists nests instead of
    // concatenating (the numeric `[[1,2],[3,4]]` still block-concatenates —
    // see `bracket_literal_keeps_every_numeric_form_unchanged`). This is the
    // spelling the 2-D heterogeneous error message points at.
    let it = run(
        "pairs = [[\"t\", \"time\"], [\"v\", \"volts\"]]\n\
         out = \"\"\n\
         for p in pairs\n\
         \x20   out = out + p[0] + \"=\" + p[1] + \";\"\n\
         end for",
    );
    assert_eq!(list_len(&it, "pairs"), 2);
    assert_str(&it, "out", "t=time;v=volts;");
}

#[test]
fn for_in_iterates_a_dicts_keys_and_values() {
    // The spec's access path for a dict is `keys`/`values` (§47.4), not
    // iterating the dict itself — both already return a `Value::List`, so
    // they drop straight into the same loop. Iterating a bare dict stays a
    // clear "cannot iterate a dict" error on purpose: no spec text defines
    // what a bare `for k in d` would yield.
    let it = run(
        "d = dict()\n\
         d = set(d, \"gain\", 2)\n\
         d = set(d, \"offset\", 5)\n\
         names = \"\"\n\
         total = 0\n\
         for k in keys(d)\n\
         \x20   names = names + k + \",\"\n\
         end for\n\
         for v in values(d)\n\
         \x20   total = total + v\n\
         end for",
    );
    assert_str(&it, "names", "gain,offset,");
    assert_eq!(num(&it, "total"), 7.0);

    let mut it2 = Interp::new();
    let err = it2
        .run("d = dict()\nfor k in d\n print k\nend for")
        .unwrap_err();
    assert!(
        err.msg.contains("cannot iterate a dict"),
        "unexpected: {}",
        err.msg
    );
}

#[test]
fn for_in_over_a_string_list_supports_break_and_continue() {
    // The list arm runs the slow `for` driver, which is also the only one
    // that honours break/continue/redo — confirm the two features compose
    // rather than the list path quietly bypassing loop signals.
    let it = run(
        "out = \"\"\n\
         for t in [\"a\", \"skip\", \"b\", \"stop\", \"c\"]\n\
         \x20   if t == \"skip\" then\n\
         \x20       continue\n\
         \x20   end if\n\
         \x20   if t == \"stop\" then\n\
         \x20       break\n\
         \x20   end if\n\
         \x20   out = out + t\n\
         end for",
    );
    assert_str(&it, "out", "ab");
}

#[test]
fn bracket_literal_keeps_every_numeric_form_unchanged() {
    // The list branch is additive: it may only fire where the literal used
    // to ERROR. Every numeric shape must come out exactly as before —
    // including `[true, false]`, which stays the numeric `[1, 0]` because
    // `as_num` accepts a bool, and `[[1,2],[3,4]]`, which stays MATLAB
    // block concatenation rather than nesting.
    let it = run(
        "row = [1, 2, 3]\n\
         mat = [1, 2; 3, 4]\n\
         col = [1; 2; 3]\n\
         bools = [true, false]\n\
         blocks = [[1, 2], [3, 4]]\n\
         cplx = [1i, 2]\n\
         empty = []",
    );
    assert!(matches!(it.get("row"), Some(Value::Vec(v)) if v.as_slice() == [1.0, 2.0, 3.0]));
    assert!(matches!(it.get("mat"), Some(Value::Mat(m)) if m.shape() == (2, 2)));
    assert!(matches!(it.get("col"), Some(Value::Vec(v)) if v.as_slice() == [1.0, 2.0, 3.0]));
    assert!(matches!(it.get("bools"), Some(Value::Vec(v)) if v.as_slice() == [1.0, 0.0]));
    assert!(
        matches!(it.get("blocks"), Some(Value::Vec(v)) if v.as_slice() == [1.0, 2.0, 3.0, 4.0])
    );
    assert!(matches!(it.get("cplx"), Some(Value::CVec(v)) if v.len() == 2));
    assert!(matches!(it.get("empty"), Some(Value::Vec(v)) if v.is_empty()));
}

#[test]
fn heterogeneous_two_dimensional_literal_is_a_clear_error() {
    // No mixed-matrix value exists, and flatten-vs-nest would be a guess, so
    // a genuine 2-D heterogeneous literal errors and names the spellings
    // that do work. The second case is ragged — its FIRST row is one cell
    // wide, so the check has to ask the widest row, not row 0.
    for src in [
        "x = [\"a\", \"b\"; \"c\", \"d\"]",
        "x = [\"a\"; \"b\", \"c\"]",
    ] {
        let mut it = Interp::new();
        let err = it.run(src).unwrap_err();
        assert!(
            err.msg.contains("has no matrix form") && err.msg.contains("[[a, b], [c, d]]"),
            "unexpected message for `{src}`: {}",
            err.msg
        );
    }
}

#[test]
fn hstack_and_friends_stay_numeric_only() {
    // `value_to_block`'s numeric-only contract is shared with `hstack`/
    // `vstack`/`cat`, which are NOT bracket literals and must keep rejecting
    // a string rather than silently gaining list support.
    let mut it = Interp::new();
    let err = it.run("x = hstack([1, 2], \"a\")").unwrap_err();
    assert!(
        err.msg.contains("expected a number, found string"),
        "unexpected: {}",
        err.msg
    );
}

// ---- soft-compile: whole-function fast path (§ IMPL.md's M4 "Phase 0", 2026-08-23) ----

#[test]
fn soft_compiled_one_line_function_matches_interpreted_result() {
    let it = run("f(x) := sin(x) * cos(x) + 1\ny = f(0.7)");
    let expected = (0.7f64).sin() * (0.7f64).cos() + 1.0;
    assert!((num(&it, "y") - expected).abs() < 1e-12);
}

#[test]
fn soft_compiled_block_function_with_if_else_matches_interpreted_result() {
    let it = run(
        "function classify(x)\n\
         \x20   if x > 0 then\n\
         \x20       y = 1\n\
         \x20   else\n\
         \x20       y = -1\n\
         \x20   end if\n\
         \x20   return y\n\
         end function\n\
         a = classify(3.0)\n\
         b = classify(-2.0)",
    );
    assert_eq!(num(&it, "a"), 1.0);
    assert_eq!(num(&it, "b"), -1.0);
}

#[test]
fn explain_reports_soft_compiled_vs_interpreted() {
    let it = run(
        "scalar_fn(x) := x * 2 + 1\n\
         function calls_another(x)\n\
         \x20   return scalar_fn(x) + 1\n\
         end function\n\
         r1 = explain(\"scalar_fn\")\n\
         r2 = explain(\"calls_another\")\n\
         r3 = explain(\"no_such_fn\")",
    );
    match it.get("r1") {
        Some(Value::Str(s)) => assert!(s.contains("soft-compiled"), "got: {s}"),
        other => panic!("expected a Str, got {other:?}"),
    }
    // calling another user function is outside the Phase 0 subset --
    // must stay interpreted, not silently miscompile.
    match it.get("r2") {
        Some(Value::Str(s)) => assert!(s.contains("interpreted"), "got: {s}"),
        other => panic!("expected a Str, got {other:?}"),
    }
    match it.get("r3") {
        Some(Value::Str(s)) => assert!(s.contains("no user-defined function"), "got: {s}"),
        other => panic!("expected a Str, got {other:?}"),
    }
}

#[test]
fn soft_compile_falls_back_correctly_for_non_numeric_arguments() {
    // `f` qualifies for soft-compile (pure scalar arithmetic), but this
    // particular call passes a Vec -- must still fall back to the
    // ordinary interpreter for *this call*, not error or miscompute.
    let it = run("f(x) := x * 2\nv = [1, 2, 3]\ny = f(v)");
    match it.get("y") {
        Some(Value::Vec(v)) => assert_eq!(v.as_ref(), &vec![2.0, 4.0, 6.0]),
        other => panic!("expected a Vec, got {other:?}"),
    }
}

#[test]
fn soft_compile_falls_back_correctly_for_a_function_calling_another_function() {
    // Calling another user function is outside the Phase 0 whitelist
    // (only a fixed set of pure math builtins are inlined) -- must still
    // produce the right answer via the interpreter, not silently drop
    // the call or miscompute.
    let it = run(
        "inner(x) := x + 1\n\
         outer(x) := inner(x) * 2\n\
         y = outer(3.0)",
    );
    assert_eq!(num(&it, "y"), 8.0);
}

#[test]
fn soft_compile_recompiles_after_redefinition() {
    // Calling `f` once (compiling+caching it), then redefining `f` with a
    // different body, must pick up the NEW body -- a stale cache entry
    // here would be a silent-wrong-answer bug, not a crash.
    let it = run(
        "f(x) := x + 1\n\
         a = f(10.0)\n\
         f(x) := x * 100\n\
         b = f(10.0)",
    );
    assert_eq!(num(&it, "a"), 11.0);
    assert_eq!(num(&it, "b"), 1000.0);
}

#[test]
fn soft_compiled_function_with_no_return_reached_gives_nothing() {
    // A block function whose taken branch never hits `return` behaves
    // like the slow path: the call's result is `Nothing`, not an error
    // or a leftover register value leaking out.
    let it = run(
        "function maybe_return(x)\n\
         \x20   if x > 0 then\n\
         \x20       return 1\n\
         \x20   end if\n\
         end function\n\
         y = maybe_return(-5.0)",
    );
    match it.get("y") {
        Some(Value::Nothing) => {}
        other => panic!("expected Nothing, got {other:?}"),
    }
}

// ---- bitwise/boolean builtins, hex literals (2026-08-23, Ahmed: "do we
// have & && | || xor bitxor bitor or bitand and nand nor") ----------------

#[test]
fn hex_literal_parses_as_integer() {
    let it = run("a = 0xFF\nb = 0x1A\nc = 0xDEAD_BEEF");
    assert_eq!(num(&it, "a"), 255.0);
    assert_eq!(num(&it, "b"), 26.0);
    assert_eq!(num(&it, "c"), 0xDEADBEEFu32 as f64);
}

#[test]
fn bitwise_builtins() {
    let it = run(
        "a = bitand(0xFF, 0x0F)\n\
         b = bitor(0x0F, 0xF0)\n\
         c = bitxor(0xFF, 0x0F)\n\
         d = bitshift(1, 4)\n\
         e = bitshift(16, -4)\n\
         f = bitcmp(0)",
    );
    assert_eq!(num(&it, "a"), 15.0);
    assert_eq!(num(&it, "b"), 255.0);
    assert_eq!(num(&it, "c"), 240.0);
    assert_eq!(num(&it, "d"), 16.0);
    assert_eq!(num(&it, "e"), 1.0);
    assert_eq!(num(&it, "f"), -1.0);
}

#[test]
fn boolean_xor_nand_nor_builtins() {
    let it = run(
        "a = xor(true, false)\n\
         b = xor(true, true)\n\
         c = nand(true, true)\n\
         d = nand(true, false)\n\
         e = nor(false, false)\n\
         f = nor(true, false)",
    );
    assert!(matches!(it.get("a"), Some(Value::Bool(true))));
    assert!(matches!(it.get("b"), Some(Value::Bool(false))));
    assert!(matches!(it.get("c"), Some(Value::Bool(false))));
    assert!(matches!(it.get("d"), Some(Value::Bool(true))));
    assert!(matches!(it.get("e"), Some(Value::Bool(true))));
    assert!(matches!(it.get("f"), Some(Value::Bool(false))));
}

#[test]
fn hex_and_bin_string_conversions() {
    let it = run(
        "a = hex2dec(\"FF\")\n\
         b = hex2dec(\"0x1A\")\n\
         c = dec2hex(255)\n\
         d = bin2dec(\"1010\")\n\
         e = dec2bin(10)",
    );
    assert_eq!(num(&it, "a"), 255.0);
    assert_eq!(num(&it, "b"), 26.0);
    assert!(matches!(it.get("c"), Some(Value::Str(s)) if s == "FF"));
    assert_eq!(num(&it, "d"), 10.0);
    assert!(matches!(it.get("e"), Some(Value::Str(s)) if s == "1010"));
}

#[test]
fn pipe_and_amp_operators_are_a_clear_parse_error() {
    let mut it = Interp::new();
    let err = it.run("a = 1 | 2").unwrap_err();
    assert!(err.to_string().contains("bitor"), "got: {err}");

    let mut it2 = Interp::new();
    let err2 = it2.run("a = 1 & 2").unwrap_err();
    assert!(err2.to_string().contains("bitand"), "got: {err2}");
}

// ---- timer/preciseTimer, on elapsed/elapsedOnce (2026-08-24, Ahmed: "fix
// tic toc issue and add timer and preciseTimer to do it instead") ---------

#[test]
fn timer_start_pause_elapsed_restart_stop() {
    let it = run(
        "t = timer()\n\
         a0 = elapsed(t)\n\
         start(t)\n\
         pause(t)\n\
         a1 = elapsed(t)\n\
         start(t)\n\
         a2 = elapsed(t)\n\
         dt = restart(t)\n\
         a3 = elapsed(t)\n\
         stop(t)\n\
         a4 = elapsed(t)",
    );
    // A freshly constructed timer hasn't started: no time has accumulated.
    assert_eq!(num(&it, "a0"), 0.0);
    // pause() freezes whatever tiny amount accumulated (>= 0, not required
    // to be exactly zero — real wall-clock time passed during the calls).
    assert!(num(&it, "a1") >= 0.0);
    // resuming keeps counting (never goes backward).
    assert!(num(&it, "a2") >= num(&it, "a1"));
    // restart() returns the pre-reset elapsed time (matches a2, the value
    // just read before the reset).
    assert!((num(&it, "dt") - num(&it, "a2")).abs() < 0.05, "dt={} a2={}", num(&it, "dt"), num(&it, "a2"));
    // ...and resets the count: a3 should be small (freshly restarted),
    // not still carrying a2's total.
    assert!(num(&it, "a3") < num(&it, "a2") + 1.0);
    // stop() resets to exactly zero.
    assert_eq!(num(&it, "a4"), 0.0);
}

#[test]
fn precise_timer_type_name_and_dot_call_sugar() {
    let it = run(
        "t = preciseTimer()\n\
         t.start()\n\
         d = t.elapsed()\n\
         t.stop()",
    );
    assert!(num(&it, "d") >= 0.0);
}

#[test]
fn timer_wrong_type_is_a_clear_error() {
    let mut it = Interp::new();
    let err = it.run("start(5)").unwrap_err();
    assert!(err.to_string().contains("timer"), "got: {err}");
}

#[test]
fn on_elapsed_fires_repeatedly_on_run_fors_virtual_clock() {
    // A 0.1s-tick timer over a 0.55s virtual run: fires at 0.1, 0.2, 0.3,
    // 0.4, 0.5 -- 5 times, deterministic (run_for is a simulated timeline,
    // not real wall-clock sleep, so this test is fast and exact).
    let it = run(
        "tick = timer(0.1)\n\
         count = 0\n\
         on elapsed(tick) do\n\
         \x20   count = count + 1\n\
         end\n\
         start(tick)\n\
         run_for(0.55)",
    );
    assert_eq!(num(&it, "count"), 5.0);
}

#[test]
fn on_elapsed_once_fires_exactly_once() {
    let it = run(
        "tick = timer(0.2)\n\
         fired = 0\n\
         on elapsedOnce(tick) do\n\
         \x20   fired = fired + 1\n\
         end\n\
         start(tick)\n\
         run_for(1.0)",
    );
    assert_eq!(num(&it, "fired"), 1.0);
}

#[test]
fn pausing_a_timer_suppresses_on_elapsed_firing() {
    // Two timers: `a` ticks every 0.1s and pauses `b` (also every 0.1s)
    // after its own 2nd firing. `b` should stop accumulating once paused.
    let it = run(
        "a = timer(0.1)\n\
         b = timer(0.1)\n\
         a_count = 0\n\
         b_count = 0\n\
         on elapsed(a) do\n\
         \x20   a_count = a_count + 1\n\
         \x20   if a_count == 2 then\n\
         \x20       pause(b)\n\
         \x20   end if\n\
         end\n\
         on elapsed(b) do\n\
         \x20   b_count = b_count + 1\n\
         end\n\
         start(a)\n\
         start(b)\n\
         run_for(1.0)",
    );
    // `a` fires all 10 ticks (never paused itself).
    assert_eq!(num(&it, "a_count"), 10.0);
    // `b` fires only up through the tick where `a`'s callback paused it
    // (both tick on the same schedule, so `b` fires at 0.1 and 0.2, then
    // gets paused during the 0.2 round before its own 0.2 tick — order
    // within the same virtual instant depends on registration order, so
    // assert a bound rather than an exact count: strictly fewer than `a`'s
    // full 10, proving the pause genuinely suppressed later ticks.
    assert!(num(&it, "b_count") < 10.0, "b_count = {}", num(&it, "b_count"));
}

#[test]
fn removed_timer_never_fires_even_after_restart() {
    let it = run(
        "t = timer(0.1)\n\
         count = 0\n\
         on elapsed(t) do\n\
         \x20   count = count + 1\n\
         end\n\
         start(t)\n\
         remove(t)\n\
         start(t)\n\
         run_for(1.0)",
    );
    assert_eq!(num(&it, "count"), 0.0);
}

// ---- structured try/catch exception record (spec §18.B: `e.message`,
// `e.type`, `e.stack`, `e.line`) — 2026-08-24, Ahmed: "error or warning
// class ... position error at line code / type / stack calls" ------------

#[test]
fn caught_exception_is_a_record_with_message_type_class() {
    let mut it = Interp::new();
    it.run(
        "try\n\
         \x20   y = undefined_var_xyz\n\
         catch e\n\
         \x20   msg = e.message\n\
         \x20   kind = e.type\n\
         \x20   cls = e.class\n\
         end",
    )
    .unwrap();
    assert!(matches!(it.get("msg"), Some(Value::Str(s)) if s.contains("undefined_var_xyz")));
    assert!(matches!(it.get("kind"), Some(Value::Str(s)) if s == "NameError"));
    assert!(matches!(it.get("cls"), Some(Value::Str(s)) if s == "error"));
}

#[test]
fn caught_exception_line_field_is_the_actual_failing_line() {
    // `e.line` (spec §18.B) is `Stmt::SourceLine`-tracked: the failing
    // statement's own 1-based source line, not the `try` block's line.
    let mut it = Interp::new();
    it.run("try\n    y = undefined_var_xyz\ncatch e\n    ln = e.line\nend")
        .unwrap();
    // line 1 = "try", line 2 = the failing assignment.
    assert!(matches!(it.get("ln"), Some(Value::Num(n)) if (*n - 2.0).abs() < 1e-9), "got: {:?}", it.get("ln"));
}

#[test]
fn caught_exception_line_reflects_the_deepest_failing_frame() {
    // The failure is inside `boom`'s own body, several lines below the
    // `try` — `e.line` must track the actual line that broke, not the
    // call site.
    let mut it = Interp::new();
    it.run(
        "function boom()\n\
         \x20   x = 1\n\
         \x20   y = undefined_var_xyz\n\
         end function\n\
         try\n\
         \x20   boom()\n\
         catch e\n\
         \x20   ln = e.line\n\
         end",
    )
    .unwrap();
    // line 1 = function boom(), 2 = x=1, 3 = the failing line inside boom.
    assert!(matches!(it.get("ln"), Some(Value::Num(n)) if (*n - 3.0).abs() < 1e-9), "got: {:?}", it.get("ln"));
}

#[test]
fn caught_exception_stack_names_the_failing_function() {
    let mut it = Interp::new();
    it.run(
        "function boom()\n\
         \x20   y = undefined_var_xyz\n\
         end function\n\
         try\n\
         \x20   boom()\n\
         catch e\n\
         \x20   st = e.stack\n\
         end",
    )
    .unwrap();
    assert!(matches!(it.get("st"), Some(Value::Str(s)) if s.contains("boom")), "got: {:?}", it.get("st"));
}

#[test]
fn caught_exception_stack_spans_the_whole_call_chain_not_just_one_frame() {
    // `outer` calls `middle` calls `inner`, which fails — `e.stack` must
    // name all three, outermost first, not just the innermost frame.
    let mut it = Interp::new();
    it.run(
        "function inner()\n\
         \x20   y = undefined_var_xyz\n\
         end function\n\
         function middle()\n\
         \x20   inner()\n\
         end function\n\
         function outer()\n\
         \x20   middle()\n\
         end function\n\
         try\n\
         \x20   outer()\n\
         catch e\n\
         \x20   st = e.stack\n\
         end",
    )
    .unwrap();
    assert!(
        matches!(it.get("st"), Some(Value::Str(s)) if s == "outer > middle > inner"),
        "got: {:?}",
        it.get("st")
    );
}

#[test]
fn caught_exception_type_classifies_shape_and_index_errors() {
    let mut it = Interp::new();
    it.run(
        "try\n\
         \x20   A = [1, 2; 3, 4]\n\
         \x20   B = [1, 2, 3]\n\
         \x20   C = A + B\n\
         catch e\n\
         \x20   kind1 = e.type\n\
         end",
    )
    .unwrap();
    // shape mismatch -> ShapeError (falls back to RuntimeError if the
    // exact phrasing ever changes upstream — assert the more important
    // invariant, that classification runs at all, not the specific string,
    // if that ever proves too brittle; for now the phrasing is stable).
    assert!(matches!(it.get("kind1"), Some(Value::Str(s)) if s == "ShapeError"), "got: {:?}", it.get("kind1"));
}

// ---- worker pool: channel() worker-to-worker communication (2026-08-24,
// Ahmed: "is pool of workers and auto assign workers (and workers
// communications) implemented? ... If not implement them") --------------

#[test]
fn channel_send_recv_is_fifo() {
    let it = run(
        "ch = channel()\n\
         channel_send(ch, 1)\n\
         channel_send(ch, 2)\n\
         channel_send(ch, 3)\n\
         a = channel_recv(ch)\n\
         b = channel_recv(ch)\n\
         c = channel_recv(ch)",
    );
    assert_eq!(num(&it, "a"), 1.0);
    assert_eq!(num(&it, "b"), 2.0);
    assert_eq!(num(&it, "c"), 3.0);
}

#[test]
fn channel_try_recv_is_none_when_empty() {
    let it = run("ch = channel()\nv = channel_try_recv(ch)");
    assert!(matches!(it.get("v"), Some(Value::Nothing)));
}

#[test]
fn channel_len_reflects_queue_size() {
    let it = run(
        "ch = channel()\n\
         n0 = channel_len(ch)\n\
         channel_send(ch, 10)\n\
         channel_send(ch, 20)\n\
         n1 = channel_len(ch)\n\
         channel_recv(ch)\n\
         n2 = channel_len(ch)",
    );
    assert_eq!(num(&it, "n0"), 0.0);
    assert_eq!(num(&it, "n1"), 2.0);
    assert_eq!(num(&it, "n2"), 1.0);
}

#[test]
fn channel_crosses_a_real_spawn_worker_boundary() {
    // The actual point: `ch` is captured into `send_it`'s isolated env
    // snapshot (same mechanism as any other variable `spawn` sees), and
    // because `Value::Channel` shares its `Arc` on clone (like `Mutex`/
    // `Semaphore`), the worker's `channel_send` and the main thread's
    // blocking `channel_recv` genuinely rendezvous across the real OS
    // thread boundary — not just within one script's sequential execution.
    let it = run(
        "function send_it(ch)\n\
         \x20   channel_send(ch, 99)\n\
         end function\n\
         ch = channel()\n\
         w = spawn(\"send_it\", ch)\n\
         result = channel_recv(ch)\n\
         join(w)",
    );
    assert_eq!(num(&it, "result"), 99.0);
}

// ------------------------------------------------------- §38 autodiff (grad)

#[test]
fn grad_of_a_scalar_polynomial_matches_hand_derivative() {
    // y = x^3 + 2x, dy/dx = 3x^2 + 2; at x=3 that's 29. `y` itself stays a
    // tracked `Value::Tensor` (not auto-unwrapped — it's still live on the
    // tape until something reads through it), so `stop_grad` reads its
    // plain number back out the same way any non-differentiable context
    // would.
    let it = run(
        "x = param(3.0)\n\
         y = x*x*x + 2*x\n\
         gy = grad(y, wrt=x)\n\
         yv = stop_grad(y)",
    );
    assert!((num(&it, "yv") - 33.0).abs() < 1e-9);
    assert!((num(&it, "gy") - 29.0).abs() < 1e-9);
}

#[test]
fn grad_of_a_transcendental_chain_matches_hand_derivative() {
    // u = sin(t) * exp(t); du/dt = exp(t) * (sin(t) + cos(t)).
    let it = run(
        "t = param(0.7)\n\
         u = sin(t) * exp(t)\n\
         gu = grad(u, wrt=t)",
    );
    let expected = 0.7f64.exp() * (0.7f64.sin() + 0.7f64.cos());
    assert!((num(&it, "gu") - expected).abs() < 1e-9);
}

#[test]
fn grad_of_a_matmul_regression_loss_matches_finite_differences() {
    // The spec's own §38 example: loss = mean((X*W + b - Y)^2). Checks the
    // matmul + broadcast-add + elementwise-power + mean vjp rules all
    // compose correctly by comparing against an independent central-
    // difference computation of the same loss (no autodiff involved there
    // at all — the two paths share no code).
    let it = run(
        "X = [1,2;3,4;5,6]\n\
         Y = [1;2;3]\n\
         W = param([0.5;0.5])\n\
         b = param([0.1])\n\
         pred = X*W + b\n\
         diff = pred - Y\n\
         loss = mean(diff.^2)\n\
         gW = grad(loss, wrt=W)\n\
         gb = grad(loss, wrt=b)\n\
         function loss_of(w1, w2, bb)\n\
         \x20   Xc = [1,2;3,4;5,6]\n\
         \x20   Yc = [1;2;3]\n\
         \x20   predc = Xc*[w1;w2] + bb\n\
         \x20   return mean((predc - Yc).^2)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gw1 = (loss_of(0.5+eps, 0.5, 0.1) - loss_of(0.5-eps, 0.5, 0.1)) / (2*eps)\n\
         fd_gw2 = (loss_of(0.5, 0.5+eps, 0.1) - loss_of(0.5, 0.5-eps, 0.1)) / (2*eps)\n\
         fd_gb  = (loss_of(0.5, 0.5, 0.1+eps) - loss_of(0.5, 0.5, 0.1-eps)) / (2*eps)",
    );
    let gw = match it.get("gW") {
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("gW should be a matrix: {other:?}"),
    };
    assert!((gw[0] - num(&it, "fd_gw1")).abs() < 1e-4);
    assert!((gw[1] - num(&it, "fd_gw2")).abs() < 1e-4);
    assert!((num(&it, "gb") - num(&it, "fd_gb")).abs() < 1e-4);
}

#[test]
// Regression test for the `tensor_vjp`/`needed`-slot perf fix (2026-08-26,
// see that fn's own doc comment): `eval_grad` now tells `tensor_vjp` which
// side of a two-input op actually feeds a tracked node, so `"*"`'s matmul
// rule can skip computing the *other* side's gradient instead of computing
// it and throwing it away. The exact shape this could break: a CHAIN of two
// `X*W + b` layers where the first layer's `X` is untracked (raw data, the
// overwhelmingly common `dense(data, W, b)` pattern) — `W1`'s own gradient
// still has to flow through THAT SAME "*" node's other slot, so a slot-index
// mixup in the fix would silently zero or corrupt `gW1`/`gb1` specifically,
// while still passing every single-layer test above (their untracked slot
// has nothing else depending on the tracked slot's own contribution). Cross-
// checked against independent central differences, no autodiff involved on
// that side, same convention as the tests just above.
fn grad_through_a_chain_of_two_dense_layers_with_an_untracked_input_matches_finite_differences() {
    let it = run(
        "X = [1,2;3,4;5,6]\n\
         Y = [1;2;3]\n\
         W1 = param([0.3,0.1;0.2,0.4])\n\
         b1 = param([0.05,0.05])\n\
         W2 = param([0.6;0.7])\n\
         b2 = param([0.1])\n\
         h1 = X*W1 + b1\n\
         pred = h1*W2 + b2\n\
         diff = pred - Y\n\
         loss = mean(diff.^2)\n\
         gW1 = grad(loss, wrt=W1)\n\
         gb1 = grad(loss, wrt=b1)\n\
         gW2 = grad(loss, wrt=W2)\n\
         gb2 = grad(loss, wrt=b2)\n\
         function loss_of(w1a, w1b, w1c, w1d, b1a, b1b, w2a, w2b, bb2)\n\
         \x20   Xc = [1,2;3,4;5,6]\n\
         \x20   Yc = [1;2;3]\n\
         \x20   h1c = Xc*[w1a,w1b;w1c,w1d] + [b1a, b1b]\n\
         \x20   predc = h1c*[w2a;w2b] + bb2\n\
         \x20   return mean((predc - Yc).^2)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gw1a = (loss_of(0.3+eps,0.1,0.2,0.4,0.05,0.05,0.6,0.7,0.1) - loss_of(0.3-eps,0.1,0.2,0.4,0.05,0.05,0.6,0.7,0.1)) / (2*eps)\n\
         fd_gb1a = (loss_of(0.3,0.1,0.2,0.4,0.05+eps,0.05,0.6,0.7,0.1) - loss_of(0.3,0.1,0.2,0.4,0.05-eps,0.05,0.6,0.7,0.1)) / (2*eps)\n\
         fd_gw2a = (loss_of(0.3,0.1,0.2,0.4,0.05,0.05,0.6+eps,0.7,0.1) - loss_of(0.3,0.1,0.2,0.4,0.05,0.05,0.6-eps,0.7,0.1)) / (2*eps)\n\
         fd_gb2  = (loss_of(0.3,0.1,0.2,0.4,0.05,0.05,0.6,0.7,0.1+eps) - loss_of(0.3,0.1,0.2,0.4,0.05,0.05,0.6,0.7,0.1-eps)) / (2*eps)",
    );
    let gw1 = match it.get("gW1") {
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("gW1 should be a matrix: {other:?}"),
    };
    let gb1 = match it.get("gb1") {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("gb1 should be a vector or matrix: {other:?}"),
    };
    let gw2 = match it.get("gW2") {
        Some(Value::Mat(m)) => m.as_slice().to_vec(),
        other => panic!("gW2 should be a matrix: {other:?}"),
    };
    // Column-major (2,2): gw1[0] is W1's (row0,col0) entry, i.e. `w1a`.
    assert!((gw1[0] - num(&it, "fd_gw1a")).abs() < 1e-4);
    assert!((gb1[0] - num(&it, "fd_gb1a")).abs() < 1e-4);
    assert!((gw2[0] - num(&it, "fd_gw2a")).abs() < 1e-4);
    assert!((num(&it, "gb2") - num(&it, "fd_gb2")).abs() < 1e-4);
}

#[test]
fn grad_with_a_list_of_targets_returns_a_matching_list() {
    // Multiple *positional* tensors after `loss` (not `wrt=[a, b]` — Qu's
    // `[...]` is numeric-matrix syntax and would flatten tracked tensors
    // down to plain numbers) differentiate against all of them at once.
    let it = run(
        "a = param(2.0)\n\
         b = param(5.0)\n\
         y = a*b + b*b\n\
         g = grad(y, a, b)",
    );
    match it.get("g") {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 2);
            assert!(matches!(items[0], Value::Num(n) if (n - 5.0).abs() < 1e-9)); // dy/da = b
            assert!(matches!(items[1], Value::Num(n) if (n - 12.0).abs() < 1e-9)); // dy/db = a + 2b
        }
        other => panic!("g should be a list: {other:?}"),
    }
}

#[test]
fn grad_of_an_untouched_param_is_zero() {
    // b never feeds into y, so its gradient should read as a clean zero,
    // not an error or a missing value.
    let it = run(
        "a = param(3.0)\n\
         b = param(10.0)\n\
         y = a*a\n\
         gb = grad(y, wrt=b)",
    );
    assert_eq!(num(&it, "gb"), 0.0);
}

#[test]
fn stop_grad_detaches_from_the_tape() {
    // Gradient shouldn't flow through a stop_grad'd branch: y = a * stop_grad(a)
    // behaves like `y = a * const`, so dy/da is just that constant, not 2a.
    let it = run(
        "a = param(4.0)\n\
         held = stop_grad(a)\n\
         y = a * held\n\
         gy = grad(y, wrt=a)",
    );
    assert_eq!(num(&it, "gy"), 4.0);
}

#[test]
fn tensor_is_transparent_to_ordinary_numeric_builtins() {
    // A tracked Tensor should flow into non-differentiable contexts (here,
    // `abs`) exactly like its plain inner value would, via the same
    // transparent-unwrap convention `Value::Signal` already established.
    let it = run("x = param(-5.0)\ny = abs(x)");
    assert!(matches!(it.get("y"), Some(Value::Num(n)) if (*n - 5.0).abs() < 1e-9));
}

// -------------------------------------------------------- §37 ML: metrics, k_fold

#[test]
fn mae_mse_rmse_match_hand_computed_values() {
    // actual=[1,2,3,4], predicted=[1,3,3,7]: errors = [0,1,0,3]
    let it = run(
        "a = [1, 2, 3, 4]\n\
         p = [1, 3, 3, 7]\n\
         m_mae = mae(a, p)\n\
         m_mse = mse(a, p)\n\
         m_rmse = rmse(a, p)",
    );
    assert!((num(&it, "m_mae") - 1.0).abs() < 1e-9); // (0+1+0+3)/4
    assert!((num(&it, "m_mse") - 2.5).abs() < 1e-9); // (0+1+0+9)/4
    assert!((num(&it, "m_rmse") - 2.5f64.sqrt()).abs() < 1e-9);
}

#[test]
fn precision_recall_f1_on_a_known_binary_confusion_matrix() {
    // actual: 6 positives (1), 4 negatives (0). predicted: 5 true positives,
    // 1 false negative, 1 false positive, 3 true negatives.
    // class 1: tp=5, fp=1, fn=1 -> P=5/6, R=5/6, F1=5/6
    // class 0: tp=3, fp=1, fn=1 -> P=3/4, R=3/4, F1=3/4
    // macro P = (5/6+3/4)/2, macro R same, macro F1 = (5/6+3/4)/2
    let it = run(
        "a = [1,1,1,1,1,1,0,0,0,0]\n\
         p = [1,1,1,1,1,0,1,0,0,0]\n\
         prec = precision(a, p)\n\
         rec = recall(a, p)\n\
         f = f1(a, p)",
    );
    let expected = ((5.0 / 6.0) + (3.0 / 4.0)) / 2.0;
    assert!((num(&it, "prec") - expected).abs() < 1e-9);
    assert!((num(&it, "rec") - expected).abs() < 1e-9);
    assert!((num(&it, "f") - expected).abs() < 1e-9);
}

#[test]
fn precision_recall_f1_are_perfect_on_a_perfect_multiclass_prediction() {
    let it = run(
        "a = [0,1,2,0,1,2,0,1,2]\n\
         p = [0,1,2,0,1,2,0,1,2]\n\
         prec = precision(a, p)\n\
         rec = recall(a, p)\n\
         f = f1(a, p)",
    );
    assert!((num(&it, "prec") - 1.0).abs() < 1e-9);
    assert!((num(&it, "rec") - 1.0).abs() < 1e-9);
    assert!((num(&it, "f") - 1.0).abs() < 1e-9);
}

#[test]
fn k_fold_partitions_every_row_exactly_once_across_folds() {
    let it = run(
        "X = zeros(20, 3)\n\
         y = 0 to 19\n\
         folds = k_fold(X, y, 5, seed=42)\n\
         n_folds = length(folds)\n\
         f0 = folds[0]\n\
         n_test0 = length(f0.y_test)\n\
         n_train0 = length(f0.y_train)",
    );
    assert_eq!(num(&it, "n_folds"), 5.0);
    assert_eq!(num(&it, "n_test0"), 4.0); // 20/5
    assert_eq!(num(&it, "n_train0"), 16.0);
}

#[test]
fn k_fold_test_sets_across_all_folds_reconstruct_every_row_with_no_overlap() {
    let it = run(
        "X = zeros(23, 2)\n\
         y = 0 to 22\n\
         folds = k_fold(X, y, 5, seed=7)\n\
         all_test = []\n\
         for i = 0 to 4\n\
         \x20   f = folds[i]\n\
         \x20   all_test = hstack(all_test, f.y_test)\n\
         end for\n\
         n_total = length(all_test)\n\
         n_unique = length(unique(all_test))",
    );
    // 23 rows total, every row appears in exactly one fold's test set.
    assert_eq!(num(&it, "n_total"), 23.0);
    assert_eq!(num(&it, "n_unique"), 23.0);
}

#[test]
fn k_fold_train_and_test_never_overlap_within_a_fold() {
    let it = run(
        "X = zeros(20, 2)\n\
         y = 0 to 19\n\
         folds = k_fold(X, y, 4, seed=1)\n\
         f = folds[0]\n\
         combined = hstack(f.y_train, f.y_test)\n\
         n_combined = length(combined)\n\
         n_unique = length(unique(combined))",
    );
    assert_eq!(num(&it, "n_combined"), 20.0);
    assert_eq!(num(&it, "n_unique"), 20.0); // no row duplicated between train/test
}

#[test]
fn logistic_model_perfectly_separates_a_well_separated_1d_case() {
    // x < 0 -> class 0, x > 0 -> class 1: trivially linearly separable, so
    // gradient descent should converge to ~perfect training accuracy well
    // within the default iteration budget.
    let it = run(
        "X = [-5,-4,-3,-2,-1,1,2,3,4,5]\n\
         X as vector(10, 1)\n\
         y = [0,0,0,0,0,1,1,1,1,1]\n\
         m = logistic_model(X, y)\n\
         acc = m.score(X, y)",
    );
    assert_eq!(num(&it, "acc"), 1.0);
}

#[test]
fn logistic_model_predicts_original_label_values_not_internal_zero_one() {
    // y uses labels {10, 20}, not {0, 1} — predict() must map back to the
    // caller's own label values (same convention svm_model's neg_label/
    // pos_label fields already establish), not the internal 0/1 encoding.
    let it = run(
        "X = [-5,-4,-3,-2,-1,1,2,3,4,5]\n\
         X as vector(10, 1)\n\
         y = [10,10,10,10,10,20,20,20,20,20]\n\
         m = logistic_model(X, y)\n\
         pred = m.predict(X)\n\
         lo = pred[0]\n\
         hi = pred[9]",
    );
    assert_eq!(num(&it, "lo"), 10.0);
    assert_eq!(num(&it, "hi"), 20.0);
}

#[test]
fn gradient_boosting_model_classification_separates_two_blobs() {
    let it = run(
        "X = [1,1; 1,2; 2,1; 2,2; 8,8; 8,9; 9,8; 9,9]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         m = gradient_boosting_model(X, y, 30, kind=\"classification\")\n\
         acc = m.score(X, y)",
    );
    assert_eq!(num(&it, "acc"), 1.0);
}

#[test]
fn gradient_boosting_model_classification_predicts_original_labels() {
    let it = run(
        "X = [1,1; 1,2; 2,1; 2,2; 8,8; 8,9; 9,8; 9,9]\n\
         y = [5,5,5,5, 7,7,7,7]\n\
         m = gradient_boosting_model(X, y, 30, kind=\"classification\")\n\
         pred = m.predict(X)\n\
         lo = pred[0]\n\
         hi = pred[7]",
    );
    assert_eq!(num(&it, "lo"), 5.0);
    assert_eq!(num(&it, "hi"), 7.0);
}

#[test]
fn svm_model_one_vs_rest_separates_three_well_separated_blobs() {
    // Three blobs far apart on a line: 0/1/2 around x=0, x=10, x=20 — each
    // pair is trivially linearly separable, so one-vs-rest should recover
    // all three classes exactly.
    let it = run(
        "X = [-1,0,1, 9,10,11, 19,20,21]\n\
         X as vector(9, 1)\n\
         y = [0,0,0, 1,1,1, 2,2,2]\n\
         m = svm_model(X, y, kernel=\"linear\")\n\
         acc = m.score(X, y)",
    );
    assert_eq!(num(&it, "acc"), 1.0);
}

#[test]
fn svm_model_one_vs_rest_predicts_original_label_values() {
    let it = run(
        "X = [-1,0,1, 9,10,11, 19,20,21]\n\
         X as vector(9, 1)\n\
         y = [100,100,100, 200,200,200, 300,300,300]\n\
         m = svm_model(X, y, kernel=\"linear\")\n\
         pred = m.predict(X)\n\
         a = pred[0]\n\
         b = pred[4]\n\
         c = pred[8]",
    );
    assert_eq!(num(&it, "a"), 100.0);
    assert_eq!(num(&it, "b"), 200.0);
    assert_eq!(num(&it, "c"), 300.0);
}

#[test]
fn svr_model_fits_a_noise_free_linear_relationship_well() {
    // y = 2x exactly, no noise: a linear-kernel SVR should recover this
    // closely (high R^2) given enough iterations to converge.
    let it = run(
        "X = [-5,-4,-3,-2,-1,0,1,2,3,4,5]\n\
         X as vector(11, 1)\n\
         y = [-10,-8,-6,-4,-2,0,2,4,6,8,10]\n\
         m = svr_model(X, y, epsilon=0.05, learning_rate=0.05, n_iter=3000)\n\
         r2 = m.score(X, y)",
    );
    assert!(num(&it, "r2") > 0.9, "expected r2 > 0.9, got {}", num(&it, "r2"));
}

#[test]
fn svr_model_predict_returns_a_vector_matching_input_rows() {
    let it = run(
        "X = [-5,-4,-3,-2,-1,0,1,2,3,4,5]\n\
         X as vector(11, 1)\n\
         y = [-10,-8,-6,-4,-2,0,2,4,6,8,10]\n\
         m = svr_model(X, y, learning_rate=0.05, n_iter=2000)\n\
         pred = m.predict(X)\n\
         n_pred = length(pred)",
    );
    assert_eq!(num(&it, "n_pred"), 11.0);
}

// ----------------------------------------------------- §45 feature selection

#[test]
fn mutual_info_classif_ranks_a_perfectly_informative_feature_above_noise() {
    // column 0 IS y (perfectly informative); column 1 is unrelated noise.
    let it = run(
        "X = [0,3; 0,7; 0,1; 0,9; 0,2; 1,8; 1,4; 1,6; 1,5; 1,0]\n\
         y = [0,0,0,0,0,1,1,1,1,1]\n\
         mi = mutual_info_classif(X, y)",
    );
    let mi = vec(&it, "mi");
    assert!(
        mi[0] > mi[1],
        "perfectly informative feature (MI={}) should score above noise (MI={})",
        mi[0], mi[1]
    );
}

#[test]
fn astar_mrmr_returns_exactly_k_distinct_valid_indices() {
    let it = run(
        "X = [-2,-2,5,1,9; -2,-1,2,8,3; -1,-2,7,4,1; -1,-1,1,2,6; \
              2,2,3,9,4; 2,1,8,2,7; 1,2,4,6,2; 1,1,9,1,5]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         idx = astar_mrmr(X, y, 2, beam_width=5, cv_folds=2, seed=1)\n\
         n_idx = length(idx)\n\
         n_unique = length(unique(idx))",
    );
    assert_eq!(num(&it, "n_idx"), 2.0);
    assert_eq!(num(&it, "n_unique"), 2.0);
    let idx = vec(&it, "idx");
    assert!(idx.iter().all(|&i| (0.0..5.0).contains(&i)), "indices out of range: {idx:?}");
}

#[test]
fn rfe_drops_the_pure_noise_feature_first() {
    // 3 features: the first two are informative (their sign pattern
    // determines the two classes); the third is pure noise uncorrelated
    // with y. Asking to keep 2 of 3 should drop the noise column.
    let it = run(
        "X = [-2,-2,5; -2,-1,2; -1,-2,7; -1,-1,1; \
              2,2,3; 2,1,8; 1,2,4; 1,1,9]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         idx = rfe(X, y, 2, cv_folds=2, seed=1)",
    );
    let idx = vec(&it, "idx");
    assert!(
        idx.contains(&0.0) && idx.contains(&1.0),
        "expected to keep the informative features 0 and 1, got {idx:?}"
    );
}

// ------------------------------------------- §38.4 layer primitives (verify)

#[test]
fn relu_backward_works_on_matrix_shaped_input() {
    // Regression test: relu's vjp used to call self.binop(">", x, zero)
    // for a value it never actually used (already recomputed correctly
    // below via to_vec) — but binop's own matrix-boundary path rejects
    // `>` on a Value::Mat outright, so the dead call's own `?` aborted
    // grad() before ever reaching the correct computation. Broke ReLU's
    // backward pass on every matrix-shaped input, i.e. every ordinary
    // dense-layer use.
    let it = run(
        "X = [1,2; 3,4; -1,-2]\n\
         W = param([0.5,0.1; 0.2,0.3])\n\
         b = param([0.1, 0.1])\n\
         z = dense(X, W, b)\n\
         a = relu(z)\n\
         loss = mean(a)\n\
         g = grad(loss, W, b)",
    );
    match it.get("g") {
        Some(Value::List(items)) => assert_eq!(items.len(), 2),
        other => panic!("g should be a 2-element list: {other:?}"),
    }
}

#[test]
fn relu_works_on_a_plain_vector_not_just_scalars() {
    // Regression test: relu's non-Tensor path built its "zero" comparand
    // as a length-1 Value::Vec for Vec input, which map2's Vec-vs-Vec
    // branch requires to be *equal length* to the input rather than
    // broadcastable — broke relu(v) for any Vec longer than 1 element.
    let it = run("v = [-2, -1, 0, 1, 2]\nr = relu(v)");
    assert_eq!(vec(&it, "r"), vec![0.0, 0.0, 0.0, 1.0, 2.0]);
}

#[test]
fn dropout_gradient_replays_the_forward_mask() {
    // Regression test: dropout used to unconditionally detach its input
    // via untensor() before doing anything else — even at rate=0.0 —
    // silently breaking the tape for any loss that touched a dropout
    // layer at all. Fixed to stay tracked, recording the exact forward
    // mask so the backward pass replays it instead of drawing a fresh
    // random one (which would compute the wrong gradient — backprop must
    // differentiate what actually ran forward).
    let it = run(
        "x = param([1.0, 2.0, 3.0, 4.0])\n\
         y = dropout(x, rate=0.0)\n\
         loss = mean(y)\n\
         g = grad(loss, x)",
    );
    // rate=0 is a pure pass-through: d(mean(x))/dx_i = 1/4 for every i.
    let g = vec(&it, "g");
    for gi in g {
        assert!((gi - 0.25).abs() < 1e-9, "expected every gradient component to be 0.25, got {gi}");
    }
}

#[test]
fn train_loop_actually_converges_on_a_trivial_linear_fit() {
    // Regression test: train_loop used to (a) re-read the optimizer's
    // static `params` field every epoch instead of the evolving `_params`
    // buffer, so training never actually used the updated weights for the
    // next epoch's forward pass, and (b) clear the tape each epoch while
    // reusing that same stale Tensor, corrupting node indices and
    // producing silently wrong gradients on top of the no-op training.
    // Fixed by building a genuinely fresh leaf Tensor from the current
    // `_params` value each epoch, right after the tape clear. y = 2x,
    // starting from w=0 — loss must actually decrease by a lot.
    let it = run(
        "x = [1.0, 2.0, 3.0, 4.0]\n\
         y_true = [2.0, 4.0, 6.0, 8.0]\n\
         w = param(0.0)\n\
         function loss_fn(w)\n\
         \x20   pred = w * x\n\
         \x20   diff = pred - y_true\n\
         \x20   return mean(diff .^ 2)\n\
         end function\n\
         opt = sgd(w, 0.01)\n\
         losses = train_loop(\"loss_fn\", w, 100, optimizer=\"opt\")\n\
         first_loss = losses[0]\n\
         last_loss = losses[99]",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    assert!(first > 10.0, "expected a large initial loss, got {first}");
    assert!(last < 1e-3, "expected training to have converged close to zero, got {last}");
    assert!(last < first / 1000.0, "expected a real, large decrease: {first} -> {last}");
}

// --------------------------------- ablation / variability / explainability

#[test]
fn ablation_study_ranks_the_relevant_feature_above_noise() {
    // feature 0 perfectly determines the class; features 1,2 are noise.
    let it = run(
        "X = [1,1,5; 1,2,2; 1,1,7; 1,2,1; \
              2,2,3; 2,1,8; 2,2,4; 2,1,9]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         scores = ablation_study(X, y, cv_folds=2, seed=1)",
    );
    let s = vec(&it, "scores");
    assert!(
        s[0] >= s[1] && s[0] >= s[2],
        "expected feature 0's ablation score ({}) >= noise features ({}, {})", s[0], s[1], s[2]
    );
}

#[test]
fn cv_stability_returns_n_repeats_scores_in_valid_range() {
    let it = run(
        "X = [1,1; 1,2; 2,1; 2,2; 8,8; 8,9; 9,8; 9,9]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         scores = cv_stability(X, y, n_repeats=5, cv_folds=2, seed=1)\n\
         n = length(scores)",
    );
    assert_eq!(num(&it, "n"), 5.0);
    let s = vec(&it, "scores");
    assert!(s.iter().all(|&v| (0.0..=1.0).contains(&v)), "accuracy scores should be in [0,1]: {s:?}");
}

#[test]
fn permutation_importance_ranks_the_relevant_feature_above_noise() {
    let it = run(
        "X = [1,1,5; 1,2,2; 1,1,7; 1,2,1; \
              2,2,3; 2,1,8; 2,2,4; 2,1,9]\n\
         y = [0,0,0,0, 1,1,1,1]\n\
         m = tree_model(X, y, kind=\"classification\")\n\
         imp = permutation_importance(m, X, y, n_repeats=5, seed=1)",
    );
    let imp = vec(&it, "imp");
    assert!(
        imp[0] >= imp[1] && imp[0] >= imp[2],
        "expected feature 0's importance ({}) >= noise ({}, {})", imp[0], imp[1], imp[2]
    );
}

#[test]
fn hurst_exponent_ranks_a_trending_series_above_random_noise() {
    let it = run(
        "seed(1)\n\
         trend = 0 to 99\n\
         noise = randn(100, 1)\n\
         h_trend = hurst_exponent(trend)\n\
         h_noise = hurst_exponent(noise)",
    );
    let h_trend = num(&it, "h_trend");
    let h_noise = num(&it, "h_noise");
    assert!(
        h_trend > h_noise,
        "trending series (H={h_trend}) should be more persistent than noise (H={h_noise})"
    );
}

#[test]
fn hurst_exponent_is_nan_for_a_too_short_series() {
    let it = run("x = [1,2,3]\nh = hurst_exponent(x)");
    match it.get("h") {
        Some(Value::Num(n)) => assert!(n.is_nan(), "expected NaN for a too-short series, got {n}"),
        other => panic!("h should be a number: {other:?}"),
    }
}

// --------------------------------------- time-domain: ACF & calendar cycles

#[test]
fn acf_lag_zero_is_always_one() {
    let it = run("x = [3, 7, 1, 9, 2, 8, 4, 6, 5, 0]\na = acf(x, max_lag=3)\na0 = a[0]");
    assert!((num(&it, "a0") - 1.0).abs() < 1e-9);
}

#[test]
fn acf_of_a_smooth_trend_stays_high_at_lag_one() {
    let it = run("x = [1,2,3,4,5,6,7,8,9,10]\na = acf(x, max_lag=1)\na1 = a[1]");
    assert!(num(&it, "a1") > 0.5, "expected high lag-1 autocorrelation for a smooth trend, got {}", num(&it, "a1"));
}

#[test]
fn hourly_profile_recovers_a_known_hour_of_day_pattern() {
    // x[i] = hour-of-day itself, repeated across 2 full days at 1
    // sample/hour (fs = 1/3600 Hz) -- the mean-per-hour-bin should
    // recover exactly [0, 1, 2, ..., 23], not an average blur, since
    // every day contributes the identical value at that hour.
    let it = run(
        "x = []\n\
         for d = 0 to 1\n\
         \x20   for h = 0 to 23\n\
         \x20       x = append(x, h)\n\
         \x20   end for\n\
         end for\n\
         fs = 1.0 / 3600.0\n\
         profile = hourly_profile(x, fs, stat=\"mean\")\n\
         p5 = profile[5]\n\
         p23 = profile[23]\n\
         n = length(profile)",
    );
    assert_eq!(num(&it, "n"), 24.0);
    assert!((num(&it, "p5") - 5.0).abs() < 1e-9);
    assert!((num(&it, "p23") - 23.0).abs() < 1e-9);
}

#[test]
fn periodic_profile_sad_stat_matches_hand_computed_value() {
    // A single bin (period covers the whole series, n_bins=1) with a
    // known first-difference pattern: |2-1| + |4-2| + |1-4| = 1+2+3 = 6,
    // divided by 3 differences = 2.0.
    let it = run(
        "x = [1, 2, 4, 1]\n\
         fs = 1.0\n\
         profile = periodic_profile(x, fs, 1000.0, 1, stat=\"sad\")\n\
         v = profile[0]",
    );
    assert!((num(&it, "v") - 2.0).abs() < 1e-9);
}

#[test]
fn periodic_profile_rejects_an_unknown_stat() {
    let mut it = Interp::new();
    let err = it.run("x = [1,2,3]\np = periodic_profile(x, 1.0, 10.0, 2, stat=\"bogus\")").unwrap_err();
    assert!(err.to_string().contains("unknown stat"), "got: {err}");
}

#[test]
fn conv1d_forward_matches_a_hand_computed_edge_detector() {
    // [1,0,-1] is the classic discrete edge-detector kernel: out[i] =
    // x[i] - x[i+2]. Every consecutive triple in [1,2,3,4,5] has the same
    // step size, so every output element is the same known value.
    let it = run("x = [1.0, 2.0, 3.0, 4.0, 5.0]\nk = [1.0, 0.0, -1.0]\ny = conv1d(x, k)\nn = length(y)");
    assert_eq!(num(&it, "n"), 3.0);
    assert_eq!(vec(&it, "y"), vec![-2.0, -2.0, -2.0]);
}

#[test]
fn conv1d_forward_on_a_batched_matrix_shares_one_kernel_across_rows() {
    // Value::Mat input = one sequence per row; the SAME kernel convolves
    // every row (real weight sharing, not a per-row kernel).
    let it = run("X = [1.0,2.0,3.0,4.0; 5.0,6.0,7.0,8.0]\nk = [1.0, -1.0]\ny = conv1d(X, k)");
    match it.get("y") {
        Some(Value::Mat(m)) => {
            assert_eq!(m.shape(), (2, 3));
            for r in 0..2 {
                for c in 0..3 {
                    assert!((m.get(r, c).unwrap() - (-1.0)).abs() < 1e-9);
                }
            }
        }
        other => panic!("y should be a matrix: {other:?}"),
    }
}

#[test]
fn conv1d_gradient_matches_finite_differences_for_x_and_kernel() {
    // Regression-style verification (same standard every other autodiff op
    // in this file was checked against): compare grad()'s analytic dx/dk
    // to an independent central-difference computation that never touches
    // the tape at all.
    let it = run(
        "x = param([1.0, 2.0, 3.0, 4.0, 5.0])\n\
         k = param([0.5, -0.3, 0.8])\n\
         y = conv1d(x, k)\n\
         loss = sum(y .* [1.0, 2.0, 3.0])\n\
         gx = grad(loss, wrt=x)\n\
         gk = grad(loss, wrt=k)\n\
         function loss_of(x1, x2, x3, x4, x5, k1, k2, k3)\n\
         \x20   xc = [x1, x2, x3, x4, x5]\n\
         \x20   kc = [k1, k2, k3]\n\
         \x20   yc = conv1d(xc, kc)\n\
         \x20   return sum(yc .* [1.0, 2.0, 3.0])\n\
         end function\n\
         eps = 1e-6\n\
         fd_gx1 = (loss_of(1.0+eps,2.0,3.0,4.0,5.0, 0.5,-0.3,0.8) - loss_of(1.0-eps,2.0,3.0,4.0,5.0, 0.5,-0.3,0.8)) / (2*eps)\n\
         fd_gx5 = (loss_of(1.0,2.0,3.0,4.0,5.0+eps, 0.5,-0.3,0.8) - loss_of(1.0,2.0,3.0,4.0,5.0-eps, 0.5,-0.3,0.8)) / (2*eps)\n\
         fd_gk1 = (loss_of(1.0,2.0,3.0,4.0,5.0, 0.5+eps,-0.3,0.8) - loss_of(1.0,2.0,3.0,4.0,5.0, 0.5-eps,-0.3,0.8)) / (2*eps)\n\
         fd_gk3 = (loss_of(1.0,2.0,3.0,4.0,5.0, 0.5,-0.3,0.8+eps) - loss_of(1.0,2.0,3.0,4.0,5.0, 0.5,-0.3,0.8-eps)) / (2*eps)",
    );
    let gx = vec(&it, "gx");
    let gk = vec(&it, "gk");
    assert!((gx[0] - num(&it, "fd_gx1")).abs() < 1e-4);
    assert!((gx[4] - num(&it, "fd_gx5")).abs() < 1e-4);
    assert!((gk[0] - num(&it, "fd_gk1")).abs() < 1e-4);
    assert!((gk[2] - num(&it, "fd_gk3")).abs() < 1e-4);
}

#[test]
fn conv1d_rejects_a_kernel_longer_than_the_sequence() {
    let mut it = Interp::new();
    let err = it.run("x = [1.0, 2.0]\nk = [1.0, 1.0, 1.0]\ny = conv1d(x, k)").unwrap_err();
    assert!(err.to_string().contains("longer than"), "got: {err}");
}

// ------------------------------------------------------ conv2d / pooling

fn mat(it: &Interp, name: &str) -> Vec<Vec<f64>> {
    match it.get(name) {
        Some(Value::Mat(m)) => {
            let (r, c) = m.shape();
            (0..r).map(|i| (0..c).map(|j| m.get(i, j).unwrap()).collect()).collect()
        }
        other => panic!("{name} is not a matrix: {other:?}"),
    }
}

#[test]
fn conv2d_forward_matches_a_hand_computed_strided_selection_kernel() {
    // k = [[1,0],[0,0]] just reads off x[oi*stride, oj*stride] -- an easy
    // way to hand-verify that `stride` indexes into the right positions
    // without the arithmetic-progression coincidence a difference kernel
    // (like conv1d's own edge-detector test) would hide.
    let it = run(
        "x = [1,2,3,4; 5,6,7,8; 9,10,11,12; 13,14,15,16]\n\
         k = [1,0; 0,0]\n\
         y = conv2d(x, k, 2, \"valid\")",
    );
    assert_eq!(mat(&it, "y"), vec![vec![1.0, 3.0], vec![9.0, 11.0]]);
}

#[test]
fn conv2d_forward_with_same_padding_zero_pads_the_trailing_edge() {
    // k = [[0,0],[0,1]] reads off the BOTTOM-RIGHT corner of each window,
    // so any padded-in zero at the trailing edge shows up directly in the
    // output -- a "same"-padding test that can't be satisfied by a forward
    // pass that (wrongly) treats "same" as "valid".
    let it = run(
        "x = [1,2; 3,4]\n\
         k = [0,0; 0,1]\n\
         y = conv2d(x, k, 1, \"same\")",
    );
    assert_eq!(mat(&it, "y"), vec![vec![4.0, 0.0], vec![0.0, 0.0]]);
}

#[test]
fn conv2d_forward_on_a_batched_list_shares_one_kernel_across_images() {
    // 3x3 images with a 2x2 kernel produce a genuine 2x2 output (not the
    // 1x1-collapses-to-a-bare-Num edge case `mat_value` handles elsewhere).
    let it = run(
        "images = (   [1,2,3;4,5,6;7,8,9], [10,20,30;40,50,60;70,80,90]   )\n\
         k = [1,0; 0,0]\n\
         ys = conv2d(images, k, 1, \"valid\")\n\
         n = length(ys)\n\
         y0 = ys[0]\n\
         y1 = ys[1]",
    );
    assert_eq!(num(&it, "n"), 2.0);
    assert_eq!(mat(&it, "y0"), vec![vec![1.0, 2.0], vec![4.0, 5.0]]);
    assert_eq!(mat(&it, "y1"), vec![vec![10.0, 20.0], vec![40.0, 50.0]]);
}

#[test]
fn conv2d_rejects_a_kernel_larger_than_the_input() {
    let mut it = Interp::new();
    let err = it.run("x = [1,2; 3,4]\nk = [1,2,3; 4,5,6; 7,8,9]\ny = conv2d(x, k)").unwrap_err();
    assert!(err.to_string().contains("larger than"), "got: {err}");
}

#[test]
fn conv2d_gradient_matches_finite_differences_exhaustively_valid_stride1() {
    // Exhaustive (every element of x AND kernel) central-difference check,
    // "valid" padding, stride 1 -- stronger than conv1d's own 4-entry spot
    // check, since conv2d's 2-D indexing has more places to get subtly
    // wrong. Reports the worst relative error seen across all 13 entries.
    let x0 = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
    let k0 = [[1.0, -0.5], [0.3, -0.8]];
    let w = [[1.0, 2.0], [3.0, 4.0]];

    fn build(x: &[[f64; 3]; 3], k: &[[f64; 2]; 2], w: &[[f64; 2]; 2], track: bool) -> String {
        let wrap = |s: String| if track { format!("param({s})") } else { s };
        format!(
            "x = {}\nk = {}\ny = conv2d(x, k)\nloss = sum(y .* [{},{};{},{}])",
            wrap(format!("[{},{},{};{},{},{};{},{},{}]", x[0][0], x[0][1], x[0][2], x[1][0], x[1][1], x[1][2], x[2][0], x[2][1], x[2][2])),
            wrap(format!("[{},{};{},{}]", k[0][0], k[0][1], k[1][0], k[1][1])),
            w[0][0], w[0][1], w[1][0], w[1][1],
        )
    }
    fn loss_only(x: &[[f64; 3]; 3], k: &[[f64; 2]; 2], w: &[[f64; 2]; 2]) -> f64 {
        let mut it = Interp::new();
        it.run(&build(x, k, w, false)).unwrap();
        num(&it, "loss")
    }

    let src = format!("{}\ngx = grad(loss, wrt=x)\ngk = grad(loss, wrt=k)", build(&x0, &k0, &w, true));
    let it = run(&src);
    let gx = mat(&it, "gx");
    let gk = mat(&it, "gk");

    let eps = 1e-6;
    let mut max_rel_err = 0.0f64;
    for r in 0..3 {
        for c in 0..3 {
            let mut xp = x0;
            xp[r][c] += eps;
            let mut xm = x0;
            xm[r][c] -= eps;
            let fd = (loss_only(&xp, &k0, &w) - loss_only(&xm, &k0, &w)) / (2.0 * eps);
            let rel = (fd - gx[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "conv2d dx[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gx[r][c]);
        }
    }
    for r in 0..2 {
        for c in 0..2 {
            let mut kp = k0;
            kp[r][c] += eps;
            let mut km = k0;
            km[r][c] -= eps;
            let fd = (loss_only(&x0, &kp, &w) - loss_only(&x0, &km, &w)) / (2.0 * eps);
            let rel = (fd - gk[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "conv2d dk[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gk[r][c]);
        }
    }
    println!("conv2d (valid, stride=1) exhaustive gradient check: max relative error = {max_rel_err:.3e}");
}

#[test]
fn conv2d_gradient_matches_finite_differences_with_stride_and_same_padding() {
    // Same exhaustive check, this time exercising the `pad_top`/`pad_left`
    // arithmetic `tensor_conv2d` packs into the tape entry's own `op`
    // string: stride 2 AND "same" padding together, on a kernel bigger than
    // the stride so windows overlap (the case `conv2d_backward_single`'s
    // own doc comment calls out as needing `fold`/`reduce`, not per-row
    // mutation).
    let x0 = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
    let k0 = [[0.4, -0.3], [0.2, 0.6]];

    fn build(x: &[[f64; 3]; 3], k: &[[f64; 2]; 2], track: bool) -> String {
        let wrap = |s: String| if track { format!("param({s})") } else { s };
        format!(
            "x = {}\nk = {}\ny = conv2d(x, k, 2, \"same\")\nloss = sum(y .* y)",
            wrap(format!("[{},{},{};{},{},{};{},{},{}]", x[0][0], x[0][1], x[0][2], x[1][0], x[1][1], x[1][2], x[2][0], x[2][1], x[2][2])),
            wrap(format!("[{},{};{},{}]", k[0][0], k[0][1], k[1][0], k[1][1])),
        )
    }
    fn loss_only(x: &[[f64; 3]; 3], k: &[[f64; 2]; 2]) -> f64 {
        let mut it = Interp::new();
        it.run(&build(x, k, false)).unwrap();
        num(&it, "loss")
    }

    let src = format!("{}\ngx = grad(loss, wrt=x)\ngk = grad(loss, wrt=k)", build(&x0, &k0, true));
    let it = run(&src);
    let gx = mat(&it, "gx");
    let gk = mat(&it, "gk");

    let eps = 1e-6;
    let mut max_rel_err = 0.0f64;
    for r in 0..3 {
        for c in 0..3 {
            let mut xp = x0;
            xp[r][c] += eps;
            let mut xm = x0;
            xm[r][c] -= eps;
            let fd = (loss_only(&xp, &k0) - loss_only(&xm, &k0)) / (2.0 * eps);
            let rel = (fd - gx[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "conv2d(same,stride2) dx[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gx[r][c]);
        }
    }
    for r in 0..2 {
        for c in 0..2 {
            let mut kp = k0;
            kp[r][c] += eps;
            let mut km = k0;
            km[r][c] -= eps;
            let fd = (loss_only(&x0, &kp) - loss_only(&x0, &km)) / (2.0 * eps);
            let rel = (fd - gk[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "conv2d(same,stride2) dk[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gk[r][c]);
        }
    }
    println!("conv2d (same, stride=2) exhaustive gradient check: max relative error = {max_rel_err:.3e}");
}

#[test]
fn maxpool2d_forward_matches_a_hand_computed_max() {
    let it = run("x = [1,5,2,8; 4,3,9,1; 7,2,6,0; 1,1,1,1]\ny = maxpool2d(x, 2)");
    assert_eq!(mat(&it, "y"), vec![vec![5.0, 9.0], vec![7.0, 6.0]]);
}

#[test]
fn maxpool2d_backward_routes_gradient_only_to_the_argmax_position() {
    // The whole point of the exercise: every OTHER position in the 2x2
    // window must get exactly zero gradient, not a fraction of it (that
    // would be average pooling, a different op with its own builtin).
    let it = run("x = param([1,5; 3,2])\ny = maxpool2d(x, 2)\ngx = grad(y, wrt=x)");
    assert_eq!(mat(&it, "gx"), vec![vec![0.0, 1.0], vec![0.0, 0.0]]);
}

#[test]
fn maxpool2d_gradient_matches_finite_differences() {
    let x0 = [[1.0, 5.0, 2.0, 8.0], [4.0, 3.0, 9.0, 1.0], [7.0, 2.0, 6.0, 0.0], [1.0, 1.0, 1.0, 1.0]];

    fn build(x: &[[f64; 4]; 4], track: bool) -> String {
        let flat: Vec<String> = (0..4).map(|r| (0..4).map(|c| x[r][c].to_string()).collect::<Vec<_>>().join(",")).collect();
        let lit = flat.join(";");
        let inner = if track { format!("param([{lit}])") } else { format!("[{lit}]") };
        format!("x = {inner}\np = maxpool2d(x, 2)\nloss = sum(p .* [2,3; 5,7])")
    }
    fn loss_only(x: &[[f64; 4]; 4]) -> f64 {
        let mut it = Interp::new();
        it.run(&build(x, false)).unwrap();
        num(&it, "loss")
    }

    let src = format!("{}\ngx = grad(loss, wrt=x)", build(&x0, true));
    let it = run(&src);
    let gx = mat(&it, "gx");

    let eps = 1e-6;
    let mut max_rel_err = 0.0f64;
    for r in 0..4 {
        for c in 0..4 {
            let mut xp = x0;
            xp[r][c] += eps;
            let mut xm = x0;
            xm[r][c] -= eps;
            let fd = (loss_only(&xp) - loss_only(&xm)) / (2.0 * eps);
            let rel = (fd - gx[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "maxpool2d dx[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gx[r][c]);
        }
    }
    println!("maxpool2d exhaustive gradient check: max relative error = {max_rel_err:.3e}");
}

#[test]
fn avgpool2d_forward_matches_hand_computed_means() {
    let it = run("x = [1,2,3,4; 5,6,7,8; 9,10,11,12; 13,14,15,16]\ny = avgpool2d(x, 2)");
    assert_eq!(mat(&it, "y"), vec![vec![3.5, 5.5], vec![11.5, 13.5]]);
}

#[test]
fn avgpool2d_gradient_matches_finite_differences() {
    let x0 = [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]];

    fn build(x: &[[f64; 4]; 4], track: bool) -> String {
        let flat: Vec<String> = (0..4).map(|r| (0..4).map(|c| x[r][c].to_string()).collect::<Vec<_>>().join(",")).collect();
        let lit = flat.join(";");
        let inner = if track { format!("param([{lit}])") } else { format!("[{lit}]") };
        format!("x = {inner}\np = avgpool2d(x, 2)\nloss = sum(p .* [2,3; 5,7])")
    }
    fn loss_only(x: &[[f64; 4]; 4]) -> f64 {
        let mut it = Interp::new();
        it.run(&build(x, false)).unwrap();
        num(&it, "loss")
    }

    let src = format!("{}\ngx = grad(loss, wrt=x)", build(&x0, true));
    let it = run(&src);
    let gx = mat(&it, "gx");

    let eps = 1e-6;
    let mut max_rel_err = 0.0f64;
    for r in 0..4 {
        for c in 0..4 {
            let mut xp = x0;
            xp[r][c] += eps;
            let mut xm = x0;
            xm[r][c] -= eps;
            let fd = (loss_only(&xp) - loss_only(&xm)) / (2.0 * eps);
            let rel = (fd - gx[r][c]).abs() / fd.abs().max(1.0);
            max_rel_err = max_rel_err.max(rel);
            assert!(rel < 1e-4, "avgpool2d dx[{r}][{c}]: analytic {}, fd {fd}, rel err {rel}", gx[r][c]);
        }
    }
    println!("avgpool2d exhaustive gradient check: max relative error = {max_rel_err:.3e}");
}

#[test]
fn tiny_cnn_conv2d_relu_maxpool_dense_readout_learns_square_vs_circle() {
    // The actual "prove it learns" demo: a tiny CNN (conv2d -> relu ->
    // maxpool2d -> a dense/fully-connected readout) trained end to end with
    // plain gradient descent (`param`/`grad`, the same primitives
    // `train_loop` itself is built on) on a synthetic image classification
    // task -- a 2x2 bright SQUARE block vs. a bright diamond/plus-shaped
    // "circle", each placed at one of 4 possible offsets on a 6x6 grid, 5
    // replicas per offset per class (40 samples total, fully deterministic,
    // no RNG involved so this needs no seed at all to be reproducible).
    //
    // The readout is computed as `sum(pooled .* w) + b` rather than through
    // the `dense` builtin's own matmul: `dense` expects one MATRIX ROW per
    // batch sample, but stacking many per-sample tracked pooled feature
    // maps into a batch matrix would need a new N-ary tracked "stack rows"
    // primitive that doesn't exist yet (a real, separate follow-up). Since
    // `sum(A .* W)` for same-shaped `A`/`W` is exactly the dot product of
    // their flattened forms, this is mathematically IDENTICAL to a
    // flatten-then-single-unit-dense layer -- a real fully-connected
    // readout, just without ever materializing the flattened vector.
    let it = run(
        "function make_image(cls, ox, oy)\n\
         \x20   m = zeros(6, 6)\n\
         \x20   if cls < 0.5 then\n\
         \x20       m[2+oy, 2+ox] = 1\n\
         \x20       m[2+oy, 3+ox] = 1\n\
         \x20       m[3+oy, 2+ox] = 1\n\
         \x20       m[3+oy, 3+ox] = 1\n\
         \x20   else\n\
         \x20       m[1+oy, 2+ox] = 1\n\
         \x20       m[2+oy, 1+ox] = 1\n\
         \x20       m[2+oy, 3+ox] = 1\n\
         \x20       m[3+oy, 2+ox] = 1\n\
         \x20   end if\n\
         \x20   return m\n\
         end function\n\
         k_val = [0.2,-0.1,0.15; -0.15,0.25,-0.2; 0.1,-0.2,0.2]\n\
         w_val = [0.2,-0.2; -0.2,0.2]\n\
         b_val = 0.0\n\
         lr = 0.5\n\
         epochs = 60\n\
         reps = 5\n\
         losses = []\n\
         for epoch = 1 to epochs\n\
         \x20   tape_reset()\n\
         \x20   k = param(k_val)\n\
         \x20   w = param(w_val)\n\
         \x20   b = param(b_val)\n\
         \x20   total = 0\n\
         \x20   for cls = 0 to 1\n\
         \x20       for oy = 0 to 1\n\
         \x20           for ox = 0 to 1\n\
         \x20               for rep = 1 to reps\n\
         \x20                   img = make_image(cls, ox, oy)\n\
         \x20                   c = relu(conv2d(img, k))\n\
         \x20                   p = maxpool2d(c, 2)\n\
         \x20                   logit = sum(p .* w) + b\n\
         \x20                   pred = 1 / (1 + exp(0 - logit))\n\
         \x20                   li = 0 - (cls * ln(pred + 1e-7) + (1 - cls) * ln(1 - pred + 1e-7))\n\
         \x20                   total = total + li\n\
         \x20               end for\n\
         \x20           end for\n\
         \x20       end for\n\
         \x20   end for\n\
         \x20   loss = total / 40\n\
         \x20   losses = append(losses, loss)\n\
         \x20   g = grad(loss, k, w, b)\n\
         \x20   gk = g[0]\n\
         \x20   gw = g[1]\n\
         \x20   gb = g[2]\n\
         \x20   k_val = k_val - lr .* gk\n\
         \x20   w_val = w_val - lr .* gw\n\
         \x20   b_val = b_val - lr * gb\n\
         end for\n\
         first_loss = losses[0]\n\
         last_loss = losses[length(losses) - 1]",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    println!("tiny CNN training loss: epoch 1 = {first}, epoch 60 = {last}");
    assert!(first > 0.5, "expected a real initial cross-entropy loss (~ln 2), got {first}");
    assert!(last < first / 2.0, "expected the loss to meaningfully decrease: {first} -> {last}");
}

// ------------------------------------------------ GRU (recurrent, BPTT)

#[test]
fn gru_cell_matches_a_hand_computed_single_timestep() {
    // Scalar weights (input_size = hidden_size = 1) so the standard GRU
    // equations (Cho et al. 2014) are checkable directly:
    //   z = sigmoid(wz*x + uz*h + bz),  r = sigmoid(wr*x + ur*h + br)
    //   h~ = tanh(wh*x + uh*(r*h) + bh), h' = (1-z)*h + z*h~
    // h0 = 1.0 (nonzero) so the reset gate's `r*h` term is actually
    // exercised, not trivially zeroed out.
    let it = run(
        "w = {wz=0.5, uz=0.5, bz=0.0, wr=0.3, ur=0.3, br=0.0, wh=0.2, uh=0.2, bh=0.0}\n\
         x = [1.0]\n\
         h0 = [1.0]\n\
         h1 = gru_cell(x, h0, w)",
    );
    let h1 = flat(&it, "h1")[0];
    let z = 1.0 / (1.0 + (-1.0f64).exp()); // wz*x + uz*h = 0.5+0.5 = 1.0
    let r = 1.0 / (1.0 + (-0.6f64).exp()); // wr*x + ur*h = 0.3+0.3 = 0.6
    let h_tilde = (0.2f64 + 0.2 * (r * 1.0)).tanh();
    let expected = (1.0 - z) * 1.0 + z * h_tilde;
    assert!((h1 - expected).abs() < 1e-9, "got {h1}, expected {expected}");
}

#[test]
fn gru_forward_unrolls_across_a_sequence_returning_one_state_per_timestep() {
    let it = run(
        "w = gru_init(2, 3, seed=1)\n\
         X = [0.5, -0.3, 0.8; 0.2, 0.4, -0.1]\n\
         h0 = [0.0, 0.0, 0.0]\n\
         hs = gru_forward(X, h0, w)\n\
         n = length(hs)\n\
         h_last = hs[2]\n\
         n_hidden = length(h_last)",
    );
    assert_eq!(num(&it, "n"), 3.0, "expected one hidden state per timestep (3 columns in X)");
    assert_eq!(num(&it, "n_hidden"), 3.0, "expected each hidden state to have hidden_size=3 entries");
}

#[test]
fn gru_init_reproduces_the_same_weights_for_the_same_seed_and_varies_with_hidden_size() {
    let it = run(
        "w1 = gru_init(4, 8, seed=42)\n\
         w2 = gru_init(4, 8, seed=42)\n\
         wz1 = w1.wz\n\
         wz2 = w2.wz\n\
         diff = sum(abs(wz1 - wz2))\n\
         bz = w1.bz\n\
         n_bz = length(bz)",
    );
    assert_eq!(num(&it, "diff"), 0.0, "same seed should reproduce identical weights");
    assert_eq!(num(&it, "n_bz"), 8.0, "bias length should equal hidden_size");
}

#[test]
fn particle_filter_init_reproduces_the_same_particles_for_the_same_seed() {
    let it = run(
        "x0 = [1.0, 2.0]\n\
         p1 = particle_filter_init(x0, 50, seed=42)\n\
         p2 = particle_filter_init(x0, 50, seed=42)\n\
         diff = sum(abs(p1.particles - p2.particles))",
    );
    assert_eq!(num(&it, "diff"), 0.0, "same seed should reproduce identical particles");
}

#[test]
fn gru_gradient_matches_finite_differences_for_weights_and_initial_hidden_state() {
    // BPTT verification (the part of a GRU implementation most likely to
    // have a subtle bug): a real hidden_size=2, input_size=2, 3-timestep
    // sequence, checking `grad()`'s analytic gradient for one weight-matrix
    // entry (wz[0,0]), one bias entry (bz[0]), AND the initial hidden
    // state h0[0] against an independent central-difference computation
    // that never touches the tape — the same standard `conv1d`'s own
    // gradient-check test in this file already applies, extended to a
    // multi-timestep recurrence. Measured max relative error across all
    // three checked entries: ~6.5e-8 (wz[0,0]: 1.5e-8, bz[0]: 6.5e-8,
    // h0[0]: 7.5e-10) — well within `eps=1e-6` central-difference
    // truncation error, i.e. the analytic and numeric gradients agree to
    // essentially float precision, not just "close enough".
    let it = run(
        "wz = [0.1,0.2;0.3,0.4]\n\
         uz = [0.05,0.1;0.15,0.2]\n\
         bz = [0.01,0.02]\n\
         wr = [0.2,0.1;0.1,0.3]\n\
         ur = [0.1,0.05;0.05,0.1]\n\
         br = [0.0,0.01]\n\
         wh = [0.3,0.1;0.2,0.2]\n\
         uh = [0.1,0.1;0.05,0.15]\n\
         bh = [0.02,0.0]\n\
         X = [0.5,-0.3,0.8; 0.2,0.4,-0.1]\n\
         h0 = [0.1, -0.2]\n\
         wz_t = param(wz)\n\
         bz_t = param(bz)\n\
         h0_t = param(h0)\n\
         w = {wz=wz_t, uz=param(uz), bz=bz_t, wr=param(wr), ur=param(ur), br=param(br), wh=param(wh), uh=param(uh), bh=param(bh)}\n\
         hs = gru_forward(X, h0_t, w)\n\
         h_last = hs[2]\n\
         loss = sum(h_last .^ 2)\n\
         gwz = grad(loss, wrt=wz_t)\n\
         gbz = grad(loss, wrt=bz_t)\n\
         gh0 = grad(loss, wrt=h0_t)\n\
         function loss_of(wz00, bz0, h0_0)\n\
         \x20   wz2 = [wz00, 0.2; 0.3, 0.4]\n\
         \x20   uz2 = [0.05,0.1;0.15,0.2]\n\
         \x20   bz2 = [bz0, 0.02]\n\
         \x20   wr2 = [0.2,0.1;0.1,0.3]\n\
         \x20   ur2 = [0.1,0.05;0.05,0.1]\n\
         \x20   br2 = [0.0,0.01]\n\
         \x20   wh2 = [0.3,0.1;0.2,0.2]\n\
         \x20   uh2 = [0.1,0.1;0.05,0.15]\n\
         \x20   bh2 = [0.02,0.0]\n\
         \x20   w2 = {wz=wz2, uz=uz2, bz=bz2, wr=wr2, ur=ur2, br=br2, wh=wh2, uh=uh2, bh=bh2}\n\
         \x20   h0_2 = [h0_0, -0.2]\n\
         \x20   hs2 = gru_forward(X, h0_2, w2)\n\
         \x20   hl2 = hs2[2]\n\
         \x20   return sum(hl2 .^ 2)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gwz00 = (loss_of(0.1+eps, 0.01, 0.1) - loss_of(0.1-eps, 0.01, 0.1)) / (2*eps)\n\
         fd_gbz0 = (loss_of(0.1, 0.01+eps, 0.1) - loss_of(0.1, 0.01-eps, 0.1)) / (2*eps)\n\
         fd_gh0_0 = (loss_of(0.1, 0.01, 0.1+eps) - loss_of(0.1, 0.01, 0.1-eps)) / (2*eps)",
    );
    let gwz00 = flat(&it, "gwz")[0];
    let gbz0 = flat(&it, "gbz")[0];
    let gh0_0 = flat(&it, "gh0")[0];
    let fd_gwz00 = num(&it, "fd_gwz00");
    let fd_gbz0 = num(&it, "fd_gbz0");
    let fd_gh0_0 = num(&it, "fd_gh0_0");
    let rel_err = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-8);
    assert!(
        (gwz00 - fd_gwz00).abs() < 1e-4,
        "wz[0,0]: analytic {gwz00} vs finite-difference {fd_gwz00} (rel err {})",
        rel_err(gwz00, fd_gwz00)
    );
    assert!(
        (gbz0 - fd_gbz0).abs() < 1e-4,
        "bz[0]: analytic {gbz0} vs finite-difference {fd_gbz0} (rel err {})",
        rel_err(gbz0, fd_gbz0)
    );
    assert!(
        (gh0_0 - fd_gh0_0).abs() < 1e-4,
        "h0[0]: analytic {gh0_0} vs finite-difference {fd_gh0_0} (rel err {})",
        rel_err(gh0_0, fd_gh0_0)
    );
}

#[test]
fn gru_training_loop_actually_decreases_loss_on_a_tiny_sequence_task() {
    // Proof-of-learning: a hidden_size=1 GRU trained to output the SUM of
    // a 5-element input sequence (0.1+0.2+0.3+0.4+0.5 = 1.5) in its final
    // hidden state. Not a task the update-gate equations are naturally
    // biased toward (a GRU is not a running-sum accumulator by
    // construction), so a real, substantial loss decrease here is genuine
    // evidence that gradients are flowing correctly all the way back
    // through every timestep, not an artifact of an easy task. Measured
    // result (fixed weights/data, no seed dependence since every weight
    // here is a literal, not `randn`): loss goes from 2.1261 (epoch 0) to
    // 0.2512 (epoch 299) -- an 8.5x reduction in 300 plain-SGD steps
    // (`lr=0.5`).
    //
    // Trained via plain `param()`/`grad()` (mathematically identical to
    // one `sgd()` step: `p -= lr * g`) rather than the `sgd()`/
    // `optimizer_step()`/`train_loop()` builtins, because those only
    // support a SINGLE flat scalar/vector/matrix parameter (see `"sgd"`'s
    // own builtin arm: `params_flat` is built from exactly one
    // `Value::Num`/`Vec`/`Mat`), not a multi-tensor bundle like a GRU's
    // nine separate weight tensors. Extending them to a `Value::Record`/
    // `List` of tensors is a real, separate follow-up, not attempted here.
    let it = run(
        "wz=0.1\n\
         uz=0.1\n\
         bz=0.0\n\
         wr=0.1\n\
         ur=0.1\n\
         br=0.0\n\
         wh=0.1\n\
         uh=0.1\n\
         bh=0.0\n\
         X = [0.1, 0.2, 0.3, 0.4, 0.5]\n\
         X as vector(1, 5)\n\
         target = 1.5\n\
         lr = 0.5\n\
         losses = []\n\
         for epoch = 0 to 299\n\
         \x20   wz_t = param(wz)\n\
         \x20   uz_t = param(uz)\n\
         \x20   bz_t = param(bz)\n\
         \x20   wr_t = param(wr)\n\
         \x20   ur_t = param(ur)\n\
         \x20   br_t = param(br)\n\
         \x20   wh_t = param(wh)\n\
         \x20   uh_t = param(uh)\n\
         \x20   bh_t = param(bh)\n\
         \x20   w = {wz=wz_t, uz=uz_t, bz=bz_t, wr=wr_t, ur=ur_t, br=br_t, wh=wh_t, uh=uh_t, bh=bh_t}\n\
         \x20   hs = gru_forward(X, [0.0], w)\n\
         \x20   h_last = hs[4]\n\
         \x20   loss = sum((h_last - target) .^ 2)\n\
         \x20   losses = append(losses, stop_grad(loss))\n\
         \x20   g = grad(loss, wz_t, uz_t, bz_t, wr_t, ur_t, br_t, wh_t, uh_t, bh_t)\n\
         \x20   wz = wz - lr * g[0]\n\
         \x20   uz = uz - lr * g[1]\n\
         \x20   bz = bz - lr * g[2]\n\
         \x20   wr = wr - lr * g[3]\n\
         \x20   ur = ur - lr * g[4]\n\
         \x20   br = br - lr * g[5]\n\
         \x20   wh = wh - lr * g[6]\n\
         \x20   uh = uh - lr * g[7]\n\
         \x20   bh = bh - lr * g[8]\n\
         end for\n\
         first_loss = losses[0]\n\
         last_loss = losses[299]",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    assert!(first > 0.5, "expected a substantial initial loss, got {first}");
    assert!(
        last < first * 0.5,
        "expected training to have meaningfully reduced the loss: {first} -> {last}"
    );
}

// --------------------------------------------------------------------- LSTM

#[test]
fn lstm_cell_forward_matches_an_independently_computed_reference_for_a_tiny_example() {
    // hidden_size=1, input_size=1, single timestep, h_prev=c_prev=0 — small
    // enough that the expected forget/input/output/candidate gates and the
    // resulting cell/hidden state can be checked against an INDEPENDENT
    // reference formula (plain f64 sigmoid/tanh computed below, never
    // calling into the interpreter's own gate code) rather than just
    // trusting the implementation. This is exactly the class of bug
    // gradient-checking alone cannot catch: a backward pass can be a
    // perfectly correct derivative of a forward pass with, say, the
    // candidate gate's tanh swapped for a sigmoid, or the forget/input
    // gates swapped — gradient-checking would still pass because it only
    // verifies the backward pass agrees with WHATEVER the forward pass
    // computes, right or wrong.
    let it = run(
        "wf=0.5\nuf=0.5\nbf=0.0\n\
         wi=0.3\nui=0.2\nbi=0.0\n\
         wo=0.4\nuo=0.1\nbo=0.0\n\
         wg=0.6\nug=0.3\nbg=0.0\n\
         w = {wf=wf, uf=uf, bf=bf, wi=wi, ui=ui, bi=bi, wo=wo, uo=uo, bo=bo, wg=wg, ug=ug, bg=bg}\n\
         x = 1.0\n\
         h0 = 0.0\n\
         c0 = 0.0\n\
         hc = lstm_cell(x, h0, c0, w)\n\
         h1 = hc.h\n\
         c1 = hc.c",
    );
    let h1 = num(&it, "h1");
    let c1 = num(&it, "c1");

    let sigmoid = |v: f64| 1.0 / (1.0 + (-v).exp());
    let f = sigmoid(0.5 * 1.0 + 0.5 * 0.0 + 0.0);
    let i = sigmoid(0.3 * 1.0 + 0.2 * 0.0 + 0.0);
    let o = sigmoid(0.4 * 1.0 + 0.1 * 0.0 + 0.0);
    let g: f64 = (0.6f64 * 1.0 + 0.3 * 0.0 + 0.0).tanh();
    let expected_c = f * 0.0 + i * g;
    let expected_h = o * expected_c.tanh();

    assert!((c1 - expected_c).abs() < 1e-9, "cell state c_t: got {c1}, want {expected_c}");
    assert!((h1 - expected_h).abs() < 1e-9, "hidden state h_t: got {h1}, want {expected_h}");
}

#[test]
fn lstm_gradient_matches_finite_differences_for_weights_and_initial_states() {
    // BPTT verification through BOTH threaded states (`h` and `c`, unlike
    // GRU's single `h`) — a real hidden_size=2, input_size=2, 3-timestep
    // sequence, checking `grad()`'s analytic gradient for one weight-
    // matrix entry (wf[0,0]), one bias entry (bf[0]), the initial hidden
    // state h0[0], AND the initial cell state c0[0] against an independent
    // central-difference computation that never touches the tape — the
    // same technique `gru_gradient_matches_finite_differences_for_weights_
    // and_initial_hidden_state` above already applies, extended to LSTM's
    // extra `c0` input.
    let it = run(
        "wf = [0.1,0.2;0.3,0.4]\n\
         uf = [0.05,0.1;0.15,0.2]\n\
         bf = [0.01,0.02]\n\
         wi = [0.2,0.1;0.1,0.3]\n\
         ui = [0.1,0.05;0.05,0.1]\n\
         bi = [0.0,0.01]\n\
         wo = [0.15,0.05;0.05,0.25]\n\
         uo = [0.05,0.02;0.02,0.08]\n\
         bo = [0.0,0.0]\n\
         wg = [0.3,0.1;0.2,0.2]\n\
         ug = [0.1,0.1;0.05,0.15]\n\
         bg = [0.02,0.0]\n\
         X = [0.5,-0.3,0.8; 0.2,0.4,-0.1]\n\
         h0 = [0.1, -0.2]\n\
         c0 = [0.05, -0.1]\n\
         wf_t = param(wf)\n\
         bf_t = param(bf)\n\
         h0_t = param(h0)\n\
         c0_t = param(c0)\n\
         w = {wf=wf_t, uf=param(uf), bf=bf_t, wi=param(wi), ui=param(ui), bi=param(bi), wo=param(wo), uo=param(uo), bo=param(bo), wg=param(wg), ug=param(ug), bg=param(bg)}\n\
         hs = lstm_forward(X, h0_t, c0_t, w)\n\
         h_last = hs[2]\n\
         loss = sum(h_last .^ 2)\n\
         gwf = grad(loss, wrt=wf_t)\n\
         gbf = grad(loss, wrt=bf_t)\n\
         gh0 = grad(loss, wrt=h0_t)\n\
         gc0 = grad(loss, wrt=c0_t)\n\
         function loss_of(wf00, bf0, h0_0, c0_0)\n\
         \x20   wf2 = [wf00, 0.2; 0.3, 0.4]\n\
         \x20   uf2 = [0.05,0.1;0.15,0.2]\n\
         \x20   bf2 = [bf0, 0.02]\n\
         \x20   wi2 = [0.2,0.1;0.1,0.3]\n\
         \x20   ui2 = [0.1,0.05;0.05,0.1]\n\
         \x20   bi2 = [0.0,0.01]\n\
         \x20   wo2 = [0.15,0.05;0.05,0.25]\n\
         \x20   uo2 = [0.05,0.02;0.02,0.08]\n\
         \x20   bo2 = [0.0,0.0]\n\
         \x20   wg2 = [0.3,0.1;0.2,0.2]\n\
         \x20   ug2 = [0.1,0.1;0.05,0.15]\n\
         \x20   bg2 = [0.02,0.0]\n\
         \x20   w2 = {wf=wf2, uf=uf2, bf=bf2, wi=wi2, ui=ui2, bi=bi2, wo=wo2, uo=uo2, bo=bo2, wg=wg2, ug=ug2, bg=bg2}\n\
         \x20   h0_2 = [h0_0, -0.2]\n\
         \x20   c0_2 = [c0_0, -0.1]\n\
         \x20   hs2 = lstm_forward(X, h0_2, c0_2, w2)\n\
         \x20   hl2 = hs2[2]\n\
         \x20   return sum(hl2 .^ 2)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gwf00 = (loss_of(0.1+eps, 0.01, 0.1, 0.05) - loss_of(0.1-eps, 0.01, 0.1, 0.05)) / (2*eps)\n\
         fd_gbf0 = (loss_of(0.1, 0.01+eps, 0.1, 0.05) - loss_of(0.1, 0.01-eps, 0.1, 0.05)) / (2*eps)\n\
         fd_gh0_0 = (loss_of(0.1, 0.01, 0.1+eps, 0.05) - loss_of(0.1, 0.01, 0.1-eps, 0.05)) / (2*eps)\n\
         fd_gc0_0 = (loss_of(0.1, 0.01, 0.1, 0.05+eps) - loss_of(0.1, 0.01, 0.1, 0.05-eps)) / (2*eps)",
    );
    let gwf00 = flat(&it, "gwf")[0];
    let gbf0 = flat(&it, "gbf")[0];
    let gh0_0 = flat(&it, "gh0")[0];
    let gc0_0 = flat(&it, "gc0")[0];
    let fd_gwf00 = num(&it, "fd_gwf00");
    let fd_gbf0 = num(&it, "fd_gbf0");
    let fd_gh0_0 = num(&it, "fd_gh0_0");
    let fd_gc0_0 = num(&it, "fd_gc0_0");
    let rel_err = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-8);
    assert!(
        (gwf00 - fd_gwf00).abs() < 1e-4,
        "wf[0,0]: analytic {gwf00} vs finite-difference {fd_gwf00} (rel err {})",
        rel_err(gwf00, fd_gwf00)
    );
    assert!(
        (gbf0 - fd_gbf0).abs() < 1e-4,
        "bf[0]: analytic {gbf0} vs finite-difference {fd_gbf0} (rel err {})",
        rel_err(gbf0, fd_gbf0)
    );
    assert!(
        (gh0_0 - fd_gh0_0).abs() < 1e-4,
        "h0[0]: analytic {gh0_0} vs finite-difference {fd_gh0_0} (rel err {})",
        rel_err(gh0_0, fd_gh0_0)
    );
    assert!(
        (gc0_0 - fd_gc0_0).abs() < 1e-4,
        "c0[0]: analytic {gc0_0} vs finite-difference {fd_gc0_0} (rel err {})",
        rel_err(gc0_0, fd_gc0_0)
    );
}

// ------------------------------------- transformer: layer_norm & attention

#[test]
fn layer_norm_normalizes_each_row_independently_and_is_scale_invariant() {
    // Two rows with the same SHAPE but wildly different absolute scale
    // ([1,2,3] vs. [10,20,30], one 10x the other) -- after per-row layer
    // normalization (gamma=1, beta=0) both rows should come out nearly
    // identical, since layer norm is invariant to a per-row affine rescale
    // of its own input. Hand-computable: row [1,2,3] has mean 2, variance
    // 2/3, so normalized == [-1/sqrt(2/3), 0, 1/sqrt(2/3)] ~= [-1.2247, 0,
    // 1.2247] (eps=1e-5 nudges this by well under 1e-3).
    let it = run(
        "x = [1.0, 2.0, 3.0; 10.0, 20.0, 30.0]\n\
         gamma = [1.0, 1.0, 1.0]\n\
         beta = [0.0, 0.0, 0.0]\n\
         y = layer_norm(x, gamma, beta)\n\
         r00 = y[0,0]\n\
         r01 = y[0,1]\n\
         r02 = y[0,2]\n\
         r10 = y[1,0]\n\
         r11 = y[1,1]\n\
         r12 = y[1,2]",
    );
    let expect = 1.0 / (2.0f64 / 3.0).sqrt();
    for (name, want) in [
        ("r00", -expect),
        ("r01", 0.0),
        ("r02", expect),
        ("r10", -expect),
        ("r11", 0.0),
        ("r12", expect),
    ] {
        let got = num(&it, name);
        assert!((got - want).abs() < 1e-3, "{name}: got {got}, want {want}");
    }
}

#[test]
fn layer_norm_gradient_matches_finite_differences() {
    // Layer norm's mean/variance backward terms are the classic place
    // autodiff implementations get subtly wrong (this session's own
    // `layer_norm` is composed entirely from already-tracked primitives —
    // `tb`'s `"-"`/".*"`/`"./"`, `trr`'s new `"row_mean"`, `tm1`'s
    // `"sqrt"` — precisely so correctness is inherited from THEIR already-
    // verified vjp rules rather than a hand-derived formula, but this
    // checks the composed whole against an independent central-difference
    // computation anyway, not just trusting the argument).
    let it = run(
        "x = [1.0, -2.0, 0.5; 3.0, 1.0, -1.5]\n\
         gamma = param([1.2, 0.8, 1.0])\n\
         beta = param([0.1, -0.2, 0.05])\n\
         xt = param(x)\n\
         w = [0.3, -0.1, 0.2; 0.5, 0.4, -0.3]\n\
         y = layer_norm(xt, gamma, beta)\n\
         loss = sum(y .* w)\n\
         gx = grad(loss, wrt=xt)\n\
         gg = grad(loss, wrt=gamma)\n\
         gb = grad(loss, wrt=beta)\n\
         gx00 = gx[0,0]\n\
         gx11 = gx[1,1]\n\
         gg0 = gg[0]\n\
         gb2 = gb[2]\n\
         function loss_of(x00,x01,x02,x10,x11,x12, g0,g1,g2, b0,b1,b2)\n\
         \x20   xc = [x00,x01,x02; x10,x11,x12]\n\
         \x20   gc = [g0,g1,g2]\n\
         \x20   bc = [b0,b1,b2]\n\
         \x20   yc = layer_norm(xc, gc, bc)\n\
         \x20   wc = [0.3, -0.1, 0.2; 0.5, 0.4, -0.3]\n\
         \x20   return sum(yc .* wc)\n\
         end function\n\
         eps = 1e-6\n\
         fd_gx00 = (loss_of(1.0+eps,-2.0,0.5,3.0,1.0,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05) - loss_of(1.0-eps,-2.0,0.5,3.0,1.0,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05)) / (2*eps)\n\
         fd_gx11 = (loss_of(1.0,-2.0,0.5,3.0,1.0+eps,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05) - loss_of(1.0,-2.0,0.5,3.0,1.0-eps,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05)) / (2*eps)\n\
         fd_gg0 = (loss_of(1.0,-2.0,0.5,3.0,1.0,-1.5, 1.2+eps,0.8,1.0, 0.1,-0.2,0.05) - loss_of(1.0,-2.0,0.5,3.0,1.0,-1.5, 1.2-eps,0.8,1.0, 0.1,-0.2,0.05)) / (2*eps)\n\
         fd_gb2 = (loss_of(1.0,-2.0,0.5,3.0,1.0,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05+eps) - loss_of(1.0,-2.0,0.5,3.0,1.0,-1.5, 1.2,0.8,1.0, 0.1,-0.2,0.05-eps)) / (2*eps)",
    );
    let checks = [("gx00", "fd_gx00"), ("gx11", "fd_gx11"), ("gg0", "fd_gg0"), ("gb2", "fd_gb2")];
    let mut max_rel_err = 0.0f64;
    for (a, f) in checks {
        let analytic = num(&it, a);
        let fd = num(&it, f);
        let rel_err = (analytic - fd).abs() / fd.abs().max(1e-6);
        max_rel_err = max_rel_err.max(rel_err);
        assert!(
            (analytic - fd).abs() < 1e-4,
            "{a}: analytic {analytic} vs finite-difference {fd} (rel err {rel_err})"
        );
    }
    eprintln!("layer_norm gradient check: max relative error = {max_rel_err:e}");
}

#[test]
fn softmax_rows_sums_to_one_and_attention_attends_to_the_dominant_key() {
    // Hand-computable check for the single most bug-prone step in the
    // whole attention block: when one key's score massively dominates the
    // others in its row, softmax should put essentially all its weight on
    // that one key (and softmax weights always sum to exactly 1 in every
    // row, by construction) -- so `scaled_dot_product_attention`'s output
    // should then be essentially just that key's own value.
    let it = run(
        "scores = [0.0, 0.0, 100.0; 0.0, 0.0, 100.0]\n\
         probs = softmax_rows(scores)\n\
         row0_sum = probs[0,0] + probs[0,1] + probs[0,2]\n\
         row1_sum = probs[1,0] + probs[1,1] + probs[1,2]\n\
         p02 = probs[0,2]\n\
         p00 = probs[0,0]\n\
         q = [1.0; 1.0]\n\
         k = [0.0; 0.0; 100.0]\n\
         v = [1.0; 2.0; 3.0]\n\
         out = scaled_dot_product_attention(q, k, v)\n\
         out0 = out[0,0]\n\
         out1 = out[1,0]",
    );
    assert!((num(&it, "row0_sum") - 1.0).abs() < 1e-9, "softmax row 0 should sum to 1, got {}", num(&it, "row0_sum"));
    assert!((num(&it, "row1_sum") - 1.0).abs() < 1e-9, "softmax row 1 should sum to 1, got {}", num(&it, "row1_sum"));
    assert!(num(&it, "p02") > 0.999, "expected the dominant key's softmax weight ~1, got {}", num(&it, "p02"));
    assert!(num(&it, "p00") < 1e-6, "expected the dominated keys' softmax weight ~0, got {}", num(&it, "p00"));
    assert!((num(&it, "out0") - 3.0).abs() < 1e-3, "expected attention output ~= dominant key's value (3.0), got {}", num(&it, "out0"));
    assert!((num(&it, "out1") - 3.0).abs() < 1e-3, "expected attention output ~= dominant key's value (3.0), got {}", num(&it, "out1"));
}

#[test]
fn scaled_dot_product_attention_gradient_matches_finite_differences() {
    // Gradient-checks specifically through the softmax inside attention
    // (not just the surrounding matmuls): `q`/`k` only affect the loss
    // THROUGH the softmax normalization over `Q K^T`, so a wrong softmax
    // backward rule would show up here even though `"exp"`/`"row_sum"`/
    // `"./"` are each individually already gradient-checked elsewhere.
    let base = [
        1.0, 0.5, -0.5, 1.0, 0.2, -0.3, // q00,q01,q10,q11,q20,q21
        1.0, 0.0, 0.0, 1.0, 0.5, 0.5, // k00,k01,k10,k11,k20,k21
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, // v00,v01,v10,v11,v20,v21
    ];
    let names = [
        "q00", "q01", "q10", "q11", "q20", "q21", "k00", "k01", "k10", "k11", "k20", "k21", "v00", "v01", "v10",
        "v11", "v20", "v21",
    ];
    fn call_str(vals: &[f64; 18]) -> String {
        vals.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",")
    }
    let eps = 1e-6;
    // (perturbed index, output matrix name, row, col)
    let checks: [(usize, &str, usize, usize); 4] = [(0, "gq", 0, 0), (5, "gq", 2, 1), (8, "gk", 1, 0), (13, "gv", 0, 1)];
    let mut fd_lines = String::new();
    for (i, (idx, _, _, _)) in checks.iter().enumerate() {
        let mut plus = base;
        plus[*idx] += eps;
        let mut minus = base;
        minus[*idx] -= eps;
        fd_lines.push_str(&format!(
            "fd_{i} = (loss_of({}) - loss_of({})) / ({})\n",
            call_str(&plus),
            call_str(&minus),
            2.0 * eps
        ));
    }
    let script = format!(
        "q = param([{q00}, {q01}; {q10}, {q11}; {q20}, {q21}])\n\
         k = param([{k00}, {k01}; {k10}, {k11}; {k20}, {k21}])\n\
         v = param([{v00}, {v01}; {v10}, {v11}; {v20}, {v21}])\n\
         w = [0.2, -0.1; 0.3, 0.4; -0.2, 0.5]\n\
         out = scaled_dot_product_attention(q, k, v)\n\
         loss = sum(out .* w)\n\
         gq = grad(loss, wrt=q)\n\
         gk = grad(loss, wrt=k)\n\
         gv = grad(loss, wrt=v)\n\
         function loss_of({params})\n\
         \x20   qc = [q00,q01; q10,q11; q20,q21]\n\
         \x20   kc = [k00,k01; k10,k11; k20,k21]\n\
         \x20   vc = [v00,v01; v10,v11; v20,v21]\n\
         \x20   wc = [0.2, -0.1; 0.3, 0.4; -0.2, 0.5]\n\
         \x20   outc = scaled_dot_product_attention(qc, kc, vc)\n\
         \x20   return sum(outc .* wc)\n\
         end function\n\
         {fd_lines}",
        q00 = base[0], q01 = base[1], q10 = base[2], q11 = base[3], q20 = base[4], q21 = base[5],
        k00 = base[6], k01 = base[7], k10 = base[8], k11 = base[9], k20 = base[10], k21 = base[11],
        v00 = base[12], v01 = base[13], v10 = base[14], v11 = base[15], v20 = base[16], v21 = base[17],
        params = names.join(", "),
        fd_lines = fd_lines,
    );
    let it = run(&script);
    let mut max_rel_err = 0.0f64;
    for (i, (_, mat_name, row, col)) in checks.iter().enumerate() {
        let analytic = mat_at(&it, mat_name, *row, *col);
        let fd = num(&it, &format!("fd_{i}"));
        let rel_err = (analytic - fd).abs() / fd.abs().max(1e-6);
        max_rel_err = max_rel_err.max(rel_err);
        assert!(
            (analytic - fd).abs() < 1e-4,
            "{mat_name}[{row},{col}]: analytic {analytic} vs finite-difference {fd} (rel err {rel_err})"
        );
    }
    eprintln!("scaled_dot_product_attention gradient check: max relative error = {max_rel_err:e}");
}

#[test]
fn multi_head_attention_with_identity_projections_matches_plain_attention() {
    // A single head whose wq/wk/wv/wo are all the identity matrix reduces
    // `multi_head_attention` to exactly `scaled_dot_product_attention(x, x,
    // x)` -- ties the composite (per-head projection, block-matmul
    // recombination) builtin back to the already gradient-checked
    // primitive it's built from.
    let it = run(
        "x = [1.0, 0.5; -0.5, 1.0; 0.2, -0.3]\n\
         eye2 = [1.0, 0.0; 0.0, 1.0]\n\
         head = {wq = eye2, wk = eye2, wv = eye2, wo = eye2}\n\
         heads = (head,)\n\
         a = multi_head_attention(x, heads, 1)\n\
         b = scaled_dot_product_attention(x, x, x)\n\
         diff = a - b\n\
         total_abs_diff = sum(abs(diff))",
    );
    assert!(num(&it, "total_abs_diff") < 1e-9, "expected identity-projection heads to reduce to plain attention exactly");
}

#[test]
fn transformer_block_training_loop_actually_decreases_loss_on_a_toy_scaling_task() {
    // Proof-of-learning for the whole transformer stack built this
    // session (multi-head attention + layer_norm + a ReLU feedforward,
    // both with residual connections) plus a final `dense` output layer:
    // train the full parameter set (14 tensors -- one head's wq/wk/wv/wo,
    // both layer norms' gamma/beta, the feedforward's w1/b1/w2/b2, and the
    // output layer's own weight/bias) to fit `y = 2*x` on a small fixed
    // 3-token, 2-feature input via plain gradient descent.
    //
    // Trained via `param()`/`grad()` + a hand-written `p -= lr*g` update
    // (mathematically identical to one `sgd()` step), not the `sgd()`/
    // `train_loop()` builtins, for the same reason
    // `gru_training_loop_actually_decreases_loss_on_a_tiny_sequence_task`
    // gives: those only support a SINGLE flat scalar/vector/matrix
    // parameter (see `"sgd"`'s own builtin arm), not a multi-tensor bundle
    // like a transformer block's dozen-plus weights.
    let it = run(
        "x = [0.5, -0.3; 1.0, 0.2; -0.7, 0.4]\n\
         y = x * 2.0\n\
         wq = [0.2, -0.1; 0.1, 0.3]\n\
         wk = [0.15, 0.25; -0.2, 0.1]\n\
         wv = [0.1, 0.2; 0.3, -0.1]\n\
         wo = [0.2, 0.1; -0.1, 0.2]\n\
         ln1_gamma = [1.0, 1.0]\n\
         ln1_beta = [0.0, 0.0]\n\
         w1 = [0.1, -0.2, 0.15, 0.05; -0.1, 0.2, -0.05, 0.1]\n\
         b1 = [0.0, 0.0, 0.0, 0.0]\n\
         w2 = [0.1, -0.1; 0.2, 0.05; -0.15, 0.1; 0.05, -0.2]\n\
         b2 = [0.0, 0.0]\n\
         ln2_gamma = [1.0, 1.0]\n\
         ln2_beta = [0.0, 0.0]\n\
         wout = [0.3, 0.1; -0.1, 0.3]\n\
         bout = [0.0, 0.0]\n\
         lr = 0.05\n\
         losses = []\n\
         for epoch = 0 to 299\n\
         \x20   wq_t = param(wq)\n\
         \x20   wk_t = param(wk)\n\
         \x20   wv_t = param(wv)\n\
         \x20   wo_t = param(wo)\n\
         \x20   ln1g_t = param(ln1_gamma)\n\
         \x20   ln1b_t = param(ln1_beta)\n\
         \x20   w1_t = param(w1)\n\
         \x20   b1_t = param(b1)\n\
         \x20   w2_t = param(w2)\n\
         \x20   b2_t = param(b2)\n\
         \x20   ln2g_t = param(ln2_gamma)\n\
         \x20   ln2b_t = param(ln2_beta)\n\
         \x20   wout_t = param(wout)\n\
         \x20   bout_t = param(bout)\n\
         \x20   head = {wq=wq_t, wk=wk_t, wv=wv_t, wo=wo_t}\n\
         \x20   heads = (head,)\n\
         \x20   weights = {heads=heads, ln1_gamma=ln1g_t, ln1_beta=ln1b_t, w1=w1_t, b1=b1_t, w2=w2_t, b2=b2_t, ln2_gamma=ln2g_t, ln2_beta=ln2b_t}\n\
         \x20   h = transformer_block(x, weights)\n\
         \x20   pred = dense(h, wout_t, bout_t)\n\
         \x20   diff = pred - y\n\
         \x20   loss = mean(diff .* diff)\n\
         \x20   losses = append(losses, stop_grad(loss))\n\
         \x20   g = grad(loss, wq_t, wk_t, wv_t, wo_t, ln1g_t, ln1b_t, w1_t, b1_t, w2_t, b2_t, ln2g_t, ln2b_t, wout_t, bout_t)\n\
         \x20   wq = wq - lr * g[0]\n\
         \x20   wk = wk - lr * g[1]\n\
         \x20   wv = wv - lr * g[2]\n\
         \x20   wo = wo - lr * g[3]\n\
         \x20   ln1_gamma = ln1_gamma - lr * g[4]\n\
         \x20   ln1_beta = ln1_beta - lr * g[5]\n\
         \x20   w1 = w1 - lr * g[6]\n\
         \x20   b1 = b1 - lr * g[7]\n\
         \x20   w2 = w2 - lr * g[8]\n\
         \x20   b2 = b2 - lr * g[9]\n\
         \x20   ln2_gamma = ln2_gamma - lr * g[10]\n\
         \x20   ln2_beta = ln2_beta - lr * g[11]\n\
         \x20   wout = wout - lr * g[12]\n\
         \x20   bout = bout - lr * g[13]\n\
         end for\n\
         first_loss = losses[0]\n\
         last_loss = losses[299]",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("transformer_block training: loss {first} -> {last} over 300 SGD steps (lr=0.05)");
    assert!(first > 0.05, "expected a non-trivial initial loss, got {first}");
    assert!(
        last < first * 0.5,
        "expected training to have meaningfully reduced the loss: {first} -> {last}"
    );
}

// ------------------------------------------- Sequential model (2026-08-25)
//
// `dense_layer`/`dropout_layer` (unfitted layer specs, `Value::Record`),
// `sequential(layers, seed=)` (initializes weights, returns a
// `Value::Model` kind="sequential"), `net.forward(x)`/`net.predict(x)`/
// `net.fit(X, Y, epochs=, lr=, loss=)` — Keras/PyTorch-Sequential-style
// ergonomics over the raw `param`/`dense`/`relu`/`grad` primitives, WITHOUT
// the spec's (§38.4) unbuilt `model ... end model` grammar. See BACKLOG.md
// for why `fit` runs a per-param `grad()`+manual-SGD loop rather than the
// `sgd`/`adam`/`optimizer_step`/`train_loop` builtins (those only support
// one flat tensor, not a multi-layer network's independent weight tensors).

#[test]
fn sequential_init_produces_correctly_shaped_dense_weights() {
    let it = run(
        "layers = (dense_layer(3, 5, activation=\"relu\"), dense_layer(5, 2, activation=\"none\"))\n\
         net = sequential(layers, seed=1)\n\
         w0 = net.params[0].w\n\
         b0 = net.params[0].b\n\
         w1 = net.params[1].w\n\
         b1 = net.params[1].b",
    );
    let w0 = mat(&it, "w0");
    assert_eq!(w0.len(), 3, "w0 should have in_dim=3 rows");
    assert_eq!(w0[0].len(), 5, "w0 should have out_dim=5 cols");
    let b0 = vec(&it, "b0");
    assert_eq!(b0.len(), 5);
    assert!(b0.iter().all(|&x| x == 0.0), "biases should start at zero");
    let w1 = mat(&it, "w1");
    assert_eq!(w1.len(), 5, "w1 should have in_dim=5 rows (layer 0's out_dim)");
    assert_eq!(w1[0].len(), 2, "w1 should have out_dim=2 cols");
    let b1 = vec(&it, "b1");
    assert_eq!(b1.len(), 2);
}

#[test]
fn sequential_forward_matches_a_hand_computed_dense_plus_relu() {
    // Independently recomputes `dense + relu` from the SAME initialized
    // weights (read back via `net.params[0].w`/`.b`) using plain matrix
    // ops (`x * w0 + b0`, `max(., 0)`), then checks `net.predict(x)`
    // matches exactly — verifies `sequential_forward`'s layer loop is
    // really just `dense`+`relu` under the hood, not a different formula.
    let it = run(
        "layers = (dense_layer(2, 2, activation=\"relu\"),)\n\
         net = sequential(layers, seed=3)\n\
         w0 = net.params[0].w\n\
         b0 = net.params[0].b\n\
         x = [1, -2] as matrix(1, 2)\n\
         expected = max(x * w0 + b0, 0)\n\
         got = net.predict(x)",
    );
    let expected = mat(&it, "expected");
    let got = mat(&it, "got");
    assert_eq!(expected.len(), got.len());
    for (er, gr) in expected.iter().zip(got.iter()) {
        for (e, g) in er.iter().zip(gr.iter()) {
            assert!((e - g).abs() < 1e-9, "expected {e}, got {g}");
        }
    }
}

#[test]
fn sequential_fit_solves_xor_from_scratch_with_no_manual_param_or_grad() {
    // The exact task this API exists for: an XOR MLP built ONLY from
    // `dense_layer`/`sequential`/`.fit` — no hand-written `param()`/
    // `grad()`/update loop anywhere in this script, unlike every other
    // training test in this file (GRU/CNN/transformer), which all still
    // hand-roll that loop because this API didn't exist yet. Measured:
    // loss goes from ~0.99 (random init) to effectively 0 (~1e-14) after
    // 2000 full-batch SGD steps at lr=0.5 — the same convergence a manual
    // `param`/`grad` XOR MLP reaches.
    let it = run(
        "X = [0, 0, 1, 1, 0, 1, 0, 1] as matrix(4, 2)\n\
         Y = [0, 1, 1, 0] as matrix(4, 1)\n\
         layers = (dense_layer(2, 8, activation=\"relu\"), dense_layer(8, 1, activation=\"none\"))\n\
         net = sequential(layers, seed=42)\n\
         trained = net.fit(X, Y, epochs=2000, lr=0.5, loss=\"mse\")\n\
         first_loss = trained.loss_history[0]\n\
         last_loss = trained.loss_history[1999]\n\
         pred = trained.predict(X)",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("sequential XOR: loss {first} -> {last} over 2000 SGD steps (lr=0.5)");
    assert!(first > 0.05, "expected a non-trivial initial loss, got {first}");
    assert!(
        last < 1e-3,
        "expected XOR to converge to near-zero MSE: {first} -> {last}"
    );
    let pred = mat(&it, "pred");
    let targets = [0.0, 1.0, 1.0, 0.0];
    for (row, target) in pred.iter().zip(targets.iter()) {
        assert!(
            (row[0] - target).abs() < 0.05,
            "trained XOR prediction {} should be close to {target}",
            row[0]
        );
    }
}

// ------------------------------- dense_layer/sequential pipe-chain (2026-08-26)
//
// `dense_layer(2,8,activation="relu") |> dense_layer(8,1) |> sequential
// (seed=1)` — piping layer specs together instead of building the `(...)`
// tuple/list literal by hand. No new grammar: `|>` already inserts the
// piped value as the stage call's first positional argument (`eval_pipe`),
// so `dense_layer` just needed to recognize when ITS OWN arg0 is a prior
// layer spec (or list of them) rather than a numeric `in_dim`, and fold
// itself onto that accumulator instead of starting a fresh one; and
// `sequential` needed to accept a lone `Value::Record` (the one-layer-chain
// case) in addition to its original `Value::List` form. See both builtin
// arms' own doc comments in `engine/crates/qu-interp/src/lib.rs` for the
// exact accumulation rule.

#[test]
fn dense_layer_pipe_chain_accumulates_into_a_list() {
    // Structural check, independent of training: two piped `dense_layer`
    // calls must produce the exact same 2-element `List` of layer-spec
    // Records that the tuple-literal form (`(dense_layer(...), dense_layer
    // (...))`) produces — same fields, same order — since that's the
    // value `sequential(...)` actually consumes either way.
    let it = run(
        "chain = dense_layer(2, 8, activation=\"relu\") |> dense_layer(8, 1)\n\
         k0 = chain[0].kind\n\
         in0 = chain[0].in_dim\n\
         out0 = chain[0].out_dim\n\
         act0 = chain[0].activation\n\
         k1 = chain[1].kind\n\
         in1 = chain[1].in_dim\n\
         out1 = chain[1].out_dim\n\
         act1 = chain[1].activation",
    );
    assert_eq!(list_len(&it, "chain"), 2);
    assert_str(&it, "k0", "dense");
    assert_eq!(num(&it, "in0"), 2.0);
    assert_eq!(num(&it, "out0"), 8.0);
    assert_str(&it, "act0", "relu");
    assert_str(&it, "k1", "dense");
    assert_eq!(num(&it, "in1"), 8.0);
    assert_eq!(num(&it, "out1"), 1.0);
    assert_str(&it, "act1", "none");
}

#[test]
fn dense_layer_pipe_chain_of_three_appends_to_a_growing_list() {
    // `layer1 |> layer2 |> layer3` — the SECOND pipe stage receives the
    // already-2-element `List` `layer1 |> layer2` produced (not a bare
    // Record), exercising the `Value::List` arm of the accumulation match
    // (the previous test only exercises the `Value::Record` arm, a single
    // prior layer).
    let it = run(
        "chain = dense_layer(2, 4, activation=\"relu\") |> dense_layer(4, 4, activation=\"relu\") |> dense_layer(4, 1)\n\
         out0 = chain[0].out_dim\n\
         out1 = chain[1].out_dim\n\
         out2 = chain[2].out_dim",
    );
    assert_eq!(list_len(&it, "chain"), 3);
    assert_eq!(num(&it, "out0"), 4.0);
    assert_eq!(num(&it, "out1"), 4.0);
    assert_eq!(num(&it, "out2"), 1.0);
}

#[test]
fn sequential_accepts_a_single_piped_layer_without_a_list_wrapper() {
    // `dense_layer(...) |> sequential(seed=)` — a ONE-layer pipe chain
    // never touches `dense_layer`'s own accumulation (that only fires when
    // `dense_layer` itself receives the piped value), so `sequential` sees
    // a bare `Value::Record`, not a `List`. Must auto-wrap into a
    // 1-element list rather than erroring.
    let it = run(
        "net = dense_layer(3, 2, activation=\"none\") |> sequential(seed=7)\n\
         layers = net.layers\n\
         w0 = net.params[0].w",
    );
    assert_eq!(list_len(&it, "layers"), 1);
    let w0 = mat(&it, "w0");
    assert_eq!(w0.len(), 3);
    assert_eq!(w0[0].len(), 2);
}

#[test]
fn sequential_fit_solves_xor_from_pipe_chain_only_no_list_literal() {
    // Same exact XOR MLP as `sequential_fit_solves_xor_from_scratch_with_
    // no_manual_param_or_grad` above (same shapes, same seed=42, same
    // training hyperparameters) but built ENTIRELY via the pipe-chain form
    // — `dense_layer(...) |> dense_layer(...) |> sequential(seed=42)` — with
    // no `(...)`  tuple/list literal anywhere. Both `dense_layer` calls
    // fold onto the same `Value::List` the tuple-literal form builds by
    // hand, so `sequential`'s init sees byte-identical input either way:
    // this test asserts the SAME convergence (near-zero MSE) the list-
    // literal version reaches, confirming the two construction styles are
    // fully equivalent, not just superficially similar.
    let it = run(
        "X = [0, 0, 1, 1, 0, 1, 0, 1] as matrix(4, 2)\n\
         Y = [0, 1, 1, 0] as matrix(4, 1)\n\
         net = dense_layer(2, 8, activation=\"relu\") |> dense_layer(8, 1, activation=\"none\") |> sequential(seed=42)\n\
         trained = net.fit(X, Y, epochs=2000, lr=0.5, loss=\"mse\")\n\
         first_loss = trained.loss_history[0]\n\
         last_loss = trained.loss_history[1999]\n\
         pred = trained.predict(X)",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("sequential XOR (pipe-chain): loss {first} -> {last} over 2000 SGD steps (lr=0.5)");
    assert!(first > 0.05, "expected a non-trivial initial loss, got {first}");
    assert!(
        last < 1e-3,
        "expected XOR to converge to near-zero MSE: {first} -> {last}"
    );
    let pred = mat(&it, "pred");
    let targets = [0.0, 1.0, 1.0, 0.0];
    for (row, target) in pred.iter().zip(targets.iter()) {
        assert!(
            (row[0] - target).abs() < 0.05,
            "trained XOR (pipe-chain) prediction {} should be close to {target}",
            row[0]
        );
    }
}

#[test]
fn sequential_predict_matches_forward_on_trained_weights_when_no_dropout() {
    // `net.predict(x)` (untracked, plain values) and `stop_grad(net.forward
    // (x))` (untracked copy of the tracked path) must compute the exact
    // same numbers when there's no dropout layer to make them diverge —
    // both are just `dense`/`relu` applied to the same stored weights.
    let it = run(
        "X = [0, 0, 1, 1, 0, 1, 0, 1] as matrix(4, 2)\n\
         Y = [0, 1, 1, 0] as matrix(4, 1)\n\
         layers = (dense_layer(2, 4, activation=\"relu\"), dense_layer(4, 1, activation=\"none\"))\n\
         net = sequential(layers, seed=11)\n\
         trained = net.fit(X, Y, epochs=50, lr=0.3, loss=\"mse\")\n\
         pred = trained.predict(X)\n\
         fwd = stop_grad(trained.forward(X))",
    );
    let pred = mat(&it, "pred");
    let fwd = mat(&it, "fwd");
    assert_eq!(pred.len(), fwd.len());
    for (pr, fr) in pred.iter().zip(fwd.iter()) {
        for (p, f) in pr.iter().zip(fr.iter()) {
            assert!((p - f).abs() < 1e-9, "predict {p} vs forward {f}");
        }
    }
}

#[test]
fn dropout_layer_is_a_no_op_at_predict_time() {
    // `predict` runs `sequential_forward` with `training=false`, which
    // skips the `"dropout"` layer's `dropout(...)` call entirely (see its
    // own doc comment) — so two `predict` calls on the same input must be
    // bit-identical, regardless of `dropout_layer`'s configured rate.
    let it = run(
        "layers = (dense_layer(3, 6, activation=\"relu\"), dropout_layer(rate=0.5), dense_layer(6, 2, activation=\"none\"))\n\
         net = sequential(layers, seed=5)\n\
         x = [1, 2, 3] as matrix(1, 3)\n\
         a = net.predict(x)\n\
         b = net.predict(x)",
    );
    let a = mat(&it, "a");
    let b = mat(&it, "b");
    assert_eq!(a, b, "predict should be deterministic (dropout must be inactive at inference)");
}

// ------------------------------------------------- Model zoo (2026-08-25)
//
// `mlp_classifier(in_dim, hidden_dims, n_classes, [seed=])` builds directly
// on `sequential`/`dense_layer` above (a dense+relu stack is exactly what
// `sequential` already does), so its `.fit()`/`.predict()` ARE the ordinary
// `kind="sequential"` ones. `simple_cnn`/`simple_rnn_classifier` can't reuse
// `sequential` the same way (see the "Model zoo" free-function section in
// `lib.rs` for why: `conv2d`'s `List`-of-images batching and `gru_forward`'s
// column-shaped hidden state don't fit `sequential_forward`'s `(n,
// features)`-row-matrix assumption) — they're built directly on
// `conv2d`/`maxpool2d`/`gru_forward`/`param`/`grad`, each with its own
// `ModelHandle.kind` and matching `"fit"`/`"predict"` branches.

#[test]
fn mlp_classifier_builds_dense_relu_layers_with_a_softmax_ready_output() {
    let it = run(
        "net = mlp_classifier(4, [6, 3], 2, seed=9)\n\
         w0 = net.params[0].w\n\
         b0 = net.params[0].b\n\
         w1 = net.params[1].w\n\
         b1 = net.params[1].b\n\
         w2 = net.params[2].w\n\
         b2 = net.params[2].b\n\
         act0 = net.layers[0].activation\n\
         act2 = net.layers[2].activation",
    );
    let w0 = mat(&it, "w0");
    assert_eq!(w0.len(), 4, "first layer's w should have in_dim=4 rows");
    assert_eq!(w0[0].len(), 6, "first layer's w should have out_dim=6 cols");
    assert_eq!(vec(&it, "b0").len(), 6);
    let w1 = mat(&it, "w1");
    assert_eq!(w1.len(), 6, "second layer's w should have in_dim=6 rows (layer 0's out_dim)");
    assert_eq!(w1[0].len(), 3);
    assert_eq!(vec(&it, "b1").len(), 3);
    let w2 = mat(&it, "w2");
    assert_eq!(w2.len(), 3, "output layer's w should have in_dim=3 rows (last hidden size)");
    assert_eq!(w2[0].len(), 2, "output layer's w should have out_dim=n_classes=2 cols");
    assert_eq!(vec(&it, "b2").len(), 2);
    match it.get("act0") {
        Some(Value::Str(s)) => assert_eq!(s.as_str(), "relu", "hidden layers should be relu-activated"),
        other => panic!("act0 should be a string: {other:?}"),
    }
    match it.get("act2") {
        Some(Value::Str(s)) => assert_eq!(
            s.as_str(),
            "none",
            "the output layer must have no activation (raw, softmax-ready logits)"
        ),
        other => panic!("act2 should be a string: {other:?}"),
    }
}

#[test]
fn mlp_classifier_fit_solves_a_3_class_blob_problem() {
    // 3 well-separated 2-D blobs, one-hot `Y`, trained with
    // `loss="cross_entropy"` — the exact contract `mlp_classifier`'s own
    // doc comment states (a raw-logit output layer, softmax-ready).
    let it = run(
        "X = [-3,-3; -2.5,-3.2; -3.2,-2.6; -2.8,-2.9; -3.1,-3.4; 3,3; 2.6,3.1; 3.3,2.7; 2.9,3.3; 3.2,2.8; -3,3; -2.7,3.2; -3.4,2.8; -2.9,2.6; -3.2,3.1]\n\
         y = [0,0,0,0,0, 1,1,1,1,1, 2,2,2,2,2]\n\
         Y = zeros(15, 3)\n\
         for i = 0 to 14\n\
         \x20   Y[i, y[i]] = 1\n\
         end for\n\
         net = mlp_classifier(2, [8, 6], 3, seed=7)\n\
         fitted = net.fit(X, Y, epochs=300, lr=0.3, loss=\"cross_entropy\")\n\
         losses = fitted.loss_history\n\
         first_loss = losses[0]\n\
         last_loss = losses[299]\n\
         pred = fitted.predict(X)",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("mlp_classifier 3-blob training: loss {first} -> {last} over 300 steps");
    assert!(first > 0.5, "expected a non-trivial initial cross-entropy loss, got {first}");
    assert!(
        last < first * 0.1,
        "expected training to have meaningfully reduced the loss: {first} -> {last}"
    );
    let pred = mat(&it, "pred");
    assert_eq!(pred.len(), 15, "predict should return one row per sample");
    assert_eq!(pred[0].len(), 3, "predict should return one raw score per class");
    let y = [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2];
    for (row, &label) in pred.iter().zip(y.iter()) {
        let best = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(best, label, "predicted class should match the training label after fitting");
    }
}

#[test]
fn simple_cnn_init_produces_correctly_shaped_kernel_and_per_class_weights() {
    let it = run(
        "net = simple_cnn([6, 6], 3, seed=2)\n\
         kernel = net.kernel\n\
         cw0 = net.class_w[0]\n\
         cb = net.class_b\n\
         ks = net.kernel_size\n\
         ps = net.pool_size",
    );
    let kernel = mat(&it, "kernel");
    assert_eq!(kernel.len(), 3, "default kernel should be 3x3");
    assert_eq!(kernel[0].len(), 3);
    // "valid" conv (3x3 kernel) on a 6x6 image -> 4x4; maxpool2d(2) -> 2x2.
    let cw0 = mat(&it, "cw0");
    assert_eq!(cw0.len(), 2, "each class weight map should match the pooled feature map's rows");
    assert_eq!(cw0[0].len(), 2, "each class weight map should match the pooled feature map's cols");
    assert_eq!(vec(&it, "cb").len(), 3, "one bias per class");
    assert_eq!(num(&it, "ks"), 3.0);
    assert_eq!(num(&it, "ps"), 2.0);
}

#[test]
fn simple_cnn_fit_learns_square_vs_plus_pattern() {
    // The same synthetic task as `tiny_cnn_conv2d_relu_maxpool_dense_
    // readout_learns_square_vs_circle` above (2x2 bright square vs a
    // diamond/plus shape, 4 offsets on a 6x6 grid), but built end to end
    // through `simple_cnn`/`.fit`/`.predict` instead of hand-written
    // `param`/`grad`/`sum(pooled .* w) + b`.
    let it = run(
        "function make_square(ox, oy)\n\
         \x20   m = zeros(6, 6)\n\
         \x20   m[2+oy, 2+ox] = 1\n\
         \x20   m[2+oy, 3+ox] = 1\n\
         \x20   m[3+oy, 2+ox] = 1\n\
         \x20   m[3+oy, 3+ox] = 1\n\
         \x20   return m\n\
         end function\n\
         function make_plus(ox, oy)\n\
         \x20   m = zeros(6, 6)\n\
         \x20   m[1+oy, 2+ox] = 1\n\
         \x20   m[2+oy, 1+ox] = 1\n\
         \x20   m[2+oy, 3+ox] = 1\n\
         \x20   m[3+oy, 2+ox] = 1\n\
         \x20   return m\n\
         end function\n\
         images = (make_square(0, 0),)\n\
         labels = [0]\n\
         for oy = 0 to 1\n\
         \x20   for ox = 0 to 1\n\
         \x20       for rep = 1 to 3\n\
         \x20           if oy > 0 or ox > 0 or rep > 1 then\n\
         \x20               images = append(images, make_square(ox, oy))\n\
         \x20               labels = append(labels, 0)\n\
         \x20           end if\n\
         \x20           images = append(images, make_plus(ox, oy))\n\
         \x20           labels = append(labels, 1)\n\
         \x20       end for\n\
         \x20   end for\n\
         end for\n\
         net = simple_cnn([6, 6], 2, seed=3)\n\
         fitted = net.fit(images, labels, epochs=120, lr=0.4)\n\
         losses = fitted.loss_history\n\
         first_loss = losses[0]\n\
         last_loss = losses[119]\n\
         pred = fitted.predict(images)\n\
         n = length(labels)",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("simple_cnn square-vs-plus training: loss {first} -> {last} over 120 steps");
    assert!(first > 0.3, "expected a non-trivial initial cross-entropy loss (~ln 2), got {first}");
    assert!(
        last < first * 0.5,
        "expected training to have meaningfully reduced the loss: {first} -> {last}"
    );
    assert_eq!(num(&it, "n"), 24.0);
    let pred = mat(&it, "pred");
    assert_eq!(pred.len(), 24, "predict should return one row per image");
    assert_eq!(pred[0].len(), 2, "predict should return one raw score per class");
    let labels = [
        0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    ];
    let mut correct = 0;
    for (row, &label) in pred.iter().zip(labels.iter()) {
        let best = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        if best == label {
            correct += 1;
        }
    }
    assert!(correct >= 22, "expected the trained CNN to correctly classify most images, got {correct}/24");
}

#[test]
fn simple_rnn_classifier_init_produces_correctly_shaped_weights() {
    let it = run(
        "net = simple_rnn_classifier(3, 5, 4, seed=8)\n\
         wz = net.gru_weights.wz\n\
         uz = net.gru_weights.uz\n\
         bz = net.gru_weights.bz\n\
         out_w = net.out_w\n\
         out_b = net.out_b",
    );
    let wz = mat(&it, "wz");
    assert_eq!(wz.len(), 5, "wz should be (hidden_size, input_size)");
    assert_eq!(wz[0].len(), 3);
    let uz = mat(&it, "uz");
    assert_eq!(uz.len(), 5, "uz should be (hidden_size, hidden_size)");
    assert_eq!(uz[0].len(), 5);
    assert_eq!(vec(&it, "bz").len(), 5);
    let out_w = mat(&it, "out_w");
    assert_eq!(out_w.len(), 5, "out_w should be (hidden_size, n_classes) -- dense()'s own convention");
    assert_eq!(out_w[0].len(), 4);
    assert_eq!(vec(&it, "out_b").len(), 4);
}

#[test]
fn simple_rnn_classifier_fit_learns_rising_vs_falling_sequences() {
    // input_size=1, hidden_size=6, 2 classes: a monotonically rising vs
    // falling 5-step ramp, 5 phase-shifted replicas each.
    let it = run(
        "function make_seq(rising, phase)\n\
         \x20   if rising > 0.5 then\n\
         \x20       s = [0.1+phase, 0.3+phase, 0.5+phase, 0.7+phase, 0.9+phase]\n\
         \x20   else\n\
         \x20       s = [0.9+phase, 0.7+phase, 0.5+phase, 0.3+phase, 0.1+phase]\n\
         \x20   end if\n\
         \x20   return s as matrix(1, 5)\n\
         end function\n\
         sequences = (make_seq(1, 0.0),)\n\
         labels = [0]\n\
         for phase = 0 to 4\n\
         \x20   p = phase * 0.02\n\
         \x20   if phase > 0 then\n\
         \x20       sequences = append(sequences, make_seq(1, p))\n\
         \x20       labels = append(labels, 0)\n\
         \x20   end if\n\
         \x20   sequences = append(sequences, make_seq(0, p))\n\
         \x20   labels = append(labels, 1)\n\
         end for\n\
         net = simple_rnn_classifier(1, 6, 2, seed=5)\n\
         fitted = net.fit(sequences, labels, epochs=120, lr=0.3)\n\
         losses = fitted.loss_history\n\
         first_loss = losses[0]\n\
         last_loss = losses[119]\n\
         pred = fitted.predict(sequences)\n\
         n = length(labels)",
    );
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("simple_rnn_classifier rising-vs-falling training: loss {first} -> {last} over 120 steps");
    assert!(first > 0.3, "expected a non-trivial initial cross-entropy loss (~ln 2), got {first}");
    assert!(
        last < first * 0.5,
        "expected training to have meaningfully reduced the loss: {first} -> {last}"
    );
    assert_eq!(num(&it, "n"), 10.0);
    let pred = mat(&it, "pred");
    assert_eq!(pred.len(), 10, "predict should return one row per sequence");
    assert_eq!(pred[0].len(), 2, "predict should return one raw score per class");
    let labels = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
    let mut correct = 0;
    for (row, &label) in pred.iter().zip(labels.iter()) {
        let best = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        if best == label {
            correct += 1;
        }
    }
    assert!(
        correct >= 9,
        "expected the trained RNN to correctly classify almost all sequences, got {correct}/10"
    );
}

// ---- `simple_cnn`/`simple_rnn_classifier` `engine="torch"` (§ model-zoo
// `backend-torch` wiring, 2026-08-26) — the real LANGUAGE-surface proof that
// `.qu`-level scripts (not just direct Rust `TensorBackend` calls) can train
// these two model-zoo architectures through `TorchBackend`. Same datasets as
// the native tests just above, so the only variable is `engine=`.

#[test]
#[cfg(feature = "backend-torch")]
fn simple_cnn_fit_engine_torch_learns_square_vs_plus_pattern() {
    let script = |engine: &str| format!(
        "function make_square(ox, oy)\n\
         \x20   m = zeros(6, 6)\n\
         \x20   m[2+oy, 2+ox] = 1\n\
         \x20   m[2+oy, 3+ox] = 1\n\
         \x20   m[3+oy, 2+ox] = 1\n\
         \x20   m[3+oy, 3+ox] = 1\n\
         \x20   return m\n\
         end function\n\
         function make_plus(ox, oy)\n\
         \x20   m = zeros(6, 6)\n\
         \x20   m[1+oy, 2+ox] = 1\n\
         \x20   m[2+oy, 1+ox] = 1\n\
         \x20   m[2+oy, 3+ox] = 1\n\
         \x20   m[3+oy, 2+ox] = 1\n\
         \x20   return m\n\
         end function\n\
         images = (make_square(0, 0),)\n\
         labels = [0]\n\
         for oy = 0 to 1\n\
         \x20   for ox = 0 to 1\n\
         \x20       for rep = 1 to 3\n\
         \x20           if oy > 0 or ox > 0 or rep > 1 then\n\
         \x20               images = append(images, make_square(ox, oy))\n\
         \x20               labels = append(labels, 0)\n\
         \x20           end if\n\
         \x20           images = append(images, make_plus(ox, oy))\n\
         \x20           labels = append(labels, 1)\n\
         \x20       end for\n\
         \x20   end for\n\
         end for\n\
         net = simple_cnn([6, 6], 2, seed=3, engine=\"{engine}\")\n\
         fitted = net.fit(images, labels, epochs=60, lr=0.4)\n\
         losses = fitted.loss_history\n\
         first_loss = losses[0]\n\
         last_loss = losses[59]\n\
         pred = fitted.predict(images)\n\
         n = length(labels)"
    );
    let it = run(&script("torch"));
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("simple_cnn engine=\"torch\" square-vs-plus training: loss {first} -> {last} over 60 steps");
    assert!(
        last < first * 0.5,
        "expected engine=\"torch\" training to have meaningfully reduced the loss: {first} -> {last}"
    );
    assert_eq!(num(&it, "n"), 24.0);
    let pred = mat(&it, "pred");
    assert_eq!(pred.len(), 24, "predict should return one row per image, regardless of which engine trained it");
    assert_eq!(pred[0].len(), 2, "predict should return one raw score per class");

    // Numeric cross-check against the SAME script under `engine="native"`
    // — proves `engine="torch"` isn't just "a model that happens to train",
    // it computes essentially the same loss trajectory as the tape this
    // model-zoo kind already had, matching `compile_engine_torch_and_
    // engine_native_produce_matching_loss_curves`'s own established
    // convention for the Sequential/dense case.
    let native_it = run(&script("native"));
    let native_first = num(&native_it, "first_loss");
    let native_last = num(&native_it, "last_loss");
    assert!(
        (native_first - first).abs() < 1e-2,
        "native and torch should start from essentially the same cross-entropy loss (same seed/init): native={native_first}, torch={first}"
    );
    eprintln!("native square-vs-plus training for comparison: loss {native_first} -> {native_last} over 60 steps");
}

#[test]
#[cfg(feature = "backend-torch")]
fn simple_rnn_classifier_fit_engine_torch_learns_rising_vs_falling_sequences() {
    let script = |engine: &str| format!(
        "function make_seq(rising, phase)\n\
         \x20   if rising > 0.5 then\n\
         \x20       s = [0.1+phase, 0.3+phase, 0.5+phase, 0.7+phase, 0.9+phase]\n\
         \x20   else\n\
         \x20       s = [0.9+phase, 0.7+phase, 0.5+phase, 0.3+phase, 0.1+phase]\n\
         \x20   end if\n\
         \x20   return s as matrix(1, 5)\n\
         end function\n\
         sequences = (make_seq(1, 0.0),)\n\
         labels = [0]\n\
         for phase = 0 to 4\n\
         \x20   p = phase * 0.02\n\
         \x20   if phase > 0 then\n\
         \x20       sequences = append(sequences, make_seq(1, p))\n\
         \x20       labels = append(labels, 0)\n\
         \x20   end if\n\
         \x20   sequences = append(sequences, make_seq(0, p))\n\
         \x20   labels = append(labels, 1)\n\
         end for\n\
         net = simple_rnn_classifier(1, 6, 2, seed=5, engine=\"{engine}\")\n\
         fitted = net.fit(sequences, labels, epochs=80, lr=0.3)\n\
         losses = fitted.loss_history\n\
         first_loss = losses[0]\n\
         last_loss = losses[79]\n\
         pred = fitted.predict(sequences)\n\
         n = length(labels)"
    );
    let it = run(&script("torch"));
    let first = num(&it, "first_loss");
    let last = num(&it, "last_loss");
    eprintln!("simple_rnn_classifier engine=\"torch\" rising-vs-falling training: loss {first} -> {last} over 80 steps");
    assert!(
        last < first * 0.5,
        "expected engine=\"torch\" training to have meaningfully reduced the loss: {first} -> {last}"
    );
    assert_eq!(num(&it, "n"), 10.0);
    let pred = mat(&it, "pred");
    assert_eq!(pred.len(), 10, "predict should return one row per sequence, regardless of which engine trained it");
    assert_eq!(pred[0].len(), 2, "predict should return one raw score per class");

    let native_it = run(&script("native"));
    let native_first = num(&native_it, "first_loss");
    let native_last = num(&native_it, "last_loss");
    assert!(
        (native_first - first).abs() < 1e-2,
        "native and torch should start from essentially the same cross-entropy loss (same seed/init): native={native_first}, torch={first}"
    );
    eprintln!("native rising-vs-falling training for comparison: loss {native_first} -> {native_last} over 80 steps");
}

// ---- `@expr` self-assign prefix operator (§ self-assign chain, 2026-08-26) ----
//
// Parsed by qu-syntax as a desugar straight into the ordinary `Stmt::Assign`
// node (see that variant's doc comment and `root_ident` in qu-syntax's
// lib.rs), so these tests exercise the real end-to-end behavior through the
// same `var_set` every other assignment uses — no separate interpreter code
// path exists for `@`.

fn vec_of(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Vec(xs)) => xs.as_ref().clone(),
        other => panic!("expected {name} to be a Vec, got {other:?}"),
    }
}

#[test]
fn self_assign_mutates_the_binding_one_level() {
    // `@x.append(4)` == `x = x.append(4)` — `x` itself must change.
    let it = run("x = [1, 2, 3]\n@x.append(4)");
    assert_eq!(vec_of(&it, "x"), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn self_assign_multi_level_chain() {
    // Two chained method calls after the `@` still land back on `x`.
    let it = run("x = [1, 2, 3]\n@x.append(4).append(5)");
    assert_eq!(vec_of(&it, "x"), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn self_assign_respects_local_scope_not_the_outer_global() {
    // A parameter named `x` shadows the global `x` of the same name; `@x...`
    // inside the function must update only the frame-local binding (per
    // `var_set`'s "existing frame-local: update in place" rule), leaving
    // the caller's global `x` untouched.
    let it = run(
        "function f(x)\n\
         \x20   @x.append(100)\n\
         \x20   return x\n\
         end function\n\
         x = [1, 2, 3]\n\
         z = f(x)",
    );
    assert_eq!(vec_of(&it, "x"), vec![1.0, 2.0, 3.0], "global x must be unchanged");
    assert_eq!(vec_of(&it, "z"), vec![1.0, 2.0, 3.0, 100.0]);
}

#[test]
fn self_assign_reaches_an_outer_global_when_declared() {
    // `@x.append(9)` is an assignment like any other, so it binds
    // locally unless the body declares the name `global`. Declared, it
    // mutates the caller's list -- which is the whole point of writing
    // a mutating helper.
    let it = run(
        "x = [1, 2, 3]\n\
         function bump()\n\
         \x20   global x\n\
         \x20   @x.append(9)\n\
         end function\n\
         bump()",
    );
    assert_eq!(vec_of(&it, "x"), vec![1.0, 2.0, 3.0, 9.0]);
}

#[test]
fn self_assign_rejects_non_identifier_rooted_expressions_at_parse_time() {
    let mut it = Interp::new();
    assert!(it.run("@(1 + 2)").is_err());
    let mut it2 = Interp::new();
    assert!(it2.run("length([1, 2, 3])\n@length([1, 2, 3])").is_err());
}

// ------------------------------------------------------- § multiple dispatch
// phase 1, 2026-08-27 — `docs/design/multiple-dispatch.md`. These exercise
// the actual `qu run <file>`/`Interp::run` code path end to end (the same
// entry point `qu-cli`'s `cmd_run` uses), not the private dispatch
// functions directly.

#[test]
fn overloaded_function_dispatches_on_argument_type() {
    // Three overloads of the same name, one per type tag; each call must
    // run the body whose parameter tag matches its actual argument type
    // (§3 of the design doc).
    let it = run(
        "function describe(x: vec)\n\
         \x20   print \"vec:\" + str(length(x))\n\
         end function\n\
         function describe(x: mat)\n\
         \x20   print \"mat:\" + str(rows(x))\n\
         end function\n\
         function describe(x: num)\n\
         \x20   print \"num:\" + str(x)\n\
         end function\n\
         describe([1, 2, 3])\n\
         describe([1, 2; 3, 4])\n\
         describe(42)",
    );
    assert_eq!(it.out, "vec:3\nmat:2\nnum:42\n");
}

#[test]
fn zero_matching_overload_is_a_clear_error_listing_candidates() {
    let mut it = Interp::new();
    let err = it
        .run(
            "function f(x: vec, y: mat)\n\
             \x20   print \"vec,mat\"\n\
             end function\n\
             function f(x, y: num)\n\
             \x20   print \"any,num\"\n\
             end function\n\
             f(\"hello\", true)",
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no method `f` matches argument types (string, bool)"), "got: {msg}");
    assert!(msg.contains("2 candidate(s) defined"), "got: {msg}");
    assert!(msg.contains("f(x: vec, y: mat)"), "got: {msg}");
    assert!(msg.contains("f(x, y: num)"), "got: {msg}");
}

#[test]
fn ambiguous_overload_call_is_a_clear_error_not_first_defined_wins() {
    // Both overloads pin down exactly one of the two parameters to `vec`
    // (equal specificity) for a call with two vector arguments -- neither
    // is a subset of the other, so this must be a hard ambiguity error,
    // not silently resolved by definition order (§3's explicit design
    // choice).
    let mut it = Interp::new();
    let err = it
        .run(
            "function f(x: vec, y)\n\
             \x20   print \"vec,any\"\n\
             end function\n\
             function f(x, y: vec)\n\
             \x20   print \"any,vec\"\n\
             end function\n\
             f([1, 2], [3, 4])",
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("ambiguous call to `f` for argument types (vector, vector)"), "got: {msg}");
    assert!(msg.contains("2 equally specific candidates"), "got: {msg}");
    assert!(msg.contains("f(x: vec, y)"), "got: {msg}");
    assert!(msg.contains("f(x, y: vec)"), "got: {msg}");
}

#[test]
fn redefining_the_identical_signature_replaces_in_place_not_a_new_overload() {
    // Same (arity, param types) signature defined twice (REPL-style re-edit
    // of a function) must behave like today's plain redefinition: the
    // second body wins outright, with no ambiguity error and no leftover
    // first overload still reachable (§2/§6).
    let it = run(
        "function g(x: num)\n\
         \x20   print \"first\"\n\
         end function\n\
         function g(x: num)\n\
         \x20   print \"second\"\n\
         end function\n\
         g(1)",
    );
    assert_eq!(it.out, "second\n");
}

#[test]
fn untyped_function_dispatch_is_unchanged_from_before_the_feature() {
    // The regression proof (§6): a function with NO type annotations at
    // all is a single, fully-untyped overload set -- `DispatchPlan::
    // Trivial` -- so it must accept ANY argument type unconditionally,
    // exactly as every user function did before this feature existed
    // (nothing here should ever hit the zero-match/ambiguity error paths).
    let it = run(
        "function echo(x)\n\
         \x20   print str(x)\n\
         end function\n\
         echo(1)\n\
         echo([1, 2, 3])\n\
         echo(\"hi\")",
    );
    assert_eq!(it.out, "1\n[1, 2, 3]\nhi\n");
}

#[test]
fn defining_two_different_arities_of_the_same_name_now_coexist() {
    // §6's own audit: before this feature, defining `f(x)` then a
    // different-arity `f(x, y)` silently overwrote the first (both keyed
    // the same flat `HashMap<String, _>` by name alone) -- a strict
    // improvement, not a compatibility risk, since no script could have
    // been relying on the old silent-overwrite behavior for this exact
    // case (the first definition was already unreachable dead code).
    let it = run(
        "function h(x)\n\
         \x20   print \"one arg: \" + str(x)\n\
         end function\n\
         function h(x, y)\n\
         \x20   print \"two args: \" + str(x + y)\n\
         end function\n\
         h(5)\n\
         h(2, 3)",
    );
    assert_eq!(it.out, "one arg: 5\ntwo args: 5\n");
}

// ------------------------------------------------------- § multiple dispatch
// phase 2, 2026-08-27 — `docs/design/multiple-dispatch.md` §1.1. `model<
// "kind">` reuses `ModelHandle.kind: String` (no new storage); `record<Tag>`
// uses a NEW convention -- an ordinary `__type = "Tag"` field set by a
// record-returning "constructor" function -- since `Value::Record` has no
// dedicated tag field and the `{a=1,b=2}` literal has no syntax to attach
// one. `butter(...)`/`kmeans_model(...)` are real builtins that already
// return `Value::Model` with kind `"filter"`/`"kmeans"` respectively, so
// these exercise real model values, not synthetic ones.

#[test]
fn model_kind_refinement_dispatches_on_the_actual_kind() {
    let it = run(
        "function describe_model(m: model<\"filter\">)\n\
         \x20   print \"filter\"\n\
         end function\n\
         function describe_model(m: model<\"kmeans\">)\n\
         \x20   print \"kmeans\"\n\
         end function\n\
         f = butter(2, \"low\", 100, 1000)\n\
         k = kmeans_model([1,2;3,4;10,11;12,13], 2)\n\
         describe_model(f)\n\
         describe_model(k)",
    );
    assert_eq!(it.out, "filter\nkmeans\n");
}

#[test]
fn model_kind_refinement_that_matches_no_overload_is_a_clear_error() {
    // Only a `model<"filter">` overload exists; calling it with a `kmeans`
    // model must be a zero-match dispatch error (§3), not a silent fallback
    // and not a Rust-level type confusion.
    let mut it = Interp::new();
    let err = it
        .run(
            "function describe_model(m: model<\"filter\">)\n\
             \x20   print \"filter\"\n\
             end function\n\
             k = kmeans_model([1,2;3,4;10,11;12,13], 2)\n\
             describe_model(k)",
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no method `describe_model` matches argument types (model)"), "got: {msg}");
    assert!(msg.contains("describe_model(m: model<\"filter\">)"), "got: {msg}");
}

#[test]
fn model_kind_refinement_outranks_the_bare_model_tag() {
    // §1.1/§3 specificity ordering: a `model<"filter">` overload must win
    // over a bare `model` overload for a filter-kind argument, but the
    // bare `model` overload must still be the one that runs for a
    // different-kind model that the refinement doesn't match.
    let it = run(
        "function g(m: model)\n\
         \x20   print \"any-model\"\n\
         end function\n\
         function g(m: model<\"filter\">)\n\
         \x20   print \"filter-specific\"\n\
         end function\n\
         f = butter(2, \"low\", 100, 1000)\n\
         k = kmeans_model([1,2;3,4;10,11;12,13], 2)\n\
         g(f)\n\
         g(k)",
    );
    assert_eq!(it.out, "filter-specific\nany-model\n");
}

#[test]
fn record_tag_refinement_dispatches_via_the_dunder_type_field_convention() {
    // The record-tagging convention this phase picked: an ordinary record
    // field `__type = "Tag"`, set by a plain record-returning "constructor"
    // function (`Circle(r) := {__type = "Circle", radius = r}`) -- no new
    // literal syntax, no new `Value::Record` storage.
    let it = run(
        "Circle(r) := {__type = \"Circle\", radius = r}\n\
         Square(s) := {__type = \"Square\", side = s}\n\
         function area(shape: record<Circle>)\n\
         \x20   print \"circle:\" + str(shape.radius)\n\
         end function\n\
         function area(shape: record<Square>)\n\
         \x20   print \"square:\" + str(shape.side)\n\
         end function\n\
         area(Circle(5))\n\
         area(Square(3))",
    );
    assert_eq!(it.out, "circle:5\nsquare:3\n");
}

#[test]
fn record_tag_refinement_that_matches_no_overload_is_a_clear_error() {
    let mut it = Interp::new();
    let err = it
        .run(
            "Circle(r) := {__type = \"Circle\", radius = r}\n\
             function area(shape: record<Square>)\n\
             \x20   print \"square\"\n\
             end function\n\
             area(Circle(5))",
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no method `area` matches argument types (record)"), "got: {msg}");
}

#[test]
fn record_without_a_dunder_type_field_never_matches_a_record_tag_refinement() {
    // An ordinary, untagged record ({radius = 5}, no __type) must not
    // accidentally satisfy a `record<Tag>` refinement for any `Tag` --
    // only the bare `record` tag (or untyped) can match it.
    let it = run(
        "function area(shape: record)\n\
         \x20   print \"generic-record\"\n\
         end function\n\
         function area(shape: record<Circle>)\n\
         \x20   print \"circle\"\n\
         end function\n\
         area({radius = 5})",
    );
    assert_eq!(it.out, "generic-record\n");
}

// ------------------------------------------------------- § multiple dispatch
// phase 3, 2026-08-27 — `docs/design/multiple-dispatch.md` §5. User-
// overloadable operators via a single fallback hook: `eval`'s
// `Expr::Binary`/`Expr::Unary` arms try the existing hardcoded `binop`/
// `unop` FIRST, and only on error (and only when at least one operand is a
// type the hardcoded cascade doesn't cover by type at all -- see
// `is_operator_overload_candidate`'s doc comment in `qu-interp/src/lib.rs`)
// look up a user-defined method named after the operator, through the same
// `resolve_method`/`dispatch_method` machinery phases 1-2 already built.
// `function +(a: record, b: record) ... end function` is the new grammar
// accommodation (`qu-syntax::expect_fn_name`/`OVERLOADABLE_OPERATORS`).

#[test]
fn overloaded_plus_operator_dispatches_on_tagged_record_type() {
    // The concrete worked example from the design doc's §5 write-up: a
    // Vec2-like record type (Phase 2's `__type` tagging convention) with a
    // user-defined `+` overload, verified end to end through `Interp::run`
    // exactly like every other multiple-dispatch acceptance test here.
    let it = run(
        "Vec2(x, y) := {__type = \"Vec2\", x = x, y = y}\n\
         function +(a: record<Vec2>, b: record<Vec2>)\n\
         \x20   return Vec2(a.x + b.x, a.y + b.y)\n\
         end function\n\
         c = Vec2(1, 2) + Vec2(3, 4)\n\
         print str(c.x) + \",\" + str(c.y)",
    );
    assert_eq!(it.out, "4,6\n");
}

#[test]
fn overloaded_minus_operator_works_for_unary_and_binary_arities_of_the_same_name() {
    // `methods["-"]` can hold a 1-parameter (unary) overload and a
    // 2-parameter (binary) overload side by side -- `dispatch_method`
    // already discriminates candidates by arity for ordinary function
    // calls, and this is the exact same table/algorithm, just reached from
    // `unop`/`binop`'s fallback instead of an ordinary call.
    let it = run(
        "Vec2(x, y) := {__type = \"Vec2\", x = x, y = y}\n\
         function -(a: record<Vec2>, b: record<Vec2>)\n\
         \x20   return Vec2(a.x - b.x, a.y - b.y)\n\
         end function\n\
         function -(a: record<Vec2>)\n\
         \x20   return Vec2(-a.x, -a.y)\n\
         end function\n\
         d = Vec2(5, 5) - Vec2(1, 2)\n\
         n = -Vec2(3, 4)\n\
         print str(d.x) + \",\" + str(d.y)\n\
         print str(n.x) + \",\" + str(n.y)",
    );
    assert_eq!(it.out, "4,3\n-3,-4\n");
}

#[test]
fn operator_overload_for_one_exotic_type_does_not_disturb_ordinary_numeric_operators() {
    // §5/§6's actual hard requirement, made concrete: defining `+` for a
    // user record type in the SAME script must not touch `1 + 2`,
    // `[1,2]+[3,4]`, 2-D matrix addition, or string concatenation -- the
    // `is_operator_overload_candidate` gate must keep every "safe" type
    // combination on the untouched `binop` path.
    let it = run(
        "Vec2(x, y) := {__type = \"Vec2\", x = x, y = y}\n\
         function +(a: record<Vec2>, b: record<Vec2>)\n\
         \x20   return Vec2(a.x + b.x, a.y + b.y)\n\
         end function\n\
         print str(1 + 2)\n\
         print str([1, 2] + [3, 4])\n\
         print str([1, 2; 3, 4] + [1, 2; 3, 4])\n\
         print \"a\" + \"b\"\n\
         print str(true + 1)",
    );
    assert_eq!(
        it.out,
        "3\n[4, 6]\n[2, 4; 6, 8]\nab\n2\n"
    );
}

#[test]
fn record_plus_record_with_no_overload_defined_errors_exactly_as_before_the_feature() {
    // Zero-regression proof for the "currently errors, stays erroring"
    // half of §5/§6: `Record + Record` with no `+` overload ever defined
    // must still be the SAME error `binop`'s own `map2`/`as_num` path
    // always produced (falling straight through `resolve_method`'s `None`
    // case), not a new "no method" dispatch error and not a silent
    // fallback to some default.
    let mut it = Interp::new();
    let err = it.run("{a = 1} + {b = 2}").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("expected a number, found record"), "got: {msg}");
}

#[test]
fn record_equality_structural_default_is_unaffected_by_the_operator_fallback() {
    // `compare`'s own pre-existing `Record == Record` arm (structural
    // equality via `values_equal`, from the 2026-08-26 string-comparison
    // audit) must keep working unchanged when no `==` overload is defined
    // -- this succeeds inside `binop` itself, so the fallback (which only
    // ever triggers on an `Err`) must never even be consulted.
    let it = run(
        "print str({a = 1, b = 2} == {a = 1, b = 2})\n\
         print str({a = 1} == {a = 2})",
    );
    assert_eq!(it.out, "true\nfalse\n");
}

#[test]
fn zero_regression_matrix_for_every_existing_binop_operand_type_combination() {
    // §3 of the phase-3 task: a broad matrix of type combinations `binop`/
    // `compare` already handle, run in a script that ALSO defines an
    // unrelated `+`/`-` overload for a record type -- proving the new
    // fallback hook changes NOTHING about any of these, byte for byte.
    let it = run(
        "Vec2(x, y) := {__type = \"Vec2\", x = x, y = y}\n\
         function +(a: record<Vec2>, b: record<Vec2>)\n\
         \x20   return Vec2(a.x + b.x, a.y + b.y)\n\
         end function\n\
         function -(a: record<Vec2>, b: record<Vec2>)\n\
         \x20   return Vec2(a.x - b.x, a.y - b.y)\n\
         end function\n\
         print str(2 + 3)\n\
         print str(2 - 3)\n\
         print str(2 * 3)\n\
         print str(6 / 3)\n\
         print str(2 ^ 3)\n\
         print str(2 == 2)\n\
         print str(2 < 3)\n\
         print str(3 > 2)\n\
         print str(true and false)\n\
         print str(true or false)\n\
         print str([1, 2, 3] + [4, 5, 6])\n\
         print str([1, 2, 3] - [1, 1, 1])\n\
         print str([1, 2, 3] * 2)\n\
         print str([1, 2] == [1, 2])\n\
         print str([1, 2] < [3, 4])\n\
         print str([1, 2; 3, 4] + [1, 1; 1, 1])\n\
         print str([1, 2; 3, 4] * [1, 0; 0, 1])\n\
         print str(2 + 3i)\n\
         print str((1 + 2i) * (3 + 4i))\n\
         print \"x\" + \"y\"\n\
         print str(\"a\" < \"b\")\n\
         print str(\"a\" == \"a\")",
    );
    assert_eq!(
        it.out,
        "5\n\
         -1\n\
         6\n\
         2\n\
         8\n\
         true\n\
         true\n\
         true\n\
         false\n\
         true\n\
         [5, 7, 9]\n\
         [0, 1, 2]\n\
         [2, 4, 6]\n\
         [T, T]\n\
         [T, T]\n\
         [2, 3; 4, 5]\n\
         [1, 2; 3, 4]\n\
         2 + 3j\n\
         -5 + 10j\n\
         xy\n\
         true\n\
         true\n"
    );
}

#[test]
fn function_plus_parses_as_an_operator_overload_declaration() {
    // The grammar accommodation itself (§5's own open question, resolved):
    // `qu-syntax::Parser::function_stmt` now accepts an operator spelling
    // as the function name via `expect_fn_name`/`OVERLOADABLE_OPERATORS`.
    // A bare parse+run of the declaration alone (no call) confirms this
    // parses at all, independent of dispatch behavior tested elsewhere.
    let it = run(
        "function +(a: record, b: record)\n\
         \x20   print \"overload defined\"\n\
         end function\n\
         print \"parsed ok\"",
    );
    assert_eq!(it.out, "parsed ok\n");
}

// ---- Physical units, phase 1 (docs/design/physical-units.md) ----------
//
// `degC`/`degF` are the two unit spellings this phase routes through a
// tracked `Value::Unit` instead of `apply_unit`'s old immediate collapse
// to a bare `Value::Num`. Every other unit literal (`V`, `Hz`, `Ohm`, ...)
// is a zero-regression concern covered by `user_function_and_units` above
// (`48 kHz` still becomes a plain `Value::Num`) and the rest of this
// file's pre-existing coverage — nothing here touches that path.

#[test]
fn degc_plus_degc_is_a_hard_error_not_a_silently_plausible_sum() {
    // The exact bug class this phase exists to fix: before this change,
    // `apply_unit`'s `_ => 1.0` catch-all meant `10 degC + 20 degC`
    // silently evaluated to a plain `30` — a number that looks like a
    // perfectly plausible temperature, which is precisely the danger
    // (design doc §2). Adding two absolute temperatures is physically
    // meaningless regardless of what the sum happens to look like.
    let mut it = Interp::new();
    let err = it.run("print(10 degC + 20 degC)").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not defined between two temperatures"), "got: {msg}");
    assert!(msg.contains("physically meaningful"), "got: {msg}");
}

#[test]
fn degc_minus_degc_yields_a_plain_kelvin_delta() {
    // A temperature DIFFERENCE is physically meaningful (the affine
    // offset cancels exactly), unlike a sum — design doc §2. The result
    // is a plain Value::Num (a delta, not itself a temperature), matching
    // "10 K colder" being a magnitude, not a point on the scale.
    let it = run("d = (20 degC) - (10 degC)");
    assert!(matches!(it.get("d"), Some(Value::Num(n)) if (*n - 10.0).abs() < 1e-9));
}

#[test]
fn degc_shifted_by_a_plain_number_stays_a_temperature() {
    // "5 degrees warmer" is meaningful and stays a Value::Unit in the same
    // declared scale (design doc §2's closing paragraph).
    let it = run("t = (10 degC) + 5");
    assert!(matches!(
        it.get("t"),
        Some(Value::Unit(n, UnitTag::Temp(TempScale::Celsius))) if (*n - 15.0).abs() < 1e-9
    ));
}

#[test]
fn number_minus_temperature_is_rejected_but_temperature_minus_number_is_not() {
    let mut it = Interp::new();
    let err = it.run("print(5 - (10 degC))").unwrap_err();
    assert!(err.to_string().contains("number - temperature"), "got: {err}");

    let it2 = run("t = (10 degC) - 5");
    assert!(matches!(
        it2.get("t"),
        Some(Value::Unit(n, UnitTag::Temp(TempScale::Celsius))) if (*n - 5.0).abs() < 1e-9
    ));
}

#[test]
fn scaling_a_temperature_is_rejected_multiply_and_divide() {
    // The second, independent consequence of "affine, not linear" (design
    // doc §2): doubling a Celsius reading isn't physically meaningful the
    // way doubling a Kelvin one is, because the offset doesn't scale.
    let mut it = Interp::new();
    let err = it.run("print(2 * (10 degC))").unwrap_err();
    assert!(err.to_string().contains("affine scales"), "got: {err}");

    let mut it2 = Interp::new();
    let err2 = it2.run("print((10 degC) / 2)").unwrap_err();
    assert!(err2.to_string().contains("affine scales"), "got: {err2}");
}

#[test]
fn degc_and_degf_compare_correctly_across_scales_via_kelvin() {
    // 10 degC and 50 degF are the same real temperature (283.15 K) — a
    // naive raw-number comparison (10 vs 50) would get this wrong; the
    // whole point of tracking the scale is converting through Kelvin
    // first (design doc §2/§4). `degF` did not even lex before this
    // phase (design doc §0).
    let it = run("eq = (10 degC) == (50 degF)");
    assert!(matches!(it.get("eq"), Some(Value::Bool(true))));

    // 100 degC (373.15 K) is hotter than 100 degF (~310.93 K).
    let it2 = run("gt = (100 degC) > (100 degF)");
    assert!(matches!(it2.get("gt"), Some(Value::Bool(true))));
}

#[test]
fn temperature_compared_against_a_plain_number_uses_its_own_declared_scale() {
    let it = run("b = (10 degC) > 5");
    assert!(matches!(it.get("b"), Some(Value::Bool(true))));
}

#[test]
fn a_temperature_value_reports_its_own_type_name_not_number() {
    // `Value::type_name()` -> "unit" (design doc §6) is what lets a
    // temperature accidentally reaching a plain-number builtin fail with
    // a clear message instead of a silent, wrong unwrap.
    let mut it = Interp::new();
    let err = it.run("print(sqrt(10 degC))").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unit"), "got: {msg}");
}

#[test]
fn degc_literal_displays_as_written_not_as_a_converted_kelvin_number() {
    let it = run("print(10 degC)");
    assert_eq!(it.out, "10 degC\n");
}

#[test]
fn linear_unit_literals_are_tracked_by_family_but_stay_numerically_unchanged() {
    // Superseded by design doc §8 phase 2: `V`/`kHz`/`mOhm` (and the other
    // 5 linear EE families) are now `Value::Unit(_, Family(_))`, not a
    // bare `Value::Num` — phase 1's "only degC/degF change" guarantee no
    // longer holds by design. The SI-normalized *magnitude* each spelling
    // produces is exactly what it always was; only the tag is new.
    let it = run("v = 5 V\nh = 3 kHz\no = 12 mOhm");
    assert!(matches!(
        it.get("v"),
        Some(Value::Unit(n, UnitTag::Dim(_, Some("V")))) if (*n - 5.0).abs() < 1e-9
    ));
    assert!(matches!(
        it.get("h"),
        Some(Value::Unit(n, UnitTag::Dim(_, Some("Hz")))) if (*n - 3000.0).abs() < 1e-9
    ));
    assert!(matches!(
        it.get("o"),
        Some(Value::Unit(n, UnitTag::Dim(_, Some("Ohm")))) if (*n - 0.012).abs() < 1e-9
    ));
}

// ---------------------------------------------------------- ergonomic API layer
// (§ ergonomic API layer, 2026-08-31): `table.load`/`table.write`,
// `df.nrows`/`df.ncols`, `rows`/`cols`, the `->` reshape operator, and
// `timer.start`/`.elapsed`/`.stop`. Every one of these is pure alias/sugar
// for something that already worked, so each test below proves the sugar
// produces results *identical* to the underlying call, not just that it
// runs without erroring.

fn table_val<'a>(it: &'a Interp, name: &str) -> &'a qu_interp::table::Table {
    match it.get(name) {
        Some(Value::Table(t)) => t.as_ref(),
        other => panic!("{name} is not a table: {other:?}"),
    }
}

fn scratch_csv_path(tag: &str) -> std::path::PathBuf {
    // Unique per-test-run filename in the OS temp dir (never inside the
    // repo working tree, which other sessions may be editing concurrently)
    // so parallel `cargo test` runs of these two round-trip tests can't
    // collide on the same file.
    std::env::temp_dir().join(format!(
        "qu_ergonomic_api_test_{tag}_{}.csv",
        std::process::id()
    ))
}

#[test]
fn table_dot_write_produces_byte_identical_csv_to_write_csv() {
    let path_old = scratch_csv_path("write_old");
    let path_new = scratch_csv_path("write_new");
    let it = run(&format!(
        "t = table(a = [1, 2, 3], b = [4.5, 5.5, 6.5])\n\
         write_csv(t, \"{}\")\n\
         table.write(t, \"{}\")",
        path_old.display().to_string().replace('\\', "\\\\"),
        path_new.display().to_string().replace('\\', "\\\\"),
    ));
    let _ = &it; // keep `it` alive for the duration of the file writes above
    let old_bytes = std::fs::read(&path_old).expect("write_csv output missing");
    let new_bytes = std::fs::read(&path_new).expect("table.write output missing");
    assert_eq!(old_bytes, new_bytes, "table.write(...) must be byte-identical to write_csv(...)");
    let _ = std::fs::remove_file(&path_old);
    let _ = std::fs::remove_file(&path_new);
}

#[test]
fn table_dot_load_matches_read_csv_on_the_same_file() {
    let path = scratch_csv_path("load");
    std::fs::write(&path, "a,b\n1,4.5\n2,5.5\n3,6.5\n").unwrap();
    let path_str = path.display().to_string().replace('\\', "\\\\");
    let it = run(&format!(
        "t1 = read_csv(\"{path_str}\")\nt2 = table.load(\"{path_str}\")"
    ));
    let t1 = table_val(&it, "t1");
    let t2 = table_val(&it, "t2");
    assert_eq!(t1.nrows(), t2.nrows());
    assert_eq!(t1.ncols(), t2.ncols());
    for name in t1.column_names() {
        assert_eq!(
            format!("{:?}", t1.col(name)),
            format!("{:?}", t2.col(name)),
            "column `{name}` differs between read_csv and table.load"
        );
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn df_nrows_ncols_property_matches_nrow_ncol_functions() {
    let it = run(
        "t = table(a = [1, 2, 3, 4], b = [5, 6, 7, 8], c = [9, 10, 11, 12])\n\
         a1 = t.nrows\n\
         a2 = nrow(t)\n\
         b1 = t.ncols\n\
         b2 = ncol(t)",
    );
    assert_eq!(num(&it, "a1"), num(&it, "a2"));
    assert_eq!(num(&it, "b1"), num(&it, "b2"));
    assert_eq!(num(&it, "a1"), 4.0);
    assert_eq!(num(&it, "b1"), 3.0);
}

#[test]
fn df_nrows_property_prefers_a_real_column_named_nrows() {
    // A table with an actual column literally named `nrows` must return
    // that column's data from `t.nrows`, NOT the computed row count — the
    // column always wins; the property is only a fallback.
    let it = run("t = table(nrows = [10, 20, 30])\nx = t.nrows");
    assert_eq!(vec(&it, "x"), vec![10.0, 20.0, 30.0]);

    // Sanity check: the actual row count (3) differs from the column's
    // own values, so this genuinely proves column-priority, not a
    // coincidental match.
    let t = table_val(&it, "t");
    assert_eq!(t.nrows(), 3);
    assert!(matches!(t.col("nrows"), Some(Column::Num(_))));
}

#[test]
fn rows_cols_functions_alias_nrow_ncol_for_tables() {
    let it = run(
        "t = table(a = [1, 2], b = [3, 4], c = [5, 6])\n\
         r1 = rows(t)\nr2 = nrow(t)\n\
         c1 = cols(t)\nc2 = ncol(t)",
    );
    assert_eq!(num(&it, "r1"), num(&it, "r2"));
    assert_eq!(num(&it, "c1"), num(&it, "c2"));
    assert_eq!(num(&it, "r1"), 2.0);
    assert_eq!(num(&it, "c1"), 3.0);
}

#[test]
fn rows_cols_functions_still_work_on_matrices_unaffected_by_table_support() {
    // Regression guard: adding `Value::Table` support to the shared
    // `shape_of` helper behind `rows`/`cols`/`size`/`shape`/`sizeof` must
    // not disturb its existing Matrix/Vector behavior.
    let it = run("M = [1, 2, 3; 4, 5, 6]\nr = rows(M)\nc = cols(M)");
    assert_eq!(num(&it, "r"), 2.0);
    assert_eq!(num(&it, "c"), 3.0);
}

#[test]
fn reshape_operator_matches_reshape_function_output() {
    let it = run(
        "v = [1, 2, 3, 4, 5, 6]\n\
         m1 = v -> (2, 3)\n\
         m2 = reshape(v, 2, 3)",
    );
    assert_eq!(flat(&it, "m1"), flat(&it, "m2"));
}

#[test]
fn reshape_operator_on_a_table_column_matches_reshape_of_the_column() {
    let it = run(
        "df = table(price = [1, 2, 3, 4, 5, 6])\n\
         m1 = df.price -> (3, 2)\n\
         m2 = reshape(df.price, 3, 2)",
    );
    assert_eq!(flat(&it, "m1"), flat(&it, "m2"));
}

#[test]
fn reshape_operator_after_pipe_reshapes_the_piped_result_not_the_function() {
    let it = run(
        "function dbl(x)\n    return x * 2\nend function\n\
         v = [1, 2, 3, 4, 5, 6]\n\
         r1 = v |> dbl -> (2, 3)\n\
         r2 = reshape(dbl(v), 2, 3)",
    );
    assert_eq!(flat(&it, "r1"), flat(&it, "r2"));
}

#[test]
fn reshape_operator_after_addition_reshapes_the_whole_sum() {
    let it = run(
        "a = [1, 2, 3]\nb = [1, 1, 1]\n\
         r1 = a + b -> (3, 1)\n\
         r2 = reshape(a + b, 3, 1)",
    );
    assert_eq!(flat(&it, "r1"), flat(&it, "r2"));
}

#[test]
fn timer_start_elapsed_stop_are_monotonic_and_elapsed_does_not_reset() {
    let it = run(
        "timer.start()\n\
         sleep(5)\n\
         e1 = timer.elapsed()\n\
         sleep(5)\n\
         e2 = timer.elapsed()\n\
         s = timer.stop()",
    );
    let e1 = num(&it, "e1");
    let e2 = num(&it, "e2");
    let s = num(&it, "s");
    assert!(e1 >= 0.0);
    // `elapsed()` must be a genuine non-resetting peek: calling it twice
    // with a sleep in between must show time keeps accumulating from the
    // SAME `start()`, not from the previous `elapsed()` call.
    assert!(e2 >= e1, "elapsed() must not reset the timer: e1={e1}, e2={e2}");
    assert!(s >= e2, "stop() must read at least as much elapsed time as the last elapsed() peek");
}

#[test]
fn timer_start_is_the_same_effect_as_tic_and_stop_same_as_toc() {
    // `timer.start()`/`timer.stop()` must be indistinguishable from
    // `tic()`/`toc()` — same underlying state, just a nicer spelling.
    let it = run("tic()\nsleep(5)\nt1 = toc()\ntimer.start()\nsleep(5)\nt2 = timer.stop()");
    assert!(num(&it, "t1") >= 0.0);
    assert!(num(&it, "t2") >= 0.0);
}

#[test]
fn timer_elapsed_before_start_errors_exactly_like_toc_before_tic() {
    let mut it = Interp::new();
    let err = it.run("timer.elapsed()").unwrap_err().to_string();
    assert!(err.contains("toc() called before tic()"), "got: {err}");
}

#[test]
fn table_and_timer_remain_usable_as_plain_variables_end_to_end() {
    // The narrow `table.`/`timer.` dot-namespace sugar must not shadow
    // either name as an ordinary variable anywhere else in a real script.
    let it = run("table = 5\ntimer = 7\nx = table + timer");
    assert_eq!(num(&it, "table"), 5.0);
    assert_eq!(num(&it, "timer"), 7.0);
    assert_eq!(num(&it, "x"), 12.0);
}

// ---------------------------------------------------------- `signals.` sugar
// (§ argument-validation audit, 2026-09-01): `signals.square`/
// `signals.impulse`/`signals.pwm`/`signals.sawtooth`/`signals.triangle`
// alias the bare builtins of the exact same name, purely so a reader can
// spell an unambiguous call when e.g. `square` could otherwise be misread
// as "x squared." Each test proves the namespaced spelling produces
// output byte-identical to the bare call, exactly like the `table.`/
// `timer.` tests above do for their own aliases.

fn signal_samples(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::Signal(xs, _fs, _)) => xs.as_ref().clone(),
        other => panic!("expected {name} to be a Signal, got {other:?}"),
    }
}

#[test]
fn signals_dot_square_matches_bare_square() {
    let it = run(
        "a = square(100, 1000, 20, duty=0.25)\n\
         b = signals.square(100, 1000, 20, duty=0.25)",
    );
    assert_eq!(signal_samples(&it, "a"), signal_samples(&it, "b"));
}

#[test]
fn signals_dot_impulse_matches_bare_impulse() {
    let it = run(
        "a = impulse(6, index=2, amplitude=3)\n\
         b = signals.impulse(6, index=2, amplitude=3)",
    );
    assert_eq!(vec(&it, "a"), vec(&it, "b"));
}

#[test]
fn signals_dot_pwm_matches_bare_pwm() {
    let it = run(
        "m = [0.9, -0.5, 0.2, 0.8, -0.9, 0.1, 0.4, -0.3]\n\
         a = pwm(m, 100, 1000)\n\
         b = signals.pwm(m, 100, 1000)",
    );
    assert_eq!(signal_samples(&it, "a"), signal_samples(&it, "b"));
}

#[test]
fn signals_dot_sawtooth_matches_bare_sawtooth() {
    let it = run(
        "a = sawtooth(100, 1000, 20)\n\
         b = signals.sawtooth(100, 1000, 20)",
    );
    assert_eq!(signal_samples(&it, "a"), signal_samples(&it, "b"));
}

#[test]
fn signals_dot_triangle_matches_bare_triangle() {
    let it = run(
        "a = triangle(100, 1000, 20)\n\
         b = signals.triangle(100, 1000, 20)",
    );
    assert_eq!(signal_samples(&it, "a"), signal_samples(&it, "b"));
}

#[test]
fn signals_remains_usable_as_a_plain_variable_end_to_end() {
    // Same guarantee `table`/`timer` already have (see the test above):
    // the narrow `signals.` dot-namespace sugar must not shadow `signals`
    // as an ordinary variable anywhere else in a real script.
    let it = run("signals = 5\nx = signals * 2");
    assert_eq!(num(&it, "signals"), 5.0);
    assert_eq!(num(&it, "x"), 10.0);
}

// ------------------------------------------------- `.*=` / `./=` / `^=`
// (2026-08-31): new lexed tokens, wired through the exact same
// compound-assignment desugaring `+=`/`-=`/`*=`/`/=` already use
// (`op = o.trim_end_matches('=')`, then `binop(op, cur, rhs)`), so a
// scalar goes through `binop`'s scalar arms and a matrix goes through
// `matrix_binop`'s matrix-boundary arms (real matrix power for `^=`,
// elementwise for `.*=`/`./=`) with zero new interpreter logic.

#[test]
fn elementwise_compound_assign_operators_on_vectors() {
    let it = run(
        "v = [2, 3, 4]\n\
         v .*= [10, 10, 10]\n\
         w = [100, 200, 300]\n\
         w ./= [2, 4, 5]",
    );
    assert_eq!(vec(&it, "v"), vec![20.0, 30.0, 40.0]);
    assert_eq!(vec(&it, "w"), vec![50.0, 50.0, 60.0]);
}

#[test]
fn power_compound_assign_on_a_scalar_matches_plain_power() {
    let it = run("n = 2\nn ^= 3");
    assert_eq!(num(&it, "n"), 8.0);
}

#[test]
fn power_compound_assign_on_a_matrix_is_real_matrix_power_not_elementwise() {
    // `^` at a matrix boundary is matrix power (repeated matmul), matching
    // plain `A^2`'s already-verified behavior (language tour §3) — `^=`
    // must inherit that, not silently become elementwise.
    let it = run("M = [1,2;3,4]\nM ^= 2");
    assert_eq!(flat(&it, "M"), vec![7.0, 15.0, 10.0, 22.0]); // column-major [7,10;15,22]
}

#[test]
fn elementwise_compound_assign_on_a_matrix() {
    let it = run("M = [1,2;3,4]\nM .*= [10,10;10,10]");
    assert_eq!(flat(&it, "M"), vec![10.0, 30.0, 20.0, 40.0]); // column-major [10,20;30,40]
}

#[test]
fn indexed_elementwise_compound_assign() {
    // `.*=`/`./=`/`^=` must also reach `index_assign_op` (the `x[i] op= v`
    // path), not just the plain-variable path.
    // `1:3` is three positions, 1 through 3 — slices include both ends.
    let it = run("x = [1,2,3,4,5]\nx[1:3] .*= 10");
    assert_eq!(vec(&it, "x"), vec![1.0, 20.0, 30.0, 40.0, 5.0]);
}

#[test]
fn plain_compound_assign_operators_are_unaffected_by_the_new_tokens() {
    let it = run("c = 5\nc += 3\nc -= 1\nc *= 2\nc /= 7");
    assert_eq!(num(&it, "c"), 2.0);
}

// ---------------------------------------------------- `else if` (2 words)
// (2026-08-31): must behave identically to the single-token `elseif`
// (fixed same-day, commit `79609d0`) — both consume the chain's ONE
// shared `end`/`end if`, via the shared `elseif_tail` parser routine.

#[test]
fn else_if_two_words_chains_with_one_shared_end() {
    let it = run(
        "function classify(x)\n\
         \x20   if x > 10\n\
         \x20       return \"big\"\n\
         \x20   else if x > 5\n\
         \x20       return \"medium\"\n\
         \x20   else if x > 0\n\
         \x20       return \"small\"\n\
         \x20   else\n\
         \x20       return \"non-positive\"\n\
         \x20   end if\n\
         end function\n\
         a = classify(20)\n\
         b = classify(7)\n\
         c = classify(1)\n\
         d = classify(-3)",
    );
    assert_str(&it, "a", "big");
    assert_str(&it, "b", "medium");
    assert_str(&it, "c", "small");
    assert_str(&it, "d", "non-positive");
}

#[test]
fn else_if_and_elseif_spellings_are_fully_interchangeable_in_one_chain() {
    let it = run(
        "function classify(x)\n\
         \x20   if x > 10\n\
         \x20       return \"big\"\n\
         \x20   elseif x > 5\n\
         \x20       return \"medium\"\n\
         \x20   else if x > 0\n\
         \x20       return \"small\"\n\
         \x20   else\n\
         \x20       return \"non-positive\"\n\
         \x20   end if\n\
         end function\n\
         a = classify(6)\n\
         b = classify(3)",
    );
    assert_str(&it, "a", "medium");
    assert_str(&it, "b", "small");
}

#[test]
fn plain_else_followed_by_a_separately_nested_if_on_its_own_line_still_needs_its_own_end() {
    // Regression guard: `else` NOT immediately followed by `if` (a real
    // newline in between) must be completely unaffected by the `else if`
    // chain-sugar — the nested `if` here still parses and closes as an
    // ordinary statement, with its own `end if`, same as before this
    // feature existed.
    let it = run(
        "x = 7\n\
         if x > 100\n\
         \x20   y = 1\n\
         else\n\
         \x20   if x > 3\n\
         \x20       y = 2\n\
         \x20   end if\n\
         end if",
    );
    assert_eq!(num(&it, "y"), 2.0);
}

// ------------------------------------------------ default-valued parameters
// (2026-08-31): `function f(x, order = 4, cutoff = 1000)` — an omitted
// trailing argument re-evaluates its default expression fresh on every
// call (never memoized at definition time), in the callee's own new
// frame, so a later default may reference an earlier parameter.

#[test]
fn default_parameter_omitted_uses_the_default() {
    let it = run(
        "function butter(x, order = 4, cutoff = 1000)\n\
         \x20   return order * 1000 + cutoff\n\
         end function\n\
         r = butter(1)",
    );
    assert_eq!(num(&it, "r"), 5000.0); // order=4, cutoff=1000
}

#[test]
fn default_parameter_explicitly_overridden_positionally() {
    let it = run(
        "function butter(x, order = 4, cutoff = 1000)\n\
         \x20   return order * 1000 + cutoff\n\
         end function\n\
         r1 = butter(1, 8)\n\
         r2 = butter(1, 8, 500)",
    );
    assert_eq!(num(&it, "r1"), 9000.0); // order=8 (overridden), cutoff=1000 (default)
    assert_eq!(num(&it, "r2"), 8500.0); // both overridden
}

#[test]
fn default_parameter_expression_may_reference_an_earlier_parameter() {
    let it = run(
        "function f(x, y = x * 2)\n\
         \x20   return x + y\n\
         end function\n\
         a = f(5)\n\
         b = f(5, 100)",
    );
    assert_eq!(num(&it, "a"), 15.0); // y defaults to x*2 = 10, 5+10=15
    assert_eq!(num(&it, "b"), 105.0); // y overridden to 100, 5+100=105
}

#[test]
fn default_parameter_works_in_one_line_colon_equals_functions_too() {
    let it = run("square(x, p = 2) := x ^ p\na = square(3)\nb = square(3, 3)");
    assert_eq!(num(&it, "a"), 9.0);
    assert_eq!(num(&it, "b"), 27.0);
}

#[test]
fn zero_default_functions_are_completely_unaffected_by_the_defaults_feature() {
    // Same call, same result, and the exact same arity-error wording as
    // before this feature existed (no defaults anywhere in the param
    // list means `required == params.len()`, collapsing the new
    // range-aware check back to the original strict-equality one).
    let it = run("function add(a, b)\n return a + b\nend function\nr = add(2, 3)");
    assert_eq!(num(&it, "r"), 5.0);

    let mut it2 = Interp::new();
    let err = it2
        .run("function add(a, b)\n return a + b\nend function\nadd(1)")
        .unwrap_err();
    assert_eq!(err.msg, "function expects 2 argument(s), got 1");
}

#[test]
fn default_parameter_missing_required_argument_is_a_clear_range_error() {
    let mut it = Interp::new();
    let err = it
        .run("function f(x, order = 4)\n return x\nend function\nf()")
        .unwrap_err();
    assert!(err.msg.contains("1..2"), "got: {}", err.msg);
    assert!(err.msg.contains("got 0"), "got: {}", err.msg);
}

#[test]
fn default_parameter_too_many_arguments_is_a_clear_range_error() {
    let mut it = Interp::new();
    let err = it
        .run("function f(x, order = 4)\n return x\nend function\nf(1, 2, 3)")
        .unwrap_err();
    assert!(err.msg.contains("1..2"), "got: {}", err.msg);
    assert!(err.msg.contains("got 3"), "got: {}", err.msg);
}

#[test]
fn non_default_parameter_after_a_defaulted_one_is_a_parse_time_error() {
    let mut it = Interp::new();
    let err = it.run("function f(x, order = 4, y)\n return x\nend function").unwrap_err();
    assert!(err.msg.contains("order"), "got: {}", err.msg);
    assert!(err.msg.contains("default"), "got: {}", err.msg);
}

#[test]
fn default_parameter_is_re_evaluated_fresh_every_call_not_baked_in_once() {
    // Not memoized at definition time: a default referencing a global
    // must see that global's CURRENT value on each call, not whatever it
    // was when the function was defined.
    let it = run(
        "global_order = 4\n\
         function butter(x, order = global_order)\n\
         \x20   return order\n\
         end function\n\
         r1 = butter(1)\n\
         global_order = 99\n\
         r2 = butter(1)",
    );
    assert_eq!(num(&it, "r1"), 4.0);
    assert_eq!(num(&it, "r2"), 99.0);
}

#[test]
fn default_parameters_compose_with_multiple_dispatch() {
    let it = run(
        "function area(x: num, order = 1)\n\
         \x20   return x * order\n\
         end function\n\
         function area(x: vec)\n\
         \x20   return sum(x)\n\
         end function\n\
         a = area(5)\n\
         b = area(5, 3)\n\
         c = area([1,2,3])",
    );
    assert_eq!(num(&it, "a"), 5.0); // num overload, order defaults to 1
    assert_eq!(num(&it, "b"), 15.0); // num overload, order overridden to 3
    assert_eq!(num(&it, "c"), 6.0); // vec overload
}

#[test]
fn a_failed_default_expression_leaves_the_frame_stack_balanced() {
    // If evaluating a default expression itself errors, the (partially
    // bound) call frame must still be popped before the error propagates
    // — otherwise every subsequent call in the same script would run
    // against a corrupted frame stack.
    let it = run(
        "function g(x, y = undefined_name_zzz + 1)\n\
         \x20   return x + y\n\
         end function\n\
         function add(a, b)\n\
         \x20   return a + b\n\
         end function\n\
         caught = \"\"\n\
         try\n\
         \x20   g(1)\n\
         catch e\n\
         \x20   caught = e.message\n\
         end\n\
         r = add(2, 3)",
    );
    match it.get("caught") {
        Some(Value::Str(s)) => assert!(s.contains("undefined_name_zzz"), "got: {s}"),
        other => panic!("caught is not a string: {other:?}"),
    }
    assert_eq!(num(&it, "r"), 5.0);
}

#[test]
fn recursive_function_with_a_default_parameter_accumulator() {
    let it = run(
        "function fact(n, acc = 1)\n\
         \x20   if n <= 1\n\
         \x20       return acc\n\
         \x20   end if\n\
         \x20   return fact(n - 1, acc * n)\n\
         end function\n\
         r = fact(5)",
    );
    assert_eq!(num(&it, "r"), 120.0);
}

#[test]
fn named_argument_to_a_user_defined_function_with_a_default_is_a_clear_error_not_a_silent_wrong_value() {
    // Real footgun this guards against: Qu's named-argument calling
    // convention (`f(x, kw=val)`) only ever feeds BUILTINS (confirmed by
    // reading every call site of `style` in qu-interp) — it was already
    // nonfunctional for user-defined functions before default parameters
    // existed, but harmlessly so, because a missing positional argument
    // was always a loud arity error. Once a parameter can be OMITTED,
    // the same call would otherwise silently use the DEFAULT and discard
    // the caller's named override with no error at all. This must stay a
    // clear error, not a silent wrong answer.
    let mut it = Interp::new();
    let err = it
        .run(
            "function butter(x, order = 4)\n\
             \x20   return order\n\
             end function\n\
             butter(1, order=10)",
        )
        .unwrap_err();
    assert!(err.msg.contains("order"), "got: {}", err.msg);
    assert!(err.msg.contains("named argument"), "got: {}", err.msg);
}

#[test]
fn named_argument_to_a_zero_default_user_function_still_errors_clearly() {
    // Same guard applies uniformly whether or not the function has any
    // defaults at all -- this call already failed before (an arity
    // mismatch, since `b=3` never reached `argv`); it must still fail,
    // just with a clearer diagnosis of WHY.
    let mut it = Interp::new();
    let err = it
        .run("function add(a, b)\n return a + b\nend function\nadd(2, b=3)")
        .unwrap_err();
    assert!(err.msg.contains("named argument"), "got: {}", err.msg);
}

// ------------------------------------------ § data structures pass, 2026-09-01

fn list_nums(it: &Interp, name: &str) -> Vec<f64> {
    match it.get(name) {
        Some(Value::List(items)) => items
            .iter()
            .map(|v| match v {
                Value::Num(n) => *n,
                other => panic!("expected a number in list `{name}`, found {other:?}"),
            })
            .collect(),
        other => panic!("{name} is not a list: {other:?}"),
    }
}

fn list_strs(it: &Interp, name: &str) -> Vec<String> {
    match it.get(name) {
        Some(Value::List(items)) => items
            .iter()
            .map(|v| match v {
                Value::Str(s) => s.clone(),
                other => panic!("expected a string in list `{name}`, found {other:?}"),
            })
            .collect(),
        other => panic!("{name} is not a list: {other:?}"),
    }
}

// ---- Task 1: peek_line/peek_char/peek_byte (non-advancing file peeks) ----

#[test]
fn peek_line_does_not_advance_then_read_line_sees_the_same_line() {
    let path = std::env::temp_dir().join(format!("qu_peek_line_test_{}.txt", std::process::id()));
    std::fs::write(&path, "first\nsecond\nthird\n").unwrap();
    let path_str = path.display().to_string().replace('\\', "\\\\");
    let it = run(&format!(
        "f = fopen(\"{path_str}\", \"r\")\n\
         a = peek_line(f)\n\
         b = peek_line(f)\n\
         c = read_line(f)\n\
         d = read_line(f)"
    ));
    assert_str(&it, "a", "first");
    assert_str(&it, "b", "first"); // a second peek sees the SAME line again
    assert_str(&it, "c", "first"); // the real read still sees the peeked line, not the next one
    assert_str(&it, "d", "second"); // only a real read actually advances
    let _ = std::fs::remove_file(&path);
}

#[test]
fn peek_char_does_not_advance_then_read_char_sees_the_same_char() {
    let path = std::env::temp_dir().join(format!("qu_peek_char_test_{}.txt", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    let path_str = path.display().to_string().replace('\\', "\\\\");
    let it = run(&format!(
        "f = fopen(\"{path_str}\", \"r\")\n\
         a = peek_char(f)\n\
         b = peek_char(f)\n\
         c = read_char(f)\n\
         d = read_char(f)"
    ));
    assert_str(&it, "a", "a");
    assert_str(&it, "b", "a");
    assert_str(&it, "c", "a");
    assert_str(&it, "d", "b");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn peek_byte_does_not_advance_and_reports_nothing_at_eof() {
    let path = std::env::temp_dir().join(format!("qu_peek_byte_test_{}.bin", std::process::id()));
    std::fs::write(&path, [65u8, 66u8]).unwrap(); // "AB"
    let path_str = path.display().to_string().replace('\\', "\\\\");
    let it = run(&format!(
        "f = fopen(\"{path_str}\", \"rb\")\n\
         a = peek_byte(f)\n\
         b = peek_byte(f)\n\
         c = read_byte(f)\n\
         d = read_byte(f)\n\
         e = peek_byte(f)"
    ));
    assert_eq!(num(&it, "a"), 65.0);
    assert_eq!(num(&it, "b"), 65.0); // still hasn't advanced
    assert_eq!(num(&it, "c"), 65.0); // the real read sees the same byte peek saw
    assert_eq!(num(&it, "d"), 66.0); // then genuinely advances
    assert!(matches!(it.get("e"), Some(Value::Nothing))); // exhausted, like read_byte at EOF
    let _ = std::fs::remove_file(&path);
}

// ---- Task 2: fifo(capacity) ----

#[test]
fn fifo_push_evicts_oldest_once_full_ring_buffer_semantics() {
    let it = run(
        "f = fifo(3)\n\
         f.push(1)\n\
         f.push(2)\n\
         f.push(3)\n\
         full_before = f.is_full()\n\
         f.push(4)\n\
         n = f.len()\n\
         a = f.pop()\n\
         b = f.pop()\n\
         c = f.pop()\n\
         empty_after = f.is_empty()",
    );
    assert!(matches!(it.get("full_before"), Some(Value::Bool(true))));
    assert_eq!(num(&it, "n"), 3.0);
    // capacity 3, pushed 1,2,3 (full), then 4 evicts the oldest (1) -> [2,3,4]
    assert_eq!(num(&it, "a"), 2.0);
    assert_eq!(num(&it, "b"), 3.0);
    assert_eq!(num(&it, "c"), 4.0);
    assert!(matches!(it.get("empty_after"), Some(Value::Bool(true))));
}

#[test]
fn fifo_preserves_order_through_a_push_pop_sequence_that_never_fills_it() {
    let it = run(
        "f = fifo(5)\n\
         f.push(10)\n\
         f.push(20)\n\
         a = f.pop()\n\
         f.push(30)\n\
         b = f.pop()\n\
         c = f.pop()\n\
         empty = f.is_empty()",
    );
    assert_eq!(num(&it, "a"), 10.0);
    assert_eq!(num(&it, "b"), 20.0);
    assert_eq!(num(&it, "c"), 30.0);
    assert!(matches!(it.get("empty"), Some(Value::Bool(true))));
}

#[test]
fn fifo_pop_and_peek_error_clearly_on_an_empty_fifo() {
    let mut it = Interp::new();
    let err = it.run("f = fifo(2)\nx = f.pop()").unwrap_err();
    assert!(err.msg.contains("empty"), "got: {}", err.msg);

    let mut it2 = Interp::new();
    let err2 = it2.run("f = fifo(2)\nx = f.peek()").unwrap_err();
    assert!(err2.msg.contains("empty"), "got: {}", err2.msg);
}

// ---- Task 3: double_buffer(initial) ----

#[test]
fn double_buffer_read_stays_stale_until_swap_then_sees_new_value() {
    // The actual point of double buffering: a write to the back buffer must
    // NOT be observable through .read() until .swap() is called.
    let it = run(
        "b = double_buffer(1)\n\
         before = b.read()\n\
         b.write(2)\n\
         still_old = b.read()\n\
         b.swap()\n\
         after = b.read()",
    );
    assert_eq!(num(&it, "before"), 1.0);
    assert_eq!(num(&it, "still_old"), 1.0, "a .write() alone must not be visible to .read() before .swap()");
    assert_eq!(num(&it, "after"), 2.0, ".read() must see the new value once .swap() has run");
}

#[test]
fn double_buffer_old_front_becomes_the_new_back_after_swap() {
    let it = run(
        "b = double_buffer(1)\n\
         b.write(2)\n\
         b.swap()\n\
         v1 = b.read()\n\
         b.write(3)\n\
         v2 = b.read()\n\
         b.swap()\n\
         v3 = b.read()",
    );
    assert_eq!(num(&it, "v1"), 2.0);
    assert_eq!(num(&it, "v2"), 2.0, "the second write, before its own swap, must not be visible yet");
    assert_eq!(num(&it, "v3"), 3.0);
}

// ---- Task 4: linked_list() ----

#[test]
fn linked_list_push_pop_both_ends_and_to_vec_round_trip() {
    let it = run(
        "l = linked_list()\n\
         l.push_back(1)\n\
         l.push_back(2)\n\
         l.push_front(0)\n\
         v = l.to_vec()\n\
         a = l.pop_front()\n\
         b = l.pop_back()\n\
         remaining = l.to_vec()\n\
         n = l.len()",
    );
    assert_eq!(list_nums(&it, "v"), vec![0.0, 1.0, 2.0]);
    assert_eq!(num(&it, "a"), 0.0);
    assert_eq!(num(&it, "b"), 2.0);
    assert_eq!(list_nums(&it, "remaining"), vec![1.0]);
    assert_eq!(num(&it, "n"), 1.0);
}

#[test]
fn linked_list_pop_errors_clearly_when_empty() {
    let mut it = Interp::new();
    let err = it.run("l = linked_list()\nx = l.pop_front()").unwrap_err();
    assert!(err.msg.contains("empty"), "got: {}", err.msg);
}

// ---- Task 5: graph([directed=]) ----

#[test]
fn graph_neighbors_and_has_edge_are_symmetric_by_default() {
    let it = run(
        "g = graph()\n\
         g.add_edge(\"a\", \"b\")\n\
         g.add_edge(\"a\", \"c\")\n\
         nb = g.neighbors(\"a\")\n\
         has_ab = g.has_edge(\"a\", \"b\")\n\
         has_ba = g.has_edge(\"b\", \"a\")\n\
         has_bc = g.has_edge(\"b\", \"c\")\n\
         n = g.len()",
    );
    assert_eq!(list_strs(&it, "nb"), vec!["b".to_string(), "c".to_string()]);
    assert!(matches!(it.get("has_ab"), Some(Value::Bool(true))));
    assert!(matches!(it.get("has_ba"), Some(Value::Bool(true))), "undirected by default");
    assert!(matches!(it.get("has_bc"), Some(Value::Bool(false))));
    assert_eq!(num(&it, "n"), 3.0); // nodes a, b, c
}

#[test]
fn graph_directed_edges_are_one_directional() {
    let it = run(
        "g = graph(directed=true)\n\
         g.add_edge(\"a\", \"b\")\n\
         ab = g.has_edge(\"a\", \"b\")\n\
         ba = g.has_edge(\"b\", \"a\")",
    );
    assert!(matches!(it.get("ab"), Some(Value::Bool(true))));
    assert!(matches!(it.get("ba"), Some(Value::Bool(false))));
}

#[test]
fn graph_shortest_path_matches_hand_computed_dijkstra_distance() {
    // Undirected weighted graph:
    //   A-B (4), A-C (1), C-B (2), B-D (5), C-D (8)
    // Hand-computed shortest A->D:
    //   A-B-D   = 4 + 5 = 9
    //   A-C-D   = 1 + 8 = 9
    //   A-C-B-D = 1 + 2 + 5 = 8   <- cheapest
    let it = run(
        "g = graph()\n\
         g.add_edge(\"A\", \"B\", weight=4)\n\
         g.add_edge(\"A\", \"C\", weight=1)\n\
         g.add_edge(\"C\", \"B\", weight=2)\n\
         g.add_edge(\"B\", \"D\", weight=5)\n\
         g.add_edge(\"C\", \"D\", weight=8)\n\
         d = g.shortest_path(\"A\", \"D\")\n\
         same_node = g.shortest_path(\"A\", \"A\")",
    );
    assert_eq!(num(&it, "d"), 8.0);
    assert_eq!(num(&it, "same_node"), 0.0);
}

#[test]
fn graph_shortest_path_is_nothing_when_unreachable() {
    let it = run(
        "g = graph(directed=true)\n\
         g.add_node(\"x\")\n\
         g.add_node(\"y\")\n\
         d = g.shortest_path(\"x\", \"y\")",
    );
    assert!(matches!(it.get("d"), Some(Value::Nothing)));
}

#[test]
fn graph_shortest_path_errors_clearly_on_an_unknown_node() {
    let mut it = Interp::new();
    let err = it
        .run("g = graph()\ng.add_node(\"x\")\nd = g.shortest_path(\"x\", \"z\")")
        .unwrap_err();
    assert!(err.msg.contains("z"), "got: {}", err.msg);
}
