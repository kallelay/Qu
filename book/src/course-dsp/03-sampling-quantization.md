# Sampling and quantization

## The idea

A camera watching a spinning fan can show the blades slow, stop, and turn
backwards while the real fan spins forward at full speed. Every frame is a
correct photograph; the lie is assembled from a sequence of truths. Turning
a continuous wave into a list of numbers is exactly this operation, and it
carries the same hazard.

**Sampling** reads a continuous-time signal at instants \(T\) apart:

\[
x[n] = x_c(nT), \qquad T = \frac{1}{f_s}.
\]

Two different frequencies can produce the *same* sequence of samples. For
integer \(k\),

\[
\cos\!\big(2\pi (f + k f_s)\,nT\big) = \cos(2\pi f\,nT),
\]

because adding a whole number of cycles per sample changes nothing a
sampler can see. The frequency axis folds onto the band \(0\) to
\(f_s/2\) like a concertina; that half-rate point is the **Nyquist
frequency**.

Harry Nyquist showed in 1928 that a channel of bandwidth \(B\) carries at
most \(2B\) independent values per second. V. A. Kotelnikov published the
matching sampling result in 1933, and Claude Shannon proved the form now
quoted, in 1949:

\[
f_s > 2B \implies x_c(t) \text{ is completely determined by } x[n].
\]

"Completely determined" means every instant between the samples is
recoverable, not merely approximated, via

\[
x_c(t) = \sum_{n=-\infty}^{\infty} x[n]\,\operatorname{sinc}\!\left(\frac{t-nT}{T}\right),
\qquad \operatorname{sinc}(u) = \frac{\sin(\pi u)}{\pi u}.
\]

The inequality is strict because at exactly \(f_s/2\) a sine and a cosine
of that frequency give completely different records — one lands on every
zero crossing, the other on every peak — so amplitude and phase are not
both recoverable at the boundary itself. The only real defence against
aliasing is preventive: an analog anti-alias filter removes everything
above \(f_s/2\) before the converter sees it, which is why compact discs
sample at 44.1 kHz rather than 40 — real filters need room to roll off.

**Quantization** is the simpler failure, chopping the value instead of
time. A \(b\)-bit converter has \(2^b\) rungs across its range, so the step
is \(\Delta = V_{FS}/2^b\), and every reading lands within \(\Delta/2\) of
a rung. For a full-scale sine the resulting signal-to-noise ratio reduces
to a compact rule:

\[
\mathrm{SNR} \approx 6.02\,b + 1.76\ \text{dB}.
\]

One bit buys roughly six decibels.

```qu
tc = linspace(0, 0.04, 400)
fs = 200
n  = 0 to 7
t  = n / fs

figure()
subplot(1, 2, 1)
plot(tc, sin(2*pi*50*tc), color = "royalblue")
stem(t, sin(2*pi*50*t), color = "crimson")
title("Sampling")
xlabel("t (s)")

xv = linspace(-1, 1, 400)
d  = 0.25
xqv = round(xv / d) * d
subplot(1, 2, 2)
plot(xv, xqv, color = "seagreen")
title("Quantization")
xlabel("input")
ylabel("output")
```

The left panel is the textbook sampling picture: a continuous wave and the
discrete instants a converter actually keeps. The right panel is a
quantizer's transfer curve — continuous input, staircase output, each
tread \(\Delta\) wide.

## In Qu

Confirm the fold directly. Two tones, 100 Hz and 900 Hz, both sampled at
1000 Hz:

```qu
fs = 1000
N  = 1000
t  = (0 to N-1) / fs
low  = sin(2*pi*100*t)
high = sin(2*pi*900*t)
print("100 Hz and 900 Hz records, worst difference from exact negatives: {max(abs(high + low)):.2e}")
```

```
100 Hz and 900 Hz records, worst difference from exact negatives: 9.68e-13
```

The 900 Hz record *is* the 100 Hz record, upside down, to within
floating-point dust. No algorithm run afterwards can separate them,
because nothing is left to separate. A 700 Hz tone at the same rate shows
where the fold lands:

```qu
x700 = cos(2*pi*700*t)
mag  = abs(rfft(x700))
f    = (0 to length(mag)-1) * fs / N
k    = argmax(mag)
print("700 Hz sampled at 1000 Hz: reported peak at {f[k]} Hz")

figure()
plot(f, mag, color = "royalblue")
vline(fs/2, color = "crimson")
title("700 Hz sampled at 1 kHz: the peak folds back to 300 Hz")
xlabel("frequency (Hz)")
```

```
700 Hz sampled at 1000 Hz: reported peak at 300 Hz
```

