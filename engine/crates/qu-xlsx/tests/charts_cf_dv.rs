//! Charts, conditional formatting and data validation: known answers read
//! back out of the saved package, and the surgical-save guarantee (parts
//! these edits do not touch come back byte for byte).

use qu_ooxml::xml::Element;
use qu_ooxml::Package;
use qu_xlsx::surgical::{CfRule, CfStyle, ValKind};
use qu_xlsx::workbook::{cf_value_num, cf_value_text, chart_kind, CellValue, ChartKind, ChartOptions, Workbook};

fn tmp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("qu_xlsx_charts_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().into_owned()
}

const CUSTOM_XML: &[u8] = b"<?xml version=\"1.0\"?>\r\n<!-- odd   spacing kept --><root  a='1'>&#38;</root>";

/// A workbook with data on `Data`, plus an unknown customXml part that no
/// Qu function understands, written to `path`.
fn fixture(path: &str) {
    let mut wb = Workbook::new();
    wb.rename_sheet("Sheet1", "Data").unwrap();
    let mut rows = vec![vec![CellValue::Str("f".into()), CellValue::Str("Zre".into()), CellValue::Str("Zim".into()), CellValue::Str("label".into())]];
    // A semicircle: R0 = 10, R1 = 100 -> Z' in [10, 110], -Z'' in [0, 50].
    for i in 0..=10 {
        let th = i as f64 * std::f64::consts::PI / 10.0;
        rows.push(vec![CellValue::Num(10f64.powi(i)), CellValue::Num(60.0 - 50.0 * th.cos()), CellValue::Num(-50.0 * th.sin()), CellValue::Str(format!("p{i}"))]);
    }
    wb.set_range("Data", "A1", &rows).unwrap();
    wb.add_sheet("Other").unwrap();
    let mut pkg = Package::from_bytes(&wb.to_bytes().unwrap()).unwrap();
    pkg.set("customXml/item1.xml", CUSTOM_XML.to_vec());
    pkg.add_rel("xl/workbook.xml", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml", "../customXml/item1.xml").unwrap();
    pkg.save(path).unwrap();
}

fn part(pkg: &Package, name: &str) -> Element {
    pkg.get_xml(name).unwrap_or_else(|e| panic!("{e}; parts: {:?}", pkg.names())).root
}

fn val<'a>(e: &'a Element, name: &str) -> &'a str {
    e.find_all(name).first().and_then(|x| x.attr("val")).unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn untouched_parts_survive_byte_for_byte() {
    let (src, out) = (tmp("src.xlsx"), tmp("out.xlsx"));
    fixture(&src);
    let before = Package::open(&src).unwrap();
    let mut wb = Workbook::open(&src).unwrap();
    let mut o = ChartOptions::new(ChartKind::Scatter);
    o.x = vec!["B2:B12".into()];
    o.y = vec!["C2:C12".into()];
    wb.add_chart("Data", &o).unwrap();
    wb.conditional_format("Data", "B2:B12", CfRule::Cell { op: "greaterThan", a: cf_value_num(100.0), b: None }, CfStyle { fill: Some("#FFC7CE".into()), ..Default::default() }).unwrap();
    wb.add_validation("Data", "D2:D12", ValKind::List(vec!["a".into(), "b".into()]), None, None, None, None, None, true).unwrap();
    wb.save(&out).unwrap();
    let after = Package::open(&out).unwrap();
    let mut changed: Vec<String> = before.names().into_iter().filter(|n| before.get(n) != after.get(n)).collect();
    changed.sort();
    assert_eq!(changed, ["[Content_Types].xml", "xl/styles.xml", "xl/worksheets/sheet1.xml"], "only what the edits need is rewritten");
    let mut added: Vec<String> = after.names().into_iter().filter(|n| !before.has(n)).collect();
    added.sort();
    assert_eq!(added, ["xl/charts/chart1.xml", "xl/drawings/_rels/drawing1.xml.rels", "xl/drawings/drawing1.xml", "xl/worksheets/_rels/sheet1.xml.rels"]);
    assert_eq!(after.get("customXml/item1.xml").unwrap(), CUSTOM_XML);
    assert_eq!(after.get("xl/theme/theme1.xml"), before.get("xl/theme/theme1.xml"));
    // Opened and saved with no edit at all: the file itself, unchanged.
    let same = tmp("same.xlsx");
    Workbook::open(&src).unwrap().save(&same).unwrap();
    assert_eq!(std::fs::read(&same).unwrap(), std::fs::read(&src).unwrap());
}

