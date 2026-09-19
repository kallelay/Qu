# 12. Wiener & Kalman Filters

A ride-hailing app's car icon jitters sideways and occasionally leaps
half a block, and nobody watching believes the car teleported: a small
mental model of "moving roughly this fast in roughly this direction" is
already running, and each noisy new fix nudges that belief rather than
replacing it. Predict, then correct — the entire idea behind this lesson,
before a single equation appears.

## The idea

"Optimal filter" means minimizing mean-squared error given what is known
about the signal and the noise, and there are two different ways to cash
that out depending on when the answer is needed.

**The Wiener filter** assumes the whole signal's statistics are already
in hand — its power spectral density \(S_{xx}(f)\) and the noise's
\(S_{nn}(f)\), both estimated ahead of time exactly as in Lesson 10. From
those it builds one fixed frequency-domain filter,

\[
H(f) = \frac{S_{xx}(f)}{S_{xx}(f) + S_{nn}(f)},
\]

which passes frequencies where the signal dominates and attenuates
frequencies where the noise does. Where `S_xx` swamps `S_nn`, `H(f)`
sits near 1; where the noise swamps the signal, `H(f)` collapses toward
0. Norbert Wiener published this result in 1949, as *Extrapolation,
Interpolation, and Smoothing of Stationary Time Series*, developed
during wartime anti-aircraft fire-control work. It is optimal, but it is
a *batch* answer, built once from statistics assumed not to change and
applied to data already sitting still:

```qu
f = linspace(0.1, 50, 300)
sxx = 1 / (f .^ 1.3)
snn = 0.002 * ones(length(f))
h = sxx / (sxx + snn)

figure()
subplot(2, 1, 1)
plot(f, sxx, label = "S_xx (signal)", color = "#0f172a")
plot(f, snn, label = "S_nn (noise)", color = "#94a3b8")
axis scale y log
ylabel("power")
title("Wiener filter: known signal and noise spectra")
legend()

subplot(2, 1, 2)
plot(f, h, color = "#e11d48")
xlabel("frequency (Hz)")
ylabel("H(f)")
title("Resulting filter: 1 where signal dominates, 0 where noise does")
```

**The Kalman filter** answers the same question for a signal arriving one
sample at a time from a process that will not hold still. Rudolf Kálmán
published the recursive alternative in 1960, as *A New Approach to Linear
Filtering and Prediction Problems*. Instead of one filter built from a
whole signal's statistics, it carries a belief — a state estimate and its
uncertainty — forward through exactly two repeated steps:

\[
\text{predict:}\quad x' = Fx,\ \ P' = FPF^\top + Q
\]
\[
\text{update:}\quad K = P'H^\top(HP'H^\top + R)^{-1},\ \ x = x' + K(z - Hx'),\ \ P = (I-KH)P'
\]

`F` says how the state evolves on its own, `Q` is how much that model is
distrusted, `H` relates a measurement to the state, `R` is how much the
measurement is distrusted, and `K` — recomputed every step — is the
automatic answer to how much weight a new reading should get against
prior belief. The predict/update cycle is a Bayesian fusion of two
Gaussians: a wide prior belief, narrowed by a measurement, into a
narrower posterior that trusts neither source completely.

```qu
x = linspace(-6, 10, 400)
prior = exp(-((x - 2) .^ 2) / (2 * 2.5 ^ 2)) / (2.5 * sqrt(2 * pi))
meas  = exp(-((x - 5) .^ 2) / (2 * 2.0 ^ 2)) / (2.0 * sqrt(2 * pi))
post  = exp(-((x - 3.83) .^ 2) / (2 * 1.56 ^ 2)) / (1.56 * sqrt(2 * pi))

figure()
plot(x, prior, label = "prior (predict)", color = "#94a3b8")
plot(x, meas, label = "measurement", color = "#0f172a")
plot(x, post, label = "posterior (update)", color = "#e11d48")
xlabel("state value")
ylabel("probability density")
title("Predict, then correct: fusing belief with a new reading")
legend()
```

The posterior sits between prior and measurement, narrower than either —
exactly the "nudge the belief, don't replace it" behavior a GPS dot
displays instinctively. Stanley Schmidt at NASA Ames adapted this
recursion for the Apollo guidance computer within a few years of Kálmán's
paper, and a descendant of it has been standard equipment for spacecraft,
and later for a phone's own GPS chip, ever since.

