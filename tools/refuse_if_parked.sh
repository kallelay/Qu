#!/usr/bin/env bash
# Refuse to build Qu Studio from a tree that has been marked parked/stale.
#
#   bash tools/refuse_if_parked.sh
#
# Why this exists: D:\QuWorkspace's top-level checkout was parked and
# renamed to PARKED-STALE-DO-NOT-BUILD on 2026-09-10, with a .PARKED-
# STALE-DO-NOT-BUILD marker file and a warning in its own name -- and
# was still perfectly buildable. A shipped Studio build from that tree
# silently carries whatever engine was current when it was parked,
# with no error and no stamp to say so. A name that advises is not a
# guard; this makes the same fact a refused build instead of a
# discoverable one -- the same reasoning as the qu.version stamp
# refresh_studio_sidecar.sh writes, applied one step earlier.
#
# Wired into tauri.conf.json's beforeBuildCommand, so it runs before
# every `tauri build` / `npm run tauri build` from this app.

set -euo pipefail

# Deliberately NOT `cd "$(dirname "${BASH_SOURCE[0]}")/.."` -- that would
# check the SCRIPT's own source location, not the tree the build is
# actually running in. This must check wherever the caller's working
# directory's repo root is, since a worktree that later gets parked
# already has this script; a build there needs the check to run against
# the worktree it lives in, not against wherever the script came from.
repo_root=$(git rev-parse --show-toplevel 2>/dev/null || echo "")
if [ -z "$repo_root" ]; then
  echo "REFUSING to build: not inside a git checkout, cannot verify freshness." >&2
  exit 1
fi

if [ -f "$repo_root/.PARKED-STALE-DO-NOT-BUILD" ]; then
  echo "REFUSING to build: this checkout is marked parked/stale." >&2
  echo "  marker: $repo_root/.PARKED-STALE-DO-NOT-BUILD" >&2
  echo "  $(head -1 "$repo_root/.PARKED-STALE-DO-NOT-BUILD" 2>/dev/null || true)" >&2
  echo "  Build from your own worktree instead -- see the marker file" >&2
  echo "  for what it is and where the current record is." >&2
  exit 1
fi

branch=$(git -C "$repo_root" branch --show-current 2>/dev/null || echo "")
case "$branch" in
  *PARKED* | *STALE* | *DO-NOT-BUILD*)
    echo "REFUSING to build: current branch '$branch' names itself" >&2
    echo "  parked/stale/do-not-build. Switch to your own worktree." >&2
    exit 1
    ;;
esac

exit 0
