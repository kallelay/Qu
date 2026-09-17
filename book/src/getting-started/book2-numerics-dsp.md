# Book 2 — Numerics, DSP, and Linear Algebra

Assumes Book 1. This is where Qu is meant to replace a MATLAB or NumPy
script outright: FFTs, matrix decompositions, signal synthesis, and the
crest-factor / smooth-max tools this language was originally built around.

A running theme in this book: **floating-point roundoff is real and visible**.
A signal that's exactly periodic in theory will show FFT bins around `1e-16`
instead of exactly `0`; a matrix times its own inverse will show off-diagonal
terms around `1e-15` instead of exactly `0`. Qu doesn't hide this — the
default `print` formatting shows tiny magnitudes in scientific notation rather
than silently rounding them to a misleading `0`. Use a format spec
(`{x:.4f}`) whenever you want a clean, rounded read-out for a report or a
teaching example.

## 1. The discrete Fourier transform

`fft`/`ifft` operate on real or complex sample vectors, **any length**.
Power-of-two lengths use a fast radix-2 kernel directly; any other length
dispatches to Bluestein's algorithm (the chirp z-transform), which
re-expresses the DFT as a linear convolution and evaluates it with three
radix-2 FFTs — still `O(n log n)`, still exact:

```qu
t = 1, 2, ..., 50        # N = 50, not a power of two
x = sin(t)
X = fft(x)
print("length(X) = {length(X)}")
print("round trip err = {max(abs(real(ifft(X)) - x)):.2e}")
```
```
length(X) = 50
round trip err = 6.99e-15
```

```qu
N = 8
n = 0 to N - 1
x = cos(2 * pi * 2 * n / N)     # a pure tone at bin 2
X = fft(x, N)
print("mag = {abs(X)}")
```
```
mag = [2.449294e-16, 3.463824e-16, 4, 3.463824e-16, 2.449294e-16, 3.463824e-16, 4, 3.463824e-16]
```

Bins 2 and 6 (the tone and its mirror image) show magnitude 4; everything
else is floating-point noise around zero, not four other real tones. Round it
for a clean read-out: `{abs(X):.4f}`.

`ifft(fft(x))` round-trips to machine precision:

```qu
xr = real(ifft(X, N))
print("round-trip err = {max(abs(xr - x)):.2e}")
```
```
round-trip err = 9.86e-32
```

### Related transforms

- **`fftc(x)`** — an explicit-complex alias of `fft` (identical function).
- **`rfft(x)` / `irfft(X, n)`** — the real-optimized half
  spectrum. A real signal's spectrum is Hermitian-symmetric, so `rfft`
  returns only the non-negative-frequency half (`floor(N/2)+1` bins);
  `irfft` reconstructs the real signal from it. (`fftr` is a deprecated
  second name for `irfft` — it reads like a forward transform and is not
  one.) It reconstructs from
  it — `n` is required explicitly since it can't be recovered from the
  half-spectrum's length alone:

  ```qu
  half = rfft(x)               # length floor(N/2)+1
  back = irfft(half, length(x))
  ```

- **`dct(x)` / `idct(X)`** — the orthonormal DCT-II/DCT-III pair (MATLAB
  `dct`/`idct`, `scipy.fft.dct(norm='ortho')` convention). Round-trips
  exactly, since an orthonormal basis's inverse is exactly its transpose:

  ```qu
  c = dct([1, 2, 3, 4, 5, 6, 7])
  xr = idct(c)                 # xr == the original signal
  ```

- **`dwt(x)` / `idwt(D)`** — one level of the Haar discrete wavelet
  transform (the simplest *exact*, dependency-free wavelet; other families
  such as Daubechies are future work, not silently approximated as Haar).
  Since Qu doesn't yet have multi-value returns, the two output bands come
  back as the rows of a `(2, N/2)` matrix:

  ```qu
  D = dwt(x)
  approx = D[0, :]
  detail = D[1, :]
  xr = idwt(D)                 # reconstructs x exactly
  ```

- **`stft(x, nfft[, hop])`** — the short-time Fourier transform: a
  periodic-Hann-windowed FFT per frame, assembled as an `(nfft, n_frames)`
  complex matrix (one column per frame). `abs(stft(x, ...))` is a
  spectrogram; default hop is 50% overlap:

  ```qu
  S = stft(x, 256, 128)
  spectrogram = abs(S)
  ```

