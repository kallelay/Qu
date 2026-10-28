//! Changing and adding text in an existing PDF (v0.4.12).
//!
//! # `replace_text`
//!
//! PDF does not store text, it stores "draw these glyph codes with this
//! font". Changing a word is therefore only safe when the bytes of the
//! string operand can be mapped to characters AND back. That is true for
//! *simple* fonts (Type1 / TrueType / MMType1: one byte = one glyph, with
//! an `/Encoding`), and it is false for composite fonts (Type0 / CID),
//! where the codes are indexes into a subset that has no stable
//! relationship to Unicode. This module edits the first kind and refuses
//! the second with an error that names the font and the reason.
//!
//! What is handled: `Tj`, `TJ`, `'` and `"` operands, literal or hex
//! strings, in a page's content stream(s) (Flate is decoded, the edited
//! page's stream is re-encoded as ONE stream; pages that are not touched
//! are not re-serialised by this function at all). A match may span
//! several strings of one `TJ` array and several consecutive show
//! operators that have no other operator between them. A `TJ` gap wider
//! than 200 thousandths of an em counts as a space (as in `extract_text`),
//! and a match over such a gap removes the gap.
//!
//! What is not: text inside Form XObjects; text split by a positioning
//! operator (`Td`, `Tm`, colour change, ...) between the pieces; composite
//! fonts; replacement characters the font cannot show. For subset
//! fonts (`ABCDEF+Name`) and Type3 fonts only characters already shown by
//! that font on that page are allowed in the replacement, because the
//! glyph outlines of the others were never embedded. Nothing is reflowed:
//! the new text is drawn from the same starting point, so a longer
//! replacement can overlap whatever follows it on the line.

use super::*;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Encoding};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Per-font facts needed to edit its strings.
struct FontInfo {
    /// byte -> the single char it shows (None: undefined code).
    to_char: Vec<Option<char>>,
    /// Why this font cannot be edited (composite, odd encoding, ...).
    unsupported: Option<String>,
    /// Only characters already shown may be reused (subset / Type3).
    restricted: bool,
    label: String,
}

fn name_str(o: Option<&Object>) -> String {
    match o {
        Some(Object::Name(n)) => String::from_utf8_lossy(n).into_owned(),
        _ => String::new(),
    }
}

