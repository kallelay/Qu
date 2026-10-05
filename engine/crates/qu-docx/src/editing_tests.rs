//! Tests for `editing.rs`: each operation on a bare package (no styles,
//! settings or footnotes parts) so every create-from-nothing path runs, and
//! on parts that no edit may touch.

use super::*;

const KEEP_XML: &[u8] = b"<vendor><keep me=\"1\"/></vendor>";
const KEEP_THEME: &[u8] = b"<a:theme xmlns:a=\"urn:a\" name=\"Keep\"><a:x/></a:theme>";

fn cell(w: u32, text: &str, bold: bool) -> String {
    let rpr = if bold { "<w:rPr><w:b/></w:rPr>" } else { "" };
    format!("<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/></w:tcPr><w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r>{rpr}<w:t>{text}</w:t></w:r></w:p></w:tc>")
}

/// Four paragraphs (one with a hyperlink, one bold-then-plain), a 3x3 table
/// whose first row is a bold header, and two parts that no edit may touch.
fn fixture() -> Document {
    let mut pkg = qu_ooxml::Package::empty();
    pkg.set("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
    pkg.set("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
    let table = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>\
         <w:tr><w:trPr><w:tblHeader/></w:trPr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>",
        cell(3000, "h1", true), cell(3000, "h2", true), cell(3000, "h3", true),
        cell(3000, "a1", false), cell(3000, "a2", false), cell(3000, "a3", false),
        cell(3000, "b1", false), cell(3000, "b2", false), cell(3000, "b3", false)
    );
    let body = format!(
        "<w:p><w:r><w:t>Intro text</w:t></w:r></w:p>\
         <w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">See the </w:t></w:r><w:r><w:t>Qu website now.</w:t></w:r></w:p>\
         <w:p><w:r><w:t xml:space=\"preserve\">Visit </w:t></w:r><w:hyperlink w:anchor=\"x\"><w:r><w:rPr><w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t>the Qu site</w:t></w:r></w:hyperlink><w:r><w:t xml:space=\"preserve\"> today, and the Qu docs.</w:t></w:r></w:p>\
         {table}\
         <w:p><w:r><w:t>Last paragraph.</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>"
    );
    pkg.set("word/document.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\"><w:body>{body}</w:body></w:document>").into_bytes());
    pkg.set("customXml/item1.xml", KEEP_XML.to_vec());
    pkg.set("word/theme/theme1.xml", KEEP_THEME.to_vec());
    Document::from_package(pkg).unwrap()
}

fn roundtrip(d: &mut Document) -> Document {
    Document::from_bytes(&d.to_bytes().unwrap()).unwrap()
}

fn untouched(d: &Document) {
    assert_eq!(d.pkg.get("customXml/item1.xml").unwrap(), KEEP_XML);
    assert_eq!(d.pkg.get("word/theme/theme1.xml").unwrap(), KEEP_THEME);
}

fn para(d: &Document, i: usize) -> &Element {
    d.body().elems().filter(|e| e.name == "w:p").nth(i).unwrap()
}

fn table(d: &Document) -> &Element {
    d.body().elems().find(|e| e.name == "w:tbl").unwrap()
}

/// Every run under `p` (links and insertions included) as (text, rPr child names).
fn runs(p: &Element) -> Vec<(String, Vec<String>)> {
    p.find_all("w:r").into_iter().map(|r| (para_display_text(r), r.child("w:rPr").map(|x| x.elems().map(|c| c.name.clone()).collect()).unwrap_or_default())).collect()
}

fn assert_schema_order(rpr: &Element) {
    let ranks: Vec<usize> = rpr.elems().filter_map(|c| RPR_ORDER.iter().position(|n| *n == c.name)).collect();
    assert!(ranks.windows(2).all(|w| w[0] < w[1]), "rPr children out of schema order: {}", rpr.to_xml());
}

fn one_rpr_per_run(d: &Document) {
    for r in d.doc.root.find_all("w:r") {
        assert!(r.elems().filter(|c| c.name == "w:rPr").count() <= 1, "a run carries two rPr: {}", r.to_xml());
    }
}

fn grid(d: &Document) -> Vec<i64> {
    grid_widths(table(d))
}