The red line marks the 500 Hz Nyquist hinge. 700 Hz is 200 Hz past it, and
the spectrum reports the fold: 200 Hz on the other side, at 300 Hz. Nothing
in the samples distinguishes this from an honest 300 Hz tone.

Now the payoff of the sampling theorem: recover a value the converter
never recorded. Ten samples of a 400 Hz tone at 1 kHz, reconstructed with
a truncated sinc sum:

```qu
function recon(tq, M)
    acc = 0
    for k = -M to M
        u = (tq - k/fs) * fs
        if abs(u) < 1e-9 then
            acc += sin(2*pi*400*k/fs)
        else
            acc += sin(2*pi*400*k/fs) * sin(pi*u)/(pi*u)
        end if
    end for
    return acc
end function

ns = 0 to 9
xs = sin(2*pi*400*ns/fs)
print("samples: {round(xs, 4)}")

tdense = linspace(0, 0.009, 300)
ydense = zeros(300)
for i = 0 to 299
    ydense[i] = recon(tdense[i], 30)
end for

figure()
plot(tdense, sin(2*pi*400*tdense), color = "gray")
plot(tdense, ydense, color = "seagreen")
stem(ns/fs, xs, color = "royalblue")
title("Sinc reconstruction from ten samples of a 400 Hz tone")
xlabel("t (s)")
legend("true x(t)", "sinc reconstruction", "samples")
```

```
samples: [0, 0.5878, -0.9511, 0.9511, -0.5878, 0, 0.5878, -0.9511, 0.9511, -0.5878]
```

The green reconstruction and the gray true curve overlap everywhere,
built entirely from the blue sample points — using samples far outside
whatever short window is being examined, which is why a real
reconstruction always truncates the theorem's infinite sum and pays for
it in the fourth decimal place, not visibly here.

Quantization's error is bounded and measurable the same way. Run the SQNR
rule against a measured spectrum, 401 whole cycles in a 4096-sample record
so nothing is cut off at the edges:

```qu
M  = 4096
tt = (0 to M-1) / fs
f0 = 401 * fs / M
x  = sin(2*pi*f0*tt)
for bits = 4 to 16 step 4
    dd  = 2 / (2 ^ bits)
    xq = round(x / dd) * dd
    err = xq - x
    measured = 10 * log10(mean(x .^ 2) / mean(err .^ 2))
    print("{bits:d} bits: measured {measured:.2f} dB, theory {6.02*bits + 1.76:.2f} dB")
end for
```

```
4 bits: measured 26.22 dB, theory 25.84 dB
8 bits: measured 50.02 dB, theory 49.92 dB
12 bits: measured 73.88 dB, theory 74.00 dB
16 bits: measured 98.17 dB, theory 98.08 dB
```

Within a couple of tenths of a decibel over a four-thousand-fold change in
step size — the residual wobble is real, because the rounding error is a
deterministic function of a deterministic signal, not true uniform noise.

That determinism has a strange consequence. Take a tone three-tenths of
one quantization step tall — smaller than an 8-bit converter's
resolution — and quantize it plain, then with a little noise added first:

```qu
d8 = 2 / 256
tiny = 0.3 * d8 * sin(2*pi*f0*tt)
plain    = round(tiny / d8) * d8
dithered = round((tiny + 0.5*d8*randn(M, seed = 21)) / d8) * d8
print("distinct levels, plain: {length(unique(plain))}, dithered: {length(unique(dithered))}")
print("bin 401 magnitude, plain: {abs(rfft(plain))[401]:.4f}, dithered: {abs(rfft(dithered))[401]:.4f}")
print("undamaged tone would give: {0.3*d8*M/2:.4f}")
```

```
distinct levels, plain: 1, dithered: 5
bin 401 magnitude, plain: 0.0000, dithered: 4.6774
undamaged tone would give: 4.8000
```

Without dither the output has exactly one value — the tone is not
buried, it is gone, and its spectral line reads 0.0000. With a small
noise push added first, the signal crosses quantizer boundaries often
enough that which side it lands on carries information, and averaging
over four thousand samples recovers 4.68 against an undamaged 4.80. A
signal below the converter's resolution came out anyway, because rounding
is a hard nonlinearity and adding noise before it is not the same as
adding noise after it.

Twice in this lesson a spectrum was asked where energy went, and both
times it answered without being told anything about the signal in
advance. That function — `rfft` — is the change of representation the
next lesson derives from first principles: what a transform actually is,
why the answer is complex, and how Cooley and Tukey made it fast enough,
in 1965, for the rest of this subject to be practical.
