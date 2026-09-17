"""bridge_shm.py -- NumPy/SciPy equivalent of catalog/qu_bridge_shm.qu.

Same synthetic bridge-SHM simulation (identical parameters, identical
per-sensor noise via a Python re-implementation of Qu's own PRNG so the
recorded data is bit-for-bit comparable) and the same spectral-analysis
damage-detection + TDOA sanity-check pipeline, built on
`scipy.signal` (periodogram, hilbert, correlate, find_peaks) as the
"advanced Python" side of the comparison. See `catalog/qu_bridge_shm.qu`
for the full explanation of what's simulated and why -- not repeated here.
"""

import math
import threading
import time

import numpy as np
import psutil
from scipy.signal import periodogram, hilbert, correlate, find_peaks

MASK = (1 << 64) - 1


class QuRng:
    """Exact replica of Qu's `randn(..., seed=)` PRNG (splitmix64 -> Box-
    Muller, cosine branch only) from `engine/crates/qu-interp/src/lib.rs`'s
    `Rng` struct -- verified bit-for-bit against `qu.exe` for `randn(1, 5,
    seed=42)` before use here, so `Y` below is the exact same recording
    `qu_bridge_shm.qu` analyzes, not just a statistically similar one.
    """

    def __init__(self, seed: int):
        self.state = (seed + 0x9E3779B97F4A7C15) & MASK

    def next_u64(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def uniform(self) -> float:
        return (self.next_u64() >> 11) / float(1 << 53)

    def normal(self) -> float:
        u1 = max(self.uniform(), 1e-12)
        u2 = self.uniform()
        return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)

    def normal_vec(self, n: int) -> np.ndarray:
        return np.array([self.normal() for _ in range(n)], dtype=np.float64)


# --- Peak-RSS sampler (mirrors Qu CLI's `--profile` peak-RSS report) ------
class PeakRssSampler:
    def __init__(self, interval=0.005):
        self.interval = interval
        self.peak = 0
        self._stop = threading.Event()
        self._proc = psutil.Process()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        while not self._stop.is_set():
            rss = self._proc.memory_info().rss
            if rss > self.peak:
                self.peak = rss
            self._stop.wait(self.interval)

    def start(self):
        self.peak = self._proc.memory_info().rss
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._thread.join()
        return self.peak


sampler = PeakRssSampler()
sampler.start()

# ---- Bridge & sensor geometry --------------------------------------------
L = 40.0
xs = np.array([4.0, 10.0, 16.0, 24.0, 30.0, 36.0])
n_sensors = len(xs)

fs = 200.0
duration = 120.0
N = round(duration * fs)
N_half = N // 2
t = np.arange(N) / fs
t_local = np.arange(N_half) / fs

t_anomaly = 60.0

# ---- Modal content (standing waves, no propagation delay) ----------------
mode_n = [1, 2, 3]
f_healthy = [2.10, 5.80, 11.30]
f_damaged = [1.98, 5.80, 11.30]
A_mode = [1.0, 0.6, 0.35]

# ---- Moving-load traveling bump (separate physics, for TDOA) -------------
v_vehicle = 20.0
t_events = [20.0, 90.0]
bump_width = 0.10
bump_amp = 8.0

noise_sigma = 0.05
noise_seed_base = 9001

# ---- Synthesize each sensor's recording -----------------------------------
t0 = time.perf_counter()

Y = np.zeros((n_sensors, N))
for si in range(n_sensors):
    x_s = xs[si]

    y_before = np.zeros(N_half)
    y_after = np.zeros(N_half)
    for mi in range(len(mode_n)):
        n = mode_n[mi]
        phi = math.sin(n * math.pi * x_s / L)
        amp = A_mode[mi]
        y_before = y_before + amp * phi * np.sin(2 * math.pi * f_healthy[mi] * t_local)
        y_after = y_after + amp * phi * np.sin(2 * math.pi * f_damaged[mi] * t_local)
    y_modal = np.concatenate([y_before, y_after])

    bump = np.zeros(N)
    for ei in range(len(t_events)):
        t_arrive = t_events[ei] + x_s / v_vehicle
        bump = bump + bump_amp * np.exp(-((t - t_arrive) ** 2) / (2 * bump_width ** 2))

    rng = QuRng(noise_seed_base + si)
    noise = noise_sigma * rng.normal_vec(N)

    Y[si, :] = y_modal + bump + noise

