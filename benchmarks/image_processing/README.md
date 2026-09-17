# CV pipeline benchmark: grayscale -> Otsu -> morphology -> blobs -> affine -> histeq

A representative computer-vision pipeline (not an isolated kernel) run on
the SAME 2000x2000 synthetic image in Qu, Python (OpenCV), and — where the
toolbox is available — MATLAB: grayscale conversion, Otsu thresholding,
morphological opening (noise cleanup), connected-component labeling +
per-blob stats, an affine transform (rotate 15 deg + scale 0.75x), and
global histogram equalization.

## Test image

`generate_image.py` builds `test_image.bmp` ONCE (2000x2000, 24-bit
uncompressed BMP — the only format Qu's `load_image` decodes) with a fixed
seed (`20260825`): Gaussian-noise background (mean 90, std 12), 6 filled
circles + 3 filled rectangles (intensity ~200-212, the "real" blobs a
segmentation pipeline should find — 9 total, hand-placed so none touch),
plus 20 single-pixel bright speckles (intensity 255) scattered in the
background — noise a morphological opening should erase, not real blobs.
`bench.qu` and `bench.py` both load this exact file rather than each
generating their own image from an independently-seeded RNG, so every
number below is computed on byte-identical input, not merely
"statistically similar" input.

Run once: `python benchmarks/image_processing/generate_image.py`
(`test_image.bmp` and every script's `*_output_*.bmp` are gitignored —
deterministic from the seed, nothing lost by not committing ~11 MB of
binary pixels).

## Pipeline mapping

| Stage | Qu | Python (OpenCV) | MATLAB (Image Processing Toolbox) |
|---|---|---|---|
| Grayscale | `grayscale(img)` | `cv2.cvtColor(img, COLOR_BGR2GRAY)` | `rgb2gray(img)` |
| Otsu threshold | `otsu_threshold(gray)` + `threshold(gray, level)` | `cv2.threshold(gray, 0, 255, THRESH_BINARY+THRESH_OTSU)` | `graythresh(gray)` + `imbinarize(gray, level)` |
| Morphological opening | `imopen(binary, 2)` (square SE, side 5) | `cv2.morphologyEx(binary, MORPH_OPEN, 5x5 rect kernel)` | `imopen(binary, strel('square',5))` |
| Labeling + stats | `label_blobs(opened)` + `blob_stats(...)` | `cv2.connectedComponentsWithStats(opened, connectivity=8)` | `bwlabel(opened,8)` + `regionprops(...,'Area','Centroid')` |
| Affine (rotate+scale) | `imrotate(gray, 15, bbox="crop")` + `imscale(rotated, 0.75, bbox="crop")` | `cv2.warpAffine(gray, getRotationMatrix2D(center,15,0.75), (w,h))` | `imwarp(gray, affine2d(...), 'OutputView', imref2d(size(gray)))` |
| Histogram equalization | `imequalize(warped)` | `cv2.equalizeHist(warped)` | `histeq(warped)` |

Qu composes the affine stage as two separate builtin calls
(`imrotate`+`imscale`, each its own `imwarp` dispatch internally); OpenCV
and MATLAB fold rotate+scale into one combined affine matrix and a single
resampling pass. Both are legitimate ways to express "rotate and scale
this image" — the timed block covers the whole stage in every language,
so the comparison is still apples-to-apples on wall-clock cost, just not
identical in call count.

## Python library used

Neither `opencv-python` nor `scikit-image` was already installed in this
environment (`import cv2` / `import skimage` both raised
`ModuleNotFoundError`). Installed `opencv-python-headless` (5.0.0) for
this benchmark — the better fit for this specific pipeline: Otsu is a
single flag on `cv2.threshold`, connected-components + stats is one call
(`connectedComponentsWithStats`), and rotate+scale combine into one
`getRotationMatrix2D` + `warpAffine`. scikit-image would need more
separate calls (`filters.threshold_otsu`, `measure.label` +
`regionprops`) for the same result.

## MATLAB: Image Processing Toolbox NOT available on this machine

`bench.m` is written and believed correct but **was not run** — this
machine's MATLAB R2025b has no Image Processing Toolbox installed.
`license('test','Image_Toolbox')` reports `1` (the license entitlement
exists) but the toolbox itself isn't present: `ver` lists only `MATLAB`
and `Parallel Computing Toolbox`, and `exist(...)` returns `0` for every
function this pipeline needs (`imbinarize`, `graythresh`, `imopen`,
`imclose`, `imerode`, `imdilate`, `bwlabel`, `regionprops`, `imwarp`,
`imrotate`, `histeq`, `strel`, `affine2d`) — confirmed directly:
`imbinarize` errors with `"imbinarize requires Image Processing
Toolbox."`. Only two base-MATLAB image utilities exist at all
(`rgb2gray`, `imresize`) — not enough to run this pipeline. **No MATLAB
numbers are reported below; do not treat `bench.m` as validated until it's
actually run on a machine with the toolbox installed.**

## Matty: no image-processing support at all

Ahmed's original ask was a four-way Qu/Python/MATLAB/Matty comparison.
Checked Matty (`../matty` (a sibling checkout), the standalone
sibling MATLAB-interpreter project — its own repo, not the nested
`Qu/matty/` dir or the retired `matty_suite/run_via_matty_jax.py` JAX
path) directly: `grep -rniE` across `src/`, `README.md`, and `TODO.md` for
`rgb2gray|imbinarize|bwlabel|regionprops|imopen|imclose|imerode|imdilate|
imwarp|graythresh|histeq|imrotate|imresize` returns **zero matches**.
Matty's `TODO.md` describes deep Control-System/Signal-Processing/
Statistics-Curve-Fitting coverage but no image-processing surface at all —
it's a numeric MATLAB-language interpreter (matplotlib-backed graphics for
plotting), not an Image Processing Toolbox reimplementation. Skipping the
Matty column entirely rather than forcing a misleading substitute.

