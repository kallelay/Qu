# Changelog

Notable changes to Qu. Dates are when the work landed, not when it was
released.

The format follows [Keep a Changelog](https://keepachangelog.com/), and
versions follow [semantic versioning](https://semver.org/) once 1.0 is
reached; before that, minor versions may break things, and breaking
changes are called out.

## [Unreleased]

## [0.3.0] - 2026-09-18

Version bump only -- no new content over v0.2.4/v0.2.5 below. Folds
the Qu Studio installer fix (a corrupted `esbuild` lockfile entry that
failed every platform's build) in under one version number, ahead of
the first public release.

## [0.2.4] - 2026-09-18

Dependabot security fixes (26 of 32 alerts, all criticals), the
linux-x86_64-static (musl) release build fixed for real (vendored
OpenSSL, then a second musl-only C++ toolchain gap in `tokenizers`),
Node.js 20->22 in CI (20 is past end-of-life), a Windows-runner npm
install fix, and the macos-x86_64 leg dropped from the release matrix
(the hosted runner never got scheduled, not a build problem). Also
covers everything that had accumulated under `[Unreleased]` through
v0.2.2/v0.2.3, retitled here since those tags shipped without a
changelog update.

### Breaking

- **Assignment inside a function now binds locally.** It used to bind to
  the module-level variable of that name whenever one existed, so a helper
  using ordinary names for its own bookkeeping silently overwrote the
  caller's. Reads still fall through to the enclosing scope; write
  `global name` to write through. Measured across 157 scripts: one needed
  a one-line change.

### Added

- `global a, b` — declares that a function writes to module scope.
- **Complex matrix linear algebra**: `det`, `norm`, `cond`, `rank`,
  `svd`, `lu`, `qr`, `chol`, `eig`, `pinv`, `solve` on complex matrices,
  plus complex index-assignment and complex `hstack`/`vstack`. `eig` is
  Hermitian-only and says so.
- **Complex elementary functions**: `exp`, `log` and `sqrt` gained complex
  branches. `exp(1i * pi)` previously did not evaluate at all.
- **Compressed sensing**, in full: `cs_recover(A, y, method=)` with six
  reconstruction methods — OMP, CoSaMP, subspace pursuit and NIHT (greedy),
  FISTA/ISTA and basis pursuit by ADMM (convex) — plus `coherence`,
  `cs_guarantee` and `tv_denoise`. `debias=true` re-fits the found support
  by least squares, which removes the shrinkage the l1 penalty leaves
  behind. A chapter covering the theory and a catalogue program running all
  six on the same data.
- **`and` and `or` now short-circuit.** The right side is not evaluated
  when the left already decides the answer — which is the meaning of the
  words, and without it the guard idiom every language shares
  (`if i < len(xs) and xs[i] > 0`) errors on exactly the input it exists to
  protect against.
- **`adc(x, bits=, vmin=, vmax=, dither=)`** — quantisation the way an
  instrument does it: a reference range and a resolution, with clipping,
  the LSB, and the `6.02·bits + 1.76` dB ideal to measure against.
- **A noise module**, 1-D and 2-D, in both directions. Noise is specified
  in decibels — `add_noise(x, "gaussian", snr = 3)` — because an amplitude
  is not a statement about a measurement and an SNR is. Nine kinds
  (gaussian, uniform, pink, brown, blue, shot, salt-and-pepper, impulse,
  quantisation), all honouring that number. `hum` with odd harmonics, `emf`
  as carrier plus switching bursts, `distort` in five flavours. Out again:
  `medfilt`/`medfilt2`, `smooth`, `savgol`, `hampel`, `detrend`, and
  `measure_snr` to close the loop.
- **Shapes**: `circle`, `ellipse`, `arc`, `polygon`, `rect`, and `axis off`.
  Drawn in data coordinates, so a circle is the correct ellipse when the
  axis scales differ.
- **Layers**: `capture(fn)` keeps what a function drew, as a value;
  `stamp(layer, x, y, scale=, rotate=)` places a transformed copy. Describe
  a symbol once, place it forty times.
- **Animation**: `animate(fn, frames, path, fps=)` writes one animated SVG,
  no JavaScript. `qu_starry_night.qu` and `qu_sea_waves.qu`.
- **Interactive export**: `explore(fn, path, names=, from=, to=, steps=)`
  writes a self-contained HTML page with a slider per parameter.
- **Colour spaces**: `hsv`, `hsl`, `lab`, `cmyk`, their `to_*` inverses,
  `delta_e` (how different two colours *look*) and `palette(n)`.
- **`load_image` reads PNG**, not only BMP, with the format sniffed from
  the file's bytes. Written on `inflate.rs`'s existing DEFLATE decoder.
- `filter_ba(b, a, x)` / `lfilter` — apply a transfer function from raw
  coefficients, for one you derived yourself or ported from MATLAB or
  scipy.
- **Colours by name.** All 148 CSS names, `#rgb`/`#rrggbb`/`#rrggbbaa`, and
  `rgb(r, g, b)` / `rgba(r, g, b, a)`.
- **Regular expressions**: `regex_match`, `regex_find`, `regex_find_all`,
  `regex_groups`, `regex_replace`, `regex_split`, `regex_count`, and `like`
  for BASIC wildcards.
- **The rest of the string surface**: `after`/`before` (and `_last`),
  `head`/`tail` beyond tables, `grep`, `parse_as`/`scan`, `format`,
  `as_text`, `flip`, `insert`, `remove`, `count`, `last_index_of`,
  `pad_left`/`pad_right`, `capitalize`, `proper`, `lines`, `chars`,
  `compare`, `write_text`/`append_text`, `"ab" * 3`, and the
  `ucase`/`lcase`/`toupper`/`tolower` aliases.
- **`md2html` / `html2md`**, both directions.
- `round(x, digits)`, `log(x, base)`, `replace(s, old, new, count)`,
  `split(s, d, limit)`, `trim(s, chars)`, `sort` on a list of strings.
- `nnls(A, b)` — non-negative least squares (Lawson–Hanson).
- `least_squares(f, x0, lower=, upper=)` — nonlinear least squares on a
  residual function, with box bounds enforced at evaluation time.
- `solve`, `cond`, `trace`, `diag`, `complex(re, im)`.
- **Binary readers**: `read_array`, `read_values`, `read_struct`,
  `read_structs`, `file_size` — enough to read instrument captures
  directly.
- `dash=` / `dot=` on any series-drawing call, matching `vline`/`hline`.
- `alpha=` and `label=` on `fill_between`; `alpha=` on `xspan`/`yspan`;
  `alpha=` on `hist`; `marker=` and `label=` on `errorbar`; `dash=` and
  `width=` on `contour`.
- Twin axes gained independent limits and scales: `ylim`/`yscale` after
  `yyaxis right` now apply to the right side.
- Argument lists may span multiple lines.
- `llms.txt` and an agent guide for coding assistants.

### Changed

- **Keyword arguments a builtin does not read are now an error.** Tracked
  from the code itself rather than a table, so it cannot drift. Found five
  silently-ignored keywords across the repository the day it was switched
  on.
- **Positional arguments past the last one a builtin reads are now an
  error** too. `sqrt(4, 9)` returned 2; `upper("a", "b", "c")` returned
  "A"; `split(s, ",", 2)` and `trim(s, "x")` looked like they took the
  argument they were given and did not. Same read-tracking mechanism as the
  keywords, and variadic builtins exempt themselves: `print` and `plot`
  cannot work without counting or walking their arguments, and both mark
  the whole list read.
- **Maths variables in figure labels are italic**, following TeX: a Latin
  letter standing for a quantity is italic, digits and operators and
  anything inside `\mathrm` are upright.
- `zeros(r, c)` and `ones(r, c)` honour an explicit shape containing a 1.
  `zeros(n, 1)` is a column and `zeros(1, n)` is a row; they used to be
  the same vector. `zeros(size(x))` is unchanged.

### Fixed

- **A named colour was blue on screen and black in the paper.** Colour
  strings reached the renderers untouched, and the renderers disagree: an
  SVG viewer resolves `royalblue`, while `hex_to_rgb` reads `"ro"` as hex,
  fails, and falls back to zero. Nothing was printed either time.
  `color = "notacolour"` behaved identically and was never refused. Every
  colour now resolves to `#rrggbb` where the argument is read, and one that
  does not resolve is an error naming the nearest real colour.
- **PDF: undeclared math font.** A figure labelled only with lower-case
  Greek referenced a font the page never declared — Latin Modern Roman has
  no lower-case Greek, so no text face was emitted and the math face
  inherited the wrong slot. Every Greek letter vanished.
- **Legends were sized from LaTeX source, not drawn text.** A 56-character
  source that draws as 21 characters produced a box three times too wide,
  wide enough that the layout gave up and moved the legend outside,
  costing half the plot. Every legend containing maths was affected.
- **Dotted rules were invisible.** A dash was a fixed pattern at any
  width while a dot scaled with the line width, so a thin dotted rule
  dotted itself out of existence.
- **Callout text lay across its own arrow.** The label is now placed
  opposite the arrow's direction of travel.
- **A legend's plain-circle swatch ignored `fill=`**, so filled markers
  were keyed with an outline. Every other glyph already honoured it.
- **`stft` reported the wrong quantity** when the window exceeded the
  signal: "fft requires at least 1 sample; received 8". It now names the
  window and the signal length.
- `inv` rejected complex matrices although the implementation existed and
  `M ^ -1` already reached it.
- `\sqrt{...}` and accented groups lost the styling of their contents, so
  `\widehat{\mathrm{CF}}` set italic and `\sqrt{2\ln K}` italicised "ln".

### Documentation

- **[Start Here](book/src/getting-started/start-here.md)** — a tutorial
  that assumes no programming at all.
- **[From Zero](book/src/getting-started/from-zero.md)** — a complete
  first session that runs as written, no external data.
- **Builtin index** generated from the interpreter's own name table, so it
  cannot claim a function the engine lacks. Coverage of the 683 builtins
  went from 87% to 100%.
- `tools/check_docs.sh` runs every ```qu block in the documentation and
  reports how far into each document a reader gets before it breaks. It
  found the `stft` and `complex` defects above.
- README, LICENSE (CC BY-NC-SA 4.0) and NOTICE, none of which existed.

### Verification

Ports checked against their reference implementations, on the same
inputs:

| Port | Result |
|---|---|
| Per-tone EIS extraction | six reported figures reproduce the Python exactly |
| Kramers-Kronig fit with automatic parameters | every reported number exact; 0.014 s against numpy's 0.060 s |
| Known-support recovery | the reference's operational facts reproduced |
| Half-rule conditioning | exact |
| `nnls`, `least_squares` | match SciPy to the digit |
| Complex linear algebra | matches NumPy; `L·U` and `Q·R` reproduce to 1e-12 |
