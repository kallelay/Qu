//! Tests for chart and shape creation: what changes in the package, what
//! the XML says, and -- where the tools exist -- what python-pptx reads
//! back and LibreOffice renders. Tests that need those tools skip, with a
//! note on stderr, when they are absent.

use crate::*;
use qu_ooxml::xml;

const CUSTOM: &str = "customXml/item1.xml";
const CUSTOM_BYTES: &[u8] = b"<?xml version=\"1.0\"?><root xmlns=\"urn:vendor\"><keep>me &amp; mine</keep></root>";

fn reopen(p: &mut Presentation) -> Presentation {
    Presentation::from_bytes(&p.to_bytes().unwrap()).unwrap()
}

/// Three slides plus an unknown customXml part, reopened ("someone's file").
fn deck() -> Presentation {
    let mut p = Presentation::new();
    p.add_slide(Some("Title Only"), Some("Results"), &[], None).unwrap();
    p.add_slide(Some("Blank"), None, &[], None).unwrap();
    p.add_slide(Some("Blank"), None, &[], None).unwrap();
    p.pkg.set(CUSTOM, CUSTOM_BYTES.to_vec());
    reopen(&mut p)
}

fn snapshot(p: &Presentation) -> Vec<(String, Vec<u8>)> {
    p.pkg.names().into_iter().map(|n| (n.clone(), p.pkg.get(&n).unwrap().to_vec())).collect()
}

