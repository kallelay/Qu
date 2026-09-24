# Builtin Index

Every function the engine will answer to, with a line saying what it
does and where it is documented properly.

The names come from the interpreter's own dispatch table, so this page
cannot claim a function the engine lacks or omit one it has. The
descriptions come from the chapters, joined on the name -- so a
description is written in exactly one place, the chapter that teaches
the subject, and this page is a view onto it rather than a second copy
that can drift.

**1093 builtins, 1005 described.** The rest are
listed at the end, by name: a gap you can see is worth more than
one quietly omitted.

`apropos("filter")` searches these names from inside a running
program, and a misspelled call suggests the nearest match.

## A

| | | |
|---|---|---|
| [`ablation_study`](../fn/ablation_study.html) | Computes one importance score per feature column of an `N`-row, `D`-column `X` and length-`N` `y`, using `cv_folds` folds (integer, default 3): the full model's cross-validated score minus.. | [Statistics & ML](statistics-ml.md) |
| [`abs`](../fn/abs.html) | Absolute value on a real scalar/`Vec`/`Mat`; magnitude (`sqrt(re^2 + im^2)`) on a `Complex` scalar, `CVec`, or `CMat` | [Core maths](core-math.md) |
| [`acf`](../fn/acf.html) | Returns a vector of length `max_lag+1` (lag 0 through `max_lag`), the autocorrelation function | [Signal processing](signal-processing.md) |
| [`acos`](../fn/acos.html) | Inverse trigonometric functions, single-argument only (there is no two-argument `atan2` in this table) | [Core maths](core-math.md) |
| [`acosh`](../fn/acosh.html) | Inverse hyperbolics. `asinh` is defined and finite on the whole real line | [Core maths](core-math.md) |
| [`adadelta`](../fn/adadelta.html) | Per-parameter learning rates derived from the running gradient history, given `params` (a Tensor to optimize) and `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`adagrad`](../fn/adagrad.html) | Per-parameter learning rates derived from the running gradient history, given `params` (a Tensor to optimize) and `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`adam`](../fn/adam.html) | The Adam family of per-parameter adaptive-learning-rate optimizers, given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`adamax`](../fn/adamax.html) | The Adam family of per-parameter adaptive-learning-rate optimizers, given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`adamw`](../fn/adamw.html) | The Adam family of per-parameter adaptive-learning-rate optimizers, given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`adaptive_threshold`](../fn/adaptive_threshold.html) | Locally adaptive, unlike `threshold`/`otsu` above: each pixel's own threshold is `mean(neighborhood) - offset`, where the neighborhood mean is a uniform box average (`method="mean"`) or a.. | [Images](images.md) |
| [`adc`](../fn/adc.html) | Quantises `x` (a length-N number vector) the way a converter does — with a reference range, not just a step size, so a signal outside `[vmin, vmax]` clips instead of being quantised as if.. | [Noise](noise.md) |
| [`add`](../fn/add.html) | The operators as named functions, so an operation can be passed where a name is what you can pass | [Core maths](core-math.md) |
| [`add_edge`](../fn/add_edge.html) | Nodes are named; an edge carries an optional weight, defaulting to 1 | [Collections & strings](collections-strings.md) |
| [`add_marker`](../fn/add_marker.html) | Added in v0.2.4. Annotates an instant — an impact, a fault injection — at `time` seconds on the signal's own (origin-aware) axis | [Signal processing](signal-processing.md) |
| [`add_node`](../fn/add_node.html) | Nodes are named; an edge carries an optional weight, defaulting to 1 | [Collections & strings](collections-strings.md) |
| [`add_noise`](../fn/add_noise.html) | Adds noise of the named `kind` (a string — see the kinds table near the top of this chapter) to `x`, a length-N number vector or an `Image` (noise is added per-pixel, per-channel) | [Noise](noise.md) |
| [`add_region`](../fn/add_region.html) | Added in v0.2.4. Annotates an interval rather than an instant | [Signal processing](signal-processing.md) |
| [`affine_identity`](../fn/affine_identity.html) | Returns the 3x3 identity `Mat` — a no-op transform, useful as a composition base case | [Images](images.md) |
| [`affine_rotate`](../fn/affine_rotate.html) | Returns a new 3x3 `Mat` encoding a CCW rotation matrix in standard math convention (see the clockwise-on-screen note above for the visual difference from `imrotate`) | [Images](images.md) |
| [`affine_scale`](../fn/affine_scale.html) | Builds a scaling transform for `imwarp` | [Images](images.md) |
| [`affine_shear`](../fn/affine_shear.html) | Returns a new 3x3 `Mat` encoding that shear | [Images](images.md) |
| [`affine_translate`](../fn/affine_translate.html) | Returns a new 3x3 homogeneous-coordinate `Mat` encoding that translation | [Images](images.md) |
| [`after`](../fn/after.html) | `after` returns the piece of `s` past the first `mark`; `before` returns the piece up to it | [Collections & strings](collections-strings.md) |
| [`after_last`](../fn/after_last.html) | Returns a `Str`: like `after`/`before` but searching from the end — the pair to reach for on a path or a dotted name | [Collections & strings](collections-strings.md) |
| [`algorigram`](../fn/algorigram.html) | Renders a flowchart of the named function's control flow: `if`/`else` as a decision diamond with `yes`/`no` branches that rejoin, `while`/`for`/`do...loop` as a decision with a routed.. | [Plotting](plotting.md) |
| [`all`](../fn/all.html) | Returns a `bool`: whether every (`all`) or at least one (`any`) element satisfies it | [Collections & strings](collections-strings.md) |
| [`and`](../fn/and.html) | Returns a `bool`. Both short-circuit: the right side is not evaluated when the left already decides the answer, so `i < len(xs) and xs[i] > 0` is a safe guard | [Collections & strings](collections-strings.md) |
| [`angle`](../fn/angle.html) | Complex argument — the angle from the positive real axis, in radians (`atan2(im, re)`) — on a `Complex`/`CVec`/`CMat`; on a signed real scalar or real `Vec`/`Mat` it degrades to `0` for a.. | [Core maths](core-math.md) |
| [`animate`](../fn/animate.html) | Calls function `"fn"` (string) once per frame index from `0` to `frames - 1` (integer) and writes the sequence to `path` (string) as one animated SVG, no JavaScript | [Plotting](plotting.md) |
| [`annotate`](../fn/annotate.html) | Same as `text` — label `text` (string) at `(x, y)` (numbers, data units) — but also draws a marker dot at that point | [Plotting](plotting.md) |
| [`any`](../fn/any.html) | Returns a `bool`: whether every (`all`) or at least one (`any`) element satisfies it | [Collections & strings](collections-strings.md) |
| [`append`](../fn/append.html) | Returns a NEW `Vec`/`List` with `value` added at the end | [Collections & strings](collections-strings.md) |
| [`append_all`](../fn/append_all.html) | Appends `s` to the end of the file at `path`, creating it if absent | [Collections & strings](collections-strings.md) |
| [`append_text`](../fn/append_text.html) | Appends `s` to the end of the file at `path`, creating it if absent | [Collections & strings](collections-strings.md) |
| [`apply`](../fn/apply.html) | Returns the SAME container type and shape as `x` (a `Signal(xs, fs)` comes back as a new `Signal` at the same `fs`, a vector/matrix keeps its own shape, a bare number/boolean applies the.. | [Signal processing](signal-processing.md) |
| [`apply_calibration`](../fn/apply_calibration.html) | Added in v0.2.4. The general linear calibration `physical = slope*raw + offset` that the two `calibrate` forms are specific instances of | [Signal processing](signal-processing.md) |
| [`apply_calibration_curve`](../fn/apply_calibration_curve.html) | Added in v0.2.4. A non-linear calibration: a lookup table from raw reading to physical value, interpolated piecewise-linearly — what a thermocouple or a certificated load cell actually.. | [Signal processing](signal-processing.md) |
| [`apply_gain`](../fn/apply_gain.html) | Added in v0.2.4. Multiplies every sample by a linear gain factor and composes it into the calibration record, so a calibrated signal stays calibrated and its level rises accordingly | [Signal processing](signal-processing.md) |
| [`apply_offset`](../fn/apply_offset.html) | Added in v0.2.4. Adds a constant to every sample (a DC trim, a tare), composed into the calibration record like any other linear step | [Signal processing](signal-processing.md) |
| [`apropos`](../fn/apropos.html) | Returns a `List` of strings: every builtin name containing that substring, in engine-registration order | [REPL & diagnostics](repl-diagnostics.md) |
| [`ar_model`](../fn/ar_model.html) | Fits an autoregressive AR(`p`) model by Yule-Walker to a length-`N` numeric vector `x`, using the integer lag order `order` (this is `p`) | [Statistics & ML](statistics-ml.md) |
| [`arc`](../fn/arc.html) | Draws part of a circle, as an open stroke | [Plotting](plotting.md) |
| [`area`](../fn/area.html) | Filled area under the curve `(x, y)` (equal-length numeric Vecs) down to `y=0`; the zero-baseline special case of `fill_between` | [Plotting](plotting.md) |
| [`arg`](../fn/arg.html) | Complex argument — the angle from the positive real axis, in radians (`atan2(im, re)`) — on a `Complex`/`CVec`/`CMat`; on a signed real scalar or real `Vec`/`Mat` it degrades to `0` for a.. | [Core maths](core-math.md) |
| [`argmax`](../fn/argmax.html) | 0-based index of the first-occurring minimum/maximum element of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened) | [Core maths](core-math.md) |
| [`argmean`](../fn/argmean.html) | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) of the element closest to `mean(x)` (ties broken by first occurrence) — not a true order-statistic index like the others above, since.. | [Core maths](core-math.md) |
| [`argmedian`](../fn/argmedian.html) | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) of the element sitting at the median position | [Core maths](core-math.md) |
| [`argmin`](../fn/argmin.html) | 0-based index of the first-occurring minimum/maximum element of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened) | [Core maths](core-math.md) |
| [`argquantile`](../fn/argquantile.html) | Index into `x` (a `Vec` or flattened `Mat`/`Signal`) nearest the `q`-quantile rank, computed as `round(q*(n-1))` where `n = len(x)`; `q` is a scalar in `[0, 1]` | [Core maths](core-math.md) |
| [`argsort`](../fn/argsort.html) | Returns a `Vec` of `number` (0-based indices): the permutation that would sort `x` — the index-returning sibling of `sort`, same convention as `argmin`/`argmax` | [Collections & strings](collections-strings.md) |
| [`argv`](../fn/argv.html) | Returns a list of strings: the command-line arguments given after a literal `--` in the invocation (e.g. `qu run analyse.qu -- data.csv 1000` gives `("data.csv", "1000")`) | [File I/O](file-io.md) |
| [`arima`](../fn/arima.html) | Fits an ARIMA(`p`, `d`, `q`) model to a length-`N` numeric vector `x`: `d` (integer, default 0) rounds of differencing, then an ARMA fit with `p` (integer, default 1) autoregressive lags.. | [Statistics & ML](statistics-ml.md) |
| [`arrow`](../fn/arrow.html) | Draws an arrow from `(x0, y0)` to `(x1, y1)` (numbers, data units), no label | [Plotting](plotting.md) |
| [`arrowtext`](../fn/arrowtext.html) | Same as `arrow` — from `(x0, y0)` to `(x1, y1)` (numbers, data units) — but the arrowhead ends in a point labeled with `text` (string) | [Plotting](plotting.md) |
| [`as_text`](../fn/as_text.html) | Returns a `List` of `Str` (each element formatted the way `print` would), so `[1, 2, 3].as_text().join(", ")` works | [Collections & strings](collections-strings.md) |
| [`asc`](../fn/asc.html) | Returns a `number`: the code point of its first character | [Collections & strings](collections-strings.md) |
| [`asin`](../fn/asin.html) | Inverse trigonometric functions, single-argument only (there is no two-argument `atan2` in this table) | [Core maths](core-math.md) |
| [`asinh`](../fn/asinh.html) | Inverse hyperbolics. `asinh` is defined and finite on the whole real line | [Core maths](core-math.md) |
| [`astar_mrmr`](../fn/astar_mrmr.html) | A guided (A*) search on an `N`-row, `D`-column `X` and length-`N` `y` for a feature subset of size `n_features` (integer) that is relevant to the target and not redundant with itself.. | [Statistics & ML](statistics-ml.md) |
| [`at`](../fn/at.html) | Returns a `Record`: one row of the table by position, always positional even when the table carries a string index | [Collections & strings](collections-strings.md) |
| [`atan`](../fn/atan.html) | Inverse trigonometric functions, single-argument only (there is no two-argument `atan2` in this table) | [Core maths](core-math.md) |
| [`atanh`](../fn/atanh.html) | Inverse hyperbolics. `asinh` is defined and finite on the whole real line | [Core maths](core-math.md) |
| [`available`](../fn/available.html) | Returns an integer: bytes waiting on the OS's serial input buffer right now, without blocking | [Concurrency](concurrency.md) |
| [`avgpool2d`](../fn/avgpool2d.html) | Downsamples an `(H, W)` matrix `x` the same way as `maxpool2d` — `size` x `size` windows, `stride` defaulting to `size` — but takes the mean of each window instead of the max | [Statistics & ML](statistics-ml.md) |

## B