## Correctness cross-check (not just timing)

Both Qu and Python, every trial, agree exactly on the two numbers that
matter most — blob count and total foreground area — which is the real
proof neither implementation is silently doing less work than the other:

| | Qu | Python (OpenCV) |
|---|---|---|
| Otsu level | 148.12 | 148.00 |
| blob count | **9** | **9** |
| total blob area (px) | **240609** | **240609** |

Blob count is exactly the 6 circles + 3 rectangles seeded into the image —
the 20 bright speckles are correctly erased by `imopen`/`morphologyEx`
before labeling in both languages (confirmed by count staying at 9, not
29). Total area matches to the pixel, every trial, in both languages — a
strong signal the threshold level, structuring-element size, and
connectivity convention are all equivalent across implementations, not
just coincidentally close. The tiny Otsu-level gap (148.12 vs 148.00,
0.08%) is expected: Otsu's algorithm sweeps a 256-bin histogram, and
Qu's/OpenCV's implementations can land on adjacent bins at a near-flat
part of the between-class-variance curve without disagreeing on which
pixels end up foreground vs background.

Histogram-equalization sanity (does the intensity spread actually widen?):
Qu 125515 -> 125740 std, Python 111127 -> 111378 std — both directions
correct, both magnitudes broadly similar (not identical, since
`imequalize` and `cv2.equalizeHist` use slightly different CDF-remap
implementations — this isn't a bug, just two independently-correct
histogram-equalization algorithms).

## Results (this machine, 3 trials each — not statistically rigorous, single-process wall-clock)

| Stage | Qu (3 trials) | Python/OpenCV (3 trials) | ratio (Qu/Py, avg) |
|---|---|---|---|
| `grayscale` | 0.0524 / 0.0233 / 0.0466 s | 0.0341 / 0.0688 / 0.0207 s | ~1.0x (tied) |
| `otsu_threshold` | 0.1566 / 0.0734 / 0.1212 s | 0.0024 / 0.0019 / 0.0016 s | ~59x |
| `imopen` | 0.6334 / 0.5359 / 0.8827 s | 0.0135 / 0.0333 / 0.0054 s | ~39x |
| `label_blobs`+`blob_stats` | 0.0655 / 0.0607 / 0.0738 s | 0.0116 / 0.0166 / 0.0072 s | ~5.6x |
| affine (rotate+scale) | 0.1413 / 0.1569 / 0.2176 s | 0.0057 / 0.0054 / 0.0025 s | ~38x |
| `imequalize`/`equalizeHist` | 0.0417 / 0.0451 / 0.0456 s | 0.0016 / 0.0166 / 0.0020 s | ~6.5x |
| **total** | **1.0909 / 0.8953 / 1.3875 s** | **0.0689 / 0.1426 / 0.0394 s** | **~13.5x** |

