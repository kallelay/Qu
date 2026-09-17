//! `xmlify(value) -> str` / `parse_xml(str) -> value` (§ data-format
//! conversion builtins, 2026-09-01, Ahmed's ask — see `BACKLOG.md`'s
//! "Format conversion builtins" entry). The genuinely new half of that
//! work: `jsonify`/`parse_json` and `csvify`/`parse_csv` are thin wrappers
//! around pre-existing machinery (`value_to_json`/`json_to_value` in
//! `lib.rs`, `Table::to_csv`/`Table::from_csv` in `table.rs`), but there was
//! no XML support anywhere in this codebase before this module — no crate,
//! no `Value`<->XML convention. Built on `quick-xml` (see `Cargo.toml`'s own
//! comment for why), a pure-Rust, actively-maintained pull-parser/writer —
//! not a hand-rolled tag scanner, per this codebase's standing "real library
//! for a real format" policy (`spade` for Voronoi, `hdf5-metno` for HDF5).
//!
//! Kept in its own module rather than folded into `lib.rs`'s already-huge
//! builtin match, same "minimize collision with concurrent work on this
//! file" reasoning as `fs_ops`/`net_ops`/`serial_ops` above it — this module
//! only touches `lib.rs` at one `pub mod` line and two one-line dispatch
//! arms (`xmlify`/`parse_xml`), since the pairwise `xml2json`/`json2xml`/
//! `xml2csv`/`csv2xml` converters are themselves one-line compositions of
//! `xml_ops::xmlify`/`xml_ops::parse_xml` with `jsonify`/`csvify`/
//! `parse_json`/`parse_csv`, written directly in `lib.rs` next to those.
//!
//! # The `Value` <-> XML mapping
//!
//! Every serialized value is one XML element whose TAG NAME is the value's
//! type name — the direct structural analogue of `value_to_json`'s own
//! `{"type": "...", ...}` envelope, just moved from a JSON object key to
//! the thing XML actually tags things with (an element name), the same way
//! `xmlify`'s `<num>3.5</num>` is doing the identical job as JSON's
//! `{"type":"num","v":3.5}`. This is "conceptually parallel to the existing
//! JSON one" exactly where that's the natural XML shape, and deliberately
//! DIFFERENT where XML has a more idiomatic shape available — concretely,
//! `Table` (rows, not JSON's column-major arrays) and every field/name-keyed
//! container (`Record`/`Dict`/`Model`, whose fields become child elements
//! NAMED after the field, not a JSON-style key inside a generic map).
//!
//! **One rule threads through every container below**: a leaf that's part
//! of an already-homogeneous, already-self-describing numeric/string
//! container (`Vec`/`Mat`/`Signal`/`Mask`/`CVec`/`CMat`'s `<item>`s, a
//! `Table` row's per-column cells) holds BARE text — the parent tag (or, for
//! a table cell, the column's own declared `kind`) already says what it is,
//! so wrapping it again would be redundant. A slot that can hold ANY value
//! kind (a `List` item, a `Record`/`Dict`/`Model` field) wraps its value in
//! a full, independently self-describing node instead (i.e. exactly what
//! `xmlify` would produce for that value standalone) — this is precisely
//! `value_to_json`'s own choice: a `Table` column's JSON `"v"` array holds
//! bare numbers/strings, but a `Record`/`Model` field recurses through
//! `value_to_json` again. XML just makes the same choice visible as two
//! different element shapes instead of two different JSON shapes.
//!
//! | `Value` | XML shape |
//! |---|---|
//! | `Num(n)` | `<num>3.5</num>` — text content. `NaN`/`Infinity`/`-Infinity` use the exact same sentinel strings `value_to_json`'s `f64_to_json`/`json_to_value`'s `as_f64` already use for JSON, not a new XML-specific special case. |
//! | `Bool(b)` | `<bool>true</bool>` / `<bool>false</bool>` |
//! | `Str(s)` | `<str>...</str>` — text content, escaped by hand via `quick_xml::escape::escape` on write (`BytesText::new` does NOT escape `<`/`&`/quotes itself in this `quick-xml` version) and resolved by hand on read, since the reader reports each `&entity;`/`&#NN;` as its own `Event::GeneralRef` rather than folding it into the surrounding `Text` — see `write_node`/`parse_root`'s own comments, both written after this exact gap was caught by this module's own `special_characters_round_trip` test silently losing every entity on the first attempt. |
//! | `Vec(xs)` | `<vec><item>1</item><item>2</item>...</vec>` — XML has no bare-array literal the way JSON does, so each element gets its own `<item>`. |
//! | `Mat(m)` | `<mat rows="R" cols="C"><item>...</item>...</mat>` — column-major flattened items (`Matrix::as_slice()`'s own order), `rows`/`cols` as attributes (scalar shape metadata, not the data payload — same role JSON's sibling `"rows"`/`"cols"` fields play). |
//! | `Complex(c)` | `<complex><re>1</re><im>2</im></complex>` |
//! | `CVec(xs)` | `<cvec><item><re>1</re><im>2</im></item>...</cvec>` |
//! | `CMat(m)` | `<cmat rows="R" cols="C"><item><re/><im/></item>...</cmat>` — column-major, same shape-attribute convention as `mat`. |
//! | `Signal(xs, fs)` | `<signal fs="100"><item>...</item></signal>` |
//! | `Mask(m)` | `<mask><item>true</item><item>false</item></mask>` |
//! | `Table(t)` | Row-oriented — see `table_to_xml_node`'s own doc comment for the full justification. |
//! | `Model(m)` | `kind` attribute + one child element per field, named after the field, wrapping that field's own self-describing node; `stages` (pipeline-only) becomes a `<stages>` list, omitted when empty. See `model_to_xml_node`. |
//! | `List(items)` | `<list><item>...</item>...</list>` — each `<item>` wraps ONE fully self-describing node, since a `List` (unlike `Vec`) is heterogeneous. |
//! | `Record(fields)` / `Dict(pairs)` | `<record>`/`<dict>` + one child element per field/key name, each wrapping its value's own self-describing node — the direct analogue of `value_to_json`'s "object key -> recursively-serialized value" choice for these two. |
//! | `Nothing` | `<nothing/>` |
//! | `EnumVal` | `<enum_val enum="Color" variant="Red"/>` |
//! | `EnumType` | `<enum_type name="Color"/>` |
//! | `Unit(n, Temp(scale))` | `<unit_temp scale="degC">36.6</unit_temp>` — `scale` attribute, same convention as `model`'s `kind`. |
//! | `Unit(n, Family(fam))` | `<unit_family family="Hz">5000</unit_family>` — same convention, `family` attribute. |
//!
//! A `Table` column name or a `Record`/`Dict`/`Model` field name that isn't
//! a legal XML element name (`is_valid_xml_name`: ASCII
//! `[A-Za-z_][A-Za-z0-9_]*`) is a clear, named `xmlify` error, not silently
//! mangled — the practical way this bites is a `read_csv` header with a
//! space or punctuation in it (`csv2xml` on real-world CSV headers), since
//! every name that originates from actual Qu source (`{a = 1}`,
//! `DataFrame(age=...)`) already satisfies this narrower-than-the-real-XML-
//! spec check.
//!
//! Every live runtime handle (`Worker`/`Mutex`/`Semaphore`/`Image`/`Timer`/
//! `File`/`Mmap`/`Channel`/`Tensor`/`TcpListener`/`TcpConn`/`Queue`/`Pool`/
//! `Lazy`/`UrlStream`/`Serial`/`Fifo`/`DoubleBuffer`/`LinkedList`/`Graph`)
//! refuses with a clear, `xmlify`-worded error — the exact same refusal
//! list `value_to_json` already has for `save`/`save_all`, for the exact
//! same reasoning (see that function's own doc comment); not repeated
//! variant-by-variant here.
//!
//! # Round-trip fidelity vs. `Table::to_csv`
//!
//! Number formatting here uses plain `f64::to_string()` (Rust's own
//! shortest-round-trip float `Display`), NOT `fmt_num`/`Table::to_csv`'s
//! 6-significant-digit cosmetic rounding — this is a brand new code path
//! with no reason to inherit CSV's existing (intentional, display-oriented)
//! precision loss. `parse_json(jsonify(x)) == x` and `parse_xml(xmlify(x))
//! == x` both hold exactly for every `Num`/`Vec`/`Mat`/... value tested;
//! `parse_csv(csvify(x)) == x` does NOT for values needing more than 6
//! significant digits, but that's `Table::to_csv`'s own pre-existing,
//! documented behavior, unrelated to this module.
//!
//! # Implementation shape
//!
//! `Value` <-> XML goes through a small untyped intermediate tree
//! (`XmlNode`: tag, attributes, children, text — never both children AND
//! text on the same node, by construction), the same "convert to/from a
//! generic tree first" shape `value_to_json`/`json_to_value` use with
//! `serde_json::Value` as their intermediate. `quick-xml`'s `Writer`
//! serializes an `XmlNode` tree to a string (`write_node`); its `Reader`
//! parses a string back into one (`parse_root`). Output is written with
//! `Writer::new_with_indent` for human readability — safe to mix with exact
//! text-content round-tripping because `quick-xml`'s indent writer only
//! inserts whitespace around `Start`/`End`/`Empty` element BOUNDARIES, never
//! adjacent to a `Text` event, so it can never corrupt a leaf value's own
//! text (verified both from `quick-xml`'s own docs and by this module's own
//! `xml_string_round_trip_preserves_whitespace` test using a string value
//! with meaningful leading/trailing spaces).

