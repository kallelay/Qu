# Table/DataFrame operations + descriptive statistics benchmark

A real tabular-analysis workflow rather than an isolated kernel: load a
dataset, filter rows, group-by + aggregate, sort, and compute descriptive
statistics/a correlation matrix. Unlike `csv_filters/`/`full_pipeline/`
(where each language regenerates its own copy of the data from the same
seed), this scenario is specifically about `Table`/DataFrame *engines*
(load, index, group, sort), so correctness only means something if all
three load the *exact same bytes* — `generate_data.py` builds ONE shared
CSV, and `bench.qu`/`bench.py`/`bench.m` all read it unmodified.

Run:

```bash
python benchmarks/data_wrangling/generate_data.py   # writes dataset.csv once
qu run benchmarks/data_wrangling/bench.qu
python benchmarks/data_wrangling/bench.py
matlab -batch "run('benchmarks/data_wrangling/bench.m')"
```

## Dataset

1,000,000 rows, 5 columns (`generate_data.py`, seed 42, numpy):

| column | type | distribution |
|---|---|---|
| `category` | text | uniform over 8 groups, `grp_0`..`grp_7` |
| `x1` | float | standard normal |
| `x2` | float | `0.7*x1 + sqrt(1-0.7^2)*noise` — deliberately correlated with `x1` (target 0.7), so the correlation-matrix cross-check has a non-trivial off-diagonal value to verify, not just near-zero numbers that would "match" even if the computation were wrong |
| `x3` | float | uniform(-10, 10) — used for the sort benchmark |
| `x4` | float | exponential(scale=2) — skewed, for a non-Gaussian numeric column |

`dataset.csv` is ~42 MB, gitignored (deterministic from the seed, regenerate
with `generate_data.py` — not committed, same convention as the other
benchmark datasets in this directory).

## Operations benchmarked

| step | Qu | pandas | MATLAB |
|---|---|---|---|
| load | `read_csv(path)` | `pd.read_csv(path)` | `readtable(path)` |
| filter (`x1 > 0`) | `filter(df, df.x1 > 0)` | `df[df.x1 > 0]` | `T(T.x1 > 0, :)` |
| groupby + agg (mean/sum/count of `x2` by `category`) | **`group_by_agg(df, "category", (("x2","mean"), ("x2","sum"), ("x2","count")))`** — one call, one pass over the table (fixed 2026-08-26, see below) | `df.groupby("category")["x2"].agg(["mean","sum","count"])` | `groupsummary(T, "category", {"mean","sum"}, "x2")` (`GroupCount` column comes free) |
| sort by `x3` | `sort_by(df, "x3")` | `df.sort_values("x3")` | `sortrows(T, "x3")` |
| descriptive stats | `describe(df)` | `df[cols].describe()` | manual `mean`/`std`/`min`/`prctile`/`max` (no one-call `describe` on a numeric matrix) |
| correlation matrix | 10x pairwise `corr(x,y)` (no full-matrix builtin — see below) | `df[cols].corr()` | `corrcoef(X)` |

**Update, 2026-08-26**: `group_by_agg` now also accepts a list of
`(value_col, agg)` pairs in one call (`Table::group_by_agg_multi`,
`engine/crates/qu-interp/src/table.rs`) — a genuine single pass over the
table's rows regardless of how many aggregates/columns are requested, not
the old N-full-passes behavior hidden behind one call. The old
single-pair form (`group_by_agg(df, "category", "x2", "mean")`) still
works unchanged. `corr`/`cov` are unaffected and deliberately out of
scope here — still scalar-pairwise only for two arbitrary vectors; only
`group_by_agg`'s Table-wide multi-column story was fixed. `corr_matrix`'s
10 pairwise calls below are still the equivalent full task for that step.

## Results (this machine, 3 trials each — not statistically rigorous)

All times in seconds.

| step | Qu (release) | pandas | MATLAB R2025b |
|---|---:|---:|---:|
| load | 0.58 – 0.64 | 0.59 – 0.77 | 3.67 – 6.48 |
| filter | 0.22 – 0.43 | 0.02 – 0.05 | 0.07 – 0.14 |
| groupby_agg | 0.70 – 1.46 | 0.05 – 0.07 | 0.34 – 0.46 |
| sort_by | 0.35 – 0.58 | 0.25 – 0.28 | 0.11 – 0.22 |
| describe | 0.51 – 0.99 | 0.16 – 0.28 | 0.40 – 0.85 |
| corr_matrix | 1.79 – 2.55 | 0.04 – 0.06 | 0.06 – 0.19 |
| **total** | **4.18 – 6.39** | **1.11 – 1.47** | **4.82 – 8.29** |

