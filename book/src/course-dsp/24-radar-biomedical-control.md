# Lesson 24 — Radar, Biomedical, and Control

Three machines that have nothing in common on the outside — a weather
radar, a bedside vital-signs monitor, a thermostat — are running the same
handful of ideas from this course underneath. A pulse goes out and its
echo comes back changed; a small electrical probe reveals what is inside
tissue without cutting it open; a measurement is compared to a target and
the difference is fed back in. This lesson is three worked closes of the
loop this course opened, not three new subjects.

## The idea

**Ranging by matched filtering.** A radar (or sonar, or ultrasound
system) sends a known waveform \(x[n]\) and listens for a delayed, noisy,
attenuated copy of it. The **matched filter** — the filter proven to
maximize output signal-to-noise ratio for a known pulse shape in white
noise — is just the pulse itself, time-reversed: \(h[n] = x^*[-n]\).
Convolving the received signal with \(h\) is mathematically identical to
correlating it with the original pulse, so cross-correlation *is* the
matched filter in practice. Wherever the correlation peaks, that is where
the echo sits, and the delay \(\tau\) between transmission and that peak
converts directly to distance:

\[
R = \frac{c\,\tau}{2}
\]

the factor of two because the wave travels to the target and back. The
same correlation-peak trick is sonar's, ultrasound's, and GPS's: whichever
physical wave is used, "how long until the known shape comes back" is one
question with one answer.

**Bioimpedance.** Pass a small alternating current through tissue and
measure the resulting voltage; their ratio is the complex impedance
\(Z(j\omega) = V(j\omega)/I(j\omega)\), a function of frequency because
cell membranes behave capacitively. A simple, widely used circuit model —
a solution resistance \(R_s\) in series with a charge-transfer resistance
\(R_{ct}\) in parallel with a double-layer capacitance \(C_{dl}\) — already
reproduces the classic shape:

\[
Z(j\omega) = R_s + \frac{R_{ct}}{1 + j\omega R_{ct} C_{dl}}
\]

Plotted as \(-\mathrm{Im}(Z)\) against \(\mathrm{Re}(Z)\) (a **Nyquist
plot**) this traces a semicircle; plotted as magnitude and phase against
log-frequency (a **Bode plot**) it shows two flat plateaus joined by a
roll-off. Both are the same measurement, read two ways — this is
electrochemical impedance spectroscopy, used from battery diagnostics to
tissue characterization.

**Feedback control.** A system left alone drifts wherever its own
dynamics take it; a **controller** compares the actual output to a
target **setpoint** and corrects the input. The workhorse is the discrete
**PID controller**, which reacts to the present error, its accumulated
history, and its rate of change:

\[
u[k] = K_p\,e[k] \;+\; K_i \sum_{i=0}^{k} e[i]\,\Delta t \;+\; K_d\,\frac{e[k]-e[k-1]}{\Delta t}
\]

The proportional term \(K_p\) reacts now, the integral term \(K_i\)
eliminates the steady offset a proportional-only controller leaves
behind, and the derivative term \(K_d\) resists overshoot. Qu has no
built-in `pid`/`controller` function as of this writing — the loop below
is the whole algorithm, in about eight lines.

```qu
pulse_g = ones(20)
lags = (0 to 2 * length(pulse_g) - 2) - (length(pulse_g) - 1)
ac = xcorr(pulse_g)

figure()
panel(2, 1, 1)
plot(0 to length(pulse_g) - 1, pulse_g)
xlabel("sample index")
ylabel("amplitude")
title("A rectangular pulse")
panel(2, 1, 2)
plot(lags, ac)
xlabel("lag (samples)")
ylabel("autocorrelation")
title("Matched filter output: a sharp peak at zero lag")
```

A pulse correlated with itself peaks exactly at zero lag — the whole
ranging idea in miniature, before any noise or delay is added.

```qu
dt_g = 0.05
n_g = 100
tau_g = 1.0
K_true = 1.5
K_assumed = 1.0
setpoint_g = 1.0

u_ol = setpoint_g / K_assumed
y_ol = zeros(n_g)
py = 0
for k in 0 to n_g - 1
    py = py + dt_g * (K_true * u_ol - py) / tau_g
    y_ol[k] = py
end for

Kp_g = 2.0
y_p = zeros(n_g)
py2 = 0
for k in 0 to n_g - 1
    e = setpoint_g - py2
    u = Kp_g * e
    py2 = py2 + dt_g * (K_true * u - py2) / tau_g
    y_p[k] = py2
end for

tg = (0 to n_g - 1) * dt_g
figure()
plot(tg, ones(n_g) * setpoint_g, label="setpoint")
plot(tg, y_ol, label="open loop (wrong assumed gain)")
plot(tg, y_p, label="proportional feedback")
xlabel("time (s)")
ylabel("output")
title("Feedback corrects for an unknown gain, but P-only control leaves an offset")
legend()

print("open loop final value: {round(y_ol[n_g-1], 4)}")
print("P-only final value: {round(y_p[n_g-1], 4)}")
```

