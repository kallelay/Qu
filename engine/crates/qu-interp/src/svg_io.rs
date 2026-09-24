//! Basic SVG vector I/O: `save_svg(path, shapes)` / `load_svg(path)` plus
//! the `svg.*` shape constructors that feed them.
//!
//! # What this is NOT
//!
//! `docs/design/toolkit-image.md` §7 specifies a whole vector-graphics
//! domain — a `Vector` scene graph with groups, symbols, gradients, masks,
//! clip paths, path-node editing, boolean geometry (`union`/`intersect`/
//! `subtract`), per-object transforms, text-on-a-path and raster<->vector
//! conversion. **None of that is here.** This module is the basic-I/O
//! slice only: make a few primitive shapes, write them out as valid SVG,
//! read simple ones back. The rest of §7 is separate, much larger work and
//! this module deliberately does not pretend to start it — no `Vector`
//! value type is introduced, because introducing one badly is worse than
//! not introducing one.
//!
//! # Not to be confused with the plotting primitives
//!
//! `arc`, `arrow`, `box`, `circle`, `rectangle`, `text`, `polygon` already
//! exist in `BUILTIN_NAMES`. Those are `plotting.rs` annotation/drawing
//! commands that decorate the *current figure* — they are not vector
//! objects and have nothing to do with this module. That name clash is
//! exactly why every constructor here lives behind the `svg.` namespace
//! (`svg.rect`, `svg.circle`, ...) and never as a bare top-level builtin:
//! `svg.circle(...)` and `circle(...)` are different functions in different
//! domains, and neither shadows the other.
//!
//! # Not to be confused with `savefig(".svg")` either
//!
//! Qu's plotting already renders figures to SVG — `plotting::render_svg`,
//! reached by `savefig("f.svg")`. That path was already complete before
//! this module and is untouched by it. The gap this fills is the *other*
//! direction: composing an SVG from explicit shapes rather than from a
//! figure. If you want a chart as SVG, `savefig` is still the answer.
//!
//! # Dependencies: none added
//!
//! Writing SVG is string assembly (`plotting.rs` has always done it by
//! hand), so no `svg` crate is warranted. Reading goes through
//! `quick-xml`, which is ALREADY an always-on dependency of this crate for
//! `xml_ops.rs` — so `load_svg` costs nothing new either. `usvg`/`resvg`
//! are the standard choice for *rendering* arbitrary third-party SVG, and
//! they would be the right call for that, but they pull in a font
//! database, `tiny-skia` and a full CSS/transform resolver to do a job
//! this module does not do (see `load_svg`'s own limits below). Per
//! `IMPL.md` §7 that weight is not justified for reading back simple
//! shapes, so it is not taken on.

use std::fmt::Write as _;
use std::sync::Arc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

use crate::{display_value, e, style_entry, EvalError, Value, R};

// ---------------------------------------------------------------------
// The shape record
// ---------------------------------------------------------------------

/// Every shape is a plain `Value::Record`, not a new `Value` variant.
///
/// Deliberate: a record is already inspectable from Qu (`s.kind`, `s.x`),
/// already printable, already comparable, already serializable, and adds
/// nothing to the 283 catch-all `_ =>` arms a new variant would have to
/// meet. It also makes `load_svg`'s round trip checkable *in Qu* rather
/// than only in Rust — `load_svg(p)[0].width` is a thing a script can
/// assert on.
///
/// Every shape carries the same four styling fields, always present (never
/// omitted when defaulted), so a round trip is exact and a consumer never
/// has to test whether a key exists.
struct Shape {
    kind: &'static str,
    /// Geometry, in the order the SVG attribute list wants them.
    geom: Vec<(&'static str, Value)>,
    fill: String,
    stroke: String,
    stroke_width: f64,
    opacity: f64,
}

impl Shape {
    fn into_record(self) -> Value {
        let mut fields: Vec<(String, Value)> = Vec::with_capacity(self.geom.len() + 5);
        fields.push(("kind".to_string(), Value::Str(self.kind.to_string())));
        for (k, v) in self.geom {
            fields.push((k.to_string(), v));
        }
        fields.push(("fill".to_string(), Value::Str(self.fill)));
        fields.push(("stroke".to_string(), Value::Str(self.stroke)));
        fields.push(("stroke_width".to_string(), Value::Num(self.stroke_width)));
        fields.push(("opacity".to_string(), Value::Num(self.opacity)));
        Value::Record(Arc::new(fields))
    }
}

/// Reads the four styling kwargs shared by every constructor.
///
/// `fill`/`stroke` defaults differ per shape and are passed in: a filled
/// black `rect` is the useful default, but a black-*filled* `line` is
/// meaningless (a line has no interior), so `svg.line` asks for the
/// stroked default instead. Getting this backwards produces a file that
/// is valid SVG and renders as nothing, which is the worst failure mode
/// available here.
fn read_style(style: &[(String, Value)], def_fill: &str, def_stroke: &str) -> R<(String, String, f64, f64)> {
    let fill = match style_entry(style, "fill") {
        Some((_, v)) => value_to_paint(v, "fill")?,
        None => def_fill.to_string(),
    };
    let stroke = match style_entry(style, "stroke") {
        Some((_, v)) => value_to_paint(v, "stroke")?,
        None => def_stroke.to_string(),
    };
    let stroke_width = match style_entry(style, "stroke_width") {
        Some((_, v)) => num_of(v, "stroke_width")?,
        None => 1.0,
    };
    let opacity = match style_entry(style, "opacity") {
        Some((_, v)) => num_of(v, "opacity")?,
        None => 1.0,
    };
    if stroke_width < 0.0 {
        return e(format!("stroke_width must be 0 or more, got {stroke_width}"));
    }
    if !(0.0..=1.0).contains(&opacity) {
        return e(format!("opacity must be between 0 and 1, got {opacity}"));
    }
    Ok((fill, stroke, stroke_width, opacity))
}

/// A paint is a color string (`"#e74c3c"`, `"red"`) or the literal
/// `"none"`. Checked here rather than passed through, because an unquoted
/// number sliding into `fill=` writes `fill="3"`, which every renderer
/// treats as an invalid paint and silently falls back to black.
fn value_to_paint(v: &Value, key: &str) -> R<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => e(format!(
            "{key}= needs a color string like \"#e74c3c\", \"red\" or \"none\", found {}",
            other.type_name()
        )),
    }
}

fn num_of(v: &Value, what: &str) -> R<f64> {
    v.as_num().map_err(|msg| EvalError { msg: format!("{what}: {msg}") })
}

/// One positional number, with the builtin's name in the error.
fn num_arg(args: &[Value], idx: usize, func: &str, what: &str) -> R<f64> {
    match crate::arg_get(args, idx) {
        Some(v) => {
            let n = num_of(v, what)?;
            if !n.is_finite() {
                return e(format!("{func}: `{what}` must be a finite number, got {n}"));
            }
            Ok(n)
        }
        None => e(format!("{func}: missing `{what}` (argument {})", idx + 1)),
    }
}