- **`dft(x)` / `idft(X)`** — the direct, textbook `O(n^2)` transform
  (as opposed to `fft`'s `O(n log n)` Bluestein-accelerated one) — an
  independent reference to check `fft` against, not a faster alternative:

  ```qu
  err = max(abs(fft(x) - dft(x)))   # ~0 to floating precision
  ```

- **`goertzel(x, k)` / `goertzel_freq(x, fs, freq)`** — the DFT value at a
  single bin `k` (or, via `goertzel_freq`, a frequency in Hz) in `O(n)`
  instead of paying for a whole FFT to read off one bin. `k` need not be
  an integer: evaluating at a non-integer `k` gives the DFT's value at an
  arbitrary point between the FFT's own grid bins, which is what makes
  Goertzel useful as a cheap "check just these frequencies" primitive
  (DTMF tone detection, a single known carrier) rather than only a fast
  path to an on-grid bin:

  ```qu
  fs = 8000
  tone_hz = 941                  # one DTMF tone
  strength = abs(goertzel_freq(samples, fs, tone_hz))
  ```

- **`vanicek(t, x, freqs)`** — Vaníček's Least-Squares Spectral Analysis
  (LSSA): a power spectrum at the requested `freqs`, valid for
  **unevenly-sampled** `(t, x)` — the one case `fft`/`dft`/`goertzel` all
  refuse to handle, since each assumes uniform sampling. At each
  frequency it fits `x(t) ~= a*cos(2*pi*f*t) + b*sin(2*pi*f*t)` by
  ordinary least squares and reports `(a^2+b^2)/var(x)` — a pure sinusoid
  scores close to `2.0` at its own frequency and far lower elsewhere:

  ```qu
  power = vanicek(t, x, [1, 3, 5, 8, 12])   # t need not be evenly spaced
  ```

### Convolution, correlation, the analytic signal, and peak detection

- **`conv(x, h, [mode="full"])`** — discrete convolution, MATLAB's own
  `mode` naming: `"full"` (default, every nonzero-overlap sample),
  `"same"` (the middle `length(x)` samples, aligned with the input — the
  usual choice for filtering in place), `"valid"` (only where `h` fully
  overlaps `x`):

  ```qu
  y = conv([1, 2, 3], [0, 1, 0.5])                  # [0, 1, 2.5, 4, 1.5]
  filtered = conv(signal, kernel, mode="same")       # same length as signal
  ```

- **`xcorr(x, [y])`** — cross-correlation over the full lag range
  (`length(x)+length(y)-1` values); autocorrelation when `y` is omitted.
  Zero lag sits at the middle index for equal-length inputs, and an
  autocorrelation's value there is exactly `sum(x.^2)` (the signal's own
  energy) — the peak an autocorrelation is guaranteed to hit:

  ```qu
  r = xcorr(x)             # autocorrelation
  lag = xcorr(a, b)        # cross-correlation between two signals
  ```

- **`hilbert(x)`** — the analytic signal `x + j*H[x]` (a `CVec`), via the
  standard FFT-based construction. `abs(hilbert(x))` is `x`'s envelope;
  `imag(hilbert(x))` is the Hilbert transform of `x` itself:

  ```qu
  envelope = abs(hilbert(am_signal))   # recovers an AM envelope
  ```

- **`find_peaks(x, [min_height=], [min_distance=], [min_prominence=])`** —
  indices of `x`'s local maxima. `min_distance` (samples) is enforced
  greedily by height, so a tight cluster of close peaks keeps only its
  tallest member rather than just the first one scanned; `min_prominence`
  filters by how much a peak rises above its *own* surrounding terrain,
  not just its absolute height (rejecting a small shoulder bump riding on
  a much taller peak, for instance):

  ```qu
  peaks = find_peaks(spectrum, min_height=0.1, min_distance=5)
  ```

  `find_peaks` returns *local* maxima, so the single largest one is
  whichever candidate has the biggest value — `max(x[peaks])`, or a small
  loop over `peaks` if you want its index too. `engine/examples/
  peak_finding.qu` is a full worked example that cross-checks that
  "biggest local max" answer against `max`/`argmax` and a hand-written
  brute-force scan, confirms all three agree, and annotates the winner on
  a plot with `point`/`annotate`.

- **`mag2db(x)` / `db2mag(db)`** and **`pow2db(x)` / `db2pow(db)`** —
  decibel conversion for amplitude/voltage ratios (factor of 20) and power
  ratios (factor of 10) respectively, named MATLAB's way rather than one
  ambiguous `db()` — which factor applies is a naming decision, not a
  silent convention:

  ```qu
  print("{mag2db(2)} dB")   # 6.0206 -- a factor-of-2 voltage ratio
  print("{pow2db(2)} dB")   # 3.0103 -- a factor-of-2 power ratio
  ```

- **`corr(x, y)`** — the Pearson correlation coefficient, a single scalar
  in `[-1, 1]` (as opposed to `xcorr`, which returns a whole vector over
  every lag). `corr(x, x) == 1` for any non-constant `x`:

  ```qu
  r = corr([1,2,3,4,5], [2,1,4,3,5])   # 0.8
  ```

- **`energy(x)`** / **`tkeo(x)`** — `energy` is total signal energy
  (`sum(x.^2)`, no time localization); `tkeo` is the Teager-Kaiser Energy
  Operator, a cheap nonlinear *instantaneous* energy tracker
  (`psi[x(n)] = x(n)^2 - x(n-1)*x(n+1)`, single-sample resolution, three
  multiplies per sample — far cheaper than a full `hilbert`-based envelope
  when you just need an onset/energy estimate):

  ```qu
  e = energy(signal)
  instant_energy = tkeo(signal)   # length(signal) - 2 (needs a neighbor each side)
  ```

- **`ste(x, [win=256], [hop=win/2])`** — short-time energy: the windowed
  extension of `energy(x)` over sliding, overlapping frames (each
  Hann-windowed, like `stft`), giving one energy value per frame instead
  of a single total. The answer to "what's energy as a function of time
  called" — the standard way to locate bursts of activity in a signal,
  e.g. muscle-activation onsets in an EMG recording:

  ```qu
  e_t = ste(emg_signal, 200, 100)   # energy per 200-sample frame, 50% overlap
  ```

