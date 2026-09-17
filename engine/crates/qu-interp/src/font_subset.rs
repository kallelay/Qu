//! Glyph-level subsetting for the fonts `render_svg` inlines as base64
//! `@font-face` data.
//!
//! A print-ready theme embeds the actual font so the figure renders the
//! same on a machine that doesn't have it installed (see `savefig`'s own
//! note). Embedding the WHOLE file to draw thirty characters of tick
//! labels is what made a two-series line plot under `theme("nature")` a
//! 1.18 MB SVG -- the entire Inter variable font, base64'd, once per
//! figure. A six-figure paper carried ~7 MB of identical font data.
//!
//! `render_pdf` embeds through the same door, and pays double without it:
//! a PDF carries a separate copy per weight, so a paper figure spent 222 KB
//! of its 240 KB on two whole copies of Latin Modern to set a title and a
//! dozen tick numbers.
//!
//! This module keeps the guarantee and drops the cost: given the exact
//! characters a figure draws, it rebuilds the font with only those glyphs.
//! Everything here is deliberately conservative -- every parse step returns
//! `Option`, and ANY failure makes `subset` return `None` so the caller
//! falls back to embedding the original bytes. A figure that renders a
//! megabyte too large is a nuisance; a figure whose text disappears is a
//! bug, so the fallback is never allowed to be a half-built font.
//!
//! What survives, and why:
//!
//! * `fvar`/`avar`/`STAT`/`MVAR`/`gvar`/`HVAR` -- Inter and Source Serif
//!   are VARIABLE fonts and the `@font-face` rule claims `font-weight:
//!   100 900`, which is only true while the weight axis still works. `gvar`
//!   (the per-glyph deltas, and over half the file) is subsetted alongside
//!   `glyf`; `HVAR` (advance-width deltas -- without it a 700-weight title
//!   sets on 400-weight advances) has its delta sets subsetted and its
//!   glyph index map rebuilt for the new glyph order. The axis tables
//!   themselves carry no glyph ids and are copied verbatim.
//! * `cmap` is rebuilt rather than kept: it is indexed by character, so a
//!   full copy would cost more than every glyph outline we keep.
//!
//! What is dropped: `GPOS`/`GSUB`/`GDEF` (OpenType layout -- kerning and
//! ligatures) and `post`'s version-2 glyph names. The layout tables are
//! keyed by glyph id, so renumbering glyphs invalidates them wholesale, and
//! rewriting them means walking every lookup subtype in the specification
//! -- the "massive undertaking" that font subsetter crates explicitly
//! decline. They are also, in these fonts, most of what is left once the
//! outlines go (`GPOS` alone is 154 KB of Inter and 422 KB of Source
//! Serif), so keeping them would cap the win at roughly 4x instead of 50x.
//! The cost is kerning pairs and `fi`/`ffi` ligatures in figure labels.

use std::collections::{BTreeMap, BTreeSet};

// ------------------------------------------------------------------ read

fn u8at(d: &[u8], o: usize) -> Option<u8> {
    d.get(o).copied()
}

fn u16at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn i16at(d: &[u8], o: usize) -> Option<i16> {
    u16at(d, o).map(|v| v as i16)
}

fn u32at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

fn slice(d: &[u8], o: usize, n: usize) -> Option<&[u8]> {
    d.get(o..o.checked_add(n)?)
}

// ----------------------------------------------------------------- write

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

// ------------------------------------------------------- table directory

/// Tables that carry no glyph ids and are copied through untouched.
///
/// `fvar`/`avar`/`STAT`/`MVAR` are the variation axis machinery (axis
/// records, named instances, axis-value mappings) — they index the
/// ItemVariationStores and the name table, never glyphs. `cvt `/`fpgm`/
/// `prep`/`gasp` are the hinting programs the retained glyph instructions
/// still call into.
const COPIED: &[&[u8; 4]] =
    &[b"OS/2", b"fvar", b"avar", b"STAT", b"MVAR", b"gasp", b"cvt ", b"fpgm", b"prep"];

struct Sfnt<'a> {
    data: &'a [u8],
    version: u32,
    tables: BTreeMap<[u8; 4], (usize, usize)>,
}

impl<'a> Sfnt<'a> {
    fn parse(d: &'a [u8]) -> Option<Self> {
        let version = u32at(d, 0)?;
        // `ttcf` (a font collection) has a different header shape and none
        // of the bundled assets is one; refusing is cheaper than guessing.
        if version == u32::from_be_bytes(*b"ttcf") {
            return None;
        }
        let n = u16at(d, 4)? as usize;
        let mut tables = BTreeMap::new();
        for i in 0..n {
            let r = 12 + i * 16;
            let mut tag = [0u8; 4];
            tag.copy_from_slice(slice(d, r, 4)?);
            let off = u32at(d, r + 8)? as usize;
            let len = u32at(d, r + 12)? as usize;
            if off > d.len() {
                return None;
            }
            // A declared length that runs past EOF is a real thing in
            // shipped fonts (`DSIG` is the usual offender); clamp rather
            // than reject the whole file over a table we drop anyway.
            tables.insert(tag, (off, len.min(d.len() - off)));
        }
        Some(Sfnt { data: d, version, tables })
    }

    fn get(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let &(o, l) = self.tables.get(tag)?;
        slice(self.data, o, l)
    }
}

fn checksum(d: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0;
    while i < d.len() {
        let mut w = [0u8; 4];
        for (k, b) in w.iter_mut().enumerate() {
            if let Some(v) = d.get(i + k) {
                *b = *v;
            }
        }
        sum = sum.wrapping_add(u32::from_be_bytes(w));
        i += 4;
    }
    sum
}

/// Lays the rebuilt tables back out as an sfnt file, 4-byte aligned, with
/// real table checksums and `head`'s `checkSumAdjustment` recomputed over
/// the finished file.
fn assemble(version: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|t| t.0);
    let n = tables.len();
    let entry_selector = (usize::BITS - 1 - n.max(1).leading_zeros()) as u16;
    let search_range = 16u16 << entry_selector;
    let range_shift = (n as u16) * 16 - search_range;

    let mut out = Vec::new();
    push_u32(&mut out, version);
    push_u16(&mut out, n as u16);
    push_u16(&mut out, search_range);
    push_u16(&mut out, entry_selector);
    push_u16(&mut out, range_shift);

    let mut offset = 12 + 16 * n;
    let mut records = Vec::with_capacity(n);
    for (tag, data) in &tables {
        records.push((*tag, checksum(data), offset as u32, data.len() as u32));
        offset += (data.len() + 3) & !3;
    }
    for (tag, sum, off, len) in &records {
        out.extend_from_slice(tag);
        push_u32(&mut out, *sum);
        push_u32(&mut out, *off);
        push_u32(&mut out, *len);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(rec) = records.iter().find(|r| &r.0 == b"head") {
        let head_off = rec.2 as usize;
        // The magic constant is the specification's: the whole file's
        // checksum has to come out to 0xB1B0AFBA once this field is folded
        // back in.
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[head_off + 8..head_off + 12].copy_from_slice(&adj.to_be_bytes());
    }
    out
}

// ------------------------------------------------------------------ cmap

