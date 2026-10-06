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

fn num(it: &Interp, n: &str) -> f64 {
    match it.get(n) {
        Some(Value::Num(x)) => *x,
        other => panic!("`{n}` is {other:?}"),
    }
}

#[test]
fn a_list_element_can_be_assigned() {
    let mut it = Interp::new();
    it.run("a = split(\"x|y|z\", \"|\")\na[1] = \"w\"\nb = a[1]\nn = length(a)").unwrap();
    assert!(matches!(it.get("b"), Some(Value::Str(s)) if s.as_str() == "w"));
    assert_eq!(num(&it, "n"), 3.0);
}

#[test]
fn list_assignment_out_of_range_is_an_error_not_a_panic() {
    let m = err("a = split(\"x|y\", \"|\")\na[9] = \"w\"");
    assert!(m.contains("out of range") || m.contains("index"), "{m}");
}

#[test]
fn extend_concatenates_where_append_nests() {
    let mut it = Interp::new();
    it.run(
        "a = split(\"x|y\", \"|\")\nb = split(\"p|q|r\", \"|\")\nnested = length(append(a, b))\njoined = length(extend(a, b))",
    )
    .unwrap();
    assert_eq!(num(&it, "nested"), 3.0);
    assert_eq!(num(&it, "joined"), 5.0);
}

#[test]
fn a_json_array_of_objects_can_be_looped_row_by_row() {
    let mut it = Interp::new();
    it.run(
        "d = parse_json(r\"\"\"[{\"a\":1},{\"a\":2},{\"a\":5}]\"\"\")\ntotal = 0\nfor o in d\n  total = total + o.a\nend",
    )
    .unwrap();
    assert_eq!(num(&it, "total"), 8.0);
}

#[test]
fn a_triple_quoted_raw_string_keeps_quotes_braces_and_backslashes() {
    let mut it = Interp::new();
    it.run("s = r\"\"\"{\"k\": \"v\\n\"}\"\"\"\nn = length(s)").unwrap();
    // {"k": "v\n"} with the backslash-n left as two characters: 12 characters
    assert_eq!(num(&it, "n"), 12.0);
}

#[test]
fn json_objects_with_nested_values_stay_records_not_flattened_text() {
    // Audit finding: an array of objects became a table whose nested
    // `["x","y"]` field was turned into the STRING `["x","y"]`.
    let mut it = Interp::new();
    it.run(
        "d = parse_json(r\"\"\"[{\"t\":[\"x\",\"y\"],\"n\":1},{\"t\":[\"z\"],\"n\":2}]\"\"\")\nfirst = d[0]\nk = length(first.t)\nn = first.n",
    )
    .unwrap();
    assert_eq!(num(&it, "k"), 2.0, "the nested list must still be a list of two");
    assert_eq!(num(&it, "n"), 1.0);
}

#[test]
fn flat_json_rows_still_become_a_table() {
    let mut it = Interp::new();
    it.run("d = parse_json(r\"\"\"[{\"a\":1,\"b\":\"x\"},{\"a\":2,\"b\":\"y\"}]\"\"\")\nt = type(d)").unwrap();
    assert!(matches!(it.get("t"), Some(Value::Str(s)) if s.as_str() == "table"));
}

#[test]
fn the_g_format_follows_c_significant_digits_rules() {
    let mut it = Interp::new();
    it.run(
        "x = 0.000012345\na = \"{x:.3g}\"\nb = \"{x:g}\"\nc = \"{123456.789:.3g}\"\nd = \"{0.5:.3g}\"\ne2 = \"{1234.5:.6g}\"",
    )
    .unwrap();
    let s = |n: &str| match it.get(n) { Some(Value::Str(v)) => v.clone(), o => panic!("{n}: {o:?}") };
    assert_eq!(s("a"), "1.23e-05");
    assert_eq!(s("b"), "1.2345e-05");
    assert_eq!(s("c"), "1.23e+05");
    assert_eq!(s("d"), "0.5");
    assert_eq!(s("e2"), "1234.5");
}

#[test]
fn list_assignment_out_of_range_uses_the_same_zero_based_wording_as_reads() {
    let m = err("y = split(\"a|b|c\", \"|\")\ny[3] = \"q\"");
    assert!(m.contains("index 3 is outside 0..2"), "{m}");
}
