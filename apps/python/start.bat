@echo off
chcp 65001 >nul
setlocal
set "PYTHONUTF8=1"
cd /d "%~dp0"

where uv.exe >nul 2>nul
if errorlevel 1 goto :missing_uv

uv run --no-project --python 3.13 --with-requirements requirements.lock python run.py %*
set "app_exit=%errorlevel%"
if not "%app_exit%"=="0" (
  echo.
  echo Application failed to start. See the message above.
  pause
)
exit /b %app_exit%

:missing_uv
echo.
echo ERROR: uv was not found in PATH.
echo Install it with: winget install --id astral-sh.uv --exact
pause
exit /b 1
