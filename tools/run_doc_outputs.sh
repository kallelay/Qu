#!/usr/bin/env bash
# Run the documentation's examples and capture what each one printed, so
# the website can show the output under the code that produced it.
#
# The point is that the outputs on the site are not typed by hand. A number
# in the prose can drift from the engine and nobody notices for a year; a
# number produced by running the block cannot, because the build reruns it.
#
# Blocks are cumulative -- a tutorial that says "now plot it" defined `x`
# three paragraphs up -- so block n runs with the earlier blocks of its
# page in front of it. Only the blocks that RAN join that context. A page
# is full of fragments that cannot run on their own (they open a file the
# reader has, or name a function the prose is about to define), and one of
# those must not blank every example after it.
#
# The output belonging to block n is the difference between what the whole
# accumulation printed and what it printed before block n was added. That
# difference is only taken when the earlier output is a literal prefix of
# the later one: a block whose output moves between runs -- an unseeded
# random draw, a timing -- is exactly the block that must not be pinned to
# the page as though it were fixed.
#
# WHY THIS IS SLOW, AND WHICH PART OF THE SLOWNESS IS THE DESIGN. It runs
# 700+ example blocks and takes roughly ten minutes, and the first instinct
# on reading that is to parallelise it. Two different things are going on,
# and only one of them is fixed:
#
#   Within a page it is serial BY CONSTRUCTION, and worse than linear.
#   Block n runs the whole accumulated prefix, so a page of k blocks runs
#   1 + 2 + ... + k blocks' worth of work. That is where the ten minutes
#   is, and it cannot be split: block n's output is DEFINED as the
#   difference against what the prefix printed without it. Remove the
#   re-running and the outputs stop being produced by the engine, which is
#   the entire property that makes the pages trustworthy rather than
#   plausible.
#
#   Across pages it is serial only INCIDENTALLY. Pages reset the
#   accumulation and are independent. What blocks it is state, not logic:
#   every block writes `run.qu` and `fig*.svg` into ONE shared sandbox, so
#   concurrent workers would overwrite each other's figures and their
#   diffs. Give each worker its own sandbox directory and this axis opens
#   -- BUT NOT WITHOUT ESTABLISHING THE PRECONDITION BELOW FIRST.
#
# THE PRECONDITION, which is the part that can bite silently. Splitting the
# sandbox is only safe if no block depends on a file another PAGE left
# behind. Note what this loop does and does not do: the sandbox is emptied
# ONCE, at line 40, and never again. `rm -f "$SAND"/fig*.svg` per block
# clears figures, not data. So every file an example writes stays visible
# to every later page in the same build -- pages are isolated in their code
# accumulation and NOT in their filesystem.
#
# Reading the chapters suggests no file is meant to cross a chapter
# boundary: 72 distinct filenames appear in examples and only `data.csv`
# and `log.txt` appear in more than one chapter, neither as a genuine
# handoff. But intent in the markdown is not the same as behaviour, and it
# is behaviour that would break. A block can currently succeed on a file it
# never wrote, by accident of `sort -V` ordering, and nothing in the
# chapters would show it.
#
# That this is not hypothetical: `.doc-run/failures.txt` holds 195 dropped
# blocks across 12 pages, 14 of them for a file that was not there --
# including `file-io` block 1, which does `fopen("log.txt", "r")` 700 lines
# before the block that writes it. Write-then-read is already not holding.
# And a block that fails is DROPPED while the build still reports success,
# so the same silence covers a block that starts failing after the split.
#
# The experiment that settles it, and the one to run before splitting:
# clear the sandbox between pages, rebuild, and diff `failures.txt`. If it
# does not grow, nothing depended on the leak, and per-worker sandboxes are
# safe. Better still, make that clearing permanent and keep the count as an
# assertion -- then the precondition is enforced rather than re-established
# by hand every time somebody wonders about the ten minutes.
#
# So: parallelise across pages if the ten minutes ever has to come down.
# Do not touch the accumulation.
#
# Usage:  bash tools/run_doc_outputs.sh <work-dir>
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
ROOT="$PWD"
QU="$ROOT/engine/target/release/qu.exe"
[ -x "$QU" ] || QU="$ROOT/engine/target/release/qu"
[ -x "$QU" ] || { echo "no engine build at engine/target/release/qu"; exit 1; }

