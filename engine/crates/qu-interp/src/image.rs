//! Qu's raster image primitive (`Value::Image`) and everything this pass's
//! scope covers: an uncompressed file format (BMP, chosen over PPM — see the
//! module-level rationale below), and dependency-free processing (grayscale,
//! resize, convolution filters, crop, flip).
//!
//! **Pixel format decision** (the one open design question this feature
//! needed before any code, per BACKLOG.md's "Image library" entry): 8-bit-
//! per-channel **RGB**, row-major, top-left origin, **no alpha**. Grayscale
//! is not a separate variant — it's just an RGB image where every pixel has
//! R=G=B (`to_grayscale` produces exactly this), which avoids a 3-way
//! RGB/RGBA/Gray branch in every single consumer (`imagesc`/`imshow`/
//! `resize`/filters/`save_image`) for a case that's cheap to represent
//! uniformly instead. RGBA was considered and rejected for this pass:
//! nothing here does compositing/blending (the one thing alpha is for), and
//! neither format this pass actually reads/writes (BMP; PPM was the other
//! candidate) carries an alpha channel to round-trip anyway — adding one now
//! would be dead, unexercised surface area. `pixels.len()` is always exactly
//! `width * height * 3`.
//!
//! **File format decision**: BMP (24-bit, uncompressed `BI_RGB`), not PPM.
//! Both were live options (per BACKLOG.md's wording, "an UNCOMPRESSED format
//! that's trivial to parse correctly by hand"); BMP won for one concrete
//! reason PPM can't match: browsers natively decode `image/bmp` data URIs in
//! an `<img>`/SVG `<image>` element, which is exactly what `imshow` (see
//! `plotting.rs`) needs to place a real loaded/processed image inline in a
//! figure — a PPM data URI would render as nothing in every mainstream
//! browser. Using the same format for on-disk load/save *and* in-figure
//! embedding also means one encoder (`encode_bmp`) serves both jobs, not two.
//!
//! **Explicitly not attempted**: PNG/JPEG decoding. PNG needs a real
//! zlib/DEFLATE implementation plus its chunk format; JPEG needs a full
//! DCT/Huffman decoder — both are substantial, error-prone undertakings on
//! their own, and "add a dependency only at the milestone that needs it"
//! (IMPL.md §7) argues for reaching for a well-vetted decode-only crate
//! *when that milestone is actually prioritized*, not hand-rolling either
//! format speculatively here. Tracked as an explicit open item in
//! BACKLOG.md, not silently implied to be covered by "image support: done."

