# Design: real-time serial plotting for signal/impedance acquisition

**Status:** proposed, pending confirmation before implementation.
**Supersedes/replaces (for Ahmed's own use):** `old_SeriPlot.py`, a 950-line
PyQt6 + matplotlib application built for the ColorfulFlower v1.1 STM32
firmware (Goertzel/FFT signal mode + impedance-sweep mode).

## 1. What the existing tool actually does (read in full before designing)

Reverse-engineered from `old_SeriPlot.py`, since the WIRE PROTOCOL is a
hardware/firmware constraint we must preserve exactly, not redesign:

- A background thread reads the serial port line-by-line (ASCII,
  newline-terminated), splits each line on `,`, and tries to parse every
  field as a float. A line that doesn't parse cleanly (a startup banner,
  etc.) is silently skipped.
- **Mode is inferred purely from field count** — no explicit mode byte:
  - **2 fields** → "signal" mode: `(adc1, adc2)` raw time-domain samples.
    Frames accumulate until 2048 samples, then the whole frame is emitted.
  - **5 fields** → "impedance" mode: `(idx, mag, phase, accMag, accPhase)`
    — one row per swept frequency point. `idx` resets to `0` at the start
    of a new sweep; seeing `idx==0` while a sweep is already buffered means
    "the previous sweep just ended," so that's the emit-and-reset trigger.
- A bounded rolling history (`deque(maxlen=15)`) of complete frames/sweeps
  is kept, independent of the plot currently on screen.
- Two display modes: **Real-time** (only the latest frame/sweep) and
  **Fading** (the last ~10 overlaid, older ones drawn more transparent).
- Seven plot views for impedance data: Nyquist, Bode magnitude, Bode
  phase, Re(Z) vs frequency index, Im(Z) vs frequency index, and two
  combined multi-panel layouts (Nyquist+Bode, Nyquist+Re/Im).
- A frequency-index filter, entered as a compact range string
  (`"0-5,8,10-19"` to keep only those indices, `"skip:3,7"` to exclude
  them) — useful for dropping known-bad frequency points from a sweep
  before plotting.
- Raw-line recording to a timestamped CSV (every incoming line, as it
  arrives) and a separate "save current buffer" export.
- Pause (stops replotting, data keeps buffering underneath), auto-scale
  toggle (freezes pan/zoom across replots when off), a scrolling raw
  console capped at 5000 lines.

This is a real, thoughtfully-built tool — the "far better" version needs
to earn that claim, not just be a reskin in a different language.

## 2. Where a Qu-native version can genuinely be better, not just different

1. **Live Kramers-Kronig validation of every sweep, for free.** Qu already
   has Ahmed's own rLKK method (`rlkk_validate`/`rlkk_reconstruct`,
   shipped 2026-08-23) — SeriPlot has *no* validation at all, it just
   plots whatever came in. Running `rlkk_validate` on every completed
   impedance sweep and flagging/highlighting anomalous points on the
   Nyquist plot in real time is a real capability improvement specific to
   this exact domain, not available by porting the Python tool as-is.
2. **~120-150 lines instead of 950**, because Qu's serial/plotting/DSP
   primitives sit at a higher level than raw PyQt widget wiring — the
   whole app is close to what SeriPlot's `_on_impedance_sweep` callback
   alone does, not a full GUI framework's worth of boilerplate.
3. **The rolling history buffer IS `fifo(15)`**, a real primitive being
   built this session — not a hand-rolled `deque(maxlen=...)`.
4. **Mode dispatch via multiple dispatch**, not an `if n_fields == 2 elif
   n_fields == 5` chain — a "signal frame" and an "impedance sweep" are
   different `record<Tag>`-refined shapes, and separate typed functions
   handle each, which reads as intent rather than a field-count guess.
5. **No GUI framework dependency at all** — cross-platform for free,
   no PyQt packaging, and the whole thing is inspectable/editable as a
   plain script rather than requiring Qt Designer knowledge to modify.

## 3. Honest constraint: Qu's plotting model is not a live-updating canvas

SeriPlot's matplotlib `FigureCanvas` redraws in place, in a GUI event
loop. Qu's plotting model (confirmed via direct testing this session) is
render-to-file (`savefig`) — there is no persistent, in-place-updating
figure object. Two honest ways to get a "live" feel without overclaiming
a capability Qu doesn't have:

