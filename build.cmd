@echo off
rem Baut Sqonto (Frontend + Backend) und legt die fertigen Programme unter dist\ ab:
rem   dist\Sqonto.exe         Desktop-Anwendung (eigenes Fenster)
rem   dist\sqonto-server.exe  Server ohne Fenster (Browser / Entwicklung)
setlocal
cd /d "%~dp0"
if not "%~1"=="" if not "%~1"=="--no-installer" (
  echo Usage: build.cmd [--no-installer]
  exit /b 1
)
node scripts\version.mjs || goto :error

if "%SQONTO_FRONTEND_READY%"=="1" (
  if not exist frontend\dist\index.html (
    echo SQONTO_FRONTEND_READY requires an already built frontend/dist/index.html.
    goto :error
  )
  echo [1-2/3] Vorbereitetes Frontend verwenden ...
) else (
  echo [1/3] Frontend-Abhaengigkeiten ...
  call npm --prefix frontend ci --no-audit --no-fund || goto :error
  echo [2/3] Frontend bauen ...
  call npm --prefix frontend run build || goto :error
)

echo [3/3] Backend bauen (Release) ...
cargo build --locked --release --manifest-path backend\Cargo.toml --bins || goto :error

if not exist dist mkdir dist
copy /y backend\target\release\sqonto-desktop.exe dist\Sqonto.exe >nul || goto :error
copy /y backend\target\release\sqonto-server.exe dist\sqonto-server.exe >nul || goto :error
copy /y LICENSE dist\LICENSE.txt >nul || goto :error
node scripts\third-party-licenses.mjs dist\THIRD-PARTY-LICENSES.txt || goto :error
if not "%~1"=="--no-installer" (
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-installer.ps1 || goto :error
)

echo.
echo Fertig: dist\Sqonto.exe  (Start per Doppelklick oder start.cmd)
exit /b 0

:error
echo.
echo Build fehlgeschlagen.
exit /b 1