```
open loop final value: 1.4911
P-only final value: 0.75
```

An open-loop command computed from the wrong assumed gain settles 49%
high, because nothing ever tells it it's wrong. Feedback halves that
error to 25% below target — better, but not fixed, because a
proportional term is only ever reacting to whatever error remains right
now. It never asks "how long has this been off," which is exactly the
integral term's job below.

## In Qu

### Radar ranging

A pulse-compression radar: a 40-sample chirp-like pulse is transmitted,
a delayed and heavily noised copy is "received," and the matched filter
(cross-correlation against the known pulse, padded to the receiver's
length so the lag axis centers cleanly) finds it back.

```qu
seed(4)
fs_r = 1e6
c_r = 3e8
pulse_len = 40
pulse_r = sin(2 * pi * (0 to pulse_len - 1) * 0.15)

true_delay = 220
Nr = 800
tx_echo = zeros(Nr)
tx_echo[true_delay:true_delay + pulse_len - 1] = 0.4 * pulse_r
received = tx_echo + 0.5 * randn(Nr, seed = 99)

template_r = zeros(Nr)
template_r[0:pulse_len - 1] = pulse_r

xc = xcorr(received, template_r)
lag_axis = (0 to length(xc) - 1) - (Nr - 1)
peak_i = argmax(xc)
est_delay = lag_axis[peak_i]
tof = est_delay / fs_r
range_m = c_r * tof / 2

print("true delay (samples): {true_delay}")
print("estimated delay (samples): {est_delay}")
print("time of flight: {tof} s")
print("estimated range: {round(range_m / 1000, 3)} km")

figure()
plot(0 to Nr - 1, received)
xlabel("sample index")
ylabel("amplitude")
title("Received radar signal: echo buried in noise")

figure()
plot(lag_axis, xc)
vline(true_delay, color="gray")
xlabel("lag (samples)")
ylabel("cross-correlation")
title("Matched filter output: peak locates the echo")
```

```
true delay (samples): 220
estimated delay (samples): 220
time of flight: 0.00022 s
estimated range: 33 km
```

The echo is invisible in the received-signal plot — amplitude 0.4 against
noise of standard deviation 0.5 is buried below the eye's threshold. The
correlation peak finds it exactly: 220 samples recovered from 220 samples
true, with zero error, because a matched filter's whole purpose is
gathering energy that is individually below the noise floor into one
unmistakable spike.

### Biomedical impedance

The Randles-cell-like model from Part 1, with plausible tissue-scale
component values, run through Qu's real EIS plotting functions:

```qu
Rs = 20
Rct = 300
Cdl = 20e-6
freqs = logspace(0, 5, 60)
w = 2 * pi * freqs
Zc = 1 / (1j * w * Cdl)
Zpar = (Rct * Zc) / (Rct + Zc)
Z = Rs + Zpar

figure()
nyquist(Z, label="tissue model")
title("Bioimpedance Nyquist plot")

figure()
panel(2, 1, 1)
bode_magnitude(Z, freqs)
title("Bode magnitude")
panel(2, 1, 2)
bode_phase(Z, freqs)
title("Bode phase")

print("|Z| at 10 Hz: {round(abs(Z[argmin(abs(freqs - 10))]), 2)} ohm")
print("|Z| at 100 kHz: {round(abs(Z[length(Z)-1]), 2)} ohm")
```

```
|Z| at 10 Hz: 298.02 ohm
|Z| at 100 kHz: 20 ohm
```

At high frequency the capacitor is effectively a short circuit and only
\(R_s = 20\ \Omega\) remains — read directly off the low-frequency and
high-frequency intercepts of the Nyquist semicircle, or the two flat
plateaus of the Bode magnitude plot. At 10 Hz the capacitor is nearly
open and the full \(R_s + R_{ct} \approx 320\ \Omega\) shows through,
measured here at 298 because 10 Hz is not yet fully into that plateau.
This is the entire diagnostic content of impedance spectroscopy: the
shape of the curve, not any single number, tells you which circuit
element dominates at which frequency.

