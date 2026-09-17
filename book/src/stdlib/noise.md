# Noise, Interference & Distortion

Putting them in, and taking them out.

The point of a noise module is not to make a signal look untidy. It is to
state exactly how hard a problem is, hand the same problem to several
methods, and see which one wins and where.

## Noise is specified in decibels

"Add Gaussian noise of amplitude 0.1" is not a statement about a
measurement — it depends entirely on how big the signal is. "Add Gaussian
noise at 3 dB SNR" is, and it is what a datasheet, a standard and a
reviewer all speak.

```qu
fs = 1000
t = (0 to 2047) / fs
clean = sin(2 * pi * 7 * t)

noisy = add_noise(clean, "gaussian", snr = 3, seed = 7)
print("asked for 3 dB, measured {measure_snr(clean, noisy):.2f} dB")
```

Yes, 3 dB is possible, and so is 0, and so is −6. The amplitude is worked
out from the signal:

```text
SNR_dB = 10·log10(P_signal / P_noise)   ⟹   P_noise = P_signal / 10^(SNR/10)
```

`amplitude=` is there for when a raw figure is what you were actually given
— a specified 2 mV of pickup, say. Give one or the other, never both:
they are two ways of saying the same thing and they would disagree.

**Every kind honours that number**, including the four whose noise is not
linear in their own parameter. Shot noise scales with the signal,
quantisation error is bounded by the step, salt-and-pepper's parameter is a
corruption *rate* — for those the level is found by bisection, so `snr = 3`
means one thing across the whole module and the kinds can be compared.

## The kinds, and what each one models

Noise is not one thing. Using white noise for all of it is how a method
comes to look robust in simulation and fail on the bench.

| Kind | Models | Spectrum |
|---|---|---|
| `gaussian`, `white` | Thermal (Johnson–Nyquist) noise; the sum of many small independent causes | Flat |
| `pink`, `flicker` | Flicker noise in semiconductors, sensor drift | 1/f |
| `brown`, `red` | A random walk — baseline wander | 1/f² |
| `blue` | What dithering and noise shaping produce | f |
| `uniform` | A bounded error source; matched in *power* to a Gaussian of the same level, so the two are comparable | Flat |
| `shot`, `poisson` | Discrete quanta arriving — photons, electrons across a junction. Variance equals mean, so it grows *with* the signal | — |
| `salt_pepper` | Dead and stuck pixels, bit errors | — |
| `impulse`, `spike` | Switching transients, ESD events | — |
| `quantize`, `adc` | The error an ADC makes | — |

Two of these are not additive at all, and that is the important part:
**salt-and-pepper *replaces* samples** rather than adding to them, which is
exactly why a linear filter cannot remove it and a median filter can.
Quantisation error is *correlated* with the signal, which is why it sounds
worse than its power suggests and why dither exists.

#### Overloads: `add_noise`

#### Case: additive kinds (`gaussian`, `pink`, `uniform`, ...)

Every sample is perturbed by a little: none of the output matches the
clean input exactly.

```qu
x = sin(2 * pi * 5 * (0 to 199) / 100)
g = add_noise(x, "gaussian", snr = 6, seed = 1)
untouched = 0
for i in 0 to 199
    if g[i] == x[i]
        untouched = untouched + 1
    end if
end for
print(untouched)   # 0 -- every sample was perturbed a little
```

#### Case: replacing (`salt_pepper`)

The same `snr` targets a corruption *rate* instead: most samples are left
bit-for-bit alone, and a fraction are slammed to an extreme.

```qu
x = sin(2 * pi * 5 * (0 to 199) / 100)
sp = add_noise(x, "salt_pepper", snr = 6, seed = 1)
untouched = 0
for i in 0 to 199
    if sp[i] == x[i]
        untouched = untouched + 1
    end if
end for
print(untouched)   # 184 -- ~8% got slammed to an extreme, the rest untouched
```

Pink noise is worth a second look: it dominates at low frequencies, which
is why a slow measurement is harder than a fast one and a long integration
is not free.

Works on a vector and on an image alike — every kind is per-sample, so one
function covers 1-D and 2-D rather than two that drift apart.

## Quantisation, as an instrument does it

