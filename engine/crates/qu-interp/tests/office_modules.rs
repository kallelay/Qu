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
fn pptx_charts_and_shapes_through_the_language() {
    let (a, pdf) = (tmp("charts.pptx"), tmp("charts.pdf"));
    let it = run(&format!(
        r##"
import pptx
p = pptx.new()
pptx.add_slide(p, layout="Title Only", title="Impedance")
t = linspace(0, pi, 21)
re = 60 - 50*cos(t)
im = -50*sin(t)
d = table(f=[1, 10, 100, 1000], z=[100, 80, 40, 30])
c1 = pptx.add_nyquist_chart(p, 0, re, im, title="Nyquist", at=[15 mm, 45 mm], width=150, height=110)
c2 = pptx.add_chart(p, 0, "scatter", d.z, x=d.f, x_log=true, title="|Z|", x_title="f / Hz", y_title="|Z| / ohm", at=[175, 45], width=150, height=70)
c3 = pptx.add_chart(p, 0, "bar", [30, 70, 12], x=["Rs", "Rct", "W"], names="fit", colors="red", at=[175, 120], width=150, height=55)
c4 = pptx.add_chart(p, 0, "line", ([1, 2, 3], [3, 2, 1]), names=["a", "b"], shape_name="two lines", at=[15, 160], width=60, height=30)
b = pptx.add_shape(p, 0, "rounded_rect", x=15, y=165, w=40, h=14, fill="#1F77B4", text="Rs", bold=true, radius=0.3, name="Rs box")
ar = pptx.add_shape(p, 0, "arrow", x=57, y=172, x2=85, y2=172, stroke="red", stroke_width=2, dash="dash")
e = pptx.add_shape(p, 0, "ellipse", x=88, y=163, w=30, h=18, fill="none", stroke="#D62728", text="CPE", size=14)
pptx.rotate_shape(p, 0, e, 20)
pptx.align_shapes(p, 0, [b, e], "middle")
pptx.align_shapes(p, 0, [c2], "right", to="slide")
pptx.align_shapes(p, 0, ["Rs box"], "left", to="slide")
pptx.save_as(p, "{a}")
q = pptx.open("{a}")
sh = pptx.shapes(q, 0)
n = len(sh)
"##
    ));
    assert_eq!(num(&it, "n"), 1.0 + 4.0 + 3.0, "title + 4 charts + 3 shapes");
    // Fresh ids, in call order.
    let ids: Vec<f64> = ["c1", "c2", "c3", "c4", "b", "ar", "e"].iter().map(|k| num(&it, k)).collect();
    assert!(ids.windows(2).all(|w| w[1] == w[0] + 1.0), "{ids:?}");

    let q = qu_pptx::Presentation::open(&a).unwrap();
    let shapes = q.shapes(0).unwrap();
    let kinds: Vec<&str> = shapes.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["placeholder", "chart", "chart", "chart", "chart", "shape", "connector", "shape"]);
    assert_eq!(shapes[4].name, "two lines");
    assert_eq!(shapes[5].name, "Rs box");
    // align right to the slide: chart 2's right edge at the slide's (338.67 mm).
    let c2 = shapes[2].rect.unwrap();
    assert!((c2.x + c2.w - 338.667).abs() < 0.01, "{c2:?}");
    // align left to the slide by NAME.
    assert!(shapes[5].rect.unwrap().x.abs() < 1e-6);
    // `middle`: the Rs box and the (rotated) ellipse share a centre line.
    let (rb, re_) = (shapes[5].rect.unwrap(), shapes[7].rect.unwrap());
    assert!(((rb.y + rb.h / 2.0) - (re_.y + re_.h / 2.0)).abs() < 0.01);
    // The arrow is flat: red, 2 pt, dashed, with a head.
    let slide = qu_ooxml::Package::open(&a).unwrap().get_str("ppt/slides/slide1.xml").unwrap();
    assert!(slide.contains("prst=\"straightConnector1\"") && slide.contains("w=\"25400\"") && slide.contains("prstDash val=\"dash\"") && slide.contains("tailEnd type=\"triangle\""));
    assert!(slide.contains("FF0000"), "stroke=\"red\" resolved to hex");

    if qu_ooxml_present() {
        run(&format!("import pptx\nq = pptx.open(\"{a}\")\npptx.to_pdf(q, \"{pdf}\")"));
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF") && bytes.len() > 20_000, "{} bytes", bytes.len());
    } else {
        eprintln!("SKIP pdf render: LibreOffice not found");
    }
}

