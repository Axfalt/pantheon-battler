#!/usr/bin/env bash
# Construit le moteur WebAssembly de la page web/, l'intègre en base64 dans
# web/engine-wasm.js, puis vérifie que la page et la CLI jouent exactement les mêmes combats.
# Usage : scripts/build-web.sh        (Git Bash sous Windows, ou tout shell POSIX avec bash)
set -euo pipefail
cd "$(dirname "$0")/.."

if ! rustup target list --installed | grep -qx 'wasm32-unknown-unknown'; then
  echo "erreur : cible WebAssembly absente. Installez-la avec :" >&2
  echo "  rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi

echo "== Moteur WebAssembly"
cargo build -p pantheon-wasm --target wasm32-unknown-unknown --profile wasm
cp target/wasm32-unknown-unknown/wasm/pantheon_wasm.wasm web/engine.wasm

echo "== CLI (pour la vérification)"
cargo build --release

echo "== Intégration dans web/engine-wasm.js"
export PANTHEON_GIT="${PANTHEON_GIT:-$(git describe --always --dirty 2>/dev/null || echo non-versionné)}"
node scripts/embed-wasm.mjs

echo "== Vérification page = CLI"
node scripts/verify-web.mjs
