//! The `image` native module — the image toolkit (`docs/design/toolkit-image.md`),
//! shipped as an importable module rather than core builtins (spec §28's
//! extension system, same shape as the `text`/`xlsx`/`codec` modules).
//!
//! **What this crate is, and is not.** The core already has a broad image op
//! set (`blur`, `edge_detect`, morphology, thresholds, `bwlabel`, ...). What
//! the gap analysis measured as missing is *meaning*: measurements without
//! units, and the higher-level analysis ops. This module adds exactly that,
//! over plain pixel data:
//!
//! * [`regions`] — the spec's §5. `regionprops` returns a *list* of records
//!   in *pixels*; `regions` returns a table of columns scaled to a stated
//!   physical size, so a measurement can be filtered, grouped, joined and
//!   plotted with the language's ordinary verbs.
//! * Edges and analysis the core lacks: [`canny`], [`bilateral`],
//!   [`distance_transform`], [`skeleton`], [`fill_holes`], [`contours`].
//! * [`blur`] with `space="linear"` — the spec's §1: filtering happens in
//!   linear light by default, so the sRGB-blur artefact stops being a thing
//!   you have to know about.
//!
//! **What is not here, on purpose.** The spec's `Image` carries `dtype`,
//! `space`, `alpha`, `pixel_size` on the *type*; here they are explicit
//! arguments (`pixel_size=`/`unit=`), because changing the core `Image`'s
//! representation is a core change and this is an extension. An 8-bit RGB
//! image under these operations is range-preserving by construction
//! (monotone 8-bit maps, non-negative kernels summing to 1), so a
//! clipped-pixel counter would count nothing; the spec's `on_clip` contract
//! lands when the image type itself carries a wider dtype.
//!
//! **Pixel layout.** Every function takes what the core's own `Image`
//! struct holds: row-major 8-bit RGB, `(y * width + x) * 3`, top-left
//! origin. Binary and grayscale data is a `Vec<u8>` of `width * height`.
//! The `Value::Image` conversion is the dispatch arm's job, not this
//! crate's — so this crate has no dependency on the interpreter.

/// A regions table: column name + values, in the order to display them.
#[derive(Debug, Clone)]
pub struct RegionTable {
    pub columns: Vec<(String, Vec<f64>)>,
}

/// ITU-R BT.601 luma, the same weights the core's `to_grayscale` uses.
pub fn luma(px: &[u8]) -> Vec<f64> {
    let mut out = Vec::with_capacity(px.len() / 3);
    for c in px.chunks_exact(3) {
        out.push(0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64);
    }
    out
}

/// `regions(labels, h, w, count, pixel_size=, unit=, intensity=)` — the
/// spec's §5.
///
/// `labels` is the label matrix a `bwlabel`/`label_blobs` run produced,
/// row-major, 0 = background, `count` the number of labels. Returns one row
/// per non-empty label: area, centroid, bounding box, extent, and this
/// crate's shape-metric extension (perimeter, eccentricity, orientation,
/// solidity), plus `intensity_mean` when `intensity` is given.
///
/// **Why the unit is in the column name.** A table column is numbers — there
/// is nowhere to hang a unit tag on a column, so an area in µm² is reported
/// as `area_um2` and the pixel case as `area_px2`. That is less than the
/// spec asks for (`r.area == 412 um^2`) and is chosen over the alternative
/// of a column named `area` whose unit depends on an argument the reader
/// cannot see from the result. The name changes when the meaning changes.
///
/// **`perimeter`** walks the region's outer boundary pixel CENTRES (8-
/// connected Moore trace, the same algorithm [`contours`] uses, isolated to
/// this one label via [`region_boundary`]) and sums the Euclidean distance
/// between consecutive centres, closing the loop back to the start. That is
/// a different, smaller convention than the common "count 1 for every
/// orthogonal border-pixel edge, 2 for the whole rectangle" formula: a
/// filled axis-aligned `W`x`H` rectangle (`W, H >= 2`) has NO diagonal steps
/// in its border ring, so this walk measures exactly `2*(W+H-2)` pixels, not
/// `2*(W+H)`. A single-pixel region has no boundary segment to walk and
/// reports `0`. Scaled by `pixel_size` like the other lengths (`bbox_width`,
/// ...), never squared.
///
/// **`eccentricity`/`orientation`** come from the region's second central
/// moments treated as an equivalent ellipse of the same area and covariance
/// (the standard "moments-based ellipse fit": axis lengths `4*sqrt(eigenvalue)`
/// of the normalized covariance matrix `[[mu20, mu11], [mu11, mu02]] / area`,
/// using each pixel's own integer `(x, y)` as a point sample — the same
/// convention `centroid_x`/`centroid_y` already use, not a pixel-as-unit-
/// square correction). `eccentricity = sqrt(1 - (minor/major)^2)`, `0` for a
/// circle/square, approaching `1` for a thin line. `orientation` is
/// `0.5 * atan2(2*mu11, mu20 - mu02)` radians in `(-pi/2, pi/2]`: `0` means
/// the major axis runs along `+x` (columns); a positive angle rotates
/// towards `+y` (rows, which increase DOWNWARD in this row-major pixel
/// layout, so a positive `orientation` turns the same way a clock's hands
/// do when the image is displayed normally, not counter-clockwise the way
/// it would in a plotted x/y coordinate system). A region with no
/// directional spread (a single pixel, or an exactly isotropic blob) has
/// both eigenvalues equal to `0` or to each other; `orientation` is defined
/// as `0` in that case by the same `atan2(0, 0) = 0` convention Rust's
/// `f64::atan2` already uses, and `eccentricity` is `0`.
///
/// **`solidity`** is `area / convex_hull_area`, unitless (the scale cancels,
/// like `extent`). The hull is built from every boundary pixel's four unit-
/// square CORNERS (`(x, y)` .. `(x+1, y+1)`), not its center — using centers
/// would make a solid, perfectly convex rectangle's hull SMALLER than its
/// own pixel-count area (the hull of `W` collinear centers spans `W-1`, not
/// `W`), reporting solidity slightly above `1`, which is not a valid ratio.
/// With corners, a solid convex region's hull area equals its pixel-count
/// area exactly, and solidity is `1.0`.
///
/// **`intensity_mean`** is the mean ITU-R BT.601 luma (see [`luma`]) of
/// `intensity` over the region's pixels — the mean of the ORIGINAL image,
/// not the label matrix. Omitted from the returned table entirely when
/// `intensity` is `None`, the same "absence changes which columns exist"
/// pattern `pixel_size`/`unit` already use.
pub fn regions(
    labels: &[f64],
    h: usize,
    w: usize,
    count: usize,
    pixel_size: Option<f64>,
    unit: Option<&str>,
    intensity: Option<&[f64]>,
) -> Result<RegionTable, String> {
    if labels.len() != h * w {
        return Err(format!(
            "regions: the label matrix has {} values, expected {} ({}x{})",
            labels.len(),
            h * w,
            w,
            h
        ));
    }
    if let Some(px) = intensity {
        if px.len() != h * w {
            return Err(format!(
                "regions: `intensity_image=` has {} values, expected {} ({}x{}) -- it must be \
                 the same size as the labeled image",
                px.len(),
                h * w,
                w,
                h
            ));
        }
    }
    // `pixel_size` is the edge length of one pixel. Absent means the
    // measurement stays in pixels and says so in the column names, rather
    // than defaulting to 1 of some unit nobody stated.
    if let Some(p) = pixel_size {
        if !(p > 0.0) || !p.is_finite() {
            return Err(format!(
                "regions: `pixel_size={p}` must be a positive, finite length per pixel"
            ));
        }
    }
    // A unit name with no scale would label pixel counts as though they had
    // been converted — the spectrogram `fs=1` default is exactly the trap
    // being avoided here. (An explicit `unit="px"` with no scale is just the
    // default spelled out, so it is allowed; a scale with `unit="px"` is a
    // contradiction, because the measurement IS scaled.)
    if let Some(u) = unit {
        if pixel_size.is_none() && u != "px" {
            return Err(
                "regions: `unit=` was given without `pixel_size=` -- a unit name with no \
                 scale would label pixel counts as though they had been converted"
                    .to_string(),
            );
        }
        if u == "px" && pixel_size.is_some() {
            return Err(
                "regions: `unit=\"px\"` with a `pixel_size=` is a contradiction -- the \
                 measurement IS scaled, so name the unit it is scaled to"
                    .to_string(),
            );
        }
    }
    let unit = unit.unwrap_or("px");
    let (len_suffix, area_suffix) = match pixel_size {
        None => ("_px".to_string(), "_px2".to_string()),
        Some(_) => (format!("_{unit}"), format!("_{unit}2")),
    };
    let k = pixel_size.unwrap_or(1.0);

    let mut area = vec![0u64; count + 1];
    let mut sum_x = vec![0f64; count + 1];
    let mut sum_y = vec![0f64; count + 1];
    let mut min_x = vec![usize::MAX; count + 1];
    let mut max_x = vec![0usize; count + 1];
    let mut min_y = vec![usize::MAX; count + 1];
    let mut max_y = vec![0usize; count + 1];
    for y in 0..h {
        for x in 0..w {
            let lbl = labels[y * w + x].round() as i64;
            if lbl <= 0 || lbl as usize > count {
                continue;
            }
            let l = lbl as usize;
            area[l] += 1;
            sum_x[l] += x as f64;
            sum_y[l] += y as f64;
            min_x[l] = min_x[l].min(x);
            max_x[l] = max_x[l].max(x);
            min_y[l] = min_y[l].min(y);
            max_y[l] = max_y[l].max(y);
        }
    }

    // Raw (unscaled) centroids, needed before the second pass can accumulate
    // deviations from them -- central moments require the mean first, so
    // this is a genuine two-pass computation, not an optimization left on
    // the table.
    let mut raw_cx = vec![0f64; count + 1];
    let mut raw_cy = vec![0f64; count + 1];
    for l in 1..=count {
        if area[l] > 0 {
            raw_cx[l] = sum_x[l] / area[l] as f64;
            raw_cy[l] = sum_y[l] / area[l] as f64;
        }
    }

    let mut mu20 = vec![0f64; count + 1];
    let mut mu02 = vec![0f64; count + 1];
    let mut mu11 = vec![0f64; count + 1];
    let mut intensity_sum = vec![0f64; count + 1];
    for y in 0..h {
        for x in 0..w {
            let lbl = labels[y * w + x].round() as i64;
            if lbl <= 0 || lbl as usize > count {
                continue;
            }
            let l = lbl as usize;
            let dx = x as f64 - raw_cx[l];
            let dy = y as f64 - raw_cy[l];
            mu20[l] += dx * dx;
            mu02[l] += dy * dy;
            mu11[l] += dx * dy;
            if let Some(px) = intensity {
                intensity_sum[l] += px[y * w + x];
            }
        }
    }

    let mut label_c = Vec::new();
    let mut area_c = Vec::new();
    let mut cx_c = Vec::new();
    let mut cy_c = Vec::new();
    let mut bw_c = Vec::new();
    let mut bh_c = Vec::new();
    let mut ext_c = Vec::new();
    let mut perim_c = Vec::new();
    let mut ecc_c = Vec::new();
    let mut orient_c = Vec::new();
    let mut solidity_c = Vec::new();
    let mut intensity_c = Vec::new();
    for l in 1..=count {
        if area[l] == 0 {
            continue;
        }
        let a = area[l] as f64;
        let bw = (max_x[l] - min_x[l] + 1) as f64;
        let bh = (max_y[l] - min_y[l] + 1) as f64;
        label_c.push(l as f64);
        area_c.push(a * k * k);
        cx_c.push(raw_cx[l] * k);
        cy_c.push(raw_cy[l] * k);
        bw_c.push(bw * k);
        bh_c.push(bh * k);
        // Extent is a ratio, so it is unitless and the scale cancels --
        // stated explicitly because a column that silently did NOT scale
        // would look identical to one that was forgotten.
        ext_c.push(a / (bw * bh));

        let boundary = region_boundary(labels, w, l, min_x[l], max_x[l], min_y[l], max_y[l]);
        perim_c.push(polygon_perimeter_closed(&boundary) * k);

        // Normalized covariance matrix [[a11, a12], [a12, a22]] = mu / area.
        let a11 = mu20[l] / a;
        let a22 = mu02[l] / a;
        let a12 = mu11[l] / a;
        let mean = (a11 + a22) / 2.0;
        let disc = (((a11 - a22) / 2.0).powi(2) + a12 * a12).sqrt();
        let lambda_major = (mean + disc).max(0.0);
        let lambda_minor = (mean - disc).max(0.0);
        if lambda_major <= 0.0 {
            // No directional spread at all (a single pixel, or every
            // included pixel coincident) -- both axes are zero-length, so
            // "eccentricity" and "orientation" have nothing to measure.
            ecc_c.push(0.0);
            orient_c.push(0.0);
        } else {
            let ratio_sq = (lambda_minor / lambda_major).clamp(0.0, 1.0);
            ecc_c.push((1.0 - ratio_sq).sqrt());
            orient_c.push(0.5 * (2.0 * a12).atan2(a11 - a22));
        }

        let corners: Vec<(f64, f64)> = boundary
            .iter()
            .flat_map(|&(px, py)| [(px, py), (px + 1.0, py), (px, py + 1.0), (px + 1.0, py + 1.0)])
            .collect();
        let hull = convex_hull(&corners);
        let hull_area = polygon_area_shoelace(&hull);
        // A degenerate hull (a single pixel's corners are 4 collinear-ish
        // points only when the region is 1x1, where the "hull" is the unit
        // square itself, area 1) never divides by zero here because a
        // non-empty region always has `hull_area >= a` by construction: the
        // hull's corner-point set contains every foreground pixel's own
        // unit square.
        solidity_c.push(if hull_area > 0.0 { a / hull_area } else { 1.0 });

        if intensity.is_some() {
            intensity_c.push(intensity_sum[l] / a);
        }
    }

    let mut columns = vec![
        ("label".to_string(), label_c),
        (format!("area{area_suffix}"), area_c),
        (format!("centroid_x{len_suffix}"), cx_c),
        (format!("centroid_y{len_suffix}"), cy_c),
        (format!("bbox_width{len_suffix}"), bw_c),
        (format!("bbox_height{len_suffix}"), bh_c),
        ("extent".to_string(), ext_c),
        (format!("perimeter{len_suffix}"), perim_c),
        ("eccentricity".to_string(), ecc_c),
        ("orientation".to_string(), orient_c),
        ("solidity".to_string(), solidity_c),
    ];
    if intensity.is_some() {
        columns.push(("intensity_mean".to_string(), intensity_c));
    }

    Ok(RegionTable { columns })
}

