# Plotting

The tables below are generated from the interpreter's own builtin dispatch
table (`fn call_builtin` and `fn exec_plot_command` in
`engine/crates/qu-interp/src/lib.rs`), not written speculatively ahead of the
implementation — every function and command listed here runs today and
produces a real `Figure`/`Panel` state that `savefig(...)` can export to SVG,
HTML, or TikZ. A browser-based interactive Playground (`docs/ide.html`) also
exists separately for live plot rendering — see its own chapter in this book.

Most chart-type and annotation calls accept trailing **named style
arguments** collected into a small key/value bag — commonly `color=`,
`label=`, `marker=`; a few calls have their own extras (`bins=`, `colormap=`,
`values=`, `hatch=`, `align=`, `position=`, `outside=`, `radius=`). These are
noted per-row only when a call has something beyond the common trio. `hold on`
(the default) overlays successive calls on the current panel; `hold off`
gives each call its own panel.

## Animation

```
animate(function_name, frames, path, [fps=24], [embed_fonts=])
```

Calls a function you wrote once per frame, with the frame number, and
writes the sequence as one animated SVG.

```qu,ignore
function frame(i)
    t = 2 * pi * i / 60
    x = (0 to 200) / 20
    plot(x, sin(x - t), color = "royalblue")
    ylim(-1.2, 1.2)
    axis off
end function

animate("frame", 60, "wave.svg", fps = 24)
```

**Why SVG and not GIF.** A GIF needs an LZW encoder, caps the palette at
256 colours, and produces a raster that is wrong at any size but the one it
was made at. An animated SVG is one file, opens in any browser, stays sharp
at any size, and reuses the renderer that already exists — the frames *are*
ordinary Qu figures.

**It animates without JavaScript.** Each frame becomes a `<g>` carrying a
CSS animation that shows it for its own slot of the cycle. No SMIL, no
script, so the file works opened from disk, embedded in a page, or dropped
into a slide.

The cost is size: a hundred frames is a hundred figures in one file. That
is the honest trade, and the frame count is yours to choose. A frame draws
into a *fresh* figure, so it cannot inherit marks from the one before it,
and the per-frame `· plot` chatter is suppressed — the animation reports
itself once.

`catalog/qu_starry_night.qu` and `catalog/qu_sea_waves.qu` are the two
worked examples. The sea is built from the deep-water dispersion relation
rather than from a sine, which is why it never repeats its shape.

## Layers

Describe a set of marks once, place it as many times as you like.

```
l = capture(function_name, [extra arguments...])
stamp(l, x, y, [scale=], [rotate=])
```

`capture` runs your function into a scratch panel and keeps what it drew.
`stamp` puts a transformed copy on the real one.

```qu,ignore
function sensor()
    circle(0, 0, 0.28, fill = "#4169e1", alpha = 0.9)
    circle(0, 0, 0.44, color = "#4169e1", width = 1.2)
    plot([0, 0], [-0.44, -0.9], color = "#4169e1", width = 1.2)
end function

s = capture("sensor")
stamp(s, 2, 5)
stamp(s, 6, 5, scale = 0.7)
stamp(s, 10, 5, rotate = 180)
```

**A layer is a value, not a mode.** It can be passed to a function,
returned from one, held in a list, and duplicated by ordinary assignment —
where a recorder with a `begin` and an `end` is one more thing to forget to
close. It separates *what* is drawn from *where*, which is the whole reason
to want it: a sensor symbol, a leaf, a bolt described once and placed forty
times.

Extra arguments go to the function, so one definition makes a family:
`capture("sensor", 0.8)` if `sensor` takes a size.

The transform is translate, then scale, then rotate about the layer's own
origin — the order that makes `stamp(l, 3, 4, rotate = 30)` mean "put it
there, turned". A stamped copy scales its stroke widths with it, so a
shrunk symbol does not read as a blob, and drops its legend label, so one
thing is not keyed twice.

Two things do not rotate, and say so rather than pretending: `vline` and
`hline` are defined as spanning the whole panel in one direction, so a
turned one is not a line of that kind any more. They translate and scale. A
rotated rule is a `polygon` or a two-point `plot`. A rotated `rectangle`
becomes the polygon it actually is.

Error bars and contours inside a layer keep their own geometry rather than
moving: both are measurements of specific data, and a copy of one somewhere
else would be a claim about data that is not there.

## Interactive export

```
explore(function_name, path, names=, from=, to=, steps=)
```

Writes a self-contained HTML page with one slider per parameter and the
figure updating as they move.

```qu,ignore
function view(frequency, damping)
    ...draw...
end function

explore("view", "explorer.html",
        names = ("frequency [Hz]", "damping"),
        from  = (10, 0.02),
        to    = (400, 0.5),
        steps = (12, 8))
```

**The limit is worth understanding before you reach it.** There is no Qu in
a browser, so the page computes nothing: every combination is rendered up
front and the sliders *choose* among them. The cost is therefore the
product of the step counts, and it is capped at 400 — two parameters at ten
steps each is a hundred figures, three at ten is a thousand, which is not a
web page. Within that limit the file needs no server, no network and no
install.

`ui_slider` is the other half of this story. It *declares* a control for a
host to render, which is right when Qu Studio is running the script and
useless when you want to email someone a file. Both exist because both
questions are real.

`catalog/qu_signal_explorer.qu` is the worked example: three sliders over a
band-pass, with the time record, the spectrum and the pole positions all
moving together.

## Shapes

| Function | Signature | Description |
|---|---|---|
| `circle` | `circle(cx, cy, r, [segments=128])` | Draws a circle centered at `(cx, cy)` (numbers, data units) with radius `r` (number, data units). `segments=` (integer, default 128) is how many straight edges approximate the curve — raise it for a very large circle, lower it for a deliberately faceted look. Returns nothing; adds a shape to the current panel. |
| `ellipse` | `ellipse(cx, cy, rx, ry, [rotate=])` | Draws an ellipse centered at `(cx, cy)` (numbers) with semi-axis radii `rx` and `ry` (numbers, data units along x and y respectively). `rotate=` (number, degrees, anticlockwise, default 0) tilts the whole ellipse about its center. Returns nothing. |
| `arc` | `arc(cx, cy, r, from, to)` | Draws part of a circle, as an open stroke. `cx` and `cy` (numbers) are the centre, in data coordinates; `r` (number, positive — a zero or negative radius is an error) is the radius, applied as the same figure to both axes, so a panel whose x and y scales differ draws an ellipse rather than a round arc — a square canvas is what makes it round, not `axis equal`, which only equalises the ranges; `from` (number, degrees) is the angle the sweep starts at, measured counterclockwise from the positive x axis, so `0` is the three-o'clock position; `to` (number, degrees) is the angle it ends at, and a `to` smaller than `from` sweeps clockwise instead. Optional `segments=` (number, default `128`) is how many straight pieces approximate the curve, and `rotate=` (number, degrees) turns the whole arc about its centre. Unlike `circle`, an arc is open — it renders as a stroke (`color=`/`width=`/`dash=`), not a fillable shape. Returns nothing. |
| `rectangle`, `rect` | `rectangle(x0, y0, x1, y1)` | Draws an axis-aligned rectangle. `x0` and `y0` (numbers, data units) are one corner; `x1` and `y1` (numbers, data units) are the corner diagonally opposite it, and which of the two you give first does not matter. `rect` is a plain alias. Returns nothing. |
| `polygon` | `polygon(xs, ys)` | Draws a closed shape connecting the points `(xs[i], ys[i])` in order and back to the first. `xs` and `ys` are equal-length Vecs of numbers (data units). Returns nothing. |
| `point` | `point(x, y)` | Marks a single position `(x, y)` (numbers, data units) with a dot. Returns nothing. Note: in the Annotations table below, `point` additionally auto-labels itself with its own coordinates — same builtin, described twice because it serves both roles. |

A shape given only `fill=` is filled and **not** outlined; give it `color=`
to outline it. That matters more than it sounds: stroking a fill anyway put
a hairline on every edge, which is invisible on one shape and reads as
banding when twenty of them tile a gradient.

`axis off` removes the spines, ticks, tick labels and grid — for an inset,
a sparkline, a schematic, or drawing that is not a plot at all, where the
panel is simply a coordinate system. `axis on` restores them. The
decoration is built and then dropped rather than skipped, so turning it off
moves nothing else on the figure.

All take `color=`, `fill=`, `alpha=`, `width=` and `dash=`.

**These are drawn in data coordinates, and that has a consequence worth
knowing.** The outline is sampled in data space and each sample is then
mapped through the axis transform — so `circle(0, 0, 1)` on a panel whose x
and y scales differ draws the correct *ellipse*, not a round blob. That is
geometrically right and it is what every plotting library does.

To get a round circle you need the two axes at the same number of data
units per pixel, and that is decided by the panel's shape as much as by its
ranges. `axis equal` widens the narrower RANGE until both spans match; it
does not reshape the panel, so on a panel wider than it is tall — the
default — matching ranges still leave the x axis more spread out than the
y, and the circle is still an ellipse. A square canvas is what actually
does it, which is why the example below sets one.

The alternative — emitting an SVG `<circle>` at a pixel radius — would be
simpler and would draw the wrong shape on any panel with unequal scales,
which is most of them, and something that is not an ellipse at all on a log
axis.

```qu
# A square canvas, so that equal ranges really are equal scales and the
# circle below comes out round. Without it the figure would render an
# ellipse while the paragraph above it explained how to avoid one.
figure_size(560, 560)
xlim(-2, 2)
ylim(-2, 2)
axis equal
circle(0, 0, 1, color = "royalblue", fill = "royalblue")
ellipse(0, 0, 1.6, 0.7, color = "crimson", rotate = 30)
arc(0, 0, 1.3, 0, 120, color = "seagreen", width = 2)
polygon([0.8, 1.6, 1.2], [-1.6, -1.6, -0.9], color = "purple", fill = "plum")
rectangle(-1.8, -1.8, -1.2, -1.2, color = "orange")
rect(1.0, -1.8, 1.8, -1.2, color = "teal")
point(0, 1.9)
```

## Colours

Anywhere a colour is taken — `color=`, `edgecolor=`, `background=`, `fill=`
— you may write:

