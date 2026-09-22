# Collections, Strings & Data Frames

The tables below are generated from the interpreter's own builtin dispatch
table (the `fn call_builtin` match statement in
`engine/crates/qu-interp/src/lib.rs`), not written speculatively ahead of the
implementation — every function listed here runs today.

## Strings

All index/length arguments are 0-based (matching every other index in this
codebase) and unicode-aware: implemented on `.chars()`, never a raw byte
slice, so a multi-byte UTF-8 string indexes by character, not by byte.

| Function | Signature | Description |
|---|---|---|
| `str` | `str(x)` | Renders `x` (any value/type) the same way `print`/`disp` would. Returns a `Str`. |
| `substr`, `mid` | `substr(s, start, [length])` | `s` is a `Str`; `start` (number, 0-based character index, must lie in `[0, char_count]`) is where the slice begins; optional `length` (number of characters, default: to the end) is clamped rather than erroring if it runs past what's left. Plain aliases of the same implementation (`mid` exists purely for BASIC/Excel discoverability). Returns a `Str`. |
| `left` | `left(s, n)` | The first `n` characters of a string. `s` is a `Str`; `n` (number of characters) selects the first `n` of them, clamped to the whole string if `n` exceeds its length. Returns a `Str`. |
| `right` | `right(s, n)` | The last `n` characters of a string. `s` is a `Str`; `n` (number of characters) selects the last `n` of them, with the same clamping as `left`. Returns a `Str`. |
| `reverse` | `reverse(x)` | `x` is a `Str`, `List`, `Vec`, or `Mask`. Unicode-aware character reversal for a `Str` (same type back); element-order reversal for a `List`/`Vec`/`Mask` (same type and shape back). Returns exactly the same type and length as `x`. |
| `upper` | `upper(s)` | `s` is a `Str`. Unicode-aware uppercase conversion. Returns a `Str` the same length (in characters). |
| `lower` | `lower(s)` | `s` is a `Str`. Unicode-aware lowercase conversion. Returns a `Str` the same length (in characters). |
| `replace` | `replace(s, old, new)` | `s` is the `Str` to search; `old` and `new` are `Str`s. Every literal (non-regex) occurrence of `old` in `s` is replaced by `new`. Returns a `Str`. |
| `starts_with` | `starts_with(s, prefix)` | `s` and `prefix` are `Str`s. Returns a `bool`: `true` if `s` starts with `prefix`. |
| `ends_with` | `ends_with(s, suffix)` | `s` and `suffix` are `Str`s. Returns a `bool`: `true` if `s` ends with `suffix`. |
| `flip` | `flip(s)` | `s` is a `Str`. The same as `reverse` on a string — the word a BASIC hand reaches for. Returns a `Str`. |
| `insert` | `insert(s, i, text)` | `s` and `text` are `Str`s; `i` (number, 0-based character position) is where `text` is spliced in. Returns a new `Str`; `s` itself is untouched. |
| `remove` | `remove(s, text)` or `remove(s, start, [count])` | `s` is a `Str`. Two overloads, picked by the type of the second argument: `text` (`Str`) deletes every occurrence of it; `start` (number, 0-based index) with optional `count` (number of characters, default: to the end) deletes that run instead. Returns a `Str`. |
| `count` | `count(s, sub)` | `s` and `sub` are `Str`s. Returns a `number`: how many non-overlapping times `sub` occurs in `s` (non-overlapping so it agrees with `replace`). |
| `index_of` | `index_of(s, sub)` | `s` and `sub` are `Str`s. Returns a `number` (0-based character position of the first occurrence) or `none` if `sub` is absent. |
| `last_index_of` | `last_index_of(s, sub)` | `s` and `sub` are `Str`s. Returns a `number` (0-based character position of the last occurrence, searching from the end) or `none` if absent — so a miss cannot be mistaken for a position. |
| `pad_left` | `pad_left(s, width, [fill=" "])` | `s` is a `Str`; `width` (number, target character count); optional `fill` (single-character `Str`, default `" "`). Pads on the left to `width` characters; a string already that wide is returned unchanged, never truncated. Returns a `Str`. |
| `pad_right` | `pad_right(s, width, [fill=" "])` | Same parameters and return as `pad_left`, padding on the right instead. Returns a `Str` of at least `width` characters — `s` unchanged when it is already that long. |
| `capitalize` | `capitalize(s)` | `s` is a `Str`. Uppercases the first character, leaves the rest untouched. Returns a `Str`. |
| `proper` | `proper(s)` | `s` is a `Str`. Uppercases every word's first character, lowercases the rest — VB's `StrConv(s, vbProperCase)`. Returns a `Str`. |
| `lines` | `lines(s)` | `s` is a `Str`. Splits on either line ending, with no spurious empty last element when the text ends in a newline. Returns a `List` of `Str`. |
| `chars` | `chars(s)` | `s` is a `Str`. Returns a `List` of one-character `Str`s. |
| `repeat_str` | `repeat_str(s, n)` | `s` is a `Str`; `n` (number of copies). `s * n` is the shorter spelling for the same thing; the function is not called `repeat` because that is a loop keyword. Returns a `Str`. |
| `chr`, `ord` | `chr(n)`, `ord(s)` | `chr(n)`: `n` is a numeric Unicode code point — not a byte, so `chr(9731)` is a snowman, not an error. `ord(s)`: `s` is a `Str`; a longer string is not an error, only its FIRST character is read, so `ord("Abc")` is 65. `asc` is a second name for `ord`. Returns a one-character `Str` from `chr`, and a `number` — the code point — from `ord`. |
| `trim` | `trim(s, [chars])` | Strips characters from both ends of a string. `s` is a `Str`; optional `chars` (`Str`, default: whitespace) is a SET of characters to strip, not a substring — `trim(field, "\"' ")` strips quotes and spaces in any order from both ends. Returns a `Str`. |
| `ltrim`, `rtrim` | `ltrim(s, [chars])` | Same parameters as `trim`, restricted to one end only (`ltrim` the left, `rtrim` the right). Returns a `Str`. |
| `split` | `split(s, delim, [limit])` | `s` and `delim` are `Str`s; optional `limit` (number of pieces, default: unlimited) stops after that many pieces and leaves the remainder undivided as the last one — `split(line, ": ", 2)` takes `key: value with: colons` apart that way. Returns a `List` of `Str`. |
| `compare` | `compare(a, b, [ignore_case])` | `a` and `b` are `Str`s; optional `ignore_case` (`bool`, default `false`). Returns a `number`: −1, 0 or 1, so it can drive a sort directly. |
| `sort` | `sort(list, [descending])` | `list` is a `List` of `Str` or a numeric `Vec`; optional `descending` (`bool`, default `false`). A list of strings sorts lexicographically, a numeric vector sorts numerically. Returns the same type as `list`. |
| `ucase`, `lcase` | `ucase(s)` | `s` is a `Str`. BASIC's names for `upper`/`lower`; `toupper`/`tolower` are C's names for the same thing. Returns a `Str`. |
| `write_text` | `write_text(path, s)` | `path` and `s` are `Str`s (file path, content to write). Writes the whole string to a file, overwriting it. Returns a `number` (bytes written). `append_text` adds to the end instead; the counterpart of `read_all_text`. |
| `after`, `before` | `after(s, mark)` | `s` and `mark` are `Str`s. `after` returns the piece of `s` past the first `mark`; `before` returns the piece up to it. `after_last`/`before_last` search from the other end. A `mark` that is not present leaves `s` whole (returned unchanged), never empty. Returns a `Str`. |
| `head`, `tail` | `head(s, n)` | `s` is a `Str`, `List`, `Vec`, or `Table`; `n` (number, default 5 when used on a table). Returns the first (`head`) or last (`tail`) `n` elements/rows, same type as `s`. |
| `grep` | `grep(s, pattern, [invert=])` | `s` is a `Str` (often multi-line) or `List` of lines; `pattern` is a regex `Str`; optional named `invert` (`bool`, default `false`) selects non-matching lines instead. Returns a `List` of the matching (or non-matching) LINES — `regex_find_all` returns the matched substrings themselves, a different thing. |
| `parse_as`, `scan` | `parse_as(s, template, [must_match=])` | `s` and `template` are `Str`s. scanf backwards: `"hello world 12".parse_as("hello world %d")` gives 12. `%d` a whole number, `%f` any number, `%s` a run of non-space, `%*` anything. The template understands `\t`, `\n`, `\r` and `\\` in its literal parts, so `"%d\thello"` and `r"%d\thello"` mean the same thing; a literal backslash is doubled. Returns the extracted value (a `number`, `Str`, or a `List`/tuple of them when `template` has more than one placeholder), or `none` if the line doesn't have the shape described — never numbers pulled from a different sentence. Optional named `must_match` (`bool`, default `false`) raises an error naming the missing part of the shape instead of returning `none` — see the limit noted below. |
| `format` | `format(x, pattern)` | `x` is a `number`; `pattern` is a `Str` picture: `"0.00"`, `"#,##0.0"`, `"0.0%"`, `"0.00e0"`. Interpolation (`{x:.2f}`) reads better inline; `format` is for a pattern held in a variable. Returns a `Str`. |
| `as_text` | `as_text(v)` | `v` is a `Vec`, `List`, or `Signal`. Returns a `List` of `Str` (each element formatted the way `print` would), so `[1, 2, 3].as_text().join(", ")` works. `str` renders the whole value as one string in one shot — a different question. |
| `md2html` | `md2html(s)` | `s` is a `Str` of Markdown source. Converts headings, paragraphs, lists, fenced code with its language, blockquotes, pipe tables, rules, and inline code/bold/italic/links/images. Returns a `Str` of HTML. |
| `html2md` | `html2md(s)` | `s` is a `Str` of HTML. Converts the same subset back to Markdown; malformed HTML degrades to its text rather than erroring. Returns a `Str`. |