fn changed(a: &[(String, Vec<u8>)], b: &[(String, Vec<u8>)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (n, bytes) in b {
        if a.iter().find(|(m, _)| m == n).map(|(_, x)| x) != Some(bytes) {
            out.push(n.clone());
        }
    }
    for (n, _) in a {
        if !b.iter().any(|(m, _)| m == n) {
            out.push(format!("-{n}"));
        }
    }
    out.sort();
    out
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

fn semicircle() -> (Vec<f64>, Vec<f64>) {
    // Z' = 60 - 50 cos t, Z'' = -50 sin t  (a capacitive arc, Z'' negative)
    let t: Vec<f64> = (0..=20).map(|i| i as f64 * std::f64::consts::PI / 20.0).collect();
    (t.iter().map(|t| 60.0 - 50.0 * t.cos()).collect(), t.iter().map(|t| -50.0 * t.sin()).collect())
}

fn chart_xml(p: &Presentation, n: usize) -> xml::Doc {
    xml::parse(&p.pkg.get_str(&format!("ppt/charts/chart{n}.xml")).unwrap()).unwrap()
}

#[test]
fn add_chart_touches_only_the_slide_its_rels_the_content_types_and_the_chart() {
    let mut p = deck();
    let before = snapshot(&p);
    let f = vec![1.0, 10.0, 100.0, 1000.0];
    let z = vec![100.0, 80.0, 40.0, 30.0];
    let id = p
        .add_chart(
            1,
            ChartKind::Scatter,
            &[ChartX::Numbers(f)],
            &[z],
            &ChartOptions { title: Some("|Z| vs f".into()), x_title: Some("f / Hz".into()), y_title: Some("|Z| / ohm".into()), x_log: true, at: Some((20.0, 30.0)), width_mm: Some(150.0), height_mm: Some(90.0), ..Default::default() },
        )
        .unwrap();
    let mut q = reopen(&mut p);
    assert_eq!(changed(&before, &snapshot(&q)), ["[Content_Types].xml", "ppt/charts/chart1.xml", "ppt/slides/_rels/slide2.xml.rels", "ppt/slides/slide2.xml"]);

    let shapes = q.shapes(1).unwrap();
    let ch = shapes.iter().find(|s| s.kind == "chart").expect("the slide lists a chart");
    assert_eq!(ch.id, id);
    let r = ch.rect.unwrap();
    assert!((r.x - 20.0).abs() < 1e-3 && (r.y - 30.0).abs() < 1e-3 && (r.w - 150.0).abs() < 1e-3 && (r.h - 90.0).abs() < 1e-3);

    let d = chart_xml(&q, 1);
    assert_eq!(d.root.find_all("c:scatterChart").len(), 1);
    assert_eq!(d.root.find_all("c:numLit").len(), 2, "x and y are cached literals");
    assert_eq!(d.root.find_all("c:logBase").len(), 1);
    assert!(q.pkg.get_str("ppt/charts/chart1.xml").unwrap().contains("|Z| vs f"));
    // The slide's relationship, the override and the frame agree.
    let rels = q.pkg.rels("ppt/slides/slide2.xml").unwrap();
    let rel = rels.iter().find(|r| r.rel_type.ends_with("/chart")).expect("chart relationship");
    assert_eq!(rel.target, "../charts/chart1.xml");
    assert_eq!(q.pkg.override_type("ppt/charts/chart1.xml").as_deref(), Some(qu_ooxml::chart::CT_CHART));
    let slide = q.pkg.get_xml("ppt/slides/slide2.xml").unwrap();
    assert_eq!(slide.root.find_all("c:chart")[0].attr("r:id"), Some(rel.id.as_str()));

    // A second chart gets its own part and relationship; the first stays put.
    let first = q.pkg.get("ppt/charts/chart1.xml").unwrap().to_vec();
    q.add_chart(2, ChartKind::Column, &[ChartX::Labels(vec!["a".into(), "b".into()])], &[vec![1.0, 2.0]], &ChartOptions::default()).unwrap();
    assert_eq!(q.pkg.get("ppt/charts/chart1.xml").unwrap(), first.as_slice());
    assert!(q.pkg.has("ppt/charts/chart2.xml"));
    assert_eq!(q.pkg.get(CUSTOM).unwrap(), CUSTOM_BYTES);
}

#[test]
fn chart_kinds_series_and_options_reach_the_xml() {
    let mut p = deck();
    // Several series, one shared x, names and colours, a bar chart's text categories.
    p.add_chart(
        1,
        ChartKind::Line,
        &[ChartX::Labels(vec!["Q1".into(), "Q2".into(), "Q3".into()])],
        &[vec![1.0, 2.0, 3.0], vec![3.0, 2.0, 1.0]],
        &ChartOptions { names: vec!["a".into(), "b".into()], colors: vec!["#ff0000".into(), "00ff00".into()], y_min: Some(0.0), y_max: Some(4.0), ..Default::default() },
    )
    .unwrap();
    p.add_chart(1, ChartKind::Column, &[], &[vec![5.0, 6.0]], &ChartOptions::default()).unwrap();
    let q = reopen(&mut p);
    let d = chart_xml(&q, 1);
    assert_eq!(d.root.find_all("c:lineChart").len(), 1);
    assert_eq!(d.root.find_all("c:ser").len(), 2);
    assert_eq!(d.root.find_all("c:strLit").len(), 2, "categories on both series");
    let cols: Vec<&str> = d.root.find_all("a:srgbClr").iter().filter_map(|c| c.attr("val")).collect();
    assert!(cols.contains(&"FF0000") && cols.contains(&"00FF00"));
    assert_eq!(d.root.find_all("c:legend").len(), 1, "two series show a legend");
    let d2 = chart_xml(&q, 2);
    assert_eq!(d2.root.find_all("c:barDir")[0].attr("val"), Some("col"));
    assert_eq!(d2.root.find_all("c:legend").len(), 0, "one unnamed series has no legend");
}

#[test]
fn nyquist_chart_has_equal_axis_scale_and_plots_minus_z_imag() {
    let mut p = deck();
    let (re, im) = semicircle();
    p.add_nyquist_chart(1, &re, &im, true, &ChartOptions { width_mm: Some(150.0), height_mm: Some(110.0), ..Default::default() }).unwrap();
    let q = reopen(&mut p);
    let d = chart_xml(&q, 1);
    let ax = d.root.find_all("c:valAx");
    assert_eq!(ax.len(), 2);
    let get = |a: &xml::Element, tag: &str| -> f64 { a.child("c:scaling").unwrap().child(tag).unwrap().attr("val").unwrap().parse().unwrap() };
    let (x0, x1, y0, y1) = (get(ax[0], "c:min"), get(ax[0], "c:max"), get(ax[1], "c:min"), get(ax[1], "c:max"));
    // Known answer (see qu-ooxml's equal_scale test): the same data fit [0,120] x [0,60].
    assert_eq!((x0, x1, y0, y1), (0.0, 120.0, 0.0, 60.0));
    // One data unit is the same length on the page along both axes.
    let pa = d.root.find_all("c:manualLayout")[0];
    let frac = |t: &str| -> f64 { pa.child(t).unwrap().attr("val").unwrap().parse().unwrap() };
    let (w_mm, h_mm) = (frac("c:w") * 150.0, frac("c:h") * 110.0);
    assert!(((x1 - x0) / w_mm - (y1 - y0) / h_mm).abs() < 1e-9, "x and y are on one scale");
    // The plotted y is -Z'': all positive for this arc, peaking at 50.
    let ys: Vec<f64> = d.root.find_all("c:yVal")[0].find_all("c:v").iter().map(|v| v.text().parse().unwrap()).collect();
    assert!(ys.iter().all(|v| *v > -1e-9));
    assert!((ys.iter().cloned().fold(f64::MIN, f64::max) - 50.0).abs() < 1e-9);
    let xml = q.pkg.get_str("ppt/charts/chart1.xml").unwrap();
    assert!(xml.contains("Z&apos;") || xml.contains("Z'"), "axis titles name Z'");
    // negate=false plots Z'' as given.
    let mut p2 = deck();
    p2.add_nyquist_chart(1, &re, &im, false, &ChartOptions::default()).unwrap();
    let d2 = chart_xml(&p2, 1);
    let ys2: Vec<f64> = d2.root.find_all("c:yVal")[0].find_all("c:v").iter().map(|v| v.text().parse().unwrap()).collect();
    assert!(ys2.iter().any(|v| *v < -49.0));
}

#[test]
fn bad_chart_requests_are_named_errors_and_leave_the_package_alone() {
    let mut p = deck();
    let before = snapshot(&p);
    let o = ChartOptions::default();
    let one = || vec![vec![1.0, 2.0, 3.0]];
    for (what, r) in [
        ("series", p.add_chart(1, ChartKind::Scatter, &[], &[], &o)),
        ("x values but", p.add_chart(1, ChartKind::Scatter, &[ChartX::Numbers(vec![1.0, 2.0])], &one(), &o)),
        ("numeric x", p.add_chart(1, ChartKind::Scatter, &[ChartX::Labels(vec!["a".into(); 3])], &one(), &o)),
        ("names", p.add_chart(1, ChartKind::Line, &[], &one(), &ChartOptions { names: vec!["a".into(), "b".into()], ..Default::default() })),
        ("#rrggbb", p.add_chart(1, ChartKind::Line, &[], &one(), &ChartOptions { colors: vec!["nope".into()], ..Default::default() })),
        ("equal_axes", p.add_chart(1, ChartKind::Line, &[], &one(), &ChartOptions { equal_axes: true, ..Default::default() })),
        ("scatter", p.add_chart(1, ChartKind::Column, &[], &one(), &ChartOptions { x_min: Some(0.0), ..Default::default() })),
        ("not bars", p.add_chart(1, ChartKind::Column, &[], &one(), &ChartOptions { lines: Some(true), ..Default::default() })),
        ("negative", p.add_chart(1, ChartKind::Line, &[], &[vec![1.0, -2.0]], &ChartOptions { y_log: true, ..Default::default() })),
        ("below max", p.add_chart(1, ChartKind::Line, &[], &one(), &ChartOptions { y_min: Some(5.0), y_max: Some(1.0), ..Default::default() })),
        ("20 to 2000", p.add_chart(1, ChartKind::Line, &[], &one(), &ChartOptions { width_mm: Some(5.0), ..Default::default() })),
        ("does not exist", p.add_chart(9, ChartKind::Line, &[], &one(), &o)),
    ] {
        let e = r.expect_err(what);
        assert!(e.contains(what), "`{e}` should mention `{what}`");
    }
    assert!(p.add_nyquist_chart(1, &[1.0, 2.0], &[1.0], true, &o).unwrap_err().contains("Z'"));
    assert_eq!(changed(&before, &snapshot(&p)), Vec::<String>::new(), "no refused call changed the package");
    assert!(chart_kind("pie").unwrap_err().contains("scatter"));
}

fn ids(p: &Presentation, i: usize) -> Vec<(u64, String, String)> {
    p.shapes(i).unwrap().into_iter().map(|s| (s.id, s.kind, s.name)).collect()
}

#[test]
fn add_shape_kinds_fill_stroke_text_and_names() {
    let mut p = deck();
    let before = snapshot(&p);
    let sty = ShapeStyle { fill: Some("#1F77B4".into()), text: Some("Rs\nCPE".into()), text_fmt: TextFormat { size: Some(18.0), bold: true, ..Default::default() }, name: Some("box".into()), ..Default::default() };
    let a = p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(20.0, 20.0, 60.0, 30.0)), &sty).unwrap();
    // A colour name is the interpreter's to resolve; the library takes hex.
    assert!(p.add_shape(1, ShapeKind::Ellipse, Geometry::Frame(rect(0.0, 0.0, 5.0, 5.0)), &ShapeStyle { stroke: Some("red".into()), ..Default::default() }).unwrap_err().contains("#rrggbb"));
    let b = p
        .add_shape(1, ShapeKind::Ellipse, Geometry::Frame(rect(100.0, 20.0, 40.0, 40.0)), &ShapeStyle { fill: Some("none".into()), stroke: Some("#FF0000".into()), stroke_pt: Some(3.0), dash: Some("dash".into()), ..Default::default() })
        .unwrap();
    let c = p.add_shape(1, ShapeKind::RoundedRect, Geometry::Frame(rect(20.0, 70.0, 60.0, 30.0)), &ShapeStyle { radius: Some(0.25), text: Some("R".into()), ..Default::default() }).unwrap();
    let l = p.add_shape(1, ShapeKind::Line, Geometry::Segment { x1: 20.0, y1: 120.0, x2: 100.0, y2: 120.0 }, &ShapeStyle::default()).unwrap();
    let ar = p.add_shape(1, ShapeKind::Arrow, Geometry::Segment { x1: 140.0, y1: 140.0, x2: 100.0, y2: 100.0 }, &ShapeStyle { stroke: Some("#00AA00".into()), ..Default::default() }).unwrap();
    assert_eq!([a, b, c, l, ar], [a, a + 1, a + 2, a + 3, a + 4], "ids are fresh and increasing");

    let mut q = reopen(&mut p);
    assert_eq!(changed(&before, &snapshot(&q)), ["ppt/slides/slide2.xml"], "a shape edit changes the one slide and nothing else");
    let kinds: Vec<String> = ids(&q, 1).into_iter().map(|(_, k, _)| k).collect();
    assert_eq!(kinds, ["shape", "shape", "shape", "connector", "connector"]);
    assert_eq!(ids(&q, 1)[0].2, "box");
    assert!(q.slide_text(1).unwrap().contains("Rs\nCPE"));

    let d = q.pkg.get_xml("ppt/slides/slide2.xml").unwrap();
    let sps = d.root.find_all("p:sp");
    let prst = |sp: &xml::Element| sp.child("p:spPr").unwrap().child("a:prstGeom").unwrap().attr("prst").unwrap().to_string();
    assert_eq!([prst(sps[0]), prst(sps[1]), prst(sps[2])], ["rect", "ellipse", "roundRect"]);
    // Filled rect: its fill, white text on the dark blue, no outline.
    let sp0 = sps[0].child("p:spPr").unwrap();
    assert_eq!(sp0.child("a:solidFill").unwrap().child("a:srgbClr").unwrap().attr("val"), Some("1F77B4"));
    assert!(sp0.child("a:ln").unwrap().child("a:noFill").is_some());
    assert!(q.pkg.get_str("ppt/slides/slide2.xml").unwrap().contains("val=\"FFFFFF\""), "text on a dark fill is white");
    assert_eq!(sps[0].find_all("a:p").len(), 2, "newline = new paragraph");
    // Unfilled ellipse: noFill, dashed 3 pt red outline.
    let sp1 = sps[1].child("p:spPr").unwrap();
    assert!(sp1.child("a:noFill").is_some());
    let ln = sp1.child("a:ln").unwrap();
    assert_eq!(ln.attr("w"), Some("38100"));
    assert_eq!(ln.child("a:prstDash").unwrap().attr("val"), Some("dash"));
    assert_eq!(ln.child("a:solidFill").unwrap().child("a:srgbClr").unwrap().attr("val"), Some("FF0000"));
    // Rounded corner 0.25 -> adj 25000.
    assert_eq!(sps[2].find_all("a:gd")[0].attr("fmla"), Some("val 25000"));
    // Connectors: plain line has no arrowhead, the arrow does and is flipped both ways.
    let cx = d.root.find_all("p:cxnSp");
    assert_eq!(cx.len(), 2);
    assert!(cx[0].find_all("a:tailEnd").is_empty());
    assert_eq!(cx[1].find_all("a:tailEnd")[0].attr("type"), Some("triangle"));
    let x = cx[1].child("p:spPr").unwrap().child("a:xfrm").unwrap();
    assert_eq!((x.attr("flipH"), x.attr("flipV")), (Some("1"), Some("1")));
    let sh = q.shapes(1).unwrap();
    let r = sh[4].rect.unwrap();
    assert!((r.x - 100.0).abs() < 1e-3 && (r.y - 100.0).abs() < 1e-3 && (r.w - 40.0).abs() < 1e-3 && (r.h - 40.0).abs() < 1e-3);
    // The new shapes are ordinary shapes: the existing edits work on them.
    q.set_shape_text(1, &ShapeRef::Id(c), "Rct").unwrap();
    q.delete_shape(1, &ShapeRef::Id(l)).unwrap();
    assert_eq!(q.shapes(1).unwrap().len(), 4);
}