#[test]
fn pptx_nyquist_takes_one_complex_vector() {
    let it = run(
        r##"
import pptx
p = pptx.new()
pptx.add_slide(p, layout="Blank")
t = linspace(0, pi, 21)
Z = (60 - 50*cos(t)) + (-50*sin(t))*1i
id = pptx.add_nyquist_chart(p, 0, Z, color="blue", name="cell")
plain = pptx.add_nyquist_chart(p, 0, 60 - 50*cos(t), 50*sin(t), negate=false, shape_name="plain")
"##,
    );
    assert_eq!(num(&it, "id"), 2.0);
    assert_eq!(num(&it, "plain"), 3.0);
}

#[test]
fn pptx_chart_and_shape_arguments_are_checked() {
    let base = "import pptx\np = pptx.new()\npptx.add_slide(p, layout=\"Blank\")\n";
    let e = |s: &str| err(&format!("{base}{s}"));
    assert!(e("pptx.add_chart(p, 0, \"pie\", [1, 2])").contains("scatter"));
    assert!(e("pptx.add_chart(p, 0, \"scatter\", [1, 2, 3], x=[1, 2])").contains("x values"));
    assert!(e("pptx.add_chart(p, 0, \"scatter\", [1, 2, 3], titel=\"x\")").contains("titel"), "a misspelt keyword is unread");
    assert!(e("pptx.add_chart(p, 0, \"line\", [1, 2, 3], x_log=true)").contains("scatter"));
    assert!(e("pptx.add_chart(p, 0, \"line\", [1, 2, 3], width=5)").contains("20 to 2000"));
    assert!(e("pptx.add_chart(p, 0, \"line\", [1, 2, 3], at=[1, 2, 3])").contains("[x, y]"));
    assert!(e("pptx.add_chart(p, 0, \"line\", \"abc\")").contains("vector of numbers"));
    assert!(e("pptx.add_chart(p, 3, \"line\", [1, 2])").contains("does not exist"));
    assert!(e("pptx.add_chart(p, 0, \"line\", [1, 2], colors=\"nope\")").contains("colors="));
    assert!(e("pptx.add_nyquist_chart(p, 0, [1, 2, 3])").contains("Z'"));
    assert!(e("pptx.add_nyquist_chart(p, 0, [1, 2, 3], [1, 2])").contains("Z''"));
    assert!(e("pptx.add_nyquist_chart(p, 0, [1, 2, 3], [1, 2, 3], x_log=true)").contains("x_log"), "a Nyquist chart sets its own axes");
    assert!(e("pptx.add_shape(p, 0, \"hexagon\", x=0, y=0, w=1, h=1)").contains("rounded_rect"));
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5)").contains("h= is required"));
    assert!(e("pptx.add_shape(p, 0, \"line\", x=0, y=0, w=5, h=5)").contains("x2="));
    assert!(e("pptx.add_shape(p, 0, \"line\", x=0, y=0, x2=5, y2=5, fill=\"red\")").contains("fill"), "a line has no fill: unread");
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5, size=12)").contains("size"), "size= without text= is unread");
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5, radius=0.2)").contains("radius"));
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5, fill=\"notacolour\")").contains("fill="));
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=3 s, y=0, w=5, h=5)").contains("length"));
    assert!(e("pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5, dash=\"wavy\")").contains("dash"));
    assert!(e("pptx.rotate_shape(p, 0, 99, 10)").contains("no shape id 99"));
    assert!(e("pptx.rotate_shape(p, 0, 2)").contains("angle"));
    assert!(e("c = pptx.add_chart(p, 0, \"line\", [1, 2])\npptx.rotate_shape(p, 0, c, 10)").contains("cannot be rotated"));
    assert!(e("a = pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5)\npptx.align_shapes(p, 0, [a], \"left\")").contains("two or more"));
    assert!(e("a = pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5)\npptx.align_shapes(p, 0, [a], \"left\", to=\"page\")").contains("slide"));
    assert!(e("a = pptx.add_shape(p, 0, \"rect\", x=0, y=0, w=5, h=5)\npptx.align_shapes(p, 0, [a], \"diagonal\", to=\"slide\")").contains("middle"));
    // A refused call leaves the deck without the half-made chart.
    let it = run(&format!("{base}try\n  pptx.add_chart(p, 0, \"line\", [1, 2], y_log=true, y_min=-1)\ncatch ex\n  msg = ex.message\nend\nn = len(pptx.shapes(p, 0))"));
    assert_eq!(num(&it, "n"), 0.0);
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