#### Overloads: `substr`

| Call | What it returns |
|---|---|
| `substr(s, start)` | Everything from `start` to the end of `s` — the 2-argument form. |
| `substr(s, start, length)` | Exactly `length` characters starting at `start`, clamped (not an error) if `length` runs past what's left — the 3-argument form. |

`mid` is the exact same builtin under a second, BASIC/Excel-flavored name;
either spelling accepts either form. The example below starts with the
simplest call — everything after a fixed position — then the bounded form,
then shows the clamping behavior when the requested length overruns the
string:

```qu
s = "Hello, World!"
print(substr(s, 7))          # "World!" — to the end
print(substr(s, 0, 5))       # "Hello" — bounded
print(substr(s, 7, 100))     # "World!" — clamped, not an error
print(mid(s, 7, 5))          # "World" — mid is substr under another name
```

These are the string functions reached for constantly: slicing out a piece,
swapping one substring for another, padding a value so a column of output
lines up, and fixing up capitalization on a name typed in whatever case a
user felt like. `s * n` repeats a string, in either order, and a non-whole
count is an error rather than a silent floor — the same operator overload
`3 * 4` uses, extended to strings rather than a separate `repeat_str` call
being the only spelling. The example below chains a slice into a replace,
pads a number-turned-string with leading zeros, title-cases a name, and
counts a substring's occurrences:

```qu
print("-" * 40)                                  # "----------------------------------------"
print("hello".substr(0, 2).replace("e", "i"))    # "hi"
print(pad_left(str(7), 3, "0"))                  # "007"
print(proper("ada lovelace"))                    # "Ada Lovelace"
print(count("banana", "a"))                      # 3
```

#### Overloads: `remove`

| Call | What it does |
|---|---|
| `remove(s, text)` | `text` is a `Str`. Deletes every occurrence of it from `s`. |
| `remove(s, start)` | `start` is a number (0-based character index). Deletes from `start` to the end of `s`. |
| `remove(s, start, count)` | `count` is a number of characters. Deletes exactly that many, starting at `start`, clamped rather than erroring if `count` runs past what's left. |

Picked by the TYPE of the second argument, not a flag: a `Str` means
"delete every occurrence of this text anywhere in `s`"; a number means
"delete this run of characters" instead, with an optional third argument
narrowing how many. Returns a `Str` either way:

```qu
s = "hello, world"
print(remove(s, ", "))       # "helloworld" -- every occurrence of the text
print(remove(s, 0, 5))       # ", world" -- 5 characters starting at position 0
print(remove(s, 7))          # "hello, " -- position 7 to the end, count omitted
```

### Patterns

A regular expression is the one string tool with no workaround — everything
else here can be written as a loop, and this cannot.

| Function | Signature | Description |
|---|---|---|
| `regex_match` | `regex_match(s, pattern)` | `s` and `pattern` are `Str`s (`pattern` a regular expression). Returns a `bool`: whether the pattern occurs anywhere in `s`. Anchor it with `^`/`$` to test the whole string. |
| `regex_find` | `regex_find(s, pattern)` | `s` and `pattern` are `Str`s. Returns a `Str` (the first match) or `none` — not an empty string, so "matched empty" and "did not match" stay apart. |
| `regex_find_all` | `regex_find_all(s, pattern)` | `s` and `pattern` are `Str`s. Returns a `List` of `Str` — every match, in order. |
| `regex_groups` | `regex_groups(s, pattern)` | `s` and `pattern` are `Str`s (`pattern` containing `(...)` capture groups). Returns a `List` of `Str` — the capture groups of the first match, group 1 onward; a group that did not participate is an empty string. Returns `none` if nothing matched. |
| `regex_replace` | `regex_replace(s, pattern, repl, [count])` | `s`, `pattern` and `repl` are `Str`s; optional `count` (number, default: all occurrences) caps how many replacements are made. `$1` and `${name}` in `repl` refer to capture groups. Returns a `Str`. |
| `regex_split` | `regex_split(s, pattern)` | `s` and `pattern` are `Str`s. Returns a `List` of `Str`, split on every match of the pattern. |
| `regex_count` | `regex_count(s, pattern)` | `s` and `pattern` are `Str`s. Returns a `number`: how many non-overlapping matches occur. |
| `like` | `like(s, pattern)` | `s` and `pattern` are `Str`s. BASIC's wildcard match, anchored at both ends: `*` any run, `?` any one character, `#` any digit, `[abc]`/`[!abc]` a set. Returns a `bool`. |

`regex_find_all` pulls out every run of digits in a string, `regex_replace`
rearranges a date using capture-group backreferences, `regex_groups` reads
those same captures back individually, and `like` answers a wildcard
question without writing a full pattern:

```qu
print(regex_find_all("a1b22c333", "[0-9]+"))                                          # ("1", "22", "333")
print(regex_replace("2026-09-07", "([0-9]+)-([0-9]+)-([0-9]+)", "$3/$2/$1"))          # "07/09/2026"
print(regex_groups("v1.2.3", "v([0-9]+)\.([0-9]+)"))                                  # ("1", "2")
print(like("hello.txt", "*.txt"))                                                      # true
```

`regex_match` just asks yes/no, `regex_find` returns the first match alone,
`regex_split` divides the string on the pattern, and `regex_count` tallies
non-overlapping matches:

```qu
s = "sensor_1: 12.4, sensor_2: 9.8, sensor_3: 15.0"
print(regex_match(s, "sensor_[0-9]+"))    # true
print(regex_find(s, "[0-9]+\.[0-9]+"))    # "12.4"
print(regex_split(s, ", *"))              # 3-piece split on each comma
print(regex_count(s, "sensor_"))          # 3
```

The engine is the `regex` crate, which is linear in the input by
construction: a pattern typed at a prompt cannot hang the interpreter the
way a backtracking engine can on `(a+)+b`. The price is no backreferences
and no lookaround, which is the right trade for a language you drive
interactively. A pattern that does not compile is an error naming the
character it choked on, never a silent no-match — a typo and a genuine miss
must not look the same.

`like` is translated into a regular expression rather than matched by a
second engine, so the two cannot disagree about anchoring or Unicode. Reach
for it first: most "does this look like that" questions are a wildcard
question, and a wildcard is far easier to get right.

### Chaining

Every string function is also a method on the value, so a chain reads left
to right:

```qu
s = "  Hello, Qu World!  "
trimmed = trim(s)
parts = split(trimmed, ", ")
print(parts)                  # ["Hello", "Qu World!"]
print(upper(parts[0]))        # HELLO
print(starts_with(trimmed, "Hello"))  # true
print("hello".substr(0, 2).replace("e", "i").flip().flip())
print("hi world".split(" ").join(", ") + "!")
```

### More string examples

Real text rarely arrives in the shape you want it: a filename needs a
substring pulled out of it, a heading needs consistent case, a field read
from a form needs its leading/trailing padding stripped before it is safe
to compare. This block runs one string through several of those cleanups —
slicing with `mid`/`left`/`right`, case conversion under all of its aliases
(`upper`/`lower`/`ucase`/`lcase`/`toupper`/`tolower`), and trimming/padding —
to show that they compose without any of them mutating the original:

```qu
s = "Hello, Qu World!"
print(mid(s, 7, 2))          # "Qu"
print(left(s, 5))            # "Hello"
print(right(s, 6))           # "World!"
print(reverse("abc"))        # "cba"
print(lower("SHOUT"))        # "shout"
print(ucase("shout"))        # "SHOUT"
print(lcase("SHOUT"))        # "shout"
print(toupper("mix"))        # "MIX"
print(tolower("MIX"))        # "mix"
print(capitalize("ada lovelace"))  # "Ada lovelace"
print(ltrim("   padded"))    # "padded"
print(rtrim("padded   ") + "|")   # "padded|"
print(pad_right("ab", 5, "."))    # "ab..."
print(compare("apple", "banana")) # -1
print(asc("A"))              # 65
```

Searching for a marker and cutting the string around it — `index_of`/
`last_index_of` from the front and back, `after`/`before`/`after_last`/
`before_last` to grab one side of it directly:

```qu
path = "report_final_v2.txt"
print(ends_with(path, ".txt"))         # true
print(index_of(path, "_"))              # 6
print(last_index_of(path, "_"))         # 12
print(insert("helloworld", 5, ", "))    # "hello, world"
print(remove("hello, world", ", "))     # "helloworld"
line = "key: value"
print(after(line, ": "))                # "value"
print(before(line, ": "))               # "key"
full = "a/b/c.txt"
print(after_last(full, "/"))            # "c.txt"
print(before_last(full, "/"))           # "a/b"
```

Breaking text apart into lines or characters, and the two ways of putting
it back together:

```qu
text = "one\ntwo\nthree"
print(lines(text))                     # ("one", "two", "three")
print(chars("abc"))                    # ("a", "b", "c")
print(repeat_str("ab", 3))             # "ababab"
print(as_text([1, 2, 3]).join(", "))   # "1, 2, 3"
```

`parse_as`/`scan` pull values out of a fixed-shape line of text; `format`
goes the other way, a number into a picture:

```qu
n = parse_as("hello world 12", "hello world %d")
print(n)                          # 12
reading = scan("temp = 98.6 F", "temp = %f %s")
print(reading)                    # (98.6, "F")
print(format(1234.5, "#,##0.0"))  # "1,234.5"
print(format(0.256, "0.0%"))      # "25.6%"
```

### Case modes and pattern languages

`replace` and `contains` take two independent keywords. Both default to
today's behaviour, so existing calls are unchanged.

`case=` is `"sensitive"` (default), `"insensitive"`, or `"preserve"` — where
the match is case-insensitive and the replacement inherits the case of what
it replaced, letter by letter:

```qu
replace("the programme, the Programme, the PROGRAMME",
        "programme", "program", case="preserve")
# "the program, the Program, the PROGRAM"
```

Inheritance is per character, so `PrograMme` becomes `PrograM`. Letters past
the end of the match follow its last letter (`PROG` → `PROGRAMME`);
non-alphabetic characters consume no position, so a digit cannot shift the
pattern (`A1B` → `X1Y`); and a character whose case mapping is not
one-to-one passes through unchanged, so German `ß` stays `ß` rather than
becoming `SS` and changing the string's length.

`pattern=` is `"literal"` (default), `"wildcard"`, or `"regex"`:

```qu
replace("axb a.b", "a.b", "Z")                     # "axb Z"   the dot is a dot
replace("axb a.b", "a.b", "Z", pattern="regex")    # "Z Z"
replace("a1 b2 c3", "?#", "X", pattern="wildcard") # "X X X"
```

Literal is the default because Word, `str.replace`, `strrep` and VB's
`Replace` all are: someone replacing `a.b` should not have the dot silently
mean any-character. `"wildcard"` is the same dialect `like` accepts —
`*`, `?`, `#`, `[abc]`, `[!abc]` — because both use the same translation.

The two compose: `replace(s, "prog*", "X", pattern="wildcard", case="preserve")`.

**`*` is greedy and runs to the end of the string.** `replace("the programme
here", "program*", "X", pattern="wildcard")` gives `"the X"`, not
`"the X here"` — `*` means "any run of characters" and there is nothing to
stop it. `like` behaves the same way, and making `*` lazy for `replace`
alone would give Qu two wildcard dialects. Use `?` per character, or a
regex, when you need a bounded match.

An unknown `case=` or `pattern=` value is an error naming the valid ones,
never a silent fallback to the default.

A non-match is an ordinary answer, because scanning a file where only some
lines are headers is the normal case. When you expected the line to fit,
`must_match=true` says which part of the shape was missing instead:

```qu
scan("12 goodbye world", "%d hello world", must_match=true)
# error: scan: `%d hello world` did not match `12 goodbye world`
#        -- expected ` hello world` after %d
```

**A limit worth stating.** Qu cannot tell you at parse time that a template
is impossible, and it does not try. `\d` is genuinely ambiguous — a regex
habit, or the start of a Windows path like `C:\data` — so an unrecognised
escape is kept literally rather than rejected, and
`scan("C:\data\x 42", r"C:\data\x %d")` works. The consequence is that a
template with a typo in it is a template that simply does not match, and a
bare `none` still means "did not match" without saying why. `must_match=`
exists because that question is answerable at the point of use, where the
input is in hand, and not before.

`tail` for the end of a string, `grep` for the lines of it that match a
pattern — both borrowed from their command-line namesakes:

```qu
print(tail("abcdefgh", 3))    # "fgh"
log = "INFO start\nERROR disk full\nINFO retry\nERROR timeout"
print(grep(log, "ERROR"))     # ("ERROR disk full", "ERROR timeout")
```

Sometimes the destination for a piece of text is a file rather than the
terminal — a log line, a generated report, a config value computed at
runtime. `write_text(path, s)` writes the whole string in one call and
hands back the byte count written, so a caller can confirm nothing was
truncated without a separate file-size check (to a scratch path here, not
anywhere inside a project -- `/tmp/` resolves to the system temporary
directory on every platform, including Windows):

```qu
out_path = "/tmp/qu_scratch_note.txt"
bytes = write_text(out_path, "hello from Qu\n")
print(bytes)                  # 14
```

Notes written as Markdown often need to become HTML for a report or a web
page, and HTML pasted from somewhere else sometimes needs to become plain
Markdown so it can be edited by hand. `md2html`/`html2md` convert between
the two formats for the common subset — headings, paragraphs, lists, code,
links, and emphasis — without pulling in an external converter:

```qu
md = "# Title\n\nSome **bold** text."
html = md2html(md)
print(html)
print(html2md(html))
```

## Bitwise & Number-Base Conversion

`Value` has no separate integer type — everything is `f64` — so all of these
read each argument as an integer via a strict integer check (rejecting
non-integral input rather than silently truncating it), do the operation as
`i64`, and hand back a `Num`. These are deliberately plain builtins, not
`&`/`|`-family operators: `and`/`or` already mean logical and/or in Qu, so
`|`/`&` are a clear parse error pointing here instead.

| Function | Signature | Description |
|---|---|---|
| `bitand` | `bitand(a, b)` | `a` and `b` are numbers holding integer values. Returns a `number`: the bitwise AND of `a` and `b` as 64-bit integers. |
| `bitor` | `bitor(a, b)` | `a` and `b` are numbers holding integer values. Returns a `number`: the bitwise OR. |
| `bitxor` | `bitxor(a, b)` | `a` and `b` are numbers holding integer values. Returns a `number`: the bitwise XOR. |
| `bitcmp` | `bitcmp(a)` | `a` is a number holding an integer value. Returns a `number`: the bitwise complement (`!a`) as a 64-bit integer. |
| `bitshift` | `bitshift(x, n)` | Shifts an integer's bits left or right. `x` is a number holding an integer value; `n` is a number holding an integer shift amount — positive shifts left, negative shifts right (MATLAB's single-function convention rather than separate `<<`/`>>` operators). Returns a `number`. |
| `xor` | `xor(a, b)` | `a` and `b` are any values, read by whole-value truthiness. Returns a `bool`: logical XOR, non-short-circuit like `and`/`or`. |
| `nand` | `nand(a, b)` | `a` and `b` are any values, read by whole-value truthiness. Returns a `bool`: logical NAND. |
| `nor` | `nor(a, b)` | `a` and `b` are any values, read by whole-value truthiness. Returns a `bool`: logical NOR. |
| `hex2dec` | `hex2dec(s)` | Reads a hexadecimal digit string as a number. `s` is a `Str` (a hex digit string, optional `0x`/`0X` prefix). Returns a `number`. |
| `dec2hex` | `dec2hex(n)` | `n` is a number holding an integer value. Returns a `Str`: uppercase hex digits, no prefix. |
| `bin2dec` | `bin2dec(s)` | Reads a binary digit string as a number. `s` is a `Str` of `0`/`1` digits. Returns a `number`. |
| `dec2bin` | `dec2bin(n)` | `n` is a number holding an integer value. Returns a `Str` of `0`/`1` digits, no prefix. |

