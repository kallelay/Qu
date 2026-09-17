# Contributing

## Before you start

For anything more than a small fix, open an issue first describing what
you want to change and why — saves both of us the work if the direction
turns out to be wrong. For a bug fix or a small, self-contained
addition, a PR alone is fine.

## Setup

See [Building from Source](../../wiki/Building-from-Source) in the wiki
for the full build/test instructions.

## What a PR needs

The [PR template](.github/PULL_REQUEST_TEMPLATE.md) covers this, but the
short version:

- **A real test, not a manual check.** If you fixed a bug, there should
  be a test that fails without your fix and passes with it. If you added
  a builtin, there should be a test with a hand-verifiable expected
  value (not just "it runs without erroring").
- **`cargo test --release -p qu-interp --lib` and `-p qu-core --lib`
  both pass.** CI checks this on every PR, but running it locally first
  saves a round trip.
- **If you touched `qu-studio-tauri`**, `npx tsc --noEmit` should be
  clean.

## Design commitments this project won't trade away

From the README, because a PR that fights these won't get merged
regardless of how well it's tested elsewhere:

- **A keyword the callee never reads is an error**, not a silent no-op.
- **A function cannot rewrite its caller's variables** without `global`.
- **Silence is the worst failure.** Where the engine can guess or say
  so, it should say so.

## Code style

`cargo fmt` is run in CI but currently advisory (not blocking) — a
handful of pre-existing files aren't fully formatted yet. New code
should still be `cargo fmt`-clean; don't add to the gap.
