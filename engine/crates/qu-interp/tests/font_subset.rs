//! What an embedded figure font must never do.
//!
//! A print-ready theme inlines the font so the figure survives a machine
//! that doesn't have it installed. It used to inline the WHOLE file: 1.18 MB
//! of base64 Inter to draw thirty characters of tick labels. These check
//! that the subset that replaced it is both small AND complete -- a font
//! missing one glyph the figure draws is a blank box in a published
//! figure, which is far worse than a large file.
//!
//! Property assertions on real rendered output, same as `figures.rs`: the
//! font is decoded back out of the finished SVG and parsed independently
//! here, rather than trusting the subsetter's own view of what it emitted.

use std::collections::BTreeSet;

use qu_interp::plotting::{self, Figure};
use qu_interp::Interp;

fn figure_of(src: &str) -> Figure {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("script failed: {e}\n{src}"));
    it.figure.clone()
}

/// A figure with every kind of label a real one has: a title (bold), axis
/// labels carrying math macros, a legend, and two decades of tick numbers.
const FIGURE: &str = "x = linspace(0, 10, 50)\n\
     plot(x, sin(x), label=\"sin wave\")\n\
     plot(x, cos(x), label=\"cos wave\")\n\
     title(\"Damped response\")\n\
     xlabel(\"Time $\\\\mu$s, $\\\\times 10^{3}$\")\n\
     ylabel(\"Amplitude (V)\")\n\
     legend()";

fn embedded(theme: &str) -> String {
    let fig = figure_of(&format!("theme(\"{theme}\")\n{FIGURE}"));
    plotting::render_svg(&fig, fig.width, fig.height, true)
}

// ------------------------------------------------------------ minimal reader

fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Every `data:` font payload in the SVG, decoded, in document order.
fn embedded_fonts(svg: &str) -> Vec<Vec<u8>> {
    svg.split("base64,")
        .skip(1)
        .map(|chunk| b64_decode(&chunk[..chunk.find(')').expect("unterminated data: url")]))
        .collect()
}

fn u16at(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}

fn u32at(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// (offset, length) of every table, by tag.
fn tables(font: &[u8]) -> Vec<(String, usize, usize)> {
    (0..u16at(font, 4) as usize)
        .map(|i| {
            let r = 12 + i * 16;
            (
                String::from_utf8_lossy(&font[r..r + 4]).to_string(),
                u32at(font, r + 8) as usize,
                u32at(font, r + 12) as usize,
            )
        })
        .collect()
}

fn table<'a>(font: &'a [u8], tag: &str) -> Option<&'a [u8]> {
    tables(font).into_iter().find(|t| t.0 == tag).map(|(_, o, l)| &font[o..o + l])
}

/// A deliberately independent `cmap` format 4 reader -- if the subsetter's
/// own writer and this disagree, one of them is wrong, which is the point.
fn glyph_for(font: &[u8], c: char) -> u16 {
    let cmap = table(font, "cmap").expect("no cmap in the embedded font");
    let n = u16at(cmap, 2) as usize;
    let sub = (0..n)
        .map(|i| u32at(cmap, 4 + i * 8 + 4) as usize)
        .find(|&off| u16at(cmap, off) == 4)
        .expect("no format 4 subtable");
    let t = &cmap[sub..];
    let c = c as u32;
    if c > 0xFFFF {
        return 0;
    }
    let c = c as u16;
    let segs = u16at(t, 6) as usize / 2;
    let (ends, starts, deltas, ranges) = (14, 14 + segs * 2 + 2, 14 + segs * 4 + 2, 14 + segs * 6 + 2);
    for i in 0..segs {
        if u16at(t, ends + i * 2) < c {
            continue;
        }
        let start = u16at(t, starts + i * 2);
        if start > c {
            return 0;
        }
        let delta = u16at(t, deltas + i * 2);
        let range = u16at(t, ranges + i * 2);
        if range == 0 {
            return c.wrapping_add(delta);
        }
        let g = u16at(t, ranges + i * 2 + range as usize + (c - start) as usize * 2);
        return if g == 0 { 0 } else { g.wrapping_add(delta) };
    }
    0
}