// ------------------------------------------------------------ sRGB math

fn srgb_to_linear(v: f64) -> f64 {
    let v = v / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v < 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    ((s * 255.0).round()).clamp(0.0, 255.0) as u8
}

/// A Gaussian kernel of `2*radius+1` taps, sigma = radius/2 + 0.5 (so
/// `radius=1` is the 3-tap `[1,2,1]/4` shape the core's 3x3 blur uses).
fn gauss_kernel(radius: usize) -> Vec<f64> {
    let sigma = radius as f64 / 2.0 + 0.5;
    let mut k = Vec::with_capacity(2 * radius + 1);
    for i in -(radius as isize)..=radius as isize {
        let x = i as f64;
        k.push((-0.5 * x * x / (sigma * sigma)).exp());
    }
    let sum: f64 = k.iter().sum();
    k.iter().map(|x| x / sum).collect()
}

/// Separable Gaussian blur over the three channels. `linear = true`
/// (the spec's §1 default) converts sRGB → linear light, filters, converts
/// back: blurring in sRGB is a measurable error (the transfer function is
/// not linear, so a blurred sRGB pixel is not the blur of the light), and
/// doing the round trip here means the caller cannot forget it.
/// `linear = false` is the named way to say "I meant it in sRGB".
pub fn blur(px: &[u8], w: usize, h: usize, radius: usize, linear: bool) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let kernel = gauss_kernel(radius);
    let r = kernel.len() / 2;
    let mut planes = [
        vec![0.0f64; w * h],
        vec![0.0f64; w * h],
        vec![0.0f64; w * h],
    ];
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                let v = px[(y * w + x) * 3 + c] as f64;
                planes[c][y * w + x] = if linear { srgb_to_linear(v) } else { v };
            }
        }
    }
    // Vertical pass.
    let mut tmp = planes.clone();
    for c in 0..3 {
        let src = &planes[c];
        let dst = &mut tmp[c];
        for x in 0..w {
            for y in 0..h {
                let mut acc = 0.0;
                for (j, k) in kernel.iter().enumerate() {
                    let idx = (y as isize + j as isize - r as isize).clamp(0, h as isize - 1) as usize;
                    acc += *k * src[idx * w + x];
                }
                dst[y * w + x] = acc;
            }
        }
    }
    // Horizontal pass.
    for c in 0..3 {
        let src = &tmp[c];
        let dst = &mut planes[c];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0;
                for (j, k) in kernel.iter().enumerate() {
                    let idx = (x as isize + j as isize - r as isize).clamp(0, w as isize - 1) as usize;
                    acc += *k * src[y * w + idx];
                }
                dst[y * w + x] = acc;
            }
        }
    }
    let mut out = vec![0u8; px.len()];
    for i in 0..w * h {
        for c in 0..3 {
            let v = planes[c][i];
            out[i * 3 + c] = if linear {
                linear_to_srgb(v)
            } else {
                v.round().clamp(0.0, 255.0) as u8
            };
        }
    }
    out
}

// ----------------------------------------------------------------- edges

/// Sobel gradient: `gx`/`gy`, each `w*h` row-major, over the image's BT.601
/// luma. Kernel taps `[-1,0,1; -2,0,2; -1,0,1]` for `gx` (transposed for
/// `gy`) — the standard 3x3 Sobel, border-clamped ("replicate") the same way
/// every other neighbourhood op in this module handles the edge.
///
/// Was private (canny's own gradient step, see its call site below) until
/// the 2026-09-24 gap pass exposed it as `sobel`/`gradient_magnitude` in
/// their own right — no change to the math, only to its visibility.
pub fn sobel(px: &[u8], w: usize, h: usize) -> (Vec<f64>, Vec<f64>) {
    let g = luma(px);
    let mut gx = vec![0.0f64; g.len()];
    let mut gy = vec![0.0f64; g.len()];
    for y in 0..h {
        for x in 0..w {
            let at = |dx: isize, dy: isize| -> f64 {
                let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                g[yy * w + xx]
            };
            gx[y * w + x] =
                (at(1, -1) + 2.0 * at(1, 0) + at(1, 1)) - (at(-1, -1) + 2.0 * at(-1, 0) + at(-1, 1));
            gy[y * w + x] =
                (at(-1, 1) + 2.0 * at(0, 1) + at(1, 1)) - (at(-1, -1) + 2.0 * at(0, -1) + at(1, -1));
        }
    }
    (gx, gy)
}