| Form | Example |
|---|---|
| A CSS colour name | `color = "royalblue"` — all 148 of them |
| Hex | `color = "#4169e1"`, or the `#abc` shorthand |
| Hex with alpha | `color = "#4169e180"` |
| Built from numbers | `color = rgb(65, 105, 225)` |
| With opacity | `color = rgba(255, 0, 0, 0.5)` — a fraction is an opacity, a whole number 0–255 is a byte |

Every one of these is resolved to `#rrggbb` at the moment the argument is
read, before any renderer sees it, and a colour that does not resolve is an
error naming the nearest real one:

```
qu: runtime error: plot: color=: `royalbleu` is not a colour -- did you mean `royalblue`?
```

#### Overloads: `rgba`

`rgba(r, g, b, a)` reads its fourth channel `a` two different ways depending
on its own magnitude, so the same call can be written either the "raw byte"
way or the CSS "opacity fraction" way and land on the same colour.

#### Case: byte alpha

Written as a whole number sharing the other three channels' 0–255 range,
`a` is read as a raw alpha byte.

```qu
print(rgba(255, 0, 0, 128))   # "#ff000080" -- half-transparent red, byte alpha
```

#### Case: fractional alpha

Written as a number 0–1 instead, `a` is read as an opacity fraction — the
CSS convention — so `0.5` here lands on the exact same byte as `128` above.
The boundary sits at `1`: `rgba(r, g, b, 1)` is read as the fraction (fully
opaque), never as the barely-visible byte value `1`.

```qu
print(rgba(255, 0, 0, 0.5))   # "#ff000080" -- the same colour, from the CSS-style opacity fraction
```

### Colour spaces

RGB is what a screen takes and a poor space to think in. Each of these
separates one perceptual question from the others, and hands back the
`#rrggbb` every colour argument accepts — so they compose with everything
without a colour type needing to exist.

| Function | Signature | Description |
|---|---|---|
| `hsv` | `hsv(h, s, v)` | Builds a colour from hue `h` (number, degrees, 0–360), saturation `s` (number, 0–1) and value/brightness `v` (number, 0–1). Returns a `"#rrggbb"` string usable anywhere a colour is taken. |
| `hsl` | `hsl(h, s, l)` | Builds a colour from hue `h` (number, degrees, 0–360), saturation `s` (number, 0–1) and lightness `l` (number, 0–1, symmetric about 0.5 — "the same colour, lighter" is one number going up). Returns a `"#rrggbb"` string. |
| `lab` | `lab(L, a, b)` | Builds a colour from CIE L\*a\*b\* coordinates: `L` (number, lightness, 0–100), `a` and `b` (numbers, roughly -128..127, green–red and blue–yellow axes). This is the only space here that is perceptually uniform. Returns a `"#rrggbb"` string. |
| `cmyk` | `cmyk(c, m, y, k)` | Builds a colour from the printer's subtractive space: cyan `c`, magenta `m`, yellow `y`, key/black `k`, each a number 0–1. Returns a `"#rrggbb"` string. |
| `to_hsv` | `to_hsv(color)` | Takes `color` (any string/value the language accepts as a colour — name, hex, `rgb(...)`, etc.) and returns its `(h, s, v)` triple as a Vec of numbers. Also `to_hsl`, `to_lab`, `to_cmyk`, `to_rgb`, each returning that space's own coordinate tuple as a Vec. Returns a 3-element `Vec` of numbers, `[h, s, v]`. |
| `delta_e` | `delta_e(a, b)` | Compares colours `a` and `b` (each any accepted colour string) in CIE76 Lab space and returns a single number: the perceptual distance. Under 1 is invisible, 2–3 is where people start to notice, over 10 is plainly a different colour. Returns a scalar `Num`. |
| `palette` | `palette(n, [saturation=, value=, start=])` | Generates `n` (integer) categorical colours of equal perceptual weight by walking hue at fixed saturation/value. `saturation=` (default 0.62) and `value=` (default 0.78) are numbers 0–1; `start=` (default 215, degrees) is the first colour's hue. Returns a List of `n` `"#rrggbb"` strings. |

HSV is the one that earns its place: fix saturation and value, walk the
hue, and every colour in the set carries the same weight. That is what
`palette(n)` does, and it is why a categorical palette picked by hand in
RGB always has one colour that shouts.

```qu
print(palette(5))
print(to_hsv("royalblue"))
print(delta_e("royalblue", "crimson"))
print(hsv(210, 0.7, 0.9))
print(hsl(210, 0.7, 0.5))
print(lab(50, 20, -30))
print(cmyk(0.1, 0.2, 0, 0.05))
print(rgb(65, 105, 225))
print(rgba(255, 0, 0, 0.5))
print(to_rgb("royalblue"))
print(to_hsl("royalblue"))
print(to_lab("royalblue"))
print(to_cmyk("royalblue"))
```

`delta_e` is the check to run before claiming two series can be told apart.
Under 1 is invisible, 2–3 is where people start to notice, over 10 is
plainly a different colour. Two RGB values differing is not the same
question, and is often reassuring when it should not be.

The Lab conversions go through linear light rather than the gamma-encoded
sRGB values. That step is what separates a correct conversion from the
common wrong one: interpolating encoded values gives a colour that is
visibly too dark, which is the classic fault in every hand-rolled gradient.

That check is not pedantry. Colour strings used to reach the backends
untouched, and the backends disagree: an SVG viewer knows what `royalblue`
means, and the PDF writer reads the first two characters as hex, fails, and
falls back to zero. The same figure came out blue on screen and **black in
the paper**, with nothing printed either time.

## Handles: tying one mark to another

`text`, `annotate`, `vline` and `hline` return a **handle** to what they
drew. Reading a property tells you what was set; assigning one changes the
drawing.

```qu
lab  = text(120, 5.35, "6 dB = 1 bit", color = "#0E7C86")
rule = hline(6, dash = true, width = 1.4)
rule.color = lab.color        # one ink, stated once
print(rule.color)             # "#0e7c86" -- the rule really did pick up the label's colour
```

The alternative is to quote the same colour at both call sites and trust
they stay equal. They do not: a restated value drifts the first time one
of the two is edited.

| Handle | Properties |
|---|---|
| `text`, `annotate` | `color`, `text`, `x`, `y`, `size`, `italic` |
| `vline`, `hline` | `color`, `width`, `dash`, `dot`, `at` |

Reading a property the call never set gives `none`, because what it will
actually be drawn in is the theme's choice, made at render time. Assigning
that `none` onward is refused rather than accepted, since the two defaults
are different inks and accepting it would produce exactly the mismatch the
assignment was written to prevent — set an explicit value on the source
first.

Handles are positions in the figure being built, so they stop being valid
once `figure()` starts a new one.

## Example data

Every example on this page runs against this, so you can paste any of them
into a file and see the figure. Nothing here needs a data file.

```qu
t  = linspace(0, 1, 500)                  # one second at 500 Hz
y  = sin(2 * pi * 5 * t) + 0.2 * randn(500, seed = 1)
mag  = abs(rfft(y))                       # 251 bins for 500 samples
f    = linspace(0, 250, len(mag))         # the matching frequency axis
yhat = sin(2 * pi * 5 * t)                # a "model" to compare against
losses = 1 ./ (1 to 40)                   # a decaying training curve
```

## Figure & Axes Control