fn font_info(doc: &Document, name: &[u8], font: &Dictionary) -> FontInfo {
    let sub = name_str(font.get(b"Subtype").ok());
    let base = name_str(font.get(b"BaseFont").ok());
    let label = if base.is_empty() {
        format!("/{} ({})", String::from_utf8_lossy(name), sub)
    } else {
        format!("/{} ({})", String::from_utf8_lossy(name), base)
    };
    let none = |why: String| FontInfo { to_char: vec![None; 256], unsupported: Some(why), restricted: true, label: label.clone() };
    match sub.as_str() {
        "Type0" => {
            return none(format!(
                "font {label} is a composite (Type0/CID) font: its glyph codes are indexes into the embedded \
                 font, not characters, so text in it cannot be edited safely"
            ))
        }
        "Type1" | "TrueType" | "MMType1" | "Type3" => {}
        other => return none(format!("font {label} has unsupported subtype `{other}`")),
    }
    let enc = match font.get_font_encoding(doc) {
        Ok(e) => e,
        Err(e) => return none(format!("font {label} has an encoding this editor cannot read ({e})")),
    };
    if matches!(enc, Encoding::UnicodeMapEncoding(_)) {
        // A ToUnicode CMap is a reading aid, not the font's encoding.
        return none(format!(
            "font {label} is described only by a ToUnicode map, so the codes for new characters cannot be derived safely"
        ));
    }
    let mut to_char = vec![None; 256];
    for b in 0..=255u8 {
        if let Ok(s) = enc.bytes_to_string(&[b]) {
            let mut it = s.chars();
            if let (Some(c), None) = (it.next(), it.next()) {
                to_char[b as usize] = Some(c);
            }
        }
    }
    let subset = base.len() > 7 && base.as_bytes()[6] == b'+' && base[..6].chars().all(|c| c.is_ascii_uppercase());
    FontInfo { to_char, unsupported: None, restricted: subset || sub == "Type3", label }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Loc {
    /// byte `pos` of the string operand (`item` is the TJ array index, None for Tj/'/").
    Byte { op: usize, item: Option<usize>, pos: usize },
    /// a TJ number that stands for a word gap.
    Gap { op: usize, item: usize },
}

struct Atom {
    ch: char,
    loc: Loc,
}

struct Chain {
    font: Vec<u8>,
    /// (op index) of each show operator in order.
    ops: Vec<usize>,
}

fn string_operand(op: &Operation) -> Vec<(Option<usize>, &Vec<u8>)> {
    match op.operator.as_str() {
        "Tj" | "'" => match op.operands.first() {
            Some(Object::String(b, _)) => vec![(None, b)],
            _ => vec![],
        },
        "\"" => match op.operands.get(2) {
            Some(Object::String(b, _)) => vec![(None, b)],
            _ => vec![],
        },
        "TJ" => match op.operands.first() {
            Some(Object::Array(items)) => items
                .iter()
                .enumerate()
                .filter_map(|(i, o)| if let Object::String(b, _) = o { Some((Some(i), b)) } else { None })
                .collect(),
            _ => vec![],
        },
        _ => vec![],
    }
}

fn is_show(op: &str) -> bool {
    matches!(op, "Tj" | "TJ" | "'" | "\"")
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        c
    } else {
        let mut l = c.to_lowercase();
        match (l.next(), l.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    }
}

/// Outcome of editing one page's operations.
struct PageEdit {
    replaced: usize,
    /// Fonts (labels) that carry text on this page but cannot be edited.
    blocked: Vec<String>,
}

fn edit_ops(
    ops: &mut Vec<Operation>,
    fonts: &BTreeMap<Vec<u8>, FontInfo>,
    old: &[char],
    new: &str,
    case_sensitive: bool,
    limit: usize,
) -> Result<PageEdit, String> {
    // 1. chains of consecutive show operators in one font
    let mut chains: Vec<Chain> = Vec::new();
    let mut cur_font: Option<Vec<u8>> = None;
    let mut open: Option<usize> = None;
    let mut blocked: BTreeSet<String> = BTreeSet::new();
    for (i, op) in ops.iter().enumerate() {
        let name = op.operator.as_str();
        if name == "Tf" {
            if let Some(Object::Name(n)) = op.operands.first() {
                cur_font = Some(n.clone());
            }
            open = None;
        } else if is_show(name) {
            let Some(f) = cur_font.clone() else { continue };
            if let Some(fi) = fonts.get(&f) {
                if fi.unsupported.is_some() {
                    blocked.insert(fi.unsupported.clone().unwrap());
                    open = None;
                    continue;
                }
            } else {
                open = None;
                continue;
            }
            // ' and " move to the next line first: they start a chain.
            let starts = name == "'" || name == "\"";
            match open {
                Some(ci) if !starts => chains[ci].ops.push(i),
                _ => {
                    chains.push(Chain { font: f, ops: vec![i] });
                    open = Some(chains.len() - 1);
                }
            }
        } else {
            open = None;
        }
    }

    // 2. per font: which chars/bytes may be used for a replacement
    let mut used: HashMap<Vec<u8>, BTreeSet<u8>> = HashMap::new();
    for ch in &chains {
        for &oi in &ch.ops {
            for (_, b) in string_operand(&ops[oi]) {
                used.entry(ch.font.clone()).or_default().extend(b.iter().copied());
            }
        }
    }

    // 3. match, per chain
    let mut deleted: HashSet<Loc> = HashSet::new();
    let mut inserts: HashMap<Loc, Vec<u8>> = HashMap::new();
    let mut replaced = 0usize;
    for ch in &chains {
        if replaced >= limit {
            break;
        }
        let fi = &fonts[&ch.font];
        let mut atoms: Vec<Atom> = Vec::new();
        for &oi in &ch.ops {
            let op = &ops[oi];
            match op.operator.as_str() {
                "TJ" => {
                    if let Some(Object::Array(items)) = op.operands.first() {
                        for (ii, it) in items.iter().enumerate() {
                            match it {
                                Object::String(b, _) => {
                                    for (pos, &byte) in b.iter().enumerate() {
                                        let c = fi.to_char[byte as usize].unwrap_or('\u{FFFF}');
                                        atoms.push(Atom { ch: c, loc: Loc::Byte { op: oi, item: Some(ii), pos } });
                                    }
                                }
                                n => {
                                    if num(n).is_some_and(|v| v < -200.0) {
                                        atoms.push(Atom { ch: ' ', loc: Loc::Gap { op: oi, item: ii } });
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    for (_, b) in string_operand(op) {
                        for (pos, &byte) in b.iter().enumerate() {
                            let c = fi.to_char[byte as usize].unwrap_or('\u{FFFF}');
                            atoms.push(Atom { ch: c, loc: Loc::Byte { op: oi, item: None, pos } });
                        }
                    }
                }
            }
        }
        let hay: Vec<char> = atoms.iter().map(|a| fold(a.ch, case_sensitive)).collect();
        let needle: Vec<char> = old.iter().map(|&c| fold(c, case_sensitive)).collect();
        let mut i = 0;
        while i + needle.len() <= hay.len() && replaced < limit {
            if hay[i..i + needle.len()] == needle[..] {
                let span = &atoms[i..i + needle.len()];
                let first_real = span.iter().find(|a| matches!(a.loc, Loc::Byte { .. }));
                let Some(first) = first_real else {
                    i += 1;
                    continue;
                };
                // encode the replacement in this font
                let mut bytes = Vec::with_capacity(new.len());
                for c in new.chars() {
                    let allowed = used.get(&ch.font);
                    let code = (0..=255u8).find(|&b| {
                        fi.to_char[b as usize] == Some(c)
                            && (!fi.restricted || allowed.is_some_and(|s| s.contains(&b)))
                    });
                    match code {
                        Some(b) => bytes.push(b),
                        None => {
                            return Err(format!(
                                "the replacement character '{}' (U+{:04X}) cannot be shown by font {}: {}",
                                c,
                                c as u32,
                                fi.label,
                                if fi.restricted {
                                    "this is a subset font, which only contains the glyphs already used on the page"
                                } else {
                                    "it is not in the font's encoding"
                                }
                            ))
                        }
                    }
                }
                for a in span {
                    deleted.insert(a.loc);
                }
                inserts.insert(first.loc, bytes);
                replaced += 1;
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }

    // 4. apply
    if replaced > 0 {
        let touched: BTreeSet<usize> = deleted
            .iter()
            .map(|l| match l {
                Loc::Byte { op, .. } | Loc::Gap { op, .. } => *op,
            })
            .collect();
        for oi in touched {
            let rebuild = |op: usize, item: Option<usize>, b: &[u8]| -> Vec<u8> {
                let mut out = Vec::with_capacity(b.len());
                for (pos, &byte) in b.iter().enumerate() {
                    let loc = Loc::Byte { op, item, pos };
                    if let Some(ins) = inserts.get(&loc) {
                        out.extend_from_slice(ins);
                    }
                    if !deleted.contains(&loc) {
                        out.push(byte);
                    }
                }
                out
            };
            let name = ops[oi].operator.clone();
            match name.as_str() {
                "TJ" => {
                    if let Some(Object::Array(items)) = ops[oi].operands.first().cloned() {
                        let mut out = Vec::new();
                        for (ii, it) in items.into_iter().enumerate() {
                            match it {
                                Object::String(b, fmt) => {
                                    let nb = rebuild(oi, Some(ii), &b);
                                    if !nb.is_empty() {
                                        out.push(Object::String(nb, fmt));
                                    }
                                }
                                other => {
                                    if !deleted.contains(&Loc::Gap { op: oi, item: ii }) {
                                        out.push(other);
                                    }
                                }
                            }
                        }
                        ops[oi].operands[0] = Object::Array(out);
                    }
                }
                _ => {
                    let idx = if name == "\"" { 2 } else { 0 };
                    if let Some(Object::String(b, fmt)) = ops[oi].operands.get(idx).cloned() {
                        let nb = rebuild(oi, None, &b);
                        ops[oi].operands[idx] = Object::String(nb, fmt);
                    }
                }
            }
        }
    }
    Ok(PageEdit { replaced, blocked: blocked.into_iter().collect() })
}

/// Replace `old` with `new` in the text of an existing PDF. `page` limits
/// the edit to one page, `count` to the first N replacements (in page then
/// content-stream order); `None` means all. Returns the edited file.
///
/// Errors (rather than a quietly unchanged file) when nothing was
/// replaced, naming the reason where one can be found: composite font,
/// text only in a Form XObject, or text split by positioning operators.
pub fn replace_text(
    bytes: &[u8],
    old: &str,
    new: &str,
    page: Option<u32>,
    count: Option<usize>,
    case_sensitive: bool,
) -> Result<Vec<u8>, String> {
    if old.is_empty() {
        return Err("replace_text: the text to find is empty".into());
    }
    let mut doc = load(bytes)?;
    let pages = doc.get_pages();
    let targets: Vec<(u32, ObjectId)> = match page {
        Some(p) => match pages.get(&p) {
            Some(&id) => vec![(p, id)],
            None => {
                return Err(format!(
                    "replace_text: no page {p} -- this document has {}",
                    describe_pages(&pages.keys().copied().collect::<Vec<_>>())
                ))
            }
        },
        None => pages.iter().map(|(&n, &id)| (n, id)).collect(),
    };
    let old_chars: Vec<char> = old.chars().collect();
    let limit = count.unwrap_or(usize::MAX);
    let mut total = 0usize;
    let mut blocked: BTreeSet<String> = BTreeSet::new();
    let mut edits: Vec<(ObjectId, Vec<u8>)> = Vec::new();
    for (_, pid) in &targets {
        if total >= limit {
            break;
        }
        let fonts_raw = doc.get_page_fonts(*pid).map_err(|e| e.to_string())?;
        let fonts: BTreeMap<Vec<u8>, FontInfo> =
            fonts_raw.iter().map(|(n, d)| (n.clone(), font_info(&doc, n, d))).collect();
        let data = doc
            .get_page_content_with_limit(*pid, crate::max_content_bytes())
            .map_err(|e| crate::content_error("replace_text", &e))?;
        let mut content = Content::decode(&data).map_err(|e| format!("replace_text: cannot parse page content: {e}"))?;
        let res = edit_ops(&mut content.operations, &fonts, &old_chars, new, case_sensitive, limit - total)
            .map_err(|e| format!("replace_text: {e}"))?;
        blocked.extend(res.blocked);
        if res.replaced > 0 {
            total += res.replaced;
            let enc = content.encode().map_err(|e| format!("replace_text: cannot re-encode content: {e}"))?;
            edits.push((*pid, enc));
        }
    }
    if total == 0 {
        // Say WHY, using the reader's own view of the text.
        let seen = {
            let nums: Vec<u32> = targets.iter().map(|t| t.0).collect();
            extract_text(bytes, Some(&nums)).unwrap_or_default()
        };
        let in_text = if case_sensitive { seen.contains(old) } else { seen.to_lowercase().contains(&old.to_lowercase()) };
        let scope = match page {
            Some(p) => format!("on page {p}"),
            None => "in the document".to_string(),
        };
        return Err(if !blocked.is_empty() {
            let why = blocked.into_iter().collect::<Vec<_>>().join("; ");
            if in_text {
                format!("replace_text: \"{old}\" is shown {scope} but not in an editable font -- {why}")
            } else {
                format!("replace_text: \"{old}\" was not found {scope} in editable text; some text uses fonts that cannot be edited -- {why}")
            }
        } else if in_text {
            format!(
                "replace_text: \"{old}\" appears in the extracted text {scope} but not as contiguous text in one \
                 font's strings (it may be split by a positioning/colour operator, drawn from a Form XObject, \
                 or built from ligatures), so it cannot be replaced safely"
            )
        } else {
            format!("replace_text: \"{old}\" was not found {scope}")
        });
    }
    for (pid, enc) in edits {
        doc.change_page_content(pid, enc).map_err(|e| format!("replace_text: {e}"))?;
    }
    save(&mut doc)
}

// ------------------------------------------------------------------ add_text

const WIN_ANSI_HIGH: [(char, u8); 27] = [
    ('\u{20AC}', 0x80), ('\u{201A}', 0x82), ('\u{0192}', 0x83), ('\u{201E}', 0x84), ('\u{2026}', 0x85),
    ('\u{2020}', 0x86), ('\u{2021}', 0x87), ('\u{02C6}', 0x88), ('\u{2030}', 0x89), ('\u{0160}', 0x8A),
    ('\u{2039}', 0x8B), ('\u{0152}', 0x8C), ('\u{017D}', 0x8E), ('\u{2018}', 0x91), ('\u{2019}', 0x92),
    ('\u{201C}', 0x93), ('\u{201D}', 0x94), ('\u{2022}', 0x95), ('\u{2013}', 0x96), ('\u{2014}', 0x97),
    ('\u{02DC}', 0x98), ('\u{2122}', 0x99), ('\u{0161}', 0x9A), ('\u{203A}', 0x9B), ('\u{0153}', 0x9C),
    ('\u{017E}', 0x9E), ('\u{0178}', 0x9F),
];

fn win_ansi(c: char) -> Option<u8> {
    let u = c as u32;
    if (0x20..0x7F).contains(&u) || (0xA0..=0xFF).contains(&u) {
        return Some(u as u8);
    }
    WIN_ANSI_HIGH.iter().find(|(ch, _)| *ch == c).map(|&(_, b)| b)
}

const STANDARD_FONTS: [&str; 12] = [
    "Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique", "Times-Roman", "Times-Bold",
    "Times-Italic", "Times-BoldItalic", "Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique",
];

/// Stamp `text` onto page `page` with its baseline starting at `(x, y)`
/// (PDF user space, points, origin bottom-left of the MediaBox as written
/// in the file; `/Rotate` and `/CropBox` are not compensated). `font` is
/// one of the twelve standard text fonts (default `Helvetica`); the text
/// is WinAnsi-encoded, so characters outside Western European + the usual
/// punctuation are an error. The page's existing content is wrapped in
/// `q`/`Q` so its graphics state cannot leak into the new text.
pub fn add_text(
    bytes: &[u8],
    page: u32,
    x: f64,
    y: f64,
    text: &str,
    size: f64,
    color: [f64; 3],
    font: &str,
) -> Result<Vec<u8>, String> {
    if !(size > 0.0 && size.is_finite()) {
        return Err(format!("add_text: size must be a positive number, got {size}"));
    }
    if !STANDARD_FONTS.contains(&font) {
        return Err(format!("add_text: font `{font}` is not one of {}", STANDARD_FONTS.join(", ")));
    }
    let mut enc = Vec::new();
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            return Err("add_text: text must be a single line (call add_text once per line)".into());
        }
        match win_ansi(c) {
            Some(b) => enc.push(b),
            None => return Err(format!("add_text: character '{}' (U+{:04X}) is not in the WinAnsi encoding of the standard fonts", c, c as u32)),
        }
    }
    let mut doc = load(bytes)?;
    let page_id = require_page(&doc, page).map_err(|e| format!("add_text: {e}"))?;
    bake_inherited_attrs(&mut doc, page_id);

    // resources -> a direct dictionary owned by this page, plus our font
    let mut resources = {
        let d = doc.get_dictionary(page_id).map_err(|e| e.to_string())?;
        match d.get(b"Resources") {
            Ok(Object::Dictionary(r)) => r.clone(),
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).map(|r| r.clone()).unwrap_or_default(),
            _ => Dictionary::new(),
        }
    };
    let mut fonts = match resources.get(b"Font") {
        Ok(Object::Dictionary(f)) => f.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).map(|f| f.clone()).unwrap_or_default(),
        _ => Dictionary::new(),
    };
    let mut n = 1;
    let fname = loop {
        let cand = format!("QuText{n}");
        if !fonts.has(cand.as_bytes()) {
            break cand;
        }
        n += 1;
    };
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => Object::Name(font.as_bytes().to_vec()),
        "Encoding" => "WinAnsiEncoding",
    });
    fonts.set(fname.as_bytes().to_vec(), Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));

    let mut esc = Vec::new();
    for b in enc {
        if matches!(b, b'(' | b')' | b'\\') {
            esc.push(b'\\');
        }
        esc.push(b);
    }
    let mut body = format!(
        "Q\nq\nBT\n/{fname} {} Tf\n{} {} {} rg\n{} {} Td\n(",
        fmt_num(size),
        fmt_num(color[0]),
        fmt_num(color[1]),
        fmt_num(color[2]),
        fmt_num(x),
        fmt_num(y)
    )
    .into_bytes();
    body.extend_from_slice(&esc);
    body.extend_from_slice(b") Tj\nET\nQ\n");
    let head_id = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
    let tail_id = doc.add_object(Stream::new(dictionary! {}, body));

    let mut contents: Vec<Object> = vec![Object::Reference(head_id)];
    match doc.get_dictionary(page_id).ok().and_then(|d| d.get(b"Contents").ok()).cloned() {
        Some(Object::Array(a)) => contents.extend(a),
        Some(Object::Reference(id)) => match doc.get_object(id) {
            Ok(Object::Array(a)) => contents.extend(a.clone()),
            _ => contents.push(Object::Reference(id)),
        },
        _ => {}
    }
    contents.push(Object::Reference(tail_id));
    let d = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
    d.set("Resources", Object::Dictionary(resources));
    d.set("Contents", Object::Array(contents));
    save(&mut doc)
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}