/// Picks the most capable Unicode subtable: format 12 (full Unicode) over
/// format 4 (BMP only), Windows platform over the deprecated Unicode one.
fn best_cmap_subtable(cmap: &[u8]) -> Option<&[u8]> {
    let n = u16at(cmap, 2)? as usize;
    let mut best: Option<(u8, usize)> = None;
    for i in 0..n {
        let r = 4 + i * 8;
        let pid = u16at(cmap, r)?;
        let eid = u16at(cmap, r + 2)?;
        let off = u32at(cmap, r + 4)? as usize;
        let Some(fmt) = u16at(cmap, off) else { continue };
        let score = match (pid, eid, fmt) {
            (3, 10, 12) => 5,
            (0, _, 12) => 4,
            (3, 1, 4) => 3,
            (0, _, 4) => 2,
            _ => 0,
        };
        if score > 0 && best.is_none_or(|(b, _)| score > b) {
            best = Some((score, off));
        }
    }
    let (_, off) = best?;
    cmap.get(off..)
}

fn lookup_format4(t: &[u8], c: u32) -> Option<u16> {
    if c > 0xFFFF {
        return Some(0);
    }
    let c = c as u16;
    let seg_count = u16at(t, 6)? as usize / 2;
    let end_at = 14;
    let start_at = end_at + seg_count * 2 + 2;
    let delta_at = start_at + seg_count * 2;
    let range_at = delta_at + seg_count * 2;
    for i in 0..seg_count {
        if u16at(t, end_at + i * 2)? < c {
            continue;
        }
        let start = u16at(t, start_at + i * 2)?;
        if start > c {
            return Some(0);
        }
        let delta = u16at(t, delta_at + i * 2)?;
        let range_off = u16at(t, range_at + i * 2)?;
        if range_off == 0 {
            return Some(c.wrapping_add(delta));
        }
        // The classic "offset from the address of this idRangeOffset entry"
        // indirection -- not from the start of the glyph id array.
        let at = range_at + i * 2 + range_off as usize + (c - start) as usize * 2;
        let g = u16at(t, at)?;
        return Some(if g == 0 { 0 } else { g.wrapping_add(delta) });
    }
    Some(0)
}

fn lookup_format12(t: &[u8], c: u32) -> Option<u16> {
    let groups = u32at(t, 12)? as usize;
    // Groups are sorted by start code, so a binary search is honest here
    // even though these fonts carry only a few hundred of them.
    let (mut lo, mut hi) = (0usize, groups);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let g = 16 + mid * 12;
        let start = u32at(t, g)?;
        let end = u32at(t, g + 4)?;
        if c < start {
            hi = mid;
        } else if c > end {
            lo = mid + 1;
        } else {
            let gid = u32at(t, g + 8)? + (c - start);
            return Some(if gid > 0xFFFF { 0 } else { gid as u16 });
        }
    }
    Some(0)
}

fn lookup_char(subtable: &[u8], c: char) -> Option<u16> {
    match u16at(subtable, 0)? {
        4 => lookup_format4(subtable, c as u32),
        12 => lookup_format12(subtable, c as u32),
        _ => None,
    }
}

/// Format 4, built from runs in which both the character and the glyph id
/// advance by one -- so every segment can use `idDelta` and the variable
/// `glyphIdArray` tail is never needed. Worst case is one segment per
/// character (8 bytes), which for a figure's hundred-odd characters is
/// still under a kilobyte.
fn write_format4(mapping: &[(u32, u16)]) -> Option<Vec<u8>> {
    let mut segs: Vec<(u16, u16, u16)> = Vec::new(); // start, end, idDelta
    for &(c, g) in mapping {
        // 0xFFFF is spoken for by the terminating segment below (and is a
        // Unicode noncharacter no font maps), so it never joins a run --
        // which also keeps the `last.1 + 1` below from overflowing.
        if c >= 0xFFFF {
            continue;
        }
        let c = c as u16;
        if let Some(last) = segs.last_mut() {
            if c == last.1 + 1 && g == c.wrapping_add(last.2) {
                last.1 = c;
                continue;
            }
        }
        segs.push((c, c, g.wrapping_sub(c)));
    }
    // The specification's mandatory terminating segment: 0xFFFF -> glyph 0.
    segs.push((0xFFFF, 0xFFFF, 1));

    let seg_count = segs.len();
    let entry_selector = (usize::BITS - 1 - seg_count.leading_zeros()) as u16;
    let search_range = 2u16.checked_shl(entry_selector as u32)?;
    let mut t = Vec::new();
    push_u16(&mut t, 4);
    push_u16(&mut t, (16 + 8 * seg_count) as u16);
    push_u16(&mut t, 0); // language
    push_u16(&mut t, (seg_count * 2) as u16);
    push_u16(&mut t, search_range);
    push_u16(&mut t, entry_selector);
    push_u16(&mut t, (seg_count * 2) as u16 - search_range);
    for s in &segs {
        push_u16(&mut t, s.1);
    }
    push_u16(&mut t, 0); // reservedPad
    for s in &segs {
        push_u16(&mut t, s.0);
    }
    for s in &segs {
        push_u16(&mut t, s.2);
    }
    for _ in &segs {
        push_u16(&mut t, 0); // idRangeOffset: never used, by construction
    }
    Some(t)
}

fn write_format12(mapping: &[(u32, u16)]) -> Vec<u8> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for &(c, g) in mapping {
        if let Some(last) = groups.last_mut() {
            if c == last.1 + 1 && g as u32 == last.2 + (last.1 - last.0) + 1 {
                last.1 = c;
                continue;
            }
        }
        groups.push((c, c, g as u32));
    }
    let mut t = Vec::new();
    push_u16(&mut t, 12);
    push_u16(&mut t, 0);
    push_u32(&mut t, (16 + 12 * groups.len()) as u32);
    push_u32(&mut t, 0); // language
    push_u32(&mut t, groups.len() as u32);
    for (start, end, gid) in groups {
        push_u32(&mut t, start);
        push_u32(&mut t, end);
        push_u32(&mut t, gid);
    }
    t
}

fn write_cmap(mapping: &[(u32, u16)]) -> Option<Vec<u8>> {
    let needs_12 = mapping.iter().any(|&(c, _)| c > 0xFFFF);
    let f4 = write_format4(mapping)?;
    let f12 = if needs_12 { Some(write_format12(mapping)) } else { None };

    // Two encoding records per subtable -- Windows (3,1)/(3,10) and the
    // Unicode platform (0,3)/(0,4) -- pointing at the same bytes. Eight
    // bytes each, and it keeps the font readable by a tool that only looks
    // for one of the two platforms.
    let mut records: Vec<(u16, u16, usize)> = vec![(0, 3, 0), (3, 1, 0)];
    if f12.is_some() {
        records.push((0, 4, 1));
        records.push((3, 10, 1));
    }
    let header = 4 + records.len() * 8;
    let f4_off = header;
    let f12_off = header + f4.len();

    let mut t = Vec::new();
    push_u16(&mut t, 0);
    push_u16(&mut t, records.len() as u16);
    for (pid, eid, which) in &records {
        push_u16(&mut t, *pid);
        push_u16(&mut t, *eid);
        push_u32(&mut t, if *which == 0 { f4_off as u32 } else { f12_off as u32 });
    }
    t.extend_from_slice(&f4);
    if let Some(f12) = f12 {
        t.extend_from_slice(&f12);
    }
    Some(t)
}