### Control loop

The full PID controller from Part 1's equation, stabilizing a simulated
first-order plant against a unit step setpoint:

```qu
dt = 0.05
n_ticks = 200
tau = 1.2
K = 2.0
setpoint = 1.0
Kp = 2.0
Ki = 1.5
Kd = 0.1

y = zeros(n_ticks)
e_prev = 0
integral = 0
plant_y = 0
for tick in 0 to n_ticks - 1
    err = setpoint - plant_y
    integral = integral + err * dt
    deriv = (err - e_prev) / dt
    u_pid = Kp * err + Ki * integral + Kd * deriv
    plant_y = plant_y + dt * (K * u_pid - plant_y) / tau
    y[tick] = plant_y
    e_prev = err
end for

t_axis = (0 to n_ticks - 1) * dt
print("final value: {round(y[n_ticks - 1], 4)}")
print("value at t=1s: {round(y[argmin(abs(t_axis - 1))], 4)}")
overshoot = (max(y) - setpoint) / setpoint * 100
print("overshoot: {round(overshoot, 2)}%")

figure()
plot(t_axis, ones(n_ticks) * setpoint, label="setpoint")
plot(t_axis, y, label="plant response")
xlabel("time (s)")
ylabel("output")
title("Discrete PID control of a first-order plant")
legend()
```

```
final value: 1
value at t=1s: 0.951
overshoot: 0%
```

The integral term that Part 1's proportional-only loop was missing earns
its keep: the plant reaches the setpoint exactly, with no lingering 25%
offset, 95% of the way there within a single second, and no overshoot at
all with this particular gain choice — a controller tuned to correct
`Ki`'s tendency to ring rather than one fighting it.

## Coming back to the phone call

Lesson 0 opened with a voice on a phone that was never really
transmitted — a codec's guess, rebuilt at the other end from about ten
numbers refreshed fifty times a second. You now have every piece needed
to read that sentence as engineering rather than as a trick. The
"ten numbers" are linear-prediction coefficients, a short filter (Lessons
2 and 8) fitted anew to each roughly-20-millisecond frame of speech,
because a vocal tract is close enough to an LTI system for that filter to
predict its resonances well. "Refreshed fifty times a second" is a real-
time deadline of about 20 milliseconds per block (Lesson 23) — miss it and
the call glitches, which is exactly the soft-real-time failure this course
just measured with its own `timer()`. The coefficients and the residual
excitation signal that drives them are quantized to a handful of bits
before they are radioed or packetized (Lesson 22) — this is, in outline,
Code-Excited Linear Prediction, CELP, the family of algorithms behind
GSM and most of twentieth-century digital telephony, and every term in
its name is now a term you have implemented, not just heard.

The wrist-pulse example from Lesson 0 resolves the same way. Extracting a
breathing rate from heart-rate variability is a Fourier transform
(Lesson 4) of an unevenly sampled point process, which is a detection and
estimation problem (Lesson 11) usually cleaned up with exactly the kind
of recursive filter Lesson 12 built for tracking a noisy state over time.
Measure the pulse itself with a bioelectrical sensor instead of a
fingertip, and the front end is the impedance spectroscopy this lesson
just ran — which happens to be the corner of this exact field the course's
own author works in for a living, log-swept from 1 Hz to 100 kHz on real
instruments rather than on 60 simulated points. Nothing in either example
was ever beyond you. It was beyond you before Lesson 1, which is the only
sense in which any of this course was ever hard.

What this course could not do is finish DSP, because DSP is not finished.
Every technique here still has an open edge nearby: adaptive filters
(Lesson 9) that must run on a sensor with no battery to spare, compressed
sensing (Lesson 20) still finding new domains where "fewer samples than
Nyquist allows" turns out to be true, machine learning for signals
(Lesson 21) replacing hand-designed features with learned ones in ways
this course's own logistic-regression example only gestured at. Some of
those edges are being worked on by people no more expert than you were
twenty-four lessons ago. The mathematics in this course took Fourier
until 1822 and Cooley and Tukey until 1965; it is not owed to finish
arriving during your career, which means there is real work left in it,
not just problems already solved and waiting to be assigned to you.

Pick a signal you actually have — a sensor on a hobby project, a
dataset from your own field, your own pulse — and ask it the question
this course kept asking: what does this contain, and how do I see it.
You now have a working, verifiable answer machine for that question, not
a set of memorized worked examples. Go point it at something nobody has
already solved for you.
