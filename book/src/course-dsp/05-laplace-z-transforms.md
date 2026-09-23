# Lesson 5 — Laplace & Z-Transforms

## The idea

A plucked guitar string rings and dies away. A microphone held too close to
its own speaker rings and grows, into feedback. Both are the natural
response of a system left alone after a single disturbance; one decays
toward silence, the other explodes.

Lesson 4 built everything from \(e^{-j2\pi ft}\), a complex exponential
that only spins in place at constant radius — a pure, undying tone. It has
no way to represent decay or growth. Give it a partner: a plain real
exponential \(e^{\sigma t}\), which shrinks when \(\sigma < 0\) and grows
when \(\sigma > 0\), and multiply the two:

\[
e^{\sigma t}\, e^{j\omega t} = e^{(\sigma + j\omega)t}
\]

The result is a spiral rather than a circle — still turning at rate
\(\omega\), but its radius shrinking or growing at rate \(\sigma\) as it
turns. A decaying string is a spiral winding inward; feedback is a spiral
winding outward; a pure Fourier tone is the special case \(\sigma = 0\),
where the spiral degenerates back into a circle.

```qu
t = (0 to 200) / 200 * 2
decaying = exp(-2 * t) .* cos(2 * pi * 5 * t)
steady = cos(2 * pi * 5 * t)
growing = exp(0.8 * t) .* cos(2 * pi * 5 * t)
figure()
subplot(1, 2, 1)
plot(t, decaying, label = "sigma < 0")
plot(t, steady, label = "sigma = 0")
plot(t, growing, label = "sigma > 0")
xlabel("time (s)")
ylabel("amplitude")

theta = (0 to 100) / 100 * 2 * pi
subplot(1, 2, 2)
plot(cos(theta), sin(theta), label = "unit circle")
scatter([0.7], [0], label = "stable pole")
scatter([1.2, 1.2], [0.3, -0.3], label = "unstable poles")
xlabel("Re")
ylabel("Im")
```

Plot every value \(s = \sigma + j\omega\) on a plane — \(\sigma\)
horizontal, \(\omega\) vertical — and you get the **s-plane**. Its
vertical axis (\(\sigma = 0\)) is exactly Fourier's territory: pure
oscillation, neither growing nor decaying. Left of it is decay; right of
it is growth. The Laplace transform is the Fourier integral with this
second axis restored:

\[
X(s) = \int_{0}^{\infty} x(t)\, e^{-st}\, dt, \qquad s = \sigma + j\omega
\]

Set \(\sigma = 0\) and this collapses exactly to Lesson 4's Fourier
transform: Fourier is not a separate idea, it is Laplace evaluated on one
particular line of the s-plane.

Sampled systems get the same treatment, built the same way the DFT was
built from the continuous integral — sum instead of integrate, over a
variable `z` instead of `s`:

\[
X(z) = \sum_{n=0}^{\infty} x[n]\, z^{-n}, \qquad z = e^{sT}
\]

`T` is the sampling period. This substitution reshapes the whole picture:
the s-plane's imaginary axis maps to \(|z| = 1\), the **unit circle**. The
left half-plane (decay) maps to *inside* the circle; the right half
(growth) maps to *outside*. A right-hand illustration above places two
example poles this way — one inside the circle, stable, and a pair outside
it, unstable — before any real filter has entered the picture at all.

A transfer function \(H(z) = B(z)/A(z)\) has **zeros**, where the numerator
vanishes (frequencies the system blocks), and **poles**, where the
denominator vanishes (frequencies the system rings at on its own, even
with no input). **Stability rule:** every pole must sit strictly inside the
unit circle (discrete-time) or strictly left of the imaginary axis
(continuous-time). A pole exactly on the boundary rings forever, neither
growing nor decaying — an idealized tuning fork, never realized exactly by
anything physical.

## In Qu

Qu does not yet expose a general continuous-time transfer-function tool —
no `s`-domain `tf()` sitting next to the filter designers — but its
discrete-time toolkit turns pole location from algebra into numbers you
can read off a real design:

```qu
fs = 1000
lp_wide = butter(2, "low", 100, fs)
lp_narrow = butter(2, "low", 10, fs)

pw = poles(lp_wide)
pn = poles(lp_narrow)
print("wide-band poles: {pw:.6f}")
print("narrow-band poles: {pn:.6f}")
print("wide |pole| = {abs(pw[0]):.4f}")
print("narrow |pole| = {abs(pn[0]):.4f}")
print("wide stable: {is_stable(lp_wide)}, narrow stable: {is_stable(lp_narrow)}")

theta = (0 to 100) / 100 * 2 * pi
figure()
plot(cos(theta), sin(theta), label = "unit circle")
scatter(real(pw), imag(pw), label = "wide poles")
scatter(real(pn), imag(pn), label = "narrow poles")
xlabel("Re")
ylabel("Im")
```