- **`spectral_entropy(x, [nfft=256], [hop=nfft/2])`** — the Shannon
  entropy of each `stft` frame's normalized power spectrum, scaled to
  `[0, 1]`: low when a frame's energy concentrates in a few frequency bins
  (a clean tone), high when it's spread broadly (noise, or a complex
  signal). The other half of the "energy or entropy as a function of
  time" answer — where `ste` tracks *how much* signal there is, this
  tracks *how tonal vs. noisy* it is:

  ```qu
  h = spectral_entropy(emg_signal, 256, 128)
  print("mean spectral entropy: {mean(h)}")
  ```

### Adaptive decomposition: EMD and VMD

Unlike `fft`/`dct`/`dwt` (which project onto a *fixed* basis), Empirical
Mode Decomposition and Variational Mode Decomposition split a signal into
a small number of oscillatory modes discovered from the data itself — the
right tool when the dominant frequencies drift over time (nonstationary,
nonlinear signals) rather than staying fixed.

- **`emd(x, [max_imfs=])`** — the classical sifting algorithm (Huang et
  al., 1998): repeatedly subtracts the mean of the upper/lower envelopes
  (fit through the signal's own local maxima/minima via a cubic spline)
  until each intrinsic mode function (IMF) is extracted, highest frequency
  first, stopping once what's left has too few extrema to be
  meaningfully oscillatory. Returns a `model(...)` (fields `imfs` — a
  matrix, one IMF per row — and `residual`, the overall trend). Sifting
  only ever subtracts, so reconstruction is exact by construction:

  ```qu
  e = emd(signal)
  recon = sum(e.imfs, axis=0) + e.residual   # == signal, to floating precision
  first_imf = e.imfs[0, :]                    # the highest-frequency mode
  ```

- **`vmd(x, k, [alpha=2000])`** — Variational Mode Decomposition
  (Dragomiretskiy & Zosso, 2014): a non-recursive alternative that solves
  for exactly `k` band-limited modes at once, by minimizing each mode's
  bandwidth around its own center frequency (an ADMM optimization in the
  frequency domain, not sifting). Less prone to the mode-mixing EMD can
  suffer on noisy data, at the cost of needing `k` decided up front.
  Returns a `model(...)` (fields `modes` and `center_freqs`, each mode's
  converged frequency normalized to cycles/sample):

  ```qu
  v = vmd(signal, 2)                 # exactly 2 modes
  freqs_hz = v.center_freqs .* fs    # convert to Hz
  ```

  **Simplified versus the reference implementation**: the original paper's
  own code mirror-pads the signal before transforming to suppress edge
  artifacts; this implementation works directly on the signal's own
  length instead — simpler, correct in the interior, but with more
  pronounced edge effects near the boundaries than the mirrored version.

### Filter design: Butterworth, batch and streaming

`butter(order, kind, cutoff, fs)` designs a digital Butterworth filter —
lowpass, highpass, bandpass, or notch (bandstop) — as a cascade of
second-order sections (SOS), the numerically stable way to realize a
filter of any order (a single high-order transfer function's own
coefficients become unstable well before order 10; cascaded biquads don't
have that problem at any practical order). `kind` is `"low"`/`"high"`/
`"band"`/`"stop"`, with `"lowpass"`/`"highpass"`/`"bandpass"`/
`"bandstop"`/`"notch"` all accepted as more readable aliases. `cutoff` is
one frequency in Hz for low/high, or `[low, high]` for band/stop:

```qu
fs = 1000
lp = butter(4, "low", 100, fs)          # 4th-order lowpass, 100 Hz cutoff
notch = butter(2, "notch", [45, 55], fs)  # removes 45-55 Hz (mains hum)
```

Two ways to apply it, matching whether you have the whole signal already
or are receiving samples one at a time:

- **Batch** — `sosfilt(filt, x)` (causal, one pass, has the usual IIR
  phase lag) and `filtfilt(filt, x)` (filters forward then backward,
  canceling phase distortion entirely — the standard choice for offline
  analysis, at double the effective filter order):

  ```qu
  filtered = sosfilt(lp, signal)
  zero_phase = filtfilt(lp, signal)
  ```

- **Streaming** — for real-time or serial-fed data where a whole array
  isn't available up front: `state = filter_init(filt)`, then repeatedly
  `state = filter_next(state, next_sample)`, reading `state.y` for each
  filtered output. This is the same "thread a new state through each
  call" idiom `kalman_predict`/`kalman_update` already use — Qu has no
  mutable-iterator-object primitive, so the state itself is threaded
  through, not mutated in place. It's mathematically identical to
  `sosfilt` — feeding a whole signal through one sample at a time gives
  the same result as `sosfilt` on the whole array:

  ```qu
  state = filter_init(lp)
  for i = 0 to length(samples) - 1
      state = filter_next(state, samples[i])
      print(state.y)         # this sample's filtered output, right away
  end for
  ```

## 2. Matrices: the linear-algebra floor

Qu treats `matmul`, `dot`, `norm`, `inv`, `pinv`, transpose, and matrix power
as core language features, not an optional package — see spec §23.

```qu
a = [1, 2, 2]
print("dot(a,a) = {dot(a,a)}")   # 9
print("norm(a) = {norm(a)}")     # 3
```

Inverse and the identity-check idiom:

```qu
A = [4, 7; 2, 6]
print("A*inv(A) = {A * inv(A)}")
```
```
A*inv(A) = [1, -1.332268e-15; 0, 1]
```

That `-1.332268e-15` is floating-point noise, not a bug — `A * inv(A)` is
mathematically the identity; round for display (`{... :.6f}`) when it matters
for a report.

For non-square or rank-deficient matrices, `M^-1`/`inv(M)` raise a clear error
(never a silent `Inf`-filled matrix); use `pinv(M)` for the Moore-Penrose
pseudoinverse instead, which is always defined:

```qu
tall = [1, 2; 3, 4; 5, 6]
P = pinv(tall)
```

Matrix left-division `A \ b` solves `A*x = b` for `x` — exact for a square
non-singular `A`, least-squares-best for an overdetermined (tall) one:

```qu
A = [4, 7; 2, 6]
b = [1; 1]
x = A \ b                              # same result as inv(A) * b here
```

`chol(A)` is the Cholesky factor `L` of a symmetric positive-definite `A`
(`A = L * L^T`, `L` lower-triangular) — the standard way to sample
correlated Gaussians or update a covariance matrix, and cheaper than `inv`
when it applies:

```qu
A = [25, 15, -5; 15, 18, 0; -5, 0, 11]
L = chol(A)
print("L * L' == A: {L * L'}")
```

A non-positive-definite `A` (e.g. `[1, 2; 2, 1]`, which has a negative
eigenvalue) raises a clear error rather than returning a NaN-filled result.

`chol` also unlocks `mvnpdf(x, mu, Sigma)`, the multivariate Gaussian
density — `x` is either one point (a plain vector, same dimension as `mu`)
or a matrix whose rows are points, `mu` is the mean vector, `Sigma` the
`(k,k)` covariance:

```qu
mu = [0, 0]
Sigma = [4, 0; 0, 9]
p = mvnpdf([2, 3], mu, Sigma)          # density at one point
ps = mvnpdf([0,0; 2,3; -2,-3], mu, Sigma)  # one density per row
```

`normpdf(x, mu, sigma)` is the univariate case (`sigma` is the standard
deviation, matching `normal`/`randn`'s own parameterization) and broadcasts
over a vector `x` the same way every elemental-math builtin does.

**`det`/`rank`/`svd`/`qr`/`lu`/`eig`** round out the matrix decompositions,
all via `nalgebra`'s own implementations (the same dependency `chol`/`pinv`
already build on):

```qu
A = [4, 7; 2, 6]
print("det(A) = {det(A)}")     # 10
print("rank(A) = {rank(A)}")   # 2
```

`svd(A)`, `qr(A)`, and `lu(A)` each return a `model(...)` with named
read-only fields (the same multi-return convention as `ols_model`/
`kalman_init`) rather than a tuple, since these calls don't have anywhere
else to put more than one output:

```qu
s = svd([1, 2; 3, 4; 5, 6])
S = [s.s[0], 0; 0, s.s[1]]
print("U*S*Vt reconstructs A: {s.u * S * s.vt}")

r = qr([1, 2; 3, 4; 5, 6])
print("Q*R == A: {r.q * r.r}")

f = lu([2, 1, 1; 4, 3, 3; 8, 7, 9])
print("P*A == L*U: {f.p * [2,1,1;4,3,3;8,7,9]} vs {f.l * f.u}")
```

`eig(A)` dispatches on symmetry (checked directly, not assumed): a
symmetric `A` gets real eigenvalues *and* real eigenvectors
(`e.values`/`e.vectors`, via the numerically preferred symmetric
algorithm); a general `A`'s eigenvalues may be complex, and
`e.vectors` is `nothing` in that case — a general matrix's eigenvectors
would themselves be complex-valued, which isn't computed here, not
silently approximated:

```qu
sym = [2, 0; 0, 3]
e = eig(sym)
print("eigenvalues = {e.values}")
print("eigenvectors = {e.vectors}")

rot = [0, -1; 1, 0]     # a rotation-like matrix: eigenvalues +-i
er = eig(rot)
print("complex eigenvalues = {er.values}")
print("eigenvectors = {er.vectors}")   # none: rot isn't symmetric
```

## 3. Complex matrices

Complex literals inside a matrix promote the whole literal to complex:

```qu
C = [1+2j, 3+4j; 0+1j, 2-1j]
```

Every real-matrix operation has a complex counterpart: `matmul`, broadcasting,
`^-1` (via a portable Gauss-Jordan elimination), and both transpose families
(`'`/`.H` conjugate, `.'`/`.T` don't — see Book 1 §6 for why there's no
`^H`/`^T`). A full complex least-squares solve — the normal-equations form
`x = (AᴴA)⁻¹Aᴴb` — reads exactly like the linear algebra:

```qu
D = [4, 6]
D as vector(2, 1)
E2 = (C.H * C)^-1 * C.H * D
print("E2 = {E2}")
```
```
E2 = [-1.25 - 3.5i; 0.75 + 1i]
```

**A genuine mathematical trap, not a Qu bug:** if your data matrix has fewer
independent rows than columns, `AᴴA` is rank-deficient and its inverse does
not exist — Qu raises a clear "matrix is singular" error rather than a
silently wrong answer. This is correct behavior: a single 2-element row
vector `C = [1+2j, 3+4j]` has `CᵀC` as a rank-1 outer product, which a 2×2
inverse cannot resolve, regardless of what language you're doing the algebra
in.

## 4. Signal synthesis and the multisine idiom

The canonical Qu multisine pattern is an **outer-product broadcast**: a
frequency column times a time row gives every tone's instantaneous phase in
one matrix, no explicit loop over tones:

```qu
K = 4
N = 64
Fs = 64
bins = [2, 5, 11, 17]
f = bins * (Fs / N)
t = 0 to (N - 1) / Fs step 1/Fs

f as vector(K, 1)      # frequency column (K,1)
t as vector(1, N)      # time row        (1,N)

phase = 2 * pi * f * t             # (K,1)*(1,N) -> (K,N), one row per tone
A = ones(K, 1) / sqrt(K)           # unit-RMS amplitude per tone
x = sum(A .* cos(phase), axis=0)   # sum the tones down the rows
```

This is the same shape discipline as NumPy broadcasting (`f[:, None] *
t[None, :]`), spelled with explicit orientation contracts instead of
`None`-indexing tricks.

A real `multisine(freqs, amps, fs, n, [phases=])` builtin now also exists,
built on the same synthesis kernel `qu-gpu`'s own batched multisine
parity-tests against — the manual outer-product idiom above still works
identically (useful when you need the intermediate `phase` matrix itself,
e.g. for a per-tone gradient), but for the common case a single call
replaces it:

```qu
x = multisine([2, 5, 11, 17] * (64 / 64), [1, 1, 1, 1], 64, 64)
```

Leaving `phases` unset uses **Schroeder's phase formula**
(`phi_k = -k(k-1)*pi/K`) — a well-known deterministic choice that keeps
crest factor low without the iterative optimization §5 covers, a good
default test signal before reaching for the real optimizer.

The rest of the signal-generation family, all returning a `Signal` (so
`x.Fs` survives, same convention as `signal(...)`):

```qu
s = sine(1 kHz, 48 kHz, 4800)             # a single tone
c = chirp(20, 20000, 48 kHz, 48000)       # linear sweep, 20Hz to 20kHz
lc = chirp(20, 20000, 48 kHz, 48000, method="logarithmic")
sq = square(100, 1000, 1000, duty=0.3)    # bipolar (+-1) pulse train
p = pwm(modulator, 5000, 48000)           # natural-sampling PWM against
                                           # a triangle carrier (duty
                                           # follows the modulator's value)
```

`impulse(n, [index=], [amplitude=])` is the one exception — it returns a
plain `Vec` (a Kronecker delta doesn't inherently need a sample rate to
mean something; wrap it in `signal(impulse(n), fs)` if you need one).

**Binarized excitation signals** compose from the pieces above with `|>`,
rather than needing dedicated builtins — `sign` (already real, elementwise
±1) and the new `sigma_delta` (a first-order noise-shaping 1-bit
quantizer, better signal fidelity at the cost of a higher switching rate —
the standard tradeoff in the literature) are both general DSP primitives,
not signal-specific ones:

```qu
signum_chirp = chirp(20, 20000, 48 kHz, 4800) |> sign

# DIBS (Discrete-Interval Binary Sequence): a multisine tuned to
# concentrate power at chosen frequencies, then binarized
excitation = multisine([50, 150, 400], [1, 1, 1], 1000, 4000)
dibs_signum = excitation |> sign            # simple thresholding
dibs_sigma_delta = excitation |> sigma_delta  # higher fidelity, more switching
```

## 5. Smooth maxima: logsumexp, smoothmax, softmax

`max` is not differentiable at the peak, so any quantity defined by a peak
is awkward to put inside a gradient method. The crest factor of a signal,
`max(abs(x)) / rms(x)` — a standard measure of how much of a converter's
dynamic range a waveform wastes on its largest excursion — is the usual
example. Qu ships the **LogSumExp** smooth-max family as a first-class
numerical primitive:

```qu
crest(x) := max(abs(x)) / rms(x)

xs = [0, 1, 2, 3]
print("hard max = {max(xs)}")
print("logsumexp = {logsumexp(xs):.4f}")       # >= hard max, a proven upper bound
print("smoothmax b=50 = {smoothmax(xs, 50):.4f}")  # -> hard max as beta grows
print("softmax = {softmax(xs)}")                # the gradient of logsumexp
```
```
hard max = 3
logsumexp = 3.4402
smoothmax b=50 = 3.0000
softmax = [0.032059, 0.087144, 0.236883, 0.643914]
```

`logsumexp(x) = (1/beta) * log(sum(exp(beta*x)))` is a smooth, convex upper
bound on `max(x)`, with an explicit, tight error bound of `log(n)/beta`.
`softmax` is its gradient — a probability distribution over the input that
concentrates on the largest elements as `beta` grows.


### 5.1. Hoist what does not depend on the variable

A `:=` function called inside an optimizer loop usually mixes two kinds of
work: the part that changes with its parameter, and the part that does not.
Only the first has to be inside.

```qu
# `grid` does not depend on `p`, so building it per call is waste.
grid   = outer(f, t)            # K x N, built ONCE
shape(p) := sum(A .* cos(grid + p), axis = 0)
```

Qu's tree-walking interpreter has no optimizing pass between the parser and
evaluation, so it will not notice for you: a loop-invariant expression left
inside a hot function body is re-evaluated every call. Hoisting it by hand
is the same move you would make with a `for` loop, and on a `K x N` grid
called a few hundred times it is worth more than any other single change.

## 6. Spectrograms: watching frequency change over time

`fft(x)` collapses a whole signal into one spectrum — fine when the content
is stationary, useless the moment a signal's frequency *changes* partway
through (a chirp, a word, a musical note). `spectrogram` slides a window
across `x` (the same short-time Fourier transform behind `stft`), takes the
magnitude of each windowed frame, and renders the whole time-frequency grid
as a heatmap in one call — no separate `stft` + `abs` + `heatmap` assembly
required:

```qu
Fs = 1000
n = 0:1:1023
t = n / Fs
x = sin(2*pi*80*t) + sin(2*pi*300*t)
spectrogram(x, 128, 32, Fs)
title("Two stationary tones")
xlabel("time (s)")
ylabel("frequency (Hz)")
```

Signature: `spectrogram(x, [nfft=256], [hop=nfft/2], [fs=1])` — the same
`nfft`/`hop` framing as `stft`, plus `fs` so the axes read in real Hz and
seconds instead of bin/frame indices. A few things worth knowing:

- Only the positive-frequency half is shown (`nfft/2 + 1` rows) — a real
  signal's spectrum mirrors around Nyquist, so the other half is redundant.
- Magnitude is shown in dB (`20*log10`, floored at -120dB), not linear —
  a few loud low-frequency bins would otherwise wash out everything else on
  a linear color scale.
- Frequency increases *upward* (row 0 is Nyquist, the bottom row is 0Hz),
  matching the usual reading convention for a spectrogram image.

## 7. Random distributions

`rand`/`randn` (uniform-on-[0,1) and standard-normal) were already used above
for synthetic noise. The rest of the RNG family follows the same two
conventions: **distribution parameters come first, size after** (`rows`,
then `cols`, both defaulting to `1`), and an explicit `seed=` gives a fresh,
reproducible local stream instead of advancing the shared one:

```qu
u = uniform(10, 20, 5)          # 5 draws, continuous uniform on [10, 20)
g = normal(5, 2, 1000, seed=1)  # 1000 draws, mean 5, std 2 — randn is mu=0/sigma=1
d = randi(1, 6, 10)             # 10 dice rolls, inclusive both ends
k = poisson(4, 500)             # 500 draws from Poisson(lambda=4)
t = exponential(2, 500)         # 500 draws, rate=2 (mean = 1/rate = 0.5)
b = binomial(20, 0.3, 500)      # 500 draws, 20 trials at p=0.3 each
```

A few things worth knowing:
- `randi`'s bounds are inclusive on both ends (MATLAB's convention), not
  NumPy's half-open `randint`.
