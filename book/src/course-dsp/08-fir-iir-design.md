# FIR and IIR design

A microphone held too close to a speaker produces a rising shriek out of
nowhere. Nothing was played — a loop formed between speaker and
microphone, and whether that loop dies out or runs away depends on one
number: whether a trip around it makes the signal a little smaller or a
little bigger than it started. That number is also the entire question
of digital filter stability.

## The idea

A digital filter is non-recursive or recursive, and the distinction is
whether the formula that computes \(y[n]\) is ever allowed to look at
its own past output.

A **FIR** filter (finite impulse response) computes its output purely
from present and past input:

\[
y[n] = \sum_{k=0}^{M} b_k\, x[n-k].
\]

This is the convolution sum, unchanged. Nothing about it can ever feed
back on itself, because \(y\) never appears on the right. Its impulse
response has exactly \(M+1\) nonzero samples, hence "finite."

An **IIR** filter (infinite impulse response) adds a second sum, over
its own past output:

\[
a_0\, y[n] = \sum_{k=0}^{M} b_k\, x[n-k] \;-\; \sum_{k=1}^{N} a_k\, y[n-k].
\]

That second sum is the microphone-and-speaker loop, written as
arithmetic. "Infinite" describes the *shape* of a well-designed
response — it rings and decays, in principle forever — not a warning
that it necessarily grows. It grows only when the loop gain is wrong,
and that is exactly one line to build:

```qu
b = [1]
a = [1, -1.5]                 # y[n] = x[n] + 1.5*y[n-1]
x = impulse(20)
y = filter_ba(b, a, x)
figure()
plot(y)
xlabel("n")
ylabel("y[n]")
title("An unstable IIR impulse response")
print("y[0:5]   = {y[0:5]}")
print("y[15:20] = {y[15:20]}")
```

```
y[0:5]   = [1, 1.5, 2.25, 3.375, 5.0625, 7.59375]
y[15:20] = [437.89389, 656.840836, 985.261253, 1477.89188, 2216.83782]
```

Every sample is 1.5 times the last, because that is exactly what
`y[n] = x[n] + 1.5 y[n-1]` says to do. This filter has a single pole at
\(z = 1.5\), outside the unit circle — outside the unit circle is the
entire, exact stability boundary for a digital IIR filter. An FIR filter
has every pole sitting at the origin regardless of design, which is the
formal way of saying it cannot do this, ever.

`butter`, `ellip`, `cheby1`, and `cheby2` all place their poles
correctly, every time, so ordinary IIR design never reproduces the
runaway above. What buys that convenience is visible at matched order:

```qu
fs  = 1000
iir = butter(4, "low", 100, fs)
fir = fir1(4, 100, kind = "low", fs = fs)
f   = linspace(0, fs / 2, 512)
figure()
plot(f, mag2db(abs(freqz(iir, 512))), label = "IIR (order 4)")
plot(f, mag2db(abs(freqz(fir, 512))), label = "FIR (order 4)")
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
ylim(-80, 5)
legend()
title("Same order, different sharpness")
print("IIR sections: {shape(iir.sos)[0]}")
print("FIR taps: {length(fir.b)}")
```

```
IIR sections: 2
FIR taps: 5
```

At equal order the IIR curve already falls faster. FIR pays for its
unconditional stability and something Lesson 9 needs unbroken —
**exactly linear phase** (every frequency delayed by the same number of
samples) — with more coefficients per unit of sharpness. Kaiser's
estimate for a windowed-sinc FIR length is
\(N \approx (A_{stop} - 8) / (2.285\,\Delta\omega)\), \(\Delta\omega\)
the transition width in radians; the Butterworth order needed for the
same spec is
\(N \geq \log_{10}\!\big[(10^{A_s/10}-1)/(10^{A_p/10}-1)\big] / \big(2\log_{10}(\omega_s/\omega_p)\big)\).
Neither is simulation — both are closed-form estimates you can evaluate
before designing anything.

## In Qu

Take an actual spec: pass everything below 100 Hz, stop everything above
110 Hz, at least 40 dB of stopband rejection — a demanding 10 Hz
transition. The two formulas above put the FIR order near 223 and the
Butterworth order near 56. Ask Qu for both:

```qu
fs = 1000
fir_sharp = fir1(223, 100, kind = "low", fs = fs)
iir_sharp = butter(56, "low", 100, fs)

f = linspace(0, fs / 2, 512)
figure()
plot(f, mag2db(abs(freqz(fir_sharp, 512))), label = "FIR order 223")
plot(f, mag2db(abs(freqz(iir_sharp, 512))), label = "IIR order 56")
xlim(0, 200)
ylim(-100, 5)
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
legend()
title("Matching a 10 Hz transition: FIR vs IIR")
print("FIR taps: {length(fir_sharp.b)}")
print("IIR sections: {shape(iir_sharp.sos)[0]} ({shape(iir_sharp.sos)[0] * 5} multiply-adds/sample)")
```

```
FIR taps: 224
IIR sections: 28 (140 multiply-adds/sample)
```

224 multiply-adds per sample against 140 — the FIR estimate that looked
cheaper at matched order is now the more expensive filter, because
sharpness, not order, is what a real spec asks for, and IIR sharpness
grows from the *ratio* of stopband to passband edge rather than the raw
gap. What the extra FIR cost buys is on the phase axis, not the
magnitude one:

```qu
gd_fir = group_delay(fir_sharp, 512)
gd_iir = group_delay(iir_sharp, 512)
figure()
plot(f, gd_fir, label = "FIR")
plot(f, gd_iir, label = "IIR")
xlim(0, 200)
xlabel("Frequency (Hz)")
ylabel("Group delay (samples)")
legend()
title("Group delay: constant vs frequency-dependent")

i40 = where(abs(f - 40) < 1)[0]
i80 = where(abs(f - 80) < 1)[0]
print("FIR group delay near 40 Hz: {gd_fir[i40]:.1f}")
print("FIR group delay near 80 Hz: {gd_fir[i80]:.1f}")
print("IIR group delay near 40 Hz: {gd_iir[i40]:.1f}")
print("IIR group delay near 80 Hz: {gd_iir[i80]:.1f}")
```

```
FIR group delay near 40 Hz: 111.5
FIR group delay near 80 Hz: 111.5
IIR group delay near 40 Hz: 58.7
IIR group delay near 80 Hz: 78.6
```

The FIR delay is the same number, 111.5 samples, everywhere in the
passband: every frequency arrives shifted by an identical amount, so a
filtered waveform keeps its shape instead of smearing. The IIR delay
moves — 58.7 samples at 40 Hz, 78.6 at 80 Hz — because the feedback that
buys its sharp transition does more work as frequency climbs toward the
cutoff. A digital audio effect, a measurement chain, anything that
cannot tolerate phase smearing wants FIR, extra multiply-adds included.

## The question this lesson has not asked

Every filter built here and in Lesson 7 had a target decided before a
single sample arrived: a cutoff typed in, coefficients computed once and
frozen. That is fine when you already know exactly what to remove —
mains hum sits at 50 Hz whether or not anyone checks. It fails the
moment the thing you want to remove will not hold still: an echo whose
delay depends on a room nobody measured, a whine whose pitch tracks an
engine's RPM. Lesson 9 builds a filter with no fixed coefficients to
type in the first place.
