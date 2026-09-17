//! Font embedding for the PDF backend.
//!
//! # Why this exists
//!
//! `render_pdf` used PDF's built-in base-14 Helvetica, referenced by name.
//! That needs no font data at all, which is why it was the right first
//! move — but it means a figure asks for Latin Modern and gets Helvetica,
//! so the PDF and the SVG of the same figure are set in different
//! typefaces. For a figure headed into a paper that is not a cosmetic
//! difference; it is the wrong output.
//!
//! Embedding needs three things the base-14 path never did: the font's own
//! advance widths (a viewer cannot lay out text it has no metrics for), a
//! font descriptor, and the font program itself. All three come out of the
//! SFNT tables that both `.ttf` and `.otf` files are built from, so one
//! parser covers every face Qu ships.
//!
//! # Scope, stated plainly
//!
//! - **Subsetting is the caller's job.** `Face::data` is whatever bytes the
//!   caller hands over; `render_pdf` hands over a `font_subset::subset` of
//!   the face cut down to the characters the content stream actually draws.
//!   Nothing here depends on that — a whole font still embeds correctly,
//!   which is what happens when the subsetter declines.
//! - **Simple fonts, WinAnsi.** Codes 32..=255 through `WinAnsiEncoding`,
//!   which covers Latin-1 and the handful of typographic characters Qu's
//!   labels use. A CID font would be needed for scripts beyond that.
//! - **Variable fonts embed as their default instance**, which is what
//!   every current viewer renders and what the SVG backend already gets.

use std::collections::HashMap;
use std::fmt::Write as _;

/// Everything the PDF writer needs to describe an embedded face.
#[derive(Debug, Clone)]
pub struct FontMetrics {
    /// Design units per em, from `head`. Widths are reported in these
    /// units and PDF wants 1000ths of an em, so every width is scaled by
    /// `1000 / units_per_em` — the scale is NOT always 1000 (Latin Modern
    /// uses 1000, Inter and Source Serif use 2048).
    pub units_per_em: f64,
    pub ascent: f64,
    pub descent: f64,
    pub cap_height: f64,
    pub italic_angle: f64,
    /// `[xMin, yMin, xMax, yMax]`, already scaled to 1000/em.
    pub bbox: [f64; 4],
    /// Advance width per Unicode scalar, scaled to 1000/em.
    widths: HashMap<u32, f64>,
    /// Width used for a character the font has no glyph for.
    pub missing_width: f64,
    /// `true` when the font program is CFF (an `.otf`), which PDF wants in
    /// a `/FontFile3` rather than a `/FontFile2`.
    pub is_cff: bool,
}

impl FontMetrics {
    /// Advance width of `ch` in 1000ths of an em.
    pub fn width_of(&self, ch: char) -> f64 {
        self.widths.get(&(ch as u32)).copied().unwrap_or(self.missing_width)
    }

    /// Total advance of a run, in 1000ths of an em. The PDF writer needs
    /// this to place anchored text, and it replaces the hardcoded Helvetica
    /// AFM table that anchoring used to estimate from — which was wrong for
    /// every face except Helvetica.
    pub fn text_width(&self, text: &str) -> f64 {
        text.chars().map(|c| self.width_of(c)).sum()
    }
}

// ---------------------------------------------------------------- SFNT

fn be_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

fn be_i16(b: &[u8], off: usize) -> Option<i16> {
    Some(i16::from_be_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

fn be_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(off)?,
        *b.get(off + 1)?,
        *b.get(off + 2)?,
        *b.get(off + 3)?,
    ]))
}

/// Offsets and lengths of every table in an SFNT container.
fn table_directory(data: &[u8]) -> Option<HashMap<[u8; 4], (usize, usize)>> {
    // `0x00010000` is a TrueType outline font, `OTTO` a CFF one. `true` is
    // the old Apple spelling and shows up in the wild often enough to be
    // worth accepting.
    let tag = be_u32(data, 0)?;
    if tag != 0x0001_0000 && tag != 0x4F54_544F && tag != 0x7472_7565 {
        return None;
    }
    let num_tables = be_u16(data, 4)? as usize;
    let mut out = HashMap::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let name = [*data.get(rec)?, *data.get(rec + 1)?, *data.get(rec + 2)?, *data.get(rec + 3)?];
        let off = be_u32(data, rec + 8)? as usize;
        let len = be_u32(data, rec + 12)? as usize;
        if off <= data.len() {
            out.insert(name, (off, len));
        }
    }
    Some(out)
}