use std::sync::Arc;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use quick_xml::XmlVersion;

use qu_core::cmatrix::CMatrix;
use qu_core::matrix::Matrix;
use qu_core::Complex64;

use crate::{dim_for_name, e, Column, Dim, EvalError, ModelHandle, Table, TempScale, UnitTag, Value, R};

/// `xmlify(value)` — see this module's own doc comment for the full
/// `Value`<->XML mapping.
pub fn xmlify(v: &Value) -> R<String> {
    let node = value_to_xml_node(v)?;
    xml_node_to_string(&node)
}

/// `parse_xml(str)` — `xmlify`'s inverse.
pub fn parse_xml(s: &str) -> R<Value> {
    let root = parse_root(s)?;
    xml_node_to_value(&root)
}

// ---------------------------------------------------------------------
// The intermediate tree
// ---------------------------------------------------------------------

/// A generic, untyped XML element tree — this module's analogue of
/// `serde_json::Value` as the intermediate `Value` converts through. By
/// construction (every constructor below), a node either holds `children`
/// (a container) or `text` (a leaf) — never a meaningful mix of both, which
/// keeps every reader in this file simple (look at `.text` OR `.children`,
/// never both).
#[derive(Debug, Clone)]
struct XmlNode {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl XmlNode {
    fn leaf(tag: impl Into<String>, text: impl Into<String>) -> Self {
        XmlNode { tag: tag.into(), attrs: Vec::new(), children: Vec::new(), text: text.into() }
    }
    fn empty(tag: impl Into<String>) -> Self {
        Self::leaf(tag, String::new())
    }
    fn container(tag: impl Into<String>, children: Vec<XmlNode>) -> Self {
        XmlNode { tag: tag.into(), attrs: Vec::new(), children, text: String::new() }
    }
    fn with_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((key.into(), value.into()));
        self
    }
    fn attr(&self, key: &str) -> R<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| EvalError { msg: format!("parse_xml: <{}> is missing the `{key}` attribute", self.tag) })
    }
    /// Like `attr`, but `None` (not an error) when the attribute is
    /// absent -- for the `unit_dim` node's `spelling`, which is legitimately
    /// missing whenever the tag has no preferred display unit.
    fn attr_opt(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    /// The first child named `tag`, e.g. a `<row>`'s `<age>` cell or a
    /// `<complex>`'s `<re>` part.
    fn child(&self, tag: &str) -> R<&XmlNode> {
        self.children
            .iter()
            .find(|c| c.tag == tag)
            .ok_or_else(|| EvalError { msg: format!("parse_xml: <{}> is missing a <{tag}> child", self.tag) })
    }
}

