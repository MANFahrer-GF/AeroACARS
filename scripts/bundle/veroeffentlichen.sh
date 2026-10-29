#!/bin/sh
# Laufzeit-Paket auf live veroeffentlichen (AeroACARS 3.0, P-3.12, P-3.13).
# Aufruf im Paketordner auf live:
#   sh veroeffentlichen.sh vorab <G>      tmp-gen-<G>/ -> gen/<G>, vorab.json = G
#   sh veroeffentlichen.sh freigeben <G>  aktuell.json = G (gen/<G> muss liegen)
# Erwartet bei "vorab": blobs/ schon hochgeladen, Manifest + Signatur in
# tmp-gen-<G>/. Unter flock: nur eine Veroeffentlichung gleichzeitig. Nur
# steigende Generationen; gen/<G> wird nie ueberschrieben; Zeiger werden als
# letzter Schritt atomar ersetzt.
set -eu
STUFE="${1:-}"
G="${2:-}"
case "$G" in ''|*[!0-9]*) echo "Generation ungueltig: $G" >&2; exit 2 ;; esac
exec 9>veroeffentlichen.lock
flock 9

gen_aus() {
  sed -n 's/.*"generation":\([0-9][0-9]*\).*/\1/p' "$1" 2>/dev/null || true
}
zeiger() {
  printf '{"generation":%s}' "$G" > "$1.neu"
  mv "$1.neu" "$1"
}
AKTIV=$(gen_aus aktuell.json); AKTIV=${AKTIV:-0}

case "$STUFE" in
  vorab)
    VORAB=$(gen_aus vorab.json); VORAB=${VORAB:-0}
    if [ "$G" -le "$AKTIV" ] || [ "$G" -le "$VORAB" ]; then
      echo "Generation $G ist nicht groesser als aktiv $AKTIV / vorab $VORAB — Abbruch" >&2
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
    zeiger vorab.json
    echo "Generation $G als Vorabstand abgelegt"
    ;;
  freigeben)
    if [ "$G" -le "$AKTIV" ]; then
      echo "Generation $G ist nicht groesser als die aktive $AKTIV — Abbruch" >&2
      exit 1
    fi
    if ! test -f "gen/$G/manifest.json" || ! test -f "gen/$G/manifest.json.minisig"; then
      echo "gen/$G fehlt — erst als Vorabstand ablegen" >&2
      exit 1
    fi
    zeiger aktuell.json
    echo "Generation $G fuer alle freigegeben"
    ;;
  *)
    echo "Stufe unbekannt: $STUFE (vorab|freigeben)" >&2
    exit 2
    ;;
esac
