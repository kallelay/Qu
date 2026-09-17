@echo off
REM ========================================
REM   QuStudio IDE - Quick Launch
REM ========================================

echo.
echo ========================================
echo   QuStudio IDE
echo   Signal Processing & ML Environment
echo ========================================
echo.

cd /d "%~dp0qu-studio-tauri"

REM Check if node_modules exists
if not exist "node_modules" (
    echo Installing dependencies...
    echo This takes 2-3 minutes on first run.
    echo.
    call npm install
    if errorlevel 1 (
        echo.
        echo ERROR: npm install failed!
        echo Please check that Node.js is installed.
        pause
        exit /b 1
    )
    echo.
    echo Dependencies installed!
    echo.
)

REM Launch
echo Starting QuStudio...
echo.
echo First build: 2-3 minutes
echo Subsequent: 10-20 seconds
echo.
echo Press Ctrl+C to stop
echo.

call npm run tauri dev

if errorlevel 1 (
    echo.
    echo Build failed! Check error messages above.
    echo.
    echo Common fixes:
    echo 1. Make sure Rust is installed: https://rustup.rs/
    echo 2. Run: cargo install tauri-cli
    echo 3. Check that qu CLI is built: cd engine\crates\qu-cli ^&^& cargo build
    echo.
    pause
)
