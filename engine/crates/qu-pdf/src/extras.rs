//! Text lines, embedded images and an HTML view of a PDF (v0.4.7).
//!
//! Everything here reads the PDF's own objects through `lopdf`; nothing
//! needs PDFium (`pdf.render` is the one function that does).
//!
//! # What "lines" means, and what it does not
//!
//! PDF stores "draw this string at this matrix", not lines. `page_lines`
//! walks the page's content stream, keeps the text matrix and the current
//! transformation matrix (`BT`/`ET`/`Td`/`TD`/`Tm`/`T*`/`'`/`"`/`cm`/`q`/`Q`/`TL`/`Tf`),
//! records where each text-showing operator starts, and groups the pieces
//! whose baselines agree (within about half the font size) into one line,
//! left to right. What it does NOT do: it has no glyph widths, so a string
//! is placed at its start only; it does not descend into Form XObjects
//! (the same limit as `extract_text`, and it errors in the same case); it
//! ignores `/Rotate`; and on a multi-column page the lines of the columns
//! are interleaved top to bottom, because reading order is not in the file.

use super::*;
use lopdf::content::Content;
use lopdf::{Dictionary, Encoding};

// --------------------------------------------------------------- text lines

/// One recovered line of text on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub page: u32,
    pub text: String,
    /// Baseline height in user space (larger is higher on the page).
    pub y: f64,
    /// Effective font size in user space (font size times the matrices).
    pub size: f64,
}

#[derive(Clone, Copy)]
struct M([f64; 6]);

const IDENT: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

/// `a` applied first, then `b` (PDF's row-vector convention).
fn mul(a: &M, b: &M) -> M {
    let (a, b) = (&a.0, &b.0);
    M([
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ])
}

fn translate(tx: f64, ty: f64) -> M {
    M([1.0, 0.0, 0.0, 1.0, tx, ty])
}