/// Walks a CFF INDEX at `p`, returning each entry's length and the offset
/// just past the INDEX.
fn cff_index_lengths(cff: &[u8], p: usize) -> (Vec<usize>, usize) {
    let count = u16at(cff, p) as usize;
    if count == 0 {
        return (Vec::new(), p + 2);
    }
    let off_size = cff[p + 2] as usize;
    let read = |i: usize| -> usize {
        (0..off_size).fold(0usize, |v, k| (v << 8) | cff[p + 3 + i * off_size + k] as usize)
    };
    let offsets: Vec<usize> = (0..=count).map(read).collect();
    let data = p + 3 + (count + 1) * off_size - 1;
    ((0..count).map(|i| offsets[i + 1] - offsets[i]).collect(), data + offsets[count])
}

/// Reads the integer operands of one Top/Private DICT operator.
fn cff_dict_ints(dict: &[u8], want: u16) -> Option<Vec<i32>> {
    let (mut i, mut ints) = (0usize, Vec::new());
    while i < dict.len() {
        let b = dict[i];
        match b {
            0..=21 => {
                let op =
                    if b == 12 { i += 2; 0x0c00 | dict[i - 1] as u16 } else { i += 1; b as u16 };
                if op == want {
                    return Some(ints);
                }
                ints.clear();
            }
            28 => { ints.push(i16::from_be_bytes([dict[i + 1], dict[i + 2]]) as i32); i += 3 }
            29 => {
                ints.push(i32::from_be_bytes([
                    dict[i + 1], dict[i + 2], dict[i + 3], dict[i + 4],
                ]));
                i += 5
            }
            30 => {
                i += 1;
                while i < dict.len() && dict[i] & 0x0F != 0x0F && dict[i] >> 4 != 0x0F {
                    i += 1;
                }
                i += 1;
            }
            32..=246 => { ints.push(b as i32 - 139); i += 1 }
            247..=250 => { ints.push((b as i32 - 247) * 256 + dict[i + 1] as i32 + 108); i += 2 }
            251..=254 => { ints.push(-(b as i32 - 251) * 256 - dict[i + 1] as i32 - 108); i += 2 }
            _ => i += 1,
        }
    }
    None
}

/// Every charstring and subroutine length in a CFF font, as
/// `(what, lengths)`.
fn cff_charstring_lengths(cff: &[u8]) -> Vec<(&'static str, Vec<usize>)> {
    let hdr = cff[2] as usize;
    let (_, e1) = cff_index_lengths(cff, hdr);
    let top_start = e1;
    let (top_lens, e2) = cff_index_lengths(cff, top_start);
    // The Top DICT INDEX's single entry, located the same way the reader does.
    let off_size = cff[top_start + 2] as usize;
    let top_data = top_start + 3 + 2 * off_size - 1 + 1;
    let top = &cff[top_data..top_data + top_lens[0]];
    let (_, e3) = cff_index_lengths(cff, e2);
    let (gsubrs, _) = cff_index_lengths(cff, e3);

    let charstrings_off = cff_dict_ints(top, 17).unwrap().pop().unwrap() as usize;
    let (charstrings, _) = cff_index_lengths(cff, charstrings_off);

    let mut out = vec![("CharStrings", charstrings), ("global subrs", gsubrs)];
    let private = cff_dict_ints(top, 18).unwrap();
    let (psize, poff) = (private[0] as usize, private[1] as usize);
    if let Some(mut subrs) = cff_dict_ints(&cff[poff..poff + psize], 19) {
        let (locals, _) = cff_index_lengths(cff, poff + subrs.pop().unwrap() as usize);
        out.push(("local subrs", locals));
    }
    out
}

/// The characters the SVG actually paints, read out of its `<text>` nodes.
fn painted(svg: &str) -> BTreeSet<char> {
    let mut out = BTreeSet::new();
    for chunk in svg.split("<text").skip(1) {
        let body = &chunk[..chunk.find("</text>").unwrap_or(chunk.len())];
        let mut in_tag = true; // we start mid-tag, just after `<text`
        let mut flat = String::new();
        for ch in body.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => flat.push(ch),
                _ => {}
            }
        }
        let flat = flat
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        out.extend(flat.chars());
    }
    out
}