- `exponential` takes a **rate** (mean = `1/rate`), the same parameterization
  `poisson`'s own `lambda` uses — not NumPy's `scale = 1/rate` convention.
- `poisson` and `binomial` are exact samplers (Knuth's algorithm, and a sum
  of Bernoulli draws, respectively) rather than approximations, so they get
  slow — not wrong — for very large `lambda`/`n_trials` in a tight loop.
- All of them respect `seed(n)`'s persistent stream the same way `rand`/
  `randn` do, so a whole script re-run with the same `seed(n)` at the top
  reproduces bit-for-bit.

`chisquare(k, ...)` rounds out the sampling side — `k` degrees of freedom,
constructed directly as the sum of `k` squared standard-normal draws:

```qu
x = chisquare(4, 1000, seed=1)
print("sample mean: {mean(x)}")   # converges to k=4, the distribution's own mean
```

`chi2pdf(x, k)`/`chi2cdf(x, k)` are the density and cumulative distribution
— goodness-of-fit tests and confidence regions read off `chi2cdf`, e.g. the
familiar `p = 0.05` critical value for 1 degree of freedom:

```qu
p = chi2cdf(3.841459, 1)   # 0.95 -- the textbook stats-table value
```

Density/CDF evaluation is a separate feature from *sampling* — `mvnpdf`/
`normpdf` (§2, alongside `chol`) are the Gaussian-family equivalents; no
Poisson/exponential/binomial PDF or CDF exists yet.

