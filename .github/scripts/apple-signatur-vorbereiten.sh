#!/usr/bin/env bash
# Apple-Signatur + Notarisierung fuer den macOS-Build vorbereiten.
#
# Liest die Repo-Secrets (als Umgebungsvariablen *_SECRET hereingereicht) und
# gibt sie in der Form weiter, die Tauri erwartet — ueber $GITHUB_ENV an die
# folgenden Schritte. Fehlt eines, bleibt der Build UNSIGNIERT wie bis v1.9.14
# (kein Fehler): ein Release darf nicht daran scheitern, dass die Schluessel
# noch nicht eingetragen sind.
#
# Tauri (tauri-bundler) signiert die .app mit APPLE_CERTIFICATE /
# APPLE_SIGNING_IDENTITY, notarisiert sie mit dem App-Store-Connect-API-
# Schluessel (APPLE_API_ISSUER / APPLE_API_KEY / APPLE_API_KEY_PATH) und
# heftet das Ergebnis an (staple). Siehe docs/macos-signierung.md.
#
# Setzt ausserdem AEROACARS_APPLE_SIGNIERT=1|0 fuer den Pruefschritt danach.
set -euo pipefail

fehlt=()
for v in APPLE_CERTIFICATE_SECRET APPLE_CERTIFICATE_PASSWORD_SECRET \
         APPLE_SIGNING_IDENTITY_SECRET APPLE_API_ISSUER_SECRET \
         APPLE_API_KEY_SECRET APPLE_API_KEY_P8_SECRET; do
  [[ -n "${!v:-}" ]] || fehlt+=("${v%_SECRET}")
done

if (( ${#fehlt[@]} > 0 )); then
  if (( ${#fehlt[@]} < 6 )); then
    # Teilweise eingetragen: das ist fast sicher ein Versehen — laut sagen,
    # aber nicht abbrechen (unsigniert ist der bisherige, funktionierende Weg).
    echo "::warning::Apple-Signatur UNVOLLSTAENDIG eingerichtet, es fehlen: ${fehlt[*]} — Build bleibt unsigniert"
  else
    echo "Apple-Secrets nicht eingetragen — Build bleibt unsigniert (wie bisher)."
  fi
  echo "AEROACARS_APPLE_SIGNIERT=0" >> "$GITHUB_ENV"
  exit 0
fi

# Der API-Schluessel muss als Datei vorliegen (APPLE_API_KEY_PATH).
p8="$RUNNER_TEMP/AuthKey_${APPLE_API_KEY_SECRET}.p8"
printf '%s\n' "$APPLE_API_KEY_P8_SECRET" > "$p8"
chmod 600 "$p8"
if ! grep -q 'BEGIN PRIVATE KEY' "$p8"; then
  echo "::error::APPLE_API_KEY_P8 ist kein .p8-Schluessel (erwartet: Inhalt der Datei AuthKey_….p8 mit 'BEGIN PRIVATE KEY')"
  exit 1
fi

# Das Zertifikat muss sich als Base64 lesen lassen — sonst scheitert Tauri
# spaeter mit einer schwer lesbaren Meldung.
if ! printf '%s' "$APPLE_CERTIFICATE_SECRET" | tr -d '\n\r ' | base64 --decode > "$RUNNER_TEMP/pruef.p12" 2>/dev/null \
   || [[ ! -s "$RUNNER_TEMP/pruef.p12" ]]; then
  echo "::error::APPLE_CERTIFICATE ist kein gueltiges Base64 (erwartet: base64 -i Zertifikat.p12 | pbcopy)"
  exit 1
fi
rm -f "$RUNNER_TEMP/pruef.p12"

if [[ "$APPLE_SIGNING_IDENTITY_SECRET" != "Developer ID Application:"* ]]; then
  echo "::error::APPLE_SIGNING_IDENTITY muss mit 'Developer ID Application:' beginnen (ist: '${APPLE_SIGNING_IDENTITY_SECRET%%:*}:…') — nur dieses Zertifikat taugt fuer die Verteilung ausserhalb des App Store"
  exit 1
fi

{
  echo "APPLE_CERTIFICATE=$(printf '%s' "$APPLE_CERTIFICATE_SECRET" | tr -d '\n\r ')"
  echo "APPLE_CERTIFICATE_PASSWORD=$APPLE_CERTIFICATE_PASSWORD_SECRET"
  echo "APPLE_SIGNING_IDENTITY=$APPLE_SIGNING_IDENTITY_SECRET"
  echo "APPLE_API_ISSUER=$APPLE_API_ISSUER_SECRET"
  echo "APPLE_API_KEY=$APPLE_API_KEY_SECRET"
  echo "APPLE_API_KEY_PATH=$p8"
  echo "AEROACARS_APPLE_SIGNIERT=1"
} >> "$GITHUB_ENV"
echo "Apple-Signatur eingerichtet: ${APPLE_SIGNING_IDENTITY_SECRET}"