#[test]
fn scatter_line_and_bar_charts_known_answers() {
    let (src, out) = (tmp("c_src.xlsx"), tmp("c_out.xlsx"));
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    let mut o = ChartOptions::new(chart_kind("scatter").unwrap());
    o.x = vec!["A2:A12".into()];
    o.y = vec!["B2:B12".into(), "Data!C2:C12".into()];
    o.names = vec!["Z'".into(), "Z''".into()];
    o.title = Some("Bode & more".into());
    o.x_title = Some("f / Hz".into());
    o.y_title = Some("Z / Ω".into());
    o.x_log = true;
    o.at = Some("F3".into());
    o.width_mm = Some(120.0);
    o.height_mm = Some(80.0);
    wb.add_chart("Data", &o).unwrap();
    let mut l = ChartOptions::new(chart_kind("line").unwrap());
    l.x = vec!["D2:D12".into()];
    l.y = vec!["B2:B12".into()];
    wb.add_chart("Other", &{
        let mut l = l.clone();
        l.y = vec!["Data!B2:B12".into()];
        l.x = vec!["Data!D2:D12".into()];
        l
    })
    .unwrap();
    let mut b = ChartOptions::new(chart_kind("barh").unwrap());
    b.y = vec!["B2:B4".into()];
    b.colors = vec!["#112233".into()];
    wb.add_chart("Data", &b).unwrap();
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();

    // Chart 1: scatter, two series, log x, titles, anchored at F3, 120 x 80 mm.
    let c = part(&pkg, "xl/charts/chart1.xml");
    assert_eq!(c.find_all("c:scatterChart").len(), 1);
    let sers = c.find_all("c:ser");
    assert_eq!(sers.len(), 2);
    let f: Vec<String> = c.find_all("c:f").iter().map(|e| e.text()).collect();
    assert_eq!(f, ["Data!$A$2:$A$12", "Data!$B$2:$B$12", "Data!$A$2:$A$12", "Data!$C$2:$C$12"]);
    assert_eq!(sers[0].find_all("c:tx")[0].text(), "Z'");
    let ycache: Vec<f64> = sers[0].child("c:yVal").unwrap().find_all("c:v").iter().map(|v| v.text().parse().unwrap()).collect();
    assert_eq!(ycache.len(), 11);
    assert!((ycache[0] - 10.0).abs() < 1e-12 && (ycache[10] - 110.0).abs() < 1e-12);
    let xcache: Vec<String> = sers[0].child("c:xVal").unwrap().find_all("c:v").iter().map(|v| v.text()).collect();
    assert_eq!(xcache[10], "10000000000");
    let texts: Vec<String> = c.find_all("a:t").iter().map(|t| t.text()).collect();
    assert_eq!(texts, ["Bode & more", "f / Hz", "Z / Ω"]);
    assert_eq!(val(c.find_all("c:valAx")[0], "c:logBase"), "10");
    assert_eq!(c.find_all("c:legend").len(), 1, "named series get a legend");
    // Sheet 1's drawing holds charts 1 and 3; `Other` got its own drawing.
    let d = part(&pkg, "xl/drawings/drawing1.xml");
    let anchors = d.find_all("xdr:twoCellAnchor");
    assert_eq!(anchors.len(), 2);
    let m = |tag: &str| -> Vec<String> { ["xdr:col", "xdr:colOff", "xdr:row", "xdr:rowOff"].iter().map(|n| anchors[0].child(tag).unwrap().child(n).unwrap().text()).collect() };
    assert_eq!(m("xdr:from"), ["5", "0", "2", "0"]);
    // 120 mm = 4 320 000 EMU over default 64 px (609 600 EMU) columns: 7
    // whole columns + 52 800; 80 mm = 2 880 000 EMU over the 13.5 pt
    // (171 450 EMU) default rows this file declares (sheetFormatPr): 16
    // whole rows + 136 800.
    assert_eq!(m("xdr:to"), ["12", "52800", "18", "136800"]);
    let ids: Vec<&str> = d.find_all("xdr:cNvPr").iter().map(|e| e.attr("id").unwrap()).collect();
    assert_eq!(ids, ["2", "3"], "shape ids are unique in the drawing");
    let rels = pkg.rels("xl/drawings/drawing1.xml").unwrap();
    assert_eq!(rels.iter().map(|r| r.target.as_str()).collect::<Vec<_>>(), ["../charts/chart1.xml", "../charts/chart3.xml"]);

    // Chart 2: a line chart on `Other` whose data live on `Data`; categories are text.
    let c2 = part(&pkg, "xl/charts/chart2.xml");
    assert_eq!(c2.find_all("c:lineChart").len(), 1);
    assert_eq!(c2.find_all("c:catAx").len(), 1);
    let cats: Vec<String> = c2.find_all("c:strCache")[0].find_all("c:v").iter().map(|v| v.text()).collect();
    assert_eq!(cats[..3], ["p0", "p1", "p2"]);
    assert_eq!(c2.find_all("c:legend").len(), 0, "one unnamed series: no legend");
    let other = qu_xlsx::surgical::sheet_part(&pkg, "Other").unwrap();
    assert_eq!(part(&pkg, &other).find_all("drawing").len(), 1);

    // Chart 3: horizontal bars in the requested colour.
    let c3 = part(&pkg, "xl/charts/chart3.xml");
    assert_eq!(val(&c3, "c:barDir"), "bar");
    assert_eq!(val(c3.find_all("c:ser")[0], "a:srgbClr"), "112233");
    let ct = String::from_utf8(pkg.get("[Content_Types].xml").unwrap().to_vec()).unwrap();
    for p in ["/xl/charts/chart1.xml", "/xl/charts/chart3.xml", "/xl/drawings/drawing2.xml"] {
        assert!(ct.contains(p), "{p} has a content type");
    }
    // The cells themselves are what they were.
    assert_eq!(Workbook::open(&out).unwrap().get("Data", "D3").unwrap(), CellValue::Str("p1".into()));
}