// ---------------------------------------------------------------------
// Constructors — `svg.rect` / `svg.circle` / `svg.line` / `svg.path` /
// `svg.text`
// ---------------------------------------------------------------------

/// Dispatch for every `svg::*` name. Mirrors `xlsx_call`/`codec_call`.
pub fn svg_call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "svg::rect" => {
            let x = num_arg(args, 0, "svg.rect", "x")?;
            let y = num_arg(args, 1, "svg.rect", "y")?;
            let w = num_arg(args, 2, "svg.rect", "width")?;
            let h = num_arg(args, 3, "svg.rect", "height")?;
            if w < 0.0 || h < 0.0 {
                return e(format!("svg.rect: width and height must be 0 or more, got {w} and {h}"));
            }
            let (fill, stroke, sw, op) = read_style(style, "#000000", "none")?;
            Ok(Shape {
                kind: "rect",
                geom: vec![
                    ("x", Value::Num(x)),
                    ("y", Value::Num(y)),
                    ("width", Value::Num(w)),
                    ("height", Value::Num(h)),
                ],
                fill,
                stroke,
                stroke_width: sw,
                opacity: op,
            }
            .into_record())
        }
        "svg::circle" => {
            let cx = num_arg(args, 0, "svg.circle", "cx")?;
            let cy = num_arg(args, 1, "svg.circle", "cy")?;
            let r = num_arg(args, 2, "svg.circle", "r")?;
            if r < 0.0 {
                return e(format!("svg.circle: radius must be 0 or more, got {r}"));
            }
            let (fill, stroke, sw, op) = read_style(style, "#000000", "none")?;
            Ok(Shape {
                kind: "circle",
                geom: vec![("cx", Value::Num(cx)), ("cy", Value::Num(cy)), ("r", Value::Num(r))],
                fill,
                stroke,
                stroke_width: sw,
                opacity: op,
            }
            .into_record())
        }
        "svg::line" => {
            let x1 = num_arg(args, 0, "svg.line", "x1")?;
            let y1 = num_arg(args, 1, "svg.line", "y1")?;
            let x2 = num_arg(args, 2, "svg.line", "x2")?;
            let y2 = num_arg(args, 3, "svg.line", "y2")?;
            // A line has no interior: `fill` defaults to none and `stroke`
            // to black, the opposite of the area shapes above.
            let (fill, stroke, sw, op) = read_style(style, "none", "#000000")?;
            Ok(Shape {
                kind: "line",
                geom: vec![
                    ("x1", Value::Num(x1)),
                    ("y1", Value::Num(y1)),
                    ("x2", Value::Num(x2)),
                    ("y2", Value::Num(y2)),
                ],
                fill,
                stroke,
                stroke_width: sw,
                opacity: op,
            }
            .into_record())
        }
        "svg::path" => {
            let d = match crate::arg_get(args, 0) {
                Some(Value::Str(s)) => s.clone(),
                Some(other) => {
                    return e(format!(
                        "svg.path: needs a path-data string like \"M 0 0 L 10 10 Z\", found {}",
                        other.type_name()
                    ))
                }
                None => return e("svg.path: missing the path-data string, e.g. \"M 0 0 L 10 10 Z\""),
            };
            // Validated, not passed through: an unchecked `d` writes
            // whatever it was handed straight into the file, and a
            // malformed one produces a document that parses as XML, opens
            // without complaint, and draws nothing.
            let normalized = validate_path_data(&d)?;
            let (fill, stroke, sw, op) = read_style(style, "none", "#000000")?;
            Ok(Shape {
                kind: "path",
                geom: vec![("d", Value::Str(normalized))],
                fill,
                stroke,
                stroke_width: sw,
                opacity: op,
            }
            .into_record())
        }
        "svg::text" => {
            let x = num_arg(args, 0, "svg.text", "x")?;
            let y = num_arg(args, 1, "svg.text", "y")?;
            let body = match crate::arg_get(args, 2) {
                Some(v) => display_value(v),
                None => return e("svg.text: missing the text to draw (argument 3)"),
            };
            let font_size = match style_entry(style, "font_size") {
                Some((_, v)) => num_of(v, "font_size")?,
                None => 16.0,
            };
            if font_size <= 0.0 {
                return e(format!("svg.text: font_size must be more than 0, got {font_size}"));
            }
            let font_family = match style_entry(style, "font_family") {
                Some((_, v)) => value_to_paint(v, "font_family")?,
                None => "sans-serif".to_string(),
            };
            // Text is painted by its FILL, not its stroke — an outlined,
            // unfilled glyph is the rare case, so the defaults follow the
            // area shapes rather than `svg.line`.
            let (fill, stroke, sw, op) = read_style(style, "#000000", "none")?;
            Ok(Shape {
                kind: "text",
                geom: vec![
                    ("x", Value::Num(x)),
                    ("y", Value::Num(y)),
                    ("text", Value::Str(body)),
                    ("font_size", Value::Num(font_size)),
                    ("font_family", Value::Str(font_family)),
                ],
                fill,
                stroke,
                stroke_width: sw,
                opacity: op,
            }
            .into_record())
        }
        other => e(format!("unknown svg function `{}`", other.trim_start_matches("svg::"))),
    }
}

// ---------------------------------------------------------------------
// Path data
// ---------------------------------------------------------------------