| Function/Command | Signature | Description |
|---|---|---|
| `hold` | `hold on` / `hold off` (bare command) | Toggles overlay mode: `on` (default) overlays successive plot calls on the current panel, `off` gives each call a fresh panel. |
| `next` | `next plot [vertical\|horizontal]` (bare command) | Starts a new panel unconditionally (ignores `hold`), stacked below (`vertical`, default) or beside (`horizontal`) the previous one. `next layer` / `next layer N` instead stays in the same cell and moves through the deck a `split plot` grid gave it. |
| `split` (command) | `split plot R C [L]` (bare command) | Lays the whole figure out as an `R`×`C` grid up front and puts the cursor on the first cell, so the plots that follow read as a sequence instead of as separate calls that happen to agree about the shape — the same grid `panel(R, C, i)` builds, said once. A third number is a LAYER count, not a starting cell: `split plot 3 2 2` is that grid two decks deep, twelve panels in six rectangles. `panel`, `subplot` and `figure` are accepted in place of `plot`. |
| `pan`, `zoom` | `pan` / `zoom` (bare command) | **Accepted and ignored.** Both parse and do nothing, so a script written for a live GUI still runs headless — there is no viewport to move when the output is a static file. Documented so that a reader porting such a script learns it here rather than from a `pan` that silently does not pan. `zoom_inset(x0, x1, y0, y1)` is a different, static-friendly feature and a real function call, not this command. |
| `clear` | `clear` / `clear figure` / `clear panel` / `clear panel N` / `clear panel R, C, I` (bare command) | Resets figure/panel state: everything, the current panel, a panel by creation index, or a panel by grid coordinate (even before that cell has been visited). `subplot` is an accepted alias of `panel` in this form too. |
| `axis` | `axis equal` / `axis tight` / `axis auto` / `axis origin` / `axis edge` / `axis scale x\|y log\|linear` (bare command) | `equal`: equal data units per pixel on both axes. `tight`: fit exactly to the data (no 5% padding); `auto` restores the default padding. `origin`: axis spines cross through data `(0,0)` (clamped to the nearest edge if 0 is out of range) instead of framing a box; `edge` restores the box. `scale x/y log`: sets that axis's scale (`z` is accepted as a no-op, reserved for a future 3-D milestone). |
| `legend` (command) | `legend` / `legend off` / `legend top\|bottom\|left\|right\|best [outside]` (bare command) | Bare form toggles visibility and/or sets position; `outside` is an order-independent modifier reserving margin outside the plot. `best` scores the four corners by how many plotted points fall in each quadrant and picks the emptiest. |
| `legend` (function) | `legend("label1", "label2", ...)` | Labels the trailing N series of the current panel (N = number of arguments given, in call order) and turns the legend on. |
| `grid` | `grid on\|off` / `grid minor [off]` (bare command) | `on`/`off` toggles major gridlines. `minor`/`minor off` independently toggles dotted minor gridlines (MATLAB allows minor grid without major grid too, so this never forces major grid on). Calling `grid(...)` as a **function** is accepted but is a no-op — only the bare command form actually changes anything. Returns `Nothing`, in both the command and the (no-op) function form. |
| `box` | `box on\|off` (bare command) | Toggles the panel's bounding box. As a **function** call, `box(...)` is accepted but is a no-op. Returns `Nothing`, in both the command and the (no-op) function form. |
| `yyaxis` | `yyaxis left\|right` (bare command) | Every series plotted from this point on (until the next switch) targets that y-axis; recorded as a switch-point list, not a per-series flag. |
| `xxaxis` | `xxaxis top\|bottom` (bare command) | Qu's own name (not a MATLAB spelling) for a secondary, independently-scaled x-axis, mirroring `yyaxis`. |
| `show` | `show plot` (bare command) | Marks a figure as rendered/finalized (increments the figure counter); headless bookkeeping today, not a live GUI pop-up. |
| `close` (command) | `close` (bare command) | Accepted as a no-op for figures (so scripts written for a live GUI still parse and run). **Note:** `close(handle)` as a *function call* is unrelated — it closes a file handle opened by `fopen`, not a figure. Returns `Nothing` either way — the figure command does nothing at all, and `close(f)` releases the handle. |
| `subplot` / `panel` | `panel(rows, cols, index)` | Selects grid cell `index` (integer, 1-based, row-major) of a `rows`-by-`cols` (integers) grid on the current figure as the panel further drawing targets. MATLAB-`subplot`-style. `subplot(...)` is a plain alias of `panel(...)`, same arguments. Returns nothing. |
| `figure` | `figure()` | Clears/resets the whole figure (all panels), starting a fresh one. Takes no arguments. Must be called with parentheses — bare `figure` (no parens) is not a recognized command verb. Returns `Nothing`. |
| `xlabel` | `xlabel(text, [align=])` | Labels the current panel's x-axis with `text` (string). `align=` (string, one of `"left"`/`"center"`/`"right"`, default `"center"`) sets the label's horizontal alignment. While `xxaxis top` is active, labels the *top* axis instead of the bottom one. Returns nothing. |
| `ylabel` | `ylabel(text, [align=])` | Labels the current panel's y-axis with `text` (string). `align=` (string, `"left"`/`"center"`/`"right"`, plus `"bottom"`/`"top"` for the rotated text, default `"center"`) sets alignment. While `yyaxis right` is active, labels the *right* axis instead of the left one. Returns nothing. |
| `title` | `title(text, [align=])` | Sets the current panel's title to `text` (string). `align=` (string, `"left"`/`"center"`/`"right"`, default `"center"`) sets its horizontal alignment. Returns nothing. |
| `fontfamily` | `fontfamily(name)` | Sets the figure-wide font (string `name`, inherited by every title/label/tick/legend). `"default"`/`"sans"` and `"print"`/`"serif"` are curated stacks (screen sans, paper/journal serif); any other string passes through verbatim as a literal CSS `font-family`. Every curated stack falls back to Latin Modern Math for Greek letters and mathematical operators, because none of the text faces covers them — Latin Modern has no `μ` at all — and `savefig(embed_fonts=true)` embeds that fallback too, but only for the symbols a given figure actually paints. Returns nothing. |
| `fontsize` | `fontsize(n)` / `fontsize(tick=, label=, title=)` | A bare number `n` scales tick/label/title text together (in points); named arguments `tick=`, `label=`, `title=` (each a number, points) override each independently (and win over the positional value if both are given). Returns nothing. |
| `figure_background` | `figure_background(color)` | Sets the current figure's SVG background fill to `color` (string: `"#rrggbb"` hex or a CSS named colour; default `"#ffffff"`), passed straight through, unparsed. Figure-wide, like `fontfamily`/`fontsize`. Does not auto-invert axis/gridline/text colors for a dark background — that's a bigger "themeable plot" feature, not built here. Returns nothing. |
| `xticks` | `xticks(positions)` | Sets explicit x tick positions from `positions` (Vec of numbers, data units), paired in order with `xticklabels(...)` when both are set. `xticks(())` (empty) removes ticks entirely. Returns nothing. |
| `xticklabels` | `xticklabels("a", "b", ...)` | Sets categorical x tick labels from the given strings (variadic positional arguments); without an `xticks(...)` call, spread evenly across the current x-extent. Returns nothing. |
| `ylim` / `xlim` | `ylim(lo, hi)` / `ylim()` | Sets explicit axis limits `lo` and `hi` (numbers, data units); calling with no arguments clears them and restores auto-scaling. Returns nothing. |
| `colormap` | `colormap(name)` | Sets the current panel's categorical/sequential palette to `name` (string — see the Markers & Styling reference below for the full list). An unknown name silently resolves to `"default"` rather than erroring. Returns nothing. |
| `colorbar` | `colorbar` (bare command) or `colorbar()` (function) | Accepted, with or without a `[extend=]`-style argument in the function form (so MATLAB-style scripts parse) but currently a documented no-op in both forms — no colorbar is actually drawn. Returns nothing. |

#### Overloads: `axis`

`axis` is one verb with several keyword arguments that switch it to
fundamentally different behavior, not variations on one theme — worth
seeing side by side rather than picking one and assuming the rest are
similar.

#### Case: equal

`axis equal` forces equal data units per pixel on both axes. Look for the
circle actually looking round, not stretched to whatever aspect ratio the
panel happens to have.

```qu
circle(0, 0, 1)
axis equal
```

#### Case: origin

`axis origin` moves the axis spines to cross through data `(0, 0)` instead
of framing the panel in a box. Look for the spines meeting in the middle of
the plot instead of at its edges.

```qu
plot([-2, -1, 0, 1, 2], [-1, 0, 1, 0, -1])
axis origin
```

#### Case: scale log

`axis scale y log` switches only the named axis to a log scale, leaving the
other one linear. Look for the exponential curve straightening into a line.

```qu
x = 1 to 50
plot(x, exp(0.1 * x))
axis scale y log
```

#### Overloads: `legend`

`legend` is two unrelated builtins sharing one name: the bare command form
controls the legend box itself (on/off/position), and the function-call
form instead labels the panel's own series. Neither one does the other's
job.

#### Case: function form — label series

`legend("sin(x)", "cos(x)")` labels the trailing two series of the current
panel, in call order, and turns the legend on. Look for a legend box
carrying those two exact strings.

```qu
t = linspace(0, 1, 50)
plot(t, sin(t))
plot(t, cos(t))
legend("sin(x)", "cos(x)")
```

#### Case: bare command — position/visibility

`legend top left` is unrelated to labeling — it only moves (or, with `off`,
hides) whatever legend already exists, using whatever labels `plot`'s own
`label=` set. Look for the box relocating to the top-left corner.

```qu
t = linspace(0, 1, 50)
plot(t, sin(t), label = "sin(x)")
legend top left
```

#### Overloads: `grid`

#### Case: bare command

`grid off` (bare) is the real toggle — it actually removes the panel's
gridlines.

```qu
t = linspace(0, 1, 50)
plot(t, sin(t))
grid off
```

#### Case: function call (no-op)

`grid(...)` called with parentheses, as a function, is accepted so a
MATLAB-style script still parses — but it changes nothing. Calling it after
`grid off` does **not** turn the grid back on; the gridlines stay off,
exactly as `grid off` left them.

```qu
t = linspace(0, 1, 50)
plot(t, sin(t))
grid off
grid()   # no-op -- gridlines stay off, not re-enabled
```

These figure/axes calls are meant to be composed, not used one at a time, so
here is a single panel exercising most of the row above: it renames the tick
positions to words instead of numbers on both axes, swaps the figure's font
and background, turns on the (currently non-drawing) colormap/colorbar pair,
and explicitly restates grid/box/scale even though they are already at their
defaults, to show that calling them is always safe. Run it and look at the
labels — `"start"`/`"mid"`/`"end"` and `"low"`/`"zero"`/`"high"` should sit
exactly where `0, 0.5, 1` and `-1, 0, 1` used to, against a warm off-white
page instead of pure white:

```qu
figure()
t = linspace(0, 1, 200)
plot(t, sin(2 * pi * 5 * t))
xticks([0, 0.5, 1])
xticklabels("start", "mid", "end")
yticks([-1, 0, 1])
yticklabels("low", "zero", "high")
fontfamily("sans")
fontsize(tick = 10, label = 12, title = 14)
figure_background("#f7f7f7")
colormap("viridis")
colorbar()
grid()
box()
subplot(1, 1, 1)
xscale("linear")
yscale("linear")
title("Styled panel")
```

## Chart Types

Unless noted otherwise, each of these draws into the figure's *current
panel* (`self.figure.panel_for_new_series()`), increments the figure/point
counter, and accepts the common `color=`/`label=`/`marker=` style trio where
relevant. `plot_xy`-style calls accept either `f(y)` (x defaults to
`0, 1, 2, ...`) or `f(x, y)`.

