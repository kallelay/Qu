"""bench.py -- same CV pipeline as bench.qu, via OpenCV (cv2), on the SAME
`test_image.bmp` (built once by generate_image.py) so both scripts time and
verify against byte-identical input.

Neither opencv-python nor scikit-image was already installed in this
environment (checked `import cv2` / `import skimage` -- both raised
ModuleNotFoundError); `opencv-python-headless` was installed for this
benchmark (`pip install opencv-python-headless`, version 5.0.0 at the time
of this run) since it's the more common/faster choice for this exact
pipeline (built-in Otsu flag on `cv2.threshold`, `connectedComponentsWithStats`
for one-call labeling+stats, `cv2.getRotationMatrix2D` for a combined
rotate+scale affine matrix) -- scikit-image would also work but needs more
separate calls (`filters.threshold_otsu`, `measure.label` + `regionprops`)
for the same result.

Run: python benchmarks/image_processing/bench.py
"""
import time
import cv2
import numpy as np

IMG_PATH = "benchmarks/image_processing/test_image.bmp"

# --- load (untimed) ----------------------------------------------------
img = cv2.imread(IMG_PATH, cv2.IMREAD_COLOR)
if img is None:
    raise SystemExit(f"could not load {IMG_PATH} -- run generate_image.py first")
h, w = img.shape[:2]
print(f"loaded             : {w}x{h}")

# --- stage 1: grayscale conversion --------------------------------------
t0 = time.perf_counter()
gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
t_gray = time.perf_counter() - t0
print(f"grayscale          : {t_gray:.4f} s")

# --- stage 2: Otsu threshold ---------------------------------------------
t0 = time.perf_counter()
level, binary = cv2.threshold(gray, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
t_otsu = time.perf_counter() - t0
print(f"otsu_threshold     : {t_otsu:.4f} s  (level={level:.2f})")

# --- stage 3: morphological opening (noise cleanup) ----------------------
t0 = time.perf_counter()
kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (5, 5))  # radius 2 -> side 2*2+1=5, same as Qu's imopen(img,2)
opened = cv2.morphologyEx(binary, cv2.MORPH_OPEN, kernel)
t_open = time.perf_counter() - t0
print(f"imopen              : {t_open:.4f} s")

# --- stage 4: connected-component labeling + blob stats ------------------
t0 = time.perf_counter()
num_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(opened, connectivity=8)
t_label = time.perf_counter() - t0
count = num_labels - 1  # exclude background label 0
total_area = int(stats[1:, cv2.CC_STAT_AREA].sum())
print(f"label_blobs+stats  : {t_label:.4f} s  (count={count})")
print(f"total_blob_area    = {total_area}")

# --- stage 5: affine transform (rotate 15 deg + scale 0.75x, same size) --
t0 = time.perf_counter()
center = (w / 2.0, h / 2.0)
M = cv2.getRotationMatrix2D(center, 15, 0.75)  # single combined rotate+scale affine
warped = cv2.warpAffine(gray, M, (w, h), flags=cv2.INTER_LINEAR, borderValue=0)
t_warp = time.perf_counter() - t0
print(f"rotate+scale (warpAffine): {t_warp:.4f} s  ({warped.shape[1]}x{warped.shape[0]})")

# --- stage 6: histogram equalization --------------------------------------
t0 = time.perf_counter()
equalized = cv2.equalizeHist(warped)
t_eq = time.perf_counter() - t0
print(f"equalizeHist       : {t_eq:.4f} s")

total = t_gray + t_otsu + t_open + t_label + t_warp + t_eq
print(f"total              : {total:.4f} s")

# sanity: equalization should widen the intensity spread (same "histogram
# std before/after" check as bench.qu).
before_hist = cv2.calcHist([warped], [0], None, [256], [0, 256]).flatten()
after_hist = cv2.calcHist([equalized], [0], None, [256], [0, 256]).flatten()
print(f"histogram std (before -> after): {before_hist.std():.2f} -> {after_hist.std():.2f}")

cv2.imwrite("benchmarks/image_processing/py_output_equalized.bmp", equalized)
cv2.imwrite("benchmarks/image_processing/py_output_binary.bmp", opened)