/// Parses and re-emits the supported subset of SVG path syntax.
///
/// Supported: `M`/`m` moveto, `L`/`l` lineto, `H`/`h` and `V`/`v` the axis
/// forms, `C`/`c` cubic and `Q`/`q` quadratic Bezier, `Z`/`z` closepath —
/// in both absolute (uppercase) and relative (lowercase) spelling, with
/// the SVG repetition rule (a command's parameters may repeat, and a
/// repeated `M`'s extra pairs are implicit `L`s).
///
/// Not supported: `A` arcs and the `S`/`T` smooth-curve shorthands. Those
/// are REFUSED with a named error rather than passed through, because
/// passing an unhandled command through would let `load_svg` read back a
/// path this module cannot account for while reporting success.
///
/// Returns the path normalized to single-space separation, which is what
/// makes a `save_svg` -> `load_svg` -> `save_svg` cycle byte-stable.
fn validate_path_data(d: &str) -> R<String> {
    let mut out = String::new();
    let mut toks = PathLexer { s: d.as_bytes(), i: 0 };
    let mut cmd: Option<char> = None;
    let mut seen_move = false;
    let mut count_in_run = 0usize;
    loop {
        toks.skip_sep();
        if toks.done() {
            break;
        }
        let c = toks.peek_char();
        if c.is_ascii_alphabetic() {
            toks.i += 1;
            cmd = Some(c);
            count_in_run = 0;
        }
        let Some(active) = cmd else {
            return e(format!(
                "svg.path: path data must begin with a command letter (M/L/H/V/C/Q/Z), found `{c}` in `{d}`"
            ));
        };
        let upper = active.to_ascii_uppercase();
        if upper != 'M' && !seen_move {
            return e(format!(
                "svg.path: path data must start with a moveto (`M` or `m`), found `{active}` first in `{d}`"
            ));
        }
        let arity = match upper {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'Q' => 4,
            'Z' => 0,
            'A' | 'S' | 'T' => {
                return e(format!(
                    "svg.path: `{active}` is not supported by this basic SVG writer \
                     (arcs and the smooth-curve shorthands S/T are not implemented) -- \
                     supported commands are M L H V C Q Z, in either case, in `{d}`"
                ))
            }
            _ => return e(format!("svg.path: `{active}` is not an SVG path command, in `{d}`")),
        };
        if upper == 'M' {
            seen_move = true;
        }
        if arity == 0 {
            if count_in_run > 0 {
                return e(format!("svg.path: `{active}` takes no numbers, in `{d}`"));
            }
            let _ = write!(out, "{}{}", if out.is_empty() { "" } else { " " }, active);
            count_in_run += 1;
            // A closepath never repeats, so the next number would be a
            // bare coordinate with no command — force an explicit one.
            cmd = None;
            continue;
        }
        let mut nums = Vec::with_capacity(arity);
        for k in 0..arity {
            toks.skip_sep();
            match toks.number() {
                Some(n) => nums.push(n),
                None => {
                    return e(format!(
                        "svg.path: `{active}` needs {arity} number(s) but got {k} before the data ran out or hit a letter, in `{d}`"
                    ))
                }
            }
        }
        // The repetition rule: `M 0 0 1 1` is a moveto then an implicit
        // lineto, so the SECOND and later runs of an `M` are emitted as
        // `L`/`l`. Every other command simply repeats itself.
        let emit = if upper == 'M' && count_in_run > 0 {
            if active.is_ascii_uppercase() {
                'L'
            } else {
                'l'
            }
        } else {
            active
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push(emit);
        for n in nums {
            let _ = write!(out, " {}", fmt_coord(n));
        }
        count_in_run += 1;
    }
    if out.is_empty() {
        return e("svg.path: the path data is empty");
    }
    Ok(out)
}

struct PathLexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl PathLexer<'_> {
    fn done(&self) -> bool {
        self.i >= self.s.len()
    }
    fn peek_char(&self) -> char {
        self.s[self.i] as char
    }
    fn skip_sep(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }
    /// One SVG number: optional sign, digits, optional fraction, optional
    /// exponent. Returns `None` (without consuming) at a letter or EOF.
    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
            self.i += 1;
        }
        let mut digits = false;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
            digits = true;
        }
        if self.i < self.s.len() && self.s[self.i] == b'.' {
            self.i += 1;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
                digits = true;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
            let save = self.i;
            self.i += 1;
            if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
                self.i += 1;
            }
            let mut exp_digits = false;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
                exp_digits = true;
            }
            if !exp_digits {
                self.i = save;
            }
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse::<f64>().ok())
    }
}

/// Numbers in an SVG attribute, without Rust's `1` -> `1` / `1.0` split.
///
/// `{}` on an `f64` writes `10` for `10.0`, which is what we want, but
/// writes `1e-7` for very small values and `NaN`/`inf` for non-finite
/// ones. Non-finite never gets here (every entry point rejects it), and
/// exponent notation is legal in an SVG number, so `{}` is correct — this
/// wrapper exists to keep that reasoning in one place.
fn fmt_coord(n: f64) -> String {
    format!("{n}")
}

// ---------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------

/// The five predefined XML entities. Text content and every attribute
/// value goes through this — a `&` or a `<` in a label is ordinary in
/// real data and produces a file that is not XML at all if written raw.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn field<'a>(rec: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    rec.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn req_num(rec: &[(String, Value)], key: &str, kind: &str) -> R<f64> {
    match field(rec, key) {
        Some(v) => num_of(v, key),
        None => e(format!("save_svg: a `{kind}` shape is missing its `{key}` field")),
    }
}

fn req_str(rec: &[(String, Value)], key: &str, kind: &str) -> R<String> {
    match field(rec, key) {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(other) => e(format!(
            "save_svg: a `{kind}` shape's `{key}` should be text, found {}",
            other.type_name()
        )),
        None => e(format!("save_svg: a `{kind}` shape is missing its `{key}` field")),
    }
}

