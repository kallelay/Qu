//! Scientific-notation and printf-style number formatting.
//!
//!   sci(x, [digits=3], [style="unicode"])   1.23 × 10⁻⁴⁵      (titles, labels)
//!       style "tex"      1.23 \times 10^{-45}   (LaTeX / MathJax)
//!       style "ascii"    1.23 x 10^-45
//!   sprintf(fmt, ...)    C/MATLAB conversions: %d %i %u %f %e %E %g %G %s %x %X %o %c %%
//!                        with flags `-+ 0#`, a width and a `.precision`
//!   printf(fmt, ...)     the same, printed (no newline added, like C)
//!
//! The same exponent logic backs the `{x:.3e}` / `{x:.3sci}` / `{x:.3tex}`
//! interpolation specs in `format_spec` (lib.rs), so every spelling of
//! "scientific notation" in Qu agrees digit for digit.
//!
//! Mantissa digits come from Rust's correctly rounded `{:.*e}`, so a value
//! that rounds up into the next power of ten (9.99e4 at 2 digits) carries
//! into the exponent instead of printing `10.0 x 10^4`.

use crate::{display_value, e, style_str, text_arg, Value, R};

/// `(mantissa text, exponent)` of `x` rounded to `prec` digits after the
/// point: 1.2345e-45 at 2 -> ("1.23", -45). `x` must be finite.
pub fn exp_parts(x: f64, prec: usize) -> (String, i32) {
    let s = format!("{:.*e}", prec, x);
    match s.split_once('e') {
        Some((m, ex)) => (m.to_string(), ex.parse().unwrap_or(0)),
        None => (s, 0),
    }
}

/// C's `%.{prec}e`: `1.235e+05`, `1.23e-45` -- the exponent always has a
/// sign and at least two digits.
pub fn format_e(x: f64, prec: usize, upper: bool) -> String {
    if !x.is_finite() {
        return non_finite(x);
    }
    let (m, ex) = exp_parts(x, prec);
    let body = format!("{m}e{}{:02}", if ex < 0 { '-' } else { '+' }, ex.abs());
    if upper {
        body.to_uppercase()
    } else {
        body
    }
}

fn non_finite(x: f64) -> String {
    if x.is_nan() {
        "NaN".to_string()
    } else if x > 0.0 {
        "Inf".to_string()
    } else {
        "-Inf".to_string()
    }
}

const SUPERSCRIPT: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

fn superscript(ex: i32) -> String {
    let mut s = String::new();
    if ex < 0 {
        s.push('⁻');
    }
    for d in ex.abs().to_string().chars() {
        s.push(SUPERSCRIPT[d.to_digit(10).unwrap_or(0) as usize]);
    }
    s
}

/// `1.23 × 10⁻⁴⁵` and its relatives. `prec` is digits after the point.
/// An exponent of 0 prints the bare mantissa (`1.23`, not `1.23 × 10⁰`).
pub fn format_sci(x: f64, prec: usize, style: &str) -> R<String> {
    // validate the style first, so a typo is an error for EVERY value and not
    // only for the ones that happen to have a non-zero exponent
    if !matches!(style, "unicode" | "tex" | "ascii") {
        return e(format!("sci: style must be \"unicode\", \"tex\" or \"ascii\", got \"{style}\""));
    }
    if !x.is_finite() {
        return Ok(non_finite(x));
    }
    let (m, ex) = exp_parts(x, prec);
    if ex == 0 {
        return Ok(m);
    }
    Ok(match style {
        "unicode" => format!("{m} × 10{}", superscript(ex)),
        "tex" => format!("{m} \\times 10^{{{ex}}}"),
        "ascii" => format!("{m} x 10^{ex}"),
        other => return e(format!("sci: style must be \"unicode\", \"tex\" or \"ascii\", got \"{other}\"")),
    })
}