#[test]
fn nyquist_chart_has_equal_axis_scaling() {
    let (src, out) = (tmp("n_src.xlsx"), tmp("n_out.xlsx"));
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    let mut o = ChartOptions::new(ChartKind::Scatter);
    o.title = Some("Nyquist".into());
    let helper = wb.add_nyquist_chart("Data", "B2:B12", "C2:C12", true, None, o).unwrap();
    assert_eq!(helper.as_deref(), Some("E2:E12"), "first free column right of A:D");
    assert_eq!(wb.formula("Data", "E7").unwrap().as_deref(), Some("-C7"));
    assert_eq!(wb.get("Data", "E1").unwrap(), CellValue::Str("-Zim".into()));
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let c = part(&pkg, "xl/charts/chart1.xml");
    assert_eq!(c.find_all("c:f")[1].text(), "Data!$E$2:$E$12");
    // The cache is -Z'' although the helper cells are uncalculated formulas.
    let y: Vec<f64> = c.find_all("c:yVal")[0].find_all("c:v").iter().map(|v| v.text().parse().unwrap()).collect();
    assert!((y[5] - 50.0).abs() < 1e-9 && y.iter().all(|v| *v >= -1e-9));
    let ax = c.find_all("c:valAx");
    let lim = |a: &Element, n: &str| -> f64 { val(a, n).parse().unwrap() };
    // Z' in [10, 110], -Z'' in [0, 50] -> a shared step of 20: x 0..120, y 0..60.
    assert_eq!((lim(ax[0], "c:min"), lim(ax[0], "c:max"), lim(ax[1], "c:min"), lim(ax[1], "c:max")), (0.0, 120.0, 0.0, 60.0));
    assert_eq!((lim(ax[0], "c:majorUnit"), lim(ax[1], "c:majorUnit")), (20.0, 20.0));
    // Same ohms per millimetre along both axes, from the manual plot area
    // and the anchor size (150 x 110 mm by default).
    let ml = c.find_all("c:manualLayout")[0];
    let w = val(ml, "c:w").parse::<f64>().unwrap() * 150.0;
    let h = val(ml, "c:h").parse::<f64>().unwrap() * 110.0;
    assert!((120.0 / w - 60.0 / h).abs() < 1e-9, "x: {} ohm/mm, y: {} ohm/mm", 120.0 / w, 60.0 / h);
    assert_eq!(c.find_all("a:t").iter().map(|t| t.text()).collect::<Vec<_>>(), ["Nyquist", "Z' / Ω", "-Z'' / Ω"]);

    // negate=false plots the column as given and changes no cell.
    let mut wb = Workbook::open(&src).unwrap();
    assert_eq!(wb.add_nyquist_chart("Data", "B2:B12", "C2:C12", false, None, ChartOptions::new(ChartKind::Scatter)).unwrap(), None);
    let bytes = wb.to_bytes().unwrap();
    let pkg = Package::from_bytes(&bytes).unwrap();
    assert_eq!(pkg.get("xl/sharedStrings.xml"), Package::open(&src).unwrap().get("xl/sharedStrings.xml"));
    // A helper column on top of the data is refused, and leaves no chart.
    let mut wb = Workbook::open(&src).unwrap();
    assert!(wb.add_nyquist_chart("Data", "B2:B12", "C2:C12", true, Some("C"), ChartOptions::new(ChartKind::Scatter)).unwrap_err().contains("overwrite"));
    assert_eq!(wb.pending_count(), 0);
}