/// Renders one shape record to an SVG element.
fn shape_to_xml(out: &mut String, v: &Value) -> R<()> {
    let rec: &[(String, Value)] = match v {
        Value::Record(r) => r.as_slice(),
        Value::Dict(d) => d.as_slice(),
        other => {
            return e(format!(
                "save_svg: every shape must be one built by `svg.rect`/`svg.circle`/`svg.line`/`svg.path`/`svg.text`, found {}",
                other.type_name()
            ))
        }
    };
    let kind = match field(rec, "kind") {
        Some(Value::Str(s)) => s.clone(),
        _ => {
            return e(
                "save_svg: a shape has no `kind` field -- build shapes with `svg.rect(...)`, `svg.circle(...)`, \
                 `svg.line(...)`, `svg.path(...)` or `svg.text(...)`",
            )
        }
    };
    // Styling is optional on a hand-built record, so it defaults here
    // rather than erroring: a record with only geometry is a reasonable
    // thing for a script to assemble.
    let fill = match field(rec, "fill") {
        Some(Value::Str(s)) => s.clone(),
        _ => if kind == "line" || kind == "path" { "none".into() } else { "#000000".into() },
    };
    let stroke = match field(rec, "stroke") {
        Some(Value::Str(s)) => s.clone(),
        _ => if kind == "line" || kind == "path" { "#000000".into() } else { "none".into() },
    };
    let sw = match field(rec, "stroke_width") {
        Some(v) => num_of(v, "stroke_width")?,
        None => 1.0,
    };
    let op = match field(rec, "opacity") {
        Some(v) => num_of(v, "opacity")?,
        None => 1.0,
    };

    let mut style_attrs = String::new();
    let _ = write!(style_attrs, " fill=\"{}\"", esc(&fill));
    let _ = write!(style_attrs, " stroke=\"{}\"", esc(&stroke));
    // `stroke-width` is only meaningful with a stroke, but writing it
    // unconditionally keeps the round trip exact and costs nothing: a
    // renderer ignores it when `stroke="none"`.
    let _ = write!(style_attrs, " stroke-width=\"{}\"", fmt_coord(sw));
    if op != 1.0 {
        let _ = write!(style_attrs, " opacity=\"{}\"", fmt_coord(op));
    }

    match kind.as_str() {
        "rect" => {
            let _ = write!(
                out,
                "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{style_attrs} />\n",
                fmt_coord(req_num(rec, "x", "rect")?),
                fmt_coord(req_num(rec, "y", "rect")?),
                fmt_coord(req_num(rec, "width", "rect")?),
                fmt_coord(req_num(rec, "height", "rect")?),
            );
        }
        "circle" => {
            let _ = write!(
                out,
                "  <circle cx=\"{}\" cy=\"{}\" r=\"{}\"{style_attrs} />\n",
                fmt_coord(req_num(rec, "cx", "circle")?),
                fmt_coord(req_num(rec, "cy", "circle")?),
                fmt_coord(req_num(rec, "r", "circle")?),
            );
        }
        "line" => {
            let _ = write!(
                out,
                "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{style_attrs} />\n",
                fmt_coord(req_num(rec, "x1", "line")?),
                fmt_coord(req_num(rec, "y1", "line")?),
                fmt_coord(req_num(rec, "x2", "line")?),
                fmt_coord(req_num(rec, "y2", "line")?),
            );
        }
        "path" => {
            let d = req_str(rec, "d", "path")?;
            let d = validate_path_data(&d)?;
            let _ = write!(out, "  <path d=\"{}\"{style_attrs} />\n", esc(&d));
        }
        "text" => {
            let body = req_str(rec, "text", "text")?;
            let fs = match field(rec, "font_size") {
                Some(v) => num_of(v, "font_size")?,
                None => 16.0,
            };
            let ff = match field(rec, "font_family") {
                Some(Value::Str(s)) => s.clone(),
                _ => "sans-serif".to_string(),
            };
            let _ = write!(
                out,
                "  <text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"{}\"{style_attrs}>{}</text>\n",
                fmt_coord(req_num(rec, "x", "text")?),
                fmt_coord(req_num(rec, "y", "text")?),
                fmt_coord(fs),
                esc(&ff),
                esc(&body),
            );
        }
        other => {
            return e(format!(
                "save_svg: `{other}` is not a shape this writer knows -- \
                 it writes rect, circle, line, path and text"
            ))
        }
    }
    Ok(())
}

/// The SVG document for a list of shapes.
pub fn render(shapes: &[Value], width: f64, height: f64) -> R<String> {
    let mut body = String::new();
    for s in shapes {
        shape_to_xml(&mut body, s)?;
    }
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // Both the namespace and an explicit width/height alongside the
    // viewBox: a standalone `.svg` file opened directly in a browser needs
    // the xmlns to be treated as SVG at all, and a viewBox without
    // width/height renders at the container's size rather than its own.
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        fmt_coord(width),
        fmt_coord(height),
        fmt_coord(width),
        fmt_coord(height)
    );
    out.push_str(&body);
    out.push_str("</svg>\n");
    Ok(out)
}

/// `save_svg(path, shapes, [width=800], [height=600])`.
///
/// The canvas is NOT auto-fitted to the shapes. Deliberate: a text run's
/// rendered width depends on a font this module does not load, so an
/// "auto" canvas would be right for geometry and wrong for labels, and
/// wrong in a way that shows up as silently clipped text. Explicit
/// numbers with a documented default are honest; a guess is not.
pub fn save_svg(path: &str, shapes_arg: &Value, style: &[(String, Value)]) -> R<Value> {
    let shapes: Vec<Value> = match shapes_arg {
        Value::List(l) => l.as_ref().clone(),
        // One shape without a list around it is the obvious call to want
        // to make, so it works rather than erroring on a technicality.
        v @ (Value::Record(_) | Value::Dict(_)) => vec![v.clone()],
        other => {
            return e(format!(
                "save_svg(path, shapes): needs a list of shapes from `svg.rect`/`svg.circle`/... , found {}",
                other.type_name()
            ))
        }
    };
    let width = match style_entry(style, "width") {
        Some((_, v)) => num_of(v, "width")?,
        None => 800.0,
    };
    let height = match style_entry(style, "height") {
        Some((_, v)) => num_of(v, "height")?,
        None => 600.0,
    };
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return e(format!("save_svg: width and height must be positive finite numbers, got {width} and {height}"));
    }
    let doc = render(&shapes, width, height)?;
    std::fs::write(path, doc.as_bytes())
        .map_err(|err| EvalError { msg: format!("save_svg: could not write `{path}`: {err}") })?;
    Ok(Value::Nothing)
}

// ---------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------

/// `load_svg(path)` -> `{ width, height, shapes }`.
///
/// # Honest limits
///
/// This reads PRESENTATION ATTRIBUTES off the five element types this
/// module writes, descending into `<g>` groups. It is not a general SVG
/// loader and does not pretend to be one. Specifically it does NOT:
///
/// - apply `transform=` on an element or an ancestor group;
/// - resolve CSS (`<style>` blocks, `style="..."`, `class=`), so a file
///   styling its shapes that way comes back with the default paints;
/// - inherit `fill`/`stroke` from an ancestor `<g>`;
/// - resolve `<use>`, `<symbol>`, gradients, patterns, masks or clip
///   paths;
/// - handle `<ellipse>`, `<polygon>`, `<polyline>`, `<image>` or arcs.
///
/// Unrecognized elements are SKIPPED rather than rejected, so a file from
/// another tool loads partially rather than not at all — but "loaded
/// without error" therefore does NOT mean "read everything in the file".
/// A round trip of a file THIS module wrote is exact; that is the case it
/// is built for, and `usvg`/`resvg` is the right answer for the rest.
pub fn load_svg(path: &str) -> R<Value> {
    let src = std::fs::read_to_string(path)
        .map_err(|err| EvalError { msg: format!("load_svg: could not read `{path}`: {err}") })?;
    parse(&src).map_err(|err| EvalError { msg: format!("load_svg: `{path}`: {}", err.msg) })
}

