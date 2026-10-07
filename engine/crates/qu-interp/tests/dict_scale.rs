//! `dict` semantics through the language, and the scale cases that used to
//! be unusable: `Dict` was an association list, so `get` was a linear scan
//! and every `set` copied the whole dict (1e5 inserts did not finish).
//!
//! The semantic tests matter more than the speed ones: `d = set(d, k, v)` is
//! now updated in place when `d` is uniquely held, and that must never be
//! visible through an alias or when `set` is not the builtin.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
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
fn set_get_and_key_order_behave_as_before() {
    let it = run(
        "d = dict()\nd = set(d, \"b\", 2)\nd = set(d, \"a\", 1)\nd = set(d, \"b\", 20)\n\
         k = join(keys(d), \",\")\nvb = get(d, \"b\")\nva = get(d, \"a\")\nmiss = get(d, \"zz\", -1)\nn = length(d)",
    );
    assert_eq!(text(&it, "k"), "b,a", "an update keeps the key's original position");
    assert_eq!(num(&it, "vb"), 20.0);
    assert_eq!(num(&it, "va"), 1.0);
    assert_eq!(num(&it, "miss"), -1.0);
    assert_eq!(num(&it, "n"), 2.0);
}

#[test]
fn numbers_and_equal_looking_numbers_share_one_key() {
    let it = run("d = dict()\nd = set(d, 1, \"x\")\nd = set(d, 1.0, \"y\")\nn = length(d)\nv = get(d, 1)");
    assert_eq!(num(&it, "n"), 1.0);
    assert_eq!(text(&it, "v"), "y");
}

#[test]
fn an_alias_never_sees_the_in_place_update() {
    // `e = d` shares the dict; `d = set(d, ...)` must copy it first.
    let it = run(
        "d = dict()\nd = set(d, \"a\", 1)\ne = d\nd = set(d, \"a\", 2)\nd = set(d, \"b\", 3)\n\
         ea = get(e, \"a\")\nen = length(e)\nda = get(d, \"a\")\ndn = length(d)",
    );
    assert_eq!(num(&it, "ea"), 1.0, "the alias keeps the old value");
    assert_eq!(num(&it, "en"), 1.0, "the alias did not grow");
    assert_eq!(num(&it, "da"), 2.0);
    assert_eq!(num(&it, "dn"), 2.0);
}

#[test]
fn the_non_rebinding_form_leaves_the_original_alone() {
    let it = run("d = dict()\nd = set(d, \"a\", 1)\nd2 = set(d, \"a\", 99)\nv = get(d, \"a\")\nv2 = get(d2, \"a\")");
    assert_eq!(num(&it, "v"), 1.0);
    assert_eq!(num(&it, "v2"), 99.0);
}

#[test]
fn a_function_argument_is_not_modified_by_a_set_inside_the_function() {
    let it = run(
        "function bump(d)\n  d = set(d, \"n\", 100)\n  return get(d, \"n\")\nend\n\
         base = dict()\nbase = set(base, \"n\", 1)\ninside = bump(base)\nafter = get(base, \"n\")",
    );
    assert_eq!(num(&it, "inside"), 100.0);
    assert_eq!(num(&it, "after"), 1.0, "the caller's dict is untouched");
}

#[test]
fn a_user_defined_set_is_not_bypassed() {
    let it = run("function set(d, k, v)\n  return 42\nend\nd = dict()\nd = set(d, \"a\", 1)");
    assert_eq!(num(&it, "d"), 42.0);
}

#[test]
fn the_value_expression_may_read_the_dict_it_is_updating() {
    let it = run(
        "d = dict()\nd = set(d, \"n\", 1)\nd = set(d, \"n\", get(d, \"n\") + 1)\nd = set(d, \"n\", get(d, \"n\") + 1)\nv = get(d, \"n\")",
    );
    assert_eq!(num(&it, "v"), 3.0);
}

#[test]
fn a_bad_key_is_still_an_error_and_leaves_the_dict_intact() {
    let mut it = Interp::new();
    it.run("d = dict()\nd = set(d, \"a\", 1)").unwrap();
    let err = it.run("d = set(d, [1, 2], 5)").expect_err("a vector is not a key").to_string();
    assert!(err.contains("keys must be strings or numbers"), "{err}");
    it.run("n = length(d)").unwrap();
    assert_eq!(num(&it, "n"), 1.0);
}

#[test]
fn a_record_keeps_its_type_through_set() {
    let it = run("r = {a = 1}\nr2 = set(r, \"b\", 2)\nt = type(r2)\nn = length(r2)");
    assert_eq!(text(&it, "t"), "record");
    assert_eq!(num(&it, "n"), 2.0);
}

#[test]
fn dict_of_keys_and_values_keeps_the_first_position_and_the_last_value() {
    let it = run(
        "d = dict([\"a\", \"b\", \"a\"], [1, 2, 3])\nn = length(d)\nk = join(keys(d), \",\")\nva = get(d, \"a\")",
    );
    assert_eq!(num(&it, "n"), 2.0);
    assert_eq!(text(&it, "k"), "a,b");
    assert_eq!(num(&it, "va"), 3.0);
}

#[test]
fn twenty_thousand_inserts_and_lookups_finish_quickly() {
    // 20,000 inserts used to copy ~2e8 key/value pairs; this is now linear.
    let t0 = std::time::Instant::now();
    let it = run(
        "d = dict()\nfor i = 0 to 19999\n  d = set(d, \"k\" + str(i), i)\nend\n\
         total = 0\nfor i = 0 to 19999\n  total = total + get(d, \"k\" + str(i))\nend\nn = length(d)",
    );
    assert_eq!(num(&it, "n"), 20000.0);
    assert_eq!(num(&it, "total"), 199_990_000.0);
    let secs = t0.elapsed().as_secs_f64();
    assert!(secs < 20.0, "20k inserts + 20k lookups took {secs:.1}s");
}

#[test]
fn a_big_dict_built_from_two_lists_is_not_quadratic() {
    let t0 = std::time::Instant::now();
    let it = run("ks = []\nfor i = 0 to 29999\n  ks = append(ks, i)\nend\nd = dict(ks, ks)\nn = length(d)\nv = get(d, 29999)");
    assert_eq!(num(&it, "n"), 30000.0);
    assert_eq!(num(&it, "v"), 29999.0);
    assert!(t0.elapsed().as_secs_f64() < 20.0);
}