#[test]
fn conditional_formats_known_answers() {
    let (src, out) = (tmp("cf_src.xlsx"), tmp("cf_out.xlsx"));
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    let hi = CfStyle { fill: Some("#ffc7ce".into()), color: Some("#9C0006".into()), bold: true };
    wb.conditional_format("Data", "B2:B12", CfRule::Cell { op: "greaterThan", a: cf_value_num(100.0), b: None }, hi.clone()).unwrap();
    wb.conditional_format("Data", "C2:C12", CfRule::Cell { op: "between", a: cf_value_num(-20.0), b: Some(cf_value_text("=$B$2")) }, CfStyle { color: Some("#0000FF".into()), ..Default::default() }).unwrap();
    wb.conditional_format("Data", "D2:D12", CfRule::Cell { op: "equal", a: cf_value_text("p\"3"), b: None }, CfStyle { bold: true, ..Default::default() }).unwrap();
    wb.conditional_format("Data", "D2:D12", CfRule::Contains("p1".into()), hi.clone()).unwrap();
    wb.conditional_format("Data", "A2:A12", CfRule::Scale(vec!["#F8696B".into(), "#FFEB84".into(), "#63BE7B".into()]), CfStyle::default()).unwrap();
    assert!(wb.conditional_format("Data", "B2", CfRule::Cell { op: "between", a: cf_value_num(1.0), b: None }, hi.clone()).unwrap_err().contains("two values"));
    assert!(wb.conditional_format("Data", "B2", CfRule::Cell { op: "equal", a: cf_value_num(1.0), b: None }, CfStyle::default()).unwrap_err().contains("fill="));
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let ws = part(&pkg, "xl/worksheets/sheet1.xml");
    let cfs = ws.find_all("conditionalFormatting");
    let sq: Vec<&str> = cfs.iter().map(|c| c.attr("sqref").unwrap()).collect();
    assert_eq!(sq, ["B2:B12", "C2:C12", "D2:D12", "D2:D12", "A2:A12"]);
    let rules = ws.find_all("cfRule");
    let pr: Vec<&str> = rules.iter().map(|r| r.attr("priority").unwrap()).collect();
    assert_eq!(pr, ["1", "2", "3", "4", "5"]);
    let formulas: Vec<String> = ws.find_all("formula").iter().map(|f| f.text()).collect();
    assert_eq!(formulas, ["100", "-20", "$B$2", "\"p\"\"3\"", "NOT(ISERROR(SEARCH(\"p1\",D2)))"]);
    assert_eq!((rules[3].attr("type"), rules[3].attr("text")), (Some("containsText"), Some("p1")));
    assert_eq!(rules[4].find_all("color").iter().map(|c| c.attr("rgb").unwrap()).collect::<Vec<_>>(), ["FFF8696B", "FFFFEB84", "FF63BE7B"]);
    assert_eq!(rules[4].attr("dxfId"), None, "a colour scale has no highlight style");
    // Each highlight rule points at the dxf holding its own style.
    let st = part(&pkg, "xl/styles.xml");
    let dxfs = st.find_all("dxf");
    let dxf = |r: &Element| dxfs[r.attr("dxfId").unwrap().parse::<usize>().unwrap()];
    assert_eq!(dxf(rules[0]).find_all("bgColor")[0].attr("rgb"), Some("FFFFC7CE"));
    assert_eq!(dxf(rules[0]).find_all("color")[0].attr("rgb"), Some("FF9C0006"));
    assert_eq!(dxf(rules[0]).find_all("b").len(), 1);
    assert_eq!(dxf(rules[1]).find_all("color")[0].attr("rgb"), Some("FF0000FF"));
    assert!(dxf(rules[1]).find_all("fill").is_empty());
    assert_eq!(st.find_all("dxfs")[0].attr("count").unwrap().parse::<usize>().unwrap(), dxfs.len());
}

