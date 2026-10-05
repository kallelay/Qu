//! Cell-level reading and in-place editing of a workbook: open an existing
//! `.xlsx`, change cells, formulas, formats and sheets, save.
//!
//! `read_sheet`/`write_sheet` in `lib.rs` move whole tables and cannot
//! modify a file (calamine reads, rust_xlsxwriter only creates). This is
//! the third leg `docs/design/toolkit-office.md` names -- `umya-spreadsheet`,
//! which reads a workbook into a model and writes it back -- kept behind
//! this module's plain types so no backend type reaches the language.
//!
//! Cells are addressed the way Excel shows them (`"B3"`, `"A1:C20"`),
//! sheets by name. Formulas are stored, not evaluated: Excel and
//! LibreOffice recalculate on open, and a formula cell read back here
//! reports its formula text (and whatever cached value the file had).

use crate::surgical::{self, CfRule, CfStyle, CfValue, ChartReq, CondReq, Pending, Rng, SeriesReq, ValKind, ValidReq};
pub use qu_ooxml::chart::ChartKind;
use umya_spreadsheet as umya;

pub struct Workbook {
    book: umya::Workbook,
    /// The file as opened. Saved unchanged (plus the pending edits) while
    /// no cell-level edit has gone through the model.
    source: Option<Vec<u8>>,
    /// A cell/sheet edit went through `umya`, so saving must re-serialize.
    model_dirty: bool,
    /// Charts, conditional formats, validations: applied to the package at
    /// save time (see `surgical.rs`).
    pending: Vec<Pending>,
}

/// A cell's value as the language sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Empty,
    Num(f64),
    Str(String),
    Bool(bool),
}

/// Formatting applied to every cell of a range.
#[derive(Clone, Debug, Default)]
pub struct CellFormat {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub size: Option<f64>,
    pub font: Option<String>,
    /// `#rrggbb`.
    pub color: Option<String>,
    /// `#rrggbb` fill.
    pub background: Option<String>,
    /// An Excel number format, e.g. `0.00`, `0.00E+00`, `yyyy-mm-dd`.
    pub number_format: Option<String>,
    /// `left`, `center`, `right`.
    pub align: Option<String>,
}

fn err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

/// `"B3"` -> (col 2, row 3), both 1-based. `$` anchors are ignored.
pub fn parse_cell(a1: &str) -> Result<(u32, u32), String> {
    let s = a1.trim().replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit()).ok_or_else(|| format!("`{a1}` is not a cell address like B3"))?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("`{a1}` is not a cell address like B3"));
    }
    let col = letters.to_ascii_uppercase().bytes().fold(0u32, |acc, b| acc * 26 + (b - b'A' + 1) as u32);
    let row: u32 = digits.parse().map_err(|_| format!("`{a1}` is not a cell address like B3"))?;
    if row == 0 || col == 0 || col > 16384 || row > 1_048_576 {
        return Err(format!("`{a1}` is outside the sheet (A1 to XFD1048576)"));
    }
    Ok((col, row))
}

/// `"A1:C20"` (or a single cell) -> (c1, r1, c2, r2), normalised so c1<=c2, r1<=r2.
pub fn parse_range(range: &str) -> Result<(u32, u32, u32, u32), String> {
    let (a, b) = match range.split_once(':') {
        Some((a, b)) => (a, b),
        None => (range, range),
    };
    let (c1, r1) = parse_cell(a)?;
    let (c2, r2) = parse_cell(b)?;
    Ok((c1.min(c2), r1.min(r2), c1.max(c2), r1.max(r2)))
}

