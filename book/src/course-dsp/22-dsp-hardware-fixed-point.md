# Lesson 22 — DSP Hardware and Fixed Point

Every signal in this course so far has lived as a 64-bit floating-point
number, with roughly sixteen decimal digits of precision and a dynamic
range that spans atoms to galaxies. A microcontroller running on a coin
cell, or a chip cast into an ASIC before you were born, may have none of
that. It has integers, a fixed number of bits, and a battery budget. DSP
hardware is the study of what survives that translation.

## The idea

**Fixed point** represents a real number as an integer, scaled by an
implicit power of two. The convention is `Qm.n`: `m` integer bits, `n`
fractional bits, plus an implied sign bit. A stored integer code \(c\)
represents the real value

\[
x = c \cdot 2^{-n}
\]

so the smallest representable step, the resolution, is

\[
\Delta = 2^{-n}
\]

and the representable range is roughly \([-2^m,\ 2^m - \Delta]\). `Q1.15`
— one sign-adjacent integer bit, fifteen fractional bits, fitting in a
16-bit word — is the format behind decades of consumer audio hardware,
because it packs the range `[-1, 1)` into exactly the machine word a cheap
1980s DSP chip could multiply in one cycle. There is no such thing as a
free floating-point unit on that chip; fixed point was not a stylistic
choice, it was the only arithmetic available.

**Quantization error** is what rounding to the nearest representable code
costs you. For round-to-nearest, the error \(e = x_q - x\) is bounded by
\(\pm\Delta/2\) and, for a signal that moves through many quantization
levels, behaves like noise uniformly distributed on that interval, with
power

\[
\sigma_e^2 = \frac{\Delta^2}{12}
\]

Comparing that noise floor to a full-scale sinusoid's signal power gives
the standard result relating word length directly to signal-to-noise
ratio:

\[
\mathrm{SNR} \approx 6.02\,n + 1.76~\text{dB}
\]

Each additional fractional bit buys almost exactly 6 dB — the reason a
16-bit format and a 24-bit format are not a modest upgrade but a
qualitative jump in how much noise floor is left to hide anything in.

**Overflow** is the other failure fixed point introduces that floating
point mostly hides. When an accumulator's true result exceeds the
representable range, hardware does one of two things: **wraps around**
(two's-complement arithmetic silently flips a large positive sum into a
large negative one) or **saturates** (clamps at the most positive or most
negative representable value and stays there). Neither is automatically
"correct" — a filter designed assuming saturation will misbehave on
hardware that wraps, and vice versa — and the choice is usually fixed in
silicon, not in your code.

```qu
seed(1)
function quantize(x, frac_bits)
    scale = 2 ^ frac_bits
    xq = round(x * scale)
    xq = clip(xq, -scale, scale - 1)
    return xq / scale
end function

N = 200
t = (0 to N - 1) / N
x = 0.9 * sin(2 * pi * 2 * t)
xq = quantize(x, 3)

figure()
plot(t, x, label="original")
plot(t, xq, label="3-bit quantized")
xlabel("time (s)")
ylabel("amplitude")
title("Quantizing a sine wave to 3 bits")
legend()
```

At three bits the staircase is obvious to the eye: sixteen possible
levels across the full swing, and the sine wave is visibly forced onto
the nearest one at every sample. Real fixed-point audio uses many more
levels than this — the point of the picture is the mechanism, not the
resolution.

## In Qu

`quantize`, defined above, is a working `Qm.n` rounding-and-clipping
model: scale by \(2^n\), round to the nearest integer, clip to the
representable range, scale back. Applying it at `frac_bits = 15` runs a
300 Hz tone through `Q1.15`, the real format behind a great deal of
telephony and consumer audio hardware:

```qu
fs = 8000
N2 = 200
t2 = (0 to N2 - 1) / fs
tone = 0.9 * sin(2 * pi * 300 * t2)
q15 = quantize(tone, 15)

zoom = 0 to 39
figure()
plot(t2[zoom], tone[zoom], label="original")
plot(t2[zoom], q15[zoom], label="Q1.15 fixed-point")
xlabel("time (s)")
ylabel("amplitude")
title("A 300 Hz tone in Q1.15 fixed point")
legend()

print("max abs error: {round(max(abs(tone - q15)), 6)}")
print("theoretical step (2^-15): {round(2^-15, 6)}")
```

```
max abs error: 1.5e-5
theoretical step (2^-15): 3.1e-5
```

The error is bounded by half the theoretical step, exactly as the model
predicts, and at 16 bits it is already far smaller than anything visible
on the waveform plot — sixteen bits is generous for audio, which is
exactly why it became the CD standard.

Sweeping the fractional-bit count and measuring actual SNR against the
`6.02n + 1.76` prediction tests the formula rather than assuming it:

```qu
bits = 2 to 15
measured = zeros(length(bits))
theory = zeros(length(bits))
for i in 0 to length(bits) - 1
    n = bits[i]
    xqn = quantize(tone, n)
    noise = tone - xqn
    measured[i] = 10 * log10(sum(tone .^ 2) / sum(noise .^ 2))
    theory[i] = 6.02 * n + 1.76
end for

figure()
plot(bits, measured, "o", label="measured")
plot(bits, theory, label="6.02n + 1.76 dB (theory)")
xlabel("fractional bits n")
ylabel("SNR (dB)")
title("Quantization SNR vs. bit depth")
legend()

print("at n=15: measured {round(measured[length(measured)-1], 2)} dB, theory {round(theory[length(theory)-1], 2)} dB")
```

```
at n=15: measured 97.17 dB, theory 92.06 dB
```

Measured SNR tracks the theoretical line's slope closely but sits a few
dB above it at every bit depth, not below. The `6.02n + 1.76` formula
assumes quantization error behaves like independent white noise, which is
only an approximation — it is derived for a busy, unpredictable signal
sweeping through many codes in an uncorrelated way. A clean, densely
sampled single tone does not fully satisfy that assumption: its
quantization error is itself a deterministic, partly periodic function of
the input rather than genuine randomness, and here that structure happens
to leave slightly less energy in the error than the idealized model
charges for. This is the textbook reason real ADCs deliberately add a
tiny amount of **dither** — actual random noise — before quantizing:
it is a strange-sounding fix, adding noise to reduce apparent error, but
it forces the error to behave like the uniform model actually assumes,
which trades a small guaranteed noise floor for freedom from unpredictable,
signal-dependent quantization artifacts.

Overflow is a separate failure from resolution, and either hardware
convention corrupts a result if the algorithm assumes the other:

```qu
v = 40000
wrapped = ((v + 32768) mod 65536) - 32768
saturated = clip(v, -32768, 32767)
print("accumulator overflow example -- raw sum: {v}")
print("two's-complement wraparound: {wrapped}")
print("saturating clamp: {saturated}")
```

```
accumulator overflow example -- raw sum: 40000
two's-complement wraparound: -25536
saturating clamp: 32767
```

A sum of `40000` does not exist in a 16-bit signed range. Wraparound
turns it into `-25536` — a wildly wrong value with the wrong sign,
propagated forward as if it were legitimate data. Saturation instead
clamps to `32767`, wrong in magnitude but at least the right sign and the
right side of correct. A FIR filter's accumulator that is sized correctly
for saturating hardware will alias badly on wrapping hardware, silently.

None of this — resolution, dither, saturation policy — says anything
about *when* the arithmetic has to finish. A fixed-point multiply-
accumulate that is numerically perfect is still wrong if it has not
produced a sample before the next one from the ADC arrives. Lesson 23
takes up exactly that constraint: processing a signal that never stops
arriving, in a time budget that never extends.