/// Scharr gradient: same shape as [`sobel`] (BT.601 luma in, `gx`/`gy` row-
/// major out, replicate border), but with the Scharr 3x3 kernel
/// `[-3,0,3; -10,0,10; -3,0,3]` (transposed for `gy`) — better rotational
/// symmetry than Sobel's, the textbook reason to reach for it when the
/// gradient DIRECTION matters and not just where an edge is.
pub fn scharr(px: &[u8], w: usize, h: usize) -> (Vec<f64>, Vec<f64>) {
    let g = luma(px);
    let mut gx = vec![0.0f64; g.len()];
    let mut gy = vec![0.0f64; g.len()];
    for y in 0..h {
        for x in 0..w {
            let at = |dx: isize, dy: isize| -> f64 {
                let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                g[yy * w + xx]
            };
            gx[y * w + x] = (3.0 * at(1, -1) + 10.0 * at(1, 0) + 3.0 * at(1, 1))
                - (3.0 * at(-1, -1) + 10.0 * at(-1, 0) + 3.0 * at(-1, 1));
            gy[y * w + x] = (3.0 * at(-1, 1) + 10.0 * at(0, 1) + 3.0 * at(1, 1))
                - (3.0 * at(-1, -1) + 10.0 * at(0, -1) + 3.0 * at(1, -1));
        }
    }
    (gx, gy)
}

/// Elementwise gradient magnitude `sqrt(gx^2 + gy^2)` from a `gx`/`gy` pair
/// (a [`sobel`] or [`scharr`] result) — shared by both, and by the standalone
/// `gradient_magnitude` builtin, so the three don't each carry their own
/// copy of the same `sqrt(a*a + b*b)`.
pub fn magnitude(gx: &[f64], gy: &[f64]) -> Vec<f64> {
    gx.iter().zip(gy.iter()).map(|(&a, &b)| (a * a + b * b).sqrt()).collect()
}

/// Discrete Laplacian over the image's BT.601 luma, `w*h` row-major, raw
/// (unclamped, signed) response — the numeric twin of `Image::edge_detect3x3`
/// (which applies the same `kernel_size=3` kernel per RGB channel and clamps
/// to `u8`): this one stays in floating point and reads luma once, for a
/// caller that wants the actual second-derivative values rather than a
/// clamped preview image.
///
/// `kernel_size`:
/// * `3` — the standard 4-neighbour kernel `[0,-1,0; -1,4,-1; 0,-1,0]`, the
///   same one `Image::edge_detect3x3` uses.
/// * `5` — the standard 5x5 discrete Laplacian (`[0,0,-1,0,0; 0,-1,-2,-1,0;
///   -1,-2,16,-2,-1; 0,-1,-2,-1,0; 0,0,-1,0,0]`), a wider-support second
///   derivative that's less sensitive to single-pixel noise than the 3x3.
///   Both kernels sum to zero, as a discrete Laplacian must (a constant
///   image produces an all-zero response).
///
/// Any other `kernel_size` is refused by name rather than silently rounded
/// to the nearest supported one.
pub fn laplacian(px: &[u8], w: usize, h: usize, kernel_size: usize) -> Result<Vec<f64>, String> {
    let g = luma(px);
    let at = |g: &[f64], x: isize, y: isize| -> f64 {
        let xx = x.clamp(0, w as isize - 1) as usize;
        let yy = y.clamp(0, h as isize - 1) as usize;
        g[yy * w + xx]
    };
    let mut out = vec![0.0f64; g.len()];
    match kernel_size {
        3 => {
            for y in 0..h as isize {
                for x in 0..w as isize {
                    let v = 4.0 * at(&g, x, y)
                        - at(&g, x - 1, y)
                        - at(&g, x + 1, y)
                        - at(&g, x, y - 1)
                        - at(&g, x, y + 1);
                    out[(y as usize) * w + (x as usize)] = v;
                }
            }
        }
        5 => {
            #[rustfmt::skip]
            let k: [[f64; 5]; 5] = [
                [ 0.0,  0.0, -1.0,  0.0,  0.0],
                [ 0.0, -1.0, -2.0, -1.0,  0.0],
                [-1.0, -2.0, 16.0, -2.0, -1.0],
                [ 0.0, -1.0, -2.0, -1.0,  0.0],
                [ 0.0,  0.0, -1.0,  0.0,  0.0],
            ];
            for y in 0..h as isize {
                for x in 0..w as isize {
                    let mut acc = 0.0f64;
                    for (ky, row) in k.iter().enumerate() {
                        for (kx, &kv) in row.iter().enumerate() {
                            if kv == 0.0 {
                                continue;
                            }
                            acc += kv * at(&g, x + kx as isize - 2, y + ky as isize - 2);
                        }
                    }
                    out[(y as usize) * w + (x as usize)] = acc;
                }
            }
        }
        other => {
            return Err(format!(
                "laplacian: kernel_size must be 3 or 5, got {other}"
            ));
        }
    }
    Ok(out)
}

/// Canny edge detection: Gaussian smooth → Sobel magnitude → non-maximum
/// suppression → hysteresis with `low`/`high` as FRACTIONS OF THE MAXIMUM
/// MAGNITUDE in the image (0..1), the convention `edge(canny, low: 0.1,
/// high: 0.3)` in the spec implies. Returns a greyscale edge map (255 on an
/// edge, 0 off).
pub fn canny(px: &[u8], w: usize, h: usize, low: f64, high: f64) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    if !(0.0..1.0).contains(&low) || !(0.0..1.0).contains(&high) || low >= high {
        return vec![0u8; px.len()];
    }
    let smoothed = blur(px, w, h, 1, true);
    let (gx, gy) = sobel(&smoothed, w, h);
    let mut mag = vec![0.0f64; w * h];
    let mut max_mag = 0.0f64;
    for i in 0..w * h {
        mag[i] = (gx[i] * gx[i] + gy[i] * gy[i]).sqrt();
        max_mag = max_mag.max(mag[i]);
    }
    if max_mag == 0.0 {
        return vec![0u8; px.len()];
    }
    let t_low = low * max_mag;
    let t_high = high * max_mag;

    // Non-maximum suppression: keep a pixel only if its magnitude is the
    // local maximum along its gradient direction.
    let mut nms = vec![0.0f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let m = mag[i];
            if m == 0.0 {
                continue;
            }
            let at = |dx: isize, dy: isize| -> f64 {
                let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                mag[yy * w + xx]
            };
            let (nx, ny) = (gx[i].abs(), gy[i].abs());
            let nmx = if nx >= ny {
                // Gradient is mostly horizontal: compare along x.
                if gx[i] >= 0.0 { (at(1, 0), at(-1, 0)) } else { (at(-1, 0), at(1, 0)) }
            } else if gy[i] >= 0.0 {
                (at(1, 1), at(-1, -1))
            } else {
                (at(-1, 1), at(1, -1))
            };
            if m >= nmx.0 && m >= nmx.1 {
                nms[i] = m;
            }
        }
    }

    // Hysteresis: strong edges (>= high) seed a flood of weak edges (>= low).
    let mut out = vec![0u8; px.len()];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..w * h {
        if nms[i] >= t_high {
            out[i * 3] = 255;
            out[i * 3 + 1] = 255;
            out[i * 3 + 2] = 255;
        }
    }
    for i in 0..w * h {
        if nms[i] >= t_high {
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        let (yy0, xx0) = (i / w, i % w);
        for (dy, dx) in [(-1i64, -1i64), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
            let xx = xx0 as i64 + dx;
            let yy = yy0 as i64 + dy;
            if xx < 0 || yy < 0 || xx >= w as i64 || yy >= h as i64 {
                continue;
            }
            let j = (yy * w as i64 + xx) as usize;
            if out[j * 3] == 0 && nms[j] >= t_low {
                out[j * 3] = 255;
                out[j * 3 + 1] = 255;
                out[j * 3 + 2] = 255;
                stack.push(j);
            }
        }
    }
    out
}

// ------------------------------------------------------------- bilateral

