# AeroACARS-Schalterprüfung

Kleines Windows-Programm, das herausfindet, welche Simulator-Variable (LVar)
zu welchem Cockpitschalter gehört. Du legst nur die Schalter um, die das
Programm dir nennt — den Rest erledigt es. Es **liest nur** und verstellt
nichts im Simulator.

## Voraussetzungen

- MSFS 2020 oder 2024 läuft, ein Flug ist geladen und du sitzt im Cockpit.
- Das **MobiFlight-Event-Module** (Ordner `mobiflight-event-module`) liegt im
  Community-Ordner. Nach dem Einlegen MSFS einmal neu starten.
- Keine weitere Installation nötig: Die Datei
  `AeroACARS-Schalterpruefung.exe` bringt alles mit.

## Start

1. Die Exe in einen beliebigen Ordner legen (z. B. den freigegebenen Ordner
   `MSFS24_Community`).
2. Doppelklick.
3. **Windows-SmartScreen:** Die Exe ist nicht signiert. Erscheint „Der
   Computer wurde durch Windows geschützt", auf **„Weitere Informationen"**
   klicken, dann **„Trotzdem ausführen"**.

## Was passiert

1. Das Programm verbindet sich mit MSFS und zeigt das geladene Flugzeug.
2. Es prüft, ob das MobiFlight-Modul antwortet und holt die LVar-Liste.
   MobiFlight listet höchstens 1000 LVars; für A380/A350/A330 (iniBuilds)
   und A220 (Synaptic) meldet das Programm deshalb zusätzlich eingebaute
   Namenslisten (`namen/`) an. Namen, die es im Flugzeug nicht gibt, bleiben
   einfach 0.
3. Es wählt anhand des Flugzeugs ein Prüfskript (iniBuilds A380, A350,
   A330, Synaptic A220 oder ein allgemeines Skript) — du kannst auch selbst
   wählen.
4. **Ruhemessung:** 5 Sekunden nichts anfassen. So erkennt das Programm Werte,
   die von allein zappeln.
5. Dann Schalter für Schalter: „Stell jetzt … auf … und drück Enter." Nach
   jedem Schalter zeigt es, welche Variable passt, z. B.
   `L:… → OFF=0, AUTO=1, ON=2`.
   - **Enter** = weiter, **W** = diesen Schalter nochmal messen,
     **F** = freier Modus, **S** (bei einer Stellung) = Schalter überspringen.
6. **Freier Modus:** beliebigen Schalter umlegen, Enter — das Programm zeigt
   sofort, welche Werte sich geändert haben (alt → neu). **Z** = zurück.
   Wird am Ende des Skripts automatisch angeboten.
7. Beenden: am Ende Enter — oder jederzeit **Strg+C** bzw. Fenster
   schließen. Das bisher Gemessene wird in jedem Fall gespeichert.

## Wo der Bericht landet

Im Unterordner **`aeroacars-messung`** neben der Exe (liegt die Exe in
`MSFS24_Community`, ist er vom Mac aus unter
`/Volumes/MSFS24_Community/aeroacars-messung/` zu sehen):

- `<Flugzeug>-<Datum_Uhrzeit>.txt` — lesbarer Bericht
- `<Flugzeug>-<Datum_Uhrzeit>.json` — alle Daten maschinenlesbar
- `live-<Flugzeug>-<Datum_Uhrzeit>.jsonl` — Live-Protokoll, eine Zeile je
  Ereignis, sofort geschrieben (zum Mitlesen mit `tail -f`)

Inhalt: Flugzeug, ICAO, Zeit, MobiFlight-Version, Anzahl LVars, je Schalter
die Kandidaten mit Werten je Stellung, alle Ergebnisse des freien Modus und
der Wertverlauf **jeder** Variable, die sich irgendwann gegenüber der
Ruhemessung geändert hat.

Kann neben der Exe nicht geschrieben werden, weicht das Programm auf den
aktuellen Ordner bzw. den Temp-Ordner aus und nennt den Pfad.

## Für Entwickler

- Bau: GitHub Actions `.github/workflows/schalterpruefung.yml`
  (Artefakt `AeroACARS-Schalterpruefung`).
- Auf dem Mac/Linux laufen die Logik-Tests: `cargo test`
  in `tools/schalterpruefung`.
- SimConnect-SDK: das im Client vendorte
  (`client/src-tauri/crates/sim-msfs/ffi/`), nur gelesen. `SimConnect.dll`
  wird in die Exe eingebettet und verzögert geladen.
- MobiFlight-Protokoll: siehe Kopfkommentar in `src/mobiflight.rs`
  (Quellzeilen aus `MobiFlight-WASM-Module` 1.0.1, `Module.cpp`).
