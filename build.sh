#!/usr/bin/env sh
# Copyright (C) 2026 Johannes Asal
# SPDX-License-Identifier: AGPL-3.0-or-later
# See LICENSE in the project root for the full license and warranty disclaimer.

# Baut Sqonto (Frontend + Backend) und legt die fertigen Programme unter dist/ ab:
#   dist/sqonto          Desktop-Anwendung (eigenes Fenster)
#   dist/sqonto-server   Server ohne Fenster (Browser / Entwicklung)
#
# Linux: die Desktop-Anwendung benötigt webkit2gtk-4.1 und libgtk-3 (Entwicklungspakete).
# Nur den Server bauen:  SQONTO_SERVER_ONLY=1 ./build.sh
set -e
cd "$(dirname "$0")"
version=$(node scripts/version.mjs)

if [ "${SQONTO_FRONTEND_READY:-}" = "1" ]; then
  if [ ! -f frontend/dist/index.html ]; then
    echo "SQONTO_FRONTEND_READY requires an already built frontend/dist/index.html." >&2
    exit 1
  fi
  echo "[1-2/3] Vorbereitetes Frontend verwenden …"
else
  echo "[1/3] Frontend-Abhängigkeiten …"
  npm --prefix frontend ci --no-audit --no-fund
  echo "[2/3] Frontend bauen …"
  npm --prefix frontend run build
fi

echo "[3/3] Backend bauen (Release) …"
mkdir -p dist
if [ -n "$SQONTO_SERVER_ONLY" ]; then
  cargo build --locked --release --manifest-path backend/Cargo.toml --no-default-features --bin sqonto-server
  files="sqonto-server LICENSE.txt THIRD-PARTY-LICENSES.txt"
  variant="-server"
else
  cargo build --locked --release --manifest-path backend/Cargo.toml --bins
  cp backend/target/release/sqonto-desktop dist/sqonto
  if [ "$(uname -s)" = "Darwin" ]; then
    # Sqonto.app: Dock, Launchpad and Finder take the icon from the bundle. The data lives in
    # ~/Library/Application Support/Sqonto, because the bundle itself is replaced on updates.
    app=dist/Sqonto.app
    rm -rf "$app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp backend/target/release/sqonto-desktop "$app/Contents/MacOS/sqonto"
    cp backend/assets/icons/sqonto.icns "$app/Contents/Resources/sqonto.icns"
    cat > "$app/Contents/MacOS/Sqonto-launcher" <<'LAUNCHER'
#!/bin/sh
dir=$(cd "$(dirname "$0")" && pwd)
export SQONTO_DATA_DIR="${SQONTO_DATA_DIR:-$HOME/Library/Application Support/Sqonto}"
exec "$dir/sqonto" "$@"
LAUNCHER
    chmod +x "$app/Contents/MacOS/Sqonto-launcher"
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Sqonto</string>
  <key>CFBundleDisplayName</key><string>Sqonto</string>
  <key>CFBundleIdentifier</key><string>de.sqonto.app</string>
  <key>CFBundleExecutable</key><string>Sqonto-launcher</string>
  <key>CFBundleIconFile</key><string>sqonto</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
  fi
  files="sqonto sqonto-server LICENSE.txt THIRD-PARTY-LICENSES.txt"
  variant=""
fi
cp backend/target/release/sqonto-server dist/sqonto-server
cp LICENSE dist/LICENSE.txt
node scripts/third-party-licenses.mjs dist/THIRD-PARTY-LICENSES.txt
mkdir -p dist/packages
platform=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
# Only ship explicitly named files, never dist/data or old packages.
tar -czf "dist/packages/Sqonto-$version-$platform-$arch$variant.tar.gz" -C dist $files
echo
echo "Fertig: dist/"
