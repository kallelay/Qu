//! § text ops and image measurement in physical units (2026-09-16).

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn text_of(it: &Interp, name: &str) -> String {
    match it.get(name) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{name}` is {other:?}, expected text"),
    }
}

fn num_of(it: &Interp, name: &str) -> f64 {
    match it.get(name) {
        Some(Value::Num(n)) => *n,
        other => panic!("`{name}` is {other:?}, expected a number"),
    }
}

// ------------------------------------------------------------------ text

#[test]
fn casefold_folds_sharp_s_where_lower_does_not() {
    // The whole reason casefold exists as a separate primitive: `lower`
    // leaves ß alone, so a lowercase comparison misses STRASSE/Straße.
    let it = run(
        r#"
a = casefold("STRASSE")
b = casefold("Straße")
la = lower("Straße")
"#,
    );
    assert_eq!(text_of(&it, "a"), text_of(&it, "b"));
    assert_eq!(text_of(&it, "a"), "strasse");
    assert_ne!(
        text_of(&it, "la"),
        "strasse",
        "if lower() ever folds ß, casefold's reason for existing needs rechecking"
    );
}

#[test]
fn codepoints_and_nbytes_disagree_on_multibyte_text() {
    // A test using only ASCII would pass with the two implemented
    // identically, which is the bug this is here to catch.
    let it = run(
        r#"
n = nbytes("héllo")
l = len("héllo")
c = len(codepoints("héllo"))
"#,
    );
    assert_eq!(num_of(&it, "n"), 6.0, "UTF-8 storage counts é as two bytes");
    assert_eq!(num_of(&it, "l"), 5.0);
    assert_eq!(num_of(&it, "c"), 5.0);
}

#[test]
fn levenshtein_matches_the_textbook_example() {
    let it = run(r#"d = levenshtein("kitten", "sitting")"#);
    assert_eq!(num_of(&it, "d"), 3.0);
}

#[test]
fn levenshtein_counts_codepoints_not_bytes() {
    // "é" -> "e" is one edit, not two, even though it is a two-byte change.
    let it = run(r#"d = levenshtein("café", "cafe")"#);
    assert_eq!(num_of(&it, "d"), 1.0);
}

#[test]
fn similar_is_one_for_identical_and_rejects_an_unknown_metric() {
    let it = run(r#"s = similar("abc", "abc")"#);
    assert_eq!(num_of(&it, "s"), 1.0);

    let mut bad = Interp::new();
    let err = bad
        .run(r#"x = similar("a", "b", metric="cosine")"#)
        .unwrap_err();
    assert!(
        format!("{err}").contains("not a metric"),
        "an unknown metric must be refused, got: {err}"
    );
}

#[test]
fn wrap_breaks_on_width_and_keeps_long_words_whole() {
    let it = run(r#"w = word_wrap("the quick brown fox jumps over the lazy dog", 15)"#);
    let w = text_of(&it, "w");
    for line in w.lines() {
        assert!(
            line.chars().count() <= 15,
            "line over width: {line:?} in {w:?}"
        );
    }
    assert_eq!(w.lines().count(), 3);

    let it2 = run(r#"w = word_wrap("short supercalifragilistic tail", 10)"#);
    assert!(
        text_of(&it2, "w").contains("supercalifragilistic"),
        "a word longer than the width must stay intact, not be split"
    );
}

#[test]
fn dedent_removes_only_the_common_prefix() {
    let it = run("s = dedent(\"    a\n      b\n    c\")");
    assert_eq!(text_of(&it, "s"), "a\n  b\nc");
}

#[test]
fn indent_skips_blank_lines() {
    let it = run("s = indent(\"a\n\nb\", 2)");
    assert_eq!(
        text_of(&it, "s"),
        "  a\n\n  b",
        "a blank line must not gain trailing whitespace"
    );
}

#[test]
fn strip_ansi_removes_colour_and_leaves_the_text() {
    let it = run(r#"s = strip_ansi(chr(27) + "[1;31mred" + chr(27) + "[0m")"#);
    assert_eq!(text_of(&it, "s"), "red");
}

// -------------------------------------------------- image measurement

/// A 2x2 blob at (5,5). Values are 0/255, NOT 0/1: `image_from_matrix`
/// takes 8-bit levels, so a matrix of 1.0 is very nearly black and
/// `bwlabel` finds `count = 0` -- an empty result that prints as a
/// perfectly well-formed table with no rows. That mistake was made while
/// writing this module and is pinned here so the next reader does not
/// repeat it.
const BLOB: &str = r#"
m = zeros(20, 20)
m[5,5] = 255.0
m[5,6] = 255.0
m[6,5] = 255.0
m[6,6] = 255.0
lab = bwlabel(image_from_matrix(m))
"#;

#[test]
fn the_fixture_actually_contains_a_blob() {
    // Guards every test below: if labelling ever stops finding this blob,
    // the others would still pass against an empty table.
    let it = run(&format!("{BLOB}\nr = image_regions(lab)\nn = nrow(r)\n"));
    assert_eq!(num_of(&it, "n"), 1.0, "the fixture must label exactly one blob");
}

#[test]
fn regions_returns_a_table_not_a_list() {
    let it = run(&format!("{BLOB}\nr = image_regions(lab)\n"));
    assert!(
        matches!(it.get("r"), Some(Value::Table(_))),
        "regions must return a Table so the language's own verbs apply"
    );
}

#[test]
fn regions_in_pixels_names_its_columns_px() {
    let it = run(&format!(
        "{BLOB}\nr = image_regions(lab)\na = r.area_px2[0]\nw = r.bbox_width_px[0]\n"
    ));
    assert_eq!(num_of(&it, "a"), 4.0);
    assert_eq!(num_of(&it, "w"), 2.0);
}

#[test]
fn regions_scales_area_by_the_square_of_the_pixel_size() {
    // The failure this catches is scaling area linearly: that would give
    // 2.6, not 1.69, and would look entirely reasonable.
    let it = run(&format!(
        "{BLOB}\nr = image_regions(lab, pixel_size=0.65, unit=\"um\")\n\
         a = r.area_um2[0]\nc = r.centroid_x_um[0]\ne = r.extent[0]\n"
    ));
    assert!((num_of(&it, "a") - 4.0 * 0.65 * 0.65).abs() < 1e-12);
    assert!((num_of(&it, "c") - 5.5 * 0.65).abs() < 1e-12);
    assert_eq!(
        num_of(&it, "e"),
        1.0,
        "extent is a ratio, so the pixel size must cancel rather than scale it"
    );
}

#[test]
fn a_unit_without_a_scale_is_refused() {
    let mut it = Interp::new();
    let err = it
        .run(&format!("{BLOB}\nr = image_regions(lab, unit=\"um\")\n"))
        .unwrap_err();
    assert!(
        format!("{err}").contains("without `pixel_size=`"),
        "labelling pixel counts with a unit name must be refused, got: {err}"
    );
}

#[test]
fn a_non_positive_pixel_size_is_refused() {
    let mut it = Interp::new();
    let err = it
        .run(&format!("{BLOB}\nr = image_regions(lab, pixel_size=0)\n"))
        .unwrap_err();
    assert!(format!("{err}").contains("positive"), "got: {err}");
}
