//! Contour lines and filled contour bands from a scalar field on a grid.
//!
//! Both are computed by splitting each grid cell into four triangles around
//! its centre and treating the field as linear on each one. That choice is
//! worth stating, because the obvious alternative -- classifying whole
//! cells, the textbook "marching squares" -- has two problems this does
//! not: it needs an explicit saddle-point rule to decide which way an
//! ambiguous cell connects, and splitting a cell along one diagonal
//! instead biases every feature towards that diagonal, which is visible as
//! a staircase on a coarse grid. Four triangles about the centre is
//! symmetric, has no ambiguous case, and is what matplotlib's own contour
//! code does.
//!
//! On a linear triangle both operations are exact, not approximated:
//!
//!   * a **line** at level `t` is where the plane `z = t` cuts the
//!     triangle -- a single straight segment, found by interpolating along
//!     whichever two edges change sign;
//!   * a **band** between `lo` and `hi` is the triangle clipped against
//!     two half-spaces, which Sutherland-Hodgman does in one pass each and
//!     which needs no case analysis at all.
//!
//! `NaN` marks "no data" (the caller's mask, e.g. a region where a formula
//! is not valid). Any cell with a `NaN` corner is dropped whole, matching
//! what matplotlib does with a masked array -- interpolating towards a
//! `NaN` would invent a boundary that is not in the data.

/// A scalar field sampled on a rectilinear grid.
///
/// `z` is row-major: `z[j * x.len() + i]` is the value at `(x[i], y[j])`.
/// That is the layout `meshgrid` produces and the one every caller here
/// builds, so it is the layout this takes rather than converting.
pub struct Grid<'a> {
    pub x: &'a [f64],
    pub y: &'a [f64],
    pub z: &'a [f64],
}

/// A vertex carrying its field value, so clipping can interpolate.
type P = (f64, f64, f64);

impl Grid<'_> {
    pub fn is_valid(&self) -> bool {
        self.x.len() >= 2 && self.y.len() >= 2 && self.z.len() == self.x.len() * self.y.len()
    }

    fn at(&self, i: usize, j: usize) -> P {
        (self.x[i], self.y[j], self.z[j * self.x.len() + i])
    }

    /// The four centre-split triangles of every cell, skipping any cell
    /// with a `NaN` corner.
    fn triangles(&self) -> Vec<[P; 3]> {
        let (nx, ny) = (self.x.len(), self.y.len());
        let mut out = Vec::with_capacity((nx - 1) * (ny - 1) * 4);
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let c = [
                    self.at(i, j),
                    self.at(i + 1, j),
                    self.at(i + 1, j + 1),
                    self.at(i, j + 1),
                ];
                if c.iter().any(|p| !p.2.is_finite()) {
                    continue;
                }
                let mid = (
                    (c[0].0 + c[1].0 + c[2].0 + c[3].0) / 4.0,
                    (c[0].1 + c[1].1 + c[2].1 + c[3].1) / 4.0,
                    (c[0].2 + c[1].2 + c[2].2 + c[3].2) / 4.0,
                );
                for k in 0..4 {
                    out.push([c[k], c[(k + 1) % 4], mid]);
                }
            }
        }
        out
    }
}

/// Linear interpolation along an edge to where the field equals `t`.
fn cross(a: P, b: P, t: f64) -> (f64, f64) {
    let d = b.2 - a.2;
    // A zero denominator means both ends sit exactly on the level; either
    // endpoint is then equally correct, and 0.5 avoids a division by zero
    // producing an infinity that would escape into the polygon.
    let s = if d.abs() < f64::EPSILON { 0.5 } else { (t - a.2) / d };
    (a.0 + s * (b.0 - a.0), a.1 + s * (b.1 - a.1))
}

/// Clip a convex polygon to the half-space `z >= t` (or `z <= t`).
///
/// Sutherland-Hodgman: walk the edges, keep every vertex on the wanted
/// side, and insert the crossing point wherever an edge changes side.
fn clip(poly: &[P], t: f64, keep_above: bool) -> Vec<P> {
    if poly.is_empty() {
        return Vec::new();
    }
    let inside = |p: &P| if keep_above { p.2 >= t } else { p.2 <= t };
    let mut out: Vec<P> = Vec::with_capacity(poly.len() + 2);
    for k in 0..poly.len() {
        let a = poly[k];
        let b = poly[(k + 1) % poly.len()];
        let (ai, bi) = (inside(&a), inside(&b));
        if ai {
            out.push(a);
        }
        if ai != bi {
            let (x, y) = cross(a, b, t);
            out.push((x, y, t));
        }
    }
    out
}

