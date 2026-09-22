@echo off
REM Qu Build Script - build the Qu engine binary.
REM
REM Delegates to tools/build_qu.sh, the canonical entry point, rather than
REM spelling the build out again: this file used to be one of SIX distinct
REM incantations of "build qu" in the repo, and the only checking it did
REM was to guess that Rust might be missing. build_qu.sh refuses to build
REM from a parked tree and verifies the artifact by RUNNING it.

setlocal enabledelayedexpansion

echo ========================================
echo   Qu Build System
echo ========================================
echo.

cd /d "%~dp0"

REM Find Git Bash specifically. Plain `bash` on Windows resolves to
REM C:\Windows\System32\bash.exe -- the WSL launcher -- which runs in a
REM different filesystem namespace and answers "not inside a git
REM checkout" for a perfectly good Windows checkout. Verified 2026-09-16:
REM `where bash` returns that WSL stub first on this machine, and calling
REM plain `bash` here made this script fail outright.
REM
REM Derive Git Bash from git's OWN location rather than hardcoding
REM %PROGRAMFILES%, so a non-default install (scoop, winget, portable)
REM still works.
set "QUBASH="
for %%G in (git.exe) do set "GITEXE=%%~$PATH:G"
if defined GITEXE (
    for %%H in ("!GITEXE!") do set "GITDIR=%%~dpH"
    if exist "!GITDIR!..\bin\bash.exe" set "QUBASH=!GITDIR!..\bin\bash.exe"
)
if not defined QUBASH if exist "%PROGRAMFILES%\Git\bin\bash.exe" set "QUBASH=%PROGRAMFILES%\Git\bin\bash.exe"
if not defined QUBASH set "QUBASH=bash"

echo Building Qu engine (debug mode)...
echo   using shell: !QUBASH!
"!QUBASH!" tools/build_qu.sh --debug
set "RC=!ERRORLEVEL!"

if "!RC!"=="0" (
    echo.
    echo ========================================
    echo   Build successful!
    echo ========================================
    echo.
    echo Quick test:
    echo   engine\target\debug\qu.exe eval "1 to 5"
    echo.
) else (
    echo.
    echo ========================================
    echo   Build FAILED ^(exit !RC!^)
    echo ========================================
    echo.
    echo If the message above says Rust is missing:
    echo   https://rustup.rs/
    echo If it says PARKED, you are in a stale checkout -- build in your
    echo own worktree instead.
    echo.
)

REM NO `pause` here, deliberately. This file used to end with an
REM unconditional one, which hangs any script or CI job that calls it,
REM forever, waiting on a keypress nobody is there to press -- and it
REM exits 0 while doing nothing, so the caller has no reason to look.
REM
REM Pausing only for a double-click was tried and does NOT work: the
REM usual %cmdcmdline% test matches a scripted `cmd /c .\QuBuild.bat`
REM just as well as an Explorer launch (verified 2026-09-16), so it
REM would have paused for exactly the callers it was meant to spare.
REM If you double-clicked this and the window vanished before you could
REM read an error, run it from a terminal instead:
REM     bash tools/build_qu.sh --debug

endlocal & exit /b %RC%