| Function | Signature | Description |
|---|---|---|
| `plot` | `plot(y)` / `plot(x, y)` / `plot(x, y, "o")` | Draws a basic line (or, with a positional or `marker=` marker string such as `"o"`, a scatter-style) plot of `y` versus `x`. `x` and `y` are equal-length Vecs (or scalars broadcast to match) of numbers, data units; omitting `x` uses `0, 1, 2, ...`. Returns nothing; adds one series to the current panel. |
| `scatter` | `scatter(x, y)` | Same machinery as `plot` — `x`, `y` are equal-length Vecs of numbers — but defaults its marker to `"o"` (dots) instead of `"line"`. Returns nothing. |
| `stem` | `stem(x, y)` | Discrete stem plot: `x`, `y` are equal-length Vecs of numbers; draws a vertical line plus a dot from the baseline up to each `(x[i], y[i])` (marker `"stem"`). Returns nothing. |
| `bar` | `bar(x, y, [values=true], [hatch=true])` | Bar chart: `x` (Vec of numbers or strings, category positions/labels) and `y` (Vec of numbers, bar heights) must be equal length. `values=true` (bool, default false) prints each bar's height above it; `hatch=true` (bool, default false) fills bars with a diagonal hatch instead of solid colour. Returns nothing. |
| `stackbar` | `stackbar(x, y)` | Stacked bar chart: `x` (Vec, category positions) and `y` (Mat or list of Vecs, one row/series per stack segment) with the same `values=`/`hatch=` options as `bar`. Returns nothing. |
| `groupbar` | `groupbar(x, y)` | Grouped (side-by-side) bar chart: `x` (Vec, category positions) and `y` (Mat or list of Vecs, one series per group of bars). Returns nothing. |
| `stair` | `stair(x, y)` | Step (staircase) plot of equal-length numeric Vecs `x`, `y`. `stair` is the only callable spelling — `step` is a reserved keyword (`to ... step ...` ranges) so it can't be a function name; the series is still internally marked marker `"step"`. Returns nothing. |
| `hist` / `histogram` | `hist(samples, [bins])` or `hist(samples, bins=)` | Bins raw `samples` (Vec of numbers) into equal-width buckets and renders as bars. `bins` (integer, positional or `bins=` keyword) sets the bucket count; defaults to `ceil(sqrt(n))` where `n = len(samples)`. Distinct from `bar`, which takes already-aggregated `(x, height)` pairs. Returns nothing. |
| `boxplot` | `boxplot(data)` / `boxplot(data1, data2, ...)` | One group per argument (each a Vec of numbers), placed at `x = 0, 1, 2, ...`; draws a Tukey five-number summary (min/Q1/median/Q3/max plus outliers) per group as a box-and-whisker. Returns nothing. |
| `waterfall` | `waterfall(x, Z)` | `x` is a Vec of numbers (shared horizontal axis); `Z` is a Mat where each row becomes its own line, each successive row offset up-and-right so traces read back-to-front. A 2-D approximation of the stacked-trace look — no true 3-D projection yet. Returns nothing. |
| `scatterfit` | `scatterfit(x, y, [degree])` | Draws a scatter of equal-length numeric Vecs `x`, `y` plus a `polyfit` trend line. `degree` (integer, positional, default 1) is the polynomial degree, sampled densely over `x`'s range. Returns nothing. |
| `splineplot` | `splineplot(x, y)` | Natural cubic spline through the control points `(x[i], y[i])` (equal-length numeric Vecs), resampled onto 200 points for a smooth line. Returns nothing. |
| `pwl` | `pwl(x, y, [n_segments])` | Continuous piecewise-linear least-squares fit through equal-length numeric Vecs `x`, `y`. `n_segments` (integer, positional, default 4) sets how many straight segments approximate the data; plotted through its own fitted breakpoints. Returns nothing. |
| `spiderplot` | `spiderplot(values)` / `spiderplot(v1, v2, ...)` | Radar chart; each argument (a Vec of numbers, one value per spoke/category) becomes its own series, layered onto the current panel's spider chart (repeated calls add more series). Category axis labels aren't wired up yet — spokes are unlabeled. Returns nothing. |
| `polarplot` | `polarplot(theta, r)` | Converts polar coordinates `theta` (Vec of numbers, radians) and `r` (Vec of numbers, radius) to Cartesian (`x=r·cos θ`, `y=r·sin θ`) and draws an ordinary line/scatter series. **Not** a native polar-axis renderer — no circular gridlines, angular ticks, or radial axis; it's a correctly-shaped curve on Cartesian-looking axes. Returns nothing. |
| `raincloud` | `raincloud(data)` / `raincloud(data1, ...)` | Allen et al. (2019) raincloud plot: one group per argument (each a Vec of numbers) at `x = 0, 1, 2, ...`, each rendered as a Gaussian KDE "cloud" + boxplot + jittered raw points ("rain"). Returns nothing. |
| `violin` | `violin(data)` / `violin(data1, ...)` | The same Gaussian KDE and Tukey summary as `raincloud`, arranged the other way: the density is MIRRORED about the category into the closed symmetric shape, with the box inside it and the raw points omitted. `rain=true` puts them back. One group per argument at `x = 0, 1, 2, ...`. Shares `raincloud`'s code path, so the two can never disagree about the density of a given sample. Returns nothing. |
| `beeswarm` | `beeswarm(data, ...)`, `[spread=]`, `[ms=]`, `[marker=]`, `[fill=]` | Every sample drawn, nudged sideways only as far as it must be to clear its neighbours, so the WIDTH is the count. The middle ground between a box (hides the sample) and a raincloud (shows it, but scattered at random). Best below a few hundred points per group. Returns nothing. |
| `ecdf` | `ecdf(data)` / `ecdf(data1, ...)` | Empirical cumulative distribution, one staircase per argument. The honest two-distribution comparison: a density estimate's shape is partly its bandwidth's, and an ECDF has no such parameter. The step rises AT the observation (`i/n`), so the curve reaches 1 at the largest value. Returns nothing. |
| `hexbin` | `hexbin(x, y, [bins=], [colormap=], [mincount=])` | The plane tiled with hexagons, each shaded by how many points fell in it — for the case a scatter cannot do, where past a few thousand marks the picture saturates into a blob. `x` and `y` are equal-length numeric `Vec`s giving the point coordinates; `bins=` (number, default 30, minimum 2) is how many hexagons span the x range; the row count follows from it at three quarters of that, which is the spacing that makes the offset rows interlock rather than overlap; `colormap=` (string) names the shading ramp; `mincount=` (number) leaves a cell unpainted until at least that many points land in it, which is what keeps a sparse tail from covering the plane in near-empty hexagons. Hexagons rather than squares because a hexagon's centre is equidistant from all six neighbours, so a cell's count does not depend on which way the data runs. Returns nothing. |
| `smith` | `smith(Z, [z0=], [grid=], [color=], [lw=], [ms=])` | Impedance on the reflection-coefficient plane: `z = Z/z0`, `Γ = (z−1)/(z+1)`, drawn on the unit disc ruled by constant-resistance circles and constant-reactance arcs. Both families are exact — the Möbius map takes circles to circles. `z0=` defaults to 50 Ω; `grid=false` draws the locus alone. Sets equal aspect, because on unequal axes the circles become ellipses and every angle on the chart is wrong. Returns nothing. |
| `errorbar` | `errorbar(x, y, yerr)` | Draws one capped whisker plus a marker per sample: `x`, `y`, `yerr` are equal-length numeric Vecs, `yerr` the (symmetric) half-height of each whisker in data units. For uncertainty bars on top of a scatter/bar/line plot. Returns nothing. |
| `area` | `area(x, y)` | Filled area under the curve `(x, y)` (equal-length numeric Vecs) down to `y=0`; the zero-baseline special case of `fill_between`. Returns nothing. |
| `fill_between` | `fill_between(x, y_lo, y_hi)` | A filled band between two curves: `x`, `y_lo`, `y_hi` are equal-length numeric Vecs giving the lower and upper boundary at each `x`, e.g. a confidence interval. It's the general form `area` calls into. Returns nothing. |
| `bubble` | `bubble(x, y, sizes)` | Scatter where marker radius encodes a third variable: `x`, `y`, `sizes` are equal-length numeric Vecs; the drawn radius is scaled from `sizes`'s own min/max. Returns nothing. |
| `donut` | `donut(values, [label1, label2, ...])` | Pie chart with a hollow center. `values` is a Vec of numbers (slice magnitudes); trailing string arguments (variadic, optional) label the slices in order. Returns nothing. |
| `pie` | `pie(values, [label1, label2, ...])` | Ordinary pie chart — same call shape as `donut` (`values` a Vec of numbers, trailing strings optional slice labels) with `donut=false`. Returns nothing. |
| `heatmap` | `heatmap(matrix, [colormap=])` | Grid of colored cells: `matrix` is a Mat, each cell coloured by its own value scaled to the matrix's own min/max. `colormap=` (string, default `"blues"`) picks the palette. Returns nothing. |
| `semilogy` | `semilogy(x, y)` | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets the y-axis to log scale. Returns nothing. |
| `semilogx` | `semilogx(x, y)` | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets the x-axis to log scale. Returns nothing. |
| `loglog` | `loglog(x, y)` | Plots equal-length numeric Vecs `x`, `y` like `plot` and additionally sets both axes to log scale. (`semilogx`/`semilogy`/`loglog` share one implementation, differing only in which axis/axes get `Scale::Log`). Returns `Nothing` — this call is made for what it draws. |
| `corrplot` / `corr_heatmap` | `corrplot(table)` | `table` is a Table (record of equal-length numeric columns). Computes the pairwise Pearson correlation of its numeric columns and renders the resulting square matrix as a heatmap. Both spellings call the same code. Returns nothing. |
| `eda` | `eda(table)` | `table` is a Table. One-call exploratory overview: a correlation heatmap in the first grid cell, plus one histogram per numeric column in the remaining cells, auto-gridded (`ceil(sqrt(n))` columns for `n` numeric columns). Returns nothing. |
| `sns_scatter` | `sns_scatter(df, xcol, ycol, [huecol])` | Seaborn-style scatter pulling columns from Table `df`: `xcol`, `ycol` (strings, column names) give the coordinates; optional `huecol` (string, column name) draws one series per distinct hue value (first-seen order) and turns the legend on. Returns nothing. |
| `sns_box` | `sns_box(df, groupcol, valuecol)` | One `boxplot` group per distinct value of column `groupcol` (string, first-seen order) in Table `df`, each group's values drawn from column `valuecol` (string) and labeled with the group's name. Returns nothing. |
| `sns_bar` | `sns_bar(df, groupcol, valuecol, [agg])` | Aggregates column `valuecol` (string, column name) within each distinct value of `groupcol` (string, column name) of Table `df`, using aggregator `agg` (string, positional, default `"mean"`; also accepts things like `"sum"`/`"median"` depending on what the underlying aggregation supports), and plots the one resulting number per group as bars. Returns nothing. |

#### Overloads: `plot`

`plot`'s three call shapes are the most common overload in this chapter,
and the marker-string form genuinely changes what gets drawn — a connected
line versus discrete, unconnected points — not just its color or width.

#### Case: y only

Called with a single Vec, `plot` treats it as `y` and fills in `x` as
`0, 1, 2, ...`. Look for a jagged line with no meaningful x-axis units.

```qu
plot([3, 1, 4, 1, 5])   # x defaults to 0, 1, 2, 3, 4
```

#### Case: x and y (line)

With two equal-length Vecs it draws the default connected line. Look for a
smooth curve with no dots at the sample points.

```qu
x = 0 to 9
plot(x, sin(x / 2))
```

#### Case: marker string (scatter-style)

A third positional string like `"o"` (or `marker="o"`) switches the exact
same call to unconnected dots instead of a line. Look for discrete points
with no line joining them.

