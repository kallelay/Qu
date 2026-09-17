# Book 3 — Specialized Domains

Assumes Books 1 and 2. Everything here runs. Where a capability has a
boundary, the boundary is stated where you would hit it, rather than a
section describing something that does not exist yet.

## 1. Machine learning

Every model in Qu is built by a `*_model` constructor and answers the same
three questions: what did you fit, what do you predict, how well did you do.

```qu
X = [1, 2, 3, 4, 5, 6, 7, 8] as matrix(4, 2)
y = [3.1, 5.2, 7.1, 9.3]

m = ols_model(X, y)          # ordinary least squares, automatic intercept
print("coefficients {m.coef}")
print("intercept    {m.intercept:.4f}")
print("R^2          {m.score(X, y):.4f}")
```

`.coef` and `.intercept` are read-only fields, not method calls. `.predict`
and `.score` take new data. The same shape holds across the family:

```qu
r = ridge_model(X, y, 0.1)           # least squares with L2 regularization
k = kmeans_model(X, 2, seed = 42)    # .predict assigns to the nearest centroid
p = pca_model(X, 1)                  # .predict projects onto the fitted components
print("cluster of row 0: {k.predict(X)[0]}")
```

The plain `ridge`, `kmeans`, `pca` builtins are still there and are the
simpler choice when you do not need `.predict` on new data later. The
`_model` versions exist for when you do.

### Pipelines

A pipeline composes named stages into one fit/predict/score object. Every
stage but the last is a plain transform; the last is an estimator
`(X, y) -> model`. Stages are named by string, the same convention `pmap`
and `spawn` use, because a stage has to be re-runnable inside the pipeline.

```qu
centre(X) := X - mean(X)
final(X, y) := ridge_model(X, y, 0.1)

pipe = pipeline("centre", "final")
fitted = pipe.fit(X, y)
print("pipeline R^2 {fitted.score(X, y):.4f}")
```

### Neural networks

Layers are specs, a network is a list of them, and training returns a new
fitted model rather than mutating the one you built — the same "return a
new value" convention as everything else in the language.

```qu
X = [0, 0, 1, 1, 0, 1, 0, 1] as matrix(4, 2)
Y = [0, 1, 1, 0] as matrix(4, 1)

layers = (dense_layer(2, 8, activation = "relu"), dense_layer(8, 1))
net = sequential(layers, seed = 42)
trained = net.fit(X, Y, epochs = 2000, lr = 0.5, loss = "mse")

print("loss {trained.loss_history[0]:.4f} -> {trained.loss_history[1999]:.6f}")
print("XOR predictions (want 0, 1, 1, 0): {trained.predict(X)}")
```

`mlp_classifier`, `simple_cnn` and `simple_rnn_classifier` build the common
architectures in one call. `conv1d`/`conv2d`, `lstm_cell`, `gru_cell`,
`multi_head_attention`, `transformer_block`, `layer_norm` and `dropout` are
the pieces to assemble something else from. Activations (`relu`, `gelu`,
`tanh`, `sigmoid`, `softmax`) are ordinary elementwise functions and work
outside a network too.

## 2. Estimation: Kalman filtering

The whole estimation family — linear Kalman, extended and unscented Kalman,
particle filters — is reached through three shared verbs rather than a
separate name per filter: `predict`, `update`, `estimate`. Each returns a
new state rather than mutating one, so a script threads it through:

```qu
state = kalman_init([0], [1])       # initial estimate x0, covariance P0
state = state.predict([1], [0])     # time update: F, Q
state = state.update([1], [2], [1]) # measurement update: H, z, R
print("estimate {state.x}, covariance {state.P}")
```

The equations are the standard ones with no shortcuts: `x' = F*x`,
`P' = F*P*F' + Q` for the time update; innovation `y = z - H*x`, gain
`K = P*H'*(H*P*H'+R)^-1`, corrected `x' = x + K*y`, `P' = (I-K*H)*P` for the
measurement update. A tracking loop reads as one:

```qu,ignore
for i = 0 to n_steps - 1
    state = state.predict(F, Q)
    state = state.update(H, measurements[i], R)
end for
```

`predict` and `update` dispatch on the state's own `.kind`, and within a
Kalman state on whether the second argument is a matrix or a function name —
so the same two verbs give you the extended filter
(`state.predict("f", Q, jac = "J")`) and the unscented one
(`method = "ukf"`). `particle_filter_init` starts a particle filter and
`state.estimate()` reads the weighted mean back out.

**Boundary:** there is no control input (`B*u` in the time update), so every
system here is driftless or purely measurement-driven.

## 3. Impedance spectroscopy: regularized linear Kramers-Kronig

A port of Kallel & Kanoun, *Regularized linear Kramers-Kronig transform for
consistency check of noisy impedance spectra with logarithmic frequency
distribution*, IWIS 2021.

Boukamp's Lin-KK test checks whether a measured impedance spectrum `Z(f)` is
causal, linear, stable and finite by fitting a Distribution-of-Relaxation-
Times model and inspecting the residual. rLKK adds Tikhonov regularization
on the DRT, so the fit tolerates noisy or truncated data without assuming
any particular equivalent circuit.

```qu
Z = [100 - 20i, 80 - 45i, 50 - 55i, 30 - 35i, 22 - 12i]   # measured impedance
freqs = [0.1, 1, 10, 100, 1000]                          # Hz

fit = rlkk_reconstruct(Z, freqs, lambda = 1e-4) # fit.z, fit.gamma
v = rlkk_validate(Z, freqs, lambda = 1e-3, threshold = 2.0)
if v.valid
    print("consistent within {v.max_residual:.2f}%")
else
    print("bad points: {where(abs(v.residuals) > 2.0)}")
end if
```

