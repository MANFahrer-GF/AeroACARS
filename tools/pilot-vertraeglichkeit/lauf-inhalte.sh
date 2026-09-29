#!/bin/zsh
S=${TMPDIR:-/tmp}/pilot-vertraeglichkeit; mkdir -p $S
R=${RECORDER_DIR:-$HOME/Claude/aeroacars-live/recorder}
C=${CLIENT_TAURI_DIR:-$PWD/../../client/src-tauri}
cp $(dirname $0)/e2e_server.ts $R/src/__e2e_server.ts
# Auch bei Abbruch (Strg-C, Fehler) aufraeumen — die Datei darf nicht im
# Recorder-Baum liegen bleiben und versehentlich mitgehen.
trap 'pkill -f "__e2e_server" 2>/dev/null; rm -f $R/src/__e2e_server.ts' EXIT INT TERM
lauf() { # name, server-env..., --, test-env...
  local name=$1; shift; local senv=(); while [ "$1" != "--" ]; do senv+=("$1"); shift; done; shift
  pkill -f "__e2e_server" 2>/dev/null; sleep 1
  ( cd $R && env SESSION_SECRET=12345678901234567890123456789012 MQTT_USERNAME=t MQTT_PASSWORD=t E2E_BEKANNT=1 "${senv[@]}" nohup npx tsx src/__e2e_server.ts > $S/inh-server-$name.log 2>&1 & )
  for i in {1..40}; do curl -s -m 1 -o /dev/null http://127.0.0.1:47831/api/healthz && break; sleep 0.5; done
  ( cd $C && env E2E_BASE=http://127.0.0.1:47831 E2E_TOKEN=e2e-pilot-passwort-0123456789ab "$@" cargo test -p aeroacars-app --lib live_zugang -- --nocapture > $S/inh-$name.log 2>&1; echo "EXIT $?" >> $S/inh-$name.log )
}
lauf schalter-an INHALTE_NUR_MIT_ANMELDUNG=1 -- E2E_SCHALTER_AN=1
lauf schalter-aus X=1 -- X=1
pkill -f "__e2e_server" 2>/dev/null
rm -f $R/src/__e2e_server.ts
echo fertig > $S/inh-alle.done