#[test]
fn bad_shape_requests_are_named_errors() {
    let mut p = deck();
    let before = snapshot(&p);
    let r = Geometry::Frame(rect(0.0, 0.0, 10.0, 10.0));
    let seg = Geometry::Segment { x1: 0.0, y1: 0.0, x2: 5.0, y2: 5.0 };
    let d = ShapeStyle::default();
    for (what, res) in [
        ("kind", ShapeKind::parse("hexagon").map(|_| 0)),
        ("not a start and end", p.add_shape(1, ShapeKind::Rect, seg, &d)),
        ("not with w and h", p.add_shape(1, ShapeKind::Line, r, &d)),
        ("no fill", p.add_shape(1, ShapeKind::Line, seg, &ShapeStyle { text: Some("x".into()), ..Default::default() })),
        ("same point", p.add_shape(1, ShapeKind::Line, Geometry::Segment { x1: 1.0, y1: 1.0, x2: 1.0, y2: 1.0 }, &d)),
        ("radius=", p.add_shape(1, ShapeKind::Rect, r, &ShapeStyle { radius: Some(0.2), ..Default::default() })),
        ("0 to 0.5", p.add_shape(1, ShapeKind::RoundedRect, r, &ShapeStyle { radius: Some(0.9), ..Default::default() })),
        ("#rrggbb", p.add_shape(1, ShapeKind::Rect, r, &ShapeStyle { fill: Some("blue".into()), ..Default::default() })),
        ("dash=", p.add_shape(1, ShapeKind::Rect, r, &ShapeStyle { dash: Some("wavy".into()), ..Default::default() })),
        ("stroke_width", p.add_shape(1, ShapeKind::Rect, r, &ShapeStyle { stroke_pt: Some(0.0), ..Default::default() })),
        ("w and h zero or more", p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(0.0, 0.0, -1.0, 5.0)), &d)),
        ("does not exist", p.add_shape(7, ShapeKind::Rect, r, &d)),
    ] {
        let e = res.expect_err(what);
        assert!(e.contains(what), "`{e}` should mention `{what}`");
    }
    assert_eq!(changed(&before, &snapshot(&p)), Vec::<String>::new());
    assert!(Align::parse("diagonal").unwrap_err().contains("middle"));
}

