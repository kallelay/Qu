//! Sheet operations (copy, hide, clear, move, sort, autofilter), Excel
//! Tables, cell notes and pictures: known answers read back out of the
//! saved package, a chart saved earlier surviving every one of them, and --
//! where the machine has them -- LibreOffice and openpyxl reading the file
//! the way a user's spreadsheet program would.

use qu_ooxml::xml::Element;
use qu_ooxml::Package;
use qu_xlsx::surgical::{parse_ref, TotalFn};
use qu_xlsx::workbook::{CellFormat, CellValue, ChartKind, ChartOptions, ClearWhat, SheetVisibility, SortKey, Workbook};

fn tmp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("qu_xlsx_sheetops_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().into_owned()
}

fn s(x: &str) -> CellValue {
    CellValue::Str(x.into())
}

fn n(x: f64) -> CellValue {
    CellValue::Num(x)
}

/// Readings on `Data`: a header, five rows (name, value, 2x value as a
/// formula with no cached result), an empty value in one, and a chart of
/// the values saved on the sheet. Returns the path.
fn fixture(name: &str) -> String {
    let path = tmp(name);
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "Data").unwrap();
    wb.set_range(
        "Data",
        "A1",
        &[
            vec![s("name"), s("value"), s("double")],
            vec![s("delta"), n(3.0), CellValue::Empty],
            vec![s("Alpha"), n(10.0), CellValue::Empty],
            vec![s("charlie"), n(1.0), CellValue::Empty],
            vec![s("bravo"), CellValue::Empty, CellValue::Empty],
            vec![s("echo"), n(7.0), CellValue::Empty],
        ],
    )
    .unwrap();
    wb.fill_formula("Data", "C2:C6", "=B2*2").unwrap();
    wb.format("Data", "A1:C1", &CellFormat { bold: Some(true), background: Some("#DDEEFF".into()), ..Default::default() }).unwrap();
    wb.format("Data", "A4:A4", &CellFormat { italic: Some(true), ..Default::default() }).unwrap();
    wb.add_sheet("Notes").unwrap();
    wb.set("Notes", "A1", &s("keep me")).unwrap();
    let mut o = ChartOptions::new(ChartKind::Column);
    o.y = vec!["B2:B6".into()];
    o.x = vec!["A2:A6".into()];
    o.title = Some("Values".into());
    wb.add_chart("Data", &o).unwrap();
    wb.save(&path).unwrap();
    path
}

fn part(pkg: &Package, name: &str) -> Element {
    pkg.get_xml(name).unwrap_or_else(|e| panic!("{e}; parts: {:?}", pkg.names())).root
}

/// The chart parts the sheet's drawing points at.
fn charts_of(pkg: &Package, sheet: &str) -> Vec<String> {
    let sp = qu_xlsx::surgical::sheet_part(pkg, sheet).unwrap();
    let ws = part(pkg, &sp);
    let Some(d) = ws.find_all("drawing").first().and_then(|e| e.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone())) else { return Vec::new() };
    let rel = pkg.rels(&sp).unwrap().into_iter().find(|r| r.id == d).unwrap();
    let dp = qu_ooxml::resolve_target(&sp, &rel.target);
    pkg.rels(&dp).unwrap().into_iter().filter(|r| r.rel_type.ends_with("/chart")).map(|r| qu_ooxml::resolve_target(&dp, &r.target)).collect()
}

fn column(wb: &Workbook, sheet: &str, col: &str, rows: std::ops::RangeInclusive<u32>) -> Vec<CellValue> {
    rows.map(|r| wb.get(sheet, &format!("{col}{r}")).unwrap()).collect()
}

#[test]
fn sort_moves_whole_rows_with_their_formulas_and_formats() {
    let src = fixture("sort_src.xlsx");
    let mut wb = Workbook::open(&src).unwrap();
    // By value, descending; the header stays; the empty value goes last.
    wb.sort_range("Data", "A1:C6", &[(SortKey::Letter("B".into()), true)], true).unwrap();
    assert_eq!(column(&wb, "Data", "A", 1..=6), [s("name"), s("Alpha"), s("echo"), s("delta"), s("charlie"), s("bravo")]);
    assert_eq!(column(&wb, "Data", "B", 2..=6), [n(10.0), n(7.0), n(3.0), n(1.0), CellValue::Empty]);
    // The formula that sat beside `charlie` (row 4) follows it to row 5 and
    // now refers to row 5's value, as Excel's sort leaves it.
    assert_eq!(wb.formula("Data", "C5").unwrap().as_deref(), Some("B5*2"));
    assert_eq!(wb.formula("Data", "C2").unwrap().as_deref(), Some("B2*2"));
    // `charlie` was italic; it still is, one row down.
    let out = tmp("sort_out.xlsx");
    wb.save(&out).unwrap();
    let back = Workbook::open(&out).unwrap();
    assert_eq!(back.get("Data", "A5").unwrap(), s("charlie"));
    assert_eq!(column(&back, "Data", "A", 2..=6)[3], s("charlie"));
    let styles = part(&Package::open(&out).unwrap(), "xl/styles.xml");
    assert!(styles.find_all("i").len() >= 1, "the italic font is still in the stylesheet");

    // Text keys sort case-blind, ascending; position 1 = first column of the range.
    let mut wb = Workbook::open(&src).unwrap();
    wb.sort_range("Data", "A2:C6", &[(SortKey::Position(1), false)], false).unwrap();
    assert_eq!(column(&wb, "Data", "A", 2..=6), [s("Alpha"), s("bravo"), s("charlie"), s("delta"), s("echo")]);
    // Two keys: ties on the first are broken by the second.
    let mut wb = Workbook::new();
    wb.set_range("Sheet1", "A1", &[vec![s("g"), n(2.0)], vec![s("a"), n(9.0)], vec![s("g"), n(1.0)], vec![s("a"), n(3.0)]]).unwrap();
    wb.sort_range("Sheet1", "A1:B4", &[(SortKey::Letter("A".into()), false), (SortKey::Letter("B".into()), true)], false).unwrap();
    assert_eq!(column(&wb, "Sheet1", "B", 1..=4), [n(9.0), n(3.0), n(2.0), n(1.0)]);
    // Numbers before text before booleans, blanks last in either direction.
    let mut wb = Workbook::new();
    wb.set_range("Sheet1", "A1", &[vec![CellValue::Bool(true)], vec![s("b")], vec![CellValue::Empty], vec![n(5.0)], vec![s("A")], vec![n(-1.0)]]).unwrap();
    wb.sort_range("Sheet1", "A1:A6", &[(SortKey::Letter("A".into()), false)], false).unwrap();
    assert_eq!(column(&wb, "Sheet1", "A", 1..=6), [n(-1.0), n(5.0), s("A"), s("b"), CellValue::Bool(true), CellValue::Empty]);
    wb.sort_range("Sheet1", "A1:A6", &[(SortKey::Letter("A".into()), true)], false).unwrap();
    assert_eq!(column(&wb, "Sheet1", "A", 1..=6), [CellValue::Bool(true), s("b"), s("A"), n(5.0), n(-1.0), CellValue::Empty]);

    // Refusals leave the sheet alone.
    let mut wb = Workbook::open(&src).unwrap();
    assert!(wb.sort_range("Data", "A1:C6", &[(SortKey::Letter("D".into()), false)], true).unwrap_err().contains("outside"));
    assert!(wb.sort_range("Data", "A1:C6", &[(SortKey::Position(4), false)], true).unwrap_err().contains("3 columns"));
    assert!(wb.sort_range("Data", "A1:C1", &[(SortKey::Position(1), false)], true).unwrap_err().contains("no rows"));
    assert!(wb.sort_range("Data", "A1:C6", &[(SortKey::Letter("C".into()), false)], true).unwrap_err().contains("no stored value"), "a formula nobody has calculated cannot be a key");
    wb.merge("Data", "A3:B3").unwrap();
    assert!(wb.sort_range("Data", "A1:C6", &[(SortKey::Letter("A".into()), false)], true).unwrap_err().contains("merged"));
    assert_eq!(wb.get("Data", "A2").unwrap(), s("delta"));
}