`add_noise(x, "quantize", ...)` takes a step size, which is the
mathematician's parameter. A converter has a **reference range** and a
**resolution**, and the difference is not cosmetic: outside the range it
*clips*, and a step-based model cannot express that at all — it will
happily quantise a signal ten times the converter's range and report no
problem.

```
adc(x, [bits = 12], [vmin =], [vmax =], [dither = false], [seed =])
```

- `x` — the signal to quantise, a length-N numeric vector (or scalar-per-sample image data), in the same units as `vmin`/`vmax`.
- `bits` — named, an integer word length of at least 1, default `12`. Fixes the number of codes, `2^bits`, and therefore the LSB.
- `vmin`, `vmax` — named numbers, the converter's reference range in the signal's own units. Default to `-peak`/`+peak` of `x` itself (symmetric full scale), so a first run shows quantisation with nothing clipping.
- `dither` — named boolean, default `false`. When `true`, adds two-LSB peak-to-peak triangular (TPDF) noise before quantising.
- `seed` — named integer, the RNG seed used only when `dither = true`; omit it and a run is still reproducible against the module's default seed.
- Returns a record — see the fields below — not a bare vector, because whether anything clipped is part of the answer.

A single sine run shows what the record gives back: not just the quantised
signal, but whether the chosen range fit it and what an ideal converter of
that word length could have done. Here a 1.2 V peak tone is digitised on a
0–4.096 V, 12-bit reference:

```qu
t = (0 to 4095) / 10000
v = 1.2 * sin(2 * pi * 101 * t) + 2.048

c = adc(v, bits = 12, vmin = 0, vmax = 4.096)
print("LSB {c.lsb * 1000:.3f} mV, clipped {c.clipped}, ideal SNR {c.snr_ideal:.1f} dB")
```

The LSB is the range over `2^bits` — not `2^bits − 1`, which is the
off-by-one that puts every reading half a step out. A 0–4.096 V 12-bit
converter therefore has an LSB of exactly 1 mV, which is why that reference
is so common.

It returns a record, not a bare signal, because `.clipped` is not a detail:

| Field | Type/shape | Description |
|---|---|---|
| `.x` | length-N number vector | The quantised signal, same length as the input `x` |
| `.lsb` | number | One least-significant bit, `(vmax - vmin) / 2^bits`, in the signal's own units |
| `.clipped` | integer count | How many input samples fell outside `[vmin, vmax]` and were clamped to it |
| `.snr_ideal` | number, dB | `6.02·bits + 1.76` dB — what an ideal converter of this word length would give on a full-scale sine |
| `.vmin`, `.vmax` | numbers | The reference range actually used — the given values, or the signal's own full scale when left to default |
| `.bits` | integer | The word length actually used, echoed back for convenience |

**A converter that clips has stopped measuring.** A script that never looks
at `.clipped` will report a flat-topped waveform as data.

`.snr_ideal` is the number to compare a measurement against. Falling short
means something else dominates; beating it means the signal is not
full-scale, or the arithmetic is wrong. Measured on a 1.5 V-range converter
with a 1.2 V peak sine:

```text
 6 bits  LSB  46875.0 µV   ideal 37.9   measured 35.6 dB
 8 bits  LSB  11718.8 µV   ideal 49.9   measured 48.1 dB
10 bits  LSB   2929.7 µV   ideal 62.0   measured 59.9 dB
12 bits  LSB    732.4 µV   ideal 74.0   measured 72.0 dB
16 bits  LSB     45.8 µV   ideal 98.1   measured 96.1 dB
```

### Dither

`dither = true` adds triangular (TPDF) noise of two LSB peak-to-peak
*before* quantising — where it goes in a real instrument, added to the
analogue signal rather than to the codes.

It raises the noise floor slightly and, in exchange, decorrelates the error
from the signal. Undithered quantisation error is a *function* of the
signal, so it appears as harmonics rather than as noise and no amount of
averaging removes it. Dithered, it becomes genuine noise — and averaging
then buys resolution, which is how a 12-bit converter is made to resolve
better than 12 bits.

TPDF specifically, not a single uniform: the triangular distribution is the
one that makes both the mean *and* the variance of the error independent of
the signal. A single uniform fixes the mean only.

## Interference is not noise

Hum and EMI are **deterministic**. They have structure, and that structure
is what lets you remove them.