/// `sci(x, [digits=3], [style="unicode"])`. `digits` counts significant
/// digits (3 -> `1.23 × 10⁻⁴⁵`), as a scientist reads them.
pub fn sci(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let x = match args.first() {
        Some(v) => v.as_num().map_err(|m| crate::EvalError { msg: format!("sci: {m}") })?,
        None => return e("sci(x, [digits=3], [style=\"unicode\"]) needs a number"),
    };
    let digits = match args.get(1) {
        Some(v) => v.as_num().map_err(|m| crate::EvalError { msg: format!("sci: digits: {m}") })?,
        None => crate::style_num(style, "digits").unwrap_or(3.0),
    };
    if !(1.0..=20.0).contains(&digits) || digits.fract() != 0.0 {
        return e(format!("sci: digits must be a whole number from 1 to 20, got {digits}"));
    }
    let st = style_str(style, "style").unwrap_or_else(|| "unicode".to_string());
    Ok(Value::Str(format_sci(x, digits as usize - 1, &st)?))
}

// ---------------------------------------------------------------- printf

#[derive(Default)]
struct Spec {
    minus: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alt: bool,
    width: usize,
    prec: Option<usize>,
}

fn pad(body: String, spec: &Spec, numeric: bool) -> String {
    let len = body.chars().count();
    if len >= spec.width {
        return body;
    }
    let fill = spec.width - len;
    if spec.minus {
        format!("{body}{}", " ".repeat(fill))
    } else if spec.zero && numeric {
        // zeros go between the sign and the digits
        let (sign, digits) = match body.chars().next() {
            Some(c @ ('-' | '+' | ' ')) => (c.to_string(), body[1..].to_string()),
            _ => (String::new(), body),
        };
        format!("{sign}{}{digits}", "0".repeat(fill))
    } else {
        format!("{}{body}", " ".repeat(fill))
    }
}

fn signed(mut body: String, x: f64, spec: &Spec) -> String {
    if !body.starts_with('-') && !(x.is_nan()) {
        if spec.plus {
            body.insert(0, '+');
        } else if spec.space {
            body.insert(0, ' ');
        }
    }
    body
}

fn num_arg(v: &Value, conv: char, n: usize) -> R<f64> {
    match v {
        Value::Num(x) => Ok(*x),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => e(format!("sprintf: %{conv} (argument {n}) needs a number, found {}", other.type_name())),
    }
}

/// One piece of a parsed format: literal text, or a conversion.
enum Tok {
    Lit(String),
    Conv { spec: Spec, conv: char, star_width: bool, star_prec: bool },
}

fn parse_format(fmt: &str) -> R<Vec<Tok>> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut toks: Vec<Tok> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '%' {
            lit.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            return e("sprintf: the format ends with a lone `%` (write `%%` for a percent sign)");
        }
        if chars[i] == '%' {
            lit.push('%');
            i += 1;
            continue;
        }
        let mut spec = Spec::default();
        while i < chars.len() {
            match chars[i] {
                '-' => spec.minus = true,
                '+' => spec.plus = true,
                ' ' => spec.space = true,
                '0' => spec.zero = true,
                '#' => spec.alt = true,
                _ => break,
            }
            i += 1;
        }
        let mut star_width = false;
        let mut w = String::new();
        if i < chars.len() && chars[i] == '*' {
            star_width = true;
            i += 1;
        } else {
            while i < chars.len() && chars[i].is_ascii_digit() {
                w.push(chars[i]);
                i += 1;
            }
            if !w.is_empty() {
                spec.width = w.parse().unwrap_or(0).min(10_000);
            }
        }
        let mut star_prec = false;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                star_prec = true;
                i += 1;
            } else {
                let mut p = String::new();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    p.push(chars[i]);
                    i += 1;
                }
                spec.prec = Some(p.parse().unwrap_or(0).min(1_000));
            }
        }
        // C length modifiers (`%ld`, `%lu`, `%lf`, `%hd`, `%lld`, `%zu`) say how
        // wide the C argument is; every Qu number is one f64, so they are
        // accepted and ignored.
        while i < chars.len() && matches!(chars[i], 'h' | 'l' | 'L' | 'z' | 'j' | 't') {
            i += 1;
        }
        if i >= chars.len() {
            return e("sprintf: the format ends inside a conversion");
        }
        let conv = chars[i];
        i += 1;
        if !matches!(conv, 'd' | 'i' | 'u' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G' | 'x' | 'X' | 'o' | 'c' | 's') {
            return e(format!("sprintf: unknown conversion `%{conv}` (use d i u f e E g G s c x X o or %%)"));
        }
        if !lit.is_empty() {
            toks.push(Tok::Lit(std::mem::take(&mut lit)));
        }
        toks.push(Tok::Conv { spec, conv, star_width, star_prec });
    }
    if !lit.is_empty() {
        toks.push(Tok::Lit(lit));
    }
    Ok(toks)
}