/// Unicode scalar -> glyph id, from a `cmap` subtable.
///
/// Only format 4 (BMP, segment-mapped) and format 12 (full range) are read.
/// Between them they cover every font Qu ships and effectively every modern
/// font; the older byte-encoding formats are not worth carrying.
fn parse_cmap(data: &[u8], off: usize) -> Option<HashMap<u32, u16>> {
    let n = be_u16(data, off + 2)? as usize;
    // Prefer a full-repertoire (3,10) table, then BMP (3,1), then (0,x).
    let mut best: Option<usize> = None;
    let mut best_score = -1i32;
    for i in 0..n {
        let rec = off + 4 + i * 8;
        let plat = be_u16(data, rec)?;
        let enc = be_u16(data, rec + 2)?;
        let sub = off + be_u32(data, rec + 4)? as usize;
        let score = match (plat, enc) {
            (3, 10) => 4,
            (3, 1) => 3,
            (0, _) => 2,
            (3, 0) => 1,
            _ => 0,
        };
        if score > best_score {
            best_score = score;
            best = Some(sub);
        }
    }
    let sub = best?;
    match be_u16(data, sub)? {
        4 => parse_cmap4(data, sub),
        12 => parse_cmap12(data, sub),
        _ => None,
    }
}

fn parse_cmap4(data: &[u8], sub: usize) -> Option<HashMap<u32, u16>> {
    let seg_x2 = be_u16(data, sub + 6)? as usize;
    let segs = seg_x2 / 2;
    let ends = sub + 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    let mut map = HashMap::new();
    for s in 0..segs {
        let end = be_u16(data, ends + s * 2)?;
        let start = be_u16(data, starts + s * 2)?;
        let delta = be_u16(data, deltas + s * 2)?;
        let range_off = be_u16(data, ranges + s * 2)?;
        if start > end {
            continue;
        }
        for c in start..=end {
            // 0xFFFF is the required terminating segment, not a character.
            if c == 0xFFFF {
                continue;
            }
            let gid = if range_off == 0 {
                c.wrapping_add(delta)
            } else {
                let idx = ranges + s * 2 + range_off as usize + 2 * (c - start) as usize;
                let g = be_u16(data, idx)?;
                if g == 0 {
                    continue;
                }
                g.wrapping_add(delta)
            };
            if gid != 0 {
                map.insert(c as u32, gid);
            }
        }
    }
    Some(map)
}

fn parse_cmap12(data: &[u8], sub: usize) -> Option<HashMap<u32, u16>> {
    let n_groups = be_u32(data, sub + 12)? as usize;
    let mut map = HashMap::new();
    for g in 0..n_groups {
        let rec = sub + 16 + g * 12;
        let start = be_u32(data, rec)?;
        let end = be_u32(data, rec + 4)?;
        let start_gid = be_u32(data, rec + 8)?;
        // A pathological font could claim an enormous range; cap the work.
        if end < start || end - start > 0x1_0000 {
            continue;
        }
        for c in start..=end {
            let gid = start_gid + (c - start);
            if gid <= u16::MAX as u32 {
                map.insert(c, gid as u16);
            }
        }
    }
    Some(map)
}

/// Advance widths per glyph id, in font design units.
fn parse_hmtx(data: &[u8], hmtx: usize, hmtx_len: usize, num_h_metrics: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(num_h_metrics);
    for i in 0..num_h_metrics {
        let off = hmtx + i * 4;
        if off + 2 > hmtx + hmtx_len {
            break;
        }
        match be_u16(data, off) {
            Some(w) => out.push(w),
            None => break,
        }
    }
    out
}