Qu also accepts `0xFF`-style hex literals directly in source; `hex2dec`/
`bin2dec` are for runtime string-to-number conversion (e.g. values read from
a file or built at runtime). The block below runs each bitwise, logical, and
base-conversion function once so their outputs can be checked at a glance —
`12` and `10` chosen because their bit patterns (`1100` and `1010`) make the
AND/OR/XOR results easy to verify by hand:

```qu
flags = bitand(12, 10)          # 8   -- bits common to both
print(flags)
print(bitor(12, 10))            # 14
print(bitxor(12, 10))           # 6
print(bitcmp(0))                # -1
print(bitshift(1, 4))           # 16  -- left shift
print(bitshift(16, -2))         # 4   -- right shift
print(xor(true, false))         # true
print(nand(true, true))         # false
print(nor(false, false))        # true
print(hex2dec("FF"))            # 255
print(dec2hex(255))             # "FF"
print(bin2dec("1010"))          # 10
print(dec2bin(10))              # "1010"
```

## Collections (arrays & lists)

`remove`/`insert`/`append` return a NEW list/vector rather than mutating in
place — Qu values are immutable by convention everywhere else.

| Function | Signature | Description |
|---|---|---|
| `remove` | `remove(collection, index)` | `collection` is a `Vec` or `List`; `index` is a number (0-based position). Returns a NEW `Vec`/`List` (same type as `collection`) with the element at `index` dropped. (`remove(timer)` is a distinct, unrelated overload for deregistering a simulated-event timer.) |
| `insert` | `insert(collection, index, value)` | `collection` is a `Vec` or `List`; `index` is a number (0-based position, `index == length` appends at the end); `value` is the element to insert — a `Vec` insert requires `value` to be numeric, a `List` insert accepts anything. Returns a NEW `Vec`/`List`. |
| `append` | `append(collection, value)` | `collection` is a `Vec` or `List`; `value` is the element to add. Returns a NEW `Vec`/`List` with `value` added at the end. |
| `contains` | `contains(collection, value)` | `collection` is a `Vec` or `List`; `value` is any comparable element. Returns a `bool`: membership test via structural equality. |
| `indexof` | `indexof(collection, value)` | `collection` is a `Vec` or `List`; `value` is any comparable element. Returns a `number` (0-based index of the first match) or `Nothing` if absent (composes with `??`: `indexof(xs, v) ?? -1`). |
| `unique` | `unique(x)` | `x` is a numeric `Vec`. Returns a `Vec`: a sorted, de-duplicated copy. |
| `sort` | `sort(x, [descending])` | `x` is a numeric `Vec` or `List` of `Str`; optional `descending` (`bool`, default `false`). Returns the same type as `x`, ascending by default; uses a proper total order (`f64::total_cmp`) so a stray `NaN` cannot corrupt the ordering of the other elements around it. |
| `argsort` | `argsort(x, [descending])` | `x` is a numeric `Vec`; optional `descending` (`bool`, default `false`). Returns a `Vec` of `number` (0-based indices): the permutation that would sort `x` — the index-returning sibling of `sort`, same convention as `argmin`/`argmax`. |
| `cumsum` | `cumsum(x)` | `x` is a numeric `Vec` or `Signal`. Returns the running sum, same type/length as `x`; a `Signal` input stays a `Signal` with the same `Fs`. |
| `diff` | `diff(x)` | `x` is a numeric `Vec` or `Signal` of length N. Returns the first difference (`x[i+1] - x[i]`), length N-1, same base type as `x`; a `Signal` input stays a `Signal` with the same `Fs`. |

`contains`/`indexof` compare elements via structural equality (`values_equal`), which does handle `Str`. The `==`/`!=` operators themselves are narrower today: they fall back to numeric coercion for anything that isn't two `Vec`s or two same-type `EnumVal`s, so `"a" == "b"` currently errors rather than returning `false` — a pre-existing gap, not a documentation omission. Prefer `contains`/`indexof` (or `starts_with`/`ends_with`/`replace` for substring checks) over `==` when comparing strings.

The functions below are the everyday toolkit for a numeric vector once it
has been collected: sort it, get a de-duplicated set of its values, or find
the order that WOULD sort it (`argsort`) without disturbing the original —
useful when a second vector needs to be reordered in step with the first:

```qu
xs = [3, 1, 4, 1, 5, 9, 2, 6]
print(unique(xs))          # [1, 2, 3, 4, 5, 6, 9]
print(sort(xs))            # [1, 1, 2, 3, 4, 5, 6, 9]
print(argsort(xs))         # permutation of indices, ascending
print(sort(xs, true))      # descending
```

#### Overloads: `sort`

| Call | What it returns |
|---|---|
| `sort(list_of_str, [descending])` | `list_of_str` is a `List` in which every element is a `Str`. Sorts lexicographically. Returns a `List`. |
| `sort(vec, [descending])` | `vec` is (or coerces to) a numeric `Vec`. Sorts numerically via `f64::total_cmp`, immune to a stray `NaN`. Returns a `Vec`. |

Picked by the TYPE of the first argument, not a flag: a `List` in which
every element is a `Str` sorts as text and comes back as a `List`; anything
else is read as a numeric `Vec` and sorted as numbers, coming back as a
`Vec` — a different return type as well as a different comparison, which
matters the moment the result is chained into something that expects one
or the other:

```qu
words = ["banana", "apple", "cherry"]
print(sort(words))            # ("apple", "banana", "cherry") -- a List, lexicographic
nums = [3, 1, 4, 1, 5]
print(sort(nums))             # [1, 1, 3, 4, 5] -- a Vec, numeric
print(type(sort(words)))      # "list"
print(type(sort(nums)))       # "vector"
```

`contains`/`indexof` for membership, `cumsum` for a running total, and
`numel` for the element count:

```qu
ys = [3, 1, 4, 1, 5]
print(contains(ys, 4))     # true
print(indexof(ys, 4))      # 2
print(cumsum(ys))          # [3, 4, 8, 9, 14]
print(numel(ys))           # 5
```

## Enums

The `enum Name ... end` declaration is a small, purely symbolic enum type
shipped 2026-08-24: named, validated constants, explicitly not a
Rust/Swift-style tagged union with per-variant payloads (Qu has no static
type system yet). Variants are reached via ordinary dot-access on the
declared type (`Season.Spring`), and two `EnumVal`s compare equal only when
both their type and variant name match.

| Function | Signature | Description |
|---|---|---|
| `enum_values` | `enum_values(EnumType)` | `EnumType` is a type declared with `enum ... end` (not a string — the bare type name). Returns a `List` of `EnumVal`s: every variant of the declared type, in declaration order — makes the type a real enumeration rather than just a handful of named constants, and lets code iterate "all seasons" without hardcoding the list a second time. |

A declared enum's variants can always be named directly (`Season.Spring`),
but a function that needs to iterate every possible value — filling a
dropdown, generating a test matrix, printing a legend — needs a way to ask
the type itself what its variants are. That's what `enum_values` is for:

```qu
enum Season
    Spring, Summer, Autumn, Winter
end

seasons = enum_values(Season)
for i = 0 to length(seasons) - 1
    print(seasons[i])
end for
```

## Data Frames / Tables

A `Table` (constructed via `table(...)`/`DataFrame(...)`) is Qu's DataFrame
primitive — see `engine/crates/qu-interp/src/table.rs` for the underlying
column-oriented representation (`Column::Num`/`Column::Str`). A numeric
column of a table is read with `df.colname` (returns a plain `Vec`); a text
column has no vector-of-strings Qu value to become yet, so it can only be
used inside table operations (`filter`/`sort_by`/`group_by_agg`).

`read_csv`/`write_csv` also produce/consume `Table` values, but are
documented in [File I/O](file-io.md) since they are fundamentally file
operations — see that chapter's CSV section.