// ------------------------------------------------------------ glyf / loca

fn read_loca(loca: &[u8], num_glyphs: usize, long: bool) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(num_glyphs + 1);
    for i in 0..=num_glyphs {
        out.push(if long { u32at(loca, i * 4)? } else { u16at(loca, i * 2)? as u32 * 2 });
    }
    Some(out)
}

/// Byte positions of every `glyphIndex` field in a composite glyph (empty
/// for a simple one), so components can be both collected and renumbered
/// without walking the record twice with two different parsers.
fn component_positions(g: &[u8]) -> Option<Vec<usize>> {
    if g.len() < 10 || i16at(g, 0)? >= 0 {
        return Some(Vec::new());
    }
    const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    const WE_HAVE_A_SCALE: u16 = 0x0008;
    const MORE_COMPONENTS: u16 = 0x0020;
    const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

    let mut pos = Vec::new();
    let mut o = 10;
    loop {
        let flags = u16at(g, o)?;
        // Make sure the glyphIndex we are about to record is really there.
        u16at(g, o + 2)?;
        pos.push(o + 2);
        o += 4;
        o += if flags & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
        if flags & WE_HAVE_A_SCALE != 0 {
            o += 2;
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            o += 4;
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            o += 8;
        }
        if flags & MORE_COMPONENTS == 0 {
            return Some(pos);
        }
        if o > g.len() {
            return None;
        }
    }
}

// ------------------------------------------------------------------ gvar

