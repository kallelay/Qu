# What makes a figure excellent, and what Qu should change

Research pass, 2026-09-05. Sources: the PGFPlots 1.18.2 manual and its own
`.code.tex` (two independent passes), ggplot2's `theme-defaults.R` and the
scales/labeling packages, the local Julia tree (`PlotUtils`, `PlotThemes`,
`Plots`, `Colors`, `ColorSchemes`, `Contour`, `Showoff`), seaborn/matplotlib/
SciencePlots/Makie theme systems, and journal artwork specifications from
Nature, Science, IEEE and Elsevier.

Companion: `docs/design/figure-reference/` holds twelve figures as source +
rendered SVG, so any change proposed here can be judged as a diff. Four
defects are recorded there with evidence.

Standing constraint, unchanged: Qu borrows **techniques**, never **identity**.
Inter / Source Serif 4 / Latin Modern, Qu's curated palettes and the white
journal panel stay. Nothing below asks for grey92, ggplot2's hues, or anyone
else's font.

---

## 1. The one finding every source agrees on

**Every dimension in a figure should derive from a single base size.**

ggplot2 is the clearest implementation: `base_size = 11` pt, and then
`base_line_size = base_size/22` (0.5 pt), `half_line = base_size/2` (5.5 pt)
as the atom for *all* spacing — plot margin, panel spacing, axis-title
margins. Text is never absolute: `axis.text` is `rel(0.8)`, `plot.title`
`rel(1.2)`, tick length `rel(0.5)`, resolved against the inherited parent.
The payoff is that `theme_grey(base_size = 8)` re-derives an entire figure
correctly for a 89 mm column.

PGFPlots reaches the same place discretely — a four-rung ladder where font
size, tick length **and tick density** co-vary together:

| preset | width | tick label | axis label | max space between ticks |
|---|---|---|---|---|
| `normalsize` | 240 pt | inherit | inherit | 35 |
| `small` | 6.5 cm | `\footnotesize` | `\small` | 25 |
| `footnotesize` | 5 cm | `\footnotesize` | `\small` | 15 |
| `tiny` | 4 cm | `\tiny` | `\tiny` | 12 |

Note the direction: smaller figures get *denser* tick spacing in points,
because the type shrank too. A scale factor applied to fonts alone would get
this backwards.

**What Qu does today.** `HALF_LINE_RATIO` and `TICK_TEXT_COLOR` are defined
and referenced exactly once each — at their own definitions. Grid width is a
hardcoded `GRID_LINE_WIDTH = 0.8`, identical across screen and publication
and unscaled by figure size, despite a doc comment claiming ggplot2's
`base_size/22` behaviour. Text offsets are hardcoded pixels that do not track
type size: title baseline at `rect.top + 12.0`, y-tick gap `origin_px - 6.0`,
x-tick baseline `origin_py + 14.0`. At publication scale the x-tick cap-top
lands 1.5 px below the frame; at `fontsize(tick: 24)` it overlaps.

**Change.** Make `base_size` real and derive everything from it. Every
absolute px in the theme structs becomes a multiplier resolved at render.
`theme("publication")` then becomes `base_size = 7`, not a parallel
hand-tuned stylesheet.

---

## 2. Tick selection

Two complete algorithms are available, and they differ in kind.

**PGFPlots — heuristic, ~30 lines, verified empirically.**

```
Wr              = axis_length_pt / max_space_between_ticks     # default 35
desirednumticks = max(try_min_ticks, trunc(Wr) + 1)            # default 4; 3 for log
h               = (max - min) / (desirednumticks - 1)
h               = snap(h)
```

The snap family is **{1, 2, 5} × 10ⁿ** — *not* 1/2/2.5/5 — with mantissa
breakpoints at 1.5 / 3.5 / 7.5. One pass verified the snapping empirically
across a range of widths and data spans and found every case fits **geometric
(log-space) rounding**, none fits linear. Log axes take `h = j·log(10)` with
`j = 1` preferred, so the step is always a whole number of decades; when
`j > 1` the uniform flag is cleared and minor ticks silently disappear.

