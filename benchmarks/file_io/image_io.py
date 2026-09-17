"""image_io.py -- image save/load round trip benchmark, Python (Pillow) side.

Same shape as image_io.qu: 2000x2000 grayscale-from-random image.

Run: python benchmarks/file_io/image_io.py
"""
import time
import numpy as np
from PIL import Image

W = H = 2000
bmp_path = "benchmarks/file_io/py_random.bmp"
png_path = "benchmarks/file_io/py_random.png"

rng = np.random.default_rng(5)
m = (rng.random((H, W)) * 255).round().clip(0, 255).astype(np.uint8)
img = Image.fromarray(m, mode="L").convert("RGB")

t0 = time.perf_counter()
img.save(bmp_path)
t_save_bmp = time.perf_counter() - t0
print(f"PIL save .bmp ({W}x{H}) : {t_save_bmp:.4f} s")

t0 = time.perf_counter()
img.save(png_path)
t_save_png = time.perf_counter() - t0
print(f"PIL save .png ({W}x{H}) : {t_save_png:.4f} s")

t0 = time.perf_counter()
img2 = Image.open(bmp_path)
img2.load()
t_load_bmp = time.perf_counter() - t0
print(f"PIL load .bmp ({W}x{H}) : {t_load_bmp:.4f} s")

print(f"width={img2.width} height={img2.height}")
g2 = np.asarray(img2.convert("L"), dtype=np.float64)
print(f"mean pixel (round-tripped) = {g2.mean():.4f}")
print(f"mean(original matrix)      = {m.astype(np.float64).mean():.4f}")
