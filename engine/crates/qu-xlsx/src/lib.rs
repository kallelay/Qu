//! Excel `.xlsx` for Qu, reached through `import xlsx`.
//!
//! A separate crate behind a default-off feature, not another block of
//! builtins in `qu-interp/src/lib.rs`. That file is 57,000 lines and its
//! flat builtin table is already 725 names; several data backends added
//! the old way would have made the language's one real bottleneck
//! materially worse. Everything here is reached as `xlsx.read(...)` and
//! adds nothing to the global namespace -- see `Stmt::Import`.
//!
//! Dependency-light in the way this workspace already distinguishes:
//! `calamine` and `rust_xlsxwriter` are pure Rust with no system library
//! and no build-time codegen, so this feature costs a compile and nothing
//! else. That is what made `.xlsx` the right format to do first -- contrast
//! `hdf5-metno`, already in the tree, which vendors HDF5's C source and
//! needs cmake and an MSVC shell on Windows.
//!
//! The unit of exchange is Qu's own `Table`, so a spreadsheet arrives as
//! the same thing `read_csv` produces and every table builtin already
//! understands.

use calamine::{Data, Reader};

/// A sheet as columns, ready for `Table::from_columns`.
///
/// Typed per column, not per cell: a column is numeric only if every data
/// cell in it is a number (blanks excepted). Excel is happy to let someone
/// type a stray "n/a" into a column of readings, and silently turning that
/// into `NaN` would put a hole in the data that looks like a measurement.
/// Falling back to text keeps what the file said.
pub enum SheetColumn {
    Num(Vec<f64>),
    Str(Vec<String>),
}

pub struct Sheet {
    pub columns: Vec<(String, SheetColumn)>,
}

/// Every sheet name in the workbook, in file order.
pub fn sheet_names(path: &str) -> Result<Vec<String>, String> {
    let wb = calamine::open_workbook_auto(path).map_err(|e| format!("xlsx.sheets: {path}: {e}"))?;
    Ok(wb.sheet_names().to_vec())
}

/// Read one sheet. `sheet` of `None` takes the first, which is what a
/// one-sheet export -- the overwhelmingly common case -- has.
///
/// `headers` treats the first row as column names. With it off, columns
/// are named `A`, `B`, `C`... after Excel's own column letters rather than
/// `col0`/`col1`: the reason to turn headers off is that you are looking
/// at the sheet in Excel while you work, and the names should be the ones
/// on screen there.
pub fn read_sheet(path: &str, sheet: Option<&str>, headers: bool) -> Result<Sheet, String> {
    let mut wb =
        calamine::open_workbook_auto(path).map_err(|e| format!("xlsx.read: {path}: {e}"))?;
    let name = match sheet {
        Some(s) => {
            if !wb.sheet_names().iter().any(|n| n == s) {
                return Err(format!(
                    "xlsx.read: `{path}` has no sheet named `{s}` -- it has: {}",
                    wb.sheet_names().join(", ")
                ));
            }
            s.to_string()
        }
        None => wb
            .sheet_names()
            .first()
            .cloned()
            .ok_or_else(|| format!("xlsx.read: `{path}` has no sheets"))?,
    };
    let range = wb
        .worksheet_range(&name)
        .map_err(|e| format!("xlsx.read: sheet `{name}`: {e}"))?;

    let mut rows = range.rows();
    let header_row: Vec<String> = if headers {
        match rows.next() {
            Some(r) => r.iter().map(cell_text).collect(),
            None => return Ok(Sheet { columns: Vec::new() }),
        }
    } else {
        Vec::new()
    };
    let body: Vec<&[Data]> = rows.collect();
    let width = body
        .iter()
        .map(|r| r.len())
        .chain(std::iter::once(header_row.len()))
        .max()
        .unwrap_or(0);

    let mut columns = Vec::with_capacity(width);
    for c in 0..width {
        let name = match header_row.get(c) {
            Some(h) if !h.trim().is_empty() => h.trim().to_string(),
            // A blank header cell still needs a name, and Excel's own
            // column letter is the one the reader can see in the app.
            _ => column_letter(c),
        };
        let cells: Vec<&Data> = body
            .iter()
            .map(|r| r.get(c).unwrap_or(&Data::Empty))
            .collect();
        columns.push((name, column_of(&cells)));
    }
    Ok(Sheet { columns })
}

/// Excel's column naming: A..Z, AA, AB, ... -- the label actually printed
/// above the column in the application.
pub fn column_letter(mut i: usize) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            return out;
        }
        i = i / 26 - 1;
    }
}