| Function | Signature | Description |
|---|---|---|
| `table`, `DataFrame` | `DataFrame(name=values, ...)` | Each named argument is a column: `values` is either a numeric `Vec` / `List` of `Str` (a real column) or a scalar `number`/`Str` (broadcast to every row). Builds a `Table` from the named columns, in the order the arguments were written; a scalar/string column broadcasts to the row count set by whichever columns are actual vectors, and vector columns must all agree in length. Returns a `Table`. |
| `table`, `DataFrame` | `table(csv_text, [sep=","], [headers=true])` | `csv_text` is a `Str` (a header line and a line per row); optional `sep` (single-character `Str`, default `","`) sets the delimiter; optional `headers` (`bool`, default `true`) — when `false`, columns are auto-named `col1`, `col2`, etc. Cells are trimmed and blank lines ignored, so the source can be aligned. Returns a `Table`. See below. |
| `nrow` | `nrow(df)` | `df` is a `Table`. Returns a `number`: the row count. `df.nrows` is property-style sugar for this — see below. |
| `ncol` | `ncol(df)` | `df` is a `Table`. Returns a `number`: the column count. `df.ncols` is property-style sugar for this — see below. |
| `head` | `head(df, [n=5])` | `df` is a `Table`; optional `n` (number of rows, default 5). Returns a `Table`: the first `n` rows. |
| `select` | `select(df, "col", ...)` | `df` is a `Table`; each remaining argument is a `Str` column name. Returns a `Table` containing only the named columns, in the order given. |
| `filter` | `filter(df, mask)` | `df` is a `Table`; `mask` is a boolean `Vec`/`Mask` the same length as `df`'s rows (typically built from a comparison, e.g. `df.x > 0`). Returns a `Table` containing only the rows where `mask` is `true`. |
| `sort_by` | `sort_by(df, "col", [descending])` | `df` is a `Table`; `"col"` is a `Str` column name; optional `descending` (`bool`, default `false`). Returns a `Table` sorted by that one column. |
| `group_by_agg` | `group_by_agg(df, "group_col", "value_col", "agg")` | `df` is a `Table`; `"group_col"`/`"value_col"` are `Str` column names; `agg` is a `Str` naming the aggregate (`"mean"` by default, plus `"sum"`, `"min"`, `"max"`, `"count"`, etc.). Returns a `Table`: one row per distinct value of `group_col`, with `value_col` aggregated. |
| `describe` | `describe(df)` | `df` is a `Table`. Pandas-style `describe()`: returns a `Table` with one row per summary statistic (`count`, `mean`, `std`, `min`, `25%`, `50%`, `75%`, `max`) and one column per NUMERIC column of `df` (text columns are skipped). Returns a `Table`, 8 rows by however many numeric columns `df` has. |

A `Table` is where several parallel columns of data live together as one
value — the shape almost every real dataset actually arrives in. The
example below builds one from three columns (two numeric, one broadcast
string), asks it its own shape, filters it down to the rows that satisfy a
condition, narrows to a subset of columns, and finally runs `describe` for
a quick statistical summary of every numeric column at once:

```qu
df = DataFrame(x=[1, 2, 3, 4, 5], y=[10, 20, 30, 40, 50], label="sample")
print(nrow(df))            # 5
print(ncol(df))             # 3
tall = filter(df, df.x > 2)
xy = select(tall, "x", "y")
print(xy)
print(describe(df))
```

### `df.nrows` / `df.ncols` — property-style row/column count

`df.nrows` and `df.ncols` read as plain (no-parens) properties, returning
exactly what `nrow(df)`/`ncol(df)` compute — pure sugar, same values. A
real column literally named `nrows`/`ncols` always takes priority: `df.col`
column access is tried first, so the property is only a fallback for the
(overwhelmingly common) case where no such column exists.

```qu
df = DataFrame(x=[1, 2, 3], y=[4, 5, 6])
print(df.nrows)             # 3, same as nrow(df)
print(df.ncols)             # 2, same as ncol(df)
```

### Writing a table the way the data arrives

`table(x = ..., y = ...)` wants the columns. Data usually arrives as rows —
a header and then a line per measurement — and giving it to the column-wise
form means transposing it by hand first, which is both tedious and a place
to make a mistake nobody will catch.

So `table` also takes one string:

```qu
runs = table("
    channel, baseline, treated
    1,       0.412,    0.240
    2,       0.508,    0.301
    3,       0.377,    0.219
")
print(runs)
print(mean(runs.baseline))
```

Every cell is trimmed and blank lines are ignored. That is what lets the
columns line up in the source without the padding reaching the table — it
matters more than it looks, because a text column would otherwise carry the
spaces and `runs.name == "steel"` would quietly be false.

A column is numeric if every cell in it parses as a number, and text
otherwise; the decision is per column, not per cell, so one stray `n/a`
turns that whole column into text rather than silently becoming a zero.

`sep=` for a different delimiter, which is what makes a table drawn with
pipes work:

```qu
alloys = table("
    name  | density | modulus
    steel | 7.85    | 200
    alu   | 2.70    | 69
", sep = "|")
print(alloys)
```

and `headers = false` when the text is bare rows, in which case the columns
are named `col1`, `col2`, and so on.

A row with the wrong number of fields is an error, not a padded row:

```
table: a row has 1 field, the header has 2
```

Silently filling the gap would make a typo look like data, which is the
one failure this form has to avoid.

For CSV that lives in a *file*, use `read_csv` — same parsing, and it takes
`decimal=` as well for comma-decimal locales. This form is for the table
you are writing down here, in the program.

## Value Constructors

The names `num`, `bool`, `str`, `vec`, `mat`, `complex`, `cvec`, `cmat`,
`signal`, `mask`, `table`, `model`, `list`, `record`, `nothing`, `enum_val`,
and `enum_type` all appear together around line 14795 of `lib.rs`, but on
inspection that block is **not** part of `call_builtin`'s dispatch at all —
it is `json_to_value`, a private helper used only by the `load(...)` builtin
to reconstruct a `Value` from the `"type"` tag written into a saved-value
JSON file by `save(...)`. None of `num`/`bool`/`vec`/`mat`/`complex`/`cvec`/
`cmat`/`mask`/`model`/`list`/`record`/`nothing`/`enum_val`/`enum_type` exist
as callable, user-facing builtins anywhere in `call_builtin` — there is no
`vec(...)`/`mat(...)`/`complex(...)` explicit-construction family; those
values are built with literals (`[1, 2, 3]`, `3 + 4j`, `{a = 1}`) or with the
domain-specific constructors documented in the other stdlib chapters
(`zeros`, `linspace`, `randn`, ...).

Three of the sixteen names ARE genuine, independently-implemented callable
builtins, documented in full above/elsewhere and unrelated to the
`json_to_value` block:

- `str(x)` — see the Strings section above.
- `signal(data, [Fs=1])` / `Signal(data, [Fs=1])` — sampled-signal
  constructor; see [Signal Processing & Filters](signal-processing.md). `Fs`
  is optional and defaults to `1.0` Hz when omitted — `signal(data)` is
  exactly `signal(data, 1.0)`.
- `table(name=values, ...)` / `DataFrame(...)` — see Data Frames above.

## Type Conversion