/// Rebuilds `gvar` for the new glyph order.
///
/// Per-glyph variation data is self-contained apart from indices into the
/// shared tuple array, which is copied wholesale — so the blobs move
/// unmodified and only the offset array is rewritten. That array is the
/// same shape as `loca`, one entry per glyph plus a terminator, which is
/// why over half of Inter disappears here for the price of forty lines.
fn subset_gvar(gvar: &[u8], old_gids: &[u16]) -> Option<Vec<u8>> {
    let axis_count = u16at(gvar, 4)?;
    let shared_count = u16at(gvar, 6)? as usize;
    let shared_off = u32at(gvar, 8)? as usize;
    let glyph_count = u16at(gvar, 12)? as usize;
    let long_offsets = u16at(gvar, 14)? & 1 != 0;
    let data_off = u32at(gvar, 16)? as usize;

    let offset_of = |i: usize| -> Option<usize> {
        Some(if long_offsets {
            u32at(gvar, 20 + i * 4)? as usize
        } else {
            u16at(gvar, 20 + i * 2)? as usize * 2
        })
    };

    let shared = slice(gvar, shared_off, shared_count * axis_count as usize * 6)?;

    let mut blobs: Vec<&[u8]> = Vec::with_capacity(old_gids.len());
    for &g in old_gids {
        let g = g as usize;
        if g >= glyph_count {
            blobs.push(&[]);
            continue;
        }
        let (start, end) = (offset_of(g)?, offset_of(g + 1)?);
        if end <= start {
            blobs.push(&[]);
        } else {
            blobs.push(slice(gvar, data_off + start, end - start)?);
        }
    }

    let n = old_gids.len();
    let new_shared_off = 20 + (n + 1) * 4;
    let new_data_off = new_shared_off + shared.len();

    let mut out = Vec::new();
    push_u16(&mut out, 1); // majorVersion
    push_u16(&mut out, 0); // minorVersion
    push_u16(&mut out, axis_count);
    push_u16(&mut out, shared_count as u16);
    push_u32(&mut out, new_shared_off as u32);
    push_u16(&mut out, n as u16);
    push_u16(&mut out, 1); // flags: long offsets, so no 2-byte alignment rule
    push_u32(&mut out, new_data_off as u32);

    // Long offsets carry no alignment rule, but every blob opens with a
    // `u16` count, so they are kept even-aligned anyway -- Chrome's
    // sanitizer does not care, a stricter rasterizer might. Trailing pad
    // bytes are harmless: the reader navigates a blob by its own internal
    // offsets, never by its declared length.
    let mut acc = 0u32;
    push_u32(&mut out, acc);
    for b in &blobs {
        acc += (b.len() as u32).next_multiple_of(2);
        push_u32(&mut out, acc);
    }
    out.extend_from_slice(shared);
    for b in &blobs {
        out.extend_from_slice(b);
        if !b.len().is_multiple_of(2) {
            out.push(0);
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ HVAR

/// Reads a DeltaSetIndexMap into one `(outer, inner)` pair per glyph.
///
/// An absent map (offset 0) is the specification's "the glyph id IS the
/// inner index, outer is 0" shorthand; a map shorter than the glyph count
/// repeats its last entry, which is how a font compresses a long tail of
/// glyphs that share one delta set.
fn read_index_map(d: &[u8], off: usize, num_glyphs: usize) -> Option<Vec<DeltaSetIndex>> {
    if off == 0 {
        return Some((0..num_glyphs).map(|g| (0u16, g as u16)).collect());
    }
    let format = u8at(d, off)?;
    let entry_format = u8at(d, off + 1)? as usize;
    let inner_bits = (entry_format & 0x0F) + 1;
    let entry_size = ((entry_format & 0x30) >> 4) + 1;
    let (map_count, data_at) = match format {
        0 => (u16at(d, off + 2)? as usize, off + 4),
        1 => (u32at(d, off + 2)? as usize, off + 6),
        _ => return None,
    };
    if map_count == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(num_glyphs);
    for g in 0..num_glyphs {
        let p = data_at + g.min(map_count - 1) * entry_size;
        let mut v = 0u32;
        for k in 0..entry_size {
            v = (v << 8) | u8at(d, p + k)? as u32;
        }
        out.push(((v >> inner_bits) as u16, (v & ((1u32 << inner_bits) - 1)) as u16));
    }
    Some(out)
}

fn write_index_map(entries: &[DeltaSetIndex]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0); // format 0: a u16 count is plenty for a subset font
    out.push(0x3F); // 4-byte entries, 16-bit inner index
    push_u16(&mut out, entries.len() as u16);
    for (outer, inner) in entries {
        push_u16(&mut out, *outer);
        push_u16(&mut out, *inner);
    }
    out
}

/// How a glyph's advance-width delta set is addressed: which
/// ItemVariationData subtable (`outer`) and which row inside it (`inner`).
type DeltaSetIndex = (u16, u16);

/// Keeps only the delta-set rows the retained glyphs actually reference,
/// returning the rebuilt store and the `(outer, inner)` renumbering.
///
/// The region list is shared by every subtable and is a few hundred bytes,
/// so it is copied verbatim and each subtable's `regionIndexes` stay valid
/// untouched. Emptied subtables are kept in place rather than removed, so
/// `outer` indices never have to move.
fn subset_ivs(
    ivs: &[u8],
    used: &BTreeSet<DeltaSetIndex>,
) -> Option<(Vec<u8>, BTreeMap<DeltaSetIndex, DeltaSetIndex>)> {
    if u16at(ivs, 0)? != 1 {
        return None;
    }
    let region_off = u32at(ivs, 2)? as usize;
    let count = u16at(ivs, 6)? as usize;

    let axis_count = u16at(ivs, region_off)? as usize;
    let region_count = u16at(ivs, region_off + 2)? as usize;
    let regions = slice(ivs, region_off, 4 + region_count * axis_count * 6)?;

    let mut remap = BTreeMap::new();
    let mut subtables: Vec<Vec<u8>> = Vec::with_capacity(count);
    for outer in 0..count {
        let at = u32at(ivs, 8 + outer * 4)? as usize;
        let item_count = u16at(ivs, at)? as usize;
        let word_delta_count = u16at(ivs, at + 2)?;
        let region_index_count = u16at(ivs, at + 4)? as usize;
        // Bit 15 says the "word" deltas are 32-bit and the rest 16-bit;
        // otherwise they are 16-bit and 8-bit.
        let long_words = word_delta_count & 0x8000 != 0;
        let words = (word_delta_count & 0x7FFF) as usize;
        if words > region_index_count {
            return None;
        }
        let (wide, narrow) = if long_words { (4, 2) } else { (2, 1) };
        let row_size = words * wide + (region_index_count - words) * narrow;
        let rows_at = at + 6 + region_index_count * 2;

        let mut keep: Vec<u16> =
            used.iter().filter(|(o, _)| *o as usize == outer).map(|(_, i)| *i).collect();
        keep.sort_unstable();
        keep.dedup();

        let mut t = Vec::new();
        push_u16(&mut t, keep.len() as u16);
        push_u16(&mut t, word_delta_count);
        push_u16(&mut t, region_index_count as u16);
        t.extend_from_slice(slice(ivs, at + 6, region_index_count * 2)?);
        for (new_inner, &inner) in keep.iter().enumerate() {
            if inner as usize >= item_count {
                return None;
            }
            t.extend_from_slice(slice(ivs, rows_at + inner as usize * row_size, row_size)?);
            remap.insert((outer as u16, inner), (outer as u16, new_inner as u16));
        }
        subtables.push(t);
    }

    let mut out = Vec::new();
    push_u16(&mut out, 1);
    let new_region_off = 8 + count * 4;
    push_u32(&mut out, new_region_off as u32);
    push_u16(&mut out, count as u16);
    let mut at = new_region_off + regions.len();
    for t in &subtables {
        push_u32(&mut out, at as u32);
        at += t.len();
    }
    out.extend_from_slice(regions);
    for t in &subtables {
        out.extend_from_slice(t);
    }
    Some((out, remap))
}

/// Rebuilds `HVAR` — the advance-width deltas that make a 700-weight title
/// set on 700-weight advances instead of the regular face's.
fn subset_hvar(hvar: &[u8], old_gids: &[u16], num_glyphs: usize) -> Option<Vec<u8>> {
    let ivs_off = u32at(hvar, 4)? as usize;
    let adv_map_off = u32at(hvar, 8)? as usize;
    let ivs = hvar.get(ivs_off..)?;
    let map = read_index_map(hvar, adv_map_off, num_glyphs)?;

    let picked: Vec<DeltaSetIndex> =
        old_gids.iter().map(|&g| *map.get(g as usize).unwrap_or(&(0, 0))).collect();
    let used: BTreeSet<DeltaSetIndex> = picked.iter().copied().collect();
    let (mut new_ivs, remap) = subset_ivs(ivs, &used)?;
    let renumbered: Vec<DeltaSetIndex> =
        picked.iter().map(|k| remap.get(k).copied()).collect::<Option<Vec<_>>>()?;
    let new_map = write_index_map(&renumbered);
    // Keep the index map that follows on a 4-byte boundary: its entries are
    // 4 bytes each and some rasterizers read them as aligned words.
    while !new_ivs.len().is_multiple_of(4) {
        new_ivs.push(0);
    }

    let mut out = Vec::new();
    push_u16(&mut out, 1); // majorVersion
    push_u16(&mut out, 0); // minorVersion
    push_u32(&mut out, 20); // itemVariationStoreOffset
    push_u32(&mut out, (20 + new_ivs.len()) as u32); // advanceWidthMapping
    push_u32(&mut out, 0); // lsbMapping: a figure never needs sidebearing deltas
    push_u32(&mut out, 0); // rsbMapping
    out.extend_from_slice(&new_ivs);
    out.extend_from_slice(&new_map);
    Some(out)
}

// ------------------------------------------------------------------ name

/// Keeps the Windows/Unicode English records plus everything `fvar` names
/// (axis and named-instance labels live at name id 256 and up, and a
/// variable font with dangling axis names is one some rasterizers refuse).
/// Source Serif's name table is 7.8 KB, most of it Macintosh duplicates and
/// translations no renderer of an embedded figure font will ever ask for.
fn subset_name(name: &[u8]) -> Option<Vec<u8>> {
    let count = u16at(name, 2)? as usize;
    let storage = u16at(name, 4)? as usize;
    let mut kept: Vec<(u16, u16, u16, u16, &[u8])> = Vec::new();
    for i in 0..count {
        let r = 6 + i * 12;
        let (pid, eid) = (u16at(name, r)?, u16at(name, r + 2)?);
        let (lid, nid) = (u16at(name, r + 4)?, u16at(name, r + 6)?);
        let len = u16at(name, r + 8)? as usize;
        let off = u16at(name, r + 10)? as usize;
        if pid != 3 || eid != 1 || (lid != 0x0409 && nid < 256) {
            continue;
        }
        kept.push((pid, eid, lid, nid, slice(name, storage + off, len)?));
    }
    if kept.is_empty() {
        return None;
    }
    // Name records must be sorted by platform/encoding/language/name id.
    kept.sort_by_key(|r| (r.0, r.1, r.2, r.3));

    let new_storage = 6 + kept.len() * 12;
    let mut records = Vec::new();
    let mut strings: Vec<u8> = Vec::new();
    for (pid, eid, lid, nid, s) in &kept {
        push_u16(&mut records, *pid);
        push_u16(&mut records, *eid);
        push_u16(&mut records, *lid);
        push_u16(&mut records, *nid);
        push_u16(&mut records, s.len() as u16);
        push_u16(&mut records, strings.len() as u16);
        strings.extend_from_slice(s);
    }
    let mut out = Vec::new();
    push_u16(&mut out, 0); // format 0
    push_u16(&mut out, kept.len() as u16);
    push_u16(&mut out, new_storage as u16);
    out.extend_from_slice(&records);
    out.extend_from_slice(&strings);
    Some(out)
}

// ------------------------------------------------------------------- CFF

struct Index<'a> {
    items: Vec<&'a [u8]>,
    end: usize,
}

fn read_index(d: &[u8], p: usize) -> Option<Index<'_>> {
    let count = u16at(d, p)? as usize;
    if count == 0 {
        return Some(Index { items: Vec::new(), end: p + 2 });
    }
    let off_size = u8at(d, p + 2)? as usize;
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offs_at = p + 3;
    let mut offs = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let q = offs_at + i * off_size;
        let mut v = 0usize;
        for k in 0..off_size {
            v = (v << 8) | u8at(d, q + k)? as usize;
        }
        offs.push(v);
    }
    // CFF INDEX offsets are 1-based from the byte before the data block.
    let data = offs_at + (count + 1) * off_size - 1;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        if offs[i] < 1 || offs[i + 1] < offs[i] {
            return None;
        }
        items.push(slice(d, data + offs[i], offs[i + 1] - offs[i])?);
    }
    Some(Index { items, end: data + offs[count] })
}

