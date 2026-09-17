# Qu engine

Reference implementation of the Qu language. **Rust**, dependency-free core,
built two ways (native + WASM/WebGPU). See [`../IMPL.md`](../IMPL.md) for the
plan and the C-vs-Rust-vs-JIT decision.

```bash
cargo test --workspace
cargo run -p qu-cli -- run examples/demo_signal.qu
cargo run -p qu-cli -- repl
```

Crates:

| crate | role |
|---|---|
| `qu-lexer` | tokens (grammar lexical classes) |
| `qu-syntax` | AST + recursive-descent parser |
| `qu-interp` | tree-walk reference interpreter (M2 subset) |
| `qu-cli` | the `qu` command |

Authoritative language definition: `../docs/qu-language-spec.md` and the frozen
grammar `../docs/qu-grammar.ebnf`.
