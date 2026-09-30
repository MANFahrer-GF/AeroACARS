#!/usr/bin/env bash
# Nach dem Bau: Ist die .app wirklich signiert, notarisiert und angeheftet?
#
# Nur wenn AEROACARS_APPLE_SIGNIERT=1 (siehe apple-signatur-vorbereiten.sh).
# Eine eingerichtete, aber nicht greifende Signatur soll den Lauf ROT machen —
# sonst ginge still eine unsignierte App an die Piloten, obwohl alle glauben,
# sie sei signiert.
#
# $1 = Verzeichnis mit der gebauten .app (…/bundle/macos)
set -euo pipefail

if [[ "${AEROACARS_APPLE_SIGNIERT:-0}" != "1" ]]; then
  echo "Apple-Signatur nicht eingerichtet — keine Pruefung."
  exit 0
fi

app=$(find "$1" -maxdepth 1 -name '*.app' -print -quit)
if [[ -z "$app" ]]; then
  echo "::error::Keine .app in $1"
  exit 1
fi
echo "Pruefe $app"

codesign --verify --deep --strict --verbose=2 "$app"
# Hardened Runtime ist Pflicht fuer die Notarisierung.
if ! codesign -d --verbose=4 "$app" 2>&1 | grep -q 'flags=.*runtime'; then
  echo "::error::Hardened Runtime fehlt"
  exit 1
fi
codesign -d --verbose=4 "$app" 2>&1 | grep -E '^(Authority|TeamIdentifier)=' || true

xcrun stapler validate "$app"

ausgabe=$(spctl --assess --type execute --verbose=4 "$app" 2>&1 || true)
echo "$ausgabe"
if ! grep -q 'source=Notarized Developer ID' <<<"$ausgabe"; then
  echo "::error::Gatekeeper nimmt die App nicht als notarisierte Developer-ID-App an"
  exit 1
fi
echo "OK: signiert, notarisiert, angeheftet."
