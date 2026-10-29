#!/bin/sh
# Erzwungener SSH-Befehl fuer den Benutzer `bundle` auf live (P-3.12, P-3.13).
# In ~bundle/.ssh/authorized_keys:
#   command="/home/bundle/ssh-befehl.sh vorab",restrict ssh-ed25519 AAAA… ci-vorab
#   command="/home/bundle/ssh-befehl.sh freigeben",restrict ssh-ed25519 AAAA… ci-freigabe
# Der Vorab-Schluessel darf nur hochladen (rsync in die Ablage) und als
# Vorabstand ablegen; der Freigabe-Schluessel (nur im geschuetzten
# GitHub-Environment) darf nur freigeben. So laesst sich die Freigabe durch
# Thomas nicht mit dem Vorab-Schluessel umgehen (Codex 29.10.2026).
set -euf
ROLLE="$1"
BEFEHL="${SSH_ORIGINAL_COMMAND:-}"
cd "$HOME/ablage"
case "$ROLLE:$BEFEHL" in
  vorab:"rsync --server "*)
    # rrsync (aus dem rsync-Paket) prueft Optionen und Pfade selbst: nur
    # Schreiben, nur innerhalb der Ablage.
    exec /usr/bin/rrsync -wo "$HOME/ablage"
    ;;
  vorab:"veroeffentlichen vorab "*)
    exec sh "$HOME/veroeffentlichen.sh" vorab "${BEFEHL#veroeffentlichen vorab }"
    ;;
  freigeben:"veroeffentlichen freigeben "*)
    exec sh "$HOME/veroeffentlichen.sh" freigeben "${BEFEHL#veroeffentlichen freigeben }"
    ;;
esac
echo "nicht erlaubt: $BEFEHL" >&2
exit 1
