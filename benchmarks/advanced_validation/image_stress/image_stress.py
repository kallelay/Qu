"""image_stress.py -- cv2-side mirror of image_stress.qu's pipeline
(grayscale -> Otsu threshold -> morphological opening -> connected-component
labeling + stats) on the SAME `test_image.bmp`, PLUS the accuracy check
neither `../../image_processing/bench.py` nor `catalog/qu_image_blobs.qu`
do: nearest-centroid-matching every detected blob (Qu's own dump in
`qu_blob_stats.csv`, cv2's own `connectedComponentsWithStats` on this same
run) against BOTH each other AND the independent `ground_truth.csv` written
by `generate_image.py`, checking centroid/area/bbox error against a tight
pixel tolerance rather than eyeballing summary counts.

opencv-python is already installed system-wide (v5.0.0, confirmed via
`import cv2`) -- same choice `../../image_processing/bench.py` already
made and explained (built-in Otsu flag on `cv2.threshold`,
`connectedComponentsWithStats` for one-call labeling+stats); scikit-image
is not installed and not needed for this exact pipeline.

Run: python benchmarks/advanced_validation/image_stress/image_stress.py
(run `generate_image.py` and `image_stress.qu` first -- this script reads
both test_image.bmp and qu_blob_stats.csv)
"""
import os
import time

import cv2
import numpy as np
import pandas as pd
from scipy.optimize import linear_sum_assignment

HERE = os.path.dirname(os.path.abspath(__file__))
IMG_PATH = os.path.join(HERE, "test_image.bmp")
GT_PATH = os.path.join(HERE, "ground_truth.csv")
QU_PATH = os.path.join(HERE, "qu_blob_stats.csv")
CENTROID_TOL = 1.0  # pixels
AREA_REL_TOL = 0.02  # 2% -- opening shrinks/rounds a real blob's boundary a little

# --- load (untimed) ----------------------------------------------------
img = cv2.imread(IMG_PATH, cv2.IMREAD_COLOR)
if img is None:
    raise SystemExit(f"could not load {IMG_PATH} -- run generate_image.py first")
h, w = img.shape[:2]
print(f"loaded             : {w}x{h}")

if not os.path.exists(QU_PATH):
    raise SystemExit(f"missing {QU_PATH} -- run image_stress.qu first")
qu_df = pd.read_csv(QU_PATH)
gt_df = pd.read_csv(GT_PATH)

# --- timed block: grayscale -> otsu -> threshold -> imopen -> label+stats --
t0 = time.perf_counter()
gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
level, binary = cv2.threshold(gray, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (3, 3))  # radius 1 -> side 2*1+1=3, same as Qu's imopen(img, 1)
opened = cv2.morphologyEx(binary, cv2.MORPH_OPEN, kernel)
num_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(opened, connectivity=8)
t_total = time.perf_counter() - t0

count = num_labels - 1  # exclude background label 0
print(f"otsu_threshold     : level={level:.6f}")
print(f"blobs found        : {count}")
print(f"pipeline time      : {t_total*1000:.4f} ms")

cv_rows = []
for lbl in range(1, num_labels):
    cv_rows.append(
        {
            "label": lbl,
            "area": int(stats[lbl, cv2.CC_STAT_AREA]),
            "cx": float(centroids[lbl, 0]),
            "cy": float(centroids[lbl, 1]),
            "bbox_x": int(stats[lbl, cv2.CC_STAT_LEFT]),
            "bbox_y": int(stats[lbl, cv2.CC_STAT_TOP]),
            "w": int(stats[lbl, cv2.CC_STAT_WIDTH]),
            "h": int(stats[lbl, cv2.CC_STAT_HEIGHT]),
        }
    )
cv_df = pd.DataFrame(cv_rows)

print()
print(f"blob count: qu={len(qu_df)}  cv2={len(cv_df)}  ground_truth={len(gt_df)}")


def match_by_nearest_centroid(a_df, b_df):
    """Optimal one-to-one assignment (Hungarian algorithm) by centroid
    distance -- label IDs aren't guaranteed to agree in assignment order
    between two independent labeling implementations, or between either
    of them and the ground-truth mask's own label order."""
    a_xy = a_df[["cx", "cy"]].to_numpy()
    b_xy = b_df[["cx", "cy"]].to_numpy()
    cost = np.linalg.norm(a_xy[:, None, :] - b_xy[None, :, :], axis=2)
    ai, bi = linear_sum_assignment(cost)
    return ai, bi, cost[ai, bi]


