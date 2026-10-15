#!/usr/bin/env sh
# Baut Sqonto (Frontend + Backend) und legt die fertigen Programme unter dist/ ab:
#   dist/sqonto          Desktop-Anwendung (eigenes Fenster)
#   dist/sqonto-server   Server ohne Fenster (Browser / Entwicklung)
#
# Linux: die Desktop-Anwendung benötigt webkit2gtk-4.1 und libgtk-3 (Entwicklungspakete).
# Nur den Server bauen:  SQONTO_SERVER_ONLY=1 ./build.sh
set -e
cd "$(dirname "$0")"
version=$(node scripts/version.mjs)

echo "[1/3] Frontend-Abhängigkeiten …"
npm --prefix frontend ci --no-audit --no-fund

echo "[2/3] Frontend bauen …"
npm --prefix frontend run build

echo "[3/3] Backend bauen (Release) …"
mkdir -p dist
if [ -n "$SQONTO_SERVER_ONLY" ]; then
  cargo build --locked --release --manifest-path backend/Cargo.toml --no-default-features --bin sqonto-server
  files="sqonto-server LICENSE.txt THIRD-PARTY-LICENSES.txt"
  variant="-server"
else
  cargo build --locked --release --manifest-path backend/Cargo.toml --bins
  cp backend/target/release/sqonto-desktop dist/sqonto
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