pub fn column_letters(mut col: u32) -> String {
    let mut s = Vec::new();
    while col > 0 {
        let r = (col - 1) % 26;
        s.push(b'A' + r as u8);
        col = (col - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

fn hex(c: &str) -> Result<String, String> {
    let h = c.trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(format!("FF{}", h.to_uppercase()))
    } else {
        Err(format!("color \"{c}\" -- use #rrggbb"))
    }
}

impl Workbook {
    pub fn open(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(err(&format!("xlsx.open: `{path}`")))?;
        let book = umya::reader::xlsx::read_reader(std::io::Cursor::new(&bytes), true).map_err(err(&format!("xlsx.open: `{path}`")))?;
        Ok(Workbook { book, source: Some(bytes), model_dirty: false, pending: Vec::new() })
    }

    /// A new workbook with one empty sheet, `Sheet1`.
    pub fn new() -> Self {
        Workbook { book: umya::new_file(), source: None, model_dirty: true, pending: Vec::new() }
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let bytes = self.to_bytes().map_err(|e| format!("xlsx.save: `{path}`: {e}"))?;
        std::fs::write(path, bytes).map_err(err(&format!("xlsx.save: `{path}`")))
    }

    /// The workbook as `.xlsx` bytes, for conversion without a file.
    ///
    /// Cell edits go through the `umya` model, which rebuilds the package;
    /// with none, the package is the file as opened. Charts, conditional
    /// formats and validations are then added to it part by part.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let base = match (&self.source, self.model_dirty) {
            (Some(b), false) => b.clone(),
            _ => {
                let mut out = std::io::Cursor::new(Vec::new());
                umya::writer::xlsx::write_writer(&self.book, &mut out).map_err(err("xlsx"))?;
                out.into_inner()
            }
        };
        if self.pending.is_empty() {
            return Ok(base);
        }
        let mut pkg = qu_ooxml::Package::from_bytes(&base)?;
        for p in &self.pending {
            surgical::apply(&mut pkg, p, self)?;
        }
        pkg.to_bytes()
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.book.sheet_collection().iter().map(|s| s.name().to_string()).collect()
    }

    fn sheet(&self, name: &str) -> Result<&umya::Worksheet, String> {
        self.book.sheet_by_name(name).map_err(|_| format!("no sheet named `{name}` -- the workbook has: {}", self.sheet_names().join(", ")))
    }

    fn sheet_mut(&mut self, name: &str) -> Result<&mut umya::Worksheet, String> {
        let names = self.sheet_names();
        self.model_dirty = true;
        self.book.sheet_by_name_mut(name).map_err(|_| format!("no sheet named `{name}` -- the workbook has: {}", names.join(", ")))
    }

    pub fn get(&self, sheet: &str, cell: &str) -> Result<CellValue, String> {
        let (c, r) = parse_cell(cell)?;
        Ok(cell_value(self.sheet(sheet)?.cell((c, r))))
    }

    /// The formula in a cell, without the leading `=`, if it has one.
    pub fn formula(&self, sheet: &str, cell: &str) -> Result<Option<String>, String> {
        let (c, r) = parse_cell(cell)?;
        Ok(self.sheet(sheet)?.cell((c, r)).map(|x| x.formula().to_string()).filter(|f| !f.is_empty()))
    }

    pub fn set(&mut self, sheet: &str, cell: &str, v: &CellValue) -> Result<(), String> {
        let (c, r) = parse_cell(cell)?;
        let cell = self.sheet_mut(sheet)?.cell_mut((c, r));
        match v {
            CellValue::Empty => {
                cell.set_value_string("");
            }
            CellValue::Num(x) => {
                if !x.is_finite() {
                    return Err(format!("{x} cannot be stored in a spreadsheet cell -- Excel has no NaN or infinity"));
                }
                cell.set_value_number(*x);
            }
            CellValue::Str(s) => {
                cell.set_value_string(s.as_str());
            }
            CellValue::Bool(b) => {
                cell.set_value_bool(*b);
            }
        }
        Ok(())
    }

    /// Store a formula (a leading `=` is optional).
    pub fn set_formula(&mut self, sheet: &str, cell: &str, formula: &str) -> Result<(), String> {
        let (c, r) = parse_cell(cell)?;
        let f = formula.trim().trim_start_matches('=');
        if f.is_empty() {
            return Err("set_formula: the formula is empty".into());
        }
        self.sheet_mut(sheet)?.cell_mut((c, r)).set_formula(f);
        Ok(())
    }

    /// Store `formula` in every cell of `range`, shifting its relative
    /// references from the range's first cell -- Excel's fill-down.
    pub fn fill_formula(&mut self, sheet: &str, range: &str, formula: &str) -> Result<usize, String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let f = formula.trim().trim_start_matches('=').to_string();
        let mut n = 0;
        for r in r1..=r2 {
            for c in c1..=c2 {
                let shifted = shift_refs(&f, c as i64 - c1 as i64, r as i64 - r1 as i64);
                self.sheet_mut(sheet)?.cell_mut((c, r)).set_formula(shifted);
                n += 1;
            }
        }
        Ok(n)
    }

    /// Values of a rectangular range, row by row.
    pub fn range(&self, sheet: &str, range: &str) -> Result<Vec<Vec<CellValue>>, String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let ws = self.sheet(sheet)?;
        Ok((r1..=r2).map(|r| (c1..=c2).map(|c| cell_value(ws.cell((c, r)))).collect()).collect())
    }

    /// Write `rows` with its top-left corner at `anchor`.
    pub fn set_range(&mut self, sheet: &str, anchor: &str, rows: &[Vec<CellValue>]) -> Result<(), String> {
        let (c0, r0) = parse_cell(anchor)?;
        for (dr, row) in rows.iter().enumerate() {
            for (dc, v) in row.iter().enumerate() {
                let a1 = format!("{}{}", column_letters(c0 + dc as u32), r0 + dr as u32);
                self.set(sheet, &a1, v)?;
            }
        }
        Ok(())
    }

    /// The smallest range covering every non-empty cell, e.g. `A1:D20`
    /// (`None` for an empty sheet).
    pub fn used_range(&self, sheet: &str) -> Result<Option<String>, String> {
        let ws = self.sheet(sheet)?;
        let (mut c2, mut r2) = (0, 0);
        let (mut c1, mut r1) = (u32::MAX, u32::MAX);
        for cell in ws.cells() {
            if cell.value().is_empty() && cell.formula().is_empty() {
                continue;
            }
            let co = cell.coordinate();
            let (c, r) = (co.col_num(), co.row_num());
            c1 = c1.min(c);
            r1 = r1.min(r);
            c2 = c2.max(c);
            r2 = r2.max(r);
        }
        Ok((c2 > 0).then(|| format!("{}{}:{}{}", column_letters(c1), r1, column_letters(c2), r2)))
    }

    pub fn add_sheet(&mut self, name: &str) -> Result<(), String> {
        if self.sheet_names().iter().any(|n| n == name) {
            return Err(format!("the workbook already has a sheet named `{name}`"));
        }
        self.model_dirty = true;
        self.book.new_sheet(name).map(|_| ()).map_err(err("add_sheet"))
    }

    pub fn rename_sheet(&mut self, old: &str, new: &str) -> Result<(), String> {
        let i = self.sheet_names().iter().position(|n| n == old).ok_or_else(|| format!("no sheet named `{old}`"))?;
        self.model_dirty = true;
        self.book.set_sheet_name(i, new).map_err(err("rename_sheet"))?;
        for p in &mut self.pending {
            p.rename_sheet(old, new);
        }
        Ok(())
    }

    pub fn delete_sheet(&mut self, name: &str) -> Result<(), String> {
        if self.sheet_names().len() == 1 {
            return Err("cannot delete the only sheet -- a workbook needs at least one".into());
        }
        self.sheet(name)?;
        self.model_dirty = true;
        self.pending.retain(|p| p.sheet() != name);
        self.book.remove_sheet_by_name(name).map_err(err("delete_sheet"))
    }

    pub fn insert_rows(&mut self, sheet: &str, at: u32, n: u32) -> Result<(), String> {
        self.sheet_mut(sheet)?.insert_new_row(at, n);
        Ok(())
    }

    pub fn delete_rows(&mut self, sheet: &str, at: u32, n: u32) -> Result<(), String> {
        self.sheet_mut(sheet)?.remove_row(at, n);
        Ok(())
    }

    pub fn insert_columns(&mut self, sheet: &str, col: &str, n: u32) -> Result<(), String> {
        parse_cell(&format!("{col}1"))?;
        self.sheet_mut(sheet)?.insert_new_column(&col.to_ascii_uppercase(), n);
        Ok(())
    }

    pub fn delete_columns(&mut self, sheet: &str, col: &str, n: u32) -> Result<(), String> {
        parse_cell(&format!("{col}1"))?;
        self.sheet_mut(sheet)?.remove_column(&col.to_ascii_uppercase(), n);
        Ok(())
    }

    /// Column width in Excel's character units.
    pub fn column_width(&mut self, sheet: &str, col: &str, width: f64) -> Result<(), String> {
        parse_cell(&format!("{col}1"))?;
        if !(0.0..=255.0).contains(&width) {
            return Err(format!("column width {width} -- Excel allows 0 to 255"));
        }
        self.sheet_mut(sheet)?.column_dimension_mut(&col.to_ascii_uppercase()).set_width(width);
        Ok(())
    }

    /// Row height in points.
    pub fn row_height(&mut self, sheet: &str, row: u32, height: f64) -> Result<(), String> {
        if !(0.0..=409.0).contains(&height) {
            return Err(format!("row height {height} pt -- Excel allows 0 to 409"));
        }
        let r = self.sheet_mut(sheet)?.row_dimension_mut(row);
        r.set_height(height);
        r.set_custom_height(true);
        Ok(())
    }

    pub fn format(&mut self, sheet: &str, range: &str, f: &CellFormat) -> Result<usize, String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let color = f.color.as_deref().map(hex).transpose()?;
        let bg = f.background.as_deref().map(hex).transpose()?;
        let align = match f.align.as_deref() {
            None => None,
            Some("left") => Some(umya::HorizontalAlignmentValues::Left),
            Some("center") | Some("centre") => Some(umya::HorizontalAlignmentValues::Center),
            Some("right") => Some(umya::HorizontalAlignmentValues::Right),
            Some(other) => return Err(format!("align=\"{other}\" -- use left, center or right")),
        };
        let ws = self.sheet_mut(sheet)?;
        let mut n = 0;
        for r in r1..=r2 {
            for c in c1..=c2 {
                let st = ws.style_mut((c, r));
                {
                    let font = st.font_mut();
                    if let Some(b) = f.bold {
                        font.set_bold(b);
                    }
                    if let Some(i) = f.italic {
                        font.set_italic(i);
                    }
                    if let Some(s) = f.size {
                        font.set_size(s);
                    }
                    if let Some(name) = &f.font {
                        font.set_name(name.as_str());
                    }
                    if let Some(c) = &color {
                        let mut col = umya::Color::default();
                        col.set_argb_str(c);
                        font.set_color(col);
                    }
                }
                if let Some(b) = &bg {
                    st.set_background_color(b);
                }
                if let Some(nf) = &f.number_format {
                    st.number_format_mut().set_format_code(nf.as_str());
                }
                if let Some(a) = &align {
                    st.alignment_mut().set_horizontal(a.clone());
                }
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn merge(&mut self, sheet: &str, range: &str) -> Result<(), String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let norm = format!("{}{}:{}{}", column_letters(c1), r1, column_letters(c2), r2);
        self.sheet_mut(sheet)?.add_merge_cells(norm);
        Ok(())
    }

    /// Freeze the rows above and the columns left of `cell` (`"A2"`
    /// freezes the header row, `"B2"` the header row and first column).
    pub fn freeze_panes(&mut self, sheet: &str, cell: &str) -> Result<(), String> {
        let (c, r) = parse_cell(cell)?;
        let ws = self.sheet_mut(sheet)?;
        let views = ws.sheet_views_mut().sheet_view_list_mut();
        if views.is_empty() {
            views.push(umya::SheetView::default());
        }
        let mut pane = umya::Pane::default();
        if c > 1 {
            pane.set_horizontal_split((c - 1) as f64);
        }
        if r > 1 {
            pane.set_vertical_split((r - 1) as f64);
        }
        let mut tl = umya::Coordinate::default();
        tl.set_coordinate(format!("{}{}", column_letters(c), r));
        pane.set_top_left_cell(tl);
        pane.set_active_pane(match (c > 1, r > 1) {
            (true, true) => umya::PaneValues::BottomRight,
            (true, false) => umya::PaneValues::TopRight,
            _ => umya::PaneValues::BottomLeft,
        });
        pane.set_state(umya::PaneStateValues::Frozen);
        views[0].set_pane(pane);
        Ok(())
    }

    /// A workbook-level defined name, e.g. ("Frequency", "Data!$A$2:$A$100").
    pub fn define_name(&mut self, name: &str, address: &str) -> Result<(), String> {
        if name.is_empty() || name.contains(' ') || name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(format!("`{name}` is not a valid Excel name (no spaces, must not start with a digit)"));
        }
        let mut d = umya::DefinedName::default();
        d.set_name(name);
        d.set_address(address);
        self.model_dirty = true;
        self.book.add_defined_names(d);
        Ok(())
    }
}

