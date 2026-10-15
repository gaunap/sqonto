@echo off
rem Startet Sqonto. Baut die Anwendung beim ersten Aufruf.
setlocal
cd /d "%~dp0"
if not exist dist\Sqonto.exe (
  call build.cmd || exit /b 1
)
start "Sqonto" dist\Sqonto.exe
