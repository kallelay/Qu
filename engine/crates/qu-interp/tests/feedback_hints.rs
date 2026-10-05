//! Error messages and small behaviours reported in Ahmed's QU_FEEDBACK.md
//! (items 1, 3, 7, 8): each used to fail with a message that did not say
//! what to do.

use qu_interp::{Interp, Value};

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

#[test]
fn a_module_used_without_import_says_to_import_it() {
    let m = err("x = pdf.page_count(\"nothing.pdf\")");
    assert!(m.contains("import pdf"), "{m}");
}

#[test]
fn an_undefined_plain_name_is_still_just_undefined() {
    let m = err("x = definitely_not_defined_xyz + 1");
    assert!(m.contains("is not defined") && !m.contains("import"), "{m}");
}

#[test]
fn a_keyword_used_as_a_name_names_itself_as_reserved() {
    let m = err("skip = false");
    assert!(m.contains("`skip` is a reserved word"), "{m}");
}

#[test]
fn appending_a_string_to_a_number_vector_points_at_lines() {
    let m = err("o = []\no = append(o, \"a\")");
    assert!(m.contains("vectors of numbers") && m.contains("lines("), "{m}");
}

#[test]
fn get_returns_its_default_for_an_absent_key() {
    let mut it = Interp::new();
    it.run("a = get(dict(), \"k\", \"dflt\")\nb = get(dict([\"k\"], [7]), \"k\", \"dflt\")\nc = get(dict(), \"k\")")
        .unwrap();
    assert!(matches!(it.get("a"), Some(Value::Str(s)) if s.as_str() == "dflt"));
    assert!(matches!(it.get("b"), Some(Value::Num(x)) if *x == 7.0));
    assert!(matches!(it.get("c"), Some(Value::Nothing)));
}