fn cells(d: &Document) -> Vec<Vec<(usize, usize, Vm)>> {
    table(d).elems().filter(|r| r.name == "w:tr").map(|r| row_cells(r).iter().map(|c| (c.start, c.span, c.vm)).collect()).collect()
}

fn bold() -> RunFormat {
    RunFormat { bold: Some(true), ..RunFormat::default() }
}

// ---------------------------------------------------------- formatting

#[test]
fn format_text_cuts_runs_and_changes_only_the_span() {
    let mut d = fixture();
    let fmt = RunFormat { italic: Some(true), color: Some("#ff0000".into()), size: Some(14.0), font: Some("Arial".into()), underline: Some(true), ..RunFormat::default() };
    assert_eq!(d.format_text(1, Some("the Qu"), false, &fmt).unwrap(), 1);
    let back = roundtrip(&mut d);
    untouched(&back);
    one_rpr_per_run(&back);
    assert_eq!(back.paragraphs()[1], "See the Qu website now.", "text unchanged");
    let r = runs(para(&back, 1));
    let texts: Vec<&str> = r.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(texts, ["See ", "the ", "Qu", " website now."]);
    // Before the span: still just bold. Inside, the bold run's piece gained
    // the formatting and kept its bold.
    assert_eq!(r[0].1, ["w:b"]);
    assert_eq!(r[1].1, ["w:rFonts", "w:b", "w:i", "w:iCs", "w:color", "w:sz", "w:szCs", "w:u"]);
    assert_eq!(r[2].1, ["w:rFonts", "w:i", "w:iCs", "w:color", "w:sz", "w:szCs", "w:u"]);
    assert!(r[3].1.is_empty(), "after the span: untouched, no rPr invented");
    let run_els: Vec<&Element> = para(&back, 1).elems().filter(|e| e.name == "w:r").collect();
    assert_schema_order(run_els[1].child("w:rPr").unwrap());
    let rpr = run_els[2].child("w:rPr").unwrap();
    assert_eq!(rpr.child("w:color").unwrap().attr("w:val"), Some("FF0000"));
    assert_eq!(rpr.child("w:sz").unwrap().attr("w:val"), Some("28"));
    assert_eq!(rpr.child("w:rFonts").unwrap().attr("w:ascii"), Some("Arial"));
}

#[test]
fn format_text_switches_inherited_properties_off_and_reaches_inside_links() {
    let mut d = fixture();
    d.format_text(1, Some("See"), false, &RunFormat { bold: Some(false), ..RunFormat::default() }).unwrap();
    let b = para(&d, 1).elems().find(|e| e.name == "w:r").unwrap().child("w:rPr").unwrap().child("w:b").unwrap().clone();
    assert_eq!(b.attr("w:val"), Some("0"), "bold off is explicit, so a style cannot bring it back");
    // A span that starts inside a hyperlink and ends after it.
    d.format_text(2, Some("Qu site today"), false, &bold()).unwrap();
    let back = roundtrip(&mut d);
    one_rpr_per_run(&back);
    let p = para(&back, 2);
    assert_eq!(para_display_text(p), "Visit the Qu site today, and the Qu docs.");
    let link = p.child("w:hyperlink").unwrap();
    assert_eq!(link.attr("w:anchor"), Some("x"), "the link survives the cut");
    let in_link = runs(link);
    assert_eq!(in_link.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["the ", "Qu site"]);
    assert_eq!(in_link[0].1, ["w:rStyle"], "'the ' is before the span: untouched");
    assert_eq!(in_link[1].1, ["w:rStyle", "w:b", "w:bCs"], "inside the link: rStyle kept and first");
    // all=true counts every occurrence.
    let mut d = fixture();
    assert_eq!(d.format_text(2, Some("Qu"), true, &RunFormat { strike: Some(true), ..RunFormat::default() }).unwrap(), 2);
    assert_eq!(d.doc.root.find_all("w:strike").len(), 2);
    // The whole paragraph.
    assert_eq!(d.format_text(0, None, false, &bold()).unwrap(), 1);
    assert_eq!(runs(para(&d, 0)), vec![("Intro text".to_string(), vec!["w:b".to_string(), "w:bCs".to_string()])]);
}

