#!/usr/bin/env bash
# Every file a chapter example opens, against what git actually tracks.
#
# The manifests are built here rather than in the Qu tool because Qu has no
# `shell()` builtin yet, so it cannot run `git` itself -- the same reason
# check_links.sh exists beside check_links.qu.
#
# `git ls-files` and not the filesystem: a fixture sitting untracked in one
# working copy looks present here and is absent in a clone, which is
# precisely the failure this checks for.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1

QU="${QU:-./engine/target/release/qu.exe}"
[ -x "$QU" ] || QU="./engine/target/release/qu"

mkdir -p .link-check
git ls-files > .link-check/tracked-all.txt

# The engine's own builtin list, so the tool can verify its vocabulary is
# real rather than trusting names someone typed from another language. The
# first version of that list had eleven names the engine does not have --
# `imread`, `imwrite`, `read_json` and other MATLAB/Python spellings -- so
# those entries matched nothing forever and images.md came back clean while
# carrying three missing .bmp files.
sed -n '/pub const BUILTIN_NAMES/,/^];/p' engine/crates/qu-interp/src/lib.rs |
  grep -oE '"[A-Za-z_][A-Za-z_0-9]*"' | tr -d '"' | sort -u \
  > .link-check/builtins.txt

# What this run actually read.
#
# `measurements.csv` was reported missing here while already fixed in
# another tree. A finding from a checkout is a statement about THAT
# checkout, and without this line a reader cannot tell a real defect from a
# stale copy. `website/.buildinfo` does the same for generated pages; a
# checker deserves it at least as much, since its whole output is claims
# about the tree it read.
head_sha=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
modified=$(git status --porcelain 2>/dev/null | wc -l)
if [ "${modified:-0}" -gt 0 ]; then
  tree_state="tree DIRTY ($modified modified)"
else
  tree_state="tree clean"
fi
printf 'commit %s, %s\n' "$head_sha" "$tree_state" > .link-check/head.txt

"$QU" run tools/check_example_files.qu
