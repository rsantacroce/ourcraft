#!/usr/bin/env bash
# Build the EELisp runner and copy it to mods/runtimes/eelisp.wasm, where
# the game finds it (and bundles it, so phones and tablets get it too).
# Needs: rustup target add wasm32-unknown-unknown, and eelisp-rs checked
# out next to this repo (../eelisp-rs from the repo root).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p ../runtimes
cp target/wasm32-unknown-unknown/release/eelisp_runner.wasm ../runtimes/eelisp.wasm
echo "built mods/runtimes/eelisp.wasm ($(wc -c < ../runtimes/eelisp.wasm | tr -d ' ') bytes)"