#[test]
fn clear_and_move_range_known_answers() {
    let src = fixture("mv_src.xlsx");
    let mut wb = Workbook::open(&src).unwrap();
    // Move B2:C3 to E2: values and formulas go, the source empties, the
    // formula keeps its text (a move, not a copy).
    assert_eq!(wb.move_range("Data", "B2:C3", "E2", None, false).unwrap(), 4);
    assert_eq!(column(&wb, "Data", "B", 2..=3), [CellValue::Empty, CellValue::Empty]);
    assert_eq!(wb.get("Data", "E3").unwrap(), n(10.0));
    assert_eq!(wb.formula("Data", "F2").unwrap().as_deref(), Some("B2*2"));
    assert_eq!(wb.get("Data", "B4").unwrap(), n(1.0), "cells outside the range are untouched");
    // Copy to another sheet: the source stays, relative references shift as in a paste.
    assert_eq!(wb.move_range("Data", "A4:C4", "B5", Some("Notes"), true).unwrap(), 3);
    assert_eq!(wb.get("Data", "A4").unwrap(), s("charlie"));
    assert_eq!(wb.get("Notes", "B5").unwrap(), s("charlie"));
    assert_eq!(wb.formula("Notes", "D5").unwrap().as_deref(), Some("C5*2"), "C4's =B4*2 moved one column right and one row down");
    // Overlapping move: shifted down by one, nothing lost or doubled.
    let mut wb = Workbook::new();
    wb.set_range("Sheet1", "A1", &[vec![n(1.0)], vec![n(2.0)], vec![n(3.0)]]).unwrap();
    wb.move_range("Sheet1", "A1:A3", "A2", None, false).unwrap();
    assert_eq!(column(&wb, "Sheet1", "A", 1..=4), [CellValue::Empty, n(1.0), n(2.0), n(3.0)]);
    // Destination cells are overwritten, including with blanks.
    let mut wb = Workbook::new();
    wb.set_range("Sheet1", "A1", &[vec![n(1.0), CellValue::Empty], vec![n(2.0), n(99.0)]]).unwrap();
    wb.move_range("Sheet1", "A1:A2", "B1", None, true).unwrap();
    assert_eq!(column(&wb, "Sheet1", "B", 1..=2), [n(1.0), n(2.0)]);
    assert!(wb.move_range("Sheet1", "A1:A2", "XFD1048576", None, false).unwrap_err().contains("off the sheet"));
    assert!(wb.move_range("Sheet1", "A1:A2", "A1", None, false).unwrap_err().contains("already"));

    // Clear: contents keep the formatting, formats keep the contents, all removes both.
    let mut wb = Workbook::open(&src).unwrap();
    assert_eq!(wb.clear("Data", "A1:C2", ClearWhat::Contents).unwrap(), 6);
    assert_eq!((wb.get("Data", "A1").unwrap(), wb.get("Data", "B2").unwrap(), wb.formula("Data", "C2").unwrap()), (CellValue::Empty, CellValue::Empty, None));
    wb.clear("Data", "A3:C3", ClearWhat::Formats).unwrap();
    assert_eq!(wb.get("Data", "A3").unwrap(), s("Alpha"));
    wb.clear("Data", "A4:C6", ClearWhat::All).unwrap();
    assert_eq!(wb.used_range("Data").unwrap().as_deref(), Some("A3:C3"));
    let out = tmp("clear_out.xlsx");
    wb.save(&out).unwrap();
    let back = Workbook::open(&out).unwrap();
    assert_eq!(back.get("Data", "B3").unwrap(), n(10.0));
    assert_eq!(back.used_range("Data").unwrap().as_deref(), Some("A3:C3"));
    // Clearing an empty region touches nothing.
    assert_eq!(wb.clear("Data", "H1:K9", ClearWhat::All).unwrap(), 0);
    assert!(ClearWhat::parse("everything").unwrap_err().contains("all, contents or formats"));
}

