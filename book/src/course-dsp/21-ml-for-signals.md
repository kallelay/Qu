# Lesson 21 — Machine learning for signals

## The idea

A classifier does not want raw samples. It wants a short vector of numbers
— **features** — computed the same way from every example, that already
separate the classes at hand. A peak frequency, a band power, a spectral
centroid: exactly the quantities Fourier analysis already produces. The
signal processing does the seeing; the model only has to decide.

For a binary classifier, logistic regression fits a linear score to a
sigmoid. Given a feature vector \(f\), weights \(w\), and bias \(b\):

\[
z = w^\top f + b, \qquad p = \sigma(z) = \frac{1}{1 + e^{-z}}
\]

```qu
zz = (-800 to 800) / 100
p = 1 / (1 + exp(-zz))
figure()
plot(zz, p, color = "#0f172a", width = 2)
xlabel("z = w . f + b")
ylabel("p = sigma(z)")
title("The logistic function")
```

\(p\) is fit, from labeled examples, to land near 1 for one class and near
0 for the other, by minimizing the cross-entropy loss

\[
\mathcal{L}(w, b) = -\frac{1}{N}\sum_{i=1}^{N}
\Big[y_i \log p_i + (1 - y_i)\log(1 - p_i)\Big]
\]

via gradient descent, \(w \leftarrow w - \eta\nabla_w\mathcal{L}\). That
update is where a detail invisible in the formula becomes decisive: the
gradient's size scales with the feature's own scale. A step size \(\eta\)
tuned for a feature of order 1 sends a feature of order 100–1000 into wild
overshoot, and a feature near zero barely moves at all. A classifier can
have every class perfectly separable and still fail to fit, because the
optimizer, not the data, is where the fit breaks:

```qu
seed(9)
n = 40
f1_a = 0.3 + 0.15 * randn(n, seed = 1)
f2_a = 200 + 40 * randn(n, seed = 2)
f1_b = 0.7 + 0.15 * randn(n, seed = 3)
f2_b = 800 + 40 * randn(n, seed = 4)
figure()
scatter(f1_a, f2_a, color = "#0f172a", label = "class A")
scatter(f1_b, f2_b, color = "#e11d48", label = "class B")
xlabel("feature 1 (range ~0-1)")
ylabel("feature 2 (range ~0-1000)")
title("Two classes, two features on very different scales")
legend()
```

Both classes separate cleanly on either feature alone. The scales
themselves — not the data, not the model family — are what a gradient
descent fit is sensitive to, and fixing it is a matter of putting every
feature on comparable footing before fitting, not of collecting more data.

## In Qu

Two classes of synthetic signal: a 100 Hz tone in noise and a 250 Hz tone
in noise, 60 examples each, 40% noise. Extract one feature — the frequency
of each signal's strongest spectral peak, in raw hertz — and fit:

```qu
seed(42)
fs = 2000
N = 256
per_class = 60
t = (0 to N - 1) / fs

function peak_only(sig)
    mag = abs(rfft(sig))
    f = (0 to length(mag) - 1) * fs / N
    return [f[argmax(mag)]]
end function

X = zeros(per_class * 2, 1)
y = zeros(per_class * 2)
row = 0
for i in 0 to per_class - 1
    sig = sin(2*pi*100*t) + 0.4 * randn(N, seed = 1000 + i)
    X[row, :] = peak_only(sig)
    y[row] = 0
    row = row + 1
end for
for i in 0 to per_class - 1
    sig = sin(2*pi*250*t) + 0.4 * randn(N, seed = 2000 + i)
    X[row, :] = peak_only(sig)
    y[row] = 1
    row = row + 1
end for

s = train_test_split(X, y, test_size = 0.3, seed = 7)
m = logistic_model(s.X_train, s.y_train)
print("raw-Hz peak frequency, test accuracy: {round(m.score(s.X_test, s.y_test), 4)}")
print("coef: {round(m.coef, 4)}, intercept: {round(m.intercept, 4)}")
```

```
raw-Hz peak frequency, test accuracy: 0.4722
coef: [10.6641], intercept: -20.7316
```

47% — worse than a coin flip, on a problem where the two classes sit at
100 Hz and 250 Hz with zero overlap. The peak frequencies really do
separate perfectly; a raw hertz value of order 100–250 is exactly the
"feature 2" situation from Part 1, and the fit never converged anywhere
near a sane boundary. The fix is Qu's `standardize`, not a hand-picked
unit conversion:

```qu
Xs = standardize(X)
s2 = train_test_split(Xs, y, test_size = 0.3, seed = 7)
m2 = logistic_model(s2.X_train, s2.y_train)
print("standardize(X), test accuracy: {round(m2.score(s2.X_test, s2.y_test), 4)}")
print("coef: {round(m2.coef, 4)}, intercept: {round(m2.intercept, 4)}")

mu = mean(X)
sigma = std(X)
boundary_raw = -m.intercept / m.coef[0]
boundary_scaled_in_hz = mu + (-m2.intercept / m2.coef[0]) * sigma
print("unscaled decision boundary: {round(boundary_raw,2)} Hz")
print("standardized decision boundary, mapped back: {round(boundary_scaled_in_hz,2)} Hz")
```

```
standardize(X), test accuracy: 1
coef: [4.5811], intercept: 0.0245
unscaled decision boundary: 1.94 Hz
standardized decision boundary, mapped back: 175.38 Hz
```

Perfect, on held-out data, once every feature is `(x - mean) / std` before
fitting. The two boundaries make the failure visible rather than abstract:
the standardized model's boundary, mapped back into hertz, lands at 175
Hz — exactly the midpoint of 100 and 250 — while the unscaled model's
boundary sits at 1.94 Hz, nowhere near any data the model was ever shown.

```qu
class0 = X[0:per_class-1, 0]
class1 = X[per_class:per_class*2-1, 0]
jitter0 = 0.05 * randn(per_class, seed = 100)
jitter1 = 1 + 0.05 * randn(per_class, seed = 200)
figure()
scatter(class0, jitter0, color = "#0f172a", label = "class 0 (100 Hz tone)")
scatter(class1, jitter1, color = "#e11d48", label = "class 1 (250 Hz tone)")
plot([boundary_raw, boundary_raw], [-0.3, 1.3], color = "#94a3b8", width = 1.5, label = "unscaled boundary")
plot([boundary_scaled_in_hz, boundary_scaled_in_hz], [-0.3, 1.3], color = "#228833", width = 1.5, label = "standardized boundary")
xlabel("peak frequency (Hz)")
ylabel("class (jittered for display)")
title("Same feature space, two decision boundaries")
legend()
```

The unscaled boundary sits off the left edge of the data entirely — a
model that, in effect, never separated anything, despite 47% looking like
a number a broken system could still produce by chance. The standardized
boundary sits cleanly between the two clusters, precisely where a person
looking at the scatter would draw it by hand.

Both fits used exactly the same model, `logistic_model`, on exactly the
same fifteen-word difference in preprocessing. The lesson is not that
scaling is a checkbox: it is that "the classes are obviously different" and
"the optimizer can find that difference" are separate claims, and only
testing the fit — not eyeballing the data — tells them apart.

### What was assumed away

`Xs` and every array before it lived as ordinary 64-bit floating point
numbers, computed by an FFT and a sigmoid that assumed as many bits and as
much power as anyone likes to spend on each one. That assumption is free on
a laptop and is not free on a hearing-aid chip, a satellite, or a
battery-powered vibration logger running for a year on one cell. Lesson 22
removes it, and asks what happens to a filter — or a classifier's
arithmetic — once it has to run in fixed point.
