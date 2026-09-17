# Strings: case modes, wildcards, regex and scan as one surface

Design pass, 2026-09-09. Written before any implementation, per the lane
brief. Everything below was measured against a binary built from master
(`3f657702`), not read off the source.

Ahmed's ask, verbatim:

> string manipulation should be smartly done because find and replace and
> etc should follow: CaseInsensitivePreserve or sensitive etc [...] Also we
> can have `?` `*` wildcard as well regex and formatted string:
> `%dhelloworld%d`  `%d\thello world`

---

## 0. What already exists

This section exists because the lane brief described the scan direction as
"the half Qu is missing", and it is not missing. Building it again is the
expensive mistake this document is meant to prevent.

| area | today | state |
|---|---|---|
| scan | `parse_as(s, t)` / `scan(s, t)`, `%d %f %s %*` | **works** |
| regex | `regex_match`, `regex_find`, `regex_find_all`, `regex_split`, `regex_groups`, `regex_count`, `regex_replace`, `grep` | works |
| wildcards | `like(s, pat)` — `*`, `?`, `#`, `[...]`, `[!...]` | match only |
| plain | `replace(s, old, new, [count])`, `contains`, `split`, `join`, `trim`, `upper`, `lower` | no case mode |

Measured, both of Ahmed's own examples:

```qu
scan("12helloworld34", "%dhelloworld%d")   // (12, 34)   works today
scan("12<TAB>hello world", "%d<TAB>hello world")  // 12   works today
```

So part 3 of the brief is **already shipped** except for one defect (§2).
`regex` is already a dependency; no new engine is needed for any of this.

The real gaps are case modes (§3) and putting the three engines on one
footing (§4).

---

## 1. The escape question, and why the obvious answer is wrong

A pattern is written as a string literal, so the string lexer sees it first.
Qu has two literal forms since today (`bde462ae`): `"..."` decodes escapes
and interpolates `{}`; `r"..."` does neither.

The reflex from Python is "use `r"..."` for patterns". **Measured against
Qu, that advice breaks the scan direction and is unnecessary for regex.**

| pattern | `"..."` | `r"..."` | agree? |
|---|---|---|---|
| `regex_match(s, \d+)` | true | true | yes |
| `regex_match(s, \t)` | true | true | yes |
| `scan(s, %d\thello world)` | **12** | **none** | **NO** |
| `"C:\temp\new"` | tab + newline | the path | **NO** |

Two separate reasons, and they pull in opposite directions:

- **Regex converges.** `\d` survives a plain literal because an unknown
  escape keeps its backslash, and `\t` converges because a decoded TAB and
  the regex `\t` match the same character. Both forms work.
- **Scan diverges.** `scan` builds its matcher by `regex::escape`-ing the
  literal text between placeholders. In a raw literal `\t` is still two
  characters, gets escaped to a literal backslash-t, and no longer matches a
  real tab. The raw form — the one users are told to reach for — is the one
  that silently returns `none`.

`none` is the worst possible failure here: `scan` returns `none` for "this
line did not have the shape you described", so a pattern bug is
indistinguishable from a data mismatch.

### Recommendation

**The pattern language owns its escapes.** `scan` should interpret `\t`,
`\n`, `\r`, `\\` in the literal parts of its template rather than escaping
them wholesale. Then both literal forms mean the same thing everywhere, and
the question "which quoting do I use for a pattern?" stops having a
consequence — which is the actual goal, not a house style.

`r"..."` then remains the right habit for Windows paths and for regexes
containing `\\`, and stops being a trap in scan.

