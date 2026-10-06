# Changelog

Notable changes to Qu. Dates are when the work landed, not when it was
released.

The format follows [Keep a Changelog](https://keepachangelog.com/), and
versions follow [semantic versioning](https://semver.org/) once 1.0 is
reached; before that, minor versions may break things, and breaking
changes are called out.

## [Unreleased]

- Scientific notation and printf: `sci(x, [digits=3], [style=])` writes `1.23 × 10⁻⁴⁵` (or `style="tex"`: `1.23 \times 10^{-45}`), `"{x:.2sci}"` / `"{x:.2tex}"` do the same inside strings, `{x:.3e}` is now C-style (`1.235e+05`, was `1.235e5`) with `E` added, and `sprintf` / `printf` are new (`%d %i %u %f %e %E %g %G %s %c %x %X %o %%`, flags, width, precision, `*`, C length modifiers, MATLAB-style recycling over vector arguments).
- Plots: a figure whose data all lives below 1e-15 (a rate of 1e-45, a capacitance of 1e-20 F) is no longer treated as flat data: it used to collapse to a line on an axis padded to -1..1. The flat-data test is now relative to the data's size; heatmap and bubble-size scaling got the same fix.

- New `diff_lines(a, b, [changed_only=])`: a line diff of two lists of strings (or two strings), returning records `{op, old, new, text}` with 1-based line numbers (it was missing: `diff` is numeric only and `pdf.text_diff` is PDF only).
- `exec`: the documentation now lists `env=` and `timeout=` (both already worked); a timed-out `exec` no longer lets `taskkill`/`kill` print their own output onto the console.

- Performance (measured in `docs/perf-0.4.9.md`, benchmarks in `benchmarks/perf-0.4.9/`): `inv` is now a blocked LU inverse when the matrix is clearly nonsingular (1000x1000: 1.25 s to 0.09 s; singular and borderline matrices still go through the SVD route and report exactly as before); `lu`, `det` and `solve` use a blocked LU on the SIMD matmul kernel (1000x1000: 2-2.4x faster); calling a user function no longer deep-copies the function's AST on every call (1.4x for a one-line function, 2x for a ~15-statement one, `quad` callbacks 4x); `sort`/`unique` of 65536+ values sort in parallel (1e7: 3-5x, results bit-identical); `filter_ba` with long coefficient vectors is about 2x faster (bit-identical); `M[i, j] = x` is about 1.4x faster; `read_npy` of float64 files is about 2.4x faster. Paired before/after numbers in `docs/perf-0.4.9.md`. Not fixed: `dict` is an association list (lookups and `set` are O(n)), sparse LU is ~6x slower than SuperLU, user-function calls are still ~0.5 us.

## [0.4.8] - 2026-10-06

From Ahmed's feedback file and two internal audits: list element assignment, `extend`, `for` over a table, `r"""..."""` raw strings, error locations in Qu Studio and the kernel (`e.file`, `e.trace`), `qu run --dry-run` and `--watch`, a **security fix making `--sandbox` deny file writes**, and hardening of the file-format readers.

- Fixed from the audits: `parse_json` of an array of objects no longer flattens nested arrays or objects to text (it returns a list of records unless every column is plain numbers or plain strings); the `g` number format follows C's significant-digit rules (`{x:.3g}` of 1.2345e-5 is `1.23e-05`); a list-assignment index error uses the same 0-based wording as reads; a syntax error in `qu run` is labelled a parse error and names the file; `--dry-run` names the path it skips, not the document handle.

- Qu Studio's terminal and the Jupyter kernel now show where a runtime error happened: the `qu kernel` run reply carries `at file:LINE in f()` / `called from ...` lines under the message (an optional `file` field in the request names the script), and Studio prints each on its own terminal line.
- **Security fix: `qu run --sandbox` now really denies file-writing calls.** It denied network, processes and a few file calls but let `write_text`, `append_text`, `write_all`, `save*`, `savefig`, `write_report`, `make_file`, `mkdir`, `write_array`, `bytes_write` and the rest through (found by an internal audit). One shared list (`is_file_writer`) now backs both `--sandbox` and `--dry-run`; `--dry-run` also refuses `fopen` in a write mode, since a file handle cannot be faked.
- `qu run --watch` is only a flag before a bare `--`, ignores the moment an editor's atomic save removes the file, and both `--watch` and `--dry-run` are listed in `qu --help`.
- Dependency security updates from Dependabot (dompurify 3.4.16, rustls 0.23.45, vitest 5 with vite 6.4, source-map-js 1.2.2, tokenizers 0.23, uuid 1.27, ...). `qu-ui-components/react` moved to vite ^6.4.3 so vitest 5 installs. The tailwindcss 3 -> 4 and vite 8 bumps are deliberately not applied.
- Hardening of the NumPy/HDF5/zip/PDF readers against memory and CPU amplification from malformed files (caps on decoded strings, B-tree and continuation walks, total decompressed size).

- From Ahmed's feedback file: `xs[i] = v` assigns a list element; `extend(list, other)` concatenates lists (`append` nests); `for row in table` iterates rows as records (so `for o in parse_json(...)` works on a JSON array of objects); `r"""..."""` is a raw string that can contain quotes and braces; `e.file` and `e.trace` in a `catch`; `qu run --watch` re-runs on save; `qu run --dry-run` skips file-writing calls; `--max-time` runs now stream output unbuffered so the last lines survive the kill.

## [0.4.7] - 2026-10-06

New: numerical integration and ODE solvers (`trapz`, `cumtrapz`, `simpson`, `quad`, `ode45`, `ode23`, `ode_stiff`, `rk4`), `import sparse`, NumPy and HDF5 readers, `qu test` and `qu fmt`, runtime errors with file:line and call chain, `pdf.images`/`extract_image`/`to_html`, and the second Office wave (xlsx sheet operations, pptx charts and shapes, docx editing).


- New `qu test [paths] [--filter s] [--fail-fast] [--json]`: runs every top-level zero-argument `test_*` function (a fresh interpreter per file; a file with none runs as a script), prints PASS/FAIL with file:line, error location and time, exits 1 on failure and 2 if nothing was found. New `qu fmt [paths] [--check] [--stdin]`: a conservative lexer-driven formatter (block indentation, trailing whitespace, final newline, spacing after commas and around `== != <= >=` and the compound-assignment operators (`<` and `>` are left alone)); comments and string contents are untouched and the result is idempotent and token-for-token identical.
- `import sparse`: compressed-sparse-row matrices as immutable values (zero-based indices). `from_triplets` (duplicates summed), `from_dense`, `eye`, `diag`, `random(m, n, density, seed=)`, `size`/`nnz`/`density`/`get`/`triplets`/`to_dense`/`transpose`, `add`/`sub`/`scale`/`hadamard`/`mul` (sparse, vector or dense right-hand side) and `solve` with sparse LU (Gilbert-Peierls, partial pivoting, reverse Cuthill-McKee ordering -- not COLAMD), conjugate gradient (optional Jacobi) and BiCGSTAB, the iterative ones returning `x`, `iterations`, `residual`, `converged`, `status`. Sizes are capped (10M rows/cols, 20M entries). `eigs` is not included. *Added in v0.4.7.*
- Scientific binary-format interop. New `read_npy`/`write_npy`/`read_npz`/`write_npz` (NumPy format versions 1-3, either byte order, C or Fortran order, float16/32/64, all int/uint widths, bool, complex64/128; 0-d to 2-d arrays, N-d is an error naming the shape; pickled object arrays are refused and never unpickled; `write_npy` is bit-exact for NaN/inf/-0.0) and a pure-Rust read-only HDF5 reader behind `h5read(path, dataset)` and `h5info(path)` (superblock v0-v3, old-style and link-message groups, contiguous/compact/chunked storage with deflate, shuffle and fletcher32, fixed-point/float/enum/string types). No C library is needed. Anything unsupported -- compound types, other filters, dense link storage, external links -- fails with an error that names it, and every offset, length and allocation read from these untrusted files is bounds-checked and capped. `read_mat` now also reads MATLAB v7.3 files (numeric/logical arrays, char rows, scalar structs) through the HDF5 reader instead of refusing them. Verified against hand-built fixtures written from the format specifications, not yet against files produced by NumPy/h5py/libhdf5.
- Numerical integration and ODEs, new flat builtins (documented in *Signal Processing & Filters*, "Numerical Integration & ODEs"): `trapz`, `cumtrapz`, `simpson` on sampled data (matrix columns integrate along dimension 1); `quad` / `quad_info` (adaptive Gauss-Kronrod G7K15, infinite limits, endpoint singularities; `quad` errors rather than return an unconverged value, `quad_info` returns `{value, error, nfev, intervals, status}`); `ode45` (Dormand-Prince with dense output for `t_eval=`), `ode23` (Bogacki-Shampine), `ode_stiff` (Rosenbrock 2(3) with a numerical Jacobian) and `rk4`, each returning a record `{t, y, steps, nfev, status}` with `y` one row per time point; `events=`/`terminal=` zero-crossing detection on the adaptive solvers. Limits: `ode_stiff` has no `jac=`, events detect one crossing per step and have no `direction=`, `rk4` has no events.

- Runtime errors now say where: `qu run` prints `at file.qu:LINE in function()` and each `called from` line up to the top level under the error message (the `try`/`catch` exception record is unchanged). Line numbers inside an imported module are that module's, shown with the main script's name.
- Hardening from an independent review: `pdf.extract_image` caps an Indexed palette at 255 entries, validates `/BitsPerComponent` and no longer reserves memory from an unchecked `/Width x /Height`; `pdf.to_html` escapes `"` in image names; `docx.split_cell(cols=)` on a table with fewer grid columns than its rows use is an error instead of a panic.

- Friendlier errors and `get` default (from Ahmed's feedback file): using `pdf.x(..)` without `import pdf` says to import it; a reserved word used as a name says it is reserved and lists the keywords; appending a string to a number vector (`o = []`) points at `lines("")`; `get(d, key, default)` now returns `default` for an absent key instead of ignoring the argument.
- `import pdf` reads more of a PDF. `pdf.extract_text` gains `normalize=true` (ligatures expanded, whitespace collapsed, line-end hyphenation re-joined) and `lines=true` (a `List` of lines rebuilt from glyph positions instead of one `Str`; errors rather than guessing when a page's lines cannot be recovered). New `pdf.images` (lists embedded images), `pdf.extract_image` (JPEG and raw/Flate images to an `Image`; JBIG2, CCITT and JPX are refused by name) and `pdf.to_html` (page sections with paragraphs, optional inline images). Layout and fonts are not reproduced, and defaults of the existing call are unchanged. `pdf.render` already gives a page as an `Image`.

- `import xlsx` gains `copy_sheet`, `hide_sheet`, `clear`, `move_range`, `sort_range`, `autofilter`, `create_table`, `add_comment` and `add_image` (PNG/JPEG/GIF). Charts saved earlier survive every operation. Formulas pointing into moved cells are not rewritten; `autofilter` only switches filtering on.
- `import pptx` gains `add_chart` (scatter/line/bar/barh), `add_nyquist_chart` (equal axes, square grid), `add_shape`, `rotate_shape` and `align_shapes`. Chart data is cached literals, not an embedded workbook; use `xlsx.add_chart` for an editable chart. `delete_shape` now removes a chart part when its frame was the last reference, and `duplicate_slide` gives the copy its own chart part.
- `import docx` edits what a document already says: tables (`add_row`, `add_column`, `merge_cells`, `split_cell`), character formatting of a matched span of text (`format_text` -- runs are cut at the span's edges, inside links and tracked insertions too), `line_spacing`, `move_paragraph`, footnote creation (`add_footnote`, which builds the footnotes part and note styles when the document has none) and tracked changes (`track_insert`, `track_delete`, written as `w:ins`/`w:del` with author and date; `accept_changes` now also removes a paragraph whose mark was tracked-deleted). Fixed on the way: cutting a run in two (`add_link`/`add_comment` on part of a run) gave its right half a second `w:rPr`, which Word rejects.

## [0.4.6] - 2026-10-01

**Changed: `rgb()` channels are 0..255 unless the call says otherwise.**
0.4.5 guessed 0..1 fractions when every channel was in 0..1 and one was
not whole; that guess is gone. `rgb(0.5, 0.75, 1.0)` is bytes again
(#010101); write `rgb(0.5, 0.75, 1.0, scale=1)` -- or `scale="normalized"`
-- for fractions. A bare vector `color = [r, g, b]` is 0..255 too.

Installers: both Windows setups (command line and Qu Studio) have a
Components page -- **Jupyter kernel** (on: `qu-jupyter.exe`, registered as
the "Qu" kernel for JupyterLab/Notebook/VS Code, unregistered on
uninstall), **offline documentation** (off: the site in `docs\`, which
`help(name)` then points at), **editor plugins** (VS Code, Sublime Text,
Notepad++, each preselected only when that editor is installed; now
removable on uninstall), and shortcuts: **Qu CLI (REPL)**, **Start
Jupyter (Qu)** (says how to get Jupyter if it is missing) and **Qu
Documentation** in the Start menu, optionally on the desktop. Silent
switches `/WITHDOCS /NOJUPYTER /NOPLUGINS /NOSHORTCUTS /DESKTOP`. Adding
`qu` to the PATH is a component too (on; untick it, or `/NOPATH`). The
release archives now carry `qu-jupyter`, `docs/` and `editors/` too.

Fixed: the Qu Studio installer never actually put `qu` on the PATH (its
in-process registry edit silently did nothing, 0.4.4 and 0.4.5 included).
It now uses the same `path-helper.ps1` as the command-line installer.

Fixed in Qu Studio: the **Interactive** tab opened to a black window (0.4.5):
`react-plotly.js` is CommonJS, and under Vite 8 its default import was the
module object, which crashed React. The CI smoke test now opens every
top-level tab. The editor coloured everything after a transpose (`X_b' *
errors`) as a string, to the end of the file: a `'` right after a name,
number or closing bracket is now highlighted as the operator it is.

Qu Studio's terminal has a **`qu>` prompt**: after Run, keep going in the
same session -- the script's variables are there (Enter runs, Shift+Enter
adds a line, Up/Down recall). A **REPL** setting in the terminal header
chooses one **shared** session for the whole window (as before) or one
session **per file**; in per-file mode the Variables and Figures panels
follow the active file, and closing a file ends its session. The Variables
panel hides the built-in constants (`pi`, `e`, `inf`, `QuCr`, `QuTab`, ...)
behind a toggle unless reassigned (`--emit-vars` and the kernel mark them
`"system": true`); plotting a variable from it no longer blanks the window
(the same `react-plotly.js` import as the Interactive tab). Opening a figure
zooms it out of its thumbnail.
`qu-jupyter install --system` and `qu-jupyter uninstall` are new.

## [0.4.5] - 2026-10-01

**Fixed: Qu Studio 0.4.4 opened to a blank dark window.** Two copies of
React ended up in the bundle (the shared UI components resolved their own
React after the Vite 8 upgrade), which crashes React as the app mounts.
Studio's build now dedupes React, and CI and the release workflow render
the built frontend in a headless browser and fail unless the app mounts.
The status bar shows the real version instead of a fixed "v0.1.0".

The command line: `qu script.qu [args]` runs the script (as `qu run`);
options may come before the command (`qu --live repl x.qu`, `qu --sandbox
x.qu`); `qu repl x.qu` streams the script's output -- and every line typed
at the prompt -- as it is printed instead of after it finishes. `help(name)`
links to the published reference and the catalog on GitHub instead of
repository paths an installed `qu` does not have.

Colours: `rgb(0.5, 0.75, 1.0)` reads MATLAB-style fractions (it rounded to
near-black #010101), `scale=1`/`scale=255` says which outright, and a bare
vector `color = [0.5, 0.75, 1.0]` is a colour -- it reached the SVG verbatim
and drew black. A non-colour value for `color=` is an error that says what
is accepted.

**Changed: `remove_file`/`remove_dir` send to the Recycle Bin/Trash by
default.** A deleted file can be recovered unless the call says
`permanent=true` (the old `recycle_bin=false` still means permanent;
contradicting the two is an error). Where no trash is available, the call
fails naming `permanent=true` instead of deleting for good.

File history: `file_versioning(true)` (or `QU_FILE_HISTORY=1`) keeps the
current content of every file a builtin is about to overwrite --
`write_text`, `write_csv`, `save`, `savefig`, `fopen("w"/"a")`, Office
`save_as`, overwriting copies/moves, permanent deletes and more -- in a
`.qu-versions/` folder beside it, the same store Qu Studio's editor
already keeps, so either sees the other's versions. `file_history(path)`
lists them, `read_version(path, v)` reads one, `restore_version(path, v)`
puts one back (undoably). 50 versions per file; files over 100 MB are not
copied.

`text(x, y, s, rotate=, align=)`: a label tilted (degrees, anticlockwise)
and anchored exactly at its point by the given edge -- gene names at 45
degrees over their arrows, labels at computed positions. `d[key]` reads a
dict (a missing key is an error listing the keys; `get` stays the
forgiving form). Named y ticks (`yticklabels`) widen the left margin
instead of running off the canvas. New catalog example
`qu_genome_synteny.qu`: a gggenomes-style synteny map (gene arrows,
homology ribbons with inversions, a transposon with its GC content) in
about 100 lines of plain Qu.

- **Windows installer for the `qu` command line.** Releases now carry `qu-<version>-windows-x86_64-setup.exe` and `-arm64-setup.exe` next to the zips: a per-user install (no admin prompt) to `%LOCALAPPDATA%\Programs\Qu` that adds `qu` to the user `PATH` without rewriting the rest of it (long PATHs, `%VARS%` and `REG_EXPAND_SZ` kept as they were, no duplicate entry), with `/S`, `/D=<dir>` and a machine-wide `/ALLUSERS`; the uninstaller removes only its own files and `PATH` entry. End-to-end checks live in the manually dispatched `installer-verify-cli.yml` (it edits the registry, so it runs only on a throwaway CI runner).

`import pptx` edits inside slides: speaker notes can now be written
(`set_notes`, creating the notes page -- and a notes master if the deck has
none -- for a slide that has no notes), shapes can be listed with their
kind, position and text (`shapes`) and edited (`set_shape_text`,
`set_slide_title`, `move_shape`, `resize_shape`, `delete_shape`,
`bring_to_front`, `send_to_back`), text runs can carry hyperlinks
(`set_link`, `links`), and the theme's colour and font schemes can be read
and set (`theme_colors`, `theme_fonts`, `set_theme_colors`,
`set_theme_fonts`). Each edit rewrites only the parts it touches.

- `import docx` edits document structure: hyperlinks (`add_link`, `links`), comment creation (`add_comment`), bookmarks and cross-references (`add_bookmark`, `bookmarks`, `add_cross_ref`), fields (`add_field`, `fields`, `add_toc` -- a dirty TOC field Word builds on open; no invented entries), headers/footers per section with `#page`/`#pages` fields (`set_header`, `set_footer`, `header_text`, `footer_text`), page setup and sections (`sections`, `page_setup`, `add_section_break`), and bulleted/numbered lists (`add_list_item`, `set_list`) -- each creating the styles/settings/numbering/comments/header part when the document has none, and leaving every other part byte for byte.

- **xlsx: charts, conditional formatting, data validation.** `xlsx.add_chart` (scatter/line/bar/barh from cell ranges, titles, axis titles and limits, log axes, placement and size), `xlsx.add_nyquist_chart` (Z' vs -Z'' with equal axis scaling for impedance spectroscopy), `xlsx.conditional_format` (comparisons, between, text contains), `xlsx.color_scale`, `xlsx.add_validation` (lists from values or cells, whole/decimal ranges, custom formulas, prompt and error messages). Written into the saved package part by part (the DrawingML chart writer lives in `qu-ooxml` for reuse); a workbook whose cells were not edited now keeps every untouched part byte for byte, and one opened and saved with no edit at all is written back unchanged.

- **Qu Studio's Windows setup (`_x64-setup.exe`) now puts `qu` on your PATH.** It adds its install folder, where the bundled `qu.exe` lives, to the current user's `PATH` (never duplicated, long PATHs preserved intact), and the uninstaller removes exactly that entry; it also copies the VS Code / Sublime Text / Notepad++ syntax integrations when that editor's settings folder exists. Previously no Studio package touched PATH -- the script meant to do it was written for Tauri 2 hooks and never ran. The Studio `.msi`, `.dmg`, `.AppImage`, `.deb` and `.rpm` still do not put `qu` on PATH. The download page no longer lists a macOS Intel command-line build, which the release matrix stopped producing on 2026-09-18.

## [0.4.4] - 2026-09-29

Every family on the distribution-relationships chart, under MATLAB's names:
continuous and discrete uniform, Bernoulli, binomial, geometric, negative
binomial, Poisson, hypergeometric, normal, log-normal, exponential,
Weibull, gamma, beta, chi-square, Student t and F, each with `*pdf`
(mass for the discrete ones), `*cdf`, `*inv`, `*rnd` and `*stat`
(`.mean`/`.var`), and maximum-likelihood `*fit` for twelve of them
(`.params`, `.loglik`; `normfit`, `lognfit`, `expfit`, `gamfit`,
`wblfit`, `betafit`, `unifit`, `poissfit`, `binofit`, `geofit`,
`nbinfit`, `bernfit`) -- 76 new builtins. Discrete masses use Loader's
saddle-point algorithm (as R/SciPy), full precision where a difference of
`lgamma`s loses 12 digits; discrete samplers invert the CDF exactly
(table-driven for wide distributions: a million binomial(1e6, 0.3) draws
in 33 ms). Checked against SciPy and mpmath: worst error a few ulp.

Parameter uncertainty on every least-squares fit (`curve_fit`,
`least_squares`, `circuit_fit`/`sysid`): `stderr`, `cov`, `correlation`,
`t`, `p_values` (Student t on `dof = n - p`), `ci` at `level=`, `rmse`,
`sigma`, and a `note` for zero degrees of freedom or unidentifiable
parameters. `bootstrap=N` (with `seed=`) adds residual-resampling
`bootstrap_stderr`/`bootstrap_ci`. `method=` selects `"lm"` (default),
`"gauss_newton"` or `"gradient_descent"`. `summary(fit)` prints the
parameter table. `least_squares` flags parameters pinned on a bound in
`at_bound`.

Accuracy and performance audit (docs/perf-audit-2026-09-28.md):
`sum`, `mean`, `dot`, `cumsum` and tensor sum/mean use compensated
(Neumaier) summation; `mean` no longer overflows on large finite values;
`norm` no longer overflows/underflows; `var`/`std` use a corrected
two-pass on the compensated mean. `median`/`quantile` use selection
instead of a full sort (5-11x faster, identical results), which also
speeds up `table_describe`. `s = s + "..."` string building appends in
place when the terms provably run no user code (linear instead of
quadratic). Leak probes (streamed printing, Office handles, repeated
fits, general values) show flat memory.

`sleep` takes a duration in any time unit -- `sleep(2 s)`, `sleep(250
ms)`, `sleep(10 us)` -- and a bare number stays milliseconds, as it
always was in the engine (the reference had wrongly documented seconds;
the one book example that followed it, `sleep(0.3)`, now reads `sleep(300
ms)`). A non-time unit is an error, and sub-millisecond durations are no
longer truncated to whole milliseconds.

Release notes on GitHub now carry the changelog since the previous
release instead of the whole file.

Boolean masks read as 0/1 in numeric reductions and arithmetic, as in
MATLAB and NumPy: `sum(x <= c)` counts, `mean(x <= c)` is the fraction
(an empirical CDF), `cumsum`/`prod` accept masks, `(x > 0) * 2` is
numeric, and `any(mask)`/`all(mask)` take a bare mask. Indexing with a
mask is unchanged.

Dependencies: ureq 3, zip 8, calamine 0.36, rust_xlsxwriter 0.99, wgpu
30, nalgebra 0.35, thiserror 2, libloading 0.9, hmac 0.13 / sha2 0.11,
tokenizers 0.22, hdf5-metno 0.15 (`http_get`, `qu-gpu` and Qu Studio's
hosted-LLM client ported). `hf-hub` is gone: 1.0 is async-only, so
`llm_load` now downloads model files itself (streamed through a `.part`
file, so an interrupted download is never mistaken for a cached one),
reads models an earlier version already cached,
and honours `HF_HOME`/`HF_HUB_CACHE`/`HF_TOKEN`; the musl OpenSSL
workaround it needed is gone with it. Frontends: vite 8 (the npm audit
is clean in both packages), fast-uri 3.1.8.

## [0.4.3] - 2026-09-28

Office toolkit (docs/design/toolkit-office.md): `import docx`, `import
pptx`, and in-place workbook editing in `import xlsx`. Documents are
handles (`open`/`new` ... `save_as`), and editing is surgical -- only the
parts an operation touches are rewritten, everything else in the package
(themes, fonts, embedded objects, macros, custom XML) is kept byte for
byte. Word: text, paragraphs, headings, find/replace across runs and
every story part, headings/paragraphs/tables/images/page breaks, cells,
comments, footnotes, tracked-change accept/reject, `to_markdown`,
`to_latex` (figures extracted). PowerPoint: slide text/titles/notes,
replace, add slides from layouts, delete/move/duplicate/hide, text boxes,
images and tables at mm positions. Excel: cells, formulas with
fill-down reference shifting, ranges, sheets, rows/columns, formats,
merge, freeze panes, defined names. `to_pdf` for all three through
LibreOffice when installed. New crates `qu-ooxml` (shared package layer),
`qu-docx`, `qu-pptx`; `qu-xlsx` gains `umya-spreadsheet`. Function names
avoid every builtin, so these imports never make a builtin ambiguous.

`pdf.render(src, page, dpi=)`: a PDF page to an Image through PDFium,
loaded at run time (no native build step); `QU_PDFIUM` or a library file
beside `qu`.

Statistics and models: `normcdf`/`norminv`, `tcdf`/`tinv`/`tpdf`,
`chi2inv`, `fcdf`/`finv`/`fpdf`, `gamcdf`/`gaminv`/`gampdf`,
`betacdf`/`betainv`/`betapdf`, `expcdf`/`expinv`/`exppdf` (MATLAB names
and parameterisations, broadcasting); `lda_model`, `qda_model`,
`pls_model`, `ica_model`/`ica` on the `predict`/`transform` protocol.

`load_library`/`native_call`: call C functions in a `.dll`/`.so`/`.dylib`
with a stated signature (scalars of one type, or double-array kernels);
refused under `--sandbox`.

Output: `qu run` and `qu eval` now stream output as it is printed
(line-flushed on a terminal, buffered into files and pipes) instead of
printing everything when the script ends -- a watcher loop shows each
round, and output before an error is no longer lost; `flush()` and
`sleep`/`read_input` push pending output out; `--report` keeps the old
behaviour and `--live` still forces per-line flushing into pipes.
`replace`/`regex_replace` gain `keep=` (leave matches inside the kept text
alone -- the stand-in for lookahead). `qu eval` joins all its arguments
and strips the single quotes cmd.exe leaves in place.

`regionprops`/`blob_stats`, `image_regions` and `image.regions` now share
one measurement implementation; names and result shapes are unchanged.

**Behaviour changes:** `image_regions(..., unit="px", pixel_size=...)` is
now refused (it labelled scaled numbers as pixels), and `unit="px"`
without `pixel_size=` is accepted. `qu-image` is always linked; the
`image` feature still gates the `image.*` module.

**Known open:** `sleep` takes milliseconds while its documentation says
seconds -- unchanged pending a decision on which is right.

## [0.4.0] - 2026-09-24

ML toolkit: function-level autodiff transforms (`jacobian`, `hessian`,
`value_and_grad`, `vmap`, `check_grads`) on top of the existing
reverse-mode tape; `circuit_fit`/`sysid` wiring the existing EIS circuit
element set into the fit/predict/score protocol, with AIC-based topology
selection over a 7-candidate ladder; four new estimators on the
`ModelHandle` protocol (`isolation_forest`, `gaussian_process`, `nmf`,
`arima`). `hdbscan`/`umap` deliberately deferred -- both are hard to get
right from scratch and a shape-correct-but-subtly-wrong version is worse
than not shipping. A real `nmf.predict` convergence bug (re-encoding a
fitted row returned different coefficients than the fit itself assigned
it) was caught by known-answer tests and fixed to run to convergence.

Shell/process toolkit: `setenv`/`unsetenv`, `env=` on `exec`/`shell`,
`kill(pid)` and `timeout=` on `exec` (both tree-killing via `taskkill
/T` on Windows so a `cmd /C`-wrapped child's own children don't survive
past the deadline), and a VB.NET-`Process`-style async handle
(`process_spawn`/`process_poll`/`process_read`/`process_wait`/
`process_kill`/etc.) for spawning without blocking and polling
incrementally.

Six more toolkit branches landed: PID/sliding-mode/fuzzy-PID
controllers; advanced time-frequency (signal-timefreq-advanced); JPEG/
TIFF image codecs; SVG basic I/O (`save_svg`/`load_svg`,
`svg.rect`/`svg.circle`/`svg.line`/`svg.path`/`svg.text`); a PDF toolkit
(`import pdf` -- `page_count`/`info`/`merge`/`extract_pages`/
`extract_text`, on `lopdf`).

Fixed a pre-existing, previously-unexplained test-infrastructure bug:
several `qu-interp` tests (the NN/autodiff-heavy ones) overflow the
default debug-build stack on Windows and abort the whole test binary,
silently truncating every test scheduled after them. `engine/.cargo/
config.toml` already set `RUST_MIN_STACK` for this, but Cargo only
discovers `.cargo/config.toml` walking up from the working directory,
never down into subdirectories -- invisible to `cargo test --manifest-
path engine/Cargo.toml` run from the repo root. A matching root-level
`.cargo/config.toml` closes that gap.

Auto-laid-out diagram rendering, the box/arrow "smart art" layout layer
noted as missing in BACKLOG.md (2026-09-10): `diagram_pipeline(fn)` draws
a `|>` pipe chain as a left-to-right chain of boxes; `algorigram(name)`
draws a function's control flow (`if`/`else`, loops, `select case`,
`try`/`catch`) as a top-to-bottom flowchart. Both return SVG and accept
`file=` to write `.svg`/`.html` directly (`.png` is refused, same
whole-figure-rasterizer scoping as `savefig`).

The rest of the signal toolkit deferred out of v0.3.0 (§8-§11 of
`docs/design/toolkit-signal.md`), now shipped: UART/SPI/I2C/CAN
decoders (`decode_uart`/`decode_spi`/`decode_i2c`/`decode_can`); a
streaming block/state processor (`block_process`, `blocks`,
`processor`/`process`); WAV file I/O (`codec.write_wav`/`encode_wav`,
joining the existing `codec.decode_wav`); transfer functions and
impedance (`transfer_function`, `impedance(voltage, current, ...)`,
`bode`); calibration, units, markers and metadata
(`calibrate`/`apply_gain`/`apply_offset`/`convert_unit`,
`metadata()`/`set_metadata`, `add_marker`/`add_region`) -- `Value::Signal`
now carries this as a third field, empty and free for every signal that
never sets one; an LMS adaptive FIR filter (`lms_init`); multirate
resampling (`upsample`/`downsample`/`resample_int`), plus a **behavior
change**: `resample_to()` now anti-alias filters before decimating when
lowering the rate, instead of aliasing (it was pure linear interpolation
between grids before); digital-comms coding (`qam_modulate`/
`qam_demodulate`, `hamming74_encode`/`hamming74_decode`, `crc`/
`crc_check`); delay-and-sum beamforming (`beamform`, `steer_delays`).
Also 3 P0 correctness fixes from a defect review: `delay()` no longer
overflows on an extreme shift, `gain()` rejects a NaN `db` instead of
silently producing NaN samples, and `block_process()` tags its output
Signal-vs-Vec by genuine per-block agreement rather than a coincidental
aggregate-length match.

## [0.3.0] - 2026-09-18

Signal toolkit expansion, scoped to what actually shipped (see
`docs/design/toolkit-signal.md`'s "v0.3.0 status" note for the full
shipped-vs-deferred breakdown): diagnostics and integrity checks
(`is_clipped`/`find_clipping`/`detect_saturation`/`verify_signal`),
rolling statistics, pulse/edge metrics, missing-sample and outlier
handling, `wrap()`/`remove_noise()`, and a `mem_usage()` builtin plus a
`profile_start()`/`profile_end()` region profiler for comparing methods
on memory as well as speed. `qu repl <file.qu>` (run a script, keep its
session alive) and a real Jupyter kernel (`qu-jupyter`, notebooks in VS
Code/JupyterLab with persistent state across cells) round out the
run/REPL story. Qu Studio's Run button is now backed by a persistent
kernel session instead of a one-shot subprocess per click. Protocol
decoders, audio file I/O, calibration, and transfer-function/impedance
measurement (§8/§9/§10/§11 of the signal-toolkit spec) remain unstarted
and are deferred to the next version.

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