#[test]
fn rotate_sets_and_clears_rot_and_refuses_frames() {
    let mut p = deck();
    let a = p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(20.0, 20.0, 60.0, 30.0)), &ShapeStyle::default()).unwrap();
    let ch = p.add_chart(1, ChartKind::Line, &[], &[vec![1.0, 2.0]], &ChartOptions::default()).unwrap();
    let before = snapshot(&p);
    p.rotate_shape(1, &ShapeRef::Id(a), 45.0).unwrap();
    assert_eq!(changed(&before, &snapshot(&p)), ["ppt/slides/slide2.xml"]);
    let rot = |p: &Presentation| p.pkg.get_xml("ppt/slides/slide2.xml").unwrap().root.find_all("p:sp")[0].child("p:spPr").unwrap().child("a:xfrm").unwrap().attr("rot").map(String::from);
    assert_eq!(rot(&p).as_deref(), Some("2700000"));
    p.rotate_shape(1, &ShapeRef::Id(a), -90.0).unwrap();
    assert_eq!(rot(&p).as_deref(), Some("16200000"), "-90 is 270 clockwise");
    p.rotate_shape(1, &ShapeRef::Id(a), 360.0).unwrap();
    assert_eq!(rot(&p), None, "a full turn clears it");
    // Position and size survive.
    let r = p.shapes(1).unwrap()[0].rect.unwrap();
    assert!((r.x - 20.0).abs() < 1e-3 && (r.w - 60.0).abs() < 1e-3);
    // Charts have no rotation; the error says so. A title placeholder inheriting its position gets one.
    let e = p.rotate_shape(1, &ShapeRef::Id(ch), 10.0).unwrap_err();
    assert!(e.contains("chart cannot be rotated"), "{e}");
    let title = p.shapes(0).unwrap().into_iter().find(|s| s.kind == "placeholder").unwrap();
    p.rotate_shape(0, &ShapeRef::Id(title.id), 5.0).unwrap();
    let t2 = p.shapes(0).unwrap().into_iter().find(|s| s.kind == "placeholder").unwrap();
    assert_eq!(t2.rect, title.rect, "the inherited rectangle was written down, not lost");
    assert!(p.rotate_shape(1, &ShapeRef::Id(a), f64::NAN).is_err());
    assert!(p.rotate_shape(1, &ShapeRef::Id(999), 1.0).unwrap_err().contains("no shape id 999"));
}

