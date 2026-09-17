"""Text processing: Python side of the Qu-vs-Python text benchmark.

Same corpus, same five operations, same order as bench.qu beside this file.
Data is built once, outside every timed region. See bench.qu's header for
why this scenario exists: nothing else in benchmarks/ touches strings, and
text is the area Qu is understood to be weakest in against CPython.
"""

import re
import time

N = 20000

# --- corpus: build once, outside the clock --------------------------------
lines = [f"row {i} sensor=A value={i * 7 % 1000} status=ok" for i in range(N)]
text = "\n".join(lines)
print(f"corpus: {N} lines, {len(text)} chars")

# --- 1. count lines containing a substring --------------------------------
t = time.perf_counter()
hits = 0
for ln in lines:
    if "status=ok" in ln:
        hits += 1
t_scan = time.perf_counter() - t
print(f"scan for substring   : {t_scan:.4f} s   ({hits} hits)")

# --- 2. split one big string into lines -----------------------------------
t = time.perf_counter()
parts = text.split("\n")
t_split = time.perf_counter() - t
print(f"split into lines     : {t_split:.4f} s   ({len(parts)} parts)")

# --- 3. replace across the whole corpus -----------------------------------
t = time.perf_counter()
swapped = text.replace("sensor=A", "sensor=B")
t_replace = time.perf_counter() - t
print(f"replace whole corpus : {t_replace:.4f} s   ({len(swapped)} chars)")

# --- 4. regex extract on every line ---------------------------------------
pat = re.compile(r"value=[0-9]+")
t = time.perf_counter()
found = 0
for ln in lines:
    if pat.search(ln):
        found += 1
t_regex = time.perf_counter() - t
print(f"regex match per line : {t_regex:.4f} s   ({found} matched)")

# --- 5. uppercase the corpus ----------------------------------------------
t = time.perf_counter()
up = text.upper()
t_upper = time.perf_counter() - t
print(f"upper whole corpus   : {t_upper:.4f} s   ({len(up)} chars)")

print(f"total                : {t_scan + t_split + t_replace + t_regex + t_upper:.4f} s")