// ---------------------------------------------------------------------
// XmlNode -> String (writing)
// ---------------------------------------------------------------------

fn xml_node_to_string(node: &XmlNode) -> R<String> {
    let mut buf = Vec::new();
    {
        let mut writer = Writer::new_with_indent(&mut buf, b' ', 2);
        write_node(&mut writer, node).map_err(|err| EvalError { msg: format!("xmlify: internal XML write error: {err}") })?;
    }
    String::from_utf8(buf).map_err(|err| EvalError { msg: format!("xmlify: produced non-UTF-8 XML: {err}") })
}

fn write_node<W: std::io::Write>(writer: &mut Writer<W>, node: &XmlNode) -> Result<(), std::io::Error> {
    let mut start = BytesStart::new(node.tag.as_str());
    for (k, v) in &node.attrs {
        start.push_attribute((k.as_str(), v.as_str()));
    }
    if node.children.is_empty() && node.text.is_empty() {
        writer.write_event(Event::Empty(start))
    } else {
        writer.write_event(Event::Start(start))?;
        if !node.text.is_empty() {
            // `BytesText::new` does NOT escape `<`/`&`/quotes on write in
            // this `quick-xml` version (confirmed the hard way: an earlier
            // version of this code that relied on it silently corrupted
            // `<tag> & "quotes" & 'apostrophes'` into `tag  quotes
            // apostrophes` — the raw `<`/`&` were written straight into the
            // stream, breaking well-formedness, and the reader silently
            // dropped the resulting garbage rather than erroring). Escaping
            // by hand via `quick_xml::escape::escape` + `from_escaped` is
            // the actually-correct way to write arbitrary text content.
            let escaped = quick_xml::escape::escape(node.text.as_str());
            writer.write_event(Event::Text(BytesText::from_escaped(escaped)))?;
        }
        for child in &node.children {
            write_node(writer, child)?;
        }
        writer.write_event(Event::End(BytesEnd::new(node.tag.as_str())))
    }
}

// ---------------------------------------------------------------------
// String -> XmlNode (parsing)
// ---------------------------------------------------------------------

fn parse_root(xml: &str) -> R<XmlNode> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let tag = qname_to_string(start.name());
                let attrs = read_attrs(&start)?;
                stack.push(XmlNode { tag, attrs, children: Vec::new(), text: String::new() });
            }
            Ok(Event::Empty(start)) => {
                let tag = qname_to_string(start.name());
                let attrs = read_attrs(&start)?;
                let node = XmlNode { tag, attrs, children: Vec::new(), text: String::new() };
                finish_node(&mut stack, &mut root, node)?;
            }
            // A `Text` event's own content is already plain (no
            // resolution needed) -- this `quick-xml` version splits text
            // around each entity/character reference (`&amp;`, `&#65;`,
            // ...) into a SEPARATE `Event::GeneralRef`, handled below,
            // rather than folding the resolved character into `Text`
            // itself. Confirmed empirically (this module's own
            // `special_characters_round_trip` test caught the very real
            // bug of silently dropping those references before this arm
            // was added — worth flagging since it contradicts what this
            // crate's own doc comments imply elsewhere about escaping).
            Ok(Event::Text(t)) => {
                let text = t.into_inner().into_owned();
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text);
                }
            }
            Ok(Event::CData(t)) => {
                let text = t.into_inner().into_owned();
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text);
                }
            }
            // `&amp;`/`&lt;`/`&gt;`/`&quot;`/`&apos;` (this mapping only
            // ever writes these five predefined entities, via
            // `quick_xml::escape::escape` in `write_node`) or a numeric
            // character reference (`&#65;`/`&#x41;`). No DTD, so no
            // custom entities to resolve -- an unrecognized named entity
            // is a clear `parse_xml` error, not silently dropped (which is
            // exactly the bug this arm's own doc comment above describes
            // catching).
            Ok(Event::GeneralRef(r)) => {
                let resolved = if r.is_char_ref() {
                    r.resolve_char_ref()
                        .map_err(|err| EvalError { msg: format!("parse_xml: malformed character reference: {err}") })?
                        .ok_or_else(|| EvalError { msg: "parse_xml: malformed character reference".to_string() })?
                        .to_string()
                } else {
                    let name = r.into_inner();
                    quick_xml::escape::resolve_xml_entity(&name).map(|s| s.to_string()).ok_or_else(|| EvalError {
                        msg: format!(
                            "parse_xml: unknown XML entity `&{name};` -- only the five predefined XML entities \
                             (lt, gt, amp, apos, quot) are supported, no DTD/custom entities"
                        ),
                    })?
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolved);
                }
            }
            Ok(Event::End(_)) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| EvalError { msg: "parse_xml: unbalanced closing tag".to_string() })?;
                finish_node(&mut stack, &mut root, node)?;
            }
            Ok(Event::Eof) => break,
            // Declaration/comment/processing-instruction — not part of this
            // mapping's data model, skipped rather than rejected (a
            // hand-written `<?xml version="1.0"?>` prolog should not make
            // `parse_xml` fail).
            Ok(_) => {}
            Err(err) => return e(format!("parse_xml: malformed XML: {err}")),
        }
    }
    root.ok_or_else(|| EvalError { msg: "parse_xml: empty document (no root element found)".to_string() })
}

