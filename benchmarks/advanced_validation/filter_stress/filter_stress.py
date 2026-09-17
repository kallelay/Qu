# filter_stress.py -- scipy.signal reference for filter_stress.qu: SAME
# noisy_signal.csv, SAME filter design (4th-order Butterworth band-stop,
# [45, 55] Hz, Fs=2000 Hz), and SAME freqz frequency grid, so any
# difference in the results can only come from the two implementations'
# actual math, never from mismatched inputs or conventions.
#
# THREE convention gotchas handled explicitly -- two expected going in
# (from qu-core/src/filter.rs), one discovered WHILE building this script
# (see "IMPORTANT CORRECTION" below):
#
#   1. freqz's grid: Qu samples n points INCLUSIVE of Nyquist
#      (omega = pi*k/(n-1)). We build that exact same omega array with
#      np.linspace(0, pi, n) and pass it to sosfreqz as an explicit array
#      (worN=w), which evaluates at exactly those points -- a bare integer
#      worN would use a different (Nyquist-exclusive-by-default) grid and
#      produce a false mismatch.
#
#   2. filtfilt has NO edge padding in Qu: plain forward-then-backward
#      sosfilt, zero initial conditions, no array extension at the ends.
#
#   3. IMPORTANT CORRECTION to the naive fix for #2: calling
#      scipy.signal.sosfiltfilt(sos, x, padtype=None) does NOT actually
#      reproduce Qu's zero-initial-condition convention, even though
#      `padtype=None` disables the array padding. Reading scipy's own
#      source (scipy/signal/_signaltools.py, fn sosfiltfilt) shows it
#      UNCONDITIONALLY seeds both passes with a non-zero initial state --
#      `zi = sosfilt_zi(sos)`, then forward pass uses `zi * x[0]` and the
#      backward pass uses `zi * y[-1]` -- regardless of padtype. Only the
#      array-extension/edge-trim step is skipped when padtype=None; the
#      steady-state-scaled initial conditions are still applied on BOTH
#      passes. For an ordinary well-conditioned filter this initial-state
#      choice matters little and decays away quickly, so it's easy to miss
#      -- but for a narrow, high-Q filter like the one in this scenario
#      (poles at radius up to 0.9946 -- see the honest verdict in the
#      README) the transient it seeds barely decays within the whole
#      signal, and elementwise diffs against `sosfiltfilt(..., padtype=None)`
#      come out to ~0.4 max-abs / ~0.018 RMS -- looking exactly like a
#      correctness bug, but entirely an artifact of comparing against the
#      wrong scipy call.
#
#      The call that ACTUALLY matches Qu's zero-initial-condition
#      convention is two independent `sosfilt` calls (whose `zi` defaults
#      to None, i.e. all-zero state) with a reversal in between -- exactly
#      mirroring qu-core's own `fn filtfilt` body:
#          forward = sosfilt(sos, x)
#          backward = sosfilt(sos, forward[::-1])
#          result = backward[::-1]
#      This is what `qu_zero_ic_filtfilt()` below does, and it is the
#      correct reference for the elementwise diff -- not sosfiltfilt at all.
#
# Run: python benchmarks/advanced_validation/filter_stress/filter_stress.py

import os
import time
import numpy as np
import pandas as pd
from scipy.signal import butter, sosfreqz, sosfilt, sosfiltfilt


def qu_zero_ic_filtfilt(sos, x):
    """Reproduce Qu's fn filtfilt exactly: forward sosfilt (zero initial
    state), reverse, forward sosfilt again (zero initial state again),
    reverse -- NOT scipy's sosfiltfilt, which always seeds non-zero
    initial conditions even with padtype=None (see module docstring)."""
    forward = sosfilt(sos, x)
    backward = sosfilt(sos, forward[::-1])
    return backward[::-1].copy()

OUT = os.path.dirname(os.path.abspath(__file__))

Fs = 2000.0
N_GRID = 4001

x = pd.read_csv(os.path.join(OUT, "noisy_signal.csv"))["x"].to_numpy()
N = len(x)

# --- Design: SAME 4th-order Butterworth band-stop --------------------------
t0 = time.perf_counter()
sos = butter(4, [45, 55], btype="bandstop", fs=Fs, output="sos")
t_butter = time.perf_counter() - t0
print(f"scipy butter(4, [45,55], bandstop, fs={Fs}) -- sos shape: {sos.shape}  "
      f"time={t_butter*1000:.4f} ms")

