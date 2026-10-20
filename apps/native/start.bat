@echo off
setlocal
cd /d "%~dp0"
if exist "dist\sysu_netauth\sysu_netauth_gui.exe" (
  start "" "dist\sysu_netauth\sysu_netauth_gui.exe" %*
  exit /b 0
)
if exist "..\..\target\x86_64-pc-windows-msvc\release\sysu_netauth_gui.exe" (
  start "" "..\..\target\x86_64-pc-windows-msvc\release\sysu_netauth_gui.exe" %*
  exit /b 0
)
echo Build the native client first: powershell -File "%~dp0scripts\build.ps1" -SkipInstaller
exit /b 1