```
wide-band poles: [0.57149 + 0.293599i, 0.57149 - 0.293599i]
narrow-band poles: [0.955599 + 0.042512i, 0.955599 - 0.042512i]
wide |pole| = 0.6425
narrow |pole| = 0.9565
wide stable: true, narrow stable: true
```

Both designs are stable — `butter` never hands you an unstable filter —
but not equally close to trouble. Narrowing the passband from 100 Hz to
10 Hz, out of the same 1000 Hz sample rate, pulls the pole magnitude from
`0.64` to `0.96`, dragging it toward the unit circle's edge. A pole near
that edge means a longer, more sharply resonant impulse response: the
filter rings more and settles more slowly. This is the general shape of
the trade-off between selectivity and stability margin, visible directly
in a printed number rather than asserted.

`poles`/`is_stable` describe an existing design. You can also watch decay
versus growth happen, by building the two one-pole systems this lesson
opened with and feeding each an impulse:

```qu
impulse_x = impulse(12)
decay = filter_ba([1], [1, -0.8], impulse_x)
grow  = filter_ba([1], [1, -1.2], impulse_x)
print("decaying (pole at 0.8): {decay:.4f}")
print("growing (pole at 1.2): {grow:.4f}")

n = 0 to 11
figure()
plot(n, decay, label = "pole at 0.8")
plot(n, grow, label = "pole at 1.2")
xlabel("sample index n")
ylabel("amplitude")
```

```
decaying (pole at 0.8): [1, 0.8, 0.64, 0.512, 0.4096, 0.32768, 0.262144, 0.209715, ... (12 elements)]
growing (pole at 1.2): [1, 1.2, 1.44, 1.728, 2.0736, 2.48832, 2.985984, 3.583181, ... (12 elements)]
```

`filter_ba(b, a, x)` runs `y[n] = x[n] + 0.8*y[n-1]` in the first case and
`y[n] = x[n] + 1.2*y[n-1]` in the second — the difference equation behind a
single real pole at `z = 0.8` and `z = 1.2`. One sits inside the unit
circle and the response is a guitar string, each sample `0.8` times the
last, approaching zero. The other sits outside, each sample `1.2` times
the last, forever: mic feedback, laid out as plain numbers. Nothing here
needed a spectrum — the pole location alone predicted the shape of the
answer before a single sample was computed.

Every claim in this lesson assumed a linear, time-invariant system with
poles fixed in place for all time. That describes a designed filter well.
It does not describe a chirp sweeping from 20 Hz to 20 kHz, a spoken word,
or a bird call, where the frequency content is not one fixed set of poles
but something that changes while you listen. The s-plane and z-plane
answer "will this system's own behavior blow up," and cannot answer "what
is happening in this signal right now, compared to a moment ago." Lesson 6
takes on that question directly.

## Exercises

1. Change the decaying pole from `0.8` to `0.5` (`filter_ba([1], [1,
   -0.5], impulse_x)`) and compare the resulting sequence's first four
   values against the lesson's own `0.8` case. **Check:** both start at
   1 (the impulse itself), but the `0.5` pole should fall off much
   faster — `[1, 0.5, 0.25, 0.125, ...]` versus `[1, 0.8, 0.64, 0.512,
   ...]` — because each sample is the pole location times the last one,
   and 0.5 shrinks a number faster than 0.8 does.

2. What does a pole placed exactly at `z = 1.0` do — neither the `0.8`
   case's decay nor the `1.2` case's growth? Predict it from the
   difference equation `y[n] = x[n] + p*y[n-1]` with `p = 1` before
   running `filter_ba([1], [1, -1.0], impulse(12))`.
   **Check:** constant output, `[1, 1, 1, 1, ...]` forever — neither
   growing nor decaying, the exact boundary case. A pole on the unit
   circle itself (not just outside it) is marginally stable: it doesn't
   blow up, but it never settles either.

3. The opening of this lesson shows \(e^{\sigma t}e^{j\omega t}\) as a
   spiral: inward for \(\sigma < 0\), a plain circle for \(\sigma = 0\),
   outward for \(\sigma > 0\) — the continuous-time picture. The
   discrete-time pole examples later use `0.8`, `1.0`, and `1.2` as
   their three cases. What is the discrete-time analogue of \(\sigma =
   0\) — what pole magnitude corresponds to the continuous case's
   perfect circle? **Check:** `|z| = 1`, the unit circle itself (which
   is exactly the `p = 1` case in exercise 2) — continuous-time's
   stability boundary is the imaginary axis (\(\sigma = 0\)), and the
   \(z = e^{sT}\) mapping that connects the two domains sends that axis
   to the unit circle.