/// Read every metric the PDF writer needs out of an SFNT font.
///
/// Returns `None` for anything it cannot read rather than guessing —
/// the caller falls back to the base-14 path, which is worse typography
/// but always renders.
pub fn parse(data: &[u8]) -> Option<FontMetrics> {
    let dir = table_directory(data)?;
    let (head_off, _) = *dir.get(b"head")?;
    let units_per_em = be_u16(data, head_off + 18)? as f64;
    if units_per_em <= 0.0 {
        return None;
    }
    let scale = 1000.0 / units_per_em;

    let bbox = [
        be_i16(data, head_off + 36)? as f64 * scale,
        be_i16(data, head_off + 38)? as f64 * scale,
        be_i16(data, head_off + 40)? as f64 * scale,
        be_i16(data, head_off + 42)? as f64 * scale,
    ];

    let (hhea_off, _) = *dir.get(b"hhea")?;
    let ascent = be_i16(data, hhea_off + 4)? as f64 * scale;
    let descent = be_i16(data, hhea_off + 6)? as f64 * scale;
    let num_h_metrics = be_u16(data, hhea_off + 34)? as usize;

    let (hmtx_off, hmtx_len) = *dir.get(b"hmtx")?;
    let advances = parse_hmtx(data, hmtx_off, hmtx_len, num_h_metrics);
    if advances.is_empty() {
        return None;
    }

    let (cmap_off, _) = *dir.get(b"cmap")?;
    let cmap = parse_cmap(data, cmap_off)?;

    // `post` carries the italic angle as a 16.16 fixed-point number.
    let italic_angle = dir
        .get(b"post")
        .and_then(|(off, _)| be_u32(data, off + 4))
        .map(|raw| (raw as i32) as f64 / 65536.0)
        .unwrap_or(0.0);

    // `OS/2` version 2 and later carry a real cap height; earlier versions
    // do not, and 0.7 * ascent is the conventional stand-in.
    let cap_height = dir
        .get(b"OS/2")
        .and_then(|(off, _)| {
            let version = be_u16(data, *off)?;
            if version >= 2 {
                Some(be_i16(data, off + 88)? as f64 * scale)
            } else {
                None
            }
        })
        .filter(|h| *h > 0.0)
        .unwrap_or(ascent * 0.7);

    let last_advance = *advances.last().unwrap_or(&0);
    let mut widths = HashMap::with_capacity(cmap.len());
    for (&ch, &gid) in &cmap {
        // A glyph past the end of `hmtx` inherits the last advance -- that
        // is the format's own rule for monospaced tails, not a fallback.
        let adv = advances.get(gid as usize).copied().unwrap_or(last_advance);
        widths.insert(ch, adv as f64 * scale);
    }

    Some(FontMetrics {
        units_per_em,
        ascent,
        descent,
        cap_height,
        italic_angle,
        bbox,
        widths,
        missing_width: last_advance as f64 * scale,
        is_cff: dir.contains_key(b"CFF "),
    })
}

// ------------------------------------------------------- PDF objects

/// How a face's glyphs are addressed from the content stream.
pub enum Encoding {
    /// `WinAnsiEncoding` over codes 32..=255: one byte per character, and
    /// the viewer maps the byte to a glyph through that standard table.
    /// Right for ordinary text and wrong for everything else — the table
    /// has 224 slots and not one of them is Greek.
    WinAnsi,
    /// `Type0` / `Identity-H`: the string is 2-byte big-endian GLYPH IDS,
    /// with no encoding table in between.
    ///
    /// This is what lets a figure set `ν₀T` rather than spelling it "nuT".
    /// A simple font cannot name a glyph WinAnsi has no code for, and the
    /// alternative — a `/Differences` array of glyph names — depends on
    /// the names in the font's own charset matching what is written, which
    /// is a guess about a font rather than a fact about it. A glyph id is
    /// neither named nor encoded; it is the glyph.
    ///
    /// Carries `(glyph id, width in 1000ths of an em)` for every glyph the
    /// figure draws, since a CID font has no `/Widths` array to fall back
    /// on and would otherwise set every one of them on the default width.
    Identity(Vec<(u16, f64)>),
}

