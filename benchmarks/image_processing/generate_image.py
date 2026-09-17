"""generate_image.py -- builds the ONE shared test image every benchmark
script (Qu, Python, MATLAB-when-available) loads via a plain `load_image`/
`cv2.imread`/`imread` call, so all three time-and-verify against
byte-identical input rather than three independently-seeded RNGs that would
never quite agree across languages.

2000x2000, 24-bit uncompressed BMP (the only format Qu's `load_image` can
decode -- see `engine/crates/qu-interp/src/image.rs::decode_bmp`'s doc
comment). Content: Gaussian-noise background + 6 filled circles + 3 filled
rectangles (the "blobs" a segmentation pipeline should find --  9 total,
sized/spaced so they never touch) + 20 single-pixel/3x3 salt speckles well
above the background level (bright noise a morphological opening should
erase, not real blobs -- mirrors the "does imopen actually clean up" check
already in the interpreter's own test suite).

Run once: `python generate_image.py` (writes `test_image.bmp` next to this
script, ~11.4 MB, gitignored like the other benchmark folders' generated
CSVs -- deterministic from the seed below, nothing lost by not committing
it).
"""
import numpy as np
import cv2
import os

SEED = 20260825
W = H = 2000
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "test_image.bmp")

rng = np.random.default_rng(SEED)

# Background: Gaussian noise around a mid-gray level, well below the shapes'
# intensity so Otsu has a clean bimodal histogram to split.
bg = rng.normal(loc=90.0, scale=12.0, size=(H, W)).clip(0, 255).astype(np.uint8)
img = np.stack([bg, bg, bg], axis=-1).copy()  # grayscale-as-RGB, matches Qu's convention

CIRCLE_VAL = (200, 200, 200)
RECT_VAL = (212, 212, 212)

# 6 circles: (cx, cy, radius) -- hand-placed on a loose grid, radius <= 90,
# so neighbours are always > 2*radius + margin apart (never touch or merge).
circles = [
    (300, 300, 70), (1000, 300, 90), (1700, 300, 60),
    (300, 1000, 80), (1000, 1000, 55), (1700, 1650, 75),
]
for (cx, cy, r) in circles:
    cv2.circle(img, (cx, cy), r, CIRCLE_VAL, thickness=-1)

# 3 rectangles: (x0, y0, x1, y1) -- also well clear of every circle/rect.
rects = [
    (150, 1550, 350, 1750),
    (900, 1550, 1150, 1700),
    (1550, 900, 1800, 1150),
]
for (x0, y0, x1, y1) in rects:
    cv2.rectangle(img, (x0, y0), (x1, y1), RECT_VAL, thickness=-1)

# 20 bright speckles (1-3px each) scattered in otherwise-empty background --
# above the eventual Otsu cut so they show up as tiny separate blobs before
# `imopen`, and should vanish after it.
speckle_xy = rng.integers(50, W - 50, size=(20, 2))
for (sx, sy) in speckle_xy:
    cv2.circle(img, (int(sx), int(sy)), 1, (255, 255, 255), thickness=-1)

# cv2 expects BGR; content is grayscale-valued (equal channels) so this is a
# no-op here, but keep the conversion explicit/honest about the convention.
img_bgr = cv2.cvtColor(img, cv2.COLOR_RGB2BGR)
ok = cv2.imwrite(OUT, img_bgr)
if not ok:
    raise SystemExit(f"failed to write {OUT}")

print(f"wrote {OUT}  ({os.path.getsize(OUT)} bytes, {W}x{H})")
print(f"circles: {len(circles)}  rectangles: {len(rects)}  speckles: {len(speckle_xy)}")
print(f"expected real blobs after cleanup: {len(circles) + len(rects)}")