These original numbers pre-date the `group_by_agg` multi-agg fix below and
still describe every other step (`corr_matrix` included — unchanged, out
of scope). Re-measured `groupby_agg` alone (this machine, release build,
3 trials, before vs. after the fix, same 1M-row dataset, same 3
aggregates of `x2` by `category`):

| groupby_agg | before (3x single-agg calls) | after (1x multi-agg call) |
|---|---:|---:|
| trial 1 | 0.4819 s | 0.1574 s |
| trial 2 | 0.5315 s | 0.1585 s |
| trial 3 | 0.5083 s | 0.1633 s |
| range | 0.48 – 0.53 s | 0.157 – 0.163 s |

**~3.1-3.3x faster** — consistent with collapsing 3 full re-groupings of
the 1M-row table into 1 (this machine measures noticeably faster overall
than the original session's numbers above for every step, load/filter/sort
included, likely a quieter run — the ~3x groupby_agg ratio, not the
absolute seconds, is the number that reflects the fix). Group aggregates
(mean/sum/count per category) are byte-identical to the pre-fix single-call
results and to pandas' own numbers (see Correctness cross-check below), so
this is a real 3x, not "faster because it computed less." Still slower
than pandas' 0.05-0.07s (pandas' compiled Cython group kernel vs one
Rust `HashMap`-bucketing pass plus interpreter dispatch overhead), but the
structural N-passes gap against pandas'/MATLAB's single-call multi-agg
API — the thing flagged as unfixed in the original write-up below — is
now closed.

**Honest read (original findings, groupby_agg gap now fixed)**: pandas
wins every single step here, often by 10-40x — this
is pandas' home turf (compiled, vectorized C/Cython group/sort/corr
kernels, no per-call re-parsing). Qu beats MATLAB's `readtable` clearly
(MATLAB's own CSV reader is genuinely slow and highly variable — 3.7s to
6.5s across trials on an identical 42 MB file) and is in the same
ballpark on total wall time, but loses to MATLAB on every step *after*
load: `groupby_agg` (now fixed, see above) and especially `corr_matrix`
were Qu's worst gaps (1.8-2.6s for ten scalar `corr(x,y)` calls over
1M-element vectors vs MATLAB's single vectorized `corrcoef`, 0.06-0.19s).
Two real, structural reasons, not just constant-factor slop:

1. ~~**No multi-aggregate/multi-column API.**~~ **Fixed for `group_by_agg`
   2026-08-26** (see above) — it now accepts a list of `(value_col, agg)`
   pairs and computes all of them in one pass. `corr` is unchanged and
   deliberately out of scope: it still only takes one `(x, y)` pair per
   call, so a 4-column correlation matrix still means 10 pairwise calls
   (`to_vec` on `df.x1` etc. also re-clones the whole 8MB column *per
   call*, since a table's numeric field access (`t.col_num(...).to_vec()`)
   always copies) instead of one pass over a single in-memory matrix —
   `corrcoef(X)` (added the same day) covers the "already have a `Mat`"
   case but not "correlate two arbitrary vectors, several pairs at once."
2. **`describe()`'s quantiles likely re-sort each column** (`quantile_of`
   is called three times per numeric column — 25th/50th/75th — each
   plausibly a fresh sort of a 1M-element column); pandas' `describe()`
   does the same three quantiles but off one sorted/partitioned array.
   Not root-caused further this session (would require reading/timing
   `quantile_of` itself, which lives in the same off-limits file).

## Correctness cross-check (not just speed)

Every number below matches across all three languages, printed by each
script's own untimed cross-check section:

- **Row counts**: 1,000,000 total, 500,392 after `x1 > 0` filter — exact
  match, all three.
