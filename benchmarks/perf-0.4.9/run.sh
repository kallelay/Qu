#!/usr/bin/env bash
# perf-0.4.9 driver: R interleaved replicates of Qu, Python (default BLAS
# threads) and Python with OPENBLAS_NUM_THREADS=1 ("py1"; on a loaded box
# OpenBLAS's spinning worker threads can make LAPACK calls 100x slower, so a
# single-thread reference is reported alongside). Order alternates each round
# so drift cannot favour one side. Results: results/<tag>/{qu,py,py1}_<k>.txt,
# then agg.qu prints median / min / max per workload.
#
#   benchmarks/perf-0.4.9/run.sh [reps=5] [tag=run] [path/to/qu.exe]
set -u
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
reps="${1:-5}"
tag="${2:-run}"
qu="${3:-$root/engine/target/release/qu.exe}"
out="$here/results/$tag"
mkdir -p "$out"
{
  echo "date: $(date -u +%FT%TZ)"
  echo "qu: $("$qu" --version 2>&1 | head -1)"
  echo "python: $(python --version 2>&1)"
  python -c "import numpy,scipy,pandas;print('numpy',numpy.__version__,'scipy',scipy.__version__,'pandas',pandas.__version__)"
  python -c "import numpy; numpy.show_config()" 2>&1 | grep -iE "name:" | head -2
  echo "cores: $(nproc)"
  powershell -NoProfile -Command "(Get-CimInstance Win32_Processor).Name; 'load%: ' + (Get-CimInstance Win32_Processor).LoadPercentage" 2>&1
} > "$out/env.txt"
runqu() { { "$qu" run "$here/bench.qu" 2>&1; "$qu" run "$here/bench_sparse.qu" 2>&1; } | grep -E "^(BENCH|qu:|error|Error)" > "$out/qu_$1.txt"; }
runpy() { python "$here/bench.py" 2>&1 | grep -E '^(BENCH|Traceback|Error)' > "$out/py_$1.txt"; }
runpy1() { OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 python "$here/bench.py" 2>&1 | grep -E '^(BENCH|Traceback|Error)' > "$out/py1_$1.txt"; }
for k in $(seq 1 "$reps"); do
  if [ $((k % 2)) -eq 1 ]; then runqu "$k"; runpy "$k"; runpy1 "$k"
  else runpy1 "$k"; runpy "$k"; runqu "$k"; fi
  echo "rep $k done"
done
echo "$reps" > "$out/reps.txt"
"$qu" run "$here/agg.qu" -- "$out" 2>&1 | tee "$out/table.md"
