# Is rayon helping or hurting the elementwise path?

The open question was whether rayon's parallel dispatch helps or hurts the
bandwidth-bound elementwise chain. A previous attempt accidentally measured
thread-spawn cost and was correctly thrown away rather than published.

**The question has no single answer, because the threshold that decides
whether to parallelize never looks at the thing that determines whether
parallelizing pays.** `Matrix`'s `PARALLEL_ELEMENTWISE_THRESHOLD` is keyed on
element count alone (`1 << 20`). The per-element function's cost is what
actually decides, and it varies by ~6x across ops that all take the same
branch.

Everything here is measurement, not reasoning about the source. Re-run it
with `powershell -File driver.ps1`.

## Answers

**1. For an expensive per-element function, rayon is a clear win.** `sin` on a
4,000,000-element matrix, 1 thread vs 16 with the container held fixed:
**5.6x faster wall for 1.4x the CPU**. That is real parallel work and the
current behaviour is right.

**2. For a cheap one, it buys little and costs a lot.** `abs` on the same
data, same 1-vs-16 comparison: **1.7x faster wall for 4.3x the CPU**, and the
1.7x is already available at 4 threads for 1.2x. Wall time bottoms out at 4
threads and gets *worse* from 8 to 16 while CPU keeps climbing — threads past
4 are stalling rather than computing. That pattern is what a memory-bandwidth
limit looks like, but note this is the *explanation it is consistent with*,
not a measured fact: I never measured this machine's streaming ceiling, so
cache-line or allocator effects are not excluded. What is measured is that
`abs` stops scaling at 4 threads and costs 4.3x the CPU to reach 16. On a
laptop or a shared machine that is a bad trade; on an idle workstation it is
a mild win.

| threads | `sin` wall | `abs` wall | `sin` CPU | `abs` CPU |
|---|---|---|---|---|
| 1 | 29.97 | 4.91 | 28.65 | 5.21 |
| 2 | 16.50 | 3.29 | 29.69 | 5.21 |
| 4 | 10.02 | **3.28** | 36.20 | 6.25 |
| 8 | 6.67 | **2.90** | 44.27 | 10.42 |
| 16 | **5.39** | 2.92 | 39.06 | 22.40 |

ms per rep, 4,000,000 elements, baseline subtracted. `sin` keeps scaling to
16; `abs` is done at 4 and pays 4.3x the CPU to get there.

**3. For `.*` the rayon question is the wrong question.** `Matrix::broadcast`
recomputes a broadcast-aware index per element (`lr + lc * self.rows`, with
per-element row/col collapse checks). That costs more than 16 threads can win
back. Identical arithmetic on identical data, `v .* w` with two distinct
4,000,000-element operands:

| | wall | CPU | effective |
|---|---|---|---|
| `Vec`, serial zip, 1 thread | **5.57 ms** | 6.25 ms | **16.0 GB/s** |
| `Mat`, `broadcast`, 16 threads | 12.50 ms | 34.11 ms | 7.2 GB/s |

**One serial thread sustains 16 GB/s; sixteen rayon threads manage 7.2 GB/s.**
The matrix path spends 5.5x the CPU to run 2.2x slower. Since a single thread
already exceeds what the parallel path achieves, memory bandwidth is *not*
the limiter of the `Mat` path — the per-element index arithmetic is. Tuning
the rayon threshold for `.*` would be tuning the wrong thing.

This rules out bandwidth as the limiter; it does **not** establish 16.0 GB/s
as the machine's streaming ceiling, since the `Vec` zip may itself be limited
by something other than DRAM. If this is ever cited to set a target for a
fixed `broadcast`, the supportable target is *"at least the `Vec` zip"* —
5.57 ms on 4M elements — not *"16 GB/s"*.

**The GB/s figures above count USEFUL bytes, and that convention matters the
moment they sit beside anyone else's.** A store costs the memory controller
twice: a read-for-ownership to fetch the line, then a writeback. Counting
what DRAM actually moves:

| | useful | DRAM traffic |
|---|---|---|
| `v .* w` (2 reads + 1 write) | 16.1 GB/s | **21.4 GB/s** |
| `abs(v)` (1 read + 1 write) | 11.7 GB/s | **17.6 GB/s** |

A separate roofline probe in this repo measured **44.34 GB/s single-thread**,
but on a **read-only** stream — which has no read-for-ownership and no
writeback, so for it the two conventions coincide. Comparing it to the
`useful` column understates these ops by ~1.3x on top of a genuine access-
pattern difference. Against the DRAM column the gap is ~2x, which is the
ordinary penalty for a store-heavy pattern rather than evidence of headroom;
and the two rows above agreeing with each other is what you would expect if
both already sit at that pattern's limit. **Neither number establishes the
other's ceiling. Do not put them in the same table without saying which
convention each uses.**

**4. Only `Mat` is ever parallelized, and only above a size; `Vec`, `Signal`
and every complex type never are.** The accurate framing is *"`Vec` never,
`Mat` above `1<<20`"* — a small `Mat` behaves exactly like a `Vec`.
`qu-interp`'s `map1`/`map2` run `Vec` and `Signal` through a plain serial
iterator with no threshold and no rayon; only `Value::Mat` reaches
`Matrix::map` / `Matrix::broadcast`. So the 5.6x on `sin` is unreachable for
a vector:

```
sin(x), 4,000,000 elements     Vec 29.65 ms      Mat 5.58 ms
```

Same numbers, same call, one `reshape` apart. `Signal` — the type the
signal-processing domain actually uses — is in the serial half.

**`map1_complex` has no parallel path at all**, and this is wider than the
`Vec` gap: both its `CVec` arm (`zs.iter()`) and its **`CMat`** arm
(`m.as_slice().iter()`) are plain serial iterators, so a complex *matrix*
gets nothing where a real matrix of the same size gets rayon. `exp` on a
2000x2000, complex ÷ real:

```
1 thread  (both serial, so this is the INTRINSIC cost of complex exp)   4.07x
16 threads (real parallelizes, complex cannot)                         14.13x
```

Complex `exp` really is ~3 transcendentals against 1, and holding threads
fixed shows that accounts for 4.07x of it. The remaining **3.5x is purely
the missing parallelism**. FFT output, phasors and impedance spectra are all
`CVec`/`CMat`, so this lands squarely on Qu's own domain.

Whether any of it is worth changing depends on answers 1-3: worth it for
`sin`-like functions, not worth it for `abs`-like ones, and not by routing
through `broadcast`.

**Consequence for the existing suite, and it cuts against a number I
published.** `matty_suite`'s `element_wise_ops` uses 10000x10000 matrices and
`bench_fair`'s uses 3000x3000 = 9M — both far above `1<<20`, both on the
parallel path. So the suite's headline elementwise figure, and the
decomposition attributing **87% of the Qu-vs-MATLAB gap** to
`element_wise_ops`, describe the *fast* path. A bracket literal collapses to
an orientation-free `Vec`, so the shape most user scripts actually write is
never measured. **Read 87% as a floor for typical code, not a ceiling.**

**5. Chunk geometry is *not* a problem, contrary to what the source
suggests.** `broadcast` parallelizes over columns via `par_chunks_mut(rows)`,
handing rayon `cols` tasks. `matmul` guards exactly this shape with
`n >= rayon::current_num_threads()` and its doc explains why ("too few to
keep a live thread count busy"); `broadcast` has no such guard, so a
`2 x 1000000` matrix becomes a million 2-element tasks and a `1000000 x 2`
becomes two enormous ones. **I predicted both would be pathological and I was
wrong.** All five shapes of the same 4,000,000 elements measure flat:

```
4 x 1000000   0.0116 s      100 x 40000  0.0124 s      2000 x 2000  0.0119 s
40000 x 100   0.0115 s      1000000 x 4  0.0123 s
```

Rayon's `par_chunks_mut` is an *indexed* parallel iterator, so it rebalances
by recursive bisection instead of dispatching per chunk. The missing guard is
a real asymmetry with `matmul`, but it costs nothing measurable.

## Why you should believe the numbers

**The machine is shared and was never quiet.** Four other sessions build and
test in this checkout; load ranged 34-100% with up to 26 concurrent
`rustc`/`cargo` processes. Three things make the numbers reproduce anyway,
and all three are load-bearing — dropping any one brought back tables that
did not replicate:

- **Baseline subtraction.** Every probe has a twin that builds the same data
  and runs the same empty loop, removing startup and `randn` cost.
- **Alternating pass order**, so drift cannot masquerade as a thread effect.
- **Minimum over passes**, the pass least contaminated by other work.

**Replication is the gate, and it caught a false result.** An earlier
thread-sweep table looked clean, coherent and monotonic — `sin` scaling to
6.5x at 16 threads. It did not replicate: the second pass peaked at 3.48x and
then fell. That table was luck, and a single coherent-looking run would have
been published as fact. The tables above are the ones that survived
replication, and two independent methodologies agree on them (in-process
interleaved timing: 5.5x / 1.45x / 2.64x; cross-process baseline-subtracted:
5.31x / 1.52x / 2.7x).

**`RAYON_NUM_THREADS` demonstrably takes effect** — not assumed. CPU-seconds
per wall-second on a fixed workload rises 0.78 / 1.24 / 1.56 / 3.02 / 3.76
across 1 / 2 / 4 / 8 / 16 threads.

**Block-designed A/B benchmarks give wrong answers on this machine, and mine
did.** Timing all of variant A then all of variant B charges any drift
entirely to whichever ran second. Three consecutive identical runs here gave
`abs` = 0.0092, 0.0047, 0.0034 s — a block design reports that 2.7x drift as
a speedup. My first version measured `sin` Mat-vs-Vec at 1.68x that way; the
interleaved version measures the same quantity at 5.5x. `interleaved.qu` is
the corrected in-process design.

**What is still provisional.** The wall figures come from runs at 44-88% load
that replicated across independent passes; the CPU figures are far more
robust, since a descheduled thread accrues no CPU time. At 100% load with 26
build processes the wall numbers collapse entirely — the same driver then
reports `sin` as "Vec 1.2x" where the quieter runs agree on "Mat 5.3x". The
structural findings (4 and 5) and the CPU columns are unaffected. A re-run on
a genuinely idle machine would tighten the wall magnitudes; it will not move
the directions, which held across every run.

`TotalProcessorTime` has 15.625 ms granularity on Windows, which over 60 reps
quantises to ~0.26 ms/rep. Fine against values of 4-48 ms — do not read the
third decimal.

## Files

| file | what it measures |
|---|---|
| `driver.ps1` | the harness: baseline subtraction, alternating passes, min-of-N |
| `interleaved.qu` | `Vec` vs `Mat`, interleaved in-process, same data reshaped |
| `mat_threads.qu` | thread sweep with the container held fixed at `Mat` |
| `shape_sweep.qu` | five shapes, one element count — the null result in 5 |
| `type_probe.qu` | which `randn` shapes are `Vec` and which are `Mat` |
| `cvec_probe.qu` | complex vs real `exp`, at 1 and 16 threads, to split intrinsic cost from missing parallelism |
| `cpu_*.qu` | one op per process so CPU time can be attributed; `*baseline*` are the empty twins |

Nothing outside `benchmarks/` was modified. Findings 1-5 are for whoever owns
`qu-core/src/matrix.rs` and `qu-interp`'s `map1`/`map2`.