**Honest read**: OpenCV wins every stage, by a wide and stage-dependent
margin — expected, since OpenCV's morphology/threshold/connected-components
kernels are decades-optimized, hand-vectorized C++ (SIMD, cache-blocked),
while Qu's image builtins (landed earlier the same day as this benchmark)
are correctness-first, straightforward Rust implementations with no SIMD
or explicit blocking pass yet. `grayscale` is the one stage that's
genuinely tied — a single linear pass over the pixel buffer doesn't leave
much room for either side to be cleverer. `otsu_threshold` and the affine
warp show the largest gaps (~38-59x); `label_blobs`/`blob_stats` and
`imequalize` are the closest of the rest (~5.5-6.5x). Qu's own run-to-run
variance is notably wider than Python's (e.g. `imopen` 0.54-0.88s vs
OpenCV's tight 0.005-0.033s) — consistent with a shared, actively-mutating
dev machine rather than a real algorithmic instability, but worth more
trials before treating any single Qu number as definitive.

## Fixed: `otsu_threshold` no longer double-converts

`otsu_threshold`/`multi_otsu`/`kapur_threshold` all read an image's luma
via `image_luma_samples`, which used to unconditionally call
`img.to_grayscale()` — a full redundant BT.601 conversion pass (plus a
whole second `width*height*3` pixel-buffer allocation) over all 4,000,000
pixels, even when the image passed in (this pipeline's own `gray`) was
*already* grayscale from a prior `grayscale(img)` call.

**Fix** (`engine/crates/qu-interp/src/lib.rs`, `image_luma_samples`):
since the BT.601 weights sum to exactly 1.0, an already-gray pixel
(R=G=B) has its luma equal to that channel's value directly — no float
math needed. Reworked into a single per-pixel pass that branches on
`r == g && g == b`: takes the cheap direct-read path for already-gray
pixels, falls through to the same BT.601 formula as before for anything
else. This also removes the old `to_grayscale()` call's extra pixel-buffer
allocation entirely (there's no second `Image` being built anymore), so
even the non-grayscale path (color image straight into `otsu_threshold`,
without a prior `grayscale()` call) is now one pass instead of two.
Regression test: `otsu_threshold_on_color_image_matches_otsu_threshold_on_its_grayscale_conversion`
in `qu-interp`'s test suite asserts `otsu_threshold(color_img)` and
`otsu_threshold(grayscale(color_img))` land on the bit-identical
threshold, proving the shortcut path agrees exactly with the general
BT.601 path. Full `qu-interp` suite (761 tests) passes unchanged.

**Re-measured on this benchmark, same machine, 3 trials each direction,
back to back** (`cargo run --release -p qu-cli -- run
benchmarks/image_processing/bench.qu`, `test_image.bmp` regenerated fresh
for this run):

| | before fix | after fix |
|---|---|---|
| `otsu_threshold` (3 trials) | 0.1168 / 0.0839 / 0.0981 s | 0.0219 / 0.0238 / 0.0233 s |
| `otsu_threshold` avg | 0.0996 s | 0.0230 s |
| Otsu level (unchanged) | 148.12 | 148.12 |

**~4.3x faster** on this stage (0.0996s -> 0.0230s avg), with the Otsu
level identical to two decimal places across every trial in both
directions — the fix changes performance, not the result. OpenCV's
equivalent stage still wins by a wide margin (~0.002-0.003s, i.e. Qu is
still ~9-10x slower after this fix, down from ~59x before) — OpenCV's
threshold+Otsu kernel is decades-optimized hand-vectorized C++, and Qu's
`image_luma_samples` is still a plain scalar Rust loop with no SIMD. The
gap this fix closes is specifically the *redundant conversion pass*, not
the algorithmic gap to OpenCV.

## Out of scope

- No MATLAB numbers (toolbox not installed on this machine — see above).
- No Matty column (no image-processing builtins exist there at all — see
  above).
- Not statistically rigorous: 3 single-process trials per language on a
  shared, actively-mutating dev machine, not a clean-room benchmark rig.
- Pixel-exact output comparison between `imequalize`/`cv2.equalizeHist`
  and between the two affine-warp implementations wasn't attempted (both
  use different, independently-valid resampling/CDF-remap algorithms) —
  the blob count/area cross-check above is the correctness bar this
  benchmark actually clears.
- `multi_otsu`/`kapur_threshold` (Qu has both) aren't exercised here —
  this pipeline only needed the single-threshold case.

## Verified

Every Qu number above came from an actual `qu run
benchmarks/image_processing/bench.qu` (also confirmed as `cargo run
--manifest-path engine/Cargo.toml -p qu-cli --release -- run
benchmarks/image_processing/bench.qu`, same binary,
`engine/target/release/qu.exe`) against the release build compiled fresh
for this session. `bench.py` was run the same way (`python
benchmarks/image_processing/bench.py`) 3 times each, back to back, no
warm-up runs discarded.