print(f"[Python] synthesized {n_sensors} sensors x {N} samples ({duration} s at {fs} Hz)")

# ---- Step 1-2: per-sensor spectral analysis + frequency-shift detection --
band_lo = 1.5
band_hi = 2.5
df = fs / N_half
idx_lo = round(band_lo / df)
idx_hi = round(band_hi / df)

shift_threshold = 0.05
shifts = np.zeros(n_sensors)

print()
print("=== Step 1-2: modal frequency-shift damage detection ===")
for si in range(n_sensors):
    before = Y[si, 0:N_half]
    after = Y[si, N_half:N]

    _, psd_before = periodogram(before, fs=fs, window="hann", detrend=False, scaling="density")
    _, psd_after = periodogram(after, fs=fs, window="hann", detrend=False, scaling="density")

    band_before = psd_before[idx_lo:idx_hi]
    band_after = psd_after[idx_lo:idx_hi]

    peak_before = idx_lo + int(np.argmax(band_before))
    peak_after = idx_lo + int(np.argmax(band_after))

    f_before = peak_before * df
    f_after = peak_after * df
    shift = f_after - f_before
    shifts[si] = shift

    flag = "damage suspected" if abs(shift) > shift_threshold else "no significant shift"
    print(
        f"sensor {si} (x={xs[si]:.0f} m): mode-1 peak before={f_before:.3f} Hz, "
        f"after={f_after:.3f} Hz, shift={shift:.3f} Hz -> {flag}"
    )

mean_shift = float(np.mean(shifts))
print()
print(f"mean detected mode-1 shift across sensors: {mean_shift:.3f} Hz "
      f"(injected: {f_damaged[0] - f_healthy[0]:.3f} Hz)")

expected_shift = f_damaged[0] - f_healthy[0]
if abs(mean_shift - expected_shift) < 0.03 and mean_shift < -shift_threshold:
    print("CHECK PASSED: detected frequency shift matches the injected damage within tolerance.")
else:
    print("CHECK FAILED: detected shift does not match the injected damage.")

# ---- Step 3: TDOA sanity check via cross-correlation ---------------------
print()
print("=== Step 3: TDOA sanity check (vehicle-bump cross-correlation) ===")

ref = 0

# Same reasoning as the Qu script: cross-correlating raw (or even envelope)
# sensor-vs-sensor waveforms lets the strong, purely periodic, undelayed
# modal content dominate and lock onto the wrong lag. Match-filter each
# sensor's Hilbert envelope against a clean Gaussian template of the known
# pulse shape instead; the shared template cancels out of the sensor-to-
# sensor difference.
Env = np.zeros((n_sensors, N))
for si in range(n_sensors):
    Env[si, :] = np.abs(hilbert(Y[si, :]))

env_ref = Env[ref, :]

detect_threshold = np.mean(env_ref) + 4 * np.std(env_ref)
peak_locations, _ = find_peaks(env_ref, height=detect_threshold, distance=round(5 * fs))
print(f"detected {len(peak_locations)} vehicle-crossing event(s) on the reference sensor")

win_pre = 1.0
win_post = 3.0
delay_errors = []

win_len_full = round((win_pre + win_post) * fs)
tau_template = np.arange(win_len_full) / fs
template = np.exp(-((tau_template - win_pre) ** 2) / (2 * bump_width ** 2))
template = template - np.mean(template)

for ev in range(len(peak_locations)):
    t_ref_arrive = peak_locations[ev] / fs
    lo = round((t_ref_arrive - win_pre) * fs)
    hi = round((t_ref_arrive + win_post) * fs)
    lo = max(lo, 0)
    hi = min(hi, N)

    w_ref = Env[ref, lo:hi]
    w_ref = w_ref - np.mean(w_ref)

    c_ref = correlate(template, w_ref, mode="full")
    peak_ref = int(np.argmax(c_ref))

    print()
    print(f"event at t~{t_ref_arrive:.2f} s (window [{lo/fs:.2f}, {hi/fs:.2f}] s):")
    for si in range(1, n_sensors):
        w_i = Env[si, lo:hi]
        w_i = w_i - np.mean(w_i)

        c_i = correlate(template, w_i, mode="full")
        peak_i = int(np.argmax(c_i))
        lag_samples = peak_ref - peak_i
        tau_est = lag_samples / fs

        tau_true = (xs[si] - xs[ref]) / v_vehicle
        err = tau_est - tau_true
        delay_errors.append(err)
        print(
            f"  sensor {si} (x={xs[si]:.0f} m): tau_est={tau_est:.4f} s, "
            f"tau_true={tau_true:.4f} s, err={err*1000:.2f} ms"
        )

