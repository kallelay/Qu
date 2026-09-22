//! § text surgery (2026-09-16) — positions, lines, offset-safe batched edits.

use qu_interp::{Interp, Value};

const DOC: &str = r#"doc = "alpha
beta
gamma
delta"
"#;

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src)
        .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn with_doc(tail: &str) -> Interp {
    run(&format!("{DOC}{tail}"))
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

fn err_of(src: &str) -> String {
    let mut it = Interp::new();
    format!("{}", it.run(&format!("{DOC}{src}")).unwrap_err())
}

// ---------------------------------------------------------------- lines

#[test]
fn line_numbers_are_one_based_like_the_engines_own_errors() {
    // The engine prints `parse error at 2:13` meaning the SECOND line. If
    // line_at were 0-based, copying a number out of an error message would
    // land one line off -- silently, since the wrong line is still a line.
    let it = with_doc("a = line_at(doc, 1)\nb = line_at(doc, 2)\n");
    assert_eq!(text_of(&it, "a"), "alpha");
    assert_eq!(text_of(&it, "b"), "beta");
    assert!(err_of("x = line_at(doc, 0)\n").contains("1-based"));
}

#[test]
fn line_count_and_range_are_inclusive() {
    let it = with_doc("n = line_count(doc)\nr = line_range(doc, 2, 3)\n");
    assert_eq!(num_of(&it, "n"), 4.0);
    assert_eq!(
        text_of(&it, "r"),
        "beta\ngamma",
        "`lines 2 to 3` is two lines, matching how the range reads aloud"
    );
}

#[test]
fn a_reversed_range_is_refused_rather_than_returning_nothing() {
    assert!(err_of("x = line_range(doc, 3, 2)\n").contains("after last line"));
}

#[test]
fn set_insert_and_delete_work_on_whole_lines() {
    let it = with_doc(
        "s = line_set(doc, 3, \"GAMMA\")\n\
         i = line_insert(doc, 2, \"NEW\")\n\
         a = line_insert(doc, 4, \"TAIL\", where=\"after\")\n\
         d = line_delete(doc, 2, 2)\n",
    );
    assert_eq!(text_of(&it, "s"), "alpha\nbeta\nGAMMA\ndelta");
    assert_eq!(text_of(&it, "i"), "alpha\nNEW\nbeta\ngamma\ndelta");
    assert_eq!(text_of(&it, "a"), "alpha\nbeta\ngamma\ndelta\nTAIL");
    assert_eq!(text_of(&it, "d"), "alpha\ndelta");
}

#[test]
fn a_trailing_newline_survives_a_line_edit() {
    // `str::lines` throws this away, so a round trip through it would
    // silently strip the final newline of every file it touched.
    let it = run("s = line_set(\"a\nb\n\", 1, \"A\")\nt = line_set(\"a\nb\", 1, \"A\")\n");
    assert_eq!(text_of(&it, "s"), "A\nb\n");
    assert_eq!(text_of(&it, "t"), "A\nb");
}

// ------------------------------------------------------------ positions

#[test]
fn pos_of_and_line_col_are_inverses() {
    let it = with_doc("p = pos_of(doc, 3, 1)\nlc = line_col(doc, p)\nl = lc.line\nc = lc.col\n");
    assert_eq!(num_of(&it, "p"), 11.0, "alpha\\nbeta\\n is 11 characters");
    assert_eq!(num_of(&it, "l"), 3.0);
    assert_eq!(num_of(&it, "c"), 1.0);
}

#[test]
fn positions_count_codepoints_not_bytes() {
    // "héllo" is 6 bytes and 5 characters. A byte-based position would put
    // line 2 one further along and slice mid-character.
    let it = run("s = \"héllo\nworld\"\np = pos_of(s, 2, 1)\nw = slice_at(s, p, 5)\n");
    assert_eq!(num_of(&it, "p"), 6.0);
    assert_eq!(text_of(&it, "w"), "world");
}

#[test]
fn a_span_past_the_end_is_refused_rather_than_truncated() {
    let msg = err_of("x = slice_at(doc, 20, 100)\n");
    assert!(
        msg.contains("past the end"),
        "a truncated span is a silently different edit, got: {msg}"
    );
}

// ------------------------------------------------------------- surgery

#[test]
fn splice_replaces_inserts_and_deletes() {
    let it = with_doc(
        "p = pos_of(doc, 3, 1)\n\
         r = splice(doc, p, 5, \"XXXXX\")\n\
         i = splice(doc, p, 0, \"NEW\")\n\
         d = splice(doc, p, 6, \"\")\n",
    );
    assert_eq!(text_of(&it, "r"), "alpha\nbeta\nXXXXX\ndelta");
    assert_eq!(text_of(&it, "i"), "alpha\nbeta\nNEWgamma\ndelta");
    assert_eq!(text_of(&it, "d"), "alpha\nbeta\ndelta");
}

#[test]
fn apply_edits_uses_original_coordinates_and_ignores_the_order_given() {
    // This is the whole point. Both edits are measured against the
    // ORIGINAL text; a hand-written loop over splice would shift the
    // second one by the length change of the first.
    let it = with_doc(
        "p4 = pos_of(doc, 4, 1)\n\
         fwd = apply_edits(doc, [[0, 5, \"FIRST\"], [p4, 5, \"LAST\"]])\n\
         rev = apply_edits(doc, [[p4, 5, \"LAST\"], [0, 5, \"FIRST\"]])\n",
    );
    assert_eq!(text_of(&it, "fwd"), "FIRST\nbeta\ngamma\nLAST");
    assert_eq!(
        text_of(&it, "rev"),
        text_of(&it, "fwd"),
        "the order the edits are listed in must not change the result"
    );
}

#[test]
fn apply_edits_survives_a_length_changing_edit_before_a_later_one() {
    // The replacement is much longer than what it replaces. If the second
    // edit were applied to the already-modified text, it would land in the
    // wrong place -- and still produce plausible-looking output.
    let it = with_doc(
        "p4 = pos_of(doc, 4, 1)\n\
         out = apply_edits(doc, [[0, 5, \"a-much-longer-first-line\"], [p4, 5, \"LAST\"]])\n",
    );
    assert_eq!(
        text_of(&it, "out"),
        "a-much-longer-first-line\nbeta\ngamma\nLAST"
    );
}

#[test]
fn overlapping_edits_are_refused() {
    let msg = err_of("x = apply_edits(doc, [[0, 6, \"A\"], [3, 4, \"B\"]])\n");
    assert!(
        msg.contains("overlaps"),
        "there is no order in which both mean what they said, got: {msg}"
    );
}

#[test]
fn touching_edits_are_allowed() {
    // b == a for adjacent spans is not an overlap, and refusing it would
    // make the common "replace these two adjacent tokens" case fail.
    let it = with_doc("out = apply_edits(doc, [[0, 5, \"A\"], [5, 1, \"-\"]])\n");
    assert_eq!(text_of(&it, "out"), "A-beta\ngamma\ndelta");
}

#[test]
fn two_insertions_at_the_same_point_are_refused() {
    // Both are zero-length at the same position, so their relative order is
    // exactly what the caller has not stated.
    let msg = err_of("x = apply_edits(doc, [[3, 0, \"A\"], [3, 0, \"B\"]])\n");
    assert!(msg.contains("overlaps"), "got: {msg}");
}

#[test]
fn an_empty_edit_list_returns_the_text_unchanged() {
    let it = with_doc("out = apply_edits(doc, [])\n");
    assert_eq!(text_of(&it, "out"), "alpha\nbeta\ngamma\ndelta");
}

#[test]
fn a_malformed_edit_names_which_one_is_wrong() {
    let msg = err_of("x = apply_edits(doc, [[0, 1, \"ok\"], [2]])\n");
    assert!(
        msg.contains("edit 1"),
        "the offending edit's index must be named, got: {msg}"
    );
}