Argue with this before I build it. The counter-case is that it makes `\`
non-literal inside a scan template, so a template matching a literal
backslash needs `\\` — but a template is a pattern, and patterns having
escapes is the less surprising of the two rules.

### The `$` and `{` rows

Both interact with the maths/interpolation rules and belong in the same
table, because the papers lane will hit them:

- `{` after `^`, `_` or a macro **inside** a `$...$` span is a LaTeX group,
  not interpolation. Elsewhere it interpolates. A pattern containing `{`
  therefore depends on whether a `$` opened a span earlier in the string.
- Escapes are not decoded inside `$...$` at all.

Neither is a bug — both are deliberate and tested — but a pattern containing
`$` or `{` must be written `r"..."` or it means something else. That is a
documentation obligation, not a code change.

---

## 2. Defect: scan's raw-template failure

Independent of any new API. `scan(s, r"%d\thello world")` returns `none`
where the plain form returns `12`. Fixed by the §1 recommendation. Small,
self-contained, and worth landing first so the rest builds on a surface that
does not disagree with itself.

---

## 3. Case modes — the genuinely new work

`replace` has no case handling whatsoever:

```qu
replace("programme x Programme", "programme", "program")
// -> "program x Programme"      only the exact-case one
```

A **mode**, not a boolean, as the brief says:

```qu
replace(s, old, new, case="sensitive")    // default, today's behaviour
replace(s, old, new, case="insensitive")  // match any case, replacement verbatim
replace(s, old, new, case="preserve")     // match any case, replacement inherits
```

`case="preserve"` is the interesting one. Per-character inheritance:

| source | replacement | result |
|---|---|---|
| `programme` | `program` | `program` |
| `Programme` | `program` | `Program` |
| `PROGRAMME` | `program` | `PROGRAM` |
| `PrograMme` | `program` | `PrograM` |

### Degenerate cases, to be written as tests first

The brief calls for these explicitly, and Word is the reference only for the
common cases:

- **Replacement longer than the match.** `prog` -> `programme`: the pattern
  runs out. Rule: characters past the end of the source pattern inherit the
  case of the source's **last** character, so `PROG` -> `PROGRAMME` and
  `Prog` -> `Programme`.
- **Replacement shorter.** `programme` -> `prog`: truncate the pattern; no
  ambiguity.
- **Non-alphabetic characters.** Digits and punctuation have no case and
  must not consume a position in the pattern, or `a1b` would shift
  everything after the digit.
- **All-caps vs title-case detection.** `PrograMme` is neither. This is why
  it must be per-character inheritance rather than the three-way
  lower/Title/UPPER classification Word appears to use — Ahmed's `PrograMme`
  -> `PrograM` example is precisely the case a three-way classifier gets
  wrong.
- **Non-ASCII, where case is not one-to-one.** German `ß` uppercases to
  `SS` (one character to two); Turkish dotted/dotless `i` maps differently
  under a Turkish locale. **Proposal: `preserve` operates per Unicode
  scalar, uses simple (non-locale, non-expanding) case mapping, and where a
  character has no simple mapping it is emitted unchanged.** `ß` therefore
  stays `ß` rather than becoming `SS` and silently changing the string's
  length. Documented as a limit, not left to chance.

Same `case=` keyword on `contains`, `find`, `split` and `like`, so the mode
is one concept rather than a `replace` peculiarity.

---

## 4. One footing for three engines

Today a user must know which engine they are in, which is the thing the
brief says should not be necessary:

- wildcards match whole strings only (`like`), and cannot replace or find
- regex has seven `regex_*` functions
- scan has its own template syntax

**Proposal: a `pattern=` keyword on the plain functions**, rather than a
fourth family of names.

```qu
find(s, "prog*",  pattern="wildcard")
replace(s, "prog*", "X", pattern="wildcard")
replace(s, "\d+",  "N", pattern="regex")
contains(s, "prog*", pattern="wildcard")
```

with `pattern="literal"` the default, so every existing call is unchanged.
`like(s, p)` stays as the readable shorthand for
`contains(s, p, pattern="wildcard")`, and the `regex_*` family stays as the
explicit form. Nothing is removed.

This composes with `case=`: `replace(s, "prog*", "X", pattern="wildcard",
case="preserve")` is one sentence with two adjectives, which is the test of
whether the surface reads intuitively.

---

## 5. Boundary with the perf lane

The perf lane owns fast substring search; this document owns the surface.
The primitive this surface needs:

```
fn find_substring(haystack: &str, needle: &str, from: usize,
                  case: CaseMode) -> Option<usize>
```

Byte offset of the first match at or after `from`, `None` for no match.
`CaseMode::Insensitive` must not allocate a lowercased copy of the haystack
per call — that is the whole point of it living in the perf lane.

`case="preserve"` needs only `Insensitive` matching; the case-inheritance
step is this lane's and is pure string work on the matched span.

I will not implement this primitive. If the signature is wrong for their
implementation, theirs wins and I adapt.

---

## Ranked plan

| # | change | why here |
|---|---|---|
| 1 | scan interprets escapes in its template (§2) | A defect, and the surface disagrees with itself until it is fixed. Smallest change here. |
| 2 | `case=` on `replace`, with `preserve` (§3) | The actual ask. Self-contained; no dependency on the perf lane for correctness, only for speed. |
| 3 | `case=` on `contains`/`find`/`split`/`like` (§3) | Makes the mode a concept rather than one function's option. |
| 4 | `pattern=` on the plain functions (§4) | Largest surface change, and the one most worth arguing about before building. |
| 5 | Adopt the perf lane's search primitive | Pure substitution once it exists. |

## Open questions

- Should `case="preserve"` be the default for `replace`? It is what a person
  usually means, but it changes existing behaviour silently. Recommendation:
  no — default stays `sensitive`.
- Does `pattern=` belong on `split` too? `split(s, "\s+", pattern="regex")`
  is useful and `regex_split` already exists, so this may be redundant.
- Ahmed's `%dhelloworld%d` works today. Is there a case where adjacent
  placeholders with no separator are ambiguous enough to warn about? `%d%d`
  cannot be split unambiguously and currently matches greedily.