/// Sheet operations, Tables, notes and pictures through the language; the
/// package-level known answers live in qu-xlsx's `sheet_ops` tests. A chart
/// is saved first and every operation then runs on the reopened file.
#[test]
fn xlsx_sheet_ops_tables_notes_and_pictures() {
    let (a, b, img) = (tmp("so_a.xlsx"), tmp("so_b.xlsx"), tmp("so.png"));
    png(&img);
    let it = run(&format!(
        r##"
import xlsx
wb = xlsx.new()
xlsx.set_range(wb, "Sheet1", "A1", table(name=["delta", "Alpha", "charlie", "echo"], value=[3, 10, 1, 7]))
xlsx.add_chart(wb, "Sheet1", "bar", "B2:B5", x="A2:A5")
xlsx.add_sheet(wb, "Notes")
xlsx.save_as(wb, "{a}")
w = xlsx.open("{a}")
xlsx.sort_range(w, "Sheet1", "A1:B5", by="B", desc=true, header=true)
first = xlsx.get_cell(w, "Sheet1", "A2")
last = xlsx.get_cell(w, "Sheet1", "A5")
n_moved = xlsx.move_range(w, "Sheet1", "A1:B5", "D1", to_sheet="Notes", copy=true)
n_cleared = xlsx.clear(w, "Notes", "E2:E3", what="contents")
xlsx.create_table(w, "Sheet1", "A1:B5", "Readings", style="light9", total="average")
avg = xlsx.formula(w, "Sheet1", "B6")
xlsx.autofilter(w, "Notes", "D1:E5")
xlsx.add_comment(w, "Sheet1", "B2", "the largest reading", author="Ahmed")
xlsx.add_image(w, "Notes", "{img}", at="H2", width=40 mm, alt="four by two")
xlsx.copy_sheet(w, "Sheet1", "Sheet1 copy")
xlsx.hide_sheet(w, "Notes", state="very_hidden")
xlsx.hide_sheet(w, "Sheet1 copy")
xlsx.save_as(w, "{b}")
names = xlsx.sheets("{b}")
"##
    ));
    assert_eq!(text(&it, "first"), "Alpha");
    assert_eq!(text(&it, "last"), "charlie");
    assert_eq!(num(&it, "n_moved"), 10.0);
    assert_eq!(num(&it, "n_cleared"), 2.0);
    assert_eq!(text(&it, "avg"), "SUBTOTAL(101,B2:B5)");
    let pkg = qu_ooxml::Package::open(&b).unwrap();
    let t = pkg.names().into_iter().filter(|n| n.starts_with("xl/tables/")).collect::<Vec<_>>();
    assert_eq!(t.len(), 2, "the table and its copy: {:?}", pkg.names());
    assert!(pkg.names().iter().any(|n| n.starts_with("xl/comments")) && pkg.names().iter().any(|n| n.starts_with("xl/media/image1.png")));
    let charts = pkg.names().into_iter().filter(|n| n.starts_with("xl/charts/chart")).count();
    assert_eq!(charts, 2, "the chart saved earlier, and the copied sheet's");
    let wbx = pkg.get_str("xl/workbook.xml").unwrap();
    assert!(wbx.contains("veryHidden") && wbx.contains("state=\"hidden\""), "{wbx}");
}