/// Bilateral filter: a Gaussian weighted by both spatial distance and
/// intensity difference, so it smooths flat regions without blurring
/// across edges. `sigma_space` in pixels (integer radius = its ceiling),
/// `sigma_color` in luma units (0..255). O(w·h·(2r+1)²) — fine for the
/// images this language processes, and it says so in its cost rather than
/// pretending otherwise.
pub fn bilateral(px: &[u8], w: usize, h: usize, sigma_space: f64, sigma_color: f64) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    if !(sigma_space > 0.0) || !(sigma_color > 0.0) {
        return px.to_vec();
    }
    let r = sigma_space.ceil() as usize;
    let g2 = 2.0 * sigma_color * sigma_color;
    let s2 = 2.0 * sigma_space * sigma_space;
    let g = luma(px);
    let mut out = vec![0u8; px.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut wr = 0.0f64;
            let mut wg = 0.0f64;
            let mut wb = 0.0f64;
            let mut wsum = 0.0f64;
            for dy in -(r as isize)..=r as isize {
                for dx in -(r as isize)..=r as isize {
                    let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                    let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                    let j = yy * w + xx;
                    let d2 = dx as f64 * dx as f64 + dy as f64 * dy as f64;
                    let diff = g[j] - g[i];
                    let weight = (-d2 / s2 - diff * diff / g2).exp();
                    wr += weight * px[j * 3] as f64;
                    wg += weight * px[j * 3 + 1] as f64;
                    wb += weight * px[j * 3 + 2] as f64;
                    wsum += weight;
                }
            }
            out[i * 3] = (wr / wsum).round().clamp(0.0, 255.0) as u8;
            out[i * 3 + 1] = (wg / wsum).round().clamp(0.0, 255.0) as u8;
            out[i * 3 + 2] = (wb / wsum).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

// ------------------------------------------------------------- morphology

/// Distance transform of a binary image (nonzero = foreground): each
/// foreground pixel gets its distance in PIXELS to the nearest background
/// pixel (0 for a background pixel). The image border counts as background,
/// the same convention `bwdist` uses, so an all-foreground image measures
/// to the edge of the image rather than to nothing. Two-pass chamfer with
/// the (1, √2) metric — exact for 4-connectivity, within 1 of the true
/// Euclidean distance.
pub fn distance_transform(binary: &[u8], w: usize, h: usize) -> Vec<f64> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let big = 1e9f64;
    // Padded with a one-pixel background border: that is what makes "the
    // border counts as background" fall out of the ordinary init below
    // instead of needing a special case.
    let pw = w + 2;
    let ph = h + 2;
    let mut d = vec![big; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            let is_bg = x == 0 || y == 0 || x == pw - 1 || y == ph - 1 || binary[(y - 1) * w + (x - 1)] == 0;
            if is_bg {
                d[y * pw + x] = 0.0;
            }
        }
    }
    let two = std::f64::consts::SQRT_2;
    // Forward pass: top-left to bottom-right.
    for y in 0..ph {
        for x in 0..pw {
            let i = y * pw + x;
            let mut v = d[i];
            if y > 0 {
                v = v.min(d[i - pw] + 1.0);
                if x > 0 {
                    v = v.min(d[i - pw - 1] + two);
                }
                if x + 1 < pw {
                    v = v.min(d[i - pw + 1] + two);
                }
            }
            if x > 0 {
                v = v.min(d[i - 1] + 1.0);
            }
            d[i] = v;
        }
    }
    // Backward pass: bottom-right to top-left.
    for y in (0..ph).rev() {
        for x in (0..pw).rev() {
            let i = y * pw + x;
            let mut v = d[i];
            if y + 1 < ph {
                v = v.min(d[i + pw] + 1.0);
                if x > 0 {
                    v = v.min(d[i + pw - 1] + two);
                }
                if x + 1 < pw {
                    v = v.min(d[i + pw + 1] + two);
                }
            }
            if x + 1 < pw {
                v = v.min(d[i + 1] + 1.0);
            }
            d[i] = v;
        }
    }
    // Crop the padding back off.
    let mut out = vec![0.0f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let v = d[(y + 1) * pw + (x + 1)];
            out[y * w + x] = if v >= big / 2.0 { 0.0 } else { v };
        }
    }
    out
}

/// Zhang-Suen thinning: iteratively erodes a binary image until only the
/// 1-pixel-wide skeleton remains. Nonzero = foreground.
pub fn skeleton(binary: &[u8], w: usize, h: usize) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let mut img = binary.to_vec();
    let n = w * h;
    // Out-of-range is background, not the clamped border pixel: a bar that
    // touches the image edge has the outside of the image on the other side
    // of that edge, and clamping would hand the test conditions a phantom
    // neighbour.
    let nb = |img: &[u8], x: usize, y: usize, dx: isize, dy: isize| -> u8 {
        let xx = x as isize + dx;
        let yy = y as isize + dy;
        if xx < 0 || yy < 0 || xx >= w as isize || yy >= h as isize {
            0
        } else {
            img[(yy as usize) * w + (xx as usize)]
        }
    };
    let mut changed = true;
    let mut guard = 0u32;
    while changed && guard < 1000 {
        changed = false;
        guard += 1;
        for step in 0..2 {
            let mut to_remove = vec![false; n];
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    if img[i] == 0 {
                        continue;
                    }
                    // 8 neighbours in the standard cyclic order p2..p9,
                    // as 0/1 — the test conditions multiply them, and the
                    // binary uses 255 for foreground, not 1.
                    let p: [u8; 8] = [
                        nb(&img, x, y, 0, -1),
                        nb(&img, x, y, 1, -1),
                        nb(&img, x, y, 1, 0),
                        nb(&img, x, y, 1, 1),
                        nb(&img, x, y, 0, 1),
                        nb(&img, x, y, -1, 1),
                        nb(&img, x, y, -1, 0),
                        nb(&img, x, y, -1, -1),
                    ]
                    .map(|v| if v != 0 { 1 } else { 0 });
                    let b: u32 = p.iter().map(|&v| v as u32).sum();
                    if b < 2 || b > 6 {
                        continue;
                    }
                    // A(z): the number of 0->1 transitions in the CYCLIC
                    // sequence, all eight pairs including the wrap p8->p2.
                    // Missing the wrap pair lets a straight line (two
                    // collinear neighbours, A=2) pass as A=1 and the
                    // algorithm eats the whole line.
                    let trans: u32 = (0..8)
                        .map(|k| {
                            if p[k] == 0 && p[(k + 1) % 8] != 0 {
                                1
                            } else {
                                0
                            }
                        })
                        .sum();
                    if trans != 1 {
                        continue;
                    }
                    if step == 0 {
                        if p[0] * p[2] * p[4] != 0 || p[2] * p[4] * p[6] != 0 {
                            continue;
                        }
                    } else {
                        if p[0] * p[2] * p[6] != 0 || p[0] * p[4] * p[6] != 0 {
                            continue;
                        }
                    }
                    to_remove[i] = true;
                    changed = true;
                }
            }
            for i in 0..n {
                if to_remove[i] {
                    img[i] = 0;
                }
            }
        }
    }
    img
}

/// Fill the background holes of a binary image: background pixels not
/// reachable from the image border by a 4-connected background path are
/// holes and become foreground. Nonzero = foreground.
pub fn fill_holes(binary: &[u8], w: usize, h: usize) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let mut out = binary.to_vec();
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let push = |stack: &mut Vec<usize>, x: usize, y: usize, seen: &mut Vec<bool>, out: &[u8]| {
        let i = y * w + x;
        if !seen[i] && out[i] == 0 {
            seen[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        push(&mut stack, x, 0, &mut seen, &out);
        push(&mut stack, x, h - 1, &mut seen, &out);
    }
    for y in 0..h {
        push(&mut stack, 0, y, &mut seen, &out);
        push(&mut stack, w - 1, y, &mut seen, &out);
    }
    while let Some(i) = stack.pop() {
        let (y, x) = (i / w, i % w);
        for (dy, dx) in [(0isize, 1), (0, -1), (1, 0), (-1, 0)] {
            let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
            let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
            push(&mut stack, xx, yy, &mut seen, &out);
        }
    }
    for i in 0..w * h {
        if !seen[i] && out[i] == 0 {
            out[i] = 255;
        }
    }
    out
}

// ------------------------------------------------------------ contours

/// Moore-neighbor tracing of each 8-connected foreground component,
/// returning one polyline per component as `(x, y)` pairs, scaled by
/// `scale` (pixels × scale = physical units, the spec's "contours returned
/// as polylines in physical units"). The starting point of each contour is
/// the first pixel encountered in raster order; the direction of traversal
/// is clockwise.
pub fn contours(binary: &[u8], w: usize, h: usize, scale: Option<f64>) -> Vec<Vec<(f64, f64)>> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let k = scale.unwrap_or(1.0);
    let mut visited = vec![false; w * h];
    let mut result = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let start = y * w + x;
            if binary[start] == 0 || visited[start] {
                continue;
            }
            if !is_border_pixel(binary, w, h, x, y) {
                continue;
            }
            let contour = trace_one_component(binary, w, h, &mut visited, x, y);
            result.push(contour.into_iter().map(|(px, py)| (px * k, py * k)).collect());
        }
    }
    result
}

// 8 directions, clockwise from east (the Moore neighbourhood). Shared by
// `contours` and `region_boundary` (`regions`'s per-label perimeter/hull
// input) so the two boundary walks cannot drift apart.
const MOORE_DIRS: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// True when `(x, y)` has at least one background (or out-of-frame)
/// 8-neighbour -- i.e. it is on the outer edge of its component, not buried
/// inside a solid blob. Starting a Moore trace anywhere else never finds its
/// way back to the start (every neighbour is foreground, so it just walks
/// off into the interior), which is why every trace start is filtered
/// through this first.
fn is_border_pixel(binary: &[u8], w: usize, h: usize, x: usize, y: usize) -> bool {
    MOORE_DIRS.iter().any(|&(dx, dy)| {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize || binary[(ny as usize) * w + (nx as usize)] == 0
    })
}

/// Moore-neighbor trace of the single foreground component touching border
/// pixel `(x0, y0)`, in raw pixel coordinates (no `scale` applied -- the
/// caller's job, since `regions`'s per-label caller needs pixel units for
/// its own area-scaled columns while `contours` wants physical units).
/// Marks every pixel it visits in `visited` so a caller iterating the whole
/// image in raster order does not re-trace the same component from a
/// different starting pixel.
fn trace_one_component(binary: &[u8], w: usize, h: usize, visited: &mut [bool], x0: usize, y0: usize) -> Vec<(f64, f64)> {
    let mut contour: Vec<(f64, f64)> = Vec::new();
    let mut cx = x0 as isize;
    let mut cy = y0 as isize;
    let mut entry = 6usize; // came from the north-west
    // A closed Moore trace visits each border pixel at most a constant
    // number of times before returning to its start; this cap is pure
    // defense in depth against a still-unforeseen degenerate shape, not the
    // primary fix (that's the is_border_pixel check at the call site) -- so
    // it is generous, not tight.
    let max_steps = w * h + 4;
    for _ in 0..max_steps {
        let i = (cy as usize) * w + (cx as usize);
        if !visited[i] {
            visited[i] = true;
            contour.push((cx as f64, cy as f64));
        }
        // Moore's rule: search clockwise starting from the neighbour just
        // clockwise of the back pixel (where we came from), not from its
        // opposite — starting opposite is what makes a trace back-track up
        // the edge it just came down instead of continuing around the blob.
        let mut found = false;
        for step in 0..8 {
            let d = (entry + 1 + step) % 8;
            let nx = cx + MOORE_DIRS[d].0;
            let ny = cy + MOORE_DIRS[d].1;
            if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                continue;
            }
            if binary[(ny as usize) * w + (nx as usize)] != 0 {
                cx = nx;
                cy = ny;
                entry = (d + 4) % 8;
                found = true;
                break;
            }
        }
        if !found || (cx == x0 as isize && cy == y0 as isize && contour.len() > 1) {
            break;
        }
    }
    contour
}

