# From Zero

A complete first session, start to finish. Every block on this page runs
as written — nothing here reads a file you do not have, and the data is
made on the spot. Paste them into one file in order, or type them into
`qu repl` a line at a time.

By the end you will have generated a signal, measured it, fitted a model
to it, and produced a figure fit to put in a paper.

## 1. Install and run

```
cargo build --release --manifest-path engine/Cargo.toml
engine/target/release/qu repl
```

`qu run script.qu` runs a file. `qu repl` gives you a prompt. Everything
below works in either.

## 2. Numbers, vectors, and printing

Qu has no declarations. A name gets a type by being assigned one.

```qu
fs = 1000               # a plain number
t  = (0 to 999) / fs    # a range, divided elementwise -> a vector
print("{len(t)} samples covering {max(t):.3f} s")
```

`0 to 999` is a range, inclusive at both ends. Dividing it by a number
divides every element. `{...}` inside a string interpolates, and `:.3f`
formats — the same spelling as most languages you already know.

Arithmetic on whole vectors is the normal case, not a special one:

```qu
x = sin(2 * pi * 50 * t)          # 50 Hz, one value per sample
y = x + 0.2 * randn(1000, seed = 1)   # the same signal, with noise on it
print("clean rms {rms(x):.4f}, noisy rms {rms(y):.4f}")
```

`seed =` makes the noise reproducible. Every random function in Qu takes
it, and a script that reports numbers should use it.

## 3. Looking at a signal

The spectrum of a real signal comes from `rfft`, which returns only the
non-negative frequencies — the half that carries all the information.

```qu
spectrum = abs(rfft(y))
freqs    = (0 to len(spectrum) - 1) * fs / len(y)
peak     = argmax(spectrum)
print("largest component at {freqs[peak]:.1f} Hz")
```

That should say 50 Hz. If it says something else, the signal is not what
you think it is — which is the first thing worth knowing.

## 4. Filtering

Filters are designed once and applied as many times as you like:

```qu
lp       = butter(4, "low", 120, fs)          # 4th order, cut at 120 Hz
smoothed = filtfilt(lp, y)                    # zero-phase, forward and back
print("noise removed: rms {rms(y - x):.4f} -> {rms(smoothed - x):.4f}")
```

`butter(order, kind, cutoff, fs)` takes the cutoff in hertz and the
sample rate alongside it, rather than as a fraction of Nyquist you have to
work out first. `filtfilt` runs the filter forwards and backwards, so the output
has no phase shift — worth it when you are going to compare against the
original in time.

## 5. Fitting a model

Suppose the thing you actually want is a decay constant. Make some data
with a known answer, then recover it:

```qu
tau   = 0.35
decay = 2.5 * exp(0 - t / tau) + 0.02 * randn(1000, seed = 2)

function resid(p)
    return p[0] * exp(0 - t / p[1]) - decay
end function

fit = least_squares("resid", [1.0, 1.0])
print("amplitude {fit.params[0]:.4f} (true 2.5)")
print("tau       {fit.params[1]:.4f} (true {tau})")
```

`least_squares` takes the NAME of a function that returns residuals — the
vector you want driven to zero. That is more flexible than fitting a
model to data: a weighting, a penalty, or a regularisation term is just
more entries in what `resid` returns.

If a parameter has a physical floor, say so and it will be respected:

```qu
bounded = least_squares("resid", [1.0, 1.0], lower = [0.0, 0.01])
print("bounded tau {bounded.params[1]:.4f}")
```

## 6. A figure you can publish

```qu
theme("publication")
figure_size(3.5, 2.4, unit = "in")

plot(t[0:400], y[0:400], color = "#b0b0b0", lw = pt(0.6), label = "measured")
plot(t[0:400], smoothed[0:400], color = "#0072BD", lw = pt(1.2), label = "filtered")
xlabel("time $t$ [s]")
ylabel("amplitude")
title("A 50 Hz tone, low-pass filtered")
legend()
savefig("first-figure.pdf")
```

`theme("publication")` switches to print styling — heavier type, thinner
grid, a palette that survives greyscale. `pt(...)` gives a width in
points, so a line is the weight you asked for on the printed page rather
than a number that means something different at every figure size.

Maths in labels is written the way you would write it in LaTeX. `$t$`
sets an italic *t*, because a variable is italic and Qu knows the
difference between a variable and a word:

```qu
xlabel("$\eta_{\mathrm{exc}}$, the excitation-dependent fraction")
```

The PDF carries its fonts with it, so the figure looks the same on a
machine that has never heard of Latin Modern.

## 7. Functions and scope

```qu
function normalize(v)
    m = mean(v)
    s = std(v)
    return (v - m) / s
end function

z = normalize(y)
print("normalized: mean {mean(z):.2e}, std {std(z):.4f}")
```

A function's assignments are its own. `m` and `s` above do not touch
anything called `m` or `s` outside it, even though reading an outer name
works fine:

```qu
scale = 100

function scaled(v)
    return v * scale        # reads the outer `scale`
end function

print("scaled max {max(scaled([0.1, 0.5])):.1f}")
```

When you do want to write to something outside, say so:

```qu
calls = 0

function counted(v)
    global calls
    calls = calls + 1
    return sum(v)
end function

counted([1, 2])
counted([3, 4])
print("called {calls} times")
```

## 8. Tables

Columns have names, and the names come from how you build it:

```qu
runs = table(
    channel  = [1, 2, 3],
    baseline = [0.412, 0.508, 0.377],
    treated  = [0.240, 0.301, 0.219]
)
runs = insert_column(runs, "reduction", 1 - runs.treated / runs.baseline)
print(runs)
```

A column is read with `runs.crest_opt` and added with `insert_column`,
which returns a new table rather than modifying the one you had.

`write_csv(runs, "runs.csv")` writes it out; `read_csv("runs.csv")` reads
one back, including the `#` provenance headers instruments like to emit
(`read_csv(path, comment = "#")`).

## 9. Where to go next

- **[Book 1 — Fundamentals](book1-fundamentals.md)** for the language in
  depth: types, contracts, control flow, error handling.
- **[Book 2 — Numerics, DSP & Linear Algebra](book2-numerics-dsp.md)** for
  transforms, filter design, and dense linear algebra including the
  complex case.
- **[Standard Library Reference](../stdlib/overview.md)** for the full set
  of functions, grouped by what they are for.
- **[Builtin Index](../stdlib/builtin-index.md)** if you know the name and
  want the signature.
- `catalog/` in the repository holds around a hundred complete programs,
  each one runnable as it stands.
