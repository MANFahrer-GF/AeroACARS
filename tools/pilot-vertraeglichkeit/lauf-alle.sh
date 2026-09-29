#!/bin/zsh
S=${TMPDIR:-/tmp}/pilot-vertraeglichkeit; mkdir -p $S
R=${RECORDER_DIR:-$HOME/Claude/aeroacars-live/recorder}
C=${CLIENT_TAURI_DIR:-$HOME/Claude/aeroacars-src/client/src-tauri}
szenario() { # name bekannt(0/1) extra-env...
  local name=$1 bekannt=$2; shift 2
  pkill -f "__e2e_server" 2>/dev/null; sleep 1
  ( cd $R && SESSION_SECRET=12345678901234567890123456789012 MQTT_USERNAME=t MQTT_PASSWORD=t E2E_BEKANNT=$bekannt nohup npx tsx src/__e2e_server.ts > $S/e2e-server-$name.log 2>&1 & )
  for i in {1..40}; do curl -s -m 1 -o /dev/null http://127.0.0.1:47831/api/healthz && break; sleep 0.5; done
  ( cd $C && env E2E_BASE=http://127.0.0.1:47831 "$@" cargo test -p aeroacars-mqtt --test e2e_recorder -- --nocapture > $S/e2e-$name.log 2>&1; echo "EXIT $?" >> $S/e2e-$name.log )
}
szenario 1-normal-bekannt 1
szenario 2-angriff-bekannt 1 E2E_ANGRIFF=1
szenario 3-normal-neu 0
szenario 4-angriff-neu 0 E2E_ANGRIFF=1 E2E_ERWARTE_GESPERRT=1
pkill -f "__e2e_server" 2>/dev/null
echo ALLE_FERTIG > $S/e2e-alle.done
