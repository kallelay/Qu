# fluid_simulation.py — same 2D diffusion (heat) equation, explicit FTCS
# scheme, IDENTICAL grid/parameters/initial-boundary conditions as
# catalog/qu_fluid_simulation.qu, so the two results are directly comparable.
#
# The one deliberate implementation difference: the Laplacian stencil update
# is expressed as vectorized NumPy array slicing (no Python-level loop over
# grid cells), instead of Qu's nested `for i / for j` scalar stencil loop —
# this is the actual point of the comparison (see that file's benchmarks
# README entry: does Qu's interpreted for-loop or NumPy's vectorized slicing
# win on a finite-difference stencil).
#
# Run standalone: python fluid_simulation.py
# Memory is sampled periodically via psutil (peak RSS in MB), matching the
# Qu side's `qu run --profile` peak-RSS report methodology.

import time
import threading
import numpy as np
import psutil

# --- Peak-RSS sampler (background thread), symmetric with Qu's --profile ---
_proc = psutil.Process()
_peak_rss_bytes = [0]
_stop_sampling = threading.Event()


def _sample_rss():
    while not _stop_sampling.is_set():
        rss = _proc.memory_info().rss
        if rss > _peak_rss_bytes[0]:
            _peak_rss_bytes[0] = rss
        _stop_sampling.wait(0.005)


_sampler = threading.Thread(target=_sample_rss, daemon=True)
_sampler.start()

# --- Parameters (identical to qu_fluid_simulation.qu) -----------------------
Lx = 3.0
Ly = 3.0
nx = 91
ny = 91
D = 0.02
t_final = 1.0
cfl_factor = 0.4

x0 = 1.5
y0 = 1.5
sigma0 = 0.15
A0 = 1.0

dx = Lx / (nx - 1)
dy = Ly / (ny - 1)
dt_max = (dx ** 2 * dy ** 2) / (2 * D * (dx ** 2 + dy ** 2))
dt = cfl_factor * dt_max
nsteps = int(np.ceil(t_final / dt))
dt = t_final / nsteps  # land exactly on t_final after nsteps

print(f"[Python] grid={nx}x{ny}, dx={dx:.5f}, dy={dy:.5f}")
print(f"[Python] dt={dt:.6f} (dt_max={dt_max:.6f}, safety factor={cfl_factor}), nsteps={nsteps}")

# --- Initial condition: Gaussian blob ---------------------------------------
xs = np.arange(nx) * dx
ys = np.arange(ny) * dy
XI, YJ = np.meshgrid(xs, ys, indexing="ij")  # match Qu's U[i,j] = f(i*dx, j*dy)

R2 = (XI - x0) ** 2 + (YJ - y0) ** 2
U = A0 * np.exp(-R2 / (2 * sigma0 ** 2))

mass0 = U.sum() * dx * dy

# --- Time-stepping: explicit FTCS diffusion, vectorized slicing ------------
t0 = time.perf_counter()
for _ in range(nsteps):
    Uold = U.copy()
    d2x = (Uold[2:, 1:-1] - 2 * Uold[1:-1, 1:-1] + Uold[:-2, 1:-1]) / dx ** 2
    d2y = (Uold[1:-1, 2:] - 2 * Uold[1:-1, 1:-1] + Uold[1:-1, :-2]) / dy ** 2
    U[1:-1, 1:-1] = Uold[1:-1, 1:-1] + D * dt * (d2x + d2y)
elapsed_ms = (time.perf_counter() - t0) * 1000

mass_final = U.sum() * dx * dy

# --- Verification: analytic Gaussian reference at t_final -------------------
sigma_final_sq = sigma0 ** 2 + 2 * D * t_final
amp_final = A0 * (sigma0 ** 2 / sigma_final_sq)
Uexact = amp_final * np.exp(-R2 / (2 * sigma_final_sq))

diff = U - Uexact
max_abs_err = np.max(np.abs(diff))
rms_err = np.sqrt(np.mean(diff ** 2))
max_field_val = np.max(Uexact)

print(f"[Python] elapsed={elapsed_ms:.1f} ms for {nsteps} steps on a {nx}x{ny} grid")
print(f"[Python] mass: initial={mass0:.6f}, final={mass_final:.6f}, relative drift={(mass_final - mass0) / mass0 * 100:.4f}%")
print(f"[Python] vs analytic Gaussian at t={t_final}: max|err|={max_abs_err:.6e}, rms(err)={rms_err:.6e}, peak field value={max_field_val:.6f}")
print(f"[Python] max|err| as % of peak field value: {max_abs_err / max_field_val * 100:.4f}%")

if max_abs_err / max_field_val < 0.02:
    print("[Python] PASS: numeric field matches analytic Gaussian diffusion to <2% of peak amplitude")
else:
    print("[Python] FAIL: numeric field deviates from analytic Gaussian diffusion by >=2% of peak amplitude")

# --- Cross-check against Qu's own final field, if it dumped one ------------
import os
qu_csv = os.path.join(os.path.dirname(__file__), "qu_fluid_field.csv")
if os.path.exists(qu_csv):
    U_qu = np.loadtxt(qu_csv, delimiter=",")
    cross_diff = U - U_qu
    print(f"[Python] vs Qu's own final field ({qu_csv}): max|diff|={np.max(np.abs(cross_diff)):.6e}, rms(diff)={np.sqrt(np.mean(cross_diff ** 2)):.6e}")

_stop_sampling.set()
_sampler.join(timeout=1.0)
peak_rss_mb = _peak_rss_bytes[0] / (1024 * 1024)
print(f"[Python] peak RSS (sampled) = {peak_rss_mb:.1f} MB")
