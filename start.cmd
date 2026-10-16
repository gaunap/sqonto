@echo off
rem Copyright (C) 2026 Johannes Asal
rem SPDX-License-Identifier: AGPL-3.0-or-later
rem See LICENSE in the project root for the full license and warranty disclaimer.

rem Startet Sqonto. Baut die Anwendung beim ersten Aufruf.
setlocal
cd /d "%~dp0"
if not exist dist\Sqonto.exe (
  call build.cmd || exit /b 1
)
start "Sqonto" dist\Sqonto.exe
