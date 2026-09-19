# Lesson 20 — Sparse and compressed sensing

## The idea

The sampling theorem says: to recover a signal with no ambiguity, sample
at twice its highest frequency, no less. That bound is airtight under one
assumption it is easy to forget is being made — that nothing beyond the
signal's bandwidth is known in advance.

Compressed sensing adds a second assumption and the bound changes
completely. Suppose a signal of length \(n\) is **sparse** in some basis:
almost all its coefficients are exactly zero, and only \(k \ll n\) are not.
A spectrum that is a handful of tones among thousands of frequency bins is
sparse in the Fourier basis; a natural image is sparse in a wavelet basis,
which is the actual mechanism a JPEG-family codec exploits.

In symbols, take \(m\) linear measurements of an \(n\)-vector \(x\):

\[
y = Ax, \qquad A \in \mathbb{R}^{m \times n},\ m \ll n,\ \|x\|_0 = k
\]

An ordinary \(m \times n\) system with \(m < n\) has infinitely many
solutions and reveals nothing. Add the sparsity constraint, and under a
condition on \(A\) called the restricted isometry property — that for
every \(k\)-sparse vector \(v\),

\[
(1 - \delta_k)\,\|v\|_2^2 \le \|Av\|_2^2 \le (1 + \delta_k)\,\|v\|_2^2
\]

holds with \(\delta_k\) small — there is exactly one sparse \(x\)
consistent with \(y\), recoverable in principle by

\[
\hat{x} = \arg\min_{x} \|x\|_1 \quad \text{subject to} \quad y = Ax,
\]

or, more cheaply, by a greedy method that builds up the support one column
at a time. Recovery needs not "twice the bandwidth" but roughly
\(m \sim k \log(n/k)\) measurements — a number that can be far smaller than
\(n\) itself. Emmanuel Candès, Justin Romberg and Terence Tao, and
independently David Donoho, published the founding results within months
of each other in 2006; Nyquist's bound is from 1928.

What makes a vector a candidate for this at all is how fast its sorted
coefficients decay — a genuinely sparse or compressible signal collapses
to near-zero after a handful of terms, where unstructured noise does not:

```qu
seed(3)
n = 200
sparse = zeros(n)
for i in 0 to 5
    sparse[i * 30 + 5] = 5.0 / (i + 1)
end for
dense = randn(n, seed = 6)
sorted_sparse = sort(abs(sparse), true)
sorted_dense = sort(abs(dense), true)

figure()
plot(0 to n - 1, sorted_sparse, color = "#0f172a", width = 2, label = "sparse signal")
plot(0 to n - 1, sorted_dense, color = "#5B7CFA", width = 2, label = "dense (noise) signal")
xlabel("coefficient rank")
ylabel("magnitude (sorted, descending)")
title("Compressibility: how fast sorted coefficients decay")
legend()
```

Six non-zero values, and the sparse curve is flat at zero past rank 6; the
dense curve decays slowly because every one of its 200 values is doing
some work. Compressed sensing is only ever exploiting curves shaped like
the first one.

## In Qu

`cs_recover` performs the reconstruction. Build a 300-bin spectrum with
five active tones, take 90 random linear measurements of it — 90, not 300
— and ask for it back:

```qu
seed(4)
n = 300
k = 5
m = 90
spectrum = zeros(n)
bins = (18, 63, 122, 201, 267)
amps = [3.0, -2.2, 1.6, 2.8, -1.1]
idx = 0
for b in bins
    spectrum[b] = amps[idx]
    idx = idx + 1
end for

A = randn(m, n, seed = 4) / sqrt(m)
y = A * spectrum
r = cs_recover(A, y, method = "omp", sparsity = k)
print("{m} compressive measurements of a {n}-bin spectrum, {k} active tones")
print("support found:  {r.support}")
print("true support:   {bins}")
print("recovery error: {round(rmse(spectrum, r.x), 8)}")

figure()
plot(0 to n - 1, spectrum, color = "#0f172a", width = 2, label = "true spectrum")
scatter(0 to n - 1, r.x, color = "#e11d48", label = "recovered")
xlabel("frequency bin")
ylabel("amplitude")
title("Recovering a 300-bin spectrum from 90 measurements")
legend()
```