On failure the whole computation re-runs with `try_min_ticks + 1`, up to 15
levels, then warns `tick computation failed`.

**PlotUtils — a scored optimiser.** `optimize_ticks_typed`,
`PlotUtils/HX80C/src/ticks.jl:205–348`. Candidate steps
`Q = [(1.0,1.0), (5.0,0.9), (2.0,0.7), (2.5,0.5), (3.0,0.2)]`, weights
granularity ¼, simplicity ⅙, coverage ⅓, niceness ¼ (summing to 1), target
5 ticks, and a −10000 penalty for a span exceeding the data range. Plots.jl
overrides to `k_min = 4, k_max = 8`. Worth knowing: this is
Wilkinson-flavoured but **not** the Talbot–Lin–Hanrahan paper — the coverage
term is `1.5·xspan/((len-1)·tickspan)`, not the paper's squared-distance
form. ggplot2 uses the actual TLH algorithm via `labeling::extended()`.

**Recommendation: implement the PGFPlots heuristic.** It is a fraction of the
work, it is the one verified against real output, and its log-space snapping
is the part that produces the "this looks designed" quality. Keep Qu's
existing `nice_ticks` step-rounding (which is already a documented
improvement on Heckbert) and replace only the density rule.

**But fix the thinning first.** Qu's `thin_ticks` strides an existing tick
list, which produces non-canonical steps: a 0.2 step becomes 0.6, and one
reference panel gets `0.1, 0.4, 0.7, 1.0` — a 0.3 step with no zero. Striding
a nice list does not yield a nice list. Choose the step for the available
width instead of choosing a step and then dropping ticks.

---

## 3. De-emphasis: the one real disagreement

PGFPlots and ggplot2 solve the same problem in opposite ways.

- **PGFPlots de-emphasises with colour.** Grid is `thin` (0.4 pt) at
  `black!25`; ticks are 0.2 pt in `gray`; axis lines and plot lines are both
  0.4 pt. Nothing is a sub-0.2 pt hairline.
- **ggplot2 de-emphasises with weight.** `theme_light` grid at `rel(0.5)`,
  `theme_linedraw` at `rel(0.1)` and `rel(0.05)` — i.e. 0.05 pt and 0.025 pt.

Nature warns that anything below **0.25 pt disappears in print**; Science
requires **0.5 pt minimum**. ggplot2's `theme_linedraw` gridlines are four to
ten times below that floor. They look superb on screen and vanish on paper.

**Recommendation: follow PGFPlots.** Qu's centre of gravity is printed
figures. De-emphasise by colour and hold every stroke at or above 0.25 pt at
final size. This is a genuine design decision, not a copy — record it.

---

## 4. Axis multiplier

`scaled ticks` is `true` by default in PGFPlots, triggering when the larger
limit exponent is `> 3` or `< -1`. It emits one `$\cdot 10^{e}$` node at the
far end of the axis (x: 90% along the tick-label line, 5 pt out; y: just past
the top at 1.03), and the manual's stated motivation is simply to *save
space*. It explicitly does not apply to log axes.

**Qu has none of this, and the absence is currently producing a wrong
figure.** `06_long_labels.svg` in the reference collection prints **eleven
y-tick labels all reading `1.2×10⁷`** for data spanning 12,344,581 →
12,346,778. The axis labels eleven distinct positions with one string.
`axis_tick_formatter` computes the decimals needed from the smallest tick gap
and then discards them; the scientific branch hardcodes one mantissa digit.

This is the highest-priority item in this document. An axis that repeats one
label eleven times is not a styling weakness, it is a figure that lies.

---

## 5. Limit expansion

PGFPlots: `enlargelimits = auto`, threshold **0.1** (10% of range) — a value
that appears nowhere in the manual and had to be read out of
`pgfplots.code.tex:6353`. The clever part is `auto`: it enlarges **only
limits that were computed automatically**, leaving any user-set `xmin`/`xmax`
exactly as given.

ggplot2: continuous `expansion(mult = 0.05)`, discrete `expansion(add = 0.6)`,
with an asymmetric two-element form so bars can sit flush on a zero baseline.