#[test]
fn format_text_refuses_what_it_cannot_do() {
    let mut d = fixture();
    assert!(d.format_text(1, Some("Qu"), false, &RunFormat::default()).unwrap_err().contains("nothing to apply"));
    assert!(d.format_text(1, Some("absent"), false, &bold()).unwrap_err().contains("does not contain"));
    assert!(d.format_text(99, None, false, &bold()).unwrap_err().contains("does not exist"));
    assert!(d.format_text(1, Some("Qu"), false, &RunFormat { color: Some("red".into()), ..bold() }).unwrap_err().contains("#rrggbb"));
    assert!(d.format_text(1, Some("Qu"), false, &RunFormat { size: Some(0.0), ..bold() }).unwrap_err().contains("size"));
    assert_eq!(d.doc.root.find_all("w:rPr").len(), 5, "the failures changed nothing");
}

/// Regression: cutting a run used to give its right half TWO `w:rPr`
/// (found while building format_text), which Word rejects.
#[test]
fn cutting_a_run_does_not_duplicate_its_properties() {
    let mut d = fixture();
    d.add_link(1, "https://example.org", Some("the Qu")).unwrap();
    d.add_comment(1, "c", Some("website"), "Qu", "", None).unwrap();
    one_rpr_per_run(&d);
}

// ---------------------------------------------------------- spacing, moving

#[test]
fn line_spacing_sets_the_spacing_element_in_schema_order() {
    let mut d = fixture();
    let slot = d.para_slot(3).unwrap();
    d.para_at(slot).children.insert(0, Node::Elem(Element::new("w:pPr").with_child(Element::new("w:pStyle").with_attr("w:val", "Quote")).with_child(Element::new("w:jc").with_attr("w:val", "center"))));
    let n = d.line_spacing(0, Some(3), &Spacing { lines: Some(1.5), before: Some(6.0), after: Some(12.0), ..Spacing::default() }).unwrap();
    assert_eq!(n, 4);
    let back = roundtrip(&mut d);
    let sp = para(&back, 0).child("w:pPr").unwrap().child("w:spacing").unwrap();
    assert_eq!((sp.attr("w:line"), sp.attr("w:lineRule"), sp.attr("w:before"), sp.attr("w:after")), (Some("360"), Some("auto"), Some("120"), Some("240")));
    let ppr = para(&back, 3).child("w:pPr").unwrap();
    assert_eq!(ppr.elems().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["w:pStyle", "w:spacing", "w:jc"], "spacing goes between pStyle and jc");
    assert_eq!(para(&back, 0).elems().next().unwrap().name, "w:pPr", "a new pPr is the paragraph's first child");
    // Switching rule keeps the other attributes and replaces the line.
    let mut d = back;
    d.line_spacing(0, None, &Spacing { exactly: Some(15.0), ..Spacing::default() }).unwrap();
    let sp = para(&d, 0).child("w:pPr").unwrap().child("w:spacing").unwrap();
    assert_eq!((sp.attr("w:line"), sp.attr("w:lineRule"), sp.attr("w:before")), (Some("300"), Some("exact"), Some("120")));
    d.line_spacing(0, None, &Spacing { at_least: Some(14.0), ..Spacing::default() }).unwrap();
    assert_eq!(para(&d, 0).child("w:pPr").unwrap().child("w:spacing").unwrap().attr("w:lineRule"), Some("atLeast"));
    assert!(d.line_spacing(0, None, &Spacing { lines: Some(1.0), exactly: Some(10.0), ..Spacing::default() }).unwrap_err().contains("not several"));
    assert!(d.line_spacing(0, None, &Spacing::default()).unwrap_err().contains("nothing to set"));
    assert!(d.line_spacing(0, Some(40), &Spacing { lines: Some(2.0), ..Spacing::default() }).unwrap_err().contains("does not exist"));
    assert!(d.line_spacing(0, None, &Spacing { lines: Some(0.0), ..Spacing::default() }).is_err());
    untouched(&d);
}