# --- Frequency response, EXACT same omega grid as Qu (see NOTE 1) ---------
t0 = time.perf_counter()
w = np.linspace(0, np.pi, N_GRID)
_, H = sosfreqz(sos, worN=w)
t_freqz = time.perf_counter() - t0
mag_db = 20 * np.log10(np.abs(H))
mag_lin = np.abs(H)
phase_deg = np.angle(H) * 180 / np.pi
f_hz = w / np.pi * (Fs / 2)
print(f"scipy sosfreqz: n={N_GRID} points, matched omega grid  time={t_freqz*1000:.4f} ms")
i50 = round(50 / (Fs / 2) * (N_GRID - 1))
print(f"  gain at 50 Hz (stopband center) = {mag_db[i50]:.2f} dB (expect deep notch)")

# --- Zero-phase filtering, matching Qu's zero-initial-condition, no-padding
# convention (see gotcha #3 in the module docstring -- sosfiltfilt(padtype=
# None) is NOT the right call for this) --------------------------------------
t0 = time.perf_counter()
zerophase = qu_zero_ic_filtfilt(sos, x)
t_filtfilt = time.perf_counter() - t0
print(f"scipy double-sosfilt (zero IC, matches Qu's filtfilt): N={N}  time={t_filtfilt*1000:.4f} ms")

t_total = t_butter + t_freqz + t_filtfilt
print(f"total (butter+freqz+filtfilt) time = {t_total*1000:.4f} ms")

# --- FFT sanity check, same as the Qu side ----------------------------------
Xin = np.abs(np.fft.fft(x, N)) * 2 / N
Xout = np.abs(np.fft.fft(zerophase, N)) * 2 / N
d = Fs / N
i5 = round(5 / d)
i50b = round(50 / d)
i120 = round(120 / d)
print(f"FFT amplitude at 5 Hz:   input={Xin[i5]:.4f}  filtered={Xout[i5]:.4f}  (keep)")
print(f"FFT amplitude at 50 Hz:  input={Xin[i50b]:.4f}  filtered={Xout[i50b]:.4f}  (remove)")
print(f"FFT amplitude at 120 Hz: input={Xin[i120]:.4f}  filtered={Xout[i120]:.4f}  (keep)")

# =========================== Elementwise comparison =========================
print("")
print("=== Elementwise comparison against Qu's dumped outputs ===")

qu_ff = pd.read_csv(os.path.join(OUT, "qu_filtfilt.csv"))["y"].to_numpy()
diff_ff = qu_ff - zerophase
max_abs_ff = np.max(np.abs(diff_ff))
rms_ff = np.sqrt(np.mean(diff_ff ** 2))
print(f"filtfilt output: max_abs_diff={max_abs_ff:.3e}  rms_diff={rms_ff:.3e}  "
      f"(N={N})")

qu_fz = pd.read_csv(os.path.join(OUT, "qu_freqz.csv"))
d_db = qu_fz["mag_db"].to_numpy() - mag_db
d_lin = qu_fz["mag_lin"].to_numpy() - mag_lin
# phase compared mod 360 to avoid a spurious 360-degree wraparound diff
d_phase_raw = qu_fz["phase_deg"].to_numpy() - phase_deg
d_phase = (d_phase_raw + 180) % 360 - 180
print(f"freqz magnitude (dB):     max_abs_diff={np.max(np.abs(d_db)):.3e}")
print(f"freqz magnitude (linear): max_abs_diff={np.max(np.abs(d_lin)):.3e}")
print(f"freqz phase (deg, wrapped): max_abs_diff={np.max(np.abs(d_phase)):.3e}")

print("")
print("=== Speed ratio (Qu time / scipy time) ===")
# These are printed for the human report; the actual Qu-side numbers are
# read from filter_stress.qu's own stdout (both scripts print independently,
# same as ../peak_finding/ and ../curve_kalman/'s convention).
print(f"scipy total (butter+freqz+filtfilt) = {t_total*1000:.4f} ms")

# --- Also show the two "looks like a bug but isn't" convention gaps, for
# interest -- neither is a Qu correctness bug, both are documented above. --
zerophase_padded = sosfiltfilt(sos, x)  # scipy's own default: padtype="odd"
zerophase_padtype_none = sosfiltfilt(sos, x, padtype=None)  # still non-zero IC!
diff_padded = np.max(np.abs(zerophase_padded - zerophase))
diff_wrong_recipe = np.max(np.abs(zerophase_padtype_none - zerophase))
print("")
print("(for interest only, NOT Qu bugs -- both fully explained above/in README)")
print(f"  scipy default padtype='odd'      vs Qu-matching zero-IC: "
      f"max_abs_diff={diff_padded:.3e} (scipy's edge-padding transient "
      f"reduction, which Qu's filtfilt deliberately omits)")
print(f"  scipy sosfiltfilt(padtype=None)  vs Qu-matching zero-IC: "
      f"max_abs_diff={diff_wrong_recipe:.3e} (the 'naive fix' that still "
      f"seeds non-zero initial conditions internally -- gotcha #3 above)")
