//! A columnar data table (`Value::Table`) — Qu's DataFrame primitive.
//!
//! Storage is column-major (`Vec<(String, Column)>`), matching every fast
//! DataFrame implementation's own reasoning: analytical operations (filter,
//! aggregate, select) touch whole columns, so keeping each column's values
//! contiguous is what makes those operations cheap in the first place.
//!
//! This is deliberately an *eager* table, not a lazy query-planned one: every
//! operation runs immediately and returns a new `Table`, the same way the
//! rest of the M2 interpreter works (`:=`'s lazy dataflow fusion is its own
//! later milestone — see the module doc at the top of `lib.rs` — and a query
//! optimizer over table operations would be the same kind of project, not
//! attempted here). What's real: genuinely columnar Rust `Vec` storage and
//! whole-column operations, which is already far more efficient than a
//! row-of-structs representation would be — just without a planner or
//! multi-core execution on top of it.

use std::borrow::Cow;
use std::fmt::Write as _;

/// Swaps the chosen decimal marker to `.` before `str::parse::<f64>`, the
/// simplest correct approach (no hand-rolled float parsing). A no-op,
/// allocation-free path when `decimal == "."` (the default).
fn normalize_decimal<'a>(field: &'a str, decimal: &str) -> Cow<'a, str> {
    if decimal == "." {
        Cow::Borrowed(field)
    } else {
        Cow::Owned(field.replace(decimal, "."))
    }
}

/// The reduction `group_by_agg` applies within each bucket.
fn group_reduce(agg: &str, xs: &[f64]) -> f64 {
    match agg {
        "mean" => xs.iter().sum::<f64>() / xs.len() as f64,
        "sum" => xs.iter().sum::<f64>(),
        "count" => xs.len() as f64,
        "min" => xs.iter().cloned().fold(f64::INFINITY, f64::min),
        "max" => xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "std" => {
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            (xs.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
        }
        _ => f64::NAN,
    }
}

// `PartialEq` so a `Table` can be compared by CONTENT rather than by
// identity -- see `values_equal`, where a table is a value-like kind and a
// user expects `contains(tables, t)` to find an equal one.
#[derive(Clone, Debug, PartialEq)]
pub enum Column {
    Num(Vec<f64>),
    Str(Vec<String>),
}

/// A single cell value read out of (or written into) a row — the
/// column-type-agnostic currency `row_values`/`rows_by_index`/`insert_row`
/// trade in, so this module never has to depend on `qu-interp`'s `Value`
/// (which itself depends on `Table`). One-to-one with `Column`'s two
/// variants; the interpreter side converts to/from `Value::Num`/`Value::Str`
/// at the boundary (see `lib.rs`'s `eval_index`/the `"at"`/`"insert_row"`
/// builtins).
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Num(f64),
    Str(String),
}