## 8. Estimation theory: Kalman, extended/unscented Kalman, and particle filters

Every filter below is built from the same three verbs — `predict`,
`update`, `estimate` — called as `state.predict(...)`/`state.update(...)`/
`state.estimate()`, the same `recv.method(args)` sugar every fitted model
in this language already uses (`model.predict(X)`, `model.score(X)`).
There is no separate `kalman_predict`/`ekf_update`/`particle_filter_...`
family of names: `predict`/`update` dispatch on the state's own kind (and,
for the Kalman family, on whether the second argument is a matrix or a
function name), so the same two calls work whether the filter underneath
is linear, extended, or unscented. Each call returns a *new* state rather
than mutating one in place — the same idiom `filter_init`/`filter_next`
(§1) already use, since Qu has no mutable-iterator-object primitive.

**The linear Kalman filter** — exact for a linear system with Gaussian
noise: `x' = F*x + noise`, measurement `z = H*x + noise`. `kalman_init(x0,
P0)` sets the initial estimate and its covariance; `predict(F, Q)` is the
time update, `update(H, z, R)` is the measurement update:

```qu
s = kalman_init([0], [10])       # 1-D state, initial uncertainty 10
for i = 1 to 8
    s = s.predict([1], [0.01])   # static state: F=1, small process noise Q
    s = s.update([1], [2], [1])  # repeatedly measuring z=2, H=1, R=1
end for
print("x = {s.x}, P = {s.P}")   # x converges toward 2, P shrinks as evidence accumulates
```