fn write_index(items: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    if items.is_empty() {
        push_u16(&mut out, 0);
        return out;
    }
    let total: usize = items.iter().map(|i| i.len()).sum::<usize>() + 1;
    let off_size = if total <= 0xFF {
        1
    } else if total <= 0xFFFF {
        2
    } else if total <= 0x00FF_FFFF {
        3
    } else {
        4
    };
    push_u16(&mut out, items.len() as u16);
    out.push(off_size as u8);
    let mut acc = 1usize;
    let mut offsets = vec![acc];
    for it in items {
        acc += it.len();
        offsets.push(acc);
    }
    for v in offsets {
        for k in (0..off_size).rev() {
            out.push(((v >> (k * 8)) & 0xFF) as u8);
        }
    }
    for it in items {
        out.extend_from_slice(it);
    }
    out
}

/// One DICT entry: the operator (a two-byte operator is folded into
/// `0x0c00 | b1`), the raw operand bytes so entries we don't touch can be
/// re-emitted byte for byte, and the decoded integers for the ones we do.
struct DictEntry {
    op: u16,
    raw: Vec<u8>,
    ints: Vec<i32>,
}

fn parse_dict(d: &[u8]) -> Option<Vec<DictEntry>> {
    let mut out = Vec::new();
    let mut operands: Vec<u8> = Vec::new();
    let mut ints: Vec<i32> = Vec::new();
    let mut i = 0usize;
    while i < d.len() {
        let b0 = d[i];
        match b0 {
            0..=21 => {
                let op = if b0 == 12 {
                    i += 2;
                    0x0c00 | *d.get(i - 1)? as u16
                } else {
                    i += 1;
                    b0 as u16
                };
                out.push(DictEntry {
                    op,
                    raw: std::mem::take(&mut operands),
                    ints: std::mem::take(&mut ints),
                });
            }
            28 => {
                ints.push(i16::from_be_bytes([*d.get(i + 1)?, *d.get(i + 2)?]) as i32);
                operands.extend_from_slice(slice(d, i, 3)?);
                i += 3;
            }
            29 => {
                ints.push(i32::from_be_bytes([
                    *d.get(i + 1)?,
                    *d.get(i + 2)?,
                    *d.get(i + 3)?,
                    *d.get(i + 4)?,
                ]));
                operands.extend_from_slice(slice(d, i, 5)?);
                i += 5;
            }
            30 => {
                // A real number: packed BCD nibbles, ended by an 0xF nibble.
                let start = i;
                i += 1;
                loop {
                    let b = *d.get(i)?;
                    i += 1;
                    if b & 0x0F == 0x0F || b >> 4 == 0x0F {
                        break;
                    }
                }
                operands.extend_from_slice(slice(d, start, i - start)?);
                ints.push(0);
            }
            32..=246 => {
                ints.push(b0 as i32 - 139);
                operands.push(b0);
                i += 1;
            }
            247..=250 => {
                ints.push((b0 as i32 - 247) * 256 + *d.get(i + 1)? as i32 + 108);
                operands.extend_from_slice(slice(d, i, 2)?);
                i += 2;
            }
            251..=254 => {
                ints.push(-(b0 as i32 - 251) * 256 - *d.get(i + 1)? as i32 - 108);
                operands.extend_from_slice(slice(d, i, 2)?);
                i += 2;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Always the 5-byte form, so rewriting an offset never changes a DICT's
/// length -- which is what lets the layout below be computed in one pass
/// instead of iterated to a fixed point.
fn dict_int(v: i32) -> [u8; 5] {
    let b = v.to_be_bytes();
    [29, b[0], b[1], b[2], b[3]]
}

fn push_op(out: &mut Vec<u8>, op: u16) {
    if op >= 0x0c00 {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
}

fn read_charset(d: &[u8], off: usize, num_glyphs: usize) -> Option<Vec<u16>> {
    let mut sids = vec![0u16; num_glyphs];
    if off <= 2 {
        // A predefined charset. ISOAdobe (0) is SID == GID; the other two
        // are expert sets no text font uses. Glyph names don't affect
        // rendering in an OpenType font (the `cmap` decides), so an
        // approximation here is harmless.
        for (g, s) in sids.iter_mut().enumerate() {
            *s = g as u16;
        }
        return Some(sids);
    }
    match u8at(d, off)? {
        0 => {
            for (g, s) in sids.iter_mut().enumerate().skip(1) {
                *s = u16at(d, off + 1 + (g - 1) * 2)?;
            }
        }
        f @ (1 | 2) => {
            let (mut g, mut p) = (1usize, off + 1);
            while g < num_glyphs {
                let first = u16at(d, p)?;
                let n_left =
                    if f == 1 { u8at(d, p + 2)? as usize } else { u16at(d, p + 2)? as usize };
                p += if f == 1 { 3 } else { 4 };
                for k in 0..=n_left {
                    if g >= num_glyphs {
                        break;
                    }
                    sids[g] = first.wrapping_add(k as u16);
                    g += 1;
                }
            }
        }
        _ => return None,
    }
    Some(sids)
}

/// A subroutine's index in a charstring is stored biased by the INDEX's
/// own length, so the bias is a property of the count, not of the entry.
fn subr_bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// Which subroutines the retained charstrings actually reach.
///
/// Latin Modern Math carries a 57 KB local subroutine INDEX — bigger than
/// everything else in the subset put together — so keeping it whole made a
/// figure with one Greek letter in it cost more than the glyph outlines of
/// the entire alphabet.
///
/// This walks Type 2 charstrings just far enough to see the number pushed
/// before each `callsubr`/`callgsubr`, following calls recursively. It does
/// NOT renumber anything: the retained subroutines stay at their original
/// indices and the emptied ones stay present as zero-length entries, so
/// every `callsubr` operand in every charstring remains correct and the
/// bias is untouched. That trades a few hundred bytes of offset array for
/// not having to rewrite charstrings, which is where a subtle rendering
/// bug would come from.
///
/// Returns `None` if anything is not understood, which makes the caller
/// keep every subroutine rather than emit a font that calls a hole.
struct SubrUse {
    local: BTreeSet<usize>,
    global: BTreeSet<usize>,
}

/// The two subroutine INDEXes and their biases, fixed for one font.
struct SubrCtx<'a> {
    local: &'a [&'a [u8]],
    global: &'a [&'a [u8]],
    lbias: i32,
    gbias: i32,
}

fn collect_subr_use(
    charstrings: &[&[u8]],
    local: &[&[u8]],
    global: &[&[u8]],
) -> Option<SubrUse> {
    let mut used = SubrUse { local: BTreeSet::new(), global: BTreeSet::new() };
    let ctx = SubrCtx {
        local,
        global,
        lbias: subr_bias(local.len()),
        gbias: subr_bias(global.len()),
    };

    // Depth is bounded by the specification at 10; anything deeper is a
    // malformed font, and the bound also stops a cyclic call graph.
    fn walk(
        cs: &[u8],
        stack: &mut Vec<i32>,
        stems: &mut usize,
        depth: usize,
        ctx: &SubrCtx,
        used: &mut SubrUse,
    ) -> Option<()> {
        if depth > 10 {
            return None;
        }
        let mut i = 0usize;
        while i < cs.len() {
            let b0 = cs[i];
            match b0 {
                32..=246 => {
                    stack.push(b0 as i32 - 139);
                    i += 1;
                }
                247..=250 => {
                    stack.push((b0 as i32 - 247) * 256 + *cs.get(i + 1)? as i32 + 108);
                    i += 2;
                }
                251..=254 => {
                    stack.push(-(b0 as i32 - 251) * 256 - *cs.get(i + 1)? as i32 - 108);
                    i += 2;
                }
                28 => {
                    stack.push(i16::from_be_bytes([*cs.get(i + 1)?, *cs.get(i + 2)?]) as i32);
                    i += 3;
                }
                255 => {
                    // 16.16 fixed; only the integer part could ever be a
                    // subroutine index, and in practice never is.
                    stack.push(i16::from_be_bytes([*cs.get(i + 1)?, *cs.get(i + 2)?]) as i32);
                    i += 5;
                }
                // hstem / vstem / hstemhm / vstemhm: two arguments per stem.
                1 | 3 | 18 | 23 => {
                    *stems += stack.len() / 2;
                    stack.clear();
                    i += 1;
                }
                // hintmask / cntrmask: any pending arguments are an implicit
                // vstem, then a bitmask of ceil(stems/8) bytes follows the
                // operator. Miscounting here desynchronises the whole walk.
                19 | 20 => {
                    *stems += stack.len() / 2;
                    stack.clear();
                    i += 1 + (*stems).div_ceil(8);
                }
                10 => {
                    let idx = usize::try_from(stack.pop()? + ctx.lbias).ok()?;
                    let sub = *ctx.local.get(idx)?;
                    used.local.insert(idx);
                    walk(sub, stack, stems, depth + 1, ctx, used)?;
                    i += 1;
                }
                29 => {
                    let idx = usize::try_from(stack.pop()? + ctx.gbias).ok()?;
                    let sub = *ctx.global.get(idx)?;
                    used.global.insert(idx);
                    walk(sub, stack, stems, depth + 1, ctx, used)?;
                    i += 1;
                }
                11 => return Some(()), // return
                14 => {
                    stack.clear();
                    i += 1;
                }
                12 => {
                    stack.clear();
                    i += 2; // two-byte operator
                }
                _ => {
                    stack.clear();
                    i += 1;
                }
            }
        }
        Some(())
    }

    for cs in charstrings {
        let mut stack = Vec::new();
        let mut stems = 0usize;
        walk(cs, &mut stack, &mut stems, 0, &ctx, &mut used)?;
    }
    Some(used)
}

/// A single `return` — a valid, empty-bodied Type 2 subroutine.
///
/// Dropped entries become this rather than zero-length. A zero-byte
/// charstring is not a charstring: it never reaches `return` or `endchar`,
/// and a validator that walks the subroutine INDEX (Chrome's does) rejects
/// the whole font over it, which is how a subset of Latin Modern Math that
/// called no subroutines at all still failed to load.
const CFF_RETURN: &[u8] = &[11];

/// An INDEX of the same length, carrying only the entries in `keep`.
///
/// The count is preserved deliberately — see `collect_subr_use`.
fn prune_index<'a>(items: &[&'a [u8]], keep: &BTreeSet<usize>) -> Vec<&'a [u8]> {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| if keep.contains(&i) { *it } else { CFF_RETURN })
        .collect()
}