#[test]
fn move_paragraph_reorders_and_leaves_tables_alone() {
    let mut d = fixture();
    assert_eq!(d.paragraphs().len(), 4);
    d.move_paragraph(3, 0).unwrap();
    assert_eq!(d.paragraphs(), ["Last paragraph.", "Intro text", "See the Qu website now.", "Visit the Qu site today, and the Qu docs."]);
    d.move_paragraph(0, 3).unwrap();
    assert_eq!(d.paragraphs()[3], "Last paragraph.");
    assert_eq!(d.paragraphs()[0], "Intro text");
    d.move_paragraph(1, 2).unwrap();
    assert_eq!(d.paragraphs()[2], "See the Qu website now.");
    d.move_paragraph(2, 2).unwrap();
    assert!(d.move_paragraph(0, 4).unwrap_err().contains("cannot move"));
    assert!(d.move_paragraph(9, 0).unwrap_err().contains("does not exist"));
    // From before the table to after it: the table stays between.
    let mut d = fixture();
    d.move_paragraph(0, 3).unwrap();
    let names: Vec<&str> = d.body().elems().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["w:p", "w:p", "w:tbl", "w:p", "w:p", "w:sectPr"]);
    assert_eq!(d.paragraphs(), ["See the Qu website now.", "Visit the Qu site today, and the Qu docs.", "Last paragraph.", "Intro text"]);
    assert_eq!(d.tables()[0][0], ["h1", "h2", "h3"]);
    // A paragraph that holds a section break stays put.
    let mut d = fixture();
    let slot = d.para_slot(0).unwrap();
    d.para_at(slot).children.insert(0, Node::Elem(Element::new("w:pPr").with_child(Element::new("w:sectPr"))));
    assert!(d.move_paragraph(0, 2).unwrap_err().contains("section break"));
}

// ---------------------------------------------------------- tables

#[test]
fn add_row_copies_the_neighbour_and_never_the_header_flag() {
    let mut d = fixture();
    d.add_row(0, &["x".into(), "y".into()], None).unwrap();
    d.add_row(0, &[], Some(1)).unwrap();
    d.add_row(0, &["top".into()], Some(0)).unwrap();
    let back = roundtrip(&mut d);
    untouched(&back);
    assert_eq!(back.tables()[0], vec![vec!["top", "", ""], vec!["h1", "h2", "h3"], vec!["", "", ""], vec!["a1", "a2", "a3"], vec!["b1", "b2", "b3"], vec!["x", "y", ""]]);
    let rows: Vec<&Element> = table(&back).elems().filter(|r| r.name == "w:tr").collect();
    assert!(rows[1].child("w:trPr").is_some(), "the real header keeps its flag");
    assert!(rows[0].child("w:trPr").is_none() && rows[2].child("w:trPr").is_none() && rows[5].child("w:trPr").is_none(), "no copy of the header flag");
    let c = rows[5].elems().find(|c| c.name == "w:tc").unwrap();
    assert_eq!(c.child("w:tcPr").unwrap().child("w:tcW").unwrap().attr("w:w"), Some("3000"));
    assert!(c.child("w:p").unwrap().child("w:pPr").unwrap().child("w:jc").is_some(), "paragraph formatting copied");
    assert!(c.find_all("w:b").is_empty(), "the body row's character formatting, not the header's bold");
    assert!(d.add_row(0, &vec!["1".into(); 4], None).unwrap_err().contains("4 values for a row of 3"));
    assert!(d.add_row(0, &[], Some(99)).unwrap_err().contains("cannot insert at row 99"));
    assert!(d.add_row(5, &[], None).unwrap_err().contains("does not exist"));
}

#[test]
fn add_row_into_a_vertical_merge_extends_it() {
    let mut d = fixture();
    d.merge_cells(0, 1, 0, 2, 0).unwrap(); // a1 + b1
    d.add_row(0, &[], Some(2)).unwrap(); // between them
    let c = cells(&d);
    assert_eq!((c[1][0].2, c[2][0].2, c[3][0].2), (Vm::Restart, Vm::Continue, Vm::Continue), "a row inserted inside a merged block joins it");
    d.add_row(0, &[], None).unwrap(); // after the block
    assert_eq!(cells(&d)[4][0].2, Vm::No);
}