Plots.jl: `widen = 1.06`, applied only when limits are `:auto` and the series
type is on a whitelist.

**Qu today** clamps padding to zero when data touches zero — good, a time axis
should not start at −0.02 s — but the result is asymmetric: in
`01_line_screen.svg` the trace is flush against the y-spine on the left and
floats half a unit clear on the right. Adopt `auto` semantics explicitly
(expand only auto-computed limits) and expose `mult`/`add` separately.

Open question flagged by the audit and not resolved: log axes currently take
the same 5% pad *in log space*, so 10 Hz–10 MHz data yields limits of 5 Hz to
20 MHz. Decade snapping may be the better default.

---

## 6. Legends

PGFPlots' defaults, verbatim from `every axis legend`: `at={(0.98,0.98)}`,
`anchor=north east`, `fill=white`, `draw=black`, `inner xsep=3pt`,
`inner ysep=2pt`, `nodes={inner sep=2pt, text depth=0.15em}`,
`legend columns=1`. The sample line is 0.6 cm through three points with
`mark repeat=2, mark phase=2` — **exactly one mark, centred**. The
`text depth=0.15em` keeps baselines even across entries with and without
descenders. Both details are a large part of why its legends look right.

Its known flaw: `legend cell align` defaults to `center`, which is why
essentially every serious user writes `legend cell align=left`.

ggplot2's flaw is worse and better documented: legend order is described in
its own source as "determined by a secret algorithm", with open issues going
back years about stacking order disagreeing with legend order and horizontal
legends silently reversing.

**Qu is ahead here and should stay ahead.** `best_corner` scores eight
candidate boxes against every transformed data point — real geometry, not a
quadrant heuristic, and neither PGFPlots (static placement) nor PGFPlots'
`legend pos` has an equivalent. Keep it.

**But `legend()` is currently a silent no-op.** With two labelled series,
`legend()` produces an SVG containing the series name zero times; bare
`legend` produces it once. `catalog/qu_rc_filter.qu:58` uses the broken form
and ships a legendless figure while still exiting 0.

Adopt: left cell alignment, a one-centred-mark sample line, and even
baselines via a text-depth equivalent.

---

## 7. Theme architecture

The failure mode is documented by matplotlib's own bundled `stylelib`: 31
files, of which **16 are `seaborn-v0_8-*`** — a flattened cross-product of
seaborn's styles × contexts × palettes. Two axes would have expressed 120
combinations in fifteen names; the flat list needed sixteen to cover a
fraction of them, and none of the sixteen compose.

seaborn's split is the design to beat: `set_style` (five looks) crossed with
`set_context` (paper/notebook/talk/poster plus a `font_scale` that multiplies
*only* the font keys, never line widths).

SciencePlots discovered the axes empirically — its directory is literally
`styles/`, `styles/journals/`, `styles/color/`, `styles/languages/`,
`styles/misc/`.

**Proposed for Qu: three declared axes.**

1. **look** — `theme(name)`: grid density, spines, tick direction, legend
   framing. Chrome only.
2. **target** — `target(name)`: physical width and a minimum point size.
   Never colour, never chrome.
3. **palette and font** — `colormap()` / `fontfamily()`, already separate.

`target` should be **physical-first**: declare `width_mm` and `min_pt`, and
let the renderer solve for `base_size` such that the smallest type clears
`min_pt` at that width. "Same figure at 85 mm and on a projector" is then a
different `target`, not a hand-tuned second theme. Since Qu emits SVG, the
floor is statically checkable — warn at export when derived tick type falls
below the target's minimum.

`theme("publication")` becomes an alias for `theme("journal") +
target("print1col")`, preserving today's call site while splitting the two
decisions it currently fuses.

**Two things to fix first.** `theme("publication")` currently overwrites both
`font_family` and the panel palette, so a `colormap()` called *before* it is
silently discarded — colour bundled into look is exactly the defect
SciencePlots split `color/` out to avoid. And of seven theme names Qu
accepts, only three distinct figures come out: `default`, `bw`, `grey`,
`classic` and an arbitrary misspelling render byte-identically. A misspelt
theme in a paper build should warn, not fall back in silence.

