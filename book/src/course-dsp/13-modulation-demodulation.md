# 13. Modulation and Demodulation

## The idea

A signal rarely travels at the frequency it was born at. A voice channel
lives under 4 kHz; a Wi-Fi link, a cellular carrier, and a GPS timing
signal all share the same air without colliding, because each was moved
into its own slice of spectrum before it was radiated. **Modulation** is
the operation that does the moving: it takes a baseband message and rides
it on a much higher-frequency carrier, for two reasons that have nothing
to do with the information itself.

The first is physics. An efficient antenna needs to be a sizeable
fraction of the wavelength it radiates, conventionally a quarter-wave,
\(L = \frac{1}{4}\cdot\frac{c}{f}\). A 3 kHz tone has a 25 km quarter-wave
length; a 900 MHz carrier has an 8 cm one. The second is coexistence:
give every transmitter its own frequency slice and a receiver tuned to
that slice hears only what was meant for it.

**Analog case — amplitude modulation.** Let \(m(t)\) be the message,
scaled to \([-1, 1]\), and \(\cos(2\pi f_c t)\) the carrier at the
assigned frequency \(f_c\). AM multiplies the carrier's amplitude by the
message:

\[
y(t) = \big(1 + m(t)\big)\cos(2\pi f_c t)
\]

The carrier oscillates at \(f_c\), far too fast to radiate a baseband
signal directly, but its peak amplitude traces \(m(t)\) exactly — the
message survives, relocated into an envelope wrapped around a wave short
enough to build an antenna for.

```qu
seed(1)
fs = 4000
t = (0 to 799) / fs
m = sin(2*pi*20*t)
fc = 200
y = (1 + 0.6*m) * cos(2*pi*fc*t)
envelope = 1 + 0.6*m

figure()
plot(t, y)
plot(t, envelope)
plot(t, -envelope)
xlabel("time (s)")
ylabel("amplitude")
title("Amplitude modulation: carrier and envelope")
legend("carrier y(t)", "envelope", "")
```

**Digital case — the constellation.** Digital modulation places each
symbol at a point in the complex plane rather than tracing a continuous
envelope. Binary Phase-Shift Keying (BPSK) sends one bit as
\(s \in \{-1, +1\}\) on the real axis. Quadrature schemes use both axes at
once: Quadrature Phase-Shift Keying (QPSK) — a 4-point special case of
Quadrature Amplitude Modulation (QAM) — packs one bit onto each axis,

\[
s = \frac{1}{\sqrt{2}}\big(b_I + j\,b_Q\big), \qquad b_I, b_Q \in \{-1, +1\},
\]

doubling the data carried per transmitted symbol for the same symbol
rate. The receiver's whole job is to decide which constellation point was
probably sent, given a noisy version of \(s\).

```qu
bpsk = [-1, 1]
bpsk_y = [0, 0]
qpsk_i = [1, -1, -1, 1] / sqrt(2)
qpsk_q = [1, 1, -1, -1] / sqrt(2)

figure()
panel(1, 2, 1)
scatter(bpsk, bpsk_y)
xlabel("in-phase")
ylabel("quadrature")
title("BPSK constellation")

panel(1, 2, 2)
scatter(qpsk_i, qpsk_q)
xlabel("in-phase")
ylabel("quadrature")
title("QPSK constellation")
```

## In Qu

**Recovering an AM message.** The receiver must pull \(m(t)\) back out of
\(y(t)\) without already knowing it. Qu's `hilbert` builds the *analytic
signal* \(a(t) = x(t) + j\,\mathcal{H}\{x(t)\}\), whose magnitude at every
instant is exactly the input's envelope, whatever the carrier phase is
doing underneath. For the AM signal above, that envelope is
\(1 + 0.5\,m(t)\), so undoing the known offset and scale hands back the
message:

```qu
seed(1)
fs = 48000
t = (0 to 4799) / fs
fc = 4000
m = sin(2*pi*200*t)
y = (1 + 0.5*m) * cos(2*pi*fc*t)
print("y[0:5] = {y[0:5]}")

envelope = abs(hilbert(y))
recovered = (envelope - mean(envelope)) / 0.5
err = rms(recovered - m)
print("recovered[0:5] = {recovered[0:5]}")
print("m[0:5]         = {m[0:5]}")
print("rms recovery error = {err:.2e}")

figure()
plot(t[0:300], m[0:300])
plot(t[0:300], recovered[0:300])
xlabel("time (s)")
ylabel("amplitude")
title("AM message vs. Hilbert-envelope recovery")
legend("original m(t)", "recovered")
```

```
y[0:5] = [1, 0.87736, 0.513084, 6.363446e-17, -0.526132, -0.922545]
recovered[0:5] = [-7.105427e-15, 0.026177, 0.052336, 0.078459, 0.104528, 0.130526]
m[0:5]         = [0, 0.026177, 0.052336, 0.078459, 0.104528, 0.130526]
rms recovery error = 1.32e-13
```

An idealized, noiseless channel gives the message back to fourteen
decimal places. This is *envelope detection*, historically done with a
diode and a capacitor rather than a Hilbert transform — the analytic
signal is the same idea done exactly instead of approximately.

**A noisy QPSK link.** Add channel noise directly to complex symbols and
watch the constellation blur:

```qu
seed(3)
bits_i = randi(0, 1, 200)
bits_q = randi(0, 1, 200)
si = 2*bits_i - 1
sq = 2*bits_q - 1
symbols_c = (si + sq*1j) / sqrt(2)
noise_c = 0.35*(randn(200) + 1j*randn(200))
rx = symbols_c + noise_c
dec_i = (sign(real(rx)) + 1) / 2
dec_q = (sign(imag(rx)) + 1) / 2
errs = sum(abs(dec_i-bits_i)) + sum(abs(dec_q-bits_q))
print("QPSK bit errors out of {2*200} bits = {errs}")

figure()
scatter(real(rx), imag(rx))
xlabel("in-phase")
ylabel("quadrature")
title("Received QPSK constellation, SNR-limited")
```

```
QPSK bit errors out of 400 bits = 11
```

Eleven of 400 bits flip once the four clean corners smear into
overlapping clouds — the receiver still decides correctly most of the
time by asking each axis's sign, but the clouds now reach across the
decision boundary at zero often enough to matter. Qu has no
`qam_modulate` or `bpsk_demod`: there is no dedicated digital-modulation
function in the standard library, only the general-purpose complex
arithmetic, `randn`, and `sign` used above, plus `pwm` (natural-sampling
pulse-width modulation) for the one specialized scheme it does ship.
That is an honest description of where the library draws its line, not a
gap in this lesson — every digital modulation scheme, at its core, is
exactly the four lines above: map bits to a point in the plane, add the
channel, decide which point was probably sent.

Both examples above skipped straight from "symbol sent" to "symbol plus
noise received," as though the medium did nothing but add a little
static. Real channels do far worse: a signal arrives by several paths at
once, at different delays and strengths, and comes out reshaped before
any noise is added. That reshaping is a convolution, and undoing it is
next.

**Next: [Lesson 14 — Channels and equalization](14-channels-equalization.md).**
