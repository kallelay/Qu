# scenario2_cnn_make_dataset.py -- generates the shared synthetic image
# dataset for Scenario 2 (Image/CNN), ONCE, so Qu/PyTorch/Keras all train
# on the exact same images. Deterministic (numpy RandomState(7)); rerunning
# reproduces the same files.
#
# 16x16 single-channel images, 3 shape classes (square / plus / diagonal
# bar), each with random position jitter + Gaussian pixel noise -- a
# similarly-scaled task to `simple_cnn`'s own square-vs-plus acceptance
# test (`engine/crates/qu-interp/tests/acceptance.rs`,
# `simple_cnn_fit_learns_square_vs_plus_pattern`, 6x6 images/2 classes/24
# samples), extended to a real "few hundred samples" scale and a 3rd class
# per the task spec.
#
# Two kinds of output, deliberately redundant, so both languages see
# byte-identical (well, float32-CSV-precision-identical) pixels without
# either language having to parse the other's native format:
#   1. cnn_{train,test}_images.npz -- for scenario2_cnn_pytorch.py /
#      scenario2_cnn_keras.py to load directly (exact floats).
#   2. cnn_{train,test}_images.csv -- Qu has no .npz reader, so each split's
#      images are stacked VERTICALLY into one plain, headerless CSV (image i
#      occupies rows [i*16, (i+1)*16), 16 columns -- the image's OWN natural
#      2-D row/col layout, no flatten-order ambiguity to get wrong across
#      languages). scenario2_cnn_qu.qu reads this with `read_csv(...,
#      headers=false)` and slices out each image with `Xall[i*16:(i+1)*16, :]`
#      (Qu's `a:b` slice is half-open, like Python/numpy).
#   labels: cnn_{train,test}_labels.csv, one `y` column, 0/1/2.
#
# Run once: python benchmarks/multi_scenario/scenario2_cnn_make_dataset.py

import os
import numpy as np
from sklearn.model_selection import train_test_split

OUT = os.path.dirname(os.path.abspath(__file__))

H, W = 16, 16
N_PER_CLASS = 100  # 300 total, "a few hundred samples" per the task spec
NOISE_STD = 0.12
rng = np.random.RandomState(7)


def make_square(rng):
    img = np.zeros((H, W))
    size = 5
    top = rng.randint(2, H - size - 2)
    left = rng.randint(2, W - size - 2)
    img[top:top + size, left:left + size] = 1.0
    return img


def make_plus(rng):
    img = np.zeros((H, W))
    cy = rng.randint(5, H - 5)
    cx = rng.randint(5, W - 5)
    arm = 4
    img[cy - arm:cy + arm + 1, cx] = 1.0
    img[cy, cx - arm:cx + arm + 1] = 1.0
    return img


def make_diagonal(rng):
    img = np.zeros((H, W))
    shift = rng.randint(-3, 4)
    flip = rng.choice([False, True])
    for i in range(H):
        j = i + shift
        if flip:
            j = (W - 1 - i) + shift
        for dj in (0, 1):
            jj = j + dj
            if 0 <= jj < W:
                img[i, jj] = 1.0
    return img


makers = [make_square, make_plus, make_diagonal]

images = []
labels = []
for label, maker in enumerate(makers):
    for _ in range(N_PER_CLASS):
        img = maker(rng) + rng.normal(0.0, NOISE_STD, size=(H, W))
        images.append(img.astype(np.float32))
        labels.append(label)

images = np.stack(images, axis=0)  # (N, H, W)
labels = np.array(labels, dtype=int)

Xtr, Xte, ytr, yte = train_test_split(
    images, labels, test_size=0.2, random_state=7, stratify=labels
)
print(f"images: train {Xtr.shape}, test {Xte.shape}, classes train={np.bincount(ytr)} test={np.bincount(yte)}")


def write_stacked_csv(path, X):
    n = X.shape[0]
    flat = X.reshape(n * H, W)
    np.savetxt(path, flat, delimiter=",", fmt="%.8g")


def write_labels_csv(path, y):
    with open(path, "w", newline="") as f:
        f.write("y\n")
        for v in y:
            f.write(f"{int(v)}\n")


write_stacked_csv(os.path.join(OUT, "cnn_train_images.csv"), Xtr)
write_stacked_csv(os.path.join(OUT, "cnn_test_images.csv"), Xte)
write_labels_csv(os.path.join(OUT, "cnn_train_labels.csv"), ytr)
write_labels_csv(os.path.join(OUT, "cnn_test_labels.csv"), yte)

np.savez(os.path.join(OUT, "cnn_train_images.npz"), X=Xtr, y=ytr)
np.savez(os.path.join(OUT, "cnn_test_images.npz"), X=Xte, y=yte)

print("wrote cnn_{train,test}_images.{csv,npz} + cnn_{train,test}_labels.csv")