#[test]
fn hide_copy_and_autofilter_known_answers() {
    let src = fixture("hide_src.xlsx");
    let out = tmp("hide_out.xlsx");
    let mut wb = Workbook::open(&src).unwrap();
    wb.set_sheet_visibility("Notes", SheetVisibility::VeryHidden).unwrap();
    wb.copy_sheet("Data", "Data (2)").unwrap();
    wb.autofilter("Data", Some("A1:C6")).unwrap();
    assert!(wb.set_sheet_visibility("Data", SheetVisibility::Hidden).is_ok(), "Data (2) is still visible");
    assert!(wb.set_sheet_visibility("Data (2)", SheetVisibility::Hidden).unwrap_err().contains("at least one visible"));
    wb.set_sheet_visibility("Data", SheetVisibility::Visible).unwrap();
    assert_eq!(wb.sheet_names(), ["Data", "Notes", "Data (2)"]);
    assert_eq!(wb.autofilter_range("Data").unwrap().as_deref(), Some("A1:C6"));
    assert!(wb.copy_sheet("Data", "data (2)").unwrap_err().contains("already"));
    assert!(wb.copy_sheet("Data", "bad/name").unwrap_err().contains("allow"));
    assert!(wb.copy_sheet("Nope", "X").unwrap_err().contains("Nope"));
    wb.save(&out).unwrap();

    let pkg = Package::open(&out).unwrap();
    let wbx = part(&pkg, "xl/workbook.xml");
    let sheets: Vec<(String, Option<String>)> = wbx.find_all("sheet").iter().map(|e| (e.attr("name").unwrap().to_string(), e.attr("state").map(String::from))).collect();
    let vis = |v: &Option<String>| v.as_deref().unwrap_or("visible").to_string();
    assert_eq!(sheets.iter().map(|(n, v)| (n.clone(), vis(v))).collect::<Vec<_>>(), [("Data".into(), "visible".into()), ("Notes".into(), "veryHidden".into()), ("Data (2)".into(), "visible".into())]);
    let ws = part(&pkg, &qu_xlsx::surgical::sheet_part(&pkg, "Data").unwrap());
    assert_eq!(ws.find_all("autoFilter")[0].attr("ref"), Some("A1:C6"));
    let order: Vec<&str> = ws.elems().map(|e| e.name.as_str()).collect();
    assert!(order.iter().position(|x| *x == "autoFilter") > order.iter().position(|x| *x == "sheetData"));
    // The copy has the cells and the formulas of the original.
    let back = Workbook::open(&out).unwrap();
    assert_eq!(back.get("Data (2)", "A3").unwrap(), s("Alpha"));
    assert_eq!(back.formula("Data (2)", "C4").unwrap().as_deref(), Some("B4*2"));
    assert_eq!(back.sheet_state("Notes").unwrap(), SheetVisibility::VeryHidden);
    assert_eq!(back.sheet_state("Data (2)").unwrap(), SheetVisibility::Visible);
    // Only one sheet is selected, so Excel does not open in group-edit mode.
    let selected: Vec<bool> = ["Data", "Data (2)"]
        .iter()
        .map(|nme| part(&pkg, &qu_xlsx::surgical::sheet_part(&pkg, nme).unwrap()).find_all("sheetView").iter().any(|v| v.attr("tabSelected") == Some("1")))
        .collect();
    assert!(!(selected[0] && selected[1]), "both copies are tab-selected: {selected:?}");
    // The copy has its own chart (the original's chart is not shared).
    let c0 = charts_of(&pkg, "Data");
    let c2 = charts_of(&pkg, "Data (2)");
    assert_eq!((c0.len(), c2.len()), (1, 1), "parts: {:?}", pkg.names());
    assert_ne!(c0, c2, "each sheet has its own chart part");

    // Hiding the active sheet moves the active tab to a visible one.
    let mut wb = Workbook::open(&src).unwrap();
    wb.set_sheet_visibility("Data", SheetVisibility::Hidden).unwrap();
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let view = part(&pkg, "xl/workbook.xml").find_all("workbookView")[0].attr("activeTab").map(String::from);
    assert_eq!(view.as_deref(), Some("1"));
    // The filter comes off again.
    let mut wb = Workbook::open(&out).unwrap();
    wb.autofilter("Notes", Some("A1:A2")).unwrap();
    wb.autofilter("Notes", None).unwrap();
    assert_eq!(wb.autofilter_range("Notes").unwrap(), None);
    assert!(wb.autofilter("Notes", Some("A1")).unwrap_err().contains("single cell"));
}

#[test]
fn a_copied_sheet_carries_the_pending_edits_and_gets_its_own_table_name() {
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "T").unwrap();
    wb.set_range("T", "A1", &[vec![s("k"), s("v")], vec![s("a"), n(1.0)], vec![s("b"), n(2.0)]]).unwrap();
    wb.create_table("T", "A1:B3", "Readings", true, None, true, None).unwrap();
    wb.add_comment("T", "B2", "first", None).unwrap();
    let mut o = ChartOptions::new(ChartKind::Line);
    o.y = vec!["B2:B3".into()];
    wb.add_chart("T", &o).unwrap();
    wb.copy_sheet("T", "U").unwrap();
    let out = tmp("copy_pending.xlsx");
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let mut tables: Vec<(String, String)> = pkg.names().iter().filter(|p| p.starts_with("xl/tables/table")).map(|p| {
        let t = part(&pkg, p);
        (t.attr("name").unwrap().to_string(), t.attr("id").unwrap().to_string())
    }).collect();
    tables.sort();
    assert_eq!(tables, [("Readings".to_string(), "1".to_string()), ("Readings_2".to_string(), "2".to_string())]);
    let charts: Vec<String> = pkg.names().into_iter().filter(|p| p.starts_with("xl/charts/chart")).collect();
    assert_eq!(charts.len(), 2);
    let formulas: Vec<String> = charts.iter().flat_map(|c| part(&pkg, c).find_all("c:f").iter().map(|f| f.text()).collect::<Vec<_>>()).collect();
    assert!(formulas.contains(&"T!$B$2:$B$3".to_string()) && formulas.contains(&"U!$B$2:$B$3".to_string()), "the copy's chart reads the copy: {formulas:?}");
    let comments: Vec<&String> = pkg.names().iter().filter(|p| p.starts_with("xl/comments")).collect::<Vec<_>>().into_iter().map(|_| &tables[0].0).collect();
    assert_eq!(comments.len(), 2);
}

