#!/usr/bin/env sh
set -eu

cargo build --release --target wasm32-unknown-unknown --lib
mkdir -p web
cp target/wasm32-unknown-unknown/release/ascii_racer.wasm web/ascii_racer.wasm
echo "Built web/ascii_racer.wasm"
