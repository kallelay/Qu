# 10. Random Processes & PSD

Put a hand against a wall that shares a room with a running air
conditioner and a street outside: one sound is a flat, textureless hiss,
the other a rumble that sits low in the chest. Neither repeats, neither
is predictable sample to sample, and yet they are obviously different
kinds of noise. The difference is not loudness — it is where each one's
power sits across frequency, and that is a number you can measure.

## The idea

A **random process** is a signal whose value at any instant is a random
variable rather than a fixed number: you cannot say what sample \(n\)
will be, only how it behaves statistically. That is still useful,
because the process can have a stable mean, a stable variance, and a
relationship between one sample and its neighbors a few steps away. For
that relationship to be a fixed, measurable property of the process
rather than something that changes every time you record it, the process
must be **wide-sense stationary**: its mean and its autocorrelation
depend only on the *lag* between two samples, never on absolute time,

\[
E[x[n]] = \mu, \qquad R_{xx}[k] = E\big[x[n]\,x[n+k]\big] \text{ independent of } n.
\]

`R_xx[k]` is the **autocorrelation**: how strongly a sample predicts one
`k` steps away. In white noise it is a spike at `k = 0` and nothing
elsewhere — no sample tells you anything about its neighbor. In colored
noise it decays smoothly, which is exactly what makes a signal *feel*
slow and heavy rather than textureless.

Norbert Wiener (1930) and Aleksandr Khinchin (1934) independently proved
that for a stationary process the autocorrelation and a frequency-domain
quantity, the **power spectral density**, are a Fourier transform pair —
the Wiener–Khinchin theorem:

\[
S_{xx}(f) = \sum_{k=-\infty}^{\infty} R_{xx}[k]\, e^{-j 2\pi f k}.
\]

`S_xx(f)` answers, with an equation, the question an ear answers
instantly: how much power sits in a narrow band around frequency `f`.
White noise has a flat PSD by definition — every frequency carries equal
power, the acoustic analogy to white light. Colored noise has a PSD that
is not flat; a common family falls off as \(1/f^\alpha\), with `α = 0`
giving white noise, `α = 1` "pink" noise (semiconductor flicker, drift in
long measurements), and `α = 2` a random walk (Brownian/red noise,
baseline wander in a slowly sagging instrument).

```qu
f = linspace(1, 500, 200)
white = ones(length(f))
pink  = 1 / f
brown = 1 / (f .^ 2)
figure()
plot(f, white, label = "white: flat")
plot(f, pink, label = "pink: 1/f")
plot(f, brown, label = "brownian: 1/f^2")
axis scale y log
xlabel("frequency (Hz)")
ylabel("power spectral density")
title("Idealized noise PSD shapes")
legend()
```

You never get `R_xx[k]` from one finite recording without averaging, and
the obvious estimator — the squared magnitude of one FFT, the
*periodogram* — is notoriously noisy: it does not improve as the
recording gets longer, only as it gets *repeated*. Peter Welch's 1967
fix, still the workhorse today, slices the recording into overlapping
segments, windows and transforms each one, and averages the results.
More segments trade frequency resolution for a cleaner estimate — the one
real knob being turned. The same white-versus-colored contrast, this
time in the time domain rather than frequency, looks like this:

```qu
t = linspace(0, 1, 300)
hiss = randn(300, seed = 1)
lp = butter(3, "low", 8, 300)
rumble = filtfilt(lp, randn(300, seed = 2))

figure()
subplot(2, 1, 1)
plot(t, hiss, color = "#0f172a")
title("White noise (hiss): flat spectrum")
ylabel("amplitude")

subplot(2, 1, 2)
plot(t, rumble, color = "#e11d48")
title("Colored noise (rumble): power concentrated at low frequency")
xlabel("time (s)")
ylabel("amplitude")
```

Both traces look equally "random" by eye. Only their PSDs tell them
apart, which is the entire reason the PSD, not the waveform, is the
object worth computing.

## In Qu

Build 4096 samples of white noise at 2 kHz, then produce a colored
version of the *exact same record* by running it through a lowpass
filter. Nothing about sample-to-sample unpredictability changes; only
where the power lives does. `welch` estimates the PSD of each, and the
two curves plotted together are the measurement a wall and an ear made
for free in the opening paragraph:

```qu
fs = 2000
n  = 4096
white   = randn(n, seed = 10)
lp      = butter(4, "low", 100, fs)
colored = filtfilt(lp, white)

pw = welch(white, fs)
pc = welch(colored, fs)
f  = linspace(0, fs / 2, length(pw))

below = f < 100
above = f >= 100

print("white:   mean below 100 Hz {mean(pw[below]):.2e}, mean above {mean(pw[above]):.2e}")
print("colored: mean below 100 Hz {mean(pc[below]):.2e}, mean above {mean(pc[above]):.2e}")
print("colored power ratio (below/above): {(mean(pc[below]) / mean(pc[above])):.1f}")
print("white   power ratio (below/above): {(mean(pw[below]) / mean(pw[above])):.2f}")

figure()
plot(f, pw, label = "white")
plot(f, pc, label = "colored (lowpass @ 100 Hz)")
axis scale y log
xlabel("frequency (Hz)")
ylabel("PSD (Welch)")
title("Measured PSD: white vs colored noise")
legend()
```

```
white:   mean below 100 Hz 9.94e-4, mean above 1.01e-3
colored: mean below 100 Hz 8.63e-4, mean above 4.52e-6
colored power ratio (below/above): 190.8
white   power ratio (below/above): 0.99
```

White noise's power below 100 Hz and above it are within a percent of
each other — flat, as promised, up to the estimate's own wobble. The
colored version, the same underlying random draw only lowpass-filtered,
has 190 times more power below 100 Hz than above it. Nothing about the
noise's unpredictability changed; everything about where its energy sits
did, and the plotted curves make the 190:1 gap visible at a glance where
the printed ratio only states it.

Look again at that white-noise ratio: 0.99, not 1.00. The true PSD of
white noise is exactly flat with no wiggle room in the theory, and yet
the measured curve is not quite. That is not a flaw in `welch` — running
more segments would shrink the gap further but never erase it, because
every finite estimate of a random process's statistics carries its own
uncertainty. You have just been handed a PSD and asked, implicitly, to
trust it. The next lesson asks the harder question directly: given a
measurement corrupted by exactly this kind of noise, how do you tell
whether something you care about is really there, and how much does an
estimate's reliability actually depend on how much data you were willing
to collect?

**Next: [Lesson 11 — Detection & Estimation](11-detection-estimation.md).**