/// What `clear` removes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClearWhat {
    /// Values and formulas; the cell keeps its formatting.
    Contents,
    /// Formatting; the cell keeps its value.
    Formats,
    All,
}

impl ClearWhat {
    pub fn parse(s: &str) -> Result<ClearWhat, String> {
        match s {
            "all" => Ok(ClearWhat::All),
            "contents" | "values" => Ok(ClearWhat::Contents),
            "formats" | "formatting" => Ok(ClearWhat::Formats),
            other => Err(format!("what=\"{other}\" -- use all, contents or formats")),
        }
    }
}

/// A sheet's visibility.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SheetVisibility {
    Visible,
    Hidden,
    /// Hidden, and absent from Excel's own Unhide list (only reachable from
    /// the VBA editor, or by `xlsx.hide_sheet(..., state="visible")`).
    VeryHidden,
}

impl SheetVisibility {
    pub fn parse(s: &str) -> Result<SheetVisibility, String> {
        match s {
            "visible" => Ok(SheetVisibility::Visible),
            "hidden" => Ok(SheetVisibility::Hidden),
            "very_hidden" | "veryhidden" | "very hidden" => Ok(SheetVisibility::VeryHidden),
            other => Err(format!("state=\"{other}\" -- use hidden, very_hidden or visible")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SheetVisibility::Visible => "visible",
            SheetVisibility::Hidden => "hidden",
            SheetVisibility::VeryHidden => "very_hidden",
        }
    }
}

/// A cell lifted off the sheet: its value or formula and its formatting.
struct Held {
    value: umya::CellValue,
    formula: Option<String>,
    style: umya::Style,
}

impl Held {
    fn of(cell: &umya::Cell) -> Held {
        Held { value: cell.cell_value().clone(), formula: Some(cell.formula().to_string()).filter(|f| !f.is_empty()), style: cell.style().clone() }
    }

    /// Put it at (col, row); relative references in a formula move by
    /// (`dc`, `dr`) -- zero for a move, the distance for a copy or a sort.
    fn place(&self, ws: &mut umya::Worksheet, (col, row): (u32, u32), dc: i64, dr: i64) {
        let cell = ws.cell_mut((col, row));
        match &self.formula {
            Some(f) => {
                cell.set_formula(if dc == 0 && dr == 0 { f.clone() } else { shift_refs(f, dc, dr) });
            }
            None => {
                cell.set_cell_value(self.value.clone());
            }
        }
        cell.set_style(self.style.clone());
    }
}

/// How two sort keys order. Excel ascending: numbers, then text (case
/// blind), then TRUE/FALSE; empty cells last in either direction.
fn key_cmp(a: &CellValue, b: &CellValue, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let rank = |v: &CellValue| match v {
        CellValue::Num(_) => 0,
        CellValue::Str(_) => 1,
        CellValue::Bool(_) => 2,
        CellValue::Empty => 3,
    };
    match (a, b) {
        (CellValue::Empty, CellValue::Empty) => Equal,
        (CellValue::Empty, _) => Greater,
        (_, CellValue::Empty) => Less,
        _ => {
            let ord = rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
                (CellValue::Num(x), CellValue::Num(y)) => x.partial_cmp(y).unwrap_or(Equal),
                (CellValue::Str(x), CellValue::Str(y)) => x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| x.cmp(y)),
                (CellValue::Bool(x), CellValue::Bool(y)) => x.cmp(y),
                _ => Equal,
            });
            if desc {
                ord.reverse()
            } else {
                ord
            }
        }
    }
}

impl Workbook {
    fn check_sheet_title(name: &str) -> Result<(), String> {
        if name.is_empty() || name.chars().count() > 31 {
            return Err(format!("sheet name \"{name}\" -- 1 to 31 characters"));
        }
        if let Some(c) = name.chars().find(|c| "[]:*?/\\".contains(*c)) {
            return Err(format!("sheet name \"{name}\" contains `{c}`, which Excel does not allow"));
        }
        if name.starts_with('\'') || name.ends_with('\'') {
            return Err(format!("sheet name \"{name}\" may not start or end with an apostrophe"));
        }
        Ok(())
    }

    /// Every table name in the workbook (saved or pending), lower-cased.
    fn table_names(&self) -> Vec<String> {
        let mut out: Vec<String> = self.book.sheet_collection().iter().flat_map(|ws| ws.tables().iter().flat_map(|t| [t.name().to_lowercase(), t.display_name().to_lowercase()])).collect();
        out.extend(self.pending.iter().filter_map(|p| if let Pending::Table(t) = p { Some(t.name.to_lowercase()) } else { None }));
        out
    }

    /// A copy of `source` as a new sheet named `new_name`, appended after
    /// the last sheet: cells, formulas, formatting, merges, column widths,
    /// freeze panes, conditional formats, validations, and whatever
    /// charts, comments, images and tables the sheet carries (a copied
    /// table is renamed `Name_2`, as table names are unique).
    pub fn copy_sheet(&mut self, source: &str, new_name: &str) -> Result<(), String> {
        Self::check_sheet_title(new_name)?;
        if self.sheet_names().iter().any(|n| n.eq_ignore_ascii_case(new_name)) {
            return Err(format!("the workbook already has a sheet named `{new_name}`"));
        }
        let mut ws = self.sheet(source)?.clone();
        ws.set_name(new_name);
        ws.set_state(umya::SheetStateValues::Visible);
        // Two selected tabs would put Excel in group-edit mode.
        for v in ws.sheet_views_mut().sheet_view_list_mut() {
            v.set_tab_selected(false);
        }
        let mut taken = self.table_names();
        for t in ws.tables_mut() {
            let stem = t.name().to_string();
            let fresh = (2..).map(|n| format!("{stem}_{n}")).find(|c| !taken.contains(&c.to_lowercase())).unwrap();
            taken.push(fresh.to_lowercase());
            t.set_name(&fresh);
            t.set_display_name(&fresh);
        }
        self.model_dirty = true;
        self.book.add_sheet(ws).map_err(err("copy_sheet"))?;
        let copies: Vec<Pending> = {
            let mut pool = taken.clone();
            let mut rename = |old: &str| -> String {
                let stem = old.to_string();
                let fresh = (2..).map(|n| format!("{stem}_{n}")).find(|c| !pool.contains(&c.to_lowercase())).unwrap();
                pool.push(fresh.to_lowercase());
                fresh
            };
            self.pending.iter().filter(|p| p.sheet() == source).map(|p| p.copy_to_sheet(source, new_name, &mut rename)).collect()
        };
        self.pending.extend(copies);
        Ok(())
    }

    pub fn sheet_state(&self, name: &str) -> Result<SheetVisibility, String> {
        Ok(match self.sheet(name)?.state() {
            umya::SheetStateValues::Visible => SheetVisibility::Visible,
            umya::SheetStateValues::Hidden => SheetVisibility::Hidden,
            umya::SheetStateValues::VeryHidden => SheetVisibility::VeryHidden,
        })
    }

