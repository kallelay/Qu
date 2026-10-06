//! `sci`, `sprintf`, `printf` and the `e`/`E`/`sci`/`tex` interpolation specs,
//! through the language.

use qu_interp::{Interp, Value};

fn text(src: &str, name: &str) -> String {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    match it.get(name) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{name}` is {other:?}"),
    }
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

#[test]
fn sci_writes_the_exponent_as_unicode_superscripts() {
    assert_eq!(text("s = sci(1.2345e-45)", "s"), "1.23 × 10⁻⁴⁵");
    assert_eq!(text("s = sci(6.02214076e23, 5)", "s"), "6.0221 × 10²³");
}

#[test]
fn sci_tex_and_ascii_styles() {
    assert_eq!(text("s = sci(1.2345e-45, 3, style=\"tex\")", "s"), "1.23 \\times 10^{-45}");
    assert_eq!(text("s = sci(1.2345e-45, 3, style=\"ascii\")", "s"), "1.23 x 10^-45");
}

#[test]
fn interpolation_specs_match_the_function() {
    assert_eq!(text("x = 1.2345e-45\ns = \"{x:.2sci}\"", "s"), "1.23 × 10⁻⁴⁵");
    assert_eq!(text("x = 1.2345e-45\ns = \"{x:.2tex}\"", "s"), "1.23 \\times 10^{-45}");
}

#[test]
fn e_format_is_c_style_with_signed_two_digit_exponent() {
    assert_eq!(text("x = 123456.789\ns = \"{x:.3e}\"", "s"), "1.235e+05");
    assert_eq!(text("x = 123456.789\ns = \"{x:.3E}\"", "s"), "1.235E+05");
    assert_eq!(text("x = 0.00000123\ns = \"{x:.1e}\"", "s"), "1.2e-06");
}

#[test]
fn sprintf_covers_the_common_conversions() {
    assert_eq!(text("s = sprintf(\"%5.2f|%-6s|%03d\", 3.14159, \"ab\", 7)", "s"), " 3.14|ab    |007");
    assert_eq!(text("s = sprintf(\"%.3g and %e\", 1.2345e-5, 12345.678)", "s"), "1.23e-05 and 1.234568e+04");
    assert_eq!(text("s = sprintf(\"%x %X %o 100%%\", 255, 255, 8)", "s"), "ff FF 10 100%");
}

#[test]
fn sprintf_argument_mismatches_are_errors_that_say_so() {
    assert!(err("s = sprintf(\"%d and %d\", 1)").contains("needs argument 2"));
    assert!(err("s = sprintf(\"plain text\", 1)").contains("no conversions"));
    assert!(err("s = sprintf(\"%d\", \"x\")").contains("needs a number"));
}

#[test]
fn printf_prints_without_adding_a_newline() {
    let mut it = Interp::new();
    it.run("printf(\"a=%d\", 1)\nprintf(\" b=%d\", 2)").unwrap();
    assert!(it.out.ends_with("a=1 b=2"), "{:?}", it.out);
}

#[test]
fn a_vector_argument_recycles_the_format_like_matlab() {
    assert_eq!(text("s = sprintf(\"%d,\", [1, 2, 3])", "s"), "1,2,3,");
    assert_eq!(text("s = sprintf(\"%5.1f|\", [1.25, 2.5])", "s"), "  1.2|  2.5|");
}

#[test]
fn star_width_and_c_length_modifiers_work_through_the_language() {
    assert_eq!(text("s = sprintf(\"%*d|%.*f|\", 4, 7, 2, 3.14159)", "s"), "   7|3.14|");
    assert_eq!(text("s = sprintf(\"%ld %lu %lf\", 1, 2, 1.5)", "s"), "1 2 1.500000");
}
