#!/usr/bin/env bash
# X-Plane-Plugin (mac.xpl) signieren + notarisieren.
#
# Mit eingerichteten Apple-Secrets: Developer-ID-Signatur mit Hardened
# Runtime und Zeitstempel, danach Notarisierung bei Apple. Eine .xpl ist eine
# lose Bibliothek, kein Bundle — anheften (staple) geht dafuer nicht; macOS
# fragt die Notarisierung beim ersten Laden online nach.
#
# Ohne Secrets: ad-hoc-Signatur wie bis v1.9.14 (ADR-0004 §8); der Client
# entfernt beim Installieren die Quarantaene.
#
# $1 = Pfad zur mac.xpl
set -euo pipefail
xpl="$1"

fehlt=0
for v in APPLE_CERTIFICATE_SECRET APPLE_CERTIFICATE_PASSWORD_SECRET \
         APPLE_SIGNING_IDENTITY_SECRET APPLE_API_ISSUER_SECRET \
         APPLE_API_KEY_SECRET APPLE_API_KEY_P8_SECRET; do
  [[ -n "${!v:-}" ]] || fehlt=1
done

if (( fehlt )); then
  echo "Apple-Secrets nicht (vollstaendig) eingetragen — ad-hoc-Signatur wie bisher."
  codesign --force --sign - "$xpl"
  codesign --verify --verbose "$xpl"
  exit 0
fi

# Eigener, kurzlebiger Schluesselbund nur fuer diesen Lauf.
kc="$RUNNER_TEMP/aeroacars-plugin.keychain-db"
kc_pw=$(openssl rand -hex 24)
security create-keychain -p "$kc_pw" "$kc"
security set-keychain-settings -lut 3600 "$kc"
security unlock-keychain -p "$kc_pw" "$kc"
printf '%s' "$APPLE_CERTIFICATE_SECRET" | tr -d '\n\r ' | base64 --decode > "$RUNNER_TEMP/plugin.p12"
security import "$RUNNER_TEMP/plugin.p12" -k "$kc" -P "$APPLE_CERTIFICATE_PASSWORD_SECRET" -T /usr/bin/codesign
rm -f "$RUNNER_TEMP/plugin.p12"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kc_pw" "$kc" > /dev/null
# Den eigenen Bund vorne in die Suchliste, die vorhandenen dahinter.
vorhandene=()
while IFS= read -r z; do
  z="${z//\"/}"; z="${z#"${z%%[![:space:]]*}"}"
  [[ -n "$z" ]] && vorhandene+=("$z")
done < <(security list-keychains -d user)
security list-keychains -d user -s "$kc" "${vorhandene[@]}"

codesign --force --options runtime --timestamp --keychain "$kc" \
  --sign "$APPLE_SIGNING_IDENTITY_SECRET" "$xpl"
codesign --verify --strict --verbose=2 "$xpl"

p8="$RUNNER_TEMP/AuthKey_${APPLE_API_KEY_SECRET}.p8"
printf '%s\n' "$APPLE_API_KEY_P8_SECRET" > "$p8"
chmod 600 "$p8"

zip="$RUNNER_TEMP/mac-xpl-notarisierung.zip"
ditto -c -k --keepParent "$xpl" "$zip"
ergebnis=$(xcrun notarytool submit "$zip" \
  --key "$p8" --key-id "$APPLE_API_KEY_SECRET" --issuer "$APPLE_API_ISSUER_SECRET" \
  --wait --timeout 30m --output-format json)
echo "$ergebnis"
status=$(printf '%s' "$ergebnis" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))')
if [[ "$status" != "Accepted" ]]; then
  id=$(printf '%s' "$ergebnis" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))')
  [[ -n "$id" ]] && xcrun notarytool log "$id" --key "$p8" --key-id "$APPLE_API_KEY_SECRET" --issuer "$APPLE_API_ISSUER_SECRET" || true
  echo "::error::Notarisierung des Plugins nicht angenommen (Status: ${status:-unbekannt})"
  exit 1
fi

security delete-keychain "$kc" || true
rm -f "$p8" "$zip"
echo "OK: mac.xpl signiert und notarisiert."
