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

/// `regions(labels, h, w, count, pixel_size=, unit=)` — the spec's §5.
///
/// `labels` is the label matrix a `bwlabel`/`label_blobs` run produced,
/// row-major, 0 = background, `count` the number of labels. Returns one row
/// per non-empty label: area, centroid, bounding box and extent.
///
/// **Why the unit is in the column name.** A table column is numbers — there
/// is nowhere to hang a unit tag on a column, so an area in µm² is reported
/// as `area_um2` and the pixel case as `area_px2`. That is less than the
/// spec asks for (`r.area == 412 um^2`) and is chosen over the alternative
/// of a column named `area` whose unit depends on an argument the reader
/// cannot see from the result. The name changes when the meaning changes.
pub fn regions(
    labels: &[f64],
    h: usize,
    w: usize,
    count: usize,
    pixel_size: Option<f64>,
    unit: Option<&str>,
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

    let mut label_c = Vec::new();
    let mut area_c = Vec::new();
    let mut cx_c = Vec::new();
    let mut cy_c = Vec::new();
    let mut bw_c = Vec::new();
    let mut bh_c = Vec::new();
    let mut ext_c = Vec::new();
    for l in 1..=count {
        if area[l] == 0 {
            continue;
        }
        let a = area[l] as f64;
        let bw = (max_x[l] - min_x[l] + 1) as f64;
        let bh = (max_y[l] - min_y[l] + 1) as f64;
        label_c.push(l as f64);
        area_c.push(a * k * k);
        cx_c.push((sum_x[l] / a) * k);
        cy_c.push((sum_y[l] / a) * k);
        bw_c.push(bw * k);
        bh_c.push(bh * k);
        // Extent is a ratio, so it is unitless and the scale cancels --
        // stated explicitly because a column that silently did NOT scale
        // would look identical to one that was forgotten.
        ext_c.push(a / (bw * bh));
    }

    Ok(RegionTable {
        columns: vec![
            ("label".to_string(), label_c),
            (format!("area{area_suffix}"), area_c),
            (format!("centroid_x{len_suffix}"), cx_c),
            (format!("centroid_y{len_suffix}"), cy_c),
            (format!("bbox_width{len_suffix}"), bw_c),
            (format!("bbox_height{len_suffix}"), bh_c),
            ("extent".to_string(), ext_c),
        ],
    })
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

fn sobel(px: &[u8], w: usize, h: usize) -> (Vec<f64>, Vec<f64>) {
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
    // 8 directions, clockwise from east (the Moore neighbourhood).
    const DIRS: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    for y in 0..h {
        for x in 0..w {
            let start = y * w + x;
            if binary[start] == 0 || visited[start] {
                continue;
            }
            // Only start tracing from a genuine border pixel -- one with at
            // least one background (or out-of-frame) neighbour. An interior
            // pixel of a solid blob has every neighbour foreground too, so a
            // Moore trace started there always finds a next step and never
            // returns to its own start: it spins forever rather than
            // terminating. Skipping non-border starts is the fix; the
            // iteration cap a few lines down is defense in depth on top of
            // it, not a substitute for it.
            let is_border = DIRS.iter().any(|&(dx, dy)| {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                nx < 0
                    || ny < 0
                    || nx >= w as isize
                    || ny >= h as isize
                    || binary[(ny as usize) * w + (nx as usize)] == 0
            });
            if !is_border {
                continue;
            }
            let mut contour: Vec<(f64, f64)> = Vec::new();
            let mut cx = x as isize;
            let mut cy = y as isize;
            let mut entry = 6usize; // came from the north-west
            // A closed Moore trace visits each border pixel at most a
            // constant number of times before returning to its start; this
            // cap is pure defense in depth against a still-unforeseen
            // degenerate shape, not the primary fix (that's the is_border
            // check above) -- so it is generous, not tight.
            let max_steps = w * h + 4;
            for _ in 0..max_steps {
                let i = (cy as usize) * w + (cx as usize);
                if !visited[i] {
                    visited[i] = true;
                    contour.push((cx as f64 * k, cy as f64 * k));
                }
            // Moore's rule: search clockwise starting from the neighbour
            // just clockwise of the back pixel (where we came from), not
            // from its opposite — starting opposite is what makes a trace
            // back-track up the edge it just came down instead of
            // continuing around the blob.
            let mut found = false;
            for step in 0..8 {
                let d = (entry + 1 + step) % 8;
                let nx = cx + DIRS[d].0;
                let ny = cy + DIRS[d].1;
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
                if !found || (cx == x as isize && cy == y as isize && contour.len() > 1) {
                    break;
                }
            }
            result.push(contour);
        }
    }
    result
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
        let px = regions(&labels, 10, 10, 1, None, None).unwrap();
        let area = px.columns.iter().find(|(n, _)| n == "area_px2").unwrap().1.clone();
        let cx = px.columns.iter().find(|(n, _)| n == "centroid_x_px").unwrap().1.clone();
        assert_eq!(area[0], 4.0);
        assert_eq!(cx[0], 5.5);

        let um = regions(&labels, 10, 10, 1, Some(0.65), Some("um")).unwrap();
        let area = um.columns.iter().find(|(n, _)| n == "area_um2").unwrap().1.clone();
        assert!((area[0] - 4.0 * 0.65 * 0.65).abs() < 1e-12);
        let ext = um.columns.iter().find(|(n, _)| n == "extent").unwrap().1.clone();
        assert_eq!(ext[0], 1.0, "extent is a ratio; the scale must cancel");
    }

    #[test]
    fn a_unit_without_a_scale_is_refused() {
        let labels = vec![0.0; 10];
        let err = regions(&labels, 2, 5, 1, None, Some("um")).unwrap_err();
        assert!(err.contains("without `pixel_size=`"), "got: {err}");
    }

    #[test]
    fn a_unit_named_px_with_a_scale_is_refused() {
        let labels = vec![0.0; 10];
        assert!(regions(&labels, 2, 5, 1, Some(0.65), Some("px")).is_err());
    }

    #[test]
    fn the_fixture_blob_is_fully_solid() {
        // The regions test above would pass against an empty blob if the
        // labelling were wrong: area would be 0 rows, not 4. This pins the
        // fixture itself.
        let labels = label_matrix(10, 10, |x, y| (5..=6).contains(&x) && (5..=6).contains(&y));
        let t = regions(&labels, 10, 10, 1, None, None).unwrap();
        assert_eq!(t.columns[0].1.len(), 1, "the 2x2 blob must label exactly one region");
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
}