#[test]
fn add_column_keeps_the_table_width_and_every_row_whole() {
    let mut d = fixture();
    let before: i64 = grid(&d).iter().sum();
    d.add_column(0, &["N".into(), "n1".into(), "n2".into()], Some(1)).unwrap();
    d.add_column(0, &[], None).unwrap();
    let back = roundtrip(&mut d);
    untouched(&back);
    assert_eq!(back.tables()[0], vec![vec!["h1", "N", "h2", "h3", ""], vec!["a1", "n1", "a2", "a3", ""], vec!["b1", "n2", "b2", "b3", ""]]);
    assert_eq!(grid(&back).len(), 5);
    assert_eq!(grid(&back).iter().sum::<i64>(), before, "the right edge did not move");
    for r in table(&back).elems().filter(|r| r.name == "w:tr") {
        let w: i64 = r.elems().filter(|c| c.name == "w:tc").filter_map(tcw).sum();
        assert!((w - before).abs() <= 5, "row width {w} vs {before}");
    }
    let hdr = table(&back).elems().find(|r| r.name == "w:tr").unwrap().elems().filter(|c| c.name == "w:tc").nth(1).unwrap();
    assert!(!hdr.find_all("w:b").is_empty(), "a header cell's new neighbour is bold too");
    assert!(d.add_column(0, &[], Some(9)).unwrap_err().contains("cannot insert at column 9"));
    assert!(d.add_column(0, &vec!["x".into(); 9], None).unwrap_err().contains("9 values for a table of 3 rows"));
}

#[test]
fn add_column_inside_a_merged_cell_widens_it() {
    let mut d = fixture();
    d.merge_cells(0, 1, 0, 1, 1).unwrap(); // a1+a2 span two columns
    d.add_column(0, &["h".into(), "ignored".into(), "b".into()], Some(1)).unwrap();
    let c = cells(&d);
    assert_eq!(c[1], vec![(0, 3, Vm::No), (3, 1, Vm::No)], "the spanning cell now covers the new column");
    assert_eq!(c[0].len(), 4);
    assert_eq!(d.tables()[0][2], vec!["b1", "b", "b2", "b3"]);
}

#[test]
fn merge_and_split_cells_round_trip() {
    let mut d = fixture();
    let original = cells(&d);
    let ws = grid(&d);
    // Horizontal: h1+h2.
    d.merge_cells(0, 0, 0, 0, 1).unwrap();
    assert_eq!(cells(&d)[0], vec![(0, 2, Vm::No), (2, 1, Vm::No)]);
    assert_eq!(d.tables()[0][0], vec!["h1\nh2", "h3"], "the other cell's text follows, one paragraph each");
    let tc = table(&d).elems().find(|r| r.name == "w:tr").unwrap().elems().find(|c| c.name == "w:tc").unwrap();
    assert_eq!(tcw(tc), Some(6000), "merged width is the sum");
    // 2x2 block: a2, a3, b2, b3.
    d.merge_cells(0, 1, 1, 2, 2).unwrap();
    assert_eq!(cells(&d)[1], vec![(0, 1, Vm::No), (1, 2, Vm::Restart)]);
    assert_eq!(cells(&d)[2], vec![(0, 1, Vm::No), (1, 2, Vm::Continue)]);
    assert_eq!(d.tables()[0][1], vec!["a1", "a2\na3\nb2\nb3"]);
    assert_eq!(d.tables()[0][2], vec!["b1", ""]);
    let back = roundtrip(&mut d);
    untouched(&back);
    // A continuation cell still ends in a paragraph, as a cell must.
    let cont = table(&back).elems().filter(|r| r.name == "w:tr").nth(2).unwrap().elems().filter(|c| c.name == "w:tc").nth(1).unwrap();
    assert_eq!(cont.elems().last().unwrap().name, "w:p");
    // Undo both, naming a cell inside each block.
    let mut d = back;
    d.split_cell(0, 2, 2, None).unwrap();
    d.split_cell(0, 0, 1, None).unwrap();
    assert_eq!(cells(&d), original, "same grid as before any merge");
    assert_eq!(grid(&d), ws);
    let t = d.tables()[0].clone();
    assert_eq!(t[0], vec!["h1\nh2", "", "h3"], "text stays in the top-left cell");
    assert_eq!(t[1], vec!["a1", "a2\na3\nb2\nb3", ""]);
    assert_eq!(t[2], vec!["b1", "", ""]);
    let tc = table(&d).elems().find(|r| r.name == "w:tr").unwrap().elems().filter(|c| c.name == "w:tc").nth(1).unwrap();
    assert_eq!(tcw(tc), Some(3000), "the split cell has its grid column's width back");
}