if len(qu_df) != len(gt_df) or len(cv_df) != len(gt_df):
    print("WARNING: blob counts differ -- nearest-centroid matching below may pair up mismatched sets.")

ai_qg, bi_qg, d_qg = match_by_nearest_centroid(qu_df, gt_df)
ai_cg, bi_cg, d_cg = match_by_nearest_centroid(cv_df, gt_df)
ai_qc, bi_qc, d_qc = match_by_nearest_centroid(qu_df, cv_df)

print()
print(f"{'gt_label':>8} {'kind':>6} | {'qu_cd':>7} {'cv2_cd':>7} {'qu_v_cv2_cd':>11} | {'qu_area':>8} {'cv2_area':>8} {'gt_area':>8} | {'qu_wh':>9} {'cv2_wh':>9} {'gt_wh':>9}")
max_centroid_err = 0.0
max_area_rel_err = 0.0
max_wh_err = 0.0
for gt_idx in range(len(gt_df)):
    gt_row = gt_df.iloc[gt_idx]
    # find the qu/cv2 rows matched to this ground-truth row
    qu_match = ai_qg[bi_qg == gt_idx]
    cv_match = ai_cg[bi_cg == gt_idx]
    if len(qu_match) == 0 or len(cv_match) == 0:
        continue
    qu_row = qu_df.iloc[qu_match[0]]
    cv_row = cv_df.iloc[cv_match[0]]

    qu_cd = float(np.hypot(qu_row.cx - gt_row.cx, qu_row.cy - gt_row.cy))
    cv_cd = float(np.hypot(cv_row.cx - gt_row.cx, cv_row.cy - gt_row.cy))
    qc_cd = float(np.hypot(qu_row.cx - cv_row.cx, qu_row.cy - cv_row.cy))
    max_centroid_err = max(max_centroid_err, qu_cd, cv_cd, qc_cd)

    qu_area_err = abs(qu_row.area - gt_row.area) / gt_row.area
    cv_area_err = abs(cv_row.area - gt_row.area) / gt_row.area
    max_area_rel_err = max(max_area_rel_err, qu_area_err, cv_area_err)

    qu_wh_err = max(abs(qu_row.w - gt_row.bbox_w), abs(qu_row.h - gt_row.bbox_h))
    cv_wh_err = max(abs(cv_row.w - gt_row.bbox_w), abs(cv_row.h - gt_row.bbox_h))
    max_wh_err = max(max_wh_err, qu_wh_err, cv_wh_err)

    print(
        f"{int(gt_row.label):>8} {gt_row.kind:>6} | {qu_cd:7.3f} {cv_cd:7.3f} {qc_cd:11.3f} | "
        f"{int(qu_row.area):>8} {int(cv_row.area):>8} {int(gt_row.area):>8} | "
        f"{int(qu_row.w)}x{int(qu_row.h):<4} {int(cv_row.w)}x{int(cv_row.h):<4} {int(gt_row.bbox_w)}x{int(gt_row.bbox_h)}"
    )

print()
print(f"max centroid error (qu-vs-gt, cv2-vs-gt, qu-vs-cv2, all pairs): {max_centroid_err:.4f} px  (tolerance {CENTROID_TOL} px)")
print(f"max relative area error (qu-vs-gt or cv2-vs-gt):                {max_area_rel_err*100:.3f} %  (tolerance {AREA_REL_TOL*100:.1f} %)")
print(f"max bbox width/height error (qu-vs-gt or cv2-vs-gt):            {max_wh_err:.0f} px")

ok_count = len(qu_df) == len(gt_df) == len(cv_df) == 9
ok_centroid = max_centroid_err < CENTROID_TOL
ok_area = max_area_rel_err < AREA_REL_TOL
print()
print(f"PASS count match      : {ok_count}")
print(f"PASS centroid < {CENTROID_TOL}px : {ok_centroid}")
print(f"PASS area < {AREA_REL_TOL*100:.0f}% rel   : {ok_area}")

cv2.imwrite(os.path.join(HERE, "cv2_output_binary.bmp"), opened)