/// A raster image: row-major RGB, 8 bits per channel. See the module doc for
/// the full pixel-format rationale.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Always exactly `width * height * 3` bytes: `[R,G,B, R,G,B, ...]`,
    /// row-major, top-left origin (row 0 is the top row).
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize, pixels: Vec<u8>) -> Result<Self, String> {
        let expected = width * height * 3;
        if pixels.len() != expected {
            return Err(format!(
                "image pixel buffer must be width*height*3 = {expected} bytes for a {width}x{height} image, got {}",
                pixels.len()
            ));
        }
        Ok(Image { width, height, pixels })
    }

    /// A solid-color image of the given size.
    pub fn filled(width: usize, height: usize, rgb: (u8, u8, u8)) -> Self {
        let mut pixels = Vec::with_capacity(width * height * 3);
        for _ in 0..(width * height) {
            pixels.push(rgb.0);
            pixels.push(rgb.1);
            pixels.push(rgb.2);
        }
        Image { width, height, pixels }
    }

    #[inline]
    pub fn get_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * self.width + x) * 3;
        (self.pixels[i], self.pixels[i + 1], self.pixels[i + 2])
    }

    #[inline]
    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: (u8, u8, u8)) {
        let i = (y * self.width + x) * 3;
        self.pixels[i] = rgb.0;
        self.pixels[i + 1] = rgb.1;
        self.pixels[i + 2] = rgb.2;
    }

    /// RGB -> grayscale via the ITU-R BT.601 luma weights (the same
    /// `0.299/0.587/0.114` broadcast-luma coefficients `plotting.rs`'s
    /// `contrast_text_color` already uses elsewhere in this codebase for
    /// perceived-brightness — not a guess, the standard weights, chosen over
    /// BT.709's `0.2126/0.7152/0.0722` for consistency with that existing
    /// use). Returns a full RGB image with R=G=B, not a separate 1-channel
    /// type — see the module doc for why.
    pub fn to_grayscale(&self) -> Image {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for chunk in self.pixels.chunks_exact(3) {
            let (r, g, b) = (chunk[0] as f64, chunk[1] as f64, chunk[2] as f64);
            let y = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
            pixels.push(y);
            pixels.push(y);
            pixels.push(y);
        }
        Image { width: self.width, height: self.height, pixels }
    }

    /// Nearest-neighbor resize: each destination pixel samples the source
    /// pixel whose center is closest, via a simple scaled-index mapping
    /// (`(dst + 0.5) * src_len/dst_len - 0.5`, clamped) — cheap, and exact
    /// (no interpolation artifacts) for both up- and down-sampling.
    pub fn resize_nearest(&self, new_width: usize, new_height: usize) -> Image {
        if new_width == 0 || new_height == 0 {
            return Image { width: new_width, height: new_height, pixels: Vec::new() };
        }
        let mut pixels = vec![0u8; new_width * new_height * 3];
        let x_scale = self.width as f64 / new_width as f64;
        let y_scale = self.height as f64 / new_height as f64;
        for ny in 0..new_height {
            let sy = (((ny as f64 + 0.5) * y_scale - 0.5).round() as isize).clamp(0, self.height as isize - 1) as usize;
            for nx in 0..new_width {
                let sx = (((nx as f64 + 0.5) * x_scale - 0.5).round() as isize).clamp(0, self.width as isize - 1) as usize;
                let (r, g, b) = self.get_pixel(sx, sy);
                let di = (ny * new_width + nx) * 3;
                pixels[di] = r;
                pixels[di + 1] = g;
                pixels[di + 2] = b;
            }
        }
        Image { width: new_width, height: new_height, pixels }
    }

    /// Bilinear resize: each destination pixel is a weighted blend of the 4
    /// nearest source pixels, standard separable-in-effect bilinear
    /// interpolation (same source-coordinate mapping as `resize_nearest`,
    /// just interpolated instead of snapped).
    pub fn resize_bilinear(&self, new_width: usize, new_height: usize) -> Image {
        if new_width == 0 || new_height == 0 {
            return Image { width: new_width, height: new_height, pixels: Vec::new() };
        }
        if self.width == 0 || self.height == 0 {
            return Image::filled(new_width, new_height, (0, 0, 0));
        }
        let mut pixels = vec![0u8; new_width * new_height * 3];
        let x_scale = self.width as f64 / new_width as f64;
        let y_scale = self.height as f64 / new_height as f64;
        for ny in 0..new_height {
            let sy = ((ny as f64 + 0.5) * y_scale - 0.5).clamp(0.0, self.height as f64 - 1.0);
            let y0 = sy.floor() as usize;
            let y1 = (y0 + 1).min(self.height - 1);
            let fy = sy - y0 as f64;
            for nx in 0..new_width {
                let sx = ((nx as f64 + 0.5) * x_scale - 0.5).clamp(0.0, self.width as f64 - 1.0);
                let x0 = sx.floor() as usize;
                let x1 = (x0 + 1).min(self.width - 1);
                let fx = sx - x0 as f64;
                let p00 = self.get_pixel(x0, y0);
                let p10 = self.get_pixel(x1, y0);
                let p01 = self.get_pixel(x0, y1);
                let p11 = self.get_pixel(x1, y1);
                let di = (ny * new_width + nx) * 3;
                let (r, g, b) = bilinear_blend(p00, p10, p01, p11, fx, fy);
                pixels[di] = r;
                pixels[di + 1] = g;
                pixels[di + 2] = b;
            }
        }
        Image { width: new_width, height: new_height, pixels }
    }

    /// Direct 2-D convolution with a square, odd-sized kernel (`k`x`k`,
    /// `kernel.len() == k*k`, row-major), applied independently per channel.
    /// Out-of-bounds taps clamp to the nearest edge pixel ("replicate"
    /// border handling — the simplest border rule that doesn't invent fake
    /// black/zero pixels at the frame, which would visibly darken edges).
    /// `qu_core::transforms::conv` is 1-D only (a single real/complex signal,
    /// not a 2-D pixel grid) and doesn't extend to this shape naturally, so
    /// this is a direct, small, from-scratch 2-D convolution rather than a
    /// forced reuse — per BACKLOG's own "don't over-engineer this part."
    pub fn convolve(&self, kernel: &[f64], k: usize) -> Result<Image, String> {
        if k == 0 || k % 2 == 0 {
            return Err(format!("convolution kernel size must be odd and positive, got {k}"));
        }
        if kernel.len() != k * k {
            return Err(format!("convolution kernel must have exactly k*k = {} entries, got {}", k * k, kernel.len()));
        }
        let radius = (k / 2) as isize;
        let (w, h) = (self.width as isize, self.height as isize);
        let mut out = vec![0u8; self.pixels.len()];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f64; 3];
                for ky in -radius..=radius {
                    let sy = (y + ky).clamp(0, h - 1) as usize;
                    for kx in -radius..=radius {
                        let sx = (x + kx).clamp(0, w - 1) as usize;
                        let kv = kernel[((ky + radius) as usize) * k + (kx + radius) as usize];
                        let (r, g, b) = self.get_pixel(sx, sy);
                        acc[0] += r as f64 * kv;
                        acc[1] += g as f64 * kv;
                        acc[2] += b as f64 * kv;
                    }
                }
                let di = (y as usize * self.width + x as usize) * 3;
                for c in 0..3 {
                    out[di + c] = acc[c].round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Ok(Image { width: self.width, height: self.height, pixels: out })
    }

    /// 3x3 box blur (uniform 1/9 average) — one of the two required
    /// convolution filters.
    pub fn blur3x3(&self) -> Image {
        let k = [1.0 / 9.0; 9];
        self.convolve(&k, 3).expect("3x3 kernel is always valid")
    }

    /// 3x3 unsharp-mask sharpen kernel (`[0,-1,0; -1,5,-1; 0,-1,0]`) — the
    /// other required convolution filter.
    pub fn sharpen3x3(&self) -> Image {
        #[rustfmt::skip]
        let k = [
             0.0, -1.0,  0.0,
            -1.0,  5.0, -1.0,
             0.0, -1.0,  0.0,
        ];
        self.convolve(&k, 3).expect("3x3 kernel is always valid")
    }

    /// 3x3 discrete Laplacian edge-detect kernel (`[0,-1,0; -1,4,-1; 0,-1,0]`)
    /// — a bonus third filter beyond the two required.
    pub fn edge_detect3x3(&self) -> Image {
        #[rustfmt::skip]
        let k = [
             0.0, -1.0,  0.0,
            -1.0,  4.0, -1.0,
             0.0, -1.0,  0.0,
        ];
        self.convolve(&k, 3).expect("3x3 kernel is always valid")
    }

    pub fn crop(&self, x0: usize, y0: usize, w: usize, h: usize) -> Result<Image, String> {
        if w == 0 || h == 0 {
            return Err("crop: width and height must be positive".to_string());
        }
        if x0 + w > self.width || y0 + h > self.height {
            return Err(format!(
                "crop: region ({x0},{y0}) {w}x{h} doesn't fit inside a {}x{} image",
                self.width, self.height
            ));
        }
        let mut pixels = Vec::with_capacity(w * h * 3);
        for y in y0..y0 + h {
            let row_start = (y * self.width + x0) * 3;
            pixels.extend_from_slice(&self.pixels[row_start..row_start + w * 3]);
        }
        Ok(Image { width: w, height: h, pixels })
    }

    pub fn flip_horizontal(&self) -> Image {
        let mut pixels = vec![0u8; self.pixels.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                let (r, g, b) = self.get_pixel(x, y);
                let di = (y * self.width + (self.width - 1 - x)) * 3;
                pixels[di] = r;
                pixels[di + 1] = g;
                pixels[di + 2] = b;
            }
        }
        Image { width: self.width, height: self.height, pixels }
    }

    pub fn flip_vertical(&self) -> Image {
        let mut pixels = vec![0u8; self.pixels.len()];
        for y in 0..self.height {
            let src_start = y * self.width * 3;
            let dst_row = self.height - 1 - y;
            let dst_start = dst_row * self.width * 3;
            pixels[dst_start..dst_start + self.width * 3].copy_from_slice(&self.pixels[src_start..src_start + self.width * 3]);
        }
        Image { width: self.width, height: self.height, pixels }
    }

    // ---- Binary morphology, connected-component labeling, and
    // foreground/background separation (§ blob-manipulation pass,
    // 2026-08-24) ----
    //
    // **Binary-mask convention** (a design call this pass had to make: no
    // `threshold`/`otsu_threshold` builtin exists yet in this shared working
    // tree, so there's no pre-established producer to match). A "binary"
    // image here is an ordinary `Value::Image` where every pixel is
    // grayscale (R=G=B, the same convention `to_grayscale`/
    // `image_from_matrix` already use), with exactly two intended levels:
    // 0 = background, 255 = foreground. `is_foreground` reads the R channel
    // (assumed equal to G and B) and treats >=128 as foreground, so a mask
    // that isn't pixel-exact 0/255 (e.g. post-resize antialiasing) still
    // behaves sanely instead of requiring bit-exact values. This keeps
    // binary masks directly `imshow`-able and round-trippable through
    // `save_image`/`load_image` with zero new `Value` variant, and is the
    // natural convention for a sibling `threshold(img, level)` builtin to
    // land on too (documented in IMPL.md for reconciliation either way).

    /// Foreground test for the binary-mask convention above.
    #[inline]
    pub fn is_foreground(&self, x: usize, y: usize) -> bool {
        self.get_pixel(x, y).0 >= 128
    }

    /// Grayscale morphological erosion: each output pixel (per channel,
    /// independently) is the MINIMUM over a square window of the given
    /// `radius` (side `2*radius+1` — a square structuring element, not a
    /// disk; documented choice, simpler to specify and test exactly).
    /// Applied to a genuine binary (0/255) mask this reduces to textbook
    /// binary erosion (shrinks foreground regions, eats thin/small ones
    /// entirely once they're smaller than the structuring element). Border
    /// handling: replicate (clamp to the nearest edge pixel), the same
    /// convention `convolve` already uses elsewhere in this module — not
    /// "assume background outside the frame," which would also be a
    /// defensible choice but would make border behavior depend on which
    /// operation (erode vs dilate) is being applied. `radius=0` is a no-op.
    pub fn erode(&self, radius: usize) -> Image {
        self.morph_filter(radius, false)
    }

    /// Grayscale morphological dilation: MAXIMUM over the same square
    /// window `erode` uses. On a binary mask this grows foreground regions
    /// and fills small background gaps. See `erode`'s doc comment for the
    /// shared structuring-element and border conventions.
    pub fn dilate(&self, radius: usize) -> Image {
        self.morph_filter(radius, true)
    }

    fn morph_filter(&self, radius: usize, is_dilate: bool) -> Image {
        if radius == 0 || self.width == 0 || self.height == 0 {
            return self.clone();
        }
        let r = radius as isize;
        let (w, h) = (self.width as isize, self.height as isize);
        let mut out = vec![0u8; self.pixels.len()];
        for y in 0..h {
            for x in 0..w {
                let mut acc = if is_dilate { [0u8; 3] } else { [255u8; 3] };
                for dy in -r..=r {
                    let sy = (y + dy).clamp(0, h - 1) as usize;
                    for dx in -r..=r {
                        let sx = (x + dx).clamp(0, w - 1) as usize;
                        let (pr, pg, pb) = self.get_pixel(sx, sy);
                        if is_dilate {
                            acc[0] = acc[0].max(pr);
                            acc[1] = acc[1].max(pg);
                            acc[2] = acc[2].max(pb);
                        } else {
                            acc[0] = acc[0].min(pr);
                            acc[1] = acc[1].min(pg);
                            acc[2] = acc[2].min(pb);
                        }
                    }
                }
                let di = (y as usize * self.width + x as usize) * 3;
                out[di] = acc[0];
                out[di + 1] = acc[1];
                out[di + 2] = acc[2];
            }
        }
        Image { width: self.width, height: self.height, pixels: out }
    }

    /// Opening: erode then dilate — removes small foreground specks/thin
    /// protrusions without shrinking the surviving larger regions overall.
    pub fn open(&self, radius: usize) -> Image {
        self.erode(radius).dilate(radius)
    }

    /// Closing: dilate then erode — fills small background holes/gaps
    /// without growing the surviving regions overall.
    pub fn close(&self, radius: usize) -> Image {
        self.dilate(radius).erode(radius)
    }

    /// Top-hat: original minus its own opening. For grayscale morphology,
    /// opening is pixel-wise <= the original everywhere, so this is always
    /// non-negative (`saturating_sub` is defensive, not load-bearing) —
    /// isolates small bright features / corrects a slowly-varying bright
    /// background, the primary foreground/background separation mechanism
    /// this pass adds (see `foreground_mask` for the composed workflow).
    pub fn tophat(&self, radius: usize) -> Image {
        let opened = self.open(radius);
        subtract_clamped(self, &opened)
    }

    /// Bottom-hat: closing minus the original — the dark-feature dual of
    /// `tophat` (closing is pixel-wise >= the original everywhere).
    pub fn bothat(&self, radius: usize) -> Image {
        let closed = self.close(radius);
        subtract_clamped(&closed, self)
    }

    /// Connected-component labeling (flood fill via an explicit stack, not
    /// recursion, so a large connected blob can't overflow the call stack).
    /// `connectivity` must be 4 (edge-adjacent only) or 8 (edge+diagonal).
    /// Returns a row-major `width*height` label buffer (0 = background,
    /// `1..=count` = blob ids, assigned in raster-scan discovery order) and
    /// the blob count.
    pub fn label_components(&self, connectivity: u8) -> Result<(Vec<u32>, u32), String> {
        if connectivity != 4 && connectivity != 8 {
            return Err(format!("connectivity must be 4 or 8, got {connectivity}"));
        }
        let (w, h) = (self.width, self.height);
        let mut labels = vec![0u32; w * h];
        let mut next_label = 0u32;
        let neighbors: &[(isize, isize)] = if connectivity == 4 {
            &[(-1, 0), (1, 0), (0, -1), (0, 1)]
        } else {
            &[(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
        };
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for y0 in 0..h {
            for x0 in 0..w {
                let idx0 = y0 * w + x0;
                if labels[idx0] != 0 || !self.is_foreground(x0, y0) {
                    continue;
                }
                next_label += 1;
                labels[idx0] = next_label;
                stack.push((x0, y0));
                while let Some((cx, cy)) = stack.pop() {
                    for (dx, dy) in neighbors {
                        let nx = cx as isize + dx;
                        let ny = cy as isize + dy;
                        if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                            continue;
                        }
                        let (nx, ny) = (nx as usize, ny as usize);
                        let nidx = ny * w + nx;
                        if labels[nidx] == 0 && self.is_foreground(nx, ny) {
                            labels[nidx] = next_label;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        Ok((labels, next_label))
    }

    // ---- Contrast/intensity adjustment, arbitrary-angle rotation, and
    // synthetic noise (§ contrast/rotation/noise pass, 2026-08-24). These
    // complete the "im*" set beyond morphology/thresholding.
    // `equalize`/`adjust`/`histogram` all key off the same BT.601 luma
    // `to_grayscale` already established elsewhere in this module (not a
    // new luma convention); `rotate` reuses the corner-interpolation math
    // `resize_bilinear` already established, factored out into the free
    // function `bilinear_blend` below so `rotate` doesn't reimplement it;
    // the two `add_*_noise` methods take an injected `next_uniform` draw
    // source rather than owning an RNG themselves, keeping this module
    // dependency-free of `qu-interp`'s own `Rng` (which lives one layer up,
    // threaded through the shared `seed=` convention every stochastic
    // builtin in this codebase already uses).

    /// Global histogram equalization, **RGB-preserving via luma remapping**
    /// (the higher-quality option the task called out, not the simpler
    /// grayscale-only fallback): computes each pixel's BT.601 luma, builds
    /// its 256-bin histogram -> cumulative distribution function, remaps
    /// luma through the standard normalized CDF lookup table (subtracting
    /// the CDF's minimum *non-zero* value before scaling to `[0,255]` — the
    /// textbook fix for "low-count histograms wash out," the same
    /// normalization OpenCV's `equalizeHist`/skimage's `equalize_hist` use),
    /// then rescales each channel by the ratio `new_luma / old_luma` so
    /// hue/saturation are preserved. A pure-black pixel (`old_luma == 0`)
    /// has an undefined ratio and maps directly to the new luma value as a
    /// gray pixel instead — it carried no color to preserve anyway. On an
    /// already-grayscale image (R=G=B) this reduces to plain single-channel
    /// equalization, since the same ratio applies to all three equal
    /// channels.
    pub fn equalize(&self) -> Image {
        let n = self.width * self.height;
        if n == 0 {
            return self.clone();
        }
        let mut luma = vec![0u8; n];
        let mut hist = [0u32; 256];
        for (i, chunk) in self.pixels.chunks_exact(3).enumerate() {
            let (r, g, b) = (chunk[0] as f64, chunk[1] as f64, chunk[2] as f64);
            let y = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
            luma[i] = y;
            hist[y as usize] += 1;
        }
        let mut cdf = [0u32; 256];
        let mut running = 0u32;
        for (i, &h) in hist.iter().enumerate() {
            running += h;
            cdf[i] = running;
        }
        let cdf_min = cdf.iter().copied().find(|&v| v > 0).unwrap_or(0) as f64;
        let denom = n as f64 - cdf_min;
        let mut lut = [0u8; 256];
        for (i, slot) in lut.iter_mut().enumerate() {
            *slot = if denom > 0.0 {
                (((cdf[i] as f64 - cdf_min) / denom) * 255.0).round().clamp(0.0, 255.0) as u8
            } else {
                i as u8 // every pixel shares one luma value: nothing to spread out
            };
        }
        let mut pixels = vec![0u8; self.pixels.len()];
        for (i, chunk) in self.pixels.chunks_exact(3).enumerate() {
            let old_y = luma[i];
            let new_y = lut[old_y as usize] as f64;
            let di = i * 3;
            if old_y == 0 {
                let v = new_y.round().clamp(0.0, 255.0) as u8;
                pixels[di] = v;
                pixels[di + 1] = v;
                pixels[di + 2] = v;
            } else {
                let ratio = new_y / old_y as f64;
                pixels[di] = (chunk[0] as f64 * ratio).round().clamp(0.0, 255.0) as u8;
                pixels[di + 1] = (chunk[1] as f64 * ratio).round().clamp(0.0, 255.0) as u8;
                pixels[di + 2] = (chunk[2] as f64 * ratio).round().clamp(0.0, 255.0) as u8;
            }
        }
        Image { width: self.width, height: self.height, pixels }
    }

    /// Linear (or gamma-corrected) contrast stretch, MATLAB `imadjust`
    /// semantics: remaps `[in_low, in_high]` (normalized `[0,1]`) to
    /// `[out_low, out_high]`, with `gamma` applied to the clamped/stretched
    /// `[0,1]` value before the final output scale — `out = out_low +
    /// (out_high-out_low) * clamp((x-in_low)/(in_high-in_low), 0, 1)^gamma`.
    /// Applied identically and **independently to each of R/G/B** (one
    /// 256-entry lookup table, reused for all three channels) — MATLAB's
    /// own documented behavior for a truecolor `imadjust(RGB, ...)` call,
    /// and deliberately NOT luma-based (unlike `equalize` above): a
    /// per-channel stretch is what actually fixes a crushed-black or
    /// blown-out-white color channel, where a luma-only stretch would
    /// leave an individually clipped channel untouched.
    pub fn adjust(&self, in_low: f64, in_high: f64, out_low: f64, out_high: f64, gamma: f64) -> Image {
        let denom = (in_high - in_low).max(1e-9);
        let mut lut = [0u8; 256];
        for (v, slot) in lut.iter_mut().enumerate() {
            let x = v as f64 / 255.0;
            let stretched = ((x - in_low) / denom).clamp(0.0, 1.0);
            let gammaed = stretched.powf(gamma);
            let out = out_low + gammaed * (out_high - out_low);
            *slot = (out * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        let pixels = self.pixels.iter().map(|&p| lut[p as usize]).collect();
        Image { width: self.width, height: self.height, pixels }
    }

    /// Raw per-bin BT.601-luma intensity histogram (same luma convention as
    /// `equalize`), `n_bins` evenly-spaced bins over `[0,256)` — a pure
    /// data-extraction step (counts, not a rendered plot); pipe the result
    /// into this codebase's own `bar`/`hist` builtins to actually display
    /// it.
    pub fn histogram(&self, n_bins: usize) -> Vec<f64> {
        let mut hist = vec![0.0f64; n_bins.max(1)];
        let scale = n_bins as f64 / 256.0;
        for chunk in self.pixels.chunks_exact(3) {
            let (r, g, b) = (chunk[0] as f64, chunk[1] as f64, chunk[2] as f64);
            let y = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0);
            let bin = ((y * scale) as usize).min(hist.len() - 1);
            hist[bin] += 1.0;
        }
        hist
    }

    /// Arbitrary-angle rotation about the image center, `angle_degrees`
    /// **counterclockwise** (positive = CCW, matching MATLAB's own
    /// `imrotate` convention — image coordinates, y increasing downward).
    /// `expand`: `true` grows the output canvas to fit the full rotated
    /// content (MATLAB's actual default, `'loose'` — see the `qu-interp`
    /// builtin wiring this calls into for why that, not `'crop'`, is the
    /// default picked here); `false` keeps the input's own width/height,
    /// cropping whatever rotates outside the frame. Pixels sampled from
    /// outside the source image (unavoidable at a non-90-degree angle, and
    /// at any angle when `expand=false`) fill as pure black — this pixel
    /// format has no alpha channel to mark them transparent instead (see
    /// the module doc's pixel-format rationale).
    pub fn rotate(&self, angle_degrees: f64, method: &str, expand: bool) -> Result<Image, String> {
        if method != "nearest" && method != "bilinear" {
            return Err(format!("rotate: unknown method `{method}` — use \"nearest\" or \"bilinear\""));
        }
        if self.width == 0 || self.height == 0 {
            return Ok(self.clone());
        }
        // Negated so a positive `angle_degrees` is CCW: the per-pixel loop
        // below finds each DESTINATION pixel's SOURCE coordinate via the
        // inverse rotation (by `-angle_degrees`), and negating up front
        // keeps that inverse-rotation math reading like a plain forward
        // rotation by `theta`.
        let theta = -angle_degrees.to_radians();
        let (cos_t, sin_t) = (theta.cos(), theta.sin());
        let (w, h) = (self.width as f64, self.height as f64);
        let (new_w, new_h) = if expand {
            let bw = (w * cos_t.abs() + h * sin_t.abs()).round().max(1.0) as usize;
            let bh = (w * sin_t.abs() + h * cos_t.abs()).round().max(1.0) as usize;
            (bw, bh)
        } else {
            (self.width, self.height)
        };
        let mut pixels = vec![0u8; new_w * new_h * 3];
        let src_cx = (w - 1.0) / 2.0;
        let src_cy = (h - 1.0) / 2.0;
        let dst_cx = (new_w as f64 - 1.0) / 2.0;
        let dst_cy = (new_h as f64 - 1.0) / 2.0;
        for ny in 0..new_h {
            for nx in 0..new_w {
                let ddx = nx as f64 - dst_cx;
                let ddy = ny as f64 - dst_cy;
                let odx = ddx * cos_t + ddy * sin_t;
                let ody = -ddx * sin_t + ddy * cos_t;
                let sx = odx + src_cx;
                let sy = ody + src_cy;
                if sx < -0.5 || sy < -0.5 || sx > w - 0.5 || sy > h - 0.5 {
                    continue; // outside the source frame: leave black
                }
                let sx = sx.clamp(0.0, w - 1.0);
                let sy = sy.clamp(0.0, h - 1.0);
                let (r, g, b) = if method == "nearest" {
                    self.get_pixel(sx.round() as usize, sy.round() as usize)
                } else {
                    self.bilinear_at(sx, sy)
                };
                let di = (ny * new_w + nx) * 3;
                pixels[di] = r;
                pixels[di + 1] = g;
                pixels[di + 2] = b;
            }
        }
        Ok(Image { width: new_w, height: new_h, pixels })
    }

    /// Bilinear sample at a continuous pixel-index coordinate already
    /// clamped to `[0, width-1] x [0, height-1]` by the caller (the same
    /// "integer = pixel center" convention `resize_bilinear` computes
    /// internally) — shares `bilinear_blend`'s corner-interpolation math
    /// with `resize_bilinear` instead of `rotate` reimplementing it.
    fn bilinear_at(&self, sx: f64, sy: f64) -> (u8, u8, u8) {
        let x0 = sx.floor() as usize;
        let y0 = sy.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let fx = sx - x0 as f64;
        let fy = sy - y0 as f64;
        bilinear_blend(self.get_pixel(x0, y0), self.get_pixel(x1, y0), self.get_pixel(x0, y1), self.get_pixel(x1, y1), fx, fy)
    }

    /// Adds independent-per-channel additive Gaussian noise, `mean`/`sigma`
    /// in normalized `[0,1]` intensity units (scaled by 255 internally).
    /// `next_uniform` must yield a fresh `[0,1)` uniform draw each call —
    /// the actual PRNG lives one layer up in `qu-interp`'s `Rng`
    /// (`seed=`-driven, per this codebase's shared stochastic-builtin
    /// convention), so this stays agnostic to which generator produced the
    /// stream and is directly unit-testable with a hand-fed deterministic
    /// sequence.
    pub fn add_gaussian_noise(&self, mean: f64, sigma: f64, mut next_uniform: impl FnMut() -> f64) -> Image {
        let (mean_255, sigma_255) = (mean * 255.0, sigma * 255.0);
        let mut pixels = vec![0u8; self.pixels.len()];
        for (i, &p) in self.pixels.iter().enumerate() {
            let u1 = next_uniform().max(1e-12);
            let u2 = next_uniform();
            let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            pixels[i] = (p as f64 + mean_255 + sigma_255 * z).round().clamp(0.0, 255.0) as u8;
        }
        Image { width: self.width, height: self.height, pixels }
    }

    /// Adds salt-and-pepper noise: `density` is the fraction of PIXELS (not
    /// individual channel samples) flipped, split evenly between pure black
    /// ("pepper") and pure white ("salt") — the classic definition, and why
    /// this draws once per pixel and sets all 3 channels together, unlike
    /// `add_gaussian_noise`'s independent per-channel draws.
    pub fn add_salt_pepper_noise(&self, density: f64, mut next_uniform: impl FnMut() -> f64) -> Image {
        let mut pixels = self.pixels.clone();
        for chunk in pixels.chunks_exact_mut(3) {
            let u = next_uniform();
            if u < density / 2.0 {
                chunk.fill(0);
            } else if u < density {
                chunk.fill(255);
            }
        }
        Image { width: self.width, height: self.height, pixels }
    }

    /// Below this many OUTPUT pixels, rayon dispatch overhead isn't worth
    /// it — plain serial row processing. Same "generously chosen, not
    /// per-machine-tuned" reasoning as `qu_core::matrix::Matrix::matmul`'s
    /// own `PARALLEL_MATMUL_THRESHOLD` and this crate's own
    /// `PARALLEL_RNG_THRESHOLD` (`lib.rs`).
    const PARALLEL_WARP_THRESHOLD: usize = 1 << 14;

    /// Applies an arbitrary 3x3 affine transform (`imwarp`'s Rust-level
    /// core — see `Affine3`'s doc comment for the point-transform
    /// convention) in ONE resampling pass, via INVERSE mapping: for each
    /// OUTPUT pixel, `m`'s inverse locates the SOURCE coordinate, which is
    /// then sampled — the standard, hole-free way to do this. Forward
    /// mapping (walking source pixels through `m` and splatting them into
    /// the output) is deliberately not implemented: it leaves gaps
    /// wherever the mapping isn't locally 1-to-1 (any upscale, or any
    /// non-axis-aligned rotation). `m`'s inverse is computed exactly ONCE,
    /// before the pixel loop — not per pixel.
    ///
    /// `loose`: `true` expands the output canvas to the bounding box of the
    /// four transformed INPUT corners (generalizing `imrotate`'s own
    /// `bbox="loose"` from pure rotation to an arbitrary affine — get this
    /// right for a combined rotate+scale, not just rotation: the four
    /// corners are transformed by the FULL `m`, not decomposed into
    /// separate rotate/scale bounding boxes). `false` keeps the input's own
    /// width/height, applying `m` directly in the input's own coordinate
    /// frame — content that maps outside that frame is cropped (matching
    /// MATLAB `imtranslate`'s default `'OutputView','same'` behavior, the
    /// natural "same size" reading of a warp).
    ///
    /// `fill`: an RGB triple painted into every output pixel whose
    /// inverse-mapped source coordinate falls outside the source image
    /// (there being no alpha channel to mark it transparent instead — see
    /// the module doc's pixel-format rationale).
    ///
    /// **The hot loop**: parallelized over output ROWS via rayon's
    /// `par_chunks_mut` once `new_w * new_h` clears
    /// [`Self::PARALLEL_WARP_THRESHOLD`] — each row is an independent,
    /// non-overlapping `&mut [u8]` slice (`chunks_mut`/`par_chunks_mut`
    /// give disjoint slices by construction, same reasoning
    /// `Matrix::matmul`'s own per-column parallelization doc comment
    /// gives), so no locking or shared mutable state is needed per pixel.
    pub fn warp(&self, m: &Affine3, method: &str, loose: bool, fill: (u8, u8, u8)) -> Result<Image, String> {
        if method != "nearest" && method != "bilinear" {
            return Err(format!("imwarp: unknown method `{method}` — use \"nearest\" or \"bilinear\""));
        }
        if self.width == 0 || self.height == 0 {
            return Ok(self.clone());
        }
        let inv = m.inverse()?;
        let (w, h) = (self.width as f64, self.height as f64);
        let (new_w, new_h, off_x, off_y) = if loose {
            // The four INPUT corners (pixel-center convention, matching
            // `rotate`'s own `src_cx = (w-1)/2` — consistent across this
            // module), transformed by the FORWARD `m`.
            let corners = [(0.0, 0.0), (w - 1.0, 0.0), (0.0, h - 1.0), (w - 1.0, h - 1.0)];
            let pts: Vec<(f64, f64)> = corners.iter().map(|&(x, y)| m.apply(x, y)).collect();
            let min_x = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let min_y = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let max_y = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let nw = ((max_x - min_x).round().max(0.0) as usize) + 1;
            let nh = ((max_y - min_y).round().max(0.0) as usize) + 1;
            (nw, nh, min_x, min_y)
        } else {
            (self.width, self.height, 0.0, 0.0)
        };
        let mut pixels = vec![0u8; new_w * new_h * 3];
        if fill != (0, 0, 0) {
            for chunk in pixels.chunks_exact_mut(3) {
                chunk[0] = fill.0;
                chunk[1] = fill.1;
                chunk[2] = fill.2;
            }
        }
        let row_stride = new_w * 3;
        let compute_row = |ny: usize, row: &mut [u8]| {
            let oy = ny as f64 + off_y;
            for nx in 0..new_w {
                let ox = nx as f64 + off_x;
                let (sx, sy) = inv.apply(ox, oy);
                if sx < -0.5 || sy < -0.5 || sx > w - 0.5 || sy > h - 0.5 {
                    continue; // outside the source frame: leave the pre-filled `fill` color
                }
                let sxc = sx.clamp(0.0, w - 1.0);
                let syc = sy.clamp(0.0, h - 1.0);
                let (r, g, b) = if method == "nearest" {
                    self.get_pixel(sxc.round() as usize, syc.round() as usize)
                } else {
                    self.bilinear_at(sxc, syc)
                };
                let di = nx * 3;
                row[di] = r;
                row[di + 1] = g;
                row[di + 2] = b;
            }
        };
        if new_w.saturating_mul(new_h) >= Self::PARALLEL_WARP_THRESHOLD {
            use rayon::prelude::*;
            pixels.par_chunks_mut(row_stride).enumerate().for_each(|(ny, row)| compute_row(ny, row));
        } else {
            for (ny, row) in pixels.chunks_exact_mut(row_stride).enumerate() {
                compute_row(ny, row);
            }
        }
        Ok(Image { width: new_w, height: new_h, pixels })
    }
}

/// A 3x3 affine (or general projective) transform in standard 2D
/// homogeneous-coordinate form: a point transforms as `M @ [x, y, 1]^T`
/// (row-major storage — `0[i][j]` is row `i`, column `j`). This is the type
/// `imwarp`'s inverse-mapping core (`Image::warp`, above) consumes;
/// `qu-interp`'s `lib.rs` builds one from a `Value::Mat` (`affine_translate`/
/// `affine_scale`/`affine_rotate`/`affine_shear`/`affine_identity`, or any
/// 3x3 the user composed via Qu's own `*`/`matmul`) via `matrix_to_affine3`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3(pub [[f64; 3]; 3]);

impl Affine3 {
    pub fn identity() -> Self {
        Affine3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    }

    /// Applies this transform to a point `(x, y)`. Divides through by the
    /// homogeneous `w` component when it isn't exactly `1` or `0` (supports
    /// a genuinely projective 3x3, not just the affine ones this feature's
    /// builders produce, whose bottom row is always `[0, 0, 1]` and so
    /// never need the division).
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.0;
        let xw = m[0][0] * x + m[0][1] * y + m[0][2];
        let yw = m[1][0] * x + m[1][1] * y + m[1][2];
        let w = m[2][0] * x + m[2][1] * y + m[2][2];
        if w != 1.0 && w != 0.0 {
            (xw / w, yw / w)
        } else {
            (xw, yw)
        }
    }

    /// General 3x3 inverse via the closed-form cofactor/adjugate formula
    /// (no iterative solver needed for a fixed 3x3 — and `imwarp` only ever
    /// needs to invert once, up front, not per pixel). Errors on a singular
    /// matrix (e.g. a zero scale factor collapsing a dimension) with a
    /// clear message, rather than silently returning `Inf`/`NaN` samples —
    /// same "never silently wrong" discipline `matrix_inverse` in `lib.rs`
    /// already follows for the general N x N case.
    pub fn inverse(&self) -> Result<Affine3, String> {
        let m = &self.0;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if det.abs() < 1e-12 {
            return Err("imwarp: transform matrix is singular and cannot be inverted".to_string());
        }
        let inv_det = 1.0 / det;
        let mut r = [[0.0; 3]; 3];
        r[0][0] = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det;
        r[0][1] = (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det;
        r[0][2] = (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det;
        r[1][0] = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det;
        r[1][1] = (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det;
        r[1][2] = (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det;
        r[2][0] = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det;
        r[2][1] = (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det;
        r[2][2] = (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det;
        Ok(Affine3(r))
    }
}

/// Shared bilinear interpolation of 4 corner RGB samples given fractional
/// offsets `fx`/`fy` in `[0,1]` — the core math behind both
/// `resize_bilinear` and `rotate`'s `"bilinear"` method, factored out so
/// `rotate` reuses it exactly rather than reimplementing it from scratch.
fn bilinear_blend(p00: (u8, u8, u8), p10: (u8, u8, u8), p01: (u8, u8, u8), p11: (u8, u8, u8), fx: f64, fy: f64) -> (u8, u8, u8) {
    let lerp = |a: u8, b: u8, c: u8, d: u8| -> u8 {
        let top = a as f64 * (1.0 - fx) + b as f64 * fx;
        let bot = c as f64 * (1.0 - fx) + d as f64 * fx;
        (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8
    };
    (lerp(p00.0, p10.0, p01.0, p11.0), lerp(p00.1, p10.1, p01.1, p11.1), lerp(p00.2, p10.2, p01.2, p11.2))
}

/// Per-channel saturating subtraction (`a - b`, floored at 0) — used by
/// `tophat`/`bothat`. A free function (not a method) since it takes two
/// same-shape images rather than mutating either.
fn subtract_clamped(a: &Image, b: &Image) -> Image {
    let mut out = vec![0u8; a.pixels.len()];
    for i in 0..a.pixels.len() {
        out[i] = a.pixels[i].saturating_sub(b.pixels[i]);
    }
    Image { width: a.width, height: a.height, pixels: out }
}

/// Encodes `width x height` row-major RGB pixels as a 24-bit uncompressed
/// BMP (`BITMAPFILEHEADER` + `BITMAPINFOHEADER`, `BI_RGB`) — the exact
/// classic-Windows-bitmap layout `decode_bmp` reads back. Rows are written
/// bottom-up (positive `height` in the DIB header, the conventional BMP row
/// order) and padded to a 4-byte boundary, both per the format spec, not a
/// simplification.
pub fn encode_bmp(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    debug_assert_eq!(rgb.len(), width * height * 3);
    let row_size = (width * 3).div_ceil(4) * 4;
    let pixel_data_size = row_size * height;
    let file_size = 54 + pixel_data_size;
    let mut data = Vec::with_capacity(file_size);

    // BITMAPFILEHEADER (14 bytes)
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&(file_size as u32).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // reserved1
    data.extend_from_slice(&0u16.to_le_bytes()); // reserved2
    data.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset

    // BITMAPINFOHEADER (40 bytes)
    data.extend_from_slice(&40u32.to_le_bytes()); // header size
    data.extend_from_slice(&(width as i32).to_le_bytes());
    data.extend_from_slice(&(height as i32).to_le_bytes()); // positive => bottom-up
    data.extend_from_slice(&1u16.to_le_bytes()); // color planes
    data.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    data.extend_from_slice(&0u32.to_le_bytes()); // compression: BI_RGB
    data.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    data.extend_from_slice(&2835i32.to_le_bytes()); // ~72 DPI
    data.extend_from_slice(&2835i32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes()); // colors in palette (none, 24bpp)
    data.extend_from_slice(&0u32.to_le_bytes()); // important colors

    let pad = row_size - width * 3;
    for y in (0..height).rev() {
        let row_start = y * width * 3;
        for x in 0..width {
            let i = row_start + x * 3;
            // BMP stores BGR, not RGB.
            data.push(rgb[i + 2]);
            data.push(rgb[i + 1]);
            data.push(rgb[i]);
        }
        data.extend(std::iter::repeat_n(0u8, pad));
    }
    data
}

/// Decodes a 24-bit uncompressed BMP into an [`Image`]. Deliberately narrow
/// (matches exactly what `encode_bmp` writes, plus the equally common
/// top-down variant): rejects any compressed, palette-indexed, or non-24bpp
/// BMP with a clear error rather than guessing — a wrong silent decode of a
/// real-world BMP (5/6/5, RLE, 8-bit palette, ...) would be worse than an
/// honest "not supported."
pub fn decode_bmp(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 54 {
        return Err("not a valid BMP file (shorter than the minimum 54-byte header)".to_string());
    }
    if &bytes[0..2] != b"BM" {
        return Err("not a BMP file (missing the 'BM' magic bytes)".to_string());
    }
    let pixel_offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let dib_size = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
    if dib_size < 40 {
        return Err(format!("unsupported BMP: DIB header must be BITMAPINFOHEADER (40 bytes), got {dib_size}"));
    }
    let width_raw = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
    let height_raw = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
    let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());
    if compression != 0 {
        return Err(format!("unsupported BMP: only uncompressed BI_RGB is supported (compression code {compression})"));
    }
    if bpp != 24 {
        return Err(format!("unsupported BMP: only 24-bit RGB is supported, found {bpp}-bit"));
    }
    if width_raw <= 0 {
        return Err(format!("invalid BMP width {width_raw}"));
    }
    if height_raw == 0 {
        return Err("invalid BMP height 0".to_string());
    }
    let width = width_raw as usize;
    let top_down = height_raw < 0;
    let height = height_raw.unsigned_abs() as usize;
    let row_size = (width * 3).div_ceil(4) * 4;
    let mut pixels = vec![0u8; width * height * 3];
    for row in 0..height {
        // BMP rows are bottom-up unless the height is negative.
        let src_row = if top_down { row } else { height - 1 - row };
        let start = pixel_offset + src_row * row_size;
        if start + width * 3 > bytes.len() {
            return Err("BMP file is truncated (pixel data runs past the end of the file)".to_string());
        }
        for x in 0..width {
            let si = start + x * 3;
            let di = (row * width + x) * 3;
            // BMP stores BGR, not RGB.
            pixels[di] = bytes[si + 2];
            pixels[di + 1] = bytes[si + 1];
            pixels[di + 2] = bytes[si];
        }
    }
    Ok(Image { width, height, pixels })
}

// ---------------------------------------------------------------- PNG encode
//
// BACKLOG item [25] ("PNG + PDF export backends, hand-rolled, no new deps")
// scope split: this half is PNG for `Value::Image` raster content, encoded
// with zero compression-library help. Full vector-figure PNG export (a real
// 2D rasterizer: scanline polygon fill, anti-aliased lines, bitmap font
// rendering for axis labels/legends) is explicitly NOT attempted — see the
// `savefig` PDF/PNG dispatch in `qu-interp/src/lib.rs`, which gives
// `savefig(fig, "x.png")` a clear "not yet supported" error instead of a
// silently blank/broken file. That's a separate, much larger future item.
//
// The trick that makes a dependency-free *encoder* tractable (unlike a
// decoder for arbitrary real-world PNGs, which would need real Huffman/LZ77):
// PNG's IDAT chunk just needs to be valid zlib/DEFLATE data, and DEFLATE's
// spec includes an uncompressed "stored block" mode (block type `00`) that's
// pure byte-copying plus a 4-byte length header — no Huffman tree, no LZ77
// match-finding. The files this produces are larger than a real PNG encoder
// would make (no actual compression happens), but every byte is spec-correct
// and any PNG-reading tool decodes it identically to a compressed one.

/// CRC-32 (ISO 3309 / ITU-T V.42, the exact variant PNG's own spec mandates
/// for every chunk's trailing checksum) — bitwise, not table-based. A table
/// would be the usual speed optimization, but nothing here is hot enough to
/// need it, and bitwise is fewer lines to get right and verify against the
/// standard `crc32("123456789") == 0xCBF43926` check value (see the test
/// below) — the same public conformance vector any CRC-32 implementation
/// checks itself against.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// Adler-32 (RFC 1950's own checksum, wrapped around every zlib stream —
/// distinct from, and simpler than, PNG's per-chunk CRC-32 above). Verified
/// below against the standard worked example (`adler32("Wikipedia") ==
/// 0x11E60398`).
fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

/// Wraps raw bytes in a minimal zlib stream (RFC 1950 header + RFC 1951
/// DEFLATE data + Adler-32 trailer) using only DEFLATE's uncompressed
/// "stored block" mode — see the module doc above for why that's enough for
/// a spec-correct (if uncompressed) PNG `IDAT` payload.
///
/// zlib header: `CMF=0x78` (compression method 8 = deflate, window size
/// 32K), `FLG=0x01` chosen so `(0x78 << 8 | FLG) % 31 == 0` as RFC 1950
/// requires (`0x7801 == 30721 == 31 * 991`) with `FDICT=0` (no preset
/// dictionary) and `FLEVEL=0` (fastest/no compression — accurate, since
/// there isn't any).
///
/// Each stored block's own header is one byte (`BFINAL` in bit 0, `BTYPE=00`
/// in bits 1-2, rest zero-padding) because a stored block always starts
/// byte-aligned; DEFLATE's 16-bit length field caps each block at 65535
/// bytes, so longer input is split across multiple blocks with `BFINAL=1`
/// only on the last one.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 16);
    out.push(0x78);
    out.push(0x01);
    let mut i = 0;
    loop {
        let remaining = data.len() - i;
        let block_len = remaining.min(65535);
        let is_final = i + block_len >= data.len();
        out.push(if is_final { 1 } else { 0 });
        out.extend_from_slice(&(block_len as u16).to_le_bytes());
        out.extend_from_slice(&(!(block_len as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + block_len]);
        i += block_len;
        if is_final {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Writes one length-prefixed, CRC-32-checked PNG chunk (`length | type |
/// data | crc`, all per spec — `length`/`crc` are big-endian, `crc` covers
/// `type` and `data` but not `length`).
fn write_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Encodes `width x height` row-major RGB pixels (the exact [`Image`] pixel
/// layout) as a real, spec-correct 24-bit-truecolor PNG — signature, `IHDR`,
/// one `IDAT` (uncompressed per the module doc above), `IEND`. No alpha
/// channel (color type 2, matching `Image`'s own no-alpha decision), no
/// interlacing, filter type `0` (None) on every scanline since there's no
/// compression pass that would benefit from a smarter filter here.
pub fn encode_png(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    debug_assert_eq!(rgb.len(), width * height * 3);
    // Raw (pre-zlib) scanline data: one filter-type byte (0 = None) followed
    // by that row's raw RGB bytes, repeated per row — exactly what PNG's
    // `IDAT` payload decompresses to for a non-interlaced truecolor image.
    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for y in 0..height {
        raw.push(0u8);
        let row_start = y * width * 3;
        raw.extend_from_slice(&rgb[row_start..row_start + width * 3]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: truecolor (RGB, no alpha)
    ihdr.push(0); // compression method (always 0)
    ihdr.push(0); // filter method (always 0)
    ihdr.push(0); // interlace method: none
    write_png_chunk(&mut out, b"IHDR", &ihdr);

    write_png_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    write_png_chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    /// The round trip is the test that matters: Qu could write PNG and not
    /// read it, which is the odd half, and this closes it.
    #[test]
    fn a_png_written_by_qu_reads_back_pixel_for_pixel() {
        let (w, h) = (7usize, 5usize);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                // A pattern with structure in both directions, so a row
                // filter applied to the wrong neighbour would show.
                rgb.extend_from_slice(&[
                    (x * 31) as u8,
                    (y * 47) as u8,
                    ((x * y) % 251) as u8,
                ]);
            }
        }
        let encoded = encode_png(w, h, &rgb);
        let back = decode_png(&encoded).expect("should decode what we just wrote");
        assert_eq!(back.width, w);
        assert_eq!(back.height, h);
        assert_eq!(back.pixels, rgb, "every pixel must survive the round trip");
    }

    /// `decode` sniffs the magic bytes rather than trusting an extension,
    /// because a `.png` that is really a BMP is a thing that happens.
    #[test]
    fn decode_recognises_both_formats_from_their_bytes() {
        let img = Image::filled(3, 2, (10, 20, 30));
        let png = decode(&encode_png(3, 2, &img.pixels)).unwrap();
        assert_eq!(png.get_pixel(1, 1), (10, 20, 30));

        let bmp = decode(&encode_bmp(img.width, img.height, &img.pixels)).unwrap();
        assert_eq!(bmp.get_pixel(1, 1), (10, 20, 30));

        // TIFF is lossless, so it joins the exact-pixel assertions above.
        // JPEG is checked separately below, where the loss is measured
        // rather than asserted away.
        let tiff = decode(&encode_tiff(img.width, img.height, &img.pixels).unwrap()).unwrap();
        assert_eq!(tiff.get_pixel(1, 1), (10, 20, 30));

        let jpeg = decode(&encode_jpeg(img.width, img.height, &img.pixels, 90).unwrap()).unwrap();
        assert_eq!((jpeg.width, jpeg.height), (3, 2));

        let err = decode(b"not an image at all").unwrap_err();
        assert!(err.contains("PNG, a BMP, a JPEG or a TIFF"), "got {err}");
    }

    /// The four formats are told apart by their magic bytes alone, which
    /// is what `decode` actually dispatches on.
    #[test]
    fn jpeg_and_tiff_are_sniffed_from_their_magic_bytes() {
        let img = Image::filled(8, 8, (200, 100, 50));
        let jpeg = encode_jpeg(8, 8, &img.pixels, 90).unwrap();
        assert_eq!(&jpeg[0..3], &[0xFF, 0xD8, 0xFF], "bad JPEG SOI marker");
        assert!(is_jpeg(&jpeg));
        assert!(!is_tiff(&jpeg));

        let tif = encode_tiff(8, 8, &img.pixels).unwrap();
        // `tiff`'s encoder writes little-endian, so `II` + magic 42.
        assert_eq!(&tif[0..4], b"II\x2A\x00", "bad TIFF header");
        assert!(is_tiff(&tif));
        assert!(!is_jpeg(&tif));

        // Big-endian TIFF is recognised too, even though we never write it.
        assert!(is_tiff(b"MM\x00\x2A\x00\x00\x00\x08"));
    }

    /// JPEG is LOSSY. This test exists to state that honestly and to
    /// MEASURE it, not to assert a bound somebody guessed: it prints the
    /// real RMS error of a round trip on a non-trivial gradient image and
    /// only then checks it is in a sane range.
    ///
    /// The lower bound matters as much as the upper one -- an RMS of
    /// exactly 0 would mean the encoder had silently become lossless (or,
    /// far more likely, that the test was comparing something against
    /// itself and proving nothing).
    #[test]
    fn jpeg_round_trip_is_close_but_not_exact() {
        // A gradient with a hard edge down the middle: smooth enough that
        // JPEG does well, but with the high-frequency content that makes
        // the loss real and measurable.
        let (w, h) = (64usize, 48usize);
        let mut pixels = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let edge = if x > w / 2 { 60u8 } else { 0u8 };
                pixels.push(((x * 255) / w) as u8);
                pixels.push(((y * 255) / h) as u8);
                pixels.push(128u8.saturating_add(edge));
            }
        }
        let original = Image::new(w, h, pixels).unwrap();

        let bytes = encode_jpeg(w, h, &original.pixels, 90).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!((back.width, back.height), (w, h));

        let sq: f64 = original
            .pixels
            .iter()
            .zip(back.pixels.iter())
            .map(|(a, b)| {
                let d = *a as f64 - *b as f64;
                d * d
            })
            .sum();
        let rms = (sq / original.pixels.len() as f64).sqrt();
        println!("JPEG q90 round-trip RMS error: {rms:.3} levels (out of 255)");

        assert!(rms > 0.0, "a JPEG round trip came back bit-identical -- the encoder is not doing what this test documents");
        assert!(rms < 12.0, "JPEG q90 round-trip RMS was {rms}, far worse than expected");

        // Higher quality must not be WORSE. This is the property a
        // `quality=` argument has to actually have to be worth exposing.
        let hi = decode(&encode_jpeg(w, h, &original.pixels, 100).unwrap()).unwrap();
        let sq_hi: f64 = original
            .pixels
            .iter()
            .zip(hi.pixels.iter())
            .map(|(a, b)| {
                let d = *a as f64 - *b as f64;
                d * d
            })
            .sum();
        let rms_hi = (sq_hi / original.pixels.len() as f64).sqrt();
        println!("JPEG q100 round-trip RMS error: {rms_hi:.3} levels");
        assert!(rms_hi <= rms, "q100 ({rms_hi}) was worse than q90 ({rms})");
    }

    /// TIFF, as this module writes it, is lossless -- so unlike JPEG its
    /// round trip IS pixel-exact, and that difference is worth pinning.
    #[test]
    fn tiff_round_trip_is_pixel_exact() {
        let (w, h) = (17usize, 5usize); // deliberately not a round number
        let mut pixels = Vec::with_capacity(w * h * 3);
        for i in 0..(w * h) {
            pixels.push((i % 256) as u8);
            pixels.push(((i * 7) % 256) as u8);
            pixels.push(((i * 31) % 256) as u8);
        }
        let original = Image::new(w, h, pixels).unwrap();
        let back = decode(&encode_tiff(w, h, &original.pixels).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (w, h));
        assert_eq!(back.pixels, original.pixels, "TIFF round trip lost pixels");
    }

    /// A grayscale JPEG loads as R=G=B rather than erroring, matching the
    /// convention `to_grayscale`/`image_from_matrix` already use.
    #[test]
    fn a_grayscale_jpeg_loads_as_rgb() {
        // Encode a grayscale source through the RGB path (R=G=B in, so the
        // encoder's own colour transform leaves it grey) and confirm the
        // decode keeps all three channels equal.
        let (w, h) = (16usize, 16usize);
        let mut pixels = Vec::with_capacity(w * h * 3);
        for i in 0..(w * h) {
            let v = ((i * 255) / (w * h)) as u8;
            pixels.extend_from_slice(&[v, v, v]);
        }
        let bytes = encode_jpeg(w, h, &pixels, 95).unwrap();
        let back = decode(&bytes).unwrap();
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = back.get_pixel(x, y);
                assert!(
                    r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2,
                    "grey pixel came back coloured at ({x},{y}): {r},{g},{b}"
                );
            }
        }
    }

    /// The format's own 16-bit dimension limit is reported as a limit,
    /// not as a panic or a corrupt file.
    #[test]
    fn an_oversized_jpeg_is_refused_with_a_reason() {
        let err = encode_jpeg(70_000, 1, &[], 90).unwrap_err();
        assert!(err.contains("65535"), "got {err}");
    }

    /// Interlaced PNGs are refused by name rather than decoded wrongly.
    #[test]
    fn an_interlaced_png_is_refused_with_a_reason() {
        let mut png = encode_png(2, 2, &[0; 12]);
        // The interlace flag is the 13th byte of IHDR: 8 signature + 8
        // chunk header + 12.
        png[8 + 8 + 12] = 1;
        let err = decode_png(&png).unwrap_err();
        assert!(err.contains("interlaced"), "got {err}");
    }

    #[test]
    fn a_truncated_or_headerless_png_says_which() {
        let png = encode_png(2, 2, &[0; 12]);
        let err = decode_png(&png[..20]).unwrap_err();
        assert!(err.contains("truncated") || err.contains("no header"), "got {err}");

        let err = decode_png(b"\x89PNG\r\n\x1a\n").unwrap_err();
        assert!(err.contains("no header"), "got {err}");
    }

    /// Paeth is the filter people get subtly wrong. Each expectation below
    /// is worked from the predictor `p = a + b - c` and the three absolute
    /// distances, rather than remembered.
    #[test]
    fn the_paeth_predictor_matches_the_specification() {
        // p = 0: pa = 1, pb = 2, pc = 3 -- `a` is nearest.
        assert_eq!(paeth(1, 2, 3), 1);
        // p = 190: pa = 180, pb = 10, pc = 170 -- `b`.
        assert_eq!(paeth(10, 200, 20), 200);
        // p = 15: pa = 5, pb = 5, pc = 0 -- `c` predicts exactly.
        assert_eq!(paeth(10, 20, 15), 15);
        // All zero: everything ties, and the first clause takes `a`.
        assert_eq!(paeth(0, 0, 0), 0);
        // The tie that actually exercises the ordering: pa = pb = 5 with
        // pc = 10, so the `pa <= pb && pa <= pc` clause must pick `a`. It
        // needs a == b to arise at all -- with a != b, equal pa and pb
        // force c to the midpoint, where pc is 0 and c wins outright.
        assert_eq!(paeth(10, 10, 5), 10);
    }

    use super::*;

    #[test]
    fn bmp_round_trip_is_pixel_exact() {
        // A small synthetic image with distinct rows/columns so a
        // row/column-order bug (e.g. bottom-up vs top-down, or BGR vs RGB)
        // would show up as a wrong-pixel mismatch, not just a wrong size.
        let img = Image::new(
            3,
            2,
            vec![
                255, 0, 0, 0, 255, 0, 0, 0, 255, // row 0: red, green, blue
                255, 255, 0, 0, 255, 255, 255, 0, 255, // row 1: yellow, cyan, magenta
            ],
        )
        .unwrap();
        let bytes = encode_bmp(img.width, img.height, &img.pixels);
        let decoded = decode_bmp(&bytes).unwrap();
        assert_eq!(decoded, img);
    }

    #[test]
    fn bmp_round_trip_odd_width_needs_row_padding() {
        // width*3 = 15, not a multiple of 4 -> exercises the row-padding path.
        let img = Image::filled(5, 3, (12, 200, 77));
        let bytes = encode_bmp(img.width, img.height, &img.pixels);
        let decoded = decode_bmp(&bytes).unwrap();
        assert_eq!(decoded, img);
    }

    #[test]
    fn decode_bmp_rejects_bad_magic() {
        assert!(decode_bmp(&[0u8; 54]).is_err());
    }

    #[test]
    fn decode_bmp_rejects_non_24bpp() {
        let mut bytes = encode_bmp(2, 2, &[0u8; 12]);
        bytes[28] = 32; // claim 32bpp
        bytes[29] = 0;
        assert!(decode_bmp(&bytes).is_err());
    }

    #[test]
    fn grayscale_matches_hand_computed_bt601_luma() {
        // Pure red, green, blue, white — each luma value hand-computed from
        // the BT.601 weights (0.299/0.587/0.114), not just "runs without
        // crashing."
        let img = Image::new(4, 1, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]).unwrap();
        let gray = img.to_grayscale();
        let expect = |v: f64| (v.round().clamp(0.0, 255.0)) as u8;
        assert_eq!(gray.get_pixel(0, 0), (expect(0.299 * 255.0), expect(0.299 * 255.0), expect(0.299 * 255.0)));
        assert_eq!(gray.get_pixel(1, 0), (expect(0.587 * 255.0), expect(0.587 * 255.0), expect(0.587 * 255.0)));
        assert_eq!(gray.get_pixel(2, 0), (expect(0.114 * 255.0), expect(0.114 * 255.0), expect(0.114 * 255.0)));
        assert_eq!(gray.get_pixel(3, 0), (255, 255, 255));
    }

    #[test]
    fn resize_nearest_2x_upsamples_each_pixel_into_a_2x2_block() {
        // A 2x1 image (black, white) upsampled to 4x2 should tile each
        // source pixel into a clean 2x2 block — hand-computable exactly.
        let img = Image::new(2, 1, vec![0, 0, 0, 255, 255, 255]).unwrap();
        let big = img.resize_nearest(4, 2);
        assert_eq!(big.width, 4);
        assert_eq!(big.height, 2);
        for y in 0..2 {
            assert_eq!(big.get_pixel(0, y), (0, 0, 0));
            assert_eq!(big.get_pixel(1, y), (0, 0, 0));
            assert_eq!(big.get_pixel(2, y), (255, 255, 255));
            assert_eq!(big.get_pixel(3, y), (255, 255, 255));
        }
    }

    #[test]
    fn resize_bilinear_midpoint_is_the_average_of_two_source_pixels() {
        // A 2x1 image (black=0, white=255) resized to 3x1: the middle output
        // pixel should land almost exactly halfway between the two source
        // samples — a hand-computable interpolation result, not a guess.
        let img = Image::new(2, 1, vec![0, 0, 0, 255, 255, 255]).unwrap();
        let out = img.resize_bilinear(3, 1);
        let (r, _, _) = out.get_pixel(1, 0);
        assert!((r as i32 - 127).abs() <= 2, "expected ~127, got {r}");
    }

    #[test]
    fn blur_of_a_single_bright_pixel_spreads_to_its_neighbors() {
        // A 5x5 all-black image with one white pixel at the exact center
        // (2,2): after a 3x3 (radius-1) box blur, the center must drop
        // (it's now an average of itself + 8 black neighbors = 255/9 ~ 28),
        // its immediate neighbor above must rise by the same hand-computable
        // amount (it picked up exactly one fractional share of the bright
        // pixel), and a true corner (0,0) — whose radius-1 neighborhood
        // (rows/cols {0,1} only, even after edge-clamping) never reaches
        // row/col 2 at all — must stay exactly untouched. (A 3x3 canvas is
        // too small for this check: at that size *every* pixel's clamped
        // neighborhood wraps around far enough to touch the center, which
        // is what a first draft of this test got wrong.)
        let mut img = Image::filled(5, 5, (0, 0, 0));
        img.set_pixel(2, 2, (255, 255, 255));
        let blurred = img.blur3x3();
        let (center, _, _) = blurred.get_pixel(2, 2);
        let expected_shared = (255.0 / 9.0_f64).round() as u8;
        assert_eq!(center, expected_shared);
        let (corner, _, _) = blurred.get_pixel(0, 0);
        assert_eq!(corner, 0); // out of reach of a radius-1 kernel from (2,2)
        let (edge, _, _) = blurred.get_pixel(2, 1);
        assert_eq!(edge, expected_shared); // directly above the center IS a 3x3 neighbor
    }

    #[test]
    fn crop_extracts_the_exact_subregion() {
        // 3x3 image with a unique value per row so cropping the middle row
        // is unambiguous to check by hand.
        let img = Image::new(
            3,
            3,
            vec![
                1, 1, 1, 2, 2, 2, 3, 3, 3, // row 0
                4, 4, 4, 5, 5, 5, 6, 6, 6, // row 1
                7, 7, 7, 8, 8, 8, 9, 9, 9, // row 2
            ],
        )
        .unwrap();
        let cropped = img.crop(1, 1, 2, 1).unwrap();
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 1);
        assert_eq!(cropped.get_pixel(0, 0), (5, 5, 5));
        assert_eq!(cropped.get_pixel(1, 0), (6, 6, 6));
    }

    #[test]
    fn crop_out_of_bounds_is_a_clear_error() {
        let img = Image::filled(2, 2, (1, 2, 3));
        assert!(img.crop(1, 1, 5, 5).is_err());
    }

    #[test]
    fn flip_horizontal_reverses_columns() {
        let img = Image::new(3, 1, vec![1, 1, 1, 2, 2, 2, 3, 3, 3]).unwrap();
        let flipped = img.flip_horizontal();
        assert_eq!(flipped.get_pixel(0, 0), (3, 3, 3));
        assert_eq!(flipped.get_pixel(1, 0), (2, 2, 2));
        assert_eq!(flipped.get_pixel(2, 0), (1, 1, 1));
    }

    #[test]
    fn flip_vertical_reverses_rows() {
        let img = Image::new(1, 3, vec![1, 1, 1, 2, 2, 2, 3, 3, 3]).unwrap();
        let flipped = img.flip_vertical();
        assert_eq!(flipped.get_pixel(0, 0), (3, 3, 3));
        assert_eq!(flipped.get_pixel(0, 1), (2, 2, 2));
        assert_eq!(flipped.get_pixel(0, 2), (1, 1, 1));
    }

    #[test]
    fn new_rejects_a_mismatched_pixel_buffer_length() {
        assert!(Image::new(2, 2, vec![0, 0, 0]).is_err());
    }

    /// Builds a binary (0/255 grayscale) mask from a `[[bool]]`-style grid,
    /// row-major, for hand-checkable morphology/labeling tests.
    fn binary_from_grid(grid: &[&[u8]]) -> Image {
        let h = grid.len();
        let w = grid[0].len();
        let mut img = Image::filled(w, h, (0, 0, 0));
        for (y, row) in grid.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                let g = if v != 0 { 255 } else { 0 };
                img.set_pixel(x, y, (g, g, g));
            }
        }
        img
    }

    #[test]
    fn erode_radius1_shrinks_a_solid_square_by_exactly_one_ring() {
        // A 5x5 foreground square sitting inside a background frame (not
        // touching the canvas edge, so replicate-border clamping never
        // masks the effect being tested): eroding with radius 1 must strip
        // exactly the outer ring, leaving only the interior 3x3.
        let img = binary_from_grid(&[
            &[0, 0, 0, 0, 0, 0, 0],
            &[0, 1, 1, 1, 1, 1, 0],
            &[0, 1, 1, 1, 1, 1, 0],
            &[0, 1, 1, 1, 1, 1, 0],
            &[0, 1, 1, 1, 1, 1, 0],
            &[0, 1, 1, 1, 1, 1, 0],
            &[0, 0, 0, 0, 0, 0, 0],
        ]);
        let eroded = img.erode(1);
        // Only the interior 3x3 (rows/cols 2..=4) should remain foreground.
        for y in 0..7 {
            for x in 0..7 {
                let expect_fg = (2..=4).contains(&x) && (2..=4).contains(&y);
                assert_eq!(eroded.is_foreground(x, y), expect_fg, "mismatch at ({x},{y})");
            }
        }
    }

    #[test]
    fn dilate_radius1_grows_a_single_pixel_into_a_3x3_block() {
        let mut img = Image::filled(7, 7, (0, 0, 0));
        img.set_pixel(3, 3, (255, 255, 255));
        let dilated = img.dilate(1);
        for y in 0..7 {
            for x in 0..7 {
                let expect_fg = (2..=4).contains(&x) && (2..=4).contains(&y);
                assert_eq!(dilated.is_foreground(x, y), expect_fg, "mismatch at ({x},{y})");
            }
        }
    }

    #[test]
    fn open_removes_a_speck_smaller_than_the_structuring_element() {
        // A single isolated foreground pixel (a "speck") should vanish
        // entirely after opening with radius 1: erosion wipes it out (no
        // 3x3 neighborhood is all-foreground), and dilating background
        // stays background.
        let mut img = Image::filled(9, 9, (0, 0, 0));
        img.set_pixel(4, 4, (255, 255, 255));
        let opened = img.open(1);
        assert!(!opened.pixels.iter().any(|&p| p != 0), "a lone speck must not survive opening");
    }

    #[test]
    fn open_preserves_a_solid_region_at_least_as_large_as_the_structuring_element() {
        let img = binary_from_grid(&[
            &[0, 0, 0, 0, 0],
            &[0, 1, 1, 1, 0],
            &[0, 1, 1, 1, 0],
            &[0, 1, 1, 1, 0],
            &[0, 0, 0, 0, 0],
        ]);
        let opened = img.open(1);
        assert_eq!(opened, img, "a region already >= the structuring element survives opening unchanged");
    }

    #[test]
    fn close_fills_a_hole_smaller_than_the_structuring_element() {
        // A solid 7x7 foreground block with a single background pixel
        // "hole" punched in the middle: closing with radius 1 should fill
        // it back in.
        let mut img = Image::filled(7, 7, (255, 255, 255));
        img.set_pixel(3, 3, (0, 0, 0));
        let closed = img.close(1);
        assert!(closed.is_foreground(3, 3), "closing should fill a small hole");
        assert!(closed.pixels.iter().all(|&p| p == 255), "closing must not erode the surrounding solid block");
    }

    #[test]
    fn tophat_isolates_a_small_bright_speck_on_a_bright_background_plateau() {
        // A large uniform mid-gray background (would be wiped flat by
        // opening) plus one small bright speck: top-hat should recover
        // (something close to) the speck and suppress the flat background
        // to near zero.
        let mut img = Image::filled(9, 9, (100, 100, 100));
        img.set_pixel(4, 4, (200, 200, 200));
        let th = img.tophat(1);
        // Flat background region far from the speck: opening reproduces
        // the background exactly, so top-hat must be exactly 0 there.
        assert_eq!(th.get_pixel(0, 0), (0, 0, 0));
        // At the speck itself, top-hat should be strictly positive (the
        // opening erodes+dilates the lone bright pixel away, so original -
        // opened > 0 there).
        let (v, _, _) = th.get_pixel(4, 4);
        assert!(v > 0, "expected the isolated bright speck to survive top-hat, got {v}");
    }

    #[test]
    fn bothat_isolates_a_small_dark_speck_on_a_dark_background_plateau() {
        let mut img = Image::filled(9, 9, (150, 150, 150));
        img.set_pixel(4, 4, (50, 50, 50));
        let bh = img.bothat(1);
        assert_eq!(bh.get_pixel(0, 0), (0, 0, 0));
        let (v, _, _) = bh.get_pixel(4, 4);
        assert!(v > 0, "expected the isolated dark speck to survive bottom-hat, got {v}");
    }

    #[test]
    fn label_components_counts_two_separate_blobs_with_correct_areas() {
        // Two disjoint foreground blocks (a 2x2 and a 1x3), far enough
        // apart that neither 4- nor 8-connectivity merges them.
        let img = binary_from_grid(&[
            &[1, 1, 0, 0, 0, 0],
            &[1, 1, 0, 0, 0, 0],
            &[0, 0, 0, 0, 0, 0],
            &[0, 0, 0, 1, 1, 1],
            &[0, 0, 0, 0, 0, 0],
        ]);
        let (labels, count) = img.label_components(8).unwrap();
        assert_eq!(count, 2);
        let area_of = |lbl: u32| labels.iter().filter(|&&v| v == lbl).count();
        // Whichever id each blob got (raster-scan order guarantees the
        // top-left 2x2 is discovered first, so it's label 1).
        assert_eq!(area_of(1), 4);
        assert_eq!(area_of(2), 3);
        // Every background pixel stays labeled 0.
        assert_eq!(labels.iter().filter(|&&v| v == 0).count(), 30 - 4 - 3);
    }

    #[test]
    fn label_components_8_connectivity_merges_a_diagonal_touch_that_4_connectivity_keeps_separate() {
        let img = binary_from_grid(&[
            &[1, 0],
            &[0, 1],
        ]);
        let (_, count4) = img.label_components(4).unwrap();
        let (_, count8) = img.label_components(8).unwrap();
        assert_eq!(count4, 2, "4-connectivity must not merge a purely diagonal touch");
        assert_eq!(count8, 1, "8-connectivity must merge a diagonal touch");
    }

    #[test]
    fn label_components_rejects_an_invalid_connectivity() {
        let img = Image::filled(3, 3, (0, 0, 0));
        assert!(img.label_components(6).is_err());
    }

    #[test]
    fn equalize_matches_hand_computed_cdf_lookup_on_a_skewed_grayscale_histogram() {
        // 8 pixels, luma values [0,0,0,0,64,64,128,255]: hist(0)=4, hist(64)=2,
        // hist(128)=1, hist(255)=1 -> cdf(0)=4, cdf(64)=6, cdf(128)=7, cdf(255)=8.
        // cdf_min = 4 (the smallest NON-ZERO cdf value, at luma 0 itself) -
        // subtracting it before scaling is the standard fix this equalize
        // implementation must apply, or lut[0] would come out as
        // round(4/8*255)=127 instead of the correct 0.
        let vals = [0u8, 0, 0, 0, 64, 64, 128, 255];
        let mut pixels = Vec::new();
        for &v in &vals {
            pixels.push(v);
            pixels.push(v);
            pixels.push(v);
        }
        let img = Image::new(8, 1, pixels).unwrap();
        let eq = img.equalize();
        let expect = |v: u8| -> u8 {
            match v {
                0 => 0,
                64 => 128,   // round((6-4)/4*255) = round(127.5) = 128
                128 => 191,  // round((7-4)/4*255) = round(191.25) = 191
                255 => 255,  // round((8-4)/4*255) = 255
                _ => unreachable!(),
            }
        };
        for (i, &v) in vals.iter().enumerate() {
            let (r, g, b) = eq.get_pixel(i, 0);
            let e = expect(v);
            assert_eq!((r, g, b), (e, e, e), "pixel {i} (orig luma {v}) expected {e}, got {r}");
        }
    }

    #[test]
    fn equalize_widens_a_narrow_histogram_spread() {
        // A near-degenerate low-contrast image (all luma packed into
        // [100,110]) must come out with a strictly wider min-max spread
        // after equalization — the actual observable "does this do
        // anything useful" check, not just an internal LUT computation.
        let img = Image::new(6, 1, vec![
            100, 100, 100, 102, 102, 102, 104, 104, 104,
            106, 106, 106, 108, 108, 108, 110, 110, 110,
        ]).unwrap();
        let eq = img.equalize();
        let orig_spread = 110 - 100;
        let (min_v, max_v) = eq.pixels.iter().fold((255u8, 0u8), |(lo, hi), &p| (lo.min(p), hi.max(p)));
        assert!(
            (max_v - min_v) as i32 > orig_spread,
            "expected equalize to widen the spread beyond {orig_spread}, got {}",
            max_v - min_v
        );
    }

    #[test]
    fn equalize_preserves_color_ratio_via_luma_remap_not_grayscale_conversion() {
        // A strongly red-tinted pixel (180:60:30 = 6:2:1) plus filler
        // pixels spreading the luma histogram: after equalization the
        // pixel must still be roughly the SAME hue, not have collapsed to
        // gray (which a luma-only/grayscale-output equalize would do).
        let mut pixels = vec![180, 60, 30];
        for v in [0u8, 40, 90, 140, 200, 255, 255, 255] {
            pixels.push(v);
            pixels.push(v);
            pixels.push(v);
        }
        let img = Image::new(9, 1, pixels).unwrap();
        let eq = img.equalize();
        let (r, g, b) = eq.get_pixel(0, 0);
        assert!(g > 0 && b > 0, "a colored pixel must not collapse to black");
        let new_rg = r as f64 / g as f64;
        let new_gb = g as f64 / b as f64;
        assert!((new_rg - 3.0).abs() < 0.2, "R:G ratio should stay ~3.0, got {new_rg}");
        assert!((new_gb - 2.0).abs() < 0.2, "G:B ratio should stay ~2.0, got {new_gb}");
    }

    #[test]
    fn adjust_stretches_a_narrow_input_range_to_the_full_output_range() {
        let img = Image::new(4, 1, vec![64, 64, 64, 96, 96, 96, 128, 128, 128, 200, 200, 200]).unwrap();
        let out = img.adjust(64.0 / 255.0, 128.0 / 255.0, 0.0, 1.0, 1.0);
        assert_eq!(out.get_pixel(0, 0), (0, 0, 0), "in_low must map to out_low exactly");
        assert_eq!(out.get_pixel(2, 0), (255, 255, 255), "in_high must map to out_high exactly");
        let (mid, _, _) = out.get_pixel(1, 0);
        assert!((mid as i32 - 128).abs() <= 2, "midpoint of the input range should land near 128, got {mid}");
        assert_eq!(out.get_pixel(3, 0), (255, 255, 255), "values above in_high must clamp to out_high");
    }

    #[test]
    fn adjust_gamma_bends_the_midpoint_without_moving_the_endpoints() {
        let img = Image::new(3, 1, vec![0, 0, 0, 128, 128, 128, 255, 255, 255]).unwrap();
        let out = img.adjust(0.0, 1.0, 0.0, 1.0, 2.0);
        assert_eq!(out.get_pixel(0, 0), (0, 0, 0));
        assert_eq!(out.get_pixel(2, 0), (255, 255, 255));
        let (mid, _, _) = out.get_pixel(1, 0);
        // gamma=2 on x=0.5019.. gives ~0.252 -> ~64, well below the linear
        // (gamma=1) midpoint of 128.
        assert!(mid < 90, "gamma=2 should darken the midtone well below linear, got {mid}");
    }

    #[test]
    fn histogram_counts_match_a_hand_computed_luma_distribution() {
        let vals = [0u8, 0, 64, 64, 64, 255];
        let mut pixels = Vec::new();
        for &v in &vals {
            pixels.push(v);
            pixels.push(v);
            pixels.push(v);
        }
        let img = Image::new(6, 1, pixels).unwrap();
        let hist = img.histogram(256);
        assert_eq!(hist.len(), 256);
        assert_eq!(hist[0], 2.0);
        assert_eq!(hist[64], 3.0);
        assert_eq!(hist[255], 1.0);
        assert_eq!(hist.iter().sum::<f64>(), 6.0);
    }

    #[test]
    fn histogram_respects_a_smaller_bin_count() {
        // n_bins=4 over [0,256) -> bin width 64: luma 0 -> bin 0, 100 -> bin 1,
        // 200 -> bin 3.
        let img = Image::new(3, 1, vec![0, 0, 0, 100, 100, 100, 200, 200, 200]).unwrap();
        let hist = img.histogram(4);
        assert_eq!(hist, vec![1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn rotate_90_degrees_matches_the_standard_ccw_rot90_permutation() {
        // 3-wide x 2-tall asymmetric pattern; at an exact 90-degree angle,
        // nearest-neighbor sampling must reproduce the textbook rot90
        // counterclockwise permutation exactly (no interpolation blur to
        // tolerate): [[1,2,3],[4,5,6]] -> [[3,6],[2,5],[1,4]] (numpy's own
        // `np.rot90` convention, matched to double-check against this
        // codebase's own `rot90` builtin cross-checked separately via a
        // real `.qu` script).
        let vals = [1u8, 2, 3, 4, 5, 6];
        let mut pixels = Vec::new();
        for &v in &vals {
            pixels.push(v);
            pixels.push(v);
            pixels.push(v);
        }
        let img = Image::new(3, 2, pixels).unwrap();
        let rotated = img.rotate(90.0, "nearest", true).unwrap();
        assert_eq!((rotated.width, rotated.height), (2, 3));
        let expect = [[3, 6], [2, 5], [1, 4]];
        for y in 0..3 {
            for x in 0..2 {
                let (r, _, _) = rotated.get_pixel(x, y);
                assert_eq!(r, expect[y][x], "mismatch at ({x},{y})");
            }
        }
    }

    #[test]
    fn rotate_expand_bbox_grows_to_fit_a_45_degree_rotation() {
        let img = Image::filled(10, 10, (200, 200, 200));
        let rotated = img.rotate(45.0, "bilinear", true).unwrap();
        let expected = (10.0_f64 * (std::f64::consts::FRAC_PI_4.cos() + std::f64::consts::FRAC_PI_4.sin())).round() as usize;
        assert_eq!(rotated.width, expected);
        assert_eq!(rotated.height, expected);
    }

    #[test]
    fn rotate_crop_mode_keeps_the_original_canvas_size() {
        let img = Image::filled(10, 10, (200, 200, 200));
        let rotated = img.rotate(45.0, "bilinear", false).unwrap();
        assert_eq!((rotated.width, rotated.height), (10, 10));
    }

    #[test]
    fn rotate_rejects_an_unknown_method() {
        let img = Image::filled(2, 2, (0, 0, 0));
        assert!(img.rotate(10.0, "bogus", true).is_err());
    }

    #[test]
    fn gaussian_noise_with_zero_sigma_and_zero_mean_is_a_no_op() {
        let img = Image::filled(4, 4, (100, 150, 200));
        let draws = [0.3, 0.7, 0.1, 0.9, 0.5, 0.5];
        let mut it = draws.iter().cycle().copied();
        let out = img.add_gaussian_noise(0.0, 0.0, || it.next().unwrap());
        assert_eq!(out, img);
    }

    #[test]
    fn gaussian_noise_mean_shifts_every_channel_by_the_expected_amount() {
        // sigma=0 isolates the deterministic mean-shift term (the
        // Box-Muller `z` term is multiplied by sigma_255=0, so whatever
        // `next_uniform` returns can't matter): mean=0.1 (25.5/255) must
        // add exactly round(25.5)=26 to every channel.
        let img = Image::filled(2, 2, (100, 100, 100));
        let out = img.add_gaussian_noise(0.1, 0.0, || 0.5);
        let (r, g, b) = out.get_pixel(0, 0);
        let expected = (100.0 + 0.1 * 255.0_f64).round() as u8;
        assert_eq!((r, g, b), (expected, expected, expected));
    }

    #[test]
    fn salt_pepper_flips_a_fraction_of_pixels_deterministically() {
        // 4 pixels, density=0.5 -> draws below 0.25 turn black ("pepper"),
        // draws in [0.25,0.5) turn white ("salt"), everything else at or
        // above 0.5 is untouched.
        let img = Image::filled(4, 1, (120, 120, 120));
        let draws = [0.1, 0.3, 0.6, 0.9];
        let mut it = draws.iter().copied();
        let out = img.add_salt_pepper_noise(0.5, || it.next().unwrap());
        assert_eq!(out.get_pixel(0, 0), (0, 0, 0));
        assert_eq!(out.get_pixel(1, 0), (255, 255, 255));
        assert_eq!(out.get_pixel(2, 0), (120, 120, 120));
        assert_eq!(out.get_pixel(3, 0), (120, 120, 120));
    }

    // ---- Affine3 / imwarp (§ affine/imwarp pass, 2026-08-24) ----

    #[test]
    fn affine3_identity_leaves_a_point_unchanged() {
        let (x, y) = Affine3::identity().apply(3.5, -2.0);
        assert_eq!((x, y), (3.5, -2.0));
    }

    #[test]
    fn affine3_translate_shifts_a_point_by_dx_dy() {
        let m = Affine3([[1.0, 0.0, 5.0], [0.0, 1.0, -3.0], [0.0, 0.0, 1.0]]);
        assert_eq!(m.apply(1.0, 1.0), (6.0, -2.0));
    }

    #[test]
    fn affine3_rotate_90_degrees_maps_unit_x_to_unit_y() {
        // Hand-verified: R(90 deg) = [[0,-1,0],[1,0,0],[0,0,1]], so (1,0) -> (0,1).
        let theta = std::f64::consts::FRAC_PI_2;
        let (s, c) = theta.sin_cos();
        let m = Affine3([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]);
        let (x, y) = m.apply(1.0, 0.0);
        assert!((x - 0.0).abs() < 1e-9, "x should be ~0, got {x}");
        assert!((y - 1.0).abs() < 1e-9, "y should be ~1, got {y}");
    }

    #[test]
    fn affine3_inverse_round_trips_a_point_through_a_composed_transform() {
        // Translate(2,3) composed with Scale(2, 0.5): compose as
        // (T then S applied via matrix product T*S is NOT what's tested
        // here directly -- this just checks a single matrix's own inverse
        // undoes it, independent of composition order).
        let m = Affine3([[2.0, 0.0, 2.0], [0.0, 0.5, 3.0], [0.0, 0.0, 1.0]]);
        let inv = m.inverse().unwrap();
        let (x, y) = m.apply(7.0, -4.0);
        let (rx, ry) = inv.apply(x, y);
        assert!((rx - 7.0).abs() < 1e-9, "expected x round-trip to 7.0, got {rx}");
        assert!((ry - (-4.0)).abs() < 1e-9, "expected y round-trip to -4.0, got {ry}");
    }

    #[test]
    fn affine3_inverse_rejects_a_singular_matrix() {
        // Zero x-scale collapses a whole dimension -> not invertible.
        let m = Affine3([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(m.inverse().is_err());
    }

    #[test]
    fn warp_identity_matrix_crop_mode_is_a_pixel_exact_round_trip() {
        let img = Image::new(
            3,
            2,
            vec![
                1, 1, 1, 2, 2, 2, 3, 3, 3, //
                4, 4, 4, 5, 5, 5, 6, 6, 6,
            ],
        )
        .unwrap();
        let out = img.warp(&Affine3::identity(), "nearest", false, (0, 0, 0)).unwrap();
        assert_eq!(out, img, "identity warp in crop mode must reproduce the input exactly");
    }

    #[test]
    fn warp_identity_matrix_loose_mode_matches_crop_size_when_already_axis_aligned() {
        // An identity transform's own corner bounding box is exactly the
        // original frame, so `loose` and `crop` must agree in size here
        // (loose only grows the canvas when content actually moves outside
        // the original frame).
        let img = Image::filled(6, 4, (10, 20, 30));
        let out = img.warp(&Affine3::identity(), "nearest", true, (0, 0, 0)).unwrap();
        assert_eq!((out.width, out.height), (6, 4));
    }

    #[test]
    fn warp_translation_shifts_content_and_fills_the_uncovered_region() {
        // A single bright pixel at (1,1) in a 5x5 black image, translated by
        // (+2, 0): with a "crop"-mode warp, the bright pixel must land at
        // (3,1), and an untouched fill-region pixel must show the requested
        // fill color exactly.
        let mut img = Image::filled(5, 5, (0, 0, 0));
        img.set_pixel(1, 1, (255, 255, 255));
        let m = Affine3([[1.0, 0.0, 2.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let out = img.warp(&m, "nearest", false, (7, 8, 9)).unwrap();
        assert_eq!((out.width, out.height), (5, 5));
        assert_eq!(out.get_pixel(3, 1), (255, 255, 255), "the bright pixel should have moved to (3,1)");
        assert_eq!(out.get_pixel(0, 0), (7, 8, 9), "an out-of-source-bounds pixel must show the fill color");
    }

    #[test]
    fn warp_loose_bbox_of_a_45_degree_rotation_matches_rotate_expand_size() {
        // Cross-check against the already-shipped, independently-implemented
        // `rotate(..., expand=true)`: a pure-rotation `imwarp` (loose bbox)
        // must compute the same output canvas size `rotate` does for the
        // same angle, since both are supposed to bound the same rotated
        // square.
        let img = Image::filled(10, 10, (200, 200, 200));
        let rotated_via_rotate = img.rotate(45.0, "bilinear", true).unwrap();
        let theta = 45.0_f64.to_radians();
        let (s, c) = theta.sin_cos();
        // `rotate`'s own CCW-positive convention negates internally; mirror
        // that sign here so the two bounding boxes describe the same
        // physical rotation.
        let m = Affine3([[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]);
        let warped = img.warp(&m, "bilinear", true, (0, 0, 0)).unwrap();
        assert_eq!(warped.width, rotated_via_rotate.width);
        assert_eq!(warped.height, rotated_via_rotate.height);
    }

    #[test]
    fn warp_rejects_an_unknown_method() {
        let img = Image::filled(2, 2, (0, 0, 0));
        assert!(img.warp(&Affine3::identity(), "bogus", true, (0, 0, 0)).is_err());
    }

    #[test]
    fn warp_rejects_a_singular_transform() {
        let img = Image::filled(2, 2, (0, 0, 0));
        let singular = Affine3([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(img.warp(&singular, "nearest", true, (0, 0, 0)).is_err());
    }

    #[test]
    fn warp_parallel_path_matches_serial_path_above_the_threshold() {
        // A synthetic image big enough (>= PARALLEL_WARP_THRESHOLD output
        // pixels) to force the rayon `par_chunks_mut` path, checked
        // pixel-for-pixel against a forced-serial run of the exact same
        // transform -- proves the parallel row split changes nothing about
        // the actual output.
        let w = 200usize;
        let h = 200usize; // 40_000 >= 1<<14 (16_384): exercises the parallel path
        let mut pixels = vec![0u8; w * h * 3];
        for (i, chunk) in pixels.chunks_exact_mut(3).enumerate() {
            let v = (i % 256) as u8;
            chunk[0] = v;
            chunk[1] = v.wrapping_add(50);
            chunk[2] = v.wrapping_add(100);
        }
        let img = Image::new(w, h, pixels).unwrap();
        let m = Affine3([[1.2, 0.1, 3.0], [-0.1, 0.9, -2.0], [0.0, 0.0, 1.0]]);
        let parallel_out = img.warp(&m, "bilinear", true, (5, 6, 7)).unwrap();

        // Force the serial path by shrinking below the threshold impossible
        // without touching the constant, so instead we recompute serially
        // here with the identical per-pixel math and compare.
        let inv = m.inverse().unwrap();
        let (fw, fh) = (w as f64, h as f64);
        let corners = [(0.0, 0.0), (fw - 1.0, 0.0), (0.0, fh - 1.0), (fw - 1.0, fh - 1.0)];
        let pts: Vec<(f64, f64)> = corners.iter().map(|&(x, y)| m.apply(x, y)).collect();
        let min_x = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        let new_w = ((max_x - min_x).round().max(0.0) as usize) + 1;
        let new_h = ((max_y - min_y).round().max(0.0) as usize) + 1;
        assert_eq!((parallel_out.width, parallel_out.height), (new_w, new_h));
        let mut serial_pixels = vec![0u8; new_w * new_h * 3];
        for chunk in serial_pixels.chunks_exact_mut(3) {
            chunk[0] = 5;
            chunk[1] = 6;
            chunk[2] = 7;
        }
        for ny in 0..new_h {
            let oy = ny as f64 + min_y;
            for nx in 0..new_w {
                let ox = nx as f64 + min_x;
                let (sx, sy) = inv.apply(ox, oy);
                if sx < -0.5 || sy < -0.5 || sx > fw - 0.5 || sy > fh - 0.5 {
                    continue;
                }
                let sxc = sx.clamp(0.0, fw - 1.0);
                let syc = sy.clamp(0.0, fh - 1.0);
                let (r, g, b) = img.bilinear_at(sxc, syc);
                let di = (ny * new_w + nx) * 3;
                serial_pixels[di] = r;
                serial_pixels[di + 1] = g;
                serial_pixels[di + 2] = b;
            }
        }
        assert_eq!(parallel_out.pixels, serial_pixels, "parallel and serial row processing must produce identical output");
    }

    // ------------------------------------------------------------ PNG export

    #[test]
    fn crc32_matches_the_standard_conformance_vector() {
        // The canonical check value every CRC-32 (this exact ISO 3309/PNG
        // variant) implementation is verified against.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn crc32_of_empty_input_is_zero() {
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn adler32_matches_the_standard_worked_example() {
        // The standard "Wikipedia" worked example from RFC 1950's own
        // checksum (independently verifiable by hand: a=1+sum(bytes) mod
        // 65521, b=running sum of a's mod 65521, result = b<<16 | a).
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn adler32_of_empty_input_is_one() {
        assert_eq!(adler32(b""), 1);
    }

    /// A minimal, test-only PNG *decoder* — deliberately narrow (only
    /// understands what `encode_png` above actually emits: one IHDR, IDAT
    /// chunks holding nothing but stored deflate blocks, filter type 0 on
    /// every row, no interlacing). This is NOT a general PNG decoder (that
    /// remains the explicitly out-of-scope item this module's own doc
    /// comment calls out) — it exists purely so the round-trip test below
    /// verifies real pixel-exact decoding, not just "the bytes look
    /// PNG-shaped."
    fn test_decode_png(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
        assert_eq!(&bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A], "bad PNG signature");
        let mut pos = 8;
        let mut width = 0usize;
        let mut height = 0usize;
        let mut idat: Vec<u8> = Vec::new();
        loop {
            let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &bytes[pos + 4..pos + 8];
            let data = &bytes[pos + 8..pos + 8 + len];
            let crc_stored = u32::from_be_bytes(bytes[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            let mut crc_input = Vec::with_capacity(4 + len);
            crc_input.extend_from_slice(kind);
            crc_input.extend_from_slice(data);
            assert_eq!(crc32(&crc_input), crc_stored, "bad chunk CRC for {:?}", std::str::from_utf8(kind));
            match kind {
                b"IHDR" => {
                    width = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
                    height = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
                    assert_eq!(data[8], 8, "expected 8-bit depth");
                    assert_eq!(data[9], 2, "expected truecolor (no alpha) color type");
                }
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => break,
                other => panic!("unexpected chunk {:?}", std::str::from_utf8(other)),
            }
            pos += 12 + len;
        }
        // zlib header.
        assert_eq!(idat[0], 0x78);
        assert_eq!(idat[1], 0x01);
        // Walk the stored deflate blocks, reassembling the raw bytes.
        let mut raw = Vec::new();
        let mut p = 2;
        loop {
            let bfinal = idat[p] & 1;
            let btype = (idat[p] >> 1) & 0b11;
            assert_eq!(btype, 0, "test_decode_png only understands stored (uncompressed) blocks");
            let block_len = u16::from_le_bytes(idat[p + 1..p + 3].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes(idat[p + 3..p + 5].try_into().unwrap());
            assert_eq!(nlen, !(block_len as u16), "LEN/NLEN one's-complement mismatch");
            raw.extend_from_slice(&idat[p + 5..p + 5 + block_len]);
            p += 5 + block_len;
            if bfinal == 1 {
                break;
            }
        }
        let adler_stored = u32::from_be_bytes(idat[p..p + 4].try_into().unwrap());
        assert_eq!(adler32(&raw), adler_stored, "Adler-32 trailer mismatch");
        // Un-filter: every row in `encode_png`'s output uses filter type 0
        // (None), so this is just stripping the leading filter-type byte.
        let mut pixels = Vec::with_capacity(width * height * 3);
        let row_bytes = width * 3;
        for row in raw.chunks_exact(1 + row_bytes) {
            assert_eq!(row[0], 0, "test_decode_png only understands filter type 0 (None)");
            pixels.extend_from_slice(&row[1..]);
        }
        (width, height, pixels)
    }

    #[test]
    fn png_round_trip_is_pixel_exact() {
        let img = Image::new(
            3,
            2,
            vec![
                255, 0, 0, 0, 255, 0, 0, 0, 255, // row 0: red, green, blue
                255, 255, 0, 0, 255, 255, 255, 0, 255, // row 1: yellow, cyan, magenta
            ],
        )
        .unwrap();
        let bytes = encode_png(img.width, img.height, &img.pixels);
        let (w, h, pixels) = test_decode_png(&bytes);
        assert_eq!((w, h), (img.width, img.height));
        assert_eq!(pixels, img.pixels);
    }

    #[test]
    fn png_round_trip_survives_a_stored_block_boundary() {
        // DEFLATE's stored-block length field caps a block at 65535 bytes,
        // so an image whose raw (filter-byte-included) scanline data
        // exceeds that must split across multiple blocks -- exercise that
        // path specifically, not just the single-block common case.
        let width = 200;
        let height = 200; // raw size = 200*(1+600) = 120200 bytes > 65535
        let mut pixels = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                pixels.push((x % 256) as u8);
                pixels.push((y % 256) as u8);
                pixels.push(((x + y) % 256) as u8);
            }
        }
        let bytes = encode_png(width, height, &pixels);
        let (w, h, decoded) = test_decode_png(&bytes);
        assert_eq!((w, h), (width, height));
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn png_signature_and_chunk_order_are_well_formed() {
        let img = Image::filled(4, 4, (10, 20, 30));
        let bytes = encode_png(img.width, img.height, &img.pixels);
        assert_eq!(&bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(&bytes[bytes.len() - 8..bytes.len() - 4], b"IEND");
    }
}

// ── PNG ────────────────────────────────────────────────────────────────
//
// Qu could WRITE png and read only BMP, which is the odd half: you could
// produce an image and not open it again, and every image anyone actually
// has is a PNG.
//
// The decoder is here rather than pulled in because the expensive part was
// already written: `inflate.rs` carries a full DEFLATE/zlib implementation,
// built for `.mat` files. What was left is chunk parsing and the five row
// filters, which is the part of PNG that is genuinely small.
//
// Supported: bit depths 1, 2, 4, 8 and 16, and colour types 0 (grey),
// 2 (truecolour), 3 (palette), 4 (grey+alpha) and 6 (RGBA). Alpha is
// composited onto white rather than kept, because `Image` is three
// channels; that is stated here and in the error path rather than left for
// someone to discover from a figure.
//
// Not supported: Adam7 interlacing, which is rare, and which would double
// the size of this for images almost nobody produces any more. It is
// refused by name rather than decoded wrongly.

/// Decode an image, sniffing the format from its first bytes.
///
/// Magic numbers rather than the file extension: a `.png` that is really a
/// BMP is a thing that happens, and the bytes are never wrong.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        decode_png(bytes)
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else if is_jpeg(bytes) {
        decode_jpeg(bytes)
    } else if is_tiff(bytes) {
        decode_tiff(bytes)
    } else {
        Err(format!(
            "not an image this build can read -- expected a PNG, a BMP, a JPEG or a TIFF, \
             and the file begins {:02x?}",
            &bytes[..bytes.len().min(4)]
        ))
    }
}

// ── JPEG and TIFF ──────────────────────────────────────────────────────
//
// Unlike PNG and BMP above, these two are NOT hand-rolled. IMPL.md §7 --
// "call vendor libraries for a real FORMAT, don't hand-roll it" -- and
// both are squarely that: JPEG is Huffman tables, the DCT, chroma
// subsampling and progressive scan interleaving; TIFF is an IFD tag
// database with strip/tile layouts and half a dozen compression schemes.
// PNG and BMP are hand-rolled here only because the expensive part
// (DEFLATE, in `inflate.rs`) already existed for `.mat` files.
//
// **JPEG is lossy.** A `save_image` to `.jpg` followed by `load_image`
// does NOT return the pixels that went in, and nothing in this module
// pretends otherwise -- see `jpeg_round_trip_is_close_but_not_exact` in
// the tests below, which measures the actual RMS error rather than
// asserting a bound nobody checked. TIFF as written here is lossless
// (uncompressed RGB8 strips), so its round trip IS pixel-exact, and that
// is tested separately.

/// JPEG's SOI marker, followed by the first byte of the next marker.
///
/// `FF D8 FF` rather than just `FF D8`: every real JPEG begins with a
/// marker segment immediately after SOI, and the third byte keeps a
/// two-byte coincidence in some other format's header from being handed
/// to the JPEG decoder.
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// TIFF's byte-order mark plus its magic number, in both endiannesses.
///
/// `II` is little-endian and `MM` big-endian (Intel/Motorola); the magic
/// is 42 either way. BigTIFF (magic 43) is matched here too so it reaches
/// the decoder and gets that crate's own error rather than this module's
/// generic "not an image" -- which would be the wrong message for a file
/// that genuinely is a TIFF.
fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II\x2A\x00")
        || bytes.starts_with(b"MM\x00\x2A")
        || bytes.starts_with(b"II\x2B\x00")
        || bytes.starts_with(b"MM\x00\x2B")
}

/// Decode a baseline or progressive JPEG into RGB.
///
/// Grayscale JPEGs are widened to R=G=B, the same convention
/// `to_grayscale`/`image_from_matrix` already use, so a grayscale photo
/// loads as an ordinary `Image` rather than erroring.
///
/// 16-bit and CMYK JPEGs are refused BY NAME rather than converted. CMYK
/// in particular carries an Adobe APP14 transform flag whose inverted/
/// non-inverted convention is easy to get backwards, and a silently
/// colour-inverted photo is exactly the "worse than an error" outcome
/// `decode_bmp` above refuses for the same reason.
pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    use jpeg_decoder::PixelFormat;
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    let data = decoder.decode().map_err(|err| format!("could not decode this JPEG: {err}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| "this JPEG decoded but carried no frame header".to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 {
        return Err(format!("invalid JPEG dimensions {width}x{height}"));
    }
    let pixels = match info.pixel_format {
        PixelFormat::RGB24 => data,
        PixelFormat::L8 => {
            let mut rgb = Vec::with_capacity(width * height * 3);
            for luma in data {
                rgb.push(luma);
                rgb.push(luma);
                rgb.push(luma);
            }
            rgb
        }
        PixelFormat::L16 => {
            return Err(
                "unsupported JPEG: 16-bit grayscale, which this build does not convert \
                 (Image is 8-bit RGB)"
                    .to_string(),
            )
        }
        PixelFormat::CMYK32 => {
            return Err(
                "unsupported JPEG: CMYK, which this build refuses rather than guess at \
                 the Adobe inversion convention and hand back colour-inverted pixels"
                    .to_string(),
            )
        }
    };
    Image::new(width, height, pixels)
}

/// Encode RGB as a baseline JPEG at the given quality (1-100).
///
/// Lossy: the bytes this produces do not decode back to `rgb`. Quality
/// 90 is `save_image`'s default -- visually near-transparent on
/// photographs while still a large size win over PNG.
pub fn encode_jpeg(width: usize, height: usize, rgb: &[u8], quality: u8) -> Result<Vec<u8>, String> {
    use jpeg_encoder::{ColorType, Encoder};
    // JPEG stores each dimension in 16 bits in its SOF header, so this is
    // a hard format limit, not an arbitrary cap.
    if width == 0 || height == 0 || width > u16::MAX as usize || height > u16::MAX as usize {
        return Err(format!(
            "cannot write a {width}x{height} image as JPEG: the format stores each \
             dimension in 16 bits, so both must be between 1 and 65535"
        ));
    }
    let quality = quality.clamp(1, 100);
    let mut out: Vec<u8> = Vec::new();
    let encoder = Encoder::new(&mut out, quality);
    encoder
        .encode(rgb, width as u16, height as u16, ColorType::Rgb)
        .map_err(|err| format!("could not encode this image as JPEG: {err}"))?;
    Ok(out)
}

/// Decode a TIFF into RGB.
///
/// Handles the 8- and 16-bit grayscale/RGB/RGBA shapes that cover
/// essentially every TIFF a measurement or microscopy tool writes.
/// 16-bit samples are scaled down to 8 (`>> 8`) because `Image` is 8-bit;
/// alpha is composited onto white, matching what `decode_png` already
/// does for the same reason. Anything else -- CMYK, YCbCr, palette,
/// floating-point samples -- is refused by name rather than guessed at.
pub fn decode_tiff(bytes: &[u8]) -> Result<Image, String> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::ColorType;

    let mut decoder = Decoder::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("could not read this TIFF: {err}"))?;
    let (w, h) = decoder.dimensions().map_err(|err| format!("could not read this TIFF's dimensions: {err}"))?;
    let (width, height) = (w as usize, h as usize);
    if width == 0 || height == 0 {
        return Err(format!("invalid TIFF dimensions {width}x{height}"));
    }
    let color = decoder.colortype().map_err(|err| format!("could not read this TIFF's colour type: {err}"))?;
    let image = decoder.read_image().map_err(|err| format!("could not decode this TIFF: {err}"))?;

    // Normalise whatever sample width the file used down to 8-bit.
    let samples: Vec<u8> = match image {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|s| (s >> 8) as u8).collect(),
        other => {
            return Err(format!(
                "unsupported TIFF: this build reads 8- and 16-bit integer samples, and this \
                 file stores {}",
                match other {
                    DecodingResult::F32(_) => "32-bit floats",
                    DecodingResult::F64(_) => "64-bit floats",
                    DecodingResult::U32(_) => "32-bit integers",
                    DecodingResult::U64(_) => "64-bit integers",
                    _ => "a sample type",
                }
            ))
        }
    };

    let channels = match color {
        ColorType::Gray(_) => 1,
        ColorType::GrayA(_) => 2,
        ColorType::RGB(_) => 3,
        ColorType::RGBA(_) => 4,
        other => {
            return Err(format!(
                "unsupported TIFF colour type {other:?}: this build reads grayscale, \
                 grayscale+alpha, RGB and RGBA"
            ))
        }
    };
    let expected = width * height * channels;
    if samples.len() < expected {
        return Err(format!(
            "TIFF pixel data is short: expected {expected} samples for a {width}x{height} \
             image with {channels} channel(s), got {}",
            samples.len()
        ));
    }

    let mut pixels = Vec::with_capacity(width * height * 3);
    for px in samples[..expected].chunks_exact(channels) {
        // Alpha composites onto WHITE, the same call `decode_png` makes:
        // `Image` has three channels, and dropping alpha outright would
        // turn a transparent background black instead of white.
        let (r, g, b, a) = match channels {
            1 => (px[0], px[0], px[0], 255u8),
            2 => (px[0], px[0], px[0], px[1]),
            3 => (px[0], px[1], px[2], 255u8),
            _ => (px[0], px[1], px[2], px[3]),
        };
        if a == 255 {
            pixels.extend_from_slice(&[r, g, b]);
        } else {
            let over = |c: u8| -> u8 {
                let c = c as u32 * a as u32 + 255 * (255 - a as u32);
                (c / 255) as u8
            };
            pixels.extend_from_slice(&[over(r), over(g), over(b)]);
        }
    }
    Image::new(width, height, pixels)
}

/// Encode RGB as an uncompressed 8-bit RGB TIFF.
///
/// Lossless, so `save_image`/`load_image` through `.tif` IS pixel-exact
/// (tested). Uncompressed rather than LZW/DEFLATE: the file is bigger,
/// but it is the shape every TIFF reader in existence handles, and
/// `save_image` already has PNG for when size matters.
pub fn encode_tiff(width: usize, height: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    use tiff::encoder::{colortype, TiffEncoder};
    if width == 0 || height == 0 {
        return Err(format!("cannot write a {width}x{height} image as TIFF"));
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut encoder =
            TiffEncoder::new(&mut buf).map_err(|err| format!("could not start a TIFF: {err}"))?;
        encoder
            .write_image::<colortype::RGB8>(width as u32, height as u32, rgb)
            .map_err(|err| format!("could not encode this image as TIFF: {err}"))?;
    }
    Ok(buf.into_inner())
}

/// Paeth's predictor, the fifth PNG row filter.
///
/// Picks whichever of left, above and upper-left is closest to their linear
/// estimate `a + b - c`. It is the filter that makes PNG compress
/// photographs well, and it is the one everybody gets subtly wrong: the
/// comparison is on the ABSOLUTE distance, and ties break toward `a`.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (a as i16, b as i16, c as i16);
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

/// Decode a PNG into RGB.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err("not a PNG (the 8-byte signature is missing)".into());
    }
    let mut pos = 8;
    let (mut width, mut height) = (0usize, 0usize);
    let (mut depth, mut colour) = (0u8, 0u8);
    let mut palette: Vec<(u8, u8, u8)> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_ihdr = false;

    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let body_at = pos + 8;
        if body_at + len + 4 > bytes.len() {
            return Err(format!(
                "truncated PNG: chunk `{}` claims {len} bytes and only {} remain",
                String::from_utf8_lossy(kind),
                bytes.len().saturating_sub(body_at)
            ));
        }
        let body = &bytes[body_at..body_at + len];

        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err("PNG header chunk is too short".into());
                }
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                depth = body[8];
                colour = body[9];
                if body[12] != 0 {
                    return Err(
                        "this PNG is interlaced (Adam7), which this decoder does not read -- \
                         save it without interlacing"
                            .into(),
                    );
                }
                seen_ihdr = true;
            }
            b"PLTE" => {
                palette = body.chunks_exact(3).map(|c| (c[0], c[1], c[2])).collect();
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos = body_at + len + 4; // skip the CRC
    }

    if !seen_ihdr {
        return Err("PNG has no header chunk".into());
    }
    if width == 0 || height == 0 {
        return Err(format!("PNG has zero extent ({width}x{height})"));
    }
    if idat.is_empty() {
        return Err("PNG has no image data".into());
    }

    let channels: usize = match colour {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        other => return Err(format!("PNG colour type {other} is not one the format defines")),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return Err(format!("PNG bit depth {depth} is not one the format defines"));
    }
    if colour == 3 && palette.is_empty() {
        return Err("PNG says it is palette-coloured but carries no palette".into());
    }

    let raw = crate::inflate::zlib_decompress(&idat)
        .map_err(|m| format!("PNG image data would not decompress: {m}"))?;

    // A row is a filter byte followed by the packed samples.
    let bits_per_pixel = channels * depth as usize;
    let row_bytes = (width * bits_per_pixel).div_ceil(8);
    // The filters work on whole BYTES, offset by one pixel — except below
    // 8 bits, where the offset is one byte. That special case is the one
    // thing about PNG filtering that is not obvious from the name.
    let step = (bits_per_pixel / 8).max(1);
    let expected = (row_bytes + 1) * height;
    if raw.len() < expected {
        return Err(format!(
            "PNG image data is short: {} bytes for a {width}x{height} image that needs {expected}",
            raw.len()
        ));
    }

    let mut lines: Vec<Vec<u8>> = Vec::with_capacity(height);
    let mut prev = vec![0u8; row_bytes];
    for y in 0..height {
        let at = y * (row_bytes + 1);
        let filter = raw[at];
        let mut line = raw[at + 1..at + 1 + row_bytes].to_vec();
        for i in 0..row_bytes {
            let a = if i >= step { line[i - step] } else { 0 };
            let b = prev[i];
            let c = if i >= step { prev[i - step] } else { 0 };
            line[i] = match filter {
                0 => line[i],
                1 => line[i].wrapping_add(a),
                2 => line[i].wrapping_add(b),
                3 => line[i].wrapping_add((((a as u16) + (b as u16)) / 2) as u8),
                4 => line[i].wrapping_add(paeth(a, b, c)),
                other => return Err(format!("PNG row {y} uses filter {other}, which is not one of the five")),
            };
        }
        prev.clone_from(&line);
        lines.push(line);
    }

    // Unpack to 8-bit RGB, compositing any alpha onto white.
    let mut pixels = Vec::with_capacity(width * height * 3);
    for line in &lines {
        // One sample, normalised to 0..255 whatever the bit depth.
        let sample = |i: usize| -> u8 {
            match depth {
                16 => line[i * 2],
                8 => line[i],
                d => {
                    let per_byte = 8 / d as usize;
                    let byte = line[i / per_byte];
                    let shift = 8 - d as usize * (i % per_byte + 1);
                    let mask = (1u16 << d) - 1;
                    let v = ((byte as u16) >> shift) & mask;
                    // Scale to the full range: 4-bit 15 must become 255,
                    // not 15, or every low-depth image reads as black.
                    ((v * 255) / mask) as u8
                }
            }
        };
        // A palette index must NOT be rescaled -- it is an index, not an
        // intensity, and scaling it turns entry 3 of 16 into entry 51.
        let index = |i: usize| -> usize {
            match depth {
                8 => line[i] as usize,
                d => {
                    let per_byte = 8 / d as usize;
                    let byte = line[i / per_byte];
                    let shift = 8 - d as usize * (i % per_byte + 1);
                    (((byte as u16) >> shift) & ((1u16 << d) - 1)) as usize
                }
            }
        };
        for x in 0..width {
            let (r, g, b, a) = match colour {
                0 => {
                    let v = sample(x);
                    (v, v, v, 255)
                }
                2 => (sample(x * 3), sample(x * 3 + 1), sample(x * 3 + 2), 255),
                3 => {
                    let i = index(x);
                    let (r, g, b) = *palette.get(i).ok_or_else(|| {
                        format!("PNG references palette entry {i} of {}", palette.len())
                    })?;
                    (r, g, b, 255)
                }
                4 => {
                    let v = sample(x * 2);
                    (v, v, v, sample(x * 2 + 1))
                }
                _ => (
                    sample(x * 4),
                    sample(x * 4 + 1),
                    sample(x * 4 + 2),
                    sample(x * 4 + 3),
                ),
            };
            if a == 255 {
                pixels.extend_from_slice(&[r, g, b]);
            } else {
                // Over white: `Image` has three channels, so transparency
                // has to become something. White is what a figure sits on
                // and what a viewer shows behind a transparent PNG.
                let over = |c: u8| -> u8 {
                    ((c as u16 * a as u16 + 255 * (255 - a as u16)) / 255) as u8
                };
                pixels.extend_from_slice(&[over(r), over(g), over(b)]);
            }
        }
    }

    Image::new(width, height, pixels)
}
