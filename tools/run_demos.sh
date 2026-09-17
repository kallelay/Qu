#!/usr/bin/env bash
# Run every catalogue program and capture what it printed and what it drew,
# so each demo page can show its real output and its real figure.
#
# The same reason the documentation examples are run: a demo page that
# quotes numbers somebody typed is a page that drifts. These are produced by
# the engine in this working tree, every build.
#
# Scripts that cannot run here -- the ones that want a serial port, a GPU, a
# network peer, or a file only the author has -- simply produce no output,
# and their page shows the program without one. That is honest, and it is
# better than excluding them: the source is still the best thing on the page.
#
# Usage:  bash tools/run_demos.sh <work-dir>
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
ROOT="$PWD"
QU="$ROOT/engine/target/release/qu.exe"
[ -x "$QU" ] || QU="$ROOT/engine/target/release/qu"
[ -x "$QU" ] || { echo "no engine build at engine/target/release/qu"; exit 1; }

WORK="${1:-.demo-run}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
rm -f "$WORK"/*.out "$WORK"/*.svg

# Each script runs in its own directory, so the `savefig`/`write_csv` calls
# in them land there instead of silting up the repository.
SAND="$WORK/sandbox"
mkdir -p "$SAND"; rm -f "$SAND"/* 2>/dev/null

MAXLINES=30
ok=0; failed=0; figs=0

for f in "$ROOT"/catalog/*.qu; do
  stem="$(basename "$f" .qu)"
  rm -f "$SAND"/fig*.svg
  # The animation and interactive-export demos render dozens of figures
  # each, which is the point of them; 90 seconds is right for a script and
  # far too short for those.
  limit=90
  case "$stem" in
    qu_starry_night|qu_sea_waves|qu_signal_explorer|qu_compressed_sensing) limit=900 ;;
  esac
  if ! out=$(cd "$SAND" && timeout "$limit" "$QU" run "$f" --max-time $((limit - 10)) --emit-figure fig.svg 2>&1); then
    failed=$((failed + 1))
    continue
  fi
  ok=$((ok + 1))

  # The `· plot: figure 1 (500 points)` lines are the engine reporting that
  # a figure was built. The page shows the figure itself, so the report is
  # noise there.
  body=$(printf '%s\n' "$out" | grep -v '^· ' | sed -e '/./,$!d')
  if [ -n "$body" ]; then
    total=$(printf '%s\n' "$body" | wc -l)
    if [ "$total" -gt "$MAXLINES" ]; then
      body="$(printf '%s\n' "$body" | head -n "$MAXLINES")
    ... $((total - MAXLINES)) more lines"
    fi
    printf '%s' "$body" > "$WORK/$stem.out"
  fi

  # The first figure only. A script that opens twelve is demonstrating a
  # loop, and twelve figures is not a page anyone reads.
  #
  # A figure over the cap is not inlined -- a four-panel plot of twenty-four
  # thousand samples is five megabytes of SVG, and a page nobody can load is
  # worse than a page without a picture. The size is recorded instead, and
  # the page says so, which is itself worth knowing about a demo.
  if [ -f "$SAND/fig.svg" ]; then
    size=$(wc -c < "$SAND/fig.svg")
    if [ "$size" -lt 900000 ]; then
      sed -e 's|<metadata id="qu-figure">.*</metadata>||' "$SAND/fig.svg" > "$WORK/$stem.svg"
      figs=$((figs + 1))
    else
      printf '%s' "$((size / 1024 / 1024))" > "$WORK/$stem.bigfig"
    fi
  fi
done

echo "demos: $ok ran, $failed could not, $figs drew a figure"