| | | |
|---|---|---|
| [`band_power`](../fn/band_power.html) | Returns the power in that frequency band by Parseval, normalised by `N^2` so a tone of amplitude `A` reads `A^2/2` | [Signal processing](signal-processing.md) |
| [`band_zero`](../fn/band_zero.html) | Returns a new `spectrum` at the same `Fs`, `N` and scaling with that band cleared — an ideal brick-wall notch | [Signal processing](signal-processing.md) |
| [`bar`](../fn/bar.html) | Bar chart: `x` (Vec of numbers or strings, category positions/labels) and `y` (Vec of numbers, bar heights) must be equal length | [Plotting](plotting.md) |
| [`basin_hopping`](../fn/basin_hopping.html) | Returns a `Model` (kind `"minimize"`, same fields as `minimize`) | [Signal processing](signal-processing.md) |
| [`beamform`](../fn/beamform.html) | Delay-and-sum beamformer. `signals`: a list of `Signal`s, one per array element, all at the same `Fs` and the same length; `positions`: a matching-length vector of element coordinates.. | [Signal processing](signal-processing.md) |
| [`beeswarm`](../fn/beeswarm.html) | Every sample drawn, nudged sideways only as far as it must be to clear its neighbours, so the WIDTH is the count | [Plotting](plotting.md) |
| [`before`](../fn/before.html) | `after` returns the piece of `s` past the first `mark`; `before` returns the piece up to it | [Collections & strings](collections-strings.md) |
| [`before_last`](../fn/before_last.html) | Returns a `Str`: like `after`/`before` but searching from the end — the pair to reach for on a path or a dotted name | [Collections & strings](collections-strings.md) |
| [`bin2dec`](../fn/bin2dec.html) | Reads a binary digit string as a number | [Collections & strings](collections-strings.md) |
| [`binomial`](../fn/binomial.html) | Exact binomial draws. `n_trials` (a positive integer) is how many trials each draw counts successes over; `p` (a number in `[0, 1]`) is the success probability of one trial; `rows` and.. | [Statistics & ML](statistics-ml.md) |
| [`bitand`](../fn/bitand.html) | Returns a `number`: the bitwise AND of `a` and `b` as 64-bit integers | [Collections & strings](collections-strings.md) |
| [`bitcmp`](../fn/bitcmp.html) | Returns a `number`: the bitwise complement (`!a`) as a 64-bit integer | [Collections & strings](collections-strings.md) |
| [`bitor`](../fn/bitor.html) | Returns a `number`: the bitwise OR | [Collections & strings](collections-strings.md) |
| [`bitshift`](../fn/bitshift.html) | Shifts an integer's bits left or right | [Collections & strings](collections-strings.md) |
| [`bitxor`](../fn/bitxor.html) | Returns a `number`: the bitwise XOR | [Collections & strings](collections-strings.md) |
| [`blackman`](../fn/blackman.html) | Returns a length-`n` real vector | [Signal processing](signal-processing.md) |
| [`blob_stats`](../fn/blob_stats.html) | Per-region area, centroid and bounding box, from a labelled image (identical operation to `regionprops` above) | [Images](images.md) |
| [`block_process`](../fn/block_process.html) | Cuts `x` into consecutive blocks, calls `f` on each, and concatenates the results | [Signal processing](signal-processing.md) |
| [`blocks`](../fn/blocks.html) | Returns a list of blocks, so `for b in s.blocks(4096)` iterates it with the ordinary `for` loop | [Signal processing](signal-processing.md) |
| [`blur`](../fn/blur.html) | Applies a fixed 3x3 box-average blur kernel | [Images](images.md) |
| [`blur_backdrop`](../fn/blur_backdrop.html) | Figure-level (not per-panel) depth-of-field effect: `(x0,y0)`–`(x1,y1)` (numbers, fractions `0..1` of the whole rendered canvas) name the rectangle that stays sharp; everything else is.. | [Plotting](plotting.md) |
| [`bode`](../fn/bode.html) | Draws a complete two-panel Bode figure — magnitude in dB above, phase in degrees below, both against a log frequency axis — and returns `Nothing` | [Signal processing](signal-processing.md) |
| [`bode_magnitude`](../fn/bode_magnitude.html) | Magnitude half of a Bode plot: `20*log10(\|Z\|)` vs. log-scaled frequency | [Signal processing](signal-processing.md) |
| [`bode_phase`](../fn/bode_phase.html) | Phase half of a Bode plot: `arg(Z)` in degrees vs. log-scaled frequency | [Signal processing](signal-processing.md) |
| [`box`](../fn/box.html) | Toggles the panel's bounding box | [Plotting](plotting.md) |
| [`boxplot`](../fn/boxplot.html) | One group per argument (each a Vec of numbers), placed at `x = 0, 1, 2, ...`; draws a Tukey five-number summary (min/Q1/median/Q3/max plus outliers) per group as a box-and-whisker | [Plotting](plotting.md) |
| [`bubble`](../fn/bubble.html) | Scatter where marker radius encodes a third variable: `x`, `y`, `sizes` are equal-length numeric Vecs; the drawn radius is scaled from `sizes`'s own min/max | [Plotting](plotting.md) |
| [`builtins`](../fn/builtins.html) | Returns a `List` of strings: every name the engine answers to (hundreds of entries; the exact count grows as builtins are added) | [REPL & diagnostics](repl-diagnostics.md) |
| [`butter`](../fn/butter.html) | Returns a `Model` (kind `"filter"`, field `sos`) | [Signal processing](signal-processing.md) |
| [`bwareaopen`](../fn/bwareaopen.html) | Labels internally (8-connectivity, matching MATLAB's default) and zeroes out every blob whose pixel count is below `min_area` | [Images](images.md) |
| [`bwlabel`](../fn/bwlabel.html) | Connected-component labeling. Returns a `Model` (kind `"blobs"`) with fields `.labels` (a `Mat` of the same height/width as `binary_img`, integer label ids, background = 0), `.count` (a.. | [Images](images.md) |

## C

| | | |
|---|---|---|
| [`calibrate`](../fn/calibrate.html) | Added in v0.2.4. Turns a raw `Signal` into a physically calibrated one, scaling the samples and stamping the resulting unit | [Signal processing](signal-processing.md) |
| [`capacitor`](../fn/capacitor.html) | `1/(jwC)` | [Signal processing](signal-processing.md) |
| [`capitalize`](../fn/capitalize.html) | Uppercases the first character, leaves the rest untouched | [Collections & strings](collections-strings.md) |
| [`capture`](../fn/capture.html) | Runs the named drawing function `"fn"` (string, a function you defined) into a scratch panel, passing along any further positional `args...`, and keeps what it drew as a value (a "layer") | [Plotting](plotting.md) |
| [`cast`](../fn/cast.html) | Returns a value of that kind. A conversion that is not defined is an error, not a best effort | [Collections & strings](collections-strings.md) |
| [`cat`](../fn/cat.html) | MATLAB-style concatenation: `dim` (a scalar, `1` or `2`) selects the axis, then the 2+ remaining arguments (same shape rules as `hstack`/`vstack`) are joined along it — `dim=1` vertical.. | [Core maths](core-math.md) |
| [`cbrt`](../fn/cbrt.html) | Real cube root, elementwise — unlike `x^(1/3)`, correctly handles a negative `x` (`cbrt(-8) = -2`, not `NaN`) | [Core maths](core-math.md) |
| [`cd`](../fn/cd.html) | Changes the working directory to `path` (a `Str`) for the whole process, so every later relative path in the script resolves from there | [File I/O](file-io.md) |
| [`ceil`](../fn/ceil.html) | Standard rounding functions, elementwise: `floor` toward negative infinity, `ceil` toward positive infinity, `round` to the nearest integer (half away from zero) | [Core maths](core-math.md) |
| [`channel`](../fn/channel.html) | Creates a new, empty channel. Returns a `Channel` handle (`type(ch) == "channel"`); cloning it shares the same underlying queue, not a separate copy | [Concurrency](concurrency.md) |
| [`channel_len`](../fn/channel_len.html) | How many messages are queued right now on channel `ch`, without consuming any of them | [Concurrency](concurrency.md) |
| [`channel_recv`](../fn/channel_recv.html) | Blocks the calling thread until a value is available on channel `ch`, then pops and returns it (FIFO — oldest `channel_send`d value first) | [Concurrency](concurrency.md) |
| [`channel_send`](../fn/channel_send.html) | Pushes `value` (any value, any type) onto channel `ch`'s queue and wakes one waiting `channel_recv` | [Concurrency](concurrency.md) |
| [`channel_try_recv`](../fn/channel_try_recv.html) | Non-blocking counterpart to `channel_recv`: returns `none` (`Nothing`) immediately if nothing is queued on channel `ch` right now, instead of waiting; otherwise pops and returns the next.. | [Concurrency](concurrency.md) |
| [`chars`](../fn/chars.html) | Returns a `List` of one-character `Str`s | [Collections & strings](collections-strings.md) |
| [`cheby1`](../fn/cheby1.html) | Returns a `Model` (kind `"filter"`, field `sos`) | [Signal processing](signal-processing.md) |
| [`cheby2`](../fn/cheby2.html) | Returns a `Model` (kind `"filter"`, field `sos`) | [Signal processing](signal-processing.md) |
| [`check_grads`](../fn/check_grads.html) | Verifies the analytic reverse-mode gradient of scalar-valued `f` at `x` against central finite differences of `f` itself — an independent route to the same number, which is the point: a.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`chi2cdf`](../fn/chi2cdf.html) | Chi-square cumulative distribution `P(X <= x)` at `x` (a number or vector, elementwise) with `k` degrees of freedom | [Statistics & ML](statistics-ml.md) |
| [`chi2pdf`](../fn/chi2pdf.html) | Chi-square probability density at `x` (a number or vector, elementwise) with `k` degrees of freedom (a positive number), computed in log space so it survives modest `k` without overflowing | [Statistics & ML](statistics-ml.md) |
| [`chirp`](../fn/chirp.html) | Identical to `sweep` above — same four positional arguments (start frequency, end frequency, sample rate, sample count) and the same `method=` (string, `"linear"` default or.. | [Plotting](plotting.md) |
| [`chisquare`](../fn/chisquare.html) | Draws from the chi-square distribution with `k` degrees of freedom (a positive integer), same `rows`/`cols`/`seed=` shape as `rand` | [Statistics & ML](statistics-ml.md) |
| [`chol`](../fn/chol.html) | Lower-triangular Cholesky factor `L` of `A = L·Lᵀ`, where `A` is a symmetric positive-definite `n×n` `Mat`; errors clearly if `A` isn't positive-definite | [Core maths](core-math.md) |
| [`chr`](../fn/chr.html) | Returns a one-character `Str` from `chr`, and a `number` — the code point — from `ord` | [Collections & strings](collections-strings.md) |
| [`circle`](../fn/circle.html) | Draws a circle centered at `(cx, cy)` (numbers, data units) with radius `r` (number, data units) | [Plotting](plotting.md) |
| [`circuit`](../fn/circuit.html) | Builds a circuit from a spec string and a flat parameter vector | [Signal processing](signal-processing.md) |
| [`circuit_fit`](../fn/circuit_fit.html) | Nonlinear least-squares fit of a circuit's parameters to measured impedance | [Signal processing](signal-processing.md) |
| [`clahe`](../fn/clahe.html) | Contrast-Limited Adaptive Histogram Equalization: `imequalize`'s tile-based local counterpart, with a clipped per-tile histogram (clip threshold `clip_limit * tile_pixel_count / 256`) and —.. | [Images](images.md) |
| [`clamp`](../fn/clamp.html) | Elementwise clamp of `x` (a scalar, `Vec`, or `Mat`/`Signal`) into `[lo, hi]` | [Core maths](core-math.md) |
| [`clip`](../fn/clip.html) | Elementwise clamp of `x` (a scalar, `Vec`, or `Mat`/`Signal`) into `[lo, hi]` | [Core maths](core-math.md) |
| [`close`](../fn/close.html) | Flushes and releases a file handle | [File I/O](file-io.md) |
| [`cm`](../fn/cm.html) | Length units as functions, converting a scalar `v` (millimetres/centimetres/inches/points as named) to the figure's own coordinate space — a tenth of a millimetre, so `mm(1)` is `10`,.. | [Core maths](core-math.md) |
| [`cmyk`](../fn/cmyk.html) | Builds a colour from the printer's subtractive space: cyan `c`, magenta `m`, yellow `y`, key/black `k`, each a number 0–1 | [Plotting](plotting.md) |
| [`coherence`](../fn/coherence.html) | Returns a single number: the largest inner product between any two different normalised columns of `A` | [Noise](noise.md) |
| [`colorbar`](../fn/colorbar.html) | Accepted, with or without a `[extend=]`-style argument in the function form (so MATLAB-style scripts parse) but currently a documented no-op in both forms — no colorbar is actually drawn | [Plotting](plotting.md) |
| [`colormap`](../fn/colormap.html) | Sets the current panel's categorical/sequential palette to `name` (string — see the Markers & Styling reference below for the full list) | [Plotting](plotting.md) |
| [`cols`](../fn/cols.html) | Row / column count of `A`. On a `Vec` (shape `(n, 1)`), a `Mat`/`Signal` (shape `(r, c)`), or a bare scalar (shape `(1, 1)`), returns the corresponding dimension as a scalar `Num` | [Core maths](core-math.md) |
| [`compare`](../fn/compare.html) | Returns a `number`: −1, 0 or 1, so it can drive a sort directly | [Collections & strings](collections-strings.md) |
| [`compile`](../fn/compile.html) | Attaches an optimizer Record (from `sgd`/`adam`/etc., default `adam()`) and a loss name string (`"mse"` or `"cross_entropy"`, default `"mse"`) to an `input(n) \|> dense(...) \|> ...`.. | [Statistics & ML](statistics-ml.md) |
| [`complex`](../fn/complex.html) | Returns a `Complex`. `3 + 4j` is the literal form; this is for computed components | [Collections & strings](collections-strings.md) |
| [`cond`](../fn/cond.html) | The 2-norm condition number of `A` (an `r×c` `Mat` or `CMat`) — the ratio of its largest to smallest singular value | [Core maths](core-math.md) |
| [`confusion_matrix`](../fn/confusion_matrix.html) | Draws a confusion-matrix heatmap comparing a length-`N` vector of true labels `actual` against a length-`N` vector of predicted labels `predicted`, over an optional integer `n_classes`.. | [Statistics & ML](statistics-ml.md) |
| [`conj`](../fn/conj.html) | Complex conjugate (negates the imaginary part); passes a real `x` through unchanged | [Core maths](core-math.md) |
| [`contains`](../fn/contains.html) | Returns a `bool`: membership test via structural equality | [Collections & strings](collections-strings.md) |
| [`contour`](../fn/contour.html) | Iso-lines through scalar field `Z` (a Mat with `len(y)` rows and `len(x)` columns) over axis vectors `x`, `y` (Vecs of numbers); `contourf` fills the bands between levels instead of only.. | [Plotting](plotting.md) |
| [`contourf`](../fn/contourf.html) | Iso-lines through scalar field `Z` (a Mat with `len(y)` rows and `len(x)` columns) over axis vectors `x`, `y` (Vecs of numbers); `contourf` fills the bands between levels instead of only.. | [Plotting](plotting.md) |
| [`conv`](../fn/conv.html) | Returns a vector of the length implied by `mode` | [Signal processing](signal-processing.md) |
| [`conv1d`](../fn/conv1d.html) | Slides a length-`K` learnable `kernel` vector over a length-`N` signal `x`, moving `stride` positions at a time (integer, default 1) with `padding` (`"valid"` — no padding, output shrinks —.. | [Statistics & ML](statistics-ml.md) |
| [`conv2d`](../fn/conv2d.html) | Slides a `(Kh, Kw)` learnable `kernel` matrix over an `(H, W)` image matrix `x`, moving `stride` positions at a time (integer, default 1) with `padding` — `"valid"` (no padding) or `"same"`.. | [Statistics & ML](statistics-ml.md) |
| [`convert_unit`](../fn/convert_unit.html) | Added in v0.2.4. Rescales the samples into another unit of the same physical quantity and moves the unit tag with them, so the two cannot disagree; a calibration's dB reference is.. | [Signal processing](signal-processing.md) |
| [`copy_file`](../fn/copy_file.html) | Copies `src` to `dst`, leaving the original in place | [File I/O](file-io.md) |
| [`corr`](../fn/corr.html) | Pearson correlation between two length-`N` numeric vectors `x` and `y` | [Statistics & ML](statistics-ml.md) |
| [`corr_heatmap`](../fn/corr_heatmap.html) | Computes the pairwise Pearson correlation of Table `t`'s numeric columns | [Plotting](plotting.md) |
| [`corrcoef`](../fn/corrcoef.html) | Called on an `N`-row, `D`-column matrix or table `M`, returns the full `(D, D)` matrix of pairwise Pearson correlations between its columns | [Statistics & ML](statistics-ml.md) |
| [`corrmat`](../fn/corrmat.html) | Computes the pairwise Pearson correlation of Table `t`'s numeric columns | [Plotting](plotting.md) |
| [`corrplot`](../fn/corrplot.html) | Computes the pairwise Pearson correlation of Table `t`'s numeric columns | [Plotting](plotting.md) |
| [`cos`](../fn/cos.html) | Standard trigonometric functions, computed elementwise | [Core maths](core-math.md) |
| [`cosh`](../fn/cosh.html) | Hyperbolic sine/cosine/tangent, elementwise | [Core maths](core-math.md) |
| [`coth`](../fn/coth.html) | The other three hyperbolics: `coth(x) = cosh(x)/sinh(x)`, `sech(x) = 1/cosh(x)`, `csch(x) = 1/sinh(x)` | [Core maths](core-math.md) |
| [`count`](../fn/count.html) | Returns a `number`: how many non-overlapping times `sub` occurs in `s` (non-overlapping so it agrees with `replace`) | [Collections & strings](collections-strings.md) |
| [`cov`](../fn/cov.html) | Sample covariance between two length-`N` numeric vectors `x` and `y`, `N-1` denominator — the same convention `var` and `std` use | [Statistics & ML](statistics-ml.md) |
| [`cpe`](../fn/cpe.html) | `1/(Q (jw)^n)` — constant-phase element | [Signal processing](signal-processing.md) |
| [`cqt`](../fn/cqt.html) | Constant-Q transform: log-spaced bins sharing one `Q = f/bandwidth`, so a musical interval spans the same number of bins at every pitch | [Signal processing](signal-processing.md) |
| [`crc`](../fn/crc.html) | Textbook unreflected, zero-initialized polynomial division: the message is shifted left by the CRC width and divided mod 2 | [Signal processing](signal-processing.md) |
| [`crc_check`](../fn/crc_check.html) | Divides the whole codeword and reports whether the remainder is all zeros | [Signal processing](signal-processing.md) |
| [`create_file`](../fn/create_file.html) | `make_file` with a collision policy: `"error"` (default) refuses if `path` already exists, `"overwrite"` truncates it like `make_file` does unconditionally, `"skip"` does nothing and.. | [File I/O](file-io.md) |
| [`crest_factor`](../fn/crest_factor.html) | `peak(x) / rms(x)`, a dimensionless ratio (a sine wave's is `sqrt(2)` ≈ 1.414) — the same quantity `sinad_estimate(bits, crest_factor)` takes as its second argument | [Core maths](core-math.md) |
| [`crop`](../fn/crop.html) | Extracts that sub-rectangle. Returns a new `Image` of exactly `width` x `height` pixels | [Images](images.md) |
| [`cs_guarantee`](../fn/cs_guarantee.html) | Returns a single number: the largest sparsity level that the matrix's coherence alone can *prove* recoverable | [Noise](noise.md) |
| [`cs_recover`](../fn/cs_recover.html) | Compressed-sensing reconstruction of a sparse vector `x` from an M×N sensing matrix `A` and a length-M measurement vector `y` | [Noise](noise.md) |
| [`csch`](../fn/csch.html) | The other three hyperbolics: `coth(x) = cosh(x)/sinh(x)`, `sech(x) = 1/cosh(x)`, `csch(x) = 1/sinh(x)` | [Core maths](core-math.md) |
| [`csd`](../fn/csd.html) | Returns a length-`nperseg/2 + 1` complex vector (`CVec`), the one-sided cross-spectral density `Pxy` — Welch's method, sharing `welch`'s own framing/windowing/averaging exactly, so it lines.. | [Signal processing](signal-processing.md) |
| [`csv2json`](../fn/csv2json.html) | Not `jsonify(parse_csv(str))` — it emits a plain JSON array of one flat `{"col": value, ...}` object per row (the natural shape for feeding to another JSON-consuming tool), rather than.. | [File I/O](file-io.md) |
| [`csv2xml`](../fn/csv2xml.html) | `str` (string, CSV text) is converted to XML: exactly `xmlify(parse_csv(str))`. Returns a string | [File I/O](file-io.md) |
| [`csvify`](../fn/csvify.html) | Converts `table` (Table) to CSV text — exactly `write_csv`'s own formatting (`Table::to_csv`), just returned as a string instead of written to a file | [File I/O](file-io.md) |
| [`ctranspose`](../fn/ctranspose.html) | Conjugate transpose (Hermitian): transpose plus elementwise complex conjugation | [Core maths](core-math.md) |
| [`cumsum`](../fn/cumsum.html) | Returns the running sum, same type/length as `x`; a `Signal` input stays a `Signal` with the same `Fs` | [Collections & strings](collections-strings.md) |
| [`cur_dir`](../fn/cur_dir.html) | `cur_dir` says what it gives back and `pwd` is what anyone who has used a shell types first, so both spellings exist rather than one being renamed out from under existing scripts | [File I/O](file-io.md) |
| [`curve_fit`](../fn/curve_fit.html) | Returns a `Model` (kind `"curve_fit"`) with fields `params` (a length-P vector), `cost` (a number, residual sum of squares), `converged` (a boolean) | [Signal processing](signal-processing.md) |
| [`cut`](../fn/cut.html) | Returns a new `Signal` at the SAME `Fs`, shorter than `sig` | [Signal processing](signal-processing.md) |
| [`cv_stability`](../fn/cv_stability.html) | Repeats cross-validation `n_repeats` times (integer, default 10) with `cv_folds` folds each (integer, default 5) on an `N`-row `X` and length-`N` `y`, reshuffling between repeats | [Statistics & ML](statistics-ml.md) |
| [`cwt`](../fn/cwt.html) | Continuous wavelet transform: one column per input sample, with no framing and no decimation — unlike `dwt`, which halves its length at every level and visits only dyadic scales | [Signal processing](signal-processing.md) |

## D

| | | |
|---|---|---|
| [`daily_profile`](../fn/daily_profile.html) | Returns a length-7 real vector, one bin per day-of-week -- so a weekday-against-weekend difference separates out | [Signal processing](signal-processing.md) |
| [`db`](../fn/db.html) | Alias of `mag2db(x)` above (`20*log10(x)`, amplitude convention) — the bare SciPy/general-DSP-flavored spelling | [Signal processing](signal-processing.md) |
| [`db2mag`](../fn/db2mag.html) | Returns the same shape: `10^(db/20)` — inverse of `mag2db` | [Signal processing](signal-processing.md) |
| [`db2pow`](../fn/db2pow.html) | Returns the same shape: `10^(db/10)` — inverse of `pow2db`/`db_power` | [Signal processing](signal-processing.md) |
| [`db_power`](../fn/db_power.html) | Alias of `pow2db(x)` above (`10*log10(x)`, power convention) — the bare SciPy/general-DSP-flavored spelling | [Signal processing](signal-processing.md) |
| [`dbfs`](../fn/dbfs.html) | Peak level relative to full scale: `20*log10(peak(abs(x)) / full_scale)` | [Core maths](core-math.md) |
| [`dbscan`](../fn/dbscan.html) | Density-based clustering on an `N`-row, `D`-column data matrix `X`: a number `eps` (the neighbourhood radius, in `X`'s own units) and an integer `min_samples` (the minimum neighbourhood.. | [Statistics & ML](statistics-ml.md) |
| [`dct`](../fn/dct.html) | Returns a length-N real vector | [Signal processing](signal-processing.md) |
| [`dec2bin`](../fn/dec2bin.html) | Returns a `Str` of `0`/`1` digits, no prefix | [Collections & strings](collections-strings.md) |
| [`dec2hex`](../fn/dec2hex.html) | Returns a `Str`: uppercase hex digits, no prefix | [Collections & strings](collections-strings.md) |
| [`decode_can`](../fn/decode_can.html) | Added in v0.3.0. `bus`: one `Digital` value — the differential pair has already been resolved to a logic level by the transceiver or by the scope's own threshold | [Signal processing](signal-processing.md) |
| [`decode_i2c`](../fn/decode_i2c.html) | Added in v0.3.0. `scl`, `sda`: two `Digital` values from `to_digital`, clock first, data second — I2C framing is defined by SDA moving while SCL is held high, so neither wire decodes alone.. | [Signal processing](signal-processing.md) |
| [`decode_spi`](../fn/decode_spi.html) | Decodes clocked SPI traffic into bytes, sampling each data line at whichever clock edge CPOL and CPHA select | [Signal processing](signal-processing.md) |
| [`decode_uart`](../fn/decode_uart.html) | Decodes asynchronous serial frames into bytes at a stated bit rate | [Signal processing](signal-processing.md) |
| [`delay`](../fn/delay.html) | Returns the SAME length as `x`, shifted later by `n` with the vacated head zero-filled and whatever runs off the end dropped | [Signal processing](signal-processing.md) |
| [`delta_e`](../fn/delta_e.html) | Compares colours `a` and `b` (each any accepted colour string) in CIE76 Lab space and returns a single number: the perceptual distance | [Plotting](plotting.md) |
| [`dense`](../fn/dense.html) | One fully-connected layer on an `N`-row, `D`-column input matrix `x`, a `(D, H)` weight matrix `w`, and a length-`H` bias vector `b`: computes `x * w + b`, broadcasting `b` over every row | [Statistics & ML](statistics-ml.md) |
| [`dense_layer`](../fn/dense_layer.html) | Builds one unfitted dense-layer spec: `in_dim`/`out_dim` (positive integers) the layer's input and output widths, `activation` a string, `"relu"` or `"none"` (default `"none"`, a bare.. | [Statistics & ML](statistics-ml.md) |
| [`describe`](../fn/describe.html) | Pandas-style `describe()`: returns a `Table` with one row per summary statistic (`count`, `mean`, `std`, `min`, `25%`, `50%`, `75%`, `max`) and one column per NUMERIC column of `df` (text.. | [Collections & strings](collections-strings.md) |
| [`det`](../fn/det.html) | Determinant of a square `Mat` or `CMat`, via LU decomposition | [Core maths](core-math.md) |
| [`detect_saturation`](../fn/detect_saturation.html) | Has `x` hit the rails of an `adc_bits`-bit converter: the clipping question for a converter whose range is *known* rather than inferred, so no flat run is required and a single sample.. | [Signal processing](signal-processing.md) |
| [`detrend`](../fn/detrend.html) | Subtracts a least-squares polynomial trend from `x`, a length-N number vector | [Noise](noise.md) |
| [`device_used`](../fn/device_used.html) | Returns a struct-like value with two fields: `.device` (a string, e.g. `"cpu"` or a GPU device name) and `.gpu_dispatches` (a number, the count of operations actually dispatched to the GPU) | [REPL & diagnostics](repl-diagnostics.md) |
| [`dft`](../fn/dft.html) | Returns a length-N `CVec`. The direct, textbook O(n²) transform, kept as an independent reference implementation to validate `fft` against and as the definitional basis `goertzel`.. | [Signal processing](signal-processing.md) |
| [`diag`](../fn/diag.html) | Overloaded on argument type: `diag(M)`, where `M` is an `r×c` `Mat`, returns its main diagonal as a length-`min(r,c)` `Vec`; `diag(v)`, where `v` is a length-`n` `Vec` (or scalar/other.. | [Core maths](core-math.md) |
| [`diagram_pipeline`](../fn/diagram_pipeline.html) | Renders a `\|>` pipe chain as a left-to-right chain of labeled boxes, one per stage | [Plotting](plotting.md) |
| [`dict`](../fn/dict.html) | Returns a `Dict`, insertion-ordered | [Collections & strings](collections-strings.md) |
| [`diff`](../fn/diff.html) | Returns the first difference (`x[i+1] - x[i]`), length N-1, same base type as `x`; a `Signal` input stays a `Signal` with the same `Fs` | [Collections & strings](collections-strings.md) |
| [`dir`](../fn/dir.html) | The bare names inside `path` (a `Str`), sorted — `"a.qu"`, not `"tools/a.qu"` | [File I/O](file-io.md) |
| [`dir_exists`](../fn/dir_exists.html) | Whether `path` (a `Str`) exists and is a regular file / a directory | [File I/O](file-io.md) |
| [`disp`](../fn/disp.html) | Writes its arguments, space-separated, and a newline, to stdout | [Collections & strings](collections-strings.md) |
| [`distinct`](../fn/distinct.html) | Returns the same type: the unique values, in the order they first appear — which `unique` does not promise, since it sorts | [Collections & strings](collections-strings.md) |
| [`distort`](../fn/distort.html) | Put the signal through a nonlinearity: `clip`, `soft`, `crossover`, `harmonic`, `quantize` | [Noise](noise.md) |
| [`div`](../fn/div.html) | The operators as named functions, so an operation can be passed where a name is what you can pass | [Core maths](core-math.md) |
| [`dominant_frequency`](../fn/dominant_frequency.html) | Returns a single number: the frequency of the largest non-DC bin in `x`'s one-sided spectrum, accurate to the bin's own resolution `fs/N` | [Signal processing](signal-processing.md) |
| [`donut`](../fn/donut.html) | Pie chart with a hollow center | [Plotting](plotting.md) |
| [`dot`](../fn/dot.html) | Dot product of two equal-length vectors `a`, `b` (each a `Vec`; a length mismatch errors) | [Core maths](core-math.md) |
| [`double_buffer`](../fn/double_buffer.html) | Returns a `double_buffer` handle; front AND back both start equal to `initial` | [Collections & strings](collections-strings.md) |
| [`downsample`](../fn/downsample.html) | Lowers the sample rate by an integer factor, anti-aliasing first: applies a lowpass at the new Nyquist (`fs/(2*M)`, at the input rate) and only then keeps every `M`th sample | [Signal processing](signal-processing.md) |
| [`drop`](../fn/drop.html) | Returns the same type as `xs`: the first `n` elements (`take`) or everything after them (`drop`) | [Collections & strings](collections-strings.md) |
| [`drop_row`](../fn/drop_row.html) | Returns a `Table` without those rows, by position | [Collections & strings](collections-strings.md) |
| [`dropout`](../fn/dropout.html) | Zeroes a random share of the entries of `x` (a vector or matrix of any shape) at probability `rate` (a number in `[0, 1)`), and scales the surviving entries up by `1 / (1 - rate)`, so the.. | [Statistics & ML](statistics-ml.md) |
| [`dropout_layer`](../fn/dropout_layer.html) | Builds one unfitted dropout-layer spec for use inside a `sequential` stack: `rate` a number in `[0, 1)`, the fraction of activations to zero during training | [Statistics & ML](statistics-ml.md) |
| [`duty_cycle`](../fn/duty_cycle.html) | Returns a single number: mean high time over mean period, as a fraction in `[0, 1]`, not a percentage — multiply by 100 for the figure a scope's front panel shows | [Signal processing](signal-processing.md) |
| [`dwt`](../fn/dwt.html) | Returns a `(2, N/2)` matrix — row 0 is the approximation band, row 1 is the detail band (Qu has no multi-return) | [Signal processing](signal-processing.md) |

## E

| | | |
|---|---|---|
| [`ecdf`](../fn/ecdf.html) | Empirical cumulative distribution, one staircase per argument | [Plotting](plotting.md) |
| [`echo`](../fn/echo.html) | Writes its arguments, space-separated, and a newline, to stdout | [Collections & strings](collections-strings.md) |
| [`eda`](../fn/eda.html) | One-call exploratory overview: a correlation heatmap in the first grid cell, plus one histogram per numeric column in the remaining cells, auto-gridded (`ceil(sqrt(n))` columns for `n`.. | [Plotting](plotting.md) |
| [`edge_detect`](../fn/edge_detect.html) | Applies a fixed 3x3 discrete Laplacian edge filter, highlighting intensity discontinuities | [Images](images.md) |
| [`edges`](../fn/edges.html) | Same `hysteresis` as `find_edges`, same return shape (a `Model`, kind `"edges"`, with `indices`/`directions`/`positions`/`rising`/`falling`/`count`) — `d.edges()` is `find_edges(x,.. | [Signal processing](signal-processing.md) |
| [`eig`](../fn/eig.html) | Eigenvalues (and, for symmetric `A`, real eigenvectors) of a square `n×n` `Mat` `A` | [Core maths](core-math.md) |
| [`elapsed`](../fn/elapsed.html) | Returns a number: elapsed seconds so far | [REPL & diagnostics](repl-diagnostics.md) |
| [`elediv`](../fn/elediv.html) | The `.*`, `./` and `.^` family: always elementwise, whatever the shapes | [Core maths](core-math.md) |
| [`elemul`](../fn/elemul.html) | The `.*`, `./` and `.^` family: always elementwise, whatever the shapes | [Core maths](core-math.md) |
| [`elepow`](../fn/elepow.html) | The `.*`, `./` and `.^` family: always elementwise, whatever the shapes | [Core maths](core-math.md) |
| [`ellip`](../fn/ellip.html) | Returns a `Model` (kind `"filter"`, field `sos`) | [Signal processing](signal-processing.md) |
| [`ellipse`](../fn/ellipse.html) | Draws an ellipse centered at `(cx, cy)` (numbers) with semi-axis radii `rx` and `ry` (numbers, data units along x and y respectively) | [Plotting](plotting.md) |
| [`emd`](../fn/emd.html) | Empirical Mode Decomposition: splits `x` into oscillatory intrinsic mode functions (IMFs, highest frequency first) plus a residual trend, via the standard sifting algorithm | [Signal processing](signal-processing.md) |
| [`emf`](../fn/emf.html) | Adds simulated switching/EMI interference to `x`, a length-N signal vector, given the sample rate `fs` in Hz: an amplitude-modulated carrier plus damped bursts at each switching edge | [Noise](noise.md) |
| [`end_time`](../fn/end_time.html) | Added in v0.2.4. `start_time + duration`, i.e. the end of the record — one sample period after the last sample's own timestamp, so that `end_time - start_time` is exactly the duration | [Signal processing](signal-processing.md) |
| [`ends_with`](../fn/ends_with.html) | Returns a `bool`: `true` if `s` ends with `suffix` | [Collections & strings](collections-strings.md) |
| [`energy`](../fn/energy.html) | Returns a single number, `sum(x.^2)` — total signal energy | [Signal processing](signal-processing.md) |
| [`enob`](../fn/enob.html) | Returns a single number, bits — effective number of bits, inverting `6.02·bits + 1.76` on the measured SINAD | [Signal processing](signal-processing.md) |
| [`enob_estimate`](../fn/enob_estimate.html) | Returns a single number, bits — effective number of bits, inverting `6.02·bits + 1.76` on the measured SINAD | [Signal processing](signal-processing.md) |
| [`enum_values`](../fn/enum_values.html) | Returns a `List` of `EnumVal`s: every variant of the declared type, in declaration order — makes the type a real enumeration rather than just a handful of named constants, and lets code.. | [Collections & strings](collections-strings.md) |
| [`eof`](../fn/eof.html) | Checks whether `f` (file handle, opened readable) has any bytes left to read from the current position | [File I/O](file-io.md) |
| [`erf`](../fn/erf.html) | The error function and its complement (`erfc(x) = 1 - erf(x)`, computed directly rather than by subtraction so it stays accurate for large `x`, where `erf(x)` itself has already saturated.. | [Core maths](core-math.md) |
| [`erfc`](../fn/erfc.html) | The error function and its complement (`erfc(x) = 1 - erf(x)`, computed directly rather than by subtraction so it stays accurate for large `x`, where `erf(x)` itself has already saturated.. | [Core maths](core-math.md) |
| [`error`](../fn/error.html) | Never returns normally — control passes to the nearest enclosing `try`/`catch`, where the caught exception object exposes the string as `.message`; uncaught, it aborts the script | [REPL & diagnostics](repl-diagnostics.md) |
| [`errorbar`](../fn/errorbar.html) | Draws one capped whisker plus a marker per sample: `x`, `y`, `yerr` are equal-length numeric Vecs, `yerr` the (symmetric) half-height of each whisker in data units | [Plotting](plotting.md) |
| [`estimate`](../fn/estimate.html) | Estimation-family only: reads a point estimate off a filter state Record `state` with no arguments | [Statistics & ML](statistics-ml.md) |
| [`estimate_complexity`](../fn/estimate_complexity.html) | Takes two same-length numeric vectors/lists: `sizes` (the problem sizes swept over) and `times` (the matching elapsed times in seconds, e.g. from `toc()` at each size) — at least 3 matching.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`estimate_frequency`](../fn/estimate_frequency.html) | Returns a single number: the frequency of the largest non-DC bin in `x`'s one-sided spectrum, accurate to the bin's own resolution `fs/N` | [Signal processing](signal-processing.md) |
| [`exec`](../fn/exec.html) | Runs `program` (a `Str`) and waits, capturing what it printed | [File I/O](file-io.md) |
| [`exp`](../fn/exp.html) | Returns exactly the same type and shape as `x` | [Core maths](core-math.md) |
| [`exp2`](../fn/exp2.html) | Returns exactly the same type and shape as `x` | [Core maths](core-math.md) |
| [`explain`](../fn/explain.html) | The report says whether that function is running the fast register-only ("soft-compiled") tier or falls back to the ordinary tree-walking interpreter, and — when interpreted — which.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`explore`](../fn/explore.html) | Writes a self-contained HTML page to `path` (string) with one slider per parameter of function `"fn"` (string) | [Plotting](plotting.md) |
| [`expm1`](../fn/expm1.html) | `e^x - 1`, computed so it stays accurate for `x` near zero (`expm1(1e-15)` does not lose precision to the cancellation that `exp(x) - 1` would hit there) | [Core maths](core-math.md) |
| [`exponential`](../fn/exponential.html) | Exponential draws. `rate` (a positive number) is the rate, not the scale: the mean is `1/rate`, and NumPy instead takes `scale = 1/rate` directly, so a value copied from NumPy needs.. | [Statistics & ML](statistics-ml.md) |
| [`eye`](../fn/eye.html) | The `n×n` identity matrix (ones on the diagonal, zeros elsewhere) | [Core maths](core-math.md) |

## F

| | | |
|---|---|---|
| [`f1`](../fn/f1.html) | Classification metrics on a length-`N` vector of true labels `actual` and a length-`N` vector of predicted labels `predicted`, each returning a single number macro-averaged over every class.. | [Statistics & ML](statistics-ml.md) |
| [`fall_time`](../fn/fall_time.html) | Returns a single number: the transition time of the first complete rising (resp | [Signal processing](signal-processing.md) |
| [`falling_edges`](../fn/falling_edges.html) | Returns a vector of the interpolated crossing positions of one polarity only — seconds for a `Signal`, samples otherwise (the same convention `find_pulses`'s `widths` field uses) | [Signal processing](signal-processing.md) |
| [`fft`](../fn/fft.html) | On the FULL (two-sided) spectrum this returns, a real tone occupies TWO conjugate-symmetric bins, and scaling makes each of them independently read the tone's full peak amplitude (or RMS).. | [Signal processing](signal-processing.md) |
| [`fftc`](../fn/fftc.html) | `rfft` returns a length-`floor(N/2)+1` complex vector, the one-sided half-spectrum; `fftc` returns a length-N complex vector, the full spectrum | [Signal processing](signal-processing.md) |
| [`fftr`](../fn/fftr.html) | Deprecated — use `irfft`. The same function under a name that reads as `rfft`'s forward partner; it is actually `rfft`'s inverse, so the pair reads backwards | [Signal processing](signal-processing.md) |
| [`fifo`](../fn/fifo.html) | Returns a `fifo` handle: a new, empty ring buffer | [Collections & strings](collections-strings.md) |
| [`figure`](../fn/figure.html) | Clears/resets the whole figure (all panels), starting a fresh one | [Plotting](plotting.md) |
| [`figure_background`](../fn/figure_background.html) | Sets the current figure's SVG background fill to `color` (string: `"#rrggbb"` hex or a CSS named colour; default `"#ffffff"`), passed straight through, unparsed | [Plotting](plotting.md) |
| [`figure_size`](../fn/figure_size.html) | Sets canvas size either from `w`, `h` (numbers, raw units) or from a named journal-column preset string (e.g. `"ieee1"`) | [Plotting](plotting.md) |
| [`file_exists`](../fn/file_exists.html) | Whether `path` (a `Str`) exists and is a regular file / a directory | [File I/O](file-io.md) |
| [`file_size`](../fn/file_size.html) | Reports a file's size without opening it | [File I/O](file-io.md) |
| [`fill_between`](../fn/fill_between.html) | A filled band between two curves: `x`, `y_lo`, `y_hi` are equal-length numeric Vecs giving the lower and upper boundary at each `x`, e.g. a confidence interval | [Plotting](plotting.md) |
| [`fill_missing`](../fn/fill_missing.html) | `x` with the missing samples patched, same length, so a `Signal` keeps its `Fs` | [Noise](noise.md) |
| [`filter`](../fn/filter.html) | Returns a `Table` containing only the rows where `mask` is `true` | [Collections & strings](collections-strings.md) |
| [`filter_ba`](../fn/filter_ba.html) | Returns a same-length vector or `Signal` | [Signal processing](signal-processing.md) |
| [`filter_init`](../fn/filter_init.html) | Returns a `Model` (kind `"filter_state"`) holding fresh all-zero streaming state for sample-at-a-time filtering: field is an `(n_sections, 2)` biquad-state matrix for an IIR (`sos`) filter,.. | [Signal processing](signal-processing.md) |
| [`filter_next`](../fn/filter_next.html) | Returns a new `"filter_state"` `Model` (field `y`, a number, holds the filtered output) | [Signal processing](signal-processing.md) |
| [`filtfilt`](../fn/filtfilt.html) | Returns a same-length vector or `Signal`, same `Fs`-preserving convention as `sosfilt` | [Signal processing](signal-processing.md) |
| [`find`](../fn/find.html) | One argument: identical to `where(mask)` — the indices where a boolean `mask` is true, as a `Vec` | [Core maths](core-math.md) |
| [`find_clipping`](../fn/find_clipping.html) | Same detection as `is_clipped`, same keywords, but returns *where* | [Signal processing](signal-processing.md) |
| [`find_edges`](../fn/find_edges.html) | The reported positions are taken at `level` either way, so switching hysteresis on to reject glitches does not move the measurements it protects | [Signal processing](signal-processing.md) |
| [`find_missing`](../fn/find_missing.html) | The indices of the missing samples, as a number vector | [Noise](noise.md) |
| [`find_outliers`](../fn/find_outliers.html) | The indices of the samples a criterion flags, as a number vector (same shape of answer as `find_peaks`) | [Noise](noise.md) |
| [`find_peaks`](../fn/find_peaks.html) | Kwarg names match SciPy; the original `min_height`/`min_distance`/`min_prominence` names are still accepted as aliases | [Signal processing](signal-processing.md) |
| [`find_pulses`](../fn/find_pulses.html) | Returns a `Model` (kind `"pulses"`) with fields `starts`, `stops` (vectors of integer sample indices — `stops`, not `ends`, because `end` is a Qu keyword), `widths` (a vector of times:.. | [Signal processing](signal-processing.md) |
| [`find_trigger`](../fn/find_trigger.html) | Returns a vector of 0-indexed integer sample indices, one per crossing, each being the first sample of the new state — a rising crossing at `i` means `x[i-1] < level <= x[i]` | [Signal processing](signal-processing.md) |
| [`find_zero_crossings`](../fn/find_zero_crossings.html) | `find_trigger` at level zero. The default is both directions, unlike `find_trigger`'s own default: the zero-crossing rate is a count of every sign change, so `len(find_zero_crossings(x))`.. | [Signal processing](signal-processing.md) |
| [`findpeaks`](../fn/findpeaks.html) | MATLAB-style `[peaks, locations] = findpeaks(x, ...)` | [Signal processing](signal-processing.md) |
| [`fir1`](../fn/fir1.html) | Returns a `Model` (kind `"filter"`, field `b`, a length-`n+1` vector) | [Signal processing](signal-processing.md) |
| [`firls`](../fn/firls.html) | Returns a `Model` (kind `"filter"`, field `b`, a length-`n+1` vector) | [Signal processing](signal-processing.md) |
| [`first`](../fn/first.html) | Returns the first (`first`) or last (`last`) element (same element type as `xs` holds), or `none` when `xs` is empty | [Collections & strings](collections-strings.md) |
| [`fit`](../fn/fit.html) | Runs a pipeline Record `pipe` (from `pipeline(...)`) on an `N`-row feature matrix `X` and a length-`N` target vector `y`: applies every transform stage in order, then calls the final.. | [Statistics & ML](statistics-ml.md) |
| [`fit_scaler`](../fn/fit_scaler.html) | Fits and stores scaling parameters from an `N`-row, `D`-column matrix (or vector, or table) `X`, for reuse on new data — the fit-once path, as opposed to the four one-shot verbs above | [Statistics & ML](statistics-ml.md) |
| [`flatten`](../fn/flatten.html) | Returns a flat `List`: one level of nesting removed | [Collections & strings](collections-strings.md) |
| [`flip`](../fn/flip.html) | Reverses row order (`flipud`, `dim=1`) or column order (`fliplr`/`mirror`, `dim=2`) of `A`, a `Str`, `Vec`, `Mat`, or `Image` | [Core maths](core-math.md) |
| [`fliplr`](../fn/fliplr.html) | Reverses row order (`flipud`, `dim=1`) or column order (`fliplr`/`mirror`, `dim=2`) of `A`, a `Str`, `Vec`, `Mat`, or `Image` | [Core maths](core-math.md) |
| [`flipud`](../fn/flipud.html) | Reverses row order (`flipud`, `dim=1`) or column order (`fliplr`/`mirror`, `dim=2`) of `A`, a `Str`, `Vec`, `Mat`, or `Image` | [Core maths](core-math.md) |
| [`floor`](../fn/floor.html) | Standard rounding functions, elementwise: `floor` toward negative infinity, `ceil` toward positive infinity, `round` to the nearest integer (half away from zero) | [Core maths](core-math.md) |
| [`fold`](../fn/fold.html) | Returns the final accumulated value (whatever type `f` produces) | [Collections & strings](collections-strings.md) |
| [`fontfamily`](../fn/fontfamily.html) | Sets the figure-wide font (string `name`, inherited by every title/label/tick/legend) | [Plotting](plotting.md) |
| [`fontsize`](../fn/fontsize.html) | A bare number `n` scales tick/label/title text together (in points); named arguments `tick=`, `label=`, `title=` (each a number, points) override each independently (and win over the.. | [Plotting](plotting.md) |
| [`fopen`](../fn/fopen.html) | Opens a file for reading, writing, or appending | [File I/O](file-io.md) |
| [`foreground_mask`](../fn/foreground_mask.html) | Thresholds `img` at `level`, then applies `imopen(radius)` to remove noise specks | [Images](images.md) |
| [`format`](../fn/format.html) | Interpolation (`{x:.2f}`) reads better inline; `format` is for a pattern held in a variable | [Collections & strings](collections-strings.md) |
| [`forward`](../fn/forward.html) | Runs the forward algorithm on an `hmm` model `hmm` for a length-`T` integer observation-symbol sequence `obs` (each entry a 0-based column index into `emission`) | [Statistics & ML](statistics-ml.md) |
| [`freqz`](../fn/freqz.html) | Returns a length-`n` complex vector (`CVec`), the response `H(e^{jω})` at `n` points from DC to Nyquist inclusive | [Signal processing](signal-processing.md) |
| [`fuzzy_pid_init`](../fn/fuzzy_pid_init.html) | A Mamdani fuzzy PD-type controller's initial state — the classic 2-input (`error`, `error_dot`), 1-output textbook fuzzy inference system | [Signal processing](signal-processing.md) |
| [`fvtool`](../fn/fvtool.html) | MATLAB's Filter Visualization Tool: a 2x2 figure (magnitude response in dB, phase, group delay, impulse response) built from `freqz`/`group_delay`/`sosfilt` | [Signal processing](signal-processing.md) |
| [`fzero`](../fn/fzero.html) | Returns a single number, the root | [Signal processing](signal-processing.md) |

## G

| | | |
|---|---|---|
| [`gain`](../fn/gain.html) | Returns the same shape as `x`, scaled by `10^(db/20)` — the AMPLITUDE convention, the same factor of 20 as `db2mag` | [Signal processing](signal-processing.md) |
| [`gamma`](../fn/gamma.html) | The gamma function (`gamma(n) = (n-1)!` for a positive integer `n`) and its natural log (`lgamma(x) = ln\|gamma(x)\|`) | [Core maths](core-math.md) |
| [`gaussian_process`](../fn/gaussian_process.html) | Gaussian-process regression on an `N`-row, `D`-column matrix `X` and a length-`N` target `y` | [Statistics & ML](statistics-ml.md) |
| [`generate`](../fn/generate.html) | Runs greedy (argmax, no sampling knobs, no temperature) text completion on a model handle `model` from `llm_load`, given a string `prompt` and an integer cap `max_tokens` (default 64) on.. | [Statistics & ML](statistics-ml.md) |
| [`gerischer`](../fn/gerischer.html) | `Zg/sqrt(k + jw)` — a coupled chemical reaction | [Signal processing](signal-processing.md) |
| [`get`](../fn/get.html) | `get` returns the stored value (any type) or `none` if `key` is absent | [Collections & strings](collections-strings.md) |
| [`getenv`](../fn/getenv.html) | Reads one environment variable | [File I/O](file-io.md) |
| [`glob`](../fn/glob.html) | Every file matching a whole-path wildcard, as full paths, sorted — never a directory, which is what makes the content and size filters mean something on every entry | [File I/O](file-io.md) |
| [`gmm_model`](../fn/gmm_model.html) | Gaussian mixture model with `k` components fitted by EM on an `N`-row, `D`-column matrix `X`, warm-started from a k-means run (optional integer `seed=`, and `max_iter` capping the EM.. | [Statistics & ML](statistics-ml.md) |
| [`goertzel`](../fn/goertzel.html) | Returns a single complex scalar for a scalar `k`, or a length-K `CVec` for a vector — the K-point DFT this is named for: K arbitrary points in O(n·K), each independently, instead of every.. | [Signal processing](signal-processing.md) |
| [`goertzel_freq`](../fn/goertzel_freq.html) | Returns a single complex scalar or a length-K `CVec`, same batching as `goertzel` above | [Signal processing](signal-processing.md) |
| [`gpu_matmul`](../fn/gpu_matmul.html) | WebGPU-accelerated matmul via `qu-gpu`, same `A`/`B` shape requirements and `r×c` `Mat` return as `matmul`; only present in builds compiled with `--features gpu` | [Core maths](core-math.md) |
| [`gpu_probe_info`](../fn/gpu_probe_info.html) | Returns a struct-like value with three fields: `.available` (a boolean, whether a GPU backend was found), `.crossover` (a number, the measured matrix/problem size above which the GPU wins.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`grad`](../fn/grad.html) | Takes `loss` — a tracked scalar that is the result of a computation built from `track`ed inputs — plus either a keyword argument `wrt=X` (one tracked tensor `X`) or one or more positional.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`gradient_boosting_model`](../fn/gradient_boosting_model.html) | Regression-only boosted ensemble on an `N`-row, `D`-column `X` and length-`N` `y`: `n_trees` (positive integer) shallow trees fitted in sequence, each to the running residual, scaled by.. | [Statistics & ML](statistics-ml.md) |
| [`graph`](../fn/graph.html) | Optional named `directed` (`bool`, default `false`) | [Collections & strings](collections-strings.md) |
| [`grayscale`](../fn/grayscale.html) | Converts RGB → luma via ITU-R BT.601 weights (matches `plotting.rs`'s `contrast_text_color`) | [Images](images.md) |
| [`grep`](../fn/grep.html) | Returns a `List` of the matching (or non-matching) LINES — `regex_find_all` returns the matched substrings themselves, a different thing | [Collections & strings](collections-strings.md) |
| [`grid`](../fn/grid.html) | Calling `grid(...)` as a function is accepted but is a no-op — only the bare command form actually changes anything | [Plotting](plotting.md) |
| [`gridworld_env`](../fn/gridworld_env.html) | Builds an `n x n` grid environment (integer `n`), with `start`/`goal` (optional integer state indices, defaulting to opposite corners), `step_reward` (number, default -1, given on every.. | [Statistics & ML](statistics-ml.md) |
| [`group_by_agg`](../fn/group_by_agg.html) | Returns a `Table`: one row per distinct value of `group_col`, with `value_col` aggregated | [Collections & strings](collections-strings.md) |
| [`group_delay`](../fn/group_delay.html) | Returns a length-`n` real vector, `-dφ/dω` in samples, same `n`-point/frequency-axis convention as `freqz` | [Signal processing](signal-processing.md) |
| [`groupbar`](../fn/groupbar.html) | Grouped (side-by-side) bar chart: `x` (Vec, category positions) and `y` (Mat or list of Vecs, one series per group of bars) | [Plotting](plotting.md) |
| [`gru_cell`](../fn/gru_cell.html) | One GRU timestep: `x_t` (length-`input_size` input vector), `h_prev` (length-`hidden_size` previous hidden state), `weights` (a Record with the nine gate fields `gru_init` produces) | [Statistics & ML](statistics-ml.md) |
| [`gru_forward`](../fn/gru_forward.html) | Unrolls `gru_cell` over a whole sequence: `x_sequence` an `(input_size, seq_len)` matrix, `h0` a length-`hidden_size` initial hidden state, `weights` the same Record `gru_cell` takes | [Statistics & ML](statistics-ml.md) |
| [`gru_init`](../fn/gru_init.html) | Builds fresh GRU weights for an input of size `input_size` (integer) and a hidden state of size `hidden_size` (integer): Xavier-scaled weight matrices and zero biases for the update, reset.. | [Statistics & ML](statistics-ml.md) |

## H

| | | |
|---|---|---|
| [`hamming`](../fn/hamming.html) | Returns a length-`n` real vector | [Signal processing](signal-processing.md) |
| [`hamming74_decode`](../fn/hamming74_decode.html) | Computes each block's syndrome, flips the bit it names, and recovers the data | [Signal processing](signal-processing.md) |
| [`hamming74_encode`](../fn/hamming74_encode.html) | Hamming(7,4) systematic encoder, 7 bits out per 4 in, laid out `[p1, p2, d1, p4, d2, d3, d4]` with the parity bits at the power-of-two positions — the layout that makes a nonzero syndrome.. | [Signal processing](signal-processing.md) |
| [`hampel`](../fn/hampel.html) | Outlier removal: replaces a sample by the local median only when it is more than `n_sigma` robust deviations (`1.4826 × MAD`) from it | [Noise](noise.md) |
| [`hann`](../fn/hann.html) | Returns a length-`n` real vector | [Signal processing](signal-processing.md) |
| [`has_edge`](../fn/has_edge.html) | `has_edge(g, a, b)` returns a `bool`; `neighbors(g, name)` returns a `List` of `Str` node ids | [Collections & strings](collections-strings.md) |
| [`has_key`](../fn/has_key.html) | Returns a `bool`: whether the dict has that key, without reading it | [Collections & strings](collections-strings.md) |
| [`havriliak_negami`](../fn/havriliak_negami.html) | `Rh/(1 + (jw tau)^a)^g` | [Signal processing](signal-processing.md) |
| [`head`](../fn/head.html) | Returns the first (`head`) or last (`tail`) `n` elements/rows, same type as `s` | [Collections & strings](collections-strings.md) |
| [`heatmap`](../fn/heatmap.html) | Grid of colored cells: `matrix` is a Mat, each cell coloured by its own value scaled to the matrix's own min/max | [Plotting](plotting.md) |
| [`help`](../fn/help.html) | Prints what is known about that name directly to the console and returns `none` — that it exists, near matches when it does not (e.g. `help("not_a_real_fn")` suggests close spellings), and.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`hessian`](../fn/hessian.html) | Takes a scalar-valued function `f` and a point `x`, and returns the `n x n` matrix of second derivatives, symmetrized | [REPL & diagnostics](repl-diagnostics.md) |
| [`hex2dec`](../fn/hex2dec.html) | Reads a hexadecimal digit string as a number | [Collections & strings](collections-strings.md) |
| [`hexbin`](../fn/hexbin.html) | The plane tiled with hexagons, each shaded by how many points fell in it — for the case a scatter cannot do, where past a few thousand marks the picture saturates into a blob | [Plotting](plotting.md) |
| [`high_time`](../fn/high_time.html) | Returns a single number: the sum of the widths of every complete pulse above (`high_time`) or below (`low_time`) the threshold, in seconds for a `Signal` and samples otherwise | [Signal processing](signal-processing.md) |
| [`hilbert`](../fn/hilbert.html) | Returns a length-N complex vector (`CVec`), the analytic signal `x + j*H[x]` | [Signal processing](signal-processing.md) |
| [`hist`](../fn/hist.html) | Bins `samples` (Vec of numbers) into equal-width buckets and renders as bars; `bins` (integer, positional or `bins=` keyword) defaults to `ceil(sqrt(n))` for `n = len(samples)` | [Plotting](plotting.md) |
| [`histeq`](../fn/histeq.html) | Global histogram equalization: builds a BT.601-luma cumulative distribution and remaps intensities to spread across the full range | [Images](images.md) |
| [`histogram`](../fn/histogram.html) | Bins `samples` (Vec of numbers) into equal-width buckets and renders as bars; `bins` (integer, positional or `bins=` keyword) defaults to `ceil(sqrt(n))` for `n = len(samples)` | [Plotting](plotting.md) |
| [`hline`](../fn/hline.html) | Draws a horizontal reference line spanning the whole panel at `y` (number, data units) | [Plotting](plotting.md) |
| [`hmm`](../fn/hmm.html) | Builds a discrete hidden Markov model from an `(S, S)` row-stochastic hidden-state transition matrix `transition`, an `(S, O)` row-stochastic emission matrix `emission` (`O` the number of.. | [Statistics & ML](statistics-ml.md) |
| [`hourly_profile`](../fn/hourly_profile.html) | Returns a length-24 real vector, one bin per hour-of-day | [Signal processing](signal-processing.md) |
| [`hsl`](../fn/hsl.html) | Builds a colour from hue `h` (number, degrees, 0–360), saturation `s` (number, 0–1) and lightness `l` (number, 0–1, symmetric about 0.5 — "the same colour, lighter" is one number going up) | [Plotting](plotting.md) |
| [`hstack`](../fn/hstack.html) | Named equivalents of bracket-literal block concatenation (`[a, b]` horizontally / `[a; b]` vertically) | [Core maths](core-math.md) |
| [`hsv`](../fn/hsv.html) | Builds a colour from hue `h` (number, degrees, 0–360), saturation `s` (number, 0–1) and value/brightness `v` (number, 0–1) | [Plotting](plotting.md) |
| [`html2md`](../fn/html2md.html) | Converts the same subset back to Markdown; malformed HTML degrades to its text rather than erroring | [Collections & strings](collections-strings.md) |
| [`http_get`](../fn/http_get.html) | Issues an HTTP(S) GET request and returns the response body as a string (UTF-8, decoded lossily — the same convention `read_all_text`/text-mode `read_all` already use) | [Concurrency](concurrency.md) |
| [`huffman_decode`](../fn/huffman_decode.html) | Inverse of `huffman_encode`: `enc` is the whole `Record` that call returned, passed back unchanged; `as_str` (named boolean, default `false`) asks for text instead of bytes | [File I/O](file-io.md) |
| [`huffman_encode`](../fn/huffman_encode.html) | Canonical Huffman coding of `data` (a `Str`, or a `Vec` of 0-255 byte values) | [File I/O](file-io.md) |
| [`hum`](../fn/hum.html) | Adds simulated mains pickup to `x`, a length-N signal vector, given the sample rate `fs` in Hz | [Noise](noise.md) |
| [`hurst_exponent`](../fn/hurst_exponent.html) | Returns a single number: 0.5 is a random walk, above that persistent, below it mean-reverting — long-range dependence | [Signal processing](signal-processing.md) |

## I

| | | |
|---|---|---|
| [`idct`](../fn/idct.html) | Returns a length-N real vector | [Signal processing](signal-processing.md) |
| [`identity`](../fn/identity.html) | The `n×n` identity matrix (ones on the diagonal, zeros elsewhere) | [Core maths](core-math.md) |
| [`idft`](../fn/idft.html) | Returns a length-N `CVec`. Inverse of `dft`, same O(n²) direct implementation | [Signal processing](signal-processing.md) |
| [`idwt`](../fn/idwt.html) | Returns a length-`2N` real vector, reconstructing the original signal | [Signal processing](signal-processing.md) |
| [`ifft`](../fn/ifft.html) | Inverse DFT, same underlying kernel as `fft` run in reverse | [Signal processing](signal-processing.md) |
| [`im`](../fn/im.html) | Imaginary component of a `Complex`/`CVec`/`CMat` (`0` elementwise when `x` is real) | [Core maths](core-math.md) |
| [`imadjust`](../fn/imadjust.html) | MATLAB-style contrast stretch, applied identically and independently to each of R/G/B | [Images](images.md) |
| [`imag`](../fn/imag.html) | Imaginary component of a `Complex`/`CVec`/`CMat` (`0` elementwise when `x` is real) | [Core maths](core-math.md) |
| [`image_from_matrix`](../fn/image_from_matrix.html) | Builds a grayscale `Image` directly from `matrix`, a 2-D `Mat` of numbers; each value is rounded and clamped to 0–255 and written to R=G=B for that pixel (matrix rows become image rows,.. | [Images](images.md) |
| [`image_new`](../fn/image_new.html) | Creates a blank solid-color canvas | [Images](images.md) |
| [`imagesc`](../fn/imagesc.html) | False-color raster display of the plain numeric matrix (no pixel buffer produced) — pushes a dense `Heatmap` onto the current plot panel, reusing the same renderer as `heatmap`, just.. | [Images](images.md) |
| [`imbothat`](../fn/imbothat.html) | Bottom-hat: `imclose(img, radius) - img`, extracts small dark features / corrects a slowly-varying dark background | [Images](images.md) |
| [`imclose`](../fn/imclose.html) | Morphological closing (dilate then erode) — fills small background holes without enlarging the overall shape | [Images](images.md) |
| [`imdilate`](../fn/imdilate.html) | Performs binary morphological dilation, growing foreground regions | [Images](images.md) |
| [`imequalize`](../fn/imequalize.html) | Histogram equalisation, spreading the intensities to fill the range (identical operation to `histeq` above) | [Images](images.md) |
| [`imerode`](../fn/imerode.html) | Binary morphological erosion — shrinks foreground regions and removes specks smaller than the element | [Images](images.md) |
| [`imfilter`](../fn/imfilter.html) | Generic 2-D convolution with edge-clamped borders — the escape hatch when `blur`/`sharpen`/`edge_detect`'s fixed kernels aren't enough | [Images](images.md) |
| [`imhist`](../fn/imhist.html) | Computes raw per-bin BT.601-luma intensity counts | [Images](images.md) |
| [`imnoise`](../fn/imnoise.html) | For `"gaussian"`: `mean`/`sigma` are optional named numbers (defaults 0.0/0.05), the additive per-channel Gaussian noise's mean/standard-deviation in normalized `[0,1]` intensity units | [Images](images.md) |
| [`imopen`](../fn/imopen.html) | Morphological opening (erode then dilate) — removes specks smaller than the structuring element while leaving larger shapes their size | [Images](images.md) |
| [`impedance`](../fn/impedance.html) | Returns a complex vector, one impedance per frequency — this is the forward form, which evaluates a circuit model; see *Measured impedance* below for `impedance(voltage, current)`, which.. | [Signal processing](signal-processing.md) |
| [`impedance_from_reflection`](../fn/impedance_from_reflection.html) | Returns `z = z0(1 + Γ)/(1 - Γ)`, the exact inverse | [Signal processing](signal-processing.md) |
| [`impulse`](../fn/impulse.html) | Returns a length-`n` plain vector (not a `Signal`, since no `fs` is given — wrap in `signal(impulse(n), fs)` if a sample rate is needed): a Kronecker delta, all zeros except `amplitude` at.. | [Signal processing](signal-processing.md) |
| [`imrotate`](../fn/imrotate.html) | Rotates an image about its own center | [Images](images.md) |
| [`imscale`](../fn/imscale.html) | Implemented as `imwarp` + `affine_scale` | [Images](images.md) |
| [`imshow`](../fn/imshow.html) | Places it inline in the current plot panel as a native SVG `<image>` element at its own resolution | [Images](images.md) |
| [`imtophat`](../fn/imtophat.html) | The image minus its opening: what the opening removed | [Images](images.md) |
| [`imtranslate`](../fn/imtranslate.html) | Implemented as `imwarp` + `affine_translate` | [Images](images.md) |
| [`imwarp`](../fn/imwarp.html) | Applies `M` to `img` in one resampling pass via inverse mapping | [Images](images.md) |
| [`inch`](../fn/inch.html) | Length units as functions, converting a scalar `v` (millimetres/centimetres/inches/points as named) to the figure's own coordinate space — a tenth of a millimetre, so `mm(1)` is `10`,.. | [Core maths](core-math.md) |
| [`index`](../fn/index.html) | Returns a `Table`: `t` with its row index set to that column, so rows can afterwards be looked up by label | [Collections & strings](collections-strings.md) |
| [`index_of`](../fn/index_of.html) | Returns a `number` (0-based character position of the first occurrence) or `none` if `sub` is absent | [Collections & strings](collections-strings.md) |
| [`indexof`](../fn/indexof.html) | Returns a `number` (0-based index of the first match) or `Nothing` if absent (composes with `??`: `indexof(xs, v) ?? -1`) | [Collections & strings](collections-strings.md) |
| [`inductor`](../fn/inductor.html) | `jwL` | [Signal processing](signal-processing.md) |
| [`input`](../fn/input.html) | Starts the pipeline sugar: `n` (a positive integer) is the number of input features | [Statistics & ML](statistics-ml.md) |
| [`insert`](../fn/insert.html) | Returns a new `Str`; `s` itself is untouched | [Collections & strings](collections-strings.md) |
| [`insert_column`](../fn/insert_column.html) | Returns a NEW `Table`; the original is untouched | [Collections & strings](collections-strings.md) |
| [`insert_row`](../fn/insert_row.html) | Inserts one row into a table at a chosen position | [Collections & strings](collections-strings.md) |
| [`interp1`](../fn/interp1.html) | 1-D interpolation through `(x, y)` (MATLAB's `interp1`) | [Signal processing](signal-processing.md) |
| [`interp2`](../fn/interp2.html) | 2-D grid interpolation (MATLAB's `interp2`): a query exactly at a grid coordinate returns that exact stored value, not an approximation; extrapolation beyond the grid is always on, matching.. | [Signal processing](signal-processing.md) |
| [`interpolate_at`](../fn/interpolate_at.html) | A `Signal`-aware `interp1`: instead of an explicit `x`, the signal's OWN implicit time axis (`sig.t`, i.e. `i/Fs`) is used as `x` and `sig`'s samples as `y` | [Signal processing](signal-processing.md) |
| [`interpolate_nan`](../fn/interpolate_nan.html) | Exactly `fill_missing(x, method="linear")` — the same engine, not a second implementation — kept under its own name because that is the operation people go looking for by name | [Noise](noise.md) |
| [`inv`](../fn/inv.html) | True inverse of `A` (a square `Mat` or `CMat`); requires full rank, errors otherwise | [Core maths](core-math.md) |
| [`inverse_transform`](../fn/inverse_transform.html) | Undoes a fitted scaler Record `scaler`'s transform on an already-scaled matrix/vector/table `newX`, returning it to the original units | [Statistics & ML](statistics-ml.md) |
| [`iqr`](../fn/iqr.html) | Interquartile range: `percentile(x,75) - percentile(x,25)` — a spread measure robust to outliers, unlike `std`/`var` | [Core maths](core-math.md) |
| [`irfft`](../fn/irfft.html) | Inverse of `rfft`. `half_spectrum` is a length-`floor(n/2)+1` complex vector (`CVec`); `n` (required number), the real output length, can't be recovered from the half-spectrum's length.. | [Signal processing](signal-processing.md) |
| [`is_clipped`](../fn/is_clipped.html) | Returns a `Bool`: does `x` contain a flat run at an extreme | [Signal processing](signal-processing.md) |
| [`is_empty`](../fn/is_empty.html) | Whether the fixed-capacity ring buffer `q` (a `Fifo` handle created by `fifo(capacity)`) currently holds nothing, or has no room left for another `push` without evicting the oldest element | [Core maths](core-math.md) |
| [`is_full`](../fn/is_full.html) | Whether the fixed-capacity ring buffer `q` (a `Fifo` handle created by `fifo(capacity)`) currently holds nothing, or has no room left for another `push` without evicting the oldest element | [Core maths](core-math.md) |
| [`is_stable`](../fn/is_stable.html) | Returns a boolean, `true` iff every pole lies strictly inside the unit circle — the standard causal-LTI stability criterion | [Signal processing](signal-processing.md) |
| [`isolation_forest`](../fn/isolation_forest.html) | Anomaly detection on an `N`-row, `D`-column matrix `X` by random partitioning (Liu, Ting & Zhou 2008): grows `n_trees` (integer, default 100) trees, each on its own subsample of.. | [Statistics & ML](statistics-ml.md) |
| [`items`](../fn/items.html) | Returns a `List`: its keys (`keys`), its values (`values`), or `(key, value)` tuples (`items`) — insertion order, not sorted | [Collections & strings](collections-strings.md) |

## J

| | | |
|---|---|---|
| [`jacobian`](../fn/jacobian.html) | Takes a function `f` and a point `x` (number, vector, or matrix), and returns the full Jacobian matrix of `f` at `x` as an `m x n` matrix, where `J[i][j]` is the derivative of output.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`join`](../fn/join.html) | Two unrelated forms selected by the argument's type | [Concurrency](concurrency.md) |
| [`js_exec`](../fn/js_exec.html) | Runs `code` (string, source in the guest language) as a subprocess | [File I/O](file-io.md) |
| [`json2csv`](../fn/json2csv.html) | `str` (string, JSON text) is converted to CSV text: exactly `csvify(parse_json(str))`. Returns a string | [File I/O](file-io.md) |
| [`json2xml`](../fn/json2xml.html) | `str` (string, JSON text) is converted to XML: exactly `xmlify(parse_json(str))`. Returns a string | [File I/O](file-io.md) |
| [`jsonify`](../fn/jsonify.html) | Converts any `value` (any `Value` — number, string, `Vec`, `Table`, `Record`, ...) to a JSON string — exactly the `save`/`load` on-disk shape (see the link above: a `{"type": ..., ...}`.. | [File I/O](file-io.md) |

## K

| | | |
|---|---|---|
| [`k_fold`](../fn/k_fold.html) | Splits an `N`-row feature matrix `X` and a length-`N` target vector `y` into an integer number of `folds` cross-validation folds | [Statistics & ML](statistics-ml.md) |
| [`kaiser`](../fn/kaiser.html) | Returns a length-`n` real vector, `w[i] = I0(beta*sqrt(1-((2i/(n-1))-1)^2)) / I0(beta)` | [Signal processing](signal-processing.md) |
| [`kalman_init`](../fn/kalman_init.html) | Initial linear-Kalman state: `x0` a length-`D` initial state estimate vector, `P0` a `(D, D)` initial covariance matrix | [Statistics & ML](statistics-ml.md) |
| [`kapur_threshold`](../fn/kapur_threshold.html) | Same "compute, don't apply" shape as `otsu_threshold`, but using Kapur's entropy-maximizing criterion instead of Otsu's between-class variance — tends to handle unequal class sizes better | [Images](images.md) |
| [`keys`](../fn/keys.html) | Returns a `List`: its keys (`keys`), its values (`values`), or `(key, value)` tuples (`items`) — insertion order, not sorted | [Collections & strings](collections-strings.md) |
| [`kmeans`](../fn/kmeans.html) | Lloyd's algorithm on an `N`-row, `D`-column data matrix `X`, partitioning it into the integer number of clusters `k`, starting from randomly chosen initial centroids (an optional integer.. | [Statistics & ML](statistics-ml.md) |
| [`kmeans_centers`](../fn/kmeans_centers.html) | The same Lloyd's-algorithm run as `kmeans` on `X`/`k`/`seed=`, returning the `(k, D)` centroid matrix instead of the labels | [Statistics & ML](statistics-ml.md) |
| [`kmeans_model`](../fn/kmeans_model.html) | k-means as a reusable handle: fits on an `N`-row, `D`-column matrix `X` into `k` clusters (optional integer `seed=`), so `.predict(Xnew)` later assigns new rows to the nearest centroid.. | [Statistics & ML](statistics-ml.md) |
| [`kmedians_model`](../fn/kmedians_model.html) | The outlier-resistant sibling of `kmeans_model`, same `X`/`k`/`seed=` shape: clusters by L1 distance around coordinate-wise median centres instead of means | [Statistics & ML](statistics-ml.md) |
| [`kmedoids_model`](../fn/kmedoids_model.html) | PAM clustering on the same `X`/`k`/`seed=` shape as `kmeans_model` — every one of the `k` centres is an actual row of `X`, never an average | [Statistics & ML](statistics-ml.md) |
| [`knn_model`](../fn/knn_model.html) | k-nearest-neighbours on an `N`-row, `D`-column `X` and a length-`N` target `y`, with integer neighbour count `k`, an optional `metric` string (distance function name), and `kind`.. | [Statistics & ML](statistics-ml.md) |
| [`kurtosis`](../fn/kurtosis.html) | Fourth standardized moment, population form, of a length-`N` numeric vector `x` | [Statistics & ML](statistics-ml.md) |

## L

| | | |
|---|---|---|
| [`lab`](../fn/lab.html) | Builds a colour from CIE L\*a\*b\* coordinates: `L` (number, lightness, 0–100), `a` and `b` (numbers, roughly -128..127, green–red and blue–yellow axes) | [Plotting](plotting.md) |
| [`label_blobs`](../fn/label_blobs.html) | Connected-component labelling: every separate region gets its own integer (identical operation to `bwlabel` above) | [Images](images.md) |
| [`last`](../fn/last.html) | Returns the first (`first`) or last (`last`) element (same element type as `xs` holds), or `none` when `xs` is empty | [Collections & strings](collections-strings.md) |
| [`last_index_of`](../fn/last_index_of.html) | Returns a `number` (0-based character position of the last occurrence, searching from the end) or `none` if absent — so a miss cannot be mistaken for a position | [Collections & strings](collections-strings.md) |
| [`layer_norm`](../fn/layer_norm.html) | Normalizes each row of an `N`-row, `D`-column matrix `x` independently to zero mean and unit variance, then applies a learned length-`D` per-feature scale `gamma` and shift `beta`, with.. | [Statistics & ML](statistics-ml.md) |
| [`lcase`](../fn/lcase.html) | BASIC's names for `upper`/`lower`; `toupper`/`tolower` are C's names for the same thing | [Collections & strings](collections-strings.md) |
| [`least_squares`](../fn/least_squares.html) | Nonlinear least squares on a residual function you named, via Levenberg-Marquardt | [Core maths](core-math.md) |
| [`left`](../fn/left.html) | The first `n` characters of a string | [Collections & strings](collections-strings.md) |
| [`legend`](../fn/legend.html) | Shows or hides the key for the current panel | [Plotting](plotting.md) |
| [`len`](../fn/len.html) | Returns a `number`: the current element count (via the same generic length dispatch every collection uses) | [Collections & strings](collections-strings.md) |
| [`length`](../fn/length.html) | Total element count of `x`: `1` for a scalar, its length for a `Vec`/`CVec`/`Signal`, `rows*cols` for a `Mat`, a Unicode-aware character count for a `Str`, and the element/node count for.. | [Core maths](core-math.md) |
| [`lfilter`](../fn/lfilter.html) | Returns a same-length vector or `Signal` | [Signal processing](signal-processing.md) |
| [`lgamma`](../fn/lgamma.html) | The gamma function (`gamma(n) = (n-1)!` for a positive integer `n`) and its natural log (`lgamma(x) = ln\|gamma(x)\|`) | [Core maths](core-math.md) |
| [`like`](../fn/like.html) | BASIC's wildcard match, anchored at both ends: `*` any run, `?` any one character, `#` any digit, `[abc]`/`[!abc]` a set | [Collections & strings](collections-strings.md) |
| [`lines`](../fn/lines.html) | Splits on either line ending, with no spurious empty last element when the text ends in a newline | [Collections & strings](collections-strings.md) |
| [`linked_list`](../fn/linked_list.html) | Returns a `linked_list` handle: a new, empty double-ended list | [Collections & strings](collections-strings.md) |
| [`linspace`](../fn/linspace.html) | Returns a length-`n` real vector, `n` linearly spaced points from `a` to `b` inclusive — the standard companion for building a time or frequency axis to plot alongside a.. | [Signal processing](signal-processing.md) |
| [`list_dir`](../fn/list_dir.html) | The bare names inside `path` (a `Str`), sorted — `"a.qu"`, not `"tools/a.qu"` | [File I/O](file-io.md) |
| [`list_files`](../fn/list_files.html) | Every file matching a whole-path wildcard, as full paths, sorted — never a directory, which is what makes the content and size filters mean something on every entry | [File I/O](file-io.md) |
| [`listdir`](../fn/listdir.html) | The bare names inside `path` (a `Str`), sorted — `"a.qu"`, not `"tools/a.qu"` | [File I/O](file-io.md) |
| [`listen_pool`](../fn/listen_pool.html) | Binds `port` and blocks forever (this call never returns under normal operation), accepting job requests over TCP and running only functions named in `allow` | [Concurrency](concurrency.md) |
| [`llm_load`](../fn/llm_load.html) | Loads a chat model, identified by `name`: a string, either a bundled name like `"tinyllama"` or a local filesystem path to a `.gguf` weights file with a `tokenizer.json` beside it | [Statistics & ML](statistics-ml.md) |
| [`lms_init`](../fn/lms_init.html) | The least-mean-squares (LMS) adaptive FIR filter's initial state | [Signal processing](signal-processing.md) |
| [`ln`](../fn/ln.html) | Natural logarithm, elementwise (`log` is an alias for `ln` — it is not base-10; see `log10` for that) | [Core maths](core-math.md) |
| [`load`](../fn/load.html) | Restores every variable a `save`/`save_all` call wrote | [File I/O](file-io.md) |
| [`load_image`](../fn/load_image.html) | Reads the file from disk and decodes it — PNG, BMP, JPEG and TIFF | [Images](images.md) |
| [`load_model`](../fn/load_model.html) | Reads a model previously written by `save_model` back from the HDF5 file at the string `path` | [Statistics & ML](statistics-ml.md) |
| [`load_svg`](../fn/load_svg.html) | Reads an SVG file back. Returns a `Record` with fields `width`, `height` (from the `width`/`height` attributes, falling back to the `viewBox` when those are absent or a percentage; absolute.. | [Images](images.md) |
| [`log`](../fn/log.html) | Natural logarithm, elementwise (`log` is an alias for `ln` — it is not base-10; see `log10` for that) | [Core maths](core-math.md) |
| [`log10`](../fn/log10.html) | Base-10 / base-2 logarithm, elementwise, same domain behavior as `ln` on a non-positive element | [Core maths](core-math.md) |
| [`log1p`](../fn/log1p.html) | `ln(1 + x)`, computed so it stays accurate for `x` near zero (`log1p(1e-15)` does not underflow to exactly `0`, unlike naively computing `1.0 + x` first) | [Core maths](core-math.md) |
| [`log2`](../fn/log2.html) | Base-10 / base-2 logarithm, elementwise, same domain behavior as `ln` on a non-positive element | [Core maths](core-math.md) |
| [`logistic_model`](../fn/logistic_model.html) | Binary logistic regression on an `N`-row, `D`-column `X` and a length-`N` 0/1 target `y` | [Statistics & ML](statistics-ml.md) |
| [`loglog`](../fn/loglog.html) | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets both axes to log scale | [Plotting](plotting.md) |
| [`logspace`](../fn/logspace.html) | Returns a length-`n` real vector, `n` logarithmically spaced points, `10^linspace(a, b, n)` — used for frequency axes in Bode-style plots and rLKK's default DRT grid | [Signal processing](signal-processing.md) |
| [`logsumexp`](../fn/logsumexp.html) | Numerically stable `log(sum(exp(x)))`, via the usual max-shift so a large input can't overflow `exp` | [Core maths](core-math.md) |
| [`low_time`](../fn/low_time.html) | Returns a single number: the sum of the widths of every complete pulse above (`high_time`) or below (`low_time`) the threshold, in seconds for a `Signal` and samples otherwise | [Signal processing](signal-processing.md) |
| [`lower`](../fn/lower.html) | Unicode-aware lowercase conversion | [Collections & strings](collections-strings.md) |
| [`lr_adaptive`](../fn/lr_adaptive.html) | Learning-rate controllers that wrap a base optimizer rather than replacing it: `lr=` the starting learning rate (number), `patience` an integer number of epochs to wait for improvement.. | [Statistics & ML](statistics-ml.md) |
| [`lr_plateau`](../fn/lr_plateau.html) | Learning-rate controllers that wrap a base optimizer rather than replacing it: `lr=` the starting learning rate (number), `patience` an integer number of epochs to wait for improvement.. | [Statistics & ML](statistics-ml.md) |
| [`lse`](../fn/lse.html) | Numerically stable `log(sum(exp(x)))`, via the usual max-shift so a large input can't overflow `exp` | [Core maths](core-math.md) |
| [`lstm_cell`](../fn/lstm_cell.html) | One LSTM timestep: `x_t` (length-`input_size` input vector), `h_prev`/`c_prev` (length-`hidden_size` previous hidden/cell state vectors), and `weights` (a Record with the twelve gate fields.. | [Statistics & ML](statistics-ml.md) |
| [`lstm_forward`](../fn/lstm_forward.html) | Unrolls `lstm_cell` over a whole sequence: `x_sequence` an `(input_size, seq_len)` matrix (one column per timestep), `h0`/`c0` length-`hidden_size` initial states, `weights` the same.. | [Statistics & ML](statistics-ml.md) |
| [`lstm_init`](../fn/lstm_init.html) | Builds fresh LSTM weights for an input of size `input_size` (integer, number of features per timestep) and a hidden state of size `hidden_size` (integer): Xavier-scaled weight matrices and.. | [Statistics & ML](statistics-ml.md) |
| [`ltrim`](../fn/ltrim.html) | Same parameters as `trim`, restricted to one end only (`ltrim` the left, `rtrim` the right) | [Collections & strings](collections-strings.md) |
| [`lu`](../fn/lu.html) | LU decomposition with partial pivoting of a square `n×n` `Mat` `A`, `P·A = L·U` | [Core maths](core-math.md) |

## M

| | | |
|---|---|---|
| [`mae`](../fn/mae.html) | The regression-error trio, each taking a length-`N` vector of true values `actual` and a length-`N` vector of predictions `predicted`, and returning a single number: `mae` the mean absolute.. | [Statistics & ML](statistics-ml.md) |
| [`mag2db`](../fn/mag2db.html) | Returns the same shape: `20*log10(x)` | [Signal processing](signal-processing.md) |
| [`magnitude`](../fn/magnitude.html) | An exact alias of `abs` — the same match arm, so the two names cannot drift apart | [Signal processing](signal-processing.md) |
| [`make_file`](../fn/make_file.html) | Creates an empty file at `path` (a `Str`), truncating it if it is already there — the same promise `fopen(path, "w")` makes, so the two agree | [File I/O](file-io.md) |
| [`map`](../fn/map.html) | Returns a `List`: `f` applied to every element | [Collections & strings](collections-strings.md) |
| [`markers`](../fn/markers.html) | Added in v0.2.4. The signal's markers as a list of records with `time` and `label`, in time order | [Signal processing](signal-processing.md) |
| [`markov_chain`](../fn/markov_chain.html) | Builds a discrete Markov chain from an `(S, S)` row-stochastic transition matrix `P` (every row must sum to 1 — rows that don't are rejected outright, never quietly renormalized) and an.. | [Statistics & ML](statistics-ml.md) |
| [`matlab_exec`](../fn/matlab_exec.html) | The same contract as `python_exec`/`js_exec`, run through a local MATLAB installation instead: `code`'s result is written to an injected `qu_output_path` variable (no leading underscore —.. | [File I/O](file-io.md) |
| [`matmul`](../fn/matmul.html) | Matrix multiplication of `A` (`r×k`) and `B` (`k×c`), both `Mat` (inner dimensions must agree) | [Core maths](core-math.md) |
| [`max`](../fn/max.html) | In the one-argument form on a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead, returning a length-`cols`/`rows` `Vec` rather than a single scalar | [Core maths](core-math.md) |
| [`maxpool2d`](../fn/maxpool2d.html) | Downsamples an `(H, W)` matrix `x` by taking the largest value in each `size` x `size` window (integer `size`), moving `stride` positions at a time (integer, defaults to `size` —.. | [Statistics & ML](statistics-ml.md) |
| [`md2html`](../fn/md2html.html) | Converts headings, paragraphs, lists, fenced code with its language, blockquotes, pipe tables, rules, and inline code/bold/italic/links/images | [Collections & strings](collections-strings.md) |
| [`mean`](../fn/mean.html) | Arithmetic mean of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal` | [Core maths](core-math.md) |
| [`measure_snr`](../fn/measure_snr.html) | Computes the achieved signal-to-noise ratio by treating `noisy - clean` as the noise | [Noise](noise.md) |
| [`medfilt`](../fn/medfilt.html) | Replaces each sample by the median of `window` samples centred on it | [Noise](noise.md) |
| [`medfilt2`](../fn/medfilt2.html) | Synonyms for `medfilt` at the Qu level: `x` is a length-N number vector or an `Image`, `window` an optional positional/named odd integer, default `3` | [Noise](noise.md) |
| [`median`](../fn/median.html) | Middle order statistic of `x` (a scalar, `Vec`, or `Mat`/`Signal`, flattened): the middle element for an odd-length input, the average of the two middle elements when `len(x)` is even | [Core maths](core-math.md) |
| [`median_filter`](../fn/median_filter.html) | Synonyms for `medfilt` at the Qu level: `x` is a length-N number vector or an `Image`, `window` an optional positional/named odd integer, default `3` | [Noise](noise.md) |
| [`mel_spectrogram`](../fn/mel_spectrogram.html) | Mel-scale spectrogram: an `stft` power spectrogram folded onto perceptually-spaced bands by triangular filters laid out on `mel = 2595*log10(1+f/700)` | [Signal processing](signal-processing.md) |
| [`mem_usage`](../fn/mem_usage.html) | Returns this process's resident-set size right now, in bytes (a `Num`) — what THIS run has actually mapped in, not the machine's total memory (`sysinfo()`'s job) | [Concurrency](concurrency.md) |
| [`meshgrid`](../fn/meshgrid.html) | The two coordinate matrices a surface or contour needs, from two axis vectors `x` (length `nx`) and `y` (length `ny`) | [Core maths](core-math.md) |
| [`metadata`](../fn/metadata.html) | Added in v0.2.4. Returns a `Record` of the signal's metadata | [Signal processing](signal-processing.md) |
| [`mid`](../fn/mid.html) | Plain aliases of the same implementation (`mid` exists purely for BASIC/Excel discoverability) | [Collections & strings](collections-strings.md) |
| [`min`](../fn/min.html) | In the one-argument form on a `Mat`, the optional `axis=` keyword (`0` or `1`) reduces along just that dimension instead, returning a length-`cols`/`rows` `Vec` rather than a single scalar | [Core maths](core-math.md) |
| [`minimize`](../fn/minimize.html) | Returns a `Model` (kind `"minimize"`) with fields `params` (length-P vector), `value` (a number), `converged` (a boolean) | [Signal processing](signal-processing.md) |
| [`minutely_profile`](../fn/minutely_profile.html) | Returns a length-60 real vector | [Signal processing](signal-processing.md) |
| [`mirror`](../fn/mirror.html) | Reverses row order (`flipud`, `dim=1`) or column order (`fliplr`/`mirror`, `dim=2`) of `A`, a `Str`, `Vec`, `Mat`, or `Image` | [Core maths](core-math.md) |
| [`mismatch_loss`](../fn/mismatch_loss.html) | Returns `-10 log10(1 - \|Γ\|²)` in dB: the incident power not delivered to the load | [Signal processing](signal-processing.md) |
| [`mkdir`](../fn/mkdir.html) | Creates the directory `path` (a `Str`), including any missing parents — `mkdir -p`, not bare `mkdir` | [File I/O](file-io.md) |
| [`mlp_classifier`](../fn/mlp_classifier.html) | Shortcut that builds a `dense_layer(..., activation="relu")` for every size in `hidden_dims` (a vector of positive integers, e.g. `[64, 32]`) followed by a final `dense_layer(...,.. | [Statistics & ML](statistics-ml.md) |
| [`mm`](../fn/mm.html) | Length units as functions, converting a scalar `v` (millimetres/centimetres/inches/points as named) to the figure's own coordinate space — a tenth of a millimetre, so `mm(1)` is `10`,.. | [Core maths](core-math.md) |
| [`mmap_len`](../fn/mmap_len.html) | Reports the mapped file's size | [File I/O](file-io.md) |
| [`mmap_open`](../fn/mmap_open.html) | Memory-maps `path` (string, filesystem path) read-only | [File I/O](file-io.md) |
| [`mmap_read`](../fn/mmap_read.html) | Reads a byte range out of a mapping without moving any cursor | [File I/O](file-io.md) |
| [`mod`](../fn/mod.html) | Returns a `number`: the remainder with the sign of the divisor, so `-1 mod 3` is 2 rather than -1 | [Collections & strings](collections-strings.md) |
| [`mode`](../fn/mode.html) | The most frequent value in a numeric or categorical vector `x` of length `N` | [Statistics & ML](statistics-ml.md) |
| [`monte_carlo`](../fn/monte_carlo.html) | Returns a length-`n` vector collecting every result | [Signal processing](signal-processing.md) |
| [`monthly_profile`](../fn/monthly_profile.html) | Returns a length-12 real vector, one bin per equal-width twelfth of a Julian year | [Signal processing](signal-processing.md) |
| [`move`](../fn/move.html) | Exactly `predict(dt)` on a `kind="particle_tracker"` state — the row above — under a verb that says what the step is: for this filter the predict step really is a concrete constant-velocity.. | [Statistics & ML](statistics-ml.md) |
| [`move_file`](../fn/move_file.html) | Moves `src` to `dst`, falling back to copy-then-remove-the-original when a plain rename fails (e.g. across drives) — the `on_exists` collision check happens first either way, so the.. | [File I/O](file-io.md) |
| [`mse`](../fn/mse.html) | The regression-error trio, each taking a length-`N` vector of true values `actual` and a length-`N` vector of predictions `predicted`, and returning a single number: `mae` the mean absolute.. | [Statistics & ML](statistics-ml.md) |
| [`mtimes`](../fn/mtimes.html) | Matrix multiplication of `A` (`r×k`) and `B` (`k×c`), both `Mat` (inner dimensions must agree) | [Core maths](core-math.md) |
| [`mul`](../fn/mul.html) | The operators as named functions, so an operation can be passed where a name is what you can pass | [Core maths](core-math.md) |
| [`multi_head_attention`](../fn/multi_head_attention.html) | Runs several attention heads over the same `(N, D)` input `x` in parallel: `heads` is a tuple of `n_heads` (integer) Records, each with fields `wq`, `wk`, `wv` (per-head projection.. | [Statistics & ML](statistics-ml.md) |
| [`multi_otsu`](../fn/multi_otsu.html) | The `n_classes - 1`-threshold generalization of `otsu_threshold` | [Images](images.md) |
| [`multisine`](../fn/multisine.html) | Returns a length-`n` `Signal`. Sum of cosines at the given frequencies/amplitudes | [Signal processing](signal-processing.md) |
| [`multithreshold`](../fn/multithreshold.html) | Quantizes into `len(levels)+1` bands at the given cut points | [Images](images.md) |
| [`mutex`](../fn/mutex.html) | Creates a shared mutable cell holding `initial` (any value, of any type — the mutex has no fixed element type) | [Concurrency](concurrency.md) |
| [`mutex_add`](../fn/mutex_add.html) | Atomic read-modify-write in a single lock acquisition (unlike `mutex_set(m, mutex_get(m) + delta)`, which would race) | [Concurrency](concurrency.md) |
| [`mutex_get`](../fn/mutex_get.html) | Reads the current value out of mutex handle `m` | [Concurrency](concurrency.md) |
| [`mutex_set`](../fn/mutex_set.html) | Overwrites the value stored in mutex handle `m` with `v` (any value, any type), discarding the old one unconditionally — no read of the previous value is involved | [Concurrency](concurrency.md) |
| [`mutex_update`](../fn/mutex_update.html) | Atomic read-modify-write via an arbitrary user function called with the current value as its one argument — for accumulations `mutex_add` can't express (a running max, an append-only log) | [Concurrency](concurrency.md) |
| [`mutual_info_classif`](../fn/mutual_info_classif.html) | k-nearest-neighbour mutual information between each column of an `N`-row, `D`-column `X` and a length-`N` discrete label vector `y`, using `k` neighbours (integer, default 3) | [Statistics & ML](statistics-ml.md) |
| [`mvnpdf`](../fn/mvnpdf.html) | Multivariate Gaussian density at a length-`D` point `x`, with a length-`D` mean vector `mu` and a `(D, D)` covariance matrix `Sigma`, evaluated through `Sigma`'s Cholesky factor rather than.. | [Statistics & ML](statistics-ml.md) |

## N

| | | |
|---|---|---|
| [`nadam`](../fn/nadam.html) | The Adam family of per-parameter adaptive-learning-rate optimizers, given `params` (a Tensor to optimize) and a learning rate `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`naive_bayes_model`](../fn/naive_bayes_model.html) | Gaussian naive Bayes classifier on an `N`-row, `D`-column `X` and a length-`N` categorical/integer target `y`, scored in log space so a product of small per-feature densities cannot.. | [Statistics & ML](statistics-ml.md) |
| [`nand`](../fn/nand.html) | Returns a `bool`: logical NAND | [Collections & strings](collections-strings.md) |
| [`ncol`](../fn/ncol.html) | Returns a `number`: the column count | [Collections & strings](collections-strings.md) |
| [`neighbors`](../fn/neighbors.html) | `has_edge(g, a, b)` returns a `bool`; `neighbors(g, name)` returns a `List` of `Str` node ids | [Collections & strings](collections-strings.md) |
| [`nesterov_sgd`](../fn/nesterov_sgd.html) | Plain stochastic gradient descent (`sgd`) and its look-ahead variant (`nesterov_sgd`, which evaluates the gradient after applying the momentum step rather than before it), given `params` (a.. | [Statistics & ML](statistics-ml.md) |
| [`newton`](../fn/newton.html) | Returns a single number, the root | [Signal processing](signal-processing.md) |
| [`nmf`](../fn/nmf.html) | Non-negative matrix factorization `X ≈ W H` of an `N`-row, `D`-column non-negative matrix `X`, by Lee & Seung's multiplicative updates on the Frobenius objective (a negative or non-finite.. | [Statistics & ML](statistics-ml.md) |
| [`nnls`](../fn/nnls.html) | Non-negative least squares by Lawson-Hanson, for when a negative coefficient would be physically meaningless — a concentration, a mass, a relaxation strength | [Core maths](core-math.md) |
| [`nor`](../fn/nor.html) | Returns a `bool`: logical NOR | [Collections & strings](collections-strings.md) |
| [`norm`](../fn/norm.html) | Euclidean (L2) norm; for a matrix this is the Frobenius norm (every element treated as one long vector), not a matrix operator norm | [Core maths](core-math.md) |
| [`normal`](../fn/normal.html) | Draws from a Gaussian. `mu` (number) is the mean; `sigma` (number) is the standard deviation; `rows` and `cols` (numbers, optional) give the shape of the draw, one number for a vector and.. | [Statistics & ML](statistics-ml.md) |
| [`normalize`](../fn/normalize.html) | Min-max scaling of `X` (a vector, an `N`-row/`D`-column matrix scaled column-wise, or a table scaled per numeric column) to `[0, 1]`: `(x - min) / (max - min)` | [Statistics & ML](statistics-ml.md) |
| [`normpdf`](../fn/normpdf.html) | Gaussian probability density at `x` (a number or vector, elementwise) with mean `mu` (default 0) and standard deviation `sigma` (default 1) | [Statistics & ML](statistics-ml.md) |
| [`now`](../fn/now.html) | Returns the current time (UTC) as a `Record` with integer fields `.year`, `.month`, `.day`, `.hour`, `.minute`, `.second`, plus `.unix` (number, Unix epoch seconds, for arithmetic) and.. | [Concurrency](concurrency.md) |
| [`nrow`](../fn/nrow.html) | Returns a `number`: the row count | [Collections & strings](collections-strings.md) |
| [`numel`](../fn/numel.html) | Total element count of `x`: `1` for a scalar, its length for a `Vec`/`CVec`/`Signal`, `rows*cols` for a `Mat`, a Unicode-aware character count for a `Str`, and the element/node count for.. | [Core maths](core-math.md) |
| [`nyquist`](../fn/nyquist.html) | The standard EIS plot: `Re(Z)` on x, `-Im(Z)` on y (the impedance-spectroscopy sign convention every EIS instrument uses, so capacitive/inductive behavior plots in the upper half-plane) | [Signal processing](signal-processing.md) |

## O

| | | |
|---|---|---|
| [`ols_model`](../fn/ols_model.html) | Fits ordinary least squares with an automatic intercept on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y` | [Statistics & ML](statistics-ml.md) |
| [`ones`](../fn/ones.html) | A vector or matrix of ones, with the same one-argument-vs-two-argument shape rules as `zeros` above | [Core maths](core-math.md) |
| [`ones_like`](../fn/ones_like.html) | A vector or matrix of the same shape as `x` (a `Vec` or `Mat`), filled with one or zero respectively — the shape is read directly from `x`, so it cannot fall out of step with it | [Core maths](core-math.md) |
| [`optimizer_step`](../fn/optimizer_step.html) | One optimizer update, for when you are writing the training loop yourself instead of calling `train_loop`: `opt` an optimizer Record (from `sgd`/`adam`/etc.), `grads` the gradient value.. | [Statistics & ML](statistics-ml.md) |
| [`or`](../fn/or.html) | Returns a `bool`. Both short-circuit: the right side is not evaluated when the left already decides the answer, so `i < len(xs) and xs[i] > 0` is a safe guard | [Collections & strings](collections-strings.md) |
| [`ord`](../fn/ord.html) | Returns a one-character `Str` from `chr`, and a `number` — the code point — from `ord` | [Collections & strings](collections-strings.md) |
| [`otsu`](../fn/otsu.html) | Compute-and-apply convenience: `otsu_threshold` followed immediately by `threshold` in one call | [Images](images.md) |
| [`otsu_threshold`](../fn/otsu_threshold.html) | Computes (does not apply) the classic Otsu optimal threshold from `x`'s histogram (`Image` reads per-pixel luma; `Vec`/`Signal` read their own values) | [Images](images.md) |
| [`overshoot`](../fn/overshoot.html) | Returns a single number, a percent of the step's own size `abs(final - initial)`: `overshoot` is how far the response travels *past its settled value* in the direction the step was going,.. | [Signal processing](signal-processing.md) |

## P

| | | |
|---|---|---|
| [`pad_left`](../fn/pad_left.html) | Pads on the left to `width` characters; a string already that wide is returned unchanged, never truncated | [Collections & strings](collections-strings.md) |
| [`pad_right`](../fn/pad_right.html) | Same parameters and return as `pad_left`, padding on the right instead | [Collections & strings](collections-strings.md) |
| [`palette`](../fn/palette.html) | Generates `n` (integer) categorical colours of equal perceptual weight by walking hue at fixed saturation/value | [Plotting](plotting.md) |
| [`panel`](../fn/panel.html) | Selects cell `k` (integer, 1-based, row-major) of a `rows`-by-`cols` (integers) grid as the target for further drawing | [Plotting](plotting.md) |
| [`parallel`](../fn/parallel.html) | Two or more circuits in parallel; admittances add | [Signal processing](signal-processing.md) |
| [`param`](../fn/param.html) | Wraps a plain number, vector, or matrix `v` as a fresh, differentiable Tensor — a new leaf node on the autodiff tape with no inputs of its own, so `grad` treats it as something to.. | [Statistics & ML](statistics-ml.md) |
| [`parse_as`](../fn/parse_as.html) | scanf backwards: `"hello world 12".parse_as("hello world %d")` gives 12 | [Collections & strings](collections-strings.md) |
| [`parse_csv`](../fn/parse_csv.html) | Converts `str` (string, CSV text) to a `Table` — exactly `read_csv`'s own parsing (`Table::from_csv`, the `headers=true, sep=",", decimal="."` defaults; use `read_csv` on a real file for.. | [File I/O](file-io.md) |
| [`parse_json`](../fn/parse_json.html) | `jsonify`'s inverse. `str` (string) is JSON text — either a `jsonify`/`save`-style `{"type": ...}` envelope, or ordinary JSON from anywhere else (e.g. an HTTP API response), which is mapped.. | [File I/O](file-io.md) |
| [`parse_xml`](../fn/parse_xml.html) | `xmlify`'s inverse. `str` (string) is XML text in `xmlify`'s own element-naming convention | [File I/O](file-io.md) |
| [`particle_filter`](../fn/particle_filter.html) | The specialized constant-velocity tracker: `n_particles` (integer), `initial_state` (a length-`D` position/velocity vector), `process_noise`/`obs_noise` (numbers, the built-in noise levels.. | [Statistics & ML](statistics-ml.md) |
| [`particle_filter_init`](../fn/particle_filter_init.html) | Generic particle filter state: `x0` a length-`D` centre vector, `n` (integer) the particle count scattered around it with spread `spread` (a number, default 1.0), and an optional integer.. | [Statistics & ML](statistics-ml.md) |
| [`pause`](../fn/pause.html) | Freezes the count, keeping the accumulated total so `start` resumes from where it left off | [REPL & diagnostics](repl-diagnostics.md) |
| [`pca`](../fn/pca.html) | Projects an `N`-row, `D`-column matrix `X` onto its top `k` principal components (columns mean-centered first) | [Statistics & ML](statistics-ml.md) |
| [`pca_components`](../fn/pca_components.html) | Fits the same PCA as `pca` on `X` and `k`, returning the `(k, D)` loading matrix instead — each row is a unit-length direction in the original `D`-dimensional feature space | [Statistics & ML](statistics-ml.md) |
| [`pca_explained_variance`](../fn/pca_explained_variance.html) | Fits the same PCA as `pca` on `X` and `k`, returning a length-`k` vector giving the fraction of the total variance in `X` that each kept component carries, in decreasing order | [Statistics & ML](statistics-ml.md) |
| [`pca_model`](../fn/pca_model.html) | PCA as a reusable handle: fits the top `k` principal components on an `N`-row, `D`-column matrix `X`, so `.predict(Xnew)` projects new rows onto the same fitted components instead of.. | [Statistics & ML](statistics-ml.md) |
| [`peak`](../fn/peak.html) | `max(abs(x))` over every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, flattened | [Core maths](core-math.md) |
| [`peek`](../fn/peek.html) | On a `fifo` handle `q`: `push(q, x)` adds `x` at the back (returns `Nothing`); `pop(q)` removes and returns the front element; `peek(q)` looks at it without removing | [Collections & strings](collections-strings.md) |
| [`peek_byte`](../fn/peek_byte.html) | Looks at the next raw byte of `f` (file handle, opened readable) without consuming it | [File I/O](file-io.md) |
| [`peek_char`](../fn/peek_char.html) | Looks at the next character of `f` (file handle, opened readable) without consuming it | [File I/O](file-io.md) |
| [`peek_line`](../fn/peek_line.html) | Looks at the next line of `f` (file handle, opened readable) without consuming it | [File I/O](file-io.md) |
| [`percentile`](../fn/percentile.html) | Same computation as `quantile`, with `p` on a `0..100` scale instead of `0..1` (`p=50` is the median) | [Core maths](core-math.md) |
| [`periodic_profile`](../fn/periodic_profile.html) | Returns a length-`n_bins` real vector, one summary statistic per bin | [Signal processing](signal-processing.md) |
| [`periodogram`](../fn/periodogram.html) | Returns a `spectrum` of `N/2 + 1` one-sided bins stamped `norm = "density"`, same scaling as `welch` (changed in v0.2.4 from a bare vector, back-compatibly — see `welch`) | [Signal processing](signal-processing.md) |
| [`permutation_importance`](../fn/permutation_importance.html) | Shuffles one column of an `N`-row, `D`-column `X` at a time and re-scores an already-fitted model Record `model` against the matching length-`N` target `y`, over `n_repeats` shuffles per.. | [Statistics & ML](statistics-ml.md) |
| [`phase`](../fn/phase.html) | Complex argument — the angle from the positive real axis, in radians (`atan2(im, re)`) — on a `Complex`/`CVec`/`CMat`; on a signed real scalar or real `Vec`/`Mat` it degrades to `0` for a.. | [Core maths](core-math.md) |
| [`pid_init`](../fn/pid_init.html) | A discrete PID controller's initial state | [Signal processing](signal-processing.md) |
| [`pie`](../fn/pie.html) | Ordinary pie chart — same call shape as `donut` (`values` a Vec of numbers, trailing strings optional slice labels) with `donut=false` | [Plotting](plotting.md) |
| [`pinv`](../fn/pinv.html) | Moore-Penrose pseudo-inverse (SVD-based) of `A`, an `r×c` `Mat` or `CMat` | [Core maths](core-math.md) |
| [`pipeline`](../fn/pipeline.html) | Composes one or more stage names (strings naming functions already defined in the program) into a single unfitted pipeline spec — every stage but the last must be a function `(X) -> X`; the.. | [Statistics & ML](statistics-ml.md) |
| [`plot`](../fn/plot.html) | Draws a basic line (or, with a positional or `marker=` marker string such as `"o"`, a scatter-style) plot of `y` versus `x` | [Plotting](plotting.md) |
| [`pmap`](../fn/pmap.html) | Returns a `List`: `f` applied to every element | [Collections & strings](collections-strings.md) |
| [`point`](../fn/point.html) | Marks a single position `(x, y)` (numbers, data units) with a dot | [Plotting](plotting.md) |
| [`poisson`](../fn/poisson.html) | Exact Poisson draws. `lambda` (a positive number) is the rate, which is also the mean and the variance of the result; `rows` and `cols` (numbers, optional) give the shape of the draw, one.. | [Statistics & ML](statistics-ml.md) |
| [`polarplot`](../fn/polarplot.html) | Converts polar coordinates `theta` (Vec of numbers, radians) and `r` (Vec of numbers, radius) to Cartesian (`x=r·cos θ`, `y=r·sin θ`) and draws an ordinary line/scatter series | [Plotting](plotting.md) |
| [`poles`](../fn/poles.html) | Returns a complex vector (`CVec`), one entry per pole across all sections/taps | [Signal processing](signal-processing.md) |
| [`polyfit`](../fn/polyfit.html) | Fits a least-squares polynomial of the given integer `degree` to a length-`N` vector `x` and a length-`N` vector `y` | [Statistics & ML](statistics-ml.md) |
| [`polygon`](../fn/polygon.html) | Draws a closed shape connecting the points `(xs[i], ys[i])` in order and back to the first | [Plotting](plotting.md) |
| [`polyval`](../fn/polyval.html) | Evaluates a coefficient vector `coeffs` (as returned by `polyfit`, highest-degree first) at `x`, either a single number or a length-`N` vector, by Horner's method | [Statistics & ML](statistics-ml.md) |
| [`pool`](../fn/pool.html) | Returns a `Pool` handle, unregistered and scoped to wherever it's used | [Concurrency](concurrency.md) |
| [`pop`](../fn/pop.html) | On a `fifo` handle `q`: `push(q, x)` adds `x` at the back (returns `Nothing`); `pop(q)` removes and returns the front element; `peek(q)` looks at it without removing | [Collections & strings](collections-strings.md) |
| [`pop_back`](../fn/pop_back.html) | Removes and returns the element from the front (`pop_front`) or back (`pop_back`) in O(1) | [Collections & strings](collections-strings.md) |
| [`pop_front`](../fn/pop_front.html) | Removes and returns the element from the front (`pop_front`) or back (`pop_back`) in O(1) | [Collections & strings](collections-strings.md) |
| [`porous`](../fn/porous.html) | `sqrt(Rp Zi)/tanh(sqrt(Rp/Zi))`, `Zi = 1/(Q (jw)^n)` — de Levie porous electrode | [Signal processing](signal-processing.md) |
| [`pow`](../fn/pow.html) | The operators as named functions, so an operation can be passed where a name is what you can pass | [Core maths](core-math.md) |
| [`pow2db`](../fn/pow2db.html) | Returns the same shape: `10*log10(x)` | [Signal processing](signal-processing.md) |
| [`precision`](../fn/precision.html) | Classification metrics on a length-`N` vector of true labels `actual` and a length-`N` vector of predicted labels `predicted`, each returning a single number macro-averaged over every class.. | [Statistics & ML](statistics-ml.md) |
| [`predict`](../fn/predict.html) | Applies a fitted model Record `model` to a new `M`-row (same `D` columns as it was fitted on) matrix or vector `Xnew` | [Statistics & ML](statistics-ml.md) |
| [`print`](../fn/print.html) | Writes its arguments, space-separated, and a newline, to stdout | [Collections & strings](collections-strings.md) |
| [`printtex`](../fn/printtex.html) | Same conversion as `tex(x)` on value `x` (number, Vec, Mat, or complex), but prints it directly instead of returning a string | [Plotting](plotting.md) |
| [`process`](../fn/process.html) | Returns the processed signal, applying `p`'s function block by block and carrying state across the block boundaries | [Signal processing](signal-processing.md) |
| [`processor`](../fn/processor.html) | Returns a `Model` (kind `"processor"`) with fields `fn`, `block`, `latency` (samples), `stateful`, plus `state` when stateful and `rate`/`latency_ms` when a rate was given | [Signal processing](signal-processing.md) |
| [`prod`](../fn/prod.html) | Product of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, real only (unlike `sum`, does not accept a `CVec`/`CMat`) | [Core maths](core-math.md) |
| [`profile_end`](../fn/profile_end.html) | Closes the window and returns a `Record` with fields `time` (elapsed seconds) and `mem` (RSS delta in bytes, `NaN` if either sample was `NaN`, e.g. on an unsupported platform) | [REPL & diagnostics](repl-diagnostics.md) |
| [`profile_start`](../fn/profile_start.html) | Starts a named profiling window and returns a `Record` handle -- `t0` (wall-clock seconds) and `mem0` (RSS bytes, `NaN` if unsupported) -- to pass unmodified to `profile_end` | [REPL & diagnostics](repl-diagnostics.md) |
| [`profile_stats`](../fn/profile_stats.html) | `profiling_mode` takes one argument, a boolean (`true` to start recording, `false` to stop), and returns `none` | [REPL & diagnostics](repl-diagnostics.md) |
| [`profiling_mode`](../fn/profiling_mode.html) | `profiling_mode` takes one argument, a boolean (`true` to start recording, `false` to stop), and returns `none` | [REPL & diagnostics](repl-diagnostics.md) |
| [`progress`](../fn/progress.html) | A tqdm-style progress bar, called once per loop iteration: `i` is the 0-based index (`0` through `n - 1`), `n` is the total count | [REPL & diagnostics](repl-diagnostics.md) |
| [`proper`](../fn/proper.html) | Uppercases every word's first character, lowercases the rest — VB's `StrConv(s, vbProperCase)` | [Collections & strings](collections-strings.md) |
| [`psd`](../fn/psd.html) | Returns a `spectrum` of `nfft/2 + 1` one-sided bins stamped `norm = "density"`, the Welch PSD in SciPy's `"density"` scaling, exactly as `welch` does — literally the same code path, so.. | [Signal processing](signal-processing.md) |
| [`pt`](../fn/pt.html) | Length units as functions, converting a scalar `v` (millimetres/centimetres/inches/points as named) to the figure's own coordinate space — a tenth of a millimetre, so `mm(1)` is `10`,.. | [Core maths](core-math.md) |
| [`pulse_frequency`](../fn/pulse_frequency.html) | Returns a single number: the mean interval between successive rising crossings (`pulse_period`) or its reciprocal (`pulse_frequency`) — seconds and Hz for a `Signal`, samples and.. | [Signal processing](signal-processing.md) |
| [`pulse_period`](../fn/pulse_period.html) | Returns a single number: the mean interval between successive rising crossings (`pulse_period`) or its reciprocal (`pulse_frequency`) — seconds and Hz for a `Signal`, samples and.. | [Signal processing](signal-processing.md) |
| [`pulse_width`](../fn/pulse_width.html) | Returns a single number: the mean width of the complete pulses, in seconds for a `Signal` and samples otherwise | [Signal processing](signal-processing.md) |
| [`pump_watches`](../fn/pump_watches.html) | Checks every registered `watch ... end` (§ Watches, below) exactly ONCE, right now, against its real current external state, firing the body of any whose state has genuinely changed since.. | [Concurrency](concurrency.md) |
| [`push`](../fn/push.html) | On a `fifo` handle `q`: `push(q, x)` adds `x` at the back (returns `Nothing`); `pop(q)` removes and returns the front element; `peek(q)` looks at it without removing | [Collections & strings](collections-strings.md) |
| [`push_back`](../fn/push_back.html) | Grows `l` at the back (`push_back`) or front (`push_front`) in O(1) | [Collections & strings](collections-strings.md) |
| [`push_front`](../fn/push_front.html) | Grows `l` at the back (`push_back`) or front (`push_front`) in O(1) | [Collections & strings](collections-strings.md) |
| [`pwd`](../fn/pwd.html) | `cur_dir` says what it gives back and `pwd` is what anyone who has used a shell types first, so both spellings exist rather than one being renamed out from under existing scripts | [File I/O](file-io.md) |
| [`pwl`](../fn/pwl.html) | Continuous piecewise-linear least-squares fit through equal-length numeric Vecs `x`, `y` | [Plotting](plotting.md) |
| [`pwm`](../fn/pwm.html) | Natural-sampling pulse-width modulation: `modulator` is a Vec of numbers (any signal, conventionally in `[-1, 1]`), `carrier_freq` and `fs` are numbers (Hz) | [Plotting](plotting.md) |
| [`python_exec`](../fn/python_exec.html) | Runs `code` (string, source in the guest language) as a subprocess | [File I/O](file-io.md) |
| [`pzplot`](../fn/pzplot.html) | Pole-zero plot (MATLAB's `zplane` concept): an `x` marker at each pole, `o` at each zero, and the unit circle as a reference polyline, titled with the filter's stability verdict | [Signal processing](signal-processing.md) |

## Q

| | | |
|---|---|---|
| [`q_learning`](../fn/q_learning.html) | Tabular temporal-difference learning on a `gridworld_env` `env` for an integer number of `episodes`, with learning rate `alpha`, discount factor `gamma`, and epsilon-greedy exploration rate.. | [Statistics & ML](statistics-ml.md) |
| [`qam_demodulate`](../fn/qam_demodulate.html) | Nearest-constellation-point decision, the exact inverse of `qam_modulate` on clean symbols | [Signal processing](signal-processing.md) |
| [`qam_modulate`](../fn/qam_modulate.html) | Maps each group of `log2(order)` bits to one point of the standard Gray-coded square constellation — the first half of the group labels the in-phase level, the second half the quadrature one | [Signal processing](signal-processing.md) |
| [`qr`](../fn/qr.html) | Thin QR decomposition of `A` (`r×c` `Mat`), `A = Q·R` | [Core maths](core-math.md) |
| [`quantile`](../fn/quantile.html) | Linear-interpolation quantile (NumPy's default `interpolation="linear"` method) | [Core maths](core-math.md) |
| [`quantile_normalize`](../fn/quantile_normalize.html) | Rank-based scaling of `X` (vector, matrix column-wise, or table per numeric column): each value is replaced by its rank against its own column, rescaled uniform onto `[0, 1]` | [Statistics & ML](statistics-ml.md) |
| [`queue`](../fn/queue.html) | Creates an empty deferred-job list | [Concurrency](concurrency.md) |
| [`quick_mlp`](../fn/quick_mlp.html) | Fits a single-hidden-layer multilayer perceptron in one call on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y` (or an `(N, n_classes)` one-hot matrix for.. | [Statistics & ML](statistics-ml.md) |

## R

| | | |
|---|---|---|
| [`raincloud`](../fn/raincloud.html) | Allen et al. (2019) raincloud plot: one group per argument (each a Vec of numbers) at `x = 0, 1, 2, ...`, each rendered as a Gaussian KDE "cloud" + boxplot + jittered raw points ("rain") | [Plotting](plotting.md) |
| [`rand`](../fn/rand.html) | Draws from the uniform distribution on `[0, 1)` | [Statistics & ML](statistics-ml.md) |
| [`randi`](../fn/randi.html) | Draws random integers. `lo` (number) is the smallest value that can come out; `hi` (number) is the largest, inclusive at both ends (MATLAB's convention, not NumPy's half-open one, so.. | [Statistics & ML](statistics-ml.md) |
| [`randn`](../fn/randn.html) | Draws from the standard normal distribution (mean 0, std 1), same `rows`/`cols`/`seed=` shape as `rand` | [Statistics & ML](statistics-ml.md) |
| [`random_forest_model`](../fn/random_forest_model.html) | Bagged ensemble of `n_trees` (a positive integer) CART trees on an `N`-row, `D`-column `X` and length-`N` `y`, each tree grown on its own bootstrap resample of the rows and a.. | [Statistics & ML](statistics-ml.md) |
| [`random_walk`](../fn/random_walk.html) | Generates a discretized Wiener-process path: `n_steps` (a positive integer) increments, each drawn `~ Normal(drift, volatility)`, cumulatively summed, with an optional integer `seed=` | [Statistics & ML](statistics-ml.md) |
| [`range`](../fn/range.html) | Inclusive numeric range as a `Vec`, from `a` to `b` (both scalars) in increments of `step` (an optional positional scalar, default `1` — `step=` as a keyword is rejected, unlike most.. | [Core maths](core-math.md) |
| [`range_decode`](../fn/range_decode.html) | Inverse of `range_encode`: `enc` is the `Record` that call returned; `as_str` (named boolean, default `false`) asks for text instead of bytes | [File I/O](file-io.md) |
| [`range_encode`](../fn/range_encode.html) | Byte-oriented adaptive range coding of `data` (a `Str`, or a `Vec` of 0-255 byte values) | [File I/O](file-io.md) |
| [`rank`](../fn/rank.html) | Numerical rank of `A` (an `r×c` `Mat` or `CMat`) — the count of singular values clearing the tolerance | [Core maths](core-math.md) |
| [`rans_decode`](../fn/rans_decode.html) | Inverse of `rans_encode`: `enc` is the `Record` that call returned; `as_str` (named boolean, default `false`) asks for text instead of bytes | [File I/O](file-io.md) |
| [`rans_encode`](../fn/rans_encode.html) | rANS entropy coding of `data` (a `Str`, or a `Vec` of 0-255 byte values) — a static frequency table, unlike `range_encode`'s adaptive model, so the table travels with the payload | [File I/O](file-io.md) |
| [`re`](../fn/re.html) | Real component of a `Complex`/`CVec`/`CMat` (identity — `x` passed through unchanged — when `x` is already real) | [Core maths](core-math.md) |
| [`read`](../fn/read.html) | `swap` atomically exchanges front and back (returns `Nothing`); `read` returns the stable front value | [Collections & strings](collections-strings.md) |
| [`read_all`](../fn/read_all.html) | Reads every remaining byte from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_all_text`](../fn/read_all_text.html) | Reads `path` (string) in full. Returns the whole file's content as one string | [File I/O](file-io.md) |
| [`read_array`](../fn/read_array.html) | Reads a whole binary file as one typed array, without an explicit `fopen` | [File I/O](file-io.md) |
| [`read_bin`](../fn/read_bin.html) | Reads up to `n` (a number) raw bytes from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_bit`](../fn/read_bit.html) | Reads one bit from `f` (file handle, opened readable), MSB-first, advancing a sub-byte cursor that only moves to the next byte once all 8 bits of it are consumed | [File I/O](file-io.md) |
| [`read_byte`](../fn/read_byte.html) | Reads one raw byte from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_bytes`](../fn/read_bytes.html) | Blocks up to the handle's `timeout_ms`, returning a `Vec` of 0-255 integer values — possibly fewer than `n` if the timeout elapses first | [Concurrency](concurrency.md) |
| [`read_char`](../fn/read_char.html) | Reads one character from `f` (file handle, opened readable), UTF-8 aware | [File I/O](file-io.md) |
| [`read_chars`](../fn/read_chars.html) | Reads exactly `n` (number, count of Unicode characters, UTF-8 aware) characters from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_csv`](../fn/read_csv.html) | Reads a CSV file into a `Table` | [File I/O](file-io.md) |
| [`read_double`](../fn/read_double.html) | Reads a 64-bit IEEE-754 float from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_float`](../fn/read_float.html) | Reads a 32-bit IEEE-754 float from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_input`](../fn/read_input.html) | Reads one line of interactive input, returned as a string with its trailing newline stripped | [File I/O](file-io.md) |
| [`read_int`](../fn/read_int.html) | The width-as-a-value counterpart of the fixed-width family above: reads `bytes` (a number, `1`/`2`/`4`/`8` only) from `f` (file handle, opened readable) and decodes them as an integer, with.. | [File I/O](file-io.md) |
| [`read_int16`](../fn/read_int16.html) | Reads a signed 16-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_int32`](../fn/read_int32.html) | Reads a signed 32-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_int64`](../fn/read_int64.html) | Reads a signed 64-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_line`](../fn/read_line.html) | Reads one line from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_mat`](../fn/read_mat.html) | Reads a MATLAB `.mat` file (via the `matfile` crate) without needing a MATLAB installation | [File I/O](file-io.md) |
| [`read_struct`](../fn/read_struct.html) | Reads one fixed-width binary record from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_structs`](../fn/read_structs.html) | Reads up to `n` (number) fixed-width binary records from `f` (file handle, opened readable), using the same `fields` layout `read_struct` takes | [File I/O](file-io.md) |
| [`read_uint16`](../fn/read_uint16.html) | Reads an unsigned 16-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_uint32`](../fn/read_uint32.html) | Reads an unsigned 32-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_uint64`](../fn/read_uint64.html) | Reads an unsigned 64-bit integer from `f` (file handle, opened readable) | [File I/O](file-io.md) |
| [`read_until`](../fn/read_until.html) | Reads from `f` (file handle, opened readable) up to and including the next occurrence of `delimiter` (string), consuming it | [File I/O](file-io.md) |
| [`read_values`](../fn/read_values.html) | Reads `n` (number, count) typed values from an already-open handle | [File I/O](file-io.md) |
| [`real`](../fn/real.html) | Real component of a `Complex`/`CVec`/`CMat` (identity — `x` passed through unchanged — when `x` is already real) | [Core maths](core-math.md) |
| [`recall`](../fn/recall.html) | Classification metrics on a length-`N` vector of true labels `actual` and a length-`N` vector of predicted labels `predicted`, each returning a single number macro-averaged over every class.. | [Statistics & ML](statistics-ml.md) |
| [`rect`](../fn/rect.html) | Draws an axis-aligned rectangle | [Plotting](plotting.md) |
| [`rectangle`](../fn/rectangle.html) | Draws an axis-aligned rectangle | [Plotting](plotting.md) |
| [`reduce`](../fn/reduce.html) | Returns the final accumulated value (whatever type `f` produces) | [Collections & strings](collections-strings.md) |
| [`reflection_coefficient`](../fn/reflection_coefficient.html) | Returns `Γ = (z - z0)/(z + z0)`, complex | [Signal processing](signal-processing.md) |
| [`regex_count`](../fn/regex_count.html) | Returns a `number`: how many non-overlapping matches occur | [Collections & strings](collections-strings.md) |
| [`regex_find`](../fn/regex_find.html) | Returns a `Str` (the first match) or `none` — not an empty string, so "matched empty" and "did not match" stay apart | [Collections & strings](collections-strings.md) |
| [`regex_find_all`](../fn/regex_find_all.html) | Returns a `List` of `Str` — every match, in order | [Collections & strings](collections-strings.md) |
| [`regex_groups`](../fn/regex_groups.html) | Returns a `List` of `Str` — the capture groups of the first match, group 1 onward; a group that did not participate is an empty string | [Collections & strings](collections-strings.md) |
| [`regex_match`](../fn/regex_match.html) | Returns a `bool`: whether the pattern occurs anywhere in `s` | [Collections & strings](collections-strings.md) |
| [`regex_replace`](../fn/regex_replace.html) | `s`, `pattern` and `repl` are `Str`s; optional `count` (number, default: all occurrences) caps how many replacements are made. `$1` and `${name}` in `repl` refer to capture groups. Returns.. | [Collections & strings](collections-strings.md) |
| [`regex_split`](../fn/regex_split.html) | Returns a `List` of `Str`, split on every match of the pattern | [Collections & strings](collections-strings.md) |
| [`regionprops`](../fn/regionprops.html) | Takes that result and returns a `List` of one `Record` per blob, in label-id order, each with fields: `label` (integer id), `area` (pixel count), `centroid_x`, `centroid_y` (real.. | [Images](images.md) |
| [`regions`](../fn/regions.html) | Added in v0.2.4. The signal's regions as a list of records with `start`, `end` and `label`, in start order | [Signal processing](signal-processing.md) |
| [`relu`](../fn/relu.html) | Elementwise activation applied to a number, vector, or matrix `x` of any shape — `relu` clips negative values to zero, `sigmoid` squashes to `(0, 1)` | [Statistics & ML](statistics-ml.md) |
| [`remove`](../fn/remove.html) | Two overloads, picked by the type of the second argument: `text` (`Str`) deletes every occurrence of it; `start` (number, 0-based index) with optional `count` (number of characters,.. | [Collections & strings](collections-strings.md) |
| [`remove_dir`](../fn/remove_dir.html) | Deletes the directory `path`. Refuses a non-empty directory unless `recursive=true` — this check applies whether or not `recycle_bin` is set, since "may I remove a whole tree" and "should.. | [File I/O](file-io.md) |
| [`remove_file`](../fn/remove_file.html) | Deletes `path` (a `Str`). Errors if it does not exist or is not a regular file, rather than silently doing nothing | [File I/O](file-io.md) |
| [`remove_nan`](../fn/remove_nan.html) | `x` with the missing samples dropped | [Noise](noise.md) |
| [`remove_noise`](../fn/remove_noise.html) | `x` is a length-N number vector or `Signal`. `method="wiener"` (default): a local adaptive Wiener filter — for each sample, compares the local variance in a `window`-wide neighborhood.. | [Noise](noise.md) |
| [`remove_outliers`](../fn/remove_outliers.html) | The same criterion, with the flagged samples dropped | [Noise](noise.md) |
| [`remove_small_blobs`](../fn/remove_small_blobs.html) | Drop connected components below that area, after labelling internally (identical operation to `bwareaopen` above) | [Images](images.md) |
| [`rename_file`](../fn/rename_file.html) | Renames `old` to `new` (both `Str`), generally within the same filesystem | [File I/O](file-io.md) |
| [`repeat_str`](../fn/repeat_str.html) | `s * n` is the shorter spelling for the same thing; the function is not called `repeat` because that is a loop keyword | [Collections & strings](collections-strings.md) |
| [`replace`](../fn/replace.html) | Every literal (non-regex) occurrence of `old` in `s` is replaced by `new` | [Collections & strings](collections-strings.md) |
| [`replace_outliers`](../fn/replace_outliers.html) | The same criterion, with the flagged samples patched in place | [Noise](noise.md) |
| [`resample_int`](../fn/resample_int.html) | Rational rate change by `L/M` in a single pass: zero-stuff by `L`, filter once, decimate by `M` | [Signal processing](signal-processing.md) |
| [`resample_to`](../fn/resample_to.html) | Returns a new `Signal` at `Fs=new_fs`, whose length is whatever it takes to span the *same* start/end time as the input (generally not N) | [Signal processing](signal-processing.md) |
| [`reset`](../fn/reset.html) | Returns the environment `env`'s starting state — a plain integer state index | [Statistics & ML](statistics-ml.md) |
| [`reshape`](../fn/reshape.html) | Reinterprets a value as a matrix of a given shape | [Core maths](core-math.md) |
| [`resistor`](../fn/resistor.html) | `R` | [Signal processing](signal-processing.md) |
| [`resize`](../fn/resize.html) | Returns a new `Image` of exactly `width` x `height` pixels | [Images](images.md) |
| [`restart`](../fn/restart.html) | Returns a number: the elapsed time up to this call (the completed "lap"), then resets the object to zero and immediately starts it running again — a one-call lap/checkpoint pattern for.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`return_loss`](../fn/return_loss.html) | Returns `-20 log10 \|Γ\|` in dB, positive for a passive load — the engineering convention in which "20 dB return loss" means well matched | [Signal processing](signal-processing.md) |
| [`reverse`](../fn/reverse.html) | Unicode-aware character reversal for a `Str` (same type back); element-order reversal for a `List`/`Vec`/`Mask` (same type and shape back) | [Collections & strings](collections-strings.md) |
| [`rewind`](../fn/rewind.html) | Resets `f`'s (file handle, opened readable) read cursor to the beginning — exactly `seek(f, 0)` | [File I/O](file-io.md) |
| [`rfe`](../fn/rfe.html) | Recursive feature elimination on an `N`-row, `D`-column `X` and length-`N` `y`, down to an integer target `n_features`: repeatedly drops the weakest-scoring remaining column and refits | [Statistics & ML](statistics-ml.md) |
| [`rfft`](../fn/rfft.html) | Real-optimized half-spectrum FFT | [Signal processing](signal-processing.md) |
| [`rgb`](../fn/rgb.html) | Builds a colour from channel numbers: `r`, `g`, `b` (and, for `rgba`, a fourth channel `a`) — a fractional 0–1 fourth channel is an opacity, a whole number 0–255 is a byte value like the.. | [Plotting](plotting.md) |
| [`rgba`](../fn/rgba.html) | Builds a colour from channel numbers: `r`, `g`, `b` (and, for `rgba`, a fourth channel `a`) — a fractional 0–1 fourth channel is an opacity, a whole number 0–255 is a byte value like the.. | [Plotting](plotting.md) |
| [`ridge`](../fn/ridge.html) | Tikhonov-regularized (L2) least squares on an `N`-row, `D`-column feature matrix `X` and a length-`N` target vector `y`, with regularization strength `alpha` (a non-negative number; plain.. | [Statistics & ML](statistics-ml.md) |
| [`ridge_model`](../fn/ridge_model.html) | The same fit as `ols_model` on an `N`-row, `D`-column `X` and a length-`N` `y`, but with L2 regularization strength `alpha` (a non-negative number) shrinking the coefficients toward zero;.. | [Statistics & ML](statistics-ml.md) |
| [`right`](../fn/right.html) | The last `n` characters of a string | [Collections & strings](collections-strings.md) |
| [`rise_time`](../fn/rise_time.html) | Returns a single number: the transition time of the first complete rising (resp | [Signal processing](signal-processing.md) |
| [`rising_edges`](../fn/rising_edges.html) | Returns a vector of the interpolated crossing positions of one polarity only — seconds for a `Signal`, samples otherwise (the same convention `find_pulses`'s `widths` field uses) | [Signal processing](signal-processing.md) |
| [`rlkk_extrapolate`](../fn/rlkk_extrapolate.html) | Returns a single length-M `CVec`, not a model | [Signal processing](signal-processing.md) |
| [`rlkk_reconstruct`](../fn/rlkk_reconstruct.html) | Returns a `Model` (kind `"rlkk"`) with fields `z` (a length-N `CVec`, the reconstructed spectrum) and `gamma` (a length-M real vector, the fitted DRT coefficients) | [Signal processing](signal-processing.md) |
| [`rlkk_validate`](../fn/rlkk_validate.html) | Returns a `Model` (kind `"rlkk_validation"`) with fields `valid` (a boolean, true iff every point's percent residual is under `threshold`), `residuals` (a length-N real vector, percent),.. | [Signal processing](signal-processing.md) |
| [`rms`](../fn/rms.html) | Root-mean-square, `sqrt(mean(x.^2))`, of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal`, flattened | [Core maths](core-math.md) |
| [`rmse`](../fn/rmse.html) | The regression-error trio, each taking a length-`N` vector of true values `actual` and a length-`N` vector of predictions `predicted`, and returning a single number: `mae` the mean absolute.. | [Statistics & ML](statistics-ml.md) |
| [`rmsprop`](../fn/rmsprop.html) | Per-parameter learning rates derived from the running gradient history, given `params` (a Tensor to optimize) and `lr` (a number, default 0.001) | [Statistics & ML](statistics-ml.md) |
| [`robust_scale`](../fn/robust_scale.html) | Outlier-resistant scaling of `X` (vector, matrix column-wise, or table per numeric column): `(x - median) / IQR` | [Statistics & ML](statistics-ml.md) |
| [`rolling_max`](../fn/rolling_max.html) | Maximum over a centred window. Same shape/type rules as `rolling_mean` | [Noise](noise.md) |
| [`rolling_mean`](../fn/rolling_mean.html) | Mean over a window centred on each sample | [Noise](noise.md) |
| [`rolling_min`](../fn/rolling_min.html) | Minimum over a centred window. Same shape/type rules as `rolling_mean` | [Noise](noise.md) |
| [`rolling_rms`](../fn/rolling_rms.html) | Root-mean-square over a centred window — a moving level, e.g. for an RMS trigger threshold | [Noise](noise.md) |
| [`rolling_std`](../fn/rolling_std.html) | Sample (N-1, unbiased) standard deviation over a centred window, matching `std`/`var` exactly, so `rolling_std(x, w)[i]` equals `std` of that window — a property you can check | [Noise](noise.md) |
| [`rot90`](../fn/rot90.html) | Rotates `A` (a `Mat`, or anything coercible to one) 90°×`k` counterclockwise; `k` is an optional scalar defaulting to `1`, may be negative for clockwise rotation, and is taken mod 4 | [Core maths](core-math.md) |
| [`round`](../fn/round.html) | Standard rounding functions, elementwise: `floor` toward negative infinity, `ceil` toward positive infinity, `round` to the nearest integer (half away from zero) | [Core maths](core-math.md) |
| [`row_mean`](../fn/row_mean.html) | Collapses each row of `M` (an `r×c` `Mat`, or anything coercible to one) to one number, returning an `r×1` column `Mat` (not a plain `Vec`) | [Core maths](core-math.md) |
| [`row_sum`](../fn/row_sum.html) | Collapses each row of `M` (an `r×c` `Mat`, or anything coercible to one) to one number, returning an `r×1` column `Mat` (not a plain `Vec`) | [Core maths](core-math.md) |
| [`rows`](../fn/rows.html) | Row / column count of `A`. On a `Vec` (shape `(n, 1)`), a `Mat`/`Signal` (shape `(r, c)`), or a bare scalar (shape `(1, 1)`), returns the corresponding dimension as a scalar `Num` | [Core maths](core-math.md) |
| [`rtrim`](../fn/rtrim.html) | Same parameters as `trim`, restricted to one end only (`ltrim` the left, `rtrim` the right) | [Collections & strings](collections-strings.md) |
| [`run_for`](../fn/run_for.html) | Drives every registered `every`/`after`/`at` timer across a simulated virtual timeline, not a real wall-clock sleep | [Concurrency](concurrency.md) |

## S

| | | |
|---|---|---|
| [`sandbox_mode`](../fn/sandbox_mode.html) | Refuses file and network access for the rest of the session and returns `none` | [REPL & diagnostics](repl-diagnostics.md) |
| [`sarsa`](../fn/sarsa.html) | Tabular temporal-difference learning on a `gridworld_env` `env` for an integer number of `episodes`, with learning rate `alpha`, discount factor `gamma`, and epsilon-greedy exploration rate.. | [Statistics & ML](statistics-ml.md) |
| [`sauvola_threshold`](../fn/sauvola_threshold.html) | Sauvola's local threshold, `T = mean * (1 + k * (stddev/r - 1))` computed per pixel over `window_size`, purpose-built for document/text images whose illumination drifts across the frame — a.. | [Images](images.md) |
| [`save`](../fn/save.html) | Writes the named variables — `path` (string, destination file) plus one or more variable names (strings) given explicitly as extra arguments — to a JSON file, each tagged with its own type | [File I/O](file-io.md) |
| [`save_all`](../fn/save_all.html) | Saves literally every current binding — every user variable plus Qu's own built-in constants (`pi`, `e`, `tau`, `none`, the `Qu*` escape-character constants, ...) — to `path` (string) | [File I/O](file-io.md) |
| [`save_image`](../fn/save_image.html) | Encodes `img` and writes it to `path`, dispatching on the path's own extension: `.png` writes a real (if uncompressed) PNG; `.jpg`/`.jpeg` writes a baseline JPEG; `.tif`/`.tiff` writes an.. | [Images](images.md) |
| [`save_model`](../fn/save_model.html) | Writes a fitted `kind="sequential"` model Record `m` (from `sequential`/`compile`/`quick_mlp`/`mlp_classifier`/etc.) to an HDF5 file at the string `path`, weights and all, in a.. | [Statistics & ML](statistics-ml.md) |
| [`save_svg`](../fn/save_svg.html) | Writes a standalone SVG document with an XML declaration, the SVG namespace, and both `width`/`height` and a matching `viewBox` — a `viewBox` alone renders at the container's size rather.. | [Images](images.md) |
| [`savefig`](../fn/savefig.html) | Exports the current figure to `path` (string, a filesystem path); format is chosen by the file extension | [Plotting](plotting.md) |
| [`savgol`](../fn/savgol.html) | Savitzky-Golay: fits a degree-`order` polynomial over each `window`-wide neighbourhood and keeps its centre value, preserving peak height/width that a moving average would flatten | [Noise](noise.md) |
| [`sawtooth`](../fn/sawtooth.html) | Each generates one period-repeating waveform: `freq` (number, Hz), `fs` (number, sample rate in Hz), `n` (integer, sample count) are all scalars, in that order | [Plotting](plotting.md) |
| [`scaled_dot_product_attention`](../fn/scaled_dot_product_attention.html) | Core attention computation: an `(Nq, D)` query matrix `q`, an `(Nk, D)` key matrix `k`, and an `(Nk, Dv)` value matrix `v` (`k` and `v` share the same row count `Nk`) | [Statistics & ML](statistics-ml.md) |
| [`scan`](../fn/scan.html) | scanf backwards: `"hello world 12".parse_as("hello world %d")` gives 12 | [Collections & strings](collections-strings.md) |
| [`scatter`](../fn/scatter.html) | Same machinery as `plot` — `x`, `y` are equal-length Vecs of numbers — but defaults its marker to `"o"` (dots) instead of `"line"` | [Plotting](plotting.md) |
| [`scatterfit`](../fn/scatterfit.html) | Draws a scatter of equal-length numeric Vecs `x`, `y` plus a `polyfit` trend line | [Plotting](plotting.md) |
| [`score`](../fn/score.html) | Scores a fitted model Record `model` against an `N`-row feature matrix `X` and, for a supervised kind, the matching length-`N` target vector `y` | [Statistics & ML](statistics-ml.md) |
| [`sech`](../fn/sech.html) | The other three hyperbolics: `coth(x) = cosh(x)/sinh(x)`, `sech(x) = 1/cosh(x)`, `csch(x) = 1/sinh(x)` | [Core maths](core-math.md) |
| [`seed`](../fn/seed.html) | Reseeds the shared random stream with integer `n`, so subsequent calls to `rand`/`randn`/etc | [Statistics & ML](statistics-ml.md) |
| [`seek`](../fn/seek.html) | Moves `f`'s (file handle, opened readable) read cursor | [File I/O](file-io.md) |
| [`select`](../fn/select.html) | Returns a `Table` containing only the named columns, in the order given | [Collections & strings](collections-strings.md) |
| [`semaphore`](../fn/semaphore.html) | Creates a counting semaphore with `n` (integer, must be non-negative) initial permits | [Concurrency](concurrency.md) |
| [`semaphore_acquire`](../fn/semaphore_acquire.html) | Blocks the calling thread until a permit is free on semaphore handle `s`, then takes it (decrementing the available count by one) | [Concurrency](concurrency.md) |
| [`semaphore_available`](../fn/semaphore_available.html) | Non-blocking snapshot of how many permits are free right now on semaphore handle `s` | [Concurrency](concurrency.md) |
| [`semaphore_release`](../fn/semaphore_release.html) | Returns one permit to semaphore handle `s` (incrementing the available count by one) | [Concurrency](concurrency.md) |
| [`semilogx`](../fn/semilogx.html) | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets the x-axis to log scale | [Plotting](plotting.md) |
| [`semilogy`](../fn/semilogy.html) | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets the y-axis to log scale | [Plotting](plotting.md) |
| [`sequential`](../fn/sequential.html) | Initializes a stack of layer specs (a list of `dense_layer`/`dropout_layer` Records, or a single one) into a trainable model: weights use He initialization (`sqrt(2/in_dim)`) on a layer.. | [Statistics & ML](statistics-ml.md) |
| [`sequential_split`](../fn/sequential_split.html) | The same shape as `train_test_split` on `X`/`y`/`test_size`, but never shuffles: the first rows become the training set, the last rows the test set, in their original order | [Statistics & ML](statistics-ml.md) |
| [`serial_open`](../fn/serial_open.html) | Opens a real OS serial port. `port` (string) is the OS device name, e.g. `"COM3"` on Windows or `"/dev/ttyUSB0"` on Linux/macOS | [Concurrency](concurrency.md) |
| [`serial_ports`](../fn/serial_ports.html) | Lists the short names of every serial device the OS currently reports (e.g. `"COM3"` on Windows, `"/dev/ttyUSB0"` on Linux/macOS) | [Concurrency](concurrency.md) |
| [`series`](../fn/series.html) | Two or more circuits in series; impedances add | [Signal processing](signal-processing.md) |
| [`set`](../fn/set.html) | `get` returns the stored value (any type) or `none` if `key` is absent | [Collections & strings](collections-strings.md) |
| [`set_metadata`](../fn/set_metadata.html) | Added in v0.2.4. Writes one field of the named schema: `channel`, `comment`, `experiment`, `gain`, `instrument`, `offset`, `operator`, `sample`, `sampling_rate`, `sensor`, `start_time`,.. | [Signal processing](signal-processing.md) |
| [`set_start_time`](../fn/set_start_time.html) | Added in v0.2.4. Dates the signal: sets the time of sample 0 in seconds | [Signal processing](signal-processing.md) |
| [`sfdr`](../fn/sfdr.html) | With no excitation named, the largest non-DC FFT bin is taken as the fundamental (the single-tone case); `tones=`/`freqs=` (a vector, Hz, needs `fs=`/`rate=` too) or `bins=` (a vector of.. | [Signal processing](signal-processing.md) |
| [`sgd`](../fn/sgd.html) | Plain stochastic gradient descent (`sgd`) and its look-ahead variant (`nesterov_sgd`, which evaluates the gradient after applying the momentum step rather than before it), given `params` (a.. | [Statistics & ML](statistics-ml.md) |
| [`shape`](../fn/shape.html) | The full shape of `A` as a 2-element `Vec`, `[rows, cols]`, using the same per-type shape rules as `rows`/`cols` above | [Core maths](core-math.md) |
| [`sharpen`](../fn/sharpen.html) | Applies a fixed 3x3 unsharp-mask sharpen kernel | [Images](images.md) |
| [`shell`](../fn/shell.html) | Hands `command` (a `Str`) to the OS shell and returns immediately, without waiting — Visual Basic's `Shell`, which is where the window styles come from too | [File I/O](file-io.md) |
| [`shortest_path`](../fn/shortest_path.html) | Returns a `number` (the shortest total edge weight, via Dijkstra) or `none` if `to` is unreachable from `from` — disconnected is an answer, not an error | [Collections & strings](collections-strings.md) |
| [`sigma_delta`](../fn/sigma_delta.html) | Returns a same-length vector or `Signal` (preserving `x`'s `Fs` when it's a `Signal`), each sample `+1`/`-1` | [Signal processing](signal-processing.md) |
| [`sigmoid`](../fn/sigmoid.html) | Elementwise activation applied to a number, vector, or matrix `x` of any shape — `relu` clips negative values to zero, `sigmoid` squashes to `(0, 1)` | [Statistics & ML](statistics-ml.md) |
| [`sign`](../fn/sign.html) | The sign of each element, using Rust's `f64::signum` semantics: `-1` for a negative value (including `-0.0`), `+1` for a non-negative value (including `+0.0` — so `sign(0)` returns `1`,.. | [Core maths](core-math.md) |
| [`signal`](../fn/signal.html) | Tags Vec `x` (numbers) with sample rate `fs` (number, Hz), so later calls (like `rfft`) do not have to be told it again | [Plotting](plotting.md) |
| [`signal_slice_time`](../fn/signal_slice_time.html) | Returns a new `Signal` at the same `Fs` | [Signal processing](signal-processing.md) |
| [`signal_unit`](../fn/signal_unit.html) | Added in v0.2.4. With one argument, reads the sample unit (`Nothing` if unset) | [Signal processing](signal-processing.md) |
| [`simple_cnn`](../fn/simple_cnn.html) | Builds an untrained conv → relu → maxpool → dense image classifier: `[height, width]` a 2-element vector giving the image size, `n_classes` a positive integer, and an optional integer.. | [Statistics & ML](statistics-ml.md) |
| [`simple_rnn_classifier`](../fn/simple_rnn_classifier.html) | Builds an untrained GRU-backed sequence classifier: `input_size`/`hidden_size`/`n_classes` positive integers (features per timestep, hidden width, and number of classes), plus an optional.. | [Statistics & ML](statistics-ml.md) |
| [`simulate`](../fn/simulate.html) | Draws one random trajectory from a `markov_chain` `chain`, starting at its `initial_state`, for an integer `n_steps` transitions (with an optional integer `seed=`) | [Statistics & ML](statistics-ml.md) |
| [`sin`](../fn/sin.html) | Standard trigonometric functions, computed elementwise | [Core maths](core-math.md) |
| [`sinad`](../fn/sinad.html) | With no excitation named, the largest non-DC FFT bin is taken as the fundamental (the single-tone case); `tones=`/`freqs=` (a vector, Hz, needs `fs=`/`rate=` too) or `bins=` (a vector of.. | [Signal processing](signal-processing.md) |
| [`sinad_estimate`](../fn/sinad_estimate.html) | Returns a single number: `sinad_estimate` in dB, `enob_estimate` in bits | [Signal processing](signal-processing.md) |
| [`sine`](../fn/sine.html) | Returns a length-`n` `Signal` (carries `Fs=fs`) | [Signal processing](signal-processing.md) |
| [`sinh`](../fn/sinh.html) | Hyperbolic sine/cosine/tangent, elementwise | [Core maths](core-math.md) |
| [`size`](../fn/size.html) | The full shape of `A` as a 2-element `Vec`, `[rows, cols]`, using the same per-type shape rules as `rows`/`cols` above | [Core maths](core-math.md) |
| [`sizeof`](../fn/sizeof.html) | Deprecated — use `shape`. The same function under a name that means a byte count in every language it could have come from | [Collections & strings](collections-strings.md) |
| [`skewness`](../fn/skewness.html) | Third standardized moment, population form, of a length-`N` numeric vector `x` | [Statistics & ML](statistics-ml.md) |
| [`sleep`](../fn/sleep.html) | Pauses the calling thread for that long | [Concurrency](concurrency.md) |
| [`smc_init`](../fn/smc_init.html) | A first-order sliding-mode controller's initial state | [Signal processing](signal-processing.md) |
| [`smith`](../fn/smith.html) | Impedance on the reflection-coefficient plane: `z = Z/z0`, `Γ = (z−1)/(z+1)`, drawn on the unit disc ruled by constant-resistance circles and constant-reactance arcs | [Plotting](plotting.md) |
| [`smooth`](../fn/smooth.html) | One word for the smoothers. `x` is a length-N number vector; `window` is a named/positional odd integer, default `5`; `method` is a named string, one of `"savgol"` (default), `"moving"`,.. | [Noise](noise.md) |
| [`smoothmax`](../fn/smoothmax.html) | Smooth (differentiable) approximation to `max(x)`: `(1/beta) * logsumexp(beta * x)`, which tracks the true maximum more tightly as `beta` grows (at the cost of a stiffer gradient) | [Core maths](core-math.md) |
| [`snr`](../fn/snr.html) | With no excitation named, the largest non-DC FFT bin is taken as the fundamental (the single-tone case); `tones=`/`freqs=` (a vector, Hz, needs `fs=`/`rate=` too) or `bins=` (a vector of.. | [Signal processing](signal-processing.md) |
| [`sns_bar`](../fn/sns_bar.html) | Aggregates column `valuecol` (string, column name) within each distinct value of `groupcol` (string, column name) of Table `df`, using aggregator `agg` (string, positional, default.. | [Plotting](plotting.md) |
| [`sns_box`](../fn/sns_box.html) | One `boxplot` group per distinct value of column `groupcol` (string, first-seen order) in Table `df`, each group's values drawn from column `valuecol` (string) and labeled with the group's.. | [Plotting](plotting.md) |
| [`sns_scatter`](../fn/sns_scatter.html) | Seaborn-style scatter pulling columns from Table `df`: `xcol`, `ycol` (strings, column names) give the coordinates; optional `huecol` (string, column name) draws one series per distinct hue.. | [Plotting](plotting.md) |
| [`softmax`](../fn/softmax.html) | Elementwise `exp(x_i) / sum(exp(x))` — the gradient of `logsumexp`, and a probability distribution that sums to 1 | [Core maths](core-math.md) |
| [`softmax_rows`](../fn/softmax_rows.html) | Softmax along each row of an `N`-row, `D`-column matrix `M` of scores, shifted by the row maximum first so a large score cannot overflow the exponential | [Statistics & ML](statistics-ml.md) |
| [`solve`](../fn/solve.html) | Solves `A·x = b` for `x` by LU with partial pivoting (or, for a `CMat`/`CVec` `A`/`b`, by complex SVD least squares) | [Core maths](core-math.md) |
| [`sort`](../fn/sort.html) | A list of strings sorts lexicographically, a numeric vector sorts numerically | [Collections & strings](collections-strings.md) |
| [`sort_by`](../fn/sort_by.html) | Returns a `Table` sorted by that one column | [Collections & strings](collections-strings.md) |
| [`sosfilt`](../fn/sosfilt.html) | Returns a same-length vector or `Signal` (keeping `x`'s own `Fs` on the way out, when `x` is a `Signal`) | [Signal processing](signal-processing.md) |
| [`spawn`](../fn/spawn.html) | Runs an already-defined user function on `rayon`'s global thread pool (the `Worker` abstraction) | [Concurrency](concurrency.md) |
| [`spectral_coherence`](../fn/spectral_coherence.html) | Returns a length-`nperseg/2 + 1` real vector, the magnitude-squared coherence `\|Pxy\|^2 / (Pxx*Pyy)` in `[0, 1]` — `1` at a frequency where `y` is an exact linear/time-invariant function.. | [Signal processing](signal-processing.md) |
| [`spectral_entropy`](../fn/spectral_entropy.html) | Returns a vector of length `num_frames` (one value per STFT frame), each the Shannon entropy of that frame's normalized power spectrum, scaled to `[0, 1]`: low when energy concentrates in a.. | [Signal processing](signal-processing.md) |
| [`spectrogram`](../fn/spectrogram.html) | If `x` is a `Signal`, its own `Fs` is used and `fs` need not be passed; passing one that disagrees with the signal's rate is an error rather than a silent relabelling (see *Which rate is.. | [Signal processing](signal-processing.md) |
| [`spectrum`](../fn/spectrum.html) | Returns a `spectrum` of `floor(N/2)+1` one-sided bins (DC to Nyquist) | [Signal processing](signal-processing.md) |
| [`spectrum_at`](../fn/spectrum_at.html) | Returns the single complex bin nearest that physical frequency (`round(freq/df)`, clamped to the available bins) — addressing the spectrum in Hz instead of by index | [Signal processing](signal-processing.md) |
| [`spectrum_normalize`](../fn/spectrum_normalize.html) | Returns a new `spectrum` with that convention applied — the same conversion `fft`/`rfft(..., scaling=)` do at transform time, usable afterward on a spectrum that already went through.. | [Signal processing](signal-processing.md) |
| [`spectrum_unnormalize`](../fn/spectrum_unnormalize.html) | Returns a new `raw_transform`-scaled `spectrum` — the exact inverse of `spectrum_normalize`/`scaling=` | [Signal processing](signal-processing.md) |
| [`spiderplot`](../fn/spiderplot.html) | Radar chart; each argument (a Vec of numbers, one value per spoke/category) becomes its own series, layered onto the current panel's spider chart (repeated calls add more series) | [Plotting](plotting.md) |
| [`spl`](../fn/spl.html) | Added in v0.2.4. Sound pressure level of a calibrated `Signal`: `20*log10(rms/reference)` using the calibration's own reference (20 µPa for pressure), AC-coupled | [Signal processing](signal-processing.md) |
| [`splineplot`](../fn/splineplot.html) | Natural cubic spline through the control points `(x[i], y[i])` (equal-length numeric Vecs), resampled onto 200 points for a smooth line | [Plotting](plotting.md) |
| [`split`](../fn/split.html) | `s` and `delim` are `Str`s; optional `limit` (number of pieces, default: unlimited) stops after that many pieces and leaves the remainder undivided as the last one — `split(line, ": ", 2)`.. | [Collections & strings](collections-strings.md) |
| [`sqrt`](../fn/sqrt.html) | Elementwise square root. `x` is a scalar, `Vec`, or `Mat`/`Signal`; a negative element gives `NaN` (there is no automatic promotion to a complex result) | [Core maths](core-math.md) |
| [`square`](../fn/square.html) | A bipolar (±1) periodic pulse train | [Plotting](plotting.md) |
| [`stackbar`](../fn/stackbar.html) | Stacked bar chart: `x` (Vec, category positions) and `y` (Mat or list of Vecs, one row/series per stack segment) with the same `values=`/`hatch=` options as `bar` | [Plotting](plotting.md) |
| [`stair`](../fn/stair.html) | Step (staircase) plot of equal-length numeric Vecs `x`, `y` | [Plotting](plotting.md) |
| [`stamp`](../fn/stamp.html) | Places a transformed copy of a captured `layer` (the value `capture` returned) at position `(x, y)` (numbers, data units) | [Plotting](plotting.md) |
| [`standardize`](../fn/standardize.html) | z-score scaling of `X` (vector, matrix column-wise, or table per numeric column): `(x - mean) / std` | [Statistics & ML](statistics-ml.md) |
| [`start`](../fn/start.html) | Begins counting, or resumes from `pause` | [REPL & diagnostics](repl-diagnostics.md) |
| [`start_time`](../fn/start_time.html) | Added in v0.2.4. The time of sample 0, in seconds (`0` unless `set_start_time` was called, so nothing about an un-dated signal changes) | [Signal processing](signal-processing.md) |
| [`starts_with`](../fn/starts_with.html) | Returns a `bool`: `true` if `s` starts with `prefix` | [Collections & strings](collections-strings.md) |
| [`stationary`](../fn/stationary.html) | Computes the long-run state distribution of a `markov_chain` `chain` by power iteration; converges for any irreducible, aperiodic chain | [Statistics & ML](statistics-ml.md) |
| [`std`](../fn/std.html) | Sample standard deviation (N-1, unbiased, denominator) of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal` | [Core maths](core-math.md) |
| [`ste`](../fn/ste.html) | Returns a vector of length `num_frames`, one energy value per frame | [Signal processing](signal-processing.md) |
| [`steer_delays`](../fn/steer_delays.html) | The geometry half of `beamform` on its own: the per-element steering delays `tau_m = x_m*sin(theta)/c`, in seconds, for element coordinates `positions` and a direction `angle_degrees`.. | [Signal processing](signal-processing.md) |
| [`stem`](../fn/stem.html) | Discrete stem plot: `x`, `y` are equal-length Vecs of numbers; draws a vertical line plus a dot from the baseline up to each `(x[i], y[i])` (marker `"stem"`) | [Plotting](plotting.md) |
| [`step`](../fn/step.html) | Applies one `action` (an integer 0-3) from the current integer `state` index in environment `env` | [Statistics & ML](statistics-ml.md) |
| [`stft`](../fn/stft.html) | Returns an `nfft`×`num_frames` complex matrix (`CMat`, the full per-frame spectrum, not one-sided) — one column per frame, built from a periodic-Hann-windowed FFT per frame | [Signal processing](signal-processing.md) |
| [`stop`](../fn/stop.html) | Freezes the timer and resets the count to zero (unlike `pause`, which keeps it); returns `none` | [REPL & diagnostics](repl-diagnostics.md) |
| [`stop_grad`](../fn/stop_grad.html) | Returns the plain, untracked inner value (same numeric content and shape) detached from the tape, so gradients don't flow through it — e.g. holding a target/label steady, or truncating.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`str`](../fn/str.html) | Renders `x` (any value/type) the same way `print`/`disp` would | [Collections & strings](collections-strings.md) |
| [`stratified_split`](../fn/stratified_split.html) | The same shape as `train_test_split` on `X`/`y`/`test_size`/`seed=`, but splits each class of `y` separately before combining, so a rare class keeps its proportion in both sets every time.. | [Statistics & ML](statistics-ml.md) |
| [`subplot`](../fn/subplot.html) | Selects cell `k` (integer, 1-based, row-major) of a `rows`-by-`cols` (integers) grid as the target for further drawing | [Plotting](plotting.md) |
| [`substr`](../fn/substr.html) | Plain aliases of the same implementation (`mid` exists purely for BASIC/Excel discoverability) | [Collections & strings](collections-strings.md) |
| [`subtract`](../fn/subtract.html) | The operators as named functions, so an operation can be passed where a name is what you can pass | [Core maths](core-math.md) |
| [`sum`](../fn/sum.html) | Sum of every element of `x` — a scalar, `Vec`, or `Mat`/`Signal` (real; additionally accepts a `CVec`/`CMat` and returns a `Complex` in that case) | [Core maths](core-math.md) |
| [`svd`](../fn/svd.html) | Thin SVD of `A` (`r×c` `Mat` or `CMat`), `A = U·diag(s)·Vᵀ` | [Core maths](core-math.md) |
| [`svm_model`](../fn/svm_model.html) | Binary-classification support-vector machine by simplified SMO on an `N`-row, `D`-column `X` and a length-`N` target `y` that must take exactly two distinct values (no one-vs-rest wrapper) | [Statistics & ML](statistics-ml.md) |
| [`svr_model`](../fn/svr_model.html) | Support-vector regression on an `N`-row, `D`-column `X` and a length-`N` numeric target `y` | [Statistics & ML](statistics-ml.md) |
| [`swap`](../fn/swap.html) | `swap` atomically exchanges front and back (returns `Nothing`); `read` returns the stable front value | [Collections & strings](collections-strings.md) |
| [`sweep`](../fn/sweep.html) | Generates a linear (or, with `method="logarithmic"`, log) frequency sweep from `f0` to `f1` (numbers, Hz) sampled at rate `fs` (number, Hz) for `n` (integer) samples | [Plotting](plotting.md) |
| [`sysid`](../fn/sysid.html) | Model selection: fits a ladder of standard topologies and picks one by an information criterion | [Signal processing](signal-processing.md) |
| [`sysinfo`](../fn/sysinfo.html) | Returns a `Record` describing this machine: `.os`/`.arch`/`.hostname`/`.cpu`/`.qu_version`/`.build` (strings), `.logical_cores` (integer), `.physical_cores` and `.total_memory` (number, or.. | [Concurrency](concurrency.md) |

## T

| | | |
|---|---|---|
| [`table`](../fn/table.html) | Each named argument is a column: `values` is either a numeric `Vec` / `List` of `Str` (a real column) or a scalar `number`/`Str` (broadcast to every row) | [Collections & strings](collections-strings.md) |
| [`tail`](../fn/tail.html) | Returns the first (`head`) or last (`tail`) `n` elements/rows, same type as `s` | [Collections & strings](collections-strings.md) |
| [`take`](../fn/take.html) | Returns the same type as `xs`: the first `n` elements (`take`) or everything after them (`drop`) | [Collections & strings](collections-strings.md) |
| [`tan`](../fn/tan.html) | Standard trigonometric functions, computed elementwise | [Core maths](core-math.md) |
| [`tanh`](../fn/tanh.html) | Hyperbolic sine/cosine/tangent, elementwise | [Core maths](core-math.md) |
| [`tape_reset`](../fn/tape_reset.html) | Discards every entry recorded so far on the autodiff tape, freeing the memory a chain of `param`/`grad` calls has built up | [Statistics & ML](statistics-ml.md) |
| [`tcp_accept`](../fn/tcp_accept.html) | Returns a `TcpListener` handle from `tcp_listen`, and a `TcpConn` handle from `tcp_accept` | [Concurrency](concurrency.md) |
| [`tcp_close`](../fn/tcp_close.html) | `sock` must be a `TcpConn` handle (from `tcp_accept`/`tcp_connect`) — not a `TcpListener`; calling it on a listener errors, since a listener has no explicit close and simply releases its OS.. | [Concurrency](concurrency.md) |
| [`tcp_connect`](../fn/tcp_connect.html) | `tcp_connect(host, port)`: `host` (string, hostname or IP) and `port` (integer 0-65535) — blocks until connected (or the OS gives up and errors), returning a `TcpConn` handle, the.. | [Concurrency](concurrency.md) |
| [`tcp_listen`](../fn/tcp_listen.html) | Returns a `TcpListener` handle from `tcp_listen`, and a `TcpConn` handle from `tcp_accept` | [Concurrency](concurrency.md) |
| [`tcp_port`](../fn/tcp_port.html) | `tcp_connect(host, port)`: `host` (string, hostname or IP) and `port` (integer 0-65535) — blocks until connected (or the OS gives up and errors), returning a `TcpConn` handle, the.. | [Concurrency](concurrency.md) |
| [`tcp_recv`](../fn/tcp_recv.html) | Neither call frames the message for you — TCP is a stream, so a length prefix or delimiter is yours to add if you need distinct messages | [Concurrency](concurrency.md) |
| [`tcp_send`](../fn/tcp_send.html) | Neither call frames the message for you — TCP is a stream, so a length prefix or delimiter is yours to add if you need distinct messages | [Concurrency](concurrency.md) |
| [`tell`](../fn/tell.html) | Reports `f`'s (file handle, opened readable) current read position | [File I/O](file-io.md) |
| [`tex`](../fn/tex.html) | Not figure export — converts a single Qu value `x` (number, Vec, Mat, or complex) to a LaTeX math-mode string (e.g. a matrix becomes a `bmatrix` environment) and returns it as a string | [Plotting](plotting.md) |
| [`text`](../fn/text.html) | Plain text label `text` (string) placed at `(x, y)` (numbers, data units), no marker dot | [Plotting](plotting.md) |
| [`thd`](../fn/thd.html) | Returns a single number, dB below the fundamental | [Signal processing](signal-processing.md) |
| [`thd_n`](../fn/thd_n.html) | Returns a single number, dB below the fundamental: harmonics *and* noise together against the signal (`-sinad_db`, the same `noise_power` `sinad` already totals over every non-excitation.. | [Signal processing](signal-processing.md) |
| [`theme`](../fn/theme.html) | Sets the whole-figure visual theme from `name` (string): one of `"default"`, `"publication"`, `"bw"`, `"minimal"`, `"grey"`, `"classic"` | [Plotting](plotting.md) |
| [`threshold`](../fn/threshold.html) | Elementwise binary threshold, general-purpose (not image-specific) | [Images](images.md) |
| [`tic`](../fn/tic.html) | Starts (or restarts) the single global stopwatch | [REPL & diagnostics](repl-diagnostics.md) |
| [`timestamps`](../fn/timestamps.html) | Added in v0.2.4. The absolute time axis as a vector, `start_time + i/Fs` for each sample | [Signal processing](signal-processing.md) |
| [`title`](../fn/title.html) | Sets the current panel's title to `text` (string) | [Plotting](plotting.md) |
| [`tkeo`](../fn/tkeo.html) | Returns a length-`(N-2)` vector (like `diff`, doesn't pad — the definition needs a neighbor on each side) | [Signal processing](signal-processing.md) |
| [`tmp_file`](../fn/tmp_file.html) | Creates a new empty file under the OS temp directory with a name nothing else will collide with (process id, nanosecond clock, and a counter — any one alone has a plausible collision window) | [File I/O](file-io.md) |
| [`to_bool`](../fn/to_bool.html) | For a `Str`, `x` is parsed, not weighed: `"true"`/`"false"` (any case) and any number spelling work, and anything else errors — because a config file holding `"false"` means false, and.. | [Collections & strings](collections-strings.md) |
| [`to_cmyk`](../fn/to_cmyk.html) | Takes apart any colour the language accepts (`color`, a string) into that space's own coordinates, returned as a Vec of numbers — `to_rgb` gives `(r,g,b)`, `to_hsl` gives `(h,s,l)`,.. | [Plotting](plotting.md) |
| [`to_digital`](../fn/to_digital.html) | Returns a `Digital` value (a `Model`, kind `"digital"`) remembering `x` and `threshold` together, so `edges`/`rising_edges`/`falling_edges`/`high_time`/`low_time`/`duty_cycle` below can be.. | [Signal processing](signal-processing.md) |
| [`to_float`](../fn/to_float.html) | The parsing half of `to_int` without the rounding — mostly for turning text read from a file or an instrument into a number | [Collections & strings](collections-strings.md) |
| [`to_hsl`](../fn/to_hsl.html) | Takes apart any colour the language accepts (`color`, a string) into that space's own coordinates, returned as a Vec of numbers — `to_rgb` gives `(r,g,b)`, `to_hsl` gives `(h,s,l)`,.. | [Plotting](plotting.md) |
| [`to_hsv`](../fn/to_hsv.html) | Takes `color` (any string/value the language accepts as a colour — name, hex, `rgb(...)`, etc.) and returns its `(h, s, v)` triple as a Vec of numbers | [Plotting](plotting.md) |
| [`to_int`](../fn/to_int.html) | Rounds to the nearest whole number, ties away from zero — the same primitive `round` uses, so the two agree by construction rather than by coincidence | [Collections & strings](collections-strings.md) |
| [`to_lab`](../fn/to_lab.html) | Takes apart any colour the language accepts (`color`, a string) into that space's own coordinates, returned as a Vec of numbers — `to_rgb` gives `(r,g,b)`, `to_hsl` gives `(h,s,l)`,.. | [Plotting](plotting.md) |
| [`to_rgb`](../fn/to_rgb.html) | Takes apart any colour the language accepts (`color`, a string) into that space's own coordinates, returned as a Vec of numbers — `to_rgb` gives `(r,g,b)`, `to_hsl` gives `(h,s,l)`,.. | [Plotting](plotting.md) |
| [`to_unit`](../fn/to_unit.html) | Added in v0.2.4. Alias of `convert_unit(s, unit)` above — the spec's own second spelling, kept so a reader following `toolkit-signal.md` §10 literally finds it | [Signal processing](signal-processing.md) |
| [`to_vec`](../fn/to_vec.html) | Returns a `List`: the linked list's elements, front to back | [Collections & strings](collections-strings.md) |
| [`toc`](../fn/toc.html) | Returns a number: elapsed seconds (float, fractional) since the last `tic()` | [REPL & diagnostics](repl-diagnostics.md) |
| [`tolower`](../fn/tolower.html) | Returns a `Str`: C's names for `upper`/`lower`, accepted so a ported line does not have to be edited | [Collections & strings](collections-strings.md) |
| [`touch`](../fn/touch.html) | Unix `touch` semantics. `path` (string) is the filesystem path: creates an empty file if it doesn't exist; if it already exists, updates its modification time WITHOUT altering its content | [File I/O](file-io.md) |
| [`toupper`](../fn/toupper.html) | Returns a `Str`: C's names for `upper`/`lower`, accepted so a ported line does not have to be edited | [Collections & strings](collections-strings.md) |
| [`trace`](../fn/trace.html) | The sum of the diagonal of `M`, a square (or rectangular — only `min(rows,cols)` diagonal entries are summed) `Mat` | [Core maths](core-math.md) |
| [`track`](../fn/track.html) | Wraps a plain number, vector, or matrix `v` as a fresh, differentiable Tensor — a new leaf node on the autodiff tape with no inputs of its own, so `grad` treats it as something to.. | [Statistics & ML](statistics-ml.md) |
| [`train_loop`](../fn/train_loop.html) | Runs the fit loop inside the engine rather than stepping it from Qu: `loss_fn_name` a string naming an in-scope function `(params) -> number`, `params` the Tensor (from `param(...)`) or.. | [Statistics & ML](statistics-ml.md) |
| [`train_test_split`](../fn/train_test_split.html) | Splits an `N`-row feature matrix `X` and a length-`N` target vector `y` into training and test sets, `X` and `y` permuted together by the same random order (unless `shuffle=false`) | [Statistics & ML](statistics-ml.md) |
| [`train_val_test_split`](../fn/train_val_test_split.html) | Three-way split of `X`/`y` into training, validation, and test sets: `val_size`/`test_size` are numbers in `(0, 1)` (both default 0.2); `val_size` is rescaled against the remainder left.. | [Statistics & ML](statistics-ml.md) |
| [`transfer_function`](../fn/transfer_function.html) | Returns an `frf` model carrying the frequency response of the system taking `input` to `output`, with fields `h` (length-`nperseg/2 + 1` `CVec`), `freq` (the matching Hz axis), `coherence`,.. | [Signal processing](signal-processing.md) |
| [`transform`](../fn/transform.html) | Applies a fitted scaler Record `scaler`'s stored parameters to a new matrix/vector/table `newX` with the same columns it was fitted on | [Statistics & ML](statistics-ml.md) |
| [`transformer_block`](../fn/transformer_block.html) | One transformer block on an `(N, D)` input `x`: multi-head self-attention plus a two-layer feed-forward network, each wrapped in a residual connection and a `layer_norm` | [Statistics & ML](statistics-ml.md) |
| [`transpose`](../fn/transpose.html) | Matrix/vector transpose: swaps rows and columns | [Core maths](core-math.md) |
| [`tree_model`](../fn/tree_model.html) | One CART decision tree on an `N`-row, `D`-column matrix `X` and a length-`N` target `y`, with `max_depth` (integer, default 20) limiting tree depth, `min_samples_split` (integer, default 2).. | [Statistics & ML](statistics-ml.md) |
| [`triangle`](../fn/triangle.html) | Each generates one period-repeating waveform: `freq` (number, Hz), `fs` (number, sample rate in Hz), `n` (integer, sample count) are all scalars, in that order | [Plotting](plotting.md) |
| [`trim`](../fn/trim.html) | Strips characters from both ends of a string | [Collections & strings](collections-strings.md) |
| [`tsne`](../fn/tsne.html) | t-SNE embedding of an `N`-row, `D`-column matrix `X` into `n_components` dimensions (an integer, default 2), controlled by `perplexity` (a number balancing local vs. global structure; unset.. | [Statistics & ML](statistics-ml.md) |
| [`tv_denoise`](../fn/tv_denoise.html) | Total-variation denoising: `y` is a length-N number vector; `lambda` is an optional positional number, the smoothing penalty, default `1.0` — larger values remove more variation | [Noise](noise.md) |
| [`type`](../fn/type.html) | Returns a `Str`: the value's type, e.g. `"number"`, `"string"`, `"vector"`, `"matrix"`, `"table"`, `"layer"` | [Collections & strings](collections-strings.md) |

## U

| | | |
|---|---|---|
| [`ucase`](../fn/ucase.html) | BASIC's names for `upper`/`lower`; `toupper`/`tolower` are C's names for the same thing | [Collections & strings](collections-strings.md) |
| [`ui_button`](../fn/ui_button.html) | Declares a push-button, labeled `label` (string); `id=` (string) as above | [Plotting](plotting.md) |
| [`ui_checkbox`](../fn/ui_checkbox.html) | Declares an on/off toggle, labeled `label` (string) | [Plotting](plotting.md) |
| [`ui_number`](../fn/ui_number.html) | Declares a free-entry number box (for a value you type exactly rather than sweep), labeled `label` (string) | [Plotting](plotting.md) |
| [`ui_select`](../fn/ui_select.html) | Declares a dropdown, labeled `label` (string), choosing among `options` (a List of strings — or of any values, displayed via their string form) | [Plotting](plotting.md) |
| [`ui_slider`](../fn/ui_slider.html) | Declares a slider control (for a value you sweep) in an `explore` page or Qu Studio, labeled `label` (string) with range `min`..`max` (numbers) | [Plotting](plotting.md) |
| [`ui_text`](../fn/ui_text.html) | Declares a free-text field, labeled `label` (string) | [Plotting](plotting.md) |
| [`undershoot`](../fn/undershoot.html) | Returns a single number, a percent of the step's own size `abs(final - initial)`: `overshoot` is how far the response travels *past its settled value* in the direction the step was going,.. | [Signal processing](signal-processing.md) |
| [`uniform`](../fn/uniform.html) | Draws from the uniform distribution on `[a, b)`, two numbers `a < b`, with the same `rows`/`cols`/`seed=` shape as `rand` | [Statistics & ML](statistics-ml.md) |
| [`unique`](../fn/unique.html) | Returns a `Vec`: a sorted, de-duplicated copy | [Collections & strings](collections-strings.md) |
| [`unit_scale`](../fn/unit_scale.html) | Returns the scale factor a `unit name = ...` declaration registered, or errors naming `name` if nothing was ever declared under it | [REPL & diagnostics](repl-diagnostics.md) |
| [`update`](../fn/update.html) | Estimation-family only: the measurement-update half of a filter, called on a state Record `state` with filter-specific arguments (an observation matrix/function, a measurement vector, and a.. | [Statistics & ML](statistics-ml.md) |
| [`upper`](../fn/upper.html) | Unicode-aware uppercase conversion | [Collections & strings](collections-strings.md) |
| [`upsample`](../fn/upsample.html) | Raises the sample rate by an integer factor: inserts `L-1` zeros between samples, then interpolates with a lowpass at the original Nyquist (`fs/2`), scaled by `L` to preserve amplitude | [Signal processing](signal-processing.md) |

## V

| | | |
|---|---|---|
| [`val`](../fn/val.html) | Materializes a value; currently the identity on any `x` (`:=` is already eager pre-M4) but will force evaluation of a deferred binding once M4 lazy fusion lands | [Core maths](core-math.md) |
| [`value_and_grad`](../fn/value_and_grad.html) | Takes a function `f` and a point `x`, and returns a two-element list `[f(x), grad_f(x)]` from a single forward pass, with the gradient shaped like `x` | [REPL & diagnostics](repl-diagnostics.md) |
| [`values`](../fn/values.html) | Returns a `List`: its keys (`keys`), its values (`values`), or `(key, value)` tuples (`items`) — insertion order, not sorted | [Collections & strings](collections-strings.md) |
| [`vanicek`](../fn/vanicek.html) | Returns a length-M real vector, the power spectrum at each requested frequency — Vanicek's Least-Squares Spectral Analysis, valid for unevenly-sampled `(t, x)` where `fft`/`dft`/`goertzel`.. | [Signal processing](signal-processing.md) |
| [`var`](../fn/var.html) | Sample variance, same N-1 (unbiased) denominator as `std`, so `var(x) == std(x)^2` always holds | [Core maths](core-math.md) |
| [`verify_signal`](../fn/verify_signal.html) | Structural sanity before any of the semantic diagnostics above is worth running — deliberately narrow: it answers "are these samples a usable record", not "is this signal any good" | [Signal processing](signal-processing.md) |
| [`violin`](../fn/violin.html) | The same Gaussian KDE and Tukey summary as `raincloud`, arranged the other way: the density is MIRRORED about the category into the closed symmetric shape, with the box inside it and the.. | [Plotting](plotting.md) |
| [`viterbi`](../fn/viterbi.html) | Runs the Viterbi algorithm on an `hmm` model `hmm` for a length-`T` integer observation sequence `obs` (same encoding as `forward`) | [Statistics & ML](statistics-ml.md) |
| [`vline`](../fn/vline.html) | Draws a vertical reference line spanning the whole panel at `x` (number, data units) | [Plotting](plotting.md) |
| [`vmap`](../fn/vmap.html) | Applies `f` across the leading batch axis of `xs` — rows of a matrix, elements of a vector, items of a list — and stacks the results: a vector when every result is a scalar, a matrix (one.. | [REPL & diagnostics](repl-diagnostics.md) |
| [`vmd`](../fn/vmd.html) | Variational Mode Decomposition: decomposes `x` into exactly `k` band-limited modes by minimizing each mode's bandwidth around its own center frequency, solved by ADMM (not sifting, unlike.. | [Signal processing](signal-processing.md) |
| [`voronoi`](../fn/voronoi.html) | Computes the Voronoi diagram of an `(N, 2)` point matrix `points`, built on a real Delaunay triangulation | [Statistics & ML](statistics-ml.md) |
| [`vstack`](../fn/vstack.html) | Named equivalents of bracket-literal block concatenation (`[a, b]` horizontally / `[a; b]` vertically) | [Core maths](core-math.md) |
| [`vswr`](../fn/vswr.html) | Returns `(1 + \|Γ\|)/(1 - \|Γ\|)` | [Signal processing](signal-processing.md) |

## W

| | | |
|---|---|---|
| [`warburg`](../fn/warburg.html) | `Aw (1-j)/sqrt(w)` — semi-infinite diffusion, a constant −45° phase at every frequency | [Signal processing](signal-processing.md) |
| [`warburg_open`](../fn/warburg_open.html) | `Rw coth(x)/x`, `x = sqrt(jw tau)` — finite length, reflecting boundary | [Signal processing](signal-processing.md) |
| [`warburg_short`](../fn/warburg_short.html) | `Rw tanh(x)/x`, `x = sqrt(jw tau)` — finite length, transmissive boundary | [Signal processing](signal-processing.md) |
| [`warn`](../fn/warn.html) | Non-fatal — execution continues on the next line, unlike `error` | [REPL & diagnostics](repl-diagnostics.md) |
| [`waterfall`](../fn/waterfall.html) | A 2-D approximation of the stacked-trace look — no true 3-D projection yet | [Plotting](plotting.md) |
| [`welch`](../fn/welch.html) | Returns a `spectrum` of `nperseg/2 + 1` one-sided bins (DC to Nyquist), stamped `norm = "density"` — SciPy's `"density"` scaling, units²/Hz | [Signal processing](signal-processing.md) |
| [`where`](../fn/where.html) | One argument `mask` (a `Vec`/`Mat` of booleans, typically produced by a comparison like `x < 0`) returns a `Vec` of the 0-based indices where it is true. Three arguments — `cond` (a boolean.. | [Core maths](core-math.md) |
| [`wigner_ville`](../fn/wigner_ville.html) | Wigner-Ville distribution: the quadratic time-frequency distribution `W(t,f) = ∫ x(t+τ/2)*conj(x(t-τ/2))*exp(-2πifτ) dτ`, discretized over the analytic signal | [Signal processing](signal-processing.md) |
| [`worker_done`](../fn/worker_done.html) | Non-blocking check of whether worker `w` (a `Worker` handle from `spawn`) has finished | [Concurrency](concurrency.md) |
| [`wrap`](../fn/wrap.html) | Returns the same shape as `x`, each value reduced modulo `hi - lo` into that range — the standard fix for angle/phase data that has accumulated past a full turn (`atan2` output, an.. | [Signal processing](signal-processing.md) |
| [`write`](../fn/write.html) | Dispatches on the type of the first argument | [REPL & diagnostics](repl-diagnostics.md) |
| [`write_array`](../fn/write_array.html) | Writes a whole `Vec`/`Mat`/`Signal` (or a bare number/`bool`) to a binary file in one call, without an explicit `fopen` — the write-side counterpart `read_array` never had | [File I/O](file-io.md) |
| [`write_bin`](../fn/write_bin.html) | The inverse of `read_bin`: writes raw bytes to `f` (file handle, opened writable/appendable) | [File I/O](file-io.md) |
| [`write_bit`](../fn/write_bit.html) | Writes one bit to `f` (file handle, opened writable/appendable) | [File I/O](file-io.md) |
| [`write_byte`](../fn/write_byte.html) | Writes one raw byte to `f` (file handle, opened writable/appendable) | [File I/O](file-io.md) |
| [`write_char`](../fn/write_char.html) | Writes `s` (string) to `f` (file handle, opened writable/appendable) with no newline added | [File I/O](file-io.md) |
| [`write_csv`](../fn/write_csv.html) | Writes a `Table` to a CSV file | [File I/O](file-io.md) |
| [`write_double`](../fn/write_double.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a 64-bit IEEE-754 float | [File I/O](file-io.md) |
| [`write_float`](../fn/write_float.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a 32-bit IEEE-754 float | [File I/O](file-io.md) |
| [`write_int`](../fn/write_int.html) | The mirror of `read_int`, writing `value` (a number) to `f` (file handle, opened writable/appendable) in the given width and byte order | [File I/O](file-io.md) |
| [`write_int16`](../fn/write_int16.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 16-bit integer | [File I/O](file-io.md) |
| [`write_int32`](../fn/write_int32.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 32-bit integer | [File I/O](file-io.md) |
| [`write_int64`](../fn/write_int64.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 64-bit integer | [File I/O](file-io.md) |
| [`write_line`](../fn/write_line.html) | Writes `s` (string) to `f` (file handle, opened writable/appendable) plus a trailing `\n` | [File I/O](file-io.md) |
| [`write_report`](../fn/write_report.html) | Writes a standalone HTML report to `path` (string) — every figure, table, and printed line the script has produced up to this point in execution, bundled into one file with nothing linked.. | [File I/O](file-io.md) |
| [`write_text`](../fn/write_text.html) | Writes the whole string to a file, overwriting it | [Collections & strings](collections-strings.md) |
| [`write_uint16`](../fn/write_uint16.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 16-bit integer | [File I/O](file-io.md) |
| [`write_uint32`](../fn/write_uint32.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 32-bit integer | [File I/O](file-io.md) |
| [`write_uint64`](../fn/write_uint64.html) | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 64-bit integer | [File I/O](file-io.md) |
| [`writeline`](../fn/writeline.html) | Writes its arguments, space-separated, and a newline, to stdout | [Collections & strings](collections-strings.md) |

## X

| | | |
|---|---|---|
| [`xbreak`](../fn/xbreak.html) | Reshapes the *main* x-axis's coordinate mapping with a fixed-width "//" jag that squeezes out the skipped range `[lo, hi]` (numbers, data units) — linear-scale axes only | [Plotting](plotting.md) |
| [`xcorr`](../fn/xcorr.html) | Returns a length-`N+M-1` vector, the full lag range (zero lag at the middle for equal-length inputs) | [Signal processing](signal-processing.md) |
| [`xlabel`](../fn/xlabel.html) | Labels the current panel's x-axis with `text` (string) | [Plotting](plotting.md) |
| [`xlim`](../fn/xlim.html) | Fixes an axis range from `lo`, `hi` (numbers, data units) | [Plotting](plotting.md) |
| [`xml2csv`](../fn/xml2csv.html) | `str` (string, XML text) is converted to CSV text: exactly `csvify(parse_xml(str))`. Returns a string | [File I/O](file-io.md) |
| [`xml2json`](../fn/xml2json.html) | Like `csv2json`, this produces the same plain array-of-row-objects shape, not `jsonify(parse_xml(str))`'s type-tagged envelope | [File I/O](file-io.md) |
| [`xmlify`](../fn/xmlify.html) | Converts any `value` (any `Value`) to an XML string — see "The `Value`<->XML mapping" below | [File I/O](file-io.md) |
| [`xor`](../fn/xor.html) | Returns a `bool`: logical XOR, non-short-circuit like `and`/`or` | [Collections & strings](collections-strings.md) |
| [`xscale`](../fn/xscale.html) | Sets that axis's scale from a string, `"linear"` or `"log"` | [Plotting](plotting.md) |
| [`xspan`](../fn/xspan.html) | Draws a translucent vertical band covering `x` in `[x0, x1]` (numbers, data units) across the panel's full height | [Plotting](plotting.md) |
| [`xticklabels`](../fn/xticklabels.html) | Sets categorical x tick labels from the given strings (variadic positional arguments); without an `xticks(...)` call, spread evenly across the current x-extent | [Plotting](plotting.md) |
| [`xticks`](../fn/xticks.html) | Sets explicit x tick positions from `positions` (Vec of numbers, data units), paired in order with `xticklabels(...)` when both are set | [Plotting](plotting.md) |

## Y

| | | |
|---|---|---|
| [`ybreak`](../fn/ybreak.html) | Same as `xbreak`, on the y-axis: squeezes out `[lo, hi]` (numbers, data units) | [Plotting](plotting.md) |
| [`ylabel`](../fn/ylabel.html) | Labels the current panel's y-axis with `text` (string) | [Plotting](plotting.md) |
| [`ylim`](../fn/ylim.html) | Fixes an axis range from `lo`, `hi` (numbers, data units) | [Plotting](plotting.md) |
| [`yscale`](../fn/yscale.html) | Sets that axis's scale from a string, `"linear"` or `"log"` | [Plotting](plotting.md) |
| [`yspan`](../fn/yspan.html) | Draws a translucent horizontal band covering `y` in `[y0, y1]` (numbers, data units) across the panel's full width | [Plotting](plotting.md) |
| [`yticklabels`](../fn/yticklabels.html) | `yticks` takes a Vec of numbers (tick positions, data units); `yticklabels` takes variadic strings to relabel them in order | [Plotting](plotting.md) |
| [`yticks`](../fn/yticks.html) | `yticks` takes a Vec of numbers (tick positions, data units); `yticklabels` takes variadic strings to relabel them in order | [Plotting](plotting.md) |

## Z

| | | |
|---|---|---|
| [`zeros`](../fn/zeros.html) | A vector or matrix of zeros. One numeric argument `n` gives a length-`n` `Vec`; two arguments `r`, `c` give an `r×c` `Mat` — even a degenerate two-argument shape like `zeros(n, 1)` stays a.. | [Core maths](core-math.md) |
| [`zeros_like`](../fn/zeros_like.html) | A vector or matrix of the same shape as `x` (a `Vec` or `Mat`), filled with one or zero respectively — the shape is read directly from `x`, so it cannot fall out of step with it | [Core maths](core-math.md) |
| [`zip`](../fn/zip.html) | Returns a `List` of tuples, pairing up the collections element by element, stopping at the shortest | [Collections & strings](collections-strings.md) |
| [`zlib_decompress`](../fn/zlib_decompress.html) | Inflates a zlib stream (RFC 1950 wrapper around RFC 1951 DEFLATE): `data` is a `Str` or a `Vec` of 0-255 byte values; `as_str` (named boolean, default `false`) asks for text instead of bytes | [File I/O](file-io.md) |
| [`zoom_inset`](../fn/zoom_inset.html) | Magnifies the data region `x` in `[x0, x1]`, `y` in `[y0, y1]` (numbers, data units) into an auto-placed inset panel, with a dashed zoom-box drawn over the source region and connector lines.. | [Plotting](plotting.md) |

## Native modules

Not builtins, and deliberately not in the count above: these
come from modules compiled into the engine, and a program
reaches them with `import`, which opens the bare name and
leaves the qualified one available too.

| | | |
|---|---|---|
| [`codec.decode_flac`](../fn/codec.decode_flac.html) | Decodes a FLAC file to samples | [File I/O](file-io.md) |
| [`codec.decode_mp3`](../fn/codec.decode_mp3.html) | Decodes an MP3 file. Same arguments and same two return forms as `codec.decode_flac` | [File I/O](file-io.md) |
| [`codec.decode_wav`](../fn/codec.decode_wav.html) | Decodes a WAV file. Same arguments and same two return forms as `codec.decode_flac` | [File I/O](file-io.md) |
| [`codec.encode_wav`](../fn/codec.encode_wav.html) | The same encoder as `codec.write_wav`, returning the file's bytes as a `Vec` of whole numbers 0-255 instead of writing them — the exact inverse of what `codec.decode_wav` accepts, so a.. | [File I/O](file-io.md) |
| [`codec.flac_info`](../fn/codec.flac_info.html) | Reads a FLAC file's header without decoding any audio, which is how to check a file is what you expect before paying for the samples | [File I/O](file-io.md) |
| [`codec.write_wav`](../fn/codec.write_wav.html) | Writes audio out as a WAV file | [File I/O](file-io.md) |
| [`xlsx.read`](../fn/xlsx.read.html) | Reads one worksheet of a workbook into a `Table`, the same type `read_csv` produces, so everything that consumes a CSV consumes a spreadsheet unchanged | [File I/O](file-io.md) |
| [`xlsx.sheets`](../fn/xlsx.sheets.html) | Names a workbook's worksheets without reading any of them, so a program can find out what it is holding before deciding what to load | [File I/O](file-io.md) |
| [`xlsx.write`](../fn/xlsx.write.html) | Writes a `Table` out as a one-worksheet workbook | [File I/O](file-io.md) |
| [`pdf.extract_pages`](../fn/pdf.extract_pages.html) | A new PDF holding just the pages asked for | [File I/O](file-io.md) |
| [`pdf.extract_text`](../fn/pdf.extract_text.html) | Text reconstructed from a PDF's content streams | [File I/O](file-io.md) |
| [`pdf.info`](../fn/pdf.info.html) | Reads what a PDF says about itself, without extracting any content | [File I/O](file-io.md) |
| [`pdf.merge`](../fn/pdf.merge.html) | Joins several PDFs into one, in list order, keeping every page | [File I/O](file-io.md) |
| [`pdf.page_count`](../fn/pdf.page_count.html) | Counts a PDF's pages. `src` (string path, or `Vec` of bytes) is the document | [File I/O](file-io.md) |
| [`pdf.write_merge`](../fn/pdf.write_merge.html) | Exactly `pdf.merge`, written to a file instead of returned | [File I/O](file-io.md) |
| [`pdf.write_pages`](../fn/pdf.write_pages.html) | Exactly `pdf.extract_pages`, written to a file instead of returned | [File I/O](file-io.md) |
| [`image.load`](../fn/image.load.html) | not described in a chapter yet | |
| [`image.luma`](../fn/image.luma.html) | not described in a chapter yet | |
| [`image.regions`](../fn/image.regions.html) | Returns a `Table`, one row per non-empty label, ordered by label id — the `regionprops`-shaped measurement but as columns rather than a `List` of `Record`s, so a measurement can be.. | [Images](images.md) |
| [`image.blur`](../fn/image.blur.html) | not described in a chapter yet | |
| [`image.canny`](../fn/image.canny.html) | not described in a chapter yet | |
| [`image.bilateral`](../fn/image.bilateral.html) | not described in a chapter yet | |
| [`image.distance_transform`](../fn/image.distance_transform.html) | not described in a chapter yet | |
| [`image.skeleton`](../fn/image.skeleton.html) | not described in a chapter yet | |
| [`image.fill_holes`](../fn/image.fill_holes.html) | not described in a chapter yet | |
| [`image.contours`](../fn/image.contours.html) | not described in a chapter yet | |
| [`image.autocrop`](../fn/image.autocrop.html) | not described in a chapter yet | |
| [`image.sobel`](../fn/image.sobel.html) | Computes the 3x3 Sobel gradient over `img`'s BT.601 luma (replicate/clamped border) | [Images](images.md) |
| [`image.scharr`](../fn/image.scharr.html) | Same signature, same `direction=` convention, and the same border/luma handling as `image.sobel`, but with the Scharr 3x3 kernel (`[-3,0,3; -10,0,10; -3,0,3]`, transposed for the.. | [Images](images.md) |
| [`image.laplacian`](../fn/image.laplacian.html) | Any other `kernel_size` is refused by name | [Images](images.md) |
| [`image.gradient_magnitude`](../fn/image.gradient_magnitude.html) | Gradient magnitude `sqrt(gx^2 + gy^2)` from the Sobel gradient over `img`'s BT.601 luma — the same value `image.sobel(img)`/`image.sobel(img, direction="both")` returns, under its own more.. | [Images](images.md) |
| [`image.rgb2hsv`](../fn/image.rgb2hsv.html) | Per-pixel RGB→HSV conversion (the whole-`Image` counterpart to the scalar `to_hsv`), reusing the exact same conversion the scalar builtin uses | [Images](images.md) |
| [`image.hsv2rgb`](../fn/image.hsv2rgb.html) | Converts back to 8-bit RGB, rounding and clamping each channel to `0..255` | [Images](images.md) |
| [`image.rgb2lab`](../fn/image.rgb2lab.html) | Per-pixel sRGB→CIE L\*a\*b\* conversion (D65 white point), the whole-`Image` counterpart to the scalar `to_lab`, reusing its exact conversion math | [Images](images.md) |
| [`image.lab2rgb`](../fn/image.lab2rgb.html) | Converts back to 8-bit sRGB, clamped into gamut | [Images](images.md) |
| [`svg.rect`](../fn/svg.rect.html) | An axis-aligned rectangle with its top-left corner at (`x`, `y`) | [Images](images.md) |
| [`svg.circle`](../fn/svg.circle.html) | A circle of radius `r` centred on (`cx`, `cy`); `r` must be 0 or more | [Images](images.md) |
| [`svg.line`](../fn/svg.line.html) | A straight segment from (`x1`, `y1`) to (`x2`, `y2`) | [Images](images.md) |
| [`svg.path`](../fn/svg.path.html) | An arbitrary path from the SVG path-data string `d` | [Images](images.md) |
| [`svg.text`](../fn/svg.text.html) | A text run anchored at (`x`, `y`) — which in SVG is the *baseline start*, not the top-left corner | [Images](images.md) |

## Not yet described

These names are in the engine and answer to a call, but no chapter
table describes them yet. The list is computed by comparing the
interpreter's table against the chapters, so it is the documentation
backlog rather than a statement about the language.

[`accessed_at`](../fn/accessed_at.html) [`accuracy`](../fn/accuracy.html)
[`apply_edits`](../fn/apply_edits.html) [`base64_decode`](../fn/base64_decode.html)
[`base64_encode`](../fn/base64_encode.html) [`bootstrap_ci`](../fn/bootstrap_ci.html)
[`brier`](../fn/brier.html) [`bytes_read`](../fn/bytes_read.html) [`bytes_write`](../fn/bytes_write.html)
[`casefold`](../fn/casefold.html) [`circuit_impedance`](../fn/circuit_impedance.html)
[`clear_bit`](../fn/clear_bit.html) [`codepoints`](../fn/codepoints.html)
[`conformal`](../fn/conformal.html) [`coverage`](../fn/coverage.html)
[`crc32`](../fn/crc32.html) [`created_at`](../fn/created_at.html) [`dedent`](../fn/dedent.html)
[`duration`](../fn/duration.html) [`entropy`](../fn/entropy.html) [`exit`](../fn/exit.html)
[`file_info`](../fn/file_info.html) [`find_hex`](../fn/find_hex.html)
[`get_bit`](../fn/get_bit.html) [`hex_decode`](../fn/hex_decode.html)
[`hex_encode`](../fn/hex_encode.html) [`hexdump`](../fn/hexdump.html)
[`image`](../fn/image.html) [`image_regions`](../fn/image_regions.html)
[`indent`](../fn/indent.html) [`invert`](../fn/invert.html) [`is_readonly`](../fn/is_readonly.html)
[`json_delete`](../fn/json_delete.html) [`json_get`](../fn/json_get.html)
[`json_set`](../fn/json_set.html) [`kfold`](../fn/kfold.html) [`kill`](../fn/kill.html)
[`levenshtein`](../fn/levenshtein.html) [`line_at`](../fn/line_at.html)
[`line_col`](../fn/line_col.html) [`line_count`](../fn/line_count.html)
[`line_delete`](../fn/line_delete.html) [`line_insert`](../fn/line_insert.html)
[`line_range`](../fn/line_range.html) [`line_set`](../fn/line_set.html)
[`modified_at`](../fn/modified_at.html) [`nbytes`](../fn/nbytes.html)
[`pack`](../fn/pack.html) [`pad_bytes`](../fn/pad_bytes.html) [`path_absolute`](../fn/path_absolute.html)
[`path_extension`](../fn/path_extension.html) [`path_join`](../fn/path_join.html)
[`path_name`](../fn/path_name.html) [`path_normalize`](../fn/path_normalize.html)
[`path_parent`](../fn/path_parent.html) [`path_relative_to`](../fn/path_relative_to.html)
[`path_stem`](../fn/path_stem.html) [`pos_of`](../fn/pos_of.html) [`preciseTimer`](../fn/preciseTimer.html)
[`process_is_running`](../fn/process_is_running.html) [`process_kill`](../fn/process_kill.html)
[`process_pid`](../fn/process_pid.html) [`process_poll`](../fn/process_poll.html)
[`process_read`](../fn/process_read.html) [`process_read_stderr`](../fn/process_read_stderr.html)
[`process_spawn`](../fn/process_spawn.html) [`process_wait`](../fn/process_wait.html)
[`r2`](../fn/r2.html) [`replace_bytes`](../fn/replace_bytes.html) [`residual_acf`](../fn/residual_acf.html)
[`reverse_bytes`](../fn/reverse_bytes.html) [`roc_auc`](../fn/roc_auc.html)
[`sample_to_time`](../fn/sample_to_time.html) [`set_bit`](../fn/set_bit.html)
[`setenv`](../fn/setenv.html) [`sha256`](../fn/sha256.html) [`similar`](../fn/similar.html)
[`slice_at`](../fn/slice_at.html) [`splice`](../fn/splice.html) [`split_time`](../fn/split_time.html)
[`strip_ansi`](../fn/strip_ansi.html) [`swap_endian`](../fn/swap_endian.html)
[`time_to_sample`](../fn/time_to_sample.html) [`timer`](../fn/timer.html)
[`toggle_bit`](../fn/toggle_bit.html) [`unpack`](../fn/unpack.html) [`unsetenv`](../fn/unsetenv.html)
[`word_wrap`](../fn/word_wrap.html)