tdoa_tolerance_samples = 8
delay_errors = np.array(delay_errors)
max_abs_err_samples = np.max(np.abs(delay_errors)) * fs
print()
print(f"max |TDOA error| across sensor pairs, all events: {max_abs_err_samples:.2f} samples "
      f"({np.max(np.abs(delay_errors))*1000:.2f} ms)")
if max_abs_err_samples < tdoa_tolerance_samples:
    print(f"CHECK PASSED: TDOA estimates match the known geometric delay within {tdoa_tolerance_samples} samples.")
else:
    print(f"CHECK FAILED: TDOA estimates deviate from the known geometric delay by more than {tdoa_tolerance_samples} samples.")

elapsed_ms = (time.perf_counter() - t0) * 1000
print()
print(f"[Python] elapsed={elapsed_ms:.2f} ms for simulate+analyze {n_sensors} sensors x {N} samples")

peak_rss = sampler.stop()
print(f"[Python] peak RSS = {peak_rss / (1024*1024):.1f} MB")

# ---- Plot (not included in the timed comparison) --------------------------
try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, axes = plt.subplots(2, 2, figsize=(11, 8))

    axes[0, 0].plot(t[:2000], Y[0, :2000], label=f"sensor 0 (x={xs[0]:.0f} m)", color="blue")
    axes[0, 0].plot(t[:2000], Y[3, :2000], label=f"sensor 3 (x={xs[3]:.0f} m)", color="red")
    axes[0, 0].set_xlabel("Time (s)")
    axes[0, 0].set_ylabel("Amplitude")
    axes[0, 0].set_title("Time-domain traces (first 10 s)")
    axes[0, 0].legend()

    t_ev = t_events[0]
    lo2 = round((t_ev - 0.5) * fs)
    hi2 = round((t_ev + 3.0) * fs)
    axes[0, 1].plot(t[lo2:hi2], Y[0, lo2:hi2], label="sensor 0", color="blue")
    axes[0, 1].plot(t[lo2:hi2], Y[5, lo2:hi2], label=f"sensor 5 (x={xs[5]:.0f} m)", color="green")
    axes[0, 1].set_xlabel("Time (s)")
    axes[0, 1].set_ylabel("Amplitude")
    axes[0, 1].set_title("Vehicle-crossing bump: arrival delay")
    axes[0, 1].legend()

    sensor_plot = 2
    before_p = Y[sensor_plot, 0:N_half]
    after_p = Y[sensor_plot, N_half:N]
    freq_axis_b, psd_b = periodogram(before_p, fs=fs, window="hann", detrend=False, scaling="density")
    _, psd_a = periodogram(after_p, fs=fs, window="hann", detrend=False, scaling="density")
    axes[1, 0].plot(freq_axis_b[:300], psd_b[:300], label="before (healthy)", color="blue")
    axes[1, 0].plot(freq_axis_b[:300], psd_a[:300], label="after (damaged)", color="red")
    axes[1, 0].set_xlabel("Frequency (Hz)")
    axes[1, 0].set_ylabel("PSD")
    axes[1, 0].set_title(f"sensor {sensor_plot}: before/after PSD (mode-1 shift)")
    axes[1, 0].legend()

    axes[1, 1].specgram(Y[sensor_plot, :], NFFT=4096, Fs=fs, noverlap=3072)
    axes[1, 1].set_ylim(0, 15)
    axes[1, 1].set_title(f"sensor {sensor_plot}: spectrogram (mode-1 shift at t={t_anomaly:.0f} s)")
    axes[1, 1].set_xlabel("Time (s)")
    axes[1, 1].set_ylabel("Frequency (Hz)")

    fig.tight_layout()
    fig.savefig("bridge_shm_python.svg")
    print("[Python] wrote bridge_shm_python.svg")
except ImportError:
    print("[Python] matplotlib not available, skipping plot")
