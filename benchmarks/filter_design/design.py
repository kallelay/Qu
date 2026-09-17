"""design.py -- filter DESIGN benchmark, Python port of design.qu.

Same scenarios (butter/cheby1/cheby2/ellip at order 4/8/16, fir1/firls at
order 64/256/1024), same repeated-loop-average timing methodology, for a
direct Qu-vs-scipy comparison of just the coefficient-computation step.

Run: python benchmarks/filter_design/design.py
"""
import time

from scipy.signal import butter, cheby1, cheby2, ellip, firwin, firls

Fs = 10000
cutoff = 500  # Hz, lowpass -- 0.1 * Nyquist (Nyquist = Fs/2 = 5000 Hz)
rp = 1        # dB passband ripple (cheby1, ellip)
rs = 40       # dB stopband attenuation (cheby2, ellip)


def avg_time(reps, fn):
    t0 = time.perf_counter()
    for _ in range(reps):
        fn()
    return (time.perf_counter() - t0) / reps


print("=== IIR filter design: butter / cheby1 / cheby2 / ellip, order 4/8/16 ===")

for order, reps in ((4, 5000), (8, 2000), (16, 500)):
    t = avg_time(reps, lambda order=order: butter(order, cutoff, btype="low", fs=Fs, output="sos"))
    print(f"butter  order={order:<3d}: {t*1e6:.2f} us/call  ({reps} reps)")
    t = avg_time(reps, lambda order=order: cheby1(order, rp, cutoff, btype="low", fs=Fs, output="sos"))
    print(f"cheby1  order={order:<3d}: {t*1e6:.2f} us/call  ({reps} reps)")
    t = avg_time(reps, lambda order=order: cheby2(order, rs, cutoff, btype="low", fs=Fs, output="sos"))
    print(f"cheby2  order={order:<3d}: {t*1e6:.2f} us/call  ({reps} reps)")
    t = avg_time(reps, lambda order=order: ellip(order, rp, rs, cutoff, btype="low", fs=Fs, output="sos"))
    print(f"ellip   order={order:<3d}: {t*1e6:.2f} us/call  ({reps} reps)")

print()
print("=== FIR filter design: fir1(firwin) / firls, order 64/256/1024 (taps = order+1) ===")

for order, reps in ((64, 5000), (256, 3000), (1024, 1000)):
    numtaps = order + 1
    t = avg_time(reps, lambda numtaps=numtaps: firwin(numtaps, cutoff, fs=Fs))
    print(f"fir1    order={order:<4d}: {t*1e6:.2f} us/call  ({reps} reps)")

# firls: scipy's implementation is dramatically faster than Qu's at large
# order (see README) -- more reps here than the Qu script uses at the same
# order is fine, this is a per-call average, not a total-time comparison.
for order, reps in ((64, 2000), (256, 300), (1024, 20)):
    numtaps = order + 1
    t = avg_time(reps, lambda numtaps=numtaps: firls(numtaps, [0, 0.08, 0.12, 1], [1, 1, 0, 0]))
    print(f"firls   order={order:<4d}: {t*1e6:.2f} us/call  ({reps} reps)")