impl Column {
    pub fn len(&self) -> usize {
        match self {
            Column::Num(v) => v.len(),
            Column::Str(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn take(&self, indices: &[usize]) -> Column {
        match self {
            Column::Num(v) => Column::Num(indices.iter().map(|&i| v[i]).collect()),
            Column::Str(v) => Column::Str(indices.iter().map(|&i| v[i].clone()).collect()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    columns: Vec<(String, Column)>,
    /// `index by`/`.index("col")`'s designated lookup-key column (§ row
    /// index, 2026-08-26). `None` is the default auto index (row position
    /// `0, 1, 2, ...`, matching "index is made automatic when none is
    /// specified"); `Some(name)` means row lookups (`table[key]`, but NOT
    /// `.at(i)` — see its own doc comment) use that column's values instead.
    /// Carried through every row/column-preserving transform below (a
    /// `select`/`drop` that still has the index column keeps using it), but
    /// NOT validated eagerly if a transform happens to drop the index
    /// column itself — a stale `index_col` naming a since-removed column is
    /// caught lazily, with a clear error, the next time it's actually used
    /// for a lookup (`rows_by_index`), not proactively on every transform.
    index_col: Option<String>,
}

impl Table {
    pub fn from_columns(columns: Vec<(String, Column)>) -> Result<Table, String> {
        let n = columns.first().map(|(_, c)| c.len());
        if let Some(n) = n {
            for (name, col) in &columns {
                if col.len() != n {
                    return Err(format!(
                        "table: column `{name}` has {} rows, expected {n} (every column must be the same length)",
                        col.len()
                    ));
                }
            }
        }
        Ok(Table { columns, index_col: None })
    }

    pub fn nrows(&self) -> usize {
        self.columns.first().map(|(_, c)| c.len()).unwrap_or(0)
    }
    pub fn ncols(&self) -> usize {
        self.columns.len()
    }
    pub fn column_names(&self) -> Vec<&str> {
        self.columns.iter().map(|(n, _)| n.as_str()).collect()
    }
    pub fn col(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }
    pub fn col_num(&self, name: &str) -> Result<&[f64], String> {
        match self.col(name) {
            Some(Column::Num(v)) => Ok(v),
            Some(Column::Str(_)) => Err(format!("column `{name}` is text, not numeric")),
            None => Err(format!("no column named `{name}` (have: {})", self.column_names().join(", "))),
        }
    }
    /// Any column read as strings — a numeric column is formatted, so string
    /// operations (`group_by`, `filter` equality) work uniformly regardless
    /// of the column's underlying type.
    pub fn col_as_strings(&self, name: &str) -> Result<Vec<String>, String> {
        match self.col(name) {
            Some(Column::Num(v)) => Ok(v.iter().map(|n| crate::fmt_num(*n)).collect()),
            Some(Column::Str(v)) => Ok(v.clone()),
            None => Err(format!("no column named `{name}` (have: {})", self.column_names().join(", "))),
        }
    }

    pub fn select(&self, names: &[String]) -> Result<Table, String> {
        let mut columns = Vec::with_capacity(names.len());
        for name in names {
            let col = self.col(name).ok_or_else(|| {
                format!("no column named `{name}` (have: {})", self.column_names().join(", "))
            })?;
            columns.push((name.clone(), col.clone()));
        }
        Ok(Table { columns, index_col: self.index_col.clone() })
    }

    /// `data.drop(name)` / `data.drop([name1, name2, ...])` — a new `Table`
    /// with the named column(s) removed, every other column untouched and
    /// in its original order. The inverse-ish counterpart to `select`
    /// (which keeps only the named columns); together the pair covers both
    /// "keep these" and "remove these" without forcing a caller to spell
    /// out every OTHER column name just to drop one (the common
    /// `data.drop("target").corrmat()` shape — dropping the label column
    /// before an all-numeric-columns computation).
    ///
    /// Errors the same way `select`/`col` already do (unknown column name,
    /// full column list in the message) rather than silently ignoring a
    /// typo'd name — a silently-kept "target" column would otherwise
    /// corrupt a downstream `corrmat()`/`describe()` with no visible sign
    /// anything went wrong.
    pub fn drop(&self, names: &[String]) -> Result<Table, String> {
        for name in names {
            if self.col(name).is_none() {
                return Err(format!(
                    "drop: no column named `{name}` (have: {})",
                    self.column_names().join(", ")
                ));
            }
        }
        Ok(Table {
            columns: self
                .columns
                .iter()
                .filter(|(n, _)| !names.iter().any(|d| d == n))
                .cloned()
                .collect(),
            index_col: self.index_col.clone(),
        })
    }

    pub fn filter_mask(&self, mask: &[bool]) -> Result<Table, String> {
        if mask.len() != self.nrows() {
            return Err(format!(
                "filter: mask has {} entries, table has {} rows",
                mask.len(),
                self.nrows()
            ));
        }
        let indices: Vec<usize> = mask.iter().enumerate().filter(|(_, &m)| m).map(|(i, _)| i).collect();
        Ok(Table {
            columns: self.columns.iter().map(|(n, c)| (n.clone(), c.take(&indices))).collect(),
            index_col: self.index_col.clone(),
        })
    }

    pub fn head(&self, n: usize) -> Table {
        let n = n.min(self.nrows());
        let indices: Vec<usize> = (0..n).collect();
        Table {
            columns: self.columns.iter().map(|(name, c)| (name.clone(), c.take(&indices))).collect(),
            index_col: self.index_col.clone(),
        }
    }

    /// The LAST `n` rows, `head`'s counterpart. Asking to see the end of a
    /// table is as ordinary as asking to see the start, and only one of the
    /// two existed.
    pub fn tail(&self, n: usize) -> Table {
        let n = n.min(self.nrows());
        let indices: Vec<usize> = (self.nrows() - n..self.nrows()).collect();
        Table {
            columns: self.columns.iter().map(|(name, c)| (name.clone(), c.take(&indices))).collect(),
            index_col: self.index_col.clone(),
        }
    }

    pub fn sort_by(&self, name: &str, descending: bool) -> Result<Table, String> {
        let mut indices: Vec<usize> = (0..self.nrows()).collect();
        match self.col(name).ok_or_else(|| format!("no column named `{name}`"))? {
            Column::Num(v) => indices.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap()),
            Column::Str(v) => indices.sort_by(|&a, &b| v[a].cmp(&v[b])),
        }
        if descending {
            indices.reverse();
        }
        Ok(Table {
            columns: self.columns.iter().map(|(n, c)| (n.clone(), c.take(&indices))).collect(),
            index_col: self.index_col.clone(),
        })
    }

    /// `index by "col"` / `.index("col")` — designates `col` as the row
    /// lookup key for `table[key]` from now on (numeric or string, whatever
    /// the column holds). Returns a NEW table, matching every other
    /// Qu compound-value operation's immutable/functional convention.
    pub fn index_by(&self, name: &str) -> Result<Table, String> {
        if self.col(name).is_none() {
            return Err(format!(
                "index: no column named `{name}` (have: {})",
                self.column_names().join(", ")
            ));
        }
        Ok(Table {
            columns: self.columns.clone(),
            index_col: Some(name.to_string()),
        })
    }

    /// `.index()` with zero arguments — resets back to the default auto
    /// index (row position `0, 1, 2, ...`). Qu has no null/`nothing`
    /// literal to spell `.index(null)` with, so the agreed workaround is
    /// the zero-arg call (see the interpreter-side `"index"` builtin).
    pub fn reset_index(&self) -> Table {
        Table {
            columns: self.columns.clone(),
            index_col: None,
        }
    }

    pub fn index_col_name(&self) -> Option<&str> {
        self.index_col.as_deref()
    }

    /// One row's values, `(column name, cell)` in column order — the shared
    /// building block `.at(i)` and every `table[key]` lookup hit use to
    /// materialize a `Value::Record` on the `qu-interp` side (kept as
    /// `Cell`, not `Value`, so this module doesn't need to depend on
    /// `Value` — see `Cell`'s own doc comment).
    pub fn row_values(&self, row: usize) -> Vec<(String, Cell)> {
        self.columns
            .iter()
            .map(|(name, col)| {
                let cell = match col {
                    Column::Num(v) => Cell::Num(v[row]),
                    Column::Str(v) => Cell::Str(v[row].clone()),
                };
                (name.clone(), cell)
            })
            .collect()
    }

    /// `t.at(i)` — ALWAYS positional row access (pandas' `.iloc`), regardless
    /// of whatever `index by`/`.index()` custom index is currently set. This
    /// is the one way to resolve the ambiguity between "index value" and
    /// "row position" once a custom index exists — `table[key]` reads
    /// through the custom index when one is set, `.at(i)` never does.
    pub fn row_at(&self, i: usize) -> Result<Vec<(String, Cell)>, String> {
        if i >= self.nrows() {
            return Err(format!("at: row index {i} out of bounds ({} rows)", self.nrows()));
        }
        Ok(self.row_values(i))
    }

    /// `table[key]` under a custom index (`index_col` is `Some`) — every row
    /// position whose index-column value equals `key`. Duplicate index
    /// values are explicitly allowed (confirmed with Ahmed, 2026-08-26, same
    /// as pandas): a lookup on a duplicate label returns every matching row,
    /// not an error and not an arbitrarily-picked single row. The caller
    /// (`eval_index`) turns a single match into a bare `Value::Record` and
    /// more than one into a `Value::List` of them.
    ///
    /// A `key` whose type doesn't match the index column's own type (a
    /// string key against a numeric index column, or vice versa) is a clear
    /// error, not a silent "no rows found" — those are two different
    /// problems for a caller to fix.
    pub fn rows_by_index(&self, name: &str, key: &Cell) -> Result<Vec<usize>, String> {
        let col = self.col(name).ok_or_else(|| {
            format!("index column `{name}` no longer exists on this table (have: {})", self.column_names().join(", "))
        })?;
        let rows: Vec<usize> = match (col, key) {
            (Column::Num(v), Cell::Num(k)) => {
                v.iter().enumerate().filter(|(_, x)| *x == k).map(|(i, _)| i).collect()
            }
            (Column::Str(v), Cell::Str(k)) => {
                v.iter().enumerate().filter(|(_, x)| *x == k).map(|(i, _)| i).collect()
            }
            (Column::Num(_), Cell::Str(k)) => {
                return Err(format!("index column `{name}` is numeric; `\"{k}\"` is not a valid lookup key"))
            }
            (Column::Str(_), Cell::Num(k)) => {
                return Err(format!("index column `{name}` is text; `{k}` is not a valid lookup key"))
            }
        };
        if rows.is_empty() {
            return Err(format!("no row with index value {key:?} in column `{name}`"));
        }
        Ok(rows)
    }

    /// `drop_row(t, i)` / `drop_row(t, [i, j, ...])` — a new `Table` with
    /// row `i` (or every row named in the list) removed, every other row
    /// untouched and in its original order. Distinct from column-`drop`
    /// (which stays a plain `drop(t, "col")`, landed separately for the
    /// `corrmat()` pipeline) rather than an overload of it: `drop(t, 2)`
    /// would be genuinely ambiguous between "drop the column named `2`"
    /// (column names are arbitrary strings, `display_value` would happily
    /// turn `2` into `"2"`) and "drop row 2" — two distinct names sidestep
    /// that ambiguity entirely, matching `insert_row`/`insert_column`'s own
    /// distinct-names split below.
    ///
    /// Takes a slice so both the single-index and multi-index call shapes
    /// (dispatched by `row_indices_arg` on the `qu-interp` side) share one
    /// implementation. Every index is validated up front, before any row is
    /// actually removed, so an out-of-range index anywhere in a multi-row
    /// call drops nothing rather than partially applying; duplicate indices
    /// in `rows` are harmless (deduped via the `HashSet`, not an error and
    /// not double-removed).
    pub fn drop_row(&self, rows: &[usize]) -> Result<Table, String> {
        for &i in rows {
            if i >= self.nrows() {
                return Err(format!("drop_row: row index {i} out of bounds ({} rows)", self.nrows()));
            }
        }
        let drop_set: std::collections::HashSet<usize> = rows.iter().copied().collect();
        let indices: Vec<usize> = (0..self.nrows()).filter(|r| !drop_set.contains(r)).collect();
        Ok(Table {
            columns: self.columns.iter().map(|(n, c)| (n.clone(), c.take(&indices))).collect(),
            index_col: self.index_col.clone(),
        })
    }

    /// `insert_column(t, "name", values, [pos=])` — a new `Table` with a
    /// new column inserted at `pos` (default: appended at the end).
    /// `values` must have exactly as many rows as the table already has
    /// (unless the table is currently empty, i.e. has no rows yet).
    pub fn insert_column(&self, name: &str, values: Column, pos: Option<usize>) -> Result<Table, String> {
        if self.col(name).is_some() {
            return Err(format!("insert_column: column `{name}` already exists"));
        }
        if self.nrows() > 0 && values.len() != self.nrows() {
            return Err(format!(
                "insert_column: `{name}` has {} rows, table has {}",
                values.len(),
                self.nrows()
            ));
        }
        let mut columns = self.columns.clone();
        let pos = pos.unwrap_or(columns.len()).min(columns.len());
        columns.insert(pos, (name.to_string(), values));
        Ok(Table { columns, index_col: self.index_col.clone() })
    }

    /// `insert_row(t, i, record)` — a new `Table` with a new row inserted at
    /// position `i` (`0..=nrows()`; `i == nrows()` appends). `values` must
    /// supply exactly one cell per existing column (matched by name, order
    /// doesn't matter — a record's field order needn't match the table's
    /// column order) of the matching type (a numeric column needs
    /// `Cell::Num`, a text column needs `Cell::Str`).
    pub fn insert_row(&self, i: usize, values: &[(String, Cell)]) -> Result<Table, String> {
        if i > self.nrows() {
            return Err(format!("insert_row: row index {i} out of bounds (0..={})", self.nrows()));
        }
        for (name, _) in &self.columns {
            if !values.iter().any(|(n, _)| n == name) {
                return Err(format!("insert_row: missing value for column `{name}`"));
            }
        }
        for (name, _) in values {
            if self.col(name).is_none() {
                return Err(format!(
                    "insert_row: no column named `{name}` (have: {})",
                    self.column_names().join(", ")
                ));
            }
        }
        let mut columns = self.columns.clone();
        for (name, col) in columns.iter_mut() {
            let (_, cell) = values.iter().find(|(n, _)| n == name).unwrap();
            match (col, cell) {
                (Column::Num(v), Cell::Num(x)) => v.insert(i, *x),
                (Column::Str(v), Cell::Str(s)) => v.insert(i, s.clone()),
                (Column::Num(_), Cell::Str(s)) => {
                    return Err(format!("insert_row: column `{name}` is numeric; `\"{s}\"` is not a valid value"))
                }
                (Column::Str(_), Cell::Num(x)) => {
                    return Err(format!("insert_row: column `{name}` is text; `{x}` is not a valid value"))
                }
            }
        }
        Ok(Table { columns, index_col: self.index_col.clone() })
    }

    /// Groups rows by `group_col`'s distinct values (first-seen order) and
    /// reduces `value_col` within each group via `agg` (`mean`/`sum`/`count`/
    /// `min`/`max`/`std`). Returns a 2-column table: the group key and the
    /// aggregate, named `{group_col}`/`{agg}_{value_col}`. A thin single-spec
    /// call into `group_by_agg_multi` (below) — same one-pass algorithm with
    /// exactly one `(column, aggregate)` spec, not a separate code path, so
    /// the two can never drift apart.
    pub fn group_by_agg(&self, group_col: &str, value_col: &str, agg: &str) -> Result<Table, String> {
        self.group_by_agg_multi(group_col, &[(value_col.to_string(), agg.to_string())])
    }

    /// Groups rows by `group_col`'s distinct values (first-seen order) and
    /// reduces one or more `(column, aggregate)` pairs — `mean`/`sum`/
    /// `count`/`min`/`max`/`std` — within each group, ALL in a single pass
    /// over the table's rows. Returns a table: the group key column plus one
    /// output column per spec, named `{agg}_{column}` (same naming as the
    /// single-spec `group_by_agg`).
    ///
    /// This is genuinely one pass over the `nrows()` input rows, not the
    /// single-spec reduction called once per spec: every requested value
    /// column is fetched once up front (deduplicated, so `[("x","mean"),
    /// ("x","std")]` reads column `x` once, not twice) and the per-row loop
    /// below pushes into every group's bucket for every needed column
    /// together. The only work that scales with the number of specs after
    /// that loop is the O(groups) reduction step, not another O(rows) scan —
    /// that's what makes this cheaper than calling `group_by_agg` N times
    /// (which re-buckets all `nrows()` rows from scratch on every call).
    ///
    /// Buckets on the *displayed string* (numeric columns go through
    /// `fmt_num` first, via `col_as_strings` — two floats that display
    /// identically group together), matching `group_by_agg`'s own rule.
    pub fn group_by_agg_multi(&self, group_col: &str, specs: &[(String, String)]) -> Result<Table, String> {
        if specs.is_empty() {
            return Err("group_by_agg: needs at least one (column, aggregate) pair".to_string());
        }
        let keys = self.col_as_strings(group_col)?;

        // Distinct value columns referenced across `specs`, in first-seen
        // order, fetched once each — the dedup that keeps a column shared by
        // multiple specs (e.g. computing both `mean` and `std` of the same
        // column) from being read or bucketed twice.
        let mut col_order: Vec<&str> = Vec::new();
        for (col, _) in specs {
            if !col_order.contains(&col.as_str()) {
                col_order.push(col.as_str());
            }
        }
        let cols: Vec<&[f64]> = col_order
            .iter()
            .map(|c| self.col_num(c))
            .collect::<Result<_, _>>()?;

        // Single pass over every row: bucket each needed column's value into
        // its group, all `col_order.len()` columns together per row.
        let mut order: Vec<String> = Vec::new();
        let mut buckets: std::collections::HashMap<String, Vec<Vec<f64>>> = std::collections::HashMap::new();
        for (row, k) in keys.iter().enumerate() {
            use std::collections::hash_map::Entry;
            let bucket = match buckets.entry(k.clone()) {
                Entry::Vacant(e) => {
                    order.push(e.key().clone());
                    e.insert(vec![Vec::new(); col_order.len()])
                }
                Entry::Occupied(e) => e.into_mut(),
            };
            for (ci, col) in cols.iter().enumerate() {
                bucket[ci].push(col[row]);
            }
        }

        // Reduce: one output column per spec, computed from the already-
        // bucketed per-group values (no re-scan of the original rows).
        let mut out_cols: Vec<(String, Column)> = Vec::with_capacity(specs.len() + 1);
        out_cols.push((group_col.to_string(), Column::Str(order.clone())));
        for (col_name, agg) in specs {
            let ci = col_order.iter().position(|c| *c == col_name.as_str()).unwrap();
            let values: Vec<f64> = order.iter().map(|k| group_reduce(agg, &buckets[k][ci])).collect();
            out_cols.push((format!("{agg}_{col_name}"), Column::Num(values)));
        }
        Table::from_columns(out_cols)
    }

    /// Minimal RFC-4180-ish CSV parser: no quoted-field escaping — adequate
    /// for numeric/simple-text data exported by common tools, not a full CSV
    /// grammar. A column is numeric only if *every* row in it parses as a
    /// float; otherwise the whole column is text. Single-pass, low-allocation
    /// CSV parse. The naive approach (buffer every field as an owned `String`
    /// into a `Vec<Vec<String>>`, then parse each column to `f64` in a second
    /// pass) allocates one `String` per cell even for purely numeric columns,
    /// just to immediately parse it and throw it away — for a 500k-row file
    /// that's a million wasted heap allocations. This instead parses each
    /// numeric field straight from its borrowed `&str` slice into a
    /// pre-sized `Vec<f64>`; a `String` is only ever allocated for a column
    /// that turns out to genuinely be text (or the rare row where a numeric
    /// column hits a non-numeric cell, e.g. a trailing blank/`NaN` field —
    /// that column retroactively converts its `f64`s-so-far to strings and
    /// continues as `Str`, so correctness matches the old all-or-nothing-
    /// per-column behavior without paying its cost on the common all-numeric
    /// path).
    ///
    /// `sep` is the field delimiter (default `","`; any string works as a
    /// `str::split` pattern, so a single character like `";"`/`"\t"`/`"|"`
    /// is the common case but a genuinely multi-character delimiter also
    /// works). `decimal` is the decimal-point marker used when parsing a
    /// numeric field: `"."` (default) parses fields as plain Rust floats;
    /// `","` first swaps every `,` in the field to `.` before parsing, the
    /// European convention. The two combine for the standard European CSV
    /// export shape: `sep=";"` + `decimal=","` (comma is already spoken for
    /// as the decimal mark, so semicolon takes over as the field separator).
    ///
    /// Deliberately does **not** attempt to auto-detect the decimal
    /// convention per field (e.g. retrying `"3,14"` as comma-decimal when
    /// `decimal="."` fails to parse it): a lone comma is genuinely ambiguous
    /// between a decimal mark and a thousands separator (`"1,234"` is either
    /// `1.234` or `1234`), and there is no safe, narrow rule that resolves
    /// that ambiguity from a single field in isolation — even a same-column
    /// "established convention" check breaks on the common case where the
    /// very first ambiguous value in a column has nothing to compare against
    /// yet. Silently guessing wrong here would be exactly the class of
    /// silent-wrong-result bug this codebase's bug-hunt passes exist to catch,
    /// not reproduce. The explicit `decimal=` kwarg is the correct, safe way
    /// to handle this; when it's absent, an unparseable field falls back to
    /// making the whole column text, same as always — visible, not silent.
    pub fn from_csv(text: &str) -> Result<Table, String> {
        Table::from_csv_opts(text, true, ",", ".")
    }

    /// Auto-generated column names for headerless data (`headers=false`):
    /// `col1`, `col2`, ... (1-indexed) — Qu has no other established
    /// auto-naming convention for a table's columns to reuse, so this is the
    /// one other `headers=false` call sites should also follow.
    pub fn auto_column_name(i: usize) -> String {
        format!("col{}", i + 1)
    }

    pub fn from_csv_opts(text: &str, headers: bool, sep: &str, decimal: &str) -> Result<Table, String> {
        if sep.is_empty() {
            return Err("read_csv: sep can't be empty".to_string());
        }
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());

        let names: Vec<String> = if headers {
            let header = lines.next().ok_or("read_csv: empty file")?;
            header.split(sep).map(|s| s.trim().to_string()).collect()
        } else {
            // Peek the first data line just to count fields; it's still
            // consumed as data below (`lines` is a live iterator, not reset).
            let first = lines.clone().next().ok_or("read_csv: empty file")?;
            let ncols = first.split(sep).count();
            (0..ncols).map(Table::auto_column_name).collect()
        };

        // Cheap upper-bound row estimate (count remaining newlines) so the
        // per-column `Vec`s are sized once instead of repeatedly doubling.
        let row_estimate = text.as_bytes().iter().filter(|&&b| b == b'\n').count();
        let mut columns: Vec<Column> = names
            .iter()
            .map(|_| Column::Num(Vec::with_capacity(row_estimate)))
            .collect();

        for line in lines {
            let mut i = 0usize;
            for field in line.split(sep) {
                let Some(col) = columns.get_mut(i) else {
                    return Err(format!(
                        "read_csv: a row has more fields than the header's {}",
                        names.len()
                    ));
                };
                let field = field.trim();
                match col {
                    Column::Num(nums) => match normalize_decimal(field, decimal).parse::<f64>() {
                        Ok(v) => nums.push(v),
                        Err(_) => {
                            // First non-numeric cell in this column: convert
                            // what's been collected so far and fall back to
                            // `Str` for the rest of the file.
                            let mut strs: Vec<String> =
                                nums.iter().map(|n| n.to_string()).collect();
                            strs.push(field.to_string());
                            *col = Column::Str(strs);
                        }
                    },
                    Column::Str(strs) => strs.push(field.to_string()),
                }
                i += 1;
            }
            if i != names.len() {
                return Err(format!(
                    "read_csv: a row has {i} field{}, the header has {}",
                    if i == 1 { "" } else { "s" },
                    names.len()
                ));
            }
        }
        Table::from_columns(names.into_iter().zip(columns).collect())
    }

    /// Writes straight into one growing buffer rather than the previous
    /// per-row `Vec<String>` + `.join(",")` (two throwaway allocations per
    /// row on top of the per-cell ones) — same "stop allocating what you're
    /// about to discard" fix as `from_csv`'s own rewrite.
    pub fn to_csv(&self) -> String {
        // Rough capacity estimate (8 bytes/cell) so the buffer isn't
        // repeatedly doubled+copied while writing a large table.
        let mut out = String::with_capacity(self.nrows() * self.ncols() * 8 + 32);
        let _ = writeln!(out, "{}", self.column_names().join(","));
        for row in 0..self.nrows() {
            for (i, (_, c)) in self.columns.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                match c {
                    Column::Num(v) => out.push_str(&crate::fmt_num(v[row])),
                    Column::Str(v) => out.push_str(&v[row]),
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn summary(&self) -> String {
        format!("table({} rows x {} cols: {})", self.nrows(), self.ncols(), self.column_names().join(", "))
    }

    /// Renders the table as an aligned, human-readable grid — column
    /// headers, a header separator rule, then one row per line — this is
    /// what `print(table)`/`disp(table)` actually shows now (via
    /// `display_value`'s `Value::Table` arm in `lib.rs`), replacing the
    /// old one-line `summary()` (which is still available on its own for
    /// callers that want the compact form). Numeric columns right-align
    /// (values line up on the ones place, matching every spreadsheet/`R`/
    /// pandas convention); text columns left-align (matches how prose
    /// reads). Plain ASCII (`-` rule, two-space gutters) rather than Unicode
    /// box-drawing characters — this file has a standing mojibake hazard
    /// with multi-byte characters (see `display_value`'s `Vec`/`Mat`
    /// truncation markers elsewhere in this crate, which already got
    /// mangled that way), so new code sticks to ASCII rather than risk
    /// adding another instance.
    ///
    /// Large tables truncate to the first `MAX_DISPLAY_ROWS` rows plus a
    /// trailing `"... N more rows (M total)"` note — the same "bounded
    /// prefix + summary trailer" shape `display_value`'s own `Vec`/`Mask`/
    /// `CVec`/`Signal` truncation already uses for oversized values, just
    /// sized to a table's natural unit (rows, not elements).
    pub fn render_terminal(&self) -> String {
        const MAX_DISPLAY_ROWS: usize = 20;

        if self.ncols() == 0 {
            return "table(0 rows x 0 cols)".to_string();
        }

        let nrows = self.nrows();
        let shown = nrows.min(MAX_DISPLAY_ROWS);
        let is_numeric: Vec<bool> = self.columns.iter().map(|(_, c)| matches!(c, Column::Num(_))).collect();

        // Cell text for just the rows actually shown — no point formatting
        // rows that get truncated away below.
        let cells: Vec<Vec<String>> = self
            .columns
            .iter()
            .map(|(_, c)| match c {
                Column::Num(v) => v[..shown].iter().map(|n| crate::fmt_num(*n)).collect(),
                Column::Str(v) => v[..shown].to_vec(),
            })
            .collect();

        let headers = self.column_names();
        let widths: Vec<usize> = (0..self.ncols())
            .map(|i| headers[i].len().max(cells[i].iter().map(|s| s.len()).max().unwrap_or(0)))
            .collect();

        fn pad(s: &str, width: usize, right: bool) -> String {
            if right {
                format!("{s:>width$}")
            } else {
                format!("{s:<width$}")
            }
        }

        let mut out = String::new();
        for (i, h) in headers.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&pad(h, widths[i], is_numeric[i]));
        }
        out.push('\n');
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&"-".repeat(*w));
        }
        for row in 0..shown {
            out.push('\n');
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                out.push_str(&pad(&cells[i][row], *w, is_numeric[i]));
            }
        }
        if nrows > shown {
            let remaining = nrows - shown;
            let _ = write!(out, "\n... {remaining} more row{} ({nrows} total)", if remaining == 1 { "" } else { "s" });
        }
        out
    }

    /// Renders the table as a complete, standalone LaTeX `tabular`
    /// environment — `tex(table)`/`printtex(table)`'s table-shaped
    /// counterpart to `display_value_tex`'s `$...$`-wrapped math snippet in
    /// `lib.rs` (a `tabular` environment isn't valid inside inline math
    /// mode, so a `Table` gets its own shape there instead of being forced
    /// into the `$...$` wrapper every other value uses — see that
    /// function's `Value::Table` early-return).
    ///
    /// Column alignment mirrors `render_terminal`'s own rule: numeric
    /// columns right-aligned (`r`), text columns left-aligned (`l`), so the
    /// generated `{rl...}` spec always has exactly one letter per column.
    /// Every header and cell goes through `tex_escape_cell`, so a column
    /// named e.g. `a_b` or a cell containing `%`/`&`/`$`/`_`/`#`/`^`/`~`/a
    /// backslash compiles instead of silently breaking the LaTeX build.
    /// Header names, formatted cells row-by-row, and which columns are
    /// numeric (right-aligns in a report the same way `to_tex` already
    /// right-aligns a `Column::Num`) -- the shared conversion point for
    /// anything that wants this table's DATA without also wanting
    /// `to_tex`'s own fixed `tabular` shape. `report_builder::TableData`
    /// is the reason this exists: a report's table needs a caption, a
    /// label and (via the emitter it is headed to) a float wrapper that
    /// `to_tex` has no way to add, so it builds its own `tabular` from
    /// this instead of from a re-parsed `to_tex()` string.
    pub fn to_cells(&self) -> (Vec<String>, Vec<Vec<String>>, Vec<bool>) {
        let headers: Vec<String> = self.column_names().iter().map(|s| s.to_string()).collect();
        let numeric: Vec<bool> = self.columns.iter().map(|(_, c)| matches!(c, Column::Num(_))).collect();
        let rows: Vec<Vec<String>> = (0..self.nrows())
            .map(|row| {
                self.columns
                    .iter()
                    .map(|(_, c)| match c {
                        Column::Num(v) => crate::fmt_num(v[row]),
                        Column::Str(v) => v[row].clone(),
                    })
                    .collect()
            })
            .collect();
        (headers, rows, numeric)
    }

    pub fn to_tex(&self) -> String {
        let align: String = self
            .columns
            .iter()
            .map(|(_, c)| if matches!(c, Column::Num(_)) { 'r' } else { 'l' })
            .collect();
        let headers: Vec<String> = self.column_names().iter().map(|h| tex_escape_cell(h)).collect();

        let mut out = String::new();
        let _ = writeln!(out, "\\begin{{tabular}}{{{align}}}");
        out.push_str("\\hline\n");
        let _ = writeln!(out, "{} \\\\", headers.join(" & "));
        out.push_str("\\hline\n");
        for row in 0..self.nrows() {
            let cells: Vec<String> = self
                .columns
                .iter()
                .map(|(_, c)| {
                    let s = match c {
                        Column::Num(v) => crate::fmt_num(v[row]),
                        Column::Str(v) => v[row].clone(),
                    };
                    tex_escape_cell(&s)
                })
                .collect();
            let _ = writeln!(out, "{} \\\\", cells.join(" & "));
        }
        out.push_str("\\hline\n");
        out.push_str("\\end{tabular}");
        out
    }
}

/// Escapes LaTeX's special characters in a header/cell string so a column
/// named e.g. `a_b` or a cell containing `%`/`&`/`$`/`#`/`~`/`^`/a literal
/// backslash renders as that literal text instead of being interpreted as
/// LaTeX markup (or, worse, breaking the compile outright — an unescaped
/// `_`/`&`/`%` inside `tabular` is a hard LaTeX error, not just a cosmetic
/// glitch). Order matters: backslash is escaped first, so the backslashes
/// introduced by escaping every other character are never themselves
/// re-escaped on a second pass (this function only makes one pass, so this
/// just documents why `\\` isn't handled by prefixing the others' own
/// replacement backslash).
fn tex_escape_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        Table::from_columns(vec![
            ("group".to_string(), Column::Str(vec!["a".into(), "a".into(), "b".into(), "b".into()])),
            ("value".to_string(), Column::Num(vec![1.0, 3.0, 10.0, 20.0])),
        ])
        .unwrap()
    }