/// Every argument as a flat run of scalar values: a vector, list or matrix
/// contributes its elements in order (column-major for a matrix), so
/// `sprintf("%d,", [1 2 3])` can recycle its format, as MATLAB does.
fn flatten_args(rest: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for v in rest {
        match v {
            Value::Vec(xs) => out.extend(xs.iter().map(|&x| Value::Num(x))),
            Value::List(items) => out.extend(items.iter().cloned()),
            Value::Mat(m) => out.extend(m.as_slice().iter().map(|&x| Value::Num(x))),
            other => out.push(other.clone()),
        }
    }
    out
}

fn star_arg(v: Option<&Value>, what: &str, n: usize) -> R<i64> {
    match v {
        Some(Value::Num(x)) if x.is_finite() && x.fract() == 0.0 => Ok(*x as i64),
        Some(other) => e(format!("sprintf: `*` {what} (argument {n}) needs a whole number, found {}", other.type_name())),
        None => e(format!("sprintf: the format needs argument {n} for a `*` {what}, but there are not enough")),
    }
}

/// `sprintf(fmt, ...)`: the formatted string.
///
/// The format is applied once; if arguments are left over it is applied
/// again from the start until they run out (MATLAB's rule, so a vector
/// argument formats every element). A format with no conversions and
/// arguments to spare is an error, since recycling it could never end.
pub fn format_printf(fmt: &str, rest: &[Value]) -> R<String> {
    let toks = parse_format(fmt)?;
    let args = flatten_args(rest);
    let per_pass: usize = toks
        .iter()
        .map(|t| match t {
            Tok::Conv { star_width, star_prec, .. } => 1 + *star_width as usize + *star_prec as usize,
            Tok::Lit(_) => 0,
        })
        .sum();
    if per_pass == 0 {
        if !args.is_empty() {
            return e(format!(
                "sprintf: the format has no conversions but {} argument(s) were given",
                args.len()
            ));
        }
        return Ok(toks.iter().map(|t| if let Tok::Lit(s) = t { s.as_str() } else { "" }).collect());
    }
    if args.len() < per_pass {
        return e(format!(
            "sprintf: the format needs argument {} for its conversion, but only {} given",
            args.len() + 1,
            args.len()
        ));
    }

    let mut out = String::new();
    let mut used = 0usize;
    let mut first_pass = true;
    while used < args.len() || first_pass {
        for t in &toks {
            match t {
                Tok::Lit(s) => out.push_str(s),
                Tok::Conv { spec, conv, star_width, star_prec } => {
                    // an unfilled conversion in a recycled pass ends the output there
                    let need = 1 + *star_width as usize + *star_prec as usize;
                    if used + need > args.len() {
                        return Ok(out);
                    }
                    let mut spec = Spec {
                        minus: spec.minus,
                        plus: spec.plus,
                        space: spec.space,
                        zero: spec.zero,
                        alt: spec.alt,
                        width: spec.width,
                        prec: spec.prec,
                    };
                    if *star_width {
                        let w = star_arg(args.get(used), "width", used + 1)?;
                        used += 1;
                        if w < 0 {
                            spec.minus = true;
                        }
                        spec.width = (w.unsigned_abs() as usize).min(10_000);
                    }
                    if *star_prec {
                        let p = star_arg(args.get(used), "precision", used + 1)?;
                        used += 1;
                        spec.prec = if p < 0 { None } else { Some((p as usize).min(1_000)) };
                    }
                    let arg = &args[used];
                    used += 1;
                    out.push_str(&convert(&spec, *conv, arg, used)?);
                }
            }
        }
        first_pass = false;
    }
    Ok(out)
}

