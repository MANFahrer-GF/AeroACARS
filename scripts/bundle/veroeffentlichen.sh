#!/bin/sh
# Laufzeit-Paket auf live aktivieren (AeroACARS 3.0, Pflichtenheft P-3.12).
# Aufruf im Paketordner auf live: sh veroeffentlichen.sh <generation>
# Erwartet: blobs/ schon hochgeladen, Manifest + Signatur in tmp-gen-<G>/.
# Unter flock: nur eine Veroeffentlichung gleichzeitig. Nur steigende
# Generation; gen/<G> wird nie ueberschrieben; der Zeiger aktuell.json wird
# als letzter Schritt atomar ersetzt.
set -eu
G="$1"
case "$G" in ''|*[!0-9]*) echo "Generation ungueltig: $G" >&2; exit 2 ;; esac
exec 9>veroeffentlichen.lock
flock 9
AKTIV=$(sed -n 's/.*"generation":\([0-9][0-9]*\).*/\1/p' aktuell.json 2>/dev/null || true)
AKTIV=${AKTIV:-0}
if [ "$G" -le "$AKTIV" ]; then
  echo "Generation $G ist nicht groesser als die aktive $AKTIV — Abbruch" >&2
  exit 1
fi
if ! test -f "tmp-gen-$G/manifest.json" || ! test -f "tmp-gen-$G/manifest.json.minisig"; then
  echo "tmp-gen-$G unvollstaendig — Abbruch" >&2
  exit 1
fi
if [ -e "gen/$G" ]; then
  echo "gen/$G existiert schon — Abbruch" >&2
  exit 1
fi
mkdir -p gen
mv "tmp-gen-$G" "gen/$G"
printf '{"generation":%s}' "$G" > aktuell.json.neu
mv aktuell.json.neu aktuell.json
echo "Generation $G aktiv"
