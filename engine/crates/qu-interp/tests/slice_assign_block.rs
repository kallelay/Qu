//! Assigning a 2-D matrix into a slice of another matrix keeps its layout.
//! `M[0:1, 0:1] = A` used to write A transposed (cells were visited row-major
//! but the source was read column-major). Expected values are written out by
//! hand, never read back from the code under test.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

/// Row-major read of a real matrix variable.
fn rows(it: &Interp, n: &str) -> Vec<Vec<f64>> {
    match it.get(n) {
        Some(Value::Mat(m)) => (0..m.rows()).map(|i| (0..m.cols()).map(|j| m.get(i, j).unwrap()).collect()).collect(),
        other => panic!("`{n}` is {other:?}"),
    }
}

#[test]
fn a_square_block_lands_untransposed() {
    let it = run("M = zeros(3, 3)\nA = [1, 2; 3, 4]\nM[0:1, 0:1] = A\n");
    assert_eq!(rows(&it, "M"), vec![vec![1.0, 2.0, 0.0], vec![3.0, 4.0, 0.0], vec![0.0, 0.0, 0.0]]);
}

#[test]
fn a_non_square_block_lands_untransposed() {
    let it = run("M = zeros(3, 4)\nA = [1, 2, 3; 4, 5, 6]\nM[1:2, 0:2] = A\n");
    assert_eq!(
        rows(&it, "M"),
        vec![vec![0.0, 0.0, 0.0, 0.0], vec![1.0, 2.0, 3.0, 0.0], vec![4.0, 5.0, 6.0, 0.0]]
    );
}

#[test]
fn rows_and_columns_still_assign_correctly() {
    let it = run("M = zeros(3, 3)\nM[1, :] = [7, 8, 9]\nM[:, 0] = [1, 2, 3]\n");
    assert_eq!(rows(&it, "M"), vec![vec![1.0, 0.0, 0.0], vec![2.0, 8.0, 9.0], vec![3.0, 0.0, 0.0]]);
}

#[test]
fn a_block_into_a_complex_matrix_lands_untransposed() {
    // real block into a complex target, then read back through real/imag
    let it = run("Z = zeros(2, 2) + 0j\nA = [1, 2; 3, 4]\nZ[0:1, 0:1] = A\nr = real(Z)\n");
    assert_eq!(rows(&it, "r"), vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
}

#[test]
fn a_compound_block_assignment_uses_the_same_layout() {
    let it = run("M = ones(2, 2)\nA = [1, 2; 3, 4]\nM[0:1, 0:1] += A\n");
    assert_eq!(rows(&it, "M"), vec![vec![2.0, 3.0], vec![4.0, 5.0]]);
}
