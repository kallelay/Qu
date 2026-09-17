"""generate_image.py -- builds the ONE shared test image `image_stress.qu`
and `image_stress.py` both load via a plain `load_image`/`cv2.imread` call,
same convention as `../../image_processing/generate_image.py` (byte-identical
BMP input on both sides, not two independently-seeded RNGs).

Unlike `../../image_processing/generate_image.py` (which paints a generic
9-shape scene and never records where the shapes actually ended up) or
`catalog/qu_image_blobs.qu` (a tiny 3-blob hand-built example), THIS
generator also writes `ground_truth.csv`: the exact area/centroid/bbox of
every painted shape, computed by running `cv2.connectedComponentsWithStats`
on a clean, noise-free, speckle-free mask of the shapes alone (drawn in the
same order, same coordinates, before any background noise or speckles are
added) -- an unambiguous third reference, independent of both Qu's and
cv2's own labeling of the FINAL noisy image, that both language's detected
blobs can be checked against.

500x500, 24-bit uncompressed BMP (the only format Qu's `load_image` can
decode -- see `engine/crates/qu-interp/src/image.rs::decode_bmp`'s doc
comment). Content: a 3x3 grid of well-separated shapes (5 circles + 4
rectangles = 9 known blobs, one per grid cell, each with generous margin
to its cell's edge so no two shapes can ever touch even before considering
the gap between cells) on a Gaussian-noise gray background, plus small
bright single-pixel speckles (well above the background level, well below
the shapes' level) that `imopen` should erase -- same "does the opening
actually clean up noise without eating real blobs" check
`../../image_processing/generate_image.py` and `catalog/qu_image_blobs.qu`
both use, but this time with recorded ground truth to check accuracy
against, not just blob survival.

Run once: `python generate_image.py` (writes `test_image.bmp` next to this
script, ~880 KB, gitignored like `../../image_processing/test_image.bmp` --
deterministic from the seed below, nothing lost by not committing it; also
(re)writes the small, committed `ground_truth.csv`).
"""
import csv
import os

import cv2
import numpy as np

SEED = 20260831
W = H = 500
HERE = os.path.dirname(os.path.abspath(__file__))
OUT_BMP = os.path.join(HERE, "test_image.bmp")
OUT_GT = os.path.join(HERE, "ground_truth.csv")

rng = np.random.default_rng(SEED)

# --- 3x3 grid of well-separated shapes --------------------------------------
# Grid cell centers 166 px apart; every shape sits within ~50px of its own
# cell center, leaving >30px clearance to the cell boundary on every side --
# neighbouring shapes are therefore always >60px apart, never touching.
CELL = W / 3.0  # ~166.67
centers = [((c + 0.5) * CELL, (r + 0.5) * CELL) for r in range(3) for c in range(3)]
centers = [(round(x), round(y)) for (x, y) in centers]

# Alternate circle/rect like a checkerboard: 5 circles (even grid index), 4
# rectangles (odd grid index).
circle_radii = [45, 38, 50, 40, 55]
rect_halfsizes = [(48, 32), (35, 45), (42, 38), (38, 40)]  # (half_w, half_h)

shapes = []  # each: dict(kind, cx, cy, ...)
ci = 0
ri = 0
for idx, (cx, cy) in enumerate(centers):
    if idx % 2 == 0:
        r = circle_radii[ci]
        ci += 1
        shapes.append({"kind": "circle", "cx": cx, "cy": cy, "r": r})
    else:
        hw, hh = rect_halfsizes[ri]
        ri += 1
        x0, y0, x1, y1 = cx - hw, cy - hh, cx + hw, cy + hh
        shapes.append({"kind": "rect", "cx": cx, "cy": cy, "x0": x0, "y0": y0, "x1": x1, "y1": y1})

assert len(shapes) == 9, "expected 5 circles + 4 rectangles = 9 shapes"

# --- ground-truth mask: shapes only, no noise, no speckles ------------------
gt_mask = np.zeros((H, W), dtype=np.uint8)
for s in shapes:
    if s["kind"] == "circle":
        cv2.circle(gt_mask, (s["cx"], s["cy"]), s["r"], 255, thickness=-1)
    else:
        cv2.rectangle(gt_mask, (s["x0"], s["y0"]), (s["x1"], s["y1"]), 255, thickness=-1)