fn finish_node(stack: &mut Vec<XmlNode>, root: &mut Option<XmlNode>, node: XmlNode) -> R<()> {
    match stack.last_mut() {
        Some(parent) => {
            parent.children.push(node);
            Ok(())
        }
        None => {
            if root.is_some() {
                return e("parse_xml: multiple root elements found (XML allows exactly one)");
            }
            *root = Some(node);
            Ok(())
        }
    }
}

/// `QName` in this `quick-xml` version already wraps a `&str` directly
/// (`QName<'a>(pub &'a str)`), not raw bytes needing a UTF-8 check.
fn qname_to_string(name: QName) -> String {
    name.as_ref().to_string()
}

fn read_attrs(start: &BytesStart) -> R<Vec<(String, String)>> {
    let mut out = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|err| EvalError { msg: format!("parse_xml: malformed attribute: {err}") })?;
        let key = qname_to_string(attr.key);
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| EvalError { msg: format!("parse_xml: malformed attribute value: {err}") })?
            .into_owned();
        out.push((key, value));
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// Value -> XmlNode
// ---------------------------------------------------------------------

/// `f64` -> XML text. Plain `to_string()` (exact round-trip), not
/// `fmt_num`'s cosmetic rounding — see this module's own doc comment.
/// `NaN`/`Infinity`/`-Infinity` use the same sentinel strings
/// `value_to_json`'s `f64_to_json` already established for JSON.
fn xml_num_text(x: f64) -> String {
    if x.is_nan() {
        "NaN".to_string()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else {
        x.to_string()
    }
}

fn complex_item_node(c: Complex64) -> XmlNode {
    XmlNode::container("item", vec![XmlNode::leaf("re", xml_num_text(c.re)), XmlNode::leaf("im", xml_num_text(c.im))])
}

/// See the module doc comment's table for the ASCII-only rule and why a
/// `read_csv` header (not a Qu identifier) is the realistic way to hit it.
fn is_valid_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn value_to_xml_node(v: &Value) -> R<XmlNode> {
    Ok(match v {
        Value::Artist(_) => {
            return Err(crate::EvalError { msg: "a plot handle cannot be serialized".into() })
        }
        Value::Report(_) => {
            return Err(crate::EvalError { msg: "a report handle cannot be serialized".into() })
        }
        Value::Module(_) => {
            return Err(crate::EvalError { msg: "a module cannot be serialized".into() })
        }
        Value::Func(_) => {
            return Err(crate::EvalError { msg: "a function cannot be serialized".into() })
        }
        // A tagged container has no serialized form YET. Refusing beats
        // writing the numbers and dropping the unit, which is the failure
        // that would only surface when someone loaded the file back and
        // got a vector of bare numbers that used to be ohms. Nothing can
        // hit this today -- the variant is new -- so it breaks nothing,
        // and it is a small, well-marked piece of follow-up.
        Value::Quantity(_, _) => {
            return Err(crate::EvalError {
                msg: "a value with a unit cannot be serialized yet -- strip it first                       (the unit would be lost silently otherwise)"
                    .into(),
            })
        }
        Value::Layer(_) => {
            return Err(crate::EvalError { msg: "a layer cannot be serialized".into() })
        }
        Value::Num(n) => XmlNode::leaf("num", xml_num_text(*n)),
        Value::Bool(b) => XmlNode::leaf("bool", if *b { "true" } else { "false" }),
        Value::Str(s) => XmlNode::leaf("str", s.clone()),
        Value::Vec(xs) => {
            XmlNode::container("vec", xs.iter().map(|&x| XmlNode::leaf("item", xml_num_text(x))).collect())
        }
        Value::Mat(m) => {
            let (rows, cols) = m.shape();
            XmlNode::container("mat", m.as_slice().iter().map(|&x| XmlNode::leaf("item", xml_num_text(x))).collect())
                .with_attr("rows", rows.to_string())
                .with_attr("cols", cols.to_string())
        }
        Value::Complex(c) => XmlNode::container(
            "complex",
            vec![XmlNode::leaf("re", xml_num_text(c.re)), XmlNode::leaf("im", xml_num_text(c.im))],
        ),
        Value::CVec(xs) => XmlNode::container("cvec", xs.iter().map(|&c| complex_item_node(c)).collect()),
        Value::CMat(m) => {
            let (rows, cols) = m.shape();
            XmlNode::container("cmat", m.as_slice().iter().map(|&c| complex_item_node(c)).collect())
                .with_attr("rows", rows.to_string())
                .with_attr("cols", cols.to_string())
        }
        Value::Signal(xs, fs) => {
            XmlNode::container("signal", xs.iter().map(|&x| XmlNode::leaf("item", xml_num_text(x))).collect())
                .with_attr("fs", xml_num_text(*fs))
        }
        Value::Circuit(c) => XmlNode::container(
            "circuit",
            crate::circuit_spec::to_params(c)
                .iter()
                .map(|v| XmlNode::leaf("item", xml_num_text(*v)))
                .collect(),
        )
        .with_attr("spec", crate::circuit_spec::to_spec(c)),
        Value::Spectrum(xs, fs, n, norm) => {
            XmlNode::container("spectrum", xs.iter().map(|&c| complex_item_node(c)).collect())
                .with_attr("fs", xml_num_text(*fs))
                .with_attr("n", n.to_string())
                .with_attr("norm", norm.name().to_string())
        }
        Value::Mask(m) => {
            XmlNode::container("mask", m.iter().map(|&b| XmlNode::leaf("item", if b { "true" } else { "false" })).collect())
        }
        Value::Table(t) => table_to_xml_node(t)?,
        Value::Model(m) => model_to_xml_node(m)?,
        Value::List(items) => {
            let children: R<Vec<XmlNode>> =
                items.iter().map(|it| Ok(XmlNode::container("item", vec![value_to_xml_node(it)?]))).collect();
            XmlNode::container("list", children?)
        }
        Value::Record(fields) => fields_to_xml_node("record", fields)?,
        Value::Dict(pairs) => fields_to_xml_node("dict", pairs)?,
        Value::Nothing => XmlNode::empty("nothing"),
        Value::EnumVal(v) => XmlNode::empty("enum_val").with_attr("enum", v.0.clone()).with_attr("variant", v.1.clone()),
        Value::EnumType(name) => XmlNode::empty("enum_type").with_attr("name", name.as_ref().clone()),
        Value::Unit(n, UnitTag::Temp(scale)) => XmlNode::leaf("unit_temp", xml_num_text(*n)).with_attr("scale", scale.unit_name()),
        // Phase 3 (design doc §8): mirrors `value_to_json`'s "unit_dim"
        // shape -- the raw exponent vector (comma-joined) plus the
        // optional preferred spelling, so a derived (unnamed) dimension
        // round-trips exactly, not just a fixed set of family names.
        Value::Unit(n, UnitTag::Dim(dim, spelling)) => {
            let dim_str = dim.0.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(",");
            let node = XmlNode::leaf("unit_dim", xml_num_text(*n)).with_attr("dim", dim_str);
            match spelling {
                Some(s) => node.with_attr("spelling", *s),
                None => node,
            }
        }
        // Live runtime handles — same refusal list, same reasoning, as
        // `value_to_json`'s own (see that function's doc comment; not
        // repeated here). `?` on `e(...)` (always `Err`) short-circuits out
        // of this function, matching `value_to_json`'s own idiom exactly.
        Value::Worker(_) => e("xmlify: a worker handle can't be converted to XML (it's a live thread reference, not data)")?,
        Value::Mutex(_) => e("xmlify: a mutex can't be converted to XML (it's live shared state, not data)")?,
        Value::Semaphore(_) => e("xmlify: a semaphore can't be converted to XML (it's live shared state, not data)")?,
        Value::Image(_) => e("xmlify: an image isn't supported by xmlify() — use save_image(path, img) instead")?,
        Value::Timer(_) => e("xmlify: a timer can't be converted to XML (it's live stopwatch state, not data)")?,
        Value::File(_) => e("xmlify: a file handle can't be converted to XML (it's a live OS file reference, not data)")?,
        Value::Mmap(_) => e("xmlify: a memory-mapped file handle can't be converted to XML (it's a live OS mapping, not data)")?,
        Value::Lazy(_) => e("xmlify: a lazy variable that hasn't been read yet has no value to convert — read it at least once first")?,
        Value::UrlStream(_) => e("xmlify: a URL stream handle can't be converted to XML (it's a live, position-tracking handle, not data)")?,
        Value::Serial(_) => e("xmlify: a serial port handle can't be converted to XML (it's a live OS device connection, not data)")?,
        Value::Fifo(_) => e("xmlify: a fifo can't be converted to XML (it's a live ring buffer, not data)")?,
        Value::DoubleBuffer(_) => e("xmlify: a double buffer can't be converted to XML (it's a live front/back handle, not data)")?,
        Value::LinkedList(_) => e("xmlify: a linked list can't be converted to XML (it's a live handle, not data) — use .to_vec() first if you want its contents")?,
        Value::Graph(_) => e("xmlify: a graph can't be converted to XML (it's a live handle, not data)")?,
        Value::Channel(_) => e("xmlify: a channel can't be converted to XML (it's a live message queue, not data)")?,
        Value::Tensor(_) => e("xmlify: a tensor can't be converted to XML (its gradient tape is live interpreter state, not data)")?,
        Value::TcpListener(_) => e("xmlify: a TCP listener can't be converted to XML (it's a live OS socket, not data)")?,
        Value::TcpConn(_) => e("xmlify: a TCP connection can't be converted to XML (it's a live OS socket, not data)")?,
        Value::Queue(_) => e("xmlify: a queue can't be converted to XML (it's a live job list, not data)")?,
        Value::Pool(_) => e("xmlify: a pool can't be converted to XML (it's a live worker-pool handle, not data)")?,
    })
}

/// `Value::Table` -> XML — the one genuinely new shape decision in this
/// mapping (the module doc comment explains the general rule; this is the
/// one place it doesn't mechanically apply). Row-oriented, NOT
/// `value_to_json`'s column-major arrays: one `<row>` child per table row,
/// and inside each row one child element per column, NAMED after the
/// column (mirroring `Record`/`Model`'s "field name -> child element name"
/// choice below).
///
/// Rejected alternative: attributes-per-row (`<row name="Alice" age="30"
/// />`). Column values are arbitrary text — possibly containing leading/
/// trailing whitespace or characters attributes normalize away — so an
/// attribute is a poor home for primary data, only for scalar metadata
/// (which is exactly what `rows`/`cols`/`kind` ARE used for elsewhere in
/// this mapping). Nested elements also keep every leaf value in this whole
/// mapping represented the same way: a named element holding text content.
///
/// An explicit `<columns>` element lists column names (and each column's
/// `kind`, `"num"` or `"str"`) up front, mirroring `value_to_json`'s own
/// explicit `"columns"` JSON array for the identical reason its comment
/// gives: column order is the table's own property, not implicit row-
/// element order (which would additionally be unrecoverable for a
/// zero-row table). The per-column `kind` attribute is necessary, not
/// decorative: without it, a `Str` column that happens to hold only
/// numeral-looking text (zip codes, "007") would be indistinguishable from
/// a `Num` column on the way back in, silently changing the reconstructed
/// column's type — exactly `value_to_json`'s own per-column `"kind"` tag,
/// just moved from a JSON object key to an XML attribute.
fn table_to_xml_node(t: &Table) -> R<XmlNode> {
    let mut column_nodes = Vec::with_capacity(t.ncols());
    for name in t.column_names() {
        if !is_valid_xml_name(name) {
            return e(format!(
                "xmlify: table column name `{name}` can't be used as an XML element name \
                 (must start with a letter or `_`, followed only by letters/digits/`_`)"
            ));
        }
        let kind = match t.col(name).expect("column_names() only returns real columns") {
            Column::Num(_) => "num",
            Column::Str(_) => "str",
        };
        column_nodes.push(XmlNode::leaf("column", name.to_string()).with_attr("kind", kind));
    }
    let mut children = vec![XmlNode::container("columns", column_nodes)];
    for r in 0..t.nrows() {
        let mut cells = Vec::with_capacity(t.ncols());
        for name in t.column_names() {
            let text = match t.col(name).expect("column_names() only returns real columns") {
                Column::Num(v) => xml_num_text(v[r]),
                Column::Str(v) => v[r].clone(),
            };
            cells.push(XmlNode::leaf(name.to_string(), text));
        }
        children.push(XmlNode::container("row", cells));
    }
    Ok(XmlNode::container("table", children))
}

/// `Value::Model` -> XML: `kind` (scalar metadata) becomes an attribute,
/// same reasoning as `mat`'s `rows`/`cols`. Each field becomes a child
/// element NAMED after the field, wrapping that field's own fully
/// self-describing node — the direct XML analogue of `value_to_json`'s own
/// Model handling, which stores fields as a JSON object keyed by field
/// name (JSON object key -> XML child element name). `stages` (pipeline-
/// only, empty for every non-pipeline model — the overwhelming majority)
/// becomes a `<stages>` wrapper of bare `<stage>` text children, the same
/// "wrapper of same-named bare-text children" shape `columns` uses above
/// for a table's column-name list; omitted entirely when empty so the
/// common case doesn't carry a meaningless empty `<stages/>`.
fn model_to_xml_node(m: &ModelHandle) -> R<XmlNode> {
    let mut children = Vec::with_capacity(m.fields.len() + 1);
    for (name, value) in &m.fields {
        if !is_valid_xml_name(name) {
            return e(format!(
                "xmlify: model field `{name}` can't be used as an XML element name \
                 (must start with a letter or `_`, followed only by letters/digits/`_`)"
            ));
        }
        children.push(XmlNode::container(name.clone(), vec![value_to_xml_node(value)?]));
    }
    if !m.stages.is_empty() {
        let stage_nodes = m.stages.iter().map(|s| XmlNode::leaf("stage", s.clone())).collect();
        children.push(XmlNode::container("stages", stage_nodes));
    }
    Ok(XmlNode::container("model", children).with_attr("kind", m.kind.clone()))
}

/// Shared by `Record`/`Dict` -> XML: one child element per field/key name,
/// each wrapping its value's own self-describing node. See
/// `model_to_xml_node`'s doc comment for the full "object key -> child
/// element name" justification (identical reasoning, no `kind` attribute).
fn fields_to_xml_node(tag: &str, fields: &[(String, Value)]) -> R<XmlNode> {
    let mut children = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        if !is_valid_xml_name(name) {
            return e(format!(
                "xmlify: {tag} field `{name}` can't be used as an XML element name \
                 (must start with a letter or `_`, followed only by letters/digits/`_`)"
            ));
        }
        children.push(XmlNode::container(name.clone(), vec![value_to_xml_node(value)?]));
    }
    Ok(XmlNode::container(tag, children))
}

