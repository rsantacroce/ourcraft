#!/usr/bin/env bash
# Build every mod to WebAssembly and copy it to dist/, where the game finds
# it (and bundles it into the game binary, so phones and tablets get it too).
# Needs: rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p dist
for f in target/wasm32-unknown-unknown/release/*.wasm; do
  cp "$f" dist/
  echo "built dist/$(basename "$f") ($(wc -c < "$f" | tr -d ' ') bytes)"
done