#[test]
fn table_known_answers() {
    let out = tmp("table.xlsx");
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "T").unwrap();
    // Header cells that are empty, numeric or repeated are made into unique text.
    wb.set_range(
        "T",
        "A1",
        &[
            vec![s("Sample"), s("Sales"), n(2024.0), s("sales"), CellValue::Empty],
            vec![s("a"), n(1.5), n(1.0), s("x"), n(4.0)],
            vec![s("b"), n(2.5), n(2.0), s("y"), n(5.0)],
            vec![s("c"), n(3.0), n(3.0), s("z"), n(6.0)],
        ],
    )
    .unwrap();
    wb.create_table("T", "A1:E4", "Measurements", true, Some("medium9"), true, Some(TotalFn::Sum)).unwrap();
    assert_eq!(column(&wb, "T", "C", 1..=1), [s("2024")]);
    assert_eq!(wb.get("T", "D1").unwrap(), s("sales2"));
    assert_eq!(wb.get("T", "E1").unwrap(), s("Column5"));
    // Totals row below the range: a label under the first (text) column,
    // SUBTOTAL under numeric ones, nothing under the text one.
    assert_eq!(wb.get("T", "A5").unwrap(), s("Total"));
    assert_eq!(wb.formula("T", "B5").unwrap().as_deref(), Some("SUBTOTAL(109,B2:B4)"));
    assert_eq!(wb.formula("T", "E5").unwrap().as_deref(), Some("SUBTOTAL(109,E2:E4)"));
    assert_eq!(wb.get("T", "D5").unwrap(), CellValue::Empty);
    // A header-less table of data and a plain one, on another sheet, with the name rules.
    wb.add_sheet("U").unwrap();
    wb.set_range("U", "B2", &[vec![n(1.0), n(2.0)], vec![n(3.0), n(4.0)]]).unwrap();
    wb.create_table("U", "B2:C3", "Plain", false, Some("none"), false, None).unwrap();
    assert!(wb.create_table("U", "E1:F3", "measurements", true, None, true, None).unwrap_err().contains("already has a table"));
    assert!(wb.create_table("U", "E1:F3", "my table", true, None, true, None).unwrap_err().contains("no spaces"));
    assert!(wb.create_table("U", "E1:F3", "B7", true, None, true, None).unwrap_err().contains("cell address"));
    assert!(wb.create_table("U", "B2:C3", "Other", true, None, true, None).unwrap_err().contains("overlaps"));
    assert!(wb.create_table("U", "E1:F1", "OneRow", true, None, true, None).unwrap_err().contains("too short"));
    assert!(wb.create_table("U", "E1:F3", "Fancy", true, Some("rainbow"), true, None).unwrap_err().contains("table style"));
    assert!(wb.create_table("T", "A9:B10", "Nope", true, None, true, Some(TotalFn::Max)).is_ok());
    wb.set("T", "D12", &s("block")).unwrap();
    assert!(wb.create_table("T", "D8:E11", "Blocked", true, None, true, Some(TotalFn::Min)).unwrap_err().contains("not empty"));
    wb.save(&out).unwrap();

    let pkg = Package::open(&out).unwrap();
    let t1 = part(&pkg, "xl/tables/table1.xml");
    assert_eq!((t1.attr("name"), t1.attr("displayName"), t1.attr("ref"), t1.attr("id"), t1.attr("totalsRowCount")), (Some("Measurements"), Some("Measurements"), Some("A1:E5"), Some("1"), Some("1")));
    assert_eq!(t1.find_all("autoFilter")[0].attr("ref"), Some("A1:E4"), "the filter covers the data, not the totals row");
    let cols: Vec<(&str, Option<&str>, Option<&str>)> = t1.find_all("tableColumn").iter().map(|c| (c.attr("name").unwrap(), c.attr("totalsRowFunction"), c.attr("totalsRowLabel"))).collect();
    assert_eq!(
        cols,
        [("Sample", None, Some("Total")), ("Sales", Some("sum"), None), ("2024", Some("sum"), None), ("sales2", None, None), ("Column5", Some("sum"), None)]
    );
    assert_eq!(t1.find_all("tableColumns")[0].attr("count"), Some("5"));
    let st = t1.find_all("tableStyleInfo")[0];
    assert_eq!((st.attr("name"), st.attr("showRowStripes")), (Some("TableStyleMedium9"), Some("1")));
    let t2 = part(&pkg, "xl/tables/table2.xml");
    assert_eq!((t2.attr("name"), t2.attr("ref"), t2.attr("headerRowCount"), t2.attr("id")), (Some("Plain"), Some("B2:C3"), Some("0"), Some("2")));
    assert!(t2.find_all("autoFilter").is_empty() && t2.find_all("tableStyleInfo").is_empty());
    assert_eq!(t2.find_all("tableColumn")[1].attr("name"), Some("Column2"));
    // The sheets reference them, in schema position, with content types.
    let ws = part(&pkg, &qu_xlsx::surgical::sheet_part(&pkg, "T").unwrap());
    let tp = ws.find_all("tableParts")[0];
    assert_eq!(tp.attr("count"), Some("2"), "T holds Measurements and Nope");
    let names: Vec<&str> = ws.elems().map(|e| e.name.as_str()).collect();
    assert_eq!(*names.last().unwrap(), "tableParts");
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap().to_vec()).unwrap();
    assert!(ct.contains("/xl/tables/table1.xml") && ct.contains("spreadsheetml.table+xml"));
    // Reopen, edit a cell, save: the tables are still there.
    let mut again = Workbook::open(&out).unwrap();
    again.set("T", "B2", &n(100.0)).unwrap();
    let out2 = tmp("table2.xlsx");
    again.save(&out2).unwrap();
    let pkg2 = Package::open(&out2).unwrap();
    let tabs: Vec<String> = pkg2.names().into_iter().filter(|p| p.starts_with("xl/tables/table")).collect();
    assert_eq!(tabs.len(), 3, "parts: {:?}", pkg2.names());
    let mut kept: Vec<String> = tabs.iter().map(|p| part(&pkg2, p).attr("name").unwrap().to_string()).collect();
    kept.sort();
    assert_eq!(kept, ["Measurements", "Nope", "Plain"]);
    // ... and a table made on top of those gets a fresh id and name check.
    let mut third = Workbook::open(&out2).unwrap();
    third.add_sheet("V").unwrap();
    third.set_range("V", "A1", &[vec![s("h")], vec![n(1.0)]]).unwrap();
    third.create_table("V", "A1:A2", "Later", true, None, true, None).unwrap();
    assert!(third.create_table("V", "C1:C2", "Plain", true, None, true, None).unwrap_err().contains("already has a table"), "names saved in the file count too");
    let out3 = tmp("table3.xlsx");
    third.save(&out3).unwrap();
    let pkg3 = Package::open(&out3).unwrap();
    let mut ids: Vec<u32> = pkg3.names().iter().filter(|p| p.starts_with("xl/tables/table")).map(|p| part(&pkg3, p).attr("id").unwrap().parse().unwrap()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4, "table ids are unique: {ids:?}");
}

