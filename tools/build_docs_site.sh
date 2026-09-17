#!/usr/bin/env bash
# Build the documentation pages of the website.
#
# Three steps, because the second needs the first's output and the third
# needs the second's:
#
#   1. render, writing each ```qu block out as a runnable file
#   2. run them, capturing what each printed
#   3. render again, folding the output in under the code
#
# The result is a site whose examples are known to run and whose printed
# results were produced by the engine in this working tree, not typed in.
#
# Usage:  bash tools/build_docs_site.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"

# Two sessions share this tree and both run this script; it has one work
# directory and one sandbox program file, so concurrent runs overwrite each
# other's code and examples fail on statements from another chapter. The
# failure is silent -- the example is reported as one that could not run and
# its output and figure vanish from the page.
LOCK=".doc-run.lock"
waited=0
while ! mkdir "$LOCK" 2>/dev/null; do
    waited=$((waited + 1))
    if [ "$waited" -gt 900 ]; then
        echo "another documentation build has held the lock for 15 minutes -- giving up"
        exit 1
    fi
    sleep 1
done
trap 'rmdir "$LOCK" 2>/dev/null || true' EXIT

WORK=".doc-run"
mkdir -p "$WORK"
rm -f "$WORK"/*.body "$WORK"/*.skip

QU_DOC_EXTRACT="$WORK" "$QU" run tools/gen_docs_site.qu
bash tools/run_doc_outputs.sh "$WORK"
QU_DOC_EXTRACT="$WORK" QU_DOC_OUTPUTS="$WORK" "$QU" run tools/gen_docs_site.qu

# --- record what built this site ------------------------------------------
#
# On 2026-09-09 the live root sat 47 commits behind master while docs were
# regenerated there and published. The pages looked current -- fresh
# timestamps, fresh numbers -- because the chapter source had moved on and
# the generators had not, so output came out newer AND wrong, quietly
# reverting fixes merged in between. Three sessions reasoned about the
# resulting sentence before anyone thought to compare the two trees.
#
# This turns that from an investigation into a `cat`.
#
# ONE file, deliberately, rather than a comment in each page. A per-page
# commit stamp would change all 950 pages on every build even when nothing
# in them changed -- and it was precisely the unchanged pages holding still
# that made today's real defect visible: 6 genuine changes among 705 files
# touched, five of them silently DELETED examples. Stamping every page
# would have hidden exactly the signal it was meant to provide.
{
  printf 'commit   %s
' "$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  printf 'branch   %s
' "$(git symbolic-ref --quiet --short HEAD 2>/dev/null || echo '(detached)')"
  printf 'behind   %s commit(s) behind master
' "$(git rev-list --count HEAD..master 2>/dev/null || echo '?')"
  n=$(git status --porcelain 2>/dev/null | wc -l)
  if [ "${n:-0}" -gt 0 ]; then
    # A page built from a dirty tree is unreproducible even when the commit
    # is right, because the files it was generated from are not that commit.
    printf 'tree     DIRTY (%s modified) -- not reproducible from the commit alone
' "$n"
  else
    printf 'tree     clean
'
  fi
  printf 'built    %s
' "$(date -Iseconds)"
} > website/.buildinfo
echo "stamped     website/.buildinfo ($(git rev-parse --short HEAD 2>/dev/null || echo unknown))"
