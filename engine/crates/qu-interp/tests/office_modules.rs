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
    ["soffice", "libreoffice"].iter().any(|b| std::process::Command::new(b).arg("--version").output().is_ok_and(|o| o.status.success()))
        || std::env::var("QU_SOFFICE").is_ok()
}