// ---------------------------------------------------------------------
// XmlNode -> Value
// ---------------------------------------------------------------------

fn xml_parse_f64(s: &str, ctx: &str) -> R<f64> {
    match s {
        "NaN" => Ok(f64::NAN),
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        other => other.trim().parse::<f64>().map_err(|_| EvalError { msg: format!("parse_xml: {ctx}: `{other}` isn't a valid number") }),
    }
}

fn xml_parse_usize(s: &str, ctx: &str) -> R<usize> {
    s.trim().parse::<usize>().map_err(|_| EvalError { msg: format!("parse_xml: {ctx}: `{s}` isn't a valid non-negative integer") })
}

fn xml_children_as_f64_vec(n: &XmlNode, ctx: &str) -> R<Vec<f64>> {
    n.children.iter().map(|c| xml_parse_f64(&c.text, ctx)).collect()
}

fn xml_parse_complex(n: &XmlNode) -> R<Complex64> {
    Ok(Complex64 { re: xml_parse_f64(&n.child("re")?.text, "complex re")?, im: xml_parse_f64(&n.child("im")?.text, "complex im")? })
}

/// One value's worth of self-describing node, wrapped one level down under
/// a field-name element (`<coef><vec>...</vec></coef>`) — `model`/`record`/
/// `dict`/`list`'s shared "unwrap one level, then recurse" step.
fn xml_first_child_value(wrapper: &XmlNode) -> R<Value> {
    let inner = wrapper
        .children
        .first()
        .ok_or_else(|| EvalError { msg: format!("parse_xml: <{}> has no value inside it", wrapper.tag) })?;
    xml_node_to_value(inner)
}

