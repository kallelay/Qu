# End-to-end check of the qu command-line installer against the REAL
# registry. It installs, reinstalls and uninstalls, rewrites the user and
# machine PATH to test fixtures, and restores them at the end -- so it runs
# ONLY on a throwaway CI machine (.github/workflows/installer-verify-cli.yml).
# Never run it on a machine whose PATH you care about.
#
#   pwsh -File installer\windows\tests\verify-installer.ps1 -Setup <setup.exe> -Version <x.y.z> [-ExpectComponents]
#
# -ExpectComponents: the installer was built with its optional payloads
# (qu-jupyter.exe, docs\, editors\ in SRCDIR -- what release.yml stages),
# so also check the Jupyter kernel, documentation and shortcut components.

param(
    [Parameter(Mandatory)] [string]$Setup,
    [Parameter(Mandatory)] [string]$Version,
    [switch]$SkipMachine,
    [switch]$ExpectComponents
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw 'verify-installer.ps1 edits the real PATH and only runs under GitHub Actions.'
}
$Setup = (Resolve-Path $Setup).Path
$work = Join-Path $env:RUNNER_TEMP 'qu-installer-verify'
New-Item -ItemType Directory -Force $work | Out-Null

$script:failures = 0
function Assert([bool]$cond, [string]$what) {
    if ($cond) { Write-Host "  ok   $what" }
    else { $script:failures++; Write-Host "::error::FAIL $what"; Write-Host "  FAIL $what" }
}