#[test]
fn xlsx_sheet_ops_check_their_arguments() {
    let pre = "import xlsx\nw = xlsx.new()\nxlsx.set_range(w, \"Sheet1\", \"A1\", table(a=[3, 1, 2], b=[1, 2, 3]))\n";
    for (call, needle) in [
        ("xlsx.sort_range(w, \"Sheet1\", \"A1:B3\")", "column to sort by"),
        ("xlsx.sort_range(w, \"Sheet1\", \"A1:B3\", by=\"A\", descending=true)", "descending"),
        ("xlsx.sort_range(w, \"Sheet1\", \"A1:B3\", by=[\"A\", \"B\"], desc=[true, false, true])", "2 sort keys"),
        ("xlsx.sort_range(w, \"Sheet1\", \"A1:B3\", by=0)", "column letter"),
        ("xlsx.clear(w, \"Sheet1\", \"A1\", what=\"everything\")", "all, contents or formats"),
        ("xlsx.move_range(w, \"Sheet1\", \"A1:B3\", \"A1\")", "already there"),
        ("xlsx.hide_sheet(w, \"Sheet1\")", "at least one visible"),
        ("xlsx.hide_sheet(w, \"Sheet1\", state=\"gone\")", "hidden, very_hidden or visible"),
        ("xlsx.copy_sheet(w, \"Sheet1\", \"Sheet1\")", "already has a sheet"),
        ("xlsx.copy_sheet(w, \"Sheet1\", \"a/b\")", "does not allow"),
        ("xlsx.autofilter(w, \"Sheet1\", \"A1\")", "single cell"),
        ("xlsx.create_table(w, \"Sheet1\", \"A1:B3\", \"my table\")", "no spaces"),
        ("xlsx.create_table(w, \"Sheet1\", \"A1:B3\", \"T\", total=\"median\")", "sum, average"),
        ("xlsx.create_table(w, \"Sheet1\", \"A1:B3\", \"Tbl\", colour=\"red\")", "colour"),
        ("xlsx.add_comment(w, \"Sheet1\", \"A1\", \"\")", "empty"),
        ("xlsx.add_comment(w, \"Sheet1\", \"A1\", \"x\", writer=\"me\")", "writer"),
        ("xlsx.add_image(w, \"Sheet1\", \"no_such_file.png\")", "could not read"),
        ("xlsx.add_image(w, \"Sheet1\")", "image path"),
    ] {
        let mut it = Interp::new();
        let msg = it.run(&format!("{pre}{call}")).err().unwrap_or_else(|| panic!("{call} should have failed")).to_string();
        assert!(msg.contains(needle), "{call}\n  expected `{needle}`, got: {msg}");
    }
    // Editing in memory is allowed in the sandbox; only writing the file is not.
    let mut it = Interp::new();
    it.set_sandboxed(true);
    it.run(&format!("{pre}xlsx.sort_range(w, \"Sheet1\", \"A1:B3\", by=\"A\", header=true)\nxlsx.create_table(w, \"Sheet1\", \"A1:B3\", \"Tbl\")\nxlsx.add_comment(w, \"Sheet1\", \"A1\", \"hi\")\nxlsx.copy_sheet(w, \"Sheet1\", \"Two\")")).unwrap();
    let msg = it.run(&format!("xlsx.save_as(w, \"{}\")", tmp("sb2.xlsx"))).unwrap_err().to_string();
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

#[test]
fn docx_edit_tables_formatting_notes_and_revisions() {
    let (a, b) = (tmp("e1.docx"), tmp("e2.docx"));
    let it = run(&format!(
        r##"
import docx
doc = docx.new()
docx.add_paragraph(doc, "Revenue rose in Q3 across all regions.")
docx.add_paragraph(doc, "Second paragraph")
docx.add_table(doc, [["Region", "Q2", "Q3"], ["EU", "1", "2"], ["US", "3", "4"]], header=true)
docx.add_row(doc, 0, ["APAC", 5, 6])
docx.add_column(doc, 0, ["Q4", 7, 8, 9], at=3)
docx.merge_cells(doc, 0, 1, 1, 1, 2)
merged = docx.tables(doc)[0][1][1]
merged_cells = len(docx.tables(doc)[0][1])
docx.split_cell(doc, 0, 1, 1)
split_cells = len(docx.tables(doc)[0][1])
split_text = docx.tables(doc)[0][1][1]
docx.split_cell(doc, 0, 2, 3, cols=2)
cut_cells = len(docx.tables(doc)[0][2])
spans = docx.format_text(doc, 0, bold=true, color="#c00000", size=13, on="Q3")
docx.format_text(doc, 0, italic=true, underline=true, font="Georgia")
n_lines = docx.line_spacing(doc, 0, to=1, lines=1.5, after=6)
docx.move_paragraph(doc, 1, 0)
docx.add_footnote(doc, 1, "Source: ministry of finance.", on="Revenue")
docx.track_insert(doc, 1, "strongly ", before="rose", author="Ahmed", date="2026-10-01T09:00:00Z")
ndel = docx.track_delete(doc, 1, on=" across all regions", author="Ahmed", date="2026-10-01T09:01:00Z")
docx.track_delete(doc, 0, author="Ahmed")
docx.save_as(doc, "{a}")
d = docx.open("{a}")
paras = docx.paragraphs(d)
p0 = paras[0]
p1 = paras[1]
notes = docx.footnotes(d)
rows = len(docx.tables(d)[0])
cell = docx.tables(d)[0][3][3]
nacc = docx.accept_changes(d)
after = docx.paragraphs(d)
na = len(after)
p_acc = after[0]
docx.save_as(d, "{b}")
"##
    ));
    assert_eq!(text(&it, "merged"), "1\n2", "merged cell texts follow one paragraph each");
    assert_eq!((num(&it, "merged_cells"), num(&it, "split_cells"), num(&it, "cut_cells")), (3.0, 4.0, 5.0));
    assert_eq!(text(&it, "split_text"), "1\n2", "split keeps the text in the top-left cell");
    assert_eq!(num(&it, "spans"), 1.0);
    assert_eq!(num(&it, "n_lines"), 2.0);
    assert_eq!(num(&it, "ndel"), 1.0);
    assert_eq!(text(&it, "p0"), "", "a tracked paragraph deletion hides the paragraph's text");
    assert_eq!(text(&it, "p1"), "Revenue strongly rose in Q3.");
    assert_eq!(it.get("notes").map(|v| format!("{v:?}")).unwrap().contains("Source: ministry of finance."), true);
    assert_eq!(num(&it, "rows"), 4.0);
    assert_eq!(text(&it, "cell"), "9");
    assert_eq!(num(&it, "nacc"), 4.0, "two insertion/deletion marks, the paragraph deletion and its mark's text");
    assert_eq!(num(&it, "na"), 2.0, "the deleted paragraph is gone after accepting; what is left is the text and the empty paragraph after the table");
    assert_eq!(text(&it, "p_acc"), "Revenue strongly rose in Q3.");
}

#[test]
fn docx_edit_arguments_are_checked() {
    let pre = "import docx\nd = docx.new()\ndocx.add_paragraph(d, \"hello world\")\ndocx.add_table(d, [[\"a\", \"b\"], [\"c\", \"d\"]])\n";
    let bad = |tail: &str| err(&format!("{pre}{tail}"));
    assert!(bad("docx.add_row(d, 0, bogus=1)").contains("bogus"), "unread keyword is an error");
    assert!(bad("docx.add_row(d, 0, [1, 2, 3])").contains("3 values for a row of 2"));
    assert!(bad("docx.add_row(d, 0, \"x\")").contains("list"));
    assert!(bad("docx.add_row(d, 3)").contains("table 3"));
    assert!(bad("docx.add_column(d, 0, at=7)").contains("cannot insert at column 7"));
    assert!(bad("docx.merge_cells(d, 0, 0, 0, 0, 0)").contains("single cell"));
    assert!(bad("docx.merge_cells(d, 0, 0, 0, 5, 5)").contains("does not exist"));
    assert!(bad("docx.merge_cells(d, 0, 0, 0, 1)").contains("missing"));
    assert!(bad("docx.split_cell(d, 0, 0, 0)").contains("not merged"));
    assert!(bad("docx.format_text(d, 0)").contains("nothing to apply"));
    assert!(bad("docx.format_text(d, 0, bold=true, size=\"big\")").contains("takes a number"));
    assert!(bad("docx.format_text(d, 0, bold=true, color=\"red\")").contains("#rrggbb"));
    assert!(bad("docx.format_text(d, 0, bold=true, on=\"absent\")").contains("does not contain"));
    assert!(bad("docx.format_text(d, 0, bold=true, bolt=true)").contains("bolt"));
    assert!(bad("docx.line_spacing(d, 0)").contains("nothing to set"));
    assert!(bad("docx.line_spacing(d, 0, lines=2, exactly=20)").contains("not several"));
    assert!(bad("docx.move_paragraph(d, 0, 5)").contains("cannot move"));
    assert!(bad("docx.add_footnote(d, 0, \"\")").contains("empty"));
    assert!(bad("docx.track_insert(d, 0, \"x\", after=\"hello\", before=\"world\")").contains("not both"));
    assert!(bad("docx.track_delete(d, 0, on=\"zzz\")").contains("does not contain"));
    assert!(bad("docx.track_delete(d, 0, author=3)").contains("string"));
}

#[test]
fn docx_edits_render_in_libreoffice_when_it_is_installed() {
    let Some(_) = qu_ooxml::find_office() else {
        eprintln!("skipping: no LibreOffice on this machine");
        return;
    };
    let out = tmp("edited.pdf");
    let it = run(&format!(
        r##"
import docx
doc = docx.new()
docx.add_paragraph(doc, "Revenue rose in Q3 across all regions.")
docx.add_table(doc, [["Region", "Q2"], ["EU", "1"]], header=true)
docx.add_row(doc, 0, ["US", 2])
docx.add_column(doc, 0, ["Q3", 3, 4])
docx.merge_cells(doc, 0, 1, 0, 1, 1)
docx.format_text(doc, 0, bold=true, on="Q3")
docx.add_footnote(doc, 0, "Source: ministry of finance.", on="Revenue")
docx.track_insert(doc, 0, "strongly ", before="rose", author="Ahmed")
n = docx.to_pdf(doc, "{out}")
"##
    ));
    assert!(num(&it, "n") > 1000.0);
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

fn qu_ooxml_present() -> bool {
    qu_ooxml::find_office().is_some()
}