// ------------------------------------------------------------------- tests

#[test]
fn a_journal_figure_no_longer_embeds_a_megabyte_of_font() {
    // The bundled Inter is 876 KB, Source Serif 1.2 MB; base64 adds a third
    // on top. Any of those inlined whole lands far above this bound, so
    // this fails loudly if the subset path is ever bypassed.
    for theme in ["nature", "ieee", "publication"] {
        let svg = embedded(theme);
        assert!(svg.contains("@font-face"), "{theme}: print-ready figure embedded no font at all");
        assert!(
            svg.len() < 100_000,
            "{theme}: embedded figure is {} bytes -- the whole font is back",
            svg.len()
        );
    }
}

#[test]
fn the_embedded_font_carries_every_character_the_figure_paints() {
    // The bug this guards: `render_math_svg` turns `$\times$` into `×` and
    // `10^{3}` into `10³`, so the characters that reach the page are NOT
    // the ones the script wrote. Collect the subset from `DrawOp::Text`
    // and the figure loses its multiplication sign to a blank box.
    //
    // The bar is the ORIGINAL font's own coverage, not Unicode's: Latin
    // Modern has no U+03BC at all (only U+00B5 MICRO SIGN), so `$\mu$`
    // fell back to a system font under `theme("publication")` long before
    // subsetting existed. Subsetting must not make coverage worse; it
    // cannot be asked to make it better.
    for (theme, family) in
        [("nature", plotting::DEFAULT_FONT_FAMILY), ("publication", plotting::TIKZ_FONT_FAMILY)]
    {
        let svg = embedded(theme);
        let font = &embedded_fonts(&svg)[0];
        let (_, original, _) = plotting::embeddable_font_for(family).unwrap();
        let mut checked = 0;
        for c in painted(&svg) {
            if c == ' ' || glyph_for(original, c) == 0 {
                continue;
            }
            assert_ne!(
                glyph_for(font, c),
                0,
                "{theme}: the embedded font has no glyph for {c:?}, which the figure paints"
            );
            checked += 1;
        }
        assert!(checked > 20, "{theme}: only checked {checked} characters, expected the whole figure");
    }
}

#[test]
fn the_math_glyphs_survive_subsetting() {
    // The specific characters `render_math_svg` synthesises, spelled out so
    // a regression names them instead of failing on "some character".
    let svg = embedded("nature");
    let font = &embedded_fonts(&svg)[0];
    // The exponent of `10^{3}` is a plain `3`, raised by the `<tspan>` --
    // NOT a precomposed `\u{00B3}`. See `to_script_chars`.
    for c in ['\u{03BC}', '\u{00D7}', '3'] {
        assert_ne!(glyph_for(font, c), 0, "no glyph for {c:?} ({:#06x})", c as u32);
    }
    assert!(!painted(&svg).contains(&'\u{00B3}'), "the SVG still precomposes superscripts");
}

#[test]
fn the_variable_weight_axis_survives_subsetting() {
    // Inter and Source Serif are variable fonts and the `@font-face` claims
    // `font-weight: 100 900`. Drop `fvar` and that claim is a lie; drop
    // `gvar` and every weight draws the same outline; drop `HVAR` and a
    // 700-weight title sets on 400-weight advances.
    for family in [plotting::DEFAULT_FONT_FAMILY, plotting::PRINT_FONT_FAMILY] {
        let (name, bytes, _) = plotting::embeddable_font_for(family).unwrap();
        let chars: BTreeSet<char> = "Damped response 0123456789.".chars().collect();
        let sub = qu_interp::font_subset::subset(bytes, &chars).expect("declined to subset");
        for tag in ["fvar", "gvar", "HVAR", "avar", "STAT"] {
            assert!(table(&sub, tag).is_some(), "{name}: subset lost the {tag} table");
        }
    }
}