# ---- registry access (raw: %VARS% not expanded, kind preserved) ---------
function Open-EnvKey([string]$Scope, [bool]$Write = $false) {
    if ($Scope -eq 'Machine') {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine', 'Registry64')
        return $base.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment', $Write)
    }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
    return $base.OpenSubKey('Environment', $Write)
}
function Get-RawPath([string]$Scope) {
    $k = Open-EnvKey $Scope
    try {
        if ($k.GetValueNames() -notcontains 'Path') { return [pscustomobject]@{ Exists = $false; Value = $null; Kind = $null } }
        return [pscustomobject]@{
            Exists = $true
            Value  = [string]$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
            Kind   = [string]$k.GetValueKind('Path')
        }
    } finally { $k.Close() }
}
function Set-RawPath([string]$Scope, $State) {
    $k = Open-EnvKey $Scope $true
    try {
        if (-not $State.Exists) { $k.DeleteValue('Path', $false) }
        else { $k.SetValue('Path', $State.Value, [Microsoft.Win32.RegistryValueKind]$State.Kind) }
    } finally { $k.Close() }
}
function Same-Path($a, $b) {
    return ($a.Exists -eq $b.Exists) -and ($a.Value -ceq $b.Value) -and ($a.Kind -eq $b.Kind)
}
function Show-Path([string]$label, $s) {
    if (-not $s.Exists) { Write-Host "  $label : <missing>" }
    else { Write-Host "  $label : [$($s.Kind), $($s.Value.Length) chars] $($s.Value)" }
}
function Count-Entries([string]$Value, [string]$Dir) {
    if (-not $Value) { return 0 }
    $key = $Dir.TrimEnd('\').ToLowerInvariant()
    return @($Value -split ';' | Where-Object {
        [Environment]::ExpandEnvironmentVariables($_.Trim().Trim('"')).TrimEnd('\').ToLowerInvariant() -eq $key
    }).Count
}
function Get-Arp([string]$Scope) {
    $hive = if ($Scope -eq 'Machine') { 'LocalMachine' } else { 'CurrentUser' }
    $k = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, 'Registry64').OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\QuCLI')
    if ($null -eq $k) { return $null }
    try {
        $o = @{}
        foreach ($n in $k.GetValueNames()) { $o[$n] = $k.GetValue($n) }
        return [pscustomobject]$o
    } finally { $k.Close() }
}

# ---- a NEW process whose PATH comes from the registry, as a fresh -------
# ---- terminal's would (machine Path, then user Path, both expanded) -----
function Invoke-FreshShell([string]$Command) {
    $m = Get-RawPath 'Machine'; $u = Get-RawPath 'User'
    $parts = @()
    if ($m.Exists) { $parts += [Environment]::ExpandEnvironmentVariables($m.Value) }
    if ($u.Exists) { $parts += [Environment]::ExpandEnvironmentVariables($u.Value) }
    $psi = [System.Diagnostics.ProcessStartInfo]::new('cmd.exe', "/d /c $Command")
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.EnvironmentVariables['Path'] = ($parts -join ';')
    $p = [System.Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd() + $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Out = $out.Trim() }
}
function Assert-QuResolves([string]$Dir) {
    $w = Invoke-FreshShell 'where qu'
    $first = ($w.Out -split "`r?`n")[0]
    # A PATH entry written with a trailing '\' can come back as 'dir\\qu.exe'.
    $first = $first -replace '\\+', '\'
    Assert ($w.Code -eq 0 -and $first -ieq (Join-Path $Dir 'qu.exe')) "new process resolves qu to $Dir\qu.exe (got: $first)"
    $v = Invoke-FreshShell 'qu --version'
    Assert ($v.Code -eq 0 -and $v.Out -ceq "qu $Version") "qu --version via PATH prints 'qu $Version' (got: $($v.Out))"
}
function Assert-QuGone {
    $w = Invoke-FreshShell 'where qu'
    Assert ($w.Code -ne 0) "qu no longer resolves via PATH (where: $($w.Out))"
}

# ---- install / uninstall ------------------------------------------------
function Install([string[]]$ArgList) {
    Write-Host "  > setup $($ArgList -join ' ')"
    $p = Start-Process -FilePath $Setup -ArgumentList $ArgList -Wait -PassThru
    Assert ($p.ExitCode -eq 0) "installer exit code 0 (got $($p.ExitCode))"
}
function Uninstall([string]$Dir, [string]$Scope) {
    $un = Join-Path $Dir 'uninstall.exe'
    Assert (Test-Path $un) "uninstall.exe present in $Dir"
    $a = @('/S')
    if ($Scope -eq 'Machine') { $a += '/ALLUSERS' }
    Write-Host "  > uninstall $($a -join ' ')"
    Start-Process -FilePath $un -ArgumentList $a -Wait | Out-Null
    # The NSIS uninstaller re-launches itself from %TEMP% and the first
    # process exits at once; wait for the real one to finish its work.
    $deadline = (Get-Date).AddSeconds(90)
    while ((Test-Path $un) -or ($null -ne (Get-Arp $Scope))) {
        if ((Get-Date) -gt $deadline) { break }
        Start-Sleep -Milliseconds 500
    }
    Assert (-not (Test-Path $un)) 'uninstall.exe removed'
    foreach ($f in 'qu.exe', 'LICENSE', 'NOTICE', 'README.md', 'path-helper.ps1', 'install.ini') {
        Assert (-not (Test-Path (Join-Path $Dir $f))) "$f removed"
    }
    Assert ($null -eq (Get-Arp $Scope)) "Add/Remove Programs entry removed ($Scope)"
}
function Assert-Arp([string]$Scope, [string]$Dir) {
    $arp = Get-Arp $Scope
    Assert ($null -ne $arp) "Add/Remove Programs entry exists ($Scope)"
    if ($null -eq $arp) { return }
    Assert ($arp.DisplayVersion -ceq $Version) "DisplayVersion = $Version (got $($arp.DisplayVersion))"
    Assert ($arp.Publisher -ceq 'Ahmed Yahia Kallel') "Publisher set (got $($arp.Publisher))"
    Assert ($arp.InstallLocation -ieq $Dir) "InstallLocation = $Dir (got $($arp.InstallLocation))"
    Assert ($arp.UninstallString -like "`"$Dir\uninstall.exe`"*") "UninstallString points at $Dir\uninstall.exe (got $($arp.UninstallString))"
}

# ---- one full user-scope cycle against a given starting PATH -------------
function Test-UserCycle([string]$Name, $Start, [string]$Dir, [bool]$ExpectAdd = $true) {
    Write-Host "`n== $Name"
    Set-RawPath 'User' $Start
    $before = Get-RawPath 'User'
    $machineBefore = Get-RawPath 'Machine'
    Show-Path 'user PATH before' $before

    $a = @('/S')
    if ($Dir) { $a += "/D=$Dir" } else { $Dir = Join-Path $env:LOCALAPPDATA 'Programs\Qu' }
    Install $a
    $after1 = Get-RawPath 'User'
    Show-Path 'user PATH after install' $after1
    Assert (Test-Path (Join-Path $Dir 'qu.exe')) "qu.exe installed in $Dir"
    Assert ((Count-Entries $after1.Value $Dir) -eq 1) 'exactly one PATH entry for the install dir'
    if ($ExpectAdd) {
        Assert ($after1.Value.StartsWith([string]$before.Value)) 'existing PATH kept verbatim as a prefix (no expansion, no truncation)'
    } else {
        Assert (Same-Path $after1 $before) 'PATH unchanged (entry was already there)'
    }
    $wantKind = if ($before.Exists) { $before.Kind } else { 'ExpandString' }
    Assert ($after1.Kind -eq $wantKind) "value kind stays $wantKind (got $($after1.Kind))"
    Assert (Same-Path (Get-RawPath 'Machine') $machineBefore) 'machine PATH untouched'
    Assert-Arp 'User' $Dir
    Assert-QuResolves $Dir

    Install $a
    $after2 = Get-RawPath 'User'
    Assert (Same-Path $after2 $after1) 'second install leaves PATH identical (no duplicate)'

    Uninstall $Dir 'User'
    $final = Get-RawPath 'User'
    Show-Path 'user PATH after uninstall' $final
    Assert (Same-Path $final $before) 'PATH restored exactly (value, kind, presence)'
    Assert (Same-Path (Get-RawPath 'Machine') $machineBefore) 'machine PATH still untouched'
    if ($ExpectAdd) { Assert-QuGone }
}

# ---- .qu file association (HKCU\Software\Classes) ------------------------
function Open-Classes([bool]$Write = $false) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
    return $base.OpenSubKey('Software\Classes', $Write)
}
# Value of a key under Software\Classes; $null if the key or value is absent.
function Get-ClassValue([string]$SubKey, [string]$Name = '') {
    $c = Open-Classes
    try {
        $k = $c.OpenSubKey($SubKey)
        if ($null -eq $k) { return $null }
        try { if ($k.GetValueNames() -notcontains $Name) { return $null } ; return [string]$k.GetValue($Name) } finally { $k.Close() }
    } finally { $c.Close() }
}
function Test-ClassKey([string]$SubKey) {
    $c = Open-Classes
    try { $k = $c.OpenSubKey($SubKey); if ($null -eq $k) { return $false } ; $k.Close(); return $true } finally { $c.Close() }
}
function Remove-ClassKey([string]$SubKey) {
    $c = Open-Classes $true
    try { $c.DeleteSubKeyTree($SubKey, $false) } finally { $c.Close() }
}
function Set-ClassValue([string]$SubKey, [string]$Name, [string]$Value) {
    $c = Open-Classes $true
    try { $k = $c.CreateSubKey($SubKey); $k.SetValue($Name, $Value); $k.Close() } finally { $c.Close() }
}
function Assert-EditorsDetect([string]$Dir) {
    # exit 0 (found) or 1 (not found) are both fine; a crash or a missing
    # subcommand is not. The contract prints one line per editor.
    $out = (& (Join-Path $Dir 'qu.exe') editors detect 2>&1 | Out-String)
    $code = $LASTEXITCODE
    Assert ($code -eq 0 -or $code -eq 1) "qu editors detect exits 0 or 1 (got $code)"
    Assert ($out -match '(?i)vs ?code|vscode') "qu editors detect lists the editors (got: $($out.Trim()))"
}

function Test-Association {
    Write-Host "`n== .qu file association"
    $dir = Join-Path $work 'Assoc'
    $qu = Join-Path $dir 'qu.exe'
    $opts = @('/S', '/NOPATH', '/NOSHORTCUTS', '/NOJUPYTER', '/NOPLUGINS', "/D=$dir")
    $bk = @{ Dot = (Get-ClassValue '.qu'); HadDot = (Test-ClassKey '.qu'); HadProg = (Test-ClassKey 'Qu.Script') }
    try {
        # A: another program already owns .qu -> never taken over, left intact
        Remove-ClassKey '.qu'; Remove-ClassKey 'Qu.Script'; Remove-ClassKey 'Other.Script'
        Set-ClassValue '.qu' '' 'Other.Script'
        Set-ClassValue '.qu\OpenWithProgids' 'Other.Script' ''
        Set-ClassValue 'Other.Script' '' 'Other script'
        Set-ClassValue 'Other.Script\shell\open\command' '' '"C:\Other\other.exe" "%1"'
        Install $opts
        Assert ((Get-ClassValue '.qu') -ceq 'Other.Script') '.qu default still Other.Script (not hijacked)'
        Assert ((Get-ClassValue 'Qu.Script') -ceq 'Qu script') 'ProgID Qu.Script named "Qu script"'
        Assert ((Get-ClassValue 'Qu.Script' 'FriendlyTypeName') -ceq 'Qu script') 'FriendlyTypeName = Qu script'
        Assert ((Get-ClassValue 'Qu.Script\DefaultIcon') -like '*qu.exe*') 'DefaultIcon set'
        $open = Get-ClassValue 'Qu.Script\shell\open\command'
        Assert ($open -like '*notepad.exe*' -and $open -notlike '*qu.exe*') "double-click verb opens an editor, does NOT run qu (got: $open)"
        $run = Get-ClassValue 'Qu.Script\shell\run\command'
        Assert ($run -like "*$qu*" -and $run -like '* run *') "explicit 'Run with Qu' verb runs 'qu run' (got: $run)"
        Assert ($null -ne (Get-ClassValue '.qu\OpenWithProgids' 'Qu.Script')) 'listed under Open with (OpenWithProgids)'
        Assert ($null -ne (Get-ClassValue '.qu\OpenWithProgids' 'Other.Script')) "the other program's OpenWithProgids entry kept"
        Assert-EditorsDetect $dir
        Uninstall $dir 'User'
        Assert (-not (Test-ClassKey 'Qu.Script')) 'uninstall removed Qu.Script'
        Assert ($null -eq (Get-ClassValue '.qu\OpenWithProgids' 'Qu.Script')) 'uninstall removed our OpenWithProgids value'
        Assert ((Get-ClassValue '.qu') -ceq 'Other.Script') 'pre-existing .qu association untouched after uninstall'
        Assert ($null -ne (Get-ClassValue '.qu\OpenWithProgids' 'Other.Script')) "other program's OpenWithProgids untouched after uninstall"
        Assert ((Get-ClassValue 'Other.Script\shell\open\command') -ceq '"C:\Other\other.exe" "%1"') "other program's ProgID untouched after uninstall"

        # B: nothing owns .qu -> becomes the default, and is fully removed
        Remove-ClassKey '.qu'; Remove-ClassKey 'Other.Script'
        Install $opts
        Assert ((Get-ClassValue '.qu') -ceq 'Qu.Script') 'unowned .qu now defaults to Qu.Script'
        Uninstall $dir 'User'
        Assert (-not (Test-ClassKey '.qu')) 'uninstall removed the .qu key it created'
        Assert (-not (Test-ClassKey 'Qu.Script')) 'uninstall removed Qu.Script'

        # C: /NOASSOC writes nothing
        Install ($opts[0..4] + '/NOASSOC' + $opts[5])   # /D= must stay last
        Assert (-not (Test-ClassKey 'Qu.Script') -and -not (Test-ClassKey '.qu')) '/NOASSOC: no file association written'
        Uninstall $dir 'User'
    } finally {
        Remove-ClassKey '.qu'; Remove-ClassKey 'Qu.Script'; Remove-ClassKey 'Other.Script'
        if ($bk.HadDot) { Set-ClassValue '.qu' '' ([string]$bk.Dot) }
    }
}

$origUser = Get-RawPath 'User'
$origMachine = Get-RawPath 'Machine'
try {
    Test-Association

    Test-UserCycle 'plain REG_EXPAND_SZ with %vars%' `
        ([pscustomobject]@{ Exists = $true; Value = '%USERPROFILE%\bin;C:\tools'; Kind = 'ExpandString' }) `
        (Join-Path $work 'A')

    $long = ((1..90 | ForEach-Object { "%SystemRoot%\fixture\a-fairly-long-directory-name\number$_" }) -join ';') + ';'
    Assert ($long.Length -gt 2000) "long fixture is $($long.Length) chars (> 2000)"
    Test-UserCycle "PATH of $($long.Length) chars, trailing ';'" `
        ([pscustomobject]@{ Exists = $true; Value = $long; Kind = 'ExpandString' }) `
        (Join-Path $work 'Long Dir With Spaces')

    Test-UserCycle 'missing PATH value' `
        ([pscustomobject]@{ Exists = $false; Value = $null; Kind = $null }) `
        (Join-Path $work 'C')

    Test-UserCycle 'REG_SZ stays REG_SZ' `
        ([pscustomobject]@{ Exists = $true; Value = 'C:\a;C:\b'; Kind = 'String' }) `
        (Join-Path $work 'D')

    $e = Join-Path $work 'E'
    Test-UserCycle 'entry already present (other case, trailing \)' `
        ([pscustomobject]@{ Exists = $true; Value = "C:\a;$($e.ToUpperInvariant())\;C:\b"; Kind = 'ExpandString' }) `
        $e $false

    Test-UserCycle 'default directory (no /D=)' `
        ([pscustomobject]@{ Exists = $true; Value = 'C:\a'; Kind = 'ExpandString' }) `
        ''

    if ($ExpectComponents) {
        Write-Host "`n== optional components, defaults (Jupyter on, docs off, Start menu on)"
        $cDir = Join-Path $work 'Components'
        $kernel = Join-Path $env:APPDATA 'jupyter\kernels\qu\kernel.json'
        $sm = Join-Path ([Environment]::GetFolderPath('Programs')) 'Qu'
        $desk = [Environment]::GetFolderPath('Desktop')
        if (Test-Path $kernel) { Remove-Item $kernel }
        Install @('/S', "/D=$cDir")
        Assert (Test-Path (Join-Path $cDir 'qu-jupyter.exe')) 'qu-jupyter.exe installed'
        Assert (Test-Path $kernel) "Qu kernelspec registered at $kernel"
        if (Test-Path $kernel) {
            $argv0 = (Get-Content $kernel -Raw | ConvertFrom-Json).argv[0]
            Assert ($argv0 -ieq (Join-Path $cDir 'qu-jupyter.exe')) "kernelspec launches the installed qu-jupyter.exe (got $argv0)"
        }
        Assert (-not (Test-Path (Join-Path $cDir 'docs'))) 'documentation not installed by default'
        Assert (Test-Path (Join-Path $sm 'Qu CLI (REPL).lnk')) 'Start menu: Qu CLI (REPL)'
        Assert (Test-Path (Join-Path $sm 'Start Jupyter (Qu).lnk')) 'Start menu: Start Jupyter (Qu)'
        Assert (-not (Test-Path (Join-Path $sm 'Qu Documentation.lnk'))) 'no documentation shortcut without the docs'
        Assert (-not (Test-Path (Join-Path $desk 'Qu CLI (REPL).lnk'))) 'no desktop shortcut by default'
        Uninstall $cDir 'User'
        Assert (-not (Test-Path $kernel)) 'uninstall removed the kernelspec'
        Assert (-not (Test-Path (Join-Path $cDir 'qu-jupyter.exe'))) 'uninstall removed qu-jupyter.exe'
        Assert (-not (Test-Path $sm)) 'uninstall removed the Start menu folder'

        Write-Host "`n== optional components: /WITHDOCS /NOJUPYTER /DESKTOP /NOPATH"
        $dDir = Join-Path $work 'Docs'
        $pathBefore = Get-RawPath 'User'
        Install @('/S', '/WITHDOCS', '/NOJUPYTER', '/DESKTOP', '/NOPATH', "/D=$dDir")
        $pathAfter = Get-RawPath 'User'
        Assert (($pathAfter.Exists -eq $pathBefore.Exists) -and ($pathAfter.Value -ceq $pathBefore.Value)) '/NOPATH: user PATH untouched'
        Assert (Test-Path (Join-Path $dDir 'docs\index.html')) 'documentation installed (docs\index.html)'
        Assert (Test-Path (Join-Path $dDir 'docs\fn\plot.html')) 'function reference pages installed'
        Assert (-not (Test-Path (Join-Path $dDir 'qu-jupyter.exe'))) '/NOJUPYTER: no qu-jupyter.exe'
        Assert (-not (Test-Path $kernel)) '/NOJUPYTER: no kernelspec'
        Assert (Test-Path (Join-Path $sm 'Qu Documentation.lnk')) 'Start menu: Qu Documentation'
        Assert (-not (Test-Path (Join-Path $sm 'Start Jupyter (Qu).lnk'))) 'no Jupyter shortcut without the kernel'
        Assert (Test-Path (Join-Path $desk 'Qu CLI (REPL).lnk')) '/DESKTOP: Qu CLI (REPL) on the desktop'
        $probe = Join-Path $work 'help_probe.qu'
        Set-Content -Path $probe -Value 'help("plot")' -Encoding ascii
        # Run directly, not through Invoke-FreshShell: cmd /c strips the
        # outer quotes of a line that starts and ends with one.
        $hOut = (& (Join-Path $dDir 'qu.exe') $probe 2>&1 | Out-String).Trim()
        $h = [pscustomobject]@{ Out = $hOut }
        Assert ($h.Out -match [regex]::Escape((Join-Path $dDir 'docs\fn\plot.html'))) "help() points at the local docs (got: $($h.Out))"
        Uninstall $dDir 'User'
        Assert (-not (Test-Path (Join-Path $dDir 'docs'))) 'uninstall removed docs\'
        Assert (-not (Test-Path (Join-Path $desk 'Qu CLI (REPL).lnk'))) 'uninstall removed the desktop shortcut'
    }

    if (-not $SkipMachine) {
        Write-Host "`n== machine-wide (/ALLUSERS)"
        Set-RawPath 'User' $origUser
        $mDir = Join-Path $work 'Machine'
        $mBefore = Get-RawPath 'Machine'
        $uBefore = Get-RawPath 'User'
        Show-Path 'machine PATH before' $mBefore
        Install @('/S', '/ALLUSERS', "/D=$mDir")
        $m1 = Get-RawPath 'Machine'
        Assert ((Count-Entries $m1.Value $mDir) -eq 1) 'machine PATH has exactly one entry'
        Assert ($m1.Kind -eq $mBefore.Kind) "machine PATH kind stays $($mBefore.Kind)"
        Assert ($m1.Value.StartsWith($mBefore.Value)) 'machine PATH kept verbatim as a prefix'
        Assert (Same-Path (Get-RawPath 'User') $uBefore) 'user PATH untouched by /ALLUSERS'
        Assert-Arp 'Machine' $mDir
        Assert ($null -eq (Get-Arp 'User')) 'no per-user Add/Remove Programs entry'
        Assert-QuResolves $mDir
        Install @('/S', '/ALLUSERS', "/D=$mDir")
        Assert (Same-Path (Get-RawPath 'Machine') $m1) 'second /ALLUSERS install: no duplicate'
        Uninstall $mDir 'Machine'
        Assert (Same-Path (Get-RawPath 'Machine') $mBefore) 'machine PATH restored exactly'
        Assert (Same-Path (Get-RawPath 'User') $uBefore) 'user PATH still untouched'
    }
} finally {
    Set-RawPath 'User' $origUser
    if (-not (Same-Path (Get-RawPath 'Machine') $origMachine)) { Set-RawPath 'Machine' $origMachine }
}

Write-Host ''
if ($script:failures -gt 0) { Write-Host "$script:failures check(s) failed"; exit 1 }
Write-Host 'installer verified'
exit 0
