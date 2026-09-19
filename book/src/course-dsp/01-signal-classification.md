# Signal classification

## The idea

A hospital's cardiac trace and its digital thermometer watch the same
patient and disagree, in a precise sense, about what kind of thing a
signal is.

A **signal** is a value that changes along some axis — usually time — and
carries information. Two independent questions classify it, and most
beginner mistakes come from collapsing them into one.

**Continuous-time or discrete-time.** A continuous-time signal is defined
at every instant and written \(x(t)\), with round brackets and a real
\(t\). A discrete-time signal exists only at a countable set of instants
and is written \(x[n]\), with square brackets and an integer \(n\). The
two are related by sampling at rate \(f_s\):

\[
x[n] = x(nT), \qquad T = \frac{1}{f_s}
\]

**Continuous-valued or discrete-valued.** A voltage can sit at any level
in its range. A 12-bit converter's output cannot — it has 4096 rungs and
nothing between them.

Crossing the two axes gives four quadrants, not two. *Analog* is
continuous on both. *Digital* is discrete on both. The other corners are
real: a sample-and-hold circuit holds an exact voltage at discrete
instants, and a logic gate's output is discrete in value but can change
at any instant it likes. Sampling moves a signal along one axis;
quantization moves it along the other. Lesson 3 covers both.

```qu
tc = linspace(0, 0.04, 400)
fs = 200
n  = 0 to 7
t  = n / fs

figure()
subplot(2, 1, 1)
plot(tc, sin(2*pi*50*tc), color = "royalblue")
title("Continuous-time: x(t)")
xlabel("t (s)")
ylabel("x(t)")
subplot(2, 1, 2)
stem(t, sin(2*pi*50*t), color = "crimson")
title("Discrete-time: x[n] = x(nT)")
xlabel("t (s)")
ylabel("x[n]")
```

The bottom panel is not a crude sketch of the top one. It is the complete
record a converter would keep — nothing is known, or needed, about the
gaps.

**Deterministic or random.** A metronome's next click is fixed by its
history. Static's is not, and no amount of listening to the past predicts
its next sample — though its long-run statistics can still be entirely
predictable, which is why "random" does not mean "unknowable."

**Periodic or aperiodic.** A signal is periodic with period \(P\) (the
smallest positive integer for which this holds) when

\[
x[n+P] = x[n] \quad \text{for every } n.
\]

That is a testable claim about the whole signal, not an impression from
a short window. A sum of two periodic signals is periodic only if their
periods share a common multiple; an irrational frequency ratio never
repeats, however periodic it looks on any finite screen.

**Energy or power.** Two measures of size, kept separate because no
signal needs only one:

\[
E = \sum_{n=-\infty}^{\infty} |x[n]|^2,
\qquad
P = \lim_{N\to\infty}\frac{1}{2N+1}\sum_{n=-N}^{N} |x[n]|^2 .
\]

An endless tone has infinite energy and finite power — a **power
signal**. A finite burst has finite energy and zero average power — an
**energy signal**. Neither quantity describes both kinds.

**Even or odd.** A signal is even if \(x[-n] = x[n]\) and odd if
\(x[-n] = -x[n]\). Almost nothing encountered in practice is purely
either, but every signal decomposes uniquely into one of each:

\[
x_e[n] = \frac{x[n] + x[-n]}{2}, \qquad x_o[n] = \frac{x[n] - x[-n]}{2}.
\]

## In Qu

Everything Qu holds in a vector is already discrete in time and value.
Sample a 50 Hz tone at 500 Hz, ten samples, and make the sampling
explicit:

```qu
fs = 500
n  = 0 to 9
t  = n / fs
x  = sin(2*pi*50*t)
print("n = {n}")
print("t = {t}")
print("x = {round(x, 4)}")
```

```
n = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
t = [0, 0.002, 0.004, 0.006, 0.008, 0.01, 0.012, 0.014, 0.016, 0.018]
x = [0, 0.5878, 0.9511, 0.9511, 0.5878, 0, -0.5878, -0.9511, -0.9511, -0.5878]
```

