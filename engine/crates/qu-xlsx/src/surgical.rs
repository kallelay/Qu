//! Charts, conditional formatting and data validation, written into the
//! saved package part by part rather than through the `umya` model.
//!
//! `umya-spreadsheet` rebuilds the whole file from its model on save, which
//! is the right tool for cell edits and the wrong one for everything else a
//! workbook holds: whatever it does not model is lost or normalised. So
//! these three are recorded as pending edits and applied to the package at
//! save time with `qu_ooxml`, touching only the parts they must: the sheet,
//! its relationships, a drawing and a chart part, `styles.xml` for a
//! highlight colour, `[Content_Types].xml`. When no cell was edited the
//! package the edits land on is the ORIGINAL file, so every other part
//! (theme, customXml, printer settings, vendor extensions) is written back
//! byte for byte.

use qu_ooxml::chart::{self, ChartKind, ChartSpec, Data, Labels, XData};
use qu_ooxml::xml::{Doc, Element, Node};
use qu_ooxml::{relative_target, resolve_target, Package, REL_OFFICE_DOC};

pub const REL_DRAWING: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
pub const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
pub const CT_DRAWING: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
const NS_XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";

/// A rectangular range on a named sheet, 1-based, normalised.
#[derive(Clone, Debug, PartialEq)]
pub struct Rng {
    pub sheet: String,
    pub c1: u32,
    pub r1: u32,
    pub c2: u32,
    pub r2: u32,
}

impl Rng {
    pub fn len(&self) -> usize {
        ((self.c2 - self.c1 + 1) * (self.r2 - self.r1 + 1)) as usize
    }

    pub fn is_line(&self) -> bool {
        self.c1 == self.c2 || self.r1 == self.r2
    }

    /// `A1:B3` (no sheet, no anchors) -- the form `sqref` takes.
    pub fn a1(&self) -> String {
        let (a, b) = (format!("{}{}", crate::workbook::column_letters(self.c1), self.r1), format!("{}{}", crate::workbook::column_letters(self.c2), self.r2));
        if a == b {
            a
        } else {
            format!("{a}:{b}")
        }
    }

    /// `'My Data'!$A$1:$B$3` -- the form a chart series or a list
    /// validation refers to cells by.
    pub fn absolute(&self) -> String {
        let cell = |c: u32, r: u32| format!("${}${}", crate::workbook::column_letters(c), r);
        let body = if (self.c1, self.r1) == (self.c2, self.r2) { cell(self.c1, self.r1) } else { format!("{}:{}", cell(self.c1, self.r1), cell(self.c2, self.r2)) };
        format!("{}!{body}", quote_sheet(&self.sheet))
    }

    /// Cells in reading order (row by row), as (col, row).
    pub fn cells(&self) -> Vec<(u32, u32)> {
        (self.r1..=self.r2).flat_map(|r| (self.c1..=self.c2).map(move |c| (c, r))).collect()
    }
}

/// A sheet name as a formula writes it: quoted unless it is a plain word.
pub fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        // `A1`, `R1C1`-looking names must be quoted too.
        && crate::workbook::parse_cell(name).is_err();
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// `"B2:B40"`, `"Data!B2:B40"`, `"'My data'!B2:B40"` -> a range, on
/// `default_sheet` when the text names none.
pub fn parse_ref(text: &str, default_sheet: &str) -> Result<Rng, String> {
    let t = text.trim();
    let (sheet, cells) = match t.rfind('!') {
        Some(i) => {
            let s = &t[..i];
            let s = if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') { s[1..s.len() - 1].replace("''", "'") } else { s.to_string() };
            (s, &t[i + 1..])
        }
        None => (default_sheet.to_string(), t),
    };
    let (c1, r1, c2, r2) = crate::workbook::parse_range(cells)?;
    Ok(Rng { sheet, c1, r1, c2, r2 })
}

// ------------------------------------------------------------------ requests

#[derive(Clone, Debug)]
pub struct SeriesReq {
    pub name: Option<String>,
    pub x: Option<Rng>,
    pub y: Rng,
    pub color: Option<String>,
    /// The cached values (and the axis extents) come from minus this range
    /// instead of from `y` -- for a helper column of `=-B2` formulas,
    /// which have no value until a spreadsheet program calculates them.
    pub negated_from: Option<Rng>,
}

#[derive(Clone, Debug)]
pub struct ChartReq {
    pub sheet: String,
    pub kind: ChartKind,
    pub title: Option<String>,
    pub x_title: Option<String>,
    pub y_title: Option<String>,
    pub series: Vec<SeriesReq>,
    /// Top-left cell, (col, row) 1-based.
    pub at: (u32, u32),
    pub width_mm: f64,
    pub height_mm: f64,
    pub lines: bool,
    pub markers: bool,
    pub x_min: Option<f64>,
    pub x_max: Option<f64>,
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub x_log: bool,
    pub y_log: bool,
    pub legend: bool,
    /// One data unit is the same length along both axes (Nyquist plots).
    pub equal_axes: bool,
}

/// The value a comparison rule compares against.
#[derive(Clone, Debug, PartialEq)]
pub enum CfValue {
    Num(f64),
    Text(String),
    /// An Excel formula, without the `=`.
    Formula(String),
}

