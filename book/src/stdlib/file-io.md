# File I/O

The tables below are generated from the interpreter's own builtin dispatch
table (the `fn call_builtin` match statement in
`engine/crates/qu-interp/src/lib.rs`), not written speculatively ahead of the
implementation — every function listed here runs today.

> **Note — WASM has no filesystem at all.** Qu's browser target
> (`wasm32-unknown-unknown`) has no native filesystem access — that's an
> Emscripten-target feature, not this one. Every builtin on this page that
> touches a PATH (`fopen`, `read_csv`, `write_csv`, ...) is native-only; a
> browser build can only reach files through JS-mediated mechanisms like the
> File System Access API or a plain `<input type=file>` read. See Book 3, "A
> hard platform boundary," for the full reasoning. The "Data-Format
> Conversion" builtins below (`jsonify`/`csvify`/`xmlify`/`parse_json`/
> `parse_csv`/`parse_xml` and the pairwise converters) are the one exception
> on this page: they only ever touch in-memory strings, no path/file
> involved, so this platform boundary doesn't apply to them.

## Opening & Closing Files

`fopen`/`close` are a general-purpose text/binary file-handling pair,
returning/consuming an opaque `Value::File` handle. `mode` follows MATLAB's
own `fopen` convention. Text and binary modes are treated identically at the
byte level — the `b` suffix is accepted for MATLAB-familiar scripts, but
`"r"` and `"rb"` behave the same way internally.

| Function | Signature | Description |
|---|---|---|
| `fopen` | `fopen(path, mode)` | Opens a file for reading, writing, or appending. `path` (string) is the filesystem path. `mode` (string) is one of `"r"` (read, errors if the file doesn't exist), `"w"` (write, truncate-or-create), `"a"` (append, create-if-missing), or the same three with a `"b"` suffix (`"rb"`/`"wb"`/`"ab"`) for binary — text and binary modes behave identically at the byte level. Returns an opaque `Value::File` handle to pass to every other builtin in this chapter. |
| `close` | `close(f)` | Flushes and releases a file handle. `f` (file handle, from `fopen`/`StreamFile`) is the handle to release. Returns `Nothing`. Reading/writing a closed handle errors clearly afterward rather than silently no-op'ing or panicking. Any pending partial `write_bit` byte is flushed, zero-padded. Calling `close` again on an already-closed handle is a harmless no-op. |
| `touch` | `touch(path)` | Unix `touch` semantics. `path` (string) is the filesystem path: creates an empty file if it doesn't exist; if it already exists, updates its modification time WITHOUT altering its content. Returns `Nothing`. Uses a real mtime-set syscall (via the `filetime` crate) rather than the unreliable "open for append, write nothing" trick, which does not reliably bump mtime on every platform/filesystem. |

> **Note — `"r"` mode reads are a real stream, not a whole-file buffer.**
> Opening a file for reading no longer loads its entire contents into memory
> up front — `fopen(path, "r")` opens a real buffered stream
> (`std::io::BufReader`) and only reads as many bytes as `read_line`/
> `read_all`/`read_char`/`seek`/etc. actually ask for. This matters for large
> files: a multi-hundred-MB (or larger) file can be opened and processed
> line-by-line or byte-by-byte without ever holding the whole thing in
> memory at once. Every read builtin's return values are unaffected — this
> is purely an internal memory-use improvement, not a behavior change
> (`write`/`append` modes were never eager-loading anything and are
> untouched).

> **Note — a different `close` exists for plotting.** The bare command
> `close` (no parentheses, used as a plot-window verb alongside `pan`/`zoom`/
> `colorbar`) is a separate, unrelated no-op accepted for GUI-script
> compatibility in headless static exports. It lives in a completely
> different match statement (the plot-command dispatch) from the
> `close(f)` file-handle builtin documented here.

## Text I/O

Every read returns `Nothing` — Qu's "absent" marker (composes with `??`,
e.g. `read_line(f) ?? "done"`) — once nothing is left to read, except
`read_all`, where an empty string is itself a legitimate non-exceptional
result.

| Function | Signature | Description |
|---|---|---|
| `read_line` | `read_line(f)` | Reads one line from `f` (file handle, opened readable). Returns a string holding the line's content without its trailing newline (`\r\n` or `\n`), or `Nothing` once there is no more content to read. |
| `read_all` | `read_all(f)` | Reads every remaining byte from `f` (file handle, opened readable). Returns the remaining content as one string — possibly the empty string `""` if called right at EOF, which is a legitimate result here, not the `Nothing` sentinel every other read in this section returns. |
| `read_until` | `read_until(f, delimiter)` | Reads from `f` (file handle, opened readable) up to and including the next occurrence of `delimiter` (string), consuming it. Returns a string of everything read, with `delimiter` stripped off the end. If `delimiter` never appears again before EOF, returns everything remaining instead of erroring. |
| `read_chars` | `read_chars(f, n)` | Reads exactly `n` (number, count of Unicode characters, UTF-8 aware) characters from `f` (file handle, opened readable). Returns a string of up to `n` characters — fewer than `n` if the file ends first — or `Nothing` if already at EOF. |
| `read_char` | `read_char(f)` | Reads one character from `f` (file handle, opened readable), UTF-8 aware. Returns a one-character string, or `Nothing` at EOF. |
| `write_line` | `write_line(f, s)` | Writes `s` (string) to `f` (file handle, opened writable/appendable) plus a trailing `\n`. Returns `Nothing`. |
| `write_char` | `write_char(f, s)` | Writes `s` (string) to `f` (file handle, opened writable/appendable) with no newline added. Returns `Nothing`. |

## Interactive input

Not a file handle at all — reads one line typed by whoever is running the
script right now, the way MATLAB's `input`/Python's `input()` do. Added in
v0.3.0.

