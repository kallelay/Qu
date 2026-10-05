//! Creating shapes (`add_shape`), rotating them and aligning them.
//!
//! Same surgical rule as the rest of the crate: each operation parses and
//! re-serializes only the one slide it changes. Nothing here touches the
//! master, the layouts, the theme or any animation timing -- a shape added
//! to a slide that has a `p:timing` leaves that timing exactly as it was.
//!
//! Shapes are written with explicit fill and outline and NO `p:style`
//! reference: a theme-styled shape takes its colours from whatever theme the
//! deck has, which makes "the colour I asked for" depend on the file. The
//! `fill=`/`stroke=` a caller gives is what is drawn.

use crate::edit::{hex_color, kind_of, ph_key, rect_of_xfrm, shape_elem, shape_elems_mut, xfrm_of, ShapeRef};
use crate::{align_attr, paragraph, run_props, Presentation, Rect, TextFormat};
use qu_ooxml::mm_to_emu;
use qu_ooxml::xml::{Element, Node};

/// Default fill: the first colour of Qu's plot palette.
pub const DEFAULT_FILL: &str = "5B7CFA";
/// Outline for a shape drawn with no fill (otherwise it would be invisible),
/// and for lines and arrows.
pub const DEFAULT_STROKE: &str = "404040";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    RoundedRect,
    /// A straight line between two points.
    Line,
    /// A straight line with an arrowhead at its end point.
    Arrow,
}

impl ShapeKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "rect" | "rectangle" | "box" => Ok(ShapeKind::Rect),
            "ellipse" | "oval" => Ok(ShapeKind::Ellipse),
            "rounded_rect" | "rounded_rectangle" | "roundrect" | "rounded" => Ok(ShapeKind::RoundedRect),
            "line" => Ok(ShapeKind::Line),
            "arrow" => Ok(ShapeKind::Arrow),
            other => Err(format!("shape kind \"{other}\" -- use rect, ellipse, rounded_rect, line or arrow")),
        }
    }

    pub fn is_line(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow)
    }
}

/// Where a shape goes, in millimetres from the slide's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geometry {
    /// A box (rect, ellipse, rounded rect).
    Frame(Rect),
    /// From the first point to the second (line, arrow).
    Segment { x1: f64, y1: f64, x2: f64, y2: f64 },
}

#[derive(Clone, Debug, Default)]
pub struct ShapeStyle {
    /// `rrggbb` (a leading `#` is fine) or `none`. Default: the palette blue.
    pub fill: Option<String>,
    /// `rrggbb` or `none`. Default: no outline on a filled shape, a dark one
    /// on an unfilled shape and on lines.
    pub stroke: Option<String>,
    /// Outline width in points (default 1.5).
    pub stroke_pt: Option<f64>,
    /// `solid`, `dash`, `dot` or `dashdot`.
    pub dash: Option<String>,
    /// Corner radius of a rounded rectangle as a fraction of its shorter
    /// side, 0 to 0.5 (default 1/6, PowerPoint's own).
    pub radius: Option<f64>,
    /// Text inside the shape (not for lines). Newlines start paragraphs.
    pub text: Option<String>,
    pub text_fmt: TextFormat,
    /// The shape's name in the selection pane (default: `Rectangle 5`, ...).
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
    /// Centre horizontally.
    Center,
    /// Centre vertically.
    Middle,
}

impl Align {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "left" => Ok(Align::Left),
            "right" => Ok(Align::Right),
            "top" => Ok(Align::Top),
            "bottom" => Ok(Align::Bottom),
            "center" | "centre" | "hcenter" => Ok(Align::Center),
            "middle" | "vcenter" => Ok(Align::Middle),
            other => Err(format!("align \"{other}\" -- use left, right, top, bottom, center (horizontal) or middle (vertical)")),
        }
    }
}

/// What `align_shapes` lines the shapes up against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignTo {
    /// The bounding box of the shapes themselves (PowerPoint's "Align
    /// Selected Objects"). Needs at least two shapes.
    Selection,
    Slide,
}

fn rgb(hex: &str) -> Element {
    Element::new("a:solidFill").with_child(Element::new("a:srgbClr").with_attr("val", hex))
}

/// A colour keyword: `none`, or a hex colour. `None` for "not given".
fn paint(v: &Option<String>, what: &str) -> Result<Option<Option<String>>, String> {
    match v {
        None => Ok(None),
        Some(s) if s.eq_ignore_ascii_case("none") => Ok(Some(None)),
        Some(s) => Ok(Some(Some(hex_color(s, what)?))),
    }
}

