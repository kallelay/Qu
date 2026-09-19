# 18. Array & Spatial Processing

A line of sensors turns tiny arrival-time differences into a direction estimate — timing, not loudness, is the whole trick.

## The idea

**Geometry as a delay.** Line up \(M\) sensors — microphones, antenna elements, geophones — at known positions \(d_m\) along a straight line. A wavefront arriving from angle \(\theta\), measured from the array's broadside, reaches each sensor at a slightly different time, because each sensor sits a different distance along the direction the wave travels:

\[
\tau_m = \frac{d_m \sin\theta}{c}
\]

where \(c\) is the wave's propagation speed (343 m/s for sound in air; radio just uses the speed of light). Two sensors half a metre apart and a source 30 degrees off-centre already produce a delay of a few hundred microseconds — too small to hear as an echo, easily large enough to measure.

**Delay-and-sum beamforming** steers a "listening direction" by undoing that geometry: shift each channel by minus its own delay so a source actually arriving from \(\theta\) lines back up in phase, then average:

\[
y(t) = \frac{1}{M}\sum_{m=1}^{M} x_m(t + \tau_m)
\]

A signal from the steered direction adds up coherently: \(M\) aligned copies sum to \(M\) times the amplitude, \(M^2\) times the power. Noise that is independent from sensor to sensor does not know which direction is steered; it adds incoherently, growing only as \(\sqrt{M}\) in amplitude, \(M\) in power. The ratio of those two growth rates is the array's whole payoff: signal-to-noise ratio improves by a factor of \(M\), or \(10\log_{10}M\) dB — the same electronic-steering principle that lets a phased array scan a listening direction without physically rotating an antenna.

```qu
c = 343.0
d_spacing = 0.5
theta_deg = linspace(-90, 90, 181)
theta = theta_deg * pi / 180
tau_micros = 1e6 * d_spacing * sin(theta) / c

plot(theta_deg, tau_micros)
title("Inter-sensor delay vs. arrival angle")
xlabel("angle from broadside (deg)")
ylabel("delay (microseconds)")
```

```qu
seed(10)
Fs = 2000
N = 400
n = 0 to N - 1
t = n / Fs

M = 4
f0 = 100
phase_step = 0.6

unsteered = zeros(N)
steered = zeros(N)
for m = 0 to M - 1
    phi = m * phase_step
    xm = sin(2*pi*f0*t + phi)
    unsteered = unsteered + xm
    steered = steered + sin(2*pi*f0*t)
end for
unsteered = unsteered / M
steered = steered / M

figure()
subplot(2, 1, 1)
plot(t[0:99], unsteered[0:99])
title("Summed without correcting the delay")
ylabel("amplitude")
subplot(2, 1, 2)
plot(t[0:99], steered[0:99])
title("Summed after correcting the delay")
xlabel("time (s)")
ylabel("amplitude")
```

The delay grows with angle exactly as \(\sin\theta\) predicts, and four tones summed with their natural phase offsets left in place partially cancel, while the same four tones summed after correcting for that offset add up to full amplitude — the entire mechanism, before any noise or measurement enters the picture.

## In Qu

A search of every stdlib chapter in this book turns up no `beamform`, no `array_factor`, no `steering_vector` — confirmed directly against the engine as well as the docs, Qu's standard library has nothing built specifically for sensor arrays. What it does have is everything delay-and-sum is made of: `signal()` to attach a sample rate to a vector, and `interpolate_at(sig, t_query, method)` to read that signal back out at arbitrary, including fractional-sample, times. Realigning a channel by a delay that is not a whole number of samples is exactly a fractional resampling problem, so the "missing" beamformer is one call away, sitting inside the interpolation chapter of the signal-processing library.

Simulate five microphones, half a metre of spread, a 1 kHz tone arriving from 30 degrees off broadside, independent noise on every channel, and measure the signal-to-noise ratio a single microphone gets against what the array gets after steering:

```qu
seed(21)
c = 343.0
Fs = 48000
N = 4800
n = 0 to N - 1
t = n / Fs

f0 = 1000
M = 5
d_spacing = 0.08
theta = 30 * pi / 180

mics = zeros(M)
for m in 0 to M - 1
    mics[m] = (m - (M - 1) / 2) * d_spacing
end for
delays = mics * sin(theta) / c

noise_sigma = 0.8
beam = zeros(N)

for m in 0 to M - 1
    tm = t - delays[m]
    xm = sin(2*pi*f0*tm) + noise_sigma * randn(N)
    aligned = interpolate_at(signal(xm, Fs), t + delays[m], "nearest")
    beam = beam + aligned
    if m == 0
        single = xm
        single_clean = sin(2*pi*f0*tm)
    end if
end for
beam = beam / M

clean = sin(2*pi*f0*t)
snr_single_db = 10*log10(energy(single_clean) / energy(single - single_clean))
snr_beam_db = 10*log10(energy(clean) / energy(beam - clean))

print("single sensor SNR: {snr_single_db:.2f} dB")
print("beamformer SNR:    {snr_beam_db:.2f} dB")
print("measured gain:     {snr_beam_db - snr_single_db:.2f} dB")
print("predicted 10*log10(M): {10*log10(M):.2f} dB")

plot(t[0:199], beam[0:199])
title("Steered array output (delay-and-sum)")
xlabel("time (s)")
ylabel("amplitude")
```

```
single sensor SNR: -1.24 dB
beamformer SNR:    6.02 dB
measured gain:     7.25 dB
predicted 10*log10(M): 6.99 dB
```

One microphone sits at -1.24 dB, more noise power than signal power. Five microphones, steered and summed, land at +6.02 dB — a 7.25 dB improvement against a textbook prediction of \(10\log_{10}5 = 6.99\) dB, the array gain measured rather than assumed. Note what this implementation is *not* doing: `interpolate_at` shifts each channel in the time domain by a literal amount of time, so this beamformer works on any waveform, not only the single tone used to keep the SNR measurement clean — a simplified textbook treatment that instead rotates each frequency's phase only matches a true time delay at one frequency at a time.

The measured 0.26 dB above prediction is worth chasing, because swapping one word in the code changes it a lot. Rerun the identical loop with `"linear"` in place of `"nearest"`:

```qu
aligned = interpolate_at(signal(xm, Fs), t + delays[m], "linear")
```

```
beamformer SNR (linear interpolation): 7.34 dB
measured gain (linear):     8.58 dB
```

More than a decibel higher, for a geometry that did not change. `"nearest"` snaps each query time to its closest existing sample — alignment with no averaging. `"linear"` blends the two samples surrounding each query time, weighted by how close the fractional delay sits to each, and blending two independent noise samples together is itself a tiny low-pass filter, quietly denoising each channel before the array sum ever runs. That side effect is real and free, but it is smoothing wearing the same clothes as array gain, which is exactly why `"nearest"` was the honest choice for the measurement above.

## What a delay built from interpolation is quietly asking for

Undoing a fractional-sample delay by evaluating a signal's own interpolant at a shifted time — what `interpolate_at` just did five times over — is a resampling operation wearing a beamforming hat. It worked here because the shifts were small and the method was simple, but "shift a signal by a fraction of a sample, correctly, at whatever quality the job demands" is a problem general enough to deserve its own machinery, with the nearest-versus-linear trade-off made explicit and controllable instead of implicit in a keyword argument. That machinery, built from the same up-and-down rate-conversion primitives, is exactly where Lesson 19, **Multirate DSP and filter banks**, picks up.