    /// Hide a sheet (Excel's Hide, or the VBA-only "very hidden"), or show
    /// it again. A workbook needs one visible sheet.
    pub fn set_sheet_visibility(&mut self, name: &str, state: SheetVisibility) -> Result<(), String> {
        let names = self.sheet_names();
        let idx = names.iter().position(|n| n == name).ok_or_else(|| format!("no sheet named `{name}` -- the workbook has: {}", names.join(", ")))?;
        if state != SheetVisibility::Visible {
            let other_visible = names.iter().enumerate().any(|(i, n)| i != idx && self.sheet_state(n).map(|s| s == SheetVisibility::Visible).unwrap_or(false));
            if !other_visible {
                return Err(format!("cannot hide `{name}` -- a workbook needs at least one visible sheet"));
            }
        }
        self.model_dirty = true;
        self.book.sheet_mut(idx).map_err(err("hide_sheet"))?.set_state(match state {
            SheetVisibility::Visible => umya::SheetStateValues::Visible,
            SheetVisibility::Hidden => umya::SheetStateValues::Hidden,
            SheetVisibility::VeryHidden => umya::SheetStateValues::VeryHidden,
        });
        if state != SheetVisibility::Visible {
            // The tab the workbook opens on must not be a hidden one.
            let active = self.book.workbook_view().active_tab() as usize;
            if active == idx {
                let first = (0..names.len()).find(|i| *i != idx && self.sheet_state(&names[*i]).map(|s| s == SheetVisibility::Visible).unwrap_or(false)).unwrap_or(0);
                self.book.set_active_sheet(first as u32);
                for (i, n) in names.iter().enumerate() {
                    for v in self.sheet_mut(n)?.sheet_views_mut().sheet_view_list_mut() {
                        v.set_tab_selected(i == first);
                    }
                }
            }
        }
        Ok(())
    }

    /// (col, row) of every stored cell inside the rectangle.
    fn stored_in(ws: &umya::Worksheet, (c1, r1, c2, r2): (u32, u32, u32, u32)) -> Vec<(u32, u32)> {
        ws.cells()
            .into_iter()
            .map(|c| (c.coordinate().col_num(), c.coordinate().row_num()))
            .filter(|(c, r)| (c1..=c2).contains(c) && (r1..=r2).contains(r))
            .collect()
    }

    /// Refuse a rectangle that cuts through a merged range.
    fn check_merges(ws: &umya::Worksheet, (c1, r1, c2, r2): (u32, u32, u32, u32), what: &str) -> Result<(), String> {
        for m in ws.merge_cells() {
            let (mc1, mr1, mc2, mr2) = parse_range(&m.range())?;
            let overlap = mc1 <= c2 && mc2 >= c1 && mr1 <= r2 && mr2 >= r1;
            let inside = mc1 >= c1 && mc2 <= c2 && mr1 >= r1 && mr2 <= r2;
            if overlap && !inside {
                return Err(format!("{what}: the range cuts through the merged cells {} -- unmerge them or include them whole", m.range()));
            }
            if overlap && what.starts_with("sort") {
                return Err(format!("{what}: the range holds merged cells ({}) -- Excel cannot sort those either", m.range()));
            }
        }
        Ok(())
    }

    /// Empty a range: its values and formulas, its formatting, or both.
    /// Returns how many stored cells it touched.
    pub fn clear(&mut self, sheet: &str, range: &str, what: ClearWhat) -> Result<usize, String> {
        let rect = parse_range(range)?;
        let ws = self.sheet_mut(sheet)?;
        let hit = Self::stored_in(ws, rect);
        for (c, r) in &hit {
            match what {
                ClearWhat::All => {
                    ws.remove_cell((*c, *r));
                }
                ClearWhat::Contents => {
                    ws.cell_mut((*c, *r)).set_blank();
                }
                ClearWhat::Formats => {
                    ws.cell_mut((*c, *r)).set_style(umya::Style::default());
                }
            }
        }
        Ok(hit.len())
    }