| Function | Signature | Description |
|---|---|---|
| `read_input` | `read_input([prompt])` | Reads one line of interactive input, returned as a string with its trailing newline stripped. `prompt` (string, optional), if given, is printed first with no added newline. In a real terminal (`qu run`/`qu repl`) this reads real stdin directly; a host that isn't a terminal (e.g. a Jupyter kernel) can wire its own prompt/reply round-trip instead — see that host's own documentation for what "interactive" means there. Not called `input` (already the machine-learning pipeline's input-layer builtin) or `read_line` (already "read one line from an open file/URL-stream/serial handle") — both are unrelated existing meanings, not available for this. |

Writing to a handle opened in `"r"` mode errors clearly rather than
silently no-op'ing or panicking; the same holds for every write builtin
below.

## Position Control & Non-Advancing Peeks

`seek`/`tell`/`rewind`/`eof` control a readable handle's read cursor
directly; `peek_line`/`peek_char`/`peek_byte` (§ 2026-09-01) read the next
unit exactly the way `read_line`/`read_char`/`read_byte` would, WITHOUT
moving that cursor — a second `peek_*` call, or the very next real
`read_*` call, sees the identical data again. Implemented as a real
save-position/read/restore-position round trip around the same buffered
reader `seek`/`tell` already control (not a hand-rolled one-item lookahead
buffer), so it composes correctly with everything else that touches the
cursor.

All of these are restricted to **readable** handles (the same restriction
`seek`/`tell`/`rewind` already have) — a write-only or append-mode handle
has no tracked read cursor to save/restore or report.

| Function | Signature | Description |
|---|---|---|
| `seek` | `seek(f, offset, [whence="start"])` | Moves `f`'s (file handle, opened readable) read cursor. `offset` (number, byte count) is the distance to move. `whence` (string, optional, default `"start"`) is `"start"`, `"current"`, or `"end"`, matching C's `fseek` — the point `offset` is measured from. Returns `Nothing`; errors if the resulting position falls outside `[0, file length]`. |
| `tell` | `tell(f)` | Reports `f`'s (file handle, opened readable) current read position. Returns a number: the current byte-aligned offset from the start of the file. |
| `rewind` | `rewind(f)` | Resets `f`'s (file handle, opened readable) read cursor to the beginning — exactly `seek(f, 0)`. Returns `Nothing`. |
| `eof` | `eof(f)` | Checks whether `f` (file handle, opened readable) has any bytes left to read from the current position. Returns a boolean: `true` iff none remain. |
| `peek_line` | `peek_line(f)` | Looks at the next line of `f` (file handle, opened readable) without consuming it. Returns exactly what `read_line(f)` would return (a string, or `Nothing` at EOF), but leaves the read cursor exactly where it was. |
| `peek_char` | `peek_char(f)` | Looks at the next character of `f` (file handle, opened readable) without consuming it. Returns exactly what `read_char(f)` would return (a one-character string, or `Nothing` at EOF), cursor restored. |
| `peek_byte` | `peek_byte(f)` | Looks at the next raw byte of `f` (file handle, opened readable) without consuming it. Returns exactly what `read_byte(f)` would return (a number 0-255, or `Nothing` at EOF), cursor restored. |

Three distinct peek functions — matching `read_line`/`read_char`/
`read_byte`'s own three existing granularities one-for-one — rather than a
single generic `peek()`: this codebase already made that same naming
choice on the read side (there is no bare `read()`), and a lone `peek()`
would additionally collide with the unrelated `fifo` collection's own
`.peek()` method (see [Data Structures](collections-strings.md#data-structures)),
which answers a completely different question.

Reach for `peek_line` whenever a parser needs to look at the next line before deciding whether to actually consume it — a "does this file start with a header row?" check, or a loop that groups lines until it sees one that doesn't belong. The example below opens a three-line log file and shows that `peek_line` and the following `read_line` see the identical first line, and only the read cursor moves once a real `read_line` call happens:

```qu
# The three-line file this reads is written here. It used to open a
# log.txt that the chapter does not create until 700 lines further down,
# so this block failed and was dropped from the page while the build
# reported success.
w = fopen("log.txt", "w")
write_line(w, "a")
write_line(w, "b")
write_line(w, "c")
close(w)

f = fopen("log.txt", "r")
first_line = peek_line(f)          # doesn't consume anything
same_line  = read_line(f)          # sees first_line again
next_line  = read_line(f)          # only NOW does this move on
print("{first_line} | {same_line} | {next_line}")
close(f)
# a | a | b
```

`peek_char`/`peek_byte` work the same way at their own granularity — a
character peek for text decisions, a byte peek for binary ones — and both
compose cleanly with `read_until`/`read_chars`/`read_byte` on the same
handle, because every one of these calls shares the same underlying cursor.
The example below writes `"Hello,World"` to a file with no newline, then
walks it back with a mix of consuming reads and non-consuming peeks,
printing every intermediate value so each call's effect (or lack of one)
on the cursor is visible:

```qu
f = fopen("greeting.txt", "w")
write_char(f, "Hello,World")
close(f)

g = fopen("greeting.txt", "r")
first       = read_char(g)         # "H"
peeked      = peek_char(g)         # "e" -- cursor not moved
next        = read_char(g)         # "e" again, same char peeked returned
chunk       = read_chars(g, 3)     # "llo"
next_byte   = peek_byte(g)         # 44, the byte for ',' -- still not consumed
before_comma = read_until(g, ",")  # "" -- the comma was right there, now consumed
w_byte      = read_byte(g)         # 87, the byte for 'W'
rest        = read_all(g)          # "orld"
close(g)
print("{first}{next}{chunk} peeked={peeked} next_byte={next_byte} before_comma=\"{before_comma}\" w_byte={w_byte} rest={rest}")
# Hello peeked=e next_byte=44 before_comma="" w_byte=87 rest=orld
```

## Binary I/O

Typed reads/writes accept an `endian=` keyword argument (`"little"` default,
or `"big"`), implemented with Rust's own `from_le_bytes`/`to_le_bytes` (and
their `_be_` counterparts) — no hand-rolled byte shuffling. A typed read
returns `Nothing` if fewer bytes remain than the type needs.

| Function | Signature | Description |
|---|---|---|
| `read_byte` | `read_byte(f)` | Reads one raw byte from `f` (file handle, opened readable). Returns a number in `0..=255`, or `Nothing` at EOF. |
| `read_bit` | `read_bit(f)` | Reads one bit from `f` (file handle, opened readable), MSB-first, advancing a sub-byte cursor that only moves to the next byte once all 8 bits of it are consumed. Returns a number, `0` or `1`, or `Nothing` at EOF. |
| `read_int16` | `read_int16(f, [endian=])` | Reads a signed 16-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 2 bytes remain. |
| `read_uint16` | `read_uint16(f, [endian=])` | Reads an unsigned 16-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 2 bytes remain. |
| `read_int32` | `read_int32(f, [endian=])` | Reads a signed 32-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 4 bytes remain. |
| `read_uint32` | `read_uint32(f, [endian=])` | Reads an unsigned 32-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 4 bytes remain. |
| `read_int64` | `read_int64(f, [endian=])` | Reads a signed 64-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 8 bytes remain. Every `Value` here is an `f64`, exact only up to `2^53` — a value outside `-2^53..=2^53` (about 9 quadrillion) comes back rounded to the nearest representable double, silently losing its low bits, the same limitation any language whose only numeric type is a double has (JavaScript's `Number`, for one). |
| `read_uint64` | `read_uint64(f, [endian=])` | Reads an unsigned 64-bit integer from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 8 bytes remain. Same `2^53` exact-precision ceiling as `read_int64`. |
| `read_float` | `read_float(f, [endian=])` | Reads a 32-bit IEEE-754 float from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 4 bytes remain. |
| `read_double` | `read_double(f, [endian=])` | Reads a 64-bit IEEE-754 float from `f` (file handle, opened readable). `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns a number, or `Nothing` if fewer than 8 bytes remain. |
| `write_byte` | `write_byte(f, b)` | Writes one raw byte to `f` (file handle, opened writable/appendable). `b` (number) must be in `0..=255`, or the call errors. Returns `Nothing`. |
| `write_bit` | `write_bit(f, b)` | Writes one bit to `f` (file handle, opened writable/appendable). `b` (number) must be `0` or `1`, accumulated MSB-first into a pending byte and flushed to disk every 8 bits (`close(f)` flushes any final partial byte, zero-padded). Returns `Nothing`. |
| `write_int16` | `write_int16(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 16-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `write_uint16` | `write_uint16(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 16-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `write_int32` | `write_int32(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 32-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `write_uint32` | `write_uint32(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 32-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `write_int64` | `write_int64(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a signed 64-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. Same `2^53` exact-precision ceiling on `n` as `read_int64`. |
| `write_uint64` | `write_uint64(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as an unsigned 64-bit integer. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. Same `2^53` exact-precision ceiling on `n` as `read_int64`. |
| `write_float` | `write_float(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a 32-bit IEEE-754 float. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `write_double` | `write_double(f, n, [endian=])` | Writes `n` (number) to `f` (file handle, opened writable/appendable) as a 64-bit IEEE-754 float. `endian` (string, optional, default `"little"`, or `"big"`) selects byte order. Returns `Nothing`. |
| `read_int` | `read_int(f, [bytes=4], [signed=true], [endian="little"])` | The width-as-a-value counterpart of the fixed-width family above: reads `bytes` (a number, `1`/`2`/`4`/`8` only) from `f` (file handle, opened readable) and decodes them as an integer, with `signed` (boolean) and `endian` (string) chosen at runtime rather than by which function you called. Reach for it when a format declares its field width in a header instead of in its spec. Returns a number, or `Nothing` if fewer than `bytes` bytes remain. Every number here is an `f64`, so an 8-byte value above `2^53` comes back rounded — the same ceiling `read_int64` documents. |
| `write_int` | `write_int(f, value, [bytes=4], [signed=true], [endian="little"])` | The mirror of `read_int`, writing `value` (a number) to `f` (file handle, opened writable/appendable) in the given width and byte order. **Unlike `write_int16`/`write_int32`, this checks that `value` fits** and errors if it does not, rather than silently wrapping: `bytes=`/`signed=` are runtime values that can come from a variable, so a wrapped result here would be far harder to notice than picking the wrong fixed-width function. Returns `Nothing`. |
| `read_bin` | `read_bin(f, n)` | Reads up to `n` (a number) raw bytes from `f` (file handle, opened readable). A partial read is not an error — hitting the end partway gives however many bytes were there, which is what makes a "drain this file in chunks" loop composable without an `eof` check each time round. Returns a `Vec` of numbers 0-255, or `Nothing` only when no bytes remain at all. |
| `write_bin` | `write_bin(f, bytes)` | The inverse of `read_bin`: writes raw bytes to `f` (file handle, opened writable/appendable). `bytes` is a `Vec`/`List` of numbers, every one of them a whole number in `0..=255` — anything else errors rather than being truncated into range. Returns `Nothing`. |

Reach for `write_bit`/`read_bit` when a format is defined at the bit level
rather than the byte level — status-flag registers, packed boolean arrays,
anything from an instrument or protocol spec that talks about individual
bits rather than whole bytes. They pack individual bits into bytes
MSB-first; `close(f)` flushes and zero-pads any partial byte left pending.
The example below writes four bits (`1,0,1,1`), which is not a whole byte,
closes the file to force that flush, then reads the same four bits back and
confirms only one byte was actually written to disk:

```qu
f = fopen("flags.bin", "wb")
write_bit(f, 1)
write_bit(f, 0)
write_bit(f, 1)
write_bit(f, 1)
close(f)                       # only 4 bits written -- flushed as one padded byte

g = fopen("flags.bin", "rb")
b0 = read_bit(g)
b1 = read_bit(g)
b2 = read_bit(g)
b3 = read_bit(g)
close(g)
print("{b0}{b1}{b2}{b3}")      # 1011
print(file_size("flags.bin"))  # 1 -- one byte on disk for 4 bits
```

The other fixed-width types round-trip the same way, each with its own
`read_*`/`write_*` pair — this is the toolset for reading and writing a
binary format with a known, fixed layout: a data-logger capture, a
device's native save file, or any interchange format built around
fixed-width integers and floats rather than text. The example below writes
one value of each width (a signed 16-bit int, an unsigned 16-bit int, an
unsigned 32-bit int, and a 64-bit double) back to back, then reads them
back in the same order to show the round trip is exact:

```qu
f = fopen("sample.bin", "wb")
write_int16(f, -5)
write_uint16(f, 300)
write_uint32(f, 70000)
write_double(f, 3.14159)
close(f)

g = fopen("sample.bin", "rb")
a = read_int16(g)
b = read_uint16(g)
c = read_uint32(g)
d = read_double(g)
close(g)
print("{a} {b} {c} {d}")       # -5 300 70000 3.14159
```

`read_int64`/`write_int64` and their unsigned counterparts round-trip the
same way, with one real caveat worth seeing rather than just stating:
every `Value` here is an `f64`, exact only up to `2^53`, so a 64-bit
integer past that (about 9 quadrillion) comes back rounded, not exact.
`write_array`/`read_array` are the block versions of the fixed-width
functions above — a whole `Vec` or `Mat` written or read in one call
instead of one element at a time; a `Mat` is written in its own
column-major order, so reading it back needs a `reshape`/`-> (r, c)` to
recover the original shape (a flat file has no shape of its own):

```qu
f = fopen("sample64.bin", "wb")
write_int64(f, -123456789012)
write_uint64(f, 9007199254740993)   # 1 past 2^53 -- the exact-precision edge
close(f)

g = fopen("sample64.bin", "rb")
a = read_int64(g)
b = read_uint64(g)
close(g)
print("{a} {b}")               # -123456789012 9007199254740992 -- rounded

M = [1, 2, 3; 4, 5, 6]
write_array("matrix.bin", M, type = "int32")
back = read_array("matrix.bin", type = "int32") -> (2, 3)
print(back)                    # [1, 2, 3; 4, 5, 6] -- shape recovered via reshape
```

## Compression & Entropy Coding

Three from-scratch entropy coders — Huffman, adaptive range coding, and
rANS — plus the zlib/DEFLATE decoder `read_mat` and `load_image` already
carried internally, exposed as script-callable builtins rather than left as
an implementation detail only those two readers could reach.

All four take their input the same way: a `Str` (its UTF-8 bytes) or a
`Vec` of numbers in `0..=255`. All four decoders hand bytes back the same
way too — a `Vec` of 0-255 numbers by default, or the decoded text as a
`Str` with `as_str = true`. An encoder returns a `Record` rather than a
bare byte string because a decoder needs the model as well as the payload;
pass the whole record straight back to the matching decoder.

There is no compressor to pair with `zlib_decompress` yet. That is a real
gap, named here rather than implied away.

| Function | Signature | Description |
|---|---|---|
| `huffman_encode` | `huffman_encode(data)` | Canonical Huffman coding of `data` (a `Str`, or a `Vec` of 0-255 byte values). Stores the code as one byte of code length per symbol, so the decoder rebuilds the tree rather than carrying it. Returns a `Record` with fields `lengths` (a length-256 `Vec`, the code length of each byte value, `0` for a value that never occurs), `bits` (a `Vec` of 0-255 packed code bytes), `bit_len` (a `Num`, how many of those bits are real) and `symbol_count` (a `Num`, the input length). Empty input gives `none` rather than a record: there is no data to encode, and none to decode back. |
| `huffman_decode` | `huffman_decode(enc, [as_str=false])` | Inverse of `huffman_encode`: `enc` is the whole `Record` that call returned, passed back unchanged; `as_str` (named boolean, default `false`) asks for text instead of bytes. Returns a `Vec` of 0-255 numbers, or the decoded text as a `Str` when `as_str = true`. |
| `range_encode` | `range_encode(data)` | Byte-oriented adaptive range coding of `data` (a `Str`, or a `Vec` of 0-255 byte values). The model adapts as it goes, so nothing but the symbol count travels alongside the payload. Returns a `Record` with fields `bytes` (a `Vec` of 0-255 coded bytes) and `symbol_count` (a `Num`, the input length). Empty input gives `none` rather than a record. |
| `range_decode` | `range_decode(enc, [as_str=false])` | Inverse of `range_encode`: `enc` is the `Record` that call returned; `as_str` (named boolean, default `false`) asks for text instead of bytes. Returns a `Vec` of 0-255 numbers, or the decoded text as a `Str` when `as_str = true`. |
| `rans_encode` | `rans_encode(data)` | rANS entropy coding of `data` (a `Str`, or a `Vec` of 0-255 byte values) — a static frequency table, unlike `range_encode`'s adaptive model, so the table travels with the payload. Returns a `Record` with fields `freq` (a length-256 `Vec`, the symbol frequency table), `bytes` (a `Vec` of 0-255 coded bytes) and `symbol_count` (a `Num`, the input length). Empty input gives `none` rather than a record. |
| `rans_decode` | `rans_decode(enc, [as_str=false])` | Inverse of `rans_encode`: `enc` is the `Record` that call returned; `as_str` (named boolean, default `false`) asks for text instead of bytes. Returns a `Vec` of 0-255 numbers, or the decoded text as a `Str` when `as_str = true`. |
| `zlib_decompress` | `zlib_decompress(data, [as_str=false])` | Inflates a zlib stream (RFC 1950 wrapper around RFC 1951 DEFLATE): `data` is a `Str` or a `Vec` of 0-255 byte values; `as_str` (named boolean, default `false`) asks for text instead of bytes. Errors clearly on a stream it cannot parse rather than returning partial output. Returns a `Vec` of 0-255 numbers, or the decompressed text as a `Str` when `as_str = true`. |

A round trip through any of the three coders returns exactly what went in:

```qu
s = "hello hello hello compression world"
enc = rans_encode(s)
print(numel(enc.bytes))                    # coded length, in bytes
print(rans_decode(enc, as_str = true) == s)  # true
```

## Directories & Paths

A script that reads a file usually has to find it first. These are the
builtins that answer "where am I", "what is in here" and "which of these
files do I actually want" — the last of which is `list_files`, and it is
the one worth reading about.

`list_dir` and `list_files` divide the work between them. `list_dir` gives
the bare names inside one directory, files and subdirectories alike, and
stops there. `list_files` takes a whole-path pattern, walks as deep as the
pattern tells it to, returns **paths you can open**, and never returns a
directory. The second is what you want for `"every HTML page under
website/"`; the first is what you want when you are about to look at each
entry and decide.

| Function | Signature | Description |
|---|---|---|
| `cur_dir`, `pwd` | `cur_dir()` / `pwd()` | Takes no arguments. `cur_dir` says what it gives back and `pwd` is what anyone who has used a shell types first, so both spellings exist rather than one being renamed out from under existing scripts. Returns a `Str`: the process's current working directory, using the platform's own separator. |
| `cd` | `cd(path)` | Changes the working directory to `path` (a `Str`) for the **whole process**, so every later relative path in the script resolves from there. Errors if `path` does not exist or is not a directory, rather than leaving you somewhere you did not expect. To run one command elsewhere without moving the script, use `exec`'s `cwd=` instead. Returns `Nothing`. |
| `list_dir`, `listdir`, `dir` | `list_dir(path, [pattern])` | The bare names inside `path` (a `Str`), sorted — `"a.qu"`, not `"tools/a.qu"`. Files **and** subdirectories both; filter them apart with `file_exists`/`dir_exists` per entry. `pattern` (an optional `Str`) keeps only the names matching a `*`/`?` wildcard, e.g. `"*.csv"`. `listdir` and `dir` are the same builtin under the names other languages and shells use. Returns a `List` of `Str`. |
| `list_files`, `glob` | `list_files([dir], [pattern], [filter=], [contains=], [size=], [recursive=])` | Every **file** matching a whole-path wildcard, as full paths, sorted — never a directory, which is what makes the content and size filters mean something on every entry. `dir` is a directory to search and `pattern` a wildcard to match (`*` and `?` stay inside one path segment, `**` crosses them); a single argument is read as a pattern when it holds a wildcard and as a directory when it does not. `filter=` is the keyword spelling of the pattern; `contains=` keeps only files whose bytes hold that text; `size=` compares against the byte size (`">2MB"`, `"<=100KB"`, `"=0"`, with 1024-based units); `recursive=` searches subdirectories without writing `**` yourself. `glob` is the same search with no filters, under the name most other languages give it. Returns a `List` of `Str`, empty when nothing matched. |
| `file_exists`, `dir_exists` | `file_exists(path)` / `dir_exists(path)` | Whether `path` (a `Str`) exists **and** is a regular file / a directory. A path that is not there at all is `false`, not an error — its absence is the answer you asked for. Returns a `Bool`. |
| `mkdir` | `mkdir(path)` | Creates the directory `path` (a `Str`), **including any missing parents** — `mkdir -p`, not bare `mkdir`. Idempotent: a directory that already exists is success, not an error, because "make sure this exists" is what callers actually want. Returns `Nothing`. |
| `make_file` | `make_file(path)` | Creates an empty file at `path` (a `Str`), **truncating it if it is already there** — the same promise `fopen(path, "w")` makes, so the two agree. Returns `Nothing`. |
| `tmp_file` | `tmp_file()` | Takes no arguments. Creates a new empty file under the OS temp directory with a name nothing else will collide with (process id, nanosecond clock, and a counter — any one alone has a plausible collision window). Returns a `Str`: its path, not an open handle, so it composes with `fopen` like any other path. |
| `remove_file` | `remove_file(path, [recycle_bin=false])` | Deletes `path` (a `Str`). Errors if it does not exist or is not a regular file, rather than silently doing nothing. `recycle_bin=true` sends it to the OS Recycle Bin/Trash instead of deleting it permanently — default `false` keeps a script's existing behavior unchanged. Returns `Nothing`. |
| `remove_dir` | `remove_dir(path, [recursive=false], [recycle_bin=false])` | Deletes the directory `path`. Refuses a **non-empty** directory unless `recursive=true` — this check applies whether or not `recycle_bin` is set, since "may I remove a whole tree" and "should this be recoverable" are different questions. Returns `Nothing`. |
| `rename_file` | `rename_file(old, new, [on_exists="error"])` | Renames `old` to `new` (both `Str`), generally within the same filesystem. `on_exists` controls what happens if `new` already exists: `"error"` (default) refuses; `"overwrite"` replaces it; `"skip"` does nothing and returns `false` instead of erroring. Returns `true` if the rename happened, `false` if skipped. Unlike `move_file`, does **not** fall back to copy+delete across drives — a cross-drive `rename_file` fails with a clear error instead of silently becoming a slower, different operation. |
| `move_file` | `move_file(src, dst, [on_exists="error"])` | Moves `src` to `dst`, falling back to copy-then-remove-the-original when a plain rename fails (e.g. across drives) — the `on_exists` collision check happens first either way, so the fallback can never overwrite something the fast path would have refused. Same `on_exists` values and return convention as `rename_file`. |
| `copy_file` | `copy_file(src, dst, [on_exists="error"])` | Copies `src` to `dst`, leaving the original in place. Same `on_exists` values and return convention as `rename_file`/`move_file`. |
| `create_file` | `create_file(path, [on_exists="error"])` | `make_file` with a collision policy: `"error"` (default) refuses if `path` already exists, `"overwrite"` truncates it like `make_file` does unconditionally, `"skip"` does nothing and returns `false`. `make_file` itself is unchanged — this is the builtin for callers who need to say what should happen on collision, not a replacement for it. |

All six above are disabled in sandboxed execution (see `sandbox_mode`), the same as `touch` and `write_csv` — each is capable of destroying or overwriting a file the script did not create, `recycle_bin=true` or not.

### The wildcards `list_files` understands

| Wildcard | Matches |
|---|---|
| `*` | any run of characters, **within one path segment** |
| `?` | exactly one character, within one path segment |
| `**` | zero or more whole segments, so `a/**/b.txt` finds `a/b.txt`, `a/x/b.txt` and `a/x/y/b.txt` alike |

`*` deliberately stops at a separator. If it did not, `website/*.html`
would also match `website/fn/add.html` and the ordinary case would be
wrong — which is why `**` has to be spelled differently, in every shell
and every language that ships a glob.

Both `/` and `\` separate segments in a pattern, so a Windows path can be
written either way, and the paths that come back use the platform's own
separator.

### Reading `list_files`'s arguments

One argument is a **pattern** if it has a wildcard in it and a
**directory** if it does not, so neither spelling needs a keyword to say
which was meant:

```text
list_files()                          every file in the current directory
list_files("C:/users/pc")             every file in that directory
list_files("*.html")                  by pattern, here
list_files("C:/users/pc", "*.html")   directory and pattern, given apart
list_files("website/**/*.html")       the whole subtree
```

`filter=` is the keyword spelling of the pattern. Passing both a
positional pattern and `filter=` is an error rather than a silent choice
between them.

### Narrowing by size and by content

| Keyword | What it keeps |
|---|---|
| `contains=` | files whose bytes contain this text — matched against raw bytes, so it works on any file and never fails on one that is not valid UTF-8. Case-sensitive. |
| `size=` | files whose byte size satisfies a comparison written as a string: `">2MB"`, `"<=100KB"`, `"=0"`, `"!=0"`. Operators are `>`, `>=`, `<`, `<=`, `=`, `==`, `!=`; with no operator at all it means equality, which is what makes `size="0"` read as "the empty ones". |
| `recursive=` | search subdirectories too, without writing `**` yourself — `list_files(d, "*.html", recursive = true)` is exactly `list_files(d, "**/*.html")`. |

**`KB`/`MB`/`GB` are 1024-based here**, the way a file manager reports a
size rather than the 1000-based SI reading. `KiB`/`MiB`/`GiB` are accepted
as explicit spellings of those same values. A unit that is not one of
these errors, rather than being read as bytes: `">2Mb"` is a typo worth
hearing about, and answering it as "2 bytes" would give a wrong answer
that looks like a right one.

The filters run in the order that reads the fewest bytes — the name
pattern first, which opens nothing; then `size=`, one metadata call; then
`contains=`, which has to read the file. Pairing `contains=` with a
pattern or a size bound is what keeps a search over a large tree cheap.

The example below makes its own files first, so it finds the same thing
wherever it is run — and note what it does **not** do: print `cur_dir()`.
That would put this machine's absolute path into the page, since the
documentation build runs every example and keeps what it printed.

```qu
mkdir("list_demo")
write_text("list_demo/report.qu", "print(1)")
write_text("list_demo/plot.qu", "print(2)")
write_text("list_demo/notes.txt", "the needle is here")
make_file("list_demo/empty.log")

qu_files = list_files("list_demo", "*.qu")
print("{length(qu_files)} Qu files")

carrying = list_files("list_demo/*", contains = "needle")
for f in carrying
    print("carries the needle: {f}")
end for

blank = list_files("list_demo/*", size = "0")
print("{length(blank)} empty file")
```

## Running Other Programs

Qu could already run *code* in another language — `python_exec`,
`js_exec`, `matlab_exec` — but not a *program*. That is a strange hole for
a language whose own build tooling is written in itself: the shell scripts
under `tools/` exist largely because the Qu program inside each one could
not ask git a question.

Two builtins, because there are two different jobs and one call does not
do both well.

| Function | Signature | Description |
|---|---|---|
| `exec` | `exec(program, [args], [shell=], [cwd=], [stdin=])` | Runs `program` (a `Str`) and **waits**, capturing what it printed. `args` is an optional `List` of arguments passed to the OS as separate items — not joined into a command line and re-split — so a path with a space in it needs no quoting rule. `shell=true` instead reads `program` as a whole command line for `cmd /C` (Windows) or `sh -c`, so pipes and `&&` work. `cwd=` (a `Str`) runs it in that directory without moving the caller. `stdin=` (a `Str`) is written to the program's input, which is then closed, so a program reading to EOF gets exactly that and does not hang. Returns a `Record` with `stdout` (a `Str`), `stderr` (a `Str`), `success` (a `Bool`) and `exit_code` (a `Num`) — the same four fields `python_exec`/`js_exec`/`matlab_exec` already give back. |
| `shell` | `shell(command, [style])` | Hands `command` (a `Str`) to the OS shell and returns **immediately**, without waiting — Visual Basic's `Shell`, which is where the window styles come from too. `style` (an optional `Str`, positional or `style=`) is one of `"normal"` (the default), `"hidden"` (also spelled `"ghost"`), `"minimized"` or `"maximized"`. Returns a `Num`: the process id. |

**A program that fails is not a Qu error.** A non-zero exit comes back as
`success = false`, not as an exception — `exec("git", ("diff",
"--quiet"))` uses the exit code as its answer, and a language that raised
on it would make that unwritable. What *does* error is being unable to
start the program at all, which is a genuinely different thing.

**`shell`'s process id is not always the program's own.** `"minimized"`
and `"maximized"` have no API reachable from Rust's `Command`, so they go
through `cmd /C start /MIN|/MAX`, and the id that comes back belongs to
that launcher — which exits as soon as it has started the program.
`"normal"` and `"hidden"` spawn directly and give a real, live id. Worth
knowing before you store one.

On anything other than Windows, `"normal"` and `"hidden"` behave (there is
no window to hide, so hidden simply means no terminal is borrowed), and
`"minimized"`/`"maximized"` error rather than being quietly ignored. They
are window-manager instructions with no meaning to `sh`, and accepting
them in silence would mean a script that looks portable and is not.

Both are refused under `qu run --sandbox`, alongside `python_exec`,
`js_exec` and `matlab_exec`. Starting a process is the most
capability-bearing thing a script can do, and a sandbox that let one
through would not be one.

```qu
r = exec("echo hello from a program", shell = true)
print("said: {trim(r.stdout)}")
print("success {r.success}, exit code {r.exit_code}")

failed = exec("exit 3", shell = true)
print("a failure is data, not an error: success {failed.success}, code {failed.exit_code}")
```

## Memory-Mapped Files

Read-only, random-access mapping of a file's bytes via the `memmap2` crate,
without loading the file into a `Vec` up front — useful for jumping around
inside a large file (a header at a known offset, a fixed-width record at an
arbitrary index) without either an eager full read or a chain of `seek`
calls on a `fopen` handle. Distinct from `Value::File`: an `mmap` handle has
no read cursor to advance — every `mmap_read` call takes an explicit byte
range. Writable mapping is out of scope; use `fopen`/the write builtins
above for writing.

| Function | Signature | Description |
|---|---|---|
| `mmap_open` | `mmap_open(path)` | Memory-maps `path` (string, filesystem path) read-only. Returns an opaque `mmap` handle. Errors if the file doesn't exist or is empty (mapping a zero-length file is unsupported). |
| `mmap_len` | `mmap_len(m)` | Reports the mapped file's size. `m` (mmap handle, from `mmap_open`) is the mapping to inspect. Returns a number: the total length in bytes. |
| `mmap_read` | `mmap_read(m, start, len)` | Reads a byte range out of a mapping without moving any cursor. `m` (mmap handle, from `mmap_open`) is the mapping to read from. `start` (number, byte offset from the beginning of the file) and `len` (number, byte count) select the range. Returns a `Vec` of `len` numbers, each 0-255 — the same representation `read_bin` uses. A range that runs past the end of the file is a **clear error**, not a silent truncation (unlike `read_bin`'s "graceful partial read" — an `mmap` handle always knows its own length up front via `mmap_len`, so a past-EOF range is far more likely a caller bug than an intentional "give me whatever's left"). |

Reach for `mmap_open`/`mmap_read` instead of `fopen`/`seek`/`read_bin` when
a program needs to jump around inside a large file at arbitrary offsets —
a header at a known position, or one record out of a huge array of
fixed-width records — without either reading the whole file up front or
issuing a `seek` call for every access. The example below maps a binary
file, reads its first 16 bytes as a "header," then reads 64 bytes starting
at byte offset `5 * 64` as if the file were a flat array of 64-byte
records, and prints both to show a single mapping serves any number of
independent, arbitrary-offset reads:

```qu
# The file is written here first. Mapping one the repository does not
# carry would make this block fail, and a block that fails is dropped
# from the page while the build still reports success.
write_array("big_dataset.bin", 0 to 511, type = "u8")

m = mmap_open("big_dataset.bin")
n = mmap_len(m)
header = mmap_read(m, 0, 16)          # first 16 bytes, wherever they live
record5 = mmap_read(m, 5 * 64, 64)    # the 6th fixed-width 64-byte record
print(n)
print(header)
print(record5)
```

## Polymorphic Streams: `StreamFile` / `StreamURL`

`StreamFile(path, [mode])` and `StreamURL(url)` (§ StreamFile/StreamURL
polymorphic streams, 2026-08-31) are two constructors that produce values
exposing the **same** method names — `read_line`/`read_all`/`eof`/`close` —
so code written against "a stream" doesn't need to know or check which
concrete kind it was handed:

| Function | Signature | Description |
|---|---|---|
| `StreamFile` | `StreamFile(path, [mode])` | A second name for exactly `fopen`'s own machinery. `path` (string, filesystem path) is the file to open. `mode` (string, optional, default `"r"`) accepts the same mode strings `fopen` does. Returns a `Value::File` handle — the same concrete type `fopen` returns — so every existing real-streaming `fopen` builtin (`read_line`, `read_all`, `seek`, `eof`, `read_bin`, ...) already works on it, not just the four listed here. |
| `StreamURL` | `StreamURL(url)` | `url` (string, an HTTP or HTTPS URL) is fetched eagerly with a GET request (same request `http_get` makes: GET only, 30s timeout, clear errors on an unreachable host or non-2xx status — see [Networking (HTTP)](concurrency.md#networking-http)). Returns an opaque stream handle wrapping the whole response body in a read-only, in-memory cursor exposing `read_line`/`read_all`/`eof`/`close`. |
| `.read_line()` / `read_line(s)` | — | `s` is a `StreamFile`/`StreamURL` handle. Returns one line as a string, without its trailing newline, or `Nothing` at EOF — identical contract on both stream kinds. |
| `.read_all()` / `read_all(s)` | — | `s` is a `StreamFile`/`StreamURL` handle. Returns the remaining content as one string (possibly empty at EOF). |
| `.eof()` / `eof(s)` | — | `s` is a `StreamFile`/`StreamURL` handle. Returns a boolean: `true` iff no bytes remain to read from the current position. |
| `.close()` / `close(s)` | — | `s` is a `StreamFile`/`StreamURL` handle. Releases it and returns `Nothing`. Idempotent on both kinds — closing an already-closed stream is a no-op, not an error. Reading or checking `eof` after `close` is a clear error naming the handle. |

**`StreamURL` is fetch-then-wrap, not incremental network streaming** — a
deliberate, investigated choice, not a shortcut. `ureq`'s response body IS a
real `Read`-implementing stream, but reusing `fopen`'s exact streaming-reader
plumbing against it would require widening `FileHandleState`'s reader field
to a boxed trait object AND handling HTTP responses that have no `Content-
Length` at all (chunked transfer encoding) — a real `FileHandleState`
redesign, not a reasonable extension of it. So `StreamURL` fetches the whole
body up front (exactly what `http_get` already does) and serves it back
through the same method names as a read-only in-memory stream — every method
above behaves identically to the file side, just without the "hasn't
downloaded yet" state a genuinely incremental reader would have.

The whole point: one function, written once against the shared method
names, works unmodified against either kind. The example below defines a
`drain` function once, using only `eof`/`read_line`/`close`, and calls it
against a local file opened with `StreamFile` and a remote resource opened
with `StreamURL` — neither the function body nor the caller needs to know
which kind of stream it was handed:

```qu
n = fopen("notes.txt", "w")
write_line(n, "line one")
write_line(n, "line two")
close(n)

function drain(s)
    total = ""
    while not eof(s)
        total = total + read_line(s) + "|"
    end while
    close(s)
    return total
end function

a = drain(StreamFile("notes.txt"))
print(a)                                    # line one|line two|

b = drain(StreamURL("https://example.com/"))
print(length(b) > 0)                        # true -- some HTML came back
```

## CSV

`read_csv`/`write_csv` operate directly on a path — no `fopen` handle
involved; they open/read/write the file internally in one call.

| Function | Signature | Description |
|---|---|---|
| `read_csv` | `read_csv(path, [headers=true], [sep=","], [decimal="."])` | Reads a CSV file into a `Table`. `path` (string) is the file to read. `headers` (boolean, optional, default `true`) — `false` treats the first line as data instead of column names (columns are then auto-named `col1`, `col2`, ...). `sep` (string, optional, default `","`) is the field delimiter. `decimal` (string, optional, default `"."`) is the decimal-point marker (e.g. `sep=";", decimal=","` for the European convention). Returns a `Table` with one row per data line and one column per field. `table.load(path)` is sugar for this (no keyword-argument forwarding — use `read_csv` directly for `headers=`/`sep=`/`decimal=`). |
| `write_csv` | `write_csv(df, path)` | Writes a `Table` to a CSV file. `df` (Table) is the data to write. `path` (string) is the destination file. Returns `Nothing`. `table.write(df, path)` is sugar for this — same argument order. |

`table.load`/`table.write` are part of a small, closed `table.`/`timer.`
"namespace-dot" alias set (§ ergonomic API layer, 2026-08-31): the parser
recognizes exactly the token sequence `table . load (` / `table . write (`
and rewrites it, at parse time, straight to `read_csv(...)`/`write_csv(...)`
— evaluation is byte-identical, since it's literally the same call. `table`
is not a real namespace value; it still works as an ordinary variable name
(`table = 5`) and the existing `table(...)` table-constructor call is
completely unaffected (that's a different token shape — no `.` involved).

## Native Modules: `xlsx`, `codec` and `pdf`

These are not builtins, and `builtins()` does not list them. They are
modules compiled into the engine, and a program reaches them through
`import`, which opens their names and leaves the qualified path available
at the same time:

```qu,ignore
import xlsx
names = sheets("book.xlsx")        # opened by the import
names = xlsx.sheets("book.xlsx")   # qualified, always available
```

Where the bare name would be ambiguous it is refused rather than guessed
at. `xlsx` offers a `read` and so does the language, so a bare `read`
after `import xlsx` is an error naming the fix — write `xlsx.read(...)`
for the module's, or drop the import to get the builtin back. `import
nope` names the modules a build actually has, which is the reliable way
to find out.

### Spreadsheets: `xlsx`

| Function | Signature | Description |
|---|---|---|
| `xlsx.sheets` | `xlsx.sheets(path)` | Names a workbook's worksheets without reading any of them, so a program can find out what it is holding before deciding what to load. `path` (string) is the `.xlsx` file. Returns a `List` of `Str`, the worksheet names in workbook order. |
| `xlsx.read` | `xlsx.read(path, [sheet], [headers=true])` | Reads one worksheet of a workbook into a `Table`, the same type `read_csv` produces, so everything that consumes a CSV consumes a spreadsheet unchanged. `path` (string) is the `.xlsx` file. `sheet` (string, optional, positional or `sheet=`) picks it by name; omitting it takes the first. `headers` (boolean, optional, default `true`) — `false` treats the first row as data and names the columns by position (`A`, `B`, `C`) instead. Returns a `Table`, one column per spreadsheet column, each column numeric or text according to what the cells hold. |
| `xlsx.write` | `xlsx.write(path, df, [sheet="Sheet1"])` | Writes a `Table` out as a one-worksheet workbook. `path` (string) is the destination, overwritten if it exists. `df` (Table) is the data. `sheet` (string, optional, positional or `sheet=`, default `"Sheet1"`) names the worksheet inside the file. Returns `Nothing`. |

A round trip, which is also the way to see what `headers=false` does — the
header row comes back as data under positional column names:

```qu
import xlsx

t = table(city = ("Aachen", "Bonn", "Cologne"), pop = (249, 331, 1087))
xlsx.write("cities.xlsx", t, sheet = "Cities")

print(xlsx.sheets("cities.xlsx"))          # ("Cities")
print(xlsx.read("cities.xlsx"))            # a Table, city/pop
print(xlsx.read("cities.xlsx", headers = false))
```

### Audio: `codec`

The readers take `src` as either a path (string) or the file's bytes as a
`Vec` of whole numbers 0-255 — a script that already has the bytes, from a
socket or an archive, should not have to write a temporary file to use
them, and the two can never be confused because a FLAC stream starts with
the bytes `fLaC`.

The WAV writer is split in two for the same reason, pointed the other way:
`codec.write_wav` writes a file, and `codec.encode_wav` hands back the
bytes for a script that is going to send them somewhere rather than store
them. Only WAV is written — see the note below the table for what is not.

| Function | Signature | Description |
|---|---|---|
| `codec.flac_info` | `codec.flac_info(src)` | Reads a FLAC file's header without decoding any audio, which is how to check a file is what you expect before paying for the samples. `src` (string path, or `Vec` of bytes) is the stream. Returns a `Record` with fields: `fs` (`Num`, the sample rate in Hz), `channels` (`Num`, how many channels the stream carries), `bits` (`Num`, bits per sample), `frames` (`Num`, or `none` — a FLAC stream is allowed not to declare its length, and `none` says so where `0` would read as an empty file). |
| `codec.decode_flac` | `codec.decode_flac(src, [channel=])` | Decodes a FLAC file to samples. `src` (string path, or `Vec` of bytes) is the stream. `channel` (number, optional, named only) picks one channel out of a multi-channel file, numbered from `0`. Returns a `Signal` when the result is a single series, and a `Record` of `Signal`s when it is not — see the two forms below. |
| `codec.decode_wav` | `codec.decode_wav(src, [channel=])` | Decodes a WAV file. Same arguments and same two return forms as `codec.decode_flac`. Integer WAV is normalised by `2^(bits-1)`, exactly as FLAC is, so the same recording in either container gives the same numbers. **Float WAV passes through untouched** — it is already in `-1.0 ..= 1.0`, and rescaling it would be the one case where normalising changes the data — and reports `bits` as `none`, because a float format has a width but no quantisation step. Returns a `Signal` for a single series — a mono file, or any file with `channel=` — and otherwise a `Record` with `fs`, `bits`, `frames` and `channels`, exactly as `codec.decode_flac` does. |
| `codec.decode_mp3` | `codec.decode_mp3(src, [channel=])` | Decodes an MP3 file. Same arguments and same two return forms as `codec.decode_flac`. `bits` is always `none`: MP3 is lossy, its output is real numbers rather than a quantised grid, and a plausible `16` there would invite arithmetic that means nothing. MP3 has no header — only a run of frames — so a file that is not MP3 fails by finding no frames rather than by rejecting a magic number, and the error says so. Returns a `Signal` for a single series — a mono file, or any file with `channel=` — and otherwise a `Record` with `fs`, `bits`, `frames` and `channels`, exactly as `codec.decode_flac` does. |

| `codec.write_wav` | `codec.write_wav(path, x, depth=, [dither=])` | Writes audio out as a WAV file. `path` (string) is the destination, overwritten if it exists. `x` is a `Signal`, or a `List` of `Signal`s for a multi-channel file — a signal carries its own sample rate, so there is no `fs=` argument to get wrong, and the rates of a list have to agree because a WAV header holds exactly one. `depth` (number, named, **required**) is `16`, `24` or `32`, where `32` means 32-bit **float**. `dither` (string, named) is `"tpdf"` or `"none"`, and at `depth=16` it is **required** — see below. Returns `Nothing`. Added in v0.2.4. |
| `codec.encode_wav` | `codec.encode_wav(x, depth=, [dither=])` | The same encoder as `codec.write_wav`, returning the file's bytes as a `Vec` of whole numbers 0-255 instead of writing them — the exact inverse of what `codec.decode_wav` accepts, so a stream can be re-encoded and passed on without ever touching disk. Same `x`, `depth` and `dither` arguments, and the same rule about naming `dither` at `depth=16`. Returns a `Vec`. Added in v0.2.4. |

`flac_info` needs no split — a header is a header. The three decoders
differ only in which format they read: they return the same shapes, scale
to the same range, and a script that handles one handles all three.

#### Bit depth and dither are named, never defaulted

`depth` has no default because the depth is what decides how much of the
signal survives the write, and a default would be the library quietly
choosing that for you.

`dither` is required at `depth=16` specifically, and leaving it out is an
error rather than a silent truncation:

```qu,ignore
import codec

codec.write_wav("out.wav", x, depth = 16)
# error -- depth = 16 quantises 64-bit samples down to 16 bits, and doing
# that without saying how is an error: silent truncation of audio is
# audible. Name dither = "tpdf" or dither = "none".
```

Samples are 64-bit floats inside a `Signal`, so 16-bit is always a
reduction, and it is the depth where the difference is plainly audible —
rounding alone leaves harmonic distortion on fades and quiet passages.
`"tpdf"` adds ±1 LSB of triangular noise, which trades that distortion for
a flat noise floor uncorrelated with the signal; `"none"` is plain
rounding, which is a legitimate choice as long as it is a *choice*. The
dither is drawn from the interpreter's own generator, so `seed(n)` makes a
dithered write reproducible.

`depth=24` does not require the argument (it accepts it), and `depth=32`
**refuses** it: 32-bit float has no quantisation step, so there would be
nothing for a dither to do.

```qu,ignore
import codec

a = codec.decode_wav("take.wav")            # a stereo file
codec.write_wav("take24.wav", a.channels, depth = 24)
codec.write_wav("take16.wav", a.channels, depth = 16, dither = "tpdf")
```

A round trip at a file's own depth is **exact**, not approximate: the
scale is `2^(depth-1)` in both directions, a power of two, so the
integers come back as the integers they were. The one asymmetry is the
format's — two's complement has no `+2^(depth-1)`, so a sample of exactly
`+1.0` clips to one step below full scale, while `-1.0` is exact.

**Not written, and not silently implied covered**: RF64 (the >4GB WAV
variant), and FLAC/OGG/MP3/AIFF/CAF encoding. `hound` has no RF64 support
in either direction and `claxon` is a FLAC *decoder* only, so each of
these means a from-scratch container implementation — exactly what
`IMPL.md` §7 exists to prevent. Reading FLAC, MP3 and WAV is unaffected;
it is only the write direction that stops at WAV.

None of the codec examples here are run by the build: they need an audio
file, and the repository carries none.

```qu,ignore
import codec

info = codec.flac_info("take.flac")
print("{info.fs} Hz, {info.channels} ch, {info.bits} bit")
```

#### Overloads: `codec.decode_flac`

What comes back depends on how many series the call produces, because
there is no one type that fits both without lying about one of them.

#### Case: one series — mono, or `channel=`

`src` is a mono file; `channel`, when given, picks one channel out of any
file, numbered from `0`. Returns a `Signal`: the sample rate travels with
the samples, so `spectrogram`/`welch`/`sosfilt` need no second argument.
That is the whole reason `Signal` exists, and it is the common case.

```qu,ignore
import codec

x = codec.decode_flac("take.flac", channel = 0)
spectrogram(x)
```

#### Case: several series — multi-channel

`src` is a file with more than one channel; `channel` is absent, so no
single series is picked out. Returns a `Record` with fields: `fs` (`Num`,
the sample rate in Hz), `bits` (`Num`, bits per sample), `frames` (`Num`,
samples per channel), `channels` (`List` of `Signal`, one per channel,
each already carrying the rate — so `a.channels[0]` goes straight into
anything that takes a signal, with no reassembly needed). A `Signal` is
one series and a `Mat` has nowhere to put the sample rate, which is why
this form cannot hand back either of them.

```qu,ignore
import codec

a = codec.decode_flac("stereo.flac")
print("{a.fs} Hz, {len(a.channels)} channels, {a.frames} frames")
welch(a.channels[0])
```

### Documents: `pdf`

Reads and rearranges the *structure* of a PDF: how many pages it has,
what it says about itself, and how to join or split files. It does not
render, OCR, fill forms or redact — all four need a native library linked
into the engine, and this module deliberately has no such dependency (see
`docs/design/toolkit-pdf.md` for the larger roadmap this is the first
slice of).

The same destination split as `codec` above: `pdf.merge` and
`pdf.extract_pages` build a new PDF and hand back its **bytes**, while
`pdf.write_merge` and `pdf.write_pages` build the same PDF and **write**
it to a path given first. Two names rather than one function with an
optional `out =`, so the return type never depends on whether a keyword
was passed — and so a sandboxed script can be denied the writing pair by
name while keeping the byte-returning pair, exactly as
`codec.write_wav`/`codec.encode_wav` are treated.

Every function takes its source the way `codec` does: a **path**, or the
file's **bytes** as a `Vec` of whole numbers 0-255. That is what lets the
output of `pdf.merge` go straight back into `pdf.page_count` or
`pdf.extract_pages` without ever touching disk.

Pages are numbered **from 1**, the number printed on the page. `0` is a
named error rather than a silent synonym for the first page.

| Function | Signature | Description |
|---|---|---|
| `pdf.page_count` <br> *Added in v0.3.0* | `pdf.page_count(src)` | Counts a PDF's pages. `src` (string path, or `Vec` of bytes) is the document. Counted by walking the page tree to its leaves rather than trusting the catalog's own `/Count` field, which a malformed or incrementally-updated file can disagree with. Returns a `Num`. |
| `pdf.info` <br> *Added in v0.3.0* | `pdf.info(src)` | Reads what a PDF says about itself, without extracting any content. `src` (string path, or `Vec` of bytes) is the document. Returns a `Record` with fields: `pages` (`Num`, as `pdf.page_count`), `version` (`Str`, the header version such as `"1.6"`), `title`, `author`, `subject`, `creator`, `producer` (each `Str`, **or `none`** where the file's `/Info` dictionary omits the key — `none` and `""` are different answers and this does not merge them), and `encrypted` (`Bool`, whether the file carries an `/Encrypt` dictionary, which explains in advance why another call on it might fail). |
| `pdf.merge` <br> *Added in v0.3.0* | `pdf.merge(list)` | Joins several PDFs into one, in list order, keeping every page. `list` is a `List` of sources (each a string path or a `Vec` of bytes); an empty list is an error. Bookmarks/outlines are dropped — a half-merged outline tree pointing at pages that moved is worse than none. Returns a `Vec` of bytes, the new PDF. |
| `pdf.write_merge` <br> *Added in v0.3.0* | `pdf.write_merge(path, list)` | Exactly `pdf.merge`, written to a file instead of returned. `path` (string) is the destination, overwritten if it exists; `list` is as above. Denied in sandboxed execution. Returns `Nothing`. |
| `pdf.extract_pages` <br> *Added in v0.3.0* | `pdf.extract_pages(src, pages)` | A new PDF holding just the pages asked for. `src` (string path, or `Vec` of bytes) is the document; `pages` is a page number, or a `Vec`/`List` of them (`1 to 3` works — it is already a `Vec`), numbered from 1. Pages come out in the **document's** order, not the order asked for, and a page the document does not have is a named error rather than a silently shorter file. Implemented by removing the other pages, so the ones that stay keep their own fonts, resources and annotations. Returns a `Vec` of bytes. |
| `pdf.write_pages` <br> *Added in v0.3.0* | `pdf.write_pages(path, src, pages)` | Exactly `pdf.extract_pages`, written to a file instead of returned. `path` (string) is the destination, overwritten if it exists. Denied in sandboxed execution. Returns `Nothing`. |
| `pdf.extract_text` <br> *Added in v0.3.0* | `pdf.extract_text(src, [pages], [page=])` | Text reconstructed from a PDF's content streams. `src` (string path, or `Vec` of bytes) is the document; `pages` (optional, positional or `page=`) is a page number or a `Vec`/`List` of them, defaulting to every page. Returns a `Str`. **Read the warning below before relying on this.** |

#### `pdf.extract_text` is the weak one, and says so

A PDF does not store paragraphs. It stores "draw glyph X at (321, 418)",
so extraction is a *reconstruction*, and it only works when a document's
fonts use a standard encoding or ship a `/ToUnicode` map. Two specific
limits apply:

- The engine reads the page's **own** content stream and does not descend
  into Form XObjects — which is what `pdfcrop` and Adobe Illustrator wrap
  page content in. Text inside one is invisible to it.
- Word and line breaks are approximate. PDF has no obligation to encode a
  space character, inter-word spacing is often kerning, and breaks land
  per text-showing operator rather than per word. Treat the result as
  **searchable, not as a transcript**.

What this module will *not* do is hand back `""` and let that read as
"this document has no text". When extraction produces nothing, it checks
whether the requested pages can reach any embedded font — following Form
XObjects, not just the page's own resources. No font reachable means the
page really has no text (a scanned bitmap, a pure vector drawing) and
`""` is the honest answer. A font reachable but no text out means the
extractor failed, and that is an **error** naming the reason, not an
empty string.

Measured on the 34 PDFs committed in this repository (2026-09-23): 21
gave readable text, 8 raised that error (every `pdfcrop`-produced file
among them), and 5 were genuinely textless. None returned the wrong text.
`pdf.page_count`, `pdf.info`, `pdf.merge` and `pdf.extract_pages` were
correct on all 34 — they are the solid part of this module.

```qu,ignore
import pdf

i = pdf.info("paper.pdf")
print("{i.pages} pages, PDF {i.version}, produced by {i.producer}")

# Join two files, then take one page out of the result -- no disk in
# between, because every function also accepts bytes.
both = pdf.merge(["intro.pdf", "results.pdf"])
print(pdf.page_count(both))
pdf.write_pages("just_page_3.pdf", both, 3)

# A page range. `1 to 5` is already a Vec, so it just works.
pdf.write_pages("front.pdf", "paper.pdf", 1 to 5)
```

Text extraction, written the way it should be — assuming it can fail:

```qu,ignore
import pdf

try
    t = pdf.extract_text("paper.pdf", page = 1)
    print(t)
catch err
    print("no text layer this engine can read: {err}")
end
```

None of the `pdf` examples here are run by the build: they need document
files that are not in the repository.

## Data-Format Conversion (JSON / CSV / XML)

`read_csv`/`write_csv` above and `save`/`save_all`/`load` (see
[Collections, Strings & Data Frames](collections-strings.md) for the
`json_to_value`/`"type"`-tag shape `save`/`load` use) all read/write a
FILE. The builtins below (§ data-format conversion builtins, 2026-09-01)
are the same conversions as plain in-memory STRINGS instead — for building
a request body, embedding data in another format's document, or converting
between formats without ever touching disk. Two primitives per format
(`xify(value) -> str` / `parse_x(str) -> value`), plus six pairwise
converters. Four of the six are one-line compositions of those
primitives — e.g. `json2csv(s)` is exactly `csvify(parse_json(s))` — not
four independent implementations. The other two, `csv2json` and
`xml2json`, deliberately do **not** go through `jsonify`: they produce a
plain JSON array of row objects (the shape most external JSON consumers
expect) rather than `jsonify`'s own `{"type":"table", ...}` envelope — see
their table entries below for the exact contract. A conversion that
doesn't make sense for the input's shape
(a bare number has no CSV shape; a live handle like a worker or mutex has
no meaning in any of these formats) is a clear, named error, not a panic or
a silently wrong result.

| Function | Signature | Description |
|---|---|---|
| `jsonify` | `jsonify(value)` | Converts any `value` (any `Value` — number, string, `Vec`, `Table`, `Record`, ...) to a JSON string — exactly the `save`/`load` on-disk shape (see the link above: a `{"type": ..., ...}` envelope), just returned as a string instead of written to a file. Returns a string. |
| `parse_json` | `parse_json(str)` | `jsonify`'s inverse. `str` (string) is JSON text — either a `jsonify`/`save`-style `{"type": ...}` envelope, or ordinary JSON from anywhere else (e.g. an HTTP API response), which is mapped in generically. Returns the reconstructed `Value`. |
| `csvify` | `csvify(table)` | Converts `table` (Table) to CSV text — exactly `write_csv`'s own formatting (`Table::to_csv`), just returned as a string instead of written to a file. Returns a string. Errors clearly on anything that isn't a `Table` (a bare scalar/vector has no tabular shape). |
| `parse_csv` | `parse_csv(str)` | Converts `str` (string, CSV text) to a `Table` — exactly `read_csv`'s own parsing (`Table::from_csv`, the `headers=true, sep=",", decimal="."` defaults; use `read_csv` on a real file for the `headers=`/`sep=`/`decimal=` keyword arguments). Returns a `Table`. |
| `xmlify` | `xmlify(value)` | Converts any `value` (any `Value`) to an XML string — see "The `Value`<->XML mapping" below. Returns a string. |
| `parse_xml` | `parse_xml(str)` | `xmlify`'s inverse. `str` (string) is XML text in `xmlify`'s own element-naming convention. Returns the reconstructed `Value`. |
| `json2csv` | `json2csv(str)` | `str` (string, JSON text) is converted to CSV text: exactly `csvify(parse_json(str))`. Returns a string. |
| `csv2json` | `csv2json(str)` | `str` (string, CSV text) is converted to JSON. **Not** `jsonify(parse_csv(str))` — it emits a plain JSON array of one flat `{"col": value, ...}` object per row (the natural shape for feeding to another JSON-consuming tool), rather than `jsonify`'s own `{"type":"table", ...}` envelope. `json2csv` accepts either shape back, since `parse_json` understands both. Returns a string. |
| `xml2json` | `xml2json(str)` | `str` (string, XML text in `xmlify`'s convention) is converted to JSON. Like `csv2json`, this produces the same plain array-of-row-objects shape, **not** `jsonify(parse_xml(str))`'s type-tagged envelope. Returns a string. |
| `json2xml` | `json2xml(str)` | `str` (string, JSON text) is converted to XML: exactly `xmlify(parse_json(str))`. Returns a string. |
| `xml2csv` | `xml2csv(str)` | `str` (string, XML text) is converted to CSV text: exactly `csvify(parse_xml(str))`. Returns a string. |
| `csv2xml` | `csv2xml(str)` | `str` (string, CSV text) is converted to XML: exactly `xmlify(parse_csv(str))`. Returns a string. |

#### Overloads: `parse_json`

#### Case: a `jsonify`/`save` envelope

`str` carries a top-level `"type"` string field — the tag `jsonify`/`save`
attach. Returns the exact typed `Value` that tag names, reconstructed
in full, not a generic guess from the JSON's own shape.

```qu
env = jsonify(3 + 4j)
back = parse_json(env)
print(env)          # {"type":"complex","re":3.0,"im":4.0}
print(type(back))   # complex
print(back)         # 3 + 4j
```

#### Case: ordinary JSON, no envelope

`str` is plain JSON from anywhere else — an HTTP API response, hand-written
JSON, or the same envelope with its `"type"` tag stripped off. Mapped in
generically instead: a bare object becomes a record field-for-field, so the
identical `re`/`im` pair that named a `Complex` above comes back as a plain
record once the tag naming it is gone.

```qu
env = jsonify(3 + 4j)
plain = replace(env, "\"type\":\"complex\",", "")
v = parse_json(plain)
print(plain)         # {"re":3.0,"im":4.0}
print(type(v))       # record
```

### The `Value`<->XML mapping

XML had no support anywhere in Qu before this (via the `quick-xml` crate —
see `engine/crates/qu-interp/Cargo.toml`'s own comment for why). Every
value serializes to one XML element named after its type (`<num>`, `<vec>`,
`<table>`, ...) — the direct analogue of `jsonify`'s own `"type"` tag, just
moved from a JSON object key to the thing XML tags things with (an element
name). A few shapes get a genuinely XML-native structure instead of a
mechanical port of the JSON one:

- **`Table`** is row-oriented, not JSON's column-major arrays: one `<row>`
  per table row, with one child element per column inside it, named after
  the column. A `<columns>` list up front declares column order and each
  column's `kind` (`"num"`/`"str"`) — necessary, not decorative: without
  it, a text column that happens to hold only numeral-looking values
  (`"007"`) would be indistinguishable from a numeric column on the way
  back in.
- **`Model`/`Record`/`Dict`** fields become child elements NAMED after the
  field (`<coef>...</coef>`), each wrapping that field's own
  self-describing node — the XML analogue of `jsonify`'s "object key ->
  recursively-serialized value" choice for these.
- A `Table` column name or `Record`/`Dict`/`Model` field name that isn't a
  legal XML element name (`[A-Za-z_][A-Za-z0-9_]*`) is a clear `xmlify`
  error naming the offending name — the practical way to hit this is a
  `read_csv`/CSV-text header with a space in it (`csv2xml`), since every
  name that originates from real Qu source already satisfies the check.

See `engine/crates/qu-interp/src/xml_ops.rs`'s own module doc comment for
the complete variant-by-variant mapping table.

The example below builds a small two-row `Table` from CSV text, converts it
to XML with `xmlify` to show the `<columns>`/`<row>` shape described above,
parses it back with `parse_xml` to confirm the round trip preserves the
data, and then runs the same CSV text through `csv2json` — printed here to
make the point from the table above concrete: the result is a plain array
of row objects, not `jsonify`'s type-tagged envelope:

```qu
t = parse_csv("name,age\nAlice,30\nBob,25\n")
print(xmlify(t))
# <table>
#   <columns>
#     <column kind="str">name</column>
#     <column kind="num">age</column>
#   </columns>
#   <row>
#     <name>Alice</name>
#     <age>30</age>
#   </row>
#   <row>
#     <name>Bob</name>
#     <age>25</age>
#   </row>
# </table>

back = parse_xml(xmlify(t))
print(back.name[0])   # Alice

j = csv2json("product,price\nWidget,9.99\n")
print(j)               # [{"product":"Widget","price":9.99}]
```

`jsonify`/`parse_json` are the JSON side of the same pair `csvify`/`parse_csv`
and `xmlify`/`parse_xml` already show above. Of the pairwise converters not
shown elsewhere on this page, `json2xml`, `xml2csv`, and `csv2xml` are each
exactly the one-line composition their own table entry says they are;
`xml2json` is the row-object conversion `csv2json` just demonstrated,
applied to XML input instead of CSV:

The next example chains every converter on this page against the same
two-row table, in both directions, to demonstrate that the whole set forms
a connected graph — CSV to JSON, JSON back to CSV, CSV to XML and back, JSON
to XML and back — and that `parse_json` at the very end can still recover a
real `Table` even though the JSON it's reading came from `xml2json`'s
row-object shape rather than `jsonify`'s own envelope:

```qu
t = parse_csv("product,price\nWidget,9.99\nGadget,4.50\n")

csv_text = csvify(t)
json_text = jsonify(t)

back_from_json = json2csv(json_text)   # csvify(parse_json(json_text))
print(back_from_json)

xml_from_csv = csv2xml(csv_text)       # xmlify(parse_csv(csv_text))
csv_from_xml = xml2csv(xml_from_csv)   # csvify(parse_xml(xml_from_csv))
print(csv_from_xml)

xml_from_json = json2xml(json_text)    # xmlify(parse_json(json_text))
json_from_xml = xml2json(xml_from_json) # the row-object array, not jsonify(parse_xml(...))
parsed = parse_json(json_from_xml)      # parse_json accepts that shape too -- back to a real Table
print(nrow(parsed))                     # 2
```

See `catalog/qu_format_conversion.qu` for a fuller worked demo (JSON/CSV/XML
round trips plus two pairwise converters on real sample data, with printed
output proving correctness field-by-field).

## Worked examples

The most basic file-writing pattern in Qu: open a handle in `"w"` mode
(truncate-or-create), write one line at a time with `write_line`, and close
the handle when done to flush it to disk. This is the shape almost every
logging or report-writing script starts from. The example prints
`file_size` afterward just to make the write's effect on disk visible:

```qu
f = fopen("log.txt", "w")
write_line(f, "run started")
write_line(f, "value = " + str(42))
close(f)
print(file_size("log.txt"))    # 23 -- "run started\n" + "value = 42\n"
```

Reading it back: `read_line` returns `Nothing` at EOF, but Qu today has
neither a source-level `nothing` literal nor `==`/`!=` support for two `Str`
operands (`compare`'s catch-all falls back to `as_num()`, which errors on a
string — the same pre-existing gap noted next to `contains`/`indexof` in
[Collections, Strings & Data Frames](collections-strings.md)), so an
EOF-sentinel-comparison loop isn't expressible yet. For a known number of
lines, a bounded loop works cleanly; for an unknown number, `read_all` is
the more honest tool today:

```qu
f = fopen("log.txt", "r")
for i = 0 to 1
    print(read_line(f))
end for
close(f)

g = fopen("log.txt", "r")
print(read_all(g))
close(g)
```

Reading a whole CSV straight into a `Table` (no `fopen` needed) and working
with it using the Data Frame builtins from
[Collections, Strings & Data Frames](collections-strings.md), then writing
a filtered subset back out to a second CSV file:

The file is written here before it is read, so the example runs anywhere
— including a fresh clone. It used to open a `measurements.csv` that no
part of the repository created, which meant the block could not run, and a
block that cannot run is dropped from the rendered page **without the
build reporting anything**: the page shipped with the example missing and
the build said it succeeded.

```qu
measurements = table(
    x = [-1.5, 0.5, 2.0, -0.25, 3.75],
    sensor = ["a", "b", "a", "c", "b"]
)
write_csv(measurements, "measurements.csv")

df = read_csv("measurements.csv")
print(nrow(df))
print(describe(df))
write_csv(filter(df, df.x > 0), "positive_only.csv")
print(read_all_text("positive_only.csv"))
```

A minimal binary I/O round trip: open a handle in `"wb"` mode, write a
32-bit integer followed by a 32-bit float, close it, then reopen the same
file in `"rb"` mode and read the two values back in the same order they
were written. This is the pattern every other binary example on this page
builds on — the typed `read_*`/`write_*` pairs only work correctly when the
read order matches the write order, since there's no type tag on disk to
check against:

```qu
f = fopen("record.bin", "wb")
write_int32(f, 12345)
write_float(f, 3.5)
close(f)

g = fopen("record.bin", "rb")
n = read_int32(g)
x = read_float(g)
close(g)
print("n = {n}, x = {x}")
```

`read_all_text(path)` is the one-call shortcut for "give me this whole file
as a string" — no `fopen`/`close` pair needed when a script only ever
wants to read a file once, in full, such as loading a small config or
notes file at startup. The example below writes a two-line file the normal
way, then reads the whole thing back in one call:

```qu
n = fopen("notes.txt", "w")
write_line(n, "line one")
write_line(n, "line two")
close(n)
print(read_all_text("notes.txt"))
# line one
# line two
```

When a file holds a run of same-typed numbers rather than one fixed-width
record, `read_values`/`read_array` read all of them in a single call
instead of looping over `read_double`/`read_int32`/etc. one value at a
time. `read_values(f, type, n)` reads `n` values of the given type from an
already-open handle (useful mid-stream, alongside other reads on the same
file); `read_array(path, type=)` skips `fopen` entirely and reads a whole
file as one typed array in one call. The example below writes three
64-bit doubles, checks the file's size with `file_size` before opening
anything, then reads the same three numbers back both ways to show they
agree:

```qu
f = fopen("temps.bin", "wb")
write_double(f, 20.5)
write_double(f, 21.0)
write_double(f, 19.8)
close(f)

print(file_size("temps.bin"))            # 24 -- 3 doubles, 8 bytes each

g = fopen("temps.bin", "rb")
vals = read_values(g, "f64", 3)
close(g)
print(vals)                              # [20.5, 21, 19.8]

whole = read_array("temps.bin", type="f64")
print(whole)                             # [20.5, 21, 19.8]
```

A file of fixed-size records — logger output, an instrument's native save
format, anything with a repeating struct layout — is read with
`read_struct`/`read_structs` instead of unpacking each field by hand.
`fields` is a list of `(name, type)` pairs describing one record's layout,
in byte order; `read_struct(f, fields)` reads exactly one record and
returns it as a record value with one field per name, while
`read_structs(f, fields, n)` reads `n` of them into a `Table`, one column
per field, which is the more useful shape once there are enough records to
want to work on them column-wise. The example below writes two
`(f64, u16)` records back to back, then reads first one and then both,
column-wise:

```qu
f = fopen("records.bin", "wb")
write_double(f, 0.0)
write_uint16(f, 12)
write_double(f, 0.5)
write_uint16(f, 15)
close(f)

fields = [["t", "f64"], ["ch", "u16"]]

g = fopen("records.bin", "rb")
one = read_struct(g, fields)
close(g)
print(one.t)                             # 0
print(one.ch)                            # 12

h = fopen("records.bin", "rb")
tbl = read_structs(h, fields, 2)
close(h)
print(nrow(tbl))                         # 2
print(tbl.ch)                            # [12, 15]
```

Interop with MATLAB doesn't require a MATLAB installation just to read
data it produced: `read_mat(path)` reads a `.mat` file directly (via the
`matfile` crate) and hands back every saved variable as a field on one
record, named after the variable, so `.mat` files from a lab instrument or
an existing MATLAB pipeline can be consumed without leaving Qu. The
example below reads a `.mat` file containing a single saved variable named
`temperature` and prints its values:

Not run by the build, and it cannot be: Qu reads MATLAB files but has no
writer for them, so this example cannot create its own input the way the
CSV one above does. It needs a `.mat` produced by MATLAB or SciPy.

```qu,ignore
vars = read_mat("measurements.mat")
print(vars.temperature)                  # [20.5, 21, 19.8]
```

`argv()` and `getenv(name)` are how a Qu script reads its own invocation
context — command-line arguments and environment variables — the same
information a shell script would get from `$1`/`$2` and `$MY_VAR`. Every
argument after a literal `--` on the command line becomes one element of
`argv()`'s list; `getenv` reads one named environment variable, coming
back as `none` (Qu's absent-value marker, composing with `??`) if it isn't
set. The example below assumes the script was invoked as
`qu run analyse.qu -- data.csv 1000`:

```qu
args = argv()
print(length(args))                      # 2 -- "data.csv" and "1000"
print(args)                              # ("data.csv", "1000")

level = getenv("QU_LOG") ?? "info"
print(level)
```

`write_report(path)` bundles everything a script has printed and plotted
so far — printed output, tables, and any figures produced up to that point
in execution — into one standalone HTML file with nothing linked from
outside it, suitable for emailing or archiving without also shipping a
folder of image files. The example below computes a small sum, prints it,
then writes and confirms the report file:

```qu
x = 1:10
print("sum = " + str(sum(x)))
write_report("summary.html")
print(file_exists("summary.html"))       # true
```

Running another language inline is for the case where a library or routine
simply has no Qu equivalent yet: `python_exec`/`js_exec`/`matlab_exec` all
share one contract. `code` (string) is the guest-language source to run.
`vars=` (optional, a Record/Dict) arrives as ordinary variables in the
guest — built with Qu's `{name=value, ...}` record syntax, not JSON-style
`{"name": value}`. An injected output-path variable (`_qu_output_path` for
Python and JavaScript, `qu_output_path` for MATLAB — see the note above on
why that one can't take a leading underscore) is where the guest writes a
JSON result, and the call always returns a record of `{stdout, stderr,
success, exit_code, result}`. The example below runs a one-line Python
snippet with no injected variables, then a second Python snippet that
receives `x` via `vars=` and returns a computed result, then the same
pattern once in JavaScript:

```qu
py = python_exec("result = 1 + 1\nimport json\njson.dump(result, open(_qu_output_path, 'w'))")
print(py.result)                         # 2

py2 = python_exec("import json\nresult = x * 2\njson.dump(result, open(_qu_output_path, 'w'))", vars={x=21})
print(py2.result)                        # 42

js = js_exec("const fs = require('fs'); fs.writeFileSync(_qu_output_path, JSON.stringify(1 + 1));")
print(js.result)                         # 2
```

`matlab_exec` follows the exact same contract — `matlab_exec(code, [vars=])`,
same `{stdout, stderr, success, exit_code, result}` return shape, `code`
writing its result to the injected `qu_output_path` variable — but actually
running it requires a working local MATLAB installation with a valid
license, which this environment does not currently have (the license here
has expired), so no runnable example is included for it. A failed run does
not panic: `success` comes back `false` and `stderr` carries MATLAB's own
error text (a license error, in this environment's case), the same
"describe the failure, don't crash" contract every other guest-language
failure follows.

## More functions

| Function | Signature | Description |
|---|---|---|
| `save` | `save(path, name1, [name2, ...])` | Writes the named variables — `path` (string, destination file) plus one or more variable names (strings) given explicitly as extra arguments — to a JSON file, each tagged with its own type. Returns `Nothing`. Unlike `save_all`, `save` only ever writes the variables it is explicitly told to; there is no implicit "the whole workspace" behavior. |
| `load` | `load(path)` | Restores every variable a `save`/`save_all` call wrote. `path` (string) is the file to read. Returns `Nothing`; each saved variable is written back into the current workspace under its original name, overwriting any existing variable of the same name (the same "load replaces" semantics MATLAB's own `load` has). |
| `save_all` | `save_all(path)` | Saves literally every current binding — every user variable plus Qu's own built-in constants (`pi`, `e`, `tau`, `none`, the `Qu*` escape-character constants, ...) — to `path` (string). Returns `Nothing`. There is no "scratch" filtering; use `save` with an explicit name list to save only specific variables. |
| `read_all_text` | `read_all_text(path)` | Reads `path` (string) in full. Returns the whole file's content as one string. For splitting it into lines, open a handle with `fopen` and loop over `read_line`; for a file that isn't text, read it as raw bytes instead — `read_bin(f, n)` on a handle opened in binary mode, or `read_array`/`mmap_read` for typed/binary data. |
| `read_values` | `read_values(f, type, n)` | Reads `n` (number, count) typed values from an already-open handle. `f` (file handle, opened readable, positioned wherever the caller left it) is the source. `type` (string, one of `f64`/`f32`/`i64`/`u64`/`i32`/`u32`/`i16`/`u16`/`i8`/`u8`/`byte`, with the aliases `double`, `float`, `int64`, `uint64`, `int32`, `uint32`, `int16`, `uint16` for the first eight respectively) is the element type. Returns a `Vec` of up to `n` numbers (fewer if the file runs out first). Useful mid-stream, interleaved with other reads on the same handle; for a whole file in one call, use `read_array` instead. |
| `read_array` | `read_array(path, [type="f64"], [endian="little"])` | Reads a whole binary file as one typed array, without an explicit `fopen`. `path` (string) is the file. `type` (string, optional, default `"f64"`, same type names as `read_values`) is the element type. `endian` (string, optional, default `"little"`, or `"big"`) is the byte order. Returns a `Vec` of numbers — one per element in the file. Errors if the file's byte length isn't a whole multiple of the element width, rather than silently truncating a partial trailing element. |
| `write_array` | `write_array(path, data, [type="f64"], [endian="little"])` | Writes a whole `Vec`/`Mat`/`Signal` (or a bare number/`bool`) to a binary file in one call, without an explicit `fopen` — the write-side counterpart `read_array` never had. `path` (string) is the destination, overwritten if it exists. `data` is the value to write; a `Mat` is flattened in its own column-major storage order (the same order `A -> (r, c)` fills), not row-major — reading it back with `read_array` and reshaping recovers the original matrix, since a flat binary file carries no shape of its own. `type`/`endian` as in `read_array`. Returns `Nothing`. |
| `read_mat` | `read_mat(path)` | Reads a MATLAB `.mat` file (via the `matfile` crate) without needing a MATLAB installation. `path` (string) is the file. Returns a record with one field per saved variable, named after the variable, each holding that variable's data (a scalar, vector, or matrix depending on what was saved). |
| `read_struct` | `read_struct(f, fields)` | Reads one fixed-width binary record from `f` (file handle, opened readable). `fields` (list of `[name, type]` pairs, e.g. `[["t", "f64"], ["ch", "u16"]]`) describes the record's layout in byte order, using the same type names as `read_values`. Returns a record with one field per entry in `fields`, or `Nothing` if fewer bytes remain than a full record needs. |
| `read_structs` | `read_structs(f, fields, n)` | Reads up to `n` (number) fixed-width binary records from `f` (file handle, opened readable), using the same `fields` layout `read_struct` takes. Returns a `Table` with one column per field (not one row-record per call) — the more useful shape once there are enough records to work on column-wise, stopping early (with however many complete records were read) if the file runs out. |
| `file_size` | `file_size(path)` | Reports a file's size without opening it. `path` (string) is the file to stat. Returns a number: the size in bytes on disk. Worth asking before you read something you did not write. |
| `argv` | `argv()` | Takes no arguments. Returns a list of strings: the command-line arguments given after a literal `--` in the invocation (e.g. `qu run analyse.qu -- data.csv 1000` gives `("data.csv", "1000")`). |
| `getenv` | `getenv(name)` | Reads one environment variable. `name` (string) is the variable's name. Returns its value as a string, or `none` if it isn't set — so `getenv("HOME") ?? "/tmp"` is the idiom for a default. |
| `write_report` | `write_report(path)` | Writes a standalone HTML report to `path` (string) — every figure, table, and printed line the script has produced up to this point in execution, bundled into one file with nothing linked from outside it. Returns `Nothing`. |
| `json2csv` | `json2csv(text)` | Converts `text` (string, JSON) to CSV text. Returns a string. Exactly `csvify(parse_json(text))`; accepts both `jsonify`'s type-tagged JSON and the plain row-object JSON `csv2json` produces. |
| `csv2json` | `csv2json(text)` | Converts `text` (string, CSV) to JSON. Returns a string: a plain JSON array of one flat `{"col": value, ...}` object per row — **not** `jsonify`'s type-tagged envelope. The round trip through `json2csv` is lossy in one direction: CSV carries no type tags, so a value that comes back is a number or string by looking at it, not by a recorded type. |
| `csv2xml`, `xml2csv` | `csv2xml(text)` / `xml2csv(text)` | The same conversion pair for XML, and exact compositions of the primitives above: `csv2xml(text)` is `xmlify(parse_csv(text))`; `xml2csv(text)` is `csvify(parse_xml(text))`. Both take/return a string. |
| `json2xml` | `json2xml(text)` | Converts `text` (string, JSON) to XML text. Returns a string. Exactly `xmlify(parse_json(text))`. |
| `xml2json` | `xml2json(text)` | Converts `text` (string, XML in `xmlify`'s own tag convention) to JSON. Returns a string: the same plain array-of-row-objects shape `csv2json` produces, **not** `jsonify(parse_xml(text))`'s type-tagged envelope. `parse_xml` (and so this function) only understands the specific element/attribute conventions `xmlify` itself produces — arbitrary hand-written XML with its own attributes is not a supported input and errors clearly rather than being guessed at. |
| `python_exec`, `js_exec` | `python_exec(code, [vars=])` / `js_exec(code, [vars=])` | Runs `code` (string, source in the guest language) as a subprocess. `vars` (optional, a Record) is injected into the guest as ordinary variables. The guest writes its result as JSON to an injected output-path variable (`_qu_output_path`), which this call reads back. Returns a record `{stdout, stderr, success, exit_code, result}`: captured output streams, whether the process exited cleanly, its exit code, and the parsed JSON result. For a library with no Qu equivalent yet — and a dependency to be honest about in anything published using it. |
| `matlab_exec` | `matlab_exec(code, [vars=])` | The same contract as `python_exec`/`js_exec`, run through a local MATLAB installation instead: `code`'s result is written to an injected `qu_output_path` variable (no leading underscore — MATLAB identifiers can't start with one) and the call returns the same `{stdout, stderr, success, exit_code, result}` record. Requires a working, licensed local MATLAB install; a failure (e.g. an expired license) does not panic — it comes back as `success = false` with MATLAB's own error text in `stderr`. Returns a `Record` with fields `stdout` (a `Str`), `stderr` (a `Str`), `success` (a `Bool`), `exit_code` (a `Num`) and `result` (whatever the parsed JSON held, or `none` if MATLAB wrote no result). |