const CFF_CHARSET: u16 = 15;
const CFF_ENCODING: u16 = 16;
const CFF_CHARSTRINGS: u16 = 17;
const CFF_PRIVATE: u16 = 18;
const CFF_SUBRS: u16 = 19;
const CFF_ROS: u16 = 0x0c1e;

/// Top DICT operators whose single operand is a SID -- an index into the
/// String INDEX, which this module rebuilds. Copying one of these through
/// verbatim leaves it pointing past the end of the new String INDEX, which
/// is a font every parser rejects. (`version`, `Notice`, `FullName`,
/// `FamilyName`, `Weight`, `Copyright`, `PostScript`, `BaseFontName`.)
const CFF_SID_OPS: &[u16] = &[0, 1, 2, 3, 4, 0x0c00, 0x0c15, 0x0c16];

/// The String INDEX being rebuilt, plus the old-SID -> new-SID map.
///
/// SIDs below 391 name the standard strings every CFF parser already
/// knows and pass through untouched; everything above that is a string
/// carried in the font's own INDEX and has to be re-interned.
struct StringPool<'a> {
    items: Vec<&'a [u8]>,
    map: BTreeMap<u16, u16>,
}

impl<'a> StringPool<'a> {
    fn intern(&mut self, sid: u16, source: &Index<'a>) -> Option<u16> {
        if sid < 391 {
            return Some(sid);
        }
        if let Some(&new) = self.map.get(&sid) {
            return Some(new);
        }
        let new = 391 + self.items.len() as u16;
        self.items.push(*source.items.get(sid as usize - 391)?);
        self.map.insert(sid, new);
        Some(new)
    }
}

