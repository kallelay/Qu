#!/usr/bin/env bash
# Build every generated part of the website, in order.
#
#   docs   — the documentation pages, with each example's output and figure
#   demos  — a page per catalogue program
#   refs   — every builtin, searchable, with its signature
#   changelog — the changelog as a page
#
# Usage:  bash tools/build_site.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"

# The specification is written ahead of the engine. Cut it down to the
# chapters that actually run before anything renders it as documentation.
"$QU" run tools/spec_coverage.qu > /dev/null
"$QU" run tools/strip_unbuilt_spec.qu

# The builtin index is an INPUT to the docs build, not an output of it:
# it writes book/src/stdlib/builtin-index.md, which build_docs_site.sh
# then renders. So it runs first.
#
# It was in no build script at all. Its own header says "a reference
# written by hand drifts the moment a builtin is added; this one is
# regenerated" -- and it was, whenever somebody remembered. Running it by
# hand and finding no diff is not reassurance.
#
# The exposure, measured on --first-parent (a plain `git log -- <path>`
# walks BOTH sides of every merge, so consecutive entries are not parent
# and child and diffing between them manufactures changes that never
# happened -- it reported more names added than the language has):
#
#   25 of the last 120 commits touching qu-interp/src/lib.rs changed
#   BUILTIN_NAMES. About one interpreter commit in five could stale this
#   page, with nothing in the build noticing.
"$QU" run tools/gen_builtin_index.qu

bash tools/build_docs_site.sh
bash tools/build_demos.sh
bash tools/build_function_pages.sh
"$QU" run tools/gen_refs.qu
"$QU" run tools/gen_changelog_page.qu