fn column_of(cells: &[&Data]) -> SheetColumn {
    let numeric = cells.iter().all(|c| {
        matches!(c, Data::Float(_) | Data::Int(_) | Data::Empty | Data::Bool(_))
            || matches!(c, Data::String(s) if s.trim().is_empty())
    });
    if numeric {
        SheetColumn::Num(cells.iter().map(|c| cell_num(c)).collect())
    } else {
        SheetColumn::Str(cells.iter().map(|c| cell_text(c)).collect())
    }
}

/// A blank cell is `NaN`, not `0`.
///
/// Zero is a measurement and blank is the absence of one; collapsing them
/// would put a real-looking value where the sheet said nothing, and every
/// statistic computed downstream would quietly include it.
fn cell_num(c: &Data) -> f64 {
    match c {
        Data::Float(f) => *f,
        Data::Int(i) => *i as f64,
        Data::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => f64::NAN,
    }
}

fn cell_text(c: &Data) -> String {
    match c {
        Data::String(s) => s.clone(),
        Data::Float(f) => trim_float(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{e:?}"),
        Data::Empty => String::new(),
    }
}

/// `12.0` should read as `12`, not `12.0`, when it becomes a header or a
/// text cell -- a spreadsheet shows the former, and a surprising number of
/// header rows are numeric years.
fn trim_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// Write columns to a new workbook.
///
/// Numbers go in as numbers, so the file opens with working formulas,
/// sorting and charts rather than text-that-looks-like-numbers -- which is
/// precisely the thing that makes handing someone a CSV annoying.
pub fn write_sheet(
    path: &str,
    sheet: &str,
    columns: &[(String, SheetColumn)],
) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(sheet)
        .map_err(|e| format!("xlsx.write: sheet name `{sheet}`: {e}"))?;
    let bold = Format::new().set_bold();
    for (c, (name, col)) in columns.iter().enumerate() {
        let c = c as u16;
        ws.write_string_with_format(0, c, name.as_str(), &bold)
            .map_err(|e| format!("xlsx.write: header `{name}`: {e}"))?;
        match col {
            SheetColumn::Num(xs) => {
                for (r, x) in xs.iter().enumerate() {
                    // NaN has no Excel representation; a blank cell is what
                    // it means, and what `read` gives back.
                    if x.is_finite() {
                        ws.write_number((r + 1) as u32, c, *x)
                            .map_err(|e| format!("xlsx.write: {e}"))?;
                    }
                }
            }
            SheetColumn::Str(ss) => {
                for (r, s) in ss.iter().enumerate() {
                    ws.write_string((r + 1) as u32, c, s.as_str())
                        .map_err(|e| format!("xlsx.write: {e}"))?;
                }
            }
        }
    }
    wb.save(path).map_err(|e| format!("xlsx.write: {path}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_column_letters_match_the_application() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(27), "AB");
        assert_eq!(column_letter(51), "AZ");
        assert_eq!(column_letter(52), "BA");
    }

    #[test]
    fn a_column_with_one_stray_word_stays_text() {
        // Excel lets someone type "n/a" into a column of readings.
        // Coercing that to NaN would put a hole in the data that looks like
        // a measurement; keeping the column as text keeps what the file
        // actually said.
        let nums = [Data::Float(1.0), Data::Int(2)];
        let mixed = [Data::Float(1.0), Data::String("n/a".into())];
        assert!(matches!(
            column_of(&nums.iter().collect::<Vec<_>>()),
            SheetColumn::Num(_)
        ));
        assert!(matches!(
            column_of(&mixed.iter().collect::<Vec<_>>()),
            SheetColumn::Str(_)
        ));
        // A blank does NOT force a column to text -- it is an absent
        // reading, not a value of a different kind.
        let gap = [Data::Float(1.0), Data::Empty, Data::Float(3.0)];
        match column_of(&gap.iter().collect::<Vec<_>>()) {
            SheetColumn::Num(xs) => assert!(xs[1].is_nan(), "a blank cell is NaN, not 0"),
            SheetColumn::Str(_) => panic!("a blank should not force the column to text"),
        }
    }

    #[test]
    fn a_whole_number_reads_back_without_a_decimal_point() {
        assert_eq!(trim_float(12.0), "12");
        assert_eq!(trim_float(2026.0), "2026");
        assert_eq!(trim_float(1.5), "1.5");
    }
}