/// Rebuilds a name-keyed CFF (Latin Modern) around the retained glyphs.
///
/// Global and local subroutines are kept whole: pruning them means
/// interpreting every retained charstring to find which subroutines it
/// calls, and then renumbering the calls (a subroutine's index is biased by
/// the total count), which is a charstring-rewriting job out of proportion
/// to the ~8 KB at stake.
fn subset_cff(cff: &[u8], old_gids: &[u16]) -> Option<Vec<u8>> {
    let hdr_size = u8at(cff, 2)? as usize;
    let name_idx = read_index(cff, hdr_size)?;
    let top_idx = read_index(cff, name_idx.end)?;
    let string_idx = read_index(cff, top_idx.end)?;
    let gsubr_idx = read_index(cff, string_idx.end)?;

    let top = top_idx.items.first()?;
    let entries = parse_dict(top)?;
    // A CID-keyed font routes glyphs through FDSelect/FDArray; none of the
    // bundled faces is one, and guessing at that machinery is exactly the
    // kind of half-built font this module refuses to emit.
    if entries.iter().any(|e| e.op == CFF_ROS) {
        return None;
    }
    let find = |op: u16| entries.iter().find(|e| e.op == op);
    let charstrings_off = *find(CFF_CHARSTRINGS)?.ints.last()? as usize;
    let charset_off = find(CFF_CHARSET).and_then(|e| e.ints.last().copied()).unwrap_or(0) as usize;
    let private = find(CFF_PRIVATE)?;
    if private.ints.len() < 2 {
        return None;
    }
    let (priv_size, priv_off) = (private.ints[0] as usize, private.ints[1] as usize);

    let charstrings = read_index(cff, charstrings_off)?;
    let num_glyphs = charstrings.items.len();
    let sids = read_charset(cff, charset_off, num_glyphs)?;

    let mut pool = StringPool { items: Vec::new(), map: BTreeMap::new() };

    // The Top DICT's own name strings are re-interned first, so the font
    // still describes itself after the String INDEX is rebuilt.
    let mut passthrough: Vec<(u16, Vec<u8>)> = Vec::new();
    for e in &entries {
        if matches!(e.op, CFF_CHARSET | CFF_ENCODING | CFF_CHARSTRINGS | CFF_PRIVATE) {
            continue;
        }
        if CFF_SID_OPS.contains(&e.op) {
            let sid = u16::try_from(*e.ints.last()?).ok()?;
            passthrough.push((e.op, dict_int(pool.intern(sid, &string_idx)? as i32).to_vec()));
        } else {
            passthrough.push((e.op, e.raw.clone()));
        }
    }

    // The retained charstrings, and the glyph-name strings they reference.
    let mut new_charstrings: Vec<&[u8]> = Vec::with_capacity(old_gids.len());
    let mut new_sids: Vec<u16> = Vec::new();
    for (new_gid, &og) in old_gids.iter().enumerate() {
        let og = og as usize;
        new_charstrings.push(*charstrings.items.get(og)?);
        if new_gid == 0 {
            continue; // .notdef's SID is implicit in a format 0 charset
        }
        new_sids.push(pool.intern(*sids.get(og)?, &string_idx)?);
    }

    let mut charset = vec![0u8]; // format 0
    for sid in &new_sids {
        push_u16(&mut charset, *sid);
    }

    // The Private DICT, with its Subrs offset (which is relative to the
    // DICT's own start) repointed at the local subroutine INDEX placed
    // immediately after it.
    let priv_dict = slice(cff, priv_off, priv_size)?;
    let priv_entries = parse_dict(priv_dict)?;
    let local_subrs = match priv_entries.iter().find(|e| e.op == CFF_SUBRS) {
        Some(e) => Some(read_index(cff, priv_off + *e.ints.last()? as usize)?),
        None => None,
    };
    let mut new_priv = Vec::new();
    for e in &priv_entries {
        if e.op == CFF_SUBRS {
            continue;
        }
        new_priv.extend_from_slice(&e.raw);
        push_op(&mut new_priv, e.op);
    }
    if local_subrs.is_some() {
        // 6 = this entry's own 5-byte operand plus its 1-byte operator.
        new_priv.extend_from_slice(&dict_int((new_priv.len() + 6) as i32));
        push_op(&mut new_priv, CFF_SUBRS);
    }

    // Emptied subroutines still occupy their slot, so nothing has to be
    // renumbered; if the walk meets anything it doesn't understand, every
    // subroutine is kept instead.
    let empty: Vec<&[u8]> = Vec::new();
    let local_items = local_subrs.as_ref().map(|i| &i.items).unwrap_or(&empty);
    let (kept_local, kept_global) =
        match collect_subr_use(&new_charstrings, local_items, &gsubr_idx.items) {
            Some(u) => {
                (prune_index(local_items, &u.local), prune_index(&gsubr_idx.items, &u.global))
            }
            None => (local_items.clone(), gsubr_idx.items.clone()),
        };

    let name_bytes = write_index(&name_idx.items);
    let string_bytes = write_index(&pool.items);
    let gsubr_bytes = write_index(&kept_global);
    let charstrings_bytes = write_index(&new_charstrings);
    let local_bytes =
        if local_subrs.is_some() { write_index(&kept_local) } else { Vec::new() };

    let build_top = |charset_at: usize, charstrings_at: usize, private_at: usize| -> Vec<u8> {
        let mut t = Vec::new();
        // Encoding is ignored in an OpenType CFF (the `cmap` rules), so it
        // was dropped rather than relocated when `passthrough` was built.
        for (op, raw) in &passthrough {
            t.extend_from_slice(raw);
            push_op(&mut t, *op);
        }
        t.extend_from_slice(&dict_int(charset_at as i32));
        push_op(&mut t, CFF_CHARSET);
        t.extend_from_slice(&dict_int(charstrings_at as i32));
        push_op(&mut t, CFF_CHARSTRINGS);
        t.extend_from_slice(&dict_int(new_priv.len() as i32));
        t.extend_from_slice(&dict_int(private_at as i32));
        push_op(&mut t, CFF_PRIVATE);
        t
    };

    // `dict_int` is fixed-width, so the placeholder and the real Top DICT
    // are byte-for-byte the same length and one pass settles the layout.
    let top_bytes = write_index(&[&build_top(0, 0, 0)[..]]);
    let prefix = 4 + name_bytes.len() + top_bytes.len() + string_bytes.len() + gsubr_bytes.len();
    let charset_at = prefix;
    let charstrings_at = charset_at + charset.len();
    let private_at = charstrings_at + charstrings_bytes.len();
    let top_bytes = write_index(&[&build_top(charset_at, charstrings_at, private_at)[..]]);

    let mut out = Vec::new();
    out.extend_from_slice(&[1, 0, 4, 4]); // major, minor, hdrSize, offSize
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(&top_bytes);
    out.extend_from_slice(&string_bytes);
    out.extend_from_slice(&gsubr_bytes);
    debug_assert_eq!(out.len(), charset_at);
    out.extend_from_slice(&charset);
    out.extend_from_slice(&charstrings_bytes);
    out.extend_from_slice(&new_priv);
    out.extend_from_slice(&local_bytes);
    Some(out)
}

// ------------------------------------------------------------------ entry

/// The characters of `chars` this font cannot draw.
///
/// Used to decide whether a figure needs a fallback face embedded beside
/// the main one: Latin Modern has no Greek and no mathematical operators
/// at all, so `$\mu$` under `theme("publication")` drops mid-word to
/// whatever the viewer's machine supplies -- visibly not the same typeface,
/// which is exactly what embedding a font is supposed to prevent.
///
/// An empty result means the main face covers everything and no fallback
/// needs embedding. A font this module cannot parse reports nothing
/// missing, so an unreadable font never triggers a spurious second face.
/// The glyph id `font` draws `ch` with, or `None` if it has no glyph.
///
/// The PDF backend needs this to address a glyph DIRECTLY. A simple font
/// reaches its glyphs through an encoding — WinAnsiEncoding, 224 codes, no
/// Greek anywhere in it — so `ν` could not be named at all and came out
/// spelled "nu". A `Type0`/`Identity-H` font addresses glyph ids instead,
/// which has no such ceiling, and this is how a character is turned into
/// one. Call it on the SUBSET, not the original: subsetting renumbers
/// every glyph.
pub fn glyph_id(font: &[u8], ch: char) -> Option<u16> {
    let sfnt = Sfnt::parse(font)?;
    let subtable = sfnt.get(b"cmap").and_then(best_cmap_subtable)?;
    match lookup_char(subtable, ch) {
        Some(0) | None => None,
        Some(g) => Some(g),
    }
}