```qu
x = 0 to 9
plot(x, sin(x / 2), "o")
```

#### Overloads: `boxplot`

`boxplot`'s argument count, not a keyword, decides how many groups are
drawn side by side — the same pattern also used by `spiderplot` and
`raincloud`, shown once here rather than three times.

#### Case: one group

A single Vec argument draws one box.

```qu
boxplot(randn(50, seed = 20))
```

#### Case: multiple groups

Each further Vec argument adds its own box at the next integer x position.
Look for three boxes side by side instead of one.

```qu
boxplot(randn(50, seed = 20), randn(50, seed = 21) + 0.5, randn(50, seed = 22) - 0.3)
```

The line/scatter family covers continuous data; these five cover data that
is naturally discrete or already summed. Reach for `stem` when each sample
is its own event (a Kronecker-delta-like sequence, an impulse response) and
a connecting line would misleadingly imply values in between; for `stair`
when the value is genuinely constant between changes (a clock signal, a
piecewise-constant control setpoint); for `groupbar`/`stackbar` when you
have several numbers per category and want them either side-by-side
(comparing magnitudes) or stacked (comparing a total and its composition);
and for `waterfall` when you have several related traces (successive
measurement passes, a parameter sweep) and want each to stay legible instead
of drawing directly on top of the last. Six figures come out of this block —
look for the stem's lollipops, the stair's right-angle corners, the two bar
layouts' different use of the same three numbers, and the waterfall's traces
climbing up and to the right of one another:

```qu
x = 0 to 9
y = sin(x / 2)
figure()
stem(x, y)
figure()
stair(x, y)
figure()
groupbar([1, 2, 3], [3, 5, 2])
figure()
stackbar([1, 2, 3], [3, 5, 2])
figure()
Z = [1, 2, 3, 4, 5; 2, 3, 4, 5, 6; 3, 4, 5, 6, 7]
waterfall(0 to 4, Z)
```

These four fit or summarize data rather than drawing it raw. `splineplot`
and `pwl` both take a handful of control points and turn them into a smooth
curve, but for different reasons: a spline is for when the points *are* the
truth and you want a visually smooth curve through all of them exactly,
while `pwl` is for when the underlying process is closer to piecewise-linear
and you want a *fit* — its four segments won't pass through every point.
`spiderplot` and `raincloud` are both about a shape rather than a fit:
`spiderplot` compares several quantities on one radar chart, `raincloud`
shows the full distribution (not just mean and spread) of two or more
samples side by side, with the boxplot's summary, the KDE cloud's shape, and
the individual jittered points all in one figure. Watch for the spline
bending smoothly through the same five points `pwl` approximates with
straight segments, and the raincloud's two "clouds" leaning the same
direction as their generating distribution:

```qu
figure()
splineplot([0, 1, 2, 3, 4], [0, 2, 1, 3, 2])
figure()
pwl([0, 1, 2, 3, 4, 5, 6], [0, 1, 1.5, 3, 3.2, 3.5, 5])
figure()
spiderplot([3, 4, 2, 5, 4])
figure()
raincloud(randn(60, seed = 6), randn(60, seed = 7))
```