/// One face to embed, with the name it will carry in the PDF.
pub struct Face<'a> {
    /// The font program, written into the PDF verbatim. `Cow` because the
    /// caller normally passes a freshly built subset (owned) but may pass
    /// the bundled bytes straight through (borrowed) when the subsetter
    /// declined — see the module note.
    pub data: std::borrow::Cow<'a, [u8]>,
    pub metrics: FontMetrics,
    /// The `/BaseFont` name. Sanitised on the way out: PDF names cannot
    /// contain spaces or delimiters, and a viewer given one will either
    /// reject the file or silently substitute a font, which is exactly the
    /// failure this whole module exists to remove.
    pub base_name: String,
    /// How the content stream addresses this face's glyphs.
    pub encoding: Encoding,
}

fn sanitize_pdf_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if cleaned.is_empty() {
        "QuEmbeddedFace".to_string()
    } else {
        cleaned
    }
}

/// Character for a `WinAnsiEncoding` code.
///
/// WinAnsi is Latin-1 except for 0x80..=0x9F, where Latin-1 has unused
/// control codes and Windows put typographic characters instead. Those are
/// the quotes, dashes and the bullet a real axis label reaches for, so the
/// range is worth spelling out rather than treating as Latin-1 and getting
/// the widths wrong.
fn winansi_char(code: u8) -> Option<char> {
    const HIGH: [u16; 32] = [
        0x20AC, 0x0000, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x0000, 0x017D, 0x0000, 0x0000, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
        0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x0000, 0x017E, 0x0178,
    ];
    match code {
        0x80..=0x9F => {
            let cp = HIGH[(code - 0x80) as usize];
            if cp == 0 { None } else { char::from_u32(cp as u32) }
        }
        _ => Some(code as char),
    }
}

/// The first and last codes a simple font's `/Widths` array covers.
const FIRST_CHAR: u8 = 32;
const LAST_CHAR: u8 = 255;

