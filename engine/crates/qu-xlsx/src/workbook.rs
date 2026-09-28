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

use umya_spreadsheet as umya;

pub struct Workbook {
    book: umya::Workbook,
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
        let book = umya::reader::xlsx::read(path).map_err(err(&format!("xlsx.open: `{path}`")))?;
        Ok(Workbook { book })
    }

    /// A new workbook with one empty sheet, `Sheet1`.
    pub fn new() -> Self {
        Workbook { book: umya::new_file() }
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        umya::writer::xlsx::write(&self.book, path).map_err(err(&format!("xlsx.save: `{path}`")))
    }

    /// The workbook as `.xlsx` bytes, for conversion without a file.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut out = std::io::Cursor::new(Vec::new());
        umya::writer::xlsx::write_writer(&self.book, &mut out).map_err(err("xlsx"))?;
        Ok(out.into_inner())
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.book.sheet_collection().iter().map(|s| s.name().to_string()).collect()
    }

    fn sheet(&self, name: &str) -> Result<&umya::Worksheet, String> {
        self.book.sheet_by_name(name).map_err(|_| format!("no sheet named `{name}` -- the workbook has: {}", self.sheet_names().join(", ")))
    }

    fn sheet_mut(&mut self, name: &str) -> Result<&mut umya::Worksheet, String> {
        let names = self.sheet_names();
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
        self.book.new_sheet(name).map(|_| ()).map_err(err("add_sheet"))
    }

    pub fn rename_sheet(&mut self, old: &str, new: &str) -> Result<(), String> {
        let i = self.sheet_names().iter().position(|n| n == old).ok_or_else(|| format!("no sheet named `{old}`"))?;
        self.book.set_sheet_name(i, new).map_err(err("rename_sheet"))
    }

    pub fn delete_sheet(&mut self, name: &str) -> Result<(), String> {
        if self.sheet_names().len() == 1 {
            return Err("cannot delete the only sheet -- a workbook needs at least one".into());
        }
        self.sheet(name)?;
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
        self.book.add_defined_names(d);
        Ok(())
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