#[test]
fn comments_known_answers() {
    let src = fixture("cm_src.xlsx");
    let out = tmp("cm_out.xlsx");
    let mut wb = Workbook::open(&src).unwrap();
    wb.add_comment("Data", "B3", "Check this <value> & \"quote\"\nsecond line", Some("Ahmed")).unwrap();
    wb.add_comment("Data", "A1", "header", None).unwrap();
    wb.add_comment("Data", "B3", "replaced", Some("Qu")).unwrap();
    wb.add_comment("Notes", "A1", "on the other sheet", None).unwrap();
    assert!(wb.add_comment("Data", "B3", "", None).unwrap_err().contains("empty"));
    assert!(wb.add_comment("Nope", "B3", "x", None).unwrap_err().contains("Nope"));
    assert!(wb.add_comment("Data", "ZZZZ1", "x", None).unwrap_err().contains("outside"));
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let names = pkg.names();
    assert_eq!(names.iter().filter(|p| p.starts_with("xl/comments")).count(), 2);
    assert_eq!(names.iter().filter(|p| p.ends_with(".vml")).count(), 2);
    let sp = qu_xlsx::surgical::sheet_part(&pkg, "Data").unwrap();
    let rels = pkg.rels(&sp).unwrap();
    let cpart = qu_ooxml::resolve_target(&sp, &rels.iter().find(|r| r.rel_type == qu_xlsx::surgical::REL_COMMENTS).unwrap().target);
    let c = part(&pkg, &cpart);
    let list: Vec<(String, String, String)> = c.find_all("comment").iter().map(|e| (e.attr("ref").unwrap().to_string(), e.attr("authorId").unwrap().to_string(), e.text())).collect();
    // Reading order (A1 before B3), the second B3 note replaced the first.
    assert_eq!(list, [("A1".into(), "1".into(), "header".into()), ("B3".into(), "1".into(), "replaced".into())]);
    let authors: Vec<String> = c.find_all("author").iter().map(|a| a.text()).collect();
    assert_eq!(authors, ["Ahmed", "Qu"], "one entry per author, in first-use order");
    let ws = part(&pkg, &sp);
    assert_eq!(ws.find_all("legacyDrawing").len(), 1);
    let vml = rels.iter().find(|r| r.rel_type == qu_xlsx::surgical::REL_VML).unwrap();
    let vtext = pkg.get_str(&qu_ooxml::resolve_target(&sp, &vml.target)).unwrap();
    assert_eq!(vtext.matches("<v:shape ").count(), 2, "one box per note: {vtext}");
    assert!(vtext.contains("<x:Row>2</x:Row><x:Column>1</x:Column>") && vtext.contains("<x:Row>0</x:Row><x:Column>0</x:Column>"));
    qu_ooxml::xml::parse(&vtext).expect("the VML is well-formed XML");
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap().to_vec()).unwrap();
    assert!(ct.contains("Extension=\"vml\"") && ct.contains("spreadsheetml.comments+xml"));
    // The chart on the same sheet is still wired up.
    assert_eq!(charts_of(&pkg, "Data").len(), 1);

    // A later, ordinary cell edit keeps the notes and a new note joins them.
    let mut again = Workbook::open(&out).unwrap();
    again.set("Data", "A2", &s("edited")).unwrap();
    again.add_comment("Data", "C5", "added after reopening", Some("Qu")).unwrap();
    let out2 = tmp("cm_out2.xlsx");
    again.save(&out2).unwrap();
    let pkg2 = Package::open(&out2).unwrap();
    let sp2 = qu_xlsx::surgical::sheet_part(&pkg2, "Data").unwrap();
    let cp2 = pkg2.rels(&sp2).unwrap().into_iter().find(|r| r.rel_type == qu_xlsx::surgical::REL_COMMENTS).expect("the comments survived the cell edit");
    let c2 = part(&pkg2, &qu_ooxml::resolve_target(&sp2, &cp2.target));
    let refs: Vec<&str> = c2.find_all("comment").iter().map(|e| e.attr("ref").unwrap()).collect();
    assert_eq!(refs, ["A1", "B3", "C5"]);
    let texts: Vec<String> = c2.find_all("comment").iter().map(|e| e.text()).collect();
    assert_eq!(texts, ["header", "replaced", "added after reopening"]);
    assert_eq!(charts_of(&pkg2, "Data").len(), 1);
}