- `cast(value, tag)` — `value` is any Qu value; `tag` is a `Str` naming the
  target kind, using the same closed tag vocabulary a typed function
  parameter's `: tag` annotation uses (`num, bool, str, vec, mat, complex,
  cvec, cmat, signal, mask, table, model, list, record, image, timer, file,
  worker, mutex, semaphore, none`, plus `any`) — explicit conversion between
  value kinds, targeted by name. Not to be confused with `x as vector(3)`
  (`Expr::As`) — that's an unrelated shape *contract* check on an
  already-typed value, not a type conversion. `x.cast(tag)` also works, via
  the same generic `recv.method(args) == method(recv, args)` sugar every
  free function gets automatically. Returns a value of the kind named by
  `tag`. Only a deliberately scoped set of (source, target) pairings is
  wired up — anything else is a clear named error, not a silent no-op:

  | Call | Result |
  |---|---|
  | `cast(list_of_strings, "num")` | `list_of_strings` is a `List` of `Str`. Returns a `Vec` of parsed numbers, same length; errors clearly naming the first element that doesn't parse. |
  | `cast(str, "num")` | `str` is a `Str`. Returns a single parsed `number`, same parse-or-error rule. |
  | `cast(vec_or_signal, "str")` | `vec_or_signal` is a numeric `Vec` or `Signal`. Returns a `List` of formatted number-strings (`fmt_num`, the same formatter `print`/`disp` already use), same length. |
  | `cast(num, "str")` / `cast(bool, "str")` | `num`/`bool` is a single `number`/`bool`. Returns a single formatted `Str`. |
  | `cast(x, "bool")` | `x` is any value. Returns a `bool`: truthy conversion — the exact same rule `if`/`and`/`or`/the ternary already use. |
  | `cast(vec, "list")` / `cast(list_of_numbers, "vec")` | Converts between a homogeneous numeric `Vec` and a `List` of boxed numbers, same length, elementwise. |
  | `cast(vec, "signal")` / `cast(signal, "vec")` | `vec` is a numeric `Vec`; `signal` is a `Signal`. Wraps a plain `Vec` as a `Signal` (default `Fs=1.0`, matching `signal(data)`'s own default) or drops a `Signal`'s `Fs` back to a plain `Vec`, same length either way. |
  | `cast(vec_or_signal, "mask")` / `cast(mask, "vec")` | `vec_or_signal` is numeric, `mask` is boolean; both are the same length in and out. Elementwise nonzero-is-true, or the numeric `0.0`/`1.0` inverse. |

  A conversion is most often needed right after reading raw text (every CSV
  cell, every `parse_as` non-numeric field, every value pulled from JSON
  arrives as a `Str` until something converts it) or right before writing
  numbers back out as text. The example below converts a tuple of numeric
  strings into a real `Vec`, does the reverse, and shows the `.cast(...)`
  method-chain spelling working identically to the free-function form:

  ```qu
  n = cast(("1", "2.5", "-3"), "num")
  print(n)                               # [1, 2.5, -3]
  s = cast([1, 2.5, -3], "str")
  print(s)                               # ("1", "2.5", "-3")
  b = "42".cast("num")
  print(b)                               # 42, via method-chain sugar
  ```

## Data Structures

Four opaque handle types (§ data structures pass, 2026-09-01), following the
same `Arc`-wrapped shared-handle convention as `timer`/`mutex`/`channel`
elsewhere in this codebase: cloning one of these into a variable, a function
argument, or a `spawn` env snapshot shares the SAME live structure, not an
independent copy.

### `fifo(capacity)` — bounded circular FIFO buffer

A plain first-in-first-out buffer of values, distinct from the unrelated
`queue()` (a queue of *deferred jobs* for a worker pool — see
[Concurrency](concurrency.md) — which never stores a plain value at all).

**Full-buffer behavior**: `.push` on an already-full `fifo` evicts the
OLDEST element to make room — real ring-buffer/circular-queue semantics —
rather than rejecting the new value. This is the deliberate choice: Qu
already has real reject/back-pressure tools elsewhere (`semaphore`'s
`acquire`/`release`, or `channel`'s unbounded queue), so a fixed-size
"keep only the most recent N" buffer is the more broadly useful shape for
a bounded container to have, and `.is_full()` lets a script detect an
impending eviction if it needs to react first.

| Function | Signature | Description |
|---|---|---|
| `fifo` | `fifo(capacity)` | `capacity` is a number (positive integer, the buffer's fixed size, set once with no resize). Returns a `fifo` handle: a new, empty ring buffer. |
| `.push` | `f.push(value)` | `f` is a `fifo` handle; `value` is any value to store. Appends at the back; evicts the OLDEST element first if `f` is already full. Returns `Nothing`. |
| `.pop` | `f.pop()` | `f` is a `fifo` handle. Removes and returns the oldest element (same type as whatever was stored). Errors clearly on an empty fifo. |
| `.peek` | `f.peek()` | `f` is a `fifo` handle. Returns the oldest element without removing it. Errors clearly on an empty fifo. |
| `.is_full` | `f.is_full()` | `f` is a `fifo` handle. Returns a `bool`: `true` iff the next `.push` would evict. |
| `.is_empty` | `f.is_empty()` | `f` is a `fifo` handle. Returns a `bool`: `true` iff `.pop`/`.peek` would error right now. |
| `len`, `length`, `numel` | `len(f)` | `f` is a `fifo` handle. Returns a `number`: the current element count (via the same generic length dispatch every collection uses). |

A `fifo` is the right container whenever the newest N readings matter more
than the complete history — a rolling window of the last few sensor samples,
a small undo buffer, a moving-average source. The example below fills a
3-slot buffer past capacity, watches the oldest value get evicted, and reads
the rest back out:

```qu
f = fifo(3)
f.push(1)
f.push(2)
f.push(3)
f.push(4)          # evicts 1 -- buffer is now [2, 3, 4]
print(peek(f))       # 2 -- oldest element, still there
print(f.pop())      # 2
print(len(f))        # 2
```

### `double_buffer(initial)` — front/back double buffering

The standard real-time/embedded/graphics pattern: `.write(v)` only ever
touches the BACK buffer; `.read()` only ever returns the FRONT buffer;
`.swap()` atomically exchanges the two. The entire reason this exists
(rather than a plain `mutex(initial)`) is that a reader must never observe
a write that hasn't been swapped in yet.

| Function | Signature | Description |
|---|---|---|
| `double_buffer` | `double_buffer(initial)` | `initial` is any value. Returns a `double_buffer` handle; front AND back both start equal to `initial`. |
| `.write` | `b.write(value)` | `b` is a `double_buffer` handle; `value` is any value. Writes ONLY the back buffer. Returns `Nothing`. |
| `.read` | `b.read()` | `b` is a `double_buffer` handle. Returns the current FRONT buffer's value (same type as whatever was last swapped in). |
| `.swap` | `b.swap()` | `b` is a `double_buffer` handle. Atomically exchanges front and back. Returns `Nothing`. |

This is the pattern for a value a writer updates continuously while a reader
must only ever see complete, consistent frames — a sensor-fusion result, a
render target, a shared configuration snapshot. The example below writes a
new value, shows it is NOT visible yet, then swaps and shows that it is:

```qu
b = double_buffer(1)
b.write(2)
print(b.read())     # 1 -- the write above is not visible yet
b.swap()
print(b.read())     # 2 -- now it is
```

### `linked_list()` — double-ended list

A double-ended list of arbitrary values with O(1) push/pop at both ends.
Implemented as a `VecDeque`-backed deque internally, not a traditional
chain of individually heap-allocated nodes: Qu has no way for a script to
hold a reference into the middle of a list (the one scenario a real node
chain's O(1) arbitrary-position splice would ever pay for), so a
`VecDeque` gives the identical O(1) front/back complexity with far better
cache locality and no extra per-element allocation — the more honest,
useful choice for how this is actually called from Qu script code.

| Function | Signature | Description |
|---|---|---|
| `linked_list` | `linked_list()` | Takes no arguments. Returns a `linked_list` handle: a new, empty double-ended list. |
| `.push_front` | `l.push_front(v)` | `l` is a `linked_list` handle; `v` is any value. Prepends `v` in O(1). Returns `Nothing`. |
| `.push_back` | `l.push_back(v)` | `l` is a `linked_list` handle; `v` is any value. Appends `v` in O(1). Returns `Nothing`. |
| `.pop_front` | `l.pop_front()` | `l` is a `linked_list` handle. Removes and returns the front element in O(1). Errors clearly if empty. |
| `.pop_back` | `l.pop_back()` | `l` is a `linked_list` handle. Removes and returns the back element in O(1). Errors clearly if empty. |
| `.to_vec` | `l.to_vec()` | `l` is a `linked_list` handle. Returns a plain `List`: the elements materialized front to back, for interop with the rest of Qu (indexing, `length`, `print`, ...). |
| `len`, `length`, `numel` | `len(l)` | `l` is a `linked_list` handle. Returns a `number`: the current element count. |

Reach for a `linked_list` when values need to be added or removed from
EITHER end at constant cost — a sliding window, a work queue processed from
the front while new jobs arrive at the back, an undo/redo stack pair. The
example below builds a small list from both ends, then drains it from both
ends too:

```qu
l = linked_list()
l.push_back(1)
l.push_back(2)
l.push_front(0)
print(l.to_vec())   # (0, 1, 2)
print(pop_front(l)) # 0 -- removes from the front
print(pop_back(l))  # 2 -- removes from the back
print(l.to_vec())   # (1) -- what's left
```

### `graph([directed=false])` — nodes, edges, and Dijkstra's algorithm

A real node+edge graph, not a chart. Node ids are normalized the same way
`dict`'s keys are (string or number, canonicalized so `1` and `1.0` name
the same node) — `.neighbors` always hands back string ids regardless of
what kind of value was originally passed in.

| Function | Signature | Description |
|---|---|---|
| `graph` | `graph([directed=false])` | Optional named `directed` (`bool`, default `false`). Returns a `graph` handle: a new, empty node+edge graph. |
| `.add_node` | `g.add_node(id)` | `g` is a `graph` handle; `id` is a `Str` or `number` node identifier (canonicalized like `dict` keys, so `1` and `1.0` name the same node). Adds a node; idempotent — adding an existing node is a no-op. Returns `Nothing`. |
| `.add_edge` | `g.add_edge(a, b, [weight=1.0])` | `g` is a `graph` handle; `a`/`b` are node ids (`Str`/`number`); optional named `weight` (`number`, default `1.0`). Adds an edge, auto-creating either endpoint if missing. Undirected (the default) stores the edge in both directions. Returns `Nothing`. |
| `.neighbors` | `g.neighbors(id)` | `g` is a `graph` handle; `id` is a node id. Returns a `List` of `Str` neighbor ids, in the order their edges were added (always strings regardless of what kind of id was originally passed in). Errors if `id` isn't a node. |
| `.has_edge` | `g.has_edge(a, b)` | `g` is a `graph` handle; `a`/`b` are node ids. Returns a `bool` — `false` (not an error) if either endpoint isn't even a node. |
| `.shortest_path` | `g.shortest_path(source, target)` | `g` is a `graph` handle; `source`/`target` are node ids. Returns a `number` (the shortest total edge weight from `source` to `target`, via Dijkstra's algorithm) or `Nothing` if unreachable. Errors on a negative edge weight or an unknown node. |
| `len`, `length`, `numel` | `len(g)` | `g` is a `graph` handle. Returns a `number`: the node count. |

Dijkstra (over a plain unweighted BFS) was chosen specifically because
edges carry a real optional `weight=` — a weighted-shortest-path algorithm
is the one that actually exercises that part of the data model. The first
example below builds a small weighted graph and asks for the cheapest route
between two nodes:

```qu
g = graph()
g.add_edge("A", "B", weight=4)
g.add_edge("A", "C", weight=1)
g.add_edge("C", "B", weight=2)
g.add_edge("B", "D", weight=5)
g.add_edge("C", "D", weight=8)
print(g.shortest_path("A", "D"))   # 8, via A -> C -> B -> D (1 + 2 + 5)
```

`add_node` on its own for an isolated node, `has_edge` to ask whether two
nodes are joined, `neighbors` to list what one node connects to:

```qu
h = graph()
add_node(h, "X")          # an isolated node, no edges yet
h.add_edge("X", "Y")
print(has_edge(h, "X", "Y"))   # true
print(has_edge(h, "X", "Z"))   # false -- Z isn't even a node
print(neighbors(h, "X"))       # ("Y")
```

## More functions

| Function | Signature | Description |
|---|---|---|
| `and`, `or` | `a and b` | `a` and `b` are any values, read by whole-value truthiness. Returns a `bool`. Both **short-circuit**: the right side is not evaluated when the left already decides the answer, so `i < len(xs) and xs[i] > 0` is a safe guard. |
| `not` | `not a` | `a` is any value. Returns a `bool`: logical negation. Anything falsy — `false`, `0`, `none`, an empty string, an empty collection — negates to `true`. |
| `mod` | `a mod b` | `a`/`b` are numbers. Returns a `number`: the remainder with the sign of the divisor, so `-1 mod 3` is 2 rather than -1. A keyword, not a function. |
| `print`, `disp`, `echo`, `writeline` | `print(x, ...)` | Each `x` is any value. Writes its arguments, space-separated, and a newline, to stdout. Returns `Nothing`. All four names are the same function. Strings interpolate: `print("n = {n}")`. |
| `input` | `input(n)` | **Not a stdin reader despite the name** — Qu has no line-reading builtin today. `n` (number, a positive integer — 0 or a non-integer errors) is the input dimension of a new neural-network pipeline. Returns a `Record` shaped `{kind: "pipeline_input" (string), out_dim: n (number), layers: [] (empty List)}`, meant to be piped into `dense(out_dim, [activation=])` stages and finished with `compile(optimizer=, loss=)` or `sequential(...)` — see [Statistics & ML](statistics-ml.md) for the full training example. |
| `type` | `type(x)` | `x` is any value. Returns a `Str`: the value's type, e.g. `"number"`, `"string"`, `"vector"`, `"matrix"`, `"table"`, `"layer"`. |
| `sizeof` | `sizeof(x)` | **Deprecated — use `shape`.** The same function under a name that means a byte count in every language it could have come from. Still works, and warns once per run. Returns a 2-element `Vec`, `[rows, cols]` — the same value `shape` gives. |
| `cast` | `cast(x, type)` | `x` is any value; `type` is a `Str` naming the target: `num`, `bool`, `str`, `vec`, `mat`, `complex`, `cvec`, `cmat`, `signal`, `mask`, `table`, `model`, `list`, `record`, `image`, `timer`, `file`, `worker`, `mutex`, `semaphore`, `none`. Returns a value of that kind. A conversion that is not defined is an error, not a best effort. |
| `to_int` | `to_int(x)` | `x` is a number, or a `Str` that parses as one. Rounds to the nearest whole number, ties away from zero — the same primitive `round` uses, so the two agree by construction rather than by coincidence. MATLAB's `int32` rounds; C's cast truncates; this rounds, and says so. A string that is not a number is a clear error, never a silent `0`. Returns a `Num`. |
| `to_float` | `to_float(x)` | `x` is a number, a `Bool` (giving `1`/`0`), or a `Str` that parses as one. The parsing half of `to_int` without the rounding — mostly for turning text read from a file or an instrument into a number. A string that is not a number, or a value of any other type, is a clear error rather than a `0`. Returns a `Num`. |
| `to_bool` | `to_bool(x)` | For a `Str`, `x` is **parsed**, not weighed: `"true"`/`"false"` (any case) and any number spelling work, and anything else errors — because a config file holding `"false"` means false, and truthiness would call that non-empty string true. For every other type this is exactly Qu's own truthiness, the same rule `if` and `while` use. Returns a `Bool`. |
| `complex` | `complex(re, im)` | `re`/`im` are numbers (real and imaginary parts). Returns a `Complex`. `3 + 4j` is the literal form; this is for computed components. |
| `all`, `any` | `all(xs, predicate)` | `xs` is a `Vec`/`List`; `predicate` is a one-argument function returning a `bool` — the function itself (`is_even`, `sqrt`, `(x) := x > 2`), or a `Str` naming one you defined. Returns a `bool`: whether every (`all`) or at least one (`any`) element satisfies it. As a **quoted name** a builtin is refused, because a string is indistinguishable from a typo; passed as a **value**, without quotes, it is allowed. |
| `first`, `last` | `first(xs)` | `xs` is a `Vec`/`List`. Returns the first (`first`) or last (`last`) element (same element type as `xs` holds), or `none` when `xs` is empty. Check for `none` if an empty input is possible; nothing further down will do it for you. |
| `take`, `drop` | `take(xs, n)` | `xs` is a `Vec`/`List`; `n` is a number of elements. Returns the same type as `xs`: the first `n` elements (`take`) or everything after them (`drop`). `n` beyond the length is clamped, not an error. |
| `distinct` | `distinct(xs)` | `xs` is a `Vec`/`List`. Returns the same type: the unique values, in the order they first appear — which `unique` does not promise, since it sorts. |
| `flatten` | `flatten(xs)` | `xs` is a `List` of `List`s (one level of nesting). Returns a flat `List`: one level of nesting removed. |
| `fold`, `reduce` | `fold(xs, f, initial)` | `xs` is a `Vec`/`List`; `f` is a two-argument function — the function itself, or a `Str` naming one you defined; `initial` is the starting accumulator value (`fold` only — `reduce` has no `initial` and instead starts from `xs`'s first element). Returns the final accumulated value (whatever type `f` produces). |
| `map`, `pmap` | `map(xs, f)` | `xs` is a `Vec`/`List`; `f` is a one-argument function — the function itself (`double`, `sqrt`, `(x) := x * 2`), or a `Str` naming one you defined. Returns a `List`: `f` applied to every element. The **collection comes first**, like every other higher-order builtin, which is what makes `xs.map(f)` and `xs |> map(f)` work; `map` took the function first until 2026-09-09 and that order still runs, with a notice once per run. `pmap(xs, f)` is the parallel form, same signature, and carries a lambda to its workers along with everything else it snapshots. |
| `zip` | `zip(a, b, ...)` | Each of `a`, `b`, ... is a `Vec`/`List`. Returns a `List` of tuples, pairing up the collections element by element, stopping at the shortest. |
| `dict` | `dict()` or `dict(k, v, ...)` | With no arguments: an empty store. Otherwise `k`/`v` are parallel `Vec`/`List` of keys and values (`Str` or `number` keys). Returns a `Dict`, insertion-ordered. Keys are compared as given, so a number and the string of that number are different keys. |
| `get`, `set` | `get(d, key)` / `set(d, key, value)` | `d` is a `Dict`; `key` is a `Str`/`number`; `value` (for `set`) is any value. `get` returns the stored value (any type) or `none` if `key` is absent. `set` returns a **new** `Dict` with that entry written — it does not modify `d` in place, so assign the result. Returns the stored value (any type) or `none` from `get`, and a new `Dict` from `set`. |
| `keys`, `values`, `items` | `keys(d)` | `d` is a `Dict`. Returns a `List`: its keys (`keys`), its values (`values`), or `(key, value)` tuples (`items`) — insertion order, not sorted. |
| `has_key` | `has_key(d, key)` | `d` is a `Dict`; `key` is a `Str`/`number`. Returns a `bool`: whether the dict has that key, without reading it. |
| `at` | `at(t, i)` or `t.at(i)` | `t` is a `Table`; `i` is a number (0-based row position). Returns a `Record`: one row of the table by position, always positional even when the table carries a string index. |
| `index` | `index(t, "col")` | `t` is a `Table`; `"col"` is a `Str` column name. Returns a `Table`: `t` with its row index set to that column, so rows can afterwards be looked up by label. |
| `insert_column` | `insert_column(t, name, values, [pos=])` | `t` is a `Table`; `name` is a `Str`; `values` is a numeric `Vec`/`List` of `Str` (or a scalar to broadcast) the same length as `t`'s rows; optional named `pos` (number, default: append at the end) sets the column's position. Returns a NEW `Table`; the original is untouched. |
| `drop_row` | `drop_row(t, i)` or `drop_row(t, [i, j])` | `t` is a `Table`; `i`/`j` are numbers (0-based row positions), or a `Vec` of them. Returns a `Table` without those rows, by position. |
| `insert_row` | `insert_row(t, i, record)` | Inserts one row into a table at a chosen position. `t` is a `Table`; `i` is a number (0-based row position, `i` equal to the row count appends); `record` is a `Record` supplying one value per existing column, matched by field name. Returns a NEW `Table`. |
| `swap`, `read` | `swap(b)` / `read(b)` | `b` is a `double_buffer` handle. `swap` atomically exchanges front and back (returns `Nothing`); `read` returns the stable front value. Writers fill the spare half and `swap` when the frame is whole; a reader never sees a partly-written frame. Returns the stable front value from `read` — whatever type was written into the buffer; `swap` hands back no value. |
| `append_text`, `append_all` | `append_text(path, s)` / `append_all(path, s)` | `path`/`s` are `Str`s. Appends `s` to the end of the file at `path`, creating it if absent. `write_text` replaces the whole file instead. `append_all` is the same builtin under the name that pairs with `read_all_text` — whole file in, whole string out, no handle in either direction — and both spellings stay, since `append_text` is already in scripts. Returns a `number`: the bytes written. |
| `after_last`, `before_last` | `after_last(s, mark)` | `s`/`mark` are `Str`s. Returns a `Str`: like `after`/`before` but searching from the end — the pair to reach for on a path or a dotted name. |
| `asc` | `asc(s)` | `s` is a `Str`. Returns a `number`: the code point of its first character. A second name for `ord`, because it is what a BASIC hand types. |
| `toupper`, `tolower` | `toupper(s)` | `s` is a `Str`. Returns a `Str`: C's names for `upper`/`lower`, accepted so a ported line does not have to be edited. |
| `to_vec` | `to_vec(ll)` | `ll` is a `linked_list` handle. Returns a `List`: the linked list's elements, front to back. |
| `push`, `pop`, `peek` | `push(q, x)` / `pop(q)` / `peek(q)` | On a `fifo` handle `q`: `push(q, x)` adds `x` at the back (returns `Nothing`); `pop(q)` removes and returns the front element; `peek(q)` looks at it without removing. `pop` on an empty FIFO is an error. On a `queue()` — the job queue — `push(q, "fname")` enqueues a function you defined instead (a `Str` naming it, not a value). Returns the front element itself from `pop`/`peek`, whatever type was pushed; `push` hands back no value. |
| `push_back`, `push_front` | `push_back(l, x)` | `l` is a `linked_list` handle; `x` is any value. Grows `l` at the back (`push_back`) or front (`push_front`) in O(1). Returns `Nothing`. |
| `pop_back`, `pop_front` | `pop_front(l)` | `l` is a `linked_list` handle. Removes and returns the element from the front (`pop_front`) or back (`pop_back`) in O(1). `to_vec` turns the result back into an ordinary `List`. Returns the removed element itself, whatever type was pushed; the list is one shorter afterwards. |
| `add_node`, `add_edge` | `add_node(g, name)` | `g` is a `graph` handle; `name` is a `Str`/`number` node id; `add_edge(g, a, b, [weight=1.0])` also takes a second node id and an optional `weight` (`number`, default `1.0`). Nodes are named; an edge carries an optional weight, defaulting to 1. Returns `Nothing` from both — the graph handle `g` is what changed. |
| `has_edge`, `neighbors` | `neighbors(g, name)` | `g` is a `graph` handle; `name`/`a`/`b` are node ids. `has_edge(g, a, b)` returns a `bool`; `neighbors(g, name)` returns a `List` of `Str` node ids. Ask about the graph: whether two nodes are joined, and what one node is joined to. Returns a `Bool` from `has_edge`, and a `List` of node ids from `neighbors`. |
| `shortest_path` | `shortest_path(g, from, to)` | `g` is a `graph` handle; `from`/`to` are node ids. Returns a `number` (the shortest total edge weight, via Dijkstra) or `none` if `to` is unreachable from `from` — disconnected is an answer, not an error. Errors instead if `from`/`to` isn't even a node in `g`. Same free-function form as `.shortest_path`, **not** the sequence of node names along the route. |

`input` is where a neural-network pipeline starts — you reach for it when
building a `sequential` model layer by layer with `|>` instead of hand-rolling
weight matrices. `input(3)` alone just records the dimension and an empty
layer list; piping it through `dense(...)` stages (documented in
[Statistics & ML](statistics-ml.md)) grows that list one layer at a time,
each new record replacing the running `out_dim` with the layer's own output
size:

```qu
spec = input(3)
print(spec.out_dim)          # 3 -- nothing built yet
print(spec.layers)           # () -- empty