- **Group aggregates** (`category`, alphabetical `grp_0`..`grp_7` — Qu's
  `group_by_agg` returns first-seen order, so each script's result is
  `sort_by`/`sort_index`/`sortrows`'d by the group key before comparing,
  itself an untimed ~8-row sort): mean/sum/count per group match to 6
  decimal places between Qu and pandas; MATLAB's mean/sum match too. Group
  counts: `125334, 124992, 124876, 124962, 125331, 125120, 124819, 124566`
  (sums to 1,000,000) — identical across all three, **after fixing a real
  bug caught by this cross-check**: the first `bench.m` draft used
  `groupsummary(..., {"mean","sum","nnz"}, "x2")`, and MATLAB's `'nnz'`
  method counts *nonzero* values, not row count — one `x2` value in this
  dataset rounds to exactly `0.000000` at the CSV's 6-decimal precision, so
  `nnz` silently undercounted `grp_6` by 1 (124818 vs the correct 124819,
  total 999,999 instead of 1,000,000). Caught by comparing against pandas'
  `.agg(["count"])`, not by inspection — exactly the kind of "fast because
  it computed something different" trap this cross-check step exists to
  catch. Fixed by using `groupsummary`'s always-present `GroupCount` column
  instead (a true per-group row count, independent of any aggregate
  method) — see the comment in `bench.m`.
- **Sort**: `sort_by(df, "x3")`'s first/last values (`-9.999995` /
  `9.999984`) and monotonicity match exactly across all three.
- **`describe()`/`summary` stats**: count/mean/std/min/max match to 6
  decimals on all four numeric columns across all three languages. The
  25th/75th percentiles match pandas and Qu exactly (both use the same
  linear-interpolation convention) but differ from MATLAB's `prctile` in
  the 6th decimal (e.g. x1's 25th percentile: pandas/Qu `-0.672716` vs
  MATLAB `-0.672718`) — a known, harmless difference in percentile
  interpolation convention (MATLAB's empirical-CDF definition vs pandas'/
  numpy's), not a bug in either.
- **Correlation matrix**: `corr(x1,x2) = 0.700215` in both Qu and pandas
  (MATLAB's `corrcoef` rounds to `0.7002` at its default display
  precision); every off-diagonal near-zero pair (`x1`-`x3`, `x1`-`x4`,
  `x2`-`x3`, `x2`-`x4`, `x3`-`x4`) also matches to 6 decimals across all
  three.

## Matty (4th language) — out of scope, confirmed not just assumed

Ahmed's original ask also wanted Matty (the sibling MATLAB/Octave
interpreter at `../matty` (a sibling checkout), run via
`python matty_runner.py <script>.m`) in the comparison. **It cannot run
this scenario**: Matty has no `table`/`readtable`/`groupsummary`/DataFrame
type at all (confirmed via `grep -rn -i "readtable\|groupsummary\|table"
src/` — nothing beyond an unrelated `symbol_table` method and a `Struct`/
`MCell` type explicitly documented in `TODO.md` as "minimally supported").
Its only CSV reader is `csvread`, a thin `np.loadtxt` wrapper — numeric-only,
no header handling. Verified directly, not just inferred from source: a
probe script calling `csvread('dataset.csv')` fails immediately —
`could not convert string 'category' to float64 at row 0, column 1` — since
`dataset.csv` has both a header row and a text categorical column, either
of which alone would already break `csvread`. No workaround was forced
(e.g. stripping the category column to get *a* number out of Matty) since
that would benchmark a different, easier scenario and misrepresent what
Matty can actually do. Reported honestly as a gap: **Matty has no
Table/DataFrame surface to compare here.**

## Known limitations hit while building this benchmark

- `table(name=values, ...)` (the in-script table constructor) can only
  broadcast a *single* string value per text column, not a genuine per-row
  string vector — irrelevant here since the shared dataset is loaded via
  `read_csv` (which does support real per-row text columns), but worth
  knowing for anyone building a table from scratch in a Qu script.
- Qu has no vector-of-strings value type, so `df.category` (a text column)
  can't be read directly in a script (`eval_field`'s documented behavor) —
  worked around above by sorting the tiny (8-row) grouped-result tables by
  their text column and comparing the numeric aggregate columns
  positionally, and by relying on `describe()`'s fixed, documented stat
  row order (`count, mean, std, min, 25%, 50%, 75%, max`) instead of
  reading its text `stat` column.
- `print(a_table)` only prints a one-line shape summary
  (`table(N rows x M cols: col1, col2, ...)`), not its contents — every
  value check above goes through field access on numeric columns instead.

## Tests

No dedicated `#[ignore]`d regression test (same reasoning as `csv_filters`
— this is a cross-language timing/correctness comparison, not a
single-language algorithm-correctness check with a natural assertion to
automate). The correctness cross-check above is the "did it actually
compute the right thing" verification, confirmed matching (after fixing
the `nnz` bug) across all three scripts' own printed output.