n_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(gt_mask, connectivity=8)
n_gt_blobs = n_labels - 1
assert n_gt_blobs == 9, f"ground-truth mask should have exactly 9 disjoint blobs, got {n_gt_blobs}"

# Match each connected-component label back to the `shapes` entry it came
# from (by nearest centroid) so the CSV records the shape's own kind/painted
# params next to its exact measured area/centroid/bbox.
gt_rows = []
for lbl in range(1, n_labels):
    mx, my = centroids[lbl]
    best = min(range(len(shapes)), key=lambda i: (shapes[i]["cx"] - mx) ** 2 + (shapes[i]["cy"] - my) ** 2)
    s = shapes[best]
    gt_rows.append(
        {
            "label": lbl,
            "kind": s["kind"],
            "painted_cx": s["cx"],
            "painted_cy": s["cy"],
            "area": int(stats[lbl, cv2.CC_STAT_AREA]),
            "cx": float(mx),
            "cy": float(my),
            "bbox_x": int(stats[lbl, cv2.CC_STAT_LEFT]),
            "bbox_y": int(stats[lbl, cv2.CC_STAT_TOP]),
            "bbox_w": int(stats[lbl, cv2.CC_STAT_WIDTH]),
            "bbox_h": int(stats[lbl, cv2.CC_STAT_HEIGHT]),
        }
    )
# Sanity: every painted shape matched to exactly one ground-truth label.
matched_shapes = {min(range(len(shapes)), key=lambda i: (shapes[i]["cx"] - centroids[l][0]) ** 2 + (shapes[i]["cy"] - centroids[l][1]) ** 2) for l in range(1, n_labels)}
assert len(matched_shapes) == 9, "ground-truth labels did not match 1:1 onto the 9 painted shapes"

with open(OUT_GT, "w", newline="") as fh:
    w = csv.DictWriter(fh, fieldnames=list(gt_rows[0].keys()))
    w.writeheader()
    w.writerows(gt_rows)

# --- final image: noisy background + shapes + speckles ----------------------
bg = rng.normal(loc=60.0, scale=10.0, size=(H, W)).clip(0, 255).astype(np.uint8)
img = np.stack([bg, bg, bg], axis=-1).copy()  # grayscale-as-RGB, matches Qu's convention

CIRCLE_VAL = (200, 200, 200)
RECT_VAL = (215, 215, 215)
for s in shapes:
    if s["kind"] == "circle":
        cv2.circle(img, (s["cx"], s["cy"]), s["r"], CIRCLE_VAL, thickness=-1)
    else:
        cv2.rectangle(img, (s["x0"], s["y0"]), (s["x1"], s["y1"]), RECT_VAL, thickness=-1)

# 15 single-PIXEL bright speckles well clear of every shape (checked against
# a 5px-margin dilation of the shape mask so `imopen`'s erosion+dilation
# radius can never accidentally fuse one onto a real blob), well above the
# background level so they form their own tiny blobs before `imopen`.
shape_dilated = cv2.dilate(gt_mask, np.ones((11, 11), np.uint8))  # 5px margin each side
n_speckles = 15
placed = 0
attempts = 0
speckle_xy = []
while placed < n_speckles and attempts < 10000:
    attempts += 1
    sx = int(rng.integers(10, W - 10))
    sy = int(rng.integers(10, H - 10))
    if shape_dilated[sy, sx] != 0:
        continue
    img[sy, sx] = (255, 255, 255)
    speckle_xy.append((sx, sy))
    placed += 1
assert placed == n_speckles, f"only placed {placed}/{n_speckles} speckles"

# cv2 expects BGR; content is grayscale-valued (equal channels) so this is a
# no-op here, but keep the conversion explicit/honest about the convention.
img_bgr = cv2.cvtColor(img, cv2.COLOR_RGB2BGR)
ok = cv2.imwrite(OUT_BMP, img_bgr)
if not ok:
    raise SystemExit(f"failed to write {OUT_BMP}")

print(f"wrote {OUT_BMP}  ({os.path.getsize(OUT_BMP)} bytes, {W}x{H})")
print(f"wrote {OUT_GT}  ({len(gt_rows)} ground-truth blobs)")
print(f"circles: {len(circle_radii)}  rectangles: {len(rect_halfsizes)}  speckles: {placed}")
print(f"expected real blobs after cleanup: {len(shapes)}")