/// A valid w x h PNG (a colour gradient), written with stored deflate
/// blocks so no compressor is needed.
fn png(w: u32, h: u32) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for b in data {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 160]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for x in &raw {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The header of a JPEG with a start-of-frame segment: enough for the
/// package layer, which never decodes pixels.
fn jpeg_header(w: u16, h: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0];
    v.extend([0xFF, 0xC0, 0x00, 0x0B, 8]);
    v.extend(h.to_be_bytes());
    v.extend(w.to_be_bytes());
    v.extend([1, 1, 0x11, 0, 0xFF, 0xD9]);
    v
}

#[test]
fn image_known_answers() {
    let src = fixture("im_src.xlsx");
    let out = tmp("im_out.xlsx");
    let mut wb = Workbook::open(&src).unwrap();
    // 96 px wide = 25.4 mm at 96 dpi; 48 px high = 12.7 mm.
    wb.add_image("Data", png(96, 48), Some("F2"), None, None, Some("a gradient")).unwrap();
    wb.add_image("Data", jpeg_header(300, 100), Some("F10"), Some(60.0), None, None).unwrap();
    wb.add_image("Notes", png(1920, 1080), None, None, None, None).unwrap();
    assert!(wb.add_image("Data", b"GIF".to_vec(), None, None, None, None).is_err());
    assert!(wb.add_image("Data", b"not an image at all, just bytes".to_vec(), None, None, None, None).unwrap_err().contains("PNG, JPEG"));
    assert!(wb.add_image("Data", png(10, 10), None, Some(5000.0), None, None).unwrap_err().contains("2000"));
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let mut media: Vec<String> = pkg.names().into_iter().filter(|p| p.starts_with("xl/media/")).collect();
    media.sort();
    assert_eq!(media, ["xl/media/image1.jpeg", "xl/media/image1.png", "xl/media/image2.png"]);
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap().to_vec()).unwrap();
    assert!(ct.contains("Extension=\"png\"") && ct.contains("Extension=\"jpeg\""));
    // `Data` already had a drawing with the chart: the pictures join it.
    let sp = qu_xlsx::surgical::sheet_part(&pkg, "Data").unwrap();
    let dr = pkg.rels(&sp).unwrap().into_iter().find(|r| r.rel_type.ends_with("/drawing")).unwrap();
    let dpart = qu_ooxml::resolve_target(&sp, &dr.target);
    let d = part(&pkg, &dpart);
    let anchors = d.find_all("xdr:twoCellAnchor");
    assert_eq!(anchors.len(), 3, "chart + 2 pictures in one drawing");
    assert_eq!(anchors.iter().filter(|a| !a.find_all("xdr:pic").is_empty()).count(), 2);
    let ids: Vec<&str> = d.find_all("xdr:cNvPr").iter().map(|e| e.attr("id").unwrap()).collect();
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), ids.len(), "shape ids are unique: {ids:?}");
    let pic = d.find_all("xdr:pic")[0];
    assert_eq!(pic.find_all("xdr:cNvPr")[0].attr("descr"), Some("a gradient"));
    let ext = pic.find_all("a:ext")[0];
    assert_eq!((ext.attr("cx"), ext.attr("cy")), (Some("914400"), Some("457200")), "1 in x 0.5 in at 96 dpi");
    let rid = pic.find_all("a:blip")[0].attr("r:embed").unwrap();
    let rel = pkg.rels(&dpart).unwrap().into_iter().find(|r| r.id == rid).unwrap();
    assert_eq!(qu_ooxml::resolve_target(&dpart, &rel.target), "xl/media/image1.png");
    assert_eq!(pkg.get("xl/media/image1.png").unwrap(), &png(96, 48)[..], "the bytes are stored as given");
    // The second picture: 60 mm wide keeps 3:1.
    let jext = d.find_all("xdr:pic")[1].find_all("a:ext")[0];
    assert_eq!((jext.attr("cx"), jext.attr("cy")), (Some("2160000"), Some("720000")));
    let from = anchors[1].child("xdr:from").unwrap();
    assert_eq!((from.child("xdr:col").unwrap().text(), from.child("xdr:row").unwrap().text()), ("5".to_string(), "1".to_string()), "F2");
    // The big picture was scaled down to 160 mm wide, 16:9.
    let nsp = qu_xlsx::surgical::sheet_part(&pkg, "Notes").unwrap();
    let nd = pkg.rels(&nsp).unwrap().into_iter().find(|r| r.rel_type.ends_with("/drawing")).unwrap();
    let nx = part(&pkg, &qu_ooxml::resolve_target(&nsp, &nd.target));
    let e = nx.find_all("a:ext")[0];
    assert_eq!((e.attr("cx"), e.attr("cy")), (Some("5760000"), Some("3240000")));
    // The chart that was in the file is untouched, and survives a later cell edit with the pictures.
    assert_eq!(charts_of(&pkg, "Data").len(), 1);
    let mut again = Workbook::open(&out).unwrap();
    again.set("Data", "A2", &s("edited")).unwrap();
    let out2 = tmp("im_out2.xlsx");
    again.save(&out2).unwrap();
    let pkg2 = Package::open(&out2).unwrap();
    assert_eq!(charts_of(&pkg2, "Data").len(), 1, "chart after the cell edit");
    assert_eq!(pkg2.names().iter().filter(|p| p.starts_with("xl/media/")).count(), 3, "pictures after the cell edit: {:?}", pkg2.names());
}

