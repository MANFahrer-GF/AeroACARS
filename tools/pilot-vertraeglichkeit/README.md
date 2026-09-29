# Pilot-Verträglichkeit: echter Client gegen neuen Server

Prüft **mit dem echten Client-Code** (Crate `aeroacars-mqtt`), dass ein GSG-Pilot
vom Recorder nie ausgesperrt oder gedrosselt wird — im Normalbetrieb und bei einem
Angriff aus derselben Adresse. Entstanden 29.09.2026 beim Absichern von live.kant.ovh
(Thomas: „Sperrst du die Piloten aus, …"). Vor JEDEM Ausrollen des Recorders laufen lassen.

Was geprüft wird (je 3 Runden): Anmeldung, Navdaten, Boden-Index, Aliase, Backup,
Chat, Bordbuch, Messung, Log-Upload klein + GROSS (>64 KB gepackt), Diagnose-Upload,
fremde Route, Prüfstatus, Flugzeug-Scan (nur `X-API-Key`, ZIP), die anonymen Abrufe
der alten Clients (Skin, Karte, Sektoren, Discord), Dauerlast (8×80 Aufrufe).

Vier Szenarien: bekannter Pilot / neuer Pilot, je ohne und mit Angriff.
Erwartung: 1–3 grün mit 0 Fehlern; Szenario 4 (neuer Pilot + >60 falsche Schlüssel
aus DERSELBEN Adresse) darf 429 liefern — das ist der einzige dokumentierte Rest.

## Ablauf
1. `e2e_server.ts` nach `<aeroacars-live>/recorder/src/__e2e_server.ts` kopieren
   (startet den Recorder auf 127.0.0.1:47831 + eine Attrappe der GSG-Webseite auf :47832;
   NICHT committen).
2. `e2e_recorder.rs` nach `<client>/src-tauri/crates/aeroacars-mqtt/tests/` kopieren.
3. `./lauf-alle.sh` (Umgebungsvariablen `RECORDER_DIR`, `CLIENT_TAURI_DIR` anpassen).
   Ergebnis je Szenario in `$TMPDIR/pilot-vertraeglichkeit/e2e-*.log`.

Nur ein cargo-Prozess gleichzeitig (siehe Gedächtnis: macos-syspolicyd-bremst-cargo).

## Zweiter Lauf: die Inhalte des NEUEN Clients (`lauf-inhalte.sh`)
Testet `live_zugang::hole_inhalt` (Skin, Kartenstil, Sektoren, Discord-ID) gegen den
Recorder, einmal mit `INHALTE_NUR_MIT_ANMELDUNG=1` (Fremde abgewiesen, Pilot mit
Token bekommt alles, falsches Token abgewiesen, Pfad ausserhalb der Liste nie zum
Server) und einmal ohne (Uebergangszeit, beides geht). Gleiche Voraussetzungen wie
oben; braucht nur den Recorder und `cargo test -p aeroacars-app --lib live_zugang`.