fn num_of(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

struct Run {
    x: f64,
    y: f64,
    size: f64,
    text: String,
}

fn page_runs(doc: &Document, page_id: ObjectId) -> Result<Vec<Run>, String> {
    let fonts = doc.get_page_fonts(page_id).map_err(|e| e.to_string())?;
    let mut encodings: std::collections::BTreeMap<Vec<u8>, Encoding> = Default::default();
    for (name, font) in fonts {
        let enc = font.get_font_encoding(doc).map_err(|e| {
            format!("font /{} has an encoding this reader cannot use: {e}", String::from_utf8_lossy(&name))
        })?;
        encodings.insert(name, enc);
    }
    let data = doc.get_page_content(page_id);
    let content = Content::decode(&data).map_err(|e| e.to_string())?;

    let mut runs: Vec<Run> = Vec::new();
    let mut ctm = IDENT;
    let mut stack: Vec<M> = Vec::new();
    let (mut tm, mut tlm) = (IDENT, IDENT);
    let mut leading = 0.0f64;
    let mut font_size = 1.0f64;
    let mut enc: Option<&Encoding> = None;
    let mut cur: Option<Run> = None;

    fn flush(cur: &mut Option<Run>, runs: &mut Vec<Run>) {
        if let Some(r) = cur.take() {
            if !r.text.trim().is_empty() {
                runs.push(r);
            }
        }
    }

    for op in &content.operations {
        let args = &op.operands;
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                if let Some(m) = stack.pop() {
                    ctm = m;
                }
            }
            "cm" if args.len() == 6 => {
                let v: Vec<f64> = args.iter().filter_map(num_of).collect();
                if v.len() == 6 {
                    ctm = mul(&M([v[0], v[1], v[2], v[3], v[4], v[5]]), &ctm);
                }
            }
            "BT" => {
                flush(&mut cur, &mut runs);
                tm = IDENT;
                tlm = IDENT;
            }
            "ET" => flush(&mut cur, &mut runs),
            "Tf" => {
                if let Some(Object::Name(n)) = args.first() {
                    enc = encodings.get(n);
                }
                if let Some(s) = args.get(1).and_then(num_of) {
                    font_size = s;
                }
            }
            "TL" => {
                if let Some(v) = args.first().and_then(num_of) {
                    leading = v;
                }
            }
            "Td" | "TD" => {
                flush(&mut cur, &mut runs);
                let v: Vec<f64> = args.iter().filter_map(num_of).collect();
                if v.len() == 2 {
                    if op.operator == "TD" {
                        leading = -v[1];
                    }
                    tlm = mul(&translate(v[0], v[1]), &tlm);
                    tm = tlm;
                }
            }
            "Tm" => {
                flush(&mut cur, &mut runs);
                let v: Vec<f64> = args.iter().filter_map(num_of).collect();
                if v.len() == 6 {
                    tlm = M([v[0], v[1], v[2], v[3], v[4], v[5]]);
                    tm = tlm;
                }
            }
            "T*" => {
                flush(&mut cur, &mut runs);
                tlm = mul(&translate(0.0, -leading), &tlm);
                tm = tlm;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if op.operator == "'" || op.operator == "\"" {
                    flush(&mut cur, &mut runs);
                    tlm = mul(&translate(0.0, -leading), &tlm);
                    tm = tlm;
                }
                let Some(enc) = enc else { continue };
                // The text pieces of this operator, with a space where a TJ
                // adjustment is a word-sized gap.
                let mut piece = String::new();
                let decode = |bytes: &[u8], piece: &mut String| -> Result<(), String> {
                    piece.push_str(&Document::decode_text(enc, bytes).map_err(|e| e.to_string())?);
                    Ok(())
                };
                match op.operator.as_str() {
                    "TJ" => {
                        if let Some(Object::Array(items)) = args.first() {
                            for it in items {
                                match it {
                                    Object::String(b, _) => decode(b, &mut piece)?,
                                    n => {
                                        if let Some(adj) = num_of(n) {
                                            if adj < -200.0 && !piece.ends_with(' ') {
                                                piece.push(' ');
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "\"" => {
                        if let Some(Object::String(b, _)) = args.get(2) {
                            decode(b, &mut piece)?;
                        }
                    }
                    _ => {
                        if let Some(Object::String(b, _)) = args.first() {
                            decode(b, &mut piece)?;
                        }
                    }
                }
                match cur.as_mut() {
                    Some(r) => r.text.push_str(&piece),
                    None => {
                        let full = mul(&tm, &ctm).0;
                        cur = Some(Run {
                            x: full[4],
                            y: full[5],
                            size: font_size.abs() * (full[2] * full[2] + full[3] * full[3]).sqrt(),
                            text: piece,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut runs);
    Ok(runs)
}

fn group_runs(mut runs: Vec<Run>) -> Vec<(String, f64, f64)> {
    runs.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut lines: Vec<(f64, Vec<Run>)> = Vec::new();
    for r in runs {
        let tol = 0.5 * r.size.max(1.0);
        match lines.last_mut() {
            Some((ref_y, rs)) if (*ref_y - r.y).abs() <= tol => rs.push(r),
            _ => lines.push((r.y, vec![r])),
        }
    }
    lines
        .into_iter()
        .map(|(y, mut rs)| {
            rs.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            let size = rs.iter().map(|r| r.size).fold(0.0, f64::max);
            let mut text = String::new();
            for r in &rs {
                let piece = r.text.trim();
                if !text.is_empty() && !piece.is_empty() {
                    text.push(' ');
                }
                text.push_str(piece);
            }
            (text, y, size)
        })
        .collect()
}

fn wanted_pages(doc: &Document, pages: Option<&[u32]>, who: &str) -> Result<Vec<u32>, String> {
    let ids = doc.get_pages();
    let available: Vec<u32> = ids.keys().copied().collect();
    match pages {
        None => Ok(available),
        Some(ps) => {
            for p in ps {
                if !available.contains(p) {
                    return Err(format!("{who}: no page {p} -- this document has {}", describe_pages(&available)));
                }
            }
            Ok(ps.to_vec())
        }
    }
}

/// The recovered lines of the requested pages (all pages when `None`), top
/// to bottom within each page. Same error contract as `extract_text`: a
/// page that reaches fonts but yields no text is an error, not an empty
/// answer.
pub fn page_lines(bytes: &[u8], pages: Option<&[u32]>) -> Result<Vec<TextLine>, String> {
    let doc = load(bytes)?;
    let ids = doc.get_pages();
    let wanted = wanted_pages(&doc, pages, "extract_text")?;
    let mut out = Vec::new();
    for p in wanted {
        let id = ids[&p];
        let runs = page_runs(&doc, id).map_err(|e| format!("extract_text: page {p}: {e}"))?;
        if runs.is_empty() {
            let fonts = reachable_fonts(&doc, id);
            if fonts > 0 {
                return Err(format!(
                    "extract_text: page {p} produced no text, but can reach {fonts} embedded \
                     font(s) -- lines could not be recovered for this page (it is drawn inside a \
                     Form XObject, or uses fonts without a usable encoding)"
                ));
            }
        }
        for (text, y, size) in group_runs(runs) {
            out.push(TextLine { page: p, text, y, size });
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- normalizing

fn expand_char(c: char, out: &mut String) {
    match c {
        '\u{FB00}' => out.push_str("ff"),
        '\u{FB01}' => out.push_str("fi"),
        '\u{FB02}' => out.push_str("fl"),
        '\u{FB03}' => out.push_str("ffi"),
        '\u{FB04}' => out.push_str("ffl"),
        '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
        '\u{00A0}' | '\u{2007}' | '\u{202F}' => out.push(' '),
        other => out.push(other),
    }
}

/// Ligatures expanded, no-break spaces made plain, whitespace runs
/// collapsed to one space, empty lines dropped, and a word split by a
/// line-end hyphen re-joined ("Fi-" / "nally" becomes "Finally"; the two
/// lines become one). The hyphen is removed only when it follows a letter
/// and the next line starts with a lowercase letter, so "well-" / "Known"
/// and "3-" / "4" are left alone.
pub fn normalize_lines(lines: Vec<String>) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();
    for l in lines {
        let mut s = String::with_capacity(l.len());
        let trimmed = l.trim_end();
        let ends_soft = trimmed.ends_with('\u{00AD}');
        for c in l.chars() {
            if c == '\u{00AD}' {
                continue;
            }
            expand_char(c, &mut s);
        }
        let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if ends_soft && !s.is_empty() {
            s.push('-');
        }
        if !s.is_empty() {
            cleaned.push(s);
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cleaned.len() {
        let mut cur = cleaned[i].clone();
        i += 1;
        while i < cleaned.len() && ends_with_letter_hyphen(&cur) && starts_lower(&cleaned[i]) {
            cur.pop();
            cur.push_str(&cleaned[i]);
            i += 1;
        }
        out.push(cur);
    }
    out
}

fn ends_with_letter_hyphen(s: &str) -> bool {
    let mut it = s.chars().rev();
    it.next() == Some('-') && it.next().is_some_and(|c| c.is_alphabetic())
}

fn starts_lower(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_lowercase())
}

/// `normalize_lines` applied to a flat string of newline-separated lines.
pub fn normalize_text(text: &str) -> String {
    normalize_lines(text.split('\n').map(str::to_string).collect()).join("\n")
}

// ------------------------------------------------------------------- images

/// One embedded image XObject, as `pdf.images` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub page: u32,
    /// 1-based position within the page's list.
    pub index: usize,
    /// Resource name; an image inside a Form XObject is `Form/Image`.
    pub name: String,
    pub width: i64,
    pub height: i64,
    pub bits: i64,
    pub colorspace: String,
    /// Filters in order joined by `,`, or `none`.
    pub filter: String,
    /// Size of the stored (still encoded) stream.
    pub bytes: usize,
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        other => other,
    }
}

fn dict_of<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match deref(doc, o) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn collect_images(doc: &Document, res: &Dictionary, prefix: &str, depth: usize, out: &mut Vec<(String, ObjectId)>) {
    let Some(xo) = res.get(b"XObject").ok().and_then(|o| dict_of(doc, o)) else {
        return;
    };
    for (name, v) in xo.iter() {
        let Object::Reference(id) = v else { continue };
        let Ok(Object::Stream(s)) = doc.get_object(*id) else { continue };
        let full = format!("{prefix}{}", String::from_utf8_lossy(name));
        match s.dict.get(b"Subtype").and_then(|o| o.as_name()) {
            Ok(b"Image") => {
                if !out.iter().any(|(n, i)| *n == full && i == id) {
                    out.push((full, *id));
                }
            }
            Ok(b"Form") if depth == 0 => {
                if let Some(inner) = s.dict.get(b"Resources").ok().and_then(|o| dict_of(doc, o)) {
                    collect_images(doc, inner, &format!("{full}/"), depth + 1, out);
                }
            }
            _ => {}
        }
    }
}

fn page_image_ids(doc: &Document, page_id: ObjectId) -> Vec<(String, ObjectId)> {
    let mut out = Vec::new();
    let Ok((own, inherited)) = doc.get_page_resources(page_id) else {
        return out;
    };
    if let Some(d) = own {
        collect_images(doc, d, "", 0, &mut out);
    }
    for id in inherited {
        if let Ok(d) = doc.get_dictionary(id) {
            collect_images(doc, d, "", 0, &mut out);
        }
    }
    out
}

fn name_string(doc: &Document, o: &Object) -> String {
    match deref(doc, o) {
        Object::Name(n) => String::from_utf8_lossy(n).into_owned(),
        Object::Array(a) => a.first().map(|f| name_string(doc, f)).unwrap_or_else(|| "none".into()),
        _ => "none".into(),
    }
}

fn filter_names(s: &Stream) -> Vec<String> {
    s.filters().map(|v| v.into_iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect()).unwrap_or_default()
}

fn int_key(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<i64> {
    d.get(key).ok().and_then(|o| deref(doc, o).as_i64().ok())
}

/// The embedded images of the requested pages (all pages when `None`).
/// Inline images (`BI ... EI`) are not listed.
pub fn images(bytes: &[u8], pages: Option<&[u32]>) -> Result<Vec<ImageInfo>, String> {
    let doc = load(bytes)?;
    let ids = doc.get_pages();
    let wanted = wanted_pages(&doc, pages, "images")?;
    let mut out = Vec::new();
    for p in wanted {
        for (i, (name, id)) in page_image_ids(&doc, ids[&p]).into_iter().enumerate() {
            let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
            let is_mask = matches!(s.dict.get(b"ImageMask").map(|o| deref(&doc, o)), Ok(Object::Boolean(true)));
            let filters = filter_names(s);
            out.push(ImageInfo {
                page: p,
                index: i + 1,
                name,
                width: int_key(&doc, &s.dict, b"Width").unwrap_or(0),
                height: int_key(&doc, &s.dict, b"Height").unwrap_or(0),
                bits: if is_mask { 1 } else { int_key(&doc, &s.dict, b"BitsPerComponent").unwrap_or(8) },
                colorspace: if is_mask {
                    "ImageMask".into()
                } else {
                    s.dict.get(b"ColorSpace").map(|o| name_string(&doc, o)).unwrap_or_else(|_| "none".into())
                },
                filter: if filters.is_empty() { "none".into() } else { filters.join(",") },
                bytes: s.content.len(),
            });
        }
    }
    Ok(out)
}

/// A decoded image: a JPEG stream to hand to a JPEG decoder as is, or
/// 8-bit RGB pixels, row-major, top row first.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractedImage {
    Jpeg(Vec<u8>),
    Rgb { width: usize, height: usize, rgb: Vec<u8> },
}

enum Cs {
    Gray,
    Rgb,
    Indexed { palette: Vec<[u8; 3]> },
}

fn resolve_cs(doc: &Document, o: &Object, depth: usize) -> Result<Cs, String> {
    if depth > 4 {
        return Err("colour space nests too deeply".into());
    }
    match deref(doc, o) {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Ok(Cs::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(Cs::Rgb),
            other => Err(format!("colour space {} is not supported", String::from_utf8_lossy(other))),
        },
        Object::Array(a) => {
            let head = a.first().map(|f| name_string(doc, f)).unwrap_or_default();
            match head.as_str() {
                "CalGray" => Ok(Cs::Gray),
                "CalRGB" => Ok(Cs::Rgb),
                "ICCBased" => {
                    let n = a
                        .get(1)
                        .and_then(|s| dict_of(doc, s))
                        .and_then(|d| int_key(doc, d, b"N"))
                        .unwrap_or(0);
                    match n {
                        1 => Ok(Cs::Gray),
                        3 => Ok(Cs::Rgb),
                        other => Err(format!("an ICCBased colour space with {other} components is not supported")),
                    }
                }
                "Indexed" | "I" => {
                    let base = resolve_cs(doc, a.get(1).ok_or("Indexed colour space without a base")?, depth + 1)?;
                    let hival = a.get(2).and_then(|h| deref(doc, h).as_i64().ok()).ok_or("Indexed colour space without hival")?;
                    let lookup: Vec<u8> = match a.get(3).map(|l| deref(doc, l)) {
                        Some(Object::String(b, _)) => b.clone(),
                        Some(Object::Stream(s)) => s.decompressed_content().map_err(|e| e.to_string())?,
                        _ => return Err("Indexed colour space without a lookup table".into()),
                    };
                    let n = match base {
                        Cs::Gray => 1,
                        Cs::Rgb => 3,
                        Cs::Indexed { .. } => return Err("an Indexed colour space cannot be based on another".into()),
                    };
                    let mut palette = Vec::new();
                    for i in 0..=(hival.max(0) as usize) {
                        let e = lookup.get(i * n..(i + 1) * n).unwrap_or(&[0; 3][..n]);
                        palette.push(if n == 1 { [e[0]; 3] } else { [e[0], e[1], e[2]] });
                    }
                    Ok(Cs::Indexed { palette })
                }
                other => Err(format!("colour space {other} is not supported")),
            }
        }
        _ => Err("colour space is not a name or an array".into()),
    }
}

/// Unpack `bits`-deep samples, `ncomp` per pixel, rows padded to a byte.
/// `scale` stretches sub-byte values to 0..255 (grey), else raw (indexed).
fn unpack(data: &[u8], w: usize, h: usize, ncomp: usize, bits: usize, scale: bool) -> Result<Vec<u8>, String> {
    let stride = (w * ncomp * bits).div_ceil(8);
    let need = stride * h;
    if data.len() < need {
        return Err(format!("the image data is truncated: {} bytes where {need} are needed", data.len()));
    }
    let per_row = w * ncomp;
    let mut out = Vec::with_capacity(per_row * h);
    for y in 0..h {
        let row = &data[y * stride..(y + 1) * stride];
        match bits {
            8 => out.extend_from_slice(&row[..per_row]),
            16 => out.extend((0..per_row).map(|i| row[i * 2])),
            1 | 2 | 4 => {
                let max = (1u32 << bits) - 1;
                for i in 0..per_row {
                    let bit = i * bits;
                    let v = (row[bit / 8] >> (8 - bits - bit % 8)) as u32 & max;
                    out.push(if scale { (v * 255 / max) as u8 } else { v as u8 });
                }
            }
            other => return Err(format!("{other} bits per component is not supported")),
        }
    }
    Ok(out)
}

/// Decode image number `index` (from `images`) of `page`.
pub fn extract_image(bytes: &[u8], page: u32, index: usize) -> Result<ExtractedImage, String> {
    let doc = load(bytes)?;
    let ids = doc.get_pages();
    wanted_pages(&doc, Some(&[page]), "extract_image")?;
    let list = page_image_ids(&doc, ids[&page]);
    if index == 0 || index > list.len() {
        return Err(format!(
            "extract_image: page {page} has {} image(s); index {index} is out of range (indexes count from 1, as pdf.images lists them)",
            list.len()
        ));
    }
    let (name, id) = &list[index - 1];
    let Ok(Object::Stream(s)) = doc.get_object(*id) else {
        return Err(format!("extract_image: image {name} is not a stream"));
    };
    let filters = filter_names(s);
    for f in &filters {
        match f.as_str() {
            "JBIG2Decode" | "CCITTFaxDecode" | "CCF" | "JPXDecode" => {
                return Err(format!(
                    "extract_image: image {name} on page {page} uses the {f} filter, which this build cannot decode"
                ))
            }
            _ => {}
        }
    }
    if filters.iter().any(|f| f == "DCTDecode" || f == "DCT") {
        if filters.len() != 1 {
            return Err(format!(
                "extract_image: image {name} chains DCTDecode with other filters ({}), which is not supported",
                filters.join(",")
            ));
        }
        return Ok(ExtractedImage::Jpeg(s.content.clone()));
    }
    let w = int_key(&doc, &s.dict, b"Width").filter(|&v| v > 0).ok_or_else(|| format!("extract_image: image {name} has no usable /Width"))? as usize;
    let h = int_key(&doc, &s.dict, b"Height").filter(|&v| v > 0).ok_or_else(|| format!("extract_image: image {name} has no usable /Height"))? as usize;
    if w.checked_mul(h).is_none_or(|n| n > 300_000_000) {
        return Err(format!("extract_image: image {name} is {w}x{h}, too large to decode"));
    }
    let data = s
        .decompressed_content()
        .map_err(|e| format!("extract_image: image {name}: cannot decode its stream: {e}"))?;
    let is_mask = matches!(s.dict.get(b"ImageMask").map(|o| deref(&doc, o)), Ok(Object::Boolean(true)));
    let bits = if is_mask { 1 } else { int_key(&doc, &s.dict, b"BitsPerComponent").unwrap_or(8) as usize };
    let decode: Vec<f64> = s
        .dict
        .get(b"Decode")
        .ok()
        .and_then(|o| deref(&doc, o).as_array().ok())
        .map(|a| a.iter().filter_map(num_of).collect())
        .unwrap_or_default();
    let inverted = decode.len() >= 2 && decode[0] == 1.0 && decode[1] == 0.0;
    let cs = if is_mask {
        Cs::Gray
    } else {
        let o = s.dict.get(b"ColorSpace").map_err(|_| format!("extract_image: image {name} has no /ColorSpace"))?;
        resolve_cs(&doc, o, 0).map_err(|e| format!("extract_image: image {name}: {e}"))?
    };
    let fail = |e: String| format!("extract_image: image {name}: {e}");
    let mut rgb = Vec::with_capacity(w * h * 3);
    match cs {
        Cs::Gray => {
            let g = unpack(&data, w, h, 1, bits, true).map_err(fail)?;
            for v in g {
                let v = if inverted { 255 - v } else { v };
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
        Cs::Rgb => {
            let default = decode.is_empty() || decode == [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
            if !default {
                return Err(fail("a non-default /Decode array on an RGB image is not supported".into()));
            }
            rgb = unpack(&data, w, h, 3, bits, true).map_err(fail)?;
        }
        Cs::Indexed { palette } => {
            let idx = unpack(&data, w, h, 1, bits, false).map_err(fail)?;
            for i in idx {
                rgb.extend_from_slice(palette.get(i as usize).unwrap_or(&[0, 0, 0]));
            }
        }
    }
    Ok(ExtractedImage::Rgb { width: w, height: h, rgb })
}

// --------------------------------------------------------------------- HTML

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            other => o.push(other),
        }
    }
    o
}

pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// An RGB8 PNG, zlib-compressed.
fn png_bytes(w: usize, h: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for row in rgb.chunks(w * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let idat = z.finish().map_err(|e| e.to_string())?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut cb = kind.to_vec();
        cb.extend_from_slice(body);
        out.extend_from_slice(&cb);
        out.extend_from_slice(&crc32(&cb).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &idat);
    chunk(b"IEND", &[]);
    Ok(out)
}

/// A simple HTML fragment for the requested pages: one
/// `<section class="page" data-page="N">` per page holding a `<p>` per
/// recovered paragraph. A new paragraph starts where the gap between two
/// lines is over 1.6 times the font size; the lines of a paragraph are
/// joined with spaces and line-end hyphenation is repaired. Fonts, sizes,
/// colours, columns and positions are not reproduced. With `images`, each
/// page's decodable images follow its text as `<img>` data URIs (JPEG kept
/// as is, other images re-encoded as PNG); the images' positions on the
/// page are not recovered, and a soft mask (transparency) is dropped.
pub fn to_html(bytes: &[u8], pages: Option<&[u32]>, with_images: bool) -> Result<String, String> {
    let doc = load(bytes)?;
    let wanted = wanted_pages(&doc, pages, "to_html")?;
    let lines = page_lines(bytes, pages).map_err(|e| e.replacen("extract_text:", "to_html:", 1))?;
    let mut html = String::new();
    for p in wanted {
        html.push_str(&format!("<section class=\"page\" data-page=\"{p}\">\n"));
        let mut para: Vec<String> = Vec::new();
        let mut prev: Option<(f64, f64)> = None;
        let emit = |para: &mut Vec<String>, html: &mut String| {
            if !para.is_empty() {
                let joined = normalize_lines(std::mem::take(para)).join(" ");
                html.push_str(&format!("<p>{}</p>\n", esc(&joined)));
            }
        };
        for l in lines.iter().filter(|l| l.page == p) {
            let size = if l.size > 0.0 { l.size } else { 10.0 };
            if let Some((py, ps)) = prev {
                if py - l.y > 1.6 * size.max(ps) {
                    emit(&mut para, &mut html);
                }
            }
            para.push(l.text.clone());
            prev = Some((l.y, size));
        }
        emit(&mut para, &mut html);
        if with_images {
            for info in images(bytes, Some(&[p]))? {
                let alt = esc(&info.name);
                match extract_image(bytes, p, info.index) {
                    Ok(ExtractedImage::Jpeg(j)) => html.push_str(&format!(
                        "<img alt=\"{alt}\" data-index=\"{}\" src=\"data:image/jpeg;base64,{}\">\n",
                        info.index,
                        base64(&j)
                    )),
                    Ok(ExtractedImage::Rgb { width, height, rgb }) => html.push_str(&format!(
                        "<img alt=\"{alt}\" data-index=\"{}\" src=\"data:image/png;base64,{}\">\n",
                        info.index,
                        base64(&png_bytes(width, height, &rgb)?)
                    )),
                    Err(e) => html.push_str(&format!("<!-- image {} skipped: {} -->\n", info.index, esc(&e.replace("--", "- -")))),
                }
            }
        }
        html.push_str("</section>\n");
    }
    Ok(html)
}