/// Whether level `t` cuts this cell in two separate places.
///
/// Walking the four corners in order, the field crosses `t` an even number
/// of times. Two crossings is the ordinary case: one entry, one exit, and
/// the region inside the cell is convex. Four means the cell is a saddle
/// -- opposite corners on one side of the level and the other two on the
/// other -- where the level makes two separate arcs and a convex clip
/// cannot represent the result.
fn is_saddle(c: &[P; 4], t: f64) -> bool {
    let mut changes = 0;
    for k in 0..4 {
        if (c[k].2 >= t) != (c[(k + 1) % 4].2 >= t) {
            changes += 1;
        }
    }
    changes > 2
}

/// Filled regions where `lo <= z <= hi`, as polygons in data coordinates.
///
/// The polygons are not merged into one outline. They share exact edges, so
/// a renderer filling them all with the same colour produces a continuous
/// region, and stitching them into a single path with holes would be a lot
/// of work for no visible difference. What *is* merged is the interior --
/// see the comment in the body, which is a size problem rather than a
/// cosmetic one.
pub fn contour_bands(g: &Grid, lo: f64, hi: f64) -> Vec<Vec<(f64, f64)>> {
    if !g.is_valid() {
        return Vec::new();
    }
    let (nx, ny) = (g.x.len(), g.y.len());
    let mut out = Vec::new();

    // Most of a band is its interior, and clipping there is wasted work
    // that produces four triangles where one rectangle would do. On the
    // 400x400 grid this was written for, emitting a polygon per triangle
    // gave 534,397 of them and a 55 MB SVG -- a correct figure that no
    // viewer or journal could open. So interior cells are found first and
    // merged into horizontal runs, and only cells the band's edge actually
    // crosses go through the clipper.
    let interior = |i: usize, j: usize| -> bool {
        [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
            .iter()
            .all(|&(a, b)| {
                let v = g.z[b * nx + a];
                v.is_finite() && v >= lo && v <= hi
            })
    };

    for j in 0..ny - 1 {
        let mut i = 0;
        while i < nx - 1 {
            if !interior(i, j) {
                // A cell the band's edge crosses (or one that is masked).
                let c = [
                    g.at(i, j),
                    g.at(i + 1, j),
                    g.at(i + 1, j + 1),
                    g.at(i, j + 1),
                ];
                if c.iter().all(|p| p.2.is_finite()) {
                    // Clipping the whole quad at once gives the same region
                    // as clipping its four centre-triangles and unioning
                    // them -- but as ONE polygon instead of four, and the
                    // boundary cells are where nearly all of the output
                    // comes from.
                    //
                    // The exception is a saddle: there the band enters and
                    // leaves the cell twice, the region is not convex, and
                    // Sutherland-Hodgman (which only handles convex clips)
                    // would connect the wrong pair of crossings. That is
                    // the case the centre point exists to resolve, so those
                    // cells still go through the four triangles.
                    if is_saddle(&c, lo) || is_saddle(&c, hi) {
                        let mid = (
                            (c[0].0 + c[1].0 + c[2].0 + c[3].0) / 4.0,
                            (c[0].1 + c[1].1 + c[2].1 + c[3].1) / 4.0,
                            (c[0].2 + c[1].2 + c[2].2 + c[3].2) / 4.0,
                        );
                        for k in 0..4 {
                            let tri = [c[k], c[(k + 1) % 4], mid];
                            let clipped = clip(&clip(&tri, lo, true), hi, false);
                            if clipped.len() >= 3 {
                                out.push(clipped.into_iter().map(|p| (p.0, p.1)).collect());
                            }
                        }
                    } else {
                        let clipped = clip(&clip(&c, lo, true), hi, false);
                        if clipped.len() >= 3 {
                            out.push(clipped.into_iter().map(|p| (p.0, p.1)).collect());
                        }
                    }
                }
                i += 1;
                continue;
            }
            // Extend the run of fully-inside cells along this row.
            let start = i;
            while i < nx - 1 && interior(i, j) {
                i += 1;
            }
            out.push(vec![
                (g.x[start], g.y[j]),
                (g.x[i], g.y[j]),
                (g.x[i], g.y[j + 1]),
                (g.x[start], g.y[j + 1]),
            ]);
        }
    }
    out
}

/// Iso-lines at `level`, chained into polylines in data coordinates.
pub fn contour_lines(g: &Grid, level: f64) -> Vec<Vec<(f64, f64)>> {
    if !g.is_valid() {
        return Vec::new();
    }
    let mut segments: Vec<[(f64, f64); 2]> = Vec::new();
    for tri in g.triangles() {
        // Where the plane z = level cuts a linear triangle there are
        // exactly two crossings, unless the level misses the triangle
        // entirely or grazes a vertex.
        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(2);
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            let (da, db) = (a.2 - level, b.2 - level);
            // `>` not `>=` on one side only, so a vertex exactly on the
            // level is counted by one edge and not both -- otherwise a
            // grazing level produces a duplicated point and a zero-length
            // segment.
            if (da < 0.0 && db >= 0.0) || (da >= 0.0 && db < 0.0) {
                pts.push(cross(a, b, level));
            }
        }
        if pts.len() == 2 && (pts[0].0 != pts[1].0 || pts[0].1 != pts[1].1) {
            segments.push([pts[0], pts[1]]);
        }
    }
    chain(segments)
}

