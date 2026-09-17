#!/usr/bin/env bash
# Copy the freshly-built engine qu.exe into Qu Studio's Tauri sidecar
# location, and stamp it with the commit it came from.
#
#   bash tools/refresh_studio_sidecar.sh
#
# Why this exists: the sidecar binary (qu-studio-tauri/src-tauri/
# binaries/qu-x86_64-pc-windows-msvc.exe) is NOT git-tracked, so it does
# not travel with commits the way source does. A worktree can sit on a
# perfectly current commit while its sidecar is hours or days stale, and
# nothing about `git status` or `git log` says so -- the only way to
# notice was to actually run a repro through the specific binary file,
# which is what caught this live on 2026-09-10 (var(axis=0) still wrong
# through a stale sidecar, in a worktree whose source was fully current).
#
# The fix here is not "remember to refresh it" -- that's the same
# vigilance-instead-of-structure mistake this whole night has been
# about. It's a version stamp written at copy time, so "is this sidecar
# current" is a `cat` away instead of something you have to test your
# way into discovering.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1

ENGINE_BIN="${CARGO_TARGET_DIR:-engine/target}/release/qu.exe"
SIDECAR_DIR="qu-studio-tauri/src-tauri/binaries"
SIDECAR_BIN="$SIDECAR_DIR/qu-x86_64-pc-windows-msvc.exe"
STAMP="$SIDECAR_DIR/qu.version"

if [ ! -f "$ENGINE_BIN" ]; then
  echo "REFUSING: $ENGINE_BIN does not exist -- build the engine first" >&2
  echo "  (cargo build -p qu-cli --release --manifest-path engine/Cargo.toml)" >&2
  exit 1
fi

mkdir -p "$SIDECAR_DIR"
cp "$ENGINE_BIN" "$SIDECAR_BIN"

commit=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
dirty=$(git status --porcelain 2>/dev/null | wc -l)
stamp_state="clean"
[ "${dirty:-0}" -gt 0 ] && stamp_state="DIRTY ($dirty modified file(s))"

{
  echo "commit      $commit ($stamp_state)"
  echo "copied      $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
  echo "source      $ENGINE_BIN"
} > "$STAMP"

echo "refreshed   $SIDECAR_BIN"
echo "stamped     $STAMP"
cat "$STAMP"
