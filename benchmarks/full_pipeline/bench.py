"""bench.py -- full signal-processing pipeline, Python port of bench.qu.

Same dataset, same seven stages (load, clean, filter, spectral, time-freq,
feature extraction, export), timed the same way (wall clock per stage,
summed for the total) for a direct Qu-vs-Python comparison of a realistic
chained workflow, not isolated kernels.

Run: python benchmarks/full_pipeline/bench.py
"""
import os
import time

import numpy as np
import pandas as pd
from scipy.signal import butter, sosfilt, find_peaks, stft

HERE = os.path.dirname(os.path.abspath(__file__))
CSV_PATH = os.path.join(HERE, "pipeline_signal.csv")
FEATURES_PATH = os.path.join(HERE, "pipeline_features.csv")

Fs = 10000
N = 200_000

# --- acquire + write CSV (untimed prep) --------------------------------
rng = np.random.default_rng(11)
t = np.arange(N) / Fs
x_raw = (
    np.sin(2 * np.pi * 60 * t)
    + 0.6 * np.sin(2 * np.pi * 440 * t)
    + 0.3 * np.sin(2 * np.pi * 1800 * t)
    + 0.15 * rng.standard_normal(N)
    + 0.5
)
pd.DataFrame({"t": t, "x": x_raw}).to_csv(CSV_PATH, index=False)


def spectral_entropy(x, nfft, hop):
    _, _, Zxx = stft(x, nperseg=nfft, noverlap=nfft - hop, boundary=None)
    power = np.abs(Zxx) ** 2
    power_sum = power.sum(axis=0, keepdims=True)
    power_sum[power_sum == 0] = 1.0
    p = power / power_sum
    p = np.where(p > 0, p, 1.0)  # avoid log(0); those bins contribute 0
    ent = -(p * np.log2(p)).sum(axis=0) / np.log2(power.shape[0])
    return ent


def short_time_energy(x, win, hop):
    n_frames = max(0, (len(x) - win) // hop + 1)
    out = np.empty(n_frames)
    for i in range(n_frames):
        seg = x[i * hop : i * hop + win]
        out[i] = np.sum(seg**2)
    return out


# --- stage 1: load --------------------------------------------------------
t0 = time.perf_counter()
df = pd.read_csv(CSV_PATH)
xs = df["x"].to_numpy()
t_load = time.perf_counter() - t0

# --- stage 2: clean (remove DC offset) ------------------------------------
t0 = time.perf_counter()
xs_clean = xs - xs.mean()
t_clean = time.perf_counter() - t0

# --- stage 3: filter (bandpass, isolate 200-1000 Hz) ----------------------
t0 = time.perf_counter()
bp = butter(4, [200, 1000], btype="band", fs=Fs, output="sos")
xs_filt = sosfilt(bp, xs_clean)
t_filter = time.perf_counter() - t0

# --- stage 4: spectral analysis (rfft magnitude + peak-finding) ----------
t0 = time.perf_counter()
spectrum = np.abs(np.fft.rfft(xs_filt))
peaks, _ = find_peaks(spectrum, height=spectrum.max() * 0.1, distance=10)
t_spectral = time.perf_counter() - t0

# --- stage 5: time-frequency (spectrogram, spectral entropy) -------------
t0 = time.perf_counter()
_, _, Zxx = stft(xs_filt, nperseg=1024, noverlap=1024 - 512, boundary=None)
spec_img = np.abs(Zxx)
entropy = spectral_entropy(xs_filt, 1024, 512)
t_timefreq = time.perf_counter() - t0

# --- stage 6: feature extraction (STE, energy, crest factor, RMS) --------
t0 = time.perf_counter()
energy_frames = short_time_energy(xs_filt, 1024, 512)
sig_energy = np.sum(xs_filt**2)
sig_rms = np.sqrt(np.mean(xs_filt**2))
sig_crest = np.max(np.abs(xs_filt)) / sig_rms
t_features = time.perf_counter() - t0

# --- stage 7: export extracted features -----------------------------------
t0 = time.perf_counter()
features = pd.DataFrame(
    [{"rms": sig_rms, "crest_factor": sig_crest, "energy": sig_energy, "n_peaks": len(peaks), "mean_entropy": entropy.mean()}]
)
features.to_csv(FEATURES_PATH, index=False)
t_export = time.perf_counter() - t0

t_total = t_load + t_clean + t_filter + t_spectral + t_timefreq + t_features + t_export

print(f"load       : {t_load:.4f} s")
print(f"clean      : {t_clean:.4f} s")
print(f"filter     : {t_filter:.4f} s")
print(f"spectral   : {t_spectral:.4f} s")
print(f"timefreq   : {t_timefreq:.4f} s")
print(f"features   : {t_features:.4f} s")
print(f"export     : {t_export:.4f} s")
print(f"TOTAL      : {t_total:.4f} s")
print()
print(f"rms={sig_rms:.4f} crest={sig_crest:.4f} energy={sig_energy:.2f} n_peaks={len(peaks)} mean_entropy={entropy.mean():.4f}")
