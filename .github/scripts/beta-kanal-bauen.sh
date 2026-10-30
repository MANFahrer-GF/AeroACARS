#!/usr/bin/env bash
# Beta-Update-Kanal (AeroACARS 3): stellt aus den Build-Dateien eines
# dev-build-Laufs (Eingang beta=true) das Paket fuer den Kanal zusammen.
#
#   beta-kanal-bauen.sh <rohordner> <zielordner> <plugin-sha256>
#
# <rohordner>: heruntergeladene Artefakte beider Laeufer (beliebig tief).
# <plugin-sha256>: Pruefsumme des X-Plane-Plugin-Pakets, die in die App
# eingebaut wurde (Job plugin-zip) — das Paket hier muss genau sie tragen.
# <zielordner>: bekommt Installer, Update-Dateien (+ .sig) und latest.json mit
# festen, versionierten Namen ohne Leerzeichen — genau die Namen, unter denen
# scripts/beta-veroeffentlichen.sh sie an die Pre-Release v<version> haengt und
# auf die latest.json zeigt. Veroeffentlicht selbst nichts.
#
# Laeuft im Repo-Wurzelverzeichnis (liest tauri.conf.json und die Notes).
set -euo pipefail

ROH=${1:?rohordner fehlt}
ZIEL=${2:?zielordner fehlt}
PLUGIN_SOLL=${3:-}
REPO=MANFahrer-GF/AeroACARS
KONF=client/src-tauri/tauri.conf.json

fehler() { echo "::error::$*" >&2; exit 1; }

VER=$(jq -r .version "$KONF")
NAME=$(jq -r .productName "$KONF")
PUBKEY=$(jq -r .plugins.updater.pubkey "$KONF")
[[ "$VER" == *-* ]] || fehler "Version $VER ist keine Vorab-Version — finale Releases laufen ueber release.yml"
[[ "$PLUGIN_SOLL" =~ ^[0-9a-f]{64}$ ]] || fehler "keine gueltige Plugin-Pruefsumme ('$PLUGIN_SOLL') — ohne sie kein Beta-Paket"
# Die App baut die Plugin-URL aus der Cargo-Version, der Updater vergleicht
# die tauri.conf-Version: alle drei muessen gleich sein (wie release.yml).
CARGO_VER=$(grep -E '^version = "' client/src-tauri/Cargo.toml | head -1 | cut -d'"' -f2)
PKG_VER=$(jq -r .version client/package.json)
[[ "$CARGO_VER" == "$VER" && "$PKG_VER" == "$VER" ]] || fehler "Versionen uneinheitlich: tauri.conf $VER, Cargo $CARGO_VER, package.json $PKG_VER"
NOTES="docs/release-notes/v${VER}.md"
[[ -s "$NOTES" ]] || fehler "$NOTES fehlt — die App zeigt diese Notes im Update-Dialog"

# Genau eine Datei je Muster, sonst ist der Lauf nicht eindeutig.
eine() {
  local treffer
  treffer=$(find "$ROH" -type f -name "$1")
  [[ -n "$treffer" ]] || fehler "keine Datei '$1' in $ROH"
  [[ $(printf '%s\n' "$treffer" | wc -l) -eq 1 ]] || fehler "mehrere Dateien '$1' in $ROH: $treffer"
  printf '%s' "$treffer"
}

EXE=$(eine "${NAME}_${VER}_x64-setup.exe")
EXE_SIG=$(eine "${NAME}_${VER}_x64-setup.exe.sig")
DMG=$(eine "${NAME}_${VER}_aarch64.dmg")
# Tauri nennt das Mac-Updatepaket nur nach dem Produkt, ohne Version/Arch.
TGZ=$(eine "${NAME}.app.tar.gz")
TGZ_SIG=$(eine "${NAME}.app.tar.gz.sig")
PLUGIN="AeroACARS-XPlane-Plugin-v${VER}.zip"
PLUGIN_PFAD=$(eine "$PLUGIN")
PLUGIN_IST=$( (sha256sum "$PLUGIN_PFAD" 2>/dev/null || shasum -a 256 "$PLUGIN_PFAD") | cut -d' ' -f1)
[[ "$PLUGIN_IST" == "$PLUGIN_SOLL" ]] || fehler "Plugin-Paket $PLUGIN hat $PLUGIN_IST, die App erwartet $PLUGIN_SOLL"

# Die Signatur muss vom Schluessel stammen, dessen oeffentlicher Teil in der
# App steckt — sonst lehnt jeder Client das Update ab (Key-ID vergleichen).
schluessel_id() { base64 -d | sed -n 2p | base64 -d | head -c 10 | tail -c 8 | od -An -tx1 | tr -d ' \n'; }
SOLL=$(printf '%s' "$PUBKEY" | schluessel_id)
for s in "$EXE_SIG" "$TGZ_SIG"; do
  IST=$(tr -d '\r\n ' < "$s" | schluessel_id)
  [[ -n "$IST" && "$IST" == "$SOLL" ]] || fehler "Signatur $s passt nicht zum pubkey der App ($IST != $SOLL)"
done

mkdir -p "$ZIEL"
P="${NAME// /.}_${VER}"
cp "$EXE" "$ZIEL/${P}_x64-setup.exe"
cp "$EXE_SIG" "$ZIEL/${P}_x64-setup.exe.sig"
cp "$DMG" "$ZIEL/${P}_aarch64.dmg"
cp "$TGZ" "$ZIEL/${P}_aarch64.app.tar.gz"
cp "$TGZ_SIG" "$ZIEL/${P}_aarch64.app.tar.gz.sig"
cp "$PLUGIN_PFAD" "$ZIEL/$PLUGIN"

BASIS="https://github.com/${REPO}/releases/download/v${VER}"
jq -n \
  --arg version "$VER" \
  --rawfile notes "$NOTES" \
  --arg pub_date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg mac_url "${BASIS}/${P}_aarch64.app.tar.gz" \
  --arg mac_sig "$(tr -d '\r\n ' < "$TGZ_SIG")" \
  --arg win_url "${BASIS}/${P}_x64-setup.exe" \
  --arg win_sig "$(tr -d '\r\n ' < "$EXE_SIG")" \
  '{version: $version, notes: $notes, pub_date: $pub_date, platforms: {
     "darwin-aarch64":      {signature: $mac_sig, url: $mac_url},
     "darwin-aarch64-app":  {signature: $mac_sig, url: $mac_url},
     "windows-x86_64":      {signature: $win_sig, url: $win_url},
     "windows-x86_64-nsis": {signature: $win_sig, url: $win_url}
   }}' > "$ZIEL/latest.json"

echo "Beta-Kanal-Paket ${VER}:"
ls -l "$ZIEL"