#[test]
fn align_to_selection_and_to_slide_known_answers() {
    let mut p = deck();
    let s = ShapeStyle::default();
    let a = p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(10.0, 10.0, 40.0, 20.0)), &s).unwrap();
    let b = p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(100.0, 50.0, 20.0, 20.0)), &s).unwrap();
    let c = p.add_shape(1, ShapeKind::Ellipse, Geometry::Frame(rect(70.0, 90.0, 30.0, 10.0)), &s).unwrap();
    let all = [ShapeRef::Id(a), ShapeRef::Id(b), ShapeRef::Id(c)];
    let rects = |p: &Presentation| -> Vec<(f64, f64)> { p.shapes(1).unwrap().iter().map(|s| (s.rect.unwrap().x, s.rect.unwrap().y)).collect() };
    let near = |got: Vec<(f64, f64)>, want: &[(f64, f64)]| {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g.0 - w.0).abs() < 1e-3 && (g.1 - w.1).abs() < 1e-3, "{got:?} vs {want:?}");
        }
    };
    // Bounding box of the three: left 10, right 120, top 10, bottom 100.
    p.align_shapes(1, &all, Align::Left, AlignTo::Selection).unwrap();
    near(rects(&p), &[(10.0, 10.0), (10.0, 50.0), (10.0, 90.0)]);
    p.align_shapes(1, &all, Align::Right, AlignTo::Selection).unwrap();
    // After Left the box is x 10..50 (a is 40 wide): everyone's right edge = 50.
    near(rects(&p), &[(10.0, 10.0), (30.0, 50.0), (20.0, 90.0)]);
    p.align_shapes(1, &all, Align::Center, AlignTo::Selection).unwrap();
    near(rects(&p), &[(10.0, 10.0), (20.0, 50.0), (15.0, 90.0)]);
    p.align_shapes(1, &all, Align::Top, AlignTo::Selection).unwrap();
    near(rects(&p), &[(10.0, 10.0), (20.0, 10.0), (15.0, 10.0)]);
    p.align_shapes(1, &all, Align::Bottom, AlignTo::Selection).unwrap();
    near(rects(&p), &[(10.0, 10.0), (20.0, 10.0), (15.0, 20.0)]);
    p.align_shapes(1, &all, Align::Middle, AlignTo::Selection).unwrap();
    near(rects(&p), &[(10.0, 10.0), (20.0, 10.0), (15.0, 15.0)]);
    // Already aligned: nothing is rewritten.
    let before = snapshot(&p);
    p.align_shapes(1, &all, Align::Middle, AlignTo::Selection).unwrap();
    assert_eq!(changed(&before, &snapshot(&p)), Vec::<String>::new());

    // To the slide (16:9 is 338.67 x 190.5 mm).
    p.align_shapes(1, &[ShapeRef::Id(c)], Align::Center, AlignTo::Slide).unwrap();
    p.align_shapes(1, &[ShapeRef::Id(c)], Align::Bottom, AlignTo::Slide).unwrap();
    let rc = p.shapes(1).unwrap()[2].rect.unwrap();
    assert!((rc.x - (338.6667 - 30.0) / 2.0).abs() < 1e-2 && (rc.y - 180.5).abs() < 1e-2, "{rc:?}");

    // A shape turned 90 degrees fills a 20 x 40 box about its centre: aligned
    // Left to the slide, its seen edge sits at 0 and its frame hangs out by 10.
    p.rotate_shape(1, &ShapeRef::Id(a), 90.0).unwrap();
    p.align_shapes(1, &[ShapeRef::Id(a)], Align::Left, AlignTo::Slide).unwrap();
    let ra = p.shapes(1).unwrap()[0].rect.unwrap();
    assert!((ra.x + 10.0).abs() < 1e-2 && (ra.w - 40.0).abs() < 1e-3, "{ra:?}");
    p.align_shapes(1, &[ShapeRef::Id(a)], Align::Top, AlignTo::Slide).unwrap();
    let ra = p.shapes(1).unwrap()[0].rect.unwrap();
    assert!((ra.y - 10.0).abs() < 1e-2, "seen top 0 -> frame y 10: {ra:?}");

    // Refusals.
    assert!(p.align_shapes(1, &[ShapeRef::Id(a)], Align::Left, AlignTo::Selection).unwrap_err().contains("two or more"));
    assert!(p.align_shapes(1, &[ShapeRef::Id(a), ShapeRef::Id(a)], Align::Left, AlignTo::Selection).unwrap_err().contains("twice"));
    assert!(p.align_shapes(1, &[ShapeRef::Id(a), ShapeRef::Id(404)], Align::Left, AlignTo::Selection).unwrap_err().contains("no shape id 404"));
    assert!(p.align_shapes(1, &[], Align::Left, AlignTo::Slide).is_err());
}