/// Black or white, whichever reads better on `fill`.
fn contrast(fill: &str) -> &'static str {
    let c = |i: usize| i64::from_str_radix(&fill[i..i + 2], 16).unwrap_or(0) as f64;
    let lum = 0.2126 * c(0) + 0.7152 * c(2) + 0.0722 * c(4);
    if lum > 150.0 {
        "000000"
    } else {
        "FFFFFF"
    }
}

fn dash_name(d: &str) -> Result<&'static str, String> {
    match d.to_ascii_lowercase().as_str() {
        "solid" => Ok("solid"),
        "dash" | "dashed" => Ok("dash"),
        "dot" | "dotted" => Ok("sysDot"),
        "dashdot" | "dash_dot" => Ok("dashDot"),
        other => Err(format!("dash=\"{other}\" -- use solid, dash, dot or dashdot")),
    }
}

fn outline(stroke: Option<&str>, width_pt: f64, dash: Option<&str>, arrow: bool) -> Element {
    let mut ln = Element::new("a:ln").with_attr("w", &((width_pt * 12700.0).round() as i64).to_string());
    match stroke {
        None => return ln.with_child(Element::new("a:noFill")),
        Some(c) => ln = ln.with_child(rgb(c)),
    }
    if let Some(d) = dash {
        ln = ln.with_child(Element::new("a:prstDash").with_attr("val", d));
    }
    if arrow {
        ln = ln.with_child(Element::new("a:tailEnd").with_attr("type", "triangle"));
    }
    ln
}

fn xfrm_el(r: Rect, flip_h: bool, flip_v: bool) -> Element {
    let mut x = Element::new("a:xfrm");
    if flip_h {
        x.set_attr("flipH", "1");
    }
    if flip_v {
        x.set_attr("flipV", "1");
    }
    x.with_child(Element::new("a:off").with_attr("x", &mm_to_emu(r.x).to_string()).with_attr("y", &mm_to_emu(r.y).to_string()))
        .with_child(Element::new("a:ext").with_attr("cx", &mm_to_emu(r.w).to_string()).with_attr("cy", &mm_to_emu(r.h).to_string()))
}

impl Presentation {
    /// Add a shape to slide `i` and return its id (the number
    /// `shapes` reports, and every other shape function takes).
    pub fn add_shape(&mut self, i: usize, kind: ShapeKind, geom: Geometry, st: &ShapeStyle) -> Result<u64, String> {
        let is_line = kind.is_line();
        match (is_line, &geom) {
            (false, Geometry::Frame(_)) | (true, Geometry::Segment { .. }) => {}
            (false, _) => return Err("a rect, ellipse or rounded_rect is placed with x, y, w, h -- not a start and end point".into()),
            (true, _) => return Err("a line or arrow is placed from (x, y) to (x2, y2) -- not with w and h".into()),
        }
        if is_line && (st.fill.is_some() || st.text.is_some() || st.radius.is_some()) {
            return Err("a line or arrow has no fill, text or corner radius".into());
        }
        if kind != ShapeKind::RoundedRect && st.radius.is_some() {
            return Err("radius= applies to rounded_rect only".into());
        }
        if let Some(r) = st.radius {
            if !(0.0..=0.5).contains(&r) {
                return Err(format!("radius={r} -- a fraction of the shorter side, 0 to 0.5"));
            }
        }
        let width_pt = st.stroke_pt.unwrap_or(1.5);
        if !(width_pt > 0.0 && width_pt <= 1000.0) {
            return Err(format!("stroke_width={width_pt} pt -- use more than 0 and at most 1000"));
        }
        let dash = st.dash.as_deref().map(dash_name).transpose()?;
        let fill = paint(&st.fill, "fill")?;
        let stroke = paint(&st.stroke, "stroke")?;

        let (frame, flip_h, flip_v) = match geom {
            Geometry::Frame(r) => {
                if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) || r.w < 0.0 || r.h < 0.0 {
                    return Err("x, y, w, h must be finite, and w and h zero or more".into());
                }
                (r, false, false)
            }
            Geometry::Segment { x1, y1, x2, y2 } => {
                if ![x1, y1, x2, y2].iter().all(|v| v.is_finite()) {
                    return Err("the line's end points must be finite numbers".into());
                }
                if x1 == x2 && y1 == y2 {
                    return Err("the line starts and ends at the same point".into());
                }
                (Rect { x: x1.min(x2), y: y1.min(y2), w: (x2 - x1).abs(), h: (y2 - y1).abs() }, x2 < x1, y2 < y1)
            }
        };
        // Unfilled shapes need an outline, or nothing is drawn.
        let fill_hex: Option<String> = match &fill {
            None => Some(DEFAULT_FILL.to_string()),
            Some(f) => f.clone(),
        };
        let stroke_hex: Option<String> = match &stroke {
            Some(s) => s.clone(),
            None if is_line || fill_hex.is_none() => Some(DEFAULT_STROKE.to_string()),
            None => None,
        };
        let (prst, base_name) = match kind {
            ShapeKind::Rect => ("rect", "Rectangle"),
            ShapeKind::Ellipse => ("ellipse", "Oval"),
            ShapeKind::RoundedRect => ("roundRect", "Rounded Rectangle"),
            ShapeKind::Line => ("line", "Straight Connector"),
            ShapeKind::Arrow => ("straightConnector1", "Straight Arrow Connector"),
        };
        let body = match &st.text {
            Some(text) if !is_line => {
                let mut fmt = st.text_fmt.clone();
                if fmt.color.is_none() {
                    fmt.color = Some(match &fill_hex {
                        Some(f) => format!("#{}", contrast(f)),
                        None => "#000000".to_string(),
                    });
                }
                let rpr = run_props(&fmt)?;
                let algn = align_attr(Some(fmt.align.as_deref().unwrap_or("center")))?;
                let mut b = Element::new("p:txBody")
                    .with_child(Element::new("a:bodyPr").with_attr("wrap", "square").with_attr("rtlCol", "0").with_attr("anchor", "ctr"))
                    .with_child(Element::new("a:lstStyle"));
                for line in text.split('\n') {
                    b = b.with_child(paragraph(line, &rpr, algn));
                }
                Some(b)
            }
            _ => None,
        };