---

## 8. Colour and print safety

Red-green colour vision deficiency affects **~8% of men and ~0.5% of women**.
The practical rule that follows is not "use a safe palette" but **never
encode by hue alone** past about three levels — pair hue with linetype,
marker, or a direct label, so the figure survives a greyscale photocopy.

Qu's palettes are in good shape: minimum CIE ΔE between the eight `default`
entries is 28.3, and 32.8 for `publication` — comfortably above
confusability. But **line series carry no dash-pattern field at all**
(`Series` has `marker`, `color`, `hatch`; bars got hatch for photocopy
safety, lines did not), and the `default` palette spans L\* 40–67, so in
greyscale all eight collapse.

PGFPlots' default cycle varies **three channels at once** — hue, mark shape
*and* dash — cycling five colours then repeating them `densely dashed`. It
also ships a first-class monochrome cycle (`black white`) that is print-safe
by construction. Both are worth copying wholesale.

Contrast floor: WCAG 2.1 asks 3:1 for graphical objects that carry meaning.
That applies to data marks and axis lines against the panel; gridlines are
decorative and legitimately exempt.

## Print sizes, for reference

| Journal | Single col | Double | Min font | Min line |
|---|---|---|---|---|
| Nature | 89 mm | 183 mm (max h 170) | 5–7 pt | 0.25 pt |
| Science | 5.7 cm | 18.4 cm | 5 pt floor, 7 pt target | 0.5 pt |
| IEEE | 83–89 mm | ~7 in | 6 pt floor, 8 pt captions | — |
| Elsevier | 85–90 mm | 190 mm | ~7 pt | — |

Vector output sidesteps DPI entirely, which Qu already does.

---

## Ranked plan

| # | Change | Why it ranks here |
|---|---|---|
| 1 | Fix the `\times` tab (`plotting.rs:1764`) and the test that asserts it (6115) | Every scientific tick in every backend currently emits `1.2<TAB>imes10`. One character. |
| 2 | Axis multiplier / shared exponent | Eleven identical tick labels is a figure that lies, not a style weakness. §4. |
| 3 | Make `legend()` work | A shipped catalog example renders without its legend and exits 0. §6. |
| 4 | Wire the theme table; warn on unknown names | Four theme names and a typo are byte-identical today. §7. |
| 5 | `base_size` derivation for every dimension | Removes the hardcoded pixel offsets that already collide at large type. §1. |
| 6 | PGFPlots tick density, and stop striding to thin | Choose the step for the width; never turn 0.2 into 0.3. §2. |
| 7 | Separate `target` from `theme` | Physical-first sizing is the only thing that guarantees ≥5 pt at 89 mm. §7. |
| 8 | Dash-pattern field on line series + a monochrome cycle | Eight series collapse in greyscale today. §8. |
| 9 | Unbundle palette from `theme()` | A `colormap()` before `theme()` is silently discarded. §7. |
| 10 | Measure tick-label extents for margins | `margin_left` is a fixed 68 px; long labels collide with the rotated y-title. |

Two notes on scope. Items 1–4 are defects with evidence in
`figure-reference/`; items 5–10 are design. And a caveat worth keeping in
view: **PGFPlots has no tick-label collision detection at all** — its
`max space between ticks` is a font-blind, content-blind proxy. If Qu adds
real glyph-width measurement (item 10) it would exceed PGFPlots here rather
than merely match it.

## Unresolved

- `clip marker paths`: the two PGFPlots passes disagree on the default. The
  second quotes the manual as *initially false* and is better evidenced.
  Verify before relying on it either way.
- Log-axis padding: 5% in log space, or snap to decade bounds?
- Should `axis tight` be the default for line plots, given the zero-clamp
  already makes the current behaviour asymmetric?
- Is `legend()` vs bare `legend` deliberate MATLAB mimicry? MATLAB's
  `legend()` with no arguments does show a legend, which argues bug.