pub fn unsupported(font: &[u8], chars: &BTreeSet<char>) -> BTreeSet<char> {
    let Some(sfnt) = Sfnt::parse(font) else { return BTreeSet::new() };
    let Some(subtable) = sfnt.get(b"cmap").and_then(best_cmap_subtable) else {
        return BTreeSet::new();
    };
    chars
        .iter()
        .copied()
        .filter(|&c| !c.is_whitespace() && lookup_char(subtable, c).unwrap_or(0) == 0)
        .collect()
}

/// Rebuilds `font` carrying only the glyphs needed to draw `chars`.
///
/// Returns `None` — meaning "embed the original" — for anything this
/// module does not fully understand, so a figure can never come out with
/// missing text because the subsetter met a table it half-recognised.
pub fn subset(font: &[u8], chars: &BTreeSet<char>) -> Option<Vec<u8>> {
    let sfnt = Sfnt::parse(font)?;
    let head = sfnt.get(b"head")?;
    let maxp = sfnt.get(b"maxp")?;
    let hhea = sfnt.get(b"hhea")?;
    let hmtx = sfnt.get(b"hmtx")?;
    let num_glyphs = u16at(maxp, 4)? as usize;
    let num_h_metrics = u16at(hhea, 34)? as usize;
    if num_glyphs == 0 || num_h_metrics == 0 {
        return None;
    }

    let cmap = sfnt.get(b"cmap")?;
    let subtable = best_cmap_subtable(cmap)?;
    let mut mapping: Vec<(u32, u16)> = Vec::new();
    let mut keep: BTreeSet<u16> = BTreeSet::new();
    keep.insert(0); // .notdef, always glyph 0
    for &c in chars {
        let gid = lookup_char(subtable, c)?;
        if gid == 0 || gid as usize >= num_glyphs {
            continue; // not in this font: the renderer already falls back
        }
        mapping.push((c as u32, gid));
        keep.insert(gid);
    }

    let glyf = sfnt.get(b"glyf");
    let loca = match (glyf, sfnt.get(b"loca")) {
        (Some(_), Some(l)) => Some(read_loca(l, num_glyphs, u16at(head, 50)? == 1)?),
        (None, _) => None,
        _ => return None,
    };

    // A composite glyph points at other glyphs, transitively.
    if let (Some(glyf), Some(loca)) = (glyf, loca.as_ref()) {
        let mut queue: Vec<u16> = keep.iter().copied().collect();
        while let Some(g) = queue.pop() {
            let (s, e) = (loca[g as usize] as usize, loca[g as usize + 1] as usize);
            if e <= s {
                continue;
            }
            let gd = slice(glyf, s, e - s)?;
            for pos in component_positions(gd)? {
                let c = u16at(gd, pos)?;
                if c as usize >= num_glyphs {
                    return None;
                }
                if keep.insert(c) {
                    queue.push(c);
                }
            }
        }
    }

    let old_gids: Vec<u16> = keep.iter().copied().collect();
    let new_of: BTreeMap<u16, u16> =
        old_gids.iter().enumerate().map(|(i, &g)| (g, i as u16)).collect();
    let n = old_gids.len();

    // The mapping the new `cmap` publishes, in the new glyph numbering.
    mapping.sort_unstable();
    let mapping: Vec<(u32, u16)> = mapping.iter().map(|&(c, g)| (c, new_of[&g])).collect();

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();

    // --- head: the same 54 bytes, but long `loca` offsets from here on.
    let mut new_head = head.get(..54)?.to_vec();
    new_head[8..12].copy_from_slice(&[0; 4]); // checkSumAdjustment, refilled
    if glyf.is_some() {
        new_head[50..52].copy_from_slice(&1u16.to_be_bytes());
    }
    tables.push((*b"head", new_head));

    // --- maxp / hhea: the glyph and metric counts.
    let mut new_maxp = maxp.to_vec();
    new_maxp[4..6].copy_from_slice(&(n as u16).to_be_bytes());
    tables.push((*b"maxp", new_maxp));
    let mut new_hhea = hhea.get(..36)?.to_vec();
    new_hhea[34..36].copy_from_slice(&(n as u16).to_be_bytes());
    tables.push((*b"hhea", new_hhea));

    // --- hmtx: every glyph gets a full metric record. The "the last
    // advance repeats" compression the original uses saves nothing at this
    // size and would have to be recomputed against the new order anyway.
    let mut new_hmtx = Vec::with_capacity(n * 4);
    for &g in &old_gids {
        let g = g as usize;
        let (advance, lsb) = if g < num_h_metrics {
            (u16at(hmtx, g * 4)?, i16at(hmtx, g * 4 + 2)?)
        } else {
            (
                u16at(hmtx, (num_h_metrics - 1) * 4)?,
                i16at(hmtx, num_h_metrics * 4 + (g - num_h_metrics) * 2)?,
            )
        };
        push_u16(&mut new_hmtx, advance);
        push_u16(&mut new_hmtx, lsb as u16);
    }
    tables.push((*b"hmtx", new_hmtx));

    tables.push((*b"cmap", write_cmap(&mapping)?));

    // --- post: version 3.0 drops the glyph-name array (32 KB in Inter) and
    // is what every web font ships; the names serve PostScript printing.
    let mut new_post = match sfnt.get(b"post") {
        Some(p) if p.len() >= 32 => p[..32].to_vec(),
        _ => vec![0u8; 32],
    };
    new_post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    tables.push((*b"post", new_post));

    if let Some(name) = sfnt.get(b"name") {
        tables.push((*b"name", subset_name(name).unwrap_or_else(|| name.to_vec())));
    }

    // --- outlines.
    if let (Some(glyf), Some(loca)) = (glyf, loca.as_ref()) {
        let mut glyf_out: Vec<u8> = Vec::new();
        let mut loca_out: Vec<u8> = Vec::new();
        for &g in &old_gids {
            push_u32(&mut loca_out, glyf_out.len() as u32);
            let (s, e) = (loca[g as usize] as usize, loca[g as usize + 1] as usize);
            if e <= s {
                continue; // an empty glyph, e.g. the space
            }
            let mut gd = slice(glyf, s, e - s)?.to_vec();
            for pos in component_positions(&gd)? {
                let c = u16at(&gd, pos)?;
                gd[pos..pos + 2].copy_from_slice(&new_of.get(&c)?.to_be_bytes());
            }
            glyf_out.extend_from_slice(&gd);
            while !glyf_out.len().is_multiple_of(4) {
                glyf_out.push(0);
            }
        }
        push_u32(&mut loca_out, glyf_out.len() as u32);
        tables.push((*b"glyf", glyf_out));
        tables.push((*b"loca", loca_out));

        if let Some(gvar) = sfnt.get(b"gvar") {
            tables.push((*b"gvar", subset_gvar(gvar, &old_gids)?));
        }
        if let Some(hvar) = sfnt.get(b"HVAR") {
            tables.push((*b"HVAR", subset_hvar(hvar, &old_gids, num_glyphs)?));
        }
    } else if let Some(cff) = sfnt.get(b"CFF ") {
        tables.push((*b"CFF ", subset_cff(cff, &old_gids)?));
    } else {
        return None;
    }

    for tag in COPIED {
        if let Some(t) = sfnt.get(tag) {
            tables.push((**tag, t.to_vec()));
        }
    }

    Some(assemble(sfnt.version, tables))
}