/// The boundary of a single label's region, in global pixel coordinates
/// (unscaled) -- `regions`'s perimeter/solidity input. Builds a mask cropped
/// to the region's own bounding box (isolating `label` from every other
/// value, including a touching different label, which a plain binary plane
/// of "foreground" would conflate) and Moore-traces it with the exact same
/// algorithm `contours` uses, so the two boundary notions cannot disagree.
fn region_boundary(
    labels: &[f64],
    w: usize,
    label: usize,
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
) -> Vec<(f64, f64)> {
    let bw = max_x - min_x + 1;
    let bh = max_y - min_y + 1;
    let mut mask = vec![0u8; bw * bh];
    for ly in 0..bh {
        for lx in 0..bw {
            let gx = min_x + lx;
            let gy = min_y + ly;
            if labels[gy * w + gx].round() as i64 == label as i64 {
                mask[ly * bw + lx] = 255;
            }
        }
    }
    let mut visited = vec![false; bw * bh];
    for ly in 0..bh {
        for lx in 0..bw {
            if mask[ly * bw + lx] == 0 || visited[ly * bw + lx] {
                continue;
            }
            if !is_border_pixel(&mask, bw, bh, lx, ly) {
                continue;
            }
            return trace_one_component(&mask, bw, bh, &mut visited, lx, ly)
                .into_iter()
                .map(|(px, py)| (px + min_x as f64, py + min_y as f64))
                .collect();
        }
    }
    // Every pixel of a non-empty region is, by construction, a border pixel
    // of its own cropped mask (the crop is exactly the region's bbox, so at
    // minimum the pixels touching the bbox edge have an out-of-mask
    // neighbour) -- this is unreached for `area[l] > 0`, kept only so the
    // function is total.
    Vec::new()
}

/// Closed-polyline length: consecutive Euclidean distances plus the segment
/// closing the last point back to the first. `points` are boundary PIXEL
/// CENTRES (as `trace_one_component`/`region_boundary` produce them), so
/// this measures the path through those centres, not the pixel-edge
/// crossing count some other tools call "perimeter" -- see `regions`'s doc
/// comment for the worked comparison.
fn polygon_perimeter_closed(points: &[(f64, f64)]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut total = 0.0;
    for w in points.windows(2) {
        total += ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
    }
    let first = points[0];
    let last = points[points.len() - 1];
    total += ((last.0 - first.0).powi(2) + (last.1 - first.1).powi(2)).sqrt();
    total
}

/// Andrew's monotone chain: the convex hull of `points`, counter-clockwise,
/// without a repeated closing point. `<= 2` distinct points returns them
/// unchanged (no polygon to close).
fn convex_hull(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.partial_cmp(&b.1).unwrap()));
    pts.dedup();
    if pts.len() <= 2 {
        return pts;
    }
    fn cross(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    }
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The shoelace formula: area of a simple polygon given in order (either
/// winding). `< 3` points has no interior.
fn polygon_area_shoelace(points: &[(f64, f64)]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % points.len()];
        sum += x0 * y1 - x1 * y0;
    }
    (sum / 2.0).abs()
}

// ------------------------------------------------------------ watershed

