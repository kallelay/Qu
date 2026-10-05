//! Charts on a slide (`add_chart`, `add_nyquist_chart`).
//!
//! The chart XML is `qu_ooxml::chart`'s -- the same writer the workbook
//! charts use. A slide chart is three things: the chart part
//! (`ppt/charts/chartN.xml`, with its content-type override), a
//! relationship from the slide to it, and a `p:graphicFrame` on the slide
//! pointing at that relationship.
//!
//! The data are CACHED LITERALS (`c:numLit` / `c:strLit`), not references
//! into an embedded workbook. A chart made this way renders in PowerPoint,
//! LibreOffice and python-pptx's reader with exactly the numbers given, but
//! PowerPoint's "Edit Data" has no sheet to open -- embedding a workbook
//! for each chart was judged too heavy for what is a figure-for-a-slide
//! feature. A plot that must stay editable belongs in a workbook
//! (`xlsx.add_chart`) with the slide pasting it.

use crate::Presentation;
use qu_ooxml::chart::{self, ChartSpec, Data, Labels, LegendPos, Series, XData};
use qu_ooxml::xml::{Element, Node};
use qu_ooxml::{mm_to_emu, relative_target};
pub use qu_ooxml::chart::ChartKind;

/// The x values of a series (scatter) or its category labels (line, bar).
#[derive(Clone, Debug, PartialEq)]
pub enum ChartX {
    Numbers(Vec<f64>),
    Labels(Vec<String>),
}