#[test]
fn shape_and_chart_edits_leave_animation_timing_and_other_slides_alone() {
    let mut p = deck();
    // Give slide 2 an animation timing tree, as PowerPoint writes it.
    let timing = "<p:timing><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"/></p:par></p:tnLst></p:timing>";
    let mut d = p.pkg.get_xml("ppt/slides/slide2.xml").unwrap();
    d.root.children.push(xml::Node::Elem(xml::parse(timing).unwrap().root));
    p.pkg.set_xml("ppt/slides/slide2.xml", &d);
    let mut p = reopen(&mut p);
    let timing_before = p.pkg.get_xml("ppt/slides/slide2.xml").unwrap().root.child("p:timing").unwrap().clone();
    let before = snapshot(&p);
    p.add_shape(1, ShapeKind::Rect, Geometry::Frame(rect(5.0, 5.0, 10.0, 10.0)), &ShapeStyle::default()).unwrap();
    p.add_chart(1, ChartKind::Line, &[], &[vec![1.0, 2.0]], &ChartOptions::default()).unwrap();
    let q = reopen(&mut p);
    let after = q.pkg.get_xml("ppt/slides/slide2.xml").unwrap();
    assert_eq!(after.root.child("p:timing").unwrap(), &timing_before);
    // timing stays after cSld and clrMapOvr, where the schema puts it.
    let order: Vec<&str> = after.root.elems().map(|e| e.name.as_str()).collect();
    assert_eq!(order, ["p:cSld", "p:clrMapOvr", "p:timing"]);
    let c = changed(&before, &snapshot(&q));
    for part in ["ppt/slideMasters/slideMaster1.xml", "ppt/slideLayouts/slideLayout3.xml", "ppt/theme/theme1.xml", "ppt/slides/slide1.xml", "ppt/slides/slide3.xml", "customXml/item1.xml"] {
        assert!(!c.contains(&part.to_string()), "{part} must not change: {c:?}");
    }
}