/// Join segments end to end into the longest polylines they form.
///
/// Contour segments come out of the triangle sweep in grid order, not in
/// path order, so drawing them individually gives a stroke made of
/// thousands of disconnected pieces -- visible as a dotted line wherever
/// the renderer puts caps on, and impossible to label. Chaining them makes
/// each iso-line one path.
fn chain(segments: Vec<[(f64, f64); 2]>) -> Vec<Vec<(f64, f64)>> {
    // Endpoints are compared through a quantized key rather than by float
    // equality. They are computed by the same interpolation from the same
    // two corner values on both sides of a shared edge, so they agree to
    // the last bit in the ordinary case -- but a level passing exactly
    // through a grid vertex reaches it from two different edges, and those
    // do not agree bitwise.
    fn key(p: (f64, f64)) -> (i64, i64) {
        const SCALE: f64 = 1e9;
        ((p.0 * SCALE).round() as i64, (p.1 * SCALE).round() as i64)
    }

    use std::collections::HashMap;
    let mut ends: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        ends.entry(key(s[0])).or_default().push(i);
        ends.entry(key(s[1])).or_default().push(i);
    }

    let mut used = vec![false; segments.len()];
    let mut paths = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut path = vec![segments[start][0], segments[start][1]];

        // Extend from both ends until nothing connects.
        for forward in [true, false] {
            loop {
                let tip = if forward { *path.last().unwrap() } else { path[0] };
                let Some(cands) = ends.get(&key(tip)) else { break };
                let Some(&next) = cands.iter().find(|&&i| !used[i]) else { break };
                used[next] = true;
                let [a, b] = segments[next];
                let other = if key(a) == key(tip) { b } else { a };
                if forward {
                    path.push(other);
                } else {
                    path.insert(0, other);
                }
            }
        }
        paths.push(path);
    }
    paths
}