#[test]
fn every_bundled_face_subsets_to_a_structurally_valid_font() {
    let chars: BTreeSet<char> = "Damped response Time (s) 0123456789.-\u{03BC}\u{00D7}".chars().collect();
    for family in [
        plotting::DEFAULT_FONT_FAMILY,
        plotting::PRINT_FONT_FAMILY,
        plotting::TIKZ_FONT_FAMILY,
        plotting::ACADEMIC_SANS_FONT_FAMILY,
    ] {
        let (name, bytes, _) = plotting::embeddable_font_for(family).unwrap();
        let sub =
            qu_interp::font_subset::subset(bytes, &chars).unwrap_or_else(|| panic!("{name}: declined"));

        assert!(sub.len() < bytes.len() / 4, "{name}: subset is {} bytes, barely smaller", sub.len());
        assert_eq!(u32at(&sub, 0), u32at(bytes, 0), "{name}: sfnt version changed");

        let present: Vec<String> = tables(&sub).into_iter().map(|t| t.0).collect();
        for required in ["head", "hhea", "hmtx", "maxp", "cmap", "name", "post", "OS/2"] {
            assert!(present.iter().any(|t| t == required), "{name}: no {required} table");
        }
        // Outlines, in whichever of the two flavours this face uses.
        assert!(
            present.iter().any(|t| t == "glyf") || present.iter().any(|t| t == "CFF "),
            "{name}: subset has no outlines at all"
        );
        // Layout tables are deliberately dropped -- see the module comment.
        for dropped in ["GPOS", "GSUB", "GDEF"] {
            assert!(!present.iter().any(|t| t == dropped), "{name}: {dropped} unexpectedly kept");
        }

        for (tag, off, len) in tables(&sub) {
            assert!(off + len <= sub.len(), "{name}: {tag} runs past the end of the file");
        }
        // `head.checkSumAdjustment` makes the whole file sum to this.
        let mut sum = 0u32;
        for i in (0..sub.len()).step_by(4) {
            let mut w = [0u8; 4];
            w[..sub.len().min(i + 4) - i].copy_from_slice(&sub[i..sub.len().min(i + 4)]);
            sum = sum.wrapping_add(u32::from_be_bytes(w));
        }
        assert_eq!(sum, 0xB1B0_AFBA, "{name}: file checksum is wrong");

        // Every requested character the ORIGINAL font could draw still
        // resolves to a real glyph (Latin Modern has no U+03BC to lose).
        for c in chars.iter().filter(|c| **c != ' ' && glyph_for(bytes, **c) != 0) {
            assert_ne!(glyph_for(&sub, *c), 0, "{name}: lost the glyph for {c:?}");
        }
    }
}

#[test]
fn the_bold_face_only_carries_the_characters_set_in_bold() {
    // `theme("publication")` embeds two separately drawn faces. The bold one
    // sets the title and nothing else, so it has no business carrying the
    // tick digits.
    let svg = embedded("publication");
    let fonts = embedded_fonts(&svg);
    // Regular, bold, and -- because this figure paints Greek -- the math
    // fallback. The bold face is still the second.
    assert_eq!(fonts.len(), 3, "expected a regular, a bold and a math face");
    assert!(
        fonts[1].len() < fonts[0].len(),
        "bold face ({} bytes) should be smaller than the regular one ({} bytes)",
        fonts[1].len(),
        fonts[0].len()
    );
    // The title is "Damped response": no digits, no `V`. The regular face
    // needs both (tick numbers, and the `ylabel` "Amplitude (V)"); the bold
    // face must not have been asked for either.
    for c in ['0', 'V'] {
        assert_ne!(glyph_for(&fonts[0], c), 0, "regular face lost {c:?}, which the figure paints");
        assert_eq!(glyph_for(&fonts[1], c), 0, "bold face carries {c:?}, which it never sets");
    }
    assert_ne!(glyph_for(&fonts[1], 'D'), 0, "bold face lost the title's own first letter");
}

