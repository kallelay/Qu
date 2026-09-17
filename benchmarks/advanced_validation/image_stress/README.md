# Image-stress test: ground-truth blob detection vs `cv2`

Goes beyond `catalog/qu_image_blobs.qu` (a small, hand-built 3-blob example
with no recorded ground truth) and `../../image_processing/` (a realistic
6-stage `load_image -> grayscale -> otsu -> imopen -> label_blobs -> imrotate
-> imequalize` pipeline benchmark, generic scene, no ground truth either) --
this scenario's whole point is a synthetic image with a **known, exact**
blob count/position/size baked in at generation time, so detection accuracy
can be checked against a real number, not just eyeballed.

Run:

```bash
python benchmarks/advanced_validation/image_stress/generate_image.py
qu run benchmarks/advanced_validation/image_stress/image_stress.qu
python benchmarks/advanced_validation/image_stress/image_stress.py
```

(Run the `.qu` script before the `.py` script -- `image_stress.qu` dumps its
own per-blob stats to `qu_blob_stats.csv` for the Python side to
nearest-centroid-match against, not just compare summary counts.)

## The image and its ground truth

`generate_image.py` (numpy + cv2, seeded `SEED=20260831`, same approach as
`../../image_processing/generate_image.py`) builds a 500x500, 24-bit
uncompressed BMP (the only format Qu's `load_image` can decode) containing:

- **9 shapes on a 3x3 grid** (one shape per 166x166 cell, checkerboard
  circle/rect, every shape kept >30px clear of its own cell's edge so no
  two shapes are ever within 60px of each other -- they can never touch or
  merge): 5 filled circles (radii 45/38/50/40/55) at cell centers
  `(83,83) (417,83) (250,250) (83,417) (417,417)`, painted at gray level
  200; 4 filled rectangles (half-sizes 48x32/35x45/42x38/38x40) at cell
  centers `(250,83) (83,250) (417,250) (250,417)`, painted at gray level
  215 -- see `ground_truth.csv` for the exact painted-center/measured-area/
  centroid/bbox of all 9, in generation order.
- **Background**: Gaussian noise, mean 60, std 10, clipped to `[0,255]`
  uint8 -- comfortably separated from the shapes (max observed background
  pixel value: 105; min shape pixel value: 200 -- a genuinely empty gap of
  94 gray levels between the two clusters, deliberately wide so Otsu has a
  clean, unambiguous split to find).
- **15 single-pixel bright speckles** (value 255) scattered in the
  background, each checked against an 11x11-dilated copy of the shape mask
  so none can ever land within 5px of a real shape -- noise `imopen` should
  erase, not real blobs.

The ground truth itself is not eyeballed or hand-computed from the paint
parameters (a circle's discretized pixel area isn't exactly `pi*r^2`,
its bounding box isn't exactly `2r+1`): `generate_image.py` draws the same
9 shapes onto a separate, noise-free, speckle-free mask and runs
`cv2.connectedComponentsWithStats` on THAT to get the exact
area/centroid/bbox actually produced by rasterizing each shape, then writes
`ground_truth.csv` (committed, 9 rows) -- an independent third reference
that both Qu's and cv2's labeling of the final noisy image can be checked
against, not just against each other.

## Results -- accuracy

**Blob count**: Qu's `label_blobs(...).count` = 9, cv2's
`connectedComponentsWithStats(..., connectivity=8)`'s label count minus
background = 9, ground truth = 9. All three agree exactly, every run. All
15 speckles are fully erased by `imopen(binary, 1)` (Qu) /
`cv2.morphologyEx(binary, MORPH_OPEN, kernel=3x3)` (cv2, same radius-1 /
3x3-square structuring element on both sides) -- none survive as spurious
blobs, matching `catalog/qu_image_blobs.qu`'s own "does imopen actually
clean up" check.

**Centroids**: after nearest-centroid (Hungarian/`linear_sum_assignment`)
matching Qu's 9 blobs and cv2's 9 blobs to `ground_truth.csv`'s 9 rows,
**every single centroid -- Qu-vs-ground-truth, cv2-vs-ground-truth, and
Qu-vs-cv2 -- lands at an error of exactly 0.0000 px** (all 9 shapes are
symmetric and grid-aligned at integer pixel centers, and the same square
opening kernel on both sides shrinks each shape's boundary symmetrically,
so the centroid is preserved exactly on both sides).

**Area/bbox size**: Qu's and cv2's per-blob `area`/`bbox_width`/
`bbox_height` are **bit-identical to each other on all 9 blobs** (same
numbers down to the last pixel -- see the per-blob table below); both
differ from the noise-free ground truth by a small, expected amount (the
radius-1 opening legitimately erodes+dilates each circle's boundary,
shaving up to 2px off its diameter/bbox and up to ~90 px^2 (<1.1%) off its
area -- rectangles, already axis-aligned with the square structuring
element, come back with their bbox and area completely unchanged).

| gt_label | kind | qu area | cv2 area | gt area | qu bbox | cv2 bbox | gt bbox |
|---:|---|---:|---:|---:|---|---|---|
| 1 | circle | 6357 | 6357 | 6361 | 89x89 | 89x89 | 91x91 |
| 2 | circle | 4509 | 4509 | 4513 | 75x75 | 75x75 | 77x77 |
| 3 | rect   | 6305 | 6305 | 6305 | 97x65 | 97x65 | 97x65 |
| 4 | circle | 7841 | 7841 | 7845 | 99x99 | 99x99 | 101x101 |
| 5 | rect   | 6461 | 6461 | 6461 | 71x91 | 71x91 | 71x91 |
| 6 | rect   | 6545 | 6545 | 6545 | 85x77 | 85x77 | 85x77 |
| 7 | circle | 9473 | 9473 | 9477 | 109x109 | 109x109 | 111x111 |
| 8 | circle | 5021 | 5021 | 5025 | 79x79 | 79x79 | 81x81 |
| 9 | rect   | 6237 | 6237 | 6237 | 77x81 | 77x81 | 77x81 |

Max centroid error (all pairs, all 9 blobs): **0.0000 px** (tolerance
1.0 px). Max relative area error vs ground truth: **0.089%** (tolerance
2%). Max bbox width/height error vs ground truth: **2 px**, only ever on
circles, only ever from the opening's expected erosion, and identical
between Qu and cv2 in every case.

### The Otsu threshold VALUE differs (105.625 vs 105.000) -- investigated, not a bug

`otsu_threshold(gray)` (Qu) returns **105.625**; `cv2.threshold(gray, 0,
255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)` (cv2) returns **105.000**, every
run (both deterministic). Root-caused by reading both algorithms rather
than assuming a bug:

- The image's histogram has a genuinely **empty gap of 94 gray levels**
  between the background cluster (max value 105, a few outlier pixels) and
  the shape cluster (min value 200) -- deliberate, from the generator's
  wide mean/std separation. Between-class variance is therefore
  **mathematically constant for every candidate cut point inside that
  gap** -- Otsu's criterion has a flat plateau, not a single sharp optimum,
  for this image.
- `qu-core/src/threshold.rs::otsu_threshold_bins` bins the *actual*
  `[min(samples), max(samples)]` range into 256 equal-width buckets (here
  `lo=16, hi=255`, bin width `≈0.9336` -- see the module's own doc comment
  for why: this generalizes to arbitrary-range signals, not just 8-bit
  images), sweeps every interior bin boundary with a strict `variance >
  best_variance` update (keeps the FIRST bin reaching the plateau's
  maximum), and reports **the upper edge of that bin**
  (`bin_edge_to_value`, `lo + (bin+1)*width`). The first bin at which the
  plateau begins is bin 95 (`[104.69, 105.625)` in real units, the bin
  containing the background's own max value 105) -- its upper edge is
  `16 + 96*0.9336 = 105.625`, reproduced exactly by hand-simulating the
  algorithm in Python against this image's own histogram (see
  investigation notes below).
- cv2's classic Otsu implementation sweeps the same kind of cumulative
  sums but over a fixed 256-bin, unit-width `[0,255]` histogram and reports
  **the raw bin index itself** (not an edge one bin above it) at which the
  plateau is first reached -- bin **105** (the same "last bin containing
  background pixels" boundary, just reported as its own value instead of
  the value just above it).

Both conventions are internally self-consistent with their own
apply-threshold semantics: Qu's `threshold(x, level)` uses `>=` (so
`105 >= 105.625` is false -> pixel 105 stays background, correct), cv2's
`THRESH_BINARY` uses `>` (so `105 > 105` is also false -> same pixel stays
background, also correct). The two threshold VALUES differ by
essentially one histogram bin width because they report opposite edges of
the identical decision boundary within a real flat plateau -- and, as the
accuracy table above shows, this has **zero effect** on the actual
segmentation: both sides classify every one of the 250,000 pixels
identically, producing bit-identical blob areas/bboxes and exactly-zero
centroid error. Not a correctness bug in either implementation; a genuine,
now fully-explained convention difference that only becomes visible
because this test deliberately built an unusually wide empty gap into the
histogram. (Confirmed by reproducing `otsu_threshold_bins`'s exact
algorithm against this image's own histogram in a throwaway Python
script: the plateau runs from bin 95 to bin 196, tied to `1e-12` given
numpy's own summation order.)

## Results -- speed

Timed block = `grayscale -> otsu_threshold -> threshold -> imopen ->
label_blobs -> blob_stats` (Qu, one `tic()/toc()`) vs `cvtColor ->
threshold(..., THRESH_OTSU) -> morphologyEx(MORPH_OPEN) ->
connectedComponentsWithStats` (cv2, one `time.perf_counter()` pair), same
500x500 image, same radius-1/3x3 opening kernel, same 8-connectivity, 3
repeated runs each (single-machine, single-trial-per-run numbers -- same
"not statistically rigorous, good enough to see the shape" caveat as every
other scenario in `../README.md`; run-to-run OS noise on this machine was
visibly larger than usual for such a small image, see the spread below):

| Run | Qu (ms) | cv2 (ms) | ratio (Qu/cv2) |
|---|---:|---:|---:|
| 1 | 17.93 | 13.57 | 1.32x |
| 2 | 17.01 | 12.44 | 1.37x |
| 3 | 15.17 | 9.59  | 1.58x |

Qu runs **~1.3-1.6x slower** than cv2 on this pipeline at 500x500 -- a
real, consistent gap in the same direction every run, but a modest one
(not an order-of-magnitude difference like `curve_fit`'s in
`../curve_kalman/README.md`), and small in absolute terms (single-digit
milliseconds either way). Both sides spend the overwhelming majority of
this block's time in `label_blobs`/`connectedComponentsWithStats` and
`imopen`/`morphologyEx` (per-pixel flood-fill and erosion/dilation) rather
than the O(1)-ish `otsu_threshold` scan; no attempt was made here to
split the block into per-stage timings the way `../../image_processing/
bench.qu` does, since the task only asked for one combined number.

## Honest verdict

Qu's `otsu_threshold` + `threshold` + `imopen` + `label_blobs` +
`blob_stats` pipeline is **fully accurate** on this ground-truth-backed
stress test: exact blob count (9/9/9 across Qu, cv2, and the independent
ground truth, every run), exactly-zero centroid error against both cv2 and
the known painted centers, and area/bbox measurements bit-identical to
cv2's own (both differing from the noise-free ground truth only by the
same small, expected, opening-induced erosion -- never a mismatch between
the two implementations themselves). The one numeric discrepancy found
(`otsu_threshold`'s reported scalar, 105.625 vs cv2's 105.000) was
root-caused to a real but harmless bin-edge-reporting convention
difference inside a genuinely flat plateau of the Otsu objective, verified
by reproducing the algorithm by hand against this image's own histogram --
not a bug, and confirmed to have zero effect on the actual segmentation.
Qu loses on speed here: consistently ~1.3-1.6x slower than cv2 on this
block across 3 runs, a real if modest gap, called out plainly rather than
buried.