## In Qu

Qu has no `wiener` builtin — the useful version of this idea for live,
one-sample-at-a-time data is the Kalman filter, via `kalman_init` and its
`.predict` / `.update` / `.estimate` methods. The example tracks a
radiosonde climbing at a roughly constant rate through a noisy altimeter,
estimating *both* altitude and climb rate — the second of which is never
measured directly at all:

```qu
n = 80
climb_rate = 2.0
meas_std = 5.0

truth = zeros(n)
meas  = zeros(n)
for t = 0 to n - 1
    truth[t] = climb_rate * t
    meas[t]  = truth[t] + meas_std * randn(seed = 100 + t)
end for

F = [1, 0, 1, 1] as matrix(2, 2)     # rows: [1,1] (position update), [0,1] (velocity carries over)
Q = [0.001, 0, 0, 0.001] as matrix(2, 2)
H = [1, 0] as matrix(1, 2)
R = [meas_std ^ 2] as matrix(1, 1)

s = kalman_init([0, 0], [100, 0, 0, 100] as matrix(2, 2))
est = zeros(n)
est_rate = zeros(n)
for t = 0 to n - 1
    s = s.predict(F, Q)
    s = s.update(H, [meas[t]], R)
    est[t] = s.estimate()[0]
    est_rate[t] = s.estimate()[1]
end for

print("raw measurement RMSE:  {rmse(truth, meas):.3f} m")
print("Kalman estimate RMSE:  {rmse(truth, est):.3f} m")
print("true climb rate:       {climb_rate:.3f} m/s")
print("estimated climb rate (last): {est_rate[n-1]:.3f} m/s")

t_axis = 0 to n - 1
figure()
scatter(t_axis, meas, color = "#94a3b8", label = "measured")
plot(t_axis, truth, color = "#0f172a", label = "truth")
plot(t_axis, est, color = "#e11d48", width = 2, label = "Kalman estimate")
xlabel("time (s)")
ylabel("altitude (m)")
title("Tracking a climbing balloon: truth vs measured vs Kalman")
legend()

figure()
plot(t_axis, climb_rate * ones(n), color = "#0f172a", label = "true climb rate")
plot(t_axis, est_rate, color = "#e11d48", label = "estimated climb rate")
xlabel("time (s)")
ylabel("climb rate (m/s)")
title("Climb rate inferred from position-only measurements")
legend()
```

```
raw measurement RMSE:  4.499 m
Kalman estimate RMSE:  1.580 m
true climb rate:       2.000 m/s
estimated climb rate (last): 1.944 m/s
```

The filter nearly triples the accuracy of the raw altimeter — 4.5 meters
of error down to 1.6 — using nothing but the same noisy readings and a
two-line model of how a climbing balloon behaves. The first plot shows
why: the red estimate tracks the black truth line far more closely than
the scattered gray measurements do. The second plot is the more
surprising result — `H = [1, 0]` only ever looks at position, yet
`est_rate` converges to 1.944 m/s against a true 2.000, inferred purely
from how the position estimate has been changing over time, at the same
kind of precision NASA needed to land on a body with no runway lights.

Two settings did the real work. `Q`, the process noise, is small (0.001)
because a climbing balloon really does keep a steady rate from one second
to the next, so the filter is told to mostly trust its own motion model.
`R`, at `meas_std² = 25`, is comparatively large, so each individual
altimeter reading gets modest weight against that trusted model. Swap the
two — a jittery process tracked by a precise sensor — and the same two
equations shift their trust the other way with no code change; `K` is
doing that arithmetic fresh every step. Nothing here required `F` or `H`
to be matrices in principle — Qu's `kalman_init` state also accepts a
named nonlinear function in their place, switching to an Extended or
Unscented Kalman filter for problems like radar range, which is not a
linear function of position at all.

Every number in `F`, `H`, `Q`, and `R` above was linear, fixed, and
correct — position really does update by adding velocity times one
second, every single sample. Real channels are rarely so obliging: a
signal traveling from a transmitter to a receiver picks up delay,
distortion, and interference that no constant matrix captures, and before
any filter in this module can track or detect anything, the signal has
to survive that trip. This module assumed the signal was already in
hand. The next one asks how it got there.

**Next: [Lesson 13 — Modulation & Demodulation](13-modulation-demodulation.md).**