| Function | Signature | Description |
|---|---|---|
| `hum` | `hum(x, fs, [freq=50], [harmonics=3], [snr=20], [amplitude=], [seed=])` | Adds simulated mains pickup to `x`, a length-N signal vector, given the sample rate `fs` in Hz. `freq` (number, Hz) is the mains fundamental, default `50`. `harmonics` (integer count) is how many **odd** harmonics to include — `3` means the 1st, 3rd and 5th, i.e. 50, 150 and 250 Hz — default `3`. `snr` (number, dB, default `20`) or `amplitude` (number, raw units) sets the interference level, one or the other, not both. `seed` (integer, optional) fixes the phase/noise draw for a reproducible run. Returns a length-N vector, `x` plus the interference. |
| `emf` | `emf(x, fs, [carrier=], [bursts=], [snr=20], [amplitude=], [seed=])` | Adds simulated switching/EMI interference to `x`, a length-N signal vector, given the sample rate `fs` in Hz: an amplitude-modulated carrier plus damped bursts at each switching edge. `carrier` (number, Hz) is the switching frequency, default `fs / 20`. `bursts` (number, burst events per second) sets how often a ringing transient fires, default `carrier / 50`. `snr` (number, dB, default `20`) or `amplitude` (number, raw units) sets the interference level, one or the other, not both. `seed` (integer, optional) fixes the random draw. Returns a length-N vector, `x` plus the interference. |

`hum` adds **odd** harmonics: mains pickup arrives through a symmetric
nonlinearity — a rectifier, a saturating core — and a symmetric
nonlinearity generates odd harmonics only. So `harmonics = 3` means 50, 150
and 250 Hz, not 50, 100 and 150. That matters: a notch at the fundamental
alone leaves two thirds of the problem behind, and a model without the
harmonics makes every notch filter look better than it is.

`emf` is two things at once, because real EMI is: a carrier at the
switching frequency, amplitude-modulated because the coupling is never
constant, *plus* a damped ring at each switching edge from the `di/dt` of
the commutation. A method tested against only one of those fails on the
other.

```qu
fs = 1000
t = (0 to 999) / fs
clean = sin(2 * pi * 7 * t)

h = hum(clean, fs, freq = 50, harmonics = 3, snr = 6)
e = emf(clean, fs, carrier = 150, snr = 6)
print("hum {measure_snr(clean, h):.2f} dB, emf {measure_snr(clean, e):.2f} dB")
```

## Distortion is neither

Distortion is a **function of the signal**. Noise is independent of it, so
averaging a hundred records reduces noise; distortion is identical in every
record and averaging is powerless against it. The only cure is not to
distort.

```
distort(x, kind, [amount = 0.3])
```

- `x` — the signal to distort, a length-N number vector.
- `kind` — a string naming the nonlinearity: `"clip"`/`"hard"`, `"soft"`/`"saturate"`, `"crossover"`, `"harmonic"`/`"cubic"`, or `"quantize"` (see the table below).
- `amount` — an optional positional number from 0 (no effect) to 1 (maximum), default `0.3`. Runs the same 0–1 range for every kind, so different kinds at the same `amount` are comparable.
- Returns a length-N number vector, the distorted signal — same length as `x`.

`amount` runs 0 to 1 for every kind, so they can be compared.

| Kind | What it is |
|---|---|
| `clip`, `hard` | The amplifier ran out of rail. Odd harmonics, abruptly |
| `soft`, `saturate` | A tanh curve — a transformer, a valve. The same harmonics, arriving gradually |
| `crossover` | A dead zone at zero, from a class-B stage whose halves do not meet. Worst on **small** signals, the opposite of clipping |
| `harmonic`, `cubic` | A weakly nonlinear amplifier; a cubic term |
| `quantize` | Word length, 16 bits down to 2 |

Two kinds on the same clean tone show why "distortion" is not one number:
hard clipping and soft saturation both add odd harmonics at the same
`amount`, but clipping does it abruptly and saturation gradually, and `thd`
(total harmonic distortion, in dB below the fundamental) shows the gap
between them directly:

