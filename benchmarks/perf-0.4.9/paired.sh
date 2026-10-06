#!/usr/bin/env bash
# Paired before/after: R alternating runs of two Qu binaries (order flips each
# round), same box, same minute. Writes results/<tag>/ in the layout agg.qu
# reads, with the "new" binary in the qu_ files and the "old" one in py_ and
# py1_ (so agg.qu's "Qu/Py" column is new/old: < 1 means the change is faster).
#
#   benchmarks/perf-0.4.9/paired.sh old.exe new.exe [reps=5] [tag=paired]
set -u
here="$(cd "$(dirname "$0")" && pwd)"
old="$1"; new="$2"; reps="${3:-5}"; tag="${4:-paired}"
out="$here/results/$tag"
mkdir -p "$out"
one() { { "$1" run "$here/bench.qu" 2>&1; "$1" run "$here/bench_sparse.qu" 2>&1; } | grep -E "^(BENCH|qu:)"; }
for k in $(seq 1 "$reps"); do
  if [ $((k % 2)) -eq 1 ]; then
    one "$new" > "$out/qu_$k.txt"; one "$old" > "$out/py_$k.txt"
  else
    one "$old" > "$out/py_$k.txt"; one "$new" > "$out/qu_$k.txt"
  fi
  cp "$out/py_$k.txt" "$out/py1_$k.txt"
  echo "pair $k done"
done
echo "$reps" > "$out/reps.txt"
echo "columns: Qu = NEW binary, Py/Py1 = OLD binary (Qu/Py = new/old)" > "$out/README.txt"
"$new" run "$here/agg.qu" -- "$out" 2>&1 | tee "$out/table.md"