impl ChartX {
    fn len(&self) -> usize {
        match self {
            ChartX::Numbers(v) => v.len(),
            ChartX::Labels(v) => v.len(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChartOptions {
    pub title: Option<String>,
    pub x_title: Option<String>,
    pub y_title: Option<String>,
    /// One name per series.
    pub names: Vec<String>,
    /// One `rrggbb` (leading `#` fine) per series; default: Qu's palette.
    pub colors: Vec<String>,
    pub lines: Option<bool>,
    pub markers: Option<bool>,
    /// Default: shown when there is more than one series or names are given.
    pub legend: Option<bool>,
    pub x_min: Option<f64>,
    pub x_max: Option<f64>,
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub x_log: bool,
    pub y_log: bool,
    /// Scatter only: one data unit is the same length along both axes.
    pub equal_axes: bool,
    /// Top-left corner in mm (default: centred on the slide).
    pub at: Option<(f64, f64)>,
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
    /// The shape's name in the selection pane (default `Chart 5`).
    pub name: Option<String>,
}

/// `"scatter"`/`"xy"`, `"line"`, `"bar"`/`"column"` (vertical), `"barh"` (horizontal).
pub fn chart_kind(name: &str) -> Result<ChartKind, String> {
    match name.to_ascii_lowercase().as_str() {
        "scatter" | "xy" => Ok(ChartKind::Scatter),
        "line" => Ok(ChartKind::Line),
        "bar" | "column" => Ok(ChartKind::Column),
        "barh" => Ok(ChartKind::Bar),
        other => Err(format!("chart kind \"{other}\" -- use scatter, line, bar (vertical) or barh (horizontal)")),
    }
}

/// Build the chart part's spec from literal data, checking everything the
/// writer cannot.
fn build_spec(kind: ChartKind, xs: &[ChartX], ys: &[Vec<f64>], o: &ChartOptions, size: (f64, f64)) -> Result<ChartSpec, String> {
    let n = ys.len();
    if n == 0 {
        return Err("a chart needs at least one series of y values".into());
    }
    if ys.iter().any(|y| y.is_empty()) {
        return Err("a series has no values".into());
    }
    if xs.len() > 1 && xs.len() != n {
        return Err(format!("{n} series but {} x vectors -- give one x for all, or one per series", xs.len()));
    }
    if !o.names.is_empty() && o.names.len() != n {
        return Err(format!("{n} series but {} names", o.names.len()));
    }
    if !o.colors.is_empty() && o.colors.len() != n {
        return Err(format!("{n} series but {} colors", o.colors.len()));
    }
    let scatter = kind == ChartKind::Scatter;
    if o.equal_axes && !scatter {
        return Err("equal_axes= needs a scatter chart -- only there are both axes numeric".into());
    }
    if (o.x_log || o.x_min.is_some() || o.x_max.is_some()) && !scatter {
        return Err("x_min=/x_max=/x_log= need a scatter chart -- a line or bar chart's x axis is categories".into());
    }
    if (o.lines.is_some() || o.markers.is_some()) && matches!(kind, ChartKind::Column | ChartKind::Bar) {
        return Err("lines=/markers= apply to scatter and line charts, not bars".into());
    }
    for (k, y) in ys.iter().enumerate() {
        let x = match xs.len() {
            0 => None,
            1 => Some(&xs[0]),
            _ => Some(&xs[k]),
        };
        if let Some(x) = x {
            if x.len() != y.len() {
                return Err(format!("series {} has {} x values but {} y values", k + 1, x.len(), y.len()));
            }
            if scatter && matches!(x, ChartX::Labels(_)) {
                return Err("a scatter chart needs numeric x values -- use kind \"line\" or \"bar\" for text categories".into());
            }
        }
    }
    // A log axis cannot show zero or negative values; Excel/PowerPoint
    // refuse to draw such a chart rather than skipping the points.
    if o.y_log && ys.iter().flatten().any(|v| v.is_finite() && *v <= 0.0) {
        return Err("y_log=true but y has zero or negative values -- a logarithmic axis cannot show them".into());
    }
    if o.x_log {
        let bad = xs.iter().any(|x| matches!(x, ChartX::Numbers(v) if v.iter().any(|a| a.is_finite() && *a <= 0.0)));
        if bad {
            return Err("x_log=true but x has zero or negative values -- a logarithmic axis cannot show them".into());
        }
    }

    let mut spec = ChartSpec::new(kind);
    spec.title = o.title.clone();
    if let Some(l) = o.lines {
        spec.lines = l;
    }
    if let Some(m) = o.markers {
        spec.markers = m;
    }
    spec.legend = o.legend.unwrap_or(n > 1 || !o.names.is_empty()).then_some(LegendPos::Right);
    spec.x_axis.title = o.x_title.clone();
    spec.y_axis.title = o.y_title.clone();
    spec.x_axis.log = o.x_log;
    spec.y_axis.log = o.y_log;
    (spec.x_axis.min, spec.x_axis.max, spec.y_axis.min, spec.y_axis.max) = (o.x_min, o.x_max, o.y_min, o.y_max);

    let (mut all_x, mut all_y) = (Vec::new(), Vec::new());
    for (k, y) in ys.iter().enumerate() {
        let x = match xs.len() {
            0 => None,
            1 => Some(&xs[0]),
            _ => Some(&xs[k]),
        };
        let xdata = x.map(|x| match (x, scatter) {
            (ChartX::Numbers(v), true) => {
                all_x.extend(v.iter().copied().filter(|a| a.is_finite()));
                XData::Num(Data::Lit(v.clone()))
            }
            (ChartX::Numbers(v), false) => XData::Text(Labels::Lit(v.iter().map(|a| chart::num(*a)).collect())),
            (ChartX::Labels(l), _) => XData::Text(Labels::Lit(l.clone())),
        });
        if x.is_none() {
            all_x.extend((1..=y.len()).map(|i| i as f64));
        }
        all_y.extend(y.iter().copied().filter(|a| a.is_finite()));
        spec.series.push(Series { name: o.names.get(k).cloned(), x: xdata, y: Some(Data::Lit(y.clone())), color: o.colors.get(k).cloned() });
    }
    if o.equal_axes {
        let (w, h) = size;
        let has_legend = spec.legend.is_some();
        // Room for the title, tick labels and axis titles; the plot
        // rectangle is fitted inside what is left.
        let top = if o.title.is_some() { 14.0 } else { 6.0 };
        let (left, right, bottom) = (22.0, if has_legend { 32.0 } else { 6.0 }, 18.0);
        let (bw, bh) = (w - left - right, h - top - bottom);
        let ((x0, x1), (y0, y1), step, (rx, ry, rw, rh)) = chart::equal_scale(&all_x, &all_y, bw, bh)?;
        spec.x_axis.min = Some(x0);
        spec.x_axis.max = Some(x1);
        spec.y_axis.min = Some(y0);
        spec.y_axis.max = Some(y1);
        spec.x_axis.major_unit = Some(step);
        spec.y_axis.major_unit = Some(step);
        spec.x_axis.gridlines = true;
        spec.plot_area = Some(((left + rx) / w, (top + ry) / h, rw / w, rh / h));
    }
    // Reject a bad limit/colour now, before any part is written.
    chart::chart_part(&spec)?;
    Ok(spec)
}

impl Presentation {
    /// Add a chart to slide `i` and return its shape id. `xs` is empty
    /// (points are 1, 2, 3, ...), one x shared by every series, or one per
    /// series; `ys` holds one vector per series.
    pub fn add_chart(&mut self, i: usize, kind: ChartKind, xs: &[ChartX], ys: &[Vec<f64>], o: &ChartOptions) -> Result<u64, String> {
        let size = self.chart_size(o, o.equal_axes)?;
        let spec = build_spec(kind, xs, ys, o, size)?;
        self.place_chart(i, &spec, o, size)
    }

    /// Add an impedance (Nyquist) plot: Z' against -Z'' with both axes on
    /// the same scale, so a semicircle looks like one. `negate` (the usual
    /// case: `im` is Z'' as measured, negative for a capacitive load) plots
    /// `-im`; `false` plots `im` as given.
    pub fn add_nyquist_chart(&mut self, i: usize, re: &[f64], im: &[f64], negate: bool, o: &ChartOptions) -> Result<u64, String> {
        if re.len() != im.len() {
            return Err(format!("Z' has {} values but Z'' has {}", re.len(), im.len()));
        }
        let mut o = o.clone();
        o.equal_axes = true;
        o.x_title.get_or_insert_with(|| "Z' / \u{3a9}".to_string());
        o.y_title.get_or_insert_with(|| "-Z'' / \u{3a9}".to_string());
        if o.lines.is_none() {
            o.lines = Some(false);
        }
        let y: Vec<f64> = if negate { im.iter().map(|v| -v).collect() } else { im.to_vec() };
        let size = self.chart_size(&o, true)?;
        let spec = build_spec(ChartKind::Scatter, &[ChartX::Numbers(re.to_vec())], &[y], &o, size)?;
        self.place_chart(i, &spec, &o, size)
    }

    /// The chart's width and height in mm: given, or a default that fits
    /// the slide (a Nyquist plot defaults a little squarer).
    fn chart_size(&self, o: &ChartOptions, square: bool) -> Result<(f64, f64), String> {
        let (sw, sh) = self.slide_size();
        let (dw, dh) = if square { (150.0_f64.min(sw * 0.9), 110.0_f64.min(sh * 0.85)) } else { (160.0_f64.min(sw * 0.9), 90.0_f64.min(sh * 0.8)) };
        let (w, h) = (o.width_mm.unwrap_or(dw), o.height_mm.unwrap_or(dh));
        if !(20.0..=2000.0).contains(&w) || !(20.0..=2000.0).contains(&h) {
            return Err(format!("chart size {w} x {h} mm -- each side must be 20 to 2000 mm"));
        }
        Ok((w, h))
    }

    fn place_chart(&mut self, i: usize, spec: &ChartSpec, o: &ChartOptions, (w, h): (f64, f64)) -> Result<u64, String> {
        let (sw, sh) = self.slide_size();
        let (x, y) = o.at.unwrap_or(((sw - w) / 2.0, (sh - h) / 2.0));
        if !x.is_finite() || !y.is_finite() {
            return Err("at= must be two finite numbers [x, y]".into());
        }
        let slide = self.slide_part(i)?;
        let xml = chart::chart_part(spec)?;
        let part = self.pkg.next_name("ppt/charts/chart", ".xml");
        self.pkg.set(&part, xml.into_bytes());
        self.pkg.add_override(&part, chart::CT_CHART)?;
        let rid = self.pkg.add_rel(&slide, chart::REL_CHART, &relative_target(&slide, &part))?;
        let name = o.name.clone();
        let res = self.with_tree(i, |tree, id| {
            let name = name.unwrap_or_else(|| format!("Chart {id}"));
            let xfrm = Element::new("p:xfrm")
                .with_child(Element::new("a:off").with_attr("x", &mm_to_emu(x).to_string()).with_attr("y", &mm_to_emu(y).to_string()))
                .with_child(Element::new("a:ext").with_attr("cx", &mm_to_emu(w).to_string()).with_attr("cy", &mm_to_emu(h).to_string()));
            let gf = Element::new("p:graphicFrame")
                .with_child(
                    Element::new("p:nvGraphicFramePr")
                        .with_child(Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &name))
                        .with_child(Element::new("p:cNvGraphicFramePr").with_child(Element::new("a:graphicFrameLocks").with_attr("noGrp", "1")))
                        .with_child(Element::new("p:nvPr")),
                )
                .with_child(xfrm)
                .with_child(chart::graphic(&rid));
            tree.children.push(Node::Elem(gf));
            Ok(id)
        });
        match res {
            Ok((id, _, _)) => Ok(id),
            Err(e) => {
                // Leave the package as it was.
                self.pkg.remove_rel(&slide, &rid)?;
                self.pkg.remove(&part);
                self.pkg.remove_override(&part)?;
                Err(e)
            }
        }
    }
}
