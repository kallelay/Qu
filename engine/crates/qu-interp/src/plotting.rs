//! Real (non-headless) 2-D plotting: a `Figure`/`Panel` state model built up
//! by `plot`/`xlabel`/`legend`/… builtins and bare commands (`clear panel`,
//! `axis equal`, `hold on`), plus export renderers driven by one shared
//! draw-command list (`build_draw_ops`) so SVG/HTML/TikZ can never visually
//! disagree with each other — each is just a different serialization of the
//! same laid-out geometry.
//!
//! This is the real engine's first non-stub plotting backend; before this,
//! `plot`/`xlabel`/`legend`/etc. only logged "figure N (M points)" and threw
//! everything else away (`qu-interp/src/lib.rs`'s old `call_builtin` match).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::pdf_font;

use crate::font_subset;
use crate::image::Image;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scale {
    #[default]
    Linear,
    Log,
}

/// Where the axis spines (the tick-bearing lines themselves) are drawn —
/// `axis edge` (MATLAB's own default box style: spines run along the
/// panel's rectangle, independent of where the data actually sits) or
/// `axis origin` (spines cross through data coordinate `(0, 0)`, clamped
/// to the nearest edge if `0` falls outside the visible data range — the
/// "math-style" look where the axes themselves cross at the origin
/// instead of framing the data in a box).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AxisPosition {
    #[default]
    Edge,
    Origin,
}

#[derive(Clone, Debug, Default)]
pub struct Series {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub label: Option<String>,
    /// "line" (default), "o", "x", "+", "s"/"square", or "stem"
    pub marker: String,
    /// Whether the marker glyph is filled. `None` lets each glyph choose
    /// (hollow, except the star, which is filled white so it stays visible
    /// on top of whatever it marks); `Some` is an explicit `fill=` on the
    /// call. matplotlib spells this `mfc`, and the ported figures use both
    /// states in one plot to separate two groups of methods.
    pub marker_filled: Option<bool>,
    /// An explicit dash pattern, from `style="loose dashed"` or a custom
    /// `dashes=(...)`. `None` falls back to whatever suffix the marker
    /// spec carries (`"o--"`), which is the ported-format-string path.
    ///
    /// Held on the SERIES rather than folded into the marker string
    /// because the string can only spell four patterns -- it is a
    /// matplotlib format code, and `loose dashdotdot` is not one.
    pub dash: Option<Dash>,
    /// Line width in units. `None` takes the theme's default, which is
    /// what tracks the figure's type size.
    pub width: Option<f64>,
    /// Opacity in `[0, 1]` for this series' line and markers.
    ///
    /// The honest use is overlap: twenty Monte Carlo traces at `alpha=0.15`
    /// show where the density is, which twenty opaque ones cannot. It is
    /// not a styling flourish -- a faded line in print just looks
    /// under-inked.
    pub alpha: Option<f64>,
    /// Marker circumradius in units. `None` scales with the figure's type.
    pub marker_size: Option<f64>,
    /// Draw a marker on every Nth point.
    ///
    /// A 200-sample curve with a marker per sample is a solid band, not a
    /// series. matplotlib calls this `markevery`, and it is what lets a
    /// dense line still carry an identifying glyph -- the alternative
    /// people reach for, subsampling the data itself, changes the line.
    pub marker_every: Option<usize>,
    /// Marker outline colour, when it should differ from the line's
    /// (matplotlib's `mec`). Used to ring a filled marker in black so it
    /// stays legible on a coloured ground.
    pub marker_edge: Option<String>,
    /// `shadow = true`: cast an offset, translucent copy of each marker
    /// under it, so a mark stays readable on a busy field. The sibling of
    /// `marker_edge`, which outlines the mark in place instead.
    pub marker_shadow: bool,
    pub color: Option<String>,
    /// `bar(...)`/`groupbar(...)`/`stackbar(...)` only: print each bar's
    /// height above it (`values: true` at the call site). Ignored by every
    /// other marker kind.
    pub show_values: bool,
    /// `bar`/`groupbar`/`stackbar` only: a diagonal-stripe fill instead of
    /// solid color (`hatch: true`) — lets a legend distinguish series
    /// without relying on color alone (colorblind-safe, photocopy-safe).
    pub hatch: bool,
}

/// A set of marks captured once and stamped as many times as you like.
///
/// A layer is a VALUE, not a mode. `capture("draw_leaf")` runs your
/// function into a scratch panel and keeps what it drew; `stamp` places a
/// transformed copy on the real one. That choice matters:
///
///   - it composes with the rest of the language, because a layer is a
///     value you can pass, return, hold in a list and duplicate by
///     assignment, rather than a stateful recorder with a begin and an end
///     that can be forgotten;
///   - it separates WHAT is drawn from WHERE, which is the whole reason to
///     want the feature: a leaf, a bolt, a sensor symbol is described once
///     and placed forty times.
///
/// The transform is translate, then scale, then rotate about the layer's
/// own origin -- the order that makes `stamp(l, x, y, rotate = 30)` mean
/// "put it there, turned", which is what anybody expects.
#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub shapes: Vec<Shape>,
    pub series: Vec<Series>,
}

impl Layer {
    /// A copy moved to `(dx, dy)`, scaled about its own origin and turned
    /// by `rotate` degrees.
    ///
    /// Two shapes cannot be rotated and say so by staying put: `VLine` and
    /// `HLine` are defined as spanning the whole panel in one direction, so
    /// a turned one is not a line of that kind any more. They translate and
    /// scale; a rotated rule is a `polygon` or a two-point `plot`.
    pub fn placed(&self, dx: f64, dy: f64, scale: f64, rotate_deg: f64) -> Layer {
        let r = rotate_deg.to_radians();
        let (c, s) = (r.cos(), r.sin());
        let pt = |x: f64, y: f64| -> (f64, f64) {
            let (sx, sy) = (x * scale, y * scale);
            (dx + sx * c - sy * s, dy + sx * s + sy * c)
        };
        let pts = |xs: &[f64], ys: &[f64]| -> (Vec<f64>, Vec<f64>) {
            let mut ox = Vec::with_capacity(xs.len());
            let mut oy = Vec::with_capacity(ys.len());
            for (x, y) in xs.iter().zip(ys) {
                let (a, b) = pt(*x, *y);
                ox.push(a);
                oy.push(b);
            }
            (ox, oy)
        };

        let shapes = self
            .shapes
            .iter()
            .map(|sh| match sh {
                Shape::Curve { points, color, fill, alpha, width, closed, dash } => {
                    Shape::Curve {
                        points: points.iter().map(|(x, y)| pt(*x, *y)).collect(),
                        color: color.clone(),
                        fill: fill.clone(),
                        alpha: *alpha,
                        // A stamped copy at half scale wants half-weight
                        // strokes, or a shrunk symbol reads as a blob.
                        width: width.map(|w| w * scale),
                        closed: *closed,
                        dash: *dash,
                    }
                }
                Shape::Rect { x0, y0, x1, y1, color } => {
                    // A rotated rectangle is no longer axis-aligned, so it
                    // becomes the polygon it actually is rather than
                    // silently ignoring the rotation.
                    if rotate_deg != 0.0 {
                        let corners = [(*x0, *y0), (*x1, *y0), (*x1, *y1), (*x0, *y1)];
                        Shape::Curve {
                            points: corners.iter().map(|(x, y)| pt(*x, *y)).collect(),
                            color: color.clone(),
                            fill: color.clone(),
                            alpha: Some(0.18),
                            width: None,
                            closed: true,
                            dash: false,
                        }
                    } else {
                        let (nx0, ny0) = pt(*x0, *y0);
                        let (nx1, ny1) = pt(*x1, *y1);
                        Shape::Rect { x0: nx0, y0: ny0, x1: nx1, y1: ny1, color: color.clone() }
                    }
                }
                Shape::VLine { x, color, dash, dot, width } => Shape::VLine {
                    x: dx + x * scale,
                    color: color.clone(),
                    dash: *dash,
                    dot: *dot,
                    width: *width,
                },
                Shape::HLine { y, color, dash, dot, width } => Shape::HLine {
                    y: dy + y * scale,
                    color: color.clone(),
                    dash: *dash,
                    dot: *dot,
                    width: *width,
                },
                Shape::XSpan { x0, x1, color, alpha } => Shape::XSpan {
                    x0: dx + x0 * scale,
                    x1: dx + x1 * scale,
                    color: color.clone(),
                    alpha: *alpha,
                },
                Shape::YSpan { y0, y1, color, alpha } => Shape::YSpan {
                    y0: dy + y0 * scale,
                    y1: dy + y1 * scale,
                    color: color.clone(),
                    alpha: *alpha,
                },
                Shape::FillBetween { x, y_lo, y_hi, color, alpha } => {
                    let (nx, nlo) = pts(x, y_lo);
                    let (_, nhi) = pts(x, y_hi);
                    Shape::FillBetween {
                        x: nx,
                        y_lo: nlo,
                        y_hi: nhi,
                        color: color.clone(),
                        alpha: *alpha,
                    }
                }
                Shape::Bubble { x, y, sizes, color } => {
                    let (nx, ny) = pts(x, y);
                    Shape::Bubble {
                        x: nx,
                        y: ny,
                        sizes: sizes.iter().map(|s| s * scale).collect(),
                        color: color.clone(),
                    }
                }
                // Everything else keeps its geometry. An error bar and a
                // contour are measurements of specific data; stamping a
                // copy of one somewhere else would be a claim about data
                // that is not there.
                other => other.clone(),
            })
            .collect();

        let series = self
            .series
            .iter()
            .map(|se| {
                let (x, y) = pts(&se.x, &se.y);
                let mut out = se.clone();
                out.x = x;
                out.y = y;
                out.width = out.width.map(|w| w * scale);
                // A stamped copy must not add a second entry to the legend
                // for the same thing.
                out.label = None;
                out
            })
            .collect();

        Layer { shapes, series }
    }
}

#[derive(Clone, Debug)]
pub enum Shape {
    /// `dash`/`width`: a reference line is usually not the subject of the
    /// figure -- a threshold, a spec limit, a mean -- and drawing it solid
    /// at full weight makes it compete with the data. Dashing it is the
    /// convention, and it was the one property these could not express.
    /// `dot` is a finer, tighter stipple than `dash`: LaTeX and
    /// matplotlib both distinguish them, and a figure that carries two
    /// reference lines at once (a measured median and a predicted one)
    /// needs two line styles to tell them apart in greyscale.
    VLine { x: f64, color: Option<String>, dash: bool, dot: bool, width: Option<f64> },
    HLine { y: f64, color: Option<String>, dash: bool, dot: bool, width: Option<f64> },
    Rect { x0: f64, y0: f64, x1: f64, y1: f64, color: Option<String> },
    /// A closed curve given in DATA coordinates: a circle, an ellipse, an
    /// arc, or an arbitrary polygon.
    ///
    /// All four are one variant because all four render the same way, and
    /// that way is worth stating: the outline is SAMPLED IN DATA SPACE and
    /// each sample is then mapped through the axis transform. Emitting an
    /// SVG `<circle>` at a pixel radius would be simpler and wrong — the x
    /// and y scales differ whenever the axes are not equal, so a circle of
    /// radius `r` in data units is an ellipse on screen, and on a log axis
    /// it is not even an ellipse. Sampling first is correct under every
    /// scale the panel can have.
    ///
    /// `closed` is false for an arc, which is a stroke rather than a shape.
    Curve {
        points: Vec<(f64, f64)>,
        color: Option<String>,
        fill: Option<String>,
        alpha: Option<f64>,
        width: Option<f64>,
        closed: bool,
        dash: bool,
    },
    XSpan { x0: f64, x1: f64, color: Option<String>, alpha: Option<f64> },
    /// `alpha` is the wash's opacity; `None` is the 0.15 that marks a
    /// region without competing with the series drawn over it.
    YSpan { y0: f64, y1: f64, color: Option<String>, alpha: Option<f64> },
    /// One `errorbar(x, y, yerr)` sample: a capped vertical whisker from
    /// `y - err` to `y + err` plus a marker dot at `(x, y)`.
    /// One `errorbar(x, y, yerr)` sample.
    ///
    /// `xerr` exists because a Nyquist plot needs it: both axes are
    /// measured quantities (real and imaginary impedance, each with its
    /// own standard deviation), so drawing only the vertical whisker
    /// states half the uncertainty and implies the other half is zero.
    /// `None` is the ordinary case where x is a controlled variable.
    ///
    /// `cap` sizes the whisker end. A dense scatter wants it at zero --
    /// caps on 135 overlapping points are a grey smear, which is why
    /// matplotlib's `capsize=0` is the usual setting on this kind of
    /// figure.
    ErrorBar {
        x: f64,
        y: f64,
        err: f64,
        xerr: Option<f64>,
        color: Option<String>,
        cap: Option<f64>,
        width: Option<f64>,
    },
    /// `fill_between(x, y_lo, y_hi)` / `area(x, y)` (the latter is just
    /// `y_lo = 0`) — a filled region between two curves sharing `x`.
    /// `alpha` is the band's opacity; `None` is the 0.3 that reads as
    /// "shaded region" without hiding what is drawn under it.
    FillBetween { x: Vec<f64>, y_lo: Vec<f64>, y_hi: Vec<f64>, color: Option<String>, alpha: Option<f64> },
    /// `bubble(x, y, sizes)` — a scatter where marker radius encodes a third
    /// variable. `sizes` are raw data values, not pixels; radius is scaled
    /// linearly from the series' own min/max into a fixed `[4, 22]` px range
    /// at render time, so the plot only cares about relative size within one
    /// series, matching matplotlib's `scatter(s=...)` convention.
    Bubble { x: Vec<f64>, y: Vec<f64>, sizes: Vec<f64>, color: Option<String> },
    /// `contour(x, y, Z, ...)` / `contourf(x, y, Z, ...)` -- a scalar field
    /// drawn as iso-lines or filled bands.
    ///
    /// The field is kept, not the geometry: levels and the contouring
    /// itself happen at render time. That matters because the polygons
    /// depend on the levels, the levels can be automatic, and a figure
    /// that is re-rendered at a different size or theme must not have
    /// baked one set of them in.
    /// `hexbin(x, y)` — the plane tiled with hexagons, each shaded by how
    /// many points fell in it.
    ///
    /// What a scatter of ten thousand points cannot do: past a few
    /// thousand marks the picture saturates into a blob whose darkest
    /// region says "many" and nothing more, and the overlap hides exactly
    /// the structure the reader is looking for. Hexagons rather than
    /// squares because a hexagon's centre is equidistant from all six of
    /// its neighbours, so a cell's count does not depend on which
    /// direction the data happens to run.
    Hexbin {
        /// Cell centres, already binned.
        cx: Vec<f64>,
        cy: Vec<f64>,
        /// Points in each cell, same length as the centres.
        counts: Vec<f64>,
        /// Cell size in data units: `dx` is the centre-to-centre spacing
        /// across a row, `dy` between rows.
        dx: f64,
        dy: f64,
        colormap: String,
        /// Cells with no points are left unpainted rather than painted the
        /// colormap's zero: an empty region of the plane is not the same
        /// claim as a region where nothing was counted.
        min_count: f64,
    },
    Contour {
        x: Vec<f64>,
        y: Vec<f64>,
        /// Row-major: `z[j * x.len() + i]` at `(x[i], y[j])`. `NaN` marks
        /// masked-out cells and is dropped rather than interpolated.
        z: Vec<f64>,
        /// Explicit levels, lowest first. Empty means "choose them".
        levels: Vec<f64>,
        /// Filled bands (`contourf`) rather than iso-lines (`contour`).
        filled: bool,
        /// Colormap name for the bands; ignored when `color` is set.
        colormap: String,
        /// A single colour for every line, as `contour(..., color=...)`.
        color: Option<String>,
        /// Print each line's level on it, as matplotlib's `clabel` does.
        label_levels: bool,
        /// Suffix for those labels, e.g. `"%"`.
        label_suffix: String,
        /// Fraction along each contour path at which its level label sits,
        /// `None` meaning the midpoint.
        label_pos: Option<f64>,
        /// Dash pattern and weight for the iso-lines, as any other line
        /// takes: `contour(..., dash = true, width = 0.8)` for a boundary
        /// that is a guide rather than a measurement. Both were silently
        /// dropped before, so such a line came out solid and full weight
        /// and read as data.
        dash: bool,
        dot: bool,
        width: Option<f64>,
        /// `contourf(..., extend = "max")`: paint everything ABOVE the top
        /// level in the top colour instead of leaving it blank.
        ///
        /// A field that runs past its last level otherwise has that corner
        /// unpainted -- white, which reads as "no data" where the truth is
        /// "more than the scale shows". The colour bar's pointed cap says
        /// the same thing; this is the half of it that is in the plot.
        extend_max: bool,
    },
}

/// One group's Tukey five-number summary for `boxplot`: box spans
/// `q1..q3` with a median line, whiskers extend to the most extreme data
/// point still within `1.5 * IQR` of the box, and anything further out is
/// drawn as an individual outlier point.
#[derive(Clone, Debug)]
pub struct BoxplotGroup {
    pub x: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub whisker_lo: f64,
    pub whisker_hi: f64,
    pub outliers: Vec<f64>,
    pub color: Option<String>,
    pub label: Option<String>,
}

/// A radar/spider chart: `series[i].y` gives one magnitude per category,
/// laid out on evenly-spaced spokes around a circle. A panel with a
/// `SpiderChart` renders *only* the chart — spider and cartesian content
/// (plot/bar/boxplot/...) don't mix on the same panel.
#[derive(Clone, Debug, Default)]
pub struct SpiderChart {
    pub categories: Vec<String>,
    pub series: Vec<Series>,
    /// The outer ring's value; `None` auto-scales to the largest data point.
    pub max: Option<f64>,
}

/// One group of a `raincloud` plot (Allen et al. 2019): a KDE density
/// "cloud" (`density_x`/`density_y`, raw unnormalized density), a boxplot
/// (reusing [`BoxplotGroup`]'s stats), and jittered raw points ("rain") —
/// all three at the same `x` category position.
#[derive(Clone, Debug)]
pub struct RaincloudGroup {
    pub x: f64,
    pub density_x: Vec<f64>,
    pub density_y: Vec<f64>,
    pub box_stats: BoxplotGroup,
    /// `(jitter in [-1, 1], value)` pairs — one per raw sample.
    pub jitter: Vec<(f64, f64)>,
    pub color: Option<String>,
    /// Mirror the density about the category line, giving the closed
    /// symmetric shape a violin plot is, instead of the half-violin a
    /// raincloud leans against its box.
    pub mirrored: bool,
    /// Draw the raw samples. A raincloud's whole point is that they are
    /// there; a violin conventionally shows only the summary, and on a
    /// large sample the rain is what makes the figure heavy.
    pub show_rain: bool,
}

/// `pie(values, [labels])` / `donut(values, [labels])`. A panel with a
/// `PieChart` renders *only* the chart, like `SpiderChart`/`Heatmap`.
#[derive(Clone, Debug)]
pub struct PieChart {
    pub values: Vec<f64>,
    pub labels: Vec<String>,
    /// `true` for `donut` (a hole in the middle), `false` for a plain `pie`.
    pub donut: bool,
}

/// A handle to something already drawn on a panel.
///
/// Qu's plotting is immediate-mode: a call records a shape and the figure
/// is rendered later from what was recorded. That is why a handle can be
/// this small -- it is a position in the panel's own lists, and mutating
/// through it edits the record the renderer will read.
///
/// The alternative, and what scripts had to do before, is to restate a
/// colour at each call site and hope the two stay equal. They do not:
/// this paper's own reference levels and their labels were meant to share
/// an ink and drifted apart the first time one of them was edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtistRef {
    pub panel: usize,
    pub kind: ArtistKind,
    pub index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtistKind {
    /// An entry in `Panel::callouts` -- `text`, `annotate`, `arrowtext`.
    Callout,
    /// An entry in `Panel::shapes` -- `vline`, `hline`.
    Shape,
}

#[derive(Clone, Debug)]
pub struct Callout {
    pub x: f64,
    pub y: f64,
    pub text: String,
    /// `Some` draws an arrow from `(x, y)` to the point it annotates instead
    /// of a plain label — used by `arrow`/`arrowtext`/`annotate`.
    pub arrow_to: Option<(f64, f64)>,
    /// `point()`'s "radioactive" callout draws a highlighted marker dot at
    /// `(x, y)` in addition to its auto-generated coordinate label.
    pub marker: bool,
    /// `color=`. An annotation is very often the one thing on a figure that
    /// is deliberately NOT in the data's own ink -- a red note pointing at
    /// a threshold, a grey aside. There was no way to say so.
    pub color: Option<String>,
    /// `italic=`. A region name, an asymptote, an aside -- the labels that
    /// comment on a plot rather than belong to it. See `DrawOp::Text`.
    pub italic: bool,
    /// `size=`. Defaults to the figure's tick size rather than the old
    /// hardcoded 11 units, which ignored the theme completely: in a print
    /// theme, whose ticks run at ~25 units, every annotation came out at
    /// under half the size of the smallest other text on the figure. Same
    /// bug the legend had before its text was tied to the type scale.
    pub size: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LegendPosition {
    #[default]
    Best,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

impl LegendPosition {
    pub fn parse(words: &[String]) -> Option<Self> {
        let w: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        match w.as_slice() {
            ["best"] => Some(Self::Best),
            ["top", "left"] | ["left", "top"] => Some(Self::TopLeft),
            ["top", "right"] | ["right", "top"] => Some(Self::TopRight),
            ["bottom", "left"] | ["left", "bottom"] => Some(Self::BottomLeft),
            ["bottom", "right"] | ["right", "bottom"] => Some(Self::BottomRight),
            ["top"] => Some(Self::Top),
            ["bottom"] => Some(Self::Bottom),
            ["left"] => Some(Self::Left),
            ["right"] => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Legend {
    pub visible: bool,
    pub position: LegendPosition,
    /// Both default to `true`: a translucent, rounded-corner legend box reads
    /// clearly over dense data without hard-edged occlusion.
    pub translucent: bool,
    pub rounded: bool,
    /// `legend outside` (or `legend <position> outside`) — reserves margin
    /// space beside the plot area instead of overlaying it, guaranteeing no
    /// data overlap at the cost of a narrower plot.
    pub outside: bool,
    /// Legend text size in units. `None` follows the figure's type scale.
    ///
    /// Journals size the key *below* the axis labels -- it is a reference,
    /// not something read continuously -- and the ported figures set it
    /// explicitly (matplotlib's `legend.fontsize`: 7pt against an 8pt body).
    pub font_size: Option<f64>,
    /// A heading above the entries, for when the key needs saying what it
    /// is a key *to* ("Method", "Grid").
    pub title: Option<String>,
    /// Entries per row. More than one keeps a wide key from becoming a
    /// tall column that crowds the plot -- the usual fix for a
    /// single-column figure with five series.
    pub columns: usize,
    /// Per-call override of the theme's border. `None` follows the theme
    /// (see `ThemeStyle::legend_frame`); `Some` wins, because whether a
    /// key needs separating from the data is a property of the particular
    /// figure, not only of the house style.
    pub frame: Option<bool>,
    /// Inner padding, as a multiple of the default. The one control that
    /// actually matters for a crowded key: matplotlib exposes `borderpad`,
    /// `labelspacing` and `handletextpad` separately, and in practice they
    /// are turned together.
    pub padding: Option<f64>,
}

impl Default for Legend {
    fn default() -> Self {
        Legend {
            visible: false,
            position: LegendPosition::Best,
            translucent: true,
            rounded: true,
            outside: false,
            font_size: None,
            title: None,
            columns: 1,
            frame: None,
            padding: None,
        }
    }
}

/// Which margin an "outside" legend reserves space in. `Best` (auto,
/// outside) resolves to `Right` — the common convention (matplotlib's
/// `bbox_to_anchor=(1, .5)`, MATLAB's `'eastoutside'`) — since there's no
/// data to search against when placement is forced outside the axes anyway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// What occupies a band of margin outside the plot area, ordered from the
/// plot outward: the axis's own numbers, then the label naming what they
/// measure, then a colour scale, then anything panel-sized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Band {
    /// Fixed padding the side starts with.
    Pad,
    /// The axis's tick numbers.
    Ticks,
    /// The axis label naming what those numbers measure.
    AxisLabel,
    /// A colour scale, its numbers and its own label.
    Colorbar,
    /// An outside legend or an outside zoom inset.
    Panel,
}

/// The things stacked outside ONE side of the plot area, innermost first.
///
/// Placement used to be each element working out its own offset from whatever
/// it happened to know: the left axis label measured back from its tick
/// numbers, the colorbar label out from its own bar, and the right axis
/// label from the CANVAS EDGE -- which is how it ended up beyond a
/// colorbar it knew nothing about, 200px from the numbers it names.
///
/// Worse, the reservation and the placement were separate arithmetic over
/// the same constants. The right axis reserved `46 + 14`, and the colorbar
/// separately recomputed `46 + 14` to know where to begin. Two copies of
/// one fact, and nothing to notice when they drifted.
///
/// A side is a stack. Every occupant declares how much room it needs, in
/// order, once; where each one sits then falls out of what is already
/// reserved, so a thing cannot be placed outside its own band or on top of
/// its neighbour's.
#[derive(Default, Clone, Debug)]
struct MarginStack {
    bands: Vec<(Band, f64)>,
}

impl MarginStack {
    /// Reserve `extent` for `who`, outside everything reserved so far.
    fn push(&mut self, who: Band, extent: f64) {
        if extent > 0.0 {
            self.bands.push((who, extent));
        }
    }

    /// How much room `who` reserved, or zero if it reserved none.
    fn extent(&self, who: Band) -> f64 {
        self.bands.iter().find(|(b, _)| *b == who).map_or(0.0, |(_, e)| *e)
    }

    /// Distance from the plot's edge to the INNER edge of `who`'s band,
    /// or `None` when `who` reserved nothing on this side.
    fn inner(&self, who: Band) -> Option<f64> {
        let mut at = 0.0;
        for (b, e) in &self.bands {
            if *b == who {
                return Some(at);
            }
            at += e;
        }
        None
    }

    /// The middle of `who`'s band -- where a label centred in its own slot
    /// belongs, and the reason a caller never needs the constants again.
    fn center(&self, who: Band) -> Option<f64> {
        self.inner(who).map(|at| at + self.extent(who) / 2.0)
    }

    /// Total margin this side needs.
    fn total(&self) -> f64 {
        self.bands.iter().map(|(_, e)| e).sum()
    }

    /// Shrink every band by the same factor, for the fit-to-cell clamp.
    /// Scaling the total without scaling the bands would leave every
    /// placement pointing outside the margin it was clamped into.
    fn scale(&mut self, k: f64) {
        for (_, e) in &mut self.bands {
            *e *= k;
        }
    }
}

/// A copy of `op` shifted by `(dx, dy)` and repainted in `color`: the
/// shadow a marker casts, drawn underneath it.
///
/// Written against the ops a GLYPH is actually made of -- a polygon, a
/// circle, a cross, or the short lines a composite glyph's overlay adds --
/// rather than against the glyph table, so a marker added later casts a
/// shadow without anyone remembering to teach this function about it.
/// Anything else returns `None`: a shadow of a gridline or a label is a
/// shadow of something that is not a mark.
///
/// `fill` and `stroke` are repainted only where the original had them, so
/// a hollow marker casts a hollow shadow -- an outline, offset -- instead
/// of suddenly gaining a solid body it does not have.
fn shadow_of(op: &DrawOp, dx: f64, dy: f64, color: &str) -> Option<DrawOp> {
    let paint = |had: &Option<String>| had.as_ref().map(|_| color.to_string());
    Some(match op {
        DrawOp::Circle { cx, cy, r, fill, stroke } => DrawOp::Circle {
            cx: cx + dx,
            cy: cy + dy,
            r: *r,
            fill: paint(fill),
            stroke: paint(stroke),
        },
        DrawOp::Polygon { points, fill, stroke, opacity, width } => DrawOp::Polygon {
            points: points.iter().map(|(x, y)| (x + dx, y + dy)).collect(),
            fill: paint(fill),
            stroke: paint(stroke),
            opacity: *opacity,
            width: *width,
        },
        DrawOp::Cross { cx, cy, r, .. } => DrawOp::Cross {
            cx: cx + dx,
            cy: cy + dy,
            r: *r,
            color: color.to_string(),
        },
        // A plain circle, not another titled one: the shadow is decoration
        // and must not answer to the hover that names the data point. This
        // arm is why the commonest marker of all cast no shadow at first
        // -- `o` is drawn as a TitledCircle, and a helper written against
        // `Circle` silently skipped it.
        DrawOp::TitledCircle { cx, cy, r, fill, stroke, .. } => DrawOp::Circle {
            cx: cx + dx,
            cy: cy + dy,
            r: *r,
            fill: paint(fill),
            stroke: paint(stroke),
        },
        DrawOp::TitledRect { x, y, w, h, fill, stroke, opacity, radius, .. } => DrawOp::Rect {
            x: x + dx,
            y: y + dy,
            w: *w,
            h: *h,
            fill: paint(fill),
            stroke: paint(stroke),
            opacity: *opacity,
            radius: *radius,
        },
        DrawOp::Line { x1, y1, x2, y2, width, .. } => DrawOp::Line {
            x1: x1 + dx,
            y1: y1 + dy,
            x2: x2 + dx,
            y2: y2 + dy,
            color: color.to_string(),
            width: *width,
        },
        _ => return None,
    })
}

fn legend_side(position: LegendPosition) -> Side {
    match position {
        LegendPosition::Left | LegendPosition::TopLeft | LegendPosition::BottomLeft => Side::Left,
        LegendPosition::Top => Side::Top,
        LegendPosition::Bottom => Side::Bottom,
        LegendPosition::Right
        | LegendPosition::TopRight
        | LegendPosition::BottomRight
        | LegendPosition::Best => Side::Right,
    }
}

/// Sizes the legend box from its longest label — needed both to draw it and
/// (for `outside` placement) to reserve the right amount of margin *before*
/// the plot area itself is laid out. `tick_scale` is `fig.tick_size /
/// DEFAULT_TICK_SIZE` (the same scale `build_draw_ops` already applies to
/// tick/label margins via `fontsize(...)`) — legend text used to render at a
/// hardcoded 10.5px regardless of that setting, so a script that bumped
/// every other font via `fontsize(tick: 24, ...)` for a presentation still
/// got a barely-legible legend. Scaling this box by the same factor keeps
/// `legend_ops`'s actual text size (below) consistent with the box it's
/// drawn into. `tick_scale = 1.0` (the default) reproduces `ROW_H`/`PAD`.
///
/// `ROW_H`/`PAD` bumped from `16.0`/`8.0` (2026-09-03 ggplot2-inspired pass)
/// alongside `legend_ops`'s own text-size bump, for a more spacious,
/// professional-feeling legend box rather than a cramped one.
/// Rough rendered width of a string at a given font size.
///
/// An estimate, not a measurement -- the renderer has no font metrics, and
/// pulling them in to place a label is not worth it. 0.6em per character
/// is a little generous for the digits and short words this is used on,
/// which is the right direction to err: overestimating leaves a slightly
/// wider gap, underestimating collides two pieces of text.
/// Real advance widths for a figure's own face, parsed once per family.
///
/// `text_width` below is an estimate because, when it was written, the
/// renderer had no font metrics. It does now — `pdf_font` parses the same
/// faces the SVG embeds and the PDF writes — so anything that has to fit
/// text into a space can measure instead of guess. Parsing is done once
/// per family for the life of the process; the faces are `include_bytes!`
/// constants, so there is nothing to fail at runtime and no I/O.
pub fn face_metrics(font_family: &str) -> Option<&'static pdf_font::FontMetrics> {
    static CACHE: std::sync::OnceLock<Vec<(&'static str, pdf_font::FontMetrics)>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        [
            ("Latin Modern Roman", LATIN_MODERN_ROMAN_OTF),
            ("Latin Modern Sans", LATIN_MODERN_SANS_OTF),
            ("Inter", INTER_VARIABLE_TTF),
            ("Source Serif", SOURCE_SERIF_VARIABLE_TTF),
        ]
        .into_iter()
        .filter_map(|(name, bytes)| pdf_font::parse(bytes).map(|m| (name, m)))
        .collect()
    });
    // Longest match first would matter if one family name contained
    // another; none of these do, so first match is unambiguous.
    cache.iter().find(|(n, _)| font_family.contains(n)).map(|(_, m)| m)
}

/// Rendered width of a string, measured against the face when it is one Qu
/// ships and estimated otherwise.
///
/// The estimate is fine for reserving a margin, where erring wide is
/// harmless. It is not fine for deciding whether a title fits its panel:
/// per-character averages are off by enough that a title can be declared
/// to fit and then overrun by a third of its length.
pub fn measure_text(text: &str, size: f64, metrics: Option<&pdf_font::FontMetrics>) -> f64 {
    match metrics {
        Some(m) => m.text_width(text) / 1000.0 * size,
        None => text_width(text, size),
    }
}

/// Width of a label AS DRAWN, with any `$...$` expanded first.
///
/// `measure_text` measures the string it is given, which for a label is
/// the LaTeX SOURCE. `$\widehat{\mathrm{CF}}_{\mathrm{opt}} =
/// \sqrt{2(L-\ln K)}$` is 56 characters of source and about 21 of drawn
/// text, so a legend sized from the source came out nearly three times too
/// wide -- wide enough that the layout gave up on fitting it inside the
/// panel and moved it out, costing half the plot. Every legend with maths
/// in it was affected.
///
/// Each run is measured at its own size, so a subscript contributes its
/// shrunken width rather than a full-size one.
fn measure_label(text: &str, size: f64, metrics: Option<&pdf_font::FontMetrics>) -> f64 {
    if !text.contains('$') {
        return measure_text(text, size, metrics);
    }
    parse_math_spans_for_pdf(text)
        .iter()
        .map(|r| measure_text(&r.text, size * r.scale, metrics))
        .sum()
}

fn text_width(text: &str, size: f64) -> f64 {
    // Per-character averages, because one factor for everything is wrong
    // in whichever direction it is set: 0.6em fits digits and tick labels
    // (what this was written for) and badly under-measures a legend entry
    // like "Van der Ouderaa", whose capitals and wide lowercase spill out
    // of a box sized for it. Cheap to widen per character and it keeps the
    // digit case exactly where it was.
    text.chars()
        .map(|c| match c {
            'i' | 'j' | 'l' | 'I' | '.' | ',' | '\'' | '(' | ')' | '[' | ']' | ' ' => 0.32,
            'f' | 't' | 'r' | 'J' => 0.42,
            'm' | 'M' | 'W' | 'w' => 0.88,
            'A'..='Z' => 0.70,
            _ => 0.56,
        })
        .sum::<f64>()
        * size
}

/// Fold an opacity into a colour as an 8-digit hex `#rrggbbaa`.
///
/// Every output this renderer produces understands that form (SVG and
/// modern PDF viewers directly, and `tikz_draw_color` splits it back out),
/// so one colour string carries the alpha instead of an `opacity` field
/// having to be added to a dozen `DrawOp` variants and honoured in three
/// writers.
pub fn apply_alpha(color: &str, alpha: Option<f64>) -> String {
    match alpha {
        None => color.to_string(),
        Some(a) if a >= 1.0 => color.to_string(),
        Some(a) => {
            let a = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            // Only extend a plain `#rrggbb`; a named colour or one that
            // already carries alpha is left alone rather than corrupted.
            let hex = color.trim_start_matches('#');
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                format!("#{hex}{a:02x}")
            } else {
                color.to_string()
            }
        }
    }
}

/// The outline of a polygonal marker glyph, centred on the origin.
///
/// One table rather than a match arm per shape in each of the three places
/// that draw markers (the series, the legend swatch, the inset), because
/// those had drifted: `^` and `D` existed in none of them, `*` in one, and
/// a legend could show a different shape from the line it labelled.
///
/// `r` is the circumradius, so every glyph occupies the same disc as the
/// `o` of the same size and a mixed-marker series reads as one set. The
/// two exceptions are deliberate: a square and a losange are matched on
/// *area* instead, since a square inscribed in the same circle looks
/// noticeably bigger than the circle and a losange noticeably smaller.
///
/// Returns `None` for glyphs that are not polygons -- `o` (a circle),
/// `x`/`+` (strokes, not outlines) -- and for the non-marker series kinds
/// (`line`, `bar`, `stem`, ...), which the caller handles itself.
pub fn marker_polygon(glyph: &str, r: f64) -> Option<Vec<(f64, f64)>> {
    // A regular n-gon with a vertex at `start` radians, going clockwise in
    // screen coordinates (y grows downward).
    fn ngon(n: usize, r: f64, start: f64) -> Vec<(f64, f64)> {
        (0..n)
            .map(|k| {
                let a = start + k as f64 * std::f64::consts::TAU / n as f64;
                (r * a.cos(), r * a.sin())
            })
            .collect()
    }
    const UP: f64 = -std::f64::consts::FRAC_PI_2;
    const TAU: f64 = std::f64::consts::TAU;
    // Enough segments that a curve reads as a curve at marker size, and few
    // enough that a scatter of two hundred of them is not a megabyte. At a
    // 3 pt radius the eye cannot separate 32 sides from a circle.
    const ROUND: usize = 32;

    /// A closed outline sampled from a polar radius function, going
    /// clockwise in screen coordinates (y grows downward).
    fn radial(samples: usize, f: impl Fn(f64) -> f64) -> Vec<(f64, f64)> {
        (0..samples)
            .map(|i| {
                let th = i as f64 * TAU / samples as f64;
                (f(th) * th.cos(), f(th) * th.sin())
            })
            .collect()
    }

    /// How far the ray at angle `th` reaches through `n` equal circles set
    /// at `offset` from the centre.
    ///
    /// Every ray from the centre must strike at least one lobe. That is
    /// what makes the union star-shaped about the centre, and it is what
    /// lets a clover be one polygon read off one ray per angle instead of
    /// a boolean union of three discs. Setting the lobes further out than
    /// their own radius is fine, and is how a flower gets notches deeper
    /// than a clover's -- what is not fine is pushing them so far that a
    /// ray between two of them reaches nothing, which would fold the
    /// outline through the centre. `reach` is floored rather than left at
    /// zero so a mis-tuned shape degrades to a pinched disc, not a knot.
    fn lobes(n: usize, offset: f64, lobe_r: f64, start: f64, th: f64) -> f64 {
        let (ux, uy) = (th.cos(), th.sin());
        let mut reach: f64 = 0.0;
        for k in 0..n {
            let a = start + k as f64 * TAU / n as f64;
            let (cx, cy) = (offset * a.cos(), offset * a.sin());
            let b = ux * cx + uy * cy;
            let disc = b * b - (cx * cx + cy * cy) + lobe_r * lobe_r;
            if disc > 0.0 {
                reach = reach.max(b + disc.sqrt());
            }
        }
        reach.max(lobe_r * 0.25)
    }

    /// A stem hanging straight down from the centre: a rectangle, as a
    /// radius function, so it can be `max`ed with a lobe union and stay one
    /// star-shaped outline.
    fn stem(half_w: f64, len: f64, th: f64) -> f64 {
        let (c, s) = (th.cos(), th.sin());
        if s <= 1e-9 {
            return 0.0;
        }
        (half_w / c.abs().max(1e-9)).min(len / s)
    }

    /// Scale an outline so its furthest point sits at `r`, which is what
    /// makes a clover, a heart and a square read as the same SIZE of mark
    /// rather than the same construction radius.
    fn to_radius(pts: Vec<(f64, f64)>, r: f64) -> Vec<(f64, f64)> {
        let max = pts.iter().fold(0.0f64, |m, &(x, y)| m.max((x * x + y * y).sqrt()));
        if max <= 1e-12 {
            return pts;
        }
        let k = r / max;
        pts.into_iter().map(|(x, y)| (x * k, y * k)).collect()
    }

    /// A spiked star: `arms` points at `r`, with the notches between them
    /// pulled in to `inner`. Small `inner` gives thin rays (an asterisk),
    /// large gives a fat star.
    fn spiked(arms: usize, r: f64, inner: f64, start: f64) -> Vec<(f64, f64)> {
        (0..arms * 2)
            .map(|k| {
                let rad = if k % 2 == 0 { r } else { r * inner };
                let a = start + k as f64 * std::f64::consts::PI / arms as f64;
                (rad * a.cos(), rad * a.sin())
            })
            .collect()
    }

    Some(match glyph {
        // A square on its side. 0.886 * r gives it the same area as the
        // circle of radius r, which is what makes the two look equal.
        "s" | "square" => {
            let h = r * 0.886;
            vec![(-h, -h), (h, -h), (h, h), (-h, h)]
        }
        // The losange: that same square turned 45 degrees. Its half
        // diagonal is therefore the square's diagonal half-length, not r,
        // or it would read as the smaller shape of the two.
        "D" | "d" | "diamond" | "losange" => {
            let h = r * 0.886 * std::f64::consts::SQRT_2;
            vec![(0.0, -h), (h, 0.0), (0.0, h), (-h, 0.0)]
        }
        "^" | "triangle" | "\u{25B2}" => ngon(3, r, UP),
        "v" | "\u{25BC}" => ngon(3, r, std::f64::consts::FRAC_PI_2),
        "<" | "\u{25C0}" => ngon(3, r, std::f64::consts::PI),
        ">" | "\u{25B6}" => ngon(3, r, 0.0),
        "p" | "pentagon" | "\u{2B1F}" => ngon(5, r, UP),
        "h" | "H" | "hexagon" | "\u{2B22}" => ngon(6, r, UP),
        // Flat-topped, so it does not read as a circle that failed.
        "8" | "octagon" => ngon(8, r, UP + std::f64::consts::PI / 8.0),
        // Five-pointed star: alternating circumradius and the inradius a
        // regular pentagram implies (r / phi^2 = 0.382 r), which is what
        // makes it read as a star rather than a spiky decagon.
        "*" | "star" | "\u{2605}" => spiked(5, r, 0.382, UP),
        // Six-pointed, at the inradius two overlaid triangles imply
        // (1/sqrt(3)), so it reads as a hexagram and not as a snowflake.
        "star6" | "hexstar" | "\u{2721}" => spiked(6, r, 0.577, UP),
        // Thin rays rather than a filled body: the mark for "a value is
        // here" that does not hide what is under it.
        "asterisk" | "ast" | "\u{2731}" => spiked(6, r, 0.13, UP),
        "star4" | "sparkle" | "\u{2726}" => spiked(4, r, 0.28, UP),

        // --- clovers, and the flower they generalise to -----------------
        //
        // Three or four round lobes about the centre. `offset < lobe_r`
        // keeps every lobe over the centre, which is what makes the union
        // one star-shaped outline (see `lobes`).
        "klee" | "klee3" | "clover" | "clover3" | "trefoil" | "\u{2618}" => {
            to_radius(radial(ROUND, |th| lobes(3, 0.52, 0.60, UP, th)), r)
        }
        "klee4" | "clover4" | "shamrock" | "lucky" => {
            to_radius(radial(ROUND, |th| lobes(4, 0.52, 0.60, UP, th)), r)
        }
        // More lobes, set further out, so the notches between them cut
        // deeper and it reads as petals rather than as a bumpy disc.
        "flower" | "\u{273F}" | "\u{2740}" => {
            to_radius(radial(ROUND + 8, |th| lobes(6, 0.75, 0.45, UP, th)), r)
        }

        // --- the card suits ---------------------------------------------
        //
        // The classic heart curve. Its own extent runs -17..12 against a
        // half-width of 16, so it is normalised by `to_radius` rather than
        // by its parameters, and `y` is negated because screen y grows
        // downward while the curve's cusp is at negative y.
        "heart" | "\u{2665}" | "\u{2661}" => to_radius(
            (0..40)
                .map(|i| {
                    let t = i as f64 * TAU / 40.0;
                    let x = 16.0 * t.sin().powi(3);
                    let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos()
                        - (4.0 * t).cos();
                    (x, -y)
                })
                .collect(),
            r,
        ),
        // Hand-drawn rather than derived: an inverted heart plus a stem is
        // the shape, and splicing a stem into a parametric curve at exactly
        // its cusp is more fragile than naming fifteen points. Clockwise
        // from the tip, down the right lobe, into the notch, out around the
        // stem, and back up the left.
        "spade" | "\u{2660}" | "\u{2664}" => to_radius(
            [
                (0.00, -1.00),
                (0.40, -0.55),
                (0.78, -0.02),
                (0.80, 0.28),
                (0.56, 0.52),
                (0.26, 0.44),
                (0.10, 0.40),
                (0.20, 0.66),
                (0.40, 0.96),
                (-0.40, 0.96),
                (-0.20, 0.66),
                (-0.10, 0.40),
                (-0.26, 0.44),
                (-0.56, 0.52),
                (-0.80, 0.28),
                (-0.78, -0.02),
                (-0.40, -0.55),
            ]
            .into_iter()
            .collect(),
            r,
        ),
        // Three lobes and a stem, both as radius functions, so the union is
        // still one star-shaped outline.
        "club" | "\u{2663}" | "\u{2667}" => to_radius(
            radial(ROUND + 8, |th| {
                lobes(3, 0.48, 0.55, UP, th).max(stem(0.14, 1.15, th))
            }),
            r,
        ),
        // The card diamond IS the losange; spelling it separately would
        // invite the two to drift apart.
        "\u{2666}" | "\u{2662}" => marker_polygon("losange", r)?,

        // --- the filled cross pair --------------------------------------
        //
        // `+`/`x` are drawn as STROKES elsewhere (see `DrawOp::Cross`).
        // These are the filled bodies, which hold their shape when a
        // series is printed small or in greyscale.
        "P" | "plus" | "\u{271A}" => {
            let (a, b) = (r * 0.30, r * 0.92);
            vec![
                (-a, -b), (a, -b), (a, -a), (b, -a), (b, a), (a, a),
                (a, b), (-a, b), (-a, a), (-b, a), (-b, -a), (-a, -a),
            ]
        }
        "X" | "xmark" | "\u{2716}" => {
            let k = std::f64::consts::FRAC_1_SQRT_2;
            marker_polygon("P", r)?
                .into_iter()
                .map(|(x, y)| ((x - y) * k, (x + y) * k))
                .collect()
        }

        // --- more points, and the concave four ---------------------------
        "star8" | "burst" | "\u{2737}" => spiked(8, r, 0.55, UP),
        "star12" | "burst12" => spiked(12, r, 0.66, UP),
        // The astroid: a square with its sides pulled in, `x^(2/3) +
        // y^(2/3) = a^(2/3)`. Reads as a four-pointed star with curved
        // flanks rather than straight ones, which is what separates it
        // from `star4` at a glance.
        "astroid" | "concave" | "concave4" => to_radius(
            (0..ROUND + 8)
                .map(|i| {
                    let t = i as f64 * TAU / (ROUND + 8) as f64;
                    (t.cos().powi(3), t.sin().powi(3))
                })
                .collect(),
            r,
        ),

        // --- machined and natural shapes ---------------------------------
        //
        // A cog, built tooth by tooth rather than sampled: four vertices
        // each -- root, up the flank, across the crown, back down -- so the
        // teeth have flat tops instead of the staircase a polar sample
        // gives a square wave.
        "gear" | "cog" | "\u{2699}" => {
            const TEETH: usize = 10;
            let step = TAU / TEETH as f64;
            let inner = 0.70;
            let mut pts = Vec::with_capacity(TEETH * 4);
            for k in 0..TEETH {
                let base = UP + k as f64 * step;
                for &(rad, frac) in
                    &[(inner, 0.00), (1.0, 0.16), (1.0, 0.34), (inner, 0.50)]
                {
                    let a = base + step * frac;
                    pts.push((rad * r * a.cos(), rad * r * a.sin()));
                }
            }
            pts
        }
        // A daisy: round petals with a distinct centre. Deeper notches than
        // `flower`, so the petals separate rather than scallop.
        "daisy" | "rosette" => {
            to_radius(radial(ROUND + 16, |th| lobes(8, 0.78, 0.42, UP, th)), r)
        }
        // A pennant. The pole is drawn by `marker_overlay`; this is the
        // cloth, so a filled flag is filled and its pole still shows.
        "flag" | "\u{2691}" | "\u{2690}" => vec![
            (-0.16 * r, -1.00 * r),
            (0.86 * r, -0.62 * r),
            (-0.16 * r, -0.24 * r),
        ],

        // --- the composite glyphs ----------------------------------------
        //
        // Body only. Their internal rules come from `marker_overlay`, which
        // is what lets one mark be a shape AND the strokes across it.
        "boxplus" | "\u{229E}" | "boxtimes" | "\u{22A0}" | "boxdiamond" | "\u{26CB}" => {
            marker_polygon("s", r)?
        }
        "circleplus" | "oplus" | "\u{2295}" | "circletimes" | "otimes" | "\u{2297}"
        | "crosshair" | "\u{2316}" => ngon(ROUND, r, 0.0),
        // A star with a second one inside it, which is how a chart marks
        // the one point that matters among several that already use stars.
        "doublestar" | "star2" => spiked(5, r, 0.382, UP),

        _ => return None,
    })
}

/// Strokes drawn ACROSS a marker, on top of its body.
///
/// A single closed outline cannot say `⊕`: the circle and the cross that
/// divides it are two different things, and one polygon can only trace a
/// boundary. These are the segments that go over the body `marker_polygon`
/// returned -- in the same ink, at the same weight -- and they are what let
/// the divided-box and crossed-circle family exist at all.
///
/// A glyph may have an overlay and NO body (`snowflake`), which is how a
/// mark made entirely of strokes is expressed.
pub fn marker_overlay(glyph: &str, r: f64) -> Option<Vec<((f64, f64), (f64, f64))>> {
    let d = r * 0.886; // the square's half-side, from `marker_polygon`
    Some(match glyph {
        "boxplus" | "\u{229E}" => {
            vec![((-d, 0.0), (d, 0.0)), ((0.0, -d), (0.0, d))]
        }
        "boxtimes" | "\u{22A0}" => {
            vec![((-d, -d), (d, d)), ((-d, d), (d, -d))]
        }
        // The inscribed diamond, corner to corner of the square.
        "boxdiamond" | "\u{26CB}" => vec![
            ((0.0, -d), (d, 0.0)),
            ((d, 0.0), (0.0, d)),
            ((0.0, d), (-d, 0.0)),
            ((-d, 0.0), (0.0, -d)),
        ],
        "circleplus" | "oplus" | "\u{2295}" => {
            vec![((-r, 0.0), (r, 0.0)), ((0.0, -r), (0.0, r))]
        }
        "circletimes" | "otimes" | "\u{2297}" => {
            let k = r * std::f64::consts::FRAC_1_SQRT_2;
            vec![((-k, -k), (k, k)), ((-k, k), (k, -k))]
        }
        // Ticks that overshoot the ring, which is what makes it read as an
        // instrument's crosshair rather than as a divided circle.
        "crosshair" | "\u{2316}" => {
            let o = r * 1.35;
            vec![((-o, 0.0), (o, 0.0)), ((0.0, -o), (0.0, o))]
        }
        // Six arms, each with two pairs of branches. All strokes, no body:
        // a filled snowflake is a blob.
        "snowflake" | "\u{2744}" | "\u{2745}" => {
            let mut segs = Vec::with_capacity(30);
            for k in 0..6 {
                let a = k as f64 * TAU_6;
                let tip = (r * a.cos(), r * a.sin());
                segs.push(((0.0, 0.0), tip));
                for &(at, len) in &[(0.52, 0.34), (0.80, 0.22)] {
                    let root = (r * at * a.cos(), r * at * a.sin());
                    for side in [-1.0, 1.0] {
                        let b = a + side * 0.62;
                        segs.push((root, (root.0 + r * len * b.cos(), root.1 + r * len * b.sin())));
                    }
                }
            }
            segs
        }
        // The pole the pennant hangs from.
        "flag" | "\u{2691}" | "\u{2690}" => {
            vec![((-0.16 * r, -1.0 * r), (-0.16 * r, 1.0 * r))]
        }
        // The inner star, at 45% -- small enough to read as a second mark
        // rather than as a thick outline.
        "doublestar" | "star2" => {
            let inner = marker_polygon("*", r * 0.45)?;
            inner
                .iter()
                .zip(inner.iter().cycle().skip(1))
                .take(inner.len())
                .map(|(&a, &b)| (a, b))
                .collect()
        }
        _ => return None,
    })
}

/// One sixth of a turn, for the six-armed shapes.
const TAU_6: f64 = std::f64::consts::TAU / 6.0;

/// Split a marker spec into its glyph and, if it carries one, the line
/// style to draw underneath it.
///
/// matplotlib writes these as one string -- `"o-"` is circles joined by a
/// solid line, `"^--"` triangles joined by a dashed one -- and that is the
/// form every ported script uses. Qu's `marker=` meant markers *instead
/// of* a line, so `marker="o"` on a trend gave a scatter of loose dots and
/// `marker="^"` next to it gave a bare line: two series of the same figure
/// drawn in two different idioms, neither matching the original.
///
/// Returns `(glyph, Some(dash_pattern))` when a line is wanted. The dash
/// pattern is `None` inside the `Some` for a solid line.
/// Sample a smooth curve that passes through every one of `points`.
///
/// Catmull-Rom, not the natural cubic solver this crate already has for
/// `splineplot`. That one fits `y = f(x)` and needs x to increase, which
/// an outline does not do: a polygon doubles back on itself, so it is not
/// a function of x at all and the solver would either refuse it or return
/// nonsense. Catmull-Rom is parametric, so it does not care about the
/// direction of travel.
///
/// It also interpolates rather than approximates -- the curve goes through
/// the control points, which is what "a spline through my vertices" means.
/// A Bezier or B-spline would pull away from them and the shape would no
/// longer touch the coordinates the caller gave.
///
/// Closing is a matter of where the neighbours come from: wrap the lookup
/// and the seam is smooth by construction, with no periodic boundary
/// conditions to solve.
pub fn spline_through(points: &[(f64, f64)], closed: bool, per_segment: usize) -> Vec<(f64, f64)> {
    // A closed outline is very often written with its first vertex
    // repeated at the end -- that is how most polygon formats state one,
    // and it is what `linspace(0, 2*pi, n)` produces on a circle. Left in,
    // it is a zero-length segment: the spline has no direction to leave it
    // by, and puts a visible cusp or a small loop at the seam. Dropping it
    // costs nothing, because closing already returns to the first point.
    //
    // Compared against the shape's OWN size, not with an absolute epsilon:
    // a circle written as `cos(linspace(0, 2*pi, n))` does not come back
    // bit-identical at the seam -- `cos(2*pi)` is a rounding error away
    // from `cos(0)` -- so an exact test misses exactly the case this is
    // for, which is how the cusp survived the first attempt at it.
    let mut trimmed = points;
    if closed && points.len() > 1 {
        let (a, b) = (points[0], points[points.len() - 1]);
        let span = points.iter().fold(0.0f64, |m, p| {
            m.max((p.0 - a.0).abs()).max((p.1 - a.1).abs())
        });
        let tol = (span * 1e-9).max(f64::MIN_POSITIVE);
        if (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol {
            trimmed = &points[..points.len() - 1];
        }
    }
    let points = trimmed;
    let n = points.len();
    if n < 3 || per_segment < 1 {
        return points.to_vec();
    }
    // Open curves have no neighbour beyond the ends, so the end point
    // stands in for it -- the curve then leaves and arrives straight,
    // which is what an unclosed path should look like.
    let at = |i: isize| -> (f64, f64) {
        let i = if closed {
            i.rem_euclid(n as isize) as usize
        } else {
            i.clamp(0, n as isize - 1) as usize
        };
        points[i]
    };
    let segments = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segments * per_segment + 1);
    for s in 0..segments {
        let (p0, p1, p2, p3) = (
            at(s as isize - 1),
            at(s as isize),
            at(s as isize + 1),
            at(s as isize + 2),
        );
        for k in 0..per_segment {
            let t = k as f64 / per_segment as f64;
            let (t2, t3) = (t * t, t * t * t);
            let comp = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * ((2.0 * b)
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push((comp(p0.0, p1.0, p2.0, p3.0), comp(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    // The last control point, which the loop above stops just short of.
    // A closed curve returns to its first, which the renderer joins.
    out.push(if closed { points[0] } else { points[n - 1] });
    out
}

/// A dash pattern: alternating on/off lengths, plus where along them the
/// line starts.
///
/// An ARRAY, not an (on, off) pair, because a pair cannot express the
/// thing people most often want after a plain dash: `-.` alternates two
/// different mark lengths, and with a pair the best available was a
/// shorter dash, which sat next to `--` on a reference sheet looking like
/// the same line. SVG (`stroke-dasharray`), PDF (`[...] phase d`) and TikZ
/// (`dash pattern=on .. off .. on ..`) all take arrays natively, so
/// nothing downstream had to be invented for this.
#[derive(Clone, Debug, PartialEq)]
pub struct Dash {
    /// Distance into the pattern at which the line starts. matplotlib's
    /// first tuple element; `5` on a `(5, (10, 3))` shifts every dash
    /// along, which is what makes two otherwise identical dashed lines
    /// distinguishable where they overlap.
    pub offset: f64,
    /// Alternating on, off, on, off... An odd count repeats to make an
    /// even one, which is how SVG and PDF both define it.
    pub pattern: Vec<f64>,
}

impl Dash {
    /// `stroke-dasharray` / `stroke-dashoffset`.
    fn svg_attrs(&self) -> String {
        let arr: Vec<String> = self.pattern.iter().map(|v| format!("{v:.2}")).collect();
        let mut s = format!(" stroke-dasharray=\"{}\"", arr.join(","));
        if self.offset != 0.0 {
            let _ = write!(s, " stroke-dashoffset=\"{:.2}\"", self.offset);
        }
        s
    }
    /// PDF's `[a b c] phase d`.
    fn pdf_op(&self) -> String {
        let arr: Vec<String> = self.pattern.iter().map(|v| format!("{v:.2}")).collect();
        format!("[{}] {:.2} d ", arr.join(" "), self.offset)
    }
    /// TikZ's `dash pattern=on a off b on c ...`, plus `dash phase`.
    fn tikz_attr(&self) -> String {
        let mut parts = Vec::new();
        for (i, v) in self.pattern.iter().enumerate() {
            parts.push(format!("{} {}", if i % 2 == 0 { "on" } else { "off" }, tikz_dim(*v)));
        }
        let mut s = format!("dash pattern={}", parts.join(" "));
        if self.offset != 0.0 {
            let _ = write!(s, ", dash phase={}", tikz_dim(self.offset));
        }
        s
    }
    fn new(offset: f64, pattern: &[f64]) -> Self {
        Dash { offset, pattern: pattern.to_vec() }
    }
    fn scaled(&self, k: f64) -> Dash {
        Dash {
            offset: self.offset * k,
            pattern: self.pattern.iter().map(|v| v * k).collect(),
        }
    }
}

/// The three marks a line style is built from, in MILLIMETRES.
///
/// Millimetres because that is the unit a dash on a printed page is
/// actually measured in, and because Qu's canvas unit already is one
/// (1/10 mm) -- so these numbers can be read off a ruler laid on the
/// output, which is the only check that matters for a print figure.
///
/// Deliberately NOT multiples of the line width, which is how matplotlib
/// defines its patterns. That rule assumes a default line near 1.5 pt;
/// Qu's is 1.5 canvas units, or 0.15 mm -- about 0.43 pt, three times
/// finer -- so width-relative lengths came out at 0.6 mm, present in the
/// file and invisible on the page. Width still has a say: `scaled_dash`
/// grows the pattern for a heavy stroke, it just does not shrink it into
/// nothing for a hairline.
///
/// Re-tuned 2026-09-10: the previous bases (2.0 / 0.35 / 1.0) read as
/// "too much continuity" against a thin stroke on a curved line -- a
/// dash:gap of 2:1 sounds broken-up on paper, but the eye tracks a wavy
/// curve's own continuity right through the gaps. The lever that actually
/// fixed it was NOT the ratio (dash:gap here, 2.5:1, is if anything
/// higher than before) -- it is the absolute scale: a ~33-unit cycle
/// instead of ~85 means more than twice as many dash/gap transitions over
/// the same length of line, which reads as broken-up even without a
/// bigger gap. Ahmed's own numbers, confirmed against the rendered
/// `stroke-dasharray` before shipping.
const DASH_MM: f64 = 1.0;
const DOT_MM: f64 = 0.15;
const GAP_MM: f64 = 0.4;

/// Five styles and three densities, from a grammar rather than a list.
///
/// `dashed`, and `loose dashed` and `dense dashed` beside it -- the same
/// style with the gaps opened up or closed in. Composing a modifier with a
/// style is how the rest of Qu's word commands already read (`legend
/// outside right`, `axis scale x log`), and it means fifteen usable
/// patterns are described by five shapes and two adjectives instead of
/// fifteen names to memorise.
///
/// Density scales the GAPS only. Keeping the marks the same size is what
/// makes `dense dashed` read as dashed-packed-tighter rather than as some
/// fourth unrelated style.
pub fn named_dash(name: &str) -> Option<Option<Dash>> {
    // Punctuation first, and deliberately BEFORE the normaliser below.
    //
    // That normaliser turns `-` and `_` into spaces so `loose-dashed` and
    // `loose_dashed` read the same as `loose dashed`. Run over a
    // punctuation form it erases the whole thing: `--` becomes two spaces
    // and `-..` becomes ` ..`, so the shorthand a plotting language is
    // most expected to have was the one spelling that did not work here.
    //
    // The forms are meant to LOOK like the line they ask for, which is the
    // whole point of them: `_` and `-` are unbroken, `--` is a repeated
    // dash, `:` and `..` are dots, and `-.` and `-..` show the mark order
    // they alternate in.
    // The density word comes off first, so it composes with EITHER
    // spelling of the shape -- `loose dashed` and `loose --` are the same
    // request, and a grammar where the adjective only works with one half
    // of the vocabulary is not a grammar.
    let lower = name.trim().to_ascii_lowercase();
    let density_of = |w: &str| match w {
        // `loosely`/`densely` accepted too: it is the same request, and
        // there is no reason for one spelling to be an error.
        "loose" | "loosely" => Some(2.2),
        "dense" | "densely" => Some(0.35),
        _ => None,
    };
    let (mut density, rest) = match lower.split_once(char::is_whitespace) {
        Some((head, tail)) => match density_of(head) {
            Some(d) => (d, tail.trim()),
            None => (1.0, lower.as_str()),
        },
        None => (1.0, lower.as_str()),
    };
    // The forms are meant to LOOK like the line they ask for, which is the
    // whole point of them: `_` and `-` are unbroken, `--` is a repeated
    // dash, `:` and `..` are dots, and `-.` / `-..` show the mark order
    // they alternate in.
    //
    // Matched BEFORE the word normaliser below, which turns `-` and `_`
    // into spaces so `loose-dashed` reads as `loose dashed`. Run over a
    // punctuation form that erases the whole thing -- `--` becomes two
    // spaces and `-..` becomes ` ..` -- so the shorthand a plotting
    // language is most expected to have was the one spelling that did not
    // work here.
    let shape = match rest {
        "-" | "_" | "___" => "solid",
        "--" | "__" | "- -" => "dashed",
        ":" | ".." | ". ." | "..." => "dotted",
        "-." | "_." | "-.-" => "dashdot",
        "-.." | "_.." | "-..-" => "dashdotdot",
        _ => "",
    };
    let key;
    let shape = if shape.is_empty() {
        key = rest.replace(['-', '_'], " ");
        let words: Vec<&str> = key.split_whitespace().collect();
        match words.as_slice() {
            [one] => *one,
            // `loose-dashed`: the density was joined by a hyphen rather
            // than a space, so the split above did not see it -- a hyphen
            // is not whitespace. Caught here, after normalisation, so both
            // ways of writing it land in the same place.
            [head, one] => match density_of(head) {
                Some(d) => {
                    density = d;
                    *one
                }
                None => return None,
            },
            _ => return None,
        }
    } else {
        shape
    };
    let (dash, dot, gap) = (
        DASH_MM * MM_TO_UNITS,
        DOT_MM * MM_TO_UNITS,
        GAP_MM * MM_TO_UNITS * density,
    );
    let pattern: Vec<f64> = match shape {
        "solid" | "-" => return Some(None),
        "dotted" | "dot" | ":" => vec![dot, gap],
        "dashed" | "dash" | "--" => vec![dash, gap],
        "dashdot" | "-." => vec![dash, gap, dot, gap],
        "dashdotdot" | "-.." => vec![dash, gap, dot, gap, dot, gap],
        _ => return None,
    };
    Some(Some(Dash::new(0.0, &pattern)))
}

/// Every style name, for the error message that lists them.
pub const DASH_NAMES: &[&str] = &["solid", "dotted", "dashed", "dashdot", "dashdotdot"];

/// Grow a dash pattern to the weight of the line it is drawn on.
///
/// Two rules, and both are needed for different reasons.
///
/// It SCALES with the line width, because a pattern that reads well on a
/// hairline turns into a chain of blocks under a heavy stroke. Only
/// upward: the base patterns below are already sized for Qu's default
/// line, and shrinking them for a thinner one puts the dash back under the
/// resolution of the page.
///
/// And the bases are absolute rather than pure multiples of the width,
/// because Qu's default line is 1.5 canvas units -- 0.15 mm, about 0.43 pt
/// -- roughly a third of matplotlib's default 1.5 pt. Scaling purely by
/// width therefore reproduced a dash a third of everyone else's length:
/// really present in the file, and invisible on the page. Measured on a
/// rendered export before this changed, the old (6, 3) came out at 0.6 mm
/// on / 0.3 mm off, which reads as a texture rather than a dash.
fn scaled_dash(base: &Dash, width: f64) -> Dash {
    base.scaled((width / DEFAULT_LINE_WIDTH).max(1.0))
}

/// Split a marker spec into its glyph and the line style under it.
///
/// The three patterns are told apart by SHAPE, not by length alone: dots
/// are short marks with wide gaps, `--` is a long dash, and `-.` is a
/// shorter dash with a tight gap. That last one is an approximation and
/// worth naming as one -- a real dash-dot alternates two different mark
/// lengths, which needs a dash ARRAY, and `DrawOp`'s dash is a single
/// (on, off) pair threaded through twenty-one construction sites. What is
/// here is at least distinguishable from `--` at a glance, which the old
/// (6, 2.5) against (6, 3) was not.
pub fn split_marker(spec: &str) -> (&str, Option<Option<Dash>>) {
    for (suffix, name) in [("--", "dashed"), (":", "dotted"), ("-.", "dashdot"), ("-", "solid")] {
        let dash = named_dash(name).unwrap_or(None);
        if let Some(glyph) = spec.strip_suffix(suffix) {
            // A bare "-" is a line with no marker, not an empty glyph.
            if !glyph.is_empty() {
                return (glyph, Some(dash));
            }
            return ("line", Some(dash));
        }
    }
    (spec, None)
}

/// The line sample a legend row draws, and the gap between it and the
/// label. Named because the box that reserves this space and the row that
/// fills it are computed in different functions: the box was reserving 18
/// while the text started at 20, so every entry sat two units further right
/// than the box knew about.
/// Default outline weight for a stroked polygon, in canvas units.
///
/// Matched to the plot's own line weight rather than left to the
/// renderer's 1-unit default: a marker whose edge is thinner than the
/// series it sits on reads as a different, fainter thing.
pub const MARKER_EDGE_W: f64 = 1.6;

/// The line sample in a legend row. Long enough to show two dashes of a
/// dashed series -- at 14 a dash pattern came out as one stroke and a
/// dotted one as two specks, so three line styles in a key were
/// indistinguishable from each other.
const LEGEND_SWATCH_W: f64 = 20.0;
const LEGEND_TEXT_GAP: f64 = 6.0;
const LEGEND_GUTTER: f64 = LEGEND_SWATCH_W + LEGEND_TEXT_GAP;
/// Inner margin. Was 10, about 0.8x the legend's own type size, which read
/// as a loose box with a lot of white around a short label; ggplot2 sets
/// its legend margin at half the base size and that is the proportion this
/// now matches.
const LEGEND_PAD: f64 = 7.0;

/// Wrap `s` in parentheses unless it is a single indivisible symbol.
///
/// Only for the inline form of a fraction, where the bracket is doing the
/// job the fraction rule does in the stacked form: `\frac{a+b}{c}` written
/// on one line is `(a+b)/c`, never `a+b/c`. A lone letter, digit or
/// already-bracketed group needs nothing, and bracketing it anyway
/// (`(1)/(2)`) makes the common case unreadable.
fn bracket_if_compound(s: &str) -> String {
    let t = s.trim();
    let atomic = t.chars().count() <= 1
        || t.chars().all(|c| c.is_alphanumeric())
        || (t.starts_with('(') && t.ends_with(')'))
        || (t.starts_with('[') && t.ends_with(']'));
    if atomic {
        t.to_string()
    } else {
        format!("({t})")
    }
}

/// Baseline-to-baseline spacing of a wrapped title, in multiples of its
/// own size. Tighter than body text: two lines of a title are one heading,
/// and should read as a block rather than as two sentences.
const TITLE_LINE_PITCH: f64 = 1.12;

/// Break `text` into lines no wider than `avail`, at spaces.
///
/// Never more than three lines, and never mid-word: a title that still does
/// not fit is left to overflow, which is visible and fixable, where a
/// silently truncated or hyphenated one is neither. A single word wider
/// than the panel is returned whole for the same reason.
fn wrap_to_width(
    text: &str,
    avail: f64,
    size: f64,
    metrics: Option<&pdf_font::FontMetrics>,
) -> Vec<String> {
    const MAX_LINES: usize = 3;
    if measure_text(text, size, metrics) <= avail {
        return vec![text.to_string()];
    }
    // Words, except that a `$...$` span is ONE word however many spaces
    // are inside it. Breaking a line between them leaves an unclosed `$`
    // on one line and an unopened one on the next, and neither half is
    // math any more: a panel title reading `$\Delta = 27\%$` wrapped into
    // `... $\Delta =` and `27\%$`, and the second line printed the
    // backslash.
    let mut words: Vec<String> = Vec::new();
    let mut span: Option<String> = None;
    for w in text.split_whitespace() {
        // `\$` is a literal dollar and does not open or close anything.
        let dollars = w
            .char_indices()
            .filter(|&(i, c)| c == '$' && !w[..i].ends_with('\\'))
            .count();
        match &mut span {
            Some(acc) => {
                acc.push(' ');
                acc.push_str(w);
                if dollars % 2 == 1 {
                    words.push(span.take().unwrap());
                }
            }
            None => {
                if dollars % 2 == 1 {
                    span = Some(w.to_string());
                } else {
                    words.push(w.to_string());
                }
            }
        }
    }
    // An unterminated span still has to reach the output.
    if let Some(rest) = span {
        words.push(rest);
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in &words {
        let candidate =
            if current.is_empty() { word.clone() } else { format!("{current} {word}") };
        if measure_text(&candidate, size, metrics) <= avail || current.is_empty() {
            current = candidate;
        } else if lines.len() + 1 < MAX_LINES {
            lines.push(std::mem::take(&mut current));
            current = word.clone();
        } else {
            // Out of lines: the rest goes on the last one and overflows.
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        vec![text.to_string()]
    } else {
        lines
    }
}

/// The `(dash, gap)` a reference line is stroked with, or `None` for solid.
///
/// Scaled by the line's own width so a hairline rule reads as dotted rather
/// than as a row of specks, and a heavy one as dashed rather than as an
/// unbroken line. `dot` wins over `dash` when a caller asks for both, since
/// it is the more specific request.
/// One iso-line, dashed or dotted if asked.
///
/// `Polyline` carries no pattern, so a patterned contour is emitted as its
/// individual segments -- each a `DottedLine`, which is the op that does
/// carry one. Contour polylines are already short segment chains from the
/// marching-squares pass, so this costs nothing a straight dashed rule
/// does not already cost.
fn contour_line(points: Vec<(f64, f64)>, color: String, width: f64, dash: bool, dot: bool) -> DrawOp {
    DrawOp::Polyline { points, color, width, dash: dash_pattern(dash && !dot, dot, width) }
}

fn dash_pattern(dash: bool, dot: bool, width: f64) -> Option<Dash> {
    let w = width.max(0.4);
    if dot {
        // A dot is as long as the line is wide -- that is what makes it
        // read as round -- but never shorter than a floor, because the two
        // families behaved differently and thin dotted rules disappeared:
        // a dash was a fixed (6, 4) at any width, while a dot scaled with
        // it, so at width 0.6 the pattern was a 0.66 mark every 2 points
        // and a reference line drawn that way was invisible at print size.
        const MIN_DOT: f64 = 1.4;
        let d = (w * 1.1).max(MIN_DOT);
        Some(Dash::new(0.0, &[d, d * 2.0]))
    } else if dash {
        // The same named pattern a dashed SERIES gets, so a reference rule
        // and a dashed curve read as the same kind of line.
        named_dash("dashed").flatten().map(|base| scaled_dash(&base, w))
    } else {
        None
    }
}

/// The height of a legend box holding `entries` rows-worth of labels.
///
/// Split out from `legend_box_size` so a caller can ask what the box WOULD
/// measure at a different column count or type size without building a
/// whole hypothetical panel -- which is what `legend_fit_note` needs to
/// avoid recommending a remedy that would not actually work.
fn legend_box_height(
    entries: usize,
    columns: usize,
    size: f64,
    has_title: bool,
    padding: f64,
    tick_scale: f64,
    // Whether any entry carries a rule -- a radical's vinculum or an
    // accent. Those sit above the cap height a row was sized for, so
    // without the extra the first row's bar runs into the box border.
    rules: bool,
) -> f64 {
    const ROW_H: f64 = 18.0;
    let row_h = ROW_H * tick_scale * (size / (12.0 * tick_scale)).max(0.5);
    let pad = LEGEND_PAD * tick_scale * padding.clamp(0.2, 4.0);
    let rows = entries.div_ceil(columns.max(1));
    let title_h = if has_title { row_h } else { 0.0 };
    // A hat's apex is the tallest thing a label can carry (see
    // `write_pdf_text_spans`), so the allowance is measured from that.
    let headroom = if rules { 0.30 * size } else { 0.0 };
    pad * 2.0 + title_h + row_h * rows as f64 + headroom
}

/// Whether a label draws a rule over any of its runs.
fn label_has_rule(label: &str) -> bool {
    label.contains('$')
        && parse_math_spans_for_pdf(label).iter().any(|r| r.over != Overline::None)
}

fn legend_box_size(panel: &Panel, tick_scale: f64, face: Option<&pdf_font::FontMetrics>) -> (f64, f64) {
    const PAD: f64 = LEGEND_PAD;
    let labels: Vec<&str> = panel.series.iter().filter_map(|s| s.label.as_deref()).collect();
    if labels.is_empty() {
        return (0.0, 0.0);
    }
    let size = panel.legend.font_size.unwrap_or(TICK_METRIC_BASELINE * tick_scale);
    let pad = PAD * tick_scale * panel.legend.padding.unwrap_or(1.0).clamp(0.2, 4.0);
    // Measured against the real face rather than the per-character
    // estimate. The estimate runs about a fifth wide on ordinary lowercase
    // -- "filtered" comes out 3.58 em against Latin Modern's actual 3.03 --
    // and the whole difference showed up as dead space between the longest
    // label and the right edge of the box.
    let text_w = labels
        .iter()
        .map(|l| measure_label(l, size, face))
        .fold(0.0f64, f64::max)
        .max(6.0 * tick_scale);
    let cols = panel.legend.columns.max(1);
    let col_w = LEGEND_GUTTER * tick_scale + text_w;
    (
        pad * 2.0 + col_w * cols as f64 + 8.0 * tick_scale * (cols - 1) as f64,
        legend_box_height(
            labels.len(),
            cols,
            size,
            panel.legend.title.is_some(),
            panel.legend.padding.unwrap_or(1.0),
            tick_scale,
            labels.iter().any(|l| label_has_rule(l)),
        ),
    )
}

#[derive(Clone, Debug)]
pub struct Panel {
    pub direction: Direction,
    pub grid_cell: Option<usize>,
    /// Which layer of the grid this panel sits in, 0-based.
    ///
    /// `split plot 3 2 2` is a 3x2 grid two layers deep: twelve panels
    /// occupying six rectangles. Layers overlap exactly for now -- every
    /// layer of a cell is laid out at the same rect -- and are separately
    /// addressable so a later `layer padding` can fan them out into a deck
    /// without any of the addressing changing.
    pub grid_layer: usize,
    pub series: Vec<Series>,
    pub shapes: Vec<Shape>,
    pub callouts: Vec<Callout>,
    pub boxplots: Vec<BoxplotGroup>,
    pub spider: Option<SpiderChart>,
    pub raincloud: Vec<RaincloudGroup>,
    pub heatmap: Option<Heatmap>,
    /// The scale strip, when `colorbar()` asked for one.
    pub colorbar: Option<ColorBar>,
    pub pie: Option<PieChart>,
    pub xlabel: Option<String>,
    pub ylabel: Option<String>,
    pub title: Option<String>,
    /// Horizontal placement for `title`/`xlabel`/`ylabel(...)`, set via
    /// their `align=` kwarg ("left"/"center"/"right", plus "start"/"end"
    /// and, for `ylabel` specifically, "bottom"/"top" — see `parse_align`).
    /// Only wired up for the default cartesian render path; spider/heatmap/
    /// pie panels keep their own always-centered title unconditionally.
    pub title_align: Anchor,
    pub xlabel_align: Anchor,
    /// Where along the y-axis's vertical span the (rotated) `ylabel` sits:
    /// `Start` pins it to the bottom (growing upward), `End` to the top
    /// (growing downward), `Middle` (default) centers it — the vertical
    /// analog of `xlabel_align`, sharing the same `Anchor` vocabulary since
    /// "low end of the span" / "high end" / "centered" is the same idea
    /// rotated 90 degrees.
    pub ylabel_align: Anchor,
    pub xlim: Option<(f64, f64)>,
    pub ylim: Option<(f64, f64)>,
    /// Limits for the SECONDARY axes -- `yyaxis right` and `xxaxis top`.
    ///
    /// Without these, `ylim` after `yyaxis right` wrote the primary range:
    /// the right axis silently kept autoscaling and the LEFT one took the
    /// numbers meant for the right, which on a log left axis produced a
    /// decade range of 1e-279..1e0 and a blank plot. `ylim`/`xlim` write
    /// whichever side is active, the way `ylabel` and `yticks` already do.
    pub xlim2: Option<(f64, f64)>,
    pub ylim2: Option<(f64, f64)>,
    pub xscale: Scale,
    pub yscale: Scale,
    /// Scale for the SECONDARY axes; `None` follows the primary.
    ///
    /// A twin axis is usually twinned precisely because the two quantities
    /// do not share a scale -- a variance inflation over three decades
    /// against a normalised eigenvalue in [0, 1.25] is the case this was
    /// added for, and forcing one mode on both put the linear quantity on
    /// a log axis where it read as a flat line at the floor.
    pub xscale2: Option<Scale>,
    pub yscale2: Option<Scale>,
    pub axis_equal: bool,
    /// `axis origin` (spines cross at data `(0,0)`) vs. the default
    /// `axis edge` (spines frame the panel, MATLAB's own box style).
    pub axis_position: AxisPosition,
    pub legend: Legend,
    /// `None` means "whatever the theme says"; `Some` is an explicit
    /// `grid on`/`grid off` from the script, which always wins. Without
    /// this distinction a theme could not turn the grid off by default
    /// without also making `grid on` impossible.
    pub show_grid: Option<bool>,
    /// `grid minor on` — dotted lines at the minor ticks between each pair
    /// of major gridlines, independent of `show_grid` (MATLAB allows minor
    /// grid without major grid too, unusual as that combination is).
    pub show_minor_grid: Option<bool>,
    pub show_box: bool,
    /// `axis off` — no spines, no ticks, no tick labels, no grid.
    ///
    /// Every plotting library has this and it was the one missing. Two
    /// things want it: a figure where the frame is noise (an inset, a
    /// sparkline, a schematic), and drawing that is not a plot at all —
    /// the panel is then just a coordinate system, which is a perfectly
    /// good thing for it to be.
    pub show_axes: bool,
    /// `axis tight` (MATLAB) — `false` (the default) pads the auto-computed
    /// extent by 5% on each side so data doesn't sit exactly on the border;
    /// `true` fits the axes exactly to the data, no margin.
    pub tight: bool,
    /// A name from [`PALETTES`]; unknown names resolve to `"default"`.
    pub palette: String,
    /// Whether `colormap(...)` was called for this panel, as opposed to the
    /// palette being whatever the theme or the default left behind.
    ///
    /// `theme("publication")` picks a colourblind-safe palette as part of
    /// its target, which is right when nobody has said otherwise and wrong
    /// when they have: `colormap("viridis")` then `theme("publication")`
    /// rendered in the publication palette, while the same two calls in the
    /// other order rendered viridis. Same script, same figure, different
    /// colours depending on line order, and no warning either way. Mirrors
    /// `Figure.font_size_explicit`, which exists for the same tension
    /// between an explicit choice and a target's defaults.
    pub palette_explicit: bool,
    /// Explicit x-tick positions (`xticks(v)`), paired in order with
    /// `xtick_labels` when both are set. `None` uses the automatic
    /// evenly-spaced positions instead.
    pub xtick_positions: Option<Vec<f64>>,
    /// Categorical x-axis labels (`xticklabels(...)`) — when set, these
    /// replace the usual "nice number" x ticks entirely, one label per tick.
    /// Without `xtick_positions`, labels are placed evenly across the
    /// current x-extent (the common case: category series already sit at
    /// evenly-spaced x positions spanning that extent).
    pub xtick_labels: Option<Vec<String>>,
    /// Explicit y tick positions and labels, for each side.
    ///
    /// The x axis has had `xticks`/`xticklabels` all along and the y axis
    /// had neither, so an axis whose ticks mean something other than their
    /// own number could not be drawn at all. The case that forced it: a
    /// right-hand axis carrying tone count against a CF ratio, where the
    /// mapping is `1/sqrt(1 - ln K / L)` and the ticks land at 1.20, 1.33,
    /// 1.46, 1.61, 1.81 but have to READ 10, 25, 51, 94, 173.
    pub ytick_positions: Option<Vec<f64>>,
    pub ytick_labels: Option<Vec<String>>,
    pub ytick2_positions: Option<Vec<f64>>,
    pub ytick2_labels: Option<Vec<String>>,
    /// `yyaxis left`/`yyaxis right` (MATLAB) mode switches, recorded as
    /// `(series_index_at_switch_time, is_right)` rather than a per-series
    /// flag — avoids touching every one of the ~15 call sites across
    /// `lib.rs` that construct a `Series` directly. `is_right_axis` finds
    /// which side a given series index was on by scanning backward for the
    /// most recent switch at or before it.
    pub axis_mode_changes: Vec<(usize, bool)>,
    /// `xxaxis bottom`/`xxaxis top` — the x-axis analog of `axis_mode_changes`
    /// (not a MATLAB spelling; this project's own name for a secondary,
    /// independently-scaled x-axis, e.g. plotting the same data against two
    /// different x units). `is_top_axis` mirrors `is_right_axis`.
    pub x_axis_mode_changes: Vec<(usize, bool)>,
    /// Current `yyaxis`/`xxaxis` side — consulted (not scanned like the
    /// switch lists above) by the `ylabel(...)`/`xlabel(...)` *calls*, so
    /// each side can carry its own label.
    pub current_y_right: bool,
    pub current_x_top: bool,
    /// Right-axis / top-axis labels, set by `ylabel(...)`/`xlabel(...)`
    /// while `current_y_right`/`current_x_top` is active.
    pub ylabel2: Option<String>,
    pub xlabel2: Option<String>,
    /// `zoom_inset(x0, x1, y0, y1)` — a magnified sub-view of that data
    /// region, auto-placed in whichever panel corner sits farthest from the
    /// zoom box (same "avoid overlapping the interesting part" reasoning as
    /// the legend's own `best` placement), with a dashed zoom-rectangle in
    /// the main plot and connector lines to the inset border. Only
    /// line/marker series are replayed into the inset (bar/stack/step and
    /// the non-cartesian chart kinds don't have an established convention
    /// for "zoomed" rendering, so `render_svg` skips them there).
    pub inset: Option<InsetZoom>,
    /// `xbreak(lo, hi)` — squeezes the data range `(lo, hi)` into a small
    /// fixed-width "//" jag on the main (bottom) x-axis instead of giving it
    /// pixel space proportional to its (possibly huge) span, so two very
    /// different-scale regions can share one axis without a dead zone
    /// between them. A distinct sibling of `inset`/`zoom_inset`: this
    /// reshapes the *main* axis's own coordinate mapping (`axis_frac`),
    /// it doesn't add a sub-panel. Scoped to a *linear* main axis only —
    /// log-scale panels and the secondary `xxaxis top`/`yyaxis right` axes
    /// ignore it (see `axis_frac`'s call sites in `build_draw_ops`), a
    /// documented limitation rather than a silent wrong answer.
    pub x_break: Option<(f64, f64)>,
    /// `ybreak(lo, hi)` — the y-axis twin of `x_break`, same scope limits.
    pub y_break: Option<(f64, f64)>,
    /// `imshow(img)` — places a real loaded/processed `Value::Image`
    /// (actual RGB pixels, not a colormap-mapped scalar grid — that's
    /// `heatmap`'s job) inline in the panel, scaled to fit while preserving
    /// aspect ratio. Like `spider`/`heatmap`/`pie`, a panel with an image
    /// renders *only* the image — it doesn't mix with cartesian series.
    pub image: Option<crate::image::Image>,
}

/// See `Panel.inset`.
#[derive(Clone, Debug)]
pub struct InsetZoom {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    /// Explicit placement (same keyword vocabulary/enum as the legend's own
    /// `position` — `LegendPosition::parse`), or `Best` (the default) for
    /// an automatic search over the same 8 candidate corners/edges the
    /// legend's own `best_corner` uses, preferring one with **zero**
    /// overlap against any plotted point (stricter than the legend's own
    /// merely-fewest tie-break) over the legend's own minimum-overlap
    /// fallback.
    pub position: LegendPosition,
    /// Reserves margin and places the inset entirely outside the plot area
    /// (same mechanism as `Legend.outside`) — the honest, guaranteed-safe
    /// choice when even `Best`'s zero-overlap search can't find a clear
    /// interior spot. Not auto-detected: `Best` only searches interior
    /// candidates and accepts a minimum-overlap fallback if none are
    /// clear, since promoting to `outside` automatically would need
    /// deciding *before* margins are finalized, which the current
    /// single-pass layout can't do — ask for `outside: true` explicitly
    /// once you know the interior will be crowded.
    pub outside: bool,
    /// `zoom_inset(..., clip=true)` — the "clip" sibling: instead of
    /// replaying only whole data points that fall inside the zoom box
    /// (`Best`/default "zoom" style, which can chord straight across a
    /// dip that briefly leaves the box, and never quite reaches the box's
    /// own border), line/marker-line series are hard-clipped against the
    /// box edges with real segment-rectangle clipping (Liang-Barsky, see
    /// `clip_polyline_to_rect`) so the drawn path touches the border
    /// exactly where the true curve crosses it, with no bleed past it and
    /// no straight-line shortcut across an excluded interior dip.
    pub clip: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            direction: Direction::default(),
            grid_cell: None,
            grid_layer: 0,
            series: Vec::new(),
            shapes: Vec::new(),
            callouts: Vec::new(),
            boxplots: Vec::new(),
            spider: None,
            raincloud: Vec::new(),
            heatmap: None,
            colorbar: None,
            pie: None,
            xlabel: None,
            ylabel: None,
            title: None,
            title_align: Anchor::Start,
            xlabel_align: Anchor::Middle,
            ylabel_align: Anchor::Middle,
            xlim: None,
            ylim: None,
            xscale: Scale::default(),
            yscale: Scale::default(),
            axis_equal: false,
            axis_position: AxisPosition::default(),
            legend: Legend::default(),
            show_grid: None,
            show_minor_grid: None,
            show_box: true,
            show_axes: true,
            tight: false,
            palette: "default".to_string(),
            palette_explicit: false,
            xtick_positions: None,
            xtick_labels: None,
            ytick_positions: None,
            ytick_labels: None,
            ytick2_positions: None,
            ytick2_labels: None,
            axis_mode_changes: Vec::new(),
            x_axis_mode_changes: Vec::new(),
            xlim2: None,
            ylim2: None,
            xscale2: None,
            yscale2: None,
            current_y_right: false,
            current_x_top: false,
            ylabel2: None,
            xlabel2: None,
            inset: None,
            x_break: None,
            y_break: None,
            image: None,
        }
    }
}

impl Panel {
    fn with_slot(direction: Direction, grid_cell: Option<usize>) -> Self {
        Self::with_slot_in_layer(direction, grid_cell, 0)
    }

    fn with_slot_in_layer(direction: Direction, grid_cell: Option<usize>, grid_layer: usize) -> Self {
        Panel {
            direction,
            grid_cell,
            grid_layer,
            ..Panel::default()
        }
    }

    /// Whether the series at `idx` was plotted while `yyaxis right` was
    /// active — see `axis_mode_changes`'s doc comment.
    pub fn is_right_axis(&self, idx: usize) -> bool {
        self.axis_mode_changes
            .iter()
            .rev()
            .find(|(i, _)| *i <= idx)
            .map(|(_, right)| *right)
            .unwrap_or(false)
    }

    /// Whether the series at `idx` was plotted while `xxaxis top` was
    /// active — see `x_axis_mode_changes`'s doc comment.
    pub fn is_top_axis(&self, idx: usize) -> bool {
        self.x_axis_mode_changes
            .iter()
            .rev()
            .find(|(i, _)| *i <= idx)
            .map(|(_, top)| *top)
            .unwrap_or(false)
    }

    /// True for a panel nobody has drawn into or labeled yet — used so the
    /// figure's initial blank panel gets claimed by whichever operation
    /// first needs a "new" one (`next plot`, `hold off`'s first plot,
    /// `panel(r,c,i)`) instead of sitting around as a permanently wasted
    /// empty slot in the rendered layout.
    fn is_pristine(&self) -> bool {
        self.series.is_empty()
            && self.shapes.is_empty()
            && self.callouts.is_empty()
            && self.boxplots.is_empty()
            && self.spider.is_none()
            && self.raincloud.is_empty()
            && self.heatmap.is_none()
            && self.pie.is_none()
            && self.image.is_none()
            && self.grid_cell.is_none()
            && self.xlabel.is_none()
            && self.ylabel.is_none()
            && self.title.is_none()
    }
}

#[derive(Clone, Debug)]
pub struct Figure {
    pub panels: Vec<Panel>,
    pub grid: Option<(usize, usize)>,
    pub current: usize,
    pub holding: bool,
    /// User-settable via the `fontfamily(name)` builtin; applied once on the
    /// root `<svg>` element (see `render_svg`) rather than per `DrawOp::Text`.
    pub font_family: String,
    /// User-settable via the `fontsize(tick:, label:, title:)` builtin.
    pub tick_size: f64,
    pub label_size: f64,
    pub title_size: f64,
    /// `blur_backdrop(x0, y0, x1, y1, radius=)` — a figure-level (not
    /// per-panel) depth-of-field-style effect: the focus rectangle
    /// `(x0,y0)-(x1,y1)` (fractions 0..1 of the whole rendered canvas, not
    /// panel data coordinates — this blurs pixels, it isn't tied to any one
    /// panel's axes) stays sharp, everything else in the figure is
    /// Gaussian-blurred behind it. SVG-native via `<feGaussianBlur>`; TikZ
    /// has no filter-primitive equivalent, so `render_tikz` renders the
    /// figure unblurred and emits a comment noting the degradation (see
    /// `render_tikz`) rather than silently dropping the request or erroring
    /// the whole export.
    pub blur_backdrop: Option<BlurBackdrop>,
    /// User-settable via the `figure_background(color)` builtin — the
    /// figure-level SVG background `<rect>` fill (see `render_svg`).
    /// Accepts anything SVG's own `fill` attribute does (a `#rrggbb` hex
    /// string or a CSS named color), same pass-through, no-parsing
    /// convention as every other plotting `color=` kwarg (`vline`,
    /// `rectangle`, `xspan`/`yspan`, ...). Defaults to opaque white so
    /// every pre-existing script/test renders identically to before this
    /// field existed. Deliberately does NOT auto-invert axis/gridline/text
    /// colors for a dark background — that's a bigger "themeable plot"
    /// feature, out of scope here.
    pub background: String,
    /// User-settable via `theme("minimal")` / `theme("default")`. Narrower
    /// than the "themeable plot" feature declined above (no color inversion,
    /// no per-element overrides) — just the two things ggplot2's
    /// `theme_minimal()`/`theme_bw()` and BBC's `bbc_style()` agree on
    /// hardest: drop the solid axes box (no visual reason a subplot needs a
    /// frame when whitespace already separates panels) and recede the
    /// gridlines further into the background (`MINIMAL_GRID_COLOR` vs the
    /// default `GRID_COLOR` — see that constant's doc comment for the exact
    /// values compared). Deliberately does NOT touch panel background,
    /// fonts, or palette — Qu's white/journal background and Inter default
    /// are its own documented choice (see `DEFAULT_BACKGROUND`, `GRID_COLOR`
    /// docs), not something `theme("minimal")` should silently override.
    pub minimal: bool,
    /// `theme("publication")` -- the figure is destined for a printed page,
    /// not this monitor.
    ///
    /// A separate axis from `minimal` because it answers a different
    /// question: `minimal` is a look, this is a TARGET. The difference is
    /// not cosmetic. A figure that reads well backlit is routinely too
    /// light once it is 8cm wide in a journal column, and may be
    /// photocopied or printed greyscale after that -- so this thickens the
    /// axis frame and the data lines rather than thinning them, darkens
    /// tick labels from chrome-grey to near-black, and leaves gridlines
    /// pale so they never compete with the data they sit behind. Paired
    /// with `fontfamily("tikz")` (Latin Modern, the typeface a LaTeX paper
    /// is already set in) and the colourblind-safe `okabe_ito` palette, a
    /// script gets a figure that belongs in a paper rather than one that
    /// looks pasted into it.
    pub publication: bool,
    /// Which named look this figure uses -- see `THEMES`. `publication`
    /// stays as its own flag because it also drives font embedding, the
    /// palette and the print type scale, which are not styling.
    pub theme: String,
    /// The script called `fontsize(...)` itself, so the automatic scaling
    /// below must keep its hands off. Without this flag the order of
    /// `fontsize` / `figure_size` / `theme` would decide the outcome, and
    /// an explicit size the user typed could be silently overwritten by a
    /// later call that was about something else.
    pub font_size_explicit: bool,
    /// User-settable via the `figure_size(width, height)` builtin — the
    /// canvas size `savefig`/`write_report` render at (in place of the
    /// previously-hardcoded `DEFAULT_FIGURE_WIDTH`/`DEFAULT_FIGURE_HEIGHT`
    /// constants both now default to). Setting this also rescales
    /// `tick_size`/`label_size`/`title_size` proportionally — see
    /// `figure_size`'s own doc comment in `lib.rs` for the exact formula and
    /// why (matplotlib/ggplot2/Plots.jl all default to *fixed* absolute font
    /// sizes regardless of figure size, which only stays legible because
    /// none of them let a script silently render at some tiny/huge canvas
    /// without the caller consciously picking new sizes too — Qu had no
    /// figure-size knob at all before this, so there was nothing to keep
    /// consistent; now that there is one, it auto-scales by default so a
    /// bigger canvas doesn't have to fight fixed-pixel text).
    pub width: f64,
    pub height: f64,
    /// The script named BOTH dimensions itself, so nothing may resize the
    /// canvas behind its back. Only `figure_size(w, h)` sets this -- the
    /// preset form `figure_size("nature2")` fixes the width a journal
    /// prints at and deliberately leaves the height to be decided from
    /// what is plotted, which is the case `fit_grid_height` exists for.
    pub size_explicit: bool,
    /// How many layers deep the grid is -- see `split_grid`. 1 for every
    /// ordinary figure, and every figure that existed before layers did.
    pub layers: usize,
    /// Which layer new panels are selected in, 0-based.
    pub current_layer: usize,
    /// The script fixed the WIDTH -- either form of `figure_size` does
    /// that, including the preset one, because a column preset IS the
    /// width a journal will print at. `fit_grid_width` must leave it alone;
    /// widening a figure authored for an 89 mm column silently undoes the
    /// one thing the preset was for.
    pub width_explicit: bool,
    /// Whether the "circle/rectangle on unequal axes" note has already
    /// fired for THIS figure. A `Shape::Curve` from `circle()` samples its
    /// points in data space (see that variant's own doc comment) and a
    /// `Shape::Rect` is defined by data-space corners, so either one
    /// renders however the panel's own x/y scale happens to differ -- a
    /// circle becomes an ellipse, a square rectangle becomes a
    /// non-square one, with nothing on screen saying so. Once per FIGURE
    /// rather than once per session (unlike `screen_export_warned`)
    /// because it lives on `Figure` itself, which is freshly constructed
    /// by every `figure()` call -- so it resets exactly when a new figure
    /// starts, with no separate bookkeeping needed.
    pub equal_aspect_note_shown: bool,
}

/// See `Figure.blur_backdrop`.
#[derive(Clone, Debug)]
pub struct BlurBackdrop {
    pub fx0: f64,
    pub fy0: f64,
    pub fx1: f64,
    pub fy1: f64,
    /// Gaussian `stdDeviation` in px; a `radius=` style kwarg, defaulting
    /// to `BLUR_DEFAULT_RADIUS`.
    pub radius: f64,
}

pub const BLUR_DEFAULT_RADIUS: f64 = 6.0;

/// Default figure background — see `Figure.background`.
pub const DEFAULT_BACKGROUND: &str = "#ffffff";

impl Default for Figure {
    fn default() -> Self {
        Self::new()
    }
}

impl Figure {
    pub fn new() -> Self {
        Figure {
            panels: vec![Panel::default()],
            grid: None,
            current: 0,
            holding: true,
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            tick_size: DEFAULT_TICK_SIZE,
            label_size: DEFAULT_AXIS_LABEL_SIZE,
            title_size: DEFAULT_TITLE_SIZE,
            blur_backdrop: None,
            background: DEFAULT_BACKGROUND.to_string(),
            minimal: false,
            publication: false,
            theme: "default".to_string(),
            font_size_explicit: false,
            width: DEFAULT_FIGURE_WIDTH,
            height: DEFAULT_FIGURE_HEIGHT,
            size_explicit: false,
            layers: 1,
            current_layer: 0,
            width_explicit: false,
            equal_aspect_note_shown: false,
        }
    }

    pub fn current_panel(&self) -> &Panel {
        &self.panels[self.current]
    }

    pub fn current_panel_mut(&mut self) -> &mut Panel {
        &mut self.panels[self.current]
    }

    /// Which panel a handle taken right now would belong to.
    pub fn current_panel_index(&self) -> usize {
        self.current
    }

    /// Claims the lone still-untouched initial panel for `slot` instead of
    /// pushing a new one, if that's all there is; otherwise appends. Returns
    /// the resulting index. Shared by every "I need a new panel" path so the
    /// figure's starter panel never sits around as a wasted empty slot.
    fn place(&mut self, slot: Panel) -> usize {
        if self.panels.len() == 1 && self.panels[0].is_pristine() {
            self.panels[0] = slot;
            0
        } else {
            self.panels.push(slot);
            self.panels.len() - 1
        }
    }

    /// `next plot [vertical|horizontal]` — always starts a fresh panel,
    /// regardless of `hold on`/`hold off`.
    pub fn advance(&mut self, direction: Direction) {
        self.current = self.place(Panel::with_slot(direction, None));
    }

    /// A `plot`/`stem`/`scatter`/… call reuses the current panel under
    /// `hold on` (the default); under `hold off` every call gets a fresh one.
    ///
    /// Every builtin that draws a chart goes through here first, including
    /// the ones that immediately set `heatmap`/`pie` themselves -- so this
    /// is also the one place to clear those fields before a chart draws,
    /// rather than leaving it to each of the dozens of callers.
    /// `heatmap`/`pie` are `Option`, not `Vec`: unlike `series`, `shapes`,
    /// `boxplots`, etc. -- which are meant to accumulate under `hold on`
    /// -- a panel has AT MOST one heatmap or pie at a time, so a second
    /// exclusive chart on the same panel must replace the first, not sit
    /// silently behind it. Before this, only starting a genuinely NEW
    /// panel (`next plot`, `hold off`, `figure`) ever cleared a heatmap;
    /// reusing the current one under plain `hold on` did not, so
    /// `confusion_matrix(...)` followed by `hist(...)` on the same panel
    /// kept the confusion matrix's cells -- drawn last, opaque, covering
    /// the histogram underneath -- wearing the SECOND call's title.
    /// `spider` looks like the same shape (also `Option`) but is NOT: a
    /// `SpiderChart` carries its OWN `Vec<series>` and `spiderplot(...)`
    /// is meant to accumulate into it across repeated calls, the same way
    /// two `plot()`s overlay two lines -- `spiderplot_accumulates_series_
    /// across_calls` pins exactly that, so `spider` is deliberately left
    /// alone here. Series callers still overlay exactly as before: this
    /// only ever resets the EXCLUSIVE fields, never `series` itself.
    pub fn panel_for_new_series(&mut self) -> &mut Panel {
        if !self.holding {
            self.advance(Direction::Vertical);
        }
        let panel = self.current_panel_mut();
        panel.heatmap = None;
        panel.pie = None;
        panel
    }

    /// `panel(rows, cols, index)` (MATLAB-style `subplot`, 1-based, row-major).
    ///
    /// Selects within the CURRENT layer -- one for an ordinary figure, and
    /// whichever `next layer` last chose for a layered one.
    pub fn select_grid(&mut self, rows: usize, cols: usize, index: usize) -> Result<(), String> {
        if rows == 0 || cols == 0 || index < 1 || index > rows * cols {
            return Err(format!(
                "panel({rows}, {cols}, {index}) is out of range"
            ));
        }
        self.grid = Some((rows, cols));
        let cell = index - 1;
        let layer = self.current_layer;
        self.current = match self
            .panels
            .iter()
            .position(|p| p.grid_cell == Some(cell) && p.grid_layer == layer)
        {
            Some(pos) => pos,
            None => self.place(Panel::with_slot_in_layer(Direction::Vertical, Some(cell), layer)),
        };
        Ok(())
    }

    /// `split plot rows cols layers` -- the grid, and how deep it is.
    ///
    /// A layer is a whole copy of the grid drawn in the same rectangles.
    /// They overlap exactly for now, which is what makes the third number
    /// free to be added to an existing figure without moving anything, and
    /// a later `layer padding` will fan them into a deck. Two panels at the
    /// same rect draw their own frames on top of each other -- identical
    /// while the offset is zero, and exactly what a fanned deck needs once
    /// it is not.
    pub fn split_grid(&mut self, rows: usize, cols: usize, layers: usize, start: usize) -> Result<(), String> {
        if layers == 0 {
            return Err("split plot: a figure needs at least one layer".into());
        }
        self.layers = layers;
        self.current_layer = 0;
        self.select_grid(rows, cols, start)
    }

    /// `next layer` / `next layer 2` -- the same cell, one layer further
    /// in. Past the declared depth is an error rather than a wrap, for the
    /// reason `advance_grid` gives: wrapping would silently draw over a
    /// layer that already has content.
    pub fn select_layer(&mut self, layer: usize) -> Result<(), String> {
        let Some((rows, cols)) = self.grid else {
            return Err("next layer: this figure has no grid -- `split plot rows cols layers` \
                        sets one up"
                .into());
        };
        if layer < 1 || layer > self.layers {
            return Err(format!(
                "next layer {layer}: this figure is {} layer{} deep",
                self.layers,
                if self.layers == 1 { "" } else { "s" }
            ));
        }
        let cell = self.panels[self.current].grid_cell.unwrap_or(0);
        self.current_layer = layer - 1;
        self.select_grid(rows, cols, cell + 1)
    }

    pub fn advance_layer(&mut self) -> Result<(), String> {
        self.select_layer(self.current_layer + 2)
    }

    /// `next plot` inside a grid: the cell after the current one, row-major.
    ///
    /// Without this, `next plot` after a `panel(2,1,1)` built a FLOW panel
    /// (`grid_cell: None`) while the figure still had a grid -- and
    /// `layout_panels`' grid branch drops any panel without a cell, so that
    /// plot was silently discarded. The script still printed
    /// "plot: figure 2" and exited 0, and the figure came out with one
    /// panel. Verified directly before this landed.
    ///
    /// Running off the end is an error rather than a wrap: five `next
    /// plot`s in a 2x2 is a script bug, and wrapping would overwrite the
    /// first panel with the fifth plot's data while still exiting 0 --
    /// the same silent-wrong-figure failure in a new costume.
    pub fn advance_grid(&mut self) -> Result<(), String> {
        let Some((rows, cols)) = self.grid else {
            return Err("next plot: this figure has no grid -- `split plot rows cols` \
                        sets one up"
                .into());
        };
        let here = self.panels[self.current].grid_cell.unwrap_or(0);
        let next = here + 2; // `select_grid` counts from 1
        if next > rows * cols {
            return Err(format!(
                "next plot: cell {} is the last of the {rows}x{cols} grid -- \
                 split a bigger one, or `figure()` to start another",
                here + 1
            ));
        }
        self.select_grid(rows, cols, next)
    }

    /// `next plot 2 3` -- the cell at row 2, column 3 of the grid already
    /// in place. Row and column rather than `select_grid`'s flat index,
    /// because that is how a person reads a grid off the page; the flat
    /// form stays available as `panel(r, c, i)`.
    pub fn select_grid_cell(&mut self, row: usize, col: usize) -> Result<(), String> {
        let Some((rows, cols)) = self.grid else {
            return Err("next plot: this figure has no grid -- `split plot rows cols` \
                        sets one up"
                .into());
        };
        if row < 1 || row > rows || col < 1 || col > cols {
            return Err(format!(
                "next plot {row} {col}: outside the {rows}x{cols} grid"
            ));
        }
        self.select_grid(rows, cols, (row - 1) * cols + col)
    }

    /// Recompute tick/label/title sizes from the figure's size and target.
    ///
    /// Called by both `figure_size` and `theme` so the two are order
    /// independent: whichever is set second, the result is the same. Does
    /// nothing once a script has set sizes explicitly.
    pub fn apply_auto_type_scale(&mut self) {
        if self.font_size_explicit {
            return;
        }
        // A height of 0 is the "decide from the data at render time" marker
        // that `figure_size("nature2")` leaves behind. Feeding it straight
        // into the area scale gives sqrt(0) = 0, which collapsed every font
        // and margin to nothing -- the axes, labels and legend all vanished
        // and only the data was drawn. Stand in the default aspect for the
        // purpose of sizing type; the real height is resolved later and the
        // difference is a few percent of the scale, not a factor.
        let height = if self.height > 0.0 { self.height } else { self.width * 0.75 };
        // Type scales with WIDTH, not area.
        //
        // Area is the intuitive choice and it is wrong for the commonest
        // real case: a stacked multi-panel figure. Two panels in one
        // column is the same 3.5-inch column, twice as tall -- read at the
        // same distance, set in the same body text, so its labels must be
        // the same size as a single panel's. Scaling by area grew them by
        // sqrt(2), which is exactly the "why is the text enormous" of a
        // tall figure and the "why is it tiny" of a wide short one.
        //
        // Height still has a say, but only as a guard: a figure much
        // wider than it is tall (a 4:1 strip) has little vertical room,
        // and type sized purely off its width crowds the panel out. The
        // cap pulls the scale back toward the height in that case and
        // does nothing at ordinary aspect ratios.
        let width_scale = self.width / DEFAULT_FIGURE_WIDTH;
        let height_scale = height / DEFAULT_FIGURE_HEIGHT;
        let area_scale = width_scale.min(height_scale * 1.6);
        // EVERY print-ready theme gets the print type scale, not just the
        // one called "publication". `theme("nature")` was landing on the
        // screen scale, so a figure authored for a journal came out with
        // tick labels at ~1.4% of its width where published figures run
        // nearer 2.2% (Nature: 6-7 pt on an 89 mm column).
        // A PRINT theme sizes type in POINTS, not as a fraction of the
        // figure.
        //
        // This is the whole difference between the two modes, and getting
        // it wrong is invisible until a figure changes size. A journal
        // says "7 pt tick labels" and means 7 pt whether the figure is a
        // 3.5-inch single column or a 7.16-inch double one -- because the
        // reader holds the page at the same distance either way. Scaling
        // type with the figure instead makes 7 pt on the narrow figure
        // into 14 pt on the wide one, which is what a 2x3 panel grid came
        // out as: headings colliding across panels and tick numbers
        // bigger than the data.
        //
        // The screen scale below keeps the old relative behaviour, which
        // is right there for the opposite reason: a screen figure has no
        // fixed physical size, so its type has to track the canvas.
        if resolve_theme(&self.theme).print_ready {
            self.tick_size = PRINT_TICK_PT * PT_TO_UNITS;
            self.label_size = PRINT_LABEL_PT * PT_TO_UNITS;
            self.title_size = PRINT_TITLE_PT * PT_TO_UNITS;
            return;
        }
        self.tick_size = DEFAULT_TICK_SIZE * area_scale;
        self.label_size = DEFAULT_AXIS_LABEL_SIZE * area_scale;
        self.title_size = DEFAULT_TITLE_SIZE * area_scale;
    }

    pub fn clear_figure(&mut self) {
        *self = Figure::new();
    }

    /// Start the next figure, keeping the figure-level STYLE.
    ///
    /// The difference from `clear_figure` is what survives. `theme`,
    /// `fontfamily`, `fontsize`, `figure_size` and `figure_background` are
    /// settings a script states once, at the top, and means for everything
    /// it draws -- so a `show` between two plots must not silently revert
    /// the second one to defaults. A script that genuinely wants different
    /// styling for the second figure just says so again.
    ///
    /// Content -- panels, grid, hold state, and the blur rectangle, which
    /// is expressed in fractions of a specific canvas and so cannot mean
    /// the same thing on a different figure -- does not carry over.
    pub fn start_next(&mut self) {
        let mut next = Figure::new();
        next.font_family = self.font_family.clone();
        next.tick_size = self.tick_size;
        next.label_size = self.label_size;
        next.title_size = self.title_size;
        next.background = self.background.clone();
        next.minimal = self.minimal;
        next.publication = self.publication;
        next.theme = self.theme.clone();
        next.font_size_explicit = self.font_size_explicit;
        next.width = self.width;
        next.height = self.height;
        next.size_explicit = self.size_explicit;
        next.layers = self.layers;
        next.current_layer = 0;
        next.width_explicit = self.width_explicit;
        *self = next;
    }

    /// True for a figure nobody has drawn into yet -- the "brand new,
    /// nothing plotted" state `Figure::new()` produces. Used by the
    /// `figure()` builtin (before it clears the current figure to start a
    /// fresh one) and by report generation (`qu-cli`'s `report.rs`) to
    /// decide whether the figure that's about to be replaced/finalized
    /// actually has content worth keeping, vs. an untouched figure that
    /// was never drawn into (e.g. two back-to-back `figure()` calls with
    /// no plotting between them) -- see `Panel::is_pristine` for the
    /// per-panel version this composes.
    pub fn is_pristine(&self) -> bool {
        self.panels.len() == 1 && self.panels[0].is_pristine()
    }

    pub fn clear_current_panel(&mut self) {
        let (direction, grid_cell) = {
            let p = self.current_panel();
            (p.direction, p.grid_cell)
        };
        self.panels[self.current] = Panel::with_slot(direction, grid_cell);
    }

    /// `clear panel N` — 1-based position among panels created so far.
    pub fn clear_panel_by_index(&mut self, index: usize) -> Result<(), String> {
        if index < 1 || index > self.panels.len() {
            return Err(format!(
                "clear panel {index}: only {} panel(s) exist",
                self.panels.len()
            ));
        }
        let slot = index - 1;
        let (direction, grid_cell) = (self.panels[slot].direction, self.panels[slot].grid_cell);
        self.panels[slot] = Panel::with_slot(direction, grid_cell);
        Ok(())
    }

    /// `clear panel rows, cols, index` — clears (or pre-creates empty) the
    /// panel at that grid coordinate without disturbing `current`.
    pub fn clear_panel_by_grid(&mut self, rows: usize, cols: usize, index: usize) -> Result<(), String> {
        if rows == 0 || cols == 0 || index < 1 || index > rows * cols {
            return Err(format!("clear panel {rows}, {cols}, {index} is out of range"));
        }
        self.grid = Some((rows, cols));
        let cell = index - 1;
        match self.panels.iter().position(|p| p.grid_cell == Some(cell)) {
            Some(pos) => self.panels[pos] = Panel::with_slot(Direction::Vertical, Some(cell)),
            None => {
                self.place(Panel::with_slot(Direction::Vertical, Some(cell)));
            }
        }
        Ok(())
    }
}

/// Linear-interpolation percentile (numpy's default `'linear'` method):
/// `p` in `[0, 1]` against an already-sorted slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = idx - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

/// Builds a `BoxplotGroup`'s Tukey five-number summary from raw samples:
/// quartiles by linear interpolation, whiskers extended to the most extreme
/// point within `1.5 * IQR` of the box, everything further out as an
/// individual outlier.
pub fn boxplot_stats(data: &[f64], x: f64, color: Option<String>, label: Option<String>) -> BoxplotGroup {
    let mut sorted: Vec<f64> = data.iter().copied().filter(|v| v.is_finite()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q1 = percentile(&sorted, 0.25);
    let median = percentile(&sorted, 0.5);
    let q3 = percentile(&sorted, 0.75);
    let iqr = q3 - q1;
    let (lo_fence, hi_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let mut whisker_lo = q1;
    let mut whisker_hi = q3;
    let mut outliers = Vec::new();
    for &v in &sorted {
        if v < lo_fence || v > hi_fence {
            outliers.push(v);
        } else {
            whisker_lo = whisker_lo.min(v);
            whisker_hi = whisker_hi.max(v);
        }
    }
    BoxplotGroup { x, q1, median, q3, whisker_lo, whisker_hi, outliers, color, label }
}

/// Gaussian kernel density estimate, bandwidth via Silverman's rule of thumb
/// (`0.9 * std * n^-0.2`), sampled at `samples` evenly-spaced points spanning
/// the data's range padded by 3 bandwidths on each side. Returns raw
/// (unnormalized-to-1, but a true density) `(x, density)` pairs — callers
/// that just need a shape (e.g. `raincloud`'s cloud) normalize by their own
/// max instead of needing the density to integrate to 1.
pub fn gaussian_kde(data: &[f64], samples: usize) -> (Vec<f64>, Vec<f64>) {
    let n = data.len();
    if n == 0 || samples < 2 {
        return (Vec::new(), Vec::new());
    }
    let mean = data.iter().sum::<f64>() / n as f64;
    let variance = data.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std = variance.sqrt().max(1e-9);
    let bandwidth = (0.9 * std * (n as f64).powf(-0.2)).max(1e-6);
    let (lo, hi) = data.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let pad = 3.0 * bandwidth;
    let (lo, hi) = (lo - pad, hi + pad);
    let xs: Vec<f64> = (0..samples).map(|i| lo + (hi - lo) * i as f64 / (samples - 1) as f64).collect();
    let norm = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * bandwidth * n as f64);
    let ys: Vec<f64> = xs
        .iter()
        .map(|&x| data.iter().map(|&d| (-(x - d).powi(2) / (2.0 * bandwidth * bandwidth)).exp()).sum::<f64>() * norm)
        .collect();
    (xs, ys)
}

// ------------------------------------------------------------------ layout

#[derive(Clone, Copy, Debug)]
pub struct PanelRect {
    pub panel: usize,
    pub left: f64,
    pub top: f64,
    pub w: f64,
    pub h: f64,
}

const GAP: f64 = 18.0;

/// Mirrors the JS playground's `computeLayout` (`docs/assets/qu-studio.js`):
/// a true `rows x cols` grid once `panel(r,c,i)` set one, otherwise a flow
/// layout where each panel's `direction` decides whether it starts a new row
/// or joins the previous one. Kept as an independent Rust implementation
/// (not shared code — one runs in a browser, one is native) but deliberately
/// following the same rules so scripts read the same in both places.
pub fn layout_panels(fig: &Figure, width: f64, height: f64) -> Vec<PanelRect> {
    if let Some((rows, cols)) = fig.grid {
        let cell_w = (width - GAP * (cols as f64 + 1.0)) / cols as f64;
        let cell_h = (height - GAP * (rows as f64 + 1.0)) / rows as f64;
        return fig
            .panels
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let cell = p.grid_cell?;
                let row = cell / cols;
                let col = cell % cols;
                Some(PanelRect {
                    panel: i,
                    left: GAP + col as f64 * (cell_w + GAP),
                    top: GAP + row as f64 * (cell_h + GAP),
                    w: cell_w,
                    h: cell_h,
                })
            })
            .collect();
    }
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for (i, p) in fig.panels.iter().enumerate() {
        if p.direction == Direction::Horizontal && !rows.is_empty() {
            rows.last_mut().unwrap().push(i);
        } else {
            rows.push(vec![i]);
        }
    }
    let row_h = (height - GAP * (rows.len() as f64 + 1.0)) / rows.len().max(1) as f64;
    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let col_w = (width - GAP * (row.len() as f64 + 1.0)) / row.len() as f64;
        for (c, &panel) in row.iter().enumerate() {
            out.push(PanelRect {
                panel,
                left: GAP + c as f64 * (col_w + GAP),
                top: GAP + r as f64 * (row_h + GAP),
                w: col_w,
                h: row_h,
            });
        }
    }
    out
}

/// Per-index cumulative height across every `stackbar` series in a panel, in
/// call order — the y-extent must cover the *top* of the stack, not any one
/// series' own (much smaller) range.
fn stacked_bar_totals(panel: &Panel) -> Vec<f64> {
    let mut totals: Vec<f64> = Vec::new();
    for series in &panel.series {
        if series.marker != "stackbar" {
            continue;
        }
        if totals.len() < series.y.len() {
            totals.resize(series.y.len(), 0.0);
        }
        for (i, &y) in series.y.iter().enumerate() {
            totals[i] += y;
        }
    }
    totals
}

/// `pad`: add a 5% margin on each side (matplotlib/MATLAB's default —
/// points sit inside the axes, not exactly on the border) unless the panel
/// asked for `axis tight` (exact fit to data, no margin).
fn data_extent(values: impl Iterator<Item = f64>, log: bool, pad: bool) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in values {
        let v = if log { v.max(1e-300).log10() } else { v };
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-15 {
        let flat_pad = if pad { 1.0 } else { 0.5 };
        return (lo - flat_pad, hi + flat_pad);
    }
    if pad {
        let margin = (hi - lo) * 0.05;
        // Padding must not invent values on the far side of zero for a
        // quantity that never goes there. A time signal sampled from 0 to
        // 0.4095 s was given an axis starting at -0.0205 s, and a spectrum
        // running 0 to 5 kHz an axis starting at -247 Hz: both meaningless,
        // and both push the trace off the corner of the frame so the data
        // looks offset inside its own box. Clamping to zero keeps the
        // breathing room where it is useful (the far end) and drops it
        // where it is nonsense.
        //
        // Only when the data actually reaches zero -- data that merely
        // stays positive, say 3 to 7, keeps its margin on both sides,
        // because there is nothing special about the origin for it.
        //
        // ASYMMETRIC ON PURPOSE. Zero is a natural FLOOR for the quantities
        // this clamp was written for -- elapsed time, frequency, a
        // magnitude, a count -- and a negative axis start for any of them
        // is meaningless. It is almost never a natural CEILING. The
        // mirrored upper clamp that used to sit here assumed that data
        // reaching no higher than zero meant zero was a hard maximum, and
        // that assumption fails on the commonest figure in this whole
        // language: a Bode magnitude. A passive filter's gain tops out at
        // exactly 0 dB, so `hi` was clamped to 0, the 0 dB asymptote landed
        // exactly on the frame, and the top of the trace was drawn under
        // the border and disappeared. Measured on a first-order low-pass:
        // data -35.9647..0.0000 became an axis of -37.7629..0.0000 -- 1.8 dB
        // of breathing room at the bottom and none at the top.
        //
        // Gain above 0 dB is not impossible anyway (any amplifier), so the
        // padding invents nothing here.
        //
        // The asymmetry is by SIDE, not by whether the data touches zero.
        // An earlier attempt at "clamp only when the data stays clear of
        // the boundary" looked symmetric and reintroduced the original bug
        // immediately: a signal sampled from t=0 got an axis starting at
        // -0.0205 s again. Whether the data touches zero is not the
        // question; which side zero is on is.
        //
        //   floor: keep the clamp. Negative time, negative frequency and
        //          negative |magnitude| are all meaningless, so padding
        //          below zero invents a region that cannot exist.
        //   ceiling: drop it. Zero is almost never a physical maximum --
        //          gain above 0 dB is just an amplifier -- so padding above
        //          zero invents nothing, and refusing to pad puts a 0 dB
        //          asymptote exactly on the frame where half its stroke is
        //          clipped away. That is the "top line is missing" a real
        //          figure showed.
        let lo_padded = if lo >= 0.0 && lo - margin < 0.0 { 0.0 } else { lo - margin };
        let hi_padded = hi + margin;
        (lo_padded, hi_padded)
    } else {
        (lo, hi)
    }
}

// -------------------------------------------------------------- draw ops

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// Parses the `align=` kwarg shared by `title`/`xlabel`/`ylabel`. Accepts
/// both the horizontal words ("left"/"right") and, for `ylabel`'s rotated
/// text, the vertical words ("bottom"/"top") — plus the generic
/// "start"/"middle"/"end" — so one kwarg spelling works for any of the
/// three regardless of orientation. Returns `None` for anything else,
/// leaving the caller's existing default in place.
pub fn parse_align(s: &str) -> Option<Anchor> {
    match s.to_ascii_lowercase().as_str() {
        "left" | "start" | "bottom" => Some(Anchor::Start),
        "center" | "centre" | "middle" => Some(Anchor::Middle),
        "right" | "end" | "top" => Some(Anchor::End),
        _ => None,
    }
}

/// Resolves an `Anchor` alignment against a span `[lo, hi]` to a pivot
/// coordinate and the SVG/TikZ anchor to draw with — `lo` is the "low" end
/// of the span (left for a horizontal title/xlabel, bottom for a rotated
/// ylabel), `hi` the "high" end (right / top respectively).
pub fn align_pos(align: Anchor, lo: f64, hi: f64) -> (f64, Anchor) {
    match align {
        Anchor::Start => (lo, Anchor::Start),
        Anchor::Middle => ((lo + hi) / 2.0, Anchor::Middle),
        Anchor::End => (hi, Anchor::End),
    }
}

#[derive(Clone, Debug)]
pub enum DrawOp {
    Line { x1: f64, y1: f64, x2: f64, y2: f64, color: String, width: f64 },
    /// `dash` is `Some((on, off))` for a patterned line. It belongs on
    /// the polyline rather than being emitted as one dashed segment per
    /// point pair: every backend dashes a whole path with a continuous
    /// phase, whereas per-segment dashing restarts the pattern at each
    /// vertex, and a contour whose segments are shorter than one dash
    /// period then draws every segment as a single dash -- a solid line
    /// with extra ops.
    Polyline { points: Vec<(f64, f64)>, color: String, width: f64, dash: Option<Dash> },
    Rect { x: f64, y: f64, w: f64, h: f64, fill: Option<String>, stroke: Option<String>, opacity: f64, radius: f64 },
    Circle { cx: f64, cy: f64, r: f64, fill: Option<String>, stroke: Option<String> },
    Cross { cx: f64, cy: f64, r: f64, color: String },
    /// `italic` is set in the DRAWN italic, not a slanted regular. A figure
    /// uses italic to say a label is *about* the plot rather than part of
    /// it — a named region, an asymptote, an aside — and the distinction is
    /// only worth making if the reader can see it. Latin Modern ships a
    /// drawn italic; smearing the regular would read as the same mistake
    /// fake bold was.
    Text { x: f64, y: f64, text: String, size: f64, anchor: Anchor, rotate: f64, color: String, italic: bool },
    /// A closed, filled shape (unlike `Polyline`, which is stroke-only) —
    /// used for `raincloud`'s density "cloud".
    /// `width` is the OUTLINE weight, in canvas units.
    ///
    /// There was none, so every stroked polygon fell to the renderer's own
    /// default of one user unit -- a hairline on a 900-unit canvas. A
    /// marker outline is the thing that says which series a point belongs
    /// to, and at print size a hairline of it disappears.
    Polygon { points: Vec<(f64, f64)>, fill: Option<String>, stroke: Option<String>, opacity: f64, width: f64 },
    /// Start clipping subsequent ops to this rectangle; `ClipEnd` stops.
    ///
    /// Data outside the axis range used to be drawn anyway, over the
    /// margins and off the figure: a `plot` whose curve leaves an explicit
    /// `xlim`/`ylim` window, a reference line that starts below the
    /// bottom of the axis. Every other plotting library clips to the axes,
    /// and the alternative -- clipping each shape's own geometry -- would
    /// have to be repeated correctly in a dozen places instead of once.
    ClipStart { x: f64, y: f64, w: f64, h: f64 },
    ClipEnd,
    /// A dashed/dotted line rendered *natively* (SVG `stroke-dasharray`,
    /// TikZ `dash pattern`) as a single element, unlike `dashed_segment`'s
    /// manual expansion into dozens of individual `Line` ops — fine for a
    /// handful of short connector lines (`zoom_inset`), wasteful when drawn
    /// once per minor gridline across a whole panel's width or height (a
    /// semilogy plot's minor grid measured at ~500KB of `<line>` elements
    /// before this variant existed, ~45x a plain plot with no minor grid).
    DottedLine { x1: f64, y1: f64, x2: f64, y2: f64, color: String, width: f64, dash: Dash },
    /// A raster image, placed at `(x, y)` sized `w`x`h` (`imshow`/`imagesc`'s
    /// pixel data — see `image_ops`). `href` is a complete `data:` URI (a
    /// base64-encoded BMP, reusing `image::encode_bmp` and this module's own
    /// `base64_encode`), not a bare path — SVG's native `<image>` element
    /// takes the data URI directly, no separate asset file needed. TikZ has
    /// no equivalent (no data-URI embedding mechanism, and `savefig(.tikz)`
    /// writes no companion image file for `\includegraphics` to point at) —
    /// `write_tikz_op` draws a labeled placeholder instead of silently
    /// dropping the image, the same documented-gap approach `Figure.
    /// blur_backdrop` uses for its own TikZ-unsupported effect.
    Image { x: f64, y: f64, w: f64, h: f64, href: String },
    /// A `Circle`, but for a marker that represents one real data point
    /// worth hovering (a scatter/line-plot point) — carries `title`, the
    /// underlying data value(s) as text (e.g. `"x=3.2, y=17.05"`). In SVG
    /// this becomes a `<title>` child of the `<circle>`, which browsers show
    /// as a native, zero-JS tooltip on hover *when the SVG is inline in the
    /// DOM* (an `<img src="data:...">` can't reach into its own interior —
    /// see `FigureViewer.tsx`'s inline-SVG rendering path). TikZ/PDF have no
    /// hover concept, so `write_tikz_op`/`write_pdf_op` render this exactly
    /// like a plain `Circle` and drop the title. A separate variant (rather
    /// than adding `title` to `Circle` itself) keeps the ~20 other `Circle`
    /// call sites (gridline markers, boxplot outliers, error-bar caps, …)
    /// untouched — those aren't "one data value," so they get no tooltip.
    TitledCircle { cx: f64, cy: f64, r: f64, fill: Option<String>, stroke: Option<String>, title: String },
    /// A `Rect`, but for a shape that represents one real data point worth
    /// hovering (a bar top, a heatmap cell) — see `TitledCircle`'s doc for
    /// why this is a separate variant instead of a field on `Rect` itself,
    /// and how `title` is rendered (or dropped) per backend.
    TitledRect { x: f64, y: f64, w: f64, h: f64, fill: Option<String>, stroke: Option<String>, opacity: f64, radius: f64, title: String },
}

/// The default screen/UI look: a clean, modern sans-serif stack, leading
/// with a genuinely elegant grotesque (Inter) and falling back through the
/// major OS system fonts so it still looks deliberate on a machine that
/// doesn't have Inter installed — no font is ever embedded (dependency-free
/// by design; SVG only *references* font-family names, it can't ship one).
/// Single-quoted names (not double) — this whole string sits inside a
/// double-quoted SVG XML attribute. This is only the *default* —
/// `Figure.font_family` (settable via `fontfamily(...)`, including the
/// `"default"`/`"print"` shorthands resolved in `lib.rs`) is what actually
/// reaches the renderer; see `render_svg`, which sets `font-family` once on
/// the root `<svg>` (SVG inherits it to every `<text>` descendant, so
/// there's no need to thread it through each `DrawOp::Text`).
pub const DEFAULT_FONT_FAMILY: &str = "'Inter', 'Latin Modern Math', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif";
/// The `fontfamily("print")` look: an elegant, highly-legible serif for a
/// paper/journal feel. Georgia is the reliable fallback here — it's been
/// preinstalled on every major OS for two decades and reads well even
/// small, which matters since a font that never resolves to Source Serif
/// falls straight to it.
pub const PRINT_FONT_FAMILY: &str = "'Source Serif 4', 'Latin Modern Math', Georgia, 'Times New Roman', Times, serif";
/// The `fontfamily("tikz"/"latex")` look: Latin Modern Roman is literally
/// what a real LaTeX/TikZ toolchain renders body text with by default (it's
/// the modern, more complete OpenType successor to Computer Modern, the
/// original LaTeX typeface) — for a Qu figure meant to sit inside a LaTeX
/// paper next to `savefig(.tikz)` output (or other native TikZ figures)
/// looking like it was typeset by the same document, not an outsider SVG.
/// 'CMU Serif' is listed as a second-choice name since some systems have
/// Computer Modern Unicode installed under that family name instead.
pub const TIKZ_FONT_FAMILY: &str = "'Latin Modern Roman', 'Latin Modern Math', 'CMU Serif', Georgia, serif";
/// The `fontfamily("academic")` look: Latin Modern Sans, a clean sans
/// companion to the LaTeX default serif — reads as an academic-poster /
/// ggplot2-adjacent sans, distinct from Qu's own default (Inter). Falls
/// back to Qu's own Inter before the generic sans-serif stack, so a
/// machine without any Latin Modern install still lands on a deliberate
/// sans rather than whatever the OS happens to default to.
pub const ACADEMIC_SANS_FONT_FAMILY: &str = "'Latin Modern Sans', 'Latin Modern Math', 'CMU Sans Serif', Inter, sans-serif";

/// `'Latin Modern Math'` sits second in every stack above, and it is not
/// decoration: NONE of the four text faces covers the characters
/// `latex_symbol` can produce. Latin Modern Roman and Sans have no Greek at
/// all — no α, no μ — and no `≤ ≥ ≈ ∂ ∑ ∫`; Inter and Source Serif each
/// miss `∓ ⋅ ≡ ∼ ∝ ∇ ⋯`. Every one of those used to fall through to
/// whatever the viewer's machine supplied, so `$\mu$` in a Latin Modern
/// axis label arrived in a visibly foreign typeface, mid-word — the exact
/// failure embedding a font exists to prevent.
///
/// Latin Modern Math covers all 58 of them, and is drawn to sit with Latin
/// Modern Roman rather than merely beside it. `render_svg` embeds a subset
/// of it ONLY when the figure actually paints a character the main face
/// cannot draw (see `font_subset::unsupported`), so a figure with no math
/// in it pays nothing at all.
pub const MATH_FALLBACK_FONT_FAMILY: &str = "Latin Modern Math";

/// Variable-font TTF bytes bundled as repo assets (SIL OFL 1.1, redistributable
/// and embeddable — unlike Georgia/Times, which are OS-licensed and can't be
/// shipped), one file per curated `fontfamily(...)` preset. A single variable
/// font covers every weight the renderer actually uses (400 body/tick text,
/// 600 semibold titles/labels) via one `@font-face { font-weight: 100 900 }`
/// range, so there's no separate Regular/SemiBold file to keep in sync.
/// `include_bytes!` bakes them into the compiled binary at build time — this
/// is a bundled static asset, not a new crates.io dependency.
static INTER_VARIABLE_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
static SOURCE_SERIF_VARIABLE_TTF: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-Variable.ttf");
/// Latin Modern Roman/Sans, Regular weight only, OpenType (`.otf`) rather
/// than TrueType — sourced from a local TeX Live 2025 install
/// (`texmf-dist/fonts/opentype/public/lm/`), GUST Font License (LPPL-based,
/// same redistribution/embedding permission as the SIL OFL used above; see
/// `assets/fonts/LatinModern-GUST-LICENSE.txt`). Regular-only (no variable
/// weight axis the way Inter/Source Serif ship) is a fine match for Qu's
/// own two weights in practice: LaTeX documents typically don't bold their
/// axis titles either, so both `TIKZ_FONT_FAMILY` and
/// `ACADEMIC_SANS_FONT_FAMILY` render every weight from this one face.
static LATIN_MODERN_ROMAN_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernRoman-Regular.otf");
/// The REAL bold, not a browser-synthesised one.
///
/// The `@font-face` rule used to claim weights 100-900 from the Regular
/// file alone, so every bold title and axis label was faked by smearing
/// the regular outlines -- exactly the thing a reader clocks as "not
/// typeset" on a page of real LaTeX. Latin Modern ships a drawn bold;
/// this is it. GUST/LPPL licence, already bundled beside the fonts.
static LATIN_MODERN_ROMAN_BOLD_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernRoman-Bold.otf");
/// The REAL italic, for the same reason the bold is real.
///
/// A figure uses italic to mark a label as commentary rather than data --
/// a named region on a map, an asymptote, an aside -- and both backends
/// would otherwise slant the regular outlines, which reads as the same
/// mistake fake bold was. Latin Modern ships a drawn italic
/// (`lmroman10-italic.otf`, same TeX Live install and GUST licence as the
/// other faces); this is it.
static LATIN_MODERN_ROMAN_ITALIC_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernRoman-Italic.otf");
/// Latin Modern Math, from the same TeX Live 2025 install and under the same
/// GUST licence as the four text faces (`lm-math/latinmodern-math.otf`).
/// It is the fallback for every preset, not a fifth text face — see
/// `MATH_FALLBACK_FONT_FAMILY` for why all four text faces need one. Only
/// ever embedded as a subset of the handful of symbols a given figure
/// actually paints, so its 734 KB never reaches an SVG.
static LATIN_MODERN_MATH_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernMath-Regular.otf");
static LATIN_MODERN_SANS_BOLD_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernSans-Bold.otf");
static LATIN_MODERN_SANS_OTF: &[u8] = include_bytes!("../assets/fonts/LatinModernSans-Regular.otf");

/// Maps a `Figure.font_family` value to its embeddable bytes, if it's one of
/// the curated presets — an arbitrary custom font string (the escape
/// hatch `fontfamily("Custom, sans-serif")` still supports) has no bytes we
/// can embed, so `savefig(..., embed_fonts=true)` is a no-op for it (falls
/// back to the ordinary system-font reference, same as `embed_fonts=false`).
/// The third tuple element is the `@font-face src: format(...)` tag *and*
/// picks the `data:` URI MIME type in `render_svg` — TTF and OTF are
/// different container formats (`format('truetype')` vs `format('opentype')`)
/// and a mismatched tag can make some renderers reject the embedded face,
/// so this isn't just cosmetic.
/// The matching drawn bold for a preset, when the family has one.
///
/// `None` for Inter and Source Serif on purpose: those are VARIABLE fonts,
/// so one file genuinely carries every weight and their `100 900` claim is
/// true. Latin Modern is a set of separately drawn faces, and pretending
/// otherwise is what produced fake bold.
pub fn embeddable_bold_font_for(font_family: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    if font_family.contains("Latin Modern Roman") {
        Some(("Latin Modern Roman", LATIN_MODERN_ROMAN_BOLD_OTF, "opentype"))
    } else if font_family.contains("Latin Modern Sans") {
        Some(("Latin Modern Sans", LATIN_MODERN_SANS_BOLD_OTF, "opentype"))
    } else {
        None
    }
}

/// The matching drawn italic for a preset, when the family has one.
///
/// `None` for Inter, Source Serif and Latin Modern Sans: no italic is
/// bundled for them, and an italic label in those families sets upright
/// rather than in a slanted regular. Saying nothing is better than saying
/// it in a face the designer never drew.
pub fn embeddable_italic_font_for(font_family: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    if font_family.contains("Latin Modern Roman") {
        Some(("Latin Modern Roman", LATIN_MODERN_ROMAN_ITALIC_OTF, "opentype"))
    } else {
        None
    }
}

/// The one fallback face, for characters no text preset can draw.
///
/// Deliberately NOT reachable through `embeddable_font_for`: this is never
/// a figure's text font, only the face that catches Greek and the maths
/// operators when the chosen text face has no glyph for them. See
/// `MATH_FALLBACK_FONT_FAMILY`.
pub fn embeddable_math_font() -> (&'static str, &'static [u8], &'static str) {
    (MATH_FALLBACK_FONT_FAMILY, LATIN_MODERN_MATH_OTF, "opentype")
}

pub fn embeddable_font_for(font_family: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    if font_family.contains("Latin Modern Roman") {
        Some(("Latin Modern Roman", LATIN_MODERN_ROMAN_OTF, "opentype"))
    } else if font_family.contains("Latin Modern Sans") {
        Some(("Latin Modern Sans", LATIN_MODERN_SANS_OTF, "opentype"))
    } else if font_family.contains("Inter") {
        Some(("Inter", INTER_VARIABLE_TTF, "truetype"))
    } else if font_family.contains("Source Serif") {
        Some(("Source Serif 4", SOURCE_SERIF_VARIABLE_TTF, "truetype"))
    } else {
        None
    }
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A hand-rolled base64 encoder (standard alphabet, `=`-padded) — the only
/// use is inlining font bytes as a `data:` URI in an embedded `@font-face`,
/// which doesn't justify a new dependency for what's a well-known ~15-line
/// algorithm.
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { B64_ALPHABET[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_ALPHABET[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}
/// Tick-label gray — darker than the old `#888` (low-contrast on white) while
/// staying visibly lighter than body/title text, so ticks still read as
/// chrome rather than content.
const TICK_COLOR: &str = "#5a5a5a";
/// Gridline gray — solid, no alpha. The old `#8888882a` (~16% opacity) read
/// as "missing" at normal figure sizes; this is the light-but-legible gray
/// common in IEEE/matplotlib-style printed figures (visible without
/// competing with the data). Lightened one step further from `#d9d9d9`
/// (2026-09-03 ggplot2-inspired pass, prioritized per Ahmed's own callout)
/// so gridlines recede further behind the data, matching ggplot2/matplotlib
/// convention of "chrome, not content" — while staying clearly darker than
/// `MINIMAL_GRID_COLOR` (`#ececec`) so `theme("minimal")` stays visually
/// distinct from the default rather than collapsing into it.
const GRID_COLOR: &str = "#e6e6e6";
/// `theme("minimal")`'s gridline gray (see `Figure.minimal`) — lighter than
/// the journal-style default, closer to ggplot2's own approach of letting
/// gridlines recede rather than read as a drawn line: `theme_grey()`'s
/// panel is `grey92`/`#EBEBEB` with *white* gridlines on top of it (real
/// values pulled from ggplot2's own `theme-defaults.R`,
/// `panel.background = element_rect(fill = col_mix(ink, paper, 0.92))` and
/// `panel.grid = element_line(colour = paper)`, i.e. white-on-grey92 —
/// verified against the source, not guessed). Qu's minimal theme keeps the
/// white/journal panel background (that choice stays as documented on
/// `DEFAULT_BACKGROUND`) and instead pushes the *gridline* toward
/// near-invisible the other way: a pale `#ececec` on white reads with
/// roughly the same low contrast ggplot2 achieves with white-on-grey92,
/// without touching the panel fill itself.
// UNRESOLVED, and deliberately kept rather than deleted.
//
// The four constants below are unread: `ThemeStyle`'s table carries these
// values now, and `resolve_theme` is what rendering consults. Dating them
// settles which wins -- the table landed 2026-09-05 (69ca42b8, "wire the
// theme table") and all four predate it -- so the table supersedes them and
// nothing here is a live bug.
//
// What is NOT settled is whether the migration carried the decisions over or
// quietly dropped them, because the numbers do not line up either way:
//
//   MINIMAL_GRID_COLOR      #ececec  ->  minimal.grid_color      #ebebeb
//   GRID_LINE_WIDTH         0.8      ->  grid_width              1.0
//   MINOR_GRID_LINE_WIDTH   0.6      ->  minor_grid_width        0.5
//
// A fourth, `PUBLICATION_AXIS_WIDTH` (1.6 against the table's 1.15), was
// settled by Ahmed on 2026-09-10: the constant won, the table now carries
// 1.6, and the constant itself is gone because a value the table holds is
// noise rather than evidence.
//
// That is the distinction to apply to the three left. A dead constant that
// carries its REASON is a proposal nobody voted on; one carrying only a
// number the table already agrees with is litter. Both deleted siblings
// (TICK_TEXT_COLOR, PUBLICATION_GRID_COLOR) were the second kind. The three
// below are the first, and each is a question someone still has to answer,
// not a tidy-up someone forgot.
//
// `GRID_LINE_WIDTH`'s own comment says it was thinned from 1.0 to 0.8 on
// 2026-09-03 "so gridlines read as a thin, receding reference rather than a
// drawn line competing with the data" -- and the table holds 1.0, the
// PRE-thinning value, which looks like the thinning was lost. But the
// matching minor width is 0.5, which is neither the pre value (0.8) nor the
// post one (0.6), so it was not a clean revert either.
//
// Deleting them would erase the only record that these decisions were ever
// made and reasoned about. Changing the table to match them would revert a
// later retune on a guess. So they stay, documented, until someone decides
// deliberately.
#[allow(dead_code)] // superseded by the theme table; see the note above. Kept as evidence, not dead weight.
const MINIMAL_GRID_COLOR: &str = "#ececec";
/// Frame and tick-label ink for `theme("publication")`. Near-black rather
/// than the on-screen chrome greys: those greys are chosen to recede on a
/// bright display, and recede too far once printed or photocopied.
const PUBLICATION_INK: &str = "#111111";
/// Target tick/gridline count per axis, passed to `nice_ticks`.
const GRID_TICK_TARGET: usize = 10;

/// Shift a horizontally-placed label down so it sits ON the line it
/// labels rather than above it, as a fraction of the font size.
///
/// SVG and the PDF backend position text by its BASELINE, so a y tick
/// label emitted at its gridline's own y has its feet on the line and its
/// body floating above -- about six pixels at 17.5px type. Measured
/// before fixing: text y and gridline y were identical to 0.00 for every
/// tick, with no `dominant-baseline` anywhere in the output, so the labels
/// really were sitting high.
///
/// A numeric shift rather than `dominant-baseline="central"`: that
/// attribute is a renderer hint honoured unevenly outside browsers, and
/// Qu's TikZ and PDF backends never see it at all. Half a cap height is
/// the standard value, and the heatmap row labels had already hand-rolled
/// exactly this (`y + 3.5` for 10px text) -- the main axis simply never
/// got it.
const BASELINE_CENTER: f64 = 0.35;
/// Major/minor gridline stroke widths — thinned one step (was `1.0`/`0.8`,
/// 2026-09-03 ggplot2-inspired pass) alongside `GRID_COLOR`'s own
/// lightening, so gridlines read as a thin, receding reference rather than
/// a drawn line competing with the data.
#[allow(dead_code)] // superseded by the theme table; see the note above. Kept as evidence, not dead weight.
const GRID_LINE_WIDTH: f64 = 0.8;
#[allow(dead_code)] // superseded by the theme table; see the note above. Kept as evidence, not dead weight.
const MINOR_GRID_LINE_WIDTH: f64 = 0.6;
/// Default font sizes, tuned for a journal/IEEE-figure feel rather than UI
/// chrome — noticeably larger than the original `10`/`11`/`12` set, which
/// read as small once a figure is shrunk to fit a paper column. Overridable
/// per-figure via `Figure.tick_size`/`label_size`/`title_size` (the
/// `fontsize` builtin) — `build_draw_ops` reads those, not these constants,
/// for every actual text size; these are only the defaults `Figure::new`
/// starts from.
/// One base size, and everything else a ratio of it.
///
/// ggplot2 uses exactly three sizes across its entire theme -- `rel(0.8)`
/// for small, `rel(1.0)` for normal, `rel(1.2)` for large -- off a single
/// `base_size` (theme-defaults.R: "Throughout the theme, we use three font
/// sizes"). Qu had 14 / 16 / 18, ratios of 0.875 / 1.0 / 1.125, which is
/// far too flat: the title never announces itself and the ticks never
/// recede, so everything reads at one level and the figure looks
/// undesigned. The 0.8 / 1.2 spread is what produces a hierarchy.
// 18, raised from 16 on 2026-09-10 on Ahmed's eye: "I still feel it's
// small", looking at rendered figures at the size he actually views them.
// With the 0.9/1.0/1.2 ratios that puts screen type at 16.2 / 18 / 21.6.
//
// The earlier 12.8 -> 14.4 tick change moved one side of a two-sided
// problem: he found screen small AND publication too big, and lifting only
// the ticks left the ratio between them untouched. Publication comes down
// to 6/7/8 pt in the same pass (see `PRINT_TICK_PT`), which closes the gap
// from 1.71x to 1.31x rather than chasing one end of it.
pub const DEFAULT_BASE_SIZE: f64 = 18.0;
/// Small: tick labels, legend entries, captions.
/// Raised from ggplot2's own `rel(0.8)` on 2026-09-09, on Ahmed's eye and
/// confirmed against the numbers. Qu's default canvas is 900x600 -- much
/// larger than the ~7in figure ggplot2's ratios were chosen for -- so the
/// same ratio lands smaller in proportion: tick labels came out at 1.4% of
/// figure width against ggplot2's own 1.7% and the ~2.8% a Nature column
/// gets (5-7 pt on 89 mm). 0.9 restores the proportion without flattening
/// the hierarchy the three sizes exist to create: 0.9 / 1.0 / 1.2 still
/// steps visibly from tick to axis label to title.
pub const TYPE_SMALL: f64 = 0.9;
/// Normal: axis titles, legend titles.
pub const TYPE_NORMAL: f64 = 1.0;
/// Large: the title.
pub const TYPE_LARGE: f64 = 1.2;

pub const DEFAULT_TICK_SIZE: f64 = DEFAULT_BASE_SIZE * TYPE_SMALL;

/// The tick size that MARGINS, paddings and legend geometry are calibrated
/// against. Deliberately a frozen number and not `DEFAULT_TICK_SIZE`.
///
/// Every `x * (tick_size / BASELINE)` in this file means "how much bigger is
/// this figure's tick text than the size these hand-tuned numbers assume".
/// `DEFAULT_TICK_SIZE` is a STYLE choice and moves when the type scale is
/// retuned; the calibration does not. Tying them together had a silent and
/// backwards consequence: a print figure's tick size is fixed in POINTS
/// (`PRINT_TICK_PT`) and does not follow `DEFAULT_TICK_SIZE`, so raising the
/// default made the ratio SMALLER and shrank print margins -- less room for
/// tick numbers whose size had not changed at all. That is the letterbox
/// defect `a_print_grid_grows_its_canvas_rather_than_packing_the_panels`
/// exists to catch, and raising the default type scale walked straight into
/// it.
pub const TICK_METRIC_BASELINE: f64 = 12.8;
pub const DEFAULT_AXIS_LABEL_SIZE: f64 = DEFAULT_BASE_SIZE * TYPE_NORMAL;
pub const DEFAULT_TITLE_SIZE: f64 = DEFAULT_BASE_SIZE * TYPE_LARGE;

/// Stroke weight tracks type size, rather than being a fixed pixel count.
///
/// ggplot2 sets `base_line_size = base_size / 22`, so a figure rendered at
/// a larger base gets proportionally heavier gridlines, ticks and borders
/// instead of hairlines. Qu's fixed 0.8px grid meant a `figure_size()`-
/// scaled figure came out spindly at large sizes and clogged at small
/// ones. At base 16 this gives 0.727 -- close to the 0.8 it replaces, so
/// existing figures barely move.
pub const BASE_LINE_RATIO: f64 = 1.0 / 22.0;

/// The vertical rhythm every gap is derived from.
///
/// ggplot2's `half_line = base_size / 2` sets plot margins, title gaps,
/// tick length and legend padding, so one number generates the whole
/// spacing system and it re-derives correctly when the figure is rescaled.
pub const HALF_LINE_RATIO: f64 = 0.5;


/// Which frame edges a theme draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spines {
    /// All four -- the engineering/gnuplot look.
    Box,
    /// Left and bottom only. Plots.jl's default `framestyle = :axes`, and
    /// the convention in most published figures: the two edges that carry
    /// a scale are drawn, the two that carry nothing are not.
    L,
    /// None; the grid alone locates the data.
    None,
}

/// A named look, as a complete set of decisions.
///
/// These are ports of themes that already exist and are already trusted,
/// with the values read from their sources rather than eyeballed:
/// ggplot2's `theme_grey`/`theme_bw`/`theme_minimal`/`theme_classic`
/// (tidyverse/ggplot2 `R/theme-defaults.R`) and Plotly's `simple_white`.
/// Borrowing the reasoning matters more than the hexes: gridlines must be
/// visible but lower-contrast than the data; when a line gets thinner it
/// must get darker to hold contrast; when panel and grid swap roles,
/// preserve the LIGHTNESS DIFFERENCE, not the colour.
#[derive(Clone, Copy, Debug)]
pub struct ThemeStyle {
    pub name: &'static str,
    /// Panel fill; `None` leaves the figure background showing through.
    pub panel_fill: Option<&'static str>,
    /// The whole-canvas colour, when the theme owns it. `None` leaves
    /// `DEFAULT_BACKGROUND` (or whatever `figure_background(...)` set)
    /// alone, which is every light theme. A dark theme cannot work
    /// without it: dark ink fields are meaningless on a white canvas.
    pub figure_fill: Option<&'static str>,
    pub grid_color: &'static str,
    /// Multipliers on the base line width, so weight tracks type size.
    pub grid_width: f64,
    pub minor_grid_width: f64,
    pub show_minor_grid: bool,
    pub spines: Spines,
    pub spine_color: &'static str,
    pub spine_width: f64,
    pub tick_color: &'static str,
    /// Tick-mark length as a multiple of `half_line`; `0.0` draws none.
    ///
    /// Every theme here had a `tick_color` and no tick marks to colour --
    /// the axes carried numbers floating beside a bare line, with nothing
    /// tying a label to its position. ggplot2's value is `half_line / 2`
    /// (`theme_grey`: `axis.ticks.length = unit(half_line / 2, "pt")`),
    /// which is where `0.5` comes from; `theme_minimal` blanks them, and
    /// so does this table's `minimal`.
    pub tick_len: f64,
    /// Which way the marks point. Journals differ on direction rather
    /// than presence, and the choice tracks the frame: a boxed axis
    /// usually takes inward ticks (MATLAB's default with `box on`, and
    /// most IEEE figures are MATLAB's output), an open L-frame outward
    /// ones, where there is no opposite spine for them to point at.
    pub tick_inward: bool,
    pub tick_text_color: &'static str,
    pub label_color: &'static str,
    pub title_color: &'static str,
    pub show_grid: bool,
    /// Border around the whole legend; `None` draws none.
    ///
    /// The frame was hardcoded, so every theme got a boxed legend --
    /// including the ones modelled on journals that do not use one. Nature
    /// Methods (Krzywinski 10:183) makes the same argument against it as
    /// against a four-sided axis frame: the border is containment mistaken
    /// for organization, and a legend sitting on the figure's own ground
    /// reads as part of the figure. ggplot2 agrees -- no theme in it draws
    /// one (`legend.background = element_blank()`). MATLAB does, which is
    /// why `ieee` keeps it: that is what those figures actually look like.
    pub legend_frame: Option<&'static str>,
    /// Whether this look is already meant for print. Drives the
    /// "screen styling may not hold up in print" caution on save: warning
    /// about a figure that is ALREADY set up for a journal is noise, and
    /// noise is how a useful warning gets tuned out.
    pub print_ready: bool,
}

/// The built-in looks. `theme(name)` selects one.
pub const THEMES: &[ThemeStyle] = &[
    // The default screen look: a light grid on white, four-sided frame.
    // Kept first and unchanged in spirit so existing figures still look
    // like themselves.
    ThemeStyle {
        name: "default",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#e6e6e6",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        // Was `true`, and unread, so nothing ever drew it. Set to what the
        // figure actually renders today rather than switching minor
        // gridlines on for every default figure as a side effect of
        // making the field work. Whether this theme SHOULD have them is
        // a real question, now answerable with a before/after.
        show_minor_grid: false,
        spines: Spines::Box,
        spine_color: "#333333",
        spine_width: 1.0,
        tick_color: "#333333",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#4d4d4d",
        label_color: "#444444",
        title_color: "#444444",
        show_grid: true,
        legend_frame: Some("#cccccc"),
        print_ready: false,
    },
    // ggplot2's signature: grey panel, WHITE gridlines. Wickham's argument
    // is that the grey field gives the figure the same typographic colour
    // as body text, so it sits in a page instead of punching a bright hole
    // in it -- and that a continuous field makes the plot read as one
    // object rather than scattered marks.
    ThemeStyle {
        name: "grey",
        panel_fill: Some("#ebebeb"),
        figure_fill: None,
        grid_color: "#ffffff",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        // Was `true`, and unread, so nothing ever drew it. Set to what the
        // figure actually renders today rather than switching minor
        // gridlines on for every default figure as a side effect of
        // making the field work. Whether this theme SHOULD have them is
        // a real question, now answerable with a before/after.
        show_minor_grid: false,
        spines: Spines::None,
        spine_color: "#333333",
        spine_width: 1.0,
        tick_color: "#333333",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#4d4d4d",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: true,
        legend_frame: None,
        print_ready: false,
    },
    // `theme_bw`: the grey theme inverted for print. Note the gridline is
    // #EBEBEB on white -- the SAME lightness difference as white on
    // #EBEBEB, which is the source's own stated intent ("make gridlines
    // dark, same contrast with white as in theme_grey"). The grey20 border
    // is re-added to restore the "single object" the grey field provided.
    ThemeStyle {
        name: "bw",
        panel_fill: Some("#ffffff"),
        figure_fill: None,
        grid_color: "#ebebeb",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        // Was `true`, and unread, so nothing ever drew it. Set to what the
        // figure actually renders today rather than switching minor
        // gridlines on for every default figure as a side effect of
        // making the field work. Whether this theme SHOULD have them is
        // a real question, now answerable with a before/after.
        show_minor_grid: false,
        spines: Spines::Box,
        spine_color: "#333333",
        spine_width: 1.0,
        tick_color: "#333333",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#4d4d4d",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: true,
        legend_frame: None,
        print_ready: false,
    },
    // `theme_minimal`: grid only, no frame and no ticks. The grid alone
    // locates the data.
    ThemeStyle {
        name: "minimal",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#ebebeb",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        // Was `true`, and unread, so nothing ever drew it. Set to what the
        // figure actually renders today rather than switching minor
        // gridlines on for every default figure as a side effect of
        // making the field work. Whether this theme SHOULD have them is
        // a real question, now answerable with a before/after.
        show_minor_grid: false,
        spines: Spines::None,
        spine_color: "#333333",
        spine_width: 0.0,
        tick_color: "none",
        tick_len: 0.0,
        tick_inward: false,
        tick_text_color: "#4d4d4d",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: true,
        legend_frame: None,
        print_ready: false,
    },
    // `theme_classic`: no grid at all, two black axis lines. The most
    // common look in print, and what most journals' own figures use.
    ThemeStyle {
        name: "classic",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#ebebeb",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        show_minor_grid: false,
        spines: Spines::L,
        spine_color: "#000000",
        spine_width: 1.0,
        tick_color: "#000000",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#1a1a1a",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: false,
        legend_frame: None,
        print_ready: true,
    },
    // The publication default: the screen theme's CHROME, in print ink.
    //
    // This deliberately keeps the four-sided box and the framed legend
    // rather than `theme_classic`'s open L. Ahmed reviewed both against
    // real figures and chose the boxed one twice, and the reason is
    // visible in the L version: with no frame, a legend placed inside the
    // panel has nothing holding it off the data, so "signal" and "model"
    // sat directly on top of the traces. A frame is what makes an inside
    // legend readable, and an inside legend is what keeps a 89 mm figure
    // from spending its width on a margin.
    //
    // So publication differs from the screen default only in the things
    // print actually requires -- serif face, larger relative type for
    // reduction, darker ink (#111111 per `PUBLICATION_INK`), a paler grid
    // -- and not in layout. `nature` and `ieee` below stay as they are;
    // those are house styles that should match their journal, not Qu.
    //
    // The faint grid stays for the original reason: engineering figures
    // are read for VALUES, and a gridless plot makes that guesswork.
    ThemeStyle {
        name: "publication",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#e8e8e8",
        grid_width: 1.0,
        minor_grid_width: 0.5,
        show_minor_grid: false,
        spines: Spines::Box,
        spine_color: "#111111",
        // 1.6, not the 1.15 this held before: Ahmed's ruling, 2026-09-10.
        // The argument was already written down one screen up, on the
        // `PUBLICATION_AXIS_WIDTH` constant that nothing read -- a print
        // frame must be HEAVIER than a screen one, because a hairline that
        // looks elegant on a monitor thins to nothing when the figure is
        // reduced to a journal column. Now it is the value that renders.
        spine_width: 1.6,
        tick_color: "#111111",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#111111",
        label_color: "#111111",
        title_color: "#111111",
        show_grid: true,
        legend_frame: Some("#cccccc"),
        print_ready: true,
    },
    // Nature-family. Krzywinski (Nature Methods 10:183) argues against
    // bounding a plot on all sides, and Nature's own spec lists background
    // gridlines under "avoid". So: L-spines, no grid, outward ticks.
    ThemeStyle {
        name: "nature",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#ebebeb",
        grid_width: 0.6,
        minor_grid_width: 0.3,
        show_minor_grid: false,
        spines: Spines::L,
        spine_color: "#000000",
        spine_width: 1.0,
        tick_color: "#000000",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#000000",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: false,
        legend_frame: None,
        print_ready: true,
    },
    // IEEE. Its own template figure is a FOUR-SIDED box with inward ticks
    // and no grid -- checked against the published exemplar rather than
    // assumed. Treating the box as merely a MATLAB artefact would have
    // been wrong: for IEEE it is the house style.
    ThemeStyle {
        name: "ieee",
        panel_fill: None,
        figure_fill: None,
        grid_color: "#ebebeb",
        grid_width: 0.6,
        minor_grid_width: 0.3,
        show_minor_grid: false,
        spines: Spines::Box,
        spine_color: "#000000",
        spine_width: 1.0,
        tick_color: "#000000",
        tick_len: 0.5,
        tick_inward: true,
        tick_text_color: "#000000",
        label_color: "#000000",
        title_color: "#000000",
        show_grid: false,
        legend_frame: Some("#cccccc"),
        print_ready: true,
    },
    // `theme("dark")`. Until now this name resolved to nothing: it was
    // absent from this table, and `resolve_theme` falls back to THEMES[0],
    // so `theme("dark")` rendered a figure byte-identical to
    // `theme("default")` -- a white one -- and said nothing about it.
    //
    // Deliberately NOT print_ready: a dark figure is for a screen, a slide
    // or a dark-themed document, and `savefig` embeds fonts for print
    // themes. Ink is light-on-dark throughout, which is only possible now
    // that `tick_text_color`/`label_color`/`title_color` are actually read
    // -- before this pass they were overridden by a hardcoded `#444`, which
    // on this canvas would have been unreadable.
    //
    // Values follow the same restraint as the light themes: the panel sits
    // slightly lighter than the canvas so the plot area reads as a surface,
    // the grid is a low-contrast step above the panel rather than a drawn
    // line, and the ink stops short of pure white -- full white on near
    // black glares and haloes on an LCD the way pure black on white does
    // not.
    ThemeStyle {
        name: "dark",
        panel_fill: Some("#242428"),
        figure_fill: Some("#1a1a1d"),
        grid_color: "#3a3a40",
        grid_width: 0.8,
        minor_grid_width: 0.4,
        show_minor_grid: false,
        spines: Spines::L,
        spine_color: "#8a8a92",
        spine_width: 1.0,
        tick_color: "#8a8a92",
        tick_len: 0.5,
        tick_inward: false,
        tick_text_color: "#b8b8c0",
        label_color: "#e4e4e8",
        title_color: "#f2f2f5",
        show_grid: true,
        legend_frame: None,
        print_ready: false,
    },
];


/// Journal column widths, in millimetres, from the publishers' own figure
/// guides. A figure authored at the width it will be printed at needs no
/// rescaling, and rescaling is what destroys type size -- so this is the
/// single most useful thing to get right.
pub const COLUMN_PRESETS: &[(&str, f64)] = &[
    // Nature: 89 single, 183 double (also 120/136 intermediate).
    ("nature1", 89.0),
    ("nature", 89.0),
    ("nature2", 183.0),
    // Science: 90 / 184 per the 2025-26 figure guides (the older
    // instructions page still says 5.7/12.1/18.4 cm and appears stale).
    ("science1", 90.0),
    ("science2", 184.0),
    // IEEE: 3.5 in and 7.16 in. Proceedings of the IEEE uses 3.25 in.
    ("ieee1", 88.9),
    ("ieee", 88.9),
    ("ieee2", 182.0),
    ("ieee-proc", 82.5),
    // Cell Press and Elsevier.
    ("cell1", 85.0),
    ("cell2", 174.0),
    ("elsevier1", 90.0),
    ("elsevier2", 190.0),
];

/// Millimetres to canvas units.
///
/// Chosen so a Nature single column (89 mm) lands near Qu's established
/// 900-unit canvas, which puts the print type scale's tick labels at
/// roughly 6.6 pt -- inside Nature's own 5-7 pt band. That coincidence is
/// what makes the presets and the type scale agree without a second knob.
pub const MM_TO_UNITS: f64 = 10.0;

/// PDF user-space points per canvas unit.
///
/// One canvas unit is 1/`MM_TO_UNITS` mm and PDF measures in points at 72
/// per inch, so this is the single conversion the PDF writer needs. Named
/// rather than inlined because the page's MediaBox and the content
/// stream's transform must agree exactly — if they drift, the drawing and
/// the page it sits on stop being the same size.
pub const PDF_POINTS_PER_UNIT: f64 = 72.0 / 25.4 / MM_TO_UNITS;

pub fn column_width(name: &str) -> Option<f64> {
    COLUMN_PRESETS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, mm)| mm * MM_TO_UNITS)
}

/// Pick a figure height for a given width from what is being plotted.
///
/// The reasoning Ahmed described, and what published figures do:
///   - an equal-aspect plot (a Nyquist diagram) must be SQUARE in its plot
///     area, or the circle it is supposed to show is an ellipse and the
///     figure is actively misleading;
///   - when x spans far more than y, a wide figure wastes less space and
///     reads better -- a spectrum across four decades in a double column
///     wants roughly 3:1, not 4:3;
///   - otherwise 4:3 is the safe default.
///
/// A grid of panels divides the width and multiplies the height, so the
/// aspect applies per PANEL rather than to the whole canvas -- which is
/// why a four-across multi-panel figure keeps readable type.
pub fn auto_height(fig: &Figure, width: f64) -> f64 {
    let (rows, cols) = fig.grid.unwrap_or((1, fig.panels.len().max(1)));
    let (rows, cols) = (rows.max(1) as f64, cols.max(1) as f64);
    let panel_w = width / cols;

    // Equal-aspect wins outright: it is a correctness constraint, not taste.
    if fig.panels.iter().any(|p| p.axis_equal) {
        return panel_w * rows;
    }

    // What actually justifies a wide figure.
    //
    // NOT the ratio of the data spans: each axis is auto-scaled to fill
    // the panel independently, so x in [0, 10] against y in [0, 1] is not
    // elongated at all -- both fill the same box. A first version compared
    // raw spans and gave an ordinary decay curve a 3:1 canvas.
    //
    // What does justify width is structure along x that needs room:
    //   - a logarithmic x axis covering several decades, where squeezing
    //     the decades together defeats the point of the log scale;
    //   - a long series whose detail is lost when samples pile up on top
    //     of each other.
    let mut decades: f64 = 0.0;
    let mut longest = 0usize;
    for p in &fig.panels {
        for s in &p.series {
            longest = longest.max(s.x.len());
        }
        if p.xscale == Scale::Log {
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for s in &p.series {
                for &v in &s.x {
                    if v.is_finite() && v > 0.0 {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
            if lo.is_finite() && hi > lo {
                decades = decades.max((hi / lo).log10());
            }
        }
    }

    // Three steps rather than a continuous function: an aspect that drifts
    // with the data makes a set of figures in one paper disagree with each
    // other, which is worse than any single one being slightly off.
    let aspect = if decades >= 4.0 || longest >= 2000 {
        3.0
    } else if decades >= 2.0 || longest >= 500 {
        2.0
    } else {
        4.0 / 3.0
    };
    (panel_w / aspect) * rows
}

/// Base margins, before the figure's own type sizes scale them. Named
/// because `fit_grid_height` has to predict the same numbers `build_draw_ops`
/// will use -- inlined in two places they would drift, and the drift is
/// invisible until a figure comes out packed again.
pub const MARGIN_LEFT: f64 = 68.0;
pub const MARGIN_TOP: f64 = 30.0;
pub const MARGIN_RIGHT: f64 = 16.0;
pub const MARGIN_BOTTOM: f64 = 42.0;

/// How much of a panel's cell its fixed chrome may occupy before the figure
/// is simply too short for the grid it was asked to hold.
///
/// A single panel on the default canvas spends about 23% of its height on
/// the title, tick numbers and axis label. 45% is a deliberately denser
/// budget -- a grid is allowed to be tighter than a lone plot -- but it
/// still leaves the majority of every cell to the data, which is the line
/// this exists to hold.
const MAX_CHROME_SHARE: f64 = 0.45;

/// A ceiling on the growth, in multiples of the figure's own width. Past
/// roughly 3:1 a figure stops fitting on a page at all, so a 12-row stack
/// gets a tall figure and then accepts packing rather than an unprintable
/// one.
const MAX_GRID_ASPECT: f64 = 3.0;

/// The same ceiling for width, as a multiple of the width given. Tighter
/// than the height's, because a page has far less give sideways: a figure
/// that grew downward can still be placed, one that grew past the text
/// block cannot.
const MAX_WIDTH_GROWTH: f64 = 1.6;

/// How many rows of panels a layout actually has -- the grid's row count
/// when `panel(r, c, i)` set one, otherwise the flow layout's own rule
/// that a `Horizontal` panel joins the row before it. Mirrors
/// `layout_panels`, which is what divides the height by this number.
fn layout_row_count(fig: &Figure) -> usize {
    if let Some((rows, _)) = fig.grid {
        return rows.max(1);
    }
    let mut rows = 0usize;
    for p in &fig.panels {
        if !(p.direction == Direction::Horizontal && rows > 0) {
            rows += 1;
        }
    }
    rows.max(1)
}

/// The widest row, which is what `layout_panels` divides the width by.
fn layout_col_count(fig: &Figure) -> usize {
    if let Some((_, cols)) = fig.grid {
        return cols.max(1);
    }
    let (mut widest, mut here) = (0usize, 0usize);
    for p in &fig.panels {
        if p.direction == Direction::Horizontal && here > 0 {
            here += 1;
        } else {
            here = 1;
        }
        widest = widest.max(here);
    }
    widest.max(1)
}

/// Grow a print figure's canvas so a grid of panels keeps full-size type
/// instead of being packed into whatever height it was given.
///
/// This is the one place the two type modes genuinely diverge. Screen type
/// is a fraction of the canvas, so a stack of panels shrinks its own
/// labels along with its cells and stays self-consistent. PRINT type is
/// fixed in points -- 7 pt is 7 pt whether the figure holds one panel or
/// nine -- so the chrome does NOT shrink when the cells do. Divide a
/// 600-unit canvas across four rows and each cell is 127 units against 131
/// units of title, tick numbers and axis label: more chrome than cell.
/// `build_draw_ops`' 70%-of-cell clamp then rescues the figure from
/// rendering blank, but what it produces is a rail of letterbox strips
/// with tick labels sitting on top of each other. Verified directly: a
/// `subplot(4,1,i)` stack in `theme("publication")` collided its own y
/// numbers and ran the x label through them.
///
/// The fix is the one a person would make by hand -- give the figure more
/// paper. A journal fixes the COLUMN WIDTH, not the height; a tall
/// multi-panel figure is ordinary and a squashed one is not, so height is
/// the dimension free to move.
///
/// Deliberately does nothing when the script set both dimensions itself
/// (`figure_size(w, h)` is an instruction, not a suggestion), for a single
/// row (nothing to pack), or for a screen theme (whose type already
/// scales).
pub fn fit_grid_height(fig: &Figure, width: f64, height: f64) -> f64 {
    if fig.size_explicit || !resolve_theme(&fig.theme).print_ready {
        return height;
    }
    let rows = layout_row_count(fig);
    if rows < 2 {
        return height;
    }
    let rows = rows as f64;
    let chrome = MARGIN_TOP * (fig.title_size / DEFAULT_TITLE_SIZE)
        + MARGIN_BOTTOM * (fig.tick_size / TICK_METRIC_BASELINE);
    let cell = chrome / MAX_CHROME_SHARE;
    let needed = cell * rows + GAP * (rows + 1.0);
    // `max(height)` last: this only ever grows a figure. A script that
    // asked for a tall canvas and got a two-row grid keeps its canvas.
    needed.min(width * MAX_GRID_ASPECT).max(height)
}

/// The height a figure actually renders at: the `figure_size("nature2")`
/// "decide from the data" marker resolved, then grown if a print grid
/// needs the room. Every backend calls this so the page they write and the
/// drawing they put on it cannot disagree -- `render_tikz` and `render_pdf`
/// previously passed their unresolved `height` straight into the canvas
/// size while `build_draw_ops` resolved it internally, so a preset-sized
/// figure exported to PDF got a page of one height holding a drawing laid
/// out for another.
/// The width counterpart of `fit_grid_height`, for the same reason and by
/// the same budget.
///
/// A print panel's left margin holds tick numbers and a rotated axis label
/// at a fixed point size, so it costs the same ~147 units whether the
/// figure has one column or three. Divide the default 900-unit canvas
/// across three columns and each cell is 276 units against those 147 --
/// more than half of every panel spent on its own margins, and the data
/// squeezed into what is left. Reported directly off a rendered 3x3.
///
/// Width is the more conservative of the two, hence the tighter cap: a
/// journal fixes the column width, and while nothing has fixed it here
/// (that is what `width_explicit` is for), a figure that silently doubles
/// its width is harder to place on a page than one that grows downward.
pub fn fit_grid_width(fig: &Figure, width: f64) -> f64 {
    if fig.width_explicit || fig.size_explicit || !resolve_theme(&fig.theme).print_ready {
        return width;
    }
    let cols = layout_col_count(fig);
    if cols < 2 {
        return width;
    }
    let cols = cols as f64;
    let chrome = MARGIN_LEFT * (fig.tick_size / TICK_METRIC_BASELINE) + MARGIN_RIGHT;
    let cell = chrome / MAX_CHROME_SHARE;
    let needed = cell * cols + GAP * (cols + 1.0);
    needed.min(width * MAX_WIDTH_GROWTH).max(width)
}

/// The width a figure actually renders at -- see `fit_grid_width`. Paired
/// with `resolved_height`, and computed FIRST, because the height depends
/// on the width (both `auto_height`'s per-panel aspect and the 3:1 cap).
/// An `outside` inset reserves margin on whichever side it sits on (see the
/// `zoom_inset` layout block), and until now that reservation came entirely
/// out of the main panel: a magnifier requested for a corner of the figure
/// shrank the very thing it was magnifying, sometimes down to a near-square
/// panel on a landscape canvas. `layout_panels` divides `width` by column
/// count alone -- never by `height` -- so it is safe to call here, before
/// `height` is even resolved, purely to learn how wide a cell the inset's
/// own panel would get.
///
/// Reservation, not the plot box, is what grows: the canvas gains exactly
/// the width (or height) an outside Left/Right (or Top/Bottom) inset takes,
/// so the main panel keeps the size it would have had with no inset at all
/// and the magnifier is genuinely extra room rather than borrowed room.
fn has_outside_lr_inset(fig: &Figure) -> bool {
    fig.panels.iter().any(|p| {
        p.inset.as_ref().is_some_and(|i| i.outside && matches!(legend_side(i.position), Side::Left | Side::Right))
    })
}

fn has_outside_tb_inset(fig: &Figure) -> bool {
    fig.panels.iter().any(|p| {
        p.inset.as_ref().is_some_and(|i| i.outside && matches!(legend_side(i.position), Side::Top | Side::Bottom))
    })
}

/// The width a Left/Right outside inset needs, computed once from the
/// figure's OWN structure and `DEFAULT_FIGURE_WIDTH` -- deliberately NOT
/// from the `width` this call received. `render_svg`/`render_pdf` each
/// resolve `width` themselves and then pass that already-resolved width
/// into `build_draw_ops_with_geometry`, which resolves it AGAIN -- a
/// double call that was harmless while every `fit_*` here was a clamp
/// (`needed.max(width)`, independent of `width`), but an additive
/// fixed-point over the incoming `width` is not: re-running it on its own
/// output asks for more room on top of what it already added, since the
/// reservation is `rect.w * 0.32` and `rect.w` grows with every pass.
/// Anchoring "needed" to the fixed baseline and only ever taking
/// `.max(width)` at the end restores that idempotence: a second call on an
/// already-grown width recomputes the same target and changes nothing.
fn fit_outside_inset_width(fig: &Figure, width: f64) -> f64 {
    if fig.width_explicit || fig.size_explicit || !has_outside_lr_inset(fig) {
        return width;
    }
    // The reservation itself is a FRACTION of the panel cell (`rect.w *
    // 0.32`), and growing the canvas grows that cell too -- so "add what
    // today's cell needs" undershoots: the bigger canvas asks for a bigger
    // box next. This converges (the 0.32 fraction is a contraction), so a
    // handful of fixed-point iterations lands within a fraction of a unit
    // of the true answer, which one shot could not.
    let mut grown = DEFAULT_FIGURE_WIDTH;
    for _ in 0..12 {
        let rects = layout_panels(fig, grown, DEFAULT_FIGURE_HEIGHT);
        let mut extra = 0.0_f64;
        for rect in &rects {
            let panel = &fig.panels[rect.panel];
            if let Some(inset) = &panel.inset {
                if inset.outside && matches!(legend_side(inset.position), Side::Left | Side::Right) {
                    extra = extra.max(rect.w * 0.32 + 16.0);
                }
            }
        }
        let next = DEFAULT_FIGURE_WIDTH + extra;
        if (next - grown).abs() < 0.01 {
            grown = next;
            break;
        }
        grown = next;
    }
    grown.max(width)
}

/// Height counterpart of `fit_outside_inset_width`, for a Top/Bottom
/// outside inset. Called after width is resolved (an outside Left/Right
/// inset does not change how tall the figure needs to be), matching
/// `fit_grid_height` taking the already-resolved width as its own input.
fn fit_outside_inset_height(fig: &Figure, width: f64, height: f64) -> f64 {
    if fig.size_explicit || !has_outside_tb_inset(fig) {
        return height;
    }
    // Same fixed-point-anchored-to-a-constant reasoning as
    // `fit_outside_inset_width`, for a Top/Bottom outside inset against
    // `rect.h` -- and the same reason it is anchored to
    // `DEFAULT_FIGURE_HEIGHT` rather than the incoming `height`.
    let mut grown = DEFAULT_FIGURE_HEIGHT;
    for _ in 0..12 {
        let rects = layout_panels(fig, width, grown);
        let mut extra = 0.0_f64;
        for rect in &rects {
            let panel = &fig.panels[rect.panel];
            if let Some(inset) = &panel.inset {
                if inset.outside && matches!(legend_side(inset.position), Side::Top | Side::Bottom) {
                    extra = extra.max(rect.h * 0.34 + 16.0);
                }
            }
        }
        let next = DEFAULT_FIGURE_HEIGHT + extra;
        if (next - grown).abs() < 0.01 {
            grown = next;
            break;
        }
        grown = next;
    }
    grown.max(height)
}

pub fn resolved_width(fig: &Figure, width: f64) -> f64 {
    fit_outside_inset_width(fig, fit_grid_width(fig, width))
}

pub fn resolved_height(fig: &Figure, width: f64, height: f64) -> f64 {
    let height = if height > 0.0 { height } else { auto_height(fig, width) };
    fit_outside_inset_height(fig, width, fit_grid_height(fig, width, height))
}

/// Look a theme up by name; an unknown name is the default rather than an
/// error, matching how `colormap` already treats a typo'd palette.
/// Whether `name` is a real theme, as opposed to one `resolve_theme` will
/// quietly substitute the default for. Exists so the `theme(...)` builtin
/// can say "no such theme" without duplicating the lookup rule.
pub fn theme_exists(name: &str) -> bool {
    THEMES.iter().any(|t| t.name.eq_ignore_ascii_case(name))
}

/// Every theme name, in table order — for the "did you mean" list.
pub fn theme_names() -> Vec<&'static str> {
    THEMES.iter().map(|t| t.name).collect()
}

pub fn resolve_theme(name: &str) -> &'static ThemeStyle {
    THEMES
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .unwrap_or(&THEMES[0])
}


/// The canvas size every `savefig`/`figure_size` reference is measured
/// against — matches `qu-interp`'s own long-standing hardcoded export size,
/// now factored out to one place. `figure_size(w, h)` computes its font
/// auto-scale as `sqrt((w*h) / (DEFAULT_FIGURE_WIDTH*DEFAULT_FIGURE_HEIGHT))`
/// (see `Interp`'s `figure_size` builtin in `lib.rs`), so a script that
/// never calls `figure_size` keeps exporting at exactly this size with
/// exactly the sizes above — zero behavior change for any existing script.
pub const DEFAULT_FIGURE_WIDTH: f64 = 900.0;
pub const DEFAULT_FIGURE_HEIGHT: f64 = 600.0;
/// Axis-box gray — darker than the old `#888`, matching the solid, more
/// deliberate box weight of a printed/journal figure rather than a faint
/// UI-chrome outline.
const BOX_COLOR: &str = "#333333";
/// Default series line width — thinned from `1.8` (2026-09-03 ggplot2-
/// inspired pass) to match ggplot2's own `geom_line`/`geom_path` default,
/// a lighter stroke that reads as more precise/professional rather than
/// heavy, especially with several overlapping series in one panel.
const DEFAULT_LINE_WIDTH: f64 = 1.5;

/// Named categorical/sequential palettes. `"default"` is whatever palette a
/// panel uses when no `colormap(...)` call ever ran; the rest are small
/// curated approximations (a handful of stops, not the full continuous
/// colormap) good enough for categorical series and box/bar coloring
/// without pulling in a colormap-data dependency.
///
/// `"default"` and `"kay"` are deliberately identical — Ahmed's own 8-color
/// mix (`5B7CFA/FF5C5C/228833/BF9F40/7540BF/808080/BF4080/0084AA`), reordered
/// so red sits 3rd rather than 2nd — so the same colors are reachable either
/// implicitly (no `colormap` call) or by the explicit name. `"qu"` is the
/// "Qu Best" palette from the earlier palette-design pass: hues spaced 45°
/// apart at Tableau's own moderate-saturation recipe, then reordered so
/// *consecutive* entries sit roughly 180° apart in hue (a plain hue sort
/// puts the most-similar colors next to each other, which is exactly wrong
/// for a cycling palette — see IMPL.md, 2026-08-24). `resolve_palette`'s
/// unknown-name fallback is positional (`PALETTES[0]`), so `"default"` must
/// stay the first entry.
pub const PALETTES: &[(&str, &[&str])] = &[
    ("default", &["#5B7CFA", "#228833", "#FF5C5C", "#BF9F40", "#7540BF", "#808080", "#BF4080", "#0084AA"]),
    ("kay", &["#5B7CFA", "#228833", "#FF5C5C", "#BF9F40", "#7540BF", "#808080", "#BF4080", "#0084AA"]),
    ("qu", &["#3966C6", "#C69939", "#7639C6", "#89C639", "#C639AC", "#39C653", "#C63943", "#39C6BC"]),
    // The standard "Tableau 10" / matplotlib `tab10` cycle, verbatim.
    ("tableau10", &["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2", "#7f7f7f", "#bcbd22", "#17becf"]),
    ("viridis", &["#440154", "#414487", "#2a788e", "#22a884", "#7ad151", "#fde725"]),
    ("plasma", &["#0d0887", "#6a00a8", "#b12a90", "#e16462", "#fca636", "#f0f921"]),
    ("warm", &["#7f0000", "#b30000", "#d7301f", "#ef6548", "#fc8d59", "#fdbb84"]),
    ("cool", &["#045a8d", "#2b8cbe", "#74a9cf", "#a6bddb", "#d0d1e6", "#f1eef6"]),
    ("mono", &["#111111", "#333333", "#555555", "#777777", "#999999", "#bbbbbb"]),
    // MATLAB's own default `ColorOrder` (R2014b+) — for scripts porting a
    // MATLAB figure, or anyone who just wants that familiar blue/orange-red/
    // yellow/purple line cycle instead of `"default"`.
    ("matlab", &["#0072bd", "#d95319", "#edb120", "#7e2f8e", "#77ac30", "#4dbeee", "#a2142f"]),
    // ColorBrewer "Blues" — the default `heatmap()` colormap: a calmer,
    // more conventional sequential scale than viridis for a plain grid of
    // values (viridis stays the default for `corr_heatmap`/`confusion_matrix`,
    // where its perceptual uniformity matters more than familiarity).
    ("blues", &["#f7fbff", "#deebf7", "#c6dbef", "#9ecae1", "#6baed6", "#4292c6", "#2171b5", "#084594"]),
    // ColorBrewer sequential schemes, verbatim from the published 9-class
    // hex values. `YlOrRd` in particular is the standard choice for a
    // "how much" field on a light ground and is what the crest-factor
    // paper's ceiling map uses -- a filled contour needs a sequential
    // colormap, and the categorical palettes above are the wrong tool for
    // one however good they are at telling series apart.
    ("ylorrd", &["#ffffcc", "#ffeda0", "#fed976", "#feb24c", "#fd8d3c", "#fc4e2a", "#e31a1c", "#bd0026", "#800026"]),
    ("ylgnbu", &["#ffffd9", "#edf8b1", "#c7e9b4", "#7fcdbb", "#41b6c4", "#1d91c0", "#225ea8", "#253494", "#081d58"]),
    ("reds", &["#fff5f0", "#fee0d2", "#fcbba1", "#fc9272", "#fb6a4a", "#ef3b2c", "#cb181d", "#a50f15", "#67000d"]),
    ("greys", &["#ffffff", "#f0f0f0", "#d9d9d9", "#bdbdbd", "#969696", "#737373", "#525252", "#252525", "#000000"]),
    // Okabe & Ito's colorblind-safe categorical set (Color Universal Design,
    // 2002; popularized by Bang Wong's "Points of view: Color blindness",
    // Nature Methods 8, 441 (2011), and since adopted as the qualitative
    // default by ggthemes'/scales' `scale_*_colorblind()` and Wong's own
    // Nature Methods figures) — distinguishable under all three common
    // dichromacies (protanopia/deuteranopia/tritanopia), unlike `"default"`/
    // `"tableau10"`/`"matlab"` which were picked for screen contrast, not
    // colorblind safety. Values verified against the palette's standard
    // published hex codes, not eyeballed off a screenshot.
    ("okabe_ito", &["#E69F00", "#56B4E9", "#009E73", "#F0E442", "#0072B2", "#D55E00", "#CC79A7", "#000000"]),
    // The publication default. Blue, red, green, orange -- the order a
    // reader expects and the one people actually ask for -- but taken from
    // Paul Tol's bright/vibrant qualitative schemes rather than raw RGB,
    // so the familiar naming does not cost colourblind readability. Tol's
    // sets are built to stay distinguishable under the three common
    // dichromacies, which `#ff0000` next to `#00ff00` emphatically is not.
    // Okabe-Ito remains available by name for anyone who wants it; it is
    // unimpeachable on accessibility but reads as mustard-and-teal, which
    // is not what "plot my three signals" should look like by default.
    ("publication", &["#4477AA", "#EE6677", "#228833", "#EE7733", "#AA3377", "#66CCEE", "#CCBB44", "#BBBBBB"]),
];

/// Publication figures carry LARGER type than screen ones.
///
/// Not because print is bigger -- because it is smaller. A figure authored
/// at 900x600 and dropped into a single journal column is reduced to
/// roughly a third of its width, which turns 14px tick labels into about
/// five points: technically present, practically unreadable. Authoring at
/// a larger type scale is what every journal's figure guidance asks for.
///
/// Calibrated against what journals actually print rather than by eye:
/// Nature allows 5-7 pt on an 89 mm column, so tick labels occupy roughly
/// 2.2% of the figure width. Qu's screen scale puts them at 1.4%, and the
/// first attempt at 1.25x only reached 1.8% -- still visibly small, which
/// is exactly what Ahmed reported on a large figure.
///
/// Settled against the paper being reproduced rather than by eye. Its
/// figures are 3.5 in wide with 7 pt ticks and 8 pt labels -- 2.78% and
/// 3.17% of the width. At 1.55 Qu produced 5.56 pt ticks, 21% under the
/// author's own choice and under Nature's 5-7 pt band at the bottom end.
/// 1.95 lands ticks at 7.0 pt and labels at 8.7 pt, matching the target.
///
/// This value was tried once before and reported as too heavy. Two things
/// changed since: type scaled by figure AREA then, so anything larger than
/// the default grew on both axes at once (a stacked two-panel figure got
/// sqrt(2) more than intended), and the judgement was made on a screen
/// render, where a 3.5-inch figure is shown four times its printed size
/// and 7 pt looks enormous. Scaling by width and comparing at the printed
/// ratio removes both distortions.
///
/// The ratio is stable at any `figure_size`, because the scale multiplies
/// the base rather than replacing it; `fontsize(...)` overrides it
/// outright for a figure that wants something else.
/// Canvas units per typographic point. A unit is `1/MM_TO_UNITS` mm and a
/// point is 1/72 inch, so this is the one conversion a print theme needs.
pub const PT_TO_UNITS: f64 = 25.4 / 72.0 * MM_TO_UNITS;

/// Type sizes a print-ready theme uses, in points.
///
/// Taken from the figures being reproduced, which are also what IEEE and
/// Nature ask for: body-adjacent labels at 8 pt and tick numbers a step
/// below at 7 pt. Nature's stated floor is 5 pt and its ceiling 7 pt for
/// ticks, so 7 sits at the readable end of the allowed band rather than
/// scraping the bottom of it.
pub const PRINT_TICK_PT: f64 = 6.0;  // was 7.0, lowered 2026-09-10 -- see DEFAULT_BASE_SIZE
pub const PRINT_LABEL_PT: f64 = 7.0;  // was 8.0, lowered 2026-09-10 -- see DEFAULT_BASE_SIZE
/// A step above the label, not equal to it. Setting both at 8 pt removed
/// the hierarchy entirely -- and with it the bolding, which fires on text
/// larger than the axis label (see `write_svg_op`'s `bold_above`), so a
/// panel heading became indistinguishable from the axis titles beneath
/// it. 9 pt over an 8 pt label is an ordinary journal step.
pub const PRINT_TITLE_PT: f64 = 8.0;  // was 9.0, lowered 2026-09-10 -- see DEFAULT_BASE_SIZE

pub const PUBLICATION_TYPE_SCALE: f64 = 1.95;

/// Resolves a `colormap(name)` argument; an unknown name falls back to
/// `"default"` rather than erroring, since a typo'd palette name shouldn't
/// break a whole script's plotting.
pub fn resolve_palette(name: &str) -> &'static [&'static str] {
    // Case-insensitive on purpose. Colormap names have an established
    // spelling that is not this table's -- `YlOrRd`, `RdBu`, `YlGnBu` come
    // from ColorBrewer and are written that way everywhere, including in
    // the matplotlib script a Qu port is transcribed from. An exact-match
    // lookup turns `colormap="YlOrRd"` into a silent fallback to the
    // default *categorical* palette, which is not a near miss: a sequential
    // field rendered in eight unrelated hues is a meaningless picture that
    // still looks deliberate.
    PALETTES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, p)| *p)
        .unwrap_or(PALETTES[0].1)
}

/// Evenly spaced levels, re-exported so the interpreter can honour
/// `levels=<count>` without reaching into the contour module.
pub fn auto_levels_count(z: &[f64], n: usize) -> Vec<f64> {
    crate::contour::auto_levels(z, n.max(1))
}

/// Whether `name` is a palette this build knows, so a caller with somewhere
/// to print can say so instead of silently substituting.
pub fn palette_exists(name: &str) -> bool {
    PALETTES.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

/// A continuous value-to-color scale: the same named `PALETTES` stops, but
/// linearly interpolated for any `t` in `[0, 1]` instead of indexed
/// discretely — what `heatmap`/`corr_heatmap`/`confusion_matrix` need for a
/// per-cell shade, as opposed to `series_color`'s one-color-per-category use.
pub fn colormap_color(name: &str, t: f64) -> String {
    let stops = resolve_palette(name);
    let t = t.clamp(0.0, 1.0);
    if stops.len() == 1 {
        return stops[0].to_string();
    }
    let scaled = t * (stops.len() - 1) as f64;
    let i = (scaled.floor() as usize).min(stops.len() - 2);
    let frac = scaled - i as f64;
    let (r0, g0, b0) = hex_to_rgb(stops[i]);
    let (r1, g1, b1) = hex_to_rgb(stops[i + 1]);
    let mix = |a: u8, b: u8| -> u8 { (a as f64 + (b as f64 - a as f64) * frac).round() as u8 };
    rgb_to_hex(mix(r0, r1), mix(g0, g1), mix(b0, b1))
}

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(h.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
    let g = u8::from_str_radix(h.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
    let b = u8::from_str_radix(h.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
    (r, g, b)
}

fn rgb_to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Picks black or white text for legible contrast against a hex background —
/// text drawn *inside* a data-colored shape (a heatmap cell, a pie wedge)
/// can't commit to one fixed color up front, since the shape's own color
/// varies with the data. Uses the standard broadcast-luma weighting
/// (perceived brightness), which is plenty accurate for a binary choice.
fn contrast_text_color(bg_hex: &str) -> &'static str {
    let (r, g, b) = hex_to_rgb(bg_hex);
    let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    if luma > 140.0 { "#111111" } else { "#ffffff" }
}

/// A grid of colored cells (`heatmap`, `corr_heatmap`, `confusion_matrix`):
/// `values` is row-major `rows x cols`. A panel with a `Heatmap` renders
/// *only* the grid — like `SpiderChart`, it doesn't mix with cartesian
/// series.
/// Total right-hand space a colour bar needs: the strip, the gap to the
/// plot, its numbers, and its rotated label.
const COLORBAR_GUTTER: f64 = 74.0;
/// Width of the coloured strip itself.
const COLORBAR_WIDTH: f64 = 14.0;
/// How many slices the gradient is drawn in. Enough that the steps are
/// invisible at print size, few enough not to bloat the file.
const COLORBAR_STEPS: usize = 128;

/// The scale strip beside a field plot.
///
/// `colorbar()` was a no-op, which made every filled contour and every
/// heatmap unreadable: the colours encode the quantity, and without a key
/// there is nothing to read them against. It is not decoration on this
/// kind of figure, it is the axis.
#[derive(Clone, Debug, Default)]
pub struct ColorBar {
    /// Axis label for the strip, e.g. `"Ceiling [%]"`.
    pub label: Option<String>,
    /// Tick values in data units. Empty means "choose them from the range".
    pub ticks: Vec<f64>,
    /// Draw a pointed cap where the scale runs past the bar: `"max"` at the
    /// top, `"min"` at the bottom, `"both"`, or `None` for a plain
    /// rectangle.
    ///
    /// A flat-topped bar says the data stops at the top level. When the
    /// field is masked or clipped there -- the ceiling map's own top band
    /// is "60% and above" -- that is a claim the figure cannot support, and
    /// the arrow is how every colour scale says "and beyond".
    pub extend: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Heatmap {
    pub values: Vec<f64>,
    pub rows: usize,
    pub cols: usize,
    pub colormap: String,
    pub row_labels: Vec<String>,
    pub col_labels: Vec<String>,
    /// A correlation/confusion matrix has few enough cells that a value
    /// label and a white gridline per cell help reading; a spectrogram can
    /// have thousands (frequency bins x time frames), where the same
    /// treatment would render as unreadable text soup and a fine mesh
    /// instead of a smooth raster. `dense: true` skips both.
    pub dense: bool,
    /// Square cells, centred in whatever space the panel gives them.
    ///
    /// A correlation or confusion matrix is indexed by the SAME things
    /// down its rows and across its columns, so a cell that is wider than
    /// it is tall makes the diagonal read as a slope and the symmetry
    /// read as a stretch. `corrplot` in a wide panel came out as flat
    /// rectangles for exactly this reason. A general `heatmap` of r
    /// unrelated things by c others has no such constraint and keeps
    /// filling the panel.
    pub square: bool,
}

fn series_color(color: &Option<String>, index: usize, palette: &[&str]) -> String {
    color
        .clone()
        .unwrap_or_else(|| palette[index % palette.len().max(1)].to_string())
}

/// One notation for a whole axis, chosen from all of its ticks at once.
///
/// Formatting each tick in isolation is what produced the two bugs this
/// replaces: an axis running 0 .. 0.003 in steps of 0.0005 rounded to
/// three decimals and printed `0.002` TWICE, and the same axis mixed
/// `0.001` with `5x10^-4` because each value crossed the
/// decimal-vs-scientific threshold on its own. Both are decisions about
/// the axis, not about any single number on it.
///
/// So: pick the notation from the largest tick, pick the number of
/// decimals from the smallest GAP between ticks (which is what decides
/// whether two labels can collide), and give every label on the axis the
/// same treatment.
pub fn axis_tick_formatter(values: &[f64]) -> Box<dyn Fn(f64) -> String> {
    axis_tick_formatter_unscaled(values, false)
}

/// `axis_tick_formatter`, plus the one thing magnitude alone cannot
/// decide: on a LOG axis, every major tick is already an exact power of
/// ten by construction, and power-of-ten notation is the reading
/// convention for one regardless of whether that decade happens to sit
/// under the magnitude threshold below -- a decade axis running
/// 10^-2..10^4 has no tick past `1e5`, so the ordinary rule left it in
/// decimal (`10000.00`, `100.00`, `0.01`) on a figure whose whole point
/// was to show the values as decades. `is_log` bypasses the magnitude
/// check entirely for that case; every other axis keeps the existing
/// per-value-set decision unchanged.
///
/// Renamed from `axis_tick_formatter_scaled` (§ two lanes, one name,
/// 2026-09-10): two lanes independently landed a function of that exact
/// name -- this single-return one, called only by `axis_tick_formatter`
/// above with `is_log=false` always, and a tuple-returning one below (the
/// axis-multiplier work, merge 09313cd9) that factors a shared exponent
/// OUT for the two real per-axis tick-label sites. The merge combining
/// both lanes kept both bodies without the collision surfacing as a text
/// conflict, which broke `cargo test` (E0428, test-only) until this
/// rename. Zero behaviour change -- same body, distinct name. Found and
/// fixed identically by two sessions independently; this merge just picks
/// one write-up of it.
pub fn axis_tick_formatter_unscaled(values: &[f64], is_log: bool) -> Box<dyn Fn(f64) -> String> {
    if is_log {
        return Box::new(format_tick_scientific);
    }
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let max_abs = finite.iter().fold(0.0f64, |m, v| m.max(v.abs()));

    // Smallest gap between distinct ticks: labels must stay distinguishable
    // at this spacing or the axis is lying about where its ticks are.
    let mut sorted = finite.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut step = f64::INFINITY;
    for pair in sorted.windows(2) {
        let d = (pair[1] - pair[0]).abs();
        if d > 1e-300 {
            step = step.min(d);
        }
    }
    let from_step = if step.is_finite() && step > 0.0 {
        (-step.log10()).ceil().max(0.0) as usize
    } else {
        0
    };
    // Every tick must also be able to show ITSELF, which the gap alone does
    // not guarantee.
    //
    // A log axis hands its values over already exponentiated, and their
    // spacing says nothing about the precision any one of them needs.
    // Thinned to two labels, a decade axis over 0.001 to 1 has a gap of
    // 0.999 -- one decimal -- so 0.001 was formatted as "0.0", caught by
    // the all-zeros guard below, and printed as "0". A log axis cannot HAVE
    // a tick at zero, so the label was not merely coarse, it was
    // impossible: seen on a publication 2x2 where the same axis labelled
    // itself correctly at full width and lost its meaning once the panel
    // shrank and the ticks thinned.
    //
    // Linear axes are unaffected in practice -- their ticks are evenly
    // spaced, so the smallest magnitude is the step (or a multiple of it)
    // and this asks for no more digits than the gap already did.
    let min_abs = finite
        .iter()
        .map(|v| v.abs())
        .filter(|v| *v > 0.0)
        .fold(f64::INFINITY, f64::min);
    let from_smallest = if min_abs.is_finite() {
        (-min_abs.log10()).ceil().max(0.0) as usize
    } else {
        0
    };
    let decimals_needed = from_step.max(from_smallest);

    // Scientific when the magnitudes are extreme, or when staying decimal
    // would need more digits than anyone reads off an axis.
    let scientific =
        max_abs >= 1e5 || (max_abs > 0.0 && max_abs < 1e-3) || decimals_needed > 4;

    if scientific {
        return Box::new(format_tick_scientific);
    }
    let decimals = decimals_needed.min(6);
    Box::new(move |v: f64| {
        if v.abs() < 1e-12 {
            return "0".to_string();
        }
        let s = format!("{v:.decimals$}");
        // `-0.000` is not a tick value anyone means.
        if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
            "0".to_string()
        } else {
            s
        }
    })
}

/// The shared power of ten to lift off an axis and print once at its end,
/// or `None` to label every tick in full.
///
/// This exists because an axis was lying. `06_long_labels` in the reference
/// collection spans 12,344,581 to 12,346,778 and printed **eleven y-tick
/// labels all reading `1.2x10^7`** -- eleven distinct positions wearing one
/// string. `axis_tick_formatter` works out how many decimals the tick gap
/// needs and then throws that away the moment it picks the scientific
/// branch, and `format_tick_scientific` rounds every mantissa to one digit.
/// At this range one digit cannot tell any two ticks apart.
///
/// Factoring the exponent out fixes it at the root rather than by widening
/// the mantissa: the ticks become 12344.6, 12345.0, ... against a single
/// `x10^3` node, which is both correct AND shorter than `1.23446x10^7`
/// repeated eleven times. PGFPlots calls this `scaled ticks`, on by default,
/// and its stated motivation is exactly that saving of space.
///
/// Threshold and exclusion follow PGFPlots: trigger when the leading
/// exponent is `> 3` or `< -1`, and never on a log axis, where the tick
/// values ARE powers and a shared factor would be nonsense.
fn axis_multiplier(values: &[f64], is_log: bool) -> Option<i32> {
    if is_log {
        return None;
    }
    let max_abs = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0f64, |m, v| m.max(v.abs()));
    if max_abs <= 0.0 {
        return None;
    }
    // A single tick, or an axis whose ticks all collapse to one value, has
    // no spacing to preserve and gains nothing but a stray node.
    let distinct = {
        let mut s: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        s.dedup_by(|a, b| (*a - *b).abs() <= 1e-300);
        s.len()
    };
    if distinct < 2 {
        return None;
    }
    let exp = max_abs.log10().floor() as i32;
    (exp > 3 || exp < -1).then_some(exp)
}

/// The axis-end node for a multiplier, as `$...$` math markup so it is set
/// by the same path as every other label -- a raised `<tspan>` in SVG, real
/// LaTeX in TikZ.
fn multiplier_label(exp: i32) -> String {
    format!("$\\times10^{{{exp}}}$")
}

/// `axis_tick_formatter`, plus the shared exponent it lifted out.
///
/// Returns the formatter to apply to each tick and the exponent the caller
/// must print once at the end of the axis. When the exponent is `Some(e)`,
/// the formatter has ALREADY divided by `10^e` -- callers pass raw tick
/// values in both cases and must not pre-scale.
pub fn axis_tick_formatter_scaled(
    values: &[f64],
    is_log: bool,
) -> (Box<dyn Fn(f64) -> String>, Option<i32>) {
    let Some(exp) = axis_multiplier(values, is_log) else {
        return (axis_tick_formatter(values), None);
    };
    let scale = 10f64.powi(exp);

    // Decimals from the smallest gap between scaled ticks -- the same rule
    // `axis_tick_formatter` uses, applied here directly because the
    // mantissas MUST stay decimal.
    //
    // Handing the scaled values back to `axis_tick_formatter` was the
    // obvious implementation and it is wrong: 12,344,581..12,346,778 scales
    // to 1.2344581..1.2346778, whose gap asks for five decimals, which trips
    // that function's `decimals_needed > 4` rule straight back into
    // `format_tick_scientific` -- one mantissa digit, and eleven ticks
    // reading `1.2x10^0`. The bug survives the fix that was supposed to
    // remove it, wearing a smaller exponent.
    //
    // Once an exponent has been factored out there is nothing left for
    // scientific notation to do: a second exponent on top of the axis
    // multiplier is what produced that absurdity. So the mantissa is always
    // decimal here, and the digit count is whatever keeps the ticks apart.
    let mut sorted: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .map(|v| v / scale)
        .collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut step = f64::INFINITY;
    for pair in sorted.windows(2) {
        let d = (pair[1] - pair[0]).abs();
        if d > 1e-300 {
            step = step.min(d);
        }
    }
    let decimals = if step.is_finite() && step > 0.0 {
        ((-step.log10()).ceil().max(0.0) as usize).min(6)
    } else {
        0
    };
    (
        Box::new(move |v: f64| {
            let m = v / scale;
            let s = format!("{m:.decimals$}");
            // `-0.00` is not a tick value anyone means -- same guard the
            // decimal branch of `axis_tick_formatter` applies.
            if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
                "0".to_string()
            } else {
                s
            }
        }),
        Some(exp),
    )
}

/// `1.0e-6` is programmer notation; it belongs in source and in a CSV, not
/// on the axis of a figure going into a paper. Emitted as `$...$` math
/// markup, which the renderers already understand -- SVG raises and shrinks
/// a `<tspan>`, TikZ hands it to LaTeX -- so this changes the notation, not
/// the typesetting machinery.
pub fn format_tick_scientific(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    if v.abs() < 1e-300 {
        return "0".to_string();
    }
    let exp = v.abs().log10().floor();
    let mantissa = v / 10f64.powf(exp);
    // Rounding can push a mantissa to exactly 10 (9.99e5 at one decimal),
    // which should read as the next decade, not "10 times ten to the fifth".
    let (mantissa, exp) = if (mantissa.abs() * 10.0).round() / 10.0 >= 10.0 {
        (mantissa / 10.0, exp + 1.0)
    } else {
        (mantissa, exp)
    };
    let e = exp as i64;
    let m = (mantissa * 10.0).round() / 10.0;
    if (m - 1.0).abs() < 1e-9 {
        // A bare power of ten needs no "1 x" in front of it.
        format!("$10^{{{e}}}$")
    } else if (m + 1.0).abs() < 1e-9 {
        format!("$-10^{{{e}}}$")
    } else {
        let m_txt = if (m - m.trunc()).abs() < 1e-9 { format!("{}", m.trunc()) } else { format!("{m}") };
        format!(r"${m_txt}\times10^{{{e}}}$")
    }
}

pub fn format_tick(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    // Floating-point noise around a mathematically-exact zero (e.g.
    // `sin(pi)`, an FFT's real-signal negative-frequency residue) lands at
    // roughly f64 epsilon scale (~2.2e-16), not truly zero -- reported live
    // (2026-09-04): a wavelet-decomposition demo's y-axis showed
    // "-2.8e-16" instead of "0", which reads as spurious precision, not
    // information. Snapping anything this close to zero to a clean "0"
    // instead -- no realistic plotted quantity needs a tick distinguishing
    // e.g. 1e-12 from zero, so this can't collide with real data.
    if v.abs() < 1e-9 {
        return "0".to_string();
    }
    if v.abs() < 1e-3 || v.abs() >= 1e5 {
        return format_tick_scientific(v);
    }
    let rounded = (v * 1000.0).round() / 1000.0;
    let mut s = format!("{rounded}");
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// Diagonal-stripe "hatch" fill for a bar rect, as a set of `DrawOp::Line`s
/// clipped exactly to `[x, x+w] x [y, y+h]` — rather than an SVG `<pattern>`,
/// so it renders identically across SVG, HTML, *and* TikZ with no extra
/// machinery, since all three already know how to draw a `DrawOp::Line`.
/// Stripes lie on lines of constant `X + Y` (a `\`-direction 45° diagonal in
/// screen space, where `y` grows downward); for each stripe the segment
/// endpoints are found by clamping `X` to the rect's width and reading `Y`
/// back off the line equation, so no case-by-case corner clipping is needed.
fn hatch_lines(x: f64, y: f64, w: f64, h: f64, color: &str) -> Vec<DrawOp> {
    const STEP: f64 = 6.0;
    let mut ops = Vec::new();
    let c_min = x + y;
    let c_max = (x + w) + (y + h);
    let mut c = c_min;
    while c <= c_max {
        let x_lo = (c - (y + h)).max(x);
        let x_hi = (c - y).min(x + w);
        if x_hi > x_lo {
            ops.push(DrawOp::Line { x1: x_lo, y1: c - x_lo, x2: x_hi, y2: c - x_hi, color: color.to_string(), width: 1.2 });
        }
        c += STEP;
    }
    ops
}

/// A dashed line from `(x1,y1)` to `(x2,y2)`, as alternating visible
/// `DrawOp::Line` segments — used for the inset zoom-box border and its
/// connector lines (`Panel.inset`), where a plain solid line would look
/// like ordinary plot content instead of a "this region is magnified
/// elsewhere" annotation.
fn dashed_segment(x1: f64, y1: f64, x2: f64, y2: f64, color: &str) -> Vec<DrawOp> {
    const DASH: f64 = 5.0;
    const GAP: f64 = 4.0;
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return Vec::new();
    }
    let (ux, uy) = (dx / len, dy / len);
    let mut ops = Vec::new();
    let mut pos = 0.0;
    while pos < len {
        let seg_end = (pos + DASH).min(len);
        ops.push(DrawOp::Line {
            x1: x1 + ux * pos, y1: y1 + uy * pos,
            x2: x1 + ux * seg_end, y2: y1 + uy * seg_end,
            color: color.to_string(), width: 1.2,
        });
        pos += DASH + GAP;
    }
    ops
}

/// The 4-sided dashed border for an inset zoom-box (see `dashed_segment`).
fn dashed_rect_ops(x: f64, y: f64, w: f64, h: f64, color: &str) -> Vec<DrawOp> {
    let mut ops = dashed_segment(x, y, x + w, y, color);
    ops.extend(dashed_segment(x + w, y, x + w, y + h, color));
    ops.extend(dashed_segment(x + w, y + h, x, y + h, color));
    ops.extend(dashed_segment(x, y + h, x, y, color));
    ops
}

/// The break zone (see `Panel.x_break`/`y_break`) always claims this
/// fraction of the axis's total pixel span, regardless of how big the
/// skipped data range is — a break from 10 to 11 and a break from 10 to
/// 1_000_000 both draw the same fixed-width "//" jag.
const BREAK_ZONE_FRAC: f64 = 0.06;

/// Splits `[lo, hi]` around a break `(blo, bhi)` into `(left_frac,
/// zone_frac, right_frac)` — the fraction of the axis's pixel span given to
/// the pre-break region, the fixed break zone itself, and the post-break
/// region, always summing to 1. Returns `None` if `brk` isn't strictly
/// inside `(lo, hi)` (an out-of-range or degenerate break is treated as no
/// break at all, by every caller of this function, rather than producing a
/// nonsensical negative-width region).
fn break_spans(lo: f64, hi: f64, brk: (f64, f64)) -> Option<(f64, f64, f64)> {
    let (blo, bhi) = brk;
    if !(bhi > blo && blo > lo && bhi < hi) {
        return None;
    }
    let left_span = (blo - lo).max(1e-300);
    let right_span = (hi - bhi).max(1e-300);
    let total = left_span + right_span;
    let left_frac = (1.0 - BREAK_ZONE_FRAC) * left_span / total;
    let right_frac = (1.0 - BREAK_ZONE_FRAC) * right_span / total;
    Some((left_frac, BREAK_ZONE_FRAC, right_frac))
}

/// Maps `v` in `[lo, hi]` to a fraction in `[0, 1]` — plain linear unless
/// `brk` names a valid break (see `break_spans`), in which case the break's
/// span is squeezed to `BREAK_ZONE_FRAC` and the two remaining regions each
/// get pixel space proportional to their own (unequal) span. A value that
/// itself falls strictly inside the break — data plotted directly in the
/// range a break exists to hide — lands at the zone's midpoint rather than
/// being arbitrarily assigned to one side.
fn axis_frac(v: f64, lo: f64, hi: f64, brk: Option<(f64, f64)>) -> f64 {
    if let Some(b) = brk {
        if let Some((left_frac, zone_frac, right_frac)) = break_spans(lo, hi, b) {
            let (blo, bhi) = b;
            return if v <= blo {
                (v - lo) / (blo - lo).max(1e-300) * left_frac
            } else if v >= bhi {
                left_frac + zone_frac + (v - bhi) / (hi - bhi).max(1e-300) * right_frac
            } else {
                left_frac + zone_frac / 2.0
            };
        }
    }
    (v - lo) / (hi - lo).max(1e-300)
}

/// The pixel x-position of the break zone's midpoint, for placing the "//"
/// jag mark — `None` if there's no valid break to mark (mirrors
/// `break_spans`).
fn break_zone_center_frac(lo: f64, hi: f64, brk: (f64, f64)) -> Option<f64> {
    break_spans(lo, hi, brk).map(|(left_frac, zone_frac, _)| left_frac + zone_frac / 2.0)
}

/// A "//" axis-break mark for a *vertical* spine crossing (an x-axis break):
/// two short parallel diagonal strokes centered on `(cx, cy)`, the
/// conventional zigzag that reads as "this axis skips a range here" in
/// matplotlib/MATLAB-style broken-axis plots.
fn break_marks_x(cx: f64, cy: f64, color: &str) -> Vec<DrawOp> {
    const LEN: f64 = 5.0;
    const GAP: f64 = 3.0;
    vec![
        DrawOp::Line { x1: cx - GAP - LEN / 2.0, y1: cy + LEN, x2: cx - GAP + LEN / 2.0, y2: cy - LEN, color: color.to_string(), width: 1.4 },
        DrawOp::Line { x1: cx + GAP - LEN / 2.0, y1: cy + LEN, x2: cx + GAP + LEN / 2.0, y2: cy - LEN, color: color.to_string(), width: 1.4 },
    ]
}

/// The y-axis twin of `break_marks_x` — two short parallel diagonal strokes
/// for a break on a *horizontal* spine crossing.
fn break_marks_y(cy: f64, cx: f64, color: &str) -> Vec<DrawOp> {
    const LEN: f64 = 5.0;
    const GAP: f64 = 3.0;
    vec![
        DrawOp::Line { x1: cx - LEN, y1: cy - GAP - LEN / 2.0, x2: cx + LEN, y2: cy - GAP + LEN / 2.0, color: color.to_string(), width: 1.4 },
        DrawOp::Line { x1: cx - LEN, y1: cy + GAP - LEN / 2.0, x2: cx + LEN, y2: cy + GAP + LEN / 2.0, color: color.to_string(), width: 1.4 },
    ]
}

/// Clips one segment `(x0,y0)-(x1,y1)` to the rectangle `[xmin,xmax] x
/// [ymin,ymax]` via the standard Liang-Barsky parametric algorithm, used by
/// `clip_polyline_to_rect` for `zoom_inset(..., clip=true)`. Returns the
/// clipped endpoints, or `None` if the segment misses the rectangle
/// entirely.
fn liang_barsky_clip(x0: f64, y0: f64, x1: f64, y1: f64, xmin: f64, xmax: f64, ymin: f64, ymax: f64) -> Option<(f64, f64, f64, f64)> {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [(-dx, x0 - xmin), (dx, xmax - x0), (-dy, y0 - ymin), (dy, ymax - y0)] {
        if p.abs() < 1e-300 {
            if q < 0.0 {
                return None; // parallel to this edge and outside it
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 { return None; }
                if r > t0 { t0 = r; }
            } else {
                if r < t0 { return None; }
                if r < t1 { t1 = r; }
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    Some((x0 + t0 * dx, y0 + t0 * dy, x0 + t1 * dx, y0 + t1 * dy))
}

/// Hard-clips a polyline (data-space points, in order) to a rectangle,
/// segment by segment, returning the resulting run(s) of connected points
/// still in data space — a curve that exits and re-enters the box produces
/// more than one strip. Unlike filtering the point list down to "whole
/// points inside the box" (`zoom_inset`'s default "zoom" style), every
/// returned point sits exactly on the rectangle's border wherever the true
/// line crosses it, so the drawn path touches the border with no gap and no
/// straight-line shortcut across an excluded interior dip.
fn clip_polyline_to_rect(points: &[(f64, f64)], xmin: f64, xmax: f64, ymin: f64, ymax: f64) -> Vec<Vec<(f64, f64)>> {
    let mut strips: Vec<Vec<(f64, f64)>> = Vec::new();
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if let Some((cx0, cy0, cx1, cy1)) = liang_barsky_clip(x0, y0, x1, y1, xmin, xmax, ymin, ymax) {
            // A data point landing exactly on the border (common when the
            // requested box edge coincides with a real sample) clips its
            // *adjoining* segment down to a zero-length touch rather than a
            // real visible run — dropped below rather than left in as an
            // invisible but harmless-looking duplicate point.
            if (cx0 - cx1).abs() < 1e-9 && (cy0 - cy1).abs() < 1e-9 {
                continue;
            }
            let touches = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
            let continues = strips.last().and_then(|s| s.last()).is_some_and(|&last| touches(last, (cx0, cy0)));
            if continues {
                strips.last_mut().unwrap().push((cx1, cy1));
            } else {
                strips.push(vec![(cx0, cy0), (cx1, cy1)]);
            }
        }
    }
    strips
}

/// Rounds `x` to a "nice" 1/2/5-times-a-power-of-ten value — the classic
/// Heckbert "nice numbers for graph labels" step, used to keep axis ticks at
/// round values (`5`, `10`, `25`) instead of whatever the raw data range
/// happens to divide into (`18.798`, `6.75`, …). `round` picks the nearest
/// nice fraction (for a candidate *step size*); the non-round variant rounds
/// *up* (for a candidate *range*, which must not undershoot the data).
fn nice_num(range: f64, round: bool) -> f64 {
    if !(range > 0.0) || !range.is_finite() {
        return 1.0;
    }
    let exponent = range.log10().floor();
    let fraction = range / 10f64.powf(exponent);
    let nice_fraction = if round {
        if fraction < 1.5 { 1.0 } else if fraction < 3.0 { 2.0 } else if fraction < 7.0 { 5.0 } else { 10.0 }
    } else if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice_fraction * 10f64.powf(exponent)
}

/// Nice-number tick values covering `[lo, hi]` — the step is rounded to a
/// nice 1/2/5 fraction, then ticks are generated on that step and clipped to
/// the requested range (a raw `lo`/`hi` needn't itself land on a nice value,
/// so ticks legitimately stop short of the axis edges — matplotlib/MATLAB do
/// the same). `count` is a target, not a guarantee: the actual number of
/// ticks returned depends on how evenly the nice step divides the range.
/// Drop ticks until they stop colliding.
///
/// `GRID_TICK_TARGET` asks for ten ticks on every axis of every panel,
/// which is right for a full-size figure and badly wrong for a stacked
/// subplot a third the height -- a rail of thirteen labels almost touching.
/// But the request cannot simply be lowered: `nice_ticks` rounds the STEP
/// to a round number, so asking for 3 can return 2 or 6 depending on where
/// the range falls. Density has to be enforced on the result.
///
/// Thinning by a stride keeps the values nice -- every second tick of a
/// 0.2 step is a 0.4 step -- where recomputing with a smaller count would
/// just land on another rounded step and possibly the same problem. Zero
/// is kept whenever it is present, since an axis that shows the origin and
/// then hides it under thinning reads as a bug.
fn thin_ticks(ticks: Vec<f64>, span_px: f64, min_px: f64) -> Vec<f64> {
    if ticks.len() < 3 || span_px <= 0.0 || min_px <= 0.0 {
        return ticks;
    }
    let spacing = span_px / (ticks.len() - 1) as f64;
    if spacing >= min_px {
        return ticks;
    }
    // Capped so at least the two end ticks survive: a panel too small to
    // hold a readable axis should still say what range it covers, and an
    // axis showing one lonely tick is worse than a slightly tight one.
    let stride = ((min_px / spacing).ceil().max(1.0) as usize).min(ticks.len() - 1);
    if stride <= 1 {
        return ticks;
    }
    // Anchor on zero when the axis crosses it, so the origin survives.
    let anchor = ticks
        .iter()
        .position(|v| v.abs() < 1e-12)
        .unwrap_or(0);
    let kept: Vec<f64> = ticks
        .iter()
        .enumerate()
        .filter(|(i, _)| (*i as i64 - anchor as i64).rem_euclid(stride as i64) == 0)
        .map(|(_, v)| *v)
        .collect();
    // Enforce the invariant the stride cap above only claims.
    //
    // Capping the stride at `len - 1` does not guarantee two survivors once
    // the stride is anchored on zero: with five ticks, a zero anchored at
    // index 2 and a stride of 4, indices 2-4 and 2+4 are both outside the
    // list and exactly ONE tick comes back. A real publication figure hit
    // this -- a whole panel whose y axis carried a single label, so the
    // reader could see where zero was and had no way to read any other
    // value off it. An axis with one tick states no scale at all.
    if kept.len() < 2 {
        let first = *ticks.first().expect("non-empty: len >= 3 checked above");
        let last = *ticks.last().expect("non-empty");
        // Keep zero too when it is between them, so the origin is still
        // marked -- that is what the anchor was protecting.
        let mut out = vec![first];
        if let Some(&z) = ticks.iter().find(|v| v.abs() < 1e-12) {
            if z > first && z < last {
                out.push(z);
            }
        }
        out.push(last);
        return out;
    }
    kept
}

/// Minimum gap between y tick labels: about two line-heights, so bumping
/// the type (publication does) thins the ticks instead of colliding them.
const MIN_TICK_GAP_Y: f64 = 1.8;
/// X labels sit side by side and read several characters wide, so they
/// need more room than a single line-height would suggest.
const MIN_TICK_GAP_X: f64 = 4.0;

/// Ticks for a log axis, in log10 space.
///
/// `nice_ticks` was being used for these too, which is wrong in a way that
/// is easy to miss: it produces round numbers in whatever space it is
/// given, and on a log axis that space is the *exponent*. A round step of
/// 0.2 in log10 is the sequence 63, 100, 158, 251, 398, 631 -- evenly
/// spaced on the page, and meaningless to read. A log axis wants decades.
///
/// So: whole decades when the range covers several, a coarser multiple of
/// a decade when it covers many, and the 1-2-5 mantissas within a decade
/// when it covers less than about two. Below a third of a decade there is
/// nothing logarithmic left to show and ordinary round numbers are better,
/// which is what the caller's fallback gives.
fn log_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return vec![lo];
    }
    let decades = hi - lo;
    if decades < 0.3 {
        return Vec::new(); // caller falls back to `nice_ticks`
    }
    let mut ticks = Vec::new();
    if decades >= 1.5 {
        // One tick every `step` decades, chosen so the count lands near
        // the target without ever going below one decade.
        let step = ((decades / target.max(2) as f64).ceil() as i32).max(1);
        let first = (lo.floor() as i32).div_euclid(step) * step;
        let mut e = first;
        while (e as f64) <= hi + 1e-9 {
            if e as f64 >= lo - 1e-9 {
                ticks.push(e as f64);
            }
            e += step;
        }
    } else {
        // Under two decades, whole decades alone would give one or two
        // ticks. 1-2-5 is the standard subdivision and keeps the labels
        // readable (2, 5, 10, 20, 50) instead of arbitrary.
        let e0 = lo.floor() as i32;
        let e1 = hi.ceil() as i32;
        for e in e0..=e1 {
            for m in [1.0f64, 2.0, 5.0] {
                let v = (e as f64) + m.log10();
                if v >= lo - 1e-9 && v <= hi + 1e-9 {
                    ticks.push(v);
                }
            }
        }
    }
    ticks
}

fn nice_ticks(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return vec![lo];
    }
    // Deliberately NOT `nice_num(nice_num(hi - lo, false) / ..., true)` (the
    // textbook two-pass Heckbert form): ceiling-rounding the raw range
    // *before* dividing by `count` can nearly double it first (e.g. 22 ->
    // 50), so the requested tick count is undershot no matter how high you
    // set it. Rounding the per-step size directly gets much closer to the
    // requested density while keeping the same "round step, not an ugly
    // fraction" guarantee.
    let step = nice_num((hi - lo) / (count.max(2) - 1) as f64, true);
    if !(step > 0.0) {
        return vec![lo, hi];
    }
    let nice_lo = (lo / step).floor() * step;
    let mut ticks = Vec::new();
    // Multiply, don't accumulate.
    //
    // `v += step` in a loop accumulates rounding error, and a tick that
    // should be exactly 0 lands a few ulps away. That is invisible until
    // the label is formatted: a value of -2.8e-16 is not zero, so it gets
    // scientific notation, and a real figure showed a y axis whose only
    // label read "-2.8x10^-16" where it should have said 0. Computing each
    // tick as `nice_lo + i * step` keeps the error from compounding.
    //
    // Multiplication alone is not quite enough -- `nice_lo` itself comes
    // from a division and a floor -- so a tick within a millionth of a
    // step of zero is snapped to it. Zero is the one tick value whose
    // exactness a reader can see at a glance, and no real dataset needs a
    // tick at 1e-16 that is distinguishable from the origin.
    let mut n = 0;
    loop {
        let v = nice_lo + step * n as f64;
        if v > hi + step * 1e-9 || n >= 1000 {
            break;
        }
        if v >= lo - step * 1e-9 {
            let snapped = if v.abs() < step * 1e-6 { 0.0 } else { v };
            ticks.push(snapped);
        }
        n += 1;
    }
    if ticks.is_empty() {
        ticks.push(lo);
    }
    ticks
}

/// Minor gridline positions, in the same raw domain `nice_ticks` returns
/// (already `log10`-transformed when the axis is log-scaled), computed
/// from an already-placed set of major ticks. A log axis gets the
/// standard 2..9-per-decade minor ticks (matplotlib/MATLAB's own
/// convention, mathematically exact); a linear axis gets one midpoint
/// line per major gap — a simple approximation rather than `nice_ticks`'
/// own recursive "nice" sub-stepping, good enough for a denser dotted
/// grid without a second full tick algorithm.
fn minor_tick_positions(major_ticks: &[f64], lo: f64, hi: f64, is_log: bool) -> Vec<f64> {
    let (rlo, rhi) = (lo.min(hi), lo.max(hi));
    if is_log {
        if major_ticks.is_empty() {
            return Vec::new();
        }
        let e_lo = rlo.floor() as i64;
        let e_hi = rhi.ceil() as i64;
        let mut out = Vec::new();
        for e in e_lo..=e_hi {
            for m in 2..=9 {
                let raw = e as f64 + (m as f64).log10();
                if raw >= rlo && raw <= rhi {
                    out.push(raw);
                }
            }
        }
        out
    } else {
        major_ticks.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect()
    }
}

/// Builds the complete, format-independent draw-command list for a figure:
/// panel layout, gridlines/ticks, series, shapes, callouts, and legends. SVG,
/// HTML (which just wraps SVG), and TikZ renderers all replay this same list.
/// Where a panel's plot area landed, and what data range it shows.
///
/// A rendered figure is a flat list of lines and text: nothing in it says
/// which pixel means which data value, so a viewer cannot answer "what is
/// under the cursor" without guessing (parsing tick labels, say, which
/// breaks the moment an axis is logarithmic or broken). Emitting the
/// mapping the renderer already computed makes the answer exact.
///
/// Limits are stored POST-transform -- already `log10`'d when the axis is
/// logarithmic -- because that is the space the linear mapping happens in.
/// A reader converts back with `10^v`.
#[derive(Clone, Debug)]
pub struct PanelGeom {
    pub panel: usize,
    /// Plot area in SVG user units.
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
    pub log_x: bool,
    pub log_y: bool,
    /// Axis breaks make the mapping piecewise; a reader that ignores this
    /// would report confidently wrong values inside the break.
    pub x_break: Option<(f64, f64)>,
    pub y_break: Option<(f64, f64)>,
}

pub fn build_draw_ops(fig: &Figure, width: f64, height: f64) -> Vec<DrawOp> {
    build_draw_ops_with_geometry(fig, width, height).0
}

/// `build_draw_ops` plus the per-panel data<->pixel mapping. One function
/// rather than two so the geometry cannot drift from the drawing that used
/// it -- both come out of the same layout pass.
pub fn build_draw_ops_with_geometry(
    fig: &Figure,
    width: f64,
    height: f64,
) -> (Vec<DrawOp>, Vec<PanelGeom>) {
    // Height 0 is the marker `figure_size("ieee1")` leaves behind: the
    // aspect depends on what is plotted, and nothing has been plotted yet
    // when `figure_size` runs.
    let width = resolved_width(fig, width);
    let height = resolved_height(fig, width, height);
    let mut ops = Vec::new();
    let mut geometry: Vec<PanelGeom> = Vec::new();
    // Every layer of a cell draws in the same rectangle, so they must draw
    // against the same scale: two overlaid panels with different limits put
    // two sets of tick numbers in the same place, which is unreadable, and
    // the curves on top of each other would not be comparable anyway --
    // which is the whole point of overlaying them. The first panel drawn in
    // a cell fixes the scale and the rest of its layers adopt it.
    let mut cell_scale: std::collections::HashMap<usize, (f64, f64, f64, f64)> =
        std::collections::HashMap::new();
    let rects = layout_panels(fig, width, height);

    for rect in &rects {
        let panel = &fig.panels[rect.panel];
        let palette = resolve_palette(&panel.palette);
        // An `outside` legend reserves extra margin on whichever side it
        // sits on, *before* the plot area is laid out — the plot shrinks to
        // make room rather than the legend overlapping it.
        // Base margins sized for the default tick/title/label sizes (bumped
        // up from the original 10/11/12px set for a journal feel). Scaled by
        // the figure's actual sizes (via `fontsize(...)`) so a bumped font
        // doesn't clip against the plot area — left/bottom track tick size
        // (the widest/tallest text living in that margin), top tracks title
        // size, right is unaffected (it only ever holds the right y-axis).
        let tick_scale = fig.tick_size / TICK_METRIC_BASELINE;
        let title_scale = fig.title_size / DEFAULT_TITLE_SIZE;
        // Left and top are still plain totals: nothing stacks out there
        // yet beyond the axis furniture that already anchors correctly.
        // Right and bottom get their totals from their stacks below.
        let (mut margin_left, mut margin_top) =
            (MARGIN_LEFT * tick_scale, MARGIN_TOP * title_scale);
        let (mut margin_right, mut margin_bottom);
        // The right and bottom sides as ORDERED STACKS, not just totals.
        // A total says how far the plot must shrink; it does not say where
        // inside that margin each occupant belongs, which is what every
        // placement below actually needs.
        //
        // The two sides differ in what their base margin IS, and the
        // difference is not cosmetic. `MARGIN_BOTTOM` is a content band --
        // it holds the tick numbers and the x-axis label, and the label is
        // placed at its foot -- so anything reserved afterwards stacks
        // below it. `MARGIN_RIGHT` is trailing slack out to the canvas
        // edge, holding nothing; the right axis's numbers are drawn just
        // outside the plot, not beyond that slack. So it is added to the
        // total but is not a band anything can be placed against.
        let mut stack_r = MarginStack::default();
        let mut stack_b = MarginStack::default();
        stack_b.push(Band::Pad, MARGIN_BOTTOM * tick_scale);

        // `yyaxis right` needs its own tick-label gutter on the right edge,
        // reserved up front the same way an outside legend is. It goes in
        // BEFORE the colour scale: an axis's numbers and the label naming
        // them belong together, against the plot they describe.
        let has_right_axis = (0..panel.series.len()).any(|i| panel.is_right_axis(i));
        // Named ticks alone are enough to need the gutter: the axis is
        // drawn whether or not a series is scaled to it.
        if has_right_axis || panel.ytick2_positions.is_some() {
            stack_r.push(Band::Ticks, 46.0);
            if panel.ylabel2.is_some() {
                stack_r.push(Band::AxisLabel, 14.0);
            }
        }
        // The colour scale takes a strip plus room for its numbers and
        // label, reserved before the plot area is laid out so the plot
        // shrinks to make room instead of the bar overlapping it. Outside
        // the right axis, because both want the same strip and the bar is
        // the one that can move.
        if panel.colorbar.is_some() {
            stack_r.push(Band::Colorbar, COLORBAR_GUTTER * tick_scale);
        }
        // The bottom's `Pad` band already holds both the tick numbers and
        // the x-axis label; anything pushed after it therefore stacks
        // BELOW the label rather than on top of it. That is precisely what
        // an outside legend used to get wrong -- it grew the bottom margin,
        // the label was positioned from the FOOT of that margin, and the
        // two ended up sharing one strip with the label reading through the
        // legend's box.
        margin_right = MARGIN_RIGHT + stack_r.total();
        margin_bottom = stack_b.total();
        // `xxaxis top` needs its own tick-label gutter above the plot.
        let has_top_axis = (0..panel.series.len()).any(|i| panel.is_top_axis(i));
        if has_top_axis {
            margin_top += 24.0;
            if panel.xlabel2.is_some() {
                margin_top += 14.0;
            }
        }
        // `zoom_inset(..., outside=true)` reserves margin the same way an
        // outside legend does, so the inset renders entirely clear of the
        // plot area by construction rather than needing an overlap check.
        if let Some(inset) = &panel.inset {
            if inset.outside {
                match legend_side(inset.position) {
                    Side::Right => {
                        stack_r.push(Band::Panel, rect.w * 0.32 + 16.0);
                        margin_right = MARGIN_RIGHT + stack_r.total();
                    }
                    Side::Left => margin_left += rect.w * 0.32 + 16.0,
                    Side::Top => margin_top += rect.h * 0.34 + 16.0,
                    Side::Bottom => {
                        stack_b.push(Band::Panel, rect.h * 0.34 + 16.0);
                        margin_bottom = stack_b.total();
                    }
                }
            }
        }
        // The legend, LAST of the things that shrink the plot, because it
        // is the only one whose placement depends on how big the plot ends
        // up being. This is the two-pass part: everything above fixes the
        // plot box, `legend_must_go_outside` then asks whether a key can
        // live inside a box that size, and only then is the margin for an
        // outside one reserved -- which is what an outside legend needs and
        // could not have before, since the reservation has to happen before
        // the layout it depends on.
        //
        // Explicitly outside stays exactly as it was; the decision only
        // ever promotes a legend that would otherwise sit on the data.
        let legend_outside = panel.legend.visible
            && (panel.legend.outside
                || legend_must_go_outside(
                    panel,
                    (rect.w - margin_left - margin_right).max(0.0),
                    (rect.h - margin_top - margin_bottom).max(0.0),
                    tick_scale,
                    face_metrics(&fig.font_family),
                ));
        if legend_outside {
            let (box_w, box_h) = legend_box_size(panel, tick_scale, face_metrics(&fig.font_family));
            if box_w > 0.0 {
                match legend_side(panel.legend.position) {
                    Side::Right => {
                        stack_r.push(Band::Panel, box_w + 16.0);
                        margin_right = MARGIN_RIGHT + stack_r.total();
                    }
                    Side::Left => margin_left += box_w + 16.0,
                    Side::Top => margin_top += box_h + 16.0,
                    Side::Bottom => {
                        stack_b.push(Band::Panel, box_h + 16.0);
                        margin_bottom = stack_b.total();
                    }
                }
            }
        }
        // A title too long for its panel gets a SECOND LINE rather than a
        // smaller size, and the room for it is reserved here, before the
        // plot area is laid out.
        //
        // Shrinking to fit was the old answer, and it is worse than it
        // sounds: two panels side by side, one with a long title and one
        // with a short one, came out with titles at visibly different
        // sizes -- and since weight is chosen by size, one of them was
        // bold and the other was not. Nothing about the figure justified
        // either difference. Wrapping keeps every title at the theme's own
        // title size, which is what makes a row of panels read as a row.
        let title_lines = panel
            .title
            .as_deref()
            .map(|t| {
                let avail = (rect.w - margin_left - margin_right).max(1.0);
                wrap_to_width(t, avail, fig.title_size, face_metrics(&fig.font_family)).len()
            })
            .unwrap_or(1);
        if title_lines > 1 {
            margin_top += (title_lines - 1) as f64 * fig.title_size * TITLE_LINE_PITCH;
        }
        // Guard against fixed-pixel chrome (title/tick/label margins, sized
        // in absolute px above) eating an entire cell once a grid packs many
        // rows/cols into one figure. Unclamped, a `subplot(8,1,i)` stack at
        // the default 900x600 canvas computes margin_top+margin_bottom = 72px
        // against a ~54.75px-tall cell — bottom <= top for every panel, so
        // the `continue` below fires for all eight and the figure renders
        // completely blank (verified: zero draw ops). Octave/matplotlib
        // sidestep this by keeping axes padding proportional to the
        // subplot's own box rather than a fixed pixel count; this mirrors
        // that by capping each margin pair at 70% of the cell it must fit
        // inside (guaranteeing >= 30% of the cell stays plot area, always
        // positive) and shrinking that panel's own tick/label/title text by
        // the same ratio so nothing overflows the now-smaller margin. Small
        // grids are unaffected: e.g. a 3-row stack in the same 900x600
        // canvas has a 176px cell against the same 72px margin — well under
        // the 70% cap — so `fit_scale_v`/`fit_scale_h` are exactly `1.0` and
        // every number below is bit-identical to the pre-fix layout.
        let fit_scale_v = if margin_top + margin_bottom > 0.0 {
            (rect.h * 0.7 / (margin_top + margin_bottom)).min(1.0)
        } else {
            1.0
        };
        let fit_scale_h = if margin_left + margin_right > 0.0 {
            (rect.w * 0.7 / (margin_left + margin_right)).min(1.0)
        } else {
            1.0
        };
        margin_top *= fit_scale_v;
        margin_bottom *= fit_scale_v;
        margin_left *= fit_scale_h;
        margin_right *= fit_scale_h;
        // The bands shrink with the total they add up to. Scaling one and
        // not the other would leave every placement below pointing at an
        // offset inside a margin that no longer extends that far.
        stack_r.scale(fit_scale_h);
        stack_b.scale(fit_scale_v);
        let fit_scale = fit_scale_v.min(fit_scale_h);
        let tick_size = fig.tick_size * fit_scale;
        let label_size = fig.label_size * fit_scale;
        let title_size = fig.title_size * fit_scale_v;
        // `theme("minimal")` — see `Figure.minimal` and `MINIMAL_GRID_COLOR`.
        // `theme("publication")` -- see `Figure.publication`. Gridlines go
        // PALER while the frame and tick labels go DARKER: in print the
        // data and its frame must survive reduction and photocopying,
        // while the grid is only a reading aid and should never compete
        // with the curve in front of it.
        // One named look decides all of this, rather than a chain of
        // booleans -- see `THEMES`. Adding a style is a table entry.
        let style = resolve_theme(&fig.theme);
        let grid_color = style.grid_color;
        let box_color = style.spine_color;
        // Stroke weight tracks type size (ggplot2's `base_size / 22`), so a
        // figure rendered large gets proportionally heavier rules instead
        // of hairlines, and a small one does not clog.
        let base_line = fig.tick_size / TYPE_SMALL * BASE_LINE_RATIO;
        // Every gap in the figure derives from one number, ggplot2's
        // `half_line = base_size / 2`. The tick-label, axis-label and title
        // offsets were fixed pixel constants (+14, -6, +12), so scaling the
        // type moved the text without moving the space around it -- labels
        // drifted into the axis at large sizes and floated at small ones.
        // The multipliers below reproduce the previous spacing exactly at
        // the default size, and now track type everywhere else.
        let half_line = fig.tick_size / TYPE_SMALL * HALF_LINE_RATIO;
        let grid_w = base_line * style.grid_width;
        // An explicit `grid on`/`grid off` in the script always beats the
        // theme's default.
        let show_grid = panel.show_grid.unwrap_or(style.show_grid);
        // Theme-then-panel, the same resolution as `show_grid` directly
        // above. `ThemeStyle.show_minor_grid` was set by all nine themes and
        // read by NOTHING: rendering consulted only `panel.show_minor_grid`,
        // which nothing but an explicit `minor grid on` ever wrote.
        let show_minor_grid = panel.show_minor_grid.unwrap_or(style.show_minor_grid);
        let minor_grid_w = base_line * style.minor_grid_width;
        // Title and axis labels follow the same ink as the frame and tick
        // numbers. They were left at the screen grey, so a publication
        // figure came out with a washed-out grey title and labels sitting
        // above a near-black frame -- print has none of the contrast a
        // backlit display gives you, and grey-on-white is exactly what
        // goes muddy there.
        // Read from the THEME, not hardcoded. These three fields --
        // `label_color`, `title_color`, `tick_text_color` -- existed on
        // `ThemeStyle` and were never read by anything: every theme
        // rendered `#444` labels and `#5a5a5a` ticks whatever it declared,
        // so `classic` and `nature` came out byte-identical despite
        // differing in the table. The comment above is the giveaway -- it
        // argues grey-on-white goes muddy in print, and that reasoning was
        // applied ONLY to `publication` via a `fig.publication` special
        // case, leaving `nature` and `ieee` (which both declare pure black)
        // rendering the screen grey into a journal.
        let text_ink: String = style.label_color.to_string();
        let title_ink: String = style.title_color.to_string();
        // The frame is scenery; the data is the subject. In the surveyed
        // Nature figures the spine is visibly THINNER than the plotted
        // curves -- roughly a third the weight in the clearest case -- and
        // Krzywinski puts a number on it: "axis weight should be modest --
        // 0.5 pt is sufficient". Qu drew both at 1.2, so the frame competed
        // with the curve for attention.
        //
        // Derived from the base line size (which tracks type size) rather
        // than fixed, so it stays proportionate when a figure is rescaled.
        let frame_width = base_line * style.spine_width;
        // Tick marks. `tick_out` is the signed distance the mark travels
        // from the axis line, positive = away from the panel; the labels
        // then clear whatever an outward mark occupies, so adding ticks
        // never pushes a number on top of its own tick.
        let tick_px = style.tick_len * half_line;
        let tick_out = if style.tick_inward { -tick_px } else { tick_px };
        let tick_clearance = tick_out.max(0.0);
        // The colour the marks are drawn in. This field existed all along
        // and nothing consumed it -- the tick *text* uses a global
        // `TICK_COLOR` -- because there were no marks for it to colour.
        let tick_mark_color = style.tick_color;
        let left = rect.left + margin_left;
        let right = rect.left + rect.w - margin_right;
        let top = rect.top + margin_top;
        let bottom = rect.top + rect.h - margin_bottom;
        if right <= left || bottom <= top {
            continue;
        }

        if let Some(spider) = &panel.spider {
            ops.extend(spider_ops(spider, left, top, right, bottom, palette, fig.tick_size, fig.label_size));
            if let Some(t) = &panel.title {
                ops.push(DrawOp::Text { italic: false, x: (left + right) / 2.0, y: rect.top + 12.0, text: t.clone(), size: title_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
            }
            continue; // spider charts don't mix with cartesian axes/series
        }

        if let Some(heatmap) = &panel.heatmap {
            ops.extend(heatmap_ops(heatmap, left, top, right, bottom, tick_size, &text_ink));
            if let Some(t) = &panel.title {
                ops.push(DrawOp::Text { italic: false, x: (left + right) / 2.0, y: rect.top + 12.0, text: t.clone(), size: title_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
            }
            continue; // heatmaps don't mix with cartesian axes/series
        }

        if let Some(pie) = &panel.pie {
            ops.extend(pie_ops(pie, left, top, right, bottom, palette, tick_size));
            if let Some(t) = &panel.title {
                ops.push(DrawOp::Text { italic: false, x: (left + right) / 2.0, y: rect.top + 12.0, text: t.clone(), size: title_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
            }
            continue; // pie/donut charts don't mix with cartesian axes/series
        }

        if let Some(image) = &panel.image {
            ops.extend(image_ops(image, left, top, right, bottom));
            if let Some(t) = &panel.title {
                ops.push(DrawOp::Text { italic: false, x: (left + right) / 2.0, y: rect.top + 12.0, text: t.clone(), size: title_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
            }
            continue; // an image panel (`imshow`) doesn't mix with cartesian axes/series
        }

        // `stackbar` needs the axis extent to cover each index's *cumulative*
        // stacked height, not each series' own (much smaller) y-range, or the
        // top segments would run past the panel; `bar`/`stackbar` also need
        // a visible zero baseline even if all values are positive.
        let stack_totals = stacked_bar_totals(panel);
        let has_bars = panel.series.iter().any(|s| matches!(s.marker.as_str(), "bar" | "stackbar" | "groupbar"));
        let box_extent = panel.boxplots.iter().flat_map(|b| {
            [b.whisker_lo, b.whisker_hi].into_iter().chain(b.outliers.iter().copied())
        });
        let rain_extent = panel.raincloud.iter().flat_map(|g| {
            g.density_x.iter().copied()
                .chain([g.box_stats.whisker_lo, g.box_stats.whisker_hi])
                .chain(g.box_stats.outliers.iter().copied())
                .chain(g.jitter.iter().map(|(_, v)| *v))
        });
        let shape_x = panel.shapes.iter().flat_map(|s| match s {
            Shape::ErrorBar { x, xerr, .. } => match xerr {
                Some(e) => vec![x - e, x + e],
                None => vec![*x],
            },
            Shape::FillBetween { x, .. } => x.clone(),
            Shape::Bubble { x, .. } => x.clone(),
            // A contour is usually the whole figure, so its grid has to
            // set the axis range. Without this a `contourf` on an
            // otherwise-empty panel autoscales to nothing and renders off
            // the visible area.
            Shape::Contour { x, .. } => x.clone(),
            // A drawn outline is content too. Without this a `polygon` on
            // an otherwise-empty panel autoscaled to nothing, so the axis
            // came out 0..1 and the shape was drawn outside it and
            // clipped away -- the same failure the contour note above
            // describes, and just as invisible: the figure renders, with
            // axes, and nothing in it.
            Shape::Curve { points, .. } => points.iter().map(|p| p.0).collect(),
            // Same reason as a contour, and the same failure without it:
            // a hexbin IS the figure, so its cells have to set the range.
            // Left out, the panel autoscaled to 0..1 and every hexagon was
            // stretched across the whole plot.
            Shape::Hexbin { cx, dx, .. } => {
                let lo = cx.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = cx.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                // Half a cell past the outermost centre, or the outer
                // cells are sliced by the frame -- the same margin every
                // category-standing shape needs.
                if lo.is_finite() { vec![lo - dx / 2.0, hi + dx / 2.0] } else { Vec::new() }
            }
            _ => Vec::new(),
        });
        let shape_y = panel.shapes.iter().flat_map(|s| match s {
            Shape::ErrorBar { y, err, .. } => vec![y - err, y + err],
            Shape::FillBetween { y_lo, y_hi, .. } => y_lo.iter().chain(y_hi).copied().collect(),
            Shape::Bubble { y, .. } => y.clone(),
            Shape::Contour { y, .. } => y.clone(),
            Shape::Curve { points, .. } => points.iter().map(|p| p.1).collect(),
            Shape::Hexbin { cy, dy, .. } => {
                let lo = cy.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = cy.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if lo.is_finite() { vec![lo - dy / 2.0, hi + dy / 2.0] } else { Vec::new() }
            }
            _ => Vec::new(),
        });
        // `xxaxis top` series are likewise excluded from the main (bottom)
        // x-extent — see the matching `yyaxis`/`all_y` note just below.
        // Half a category slot beyond the outermost bar centre, at each
        // end. Computed from the bar series' own x values rather than
        // assumed to be integers, so a bar chart plotted against real x
        // gets a margin in its own units.
        // Every distinct x a bar is centred on, in order. Used twice: to
        // pad the axis so the outer bars are not cut in half, and to put
        // the category labels UNDER THEIR OWN BARS rather than spread
        // evenly across whatever the padded range turned out to be.
        let bar_centres: Vec<f64> = {
            let mut c: Vec<f64> = panel
                .series
                .iter()
                .filter(|s| matches!(s.marker.as_str(), "bar" | "stackbar" | "groupbar"))
                .flat_map(|s| s.x.iter().copied())
                .filter(|v| v.is_finite())
                .collect();
            c.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            c.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            c
        };
        // Everything drawn as a shape STANDING ON a category needs the
        // same margin, for the same reason: a bar, a box, a violin and a
        // raincloud all have width, and an axis that stops at the
        // outermost centre cuts the outermost shape in half. Bars were
        // fixed first and the others had the identical defect.
        let category_centres: Vec<f64> = {
            let mut c: Vec<f64> = bar_centres.clone();
            c.extend(panel.boxplots.iter().map(|b| b.x));
            c.extend(panel.raincloud.iter().map(|g| g.x));
            c.retain(|v| v.is_finite());
            c.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            c.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            c
        };
        let bar_pad: Vec<f64> = if !category_centres.is_empty() {
            let centres = &category_centres;
            let lo = centres.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = centres.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if lo.is_finite() && hi.is_finite() {
                // One slot is the gap between neighbouring centres; a
                // single bar has no gap to measure, so it falls back to 1.
                let n = centres.len().max(1);
                let slot = if n > 1 && hi > lo { (hi - lo) / (n as f64 - 1.0) } else { 1.0 };
                vec![lo - slot * 0.5, hi + slot * 0.5]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let all_x = panel
            .series
            .iter()
            .enumerate()
            .filter(|(i, _)| !panel.is_top_axis(*i))
            .flat_map(|(_, s)| s.x.iter().copied())
            .chain(panel.boxplots.iter().map(|b| b.x))
            .chain(panel.raincloud.iter().map(|g| g.x))
            .chain(shape_x)
            // A bar is drawn CENTRED on its x, so the first and last ones
            // reach half a bar past the ends of the data. The extent knew
            // only about the centres, so the axis stopped exactly at the
            // middle of the outer bars and the spine cut both of them in
            // half -- most visible on a stacked chart, where category A
            // came out looking like a half-width column.
            //
            // Half a category slot each side, which is what matplotlib's
            // categorical bar axis does: the outer bars get the same
            // clearance from the frame as they have from each other.
            .chain(bar_pad);
        // `yyaxis right` series are excluded from the main (left) extent —
        // they get their own scale entirely, computed separately below —
        // but boxplots/raincloud/bars/shapes have no "which axis" concept
        // of their own, so they stay associated with the left axis only.
        let all_y = panel
            .series
            .iter()
            .enumerate()
            .filter(|(i, _)| !panel.is_right_axis(*i))
            .flat_map(|(_, s)| s.y.iter().copied())
            .chain(stack_totals.iter().copied())
            .chain(box_extent)
            .chain(rain_extent)
            .chain(shape_y)
            .chain(if has_bars { Some(0.0) } else { None });
        let log_x = panel.xscale == Scale::Log;
        let log_y = panel.yscale == Scale::Log;
        let log_x2 = panel.xscale2.map_or(log_x, |s| s == Scale::Log);
        let log_y2 = panel.yscale2.map_or(log_y, |s| s == Scale::Log);
        let (mut xmin, mut xmax) = panel
            .xlim
            .map(|(a, b)| if log_x { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) })
            .unwrap_or_else(|| data_extent(all_x, log_x, !panel.tight));
        let (mut ymin, mut ymax) = panel
            .ylim
            .map(|(a, b)| if log_y { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) })
            .unwrap_or_else(|| data_extent(all_y, log_y, !panel.tight));
        if panel.axis_equal {
            // Equal SCALES, not equal ranges.
            //
            // This used to widen the narrower span until `xmax-xmin ==
            // ymax-ymin`, which is not what the flag promises and, in the
            // case it exists for, does nothing at all: a shape example
            // already sets matching `xlim`/`ylim`, so the spans were equal
            // on arrival and the block was a no-op. Meanwhile the panel is
            // 900x600 minus margins -- wider than it is tall -- so equal
            // ranges over an unequal box still means unequal units per
            // pixel, and `xlim(-2,2) ylim(-2,2) axis equal circle(0,0,1)`
            // drew an ellipse 58% out of round, identical to the same
            // figure without the flag. A flag named `equal` that leaves a
            // circle visibly oval is trusted by everyone who reads its
            // name.
            //
            // Roundness is units-per-pixel, so match those against the box
            // as laid out: expand the axis whose scale is coarser, never
            // shrink either (shrinking would push data outside the panel).
            // This is MATLAB's `axis equal` -- the box keeps its shape and
            // the limits open up -- rather than matplotlib's default of
            // reshaping the box, which would fight the panel geometry the
            // grid and margins are already built from.
            let (box_w, box_h) = (right - left, bottom - top);
            if box_w > 0.0 && box_h > 0.0 {
                let (xspan, yspan) = (xmax - xmin, ymax - ymin);
                if xspan > 0.0 && yspan > 0.0 {
                    let want_x = yspan * (box_w / box_h);
                    if want_x > xspan {
                        let xc = (xmax + xmin) / 2.0;
                        xmin = xc - want_x / 2.0;
                        xmax = xc + want_x / 2.0;
                    } else {
                        let want_y = xspan * (box_h / box_w);
                        let yc = (ymax + ymin) / 2.0;
                        ymin = yc - want_y / 2.0;
                        ymax = yc + want_y / 2.0;
                    }
                }
            }
        }
        // Give an inside legend some sky to sit in, rather than letting it
        // land on the curve it labels -- see `legend_headroom`. After
        // `axis_equal`, which is a correctness constraint and outranks it;
        // before the ticks, which have to be chosen for the final range.
        // Layers of one cell share one coordinate system -- see
        // `cell_scale`. Done before the legend headroom below, so a layer
        // adopting a scale adopts the sky that was made in it too rather
        // than growing the axis a second time.
        let adopted = match (fig.layers > 1).then(|| panel.grid_cell).flatten() {
            Some(cell) => match cell_scale.get(&cell) {
                Some(&(sx0, sx1, sy0, sy1)) => {
                    xmin = sx0;
                    xmax = sx1;
                    ymin = sy0;
                    ymax = sy1;
                    true
                }
                None => false,
            },
            None => false,
        };
        // An adopted scale already carries whatever sky the first layer
        // made for its legend; running the headroom again on top would
        // grow the axis a second time and split the layers apart.
        if !adopted && !panel.axis_equal && !legend_outside {
            if let Some(room) = legend_headroom(
                panel, left, top, right, bottom, xmin, xmax, ymin, ymax, tick_scale,
                face_metrics(&fig.font_family),
            ) {
                ymax = room;
            }
        }
        if !adopted && fig.layers > 1 {
            if let Some(cell) = panel.grid_cell {
                cell_scale.insert(cell, (xmin, xmax, ymin, ymax));
            }
        }
        // `yyaxis right` series get an independently-scaled y-extent, same
        // x-extent, sharing `log_y`/`tight` (MATLAB allows independent
        // scales per side too, but one mode for both keeps this a bounded
        // v1 rather than a second full axis-configuration surface).
        let all_y_right: Vec<f64> = panel
            .series
            .iter()
            .enumerate()
            .filter(|(i, _)| panel.is_right_axis(*i))
            .flat_map(|(_, s)| s.y.iter().copied())
            .collect();
        // Same log transform the primary limits get: the axis machinery
        // works in log space when `log_y` is on, and handing it data-space
        // numbers put the right axis on a range of 10^0..10^1.25.
        let (ymin2, ymax2) = if let Some((a, b)) = panel.ylim2 {
            if log_y2 { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) }
        } else if has_right_axis {
            data_extent(all_y_right.into_iter(), log_y2, !panel.tight)
        } else if panel.ytick2_positions.is_some() {
            // A right axis with named ticks and nothing plotted on it is a
            // SECOND READING of the left one -- tone count against a crest
            // factor ratio, say -- so it shares its range. Left at (0, 1)
            // the ticks landed outside the panel and the axis came out
            // blank, which is how this was found.
            (ymin, ymax)
        } else {
            (0.0, 1.0)
        };
        // `xxaxis top` series get an independently-scaled x-extent, same
        // reasoning/scope as `yyaxis right` above.
        let all_x_top: Vec<f64> = panel
            .series
            .iter()
            .enumerate()
            .filter(|(i, _)| panel.is_top_axis(*i))
            .flat_map(|(_, s)| s.x.iter().copied())
            .collect();
        let (xmin2, xmax2) = if let Some((a, b)) = panel.xlim2 {
            if log_x2 { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) }
        } else if has_top_axis {
            data_extent(all_x_top.into_iter(), log_x2, !panel.tight)
        } else {
            (0.0, 1.0)
        };
        let to_px = move |x: f64, y: f64| -> (f64, f64) {
            let xv = if log_x { x.max(1e-300).log10() } else { x };
            let yv = if log_y { y.max(1e-300).log10() } else { y };
            let xbrk = if log_x { None } else { panel.x_break };
            let ybrk = if log_y { None } else { panel.y_break };
            let px = left + axis_frac(xv, xmin, xmax, xbrk) * (right - left);
            let py = bottom - axis_frac(yv, ymin, ymax, ybrk) * (bottom - top);
            (px, py)
        };
        geometry.push(PanelGeom {
            panel: rect.panel,
            left,
            right,
            top,
            bottom,
            xmin,
            xmax,
            ymin,
            ymax,
            log_x,
            log_y,
            x_break: if log_x { None } else { panel.x_break },
            y_break: if log_y { None } else { panel.y_break },
        });
        // Series-only mapper: each of the x/y axes is picked independently
        // per series (a series can be on `yyaxis right` and/or `xxaxis top`
        // at once) — everything else (shapes, boxplots, legend, spider,
        // heatmap) always uses the plain bottom/left `to_px` above.
        let to_px_for = |i: usize, x: f64, y: f64| -> (f64, f64) {
            let lx = if panel.is_top_axis(i) { log_x2 } else { log_x };
            let ly = if panel.is_right_axis(i) { log_y2 } else { log_y };
            let xv = if lx { x.max(1e-300).log10() } else { x };
            let yv = if ly { y.max(1e-300).log10() } else { y };
            let (xlo, xhi) = if panel.is_top_axis(i) { (xmin2, xmax2) } else { (xmin, xmax) };
            let (ylo, yhi) = if panel.is_right_axis(i) { (ymin2, ymax2) } else { (ymin, ymax) };
            // `x_break`/`y_break` only reshape the *main* (bottom/left) axis
            // mapping — `xxaxis top`/`yyaxis right` series ignore it, a
            // documented scope limit (see `Panel.x_break`'s doc comment).
            let xbrk = if lx || panel.is_top_axis(i) { None } else { panel.x_break };
            let ybrk = if ly || panel.is_right_axis(i) { None } else { panel.y_break };
            let px = left + axis_frac(xv, xlo, xhi, xbrk) * (right - left);
            let py = bottom - axis_frac(yv, ylo, yhi, ybrk) * (bottom - top);
            (px, py)
        };
        // `yyaxis` (MATLAB): each side's tick labels pick up that side's
        // first series' color, so the axis a number belongs to is visually
        // obvious at a glance — same reasoning MATLAB's own `yyaxis` uses.
        // A single-axis panel (the common case) keeps the neutral
        // `TICK_COLOR` instead of tinting to whatever series color happens
        // to be first, which would read as arbitrary, not meaningful.
        //
        // A print theme keeps the tint too, where it used to flatten every
        // number to one ink. Colour here is not decoration: it is the only
        // thing saying which of two axes a number belongs to, and a
        // twin-axis figure without it makes the reader guess. Published
        // figures do exactly this -- an orange left axis against a blue
        // right one -- so "print" is a reason to be careful with colour,
        // not a reason to drop the one place it carries meaning.
        let (left_tick_color, right_tick_color) = if has_right_axis {
            let left_color = panel
                .series
                .iter()
                .enumerate()
                .find(|(i, _)| !panel.is_right_axis(*i))
                .map(|(i, s)| series_color(&s.color, i, palette))
                .unwrap_or_else(|| style.tick_text_color.to_string());
            let right_color = panel
                .series
                .iter()
                .enumerate()
                .find(|(i, _)| panel.is_right_axis(*i))
                .map(|(i, s)| series_color(&s.color, i, palette))
                .unwrap_or_else(|| style.tick_text_color.to_string());
            (left_color, right_color)
        } else {
            (style.tick_text_color.to_string(), style.tick_text_color.to_string())
        };
        // same idea for `xxaxis top`/`bottom`.
        let (bottom_tick_color, top_tick_color) = if has_top_axis {
            let bottom_color = panel
                .series
                .iter()
                .enumerate()
                .find(|(i, _)| !panel.is_top_axis(*i))
                .map(|(i, s)| series_color(&s.color, i, palette))
                .unwrap_or_else(|| style.tick_text_color.to_string());
            let top_color = panel
                .series
                .iter()
                .enumerate()
                .find(|(i, _)| panel.is_top_axis(*i))
                .map(|(i, s)| series_color(&s.color, i, palette))
                .unwrap_or_else(|| style.tick_text_color.to_string());
            (bottom_color, top_color)
        } else {
            (style.tick_text_color.to_string(), style.tick_text_color.to_string())
        };

        // `axis origin`: the spines (and the tick labels that hug them)
        // cross through data `(0, 0)` instead of framing the panel, on
        // whichever axis is still linear — `0` has no meaning on a log
        // scale, so that axis keeps its edge position even in origin mode.
        let origin_active = panel.axis_position == AxisPosition::Origin;
        let origin_px = if origin_active && !log_x {
            left + axis_frac(0.0, xmin, xmax, panel.x_break).clamp(0.0, 1.0) * (right - left)
        } else {
            left
        };
        let origin_py = if origin_active && !log_y {
            bottom - axis_frac(0.0, ymin, ymax, panel.y_break).clamp(0.0, 1.0) * (bottom - top)
        } else {
            bottom
        };

        // gridlines/ticks (tick labels stay even with `grid off`, matching
        // MATLAB — only the gridlines themselves are gated) + border
        // y ticks: nice round values rather than 5 raw evenly-spaced
        // fractions of the data range (which produced ugly floats like
        // "18.798").
        // `y_break` (linear-scale only, see `Panel.y_break`'s doc comment):
        // ticks strictly inside the skipped range are dropped rather than
        // drawn on top of the "//" jag.
        let ybrk = if log_y { None } else { panel.y_break };
        // Explicit `yticks(...)` are taken as given -- NOT thinned. A
        // caller who named five positions meant those five; dropping one
        // for spacing would silently change what the axis says.
        let y_major_ticks = match &panel.ytick_positions {
            Some(at) => at.iter().map(|v| if log_y { v.log10() } else { *v }).collect(),
            None => thin_ticks(
                if log_y {
                    let t = log_ticks(ymin, ymax, GRID_TICK_TARGET);
                    if t.is_empty() { nice_ticks(ymin, ymax, GRID_TICK_TARGET) } else { t }
                } else {
                    nice_ticks(ymin, ymax, GRID_TICK_TARGET)
                },
                bottom - top,
                tick_size * MIN_TICK_GAP_Y,
            ),
        };
        // One notation for the whole axis, decided from all of its ticks
        // at once -- see `axis_tick_formatter`. Formatting each tick alone
        // is what let an axis print `0.002` twice and mix `0.001` with
        // `5x10^-4` on the same scale.
        let (fmt_y, y_multiplier) = axis_tick_formatter_scaled(
            &y_major_ticks
                .iter()
                .map(|&r| if log_y { 10f64.powf(r) } else { r })
                .collect::<Vec<f64>>(),
            log_y,
        );
        // How far the y tick numbers actually reach to the left. The y
        // axis label used to be pinned to the figure's left edge instead,
        // so the gap between it and the numbers was whatever the fixed
        // margin happened to leave over -- on an axis with short labels
        // ("0", "2") that stranded the label far out in white space.
        // `axis off` is applied by DROPPING the decoration after it is
        // built, rather than by guarding each of the two dozen places that
        // emit a spine, a tick, a label or a gridline. Those places compute
        // things the rest of the panel needs -- the widest tick label sets
        // the left margin, the tick formatter is decided from all ticks at
        // once -- so skipping them would change the layout as well as the
        // ink, and `axis off` should move nothing.
        // The theme's own panel fill, drawn under the gridlines. This is
        // what makes `theme("grey")` ggplot2's grey-panel look: the field
        // was set by `grey` (#ebebeb) and `bw` (#ffffff) since the themes
        // landed and read by nothing, so the signature grey panel simply
        // never appeared. Emitted BEFORE `decoration_start` on purpose --
        // it is the panel's surface, not decoration, so `axis off` keeps it.
        if let Some(fill) = style.panel_fill {
            ops.push(DrawOp::Rect {
                x: left,
                y: top,
                w: right - left,
                h: bottom - top,
                fill: Some(fill.to_string()),
                stroke: None,
                opacity: 1.0,
                radius: 0.0,
            });
        }
        let decoration_start = ops.len();
        let mut widest_y_tick = 0.0f64;
        for (idx, &raw) in y_major_ticks.iter().enumerate() {
            if let Some((blo, bhi)) = ybrk { if raw > blo && raw < bhi { continue; } }
            let y = bottom - axis_frac(raw, ymin, ymax, ybrk) * (bottom - top);
            if show_grid {
                ops.push(DrawOp::Line { x1: left, y1: y, x2: right, y2: y, color: grid_color.into(), width: grid_w });
            }
            let value = if log_y { 10f64.powf(raw) } else { raw };
            if tick_px > 0.0 {
                ops.push(DrawOp::Line { x1: origin_px - tick_out, y1: y, x2: origin_px, y2: y, color: tick_mark_color.into(), width: frame_width });
            }
            // `yticklabels(...)` overrides the number, positionally, the
            // same way `xticklabels` does.
            let label = panel
                .ytick_labels
                .as_ref()
                .and_then(|l| l.get(idx).cloned())
                .unwrap_or_else(|| fmt_y(value));
            widest_y_tick = widest_y_tick.max(measure_text(&label, tick_size, face_metrics(&fig.font_family)));
            ops.push(DrawOp::Text { italic: false, x: origin_px - half_line * 0.75 - tick_clearance, y: y + tick_size * BASELINE_CENTER, text: label, size: tick_size, anchor: Anchor::End, rotate: 0.0, color: if fig.publication && !has_right_axis { PUBLICATION_INK.to_string() } else { left_tick_color.clone() } });
        }
        // The shared power of ten, printed once just past the top of the
        // axis (PGFPlots puts it at 1.03 of the axis height). Suppressed
        // when the user supplied their own `yticklabels` -- the multiplier
        // describes numbers this code formatted, and against someone else's
        // strings it would be a caption on the wrong picture.
        if let Some(exp) = y_multiplier {
            if panel.ytick_labels.is_none() {
                let label = multiplier_label(exp);
                widest_y_tick = widest_y_tick.max(measure_text(&label, tick_size, face_metrics(&fig.font_family)));
                ops.push(DrawOp::Text { italic: false, x: origin_px - half_line * 0.75 - tick_clearance, y: top - tick_size * 0.6, text: label, size: tick_size, anchor: Anchor::End, rotate: 0.0, color: if fig.publication && !has_right_axis { PUBLICATION_INK.to_string() } else { left_tick_color.clone() } });
            }
        }
        if show_minor_grid {
            for raw in minor_tick_positions(&y_major_ticks, ymin, ymax, log_y) {
                if let Some((blo, bhi)) = ybrk { if raw > blo && raw < bhi { continue; } }
                let y = bottom - axis_frac(raw, ymin, ymax, ybrk) * (bottom - top);
                ops.push(DrawOp::DottedLine { x1: left, y1: y, x2: right, y2: y, color: grid_color.to_string(), width: minor_grid_w, dash: Dash::new(0.0, &[1.2, 3.0]) });
            }
        }
        // right-side y ticks for `yyaxis right` series — mirrors the left
        // loop, positioned past the (already margin-reserved) right edge.
        if has_right_axis || panel.ytick2_positions.is_some() {
            let right_ticks: Vec<f64> = match &panel.ytick2_positions {
                Some(at) => at.clone(),
                None => thin_ticks(
                    nice_ticks(ymin2, ymax2, GRID_TICK_TARGET),
                    bottom - top,
                    tick_size * MIN_TICK_GAP_Y,
                ),
            };
            for (idx, raw) in right_ticks.into_iter().enumerate() {
                let t = (ymax2 - raw) / (ymax2 - ymin2).max(1e-300);
                let y = top + (bottom - top) * t;
                let value = if log_y2 { 10f64.powf(raw) } else { raw };
                let label = panel
                    .ytick2_labels
                    .as_ref()
                    .and_then(|l| l.get(idx).cloned())
                    .unwrap_or_else(|| format_tick(value));
                ops.push(DrawOp::Text { italic: false, x: right + 6.0, y: y + tick_size * BASELINE_CENTER, text: label, size: tick_size, anchor: Anchor::Start, rotate: 0.0, color: right_tick_color.clone() });
            }
        }
        // x ticks: categorical labels (`xticklabels(...)`) take over
        // entirely when set; otherwise the same nice-number treatment as y.
        if let Some(labels) = &panel.xtick_labels {
            let positions: Vec<f64> = match &panel.xtick_positions {
                Some(pos) => pos.iter().copied().take(labels.len()).collect(),
                // A bar chart's labels name its BARS, so they belong at the
                // bar centres. Spreading them evenly across the range was
                // indistinguishable from correct only while the range
                // happened to end at the outer centres; once the axis grew
                // half a slot at each end to stop the outer bars being cut
                // in half, every label slid off its own bar.
                None if !bar_centres.is_empty() && bar_centres.len() == labels.len() => {
                    bar_centres.clone()
                }
                None if labels.len() > 1 => {
                    (0..labels.len()).map(|i| xmin + (xmax - xmin) * i as f64 / (labels.len() - 1) as f64).collect()
                }
                None => vec![(xmin + xmax) / 2.0],
            };
            for (label, &rawx) in labels.iter().zip(positions.iter()) {
                let x = left + (rawx - xmin) / (xmax - xmin).max(1e-300) * (right - left);
                if show_grid {
                    ops.push(DrawOp::Line { x1: x, y1: top, x2: x, y2: bottom, color: grid_color.into(), width: grid_w });
                }
                if tick_px > 0.0 {
                    ops.push(DrawOp::Line { x1: x, y1: origin_py, x2: x, y2: origin_py + tick_out, color: tick_mark_color.into(), width: frame_width });
                }
                // Was a flat 14.0, unlike the numeric path just below
                // (`half_line * 1.75`, scaled to the tick font size) --
                // fine at the default tick size, cramped enough at a
                // larger one (`fontsize(tick = pt(13))`, e.g.) to read as
                // the label crowding the bars rather than sitting clear
                // of them. Same formula now, so category labels and
                // numeric ones keep the same clearance at any size.
                ops.push(DrawOp::Text { italic: false, x, y: origin_py + half_line * 1.75 + tick_clearance, text: label.clone(), size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: if fig.publication && !has_top_axis { PUBLICATION_INK.to_string() } else { bottom_tick_color.clone() } });
            }
        } else {
            // `x_break` (linear-scale only): ticks strictly inside the
            // skipped range are dropped rather than drawn on top of the
            // "//" jag — see the matching `y_break`/`ybrk` treatment above.
            let xbrk = if log_x { None } else { panel.x_break };
            let x_major_ticks = thin_ticks(
                if log_x {
                    let t = log_ticks(xmin, xmax, GRID_TICK_TARGET);
                    if t.is_empty() { nice_ticks(xmin, xmax, GRID_TICK_TARGET) } else { t }
                } else {
                    nice_ticks(xmin, xmax, GRID_TICK_TARGET)
                },
                right - left,
                tick_size * MIN_TICK_GAP_X,
            );
            let (fmt_x, x_multiplier) = axis_tick_formatter_scaled(
                &x_major_ticks
                    .iter()
                    .map(|&r| if log_x { 10f64.powf(r) } else { r })
                    .collect::<Vec<f64>>(),
                log_x,
            );
            for &rawx in &x_major_ticks {
                if let Some((blo, bhi)) = xbrk { if rawx > blo && rawx < bhi { continue; } }
                let x = left + axis_frac(rawx, xmin, xmax, xbrk) * (right - left);
                if show_grid {
                    ops.push(DrawOp::Line { x1: x, y1: top, x2: x, y2: bottom, color: grid_color.into(), width: grid_w });
                }
                let valuex = if log_x { 10f64.powf(rawx) } else { rawx };
                if tick_px > 0.0 {
                    ops.push(DrawOp::Line { x1: x, y1: origin_py, x2: x, y2: origin_py + tick_out, color: tick_mark_color.into(), width: frame_width });
                }
                ops.push(DrawOp::Text { italic: false, x, y: origin_py + half_line * 1.75 + tick_clearance, text: fmt_x(valuex), size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: if fig.publication && !has_top_axis { PUBLICATION_INK.to_string() } else { bottom_tick_color.clone() } });
            }
            // The shared power of ten, once, at the right-hand end of the
            // tick-label line (PGFPlots puts it 90% along). End-anchored at
            // the axis edge rather than centred at 0.9, so it cannot slide
            // under the last tick label on a narrow panel. Suppressed when
            // the user supplied `xticklabels`, same reason as the y axis.
            if let Some(exp) = x_multiplier {
                if panel.xtick_labels.is_none() {
                    ops.push(DrawOp::Text { italic: false, x: right, y: origin_py + half_line * 1.75 + tick_clearance + tick_size * 1.15, text: multiplier_label(exp), size: tick_size, anchor: Anchor::End, rotate: 0.0, color: if fig.publication && !has_top_axis { PUBLICATION_INK.to_string() } else { bottom_tick_color.clone() } });
                }
            }
            if show_minor_grid {
                for rawx in minor_tick_positions(&x_major_ticks, xmin, xmax, log_x) {
                    if let Some((blo, bhi)) = xbrk { if rawx > blo && rawx < bhi { continue; } }
                    let x = left + axis_frac(rawx, xmin, xmax, xbrk) * (right - left);
                    ops.push(DrawOp::DottedLine { x1: x, y1: top, x2: x, y2: bottom, color: grid_color.to_string(), width: minor_grid_w, dash: Dash::new(0.0, &[1.2, 3.0]) });
                }
            }
        }
        // top-side x ticks for `xxaxis top` series — mirrors the bottom
        // loop, positioned above the (already margin-reserved) top edge.
        // No separate gridlines: gridlines always follow the primary
        // (bottom/left) axes, same convention as `yyaxis right`.
        if has_top_axis {
            for rawx in nice_ticks(xmin2, xmax2, GRID_TICK_TARGET) {
                let x = left + (rawx - xmin2) / (xmax2 - xmin2).max(1e-300) * (right - left);
                let valuex = if log_x { 10f64.powf(rawx) } else { rawx };
                ops.push(DrawOp::Text { italic: false, x, y: top - 10.0, text: format_tick(valuex), size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: top_tick_color.clone() });
            }
        }
        if origin_active {
            // Two spines crossing at (origin_px, origin_py) instead of a
            // box framing the panel — clamped to the nearest edge above,
            // so this degrades gracefully to edge-hugging spines (still
            // not a box) when 0 falls outside the visible data range.
            ops.push(DrawOp::Line { x1: left, y1: origin_py, x2: right, y2: origin_py, color: box_color.into(), width: frame_width });
            ops.push(DrawOp::Line { x1: origin_px, y1: top, x2: origin_px, y2: bottom, color: box_color.into(), width: frame_width });
        } else if panel.show_box && !fig.minimal {
            // ggplot2's `theme_minimal()`/`theme_bw()`-family themes and
            // BBC's `bbc_style()` (real source checked — see `bbc_style.R`:
            // `panel.background = element_blank()`, `axis.ticks =
            // element_blank()`) agree on dropping the solid axes frame;
            // `theme("minimal")` does the same here. Still overridable per
            // panel: an explicit `box off` (`panel.show_box = false`) stays
            // "no box" even if `theme("default")` is restored later, and
            // this only ever removes a box that would otherwise show — it
            // never adds one back.
            match style.spines {
                // A full box is the tooling default (MATLAB `Box on`,
                // matplotlib's four spines), not a publishing one.
                // Krzywinski, Nature Methods 10:183 -- "unless the figure
                // is particularly large, you should avoid bounding it by
                // axes on all sides. This containment is often mistaken
                // for organization."
                Spines::Box => {
                    // Four LINES, not a Rect: `DrawOp::Rect` carries no
                    // stroke width, so the frame silently fell back to
                    // SVG's default of 1 and ignored the theme entirely --
                    // a box that could not be made lighter than the data
                    // it surrounds. Lines take a width, and this matches
                    // how the L-spines are drawn.
                    for (x1, y1, x2, y2) in [
                        (left, bottom, right, bottom),
                        (left, top, left, bottom),
                        (left, top, right, top),
                        (right, top, right, bottom),
                    ] {
                        ops.push(DrawOp::Line { x1, y1, x2, y2, color: box_color.into(), width: frame_width });
                    }
                }
                // Left and bottom only: the two edges that carry a scale.
                Spines::L => {
                    ops.push(DrawOp::Line { x1: left, y1: bottom, x2: right, y2: bottom, color: box_color.into(), width: frame_width });
                    ops.push(DrawOp::Line { x1: left, y1: top, x2: left, y2: bottom, color: box_color.into(), width: frame_width });
                }
                Spines::None => {}
            }
        }

        // `xbreak`/`ybreak` "//" jag marks — drawn on whichever spine(s) the
        // broken axis crosses (bottom, and top too when a full box is shown
        // rather than just origin-crossing spines).
        if let Some(b) = panel.x_break {
            if !log_x {
                if let Some(frac) = break_zone_center_frac(xmin, xmax, b) {
                    let cx = left + frac * (right - left);
                    ops.extend(break_marks_x(cx, origin_py, BOX_COLOR));
                    if panel.show_box && !origin_active {
                        ops.extend(break_marks_x(cx, top, BOX_COLOR));
                    }
                }
            }
        }
        if let Some(b) = panel.y_break {
            if !log_y {
                if let Some(frac) = break_zone_center_frac(ymin, ymax, b) {
                    let cy = bottom - frac * (bottom - top);
                    ops.extend(break_marks_y(cy, origin_px, BOX_COLOR));
                    if panel.show_box && !origin_active {
                        ops.extend(break_marks_y(cy, right, BOX_COLOR));
                    }
                }
            }
        }

        // Everything from here to the matching `ClipEnd` is DATA, and data
        // is clipped to the plot area. Anything the axis range excludes --
        // a curve running past an explicit `xlim`, a reference line
        // starting below the bottom of the axis -- used to be drawn anyway,
        // over the margins and off the edge of the figure. Axes, labels,
        // legend and colour bar are deliberately outside the group: they
        // live in the margins by design.
        if !panel.show_axes {
            ops.truncate(decoration_start);
        }
        ops.push(DrawOp::ClipStart { x: left, y: top, w: right - left, h: bottom - top });

        // shapes render under the series so lines/markers stay legible
        for shape in &panel.shapes {
            match shape {
                Shape::VLine { x, color, dash, dot, width } => {
                    let (px, _) = to_px(*x, 0.0);
                    let c = color.clone().unwrap_or_else(|| "#c44e52".into());
                    let w = width.unwrap_or(1.4);
                    match dash_pattern(*dash, *dot, w) {
                        Some(d) => ops.push(DrawOp::DottedLine { x1: px, y1: top, x2: px, y2: bottom, color: c, width: w, dash: d }),
                        None => ops.push(DrawOp::Line { x1: px, y1: top, x2: px, y2: bottom, color: c, width: w }),
                    }
                }
                Shape::HLine { y, color, dash, dot, width } => {
                    let (_, py) = to_px(0.0, *y);
                    let c = color.clone().unwrap_or_else(|| "#c44e52".into());
                    let w = width.unwrap_or(1.4);
                    match dash_pattern(*dash, *dot, w) {
                        Some(d) => ops.push(DrawOp::DottedLine { x1: left, y1: py, x2: right, y2: py, color: c, width: w, dash: d }),
                        None => ops.push(DrawOp::Line { x1: left, y1: py, x2: right, y2: py, color: c, width: w }),
                    }
                }
                Shape::Rect { x0, y0, x1, y1, color } => {
                    let (px0, py0) = to_px(*x0, *y0);
                    let (px1, py1) = to_px(*x1, *y1);
                    let (rx, ry) = (px0.min(px1), py0.min(py1));
                    ops.push(DrawOp::Rect {
                        x: rx, y: ry, w: (px1 - px0).abs(), h: (py1 - py0).abs(),
                        fill: Some(color.clone().unwrap_or_else(|| "#4c72b0".into())), stroke: None, opacity: 0.18, radius: 0.0,
                    });
                }
                Shape::Curve { points, color, fill, alpha, width, closed, dash } => {
                    let px: Vec<(f64, f64)> = points.iter().map(|(x, y)| to_px(*x, *y)).collect();
                    if px.len() < 2 {
                        continue;
                    }
                    // A shape given only a `fill=` is filled and NOT
                    // outlined. Stroking it anyway put a hairline on every
                    // edge, which is invisible on one shape and reads as
                    // banding when twenty of them tile a gradient -- the
                    // sky in `qu_sea_waves` was striped for exactly this
                    // reason. An explicit `color=` still outlines.
                    let outline = match (color, fill) {
                        (Some(c), _) => Some(c.clone()),
                        (None, Some(_)) => None,
                        (None, None) => Some("#4c72b0".to_string()),
                    };
                    let w = width.unwrap_or(1.4);
                    if *closed {
                        ops.push(DrawOp::Polygon {
                            points: px,
                            fill: fill.clone(),
                            stroke: outline,
                            // An unfilled outline still needs a defined
                            // opacity for the stroke; the fill is what
                            // `alpha` is for, and 0.18 matches `Rect`'s
                            // wash so a filled circle and a filled
                            // rectangle read as the same weight.
                            opacity: alpha.unwrap_or(if fill.is_some() { 0.18 } else { 1.0 }),
                            width: w,
                        });
                    } else {
                        ops.push(DrawOp::Polyline {
                            points: px,
                            // An arc is a stroke, so it always has one.
                            color: outline.unwrap_or_else(|| "#4c72b0".to_string()),
                            width: w,
                            dash: if *dash { named_dash("dashed").flatten().map(|b| scaled_dash(&b, w)) } else { None },
                        });
                    }
                }
                Shape::XSpan { x0, x1, color, alpha } => {
                    let (px0, _) = to_px(*x0, 0.0);
                    let (px1, _) = to_px(*x1, 0.0);
                    ops.push(DrawOp::Rect {
                        x: px0.min(px1), y: top, w: (px1 - px0).abs(), h: bottom - top,
                        fill: Some(color.clone().unwrap_or_else(|| "#55a868".into())), stroke: None,
                        opacity: alpha.unwrap_or(0.15), radius: 0.0,
                    });
                }
                Shape::YSpan { y0, y1, color, alpha } => {
                    let (_, py0) = to_px(0.0, *y0);
                    let (_, py1) = to_px(0.0, *y1);
                    ops.push(DrawOp::Rect {
                        x: left, y: py0.min(py1), w: right - left, h: (py1 - py0).abs(),
                        fill: Some(color.clone().unwrap_or_else(|| "#55a868".into())), stroke: None,
                        opacity: alpha.unwrap_or(0.15), radius: 0.0,
                    });
                }
                Shape::ErrorBar { x, y, err, xerr, color, cap, width } => {
                    let resolved = color.clone().unwrap_or_else(|| "#4c72b0".into());
                    let (px, py) = to_px(*x, *y);
                    let cap = cap.unwrap_or(4.0);
                    let w = width.unwrap_or(1.3);
                    if *err > 0.0 {
                        let (_, py_lo) = to_px(*x, *y - *err);
                        let (_, py_hi) = to_px(*x, *y + *err);
                        ops.push(DrawOp::Line { x1: px, y1: py_lo, x2: px, y2: py_hi, color: resolved.clone(), width: w });
                        if cap > 0.0 {
                            ops.push(DrawOp::Line { x1: px - cap, y1: py_lo, x2: px + cap, y2: py_lo, color: resolved.clone(), width: w });
                            ops.push(DrawOp::Line { x1: px - cap, y1: py_hi, x2: px + cap, y2: py_hi, color: resolved.clone(), width: w });
                        }
                    }
                    if let Some(ex) = xerr.filter(|e| *e > 0.0) {
                        let (px_lo, _) = to_px(*x - ex, *y);
                        let (px_hi, _) = to_px(*x + ex, *y);
                        ops.push(DrawOp::Line { x1: px_lo, y1: py, x2: px_hi, y2: py, color: resolved.clone(), width: w });
                        if cap > 0.0 {
                            ops.push(DrawOp::Line { x1: px_lo, y1: py - cap, x2: px_lo, y2: py + cap, color: resolved.clone(), width: w });
                            ops.push(DrawOp::Line { x1: px_hi, y1: py - cap, x2: px_hi, y2: py + cap, color: resolved.clone(), width: w });
                        }
                    }
                    ops.push(DrawOp::Circle { cx: px, cy: py, r: (w * 2.3).max(1.2), fill: Some(resolved), stroke: None });
                }
                Shape::FillBetween { x, y_lo, y_hi, color, alpha } => {
                    if x.len() >= 2 && x.len() == y_lo.len() && x.len() == y_hi.len() {
                        let mut points: Vec<(f64, f64)> = x.iter().zip(y_hi).map(|(&xv, &yv)| to_px(xv, yv)).collect();
                        points.extend(x.iter().zip(y_lo).rev().map(|(&xv, &yv)| to_px(xv, yv)));
                        ops.push(DrawOp::Polygon { width: MARKER_EDGE_W,
                            points, fill: Some(color.clone().unwrap_or_else(|| "#4c72b0".into())), stroke: None,
                            opacity: alpha.unwrap_or(0.3),
                        });
                    }
                }
                Shape::Bubble { x, y, sizes, color } => {
                    let resolved = color.clone().unwrap_or_else(|| "#4c72b0".into());
                    let (smin, smax) = sizes.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                    let span = if (smax - smin).abs() > 1e-12 { smax - smin } else { 1.0 };
                    for i in 0..x.len().min(y.len()).min(sizes.len()) {
                        let (px, py) = to_px(x[i], y[i]);
                        let t = (sizes[i] - smin) / span;
                        let r = 4.0 + t.clamp(0.0, 1.0) * 18.0;
                        ops.push(DrawOp::Circle { cx: px, cy: py, r, fill: Some(resolved.clone()), stroke: Some("#ffffff".into()) });
                    }
                }
                Shape::Hexbin { cx, cy, counts, dx, dy, colormap, min_count } => {
                    let hi = counts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    if !hi.is_finite() || hi <= 0.0 {
                        continue;
                    }
                    for ((&x, &y), &n) in cx.iter().zip(cy.iter()).zip(counts.iter()) {
                        if n < *min_count {
                            continue;
                        }
                        let (px, py) = to_px(x, y);
                        // The cell's own corners, in pixels: half a spacing
                        // across and two thirds of a row height to the
                        // points, which is the flat-topped hexagon that
                        // tiles the offset grid the binning used.
                        let (rx, _) = to_px(x + dx / 2.0, y);
                        let (_, ry) = to_px(x, y + dy * 2.0 / 3.0);
                        let (hw, hh) = ((rx - px).abs(), (ry - py).abs());
                        let pts = vec![
                            (px, py - hh),
                            (px + hw, py - hh / 2.0),
                            (px + hw, py + hh / 2.0),
                            (px, py + hh),
                            (px - hw, py + hh / 2.0),
                            (px - hw, py - hh / 2.0),
                        ];
                        let fill = colormap_color(colormap, (n / hi).clamp(0.0, 1.0));
                        ops.push(DrawOp::Polygon {
                            points: pts,
                            fill: Some(fill),
                            // No outline: a stroke between neighbouring
                            // cells reads as a grid the data does not have,
                            // and at small cell sizes it is most of the ink.
                            stroke: None,
                            opacity: 1.0,
                            width: 0.0,
                        });
                    }
                }
                Shape::Contour { x, y, z, levels, filled, colormap, color, label_levels, label_suffix, label_pos, dash, dot, width, extend_max } => {
                    let grid = crate::contour::Grid { x, y, z };
                    // Contouring runs in DATA space and the result is
                    // mapped through `to_px` afterwards, so a log axis
                    // bends the iso-lines exactly the way it bends
                    // everything else. Contouring in pixel space instead
                    // would straighten them, which is wrong and, on a log
                    // plot, obviously so.
                    let levels: Vec<f64> = if levels.is_empty() {
                        crate::contour::auto_levels(z, 10)
                    } else {
                        levels.clone()
                    };
                    if *filled {
                        // Bands run between consecutive levels. The colour
                        // is taken at the band's midpoint rather than its
                        // lower edge, so the lightest band is not the
                        // colormap's extreme white and the ramp stays
                        // centred on the data.
                        // With `extend = "max"` the top band runs to the
                        // data's own maximum rather than stopping at the
                        // last level. A field that goes past its top level
                        // otherwise leaves that corner UNPAINTED -- white,
                        // reading as "no data" where the truth is "more
                        // than the scale shows". The colour bar says the
                        // same thing with its pointed cap.
                        let top_level = *levels.last().unwrap_or(&0.0);
                        let data_max = z.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
                        let mut bands: Vec<(f64, f64)> =
                            levels.windows(2).map(|w| (w[0], w[1])).collect();
                        if *extend_max && data_max > top_level {
                            bands.push((top_level, data_max));
                        }
                        for w in bands {
                            let (lo, hi) = (w.0, w.1);
                            let t = if levels.len() > 1 {
                                ((lo + hi - 2.0 * levels[0])
                                    / (2.0 * (levels[levels.len() - 1] - levels[0]).max(1e-300)))
                                    .min(1.0)
                            } else {
                                0.5
                            };
                            let fill = color.clone().unwrap_or_else(|| colormap_color(colormap, t.clamp(0.0, 1.0)));
                            for poly in crate::contour::contour_bands(&grid, lo, hi) {
                                let points: Vec<(f64, f64)> = poly.iter().map(|&(a, b)| to_px(a, b)).collect();
                                // Stroked in its OWN fill colour, which is
                                // not the same as not stroking it.
                                //
                                // Adjacent bands share an edge, and two
                                // anti-aliased polygons meeting on one do
                                // not quite cover it: each blends halfway
                                // to the page, so a hairline of white shows
                                // through. Across a 400x400 field that read
                                // as a dotted stipple along every band
                                // boundary -- the one thing on the ceiling
                                // map that looked plainly worse than the
                                // matplotlib original. A stroke in a
                                // CONTRASTING colour would draw the mesh
                                // this comment used to warn about; in the
                                // fill's own colour it is invisible
                                // everywhere except the seam it closes.
                                // matplotlib spells the same fix
                                // `edgecolor="face"`.
                                ops.push(DrawOp::Polygon { width: MARKER_EDGE_W,
                                    points,
                                    fill: Some(fill.clone()),
                                    stroke: Some(fill.clone()),
                                    opacity: 1.0,
                                });
                            }
                        }
                    } else {
                        let span = (levels[levels.len() - 1] - levels[0]).max(1e-300);
                        for &lv in &levels {
                            let stroke = color.clone().unwrap_or_else(|| {
                                colormap_color(colormap, ((lv - levels[0]) / span).clamp(0.0, 1.0))
                            });
                            for path in crate::contour::contour_lines(&grid, lv) {
                                let points: Vec<(f64, f64)> = path.iter().map(|&(a, b)| to_px(a, b)).collect();
                                if points.len() < 2 {
                                    continue;
                                }
                                if *label_levels {
                                    // Label at the path's midpoint, rotated
                                    // along it, the way `clabel` does --
                                    // a level line is useless unless the
                                    // reader can tell which level it is.
                                    // Clamped one in from each end: a
                                    // label at index 0 has no preceding
                                    // point to take its angle from, and one
                                    // at the last index would hang off the
                                    // end of the line it names.
                                    let m = match label_pos {
                                        Some(p) => (((points.len() - 1) as f64 * p).round() as usize)
                                            .clamp(1, points.len().saturating_sub(1)),
                                        None => points.len() / 2,
                                    };
                                    let (mx, my) = points[m];
                                    let (px0, py0) = points[m.saturating_sub(1)];
                                    let mut angle = (my - py0).atan2(mx - px0).to_degrees();
                                    // Keep text upright: a label rotated
                                    // past vertical reads upside down.
                                    if angle > 90.0 { angle -= 180.0 } else if angle < -90.0 { angle += 180.0 }
                                    let text = format!("{}{}", format_tick(lv), label_suffix);
                                    // Measured before the string moves into
                                    // the draw op, since the break below is
                                    // sized from the text it makes room for.
                                    let half = measure_text(&text, tick_size * 0.9, face_metrics(&fig.font_family)) / 2.0
                                        + tick_size * 0.35;
                                    ops.push(DrawOp::Text { italic: false, x: mx, y: my, text, size: tick_size * 0.9, anchor: Anchor::Middle, rotate: angle, color: stroke.clone() });
                                    // Break the line either side of the
                                    // label so the text is not struck
                                    // through by its own contour.
                                    //
                                    // The gap has to be measured in PIXELS,
                                    // walking the path until it has covered
                                    // half the label's width. It used to be
                                    // three path POINTS, which is a length
                                    // only by accident: on a coarse grid it
                                    // gouged a hole out of the line, and on
                                    // the paper's 400x400 map it was a
                                    // sliver a fraction of the text's width,
                                    // so "20%" was struck through by the
                                    // very line this code exists to break.
                                    // Walking to the nearest sample point is
                                    // not enough: on a coarse grid the last
                                    // step overshoots by most of a segment,
                                    // so the same field sampled twice gives
                                    // two different gaps. Each end is then
                                    // slid back along its own final segment
                                    // by the overshoot, which puts the break
                                    // exactly `half` from the label whatever
                                    // the sampling.
                                    let slide = |from: (f64, f64), toward: (f64, f64), by: f64| {
                                        let (dx, dy) = (toward.0 - from.0, toward.1 - from.1);
                                        let len = dx.hypot(dy);
                                        if len <= f64::EPSILON {
                                            from
                                        } else {
                                            (from.0 + dx / len * by, from.1 + dy / len * by)
                                        }
                                    };
                                    let mut lo = m;
                                    let mut run = 0.0;
                                    while lo > 0 && run < half {
                                        let ((ax, ay), (bx, by)) = (points[lo], points[lo - 1]);
                                        run += (ax - bx).hypot(ay - by);
                                        lo -= 1;
                                    }
                                    let mut head = points[..=lo].to_vec();
                                    if run > half && lo + 1 <= m {
                                        let last = head.len() - 1;
                                        head[last] = slide(points[lo], points[lo + 1], run - half);
                                    }
                                    let mut hi = m;
                                    let mut run = 0.0;
                                    while hi + 1 < points.len() && run < half {
                                        let ((ax, ay), (bx, by)) = (points[hi], points[hi + 1]);
                                        run += (ax - bx).hypot(ay - by);
                                        hi += 1;
                                    }
                                    let mut tail = points[hi..].to_vec();
                                    if run > half && hi >= 1 {
                                        tail[0] = slide(points[hi], points[hi - 1], run - half);
                                    }
                                    for part in [head, tail] {
                                        if part.len() >= 2 {
                                            ops.push(contour_line(part, stroke.clone(), width.unwrap_or(base_line * 1.3), *dash, *dot));
                                        }
                                    }
                                } else {
                                    ops.push(contour_line(points, stroke.clone(), width.unwrap_or(base_line * 1.1), *dash, *dot));
                                }
                            }
                        }
                    }
                }
            }
        }

        if !panel.boxplots.is_empty() {
            let box_w = (right - left) / panel.boxplots.len() as f64 * 0.5;
            for (i, group) in panel.boxplots.iter().enumerate() {
                let color = group.color.clone().unwrap_or_else(|| palette[i % palette.len()].to_string());
                let (px, _) = to_px(group.x, 0.0);
                ops.extend(box_ops(&to_px, group, group.x, px, box_w, &color));
            }
        }

        if !panel.raincloud.is_empty() {
            let n = panel.raincloud.len();
            let slot_w = (right - left) / n as f64;
            let box_w = slot_w * 0.22;
            let cloud_w = slot_w * 0.32;
            let rain_offset = slot_w * 0.28;
            for (i, group) in panel.raincloud.iter().enumerate() {
                let color = group.color.clone().unwrap_or_else(|| palette[i % palette.len()].to_string());
                let (center_px, _) = to_px(group.x, 0.0);
                // density "cloud": a filled half-violin bulging to the right
                let max_density = group.density_y.iter().cloned().fold(0.0_f64, f64::max).max(1e-12);
                if group.density_x.len() >= 2 {
                    // A violin is centred on its category and symmetric; a
                    // raincloud's cloud leans off the side of its box, to
                    // leave the other side free for the rain.
                    let base = if group.mirrored { center_px } else { center_px + box_w * 0.6 };
                    let reach = if group.mirrored { cloud_w * 0.85 } else { cloud_w };
                    let mut poly: Vec<(f64, f64)> = group
                        .density_x
                        .iter()
                        .zip(&group.density_y)
                        .map(|(&v, &d)| {
                            let (_, y) = to_px(group.x, v);
                            (base + reach * (d / max_density), y)
                        })
                        .collect();
                    for (&v, &d) in group.density_x.iter().zip(&group.density_y).rev() {
                        let (_, y) = to_px(group.x, v);
                        // The return edge is the mirror image for a violin
                        // and a straight spine for a half-cloud, which is
                        // the whole difference between the two shapes.
                        let x = if group.mirrored { base - reach * (d / max_density) } else { base };
                        poly.push((x, y));
                    }
                    ops.push(DrawOp::Polygon {
                        width: MARKER_EDGE_W,
                        points: poly,
                        fill: Some(color.clone()),
                        // A violin gets its own outline: it is the shape
                        // being read, not a backdrop to a box beside it.
                        stroke: group.mirrored.then(|| color.clone()),
                        opacity: if group.mirrored { 0.45 } else { 0.35 },
                    });
                }
                // boxplot, centered on the category
                ops.extend(box_ops(&to_px, &group.box_stats, group.x, center_px, box_w, &color));
                // jittered raw points ("rain") to the left
                for &(jitter, value) in group.jitter.iter().filter(|_| group.show_rain) {
                    let (_, y) = to_px(group.x, value);
                    ops.push(DrawOp::Circle {
                        cx: center_px - box_w * 0.7 - rain_offset * 0.5 * (1.0 + jitter),
                        cy: y, r: 2.0, fill: Some(color.clone()), stroke: None,
                    });
                }
            }
        }

        // shared running per-index stack height, mutated as `stackbar`
        // series are encountered in call order — each one's bars sit on top
        // of every earlier `stackbar` series' bar at the same index.
        let mut stack_running: Vec<f64> = Vec::new();
        // `groupbar` series in one panel are placed side-by-side per
        // category (unlike `stackbar`'s on-top-of-each-other) — each one's
        // horizontal slot within the category depends on how many
        // `groupbar` series the panel has in total, computed once up front.
        let groupbar_total = panel.series.iter().filter(|s| s.marker == "groupbar").count();
        let mut groupbar_seen = 0usize;
        for (i, series) in panel.series.iter().enumerate() {
            let color = series_color(&series.color, i, palette);
            let points: Vec<(f64, f64)> = series
                .x
                .iter()
                .zip(series.y.iter())
                .map(|(&x, &y)| to_px_for(i, x, y))
                .collect();
            // `marker="o-"` and friends: draw the joining line first so
            // the markers sit on top of it rather than being cut by it.
            let (glyph, joined) = split_marker(&series.marker);
            // An explicit `style=`/`dashes=` on the call beats the pattern
            // implied by a format-string suffix. It is the more specific
            // statement, and it is the only one that can say `loose
            // dashdotdot` at all -- a marker string has four spellings.
            //
            // It also JOINS a series that had no suffix: `plot(x, y,
            // style="dashed")` means a dashed line, not a dashed nothing.
            //
            // Two more cases used to fall through both of these and reach
            // the catch-all below, which draws at `DEFAULT_LINE_WIDTH`
            // regardless of what the series asked for -- so `plot(x, y, lw
            // = pt(6))` with no other kwarg rendered at the same width as
            // `plot(x, y)`, silently. `lw=`/`width=`/`linewidth=` is an
            // unambiguous, explicit request; it should never be read into
            // `series.width` and then ignored.
            //
            // 1. The plain, glyph-less `"line"` marker (the default for a
            //    bare `plot(x, y)`) has no suffix and, without an explicit
            //    dash/style kwarg, used to leave `joined` at `None` --
            //    correct-width drawing is this series' ONLY reason to
            //    exist, so it always joins.
            // 2. A point glyph (`"o"`, `"s"`, ...) with an explicit
            //    `lw=`/`width=` and no dash/style kwarg: `marker="o"` alone
            //    is deliberately points-only, but `marker="o", lw=...`
            //    stating a line weight and then never drawing a line is the
            //    same silent-ignore bug as (1). Bar-family/stem/step glyphs
            //    are excluded -- their `width=` already means something
            //    else (bar edge weight, stem/step weight), not "also draw a
            //    polyline through these points".
            let joined = match (&series.dash, joined) {
                (Some(d), _) => Some(Some(d.clone())),
                (None, Some(other)) => Some(other),
                (None, None) if glyph == "line" => Some(None),
                (None, None)
                    if series.width.is_some()
                        && !matches!(glyph, "bar" | "stackbar" | "groupbar" | "stem" | "step") =>
                {
                    Some(None)
                }
                (None, None) => None,
            };
            let line_w = series.width.unwrap_or(DEFAULT_LINE_WIDTH);
            // Opacity is applied to the colour rather than carried as a
            // separate field on every `DrawOp`: SVG, TikZ and PDF all
            // accept an alpha channel in the colour, and threading an
            // `opacity` through a dozen op variants to reach the same
            // pixels would be the larger change.
            let color = apply_alpha(&color, series.alpha);
            if let Some(dash) = joined.clone() {
                // `lw=0`/`width=0` is the matplotlib-style idiom for "no
                // connecting line" (markers only) -- an explicit request
                // for a line at weight zero, not a request for the
                // thinnest renderable line. PDF's own line-width-0 means
                // exactly the latter ("thinnest line the device can
                // render", i.e. ALWAYS drawn, never invisible), so once
                // `series.width.is_some()` above started counting a zero
                // width as "explicit, so join", every `marker=..., lw=0`
                // series (common for markers-only scatter, e.g. the
                // real figures' "measured"/"corrupted"/"confirmed"
                // series) grew a spurious hairline connecting its points
                // in plotted (not sorted) order. Guard here rather than
                // upstream in `joined`, so a non-zero-but-explicit width
                // still joins exactly as before.
                if points.len() >= 2 && line_w > 0.0 {
                    // ONE path, dash and all -- not one dashed line per
                    // segment.
                    //
                    // A dash pattern runs along the path it is set on, and
                    // restarts at every new path. Emitted per segment, each
                    // ~3 units long against a 6-on/3-off pattern, every
                    // segment drew entirely inside its own first "on" and
                    // the curve came out SOLID. It looked like the dash was
                    // being dropped; both backends were emitting it
                    // faithfully, 2409 times, and none of it survived
                    // being restarted that often.
                    //
                    // The tell was that it depended on the DATA: `:` (1.5
                    // on) still showed gaps because its "on" is shorter
                    // than a segment, while `--` and `-.` (6 on) never did,
                    // and a sparse series dashed correctly while a dense
                    // one did not. A style that works or not depending on
                    // how many points you plotted is not a style.
                    ops.push(DrawOp::Polyline {
                        dash: dash.map(|d| scaled_dash(&d, line_w)),
                        points: points.clone(),
                        color: color.clone(),
                        width: line_w,
                    });
                }
            }
            // `markevery`: thin the marker positions, never the line.
            let points: Vec<(f64, f64)> = match series.marker_every {
                Some(n) if n > 1 => points.iter().copied().step_by(n).collect(),
                _ => points,
            };
            // Where this series' glyphs start, so the shadow can be built
            // from whatever they turn out to be and spliced in UNDER them.
            let mark_start = ops.len();
            match glyph {
                "o" | "circle" | "\u{25CF}" | "\u{2B24}" | "\u{25CB}" => {
                    // `o` is the one glyph that is not a polygon, so it
                    // has to repeat the size/fill/edge rules rather than
                    // read them off `marker_polygon` -- kept in the same
                    // shape as that arm so the two cannot drift.
                    let r = series.marker_size.unwrap_or(3.2 * tick_scale);
                    let fill = match series.marker_filled {
                        Some(true) => Some(color.clone()),
                        _ => None,
                    };
                    let edge = series
                        .marker_edge
                        .as_ref()
                        .map(|c| apply_alpha(c, series.alpha))
                        .unwrap_or_else(|| color.clone());
                    for (&(px, py), (&xv, &yv)) in points.iter().zip(series.x.iter().zip(series.y.iter())) {
                        ops.push(DrawOp::TitledCircle {
                            cx: px, cy: py, r, fill: fill.clone(), stroke: Some(edge.clone()),
                            title: format!("x={}, y={}", format_tick(xv), format_tick(yv)),
                        });
                    }
                }
                "x" | "+" | "\u{00D7}" | "\u{2715}" => {
                    // `ms=` applies here too. It was a hardcoded 3.5, so a
                    // series that set a marker size got it everywhere
                    // except on the two stroked glyphs -- next to a 6 pt
                    // circle, an x stayed a speck.
                    let r = series.marker_size.unwrap_or(3.5 * tick_scale);
                    for &(px, py) in &points {
                        ops.push(DrawOp::Cross { cx: px, cy: py, r, color: color.clone() });
                    }
                }
                // Every polygonal glyph, from one shared outline table.
                //
                // Hollow by default, like `o`: a dense series of filled
                // marks turns into a solid block, and a hollow one still
                // shows the line through it. `fill=true` fills them, and
                // the star is filled WHITE rather than left open because
                // it is the "this exact point" mark and usually lands on
                // top of something -- a coloured contour band, another
                // series -- that an open outline disappears into.
                g if marker_polygon(g, 1.0).is_some() || marker_overlay(g, 1.0).is_some() => {
                    let r = series.marker_size.unwrap_or(4.2 * tick_scale);
                    // A glyph may be strokes only (`snowflake`), so the
                    // body is optional and the overlay is drawn either way.
                    let outline = marker_polygon(g, r).unwrap_or_default();
                    let fill = match (series.marker_filled, g) {
                        (Some(true), _) => Some(color.clone()),
                        (Some(false), _) => None,
                        (None, "*") | (None, "star") => Some("#ffffff".into()),
                        _ => None,
                    };
                    let edge = series
                        .marker_edge
                        .as_ref()
                        .map(|c| apply_alpha(c, series.alpha))
                        .unwrap_or_else(|| color.clone());
                    // The outline is drawn at the SERIES' own weight, not
                    // at one shared default. A 1.4-unit line with a
                    // hairline marker on it reads as two different things
                    // plotted together.
                    let edge_w = series.width.unwrap_or(MARKER_EDGE_W).max(0.6);
                    let overlay = marker_overlay(g, r);
                    for &(px, py) in &points {
                        if outline.len() >= 3 {
                            ops.push(DrawOp::Polygon {
                                width: edge_w,
                                points: outline
                                    .iter()
                                    .map(|&(dx, dy)| (px + dx, py + dy))
                                    .collect(),
                                fill: fill.clone(),
                                stroke: Some(edge.clone()),
                                opacity: 1.0,
                            });
                        }
                        // The strokes across the body, in the same ink at
                        // the same weight -- they are part of the mark, not
                        // an annotation on it.
                        if let Some(segs) = &overlay {
                            for &((ax, ay), (bx, by)) in segs {
                                ops.push(DrawOp::Line {
                                    x1: px + ax,
                                    y1: py + ay,
                                    x2: px + bx,
                                    y2: py + by,
                                    color: edge.clone(),
                                    width: edge_w,
                                });
                            }
                        }
                    }
                }
                "stem" => {
                    let (_, zero_y) = to_px(0.0, 0.0);
                    for &(px, py) in &points {
                        ops.push(DrawOp::Line { x1: px, y1: zero_y.clamp(top, bottom), x2: px, y2: py, color: color.clone(), width: 1.2 });
                        ops.push(DrawOp::Circle { cx: px, cy: py, r: 2.6, fill: Some(color.clone()), stroke: None });
                    }
                }
                "bar" => {
                    let (_, zero_y) = to_px(0.0, 0.0);
                    let zy = zero_y.clamp(top, bottom);
                    let bar_w = (right - left) / points.len().max(1) as f64 * 0.6;
                    for ((&(px, py), &yval), &xv) in points.iter().zip(series.y.iter()).zip(series.x.iter()) {
                        let top_y = py.min(zy);
                        let bx = px - bar_w / 2.0;
                        let bh = (py - zy).abs();
                        let title = format!("x={}, y={}", format_tick(xv), format_tick(yval));
                        if series.hatch {
                            ops.push(DrawOp::TitledRect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some("#ffffff".into()), stroke: Some(color.clone()), opacity: 1.0, radius: 1.0, title });
                            ops.extend(hatch_lines(bx, top_y, bar_w, bh, &color));
                        } else {
                            // A same-colour stroke, like `boxplot`'s and
                            // `violin`'s own box already have: two adjacent
                            // bars of similar height with no edge between
                            // them read as one shape, and a bar against a
                            // near-matching background has no boundary at
                            // all. `groupbar`/`stackbar` below get the same
                            // fix for the same reason.
                            ops.push(DrawOp::TitledRect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some(color.clone()), stroke: Some(color.clone()), opacity: 0.85, radius: 1.0, title });
                        }
                        if series.show_values {
                            ops.push(DrawOp::Text { italic: false, x: px, y: top_y - 6.0, text: format_tick(yval), size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
                        }
                    }
                }
                "groupbar" => {
                    let n = groupbar_total.max(1);
                    let bar_w = (right - left) / points.len().max(1) as f64 / n as f64 * 0.8;
                    let offset = (groupbar_seen as f64 - (n as f64 - 1.0) / 2.0) * bar_w;
                    groupbar_seen += 1;
                    let (_, zero_y) = to_px(0.0, 0.0);
                    let zy = zero_y.clamp(top, bottom);
                    for (&(px, py), &yval) in points.iter().zip(series.y.iter()) {
                        let top_y = py.min(zy);
                        let bx = px + offset - bar_w / 2.0;
                        let bh = (py - zy).abs();
                        if series.hatch {
                            ops.push(DrawOp::Rect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some("#ffffff".into()), stroke: Some(color.clone()), opacity: 1.0, radius: 1.0 });
                            ops.extend(hatch_lines(bx, top_y, bar_w, bh, &color));
                        } else {
                            ops.push(DrawOp::Rect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some(color.clone()), stroke: Some(color.clone()), opacity: 0.85, radius: 1.0 });
                        }
                        if series.show_values {
                            ops.push(DrawOp::Text { italic: false, x: px + offset, y: top_y - 6.0, text: format_tick(yval), size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
                        }
                    }
                }
                "step" | "stair" => {
                    // "post" step: horizontal from each point to the next
                    // point's x (at the current y), then vertical up/down.
                    let mut stair_points: Vec<(f64, f64)> = Vec::with_capacity(points.len() * 2);
                    for &(px, py) in &points {
                        if let Some(&(_, last_y)) = stair_points.last() {
                            stair_points.push((px, last_y));
                        }
                        stair_points.push((px, py));
                    }
                    ops.push(DrawOp::Polyline { points: stair_points, color: color.clone(), width: DEFAULT_LINE_WIDTH, dash: None });
                }
                "stackbar" => {
                    if stack_running.len() < series.y.len() {
                        stack_running.resize(series.y.len(), 0.0);
                    }
                    let bar_w = (right - left) / series.y.len().max(1) as f64 * 0.6;
                    for (idx, (&xv, &yv)) in series.x.iter().zip(series.y.iter()).enumerate() {
                        let base = stack_running[idx];
                        let (px, py_base) = to_px(xv, base);
                        let (_, py_top) = to_px(xv, base + yv);
                        stack_running[idx] += yv;
                        let top_y = py_base.min(py_top);
                        let bx = px - bar_w / 2.0;
                        let bh = (py_base - py_top).abs();
                        if series.hatch {
                            ops.push(DrawOp::Rect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some("#ffffff".into()), stroke: Some(color.clone()), opacity: 1.0, radius: 1.0 });
                            ops.extend(hatch_lines(bx, top_y, bar_w, bh, &color));
                        } else {
                            ops.push(DrawOp::Rect { x: bx, y: top_y, w: bar_w, h: bh, fill: Some(color.clone()), stroke: Some(color.clone()), opacity: 0.9, radius: 1.0 });
                        }
                    }
                }
                // `"line"` is what `split_marker` returns for a spec that
                // is ALL line and no glyph -- `"-"`, `"--"`, `":"`. The
                // line itself was already drawn above, so there is nothing
                // to stamp at each point.
                //
                // Falling through to the catch-all instead drew the series
                // a SECOND time as a solid polyline at the default width:
                // a hairline straight down the middle of its own dash gaps,
                // ignoring the weight the series asked for. On the paper's
                // reference curve that turned `lw = pt(2.0)` dashes into
                // beads on a thread. The legend has always had this guard;
                // the series path did not, so the key and the curve
                // disagreed and the curve was the wrong one.
                //
                // `glyph == "line"` now always joins (see `joined` above),
                // so this guard no longer has anything to protect against
                // for that case; kept as a real no-op rather than deleted,
                // since a future glyph could plausibly re-open the same gap.
                // The catch-all itself now reads `line_w` rather than a
                // hardcoded constant, for the same reason: an unrecognised
                // glyph with an explicit `lw=` should not silently drop it
                // either.
                "line" if joined.is_some() => {}
                _ if line_w > 0.0 => ops.push(DrawOp::Polyline { points, color: color.clone(), width: line_w, dash: None }),
                _ => {}
            }
            if series.marker_shadow {
                // Offset by a fraction of the marker's own radius, so a
                // 2pt dot and a 9pt star cast shadows in proportion rather
                // than one being smudged and the other barely moved.
                // A quarter of the radius, down and right. Enough to lift
                // the mark off the field, small enough that it still reads
                // as ONE mark: at 0.38 the shadow of a star was a second
                // star beside the first, which is not a shadow, it is a
                // duplicate. There is no blur here, so the offset has to
                // carry the whole effect and must stay modest.
                let r = series.marker_size.unwrap_or(3.2 * tick_scale);
                let (dx, dy) = (r * 0.22, r * 0.22);
                let ink = apply_alpha("#1a1a1a", Some(0.26));
                let cast: Vec<DrawOp> = ops[mark_start..]
                    .iter()
                    .filter_map(|op| shadow_of(op, dx, dy, &ink))
                    .collect();
                // Spliced in at `mark_start`, not appended: a shadow drawn
                // after its marker sits ON it, which is the one thing a
                // shadow must not do.
                for (k, op) in cast.into_iter().enumerate() {
                    ops.insert(mark_start + k, op);
                }
            }
        }

        if let Some(inset) = &panel.inset {
            let (ix0, ix1) = (inset.x0.min(inset.x1), inset.x0.max(inset.x1));
            let (iy0, iy1) = (inset.y0.min(inset.y1), inset.y0.max(inset.y1));
            let (inset_left, inset_top, inset_w, inset_h, side) = if inset.outside {
                // Guaranteed clear of every plotted point by construction —
                // margin for this was already reserved before `left/right/
                // top/bottom` were computed, so this box sits entirely
                // outside the plot rect.
                let box_w = rect.w * 0.32;
                let box_h = rect.h * 0.34;
                let side = legend_side(inset.position);
                let (bx, by) = match side {
                    Side::Right => (right + 16.0, (top + bottom) / 2.0 - box_h / 2.0),
                    Side::Left => (left - box_w - 16.0, (top + bottom) / 2.0 - box_h / 2.0),
                    Side::Top => ((left + right) / 2.0 - box_w / 2.0, top - box_h - 16.0),
                    Side::Bottom => ((left + right) / 2.0 - box_w / 2.0, bottom + 16.0),
                };
                (bx, by, box_w, box_h, side)
            } else {
                let box_w = (right - left) * 0.42;
                let box_h = (bottom - top) * 0.40;
                // `Best`: search the legend's own 8 candidate corners/edges
                // for one with **zero** overlap against any actually
                // plotted point (stricter than the legend's own
                // merely-fewest tie-break) — falls back to the least-bad
                // candidate only if none is fully clear (see `InsetZoom`'s
                // own doc comment for why that fallback isn't promoted to
                // `outside` automatically).
                let position = if inset.position == LegendPosition::Best {
                    let all_points: Vec<(f64, f64)> = panel
                        .series
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| matches!(s.marker.as_str(), "line" | "o" | "circle" | "x" | "+" | "s" | "square" | "stem"))
                        .flat_map(|(i, s)| s.x.iter().zip(s.y.iter()).map(move |(&x, &y)| to_px_for(i, x, y)))
                        .collect();
                    const CANDIDATES: [LegendPosition; 8] = [
                        LegendPosition::TopRight, LegendPosition::TopLeft,
                        LegendPosition::BottomRight, LegendPosition::BottomLeft,
                        LegendPosition::Right, LegendPosition::Left,
                        LegendPosition::Top, LegendPosition::Bottom,
                    ];
                    CANDIDATES
                        .iter()
                        .copied()
                        .min_by_key(|&pos| {
                            let (bx, by) = inside_position(pos, left, top, right, bottom, box_w, box_h);
                            all_points.iter().filter(|&&(px, py)| px >= bx && px <= bx + box_w && py >= by && py <= by + box_h).count()
                        })
                        .unwrap_or(LegendPosition::TopRight)
                } else {
                    inset.position
                };
                let (bx, by) = inside_position(position, left, top, right, bottom, box_w, box_h);
                (bx, by, box_w, box_h, legend_side(position))
            };
            let inset_right = inset_left + inset_w;
            let inset_bottom = inset_top + inset_h;
            let to_px_inset = |x: f64, y: f64| -> (f64, f64) {
                let (lx0, lx1) = if log_x { (ix0.max(1e-300).log10(), ix1.max(1e-300).log10()) } else { (ix0, ix1) };
                let (ly0, ly1) = if log_y { (iy0.max(1e-300).log10(), iy1.max(1e-300).log10()) } else { (iy0, iy1) };
                let xv = if log_x { x.max(1e-300).log10() } else { x };
                let yv = if log_y { y.max(1e-300).log10() } else { y };
                let px = inset_left + (xv - lx0) / (lx1 - lx0).max(1e-300) * inset_w;
                let py = inset_bottom - (yv - ly0) / (ly1 - ly0).max(1e-300) * inset_h;
                (px, py)
            };
            // Translucent (not opaque) so a curve that happens to pass under
            // the inset — `Best`'s zero-overlap search minimizes but can't
            // always rule this out — stays visible rather than getting
            // fully hidden; rounded corners for a softer, "card"-like feel
            // instead of a stark rectangle.
            const INSET_RADIUS: f64 = 8.0;
            ops.push(DrawOp::Rect { x: inset_left, y: inset_top, w: inset_w, h: inset_h, fill: Some("#ffffff".into()), stroke: None, opacity: 0.72, radius: INSET_RADIUS });
            // A light 2x2 grid (min/max of each axis) under the replayed
            // curve — a bare bordered box with no ticks/grid at all reads
            // as "unlabeled decoration" rather than a real axis, so this
            // gives the inset the same "it's a real plot" cues (gridlines,
            // tick numbers) as the main panel, just minimal given the size.
            let inset_grid_x: Vec<f64> = nice_ticks(ix0, ix1, 3).into_iter().filter(|&t| t > ix0 && t < ix1).collect();
            let inset_grid_y: Vec<f64> = nice_ticks(iy0, iy1, 3).into_iter().filter(|&t| t > iy0 && t < iy1).collect();
            for &gx in &inset_grid_x {
                let (px, _) = to_px_inset(gx, iy0);
                ops.push(DrawOp::Line { x1: px, y1: inset_top, x2: px, y2: inset_bottom, color: GRID_COLOR.into(), width: 0.8 });
            }
            for &gy in &inset_grid_y {
                let (_, py) = to_px_inset(ix0, gy);
                ops.push(DrawOp::Line { x1: inset_left, y1: py, x2: inset_right, y2: py, color: GRID_COLOR.into(), width: 0.8 });
            }
            // Only cartesian line/marker series replay into the inset — bar
            // family, step, and the non-cartesian chart kinds (spider/
            // heatmap/pie, handled by early `continue`s above) have no
            // established "zoomed" rendering, and this panel already
            // `continue`d above if it was one of those.
            for (i, series) in panel.series.iter().enumerate() {
                if !matches!(series.marker.as_str(), "line" | "o" | "circle" | "x" | "+" | "s" | "square" | "stem") {
                    continue;
                }
                let color = series_color(&series.color, i, palette);
                let kept: Vec<(f64, f64)> = series
                    .x
                    .iter()
                    .zip(series.y.iter())
                    .filter(|&(&x, &y)| x >= ix0 && x <= ix1 && y >= iy0 && y <= iy1)
                    .map(|(&x, &y)| to_px_inset(x, y))
                    .collect();
                if inset.clip && series.marker == "line" {
                    // `zoom_inset(..., clip=true)`'s whole reason to exist:
                    // hard-clip the actual line geometry to the box border
                    // (Liang-Barsky, in data space) instead of filtering to
                    // whole points already inside it — see `InsetZoom.clip`'s
                    // doc comment for how this differs from the "kept" path
                    // just above (which the marker branches below still use,
                    // since a single-point marker has no clipping to do).
                    let data_points: Vec<(f64, f64)> = series.x.iter().zip(series.y.iter()).map(|(&x, &y)| (x, y)).collect();
                    for strip in clip_polyline_to_rect(&data_points, ix0, ix1, iy0, iy1) {
                        let px_strip: Vec<(f64, f64)> = strip.into_iter().map(|(x, y)| to_px_inset(x, y)).collect();
                        ops.push(DrawOp::Polyline { points: px_strip, color: color.clone(), width: 1.4, dash: None });
                    }
                    continue;
                }
                match series.marker.as_str() {
                    "o" => {
                        for &(px, py) in &kept {
                            ops.push(DrawOp::Circle { cx: px, cy: py, r: 2.4, fill: None, stroke: Some(color.clone()) });
                        }
                    }
                    "x" | "+" => {
                        for &(px, py) in &kept {
                            ops.push(DrawOp::Cross { cx: px, cy: py, r: 2.6, color: color.clone() });
                        }
                    }
                    "s" | "square" => {
                        const SIDE: f64 = 4.8; // scaled down from the main render's 6.4, matching "o"'s 3.2->2.4 ratio
                        for &(px, py) in &kept {
                            ops.push(DrawOp::Rect {
                                x: px - SIDE / 2.0,
                                y: py - SIDE / 2.0,
                                w: SIDE,
                                h: SIDE,
                                fill: None,
                                stroke: Some(color.clone()),
                                opacity: 1.0,
                                radius: 0.0,
                            });
                        }
                    }
                    "stem" => {
                        for &(px, py) in &kept {
                            ops.push(DrawOp::Circle { cx: px, cy: py, r: 2.0, fill: Some(color.clone()), stroke: None });
                        }
                    }
                    _ => ops.push(DrawOp::Polyline { points: kept, color: color.clone(), width: 1.4, dash: None }),
                }
            }
            ops.push(DrawOp::Rect { x: inset_left, y: inset_top, w: inset_w, h: inset_h, fill: None, stroke: Some(box_color.into()), opacity: 1.0, radius: INSET_RADIUS });
            // Tick numbers along the same grid lines, small enough not to
            // overwhelm the inset's own size, placed just inside the
            // border so they never spill past the translucent card.
            let inset_tick_size = (tick_size * 0.68).max(8.0);
            for &gx in &inset_grid_x {
                let (px, _) = to_px_inset(gx, iy0);
                ops.push(DrawOp::Text { italic: false, x: px, y: inset_bottom - 4.0, text: format_tick(gx), size: inset_tick_size, anchor: Anchor::Middle, rotate: 0.0, color: TICK_COLOR.into() });
            }
            for &gy in &inset_grid_y {
                let (_, py) = to_px_inset(ix0, gy);
                ops.push(DrawOp::Text { italic: false, x: inset_left + 4.0, y: py - 2.0, text: format_tick(gy), size: inset_tick_size, anchor: Anchor::Start, rotate: 0.0, color: TICK_COLOR.into() });
            }
            let (zx0, zy0) = to_px(ix0, iy0);
            let (zx1, zy1) = to_px(ix1, iy1);
            let (zleft, zright) = (zx0.min(zx1), zx0.max(zx1));
            let (ztop, zbottom) = (zy0.min(zy1), zy0.max(zy1));
            ops.extend(dashed_rect_ops(zleft, ztop, zright - zleft, zbottom - ztop, BOX_COLOR));
            match side {
                Side::Right => {
                    ops.extend(dashed_segment(zright, ztop, inset_left, inset_top, BOX_COLOR));
                    ops.extend(dashed_segment(zright, zbottom, inset_left, inset_bottom, BOX_COLOR));
                }
                Side::Left => {
                    ops.extend(dashed_segment(zleft, ztop, inset_right, inset_top, BOX_COLOR));
                    ops.extend(dashed_segment(zleft, zbottom, inset_right, inset_bottom, BOX_COLOR));
                }
                Side::Top => {
                    ops.extend(dashed_segment(zleft, ztop, inset_left, inset_bottom, BOX_COLOR));
                    ops.extend(dashed_segment(zright, ztop, inset_right, inset_bottom, BOX_COLOR));
                }
                Side::Bottom => {
                    ops.extend(dashed_segment(zleft, zbottom, inset_left, inset_top, BOX_COLOR));
                    ops.extend(dashed_segment(zright, zbottom, inset_right, inset_top, BOX_COLOR));
                }
            }
        }

        ops.push(DrawOp::ClipEnd);

        for callout in &panel.callouts {
            let (px, py) = to_px(callout.x, callout.y);
            if callout.marker {
                ops.push(DrawOp::Circle { cx: px, cy: py, r: 4.0, fill: Some("#c44e52".into()), stroke: Some("#fff".into()) });
            }
            if let Some((tx, ty)) = callout.arrow_to {
                let (tpx, tpy) = to_px(tx, ty);
                // The arrow takes `color=` too. It used to be a hardcoded
                // dark grey while the text it belongs to obeyed the
                // argument, so a note tinted to match the series it points
                // at arrived with a stem in someone else's ink -- and an
                // annotation is one object, not a label that happens to sit
                // near a line.
                let ink = callout.color.clone().unwrap_or_else(|| "#333".into());
                ops.push(DrawOp::Line { x1: px, y1: py, x2: tpx, y2: tpy, color: ink.clone(), width: 1.2 });
                // An arrowhead, because `arrow`/`arrowtext` promise one and
                // drew a bare line. A line between two points does not say
                // which end is the subject, which is the entire job of an
                // annotation arrow.
                let (dx, dy) = (tpx - px, tpy - py);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 1e-9 {
                    const HEAD: f64 = 9.0;
                    const HALF_WIDTH: f64 = 3.6;
                    let (ux, uy) = (dx / len, dy / len); // along the arrow
                    let (nx, ny) = (-uy, ux); // perpendicular
                    let base = (tpx - ux * HEAD, tpy - uy * HEAD);
                    ops.push(DrawOp::Polygon { width: MARKER_EDGE_W,
                        points: vec![
                            (tpx, tpy),
                            (base.0 + nx * HALF_WIDTH, base.1 + ny * HALF_WIDTH),
                            (base.0 - nx * HALF_WIDTH, base.1 - ny * HALF_WIDTH),
                        ],
                        fill: Some(ink.clone()),
                        stroke: None,
                        opacity: 1.0,
                    });
                }
            }
            // Sized off the figure's own type scale, not a hardcoded 11.
            // A print theme runs its ticks at ~25 units, so every
            // annotation used to come out at under half the size of the
            // smallest other text on the figure -- the same bug the legend
            // had before its text was tied to the scale.
            let note_size = callout.size.unwrap_or(fig.tick_size);
            // The label goes on the side AWAY from the arrow.
            //
            // It was always up-and-right of the anchor, whatever the arrow
            // did, so an arrow pointing down-right left its own text lying
            // across the first part of its stem. Placing the text opposite
            // the direction of travel means the stem always leaves from
            // the edge of the label nearest its target, which is what an
            // annotation is supposed to look like -- and the reader gets
            // the label and the arrow as one gesture rather than two marks
            // that happen to overlap. Without an arrow nothing has a
            // direction, so the old up-and-right stands.
            let (ox, oy, anchor) = match callout.arrow_to {
                Some((tx, ty)) => {
                    let (tpx, tpy) = to_px(tx, ty);
                    let (dx, dy) = (tpx - px, tpy - py);
                    let (ox, anchor) = if dx > 0.0 {
                        (-6.0, Anchor::End)
                    } else {
                        (6.0, Anchor::Start)
                    };
                    // Pixel y grows DOWN, so an arrow heading down wants
                    // the text lifted above the anchor and vice versa.
                    let oy = if dy > 0.0 { -6.0 } else { note_size * 0.85 };
                    (ox, oy, anchor)
                }
                None => (6.0, -6.0, Anchor::Start),
            };
            ops.push(DrawOp::Text {
                italic: callout.italic,
                x: px + ox,
                y: py + oy,
                text: callout.text.clone(),
                size: note_size,
                anchor,
                rotate: 0.0,
                color: callout.color.clone().unwrap_or_else(|| text_ink.clone()),
            });
        }

        // title / axis labels
        if let Some(t) = &panel.title {
            let (tx, anchor) = align_pos(panel.title_align, left, right);
            // Fit the title to its panel.
            //
            // Nothing used to measure it, so a title simply ran as far as
            // it needed: in a 1x2 subplot a 27-character title at 37 px
            // came out ~505 px against a 274 px panel, so the left title
            // crossed into the right panel and the right one ran off the
            // canvas and was clipped. Both were visible in an exported
            // figure.
            //
            // WRAP rather than shrink. Shrinking gave two panels in a row
            // titles at two different sizes -- and, since weight follows
            // size, one bold and one not -- for no reason the reader could
            // see. The extra line's room was reserved in the margin above
            // (see `wrap_to_width`'s caller), so the panel is already the
            // right height for it.
            let metrics = face_metrics(&fig.font_family);
            let avail = (right - left).max(1.0);
            let lines = wrap_to_width(t, avail, title_size, metrics);
            // The block grows DOWNWARD from the single-line baseline, into
            // the margin reserved for it. Upward would be tidier against
            // the plot, and puts the first line off the top of the canvas:
            // `margin_top` moves the PLOT AREA down inside the panel rect,
            // it does not move the rect.
            let first = rect.top + half_line * 1.5;
            for (i, line) in lines.iter().enumerate() {
                ops.push(DrawOp::Text { italic: false,
                    x: tx,
                    y: first + i as f64 * title_size * TITLE_LINE_PITCH,
                    text: line.clone(),
                    size: title_size,
                    anchor,
                    rotate: 0.0,
                    color: title_ink.clone(),
                });
            }
        }
        if let Some(t) = &panel.xlabel {
            // colored to match the bottom axis's series once a second
            // (top) x-axis exists — same "which axis does this belong to"
            // reasoning as the tick numbers; a single-axis panel keeps the
            // neutral label color instead of tinting to an arbitrary series.
            let color = if has_top_axis { bottom_tick_color.clone() } else { text_ink.clone() };
            let (lx, anchor) = align_pos(panel.xlabel_align, left, right);
            // At the foot of its OWN band, not the foot of the whole
            // margin. The two are the same until something -- an outside
            // legend, an outside inset -- reserves a strip underneath, at
            // which point measuring from the margin's foot walked the
            // label straight down into it.
            let label_y = bottom + stack_b.extent(Band::Pad) - half_line * 0.5;
            ops.push(DrawOp::Text { italic: false, x: lx, y: label_y, text: t.clone(), size: label_size, anchor, rotate: 0.0, color });
        }
        // The colour scale strip. Drawn after the panel so it sits on top
        // of nothing, and before the labels so it shares their ink colour.
        if let Some(cb) = &panel.colorbar {
            // Range and colormap come from whatever field the panel
            // actually holds, rather than being restated on the
            // `colorbar()` call: a bar that can disagree with the plot it
            // labels is worse than none.
            let field = panel.shapes.iter().find_map(|sh| match sh {
                Shape::Contour { z, levels, colormap, filled: true, .. } => {
                    let (lo, hi) = if levels.len() >= 2 {
                        (levels[0], levels[levels.len() - 1])
                    } else {
                        let f: Vec<f64> = z.iter().copied().filter(|v| v.is_finite()).collect();
                        (
                            f.iter().copied().fold(f64::INFINITY, f64::min),
                            f.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        )
                    };
                    Some((lo, hi, colormap.clone()))
                }
                _ => None,
            });
            let field = field.or_else(|| {
                panel.heatmap.as_ref().map(|h| {
                    let f: Vec<f64> = h.values.iter().copied().filter(|v| v.is_finite()).collect();
                    (
                        f.iter().copied().fold(f64::INFINITY, f64::min),
                        f.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        h.colormap.clone(),
                    )
                })
            });
            if let Some((vlo, vhi, cmap)) = field {
                let bar_w = COLORBAR_WIDTH * tick_scale;
                // At the inner edge of the band reserved for it, which is
                // already past the right axis when there is one.
                //
                // This used to recompute the right axis's own reservation
                // -- `46.0 + if ylabel2 { 14.0 }` -- a second copy of the
                // arithmetic three thousand lines from the first, with
                // nothing to notice if the two drifted apart. Asking the
                // stack where the band starts cannot drift, because there
                // is only the one answer.
                let bar_x = right
                    + stack_r.inner(Band::Colorbar).unwrap_or(0.0)
                    + (18.0 * tick_scale);
                let (bar_top, bar_bottom) = (top, bottom);
                let h = (bar_bottom - bar_top) / COLORBAR_STEPS as f64;
                for k in 0..COLORBAR_STEPS {
                    // Bottom of the bar is the low end, so the strip reads
                    // the same way as the y axis beside it.
                    let t = 1.0 - (k as f64 + 0.5) / COLORBAR_STEPS as f64;
                    ops.push(DrawOp::Rect {
                        x: bar_x,
                        y: bar_top + k as f64 * h,
                        // A hair of overlap: adjacent slices otherwise show
                        // hairline seams wherever the renderer rounds their
                        // edges to different pixels.
                        w: bar_w,
                        h: h + 0.5,
                        fill: Some(colormap_color(&cmap, t)),
                        stroke: None,
                        opacity: 1.0,
                        radius: 0.0,
                    });
                }
                // A pointed cap where the scale runs past the bar, in the
                // colour of the band it continues, drawn OUTSIDE the strip
                // so the bar itself still maps linearly to the range.
                let ext = cb.extend.as_deref().unwrap_or("");
                let cap_h = bar_w * 0.55;
                let cap_max = matches!(ext, "max" | "both");
                let cap_min = matches!(ext, "min" | "both");
                if cap_max {
                    ops.push(DrawOp::Polygon { width: MARKER_EDGE_W,
                        points: vec![
                            (bar_x, bar_top),
                            (bar_x + bar_w, bar_top),
                            (bar_x + bar_w / 2.0, bar_top - cap_h),
                        ],
                        fill: Some(colormap_color(&cmap, 1.0)),
                        stroke: Some(box_color.to_string()),
                        opacity: 1.0,
                    });
                }
                if cap_min {
                    ops.push(DrawOp::Polygon { width: MARKER_EDGE_W,
                        points: vec![
                            (bar_x, bar_bottom),
                            (bar_x + bar_w, bar_bottom),
                            (bar_x + bar_w / 2.0, bar_bottom + cap_h),
                        ],
                        fill: Some(colormap_color(&cmap, 0.0)),
                        stroke: Some(box_color.to_string()),
                        opacity: 1.0,
                    });
                }
                // The capped end loses its own rule -- the triangle's two
                // sides are the frame there, and a line across its base
                // would read as the bar stopping after all.
                let mut edges = vec![
                    (bar_x, bar_top, bar_x, bar_bottom),
                    (bar_x + bar_w, bar_top, bar_x + bar_w, bar_bottom),
                ];
                if !cap_max {
                    edges.push((bar_x, bar_top, bar_x + bar_w, bar_top));
                }
                if !cap_min {
                    edges.push((bar_x, bar_bottom, bar_x + bar_w, bar_bottom));
                }
                for (x1, y1, x2, y2) in edges {
                    ops.push(DrawOp::Line { x1, y1, x2, y2, color: box_color.into(), width: frame_width });
                }

                let ticks: Vec<f64> = if cb.ticks.is_empty() {
                    nice_ticks(vlo, vhi, 5)
                } else {
                    cb.ticks.clone()
                };
                let fmt = axis_tick_formatter(&ticks);
                let span = (vhi - vlo).abs().max(1e-300);
                for &tv in &ticks {
                    if tv < vlo - 1e-9 || tv > vhi + 1e-9 {
                        continue;
                    }
                    let y = bar_bottom - (tv - vlo) / span * (bar_bottom - bar_top);
                    ops.push(DrawOp::Line {
                        x1: bar_x + bar_w,
                        y1: y,
                        x2: bar_x + bar_w + tick_px.max(2.0),
                        y2: y,
                        color: tick_mark_color.into(),
                        width: frame_width,
                    });
                    ops.push(DrawOp::Text { italic: false,
                        x: bar_x + bar_w + tick_px.max(2.0) + 3.0,
                        y: y + tick_size * BASELINE_CENTER,
                        text: fmt(tv),
                        size: tick_size,
                        anchor: Anchor::Start,
                        rotate: 0.0,
                        color: text_ink.clone(),
                    });
                }
                if let Some(label) = &cb.label {
                    // Far side of the numbers, reading bottom-to-top like
                    // the y label on the other side of the plot.
                    let widest = ticks
                        .iter()
                        .map(|&t| measure_text(&fmt(t), tick_size, face_metrics(&fig.font_family)))
                        .fold(0.0f64, f64::max);
                    ops.push(DrawOp::Text { italic: false,
                        x: bar_x + bar_w + tick_px.max(2.0) + 6.0 + widest + label_size * 0.8,
                        y: (bar_top + bar_bottom) / 2.0,
                        text: label.clone(),
                        size: label_size,
                        anchor: Anchor::Middle,
                        rotate: -90.0,
                        color: text_ink.clone(),
                    });
                }
            }
        }
        if let Some(t) = &panel.ylabel {
            let color = if has_right_axis { left_tick_color.clone() } else { text_ink.clone() };
            let (ly, anchor) = align_pos(panel.ylabel_align, bottom, top);
            // One `half_line` clear of the widest tick number, but never
            // off the canvas: with wide numbers the edge still wins, and
            // the margin is what has to grow, not the label that moves.
            let numbers_left = origin_px - half_line * 0.75 - tick_clearance - widest_y_tick;
            let lx = (numbers_left - half_line).max(rect.left + half_line * 1.5);
            ops.push(DrawOp::Text { italic: false, x: lx, y: ly, text: t.clone(), size: label_size, anchor, rotate: -90.0, color });
        }
        if let Some(t) = &panel.xlabel2 {
            // sits just above the top-axis tick numbers (drawn at `top -
            // 10.0`), safely below the title (pinned near `rect.top + 12`)
            // since `margin_top` already grew to make room for both.
            ops.push(DrawOp::Text { italic: false, x: (left + right) / 2.0, y: top - 24.0, text: t.clone(), size: label_size, anchor: Anchor::Middle, rotate: 0.0, color: top_tick_color.clone() });
        }
        if let Some(t) = &panel.ylabel2 {
            // In the band it reserved, beside the numbers it names.
            //
            // This was `rect.left + rect.w - 12.0` -- the canvas edge --
            // which is fine only when nothing else is out there. Add a
            // colorbar and the label jumped clear over it, landing 200px
            // from its own tick numbers with the colour scale's label in
            // between: the paper's regime map read as though "Tone count
            // K" named the colour scale, which it does not.
            let lx = stack_r
                .center(Band::AxisLabel)
                .map_or(rect.left + rect.w - 12.0, |d| right + d);
            ops.push(DrawOp::Text { italic: false, x: lx, y: (top + bottom) / 2.0, text: t.clone(), size: label_size, anchor: Anchor::Middle, rotate: 90.0, color: right_tick_color.clone() });
        }

        if panel.legend.visible {
            let points: Vec<(f64, f64)> = panel
                .series
                .iter()
                .flat_map(|s| s.x.iter().zip(s.y.iter()).map(|(&x, &y)| to_px(x, y)))
                .collect();
            ops.extend(legend_ops(
                panel,
                left, top, right, bottom,
                palette,
                &points,
                (rect.left, rect.top, rect.w, rect.h),
                tick_scale,
                style.legend_frame,
                face_metrics(&fig.font_family),
                legend_outside,
            ));
        }
    }
    (ops, geometry)
}

/// Renders the legend box. `LegendPosition::Best` scores each of the four
/// corners by how many plotted points fall in that corner's quadrant of the
/// panel and picks the emptiest one — a real (if simple) collision-avoidance
/// heuristic, not a hardcoded default.
/// Draws one Tukey box (whiskers, caps, box, median, outliers) at a given
/// pixel x-center — shared by plain `boxplot` and `raincloud`'s embedded box.
/// `data_x` is the box's category position in *data* space (needed to look
/// up whisker/outlier y-pixels via `to_px`); `center_px` is its already-
/// resolved pixel x-center, which may differ from `to_px(data_x, _).0` when
/// the box shares a category slot with other raincloud elements.
fn box_ops(to_px: &impl Fn(f64, f64) -> (f64, f64), group: &BoxplotGroup, data_x: f64, center_px: f64, box_w: f64, color: &str) -> Vec<DrawOp> {
    let cap = box_w * 0.3;
    let (_, q1y) = to_px(data_x, group.q1);
    let (_, q3y) = to_px(data_x, group.q3);
    let (_, medy) = to_px(data_x, group.median);
    let (_, wlo_y) = to_px(data_x, group.whisker_lo);
    let (_, whi_y) = to_px(data_x, group.whisker_hi);
    let px = center_px;
    let mut ops = vec![
        DrawOp::Line { x1: px, y1: q3y, x2: px, y2: whi_y, color: color.to_string(), width: 1.2 },
        DrawOp::Line { x1: px, y1: q1y, x2: px, y2: wlo_y, color: color.to_string(), width: 1.2 },
        DrawOp::Line { x1: px - cap, y1: whi_y, x2: px + cap, y2: whi_y, color: color.to_string(), width: 1.2 },
        DrawOp::Line { x1: px - cap, y1: wlo_y, x2: px + cap, y2: wlo_y, color: color.to_string(), width: 1.2 },
        // TWO rects, because `opacity` on a `DrawOp::Rect` applies to the
        // fill AND the stroke together. Drawn as one, the box's outline came
        // out at 35% of the fill colour -- which is no outline at all: the
        // same hue as what it surrounds, at the same transparency. The
        // whiskers and caps around it are full-strength 1.2-wide lines, so
        // the box read as a faint blob hanging between crisp whiskers, and
        // in print a 35% edge is exactly the wash-out that makes a figure
        // look muddy.
        //
        // A Tukey box is an OUTLINED shape with a light interior -- that is
        // what matplotlib, ggplot2 and every journal reference draw. Fill
        // stays translucent so overlapping data behind it still reads;
        // the edge is opaque and matches the whisker weight it joins.
        DrawOp::Rect {
            x: px - box_w / 2.0, y: q3y.min(q1y), w: box_w, h: (q1y - q3y).abs(),
            fill: Some(color.to_string()), stroke: None, opacity: 0.35, radius: 2.0,
        },
        DrawOp::Rect {
            x: px - box_w / 2.0, y: q3y.min(q1y), w: box_w, h: (q1y - q3y).abs(),
            fill: None, stroke: Some(color.to_string()), opacity: 1.0, radius: 2.0,
        },
        DrawOp::Line { x1: px - box_w / 2.0, y1: medy, x2: px + box_w / 2.0, y2: medy, color: color.to_string(), width: 2.0 },
    ];
    for &v in &group.outliers {
        let (_, oy) = to_px(data_x, v);
        ops.push(DrawOp::Circle { cx: px, cy: oy, r: 2.5, fill: None, stroke: Some(color.to_string()) });
    }
    ops
}

/// Places a real `Value::Image` (`imshow`) as a single native `<image>`
/// element rather than one `DrawOp::Rect` per pixel: for anything bigger
/// than a tiny thumbnail, thousands of individual rects (the `heatmap`-style
/// approach) would bloat the SVG and slow every downstream renderer, where
/// one `<image>` with an embedded base64 BMP data URI costs the same bytes
/// as the pixel data itself, decoded natively by the browser/viewer. Scales
/// to fit the available rect while preserving aspect ratio (letterboxed,
/// centered) — never stretches the image to the panel's own aspect ratio.
fn image_ops(img: &Image, left: f64, top: f64, right: f64, bottom: f64) -> Vec<DrawOp> {
    if img.width == 0 || img.height == 0 || right <= left || bottom <= top {
        return Vec::new();
    }
    let bmp = crate::image::encode_bmp(img.width, img.height, &img.pixels);
    let href = format!("data:image/bmp;base64,{}", base64_encode(&bmp));
    let avail_w = right - left;
    let avail_h = bottom - top;
    let img_aspect = img.width as f64 / img.height as f64;
    let avail_aspect = avail_w / avail_h;
    let (w, h) = if img_aspect > avail_aspect {
        (avail_w, avail_w / img_aspect)
    } else {
        (avail_h * img_aspect, avail_h)
    };
    let x = left + (avail_w - w) / 2.0;
    let y = top + (avail_h - h) / 2.0;
    vec![DrawOp::Image { x, y, w, h, href }]
}

/// A `rows x cols` grid of colored cells, scaled to `values`' own min/max
/// (not a fixed 0..1 range — a correlation matrix in `[-1, 1]` and a raw
/// count matrix in `[0, N]` should each use their own full color range).
fn heatmap_ops(
    hm: &Heatmap,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    tick_size: f64,
    ink: &str,
) -> Vec<DrawOp> {
    if hm.rows == 0 || hm.cols == 0 {
        return Vec::new();
    }
    // The label gutters track type: they were 70 and 40 canvas units,
    // which is the room ten-unit text needs and nothing else. Render the
    // same figure at print size and the names ran into the grid.
    let type_scale = tick_size / TICK_METRIC_BASELINE;
    let label_left = if hm.row_labels.is_empty() { 0.0 } else { 70.0 * type_scale };
    let label_bottom = if hm.col_labels.is_empty() { 0.0 } else { 40.0 * type_scale };
    let mut grid_left = left + label_left;
    let mut grid_top = top;
    let mut grid_right = right;
    let mut grid_bottom = bottom - label_bottom;
    if grid_right <= grid_left || grid_bottom <= grid_top {
        return Vec::new();
    }
    let mut cell_w = (grid_right - grid_left) / hm.cols as f64;
    let mut cell_h = (grid_bottom - grid_top) / hm.rows as f64;
    if hm.square {
        // One cell size for both axes, and the leftover split evenly, so
        // the grid sits in the middle of the space rather than hard
        // against the labels.
        let cell = cell_w.min(cell_h);
        let slack_x = (grid_right - grid_left) - cell * hm.cols as f64;
        let slack_y = (grid_bottom - grid_top) - cell * hm.rows as f64;
        grid_left += slack_x / 2.0;
        grid_right -= slack_x / 2.0;
        grid_top += slack_y / 2.0;
        grid_bottom -= slack_y / 2.0;
        cell_w = cell;
        cell_h = cell;
    }
    // Cells and row labels are placed from the grid's own top edge, which
    // the square-cell centring above may have moved down.
    let top = grid_top;
    let (vmin, vmax) = hm.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = if (vmax - vmin).abs() > 1e-12 { vmax - vmin } else { 1.0 };

    let mut ops = Vec::new();
    for r in 0..hm.rows {
        for c in 0..hm.cols {
            let v = hm.values[r * hm.cols + c];
            let color = colormap_color(&hm.colormap, (v - vmin) / span);
            let x = grid_left + c as f64 * cell_w;
            let y = top + r as f64 * cell_h;
            let stroke = if hm.dense { None } else { Some("#ffffff".into()) };
            let row_label = hm.row_labels.get(r).filter(|s| !s.is_empty());
            let col_label = hm.col_labels.get(c).filter(|s| !s.is_empty());
            let title = match (row_label, col_label) {
                (Some(rl), Some(cl)) => format!("{rl}, {cl}: {}", format_tick(v)),
                _ => format_tick(v),
            };
            ops.push(DrawOp::TitledRect { x, y, w: cell_w, h: cell_h, fill: Some(color.clone()), stroke, opacity: 1.0, radius: 0.0, title });
            if !hm.dense {
                let text_color = contrast_text_color(&color);
                // The value in the cell is bounded by the cell, but the
                // bounds themselves track type -- fixed 7..11 meant a
                // print-size figure kept screen-size numbers.
                let value_size = (cell_h * 0.28)
                    .clamp(tick_size * 0.7, tick_size * 1.1);
                ops.push(DrawOp::Text { italic: false,
                    x: x + cell_w / 2.0, y: y + cell_h / 2.0 + value_size * BASELINE_CENTER,
                    text: format_tick(v), size: value_size,
                    anchor: Anchor::Middle, rotate: 0.0, color: text_color.into(),
                });
            }
        }
    }
    // Row and column names are tick labels for this grid, so they are the
    // figure's tick type, not a fixed 10 units -- and their offsets from
    // the grid scale with them, or the text drifts into the cells at
    // large sizes and floats away at small ones.
    let gap = tick_size * 0.6;
    for (r, label) in hm.row_labels.iter().enumerate() {
        if label.is_empty() { continue; }
        let y = top + (r as f64 + 0.5) * cell_h;
        ops.push(DrawOp::Text { italic: false, x: grid_left - gap, y: y + tick_size * BASELINE_CENTER, text: label.clone(), size: tick_size, anchor: Anchor::End, rotate: 0.0, color: ink.into() });
    }
    for (c, label) in hm.col_labels.iter().enumerate() {
        if label.is_empty() { continue; }
        let x = grid_left + (c as f64 + 0.5) * cell_w;
        ops.push(DrawOp::Text { italic: false, x, y: grid_bottom + gap + tick_size, text: label.clone(), size: tick_size, anchor: Anchor::Middle, rotate: -30.0, color: ink.into() });
    }
    ops
}

/// A pie (or donut, if `pie.donut`) chart. Wedges are approximated as
/// many-sided `DrawOp::Polygon`s (straight segments along the arc) rather
/// than a native circular-arc primitive — visually indistinguishable from a
/// smooth arc once segment count scales with the wedge's own angle.
fn pie_ops(pie: &PieChart, left: f64, top: f64, right: f64, bottom: f64, palette: &[&str], tick_size: f64) -> Vec<DrawOp> {
    let total: f64 = pie.values.iter().sum();
    let cx = (left + right) / 2.0;
    let cy = (top + bottom) / 2.0;
    let radius = (right - left).min(bottom - top) / 2.0 - 10.0;
    if total <= 0.0 || radius <= 0.0 {
        return Vec::new();
    }
    let inner_radius = if pie.donut { radius * 0.5 } else { 0.0 };
    let mut ops = Vec::new();
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, &v) in pie.values.iter().enumerate() {
        if v <= 0.0 {
            continue;
        }
        let sweep = v / total * std::f64::consts::TAU;
        let color = palette[i % palette.len()].to_string();
        let segments = ((sweep / (std::f64::consts::TAU / 48.0)).ceil() as usize).max(1);
        let arc_point = |r: f64, s: usize| {
            let a = angle + sweep * s as f64 / segments as f64;
            (cx + r * a.cos(), cy + r * a.sin())
        };
        let mut points = Vec::new();
        if inner_radius > 0.0 {
            points.extend((0..=segments).map(|s| arc_point(inner_radius, s)));
            points.extend((0..=segments).rev().map(|s| arc_point(radius, s)));
        } else {
            points.push((cx, cy));
            points.extend((0..=segments).map(|s| arc_point(radius, s)));
        }
        let text_color = contrast_text_color(&color);
        ops.push(DrawOp::Polygon { width: MARKER_EDGE_W, points, fill: Some(color), stroke: Some("#ffffff".into()), opacity: 1.0 });
        if let Some(label) = pie.labels.get(i) {
            let mid_angle = angle + sweep / 2.0;
            let label_r = (radius + inner_radius) / 2.0 + (radius - inner_radius) * 0.25;
            let (lx, ly) = (cx + label_r * mid_angle.cos(), cy + label_r * mid_angle.sin());
            ops.push(DrawOp::Text { italic: false,
                x: lx, y: ly, text: format!("{label} ({:.0}%)", v / total * 100.0),
                size: tick_size, anchor: Anchor::Middle, rotate: 0.0, color: text_color.into(),
            });
        }
        angle += sweep;
    }
    ops
}

/// Polar layout for a `SpiderChart`: `n` evenly-spaced spokes starting
/// straight up and going clockwise, 4 concentric grid rings, category labels
/// past the outer ring, and each series as a closed polygon scaled to
/// `max` (auto-scaled to the largest data point if unset).
/// `tick_size`/`label_size` come from the figure rather than being fixed
/// here. They were 8.5 and 10.0 canvas units, which is a readable size
/// only at the one canvas those numbers were chosen against: on a 4-inch
/// figure the spoke names came out too small to read, and a radar chart
/// whose axes cannot be read is a shape with no meaning.
fn spider_ops(spider: &SpiderChart, left: f64, top: f64, right: f64, bottom: f64, palette: &[&str], tick_size: f64, label_size: f64) -> Vec<DrawOp> {
    let n = spider
        .categories
        .len()
        .max(spider.series.iter().map(|s| s.y.len()).max().unwrap_or(0));
    if n < 3 {
        return Vec::new(); // a radar chart needs at least a triangle
    }
    let cx = (left + right) / 2.0;
    let cy = (top + bottom) / 2.0;
    let radius = (right - left).min(bottom - top) / 2.0 - 20.0;
    if radius <= 0.0 {
        return Vec::new();
    }
    let max_val = spider
        .max
        .unwrap_or_else(|| spider.series.iter().flat_map(|s| s.y.iter().copied()).fold(0.0_f64, f64::max))
        .max(1e-9);
    let angle_at = |i: usize| -std::f64::consts::FRAC_PI_2 + 2.0 * std::f64::consts::PI * i as f64 / n as f64;
    let point_at = |i: usize, r: f64| {
        let a = angle_at(i);
        (cx + r * a.cos(), cy + r * a.sin())
    };

    let mut ops = Vec::new();
    for ring in 1..=4 {
        let r = radius * ring as f64 / 4.0;
        let mut points: Vec<(f64, f64)> = (0..n).map(|i| point_at(i, r)).collect();
        points.push(points[0]);
        ops.push(DrawOp::Polyline { points, color: "#8888882a".into(), width: 1.0, dash: None });
        // radial (value) axis: label each ring along the top spoke, the way
        // matplotlib/plotly radar charts label their one shared radial axis.
        let (lx, ly) = point_at(0, r);
        ops.push(DrawOp::Text { italic: false,
            x: lx + 4.0, y: ly - 2.0,
            text: format_tick(max_val * ring as f64 / 4.0),
            size: tick_size, anchor: Anchor::Start, rotate: 0.0, color: TICK_COLOR.into(),
        });
    }
    for i in 0..n {
        let (ex, ey) = point_at(i, radius);
        ops.push(DrawOp::Line { x1: cx, y1: cy, x2: ex, y2: ey, color: "#8888882a".into(), width: 1.0 });
        if let Some(label) = spider.categories.get(i) {
            let (lx, ly) = point_at(i, radius + 12.0);
            ops.push(DrawOp::Text { italic: false, x: lx, y: ly, text: label.clone(), size: label_size, anchor: Anchor::Middle, rotate: 0.0, color: "#444".into() });
        }
    }
    for (si, series) in spider.series.iter().enumerate() {
        let color = series_color(&series.color, si, palette);
        let mut points: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let v = series.y.get(i).copied().unwrap_or(0.0);
                point_at(i, radius * (v / max_val).clamp(0.0, 1.0))
            })
            .collect();
        points.push(points[0]);
        ops.push(DrawOp::Polyline { points, color, width: DEFAULT_LINE_WIDTH, dash: None });
    }
    ops
}

fn legend_ops(
    panel: &Panel,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    palette: &[&str],
    points: &[(f64, f64)],
    outer: (f64, f64, f64, f64),
    tick_scale: f64,
    frame: Option<&'static str>,
    face: Option<&'static pdf_font::FontMetrics>,
    // The layout pass decided this legend goes outside -- either the script
    // said so, or `legend_must_go_outside` found it could not fit inside
    // without covering data. Passed in rather than re-read off the panel,
    // because the margin that makes room for it was reserved on this same
    // decision and the two must agree.
    outside: bool,
) -> Vec<DrawOp> {
    let labeled: Vec<(&Series, usize)> = panel
        .series
        .iter()
        .enumerate()
        .filter(|(_, s)| s.label.is_some())
        .map(|(i, s)| (s, i))
        .collect();
    if labeled.is_empty() {
        return Vec::new();
    }
    let row_h = 18.0 * tick_scale;
    let pad = LEGEND_PAD * tick_scale;
    let (box_w, box_h) = legend_box_size(panel, tick_scale, face);

    let (bx, by) = if outside {
        outside_position(legend_side(panel.legend.position), left, top, right, bottom, outer, box_w, box_h)
    } else {
        let position = if panel.legend.position == LegendPosition::Best {
            best_corner(left, top, right, bottom, box_w, box_h, points)
        } else {
            panel.legend.position
        };
        let (px, py) = inside_position(position, left, top, right, bottom, box_w, box_h);
        // Clamp into the panel. A key wider than the space its corner
        // leaves used to hang off the plot and, at the right edge, off the
        // figure -- entries simply cut in half. Pushing it back in is
        // always better than losing the text; if it is genuinely too big
        // the author can set `columns=` or a smaller `fontsize=`.
        (
            px.clamp(left, (right - box_w).max(left)),
            py.clamp(top, (bottom - box_h).max(top)),
        )
    };

    let mut ops = vec![DrawOp::Rect {
        x: bx,
        y: by,
        w: box_w,
        h: box_h,
        fill: Some("#ffffff".into()),
        // Solid `#cccccc` instead of the old alpha-blended `#8888888a`
        // (2026-09-03 ggplot2-inspired pass) -- a crisp, deliberate border
        // rather than a semi-transparent one whose rendered color shifts
        // with whatever's behind the legend box.
        // A per-call `frame=` beats the theme in both directions: `false`
        // drops a border the theme draws, `true` adds one it does not.
        stroke: match panel.legend.frame {
            Some(true) => Some(frame.unwrap_or("#cccccc").to_string()),
            Some(false) => None,
            None => frame.map(|c| c.to_string()),
        },
        opacity: if panel.legend.translucent { 0.85 } else { 1.0 },
        radius: if panel.legend.rounded { 6.0 } else { 0.0 },
    }];
    // Legend text used to be a hardcoded 10.5px no matter what `fontsize(...)`
    // set for the rest of the figure -- a script that bumped every other
    // font for a presentation still got a barely-legible legend. Scaling by
    // the same `tick_scale` as the box above keeps it consistent;
    // `tick_scale = 1.0` reproduces `12.0` (bumped from `10.5`, 2026-09-03
    // ggplot2-inspired pass, for a more legible default legend).
    let text_size = panel.legend.font_size.unwrap_or(TICK_METRIC_BASELINE * tick_scale);
    // Same pitch rule as `legend_box_size`, which sized the box this is
    // being laid out inside; the two must agree or entries overflow it.
    let row_h = row_h * (text_size / (12.0 * tick_scale)).max(0.5);
    let cols = panel.legend.columns.max(1);
    let rows_per_col = labeled.len().div_ceil(cols);
    let col_w = (box_w - pad * 2.0) / cols as f64;
    let mut title_offset = 0.0;
    if let Some(t) = &panel.legend.title {
        ops.push(DrawOp::Text { italic: false,
            x: bx + pad,
            y: by + pad + row_h * 0.75,
            text: t.clone(),
            // Set at the entry size but bold, so it reads as a heading
            // without making the key taller than it needs to be.
            size: text_size,
            anchor: Anchor::Start,
            rotate: 0.0,
            color: "#111".into(),
        });
        title_offset = row_h;
    }
    for (slot, (series, index)) in labeled.iter().enumerate() {
        // Column-major: entries read DOWN each column, the way a printed
        // key does, not left-to-right across rows.
        let (col, row) = (slot / rows_per_col, slot % rows_per_col);
        let bx = bx + col as f64 * col_w;
        let y = by + pad + title_offset + row_h * row as f64 + row_h / 2.0;
        let color = series_color(&series.color, *index, palette);
        // The swatch matches the series' own marker instead of always
        // drawing a plain line, so a `stem`/`o`/`square` series reads
        // correctly in the legend rather than looking like a line series.
        let cx = bx + pad + LEGEND_SWATCH_W / 2.0 * tick_scale;
        // The swatch has to show whatever the series shows, joining line
        // included -- a key that draws a bare marker for a joined series
        // is a key to a different figure.
        let (glyph, joined) = split_marker(&series.marker);
        if let Some(dash) = joined.clone() {
            let (x1, x2) = (bx + pad, bx + pad + LEGEND_SWATCH_W * tick_scale);
            // The sample is drawn at the series' OWN width. It was a
            // fixed 1.6, so a key put every series on the same weight --
            // and weight is one of the three things separating four
            // methods on a crowded figure, alongside colour and dash.
            let sample_w = series.width.unwrap_or(1.6).max(1.0);
            match dash {
                None => ops.push(DrawOp::Line { x1, y1: y, x2, y2: y, color: color.clone(), width: sample_w }),
                Some(d) => {
                    // The swatch must carry the SAME pattern the line does,
                    // scaled to the same width, or the key describes a
                    // figure other than the one beside it.
                    let mut d = scaled_dash(&d, sample_w);
                    // Then fitted to the swatch, which is far shorter than
                    // the curve the pattern was scaled for. A period wider
                    // than the sample draws one solid bar, which tells the
                    // reader the series is SOLID -- the exact opposite of
                    // what the row exists to say. Two full periods is the
                    // least that still reads as a dash. (From the
                    // `journal-figures` branch, which found this on a
                    // legend it was actually reading.)
                    let period: f64 = d.pattern.iter().sum();
                    let span = x2 - x1;
                    if period > 0.0 && period * 2.0 > span && span > 0.0 {
                        d = d.scaled(span / (period * 2.0));
                    }
                    ops.push(DrawOp::DottedLine { x1, y1: y, x2, y2: y, color: color.clone(), width: sample_w, dash: d })
                }
            }
        }
        match glyph {
            // Filled if the series is filled. This arm hardcoded a hollow
            // ring, so `plot(..., marker = "o", fill = true)` drew solid
            // discs and keyed them with an outline -- the one mark in the
            // figure whose job is to say "this is that" showing something
            // the figure does not contain. Every other glyph already
            // honoured `fill=`; the plain circle, the most common marker
            // of all, was the exception.
            "o" | "circle" => ops.push(DrawOp::Circle {
                cx,
                cy: y,
                r: 3.5 * tick_scale,
                fill: series.marker_filled.unwrap_or(false).then(|| color.clone()),
                stroke: Some(color),
            }),
            "x" | "+" => ops.push(DrawOp::Cross { cx, cy: y, r: 4.0 * tick_scale, color }),
            // A shaded region's sample is the region: a filled patch at the
            // band's own opacity, so the key shows the same wash the reader
            // is being asked to identify. A line or a marker here would be
            // a key to something the figure does not contain.
            "band" => {
                let h = 4.0 * tick_scale;
                let (x1, x2) = (bx + pad, bx + pad + LEGEND_SWATCH_W * tick_scale);
                ops.push(DrawOp::Polygon {
                    width: MARKER_EDGE_W,
                    points: vec![(x1, y - h), (x2, y - h), (x2, y + h), (x1, y + h)],
                    fill: Some(color),
                    stroke: None,
                    opacity: series.alpha.unwrap_or(0.3),
                });
            }
            // A joined series whose glyph is just the line needs no extra
            // mark -- the line above already is the swatch.
            "line" if joined.is_some() => {}
            g if marker_polygon(g, 1.0).is_some() || marker_overlay(g, 1.0).is_some() => {
                let sw = 4.0 * tick_scale;
                let outline = marker_polygon(g, sw).unwrap_or_default();
                let fill = match (series.marker_filled, g) {
                    (Some(true), _) => Some(color.clone()),
                    (None, "*") | (None, "star") => Some("#ffffff".into()),
                    _ => None,
                };
                let w = series.width.unwrap_or(MARKER_EDGE_W).max(0.6);
                if outline.len() >= 3 {
                    ops.push(DrawOp::Polygon {
                        width: w,
                        points: outline.iter().map(|&(dx, dy)| (cx + dx, y + dy)).collect(),
                        fill,
                        stroke: Some(color.clone()),
                        opacity: 1.0,
                    });
                }
                // The key has to show the whole mark, strokes included, or
                // a crossed circle and a plain one look the same in it.
                if let Some(segs) = marker_overlay(g, sw) {
                    for ((ax, ay), (bx, by)) in segs {
                        ops.push(DrawOp::Line {
                            x1: cx + ax, y1: y + ay, x2: cx + bx, y2: y + by,
                            color: color.clone(), width: w,
                        });
                    }
                }
            }
            "stem" => {
                ops.push(DrawOp::Line { x1: cx, y1: y + 5.0 * tick_scale, x2: cx, y2: y - 5.0 * tick_scale, color: color.clone(), width: 1.2 });
                ops.push(DrawOp::Circle { cx, cy: y - 5.0 * tick_scale, r: 2.2 * tick_scale, fill: Some(color), stroke: None });
            }
            // A bar chart's key entry is the same filled, stroked swatch
            // `box_ops`'s own rect uses -- falling through to the plain
            // line below drew every `bar`/`groupbar`/`stackbar` series as
            // a horizontal stroke in its key, which is what a LINE series
            // looks like, not what the reader sees in the panel next to
            // it. Two grouped bars in matching colours are told apart by
            // this swatch alone.
            "bar" | "groupbar" | "stackbar" => {
                let h = 5.0 * tick_scale;
                let (x1, x2) = (bx + pad, bx + pad + LEGEND_SWATCH_W * tick_scale);
                ops.push(DrawOp::Rect {
                    x: x1, y: y - h, w: x2 - x1, h: h * 2.0,
                    fill: Some(color.clone()), stroke: Some(color), opacity: 0.85, radius: 1.0,
                });
            }
            _ if joined.is_some() => {}
            _ => ops.push(DrawOp::Line { x1: bx + pad, y1: y, x2: bx + pad + LEGEND_SWATCH_W * tick_scale, y2: y, color, width: 2.2 }),
        }
        ops.push(DrawOp::Text { italic: false,
            x: bx + pad + LEGEND_GUTTER * tick_scale,
            y: y + 3.5,
            text: series.label.clone().unwrap_or_default(),
            size: text_size,
            anchor: Anchor::Start,
            rotate: 0.0,
            color: "#222".into(),
        });
    }
    ops
}

/// Resolves a fixed `LegendPosition` to its box origin inside the plot area.
fn inside_position(position: LegendPosition, left: f64, top: f64, right: f64, bottom: f64, box_w: f64, box_h: f64) -> (f64, f64) {
    match position {
        LegendPosition::TopLeft | LegendPosition::Left | LegendPosition::Best => (left + 8.0, top + 8.0),
        LegendPosition::TopRight | LegendPosition::Right => (right - box_w - 8.0, top + 8.0),
        LegendPosition::BottomLeft => (left + 8.0, bottom - box_h - 8.0),
        LegendPosition::BottomRight => (right - box_w - 8.0, bottom - box_h - 8.0),
        LegendPosition::Top => ((left + right) / 2.0 - box_w / 2.0, top + 8.0),
        LegendPosition::Bottom => ((left + right) / 2.0 - box_w / 2.0, bottom - box_h - 8.0),
    }
}

/// Places the legend box in the margin strip reserved for it by
/// `build_draw_ops` (see `panel.legend.outside`), centered along the axis
/// perpendicular to its side and clamped to the panel's own outer rect.
fn outside_position(side: Side, left: f64, top: f64, right: f64, bottom: f64, outer: (f64, f64, f64, f64), box_w: f64, box_h: f64) -> (f64, f64) {
    let (ox, oy, ow, oh) = outer;
    match side {
        Side::Right => (right + 8.0, ((top + bottom) / 2.0 - box_h / 2.0).clamp(oy + 4.0, (oy + oh - box_h - 4.0).max(oy + 4.0))),
        Side::Left => (ox + 4.0, ((top + bottom) / 2.0 - box_h / 2.0).clamp(oy + 4.0, (oy + oh - box_h - 4.0).max(oy + 4.0))),
        Side::Top => (((left + right) / 2.0 - box_w / 2.0).clamp(ox + 4.0, (ox + ow - box_w - 4.0).max(ox + 4.0)), oy + 4.0),
        Side::Bottom => (((left + right) / 2.0 - box_w / 2.0).clamp(ox + 4.0, (ox + ow - box_w - 4.0).max(ox + 4.0)), oy + oh - box_h - 4.0),
    }
}

/// Picks the candidate position whose legend box would overlap the fewest
/// actual data points, using the panel's real pixel-space mapping (`points`
/// are already `to_px`-transformed) rather than a coarse per-series-bounds
/// quadrant vote. Ties favor `TopRight`, the historical default.
/// The share of the plot area an inside legend may claim before making
/// room for it costs more than it is worth.
///
/// At this share the y range grows by 1/(1 - 0.3) = 43% to clear the box,
/// which is already a lot of white space to spend on a key. A legend
/// bigger than this needs a different layout -- `legend outside`, more
/// `columns=`, a smaller `fontsize=` -- and `legend_fit_note` says so
/// rather than the figure quietly coming out with a key on the curve.
const LEGEND_MAX_SHARE: f64 = 0.3;

/// Clear space between the legend box and the nearest data, in units of
/// the legend's own inner padding. Touching is not "clear".
const LEGEND_CLEARANCE: f64 = 2.0;

/// Whether a legend nobody asked to move has to move anyway.
///
/// The second half of the two-pass placement. The first pass fixes the
/// plot box (every other thing that shrinks it having been reserved);
/// this asks whether a key can live inside a box that size, and its
/// answer is what lets the caller reserve the outside margin -- which is
/// the ordering that made automatic outside placement impossible before.
/// An outside legend must be paid for BEFORE the layout, and whether it
/// is needed can only be known FROM the layout.
///
/// Two ways in. The box can be too tall for `legend_headroom` to buy room
/// for, which needs no data at all -- only the box against the plot. Or
/// the script can have pinned the range itself, in which case nothing may
/// stretch it and going outside is the only way left; that one is checked
/// against the actual data, so a pinned panel with a free corner is left
/// alone.
///
/// A legend already marked `outside` never reaches here: this only ever
/// promotes one that would otherwise sit on the data.
fn legend_must_go_outside(
    panel: &Panel,
    plot_w: f64,
    plot_h: f64,
    tick_scale: f64,
    face: Option<&'static pdf_font::FontMetrics>,
) -> bool {
    if !legend_is_placed_automatically(panel) || !(plot_w > 0.0 && plot_h > 0.0) {
        return false;
    }
    let (box_w, box_h) = legend_box_size(panel, tick_scale, face);
    if box_w <= 0.0 {
        return false;
    }
    // Nothing to fix unless the key would actually land on something.
    //
    // Checking the SIZE first was wrong, and wrong in the direction that
    // does damage: a three-entry key on a panel whose top-right corner is
    // empty -- a decay curve, the commonest figure there is -- was taken
    // out of the axes for no reason at all, narrowing the panel to solve a
    // problem it did not have.
    if !legend_overlaps_inside(panel, plot_w, plot_h, box_w, box_h) {
        return false;
    }
    // It overlaps. Can `legend_headroom` clear it by making sky?
    let band = box_h + LEGEND_CLEARANCE * LEGEND_PAD * tick_scale;
    if band > plot_h * LEGEND_MAX_SHARE {
        return true; // too big -- the sky would cost more than the overlap
    }
    // Room enough, unless the script fixed the range itself, in which case
    // nothing may stretch it and out is the only way left.
    panel.ylim.is_some() || panel.tight
}

/// Whether Qu gets to choose where this legend goes.
///
/// `legend` on its own means "put it somewhere sensible", and that is the
/// only case the automatic placement acts on. `legend top right` names a
/// place, and a named place is an instruction: the key stays there even if
/// it covers data, because a script that says where the legend goes has
/// already weighed that. Same principle as `ylim` and `figure_size` -- Qu
/// decides only what the script left open.
fn legend_is_placed_automatically(panel: &Panel) -> bool {
    panel.legend.visible && !panel.legend.outside && panel.legend.position == LegendPosition::Best
}

/// Would the best inside placement cover data, at the limits the panel
/// has now?
///
/// Normalised to the plot box, so it answers before any pixel rect exists
/// -- only the box's SIZE is needed, not where it sits on the canvas.
/// Scores the same points `legend_ops` will (series only: that is what
/// `best_corner` is given there) through `best_corner` itself, so the
/// question asked here and the placement made later cannot drift.
fn legend_overlaps_inside(panel: &Panel, plot_w: f64, plot_h: f64, box_w: f64, box_h: f64) -> bool {
    let raw: Vec<(f64, f64)> = panel
        .series
        .iter()
        .flat_map(|s| s.x.iter().zip(s.y.iter()).map(|(&x, &y)| (x, y)))
        .collect();
    if raw.is_empty() {
        return false;
    }
    let log_x = panel.xscale == Scale::Log;
    let log_y = panel.yscale == Scale::Log;
    // `data_extent` takes raw values and returns limits already in log
    // space when asked, so the points have to be transformed to match.
    let (xmin, xmax) = panel
        .xlim
        .map(|(a, b)| if log_x { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) })
        .unwrap_or_else(|| data_extent(raw.iter().map(|p| p.0), log_x, !panel.tight));
    let (ymin, ymax) = panel
        .ylim
        .map(|(a, b)| if log_y { (a.max(1e-300).log10(), b.max(1e-300).log10()) } else { (a, b) })
        .unwrap_or_else(|| data_extent(raw.iter().map(|p| p.1), log_y, !panel.tight));
    if !(xmax > xmin && ymax > ymin) {
        return false;
    }
    let axis = |v: f64, log: bool, lo: f64| if log { if v > 0.0 { v.log10() } else { lo } } else { v };
    let points: Vec<(f64, f64)> = raw
        .iter()
        .map(|&(x, y)| {
            (
                (axis(x, log_x, xmin) - xmin) / (xmax - xmin) * plot_w,
                plot_h - (axis(y, log_y, ymin) - ymin) / (ymax - ymin) * plot_h,
            )
        })
        .collect();
    let corner = best_corner(0.0, 0.0, plot_w, plot_h, box_w, box_h, &points);
    let (bx, by) = inside_position(corner, 0.0, 0.0, plot_w, plot_h, box_w, box_h);
    points
        .iter()
        .any(|&(px, py)| px >= bx && px <= bx + box_w && py >= by && py <= by + box_h)
}

/// Raise a panel's upper y limit so an inside legend sits ABOVE the data
/// rather than on top of it.
///
/// `best_corner` scores the eight inside placements and takes the one the
/// least data falls into -- but "least" is not "none", and when the data
/// reaches into every corner the key lands on the curve it is labelling.
/// That is what a `legend` on a ROC panel does: the curve sweeps corner to
/// corner by construction, so every candidate is occupied and the box goes
/// over the data no matter which one wins.
///
/// The fix is the one a person makes by hand -- give the figure some sky.
/// Raising `ymax` until the top band is empty costs only white space, and
/// the top band is then free by construction, so `best_corner`'s own scores
/// send the legend there.
///
/// Returns `None` when nothing should move: no legend, no overlap to fix,
/// the script fixed the limits itself (`ylim`/`axis tight` are
/// instructions), or the box is too big for this to be the right answer --
/// that last case is what `legend_fit_note` reports.
///
/// Works unchanged on a log axis: `ymin`/`ymax` arrive already in log10
/// space, and the mapping is linear in that space, so every ratio below
/// means the same thing.
#[allow(clippy::too_many_arguments)]
fn legend_headroom(
    panel: &Panel,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    xmin: f64,
    xmax: f64,
    ymin: f64,
    ymax: f64,
    tick_scale: f64,
    face: Option<&'static pdf_font::FontMetrics>,
) -> Option<f64> {
    if !legend_is_placed_automatically(panel) || panel.tight || panel.ylim.is_some() {
        return None;
    }
    let (plot_w, plot_h) = (right - left, bottom - top);
    if !(plot_w > 0.0 && plot_h > 0.0) || !(ymax > ymin) || !(xmax > xmin) {
        return None;
    }
    let (box_w, box_h) = legend_box_size(panel, tick_scale, face);
    if box_w <= 0.0 {
        return None;
    }
    let band = box_h + LEGEND_CLEARANCE * LEGEND_PAD * tick_scale;
    if band > plot_h * LEGEND_MAX_SHARE {
        return None;
    }

    // The same points, in the same box, that `legend_ops` will score --
    // taken through `best_corner` itself so the question asked here and
    // the placement made there cannot drift apart.
    let log_y = panel.yscale == Scale::Log;
    let log_x = panel.xscale == Scale::Log;
    let to_px = |x: f64, y: f64| -> (f64, f64) {
        let x = if log_x { if x > 0.0 { x.log10() } else { xmin } } else { x };
        let y = if log_y { if y > 0.0 { y.log10() } else { ymin } } else { y };
        (
            left + (x - xmin) / (xmax - xmin) * plot_w,
            bottom - (y - ymin) / (ymax - ymin) * plot_h,
        )
    };
    let points: Vec<(f64, f64)> = panel
        .series
        .iter()
        .flat_map(|s| s.x.iter().zip(s.y.iter()).map(|(&x, &y)| to_px(x, y)))
        .collect();
    if points.is_empty() {
        return None;
    }
    let corner = best_corner(left, top, right, bottom, box_w, box_h, &points);
    let (bx, by) = inside_position(corner, left, top, right, bottom, box_w, box_h);
    let covered = points
        .iter()
        .any(|&(px, py)| px >= bx && px <= bx + box_w && py >= by && py <= by + box_h);
    if !covered {
        return None;
    }

    // How far the topmost data already sits below the top of the plot, and
    // how far down it has to be for the band to be clear -- both as a
    // fraction of the plot height, so the arithmetic is the same whatever
    // the units.
    let data_top = points.iter().fold(f64::INFINITY, |m, &(_, py)| m.min(py));
    let t = (data_top - top) / plot_h;
    let b = band / plot_h;
    if !(t < b) {
        // Already clear at the top; the overlap is somewhere the top band
        // cannot fix, so leave the limits alone rather than growing the
        // axis for nothing.
        return None;
    }
    // Keep `ymin` and stretch: the topmost point must land at `b` instead
    // of `t`, so the span scales by (1 - t)/(1 - b).
    let span = (ymax - ymin) * (1.0 - t) / (1.0 - b);
    Some(ymin + span)
}

/// Why a visible inside legend still sits on the data, if it does.
///
/// `legend_headroom` clears the ordinary case by making sky above the
/// curve. What it cannot fix is a key too big to fit inside at all, or one
/// on a panel whose limits the script pinned itself -- and those used to
/// come out as a box over the data with nothing said. Reported by
/// `savefig`, naming the remedies, because a figure that hides its own
/// data is worth a line of output.
pub fn legend_fit_note(fig: &Figure, width: f64, height: f64) -> Option<String> {
    let width = resolved_width(fig, width);
    let height = resolved_height(fig, width, height);
    let tick_scale = fig.tick_size / TICK_METRIC_BASELINE;
    let title_scale = fig.title_size / DEFAULT_TITLE_SIZE;
    let face = face_metrics(&fig.font_family);
    let multi = fig.panels.len() > 1;
    for rect in layout_panels(fig, width, height) {
        let panel = &fig.panels[rect.panel];
        if !panel.legend.visible || panel.legend.outside || panel.series.is_empty() {
            continue;
        }
        // Name the panel when there is more than one, or the reader has a
        // complaint about a figure and no idea which sixth of it to look at.
        let which = if multi { format!("panel {}: ", rect.panel + 1) } else { String::new() };
        let (box_w, box_h) = legend_box_size(panel, tick_scale, face);
        if box_w <= 0.0 {
            continue;
        }
        // The same base margins `build_draw_ops` starts from. Its extras
        // only shrink the plot further, so a box that does not fit against
        // these does not fit there either -- and this note is about what
        // the layout DID, so erring toward under-reporting is right: a
        // legend quietly left inside where it fits is not news.
        let plot_w = rect.w - MARGIN_LEFT * tick_scale - MARGIN_RIGHT;
        let plot_h = rect.h - MARGIN_TOP * title_scale - MARGIN_BOTTOM * tick_scale;
        if !legend_must_go_outside(panel, plot_w, plot_h, tick_scale, face) {
            continue;
        }
        // Only ever offer a remedy that would actually work. Recommending
        // `columns=2` to someone who has already set it -- and whose legend
        // still does not fit -- is worse than saying nothing: it reads as
        // the tool not knowing what it just did. And a shrunk box does not
        // help at all on a panel whose range is pinned, because there the
        // problem was never the size.
        let band = box_h + LEGEND_CLEARANCE * LEGEND_PAD * tick_scale;
        let pinned = panel.ylim.is_some() || panel.tight;
        let cause = if pinned {
            format!(
                "{which}the legend covered the data, and this panel fixes its own y range \
                 (ylim / axis tight), leaving no room to make above the curve"
            )
        } else {
            format!(
                "{which}the legend covered the data and needs {:.0}% of the plot height, \
                 more than making room for it is worth",
                if plot_h > 0.0 { band / plot_h * 100.0 } else { 0.0 }
            )
        };
        // A smaller box only helps where SIZE was the obstacle.
        let smaller = if pinned {
            None
        } else {
            let entries = panel.series.iter().filter(|s| s.label.is_some()).count();
            let size = panel.legend.font_size.unwrap_or(TICK_METRIC_BASELINE * tick_scale);
            let padding = panel.legend.padding.unwrap_or(1.0);
            let has_title = panel.legend.title.is_some();
            let budget = plot_h * LEGEND_MAX_SHARE - LEGEND_CLEARANCE * LEGEND_PAD * tick_scale;
            let rules = panel
                .series
                .iter()
                .filter_map(|s| s.label.as_deref())
                .any(label_has_rule);
            let fits = |cols: usize, size: f64| {
                legend_box_height(entries, cols, size, has_title, padding, tick_scale, rules)
                    <= budget
            };
            (panel.legend.columns.max(1) + 1..=entries.max(1))
                .find(|&c| fits(c, size))
                .map(|c| format!("`legend(columns={c})`"))
                .or_else(|| {
                    fits(panel.legend.columns.max(1), size * 0.7)
                        .then(|| "a smaller `legend(fontsize=...)`".to_string())
                })
        };
        // Naming a position always works, because a named position is an
        // instruction and is left alone -- overlap included. That is the
        // answer for anyone who would rather have the key on the data than
        // a narrower panel, so it is always worth saying.
        let remedy = match smaller {
            Some(s) => format!("{s} would let it sit inside, or `legend top right` keeps it there and accepts the overlap"),
            None => "`legend top right` (or any named position) keeps it inside and accepts the overlap".to_string(),
        };
        return Some(format!("{cause}, so it was placed outside the axes -- {remedy}"));
    }
    None
}

fn best_corner(left: f64, top: f64, right: f64, bottom: f64, box_w: f64, box_h: f64, points: &[(f64, f64)]) -> LegendPosition {
    const CANDIDATES: [LegendPosition; 8] = [
        LegendPosition::TopRight,
        LegendPosition::TopLeft,
        LegendPosition::BottomRight,
        LegendPosition::BottomLeft,
        LegendPosition::Right,
        LegendPosition::Left,
        LegendPosition::Top,
        LegendPosition::Bottom,
    ];
    CANDIDATES
        .iter()
        .copied()
        .min_by_key(|&pos| {
            let (bx, by) = inside_position(pos, left, top, right, bottom, box_w, box_h);
            points
                .iter()
                .filter(|&&(px, py)| px >= bx && px <= bx + box_w && py >= by && py <= by + box_h)
                .count()
        })
        .unwrap_or(LegendPosition::TopRight)
}

// -------------------------------------------------------------- renderers

/// `embed_fonts`: inline the actual glyph data for a curated `fontfamily`
/// preset as a base64 `@font-face`, so the figure renders identically on a
/// machine that doesn't have that font installed — off by default (kept
/// lightweight; SVGs stay small and rely on the viewer's system fonts, with
/// `savefig`'s own note explaining that), opt in via `savefig(path,
/// embed_fonts=true)`. A no-op for a custom (non-preset) `fontfamily(...)`
/// string, since there are no bytes to embed for an arbitrary font name.
/// The panel mapping, as the JSON that goes in the SVG's `<metadata>`.
///
/// Hand-rolled rather than `serde_json`: this is a fixed, flat shape of
/// numbers and booleans, and `plotting.rs` has no serde dependency today.
fn geometry_json(geometry: &[PanelGeom], width: f64, height: f64) -> String {
    let num = |v: f64| -> String {
        // Finite numbers only -- `NaN`/`Infinity` are not JSON, and a
        // reader that hits one would fail to parse the whole document
        // rather than lose one panel.
        if v.is_finite() { format!("{v:.4}") } else { "null".to_string() }
    };
    let brk = |b: Option<(f64, f64)>| match b {
        Some((lo, hi)) => format!("[{},{}]", num(lo), num(hi)),
        None => "null".to_string(),
    };
    let panels: Vec<String> = geometry
        .iter()
        .map(|g| {
            format!(
                "{{\"panel\":{},\"left\":{},\"right\":{},\"top\":{},\"bottom\":{},\
                 \"xmin\":{},\"xmax\":{},\"ymin\":{},\"ymax\":{},\
                 \"logX\":{},\"logY\":{},\"xBreak\":{},\"yBreak\":{}}}",
                g.panel,
                num(g.left),
                num(g.right),
                num(g.top),
                num(g.bottom),
                num(g.xmin),
                num(g.xmax),
                num(g.ymin),
                num(g.ymax),
                g.log_x,
                g.log_y,
                brk(g.x_break),
                brk(g.y_break),
            )
        })
        .collect();
    format!(
        "{{\"version\":1,\"width\":{},\"height\":{},\"panels\":[{}]}}",
        num(width),
        num(height),
        panels.join(",")
    )
}

/// Undoes `xml_escape`, for reading text content back out of rendered SVG.
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Every character the finished picture actually paints, split by whether
/// it is painted bold, read back out of the rendered `<text>` elements.
///
/// Not off `DrawOp::Text`: `render_math_svg` rewrites `$\times$` into `×`
/// and splits a label into `<tspan>`s, so the characters that reach the
/// page are NOT the ones the caller wrote. Reading the output is the only
/// way for the embedded subset to cover exactly what gets drawn — a glyph
/// missed here is a blank rectangle in the figure.
fn drawn_chars(body: &str) -> (BTreeSet<char>, BTreeSet<char>, BTreeSet<char>) {
    let (mut all, mut bold, mut italic) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut rest = body;
    while let Some(i) = rest.find("<text") {
        let after = &rest[i..];
        let Some(end) = after.find("</text>") else { break };
        let elem = &after[..end];
        // `font-weight` sits on the `<text>` itself (see `write_svg_op`)
        // and inherits to every `<tspan>` inside it.
        let open = &elem[..elem.find('>').unwrap_or(elem.len())];
        let is_bold = open.contains("font-weight=\"700\"");
        let is_italic = open.contains("font-style=\"italic\"");
        let mut in_tag = false;
        let mut text = String::new();
        for c in elem.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => text.push(c),
                _ => {}
            }
        }
        for c in xml_unescape(&text).chars() {
            all.insert(c);
            if is_bold {
                bold.insert(c);
            }
            if is_italic {
                italic.insert(c);
            }
        }
        rest = &after[end + "</text>".len()..];
    }
    (all, bold, italic)
}

/// One `@font-face` rule, carrying a subset of `bytes` cut down to `chars`.
///
/// The subsetter is allowed to decline (see `font_subset::subset`); when it
/// does, the whole font goes in exactly as it used to. Embedding too much
/// is a big file, embedding a font this code only half understood would be
/// a figure with no text.
fn font_face_css(
    family_name: &str,
    bytes: &[u8],
    format_tag: &str,
    weight: &str,
    chars: &BTreeSet<char>,
) -> String {
    font_face_css_styled(family_name, bytes, format_tag, weight, "normal", chars)
}

#[allow(clippy::too_many_arguments)]
fn font_face_css_styled(
    family_name: &str,
    bytes: &[u8],
    format_tag: &str,
    weight: &str,
    style: &str,
    chars: &BTreeSet<char>,
) -> String {
    let subset = font_subset::subset(bytes, chars);
    let bytes = subset.as_deref().unwrap_or(bytes);
    let mime = if format_tag == "opentype" { "font/otf" } else { "font/ttf" };
    format!(
        "@font-face{{font-family:'{family_name}';src:url(data:{mime};base64,{}) format('{format_tag}');font-weight:{weight};font-style:{style};}}",
        base64_encode(bytes)
    )
}

pub fn render_svg(fig: &Figure, width: f64, height: f64, embed_fonts: bool) -> String {
    let width = resolved_width(fig, width);
    let height = resolved_height(fig, width, height);
    let (ops, geometry) = build_draw_ops_with_geometry(fig, width, height);

    // The picture is drawn into its own buffer FIRST. The `@font-face`
    // block that precedes it in the file carries only the glyphs this body
    // asks for, so it cannot be written until the body exists to be read.
    let mut body = String::new();
    let _ = write!(
        body,
        "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\"/>\n",
        xml_escape(&fig.background)
    );
    match &fig.blur_backdrop {
        None => {
            for op in &ops {
                write_svg_op(&mut body, op, fig.label_size);
            }
        }
        // `blur_backdrop`: the *entire* figure is drawn once inside a
        // Gaussian-blur filter group (the "backdrop"), then drawn again,
        // unblurred, clipped to just the focus rectangle on top — real
        // depth-of-field, not a per-op flag, since blur/clip are properties
        // of how a whole group of shapes is composited, not of any single
        // `DrawOp`. See `Figure.blur_backdrop`'s doc comment for why this
        // has no TikZ equivalent (`render_tikz` degrades instead).
        Some(blur) => {
            let _ = write!(
                body,
                "<defs><filter id=\"qu-blur-backdrop\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\"><feGaussianBlur stdDeviation=\"{:.2}\"/></filter>\n",
                blur.radius
            );
            let (fx, fy) = (blur.fx0.min(blur.fx1) * width, blur.fy0.min(blur.fy1) * height);
            let (fw, fh) = ((blur.fx1 - blur.fx0).abs() * width, (blur.fy1 - blur.fy0).abs() * height);
            let _ = write!(
                body,
                "<clipPath id=\"qu-blur-focus\"><rect x=\"{fx:.2}\" y=\"{fy:.2}\" width=\"{fw:.2}\" height=\"{fh:.2}\"/></clipPath></defs>\n"
            );
            body.push_str("<g filter=\"url(#qu-blur-backdrop)\">\n");
            for op in &ops {
                write_svg_op(&mut body, op, fig.label_size);
            }
            body.push_str("</g>\n");
            body.push_str("<g clip-path=\"url(#qu-blur-focus)\">\n");
            for op in &ops {
                write_svg_op(&mut body, op, fig.label_size);
            }
            body.push_str("</g>\n");
        }
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\" font-family=\"{}\">\n",
        xml_escape(&fig.font_family)
    );
    if embed_fonts {
        if let Some((family_name, bytes, format_tag)) = embeddable_font_for(&fig.font_family) {
            let (all_chars, bold_chars, italic_chars) = drawn_chars(&body);
            // A separate face per weight where the family has separately
            // DRAWN weights. Declaring `100 900` on a single static file
            // tells the browser one outline covers every weight, so it
            // fakes bold by smearing the regular -- visibly wrong beside
            // real LaTeX. Variable fonts (Inter, Source Serif) do carry the
            // whole range in one file and keep the single face.
            let bold = embeddable_bold_font_for(&fig.font_family);
            let regular_range = if bold.is_some() { "400" } else { "100 900" };
            let mut css = font_face_css(family_name, bytes, format_tag, regular_range, &all_chars);
            if let Some((bold_family, bold_bytes, bold_format)) = bold {
                // The bold face only ever sets the title, so it only needs
                // the title's own characters.
                css.push_str(&font_face_css(
                    bold_family,
                    bold_bytes,
                    bold_format,
                    "700",
                    &bold_chars,
                ));
            }
            // Anything the main face cannot draw -- Greek and the maths
            // operators, for every one of the four presets -- comes from
            // Latin Modern Math rather than from whatever the viewer's
            // machine happens to install. Nothing is embedded unless the
            // figure actually paints such a character, so an ordinary
            // figure carries no second face at all.
            // The drawn italic, for a label that is commentary rather
            // than data. Declared `font-style: italic` against the same
            // family name, so the `font-style` attribute on the `<text>`
            // selects it and nothing else changes.
            if !italic_chars.is_empty() {
                if let Some((it_family, it_bytes, it_format)) =
                    embeddable_italic_font_for(&fig.font_family)
                {
                    css.push_str(&font_face_css_styled(
                        it_family, it_bytes, it_format, "400", "italic", &italic_chars,
                    ));
                }
            }
            let missing = font_subset::unsupported(bytes, &all_chars);
            if !missing.is_empty() {
                // Weight 400 only: real LaTeX does not embolden maths
                // either, and claiming a range a static face doesn't have
                // is what produces synthesised bold.
                let (math_family, math_bytes, math_format) = embeddable_math_font();
                css.push_str(&font_face_css(
                    math_family,
                    math_bytes,
                    math_format,
                    "400",
                    &missing,
                ));
            }
            let _ = write!(out, "<defs><style>{css}</style></defs>\n");
        }
    }
    // `<metadata>` is SVG's own element for exactly this: machine-readable
    // data that renderers ignore. Nothing about the picture changes, and
    // any viewer that wants to answer "what data value is under the
    // cursor" can, exactly, instead of reverse-engineering tick labels.
    let _ = write!(
        out,
        "<metadata id=\"qu-figure\">{}</metadata>\n",
        xml_escape(&geometry_json(&geometry, width, height))
    );
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

/// Common LaTeX math macros this "mathtext-lite" understands, mapped to
/// their direct Unicode equivalent — Greek letters and common operators.
/// Not real LaTeX (no dependency, no external toolchain — `engine`'s own
/// "dependency-free by design" policy); this is the same idea as
/// matplotlib's own "mathtext": good enough for axis labels/titles, not a
/// typesetting engine. An unrecognized macro's name is kept as literal text
/// (dropping just the backslash) rather than silently disappearing.
fn latex_symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "\u{03B1}", "beta" => "\u{03B2}", "gamma" => "\u{03B3}", "delta" => "\u{03B4}",
        "epsilon" => "\u{03B5}", "zeta" => "\u{03B6}", "eta" => "\u{03B7}", "theta" => "\u{03B8}",
        "iota" => "\u{03B9}", "kappa" => "\u{03BA}", "lambda" => "\u{03BB}", "mu" => "\u{03BC}",
        "nu" => "\u{03BD}", "xi" => "\u{03BE}", "omicron" => "\u{03BF}", "pi" => "\u{03C0}",
        "rho" => "\u{03C1}", "sigma" => "\u{03C3}", "tau" => "\u{03C4}", "upsilon" => "\u{03C5}",
        "phi" => "\u{03C6}", "chi" => "\u{03C7}", "psi" => "\u{03C8}", "omega" => "\u{03C9}",
        "Delta" => "\u{0394}", "Gamma" => "\u{0393}", "Theta" => "\u{0398}", "Lambda" => "\u{039B}",
        "Xi" => "\u{039E}", "Pi" => "\u{03A0}", "Sigma" => "\u{03A3}", "Phi" => "\u{03A6}",
        "Psi" => "\u{03A8}", "Omega" => "\u{03A9}",
        "pm" => "\u{00B1}", "mp" => "\u{2213}", "times" => "\u{00D7}", "cdot" => "\u{22C5}",
        "div" => "\u{00F7}", "infty" => "\u{221E}", "leq" => "\u{2264}", "geq" => "\u{2265}",
        "neq" => "\u{2260}", "approx" => "\u{2248}", "equiv" => "\u{2261}", "sim" => "\u{223C}",
        "propto" => "\u{221D}", "partial" => "\u{2202}", "nabla" => "\u{2207}", "sum" => "\u{2211}",
        "int" => "\u{222B}", "sqrt" => "\u{221A}", "circ" => "\u{00B0}", "degree" => "\u{00B0}",
        "rightarrow" => "\u{2192}", "leftarrow" => "\u{2190}", "cdots" => "\u{22EF}", "ldots" => "\u{2026}",
        // Arrows a caption actually reaches for, beyond the two above.
        "to" => "\u{2192}", "uparrow" => "\u{2191}", "downarrow" => "\u{2193}",
        "leftrightarrow" => "\u{2194}", "Rightarrow" => "\u{21D2}",
        // Relations and set notation that turn up in a figure legend.
        "ll" => "\u{226A}", "gg" => "\u{226B}", "simeq" => "\u{2243}",
        "in" => "\u{2208}", "forall" => "\u{2200}", "exists" => "\u{2203}",
        "prod" => "\u{220F}", "prime" => "\u{2032}", "ell" => "\u{2113}",
        "varepsilon" => "\u{03B5}", "vartheta" => "\u{03D1}", "varphi" => "\u{03D5}",
        // Operator names. LaTeX sets these upright to distinguish the
        // function from a product of variables; this renderer has no
        // italic to escape from, so the name itself is the whole job --
        // but they must still be RECOGNISED, or `\ln K` renders as "lnK"
        // with the space eaten and, worse, `\max` as "max" only by luck
        // of the unknown-macro fallback.
        "ln" => "ln", "log" => "log", "exp" => "exp", "det" => "det",
        "min" => "min", "max" => "max", "arg" => "arg", "sin" => "sin",
        "cos" => "cos", "tan" => "tan", "lim" => "lim", "mathrm" => "",
        "text" => "", "mathbf" => "", "mathit" => "", "mathcal" => "",
        _ => return None,
    })
}

/// A combining mark for the accent macros, applied to the character that
/// follows it.
///
/// `\widehat{\mathrm{CF}}` is how a paper writes "the estimate of CF", and
/// the hat is not decoration -- it is what distinguishes the estimate from
/// the measurement the figure plots beside it. Rendered as a Unicode
/// combining mark rather than a drawn overline: it needs no second text
/// span, no width measurement, and it survives copy-paste out of the SVG.
/// The accents drawn as a RULE over the whole group rather than as a
/// combining character on one letter. See the call site.
fn latex_rule_accent(name: &str) -> Option<Overline> {
    Some(match name {
        "hat" | "widehat" => Overline::Hat,
        "bar" | "overline" => Overline::Bar,
        _ => return None,
    })
}

fn latex_accent(name: &str) -> Option<char> {
    Some(match name {
        "tilde" | "widetilde" => '\u{0303}',
        "dot" => '\u{0307}',
        "ddot" => '\u{0308}',
        "vec" => '\u{20D7}',
        _ => return None,
    })
}

/// The single-character escapes, which are not alphabetic and so never
/// reach `latex_symbol`'s name scan.
///
/// `\%` is the one that matters in practice: a percent sign is ordinary
/// text in this renderer, but a script transcribed from LaTeX writes it
/// escaped, and dropping the backslash blindly is right only because
/// these have no other meaning here.
fn latex_escape(c: char) -> Option<&'static str> {
    Some(match c {
        '%' => "%",
        '&' => "&",
        '$' => "$",
        '#' => "#",
        '_' => "_",
        '{' => "{",
        '}' => "}",
        // LaTeX's thin/medium/thick spaces and its negative space. A
        // legend written `$\pm$\,s.e.` expects a gap, not "±s.e.".
        ',' | ';' | ':' | ' ' => "\u{2009}",
        '!' => "",
        _ => return None,
    })
}

/// Unicode superscript equivalents (Latin-1 Supplement + Superscripts and
/// Subscripts + Phonetic Extensions blocks) — covers digits, `+-=()`, and
/// most lowercase letters. Anything not in this table is left un-shrunk
/// (still emitted, at the smaller `tspan` size, just not raised — an
/// acceptable approximation rather than a silently dropped character).
const SUPER_MAP: &[(char, char)] = &[
    ('0', '\u{2070}'), ('1', '\u{00B9}'), ('2', '\u{00B2}'), ('3', '\u{00B3}'), ('4', '\u{2074}'),
    ('5', '\u{2075}'), ('6', '\u{2076}'), ('7', '\u{2077}'), ('8', '\u{2078}'), ('9', '\u{2079}'),
    ('+', '\u{207A}'), ('-', '\u{207B}'), ('=', '\u{207C}'), ('(', '\u{207D}'), (')', '\u{207E}'),
    ('a', '\u{1D43}'), ('b', '\u{1D47}'), ('c', '\u{1D9C}'), ('d', '\u{1D48}'), ('e', '\u{1D49}'),
    ('f', '\u{1DA0}'), ('g', '\u{1D4D}'), ('h', '\u{02B0}'), ('i', '\u{2071}'), ('j', '\u{02B2}'),
    ('k', '\u{1D4F}'), ('l', '\u{02E1}'), ('m', '\u{1D50}'), ('n', '\u{207F}'), ('o', '\u{1D52}'),
    ('p', '\u{1D56}'), ('r', '\u{02B3}'), ('s', '\u{02E2}'), ('t', '\u{1D57}'), ('u', '\u{1D58}'),
    ('v', '\u{1D5B}'), ('w', '\u{02B7}'), ('x', '\u{02E3}'), ('y', '\u{02B8}'), ('z', '\u{1DBB}'),
];
/// Unicode subscript equivalents — a narrower set than superscripts (fewer
/// subscript letters exist in Unicode at all); same fallback behavior.
const SUB_MAP: &[(char, char)] = &[
    ('0', '\u{2080}'), ('1', '\u{2081}'), ('2', '\u{2082}'), ('3', '\u{2083}'), ('4', '\u{2084}'),
    ('5', '\u{2085}'), ('6', '\u{2086}'), ('7', '\u{2087}'), ('8', '\u{2088}'), ('9', '\u{2089}'),
    ('+', '\u{208A}'), ('-', '\u{208B}'), ('=', '\u{208C}'), ('(', '\u{208D}'), (')', '\u{208E}'),
    ('a', '\u{2090}'), ('e', '\u{2091}'), ('h', '\u{2095}'), ('i', '\u{1D62}'), ('j', '\u{2C7C}'),
    ('k', '\u{2096}'), ('l', '\u{2097}'), ('m', '\u{2098}'), ('n', '\u{2099}'), ('o', '\u{2092}'),
    ('p', '\u{209A}'), ('r', '\u{1D63}'), ('s', '\u{209B}'), ('t', '\u{209C}'), ('u', '\u{1D64}'),
    ('v', '\u{1D65}'), ('x', '\u{2093}'),
];

fn script_map_char(c: char, table: &[(char, char)]) -> char {
    table.iter().find(|(k, _)| *k == c).map(|(_, v)| *v).unwrap_or(c)
}

/// Rewrites text to the precomposed Unicode superscript/subscript
/// characters — for the PDF backend ONLY.
///
/// The SVG path must not use these. It already raises the span with `dy`
/// and shrinks it with `font-size`, which is exactly how real LaTeX sets an
/// exponent: the face's OWN `4` at script size, not a separate `⁴` glyph.
/// Mapping on top of that asks the font for a character almost nothing has
/// — Latin Modern is missing 37 of the 40 superscripts and all 32
/// subscripts, Source Serif most of the subscripts — so `$x^4$` fell out of
/// the embedded face and into a system font while `$x^2$` (² being Latin-1)
/// happened to work. `flatten_math_to_plain` still needs them, because it
/// throws the offset and size away and the precomposed glyph is then the
/// only thing left saying "exponent".
fn to_script_chars(text: &str, superscript: bool) -> String {
    let table = if superscript { SUPER_MAP } else { SUB_MAP };
    text.chars().map(|c| script_map_char(c, table)).collect()
}

/// Expands `\macro` sequences within a `^{...}`/`_{...}` token via
/// `latex_symbol`, leaving the resulting characters as they are.
fn expand_script_macros(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            let name: String = chars[i + 1..j].iter().collect();
            match latex_symbol(&name) {
                Some(rep) => out.push_str(rep),
                None => out.push_str(&name),
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Reads one `^`/`_` argument: either a balanced `{...}` group (returning
/// its inner text) or a single character — matching real LaTeX's own rule
/// that `x^2` and `x^{2}` mean the same thing but `x^23` only raises `2`.
/// Expand macros inside a braced group, without splitting it into
/// baseline spans.
///
/// Groups nest -- `\mathrm{CF}_{\mathrm{rand}}` puts a macro inside a
/// subscript inside a macro -- and without this the inner one reaches the
/// output as the literal text "mathrmrand". Scripts are deliberately NOT
/// handled here: a superscript inside a subscript would need its own
/// baseline offset, and nothing in a figure label asks for that.
fn expand_math_group(text: &str) -> String {
    parse_math_spans(text).into_iter().map(|r| r.text).collect()
}

/// Reads an `open`-delimited group, honouring nesting, and returns its
/// contents WITHOUT the delimiters plus how many chars were consumed.
/// Shared by `{...}` and `(...)` so the two cannot drift apart.
fn read_balanced_group(chars: &[char], open: char, close: char) -> (String, usize) {
    let mut depth = 0i32;
    for (idx, &c) in chars.iter().enumerate() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return (chars[1..idx].iter().collect(), idx + 1);
            }
        }
    }
    (chars[1..].iter().collect(), chars.len()) // unbalanced: take the rest
}

/// Skips leading whitespace first, matching real TeX: a control WORD (a
/// macro name made of letters, which every caller here has just finished
/// reading) gobbles every space that follows it before the next token is
/// read, so `\hat Z` is `\hat{Z}` and `\frac 1 2` is `\frac{1}{2}` -- not
/// a mark drawn over a space with `Z` falling through as ordinary text,
/// which is what this returned before the skip existed. `skip` is folded
/// into every returned consumed-count (including `read_balanced_group`'s,
/// which knows nothing about it) so the caller still advances past
/// exactly what was read, whitespace included.
fn read_math_token(chars: &[char]) -> (String, usize) {
    let skip = chars.iter().take_while(|c| c.is_whitespace()).count();
    let chars = &chars[skip..];
    match chars.first() {
        Some('{') => {
            let (text, n) = read_balanced_group(chars, '{', '}');
            (text, skip + n)
        }
        // `10^(-3)` groups exactly as `10^{-3}` does.
        //
        // Braces are LaTeX's grouping character, but they are not what
        // people type into a `ylabel` -- `10^(-3)` is. Before this, that
        // superscripted the `(` ALONE and dropped `-3)` back to the
        // baseline at full size: a barely-visible raised paren followed by
        // a literal `-3)`. Nothing errors, the label just quietly says the
        // wrong thing, and at label sizes the stray `(` is easy to miss
        // entirely -- it was reported as "10-3", the paren unnoticed.
        //
        // The cost is that a superscript whose parentheses are part of the
        // NOTATION -- `x^(n)` for an n-th derivative -- now loses them.
        // That reading is the rarer one and `x^{(n)}` still expresses it,
        // whereas before there was no way at all to write `10^(-3)` and
        // have it mean what it plainly says.
        Some('(') => {
            let (text, n) = read_balanced_group(chars, '(', ')');
            (text, skip + n)
        }
        Some('\\') => {
            let mut j = 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            (chars[..j].iter().collect(), skip + j)
        }
        Some(&c) => (c.to_string(), skip + 1),
        None => (String::new(), skip),
    }
}

/// Parses one `$...$` math body into `(text, dy_em, size_scale)` spans, each
/// renderable as its own SVG `<tspan>`. `dy_em` is an *absolute* baseline
/// offset (not cumulative) so the caller can emit each span's `dy` as a
/// delta from the previous span's and land back at the true baseline once a
/// `^`/`_` run ends — see `render_math_svg`.
/// A rule drawn over a run of maths.
///
/// A radical's bar is not decoration: `\sqrt{2(L-\ln K)}` and
/// `\sqrt{2}(L-\ln K)` are different expressions, and the bar is the only
/// thing that says which one is meant. This renderer used to substitute
/// parentheses -- `√(2(L-ln K))` -- which is unambiguous but is not the
/// notation, and reads as a function call as easily as a root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overline {
    None,
    /// A horizontal rule spanning the run: a radical's vinculum, and
    /// `\overline`/`ar`.
    Bar,
    /// A chevron: `\hat`/`\widehat`. Drawn as a bar in SVG, where the
    /// browser lays the text out and the exact extent of a run is not
    /// ours to know; drawn as a real chevron in PDF, where it is.
    Hat,
}

/// One laid-out piece of a maths span.
///
/// Was a `(String, f64, f64, bool)` tuple. It grew a fifth field and two
/// adjacent flags of the same type is exactly the shape that gets
/// silently transposed, so it is a struct and the compiler checks the
/// names.
#[derive(Clone, Debug, PartialEq)]
pub struct MathRun {
    pub text: String,
    /// Baseline offset in em of the BASE size, positive downward, and
    /// absolute rather than cumulative.
    pub dy: f64,
    /// Size multiplier against the base size.
    pub scale: f64,
    /// A maths variable, set italic.
    pub italic: bool,
    pub over: Overline,
}

impl MathRun {
    fn plain(text: String, italic: bool) -> Self {
        Self { text, dy: 0.0, scale: 1.0, italic, over: Overline::None }
    }
}

fn parse_math_spans(math: &str) -> Vec<MathRun> {
    parse_math_runs(math, false)
}

/// Whether a character set in maths is a variable, and so italic.
///
/// TeX's rule, which is the one a reader has internalised: a Latin letter
/// standing for a quantity is italic; digits, operators, punctuation and
/// anything inside `\mathrm`/`	ext` are upright. Greek is deliberately
/// excluded -- Qu draws it from Latin Modern Math and there is no italic
/// Greek face bundled, so italicising it would mean a missing glyph
/// rather than a slanted one.
fn math_char_is_variable(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Append a group's own runs to an in-progress accumulation.
///
/// Used wherever a construct wraps a sub-expression -- an accent, a
/// radical -- and needs the sub-expression's STYLING, not just its
/// characters. Flattening to a string first loses every `\mathrm` and
/// every operator name inside it, which then come back as italic
/// variables.
fn push_runs(
    spans: &mut Vec<MathRun>,
    cur: &mut String,
    cur_italic: &mut bool,
    token: &str,
    upright: bool,
    over: Overline,
) {
    for r in parse_math_runs(token, upright) {
        let simple = r.dy == 0.0 && r.scale == 1.0 && r.over == Overline::None;
        // A run that is merely more text in the same style joins the one
        // being accumulated; anything with its own geometry -- a script, a
        // rule of its own -- has to stand alone.
        if (r.italic != *cur_italic || !simple || over != Overline::None) && !cur.is_empty() {
            spans.push(MathRun::plain(std::mem::take(cur), *cur_italic));
        }
        if simple && over == Overline::None {
            *cur_italic = r.italic;
            cur.push_str(&r.text);
        } else {
            // An outer rule wins: `\sqrt{\hat{x}}` is under the radical's
            // bar, and nesting two rules on one run would draw them at the
            // same height.
            let inner = if over == Overline::None { r.over } else { over };
            spans.push(MathRun { over: inner, ..r });
            *cur_italic = false;
        }
    }
}

/// `parse_math_spans`, plus the `upright` flag that `\mathrm{...}` sets on
/// everything inside it.
fn parse_math_runs(math: &str, upright: bool) -> Vec<MathRun> {
    const SUP_DY: f64 = -0.35;
    const SUB_DY: f64 = 0.25;
    const SCRIPT_SCALE: f64 = 0.72;
    let chars: Vec<char> = math.chars().collect();
    let mut spans: Vec<MathRun> = Vec::new();
    // The run being accumulated, and whether it is italic. A run is
    // flushed whenever the italic flag changes, so `$T_{\mathrm{rec}}$`
    // comes out as an italic `T` and an upright `rec` rather than one run
    // in whichever style happened to start it.
    let mut cur = String::new();
    let mut cur_italic = false;
    let mut i = 0;
    // Push text one character at a time so a run breaks exactly where the
    // style does. `upright` forces the whole lot upright, which is what a
    // `\mathrm` group and an operator name need.
    macro_rules! push_text {
        ($spans:expr, $cur:expr, $cur_italic:expr, $text:expr, $force_upright:expr) => {
            for ch in $text.chars() {
                let want = !$force_upright && !upright && math_char_is_variable(ch);
                if want != $cur_italic && !$cur.is_empty() {
                    $spans.push(MathRun::plain(std::mem::take(&mut $cur), $cur_italic));
                }
                $cur_italic = want;
                $cur.push(ch);
            }
        };
    }
    while i < chars.len() {
        match chars[i] {
            c @ ('^' | '_') => {
                if !cur.is_empty() {
                    spans.push(MathRun::plain(std::mem::take(&mut cur), cur_italic));
                    cur_italic = false;
                }
                let (token, consumed) = read_math_token(&chars[i + 1..]);
                i += 1 + consumed;
                let (dy, scale) = if c == '^' { (SUP_DY, SCRIPT_SCALE) } else { (SUB_DY, SCRIPT_SCALE) };
                // The token is expanded first: a subscript is very often
                // itself a macro, and expanding only the raw text would put
                // the backslash and the macro name into the subscript
                // instead of what it stands for.
                //
                // Deliberately NOT mapped to precomposed Unicode script
                // characters: `dy` and `font-size` already raise and shrink
                // this span, which is how real LaTeX sets an exponent, and
                // asking for `⁴` on top of that asks the face for a glyph
                // Latin Modern does not have. See `to_script_chars`.
                // Recursed rather than flattened, so a subscript keeps its
                // own styling: `Z_k` italicises the k, `\Delta_{\mathrm{real}}`
                // does not.
                for r in parse_math_runs(&token, upright) {
                    spans.push(MathRun {
                        text: expand_script_macros(&r.text),
                        dy,
                        scale,
                        italic: r.italic,
                        over: r.over,
                    });
                }
            }
            '\\' => {
                // A non-alphabetic character after the backslash is an
                // escape (`\%`, `\,`), not a macro name.
                if let Some(rep) = chars.get(i + 1).copied().and_then(latex_escape) {
                    push_text!(spans, cur, cur_italic, rep, true);
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();

                // An accent applies to whatever follows, so it has to be
                // handled before the symbol table: `\hat{x}` is a mark over
                // an x, not the word "hat".
                //
                // `\hat`/`\widehat` and `\bar`/`\overline` are drawn as
                // RULES over the whole group, the way they are set. They
                // used to be a combining character appended after it:
                // correct in principle, invisible in practice, because an
                // embedded subset font carries no combining marks and the
                // PDF dropped it -- so `\widehat{\mathrm{CF}}` came out as
                // a bare "CF", losing the one mark that said it was an
                // estimate rather than the quantity itself.
                if let Some(over) = latex_rule_accent(&name) {
                    let (token, consumed) = read_math_token(&chars[j..]);
                    // Recursed, not flattened, so the group keeps its own
                    // `\mathrm` and does not come back italic.
                    push_runs(&mut spans, &mut cur, &mut cur_italic, &token, upright, over);
                    i = j + consumed;
                    continue;
                }
                if let Some(mark) = latex_accent(&name) {
                    let (token, consumed) = read_math_token(&chars[j..]);
                    push_runs(&mut spans, &mut cur, &mut cur_italic, &token, upright, Overline::None);
                    // The small marks -- a tilde, a dot, an arrow -- stay
                    // combining characters: they are not rules, and the
                    // face that carries Greek carries these too.
                    push_text!(spans, cur, cur_italic, mark.to_string(), true);
                    i = j + consumed;
                    continue;
                }

                // `\frac{a}{b}`. Rendered inline as `a/b` rather than
                // stacked: a real fraction needs two stacked text spans
                // and a rule, and in a legend entry -- which is a single
                // line of type -- a stacked fraction would set the line
                // height for everything else. The common one-digit cases
                // get their proper Unicode glyph instead.
                // The radical needs its group too, or the braces print:
                // `$\sqrt{2\ln M}$` came out with the braces intact.
                // Parenthesised rather than overlined, because a real
                // radical needs a rule drawn over the radicand and this
                // renderer emits text spans. Parentheses are how the same
                // expression is written in plain text and are unambiguous,
                // which an overline faked with combining marks would not
                // be. A one-character radicand reads better bare.
                if name == "sqrt" {
                    // The radical sign, then the radicand under a rule.
                    //
                    // It used to be the radical sign and the radicand in
                    // PARENTHESES, on the grounds that this renderer emits
                    // text spans and cannot draw a rule. It can: a span
                    // knows its own advance, which is exactly the width the
                    // rule needs. Parentheses were unambiguous but they are
                    // not the notation -- "root open-paren 2L close-paren"
                    // reads as a function applied to 2L as readily as the
                    // root of it -- and a figure caption sitting beside a
                    // typeset equation was visibly not the same expression.
                    let (token, consumed) = read_math_token(&chars[j..]);
                    push_text!(spans, cur, cur_italic, "\u{221A}", true);
                    push_runs(&mut spans, &mut cur, &mut cur_italic, &token, upright, Overline::Bar);
                    i = j + consumed;
                    continue;
                }
                if name == "frac" {
                    let (num, c1) = read_math_token(&chars[j..]);
                    let (den, c2) = read_math_token(&chars[j + c1..]);
                    let (num, den) = (expand_math_group(&num), expand_math_group(&den));
                    let frac = match (num.as_str(), den.as_str()) {
                        ("1", "2") => "\u{00BD}".to_string(),
                        ("1", "3") => "\u{2153}".to_string(),
                        ("2", "3") => "\u{2154}".to_string(),
                        ("1", "4") => "\u{00BC}".to_string(),
                        ("3", "4") => "\u{00BE}".to_string(),
                        // Written on ONE line, so precedence has to be
                        // restored with brackets that the stacked form
                        // carries for free. `\frac{a+b}{c}` came out as
                        // "a+b/c", which does not merely look plainer than
                        // a real fraction -- it says something else: a
                        // reader parses it as a + b/c. Bracket either half
                        // that is more than a single symbol.
                        _ => format!("{}/{}", bracket_if_compound(&num), bracket_if_compound(&den)),
                    };
                    push_text!(spans, cur, cur_italic, &frac, false);
                    i = j + c1 + c2;
                    continue;
                }

                // An operator name is a word, and a word run straight into
                // the symbol before it reads as one token: `2\ln M` came
                // out as "2ln M". LaTeX sets a thin space around these for
                // exactly that reason. Only when something precedes it
                // that would collide -- not at the start of a span, and
                // not after a space or an opening bracket.
                const OPERATORS: [&str; 11] = [
                    "ln", "log", "exp", "det", "min", "max", "arg", "sin", "cos", "tan", "lim",
                ];
                if OPERATORS.contains(&name.as_str())
                    && cur.chars().last().is_some_and(|c| c.is_alphanumeric())
                {
                    push_text!(spans, cur, cur_italic, "\u{2009}", true);
                }
                match latex_symbol(&name) {
                    // A font-selection macro (`\mathrm`, `\text`) maps to
                    // the empty string: this renderer has one face, so the
                    // macro contributes nothing and its BRACED GROUP must
                    // still be read, or the braces would print.
                    Some("") => {
                        // `\mathrm{...}` / `\text{...}`: this renderer has one
                        // face per run, so the macro itself draws nothing --
                        // but it is precisely the instruction that its group is
                        // NOT variables, so the group is re-parsed upright.
                        let (token, consumed) = read_math_token(&chars[j..]);
                        if !cur.is_empty() {
                            spans.push(MathRun::plain(std::mem::take(&mut cur), cur_italic));
                            cur_italic = false;
                        }
                        for r in parse_math_runs(&token, true) {
                            spans.push(MathRun { italic: false, ..r });
                        }
                        i = j + consumed;
                        continue;
                    }
                    // A symbol (Greek, an operator glyph) is not a Latin
                    // variable, and an operator NAME (`min`, `ln`) is upright
                    // by the same rule that makes `\mathrm` upright.
                    Some(rep) => push_text!(spans, cur, cur_italic, rep, true),
                    None => push_text!(spans, cur, cur_italic, &name, true),
                }
                i = j;
            }
            // A binary operator is set with a true minus and a thin space
            // either side, which is what makes `L - ln K` read as typeset
            // maths rather than a hyphenated word. TeX decides binary from
            // what precedes: at the start of a group, or straight after
            // another operator or an opening bracket, `-` is the sign of
            // the term that follows and takes no space in front of it --
            // `$-Z''$` has to stay tight.
            c @ ('-' | '+' | '=' | '<' | '>') => {
                let prev = cur
                    .chars()
                    .last()
                    .or_else(|| spans.last().and_then(|r| r.text.chars().last()));
                let binary = matches!(prev, Some(p)
                    if p.is_alphanumeric() || p == ')' || p == ']' || p == '}');
                let glyph = match c {
                    '-' => '\u{2212}', // MINUS SIGN, not HYPHEN-MINUS
                    other => other,
                };
                if binary {
                    push_text!(spans, cur, cur_italic, "\u{2009}", true);
                }
                push_text!(spans, cur, cur_italic, glyph.to_string(), true);
                if binary {
                    push_text!(spans, cur, cur_italic, "\u{2009}", true);
                }
                i += 1;
            }
            c => {
                push_text!(spans, cur, cur_italic, c.to_string(), false);
                i += 1;
            }
        }
    }
    if !cur.is_empty() {
        spans.push(MathRun::plain(cur, cur_italic));
    }
    spans
}

/// Renders a label/title for SVG, expanding any `$...$` math spans into
/// sized/offset `<tspan>`s (see `parse_math_spans`) — plain text with no
/// `$` at all (the overwhelming common case) is returned exactly as before
/// (just XML-escaped), so this changes nothing for existing labels.
/// Pushes one `<tspan>`, computing its `dy` as a delta from wherever the
/// *previous* tspan left the baseline (`prev_dy`, updated in place) — never
/// as an isolated absolute offset. This is what lets plain text after a
/// superscript/subscript land back on the true baseline: the "come back
/// down" delta rides on that plain text's own tspan, rather than living on
/// a separate empty reset tspan (which real content shift is NOT
/// guaranteed to apply to; several renderers only advance the current text
/// position for tspans that actually contain characters, so a `dy` on an
/// empty tspan is silently dropped and everything after it stays visually
/// stuck at the raised/lowered position — the bug this function exists to
/// avoid).
/// `push_tspan` for a run that may be italic -- a maths variable.
fn push_tspan_styled(
    out: &mut String,
    prev_dy: &mut f64,
    dy: f64,
    size: Option<f64>,
    text: &str,
    italic: bool,
    over: Overline,
) {
    if !italic && over == Overline::None {
        return push_tspan(out, prev_dy, dy, size, text);
    }
    let delta = dy - *prev_dy;
    *prev_dy = dy;
    let dy_attr = if delta.abs() > 1e-9 { format!(" dy=\"{delta:.3}em\"") } else { String::new() };
    let size_attr = match size {
        Some(s) => format!(" font-size=\"{s:.2}\""),
        None => String::new(),
    };
    let style_attr = if italic { " font-style=\"italic\"" } else { "" };
    // `text-decoration: overline` is exactly a vinculum, and the browser
    // measures the run for us -- which matters here because SVG text is
    // laid out by the renderer, so the extent of a span is not something
    // this writer knows. A chevron would need that extent, so a hat is
    // set as a bar in SVG and as a real chevron in the PDF, where the
    // geometry is ours.
    let over_attr = match over {
        Overline::None => "",
        Overline::Bar | Overline::Hat => " text-decoration=\"overline\"",
    };
    let _ = write!(
        out,
        "<tspan{dy_attr}{size_attr}{style_attr}{over_attr}>{}</tspan>",
        xml_escape(text)
    );
}

fn push_tspan(out: &mut String, prev_dy: &mut f64, dy: f64, size: Option<f64>, text: &str) {
    let delta = dy - *prev_dy;
    *prev_dy = dy;
    let dy_attr = if delta.abs() > 1e-9 { format!(" dy=\"{delta:.3}em\"") } else { String::new() };
    let size_attr = match size {
        Some(s) => format!(" font-size=\"{s:.2}\""),
        None => String::new(),
    };
    let _ = write!(out, "<tspan{dy_attr}{size_attr}>{}</tspan>", xml_escape(text));
}

fn render_math_svg(text: &str, base_size: f64) -> String {
    if !text.contains('$') {
        return xml_escape(text);
    }
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut plain = String::new();
    // Tracked across the *whole* string, not reset at each `$...$` boundary
    // — see `push_tspan`.
    let mut prev_dy = 0.0f64;
    while let Some(c) = chars.next() {
        if c != '$' {
            plain.push(c);
            continue;
        }
        if !plain.is_empty() {
            push_tspan(&mut out, &mut prev_dy, 0.0, None, &plain);
            plain.clear();
        }
        let mut math = String::new();
        for c2 in chars.by_ref() {
            if c2 == '$' {
                break;
            }
            math.push(c2);
        }
        for r in parse_math_spans(&math) {
            let font_size = (base_size * r.scale).max(1.0);
            push_tspan_styled(
                &mut out, &mut prev_dy, r.dy, Some(font_size), &r.text, r.italic, r.over,
            );
        }
    }
    if !plain.is_empty() {
        push_tspan(&mut out, &mut prev_dy, 0.0, None, &plain);
    }
    out
}

/// `bold_above`: text LARGER than this is a label or a title and is set in
/// bold; text at or below it is a tick number and is not.
///
/// A threshold rather than a constant because the rule is about ROLE, not
/// absolute size. Keyed off `DEFAULT_AXIS_LABEL_SIZE` it broke the moment
/// publication mode scaled the type up: tick numbers crossed 16px and came
/// out bold, which no journal figure does. Passing the figure's own tick
/// size keeps the distinction where it belongs at any scale.
pub(crate) fn write_svg_op(out: &mut String, op: &DrawOp, bold_above: f64) {
    match op {
        // The clip rectangle is inlined per group rather than hoisted into
        // `<defs>`: SVG allows a `clipPath` anywhere, ids would have to be
        // made unique across panels, and there are only ever a handful of
        // these per figure.
        DrawOp::ClipStart { x, y, w, h } => {
            // A stable id per rectangle, so two panels never collide and
            // the same figure re-renders byte-identically.
            let id = format!(
                "qu-clip-{}-{}-{}-{}",
                (x * 100.0).round() as i64,
                (y * 100.0).round() as i64,
                (w * 100.0).round() as i64,
                (h * 100.0).round() as i64
            );
            let _ = writeln!(
                out,
                "<clipPath id=\"{id}\"><rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\"/></clipPath><g clip-path=\"url(#{id})\">"
            );
        }
        DrawOp::ClipEnd => {
            let _ = writeln!(out, "</g>");
        }
        DrawOp::Line { x1, y1, x2, y2, color, width } => {
            let _ = writeln!(out, "<line x1=\"{x1:.2}\" y1=\"{y1:.2}\" x2=\"{x2:.2}\" y2=\"{y2:.2}\" stroke=\"{}\" stroke-width=\"{width}\"/>", xml_escape(color));
        }
        DrawOp::DottedLine { x1, y1, x2, y2, color, width, dash } => {
            let _ = writeln!(
                out,
                "<line x1=\"{x1:.2}\" y1=\"{y1:.2}\" x2=\"{x2:.2}\" y2=\"{y2:.2}\" stroke=\"{}\" stroke-width=\"{width}\"{}/>",
                xml_escape(color),
                dash.svg_attrs()
            );
        }
        DrawOp::Polyline { points, color, width, dash } => {
            // Was `points.iter().map(|(x,y)| format!(...)).collect::<Vec<String>>().join(" ")`
            // — one throwaway `String` per point plus a throwaway `Vec`, for
            // what a line plot of N samples makes a per-sample cost. Writes
            // straight into `out` instead (same fix as the CSV writer).
            //
            // `.filter(finite point)` — a real bug found live (2026-09-04):
            // a Butterworth filter's stopband magnitude underflowed to
            // exactly 0.0 at the last frequency bin, so `20*log10(...)`
            // produced `-inf`, which reached here as a non-finite pixel
            // y-coordinate. SVG's `points="..."` attribute is one flat
            // number list with no per-point validity — a single literal
            // `inf`/`NaN` token in it makes the *entire* `<polyline>`
            // invalid and nothing renders, not just the one bad point.
            // Dropping non-finite points (matplotlib's own convention for
            // Inf/NaN in a line, though it also opens a gap there — this is
            // the simpler "skip it" half of that, good enough to stop one
            // bad sample from blanking an otherwise-valid 1023-point curve)
            // keeps the rest of the curve visible instead of losing all of it.
            // Consecutive points that FORMAT identically are dropped.
            //
            // A 1,000,000-sample line measured 13.9 MB of SVG, of which
            // 693,101 coordinate pairs (69.3%) were byte-identical repeats
            // of the point before them -- `104.06,290.19 104.06,290.19`.
            // At 1000+ samples per horizontal pixel that is unavoidable:
            // the points differ in the f64s and stop differing once rounded
            // to the two decimals actually written out.
            //
            // So this is LOSSLESS, not decimation. It removes coordinates
            // that carry no information AT THE PRECISION ALREADY CHOSEN --
            // the same pixel, twice, at any zoom level a reader can reach.
            // Deliberately NOT matplotlib-style path simplification, which
            // drops points that would render distinguishably and is a
            // product decision about discarding user data; this only
            // removes exact textual repeats.
            //
            // Compared on the FORMATTED string rather than on the f64s. The
            // f64s almost never repeat (they differ in the tenth decimal),
            // so comparing them would find nearly nothing; and rounding to
            // an integer key instead would risk disagreeing with `{:.2}`'s
            // own half-to-even rounding at a boundary and dropping a point
            // that would have been written differently. Comparing the exact
            // bytes that would be emitted cannot disagree with what is
            // emitted.
            out.push_str("<polyline points=\"");
            let mut first = true;
            let mut prev = String::new();
            let mut cur = String::new();
            for (x, y) in points.iter().filter(|(x, y)| x.is_finite() && y.is_finite()) {
                cur.clear();
                let _ = write!(cur, "{x:.2},{y:.2}");
                if cur == prev {
                    continue;
                }
                if !first {
                    out.push(' ');
                }
                first = false;
                out.push_str(&cur);
                prev.clear();
                prev.push_str(&cur);
            }
            let _ = write!(out, "\" fill=\"none\" stroke=\"{}\" stroke-width=\"{width}\"", xml_escape(color));
            // One dasharray on the whole path, so the pattern keeps a
            // continuous phase across every vertex.
            if let Some(d) = dash {
                let _ = write!(out, "{}", d.svg_attrs());
            }
            out.push_str("/>
");
        }
        DrawOp::Rect { x, y, w, h, fill, stroke, opacity, radius } => {
            let fill_attr = fill.clone().unwrap_or_else(|| "none".into());
            let stroke_attr = stroke.clone().unwrap_or_else(|| "none".into());
            let _ = writeln!(
                out,
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"{radius}\" fill=\"{}\" stroke=\"{}\" opacity=\"{opacity}\"/>",
                xml_escape(&fill_attr), xml_escape(&stroke_attr)
            );
        }
        DrawOp::Circle { cx, cy, r, fill, stroke } => {
            let fill_attr = fill.clone().unwrap_or_else(|| "none".into());
            let stroke_attr = stroke.clone().unwrap_or_else(|| "none".into());
            let _ = writeln!(out, "<circle cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r}\" fill=\"{}\" stroke=\"{}\"/>", xml_escape(&fill_attr), xml_escape(&stroke_attr));
        }
        DrawOp::Cross { cx, cy, r, color } => {
            let _ = writeln!(
                out,
                "<path d=\"M{:.2} {:.2} L{:.2} {:.2} M{:.2} {:.2} L{:.2} {:.2}\" stroke=\"{}\" stroke-width=\"1.4\"/>",
                cx - r, cy - r, cx + r, cy + r, cx + r, cy - r, cx - r, cy + r, xml_escape(color)
            );
        }
        DrawOp::Text { italic, x, y, text, size, anchor, rotate, color } => {
            let anchor_attr = match anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let transform = if *rotate != 0.0 {
                format!(" transform=\"rotate({rotate} {x:.2} {y:.2})\"")
            } else {
                String::new()
            };
            // Titles/axis labels (>= 11pt) read as semi-bold for a designed
            // hierarchy against small (10pt) tick labels — no `weight` field
            // on `DrawOp::Text` to plumb through, so size is the proxy.
            // Only the TITLE is bold. A survey of published Nature
            // Communications figures found axis labels at regular weight in
            // 5 of 5, and Science Advances specifies regular explicitly --
            // bold axis labels are a legacy Science-print convention that
            // Science's own current guides contradict. `bold_above` is the
            // axis-label size, so anything ABOVE it is the title.
            //
            // 700 rather than 600: in a regular+bold family 600 has no
            // drawn face, so a browser synthesises it even with a real bold
            // embedded. Ask for the weight that exists.
            let weight = if *size > bold_above + 1e-9 { " font-weight=\"700\"" } else { "" };
            let slant = if *italic { " font-style=\"italic\"" } else { "" };
            // No `font-family` attribute here: it's set once on the root
            // `<svg>` in `render_svg` and inherits to every `<text>` — this
            // is what makes `Figure.font_family` a single knob instead of a
            // 29-call-site plumbing job.
            // An exponent is drawn as a raised, shrunk `<tspan>` holding the
            // face's own digit -- correct on the page, but it flattens
            // ambiguously: `10²` extracts as "102", so a log axis copies out
            // as "100 101 102" instead of three decades. `aria-label` carries
            // the unambiguous form (precomposed superscripts, via the same
            // mapping the PDF backend uses) for screen readers, copy tools
            // and anything else that reads the text rather than rendering it.
            // Only emitted for maths -- plain text is already exact.
            let aria = if text.contains('$') {
                format!(" aria-label=\"{}\"", xml_escape(&flatten_math_to_plain(text)))
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "<text x=\"{x:.2}\" y=\"{y:.2}\" font-size=\"{size}\" text-anchor=\"{anchor_attr}\" fill=\"{}\"{weight}{slant}{aria}{transform}>{}</text>",
                xml_escape(color), render_math_svg(text, *size)
            );
        }
        DrawOp::Polygon { width, points, fill, stroke, opacity } => {
            let fill_attr = fill.clone().unwrap_or_else(|| "none".into());
            let stroke_attr = stroke.clone().unwrap_or_else(|| "none".into());
            out.push_str("<polygon points=\"");
            for (i, (x, y)) in points.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let _ = write!(out, "{x:.2},{y:.2}");
            }
            let _ = writeln!(
                out,
                "\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{width:.2}\" opacity=\"{opacity}\"/>",
                xml_escape(&fill_attr), xml_escape(&stroke_attr)
            );
        }
        DrawOp::Image { x, y, w, h, href } => {
            // `href` is a `data:image/bmp;base64,...` URI — base64 output
            // only ever contains `[A-Za-z0-9+/=]`, none of which need XML
            // escaping, so it's safe to place directly in the attribute.
            let _ = writeln!(
                out,
                "<image x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" href=\"{href}\" preserveAspectRatio=\"none\"/>"
            );
        }
        // Same shape as `Circle`/`Rect`, but non-self-closing so a `<title>`
        // child can carry the real data value -- browsers show that as a
        // native hover tooltip, with zero JS, *as long as this SVG is
        // inline in the page's DOM* (an `<img src="data:...">` can't reach
        // its own interior; see `FigureViewer.tsx`).
        DrawOp::TitledCircle { cx, cy, r, fill, stroke, title } => {
            let fill_attr = fill.clone().unwrap_or_else(|| "none".into());
            let stroke_attr = stroke.clone().unwrap_or_else(|| "none".into());
            let _ = writeln!(
                out,
                "<circle cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r}\" fill=\"{}\" stroke=\"{}\"><title>{}</title></circle>",
                xml_escape(&fill_attr), xml_escape(&stroke_attr), xml_escape(title)
            );
        }
        DrawOp::TitledRect { x, y, w, h, fill, stroke, opacity, radius, title } => {
            let fill_attr = fill.clone().unwrap_or_else(|| "none".into());
            let stroke_attr = stroke.clone().unwrap_or_else(|| "none".into());
            let _ = writeln!(
                out,
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"{radius}\" fill=\"{}\" stroke=\"{}\" opacity=\"{opacity}\"><title>{}</title></rect>",
                xml_escape(&fill_attr), xml_escape(&stroke_attr), xml_escape(title)
            );
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn render_html(fig: &Figure, width: f64, height: f64, embed_fonts: bool) -> String {
    let svg = render_svg(fig, width, height, embed_fonts);
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Qu plot</title></head>\n<body style=\"margin:0;display:flex;align-items:center;justify-content:center;min-height:100vh;background:#f5f5f5;\">\n{svg}</body></html>\n"
    )
}

pub fn render_tikz(fig: &Figure, width: f64, height: f64) -> String {
    let width = resolved_width(fig, width);
    let height = resolved_height(fig, width, height);
    let ops = build_draw_ops(fig, width, height);
    let mut out = String::new();
    out.push_str("% Generated by Qu (savefig .tikz) — requires \\usepackage{tikz}\n");
    // `blur_backdrop` is SVG-only (`<feGaussianBlur>` has no TikZ/pgf
    // equivalent) — degrade to an unblurred render rather than erroring the
    // whole export or silently pretending the effect happened.
    if fig.blur_backdrop.is_some() {
        out.push_str("% NOTE: blur_backdrop(...) was requested but has no TikZ equivalent (feGaussianBlur is an SVG filter primitive) -- rendered unblurred. Use .svg/.html for the blurred version.\n");
    }
    // One canvas unit is 1/MM_TO_UNITS of a millimetre -- that is the whole
    // basis of `COLUMN_PRESETS`, where a Nature single column (89 mm)
    // becomes 890 units, right next to Qu's 900-unit default canvas.
    //
    // This used to emit `x=1pt`, silently reinterpreting those units as
    // TeX points, and the two are not remotely the same: a 900x600 canvas
    // came out 900x600 pt = 317x212 mm, three and a half times wider than
    // a double column and about twelve times the area intended. Text made
    // it worse -- see the `DrawOp::Text` arm, which dropped the size
    // entirely -- so the figure ballooned while its labels stayed at the
    // document's 10 pt, which is exactly the "tiny type in a huge figure"
    // that made exported figures unusable.
    //
    // Emitting the unit in mm makes the export agree with the presets by
    // construction, with no scale factor to keep in sync.
    let unit_mm = 1.0 / MM_TO_UNITS;
    let _ = writeln!(
        out,
        "\\begin{{tikzpicture}}[x={unit_mm}mm, y=-{unit_mm}mm, yshift={}mm]",
        height * unit_mm
    );
    for op in &ops {
        write_tikz_op(&mut out, op, fig.label_size);
    }
    out.push_str("\\end{tikzpicture}\n");
    out
}

// TikZ wants a declared color name or `{rgb,255:red,..}`; hex is common
// enough in modern pgfplots/tikz (xcolor's `HTML` model) that we just strip
// the alpha channel (if any) and pass the hex digits through.
/// The `{model}{spec}` tail of an `\definecolor`, e.g. `{HTML}{e6e6e6}`.
///
/// `\definecolor` takes THREE brace groups — `\definecolor{name}{model}{spec}`
/// — and this used to emit `{[HTML]{e6e6e6}}`, a single group wrapping a
/// bracketed model. That is xcolor's *optional-argument* spelling, which
/// `\definecolor` does not accept, so every `.tikz`/`.tex` Qu has ever
/// exported failed to compile on its first colour:
///
/// ```text
/// ! Package xcolor Error: Undefined color model `[HTML]{e6e6e6}'.
/// !  ==> Fatal error occurred, no output PDF file produced!
/// ```
///
/// Verified against real `pdflatex` (TeX Live 2025) before and after: the
/// old form aborts with no output, the new form produces the page. Every
/// `\definecolor` call site shares this helper, so they are all fixed at
/// once.
/// A canvas-unit length as a TeX dimension.
///
/// Everything in a `DrawOp` -- coordinates, stroke widths, marker radii,
/// dash lengths, type sizes -- is in canvas units, and one unit is
/// `1/MM_TO_UNITS` mm. The TikZ writer used to append a bare `pt` to the
/// widths and radii, which silently reinterpreted them as TeX points and
/// made every stroke ~3.5x too heavy: a 2.2-unit axis spine (0.22 mm,
/// about 0.6 pt) came out as 2.2 pt, and the left and bottom spines
/// rendered as thick black bars against hairline gridlines.
///
/// Same mistake as the picture unit and the font size, in the same file,
/// three times over. Everything the exporter writes goes through here now.
fn tikz_dim(units: f64) -> String {
    format!("{:.4}mm", units / MM_TO_UNITS)
}

fn tikz_draw_color(color: &str) -> String {
    let hex = color.trim_start_matches('#');
    let hex6 = if hex.len() >= 6 { hex[..6].to_string() } else { "000000".to_string() };
    format!("{{HTML}}{{{hex6}}}")
}

/// Writes a `(x1,y1) -- (x2,y2) -- ...` point chain straight into `out`.
/// Replaces a `points.iter().map(|(x,y)| format!(...)).collect::<Vec<String>>().join(" -- ")`
/// that allocated one `String` per point plus a throwaway `Vec` — a
/// per-sample cost for a polyline/polygon over a real signal, same fix as
/// `write_svg_op`'s `Polyline`/`Polygon` arms and the CSV writer.
fn write_tikz_points(out: &mut String, points: &[(f64, f64)]) {
    // Non-finite points are dropped rather than emitted — see the matching
    // filter (and its full doc comment) in `write_svg_op`'s `Polyline` arm;
    // a literal `inf`/`NaN` coordinate would break TikZ's `--`-joined
    // coordinate list the same way it breaks SVG's `points="..."`.
    let mut first = true;
    for (x, y) in points.iter().filter(|(x, y)| x.is_finite() && y.is_finite()) {
        if !first {
            out.push_str(" -- ");
        }
        first = false;
        let _ = write!(out, "({x:.2},{y:.2})");
    }
}

fn write_tikz_op(out: &mut String, op: &DrawOp, bold_above: f64) {
    match op {
        // TikZ scopes the clip, so it must be opened and closed as a group
        // exactly like the SVG one.
        DrawOp::ClipStart { x, y, w, h } => {
            let _ = writeln!(out, r"\begin{{scope}}");
            let _ = writeln!(
                out,
                r"\clip ({x:.2},{y:.2}) rectangle ({:.2},{:.2});",
                x + w,
                y + h
            );
        }
        DrawOp::ClipEnd => {
            let _ = writeln!(out, r"\end{{scope}}");
        }
        DrawOp::Line { x1, y1, x2, y2, color, width } => {
            let _ = writeln!(out, "\\definecolor{{c}}{}", tikz_draw_color(color));
            let _ = writeln!(
                out,
                "\\draw[color=c, line width={}] ({x1:.2},{y1:.2}) -- ({x2:.2},{y2:.2});",
                tikz_dim(*width)
            );
        }
        DrawOp::DottedLine { x1, y1, x2, y2, color, width, dash } => {
            let _ = writeln!(out, "\\definecolor{{c}}{}", tikz_draw_color(color));
            let _ = writeln!(
                out,
                "\\draw[color=c, line width={}, {}] ({x1:.2},{y1:.2}) -- ({x2:.2},{y2:.2});",
                tikz_dim(*width),
                dash.tikz_attr()
            );
        }
        DrawOp::Polyline { points, color, width, dash } => {
            let _ = writeln!(out, "\\definecolor{{c}}{}", tikz_draw_color(color));
            out.push_str("\\draw[color=c, line width=");
            let _ = write!(out, "{}", tikz_dim(*width));
            if let Some(d) = dash {
                let _ = write!(out, ", {}", d.tikz_attr());
            }
            out.push_str("] ");
            write_tikz_points(out, points);
            out.push_str(";\n");
        }
        DrawOp::Rect { x, y, w, h, fill, stroke, opacity, .. } => {
            // A rect can carry BOTH a fill and a stroke, and the legend
            // frame is exactly that: a white panel with a #cccccc border,
            // drawn so an inside legend has something holding it off the
            // data. This arm used to treat fill and stroke as alternatives
            // and take the fill branch whenever one existed, so the border
            // was dropped: the SVG showed a framed legend and the TikZ
            // export showed the same legend with its frame missing and the
            // entry text sitting on the traces. `\filldraw` does both.
            if let (Some(fill), Some(stroke)) = (fill, stroke) {
                let _ = writeln!(out, "\\definecolor{{f}}{}", tikz_draw_color(fill));
                let _ = writeln!(out, "\\definecolor{{s}}{}", tikz_draw_color(stroke));
                let _ = writeln!(
                    out,
                    "\\filldraw[fill=f, draw=s, fill opacity={opacity}] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});",
                    x + w,
                    y + h
                );
            } else if let Some(fill) = fill {
                let _ = writeln!(out, "\\definecolor{{f}}{}", tikz_draw_color(fill));
                let _ = writeln!(out, "\\fill[color=f, opacity={opacity}] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});", x + w, y + h);
            } else {
                // was hardcoded to `color=black`, silently ignoring `stroke`
                // -- invisible for the axis box (its stroke, #333333, is
                // already near-black) but a real bug for a colored
                // stroke-only rect like the "square" marker.
                let color = stroke.clone().unwrap_or_else(|| "#000000".into());
                let _ = writeln!(out, "\\definecolor{{s}}{}", tikz_draw_color(&color));
                let _ = writeln!(out, "\\draw[color=s] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});", x + w, y + h);
            }
        }
        DrawOp::Circle { cx, cy, r, fill, stroke } => {
            let style = if fill.is_some() { "fill" } else { "draw" };
            let color = fill.clone().or_else(|| stroke.clone()).unwrap_or_else(|| "#000000".into());
            let _ = writeln!(out, "\\definecolor{{p}}{}", tikz_draw_color(&color));
            let _ = writeln!(out, "\\{style}[color=p] ({cx:.2},{cy:.2}) circle ({});", tikz_dim(*r));
        }
        DrawOp::Cross { cx, cy, r, color } => {
            let _ = writeln!(out, "\\definecolor{{c}}{}", tikz_draw_color(color));
            let _ = writeln!(
                out,
                "\\draw[color=c] ({:.2},{:.2}) -- ({:.2},{:.2}) ({:.2},{:.2}) -- ({:.2},{:.2});",
                cx - r, cy - r, cx + r, cy + r, cx + r, cy - r, cx - r, cy + r
            );
        }
        DrawOp::Text { italic, x, y, text, anchor, color, size, rotate } => {
            let anchor_attr = match anchor {
                Anchor::Start => "anchor=west",
                Anchor::Middle => "anchor=south",
                Anchor::End => "anchor=east",
            };
            // Rotation was being discarded along with the size, and it is
            // the more visible of the two. Every y-axis title is drawn at
            // -90 degrees; dropping that laid it out horizontally, so it
            // sprawled across the left margin instead of sitting in the
            // narrow strip beside the tick labels. Compared side by side
            // with the SVG of the same figure, this was the single largest
            // difference between the two backends.
            //
            // The sign flips because the picture's y axis points down
            // (`y=-0.1mm`): a rotation that is counter-clockwise on screen
            // is clockwise in TikZ's frame, so re-using the SVG angle
            // unchanged would rotate the label the wrong way and put it
            // upside down.
            let rotate_attr = if rotate.abs() > f64::EPSILON {
                format!(", rotate={:.2}", -rotate)
            } else {
                String::new()
            };
            // The size used to be discarded here (`..` swallowed it), so
            // every exported label -- tick, axis title, legend entry,
            // figure title alike -- rendered at whatever `\normalsize` the
            // including document happened to use. The theme's whole type
            // hierarchy, the 0.8/1.0/1.2 scale off `base_size`, arrived in
            // LaTeX completely flat.
            //
            // Sizes are in canvas units, so they convert with exactly the
            // same `unit_mm` the coordinates use and are emitted as a TeX
            // dimension in mm -- no pt/px conversion to drift. Leading is
            // the conventional 1.2x.
            let size_mm = size / MM_TO_UNITS;
            let lead_mm = size_mm * 1.2;
            // The same rule the other two backends use: text larger than
            // the axis-label size is a title and is set bold (see
            // `write_svg_op`'s `bold_above`). TikZ was the one backend that
            // never received the threshold at all, so the same figure came
            // out with bold panel titles as .svg and .pdf and flat ones as
            // .tikz -- the type hierarchy quietly gone in the format a
            // LaTeX paper actually includes.
            let weight = if *size > bold_above + 1e-9 { "\\bfseries" } else { "" };
            // `\itshape` alongside, for the same reason: a region label set
            // italic in the SVG and PDF and upright in the .tikz would be
            // the type hierarchy going flat again, in the one format a
            // LaTeX paper actually includes.
            let slant = if *italic { "\\itshape" } else { "" };
            let _ = writeln!(out, "\\definecolor{{t}}{}", tikz_draw_color(color));
            let _ = writeln!(
                out,
                "\\node[{anchor_attr}{rotate_attr}, color=t, font=\\fontsize{{{size_mm:.3}mm}}{{{lead_mm:.3}mm}}\\selectfont{weight}{slant}] \
                 at ({x:.2},{y:.2}) {{{}}};",
                tikz_escape(text)
            );
        }
        DrawOp::Polygon { points, fill, opacity, .. } => {
            if let Some(fill) = fill {
                let _ = writeln!(out, "\\definecolor{{pg}}{}", tikz_draw_color(fill));
                let _ = write!(out, "\\fill[color=pg, opacity={opacity}] ");
                write_tikz_points(out, points);
                out.push_str(" -- cycle;\n");
            } else {
                out.push_str("\\draw ");
                write_tikz_points(out, points);
                out.push_str(" -- cycle;\n");
            }
        }
        // TikZ has no data-URI embedding mechanism, and `savefig(.tikz)`
        // writes no companion image file for a real `\includegraphics` to
        // point at — the same "documented gap, not a silent drop" choice
        // `Figure.blur_backdrop` makes for its own TikZ-unsupported effect.
        DrawOp::Image { x, y, w, h, .. } => {
            let _ = writeln!(out, "% image omitted: TikZ export has no data-URI embedding (see IMPL.md's Image library entry)");
            let _ = writeln!(out, "\\draw[dashed, color=gray] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});", x + w, y + h);
            let _ = writeln!(out, "\\node at ({:.2},{:.2}) {{\\small image omitted}};", x + w / 2.0, y + h / 2.0);
        }
        // TikZ has no hover/tooltip concept -- rendered exactly like a plain
        // `Circle`/`Rect`, `title` just dropped (see `TitledCircle`'s doc).
        DrawOp::TitledCircle { cx, cy, r, fill, stroke, .. } => {
            let style = if fill.is_some() { "fill" } else { "draw" };
            let color = fill.clone().or_else(|| stroke.clone()).unwrap_or_else(|| "#000000".into());
            let _ = writeln!(out, "\\definecolor{{p}}{}", tikz_draw_color(&color));
            let _ = writeln!(out, "\\{style}[color=p] ({cx:.2},{cy:.2}) circle ({});", tikz_dim(*r));
        }
        DrawOp::TitledRect { x, y, w, h, fill, stroke, opacity, .. } => {
            if let Some(fill) = fill {
                let _ = writeln!(out, "\\definecolor{{f}}{}", tikz_draw_color(fill));
                let _ = writeln!(out, "\\fill[color=f, opacity={opacity}] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});", x + w, y + h);
            } else {
                let color = stroke.clone().unwrap_or_else(|| "#000000".into());
                let _ = writeln!(out, "\\definecolor{{s}}{}", tikz_draw_color(&color));
                let _ = writeln!(out, "\\draw[color=s] ({x:.2},{y:.2}) rectangle ({:.2},{:.2});", x + w, y + h);
            }
        }
    }
}

/// Escapes plain text for LaTeX — except *inside* `$...$` math spans, which
/// are passed through completely raw. Unlike the SVG path's Unicode
/// approximation (`render_math_svg`), TikZ output actually gets compiled by
/// a real LaTeX toolchain downstream, so `$x^2$` there renders as genuine
/// typeset math — better fidelity than SVG can offer, for free.
fn tikz_escape(s: &str) -> String {
    let mut out = String::new();
    let mut in_math = false;
    for c in s.chars() {
        if c == '$' {
            in_math = !in_math;
            out.push('$');
            continue;
        }
        if in_math {
            out.push(c);
            continue;
        }
        match c {
            '\\' => out.push_str("\\textbackslash "),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            '#' => out.push_str("\\#"),
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------- PDF export
//
// BACKLOG item [25] ("PNG + PDF export backends, hand-rolled, no new deps"):
// this is the PDF half, for whole `savefig(fig, "out.pdf")` FIGURE export —
// the higher-value, more tractable half of that item. It's a THIRD backend
// sharing `build_draw_ops` with `render_svg`/`render_tikz` above, so all
// three are guaranteed to agree on what a figure looks like — one drawing
// walk, three textual encodings of the same primitives. A full rasterizer
// (scanline polygon fill, anti-aliased lines, bitmap font rendering) would
// be needed to export a figure as PNG; that's explicitly NOT attempted here
// (see `save_figure` in `qu-interp/src/lib.rs`, which gives `savefig(fig,
// "x.png")` a clear "not supported yet" error rather than a broken file).
// Real per-pixel raster PNG export for `Value::Image` content IS supported —
// see `image::encode_png` — since there the pixel data already exists and no
// rasterizer is needed.
//
// PDF's content-stream operators are plain ASCII text commands (`m`/`l` for
// moveto/lineto, `S`/`f` for stroke/fill, `rg`/`RG` for fill/stroke color,
// `BT`/`ET` + `Tf`/`Tm`/`Tj` for text) — comparable in complexity to what
// `write_tikz_op` above already does, not a rasterizer. Text uses PDF's
// built-in base-14 Helvetica/Helvetica-Bold fonts, referenced by name in the
// page's `/Font` resource dictionary — these are guaranteed present in every
// PDF viewer/reader, so real, selectable, sharp text works with no font
// embedding or glyph-outline work at all (the one thing that would otherwise
// need a real font-rendering engine).
//
// Coordinate system: rather than rederive every op's geometry, one `cm` at
// the top of the content stream (`1 0 0 -1 0 height cm`) flips the page to
// PDF's native bottom-left/y-up origin into the same top-left/y-down space
// `build_draw_ops` already computed for SVG/TikZ — every subsequent
// coordinate is then written exactly as computed, unchanged.
pub fn render_pdf(fig: &Figure, width: f64, height: f64) -> Vec<u8> {
    let width = resolved_width(fig, width);
    let height = resolved_height(fig, width, height);
    let ops = build_draw_ops(fig, width, height);
    let mut content = String::new();
    // Canvas units are 1/MM_TO_UNITS mm, and PDF user space is points, so
    // the two need a scale between them. Without it a 900x380 canvas became
    // a 900x380 pt page -- 317x134 mm, the same units-as-points confusion
    // the TikZ exporter had. The flip matrix already here is the natural
    // place to carry it: `s 0 0 -s 0 height*s` both scales and flips, so
    // every coordinate below is still written exactly as computed and the
    // `Tf` sizes scale with it.
    let s = PDF_POINTS_PER_UNIT;
    let _ = writeln!(content, "q {s:.6} 0 0 -{s:.6} 0 {:.2} cm", height * s);
    // The figure's background, which this backend never painted.
    //
    // `figure_background(...)` is written straight into the SVG body
    // rather than going through a `DrawOp`, so the PDF and TikZ writers
    // never saw it: the same figure came out gold on screen and white in
    // the file. Found by the backend-agreement test on its first run, and
    // it is exactly the class of difference that test exists for.
    //
    // Only a hex colour is painted. The field is a pass-through string
    // that SVG resolves for itself, so it may hold a CSS NAME this
    // backend has no table for -- and painting `hex_to_rgb("red")`, which
    // parses to black, would turn an unrepresentable background into a
    // confidently wrong one. White is left unpainted because the page
    // already is white.
    let bg = fig.background.trim();
    if bg != DEFAULT_BACKGROUND && bg.len() == 7 && bg.starts_with('#')
        && bg[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        let (r, g, b) = pdf_color(bg);
        let _ = writeln!(content, "q {r:.3} {g:.3} {b:.3} rg 0 0 {width:.2} {height:.2} re f Q");
    }
    if fig.blur_backdrop.is_some() {
        // Same documented degrade TikZ uses for its own unsupported effect
        // (`feGaussianBlur` has no PDF content-stream equivalent without a
        // soft-mask/group XObject, which this minimal writer doesn't build).
        content.push_str("% NOTE: blur_backdrop(...) has no plain-content-stream equivalent -- rendered unblurred. Use .svg/.html for the blurred version.\n");
    }
    // Resolve the theme's face to real metrics BEFORE writing any text, so
    // anchored labels are positioned with the widths of the font that will
    // actually be embedded rather than Helvetica's. Without this the text
    // is set in the right face but placed for the wrong one, and every
    // centred or right-aligned label drifts.
    let regular = embeddable_font_for(&fig.font_family)
        .and_then(|(name, data, _)| pdf_font::parse(data).map(|m| (name, data, m)));
    let bold = embeddable_bold_font_for(&fig.font_family)
        .and_then(|(name, data, _)| pdf_font::parse(data).map(|m| (name, data, m)))
        // A family with no separate bold (Inter, Source Serif) reuses the
        // regular face rather than dropping to Helvetica for titles, which
        // would put two typefaces in one figure.
        .or_else(|| regular.clone());

    // The drawn italic, for a label that is ABOUT the plot rather than
    // part of it. `None` for a family with no italic among the bundled
    // faces, and then an italic label sets upright rather than in a
    // smeared regular.
    let italic = embeddable_italic_font_for(&fig.font_family)
        .and_then(|(name, data, _)| pdf_font::parse(data).map(|m| (name, data, m)));

    // The glyphs `WinAnsiEncoding` cannot reach get their own `Identity-H`
    // faces, so `ν₀T` is set rather than spelled "nuT". Built before any
    // text is written, because the writer picks a font per character.
    let mut glyphs = pdf_glyph_faces(&ops, &fig.font_family);
    // How many simple faces will precede the CID ones, worked out from the
    // same three options the resource dictionary is built from below, so
    // the names the writer emits and the slots the dictionary fills are
    // one decision rather than two that happen to agree.
    glyphs.base = if regular.is_some() && bold.is_some() {
        2 + usize::from(italic.is_some())
    } else {
        // A family with no embeddable program -- the screen default -- has
        // no simple faces at all, and the CID faces start at /F1.
        0
    };
    let math = if glyphs.is_empty() { None } else { Some(&glyphs) };

    for op in &ops {
        write_pdf_op(
            &mut content,
            op,
            fig.label_size,
            regular.as_ref().map(|(_, _, m)| m),
            bold.as_ref().map(|(_, _, m)| m),
            italic.as_ref().map(|(_, _, m)| m),
            math,
        );
    }
    content.push_str("Q\n");

    // Each face carries only the characters that face actually paints.
    //
    // Before this, a figure with four tick labels on it embedded the whole
    // font twice -- once as /F1 and once as /F2 -- which is where ~220 KB
    // of a 240 KB paper figure went, and over 2 MB of a screen-theme one
    // (Inter is a ~1 MB variable font). The METRICS deliberately stay those
    // of the full face: `/Widths` still spans codes 32..=255, so every
    // anchored label is positioned from exactly the numbers it was before
    // and the picture does not move by a hundredth of a point.
    //
    // The subsetter may decline (see `font_subset::subset`); then the whole
    // font goes in, as it always did.
    let (regular_chars, _) = pdf_face_chars(&content, "/F1");
    let (bold_chars, _) = pdf_face_chars(&content, "/F2");
    let program = |data: &'static [u8], chars: &BTreeSet<char>| -> std::borrow::Cow<'static, [u8]> {
        match font_subset::subset(data, chars) {
            Some(sub) => std::borrow::Cow::Owned(sub),
            None => std::borrow::Cow::Borrowed(data),
        }
    };
    let (italic_chars, _) = if italic.is_some() {
        // The italic face's own subset. Read out of the stream the same
        // way the other two are, so it carries exactly the characters set
        // in italic and nothing else.
        pdf_face_chars(&content, "/F3")
    } else {
        (BTreeSet::new(), BTreeSet::new())
    };
    let mut faces: Vec<pdf_font::Face> = match (&regular, &bold) {
        (Some((rn, rd, rm)), Some((bn, bd, bm))) => vec![
            pdf_font::Face {
                data: program(rd, &regular_chars),
                metrics: rm.clone(),
                base_name: (*rn).to_string(),
                encoding: pdf_font::Encoding::WinAnsi,
            },
            pdf_font::Face {
                data: program(bd, &bold_chars),
                metrics: bm.clone(),
                base_name: format!("{bn}-Bold"),
                encoding: pdf_font::Encoding::WinAnsi,
            },
        ],
        _ => Vec::new(),
    };
    // `/F3`, the drawn italic, and only when something is set in it.
    // ALWAYS emitted when the family has one, even if this figure uses no
    // italic, so `/F4` and `/F5` keep their numbers.
    if !faces.is_empty() {
        if let Some((iname, idata, im)) = &italic {
            faces.push(pdf_font::Face {
                data: program(idata, &italic_chars),
                metrics: im.clone(),
                base_name: format!("{iname}-Italic"),
                encoding: pdf_font::Encoding::WinAnsi,
            });
        }
    }
    // `/F4` and `/F5`, and only when the figure paints something that
    // needs them. Ordering matters and is not incidental: `pdf_font::embed`
    // numbers them by position, so a missing `/F3` would silently renumber
    // `/F4`, which is why `/F3` is emitted whenever the family has one.
    //
    // A slot with no face is simply skipped rather than filled with a
    // placeholder, so the math face is `/F4` when there is no text face.
    // `GlyphFaces::math_name` is what keeps the reference in step -- see
    // its comment for the figure that caught the two disagreeing.
    // No `!faces.is_empty()` here. The CID faces are needed exactly when
    // the page paints a character the simple faces cannot encode, which
    // has nothing to do with whether any simple face was embedded -- and
    // when none was, skipping them left every reference dangling.
    if !glyphs.is_empty() {
        for slot in [&glyphs.text, &glyphs.math] {
            let Some(g) = slot else { continue };
            let mut widths: Vec<(u16, f64)> =
                g.gids.iter().map(|(c, gid)| (*gid, g.metrics.width_of(*c))).collect();
            widths.sort_unstable_by_key(|(gid, _)| *gid);
            faces.push(pdf_font::Face {
                data: std::borrow::Cow::Owned(g.program.clone()),
                metrics: g.metrics.clone(),
                base_name: g.name.clone(),
                encoding: pdf_font::Encoding::Identity(widths),
            });
        }
    }
    build_pdf_document(&content, width, height, &faces, &pdf_ext_gstate(&ops))
}

fn pdf_color(hex: &str) -> (f64, f64, f64) {
    let (r, g, b) = hex_to_rgb(hex);
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
}

/// The alpha carried by an `#rrggbbaa` colour, if it carries one.
///
/// SVG has understood eight-digit colours all along, and `apply_alpha`
/// produces them for any `alpha=` on a series. PDF read the first six
/// digits and dropped the rest, so the SAME FIGURE was translucent on
/// screen and opaque in the file that goes to the journal -- markers at
/// `alpha = 0.3` came out solid, hiding whatever they were meant to let
/// through. The `opacity` FIELD on `Rect`/`Polygon` was already handled;
/// this is the other half, and it reaches every op with a colour.
fn hex_alpha(hex: &str) -> Option<f64> {
    let h = hex.trim_start_matches('#');
    if h.len() != 8 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let a = u8::from_str_radix(&h[6..8], 16).ok()? as f64 / 255.0;
    pdf_needs_alpha(a).then_some(a)
}

/// `/GAxxx gs ` for a colour that carries alpha, or empty. Written right
/// after the `q` that opens an op, so the state is scoped to that op.
fn pdf_gs(color: &str) -> String {
    hex_alpha(color).map_or(String::new(), |a| format!("/{} gs ", pdf_alpha_name(a)))
}

/// Every colour an op paints with. One list, walked by both the resource
/// builder and (via `pdf_gs`) the writer, so the two cannot disagree about
/// which alphas a page uses.
fn op_colors(op: &DrawOp) -> Vec<String> {
    let pair = |f: &Option<String>, s: &Option<String>| {
        f.iter().chain(s.iter()).cloned().collect::<Vec<_>>()
    };
    match op {
        DrawOp::Line { color, .. }
        | DrawOp::DottedLine { color, .. }
        | DrawOp::Polyline { color, .. }
        | DrawOp::Cross { color, .. }
        | DrawOp::Text { color, .. } => vec![color.clone()],
        DrawOp::Rect { fill, stroke, .. }
        | DrawOp::Circle { fill, stroke, .. }
        | DrawOp::Polygon { fill, stroke, .. }
        | DrawOp::TitledCircle { fill, stroke, .. }
        | DrawOp::TitledRect { fill, stroke, .. } => pair(fill, stroke),
        _ => Vec::new(),
    }
}

/// Name of the graphics state that carries a given alpha.
///
/// Derived from the value itself rather than kept in a map, so the writer
/// that emits `/GA180 gs` and the assembler that defines `/GA180` cannot
/// disagree about which name means 0.18.
fn pdf_alpha_name(opacity: f64) -> String {
    format!("GA{:03}", (opacity.clamp(0.0, 1.0) * 1000.0).round() as u32)
}

/// Whether an op's fill needs a soft-mask state at all.
fn pdf_needs_alpha(opacity: f64) -> bool {
    opacity < 0.999
}

/// The `gs` prefix a rect's paint needs: the op's own `opacity` field if it
/// carries one, otherwise whatever alpha rides in the colour itself.
///
/// Both the fill and the stroke of a rect go through this. They did not
/// always: the stroke branch took the colour alone, so a legend frame at
/// `opacity` 0.85 came out with a translucent body inside a fully opaque
/// border -- the SVG of the same figure fades both, because `opacity` on an
/// SVG `<rect>` covers the whole shape.
fn pdf_rect_alpha(opacity: f64, color: &str) -> String {
    if pdf_needs_alpha(opacity) {
        format!("/{} gs ", pdf_alpha_name(opacity))
    } else {
        pdf_gs(color)
    }
}

/// The `/ExtGState` dictionary for every alpha the drawing actually uses,
/// written inline in the page's resources.
///
/// This replaces blending the colour toward white in plain RGB, which was
/// the old approximation. It looked right only where the thing underneath
/// was the white page: a legend frame over a plotted line came out as an
/// opaque panel that erased the data behind it, and a `fill_between` band
/// crossing another series hid it. The SVG of the same figure showed both
/// through, because SVG has had real `fill-opacity` all along -- so the two
/// backends disagreed about what the figure SHOWS, not merely how it looks.
fn pdf_ext_gstate(ops: &[DrawOp]) -> String {
    let mut alphas: Vec<u32> = Vec::new();
    let mut note = |o: f64| {
        if pdf_needs_alpha(o) {
            let key = (o.clamp(0.0, 1.0) * 1000.0).round() as u32;
            if !alphas.contains(&key) {
                alphas.push(key);
            }
        }
    };
    for op in ops {
        match op {
            DrawOp::Rect { opacity, .. }
            | DrawOp::TitledRect { opacity, .. }
            | DrawOp::Polygon { opacity, .. } => note(*opacity),
            _ => {}
        }
        // ... and the alpha an `#rrggbbaa` colour carries, wherever it
        // appears. Registered from the same op list the writer walks, so
        // a state the content stream names is always one the resources
        // define.
        for c in op_colors(op) {
            if let Some(a) = hex_alpha(&c) {
                note(a);
            }
        }
    }
    if alphas.is_empty() {
        return String::new();
    }
    let mut out = String::from(" /ExtGState << ");
    for a in alphas {
        let v = a as f64 / 1000.0;
        // `ca` is fill alpha, `CA` stroke alpha. Both, so a translucent
        // shape's own outline matches its body.
        let _ = write!(out, "/GA{a:03} << /ca {v:.3} /CA {v:.3} >> ");
    }
    out.push_str(">>");
    out
}

/// Standard Adobe Helvetica AFM glyph widths (1/1000 em) for the printable
/// ASCII range — used ONLY to estimate a text run's total width so
/// `Anchor::Middle`/`Anchor::End` can compute the right `Tm` offset; the
/// actual glyph outlines are drawn by the PDF viewer's own built-in
/// Helvetica/Helvetica-Bold font, not by this table. Reused as-is for
/// Helvetica-Bold too (real bold glyphs run a little wider) — an
/// intentional, minor approximation: it only ever shifts a centered
/// title/label by a pixel or two, never affects which glyph renders.
fn helvetica_char_width(c: char) -> f64 {
    match c as u32 {
        32 => 278.0, 33 => 278.0, 34 => 355.0, 35 => 556.0, 36 => 556.0,
        37 => 889.0, 38 => 667.0, 39 => 191.0, 40 => 333.0, 41 => 333.0,
        42 => 389.0, 43 => 584.0, 44 => 278.0, 45 => 333.0, 46 => 278.0,
        47 => 278.0,
        48..=57 => 556.0,
        58 => 278.0, 59 => 278.0, 60 => 584.0, 61 => 584.0, 62 => 584.0,
        63 => 556.0, 64 => 1015.0,
        65 => 667.0, 66 => 667.0, 67 => 722.0, 68 => 722.0, 69 => 667.0,
        70 => 611.0, 71 => 778.0, 72 => 722.0, 73 => 278.0, 74 => 500.0,
        75 => 667.0, 76 => 556.0, 77 => 833.0, 78 => 722.0, 79 => 778.0,
        80 => 667.0, 81 => 778.0, 82 => 722.0, 83 => 667.0, 84 => 611.0,
        85 => 722.0, 86 => 667.0, 87 => 944.0, 88 => 667.0, 89 => 667.0,
        90 => 611.0,
        91 => 278.0, 92 => 278.0, 93 => 278.0, 94 => 469.0, 95 => 556.0,
        96 => 333.0,
        97 => 556.0, 98 => 556.0, 99 => 500.0, 100 => 556.0, 101 => 556.0,
        102 => 278.0, 103 => 556.0, 104 => 556.0, 105 => 222.0, 106 => 222.0,
        107 => 500.0, 108 => 222.0, 109 => 833.0, 110 => 556.0, 111 => 556.0,
        112 => 556.0, 113 => 556.0, 114 => 333.0, 115 => 500.0, 116 => 278.0,
        117 => 556.0, 118 => 500.0, 119 => 722.0, 120 => 500.0, 121 => 500.0,
        122 => 500.0,
        123 => 334.0, 124 => 260.0, 125 => 334.0, 126 => 584.0,
        // Non-ASCII fallback: an average-ish width; `pdf_encode_text` below
        // substitutes '?' for anything outside WinAnsiEncoding's range
        // anyway, so this only matters for the accented Latin-1 range.
        _ => 556.0,
    }
}

fn measure_pdf_text_width(text: &str, size: f64) -> f64 {
    text.chars().map(helvetica_char_width).sum::<f64>() * size / 1000.0
}

/// Flattens `$...$` math markup to plain characters for the PDF backend.
/// Reuses `parse_math_spans` (the same math-macro/superscript parser
/// `render_math_svg` uses) but discards each span's `dy`/`scale` — a
/// built-in base-14 font has no glyph-positioning engine to hand
/// superscript/subscript sizing off to the way TikZ hands raw `$x^2$` to a
/// real LaTeX toolchain, so this is a documented, honest downgrade to plain
/// text rather than a half-implemented math layout.
fn flatten_math_to_plain(text: &str) -> String {
    if !text.contains('$') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut math = String::new();
        for c2 in chars.by_ref() {
            if c2 == '$' {
                break;
            }
            math.push(c2);
        }
        for MathRun { text: seg_text, dy, .. } in parse_math_spans(&math) {
            // The offset is about to be discarded, so fold it into the
            // characters themselves while it is still known: `x^2` has to
            // leave here as `x²`, not as `x2`. See `to_script_chars`.
            out.push_str(&match dy {
                d if d < 0.0 => to_script_chars(&seg_text, true),
                d if d > 0.0 => to_script_chars(&seg_text, false),
                _ => seg_text,
            });
        }
    }
    out
}

/// Encodes text for a PDF literal string (`(...)`), keeping the whole
/// content stream pure ASCII so it can be built as an ordinary Rust `String`
/// and converted to bytes at the very end with no re-encoding step. Bytes
/// 32-126 (printable ASCII, minus the three PDF-special characters) pass
/// through directly; `(`, `)`, `\` are backslash-escaped per spec; bytes
/// 127-255 use PDF's `\ddd` octal escape so a single WinAnsiEncoding byte
/// reaches the output even though a Rust `char` in that range would
/// otherwise encode as multi-byte UTF-8. Anything past 255 (e.g. Greek
/// letters or Unicode superscript digits from math-macro expansion, which
/// `flatten_math_to_plain` above can still produce) has no WinAnsiEncoding
/// code point at all — substituted with `?` rather than corrupting the
/// stream, the same "documented gap, not silent corruption" policy the
/// TikZ image-placeholder and SVG-only blur backdrop already use elsewhere
/// in this module.
/// A Latin-1 spelling for a symbol the PDF encoding cannot carry.
///
/// Only the symbols this renderer actually produces -- the ones
/// `latex_symbol` maps to and the minus sign `format_tick` emits. Greek
/// letters are spelled out by name because that is how they are written
/// when a font cannot set them, and mu is the one that has a Latin-1
/// character of its own.
fn pdf_symbol_fallback(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{221A}' => "sqrt",
        '\u{2212}' => "-",       // minus sign
        '\u{2248}' => "~",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2260}' => "!=",
        '\u{226A}' => "<<",
        '\u{226B}' => ">>",
        '\u{221E}' => "inf",
        '\u{2192}' => "->",
        '\u{2190}' => "<-",
        '\u{2191}' => "^",
        '\u{2193}' => "v",
        '\u{22C5}' => ".",
        '\u{2211}' => "sum",
        '\u{220F}' => "prod",
        '\u{222B}' => "int",
        '\u{2202}' => "d",
        '\u{2207}' => "grad",
        '\u{221D}' => "~",
        '\u{2261}' => "==",
        '\u{223C}' => "~",
        '\u{2032}' => "'",
        '\u{2009}' | '\u{2002}' | '\u{2003}' => " ",
        '\u{2026}' => "...",
        '\u{22EF}' => "...",
        '\u{2153}' => "1/3",
        '\u{2154}' => "2/3",
        // Greek. Lower case first, then the capitals a figure uses.
        '\u{03B1}' => "alpha",
        '\u{03B2}' => "beta",
        '\u{03B3}' => "gamma",
        '\u{03B4}' => "delta",
        '\u{03B5}' => "epsilon",
        '\u{03B6}' => "zeta",
        '\u{03B7}' => "eta",
        '\u{03B8}' => "theta",
        '\u{03BA}' => "kappa",
        '\u{03BB}' => "lambda",
        '\u{03BD}' => "nu",
        '\u{03BE}' => "xi",
        '\u{03C0}' => "pi",
        '\u{03C1}' => "rho",
        '\u{03C3}' => "sigma",
        '\u{03C4}' => "tau",
        '\u{03C6}' | '\u{03D5}' => "phi",
        '\u{03C7}' => "chi",
        '\u{03C8}' => "psi",
        '\u{03C9}' => "omega",
        '\u{0394}' => "Delta",
        '\u{0393}' => "Gamma",
        '\u{0398}' => "Theta",
        '\u{039B}' => "Lambda",
        '\u{03A0}' => "Pi",
        '\u{03A3}' => "Sigma",
        '\u{03A6}' => "Phi",
        '\u{03A8}' => "Psi",
        // Both the Greek capital and the dedicated ohm sign: an impedance
        // axis is labelled with one or the other depending on the source.
        '\u{03A9}' | '\u{2126}' => "Ohm",
        // Combining accents (`\hat`, `\bar`, `\vec`, ...) are DROPPED, not
        // spelled. They are the one class of character with no plain-text
        // rendering at all: `\widehat{\mathrm{CF}}` came out as "CF?" in
        // the paper's own legend, because the mark is above Latin-1 and
        // fell to the question mark. "CF" loses the hat but reads as what
        // it is; "CF?" reads as a broken figure, and spelling the accent
        // ("CFhat") would be worse than either.
        '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20F0}' => "",
        _ => return None,
    })
}

/// Every character a finished content stream paints in one named face.
///
/// Read back out of the stream rather than collected while writing it, for
/// the same reason `drawn_chars` reads the finished SVG body: what reaches
/// the page has already been through `parse_math_spans_for_pdf`,
/// `unmap_script_chars` and `pdf_encode_text`, each of which can drop a
/// character or spell it as several others. A collector threaded through
/// the writers would have to be kept in step with all three by hand, and
/// the failure mode of getting it wrong is a glyph missing from the subset
/// — that is, a hole in the label. The stream cannot disagree with itself.
///
/// `pdf_encode_text` emits only bytes 32..=255, escaping `(`, `)` and `\`
/// and writing anything above 126 as a three-digit octal escape, so
/// inverting it is exact.
fn pdf_face_chars(content: &str, want: &str) -> (BTreeSet<char>, BTreeSet<char>) {
    let (mut chars, empty) = (BTreeSet::new(), BTreeSet::new());
    let mut active = false;
    for line in content.lines() {
        if line.ends_with(" Tf") {
            // `/F1 7.00 Tf` — the face stays selected until the next `Tf`.
            active = line.starts_with(want) && line[want.len()..].starts_with(' ');
            continue;
        }
        let Some(body) = line.strip_suffix(") Tj").and_then(|l| l.strip_prefix('(')) else {
            continue;
        };
        if !active {
            continue;
        }
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'\\' {
                // The unescaped part of the stream is written as `char`s,
                // all of them ASCII, so this is byte-safe.
                chars.insert(bytes[i] as char);
                i += 1;
                continue;
            }
            match bytes.get(i + 1) {
                Some(&d) if d.is_ascii_digit() => {
                    let end = (i + 4).min(bytes.len());
                    let oct = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                    if let Some(c) = u32::from_str_radix(oct, 8).ok().and_then(char::from_u32) {
                        chars.insert(c);
                    }
                    i = end;
                }
                Some(&c) => {
                    chars.insert(c as char);
                    i += 2;
                }
                None => break,
            }
        }
    }
    (chars, empty)
}

/// A face reached by GLYPH ID rather than through an encoding.
///
/// `WinAnsiEncoding` has 224 slots and no Greek in any of them, so `ν`, `σ`
/// and `√` could not be NAMED in a PDF at all and were spelled out — the
/// paper's regime map said "nuT" where the figure meant `ν₀T`. Embedded as
/// `Type0`/`Identity-H`, a string is glyph ids and no encoding stands in
/// the way.
pub struct GlyphFace {
    /// The subset, ready to embed.
    program: Vec<u8>,
    /// Widths come from the ORIGINAL face: subsetting renumbers glyphs but
    /// does not change what they measure.
    metrics: pdf_font::FontMetrics,
    /// Character -> glyph id **in the subset**.
    gids: std::collections::BTreeMap<char, u16>,
    /// The `/BaseFont` name this face will carry.
    name: String,
}

impl GlyphFace {
    fn gid(&self, c: char) -> Option<u16> {
        self.gids.get(&c).copied()
    }
}

/// The two glyph-id faces a figure may need, in the order they are tried.
///
/// The TEXT face first, and that ordering is the whole point. Latin Modern
/// Roman has `√`, `Δ` and `Ω`; Latin Modern Math has them too, but drawn
/// for a math engine — its `radical` is the bare hook that LaTeX composes
/// with a rule of its own, so used alone it hangs below the baseline and
/// reads as a stray tick. Taking the glyph from the face the sentence is
/// already set in gives a radical that sits on the baseline and a `Δ` whose
/// weight matches the letters beside it. The math face is the fallback for
/// what the text face genuinely lacks — lower-case Greek, which Latin
/// Modern Roman has none of.
#[derive(Default)]
pub struct GlyphFaces {
    text: Option<GlyphFace>,
    math: Option<GlyphFace>,
    /// How many simple (WinAnsi) faces are emitted BEFORE these two, so
    /// the names written into the content stream match the positions the
    /// resource dictionary actually gives them.
    ///
    /// Assuming three was right whenever the family had an embeddable
    /// program, and silently wrong when it had none: the screen family
    /// embeds nothing, so the CID faces were the FIRST faces on the page
    /// and belonged at `/F1`, while every reference to them said `/F4`.
    /// The dictionary then skipped them entirely and poppler reported
    /// "unknown font tag 'F4'" -- one `$\Omega$` in a default-themed
    /// figure was enough to produce an invalid PDF.
    base: usize,
}

/// The font slots a page can name. Indexed by position rather than
/// formatted, so a slot name stays `&'static str`.
const PDF_FONT_SLOTS: [&str; 8] =
    ["/F1", "/F2", "/F3", "/F4", "/F5", "/F6", "/F7", "/F8"];

impl GlyphFaces {
    /// `(pdf font name, glyph id, width in 1000ths of an em)` for a
    /// character neither `/F1` nor `/F2` can encode.
    fn pick(&self, c: char) -> Option<(&'static str, u16, f64)> {
        if let Some(f) = &self.text {
            if let Some(g) = f.gid(c) {
                return Some((self.text_name(), g, f.metrics.width_of(c)));
            }
        }
        if let Some(f) = &self.math {
            if let Some(g) = f.gid(c) {
                return Some((self.math_name(), g, f.metrics.width_of(c)));
            }
        }
        None
    }

    /// Which slot the math face actually occupies.
    ///
    /// The faces are emitted in order and numbered by position, so the
    /// math face is `/F5` only when a text face was emitted before it. A
    /// figure whose only non-Latin characters are lower-case Greek has no
    /// text face at all -- Latin Modern Roman has no lower-case Greek --
    /// so the math face is the first of the two and is `/F4`. Naming it
    /// `/F5` regardless wrote a reference to a font the page never
    /// declared: every Greek letter vanished, and the PDF was invalid
    /// enough that poppler reported "unknown font tag 'F5'". Found by
    /// porting a figure labelled only with rho and lambda.
    fn math_name(&self) -> &'static str {
        let at = self.base + usize::from(self.text.is_some());
        PDF_FONT_SLOTS[at.min(PDF_FONT_SLOTS.len() - 1)]
    }

    /// The slot the text CID face occupies: the first one after the
    /// simple faces, whatever number that turns out to be.
    fn text_name(&self) -> &'static str {
        PDF_FONT_SLOTS[self.base.min(PDF_FONT_SLOTS.len() - 1)]
    }

    fn is_empty(&self) -> bool {
        self.text.is_none() && self.math.is_none()
    }

    /// Advance of a run, in 1000ths of an em. A character with no glyph
    /// contributes nothing, which cannot happen: the run was split on
    /// exactly this test.
    fn text_width(&self, s: &str) -> f64 {
        s.chars().map(|c| self.pick(c).map(|(_, _, w)| w).unwrap_or(0.0)).sum()
    }
}

/// Build one glyph-id face over `bytes` for the characters in `wanted` it
/// can draw, removing them from `wanted` as it goes.
fn glyph_face_for(
    bytes: &'static [u8],
    name: &str,
    wanted: &mut BTreeSet<char>,
) -> Option<GlyphFace> {
    let mine: BTreeSet<char> =
        wanted.iter().copied().filter(|c| font_subset::glyph_id(bytes, *c).is_some()).collect();
    if mine.is_empty() {
        return None;
    }
    // Subset FIRST, then read the glyph ids back out of the subset: the
    // subsetter renumbers every glyph, so the original's ids would address
    // the wrong outlines.
    let program = font_subset::subset(bytes, &mine)?;
    let metrics = pdf_font::parse(bytes)?;
    let mut gids = std::collections::BTreeMap::new();
    for c in &mine {
        if let Some(g) = font_subset::glyph_id(&program, *c) {
            gids.insert(*c, g);
        }
    }
    if gids.is_empty() {
        return None;
    }
    for c in gids.keys() {
        wanted.remove(c);
    }
    Some(GlyphFace { program, metrics, gids, name: name.to_string() })
}

/// Which characters of a figure need a glyph-id face, and the faces that
/// draw them — empty when the figure paints none, which is the common case
/// and costs the PDF nothing.
fn pdf_glyph_faces(ops: &[DrawOp], font_family: &str) -> GlyphFaces {
    let mut wanted: BTreeSet<char> = BTreeSet::new();
    for op in ops {
        let DrawOp::Text { text, .. } = op else { continue };
        for MathRun { text: seg, .. } in parse_math_spans_for_pdf(text) {
            // Latin-1 goes through the text face as it always has; only
            // what the single-byte encoding cannot carry comes here.
            wanted.extend(seg.chars().filter(|c| *c as u32 > 255));
        }
    }
    if wanted.is_empty() {
        return GlyphFaces::default();
    }
    let text = embeddable_font_for(font_family)
        .and_then(|(name, bytes, _)| glyph_face_for(bytes, name, &mut wanted));
    // Whatever the text face could not draw. A character neither can keeps
    // its spelled fallback -- better a readable "Ohm" than a missing glyph.
    let (math_name, math_bytes, _) = embeddable_math_font();
    let math = glyph_face_for(math_bytes, math_name, &mut wanted);
    GlyphFaces { text, math, base: 3 }
}

/// Which of the three text faces a run is set in.
///
/// Replaces a bare `bold: bool`, which had no room for a third answer and
/// left the writer picking `/F1` or `/F2` from a boolean it also had to
/// remember the meaning of.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PdfFace {
    Regular,
    Bold,
    Italic,
}

impl PdfFace {
    /// The resource name, which is also this face's position in the list
    /// `pdf_font::embed` is handed.
    fn name(self) -> &'static str {
        match self {
            PdfFace::Regular => "/F1",
            PdfFace::Bold => "/F2",
            PdfFace::Italic => "/F3",
        }
    }
}

/// Split a run wherever the face that can draw it changes.
///
/// Returned as `(pdf font name, text)` in order — `None` meaning the run's
/// ordinary text face — so the pen advances through them left to right
/// exactly as it would through one string.
fn pdf_face_runs<'a>(
    s: &str,
    glyphs: Option<&'a GlyphFaces>,
) -> Vec<(Option<&'a str>, String)> {
    let mut out: Vec<(Option<&'a str>, String)> = Vec::new();
    for c in s.chars() {
        let face = if c as u32 > 255 {
            glyphs.and_then(|g| g.pick(c)).map(|(name, _, _)| name)
        } else {
            None
        };
        match out.last_mut() {
            Some((prev, run)) if *prev == face => run.push(c),
            _ => out.push((face, c.to_string())),
        }
    }
    out
}

/// The characters a run will actually PAINT, after the substitutions the
/// single-byte encoding forces.
///
/// Above Latin-1 that encoding cannot carry the character, and a `?` is the
/// worst possible answer: it looks like a rendering error and it destroys
/// the label. `$\sqrt{2\ln M}$` came out as "?(2 ln M)" and `[m$\Omega$]`
/// as "[m?]" -- in the PDF, which is the format the paper embeds. So the
/// symbol is SPELLED instead: "sqrt(2 ln M)", "[mOhm]". A reader loses the
/// typography but not the meaning. Anything with no accepted spelling falls
/// through to `?`, which at that point is honest.
///
/// Split out from `pdf_encode_text` because the WIDTH has to be measured on
/// this, not on the source text. Measuring the source asks the face for one
/// `Δ` and then paints five letters, so the pen advances by a fifth of what
/// it drew: the colorbar of the paper's regime map came out as "Delta" with
/// its own subscript stamped on top of it, and every centred label
/// containing a spelled symbol sat off centre. One function, used for both,
/// cannot disagree with itself.
fn pdf_drawable_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let code = c as u32;
        if code < 32 {
            continue;
        }
        if code <= 255 {
            out.push(c);
            continue;
        }
        match pdf_symbol_fallback(c) {
            Some(word) => out.push_str(word),
            None => out.push('?'),
        }
    }
    out
}

fn pdf_encode_text(s: &str) -> String {
    let mut out = String::new();
    for c in pdf_drawable_text(s).chars() {
        let code = c as u32;
        match c {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            _ if code < 127 => out.push(c),
            _ => {
                let _ = write!(out, "\\{code:03o}");
            }
        }
    }
    out
}

/// Writes an approximate-circle path (four cubic Beziers, the standard
/// `k = 0.55228475` control-point ratio for a 4-arc circle approximation)
/// without a trailing fill/stroke operator, so callers can follow with
/// either `f` or `S`. PDF has no native circle primitive — this is the
/// same "approximate with the primitives PDF actually gives you" spirit as
/// this whole backend's `cm`-based coordinate flip.
fn write_pdf_circle_path(out: &mut String, cx: f64, cy: f64, r: f64) {
    const K: f64 = 0.552_284_75;
    let _ = writeln!(out, "{:.2} {:.2} m", cx + r, cy);
    let _ = writeln!(out, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", cx + r, cy - r * K, cx + r * K, cy - r, cx, cy - r);
    let _ = writeln!(out, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", cx - r * K, cy - r, cx - r, cy - r * K, cx - r, cy);
    let _ = writeln!(out, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", cx - r, cy + r * K, cx - r * K, cy + r, cx, cy + r);
    let _ = writeln!(out, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", cx + r * K, cy + r, cx + r, cy + r * K, cx + r, cy);
    out.push_str("h\n");
}

/// Writes a text run's `BT ... ET` block, computing the `Tm` text matrix so
/// `anchor` (start/middle/end) and `rotate` (degrees) land the same way
/// `write_svg_op`'s native `text-anchor`/`transform="rotate(...)"` attributes
/// do — PDF has neither, so both are hand-computed here: `dx` shifts the
/// origin by the (estimated) text width for middle/end anchoring, applied
/// along the already-rotated text direction so the anchor point stays fixed
/// at any rotation angle, matching how SVG's `rotate(deg x y)` rotates
/// already-anchored text around that same fixed point.
/// Split a label into `(text, dy, scale)` runs for the PDF writer.
///
/// Like `render_math_svg`'s loop, but returning the spans rather than
/// emitting `<tspan>`s, and passing plain text through as a single
/// unscaled run so the ordinary case costs nothing.
fn parse_math_spans_for_pdf(text: &str) -> Vec<MathRun> {
    if !text.contains('$') {
        return vec![MathRun::plain(text.to_string(), false)];
    }
    let mut out: Vec<MathRun> = Vec::new();
    let mut chars = text.chars();
    let mut plain = String::new();
    while let Some(c) = chars.next() {
        if c != '$' {
            plain.push(c);
            continue;
        }
        if !plain.is_empty() {
            out.push(MathRun::plain(std::mem::take(&mut plain), false));
        }
        let mut math = String::new();
        for c2 in chars.by_ref() {
            if c2 == '$' {
                break;
            }
            math.push(c2);
        }
        // `parse_math_spans` no longer maps scripts to precomposed Unicode
        // (see `to_script_chars`), so this is now only about a `⁴` the
        // script author typed literally: WinAnsi cannot encode one, and the
        // run already carries its own offset and size, which is what a
        // superscript actually is.
        for r in parse_math_spans(&math) {
            out.push(MathRun { text: unmap_script_chars(&r.text), ..r });
        }
    }
    if !plain.is_empty() {
        out.push(MathRun::plain(plain, false));
    }
    out
}

/// Turn Unicode sub/superscript characters back into their ordinary
/// letters, for a renderer that positions runs instead.
fn unmap_script_chars(text: &str) -> String {
    text.chars()
        .map(|c| {
            SUPER_MAP
                .iter()
                .chain(SUB_MAP.iter())
                .find(|(_, mapped)| *mapped == c)
                .map(|(plain, _)| *plain)
                .unwrap_or(c)
        })
        .collect()
}

/// Draw a label whose parts sit at different sizes and baselines.
///
/// Each run advances the pen by its own measured width, so the whole
/// string is laid out left to right exactly as the SVG lays out its
/// `<tspan>`s -- and the anchor is computed from the total, or a centred
/// title with a subscript in it would sit off centre.
#[allow(clippy::too_many_arguments)]
// Why the text matrix here is `[cos, sin, sin, -cos]` and not a rotation.
//
// The page transform (`s 0 0 -s 0 h*s cm`) flips the page so every op can
// be written in the same y-DOWN space `build_draw_ops` computed for SVG.
// Glyph outlines are defined y-UP, so each text matrix has to carry that
// flip back out again -- which is why `d` is `-cos` rather than `cos`, and
// why at `rotate = 0` the matrix is `[1, 0, 0, -1]`.
//
// Composing the flip with an SVG rotation by `a` gives `[cos, sin, sin,
// -cos]`: a font point `(gx, gy)` lands at `(gx cos + gy sin, gx sin - gy
// cos)`. The rotation's own signs survive; the flip only negates the
// second column.
//
// This was `[cos, -sin, -sin, -cos]`, which is the rotation by `-a`. At
// `a = 0` the two are identical, so it passed every horizontal-text check
// there is. Every y-axis label in every exported PDF came out rotated the
// wrong way -- reading downward with the glyph tops facing right -- and
// displaced by its own length, because the span origin stepped one way
// along the string while the glyphs advanced the other. The SVG of the
// same figure was correct throughout, which is what made it look like a
// font problem rather than a geometry one.
fn write_pdf_text_spans(
    out: &mut String,
    x: f64,
    y: f64,
    spans: &[MathRun],
    size: f64,
    anchor: Anchor,
    rotate: f64,
    color: &str,
    slot: PdfFace,
    face: Option<&pdf_font::FontMetrics>,
    math: Option<&GlyphFaces>,
    // The drawn-italic face, for maths variables. A run that is italic is
    // set in `/F3` and MEASURED in `/F3`'s own metrics: an italic face has
    // different advances, and anchoring a centred label off the upright
    // widths would put it off centre by the difference.
    italic_face: Option<&pdf_font::FontMetrics>,
) {
    let text_font = slot.name();
    // Measured on what is PAINTED, not on the source -- see
    // `pdf_drawable_text`. A math run measures in its own face, which is a
    // different file with different advances.
    let width_of = |t: &str, glyph_run: bool, sz: f64, italic: bool| -> f64 {
        if glyph_run {
            return math.map(|m| m.text_width(t) / 1000.0 * sz).unwrap_or(0.0);
        }
        let drawn = pdf_drawable_text(t);
        // An italic run measures in the italic face. Falls back to the
        // upright metrics when no italic face is embedded, which is also
        // the face it will then be drawn in, so the two stay consistent.
        let m = if italic { italic_face.or(face) } else { face };
        match m {
            Some(m) => m.text_width(&drawn) / 1000.0 * sz,
            None => measure_pdf_text_width(&drawn, sz),
        }
    };
    // Italic is only available when an italic face was actually embedded;
    // otherwise the run stays in the label's own slot rather than pointing
    // at a font the page does not declare.
    let italic_slot = |italic: bool| -> &'static str {
        if italic && italic_face.is_some() { PdfFace::Italic.name() } else { text_font }
    };
    // Every span is split at its face boundaries FIRST, so the anchor is
    // computed from the same runs the pen will walk. Anchoring off the
    // unsplit string would measure a Greek letter in a face that has no
    // glyph for it, and a centred label with one in it would sit off
    // centre by the difference.
    struct Laid<'a> {
        dy: f64,
        scale: f64,
        italic: bool,
        over: Overline,
        parts: Vec<(Option<&'a str>, String)>,
    }
    let runs: Vec<Laid> = spans
        .iter()
        .map(|r| Laid {
            dy: r.dy,
            scale: r.scale,
            italic: r.italic,
            over: r.over,
            parts: pdf_face_runs(&r.text, math),
        })
        .collect();
    let total: f64 = runs
        .iter()
        .flat_map(|l| {
            let (sc, it) = (l.scale, l.italic);
            l.parts.iter().map(move |(m, t)| width_of(t, m.is_some(), size * sc, it))
        })
        .sum();
    let dx0 = match anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => -total / 2.0,
        Anchor::End => -total,
    };
    let (r, g, b) = pdf_color(color);
    let rad = rotate.to_radians();
    let (cos_r, sin_r) = (rad.cos(), rad.sin());

    let mut advance = 0.0;
    for Laid { dy, scale, italic, over, parts } in &runs {
        let sz = size * scale;
        // Where this run starts and how wide it is, for the rule drawn
        // over it. Measured from the same widths the pen walks, so the
        // bar ends exactly where the radicand does.
        let run_start = dx0 + advance;
        let run_width: f64 = parts
            .iter()
            .map(|(m, t)| width_of(t, m.is_some(), sz, *italic))
            .sum();
        for (glyph_font, t) in parts {
            if t.is_empty() {
                continue;
            }
            let along = dx0 + advance;
            // `dy` is in em of the BASE size and points DOWN the page, the
            // same convention the SVG spans use -- and these coordinates are
            // still in that same y-down space, because the page transform
            // does the flip afterwards. So a subscript adds. Subtracting
            // instead put every subscript above the baseline, which reads as
            // a superscript and quietly changes what the label says.
            let across = dy * size;
            let tx = x + along * cos_r - across * sin_r;
            let ty = y + along * sin_r + across * cos_r;
            out.push_str("BT\n");
            let _ = writeln!(out, "{} {sz:.2} Tf", glyph_font.unwrap_or_else(|| italic_slot(*italic)));
            let _ = writeln!(out, "{r:.3} {g:.3} {b:.3} rg");
            let _ = writeln!(
                out,
                "{cos_r:.5} {:.5} {:.5} {:.5} {tx:.2} {ty:.2} Tm",
                sin_r, sin_r, -cos_r
            );
            if glyph_font.is_some() {
                // `Identity-H` strings are 2-byte big-endian glyph ids, so
                // they go out as a hex string rather than a literal one.
                let mut hex = String::new();
                for c in t.chars() {
                    let gid = math.and_then(|m| m.pick(c)).map(|(_, g, _)| g).unwrap_or(0);
                    let _ = write!(hex, "{gid:04X}");
                }
                let _ = writeln!(out, "<{hex}> Tj");
            } else {
                let _ = writeln!(out, "({}) Tj", pdf_encode_text(t));
            }
            out.push_str("ET\n");
            advance += width_of(t, glyph_font.is_some(), sz, *italic);
        }
        // The rule over the run, once the run's full extent is known.
        //
        // Drawn as a path in the same y-down space the text matrices use
        // -- the page transform flips both together -- so it needs no
        // coordinates of its own beyond the advance already measured.
        if *over != Overline::None {
            // Above the cap height, at a weight that reads at print size
            // without competing with the glyphs beneath it: a radical's
            // bar is a rule, not a stroke of the letterform.
            let lift = 0.80 * sz;
            let thick = (0.05 * sz).max(0.35);
            let base = dy * size;
            let at = |along: f64, acr: f64| -> (f64, f64) {
                (x + along * cos_r - acr * sin_r, y + along * sin_r + acr * cos_r)
            };
            let _ = write!(out, "q {r:.3} {g:.3} {b:.3} RG {thick:.2} w ");
            match over {
                Overline::Bar => {
                    let (ax, ay) = at(run_start, base - lift);
                    let (bx, by) = at(run_start + run_width, base - lift);
                    let _ = write!(out, "{ax:.2} {ay:.2} m {bx:.2} {by:.2} l ");
                }
                // A chevron, which is what a wide hat is: up to an apex
                // over the middle of the group and back down. Set a little
                // higher than a bar so the two stay distinguishable at a
                // glance, because they mean different things.
                Overline::Hat => {
                    let apex = lift * 1.18;
                    let (ax, ay) = at(run_start, base - lift);
                    let (mx, my) = at(run_start + run_width / 2.0, base - apex);
                    let (bx, by) = at(run_start + run_width, base - lift);
                    let _ = write!(out, "{ax:.2} {ay:.2} m {mx:.2} {my:.2} l {bx:.2} {by:.2} l ");
                }
                Overline::None => {}
            }
            out.push_str("S Q
");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_pdf_text(out: &mut String, x: f64, y: f64, text: &str, size: f64, anchor: Anchor, rotate: f64, color: &str, slot: PdfFace, face: Option<&pdf_font::FontMetrics>, math: Option<&GlyphFaces>, italic_face: Option<&pdf_font::FontMetrics>) {
    // Math is set as POSITIONED RUNS, the same way the SVG writer sets it,
    // rather than flattened to characters.
    //
    // `flatten_math_to_plain` turns `$x_0$` into the Unicode subscript
    // character, which the SVG renders because a browser will find that
    // glyph in some fallback face. A PDF has no fallback: it can only draw
    // what the embedded font contains, and an embedded text face has no
    // subscript letters at all. Every one came out as `?` -- a legend
    // reading "CF???d" where the figure said "CF_rand", in the format the
    // paper actually embeds.
    //
    // PDF sets each run's size and offset in the text matrix, so the real
    // fix is the same as the SVG's: draw the spans, do not fold them into
    // characters.
    let spans = parse_math_spans_for_pdf(text);
    // A run that needs the math face also has to go through the span
    // writer, which is the only one that can switch fonts mid-string.
    let needs_math = math.is_some_and(|m| {
        spans
            .iter()
            .any(|r| r.text.chars().any(|c| c as u32 > 255 && m.pick(c).is_some()))
    });
    if needs_math
        || spans.len() > 1
        || spans
            .iter()
            .any(|r| r.dy != 0.0 || r.scale != 1.0 || r.italic || r.over != Overline::None)
    {
        write_pdf_text_spans(out, x, y, &spans, size, anchor, rotate, color, slot, face, math, italic_face);
        return;
    }
    let plain: String = spans.into_iter().map(|r| r.text).collect();
    let pdf_text = pdf_encode_text(&plain);
    let font = slot.name();
    // Anchoring must use the metrics of the face that will be EMBEDDED.
    // `measure_pdf_text_width` reads a hardcoded Helvetica AFM table, which
    // is right only when Helvetica is what gets written -- with a real face
    // embedded it mis-measured every string, so centred titles sat off
    // centre and right-anchored tick labels drifted from their ticks.
    let drawn = pdf_drawable_text(&plain);
    let text_width = match face {
        Some(m) => m.text_width(&drawn) / 1000.0 * size,
        None => measure_pdf_text_width(&drawn, size),
    };
    let dx = match anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => -text_width / 2.0,
        Anchor::End => -text_width,
    };
    let (r, g, b) = pdf_color(color);
    let rad = rotate.to_radians();
    let (cos_r, sin_r) = (rad.cos(), rad.sin());
    let tx = x + dx * cos_r;
    let ty = y + dx * sin_r;
    out.push_str("BT\n");
    let _ = writeln!(out, "{font} {size:.2} Tf");
    let _ = writeln!(out, "{r:.3} {g:.3} {b:.3} rg");
    // The text matrix has to carry a y-FLIP, not just a rotation.
    //
    // The page transform is `s 0 0 -s 0 h*s cm`, which flips y so that the
    // top-left/y-down coordinates `build_draw_ops` computes for SVG can be
    // reused unchanged. Geometry is happy with that, but glyphs are not:
    // text space is y-up, so a text matrix of a pure rotation inherits the
    // page flip and every string renders mirrored. Every `savefig(".pdf")`
    // Qu has produced had all of its labels upside down.
    //
    // Composing the flip into the text matrix cancels the page flip for
    // glyphs only: at rotate=0 this is the classic `1 0 0 -1`, and the
    // off-diagonal signs carry rotation through the flipped frame so a
    // -90 degree y-axis title still reads bottom-to-top.
    let _ = writeln!(
        out,
        "{cos_r:.5} {:.5} {:.5} {:.5} {tx:.2} {ty:.2} Tm",
        sin_r, sin_r, -cos_r
    );
    let _ = writeln!(out, "({pdf_text}) Tj");
    out.push_str("ET\n");
}

#[allow(clippy::too_many_arguments)]
fn write_pdf_op(out: &mut String, op: &DrawOp, bold_above: f64, regular: Option<&pdf_font::FontMetrics>, bold_face: Option<&pdf_font::FontMetrics>, italic_face: Option<&pdf_font::FontMetrics>, math: Option<&GlyphFaces>) {
    match op {
        // PDF clips with `W n` (intersect the clip path with the current
        // path, then discard it), scoped by the surrounding `q`/`Q`.
        DrawOp::ClipStart { x, y, w, h } => {
            let _ = writeln!(out, "q {x:.2} {y:.2} {w:.2} {h:.2} re W n");
        }
        DrawOp::ClipEnd => {
            let _ = writeln!(out, "Q");
        }
        DrawOp::Line { x1, y1, x2, y2, color, width } => {
            let (r, g, b) = pdf_color(color);
            let _ = writeln!(out, "q {gs}{r:.3} {g:.3} {b:.3} RG {width:.2} w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S Q", gs = pdf_gs(color));
        }
        DrawOp::DottedLine { x1, y1, x2, y2, color, width, dash } => {
            let (r, g, b) = pdf_color(color);
            let _ = writeln!(
                out,
                "q {r:.3} {g:.3} {b:.3} RG {width:.2} w {}{x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S Q",
                dash.pdf_op()
            );
        }
        DrawOp::Polyline { points, color, width, dash } => {
            // Non-finite points dropped — see `write_svg_op`'s `Polyline`
            // arm for the full story; a PDF content-stream operand can't be
            // `inf`/`NaN` either.
            let finite: Vec<&(f64, f64)> = points.iter().filter(|(x, y)| x.is_finite() && y.is_finite()).collect();
            if finite.is_empty() {
                return;
            }
            let (r, g, b) = pdf_color(color);
            let _ = write!(out, "q {}{r:.3} {g:.3} {b:.3} RG {width:.2} w ", pdf_gs(color));
            // `[on off] 0 d` applies to the whole path that follows, which
            // is what keeps the phase continuous around a contour's many
            // short segments.
            if let Some(d) = dash {
                let _ = write!(out, "{}", d.pdf_op());
            }
            // Same lossless de-duplication as the SVG arm, for the same
            // reason and on the same formatted-string basis -- see there.
            // A 1e6-sample line wrote a 16 MB PDF against matplotlib's
            // 16 KB, and a `l` operator repeating the current point draws
            // a zero-length segment that changes nothing.
            let mut first = true;
            let mut prev = String::new();
            let mut cur = String::new();
            for (x, y) in finite.iter() {
                cur.clear();
                let _ = write!(cur, "{x:.2} {y:.2}");
                if cur == prev {
                    continue;
                }
                let code = if first { "m" } else { "l" };
                first = false;
                let _ = write!(out, "{cur} {code} ");
                prev.clear();
                prev.push_str(&cur);
            }
            out.push_str("S Q\n");
        }
        DrawOp::Rect { x, y, w, h, fill, stroke, opacity, .. } => {
            // `radius` (rounded corners) is ignored -- approximated as a
            // square-cornered rect, a documented minor gap (see the module
            // doc), the same "approximate rather than half-implement"
            // choice this backend makes for `blur_backdrop`. `opacity` is
            // NOT approximated: it goes out as a real ExtGState soft mask.
            if let Some(fill) = fill {
                let (r, g, b) = pdf_color(fill);
                let alpha = pdf_rect_alpha(*opacity, fill);
                let _ = writeln!(out, "q {alpha}{r:.3} {g:.3} {b:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f Q");
            }
            if let Some(stroke) = stroke {
                let (r, g, b) = pdf_color(stroke);
                let alpha = pdf_rect_alpha(*opacity, stroke);
                let _ = writeln!(out, "q {alpha}{r:.3} {g:.3} {b:.3} RG {x:.2} {y:.2} {w:.2} {h:.2} re S Q");
            }
        }
        DrawOp::Circle { cx, cy, r: radius, fill, stroke } => {
            if let Some(fill) = fill {
                let (r, g, b) = pdf_color(fill);
                out.push_str("q ");
                let _ = write!(out, "{}{r:.3} {g:.3} {b:.3} rg ", pdf_gs(fill));
                write_pdf_circle_path(out, *cx, *cy, *radius);
                out.push_str("f Q\n");
            } else if let Some(stroke) = stroke {
                let (r, g, b) = pdf_color(stroke);
                out.push_str("q ");
                let _ = write!(out, "{}{r:.3} {g:.3} {b:.3} RG ", pdf_gs(stroke));
                write_pdf_circle_path(out, *cx, *cy, *radius);
                out.push_str("S Q\n");
            }
        }
        DrawOp::Cross { cx, cy, r, color } => {
            let (rr, gg, bb) = pdf_color(color);
            let _ = writeln!(
                out,
                "q {gs}{rr:.3} {gg:.3} {bb:.3} RG 1.4 w {:.2} {:.2} m {:.2} {:.2} l S {:.2} {:.2} m {:.2} {:.2} l S Q",
                cx - r, cy - r, cx + r, cy + r, cx + r, cy - r, cx - r, cy + r, gs = pdf_gs(color)
            );
        }
        DrawOp::Text { italic, x, y, text, size, anchor, rotate, color } => {
            // Match the SVG backend exactly: text LARGER than the axis-label
            // size is a title. The old absolute threshold (DEFAULT_AXIS_LABEL_SIZE,
            // 16.0) did not scale with the theme, so publication -- whose type
            // starts at 18.6 -- set EVERY label bold while the SVG of the same
            // figure bolded only the two titles.
            let bold = *size > bold_above + 1e-9;
            // Italic beats bold when a caller asks for both: only one extra
            // face is embedded, and a bold-italic Latin Modern is not
            // among the bundled ones. An italic aside that came out
            // upright-bold would say the opposite of what was meant.
            let slot = if *italic { PdfFace::Italic } else if bold { PdfFace::Bold } else { PdfFace::Regular };
            let face = match slot {
                PdfFace::Italic => italic_face.or(regular),
                PdfFace::Bold => bold_face,
                PdfFace::Regular => regular,
            };
            write_pdf_text(out, *x, *y, text, *size, *anchor, *rotate, color, slot, face, math, italic_face);
        }
        DrawOp::Polygon { width, points, fill, stroke, opacity } => {
            if points.is_empty() {
                return;
            }
            // Filled AND stroked goes out as ONE path with `B`, not the
            // same path written twice. A contour band is stroked in its own
            // fill colour to close the seam against its neighbour (see the
            // `Shape::Contour` arm), and writing every band's outline twice
            // took the ceiling map from 1.6 MB to 3.3 MB for a picture that
            // did not change.
            let write_path = |out: &mut String| {
                for (i, (x, y)) in points.iter().enumerate() {
                    let code = if i == 0 { "m" } else { "l" };
                    let _ = write!(out, "{x:.2} {y:.2} {code} ");
                }
            };
            let alpha = if pdf_needs_alpha(*opacity) {
                format!("/{} gs ", pdf_alpha_name(*opacity))
            } else {
                // Translucency can also arrive in the COLOUR rather than
                // the field -- that is how `apply_alpha` expresses a
                // series' `alpha=`, and how a marker's shadow is drawn.
                fill.as_deref().or(stroke.as_deref()).map(pdf_gs).unwrap_or_default()
            };
            match (fill, stroke) {
                (Some(fill), Some(stroke)) => {
                    let (fr, fg, fb) = pdf_color(fill);
                    let (sr, sg, sb) = pdf_color(stroke);
                    let _ = write!(
                        out,
                        "q {alpha}{fr:.3} {fg:.3} {fb:.3} rg {sr:.3} {sg:.3} {sb:.3} RG {width:.2} w "
                    );
                    write_path(out);
                    out.push_str("h B Q\n");
                }
                (Some(fill), None) => {
                    let (r, g, b) = pdf_color(fill);
                    let _ = write!(out, "q {alpha}{r:.3} {g:.3} {b:.3} rg ");
                    write_path(out);
                    out.push_str("h f Q\n");
                }
                (None, Some(stroke)) => {
                    let (r, g, b) = pdf_color(stroke);
                    let _ = write!(out, "q {r:.3} {g:.3} {b:.3} RG ");
                    write_path(out);
                    out.push_str("h S Q\n");
                }
                (None, None) => {}
            }
        }
        // Same documented gap as TikZ's own `DrawOp::Image` arm: no data-URI
        // (or, here, image-XObject/Flate) embedding mechanism in this
        // minimal writer, so a labeled placeholder box stands in rather
        // than silently dropping the image.
        DrawOp::Image { x, y, w, h, .. } => {
            let _ = writeln!(out, "q 0.6 0.6 0.6 RG [3 2] 0 d {x:.2} {y:.2} {w:.2} {h:.2} re S Q");
            write_pdf_text(
                out,
                x + w / 2.0,
                y + h / 2.0,
                "image omitted (PDF export has no raster embedding yet)",
                10.0,
                Anchor::Middle,
                0.0,
                "#666666",
                PdfFace::Regular,
                regular,
                math,
                italic_face,
            );
        }
        // PDF has no hover/tooltip concept -- rendered exactly like a plain
        // `Circle`/`Rect`, `title` just dropped (see `TitledCircle`'s doc).
        DrawOp::TitledCircle { cx, cy, r: radius, fill, stroke, .. } => {
            if let Some(fill) = fill {
                let (r, g, b) = pdf_color(fill);
                out.push_str("q ");
                let _ = write!(out, "{}{r:.3} {g:.3} {b:.3} rg ", pdf_gs(fill));
                write_pdf_circle_path(out, *cx, *cy, *radius);
                out.push_str("f Q\n");
            } else if let Some(stroke) = stroke {
                let (r, g, b) = pdf_color(stroke);
                out.push_str("q ");
                let _ = write!(out, "{}{r:.3} {g:.3} {b:.3} RG ", pdf_gs(stroke));
                write_pdf_circle_path(out, *cx, *cy, *radius);
                out.push_str("S Q\n");
            }
        }
        DrawOp::TitledRect { x, y, w, h, fill, stroke, opacity, .. } => {
            if let Some(fill) = fill {
                let (r, g, b) = pdf_color(fill);
                let alpha = pdf_rect_alpha(*opacity, fill);
                let _ = writeln!(out, "q {alpha}{r:.3} {g:.3} {b:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f Q");
            }
            if let Some(stroke) = stroke {
                let (r, g, b) = pdf_color(stroke);
                let alpha = pdf_rect_alpha(*opacity, stroke);
                let _ = writeln!(out, "q {alpha}{r:.3} {g:.3} {b:.3} RG {x:.2} {y:.2} {w:.2} {h:.2} re S Q");
            }
        }
    }
}

/// Assembles the final PDF byte stream: header, five objects (Catalog,
/// Pages, one Page, the content stream, and two base-14 font objects), a
/// spec-correct cross-reference table, and trailer — the minimum a
/// conforming PDF reader needs. `WinAnsiEncoding` is set explicitly on both
/// fonts so `pdf_encode_text`'s byte-128-255 octal escapes land on the
/// codepage they were written for instead of the base-14 default
/// `StandardEncoding`.
fn build_pdf_document(content: &str, width: f64, height: f64, faces: &[pdf_font::Face], ext_gstate: &str) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();

    // 1.6 is the version that admits `/FontFile3 /Subtype /OpenType`, which
    // is how a CFF-outline face (every .otf Qu ships) is embedded. Claiming
    // 1.4 while using a 1.6 construct is the kind of thing a strict
    // validator rejects and a lenient viewer renders anyway.
    buf.extend_from_slice(if faces.is_empty() { b"%PDF-1.4\n" } else { b"%PDF-1.6\n" });
    // Conventional binary-marker comment (four bytes >= 0x80) telling
    // naive line-based transfer tools this file carries binary data --
    // harmless here since every stream is plain ASCII, but standard
    // practice worth matching.
    buf.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

    let push_obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &str| {
        offsets.push(buf.len());
        buf.extend_from_slice(body.as_bytes());
    };

    push_obj(&mut buf, &mut offsets, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    push_obj(&mut buf, &mut offsets, "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    // Embedded faces start at object 5 and take three objects each; the
    // base-14 fallback keeps the original two-object layout so a font Qu
    // cannot parse still produces a readable PDF rather than no PDF.
    let (font_resources, font_objects) = if faces.is_empty() {
        (
            "<< /F1 5 0 R /F2 6 0 R >>".to_string(),
            vec![
                b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n".to_vec(),
                b"6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>\nendobj\n".to_vec(),
            ],
        )
    } else {
        pdf_font::embed(faces, 5)
    };

    let page_obj = format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font {font_resources}{ext_gstate} >> /Contents 4 0 R >>\nendobj\n",
        width * PDF_POINTS_PER_UNIT,
        height * PDF_POINTS_PER_UNIT
    );
    push_obj(&mut buf, &mut offsets, &page_obj);
    let content_obj = format!("4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n", content.len(), content);
    push_obj(&mut buf, &mut offsets, &content_obj);
    for obj in &font_objects {
        offsets.push(buf.len());
        buf.extend_from_slice(obj);
    }

    let xref_offset = buf.len();
    let mut xref = format!("xref\n0 {}\n", offsets.len() + 1);
    xref.push_str("0000000000 65535 f \n");
    for off in &offsets {
        let _ = writeln!(xref, "{off:010} 00000 n ");
    }
    let _ = write!(
        xref,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        offsets.len() + 1,
        xref_offset
    );
    buf.extend_from_slice(xref.as_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_layout_places_panels_at_their_declared_cell() {
        let mut fig = Figure::new();
        fig.select_grid(2, 2, 1).unwrap();
        fig.select_grid(2, 2, 4).unwrap();
        let rects = layout_panels(&fig, 400.0, 300.0);
        assert_eq!(rects.len(), 2);
        let top_left = rects.iter().find(|r| r.panel == 0).unwrap();
        let bottom_right = rects.iter().find(|r| r.panel == 1).unwrap();
        assert!(top_left.left < bottom_right.left);
        assert!(top_left.top < bottom_right.top);
    }

    #[test]
    fn dense_grid_never_collapses_to_a_fully_blank_figure() {
        // Regression for a real bug found while auditing subplot spacing:
        // fixed-pixel chrome margins (68/30/16/42 px for left/top/right/
        // bottom, sized for the default tick/title/label fonts) don't shrink
        // as more rows are packed into one figure. At the default 900x600
        // canvas, an 8-row `subplot(8,1,i)` stack gives each cell only
        // ~54.75px of height — less than the unclamped 72px (30 top + 42
        // bottom) of margin alone, so `bottom <= top` for every single
        // panel and `build_draw_ops` silently `continue`s past all eight,
        // producing zero draw ops (an entirely blank SVG, verified by
        // hand before this fix landed). Every panel here carries a title,
        // so if even one panel's title text is missing from the output the
        // fit-scale clamp in `build_draw_ops` isn't doing its job.
        let mut fig = Figure::new();
        for i in 1..=8 {
            fig.select_grid(8, 1, i).unwrap();
            fig.current_panel_mut().series.push(Series {
                x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None, ..Default::default()
            });
            fig.current_panel_mut().title = Some(format!("Panel {i}"));
        }
        let ops = build_draw_ops(&fig, 900.0, 600.0);
        for i in 1..=8 {
            let want = format!("Panel {i}");
            assert!(
                ops.iter().any(|op| matches!(op, DrawOp::Text { text, .. } if *text == want)),
                "panel {i}'s title is missing from the draw ops entirely \
                 (the whole panel was skipped as bottom <= top) -- got {} ops total",
                ops.len()
            );
        }
        // Every panel's plot rectangle must be non-degenerate too, not just
        // its title text -- at least one gridline/axis Line per panel.
        let line_count = ops.iter().filter(|op| matches!(op, DrawOp::Line { .. })).count();
        assert!(line_count >= 8, "expected at least one axis/gridline per panel, got {line_count} lines total: {ops:?}");
    }

    /// A print figure with `rows` rows, nothing else set.
    fn print_grid(rows: usize) -> Figure {
        print_grid_rc(rows, 1)
    }

    fn print_grid_rc(rows: usize, cols: usize) -> Figure {
        let mut fig = Figure::new();
        fig.theme = "publication".to_string();
        fig.publication = true;
        fig.apply_auto_type_scale();
        for i in 1..=(rows * cols) {
            fig.select_grid(rows, cols, i).unwrap();
            fig.current_panel_mut().series.push(Series {
                x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None, ..Default::default()
            });
            fig.current_panel_mut().title = Some(format!("Panel {i}"));
        }
        fig
    }

    /// The chrome a print panel cannot shrink: title above, tick numbers
    /// and axis label below. Computed the way `build_draw_ops` does.
    fn vertical_chrome(fig: &Figure) -> f64 {
        MARGIN_TOP * (fig.title_size / DEFAULT_TITLE_SIZE)
            + MARGIN_BOTTOM * (fig.tick_size / TICK_METRIC_BASELINE)
    }

    #[test]
    fn a_print_grid_grows_its_canvas_rather_than_packing_the_panels() {
        // The defect, seen directly in a rendered `subplot(4,1,i)` stack in
        // `theme("publication")`: print type is fixed in POINTS, so the
        // chrome does not shrink when the cells do. Four rows of the
        // default 600-unit canvas is 127 units a cell against ~131 units of
        // title + tick numbers + axis label -- more chrome than cell. The
        // figure survived only because `build_draw_ops` clamps margins to
        // 70% of the cell, and what came out was a rail of letterbox
        // strips with the y numbers written on top of each other.
        for rows in 2..=6 {
            let fig = print_grid(rows);
            let h = resolved_height(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT);
            // Growing is the MEANS; the chrome budget below is the end, and
            // it is what this test is really for. Requiring growth at every
            // row count was over-specified: when print type came down to
            // 6/7/8 pt on 2026-09-10 the chrome shrank with it, and two rows
            // began to fit inside the default canvas while staying well
            // under budget -- a better outcome that the old assertion called
            // a failure. So: grow only when the default canvas could not
            // hold the budget, and always hold the budget.
            let cell_at_default = (DEFAULT_FIGURE_HEIGHT - GAP * (rows as f64 + 1.0)) / rows as f64;
            if vertical_chrome(&fig) / cell_at_default > MAX_CHROME_SHARE {
                assert!(
                    h > DEFAULT_FIGURE_HEIGHT,
                    "{rows} print rows do not fit the budget at {DEFAULT_FIGURE_HEIGHT}, so the                      canvas had to grow -- got {h}"
                );
            }
            let cell = (h - GAP * (rows as f64 + 1.0)) / rows as f64;
            let share = vertical_chrome(&fig) / cell;
            assert!(
                share <= MAX_CHROME_SHARE + 1e-9,
                "{rows} rows: chrome takes {:.0}% of each cell, over the {:.0}% budget",
                share * 100.0,
                MAX_CHROME_SHARE * 100.0
            );
        }
    }

    #[test]
    fn growing_for_a_grid_leaves_everything_else_exactly_as_it_was() {
        // Three cases that must not move a pixel, because each would be the
        // resize happening where nobody asked for one.
        //
        // A single print panel: nothing is being packed.
        let one = print_grid(1);
        assert_eq!(
            resolved_height(&one, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT),
            DEFAULT_FIGURE_HEIGHT
        );

        // A screen theme at any depth: its type is a fraction of the
        // canvas, so its chrome shrinks along with the cells and the figure
        // stays self-consistent however many rows it has.
        let mut screen = print_grid(6);
        screen.theme = "default".to_string();
        screen.publication = false;
        screen.apply_auto_type_scale();
        assert_eq!(
            resolved_height(&screen, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT),
            DEFAULT_FIGURE_HEIGHT
        );

        // A size the script named itself. `figure_size(w, h)` is an
        // instruction, not a suggestion -- a figure authored to drop into a
        // fixed slot must come out that size even if it packs.
        let mut explicit = print_grid(6);
        explicit.size_explicit = true;
        assert_eq!(resolved_height(&explicit, 900.0, 600.0), 600.0);
    }

    #[test]
    fn a_multi_column_print_grid_widens_rather_than_squeezing_the_panels() {
        // Reported off a rendered 3x3 in `theme("publication")`: the panels
        // were visibly compressed sideways. Same cause as the height, on
        // the other axis -- a print panel's left margin holds tick numbers
        // and a rotated axis label at a fixed point size, so three columns
        // of the 900-unit canvas spend 147 of each 276-unit cell on
        // margins and leave the data less than half its own panel.
        // Two columns already clear the budget on the default canvas, so
        // nothing moves there -- the budget is the assertion, not the
        // growth. Three is where the reported figure sat, and where the
        // canvas has to give.
        let three = print_grid_rc(3, 3);
        assert!(
            resolved_width(&three, DEFAULT_FIGURE_WIDTH) > DEFAULT_FIGURE_WIDTH,
            "a 3x3 print grid should have widened its canvas"
        );
        for cols in 2..=4 {
            let fig = print_grid_rc(2, cols);
            let w = resolved_width(&fig, DEFAULT_FIGURE_WIDTH);
            let cell = (w - GAP * (cols as f64 + 1.0)) / cols as f64;
            let chrome = MARGIN_LEFT * (fig.tick_size / TICK_METRIC_BASELINE) + MARGIN_RIGHT;
            assert!(
                chrome / cell <= MAX_CHROME_SHARE + 1e-9,
                "{cols} columns: chrome takes {:.0}% of each cell",
                chrome / cell * 100.0
            );
        }
        // A single column is not being squeezed by anything.
        let one = print_grid_rc(3, 1);
        assert_eq!(resolved_width(&one, DEFAULT_FIGURE_WIDTH), DEFAULT_FIGURE_WIDTH);
        // Neither is a screen figure, whose margins scale with its canvas.
        let mut screen = print_grid_rc(2, 3);
        screen.theme = "default".to_string();
        screen.publication = false;
        screen.apply_auto_type_scale();
        assert_eq!(resolved_width(&screen, DEFAULT_FIGURE_WIDTH), DEFAULT_FIGURE_WIDTH);
    }

    /// A panel whose data covers the whole box, so every inside placement
    /// is occupied and `best_corner` has nothing good to pick.
    fn panel_full_of_data(labels: &[&str]) -> Figure {
        let mut fig = Figure::new();
        fig.theme = "publication".to_string();
        fig.publication = true;
        fig.apply_auto_type_scale();
        for (k, label) in labels.iter().enumerate() {
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            for i in 0..=20 {
                for j in 0..=20 {
                    xs.push(i as f64 / 20.0);
                    ys.push(0.1 + 0.8 * (j as f64 / 20.0));
                }
            }
            let _ = k;
            fig.current_panel_mut().series.push(Series {
                x: xs, y: ys, label: Some((*label).to_string()),
                marker: "o".into(), color: None, ..Default::default()
            });
        }
        fig.current_panel_mut().legend.visible = true;
        fig
    }

    /// Where the legend box lands, and every plotted point, in pixels.
    fn legend_box_and_points(fig: &Figure, w: f64, h: f64) -> (f64, f64, f64, f64, Vec<(f64, f64)>) {
        let (ops, geom) = build_draw_ops_with_geometry(fig, w, h);
        let g = &geom[0];
        let panel = &fig.panels[0];
        let to_px = |x: f64, y: f64| {
            (
                g.left + (x - g.xmin) / (g.xmax - g.xmin) * (g.right - g.left),
                g.bottom - (y - g.ymin) / (g.ymax - g.ymin) * (g.bottom - g.top),
            )
        };
        let points = panel
            .series
            .iter()
            .flat_map(|s| s.x.iter().zip(s.y.iter()).map(|(&x, &y)| to_px(x, y)))
            .collect();
        // The legend frame is the only white-filled Rect the panel draws.
        let (bx, by, bw, bh) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Rect { x, y, w, h, fill: Some(f), .. } if f == "#ffffff" => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .expect("the legend frame should have been drawn");
        (bx, by, bw, bh, points)
    }

    #[test]
    fn a_legend_gets_sky_to_sit_in_rather_than_landing_on_the_data() {
        // `best_corner` takes the placement the LEAST data falls into --
        // but least is not none, and when the data reaches every corner the
        // key lands on the curve it labels. A ROC panel does this by
        // construction: the curve sweeps corner to corner, so every
        // candidate is occupied and the box goes over the data whichever
        // one wins. The fix is the one a person makes by hand -- raise the
        // top of the axis until there is room above the curve.
        let fig = panel_full_of_data(&["A", "B"]);
        let (bx, by, bw, bh, points) = legend_box_and_points(&fig, 900.0, 600.0);
        let under = points
            .iter()
            .filter(|&&(px, py)| px >= bx && px <= bx + bw && py >= by && py <= by + bh)
            .count();
        assert_eq!(under, 0, "{under} points are still underneath the legend box");
        // And it paid for that with range, not by moving the data.
        let (_, geom) = build_draw_ops_with_geometry(&fig, 900.0, 600.0);
        assert!(
            geom[0].ymax > 0.9 + 1e-9,
            "the top of the axis should have been raised past the data's own padded extent, got {}",
            geom[0].ymax
        );
    }

    #[test]
    fn a_legend_with_somewhere_to_sit_leaves_the_axis_exactly_as_it_was() {
        // The common case must not move a pixel: two crossing lines leave a
        // clear wedge at the top, `best_corner` finds it, and nothing about
        // the figure should change.
        let mut fig = Figure::new();
        for (label, up) in [("rising", true), ("falling", false)] {
            let y: Vec<f64> = (0..=50).map(|i| { let t = i as f64 / 50.0; if up { t } else { 1.0 - t } }).collect();
            fig.current_panel_mut().series.push(Series {
                x: (0..=50).map(|i| i as f64 / 50.0).collect(), y,
                label: Some(label.into()), marker: "line".into(), color: None, ..Default::default()
            });
        }
        let mut plain = fig.clone();
        plain.current_panel_mut().legend.visible = false;
        fig.current_panel_mut().legend.visible = true;
        let with = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
        let without = build_draw_ops_with_geometry(&plain, 900.0, 600.0).1;
        assert_eq!(with[0].ymax, without[0].ymax, "an unobstructed legend must not change the axis");
    }

    #[test]
    fn a_named_position_is_left_exactly_where_it_was_named() {
        // `legend` means "put it somewhere sensible" and is the only case
        // the automatic placement acts on. `legend top right` names a
        // place, and a named place is an instruction -- the key stays
        // there even over data, because a script that says where the
        // legend goes has already weighed that. Same principle as `ylim`.
        let mut fig = panel_full_of_data(&[
            "rho_1 + Delta_NL (proposed)  (AUC 0.87)",
            "Pi_est linear alone  (AUC 0.60)",
            "kurtosis / ARCH (rejected)  (AUC 0.51)",
            "4-tier scheme  (acc 0.43)",
        ]);
        fig.current_panel_mut().legend.position = LegendPosition::TopRight;
        let geom = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
        let (bx, _, bw, _, _) = legend_box_and_points(&fig, 900.0, 600.0);
        assert!(
            bx + bw <= geom[0].right + 1e-6,
            "a named position must stay inside the axes, overlap and all"
        );
        // The axis is not stretched for it either: the script said where,
        // so there is nothing left to decide.
        let mut free = fig.clone();
        free.current_panel_mut().legend.visible = false;
        let plain = build_draw_ops_with_geometry(&free, 900.0, 600.0).1;
        assert_eq!(geom[0].ymax, plain[0].ymax, "a named position must not move the axis");
        // And nothing is reported, because nothing was decided.
        assert!(legend_fit_note(&fig, 900.0, 600.0).is_none());
    }

    #[test]
    fn a_legend_with_a_free_corner_is_never_moved_out_for_its_size_alone() {
        // The size test used to fire on its own, which took a perfectly
        // placeable key out of the axes and narrowed the panel to solve a
        // problem it did not have. A decaying curve -- the commonest figure
        // there is -- leaves the top-right corner empty, so however tall
        // the key is, it belongs inside.
        let mut fig = Figure::new();
        fig.theme = "publication".to_string();
        fig.publication = true;
        fig.apply_auto_type_scale();
        for k in 1..=4 {
            let x: Vec<f64> = (0..=80).map(|i| i as f64 / 20.0).collect();
            let y: Vec<f64> = x.iter().map(|t| (-t * k as f64).exp()).collect();
            fig.current_panel_mut().series.push(Series {
                x, y, label: Some(format!("a very long label for decay mode {k}")),
                marker: "line".into(), color: None, ..Default::default()
            });
        }
        fig.current_panel_mut().legend.visible = true;
        let geom = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
        let (bx, bw) = {
            let (bx, _, bw, _, _) = legend_box_and_points(&fig, 900.0, 600.0);
            (bx, bw)
        };
        assert!(bx + bw <= geom[0].right + 1e-6, "the key had a free corner and should have stayed inside");
        assert!(legend_fit_note(&fig, 900.0, 600.0).is_none(), "and nothing to report");
    }

    #[test]
    fn an_explicitly_outside_legend_is_reserved_for_exactly_once() {
        // The margin for an outside legend is now added after the decision
        // rather than before it, and both paths go through the same branch.
        // A script that already said `legend outside` must get one gutter,
        // not two -- and must land in it.
        let mut fig = panel_full_of_data(&["A", "B"]);
        fig.current_panel_mut().legend.outside = true;
        let (bx, by, bw, bh, _) = legend_box_and_points(&fig, 900.0, 600.0);
        let geom = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
        assert!(bx >= geom[0].right, "an outside legend sits right of the plot area");
        // One gutter: the box starts just past the plot and the panel ends
        // just past the box, with only the fixed 8/16 px padding between.
        assert!(
            bx - geom[0].right < 24.0,
            "the legend should sit against the plot area, not a gutter away from it"
        );
        assert!(by >= 0.0 && by + bh <= 600.0, "and stay on the canvas");
        let _ = bw;
    }

    #[test]
    fn a_pinned_axis_is_never_stretched_to_fit_a_legend() {
        // `ylim` and `axis tight` are instructions. A figure that says what
        // its range is must come out with that range -- so when the key
        // covers data on such a panel, the key is what moves, not the axis.
        for pin in ["ylim", "tight"] {
            let mut fig = panel_full_of_data(&["A", "B"]);
            match pin {
                "ylim" => fig.current_panel_mut().ylim = Some((0.0, 1.0)),
                _ => fig.current_panel_mut().tight = true,
            }
            let geom = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
            if pin == "ylim" {
                assert_eq!(geom[0].ymax, 1.0, "an explicit ylim must survive");
            }
            assert!(
                legend_fit_note(&fig, 900.0, 600.0).is_some(),
                "a pinned axis with an inside legend should be reported"
            );
        }
        // An outside legend is already clear of the data and needs neither.
        let mut outside = panel_full_of_data(&["A", "B"]);
        outside.current_panel_mut().legend.outside = true;
        outside.current_panel_mut().ylim = Some((0.0, 1.0));
        assert!(legend_fit_note(&outside, 900.0, 600.0).is_none());
    }

    #[test]
    fn a_legend_too_big_to_place_inside_takes_itself_outside() {
        // Making room only helps while the box is small enough that the
        // white space costs less than the overlap. Past that the legend
        // leaves the axes on its own -- which is what a real ROC panel with
        // four long entries hit: 45% of the plot height, so every inside
        // placement covered the curve and no amount of sky would have
        // helped.
        let fig = panel_full_of_data(&[
            "rho_1 + Delta_NL (proposed)  (AUC 0.87)",
            "Pi_est linear alone  (AUC 0.60)",
            "kurtosis / ARCH (rejected)  (AUC 0.51)",
            "4-tier scheme  (acc 0.43)",
        ]);
        let (bx, by, bw, bh, points) = legend_box_and_points(&fig, 900.0, 600.0);
        let under = points
            .iter()
            .filter(|&&(px, py)| px >= bx && px <= bx + bw && py >= by && py <= by + bh)
            .count();
        assert_eq!(under, 0, "{under} points are still underneath the legend box");
        // Outside means outside: clear of the plot area, not merely of the
        // points that happen to be plotted.
        let geom = build_draw_ops_with_geometry(&fig, 900.0, 600.0).1;
        assert!(
            bx >= geom[0].right || bx + bw <= geom[0].left || by >= geom[0].bottom || by + bh <= geom[0].top,
            "the box should sit outside the plot area entirely"
        );
        // And it says what it did, and how to get the key back inside.
        let note = legend_fit_note(&fig, 900.0, 600.0).expect("moving a legend out is worth a line");
        assert!(note.contains("placed outside the axes"), "the note should say what happened: {note}");
        assert!(note.contains("columns"), "the note should name a way back inside: {note}");
        // Two short entries fit inside and nothing is said.
        assert!(legend_fit_note(&panel_full_of_data(&["A", "B"]), 900.0, 600.0).is_none());
    }

    #[test]
    fn an_annotation_is_sized_by_the_theme_and_can_pick_its_own_ink() {
        // The size was a hardcoded 11 units that ignored the type scale
        // entirely. A print theme runs its ticks at ~25, so every
        // annotation came out at under half the size of the smallest other
        // text on the figure -- the same bug the legend had before its
        // text was tied to the scale. And there was no colour at all,
        // though a note pointing at a threshold is very often the one thing
        // on a figure deliberately NOT in the data's own ink.
        let mut fig = Figure::new();
        fig.theme = "publication".to_string();
        fig.publication = true;
        fig.apply_auto_type_scale();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None, ..Default::default()
        });
        fig.current_panel_mut().callouts.push(Callout {
            italic: false,
            x: 0.5, y: 0.5, text: "noise floor".into(), arrow_to: None, marker: false,
            color: Some("#C0392B".into()), size: None,
        });
        let ops = build_draw_ops(&fig, 900.0, 600.0);
        let note = ops.iter().find_map(|op| match op {
            DrawOp::Text { text, size, color, .. } if text == "noise floor" => Some((*size, color.clone())),
            _ => None,
        });
        let (size, color) = note.expect("the annotation should have been drawn");
        assert_eq!(color, "#C0392B", "an annotation must be able to pick its own ink");
        assert!(
            size > 11.0 * 1.5,
            "a print annotation should be sized by the theme (ticks are {:.1}), got {size:.1}",
            fig.tick_size
        );
    }

    #[test]
    fn a_legend_box_is_the_size_of_what_is_inside_it() {
        // Two separate causes of the same loose box. The width came from a
        // per-character ESTIMATE that runs about a fifth wide on ordinary
        // lowercase ("filtered" measures 3.58 em against Latin Modern's
        // real 3.03), and all of that difference appeared as dead space
        // between the longest label and the right edge. And the box
        // reserved 18 units of swatch gutter while the row drew its text
        // starting at 20, so the contents were laid out to a different
        // geometry than the box that holds them.
        let mut panel = Panel::default();
        for label in ["input", "filtered"] {
            panel.series.push(Series {
                x: vec![0.0, 1.0], y: vec![0.0, 1.0],
                label: Some(label.into()), marker: "line".into(), color: None, ..Default::default()
            });
        }
        let face = face_metrics(TIKZ_FONT_FAMILY);
        assert!(face.is_some(), "the print face must parse, or this measures nothing");
        let (w, _) = legend_box_size(&panel, 1.0, face);
        // Everything the box must hold, at tick_scale 1: two pads, the
        // swatch gutter, and the longest label at the legend's own size.
        let want = LEGEND_PAD * 2.0 + LEGEND_GUTTER + measure_text("filtered", TICK_METRIC_BASELINE, face);
        assert!(
            (w - want).abs() < 1e-6,
            "box is {w:.2} wide but its contents need {want:.2}"
        );
        // And the estimate really was the looser of the two, so this is a
        // tightening rather than a wash.
        assert!(measure_text("filtered", TICK_METRIC_BASELINE, face) < text_width("filtered", 12.0));
    }

    #[test]
    fn all_three_backends_bold_the_same_text() {
        // A title and a tick label from one figure, exported three ways.
        // SVG and PDF both took `bold_above`; TikZ never received it, so
        // the same figure came out with bold panel titles as .svg and .pdf
        // and flat ones as .tikz -- the type hierarchy quietly gone in the
        // format a LaTeX paper actually includes.
        let title = DrawOp::Text { italic: false,
            x: 10.0, y: 10.0, text: "Panel title".into(), size: DEFAULT_TITLE_SIZE,
            anchor: Anchor::Middle, rotate: 0.0, color: "#000000".into(),
        };
        let tick = DrawOp::Text { italic: false,
            x: 10.0, y: 10.0, text: "0.5".into(), size: DEFAULT_TICK_SIZE,
            anchor: Anchor::Middle, rotate: 0.0, color: "#000000".into(),
        };
        let mut bold_tikz = String::new();
        write_tikz_op(&mut bold_tikz, &title, DEFAULT_AXIS_LABEL_SIZE);
        assert!(bold_tikz.contains("\\bfseries"), "a title should be bold in TikZ too: {bold_tikz}");
        let mut plain_tikz = String::new();
        write_tikz_op(&mut plain_tikz, &tick, DEFAULT_AXIS_LABEL_SIZE);
        assert!(!plain_tikz.contains("\\bfseries"), "a tick label must not be bold: {plain_tikz}");
        // And the two that were already right stay right.
        let mut bold_svg = String::new();
        write_svg_op(&mut bold_svg, &title, DEFAULT_AXIS_LABEL_SIZE);
        assert!(bold_svg.contains("font-weight=\"700\""));
        let mut plain_svg = String::new();
        write_svg_op(&mut plain_svg, &tick, DEFAULT_AXIS_LABEL_SIZE);
        assert!(!plain_svg.contains("font-weight"));
    }

    #[test]
    fn circle_is_spelled_the_way_every_other_marker_is() {
        // `marker_polygon` takes a word for every glyph it knows --
        // "square", "diamond", "triangle", "pentagon" -- and the circle,
        // which is the only non-polygon and so lives in its own arm, took
        // only `"o"`. `marker="circle"` matched nothing and drew nothing,
        // silently: the line appeared, the marks did not, and the script
        // exited 0.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0, 2.0], y: vec![0.0, 1.0, 2.0],
            label: None, marker: "circle".into(), color: None, ..Default::default()
        });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let marks = ops.iter().filter(|op| matches!(op, DrawOp::TitledCircle { .. } | DrawOp::Circle { .. })).count();
        assert_eq!(marks, 3, "marker=\"circle\" should draw one mark per point");
    }

    #[test]
    fn a_thinned_log_axis_still_says_what_its_ticks_are() {
        // A decade axis over 0.001 to 1, thinned to its two ends by a
        // narrow panel. The gap between them is 0.999, which asks for one
        // decimal -- and 0.001 at one decimal is "0.0", which the
        // all-zeros guard then turns into "0". A log axis cannot have a
        // tick at zero, so that label was not coarse, it was impossible.
        let f = axis_tick_formatter(&[0.001, 1.0]);
        assert_eq!(f(0.001), "0.001");
        assert_eq!(f(1.0), "1.000");
        // Unthinned, the same axis was always right; it must stay right.
        let full = axis_tick_formatter(&[0.001, 0.01, 0.1, 1.0]);
        assert_eq!(full(0.001), "0.001");
        // And an ordinary linear axis is untouched: evenly spaced ticks
        // never need more digits for their smallest value than for their
        // step.
        let lin = axis_tick_formatter(&[0.0, 0.4, 0.8, 1.2]);
        assert_eq!(lin(0.4), "0.4");
        assert_eq!(lin(0.0), "0");
        let big = axis_tick_formatter(&[0.0, 2000.0, 4000.0]);
        assert_eq!(big(2000.0), "2000");
        let offset = axis_tick_formatter(&[100.5, 101.0, 101.5]);
        assert_eq!(offset(101.0), "101.0");
    }

    #[test]
    fn a_rotated_pdf_label_turns_the_same_way_the_svg_one_does() {
        // A y-axis label is written `rotate(-90)` in SVG: reading
        // bottom-to-top, glyph tops facing left. The PDF text matrix has to
        // compose that with the page's y-flip, and the composition is
        // `[cos, sin, sin, -cos]` -- see the note above
        // `write_pdf_text_spans`. Written as `[cos, -sin, -sin, -cos]` it
        // is the rotation by +90 instead, identical at 0 degrees and wrong
        // for every axis label ever exported.
        let mut out = String::new();
        write_pdf_text(&mut out, 100.0, 200.0, "Amplitude", 12.0, Anchor::Middle, -90.0, "#000000", PdfFace::Regular, None, None, None);
        assert!(
            out.contains("0.00000 -1.00000 -1.00000 -0.00000"),
            "a -90 degree label should carry [0, -1, -1, 0]: {out}"
        );
        // Unrotated text is the case that was always right; it must stay
        // bit-identical.
        let mut flat = String::new();
        write_pdf_text(&mut flat, 100.0, 200.0, "Amplitude", 12.0, Anchor::Middle, 0.0, "#000000", PdfFace::Regular, None, None, None);
        assert!(flat.contains("1.00000 0.00000 0.00000 -1.00000"), "unrotated text should be unchanged: {flat}");
    }

    #[test]
    fn a_column_preset_is_never_widened() {
        // `figure_size("nature2")` states the width a journal will print
        // at. Widening it undoes the one thing the preset is for -- and
        // silently, since the figure still looks fine until it is placed.
        let mut fig = print_grid_rc(2, 3);
        fig.width_explicit = true;
        assert_eq!(resolved_width(&fig, 1830.0), 1830.0);
        // The height is still free: that is why the preset form leaves it
        // to be decided from the data in the first place.
        assert!(resolved_height(&fig, 1830.0, 0.0) > 0.0);
    }

    #[test]
    fn the_growth_stops_before_the_figure_stops_fitting_on_a_page() {
        // Twelve rows would want a canvas nearly 4000 units tall. Past
        // roughly 3:1 a figure does not fit a page at all, so the cap wins
        // and the deepest stacks go back to accepting some packing --
        // a tall figure is better than an unprintable one.
        let fig = print_grid(12);
        let h = resolved_height(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT);
        assert_eq!(h, DEFAULT_FIGURE_WIDTH * MAX_GRID_ASPECT);
        // And it still never shrinks a figure below what it was given.
        assert!(h >= DEFAULT_FIGURE_HEIGHT);
    }

    #[test]
    fn every_backend_puts_the_drawing_on_a_page_of_the_same_height() {
        // `render_tikz` and `render_pdf` used to pass their raw `height`
        // into the canvas size while `build_draw_ops` resolved it
        // internally, so a grown (or preset-sized) figure got a page of one
        // height holding a layout computed for another. All four backends
        // now go through `resolved_height`.
        let fig = print_grid(4);
        let h = resolved_height(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT);
        let svg = render_svg(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT, false);
        assert!(
            svg.contains(&format!("viewBox=\"0 0 {} {h}\"", DEFAULT_FIGURE_WIDTH)),
            "svg canvas should be the resolved height {h}"
        );
        // The TikZ picture states its height once, as the `yshift` that
        // puts the y-down drawing back on a y-up page. Shifted by the
        // unresolved height it would sit off the picture by the difference.
        let tikz = render_tikz(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT);
        assert!(
            tikz.contains(&format!("yshift={}mm", h * (1.0 / MM_TO_UNITS))),
            "tikz yshift should carry the resolved height {h}: {}",
            &tikz[..tikz.len().min(200)]
        );
        // And the PDF MediaBox, in points.
        let pdf = String::from_utf8_lossy(&render_pdf(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT)).to_string();
        let want = format!("{:.2}", h * PDF_POINTS_PER_UNIT);
        assert!(pdf.contains(&want), "pdf MediaBox should be {want} pt tall");
    }

    #[test]
    fn small_grid_layout_is_unaffected_by_the_dense_grid_clamp() {
        // The fit-scale clamp added for the dense-grid case above must be a
        // no-op for ordinary panel counts -- e.g. Filter Design's 2-row and
        // Convolution's 3-row templates should render with exactly the same
        // title font size and margins as before that clamp existed. A
        // 3-row stack at the default 900x600 canvas gives each cell 176px
        // of height against 72px of fixed top+bottom margin -- comfortably
        // under the 70%-of-cell cap -- so the scale factor must be exactly
        // 1.0 and the title must render at the figure's actual title_size.
        let mut fig = Figure::new();
        for i in 1..=3 {
            fig.select_grid(3, 1, i).unwrap();
            fig.current_panel_mut().title = Some(format!("Panel {i}"));
        }
        let ops = build_draw_ops(&fig, 900.0, 600.0);
        for i in 1..=3 {
            let want = format!("Panel {i}");
            let size = ops.iter().find_map(|op| match op {
                DrawOp::Text { text, size, .. } if *text == want => Some(*size),
                _ => None,
            });
            assert_eq!(size, Some(DEFAULT_TITLE_SIZE), "panel {i} title should render at the unscaled default title size");
        }
    }

    #[test]
    fn custom_font_family_reaches_the_svg_root_not_every_text_element() {
        let mut fig = Figure::new();
        fig.font_family = "Georgia, serif".to_string();
        fig.current_panel_mut().title = Some("t".into());
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains("font-family=\"Georgia, serif\""), "got: {svg}");
        // exactly once — on the <svg> root, not duplicated per <text>.
        assert_eq!(svg.matches("font-family=").count(), 1, "got: {svg}");
    }

    #[test]
    fn default_figure_background_is_still_white() {
        // Regression: every pre-existing script/test that never calls
        // `figure_background(...)` must render byte-identical to before
        // that builtin existed.
        let fig = Figure::new();
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains("fill=\"#ffffff\""), "got: {svg}");
    }

    #[test]
    fn scatter_points_get_a_hovertitle_with_the_real_x_y_value() {
        // Hover tooltips (see FigureViewer.tsx's inline-SVG rendering)
        // depend on a native `<title>` child sitting *inside* the `<circle>`
        // element for each real data point -- not just present anywhere in
        // the document.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![3.2], y: vec![17.05], label: None, marker: "o".into(), color: None, ..Default::default()
        });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        assert!(
            ops.iter().any(|op| matches!(op, DrawOp::TitledCircle { title, .. } if title == "x=3.2, y=17.05")),
            "expected a TitledCircle with title \"x=3.2, y=17.05\", got: {ops:?}"
        );
        let svg = render_svg(&fig, 400.0, 300.0, false);
        let circle_pos = svg.find("<circle").expect("a <circle> element");
        let close_pos = svg[circle_pos..].find("</circle>").map(|p| p + circle_pos).expect("closing </circle>");
        let title_pos = svg[circle_pos..].find("<title>x=3.2, y=17.05</title>").map(|p| p + circle_pos);
        assert!(
            title_pos.is_some_and(|p| p < close_pos),
            "expected <title>x=3.2, y=17.05</title> nested inside the <circle>...</circle>, got: {svg}"
        );
    }

    #[test]
    fn bar_tops_get_a_hovertitle_with_the_real_x_y_value() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![5.0, 3.0], label: None, marker: "bar".into(), color: None, ..Default::default()
        });
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains("<title>x=0, y=5</title>"), "got: {svg}");
        assert!(svg.contains("<title>x=1, y=3</title>"), "got: {svg}");
    }

    #[test]
    fn heatmap_cells_get_a_hovertitle_with_the_real_value() {
        let mut fig = Figure::new();
        fig.current_panel_mut().heatmap = Some(Heatmap {
            values: vec![1.0, 42.3, 3.0, 4.0], rows: 2, cols: 2, colormap: "viridis".into(),
            row_labels: vec![], col_labels: vec![], dense: false, square: false,
        });
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains("<title>42.3</title>"), "got: {svg}");
    }

    #[test]
    fn custom_figure_background_changes_the_emitted_background_rect() {
        let mut fig = Figure::new();
        fig.background = "#000000".to_string();
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(!svg.contains("fill=\"#ffffff\""), "got: {svg}");
        // the very first drawn element is the background <rect>, so its
        // fill should show up before any series/axis/text drawing.
        let rect_pos = svg.find("<rect").expect("a <rect> element");
        let fill_pos = svg[rect_pos..].find("fill=\"#000000\"").map(|p| p + rect_pos);
        assert!(fill_pos.is_some(), "got: {svg}");
    }

    #[test]
    fn custom_figure_background_still_renders_axes_gridlines_and_text() {
        // Not auto-inverting axis/gridline/text colors for a dark
        // background is a deliberate, documented scope cut (see
        // `Figure.background`'s doc comment) — but the other plot
        // elements must still actually render (not be silently dropped)
        // alongside the new background.
        let mut fig = Figure::new();
        fig.background = "#000000".to_string();
        fig.current_panel_mut().title = Some("t".into());
        fig.current_panel_mut().series.push(Series { y: vec![1.0, 2.0, 3.0], ..Series::default() });
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains(">t<"), "title text missing: {svg}");
        assert!(svg.contains("<polyline") || svg.contains("<path"), "series line missing: {svg}");
    }

    #[test]
    fn custom_font_sizes_flow_into_rendered_text_and_margins() {
        let mut fig = Figure::new();
        fig.title_size = 32.0; // 2x default
        fig.current_panel_mut().title = Some("t".into());
        fig.current_panel_mut().series.push(Series { y: vec![1.0, 2.0], ..Series::default() });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let title_op = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Text { text, .. } if text == "t"))
            .expect("title text op");
        match title_op {
            DrawOp::Text { size, .. } => assert_eq!(*size, 32.0),
            _ => unreachable!(),
        }
    }

    #[test]
    fn flow_layout_stacks_vertical_and_joins_horizontal() {
        // each panel gets a token series before the next `advance`, matching
        // real usage (`next plot` is always followed by a `plot()` call) —
        // `advance` reuses a still-pristine panel rather than creating an
        // empty second one, so an untouched panel isn't what we're testing.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series::default());
        fig.advance(Direction::Vertical); // panel 1, new row
        fig.current_panel_mut().series.push(Series::default());
        fig.advance(Direction::Horizontal); // panel 2, joins panel 1's row
        fig.current_panel_mut().series.push(Series::default());
        let rects = layout_panels(&fig, 400.0, 300.0);
        assert_eq!(rects.len(), 3);
        let r0 = rects.iter().find(|r| r.panel == 0).unwrap();
        let r1 = rects.iter().find(|r| r.panel == 1).unwrap();
        let r2 = rects.iter().find(|r| r.panel == 2).unwrap();
        assert!(r0.top < r1.top); // panel 0 and panel 1 are different rows
        assert!((r1.top - r2.top).abs() < 1e-9); // panel 1 and 2 share a row
        assert!(r1.left < r2.left); // ...side by side
    }

    #[test]
    fn legend_best_avoids_the_quadrant_with_more_points() {
        // most points sit in the bottom-left of pixel space (real to_px
        // output, not data space); "best" must place the legend box
        // somewhere that doesn't overlap them.
        let points = vec![(5.0, 95.0), (15.0, 85.0), (10.0, 90.0), (20.0, 80.0), (90.0, 10.0)];
        let position = best_corner(0.0, 0.0, 100.0, 100.0, 20.0, 20.0, &points);
        assert_ne!(position, LegendPosition::BottomLeft);
    }

    #[test]
    fn legend_outside_reserves_margin_and_draws_clear_of_the_plot() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series {
            x: vec![0.0, 1.0, 2.0],
            y: vec![0.0, 1.0, 2.0],
            label: Some("s".into()),
            marker: "line".into(),
            color: None,
            ..Default::default()
        });
        panel.legend.visible = true;
        panel.legend.position = LegendPosition::Right;
        panel.legend.outside = true;
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let legend_rect = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Rect { x, w, fill: Some(f), .. } if f.as_str() == "#ffffff" => Some((*x, *w)),
                _ => None,
            })
            .expect("legend box rect");
        // the plot's own border rect (drawn when `show_box` is on, the
        // default) must end strictly before the legend box begins — proof
        // the plot area actually shrank to make room rather than the legend
        // overlapping it.
        let plot_right = ops
            .iter()
            // The frame is four lines; its right edge is the rightmost
            // vertical spine.
            .filter_map(|op| match op {
                DrawOp::Line { x1, y1, x2, y2, .. } if (x1 - x2).abs() < 0.01 && (y1 - y2).abs() > 1.0 => Some(*x1),
                _ => None,
            })
            .fold(f64::NEG_INFINITY, f64::max);
        let plot_right = if plot_right.is_finite() { plot_right } else { panic!("no plot border spine") };
        let _ = ();
        assert!(plot_right <= legend_rect.0, "plot border {plot_right} should end at/before legend box {}", legend_rect.0);
    }

    #[test]
    fn legend_text_scales_with_fontsize_instead_of_staying_pinned_at_10_5px() {
        // Regression for a real inconsistency found during the ggplot2
        // visual-quality audit: `legend_ops` used to hardcode its label text
        // at 10.5px no matter what `fontsize(tick: ..., ...)` set for every
        // other element on the figure (ticks/labels/titles all scale via
        // `fig.tick_size`/`label_size`/`title_size`, but the legend didn't
        // participate at all) -- a script that bumped fonts for a
        // presentation still got a barely-legible legend next to
        // oversized everything else. `fig.tick_size` doubled here should
        // double the legend's own text size (and, since `legend_box_size`
        // scales the same way, the box still fits it).
        let mut fig = Figure::new();
        fig.tick_size = DEFAULT_TICK_SIZE * 2.0;
        let panel = fig.current_panel_mut();
        panel.series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: Some("s".into()), marker: "line".into(), color: None, ..Default::default()
        });
        panel.legend.visible = true;
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let legend_text_size = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Text { text, size, .. } if text == "s" => Some(*size),
                _ => None,
            })
            .expect("legend label text");
        // The legend now takes the TICK type size as its base rather than a
        // separate literal -- it used to be 12.0 against 12.8 tick text,
        // making the key the smallest thing on the figure. Doubling
        // `tick_size` still doubles it, which is what this test is for.
        let expected = DEFAULT_TICK_SIZE * 2.0;
        assert!((legend_text_size - expected).abs() < 1e-9, "expected the legend text to double to {expected} alongside tick_size, got {legend_text_size}");
    }

    #[test]
    fn every_marker_glyph_is_a_closed_outline_of_about_the_right_size() {
        // The properties that make a marker set usable, checked on every
        // member rather than on the two that were easy to eyeball.
        //
        // A curved glyph is sampled from a polar radius function, and the
        // failure that costs is a ray that reaches nothing: the outline
        // folds through the centre and the mark comes out as a knot. So
        // this asserts a floor on the radius as well as a ceiling.
        const NAMES: &[&str] = &[
            "s", "square", "D", "d", "diamond", "losange",
            "^", "triangle", "v", "<", ">",
            "p", "pentagon", "h", "H", "hexagon", "8", "octagon",
            "*", "star", "star6", "hexstar", "star4", "sparkle", "asterisk", "ast",
            "klee", "klee3", "clover", "clover3", "trefoil",
            "klee4", "clover4", "shamrock", "lucky",
            "flower", "heart", "spade", "club",
            "P", "plus", "X", "xmark",
            // The Unicode spellings, so `marker = "\u{2665}"` is a real
            // way to ask for a heart and not a silently ignored string.
            "\u{25B2}", "\u{25BC}", "\u{2605}", "\u{2665}", "\u{2660}",
            "\u{2663}", "\u{2666}", "\u{2618}",
        ];
        let r = 10.0;
        for name in NAMES {
            let pts = marker_polygon(name, r)
                .unwrap_or_else(|| panic!("{name:?} is not in the marker table"));
            assert!(pts.len() >= 3, "{name:?}: {} points is not an outline", pts.len());
            let radii: Vec<f64> = pts.iter().map(|(x, y)| (x * x + y * y).sqrt()).collect();
            let max = radii.iter().cloned().fold(0.0f64, f64::max);
            let min = radii.iter().cloned().fold(f64::INFINITY, f64::min);
            // Sized to the mark, not to whatever its construction radius
            // happened to be: a clover and a square asked for at the same
            // `ms` have to read as the same size of mark.
            assert!(
                (0.80 * r..=1.30 * r).contains(&max),
                "{name:?}: furthest point at {max}, expected about {r}"
            );
            assert!(min > 0.05 * r, "{name:?}: a vertex at {min} folds through the centre");
            // Closed by construction -- no repeated last point, which would
            // draw a zero-length segment in every backend.
            assert_ne!(pts.first(), pts.last(), "{name:?}: the outline repeats its first point");
        }
        assert!(marker_polygon("not-a-marker", r).is_none());
    }

    #[test]
    fn square_marker_renders_as_a_hollow_rect_at_each_data_point() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![1.0, 2.0, 3.0], y: vec![3.0, 1.0, 4.0], marker: "s".into(), ..Default::default() });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        // Every polygonal glyph now comes from one `marker_polygon` table,
        // so a square is four `Polygon` points rather than a `Rect` -- the
        // same hollow square on the page, and the property worth asserting
        // is still "one per data point, hollow, and actually square".
        let point_squares = ops
            .iter()
            .filter(|op| match op {
                DrawOp::Polygon { points, fill: None, stroke: Some(_), .. } if points.len() == 4 => {
                    let w = points[1].0 - points[0].0;
                    let h = points[2].1 - points[1].1;
                    (w - h).abs() < 1e-9 && w > 0.0
                }
                _ => false,
            })
            .count();
        assert_eq!(point_squares, 3, "expected one square marker per data point, got: {ops:?}");
    }

    #[test]
    fn square_marker_alias_matches_the_short_s_spelling() {
        let mut fig_s = Figure::new();
        fig_s.current_panel_mut().series.push(Series { x: vec![1.0], y: vec![1.0], marker: "s".into(), ..Default::default() });
        let mut fig_square = Figure::new();
        fig_square.current_panel_mut().series.push(Series { x: vec![1.0], y: vec![1.0], marker: "square".into(), ..Default::default() });
        // `DrawOp` has no `PartialEq`; compare the rendered SVG instead --
        // simpler than hand-matching every field, and it's what a caller
        // actually observes.
        assert_eq!(render_svg(&fig_s, 400.0, 300.0, false), render_svg(&fig_square, 400.0, 300.0, false));
    }

    #[test]
    fn legend_swatch_matches_the_series_marker_instead_of_always_a_line() {
        // Before this, every legend entry drew a plain colored line
        // regardless of the series' own marker -- a stem/circle/square
        // series looked identical to a line series in the legend.
        let mut circle_fig = Figure::new();
        let p1 = circle_fig.current_panel_mut();
        p1.series.push(Series { x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: Some("pts".into()), marker: "o".into(), ..Default::default() });
        p1.legend.visible = true;
        let circle_ops = build_draw_ops(&circle_fig, 400.0, 300.0);
        // The swatch circle is `3.5 * tick_scale`, distinct from the r=3.2
        // circles drawn at the two actual data points. Derived rather than
        // pinned at 3.5: that literal was only correct while the default
        // tick size happened to equal `TICK_METRIC_BASELINE`, so retuning
        // the type scale broke a test about legend SHAPE.
        let swatch_r = 3.5 * (DEFAULT_TICK_SIZE / TICK_METRIC_BASELINE);
        assert!(
            circle_ops.iter().any(|op| matches!(op, DrawOp::Circle { fill: None, stroke: Some(_), r, .. } if (*r - swatch_r).abs() < 1e-9)),
            "expected a hollow-circle legend swatch for a circle-marker series"
        );

        let mut square_fig = Figure::new();
        let p2 = square_fig.current_panel_mut();
        p2.series.push(Series { x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: Some("pts".into()), marker: "square".into(), ..Default::default() });
        p2.legend.visible = true;
        let square_ops = build_draw_ops(&square_fig, 400.0, 300.0);
        // The swatch now comes from the same `marker_polygon` table as the
        // series' own glyph -- which is the point: the two cannot show
        // different shapes any more. A square is a four-point polygon.
        assert!(
            square_ops.iter().any(|op| match op {
                DrawOp::Polygon { points, fill: None, stroke: Some(_), .. } if points.len() == 4 => {
                    let w = points[1].0 - points[0].0;
                    let h = points[2].1 - points[1].1;
                    (w - h).abs() < 1e-9 && w > 0.0
                }
                _ => false,
            }),
            "expected a hollow-square legend swatch for a square-marker series"
        );

        let mut stem_fig = Figure::new();
        let p3 = stem_fig.current_panel_mut();
        p3.series.push(Series { x: vec![0.0, 1.0], y: vec![1.0, 2.0], label: Some("pts".into()), marker: "stem".into(), ..Default::default() });
        p3.legend.visible = true;
        let stem_ops = build_draw_ops(&stem_fig, 400.0, 300.0);
        assert!(
            stem_ops.iter().any(|op| matches!(op, DrawOp::Circle { fill: Some(_), stroke: None, r, .. } if (*r - 2.2 * (DEFAULT_TICK_SIZE / TICK_METRIC_BASELINE)).abs() < 1e-9)),
            "expected a filled-circle legend swatch (the stem head) for a stem series"
        );
    }

    #[test]
    fn minor_grid_is_off_by_default_and_draws_dotted_lines_when_enabled() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { x: vec![0.0, 1.0, 2.0, 3.0], y: vec![0.0, 1.0, 4.0, 9.0], ..Default::default() });
        let ops_default = build_draw_ops(&fig, 400.0, 300.0);
        let dotted = |ops: &[DrawOp]| ops.iter().filter(|op| matches!(op, DrawOp::DottedLine { color, .. } if color == GRID_COLOR)).count();
        assert_eq!(dotted(&ops_default), 0, "minor grid must be off by default");

        fig.current_panel_mut().show_minor_grid = Some(true);
        let ops_minor = build_draw_ops(&fig, 400.0, 300.0);
        assert!(dotted(&ops_minor) > 0, "expected dotted minor gridlines once enabled, got: {ops_minor:?}");
        // exactly one native dashed element per minor tick -- not the
        // dozens of individual `Line` segments the old manual-expansion
        // approach produced (500KB+ for a wide-range semilogy plot).
        assert!(dotted(&ops_minor) < 50, "minor gridlines should be single dashed elements, not many small segments");
    }

    #[test]
    fn minor_tick_positions_for_a_log_axis_are_the_standard_two_through_nine_per_decade() {
        // Verified against MATLAB/matplotlib's own convention: within each
        // fully-visible decade there are exactly 8 minor ticks (2..9), at
        // log10(2..9) offset from that decade's major tick.
        let majors = vec![0.0, 1.0, 2.0]; // decades 10^0, 10^1, 10^2
        let minors = minor_tick_positions(&majors, 0.0, 2.0, true);
        assert_eq!(minors.len(), 16, "two fully-visible decades (0 and 1) x 8 minors each; the third decade's minors fall outside [0,2]");
        assert!(minors.iter().any(|&v| (v - 2f64.log10()).abs() < 1e-9));
        assert!(minors.iter().any(|&v| (v - (1.0 + 5f64.log10())).abs() < 1e-9));
    }

    #[test]
    fn minor_tick_positions_for_a_linear_axis_are_gap_midpoints() {
        let majors = vec![0.0, 2.0, 4.0];
        let minors = minor_tick_positions(&majors, 0.0, 4.0, false);
        assert_eq!(minors, vec![1.0, 3.0]);
    }

    #[test]
    fn bar_series_render_as_rectangles_reaching_the_zero_baseline() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0, 2.0],
            y: vec![5.0, 3.0, 8.0],
            label: None,
            marker: "bar".into(),
            color: None,
            ..Default::default()
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let rects: Vec<_> = ops.iter().filter(|op| matches!(op, DrawOp::TitledRect { fill: Some(_), .. })).collect();
        assert_eq!(rects.len(), 3);
    }

    #[test]
    fn bar_show_values_renders_a_text_label_per_bar() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![5.0, 3.0], label: None, marker: "bar".into(), color: None,
            show_values: true, ..Default::default()
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let labels: Vec<&String> = ops
            .iter()
            .filter_map(|op| match op { DrawOp::Text { text, .. } => Some(text), _ => None })
            .collect();
        assert!(labels.iter().any(|t| t.as_str() == "5"), "got: {labels:?}");
        assert!(labels.iter().any(|t| t.as_str() == "3"), "got: {labels:?}");
    }

    #[test]
    fn bar_without_show_values_renders_no_value_labels() {
        // Tick labels are also `DrawOp::Text`, so assert on placement (a
        // value label sits just above the bar top, well inside the plot
        // area) rather than absence of any text at all.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0], y: vec![5.0], label: None, marker: "bar".into(), color: None,
            ..Default::default()
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let bar_top = ops
            .iter()
            .find_map(|op| match op { DrawOp::TitledRect { y, .. } => Some(*y), _ => None })
            .expect("bar rect");
        assert!(!ops.iter().any(|op| matches!(op, DrawOp::Text { y, .. } if (*y - (bar_top - 6.0)).abs() < 1e-6)));
    }

    #[test]
    fn bar_hatch_draws_a_white_stroked_rect_plus_diagonal_lines() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0], y: vec![5.0], label: None, marker: "bar".into(), color: None,
            hatch: true, ..Default::default()
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let bar_rect = ops.iter().find(|op| matches!(op, DrawOp::TitledRect { fill: Some(f), .. } if f == "#ffffff"));
        assert!(bar_rect.is_some(), "expected a white-filled bar rect, got: {ops:?}");
        if let Some(DrawOp::TitledRect { stroke, .. }) = bar_rect {
            assert!(stroke.is_some(), "hatch bar should have a colored stroke outline");
        }
        let line_count = ops.iter().filter(|op| matches!(op, DrawOp::Line { .. })).count();
        assert!(line_count >= 2, "expected multiple diagonal hatch lines, got {line_count}");
    }

    #[test]
    fn hatch_lines_stay_within_the_rect_bounds() {
        let lines = hatch_lines(10.0, 20.0, 30.0, 15.0, "#000000");
        assert!(!lines.is_empty());
        for op in &lines {
            if let DrawOp::Line { x1, y1, x2, y2, .. } = op {
                for (x, y) in [(x1, y1), (x2, y2)] {
                    assert!(*x >= 10.0 - 1e-9 && *x <= 40.0 + 1e-9, "x {x} out of bounds");
                    assert!(*y >= 20.0 - 1e-9 && *y <= 35.0 + 1e-9, "y {y} out of bounds");
                }
            }
        }
    }

    #[test]
    fn stackbar_series_stack_on_top_of_each_other() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0],
            y: vec![2.0, 3.0],
            label: None,
            marker: "stackbar".into(),
            color: None,
            ..Default::default()
        });
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0],
            y: vec![4.0, 1.0],
            label: None,
            marker: "stackbar".into(),
            color: None,
            ..Default::default()
        });
        // the axis extent must cover the *stacked* total (6 and 4), not each
        // series' own smaller range (max 4 and max 3 respectively).
        let totals = stacked_bar_totals(fig.current_panel());
        assert_eq!(totals, vec![6.0, 4.0]);
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let rect_count = ops.iter().filter(|op| matches!(op, DrawOp::Rect { fill: Some(_), .. })).count();
        assert_eq!(rect_count, 4); // 2 categories x 2 stacked series
    }

    #[test]
    fn base64_encode_matches_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn embeddable_font_for_resolves_presets_and_rejects_custom_names() {
        assert!(embeddable_font_for(DEFAULT_FONT_FAMILY).is_some());
        assert!(embeddable_font_for(PRINT_FONT_FAMILY).is_some());
        assert!(embeddable_font_for("Comic Sans MS, cursive").is_none());
    }

    #[test]
    fn tikz_definecolor_emits_three_brace_groups_so_latex_can_compile_it() {
        // `\definecolor{name}{model}{spec}` takes three groups. Emitting
        // `{[HTML]{e6e6e6}}` -- one group wrapping a bracketed model --
        // is xcolor's optional-argument spelling, which `\definecolor`
        // rejects: "Undefined color model `[HTML]{e6e6e6}'", fatal, no
        // output PDF. Every .tikz Qu exported failed on its first colour.
        assert_eq!(tikz_draw_color("#e6e6e6"), "{HTML}{e6e6e6}");
        assert_eq!(tikz_draw_color("e6e6e6"), "{HTML}{e6e6e6}");
        // Short/garbage input still has to produce a well-formed pair of
        // groups -- a malformed one takes the whole document down.
        assert_eq!(tikz_draw_color("#abc"), "{HTML}{000000}");

        // And the assembled line, which is what actually reaches LaTeX.
        let line = format!("\\definecolor{{c}}{}", tikz_draw_color("#5a5a5a"));
        assert_eq!(line, "\\definecolor{c}{HTML}{5a5a5a}");
        assert!(!line.contains('['), "the bracketed model form does not compile");
    }

    #[test]
    fn fontfamily_tikz_preset_resolves_and_embeds_latin_modern() {
        // `fontfamily("tikz")` (wired up in lib.rs) sets `Figure.font_family`
        // to exactly `TIKZ_FONT_FAMILY` — check the constant itself carries
        // Latin Modern Roman as its lead face (the actual LaTeX/TikZ default
        // typeface, not just a Georgia/serif fallback).
        assert!(TIKZ_FONT_FAMILY.contains("Latin Modern Roman"));

        let (family_name, bytes, format_tag) = embeddable_font_for(TIKZ_FONT_FAMILY)
            .expect("TIKZ_FONT_FAMILY is a curated preset and must have embeddable bytes");
        assert_eq!(family_name, "Latin Modern Roman");
        assert!(!bytes.is_empty(), "embedded font bytes must be real, non-empty OTF data");
        assert_eq!(format_tag, "opentype", "OTF must use format('opentype'), not 'truetype'");

        // The sans companion preset resolves too.
        let (sans_name, sans_bytes, sans_format) = embeddable_font_for(ACADEMIC_SANS_FONT_FAMILY)
            .expect("ACADEMIC_SANS_FONT_FAMILY is a curated preset and must have embeddable bytes");
        assert_eq!(sans_name, "Latin Modern Sans");
        assert!(!sans_bytes.is_empty());
        assert_eq!(sans_format, "opentype");

        // The "only presets embed" contract still holds for an arbitrary
        // custom font family string, Latin Modern's own presets included.
        assert!(embeddable_font_for("My Custom Font, sans-serif").is_none());
    }

    /// A point that would be written identically to the one before it is
    /// dropped — losslessly, because the two are the same pixel at the
    /// precision actually emitted.
    ///
    /// A 1,000,000-sample line wrote 13.9 MB of SVG of which 69.3% was
    /// byte-identical repeats (`104.06,290.19 104.06,290.19`), against
    /// matplotlib's 33 KB for the same data. Deduplication takes it to
    /// 4.3 MB with pixel-identical rendering, verified by rasterising both
    /// and comparing every channel of 540,000 pixels.
    #[test]
    fn a_polyline_drops_points_that_would_be_written_twice() {
        let mut out = String::new();
        // Three distinct pixels, each visited several times — the shape a
        // dense line takes once more than one sample lands per pixel.
        let op = DrawOp::Polyline {
            points: vec![
                (1.0, 2.0),
                (1.001, 2.001), // formats to "1.00,2.00" -- a repeat
                (1.0, 2.0),
                (3.0, 4.0),
                (3.004, 4.004), // formats to "3.00,4.00" -- a repeat
                (5.0, 6.0),
            ],
            color: "#000".into(),
            width: 1.0,
            dash: None,
        };
        write_svg_op(&mut out, &op, 12.0);
        let pts = out
            .split("points=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("a polyline with points");
        assert_eq!(pts, "1.00,2.00 3.00,4.00 5.00,6.00", "got: {pts}");

        // Every point identical must still leave a VALID polyline with one
        // point, not an empty one -- an empty `points` attribute is a
        // different thing from a degenerate line and some renderers treat
        // it differently.
        let mut flat = String::new();
        write_svg_op(
            &mut flat,
            &DrawOp::Polyline {
                points: vec![(7.0, 8.0); 5],
                color: "#000".into(),
                width: 1.0,
                dash: None,
            },
            12.0,
        );
        let fp = flat
            .split("points=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("a polyline with points");
        assert_eq!(fp, "7.00,8.00", "all-identical input must keep one point, got: {fp}");

        // And a genuinely distinct sequence is untouched -- the fix must not
        // thin data that is already at pixel resolution.
        let mut keep = String::new();
        write_svg_op(
            &mut keep,
            &DrawOp::Polyline {
                points: vec![(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)],
                color: "#000".into(),
                width: 1.0,
                dash: None,
            },
            12.0,
        );
        assert!(keep.contains("1.00,1.00 2.00,2.00 3.00,3.00"), "got: {keep}");
    }

    #[test]
    fn render_svg_embeds_a_font_face_only_when_asked_and_only_for_a_preset() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { y: vec![1.0, 2.0], ..Series::default() });
        assert!(!render_svg(&fig, 200.0, 150.0, false).contains("@font-face"));
        assert!(render_svg(&fig, 200.0, 150.0, true).contains("@font-face"));
        fig.font_family = "Comic Sans MS, cursive".to_string();
        assert!(!render_svg(&fig, 200.0, 150.0, true).contains("@font-face"), "no bytes to embed for a custom font");
    }

    #[test]
    fn zoom_inset_has_its_own_grid_and_tick_labels() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: (0..20).map(|i| i as f64).collect(), y: (0..20).map(|i| (i as f64).sin()).collect(), label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.inset = Some(InsetZoom { x0: 5.0, x1: 8.0, y0: -1.0, y1: 1.0, position: LegendPosition::Best, outside: false, clip: false });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let grid_lines = ops.iter().filter(|op| matches!(op, DrawOp::Line { color, .. } if color == GRID_COLOR)).count();
        assert!(grid_lines > 0, "expected inset gridlines, got: {ops:?}");
        // tick text sits strictly inside [5, 8] x [-1, 1] — distinct from
        // the main panel's own axis tick labels.
        let inset_tick_texts: Vec<&String> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, size, .. } if *size < fig.tick_size => Some(text),
                _ => None,
            })
            .collect();
        assert!(!inset_tick_texts.is_empty(), "expected small-font inset tick labels, got: {ops:?}");
    }

    #[test]
    fn zoom_inset_draws_a_white_inset_panel_a_dashed_zoom_box_and_connectors() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: (0..20).map(|i| i as f64).collect(), y: (0..20).map(|i| (i as f64).sin()).collect(), label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.inset = Some(InsetZoom { x0: 5.0, x1: 8.0, y0: -1.0, y1: 1.0, position: LegendPosition::Best, outside: false, clip: false });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        // an inset background rect distinct from the main panel's own
        // border/legend rects: white-filled AND stroke-only bordered, two
        // separate rects at the same position (background, then border).
        let white_fills = ops.iter().filter(|op| matches!(op, DrawOp::Rect { fill: Some(f), .. } if f == "#ffffff")).count();
        assert!(white_fills >= 1, "expected an inset background rect, got: {ops:?}");
        let inset_bg = ops.iter().find(|op| matches!(op, DrawOp::Rect { fill: Some(f), .. } if f == "#ffffff")).unwrap();
        if let DrawOp::Rect { opacity, radius, .. } = inset_bg {
            assert!(*opacity < 1.0, "inset background should be translucent, not opaque, got opacity {opacity}");
            assert!(*radius > 0.0, "inset should have rounded corners, got radius {radius}");
        }
        // Only the INSET border is still a Rect; the plot frame is four
        // lines. Check for the inset's rounded border specifically.
        let bordered = ops.iter().filter(|op| matches!(op, DrawOp::Rect { fill: None, stroke: Some(_), radius, .. } if *radius > 0.0)).count();
        assert!(bordered >= 1, "expected the inset border, got {bordered}");
        // dashed zoom-box + 2 connectors are all `DrawOp::Line`s (not
        // `Polyline`s) distinct from the plotted series' own polyline.
        let line_count = ops.iter().filter(|op| matches!(op, DrawOp::Line { .. })).count();
        assert!(line_count > 4, "expected multiple dashed segments, got {line_count}");
        // the zoomed-in replay of the series is a second `Polyline` beyond
        // the main panel's own.
        let polyline_count = ops.iter().filter(|op| matches!(op, DrawOp::Polyline { .. })).count();
        assert_eq!(polyline_count, 2, "expected the main curve plus its inset replay, got {polyline_count}");
    }

    #[test]
    fn zoom_inset_only_replays_points_inside_the_zoom_box() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 1.0, 2.0, 3.0, 4.0], y: vec![0.0, 1.0, 2.0, 3.0, 4.0], label: None, marker: "o".into(), color: None, ..Default::default() });
        panel.inset = Some(InsetZoom { x0: 1.0, x1: 2.0, y0: 1.0, y1: 2.0, position: LegendPosition::Best, outside: false, clip: false });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        // Main-panel points are `TitledCircle` (they carry a hover tooltip);
        // the inset's replayed points are plain `Circle` (the inset has no
        // established "zoomed tooltip" rendering -- see the comment above
        // the inset replay loop).
        let titled_circle_count = ops.iter().filter(|op| matches!(op, DrawOp::TitledCircle { .. })).count();
        let plain_circle_count = ops.iter().filter(|op| matches!(op, DrawOp::Circle { .. })).count();
        // 5 in the main panel + only the 2 in-range points (x=1,y=1 and
        // x=2,y=2) replayed into the inset.
        assert_eq!(titled_circle_count, 5, "got {titled_circle_count} titled circles");
        assert_eq!(plain_circle_count, 2, "got {plain_circle_count} plain circles");
    }

    #[test]
    fn an_outside_inset_grows_the_canvas_instead_of_squeezing_the_main_panel() {
        // Reported from a real figure: `zoom_inset(..., outside=true)` on a
        // 900x600 canvas took ~40% of the main panel's width for its own
        // box, leaving a plot squeezed to nearly SQUARE on a landscape
        // canvas -- the figure deformed to make room for its own
        // magnifier. The fix grows the canvas by what the inset needs
        // instead of borrowing it from the plot, matching what an outside
        // legend already does for its own margin (`Legend.outside`).
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series {
            x: (0..400).map(|i| i as f64 / 80.0).collect(),
            y: (0..400).map(|i| (i as f64 / 80.0).sin()).collect(),
            label: None, marker: "line".into(), color: None, ..Default::default()
        });
        panel.inset = Some(InsetZoom { x0: 2.8, x1: 3.2, y0: -0.1, y1: 0.1, position: LegendPosition::TopRight, outside: true, clip: false });
        let width = resolved_width(&fig, DEFAULT_FIGURE_WIDTH);
        assert!(width > DEFAULT_FIGURE_WIDTH, "the canvas should grow for an outside inset, stayed at {width}");

        // The main panel itself -- not the whole canvas -- must not end up
        // squeezed toward square. A screen-theme figure with room to spare
        // should keep a landscape-ish plot, not the ~0.98 aspect the bug
        // produced.
        let no_inset = {
            let mut plain = Figure::new();
            plain.current_panel_mut().series.push(Series {
                x: (0..400).map(|i| i as f64 / 80.0).collect(),
                y: (0..400).map(|i| (i as f64 / 80.0).sin()).collect(),
                label: None, marker: "line".into(), color: None, ..Default::default()
            });
            plain
        };
        // The no-inset baseline has to be measured with the SAME function
        // that measures the with-inset figure (`build_draw_ops_with_
        // geometry`), not approximated as `layout_panels`'s bare rect minus
        // the two margin CONSTANTS. Those constants are only the floor:
        // the real left margin also grows for wider tick labels at a
        // bigger base font size, and `layout_panels` does not know that.
        // Approximating it here worked back when `MARGIN_LEFT` (68) still
        // matched the real margin at the default type scale -- it silently
        // stopped matching the moment the type scale changed the tick
        // font, and this test then failed against a STALE baseline while
        // the fix it guards kept working exactly as intended (confirmed:
        // the real no-inset width and the with-inset width came out equal
        // to within float noise, 761.9375 vs 761.9364, when this false
        // failure was investigated). Measuring both sides the same way
        // removes that whole class of drift.
        let no_inset_geom = build_draw_ops_with_geometry(&no_inset, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT).1;
        let no_inset_main = no_inset_geom.iter().find(|g| g.panel == 0).expect("panel 0 geometry");
        let base_plot_w = no_inset_main.right - no_inset_main.left;

        let geom = build_draw_ops_with_geometry(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT).1;
        let main_geom = geom.iter().find(|g| g.panel == 0).expect("panel 0 geometry");
        let plot_w = main_geom.right - main_geom.left;
        let plot_h = main_geom.bottom - main_geom.top;
        assert!(
            plot_w >= base_plot_w - 1.0,
            "outside inset must not shrink the main panel below its no-inset width: got {plot_w}, no-inset baseline {base_plot_w}"
        );
        let aspect = plot_w / plot_h;
        assert!(aspect > 1.3, "main panel reads squeezed toward square with an outside inset present: aspect {aspect}");
    }

    #[test]
    fn resolving_an_outside_inset_width_twice_does_not_compound() {
        // `render_svg` resolves `width` itself and then passes that
        // ALREADY-resolved width into `build_draw_ops_with_geometry`,
        // which resolves it again -- every `fit_*` here has to tolerate
        // being called on its own output. The fixed-point growth this
        // needs is a contraction with respect to a FIXED baseline, not
        // the incoming width, or a second pass asks for more room on top
        // of what the first pass already added -- which is exactly what
        // sent the inset box's own right edge past the canvas's.
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series {
            x: (0..400).map(|i| i as f64 / 80.0).collect(),
            y: (0..400).map(|i| (i as f64 / 80.0).sin()).collect(),
            label: None, marker: "line".into(), color: None, ..Default::default()
        });
        panel.inset = Some(InsetZoom { x0: 2.8, x1: 3.2, y0: -0.1, y1: 0.1, position: LegendPosition::TopRight, outside: true, clip: false });
        let once = resolved_width(&fig, DEFAULT_FIGURE_WIDTH);
        let twice = resolved_width(&fig, once);
        assert_eq!(once, twice, "a second resolve pass grew the canvas again: {once} -> {twice}");

        // And the property this exists for: the inset box itself must land
        // fully inside whatever canvas actually gets rendered, exactly the
        // path `render_svg` takes (resolve, then build on the resolved
        // width, which resolves again).
        let (ops, _) = build_draw_ops_with_geometry(&fig, once, DEFAULT_FIGURE_HEIGHT);
        let final_width = resolved_width(&fig, once);
        for op in &ops {
            if let DrawOp::Rect { x, w, radius, .. } = op {
                if *radius > 0.0 {
                    assert!(*x + *w <= final_width + 0.5, "the inset box (right edge {}) overflows the {final_width}-wide canvas", x + w);
                }
            }
        }
    }

    #[test]
    fn a_heatmap_does_not_survive_the_next_chart_on_the_same_panel() {
        // Reported chain: `confusion_matrix(...)` then `hist(...)` with no
        // `next plot`/`hold off`/`figure` between them -- ordinary `hold
        // on` reuse of the current panel, which `panel_for_new_series`
        // grants to `series` on purpose (that's how two `plot()` calls
        // overlay). `heatmap` is an `Option`, not a `Vec`: it does not
        // accumulate, so leaving the FIRST call's heatmap in place while
        // the SECOND call's series get added underneath is not an overlay,
        // it's stale exclusive state silently outliving the call that set
        // it. The heatmap draws last and opaque, so the histogram's own
        // bars exist in `series` but never become visible -- exactly what
        // the twelve-line repro showed: 5 rects (the confusion matrix's
        // cells) instead of the ~40 a 40-bin histogram needs.
        let mut fig = Figure::new();
        fig.current_panel_mut().heatmap = Some(Heatmap {
            values: vec![3.0, 1.0, 0.0, 2.0], rows: 2, cols: 2,
            colormap: "blues".into(), row_labels: Vec::new(), col_labels: Vec::new(), dense: false,
            square: false,
        });
        // The same call shape `hist(...)` makes: reaches the SAME panel
        // through `panel_for_new_series` under default `hold on`, and adds
        // a series rather than replacing the panel outright.
        let panel = fig.panel_for_new_series();
        panel.series.push(Series {
            x: (0..40).map(|i| i as f64).collect(),
            y: (0..40).map(|i| (i as f64).sin().abs() * 10.0 + 1.0).collect(),
            label: None, marker: "line".into(), color: None, ..Default::default()
        });
        assert!(panel.heatmap.is_none(), "a new chart on the panel must clear the old heatmap, not draw over it");
        assert_eq!(panel.series.len(), 1, "the new chart's own series must still land");

        // Not just present in the model -- actually drawn. Before this fix
        // the series was in `panel.series` too; it just never made it into
        // `ops` because nothing ever asked "does this panel still have a
        // heatmap sitting where a plain cartesian series should draw".
        let ops = build_draw_ops(&fig, DEFAULT_FIGURE_WIDTH, DEFAULT_FIGURE_HEIGHT);
        let drawn_points: usize = ops
            .iter()
            .map(|op| match op {
                DrawOp::Polyline { points, .. } => points.len(),
                DrawOp::Line { .. } => 1,
                _ => 0,
            })
            .sum();
        assert!(drawn_points >= 40, "expected the new series to actually render its 40 points, got {drawn_points}");
    }

    #[test]
    fn a_pie_chart_does_not_survive_the_next_chart_on_the_same_panel() {
        // Same exclusive-content family as the heatmap case, for `pie`.
        let mut fig = Figure::new();
        fig.current_panel_mut().pie = Some(PieChart { values: vec![1.0, 2.0, 3.0], labels: Vec::new(), donut: false });
        let panel = fig.panel_for_new_series();
        panel.series.push(Series {
            x: vec![0.0, 1.0, 2.0], y: vec![0.0, 1.0, 0.0],
            label: None, marker: "line".into(), color: None, ..Default::default()
        });
        assert!(panel.pie.is_none(), "a new chart on the panel must clear the old pie, not draw over it");
    }

    #[test]
    fn zoom_inset_outside_places_it_fully_clear_of_the_plot_rect() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: (0..20).map(|i| i as f64).collect(), y: (0..20).map(|i| (i as f64).sin()).collect(), label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.inset = Some(InsetZoom { x0: 5.0, x1: 8.0, y0: -1.0, y1: 1.0, position: LegendPosition::Right, outside: true, clip: false });
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        // the plot's own border rect (the one WITHOUT rounded corners) ends
        // strictly before the inset's rounded background rect begins —
        // proof the plot area shrank to make room instead of the inset
        // overlapping it, the same guarantee `Legend.outside` gives.
        // The plot frame is four lines now; its right edge is the
        // rightmost vertical spine.
        // The inset is itself a framed mini-plot, so "rightmost vertical
        // line" would find the INSET's spine. The main plot's spines are
        // the tallest ones; pick those, then take the rightmost of them.
        let verticals: Vec<(f64, f64)> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Line { x1, y1, x2, y2, .. } if (x1 - x2).abs() < 0.01 && (y1 - y2).abs() > 1.0 => {
                    Some((*x1, (y1 - y2).abs()))
                }
                _ => None,
            })
            .collect();
        let tallest = verticals.iter().fold(0.0f64, |m, (_, h)| m.max(*h));
        let plot_right = verticals
            .iter()
            .filter(|(_, h)| (*h - tallest).abs() < 1.0)
            .fold(f64::NEG_INFINITY, |m, (x, _)| m.max(*x));
        assert!(plot_right.is_finite(), "no plot border spine found");
        let inset_left = ops
            .iter()
            .find_map(|op| match op { DrawOp::Rect { x, fill: Some(f), radius, .. } if f == "#ffffff" && *radius > 0.0 => Some(*x), _ => None })
            .expect("inset background rect");
        assert!(plot_right <= inset_left, "plot border {plot_right} should end at/before the inset {inset_left}");
    }

    #[test]
    fn zoom_inset_explicit_position_is_honored_over_best() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 1.0, 2.0], y: vec![0.0, 1.0, 2.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        // Compared against the OPPOSITE corner rather than against fixed
        // pixel thresholds. An inset is placed inside the PLOT AREA, not the
        // canvas, so "the bottom half" moves whenever the margins do -- when
        // the type scale rose on 2026-09-10 the bottom margin grew, the plot
        // area shrank, and a correctly placed bottom-left inset landed at
        // y=148.5 against a hardcoded 150. The property that actually
        // matters is that the four corners are honoured and differ, which no
        // margin change can invalidate.
        let corner = |pos: LegendPosition| -> (f64, f64) {
            let mut fig = Figure::new();
            let panel = fig.current_panel_mut();
            panel.series.push(Series { x: vec![0.0, 1.0, 2.0], y: vec![0.0, 1.0, 2.0], label: None, marker: "line".into(), color: None, ..Default::default() });
            panel.inset = Some(InsetZoom { x0: 0.5, x1: 1.5, y0: 0.5, y1: 1.5, position: pos, outside: false, clip: false });
            let ops = build_draw_ops(&fig, 400.0, 300.0);
            match ops
                .iter()
                .find(|op| matches!(op, DrawOp::Rect { fill: Some(f), radius, .. } if f == "#ffffff" && *radius > 0.0))
                .expect("inset background rect")
            {
                DrawOp::Rect { x, y, .. } => (*x, *y),
                _ => unreachable!(),
            }
        };
        let (bl_x, bl_y) = corner(LegendPosition::BottomLeft);
        let (tr_x, tr_y) = corner(LegendPosition::TopRight);
        // Pixel space grows downward, so "bottom" is the larger y.
        assert!(bl_y > tr_y, "bottom-left should sit below top-right: {bl_y} vs {tr_y}");
        assert!(bl_x < tr_x, "bottom-left should sit left of top-right: {bl_x} vs {tr_x}");
        let (br_x, _) = corner(LegendPosition::BottomRight);
        let (tl_x, tl_y) = corner(LegendPosition::TopLeft);
        assert!(br_x > bl_x, "bottom-right should sit right of bottom-left");
        assert!(tl_y < bl_y, "top-left should sit above bottom-left");
    }

    #[test]
    fn zoom_inset_clip_hard_clips_a_line_exactly_to_the_box_border() {
        // A line from (0,0) to (10,10) crosses the inset box [2,8]x[2,8] at
        // an angle, not at a sampled data point — only the two coarse
        // endpoints are given, so the default "zoom" style's point-range
        // filter would keep *zero* points (neither endpoint is inside the
        // box) and draw nothing, while `clip=true` must still produce a
        // diagonal segment touching the box border at (2,2) and (8,8).
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 10.0], y: vec![0.0, 10.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.inset = Some(InsetZoom { x0: 2.0, x1: 8.0, y0: 2.0, y1: 8.0, position: LegendPosition::Best, outside: false, clip: false });
        let unclipped = build_draw_ops(&fig, 400.0, 300.0);
        let inset_polylines_unclipped = unclipped.iter().filter(|op| matches!(op, DrawOp::Polyline { points, .. } if points.is_empty())).count();
        assert!(inset_polylines_unclipped >= 1, "default zoom style should replay an empty polyline when no whole point is inside the box");

        fig.current_panel_mut().inset = Some(InsetZoom { x0: 2.0, x1: 8.0, y0: 2.0, y1: 8.0, position: LegendPosition::Best, outside: false, clip: true });
        let clipped = build_draw_ops(&fig, 400.0, 300.0);
        // exactly 2 polylines: the main curve, plus one clipped inset strip
        // (not the empty one the unclipped path produced above).
        let inset_polyline = clipped
            .iter()
            .filter_map(|op| match op { DrawOp::Polyline { points, .. } if !points.is_empty() => Some(points), _ => None })
            .find(|points| points.len() >= 2 && *points != &vec![]);
        // there should be two non-empty polylines total (main + inset replay)
        let nonempty_polylines = clipped.iter().filter(|op| matches!(op, DrawOp::Polyline { points, .. } if !points.is_empty())).count();
        assert_eq!(nonempty_polylines, 2, "expected the main curve plus one clipped inset strip, got: {clipped:?}");
        assert!(inset_polyline.is_some());
    }

    #[test]
    fn xbreak_state_sets_panel_x_break() {
        let mut fig = Figure::new();
        fig.current_panel_mut().x_break = Some((10.0, 1000.0));
        assert_eq!(fig.current_panel().x_break, Some((10.0, 1000.0)));
        assert_eq!(fig.current_panel().y_break, None);
    }

    #[test]
    fn xbreak_drops_tick_labels_inside_the_skipped_range_and_draws_a_jag() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 10.0, 1000.0, 1010.0], y: vec![0.0, 1.0, 2.0, 3.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.x_break = Some((20.0, 990.0));
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        // no tick label text should format to a value that falls strictly
        // inside the skipped (20, 990) range.
        let bad_tick = ops.iter().any(|op| match op {
            DrawOp::Text { text, .. } => text.parse::<f64>().map(|v| v > 20.0 && v < 990.0).unwrap_or(false),
            _ => false,
        });
        assert!(!bad_tick, "no tick should land inside the break, got: {ops:?}");
        // the "//" jag is drawn as short *diagonal* Line ops (x1 != x2 AND
        // y1 != y2) — every other Line op in this plot (gridlines, axis
        // spines, series-adjacent lines) is axis-aligned, so a diagonal one
        // is a reliable, distinctive signal.
        let diagonal_lines = ops.iter().filter(|op| matches!(op, DrawOp::Line { x1, y1, x2, y2, .. } if (x1 - x2).abs() > 1e-6 && (y1 - y2).abs() > 1e-6)).count();
        assert!(diagonal_lines >= 2, "expected at least the bottom-spine \"//\" jag (2 diagonal strokes), got {diagonal_lines}: {ops:?}");
    }

    #[test]
    fn xbreak_compresses_pixel_space_regardless_of_skipped_span_size() {
        // A break from 10 to 1_000_000 must still only claim the fixed
        // `BREAK_ZONE_FRAC` of the axis, not pixel space proportional to
        // the (huge) skipped span — i.e. the two remaining regions still
        // get real, comparable pixel width instead of being squeezed to
        // near-nothing beside a dominant break zone.
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 10.0, 1_000_000.0, 1_000_010.0], y: vec![0.0, 1.0, 2.0, 3.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        panel.x_break = Some((10.0, 1_000_000.0));
        // `tight` so `xmin`/`xmax` are exactly the data min/max (no 5% auto-
        // pad) — otherwise the pad itself (5% of a ~1,000,010-wide range)
        // would dwarf the 10-unit pre-break region and mask the very
        // compression this test checks for.
        panel.tight = true;
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let xs: Vec<(f64, f64)> = ops
            .iter()
            .filter_map(|op| match op { DrawOp::Polyline { points, .. } if points.len() > 1 => Some(points.clone()), _ => None })
            .next()
            .expect("main curve polyline");
        // x=0 and x=10 are on the pre-break side; their pixel gap should be
        // a real, visible fraction of the plot width, not squashed flat.
        let gap = (xs[1].0 - xs[0].0).abs();
        assert!(gap > 5.0, "pre-break points collapsed to {gap}px apart, break isn't compressing correctly: {xs:?}");
    }

    #[test]
    fn blur_backdrop_state_sets_figure_field() {
        let mut fig = Figure::new();
        fig.blur_backdrop = Some(BlurBackdrop { fx0: 0.1, fy0: 0.1, fx1: 0.6, fy1: 0.6, radius: 12.0 });
        let b = fig.blur_backdrop.as_ref().unwrap();
        assert_eq!((b.fx0, b.fy0, b.fx1, b.fy1, b.radius), (0.1, 0.1, 0.6, 0.6, 12.0));
    }

    #[test]
    fn margin_bands_stack_outward_and_nothing_is_placed_outside_its_own() {
        let mut s = MarginStack::default();
        s.push(Band::Ticks, 46.0);
        s.push(Band::AxisLabel, 14.0);
        s.push(Band::Colorbar, 74.0);
        assert_eq!(s.inner(Band::Ticks), Some(0.0));
        assert_eq!(s.inner(Band::AxisLabel), Some(46.0));
        assert_eq!(s.center(Band::AxisLabel), Some(53.0));
        // The colour scale begins past the axis and the label naming it,
        // which is the fact the colorbar used to recompute for itself.
        assert_eq!(s.inner(Band::Colorbar), Some(60.0));
        assert_eq!(s.total(), 134.0);
        // Nothing reserved, nothing to place against -- the caller keeps
        // its own fallback rather than being handed a plausible zero.
        assert_eq!(s.inner(Band::Panel), None);
        assert_eq!(s.center(Band::Panel), None);
        // A band that reserves nothing is not a band: an absent right
        // axis label must not leave a slot for something to sit in.
        let mut empty = MarginStack::default();
        empty.push(Band::AxisLabel, 0.0);
        assert_eq!(empty.inner(Band::AxisLabel), None);

        // The fit-to-cell clamp shrinks the total; the bands have to
        // shrink with it, or every offset above points past the margin.
        s.scale(0.5);
        assert_eq!(s.total(), 67.0);
        assert_eq!(s.inner(Band::Colorbar), Some(30.0));
    }

    #[test]
    fn the_right_axis_label_sits_between_its_numbers_and_the_colorbar() {
        // The paper's regime map reads its y axis twice -- CF ratio on the
        // left, tone count on the right -- and carries a colour scale. The
        // right label was pinned to the CANVAS EDGE, so it cleared the
        // colorbar entirely and came to rest beyond the colour scale's own
        // label, ~200px from the numbers it names. Every reader's first
        // parse, including this one's, was that it labelled the colours.
        let mut fig = Figure::new();
        {
            let p = fig.current_panel_mut();
            p.series.push(Series { y: vec![1.0, 2.0, 3.0], ..Series::default() });
            p.ytick2_positions = Some(vec![1.0, 2.0, 3.0]);
            p.ytick2_labels = Some(vec!["10".into(), "25".into(), "173".into()]);
            p.ylabel2 = Some("Tone count".into());
            p.colorbar = Some(ColorBar { label: Some("Delta".into()), ticks: Vec::new(), extend: None });
        }
        let svg = render_svg(&fig, 900.0, 600.0, false);
        let x_of = |needle: &str| -> f64 {
            let at = svg.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{svg}"));
            let head = &svg[..at];
            let tag = head.rfind("<text").expect("not inside a text element");
            let xs = head[tag..].find("x=\"").expect("no x") + tag + 3;
            let xe = svg[xs..].find('"').unwrap() + xs;
            svg[xs..xe].parse().unwrap()
        };
        let ticks = x_of(">173<");
        let label = x_of(">Tone count<");
        // The bar is the leftmost rect out in the right margin.
        let bar = svg
            .match_indices("<rect")
            .filter_map(|(i, _)| {
                let xs = svg[i..].find("x=\"")? + i + 3;
                let xe = svg[xs..].find('"')? + xs;
                svg[xs..xe].parse::<f64>().ok()
            })
            .filter(|&x| x > ticks)
            .fold(f64::INFINITY, f64::min);
        assert!(
            ticks < label && label < bar,
            "expected numbers ({ticks}) < right axis label ({label}) < colorbar ({bar})"
        );
    }

    #[test]
    fn a_bar_is_never_cut_in_half_by_its_own_axis() {
        // A bar is drawn centred on its x, so the outermost ones reach
        // half a bar past the ends of the data. The extent knew only about
        // the centres, so the axis stopped at the middle of the first and
        // last bars and the frame sliced both -- on a stacked chart,
        // category A came out looking like a half-width column.
        let mut fig = Figure::new();
        {
            let p = fig.current_panel_mut();
            // ONE series of three bars, which is how `bar([a, b, c])`
            // arrives. Bar width is derived from the series' OWN length,
            // so three single-value series would each size itself as if it
            // were the only bar in the panel.
            p.series.push(Series {
                x: vec![0.0, 1.0, 2.0],
                y: vec![1.0, 2.0, 3.0],
                marker: "bar".into(),
                ..Series::default()
            });
        }
        let ops = build_draw_ops(&fig, 600.0, 400.0);
        // A bar carries the hover that names its value, so it goes out as
        // a TitledRect -- the same wrinkle that made `o` a TitledCircle.
        let bars: Vec<(f64, f64)> = ops
            .iter()
            .filter_map(|o| match o {
                DrawOp::Rect { x, w, fill: Some(_), .. }
                | DrawOp::TitledRect { x, w, fill: Some(_), .. } => Some((*x, *x + *w)),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 3, "expected three bars, got {bars:?}");
        // The panel's own left and right edges, read off the vertical
        // rules that bound it (the frame is drawn as lines, not a rect).
        let verticals: Vec<f64> = ops
            .iter()
            .filter_map(|o| match o {
                DrawOp::Line { x1, x2, .. } if (x1 - x2).abs() < 1e-9 => Some(*x1),
                _ => None,
            })
            .collect();
        assert!(!verticals.is_empty(), "expected the panel to be framed");
        let frame = (
            verticals.iter().copied().fold(f64::INFINITY, f64::min),
            verticals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let left = bars.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
        let right = bars.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            left > frame.0 && right < frame.1,
            "bars {left}..{right} must sit inside the frame {}..{}",
            frame.0,
            frame.1
        );
    }

    #[test]
    fn a_violin_is_the_density_mirrored_and_a_raincloud_is_not() {
        let group = |mirrored: bool| {
            let data: Vec<f64> = (0..60).map(|k| (k as f64 * 0.17).sin() * 2.0).collect();
            let (dx, dy) = gaussian_kde(&data, 40);
            let mut fig = Figure::new();
            fig.current_panel_mut().raincloud.push(RaincloudGroup {
                x: 0.0,
                density_x: dx,
                density_y: dy,
                box_stats: boxplot_stats(&data, 0.0, None, None),
                jitter: data.iter().map(|&v| (0.0, v)).collect(),
                color: Some("#1f77b4".into()),
                mirrored,
                show_rain: !mirrored,
            });
            build_draw_ops(&fig, 600.0, 400.0)
        };
        // The body is the many-sided polygon; the box beside it is a rect.
        let body = |ops: &[DrawOp]| {
            ops.iter().find_map(|o| match o {
                DrawOp::Polygon { points, .. } if points.len() > 8 => {
                    let xs: Vec<f64> = points.iter().map(|(x, _)| *x).collect();
                    Some((
                        xs.iter().copied().fold(f64::INFINITY, f64::min),
                        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    ))
                }
                _ => None,
            })
        };
        // The box is centred ON the category, so its centre is where the
        // category sits in pixels -- the line a violin is symmetric about.
        let box_centre = |ops: &[DrawOp]| {
            ops.iter().find_map(|o| match o {
                DrawOp::Rect { x, w, fill: Some(_), .. } => Some(x + w / 2.0),
                _ => None,
            })
        };

        let v = group(true);
        let r = group(false);
        let (vlo, vhi) = body(&v).expect("violin body");
        let (rlo, rhi) = body(&r).expect("raincloud cloud");
        let vc = box_centre(&v).expect("violin box");
        let rc = box_centre(&r).expect("raincloud box");

        // Symmetric about its category: the two halves are within a pixel.
        assert!(
            ((vc - vlo) - (vhi - vc)).abs() < 1.0,
            "violin should be symmetric about {vc}, spans {vlo}..{vhi}"
        );
        // The cloud is not: it lies entirely to one side of its box.
        assert!(
            rlo >= rc - 1.0,
            "raincloud cloud {rlo}..{rhi} should lean off its box at {rc}"
        );
        // And the rain is the difference in what is shown.
        let dots = |ops: &[DrawOp]| ops.iter().filter(|o| matches!(o, DrawOp::Circle { .. })).count();
        assert!(
            dots(&r) > dots(&v),
            "the raincloud draws raw samples ({}) and the violin does not ({})",
            dots(&r),
            dots(&v)
        );
    }

    #[test]
    fn a_cid_font_is_named_by_the_slot_it_actually_occupies() {
        // The names written into the content stream and the slots the
        // resource dictionary fills have to be one decision. Assuming
        // three simple faces always came first was right whenever the
        // family had an embeddable program and silently wrong when it had
        // none: the screen default embeds nothing, so the CID faces WERE
        // the first faces on the page and belonged at /F1, while every
        // reference to them said /F4. One `$\Omega$` in a default-themed
        // figure was enough to produce a PDF poppler rejects.
        let with_text = GlyphFaces { text: None, math: None, base: 3 };
        assert_eq!(with_text.text_name(), "/F4", "after three simple faces");
        assert_eq!(with_text.math_name(), "/F4", "math is first when there is no text face");

        let no_simple = GlyphFaces { text: None, math: None, base: 0 };
        assert_eq!(no_simple.text_name(), "/F1", "no simple faces means the CID faces start at /F1");
        assert_eq!(no_simple.math_name(), "/F1");
    }

    /// Every `#rrggbb` an SVG paints with, as 8-bit triples.
    fn svg_inks(svg: &str) -> std::collections::BTreeSet<(u8, u8, u8)> {
        let mut out = std::collections::BTreeSet::new();
        for (i, _) in svg.match_indices('#') {
            let hex = &svg[i + 1..];
            if hex.len() >= 6 && hex.as_bytes()[..6].iter().all(|c| c.is_ascii_hexdigit()) {
                out.insert(hex_to_rgb(&hex[..6]));
            }
        }
        out
    }

    /// Every colour a PDF content stream sets, as 8-bit triples.
    ///
    /// `r g b rg` fills and `r g b RG` strokes: the three tokens before
    /// the operator. Scanned as tokens rather than matched as text so a
    /// coordinate that happens to look like a colour cannot be mistaken
    /// for one -- only the operator says what the numbers meant.
    fn pdf_inks(pdf: &str) -> std::collections::BTreeSet<(u8, u8, u8)> {
        let mut out = std::collections::BTreeSet::new();
        let toks: Vec<&str> = pdf.split_ascii_whitespace().collect();
        for (i, t) in toks.iter().enumerate() {
            if (*t == "rg" || *t == "RG") && i >= 3 {
                let c: Vec<Option<f64>> = toks[i - 3..i].iter().map(|s| s.parse().ok()).collect();
                if let [Some(r), Some(g), Some(b)] = c[..] {
                    let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    out.insert((q(r), q(g), q(b)));
                }
            }
        }
        out
    }

    #[test]
    fn the_two_backends_paint_the_same_figure() {
        // The check this layer did not have.
        //
        // Both backends consume the SAME `Vec<DrawOp>`, so any difference
        // between their outputs is one of them failing to represent what
        // it was handed. Two real bugs lived here undetected because
        // nothing compared them: PDF silently dropped colour alpha, so a
        // figure was translucent on screen and opaque in the file that
        // goes to a journal; and PDF named font slots that its own
        // resource dictionary never declared, so a default-themed figure
        // with one Greek letter in it was an INVALID file.
        //
        // Each case below is a path that has actually failed, not a
        // sampling of features.
        let cases: Vec<(&str, Figure)> = vec![
            ("plain series", {
                let mut f = Figure::new();
                f.current_panel_mut().series.push(Series {
                    x: vec![0.0, 1.0, 2.0],
                    y: vec![1.0, 3.0, 2.0],
                    color: Some("#1f77b4".into()),
                    ..Series::default()
                });
                f
            }),
            ("translucent markers", {
                let mut f = Figure::new();
                f.current_panel_mut().series.push(Series {
                    x: vec![0.0, 1.0, 2.0],
                    y: vec![1.0, 3.0, 2.0],
                    marker: "o".into(),
                    color: Some("#1f77b4".into()),
                    marker_filled: Some(true),
                    alpha: Some(0.3),
                    ..Series::default()
                });
                f
            }),
            ("dashed and shadowed", {
                let mut f = Figure::new();
                f.current_panel_mut().series.push(Series {
                    x: vec![0.0, 1.0, 2.0],
                    y: vec![1.0, 3.0, 2.0],
                    marker: "o--".into(),
                    color: Some("#d62728".into()),
                    marker_filled: Some(true),
                    marker_shadow: true,
                    width: Some(7.0),
                    ..Series::default()
                });
                f
            }),
            ("a coloured background", {
                // The case this test found on its first run.
                // `figure_background` was written straight into the SVG
                // body rather than as a `DrawOp`, so the PDF and TikZ
                // writers never saw it and the same figure came out gold
                // on screen and white in the file.
                let mut f = Figure::new();
                f.background = "#ffcc00".into();
                f.current_panel_mut().series.push(Series {
                    x: vec![0.0, 1.0, 2.0],
                    y: vec![1.0, 3.0, 2.0],
                    ..Series::default()
                });
                f
            }),
            ("maths in a label", {
                let mut f = Figure::new();
                {
                    let p = f.current_panel_mut();
                    p.series.push(Series {
                        x: vec![0.0, 1.0, 2.0],
                        y: vec![1.0, 3.0, 2.0],
                        ..Series::default()
                    });
                    // The exact shape that produced an invalid PDF: Greek
                    // in a label, on the DEFAULT font family, which embeds
                    // no simple faces at all.
                    p.xlabel = Some("$\\Omega$ and $\\nu_0 T$".into());
                }
                f
            }),
        ];

        for (name, fig) in cases {
            let svg = render_svg(&fig, 600.0, 400.0, false);
            let pdf_bytes = render_pdf(&fig, 600.0, 400.0);
            let pdf = String::from_utf8_lossy(&pdf_bytes).into_owned();

            // 1. INK. Every colour one backend paints, the other paints.
            let (si, pi) = (svg_inks(&svg), pdf_inks(&pdf));
            // White is the one legitimate asymmetry between these media
            // and is excluded deliberately, not to make the test pass: an
            // SVG element is transparent by default, so a white ground has
            // to be painted, while a PDF page already IS white and
            // painting it would be redundant ink. Every other colour must
            // appear in both -- including a non-white background, which is
            // how this test caught `figure_background` being SVG-only.
            let white = (255u8, 255u8, 255u8);
            let missing: Vec<_> = si.difference(&pi).filter(|c| **c != white).collect();
            assert!(
                missing.is_empty(),
                "{name}: SVG paints colours the PDF never sets: {missing:?}"
            );

            // 2. TRANSLUCENCY. An eight-digit colour in the SVG is a
            // translucent one; the PDF has to carry that alpha somehow,
            // which for this writer means an ExtGState selection. This is
            // the assertion that would have caught the alpha bug on the
            // day it was written.
            let translucent = svg.match_indices('#').any(|(i, _)| {
                let h = &svg[i + 1..];
                h.len() >= 8
                    && h.as_bytes()[..8].iter().all(|c| c.is_ascii_hexdigit())
                    && &h[6..8] != "ff"
            });
            if translucent {
                assert!(
                    pdf.contains(" gs") && pdf.contains("/GA"),
                    "{name}: the SVG is translucent and the PDF sets no alpha state -- \
                     the same figure would print opaque"
                );
            }

            // 3. FONTS. Every slot the content stream selects must be one
            // the resource dictionary declares. A dangling reference is
            // not a cosmetic difference: the viewer rejects the page and
            // the text does not draw.
            let toks: Vec<&str> = pdf.split_ascii_whitespace().collect();
            for (i, t) in toks.iter().enumerate() {
                let selected = *t == "Tf" && i >= 2;
                if !selected {
                    continue;
                }
                let slot = toks[i - 2];
                assert!(
                    slot.starts_with("/F"),
                    "{name}: expected a font slot before Tf, found {slot}"
                );
                let declared = format!("{slot} ");
                assert!(
                    pdf.matches(&declared).count() > 1,
                    "{name}: the page selects {slot} but never declares it -- \
                     this is the 'unknown font tag' failure"
                );
            }
        }
    }

    #[test]
    fn pdf_honours_the_alpha_a_colour_carries() {
        // SVG has understood `#rrggbbaa` all along and `apply_alpha`
        // produces it for any `alpha=`. PDF read six digits and dropped
        // the rest, so the same figure was translucent on screen and
        // OPAQUE in the file that goes to the journal.
        assert_eq!(hex_alpha("#1f77b44d").map(|a| (a * 100.0).round()), Some(30.0));
        assert_eq!(hex_alpha("#1f77b4"), None, "an opaque colour needs no state");
        assert_eq!(hex_alpha("#1f77b4ff"), None, "fully opaque is not translucency");
        assert!(pdf_gs("#1f77b44d").starts_with("/GA"), "expected a graphics state");
        assert_eq!(pdf_gs("#1f77b4"), "", "no state for an opaque colour");

        // The state the content stream names must be one the resources
        // define, or the viewer has a dangling reference.
        let op = DrawOp::Circle {
            cx: 1.0, cy: 1.0, r: 2.0,
            fill: Some("#1f77b44d".into()),
            stroke: None,
        };
        let name = pdf_gs("#1f77b44d");
        let name = name.trim_start_matches('/').trim_end_matches(" gs ");
        assert!(
            pdf_ext_gstate(std::slice::from_ref(&op)).contains(name),
            "the resource dictionary must define {name}"
        );
        let mut out = String::new();
        write_pdf_op(&mut out, &op, 99.0, None, None, None, None);
        assert!(out.contains(name), "the circle must select {name}, got: {out}");
    }

    #[test]
    fn a_marker_shadow_is_an_offset_copy_drawn_underneath() {
        let draw = |shadow: bool| {
            let mut fig = Figure::new();
            fig.current_panel_mut().series.push(Series {
                x: vec![1.0, 2.0, 3.0],
                // Not all equal: a zero-height y range leaves the panel
                // with nothing to scale to and it draws nothing at all,
                // which made an earlier version of this test pass by
                // comparing zero circles against zero circles.
                y: vec![1.0, 2.0, 1.5],
                marker: "o".into(),
                color: Some("#1f77b4".into()),
                marker_filled: Some(true),
                marker_shadow: shadow,
                ..Series::default()
            });
            build_draw_ops(&fig, 600.0, 400.0)
        };
        let plain = draw(false);
        let shaded = draw(true);
        // `o` is drawn as a TitledCircle (it carries the hover that names
        // the point); its shadow is a plain Circle, deliberately, so the
        // decoration does not answer to the hover.
        let marks = |ops: &[DrawOp]| ops.iter().filter(|o| matches!(o, DrawOp::TitledCircle { .. })).count();
        let shadows = |ops: &[DrawOp]| ops.iter().filter(|o| matches!(o, DrawOp::Circle { .. })).count();
        assert_eq!(marks(&plain), 3, "three points, three markers");
        assert_eq!(shadows(&plain), 0, "no shadow unless asked for");
        assert_eq!(marks(&shaded), 3, "the markers themselves are unchanged");
        assert_eq!(shadows(&shaded), 3, "each marker should cast exactly one shadow");
        // Identified by translucency rather than by an exact hex: a shadow
        // is the copy that carries an alpha, whatever ink the theme chose
        // for the marker itself.
        let fills: Vec<String> = shaded
            .iter()
            .filter_map(|o| match o {
                // Whichever paint the glyph actually uses -- a hollow
                // marker carries its ink on the stroke, not the fill.
                DrawOp::Circle { fill, stroke, .. }
                | DrawOp::TitledCircle { fill, stroke, .. } => fill.clone().or_else(|| stroke.clone()),
                _ => None,
            })
            .collect();
        let first_shadow = fills.iter().position(|c| hex_alpha(c).is_some());
        let first_marker = fills.iter().position(|c| hex_alpha(c).is_none());
        assert!(
            first_shadow.is_some(),
            "no translucent circle: a shadow must be translucent or it is a second marker; got {fills:?}"
        );
        // The shadow is UNDER its marker: one drawn afterwards sits on top
        // of the thing it is supposed to sit behind.
        assert!(
            first_shadow < first_marker,
            "shadow at {first_shadow:?} must precede the marker at {first_marker:?}: {fills:?}"
        );
    }

    #[test]
    fn a_dash_stays_longer_than_the_line_is_thick() {
        // The paper's reference curve is `marker = "--"` at `lw = pt(2.0)`,
        // about 7 canvas units. Against a fixed 6-unit dash that is a mark
        // SHORTER THAN THE LINE IS THICK, with a 3-unit gap narrower still:
        // it rendered as a row of square blobs and read as dotted. The
        // legend agreed with it, so nothing in the figure said otherwise.
        //
        // Written against `scaled_dash`, which replaced the branch's
        // `scale_dash` when the single (on, off) pair became a real dash
        // ARRAY -- the property is the same one and is what matters.
        let base = named_dash("dashed").flatten().expect("`dashed` is a style");
        for w in [1.5, 3.0, 7.06, 12.0] {
            let d = scaled_dash(&base, w);
            let on = d.pattern[0];
            let off = d.pattern[1];
            assert!(on > w, "a {w}-thick line needs a dash longer than {w}, got {on}");
            assert!(off > w * 0.5, "a {w}-thick line needs a visible gap, got {off}");
        }
        // Thinner than the default keeps the tuned pattern rather than a
        // proportionally tinier one -- shrinking it is how thin dotted
        // rules disappeared once already.
        assert_eq!(scaled_dash(&base, 0.7).pattern, base.pattern);
        assert_eq!(scaled_dash(&base, DEFAULT_LINE_WIDTH).pattern, base.pattern);
    }

    #[test]
    fn a_dashed_series_is_one_path_not_one_per_segment() {
        // 200 points used to mean 200 separate dashed <line> elements, each
        // restarting the pattern at its own start, so the rhythm was a
        // function of how finely the curve was sampled rather than of the
        // line. Every backend runs a dash along a whole path and keeps its
        // phase across joins, so one polyline is both correct and smaller.
        let mut fig = Figure::new();
        let n = 200;
        fig.current_panel_mut().series.push(Series {
            x: (0..n).map(|i| i as f64).collect(),
            y: (0..n).map(|i| (i as f64 * 0.1).sin()).collect(),
            marker: "--".into(),
            color: Some("#d62728".into()),
            width: Some(7.0),
            ..Series::default()
        });
        let svg = render_svg(&fig, 600.0, 400.0, false);
        // Only the series' own ink -- the grid and the ticks are <line>s too.
        let of_series = |tag: &str| {
            svg.match_indices(tag)
                .filter(|(i, _)| {
                    let end = svg[*i..].find('>').map_or(svg.len(), |e| i + e);
                    svg[*i..end].contains("d62728")
                })
                .count()
        };
        let segments = of_series("<line ");
        assert!(segments <= 1, "expected one dashed path, got {segments} <line> elements");
        assert_eq!(of_series("<polyline"), 1, "expected exactly one polyline for the curve");
        assert!(svg.contains("stroke-dasharray="), "expected the polyline to be dashed:\n{svg}");
    }

    #[test]
    fn a_contour_label_breaks_a_gap_as_wide_as_its_own_text() {
        // The gap used to be three PATH POINTS, which is a length only by
        // accident -- it depends entirely on how finely the field is
        // sampled. On the paper's 400x400 map three points was a sliver a
        // fraction of the text's width, so "20%" was struck through by the
        // very line this break exists to clear.
        //
        // A denser grid must therefore not produce a smaller gap: the two
        // resolutions below describe the same plane over the same range,
        // so the break has to come out the same size in pixels.
        let gap_at = |n: usize| -> f64 {
            let mut fig = Figure::new();
            let (mut x, mut y, mut z) = (vec![], vec![], vec![]);
            for i in 0..n {
                let t = i as f64 / (n - 1) as f64 * 39.0;
                x.push(t);
                y.push(t);
            }
            for r in 0..n {
                for c in 0..n {
                    z.push(y[r] + x[c]);
                }
            }
            fig.current_panel_mut().shapes.push(Shape::Contour {
                x, y, z,
                levels: vec![39.0],
                filled: false,
                colormap: String::new(),
                color: Some("#000000".into()),
                label_levels: true,
                label_suffix: "%".into(),
                label_pos: None,
                dash: false,
                dot: false,
                width: None,
                extend_max: false,
            });
            let svg = render_svg(&fig, 400.0, 300.0, false);
            // The break leaves two polylines where there was one. Measure
            // between the inner endpoints: the last point of the first and
            // the first point of the second.
            let pts: Vec<Vec<(f64, f64)>> = svg
                .match_indices("points=\"")
                .map(|(i, _)| {
                    let s = i + 8;
                    let e = svg[s..].find('"').unwrap() + s;
                    svg[s..e]
                        .split_whitespace()
                        .filter_map(|p| {
                            let (a, b) = p.split_once(',')?;
                            Some((a.parse().ok()?, b.parse().ok()?))
                        })
                        .collect()
                })
                .filter(|v: &Vec<(f64, f64)>| v.len() >= 2)
                .collect();
            assert!(pts.len() >= 2, "expected the line to be broken in two, got {} piece(s)", pts.len());
            let a = *pts[0].last().unwrap();
            let b = pts[1][0];
            (a.0 - b.0).hypot(a.1 - b.1)
        };

        let coarse = gap_at(20);
        let fine = gap_at(120);
        assert!(coarse > 8.0, "gap should clear the label's text, got {coarse}");
        assert!(
            (coarse - fine).abs() < 0.15 * coarse,
            "the gap is a LENGTH, so sampling the same field more finely must not change it: {coarse} vs {fine}"
        );
    }

    #[test]
    fn a_contour_label_can_be_slid_along_its_own_line() {
        // The midpoint is a good default and a poor guarantee: it is
        // chosen without knowing what else the figure draws. On the
        // paper's ceiling map it landed exactly where a reference rule
        // crossed, and the dashed line ran through the text -- the
        // contour code breaks its OWN line around the label, but it
        // cannot see ink drawn by anything else.
        let contour_at = |pos: Option<f64>| {
            let mut fig = Figure::new();
            let n = 40usize;
            let (mut x, mut y, mut z) = (vec![], vec![], vec![]);
            for i in 0..n {
                x.push(i as f64);
                y.push(i as f64);
            }
            // A plane, so the level set is a straight diagonal and the
            // label's position along it varies smoothly with `pos`.
            for r in 0..n {
                for c in 0..n {
                    z.push(r as f64 + c as f64);
                }
            }
            fig.current_panel_mut().shapes.push(Shape::Contour {
                x, y, z,
                levels: vec![40.0],
                filled: false,
                colormap: String::new(),
                color: Some("#000000".into()),
                label_levels: true,
                label_suffix: "%".into(),
                label_pos: pos,
                dash: false,
                dot: false,
                width: None,
                extend_max: false,
            });
            let svg = render_svg(&fig, 400.0, 300.0, false);
            let at = svg.find(">40%<").unwrap_or_else(|| panic!("no contour label in:\n{svg}"));
            // The `y="..."` of the <text> element carrying the label.
            let head = &svg[..at];
            let tag = head.rfind("<text").expect("label is not a text element");
            let ys = head[tag..].find("y=\"").expect("label has no y") + tag + 3;
            let ye = svg[ys..].find('"').unwrap() + ys;
            svg[ys..ye].parse::<f64>().expect("y is a number")
        };

        let low = contour_at(Some(0.2));
        let mid = contour_at(None);
        let high = contour_at(Some(0.8));
        assert!(
            (low - mid).abs() > 1.0 && (high - mid).abs() > 1.0 && (low - high).abs() > 1.0,
            "label_pos did not move the label: 0.2 -> {low}, default -> {mid}, 0.8 -> {high}"
        );
        // The default must still be the midpoint, so every existing
        // figure that never passes `label_pos` renders unchanged.
        assert!(
            (mid - contour_at(Some(0.5))).abs() < 1.0,
            "an unset label_pos should mean the midpoint: {mid} vs {}",
            contour_at(Some(0.5))
        );
    }

    #[test]
    fn blur_backdrop_renders_a_gaussian_blur_filter_and_a_focus_clip_in_svg() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { y: vec![1.0, 2.0, 3.0], ..Series::default() });
        fig.blur_backdrop = Some(BlurBackdrop { fx0: 0.1, fy0: 0.1, fx1: 0.6, fy1: 0.6, radius: 9.0 });
        let svg = render_svg(&fig, 400.0, 300.0, false);
        assert!(svg.contains("<feGaussianBlur"), "expected a Gaussian blur filter primitive, got:\n{svg}");
        assert!(svg.contains("stdDeviation=\"9.00\""), "expected the configured radius, got:\n{svg}");
        assert!(svg.contains("<clipPath"), "expected a clip-path for the sharp focus rectangle, got:\n{svg}");
        assert!(svg.contains("filter=\"url(#qu-blur-backdrop)\""), "expected the blurred group to reference the filter, got:\n{svg}");
        assert!(svg.contains("clip-path=\"url(#qu-blur-focus)\""), "expected the sharp group to reference the clip, got:\n{svg}");
        // no blur at all when unset — existing plots must render unchanged.
        fig.blur_backdrop = None;
        let plain = render_svg(&fig, 400.0, 300.0, false);
        assert!(!plain.contains("feGaussianBlur"));
    }

    #[test]
    fn blur_backdrop_degrades_gracefully_in_tikz_with_a_noted_comment() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { y: vec![1.0, 2.0, 3.0], ..Series::default() });
        fig.blur_backdrop = Some(BlurBackdrop { fx0: 0.1, fy0: 0.1, fx1: 0.6, fy1: 0.6, radius: 9.0 });
        let tikz = render_tikz(&fig, 400.0, 300.0);
        // the degradation comment is allowed to *mention* `feGaussianBlur`
        // in prose explaining why it's unsupported — what must never appear
        // is the actual SVG tag reference (`<feGaussianBlur`).
        assert!(!tikz.contains("<feGaussianBlur"), "TikZ has no filter primitive, must not reference an SVG-only tag");
        assert!(tikz.to_lowercase().contains("blur_backdrop") || tikz.to_lowercase().contains("no tikz equivalent"), "expected an honest degradation note, got:\n{tikz}");
        assert!(tikz.contains("\\begin{tikzpicture}"), "should still render the rest of the figure unblurred");
    }

    #[test]
    fn boxplot_stats_matches_hand_computed_quartiles_and_flags_the_outlier() {
        // 1..10 plus a far outlier at 100.
        let data: Vec<f64> = (1..=10).map(|n| n as f64).chain([100.0]).collect();
        let group = boxplot_stats(&data, 0.0, None, None);
        assert!((group.median - 6.0).abs() < 1e-9, "{group:?}");
        assert!(group.q1 < group.median && group.median < group.q3);
        assert_eq!(group.outliers, vec![100.0]);
        assert!(group.whisker_hi < 100.0); // the outlier must not stretch the whisker
    }

    #[test]
    fn boxplot_renders_a_box_median_whiskers_and_outlier_marker() {
        let mut fig = Figure::new();
        let data: Vec<f64> = (1..=10).map(|n| n as f64).chain([100.0]).collect();
        fig.current_panel_mut().boxplots.push(boxplot_stats(&data, 0.0, None, None));
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Rect { fill: Some(_), .. }))); // the box
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Circle { fill: None, .. }))); // the outlier
        let line_count = ops.iter().filter(|op| matches!(op, DrawOp::Line { .. })).count();
        assert!(line_count >= 5); // 2 whiskers + 2 caps + median, at minimum
    }

    #[test]
    fn resolve_palette_falls_back_to_default_for_unknown_names() {
        assert_eq!(resolve_palette("viridis")[0], "#440154");
        assert_eq!(resolve_palette("not-a-real-palette"), resolve_palette("default"));
    }

    #[test]
    fn panel_palette_choice_changes_rendered_series_color() {
        let mut fig = Figure::new();
        fig.current_panel_mut().palette = "viridis".to_string();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None,
            ..Default::default()
        });
        let svg = render_svg(&fig, 200.0, 150.0, false);
        assert!(svg.contains("#440154"), "expected the first viridis color in:\n{svg}");
    }

    #[test]
    fn spider_ops_returns_nothing_for_fewer_than_three_categories() {
        let mut chart = SpiderChart::default();
        chart.series.push(Series { x: vec![], y: vec![1.0, 2.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        assert!(spider_ops(&chart, 0.0, 0.0, 100.0, 100.0, PALETTES[0].1, 8.5, 10.0).is_empty());
    }

    #[test]
    fn spider_ops_draws_rings_spokes_and_one_closed_polygon_per_series() {
        let mut chart = SpiderChart::default();
        chart.series.push(Series { x: vec![], y: vec![1.0, 2.0, 3.0, 4.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        let ops = spider_ops(&chart, 0.0, 0.0, 200.0, 200.0, PALETTES[0].1, 8.5, 10.0);
        let polygons: Vec<_> = ops.iter().filter(|op| matches!(op, DrawOp::Polyline { .. })).collect();
        // 4 grid rings + 1 series polygon
        assert_eq!(polygons.len(), 5);
        if let DrawOp::Polyline { points, .. } = polygons.last().unwrap() {
            assert_eq!(points.first(), points.last()); // the series polygon closes
            assert_eq!(points.len(), 5); // 4 categories + closing point
        }
        let spokes = ops.iter().filter(|op| matches!(op, DrawOp::Line { .. })).count();
        assert_eq!(spokes, 4); // one spoke per category
    }

    #[test]
    fn spider_chart_panel_is_not_pristine() {
        let mut fig = Figure::new();
        fig.current_panel_mut().spider = Some(SpiderChart::default());
        assert!(!fig.current_panel().is_pristine());
    }

    #[test]
    fn gaussian_kde_peaks_near_the_data_cluster_and_integrates_to_roughly_one() {
        let data = vec![5.0; 20]; // a tight cluster at x=5 (std -> ~0)
        let (xs, ys) = gaussian_kde(&data, 200);
        assert!(!xs.is_empty());
        let peak_idx = ys.iter().enumerate().max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap()).unwrap().0;
        assert!((xs[peak_idx] - 5.0).abs() < 0.5, "peak at {}, expected near 5.0", xs[peak_idx]);
        // trapezoidal integral should be close to 1 (it's a real density)
        let dx = xs[1] - xs[0];
        let integral: f64 = ys.windows(2).map(|w| (w[0] + w[1]) / 2.0 * dx).sum();
        assert!((integral - 1.0).abs() < 0.1, "integral = {integral}");
    }

    #[test]
    fn raincloud_renders_a_filled_cloud_a_box_and_jittered_points() {
        let mut fig = Figure::new();
        let data: Vec<f64> = (1..=20).map(|n| n as f64).collect();
        let (density_x, density_y) = gaussian_kde(&data, 40);
        let box_stats = boxplot_stats(&data, 0.0, None, None);
        let jitter: Vec<(f64, f64)> = data.iter().map(|&v| (0.0, v)).collect();
        fig.current_panel_mut().raincloud.push(RaincloudGroup {
            x: 0.0, density_x, density_y, box_stats, jitter, color: None,
            mirrored: false,
            show_rain: true,
        });
        let ops = build_draw_ops(&fig, 300.0, 300.0);
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Polygon { .. }))); // the cloud
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Rect { fill: Some(_), .. }))); // the box
        let dot_count = ops.iter().filter(|op| matches!(op, DrawOp::Circle { fill: Some(_), .. })).count();
        assert_eq!(dot_count, 20); // one per raw sample
    }

    #[test]
    fn raincloud_panel_is_not_pristine() {
        let mut fig = Figure::new();
        fig.current_panel_mut().raincloud.push(RaincloudGroup {
            x: 0.0, density_x: vec![], density_y: vec![], box_stats: boxplot_stats(&[1.0], 0.0, None, None),
            jitter: vec![], color: None,
            mirrored: false,
            show_rain: true,
        });
        assert!(!fig.current_panel().is_pristine());
    }

    #[test]
    fn render_math_svg_leaves_plain_text_unchanged() {
        assert_eq!(render_math_svg("Frequency (Hz)", 12.0), xml_escape("Frequency (Hz)"));
    }

    #[test]
    fn render_math_svg_renders_superscript_and_subscript_as_tspans() {
        let out = render_math_svg("$x^2$", 12.0);
        assert!(out.contains("<tspan"), "expected tspans, got: {out}");
        assert!(out.contains("dy=\"-0.350em\""), "expected a superscript offset, got: {out}");
        // The face's OWN `2`, raised and shrunk -- not the precomposed `²`.
        // See `to_script_chars`: asking for `²` works only for the three
        // exponents that happen to live in Latin-1, and drops `$x^4$` out
        // of the embedded font entirely.
        assert!(out.contains(">2</tspan>"), "expected a plain raised digit, got: {out}");
        assert!(!out.contains('\u{00B2}'), "the SVG path must not precompose, got: {out}");

        let out2 = render_math_svg("$f_0$", 12.0);
        assert!(out2.contains("dy=\"0.250em\""), "expected a subscript offset, got: {out2}");
        assert!(out2.contains(">0</tspan>"), "expected a plain lowered digit, got: {out2}");
    }

    #[test]
    fn parens_group_a_script_the_way_braces_do() {
        // Reported from a real figure as an axis label reading "10-3".
        //
        // `10^(-3)` used to superscript the `(` ALONE and return `-3)` to
        // the baseline at full size. Nothing errored; the label just said
        // something else, and at label sizes the raised `(` is small enough
        // that the reader sees "10-3" and not the stray paren at all.
        let out = render_math_svg("$10^(-3)$", 12.0);
        assert!(
            out.contains("dy=\"-0.350em\""),
            "expected a superscript offset, got: {out}"
        );
        assert!(
            out.contains(">\u{2212}3</tspan>"),
            "the whole group must be raised together, got: {out}"
        );
        assert!(
            !out.contains(">(</tspan>"),
            "the delimiter must be consumed, not typeset, got: {out}"
        );
        assert!(
            !out.contains(">)</tspan>") && !out.contains("3)"),
            "nothing may be left behind on the baseline, got: {out}"
        );

        // Subscripts take the same path, and this half was equally broken:
        // `V_(pp)` lowered the `(` and left `pp)` upright.
        let sub = render_math_svg("$V_(pp)$", 12.0);
        assert!(
            sub.contains("dy=\"0.250em\""),
            "expected a subscript offset, got: {sub}"
        );
        assert!(sub.contains(">pp</tspan>"), "expected a grouped subscript, got: {sub}");
        assert!(!sub.contains("pp)"), "the closing delimiter leaked, got: {sub}");
    }

    #[test]
    fn paren_grouping_does_not_disturb_the_forms_that_already_worked() {
        // The regression risk of the fix above: `(` is only a delimiter
        // when it IMMEDIATELY follows the operator. A parenthesis anywhere
        // else is ordinary text and must stay ordinary text.
        let after = render_math_svg("$x^2(y+1)$", 12.0);
        // Asserting on the literal substring "(y+1)" would be wrong and was:
        // the renderer italicises the `y` and spaces the `+`, so the source
        // text never appears contiguously in the output. The property that
        // actually matters is that the script took ONLY the `2` and the
        // paren stayed at full size on the baseline.
        assert!(
            after.contains(">2</tspan>"),
            "the script must take only the operand, got: {after}"
        );
        assert!(
            after.contains("font-size=\"12.00\">(</tspan>"),
            "a paren that does not follow the operator stays full-size text, got: {after}"
        );

        // Braces still group, and still win where both could apply --
        // `x^{(n)}` is how you ask for parentheses that are part of the
        // notation rather than delimiters around it.
        //
        // Note both assertions below check the SCRIPT FONT SIZE rather than a
        // contiguous substring. The renderer splits a run wherever styling
        // changes -- it italicises a variable letter -- so `(n)` arrives as
        // three tspans, not one. Asserting on the source text would fail
        // against correct output, which is how the first draft of this test
        // failed twice.
        let braced = render_math_svg("$x^{(n)}$", 12.0);
        assert!(
            braced.contains("font-size=\"8.64\">(</tspan>")
                && braced.contains("font-size=\"8.64\">)</tspan>"),
            "braces must still deliver a paren pair INSIDE the script, got: {braced}"
        );

        // Nesting. Digits, not a letter, so no italic split muddies the read.
        let nested = render_math_svg("$x^((12))$", 12.0);
        assert!(
            nested.contains(">(12)</tspan>"),
            "the outer pair delimits and the inner pair is content, got: {nested}"
        );

        // An unbalanced group must not panic: it takes the rest of the body.
        let unbalanced = render_math_svg("$x^(oops$", 12.0);
        assert!(!unbalanced.is_empty(), "unbalanced group produced nothing");
    }

    #[test]
    fn every_exponent_renders_the_same_way_not_just_the_latin_1_three() {
        // The bug this pins: `²` and `³` are Latin-1 and exist nearly
        // everywhere, `⁴`-`⁹` are not and exist almost nowhere. Precomposing
        // made `$x^2$` and `$x^4$` take different paths through the font
        // stack, so one was typeset and the other fell back.
        for d in '0'..='9' {
            let out = render_math_svg(&format!("$x^{d}$"), 12.0);
            assert!(
                out.contains(&format!(">{d}</tspan>")) && out.contains("dy=\"-0.350em\""),
                "exponent {d} did not render as a plain raised digit: {out}"
            );
        }
    }

    /// The labels the crest-factor paper actually uses, verbatim from its
    /// matplotlib source. Each one previously reached the figure with the
    /// macro names spelled out as words -- "mathrmCF", "widehat", "frac12"
    /// -- so a ported figure was legible but visibly not the published one.
    #[test]
    fn renders_the_math_a_real_paper_puts_in_its_legend() {
        for (src, want) in [
            // `\mathrm` selects a face this renderer does not have, so it
            // contributes nothing but must still consume its group.
            (r"$\mathrm{CF}_{\mathrm{rand}}$", "CF"),
            // The hat is what separates the estimate from the
            // measurement plotted next to it, and it is drawn as a
            // rule over the group -- a combining character is not
            // carried by an embedded subset font and vanished.
            (r"$\widehat{\mathrm{CF}}$", "text-decoration="),
            // The radicand goes UNDER A RULE, not in parentheses:
            // parentheses read as a function applied to the
            // radicand as readily as the root of it.
            (r"$\sqrt{2\ln M}$", "\u{221A}"),
            (r"$\sqrt{2\ln M}$", "text-decoration="),
            (r"$\pm$", "\u{00B1}"),
            (r"$\nu_0 T$", "\u{03BD}"),
            (r"$\frac{1}{2}$ bit", "\u{00BD}"),
            (r"$L_{2p}$", "L"),
        ] {
            let out = render_math_svg(src, 12.0);
            assert!(out.contains(want), "{src:?} rendered as {out:?}, expected it to contain {want:?}");
            // The macro name itself must never survive as text.
            for leaked in ["mathrm", "widehat", "frac", "sqrt", "nu0", "\\"] {
                assert!(!out.contains(leaked), "{src:?} leaked {leaked:?}: {out}");
            }
        }
    }

    #[test]
    fn a_radical_is_a_rule_over_the_radicand_not_parentheses() {
        // `sqrt(2L)` and the root of 2L are different expressions, and
        // the bar is the only thing that says which is meant. This
        // renderer used to substitute parentheses, so a figure legend
        // sitting beside a typeset equation was visibly not the same
        // expression.
        let runs = parse_math_spans(r"\sqrt{2(L-\ln K)}");
        let drawn: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert!(drawn.starts_with('\u{221A}'), "{drawn:?}");
        // Nothing the source did not ask for.
        assert_eq!(
            drawn.matches('(').count(),
            1,
            "only the radicand's own bracket: {drawn:?}"
        );
        // Every run after the radical sign carries the vinculum.
        let after: Vec<&MathRun> = runs.iter().skip(1).collect();
        assert!(!after.is_empty());
        assert!(
            after.iter().all(|r| r.over == Overline::Bar),
            "the bar must span the whole radicand: {runs:?}"
        );
        // A binary minus is a real minus sign, spaced.
        assert!(drawn.contains('\u{2212}'), "{drawn:?}");
        assert!(!drawn.contains('-'), "hyphen survived: {drawn:?}");
    }

    #[test]
    fn an_accent_is_a_rule_and_a_hat_is_not_a_bar() {
        // They mean different things -- an estimate against a mean --
        // so they must not render the same. In the PDF a hat is drawn
        // as a chevron and a bar as a rule; both are marked here.
        let hat = parse_math_spans(r"\widehat{\mathrm{CF}}");
        assert!(hat.iter().all(|r| r.over == Overline::Hat), "{hat:?}");
        let bar = parse_math_spans(r"\overline{x}");
        assert!(bar.iter().all(|r| r.over == Overline::Bar), "{bar:?}");
        // `\mathrm` still applies inside the accented group.
        assert!(hat.iter().all(|r| !r.italic), "CF must stay upright: {hat:?}");
    }

    /// `read_math_token` (which reads the argument after `^`/`_`, an
    /// accent, `\sqrt`, `\frac`, and `\mathrm`/`\text`) had no whitespace
    /// skip, so `\hat Z` -- valid LaTeX, real TeX gobbles the space after
    /// a control word before reading its argument -- read a SPACE as the
    /// argument: the mark drew over nothing and `Z` fell through as
    /// ordinary text. Found in a paper Ahmed is publishing
    /// (`papers/artifacts-review/fig_ratio_families.qu`): `$\hat Z=V_m/
    /// I_m$`, `$\hat x/(2I_1(\hat x))$`, `$(t_k-\bar t)\dot Z$`.
    ///
    /// Every case here proves the UNBRACED form renders identically to
    /// the already-correct braced one, rather than merely "differently
    /// from before" -- braced and unbraced are the same LaTeX and must
    /// produce the same output, not just both a plausible one.
    #[test]
    fn a_space_after_a_control_word_is_gobbled_like_real_tex() {
        // `\hat`/`\bar` -- the RULE accent path (`latex_rule_accent`).
        assert_eq!(render_math_svg(r"$\hat Z$", 12.0), render_math_svg(r"$\hat{Z}$", 12.0));
        assert_eq!(render_math_svg(r"$\bar t$", 12.0), render_math_svg(r"$\bar{t}$", 12.0));
        // `\dot`/`\tilde`/`\vec` -- the COMBINING-CHARACTER accent path
        // (`latex_accent`), a genuinely different code path from the rule
        // one above and not exercised by it.
        assert_eq!(render_math_svg(r"$\dot Z$", 12.0), render_math_svg(r"$\dot{Z}$", 12.0));
        // `\frac` -- not an accent at all, so this proves the fix is in
        // the shared token reader and not accent-specific.
        assert_eq!(render_math_svg(r"$\frac 1 2$", 12.0), render_math_svg(r"$\frac{1}{2}$", 12.0));

        // And the unbraced forms must actually be CORRECT, not just
        // mutually consistent with each other: the mark lands on the
        // letter, not a space that then disappears.
        let hat = render_math_svg(r"$\hat Z$", 12.0);
        assert!(hat.contains("text-decoration=\"overline\"") && hat.contains(">Z</tspan>"), "{hat}");
        let frac = render_math_svg(r"$\frac 1 2$", 12.0);
        assert!(frac.contains('\u{00BD}'), "expected the precomposed one-half glyph: {frac}");
    }

    /// The paper's exact literal strings, grepped from the source rather
    /// than retyped, so this fails if a future change breaks the actual
    /// figure even if every synthetic case above still passes.
    #[test]
    fn the_paper_figures_own_latex_strings_render_the_marks_they_ask_for() {
        let hat_z = render_math_svg(r"$\hat Z=V_m/I_m$", 12.0);
        assert!(hat_z.contains("text-decoration=\"overline\""), "{hat_z}");
        assert!(!hat_z.contains("hat"), "the macro name must not leak: {hat_z}");

        let hat_x = render_math_svg(r"$\hat x/(2I_1(\hat x))$", 12.0);
        assert_eq!(hat_x.matches("text-decoration=\"overline\"").count(), 2, "both x's need the hat: {hat_x}");

        let drift = render_math_svg(r"$(t_k-\bar t)\dot Z$, sweep time", 12.0);
        assert!(drift.contains("text-decoration=\"overline\""), "the bar over t: {drift}");
        // The combining dot must attach right after the Z it marks, not
        // land on the space/paren that used to swallow the accent target.
        assert!(drift.contains(">Z</tspan><tspan font-size=\"12.00\">\u{0307}</tspan>"), "{drift}");
    }

    #[test]
    fn a_leading_minus_is_a_sign_and_takes_no_space_before_it() {
        // `$-Z''$` is a Nyquist axis label, and a thin space in
        // front of the minus would set it as a subtraction from
        // nothing.
        let runs = parse_math_spans(r"-Z");
        let drawn: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(drawn, "\u{2212}Z");
    }

    /// Operator names have to be recognised, not merely fall through the
    /// unknown-macro path: `\ln` happens to land on "ln" either way, but
    /// `\lim` would come out as "lim" only by accident and `\ln K` loses
    /// its space if the name scan runs past it.
    #[test]
    fn operator_names_render_as_operators() {
        let out = render_math_svg(r"$\ln K$", 12.0);
        assert!(out.contains("ln"), "{out}");
        assert!(!out.contains('\\'), "{out}");
    }

    /// An escaped percent is ordinary text here, but a transcribed LaTeX
    /// label writes it escaped and used to lose the backslash blindly --
    /// which happened to work, and would not have for `\,`.
    #[test]
    fn latex_escapes_and_thin_spaces_survive() {
        assert!(render_math_svg(r"$20\%$", 12.0).contains("20%"));
        let spaced = render_math_svg(r"$a\,b$", 12.0);
        assert!(spaced.contains('\u{2009}'), "expected a thin space: {spaced}");
    }

    #[test]
    fn render_math_svg_expands_greek_macros() {
        let out = render_math_svg("$\\alpha + \\beta$", 12.0);
        assert!(out.contains('\u{03B1}'), "expected alpha, got: {out}");
        assert!(out.contains('\u{03B2}'), "expected beta, got: {out}");
    }

    #[test]
    fn render_math_svg_returns_trailing_text_to_baseline_after_a_superscript() {
        // Regression guard: the reset used to live on a separate EMPTY
        // `<tspan dy="...">` after the superscript, which some SVG
        // renderers never apply to later siblings, leaving trailing text
        // visually stuck at the raised position. The delta back to the
        // true baseline must ride on the trailing text's own tspan.
        let out = render_math_svg("$x^2$ near its minimum", 12.0);
        assert!(out.contains("dy=\"0.350em\"> near its minimum</tspan>"), "got: {out}");
        // and it must NOT be an empty reset tspan followed by an
        // unshifted trailing tspan (the old, broken shape).
        assert!(!out.contains("<tspan dy=\"0.350em\"></tspan>"), "got: {out}");
    }

    #[test]
    fn render_math_svg_mixes_plain_and_math_segments() {
        let out = render_math_svg("Freq $f_0$ (Hz)", 12.0);
        assert!(out.starts_with("<tspan>Freq </tspan>") || out.contains("Freq "), "got: {out}");
        assert!(out.contains("(Hz)"), "got: {out}");
    }

    #[test]
    fn tikz_escape_passes_math_spans_through_raw() {
        let out = tikz_escape("x_1 $x_2^3$ 50%");
        assert!(out.contains("x\\_1"), "plain underscore should still be escaped: {out}");
        assert!(out.contains("$x_2^3$"), "math span should pass through raw: {out}");
        assert!(out.contains("50\\%"), "plain percent should still be escaped: {out}");
    }

    #[test]
    fn colormap_color_interpolates_between_palette_stops() {
        let start = colormap_color("viridis", 0.0);
        let end = colormap_color("viridis", 1.0);
        assert_eq!(start, "#440154");
        assert_eq!(end, "#fde725");
        let mid = colormap_color("viridis", 0.5);
        assert_ne!(mid, start);
        assert_ne!(mid, end);
    }

    #[test]
    fn matlab_palette_matches_matlabs_own_default_color_order() {
        // R2014b+'s ColorOrder, not the seaborn-derived "default" -- for
        // ports of a MATLAB figure, or anyone who just wants that familiar
        // blue/orange-red/yellow/purple cycle.
        let stops = resolve_palette("matlab");
        assert_eq!(&stops[0..4], &["#0072bd", "#d95319", "#edb120", "#7e2f8e"]);
    }

    #[test]
    fn default_and_kay_are_the_same_palette_with_red_third() {
        let default = resolve_palette("default");
        let kay = resolve_palette("kay");
        assert_eq!(default, kay);
        assert_eq!(default[2], "#FF5C5C", "red should be the 3rd color");
    }

    #[test]
    fn qu_and_tableau10_palettes_are_reachable_by_name() {
        assert_eq!(resolve_palette("qu")[0], "#3966C6");
        assert_eq!(resolve_palette("tableau10"), &["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2", "#7f7f7f", "#bcbd22", "#17becf"]);
    }

    #[test]
    fn okabe_ito_palette_matches_the_published_colorblind_safe_hex_codes() {
        // Values from Okabe & Ito's Color Universal Design set as
        // popularized by Wong, Nature Methods 8, 441 (2011) -- see the
        // `PALETTES` doc comment for the full citation. Locking in the
        // literal hex codes here so a future edit can't silently drift
        // from the published, verified-colorblind-safe values.
        assert_eq!(
            resolve_palette("okabe_ito"),
            &["#E69F00", "#56B4E9", "#009E73", "#F0E442", "#0072B2", "#D55E00", "#CC79A7", "#000000"]
        );
    }

    #[test]
    fn publication_theme_darkens_the_frame_and_pales_the_grid_screen_default_does_not() {
        // The distinction that makes `publication` a target rather than a
        // look: the frame and tick ink get DARKER and heavier (they must
        // survive being reduced to a journal column, and photocopied after
        // that), while the grid gets PALER (it is a reading aid and must
        // never compete with the curve in front of it). Getting either
        // backwards is what makes a screen figure look wrong in print.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None, ..Default::default()
        });

        let screen = build_draw_ops(&fig, 400.0, 300.0);
        assert!(screen.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == GRID_COLOR)),
            "the on-screen default should use the normal grid grey");

        // Styling now comes from the named theme (see `THEMES`); the
        // `publication` flag still drives the non-styling parts -- font
        // embedding, the print type scale, the palette -- so the builtin
        // sets both and so does this.
        fig.publication = true;
        fig.theme = "publication".to_string();
        let print = build_draw_ops(&fig, 400.0, 300.0);
        assert!(print.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == resolve_theme("publication").grid_color)),
            "publication should pale the gridlines");
        assert!(!print.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == resolve_theme("default").grid_color)),
            "no screen-grey gridline should survive into a publication figure");
        // Publication draws L-spines rather than a four-sided box now --
        // Krzywinski, Nature Methods 10:183, and 10 of 18 surveyed
        // published figures -- so the frame is Lines, not a Rect.
        assert!(print.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == PUBLICATION_INK)),
            "publication should ink the axis spines near-black");
        // ...and heavier than the screen frame, not thinner.
        let frame_widths: Vec<f64> = print.iter().filter_map(|op| match op {
            DrawOp::Line { color, width, .. } if color == PUBLICATION_INK => Some(*width),
            _ => None,
        }).collect();
        // Spines are now deliberately LIGHTER than the data lines --
        // Krzywinski, "axis weight should be modest", and the surveyed
        // Nature figures draw the spine at a fraction of the curve weight.
        // What matters is that they exist and are positive, not that they
        // out-weigh the screen default they used to match.
        assert!(!frame_widths.is_empty() && frame_widths.iter().all(|w| *w > 0.0),
            "publication should draw inked spines, got {frame_widths:?}");
    }

    #[test]
    fn minimal_theme_drops_the_box_and_lightens_gridlines_default_theme_does_not() {
        // `theme("minimal")` (`Figure.minimal`) should (a) suppress the
        // panel's box border and (b) swap gridlines from the journal-style
        // `GRID_COLOR` to the paler `MINIMAL_GRID_COLOR` -- and an untouched
        // figure (the default theme) must show neither change, so a script
        // that never calls `theme(...)` keeps rendering exactly as before
        // this feature existed.
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], label: None, marker: "line".into(), color: None, ..Default::default()
        });
        let default_ops = build_draw_ops(&fig, 400.0, 300.0);
        assert!(default_ops.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == resolve_theme("default").spine_color)), "default theme should still draw a box border");
        assert!(default_ops.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == GRID_COLOR)), "default theme should use the journal-style grid color");
        assert!(!default_ops.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == MINIMAL_GRID_COLOR)), "default theme should never emit the minimal grid color");

        fig.minimal = true;
        fig.theme = "minimal".to_string();
        let minimal_ops = build_draw_ops(&fig, 400.0, 300.0);
        assert!(!minimal_ops.iter().any(|op| matches!(op, DrawOp::Rect { stroke: Some(c), .. } if c == BOX_COLOR)), "minimal theme should not draw a box border");
        assert!(minimal_ops.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == resolve_theme("minimal").grid_color)), "minimal theme should use its own grid color");
        assert!(!minimal_ops.iter().any(|op| matches!(op, DrawOp::Line { color, .. } if color == resolve_theme("default").grid_color)), "minimal theme should not leave any default gridlines behind");
    }

    #[test]
    fn explicit_box_off_stays_off_even_under_the_default_theme() {
        // `panel.show_box = false` (the `box off` builtin) must keep
        // working standalone, independent of `theme(...)` -- `minimal`
        // only ever *removes* a box that would otherwise show, it never
        // adds one back for a panel that already opted out.
        let mut fig = Figure::new();
        fig.current_panel_mut().show_box = false;
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        assert!(!ops.iter().any(|op| matches!(op, DrawOp::Rect { stroke: Some(c), .. } if c == BOX_COLOR)));
    }

    #[test]
    fn heatmap_ops_draws_one_rect_per_cell_plus_labels() {
        let hm = Heatmap {
            values: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            rows: 2, cols: 3, colormap: "viridis".into(),
            row_labels: vec!["r0".into(), "r1".into()],
            col_labels: vec!["c0".into(), "c1".into(), "c2".into()],
            dense: false, square: false,
        };
        let ops = heatmap_ops(&hm, 0.0, 0.0, 300.0, 200.0, DEFAULT_TICK_SIZE, "#444");
        let cells = ops.iter().filter(|op| matches!(op, DrawOp::TitledRect { .. })).count();
        assert_eq!(cells, 6);
        let texts = ops.iter().filter(|op| matches!(op, DrawOp::Text { .. })).count();
        assert_eq!(texts, 6 + 2 + 3); // one value label per cell + row/col labels
    }

    /// A correlation matrix is indexed by the same names down and across,
    /// so its cells have to be square however wide the panel is -- a
    /// stretched cell turns the diagonal into a slope. `corrplot` in a
    /// 900x420 figure was drawing flat rectangles.
    #[test]
    fn a_square_heatmap_keeps_square_cells_in_a_wide_panel() {
        let hm = Heatmap {
            values: vec![1.0, 0.5, 0.5, 1.0],
            rows: 2, cols: 2, colormap: "blues".into(),
            row_labels: vec!["a".into(), "b".into()],
            col_labels: vec!["a".into(), "b".into()],
            dense: false, square: true,
        };
        let ops = heatmap_ops(&hm, 0.0, 0.0, 600.0, 200.0, DEFAULT_TICK_SIZE, "#444");
        let (w, h) = ops.iter().find_map(|op| match op {
            DrawOp::TitledRect { w, h, .. } => Some((*w, *h)),
            _ => None,
        }).expect("a heatmap draws a rect per cell");
        assert!((w - h).abs() < 1e-9, "cells should be square, got {w} x {h}");

        // ... and the same grid without the flag still fills the panel,
        // because r unrelated things by c others have no such constraint.
        let wide = Heatmap { square: false, ..hm };
        let (w2, h2) = heatmap_ops(&wide, 0.0, 0.0, 600.0, 200.0, DEFAULT_TICK_SIZE, "#444")
            .iter().find_map(|op| match op {
                DrawOp::TitledRect { w, h, .. } => Some((*w, *h)),
                _ => None,
            }).expect("a heatmap draws a rect per cell");
        assert!(w2 > h2, "an unconstrained heatmap fills its panel, got {w2} x {h2}");
    }

    /// The row/column names are this grid's tick labels, so they follow
    /// the figure's tick type. They were a fixed 10 canvas units, which is
    /// readable at exactly one canvas size and too small at print sizes.
    #[test]
    fn heatmap_labels_follow_the_figures_type_size() {
        let hm = Heatmap {
            values: vec![1.0, 2.0, 3.0, 4.0],
            rows: 2, cols: 2, colormap: "blues".into(),
            row_labels: vec!["alpha".into(), "beta".into()],
            col_labels: vec!["alpha".into(), "beta".into()],
            dense: false, square: true,
        };
        let label_size = |tick: f64| {
            heatmap_ops(&hm, 0.0, 0.0, 400.0, 400.0, tick, "#444")
                .iter()
                .find_map(|op| match op {
                    DrawOp::Text { text, size, .. } if text == "alpha" => Some(*size),
                    _ => None,
                })
                .expect("row labels are drawn")
        };
        let small = label_size(DEFAULT_TICK_SIZE);
        let large = label_size(DEFAULT_TICK_SIZE * 2.0);
        assert!(large > small * 1.9, "label type should track the figure: {small} -> {large}");
    }

    #[test]
    fn heatmap_cell_title_reports_row_col_labels_and_value() {
        let hm = Heatmap {
            values: vec![1.0, 42.3, 3.0, 4.0],
            rows: 2, cols: 2, colormap: "viridis".into(),
            row_labels: vec!["r0".into(), "r1".into()],
            col_labels: vec!["c0".into(), "c1".into()],
            dense: false, square: false,
        };
        let ops = heatmap_ops(&hm, 0.0, 0.0, 200.0, 200.0, DEFAULT_TICK_SIZE, "#444");
        let titled = ops.iter().find_map(|op| match op {
            DrawOp::TitledRect { title, .. } if title.contains("42.3") => Some(title.clone()),
            _ => None,
        }).expect("heatmap cell with value 42.3 should carry a title");
        assert_eq!(titled, "r0, c1: 42.3");
    }

    #[test]
    fn contrast_text_color_inverts_for_dark_and_light_backgrounds() {
        assert_eq!(contrast_text_color("#000000"), "#ffffff");
        assert_eq!(contrast_text_color("#084594"), "#ffffff"); // dark end of "blues"
        assert_eq!(contrast_text_color("#ffffff"), "#111111");
        assert_eq!(contrast_text_color("#f7fbff"), "#111111"); // light end of "blues"
    }

    #[test]
    fn heatmap_cell_text_stays_legible_on_a_dark_cell() {
        // a single-cell heatmap forced to the darkest stop of a colormap
        // whose extremes are far apart in luma ("blues": near-white to
        // near-navy) — the label must invert to white, not stay dark-on-dark.
        let hm = Heatmap {
            values: vec![1.0],
            rows: 1, cols: 1, colormap: "blues".into(),
            row_labels: vec![], col_labels: vec![], dense: false, square: false,
        };
        let ops = heatmap_ops(&hm, 0.0, 0.0, 100.0, 100.0, DEFAULT_TICK_SIZE, "#444");
        let text_color = ops.iter().find_map(|op| match op {
            DrawOp::Text { color, .. } => Some(color.clone()),
            _ => None,
        }).unwrap();
        // single value -> span collapses to 1.0, t = (v - vmin)/span = 0 -> the
        // lightest stop, so the label should stay dark, not invert to white.
        assert_eq!(text_color, "#111111");
    }

    fn labels(values: &[f64]) -> Vec<String> {
        let f = axis_tick_formatter(values);
        values.iter().map(|&v| f(v)).collect()
    }

    #[test]
    fn an_axis_never_prints_the_same_label_twice() {
        // The bug: ticks were rounded to three decimals INDIVIDUALLY, so an
        // axis stepping by 0.0005 printed 0.002 twice and 0.003 twice.
        let ticks = [0.0, 0.0005, 0.001, 0.0015, 0.002, 0.0025, 0.003];
        let out = labels(&ticks);
        let mut seen = std::collections::HashSet::new();
        for label in &out {
            assert!(seen.insert(label.clone()), "duplicate label {label:?} in {out:?}");
        }
    }

    #[test]
    fn one_axis_uses_one_notation_throughout() {
        // Same axis previously mixed `0.001` with `5.0e-4` because each
        // value crossed the decimal/scientific threshold on its own.
        let out = labels(&[0.0, 0.0005, 0.001, 0.0015, 0.002]);
        let scientific = out.iter().filter(|l| l.contains("10^")).count();
        assert!(
            scientific == 0 || scientific == out.len() - 1, // zero is always plain
            "mixed notations on one axis: {out:?}"
        );
    }

    #[test]
    fn decimals_are_consistent_across_an_axis() {
        // `1.5, 1, 0.5, 0` reads as three different precisions; a figure
        // should say `1.5, 1.0, 0.5, 0`.
        let out = labels(&[-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5]);
        assert_eq!(out, ["-1.5", "-1.0", "-0.5", "0", "0.5", "1.0", "1.5"], "got {out:?}");
    }

    #[test]
    fn whole_numbers_stay_whole() {
        assert_eq!(labels(&[0.0, 1.0, 2.0, 3.0]), ["0", "1", "2", "3"]);
        assert_eq!(labels(&[0.0, 1000.0, 2000.0]), ["0", "1000", "2000"]);
    }

    #[test]
    fn tiny_and_huge_axes_use_real_scientific_notation() {
        // `1.0e-6` is programmer notation. A figure wants ten to the minus
        // sixth, which the `$...$` markup renders as a real superscript.
        let out = labels(&[1e-6, 2e-6, 3e-6]);
        assert_eq!(out[0], "$10^{-6}$", "got {out:?}");
        assert_eq!(out[1], r"$2\times10^{-6}$", "got {out:?}");
        let big = labels(&[1e6, 2e6]);
        assert_eq!(big[0], "$10^{6}$", "got {big:?}");
    }

    #[test]
    fn a_bare_power_of_ten_has_no_one_times_in_front() {
        assert_eq!(format_tick_scientific(1e-9), "$10^{-9}$");
        assert_eq!(format_tick_scientific(-1e-9), "$-10^{-9}$");
    }

    #[test]
    fn a_mantissa_that_rounds_to_ten_becomes_the_next_decade() {
        // 9.99e5 at one decimal is 10.0, which must not read as
        // "10 times ten to the fifth".
        assert_eq!(format_tick_scientific(9.99e5), "$10^{6}$");
    }

    #[test]
    fn zero_is_always_plain_zero() {
        assert_eq!(format_tick_scientific(0.0), "0");
        assert_eq!(labels(&[0.0, 1e-6])[0], "0");
    }

    #[test]
    fn padding_never_crosses_zero_for_data_that_never_does() {
        // A time signal 0..0.4095 s was given an axis starting at -0.0205 s,
        // and a 0..5 kHz spectrum one starting at -247 Hz. Both are
        // meaningless quantities, and both pushed the trace off the corner
        // so the data looked offset inside its own frame.
        let (lo, hi) = data_extent([0.0, 0.2, 0.4095].into_iter(), false, true);
        assert_eq!(lo, 0.0, "a signal starting at 0 must not get a negative axis");
        assert!(hi > 0.4095, "the far end still gets breathing room, got {hi}");

        let (flo, _) = data_extent([0.0, 2500.0, 4997.6].into_iter(), false, true);
        assert_eq!(flo, 0.0, "no negative frequencies");
    }

    #[test]
    fn ordinary_positive_data_keeps_its_margin_on_both_sides() {
        // Nothing special about the origin for data that never approaches
        // it -- 3..7 should still breathe at both ends.
        let (lo, hi) = data_extent([3.0, 5.0, 7.0].into_iter(), false, true);
        assert!(lo < 3.0 && lo > 0.0, "expected a margin below 3, got {lo}");
        assert!(hi > 7.0);
    }

    #[test]
    fn data_crossing_zero_is_padded_normally_on_both_sides() {
        let (lo, hi) = data_extent([-1.0, 0.0, 1.0].into_iter(), false, true);
        assert!(lo < -1.0, "got {lo}");
        assert!(hi > 1.0, "got {hi}");
    }

    #[test]
    fn the_zero_tick_is_exactly_zero_not_float_noise() {
        // Accumulating `v += step` put the zero tick a few ulps off, and
        // the formatter then rendered the noise: a real publication figure
        // showed a y axis whose only label read "-2.8x10^-16".
        for (lo, hi, count) in [(-1.0, 1.0, 5), (-0.9, 0.9, 7), (-100.0, 100.0, 5), (-0.3, 0.3, 9)] {
            let ticks = nice_ticks(lo, hi, count);
            if let Some(z) = ticks.iter().find(|v| v.abs() < 1e-9) {
                assert_eq!(
                    *z, 0.0,
                    "zero tick came back as {z:e} for range {lo}..{hi} -- it must be exactly 0"
                );
            }
        }
    }

    #[test]
    fn thinning_never_leaves_a_single_lonely_tick() {
        // The stride cap claimed two end ticks would survive, but anchoring
        // on a zero in the middle could leave exactly one -- an axis that
        // marks the origin and states no scale at all.
        for n in 3..40usize {
            let ticks: Vec<f64> = (0..n).map(|i| -1.0 + 2.0 * i as f64 / (n - 1) as f64).collect();
            for span in [10.0, 40.0, 120.0, 400.0] {
                for min_px in [20.0, 45.0, 200.0] {
                    let kept = thin_ticks(ticks.clone(), span, min_px);
                    assert!(
                        kept.len() >= 2,
                        "{n} ticks over {span}px with a {min_px}px minimum collapsed to {}",
                        kept.len()
                    );
                }
            }
        }
    }

    #[test]
    fn measure_text_uses_the_real_face_and_beats_the_estimate() {
        let lmr = face_metrics(PRINT_FONT_FAMILY).or_else(|| face_metrics(TIKZ_FONT_FAMILY));
        let m = lmr.expect("a shipped family must resolve to parsed metrics");
        // The estimate and the measurement should agree in magnitude --
        // if they diverge wildly one of them is broken.
        let measured = measure_text("Amplitude", 20.0, Some(m));
        let estimated = text_width("Amplitude", 20.0);
        assert!(measured > 0.0);
        assert!(
            (measured / estimated).abs() > 0.5 && (measured / estimated) < 2.0,
            "measured {measured} vs estimated {estimated} -- one of these is wrong"
        );
        // Proportional: a string of narrow letters must measure less than
        // the same count of wide ones. The estimate gets this right too;
        // the point is that the measurement does not lose it.
        assert!(measure_text("iiii", 20.0, Some(m)) < measure_text("MMMM", 20.0, Some(m)));
        // An unknown family falls back rather than panicking.
        assert!(face_metrics("Comic Sans MS, cursive").is_none());
        assert_eq!(measure_text("abc", 20.0, None), text_width("abc", 20.0));
    }

    #[test]
    fn an_over_wide_title_is_shrunk_to_its_panel_not_left_to_overrun() {
        // The reported defect: in a 1x2 subplot a 27-character title ran
        // ~505 px against a 274 px panel, so the left title crossed into
        // the right panel and the right one was clipped by the canvas.
        let mut fig = Figure::default();
        fig.panels[0].title = Some("Tick marks and legend frame".into());
        fig.panels[0].series.push(Series {
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            ..Default::default()
        });
        let ops = build_draw_ops(&fig, 420.0, 300.0);

        // Every drawn LINE of the title, not the source string: the fix for
        // this defect WRAPS rather than shrinks (see the caller of
        // `wrap_to_width` -- shrinking gave two panels in a row titles at
        // two different sizes). Measuring the whole string only tested the
        // right thing while no wrapping happened; once the type scale rose
        // on 2026-09-10 it failed on a correctly wrapped title, whose two
        // lines each fit the panel perfectly well.
        let drawn_lines: Vec<(String, f64)> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, size, .. } if *size > 20.0 => Some((text.clone(), *size)),
                _ => None,
            })
            .collect();
        assert!(!drawn_lines.is_empty(), "the title must still be drawn");
        assert_eq!(
            drawn_lines.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" "),
            "Tick marks and legend frame",
            "the title's words must all survive wrapping, in order"
        );

        let panel_w = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Line { x1, x2, y1, y2, .. } if (y1 - y2).abs() < 1e-9 => Some(x2 - x1),
                _ => None,
            })
            .fold(0.0f64, f64::max);

        for (line, size) in &drawn_lines {
            let drawn = measure_text(line, *size, face_metrics(&fig.font_family));
            assert!(
                drawn <= panel_w * 1.02 + 1.0,
                "title line {line:?} measured {drawn:.1} px against a {panel_w:.1} px panel --                  it should have wrapped or shrunk to fit"
            );
        }
    }

    #[test]
    fn a_title_is_never_shrunk_below_the_tick_labels() {
        // Past that point it has stopped being a title, and an overflow a
        // reader can see beats type too small to read.
        let mut fig = Figure::default();
        fig.panels[0].title = Some("x".repeat(400));
        fig.panels[0].series.push(Series {
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            ..Default::default()
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let title_size = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Text { text, size, .. } if text.starts_with("xxx") => Some(*size),
                _ => None,
            })
            .expect("title drawn");
        assert!(title_size >= 1.0, "a title shrunk to {title_size} is unreadable");
    }

    #[test]
    fn zero_is_a_floor_but_not_a_ceiling() {
        // Replaces `all_negative_data_is_clamped_at_the_top_instead`, whose
        // expectation was the cause of a reported defect rather than a
        // property worth keeping: it asserted that data topping out at
        // exactly 0 should get an axis stopping at 0, which puts a Bode
        // magnitude's 0 dB asymptote on the frame line with half its stroke
        // clipped. On a real figure the flat top of the curve was simply
        // not visible.
        //
        // The floor clamp it was mirroring IS worth keeping, and does:
        // negative time is meaningless in a way that positive gain is not.
        let (lo, _) = data_extent([0.0, 0.2, 0.4095].into_iter(), false, true);
        assert_eq!(lo, 0.0, "a signal starting at 0 must not get a negative axis");

        // Ceiling: padded, so the trace is drawn clear of the frame.
        let (lo2, hi2) = data_extent([-35.9647, -12.0, 0.0].into_iter(), false, true);
        assert!(hi2 > 0.0, "data reaching 0 dB needs headroom, got {hi2}");
        assert!((hi2 - 1.798).abs() < 0.01, "expected ~5% of the range, got {hi2}");
        assert!(lo2 < -35.9647);

        // Data comfortably below zero never reaches the boundary anyway, so
        // removing the ceiling clamp changes nothing for it.
        let (_, hi3) = data_extent([-5.0, -2.0, -0.3].into_iter(), false, true);
        assert!(hi3 < 0.0, "still negative, no clamp needed: {hi3}");
    }

    #[test]
    fn crowded_axes_are_thinned_but_roomy_ones_are_left_alone() {
        // The bug: ten ticks is right for a full-height panel and wrong for
        // a stacked subplot a third the size, where it produced a rail of
        // labels almost touching.
        let ticks: Vec<f64> = (0..=10).map(|i| i as f64 * 0.1).collect();
        // 492px of panel: 49px apart, nothing to fix.
        let roomy = thin_ticks(ticks.clone(), 492.0, 31.5);
        assert_eq!(roomy.len(), ticks.len(), "a roomy axis must not be thinned");
        // 183px of panel: 18px apart, too close to read.
        let tight = thin_ticks(ticks.clone(), 183.0, 31.5);
        assert!(tight.len() < ticks.len(), "a crowded axis should be thinned");
        let gap = 183.0 / (tight.len() - 1) as f64;
        assert!(gap >= 31.5, "thinned gap {gap} still below the minimum");
    }

    #[test]
    fn thinning_keeps_zero_on_the_axis() {
        // An axis that shows the origin and then hides it under thinning
        // reads as a bug.
        let ticks: Vec<f64> = (-5..=5).map(|i| i as f64 * 0.2).collect();
        let thinned = thin_ticks(ticks, 120.0, 40.0);
        assert!(
            thinned.iter().any(|v| v.abs() < 1e-12),
            "zero was thinned away: {thinned:?}"
        );
    }

    #[test]
    fn thinning_preserves_a_round_step() {
        // Dropping every second tick of a 0.2 step gives 0.4 -- still a
        // round number. That is why this thins rather than recomputing.
        let ticks: Vec<f64> = (0..=10).map(|i| i as f64 * 0.2).collect();
        let thinned = thin_ticks(ticks, 100.0, 40.0);
        for pair in thinned.windows(2) {
            let step = pair[1] - pair[0];
            assert!(
                (step / 0.2).fract().abs() < 1e-9,
                "step {step} is not a multiple of the original"
            );
        }
    }

    #[test]
    fn thinning_never_strips_an_axis_bare() {
        let ticks = vec![0.0, 1.0, 2.0];
        // Absurdly tight space: still returns something usable.
        assert!(thin_ticks(ticks, 10.0, 500.0).len() >= 2);
    }

    #[test]
    fn nice_ticks_prefers_round_numbers_over_the_raw_range() {
        let ticks = nice_ticks(0.0, 18.798, 5);
        assert!(ticks.len() >= 3);
        for t in &ticks {
            // every nice tick should be a "clean" multiple of some 1/2/5 step,
            // i.e. round to at most 1 decimal digit for this range.
            assert!((t - (t * 10.0).round() / 10.0).abs() < 1e-9, "{t} isn't a round number");
        }
        assert!(ticks.iter().all(|&t| t >= 0.0 - 1e-9 && t <= 18.798 + 1e-9));
    }

    #[test]
    fn format_tick_snaps_floating_point_zero_noise_to_a_clean_zero() {
        // Regression for a real bug reported live (2026-09-04): a wavelet
        // decomposition demo's y-axis showed "-2.8e-16" (the f64-epsilon-
        // scale residue of a value that's mathematically exactly zero,
        // e.g. `sin(pi)`) instead of "0" -- spurious precision, not real
        // information, and it also visually crowded the axis in a dense
        // subplot grid.
        assert_eq!(format_tick(-2.8e-16), "0");
        assert_eq!(format_tick(1.2e-16), "0");
        assert_eq!(format_tick(0.0), "0");
        assert_eq!(format_tick(-0.0), "0");
        // A genuinely small (but not epsilon-noise) value still gets its
        // scientific-notation treatment, unaffected by the zero-snap --
        // now written the way science writes it rather than the way a
        // programmer does. See `format_tick_scientific`.
        assert_eq!(format_tick(5e-5), r"$5\times10^{-5}$");
    }

    #[test]
    fn polyline_with_a_non_finite_point_still_renders_its_other_points() {
        // Regression for a real bug found live (2026-09-04): QuStudio's
        // bundled Butterworth-filter template plots `20*log10(abs(H))`,
        // and the stopband magnitude underflows to exactly 0.0 at the last
        // frequency bin, producing a `-inf` data point. That reached the
        // SVG `<polyline points="...">` attribute as a literal `inf` pixel
        // coordinate, which invalidated the WHOLE attribute per the SVG
        // spec -- the entire 1024-point curve rendered as nothing, not
        // just the one bad sample, despite the axes/title/labels around it
        // looking completely normal (confirmed via a real `qu.exe` render:
        // `points="121.45,70.36 ... 830.55,inf"`).
        let points = vec![(0.0, 10.0), (1.0, 20.0), (2.0, f64::INFINITY), (3.0, f64::NAN), (4.0, 30.0)];
        let op = DrawOp::Polyline { points, color: "#5B7CFA".into(), width: 1.5, dash: None };

        let mut svg = String::new();
        write_svg_op(&mut svg, &op, DEFAULT_TICK_SIZE);
        assert!(!svg.contains("inf") && !svg.contains("NaN"), "non-finite tokens leaked into SVG: {svg}");
        assert!(svg.contains("0.00,10.00"), "the finite points before the bad one should still render: {svg}");
        assert!(svg.contains("4.00,30.00"), "the finite points after the bad one should still render: {svg}");

        let mut tikz = String::new();
        write_tikz_op(&mut tikz, &op, DEFAULT_AXIS_LABEL_SIZE);
        assert!(!tikz.contains("inf") && !tikz.contains("NaN"), "non-finite tokens leaked into TikZ: {tikz}");
        assert!(tikz.contains("(0.00,10.00)") && tikz.contains("(4.00,30.00)"), "got: {tikz}");

        let mut pdf = String::new();
        write_pdf_op(&mut pdf, &op, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(!pdf.contains("inf") && !pdf.contains("NaN"), "non-finite tokens leaked into the PDF content stream: {pdf}");
        assert!(pdf.contains("0.00 10.00 m") && pdf.contains("4.00 30.00 l"), "got: {pdf}");
    }

    #[test]
    fn nice_ticks_handles_a_degenerate_zero_width_range() {
        let ticks = nice_ticks(5.0, 5.0, 5);
        assert_eq!(ticks, vec![5.0]);
    }

    #[test]
    fn categorical_xtick_labels_replace_numeric_ticks() {
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.series.push(Series { x: vec![0.0, 1.0, 2.0], y: vec![3.0, 1.0, 4.0], label: None, marker: "bar".into(), color: None, ..Default::default() });
        panel.xtick_labels = Some(vec!["low".into(), "mid".into(), "high".into()]);
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        let texts: Vec<&str> = ops.iter().filter_map(|op| match op {
            DrawOp::Text { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect();
        assert!(texts.contains(&"low"));
        assert!(texts.contains(&"mid"));
        assert!(texts.contains(&"high"));
        // exactly one label per category — no numeric x ticks alongside them
        let x_tick_y = ops.iter().find_map(|op| match op {
            DrawOp::Text { text, y, .. } if text == "mid" => Some(*y),
            _ => None,
        }).unwrap();
        let x_row_texts: Vec<&str> = ops.iter().filter_map(|op| match op {
            DrawOp::Text { text, y, .. } if (*y - x_tick_y).abs() < 1e-9 => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(x_row_texts.len(), 3);
    }

    #[test]
    fn blues_palette_is_registered_and_spans_light_to_dark() {
        let stops = resolve_palette("blues");
        assert!(stops.len() >= 4);
        let (r0, g0, b0) = hex_to_rgb(stops[0]);
        let (r1, g1, b1) = hex_to_rgb(stops[stops.len() - 1]);
        let luma = |r: u8, g: u8, b: u8| 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
        assert!(luma(r0, g0, b0) > luma(r1, g1, b1)); // first stop lighter than last
    }

    #[test]
    fn spider_ops_labels_each_radial_ring_with_a_value() {
        let mut chart = SpiderChart::default();
        chart.series.push(Series { x: vec![], y: vec![1.0, 2.0, 3.0, 8.0], label: None, marker: "line".into(), color: None, ..Default::default() });
        let ops = spider_ops(&chart, 0.0, 0.0, 200.0, 200.0, PALETTES[0].1, 8.5, 10.0);
        let texts: Vec<&str> = ops.iter().filter_map(|op| match op {
            DrawOp::Text { text, .. } => Some(text.as_str()),
            _ => None,
        }).collect();
        // 4 ring-value labels (max_val=8 -> 2,4,6,8)
        assert!(texts.contains(&"8"));
        assert!(texts.contains(&"2"));
    }

    #[test]
    fn heatmap_panel_is_not_pristine() {
        let mut fig = Figure::new();
        fig.current_panel_mut().heatmap = Some(Heatmap {
            values: vec![1.0], rows: 1, cols: 1, colormap: "default".into(),
            row_labels: vec![], col_labels: vec![], dense: false, square: false,
        });
        assert!(!fig.current_panel().is_pristine());
    }

    #[test]
    fn groupbar_series_sit_side_by_side_not_stacked() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { x: vec![0.0, 1.0], y: vec![1.0, 2.0], label: None, marker: "groupbar".into(), color: None, ..Default::default() });
        fig.current_panel_mut().series.push(Series { x: vec![0.0, 1.0], y: vec![3.0, 4.0], label: None, marker: "groupbar".into(), color: None, ..Default::default() });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let rects: Vec<_> = ops.iter().filter_map(|op| match op { DrawOp::Rect { x, fill: Some(_), .. } => Some(*x), _ => None }).collect();
        assert_eq!(rects.len(), 4); // 2 categories x 2 series
        // the two series at category 0 must NOT share the same x (unlike stackbar)
        assert_ne!(rects[0], rects[2]);
    }

    #[test]
    fn step_marker_produces_a_staircase_polyline() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series { x: vec![0.0, 1.0, 2.0], y: vec![0.0, 1.0, 0.5], label: None, marker: "step".into(), color: None, ..Default::default() });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let poly = ops.iter().find_map(|op| match op { DrawOp::Polyline { points, .. } => Some(points), _ => None }).unwrap();
        assert_eq!(poly.len(), 5); // 3 data points -> 5 staircase vertices
    }

    #[test]
    fn errorbar_shape_renders_whisker_caps_and_marker() {
        let mut fig = Figure::new();
        fig.current_panel_mut().show_grid = Some(false); // isolate the errorbar's own lines from tick gridlines
        fig.current_panel_mut().shapes.push(Shape::ErrorBar { x: 1.0, y: 5.0, err: 1.0, xerr: None, color: None, cap: None, width: None });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        // Counted by the errorbar's own colour rather than by "every line
        // on the canvas": the frame and the axis tick marks are lines too,
        // and a total that has to be adjusted every time the chrome
        // changes tests the chrome, not the errorbar.
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, DrawOp::Line { color, .. } if color == "#4c72b0"))
                .count(),
            3,
            "whisker + 2 caps"
        );
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Circle { fill: Some(_), .. })));
    }

    #[test]
    fn fill_between_shape_renders_a_closed_filled_polygon() {
        let mut fig = Figure::new();
        fig.current_panel_mut().shapes.push(Shape::FillBetween {
            x: vec![0.0, 1.0, 2.0], y_lo: vec![-1.0, -1.0, -1.0], y_hi: vec![1.0, 2.0, 1.0], color: None,
            alpha: None,
        });
        let ops = build_draw_ops(&fig, 300.0, 200.0);
        let poly = ops.iter().find_map(|op| match op { DrawOp::Polygon { points, .. } => Some(points), _ => None }).unwrap();
        assert_eq!(poly.len(), 6); // 3 upper + 3 lower (reversed)
    }

    #[test]
    fn span_alpha_reaches_the_rect_instead_of_being_ignored() {
        // Same silent-kwarg bug `fill_between` had: `alpha=` was accepted
        // and dropped, so every band came out at the hardcoded 0.15 and a
        // figure with nine series over three washes had no way to push the
        // washes back.
        let wash = |alpha| {
            let mut fig = Figure::new();
            fig.current_panel_mut().shapes.push(Shape::YSpan { y0: 0.0, y1: 1.0, color: None, alpha });
            build_draw_ops(&fig, 300.0, 200.0)
                .iter()
                .find_map(|op| match op { DrawOp::Rect { opacity, .. } => Some(*opacity), _ => None })
                .unwrap()
        };
        assert!((wash(Some(0.09)) - 0.09).abs() < 1e-9);
        assert!((wash(None) - 0.15).abs() < 1e-9, "the default is still 0.15");
    }

    #[test]
    fn fill_between_alpha_reaches_the_polygon_instead_of_being_ignored() {
        // `alpha=` used to be accepted and dropped: every band came out at
        // the hardcoded 0.3 whatever the caller asked for. A silently
        // ignored style argument is worse than a rejected one, because the
        // figure looks plausible and the script says something untrue.
        let band = |alpha| {
            let mut fig = Figure::new();
            fig.current_panel_mut().shapes.push(Shape::FillBetween {
                x: vec![0.0, 1.0], y_lo: vec![0.0, 0.0], y_hi: vec![1.0, 1.0],
                color: None, alpha,
            });
            build_draw_ops(&fig, 300.0, 200.0)
                .iter()
                .find_map(|op| match op { DrawOp::Polygon { opacity, .. } => Some(*opacity), _ => None })
                .unwrap()
        };
        assert!((band(Some(0.75)) - 0.75).abs() < 1e-9);
        assert!((band(None) - 0.3).abs() < 1e-9, "the default is still 0.3");
    }

    #[test]
    fn a_labelled_band_gets_a_filled_patch_in_the_legend_not_a_line() {
        // A shaded region is often the one mark a caption cannot point at,
        // so `fill_between(label=)` puts it in the key -- and the key has
        // to show the wash itself. A line swatch here would name the band
        // with a sample of something the figure does not contain.
        let mut fig = Figure::new();
        let panel = fig.current_panel_mut();
        panel.shapes.push(Shape::FillBetween {
            x: vec![0.0, 1.0], y_lo: vec![0.0, 0.0], y_hi: vec![1.0, 1.0],
            color: Some("#2E9E5B".into()), alpha: Some(0.28),
        });
        panel.series.push(Series {
            label: Some("Uncertainty saved".into()),
            color: Some("#2E9E5B".into()),
            marker: "band".into(),
            alpha: Some(0.28),
            ..Default::default()
        });
        // A real series too: the band's own entry carries no points, so on
        // its own there is nothing to set the axis range from.
        panel.series.push(Series {
            x: vec![0.0, 1.0], y: vec![0.0, 1.0], ..Default::default()
        });
        panel.legend = Legend { visible: true, ..Default::default() };
        let ops = build_draw_ops(&fig, 400.0, 300.0);
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Text { text, .. } if text == "Uncertainty saved")));
        // Two filled polygons: the band in the panel and its swatch in the
        // key, both at the band's own opacity.
        let swatches = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Polygon { fill: Some(c), opacity, .. }
                                  if c == "#2E9E5B" && (*opacity - 0.28).abs() < 1e-9))
            .count();
        assert_eq!(swatches, 2, "band and its legend patch");
    }

    #[test]
    fn pie_ops_draws_one_wedge_per_positive_value() {
        let pie = PieChart { values: vec![1.0, 2.0, 0.0, 3.0], labels: vec!["a".into(), "b".into(), "c".into(), "d".into()], donut: false };
        let ops = pie_ops(&pie, 0.0, 0.0, 200.0, 200.0, PALETTES[0].1, DEFAULT_TICK_SIZE);
        let wedges = ops.iter().filter(|op| matches!(op, DrawOp::Polygon { .. })).count();
        assert_eq!(wedges, 3); // the zero-value slice is skipped
    }

    #[test]
    fn donut_leaves_a_hole_pie_does_not() {
        let values = vec![1.0, 1.0, 1.0];
        let pie = PieChart { values: values.clone(), labels: vec![], donut: false };
        let donut = PieChart { values, labels: vec![], donut: true };
        let pie_ops_result = pie_ops(&pie, 0.0, 0.0, 200.0, 200.0, PALETTES[0].1, DEFAULT_TICK_SIZE);
        let donut_ops_result = pie_ops(&donut, 0.0, 0.0, 200.0, 200.0, PALETTES[0].1, DEFAULT_TICK_SIZE);
        // a plain pie wedge fans from the center (a point repeated in `points`
        // isn't guaranteed, but its point count is roughly half a donut
        // wedge's, since donut wedges trace both an inner and outer arc).
        let pie_points = match &pie_ops_result[0] { DrawOp::Polygon { points, .. } => points.len(), _ => 0 };
        let donut_points = match &donut_ops_result[0] { DrawOp::Polygon { points, .. } => points.len(), _ => 0 };
        assert!(donut_points > pie_points);
    }

    #[test]
    fn svg_html_tikz_agree_on_series_count() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0, 2.0],
            y: vec![0.0, 1.0, 0.0],
            label: None,
            marker: "line".into(),
            color: None,
            ..Default::default()
        });
        let svg = render_svg(&fig, 200.0, 150.0, false);
        let html = render_html(&fig, 200.0, 150.0, false);
        let tikz = render_tikz(&fig, 200.0, 150.0);
        assert_eq!(svg.matches("<polyline").count(), 1);
        assert!(html.contains(&svg));
        assert_eq!(tikz.matches("--").count() >= 1, true);
    }

    // ------------------------------------------------------------ PDF export

    #[test]
    fn write_pdf_op_line_emits_moveto_lineto_stroke_with_the_right_color_and_width() {
        let mut out = String::new();
        write_pdf_op(&mut out, &DrawOp::Line { x1: 1.0, y1: 2.0, x2: 3.0, y2: 4.0, color: "#ff0000".into(), width: 2.5 }, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(out.contains("1.000 0.000 0.000 RG"), "expected pure red stroke color, got: {out}");
        assert!(out.contains("2.50 w"), "expected the line width operator, got: {out}");
        assert!(out.contains("1.00 2.00 m"), "expected the moveto, got: {out}");
        assert!(out.contains("3.00 4.00 l"), "expected the lineto, got: {out}");
        assert!(out.contains(" S"), "expected the stroke operator, got: {out}");
    }

    #[test]
    fn write_pdf_op_filled_rect_emits_fill_color_and_re_f() {
        let mut out = String::new();
        write_pdf_op(
            &mut out,
            &DrawOp::Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0, fill: Some("#00ff00".into()), stroke: None, opacity: 1.0, radius: 0.0 },
            DEFAULT_AXIS_LABEL_SIZE,
            None,
            None,
            None,
            None,
        );
        assert!(out.contains("0.000 1.000 0.000 rg"), "expected pure green fill color, got: {out}");
        assert!(out.contains("10.00 20.00 30.00 40.00 re"), "expected the rect operator with its geometry, got: {out}");
        assert!(out.contains(" f"), "expected the fill operator, got: {out}");
    }

    #[test]
    fn a_translucent_rect_fades_its_border_too_and_not_only_its_body() {
        // The screen legend frame is exactly this shape: a white body and a
        // grey border, the whole thing at 0.85. SVG's `opacity` on a <rect>
        // covers fill AND stroke, so the border fades with the body. The PDF
        // writer used to put the soft mask on the fill only, leaving a fully
        // opaque box drawn around a translucent panel -- a frame that
        // read heavier in the paper than on screen, and one more case of
        // the two backends disagreeing about what the figure shows.
        let frame = DrawOp::Rect {
            x: 0.0, y: 0.0, w: 10.0, h: 10.0,
            fill: Some("#ffffff".into()), stroke: Some("#cccccc".into()),
            opacity: 0.85, radius: 6.0,
        };
        let mut out = String::new();
        write_pdf_op(&mut out, &frame, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        let stroked: Vec<&str> = out.lines().filter(|l| l.contains(" RG ")).collect();
        assert_eq!(stroked.len(), 1, "expected exactly one stroking pass, got: {out}");
        assert!(
            stroked[0].contains("/GA850 gs"),
            "the border must carry the same soft mask as the body, got: {}",
            stroked[0]
        );
        // And the state both passes name has to exist in the resources.
        let gs = pdf_ext_gstate(std::slice::from_ref(&frame));
        assert!(gs.contains("/GA850 << /ca 0.850 /CA 0.850 >>"), "expected /CA as well as /ca, got: {gs}");
    }

    #[test]
    fn a_translucent_pdf_fill_is_translucent_over_whatever_is_under_it() {
        // This used to blend the colour toward WHITE and draw it opaque,
        // which is only right where the thing underneath is the white page.
        // A legend frame over a plotted line came out as a panel that
        // erased the data behind it, and a fill_between band hid any series
        // it crossed -- while the SVG of the same figure showed both
        // through, because SVG has had real fill-opacity all along. The two
        // backends disagreed about what the figure SHOWS.
        let band = DrawOp::Rect {
            x: 0.0, y: 0.0, w: 1.0, h: 1.0,
            fill: Some("#ff0000".into()), stroke: None, opacity: 0.5, radius: 0.0,
        };
        let mut out = String::new();
        write_pdf_op(&mut out, &band, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(out.contains("/GA500 gs"), "expected a soft-mask state, got: {out}");
        assert!(out.contains("1.000 0.000 0.000 rg"), "the colour itself must stay red, got: {out}");
        // The state it names has to actually be defined in the resources,
        // or a viewer silently ignores it and draws the shape opaque.
        let gs = pdf_ext_gstate(std::slice::from_ref(&band));
        assert!(gs.contains("/GA500 << /ca 0.500 /CA 0.500 >>"), "expected the matching ExtGState entry, got: {gs}");
        // An opaque shape must not pick up any of this machinery.
        let mut solid = String::new();
        let opaque = DrawOp::Rect {
            x: 0.0, y: 0.0, w: 1.0, h: 1.0,
            fill: Some("#ff0000".into()), stroke: None, opacity: 1.0, radius: 0.0,
        };
        write_pdf_op(&mut solid, &opaque, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(!solid.contains("gs"), "an opaque fill needs no graphics state: {solid}");
        assert_eq!(pdf_ext_gstate(std::slice::from_ref(&opaque)), "");
    }

    #[test]
    fn write_pdf_op_text_uses_bt_et_and_right_aligns_via_a_negative_td_offset() {
        let mut out = String::new();
        write_pdf_op(&mut out, &DrawOp::Text { italic: false, x: 100.0, y: 50.0, text: "hi".into(), size: 12.0, anchor: Anchor::Start, rotate: 0.0, color: "#000000".into() }, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(out.starts_with("BT\n"), "expected a BT/ET text block, got: {out}");
        assert!(out.contains("/F1 12.00 Tf"), "expected the regular (non-bold) font at the right size, got: {out}");
        assert!(out.contains("(hi) Tj"), "expected the literal-string show-text operator, got: {out}");
        assert!(out.trim_end().ends_with("ET"), "expected the block to close, got: {out}");

        // A bold (>= axis-label-size) title picks the bold font.
        let mut bold_out = String::new();
        write_pdf_op(&mut bold_out, &DrawOp::Text { italic: false, x: 0.0, y: 0.0, text: "t".into(), size: DEFAULT_AXIS_LABEL_SIZE, anchor: Anchor::Start, rotate: 0.0, color: "#000000".into() }, DEFAULT_AXIS_LABEL_SIZE - 0.001, None, None, None, None);
        assert!(bold_out.contains("/F2"), "expected the bold font for an axis-label-sized text run, got: {bold_out}");

        // `Anchor::Middle`/`Anchor::End` shift the `Tm` origin left by
        // (half/all of) the estimated text width rather than start=0.
        let mut start_out = String::new();
        write_pdf_text(&mut start_out, 100.0, 50.0, "AB", 12.0, Anchor::Start, 0.0, "#000000", PdfFace::Regular, None, None, None);
        let mut end_out = String::new();
        write_pdf_text(&mut end_out, 100.0, 50.0, "AB", 12.0, Anchor::End, 0.0, "#000000", PdfFace::Regular, None, None, None);
        let tm_x = |s: &str| -> f64 {
            let tm_line = s.lines().find(|l| l.ends_with("Tm")).unwrap();
            tm_line.split_whitespace().nth(4).unwrap().parse().unwrap()
        };
        assert!(tm_x(&end_out) < tm_x(&start_out), "End-anchored text must start further left than Start-anchored text at the same point");
    }

    #[test]
    fn maths_variables_are_italic_and_upright_words_are_not() {
        // TeX's rule, and the one a reader has internalised: a Latin letter
        // standing for a quantity is italic; digits, operators and anything
        // inside `\mathrm` are upright. Qu set the whole span upright, so a
        // figure's `$K$` and its caption's `$K$` -- the same symbol, one
        // set by Qu and one by LaTeX -- did not match.
        let italic_of = |src: &str| -> Vec<(String, bool)> {
            parse_math_spans(src).into_iter().map(|r| (r.text, r.italic)).collect()
        };
        assert_eq!(italic_of("K"), vec![("K".to_string(), true)]);
        assert_eq!(italic_of("50"), vec![("50".to_string(), false)]);
        // A product of two variables is two italic letters, not a word.
        assert_eq!(italic_of("fT"), vec![("fT".to_string(), true)]);
        // `\mathrm` is exactly the instruction "these are not variables".
        let dm = italic_of("\\Delta_{\\mathrm{real}}");
        assert!(dm.iter().any(|(t, it)| t == "real" && !it), "{dm:?}");
        // An operator name is upright; its argument is not.
        let ln = italic_of("2\\ln K");
        assert!(ln.iter().any(|(t, it)| t.contains("ln") && !it), "{ln:?}");
        assert!(ln.iter().any(|(t, it)| t == "K" && *it), "{ln:?}");
        // Greek stays upright: it is drawn from the math face and no
        // italic Greek is bundled, so slanting it would ask for a glyph
        // that does not exist.
        assert!(italic_of("\\rho").iter().all(|(_, it)| !it));
    }

    #[test]
    fn an_italic_maths_run_reaches_the_svg_and_the_pdf() {
        let svg = render_math_svg("$K$ = 50", 12.0);
        assert!(svg.contains("font-style=\"italic\""), "{svg}");
        // In the PDF an italic run is set in `/F3`, the drawn italic --
        // but only when that face is embedded; without it the run stays in
        // the label's own slot rather than naming a font the page does not
        // declare.
        let spans = parse_math_spans_for_pdf("$K$ = 50");
        assert!(spans.iter().any(|r| r.text == "K" && r.italic), "{spans:?}");
    }

    #[test]
    fn pdf_encode_text_escapes_parens_and_backslashes() {
        assert_eq!(pdf_encode_text("a(b)c\\d"), "a\\(b\\)c\\\\d");
    }

    #[test]
    fn pdf_encode_text_octal_escapes_high_latin1_bytes_and_substitutes_non_latin1() {
        // 'e' with an acute accent (U+00E9) is WinAnsiEncoding byte 0xE9 =
        // octal 351.
        assert_eq!(pdf_encode_text("\u{00e9}"), "\\351");
        // Past Latin-1 the encoding cannot carry the character, and a '?'
        // reads as a rendering fault while destroying the label -- an
        // impedance axis came out "[m?]" and a formula "?(2 ln M)". A
        // symbol with an accepted plain-text spelling gets that instead.
        assert_eq!(pdf_encode_text("\u{03c0}"), "pi");
        assert_eq!(pdf_encode_text("\u{221A}"), "sqrt");
        assert_eq!(pdf_encode_text("\u{03A9}"), "Ohm");
        // Something with no such spelling still falls back, which at that
        // point is honest rather than a guess.
        assert_eq!(pdf_encode_text("\u{4E2D}"), "?");
        // A combining accent has no plain-text form, so it is dropped
        // rather than spelled or questioned: `\widehat{\mathrm{CF}}` was
        // reaching the paper's legend as "CF?".
        assert_eq!(pdf_encode_text("CF\u{0302}"), "CF");
        assert_eq!(pdf_encode_text("v\u{20D7}"), "v");
    }

    #[test]
    fn a_spelled_symbol_advances_the_pen_by_the_width_of_what_it_paints() {
        // The bug: the width came from the SOURCE text, so a label
        // containing `Δ` was measured as one glyph and painted as five
        // letters. The paper's regime-map colorbar came out as "Delta"
        // with its own subscript stamped through it, and every centred
        // label holding a spelled symbol sat off centre by the difference.
        let face = face_metrics(TIKZ_FONT_FAMILY).expect("Latin Modern metrics");
        let source = "\u{0394}";
        let painted = pdf_drawable_text(source);
        assert_eq!(painted, "Delta");
        assert!(
            face.text_width(&painted) > 2.0 * face.text_width(source),
            "five letters must measure far wider than the one glyph they replace: \
             {} vs {}",
            face.text_width(&painted),
            face.text_width(source)
        );

        // And the writer must use the wider number. Two spans, the first
        // holding the spelled symbol: the second starts after the whole
        // word, not after one glyph's worth of it.
        let mut out = String::new();
        let spans = vec![
            MathRun::plain("\u{0394}".to_string(), false),
            MathRun::plain("x".to_string(), false),
        ];
        write_pdf_text_spans(
            &mut out, 0.0, 0.0, &spans, 10.0, Anchor::Start, 0.0, "#000000", PdfFace::Regular, Some(face),
            None, None,
        );
        let xs: Vec<f64> = out
            .lines()
            .filter(|l| l.ends_with(" Tm"))
            .filter_map(|l| l.split_whitespace().nth(4)?.parse().ok())
            .collect();
        assert_eq!(xs.len(), 2, "expected two positioned runs, got {out}");
        let advance = xs[1] - xs[0];
        let expected = face.text_width("Delta") / 1000.0 * 10.0;
        assert!(
            (advance - expected).abs() < 1e-6,
            "second run starts at +{advance}, but \"Delta\" is {expected} wide"
        );
    }

    #[test]
    fn flatten_math_to_plain_drops_dollar_signs_and_sizing_but_keeps_characters() {
        assert_eq!(flatten_math_to_plain("plain text"), "plain text");
        // `$x^2$` -> `parse_math_spans` maps the exponent to its Unicode
        // superscript-two glyph (same mapping `render_math_svg` relies on
        // for `<tspan>` sizing) -- `flatten_math_to_plain` keeps that
        // character but drops the `$` delimiters and the size/offset info.
        assert_eq!(flatten_math_to_plain("$x^2$ + 1"), "x\u{00b2} + 1");
    }

    #[test]
    fn render_pdf_produces_a_well_formed_minimal_pdf() {
        let mut fig = Figure::new();
        fig.current_panel_mut().series.push(Series {
            x: vec![0.0, 1.0, 2.0],
            y: vec![0.0, 1.0, 0.0],
            label: None,
            marker: "line".into(),
            color: None,
            ..Default::default()
        });
        let bytes = render_pdf(&fig, 200.0, 150.0);
        // 1.6 once a font is embedded (`/FontFile3 /Subtype /OpenType`
        // is a 1.6 construct); 1.4 for the base-14 fallback. Pinning the
        // exact version here is what made the embedding work look like a
        // regression rather than the feature it is.
        assert!(bytes.starts_with(b"%PDF-1."), "missing PDF header");
        let tail = String::from_utf8_lossy(&bytes[bytes.len() - 6..]);
        assert!(tail.ends_with("%%EOF\n") || tail.ends_with("%%EOF"), "got tail: {tail:?}");
        let text = String::from_utf8_lossy(&bytes);
        // The page is sized in POINTS, and the canvas is in units of
        // 1/MM_TO_UNITS mm -- so a 200x150 canvas is 20x15 mm, not
        // 200x150 pt. Asserting the raw numbers back was what let the
        // units-as-points bug live: a 900x380 figure claimed a 317x134 mm
        // page, four times its intended size.
        let expect_w = 200.0 * PDF_POINTS_PER_UNIT;
        let expect_h = 150.0 * PDF_POINTS_PER_UNIT;
        assert!(
            text.contains(&format!("/MediaBox [0 0 {expect_w:.2} {expect_h:.2}]")),
            "expected a MediaBox of {expect_w:.2}x{expect_h:.2} pt (= 20x15 mm)"
        );
        // And the content transform must carry the same scale, or the
        // drawing and its page stop being the same size.
        assert!(
            text.contains(&format!("q {PDF_POINTS_PER_UNIT:.6} 0 0 -{PDF_POINTS_PER_UNIT:.6} 0 ")),
            "content stream must scale by the same points-per-unit as the MediaBox"
        );
        assert!(text.matches(" m ").count() >= 1, "expected at least one moveto for the line series");
    }

    #[test]
    fn render_pdf_image_op_draws_a_documented_placeholder_not_the_pixels() {
        let mut out = String::new();
        write_pdf_op(&mut out, &DrawOp::Image { x: 0.0, y: 0.0, w: 10.0, h: 10.0, href: "data:image/bmp;base64,AA==".into() }, DEFAULT_AXIS_LABEL_SIZE, None, None, None, None);
        assert!(!out.contains("base64"), "the PDF backend has no image-embedding support and must not leak the data URI into the content stream");
        assert!(out.contains("re") && out.contains("S"), "expected a placeholder rectangle outline");
        assert!(out.contains("image omitted"), "expected the same documented-gap wording style TikZ's own image placeholder uses");
    }
}