/// A chart saved earlier is still there, wired to its sheet and parseable,
/// after every operation of this lane.
#[test]
fn a_chart_saved_earlier_survives_every_operation() {
    let src = fixture("keep_src.xlsx");
    let original = {
        let pkg = Package::open(&src).unwrap();
        let ch = charts_of(&pkg, "Data");
        assert_eq!(ch.len(), 1);
        pkg.get_str(&ch[0]).unwrap()
    };
    let check = |label: &str, wb: &Workbook, sheet: &str| {
        let out = tmp(&format!("keep_{label}.xlsx"));
        wb.save(&out).unwrap();
        let pkg = Package::open(&out).unwrap();
        let ch = charts_of(&pkg, sheet);
        assert_eq!(ch.len(), 1, "{label}: chart part for {sheet} (parts: {:?})", pkg.names());
        let c = part(&pkg, &ch[0]);
        assert_eq!(c.find_all("c:barChart").len(), 1, "{label}");
        assert!(c.find_all("c:f").iter().any(|f| f.text() == "Data!$B$2:$B$6"), "{label}: series reference");
        // And a plain reopen still works.
        Workbook::open(&out).unwrap_or_else(|e| panic!("{label}: {e}"));
        pkg.get_str(&ch[0]).unwrap()
    };
    let ops: Vec<(&str, Box<dyn Fn(&mut Workbook)>)> = vec![
        ("hide", Box::new(|w| w.set_sheet_visibility("Notes", SheetVisibility::Hidden).unwrap())),
        ("clear", Box::new(|w| {
            w.clear("Data", "C1:C6", ClearWhat::All).unwrap();
        })),
        ("move", Box::new(|w| {
            w.move_range("Data", "C1:C6", "H1", None, false).unwrap();
        })),
        ("sort", Box::new(|w| w.sort_range("Data", "A1:B6", &[(SortKey::Letter("B".into()), false)], true).unwrap())),
        ("filter", Box::new(|w| w.autofilter("Data", Some("A1:C6")).unwrap())),
        ("table", Box::new(|w| w.create_table("Data", "A1:B6", "KeepTable", true, None, true, Some(TotalFn::Sum)).unwrap())),
        ("comment", Box::new(|w| w.add_comment("Data", "B2", "note", None).unwrap())),
        ("image", Box::new(|w| w.add_image("Data", png(20, 20), Some("H2"), None, None, None).unwrap())),
    ];
    for (label, op) in &ops {
        let mut wb = Workbook::open(&src).unwrap();
        op(&mut wb);
        check(label, &wb, "Data");
    }
    // All of them at once, in one save.
    let mut wb = Workbook::open(&src).unwrap();
    wb.set_sheet_visibility("Notes", SheetVisibility::Hidden).unwrap();
    wb.sort_range("Data", "A1:B6", &[(SortKey::Letter("B".into()), false)], true).unwrap();
    wb.add_comment("Data", "B2", "note", None).unwrap();
    wb.add_image("Data", png(20, 20), Some("H2"), None, None, None).unwrap();
    wb.create_table("Data", "A1:B6", "KeepTable", true, None, true, None).unwrap();
    wb.copy_sheet("Data", "Copy").unwrap();
    let after = check("all", &wb, "Data");
    // The chart XML itself is the one that was written before (the series
    // refer to the same cells; only the cached values may differ after a sort).
    assert_eq!(original.matches("<c:ser>").count(), after.matches("<c:ser>").count());
    // The copy has a chart too.
    let out = tmp("keep_all.xlsx");
    let pkg = Package::open(&out).unwrap();
    assert_eq!(charts_of(&pkg, "Copy").len(), 1);
    // Copy, then edit a cell on the original: both charts remain.
    let mut wb = Workbook::open(&src).unwrap();
    wb.copy_sheet("Data", "Dup").unwrap();
    let mid = tmp("keep_dup.xlsx");
    wb.save(&mid).unwrap();
    let mut wb = Workbook::open(&mid).unwrap();
    wb.set("Dup", "A2", &s("changed")).unwrap();
    let fin = tmp("keep_dup2.xlsx");
    wb.save(&fin).unwrap();
    let pkg = Package::open(&fin).unwrap();
    assert_eq!((charts_of(&pkg, "Data").len(), charts_of(&pkg, "Dup").len()), (1, 1));
}

#[test]
fn renaming_or_deleting_a_sheet_carries_its_tables_notes_and_pictures() {
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "Old").unwrap();
    wb.set_range("Old", "A1", &[vec![s("k"), s("v")], vec![s("a"), n(1.0)], vec![s("b"), n(2.0)]]).unwrap();
    wb.create_table("Old", "A1:B3", "Tab", true, None, true, Some(TotalFn::Sum)).unwrap();
    wb.add_comment("Old", "A2", "note", None).unwrap();
    wb.add_image("Old", png(16, 16), Some("D1"), None, None, None).unwrap();
    wb.add_sheet("Gone").unwrap();
    wb.add_comment("Gone", "A1", "dies with its sheet", None).unwrap();
    wb.add_image("Gone", png(16, 16), None, None, None, None).unwrap();
    wb.rename_sheet("Old", "New sheet").unwrap();
    wb.delete_sheet("Gone").unwrap();
    let out = tmp("rename_pending.xlsx");
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let t = part(&pkg, "xl/tables/table1.xml");
    assert_eq!(t.attr("ref"), Some("A1:B4"));
    assert_eq!(pkg.names().iter().filter(|p| p.starts_with("xl/comments")).count(), 1, "only the surviving sheet's note: {:?}", pkg.names());
    assert_eq!(pkg.names().iter().filter(|p| p.starts_with("xl/media/")).count(), 1);
    let sp = qu_xlsx::surgical::sheet_part(&pkg, "New sheet").unwrap();
    let ws = part(&pkg, &sp);
    assert_eq!((ws.find_all("tablePart").len(), ws.find_all("legacyDrawing").len(), ws.find_all("drawing").len()), (1, 1, 1));
    assert_eq!(Workbook::open(&out).unwrap().get("New sheet", "B4").unwrap(), CellValue::Empty, "the SUBTOTAL has no cached value");
    assert_eq!(Workbook::open(&out).unwrap().formula("New sheet", "B4").unwrap().as_deref(), Some("SUBTOTAL(109,B2:B3)"));
}

#[test]
fn references_in_ranges_use_the_sheet_they_name() {
    // `parse_ref` is what table and chart ranges go through.
    assert_eq!(parse_ref("'My data'!A1:B2", "S").unwrap().sheet, "My data");
}

// ------------------------------------------------------------------ external tools

fn python_with_openpyxl() -> Option<String> {
    for exe in ["python", "python3", "py"] {
        let ok = std::process::Command::new(exe).args(["-c", "import openpyxl"]).output().map(|o| o.status.success()).unwrap_or(false);
        if ok {
            return Some(exe.to_string());
        }
    }
    None
}