fn xml_node_to_fields(n: &XmlNode) -> R<Vec<(String, Value)>> {
    n.children.iter().map(|c| Ok((c.tag.clone(), xml_first_child_value(c)?))).collect()
}

fn xml_node_to_table(n: &XmlNode) -> R<Value> {
    let columns_node = n.child("columns")?;
    let mut names = Vec::with_capacity(columns_node.children.len());
    let mut columns: Vec<Column> = Vec::with_capacity(columns_node.children.len());
    for c in &columns_node.children {
        if c.tag != "column" {
            return e(format!("parse_xml: <columns> contains a non-<column> element `<{}>`", c.tag));
        }
        match c.attr("kind")? {
            "num" => columns.push(Column::Num(Vec::new())),
            "str" => columns.push(Column::Str(Vec::new())),
            other => return e(format!("parse_xml: table column `{}` has unknown kind `{other}` (expected `num` or `str`)", c.text)),
        }
        names.push(c.text.clone());
    }
    for row in n.children.iter().filter(|c| c.tag == "row") {
        for (i, name) in names.iter().enumerate() {
            let cell = row.child(name)?;
            match &mut columns[i] {
                Column::Num(v) => v.push(xml_parse_f64(&cell.text, &format!("table column `{name}`"))?),
                Column::Str(v) => v.push(cell.text.clone()),
            }
        }
    }
    let built: Vec<(String, Column)> = names.into_iter().zip(columns).collect();
    Ok(Value::Table(Arc::new(Table::from_columns(built).map_err(|msg| EvalError { msg: format!("parse_xml: {msg}") })?)))
}

