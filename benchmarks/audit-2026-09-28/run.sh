#!/usr/bin/env bash
# Perf-audit runner (2026-09-28): runs every *.qu script in this directory
# with `qu run --profile`, prints each script's BENCH/ACC lines and its
# peak RSS (read by the profiler from /proc).
#
#   benchmarks/audit-2026-09-28/run.sh [path/to/qu] [script.qu ...]
#
# Default binary: engine/target/release/qu relative to the repo root.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
qu="${1:-$root/engine/target/release/qu}"
shift || true
scripts=("$@")
if [ ${#scripts[@]} -eq 0 ]; then
  scripts=(interp.qu vector.qu linalg.qu signal.qu table_text.qu stats.qu accuracy.qu)
fi
for s in "${scripts[@]}"; do
  echo "=== $s"
  prof="$(mktemp)"
  "$qu" run "$here/$s" --profile --profile-output "$prof" 2>&1 | grep -E '^(BENCH|ACC)|error' || true
  grep -iE 'peak' "$prof" | head -2 | sed 's/^/    /'
  rm -f "$prof"
done