fn openpyxl(exe: &str, code: &str, path: &str) -> String {
    let out = std::process::Command::new(exe).args(["-c", code, path]).output().expect("python runs");
    assert!(out.status.success(), "openpyxl script failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n").trim().to_string()
}

/// One workbook with every feature of the lane; openpyxl reads it, then
/// LibreOffice opens it, renders it, and writes it back as .xlsx, which
/// openpyxl reads again -- so the table, the note and the picture were each
/// understood by a program that is not ours. Skips when either is missing.
#[test]
fn openpyxl_and_libreoffice_read_what_was_written() {
    let Some(py) = python_with_openpyxl() else {
        eprintln!("skipped: no python with openpyxl on this machine");
        return;
    };
    let path = tmp("ext.xlsx");
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "Data").unwrap();
    wb.set_range(
        "Data",
        "A1",
        &[vec![s("name"), s("value")], vec![s("delta"), n(3.0)], vec![s("alpha"), n(10.0)], vec![s("charlie"), n(1.0)], vec![s("echo"), n(7.0)]],
    )
    .unwrap();
    wb.sort_range("Data", "A1:B5", &[(SortKey::Letter("B".into()), true)], true).unwrap();
    wb.create_table("Data", "A1:B5", "Readings", true, Some("medium9"), true, Some(TotalFn::Sum)).unwrap();
    wb.add_comment("Data", "B2", "largest reading", Some("Ahmed")).unwrap();
    wb.add_image("Data", png(120, 60), Some("D2"), None, None, Some("gradient")).unwrap();
    let mut o = ChartOptions::new(ChartKind::Column);
    o.y = vec!["B2:B5".into()];
    o.x = vec!["A2:A5".into()];
    o.at = Some("D8".into());
    wb.add_chart("Data", &o).unwrap();
    wb.add_sheet("Hidden").unwrap();
    wb.set("Hidden", "A1", &s("secret")).unwrap();
    wb.set_sheet_visibility("Hidden", SheetVisibility::Hidden).unwrap();
    wb.copy_sheet("Data", "Data2").unwrap();
    wb.move_range("Data2", "A1:B1", "A20", None, false).unwrap();
    wb.clear("Data2", "A3:B3", ClearWhat::Contents).unwrap();
    wb.autofilter("Data2", Some("A20:B24")).unwrap();
    wb.save(&path).unwrap();

    let script = r#"
import sys, openpyxl
wb = openpyxl.load_workbook(sys.argv[1])
print("sheets", [(w.title, w.sheet_state) for w in wb.worksheets])
ws = wb["Data"]
print("rows", [[c.value for c in r] for r in ws["A1:B5"]])
print("tables", {k: ws.tables[k].ref for k in ws.tables})
if "Readings" in ws.tables:
    t = ws.tables["Readings"]
    print("style", t.tableStyleInfo.name if t.tableStyleInfo else None, "totals", t.totalsRowCount)
c = ws["B2"].comment
print("comment", None if c is None else (c.text, c.author))
print("images", len(ws._images), "charts", len(ws._charts))
w2 = wb["Data2"]
print("copy", w2["A20"].value, w2["A3"].value, w2.auto_filter.ref)
"#;
    let got = openpyxl(&py, script, &path);
    eprintln!("openpyxl on our file:\n{got}");
    assert!(got.contains("sheets [('Data', 'visible'), ('Hidden', 'hidden'), ('Data2', 'visible')]"), "{got}");
    assert!(got.contains("rows [['name', 'value'], ['alpha', 10], ['echo', 7], ['delta', 3], ['charlie', 1]]"), "{got}");
    assert!(got.contains("tables {'Readings': 'A1:B6'}"), "{got}");
    assert!(got.contains("style TableStyleMedium9 totals 1"), "{got}");
    assert!(got.contains("comment ('largest reading', 'Ahmed')"), "{got}");
    assert!(got.contains("images 1"), "{got}");
    assert!(got.contains("copy name None A20:B24"), "{got}");

    // LibreOffice: opens it, lays it out (PDF), and writes it back as xlsx.
    let Some(_) = qu_ooxml::find_office() else {
        eprintln!("skipped LibreOffice half: no soffice on this machine");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let pdf = qu_ooxml::convert_with_office(&bytes, "xlsx", "pdf", 180).expect("LibreOffice renders the workbook");
    assert!(pdf.starts_with(b"%PDF"), "not a PDF");
    // The first sheet as CSV: the sort and the table's totals row, as LibreOffice sees them.
    let csv = String::from_utf8_lossy(&qu_ooxml::convert_with_office(&bytes, "xlsx", "csv", 180).expect("LibreOffice exports csv")).replace("\r\n", "\n");
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(&lines[..5], ["name,value", "alpha,10", "echo,7", "delta,3", "charlie,1"], "{csv}");
    assert!(lines[5].starts_with("Total,21"), "LibreOffice calculated the SUBTOTAL: {:?}", lines.get(5));
    let back = qu_ooxml::convert_with_office(&bytes, "xlsx", "xlsx", 180).expect("LibreOffice writes xlsx");
    let rt = tmp("ext_via_lo.xlsx");
    std::fs::write(&rt, back).unwrap();
    let got2 = openpyxl(
        &py,
        r#"
import sys, openpyxl
wb = openpyxl.load_workbook(sys.argv[1])
ws = wb["Data"]
print("sheets", [(w.title, w.sheet_state) for w in wb.worksheets])
print("tables", sorted(ws.tables.keys()))
c = ws["B2"].comment
print("comment", None if c is None else c.text.strip().replace("\n", " "))
print("images", len(ws._images))
"#,
        &rt,
    );
    eprintln!("openpyxl on LibreOffice's copy:\n{got2}");
    assert!(got2.contains("('Hidden', 'hidden')"), "{got2}");
    assert!(got2.contains("images 1"), "LibreOffice kept the picture: {got2}");
    assert!(got2.contains("largest reading"), "LibreOffice kept the note: {got2}");
    assert!(got2.contains("tables ['Readings']"), "LibreOffice kept the table: {got2}");
}
