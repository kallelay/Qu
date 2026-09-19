# Analog and digital filters

A wall lets bass through and stops treble almost completely, purely
because a stiff, heavy partition resists moving at high frequencies far
more than at low ones. That is a filter: a system that treats some
frequencies one way and other frequencies another, whether or not anyone
designed it to.

## The idea

A filter can be built two ways.

An **analog** filter is a physical arrangement of components — resistor,
capacitor, inductor, op-amp — through which a continuous voltage flows.
The simplest case, a resistor and capacitor in series with the output
taken across the capacitor, has transfer function

\[
H(j\omega) = \frac{1}{1 + j\omega RC}, \qquad \omega_c = \frac{1}{RC},
\]

where \(\omega_c\) is the cutoff: the frequency at which the output has
fallen to \(1/\sqrt{2}\) of the input, about \(-3\,\mathrm{dB}\). No
clock, no samples — the shaping happens because the physics of charge
and voltage cannot do anything else.

A **digital** filter has no physical continuity to lean on. It sees a
sequence of numbers arriving at a fixed rate and must produce another
sequence using nothing but arithmetic on samples it has already seen:

\[
\sum_{k=0}^{N} a_k\, y[n-k] = \sum_{k=0}^{M} b_k\, x[n-k].
\]

This is the convolution sum with one generalization: the right side may
now depend on the filter's own past output (\(a_k\), \(k \geq 1\)) as
well as the input. Whether that dependency exists at all is the entire
subject of Lesson 8. Both machines chase the identical goal — reshape a
spectrum — and Stephen Butterworth published the "maximally flat"
response in 1930 as an arrangement of coils and capacitors, decades
before a digital version of the same mathematics existed.

**The four shapes.** Almost every filter is one of four kinds, named
for what survives:

- **Low-pass** — keeps frequencies below a cutoff \(f_c\).
- **High-pass** — keeps frequencies above \(f_c\).
- **Band-pass** — keeps a window \([f_1, f_2]\), discards the rest.
- **Band-stop** (notch) — discards \([f_1, f_2]\), keeps the rest.

One question answers all four: at a given frequency, does the gain
\(|H(f)|\) sit near 1 (pass) or near 0 (stop)? A **magnitude response**
is exactly that gain, plotted against frequency:

```qu
fs = 1000
lp = butter(4, "low", 100, fs)
hp = butter(4, "high", 100, fs)
bp = butter(4, "band", [80, 120], fs)
bs = butter(4, "stop", [80, 120], fs)
f  = linspace(0, fs / 2, 512)
figure()
plot(f, mag2db(abs(freqz(lp, 512))), label = "low-pass")
plot(f, mag2db(abs(freqz(hp, 512))), label = "high-pass")
plot(f, mag2db(abs(freqz(bp, 512))), label = "band-pass")
plot(f, mag2db(abs(freqz(bs, 512))), label = "band-stop")
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
ylim(-80, 5)
legend()
title("The four basic filter shapes")
```

Four fourth-order Butterworth designs, same edge frequencies, one plot.
Reading this curve — finding where it crosses \(-3\,\mathrm{dB}\) — is
the one skill this lesson builds toward.

## In Qu

Mains hum is a textbook band-stop problem: interference sitting at a
fixed, known frequency (50 or 60 Hz) riding on top of a real signal.
Build a synthetic case — a 10 Hz signal contaminated with 60 Hz hum and
a little measurement noise — and remove only the hum:

```qu
seed(7)
fs = 1000
N  = 1000
t  = (0 to N - 1) / fs
x  = sin(2 * pi * 10 * t) + 0.6 * sin(2 * pi * 60 * t) + 0.05 * randn(N)

notch = butter(4, "stop", [58, 62], fs)
y     = sosfilt(notch, x)

Xf = abs(rfft(x))
Yf = abs(rfft(y))
f  = (0 to length(Xf) - 1) * fs / N

figure()
plot(f, Xf, label = "before notch")
plot(f, Yf, label = "after notch")
xlim(0, 100)
xlabel("Frequency (Hz)")
ylabel("Magnitude")
legend()
title("60 Hz mains hum, before and after notch filtering")

print("magnitude at 60 Hz before: {Xf[60]:.2f}")
print("magnitude at 60 Hz after:  {Yf[60]:.2f}")
print("attenuation: {mag2db(Yf[60] / Xf[60]):.1f} dB")
```

```
magnitude at 60 Hz before: 299.07
magnitude at 60 Hz after:  0.87
attenuation: -50.8 dB
```

The 60 Hz spike is gone; the 10 Hz component is untouched, because it
sits nowhere near the stopband. `butter(4, "stop", [58, 62], fs)` chose
every coefficient in that difference equation for you — this is Qu's
`kind="stop"` overload of the same call used for all four shapes above.

`-50.8 dB` on real data is not the whole story. Read the filter's own,
noise-free magnitude response and check what it is actually capable of:

```qu
fresp = mag2db(abs(freqz(notch, 512)))
faxis = linspace(0, fs / 2, 512)
figure()
plot(faxis, fresp)
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
title("Notch filter magnitude response")

idx60 = where(abs(faxis - 60) < 0.5)
print("notch depth at 60 Hz: {fresp[idx60[0]]:.1f} dB")
```

```
notch depth at 60 Hz: -68.1 dB
```

The design itself carries \(-68.1\,\mathrm{dB}\) of rejection at 60 Hz;
the measured `-50.8 dB` is worse because the 0.05-amplitude noise floor
in `x` limits how deep any measurement of "how much is left" can read,
no matter how good the filter is. A filter's own response and what you
can verify of it from noisy data are two different numbers, and the gap
between them is not a bug.

## What this lesson has not asked

Every filter above came from one function call — `butter` — that quietly
made a decision you never saw: whether the difference equation feeds
back on itself at all, and if so, how. `butter` chose feedback; it is an
*IIR* design. `fir1`, next to it in the standard library, makes the
opposite choice, and the two are not interchangeable defaults with
different names. One of them can never become unstable no matter how
badly it is designed. The other can, in one line of arithmetic, on
purpose, so you can watch it happen. Lesson 8 is that arithmetic.