#[test]
fn data_validation_known_answers() {
    let (src, out) = (tmp("dv_src.xlsx"), tmp("dv_out.xlsx"));
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    wb.add_validation("Data", "E2:E20", ValKind::List(vec!["ok".into(), "bad".into(), "3".into()]), Some("Pick one".into()), Some("Status".into()), None, None, None, true).unwrap();
    let from = qu_xlsx::surgical::parse_ref("D2:D12", "Data").unwrap();
    wb.add_validation("Data", "F2:F20", ValKind::ListFrom(from), None, None, None, None, None, false).unwrap();
    let other = qu_xlsx::surgical::parse_ref("Data!D2:D12", "Other").unwrap();
    wb.add_validation("Other", "A1", ValKind::ListFrom(other), None, None, None, None, None, true).unwrap();
    wb.add_validation("Data", "G2:G20", ValKind::Number("whole", Some(0.0), Some(100.0)), None, None, Some("0 to 100".into()), Some("Out of range".into()), Some("warning".into()), true).unwrap();
    wb.add_validation("Data", "H2:H20", ValKind::Number("decimal", Some(0.5), None), None, None, None, None, None, true).unwrap();
    wb.add_validation("Data", "I2:I20", ValKind::Custom("=ISNUMBER(I2)".into()), None, None, None, None, None, true).unwrap();
    assert!(wb.add_validation("Data", "E2", ValKind::List(vec!["a,b".into()]), None, None, None, None, None, true).unwrap_err().contains("commas"));
    assert!(wb.add_validation("Data", "E2", ValKind::Number("whole", Some(5.0), Some(1.0)), None, None, None, None, None, true).unwrap_err().contains("above"));
    assert!(wb.add_validation("Data", "E2", ValKind::Number("whole", Some(1.0), None), None, None, None, None, Some("loud".into()), true).unwrap_err().contains("stop"));
    wb.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let ws = part(&pkg, "xl/worksheets/sheet1.xml");
    let dvs = ws.find_all("dataValidations");
    assert_eq!(dvs.len(), 1);
    assert_eq!(dvs[0].attr("count"), Some("5"));
    let v = dvs[0].find_all("dataValidation");
    let t: Vec<(&str, Option<&str>, &str)> = v.iter().map(|d| (d.attr("type").unwrap(), d.attr("operator"), d.attr("sqref").unwrap())).collect();
    assert_eq!(t, [("list", None, "E2:E20"), ("list", None, "F2:F20"), ("whole", Some("between"), "G2:G20"), ("decimal", Some("greaterThanOrEqual"), "H2:H20"), ("custom", None, "I2:I20")]);
    let f1: Vec<String> = v.iter().map(|d| d.child("formula1").unwrap().text()).collect();
    assert_eq!(f1, ["\"ok,bad,3\"", "$D$2:$D$12", "0", "0.5", "ISNUMBER(I2)"]);
    assert_eq!(v[2].child("formula2").unwrap().text(), "100");
    assert_eq!((v[0].attr("prompt"), v[0].attr("promptTitle"), v[0].attr("allowBlank")), (Some("Pick one"), Some("Status"), Some("1")));
    assert_eq!((v[2].attr("error"), v[2].attr("errorTitle"), v[2].attr("errorStyle")), (Some("0 to 100"), Some("Out of range"), Some("warning")));
    assert_eq!(v[1].attr("allowBlank"), None);
    let other = qu_xlsx::surgical::sheet_part(&pkg, "Other").unwrap();
    assert_eq!(part(&pkg, &other).find_all("formula1")[0].text(), "Data!$D$2:$D$12", "a list from another sheet names it");
    // Schema order: dataValidations after conditional formats / before pageMargins.
    let order: Vec<&str> = ws.elems().map(|e| e.name.as_str()).collect();
    let pos = |n: &str| order.iter().position(|x| *x == n);
    assert!(pos("dataValidations") > pos("sheetData"));
    if let Some(pm) = pos("pageMargins") {
        assert!(pos("dataValidations").unwrap() < pm);
    }
}

