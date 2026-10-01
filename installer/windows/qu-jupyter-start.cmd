@echo off
rem Start Jupyter (Qu) -- the Start-menu shortcut both Qu installers create
rem when the "Jupyter kernel" component is chosen. The installer has
rem already registered the Qu kernel (qu-jupyter install), so it appears in
rem JupyterLab's and Notebook's kernel picker. Jupyter itself is a Python
rem program the installer does not ship; without it this says how to get it
rem instead of flashing a window shut.
title Start Jupyter (Qu)
cd /d "%USERPROFILE%"

where jupyter >nul 2>nul
if errorlevel 1 goto no_jupyter

echo Starting JupyterLab -- pick the "Qu" kernel for a new notebook.
echo Close this window (or press Ctrl+C) to stop the server.
jupyter lab
if not errorlevel 1 goto done
echo.
echo JupyterLab did not start; trying the classic Notebook instead.
jupyter notebook
if not errorlevel 1 goto done
echo.
echo Jupyter is installed but neither JupyterLab nor Notebook started.
pause
goto done

:no_jupyter
echo Jupyter is not installed on this machine.
echo.
echo The Qu kernel is already registered: install Jupyter once, for example
echo     py -m pip install jupyterlab
echo and this shortcut starts JupyterLab with "Qu" in its kernel list.
echo VS Code's Jupyter extension finds the same kernel too.
echo.
pause

:done