#[test]
fn every_math_character_is_drawable_by_an_embedded_face() {
    // The gap this pins: NONE of the four text faces covers what
    // `latex_symbol` can emit. Latin Modern has no Greek at all, so `$\mu$`
    // under `theme("publication")` used to arrive in whatever the viewer's
    // machine supplied -- visibly a different typeface, mid-word. Latin
    // Modern Math is embedded as the fallback for exactly these.
    //
    // Reads the macro table out of the source rather than restating it, so
    // a macro added later is covered by this test the day it lands.
    let src = include_str!("../src/plotting.rs");
    let body = &src[src.find("fn latex_symbol").expect("latex_symbol moved")..];
    let body = &body[..body.find("\n}").unwrap()];
    let mut chars: Vec<char> = Vec::new();
    for (i, _) in body.match_indices("=> \"\\u{") {
        let hex = &body[i + 7..];
        let hex = &hex[..hex.find('}').unwrap()];
        chars.push(char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap());
    }
    assert!(chars.len() > 50, "only found {} macros, the parse is wrong", chars.len());

    let (_, math, _) = plotting::embeddable_math_font();
    for family in [
        plotting::DEFAULT_FONT_FAMILY,
        plotting::PRINT_FONT_FAMILY,
        plotting::TIKZ_FONT_FAMILY,
        plotting::ACADEMIC_SANS_FONT_FAMILY,
    ] {
        let (name, text_face, _) = plotting::embeddable_font_for(family).unwrap();
        assert!(
            family.contains(plotting::MATH_FALLBACK_FONT_FAMILY),
            "{name}: the stack has no '{}' to fall back to",
            plotting::MATH_FALLBACK_FONT_FAMILY
        );
        for &c in &chars {
            assert!(
                glyph_for(text_face, c) != 0 || glyph_for(math, c) != 0,
                "{name}: neither it nor the math fallback can draw {c:?} ({:#06x})",
                c as u32
            );
        }
    }
}

#[test]
fn a_greek_label_embeds_the_math_face_and_a_plain_one_does_not() {
    // The fallback is not free, so it must not ride along on figures that
    // have no maths in them.
    let plain = figure_of("theme(\"publication\")\nplot([1,2,3])\nxlabel(\"Time (s)\")");
    let plain = plotting::render_svg(&plain, plain.width, plain.height, true);
    // The family name is in every stack on the root `<svg>`, so this has to
    // look for the `@font-face` rule rather than for the name.
    let declared = |svg: &str| svg.contains("font-family:'Latin Modern Math'");
    assert!(!declared(&plain), "a figure with no maths embedded the math fallback anyway");

    let greek = figure_of("theme(\"publication\")\nplot([1,2,3])\nxlabel(\"$\\\\mu$s\")");
    let greek = plotting::render_svg(&greek, greek.width, greek.height, true);
    assert!(declared(&greek), "a figure painting Greek did not embed the math fallback");
    // And the fallback really does carry the character in question.
    let math_face = embedded_fonts(&greek).pop().expect("no faces embedded");
    assert_ne!(glyph_for(&math_face, '\u{03BC}'), 0, "the embedded fallback has no mu");
}

#[test]
fn no_charstring_or_subroutine_is_left_empty() {
    // A zero-byte charstring is not a charstring: it reaches neither
    // `return` nor `endchar`. Pruning unused subroutines to zero length
    // produced a Latin Modern Math subset that Chrome rejected outright --
    // and because a rejected `@font-face` just falls back silently, the
    // figure still "looked fine" while quietly using a system font. Dropped
    // subroutines are a single `return` instead.
    let chars: BTreeSet<char> = "Damped 0123\u{03BC}\u{2211}".chars().collect();
    let faces: Vec<(&str, &[u8])> = vec![
        ("Latin Modern Roman", plotting::embeddable_font_for(plotting::TIKZ_FONT_FAMILY).unwrap().1),
        (
            "Latin Modern Sans",
            plotting::embeddable_font_for(plotting::ACADEMIC_SANS_FONT_FAMILY).unwrap().1,
        ),
        ("Latin Modern Math", plotting::embeddable_math_font().1),
    ];
    for (name, bytes) in faces {
        let sub = qu_interp::font_subset::subset(bytes, &chars).expect("declined");
        let cff = table(&sub, "CFF ").expect("no CFF table in a CFF subset");
        let mut checked = 0;
        for (what, lengths) in cff_charstring_lengths(cff) {
            // An INDEX with no entries at all is fine and normal — Latin
            // Modern Math has no global subroutines. An INDEX with an
            // entry of length zero is not.
            for (i, len) in lengths.iter().enumerate() {
                assert!(*len > 0, "{name}: {what}[{i}] is empty, which no parser accepts");
                checked += 1;
            }
        }
        assert!(checked > 100, "{name}: only checked {checked} entries, the walk is wrong");
    }
}