`rlkk_reconstruct(Z, freqs, [lambda=1e-4], [fx=])` returns the reconstructed
spectrum in `.z` and the fitted DRT coefficients in `.gamma`. `fx` overrides
the default DRT frequency grid (`logspace(-8, 8, 160)` Hz — deliberately far
broader than any realistic measurement band). A larger `lambda` tolerates
noisier data at the cost of fitting detail; there is no separate
"aggressive" variant, because passing a larger `lambda`, or a
per-point-derived one such as `lambda = 1e10 ./ abs(Z)`, is the same thing.

`rlkk_validate` is `valid` only when every point's percent residual
`(|Z_recon| - |Z|) / |Z_recon| * 100` is under `threshold`. `.residuals` and
`.max_residual` come back too, so "did it pass" and "which points are bad"
are one call rather than two.

`rlkk_extrapolate(Z, freqs, target_freqs, [lambda=1e-4])` refits and
re-evaluates at `target_freqs` — to extend a truncated measurement past its
measured band, or to move onto a different grid.

`nyquist(Z)` draws the standard EIS view (`Re(Z)` against `-Im(Z)`, the sign
convention the instruments themselves use). `bode_magnitude` and
`bode_phase` are the two halves of a Bode plot, kept as separate calls so a
script keeps control of its own layout:

```qu
panel(2, 1, 1)
bode_magnitude(Z, freqs)
panel(2, 1, 2)
bode_phase(Z, freqs)
```

**Boundary:** no equivalent-circuit fitting (R/L/C/CPE/Warburg elements,
global and local optimization, fixed parameters) and no model search across
candidate circuits. rLKK validates and reconstructs without assuming a
topology at all, which is a different tool from circuit fitting rather than
a lesser one.

## 4. Timers, concurrency and shared state

```qu
count = 0
every 1 s do
    count = count + 1
end
run_for(5 s)
print("fired {count} times")
```

Timers run on virtual time, so a five-second test takes no time at all.

Real background work and share-nothing parallel loops:

```qu
heavy(n) := n * n

w = spawn("heavy", 7)
print("spawned result {join(w)}")

results = zeros(10)
parallel for i in 0 to 9
    results[i] = heavy(i)
end parallel
print("parallel results {results}")
```

Each iteration of a `parallel for` gets an isolated environment and a fresh
RNG stream. There is no shared mutable state, so there are no races, by
construction rather than by discipline.

`pmap` is the same isolation as an expression, for the common "apply this
pure function to every sample" shape, and composes with `|>`:

```qu
extract(x) := x * x
out = (0 to 9) |> pmap("extract")
print(out)
```

Only a function *you* defined can be `pmap`'d or `spawn`'d, not a builtin —
it has to run inside the worker's own environment.

When you genuinely need shared state — a counter, a bounded resource — you
opt in:

```qu
counter = mutex(0)
parallel for i in 1 to 100
    mutex_add(counter, 1)
end parallel
print("total {mutex_get(counter)}")
```

`semaphore`/`semaphore_acquire`/`semaphore_release` bound a resource;
`channel_send`/`channel_recv` pass values between workers; `queue`, `fifo`
and `double_buffer` are the buffering structures.

## 5. Serial ports

Real UART/RS-232/USB-serial I/O, through the OS.

```qu,ignore
ports = serial_ports()
print("found {length(ports)} port(s): {ports}")

conn = serial_open(ports[0], 115200, timeout_ms = 1000)
write(conn, "MEAS?\n")
line = read_line(conn) ?? "(timed out)"
print(line)
close(conn)
```

`serial_open(port, baud, [data_bits=8, stop_bits=1, parity="none",
timeout_ms=1000])` opens the port and errors clearly, naming it, when it
does not exist or the OS refuses it. Every blocking read is bounded by
`timeout_ms`, and a read that times out with nothing collected returns
`none` rather than blocking forever or claiming end-of-file — the two are
different, and a device that is simply slow must not look like a device that
has hung up. `?? default` is how you handle it in one line.

`read_line`, `read_until`, `read_byte`, `read_bytes`, `peek_line` and
`peek_byte` are the read surface; `write`, `write_line`, `write_byte` the
write side.

### A hard platform boundary: the browser has no serial or filesystem

`wasm32-unknown-unknown`, Qu's browser target, has no native serial port or
filesystem access. That is a browser sandbox, not a Qu limitation:

- **Serial** is reachable only through the Web Serial API — JavaScript only,
  Chromium only, HTTPS only, and gated behind a user gesture to pick the
  device.
- **Files** have no filesystem in this target at all. Browser file access
  means the File System Access API (Chromium, permission-gated) or a
  one-shot `<input type=file>` read.

So `serial_open` and the file builtins are native-CLI-first. A browser
Studio needs an explicitly designed JavaScript bridge for each, not
something that falls out of the shared Rust core the way FFT and matrix
maths do.

## 6. Where to look next

- **[Builtin index](../stdlib/builtin-index.md)** — every name the engine
  answers to, generated from its own table. If a name is not there, the
  engine does not have it.
- **`catalog/`** — around a hundred complete programs. Read one near your
  own work and change it.
- **[The specification](../language-reference/spec.md)** — normative, and
  ahead of the engine in places. It is the design document, so it describes
  things that are not built; the builtin index is the authority on what is.
