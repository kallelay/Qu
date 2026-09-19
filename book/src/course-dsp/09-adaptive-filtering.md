# Adaptive filtering

An echo on a video call is disorienting for exactly as long as it takes
the software to notice and cancel it — and then it is gone, cleanly, for
the rest of the call. Nobody measured the room in advance or typed in a
delay. The software built a filter for that specific, unmeasured room
live, while the call was happening.

## The idea

Every filter in Lessons 7 and 8 needed a target decided in advance: a
cutoff, a stopband, coefficients computed once and frozen. An **adaptive
filter** instead keeps a small set of weights, \(w\), and on every new
sample does three things: it guesses, it checks how wrong the guess was,
and it nudges every weight a little in the direction that would have
made the guess less wrong. Nothing about the underlying system is known
ahead of time — only the guess and the error.

Call the filter's guess \(y[n] = w \cdot x[n]\) (the weights dotted with
the current window of input) and \(d[n]\) the signal actually observed.
The error is \(e[n] = d[n] - y[n]\). Widrow and Hoff, in 1960, showed
that nudging each weight by an amount proportional to \(e[n]\) times the
input sample it multiplies,

\[
w[n+1] = w[n] + \mu\, e[n]\, x[n],
\]

is a stochastic gradient-descent step on the squared error \(e[n]^2\),
without ever computing a derivative symbolically — \(e[n]x[n]\) is, up
to sign, the direction that shrinks \(e[n]^2\) fastest, evaluated fresh
at every sample instead of over a whole batch. This is the **Least Mean
Squares** algorithm (LMS), one of the most-deployed algorithms in the
history of signal processing, and it fits in one line. \(\mu\) is the
step size: too large and the weights overshoot and oscillate, too small
and learning crawls. The formal guarantee is a boundary, not a
recommendation: LMS converges in the mean as long as
\(0 < \mu < 2 / (N \cdot E[x^2])\), with \(N\) the number of weights —
push \(\mu\) past it for a given input power and the same update that
converges cleanly instead grows without bound, the same failure Lesson
8 built on purpose with a misplaced pole.

Strip the problem to one hidden number to see the arithmetic in full: a
single weight trying to discover a fixed gain from nothing but
input/output pairs, updated one sample at a time.

```qu
mu = 0.1
h  = 0.5                      # the hidden gain the filter has to discover
n_iters = 50
w  = 0
ws = zeros(n_iters)
for n = 0 to n_iters - 1
    e = h - w                 # error, since x[n] = 1 for every n here
    w = w + mu * e
    ws[n] = w
end for
figure()
plot(ws)
xlabel("iteration")
ylabel("w")
title("LMS converging to a hidden gain")
print("w after 5:  {ws[4]:.4f}")
print("w after 20: {ws[19]:.4f}")
print("w after 50: {ws[49]:.4f}")
```

```
w after 5:  0.2048
w after 20: 0.4392
w after 50: 0.4974
```

`w` closes in on 0.5 monotonically, never overshooting, because for this
constant input the update reduces to a stable linear recurrence with
fixed point 0.5. Nobody told the filter the answer. It found it by being
wrong in a measurable direction, repeatedly, and moving a fixed fraction
of the way toward less wrong each time.

## In Qu

**A plain check, freshly run.** Qu's standard library has no `lms`,
`rls`, `nlms`, or adaptive-filter builtin of any kind — probing the
interpreter directly (`lms()`, `rls()`, `adaptive_filter()`, `nlms()`)
returns `unknown function` for every one of them, and [Signal Processing
& Filters](../stdlib/signal-processing.md) confirms it from the source.
Qu's related estimation family — `kalman_init` and friends — tracks a
*state* through noisy observations, a related but different problem from
adjusting a *filter's own coefficients* against an error signal. There
is no shortcut: the rest of this lesson is the classic algorithm, built
from nothing but a loop and a weight vector.

A single gain is the simplest possible unknown system. A real echo path
is a short FIR filter — several delayed, scaled copies of what went out
— so identifying it needs a weight *vector*, one entry per tap, updated
by the exact same rule with `x[n]` now a sliding window of input. Drive
it with white noise, because a flat spectrum excites every tap equally,
and run it long enough to actually converge:

```qu
seed(3)
h_true = [0.5, 0.3, -0.2]      # the hidden 3-tap system to identify
mu = 0.05
n_samples = 400
x  = randn(n_samples)
w  = [0, 0, 0]
hist = [0, 0, 0]
w0 = zeros(n_samples)
w1 = zeros(n_samples)
w2 = zeros(n_samples)
err = zeros(n_samples)

for n = 0 to n_samples - 1
    hist = [x[n], hist[0], hist[1]]     # slide the new sample into the window
    d = h_true[0] * hist[0] + h_true[1] * hist[1] + h_true[2] * hist[2]
    y = w[0] * hist[0] + w[1] * hist[1] + w[2] * hist[2]
    e = d - y
    w = w + mu * e * hist
    w0[n] = w[0]
    w1[n] = w[1]
    w2[n] = w[2]
    err[n] = e ^ 2
end for

figure()
plot(w0, label = "w0 -> 0.5")
plot(w1, label = "w1 -> 0.3")
plot(w2, label = "w2 -> -0.2")
xlabel("sample n")
ylabel("weight")
legend()
title("LMS identifying a 3-tap system")
print("final weights: [{w[0]:.4f}, {w[1]:.4f}, {w[2]:.4f}]")
```

```
final weights: [0.5000, 0.3000, -0.2000]
```

All three weights land on `h_true` to four decimal places, each along
its own path — the same update, run in three dimensions instead of one.
The error driving that convergence is the other half of the picture:

```qu
figure()
plot(err)
xlabel("sample n")
ylabel("squared error")
title("LMS squared error decay")
print("mean squared error, first 20 samples: {mean(err[0:20]):.4f}")
print("mean squared error, last 20 samples:  {mean(err[380:400]):.4f}")
```

```
mean squared error, first 20 samples: 0.1391
mean squared error, last 20 samples:  0.0000
```

The squared error falls from an average of 0.14 over the first 20
samples to indistinguishable from zero over the last 20 — the same
descent the single-weight example showed, just no longer visible as a
single clean curve because three weights are moving through a
three-dimensional error surface at once.

## What this buys you, and what it does not

An adaptive filter never needs a person to say "the interference is at
50 Hz" or "the echo is 40 milliseconds long." It needs an error signal
and a step size, and it chases whatever regularity connects its input to
that error. What it does not do is know anything about *why* the
regularity is there, or trust one measurement more than another — every
sample counts equally, a strength when the world is stationary and a
weakness the moment it is not. That weakness — no notion of confidence,
no way to say "this measurement is noisier than that one" — is precisely
where the next module starts. `x[n]` and `d[n]` have been informal
stand-ins for "noise" throughout this lesson; describing noise precisely
enough to say how well any filter performs against it needs a
mathematical description. Lesson 10 gives it one.