/// Build the `/Font` resource dictionary and the PDF objects behind it.
///
/// A WinAnsi face costs three objects (font dictionary, descriptor, program);
/// an `Identity` face costs four, the extra one being the CID font the
/// `Type0` dictionary descends into. Returns the resource-dict text (to drop
/// into the page's `/Resources`) and the object bodies, where `objects[i]`
/// belongs to object id `first_id + i`.
pub fn embed(faces: &[Face], first_id: u32) -> (String, Vec<Vec<u8>>) {
    let mut resources = String::from("<< ");
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // Ids are handed out as the objects are built, because a face's cost
    // depends on its encoding -- the old `first_id + i * 3` silently
    // overlapped the moment one face needed four.
    let mut next_id = first_id;

    for (i, face) in faces.iter().enumerate() {
        let name = sanitize_pdf_name(&face.base_name);
        let m = &face.metrics;

        let font_id = next_id;
        next_id += 1;
        let cid_id = match face.encoding {
            Encoding::Identity(_) => {
                let id = next_id;
                next_id += 1;
                Some(id)
            }
            Encoding::WinAnsi => None,
        };
        let desc_id = next_id;
        let file_id = next_id + 1;
        next_id += 2;

        resources.push_str(&format!("/F{} {font_id} 0 R ", i + 1));

        // CFF outlines (an .otf) go in a /FontFile3; TrueType outlines in a
        // /FontFile2. Getting this backwards produces a file that opens and
        // renders nothing, so it is driven off the actual table present.
        let (subtype, file_key, file_extra) = if m.is_cff {
            ("/Type1", "/FontFile3", " /Subtype /OpenType")
        } else {
            ("/TrueType", "/FontFile2", "")
        };

        match &face.encoding {
            Encoding::WinAnsi => {
                // Widths for every code in the encoding, in 1000ths of an
                // em. A code the font has no glyph for gets the missing
                // width rather than 0 -- a zero would stack the following
                // glyphs on top of it.
                let mut widths = String::new();
                for code in FIRST_CHAR..=LAST_CHAR {
                    let w =
                        winansi_char(code).map(|c| m.width_of(c)).unwrap_or(m.missing_width);
                    let _ = write!(widths, "{w:.0} ");
                }
                objects.push(
                    format!(
                        "{font_id} 0 obj\n<< /Type /Font /Subtype {subtype} /BaseFont /{name} \
                         /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} /Widths [{}] \
                         /FontDescriptor {desc_id} 0 R /Encoding /WinAnsiEncoding >>\nendobj\n",
                        widths.trim_end()
                    )
                    .into_bytes(),
                );
            }
            Encoding::Identity(widths) => {
                let cid_id = cid_id.expect("an Identity face allocates a CID font id");
                // A CID font's widths are sparse: `/W [ gid [w] ... ]`,
                // with `/DW` covering everything not listed. Listing only
                // the glyphs drawn keeps this proportional to the figure
                // rather than to the font.
                let mut w = String::new();
                for (gid, width) in widths {
                    let _ = write!(w, "{gid} [{width:.0}] ");
                }
                // CIDFontType0 descends from CFF outlines, CIDFontType2
                // from TrueType. `/CIDToGIDMap /Identity` says CID == glyph
                // id, which is what `Identity-H` already assumes and what
                // makes `font_subset::glyph_id` the right lookup.
                let (cid_subtype, cid_extra) = if m.is_cff {
                    ("/CIDFontType0", "")
                } else {
                    ("/CIDFontType2", " /CIDToGIDMap /Identity")
                };
                objects.push(
                    format!(
                        "{font_id} 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /{name} \
                         /Encoding /Identity-H /DescendantFonts [{cid_id} 0 R] >>\nendobj\n"
                    )
                    .into_bytes(),
                );
                objects.push(
                    format!(
                        "{cid_id} 0 obj\n<< /Type /Font /Subtype {cid_subtype} /BaseFont /{name} \
                         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                         /FontDescriptor {desc_id} 0 R /DW 1000 /W [{}]{cid_extra} >>\nendobj\n",
                        w.trim_end()
                    )
                    .into_bytes(),
                );
            }
        }

        // Flags bit 3 (value 4) is "symbolic", bit 6 (32) "nonsymbolic";
        // a text face with a standard encoding is the latter. Bit 2 (2) is
        // serif. StemV has no accessible source in the SFNT tables and 80
        // is the conventional stand-in for a regular weight -- viewers use
        // it only for synthetic substitution, which an embedded font never
        // needs.
        // An Identity face is reached by glyph id, not through a standard
        // encoding, which is exactly what "symbolic" (bit 3, value 4) means
        // -- claiming nonsymbolic there invites a viewer to re-map the
        // glyphs through a table that does not apply.
        let flags = match face.encoding {
            Encoding::WinAnsi => 32,
            Encoding::Identity(_) => 4,
        };
        objects.push(
            format!(
                "{desc_id} 0 obj\n<< /Type /FontDescriptor /FontName /{name} /Flags {flags} \
                 /FontBBox [{:.0} {:.0} {:.0} {:.0}] /ItalicAngle {:.1} /Ascent {:.0} \
                 /Descent {:.0} /CapHeight {:.0} /StemV 80 /MissingWidth {:.0} \
                 {file_key} {file_id} 0 R >>\nendobj\n",
                m.bbox[0], m.bbox[1], m.bbox[2], m.bbox[3],
                m.italic_angle, m.ascent, m.descent, m.cap_height, m.missing_width
            )
            .into_bytes(),
        );

        // The font program itself. /Length1 is required for a /FontFile2
        // and is the uncompressed length of the TrueType program; a CFF
        // stream carries /Subtype instead.
        let mut stream = Vec::new();
        let header = if m.is_cff {
            format!("{file_id} 0 obj\n<< /Length {}{file_extra} >>\nstream\n", face.data.len())
        } else {
            format!(
                "{file_id} 0 obj\n<< /Length {} /Length1 {} >>\nstream\n",
                face.data.len(),
                face.data.len()
            )
        };
        stream.extend_from_slice(header.as_bytes());
        stream.extend_from_slice(&face.data);
        stream.extend_from_slice(b"\nendstream\nendobj\n");
        objects.push(stream);
    }

    resources.push_str(">>");
    (resources, objects)
}