#[test]
fn merge_cells_validates_before_touching_anything() {
    let mut d = fixture();
    let snapshot = d.doc.clone();
    assert!(d.merge_cells(0, 1, 1, 0, 0).unwrap_err().contains("backwards"));
    assert!(d.merge_cells(0, 1, 1, 1, 1).unwrap_err().contains("single cell"));
    assert!(d.merge_cells(0, 0, 0, 9, 1).unwrap_err().contains("3 rows"));
    assert!(d.merge_cells(0, 0, 0, 1, 9).unwrap_err().contains("3 columns"));
    assert_eq!(d.doc, snapshot, "refused merges changed nothing");
    d.merge_cells(0, 0, 0, 0, 1).unwrap();
    let merged = d.doc.clone();
    assert!(d.merge_cells(0, 0, 1, 1, 2).unwrap_err().contains("cuts through a merged cell"));
    assert_eq!(d.doc, merged);
    d.merge_cells(0, 1, 0, 2, 0).unwrap();
    assert!(d.merge_cells(0, 2, 0, 2, 1).unwrap_err().contains("starts in the middle of a vertically merged cell"));
    assert!(d.merge_cells(0, 1, 0, 1, 1).unwrap_err().contains("ends in the middle of a vertically merged cell"));
    // Splitting what is not merged.
    let mut d = fixture();
    assert!(d.split_cell(0, 1, 1, None).unwrap_err().contains("not merged"));
    assert!(d.split_cell(0, 1, 1, Some(1)).unwrap_err().contains("2 or more"));
}

#[test]
fn split_cell_into_columns_keeps_other_rows_aligned() {
    let mut d = fixture();
    d.split_cell(0, 1, 1, Some(3)).unwrap();
    let back = roundtrip(&mut d);
    assert_eq!(grid(&back), vec![3000, 1000, 1000, 1000, 3000]);
    let c = cells(&back);
    assert_eq!(c[1].len(), 5, "the cut row has five cells");
    assert_eq!(c[0], vec![(0, 1, Vm::No), (1, 3, Vm::No), (4, 1, Vm::No)], "other rows span the new columns");
    assert_eq!(c[2], vec![(0, 1, Vm::No), (1, 3, Vm::No), (4, 1, Vm::No)]);
    assert_eq!(back.tables()[0][1], vec!["a1", "a2", "", "", "a3"]);
    // And the pieces merge back.
    let mut d = back;
    d.merge_cells(0, 1, 1, 1, 3).unwrap();
    assert_eq!(cells(&d)[1], vec![(0, 1, Vm::No), (1, 3, Vm::No), (4, 1, Vm::No)]);
    assert_eq!(d.tables()[0][1], vec!["a1", "a2", "a3"]);
    assert!(d.split_cell(0, 1, 1, Some(2)).unwrap_err().contains("merged"));
}

// ---------------------------------------------------------- footnotes

