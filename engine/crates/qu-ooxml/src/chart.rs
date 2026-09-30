//! DrawingML charts (`c:chartSpace`, ECMA-376 part 1 §21.2): the chart
//! part itself, independent of which document holds it.
//!
//! A chart is the same XML in all three formats; only where it hangs
//! differs -- an `xdr:graphicFrame` in a spreadsheet drawing, a
//! `p:graphicFrame` on a slide, a `wp:inline` in a Word paragraph. So this
//! module writes the chart part and the `a:graphic` reference to it, and
//! each format supplies its own frame. Data are either cell references
//! with an optional cached copy (a workbook's own charts) or literals (a
//! chart with no workbook behind it).
//!
//! Element order follows the schema sequences exactly (CT_ScatterSer,
//! CT_ValAx, ...): Excel rejects a chart whose children are out of order,
//! even where LibreOffice would forgive it.

use crate::xml::{Doc, Element};

pub const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
pub const REL_CHART: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

/// Qu's default plot palette (`plotting.rs`, "default"), so a chart in a
/// workbook and a figure from `plot` read as the same family.
pub const PALETTE: &[&str] = &["5B7CFA", "228833", "FF5C5C", "BF9F40", "7540BF", "808080", "BF4080", "0084AA"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartKind {
    /// XY: both coordinates numeric.
    Scatter,
    /// Values against categories, joined by lines.
    Line,
    /// Vertical bars.
    Column,
    /// Horizontal bars.
    Bar,
}

/// Numbers for one series coordinate.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    /// A reference such as `'Sheet 1'!$B$2:$B$20`, with the values the
    /// cells held when the chart was written (`None` for an empty cell).
    /// Readers that do not recalculate show the cache.
    Ref { formula: String, cache: Vec<Option<f64>> },
    Lit(Vec<f64>),
}