#[test]
fn edits_follow_a_renamed_sheet_and_survive_a_cell_edit_later() {
    let (src, mid, out) = (tmp("r_src.xlsx"), tmp("r_mid.xlsx"), tmp("r_out.xlsx"));
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    let mut o = ChartOptions::new(ChartKind::Scatter);
    o.x = vec!["B2:B12".into()];
    o.y = vec!["C2:C12".into()];
    wb.add_chart("Data", &o).unwrap();
    wb.conditional_format("Data", "B2:B12", CfRule::Cell { op: "lessThan", a: cf_value_num(0.0), b: None }, CfStyle { bold: true, ..Default::default() }).unwrap();
    wb.rename_sheet("Data", "Measured data").unwrap();
    wb.save(&mid).unwrap();
    let pkg = Package::open(&mid).unwrap();
    let f: Vec<String> = part(&pkg, "xl/charts/chart1.xml").find_all("c:f").iter().map(|e| e.text()).collect();
    assert_eq!(f, ["'Measured data'!$B$2:$B$12", "'Measured data'!$C$2:$C$12"]);
    // Reopen and edit a cell: the model-based save must keep the chart,
    // the rule and the drawing it did not create.
    let mut again = Workbook::open(&mid).unwrap();
    again.set("Measured data", "B2", &CellValue::Num(11.0)).unwrap();
    again.save(&out).unwrap();
    let pkg = Package::open(&out).unwrap();
    let charts: Vec<String> = pkg.names().into_iter().filter(|n| n.starts_with("xl/charts/chart")).collect();
    assert_eq!(charts.len(), 1, "parts: {:?}", pkg.names());
    let c = part(&pkg, &charts[0]);
    assert_eq!(c.find_all("c:scatterChart").len(), 1);
    let f2: Vec<String> = c.find_all("c:f").iter().map(|e| e.text()).collect();
    assert_eq!(f2, f);
    let ws = part(&pkg, &qu_xlsx::surgical::sheet_part(&pkg, "Measured data").unwrap());
    assert_eq!(ws.find_all("conditionalFormatting").len(), 1);
    assert_eq!(Workbook::open(&out).unwrap().get("Measured data", "B2").unwrap(), CellValue::Num(11.0));
    // Deleting the sheet drops its pending edits instead of failing the save.
    let mut wb = Workbook::open(&src).unwrap();
    wb.add_chart("Other", &{
        let mut o = o.clone();
        o.x = vec!["Data!B2:B12".into()];
        o.y = vec!["Data!C2:C12".into()];
        o
    })
    .unwrap();
    wb.delete_sheet("Other").unwrap();
    assert_eq!(wb.pending_count(), 0);
    wb.save(&tmp("r_del.xlsx")).unwrap();
}

#[test]
fn chart_arguments_are_checked() {
    let src = tmp("a_src.xlsx");
    fixture(&src);
    let mut wb = Workbook::open(&src).unwrap();
    let mut o = ChartOptions::new(ChartKind::Scatter);
    o.x = vec!["A2:A12".into()];
    o.y = vec!["B2:C12".into()];
    assert!(wb.add_chart("Data", &o).unwrap_err().contains("block"));
    o.y = vec!["B2:B11".into()];
    assert!(wb.add_chart("Data", &o).unwrap_err().contains("has 11 cells"));
    o.y = vec!["Nope!B2:B12".into()];
    assert!(wb.add_chart("Data", &o).unwrap_err().contains("Nope"));
    let mut l = ChartOptions::new(ChartKind::Line);
    l.y = vec!["B2:B12".into()];
    l.x_log = true;
    assert!(wb.add_chart("Data", &l).unwrap_err().contains("scatter"));
    let mut s = ChartOptions::new(ChartKind::Scatter);
    s.y = vec!["B2:B12".into()];
    s.y_min = Some(5.0);
    s.y_max = Some(1.0);
    assert!(wb.add_chart("Data", &s).unwrap_err().contains("below max"));
    assert!(chart_kind("pie").unwrap_err().contains("scatter, line, bar"));
    assert_eq!(wb.pending_count(), 0, "refused calls leave nothing behind");
}
