#!/usr/bin/env bash
# Run every ```qu example in the documentation.
#
# The reference must describe what the engine does. An example that does
# not run is either drift or a promise the engine does not keep, and both
# have to be settled before a release rather than discovered by a reader.
#
# Usage:  bash tools/check_docs.sh [work-dir]
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"
WORK="${1:-$(mktemp -d)}"
mkdir -p "$WORK"
rm -f "$WORK"/*.qu "$WORK"/*.skip 2>/dev/null

if ! QU_DOCEX_OUT="$WORK" "$QU" run tools/extract_doc_examples.qu; then
  echo "check_docs: the extractor failed -- no examples were written, so" >&2
  echo "there is nothing to report. This is not a clean run." >&2
  exit 1
fi

# Blocks are cumulative, so the first failure in a document poisons every
# block after it. Reporting only that first failure answers the question a
# reader has -- how far into this page do I get before it breaks -- and
# reporting all of them would just count the same defect once per
# remaining paragraph.
clean=0; broken=0
: > "$WORK/failures.txt"
prev_doc=""
for f in $(ls "$WORK"/*.qu 2>/dev/null | sort -V); do
  doc=$(basename "$f" | sed -E 's/_[0-9]+\.qu$//')
  if [ "$doc" != "$prev_doc" ]; then
    prev_doc="$doc"
    doc_broken=0
  fi
  [ "$doc_broken" = 1 ] && continue
  if ! out=$(cd "$WORK" && timeout 30 "$OLDPWD/$QU" run "$(basename "$f")" 2>&1); then
    doc_broken=1
    broken=$((broken+1))
    n=$(basename "$f" | grep -oE '[0-9]+\.qu$' | tr -d '.qu')
    printf '%s	block %s	%s
' "$doc" "$n"       "$(printf '%s' "$out" | grep -oE '^qu: .*' | head -1)" >> "$WORK/failures.txt"
  fi
done
docs=$(ls "$WORK"/*.qu 2>/dev/null | sed -E 's/_[0-9]+\.qu$//' | sort -u | wc -l)

# "Nothing found" and "nothing looked at" must not print the same thing.
# With no examples extracted this reported "0 run clean, 0 break partway
# (of 0)" and exited 0, which reads as a pass -- the same shape as the
# receiver-check tool that silently stopped reporting the one signature
# it existed to find (board2, 2026-09-09). A checker's silence must
# never be readable as a result.
if [ "$docs" -eq 0 ]; then
  echo "check_docs: no examples were extracted from the documentation." >&2
  echo "That is a broken run, not a clean one -- the book has hundreds." >&2
  exit 1
fi
clean=$((docs - broken))
echo "documents: $clean run clean, $broken break partway (of $docs)"
if [ "$broken" -gt 0 ]; then
  echo ""
  column -t -s "$(printf '	')" "$WORK/failures.txt" 2>/dev/null || cat "$WORK/failures.txt"
fi