    /// Move a range so that its top-left corner lands on `to`, on
    /// `to_sheet` (default: the same sheet), overwriting what is there.
    /// With `copy`, the source stays and relative references in copied
    /// formulas shift, as in a paste; a move keeps formulas as they are.
    /// Formulas elsewhere that point into the moved cells are not rewritten.
    pub fn move_range(&mut self, sheet: &str, range: &str, to: &str, to_sheet: Option<&str>, copy: bool) -> Result<usize, String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let (tc, tr) = parse_cell(to)?;
        let (w, h) = (c2 - c1, r2 - r1);
        let dest_sheet = to_sheet.unwrap_or(sheet).to_string();
        if tc + w > 16384 || tr + h > 1_048_576 {
            return Err(format!("moving {range} to {to} runs off the sheet (XFD1048576)"));
        }
        if dest_sheet == sheet && (tc, tr) == (c1, r1) {
            return Err("the range is already there".into());
        }
        let dest = (tc, tr, tc + w, tr + h);
        Self::check_merges(self.sheet(sheet)?, (c1, r1, c2, r2), "move_range")?;
        Self::check_merges(self.sheet(&dest_sheet)?, dest, "move_range")?;
        let held: Vec<((u32, u32), Held)> = {
            let ws = self.sheet(sheet)?;
            Self::stored_in(ws, (c1, r1, c2, r2)).into_iter().filter_map(|(c, r)| ws.cell((c, r)).map(|cell| ((c, r), Held::of(cell)))).collect()
        };
        if !copy {
            let ws = self.sheet_mut(sheet)?;
            for ((c, r), _) in &held {
                ws.remove_cell((*c, *r));
            }
        }
        let ws = self.sheet_mut(&dest_sheet)?;
        for (c, r) in Self::stored_in(ws, dest) {
            ws.remove_cell((c, r));
        }
        let (dc, dr) = (tc as i64 - c1 as i64, tr as i64 - r1 as i64);
        for ((c, r), cell) in &held {
            cell.place(ws, ((*c as i64 + dc) as u32, (*r as i64 + dr) as u32), if copy { dc } else { 0 }, if copy { dr } else { 0 });
        }
        Ok(held.len())
    }

    /// Sort the rows of `range` by one or more key columns. A key is a
    /// column letter on the sheet (`"C"`) or a 1-based position inside the
    /// range, with its own direction. Whole rows move -- values, formulas
    /// (relative references follow the row, as in Excel), formatting. With
    /// `header` the first row stays on top. The sort is stable.
    pub fn sort_range(&mut self, sheet: &str, range: &str, keys: &[(SortKey, bool)], header: bool) -> Result<(), String> {
        let (c1, r1, c2, r2) = parse_range(range)?;
        let first = if header { r1 + 1 } else { r1 };
        if first > r2 {
            return Err(format!("`{range}` has no rows to sort{}", if header { " below the header" } else { "" }));
        }
        if keys.is_empty() {
            return Err("sort_range needs a key column (by=)".into());
        }
        let mut cols = Vec::new();
        for (k, desc) in keys {
            let col = match k {
                SortKey::Letter(l) => parse_cell(&format!("{l}1"))?.0,
                SortKey::Position(n) => {
                    if *n == 0 || c1 + n - 1 > c2 {
                        return Err(format!("key {n} is outside `{range}`, which has {} columns", c2 - c1 + 1));
                    }
                    c1 + n - 1
                }
            };
            if !(c1..=c2).contains(&col) {
                return Err(format!("key column {} is outside `{range}`", column_letters(col)));
            }
            cols.push((col, *desc));
        }
        Self::check_merges(self.sheet(sheet)?, (c1, r1, c2, r2), "sort_range")?;
        let ws = self.sheet(sheet)?;
        let mut order: Vec<(u32, Vec<CellValue>)> = Vec::new();
        for r in first..=r2 {
            let mut vals = Vec::new();
            for (col, _) in &cols {
                let cell = ws.cell((*col, r));
                if let Some(cell) = cell {
                    if !cell.formula().is_empty() && cell.value().is_empty() {
                        return Err(format!("{}{r} is a formula with no stored value -- nothing to sort by until Excel has calculated it", column_letters(*col)));
                    }
                }
                vals.push(cell_value(cell));
            }
            order.push((r, vals));
        }
        order.sort_by(|a, b| {
            for (i, (_, desc)) in cols.iter().enumerate() {
                let o = key_cmp(&a.1[i], &b.1[i], *desc);
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            std::cmp::Ordering::Equal
        });
        if order.iter().enumerate().all(|(i, (r, _))| *r == first + i as u32) {
            return Ok(());
        }
        let mut rows: Vec<(u32, Vec<Option<Held>>)> = Vec::new();
        for (old, _) in &order {
            rows.push((*old, (c1..=c2).map(|c| ws.cell((c, *old)).map(Held::of)).collect()));
        }
        let ws = self.sheet_mut(sheet)?;
        for (i, (old, cells)) in rows.iter().enumerate() {
            let new = first + i as u32;
            for (j, held) in cells.iter().enumerate() {
                let col = c1 + j as u32;
                ws.remove_cell((col, new));
                if let Some(h) = held {
                    h.place(ws, (col, new), 0, new as i64 - *old as i64);
                }
            }
        }
        Ok(())
    }

    /// Put filter buttons on the header row of `range` (`None` removes
    /// them). The filter is switched on, not applied: no row is hidden.
    pub fn autofilter(&mut self, sheet: &str, range: Option<&str>) -> Result<(), String> {
        let Some(range) = range else {
            self.sheet_mut(sheet)?.remove_auto_filter();
            return Ok(());
        };
        let (c1, r1, c2, r2) = parse_range(range)?;
        let a1 = format!("{}{}:{}{}", column_letters(c1), r1, column_letters(c2), r2);
        if (c1, r1) == (c2, r2) {
            return Err(format!("`{range}` is a single cell -- give the header row and the data below it, e.g. A1:D20"));
        }
        self.sheet_mut(sheet)?.set_auto_filter(a1);
        Ok(())
    }

    /// The range of the sheet's autofilter, if it has one.
    pub fn autofilter_range(&self, sheet: &str) -> Result<Option<String>, String> {
        Ok(self.sheet(sheet)?.auto_filter().map(|f| f.range().range()))
    }

    /// Turn `range` into an Excel Table named `name`: banded rows, filter
    /// buttons on the header row, structured references. `range` includes
    /// the header row (unless `header` is false). With `total` a totals row
    /// is written just below `range` (it must be empty): the label `Total`
    /// in the first column and a `SUBTOTAL` of that kind under every
    /// column that holds only numbers.
    pub fn create_table(&mut self, sheet: &str, range: &str, name: &str, header: bool, style: Option<&str>, stripes: bool, total: Option<surgical::TotalFn>) -> Result<(), String> {
        let rng = self.sheet_range(range, sheet)?;
        if rng.sheet != sheet {
            return Err("the range must be on the sheet the table goes on (give it without a sheet name)".into());
        }
        surgical::check_table_name(name)?;
        if self.table_names().contains(&name.to_lowercase()) {
            return Err(format!("the workbook already has a table named `{name}`"));
        }
        let style = match style {
            Some(s) => surgical::table_style(s)?,
            None => Some("TableStyleMedium2".to_string()),
        };
        let min_rows = if header { 2 } else { 1 };
        if rng.r2 - rng.r1 + 1 < min_rows {
            return Err(format!("`{range}` is too short for a table -- it needs a header row and at least one data row (header=false for data only)"));
        }
        let ws = self.sheet(sheet)?;
        let rect = (rng.c1, rng.r1, rng.c2, rng.r2 + u32::from(total.is_some()));
        Self::check_merges(ws, rect, "create_table")?;
        // Overlap with another table.
        let mut others: Vec<(u32, u32, u32, u32)> = ws.tables().iter().map(|t| (t.area().0.col_num(), t.area().0.row_num(), t.area().1.col_num(), t.area().1.row_num())).collect();
        for p in &self.pending {
            if let Pending::Table(t) = p {
                if t.range.sheet == sheet {
                    others.push((t.range.c1, t.range.r1, t.range.c2, t.range.r2 + u32::from(t.total.is_some())));
                }
            }
        }
        if others.iter().any(|(a, b, c, d)| *a <= rect.2 && *c >= rect.0 && *b <= rect.3 && *d >= rect.1) {
            return Err(format!("`{range}` overlaps a table that is already there"));
        }
        if let Some(a) = ws.auto_filter() {
            let (a1, b1, a2, b2) = parse_range(&a.range().range())?;
            if a1 <= rect.2 && a2 >= rect.0 && b1 <= rect.3 && b2 >= rect.1 {
                return Err(format!("`{range}` overlaps the sheet's autofilter -- a table has its own filter buttons; remove it with xlsx.autofilter(.., remove=true)"));
            }
        }
        // Which columns are all numbers (they get a total), and is the
        // totals row free?
        let first_data = if header { rng.r1 + 1 } else { rng.r1 };
        let mut total_cols = Vec::new();
        if total.is_some() {
            for c in rng.c1..=rng.c2 {
                let below = ws.cell((c, rng.r2 + 1));
                if below.is_some_and(|x| !x.value().is_empty() || !x.formula().is_empty()) {
                    return Err(format!("{}{} is not empty -- the totals row goes right below the range", column_letters(c), rng.r2 + 1));
                }
                let vals: Vec<CellValue> = (first_data..=rng.r2).map(|r| cell_value(ws.cell((c, r)))).collect();
                if vals.iter().any(|v| matches!(v, CellValue::Num(_))) && vals.iter().all(|v| matches!(v, CellValue::Num(_) | CellValue::Empty)) {
                    total_cols.push((c - rng.c1) as usize);
                }
            }
        }
        // Header cells must be unique text: write what the table will call
        // its columns, so the cells and the table agree.
        let mut names: Vec<String> = Vec::new();
        if header {
            for c in rng.c1..=rng.c2 {
                names.push(match cell_value(ws.cell((c, rng.r1))) {
                    CellValue::Empty => String::new(),
                    CellValue::Str(s) => s,
                    CellValue::Num(x) => format!("{x}"),
                    CellValue::Bool(b) => (if b { "TRUE" } else { "FALSE" }).to_string(),
                });
            }
            surgical::normalize_headers(&mut names);
            for (i, n) in names.iter().enumerate() {
                let a1 = format!("{}{}", column_letters(rng.c1 + i as u32), rng.r1);
                if self.get(sheet, &a1)? != CellValue::Str(n.clone()) {
                    self.set(sheet, &a1, &CellValue::Str(n.clone()))?;
                }
            }
        }
        if let Some(f) = total {
            let r = rng.r2 + 1;
            for c in rng.c1..=rng.c2 {
                let col = column_letters(c);
                if total_cols.contains(&((c - rng.c1) as usize)) {
                    self.set_formula(sheet, &format!("{col}{r}"), &format!("SUBTOTAL({},{col}{first_data}:{col}{})", f.subtotal(), rng.r2))?;
                } else if c == rng.c1 {
                    self.set(sheet, &format!("{col}{r}"), &CellValue::Str("Total".into()))?;
                }
            }
        }
        self.pending.push(Pending::Table(surgical::TableReq { range: rng, name: name.to_string(), header, style, stripes, total, total_cols }));
        Ok(())
    }

    /// Attach a note to a cell (it shows when the cell is hovered). A
    /// second note on the same cell replaces the first.
    pub fn add_comment(&mut self, sheet: &str, cell: &str, text: &str, author: Option<&str>) -> Result<(), String> {
        self.sheet(sheet)?;
        let cell = parse_cell(cell)?;
        if text.is_empty() {
            return Err("the note is empty".into());
        }
        if text.chars().count() > 32767 {
            return Err("a note holds at most 32767 characters".into());
        }
        if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r') {
            return Err("the note contains control characters".into());
        }
        let author = author.unwrap_or("Qu").to_string();
        if author.is_empty() || author.chars().count() > 52 {
            return Err("author= takes 1 to 52 characters".into());
        }
        self.pending.push(Pending::Comment(surgical::CommentReq { sheet: sheet.to_string(), cell, text: text.replace("\r\n", "\n").replace('\r', "\n"), author }));
        Ok(())
    }

    /// Place a PNG, JPEG or GIF with its top-left corner at `at`
    /// (default: the free spot `add_chart` uses). With neither size given it
    /// is shown at its pixel size at 96 dpi, scaled down to at most 160 mm
    /// wide; with one, the other follows the aspect ratio.
    pub fn add_image(&mut self, sheet: &str, bytes: Vec<u8>, at: Option<&str>, width_mm: Option<f64>, height_mm: Option<f64>, alt: Option<&str>) -> Result<(), String> {
        self.sheet(sheet)?;
        let (pw, ph, _, _) = qu_ooxml::image_info(&bytes)?;
        if pw == 0 || ph == 0 {
            return Err("the image has no pixels".into());
        }
        let native = (pw as f64 * 25.4 / 96.0, ph as f64 * 25.4 / 96.0);
        let (w, h) = match (width_mm, height_mm) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * native.1 / native.0),
            (None, Some(h)) => (h * native.0 / native.1, h),
            (None, None) => {
                let k = (160.0 / native.0).min(1.0);
                (native.0 * k, native.1 * k)
            }
        };
        if !(1.0..=2000.0).contains(&w) || !(1.0..=2000.0).contains(&h) {
            return Err(format!("image size {w:.1} x {h:.1} mm -- each side must be 1 to 2000 mm"));
        }
        let at = match at {
            Some(a) => parse_cell(a)?,
            None => self.free_anchor(sheet)?,
        };
        self.pending.push(Pending::Image(surgical::ImageReq { sheet: sheet.to_string(), bytes, at, width_mm: w, height_mm: h, alt: alt.map(String::from) }));
        Ok(())
    }
}