**The extended Kalman filter (EKF)** — for a *nonlinear* `process_fn`/
`observation_fn`, still with (approximately) Gaussian noise: the function
itself is evaluated exactly, only its Jacobian is linearized around the
current estimate to propagate the covariance. Pass a function name where
`F`/`H` was a matrix above — same `kalman_init` state, same `predict`/
`update` names, just a different argument type selecting the nonlinear
path:

```qu
proc(x) := x                       # static state (nonlinear in general)
obs(x) := [x[0]^2]                 # nonlinear observation, e.g. a squared reading
jac_obs(x) := [2*x[0]]             # its exact Jacobian, dobs/dx

s = kalman_init([2], [10])         # initial guess x0=2, true x is 3 (z=9=3^2)
for i = 1 to 10
    s = s.predict("proc", [0.001])
    s = s.update("obs", [9], [0.5], jac="jac_obs")
end for
print("x = {s.x}")   # converges toward 3
```

`jac=` is optional — omit it and `predict`/`update` fall back to a
central-difference approximation of the Jacobian (the same numerical
fallback `minimize`/`curve_fit` use for their own derivatives), which is
enough for a smooth nonlinearity like the one above; give it when the
exact Jacobian is cheap to write, for less approximation error.

**The unscented Kalman filter (UKF)** — a second way to handle the same
nonlinear `process_fn`/`observation_fn`, without a Jacobian at all: it
propagates a small deterministic set of "sigma points" through the
function exactly and recombines their mean/covariance, rather than
linearizing. More expensive per step (`2n+1` function calls instead of
one function call plus a Jacobian) and more accurate for a strongly
curved nonlinearity. Add `method="ukf"` to the exact same `predict`/
`update` calls — everything else about the script above is unchanged:

