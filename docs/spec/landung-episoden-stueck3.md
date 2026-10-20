# Landungskante: Wegsteigen nach dem Aufsetzen („Stück 3“, 20.10.2026)

## Anlass

EWG 1599 (Pilot 15, Client 2.1.4, A321, MSFS): Aufsetzer −667 fpm / 1,71 G,
26 s Bodenlauf, Durchstart, 100 ft erst 35,9 s nach dem Aufsetzen, Platzrunde,
Landung −55 fpm. Live zeigte −667, phpVMS −55, die Gesamtnote mischte beide
Aufsetzer, und die Phase stand 15 Minuten in 4000 ft auf `Landing`.

Ursachen (Claude + Codex, Code 2.1.4):

1. Der Touch-and-Go-Erkenner im `Landing`-Arm schaute nur 30 s ab dem
   Aufsetzen hin. Ein langer Bodenlauf fraß das Fenster auf.
2. `Landing` hatte keinen Ausgang zurück in die Luft (nur `TaxiIn`).
3. Zwei Hälften schlossen dieselbe Landung ab: Der Aufsetz-Sampler setzte
   beim Wegsteigen seine Messfelder zurück und erfasste den zweiten
   Aufsetzer; Note, Live-Meldung und Phase blieben beim ersten.
4. v2 spiegelt außerhalb des Reisebands die Phase von v1 — es sah den
   Steigflug, wurde aber überstimmt.

## Wer entscheidet

**Das Wegsteigen nach einem Aufsetzen** entscheidet eine einzige Bedingung,
`wegsteigen_nach_aufsetzen` (lib.rs): eine offene Landung (Phasenlogik oder
Sampler), eine Phase, in der gelandet werden kann, `echter_steigflug_nach_aufsetzen`
(über 100 ft, Position und Gelände plausibel), die Auswertung des Aufsetzers
liegt vor — und das eine Sekunde am Stück. **Keine Zeitgrenze ab dem
Aufsetzen, kein Triebwerks-Riegel.** Das ist das Signal, mit dem der Sampler
sein Wegsteigen schon immer erkannt hat; es gibt keinen dritten Erkenner.

**Abgeschlossen** wird an einer Stelle, `landung_beim_wegsteigen_abschliessen`:
Eintrag des Aufsetzers sichern (Kanonik-Sinkrate, G, Ort), einen vorläufig
verbuchten `FinalLanding` in `TouchAndGo` umwandeln, Durchstart zählen, dann
alles zurücksetzen, was zu diesem Aufsetzer gehört (Phasenlogik, Sampler,
Anflug, Rollstrecke, Spur, Wind, Sprit, Abdeckungs-Markierung, Live-Meldung).
Zwei Aufrufer: der Sampler (50 Hz) und `step_flight_at` (wenn kein Sampler
läuft, z. B. im Korpus-Replay). Wer zuerst kommt, schließt ab. Nach `TaxiIn`
schließt nichts mehr ab (Slew/Neuladen nach der Landung ist kein Touch-and-Go).

**Die Phase** wechselt im nächsten Takt nach `Climb` (Wächter-Arm vor dem
`Landing`-Arm). Derselbe Arm fängt `Landing` in der Luft ohne offene Landung
ab (z. B. nach einem Neustart der App im Steigflug).

**v2** entscheidet diese Kante nicht selbst: Sein 60-s-Fenster braucht nach
einem langen Bodenlauf zu lange, bis die Mehrheit „nicht am Boden“ zeigt
(Codex-Analyse 20.10.2026). v2 folgt dem Wechsel `Landing → Climb` über den
bestehenden Weg `fresh_enroute_transition_from_landing` im selben Takt —
geprüft im Test `durchstart_nach_jedem_bodenlauf_verlaesst_landing`
(`shadow_phase == Climb`). Der Gewinn von Stück 3 ist eine Instanz statt
zwei und kein Zeitlimit, nicht ein neues Fenster-Verfahren.

## Ein Datensatz je Aufsetzer

`touchdown_events` ist die Liste aller Aufsetzer: frühere als `TouchAndGo`,
der letzte als `FinalLanding` (verbucht nach 30 s am Boden oder beim
Abrollen). Daraus:

- PIREP an live: `aufsetzer[]`, `haertester_aufsetzer_fpm/_g` (nur bei mehr
  als einem Aufsetzer).
- phpVMS-Feld „Hardest Touchdown“ (Zahl zuerst) für die Hard-Landing-Prüfung
  der GSG-Wartung.
- Recorder (`abgleichAufsetzerMitPirep`): überholt Touch-and-Go-Zeilen, legt
  fehlende aus dem PIREP an, gleicht die finale Zeile an den PIREP an.

## Ein Wert je Landung

Client: `gemeldete_landung` hält fest, was an live ging;
`landung_widerspricht_meldung` meldet beim Einreichen jede Abweichung laut.
Recorder: weicht die Zeile der finalen Landung vom PIREP ab, wird sie
angeglichen (PIREP ist die Wahrheit), Audit-Log `landung_live_angeglichen`,
Sentry-Warnung, Zähler `landung_angeglichen_7d` (Soll 0). Entscheid Thomas
20.10.2026: angleichen und Alarm, die Abgabe nicht sperren.

## Nicht in diesem Schritt

Boden-Band (Boarding…Takeoff, TaxiIn…Arrived) bleibt bei v1 — dort steckt
Ablauf-Logik (OSM-Stände, Divert, Auto-Abgabe, Sonderwege), keine
Bewegungs-Auswertung. Eigenes Stück 4, Entscheidung Thomas.

## Prüffälle

`landung_episoden_tests` (lib.rs): Bodenlauf 10/29/31/36/60/120 s,
Triebwerke 0, Busch-Landung in `Takeoff`, EWG 1599 als Replay
(`tests/fixtures/td_balked_after_ground_roll.jsonl.gz`, anonymisierte
Kinematik), abgerollte Landung bleibt Landung, einzelner AGL-Ausreißer,
ungemessener erster Aufsetzer, härtester Aufsetzer im PIREP, Widerspruch zur
Live-Meldung. Recorder: `__tests__/aufsetzerAbgleich.test.ts`.