#[cfg(test)]
mod tests {
    use super::*;


    // The faces the plotting backend actually embeds.
    static INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
    static LMR: &[u8] = include_bytes!("../assets/fonts/LatinModernRoman-Regular.otf");
    static LMR_BOLD: &[u8] = include_bytes!("../assets/fonts/LatinModernRoman-Bold.otf");
    static SOURCE_SERIF: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-Variable.ttf");

    #[test]
    fn parses_every_font_qu_ships() {
        for (name, bytes) in [
            ("Inter", INTER),
            ("LatinModernRoman", LMR),
            ("LatinModernRoman-Bold", LMR_BOLD),
            ("SourceSerif4", SOURCE_SERIF),
        ] {
            let m = parse(bytes).unwrap_or_else(|| panic!("{name} failed to parse"));
            assert!(m.units_per_em > 0.0, "{name}: no unitsPerEm");
            assert!(m.ascent > 0.0, "{name}: ascent {}", m.ascent);
            assert!(m.descent < 0.0, "{name}: descent should be negative, got {}", m.descent);
            assert!(m.cap_height > 0.0, "{name}: cap height {}", m.cap_height);
            // Every face must at least know the ASCII a figure label uses.
            for ch in ['0', '9', 'A', 'z', '.', '-', '(', ')'] {
                assert!(m.width_of(ch) > 0.0, "{name}: no width for {ch:?}");
            }
        }
    }

    #[test]
    fn otf_is_flagged_as_cff_and_ttf_is_not() {
        // This decides /FontFile3 vs /FontFile2, so getting it backwards
        // produces a PDF no viewer can render.
        assert!(parse(LMR).unwrap().is_cff, "an .otf carries a CFF table");
        assert!(!parse(INTER).unwrap().is_cff, "a .ttf carries glyf outlines");
    }

    #[test]
    fn widths_are_scaled_to_a_1000_unit_em_not_left_in_design_units() {
        // Inter and Source Serif are 2048/em, Latin Modern is 1000/em --
        // the exact reason the scale cannot be assumed. A space that came
        // back as 512 instead of 250 would set every anchored label adrift.
        let inter = parse(INTER).unwrap();
        assert_eq!(inter.units_per_em, 2048.0);
        let lmr = parse(LMR).unwrap();
        assert_eq!(lmr.units_per_em, 1000.0);

        for (name, m) in [("Inter", &inter), ("LatinModernRoman", &lmr)] {
            let space = m.width_of(' ');
            assert!(
                (100.0..=600.0).contains(&space),
                "{name}: a space of {space}/1000 em is not a plausible scaled width"
            );
            let cap_m = m.width_of('M');
            assert!(
                (500.0..=1200.0).contains(&cap_m),
                "{name}: an 'M' of {cap_m}/1000 em is not plausible"
            );
        }
    }

    #[test]
    fn text_width_sums_its_characters() {
        let m = parse(LMR).unwrap();
        let sum = m.width_of('A') + m.width_of('B') + m.width_of('C');
        assert!((m.text_width("ABC") - sum).abs() < 1e-9);
        assert_eq!(m.text_width(""), 0.0);
    }

    #[test]
    fn proportional_faces_do_not_report_one_width_for_everything() {
        // A monospaced result would mean the cmap or hmtx lookup collapsed,
        // which still "works" but silently mis-places every anchored label.
        let m = parse(LMR).unwrap();
        assert!(
            m.width_of('i') < m.width_of('m'),
            "expected proportional widths: i={} m={}",
            m.width_of('i'),
            m.width_of('m')
        );
    }

    #[test]
    fn garbage_input_is_rejected_rather_than_guessed_at() {
        assert!(parse(b"").is_none());
        assert!(parse(b"not a font at all").is_none());
        // A plausible header with nothing behind it must not panic.
        assert!(parse(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x05]).is_none());
        // Truncated mid-table-directory.
        let mut truncated = INTER[..200].to_vec();
        truncated.truncate(200);
        assert!(parse(&truncated).is_none());
    }
}
