# 14. Channels and Equalization

## The idea

Between a transmitter and a receiver sits a **channel**: the air, a
cable, an acoustic space. A wave rarely arrives by one path only — it
takes a direct route and several reflections at once, each of a
different length, so each arrives at a slightly different time and
strength. The receiver sees all of them summed, plus thermal noise from
its own electronics. That is exactly the convolution equation from
Lesson 2, applied to a physically real system:

\[
y[n] = \sum_k h[k]\,x[n-k] + w[n]
\]

\(h\) is the channel's impulse response, one tap per path; \(w\) is
additive noise. Multipath is not a separate phenomenon from convolution —
it is convolution, with a physical cause.

```qu
seed(10)
fs = 2000
t = (0 to 199) / fs
x = sin(2*pi*40*t)
h = [1, 0, 0, 0.6, 0, 0, 0.3]
y = conv(x, h, mode="same") + 0.02*randn(len(x))

figure()
plot(t, x)
plot(t, y)
xlabel("time (s)")
ylabel("amplitude")
title("A signal before and after a multipath channel")
legend("transmitted x(t)", "received y(t)")
```

**Equalization** is the receiver's attempt to undo \(h\). Convolution in
time is multiplication in frequency, \(Y(f) = X(f)H(f)\), so the most
direct inverse — *zero-forcing* equalization — simply divides it back
out:

\[
\hat{X}(f) = \frac{Y(f)}{H(f)}
\]

This inverts \(h\) exactly wherever \(H(f)\) is well away from zero. It
also inverts \(h\) at frequencies where \(H(f)\) is *near* zero — a deep
fade from two paths cancelling — and dividing by a near-zero number
amplifies whatever noise lives at that frequency by the same huge factor:

```qu
f = (0 to 255) / 256
h_mild = [1, 0, 0, 0.5]
h_null = [1, 0, -0.999]
Hm = abs(fft(h_mild, 256))
Hn = abs(fft(h_null, 256))

figure()
plot(f, Hm)
plot(f, Hn)
xlabel("normalized frequency")
ylabel("|H(f)|")
title("Channel frequency response: mild fading vs. a deep null")
legend("mild multipath", "near-cancelling multipath")
```

A receiver rarely knows \(h\) in advance — multipath depends on geometry,
weather, and traffic — so real systems estimate it first, typically from
a *training sequence* the transmitter sends and the receiver already
knows the answer to. Every Wi-Fi packet and cellular frame spends part of
its airtime this way, telling the receiver nothing about the message,
only about the channel the message is about to travel through.

## In Qu

**Undoing a known channel.** Send a chirp through a three-tap multipath
channel, corrupt it with a little noise, and equalize with the channel's
own inverse spectrum:

```qu
seed(5)
fs = 8000
x = chirp(500, 3000, fs, 64)
h = [1, 0, 0, 0.5, 0, 0, 0.25]
y = conv(x, h, mode="full")
noisy = y + 0.005*randn(len(y))
err_raw = rms(noisy[0:len(x)-1] - x) / rms(x)
print("len(x) = {len(x)}, len(y) = {len(y)}")
print("relative error, channel output vs. original = {err_raw:.4f}")

N = len(y)
H = fft(h, N)
print("min |H| = {min(abs(H)):.4f}, max |H| = {max(abs(H)):.4f}")

Y = fft(noisy, N)
xhat = real(ifft(Y ./ H, N))[0:len(x)-1]
err_eq = rms(xhat - x) / rms(x)
print("relative error after equalization = {err_eq:.4f}")

n = 0 to len(x)-2
figure()
plot(n, x)
plot(n, noisy[0:len(x)-1])
plot(n, xhat)
xlabel("sample")
ylabel("amplitude")
title("Chirp: original, channel output, equalized")
legend("original x", "channel output", "equalized xhat")
```

```
len(x) = 64, len(y) = 70
relative error, channel output vs. original = 0.5284
min |H| = 0.6495, max |H| = 1.7500
relative error after equalization = 0.0093
```

53% relative error becomes 0.93%. `min |H|` never dropped much below 0.65,
so dividing by \(H\) never demanded much amplification — this is the same
move as Lesson 9's adaptive filters, done here with the channel given
rather than learned.

**Where the inverse breaks.** Swap in a channel with two nearly-equal,
nearly out-of-phase paths — a much deeper null — and repeat at two noise
levels:

```qu
seed(5)
fs = 8000
x = chirp(500, 3000, fs, 64)
h2 = [1, 0, -0.999]
y2 = conv(x, h2, mode="full")
N2 = len(y2)
H2 = fft(h2, N2)
print("min |H2| = {min(abs(H2)):.5f}, max |H2| = {max(abs(H2)):.4f}")

noisy_lo = y2 + 0.005*randn(N2)
noisy_hi = y2 + 0.05*randn(N2)
eq_lo = real(ifft(fft(noisy_lo, N2) ./ H2, N2))[0:len(x)-1]
eq_hi = real(ifft(fft(noisy_hi, N2) ./ H2, N2))[0:len(x)-1]
err_lo = rms(eq_lo - x) / rms(x)
err_hi = rms(eq_hi - x) / rms(x)
print("relative error, near-null channel, low noise  = {err_lo:.4f}")
print("relative error, near-null channel, high noise = {err_hi:.4f}")

n = 0 to len(x)-2
figure()
plot(n, x)
plot(n, eq_lo)
xlabel("sample")
ylabel("amplitude")
title("Zero-forcing equalizer amplifying noise near a channel null")
legend("original x", "equalized (near-null H, low noise)")
```

```
min |H2| = 0.00100, max |H2| = 1.9967
relative error, near-null channel, low noise  = 0.3444
relative error, near-null channel, high noise = 14.9086
```

`min |H2|` of 0.001 means `1/H2` is roughly 1000 at one frequency — the
equalizer amplifies whatever noise lives there a thousandfold. At the
milder noise level the correction is already a third of the signal's own
size; at ten times the noise, the "equalized" output is nearly fifteen
times larger than the thing it was meant to reconstruct. Zero-forcing
inverts the channel exactly, which is precisely the problem: it inverts
the parts that were nearly zero along with the parts that mattered.
Better equalizers trade a little residual distortion for far less noise
amplification, but none of them make the trade disappear — some
frequencies of some channels are close enough to unrecoverable that no
linear filter fixes them.

Even with the channel known exactly and the best inverse filter applied,
a bit will occasionally come out wrong on the other end — not because the
mathematics is incomplete, but because information was genuinely
destroyed in transit. The next move is not to fight the channel harder.
It is to add structure to the data itself, so a wrong bit can be caught
and fixed after the fact without asking the transmitter to resend it.

**Next: [Lesson 15 — Coding and compression](15-coding-compression.md).**
