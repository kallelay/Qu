# Lesson 4 — Fourier, DFT & FFT

## The idea

A chord sounds like one sound, yet a piano string tuned to any of its notes
will start resonating when the chord plays — proof that several pure tones
are hiding inside the single pressure wave your ear receives.

Joseph Fourier proposed, in 1822, that essentially any function can be
built from a sum of sines and cosines at different frequencies, amplitudes,
and phases. The continuous Fourier transform makes that claim precise:

\[
X(f) = \int_{-\infty}^{\infty} x(t)\, e^{-j2\pi f t}\, dt
\]

`x(t)` is multiplied by a complex exponential spinning at frequency `f` and
integrated over all time. Energy in `x(t)` at frequency `f` reinforces
itself under this integral instead of averaging to zero; energy at any
other frequency does not. `X(f)` is complex: its magnitude says how strong
that frequency is, its angle says the phase at which it occurs.

Real measurements are not continuous or infinite. The **discrete Fourier
transform** (DFT) restricts the same idea to `N` samples:

\[
X[k] = \sum_{n=0}^{N-1} x[n]\, e^{-j2\pi kn/N}
\]

`k` runs from `0` to `N-1` and indexes a discrete set of frequencies rather
than a continuous `f`. Bin `k` corresponds to a real frequency

\[
f_k = k \cdot \frac{f_s}{N}
\]

where `f_s` is the sample rate. This one relationship — bin index to hertz
— is the single fact used to read every spectrum in this course.

Computed directly, the DFT costs \(O(N^2)\) multiplications: `N` output
bins, each a sum over `N` samples. James Cooley and John Tukey published,
in 1965, a way to compute the identical numbers in \(O(N \log N)\) by
recursively splitting the sum into even- and odd-indexed halves and reusing
partial sums both halves need — the **fast Fourier transform** (FFT). It is
not an approximation of the DFT. It is the DFT with the redundant work
removed, and the reason a phone can run a spectrum analyzer in real time.

A DFT also assumes something rarely true in practice: that the `N` samples
contain a whole number of cycles of every frequency present. When a tone's
period does not divide evenly into the window, its energy does not
disappear — it smears across neighboring bins instead, a discontinuity
introduced at the seam where the DFT implicitly repeats the window to make
it periodic. This is **spectral leakage**, and it is unavoidable in
general; a window function that tapers the samples to zero at both edges
trades away some of the sharp central peak for a far lower noise floor
elsewhere.

```qu
fs = 200
N = 200
t = (0 to N - 1) / fs
tone1 = cos(2 * pi * 5 * t)
tone2 = 0.6 * cos(2 * pi * 12 * t)
figure()
subplot(2, 1, 1)
plot(t, tone1, label = "5 Hz")
plot(t, tone2, label = "12 Hz")
xlabel("time (s)")
ylabel("amplitude")
subplot(2, 1, 2)
plot(t, tone1 + tone2)
xlabel("time (s)")
ylabel("amplitude")
```

Two clean tones (top) sum into a single wobbling curve (bottom) with no
visible trace of "two" anything — the DFT is the machine that reverses
this, in the same way your cochlea does it mechanically, one resonant
patch of the basilar membrane per frequency.

## In Qu

Build a 5 Hz and a 12 Hz tone and ask Qu which frequencies are present:

```qu
fs = 64
N = 64
t = (0 to N - 1) / fs
x = cos(2 * pi * 5 * t) + 0.5 * cos(2 * pi * 12 * t)
X = rfft(x)
mag = abs(X)
f = (0 to length(mag) - 1) * fs / N
peak = argmax(mag)
print("length(mag) = {length(mag)}")
print("peak bin = {peak}, peak freq = {f[peak]} Hz")
print("mag[5] = {mag[5]:.4f}")
print("mag[12] = {mag[12]:.4f}")
figure()
plot(f, mag)
xlabel("frequency (Hz)")
ylabel("magnitude")
```

```
length(mag) = 33
peak bin = 5, peak freq = 5 Hz
mag[5] = 32.0000
mag[12] = 16.0000
```

`rfft` returns only the non-negative-frequency half (`floor(N/2)+1` bins) —
a real-valued signal's spectrum is mirror-symmetric, so the other half adds
no information. Every bin besides 5 and 12 sits below `1.2e-14`, floating
point noise rather than a third tone. The raw magnitude of an exactly
bin-aligned tone of amplitude `A` comes out to `N*A/2` (`32` and `16`
here), a fact about how energy splits between a frequency and its mirror
image, not a Qu quirk.

Qu keeps both the direct \(O(N^2)\) sum and the accelerated transform so
you can check one against the other:

```qu
N = 50
t = 1, 2, ..., N
x = sin(t)
err = max(abs(fft(x) - dft(x)))
print("max |fft - dft| = {err:.2e}")
```

```
max |fft - dft| = 7.96e-14
```

`dft` is the textbook sum; `fft` uses Bluestein's algorithm for lengths
that are not powers of two, still \(O(N \log N)\). They agree to
floating-point rounding on `N = 50` — not a power of two — which is the
"same math, done fast" claim demonstrated rather than asserted.

Now break the whole-cycles assumption on purpose, then fix it with a
window:

```qu
N2 = 8
n = 0 to N2 - 1
aligned = cos(2 * pi * 2 * n / N2)
leaky = cos(2 * pi * 2.5 * n / N2)
print("aligned mag = {abs(fft(aligned, N2)):.4f}")
print("leaky mag = {abs(fft(leaky, N2)):.4f}")

N3 = 64
n3 = 0 to N3 - 1
tone = cos(2 * pi * 10.5 * n3 / N3)
rect_mag = abs(fft(tone, N3))
hann_mag = abs(fft(tone .* hann(N3), N3))
figure()
subplot(2, 1, 1)
plot(rect_mag[0:N3 / 2], label = "rectangular")
xlabel("frequency bin")
ylabel("magnitude")
subplot(2, 1, 2)
plot(hann_mag[0:N3 / 2], label = "hann")
xlabel("frequency bin")
ylabel("magnitude")
print("rectangular, far bins: {rect_mag[30:33]:.4e}")
print("hann,        far bins: {hann_mag[30:33]:.4e}")
```

```
aligned mag = [2.449294e-16, 3.463824e-16, 4, 3.463824e-16, 2.449294e-16, 3.463824e-16, 4, 3.463824e-16]
leaky mag = [1, 1.192058, 2.797933, 2.398035, 1, 2.398035, 2.797933, 1.192058]
rectangular, far bins: [1.00848, 1.002107, 1, 1.002107]
hann,        far bins: [0.000462, 0.000415, 0.0004, 0.000415]
```

Bin `2.5` does not exist, so `leaky`'s energy — the same total energy as
`aligned` carries — scatters across every bin instead of sitting in two.
Bins 30–33 sit far from the 10.5 tone and should carry nothing: without a
window they hover near 1.0; with a Hann window applied first, that floor
drops by roughly a factor of two thousand. The cost is real too — the main
peak gets shorter and wider under the Hann window — a genuine trade of
resolution for cleanliness, not a free improvement.

`X[k]` is one number per frequency, computed from all `N` samples at once:
time has been averaged away completely to obtain it. Ask this spectrum
*when* the 5 Hz energy occurred relative to the 12 Hz energy and there is
no answer — every property demonstrated here (resolution, leakage,
windowing) belongs to a signal Fourier analysis assumes holds still for
its entire record. Lesson 5 asks what happens to this picture when the
thing being measured is also growing or decaying while you watch it.
