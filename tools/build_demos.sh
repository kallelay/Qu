#!/usr/bin/env bash
# Build the Demos section: run every catalogue program, then render a page
# for each one carrying its description, its output, its figure and its
# whole source.
#
# Usage:  bash tools/build_demos.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"

WORK=".demo-run"
mkdir -p "$WORK" website/demos
bash tools/run_demos.sh "$WORK"
QU_DEMO_OUTPUTS="$WORK" "$QU" run tools/gen_demos.qu
