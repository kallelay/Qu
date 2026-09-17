# Figure reference collection

A fixed set of Qu figures that stress the parts of the plotting engine where
quality is decided, kept as **source + rendered SVG** so a change to
`engine/crates/qu-interp/src/plotting.rs` can be judged by looking at a
before/after rather than by argument.

This is not a test suite — nothing here asserts. It is a **visual reference**:
the eleven cases below were chosen because each one has historically been
where plotting engines fall down, and because between them they cover every
axis type, both themes, subplots, and the two label-collision cases.

Rendered with `qu.exe` at commit `b38f1e8` + the working-tree units fix,
2026-09-05.

The checked-in SVGs predate glyph subsetting (`font_subset.rs`), so the
`*_publication` files here still carry a whole embedded font at ~305-317 KB.
Re-rendering these three today gives 26 KB, 32 KB and 41 KB — the picture is
unchanged, only the embedded font shrinks. They are deliberately NOT
regenerated in that commit: they are also several plotting changes stale, so
a re-render would fold unrelated visual differences into a diff about file
size. Regenerate the whole collection at once, next time it is re-blessed.

## Regenerating

Run from the `svg/` directory so each script's `savefig` lands beside its
siblings:

```bash
cd docs/design/figure-reference/svg
for f in ../sources/*.qu; do qu run "$f"; done
```

```powershell
cd docs/design/figure-reference/svg
Get-ChildItem ../sources/*.qu | ForEach-Object { qu run $_.FullName }
```

There is deliberately no Qu-native runner. Qu has no `import`, no way to run
another `.qu` file, and no process spawn, so a driver script cannot be written
in Qu today — which is itself an argument for the module system tracked in
`BACKLOG.md` under Language Ergonomics.

## The cases

| # | Case | What it stresses | What to look at |
|---|------|------------------|-----------------|
| 01 | `line_screen` | The baseline. One series, screen theme. | Everything else is a delta from this. Grid weight against line weight; label-to-axis distance. |
| 02 | `line_publication` | The same figure under `theme("publication")`. | Diff against 01. Font swap to Latin Modern, ink colour, tick and grid weight. The file is bigger than 01 because the font is embedded — the intended cost of `embed_fonts`, now paid only for the glyphs the figure actually draws. |
| 03 | `eight_series` | Palette capacity. | Are all eight distinguishable? Do any two adjacent series collide in hue or lightness? Would they survive greyscale, and does anything but colour separate them? |
| 04 | `log_x_bode` | Log-scaled x over six decades. | Decade tick placement, minor ticks between decades, whether labels are `10^n` or expanded, and whether the grid reads as logarithmic. |
| 05 | `tiny_magnitudes` | y values around 1e-7. | Does an axis multiplier appear, or does every tick label carry its own exponent? Is the multiplier placed where it can't be missed? |
| 06 | `long_labels` | Long title, long axis titles, 7-digit tick values. | Collision and overflow. Whether long tick values force a multiplier, and whether the title wraps or is clipped. |
| 07 | `subplot_stack` | 3×1 panels, screen theme. | Panel spacing, left-margin alignment across panels, whether per-panel titles crowd the panel above. |
| 08 | `subplot_stack_publication` | The same stack in publication theme. | Diff against 07, and whether the theme scales panel spacing or only fonts. |
| 09 | `legend_six_long` | Six entries with long names. | Placement (Qu auto-places with `best_corner`), whether the box overlaps data, entry spacing, and legend order versus series order. |
| 10 | `dsp_bode_psd` | Two panels with unrelated scales — dB above log-y PSD. | Do two panels with different y-scalings still read as one figure? `semilogx` and `semilogy` in the same figure. |
| 11 | `signal_qc_publication` | A realistic three-panel measurement figure. | The closest case to something that would go in a paper, so judge the publication theme by this one. Includes an explicitly coloured series against the palette defaults. |
| 12 | `theme_matrix` | Every theme name Qu accepts, one figure each. | **Currently a defect record.** Seven names produce three distinct figures — see below. |
| 13 | `markers_and_lines` | The whole mark vocabulary: 36 glyphs, 4 line styles, 6 weights, 6 marker sizes, fill, contrast edge, drop shadow, and the distribution family. | Whether a dash still reads as a dash at every weight, whether a composite glyph's overlay survives being filled, and whether two glyphs that must be told apart actually can be. Added because the set had NO dashed line in it — which is why a bug that painted a solid line over every dash lived here undetected. |
| 14 | `chart_types` | The charts added for measurement work: beeswarm, violin, ecdf, hexbin, polar axes, Smith. | Whether each is honest at the sample size it is shown at, and whether the geometric ones agree with their own frame — a Smith locus for a series RLC must be a constant-resistance circle, and it must land where the ruling says. |