```qu
s = kalman_init([2], [10])
for i = 1 to 10
    s = s.predict("proc", [0.001], method="ukf")
    s = s.update("obs", [9], [0.5], method="ukf")
end for
print("x = {s.x}")   # converges toward 3, same as the EKF version above
```

`method="ukf"` takes its own optional `alpha=`/`beta=`/`kappa=` (defaults
`1e-3`/`2.0`/`0.0`, the standard scaled-sigma-point parameters) instead of
`jac=` — giving both together is a clear error, since the unscented filter
never linearizes anything. For a genuinely *linear* `process_fn`/
`observation_fn`, UKF and the plain linear Kalman filter agree exactly:
propagating sigma points through a linear map and recombining reproduces
`F*x`/`F*P*F^T` to floating-point precision, the standard sanity check
that sigma points are a generalization of the linear math, not an
approximation of it.

**The particle filter** — for everything none of the three above can
handle: strongly nonlinear models, non-Gaussian noise, multimodal
beliefs. Instead of tracking a mean and covariance analytically, it
represents the belief as a weighted set of sample states ("particles").
`process_fn`/`likelihood_fn` are ordinary user-defined Qu functions — the
same "by name" convention `minimize`/`curve_fit` use — since a particle
filter has no closed form to fall back to. `predict`/`update` dispatch to
it the same way, by the state's own kind; `estimate()` (the particle
filter's own third verb — the linear/EKF/UKF states don't need it, since
their point estimate is just `state.x`) reads off the weighted-mean point
estimate:

```qu
process(p) := p + 0.05*randn()                       # process_fn injects its
                                                        # own noise, like monte_carlo
likelihood(p, z) := exp(-(p[0]-z[0])^2 / (2*0.25))    # Gaussian likelihood, sigma=0.5

pf = particle_filter_init([0], 200, spread=2.0, seed=1)   # 200 particles around x0=0
for i = 1 to 10
    pf = pf.predict("process")
    pf = pf.update("likelihood", [2])    # repeatedly measuring z=2
end for
est = pf.estimate()   # weighted-mean point estimate, converges toward 2
```

`update` reweights every particle by `likelihood_fn`, renormalizes, then
automatically resamples (systematic resampling) once the effective sample
size drops below `resample_threshold * n` (default `0.5`) — the standard
guard against *degeneracy*, where almost all weight collapses onto a
handful of particles after a few steps. The returned state's `neff` field
is that effective sample size (before any resampling), and `resampled`
says whether this call triggered it.

## 9. Numerical optimization: root-finding, curve fitting, minimization

Every optimization builtin below takes the target as a **function name**
(a string), the same "by name" convention `pmap`/`spawn` already use —
only a function *you defined* can be the target, not a builtin.

**Root-finding** — two algorithms with different tradeoffs:

```qu
f(x) := x^2 - 2
root = fzero("f", 0, 2)          # Brent's method: needs a bracket
                                   # [a,b] where f changes sign, always
                                   # converges, no derivative needed

fprime(x) := 2*x
root2 = newton("f", "fprime", 1.0)  # Newton's method: fast (quadratic
                                      # convergence) when it works, but
                                      # needs a derivative and can diverge
```

