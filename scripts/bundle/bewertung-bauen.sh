#!/bin/sh
# Baut das Bewertungsmodul (P-4.01, P-4.30) reproduzierbar nach
# bundle/module/bewertung.wasm. Pfade werden umgeschrieben, damit Mac und
# CI dieselben Bytes erzeugen (die CI prueft das mit --pruefen).
#   sh scripts/bundle/bewertung-bauen.sh            # bauen und ablegen
#   sh scripts/bundle/bewertung-bauen.sh --pruefen  # bauen, mit Ablage vergleichen
set -eu
# Feste Toolchain: andere rustc-Fassungen erzeugen andere Bytes.
export RUSTUP_TOOLCHAIN=1.96.0
WURZEL=$(cd "$(dirname "$0")/../.." && pwd)
ZIEL="${CARGO_TARGET_DIR:-$WURZEL/client/src-tauri/target}/bewertung-wasm"
CARGO_HOME_PFAD="${CARGO_HOME:-$HOME/.cargo}"
RUSTFLAGS="--remap-path-prefix=$WURZEL=/aeroacars --remap-path-prefix=$CARGO_HOME_PFAD=/cargo -C debuginfo=0 -C codegen-units=1" \
  CARGO_TARGET_DIR="$ZIEL" \
  cargo build --manifest-path "$WURZEL/client/src-tauri/Cargo.toml" -p bewertung-wasm \
  --target wasm32-unknown-unknown --release --locked
NEU="$ZIEL/wasm32-unknown-unknown/release/bewertung_wasm.wasm"
ABLAGE="$WURZEL/bundle/module/bewertung.wasm"
if [ "${1:-}" = "--pruefen" ]; then
  cmp "$NEU" "$ABLAGE" && echo "bewertung.wasm: gleich mit der Ablage"
else
  cp "$NEU" "$ABLAGE"
  echo "bewertung.wasm abgelegt: $(wc -c < "$ABLAGE") Bytes"
fi