    #[test]
    fn rejects_mismatched_column_lengths() {
        let err = Table::from_columns(vec![
            ("a".to_string(), Column::Num(vec![1.0, 2.0])),
            ("b".to_string(), Column::Num(vec![1.0])),
        ])
        .unwrap_err();
        assert!(err.contains("b"));
    }

    #[test]
    fn filter_mask_keeps_matching_rows_in_every_column() {
        let t = sample();
        let filtered = t.filter_mask(&[true, false, true, false]).unwrap();
        assert_eq!(filtered.nrows(), 2);
        assert_eq!(filtered.col_num("value").unwrap(), &[1.0, 10.0]);
        assert_eq!(filtered.col_as_strings("group").unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn drop_removes_named_column_and_keeps_others() {
        let t = sample();
        let dropped = t.drop(&["group".to_string()]).unwrap();
        assert_eq!(dropped.column_names(), vec!["value"]);
        assert_eq!(dropped.col_num("value").unwrap(), t.col_num("value").unwrap());
    }

    #[test]
    fn drop_multiple_columns_at_once() {
        let t = Table::from_columns(vec![
            ("a".to_string(), Column::Num(vec![1.0, 2.0])),
            ("b".to_string(), Column::Num(vec![3.0, 4.0])),
            ("c".to_string(), Column::Num(vec![5.0, 6.0])),
        ])
        .unwrap();
        let dropped = t.drop(&["a".to_string(), "c".to_string()]).unwrap();
        assert_eq!(dropped.column_names(), vec!["b"]);
    }

    #[test]
    fn drop_unknown_column_is_a_clear_error() {
        let t = sample();
        let err = t.drop(&["nope".to_string()]).unwrap_err();
        assert!(err.contains("nope"));
        assert!(err.contains("group"));
    }

    #[test]
    fn sort_by_orders_every_column_consistently() {
        let t = sample().sort_by("value", true).unwrap();
        assert_eq!(t.col_num("value").unwrap(), &[20.0, 10.0, 3.0, 1.0]);
        assert_eq!(t.col_as_strings("group").unwrap(), vec!["b", "b", "a", "a"]);
    }

    #[test]
    fn group_by_agg_reduces_each_group() {
        let t = sample().group_by_agg("group", "value", "mean").unwrap();
        assert_eq!(t.col_as_strings("group").unwrap(), vec!["a", "b"]);
        assert_eq!(t.col_num("mean_value").unwrap(), &[2.0, 15.0]);
    }

    /// Multi-spec table used by the `group_by_agg_multi` tests below: two
    /// numeric columns (`value`, `qty`) so a spec list can mix columns, not
    /// just aggregates of the same one.
    fn sample_multi() -> Table {
        Table::from_columns(vec![
            ("group".to_string(), Column::Str(vec!["a".into(), "a".into(), "b".into(), "b".into(), "b".into()])),
            ("value".to_string(), Column::Num(vec![1.0, 3.0, 10.0, 20.0, 30.0])),
            ("qty".to_string(), Column::Num(vec![5.0, 7.0, 1.0, 2.0, 3.0])),
        ])
        .unwrap()
    }

    #[test]
    fn group_by_agg_multi_computes_every_spec_in_one_call() {
        let t = sample_multi();
        let multi = t
            .group_by_agg_multi(
                "group",
                &[
                    ("value".to_string(), "mean".to_string()),
                    ("value".to_string(), "sum".to_string()),
                    ("qty".to_string(), "count".to_string()),
                    ("qty".to_string(), "min".to_string()),
                    ("qty".to_string(), "max".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(multi.column_names(), vec!["group", "mean_value", "sum_value", "count_qty", "min_qty", "max_qty"]);
        assert_eq!(multi.col_as_strings("group").unwrap(), vec!["a", "b"]);
        assert_eq!(multi.col_num("mean_value").unwrap(), &[2.0, 20.0]);
        assert_eq!(multi.col_num("sum_value").unwrap(), &[4.0, 60.0]);
        assert_eq!(multi.col_num("count_qty").unwrap(), &[2.0, 3.0]);
        assert_eq!(multi.col_num("min_qty").unwrap(), &[5.0, 1.0]);
        assert_eq!(multi.col_num("max_qty").unwrap(), &[7.0, 3.0]);
    }

    #[test]
    fn group_by_agg_multi_matches_calling_the_single_spec_form_n_times() {
        // Equivalence proof: every column of the multi-spec result must
        // match what the old single-`(col,agg)` form produces when called
        // once per spec, across every aggregate the single form supports.
        let t = sample_multi();
        let specs = [
            ("value".to_string(), "mean".to_string()),
            ("value".to_string(), "sum".to_string()),
            ("value".to_string(), "count".to_string()),
            ("value".to_string(), "min".to_string()),
            ("value".to_string(), "max".to_string()),
            ("value".to_string(), "std".to_string()),
            ("qty".to_string(), "mean".to_string()),
            ("qty".to_string(), "sum".to_string()),
            ("qty".to_string(), "std".to_string()),
        ];
        let multi = t.group_by_agg_multi("group", &specs).unwrap();
        assert_eq!(multi.col_as_strings("group").unwrap(), vec!["a", "b"]);
        for (col, agg) in &specs {
            let single = t.group_by_agg("group", col, agg).unwrap();
            let single_col_name = format!("{agg}_{col}");
            assert_eq!(
                multi.col_num(&single_col_name).unwrap(),
                single.col_num(&single_col_name).unwrap(),
                "mismatch for spec ({col}, {agg})"
            );
            // Group ordering/labels must also line up.
            assert_eq!(multi.col_as_strings("group").unwrap(), single.col_as_strings("group").unwrap());
        }
    }

    #[test]
    fn group_by_agg_single_spec_form_matches_multi_with_one_spec() {
        // `group_by_agg` is now defined as a one-spec call into
        // `group_by_agg_multi` — pin that they agree exactly.
        let t = sample_multi();
        let single = t.group_by_agg("group", "value", "mean").unwrap();
        let multi = t.group_by_agg_multi("group", &[("value".to_string(), "mean".to_string())]).unwrap();
        assert_eq!(single.column_names(), multi.column_names());
        assert_eq!(single.col_num("mean_value").unwrap(), multi.col_num("mean_value").unwrap());
    }

    #[test]
    fn group_by_agg_multi_empty_specs_is_a_clear_error() {
        let t = sample_multi();
        let err = t.group_by_agg_multi("group", &[]).unwrap_err();
        assert!(err.contains("at least one"));
    }

    #[test]
    fn csv_round_trip_preserves_numeric_and_text_columns() {
        let t = sample();
        let csv = t.to_csv();
        let parsed = Table::from_csv(&csv).unwrap();
        assert_eq!(parsed.col_num("value").unwrap(), t.col_num("value").unwrap());
        assert_eq!(parsed.col_as_strings("group").unwrap(), t.col_as_strings("group").unwrap());
    }

    #[test]
    fn csv_column_is_text_if_any_row_fails_to_parse_as_a_number() {
        let t = Table::from_csv("a,b\n1,x\n2,3\n").unwrap();
        assert!(matches!(t.col("a"), Some(Column::Num(_))));
        assert!(matches!(t.col("b"), Some(Column::Str(_)))); // "x" forces the whole column to text
    }

    #[test]
    fn csv_headers_false_auto_names_columns_and_keeps_first_line_as_data() {
        let t = Table::from_csv_opts("1,2,3\n4,5,6\n", false, ",", ".").unwrap();
        assert_eq!(t.column_names(), vec!["col1", "col2", "col3"]);
        assert_eq!(t.nrows(), 2);
        assert_eq!(t.col_num("col1").unwrap(), &[1.0, 4.0]);
        assert_eq!(t.col_num("col3").unwrap(), &[3.0, 6.0]);
    }

    #[test]
    fn csv_custom_sep_splits_fields_on_semicolon() {
        let t = Table::from_csv_opts("a;b\n1;2\n3;4\n", true, ";", ".").unwrap();
        assert_eq!(t.column_names(), vec!["a", "b"]);
        assert_eq!(t.col_num("a").unwrap(), &[1.0, 3.0]);
        assert_eq!(t.col_num("b").unwrap(), &[2.0, 4.0]);
    }

    #[test]
    fn csv_comma_decimal_converts_correctly() {
        let t = Table::from_csv_opts("a\n3,14\n2,5\n", true, ";", ",").unwrap();
        assert_eq!(t.col_num("a").unwrap(), &[3.14, 2.5]);
    }

    #[test]
    fn csv_european_sep_and_decimal_combination_end_to_end() {
        // The motivating real-world case: sep=";" (comma is already taken as
        // the decimal mark) + decimal=",".
        let t = Table::from_csv_opts("name;value\nwidget;3,14\ngadget;2,50\n", true, ";", ",").unwrap();
        assert_eq!(t.column_names(), vec!["name", "value"]);
        assert_eq!(t.col_as_strings("name").unwrap(), vec!["widget", "gadget"]);
        assert_eq!(t.col_num("value").unwrap(), &[3.14, 2.5]);
    }

    #[test]
    fn csv_default_options_unchanged_regression() {
        // Same as `csv_round_trip_preserves_numeric_and_text_columns` and
        // `csv_column_is_text_if_any_row_fails_to_parse_as_a_number`, but
        // routed through `from_csv_opts` explicitly with the documented
        // defaults, to pin that `from_csv` really is just this.
        let t = Table::from_csv_opts("a,b\n1,x\n2,3\n", true, ",", ".").unwrap();
        assert!(matches!(t.col("a"), Some(Column::Num(_))));
        assert!(matches!(t.col("b"), Some(Column::Str(_))));
    }

    #[test]
    fn csv_no_implicit_decimal_fallback_falls_back_to_text_column() {
        // With decimal="." (the default), a comma-decimal field ("3,14")
        // does NOT get an implicit second-chance parse as comma-decimal —
        // it's genuinely ambiguous with a thousands separator, so the
        // column correctly falls back to text instead of silently guessing.
        // Uses sep=";" so the field's own comma survives intact instead of
        // being read as a second field.
        let t = Table::from_csv_opts("a\n3,14\n2.5\n", true, ";", ".").unwrap();
        assert!(matches!(t.col("a"), Some(Column::Str(_))));
    }

    #[test]
    fn csv_empty_sep_is_a_clear_error() {
        let err = Table::from_csv_opts("a,b\n1,2\n", true, "", ".").unwrap_err();
        assert!(err.contains("sep"));
    }

    // ---- row index + bracket-indexing model + drop/insert (§ table row
    // index, 2026-08-26) ----

    #[test]
    fn default_index_is_none_and_row_at_matches_row_values() {
        let t = sample();
        assert_eq!(t.index_col_name(), None);
        assert_eq!(t.row_at(1).unwrap(), t.row_values(1));
        assert_eq!(t.row_at(1).unwrap(), vec![
            ("group".to_string(), Cell::Str("a".to_string())),
            ("value".to_string(), Cell::Num(3.0)),
        ]);
    }

    #[test]
    fn row_at_out_of_bounds_is_a_clear_error() {
        let t = sample();
        let err = t.row_at(4).unwrap_err();
        assert!(err.contains('4'));
    }

    #[test]
    fn index_by_sets_a_custom_index_and_rows_by_index_looks_up_by_label() {
        let t = sample().index_by("group").unwrap();
        assert_eq!(t.index_col_name(), Some("group"));
        let rows = t.rows_by_index("group", &Cell::Str("b".to_string())).unwrap();
        assert_eq!(rows, vec![2, 3]);
    }

    #[test]
    fn rows_by_index_returns_every_duplicate_match() {
        // Confirmed with Ahmed, 2026-08-26: duplicate index values are
        // explicitly allowed (same as pandas) — every matching row comes
        // back, not just the first, and not an error.
        let t = sample().index_by("group").unwrap();
        let rows_a = t.rows_by_index("group", &Cell::Str("a".to_string())).unwrap();
        assert_eq!(rows_a, vec![0, 1]);
    }

    #[test]
    fn rows_by_index_unknown_label_is_a_clear_error() {
        let t = sample().index_by("group").unwrap();
        let err = t.rows_by_index("group", &Cell::Str("z".to_string())).unwrap_err();
        assert!(err.contains('z'));
    }

    #[test]
    fn rows_by_index_type_mismatch_is_a_clear_error_not_a_silent_no_match() {
        let t = sample().index_by("group").unwrap();
        let err = t.rows_by_index("group", &Cell::Num(1.0)).unwrap_err();
        assert!(err.contains("numeric") || err.contains("valid lookup key"));
    }

    #[test]
    fn index_by_unknown_column_is_a_clear_error() {
        let err = sample().index_by("nope").unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn reset_index_clears_a_custom_index_back_to_default() {
        let t = sample().index_by("group").unwrap();
        assert_eq!(t.index_col_name(), Some("group"));
        let t2 = t.reset_index();
        assert_eq!(t2.index_col_name(), None);
    }

    #[test]
    fn drop_row_removes_only_that_row_and_keeps_others_in_order() {
        let t = sample();
        let dropped = t.drop_row(&[1]).unwrap();
        assert_eq!(dropped.nrows(), 3);
        assert_eq!(dropped.col_num("value").unwrap(), &[1.0, 10.0, 20.0]);
        assert_eq!(dropped.col_as_strings("group").unwrap(), vec!["a", "b", "b"]);
        // original untouched
        assert_eq!(t.nrows(), 4);
    }

    #[test]
    fn drop_row_multi_removes_every_named_row_and_keeps_order() {
        let t = sample();
        // sample() has 4 rows (value 1, 3, 10, 20 / group a, a, b, b);
        // drop rows 0 and 2, out of order in the list.
        let dropped = t.drop_row(&[2, 0]).unwrap();
        assert_eq!(dropped.nrows(), 2);
        assert_eq!(dropped.col_num("value").unwrap(), &[3.0, 20.0]);
        assert_eq!(dropped.col_as_strings("group").unwrap(), vec!["a", "b"]);
        // original untouched
        assert_eq!(t.nrows(), 4);
    }

    #[test]
    fn drop_row_out_of_bounds_is_a_clear_error() {
        let t = sample();
        let err = t.drop_row(&[10]).unwrap_err();
        assert!(err.contains("10"));
    }

    #[test]
    fn drop_row_multi_out_of_bounds_names_the_bad_index_and_drops_nothing() {
        let t = sample();
        let err = t.drop_row(&[1, 99]).unwrap_err();
        assert!(err.contains("99"));
        // original untouched (nothing partially applied)
        assert_eq!(t.nrows(), 4);
    }

    #[test]
    fn insert_column_appends_by_default_and_does_not_mutate_original() {
        let t = sample();
        let t2 = t.insert_column("flag", Column::Num(vec![1.0, 0.0, 1.0, 0.0]), None).unwrap();
        assert_eq!(t2.column_names(), vec!["group", "value", "flag"]);
        assert_eq!(t2.col_num("flag").unwrap(), &[1.0, 0.0, 1.0, 0.0]);
        // original untouched
        assert_eq!(t.column_names(), vec!["group", "value"]);
    }

    #[test]
    fn insert_column_at_explicit_position() {
        let t = sample();
        let t2 = t.insert_column("first", Column::Num(vec![9.0, 9.0, 9.0, 9.0]), Some(0)).unwrap();
        assert_eq!(t2.column_names(), vec!["first", "group", "value"]);
    }

    #[test]
    fn insert_column_wrong_length_is_a_clear_error() {
        let t = sample();
        let err = t.insert_column("bad", Column::Num(vec![1.0]), None).unwrap_err();
        assert!(err.contains("bad"));
    }

    #[test]
    fn insert_column_duplicate_name_is_a_clear_error() {
        let t = sample();
        let err = t.insert_column("value", Column::Num(vec![1.0, 2.0, 3.0, 4.0]), None).unwrap_err();
        assert!(err.contains("value"));
    }

    #[test]
    fn insert_row_inserts_at_position_and_does_not_mutate_original() {
        let t = sample();
        let t2 = t
            .insert_row(1, &[
                ("group".to_string(), Cell::Str("x".to_string())),
                ("value".to_string(), Cell::Num(99.0)),
            ])
            .unwrap();
        assert_eq!(t2.nrows(), 5);
        assert_eq!(t2.col_num("value").unwrap(), &[1.0, 99.0, 3.0, 10.0, 20.0]);
        assert_eq!(t2.col_as_strings("group").unwrap(), vec!["a", "x", "a", "b", "b"]);
        // original untouched
        assert_eq!(t.nrows(), 4);
    }

    #[test]
    fn insert_row_append_at_nrows() {
        let t = sample();
        let n = t.nrows();
        let t2 = t
            .insert_row(n, &[
                ("group".to_string(), Cell::Str("c".to_string())),
                ("value".to_string(), Cell::Num(42.0)),
            ])
            .unwrap();
        assert_eq!(t2.nrows(), 5);
        assert_eq!(t2.col_num("value").unwrap()[4], 42.0);
    }

    #[test]
    fn insert_row_missing_field_is_a_clear_error() {
        let t = sample();
        let err = t.insert_row(0, &[("group".to_string(), Cell::Str("x".to_string()))]).unwrap_err();
        assert!(err.contains("value"));
    }

    #[test]
    fn insert_row_out_of_bounds_is_a_clear_error() {
        let t = sample();
        let err = t
            .insert_row(99, &[
                ("group".to_string(), Cell::Str("x".to_string())),
                ("value".to_string(), Cell::Num(1.0)),
            ])
            .unwrap_err();
        assert!(err.contains("99"));
    }

    // ---- render_terminal / to_tex (§ real table rendering, 2026-08-26) ----

    #[test]
    fn render_terminal_aligns_headers_and_columns_numeric_right_text_left() {
        let t = sample(); // columns: group (Str: a,a,b,b), value (Num: 1,3,10,20)
        let out = t.render_terminal();
        let lines: Vec<&str> = out.lines().collect();
        // header, separator, 4 data rows, no truncation note
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], "group  value");
        assert_eq!(lines[1], "-----  -----");
        // text left-aligned (no leading pad needed here since "group"/"a" are
        // already the same effective width), numeric right-aligned against
        // the "value" header's own width
        assert_eq!(lines[2], "a          1");
        assert_eq!(lines[3], "a          3");
        assert_eq!(lines[4], "b         10");
        assert_eq!(lines[5], "b         20");
    }

    #[test]
    fn render_terminal_right_aligns_numeric_widths_against_widest_value() {
        let t = Table::from_columns(vec![
            ("n".to_string(), Column::Num(vec![1.0, 22.0, 333.0])),
        ])
        .unwrap();
        let out = t.render_terminal();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "  n");
        assert_eq!(lines[1], "---");
        assert_eq!(lines[2], "  1");
        assert_eq!(lines[3], " 22");
        assert_eq!(lines[4], "333");
    }

    #[test]
    fn render_terminal_truncates_large_tables_with_a_trailing_count() {
        let n: Vec<f64> = (0..25).map(|i| i as f64).collect();
        let t = Table::from_columns(vec![("n".to_string(), Column::Num(n))]).unwrap();
        let out = t.render_terminal();
        let lines: Vec<&str> = out.lines().collect();
        // header + separator + 20 shown rows + 1 trailing note line
        assert_eq!(lines.len(), 23);
        assert_eq!(lines[22], "... 5 more rows (25 total)");
    }

    #[test]
    fn render_terminal_singular_more_row_message() {
        let n: Vec<f64> = (0..21).map(|i| i as f64).collect();
        let t = Table::from_columns(vec![("n".to_string(), Column::Num(n))]).unwrap();
        let out = t.render_terminal();
        assert!(out.ends_with("... 1 more row (21 total)"));
    }

    #[test]
    fn render_terminal_small_table_has_no_truncation_note() {
        let t = sample();
        assert!(!t.render_terminal().contains("more row"));
    }

    #[test]
    fn render_terminal_empty_table_shows_a_placeholder() {
        let t = Table::from_columns(vec![]).unwrap();
        assert_eq!(t.render_terminal(), "table(0 rows x 0 cols)");
    }

    #[test]
    fn tex_escape_cell_handles_every_special_character() {
        assert_eq!(tex_escape_cell("a_b"), "a\\_b");
        assert_eq!(tex_escape_cell("50%"), "50\\%");
        assert_eq!(tex_escape_cell("a&b"), "a\\&b");
        assert_eq!(tex_escape_cell("$5"), "\\$5");
        assert_eq!(tex_escape_cell("#1"), "\\#1");
        assert_eq!(tex_escape_cell("a^b"), "a\\textasciicircum{}b");
        assert_eq!(tex_escape_cell("a~b"), "a\\textasciitilde{}b");
        assert_eq!(tex_escape_cell("a\\b"), "a\\textbackslash{}b");
        assert_eq!(tex_escape_cell("plain"), "plain");
    }

    #[test]
    fn to_tex_produces_a_balanced_tabular_with_correct_column_count_and_alignment() {
        let t = sample(); // group: Str, value: Num
        let tex = t.to_tex();
        assert!(tex.starts_with("\\begin{tabular}{lr}\n"));
        assert!(tex.trim_end().ends_with("\\end{tabular}"));
        assert_eq!(tex.matches("\\begin{tabular}").count(), 1);
        assert_eq!(tex.matches("\\end{tabular}").count(), 1);
        // header + hline + 4 data rows + hline, each data/header row ends "\\"
        let body_lines: Vec<&str> = tex.lines().collect();
        assert_eq!(body_lines[0], "\\begin{tabular}{lr}");
        assert_eq!(body_lines[1], "\\hline");
        assert_eq!(body_lines[2], "group & value \\\\");
        assert_eq!(body_lines[3], "\\hline");
        assert_eq!(body_lines[4], "a & 1 \\\\");
        assert_eq!(body_lines[5], "a & 3 \\\\");
        assert_eq!(body_lines[6], "b & 10 \\\\");
        assert_eq!(body_lines[7], "b & 20 \\\\");
        assert_eq!(body_lines[8], "\\hline");
        assert_eq!(body_lines[9], "\\end{tabular}");
    }

    #[test]
    fn to_tex_escapes_a_special_char_in_the_column_name_and_in_cell_content() {
        let t = Table::from_columns(vec![
            ("a_b".to_string(), Column::Str(vec!["50%".to_string(), "x&y".to_string()])),
        ])
        .unwrap();
        let tex = t.to_tex();
        assert!(tex.contains("a\\_b"));
        assert!(tex.contains("50\\%"));
        assert!(tex.contains("x\\&y"));
        // and the raw, dangerous characters must NOT survive unescaped
        assert!(!tex.contains("a_b \\\\")); // header line unescaped would look like this
        assert!(!tex.contains("50% \\\\"));
    }

    #[test]
    fn to_tex_column_count_in_spec_matches_actual_column_count() {
        let t = Table::from_columns(vec![
            ("a".to_string(), Column::Num(vec![1.0])),
            ("b".to_string(), Column::Str(vec!["x".to_string()])),
            ("c".to_string(), Column::Num(vec![2.0])),
        ])
        .unwrap();
        let tex = t.to_tex();
        let spec_line = tex.lines().next().unwrap();
        let spec = spec_line.trim_start_matches("\\begin{tabular}{").trim_end_matches('}');
        assert_eq!(spec.len(), t.ncols());
        assert_eq!(spec, "rlr");
    }
}