### What case 12 records

As of 2026-09-05, holding everything constant except the theme name:

| `theme(...)` | md5 |
|---|---|
| `default` | `4a7ec336` |
| `bw` | `4a7ec336` |
| `grey` | `4a7ec336` |
| `classic` | `4a7ec336` |
| `no_such_theme` | `4a7ec336` |
| `minimal` | `57c1022e` |
| `publication` | `4a4b2dc4` |

Four named themes and an arbitrary misspelling render **byte-identical** output.
Only `minimal` and `publication` change anything, and those two are driven by
separate flags rather than by the theme table. A misspelt name is accepted in
silence — a typo in a paper build renders the default and says nothing.

Case 12 holds the title constant on purpose. Vary the title per theme and all
seven files differ trivially, which hides the defect completely — that mistake
was made and caught while building this collection.

The bar for "themes work": seven distinct hashes, and the misspelling warns
instead of falling back.

## Known defects visible in this collection

Four found by auditing the SVGs below, each verified directly in the rendered
output on 2026-09-05. They are listed here because the files in `svg/` are the
evidence — re-render after a fix and the diff is the proof.

**1. Every scientific tick label contains a tab instead of a multiplication
sign.** `plotting.rs:1764` reads `format!("${m_txt}\times10^{{{e}}}$")`, and
Rust interprets `\t` as a tab, so the emitted string is `1.2<TAB>imes10`.
Grep any scientific-tick SVG for a tab and you find it — `05_tiny_magnitudes.svg`
has eight. The unit test at `plotting.rs:6115` asserts the broken string
(`r"$5<TAB>imes10^{-5}$"`), so the suite is green and always has been. TikZ
export sends `2 imes10` to LaTeX.

**2. No axis multiplier, so the axis prints duplicate labels.**
`06_long_labels.svg` has y data spanning 12,344,581 → 12,346,778 and prints
**eleven y-tick labels that all read `1.2×10⁷`** — the axis labels eleven
distinct positions with one string. `axis_tick_formatter` computes the
decimals needed from the smallest tick gap and then discards it; the
scientific branch hardcodes one mantissa digit. `05_tiny_magnitudes.svg`
shows the milder form: the same exponent repeated on nine ticks where one
`×10⁻⁸` at the axis end would do. PGFPlots' `scaled ticks` is the reference
behaviour here.

**3. `legend()` is a silent no-op; bare `legend` works.** With two labelled
series, `legend()` produces an SVG containing the series name zero times and
`legend` produces it once. `catalog/qu_rc_filter.qu:58` uses `legend()`, so a
shipped example renders without its legend — and still exits 0, which is why a
whole-catalog run reports it as passing. Cases 01, 03 and 09 in this collection
were written with `legend()` and are therefore missing their legends: leave
them that way until the call form is fixed, then re-render and the legends
should appear.

**4. Stacked subplots are mostly chrome.** `08_subplot_stack_publication.svg`
gives three panels a 763 × 86 px plot area inside a 900 × 600 canvas — 43% data,
57% margin, an 8.9:1 aspect per panel — and repeats the identical x-tick rail
in all three panels because there is no shared-axis collapsing. The cause is
fixed-pixel margins applied per panel rather than per figure.

## Standards to judge against

Numbers a figure has to survive, gathered alongside this collection.

**Print sizes, at final scale.** Nature 89 mm single column / 183 mm double,
max height 170 mm. Science 5.7 cm / 18.4 cm. IEEE 3.25–3.5 in single column.
Elsevier 85–90 mm / 190 mm.

**Minimums, at final size.** Font: Science floors at 5 pt and targets 7 pt;
IEEE treats 6 pt as absolute. Line weight: Nature warns anything below
0.25 pt disappears in print; Science requires 0.5 pt minimum. A figure that
is legible on screen at 900 px and illegible at 89 mm has failed, and this is
the check the collection exists to make cheap.

**Colour.** Red-green colour vision deficiency affects ~8% of men and ~0.5%
of women. Never encode by hue alone past ~3 levels — pair hue with linetype,
marker, or a direct label. Sequential ramps should be monotonic in lightness
so they survive a greyscale photocopy.

**Contrast.** WCAG 2.1 asks 4.5:1 for normal text and 3:1 for graphical
objects that carry meaning. That floor applies to data marks and axis lines
against the panel; gridlines are decorative and legitimately exempt.

## Adding a case

Add a `.qu` to `sources/` whose `savefig` name matches the file stem, add a
row to the table saying what it stresses, and regenerate. A case earns its
place by covering something none of the existing eleven does — a new axis
type, a new collision, a new mark. Duplicating an existing stress with
prettier data does not.