/// Evenly spaced levels spanning the field's finite range.
///
/// Used when the caller does not name levels. `n` is a target, not a
/// promise: the span is divided into `n` intervals, which is what
/// matplotlib's default does too.
pub fn auto_levels(z: &[f64], n: usize) -> Vec<f64> {
    let finite: Vec<f64> = z.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() || n == 0 {
        return Vec::new();
    }
    let lo = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !(hi > lo) {
        return vec![lo];
    }
    (0..=n).map(|k| lo + (hi - lo) * k as f64 / n as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plane `z = x` has its level-`t` contour at exactly `x = t`, for
    /// every point on it. Anything wrong with the interpolation, the
    /// triangle split, or the chaining shows up here as a point off that
    /// line.
    fn plane_grid() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..11).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..7).map(|j| j as f64).collect();
        let mut z = Vec::new();
        for _ in &y {
            for &xi in &x {
                z.push(xi);
            }
        }
        (x, y, z)
    }

    #[test]
    fn a_planar_field_contours_on_a_straight_line() {
        let (x, y, z) = plane_grid();
        let g = Grid { x: &x, y: &y, z: &z };
        let paths = contour_lines(&g, 3.5);
        assert!(!paths.is_empty(), "expected a contour at z = 3.5");
        for p in &paths {
            for &(px, py) in p {
                assert!((px - 3.5).abs() < 1e-9, "point ({px}, {py}) is not on x = 3.5");
            }
        }
        // And it should span the full height of the grid, in one path.
        let ys: Vec<f64> = paths.iter().flatten().map(|p| p.1).collect();
        assert!(ys.iter().cloned().fold(f64::INFINITY, f64::min) <= 0.0 + 1e-9);
        assert!(ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) >= 6.0 - 1e-9);
    }

    /// Segments must be joined, not left loose: an unchained contour is
    /// thousands of separate strokes, which cannot be labelled and renders
    /// as a dotted line.
    #[test]
    fn segments_are_chained_into_few_paths_not_many() {
        let (x, y, z) = plane_grid();
        let g = Grid { x: &x, y: &y, z: &z };
        let paths = contour_lines(&g, 3.5);
        assert!(paths.len() <= 2, "expected the contour chained, got {} paths", paths.len());
        assert!(paths[0].len() > 5, "expected a path with many points, got {:?}", paths[0]);
    }

    /// A level below or above everything has no contour -- not an empty
    /// path, and not a panic.
    #[test]
    fn a_level_outside_the_data_produces_nothing() {
        let (x, y, z) = plane_grid();
        let g = Grid { x: &x, y: &y, z: &z };
        assert!(contour_lines(&g, -5.0).is_empty());
        assert!(contour_lines(&g, 99.0).is_empty());
    }

    /// The band `lo..hi` of a planar field is a rectangle, so the polygons
    /// must tile exactly that strip and nothing outside it.
    #[test]
    fn bands_cover_exactly_the_strip_between_two_levels() {
        let (x, y, z) = plane_grid();
        let g = Grid { x: &x, y: &y, z: &z };
        let polys = contour_bands(&g, 2.0, 4.0);
        assert!(!polys.is_empty());
        for p in &polys {
            for &(px, _) in p {
                assert!(
                    (2.0 - 1e-9..=4.0 + 1e-9).contains(&px),
                    "band polygon reaches x = {px}, outside [2, 4]"
                );
            }
        }
        // Total area must equal the strip: 2 wide by 6 tall.
        let area: f64 = polys.iter().map(|p| shoelace(p).abs()).sum();
        assert!((area - 12.0).abs() < 1e-6, "band area {area}, expected 12");
    }

    fn shoelace(p: &[(f64, f64)]) -> f64 {
        let mut a = 0.0;
        for k in 0..p.len() {
            let (x1, y1) = p[k];
            let (x2, y2) = p[(k + 1) % p.len()];
            a += x1 * y2 - x2 * y1;
        }
        a / 2.0
    }

    /// A masked region must stay empty. Interpolating towards a `NaN`
    /// would draw a boundary the data does not have -- exactly the failure
    /// that makes a wrong figure look plausible.
    #[test]
    fn nan_cells_are_dropped_whole_rather_than_interpolated_into() {
        let x: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..5).map(|j| j as f64).collect();
        let mut z: Vec<f64> = Vec::new();
        for j in 0..5 {
            for i in 0..5 {
                // Mask the whole right half.
                z.push(if i >= 3 { f64::NAN } else { i as f64 + j as f64 });
            }
        }
        let g = Grid { x: &x, y: &y, z: &z };
        for p in contour_bands(&g, 0.0, 10.0).iter().flatten() {
            assert!(p.0 <= 2.0 + 1e-9, "a band polygon reached into the masked half at x = {}", p.0);
        }
        for p in contour_lines(&g, 3.0).iter().flatten() {
            assert!(p.0 <= 2.0 + 1e-9, "a contour line reached into the masked half at x = {}", p.0);
        }
    }

    #[test]
    fn auto_levels_span_the_finite_range_and_ignore_nan() {
        let z = vec![0.0, f64::NAN, 10.0, 5.0];
        let l = auto_levels(&z, 5);
        assert_eq!(l.len(), 6);
        assert_eq!(l[0], 0.0);
        assert_eq!(*l.last().unwrap(), 10.0);
    }

    #[test]
    fn a_grid_whose_z_does_not_match_its_axes_yields_nothing_rather_than_panicking() {
        let x = [0.0, 1.0];
        let y = [0.0, 1.0];
        let z = [1.0, 2.0, 3.0]; // should be 4
        let g = Grid { x: &x, y: &y, z: &z };
        assert!(!g.is_valid());
        assert!(contour_lines(&g, 1.5).is_empty());
        assert!(contour_bands(&g, 1.0, 2.0).is_empty());
    }
}
