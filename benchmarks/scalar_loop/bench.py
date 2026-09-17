"""Sustained scalar iteration -- Python side.

Same biquad, same coefficients, same input, same order as bench.qu beside
this file. See bench.qu's header for why this scenario exists: the fair
suite's loop kernel is 100,000 trivial additions and contributes 1.6% of the
Qu-vs-MATLAB gap, which makes interpreter overhead look irrelevant. A
sequential filter cannot be vectorized away, so it measures per-iteration
dispatch, which is what the M4 JIT decision turns on.

Pure Python on purpose. A numpy `lfilter` call would measure C, not the
interpreter, and the interpreter is the thing under test.
"""

import math
import time

N = 1000000

b0, b1, b2 = 0.0675, 0.1349, 0.0675
a1, a2 = -1.1430, 0.4128

# Input built outside the clock, no RNG, identical to the other two ports.
x = [0.0] * N
for i in range(N):
    x[i] = math.sin(i * 0.0001) + 0.5 * math.sin(i * 0.0013)

t = time.perf_counter()
y = [0.0] * N
w1 = 0.0
w2 = 0.0
for i in range(N):
    w0 = x[i] - a1 * w1 - a2 * w2
    y[i] = b0 * w0 + b1 * w1 + b2 * w2
    w2 = w1
    w1 = w0
t_iir = time.perf_counter() - t

print(f"biquad, {N} samples, scalar loop : {t_iir:.4f} s")
print(f"checksum                          : {sum(y):.6f}")
print(f"per-iteration                     : {t_iir / N * 1e9:.1f} ns")
