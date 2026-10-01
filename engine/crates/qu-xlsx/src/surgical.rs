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

#[derive(Clone, Debug)]
pub enum Pending {
    Chart(ChartReq),
    Cond(CondReq),
    Valid(ValidReq),
}

impl Pending {
    pub fn sheet(&self) -> &str {
        match self {
            Pending::Chart(c) => &c.sheet,
            Pending::Cond(c) => &c.range.sheet,
            Pending::Valid(v) => &v.range.sheet,
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
        }
    }

    pub fn rename_sheet(&mut self, old: &str, new: &str) {
        if let Pending::Chart(c) = self {
            if c.sheet == old {
                c.sheet = new.to_string();
            }
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

fn add_chart(pkg: &mut Package, c: &ChartReq, cells: &dyn Cells) -> Result<(), String> {
    let spec = chart_spec(c, cells)?;
    let sheet = sheet_part(pkg, &c.sheet)?;
    let chart_path = pkg.next_name("xl/charts/chart", ".xml");
    pkg.set(&chart_path, chart::chart_part(&spec)?.into_bytes());
    pkg.add_override(&chart_path, chart::CT_CHART)?;

    // The sheet's drawing: the one it has, or a new one.
    let mut ws = pkg.get_xml(&sheet)?;
    let px = prefix_of(&ws.root.name).to_string();
    let existing = ws.root.elems().find(|e| local(&e.name) == "drawing").and_then(|e| e.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone()));
    let drawing = match existing {
        Some(rid) => {
            let rel = pkg.rels(&sheet)?.into_iter().find(|r| r.id == rid).ok_or_else(|| format!("the sheet's drawing relationship {rid} is missing"))?;
            resolve_target(&sheet, &rel.target)
        }
        None => {
            let d = pkg.next_name("xl/drawings/drawing", ".xml");
            let root = Element::new("xdr:wsDr").with_attr("xmlns:xdr", NS_XDR).with_attr("xmlns:a", chart::NS_A);
            pkg.set_xml(&d, &Doc::new(root));
            pkg.add_override(&d, CT_DRAWING)?;
            let rid = pkg.add_rel(&sheet, REL_DRAWING, &relative_target(&sheet, &d))?;
            ensure_r_namespace(&mut ws.root);
            insert_ordered(&mut ws.root, Element::new(&format!("{px}drawing")).with_attr("r:id", &rid), SHEET_ORDER);
            pkg.set_xml(&sheet, &ws);
            d
        }
    };
    let chart_rid = pkg.add_rel(&drawing, chart::REL_CHART, &relative_target(&drawing, &chart_path))?;
    let mut dr = pkg.get_xml(&drawing)?;
    let dp = prefix_of(&dr.root.name).to_string();
    if dr.root.attr("xmlns:a").is_none() {
        dr.root.set_attr("xmlns:a", chart::NS_A);
    }
    let next_id = dr.root.find_all(&format!("{dp}cNvPr")).iter().filter_map(|e| e.attr("id")?.parse::<u32>().ok()).max().unwrap_or(1) + 1;
    let n_charts = dr.root.find_all("c:chart").len() + 1;
    let x = |n: &str| Element::new(&format!("{dp}{n}"));
    let (col, row) = c.at;
    let (tc, tco, tr, tro) = anchor_end(cells, &c.sheet, c.at, c.width_mm, c.height_mm);
    let marker = |tag: &str, c: u32, co: i64, r: u32, ro: i64| {
        x(tag)
            .with_child(x("col").with_text(&c.to_string()))
            .with_child(x("colOff").with_text(&co.to_string()))
            .with_child(x("row").with_text(&r.to_string()))
            .with_child(x("rowOff").with_text(&ro.to_string()))
    };
    let anchor = x("twoCellAnchor")
        .with_attr("editAs", "oneCell")
        .with_child(marker("from", col - 1, 0, row - 1, 0))
        .with_child(marker("to", tc, tco, tr, tro))
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

#[cfg(test)]
mod tests {
    use super::*;

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
