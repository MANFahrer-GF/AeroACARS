#!/bin/sh
# Einmalig: Benutzer `bundle` auf live fuer die Ablage der Laufzeit-Pakete
# (AeroACARS 3.0, P-3.12, P-3.13). Wird vom Mac aus gestartet:
#
#   sh ~/Claude/aeroacars-v3/scripts/bundle/live-einrichten.sh
#
# Was es tut (nur hinzufuegend, kein Neustart, kein laufender Dienst
# beruehrt):
#   - Systembenutzer `bundle` ohne Passwort
#   - Ablage /var/lib/aeroacars-recorder/bundles (gehoert bundle, lesbar
#     fuer den Recorder), Verweis ~bundle/ablage
#   - ssh-befehl.sh und veroeffentlichen.sh root-eigen nach /home/bundle
#   - zwei Schluessel in authorized_keys (root-eigen): der Vorab-Schluessel
#     darf nur hochladen und als Vorabstand ablegen, der Freigabe-Schluessel
#     nur freigeben (erzwungener Befehl, `restrict`)
# Wiederholbar: Was schon da ist, bleibt.
set -eu
HIER=$(cd "$(dirname "$0")" && pwd)
SCHLUESSEL="$HOME/.aeroacars-paket-schluessel"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
{
  echo "command=\"/home/bundle/ssh-befehl.sh vorab\",restrict $(cat "$SCHLUESSEL/ssh-vorab.pub")"
  echo "command=\"/home/bundle/ssh-befehl.sh freigeben\",restrict $(cat "$SCHLUESSEL/ssh-freigabe.pub")"
} > "$TMP/authorized_keys"
scp -q "$HIER/ssh-befehl.sh" "$HIER/veroeffentlichen.sh" "$TMP/authorized_keys" live:/root/
ssh live 'set -e
id bundle >/dev/null 2>&1 || useradd --system --create-home --home-dir /home/bundle --shell /bin/sh bundle
install -d -o bundle -g bundle -m 755 /var/lib/aeroacars-recorder/bundles
[ -L /home/bundle/ablage ] || ln -s /var/lib/aeroacars-recorder/bundles /home/bundle/ablage
install -o root -g root -m 755 /root/ssh-befehl.sh /home/bundle/ssh-befehl.sh
install -o root -g root -m 755 /root/veroeffentlichen.sh /home/bundle/veroeffentlichen.sh
install -d -o root -g root -m 755 /home/bundle/.ssh
install -o root -g root -m 644 /root/authorized_keys /home/bundle/.ssh/authorized_keys
rm -f /root/ssh-befehl.sh /root/veroeffentlichen.sh /root/authorized_keys
echo "fertig:"; id bundle; ls -ld /var/lib/aeroacars-recorder/bundles /home/bundle/ablage'