/// python-pptx as an independent reader of what was written. `None` when
/// python or the library is missing.
fn python_pptx(path: &std::path::Path, script: &str) -> Option<String> {
    for py in ["python", "python3", "py"] {
        let probe = std::process::Command::new(py).args(["-c", "import pptx"]).output();
        if !probe.is_ok_and(|o| o.status.success()) {
            continue;
        }
        let out = std::process::Command::new(py).args(["-c", script]).arg(path).output().ok()?;
        assert!(out.status.success(), "python-pptx failed to read the file: {}", String::from_utf8_lossy(&out.stderr));
        return Some(String::from_utf8_lossy(&out.stdout).trim().replace("\r\n", "\n"));
    }
    None
}

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qu_pptx_create_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn showcase() -> Presentation {
    let mut p = Presentation::new();
    p.add_slide(Some("Title Only"), Some("Impedance"), &[], None).unwrap();
    let (re, im) = semicircle();
    p.add_nyquist_chart(0, &re, &im, true, &ChartOptions { title: Some("Nyquist".into()), at: Some((15.0, 45.0)), width_mm: Some(150.0), height_mm: Some(110.0), ..Default::default() }).unwrap();
    p.add_chart(
        0,
        ChartKind::Scatter,
        &[ChartX::Numbers(vec![1.0, 10.0, 100.0, 1000.0, 10000.0])],
        &[vec![100.0, 95.0, 60.0, 31.0, 30.0], vec![80.0, 70.0, 50.0, 35.0, 30.5]],
        &ChartOptions { title: Some("|Z|".into()), x_title: Some("f / Hz".into()), y_title: Some("|Z| / ohm".into()), names: vec!["cell A".into(), "cell B".into()], x_log: true, lines: Some(true), at: Some((175.0, 45.0)), width_mm: Some(150.0), height_mm: Some(70.0), ..Default::default() },
    )
    .unwrap();
    p.add_chart(0, ChartKind::Column, &[ChartX::Labels(vec!["Rs".into(), "Rct".into(), "W".into()])], &[vec![30.0, 70.0, 12.0]], &ChartOptions { title: Some("Fit".into()), y_title: Some("ohm".into()), at: Some((175.0, 120.0)), width_mm: Some(150.0), height_mm: Some(55.0), ..Default::default() }).unwrap();
    let blue = ShapeStyle { fill: Some("#1F77B4".into()), text: Some("Rs".into()), text_fmt: TextFormat { size: Some(16.0), bold: true, ..Default::default() }, ..Default::default() };
    p.add_shape(0, ShapeKind::RoundedRect, Geometry::Frame(rect(15.0, 165.0, 40.0, 14.0)), &blue).unwrap();
    p.add_shape(0, ShapeKind::Arrow, Geometry::Segment { x1: 57.0, y1: 172.0, x2: 85.0, y2: 172.0 }, &ShapeStyle::default()).unwrap();
    let e = p.add_shape(0, ShapeKind::Ellipse, Geometry::Frame(rect(88.0, 163.0, 30.0, 18.0)), &ShapeStyle { fill: Some("none".into()), stroke: Some("#D62728".into()), stroke_pt: Some(2.5), text: Some("CPE".into()), ..Default::default() }).unwrap();
    p.rotate_shape(0, &ShapeRef::Id(e), 20.0).unwrap();
    p
}

