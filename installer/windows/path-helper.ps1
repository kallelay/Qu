# PATH editing for the Qu command-line installer (installer/windows/qu.nsi).
#
# Why PowerShell and not NSIS string code: NSIS strings are capped at 1024
# characters in the standard build (8192 in the large-strings build), and a
# real developer PATH is routinely longer. Anything longer is silently
# truncated -- and then written back, destroying the user's PATH. Here the
# PATH never passes through an NSIS variable at all.
#
# Two layers:
#   * Add-QuPathEntry / Remove-QuPathEntry are pure string functions -- no
#     registry, no environment -- so tests/path-logic.tests.ps1 can check
#     them against strings on any machine.
#   * The -Action wrapper at the bottom reads and writes the registry value
#     WITHOUT expanding %VARS% and keeping its value kind (REG_EXPAND_SZ
#     stays REG_EXPAND_SZ). Only the installer calls it.
#
# Invertibility: Remove(Add(p, d), d) == p for every p, including an empty
# PATH, a trailing ';' and a PATH that already has empty segments. That is
# why Add appends "d;" (not ";d") when p already ends in ';'.
#
# Usage (from the installer):
#   powershell -NoProfile -ExecutionPolicy Bypass -File path-helper.ps1 `
#       -Action Add|Remove -Dir <dir> -Scope User|Machine
# Exit codes: 0 = changed, 10 = nothing to do (already present / not
# present), anything else = failure.

param(
    [ValidateSet('Add', 'Remove', '')]
    [string]$Action = '',
    [string]$Dir = '',
    [ValidateSet('User', 'Machine')]
    [string]$Scope = 'User'
)

function ConvertTo-QuPathKey([string]$Entry) {
    # Comparison key only; never written back. Expands %VARS% so that
    # "%LOCALAPPDATA%\Programs\Qu" matches "C:\Users\x\AppData\Local\Programs\Qu".
    $e = $Entry.Trim().Trim('"').Trim()
    $e = [Environment]::ExpandEnvironmentVariables($e)
    $e = $e.TrimEnd('\', '/')
    return $e.ToLowerInvariant()
}

function Test-QuPathContains([string]$Path, [string]$Dir) {
    $key = ConvertTo-QuPathKey $Dir
    if ($key -eq '') { return $false }
    foreach ($seg in ($Path -split ';')) {
        if ((ConvertTo-QuPathKey $seg) -eq $key) { return $true }
    }
    return $false
}

# Returns the new PATH string, or $null when $Dir is already present.
function Add-QuPathEntry([string]$Path, [string]$Dir) {
    if ($null -eq $Path) { $Path = '' }
    if (Test-QuPathContains $Path $Dir) { return $null }
    if ($Path -eq '') { return $Dir }
    if ($Path.EndsWith(';')) { return "$Path$Dir;" }
    return "$Path;$Dir"
}

# Removes ONE segment matching $Dir -- the last one, which is where Add put
# it -- and leaves every other byte alone. Returns $null when absent.
function Remove-QuPathEntry([string]$Path, [string]$Dir) {
    if ($null -eq $Path -or $Path -eq '') { return $null }
    $key = ConvertTo-QuPathKey $Dir
    if ($key -eq '') { return $null }
    $segs = [System.Collections.Generic.List[string]]($Path -split ';')
    for ($i = $segs.Count - 1; $i -ge 0; $i--) {
        if ((ConvertTo-QuPathKey $segs[$i]) -eq $key) {
            $segs.RemoveAt($i)
            return ($segs -join ';')
        }
    }
    return $null
}

function Invoke-QuPathAction([string]$Action, [string]$Dir, [string]$Scope) {
    if ($Dir -eq '') { throw 'Dir is empty' }
    if ($Scope -eq 'Machine') {
        $root = [Microsoft.Win32.Registry]::LocalMachine
        $sub = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
    } else {
        $root = [Microsoft.Win32.Registry]::CurrentUser
        $sub = 'Environment'
    }
    $key = $root.OpenSubKey($sub, $true)
    if ($null -eq $key) {
        if ($Action -eq 'Remove') { return 10 }
        $key = $root.CreateSubKey($sub)
    }
    try {
        $exists = $key.GetValueNames() -contains 'Path'
        $old = ''
        $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        if ($exists) {
            $old = [string]$key.GetValue('Path', '',
                [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $kind = $key.GetValueKind('Path')
            if ($kind -ne [Microsoft.Win32.RegistryValueKind]::String -and
                $kind -ne [Microsoft.Win32.RegistryValueKind]::ExpandString) {
                throw "Path has unexpected registry type $kind; refusing to touch it"
            }
        }
        if ($Action -eq 'Add') {
            $new = Add-QuPathEntry $old $Dir
        } else {
            $new = Remove-QuPathEntry $old $Dir
        }
        if ($null -eq $new) { return 10 }
        if ($new -eq '') {
            # Only reachable by removing the sole entry we added to a PATH
            # that was empty or missing before: missing again, as it was.
            $key.DeleteValue('Path', $false)
        } else {
            $key.SetValue('Path', $new, $kind)
        }
        return 0
    } finally {
        $key.Close()
    }
}

if ($Action -ne '') {
    try {
        exit (Invoke-QuPathAction $Action $Dir $Scope)
    } catch {
        [Console]::Error.WriteLine("path-helper: $($_.Exception.Message)")
        exit 1
    }
}
