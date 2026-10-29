#!/bin/sh
# Baut ein Paketmodul reproduzierbar nach bundle/module/<name>.wasm:
# bewertung (P-4.01, P-4.30) oder kabine (P-3.58). Pfade werden
# umgeschrieben, damit Mac und CI dieselben Bytes erzeugen.
#   sh scripts/bundle/modul-bauen.sh bewertung            # bauen und ablegen
#   sh scripts/bundle/modul-bauen.sh kabine --pruefen     # bauen, Rechnung mit Ablage vergleichen
set -eu
NAME="${1:?Modul: bewertung oder kabine}"
case "$NAME" in
  bewertung) PRUEFTEST="-p bewertung-host --locked frisch_gebautes_modul_rechnet_wie_das_eingecheckte -- --exact tests::frisch_gebautes_modul_rechnet_wie_das_eingecheckte" ;;
  # Release: wasmi unoptimiert braucht fuer sechs Fluege Takt fuer Takt Minuten.
  kabine) PRUEFTEST="-p kabine-wasm --release --locked --test gleich_nativ frisch_gebautes_modul_rechnet_wie_das_eingecheckte -- --exact" ;;
  *) echo "unbekanntes Modul: $NAME" >&2; exit 2 ;;
esac
# Feste Toolchain: andere rustc-Fassungen erzeugen andere Bytes.
export RUSTUP_TOOLCHAIN=1.96.0
WURZEL=$(cd "$(dirname "$0")/../.." && pwd)
ZIEL="${CARGO_TARGET_DIR:-$WURZEL/client/src-tauri/target}/$NAME-wasm"
CARGO_HOME_PFAD="${CARGO_HOME:-$HOME/.cargo}"
RUSTFLAGS="--remap-path-prefix=$WURZEL=/aeroacars --remap-path-prefix=$CARGO_HOME_PFAD=/cargo -C debuginfo=0 -C codegen-units=1" \
  CARGO_TARGET_DIR="$ZIEL" \
  cargo build --manifest-path "$WURZEL/client/src-tauri/Cargo.toml" -p "$NAME-wasm" \
  --target wasm32-unknown-unknown --release --locked
NEU="$ZIEL/wasm32-unknown-unknown/release/${NAME}_wasm.wasm"
ABLAGE="$WURZEL/bundle/module/$NAME.wasm"
if [ "${2:-}" = "--pruefen" ]; then
  # Bytegleich sind Mac- und Linux-Build nicht; geprueft wird, dass das
  # frische Modul alle Faelle wie die Ablage und wie das Crate rechnet.
  FRISCH=$(echo "$NAME" | tr a-z A-Z)_WASM_FRISCH
  # shellcheck disable=SC2086
  env "$FRISCH=$NEU" cargo test --manifest-path "$WURZEL/client/src-tauri/Cargo.toml" $PRUEFTEST
  echo "$NAME.wasm: rechnet wie der Quellstand"
else
  cp "$NEU" "$ABLAGE"
  echo "$NAME.wasm abgelegt: $(wc -c < "$ABLAGE") Bytes"
fi
