# NOTICE — `engine/` is a prototype, not the canonical engine

The **canonical Qu engine is the root `Cargo.toml` workspace** (`qu-parser`,
`qu-core`, `qu-interpreter`, `qu-dsp`, `qu-data`, `qu-ml`, `qu-gpu`, `qu-wasm`).
Build there.

This `engine/` workspace (`qu-lexer`, `qu-syntax`, `qu-interp`, `qu-cli`) is a
**working reference prototype** of the parser + interpreter — the two pieces the
root workspace is still missing (root `qu-parser` has a lexer but no parser/AST;
root `qu-interpreter` is a stub). It parses and runs `.qu` end-to-end with 27
green tests.

**Intended fate (see `../IMPL.md` §5):** fold `qu-syntax` into
`qu-parser::parser`/`::ast` (consuming root's existing `Token`/`TokenKind`) and
`qu-interp` into `qu-interpreter`, then **delete `engine/`**. Do this in
coordination with the `qu-parser`/`qu-interpreter` owners — do not overwrite
their `lexer.rs`.

Until then, `engine/` is safe to ignore: it is a standalone workspace and is not
a member of the root workspace, so it does not affect the root build.
