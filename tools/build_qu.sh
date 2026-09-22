#!/usr/bin/env bash
# The canonical way to build the `qu` engine binary.
#
#   bash tools/build_qu.sh              # release (default)
#   bash tools/build_qu.sh --debug      # debug
#   bash tools/build_qu.sh --features gpu
#   bash tools/build_qu.sh -j 1         # serialise (shared/contended box)
#
# WHY THIS EXISTS. There was no single correct way to build Qu, and the
# most obvious one did not work. From the repo root:
#
#   cargo build --release --bin qu
#   error: no bin target named `qu` in default-run packages
#
# because the root `Cargo.toml` is a legacy workspace (qu-core, qu-dsp,
# qu-data, qu-ml, qu-gpu, qu-wasm) that does not contain the CLI at all --
# the real engine is a SEPARATE workspace under `engine/`, with the bin
# defined in `engine/crates/qu-cli/Cargo.toml`. The error names the right
# symbol and gives no hint where to look.
#
# A survey of the repo's own docs and scripts found SIX distinct spellings
# of "build qu", plus 26 bare `cargo build` lines that only work if you
# already happen to be standing in `engine/`. Reported by the perf lane
# (Qu-96, 2026-09-16) after it cost them a build cycle; the papercut is
# that every one of those spellings is someone's reasonable first guess.
#
# This script is the one spelling. It works from ANY directory inside the
# checkout, refuses to build from a parked tree, and -- the part that
# matters -- verifies the artifact by RUNNING it rather than by trusting
# cargo's summary line or the file's timestamp.

set -euo pipefail

profile="release"
jobs=""
features=""

while [ $# -gt 0 ]; do
  case "$1" in
    --debug)   profile="debug" ;;
    --release) profile="release" ;;
    --features)
      shift
      [ $# -gt 0 ] || { echo "--features expects an argument" >&2; exit 2; }
      features="$1"
      ;;
    -j)
      shift
      [ $# -gt 0 ] || { echo "-j expects an argument" >&2; exit 2; }
      jobs="$1"
      ;;
    -h|--help)
      sed -n '2,32p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown option: $1 (try --help)" >&2
      exit 2
      ;;
  esac
  shift
done

# Resolve from the CALLER's position, not this script's, so that a build
# run inside a worktree builds THAT worktree -- the same reasoning
# `refuse_if_parked.sh` documents for itself.
repo_root=$(git rev-parse --show-toplevel 2>/dev/null || echo "")
if [ -z "$repo_root" ]; then
  echo "REFUSING to build: not inside a git checkout." >&2
  exit 1
fi

# Parked-tree refusal. The marker is checked DIRECTLY here rather than
# only delegating to `tools/refuse_if_parked.sh`, because delegating alone
# fails in exactly the case that matters.
#
# Measured 2026-09-16: D:\QuWorkspace is parked and carries the
# `.PARKED-STALE-DO-NOT-BUILD` marker, but it does NOT carry
# `tools/refuse_if_parked.sh` -- the guard postdates the parking, so the
# tree it most needs to stop is the one tree without it. An
# `if [ -f <guard> ]` wrapper therefore skipped the check silently and
# built the parked tree happily (verified: it did, in 2m07s). The marker
# travels with the parked tree by definition; the guard script does not.
if [ -f "$repo_root/.PARKED-STALE-DO-NOT-BUILD" ]; then
  echo "REFUSING to build: $repo_root is a PARKED tree." >&2
  echo "  (marker: $repo_root/.PARKED-STALE-DO-NOT-BUILD)" >&2
  echo "  A build from a parked tree silently ships whatever engine was" >&2
  echo "  current when it was parked, with no error and no stamp." >&2
  echo "  Build in your own worktree instead." >&2
  exit 1
fi
# Still delegate when the fuller guard IS present: it knows checks this
# one does not, and duplicating its logic would let the two drift.
if [ -f "$repo_root/tools/refuse_if_parked.sh" ]; then
  bash "$repo_root/tools/refuse_if_parked.sh"
fi

manifest="$repo_root/engine/Cargo.toml"
if [ ! -f "$manifest" ]; then
  echo "REFUSING to build: no engine workspace at $manifest" >&2
  exit 1
fi

args=(build --manifest-path "$manifest" -p qu-cli)
[ "$profile" = "release" ] && args+=(--release)
[ -n "$features" ] && args+=(--features "$features")
[ -n "$jobs" ] && args+=(-j "$jobs")

echo "building qu (${profile}${features:+, features: $features}) from $repo_root"
echo "  cargo ${args[*]}"

# `set -e` already aborts on a non-zero cargo exit, but be explicit: a
# cargo that fails after printing warnings still looks busy and green in a
# scrollback, and this repo has been bitten by reading the summary line
# instead of the status.
if ! cargo "${args[@]}"; then
  echo "BUILD FAILED (cargo exited non-zero)" >&2
  exit 1
fi

# `.exe` FIRST. Under MSYS/Git-Bash a test on the extension-less name
# succeeds anyway (it silently appends `.exe` for you), so checking that
# spelling first would print a path the user cannot actually type or hand
# to another tool -- a small lie, and the annoying kind to debug.
exe="$repo_root/engine/target/$profile/qu.exe"
[ -f "$exe" ] || exe="$repo_root/engine/target/$profile/qu"
if [ ! -f "$exe" ]; then
  echo "BUILD REPORTED SUCCESS BUT PRODUCED NO ARTIFACT at $exe" >&2
  exit 1
fi

# Verify by BEHAVIOUR, not by timestamp. A stale binary answers fluently:
# a newer mtime proves a file was written, not that the build that wrote
# it was the one you asked for. Running it is the only check that reaches
# the actual subject.
probe=$("$exe" eval 'print(6*7)' 2>&1 || true)
if [ "$(printf '%s' "$probe" | tr -d '\r\n')" != "42" ]; then
  echo "ARTIFACT EXISTS BUT DOES NOT RUN CORRECTLY" >&2
  echo "  $exe eval 'print(6*7)' -> $probe" >&2
  exit 1
fi

echo "ok: $exe"
"$exe" version
