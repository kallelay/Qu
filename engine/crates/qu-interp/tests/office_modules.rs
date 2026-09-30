//! `import docx` / `import pptx` / `import xlsx` workbooks, through the
//! language: argument order, keyword names, handle lifetime, lengths as
//! units, and the files surviving a save/reopen.
#![cfg(all(feature = "docx", feature = "pptx", feature = "xlsx"))]

use qu_interp::{Interp, Value};

fn tmp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("qu_office_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().replace('\\', "/")
}

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

fn num(it: &Interp, n: &str) -> f64 {
    match it.get(n) {
        Some(Value::Num(x)) => *x,
        other => panic!("`{n}` is {other:?}"),
    }
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{n}` is {other:?}"),
    }
}

/// A 4x2 PNG written from bytes, so the tests need no image on disk.
fn png(path: &str) {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend_from_slice(&4u32.to_be_bytes());
    b.extend_from_slice(&2u32.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    std::fs::write(path, b).unwrap();
}

#[test]
fn docx_build_save_reopen_edit() {
    let (a, b, img) = (tmp("a.docx"), tmp("b.docx"), tmp("i.png"));
    png(&img);
    let it = run(&format!(
        r##"
import docx
doc = docx.new()
docx.add_heading(doc, "Report", 0)
docx.add_heading(doc, "Results", 1)
docx.add_paragraph(doc, "Z was 2025 ohm.", bold=true, size=12)
docx.add_table(doc, table(f=[1000, 2000], Z=[42.3, 40.1]))
docx.add_image(doc, "{img}", width=4 cm)
docx.save_as(doc, "{a}")
d = docx.open("{a}")
n = docx.replace_text(d, "2025", "2026")
docx.set_cell(d, 0, 1, 1, "43")
docx.save_as(d, "{b}")
e = docx.open("{b}")
body = docx.full_text(e)
h = docx.headings(e)
nh = len(h)
cell = docx.tables(e)[0][1][1]
md = docx.to_markdown(e)
tex = docx.to_latex(e, full=false)
info = docx.info(e)
images = info.images
"##
    ));
    assert_eq!(num(&it, "n"), 1.0);
    assert!(text(&it, "body").contains("Z was 2026 ohm."));
    assert_eq!(num(&it, "nh"), 2.0);
    assert_eq!(text(&it, "cell"), "43");
    assert!(text(&it, "md").contains("| f | Z |"));
    assert!(text(&it, "tex").contains("\\section{Results}"));
    assert_eq!(num(&it, "images"), 1.0);
}

#[test]
fn docx_structure_links_comments_sections_lists_fields() {
    let (a, b) = (tmp("s1.docx"), tmp("s2.docx"));
    let it = run(&format!(
        r##"
import docx
doc = docx.new()
docx.add_heading(doc, "Method", 1)
docx.add_paragraph(doc, "See the Qu website for details.")
docx.add_paragraph(doc, "As shown in ")
docx.add_link(doc, 1, "https://example.org/qu", on="Qu website")
docx.add_comment(doc, 1, "Cite it", on="details", author="Ahmed", initials="AK", date="2026-09-30T10:00:00Z")
docx.add_bookmark(doc, 0, "method")
docx.add_cross_ref(doc, 2, "method")
docx.add_cross_ref(doc, 2, "method", show="page")
docx.add_toc(doc, levels=2, at=0, title="Contents")
docx.set_header(doc, "Qu report", align="right")
docx.set_footer(doc, "Page #page of #pages", align="center")
docx.add_list_item(doc, "first")
docx.add_list_item(doc, "nested", level=1)
docx.add_list_item(doc, "step", kind="number")
docx.add_section_break(doc, start="continuous")
docx.add_paragraph(doc, "wide")
docx.page_setup(doc, section=1, orientation="landscape", margins=15 mm)
docx.page_setup(doc, section=0, size="Letter", top=2 cm)
docx.save_as(doc, "{a}")
d = docx.open("{a}")
links = docx.links(d)
link_text = links[0].text
link_url = links[0].url
c = docx.comments(d)
c_author = c[0].author
c_text = c[0].text
marks = docx.bookmarks(d)
mark = marks[0].name
fs = docx.fields(d)
nf = len(fs)
toc = fs[0]
ref = fs[1]
xref = docx.paragraphs(d)[4]
head = docx.header_text(d)
foot = docx.footer_text(d)
none_first = docx.header_text(d, kind="first")
s = docx.sections(d)
ns = len(s)
w0 = s[0].width
top0 = s[0].top
o1 = s[1].orientation
w1 = s[1].width
left1 = s[1].left
start1 = s[1].start
inherited = docx.header_text(d, section=1)
md = docx.to_markdown(d)
docx.set_header(d, "Appendix", section=1)
docx.set_list(d, 5, kind="none")
docx.save_as(d, "{b}")
e = docx.open("{b}")
h0 = docx.header_text(e, section=0)
h1 = docx.header_text(e, section=1)
"##
    ));
    assert_eq!(text(&it, "link_text"), "Qu website");
    assert_eq!(text(&it, "link_url"), "https://example.org/qu");
    assert_eq!(text(&it, "c_author"), "Ahmed");
    assert_eq!(text(&it, "c_text"), "Cite it");
    assert_eq!(text(&it, "mark"), "method");
    assert_eq!(num(&it, "nf"), 3.0);
    assert_eq!(text(&it, "toc"), "TOC \\o \"1-2\" \\h \\z \\u");
    assert_eq!(text(&it, "ref"), "REF method \\h");
    assert_eq!(text(&it, "xref"), "As shown in Method", "paragraph 4 after the TOC title and field");
    assert_eq!(text(&it, "head"), "Qu report");
    assert_eq!(text(&it, "foot"), "Page 1 of 1");
    assert!(matches!(it.get("none_first"), Some(Value::Nothing)));
    assert_eq!(num(&it, "ns"), 2.0);
    assert_eq!(num(&it, "w0"), 215.9);
    assert_eq!(num(&it, "top0"), 20.0);
    assert_eq!(text(&it, "o1"), "landscape");
    assert_eq!(num(&it, "w1"), 297.0);
    assert_eq!(num(&it, "left1"), 15.0);
    assert_eq!(text(&it, "start1"), "continuous");
    assert_eq!(text(&it, "inherited"), "Qu report");
    assert!(text(&it, "md").contains("- first\n\n- nested\n\n- step"), "{}", text(&it, "md"));
    assert_eq!(text(&it, "h0"), "Qu report");
    assert_eq!(text(&it, "h1"), "Appendix");
}

#[test]
fn docx_structure_arguments_are_checked() {
    let pre = "import docx\nd = docx.new()\ndocx.add_paragraph(d, \"hello world\")\n";
    let bad = |tail: &str| err(&format!("{pre}{tail}"));
    assert!(bad("docx.add_link(d, 0, \"https://x\", onn=\"hello\")").contains("onn"), "unread keyword is an error");
    assert!(bad("docx.add_link(d, 0, \"https://x\", on=\"absent\")").contains("does not contain"));
    assert!(bad("docx.add_link(d, 3, \"https://x\")").contains("paragraph 3"));
    assert!(bad("docx.add_comment(d, 0, \"c\", on=5)").contains("string"));
    assert!(bad("docx.add_cross_ref(d, 0, \"nope\")").contains("no bookmark"));
    assert!(bad("docx.add_toc(d, levels=12)").contains("1 to 9"));
    assert!(bad("docx.page_setup(d, size=\"A9\")").contains("A4"));
    assert!(bad("docx.page_setup(d, orientation=\"sideways\")").contains("landscape"));
    assert!(bad("docx.page_setup(d)").contains("at least one"));
    assert!(bad("docx.page_setup(d, margins=3 s)").contains("length"));
    assert!(bad("docx.set_header(d, \"h\", kind=\"odd-ish\")").contains("default, first or even"));
    assert!(bad("docx.set_footer(d, \"h\", section=4)").contains("section 4"));
    assert!(bad("docx.add_list_item(d, \"x\", kind=\"roman\")").contains("bullet or number"));
    assert!(bad("docx.add_section_break(d, start=\"later\")").contains("next_page"));
    // A bad list call leaves no stray paragraph behind.
    let mut it = Interp::new();
    let _ = it.run(&format!("{pre}n0 = len(docx.paragraphs(d))"));
    let _ = it.run("docx.add_list_item(d, \"x\", level=12)");
    it.run("n1 = len(docx.paragraphs(d))").unwrap();
    assert_eq!(num(&it, "n0"), num(&it, "n1"));
}

#[test]
fn pptx_slides_shapes_and_rearranging() {
    let (a, img) = (tmp("a.pptx"), tmp("p.png"));
    png(&img);
    let it = run(&format!(
        r##"
import pptx
p = pptx.new()
pptx.add_slide(p, layout="Title Slide", title="Impedance", body="2025 lecture")
pptx.add_slide(p, title="Results", body=["one", "two"])
pptx.add_slide(p, layout="Blank")
pptx.add_image(p, 2, "{img}", x=40 mm, y=50 mm, w=12 cm)
pptx.add_text(p, 2, "note", x=180, y=60, size=24, bold=true)
pptx.add_table(p, 1, table(f=[1, 2], Z=[3, 4]), x=30, y=110, w=120)
dup = pptx.duplicate_slide(p, 1)
pptx.move_slide(p, 3, 0)
k = pptx.replace_text(p, "2025", "2026")
pptx.save_as(p, "{a}")
q = pptx.open("{a}")
count = pptx.slide_count(q)
first = pptx.slide_text(q, 0)
title = pptx.slide_title(q, 1)
md = pptx.to_markdown(q)
"##
    ));
    assert_eq!(num(&it, "dup"), 2.0);
    assert_eq!(num(&it, "k"), 1.0);
    assert_eq!(num(&it, "count"), 4.0);
    assert!(text(&it, "first").contains("note"), "moved slide is first");
    assert_eq!(text(&it, "title"), "Impedance");
    assert!(text(&it, "md").contains("- 2026 lecture"));
}

#[test]
fn pptx_notes_shapes_links_and_theme() {
    let (a, b) = (tmp("n.pptx"), tmp("n2.pptx"));
    let it = run(&format!(
        r##"
import pptx
p = pptx.new()
pptx.add_slide(p, title="Results", body=["one", "two"])
pptx.add_slide(p, layout="Title Only")
pptx.add_text(p, 0, "see the Nyquist plot", x=20, y=150, w=10 cm, h=2 cm, size=20)
pptx.save_as(p, "{a}")
q = pptx.open("{a}")
before = pptx.notes(q, 0)
pptx.set_notes(q, 0, "Speak slowly.\nPause.")
sh = pptx.shapes(q, 0)
nshapes = len(sh)
kind2 = sh[2].kind
tb = sh[2].id
title_w = sh[0].w
pptx.set_shape_text(q, 0, tb, "the Nyquist plot, again")
pptx.move_shape(q, 0, tb, x=3 cm)
pptx.resize_shape(q, 0, "Title 1", h=25)
pptx.bring_to_front(q, 0, "Title 1")
nlinks = pptx.set_link(q, 0, tb, "https://example.org/nyquist", text="Nyquist")
pptx.set_slide_title(q, 1, "Made here")
pptx.set_theme_colors(q, accent1="#123456")
pptx.set_theme_fonts(q, major="Georgia")
pptx.save_as(q, "{b}")
r = pptx.open("{b}")
notes = pptx.notes(r, 0)
other = pptx.notes(r, 1)
sh2 = pptx.shapes(r, 0)
last_name = sh2[2].name
moved_x = sh2[1].x
title_h = sh2[2].h
lk = pptx.links(r, 0)
link_text = lk[0].text
link_url = lk[0].url
title1 = pptx.slide_title(r, 1)
body0 = pptx.slide_text(r, 0)
accent = pptx.theme_colors(r).accent1
major = pptx.theme_fonts(r).major
pptx.delete_shape(r, 0, tb)
after_delete = len(pptx.shapes(r, 0))
"##
    ));
    assert_eq!(text(&it, "before"), "");
    assert_eq!(num(&it, "nshapes"), 3.0);
    assert_eq!(text(&it, "kind2"), "text");
    assert!((num(&it, "title_w") - 10515600.0 / 36000.0).abs() < 1e-9, "inherited from the master");
    assert_eq!(text(&it, "notes"), "Speak slowly.\nPause.");
    assert_eq!(text(&it, "other"), "");
    assert_eq!(text(&it, "last_name"), "Title 1", "brought to front");
    assert_eq!(num(&it, "moved_x"), 30.0);
    assert_eq!(num(&it, "title_h"), 25.0);
    assert_eq!(num(&it, "nlinks"), 1.0);
    assert_eq!(text(&it, "link_text"), "Nyquist");
    assert_eq!(text(&it, "link_url"), "https://example.org/nyquist");
    assert_eq!(text(&it, "title1"), "Made here");
    assert!(text(&it, "body0").contains("the Nyquist plot, again"));
    assert_eq!(text(&it, "accent"), "#123456");
    assert_eq!(text(&it, "major"), "Georgia");
    assert_eq!(num(&it, "after_delete"), 2.0);
}

#[test]
fn pptx_shape_edits_check_their_arguments() {
    let base = "import pptx\np = pptx.new()\npptx.add_slide(p, title=\"T\")\n";
    assert!(err(&format!("{base}pptx.set_shape_text(p, 0, 42, \"x\")")).contains("no shape id 42"));
    assert!(err(&format!("{base}pptx.set_shape_text(p, 0, [1, 2], \"x\")")).contains("id (a whole number) or its name"));
    assert!(err(&format!("{base}pptx.move_shape(p, 0, \"Title 1\")")).contains("give x= and/or y="));
    assert!(err(&format!("{base}pptx.move_shape(p, 0, \"Title 1\", x=3 s)")).contains("length"));
    // A misspelt slot is an unread keyword, not silently ignored.
    assert!(err(&format!("{base}pptx.set_theme_colors(p, accent7=\"#000000\")")).contains("accent7"));
    assert!(err(&format!("{base}pptx.set_theme_colors(p, accent1=\"#000000\", acent2=\"#111111\")")).contains("acent2"));
    assert!(err(&format!("{base}pptx.set_theme_colors(p, accent1=\"blue\")")).contains("#rrggbb"));
    assert!(err(&format!("{base}pptx.set_link(p, 0, \"Title 1\", \"https://x\", text=\"nope\")")).contains("does not occur"));
    // Editing in memory is allowed in the sandbox; only writing is refused.
    let mut it = Interp::new();
    it.set_sandboxed(true);
    it.run(&format!("{base}pptx.set_notes(p, 0, \"n\")\nn = pptx.notes(p, 0)")).unwrap();
    assert_eq!(text(&it, "n"), "n");
    let msg = it.run(&format!("pptx.save_as(p, \"{}\")", tmp("sb.pptx"))).unwrap_err().to_string();
    assert!(msg.contains("save_as"), "{msg}");
}

#[test]
fn xlsx_workbook_edit_in_place() {
    let (a, b) = (tmp("a.xlsx"), tmp("b.xlsx"));
    let it = run(&format!(
        r##"
import xlsx
wb = xlsx.new()
xlsx.set_range(wb, "Sheet1", "A1", table(f=[1000, 2000], Z=[42.5, 40]))
xlsx.fill_formula(wb, "Sheet1", "C2:C3", "=A2*B2")
xlsx.format_cells(wb, "Sheet1", "A1:C1", bold=true, background="#DDEEFF")
xlsx.freeze_panes(wb, "Sheet1", "A2")
xlsx.add_sheet(wb, "Notes")
xlsx.set_cell(wb, "Notes", "A1", "bench")
xlsx.save_as(wb, "{a}")
w = xlsx.open("{a}")
z = xlsx.get_cell(w, "Sheet1", "B2")
f = xlsx.formula(w, 0, "C3")
used = xlsx.used_range(w, "Sheet1")
m = xlsx.get_range(w, "Sheet1", "A2:B3", numeric=true)
xlsx.set_cell(w, 0, "B2", 99)
xlsx.save_as(w, "{b}")
t = xlsx.read("{b}")
back = t.Z[0]
names = xlsx.sheets("{b}")
"##
    ));
    assert_eq!(num(&it, "z"), 42.5);
    assert_eq!(text(&it, "f"), "A3*B3");
    assert_eq!(text(&it, "used"), "A1:C3");
    assert_eq!(num(&it, "back"), 99.0);
    assert!(matches!(it.get("m"), Some(Value::Mat(m)) if m.shape() == (2, 2)));
}

/// Charts, conditional formats and validations through the language; the
/// package-level known answers live in qu-xlsx's `charts_cf_dv` tests.
#[test]
fn xlsx_charts_conditional_formats_and_validation() {
    let (a, b) = (tmp("ch.xlsx"), tmp("ch_edit.xlsx"));
    let it = run(&format!(
        r##"
import xlsx
wb = xlsx.new()
xlsx.set_range(wb, "Sheet1", "A1", table(f=[1, 10, 100, 1000], Zre=[110, 90, 40, 12], Zim=[-5, -45, -40, -3]))
xlsx.add_chart(wb, "Sheet1", "scatter", ["B2:B5", "C2:C5"], x="A2:A5", names=["Z'", "Z''"], title="Bode", x_title="f / Hz", x_log=true, at="F2", width=12 cm, height=80)
xlsx.add_chart(wb, "Sheet1", "bar", "B2:B5", x="A2:A5", colors=["teal"], legend=false)
helper = xlsx.add_nyquist_chart(wb, "Sheet1", "B2:B5", "C2:C5", title="Nyquist")
xlsx.conditional_format(wb, "Sheet1", "B2:B5", "greater_than", 50)
xlsx.conditional_format(wb, "Sheet1", "C2:C5", "between", -40, -10, fill="#DDEEFF", bold=true)
xlsx.conditional_format(wb, "Sheet1", "A1:C1", "contains", "Z", color="red")
xlsx.color_scale(wb, "Sheet1", "A2:A5", mid="#FFEB84")
xlsx.add_validation(wb, "Sheet1", "H2:H10", "list", values=["pass", "fail"], prompt="Result?")
xlsx.add_validation(wb, "Sheet1", "I2:I10", "whole", min=0, max=100, error="0-100 only", error_style="warning")
xlsx.add_validation(wb, "Sheet1", "J2:J10", "list", source="A2:A5")
xlsx.add_validation(wb, "Sheet1", "K2:K10", "custom", formula="=K2>A2")
xlsx.save_as(wb, "{a}")
w = xlsx.open("{a}")
hf = xlsx.formula(w, "Sheet1", "D3")
head = xlsx.get_cell(w, "Sheet1", "D1")
xlsx.set_cell(w, "Sheet1", "B2", 111)
xlsx.save_as(w, "{b}")
back = xlsx.read("{b}").Zre[0]
"##
    ));
    assert_eq!(text(&it, "helper"), "D2:D5");
    assert_eq!(text(&it, "hf"), "-C3");
    assert_eq!(text(&it, "head"), "-Zim");
    assert_eq!(num(&it, "back"), 111.0);
    let pkg = qu_ooxml::Package::open(&a).unwrap();
    let charts: Vec<String> = pkg.names().into_iter().filter(|n| n.starts_with("xl/charts/")).collect();
    assert_eq!(charts.len(), 3);
    let c1 = pkg.get_str("xl/charts/chart1.xml").unwrap();
    assert!(c1.contains("<c:logBase val=\"10\"/>") && c1.contains("Sheet1!$C$2:$C$5") && c1.contains("<c:v>Z''</c:v>"));
    assert!(pkg.get_str("xl/charts/chart2.xml").unwrap().contains("<a:srgbClr val=\"008080\"/>"), "colour names resolve");
    let ws = pkg.get_str("xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(ws.matches("<conditionalFormatting").count(), 4);
    assert_eq!(ws.matches("<dataValidation ").count(), 4);
    assert!(ws.contains("<formula1>\"pass,fail\"</formula1>") && ws.contains("<formula1>K2&gt;A2</formula1>"));
    // The later cell edit went through the model; the charts came along.
    let edited = qu_ooxml::Package::open(&b).unwrap();
    assert_eq!(edited.names().into_iter().filter(|n| n.starts_with("xl/charts/")).count(), 3);
}

#[test]
fn xlsx_chart_arguments_are_checked() {
    let pre = "import xlsx\nw = xlsx.new()\nxlsx.set_range(w, \"Sheet1\", \"A1\", table(a=[1, 3], b=[2, 4]))\n";
    assert!(err(&format!("{pre}xlsx.add_chart(w, \"Sheet1\", \"pie\", \"B2:B3\")")).contains("scatter, line, bar"));
    assert!(err(&format!("{pre}xlsx.add_chart(w, \"Sheet1\", \"line\", \"B2:B3\", colour=\"red\")")).contains("colour"), "unread keywords are errors");
    let m = err(&format!("{pre}xlsx.add_nyquist_chart(w, \"Sheet1\", \"A2:A3\", \"B2:B3\", x_min=0)"));
    assert!(m.contains("x_min"), "a Nyquist chart sets its own limits: {m}");
    assert!(err(&format!("{pre}xlsx.add_chart(w, \"Sheet1\", \"scatter\", \"B2:B3\", width=3 s)")).contains("length"));
    assert!(err(&format!("{pre}xlsx.conditional_format(w, \"Sheet1\", \"A1:B2\", \"bigger\", 1)")).contains("greater_than"));
    assert!(err(&format!("{pre}xlsx.conditional_format(w, \"Sheet1\", \"A1:B2\", \"greater_than\", 1, fill=\"notacolour\")")).contains("fill="));
    assert!(err(&format!("{pre}xlsx.add_validation(w, \"Sheet1\", \"A1\", \"list\", values=[\"a\"], min=1)")).contains("min"), "min= means nothing to a list");
    assert!(err(&format!("{pre}xlsx.add_validation(w, \"Sheet1\", \"A1\", \"list\")")).contains("values="));
    // Editing in memory is allowed in the sandbox; only writing the file is not.
    let mut it = Interp::new();
    it.set_sandboxed(true);
    it.run(&format!("{pre}xlsx.add_chart(w, \"Sheet1\", \"line\", \"B2:B3\")\nxlsx.add_validation(w, \"Sheet1\", \"C1\", \"whole\", min=0)")).unwrap();
    let msg = it.run(&format!("xlsx.save_as(w, \"{}\")", tmp("sb.xlsx"))).unwrap_err().to_string();
    assert!(msg.contains("save_as"), "{msg}");
}

#[test]
fn handles_and_arguments_are_checked() {
    assert!(err("import docx\nd = docx.new()\ndocx.discard(d)\ndocx.full_text(d)").contains("discarded"));
    assert!(err("import docx\nimport pptx\np = pptx.new()\ndocx.full_text(p)").contains("docx.open/docx.new document"));
    assert!(err("import docx\nd = docx.new()\ndocx.add_heading(d, \"x\", 12)").contains("Heading 1-9"));
    assert!(err("import docx\nd = docx.new()\ndocx.add_image(d, \"x.png\", width=3 s)").contains("length"));
    assert!(err("import xlsx\nw = xlsx.new()\nxlsx.get_cell(w, \"Nope\", \"A1\")").contains("Sheet1"));
    assert!(err("import xlsx\nw = xlsx.new()\nxlsx.get_cell(w, \"Sheet1\", \"1A\")").contains("cell address"));
    assert!(err("import pptx\np = pptx.new()\npptx.add_slide(p, layout=\"Nope\")").contains("Title and Content"));
    // Importing an Office module must not break the builtins its names
    // would otherwise shadow.
    let it = run("import xlsx\nimport docx\nimport pptx\nd = dict()\nd = set(d, \"k\", 1)\nv = get(d, \"k\")");
    assert_eq!(num(&it, "v"), 1.0);
}

#[test]
fn saving_and_converting_are_refused_in_the_sandbox() {
    let mut it = Interp::new();
    it.set_sandboxed(true);
    let msg = it.run(&format!("import docx\nd = docx.new()\ndocx.save_as(d, \"{}\")", tmp("s.docx"))).unwrap_err().to_string();
    assert!(msg.contains("save_as"), "{msg}");
}

#[test]
fn to_pdf_uses_libreoffice_or_says_it_is_missing() {
    let out = tmp("o.pdf");
    let mut it = Interp::new();
    let r = it.run(&format!("import docx\nd = docx.new()\ndocx.add_paragraph(d, \"hello\")\nn = docx.to_pdf(d, \"{out}\")"));
    match qu_ooxml_present() {
        true => {
            r.unwrap();
            assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
        }
        false => assert!(r.unwrap_err().to_string().contains("LibreOffice")),
    }
}

fn qu_ooxml_present() -> bool {
    qu_ooxml::find_office().is_some()
}
