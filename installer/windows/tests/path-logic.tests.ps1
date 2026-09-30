# String-only tests for installer/windows/path-helper.ps1. Touches no
# registry and no environment, so it is safe to run anywhere:
#   powershell -NoProfile -ExecutionPolicy Bypass -File installer\windows\tests\path-logic.tests.ps1
# Exits non-zero on the first failing group (every failure is printed).

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\path-helper.ps1')

$script:fail = 0
function Check([string]$name, $got, $want) {
    if ($got -ceq $want) { Write-Host "ok   $name" }
    else {
        $script:fail++
        Write-Host "FAIL $name`n     got:  [$got]`n     want: [$want]"
    }
}

$d = 'C:\Users\u\AppData\Local\Programs\Qu'

# --- Add ---------------------------------------------------------------
Check 'add to empty' (Add-QuPathEntry '' $d) $d
Check 'add to null' (Add-QuPathEntry $null $d) $d
Check 'add plain' (Add-QuPathEntry 'C:\a;C:\b' $d) "C:\a;C:\b;$d"
Check 'add trailing ;' (Add-QuPathEntry 'C:\a;C:\b;' $d) "C:\a;C:\b;$d;"
Check 'add keeps %vars% unexpanded' (Add-QuPathEntry '%USERPROFILE%\bin;%SystemRoot%' $d) "%USERPROFILE%\bin;%SystemRoot%;$d"
Check 'dup exact' (Add-QuPathEntry "C:\a;$d" $d) $null
Check 'dup case' (Add-QuPathEntry "C:\a;$($d.ToUpper())" $d) $null
Check 'dup trailing \' (Add-QuPathEntry "C:\a;$d\" $d) $null
Check 'dup quoted' (Add-QuPathEntry "C:\a;`"$d`";C:\b" $d) $null
Check 'dup dir given with \' (Add-QuPathEntry "C:\a;$d" "$d\") $null
$env:QU_TEST_ROOT = 'C:\Users\u\AppData\Local'
Check 'dup via %var%' (Add-QuPathEntry 'C:\a;%QU_TEST_ROOT%\Programs\Qu' $d) $null
Check 'prefix is not dup' (Add-QuPathEntry "C:\a;$d-old" $d) "C:\a;$d-old;$d"
Check 'empty segments not dup' (Add-QuPathEntry 'C:\a;;C:\b' $d) "C:\a;;C:\b;$d"

# --- Remove -------------------------------------------------------------
Check 'remove last' (Remove-QuPathEntry "C:\a;$d" $d) 'C:\a'
Check 'remove middle' (Remove-QuPathEntry "C:\a;$d;C:\b" $d) 'C:\a;C:\b'
Check 'remove only' (Remove-QuPathEntry $d $d) ''
Check 'remove absent' (Remove-QuPathEntry 'C:\a;C:\b' $d) $null
Check 'remove from empty' (Remove-QuPathEntry '' $d) $null
Check 'remove touches one only' (Remove-QuPathEntry "$d;C:\a;$d" $d) "$d;C:\a"
Check 'remove case-insensitive' (Remove-QuPathEntry "C:\a;$($d.ToLower())\" $d) 'C:\a'
Check 'remove never prefix' (Remove-QuPathEntry "C:\a;$d-old" $d) $null

# --- Round trip: Remove(Add(p)) == p exactly -----------------------------
$long = (1..120 | ForEach-Object { "%SystemRoot%\some\fairly\long\tool\dir$_" }) -join ';'
if ($long.Length -le 2000) { throw 'long fixture too short' }
$cases = @(
    '', ';', ';;', 'C:\a', 'C:\a;', 'C:\a;;C:\b', ';C:\a', 'C:\a;C:\b;',
    '%USERPROFILE%\.cargo\bin;%SystemRoot%\system32;', ' C:\spaced ;C:\x',
    $long, "$long;"
)
$i = 0
foreach ($p in $cases) {
    $i++
    $added = Add-QuPathEntry $p $d
    Check "roundtrip #$i adds" ($null -ne $added) $true
    Check "roundtrip #$i exact (len $($p.Length))" (Remove-QuPathEntry $added $d) $p
    Check "roundtrip #$i re-add is no-op" (Add-QuPathEntry $added $d) $null
}
Check 'long PATH kept whole' (Add-QuPathEntry $long $d).Length ($long.Length + 1 + $d.Length)

Remove-Item Env:\QU_TEST_ROOT
if ($script:fail -gt 0) { Write-Host "$script:fail failure(s)"; exit 1 }
Write-Host 'all path-logic tests passed'
exit 0
