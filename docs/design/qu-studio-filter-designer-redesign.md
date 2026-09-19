# Plan: Qu Studio Filter Designer redesign

## 0. Verified ground truth

Read against master (`beee3f96`), not assumed:

- `DspWorkbenchPanel.tsx:206` — `useState<'spectrum'|'filter'>('spectrum')`; `filterScript()` at :192 hardcodes `butter`; the code snippet is rendered at :425 as a `<div>` of monospace text; live re-run is a 350 ms debounce at :366.
- Engine builtins already live in `engine/crates/qu-interp/src/lib.rs`: `butter` (24994), `ellip` (25035), `cheby1` (25077), `cheby2` (25117), `fir1` (25165), `firls` (25215), `sosfilt`, `filtfilt`, `freqz` (25367), `group_delay` (25379), `fvtool` (25392), `filter_ba`/`lfilter` (25270), `filter_init`/`filter_next` (25580/25608), **`poles` (25656), `is_stable` (25665), `pzplot` (25675)**.
- `qu-core/src/filter.rs` already has `sos_poles`, `sos_zeros`, `fir_poles`, `fir_zeros`, `is_stable`, `polynomial_roots`, `fir_group_delay`, `group_delay`. More exists than the brief assumed — pole/zero and stability analysis need **zero** new math.
- Filter models: IIR → `{sos: Matrix, fs}`; FIR → `{b: Vec, fs}`. No `a`/`b` pair for IIR. Confirmed.
- `filter_ba` already prints a conditioning warning above 9 coefficients (lib.rs:25292) — *the precedent for the `[a,b]` caveat already exists in this codebase and should be reused verbatim in tone.*
- Plots arrive as `data:image/svg+xml;base64,...`; `FigureViewer` already inlines SVG (`decodeSvgDataUri`). So an SVG string is a first-class citizen in this UI.
- Peer work (`diagram.rs`): 764 lines, everything `pub(crate)`; `render_pipeline_svg` (:198), `render_algorigram_svg` (:509), `arrow_ops` (:654), palette `INK #333333` / `FILL #eef3fb` / `DECISION_FILL #fdf1df` (:545-547).

## 1. Brainstorm: what "graph theory" plausibly means

Three concrete directions, all defensible:

**Direction A — the design provenance graph.** Four-to-five boxes, left to right, replacing the code snippet in its exact slot: `Spec (LP, fc=100 Hz, fs=1 kHz)` → `Design (Butterworth, order 16)` → `SOS cascade (8 sections)` → `Response (mag/phase/GD/impulse)`. Each node is live-labelled from the controls; clicking a node focuses/scrolls to the controls that produced it. Structurally this *is* a pipeline.

**Direction B — the signal-flow graph.** The real graph-theory object in DSP: the biquad cascade drawn as boxes `H₁(z) → H₂(z) → …`, each expandable into a direct-form-II SFG with `z⁻¹` delay nodes, gain triangles and summing junctions. This is what MATLAB's "Filter Structure" view shows. For FIR it degenerates into the tapped-delay-line graph. Honest downside: at order 16 it is 8 sections; at order 20, 10 — it needs a level-of-detail collapse (`×8 sections`, expand on click).

**Direction C — pole-zero graph on the unit circle.** Nodes = poles/zeros, the geometry that *causes* the response. Not really box-and-arrow, and `pzplot` already draws it — so this is a panel, not the diagram.

**Recommendation: A as the default, B as an expandable second tier, C as a report panel.** A is the direct answer to Ahmed's actual complaint ("I don't want to read `filt = butter(16,...)`"): it occupies the same screen real estate, carries the same information, and is readable at a glance. B is the thing that earns the phrase "graph theory" literally, and is the natural place to go deeper once A ships. Ship A in slice 1; B in slice 3.

## 2. Coordination with Qu-Pipeline: the decision

**Take option 2 — shared visual language, separate implementation — and make the split explicit rather than accidental.**

Reasoning, given their constraint that `diagram.rs` is `pub(crate)` and reachable only by shelling `qu.exe`:

1. **Latency.** The panel redraws on a 350 ms debounce as you type. Routing the diagram through `qu.exe` adds a process spawn per keystroke *for the part of the UI that must feel instant*. The figure can afford that (it's real math); a four-box diagram cannot justify it.
2. **Interactivity.** A returned SVG string is opaque. Direction A's value is clicking a node to jump to its controls, hovering `SOS cascade` to see section count, showing an error state on the `Design` node when `cheby2` rejects a cutoff. That requires React-owned DOM.
3. **Lossy round-trip.** Expressing the design as `|>` syntax means encoding `window="kaiser", beta=6.0, ripple_db=1.0, atten_db=60` into a chain whose renderer only knows how to label expressions. The label would degrade to the very source text Ahmed asked to stop seeing.
4. **Their own read is right.** A recursive branching flowchart layout is the wrong engine for a 4-node straight line.

**But do not fork the look.** Concretely:
- Add `qu-ui-components/src/utils/diagramTheme.ts` holding `INK`/`FILL`/`DECISION_FILL`/stroke widths/corner radii/arrowhead geometry, with a header comment naming `engine/crates/qu-interp/src/diagram.rs:545` as the source of truth, and a reciprocal comment added there pointing back. Two files, one contract, both annotated. This is cheap and it is what keeps the two surfaces from drifting.
- Record the contract in a short "Diagram visual language" section of this doc so a third consumer has somewhere to look.

**Future convergence (slice 3+, not now):** once `diagram.rs` lands on master, add a Rust-side `diagram_filter(filt)` builtin for the *headless* path — script use, PDF/report export, `qu` from the CLI. The interactive path stays TS. That is a deliberate two-surface split for two genuinely different requirements (interactive vs. exportable), documented as such, not duplication by neglect.

**Addendum: `engine/crates/qu-interp/src/graph.rs` exists and is not a shortcut here.** It's a real node/edge graph *data structure* (`graph()`, weighted `add_edge`, Dijkstra `shortest_path`) — explicitly "a real node+edge graph structure, not a chart," with no layout or rendering at all. It doesn't provide a third pre-existing renderer, so the §2 recommendation stands unchanged. It IS, however, the natural home for a future scriptable filter-structure representation — e.g. `filter_graph(filt)` returning a `graph` of biquad sections a script could traverse or hand to `diagram_filter()`. Slice-4+, only worth doing if Direction B is chosen; noted so it isn't rediscovered later.

## 3. New controls, and how they map to real builtins

Left rail, top to bottom, with conditional fields:

| Control | Values | Maps to |
|---|---|---|
| Implementation | IIR / FIR | picks the builtin family below |
| Family (IIR) | Butterworth / Chebyshev I / Chebyshev II / Elliptic | `butter` / `cheby1` / `cheby2` / `ellip` |
| Method (FIR) | Windowed sinc / Least squares | `fir1` / `firls` |
| Band type | low / high / band / stop | 2nd (or 3rd/4th) positional arg; `kind=` kwarg for `fir1` |
| Order (IIR) | 1–20 | positional arg 0 |
| Taps N (FIR) | 3–255 | `n = N-1`; `firls` requires **even** `n` |
| Window (FIR, fir1 only) | Hamming / Hann / Blackman / Kaiser | `window=` kwarg |
| Kaiser β | float, shown only for Kaiser | `beta=` kwarg (**required** — `fir1` errors without it, lib.rs:25187) |
| Passband ripple dB | shown for Cheby I, Elliptic | `ripple_db` positional |
| Stopband atten dB | shown for Cheby II, Elliptic | `atten_db` positional |
| f1 / f2, fs | Hz | as today |

Two engine asymmetries the UI must handle, and both are worth fixing upstream:

- **`fir1` defaults `fs=2.0`** (normalized). The generated script must always pass `fs=` explicitly so the Hz controls mean the same thing across IIR and FIR.
- **`firls` has no `fs` kwarg at all** — `fs` is hardcoded `2.0` (lib.rs:25224) and band edges are normalized `0..1`. The UI must convert `f/(fs/2)` itself, *and* the returned model carries `fs: 2.0`, which then makes `fvtool`'s frequency axis wrong (`fvtool` reads `fs` off the model, lib.rs:25396). **This is a real bug for the FIR-LS path, not just a UI inconvenience.** Recommend a small engine fix: give `firls` the same `fs=` kwarg as `fir1`. ~15 lines. Do it before wiring FIR-LS into the UI, or omit FIR-LS from slice 1.

Also note `cheby1`/`cheby2` skip `check_cutoff_below_nyquist` (only `butter`/`ellip`/`fir1` call it) — minor inconsistency worth a follow-up.

**The generated Qu source does not go away** — it is still the execution mechanism, and it has export value. It moves from "the thing you stare at" to a collapsed `Show Qu source ▸` disclosure with copy-to-clipboard and "Open in editor". Ahmed said he doesn't want the user *looking* at it, not that it must be unreachable.

## 4. New analysis outputs

### 4.1 "Lag time"

DSP-correct reading: **group delay expressed in time, reported as scalars**, not a curve. `fvtool`'s Group Delay panel already plots τ(f) in samples across all frequencies — useful for shape, useless for answering "how late is my measurement?". Concretely report:

- τ at DC (or at passband centre for BP/BS), in **ms**: `gd/fs*1000`.
- τ_max over the passband, and **passband spread** `τ_max − τ_min` (the phase-linearity number).
- For FIR symmetric designs, note it is exactly `n/2` samples and flat — a nice teaching signal.
- A one-line note that `filtfilt` gives zero phase lag at double the effective order.

**Math exists.** `repr_group_delay` is already called by `fvtool`. Slice 1 needs no engine work (compute in the generated script from `group_delay(filt)`). A convenience builtin `lag_time(filt, f)` is ~30 lines if wanted later.

### 4.2 "Response time"

Reading: **step-response timing**. Nothing computes this today. Report rise time (10→90 %), settling time (first entry into ±2 % of final value with no later exit), and percent overshoot.

Buildable *today* with no engine change: `y = sosfilt(filt, ones(N))`, then index arithmetic in Qu. The only subtlety is picking `N` — derive it from max group delay (e.g. `N = clamp(20*τ_max, 256, 8192)`), and mark the result "did not settle within N" rather than reporting a wrong number.

Engine work if promoted to builtins: `stepz(filt, n)` plus `step_info(filt)` returning a model with `rise_time`/`settling_time`/`overshoot`/`final_value` — ~100 lines plus tests and `builtin_docs.rs` entries. Recommend slice 2, after the script version proves the definitions.

### 4.3 `[a, b]` coefficients — the real design decision

**Do not paper over this.** The tension, stated plainly:

Qu deliberately stores IIR filters as SOS because a 16th-order direct-form denominator has coefficients spanning many orders of magnitude and cannot represent its own poles. `toolkit-signal.md` §4 says so; `filter_ba` already prints a runtime warning above order 8; scipy's own docs carry the same warning. Exposing `[a,b]` hands the user a representation that is *correct on paper and wrong in floating point* at exactly the orders this panel's slider goes up to.

Three options, in order of preference:

1. **Add `sos2tf(filt)` as an explicit builtin, returning a model `{b, a}`** (Qu has no multi-return; the model-return convention is already established by `findpeaks` etc.). The math is polynomial convolution across sections — ~25 lines of Rust. **And ship it with a measured conditioning number, not a vibe:** re-evaluate `|H(f)|` from `(b,a)` via `filter_ba` on an impulse and compare against `freqz(filt)`, reporting `max |ΔdB|`. That converts the caveat from folklore into a number the user can look at: "transfer-function form deviates by up to 0.03 dB from the SOS design" is trustworthy; "may be inaccurate at high order" is noise. This is cheap and it is the recommendation.
2. Cap the panel: refuse to show `[a,b]` above order 8. Safe, but paternalistic and inconsistent with `filter_ba`'s existing "a warning, not a guard" philosophy (its own comment says the ported script is usually right about what it wants).
3. Do nothing. Leaves requirement 4 unmet.

**Explicitly do NOT add `.a`/`.b` as lazy fields on the IIR filter model.** That would make the fragile representation look exactly as blessed as `sos`, which is the one thing this codebase has been careful not to do. A named function call is an opt-in; a field is an endorsement.

UI: a collapsed `Transfer function [b, a] ▸` section. Above order 8 it opens with the deviation figure and the `filter_ba`-style warning text, reused near-verbatim for consistency of voice.

### 4.4 "More advanced information" — brainstorm, then a tier

What fdatool / scipy / a measurement engineer actually want:

*Computable today* (from `freqz` / `group_delay` / `sosfilt` / `poles` / `is_stable`, all shipping):
- Stability: `is_stable(filt)`, plus **stability margin** = `1 − max|p|` (how close the worst pole sits to the unit circle). Free — `poles` exists.
- Measured passband ripple (dB peak-to-peak inside the passband) and measured stopband attenuation (min |H| dB beyond the stopband edge) — *achieved*, versus *requested*. The gap between the two is the single most useful thing a filter tool can tell you.
- Actual −3 dB frequency vs. the requested cutoff (they differ for cheby1/cheby2/ellip by definition — the docs at lib.rs:25028 already explain why; surfacing it teaches the user).
- Transition width, DC gain, Nyquist gain.
- Group-delay flatness / phase-linearity metric (§4.1).
- Step settling / rise / overshoot (§4.2).
- Pole-zero plot — `pzplot(filt)` already exists; just call it as a second figure.
- Impulse-response effective length (samples until |h| < 1e-4 of peak).

*Needs new engine work:*
- **Minimum-order estimators** `buttord`/`cheb1ord`/`cheb2ord`/`ellipord` — "you asked for order 16; order 7 meets your spec." Genuinely the highest-value missing feature for a design tool. Medium: ~150 lines each of real analog-prototype math plus tests.
- Fixed-point coefficient-quantization preview (16-bit response overlay). The measurement-science angle Ahmed would like; larger, slice 4+.
- Noise gain / ENBW.

**Recommended tier-1 "Filter report" card** (all free math, ships in slice 1–2): stability + margin, measured passband ripple, measured stopband attenuation, actual −3 dB point vs requested, transition width, DC gain, lag time at DC + passband spread, settling time, rise time, overshoot. Tier-2: `*ord()` estimators, quantization.

### 4.5 Plumbing for the report

The figure already comes back through `execute_code`'s `plots`. The scalars need a channel. `ExecuteResult.output` is a plain string. For slice 1, have the generated script `print` one delimited machine-readable line (`#QUREPORT{...}`) and parse it in TS — zero engine work, ugly but contained and easy to delete. For slice 2+, prefer a typed Tauri command `design_filter(spec) -> FilterReport` so the frontend stops string-parsing. Name this as planned debt in the code comment, so it is not mistaken for the intended end state.

## 5. Default tab

`useState<'spectrum'|'filter'>('filter')` — one token. But the honest UX consequence: a Spectrum-first user now pays a click every launch, and the argument just inverts.

**Recommendation: persist last-used, seeded to `'filter'`**, following the existing `GuiDesignerPanel.tsx:53` localStorage pattern (`localStorage.getItem(...)` in a lazy `useState` initializer), key `qu.dsp.mode`. First launch = Filter Designer (Ahmed's ask); after that the panel remembers. Also **reorder the tab buttons** so Filter Designer is first — DOM/tab order should match the default, or keyboard and screen-reader order contradict the visual emphasis.

## 6. Sequencing

- **Slice 1 (no engine work):** default tab + persistence + reorder; Direction-A diagram replacing the snippet (hand-rolled SVG in `qu-ui-components`, `diagramTheme.ts` mirroring `diagram.rs`); IIR/FIR toggle, family, window (+Kaiser β), ripple/atten controls; `Show Qu source ▸` disclosure. FIR-LS held back pending the `firls` `fs` fix.
- **Slice 2:** tier-1 report via generated script + `#QUREPORT` parsing — lag time, response time, ripple/atten achieved, stability margin, −3 dB verification. Add `pzplot` as a second figure.
- **Slice 3 (engine):** `sos2tf` + measured-deviation number; `stepz`/`step_info`; `firls` `fs=` kwarg; `cheby1`/`cheby2` Nyquist check; typed `design_filter` Tauri command replacing the marker parsing.
- **Slice 4:** Direction-B structure/SFG view with LOD collapse; `*ord()` estimators; optional `diagram_filter()` Rust builtin for export once `diagram.rs` is on master.

**Do not block on Qu-Pipeline.** The only dependency taken is their palette and arrow geometry — stable enough to mirror with a cross-reference comment today. Nothing in slices 1–3 is invalidated by whatever shape their builtins land in.

## 7. Open questions — Ahmed's call

1. Diagram direction: A alone, or A now with B committed for slice 4?
2. Is the collapsed `Show Qu source` disclosure acceptable, or should the generated code be entirely absent from this panel?
3. Exact tier-1 report field list (§4.4) — which of the eleven earn their space?
4. `[a,b]` policy: **option 1** (always show, with the measured dB-deviation number) as recommended, or option 2 (cap above order 8)?
5. Tolerance conventions: settling ±2 % or ±5 %; rise 10–90 % or 5–95 %?
6. Report placement: left rail below the controls, or a third column beside the figure?
7. Should a finished design be exportable into the editor as Qu source ("Open in editor")?
8. Are the `firls` `fs` bug and the `cheby1`/`cheby2` missing Nyquist check in scope here, or separate fixes?

## Critical files for implementation

- `qu-studio-tauri/src/DspWorkbenchPanel.tsx`
- `engine/crates/qu-interp/src/lib.rs` (builtin dispatch, lines ~24985–25700)
- `qu-core/src/filter.rs`
- `qu-ui-components/src/components/FigureViewer.tsx`
- `engine/crates/qu-interp/src/diagram.rs` (visual-language reference, lines 545–720)
