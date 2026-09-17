#!/usr/bin/env bash
# A page per builtin, at website/fn/<name>.html.
#
# Qu has no directory listing, so the manifest of already-run example
# blocks is built here and read there. The blocks themselves come from
# tools/run_doc_outputs.sh, which the documentation build has already run --
# this joins them to the builtins each one calls.
#
# This working tree is shared by many concurrent sessions, so this script
# can and does get run twice at once. The old version wrote straight into
# website/fn after an `rm -f *.html`, which left the directory empty (and
# every already-open function page a 404) for however long the regenerate
# took, and for any concurrent run entirely. This version generates into a
# scratch directory and copies it over the live one under a lock that
# serialises concurrent runs of this same script -- a reader never sees
# website/fn missing or empty.
#
# Usage:  bash tools/build_function_pages.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"

[ -d .doc-run ] || { echo "no .doc-run: run bash tools/build_docs_site.sh first"; exit 1; }

LOCK="website/.fn.lock"
waited=0
while ! mkdir "$LOCK" 2>/dev/null; do
    waited=$((waited + 1))
    if [ "$waited" -gt 150 ]; then
        echo "another build_function_pages.sh has held the lock for 30s -- giving up"
        exit 1
    fi
    sleep 0.2
done
trap 'rmdir "$LOCK" 2>/dev/null || true' EXIT

TMP="website/.fn.new"
# Dropbox holds directory handles here, so removing the scratch
# directory can fail. Clearing its contents works and is enough.
mkdir -p "$TMP"
rm -f "$TMP"/*.html 2>/dev/null || true

ls .doc-run/*.body 2>/dev/null | sed 's|.*/||; s|\.body$||' | sort -V > .doc-run/manifest.txt

QU_FN_OUT_DIR="$TMP" "$QU" run tools/gen_function_pages.qu

# Copy over the live directory rather than renaming it into place. A
# rename fails on this filesystem while Dropbox holds the directory, and a
# half-completed rename swap leaves website/fn missing rather than stale.
mkdir -p website/fn
cp -f "$TMP"/*.html website/fn/

# Drop pages for names the engine no longer has.
for f in website/fn/*.html; do
  [ -f "$TMP/$(basename "$f")" ] || rm -f "$f"
done

rm -rf "$TMP" 2>/dev/null || true
echo "published $(ls website/fn/*.html | wc -l) pages"
