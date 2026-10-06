//! `diff_lines(a, b, [changed_only=])` through the language: the record shape,
//! the 1-based numbering, strings versus lists, and the argument errors.

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
        other => panic!("`{n}` is {other:?}"),
    }
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{n}` is {other:?}"),
    }
}

#[test]
fn a_changed_line_gives_a_delete_and_an_insert_with_one_based_numbers() {
    let it = run(
        "a = split(\"one|two|three\", \"|\")\nb = split(\"one|2|three\", \"|\")\nd = diff_lines(a, b)\n\
         n = length(d)\nop1 = d[1].op\nold1 = d[1].old\nop2 = d[2].op\nnew2 = d[2].new\nt2 = d[2].text",
    );
    assert_eq!(num(&it, "n"), 4.0, "equal, delete, insert, equal");
    assert_eq!(text(&it, "op1"), "delete");
    assert_eq!(num(&it, "old1"), 2.0, "line numbers are 1-based, as an editor shows them");
    assert_eq!(text(&it, "op2"), "insert");
    assert_eq!(num(&it, "new2"), 2.0);
    assert_eq!(text(&it, "t2"), "2");
}

#[test]
fn changed_only_drops_the_equal_lines() {
    let it = run(
        "a = split(\"a|b|c|d\", \"|\")\nb = split(\"a|b|X|d\", \"|\")\nd = diff_lines(a, b, changed_only=true)\nn = length(d)",
    );
    assert_eq!(num(&it, "n"), 2.0);
}

#[test]
fn two_strings_are_split_into_lines() {
    let it = run("d = diff_lines(\"x\\ny\\nz\", \"x\\nz\")\nn = length(d)\nop = d[1].op\nt = d[1].text");
    assert_eq!(num(&it, "n"), 3.0);
    assert_eq!(text(&it, "op"), "delete");
    assert_eq!(text(&it, "t"), "y");
}

#[test]
fn identical_inputs_have_no_changes() {
    let it = run("a = split(\"p|q\", \"|\")\nd = diff_lines(a, a, changed_only=true)\nn = length(d)");
    assert_eq!(num(&it, "n"), 0.0);
}

#[test]
fn the_wrong_kind_of_argument_is_an_error_that_names_the_function() {
    let m = err("d = diff_lines([1, 2, 3], [1, 2])");
    assert!(m.contains("diff_lines") && m.contains("vector"), "{m}");
    let m = err("d = diff_lines(5, \"x\")");
    assert!(m.contains("diff_lines"), "{m}");
}

#[test]
fn an_insert_has_no_old_line_and_a_delete_has_no_new_line() {
    let it = run(
        "d = diff_lines(split(\"a|b\", \"|\"), split(\"a|b|c\", \"|\"))\nlast = d[2]\nhas_old = last.old\nnew_no = last.new",
    );
    assert!(matches!(it.get("has_old"), Some(Value::Nothing)), "{:?}", it.get("has_old"));
    assert_eq!(num(&it, "new_no"), 3.0);
}