**Nonlinear curve fitting** — `curve_fit(model, xdata, ydata, p0)` finds
the parameters minimizing `sum((model(x, params) - y)^2)` via
Levenberg-Marquardt (damped Gauss-Newton), with a numerically-computed
Jacobian (Qu has no automatic differentiation):

```qu
model(x, p) := p[0] * exp(-p[1] * x)
fit = curve_fit("model", xdata, ydata, [1, 1])   # initial guess [1, 1]
print("a = {fit.params[0]}, b = {fit.params[1]}, cost = {fit.cost}")
```

**General minimization** — `minimize(f, x0, [grad=])` is L-BFGS, a
quasi-Newton ("pseudo-Newton" — it approximates the inverse Hessian from
recent gradient history instead of computing it outright) multivariate
minimizer. By default the gradient is computed numerically (central
differences):

```qu
bowl(p) := (p[0]-3)^2 + (p[1]+2)^2
m = minimize("bowl", [0, 0])
print("minimum at {m.params}, value {m.value}")
```

If you can write the gradient by hand, pass it as `grad=` — a function
`params -> gradient vector`, the same shape as `f` itself. An analytical
gradient is both cheaper (one function call per step instead of `2n`,
one per parameter in each direction) and exact, where finite differences
are only ever an approximation:

```qu
bowl_grad(p) := [2*(p[0]-3), 2*(p[1]+2)]
m2 = minimize("bowl", [0, 0], grad="bowl_grad")   # same minimum, fewer
                                                    # evaluations per step
```

`minimize` only finds the *local* minimum nearest its starting point.
**`basin_hopping(f, x0, [grad=], [n_iter=], [step_size=], [temperature=],
[seed=])`** adds a global search on top: it repeatedly perturbs the
current point with random noise and re-runs `minimize` from there,
accepting a worse local minimum with Metropolis probability
`exp(-(new-old)/temperature)` instead of always rejecting it — letting
the search escape the first local minimum it lands in. `grad=`, same
convention as `minimize`, is forwarded to every one of its local
searches:

```qu
wavy(p) := -0.5*exp(-(p[0]+2)^2) - 2*exp(-(p[0]-3)^2/2)  # shallow local
                                                            # min at -2,
                                                            # deeper global
                                                            # min at 3
stuck = minimize("wavy", [-2])            # stays at the shallow one
found = basin_hopping("wavy", [-2], n_iter=60, step_size=3)  # finds the deep one
```

**Monte Carlo simulation** — `monte_carlo(f, n, [seed=])` calls a
zero-argument function (typically one that draws its own randomness via
`randn`/`rand`) `n` times and collects every result:

```qu
sample() := randn()
draws = monte_carlo("sample", 10000, seed=1)
print("mean = {mean(draws)}, std = {std(draws)}")
```

## 10. Pipelines: chaining transforms with `|>`

`|>` threads a value through a sequence of function calls left to right —
each stage receives the previous stage's result as its first argument, so a
multi-step DSP pipeline reads in the order it actually runs, instead of
nesting inside-out:

```qu
n = 0 to 255
x = sin(n * 0.1) + 0.05 * randn(256)
spectrum_mag = x |> fft |> abs
smoothed = spectrum_mag |> dct |> idct
```

is exactly `smoothed = idct(dct(abs(fft(x))))`, just read forward instead of
backward. `|>` is plain syntax sugar over ordinary calls, not a different
evaluation model — the one rule to remember is that the piped value always
lands in the function's *first* argument slot: `x |> f(a, b)` means
`f(x, a, b)`.

That rule also tells you when *not* to pipe: `polyval`'s signature is
`polyval(coeffs, x)` — the value you're evaluating is the *second*
argument, not the first — so `x |> polyval(c)` would wrongly try
`polyval(x, c)`. Call it directly instead: `polyval(c, x)`.

`pmap` (§4 of [Book 3](book3-specialized.md)) fits this same pipeline style
for parallel per-sample work, e.g. `signals |> pmap("extract_features")`.

## 11. Cheat sheet: NumPy/SciPy → Qu

| NumPy/SciPy | Qu |
|---|---|
| `np.fft.fft(x)` | `fft(x)` (power-of-two length only, today) |
| `np.fft.ifft(X)` | `ifft(X)` |
| `np.abs(X)`, `np.angle(X)` | `abs(X)`, `angle(X)` |
| `A @ B` | `A * B` |
| `A.conj().T` | `A'` (or `A.H`, `ctranspose(A)`) |
| `np.linalg.inv(A)` | `inv(A)` (or `A^-1`) |
| `np.linalg.pinv(A)` | `pinv(A)` |
| `scipy.special.logsumexp` | `logsumexp(x)` |
| `scipy.special.softmax` | `softmax(x)` |
| `np.outer(u, v)` | `u * v` with explicit `as vector(r,c)` orientations |
| `np.linspace(a, b, n)` | `linspace(a, b, n)` |
| `np.logspace(a, b, n)` | `logspace(a, b, n)` (returns `10^a … 10^b`) |

## What's next

**[Book 3 — Specialized Domains](book3-specialized.md)** covers circuits,
machine learning, and distributed execution — mostly a preview of the
specification, since the reference interpreter's M2 milestone is scoped to
the numerics in this book.
