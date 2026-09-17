#!/usr/bin/env bash
# Run the matty_suite cross-language benchmark inside a genuinely quiet
# machine, and refuse to produce numbers otherwise.
#
# WHY THIS EXISTS
# This suite's own README warns every kernel moves 2-3x between a quiet
# machine and a loaded one -- for Qu and MATLAB roughly equally. Numbers
# taken from a busy machine are noise wearing the authority of a
# measurement, which is worse than no numbers, because someone will quote
# them.
#
# So the checks are not advisory. The script writes nothing and exits
# non-zero unless the machine was quiet BEFORE, DURING and AFTER the run.
#
# THE DURING CHECK IS THE ONE THAT MATTERS, and it is why this samples
# process names and not just CPU load. A freshly spawned `rustc` reads
# near zero for its first moments, so the run most likely to be silently
# contaminated is precisely the one where a build lands just after the
# start check passed. A gate that only samples load would wave that
# through: the check ran, the check passed, and the thing it guarded
# against came in through a door it was not watching. (Found by review
# on 2026-09-09, before it ever produced a wrong number -- which is the
# only good time to find it.)
#
# Written in bash rather than Qu for the same reason tools/backup.sh is:
# Qu has no shell()/subprocess builtin yet, so it cannot drive `matlab`
# or read a process table. When it grows one, this is a candidate to port.
#
# Usage:
#   run_quiet_window.sh [out.md]              run once, refuse if busy
#   run_quiet_window.sh [out.md] --wait 30    poll up to 30 min for quiet

set -uo pipefail

OUT="${1:-matty_suite_quiet_run.md}"
WAIT_MIN=0
[ "${2:-}" = "--wait" ] && WAIT_MIN="${3:-30}"

BUSY_PCT=15           # refuse to start at or above this
DRIFT_PCT=25          # unpublishable if it ends at or above this
QU_SWARM=3            # more than this many qu.exe = a docs build, not us
MATLAB="/c/Program Files/MATLAB/R2025b/bin/matlab.exe"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
say() { printf '%s\n' "$*"; }

ps_query() { powershell -NoProfile -Command "$1" 2>/dev/null | tr -d '\r'; }

# Median of three: one spike must not veto a quiet machine, one lull must
# not excuse a busy one.
cpu_load() {
  ps_query '$s = 1..3 | ForEach-Object {
      (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
      Start-Sleep -Milliseconds 700
    }
    ($s | Sort-Object)[1]'
}

# Builds, BY NAME. `qu.exe` is counted separately and with a threshold:
# the docs build runs 700+ examples serially through it for ~10 minutes
# and nobody calls that a build, but it will wreck a measurement.
builds_running() {
  ps_query "(Get-Process | Where-Object { \$_.ProcessName -match '^(cargo|rustc|link)$' }).Count"
}
qu_running() {
  ps_query "(Get-Process | Where-Object { \$_.ProcessName -eq 'qu' }).Count"
}

# A refusal that names the culprit is a diagnosis; one that only gives a
# percentage sends the reader off to find out who is to blame.
top_consumer() {
  ps_query "(Get-Process | Sort-Object CPU -Descending | Select-Object -First 1).ProcessName"
}

# Returns 0 when the machine is fit to measure; prints why not otherwise.
gate_reason() {
  local load builds qus
  load="$(cpu_load)"; builds="$(builds_running)"; qus="$(qu_running)"
  GATE_LOAD="$load"
  if [ -z "$load" ]; then echo "could not read CPU load"; return 1; fi
  if [ "${builds:-1}" -ne 0 ]; then echo "${builds} build process(es) running (cargo/rustc/link)"; return 1; fi
  if [ "${qus:-0}" -gt "$QU_SWARM" ]; then echo "${qus} qu.exe processes -- looks like a docs/example run"; return 1; fi
  if [ "$load" -ge "$BUSY_PCT" ]; then echo "${load}% CPU (top: $(top_consumer))"; return 1; fi
  return 0
}

# --- 1. gate FIRST, before any setup work ---------------------------------
say "== quiet-window gate =="
deadline=$(( $(date +%s) + WAIT_MIN * 60 ))
while true; do
  if reason="$(gate_reason)"; then
    say "  quiet: ${GATE_LOAD}% CPU, no builds. Proceeding."
    break
  fi
  say "  not quiet: $reason"
  if [ "$(date +%s)" -ge "$deadline" ]; then
    say ""
    say "REFUSING TO MEASURE. No numbers written."
    say "A window that proves the machine is not quiet is a successful window."
    exit 1
  fi
  sleep 30
done

# --- 2. provenance --------------------------------------------------------
QU_EXE="${QU_EXE:-$here/../../engine/target/release/qu.exe}"
qu_hash="$(cd "$here" && git rev-parse --short HEAD 2>/dev/null || echo unknown)"
matlab_ver="$("$MATLAB" -batch "fprintf('%s', version)" 2>/dev/null | tr -d '\r')"
before="$GATE_LOAD"

# --- 3. watch DURING, by name, not only by load ---------------------------
intruder=""
watch_flag="$(mktemp)"
( while :; do
    b="$(builds_running)"; q="$(qu_running)"
    if [ "${b:-0}" -ne 0 ]; then echo "build started mid-run (${b} cargo/rustc/link)" > "$watch_flag"; fi
    if [ "${q:-0}" -gt "$QU_SWARM" ]; then echo "qu.exe swarm mid-run (${q})" > "$watch_flag"; fi
    sleep 2
  done ) & watcher=$!

say ""
say "== running (back-to-back, same machine state) =="
qu_out="$("$QU_EXE" run "$here/bench.qu" 2>&1)"
ml_out="$("$MATLAB" -batch "run('$here/bench.m')" 2>&1)"

kill "$watcher" 2>/dev/null; wait "$watcher" 2>/dev/null
[ -s "$watch_flag" ] && intruder="$(cat "$watch_flag")"
rm -f "$watch_flag"

# --- 4. verdict -----------------------------------------------------------
after="$(cpu_load)"
verdict="PUBLISHABLE"
if [ -n "$intruder" ]; then
  verdict="UNPUBLISHABLE -- $intruder"
elif [ -z "$after" ] || [ "$after" -ge "$DRIFT_PCT" ]; then
  verdict="UNPUBLISHABLE -- machine became busy mid-run (${before}% -> ${after}%)"
fi

{
  printf '# matty_suite quiet-window run\n\n'
  printf -- '- when         : %s\n' "$(date -Iseconds)"
  printf -- '- qu build     : %s (release)\n' "$qu_hash"
  printf -- '- MATLAB       : %s\n' "$matlab_ver"
  printf -- '- resource tag : on="cpu" -- local CPU, the tag a MATLAB user gets\n'
  printf -- '- cpu load     : %s%% before, %s%% after\n' "$before" "$after"
  printf -- '- verdict      : %s\n\n' "$verdict"
  printf '## Qu (on="cpu")\n\n```\n%s\n```\n\n' "$qu_out"
  printf '## MATLAB %s\n\n```\n%s\n```\n' "$matlab_ver" "$ml_out"
} > "$OUT"

say ""
say "  verdict: $verdict"
say "  written: $OUT"
[ "$verdict" = "PUBLISHABLE" ]
