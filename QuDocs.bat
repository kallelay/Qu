@echo off
REM Qu Documentation
REM Opens the Qu language documentation in browser

set "DOCS_PATH=%~dp0docs\index.html"

if exist "%DOCS_PATH%" (
    echo Opening Qu Documentation...
    start "" "%DOCS_PATH%"
) else (
    echo Error: Documentation not found
    echo %DOCS_PATH%
    pause
    exit /b 1
)