/// One conversion of one argument.
fn convert(spec: &Spec, conv: char, arg: &Value, used: usize) -> R<String> {
    let body = match conv {
        'd' | 'i' | 'u' => {
            let x = num_arg(arg, conv, used)?;
            if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e18 {
                let mut s = format!("{}", x as i64);
                if let Some(p) = spec.prec {
                    let neg = s.starts_with('-');
                    let digits = s.trim_start_matches('-').to_string();
                    let padded = format!("{digits:0>p$}");
                    s = if neg { format!("-{padded}") } else { padded };
                }
                pad(signed(s, x, spec), spec, spec.prec.is_none())
            } else {
                // MATLAB's rule: a non-integer under %d prints in the
                // shortest general form instead of truncating silently.
                pad(signed(display_value(&Value::Num(x)), x, spec), spec, true)
            }
        }
        'f' | 'F' => {
            let x = num_arg(arg, conv, used)?;
            let s = if x.is_finite() { format!("{:.*}", spec.prec.unwrap_or(6), x) } else { non_finite(x) };
            pad(signed(s, x, spec), spec, x.is_finite())
        }
        'e' | 'E' => {
            let x = num_arg(arg, conv, used)?;
            pad(signed(format_e(x, spec.prec.unwrap_or(6), conv == 'E'), x, spec), spec, x.is_finite())
        }
        'g' | 'G' => {
            let x = num_arg(arg, conv, used)?;
            let s = if x.is_finite() {
                let g = crate::format_g(x, spec.prec.unwrap_or(6));
                if conv == 'G' {
                    g.to_uppercase()
                } else {
                    g
                }
            } else {
                non_finite(x)
            };
            pad(signed(s, x, spec), spec, x.is_finite())
        }
        'x' | 'X' | 'o' => {
            let x = num_arg(arg, conv, used)?;
            if !(x.is_finite() && x.fract() == 0.0 && x >= 0.0 && x < 1.8e19) {
                return e(format!("sprintf: %{conv} (argument {used}) needs a non-negative whole number, got {x}"));
            }
            let n = x as u64;
            let mut s = match conv {
                'x' => format!("{n:x}"),
                'X' => format!("{n:X}"),
                _ => format!("{n:o}"),
            };
            if spec.alt && n != 0 {
                s = match conv {
                    'x' => format!("0x{s}"),
                    'X' => format!("0X{s}"),
                    _ => format!("0{s}"),
                };
            }
            pad(s, spec, true)
        }
        'c' => {
            let s = match arg {
                Value::Str(t) => t.chars().next().map(|c| c.to_string()).unwrap_or_default(),
                Value::Num(x) => char::from_u32(*x as u32).map(|c| c.to_string()).unwrap_or_default(),
                other => {
                    return e(format!("sprintf: %c (argument {used}) needs a character or code, found {}", other.type_name()))
                }
            };
            pad(s, spec, false)
        }
        _ => {
            // 's'
            let mut s = display_value(arg);
            if let Some(p) = spec.prec {
                s = s.chars().take(p).collect();
            }
            pad(s, spec, false)
        }
    };
    Ok(body)
}