/// Split out from `load_svg` so the tests can drive it from a string.
pub fn parse(src: &str) -> R<Value> {
    let mut reader = Reader::from_str(src);
    let mut shapes: Vec<Value> = Vec::new();
    let mut width = 0.0f64;
    let mut height = 0.0f64;
    let mut seen_root = false;
    // `<text>`'s content arrives as a separate event, so the element
    // being filled is held here between its Start and End.
    let mut pending_text: Option<(Vec<(String, String)>, String)> = None;

    loop {
        let ev = reader.read_event();
        // Start and Empty carry the same element, but ONLY Start is
        // followed by an End. That difference matters for `<text>`, which
        // is the one element whose content arrives separately: a
        // self-closing `<text/>` handled as a Start would arm
        // `pending_text` and never disarm it, silently dropping that
        // element AND leaking its attributes onto the next `<text>`. So
        // the two events are told apart here rather than merged.
        let self_closing = matches!(ev, Ok(Event::Empty(_)));
        match ev {
            Ok(Event::Start(el)) | Ok(Event::Empty(el)) => {
                let tag = local_name(el.name());
                let attrs = attrs_of(&el)?;
                match tag.as_str() {
                    "svg" => {
                        seen_root = true;
                        // `width`/`height` may carry a unit (`100px`) or be
                        // absent entirely, in which case the viewBox is the
                        // only size the file states.
                        width = attrs
                            .iter()
                            .find(|(k, _)| k == "width")
                            .and_then(|(_, v)| parse_len(v))
                            .unwrap_or(0.0);
                        height = attrs
                            .iter()
                            .find(|(k, _)| k == "height")
                            .and_then(|(_, v)| parse_len(v))
                            .unwrap_or(0.0);
                        if width <= 0.0 || height <= 0.0 {
                            if let Some((_, vb)) = attrs.iter().find(|(k, _)| k == "viewBox") {
                                let parts: Vec<f64> = vb
                                    .split(|c: char| c.is_ascii_whitespace() || c == ',')
                                    .filter(|t| !t.is_empty())
                                    .filter_map(|t| t.parse::<f64>().ok())
                                    .collect();
                                if parts.len() == 4 {
                                    if width <= 0.0 {
                                        width = parts[2];
                                    }
                                    if height <= 0.0 {
                                        height = parts[3];
                                    }
                                }
                            }
                        }
                    }
                    // `<text/>` has no content and no End event, so it is
                    // finished here; `<text>` waits for its End below.
                    "text" if self_closing => {
                        if let Some(s) = element_to_shape("text", &attrs, Some(String::new()))? {
                            shapes.push(s);
                        }
                    }
                    "text" => {
                        pending_text = Some((attrs, String::new()));
                    }
                    "rect" | "circle" | "line" | "path" => {
                        if let Some(s) = element_to_shape(&tag, &attrs, None)? {
                            shapes.push(s);
                        }
                    }
                    // `<g>` and everything else: descend, collect nothing.
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((_, body)) = pending_text.as_mut() {
                    body.push_str(&t.into_inner().into_owned());
                }
            }
            // This `quick-xml` version splits text around each entity or
            // character reference into a SEPARATE event rather than
            // resolving it into `Text` — see `xml_ops::parse_root`'s own
            // note, which records that contradicting the crate's docs cost
            // a real silently-dropped-character bug there.
            Ok(Event::GeneralRef(r)) => {
                if pending_text.is_some() {
                    let resolved = if r.is_char_ref() {
                        r.resolve_char_ref()
                            .map_err(|err| EvalError { msg: format!("malformed character reference: {err}") })?
                            .ok_or_else(|| EvalError { msg: "malformed character reference".to_string() })?
                            .to_string()
                    } else {
                        let name = r.into_inner();
                        quick_xml::escape::resolve_xml_entity(&name).map(|s| s.to_string()).ok_or_else(|| {
                            EvalError { msg: format!("unknown XML entity `&{name};`") }
                        })?
                    };
                    body_push(&mut pending_text, &resolved);
                }
            }
            Ok(Event::End(el)) => {
                if local_name(el.name()) == "text" {
                    if let Some((attrs, body)) = pending_text.take() {
                        if let Some(s) = element_to_shape("text", &attrs, Some(body))? {
                            shapes.push(s);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return e(format!("not valid XML: {err}")),
        }
    }
    if !seen_root {
        return e("no <svg> root element -- this does not look like an SVG file");
    }
    Ok(Value::Record(Arc::new(vec![
        ("width".to_string(), Value::Num(width)),
        ("height".to_string(), Value::Num(height)),
        ("shapes".to_string(), Value::List(Arc::new(shapes))),
    ])))
}

fn body_push(pending: &mut Option<(Vec<(String, String)>, String)>, s: &str) {
    if let Some((_, body)) = pending.as_mut() {
        body.push_str(s);
    }
}

/// `svg:rect` and `rect` are the same element; the prefix depends on how
/// the writing tool declared the namespace.
///
/// `QName` in this `quick-xml` version already wraps a `&str` (see
/// `xml_ops::qname_to_string`'s note), so there is no UTF-8 check here.
fn local_name(name: QName) -> String {
    let s: &str = name.as_ref();
    match s.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.to_string(),
    }
}

fn attrs_of(el: &BytesStart) -> R<Vec<(String, String)>> {
    let mut out = Vec::new();
    for a in el.attributes() {
        let a = a.map_err(|err| EvalError { msg: format!("malformed attribute: {err}") })?;
        let key = local_name(a.key);
        // `normalized_value` is what `xml_ops::read_attrs` uses in this
        // version: it resolves the entity references inside the value.
        let val = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| EvalError { msg: format!("malformed attribute value: {err}") })?
            .into_owned();
        out.push((key, val));
    }
    Ok(out)
}

fn attr_num(attrs: &[(String, String)], key: &str) -> Option<f64> {
    attrs.iter().find(|(k, _)| k == key).and_then(|(_, v)| parse_len(v))
}

/// A length with an optional CSS unit suffix. Only absolute units are
/// honored; a percentage depends on a viewport this reader does not
/// resolve, so it is refused (None) rather than silently read as a count
/// of pixels.
fn parse_len(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.ends_with('%') {
        return None;
    }
    let num_end = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(num_end);
    let v: f64 = num.parse().ok()?;
    let scale = match unit.trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        _ => return None,
    };
    Some(v * scale)
}

fn element_to_shape(tag: &str, attrs: &[(String, String)], text_body: Option<String>) -> R<Option<Value>> {
    let paint = |key: &str, default: &str| -> String {
        attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_else(|| default.to_string())
    };
    let def_fill = if tag == "line" || tag == "path" { "none" } else { "#000000" };
    let def_stroke = if tag == "line" || tag == "path" { "#000000" } else { "none" };
    let fill = paint("fill", def_fill);
    let stroke = paint("stroke", def_stroke);
    let stroke_width = attr_num(attrs, "stroke-width").unwrap_or(1.0);
    let opacity = attr_num(attrs, "opacity").unwrap_or(1.0);

    // A required coordinate that is absent defaults to 0, which is what
    // the SVG spec itself says for x/y/cx/cy — not an error.
    let n = |k: &str| attr_num(attrs, k).unwrap_or(0.0);

    let shape = match tag {
        "rect" => Shape {
            kind: "rect",
            geom: vec![
                ("x", Value::Num(n("x"))),
                ("y", Value::Num(n("y"))),
                ("width", Value::Num(n("width"))),
                ("height", Value::Num(n("height"))),
            ],
            fill,
            stroke,
            stroke_width,
            opacity,
        },
        "circle" => Shape {
            kind: "circle",
            geom: vec![("cx", Value::Num(n("cx"))), ("cy", Value::Num(n("cy"))), ("r", Value::Num(n("r")))],
            fill,
            stroke,
            stroke_width,
            opacity,
        },
        "line" => Shape {
            kind: "line",
            geom: vec![
                ("x1", Value::Num(n("x1"))),
                ("y1", Value::Num(n("y1"))),
                ("x2", Value::Num(n("x2"))),
                ("y2", Value::Num(n("y2"))),
            ],
            fill,
            stroke,
            stroke_width,
            opacity,
        },
        "path" => {
            let d = match attrs.iter().find(|(k, _)| k == "d") {
                Some((_, v)) => v.clone(),
                // A `<path>` with no `d` draws nothing; skipping it is
                // closer to what the file means than inventing one.
                None => return Ok(None),
            };
            // Round-tripped through the same validator the writer uses, so
            // a path this reader accepts is one the writer can write back.
            let d = validate_path_data(&d)?;
            Shape { kind: "path", geom: vec![("d", Value::Str(d))], fill, stroke, stroke_width, opacity }
        }
        "text" => {
            let body = text_body.unwrap_or_default();
            let fs = attr_num(attrs, "font-size").unwrap_or(16.0);
            let ff = attrs
                .iter()
                .find(|(k, _)| k == "font-family")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "sans-serif".to_string());
            Shape {
                kind: "text",
                geom: vec![
                    ("x", Value::Num(n("x"))),
                    ("y", Value::Num(n("y"))),
                    ("text", Value::Str(body)),
                    ("font_size", Value::Num(fs)),
                    ("font_family", Value::Str(ff)),
                ],
                fill,
                stroke,
                stroke_width,
                opacity,
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(shape.into_record()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the same shape record `svg.rect(...)` would, without going
    /// through the interpreter.
    fn shp(kind: &'static str, geom: Vec<(&'static str, Value)>, fill: &str, stroke: &str, sw: f64) -> Value {
        Shape { kind, geom, fill: fill.to_string(), stroke: stroke.to_string(), stroke_width: sw, opacity: 1.0 }
            .into_record()
    }

    fn rec_str(v: &Value, key: &str) -> String {
        let Value::Record(r) = v else { panic!("not a record: {v:?}") };
        match field(r.as_slice(), key) {
            Some(Value::Str(s)) => s.clone(),
            other => panic!("`{key}` is not text: {other:?}"),
        }
    }

    fn rec_num(v: &Value, key: &str) -> f64 {
        let Value::Record(r) = v else { panic!("not a record: {v:?}") };
        match field(r.as_slice(), key) {
            Some(Value::Num(n)) => *n,
            other => panic!("`{key}` is not a number: {other:?}"),
        }
    }

    fn shapes_of(doc: &Value) -> Vec<Value> {
        let Value::Record(r) = doc else { panic!("not a record") };
        match field(r.as_slice(), "shapes") {
            Some(Value::List(l)) => l.as_ref().clone(),
            other => panic!("`shapes` is not a list: {other:?}"),
        }
    }

    /// The document must be well-formed XML with the SVG namespace, both
    /// sizing forms, and one element per shape. Checked by PARSING it back
    /// with a real XML parser rather than by substring-matching the string
    /// we just built -- a writer test that only greps its own output
    /// cannot tell valid XML from a tag it forgot to close.
    #[test]
    fn a_written_document_is_well_formed_svg() {
        let shapes = vec![
            shp(
                "rect",
                vec![
                    ("x", Value::Num(10.0)),
                    ("y", Value::Num(20.0)),
                    ("width", Value::Num(100.0)),
                    ("height", Value::Num(50.0)),
                ],
                "#e74c3c",
                "none",
                1.0,
            ),
            shp(
                "circle",
                vec![("cx", Value::Num(200.0)), ("cy", Value::Num(60.0)), ("r", Value::Num(40.0))],
                "#3498db",
                "#000000",
                2.0,
            ),
        ];
        let doc = render(&shapes, 320.0, 180.0).expect("render");

        assert!(doc.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"), "got: {doc}");
        assert!(doc.contains("xmlns=\"http://www.w3.org/2000/svg\""), "no namespace: {doc}");
        // Both width/height AND viewBox -- a viewBox alone renders at the
        // container's size, which is wrong for a standalone file.
        assert!(doc.contains("width=\"320\" height=\"180\" viewBox=\"0 0 320 180\""), "got: {doc}");

        // Now prove it is really XML: every tag balanced, nothing stray.
        let mut reader = Reader::from_str(&doc);
        let mut depth = 0i32;
        let mut elements: Vec<String> = Vec::new();
        loop {
            match reader.read_event() {
                Ok(Event::Start(el)) => {
                    elements.push(local_name(el.name()));
                    depth += 1;
                }
                Ok(Event::Empty(el)) => elements.push(local_name(el.name())),
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(err) => panic!("the document we wrote is not valid XML: {err}\n{doc}"),
            }
            assert!(depth >= 0, "a closing tag with nothing open: {doc}");
        }
        assert_eq!(depth, 0, "unbalanced tags: {doc}");
        assert_eq!(elements, vec!["svg", "rect", "circle"], "got: {elements:?}");
    }

    /// The attributes must be the ones SVG actually names -- `stroke-width`
    /// with a hyphen, not `stroke_width` the way the Qu-side field is
    /// spelt. A renderer silently ignores an attribute it does not know,
    /// so getting this wrong draws a hairline instead of erroring.
    #[test]
    fn styling_is_written_with_svg_attribute_spelling_not_the_qu_field_spelling() {
        let shapes = vec![shp(
            "rect",
            vec![
                ("x", Value::Num(0.0)),
                ("y", Value::Num(0.0)),
                ("width", Value::Num(4.0)),
                ("height", Value::Num(4.0)),
            ],
            "red",
            "blue",
            3.5,
        )];
        let doc = render(&shapes, 10.0, 10.0).expect("render");
        assert!(doc.contains("stroke-width=\"3.5\""), "got: {doc}");
        assert!(!doc.contains("stroke_width"), "Qu's field spelling leaked into the file: {doc}");
        assert!(doc.contains("fill=\"red\""), "got: {doc}");
        assert!(doc.contains("stroke=\"blue\""), "got: {doc}");
    }

    /// `&`, `<` and `>` in a label are ordinary in real data, and written
    /// raw they produce a file that is not XML at all.
    #[test]
    fn text_content_and_attributes_are_xml_escaped() {
        let shapes = vec![shp(
            "text",
            vec![
                ("x", Value::Num(5.0)),
                ("y", Value::Num(15.0)),
                ("text", Value::Str("R&D <load> \"x\"".to_string())),
                ("font_size", Value::Num(12.0)),
                ("font_family", Value::Str("serif".to_string())),
            ],
            "#111111",
            "none",
            1.0,
        )];
        let doc = render(&shapes, 100.0, 40.0).expect("render");
        assert!(doc.contains("R&amp;D &lt;load&gt;"), "got: {doc}");
        // Still parses, and the text comes back out as the original.
        let back = parse(&doc).expect("parse");
        let got = shapes_of(&back);
        assert_eq!(got.len(), 1);
        assert_eq!(rec_str(&got[0], "text"), "R&D <load> \"x\"");
    }

    /// The property that matters for `load_svg`: a file this module wrote
    /// reads back as the same shapes. Checked BOTH ways -- field by field
    /// (so a wrong value cannot hide) and by re-rendering (so a dropped or
    /// reordered shape cannot hide either).
    #[test]
    fn a_written_file_round_trips_through_load_exactly() {
        let shapes = vec![
            shp(
                "rect",
                vec![
                    ("x", Value::Num(10.0)),
                    ("y", Value::Num(20.5)),
                    ("width", Value::Num(100.0)),
                    ("height", Value::Num(50.0)),
                ],
                "#e74c3c",
                "none",
                1.0,
            ),
            shp(
                "circle",
                vec![("cx", Value::Num(200.0)), ("cy", Value::Num(60.0)), ("r", Value::Num(40.0))],
                "#3498db",
                "#000000",
                2.0,
            ),
            shp(
                "line",
                vec![
                    ("x1", Value::Num(0.0)),
                    ("y1", Value::Num(0.0)),
                    ("x2", Value::Num(300.0)),
                    ("y2", Value::Num(120.0)),
                ],
                "none",
                "#333333",
                1.5,
            ),
            shp("path", vec![("d", Value::Str("M 10 100 L 60 40 L 110 100 Z".to_string()))], "none", "#2ecc71", 2.0),
            shp(
                "text",
                vec![
                    ("x", Value::Num(20.0)),
                    ("y", Value::Num(150.0)),
                    ("text", Value::Str("Hello".to_string())),
                    ("font_size", Value::Num(16.0)),
                    ("font_family", Value::Str("sans-serif".to_string())),
                ],
                "#111111",
                "none",
                1.0,
            ),
        ];
        let doc = render(&shapes, 320.0, 180.0).expect("render");
        let back = parse(&doc).expect("parse");

        assert_eq!(rec_num(&back, "width"), 320.0);
        assert_eq!(rec_num(&back, "height"), 180.0);
        let got = shapes_of(&back);
        assert_eq!(got.len(), 5, "shape count changed: {got:?}");

        assert_eq!(rec_str(&got[0], "kind"), "rect");
        assert_eq!(rec_num(&got[0], "y"), 20.5);
        assert_eq!(rec_str(&got[0], "fill"), "#e74c3c");
        assert_eq!(rec_str(&got[0], "stroke"), "none");

        assert_eq!(rec_str(&got[1], "kind"), "circle");
        assert_eq!(rec_num(&got[1], "r"), 40.0);
        assert_eq!(rec_num(&got[1], "stroke_width"), 2.0);

        assert_eq!(rec_str(&got[2], "kind"), "line");
        assert_eq!(rec_num(&got[2], "x2"), 300.0);

        assert_eq!(rec_str(&got[3], "kind"), "path");
        assert_eq!(rec_str(&got[3], "d"), "M 10 100 L 60 40 L 110 100 Z");

        assert_eq!(rec_str(&got[4], "kind"), "text");
        assert_eq!(rec_str(&got[4], "text"), "Hello");
        assert_eq!(rec_num(&got[4], "font_size"), 16.0);

        // And the whole document re-renders byte-identically.
        let again = render(&got, rec_num(&back, "width"), rec_num(&back, "height")).expect("re-render");
        assert_eq!(doc, again, "round trip is not stable");
    }

    #[test]
    fn path_data_accepts_the_documented_subset_and_normalizes_spacing() {
        // Comma separators, no spaces, mixed case, relative forms.
        let got = validate_path_data("M10,20L30,40H50V60Z").expect("valid");
        assert_eq!(got, "M 10 20 L 30 40 H 50 V 60 Z");
        // Curves -- the stretch goal.
        let got = validate_path_data("M 0 0 C 1 2 3 4 5 6 Q 7 8 9 10").expect("valid");
        assert_eq!(got, "M 0 0 C 1 2 3 4 5 6 Q 7 8 9 10");
        // Relative spelling is preserved, not silently absolutized.
        let got = validate_path_data("m 1 1 l 2 2 z").expect("valid");
        assert_eq!(got, "m 1 1 l 2 2 z");
        // Negative numbers run together without separators, as real SVG
        // files write them.
        let got = validate_path_data("M-1.5-2.5L3e2 4").expect("valid");
        assert_eq!(got, "M -1.5 -2.5 L 300 4");
    }

    /// The SVG repetition rule: a moveto's SECOND and later coordinate
    /// pairs are implicit linetos, not extra movetos. Read wrong, a
    /// polygon becomes a scatter of disconnected points -- a file that
    /// renders, just not as the shape it describes.
    #[test]
    fn repeated_moveto_pairs_become_implicit_linetos() {
        let got = validate_path_data("M 0 0 10 0 10 10 Z").expect("valid");
        assert_eq!(got, "M 0 0 L 10 0 L 10 10 Z");
        let got = validate_path_data("m 0 0 5 5").expect("valid");
        assert_eq!(got, "m 0 0 l 5 5");
        // A plain repeated command keeps repeating itself.
        let got = validate_path_data("M 0 0 L 1 1 2 2").expect("valid");
        assert_eq!(got, "M 0 0 L 1 1 L 2 2");
    }

    /// Unsupported commands are REFUSED, not passed through. Passing an
    /// arc through would write a file this module cannot read back while
    /// reporting success -- the failure mode the whole validator exists to
    /// prevent.
    #[test]
    fn unsupported_path_commands_are_refused_by_name() {
        for d in ["M 0 0 A 5 5 0 0 1 10 10", "M 0 0 C 1 1 2 2 3 3 S 4 4 5 5", "M 0 0 Q 1 1 2 2 T 3 3"] {
            let err = validate_path_data(d).expect_err(d);
            assert!(err.msg.contains("not supported"), "got: {}", err.msg);
        }
        // Malformed: a command short of its numbers.
        let err = validate_path_data("M 0 0 L 5").expect_err("short lineto");
        assert!(err.msg.contains("needs 2 number(s)"), "got: {}", err.msg);
        // Data that never starts with a moveto.
        let err = validate_path_data("L 1 1").expect_err("no moveto");
        assert!(err.msg.contains("must start with a moveto"), "got: {}", err.msg);
        let err = validate_path_data("5 5").expect_err("no command");
        assert!(err.msg.contains("must begin with a command letter"), "got: {}", err.msg);
        // Garbage that is not a command at all.
        let err = validate_path_data("M 0 0 X 1 1").expect_err("bad command");
        assert!(err.msg.contains("not an SVG path command"), "got: {}", err.msg);
    }

    /// A `<g>` is not a shape but its children are, and a file from
    /// another tool routinely wraps everything in one. Skipping the group
    /// without descending would read such a file as empty and call it a
    /// success.
    #[test]
    fn shapes_inside_a_group_are_still_found() {
        let src = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"50\" height=\"50\">\
                   <g><rect x=\"1\" y=\"2\" width=\"3\" height=\"4\"/></g></svg>";
        let got = shapes_of(&parse(src).expect("parse"));
        assert_eq!(got.len(), 1, "the group swallowed its child: {got:?}");
        assert_eq!(rec_num(&got[0], "width"), 3.0);
    }

    /// A self-closing `<text/>` emits Empty and never End. Treated as a
    /// Start it would arm the pending-text slot and never disarm it, so
    /// the element would vanish AND its attributes would be handed to the
    /// next `<text>`. Both halves are asserted: the empty one survives
    /// with its own coordinates, and the following one keeps its own.
    #[test]
    fn a_self_closing_text_element_is_not_dropped_or_merged_into_the_next() {
        let src = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"80\" height=\"80\">\
                   <text x=\"1\" y=\"2\"/><text x=\"30\" y=\"40\">hi</text></svg>";
        let got = shapes_of(&parse(src).expect("parse"));
        assert_eq!(got.len(), 2, "the empty <text/> was dropped: {got:?}");
        assert_eq!(rec_num(&got[0], "x"), 1.0);
        assert_eq!(rec_str(&got[0], "text"), "");
        // The second must keep its OWN coordinates, not the first's.
        assert_eq!(rec_num(&got[1], "x"), 30.0, "attributes leaked from the empty element");
        assert_eq!(rec_str(&got[1], "text"), "hi");
    }

    /// A namespace prefix is a spelling of the same element.
    #[test]
    fn a_namespace_prefix_does_not_hide_an_element() {
        let src = "<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\" width=\"9\" height=\"9\">\
                   <svg:circle cx=\"1\" cy=\"2\" r=\"3\"/></svg:svg>";
        let got = shapes_of(&parse(src).expect("parse"));
        assert_eq!(got.len(), 1, "the prefixed element was missed: {got:?}");
        assert_eq!(rec_num(&got[0], "r"), 3.0);
    }

    /// With no usable `width`/`height` the viewBox is the only size the
    /// file states, and a reader that ignored it would report 0 x 0.
    #[test]
    fn the_viewbox_supplies_the_size_when_width_and_height_cannot() {
        let src = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 640 480\"><rect/></svg>";
        let doc = parse(src).expect("parse");
        assert_eq!(rec_num(&doc, "width"), 640.0);
        assert_eq!(rec_num(&doc, "height"), 480.0);

        // A percentage depends on a viewport this reader does not resolve,
        // so it must fall through to the viewBox rather than be read as a
        // count of pixels.
        let src = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" height=\"100%\" \
                   viewBox=\"0 0 32 16\"><rect/></svg>";
        let doc = parse(src).expect("parse");
        assert_eq!(rec_num(&doc, "width"), 32.0, "a percentage was read as pixels");
        assert_eq!(rec_num(&doc, "height"), 16.0);
    }

    /// CSS absolute units are real in files from drawing tools; `1in` is
    /// 96 user units, not 1.
    #[test]
    fn absolute_css_units_on_the_canvas_size_are_converted() {
        let src = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1in\" height=\"72pt\"><rect/></svg>";
        let doc = parse(src).expect("parse");
        assert_eq!(rec_num(&doc, "width"), 96.0);
        assert_eq!(rec_num(&doc, "height"), 96.0);
    }

    /// Not an SVG at all must say so, rather than returning an empty shape
    /// list that reads as "this file has no shapes".
    #[test]
    fn a_file_without_an_svg_root_is_refused() {
        let err = parse("<html><body>no</body></html>").expect_err("not svg");
        assert!(err.msg.contains("does not look like an SVG file"), "got: {}", err.msg);
    }

    /// The defaults differ per shape and getting them backwards produces a
    /// valid file that renders as nothing -- a black-filled line has no
    /// interior to fill, and an unstroked one has nothing to draw.
    #[test]
    fn a_line_defaults_to_stroked_and_an_area_shape_to_filled() {
        let line = svg_call("svg::line", &[Value::Num(0.0), Value::Num(0.0), Value::Num(1.0), Value::Num(1.0)], &[])
            .expect("line");
        assert_eq!(rec_str(&line, "stroke"), "#000000");
        assert_eq!(rec_str(&line, "fill"), "none");

        let rect = svg_call("svg::rect", &[Value::Num(0.0), Value::Num(0.0), Value::Num(1.0), Value::Num(1.0)], &[])
            .expect("rect");
        assert_eq!(rec_str(&rect, "fill"), "#000000");
        assert_eq!(rec_str(&rect, "stroke"), "none");
    }

    /// A number where a color belongs writes `fill="3"`, which every
    /// renderer treats as invalid and silently falls back to black.
    #[test]
    fn a_non_string_paint_is_refused_rather_than_written() {
        let err = svg_call(
            "svg::rect",
            &[Value::Num(0.0), Value::Num(0.0), Value::Num(1.0), Value::Num(1.0)],
            &[("fill".to_string(), Value::Num(3.0))],
        )
        .expect_err("numeric fill");
        assert!(err.msg.contains("needs a color string"), "got: {}", err.msg);
    }

    /// Non-finite geometry would write `NaN`/`inf` into an attribute,
    /// which is not a valid SVG number.
    #[test]
    fn non_finite_geometry_is_refused() {
        for bad in [f64::NAN, f64::INFINITY] {
            let err = svg_call("svg::circle", &[Value::Num(bad), Value::Num(0.0), Value::Num(1.0)], &[])
                .expect_err("non-finite");
            assert!(err.msg.contains("finite"), "got: {}", err.msg);
        }
    }

    /// `save_svg` must reject a canvas that cannot exist, rather than
    /// writing `width="0"` and leaving an invisible file behind.
    #[test]
    fn a_zero_or_negative_canvas_is_refused() {
        let shapes = Value::List(Arc::new(vec![]));
        for (w, h) in [(0.0, 10.0), (10.0, -5.0)] {
            let err = save_svg(
                "unused.svg",
                &shapes,
                &[("width".to_string(), Value::Num(w)), ("height".to_string(), Value::Num(h))],
            )
            .expect_err("bad canvas");
            assert!(err.msg.contains("positive finite"), "got: {}", err.msg);
        }
    }
}
