#!/usr/bin/env bash
# Paired before/after of qr / svd / eig: R alternating runs of the OLD Qu
# binary (built from master before the blocked decompositions), the NEW one and
# single-thread NumPy (OPENBLAS_NUM_THREADS=1), the order rotating each round so
# box load cancels. Writes results/<tag>/{old,new,np}_<k>.txt and a table.
#
#   benchmarks/perf-0.4.9/paired_linalg.sh old.exe new.exe [reps=5] [tag=linalg]
set -u
here="$(cd "$(dirname "$0")" && pwd)"
old="$1"; new="$2"; reps="${3:-5}"; tag="${4:-linalg}"
out="$here/results/$tag"
mkdir -p "$out"
{
  echo "date: $(date -u +%FT%TZ)"
  echo "old: $old"; echo "new: $new"
  python -c "import numpy;print('numpy',numpy.__version__)"
  echo "cores: $(nproc)"
} > "$out/env.txt"
runq() { "$1" run "$here/bench_linalg.qu" 2>&1 | grep -E "^(BENCH|qu:|error|Error)" > "$2"; }
runp() { OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 python "$here/bench_linalg.py" 2>&1 | grep -E '^(BENCH|Traceback|Error)' > "$1"; }
for k in $(seq 1 "$reps"); do
  case $((k % 3)) in
    1) runq "$new" "$out/new_$k.txt"; runq "$old" "$out/old_$k.txt"; runp "$out/np_$k.txt" ;;
    2) runq "$old" "$out/old_$k.txt"; runp "$out/np_$k.txt"; runq "$new" "$out/new_$k.txt" ;;
    0) runp "$out/np_$k.txt"; runq "$new" "$out/new_$k.txt"; runq "$old" "$out/old_$k.txt" ;;
  esac
  echo "round $k done"
done
echo "$reps" > "$out/reps.txt"
"$new" run "$here/agg_linalg.qu" -- "$out" 2>&1 | tee "$out/table.md"
