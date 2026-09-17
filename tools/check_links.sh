#!/usr/bin/env bash
# Every local link on the site, against what git actually tracks.
#
# git supplies the two lists because Qu cannot run it: the point is to catch
# a page that exists in this working copy and nowhere else, which the
# filesystem alone cannot tell you.
#
# Usage:  bash tools/check_links.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
QU="engine/target/release/qu.exe"
[ -x "$QU" ] || QU="engine/target/release/qu"

mkdir -p .link-check
git ls-files website > .link-check/tracked.txt
# Template fragments are spliced into pages elsewhere, so their relative
# links resolve from where they land, not from where they sit.
git ls-files 'website/**/*.html' 'website/*.html' \
  | grep -v '^website/assets/' > .link-check/pages.txt

"$QU" run tools/check_links.qu
