"""bench_fft_1m.py -- numpy port of bench_fft_1m.qu.

Same exact-bin-sinusoid ground truth, same X[k] = (N*A/2)*exp(+i*phi)
formula (phase sign verified against a real FFT at N=1024 before this
file was written -- the naive guess had it backwards), same data-
outside-the-clock / one-warm-up-discarded / best-of-3 methodology.
"""

import time

import numpy as np


def make_signal(N):
    k1, A1, phi1 = 137, 1.0, 0.0
    k2, A2, phi2 = 9973, 2.5, np.pi / 3.0
    k3, A3, phi3 = round(N / 5.0) + 11, 0.7, -np.pi / 4.0

    n = np.arange(N, dtype=float)
    x = (
        A1 * np.cos(2.0 * np.pi * k1 * n / N)
        + A2 * np.cos(2.0 * np.pi * k2 * n / N + phi2)
        + A3 * np.cos(2.0 * np.pi * k3 * n / N + phi3)
    )
    return dict(x=x, k1=k1, A1=A1, phi1=phi1, k2=k2, A2=A2, phi2=phi2, k3=k3, A3=A3, phi3=phi3)


def check_accuracy(X, sig, N):
    bins = [sig["k1"], N - sig["k1"], sig["k2"], N - sig["k2"], sig["k3"], N - sig["k3"]]
    amps = [sig["A1"], sig["A1"], sig["A2"], sig["A2"], sig["A3"], sig["A3"]]
    phis = [sig["phi1"], -sig["phi1"], sig["phi2"], -sig["phi2"], sig["phi3"], -sig["phi3"]]

    max_signal_err = 0.0
    for k, a, phi in zip(bins, amps, phis):
        expected = (N * a / 2.0) * (np.cos(phi) + 1j * np.sin(phi))
        err = abs(X[k] - expected)
        max_signal_err = max(max_signal_err, err)

    max_noise = 0.0
    bin_set = set(bins)
    for k in range(0, N, 997):
        if k not in bin_set:
            max_noise = max(max_noise, abs(X[k]))

    return max_signal_err, max_noise


def run_case(N, label):
    sig = make_signal(N)

    X = np.fft.fft(sig["x"])  # warm-up, discarded
    best_t = float("inf")
    xr = None
    for _ in range(3):
        t0 = time.perf_counter()
        X = np.fft.fft(sig["x"])
        xr = np.fft.ifft(X)
        t = time.perf_counter() - t0
        best_t = min(best_t, t)

    max_signal_err, max_noise = check_accuracy(X, sig, N)
    roundtrip_err = np.max(np.abs(xr - sig["x"]))

    print(
        f"{label} (N={N}): time={best_t:.6f} s   "
        f"max signal-bin error={max_signal_err:.2e}   "
        f"noise floor={max_noise:.2e}   "
        f"roundtrip |ifft(fft(x))-x|={roundtrip_err:.2e}"
    )


run_case(1048576, "power-of-two  ")
run_case(1000000, "non-power-of-2")
