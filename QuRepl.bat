@echo off
REM Qu REPL - Interactive Command-line Interpreter
REM Quick access to the Qu language REPL

cd /d "%~dp0engine"

if exist "target\debug\qu.exe" (
    target\debug\qu.exe repl
) else if exist "target\release\qu.exe" (
    target\release\qu.exe repl
) else (
    echo Qu engine not built yet!
    echo.
    echo Building...
    cargo build
    if %ERRORLEVEL% EQU 0 (
        echo.
        echo Build complete! Starting REPL...
        echo.
        target\debug\qu.exe repl
    ) else (
        echo.
        echo Build failed! Make sure Rust is installed.
        echo https://rustup.rs/
        pause
        exit /b 1
    )
)