```qu
t = (0 to 999) / 1000
pure = sin(2 * pi * 31 * t)

clipped = distort(pure, "clip", 0.4)
soft = distort(pure, "soft", 0.4)
print("clip {thd(clipped):.1f} dB, soft {thd(soft):.1f} dB, below the fundamental")
```

Measured on a single 31 Hz tone, `thd` in dB below the fundamental:

```text
clean tone          -44.5 dB
soft         0.4    -20.3 dB
crossover    0.4    -33.8 dB
harmonic     0.4    -34.8 dB
clip         0.4    -25.6 dB
```

## Taking it out again

| Function | Signature | For |
|---|---|---|
| `medfilt` | `medfilt(x, [window=3])` | Replaces each sample by the median of `window` samples centred on it. `x` is a length-N number vector or an `Image` (filtered channel by channel); `window` is a positional integer, odd, default `3`. Returns a value the same shape as `x`: salt-and-pepper, spikes. |
| `smooth` | `smooth(x, [window=5], [method="savgol"], [order=2])` | One word for the smoothers. `x` is a length-N number vector; `window` is a named/positional odd integer, default `5`; `method` is a named string, one of `"savgol"` (default), `"moving"`, `"median"`; `order` is a named integer, the polynomial order, used only by `"savgol"`, default `2`. Returns a length-N vector. |
| `savgol` | `savgol(x, window, [order=2])` | Savitzky-Golay: fits a degree-`order` polynomial over each `window`-wide neighbourhood and keeps its centre value, preserving peak height/width that a moving average would flatten. `x` is a length-N number vector; `window` is a required positional odd integer; `order` is an optional positional integer, default `2`. Returns a length-N vector. When the *shape* matters. |
| `hampel` | `hampel(x, [window=7], [n_sigma=3])` | Outlier removal: replaces a sample by the local median only when it is more than `n_sigma` robust deviations (`1.4826 × MAD`) from it. `x` is a length-N number vector; `window` is an optional positional odd integer, default `7`; `n_sigma` is an optional positional number, default `3`. Returns a record `{x, replaced}` — the cleaned length-N vector and the integer count of samples actually changed. |
| `detrend` | `detrend(x, [order=1])` | Subtracts a least-squares polynomial trend from `x`, a length-N number vector. `order` is an optional positional integer: `0` removes the mean, `1` (default) a linear drift, `2` a quadratic bow. Returns a length-N vector. For baseline wander. |
| `measure_snr` | `measure_snr(clean, noisy)` | Computes the achieved signal-to-noise ratio by treating `noisy - clean` as the noise. `clean` and `noisy` are two length-N number vectors of equal length. Returns a single number, in dB. Closing the loop. |

#### Overloads: `medfilt`

#### Case: vector

`x` a length-N number vector. Slides a `window`-wide 1-D neighbourhood
and keeps the median.

```qu
v = [5, 3, 40, 6, 5, 4, 3]
print(medfilt(v, 3))   # [3, 5, 6, 6, 5, 4, 4] -- the spike at index 2 is gone
```

#### Case: image

`x` an `Image`. Runs the true 2-D median over a `window`×`window` square,
channel by channel — not a row-by-row 1-D median, which would smear
vertical structure it never looks at. Only possible because the `Image`
carries its own width and height. (`medfilt2`/`median_filter` are the
exact same builtin under other names, so this case applies to them too.)

```qu
m = [128,128,128,128,128; 128,128,255,128,128; 128,128,128,128,128; 128,128,128,128,128; 128,128,128,128,128]
img = image_from_matrix(m)
cleaned = medfilt(img, 3)
print(imhist(img, 4))       # [0, 0, 24, 1] -- one bright pixel
print(imhist(cleaned, 4))   # [0, 0, 25, 0] -- gone, over a 2-D neighbourhood this time
```

The paragraph and table above describe the removal tools individually; the
example below chains several of them into one cleanup pipeline. First,
`impulse(n, [index=], [amplitude=])` builds the spike to inject in the
first place — a plain length-`n` zero vector with one sample set to
`amplitude` at position `index`. `n` is a required positional integer, the
output length; `index` is a named integer sample position, default `0`;
`amplitude` is a named number, default `1`. It returns a length-`n` number
vector, and is the natural way to add a single, controlled transient rather
than reaching for `add_noise`'s random placement.