/// `sprintf(fmt, ...)` -> string.
pub fn sprintf(args: &[Value]) -> R<Value> {
    let fmt = text_arg(args, 0)?;
    Ok(Value::Str(format_printf(&fmt, &args[1..])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, rest: &[Value]) -> String {
        format_printf(fmt, rest).unwrap()
    }

    #[test]
    fn e_format_is_c_style() {
        assert_eq!(format_e(12345.678, 3, false), "1.235e+04");
        assert_eq!(format_e(1.2345e-45, 2, false), "1.23e-45");
        assert_eq!(format_e(0.0, 2, false), "0.00e+00");
        assert_eq!(format_e(9.999e4, 2, false), "1.00e+05", "rounding carries into the exponent");
        assert_eq!(format_e(-1e100, 1, true), "-1.0E+100");
    }

    #[test]
    fn sci_styles() {
        assert_eq!(format_sci(1.2345e-45, 2, "unicode").unwrap(), "1.23 × 10⁻⁴⁵");
        assert_eq!(format_sci(1.2345e-45, 2, "tex").unwrap(), "1.23 \\times 10^{-45}");
        assert_eq!(format_sci(1.2345e-45, 2, "ascii").unwrap(), "1.23 x 10^-45");
        // `format_sci` takes digits AFTER the point: 4 of them is 5 significant
        assert_eq!(format_sci(6.02214076e23, 4, "unicode").unwrap(), "6.0221 × 10²³");
        assert_eq!(format_sci(1.5, 2, "unicode").unwrap(), "1.50", "exponent 0 prints the bare mantissa");
        assert_eq!(format_sci(9.999e4, 1, "unicode").unwrap(), "1.0 × 10⁵");
        assert!(format_sci(1.0, 1, "wat").is_err());
        assert_eq!(format_sci(f64::NAN, 2, "tex").unwrap(), "NaN");
    }

    #[test]
    fn printf_basics() {
        assert_eq!(f("%d items", &[Value::Num(3.0)]), "3 items");
        assert_eq!(f("%5.2f|", &[Value::Num(3.14159)]), " 3.14|");
        assert_eq!(f("%-8s|", &[Value::Str("ab".into())]), "ab      |");
        assert_eq!(f("%08.3f", &[Value::Num(-3.14159)]), "-003.142");
        assert_eq!(f("%+d %+d", &[Value::Num(5.0), Value::Num(-5.0)]), "+5 -5");
        assert_eq!(f("%e", &[Value::Num(12345.678)]), "1.234568e+04");
        assert_eq!(f("%g %g %g", &[Value::Num(0.0001), Value::Num(100000.0), Value::Num(1234567.0)]), "0.0001 100000 1.23457e+06");
        assert_eq!(f("%.3g", &[Value::Num(1.2345e-5)]), "1.23e-05");
        assert_eq!(f("%x %X %#x %o", &[Value::Num(255.0), Value::Num(255.0), Value::Num(255.0), Value::Num(8.0)]), "ff FF 0xff 10");
        assert_eq!(f("100%%", &[]), "100%");
        assert_eq!(f("%s=%d", &[Value::Str("n".into()), Value::Num(7.0)]), "n=7");
        assert_eq!(f("%c%c", &[Value::Str("hi".into()), Value::Num(66.0)]), "hB");
        assert_eq!(f("%.2s", &[Value::Str("abcdef".into())]), "ab");
        assert_eq!(f("%6.1f|%-6.1f|", &[Value::Num(f64::INFINITY), Value::Num(f64::NAN)]), "   Inf|NaN   |");
    }

    #[test]
    fn printf_errors_name_the_problem() {
        assert!(format_printf("%d", &[]).unwrap_err().to_string().contains("needs argument 1"));
        assert!(format_printf("no conversions", &[Value::Num(1.0)]).unwrap_err().to_string().contains("no conversions"));
        assert!(format_printf("%d", &[Value::Str("x".into())]).unwrap_err().to_string().contains("needs a number"));
        assert!(format_printf("%q", &[Value::Num(1.0)]).unwrap_err().to_string().contains("unknown conversion"));
        assert!(format_printf("50%", &[]).unwrap_err().to_string().contains("lone `%`"));
    }

    #[test]
    fn extra_arguments_recycle_the_format_like_matlab() {
        let xs = Value::Vec(std::sync::Arc::new(vec![1.0, 2.0, 3.0]));
        assert_eq!(f("%d,", &[xs.clone()]), "1,2,3,");
        assert_eq!(f("[%d] ", &[Value::Num(1.0), Value::Num(2.0)]), "[1] [2] ");
        // an unfilled conversion in a later pass ends the output there
        assert_eq!(f("%d-%d ", &[xs]), "1-2 3-");
    }

    #[test]
    fn a_star_takes_width_and_precision_from_the_arguments() {
        assert_eq!(f("%*d|", &[Value::Num(5.0), Value::Num(42.0)]), "   42|");
        assert_eq!(f("%-*d|", &[Value::Num(5.0), Value::Num(42.0)]), "42   |");
        assert_eq!(f("%*d|", &[Value::Num(-5.0), Value::Num(42.0)]), "42   |");
        assert_eq!(f("%.*f", &[Value::Num(2.0), Value::Num(3.14159)]), "3.14");
        assert_eq!(f("%*.*f|", &[Value::Num(8.0), Value::Num(3.0), Value::Num(3.14159)]), "   3.142|");
    }

    #[test]
    fn c_length_modifiers_are_accepted_and_ignored() {
        assert_eq!(f("%ld %lu %lld %hd %zu", &[Value::Num(1.0), Value::Num(2.0), Value::Num(3.0), Value::Num(4.0), Value::Num(5.0)]), "1 2 3 4 5");
        assert_eq!(f("%lf %Lf", &[Value::Num(1.5), Value::Num(2.5)]), "1.500000 2.500000");
        assert_eq!(f("%li", &[Value::Num(-7.0)]), "-7");
    }

    #[test]
    fn a_non_integer_under_percent_d_is_not_truncated() {
        assert_eq!(f("%d", &[Value::Num(2.5)]), "2.5");
    }
}