/// A sort key: a sheet column letter, or a 1-based column inside the range.
#[derive(Clone, Debug, PartialEq)]
pub enum SortKey {
    Letter(String),
    Position(u32),
}

/// What `add_chart` is asked for, before the ranges are checked.
#[derive(Clone, Debug)]
pub struct ChartOptions {
    pub kind: ChartKind,
    /// One range per series (`"B2:B40"`, `"Data!B2:B40"`).
    pub y: Vec<String>,
    /// None, one range shared by every series, or one per series.
    pub x: Vec<String>,
    pub names: Vec<String>,
    pub colors: Vec<String>,
    pub title: Option<String>,
    pub x_title: Option<String>,
    pub y_title: Option<String>,
    /// Top-left cell; default: two columns right of the used range, row 2.
    pub at: Option<String>,
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
    pub lines: Option<bool>,
    pub markers: Option<bool>,
    pub x_min: Option<f64>,
    pub x_max: Option<f64>,
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub x_log: bool,
    pub y_log: bool,
    pub legend: Option<bool>,
    pub equal_axes: bool,
}

impl ChartOptions {
    pub fn new(kind: ChartKind) -> Self {
        ChartOptions {
            kind,
            y: Vec::new(),
            x: Vec::new(),
            names: Vec::new(),
            colors: Vec::new(),
            title: None,
            x_title: None,
            y_title: None,
            at: None,
            width_mm: None,
            height_mm: None,
            lines: None,
            markers: None,
            x_min: None,
            x_max: None,
            y_min: None,
            y_max: None,
            x_log: false,
            y_log: false,
            legend: None,
            equal_axes: false,
        }
    }
}

/// `"scatter"`/`"xy"`, `"line"`, `"bar"`/`"column"` (vertical), `"barh"` (horizontal).
pub fn chart_kind(name: &str) -> Result<ChartKind, String> {
    match name.to_ascii_lowercase().as_str() {
        "scatter" | "xy" => Ok(ChartKind::Scatter),
        "line" => Ok(ChartKind::Line),
        "bar" | "column" => Ok(ChartKind::Column),
        "barh" => Ok(ChartKind::Bar),
        other => Err(format!("chart kind \"{other}\" -- use scatter, line, bar (vertical) or barh (horizontal)")),
    }
}

/// A comparison rule by its Qu name.
pub fn cf_operator(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "greater_than" => "greaterThan",
        "less_than" => "lessThan",
        "greater_equal" => "greaterThanOrEqual",
        "less_equal" => "lessThanOrEqual",
        "equal" => "equal",
        "not_equal" => "notEqual",
        "between" => "between",
        "not_between" => "notBetween",
        _ => return None,
    })
}

impl surgical::Cells for Workbook {
    fn number(&self, sheet: &str, col: u32, row: u32) -> Option<f64> {
        match self.book.sheet_by_name(sheet).ok().map(|ws| cell_value(ws.cell((col, row)))) {
            Some(CellValue::Num(x)) => Some(x),
            _ => None,
        }
    }

    fn text(&self, sheet: &str, col: u32, row: u32) -> String {
        match self.book.sheet_by_name(sheet).ok().map(|ws| cell_value(ws.cell((col, row)))) {
            Some(CellValue::Num(x)) => format!("{x}"),
            Some(CellValue::Str(s)) => s,
            Some(CellValue::Bool(b)) => (if b { "TRUE" } else { "FALSE" }).to_string(),
            _ => String::new(),
        }
    }

    fn col_width(&self, sheet: &str, col: u32) -> f64 {
        let Ok(ws) = self.book.sheet_by_name(sheet) else { return 8.43 };
        match ws.column_dimension_by_number(col).map(|c| c.width()) {
            Some(w) if w > 0.0 => w,
            _ => Some(ws.sheet_format_properties().default_column_width()).filter(|w| *w > 0.0).unwrap_or(8.43),
        }
    }

    fn row_height(&self, sheet: &str, row: u32) -> f64 {
        let Ok(ws) = self.book.sheet_by_name(sheet) else { return 15.0 };
        match ws.row_dimension(row).map(|r| r.height()) {
            Some(h) if h > 0.0 => h,
            _ => Some(ws.sheet_format_properties().default_row_height()).filter(|h| *h > 0.0).unwrap_or(15.0),
        }
    }
}

impl Workbook {
    fn sheet_range(&self, text: &str, default_sheet: &str) -> Result<Rng, String> {
        let r = surgical::parse_ref(text, default_sheet)?;
        self.sheet(&r.sheet)?;
        Ok(r)
    }

    /// The cell two columns right of the used range, row 2 (`B2` on an empty sheet).
    fn free_anchor(&self, sheet: &str) -> Result<(u32, u32), String> {
        Ok(match self.used_range(sheet)? {
            Some(u) => (parse_range(&u)?.2 + 2, 2),
            None => (2, 2),
        })
    }

    /// Charts on this workbook that are not saved yet.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Add a chart to `sheet` from cell ranges. The chart refers to the
    /// cells (it follows later edits in Excel/LibreOffice) and carries a
    /// cached copy of their current values for readers that do not
    /// recalculate.
    pub fn add_chart(&mut self, sheet: &str, o: &ChartOptions) -> Result<(), String> {
        self.add_chart_with(sheet, o, None)
    }