Two different kinds of "more than one number per point" live here.
`errorbar` and `fill_between` both communicate uncertainty around a central
curve — a whisker per sample versus a continuous shaded band — and `bubble`
communicates a third *dimension* by encoding it as marker size rather than
position. `donut` and `pie` are the odd ones out: they show how a whole
splits into parts, which is a fundamentally different question ("what
fraction is this?") from everything else on this page ("how does this vary
against that?"), which is also why pie/donut charts are worth reaching for
sparingly. In the rendered figures, check that the error whiskers and the
`fill_between` band both widen and narrow with the same `0.1`/`±0.2`
envelope around the same sine wave, and that the donut's hollow center is
the only visual difference from the plain pie beside it:

```qu
x = 0 to 9
y = sin(x / 2)
figure()
errorbar(x, y, 0.1 * ones(10))
figure()
fill_between(x, y - 0.2, y + 0.2)
figure()
bubble(x, y, abs(y) * 20 + 5)
figure()
donut([30, 20, 50], "a", "b", "c")
figure()
pie([30, 20, 50], "a", "b", "c")
```

Four unrelated needs share this block only because they all involve a
transform of the axes or the data before plotting. `semilogx`/`semilogy`/
`loglog` are for data that spans decades — a Bode magnitude, an exponential
growth curve — where a linear axis would crush most of the interesting
structure into a few pixels at one end. `hist` turns raw samples into a
distribution shape. `contour`/`contourf` need a full 2-D field: note that
`meshgrid` returns one `Record` with `.x`/`.y` fields (not two separate
values), and that `contour`/`contourf` always take three arguments — `(X,
Y, Z)` — never `Z` alone, because Qu has no notion of "the implicit grid a
matrix was measured on". `corr_heatmap`/`corrplot` close the block by
turning a table's numeric columns into a single correlation matrix. In the
output, the semilog plot should look straight (or straighten a curve that
was exponential), the log-log Bode-style curve should show a shallow slope
rather than a hooked one, and the correlation heatmap's diagonal should
read as the same colour throughout — a column is always perfectly
correlated with itself:

```qu
f = logspace(0, 3, 200)
mag = 1 ./ sqrt(1 + (f / 100) .^ 2)
semilogx(f, mag)
figure()
x = 1 to 50
semilogy(x, exp(0.1 * x))
figure()
loglog(f, mag)
figure()
samples = randn(500, seed = 2)
hist(samples, bins = 15)
figure()
g = meshgrid(-3 to 3 step 0.2, -3 to 3 step 0.2)
Z = sin(sqrt(g.x .^ 2 + g.y .^ 2))
contour(g.x, g.y, Z)
figure()
contourf(g.x, g.y, Z)
figure()
tbl = table(a = randn(30, seed = 3), b = randn(30, seed = 4), c = randn(30, seed = 5))
corr_heatmap(tbl)
figure()
corrplot(tbl)
```

If your data already lives in a `table(...)` (a data frame, in other
languages' terms), the `sns_*` family saves you the step of pulling columns
out into loose vectors by hand — you name the columns and Qu does the
grouping. `sns_scatter` colours points by a categorical column, `sns_box`
draws one boxplot group per category, and `sns_bar` aggregates a numeric
column within each category before plotting the result. This example builds
one small table with a numeric `x`, `y` and a two-level categorical `grp`
column, then feeds it to all three; look for the scatter's two colours
(`"a"` vs `"b"`), the two boxplot groups, and the two bars showing each
group's mean `y`:

```qu
df = table(x = randn(30, seed = 1), y = randn(30, seed = 2),
           grp = ["a","b","a","b","a","b","a","b","a","b",
                  "a","b","a","b","a","b","a","b","a","b",
                  "a","b","a","b","a","b","a","b","a","b"])
sns_scatter(df, "x", "y", "grp")
figure()
sns_box(df, "grp", "y")
figure()
sns_bar(df, "grp", "y")
print("rows: " + str(rows(df)))
```

## Annotations & Overlays

All of these attach shapes/callouts to the *current panel* (or, for
`blur_backdrop`, to the whole figure).

| Function | Signature | Description |
|---|---|---|
| `vline` | `vline(x, [color=])` | Draws a vertical reference line spanning the whole panel at `x` (number, data units). `color=` (string, colour, default the theme's rule colour). Returns a **handle** (see "Handles" above) whose `color`/`width`/`dash`/`dot`/`at` properties can be read or reassigned. |
| `hline` | `hline(y, [color=])` | Draws a horizontal reference line spanning the whole panel at `y` (number, data units). `color=` (string, colour). Returns a handle, same properties as `vline`. |
| `rectangle` | `rectangle(x0, y0, x1, y1, [color=])` | Draws a translucent highlighted box from corner `(x0, y0)` to `(x1, y1)` (numbers, data units). `color=` (string, colour) tints the fill. Returns nothing. |
| `xspan` | `xspan(x0, x1, [color=])` | Draws a translucent vertical band covering `x` in `[x0, x1]` (numbers, data units) across the panel's full height. `color=` (string, colour). Returns nothing. |
| `yspan` | `yspan(y0, y1, [color=])` | Draws a translucent horizontal band covering `y` in `[y0, y1]` (numbers, data units) across the panel's full width. `color=` (string, colour). Returns nothing. |
| `zoom_inset` | `zoom_inset(x0, x1, y0, y1, [position=], [outside=], [clip=])` | Magnifies the data region `x` in `[x0, x1]`, `y` in `[y0, y1]` (numbers, data units) into an auto-placed inset panel, with a dashed zoom-box drawn over the source region and connector lines back to it. `position=` (string, reuses the legend's keyword vocabulary: `"top right"`, `"best"`, ...) chooses where the inset sits; `outside=true` (bool, default false) reserves margin and places the inset entirely outside the plot area instead of overlapping it; `clip=` (bool) controls whether the inset's own contents are clipped to its box. Returns nothing. |
| `xbreak` | `xbreak(lo, hi)` | Reshapes the *main* x-axis's coordinate mapping with a fixed-width "//" jag that squeezes out the skipped range `[lo, hi]` (numbers, data units) — linear-scale axes only. Not a sub-panel (unlike `zoom_inset`). Returns nothing. |
| `ybreak` | `ybreak(lo, hi)` | Same as `xbreak`, on the y-axis: squeezes out `[lo, hi]` (numbers, data units). Returns nothing. |
| `blur_backdrop` | `blur_backdrop(x0, y0, x1, y1, [radius=])` | Figure-level (not per-panel) depth-of-field effect: `(x0,y0)`–`(x1,y1)` (numbers, fractions `0..1` of the whole rendered canvas) name the rectangle that stays sharp; everything else is Gaussian-blurred. `radius=` (number, blur strength). SVG-native (`<feGaussianBlur>`); the TikZ export has no filter-primitive equivalent so it degrades to an unblurred render plus an explanatory comment. Returns nothing. |
| `text` | `text(x, y, text)` | Plain text label `text` (string) placed at `(x, y)` (numbers, data units), no marker dot. Returns a handle with `color`/`text`/`x`/`y`/`size`/`italic` properties. |
| `annotate` | `annotate(x, y, text)` | Same as `text` — label `text` (string) at `(x, y)` (numbers, data units) — but also draws a marker dot at that point. Returns a handle with the same properties as `text`. |
| `arrow` | `arrow(x0, y0, x1, y1)` | Draws an arrow from `(x0, y0)` to `(x1, y1)` (numbers, data units), no label. Returns nothing. |
| `arrowtext` | `arrowtext(x0, y0, x1, y1, text)` | Same as `arrow` — from `(x0, y0)` to `(x1, y1)` (numbers, data units) — but the arrowhead ends in a point labeled with `text` (string). Returns nothing. |
| `point` | `point(x, y)` | Draws a highlighted "radioactive" callout marker at `(x, y)` (numbers, data units), auto-labeled with its own coordinates, e.g. `"(3, 4)"` — for calling out one sample without a separate `text(...)` call. Returns nothing. |

These overlays exist for the moment a plain line/scatter isn't enough to
make a point: `xspan`/`yspan` shade a region you want the reader to notice
without drawing a whole extra series over it (a resonance band, a
confidence window); `arrow`/`arrowtext` point at a specific feature instead
of relying on the reader to find it; `xbreak`/`ybreak` compress a wide but
uninteresting stretch of an axis so the parts that matter both get more
room; and `blur_backdrop` throws everything except one rectangle out of
focus, the way a camera would, to pull the eye toward it. The example below
touches all of these across four figures — in the first, look for the two
tinted rectangles and the labeled arrow pointing at "peak"; in the second
and third, a straight `y = x` line should visibly bend at the "//" jag where
the break swallows `[0.3, 0.6]`; in the fourth, only the rendered figure's
own center square stays sharp. The trailing `tex`/`printtex` calls are
unrelated to any of this — included here only to print something concrete
for the block, not to demonstrate an overlay:

```qu
t = linspace(0, 1, 200)
y = sin(2 * pi * 5 * t)
plot(t, y)
xspan(0.2, 0.4, color = "orange")
yspan(-0.3, 0.3, color = "green")
arrow(0.6, 0, 0.8, 0.8)
arrowtext(0.85, -0.5, 0.95, 0.2, "peak")
figure()
plot(t, t)
xbreak(0.3, 0.6)
figure()
plot(t, t)
ybreak(0.3, 0.6)
figure()
plot(t, y)
blur_backdrop(0.2, 0.2, 0.8, 0.8, radius = 4)
print(tex([1, 2; 3, 4]))
printtex([1, 2; 3, 4])
```

## Diagrams

Auto-laid-out box-and-arrow diagrams — the coordinates are computed, not
placed by hand as `rectangle`/`arrow`/`text` above require. Both return the
diagram as an SVG string (so it can be captured, `print`ed, or embedded)
and, given `file=`, also write it to disk. Format is chosen by extension
the same way `savefig` chooses one: `.svg` and `.html`/`.htm` are
implemented; `.png` is **not** (same whole-figure rasterizer gap `savefig`
documents) and raises a clear error naming `.svg`/`.html` instead.

| Function | Signature | Description |
|---|---|---|
| `diagram_pipeline` | `diagram_pipeline(fn, [file=])` | Renders a `\|>` pipe chain as a left-to-right chain of labeled boxes, one per stage. `fn` is a zero-parameter lambda whose body is the pipeline (`() := data \|> lowpass(fc=1000) \|> fft() \|> abs()`) — a call argument is normally evaluated before any builtin sees it, which would run the pipeline instead of describing it, so the pipeline has to be wrapped so its `\|>` chain survives as an expression. A function name (string) defined the same one-line way (`mypipe() := data \|> f() \|> g()`) is also accepted. The source expression should be a variable or parenthesized, not a bare `to` range literal — `0 to 99 \|> f()` parses as `0 to (99 \|> f())`, since `\|>` binds tighter than `to`. Returns the SVG (string). |
| `algorigram` | `algorigram(name, [file=])` | Renders a flowchart of the named function's control flow: `if`/`else` as a decision diamond with `yes`/`no` branches that rejoin, `while`/`for`/`do...loop` as a decision with a routed "repeat" back-edge, `select case` and `try`/`catch` as their own multi-way/two-way branches. `name` (string) must resolve to exactly one overload (§ multiple dispatch) — an overloaded function raises a clear error asking for a single-overload name instead. Returns the SVG (string). |

```qu
function classify(x)
  if x > 0
    y = 1
  else
    y = -1
  end if
  return y
end function
svg = algorigram("classify")
data = 0 to 99
print(diagram_pipeline(() := data |> lowpass(fc=1000) |> fft() |> abs()))
```

## Interpolation for plotting

| Function | Signature | Description |
|---|---|---|
| `interp1` | `interp1(x, y, xq, [method])` | Interpolates (and freely extrapolates — there's no opt-in flag, a query point outside `x`'s range is always answered) `y` at query point(s) `xq`. `x`, `y` are equal-length Vecs of numbers (the known samples); `xq` is a number or a Vec of numbers (the query point(s)), and the return type matches: a number for a scalar `xq`, a Vec for a vector one. `method` (string, positional, default `"linear"`) is `"linear"` (extrapolates via the boundary segment's slope), `"spline"` (natural cubic, shares `splineplot`'s solver), or `"nearest"` — these are string arguments to `interp1`, not separate functions. |

#### Overloads: `interp1`

The `method` string picks between three genuinely different curve shapes
through the same six points — worth plotting side by side rather than
trusting the names alone.

#### Case: linear

Straight segments between consecutive known points. Look for sharp corners
at each of the six original samples (drawn as a separate scatter).

```qu
x = [0, 2, 4, 6, 8, 10]
y = [0, 1.8, 1.2, 3.5, 3.0, 4.8]
xq = 0 to 10 step 0.1
plot(xq, interp1(x, y, xq, "linear"))
scatter(x, y)
```

#### Case: spline

A natural cubic spline through the same points. Look for a smooth curve
with no corners, that can overshoot slightly between points the straight
segments above would not.

```qu
x = [0, 2, 4, 6, 8, 10]
y = [0, 1.8, 1.2, 3.5, 3.0, 4.8]
xq = 0 to 10 step 0.1
plot(xq, interp1(x, y, xq, "spline"))
scatter(x, y)
```

#### Case: nearest

Snaps each query point to whichever known sample is closest. Look for a
blocky, staircase-like curve instead of anything smooth or straight.

```qu
x = [0, 2, 4, 6, 8, 10]
y = [0, 1.8, 1.2, 3.5, 3.0, 4.8]
xq = 0 to 10 step 0.1
plot(xq, interp1(x, y, xq, "nearest"))
scatter(x, y)
```

## Saving & Export

| Function | Signature | Description |
|---|---|---|
| `savefig` | `savefig(path, [embed_fonts=])` | Exports the current figure to `path` (string, a filesystem path); format is chosen by the file extension. `.svg`, `.html`/`.htm` (an SVG embedded in a minimal page), `.tikz`/`.tex` (LaTeX/TikZ source) and `.pdf` are implemented and share one draw-command list so they never visually disagree. `.png` is **not implemented** for a whole figure (it needs a real 2-D rasterizer) and raises a clear error saying so; `save_image(path, img)` does write a PNG of raster `Value::Image` content. `embed_fonts=` (bool) inlines the actual font bytes (only possible for the curated `fontfamily(...)` presets), subset to the glyphs the figure actually draws so a journal-theme figure costs tens of kilobytes rather than the whole font file; it defaults to on under a print-ready theme and off otherwise. A `.pdf` always embeds its faces, and subsets them the same way. Returns `Nothing`; the file on disk is the result. |
| `save` / `save_all` | `save(path, "name1", "name2", ...)` / `save_all(path)` | Not figure-specific: serializes named top-level variables — `path` (string) plus variadic string variable names for `save`, or every top-level variable for `save_all(path)` — to a human-readable JSON file. Operates on the base workspace only, like MATLAB's own `save`. Included here because it's the companion of `savefig` for round-tripping plotted *data* rather than the rendered figure; `load(path)` restores it. Returns nothing. |
| `tex` | `tex(x)` | **Not figure export** — converts a single Qu value `x` (number, Vec, Mat, or complex) to a LaTeX math-mode string (e.g. a matrix becomes a `bmatrix` environment) and returns it as a string. Pairs with `str` the way `printtex` pairs with `print`. Returns a `Str`. |
| `printtex` | `printtex(x)` | Same conversion as `tex(x)` on value `x` (number, Vec, Mat, or complex), but prints it directly instead of returning a string. Returns nothing. |

The actual **figure**-to-LaTeX export path is `savefig("plot.tikz")` or
`savefig("plot.tex")` (see the `savefig` row above) — `tex`/`printtex` are a
separate, value-level feature that happens to share the "LaTeX" theme, not
alternate spellings of figure export.

## Markers & Styling reference

Marker strings (passed as `marker="..."` or, for a few chart calls, as a
bare positional string like `plot(x, y, "o")`):

- `"line"` — the default for `plot`/`splineplot`/`pwl`/`waterfall`/etc.
- `"o"` — circle marker (scatter-style).
- `"x"` / `"+"` — cross marker (both render identically).
- `"s"` / `"square"` — square marker.
- `"stem"` — vertical stem-and-marker (set automatically by `stem(...)`).
- `"bar"` / `"stackbar"` / `"groupbar"` — bar-family renderings (set
  automatically by the matching chart-type call).
- `"step"` — staircase line (set automatically by `stair(...)`; not
  independently callable as a marker name since `step` is a reserved
  keyword).

A marker spec may also carry a **line style suffix**, and then the series
is drawn as markers joined by that line: `"o-"` (circles, solid), `"s--"`
(squares, dashed), `"^:"` (triangles, dotted), `"D-."` (diamonds,
dash-dot). A spec that is all line and no glyph — `"-"`, `"--"`, `":"`,
`"-."` — draws the line alone. Dash patterns scale with the line's weight,
so a dash stays longer than the line is thick at any `lw`.

Keeping a marker readable on a busy background — a filled contour, a dense
cloud, another series — has two answers, and they compose:

| Keyword | Effect |
|---|---|
| `edge=` (or `mec=`) | Outlines the marker **in place**, in the colour given. `edge = "#ffffff"` is the usual choice on a saturated field. |
| `shadow=true` | Casts an **offset, translucent copy** of the marker underneath it. Scales with the marker's own size, so a 2pt dot and a 9pt star are shadowed in proportion. |

`fill=true`/`false` (alias `mfc=`) chooses filled or hollow; hollow is the
default for most glyphs. Composite glyphs (`boxplus`, `circletimes`,
`crosshair`, …) are a shape plus an overlay in the same ink, so filling
them hides the mark that distinguishes them — draw those hollow.

See `docs/design/figure-reference/svg/13_markers_and_lines.svg` for every
glyph, line style, weight and size on one sheet.

Named colormaps (`colormap(name)` or a chart's own `colormap=`), from
`plotting::PALETTES`:

- `"default"` — Qu's own curated 8-color mix; used implicitly whenever no
  `colormap(...)` call has run.
- `"kay"` — the same 8 colors as `"default"`, reordered.
- `"qu"` — the "Qu Best" palette: hues spaced 45° apart, reordered so
  consecutive entries sit roughly 180° apart (better for a cycling series
  palette than a plain hue sort).
- `"tableau10"` — the standard Tableau 10 / matplotlib `tab10` cycle.
- `"viridis"`, `"plasma"` — perceptually-uniform sequential scales; `"viridis"`
  is the default for `corr_heatmap`/`corrplot`/`eda`'s correlation panel.
- `"warm"`, `"cool"`, `"mono"` — small curated sequential scales.
- `"matlab"` — MATLAB's own default `ColorOrder` (R2014b+), for porting a
  MATLAB figure's familiar blue/orange-red/yellow/purple line cycle.
- `"blues"` — ColorBrewer "Blues"; the default for plain `heatmap(...)`.

An unrecognized colormap name is accepted and silently resolves to
`"default"` rather than erroring the whole script over a typo.

## Worked examples

The examples below are the smallest complete scripts each showing one common
task end to end, rather than one call in isolation. Each is self-contained
except where it says otherwise — it either defines its own data or reuses
the vectors from the "Example data" section above (`t`, `y`, `mag`, `f`,
`yhat`, `losses`), so pasting the Example data block once and then any one
of these below it should reproduce exactly what's described.

A basic plot with labels, legend, and export — the shape almost every script
starts from: two series on one panel, both axes labeled, a legend to tell
them apart, and a save to disk. Look for two full sine/cosine periods
sharing one set of axes, colour-coded and named in the legend box:

```qu
x = 0 to 2*pi step 0.05
plot(x, sin(x), label="sin(x)")
plot(x, cos(x), label="cos(x)")   # hold on (default) overlays on the same panel
xlabel("x")
ylabel("amplitude")
title("Two waves")
legend
grid on
show plot
savefig("waves.svg")
```

A 2x2 subplot grid, for laying out several related views of the same
dataset — here a signal's time-domain trace, its frequency spectrum, an
actual-vs-predicted scatter, and a training-loss curve, the four views a
typical signal-processing or model-fitting report wants side by side. Each
`panel(2, 2, k)` call selects a cell before the plot call that fills it;
expect four small, independent-looking charts sharing one figure, not one
chart repeated four times:

```qu
panel(2, 2, 1)
plot(t, y)
panel(2, 2, 2)
plot(f, mag)
panel(2, 2, 3)
scatter(y, yhat)
panel(2, 2, 4)
plot(losses, color="orange")
```

A scatter with a fitted trend line, for the everyday question "is there a
relationship here, and if so, roughly what shape?" `scatterfit` draws the
raw points and a `polyfit` curve through them in one call instead of two.
Reusing the shared noisy sine `t`/`y` here is a deliberately awkward choice
to make a point: a periodic signal has no real quadratic trend, so the
fitted degree-2 curve should come out nearly flat while the scatter itself
keeps oscillating around it — a reminder that `scatterfit` will always draw
*a* line, whether or not a polynomial is the right model for the data:

```qu
scatterfit(t, y, 2)   # degree-2 polynomial fit through the scatter
xlabel("x")
ylabel("y")
```

A histogram plus boxplot side by side, for comparing a distribution's
overall shape against a per-group five-number summary in one figure. The
histogram (left) bins the shared noisy sine `y` into 20 buckets; the boxplot
(right) compares three freshly generated groups so the two panels are
visibly independent of each other:

```qu
group_a = randn(40, seed = 11)
group_b = randn(40, seed = 12) + 0.5
group_c = randn(40, seed = 13) - 0.3
panel(1, 2, 1)
histogram(y, 20)
panel(1, 2, 2)
boxplot(group_a, group_b, group_c)
```

An annotated series with a zoomed inset, for the common case of a feature —
a transient, a peak, a glitch — that is too small on the full-range plot to
read its value off directly. This example builds its own decaying
oscillation (rather than reusing the shared data, since the reference
point at `x=3` needs an axis that actually reaches past 3), marks that
point with a vertical rule and a text callout, and then magnifies the
region right around it into an inset panel in the top-right corner. Look
for the small dashed box around `x=3` on the main curve, connector lines
running to a zoomed-in copy of that same region, and the callout text
sitting at the curve's value there:

```qu
xs = linspace(0, 5, 400)
ys = sin(2 * pi * 1.2 * xs) .* exp(-0.3 * xs)
y_at_3 = sin(2 * pi * 1.2 * 3) * exp(-0.3 * 3)
plot(xs, ys)
vline(3, color="red")
annotate(3, y_at_3, "transient")
zoom_inset(2.8, 3.2, -0.1, 0.1, position="top right", outside=true)
print(y_at_3)
```

A heatmap, for any matrix where the *pattern* of values matters more than
reading off individual numbers — a correlation matrix, a confusion matrix,
a physical field. `colorbar` (the bare command; see the Figure & Axes
Control table above) is accepted here for MATLAB-style familiarity, though
today it doesn't change the rendered figure. Expect a 10x10 grid of cells
shaded from the `"viridis"` scale according to each cell's own value:

```qu
m = randn(10, 10, seed = 4)
heatmap(m, colormap="viridis")
colorbar
title("Random field")
print("value range: [" + str(min(m)) + ", " + str(max(m)) + "]")
```

A correlation overview of a table in one call, for the first-look pass over
a new dataset before anything else — `eda` puts a correlation heatmap and
one histogram per numeric column on a single auto-gridded figure, which is
usually enough to spot an obviously skewed column or an unexpectedly
strong pairwise correlation before writing any analysis code:

```qu
df = table(a = randn(30, seed = 3), b = randn(30, seed = 4), c = randn(30, seed = 5))
eda(df)   # correlation heatmap + one histogram per numeric column, auto-gridded
print("columns: " + str(cols(df)))
```

Interpolating between measured points before plotting, for the case where
you have a handful of measurements and want a continuous curve to compare
them against a model, or simply to make a sparse instrument reading look
like a continuous signal. `interp1` fills in the query points `xq`; the
example then plots the smooth `"spline"` fit as a line with the original
six measurements overlaid as a separate scatter series, so both the raw
data and the interpolation are visible on the same axes:

```qu
x_measured = [0, 2, 4, 6, 8, 10]
y_measured = [0, 1.8, 1.2, 3.5, 3.0, 4.8]
xq = 0 to 10 step 0.1
yq = interp1(x_measured, y_measured, xq, "spline")
plot(xq, yq, label="spline fit")
scatter(x_measured, y_measured, label="measurements")
legend
```

## More functions

| Function | Signature | Description |
|---|---|---|
| `panel`, `subplot` | `panel(rows, cols, k)` | Selects cell `k` (integer, 1-based, row-major) of a `rows`-by-`cols` (integers) grid as the target for further drawing. `subplot` is the same call under MATLAB's name. Returns nothing. |
| `legend` | `legend()` or `legend off` | Shows or hides the key for the current panel. With no argument it turns on and places itself where it overlaps least; `legend outside` (bare command) reserves margin instead. See also the function form `legend("label1", ...)` in the Figure & Axes Control table above, which labels series rather than toggling visibility. Returns nothing. |
| `theme` | `theme(name)` | Sets the whole-figure visual theme from `name` (string): one of `"default"`, `"publication"`, `"bw"`, `"minimal"`, `"grey"`, `"classic"`. `"publication"` also embeds fonts, because a figure asked to be print-ready has to look the same wherever it lands. Returns nothing. |
| `figure_size` | `figure_size(w, h)` or `figure_size("ieee1")` | Sets canvas size either from `w`, `h` (numbers, raw units) or from a named journal-column preset string (e.g. `"ieee1"`). Type sizes scale with the square root of the area, so a figure shrunk to a column keeps its proportions. Returns nothing. |
| `xlim`, `ylim` | `xlim(lo, hi)` | Fixes an axis range from `lo`, `hi` (numbers, data units). After `yyaxis right` these apply to the right-hand axis. Returns nothing. |
| `xscale`, `yscale` | `xscale("log")` | Sets that axis's scale from a string, `"linear"` or `"log"`. A log axis draws minor ticks per decade without being asked. Returns nothing. |
| `yticks`, `yticklabels` | `yticks([0, 1, 2])` | `yticks` takes a Vec of numbers (tick positions, data units); `yticklabels` takes variadic strings to relabel them in order. `yticks(())` (empty) removes ticks entirely; `axis off` removes the whole frame instead. Returns nothing. |
| `hist`, `histogram` | `hist(samples, [bins])` | Bins `samples` (Vec of numbers) into equal-width buckets and renders as bars; `bins` (integer, positional or `bins=` keyword) defaults to `ceil(sqrt(n))` for `n = len(samples)`. Returns nothing. (Same builtin as the `hist`/`histogram` row in the Chart Types table above.) |
| `meshgrid` | `meshgrid(x, y)` | Builds the pair of coordinate matrices a scalar-field plot needs from axis vectors `x` (Vec of numbers, length `nx`) and `y` (Vec of numbers, length `ny`): every `(x[i], y[j])` combination, laid out as two `ny`-by-`nx` matrices. **Returns a single Record** with fields `.x` and `.y` (each a Mat) — not two separate values, so `g = meshgrid(xs, ys)` then `g.x`, `g.y` is the pattern, never a multi-assignment like `[X, Y] = meshgrid(...)` (Qu has no destructuring assignment). Typically consumed directly by `contour`/`contourf`. |
| `contour`, `contourf` | `contour(x, y, Z)` | Iso-lines through scalar field `Z` (a Mat with `len(y)` rows and `len(x)` columns) over axis vectors `x`, `y` (Vecs of numbers); `contourf` fills the bands between levels instead of only outlining them. **All three arguments are always required** — `contour(Z)` alone is a runtime error (`contour(x, y, Z) needs three arguments`), even though `Z` alone is enough information for some other libraries' contour calls. `levels=` (integer count, or a Vec of explicit level values) controls how many iso-lines are drawn or exactly which. Returns nothing. |
| `corr_heatmap`, `corrplot`, `corrmat` | `corr_heatmap(t)` | Computes the pairwise Pearson correlation of Table `t`'s numeric columns. `corr_heatmap`/`corrplot` render it directly as a heatmap and return nothing; `corrmat(t)` instead returns a tagged Model (`.matrix`, an `n`-by-`n` Mat of correlation values, plus `.labels`, a List of the `n` column names) for when you want the numbers rather than a picture — calling `.plot()` on that Model also renders the labeled heatmap. Returns a tagged `Model` from `corrmat` — `.matrix`, an `n`-by-`n` `Mat` of correlations, plus `.labels`, a `List` of the `n` column names; `corr_heatmap`/`corrplot` draw instead and hand back no value. |
| `capture` | `capture("fn", [args...])` | Runs the named drawing function `"fn"` (string, a function you defined) into a scratch panel, passing along any further positional `args...`, and keeps what it drew as a value (a "layer"). See Layers, near the top of this chapter. Returns a layer value. |
| `stamp` | `stamp(layer, x, y, [scale=], [rotate=])` | Places a transformed copy of a captured `layer` (the value `capture` returned) at position `(x, y)` (numbers, data units). `scale=` (number, default 1) and `rotate=` (number, degrees, default 0) transform the copy: translate, then scale, then rotate about the layer's own origin. Returns nothing. |
| `animate` | `animate("fn", frames, path, [fps=])` | Calls function `"fn"` (string) once per frame index from `0` to `frames - 1` (integer) and writes the sequence to `path` (string) as one animated SVG, no JavaScript. `fps=` (number, default 24) sets playback speed. Returns nothing. |
| `explore` | `explore("fn", path, names=, from=, to=, steps=)` | Writes a self-contained HTML page to `path` (string) with one slider per parameter of function `"fn"` (string). `names=` (List of strings, slider labels), `from=`/`to=` (Lists/tuples of numbers, per-parameter ranges), `steps=` (List of integers, per-parameter step counts) are parallel, one entry per parameter. Every combination is rendered up front (capped at 400 total), because a browser cannot run Qu. Returns nothing. |
| `rgb`, `rgba` | `rgb(r, g, b)` | Builds a colour from channel numbers: `r`, `g`, `b` (and, for `rgba`, a fourth channel `a`) — a fractional 0–1 fourth channel is an opacity, a whole number 0–255 is a byte value like the other three. For channels that are computed rather than typed literally. Returns a `"#rrggbb"` (or with-alpha) string. |
| `to_rgb`, `to_hsl`, `to_lab`, `to_cmyk` | `to_rgb(color)` | Takes apart any colour the language accepts (`color`, a string) into that space's own coordinates, returned as a Vec of numbers — `to_rgb` gives `(r,g,b)`, `to_hsl` gives `(h,s,l)`, `to_lab` gives `(L,a,b)`, `to_cmyk` gives `(c,m,y,k)`. See also `to_hsv` in the Colour spaces table above, which is the same idea. Returns a `Vec` of numbers — 3 elements for `to_rgb`/`to_hsl`/`to_lab`, 4 for `to_cmyk`. |
| `sweep` | `sweep(f0, f1, fs, n, [method=])` | Generates a linear (or, with `method="logarithmic"`, log) frequency sweep from `f0` to `f1` (numbers, Hz) sampled at rate `fs` (number, Hz) for `n` (integer) samples. `chirp` is the exact same call under a second name — both share one implementation. Returns a `Signal` (a tagged Vec carrying its own sample rate) of length `n`. |
| `chirp` | `chirp(f0, f1, fs, n, [method=])` | Identical to `sweep` above — same four positional arguments (start frequency, end frequency, sample rate, sample count) and the same `method=` (string, `"linear"` default or `"logarithmic"`/`"log"`/`"exponential"`). Returns a `Signal` of length `n`. |
| `triangle`, `sawtooth` | `triangle(freq, fs, n)` / `sawtooth(freq, fs, n)` | Each generates one period-repeating waveform: `freq` (number, Hz), `fs` (number, sample rate in Hz), `n` (integer, sample count) are all scalars, in that order. `sawtooth` ramps linearly from -1 up to (but not including) +1 each period then drops back to -1; `triangle` rises and falls symmetrically. Both are band-unlimited — they carry every harmonic, which is the point when testing a filter. Returns a `Signal` of length `n`. |
| `square` | `square(freq, fs, n, [duty=])` | A bipolar (±1) periodic pulse train. `freq` (number, Hz) is the repetition rate; `fs` (number, Hz) is the sample rate the train is generated at; `n` (number) is how many samples to produce; `duty=` (number, 0–1, default 0.5) is the fraction of each period spent at `+1` rather than `-1`. Returns a `Signal` of length `n`. |
| `pwm` | `pwm(modulator, carrier_freq, fs, [carrier=])` | Natural-sampling pulse-width modulation: `modulator` is a Vec of numbers (any signal, conventionally in `[-1, 1]`), `carrier_freq` and `fs` are numbers (Hz). Emits `+1` where the modulator exceeds a carrier wave oscillating in `[-1, 1]` at `carrier_freq`, else `-1`. `carrier=` (string, `"triangle"` default or `"sawtooth"`/`"saw"`) picks the carrier shape. Returns a `Signal` the same length as `modulator`. |
| `signal` | `signal(x, fs)` | Tags Vec `x` (numbers) with sample rate `fs` (number, Hz), so later calls (like `rfft`) do not have to be told it again. Returns a `Signal`. |
| `ui_slider` | `ui_slider(label, min, max, [default], [step=], [id=])` | Declares a slider control (for a value you sweep) in an `explore` page or Qu Studio, labeled `label` (string) with range `min`..`max` (numbers). `default` (number, positional, default `min`) is the starting value; `step=` (number) sets the increment; `id=` (string, default `label`) is the identifier the host uses to persist/report a value, useful when two sliders would otherwise share a label. Outside a host, returns the current (or default) value as a number — arguments are positional, not `steps=`-style keywords, and `min`/`max` (not `from`/`to`) are the names that matter. Returns a scalar `Num` — the host's current value for this control, or `default` outside one. |
| `ui_number` | `ui_number(label, [default], [id=])` | Declares a free-entry number box (for a value you type exactly rather than sweep), labeled `label` (string). `default` (number, positional, default 0) is the starting value; `id=` (string) as with `ui_slider`. Returns the current (or default) value as a number. |
| `ui_select` | `ui_select(label, options, [default], [id=])` | Declares a dropdown, labeled `label` (string), choosing among `options` (a List of strings — or of any values, displayed via their string form). `default` (positional, default `options`'s first entry) and `id=` (string) as above. Returns the current (or default) choice as a string. |
| `ui_checkbox` | `ui_checkbox(label, [default], [id=])` | Declares an on/off toggle, labeled `label` (string). `default` (bool, positional, default false) and `id=` (string) as above. Returns the current (or default) value as a bool. These two (`ui_select`/`ui_checkbox`) are for a parameter that is a mode rather than a magnitude. |
| `ui_button` | `ui_button(label, [id=])` | Declares a push-button, labeled `label` (string); `id=` (string) as above. Returns `true` only on the run the host reports it as pressed, `false` otherwise — it does not stay `true` on later re-runs, so `if ui_button("Reset") ... end` reads naturally. Unlike the other UI widgets, it takes no `default=`. |
| `ui_text` | `ui_text(label, [default], [id=])` | Declares a free-text field, labeled `label` (string). `default` (string, positional, default `""`) and `id=` (string) as above. Returns the current (or default) text as a string. |

There is deliberately no `step` row in the table above: a control-systems
step response isn't implemented under any name in this chapter, and `step`
the identifier is already spoken for twice over — it's a reserved keyword
(`to ... step ...` range syntax), so `step(...)` can't even parse as a
function call, and the one runtime function actually named `step` is
`env.step(state, action)`, an unrelated reinforcement-learning method on a
`gridworld_env` model that returns a `{next_state, reward, done}` record.
For a staircase plot of data you already have, see `stair` in the Chart
Types table above.

Three periodic-waveform generators, plotted as ordinary vectors. `triangle`
and `sawtooth` take `(freq, fs, n)` — a frequency, a sample rate, and a
sample count, all scalars — while `sweep`/`chirp` need a start *and* end
frequency, so their positional order is `(f0, f1, fs, n)`. Reach for
`triangle`/`sawtooth`/`square` when you need a fixed-frequency test signal
rich in harmonics (for exercising a filter's whole passband at once), and
for `sweep`/`chirp` when you need the frequency itself to move over time
(for measuring a frequency response by scanning it). In the three resulting
figures, the triangle should look like a symmetric zig-zag, the sawtooth a
one-sided ramp that snaps back, and the sweep a sine whose visible period
visibly shortens as it runs from 10 Hz toward 100 Hz:

```qu
tri = triangle(5, 500, 200)
plot(tri, label = "triangle")
figure()
saw = sawtooth(5, 500, 200)
plot(saw, label = "sawtooth")
figure()
s = sweep(10, 100, 500, 400)
plot(s, label = "sweep")
```

Every `ui_*` call does two things at once: it *declares* a control for a
host (Qu Studio, or an `explore(...)` page) to render, and it *returns* that
control's current value right away — the host isn't required for the script
to run, which is what "headless" means here. That dual nature is why each
one takes a `default` positional argument rather than a `default=` keyword:
outside a host, the default *is* the value, so a script can be developed
and tested from the command line, then handed to a host later without
changing a line. This example declares one of each of the six widget kinds
and prints what each one currently holds, which — run without a host —
should simply echo back the defaults given:

```qu
freq = ui_slider("freq", 1, 20, 5)
gain = ui_number("gain", 1.0)
mode = ui_select("mode", ["fast", "slow"])
enabled = ui_checkbox("enabled", true)
pressed = ui_button("reset")
label = ui_text("label", "wave")
print(freq)
print(gain)
print(mode)
print(enabled)
print(pressed)
print(label)
```
