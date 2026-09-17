@echo off
REM Qu Build Script - Build all components
REM Builds the Qu engine and all crates

echo ========================================
echo   Qu Build System
echo ========================================
echo.

cd /d "%~dp0engine"

echo Building Qu engine (debug mode)...
cargo build

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo   Build successful!
    echo   Qu executable: engine\target\debug\qu.exe
    echo ========================================
    echo.
    echo Quick test:
    echo   engine\target\debug\qu.exe eval "1 to 5"
    echo.
) else (
    echo.
    echo ========================================
    echo   Build FAILED
    echo ========================================
    echo.
    echo Make sure Rust is installed:
    echo   https://rustup.rs/
    echo.
)

pause