net = input(3) |> dense(5, activation="relu") |> dense(1)
print(net.out_dim)            # 1 -- the last layer's output size
print(length(net.layers))     # 2 -- one record per dense() stage
```

`and`/`or`/`mod` as operators, and the `print` family/`type`/`shape` for
inspecting a value:

```qu
i = 2
xs = [10, 20, 30]
print(i < length(xs) and xs[i] > 0)  # true -- short-circuiting guard
print(-1 mod 3)                       # 2 -- sign of the divisor
disp("disp, echo, writeline, and print are the same function")
echo("this line came from echo(...)")
writeline("and this one from writeline(...)")
print(type(3.5))                      # "number"
print(type("hi"))                     # "string"
print(shape([1, 2, 3]))               # [3, 1]
```

`all`/`any` test a predicate over a collection; `first`/`last`/`take`
slice it; `distinct`/`flatten` reshape it; `fold`/`reduce`/`map`/`zip`
combine it with a named function:

```qu
function is_even(x)
    return x mod 2 == 0
end function

function add2(a, b)
    return a + b
end function

function double(x)
    return x * 2
end function

xs = [1, 2, 3, 4, 5]
print(all(xs, is_even))           # false
print(any(xs, is_even))           # true
print(first(xs))                 # 1
print(last(xs))                  # 5
print(take(xs, 3))                # (1, 2, 3)
print(distinct([1, 1, 2, 2, 3])) # (1, 2, 3) -- first-seen order
print(flatten([[1, 2], [3, 4]]))  # (1, 2, 3, 4)
print(fold(xs, add2, 0))          # 15 -- with an initial value
print(reduce(xs, add2))           # 15 -- without one
print(map(xs, double))            # (2, 4, 6, 8, 10) -- collection first
print(map(xs, (x) := x * 10))     # (10, 20, 30, 40, 50) -- or write it inline
print(map(xs, "double"))          # (2, 4, 6, 8, 10) -- the name as a string still works
print(xs.map(double))             # (2, 4, 6, 8, 10) -- and so the method form works
print(zip([1, 2, 3], ["a", "b", "c"]))  # ((1, "a"), (2, "b"), (3, "c"))
```

`dict`/`get`/`set`/`keys`/`values`/`items`/`has_key` build and inspect a
key-value store:

```qu
d = dict(["a", "b"], [1, 2])
d = set(d, "c", 3)
print(get(d, "b"))       # 2
print(keys(d))           # ("a", "b", "c")
print(values(d))         # (1, 2, 3)
print(items(d))          # (("a", 1), ("b", 2), ("c", 3))
print(has_key(d, "c"))   # true
print(has_key(d, "z"))   # false
```

`at`/`index`/`insert_row` on a table -- one row by position, setting a
label column as the row index, and adding a new row:

```qu
df = DataFrame(id=[1, 2, 3], name=["alu", "steel", "brass"])
print(at(df, 1))                              # {id = 2, name = "steel"}
labeled = index(df, "name")
print(labeled)
grown = insert_row(df, 3, {id=4, name="glass"})
print(grown)
```

`append_text` adds to the end of a file without disturbing what is
already there (a scratch path here, not anywhere inside a project):

```qu
path = "/tmp/qu_scratch_append.txt"
write_text(path, "first line\n")
append_text(path, "second line\n")
print(read_all_text(path))   # "first line\nsecond line\n"
```
