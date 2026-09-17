# driver.ps1 -- re-runnable harness for the rayon/elementwise measurement.
#
# Wall-clock timing on this machine is unusable on its own: measured 2-3x
# run-to-run drift while other sessions build (three consecutive identical
# runs gave abs = 0.0092, 0.0047, 0.0034 s). Three things make the numbers
# reproduce anyway, and all three are load-bearing -- dropping any one of
# them brought back tables that did not replicate:
#
#   1. BASELINE SUBTRACTION. Every probe has a twin that builds the same data
#      and runs the same empty loop. Subtracting it removes process startup
#      and randn() generation, which are a large and variable share of a
#      short run.
#   2. ALTERNATING PASS ORDER. Ascending then descending thread order, so a
#      monotonic drift cannot masquerade as a thread-count effect.
#   3. MIN OVER PASSES. The minimum is the pass least contaminated by
#      whatever else the machine was scheduling.
#
# TotalProcessorTime has 15.625 ms granularity on Windows; over 60 reps that
# quantises to ~0.26 ms/rep, which is why CPU figures land on multiples of
# it. Fine against values of 4-48 ms, but do not read the third decimal.
#
# Usage:  powershell -File driver.ps1 [-Reps 4]

param([int]$Reps = 4)

$QU  = "D:\qu-bench\engine\target\release\qu.exe"
$DIR = $PSScriptRoot

function Probe($script, $threads) {
    $env:RAYON_NUM_THREADS = $threads
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName  = $QU
    $psi.Arguments = "run $DIR\$script"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    $null = $p.StandardOutput.ReadToEnd()
    $p.WaitForExit()
    [pscustomobject]@{
        CPU  = $p.TotalProcessorTime.TotalSeconds
        Wall = ($p.ExitTime - $p.StartTime).TotalSeconds
    }
}

function Sweep($ops, $threads) {
    $mc = @{}; $mw = @{}
    foreach ($pass in 1..$Reps) {
        $seq = if ($pass % 2 -eq 1) { $ops } else { $ops[($ops.Count - 1)..0] }
        foreach ($t in $threads) {
            foreach ($op in $seq) {
                $r = Probe "cpu_$op.qu" $t
                $k = "$op|$t"
                if (-not $mc.ContainsKey($k) -or $r.CPU  -lt $mc[$k]) { $mc[$k] = $r.CPU }
                if (-not $mw.ContainsKey($k) -or $r.Wall -lt $mw[$k]) { $mw[$k] = $r.Wall }
            }
        }
    }
    @{ CPU = $mc; Wall = $mw }
}

$load = (Get-CimInstance Win32_Processor).LoadPercentage
$busy = (Get-Process -Name rustc, cargo -ErrorAction SilentlyContinue | Measure-Object).Count
"machine at start: load $load %, rustc+cargo processes $busy"
if ($load -gt 25) { "WARNING: above the 25% gate -- treat WALL numbers as provisional." }
""

# ---- thread sweep, matrix path only (container fixed, pool width varies) ---
$threads = @(1, 2, 4, 8, 16)
$r = Sweep @("baseline", "sin", "abs", "mul") $threads
"4,000,000-element MATRIX, 60 reps/process, baseline subtracted, min of $Reps passes"
""
"{0,-8} {1,9} {2,9} {3,9}   {4,9} {5,9} {6,9}" -f "threads", "sin w", "abs w", "mul w", "sin cpu", "abs cpu", "mul cpu"
foreach ($t in $threads) {
    $bw = $r.Wall["baseline|$t"]; $bc = $r.CPU["baseline|$t"]
    $v = foreach ($o in @("sin", "abs", "mul")) { [math]::Round(1000 * ($r.Wall["$o|$t"] - $bw) / 60, 2) }
    $c = foreach ($o in @("sin", "abs", "mul")) { [math]::Round(1000 * ($r.CPU["$o|$t"]  - $bc) / 60, 2) }
    "{0,-8} {1,9} {2,9} {3,9}   {4,9} {5,9} {6,9}" -f $t, $v[0], $v[1], $v[2], $c[0], $c[1], $c[2]
}
""

# ---- Vec (never parallelised) vs Mat (full pool), identical arithmetic -----
$r2 = Sweep @("vbaseline", "vsin", "vabs", "vmul", "baseline", "sin", "abs", "mul") @(16)
"Vec (serial, never parallelised at any size) vs Mat (full 16-thread pool)"
""
"{0,-8} {1,10} {2,10} {3,10} {4,10} {5,14}" -f "op", "Vec wall", "Mat wall", "Vec CPU", "Mat CPU", "verdict"
foreach ($o in @("sin", "abs", "mul")) {
    $vw = ($r2.Wall["v$o|16"] - $r2.Wall["vbaseline|16"]) / 60
    $mw = ($r2.Wall["$o|16"]  - $r2.Wall["baseline|16"])  / 60
    $vc = ($r2.CPU["v$o|16"]  - $r2.CPU["vbaseline|16"])  / 60
    $mc = ($r2.CPU["$o|16"]   - $r2.CPU["baseline|16"])   / 60
    $ratio = $vw / $mw
    $verdict = if ($ratio -gt 1.15) { "Mat " + [math]::Round($ratio, 2) + "x" }
               elseif ($ratio -lt 0.87) { "Vec " + [math]::Round(1 / $ratio, 2) + "x" }
               else { "tie" }
    "{0,-8} {1,10} {2,10} {3,10} {4,10} {5,14}" -f $o,
        [math]::Round(1000 * $vw, 2), [math]::Round(1000 * $mw, 2),
        [math]::Round(1000 * $vc, 2), [math]::Round(1000 * $mc, 2), $verdict
}
""

# ---- the same comparison with two DISTINCT operands ------------------------
$r3 = Sweep @("vbaseline2", "vmul2", "baseline2", "mul2") @(16)
$vw = ($r3.Wall["vmul2|16"] - $r3.Wall["vbaseline2|16"]) / 60
$mw = ($r3.Wall["mul2|16"]  - $r3.Wall["baseline2|16"])  / 60
$vc = ($r3.CPU["vmul2|16"]  - $r3.CPU["vbaseline2|16"])  / 60
$mc = ($r3.CPU["mul2|16"]   - $r3.CPU["baseline2|16"])   / 60
$mb = 3 * 4000000 * 8 / 1MB
"v .* w, two DISTINCT operands (cpu_mul.qu squares one array, halving traffic)"
"  Vec serial : {0,6} ms wall, {1,6} ms CPU, {2,5} GB/s" -f [math]::Round(1000*$vw,2), [math]::Round(1000*$vc,2), [math]::Round($mb/1024/$vw,1)
"  Mat 16-thr : {0,6} ms wall, {1,6} ms CPU, {2,5} GB/s" -f [math]::Round(1000*$mw,2), [math]::Round(1000*$mc,2), [math]::Round($mb/1024/$mw,1)
"  Mat/Vec    : {0}x wall, {1}x CPU" -f [math]::Round($mw/$vw,2), [math]::Round($mc/$vc,2)
$env:RAYON_NUM_THREADS = ""