    fn add_chart_with(&mut self, sheet: &str, o: &ChartOptions, negated_from: Option<Rng>) -> Result<(), String> {
        self.sheet(sheet)?;
        if o.y.is_empty() {
            return Err("add_chart needs at least one y range".into());
        }
        let n = o.y.len();
        if o.x.len() > 1 && o.x.len() != n {
            return Err(format!("{n} y ranges but {} x ranges -- give one x range for all, or one per series", o.x.len()));
        }
        if !o.names.is_empty() && o.names.len() != n {
            return Err(format!("{n} series but {} names", o.names.len()));
        }
        if !o.colors.is_empty() && o.colors.len() != n {
            return Err(format!("{n} series but {} colors", o.colors.len()));
        }
        if o.equal_axes && o.kind != ChartKind::Scatter {
            return Err("equal_axes= needs a scatter chart -- only there are both axes numeric".into());
        }
        if (o.x_log || o.x_min.is_some() || o.x_max.is_some()) && o.kind != ChartKind::Scatter {
            return Err("x_min=/x_max=/x_log= need a scatter chart -- a line or bar chart's x axis is categories".into());
        }
        for c in &o.colors {
            surgical::hex6(c)?;
        }
        let mut series = Vec::with_capacity(n);
        for (i, y) in o.y.iter().enumerate() {
            let y = self.sheet_range(y, sheet)?;
            if !y.is_line() {
                return Err(format!("y range `{}` is a block -- a series is one row or one column", y.a1()));
            }
            let x = match o.x.len() {
                0 => None,
                1 => Some(self.sheet_range(&o.x[0], sheet)?),
                _ => Some(self.sheet_range(&o.x[i], sheet)?),
            };
            if let Some(x) = &x {
                if !x.is_line() || x.len() != y.len() {
                    return Err(format!("x range `{}` has {} cells but y range `{}` has {}", x.a1(), x.len(), y.a1(), y.len()));
                }
            }
            series.push(SeriesReq { name: o.names.get(i).cloned(), x, y, color: o.colors.get(i).cloned(), negated_from: if i == 0 { negated_from.clone() } else { None } });
        }
        let at = match &o.at {
            Some(a) => parse_cell(a)?,
            None => self.free_anchor(sheet)?,
        };
        let (w, h) = (o.width_mm.unwrap_or(if o.equal_axes { 150.0 } else { 160.0 }), o.height_mm.unwrap_or(if o.equal_axes { 110.0 } else { 90.0 }));
        if !(20.0..=2000.0).contains(&w) || !(20.0..=2000.0).contains(&h) {
            return Err(format!("chart size {w} x {h} mm -- each side must be 20 to 2000 mm"));
        }
        let scatter = o.kind == ChartKind::Scatter;
        let req = ChartReq {
            sheet: sheet.to_string(),
            kind: o.kind,
            title: o.title.clone(),
            x_title: o.x_title.clone(),
            y_title: o.y_title.clone(),
            legend: o.legend.unwrap_or(n > 1 || !o.names.is_empty()),
            series,
            at,
            width_mm: w,
            height_mm: h,
            lines: o.lines.unwrap_or(true),
            markers: o.markers.unwrap_or(scatter),
            x_min: o.x_min,
            x_max: o.x_max,
            y_min: o.y_min,
            y_max: o.y_max,
            x_log: o.x_log,
            y_log: o.y_log,
            equal_axes: o.equal_axes,
        };
        // Build it once now, so a bad axis or colour is reported at the
        // call and not at save time.
        let spec = surgical::chart_spec(&req, self)?;
        qu_ooxml::chart::chart_part(&spec)?;
        self.pending.push(Pending::Chart(req));
        Ok(())
    }

    /// A Nyquist plot (impedance spectroscopy): Z' along x, -Z'' up, both
    /// axes on the same scale so a semicircle looks like one.
    ///
    /// `re`/`im` are the ranges holding Z' and Z''. With `negate` (the
    /// usual case: Z'' stored with its sign, negative for a capacitive
    /// arc) a helper column of `=-Z''` formulas is written -- `helper` names
    /// its column, default the first free column right of the used range --
    /// and plotted; its range is returned. Without it `im` is plotted as is
    /// and no cell changes.
    pub fn add_nyquist_chart(&mut self, sheet: &str, re: &str, im: &str, negate: bool, helper: Option<&str>, mut o: ChartOptions) -> Result<Option<String>, String> {
        let re_r = self.sheet_range(re, sheet)?;
        let im_r = self.sheet_range(im, sheet)?;
        if !re_r.is_line() || !im_r.is_line() || re_r.len() != im_r.len() {
            return Err(format!("Z' range `{}` and Z'' range `{}` must be single columns (or rows) of the same length", re_r.a1(), im_r.a1()));
        }
        let mut plotted = im_r.clone();
        let mut helper_col = None;
        if negate {
            if im_r.c1 != im_r.c2 {
                return Err("negate=true writes a helper COLUMN, so Z'' must be a column range".into());
            }
            let col = match helper {
                Some(c) => parse_cell(&format!("{c}1"))?.0,
                None => self.free_anchor(&im_r.sheet)?.0 - 1,
            };
            if col == im_r.c1 || (col == re_r.c1 && re_r.sheet == im_r.sheet) {
                return Err(format!("helper column {} would overwrite the data", column_letters(col)));
            }
            plotted = Rng { sheet: im_r.sheet.clone(), c1: col, r1: im_r.r1, c2: col, r2: im_r.r2 };
            helper_col = Some(col);
        }
        o.kind = ChartKind::Scatter;
        o.equal_axes = true;
        o.x = vec![re_r.absolute()];
        o.y = vec![plotted.absolute()];
        o.x_title.get_or_insert_with(|| "Z' / Ω".to_string());
        o.y_title.get_or_insert_with(|| "-Z'' / Ω".to_string());
        if o.lines.is_none() {
            o.lines = Some(false);
        }
        // The chart is checked before any cell is written, so a refused
        // call leaves the sheet as it was.
        self.add_chart_with(sheet, &o, negate.then(|| im_r.clone()))?;
        let Some(col) = helper_col else { return Ok(None) };
        let s = im_r.sheet.clone();
        let dest = format!("{}{}:{}{}", column_letters(col), im_r.r1, column_letters(col), im_r.r2);
        self.fill_formula(&s, &dest, &format!("=-{}{}", column_letters(im_r.c1), im_r.r1))?;
        if im_r.r1 > 1 {
            let head = format!("{}{}", column_letters(col), im_r.r1 - 1);
            if self.get(&s, &head)? == CellValue::Empty {
                let label = match self.get(&s, &format!("{}{}", column_letters(im_r.c1), im_r.r1 - 1))? {
                    CellValue::Str(t) if !t.is_empty() => format!("-{t}"),
                    _ => "-Z''".to_string(),
                };
                self.set(&s, &head, &CellValue::Str(label))?;
            }
        }
        Ok(Some(dest))
    }

    /// Highlight the cells of `range` that satisfy a rule, with a fill,
    /// font colour and/or bold.
    pub fn conditional_format(&mut self, sheet: &str, range: &str, rule: CfRule, style: CfStyle) -> Result<(), String> {
        let range = self.sheet_range(range, sheet)?;
        if range.sheet != sheet {
            return Err("the range must be on the sheet being formatted (give it without a sheet name)".into());
        }
        match &rule {
            CfRule::Cell { op, b, .. } => {
                let two = matches!(*op, "between" | "notBetween");
                if two != b.is_some() {
                    return Err(if two { "between/not_between need two values".into() } else { "only between/not_between take a second value".into() });
                }
            }
            CfRule::Contains(t) if t.is_empty() => return Err("contains: the text to look for is empty".into()),
            CfRule::Scale(cs) => {
                if !(2..=3).contains(&cs.len()) {
                    return Err("a colour scale has two or three colours".into());
                }
                for c in cs {
                    surgical::hex6(c)?;
                }
            }
            _ => {}
        }
        if !matches!(rule, CfRule::Scale(_)) {
            if style.fill.is_none() && style.color.is_none() && !style.bold {
                return Err("give the highlight: fill=, color= and/or bold=".into());
            }
            for c in style.fill.iter().chain(style.color.iter()) {
                surgical::hex6(c)?;
            }
        }
        self.pending.push(Pending::Cond(CondReq { range, rule, style }));
        Ok(())
    }

    /// Restrict what can be typed into `range`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_validation(&mut self, sheet: &str, range: &str, kind: ValKind, prompt: Option<String>, prompt_title: Option<String>, error: Option<String>, error_title: Option<String>, error_style: Option<String>, allow_blank: bool) -> Result<(), String> {
        let range = self.sheet_range(range, sheet)?;
        if range.sheet != sheet {
            return Err("the range must be on the sheet being validated (give it without a sheet name)".into());
        }
        match &kind {
            ValKind::List(items) => {
                if items.is_empty() {
                    return Err("a list validation needs at least one value".into());
                }
                if let Some(bad) = items.iter().find(|s| s.contains(',') || s.contains('"')) {
                    return Err(format!("list value \"{bad}\" -- an inline list cannot hold commas or quotes; put the values in cells and use source="));
                }
                let len = items.iter().map(|s| s.chars().count() + 1).sum::<usize>();
                if len > 256 {
                    return Err(format!("the list is {len} characters -- Excel allows 255 inline; put the values in cells and use source="));
                }
            }
            ValKind::ListFrom(r) => {
                self.sheet(&r.sheet)?;
                if !r.is_line() {
                    return Err("source= must be one row or one column".into());
                }
            }
            ValKind::Number(_, lo, hi) => {
                if lo.is_none() && hi.is_none() {
                    return Err("give min=, max= or both".into());
                }
                if let (Some(a), Some(b)) = (lo, hi) {
                    if a > b {
                        return Err(format!("min ({a}) is above max ({b})"));
                    }
                }
            }
            ValKind::Custom(f) if f.trim().trim_start_matches('=').is_empty() => return Err("the custom formula is empty".into()),
            _ => {}
        }
        let error_style = error_style.unwrap_or_else(|| "stop".into());
        if !matches!(error_style.as_str(), "stop" | "warning" | "information") {
            return Err(format!("error_style=\"{error_style}\" -- use stop, warning or information"));
        }
        for (what, v, max) in [("prompt", &prompt, 255), ("prompt_title", &prompt_title, 32), ("error", &error, 255), ("error_title", &error_title, 32)] {
            if v.as_ref().is_some_and(|s| s.chars().count() > max) {
                return Err(format!("{what}= is longer than Excel's {max} characters"));
            }
        }
        self.pending.push(Pending::Valid(ValidReq { range, kind, prompt, prompt_title, error, error_title, error_style, allow_blank }));
        Ok(())
    }
}