        let name_override = st.name.clone();
        let (id, _, _) = self.with_tree(i, |tree, id| {
            let name = name_override.unwrap_or_else(|| format!("{base_name} {id}"));
            let cnv = Element::new("p:cNvPr").with_attr("id", &id.to_string()).with_attr("name", &name);
            let mut geom_el = Element::new("a:prstGeom").with_attr("prst", prst);
            let mut av = Element::new("a:avLst");
            if let Some(r) = st.radius {
                av = av.with_child(Element::new("a:gd").with_attr("name", "adj").with_attr("fmla", &format!("val {}", (r * 100000.0).round() as i64)));
            }
            geom_el = geom_el.with_child(av);
            let node = if is_line {
                let ln = outline(stroke_hex.as_deref(), width_pt, dash, kind == ShapeKind::Arrow);
                Element::new("p:cxnSp")
                    .with_child(Element::new("p:nvCxnSpPr").with_child(cnv).with_child(Element::new("p:cNvCxnSpPr")).with_child(Element::new("p:nvPr")))
                    .with_child(Element::new("p:spPr").with_child(xfrm_el(frame, flip_h, flip_v)).with_child(geom_el).with_child(ln))
            } else {
                let mut sp_pr = Element::new("p:spPr").with_child(xfrm_el(frame, false, false)).with_child(geom_el);
                sp_pr = sp_pr.with_child(match &fill_hex {
                    Some(f) => rgb(f),
                    None => Element::new("a:noFill"),
                });
                sp_pr = sp_pr.with_child(outline(stroke_hex.as_deref(), width_pt, dash, false));
                let mut sp = Element::new("p:sp")
                    .with_child(Element::new("p:nvSpPr").with_child(cnv).with_child(Element::new("p:cNvSpPr")).with_child(Element::new("p:nvPr")))
                    .with_child(sp_pr);
                if let Some(b) = body {
                    sp = sp.with_child(b);
                }
                sp
            };
            tree.children.push(Node::Elem(node));
            Ok(id)
        })?;
        Ok(id)
    }

    /// Set a shape's rotation to `degrees` clockwise (PowerPoint's Rotation
    /// field; 0 removes it). A chart, table or other graphic frame cannot be
    /// rotated -- PowerPoint has no such thing -- and says so.
    pub fn rotate_shape(&mut self, i: usize, r: &ShapeRef, degrees: f64) -> Result<(), String> {
        if !degrees.is_finite() {
            return Err("the angle must be a finite number of degrees".into());
        }
        let (has_xfrm, kind) = {
            let (_, d) = self.slide_doc(i)?;
            let tree = d.root.child("p:cSld").and_then(|c| c.child("p:spTree")).ok_or("the slide has no shape tree")?;
            let k = Self::locate(tree, r)?;
            let Node::Elem(node) = &tree.children[k] else { unreachable!() };
            let s = shape_elem(node).unwrap();
            (xfrm_of(s).is_some(), kind_of(s))
        };
        if matches!(kind, "chart" | "table" | "graphic") {
            return Err(format!("a {kind} cannot be rotated -- PowerPoint only rotates shapes, pictures and groups"));
        }
        if !has_xfrm {
            // A placeholder positioned by its layout: give it its own
            // position (the inherited one) so there is a transform to turn.
            self.place_shape(i, r, None, None, None, None)?;
        }
        let rot = (degrees.rem_euclid(360.0) * 60000.0).round() as i64 % 21_600_000;
        self.with_tree(i, |tree, _| {
            let k = Self::locate(tree, r)?;
            let Node::Elem(node) = &mut tree.children[k] else { unreachable!() };
            for s in shape_elems_mut(node) {
                let x = if s.name == "p:grpSp" {
                    s.child_mut("p:grpSpPr").and_then(|g| g.child_mut("a:xfrm"))
                } else {
                    s.child_mut("p:spPr").and_then(|g| g.child_mut("a:xfrm"))
                };
                let Some(x) = x else { return Err("the shape has no transform to rotate".into()) };
                if rot == 0 {
                    x.remove_attr("rot");
                } else {
                    x.set_attr("rot", &rot.to_string());
                }
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Line shapes up. `Selection` aligns to the bounding box of the shapes
    /// given (needs two or more); `Slide` aligns each to the slide. A
    /// rotated shape is aligned by the box it is seen to fill, as PowerPoint
    /// does. Shapes already in place are not touched.
    pub fn align_shapes(&mut self, i: usize, refs: &[ShapeRef], how: Align, to: AlignTo) -> Result<(), String> {
        if refs.is_empty() {
            return Err("give the shapes to align".into());
        }
        if to == AlignTo::Selection && refs.len() < 2 {
            return Err("aligning to the selection needs two or more shapes -- or give to=\"slide\"".into());
        }
        let (slide_w, slide_h) = self.slide_size();
        let (part, d) = self.slide_doc(i)?;
        let tree = d.root.child("p:cSld").and_then(|c| c.child("p:spTree")).ok_or("the slide has no shape tree")?;
        struct Item {
            at: usize,
            frame: Rect,
            /// The box the shape is seen to fill: (left, top, width, height).
            seen: (f64, f64, f64, f64),
        }
        let mut items: Vec<Item> = Vec::new();
        for r in refs {
            let k = Self::locate(tree, r)?;
            if items.iter().any(|it| it.at == k) {
                return Err("the same shape is listed twice".into());
            }
            let Node::Elem(node) = &tree.children[k] else { unreachable!() };
            let s = shape_elem(node).unwrap();
            let frame = match xfrm_of(s).and_then(rect_of_xfrm) {
                Some(f) => f,
                None => match ph_key(s) {
                    Some(key) if s.name == "p:sp" => self.inherited_rect(&part, &key)?.ok_or("a placeholder has no position of its own or from its layout")?,
                    _ => return Err("a shape has no position to align".into()),
                },
            };
            let rot = xfrm_of(s).and_then(|x| x.attr("rot")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 60000.0;
            let (sin, cos) = rot.to_radians().sin_cos();
            let (bw, bh) = (frame.w * cos.abs() + frame.h * sin.abs(), frame.w * sin.abs() + frame.h * cos.abs());
            let (cx, cy) = (frame.x + frame.w / 2.0, frame.y + frame.h / 2.0);
            items.push(Item { at: k, frame, seen: (cx - bw / 2.0, cy - bh / 2.0, bw, bh) });
        }
        let (rl, rt, rr, rb) = match to {
            AlignTo::Slide => (0.0, 0.0, slide_w, slide_h),
            AlignTo::Selection => items.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(l, t, r, b), it| {
                (l.min(it.seen.0), t.min(it.seen.1), r.max(it.seen.0 + it.seen.2), b.max(it.seen.1 + it.seen.3))
            }),
        };
        let (rcx, rcy) = ((rl + rr) / 2.0, (rt + rb) / 2.0);
        let mut moves: Vec<(ShapeRef, f64, f64)> = Vec::new();
        for (r, it) in refs.iter().zip(&items) {
            let (l, t, w, h) = it.seen;
            let (dx, dy) = match how {
                Align::Left => (rl - l, 0.0),
                Align::Right => (rr - (l + w), 0.0),
                Align::Center => (rcx - (l + w / 2.0), 0.0),
                Align::Top => (0.0, rt - t),
                Align::Bottom => (0.0, rb - (t + h)),
                Align::Middle => (0.0, rcy - (t + h / 2.0)),
            };
            if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                moves.push((r.clone(), it.frame.x + dx, it.frame.y + dy));
            }
        }
        for (r, x, y) in moves {
            self.place_shape(i, &r, Some(x), Some(y), None, None)?;
        }
        Ok(())
    }
}