#[test]
fn python_pptx_reads_back_charts_and_shapes() {
    let mut p = showcase();
    let path = temp_file("showcase.pptx");
    p.save(path.to_str().unwrap()).unwrap();
    let script = r#"
import sys
from pptx import Presentation
from pptx.util import Emu
prs = Presentation(sys.argv[1])
s = prs.slides[0]
for sh in s.shapes:
    if sh.has_chart:
        c = sh.chart
        pl = c.plots[0]
        ser = [(x.name, list(x.values)) for x in pl.series]
        t = c.chart_title.text_frame.text if c.has_title else ''
        print('chart', type(pl).__name__, round(Emu(sh.left).mm), round(Emu(sh.top).mm), round(Emu(sh.width).mm), round(Emu(sh.height).mm), t, len(ser), ser[0][1][:3])
    elif sh.shape_type == 1:
        print('autoshape', sh.auto_shape_type, sh.rotation, sh.text_frame.text)
    else:
        print('other', sh.shape_type, sh.name)
"#;
    let Some(out) = python_pptx(&path, script) else {
        eprintln!("SKIP python_pptx_reads_back_charts_and_shapes: python-pptx not available");
        return;
    };
    eprintln!("{out}");
    assert!(out.contains("chart XyPlot 15 45 150 110 Nyquist 1 [0.0, 7.8217"), "{out}");
    assert!(out.contains("chart XyPlot 175 45 150 70 |Z| 2 [100.0, 95.0, 60.0]"), "{out}");
    assert!(out.contains("chart BarPlot 175 120 150 55 Fit 1 [30.0, 70.0, 12.0]"), "{out}");
    assert!(out.contains("ROUNDED_RECTANGLE (5) 0.0 Rs"), "{out}");
    assert!(out.contains("OVAL (9) 20.0 CPE"), "{out}");
}

#[test]
fn libreoffice_renders_the_charts_and_shapes_to_pdf() {
    if qu_ooxml::find_office().is_none() {
        eprintln!("SKIP libreoffice_renders_the_charts_and_shapes_to_pdf: LibreOffice not found");
        return;
    }
    let mut p = showcase();
    let bytes = p.to_bytes().unwrap();
    let pdf = qu_ooxml::convert_with_office(&bytes, "pptx", "pdf", 180).expect("LibreOffice converts the deck");
    assert!(pdf.starts_with(b"%PDF"));
    let out = temp_file("showcase.pdf");
    std::fs::write(&out, &pdf).unwrap();
    // A page with three charts and six shapes is far bigger than the same
    // page with only a title.
    let mut blank = Presentation::new();
    blank.add_slide(Some("Title Only"), Some("Impedance"), &[], None).unwrap();
    let blank_pdf = qu_ooxml::convert_with_office(&blank.to_bytes().unwrap(), "pptx", "pdf", 180).unwrap();
    assert!(pdf.len() > blank_pdf.len() + 5_000, "chart page {} bytes vs blank {} bytes", pdf.len(), blank_pdf.len());
}

#[test]
fn deleting_a_chart_shape_removes_its_part_and_duplicating_gives_the_copy_its_own() {
    let mut p = deck();
    let before = snapshot(&p);
    let id = p.add_chart(1, ChartKind::Line, &[], &[vec![1.0, 2.0, 3.0]], &ChartOptions::default()).unwrap();
    // Duplicate: the copy must not share the chart part with the original.
    let dup = p.duplicate_slide(1).unwrap();
    assert_eq!(dup, 2);
    let mut q = reopen(&mut p);
    assert!(q.pkg.has("ppt/charts/chart1.xml") && q.pkg.has("ppt/charts/chart2.xml"));
    let target = |q: &Presentation, slide: &str| q.pkg.rels(slide).unwrap().into_iter().find(|r| r.rel_type.ends_with("/chart")).unwrap().target;
    let slides = q.slide_parts().unwrap();
    let (a, b) = (target(&q, &slides[1]), target(&q, &slides[2]));
    assert_ne!(a, b, "two slides, two chart parts");
    assert_eq!(q.pkg.override_type("ppt/charts/chart2.xml").as_deref(), Some(qu_ooxml::chart::CT_CHART));
    assert_eq!(q.pkg.get("ppt/charts/chart1.xml"), q.pkg.get("ppt/charts/chart2.xml"));
    // Delete the copy's chart: its part, relationship and override go; the original stays.
    let copy_id = q.shapes(2).unwrap().into_iter().find(|s| s.kind == "chart").unwrap().id;
    q.delete_shape(2, &ShapeRef::Id(copy_id)).unwrap();
    assert!(!q.pkg.has("ppt/charts/chart2.xml") && q.pkg.override_type("ppt/charts/chart2.xml").is_none());
    assert!(q.pkg.has("ppt/charts/chart1.xml"));
    q.delete_shape(1, &ShapeRef::Id(id)).unwrap();
    let q = reopen(&mut q);
    assert!(!q.pkg.names().iter().any(|n| n.contains("charts/")), "{:?}", q.pkg.names());
    assert!(q.pkg.override_type("ppt/charts/chart1.xml").is_none());
    assert_eq!(before.iter().filter(|(n, _)| n.contains("charts/")).count(), 0);
}