fn xml_node_to_model(n: &XmlNode) -> R<Value> {
    let kind = n.attr("kind")?.to_string();
    let mut fields = Vec::new();
    let mut stages = Vec::new();
    for child in &n.children {
        if child.tag == "stages" {
            stages = child.children.iter().map(|s| s.text.clone()).collect();
        } else {
            fields.push((child.tag.clone(), xml_first_child_value(child)?));
        }
    }
    Ok(Value::Model(Arc::new(ModelHandle::with_stages(kind, fields, stages))))
}

fn xml_node_to_value(n: &XmlNode) -> R<Value> {
    Ok(match n.tag.as_str() {
        "num" => Value::Num(xml_parse_f64(&n.text, "num")?),
        "bool" => match n.text.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            other => return e(format!("parse_xml: <bool> expected `true`/`false`, found `{other}`")),
        },
        "str" => Value::Str(n.text.clone()),
        "vec" => Value::Vec(Arc::new(xml_children_as_f64_vec(n, "vec item")?)),
        "mat" => {
            let rows = xml_parse_usize(n.attr("rows")?, "mat rows")?;
            let cols = xml_parse_usize(n.attr("cols")?, "mat cols")?;
            Value::Mat(Arc::new(Matrix::from_col_major(rows, cols, xml_children_as_f64_vec(n, "mat item")?)))
        }
        "complex" => Value::Complex(xml_parse_complex(n)?),
        "cvec" => {
            let items: R<Vec<Complex64>> = n.children.iter().map(xml_parse_complex).collect();
            Value::CVec(Arc::new(items?))
        }
        "cmat" => {
            let rows = xml_parse_usize(n.attr("rows")?, "cmat rows")?;
            let cols = xml_parse_usize(n.attr("cols")?, "cmat cols")?;
            let items: R<Vec<Complex64>> = n.children.iter().map(xml_parse_complex).collect();
            Value::CMat(Arc::new(CMatrix::from_col_major(rows, cols, items?)))
        }
        "signal" => {
            let fs = xml_parse_f64(n.attr("fs")?, "signal fs")?;
            Value::Signal(Arc::new(xml_children_as_f64_vec(n, "signal item")?), fs)
        }
        "circuit" => {
            let spec = n.attr("spec")?.to_string();
            let params = xml_children_as_f64_vec(n, "circuit param")?;
            Value::Circuit(Arc::new(
                crate::circuit_spec::parse(&spec, &params).map_err(|m| crate::EvalError {
                    msg: format!("malformed circuit: {m}"),
                })?,
            ))
        }
        "spectrum" => {
            let fs = xml_parse_f64(n.attr("fs")?, "spectrum fs")?;
            let len = xml_parse_usize(n.attr("n")?, "spectrum n")?;
            let norm = crate::SpectrumNorm::from_name(n.attr("norm")?).ok_or_else(|| {
                crate::EvalError {
                    msg: "spectrum has an unknown normalisation -- written by a newer Qu".into(),
                }
            })?;
            let items: R<Vec<Complex64>> = n.children.iter().map(xml_parse_complex).collect();
            Value::Spectrum(Arc::new(items?), fs, len, norm)
        }
        "mask" => {
            let bits: R<Vec<bool>> = n
                .children
                .iter()
                .map(|c| match c.text.as_str() {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    other => e(format!("parse_xml: mask <item> expected `true`/`false`, found `{other}`")),
                })
                .collect();
            Value::Mask(bits?)
        }
        "table" => return xml_node_to_table(n),
        "model" => return xml_node_to_model(n),
        "list" => {
            let items: R<Vec<Value>> = n.children.iter().map(xml_first_child_value).collect();
            Value::List(Arc::new(items?))
        }
        "record" => Value::Record(Arc::new(xml_node_to_fields(n)?)),
        "dict" => Value::Dict(Arc::new(xml_node_to_fields(n)?)),
        "nothing" => Value::Nothing,
        "enum_val" => Value::EnumVal(Arc::new((n.attr("enum")?.to_string(), n.attr("variant")?.to_string()))),
        "enum_type" => Value::EnumType(Arc::new(n.attr("name")?.to_string())),
        "unit_temp" => {
            let val = xml_parse_f64(&n.text, "unit_temp")?;
            match n.attr("scale")? {
                "degC" => Value::Unit(val, UnitTag::Temp(TempScale::Celsius)),
                "degF" => Value::Unit(val, UnitTag::Temp(TempScale::Fahrenheit)),
                other => return e(format!("parse_xml: unknown temperature scale `{other}`")),
            }
        }
        // Phase 3 shape (current `value_to_xml_node`): the raw exponent
        // vector plus the optional preferred spelling.
        "unit_dim" => {
            let val = xml_parse_f64(&n.text, "unit_dim")?;
            let dim_str = n.attr("dim")?;
            let parts: Vec<&str> = dim_str.split(',').collect();
            if parts.len() != 7 {
                return e("parse_xml: malformed unit_dim -- `dim` must have 7 entries".to_string());
            }
            let mut d = [0i8; 7];
            for (i, p) in parts.iter().enumerate() {
                d[i] = p.trim().parse::<i8>().map_err(|_| EvalError {
                    msg: format!("parse_xml: malformed unit_dim exponent `{p}`"),
                })?;
            }
            let spelling = n.attr_opt("spelling").and_then(|s| qu_lexer::UNITS.iter().find(|&&u| u == s).copied());
            Value::Unit(val, UnitTag::Dim(Dim(d), spelling))
        }
        // Legacy shape, from before phase 3 (§8) -- see the identical note
        // on `json_to_value`'s own `"unit_family"` arm in lib.rs, including
        // the "m" (length) fix: this decoder used to list only 8 of
        // `unit_family`'s 9 names.
        "unit_family" => {
            let val = xml_parse_f64(&n.text, "unit_family")?;
            let fam = n.attr("family")?;
            match dim_for_name(fam) {
                Some(d) => {
                    let spelling = qu_lexer::UNITS.iter().find(|&&u| u == fam).copied();
                    Value::Unit(val, UnitTag::Dim(d, spelling))
                }
                None => return e(format!("parse_xml: unknown unit family `{fam}`")),
            }
        }
        other => return e(format!("parse_xml: unknown element `<{other}>` — not a value this mapping understands")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(v: Value) -> Value {
        let xml = xmlify(&v).unwrap_or_else(|err| panic!("xmlify failed: {}", err.msg));
        parse_xml(&xml).unwrap_or_else(|err| panic!("parse_xml failed on:\n{xml}\n\nerror: {}", err.msg))
    }

    #[test]
    fn num_round_trips_exactly() {
        match rt(Value::Num(3.14159265358979)) {
            Value::Num(n) => assert_eq!(n, 3.14159265358979),
            other => panic!("expected Num, got {other:?}"),
        }
    }

    #[test]
    fn nan_and_infinity_round_trip() {
        match rt(Value::Num(f64::NAN)) {
            Value::Num(n) => assert!(n.is_nan()),
            other => panic!("expected Num, got {other:?}"),
        }
        match rt(Value::Num(f64::INFINITY)) {
            Value::Num(n) => assert_eq!(n, f64::INFINITY),
            other => panic!("expected Num, got {other:?}"),
        }
    }

    #[test]
    fn vec_round_trips() {
        match rt(Value::Vec(Arc::new(vec![1.0, 2.0, 3.5]))) {
            Value::Vec(v) => assert_eq!(*v, vec![1.0, 2.0, 3.5]),
            other => panic!("expected Vec, got {other:?}"),
        }
    }

    /// The exact test this module's own doc comment promises: a string
    /// with meaningful leading/trailing whitespace must survive
    /// `Writer::new_with_indent` untouched, proving the indent writer
    /// never inserts whitespace adjacent to a `Text` event.
    #[test]
    fn xml_string_round_trip_preserves_whitespace() {
        let s = "  padded value with  spaces\tand\nnewlines  ".to_string();
        match rt(Value::Str(s.clone())) {
            Value::Str(out) => assert_eq!(out, s),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn special_characters_round_trip() {
        let s = "<tag> & \"quotes\" & 'apostrophes'".to_string();
        match rt(Value::Str(s.clone())) {
            Value::Str(out) => assert_eq!(out, s),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn table_round_trips_with_kind_preserved() {
        let t = Table::from_columns(vec![
            ("name".to_string(), Column::Str(vec!["Alice".to_string(), "007".to_string()])),
            ("age".to_string(), Column::Num(vec![30.0, 25.0])),
        ])
        .unwrap();
        match rt(Value::Table(Arc::new(t))) {
            Value::Table(t2) => {
                assert_eq!(t2.column_names(), vec!["name", "age"]);
                assert_eq!(t2.nrows(), 2);
                match t2.col("name").unwrap() {
                    // "007" must stay a string, not silently become 7.0 --
                    // this is exactly what the per-column `kind` attribute
                    // exists to guarantee.
                    Column::Str(v) => assert_eq!(v, &vec!["Alice".to_string(), "007".to_string()]),
                    other => panic!("expected Str column, got {other:?}"),
                }
                match t2.col("age").unwrap() {
                    Column::Num(v) => assert_eq!(v, &vec![30.0, 25.0]),
                    other => panic!("expected Num column, got {other:?}"),
                }
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn record_round_trips() {
        let fields = vec![("a".to_string(), Value::Num(1.0)), ("b".to_string(), Value::Str("hi".to_string()))];
        match rt(Value::Record(Arc::new(fields))) {
            Value::Record(f) => {
                assert_eq!(f.len(), 2);
                assert!(matches!(&f[0], (k, Value::Num(n)) if k == "a" && *n == 1.0));
                assert!(matches!(&f[1], (k, Value::Str(s)) if k == "b" && s == "hi"));
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn invalid_column_name_is_a_clear_error() {
        let t = Table::from_columns(vec![("full name".to_string(), Column::Str(vec!["Alice".to_string()]))]).unwrap();
        let err = xmlify(&Value::Table(Arc::new(t))).unwrap_err();
        assert!(err.msg.contains("full name"), "got: {}", err.msg);
        assert!(err.msg.contains("xmlify"), "got: {}", err.msg);
    }

    #[test]
    fn live_handle_refuses_clearly() {
        let err = xmlify(&Value::Worker(1)).unwrap_err();
        assert!(err.msg.contains("worker"), "got: {}", err.msg);
        assert!(err.msg.contains("xmlify"), "got: {}", err.msg);
    }
}
