# Quickstart: A Guided Tour

This chapter is the on-ramp: install Qu, run your first script, then one
short, runnable example per domain — syntax, matrices, plotting, signal
processing, data, machine learning, concurrency. Every snippet below has
been run against the reference interpreter and its printed output is the
actual output, not a mockup. For real depth on any one of these topics,
this chapter hands off to **[Book 1](book1-fundamentals.md)**,
**[Book 2](book2-numerics-dsp.md)**, and **[Book 3](book3-specialized.md)**,
plus the [Standard Library Reference](../stdlib/overview.md) and the
[language specification](../language-reference/spec.md) — this walkthrough
deliberately stays shallow so it stays fast.

## 1. Install and run

Qu ships today as **source you build yourself** — there is no packaged
binary or installer yet. You need a working Rust toolchain
([rustup.rs](https://rustup.rs)) and a clone of this repository:

```bash
git clone <this repository>
cd Qu
cargo build --manifest-path engine/Cargo.toml -p qu-cli
```

That builds a `qu` binary (`engine/target/debug/qu`, or use
`cargo run` as shown below to build-and-run in one step). Two ways to use
it:

**Run a script.** Save this as `hello.qu`:

```qu
print("Hello, Qu!")
```

```bash
cargo run --manifest-path engine/Cargo.toml -p qu-cli -- run hello.qu
```
```
Hello, Qu!
```

**Or start the REPL** for interactive, line-at-a-time exploration:

```bash
cargo run --manifest-path engine/Cargo.toml -p qu-cli -- repl
```
```
qu> print(1 + 1)
2
qu>
```

A line that doesn't end in `;` echoes its result automatically
(`ans = ...` for a bare expression). `:vars` lists everything currently
bound, `:clear` resets the session, `:quit`/`:exit` (or Ctrl+D) leaves —
type `:help` inside the REPL for the full list. The rest of this chapter
uses `qu run <file>` for every example; every one of them works pasted into
the REPL too.

### `qu run` flags for running untrusted or unattended scripts

`qu run <file.qu>` takes a few flags for running a script you don't fully
trust or don't want to babysit — useful for a script from someone else, a
CI job, or anything you want a hard ceiling on:

- **`--max-time <seconds>`** and **`--max-memory <MB>`** — hard resource
  caps. Exceeding either one kills the process outright (not a graceful
  stop — the script's own output up to that point is discarded, same as
  any other hard timeout/OOM kill) with a message naming which limit was
  hit and the actual measured value:
  ```
  qu: killed: exceeded --max-time 30s (ran 30.4s)
  qu: killed: exceeded --max-memory 512MB (used ~520MB)
  ```
  Enforcement is a background thread polling wall-clock time and process
  memory every 25ms, not a hard allocator-level ceiling or a cooperative
  check inside the interpreter's own loop — chosen specifically because it
  also catches a single slow *native* operation (a huge matrix op, say),
  not just a runaway Qu-level loop. That polling interval is the real
  precision bound: a run can overshoot either limit by up to ~25ms (and,
  for memory, by however much one single large allocation adds between two
  polls) before getting killed.
- **`--sandbox`** — denies a fixed list of capability-bearing builtins at
  call time instead of running them: `http_get`, the `tcp_*` family,
  `listen_pool`, `python_exec`, and file-*write* operations specifically
  (`write_csv`, `touch`, `fopen`/`StreamFile` opened in a write mode).
  File *reads* stay allowed — a sandboxed script can still load input data
  with `read_csv`/`fopen(path, "r")`/etc., it just can't reach the
  network, spawn external processes, or write to disk. A denied call fails
  with a clear error (`qu: ... sandbox: 'http_get' is disabled in
  sandboxed execution`) rather than silently doing nothing.
  **This is a capability restriction, not a full security sandbox** — it
  does nothing about CPU/memory exhaustion on its own. Pair it with
  `--max-time`/`--max-memory` for that: `--sandbox --max-time 10
  --max-memory 256` together is the realistic "run this untrusted script"
  recipe; neither flag alone is a complete answer.
- **`--profile`** (optionally with `--profile-output <path>`) — reports,
  to stderr by default or to the given file, per-function call count/total
  wall time/time% (aggregated across every call to that function name,
  sorted hottest-first) plus the run's peak resident memory:
  ```
  qu profile
    wall-clock time : 0.842s
    peak RSS        : 61 MB

    function                       calls   total time   time %
    train_epoch                       50      0.701s     83.3%
    forward                          500      0.098s     11.6%
    load_batch                        50      0.015s      1.8%
  ```

### Sandbox/profiling as part of the program, not just CLI flags

`--sandbox`/`--profile` above apply to the whole process, set once before
the script starts. `sandbox_mode(on)`, `profiling_mode(on)`, and
`profile_stats()` are the same two underlying controls exposed as
builtins, so a script can turn either on for just the section it cares
about instead of all-or-nothing for the whole run:

```qu
function load_and_process(path)
    profiling_mode(true)
    data = read_csv(path, headers=true)
    result = data.sort_by("value")
    profiling_mode(false)
    print(profile_stats())     # a real Table: function / calls / seconds

    sandbox_mode(true)
    try
        touch("audit.log")     # denied -- sandboxed for this section only
    catch err
        print("blocked: {err.message}")
    end
    sandbox_mode(false)

    return result
end function
```

`profile_stats()` returns a `Table` the script decides what to do with —
print it, filter it, save it — rather than the CLI's external report
string. `--max-time`/`--max-memory` have no in-script equivalent yet:
they're enforced by an external OS-level watchdog thread in `qu-cli`
started before the interpreter runs at all, so an in-script
`max_time(...)`/`max_memory(...)` would need the watchdog to poll a value
the interpreter can update mid-run rather than a fixed constant set at
startup — a real design question, not yet built (see `BACKLOG.md`).

## 2. Core syntax in one script

```qu
x = 5
y = 2.5
print("x + y = {x + y}")

for k = 1 to 5
    print("k = {k}")
end for

n = 0
total = 0
while n < 5
    total = total + n
    n = n + 1
end while
print("total = {total}")

if total > 5 then
    print("total is big")
else
    print("total is small")
end if

square(v) := v ^ 2
print("square(6) = {square(6)}")

function greet(who)
    return "hello, " + who
end function
print(greet("Qu"))

r = 0 to 10 step 2
print("r = {r}")
```
```
x + y = 7.5
k = 1
k = 2
k = 3
k = 4
k = 5
total = 10
total is big
square(6) = 36
hello, Qu
r = [0, 2, 4, 6, 8, 10]
```

Notes on what just happened:

- Variables are implicitly typed — no declarations. `for`/`while`/`if` all
  close with an explicit `end` (or `end for`/`end while`/`end if`).
- `:=` defines a one-line, non-recursive, *elemental* function (call it on a
  vector and it maps automatically); `function ... end function` is the
  multi-statement form with explicit `return`.
- `start to stop [step delta]` is a range, **inclusive at both ends** —
  unlike Python's `range`.
- `"{expr}"` interpolates directly in a double-quoted string, with an
  optional `:spec` (`.4f`, `.2e`, `d`, ...) for formatting.

Book 1 goes much deeper here: `type explicit` mode, unit literals
(`5 kHz`), complex numbers, logical/mask indexing, and `save`/`load`.

## 3. Vectors and matrices

```qu
a = [1, 2, 3]
b = [10, 20, 30]
print("a + b = {a + b}")
print("a .* b = {a .* b}")

# zero-based indexing -- a MATLAB user's #1 surprise
print("a[0] = {a[0]}")

M = [1, 2; 3, 4]
print("M = {M}")
print("M[0,1] = {M[0,1]}")
print("M*M = {M*M}")

# solve A*x = b for x, using left division
A = [4, 7; 2, 6]
rhs = [1; 1]
x = A \ rhs
print("x = {x}")
print("A*x = {A * x}")
```
```
a + b = [11, 22, 33]
a .* b = [10, 40, 90]
a[0] = 1
M = [1, 2; 3, 4]
M[0,1] = 2
M*M = [7, 10; 15, 22]
x = [-0.1; 0.2]
A*x = [1; 1]
```

**The one fact worth memorizing before anything else**: `A[0,1]` reads row
0, column 1 — zero-based, like Python/NumPy, **not** one-based like MATLAB.
The other MATLAB-familiar convention that *does* carry over: bare `*` is
matrix multiplication at a 2-D boundary (not elementwise — use `.*` for
that), and `A \ b` solves `A*x = b` exactly like MATLAB's own backslash.
Book 1 §6–7 covers orientation, transpose spellings, and logical indexing;
Book 2 §2 covers the rest of the linear-algebra floor (`inv`, `det`, `svd`,
`qr`, `lu`, `eig`, `chol`).

## 4. Plotting

```qu
x = 0 to 10 step 0.1
y = sin(x)
plot(x, y)
xlabel("x")
ylabel("sin(x)")
title("First plot")
grid on
savefig("quickstart_plot.svg")
print("saved quickstart_plot.svg")
```
```
saved quickstart_plot.svg
```

`savefig` picks the output format from the file extension. `.svg` and
`.pdf` are both real, implemented vector backends today (`.html` embeds the
same SVG in a minimal page, `.tikz`/`.tex` emits LaTeX source) — they share
one draw-command list, so they never visually disagree with each other.
Full-figure `.png` is the one format **not** implemented yet (it needs a
real rasterizer); raster `.png` export for actual image data
(`save_image`) is a separate, already-working feature. See
[Plotting](../stdlib/plotting.md) for the full chart-type list
(`stem`/`scatter`/`bar`/`hist`/`heatmap`/`spectrogram`/... ) and subplot
layout.

## 5. Signal processing: filtering a noisy signal

```qu
fs = 1000
n = 0 to 999
t = n / fs
clean = sin(2 * pi * 5 * t)
noisy = clean + sin(2 * pi * 120 * t)

lp = butter(4, "low", 30, fs)
filtered = filtfilt(lp, noisy)

err_noisy = rms(noisy - clean)
err_filtered = rms(filtered - clean)
print("rms error before filtering = {err_noisy:.4f}")
print("rms error after filtering  = {err_filtered:.4f}")
```
```
rms error before filtering = 0.7071
rms error after filtering  = 0.0266
```

`butter(order, kind, cutoff, fs)` designs a digital Butterworth filter as a
cascade of second-order sections; `filtfilt` runs it forward then backward
for zero phase distortion (the right choice for offline analysis).
`sosfilt` is the single-pass, causal alternative, and `filter_init`/
`filter_next` thread filter state one sample at a time for streaming use.
See Book 2 §1 for FFT, spectrograms, EMD/VMD, and the rest of the DSP
toolbox.

## 6. Working with data

Save this as `sensors.csv`:

```csv
temp,humidity,pressure,label
21.5,45.2,1012.1,ok
22.1,46.0,1011.8,ok
35.0,20.1,1009.5,warn
21.8,45.5,1012.0,ok
36.2,19.5,1008.9,warn
22.0,45.8,1012.3,ok
34.5,21.0,1009.8,warn
21.6,45.1,1012.2,ok
35.8,20.4,1009.1,warn
22.3,46.2,1011.9,ok
```

The block below writes that same file before reading it, so the page you
are looking at really did run this. Without it the example opens a file
the build does not have, fails, and is dropped from the page — leaving a
quickstart whose output a newcomer cannot see.

```qu
write_csv(table(
    temp     = [21.5, 22.1, 35.0, 21.8, 36.2, 22.0, 34.5, 21.6, 35.8, 22.3],
    humidity = [45.2, 46.0, 20.1, 45.5, 19.5, 45.8, 21.0, 45.1, 20.4, 46.2],
    pressure = [1012.1, 1011.8, 1009.5, 1012.0, 1008.9, 1012.3, 1009.8, 1012.2, 1009.1, 1011.9],
    label    = ["ok", "ok", "warn", "ok", "warn", "ok", "warn", "ok", "warn", "ok"]
), "sensors.csv")

df = read_csv("sensors.csv")
print("rows = {nrow(df)}, cols = {ncol(df)}")
print(head(df, 3))

numeric = drop(df, "label")
c = corrmat(numeric)
print(c.matrix)
c.plot()
savefig("quickstart_corrmat.svg")

scaled_temp = standardize(df.temp)
print("standardized temp[0] = {scaled_temp[0]:.4f}")
print("mean(standardized) = {mean(scaled_temp):.4f}, std(standardized) = {std(scaled_temp):.4f}")
```
```
rows = 10, cols = 4
table(3 rows x 4 cols: temp, humidity, pressure, label)
[1, -0.997357, -0.991317; -0.997357, 1, 0.983511; -0.991317, 0.983511, 1]
standardized temp[0] = -0.8275
mean(standardized) = -0.0000, std(standardized) = 1.0000
```

A `Table` prints as a compact one-line summary rather than a full grid —
use `print(select(df, "temp", "humidity"))` for a subset, or
`print(describe(df))` for pandas-style summary statistics, when you want
to actually see values. `drop(df, "col", ...)` returns a new table with
those columns removed (Qu tables are immutable, like everything else);
`corrmat(df)` returns a `model(...)` with a `.matrix` field, and doubles as
a plottable heatmap via `.plot()`. `standardize`/`normalize`/
`robust_scale`/`quantile_normalize` are one-shot scalers; `fit_scaler(X,
method=...)` plus `.transform`/`.inverse_transform` is the fit-once/
reuse-on-new-data version for a real train/test split. See
[Collections, Strings & Data Frames](../stdlib/collections-strings.md) and
[File I/O](../stdlib/file-io.md) for the rest of the `Table` API.

## 7. Machine learning: train, predict, check accuracy

```qu
n_per_class = 60
c0x = normal(0, 0.6, n_per_class, seed=1)
c0y = normal(0, 0.6, n_per_class, seed=2)
c1x = normal(4, 0.6, n_per_class, seed=3)
c1y = normal(4, 0.6, n_per_class, seed=4)

Xc0 = hstack(reshape(c0x, n_per_class, 1), reshape(c0y, n_per_class, 1))
Xc1 = hstack(reshape(c1x, n_per_class, 1), reshape(c1y, n_per_class, 1))
Xc = vstack(Xc0, Xc1)             # (120, 2)

n = 2 * n_per_class
idx = 0 to n - 1
y = floor(idx / n_per_class)      # 0 for the first 60 rows, 1 for the last 60

function one_hot(labels, rows, k)
    m = zeros(rows, k)
    for i = 0 to rows - 1
        m[i, labels[i]] = 1
    end for
    return m
end function

Y = one_hot(y, n, 2)

net = mlp_classifier(2, [8], 2, seed=42)
trained = net.fit(Xc, Y, epochs=200, lr=0.1, loss="cross_entropy")

function accuracy(model, X, Ytrue, rows)
    logits = model.predict(X)
    correct = 0
    for i = 0 to rows - 1
        if argmax(logits[i, :]) == argmax(Ytrue[i, :]) then
            correct = correct + 1
        end if
    end for
    return correct / rows
end function

acc = accuracy(trained, Xc, Y, n)
print("training accuracy = {acc:.4f}")
```
```
training accuracy = 1.0000
```

`mlp_classifier(in_dim, hidden_dims, n_classes, [seed=])` builds a
dense+ReLU stack ending in raw ("softmax-ready") logits; `.fit(X, Y,
epochs=, lr=, loss="cross_entropy")` trains it with one-hot `Y`, and
`.predict(X)` runs inference (dropout off, untracked). Two classes drawn
from well-separated Gaussian blobs is an easy problem — 100% training
accuracy just confirms the whole pipeline actually works end to end, not
that the model is magic. For classical (non-neural) ML with the same
`fit`/`predict`/`score` protocol, see `kmeans_model`, `ols_model`,
`tree_model`, `knn_model`, and the rest of
[Statistics & Machine Learning](../stdlib/statistics-ml.md); Book 3 §2
covers the model-zoo and pipeline story in more depth.

## 8. Concurrency: spawn and join

```qu
heavy(n) := n * n

w = spawn("heavy", 7)
result = join(w)
print("result = {result}")
```
```
result = 49
```

`spawn("fn_name", args...)` runs a user-defined function on its own OS
thread (only a function you defined can be spawned, never a builtin);
`join(w)` blocks for its result. `parallel for` and `pmap` are the
shared-nothing loop/pipeline-stage equivalents for "apply this to every
element"; `mutex`/`semaphore` opt in to real shared state when you need it.
See Book 3 §4 and §5 for timers (`every`/`after`), `parallel for`, and
real serial-port I/O.

## Where to go from here

- **[Book 1 — Fundamentals](book1-fundamentals.md)**, **[Book 2 —
  Numerics, DSP & Linear Algebra](book2-numerics-dsp.md)**, and **[Book 3 —
  Specialized Domains](book3-specialized.md)** go deep on everything this
  chapter only sampled, in that order.
- The **[Standard Library Reference](../stdlib/overview.md)** is the
  function-by-function index, organized by domain, generated straight from
  the interpreter's own builtin dispatch table.
- **[The Specification](../language-reference/spec.md)** is the source of
  truth for anything this book and the spec ever disagree on.