A clean tone corrupted by both a single spike and a slow linear drift needs
two different tools, applied in the right order — a median filter cannot
see through a ramp, and a polynomial detrend cannot see through a spike, so
the spike must go first:

```qu
fs = 1000
n = 500
t = (0 to n - 1) / fs
clean = sin(2 * pi * 7 * t)

spike = 5 * impulse(n, index = 200)
corrupted = clean + spike + 0.01 * (0 to n - 1)   # a spike, plus baseline drift

fixed = medfilt(corrupted, 5)     # the spike
flat = detrend(fixed, 1)          # the ramp
sg = savgol(flat, 11, 2)          # a final polish, shape-preserving
sm = smooth(flat, 11)             # same thing, one word (default method is savgol)
print("after cleanup: {measure_snr(clean, sg):.2f} dB (savgol), {measure_snr(clean, sm):.2f} dB (smooth)")
```

### Why a median, and when not

An average is pulled by an outlier in proportion to how far out it is. A
median ignores it entirely, as long as fewer than half the window is
corrupt. On salt-and-pepper at 8%:

```text
as measured          3.97 dB
median filter (5)   28.52 dB
moving average (5)  10.65 dB
```

Twenty decibels. It is also why a median preserves an edge that an average
smears — a step is not an outlier to a median.

But the reverse holds too, and neither should be mistaken for a universal
answer. On plain Gaussian noise the moving average wins, because it is the
optimal linear estimator for it:

```text
gaussian at 6 dB
  moving average (11)  15.74 dB
  median filter (11)   14.13 dB
```

A median is for **outliers**. Using one for noise costs you.

### Why Savitzky-Golay

A moving average is a low-pass filter, and a peak is high-frequency, so it
flattens the peak while removing the noise. Savitzky-Golay fits a local
polynomial instead and keeps the height and width. On a Gaussian peak under
10 dB of noise, both with a 61-point window:

```text
true height       1.000
Savitzky-Golay    1.000
moving average    0.907
```

The average removed more noise and destroyed the measurement. That is why
Savitzky-Golay is the default `smooth` method, and why it is standard in
every spectroscopy package.

`window` must be odd for all of these, so there is a middle sample; an even
one is refused rather than silently rounded, because which way it rounds
changes the answer.

### Hampel, when the data are good apart from a few spikes

`hampel` replaces a sample by the local median **only** when it lies more
than `n_sigma` robust deviations from it, and leaves everything else bit
for bit. That is the difference from `medfilt`, which rewrites every sample
whether it needed it or not.

```qu
spiky = sin(2 * pi * (0 to 499) / 100)
spiky[120] = 6.0
spiky[300] = -7.0

r = hampel(spiky, 7, 3)
print("replaced {r.replaced} samples of {len(spiky)}")
```

It returns both the cleaned signal and the count, because "it removed 3
outliers" and "it rewrote 400 samples" are very different results and
should not look the same.

The scale is `1.4826 × MAD` — the constant that makes the median absolute
deviation an unbiased estimate of the standard deviation on Gaussian data,
so `n_sigma` means what it says.

`catalog/qu_noise_and_removal.qu` runs all of this on one signal.

## More functions

