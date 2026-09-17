#!/usr/bin/env bash
# Refuse to commit test work that carries someone else's hunks.
#
# WHY THIS EXISTS
# Several sessions edit `engine/crates/qu-interp/src/lib.rs` at once -- it is
# 68k lines and every lane adds builtins to it. Naming an explicit path on
# `git commit` protects you from other FILES and does nothing here: the
# collision is inside one file. Three commits have shipped another lane's
# half-finished work this way (8fa6d57d, a20968ec, 6e224f0a), two of them
# referencing functions defined nowhere on master, so the merge does not
# build.
#
# WHY A SCRIPT RATHER THAN CARE
# The last one happened while I was running the check by hand. It printed
# `foreign hunks: 3`; I read past it and committed. **A check that cannot
# stop the action is decoration** -- the same defect as `|| true` in
# make_public.sh, with a human standing in for the shell default. So this
# EXITS NON-ZERO, and it prints what it found rather than how much: three
# function names you do not recognise are harder to rationalise than "3".
#
# WHAT IT CHECKS
# Test-writing work adds lines inside `mod tests` and nowhere else. Any
# changed line above that module belongs to somebody else. That is a narrow
# rule, and narrow is the point: it is exactly true for this kind of work,
# so it never needs a judgement call.
#
# WHEN IT ABORTS ON YOUR OWN WORK
# It guards TEST-ONLY commits. A legitimate change to engine code trips it,
# and that is correct behaviour, not a false positive -- commit the engine
# change separately and this has nothing to say about it. **Do not loosen
# the rule to make the abort go away.** The narrowness is what makes it
# safe to run unattended: a guard that has to decide something will
# eventually decide wrong, and then it is narration again, which is the
# failure it was written to retire.
#
#   bash tools/guard_test_commit.sh [path]     # default: qu-interp/src/lib.rs
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1

FILE="${1:-engine/crates/qu-interp/src/lib.rs}"

start=$(grep -n '^#\[cfg(test)\]' "$FILE" | tail -1 | cut -d: -f1)
if [ -z "${start:-}" ]; then
  echo "guard: no '#[cfg(test)]' in $FILE -- cannot tell test code from engine code" >&2
  exit 1
fi

# Hunk headers give the new-file line each hunk starts at.
outside=0
: > /tmp/guard_hits.txt
while IFS= read -r line; do
  case "$line" in
    @@*)
      # @@ -old,+new @@ -- take the new-side start line.
      at=$(printf '%s' "$line" | sed -E 's/^@@ -[0-9,]+ \+([0-9]+).*/\1/')
      ;;
    +*)
      [ "${at:-0}" -lt "$start" ] && { outside=$((outside + 1)); printf '%s\n' "$line" >> /tmp/guard_hits.txt; }
      ;;
  esac
done < <(git diff "$FILE")

if [ "$outside" -eq 0 ]; then
  echo "guard: clean -- every added line is inside the test module (line $start+)"
  exit 0
fi

echo "guard: ABORT -- $outside added line(s) OUTSIDE the test module in $FILE" >&2
echo "" >&2
sed -E 's/^\+/  /' /tmp/guard_hits.txt | head -12 >&2
[ "$outside" -gt 12 ] && echo "  ... and $((outside - 12)) more" >&2
echo "" >&2
echo "These are another lane's in-flight edits to the same file. Committing" >&2
echo "them ships half of someone else's feature under your message, and if" >&2
echo "the other half is uncommitted the merge will not build." >&2
echo "Stage your own hunks instead:  git add -p $FILE" >&2
exit 1
