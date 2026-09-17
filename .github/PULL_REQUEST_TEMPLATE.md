## What this changes

## Why

## Testing

- [ ] `cargo test --release -p qu-interp --lib --manifest-path engine/Cargo.toml` passes
- [ ] `cargo test --release -p qu-core --lib --manifest-path engine/Cargo.toml` passes
- [ ] New behaviour has a new test, not just a manual check
- [ ] If this touches `qu-studio-tauri`, `npx tsc --noEmit` is clean
