@echo off
REM Qu Test Suite - Run all tests
REM Runs the complete test suite for the Qu engine

echo ========================================
echo   Qu Test Suite
echo ========================================
echo.

cd /d "%~dp0engine"

echo Running all tests...
echo.

cargo test --lib

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo   All tests PASSED
    echo ========================================
) else (
    echo.
    echo ========================================
    echo   Some tests FAILED
    echo ========================================
)

echo.
pause