#[test]
fn footnotes_are_created_from_nothing_and_numbered_by_position() {
    let mut d = fixture();
    d.add_footnote(1, "Source: the Qu site.", Some("website")).unwrap();
    d.add_footnote(0, "A note\nsecond line", None).unwrap();
    d.add_footnote(2, "Inside the link.", Some("Qu site")).unwrap();
    let back = roundtrip(&mut d);
    untouched(&back);
    assert_eq!(back.notes("footnotes"), vec!["Source: the Qu site.", "A note\nsecond line", "Inside the link."]);
    let ct = back.pkg.get_str("[Content_Types].xml").unwrap();
    assert!(ct.contains("/word/footnotes.xml") && ct.contains("footnotes+xml") && ct.contains("/word/styles.xml") && ct.contains("/word/settings.xml"), "{ct}");
    let part = back.pkg.get_str("word/footnotes.xml").unwrap();
    assert!(part.contains("w:type=\"separator\"") && part.contains("w:type=\"continuationSeparator\""), "{part}");
    let settings = back.pkg.get_str("word/settings.xml").unwrap();
    assert!(settings.contains("<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>"), "{settings}");
    let styles = back.pkg.get_str("word/styles.xml").unwrap();
    assert!(styles.contains("w:styleId=\"FootnoteReference\"") && styles.contains("w:styleId=\"FootnoteText\"") && styles.contains("superscript"));
    // Where the references sit.
    let p1 = para(&back, 1);
    let kinds: Vec<&str> = p1.elems().map(|e| if e.find_all("w:footnoteReference").is_empty() { "run" } else { "ref" }).collect();
    assert_eq!(kinds, ["run", "run", "ref", "run"], "the reference follows 'website' ({})", p1.to_xml());
    assert_eq!(para_display_text(p1), "See the Qu website now.");
    assert!(para(&back, 2).child("w:hyperlink").unwrap().find_all("w:footnoteReference").is_empty(), "a note at a link's end sits outside the link");
    let ids: Vec<&str> = back.doc.root.find_all("w:footnoteReference").iter().filter_map(|r| r.attr("w:id")).collect();
    assert_eq!(ids, ["2", "1", "3"], "ids follow creation; Word numbers by position");
    // A later note reuses the part.
    let mut again = back;
    again.add_footnote(3, "Another", None).unwrap();
    assert_eq!(again.notes("footnotes").len(), 4);
    assert!(again.add_footnote(0, "  ", None).unwrap_err().contains("empty"));
    assert!(again.add_footnote(0, "x", Some("absent")).unwrap_err().contains("does not contain"));
    assert_eq!(again.notes("footnotes").len(), 4, "a refused footnote leaves nothing behind");
}

// ---------------------------------------------------------- tracked changes

fn tracked() -> Document {
    let mut d = fixture();
    d.track_insert(1, "really ", None, Some("Qu"), "Ahmed", Some("2026-10-01T09:00:00Z")).unwrap();
    assert_eq!(d.track_delete(1, Some("website "), false, "Ahmed", Some("2026-10-01T09:01:00Z")).unwrap(), 1);
    d.track_insert(0, "!", Some("text"), None, "Ahmed", Some("2026-10-01T09:02:00Z")).unwrap();
    d
}

#[test]
fn track_insert_and_delete_produce_revisions_that_accept_and_reject() {
    let mut d = tracked();
    let back = roundtrip(&mut d);
    untouched(&back);
    one_rpr_per_run(&back);
    let ins = back.doc.root.find_all("w:ins");
    let del = back.doc.root.find_all("w:del");
    assert_eq!((ins.len(), del.len()), (2, 1));
    // Document order: the "!" in paragraph 0 comes before "really " in paragraph 1.
    assert_eq!(ins[1].attr("w:author"), Some("Ahmed"));
    assert_eq!(ins[1].attr("w:date"), Some("2026-10-01T09:00:00Z"));
    assert_eq!(ins[0].attr("w:date"), Some("2026-10-01T09:02:00Z"));
    assert_eq!(del[0].attr("w:date"), Some("2026-10-01T09:01:00Z"));
    let mut ids: Vec<&str> = ins.iter().chain(del.iter()).filter_map(|e| e.attr("w:id")).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "revision ids are unique");
    assert_eq!(del[0].find_all("w:delText")[0].text(), "website ");
    assert!(del[0].find_all("w:t").is_empty(), "no live w:t left inside a deletion");
    // What a reader sees: insertions shown, deletions hidden.
    assert_eq!(back.paragraphs()[1], "See the really Qu now.");
    assert_eq!(back.paragraphs()[0], "Intro text!");
    // The inserted run took the formatting of the run before it (the bold "See the ").
    assert_eq!(runs(ins[1])[0].1, ["w:b"]);
    let mut acc = tracked();
    assert_eq!(acc.resolve_changes(true).unwrap(), 3);
    assert_eq!(acc.paragraphs()[1], "See the really Qu now.");
    assert_eq!(acc.paragraphs()[0], "Intro text!");
    assert!(acc.doc.root.find_all("w:delText").is_empty());
    let mut rej = tracked();
    rej.resolve_changes(false).unwrap();
    assert_eq!(rej.paragraphs()[1], "See the Qu website now.");
    assert_eq!(rej.paragraphs()[0], "Intro text");
    assert!(rej.doc.root.find_all("w:ins").is_empty() && rej.doc.root.find_all("w:del").is_empty());
    assert!(rej.doc.root.find_all("w:delText").is_empty(), "rejection restores w:t");
}