#[test]
fn subsetting_declines_rather_than_emitting_a_font_it_half_understood() {
    let chars: BTreeSet<char> = "abc".chars().collect();
    let (_, real, _) = plotting::embeddable_font_for(plotting::DEFAULT_FONT_FAMILY).unwrap();
    for (what, bytes) in [
        ("empty", Vec::new()),
        ("garbage", vec![0xAB; 4096]),
        ("a truncated font", real[..real.len() / 3].to_vec()),
        ("a font collection", b"ttcf\0\x01\0\0\0\0\0\x02".to_vec()),
    ] {
        assert!(
            qu_interp::font_subset::subset(&bytes, &chars).is_none(),
            "{what} should have been declined, not subsetted"
        );
    }
}

#[test]
fn a_figure_that_paints_nothing_still_produces_a_readable_font() {
    // No text at all: the subset is glyph 0 and nothing else, and must
    // still be a font the browser accepts rather than a truncated stub.
    let fig = figure_of("theme(\"nature\")\nplot([1,2,3])");
    let svg = plotting::render_svg(&fig, fig.width, fig.height, true);
    let font = &embedded_fonts(&svg)[0];
    assert!(table(font, "head").is_some(), "no head table");
    assert!(table(font, "cmap").is_some(), "no cmap table");
    for c in painted(&svg).into_iter().filter(|c| *c != ' ') {
        assert_ne!(glyph_for(font, c), 0, "no glyph for {c:?}");
    }
}

// ------------------------------------------------------------- the PDF side
//
// PDF is the format the paper actually embeds, and it embeds its faces
// TWICE -- once as /F1, once as /F2 -- so a whole font costs double there.
// These mirror the SVG tests above: the font program is cut back out of the
// finished file and parsed here, and the characters it must cover are read
// out of the content stream rather than taken from the writer's word.

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn pdf_of(theme: &str) -> Vec<u8> {
    let fig = figure_of(&format!("theme(\"{theme}\")\n{FIGURE}"));
    plotting::render_pdf(&fig, fig.width, fig.height)
}

/// Every embedded font program in a PDF, in document order — which is the
/// order `pdf_font::embed` writes them, so `[0]` is /F1 and `[1]` is /F2.
fn pdf_font_programs(pdf: &[u8]) -> Vec<Vec<u8>> {
    const MARK: &[u8] = b">>\nstream\n";
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = find(&pdf[at..], MARK) {
        let dict_end = at + rel;
        let dict_start = pdf[..dict_end]
            .windows(2)
            .rposition(|w| w == b"<<")
            .expect("a stream with no dictionary before it");
        let dict = String::from_utf8_lossy(&pdf[dict_start..dict_end]).to_string();
        let body = dict_end + MARK.len();
        // `/Length1` marks a TrueType program, `/Subtype /OpenType` a CFF
        // one; the page's own content stream carries neither.
        if dict.contains("/Length1") || dict.contains("/Subtype /OpenType") {
            let len: usize = dict
                .split("/Length ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
                .expect("a font stream with no /Length");
            out.push(pdf[body..body + len].to_vec());
        }
        at = body;
    }
    out
}

/// The characters the PDF paints, split by face, read straight out of the
/// (uncompressed) content stream. Inverts `pdf_encode_text`: `\(`, `\)`,
/// `\\`, and a three-digit octal escape for anything above 126.
fn pdf_painted(pdf: &[u8]) -> (BTreeSet<char>, BTreeSet<char>) {
    let text = String::from_utf8_lossy(pdf);
    let (mut regular, mut bold) = (BTreeSet::new(), BTreeSet::new());
    let mut in_bold = false;
    for line in text.lines() {
        if line.ends_with(" Tf") {
            in_bold = line.starts_with("/F2");
            continue;
        }
        let Some(body) = line.strip_suffix(") Tj").and_then(|l| l.strip_prefix('(')) else {
            continue;
        };
        let target = if in_bold { &mut bold } else { &mut regular };
        let b = body.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'\\' {
                target.insert(b[i] as char);
                i += 1;
            } else if b.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                let end = (i + 4).min(b.len());
                let oct = u32::from_str_radix(&body[i + 1..end], 8).expect("bad octal escape");
                target.insert(char::from_u32(oct).expect("octal escape is not a character"));
                i = end;
            } else {
                target.insert(b[i + 1] as char);
                i += 2;
            }
        }
    }
    (regular, bold)
}

