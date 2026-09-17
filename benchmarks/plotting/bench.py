"""Plotting: matplotlib side of the Qu-vs-matplotlib figure benchmark.

Same figures, same data, same output formats as bench.qu beside this file.
See bench.qu's header for why this scenario exists and why its TIMINGS ARE
PROVISIONAL (machine at 41-71% CPU during the run).

matplotlib's import is timed and reported separately rather than folded into
the first figure. Import cost is real -- a user waits for it -- but burying
it inside a per-figure number is exactly the cold-start distortion that
produced false claims in the existing suite, so it is named rather than
hidden.
"""

import os
import time

t = time.perf_counter()
import matplotlib

matplotlib.use("Agg")  # no display; the fair comparison for file output
import matplotlib.pyplot as plt
import numpy as np

t_import = time.perf_counter() - t
print(f"matplotlib import (once, not per figure): {t_import:.4f} s")

os.makedirs("plots", exist_ok=True)
SIZES = [1000, 100000, 1000000]

for n in SIZES:
    x = np.linspace(0, 100, n)
    y = np.sin(x * 0.1) + 0.3 * np.sin(x * 0.73)

    t = time.perf_counter()
    fig, ax = plt.subplots()
    ax.plot(x, y)
    ax.set_xlabel("Time (s)")
    ax.set_ylabel("Amplitude (V)")
    ax.set_title(f"scaling check, n = {n}")
    t_build = time.perf_counter() - t

    t = time.perf_counter()
    fig.savefig(f"plots/py_plot_{n}.svg")
    t_svg = time.perf_counter() - t

    t = time.perf_counter()
    fig.savefig(f"plots/py_plot_{n}.pdf")
    t_pdf = time.perf_counter() - t

    plt.close(fig)
    print(f"n = {n:8d}  build {t_build:.4f} s   svg {t_svg:.4f} s   pdf {t_pdf:.4f} s")

print("")
print("--- multi-panel (2x2), n = 100000 per panel ---")
n2 = 100000
x2 = np.linspace(0, 100, n2)
t = time.perf_counter()
fig, axs = plt.subplots(2, 2)
axs[0, 0].plot(x2, np.sin(x2 * 0.1))
axs[0, 0].set_title("a")
axs[0, 1].plot(x2, np.cos(x2 * 0.1))
axs[0, 1].set_title("b")
axs[1, 0].plot(x2, np.sin(x2 * 0.3))
axs[1, 0].set_title("c")
axs[1, 1].plot(x2, np.cos(x2 * 0.7))
axs[1, 1].set_title("d")
t_grid = time.perf_counter() - t

t = time.perf_counter()
fig.savefig("plots/py_grid.svg")
t_grid_svg = time.perf_counter() - t
plt.close(fig)
print(f"2x2 grid      build {t_grid:.4f} s   svg {t_grid_svg:.4f} s")