```
90 compressive measurements of a 300-bin spectrum, 5 active tones
support found:  [18, 63, 122, 201, 267]
true support:   (18, 63, 122, 201, 267)
recovery error: 0
```

Zero, not approximately zero. `cs_recover(..., method = "omp")` —
orthogonal matching pursuit — adds one column of `A` to its working
support per iteration, the one most correlated with whatever of `y` is not
yet explained, then re-solves an ordinary least-squares problem on exactly
those columns once it has found all five. With the true support in hand,
that solve is exact to floating point. The 90-row sensing matrix never
knew where the five nonzero bins were. It did not need to.

Before trusting a result this clean, `coherence(A)` measures the
worst-case similarity between any two columns of `A` (small is good), and
`cs_guarantee(A)` turns that into a sparsity bound the theory can actually
*prove* recoverable:

```qu
print("coherence {round(coherence(A), 3)}, provable sparsity {cs_guarantee(A)}")
```

```
coherence 0.439, provable sparsity 1
```

Read literally, only \(k=1\) is guaranteed — yet the recovery above found
\(k=5\) perfectly. The guarantee is genuinely that pessimistic: coherence
is the only certificate cheap enough to compute (the restricted isometry
property itself is NP-hard to verify for a specific matrix), and a random
Gaussian matrix routinely outperforms what coherence alone can promise.
Treat a coherence pass as reassurance, a failure as no information, and
never treat success on this bound as proof recovery will fail.

Hold the signal fixed and shrink the number of measurements toward the
sparsity count itself to see where the guarantee stops being pessimistic
and starts being right:

```qu
ms = (90, 60, 30, 20, 15, 10, 8)
errs = zeros(length(ms))
i = 0
for mm in ms
    Am = randn(mm, n, seed = 4) / sqrt(mm)
    ym = Am * spectrum
    rm = cs_recover(Am, ym, method = "omp", sparsity = k)
    errs[i] = rmse(spectrum, rm.x)
    print("m={mm}: recovery error {round(errs[i], 6)}")
    i = i + 1
end for

figure()
plot(ms, errs, color = "#0f172a", width = 2)
scatter(ms, errs, color = "#e11d48")
xlabel("number of measurements m")
ylabel("recovery rmse")
title("Recovery collapses as m approaches k=5")
```

```
m=90: recovery error 0
m=60: recovery error 0
m=30: recovery error 0.30425
m=20: recovery error 0.219978
m=15: recovery error 0.357057
m=10: recovery error 0.387586
m=8: recovery error 0.437739
```

The textbook rule of thumb is \(m \approx 4k = 20\) measurements. The curve
is not perfectly monotonic — `m=20` lands slightly better than `m=30`,
sampling jitter this close to the transition, not a contradiction — but
the shape is unmistakable: exact recovery through `m=60`, then a real
collapse, not a graceful fade, well before `m` reaches the sparsity count
itself. The rule of thumb is a starting point for "try it and check," not
a guarantee; the coherence bound is the only number here that comes with
one attached, and it was the more pessimistic of the two by a wide margin.

Nothing about `cs_recover` reconstructed a *generic* 300-number vector
from 90 measurements — that is impossible, and no algorithm changes the
linear algebra of an underdetermined system. What made it possible was the
sparsity assumption, checked after the fact by the recovered residual and
never smuggled in as a free lunch. That trade — structure for sample
count — is also the trade a classifier makes: rather than reconstructing
every sample of a signal faithfully, it bets that a few well-chosen
numbers extracted from it already carry everything a decision needs.
Lesson 21 takes that bet and measures whether it pays off.