/// Category labels (line/bar x-axis), or a series name.
#[derive(Clone, Debug, PartialEq)]
pub enum Labels {
    Ref { formula: String, cache: Vec<String> },
    Lit(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum XData {
    Num(Data),
    Text(Labels),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub name: Option<String>,
    /// Scatter: the x values (absent = 1, 2, 3, ...). Line/bar: categories.
    pub x: Option<XData>,
    pub y: Option<Data>,
    /// `rrggbb`, no `#`; `None` takes the palette entry for the series index.
    pub color: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub title: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub major_unit: Option<f64>,
    pub log: bool,
    /// An Excel number format for the tick labels.
    pub number_format: Option<String>,
    pub gridlines: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LegendPos {
    Right,
    Top,
    Bottom,
    Left,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub title: Option<String>,
    pub series: Vec<Series>,
    pub x_axis: Axis,
    pub y_axis: Axis,
    /// Scatter/line: join the points.
    pub lines: bool,
    /// Scatter/line: draw a marker at each point.
    pub markers: bool,
    pub legend: Option<LegendPos>,
    /// The inner plot rectangle as fractions of the chart (x, y, w, h) --
    /// fixed by the caller when the axes must keep an aspect ratio.
    pub plot_area: Option<(f64, f64, f64, f64)>,
}

impl ChartSpec {
    pub fn new(kind: ChartKind) -> Self {
        ChartSpec {
            kind,
            title: None,
            series: Vec::new(),
            x_axis: Axis::default(),
            y_axis: Axis { gridlines: true, ..Axis::default() },
            lines: kind != ChartKind::Scatter,
            markers: kind == ChartKind::Scatter,
            legend: Some(LegendPos::Right),
            plot_area: None,
        }
    }
}

fn el(name: &str) -> Element {
    Element::new(name)
}

fn val(name: &str, v: &str) -> Element {
    Element::new(name).with_attr("val", v)
}

/// A number as xsd:double text. Rust's `Display` never uses an exponent,
/// which is valid but unbounded in length; `{:e}` is shorter at the
/// extremes and equally valid.
pub fn num(x: f64) -> String {
    let a = x.abs();
    if a != 0.0 && !(1e-6..1e15).contains(&a) {
        format!("{x:e}")
    } else {
        format!("{x}")
    }
}

fn rich_text(text: &str, bold: bool, size_hundredths: Option<u32>) -> Element {
    let mut rpr = el("a:defRPr").with_attr("b", if bold { "1" } else { "0" });
    if let Some(s) = size_hundredths {
        rpr.set_attr("sz", &s.to_string());
    }
    let mut run_pr = el("a:rPr").with_attr("lang", "en-US").with_attr("b", if bold { "1" } else { "0" });
    if let Some(s) = size_hundredths {
        run_pr.set_attr("sz", &s.to_string());
    }
    el("c:rich")
        .with_child(el("a:bodyPr"))
        .with_child(el("a:lstStyle"))
        .with_child(
            el("a:p")
                .with_child(el("a:pPr").with_child(rpr))
                .with_child(el("a:r").with_child(run_pr).with_child(el("a:t").with_text(text))),
        )
}

fn title(text: &str, size: u32) -> Element {
    el("c:title").with_child(el("c:tx").with_child(rich_text(text, true, Some(size)))).with_child(val("c:overlay", "0"))
}

fn num_data(tag: &str, d: &Data) -> Element {
    let inner = match d {
        Data::Ref { formula, cache } => {
            let mut r = el("c:numRef").with_child(el("c:f").with_text(formula));
            if !cache.is_empty() {
                let mut c = el("c:numCache").with_child(el("c:formatCode").with_text("General")).with_child(val("c:ptCount", &cache.len().to_string()));
                for (i, v) in cache.iter().enumerate() {
                    if let Some(v) = v.filter(|v| v.is_finite()) {
                        c = c.with_child(el("c:pt").with_attr("idx", &i.to_string()).with_child(el("c:v").with_text(&num(v))));
                    }
                }
                r = r.with_child(c);
            }
            r
        }
        Data::Lit(xs) => {
            let mut c = el("c:numLit").with_child(el("c:formatCode").with_text("General")).with_child(val("c:ptCount", &xs.len().to_string()));
            for (i, v) in xs.iter().enumerate() {
                if v.is_finite() {
                    c = c.with_child(el("c:pt").with_attr("idx", &i.to_string()).with_child(el("c:v").with_text(&num(*v))));
                }
            }
            c
        }
    };
    el(tag).with_child(inner)
}

fn str_points(mut c: Element, xs: &[String]) -> Element {
    c = c.with_child(val("c:ptCount", &xs.len().to_string()));
    for (i, s) in xs.iter().enumerate() {
        c = c.with_child(el("c:pt").with_attr("idx", &i.to_string()).with_child(el("c:v").with_text(s)));
    }
    c
}

fn label_data(tag: &str, l: &Labels) -> Element {
    let inner = match l {
        Labels::Ref { formula, cache } => {
            let mut r = el("c:strRef").with_child(el("c:f").with_text(formula));
            if !cache.is_empty() {
                r = r.with_child(str_points(el("c:strCache"), cache));
            }
            r
        }
        Labels::Lit(xs) => str_points(el("c:strLit"), xs),
    };
    el(tag).with_child(inner)
}

fn x_data(tag: &str, x: &XData) -> Element {
    match x {
        XData::Num(d) => num_data(tag, d),
        XData::Text(l) => label_data(tag, l),
    }
}

fn solid(color: &str) -> Element {
    el("a:solidFill").with_child(val("a:srgbClr", color))
}

fn series_color(s: &Series, i: usize) -> Result<String, String> {
    match &s.color {
        None => Ok(PALETTE[i % PALETTE.len()].to_string()),
        Some(c) => {
            let h = c.trim_start_matches('#');
            if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(h.to_ascii_uppercase())
            } else {
                Err(format!("series color \"{c}\" -- use #rrggbb"))
            }
        }
    }
}

fn series(spec: &ChartSpec, s: &Series, i: usize) -> Result<Element, String> {
    let color = series_color(s, i)?;
    let y = s.y.as_ref().ok_or_else(|| format!("series {} has no values", i + 1))?;
    let mut e = el("c:ser").with_child(val("c:idx", &i.to_string())).with_child(val("c:order", &i.to_string()));
    if let Some(n) = &s.name {
        e = e.with_child(el("c:tx").with_child(el("c:v").with_text(n)));
    }
    match spec.kind {
        ChartKind::Scatter | ChartKind::Line => {
            let ln = if spec.lines {
                el("a:ln").with_attr("w", "19050").with_attr("cap", "rnd").with_child(solid(&color)).with_child(el("a:round"))
            } else {
                el("a:ln").with_attr("w", "19050").with_child(el("a:noFill"))
            };
            e = e.with_child(el("c:spPr").with_child(ln));
            let marker = if spec.markers {
                el("c:marker")
                    .with_child(val("c:symbol", "circle"))
                    .with_child(val("c:size", "5"))
                    .with_child(el("c:spPr").with_child(solid(&color)).with_child(el("a:ln").with_attr("w", "9525").with_child(solid(&color))))
            } else {
                el("c:marker").with_child(val("c:symbol", "none"))
            };
            e = e.with_child(marker);
            if spec.kind == ChartKind::Scatter {
                if let Some(x) = &s.x {
                    e = e.with_child(x_data("c:xVal", x));
                }
                e = e.with_child(num_data("c:yVal", y));
            } else {
                if let Some(x) = &s.x {
                    e = e.with_child(x_data("c:cat", x));
                }
                e = e.with_child(num_data("c:val", y));
            }
            e = e.with_child(val("c:smooth", "0"));
        }
        ChartKind::Column | ChartKind::Bar => {
            e = e.with_child(el("c:spPr").with_child(solid(&color)));
            e = e.with_child(val("c:invertIfNegative", "0"));
            if let Some(x) = &s.x {
                e = e.with_child(x_data("c:cat", x));
            }
            e = e.with_child(num_data("c:val", y));
        }
    }
    Ok(e)
}

fn gridlines() -> Element {
    el("c:majorGridlines").with_child(el("c:spPr").with_child(el("a:ln").with_attr("w", "6350").with_child(solid("D9D9D9"))))
}

fn axis_line() -> Element {
    el("c:spPr").with_child(el("a:ln").with_attr("w", "9525").with_child(solid("595959")))
}

/// A value axis. `pos` is b/l/t/r.
fn val_axis(id: &str, cross: &str, pos: &str, a: &Axis, cross_between: &str) -> Element {
    let mut scaling = el("c:scaling");
    if a.log {
        scaling = scaling.with_child(val("c:logBase", "10"));
    }
    scaling = scaling.with_child(val("c:orientation", "minMax"));
    if let Some(m) = a.max {
        scaling = scaling.with_child(val("c:max", &num(m)));
    }
    if let Some(m) = a.min {
        scaling = scaling.with_child(val("c:min", &num(m)));
    }
    let mut e = el("c:valAx").with_child(val("c:axId", id)).with_child(scaling).with_child(val("c:delete", "0")).with_child(val("c:axPos", pos));
    if a.gridlines {
        e = e.with_child(gridlines());
    }
    if let Some(t) = &a.title {
        e = e.with_child(title(t, 1000));
    }
    let fmt = a.number_format.as_deref().unwrap_or("General");
    e = e.with_child(el("c:numFmt").with_attr("formatCode", fmt).with_attr("sourceLinked", if a.number_format.is_some() { "0" } else { "1" }));
    e = e
        .with_child(val("c:majorTickMark", "out"))
        .with_child(val("c:minorTickMark", "none"))
        .with_child(val("c:tickLblPos", "low"))
        .with_child(axis_line())
        .with_child(val("c:crossAx", cross))
        .with_child(val("c:crosses", "autoZero"))
        .with_child(val("c:crossBetween", cross_between));
    if let Some(u) = a.major_unit {
        e = e.with_child(val("c:majorUnit", &num(u)));
    }
    e
}

fn cat_axis(id: &str, cross: &str, pos: &str, a: &Axis) -> Element {
    let mut e = el("c:catAx")
        .with_child(val("c:axId", id))
        .with_child(el("c:scaling").with_child(val("c:orientation", "minMax")))
        .with_child(val("c:delete", "0"))
        .with_child(val("c:axPos", pos));
    if a.gridlines {
        e = e.with_child(gridlines());
    }
    if let Some(t) = &a.title {
        e = e.with_child(title(t, 1000));
    }
    e.with_child(el("c:numFmt").with_attr("formatCode", "General").with_attr("sourceLinked", "1"))
        .with_child(val("c:majorTickMark", "out"))
        .with_child(val("c:minorTickMark", "none"))
        .with_child(val("c:tickLblPos", "low"))
        .with_child(axis_line())
        .with_child(val("c:crossAx", cross))
        .with_child(val("c:crosses", "autoZero"))
        .with_child(val("c:auto", "1"))
        .with_child(val("c:lblAlgn", "ctr"))
        .with_child(val("c:lblOffset", "100"))
        .with_child(val("c:noMultiLvlLbl", "0"))
}

/// The chart part (`xl/charts/chartN.xml`, `ppt/charts/chartN.xml`, ...).
pub fn chart_part(spec: &ChartSpec) -> Result<String, String> {
    if spec.series.is_empty() {
        return Err("a chart needs at least one series".into());
    }
    for (name, a) in [("x", &spec.x_axis), ("y", &spec.y_axis)] {
        if let (Some(lo), Some(hi)) = (a.min, a.max) {
            if !(lo < hi) {
                return Err(format!("{name} axis: min ({lo}) must be below max ({hi})"));
            }
        }
        if a.log && a.min.is_some_and(|m| m <= 0.0) {
            return Err(format!("{name} axis is logarithmic, so its min must be positive"));
        }
        if a.major_unit.is_some_and(|u| !(u > 0.0)) {
            return Err(format!("{name} axis: the major unit must be positive"));
        }
    }
    let (ax_x, ax_y) = ("500000001", "500000002");
    let mut group = match spec.kind {
        ChartKind::Scatter => el("c:scatterChart").with_child(val("c:scatterStyle", if spec.lines { "lineMarker" } else { "marker" })).with_child(val("c:varyColors", "0")),
        ChartKind::Line => el("c:lineChart").with_child(val("c:grouping", "standard")).with_child(val("c:varyColors", "0")),
        ChartKind::Column | ChartKind::Bar => el("c:barChart")
            .with_child(val("c:barDir", if spec.kind == ChartKind::Bar { "bar" } else { "col" }))
            .with_child(val("c:grouping", "clustered"))
            .with_child(val("c:varyColors", "0")),
    };
    for (i, s) in spec.series.iter().enumerate() {
        group = group.with_child(series(spec, s, i)?);
    }
    match spec.kind {
        ChartKind::Line => group = group.with_child(val("c:marker", "1")),
        ChartKind::Column | ChartKind::Bar => group = group.with_child(val("c:gapWidth", "150")),
        ChartKind::Scatter => {}
    }
    group = group.with_child(val("c:axId", ax_x)).with_child(val("c:axId", ax_y));

    let mut layout = el("c:layout");
    if let Some((x, y, w, h)) = spec.plot_area {
        let ml = el("c:manualLayout")
            .with_child(val("c:layoutTarget", "inner"))
            .with_child(val("c:xMode", "edge"))
            .with_child(val("c:yMode", "edge"))
            .with_child(val("c:x", &num(x)))
            .with_child(val("c:y", &num(y)))
            .with_child(val("c:w", &num(w)))
            .with_child(val("c:h", &num(h)));
        layout = layout.with_child(ml);
    }
    let mut plot = el("c:plotArea").with_child(layout).with_child(group);
    // Horizontal bars put the category axis on the left.
    let (cat_pos, val_pos) = if spec.kind == ChartKind::Bar { ("l", "b") } else { ("b", "l") };
    match spec.kind {
        ChartKind::Scatter => {
            plot = plot.with_child(val_axis(ax_x, ax_y, "b", &spec.x_axis, "midCat")).with_child(val_axis(ax_y, ax_x, "l", &spec.y_axis, "midCat"));
        }
        _ => {
            plot = plot.with_child(cat_axis(ax_x, ax_y, cat_pos, &spec.x_axis)).with_child(val_axis(ax_y, ax_x, val_pos, &spec.y_axis, "between"));
        }
    }
    let mut chart = el("c:chart");
    if let Some(t) = &spec.title {
        chart = chart.with_child(title(t, 1400));
    }
    chart = chart.with_child(val("c:autoTitleDeleted", if spec.title.is_some() { "0" } else { "1" })).with_child(plot);
    if let Some(pos) = spec.legend {
        let p = match pos {
            LegendPos::Right => "r",
            LegendPos::Top => "t",
            LegendPos::Bottom => "b",
            LegendPos::Left => "l",
        };
        chart = chart.with_child(el("c:legend").with_child(val("c:legendPos", p)).with_child(val("c:overlay", "0")));
    }
    chart = chart.with_child(val("c:plotVisOnly", "1")).with_child(val("c:dispBlanksAs", "gap"));
    let root = el("c:chartSpace")
        .with_attr("xmlns:c", NS_C)
        .with_attr("xmlns:a", NS_A)
        .with_attr("xmlns:r", NS_R)
        .with_child(val("c:roundedCorners", "0"))
        .with_child(chart)
        .with_child(el("c:spPr").with_child(solid("FFFFFF")).with_child(el("a:ln").with_child(el("a:noFill"))));
    Ok(Doc::new(root).to_xml())
}

/// The `a:graphic` that points a frame (spreadsheet, slide or Word) at a
/// chart part through relationship `rid`.
pub fn graphic(rid: &str) -> Element {
    el("a:graphic").with_child(
        el("a:graphicData")
            .with_attr("uri", NS_C)
            .with_child(el("c:chart").with_attr("xmlns:c", NS_C).with_attr("xmlns:r", NS_R).with_attr("r:id", rid)),
    )
}

/// A "nice" step (1, 2 or 5 x 10^k) giving about `target` intervals over `span`.
pub fn nice_step(span: f64, target: f64) -> f64 {
    if !(span > 0.0) || !span.is_finite() {
        return 1.0;
    }
    let raw = span / target.max(1.0);
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    let m = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    m * mag
}

/// Axis limits that give both axes the SAME scale (one data unit is the
/// same length on the page along x and along y) inside a plot box of
/// `box_w` x `box_h` (any unit): the Nyquist-plot requirement, where a
/// semicircle must look like one.
///
/// Both ranges are whole multiples of a shared step, so the gridlines form
/// squares; the returned plot rectangle (x, y, w, h, same unit as the box,
/// origin top-left) is the largest one with the ranges' aspect ratio that
/// fits the box, centred. Returns ((xmin, xmax), (ymin, ymax), step, rect).
pub fn equal_scale(xs: &[f64], ys: &[f64], box_w: f64, box_h: f64) -> Result<((f64, f64), (f64, f64), f64, (f64, f64, f64, f64)), String> {
    let fin = |v: &[f64]| -> Option<(f64, f64)> {
        let mut it = v.iter().copied().filter(|x| x.is_finite());
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), x| (lo.min(x), hi.max(x))))
    };
    let (x0, x1) = fin(xs).ok_or("no finite x values to scale the axes from")?;
    let (y0, y1) = fin(ys).ok_or("no finite y values to scale the axes from")?;
    if !(box_w > 0.0 && box_h > 0.0) {
        return Err("the chart is too small to hold its axes".into());
    }
    // The origin belongs on a Nyquist plot's y axis: -Z'' is measured from 0.
    let (y0, y1) = (y0.min(0.0), y1.max(0.0));
    let span = (x1 - x0).max(y1 - y0).max(f64::MIN_POSITIVE);
    let mut step = nice_step(span, 6.0);
    // A few passes: the step chosen from the larger span can leave the box
    // under-used when rounding pushes a range up a whole step.
    let (mut lo_x, mut hi_x, mut lo_y, mut hi_y);
    loop {
        lo_x = (x0 / step).floor() * step;
        hi_x = (x1 / step).ceil() * step;
        lo_y = (y0 / step).floor() * step;
        hi_y = (y1 / step).ceil() * step;
        if hi_x <= lo_x {
            hi_x = lo_x + step;
        }
        if hi_y <= lo_y {
            hi_y = lo_y + step;
        }
        let n = ((hi_x - lo_x) / step).max((hi_y - lo_y) / step).round();
        if n <= 12.0 {
            break;
        }
        // The next nice step up (1 -> 2 -> 5 -> 10 ...).
        step = nice_step(step * 1.01, 1.0);
    }
    let (rx, ry) = (hi_x - lo_x, hi_y - lo_y);
    let s = (box_w / rx).min(box_h / ry);
    let (w, h) = (rx * s, ry * s);
    let rect = ((box_w - w) / 2.0, (box_h - h) / 2.0, w, h);
    Ok(((lo_x, hi_x), (lo_y, hi_y), step, rect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn scatter() -> ChartSpec {
        let mut s = ChartSpec::new(ChartKind::Scatter);
        s.title = Some("Z & phase".into());
        s.x_axis.title = Some("f / Hz".into());
        s.x_axis.log = true;
        s.series.push(Series {
            name: Some("cell 1".into()),
            x: Some(XData::Num(Data::Ref { formula: "'My Data'!$A$2:$A$4".into(), cache: vec![Some(1.0), None, Some(1e-9)] })),
            y: Some(Data::Lit(vec![1.0, 2.5, f64::NAN])),
            color: None,
        });
        s
    }

    #[test]
    fn scatter_part_is_well_formed_and_ordered() {
        let x = chart_part(&scatter()).unwrap();
        let d = xml::parse(&x).unwrap();
        let ser = d.root.find_all("c:ser")[0];
        let order: Vec<&str> = ser.elems().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["c:idx", "c:order", "c:tx", "c:spPr", "c:marker", "c:xVal", "c:yVal", "c:smooth"]);
        assert_eq!(d.root.find_all("c:f")[0].text(), "'My Data'!$A$2:$A$4");
        // An empty cell and a NaN are gaps, not zeros; ptCount keeps the length.
        let caches = d.root.find_all("c:pt");
        let idx: Vec<&str> = caches.iter().map(|p| p.attr("idx").unwrap()).collect();
        assert_eq!(idx, ["0", "2", "0", "1"]);
        assert_eq!(caches[1].text(), "1e-9");
        assert!(x.contains("Z &amp; phase"));
        let ax = d.root.find_all("c:valAx");
        assert_eq!(ax.len(), 2);
        let scaling: Vec<&str> = ax[0].child("c:scaling").unwrap().elems().map(|e| e.name.as_str()).collect();
        assert_eq!(scaling, ["c:logBase", "c:orientation"]);
        let axo: Vec<&str> = ax[0].elems().map(|e| e.name.as_str()).collect();
        assert_eq!(axo, ["c:axId", "c:scaling", "c:delete", "c:axPos", "c:title", "c:numFmt", "c:majorTickMark", "c:minorTickMark", "c:tickLblPos", "c:spPr", "c:crossAx", "c:crosses", "c:crossBetween"]);
    }

    #[test]
    fn bar_and_line_use_category_axes() {
        for (k, group, dir) in [(ChartKind::Column, "c:barChart", Some("col")), (ChartKind::Bar, "c:barChart", Some("bar")), (ChartKind::Line, "c:lineChart", None)] {
            let mut s = ChartSpec::new(k);
            s.series.push(Series { x: Some(XData::Text(Labels::Lit(vec!["a".into(), "b".into()]))), y: Some(Data::Lit(vec![1.0, 2.0])), ..Default::default() });
            let d = xml::parse(&chart_part(&s).unwrap()).unwrap();
            let g = d.root.find_all(group);
            assert_eq!(g.len(), 1);
            assert_eq!(g[0].child("c:barDir").and_then(|e| e.attr("val")), dir);
            assert_eq!(d.root.find_all("c:catAx").len(), 1);
            assert_eq!(d.root.find_all("c:strLit").len(), 1);
        }
    }

    #[test]
    fn bad_axes_and_colors_are_refused() {
        let mut s = scatter();
        s.y_axis.min = Some(3.0);
        s.y_axis.max = Some(1.0);
        assert!(chart_part(&s).unwrap_err().contains("below max"));
        let mut s = scatter();
        s.x_axis.min = Some(0.0);
        assert!(chart_part(&s).unwrap_err().contains("positive"));
        let mut s = scatter();
        s.series[0].color = Some("red".into());
        assert!(chart_part(&s).unwrap_err().contains("#rrggbb"));
        assert!(chart_part(&ChartSpec::new(ChartKind::Line)).is_err());
    }

    #[test]
    fn equal_scale_known_answer() {
        // A semicircle of radius 50 centred at (60, 0), -Z'' up: x in [10, 110], y in [0, 50].
        let xs: Vec<f64> = (0..=20).map(|i| 60.0 - 50.0 * (i as f64 * std::f64::consts::PI / 20.0).cos()).collect();
        let ys: Vec<f64> = (0..=20).map(|i| 50.0 * (i as f64 * std::f64::consts::PI / 20.0).sin()).collect();
        let ((x0, x1), (y0, y1), step, (_, _, w, h)) = equal_scale(&xs, &ys, 150.0, 100.0).unwrap();
        assert_eq!(step, 20.0);
        assert_eq!((x0, x1, y0, y1), (0.0, 120.0, 0.0, 60.0));
        // Same mm per ohm both ways, and the box is filled along one side.
        assert!(((x1 - x0) / w - (y1 - y0) / h).abs() < 1e-12);
        assert!((w - 150.0).abs() < 1e-9 || (h - 100.0).abs() < 1e-9);
        assert_eq!(nice_step(7.3, 6.0), 2.0);
        assert!((nice_step(0.0042, 4.0) - 0.002).abs() < 1e-15);
        assert!((nice_step(20.2, 1.0) - 50.0).abs() < 1e-12);
    }
}
