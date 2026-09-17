@echo off
REM QuStudio - Modern IDE for Qu Language (Tauri)
REM Launches the QuStudio development environment

cd /d "%~dp0qu-studio-tauri"

echo ========================================
echo   Qu Studio v0.1.0
echo   Modern IDE powered by Tauri
echo ========================================
echo.

REM Check if Node.js is installed
node --version >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo Node.js not found!
    echo.
    echo Please install Node.js from:
    echo https://nodejs.org/
    echo.
    pause
    exit /b 1
)

REM Check if Rust is installed
rustc --version >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo Rust not found!
    echo.
    echo Please install Rust from:
    echo https://rustup.rs/
    echo.
    pause
    exit /b 1
)

REM Install dependencies if node_modules doesn't exist
if not exist "node_modules" (
    echo Installing dependencies...
    npm install
    if %ERRORLEVEL% NEQ 0 (
        echo.
        echo Failed to install dependencies!
        pause
        exit /b 1
    )
)

echo.
echo Starting Qu Studio in development mode...
echo.
echo Press Ctrl+C to stop
echo.
npm run tauri dev