#[test]
fn tracked_deletion_of_a_whole_paragraph_removes_it_on_accept() {
    let mut d = fixture();
    assert_eq!(d.track_delete(0, None, false, "Qu", None).unwrap(), 1);
    let ppr = para(&d, 0).child("w:pPr").unwrap();
    assert_eq!(ppr.child("w:rPr").unwrap().elems().next().unwrap().name, "w:del", "the paragraph mark is marked first in its rPr");
    assert_eq!(d.paragraphs()[0], "", "deleted text is not displayed");
    let mut rej = roundtrip(&mut d);
    rej.resolve_changes(false).unwrap();
    assert_eq!(rej.paragraphs()[0], "Intro text");
    assert_eq!(rej.paragraphs().len(), 4);
    d.resolve_changes(true).unwrap();
    assert_eq!(d.paragraphs().len(), 3);
    assert_eq!(d.paragraphs()[0], "See the Qu website now.");
    // Several spans at once.
    let mut d = fixture();
    assert_eq!(d.track_delete(2, Some("Qu"), true, "Qu", None).unwrap(), 2);
    assert_eq!(d.doc.root.find_all("w:del").len(), 2);
    assert_eq!(d.paragraphs()[2], "Visit the  site today, and the  docs.");
}

#[test]
fn tracked_edits_refuse_fields_and_nested_revisions() {
    let mut d = fixture();
    d.add_field(0, "PAGE", "7").unwrap();
    assert_eq!(d.paragraphs()[0], "Intro text7");
    assert!(d.track_delete(0, Some("7"), false, "Qu", None).unwrap_err().contains("field"));
    assert!(d.track_insert(0, "x", None, None, "Qu", None).is_ok(), "the end of the paragraph is always free");
    d.track_insert(1, "NEW", Some("See"), None, "Qu", None).unwrap();
    assert!(d.track_insert(1, "again", Some("SeeN"), None, "Qu", None).unwrap_err().contains("tracked insertion"));
    assert!(d.track_insert(1, "x", Some("NE"), None, "Qu", None).unwrap_err().contains("tracked insertion"));
    assert!(d.track_insert(1, "", None, None, "Qu", None).unwrap_err().contains("empty"));
    assert!(d.track_insert(1, "x", Some("a"), Some("b"), "Qu", None).unwrap_err().contains("not both"));
    assert!(d.track_insert(1, "x", Some("absent"), None, "Qu", None).unwrap_err().contains("does not contain"));
    // Deleting inserted text nests the deletion in the insertion.
    d.track_delete(1, Some("NEW"), false, "Qu", None).unwrap();
    assert!(d.doc.root.find_all("w:ins").iter().any(|i| !i.find_all("w:del").is_empty()));
    d.resolve_changes(true).unwrap();
    assert_eq!(d.paragraphs()[1], "See the Qu website now.");
}

#[test]
fn every_edit_leaves_unknown_parts_byte_for_byte() {
    let mut d = fixture();
    d.format_text(1, Some("Qu"), false, &bold()).unwrap();
    d.line_spacing(0, None, &Spacing { lines: Some(2.0), ..Spacing::default() }).unwrap();
    d.move_paragraph(0, 1).unwrap();
    d.add_row(0, &[], None).unwrap();
    d.add_column(0, &[], None).unwrap();
    d.merge_cells(0, 0, 0, 1, 1).unwrap();
    d.split_cell(0, 0, 0, None).unwrap();
    d.add_footnote(0, "n", None).unwrap();
    d.track_insert(0, "i", None, None, "Qu", None).unwrap();
    d.track_delete(0, Some("See"), false, "Qu", None).unwrap();
    let back = roundtrip(&mut d);
    untouched(&back);
    assert_eq!(back.pkg.get("_rels/.rels").unwrap(), fixture().pkg.get("_rels/.rels").unwrap());
}