impl CfValue {
    fn formula(&self) -> String {
        match self {
            CfValue::Num(x) => chart::num(*x),
            CfValue::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            CfValue::Formula(f) => f.trim().trim_start_matches('=').to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CfRule {
    /// `cellIs` with an operator (`greaterThan`, `between`, ...).
    Cell { op: &'static str, a: CfValue, b: Option<CfValue> },
    /// Text contains (case-insensitive, as Excel's rule is).
    Contains(String),
    /// Two or three `#rrggbb` colours from the lowest to the highest value.
    Scale(Vec<String>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CfStyle {
    pub fill: Option<String>,
    pub color: Option<String>,
    pub bold: bool,
}

#[derive(Clone, Debug)]
pub struct CondReq {
    pub range: Rng,
    pub rule: CfRule,
    pub style: CfStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValKind {
    List(Vec<String>),
    ListFrom(Rng),
    /// (`whole`|`decimal`, min, max); a missing bound is open.
    Number(&'static str, Option<f64>, Option<f64>),
    Custom(String),
}

#[derive(Clone, Debug)]
pub struct ValidReq {
    pub range: Rng,
    pub kind: ValKind,
    pub prompt: Option<String>,
    pub prompt_title: Option<String>,
    pub error: Option<String>,
    pub error_title: Option<String>,
    /// `stop`, `warning` or `information`.
    pub error_style: String,
    pub allow_blank: bool,
}

/// An Excel Table (a named, banded range with filter buttons).
#[derive(Clone, Debug)]
pub struct TableReq {
    /// Header row + data rows (the totals row, if any, is the row below).
    pub range: Rng,
    pub name: String,
    /// The first row holds column names.
    pub header: bool,
    /// `TableStyleMedium2`, ...; None for no style.
    pub style: Option<String>,
    pub stripes: bool,
    /// A totals row sits below `range`; the function for numeric columns.
    pub total: Option<TotalFn>,
    /// Columns (0-based) that get a totals function.
    pub total_cols: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TotalFn {
    Sum,
    Average,
    Count,
    Min,
    Max,
}

impl TotalFn {
    pub fn parse(s: &str) -> Result<TotalFn, String> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "sum" => TotalFn::Sum,
            "average" | "mean" | "avg" => TotalFn::Average,
            "count" => TotalFn::Count,
            "min" => TotalFn::Min,
            "max" => TotalFn::Max,
            other => return Err(format!("total=\"{other}\" -- use sum, average, count, min or max")),
        })
    }

    /// The `totalsRowFunction` attribute value.
    fn attr(self) -> &'static str {
        match self {
            TotalFn::Sum => "sum",
            TotalFn::Average => "average",
            TotalFn::Count => "count",
            TotalFn::Min => "min",
            TotalFn::Max => "max",
        }
    }

    /// The SUBTOTAL function number that ignores hidden rows.
    pub fn subtotal(self) -> u32 {
        match self {
            TotalFn::Average => 101,
            TotalFn::Count => 102,
            TotalFn::Max => 104,
            TotalFn::Min => 105,
            TotalFn::Sum => 109,
        }
    }
}

/// A cell note.
#[derive(Clone, Debug)]
pub struct CommentReq {
    pub sheet: String,
    /// (col, row), 1-based.
    pub cell: (u32, u32),
    pub text: String,
    pub author: String,
}

/// A picture placed on a sheet.
#[derive(Clone, Debug)]
pub struct ImageReq {
    pub sheet: String,
    pub bytes: Vec<u8>,
    pub at: (u32, u32),
    pub width_mm: f64,
    pub height_mm: f64,
    pub alt: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Pending {
    Chart(ChartReq),
    Cond(CondReq),
    Valid(ValidReq),
    Table(TableReq),
    Comment(CommentReq),
    Image(ImageReq),
}

impl Pending {
    pub fn sheet(&self) -> &str {
        match self {
            Pending::Chart(c) => &c.sheet,
            Pending::Cond(c) => &c.range.sheet,
            Pending::Valid(v) => &v.range.sheet,
            Pending::Table(t) => &t.range.sheet,
            Pending::Comment(c) => &c.sheet,
            Pending::Image(i) => &i.sheet,
        }
    }

    /// Every range the edit mentions, for renaming a sheet under it.
    pub fn ranges_mut(&mut self) -> Vec<&mut Rng> {
        match self {
            Pending::Chart(c) => c.series.iter_mut().flat_map(|s| [Some(&mut s.y), s.x.as_mut(), s.negated_from.as_mut()].into_iter().flatten()).collect(),
            Pending::Cond(c) => vec![&mut c.range],
            Pending::Valid(v) => {
                let mut out = vec![&mut v.range];
                if let ValKind::ListFrom(r) = &mut v.kind {
                    out.push(r);
                }
                out
            }
            Pending::Table(t) => vec![&mut t.range],
            Pending::Comment(_) | Pending::Image(_) => Vec::new(),
        }
    }

    /// The same edit for a copy of sheet `from` named `to`: ranges on
    /// `from` follow the copy, ranges on other sheets stay. A table gets
    /// `table_name` (names are unique in a workbook).
    pub fn copy_to_sheet(&self, from: &str, to: &str, table_name: &mut dyn FnMut(&str) -> String) -> Pending {
        let mut p = self.clone();
        match &mut p {
            Pending::Chart(c) => c.sheet = to.to_string(),
            Pending::Comment(c) => c.sheet = to.to_string(),
            Pending::Image(i) => i.sheet = to.to_string(),
            Pending::Table(t) => t.name = table_name(&t.name),
            _ => {}
        }
        for r in p.ranges_mut() {
            if r.sheet == from {
                r.sheet = to.to_string();
            }
        }
        p
    }

    pub fn rename_sheet(&mut self, old: &str, new: &str) {
        match self {
            Pending::Chart(c) if c.sheet == old => c.sheet = new.to_string(),
            Pending::Comment(c) if c.sheet == old => c.sheet = new.to_string(),
            Pending::Image(i) if i.sheet == old => i.sheet = new.to_string(),
            _ => {}
        }
        for r in self.ranges_mut() {
            if r.sheet == old {
                r.sheet = new.to_string();
            }
        }
    }
}

pub fn hex6(c: &str) -> Result<String, String> {
    let h = c.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(h.to_ascii_uppercase())
    } else {
        Err(format!("color \"{c}\" -- use #rrggbb"))
    }
}

// ------------------------------------------------------------------ package helpers

/// The prefix an element name carries (`x:` in `x:worksheet`), or "".
fn prefix_of(name: &str) -> &str {
    match name.find(':') {
        Some(i) => &name[..=i],
        None => "",
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Insert `new` among `root`'s children at the place the schema sequence
/// `order` (local names) puts it: after every child that must precede it.
fn insert_ordered(root: &mut Element, new: Element, order: &[&str]) {
    let rank = |n: &str| order.iter().position(|o| *o == local(n));
    let mine = rank(&new.name).expect("the new element is in its own schema order");
    let pos = root
        .children
        .iter()
        .position(|n| matches!(n, Node::Elem(e) if rank(&e.name).is_some_and(|r| r > mine)))
        .unwrap_or(root.children.len());
    root.children.insert(pos, Node::Elem(new));
}

/// CT_Worksheet's child sequence.
const SHEET_ORDER: &[&str] = &[
    "sheetPr", "dimension", "sheetViews", "sheetFormatPr", "cols", "sheetData", "sheetCalcPr", "sheetProtection", "protectedRanges", "scenarios",
    "autoFilter", "sortState", "dataConsolidate", "customSheetViews", "mergeCells", "phoneticPr", "conditionalFormatting", "dataValidations",
    "hyperlinks", "printOptions", "pageMargins", "pageSetup", "headerFooter", "rowBreaks", "colBreaks", "customProperties", "cellWatches",
    "ignoredErrors", "smartTags", "drawing", "legacyDrawing", "legacyDrawingHF", "drawingHF", "picture", "oleObjects", "controls",
    "webPublishItems", "tableParts", "extLst",
];

/// CT_Stylesheet's child sequence.
const STYLES_ORDER: &[&str] = &["numFmts", "fonts", "fills", "borders", "cellStyleXfs", "cellXfs", "cellStyles", "dxfs", "tableStyles", "colors", "extLst"];

fn workbook_part(pkg: &Package) -> Result<String, String> {
    let rel = pkg.rels("")?.into_iter().find(|r| r.rel_type == REL_OFFICE_DOC).ok_or("the package has no workbook part")?;
    Ok(resolve_target("", &rel.target))
}

/// The part holding worksheet `name`.
pub fn sheet_part(pkg: &Package, name: &str) -> Result<String, String> {
    let wb = workbook_part(pkg)?;
    let doc = pkg.get_xml(&wb)?;
    let sheets: Vec<&Element> = doc.root.find_all("sheet").into_iter().chain(doc.root.find_all(&format!("{}sheet", prefix_of(&doc.root.name)))).collect();
    let el = sheets.iter().find(|e| e.attr("name") == Some(name)).ok_or_else(|| format!("no sheet named `{name}` in the saved workbook"))?;
    let rid = el.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone()).ok_or_else(|| format!("sheet `{name}` has no relationship id"))?;
    let rel = pkg.rels(&wb)?.into_iter().find(|r| r.id == rid).ok_or_else(|| format!("sheet `{name}`: relationship {rid} is missing"))?;
    Ok(resolve_target(&wb, &rel.target))
}

fn styles_part(pkg: &Package) -> Result<String, String> {
    let wb = workbook_part(pkg)?;
    let rel = pkg.rels(&wb)?.into_iter().find(|r| r.rel_type == REL_STYLES).ok_or("the workbook has no styles part")?;
    Ok(resolve_target(&wb, &rel.target))
}

fn ensure_r_namespace(root: &mut Element) {
    if root.attr("xmlns:r").is_none() {
        root.set_attr("xmlns:r", qu_ooxml::chart::NS_R);
    }
}

// ------------------------------------------------------------------ apply

/// Cell values for chart caches and axis extents.
pub trait Cells {
    fn number(&self, sheet: &str, col: u32, row: u32) -> Option<f64>;
    fn text(&self, sheet: &str, col: u32, row: u32) -> String;
    /// Column width in Excel's character units.
    fn col_width(&self, sheet: &str, col: u32) -> f64;
    /// Row height in points.
    fn row_height(&self, sheet: &str, row: u32) -> f64;
}

/// Where a `w_mm` x `h_mm` object whose top-left is at 1-based (col, row)
/// ends: the 0-based (col, colOff EMU, row, rowOff EMU) of its bottom-right
/// corner, walking the sheet's actual column widths and row heights.
///
/// A two-cell anchor is used rather than a one-cell anchor with an
/// explicit size because it is the form every reader keeps: `umya` (and
/// with it any later cell edit through `xlsx.open`) silently drops a chart
/// frame in a one-cell anchor.
pub fn anchor_end(cells: &dyn Cells, sheet: &str, (col, row): (u32, u32), w_mm: f64, h_mm: f64) -> (u32, i64, u32, i64) {
    // Excel's column width -> pixels at the default font (7 px per
    // character plus 5 px of padding), 9525 EMU per pixel at 96 dpi.
    let col_emu = |c: u32| ((cells.col_width(sheet, c) * 7.0 + 5.0).trunc().max(0.0) * 9525.0) as i64;
    let row_emu = |r: u32| (cells.row_height(sheet, r) * 12700.0).round() as i64;
    let (mut c, mut left) = (col, qu_ooxml::mm_to_emu(w_mm));
    while c < 16384 && left >= col_emu(c) && col_emu(c) > 0 {
        left -= col_emu(c);
        c += 1;
    }
    let (mut r, mut down) = (row, qu_ooxml::mm_to_emu(h_mm));
    while r < 1_048_576 && down >= row_emu(r) && row_emu(r) > 0 {
        down -= row_emu(r);
        r += 1;
    }
    (c - 1, left, r - 1, down)
}

pub fn apply(pkg: &mut Package, p: &Pending, cells: &dyn Cells) -> Result<(), String> {
    match p {
        Pending::Chart(c) => add_chart(pkg, c, cells),
        Pending::Cond(c) => add_cond(pkg, c),
        Pending::Valid(v) => add_validation(pkg, v),
        Pending::Table(t) => add_table(pkg, t, cells),
        Pending::Comment(c) => add_comment(pkg, c),
        Pending::Image(i) => add_image(pkg, i, cells),
    }
}

fn numbers(r: &Rng, cells: &dyn Cells, scale: f64) -> Vec<Option<f64>> {
    r.cells().into_iter().map(|(c, row)| cells.number(&r.sheet, c, row).map(|x| x * scale)).collect()
}

/// The chart part for a request, with the plot area fixed when the axes
/// must share a scale.
pub fn chart_spec(c: &ChartReq, cells: &dyn Cells) -> Result<ChartSpec, String> {
    let mut spec = ChartSpec::new(c.kind);
    spec.title = c.title.clone();
    spec.lines = c.lines;
    spec.markers = c.markers;
    spec.legend = c.legend.then_some(chart::LegendPos::Right);
    spec.x_axis.title = c.x_title.clone();
    spec.y_axis.title = c.y_title.clone();
    spec.x_axis.log = c.x_log;
    spec.y_axis.log = c.y_log;
    (spec.x_axis.min, spec.x_axis.max, spec.y_axis.min, spec.y_axis.max) = (c.x_min, c.x_max, c.y_min, c.y_max);
    let (mut all_x, mut all_y) = (Vec::new(), Vec::new());
    for s in &c.series {
        let y_cache = match &s.negated_from {
            Some(src) => numbers(src, cells, -1.0),
            None => numbers(&s.y, cells, 1.0),
        };
        let x = match &s.x {
            None => None,
            Some(r) if c.kind == ChartKind::Scatter => {
                let xs = numbers(r, cells, 1.0);
                all_x.extend(xs.iter().flatten().copied());
                Some(XData::Num(Data::Ref { formula: r.absolute(), cache: xs }))
            }
            Some(r) => Some(XData::Text(Labels::Ref { formula: r.absolute(), cache: r.cells().into_iter().map(|(cc, rr)| cells.text(&r.sheet, cc, rr)).collect() })),
        };
        if s.x.is_none() {
            all_x.extend((1..=y_cache.len()).map(|i| i as f64));
        }
        all_y.extend(y_cache.iter().flatten().copied());
        spec.series.push(chart::Series { name: s.name.clone(), x, y: Some(Data::Ref { formula: s.y.absolute(), cache: y_cache }), color: s.color.clone() });
    }
    if c.equal_axes {
        // The chart box in mm, less room for the title, tick labels and
        // axis titles; the plot rectangle is fitted inside what is left.
        let top = if c.title.is_some() { 14.0 } else { 6.0 };
        let (left, right, bottom) = (22.0, if c.legend { 32.0 } else { 6.0 }, 18.0);
        let (bw, bh) = (c.width_mm - left - right, c.height_mm - top - bottom);
        let ((x0, x1), (y0, y1), step, (rx, ry, rw, rh)) = chart::equal_scale(&all_x, &all_y, bw, bh)?;
        spec.x_axis.min = Some(x0);
        spec.x_axis.max = Some(x1);
        spec.y_axis.min = Some(y0);
        spec.y_axis.max = Some(y1);
        spec.x_axis.major_unit = Some(step);
        spec.y_axis.major_unit = Some(step);
        spec.x_axis.gridlines = true;
        spec.plot_area = Some(((left + rx) / c.width_mm, (top + ry) / c.height_mm, rw / c.width_mm, rh / c.height_mm));
    }
    Ok(spec)
}

/// The drawing part of the sheet at `sheet` (the part path): the one it
/// has, or a new empty one wired into the sheet and the content types.
fn ensure_drawing(pkg: &mut Package, sheet: &str) -> Result<String, String> {
    let mut ws = pkg.get_xml(sheet)?;
    let px = prefix_of(&ws.root.name).to_string();
    let existing = ws.root.elems().find(|e| local(&e.name) == "drawing").and_then(|e| e.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone()));
    Ok(match existing {
        Some(rid) => {
            let rel = pkg.rels(sheet)?.into_iter().find(|r| r.id == rid).ok_or_else(|| format!("the sheet's drawing relationship {rid} is missing"))?;
            resolve_target(sheet, &rel.target)
        }
        None => {
            let d = pkg.next_name("xl/drawings/drawing", ".xml");
            let root = Element::new("xdr:wsDr").with_attr("xmlns:xdr", NS_XDR).with_attr("xmlns:a", chart::NS_A);
            pkg.set_xml(&d, &Doc::new(root));
            pkg.add_override(&d, CT_DRAWING)?;
            let rid = pkg.add_rel(sheet, REL_DRAWING, &relative_target(sheet, &d))?;
            ensure_r_namespace(&mut ws.root);
            insert_ordered(&mut ws.root, Element::new(&format!("{px}drawing")).with_attr("r:id", &rid), SHEET_ORDER);
            pkg.set_xml(sheet, &ws);
            d
        }
    })
}

/// A shape id not yet used in the drawing rooted at `root`.
fn next_shape_id(root: &Element, dp: &str) -> u32 {
    root.find_all(&format!("{dp}cNvPr")).iter().filter_map(|e| e.attr("id")?.parse::<u32>().ok()).max().unwrap_or(1) + 1
}

/// `<xdr:twoCellAnchor editAs="oneCell">` with its from/to markers for an
/// object whose top-left is at 1-based (col, row) and that is `w_mm` x
/// `h_mm` big; the caller adds the object and `clientData`.
fn two_cell_anchor(dp: &str, cells: &dyn Cells, sheet: &str, at: (u32, u32), w_mm: f64, h_mm: f64) -> Element {
    let x = |n: &str| Element::new(&format!("{dp}{n}"));
    let (tc, tco, tr, tro) = anchor_end(cells, sheet, at, w_mm, h_mm);
    let marker = |tag: &str, c: u32, co: i64, r: u32, ro: i64| {
        x(tag)
            .with_child(x("col").with_text(&c.to_string()))
            .with_child(x("colOff").with_text(&co.to_string()))
            .with_child(x("row").with_text(&r.to_string()))
            .with_child(x("rowOff").with_text(&ro.to_string()))
    };
    x("twoCellAnchor").with_attr("editAs", "oneCell").with_child(marker("from", at.0 - 1, 0, at.1 - 1, 0)).with_child(marker("to", tc, tco, tr, tro))
}

fn add_chart(pkg: &mut Package, c: &ChartReq, cells: &dyn Cells) -> Result<(), String> {
    let spec = chart_spec(c, cells)?;
    let sheet = sheet_part(pkg, &c.sheet)?;
    let chart_path = pkg.next_name("xl/charts/chart", ".xml");
    pkg.set(&chart_path, chart::chart_part(&spec)?.into_bytes());
    pkg.add_override(&chart_path, chart::CT_CHART)?;

    // The sheet's drawing: the one it has, or a new one.
    let drawing = ensure_drawing(pkg, &sheet)?;
    let chart_rid = pkg.add_rel(&drawing, chart::REL_CHART, &relative_target(&drawing, &chart_path))?;
    let mut dr = pkg.get_xml(&drawing)?;
    let dp = prefix_of(&dr.root.name).to_string();
    if dr.root.attr("xmlns:a").is_none() {
        dr.root.set_attr("xmlns:a", chart::NS_A);
    }
    let next_id = next_shape_id(&dr.root, &dp);
    let n_charts = dr.root.find_all("c:chart").len() + 1;
    let x = |n: &str| Element::new(&format!("{dp}{n}"));
    let anchor = two_cell_anchor(&dp, cells, &c.sheet, c.at, c.width_mm, c.height_mm)
        .with_child(
            x("graphicFrame")
                .with_attr("macro", "")
                .with_child(
                    x("nvGraphicFramePr")
                        .with_child(x("cNvPr").with_attr("id", &next_id.to_string()).with_attr("name", &format!("Chart {n_charts}")))
                        .with_child(x("cNvGraphicFramePr")),
                )
                .with_child(
                    x("xfrm")
                        .with_child(Element::new("a:off").with_attr("x", "0").with_attr("y", "0"))
                        .with_child(Element::new("a:ext").with_attr("cx", "0").with_attr("cy", "0")),
                )
                .with_child(chart::graphic(&chart_rid)),
        )
        .with_child(x("clientData"));
    dr.root.children.push(Node::Elem(anchor));
    pkg.set_xml(&drawing, &dr);
    Ok(())
}

/// The largest cfRule priority already on the sheet (x14 extension rules
/// included: priorities are shared across both).
fn max_priority(root: &Element) -> u32 {
    let mut best = 0;
    root.clone().walk_mut(&mut |e| {
        if local(&e.name) == "cfRule" {
            if let Some(p) = e.attr("priority").and_then(|p| p.parse::<u32>().ok()) {
                best = best.max(p);
            }
        }
    });
    best
}

/// Append a differential format (the highlight a rule applies) to
/// styles.xml and return its index.
fn add_dxf(pkg: &mut Package, st: &CfStyle) -> Result<usize, String> {
    let part = styles_part(pkg)?;
    let mut doc = pkg.get_xml(&part)?;
    let px = prefix_of(&doc.root.name).to_string();
    let x = |n: &str| Element::new(&format!("{px}{n}"));
    if !doc.root.elems().any(|e| local(&e.name) == "dxfs") {
        insert_ordered(&mut doc.root, x("dxfs").with_attr("count", "0"), STYLES_ORDER);
    }
    let dxfs = doc.root.elems_mut().find(|e| local(&e.name) == "dxfs").unwrap();
    let mut dxf = x("dxf");
    if st.bold || st.color.is_some() {
        let mut font = x("font");
        if st.bold {
            font = font.with_child(x("b"));
        }
        if let Some(c) = &st.color {
            font = font.with_child(x("color").with_attr("rgb", &format!("FF{}", hex6(c)?)));
        }
        dxf = dxf.with_child(font);
    }
    if let Some(f) = &st.fill {
        let rgb = format!("FF{}", hex6(f)?);
        dxf = dxf.with_child(x("fill").with_child(x("patternFill").with_attr("patternType", "solid").with_child(x("fgColor").with_attr("rgb", &rgb)).with_child(x("bgColor").with_attr("rgb", &rgb))));
    }
    let idx = dxfs.elems().filter(|e| local(&e.name) == "dxf").count();
    dxfs.children.push(Node::Elem(dxf));
    dxfs.set_attr("count", &(idx + 1).to_string());
    pkg.set_xml(&part, &doc);
    Ok(idx)
}

fn add_cond(pkg: &mut Package, c: &CondReq) -> Result<(), String> {
    let sheet = sheet_part(pkg, &c.range.sheet)?;
    let dxf = match &c.rule {
        CfRule::Scale(_) => None,
        _ => Some(add_dxf(pkg, &c.style)?),
    };
    let mut ws = pkg.get_xml(&sheet)?;
    let px = prefix_of(&ws.root.name).to_string();
    let x = |n: &str| Element::new(&format!("{px}{n}"));
    let priority = (max_priority(&ws.root) + 1).to_string();
    let top_left = format!("{}{}", crate::workbook::column_letters(c.range.c1), c.range.r1);
    let mut rule = x("cfRule");
    match &c.rule {
        CfRule::Cell { op, a, b } => {
            rule = rule.with_attr("type", "cellIs").with_attr("dxfId", &dxf.unwrap().to_string()).with_attr("priority", &priority).with_attr("operator", op);
            rule = rule.with_child(x("formula").with_text(&a.formula()));
            if let Some(b) = b {
                rule = rule.with_child(x("formula").with_text(&b.formula()));
            }
        }
        CfRule::Contains(t) => {
            let lit = t.replace('"', "\"\"");
            rule = rule
                .with_attr("type", "containsText")
                .with_attr("dxfId", &dxf.unwrap().to_string())
                .with_attr("priority", &priority)
                .with_attr("operator", "containsText")
                .with_attr("text", t)
                .with_child(x("formula").with_text(&format!("NOT(ISERROR(SEARCH(\"{lit}\",{top_left})))")));
        }
        CfRule::Scale(colors) => {
            let mut scale = x("colorScale").with_child(x("cfvo").with_attr("type", "min"));
            if colors.len() == 3 {
                scale = scale.with_child(x("cfvo").with_attr("type", "percentile").with_attr("val", "50"));
            }
            scale = scale.with_child(x("cfvo").with_attr("type", "max"));
            for col in colors {
                scale = scale.with_child(x("color").with_attr("rgb", &format!("FF{}", hex6(col)?)));
            }
            rule = rule.with_attr("type", "colorScale").with_attr("priority", &priority).with_child(scale);
        }
    }
    insert_ordered(&mut ws.root, x("conditionalFormatting").with_attr("sqref", &c.range.a1()).with_child(rule), SHEET_ORDER);
    pkg.set_xml(&sheet, &ws);
    Ok(())
}

fn add_validation(pkg: &mut Package, v: &ValidReq) -> Result<(), String> {
    let sheet = sheet_part(pkg, &v.range.sheet)?;
    let mut ws = pkg.get_xml(&sheet)?;
    let px = prefix_of(&ws.root.name).to_string();
    let x = |n: &str| Element::new(&format!("{px}{n}"));
    let mut dv = x("dataValidation");
    let (ty, op, f1, f2): (&str, Option<&str>, Option<String>, Option<String>) = match &v.kind {
        ValKind::List(items) => ("list", None, Some(format!("\"{}\"", items.join(","))), None),
        ValKind::ListFrom(r) => ("list", None, Some(if r.sheet == v.range.sheet { r.absolute().split_once('!').unwrap().1.to_string() } else { r.absolute() }), None),
        ValKind::Number(t, lo, hi) => match (lo, hi) {
            (Some(a), Some(b)) => (t, Some("between"), Some(chart::num(*a)), Some(chart::num(*b))),
            (Some(a), None) => (t, Some("greaterThanOrEqual"), Some(chart::num(*a)), None),
            (None, Some(b)) => (t, Some("lessThanOrEqual"), Some(chart::num(*b)), None),
            (None, None) => return Err("a number validation needs min=, max= or both".into()),
        },
        ValKind::Custom(f) => ("custom", None, Some(f.trim().trim_start_matches('=').to_string()), None),
    };
    dv.set_attr("type", ty);
    if let Some(op) = op {
        dv.set_attr("operator", op);
    }
    if v.error_style != "stop" {
        dv.set_attr("errorStyle", &v.error_style);
    }
    if v.allow_blank {
        dv.set_attr("allowBlank", "1");
    }
    dv.set_attr("showInputMessage", "1");
    dv.set_attr("showErrorMessage", "1");
    for (k, val) in [("errorTitle", &v.error_title), ("error", &v.error), ("promptTitle", &v.prompt_title), ("prompt", &v.prompt)] {
        if let Some(s) = val {
            dv.set_attr(k, s);
        }
    }
    dv.set_attr("sqref", &v.range.a1());
    if let Some(f) = f1 {
        dv = dv.with_child(x("formula1").with_text(&f));
    }
    if let Some(f) = f2 {
        dv = dv.with_child(x("formula2").with_text(&f));
    }
    if !ws.root.elems().any(|e| local(&e.name) == "dataValidations") {
        insert_ordered(&mut ws.root, x("dataValidations").with_attr("count", "0"), SHEET_ORDER);
    }
    let dvs = ws.root.elems_mut().find(|e| local(&e.name) == "dataValidations").unwrap();
    dvs.children.push(Node::Elem(dv));
    let n = dvs.elems().count();
    dvs.set_attr("count", &n.to_string());
    pkg.set_xml(&sheet, &ws);
    Ok(())
}

// ------------------------------------------------------------------ tables

pub const REL_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
pub const CT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";
const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// Excel's Table names: letters, digits, `_` and `.`; start with a letter or
/// `_`; no spaces; not a cell address (`A1`, `R1C1`).
pub fn check_table_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().count() <= 255
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
    if !ok {
        return Err(format!("table name \"{name}\" -- letters, digits, `_` and `.` only, starting with a letter or `_`, no spaces"));
    }
    let lower = name.to_ascii_lowercase();
    let r1c1 = lower.strip_prefix('r').is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == 'c') && rest.contains(|c: char| c.is_ascii_digit()));
    if crate::workbook::parse_cell(name).is_ok() || r1c1 || lower == "r" || lower == "c" {
        return Err(format!("table name \"{name}\" looks like a cell address -- pick another"));
    }
    Ok(())
}

/// `"medium2"`, `"Medium2"` or `"TableStyleMedium2"` -> `TableStyleMedium2`
/// (light 1-21, medium 1-28, dark 1-11); `"none"` -> None.
pub fn table_style(name: &str) -> Result<Option<String>, String> {
    let t = name.trim();
    if t.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let short = if t.len() > 10 && t[..10].eq_ignore_ascii_case("TableStyle") { &t[10..] } else { t };
    let lower = short.to_ascii_lowercase();
    for (kind, label, max) in [("light", "Light", 21u32), ("medium", "Medium", 28), ("dark", "Dark", 11)] {
        if let Some(n) = lower.strip_prefix(kind).and_then(|d| d.parse::<u32>().ok()) {
            if (1..=max).contains(&n) {
                return Ok(Some(format!("TableStyle{label}{n}")));
            }
        }
    }
    Err(format!("table style \"{name}\" -- use light1-21, medium1-28, dark1-11 (or TableStyleMedium2 ...), or \"none\""))
}

/// Column names that Excel accepts: empty ones become `Column{n}`, repeats
/// get a number (`Sales`, `Sales2`) -- compared case-insensitively, as
/// Excel does.
pub fn normalize_headers(names: &mut [String]) {
    let mut seen: Vec<String> = Vec::new();
    for (i, n) in names.iter_mut().enumerate() {
        let mut cand = if n.trim().is_empty() { format!("Column{}", i + 1) } else { n.clone() };
        let base = cand.clone();
        let mut k = 2;
        while seen.contains(&cand.to_lowercase()) {
            cand = format!("{base}{k}");
            k += 1;
        }
        seen.push(cand.to_lowercase());
        *n = cand;
    }
}

fn add_table(pkg: &mut Package, t: &TableReq, cells: &dyn Cells) -> Result<(), String> {
    let sheet = sheet_part(pkg, &t.range.sheet)?;
    let r = &t.range;
    let ncols = (r.c2 - r.c1 + 1) as usize;
    let mut names: Vec<String> = if t.header { (r.c1..=r.c2).map(|c| cells.text(&r.sheet, c, r.r1)).collect() } else { vec![String::new(); ncols] };
    normalize_headers(&mut names);
    // Names and ids are workbook-wide: look at the tables already there.
    let (mut max_id, mut taken) = (0u32, Vec::new());
    for part in pkg.names().into_iter().filter(|n| n.starts_with("xl/tables/table") && n.ends_with(".xml")) {
        let d = pkg.get_xml(&part)?;
        max_id = max_id.max(d.root.attr("id").and_then(|i| i.parse().ok()).unwrap_or(0));
        taken.extend(d.root.attr("name").map(|s| s.to_lowercase()));
        taken.extend(d.root.attr("displayName").map(|s| s.to_lowercase()));
    }
    if taken.contains(&t.name.to_lowercase()) {
        return Err(format!("the workbook already has a table named `{}`", t.name));
    }
    let last = if t.total.is_some() { r.r2 + 1 } else { r.r2 };
    let full = Rng { r2: last, ..r.clone() }.a1();
    let mut table = Element::new("table")
        .with_attr("xmlns", NS_MAIN)
        .with_attr("id", &(max_id + 1).to_string())
        .with_attr("name", &t.name)
        .with_attr("displayName", &t.name)
        .with_attr("ref", &full);
    if !t.header {
        table.set_attr("headerRowCount", "0");
    }
    if t.total.is_some() {
        table.set_attr("totalsRowCount", "1");
    }
    if t.header {
        table = table.with_child(Element::new("autoFilter").with_attr("ref", &r.a1()));
    }
    let mut cols = Element::new("tableColumns").with_attr("count", &ncols.to_string());
    for (i, n) in names.iter().enumerate() {
        let mut c = Element::new("tableColumn").with_attr("id", &(i + 1).to_string()).with_attr("name", n);
        if let Some(f) = t.total {
            if t.total_cols.contains(&i) {
                c.set_attr("totalsRowFunction", f.attr());
            } else if i == 0 {
                c.set_attr("totalsRowLabel", "Total");
            }
        }
        cols = cols.with_child(c);
    }
    table = table.with_child(cols);
    if let Some(style) = &t.style {
        table = table.with_child(
            Element::new("tableStyleInfo")
                .with_attr("name", style)
                .with_attr("showFirstColumn", "0")
                .with_attr("showLastColumn", "0")
                .with_attr("showRowStripes", if t.stripes { "1" } else { "0" })
                .with_attr("showColumnStripes", "0"),
        );
    }
    let part = pkg.next_name("xl/tables/table", ".xml");
    pkg.set_xml(&part, &Doc::new(table));
    pkg.add_override(&part, CT_TABLE)?;
    let rid = pkg.add_rel(&sheet, REL_TABLE, &relative_target(&sheet, &part))?;
    let mut ws = pkg.get_xml(&sheet)?;
    let px = prefix_of(&ws.root.name).to_string();
    ensure_r_namespace(&mut ws.root);
    let tp = Element::new(&format!("{px}tablePart")).with_attr("r:id", &rid);
    let has_parts = ws.root.elems().any(|e| local(&e.name) == "tableParts");
    if has_parts {
        let parts = ws.root.elems_mut().find(|e| local(&e.name) == "tableParts").unwrap();
        parts.children.push(Node::Elem(tp));
        let n = parts.elems().count();
        parts.set_attr("count", &n.to_string());
    } else {
        insert_ordered(&mut ws.root, Element::new(&format!("{px}tableParts")).with_attr("count", "1").with_child(tp), SHEET_ORDER);
    }
    pkg.set_xml(&sheet, &ws);
    Ok(())
}

// ------------------------------------------------------------------ comments

pub const REL_COMMENTS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
pub const REL_VML: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing";
pub const CT_COMMENTS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
pub const CT_VML: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

/// The VML shape Excel draws a note's box with. Hidden until the cell is
/// hovered; sized from the text so a long note is not cut off.
fn vml_shape(id: u32, (col, row): (u32, u32), text: &str) -> String {
    let (c0, r0) = (col - 1, row - 1);
    let lines: usize = text.split('\n').map(|l| l.chars().count().max(1).div_ceil(26)).sum::<usize>().max(2);
    let rows = (lines as u32 + 1).min(60);
    format!(
        "<v:shape id=\"_x0000_s{id}\" type=\"#_x0000_t202\" style=\"position:absolute;margin-left:80pt;margin-top:2pt;width:160pt;height:{h}pt;z-index:{z};visibility:hidden\" fillcolor=\"#ffffe1\" o:insetmode=\"auto\">\
<v:fill color2=\"#ffffe1\"/><v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/>\
<v:textbox style=\"mso-direction-alt:auto\"><div style=\"text-align:left\"></div></v:textbox>\
<x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{l}, 15, {t}, 10, {r}, 15, {b}, 4</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{r0}</x:Row><x:Column>{c0}</x:Column></x:ClientData></v:shape>",
        h = 14 * lines + 8,
        z = id % 1024,
        l = c0 + 1,
        t = r0.saturating_sub(1),
        r = c0 + 4,
        b = r0.saturating_sub(1) + rows,
    )
}

fn add_comment(pkg: &mut Package, c: &CommentReq) -> Result<(), String> {
    let sheet = sheet_part(pkg, &c.sheet)?;
    let (col, row) = c.cell;
    let cell_ref = format!("{}{}", crate::workbook::column_letters(col), row);
    let rels = pkg.rels(&sheet)?;

    // -- the comments part
    let part = match rels.iter().find(|r| r.rel_type == REL_COMMENTS) {
        Some(r) => resolve_target(&sheet, &r.target),
        None => {
            let p = pkg.next_name("xl/comments", ".xml");
            let root = Element::new("comments").with_attr("xmlns", NS_MAIN).with_child(Element::new("authors")).with_child(Element::new("commentList"));
            pkg.set_xml(&p, &Doc::new(root));
            pkg.add_override(&p, CT_COMMENTS)?;
            pkg.add_rel(&sheet, REL_COMMENTS, &relative_target(&sheet, &p))?;
            p
        }
    };
    let mut doc = pkg.get_xml(&part)?;
    let px = prefix_of(&doc.root.name).to_string();
    let x = |n: &str| Element::new(&format!("{px}{n}"));
    let authors = doc.root.elems_mut().find(|e| local(&e.name) == "authors").ok_or("the comments part has no <authors>")?;
    let found = authors.elems().position(|a| a.text() == c.author);
    let author_id = match found {
        Some(i) => i,
        None => {
            authors.children.push(Node::Elem(x("author").with_text(&c.author)));
            authors.elems().count() - 1
        }
    };
    let text = x("text").with_child(x("r").with_child(x("t").with_attr("xml:space", "preserve").with_text(&c.text)));
    let list = doc.root.elems_mut().find(|e| local(&e.name) == "commentList").ok_or("the comments part has no <commentList>")?;
    let existing = list.elems_mut().find(|e| local(&e.name) == "comment" && e.attr("ref").is_some_and(|r| r.eq_ignore_ascii_case(&cell_ref)));
    let mut replaced = false;
    if let Some(e) = existing {
        e.set_attr("authorId", &author_id.to_string());
        e.children = vec![Node::Elem(text.clone())];
        replaced = true;
    }
    if !replaced {
        let new = x("comment").with_attr("ref", &cell_ref).with_attr("authorId", &author_id.to_string()).with_attr("shapeId", "0").with_child(text);
        // Reading order, as Excel writes them.
        let key = |e: &Element| e.attr("ref").and_then(|r| crate::workbook::parse_cell(r).ok()).map(|(c, r)| (r, c));
        let mine = (row, col);
        let pos = list.children.iter().position(|n| matches!(n, Node::Elem(e) if key(e).is_some_and(|k| k > mine))).unwrap_or(list.children.len());
        list.children.insert(pos, Node::Elem(new));
    }
    pkg.set_xml(&part, &doc);

    // -- the legacy VML drawing that draws the note boxes
    let mut ws = pkg.get_xml(&sheet)?;
    let rid = ws.root.elems().find(|e| local(&e.name) == "legacyDrawing").and_then(|e| e.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone()));
    match rid {
        Some(rid) => {
            let rel = rels.iter().find(|r| r.id == rid).ok_or_else(|| format!("the sheet's legacyDrawing relationship {rid} is missing"))?;
            let vml_part = resolve_target(&sheet, &rel.target);
            let mut vml = pkg.get_str(&vml_part)?;
            let marker = format!("<x:Row>{}</x:Row>", row - 1);
            let has_shape = vml.match_indices(&marker).any(|(i, _)| vml[i..].starts_with(&format!("{marker}<x:Column>{}</x:Column>", col - 1)) || vml[i..].starts_with(&format!("{marker}\n<x:Column>{}</x:Column>", col - 1)));
            if !has_shape {
                let max_id = vml.match_indices("_x0000_s").filter_map(|(i, m)| vml[i + m.len()..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok()).max();
                let id = max_id.map(|m| m + 1).unwrap_or(1025);
                let end = vml.rfind("</xml>").ok_or("the sheet's VML drawing is not in the form this edit understands")?;
                vml.insert_str(end, &vml_shape(id, c.cell, &c.text));
                pkg.set(&vml_part, vml.into_bytes());
            }
        }
        None => {
            let n = pkg.names().iter().filter(|n| n.ends_with(".vml")).count() as u32 + 1;
            let vml_part = pkg.next_name("xl/drawings/vmlDrawing", ".vml");
            let vml = format!(
                "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\">\
<o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"{n}\"/></o:shapelayout>\
<v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>{}</xml>",
                vml_shape(n * 1024 + 1, c.cell, &c.text)
            );
            pkg.set(&vml_part, vml.into_bytes());
            pkg.ensure_default_type("vml", CT_VML)?;
            let rid = pkg.add_rel(&sheet, REL_VML, &relative_target(&sheet, &vml_part))?;
            let spx = prefix_of(&ws.root.name).to_string();
            ensure_r_namespace(&mut ws.root);
            insert_ordered(&mut ws.root, Element::new(&format!("{spx}legacyDrawing")).with_attr("r:id", &rid), SHEET_ORDER);
            pkg.set_xml(&sheet, &ws);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ images

fn add_image(pkg: &mut Package, i: &ImageReq, cells: &dyn Cells) -> Result<(), String> {
    let (_, _, ext, _) = qu_ooxml::image_info(&i.bytes)?;
    let ctype = match ext {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        _ => "image/gif",
    };
    let sheet = sheet_part(pkg, &i.sheet)?;
    let media = pkg.next_name("xl/media/image", &format!(".{ext}"));
    pkg.set(&media, i.bytes.clone());
    pkg.ensure_default_type(ext, ctype)?;
    let drawing = ensure_drawing(pkg, &sheet)?;
    let rid = pkg.add_rel(&drawing, qu_ooxml::REL_IMAGE, &relative_target(&drawing, &media))?;
    let mut dr = pkg.get_xml(&drawing)?;
    let dp = prefix_of(&dr.root.name).to_string();
    if dr.root.attr("xmlns:a").is_none() {
        dr.root.set_attr("xmlns:a", chart::NS_A);
    }
    let id = next_shape_id(&dr.root, &dp);
    let n_pics = dr.root.find_all(&format!("{dp}pic")).len() + 1;
    let x = |n: &str| Element::new(&format!("{dp}{n}"));
    let mut cnv = x("cNvPr").with_attr("id", &id.to_string()).with_attr("name", &format!("Picture {n_pics}"));
    if let Some(a) = &i.alt {
        cnv.set_attr("descr", a);
    }
    let (cx, cy) = (qu_ooxml::mm_to_emu(i.width_mm), qu_ooxml::mm_to_emu(i.height_mm));
    let pic = x("pic")
        .with_child(x("nvPicPr").with_child(cnv).with_child(x("cNvPicPr").with_child(Element::new("a:picLocks").with_attr("noChangeAspect", "1"))))
        .with_child(x("blipFill").with_child(Element::new("a:blip").with_attr("xmlns:r", chart::NS_R).with_attr("r:embed", &rid)).with_child(Element::new("a:stretch").with_child(Element::new("a:fillRect"))))
        .with_child(
            x("spPr")
                .with_child(
                    Element::new("a:xfrm")
                        .with_child(Element::new("a:off").with_attr("x", "0").with_attr("y", "0"))
                        .with_child(Element::new("a:ext").with_attr("cx", &cx.to_string()).with_attr("cy", &cy.to_string())),
                )
                .with_child(Element::new("a:prstGeom").with_attr("prst", "rect").with_child(Element::new("a:avLst"))),
        );
    let anchor = two_cell_anchor(&dp, cells, &i.sheet, i.at, i.width_mm, i.height_mm).with_child(pic).with_child(x("clientData"));
    dr.root.children.push(Node::Elem(anchor));
    pkg.set_xml(&drawing, &dr);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_styles_and_headers() {
        assert!(check_table_name("Sales_2024").is_ok() && check_table_name("_x.y").is_ok());
        for bad in ["", "1Sales", "my table", "A1", "R1C1", "r", "C", "a-b"] {
            assert!(check_table_name(bad).is_err(), "{bad} must be refused");
        }
        assert_eq!(table_style("medium2").unwrap().as_deref(), Some("TableStyleMedium2"));
        assert_eq!(table_style("TableStyleLight15").unwrap().as_deref(), Some("TableStyleLight15"));
        assert_eq!(table_style("none").unwrap(), None);
        assert!(table_style("medium29").is_err() && table_style("dark12").is_err() && table_style("fancy").is_err());
        let mut h: Vec<String> = ["a", "A", "", "a", "Column3"].iter().map(|s| s.to_string()).collect();
        normalize_headers(&mut h);
        assert_eq!(h, ["a", "A2", "Column3", "a3", "Column32"]);
    }

    #[test]
    fn references_parse_and_print() {
        let r = parse_ref("'My ''data'''!b2:B9", "S").unwrap();
        assert_eq!((r.sheet.as_str(), r.c1, r.r1, r.r2), ("My 'data'", 2, 2, 9));
        assert_eq!(r.absolute(), "'My ''data'''!$B$2:$B$9");
        assert_eq!(parse_ref("C3", "Sheet1").unwrap().absolute(), "Sheet1!$C$3");
        assert_eq!(quote_sheet("A1"), "'A1'", "a cell-like name is quoted");
        assert_eq!(quote_sheet("Données"), "Données");
        assert_eq!(parse_ref("A1:C2", "S").unwrap().a1(), "A1:C2");
    }

    #[test]
    fn insertion_follows_the_schema_order() {
        let mut root = qu_ooxml::xml::parse("<worksheet><sheetData/><mergeCells/><pageMargins/><drawing/></worksheet>").unwrap().root;
        insert_ordered(&mut root, Element::new("conditionalFormatting"), SHEET_ORDER);
        insert_ordered(&mut root, Element::new("dataValidations"), SHEET_ORDER);
        insert_ordered(&mut root, Element::new("conditionalFormatting").with_attr("n", "2"), SHEET_ORDER);
        let names: Vec<String> = root.elems().map(|e| e.name.clone() + e.attr("n").unwrap_or("")).collect();
        assert_eq!(names, ["sheetData", "mergeCells", "conditionalFormatting", "conditionalFormatting2", "dataValidations", "pageMargins", "drawing"]);
    }
}