- **(a) File-watch + re-render loop**: the acquisition script calls
  `savefig("live.svg")` after every processed frame/sweep; a viewer (a
  browser tab, or QuStudio's own Figures panel) uses `watch file(path)`
  (a primitive being built this session) to detect the new render and
  reload it. Simple, uses only what's real today.
- **(b) QuStudio's Interactive Mode**: built for slider-driven re-runs of
  a whole script, not a long-running process pushing incremental updates
  — not a natural fit for this without real changes to that feature, so
  **not** the v1 approach; noted as a possible future integration, not
  promised here.

**This spec commits to (a).** The acquisition script is a plain, honest
`qu run` process; "live" means "re-rendered on every new frame/sweep, and
picked up by a watcher within one poll interval," not true continuous
redraw. Stated plainly so nobody is surprised later.

## 4. Proposed architecture

```qu
# --- protocol layer -------------------------------------------------
port = serial_open("COM5", 115200, timeout_ms=200)

# a signal frame: {__type="SignalFrame", adc1: vec, adc2: vec}
# an impedance sweep: {__type="ImpedanceSweep", idx: vec, mag: vec,
#                       phase: vec, acc_mag: vec, acc_phase: vec}

function parse_line(line: str)
    fields = split(line, ",")
    n = length(fields)
    if n == 2
        return {__type="SignalSample", adc1=cast(fields[0], num), adc2=cast(fields[1], num)}
    else if n == 5
        return {__type="ImpedancePoint",
                idx=cast(fields[0], num), mag=cast(fields[1], num), phase=cast(fields[2], num),
                acc_mag=cast(fields[3], num), acc_phase=cast(fields[4], num)}
    else
        return none   # startup banner / garbage line -- skip
    end if
end function

# --- accumulation, dispatched by tag, not an if/elif field-count guess
function accumulate(sample: record<SignalSample>, state) ... end function
function accumulate(point: record<ImpedancePoint>, state)  ... end function

# --- rolling history -------------------------------------------------
signal_history = fifo(15)
impedance_history = fifo(15)

# --- the pump loop -----------------------------------------------------
# Qu is single-threaded/synchronous (confirmed this session) -- there is
# no background-thread model the way SeriPlot's QThread has. The
# equivalent honest model: serial_open's own timeout_ms already makes
# .read_line() a bounded, non-hanging call per iteration, so a plain
# loop IS the real "pump":
while true
    line = port.read_line()
    if line == "" : continue end if
    sample = parse_line(line)
    if sample == none : continue end if
    accumulate(sample, ...)   # dispatches by record<Tag>
    # on frame/sweep completion (handled inside accumulate via a
    # returned "complete" flag, or a callback -- exact mechanism to be
    # finalized against whatever accumulate ultimately returns):
    #   - push the completed frame/sweep into its fifo
    #   - if impedance: run rlkk_validate, flag anomalies
    #   - render the current plot view to live.svg via savefig
end while
```

Frequency-index filtering reuses real Qu vector/mask operations
(`filter`, boolean masks) instead of hand-rolled range-string parsing
logic for the FILTERING itself — the compact `"0-5,8,10-19"`/`"skip:3,7"`
input syntax is worth keeping as a small parsing utility (real UX value,
independent of the plotting/DSP layer).

Recording is just `write_csv`/append-mode file writes on the raw parsed
values, timestamped — no new primitive needed, everything here already
exists.

## 5. Open questions before implementation (need Ahmed's call)

1. **Plot views**: build all 7 of SeriPlot's views, or start with the
   ones actually used day-to-day (Nyquist + Bode magnitude/phase cover
   most impedance-spectroscopy workflows) and add the rest if wanted?
2. **rLKK integration**: validate every sweep automatically, or make it
   opt-in (a flag/argument) in case it's not always wanted (e.g. slows
   down a fast acquisition loop, or isn't meaningful for every dataset)?
3. **"Fading" multi-sweep overlay**: real value for this, or is
   real-time-only sufficient for a v1?
4. **Live-refresh viewer**: a plain browser tab polling `live.svg` via
   `watch file`, or wire this into QuStudio's Figures panel specifically?

## 6. Sequencing

This depends on the serial-port primitive (`serial_open`/`.read_line()`)
and `fifo()` currently being built in parallel this session, plus the
already-existing `rlkk_validate`, `cast()`, multiple dispatch, and
plotting. Implementation starts once those land and the open questions
above are answered.