/// One priority-queue entry for `watershed`'s flood: ascending by
/// `surface` value (so the queue floods low points first), ties broken by
/// insertion order (`seq`) for a deterministic result independent of
/// `HashMap`/iteration-order accidents. `f64` has no total order (NaN), so
/// this wraps the comparison rather than deriving `Ord` -- `partial_cmp`
/// unwrapped, since a NaN surface value is a caller bug this isn't trying
/// to handle gracefully, matching this crate's other float-heavy code
/// (`distance_transform`, `skeleton`) which makes the same assumption.
#[derive(PartialEq)]
struct FloodEntry {
    value: f64,
    seq: u64,
    idx: usize,
}
impl Eq for FloodEntry {}
impl Ord for FloodEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed, so `BinaryHeap` (a max-heap) pops the SMALLEST value
        // first -- turns it into the min-heap the flood needs without a
        // separate `Reverse` wrapper at every call site.
        other
            .value
            .partial_cmp(&self.value)
            .unwrap()
            .then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for FloodEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Marker-controlled watershed segmentation (priority-flood / Meyer's
/// algorithm). `surface` (row-major, `w*h`) is the topographic surface to
/// flood -- typically the NEGATED output of [`distance_transform`] (so a
/// blob's centre, the point farthest from its boundary, is the surface's
/// deepest point and floods first) or a gradient magnitude. `markers`
/// (row-major, `w*h`, same shape) is the seed label buffer: `0` means
/// unlabeled/to-be-flooded, any other value is a seed already assigned to
/// that label.
///
/// Floods strictly by ascending `surface` value, 4-connected, via a binary
/// min-heap: each unlabeled pixel adjacent to already-labeled territory is
/// queued at its own surface value, and the queue always processes the
/// globally lowest still-queued point next. When a queued pixel is
/// popped, if every already-labeled neighbor it now has agrees on one
/// label, it takes that label and queues its own unlabeled neighbors in
/// turn; if its labeled neighbors disagree (two different regions'
/// flood-fronts have both reached it), it becomes a **watershed line**
/// pixel instead, and does not propagate further -- this is what stops
/// two touching regions from merging into one.
///
/// **Watershed-line convention**: a line pixel is written `0`, the same
/// value `markers` already uses for "unlabeled" (not `-1`), so the whole
/// result stays a plain label buffer that composes with e.g. [`regions`]
/// without a caller having to special-case a negative sentinel. The
/// tradeoff this makes explicit: a genuinely unreached pixel (unreachable
/// from any seed at all, which cannot happen here since the flood covers
/// every pixel the surface has) and a real watershed-line pixel are both
/// `0` in the output; they are only distinguishable, if it matters, by
/// checking whether all of a `0` pixel's neighbors are non-zero (a line)
/// or not.
///
/// **No `mask=`**: every pixel in `surface` is flooded, including regions
/// with no nearby marker -- there is no "outside the region of interest"
/// concept here, unlike some watershed variants. A caller that wants
/// certain pixels excluded from ever taking a real label should pre-seed
/// them as their own dedicated marker id and discard that id afterward,
/// or restrict `surface`'s dynamic range so those pixels flood last.
///
/// **`markers` is required, not derived from local extrema when omitted.**
/// A "derive seeds from the input automatically" fallback is a second,
/// separate algorithm (peak/minima finding, itself parameter-sensitive --
/// how close is "the same" extremum, how flat a plateau counts) bolted
/// onto a function whose entire point is that a marker-CONTROLLED result
/// is only as correct as the markers it's given. Silently guessing them
/// would make a wrong guess look like a wrong watershed instead of what it
/// actually is: a wrong guess. Refusing (by construction: this function
/// simply requires the argument, and the dispatch arm around it names the
/// keyword in its error) keeps that failure honest.
pub fn watershed(surface: &[f64], markers: &[i32], w: usize, h: usize) -> Vec<i32> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let n = w * h;
    let mut labels = markers.to_vec();
    let mut visited = vec![false; n];
    let mut queued = vec![false; n];
    for i in 0..n {
        if labels[i] != 0 {
            visited[i] = true;
        }
    }
    let neighbors4 = |idx: usize| -> Vec<usize> {
        let (x, y) = (idx % w, idx / w);
        let mut out = Vec::with_capacity(4);
        if x > 0 {
            out.push(idx - 1);
        }
        if x + 1 < w {
            out.push(idx + 1);
        }
        if y > 0 {
            out.push(idx - w);
        }
        if y + 1 < h {
            out.push(idx + w);
        }
        out
    };
    let mut heap: std::collections::BinaryHeap<FloodEntry> = std::collections::BinaryHeap::new();
    let mut seq: u64 = 0;
    let push = |heap: &mut std::collections::BinaryHeap<FloodEntry>, queued: &mut [bool], seq: &mut u64, idx: usize| {
        if !queued[idx] {
            queued[idx] = true;
            heap.push(FloodEntry { value: surface[idx], seq: *seq, idx });
            *seq += 1;
        }
    };
    for i in 0..n {
        if labels[i] != 0 {
            for nb in neighbors4(i) {
                if !visited[nb] {
                    push(&mut heap, &mut queued, &mut seq, nb);
                }
            }
        }
    }
    while let Some(FloodEntry { idx, .. }) = heap.pop() {
        if visited[idx] {
            continue;
        }
        let mut found: Option<i32> = None;
        let mut conflict = false;
        for nb in neighbors4(idx) {
            let l = labels[nb];
            if l != 0 {
                match found {
                    None => found = Some(l),
                    Some(existing) if existing != l => conflict = true,
                    _ => {}
                }
            }
        }
        visited[idx] = true;
        match found {
            Some(l) if !conflict => {
                labels[idx] = l;
                for nb in neighbors4(idx) {
                    if !visited[nb] {
                        push(&mut heap, &mut queued, &mut seq, nb);
                    }
                }
            }
            _ => {
                // Either a genuine conflict (two regions met here -- a
                // watershed line) or, defensively, no labeled neighbor at
                // all (shouldn't happen: `idx` is only ever queued because
                // a labeled neighbor pushed it, and labels only go from 0
                // to non-zero, never back). Either way: leave it `0` and
                // do not propagate past it.
                labels[idx] = 0;
            }
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_img(w: usize, h: usize, f: impl Fn(usize, usize) -> bool) -> Vec<u8> {
        let mut b = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if f(x, y) {
                    b[y * w + x] = 255;
                }
            }
        }
        b
    }

    fn rgb(w: usize, h: usize, f: impl Fn(usize, usize) -> (u8, u8, u8)) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = f(x, y);
                let i = (y * w + x) * 3;
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
            }
        }
        px
    }

    /// A label matrix: 0 = background, 1 on the blob — the shape a
    /// `bwlabel`/`label_blobs` run produces, where the label VALUE is the
    /// label number, not 255.
    fn label_matrix(w: usize, h: usize, f: impl Fn(usize, usize) -> bool) -> Vec<f64> {
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                out.push(if f(x, y) { 1.0 } else { 0.0 });
            }
        }
        out
    }

    #[test]
    fn regions_scales_area_by_the_square_of_the_pixel_size() {
        // A 2x2 blob at (5,5): area 4 px², centroid (5.5, 5.5).
        let labels = label_matrix(10, 10, |x, y| (5..=6).contains(&x) && (5..=6).contains(&y));
        let px = regions(&labels, 10, 10, 1, None, None, None).unwrap();
        let area = px.columns.iter().find(|(n, _)| n == "area_px2").unwrap().1.clone();
        let cx = px.columns.iter().find(|(n, _)| n == "centroid_x_px").unwrap().1.clone();
        assert_eq!(area[0], 4.0);
        assert_eq!(cx[0], 5.5);

        let um = regions(&labels, 10, 10, 1, Some(0.65), Some("um"), None).unwrap();
        let area = um.columns.iter().find(|(n, _)| n == "area_um2").unwrap().1.clone();
        assert!((area[0] - 4.0 * 0.65 * 0.65).abs() < 1e-12);
        let ext = um.columns.iter().find(|(n, _)| n == "extent").unwrap().1.clone();
        assert_eq!(ext[0], 1.0, "extent is a ratio; the scale must cancel");
    }

    #[test]
    fn a_unit_without_a_scale_is_refused() {
        let labels = vec![0.0; 10];
        let err = regions(&labels, 2, 5, 1, None, Some("um"), None).unwrap_err();
        assert!(err.contains("without `pixel_size=`"), "got: {err}");
    }

    #[test]
    fn a_unit_named_px_with_a_scale_is_refused() {
        let labels = vec![0.0; 10];
        assert!(regions(&labels, 2, 5, 1, Some(0.65), Some("px"), None).is_err());
    }

    #[test]
    fn the_fixture_blob_is_fully_solid() {
        // The regions test above would pass against an empty blob if the
        // labelling were wrong: area would be 0 rows, not 4. This pins the
        // fixture itself.
        let labels = label_matrix(10, 10, |x, y| (5..=6).contains(&x) && (5..=6).contains(&y));
        let t = regions(&labels, 10, 10, 1, None, None, None).unwrap();
        assert_eq!(t.columns[0].1.len(), 1, "the 2x2 blob must label exactly one region");
    }

    fn col<'a>(t: &'a RegionTable, name: &str) -> &'a [f64] {
        &t.columns.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no column `{name}`")).1
    }

    // A filled, axis-aligned W x H rectangle is the cleanest known-answer
    // fixture for every new shape metric at once, because every one of them
    // has an exact closed form for it (no continuous-vs-discrete
    // approximation to worry about):
    //
    //   * `perimeter`: the border ring of a solid rectangle is connected by
    //     ORTHOGONAL steps only (no diagonal jump is ever needed to walk
    //     from one border pixel to the next), so the closed-loop walk this
    //     crate uses is exactly `2*(W+H-2)` for W,H >= 2 -- e.g. W=5,H=3
    //     has a 12-pixel ring (15 pixels total minus a 3x1 interior), not
    //     the `2*(W+H)=16` "edge crossing" convention some other tools use.
    //   * `eccentricity`/`orientation`: for pixel centres at integer
    //     coordinates 0..W-1 (a discrete uniform distribution), the exact
    //     population variance is `(W*W-1)/12` -- a standard closed form,
    //     not an approximation -- so `mu20/area = (W^2-1)/12`,
    //     `mu02/area = (H^2-1)/12`, `mu11 = 0` (axis-aligned, so x and y
    //     deviations are independent).
    //   * `solidity`: a rectangle's convex hull is itself, and this crate's
    //     hull is built from pixel CORNERS specifically so a convex
    //     region's hull area equals its own pixel-count area -- solidity
    //     must come out to exactly `1.0`, not merely "close to 1".
    #[test]
    fn rectangle_shape_metrics_match_hand_computed_values() {
        // 5 wide (x: 2..=6), 3 tall (y: 2..=4), inside a 12x10 canvas so the
        // region touches none of the image's own edges.
        let (bw, bh) = (5usize, 3usize);
        let labels = label_matrix(12, 10, |x, y| (2..2 + bw).contains(&x) && (2..2 + bh).contains(&y));
        let t = regions(&labels, 10, 12, 1, None, None, None).unwrap();

        assert_eq!(col(&t, "area_px2")[0], (bw * bh) as f64);

        let expected_perimeter = 2.0 * (bw + bh - 2) as f64;
        assert_eq!(
            col(&t, "perimeter_px")[0],
            expected_perimeter,
            "a solid rectangle's border ring is all orthogonal steps: 2*(W+H-2), not 2*(W+H)"
        );

        let mu20_over_a = ((bw * bw - 1) as f64) / 12.0;
        let mu02_over_a = ((bh * bh - 1) as f64) / 12.0;
        let expected_ecc = (1.0 - mu02_over_a / mu20_over_a).sqrt();
        let ecc = col(&t, "eccentricity")[0];
        assert!((ecc - expected_ecc).abs() < 1e-9, "expected {expected_ecc}, got {ecc}");

        // W > H, mu11 = 0: the major axis runs along +x, orientation = 0.
        assert_eq!(col(&t, "orientation")[0], 0.0);

        assert_eq!(col(&t, "solidity")[0], 1.0, "a convex region's hull area must equal its own area exactly");
    }

    #[test]
    fn a_taller_rectangle_has_a_vertical_major_axis() {
        // Swap W and H from the test above: same eccentricity (the ratio of
        // squared axis lengths is unchanged), but now H > W, so mu20 < mu02
        // and the major axis runs along +y -- orientation = pi/2 exactly
        // (mu11 = 0, so atan2(0, negative) = pi, halved).
        let (bw, bh) = (3usize, 5usize);
        let labels = label_matrix(10, 12, |x, y| (2..2 + bw).contains(&x) && (2..2 + bh).contains(&y));
        let t = regions(&labels, 12, 10, 1, None, None, None).unwrap();
        let orient = col(&t, "orientation")[0];
        assert!((orient - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "got {orient}");
    }

    #[test]
    fn right_triangle_eccentricity_and_orientation_match_the_continuous_closed_form() {
        // A filled right triangle with both legs length L, right angle at
        // the origin: {(x, y): x, y >= 0, x + y <= L-1}. Its continuous
        // (non-discretized) second central moments about the centroid
        // (L/3, L/3) are a standard textbook integral:
        //
        //   mu20/A = mu02/A = L^2/18   (equal: this triangle is symmetric
        //                               under swapping x and y)
        //   mu11/A = -L^2/36
        //
        // giving eigenvalues L^2/12 (major) and L^2/36 (minor) of the
        // normalized covariance matrix, so:
        //
        //   eccentricity = sqrt(1 - (L^2/36)/(L^2/12)) = sqrt(1 - 1/3) = sqrt(2/3)
        //   orientation  = 0.5*atan2(2*mu11, mu20-mu02) = 0.5*atan2(negative, 0) = -pi/4
        //
        // (Independently verified by direct discrete summation over several
        // L before writing this test: the ratio-based metrics below track
        // the continuous prediction to ~1e-10 even at L=20, because mu20
        // and mu02 are EXACTLY equal for every L by the x<->y symmetry of
        // this shape, not merely in the limit -- so a small, fast L is
        // already a tight check, not merely an asymptotic one.)
        let l = 24usize;
        let labels = label_matrix(l + 2, l + 2, |x, y| x + y <= l - 1);
        let t = regions(&labels, l + 2, l + 2, 1, None, None, None).unwrap();

        let expected_ecc = (2.0f64 / 3.0).sqrt();
        let ecc = col(&t, "eccentricity")[0];
        assert!((ecc - expected_ecc).abs() < 1e-6, "expected {expected_ecc}, got {ecc}");

        let expected_orientation = -std::f64::consts::FRAC_PI_4;
        let orient = col(&t, "orientation")[0];
        assert!((orient - expected_orientation).abs() < 1e-6, "expected {expected_orientation}, got {orient}");
    }

    #[test]
    fn solidity_drops_below_one_for_a_non_convex_notch() {
        // A 6x6 square (x, y in 0..=5) with a 2-pixel notch cut from the
        // MIDDLE of the top edge (x in {2,3}, y=0) -- deliberately not
        // touching any of the square's four corners, so all four extreme
        // corner points ((0,0), (6,0), (0,6), (6,6) in this crate's
        // pixel-corner convention) are still present in the point set and
        // the convex hull is EXACTLY the full 6x6 bounding box, area 36,
        // with no approximation or staircase ambiguity to work out by hand.
        let notch = |x: usize, y: usize| y == 0 && (x == 2 || x == 3);
        let labels = label_matrix(6, 6, |x, y| !notch(x, y));
        let t = regions(&labels, 6, 6, 1, None, None, None).unwrap();

        let area = col(&t, "area_px2")[0];
        assert_eq!(area, 34.0, "36 pixels minus the 2-pixel notch");
        let solidity = col(&t, "solidity")[0];
        let expected = 34.0 / 36.0;
        assert!((solidity - expected).abs() < 1e-9, "expected {expected}, got {solidity}");
        assert!(solidity < 1.0, "a non-convex region's solidity must be strictly below 1");
    }

    #[test]
    fn intensity_mean_averages_the_original_image_not_the_label_matrix() {
        // Label matrix: a single 2x2 region at (1,1)-(2,2) in a 4x4 grid.
        // Intensity image: NOT binary/uniform, so the mean is a real check
        // rather than a value that would also fall out of a broken
        // implementation that mixed up labels and intensities.
        let labels = label_matrix(4, 4, |x, y| (1..=2).contains(&x) && (1..=2).contains(&y));
        #[rustfmt::skip]
        let intensity: Vec<f64> = vec![
            0.0,   0.0,   0.0,   0.0,
            0.0,  10.0,  30.0,   0.0,
            0.0,  50.0,  90.0,   0.0,
            0.0,   0.0,   0.0,   0.0,
        ];
        let t = regions(&labels, 4, 4, 1, None, None, Some(&intensity)).unwrap();
        let mean = col(&t, "intensity_mean")[0];
        // The region covers exactly the four interior values 10, 30, 50, 90.
        assert_eq!(mean, (10.0 + 30.0 + 50.0 + 90.0) / 4.0);

        // Omitting `intensity` must omit the column entirely, not fill it
        // with zeros or an error.
        let without = regions(&labels, 4, 4, 1, None, None, None).unwrap();
        assert!(
            without.columns.iter().all(|(n, _)| n != "intensity_mean"),
            "intensity_mean must not appear when no intensity image was given"
        );
    }

    #[test]
    fn blur_in_linear_light_is_not_the_same_as_blur_in_srgb() {
        // A dark pixel next to a bright one: the sRGB blur and the linear
        // blur differ by a measurable amount, and the linear one is the
        // one that is the blur OF THE LIGHT.
        let px = rgb(3, 1, |x, _| {
            let v = if x == 1 { 200u8 } else { 20 };
            (v, v, v)
        });
        let lin = blur(&px, 3, 1, 1, true);
        let srgb = blur(&px, 3, 1, 1, false);
        assert_ne!(lin, srgb, "the two colour spaces must actually differ");
        // The linear-light blur of symmetric 20/200 about the midpoint
        // lands at the linear midpoint, which is NOT 110 in sRGB.
        let mid = luma(&lin)[1];
        let mid_srgb = luma(&srgb)[1];
        assert!((mid - mid_srgb).abs() > 1.0, "mid={mid}, mid_srgb={mid_srgb}");
    }

    #[test]
    fn blur_preserves_a_flat_image_exactly() {
        let px = rgb(4, 4, |_, _| (100, 150, 200));
        let out = blur(&px, 4, 4, 2, true);
        assert_eq!(out, px, "a uniform image is its own blur");
    }

    #[test]
    fn canny_finds_the_edge_of_a_filled_square() {
        let px = rgb(20, 20, |x, y| {
            let v = if (5..=14).contains(&x) && (5..=14).contains(&y) { 255u8 } else { 0 };
            (v, v, v)
        });
        let edges = canny(&px, 20, 20, 0.1, 0.3);
        let g = luma(&edges);
        // The edge runs around the square's border: the deep interior is
        // clean, and the edge must be a substantial fraction of the
        // ~40-pixel perimeter rather than a couple of stray pixels.
        assert_eq!(g[10 * 20 + 10], 0.0, "the interior must be clean");
        let mut edge_pixels = 0usize;
        let mut interior_edges = 0usize;
        for y in 0..20 {
            for x in 0..20 {
                if g[y * 20 + x] > 0.0 {
                    edge_pixels += 1;
                    if (7..=12).contains(&x) && (7..=12).contains(&y) {
                        interior_edges += 1;
                    }
                }
            }
        }
        assert_eq!(interior_edges, 0, "no edge may fall deep inside the square");
        assert!(edge_pixels >= 20, "the perimeter is ~40 px; the edge must be a substantial fraction, got {edge_pixels}");
        // A flat image has no edge at all.
        let flat = rgb(10, 10, |_, _| (128, 128, 128));
        let none = canny(&flat, 10, 10, 0.1, 0.3);
        assert_eq!(luma(&none).iter().filter(|&&v| v > 0.0).count(), 0, "a flat image has no edges");
    }

    #[test]
    fn bilateral_smooths_flat_and_keeps_the_step() {
        // Left half 50, right half 200, one sharp step. A plain blur smears
        // the step; bilateral should keep the step sharp while flattening
        // noise inside the halves.
        let px = rgb(16, 1, |x, _| {
            let v = if x < 8 { 50u8 } else { 200 };
            (v, v, v)
        });
        let out = bilateral(&px, 16, 1, 2.0, 25.0);
        let g = luma(&out);
        assert!(g[3] < 70.0, "the flat left side must stay ~50, got {}", g[3]);
        assert!(g[12] > 180.0, "the flat right side must stay ~200, got {}", g[12]);
        // The step must still exist: a plain Gaussian of this width would
        // pull both sides substantially toward the middle.
        assert!(g[12] - g[3] > 80.0, "the step must survive, got {g:?}");
    }

    /// A pure vertical edge (left half dark, right half bright, no
    /// variation along y at all) is the textbook hand-computable case: the
    /// vertical gradient must be EXACTLY zero everywhere (every row is
    /// identical, so there is nothing for `gy` to see, border-clamping or
    /// not), and the horizontal gradient must be a specific nonzero number
    /// at the two columns straddling the edge and exactly zero everywhere
    /// else. Expected `gx` at the edge worked by hand against the standard
    /// Sobel kernel `[-1,0,1; -2,0,2; -1,0,1]`: three taps of `255` on the
    /// bright side, three of `0` on the dark side, `(255+2*255+255) - 0 =
    /// 1020`.
    #[test]
    fn sobel_on_a_vertical_edge_has_zero_vertical_response_and_a_known_horizontal_one() {
        let (w, h) = (8, 4);
        let px = rgb(w, h, |x, _| {
            let v = if x < 4 { 0u8 } else { 255 };
            (v, v, v)
        });
        let (gx, gy) = sobel(&px, w, h);
        assert!(gy.iter().all(|&v| v == 0.0), "no y-variation at all: gy must be exactly zero, got {gy:?}");
        for y in 0..h {
            for x in 0..w {
                let v = gx[y * w + x];
                if x == 3 || x == 4 {
                    assert_eq!(v, 1020.0, "at ({x},{y}), straddling the edge, got {v}");
                } else {
                    assert_eq!(v, 0.0, "at ({x},{y}), away from the edge, got {v}");
                }
            }
        }
    }

    /// Same edge, same hand-worked shape, but the Scharr kernel
    /// `[-3,0,3; -10,0,10; -3,0,3]`: `(3*255+10*255+3*255) - 0 = 4080`.
    #[test]
    fn scharr_on_a_vertical_edge_has_zero_vertical_response_and_a_known_horizontal_one() {
        let (w, h) = (8, 4);
        let px = rgb(w, h, |x, _| {
            let v = if x < 4 { 0u8 } else { 255 };
            (v, v, v)
        });
        let (gx, gy) = scharr(&px, w, h);
        assert!(gy.iter().all(|&v| v == 0.0), "no y-variation at all: gy must be exactly zero, got {gy:?}");
        for y in 0..h {
            for x in 0..w {
                let v = gx[y * w + x];
                if x == 3 || x == 4 {
                    assert_eq!(v, 4080.0, "at ({x},{y}), straddling the edge, got {v}");
                } else {
                    assert_eq!(v, 0.0, "at ({x},{y}), away from the edge, got {v}");
                }
            }
        }
    }

    #[test]
    fn gradient_magnitude_is_the_euclidean_norm_of_gx_gy() {
        // 3-4-5 triangle, so the expected answer is an exact integer rather
        // than something that only checks approximately.
        let gx = vec![3.0, 0.0, -3.0];
        let gy = vec![4.0, 5.0, 4.0];
        let mag = magnitude(&gx, &gy);
        assert_eq!(mag, vec![5.0, 5.0, 5.0]);
    }

    /// A single bright point on a dark field, `kernel_size=3`: at the
    /// point itself the standard 4-neighbour kernel `[0,-1,0; -1,4,-1;
    /// 0,-1,0]` gives `4*255 - (0+0+0+0) = 1020`; at each of its four
    /// direct neighbours it gives `4*0 - 255 = -255` (three dark neighbours
    /// contribute 0, the one bright neighbour -- the point itself --
    /// contributes `-255`); everywhere else (no neighbour touches the
    /// point) it is exactly zero. A flat image gives exactly zero
    /// everywhere, the other hand-obvious case (the kernel sums to zero by
    /// construction).
    #[test]
    fn laplacian_3x3_on_a_point_source_matches_the_kernel_by_hand() {
        let (w, h) = (5, 5);
        let px = rgb(w, h, |x, y| if x == 2 && y == 2 { (255, 255, 255) } else { (0, 0, 0) });
        let out = laplacian(&px, w, h, 3).unwrap();
        assert_eq!(out[2 * w + 2], 1020.0, "at the point itself");
        for &(nx, ny) in &[(1usize, 2usize), (3, 2), (2, 1), (2, 3)] {
            assert_eq!(out[ny * w + nx], -255.0, "at neighbour ({nx},{ny})");
        }
        assert_eq!(out[0], 0.0, "far corner, untouched by the point");

        let flat = rgb(w, h, |_, _| (77, 77, 77));
        let flat_out = laplacian(&flat, w, h, 3).unwrap();
        assert!(flat_out.iter().all(|&v| v == 0.0), "a flat image's Laplacian is exactly zero, got {flat_out:?}");
    }

    /// The 5x5 kernel sums to zero the same way the 3x3 one does, so a flat
    /// image is still the simplest hand-checkable case; the point-source
    /// numeric response is a straightforward kernel dot-product but not as
    /// hand-obvious as 3x3, so it is left to the flat-field zero check plus
    /// the "same sign, larger support" property against the 3x3 result.
    #[test]
    fn laplacian_5x5_is_zero_on_a_flat_image_and_rejects_other_kernel_sizes() {
        let (w, h) = (7, 7);
        let flat = rgb(w, h, |_, _| (140, 140, 140));
        let out = laplacian(&flat, w, h, 5).unwrap();
        assert!(out.iter().all(|&v| v == 0.0), "a flat image's Laplacian is exactly zero, got {out:?}");

        let err = laplacian(&flat, w, h, 4).unwrap_err();
        assert!(err.contains("kernel_size"), "got: {err}");
    }

    #[test]
    fn distance_transform_grows_away_from_the_background() {
        // A 5x5 all-foreground square: the border counts as background, so
        // a corner is 1 from the edge and the centre is 3 (the padding sits
        // one pixel beyond the border).
        let b = binary_img(5, 5, |_, _| true);
        let d = distance_transform(&b, 5, 5);
        assert!((d[0] - 1.0).abs() < 1e-12, "a corner is 1 from the border, got {}", d[0]);
        assert!((d[2 * 5 + 2] - 3.0).abs() < 1e-12, "centre of a 5x5 is 3 from the border, got {}", d[2 * 5 + 2]);
    }

    #[test]
    fn distance_transform_is_zero_on_background_and_finds_a_cavity() {
        // Foreground ring, background centre: the centre is background (0);
        // the ring pixel above the cavity is 1 from it; the corner is 1
        // from the (counted-as-background) image border.
        let b = binary_img(5, 5, |x, y| x == 0 || y == 0 || x == 4 || y == 4);
        let d = distance_transform(&b, 5, 5);
        assert_eq!(d[2 * 5 + 2], 0.0, "a background pixel is its own nearest background");
        assert!((d[1] - 1.0).abs() < 1e-12, "ring pixel (1,0) is 1 from the cavity, got {}", d[1]);
        assert!((d[0] - 1.0).abs() < 1e-12, "the corner is 1 from the image border, got {}", d[0]);
    }

    #[test]
    fn skeleton_reduces_a_thick_bar_to_one_pixel_wide() {
        // A 3-pixel-wide vertical bar: the skeleton must be 1 wide and
        // about as long.
        let b = binary_img(7, 10, |x, _| (2..=4).contains(&x));
        let s = skeleton(&b, 7, 10);
        let mut widths = Vec::new();
        for y in 0..10 {
            let row: Vec<bool> = (0..7).map(|x| s[y * 7 + x] != 0).collect();
            let n = row.iter().filter(|&&v| v).count();
            if n > 0 {
                widths.push(n);
            }
        }
        assert!(!widths.is_empty(), "the skeleton must not be empty: {s:?}");
        assert!(widths.iter().all(|&n| n <= 2), "a bar's skeleton is 1-wide (2 at most at joins), got widths {widths:?}");
        assert!(widths.len() >= 6, "the skeleton must keep the bar's length, got {} rows", widths.len());
    }

    #[test]
    fn fill_holes_closes_a_ring() {
        // A 5x5 ring of foreground with an empty centre: the centre is a
        // hole and must become foreground, while a background pixel
        // reachable from the border stays background.
        let ring = binary_img(5, 5, |x, y| x == 0 || y == 0 || x == 4 || y == 4);
        let f = fill_holes(&ring, 5, 5);
        assert_eq!(f[2 * 5 + 2], 255, "the centre of a closed ring is a hole");
        // A ring missing one pixel is open to the border: the centre is
        // not a hole.
        let open = binary_img(5, 5, |x, y| (x == 0 || y == 0 || x == 4 || y == 4) && !(x == 2 && y == 0));
        let f2 = fill_holes(&open, 5, 5);
        assert_eq!(f2[2 * 5 + 2], 0, "an open ring has no hole");
    }

    #[test]
    fn contours_traces_one_component_per_blob() {
        // Two disjoint 2x2 squares: two contours, four corners each
        // (a 2x2 block traces a 4-vertex loop).
        let b = binary_img(8, 8, |x, y| {
            ((1..=2).contains(&x) && (1..=2).contains(&y)) || ((5..=6).contains(&x) && (5..=6).contains(&y))
        });
        let c = contours(&b, 8, 8, None);
        assert_eq!(c.len(), 2, "two disjoint blobs trace two contours, got {}", c.len());
        for poly in &c {
            assert!(!poly.is_empty());
        }
    }

    #[test]
    fn contours_scale_to_physical_units() {
        let b = binary_img(4, 4, |x, y| x == 1 && y == 1);
        let c = contours(&b, 4, 4, Some(0.5));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0][0], (0.5, 0.5));
    }

    /// A plain 4-connected flood-fill component counter over a raw binary
    /// mask -- deliberately naive, with no watershed-style splitting. Used
    /// only to demonstrate the actual failure mode marker-controlled
    /// watershed exists to fix: two touching/overlapping blobs are ONE
    /// connected component to this, no matter how the flood is ordered.
    fn count_4connected_components(mask: &[u8], w: usize, h: usize) -> usize {
        let mut seen = vec![false; w * h];
        let mut count = 0;
        for start in 0..w * h {
            if mask[start] == 0 || seen[start] {
                continue;
            }
            count += 1;
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(idx) = stack.pop() {
                let (x, y) = (idx % w, idx / w);
                let candidates = [
                    (x.checked_sub(1), Some(y)),
                    (x.checked_add(1).filter(|&v| v < w), Some(y)),
                    (Some(x), y.checked_sub(1)),
                    (Some(x), y.checked_add(1).filter(|&v| v < h)),
                ];
                for (nx, ny) in candidates {
                    if let (Some(nx), Some(ny)) = (nx, ny) {
                        let nidx = ny * w + nx;
                        if mask[nidx] != 0 && !seen[nidx] {
                            seen[nidx] = true;
                            stack.push(nidx);
                        }
                    }
                }
            }
        }
        count
    }

    #[test]
    fn watershed_with_a_single_marker_labels_the_whole_flooded_surface() {
        // Base case, no splitting possible: one marker, flat surface (so
        // every tie is broken purely by queue order) -- the entire image
        // must end up carrying that one label, with no `0` watershed lines
        // anywhere (there is only ever one flood front, so it can never
        // meet a DIFFERENT one).
        let (w, h) = (6, 6);
        let surface = vec![0.0f64; w * h];
        let mut markers = vec![0i32; w * h];
        markers[0] = 7;
        let labels = watershed(&surface, &markers, w, h);
        assert!(labels.iter().all(|&l| l == 7), "a single marker on a flat surface must flood everything with its own label, got {labels:?}");
    }

    #[test]
    fn watershed_separates_two_touching_blobs_that_connected_components_cannot() {
        // The actual point of watershed, made concrete: two circular blobs
        // (radius 5, centers 8 apart -- overlapping by construction, not
        // merely adjacent) that a plain connected-component labeling sees
        // as ONE blob. Marker-controlled watershed, seeded with one marker
        // per circle's own center, must recover the two original regions.
        let (w, h) = (21usize, 13usize);
        let in_a = |x: usize, y: usize| {
            let (dx, dy) = (x as isize - 6, y as isize - 6);
            dx * dx + dy * dy <= 25
        };
        let in_b = |x: usize, y: usize| {
            let (dx, dy) = (x as isize - 14, y as isize - 6);
            dx * dx + dy * dy <= 25
        };
        let mask = binary_img(w, h, |x, y| in_a(x, y) || in_b(x, y));

        // Confirm the premise first: to a naive connected-component
        // labeling, this really is one single blob, not a pair that
        // happens to already be separable.
        assert_eq!(
            count_4connected_components(&mask, w, h),
            1,
            "the two circles must genuinely be one connected blob for this test to demonstrate anything"
        );

        let dt = distance_transform(&mask, w, h);
        // Negate so each circle's own center -- the point farthest from
        // any background, i.e. `distance_transform`'s local maximum -- is
        // the DEEPEST point of the surface watershed floods from first.
        let surface: Vec<f64> = dt.iter().map(|&d| -d).collect();

        let mut markers = vec![0i32; w * h];
        markers[6 * w + 6] = 1; // circle A's own center
        markers[6 * w + 14] = 2; // circle B's own center

        let labels = watershed(&surface, &markers, w, h);

        // Two points deep inside each circle's EXCLUSIVE territory (not in
        // the other circle at all), far from the overlap band.
        assert!(in_a(2, 6) && !in_b(2, 6));
        assert!(in_b(18, 6) && !in_a(18, 6));
        let idx_a = 6 * w + 2;
        let idx_b = 6 * w + 18;

        assert_eq!(labels[idx_a], 1, "circle A's exclusive territory must carry marker 1's label, got {}", labels[idx_a]);
        assert_eq!(labels[idx_b], 2, "circle B's exclusive territory must carry marker 2's label, got {}", labels[idx_b]);
        assert_ne!(
            labels[idx_a], labels[idx_b],
            "watershed must separate the two touching blobs into two distinct labels -- \
             naive connected-component labeling (confirmed above) cannot do this at all"
        );

        let distinct: std::collections::HashSet<i32> = labels.iter().copied().filter(|&l| l != 0).collect();
        assert_eq!(distinct.len(), 2, "expected exactly the two seeded labels to appear in the output, got {distinct:?}");
    }
}