| Function | Signature | Description |
|---|---|---|
| `adc` | `adc(x, [bits=12], [vmin=], [vmax=], [dither=false], [seed=])` | Quantises `x` (a length-N number vector) the way a converter does — with a reference range, not just a step size, so a signal outside `[vmin, vmax]` **clips** instead of being quantised as if the range were infinite. `bits` (positional or named integer, at least 1, default `12`) fixes the code count `2^bits`; `vmin`/`vmax` (named numbers) default to `-peak`/`+peak` of `x` itself; `dither` (named boolean, default `false`) adds two-LSB peak-to-peak TPDF noise before quantising, with `seed` (named integer) the RNG seed it uses. See the section above for the full field-by-field discussion. Returns a `Record` with fields `x` (a length-N `Vec`, the quantised signal), `lsb` (a `Num`, `(vmax - vmin) / 2^bits`), `clipped` (a `Num`, how many samples fell outside the range), `snr_ideal` (a `Num`, dB), `vmin` (a `Num`, the low end of the range actually used), `vmax` (a `Num`, the high end) and `bits` (a `Num`, the word length actually used). |
| `add_noise` | `add_noise(x, kind, [snr=], [amplitude=], [seed=])` | Adds noise of the named `kind` (a string — see the kinds table near the top of this chapter) to `x`, a length-N number vector or an `Image` (noise is added per-pixel, per-channel). `snr` (named number, dB) or `amplitude` (named number, raw units) sets the level — give one or the other, never both; with neither, defaults to 20 dB. `seed` is a named integer for a reproducible draw, default `1`. Returns a value the same shape as `x`. |
| `distort` | `distort(x, kind, [amount=0.3])` | Put the signal through a nonlinearity: `clip`, `soft`, `crossover`, `harmonic`, `quantize`. See the dedicated table and signature above for the full parameter breakdown. Returns a plain length-N `Vec` — a `Signal` argument comes back as a bare vector, so re-wrap it with `signal(...)` if you need the sample rate. |
| `medfilt2`, `median_filter` | `medfilt2(x, [window=3])` | Synonyms for `medfilt` at the Qu level: `x` is a length-N number vector or an `Image`, `window` an optional positional/named odd integer, default `3`. On a plain vector the result is identical to `medfilt(x, window)` (a 1-D median); the two-dimensional median — over a `window`×`window` square, channel by channel, not separable, so the median of medians is never substituted for the real thing — only happens when `x` is an `Image`, since the image itself carries its own width and height. Returns a value the same shape as `x`. |
| `tv_denoise` | `tv_denoise(y, [lambda=1.0])` | Total-variation denoising: `y` is a length-N number vector; `lambda` is an optional positional number, the smoothing penalty, default `1.0` — larger values remove more variation. Assumes few CHANGES rather than few non-zeros, so a step survives where an l1 method would flatten it. Condat's direct algorithm, one pass. Returns a length-N vector. |
| `coherence` | `coherence(A)` | `A` is an M×N sensing matrix. Returns a single number: the largest inner product between any two different normalised columns of `A`. Small is good. Unrelated to `spectral_coherence` (magnitude-squared spectral coherence of two signals) in the Signal Processing chapter's Spectral Analysis section — two different functions, two different names. |
| `cs_recover` | `cs_recover(A, y, [method="omp"], [sparsity=], [lambda=], [iters=200], [tol=1e-9], [step=], [rho=1.0], [debias=false])` | Compressed-sensing reconstruction of a sparse vector `x` from an M×N sensing matrix `A` and a length-M measurement vector `y`. `method` (named string) selects the algorithm: the greedy family `omp`, `cosamp`, `sp` and `iht`/`niht` need `sparsity=` (named integer, the assumed number of non-zeros); the convex family `fista`, `ista` and `bp` need `lambda=` (named number, the penalty) instead. `iters` (named integer) caps the iteration count, `tol` (named number) the convergence tolerance, `step` (named number) the IHT step size, `rho` (named number) the ADMM penalty for `bp`; `debias=true` re-fits the recovered support by plain least squares afterwards. Returns a record `{x, support, residual, iterations, converged, method}`: the recovered length-N vector, the integer indices of its non-zero support, the final residual norm, the iteration count, whether it converged, and the method name used. |
| `cs_guarantee` | `cs_guarantee(A)` | `A` is an M×N sensing matrix. Returns a single number: the largest sparsity level that the matrix's coherence alone can *prove* recoverable. Badly pessimistic, and the only certificate that is computable at all. |

On an `Image`, `medfilt2` (and its synonym `median_filter`) run the true
2-D median over a square window, channel by channel, rather than the 1-D
`medfilt` above. A 1-D median taken row-by-row on a 2-D image would smear
vertical structure it never looks at, which is why the two-dimensional
version exists as its own function rather than a loop over `medfilt`. The
example below builds a small, badly salt-and-peppered checkerboard as a
matrix, turns it into an `Image`, and runs both names of the filter over
it — the point being that they are the same function, so nothing about
choosing one name over the other changes the result:

```qu
m = [10, 200, 10; 200, 10, 200; 10, 200, 10]   # a badly salt-and-peppered checkerboard
img = image_from_matrix(m)

a = medfilt2(img, 3)
b = median_filter(img, 3)      # same filter, other name
print("{a.width}x{a.height} cleaned, {b.width}x{b.height} cleaned")
```