Now quantize with a 3-bit converter spanning -1 to +1, step \(\Delta =
2/8\), rounding every reading to the nearest rung:

```qu
q  = 2 / 8
xq = round(x / q) * q
print("xq = {xq}")
print("worst error: {max(abs(x - xq)):.4f}, half a step is {q/2}")

figure()
plot(t, x, color = "royalblue", label = "x")
stair(t, xq, color = "crimson", label = "quantized")
legend("x", "quantized")
title("A 50 Hz tone and its 3-bit quantized version")
xlabel("t (s)")
ylabel("amplitude")
```

```
xq = [0, 0.5, 1, 1, 0.5, 0, -0.5, -1, -1, -0.5]
worst error: 0.0878, half a step is 0.125
```

The error is bounded by half a step and is a deterministic function of
the input, not noise — a fact Lesson 3 turns to its advantage.

The other classifications are just as measurable. Two unrelated `randn`
draws agree on mean and standard deviation to two decimals despite
sharing no sample:

```qu
N     = 2000
t2    = (0 to N-1) / fs
tone  = sin(2*pi*50*t2)
hiss  = randn(N, seed = 7)
print("hiss mean {mean(hiss):.4f}, std {std(hiss):.4f}")

P = fs / 50
print("tone, mismatch at lag P: {max(abs(tone[P:N-1] - tone[0:N-1-P])):.2e}")
print("hiss, mismatch at lag P: {max(abs(hiss[P:N-1] - hiss[0:N-1-P])):.4f}")
```

```
hiss mean 0.0201, std 0.9839
tone, mismatch at lag P: 2.06e-13
hiss, mismatch at lag P: 5.3753
```

The tone repeats to thirteen digits of floating-point dust; the noise is
off by 5.4, about as wrong as a unit-variance signal can be. And an
endless tone against a 100 ms burst confirms energy and power measure
different things:

```qu
function report(secs)
    n2 = fs * secs
    tt = (0 to n2-1) / fs
    tn = sin(2*pi*50*tt)
    burst = where(tt < 0.1, tn, 0.0)
    print("{secs} s: tone power {mean(tn.^2):.4f}  |  burst energy {energy(burst):.1f}, power {mean(burst.^2):.4f}")
    return 0
end function
r = report(1)
r = report(8)
```

```
1 s: tone power 0.5000  |  burst energy 25.0, power 0.0500
8 s: tone power 0.5000  |  burst energy 25.0, power 0.0062
```

Eight times the recording, the same tone power, but the burst's fixed
25.0 of energy drains toward zero average power — it never had more to
give.

Last, split a one-sided decay into even and odd parts, using `flip` as
the discrete \(x[-n]\):

```qu
m  = -3 to 3
d  = where(m >= 0, 0.8 .^ m, 0.0)
de = (d + flip(d)) / 2
dd = (d - flip(d)) / 2
print("d  = {round(d, 4)}")
print("de + do - d, worst: {max(abs(de + dd - d)):.2e}")

figure()
plot(m, d, color = "royalblue", marker = "o", label = "d")
plot(m, de, color = "seagreen", marker = "o", label = "even part")
plot(m, dd, color = "crimson", marker = "o", label = "odd part")
legend("d", "even part", "odd part")
title("A one-sided signal split into even and odd parts")
xlabel("n")
ylabel("d[n]")
```

```
d  = [0, 0, 0, 1, 0.8, 0.64, 0.512]
de + do - d, worst: 0.00e0
```

The reconstruction is not approximate. Adding the even and odd parts back
returns the original bit-for-bit, because the operations involved — sum,
scale, negate an index — treat a sum of inputs as the sum of what they do
to each input separately.

That property has a name, and a system that has it, together with one
more, can be described completely by a single measurement: what it does
to one tap. Lesson 2 makes that measurement.
