# 11. Detection & Estimation

A phone's ringtone cuts through café noise not because it is louder than
the espresso machine, but because the ear already has a template for it
and is sliding that template forward in time, looking for where it lines
up. Made precise, that comparison is a matched filter — provably the best
possible way to find a known shape buried in noise, not merely a
convenient one.

## The idea

**Detection** and **estimation** are taught together because they share
machinery, but they answer different questions. Detection asks a yes/no
question — is a signal of known shape present in this data at all?
Estimation asks a quantitative one — given that it is present, what is
its amplitude, its timing, its mean? A radar either sees a target or it
does not; only once it does does estimation say how far away it is.

**Detection as a threshold on a random variable.** Whatever statistic a
detector computes, it behaves as one distribution when only noise is
present (`H0`) and a shifted distribution when a signal is really there
(`H1`). Detection is choosing a threshold on that statistic, and no
threshold eliminates both kinds of mistake at once: set it low and every
real pulse is caught, at the cost of flagging noise spikes as targets — a
false alarm; set it high and false alarms vanish, but a genuine, weak
pulse now fails to cross the line — a missed detection. Jerzy Neyman and
Egon Pearson formalized the trade-off in 1933: for a fixed, acceptable
false-alarm rate, there is a most powerful test that maximizes detection
probability, and it works by comparing a likelihood ratio to exactly one
threshold.

```qu
x = linspace(-4, 8, 400)
h0 = exp(-(x .^ 2) / 2) / sqrt(2 * pi)
h1 = exp(-((x - 3) .^ 2) / 2) / sqrt(2 * pi)

figure()
plot(x, h0, label = "H0: noise only")
plot(x, h1, label = "H1: signal + noise")
plot([1.5, 1.5], [0, 0.42], color = "#111827", label = "decision threshold")
xlabel("matched-filter output")
ylabel("probability density")
title("Detection as a threshold choice")
legend()
```

Where the two curves overlap, no threshold is free: moving the line left
trades fewer missed detections for more false alarms, and moving it
right does the reverse. The overlap itself — not the threshold — is what
a matched filter minimizes, by making `H1`'s distribution sit as far from
`H0`'s as the noise allows.

**The matched filter.** Comparing two signals for similarity by sliding
one along the other is cross-correlation,
\(R_{xy}[k] = \sum_n x[n]\,y[n-k]\). A matched filter is exactly this:
correlate the noisy data against a copy of the pulse being sought, and
read off where the correlation peaks. D. O. North proved, in a 1943 RCA
report not widely published until a 1963 IEEE reprint, that when the
noise is white and Gaussian, no other linear operation extracts a known
pulse from it at a higher output signal-to-noise ratio.

**Estimation and the cost of precision.** Given `N` noisy samples of a
constant true value, the sample mean \(\hat{x} = \frac{1}{N}\sum_n x[n]\)
is, for Gaussian noise, both the maximum-likelihood estimate and the
minimum-variance estimate among every unbiased alternative (Gauss–Markov
theorem). Its own uncertainty, the standard error, is

\[
\sigma_{\hat{x}} = \frac{\sigma}{\sqrt{N}}.
\]

Halving the uncertainty costs four times the data, not two — the point
people most often get wrong about collecting more measurements.

## In Qu

The pulse below is a Gaussian shape, height 1, hidden inside 2000 samples
of noise with standard deviation 0.7. The raw trace's largest noise
excursion is already more than double the pulse's own height, so no
threshold on the raw signal finds it without also flagging noise:

```qu
n = 2000
k = -20 to 20
pulse = exp(-(k .^ 2) / (2 * 5 ^ 2))
plen = length(pulse)

true_start = 1200
x = zeros(n)
x[true_start:true_start + plen - 1] = pulse
noisy = x + 0.7 * randn(n, seed = 12)

corr = xcorr(noisy, pulse)
peak_k = argmax(corr)
detected_start = peak_k - (plen - 1)
peak_val = max(corr)

far = corr[0:800]
print("peak correlation value:        {peak_val:.2f}")
print("typical correlation elsewhere: {std(far):.2f} (std), {max(abs(far)):.2f} (max |.|)")
print("true pulse start index:        {true_start}")
print("matched-filter start index:    {detected_start}")
print("max |noisy| (raw trace):       {max(abs(noisy)):.2f}   (pulse peak was 1.00)")

lags = (0 to length(corr) - 1) - (plen - 1)

figure()
subplot(2, 1, 1)
plot(0 to n - 1, noisy, color = "#94a3b8")
title("Raw noisy trace (pulse hidden near sample 1200)")
ylabel("amplitude")

subplot(2, 1, 2)
plot(lags, corr, color = "#e11d48")
title("Matched-filter output (cross-correlation with the pulse)")
xlabel("lag (samples)")
ylabel("correlation")
```

```
peak correlation value:        9.52
typical correlation elsewhere: 2.06 (std), 5.63 (max |.|)
true pulse start index:        1200
matched-filter start index:    1201
max |noisy| (raw trace):       2.33   (pulse peak was 1.00)
```

The raw trace peaks at 2.33 — more than double the pulse's own height —
purely from noise. The matched filter's correlation peaks at 9.52, well
clear of the 5.63 reached anywhere else in the record, and lands the
pulse's start within a single sample of where it actually began. Any
threshold between 5.63 and 9.52 catches this pulse in this record with
zero false alarms: the Neyman–Pearson trade-off made concrete, in one
number, on one record.

Now the estimation half: how much does more data actually buy you? Take
`N` noisy samples of a sensor reading a steady 5.0 units with 2.0 units
of noise, repeat the sample-mean estimate over 200 trials at each of four
sample counts, and compare the measured RMS error against the predicted
`σ/√N`:

```qu
true_amp = 5.0
noise_std = 2.0
n_trials = 200

ns = [10, 100, 1000, 10000]
measured = zeros(4)
predicted = zeros(4)
for i = 0 to 3
    n = ns[i]
    errs = zeros(n_trials)
    for trial = 0 to n_trials - 1
        samples = true_amp + noise_std * randn(n, seed = 1000 * i + trial)
        errs[trial] = mean(samples) - true_amp
    end for
    measured[i] = sqrt(mean(errs .^ 2))
    predicted[i] = noise_std / sqrt(n)
    print("n={n}: rms error over {n_trials} trials = {measured[i]:.4f}   predicted std-error = {predicted[i]:.4f}")
end for

figure()
plot(ns, measured, "o", label = "measured RMS error", color = "#e11d48")
plot(ns, predicted, label = "predicted sigma / sqrt(N)", color = "#0f172a")
axis scale x log
axis scale y log
xlabel("N (samples per estimate)")
ylabel("error")
title("Estimator error shrinks as 1/sqrt(N)")
legend()
```

```
n=10: rms error over 200 trials = 0.6508   predicted std-error = 0.6325
n=100: rms error over 200 trials = 0.2112   predicted std-error = 0.2000
n=1000: rms error over 200 trials = 0.0577   predicted std-error = 0.0632
n=10000: rms error over 200 trials = 0.0196   predicted std-error = 0.0200
```

Across four orders of magnitude in sample count, the measured markers sit
almost exactly on the predicted line — ten times the data buys roughly
three times the precision, every time, because \(\sqrt{10} \approx 3.16\)
and the simulation has nowhere to hide from that arithmetic.

Both results assumed something the café analogy quietly supplied for
free: that the noise was flat across frequency (Lesson 10's white noise),
so a matched filter weighting every frequency equally was already the
right thing to do. Facing colored noise instead, the optimal move is to
*whiten* it first — filter both the data and the template by the inverse
of the noise's own spectral shape — before correlating; the correlation
step does not change, only what is handed to it does. Neither tool here
does anything with a measurement that arrives one sample at a time from a
quantity that is itself drifting, where yesterday's estimate should
inform today's rather than being thrown out and recomputed from scratch.
That recursive kind of estimator is where the next lesson goes.

**Next: [Lesson 12 — Wiener & Kalman Filters](12-wiener-kalman-filters.md).**