/// A rule's comparison value from a Qu number or string (`"=..."` is a formula).
pub fn cf_value_num(x: f64) -> CfValue {
    CfValue::Num(x)
}

pub fn cf_value_text(s: &str) -> CfValue {
    match s.strip_prefix('=') {
        Some(f) => CfValue::Formula(f.to_string()),
        None => CfValue::Text(s.to_string()),
    }
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

fn cell_value(cell: Option<&umya::Cell>) -> CellValue {
    let Some(cell) = cell else { return CellValue::Empty };
    let v = cell.value();
    match cell.data_type() {
        "b" => CellValue::Bool(v == "TRUE" || v == "1"),
        "n" | "" => {
            if v.is_empty() {
                CellValue::Empty
            } else {
                v.parse::<f64>().map(CellValue::Num).unwrap_or_else(|_| CellValue::Str(v.to_string()))
            }
        }
        _ => {
            if v.is_empty() {
                CellValue::Empty
            } else {
                CellValue::Str(v.to_string())
            }
        }
    }
}

/// Shift every relative A1 reference in a formula by (`dc`, `dr`),
/// leaving `$`-anchored parts, string literals and function names alone.
pub fn shift_refs(f: &str, dc: i64, dr: i64) -> String {
    let b = f.as_bytes();
    let mut out = String::with_capacity(f.len() + 8);
    let mut i = 0;
    while i < b.len() {
        let c = f[i..].chars().next().unwrap();
        // String literals and quoted sheet names ('My Data'!A1) pass
        // through untouched; the reference after a sheet name is still
        // shifted on the next pass.
        if c == '"' || c == '\'' {
            let end = f[i + 1..].find(c).map(|e| i + 1 + e + 1).unwrap_or(b.len());
            out.push_str(&f[i..end]);
            i = end;
            continue;
        }
        let prev_ident = i > 0 && ((b[i - 1] as char).is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.');
        if !prev_ident && (c == '$' || c.is_ascii_alphabetic()) {
            // Try to read [$]letters[$]digits not followed by '(' or a letter.
            let mut j = i;
            let col_abs = b[j] == b'$';
            if col_abs {
                j += 1;
            }
            let ls = j;
            while j < b.len() && (b[j] as char).is_ascii_alphabetic() {
                j += 1;
            }
            let letters = &f[ls..j];
            let row_abs = j < b.len() && b[j] == b'$';
            if row_abs {
                j += 1;
            }
            let ds = j;
            while j < b.len() && (b[j] as char).is_ascii_digit() {
                j += 1;
            }
            let digits = &f[ds..j];
            let followed = j < b.len() && ((b[j] as char).is_ascii_alphanumeric() || b[j] == b'(' || b[j] == b'_');
            if !letters.is_empty() && letters.len() <= 3 && !digits.is_empty() && !followed {
                if let Ok((col, row)) = parse_cell(&format!("{letters}{digits}")) {
                    let nc = if col_abs { col as i64 } else { col as i64 + dc };
                    let nr = if row_abs { row as i64 } else { row as i64 + dr };
                    if nc >= 1 && nr >= 1 {
                        out.push_str(&format!(
                            "{}{}{}{}",
                            if col_abs { "$" } else { "" },
                            column_letters(nc as u32),
                            if row_abs { "$" } else { "" },
                            nr
                        ));
                        i = j;
                        continue;
                    }
                }
            }
            // Not a reference: copy the identifier through untouched.
            let mut k = i + 1;
            while k < b.len() && ((b[k] as char).is_ascii_alphanumeric() || b[k] == b'_' || b[k] == b'.') {
                k += 1;
            }
            out.push_str(&f[i..k]);
            i = k;
            continue;
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_and_print() {
        assert_eq!(parse_cell("B3").unwrap(), (2, 3));
        assert_eq!(parse_cell("$AA$10").unwrap(), (27, 10));
        assert_eq!(parse_range("C20:A1").unwrap(), (1, 1, 3, 20));
        assert_eq!(column_letters(28), "AB");
        assert_eq!(column_letters(16384), "XFD");
        assert!(parse_cell("A0").is_err() && parse_cell("1A").is_err() && parse_cell("XFE1").is_err());
    }

    #[test]
    fn fill_down_shifts_only_relative_references() {
        assert_eq!(shift_refs("A2+B2*$C$1", 0, 3), "A5+B5*$C$1");
        assert_eq!(shift_refs("SUM(A1:A10)+$A2+B$2", 1, 1), "SUM(B2:B11)+$A3+C$2");
        assert_eq!(shift_refs("LOG10(A2)&\"B2\"", 0, 1), "LOG10(A3)&\"B2\"", "function names and strings stay");
        assert_eq!(shift_refs("'Données B1'!A1*2+Ω1", 0, 1), "'Données B1'!A2*2+Ω1", "quoted sheet names and non-ASCII pass through");
    }

    #[test]
    fn edit_save_reopen() {
        let dir = std::env::temp_dir().join(format!("qu_xlsx_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.xlsx");
        let p = p.to_str().unwrap();
        let mut wb = Workbook::new();
        wb.set_range("Sheet1", "A1", &[vec![CellValue::Str("f".into()), CellValue::Str("Z".into())], vec![CellValue::Num(1000.0), CellValue::Num(42.5)], vec![CellValue::Num(2000.0), CellValue::Num(40.0)]]).unwrap();
        wb.fill_formula("Sheet1", "C2:C3", "=A2*B2").unwrap();
        wb.format("Sheet1", "A1:B1", &CellFormat { bold: Some(true), background: Some("#DDEEFF".into()), ..Default::default() }).unwrap();
        wb.add_sheet("Notes").unwrap();
        wb.set("Notes", "A1", &CellValue::Bool(true)).unwrap();
        wb.freeze_panes("Sheet1", "A2").unwrap();
        wb.define_name("Frequency", "Sheet1!$A$2:$A$3").unwrap();
        wb.column_width("Sheet1", "A", 14.0).unwrap();
        wb.save(p).unwrap();
        let mut back = Workbook::open(p).unwrap();
        assert_eq!(back.sheet_names(), ["Sheet1", "Notes"]);
        assert_eq!(back.get("Sheet1", "B2").unwrap(), CellValue::Num(42.5));
        assert_eq!(back.get("Sheet1", "A1").unwrap(), CellValue::Str("f".into()));
        assert_eq!(back.formula("Sheet1", "C3").unwrap().as_deref(), Some("A3*B3"));
        assert_eq!(back.get("Notes", "A1").unwrap(), CellValue::Bool(true));
        assert_eq!(back.used_range("Sheet1").unwrap().as_deref(), Some("A1:C3"));
        back.set("Sheet1", "B2", &CellValue::Num(43.0)).unwrap();
        back.rename_sheet("Notes", "Info").unwrap();
        back.save(p).unwrap();
        let again = Workbook::open(p).unwrap();
        assert_eq!(again.get("Sheet1", "B2").unwrap(), CellValue::Num(43.0));
        assert_eq!(again.sheet_names(), ["Sheet1", "Info"]);
        assert!(again.get("Nope", "A1").unwrap_err().contains("Sheet1, Info"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