WORK="${1:-.doc-run}"
[ -d "$WORK" ] || { echo "no such work directory: $WORK"; exit 1; }
WORK="$(cd "$WORK" && pwd)"
rm -f "$WORK"/*.out "$WORK"/*.svg "$WORK"/run.qu "$WORK"/failures.txt
: > "$WORK/failures.txt"

# Examples that draw write files; they run in their own directory so the
# repository does not collect a hundred stray PDFs.
SAND="$WORK/sandbox"
mkdir -p "$SAND"; rm -f "$SAND"/* 2>/dev/null

MAXLINES=14
ok=0; failed=0; moving=0; empty=0; figs=0

page=""; prefix=""; prev=""; prevsvg=""
# -V so block 10 sorts after block 9 rather than after block 1.
for f in $(ls "$WORK"/*.body 2>/dev/null | sort -V); do
  base="$(basename "$f" .body)"
  p="${base%_*}"
  if [ "$p" != "$page" ]; then page="$p"; prefix=""; prev=""; prevsvg=""; fi

  # `$(cat)` eats trailing newlines, so one is put back: without it the
  # last line of a block welds onto the first line of the next.
  cand="$prefix$(cat "$f")
"
  printf '%s' "$cand" > "$SAND/run.qu"
  rm -f "$SAND"/fig*.svg
  # `--emit-figure` writes every figure the run produced, numbered from the
  # second. Since the run includes the page's earlier blocks, the figures
  # this block is responsible for are the ones past the count the prefix
  # alone produced -- the same difference the printed output is taken by.
  if ! out=$(cd "$SAND" && timeout 60 "$QU" run run.qu --emit-figure fig.svg 2>&1); then
    failed=$((failed + 1))
    printf '%s\tblock %s\t%s\n' "$p" "${base##*_}" \
      "$(printf '%s' "$out" | grep -oE '^qu: .*' | head -1)" >> "$WORK/failures.txt"
    continue
  fi
  ok=$((ok + 1))

  # Which figure belongs to this block is not a matter of counting them.
  # Consecutive blocks in a plotting chapter usually add to the SAME figure
  # -- a `plot`, then an `xlabel`, then a `legend`, each its own block --
  # so the file count never moves and only the first block would get one.
  # What identifies the block's contribution is that the figure CHANGED.
  # So: take the last figure the run produced, and keep it if it differs
  # from what the page's previous block left. That also shows the figure at
  # the point the reader has reached, which is what the prose is describing.
  last=$(ls "$SAND"/fig*.svg 2>/dev/null | sort -V | tail -1)
  if [ -n "$last" ] && [ "$(wc -c < "$last")" -lt 200000 ]; then
    # The metadata block carries a fixed id, and a page holds many figures;
    # duplicate ids in one document are invalid HTML.
    cur=$(sed -e 's|<metadata id="qu-figure">.*</metadata>||' "$last")
    if [ "$cur" != "$prevsvg" ]; then
      printf '%s' "$cur" > "$WORK/$base.svg"
      figs=$((figs + 1))
    fi
    prevsvg="$cur"
  fi

  case "$out" in
    "$prev"*) delta="${out#"$prev"}" ;;
    *) moving=$((moving + 1)); prefix="$cand"; prev="$out"; continue ;;
  esac
  prefix="$cand"; prev="$out"

  # Leading blank lines are an artefact of the split, not of the example.
  delta="$(printf '%s' "$delta" | sed -e '/./,$!d')"
  [ -n "$delta" ] || { empty=$((empty + 1)); continue; }

  total=$(printf '%s\n' "$delta" | wc -l)
  if [ "$total" -gt "$MAXLINES" ]; then
    delta="$(printf '%s\n' "$delta" | head -n "$MAXLINES")
    ... $((total - MAXLINES)) more lines"
  fi
  printf '%s' "$delta" > "$WORK/$base.out"
done

echo "examples: $ok ran, $failed could not, $empty printed nothing, $moving not reproducible"
echo "figures:  $figs drawn"
