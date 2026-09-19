# LTI systems and convolution

## The idea

A concrete stairwell returns more than the clap sent into it — copies of
that same clap, delayed and shrunk by each wall. Talk in the stairwell
instead, and every syllable gets the identical treatment.

A **system** takes a signal in and returns a signal out: a stairwell, a
microphone, a moving average, a car suspension. Two properties, independent
of each other, decide whether that system is tractable.

**Linearity.** Scaling the input scales the output by the same factor, and
the response to a sum of inputs is the sum of the responses:

\[
S(a\,x_1[n] + b\,x_2[n]) = a\,S(x_1[n]) + b\,S(x_2[n]).
\]

**Time-invariance.** Delaying the input only delays the output, unchanged:
if \(S(x[n]) = y[n]\), then \(S(x[n-d]) = y[n-d]\).

A system with both is **LTI**, and nearly everything not deliberately
built otherwise qualifies, at least over the range it is used in. The
payoff is an equivalence: any discrete signal is a sum of scaled, shifted
impulses — that is what a list of numbers *means*. By time-invariance, the
system's response to a unit impulse at sample \(k\) is its **impulse
response** \(h[n]\), shifted by \(k\). By linearity, the response to any
input is a sum of shifted, scaled copies of \(h[n]\):

\[
y[n] = \sum_{k=-\infty}^{\infty} h[k]\,x[n-k] = (x * h)[n].
\]

This operation is **convolution**. Flip one sequence, slide it to
position \(n\), multiply the overlap, sum — once per output sample. The
German name, *Faltung*, means folding, and the fold is visible in the
formula: as \(k\) runs forward through \(h\), the index into \(x\) runs
backward.

```qu
n = 0 to 15
h = 0.7 .^ n

figure()
subplot(1, 2, 1)
stem(n, h, color = "royalblue")
title("A generic impulse response h[n]")
xlabel("n")
ylabel("h[n]")

a = zeros(16)
a[3:7] = 1.0
c = conv(a, a)
subplot(1, 2, 2)
stem(0 to length(c)-1, c, color = "seagreen")
title("Two rectangular pulses convolved: a triangle")
xlabel("n")
ylabel("amplitude")
```

The exponential decay on the left is a plausible \(h[n]\) — a system that
remembers its recent past and forgets it geometrically. The triangle on
the right is what convolving two identical rectangular pulses always
produces: overlap grows linearly to a peak, then shrinks the same way.

Two structural facts follow directly from \(h[n]\), and both matter later.
A system is **causal** — it does not respond before it is struck — exactly
when \(h[n] = 0\) for all \(n < 0\). It is **stable**, meaning a bounded
input can never produce an unbounded output, exactly when
\(\sum_n |h[n]| < \infty\).

## In Qu

A stairwell with two walls, written literally — direct sound, one
reflection three samples later at 60% amplitude, a second seven samples
later at 30%:

```qu
function room(x)
    n = length(x)
    y = zeros(n)
    for i = 0 to n-1
        y[i] = x[i]
        if i >= 3 then
            y[i] += 0.6 * x[i-3]
        end if
        if i >= 7 then
            y[i] += 0.3 * x[i-7]
        end if
    end for
    return y
end function

clap = impulse(8)
h    = room(clap)
print("clap = {clap}")
print("h    = {h}")

figure()
stem(0 to 7, h, color = "royalblue")
title("The room's impulse response h[n], measured with one clap")
xlabel("n")
ylabel("h[n]")
```

```
clap = [1, 0, 0, 0, 0, 0, 0, 0]
h    = [1, 0, 0, 0.6, 0, 0, 0, 0.3]
```

That output is the floor plan of the room, read straight off a single
measurement. Check both defining properties before trusting anything
built on them:

```qu
a2 = [1, 0, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0]
b  = randn(12, seed = 3)
lhs = room(2*a2 - 5*b)
rhs = 2*room(a2) - 5*room(b)
print("linearity, worst difference: {max(abs(lhs - rhs)):.2e}")

p = impulse(20, index = 2)
r = impulse(20, index = 9)
yp = room(p)
yr = room(r)
print("time invariance, worst difference: {max(abs(yr[7:19] - yp[0:12])):.2e}")
```

```
linearity, worst difference: 4.44e-16
time invariance, worst difference: 0.00e0
```

Four parts in ten quadrillion is double-precision rounding. The second
result is exactly zero, because delaying a tap and delaying the answer are
the same shuffle of the same numbers.

Watch the pile-of-copies argument work on two claps, one of them louder
and later:

```qu
x2 = zeros(24)
x2[0]  = 1
x2[12] = 0.7
y2 = room(x2)

figure()
subplot(2, 1, 1)
stem(0 to 23, x2, color = "royalblue")
title("Input: two claps")
xlabel("n")
ylabel("x[n]")
subplot(2, 1, 2)
stem(0 to 23, y2, color = "crimson")
title("Output: each clap dragging its own pair of echoes")
xlabel("n")
ylabel("y[n]")
```

Each clap in the top panel produces its own scaled, shifted copy of
\(h[n]\) in the bottom panel, and the two copies add where they overlap.
Nothing else is possible once linearity and time-invariance hold.

The hand-written reflection loop never called `conv`, and it did not need
to:

```qu
xs = [1, 2, 3, 4]
g  = [0.5, 1, 0.5]
print("conv(xs, g) = {conv(xs, g)}")
print("y[3] by hand = {xs[3]*g[0] + xs[2]*g[1] + xs[1]*g[2]}")

v = randn(64, seed = 11)
print("room vs conv, worst difference: {max(abs(room(v) - conv(v, h)[0:63])):.2e}")
```

```
conv(xs, g) = [0.5, 2, 4, 6, 5.5, 2]
y[3] by hand = 6
room vs conv, worst difference: 4.44e-16
```

The hand calculation is \(0.5\cdot4 + 1\cdot3 + 0.5\cdot2 = 6\), matching
the vector's fourth entry. Sixty-four random samples through the loop and
through `conv` against the measured \(h\) agree to the last bit but one.
Every LTI system is a convolution with its own impulse response, and every
convolution is an LTI system — the same object, seen from different
sides. That is why commercial reverb plug-ins ship as libraries of
impulse responses of real cathedrals: clap once, and any voice can be put
into that room.

Every line above took the input for granted — a list of numbers standing
in for a wave, faithfully. Test that assumption directly:

```qu
fs = 1000
t  = (0 to 199) / fs
low  = sin(2*pi*100*t)
high = sin(2*pi*900*t)
print("100 Hz and 900 Hz sequences, worst difference from being exact negatives: {max(abs(high + low)):.2e}")
print("through the room:  worst difference: {max(abs(room(high) + room(low))):.2e}")
```

```
100 Hz and 900 Hz sequences, worst difference from being exact negatives: 1.07e-13
through the room:  worst difference: 1.03e-13
```

A 900 Hz tone and a 100 Hz tone, sampled at 1000 Hz, are the same sequence
with a minus sign — not similar, identical to thirteen decimal places. The
room, behaving perfectly, returns one answer for two completely different
sounds, and no processing afterwards can recover which one arrived.
Convolution did nothing wrong here. The damage happened earlier, in the
act of turning a wave into a list — which is exactly what Lesson 3
examines.