#[test]
fn a_pdf_figure_no_longer_embeds_two_whole_fonts() {
    // Latin Modern is 111 KB a face and the PDF embeds two of them, which
    // was 222 KB of a 240 KB paper figure -- the picture was 8% of its own
    // file. Inter and Source Serif are ~1 MB each, so the screen themes
    // were far worse.
    for (theme, bound) in [("publication", 80_000), ("nature", 100_000), ("ieee", 80_000)] {
        let pdf = pdf_of(theme);
        let programs = pdf_font_programs(&pdf);
        // Regular, bold, and -- because `FIGURE` paints `μ` and `×` -- one
        // or both glyph-id faces. Whole, Latin Modern Math alone is 733 KB.
        assert!(
            (3..=5).contains(&programs.len()),
            "{theme}: expected regular, bold and at least one glyph face, got {}",
            programs.len()
        );
        for p in &programs {
            assert!(!p.is_empty(), "{theme}: an embedded font program is empty");
        }
        assert!(
            pdf.len() < bound,
            "{theme}: the PDF is {} bytes ({:?} of font) -- a whole face is back",
            pdf.len(),
            programs.iter().map(|p| p.len()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn greek_reaches_the_pdf_as_a_glyph_rather_than_a_spelling() {
    // `WinAnsiEncoding` has no Greek in it, so a simple font could not name
    // `ν` and the writer spelled it: the paper's regime map said "nuT"
    // where the figure meant `ν₀T`, and its colorbar said "Delta". The
    // math face is embedded as `Type0`/`Identity-H` instead, where a
    // string is glyph ids and no encoding stands in the way.
    let fig = figure_of(
        "theme(\"publication\")\n\
         plot([1,2,3])\n\
         xlabel(\"$\\\\nu_0 T$ and $\\\\sigma$\")\n\
         ylabel(\"Ceiling $\\\\Delta_{\\\\max}$\")",
    );
    let pdf = plotting::render_pdf(&fig, fig.width, fig.height);
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/Subtype /Type0"), "no Type0 font in the PDF");
    assert!(text.contains("/Encoding /Identity-H"), "the math face is not Identity-H");
    assert!(text.contains("/Subtype /CIDFontType0"), "no CID descendant font");
    // Two glyph-id faces: the TEXT face draws what it has -- Latin Modern
    // Roman covers Delta -- and the math face covers the rest. Taking
    // everything from the math face gave a radical drawn for a LaTeX
    // engine to compose with a rule of its own, which alone hangs below
    // the baseline and reads as a stray tick.
    // /F1 regular, /F2 bold, /F3 the drawn italic, then the two glyph-id
    // faces: /F4 takes what the text face has (Latin Modern Roman covers
    // Delta) and /F5 the rest. Everything from the math face gave a
    // radical drawn for a LaTeX engine to compose with a rule of its own,
    // which alone hangs below the baseline.
    assert!(text.contains("/F4 "), "no text glyph face");
    assert!(text.contains("/F5 "), "no math glyph face for the lower-case Greek");
    // The spellings must be gone, not merely joined by the glyphs.
    for spelled in ["nu", "sigma", "Delta"] {
        assert!(
            !text.contains(&format!("({spelled}") ) && !text.contains(&format!("{spelled}) Tj")),
            "the PDF still spells {spelled:?} out"
        );
    }
    // And the glyphs are actually addressed: a hex string selecting /F3.
    let glyph_runs = text.matches("/F4 ").count() + text.matches("/F5 ").count();
    assert!(glyph_runs >= 5, "expected at least five glyph runs, found {glyph_runs}");
    assert!(text.contains("> Tj"), "no hex-string run: Identity-H text is 2-byte glyph ids");

    // The math face is subset like the others -- whole it is 733 KB.
    let programs = pdf_font_programs(&pdf);
    assert_eq!(programs.len(), 5, "expected regular, bold, italic, text-glyph and math faces");
    for (i, p) in programs.iter().enumerate().skip(2) {
        assert!(p.len() < 40_000, "glyph face {i} is {} bytes -- it went in whole", p.len());
    }
}

#[test]
fn the_pdf_faces_carry_every_character_they_paint() {
    // The failure this guards is a hole in a published figure: a glyph the
    // content stream asks for that the subset does not have renders as
    // nothing at all, and a PDF -- unlike a browser -- has no fallback face
    // to quietly cover for it.
    //
    // Both outline formats are covered on purpose: `publication` is Latin
    // Modern (CFF, subset by rebuilding charstrings) and `nature` is Inter
    // (TrueType, subset by rebuilding `glyf`/`loca`).
    for (theme, family) in
        [("publication", plotting::TIKZ_FONT_FAMILY), ("nature", plotting::DEFAULT_FONT_FAMILY)]
    {
        let pdf = pdf_of(theme);
        let programs = pdf_font_programs(&pdf);
        let (regular_chars, bold_chars) = pdf_painted(&pdf);
        assert!(!bold_chars.is_empty(), "{theme}: nothing was set in bold, so /F2 is untested");

        let (_, original, _) = plotting::embeddable_font_for(family).unwrap();
        let mut checked = 0;
        for (which, font, chars) in [
            ("/F1", &programs[0], &regular_chars),
            ("/F2", &programs[1], &bold_chars),
        ] {
            for &c in chars {
                // The bar is the ORIGINAL face's coverage: subsetting must
                // not lose a glyph, and cannot be asked to add one.
                if c == ' ' || glyph_for(original, c) == 0 {
                    continue;
                }
                assert_ne!(
                    glyph_for(font, c),
                    0,
                    "{theme} {which}: no glyph for {c:?}, which the content stream paints"
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "{theme}: only checked {checked} characters");
    }
}

#[test]
fn the_pdf_bold_face_carries_only_what_is_set_in_bold() {
    // /F1 and /F2 are the same file in a family with no separate bold, so a
    // subset shared between them would silently double every figure's font
    // cost back up. The title is the only bold text a figure has.
    let pdf = pdf_of("publication");
    let programs = pdf_font_programs(&pdf);
    let (regular_chars, bold_chars) = pdf_painted(&pdf);
    assert!(
        bold_chars.len() * 2 < regular_chars.len(),
        "expected the title to be a small fraction of the figure's text, got {} vs {}",
        bold_chars.len(),
        regular_chars.len()
    );
    assert!(
        programs[1].len() < programs[0].len(),
        "the bold face ({} bytes) is not smaller than the regular one ({} bytes), so it is \
         carrying glyphs nothing sets in bold",
        programs[1].len(),
        programs[0].len()
    );
}

#[test]
fn a_pdf_face_whose_subset_is_declined_still_embeds_whole() {
    // The fallback has to stay real: `pdf_font::embed` writes whatever
    // bytes it is handed, so a declined subset must produce the original
    // font rather than an empty stream or a missing /FontFile.
    let (_, original, _) = plotting::embeddable_font_for(plotting::TIKZ_FONT_FAMILY).unwrap();
    let face = qu_interp::pdf_font::Face {
        data: std::borrow::Cow::Borrowed(original),
        metrics: qu_interp::pdf_font::parse(original).unwrap(),
        base_name: "LatinModernRoman".to_string(),
        encoding: qu_interp::pdf_font::Encoding::WinAnsi,
    };
    let (resources, objects) = qu_interp::pdf_font::embed(&[face], 5);
    assert!(resources.contains("/F1 5 0 R"));
    let program = objects.last().unwrap();
    assert!(
        program.len() > original.len(),
        "the whole font did not reach the stream: {} bytes for a {} byte face",
        program.len(),
        original.len()
    );
}
