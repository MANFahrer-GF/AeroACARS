# Phasenerkennung: Reiseband entscheidet v2 („Stück 2", 17.10.2026)

## Ausgangslage

Seit v0.16.12 laufen zwei Phasenerkennungen:

- **v1** (`step_flight_at`, lib.rs): Einzelwert-Regeln, mechanisch maßgeblich
  (Phasenwechsel-Ereignisse, ACARS-Log, Sendetakt, Server-Prüfer, Anflug-Puffer).
- **v2** (`phase_v2.rs`): fenster-basiert (60-s-Kinematik-Segmenter), seit v0.19.0
  die angezeigte Phase.

Korpus 30 Tage, 447 Flüge: v1 flatterte zwischen Cruise und Descent in 40 % der
Flüge (389 Mal) und begann den Reiseflug im Steigflug zu früh (22,8 h in
111 Flügen). v2 entschied dieselben Fälle richtig (49 Mal bzw. 1,3 h).

## Design

Eine gültige Phase (`stats.phase`). Im **Reiseband** (Climb, Cruise, Descent)
übernimmt v1 die v2-Phase des Vortakts (`reiseband_phase`), höchstens einen
Takt (≈ 3 s) später. Die alten Einzelwert-Regeln entfallen: Climb→Cruise bei
|V/S| < 200, Cruise→Descent nach 5000 ft Verlust, B-003 Descent→Cruise,
`check_descent_transition`.

v1 behält, wo es besser ist oder Nebenwirkungen trägt:

- **Anflug-Kante** im Descent-Arm im selben Takt (AGL < 5000, V/S < 0); an ihr
  beginnt der Anflug-Puffer der Stabilitätsnote. Resets in `anflug_beginnt`.
- **Cruise-Schnellweg** in den Sinkflug (AGL < 3000, V/S < −500), keine
  5000-ft-Anflug-Kante aus dem Reiseflug.
- **Warteschleife**, **Anflug/Final**, **Durchstart-Detektor** (zählt für den
  PIREP, setzt Vref zurück, schneidet die Anflug-Forensik), **Landungskante**
  (Bodenberührung, Aufsetz-Zeitpunkt), **Boden**.

v2-Anpassungen:

- Folgt einem v1-Durchstarter (Approach/Final → Climb, in der Luft) sofort.
- Cruise→Approach nur mit Fenster-Evidenz (`Segment::Descending`).
- Beobachtete Reiseflughöhe (`obsref`) wird gespeichert (`v2_reiseflughoehe_ft`).

Versöhner: nur noch v1 im Anflug/Final (verpasster Durchstarter, verpasste
Bodenberührung, Steigen ohne Durchstarter) und v1 im Reiseband bei v2 im
Anflug/Final/Landung (GSG22-Klasse).

Anzeige, Live-Karte, phpVMS-Heartbeat, Tray, Bordbuch und ACARS-Log lesen
`stats.phase` (`PHASE_V2_CUTOVER`, `effective_phase`, `protokoll_phase`,
`effective_display_phase` entfallen). `shadow_phase`/`shadow_segment` bleiben
als v2-Rohsicht in der Position-JSONL; das „LEVEL"-Etikett liest `shadow_segment`.

## Synchron-Regel (unverändert aus phase_v2.rs)

v2 tickt nach dem v1-Schritt (`phasen_takt`) mit der Post-Step-Phase: außerhalb
des Luft-Bands spiegelt v2 v1; beim Eintritt ins Band (Takeoff→Climb, Resume,
Touch-and-Go, Durchstarter) Baseline-Sync mit Segmenter-Reset; im Band läuft v2
frei. Weil v1 im Reiseband v2 übernimmt, laufen beide dort zusammen.

## Prüfung (Korpus-Replay)

`phasen_korpus_replay` (`#[ignore]`) spielt Recorder-Logs durch `phasen_takt`
(dieselbe Kopplung wie der Streamer) und schreibt je Flug Phasenbogen,
Durchstarter und Anflug-Stabilität (`compute_approach_stability_v2`).
Grundlinie = derselbe Werkzeug-Stand auf `main`.

| Prüfgröße | Grundlinie | Neu |
|---|---|---|
| Flattern im Reiseband | 389 | 52 (+7 an Warteschleifen) |
| Früher Reiseflug (in 15 min noch > 4000 ft Steigen) | 22,8 h / 111 Flüge | 1,6 h / 17 Flüge |
| Anflug-Stabilität identisch | — | 445/447 |
| Warteschleifen | 9 (davon 2 Schein-Holds im flatternden Sinkflug) | 9 (Prüfung jetzt auch im Sinkflug) |

Erklärte Abweichungen:

- **Anflug-Stabilität** (2 Flüge, beide Kleinflugzeug): yNOgAr — neu richtig,
  Landung im echten Aufsetzmoment statt 70 s später (alt: Ausroll-Daten im
  Puffer); Qd245 — v2 erkennt einen kurzen tiefen Sinkflug später.
- **Durchstarter** (4 Flüge): einer weniger (falsch, kurz nach dem Start), drei
  mehr bei tiefen Kleinflugzeug-Flügen. Der Detektor ist schon in der
  Grundlinie ungenau: 7 Durchstarter mit Tiefpunkt > 3 NM von jeder Bahn.
- **Anflugbeginn** (32 Flüge > 10 s verschoben): meist früher und richtig (die
  alte Phase blieb im Reiseflug, bis 5000 ft verloren waren); 10 Flüge bis 76 s
  später (v2-Verzögerung bei kurzen tiefen Sinkflügen).

## Offen (Backlog)

- Durchstart-Detektor: Tiefpunkt nahe einer Bahn verlangen.
- v2: Sinkflug-Verzögerung bei kurzen Sinkflügen tiefer Kleinflugzeug-Flüge.
- Rest-Flattern (52) nach Paaren auswerten.
- Server: `POSITION_DELTA_EXCESSIVE`-Rauschgrenze im Steigflug (500 kt). Mit dem
  Wegfall des frühen Reiseflugs bleiben lange Steigflüge länger in CLIMB — die
  Fehlalarme dort steigen, bis die Grenze angehoben ist (Vorschlag 700 kt).

## QS

Advisor in jedem Schritt; Codex (gpt-5.6-sol) Runde 1: P2 flacher Anflug aus
dem Reiseflug, P3 Ende der Warteschleife — behoben, Runde 2 ohne Befunde.
Gegenproben: vier Verhaltens-Tests rot auf `main`, Warteschleifen- und
Codex-Tests rot ohne den jeweiligen Fix. Abschluss-Lauf: jede Landung der
Grundlinie wird erkannt.

## Restpunkte (2.0.25, 17.10.2026)

Was 2.0.24 als „Backlog" liegen ließ, im selben Thema behoben (Thomas: „kein
Stückwerk"). Grundlinie = 2.0.24 im selben Korpus-Replay.

| Punkt | Befund | Lösung |
|---|---|---|
| Durchstarter | 8 von 18 gezählten lagen quer zur Bahn oder 5,3–25 NM von jeder Schwelle; sie zählten im PIREP und gaben in der Treibstoff-Teilnote eine Mehrweg-Gutschrift | Zählung nur mit Landeschwelle ≤ 5 NM voraus und ≤ 45° Kursabweichung am Tiefpunkt (`runway::schwelle_voraus`); der Tiefpunkt darf über der Bahn liegen, nicht hinter dem Bahnende; sonst Climb ohne Zählung |
| Warteschleife | Ende nach 30 s geradeaus — kürzer als ein Schenkel; jede echte Warteschleife zerfiel in Bruchstücke, lange Kurven auf FL388 zählten | Eintritt ≥ 300° gleichsinnige Drehung in 6 min im Höhenflug > 1500 ft AGL; Ende nach 3 min geradeaus |
| Schwingung / Korrekturen | Climb↔Descent folgte jeder Halbschwingung (±2000 ft) und 40–60-s-Korrekturen | Umkehr-Sperre: über 3000 ft AGL erst nach 120 s anhaltender neuer Richtung |
| Dellen / Stufen im Reiseflug | Cruise↔Descent bei 600–2300 ft, Cruise↔Climb bei Stufen | Band 2000 ft um die AKTUELLE Reiseflughöhe (tieferen Stufen folgt sie erst nach 10 min, sonst würde ein Treppen-Sinkflug Cruise); 10 min Level im Sinkflug = neuer Reiseflug; Rückkehr aus Descent mit 1000 ft Hysterese |
| Kurze tiefe Sinkflüge | v2 erkannte sie bis 76 s zu spät | Climb-Schnellweg (< 3000 ft AGL, < −500 fpm, 10 s), frühestens 2 min nach Climb-Beginn (sonst zweiter falscher Durchstarter nach Joels Fehlanflug) |
| Parkbremse FBW-Familie | Headwind A330neo ohne Marker (ICAO A330) als Default erkannt; A380X/Headwind lasen die Standard-SimVar, die sich nie bewegt | Headwind am Ordner; Hebel-Variable für die ganze Familie |
| Anflug über Gebirge (Joel) | kein Erkennungsfehler: Er sank zu früh auf ~8000 ft und stieg wieder; falsch war nur der gezählte Durchstarter | durch die Durchstart-Regel erledigt; eine Höhen-Regel für den Anflugbeginn hätte 199 von 472 Anflügen verschoben |

Standard-A320neo MSFS 2024 (iniBuilds): bleibt auf der Standard-SimVar; ein
einziger Flug ohne gesetzte Bremse reicht nicht als Beleg — beim nächsten Flug
prüfen.

Ergebnis (447 Flüge, gegen 2.0.24):

| Größe | ohne Höhenvorgabe | mit Vorgabe (`REF=max`) |
|---|---|---|
| Wechsel < 3 min im Reiseband | 59 → 11 | 73 → 28 (24 davon echte Stufensteigflüge, 1–2,6 kft in ~1,5 min) |
| Warteschleifen | 9 Bruchstücke → 4 echte | gleich |
| Anflug-Stabilität gleich | 446/447 (yNOgAr: neu richtig) | gleich |
| Landung erreicht | alle | alle |
| gezählte Durchstarter | 8 Flüge verlieren einen falschen | gleich |
| Sinkflug-Beginn | p50 +54 s, p90 +120 s | p50 +51 s, p90 +117 s |

Mit Vorgabe steigt „früher Reiseflug" 0,9 → 1,2 h, fast ganz aus einem Flug
(4/PQZk, A380X): 2.0.24 zeigte dort 8,6 h Sinkflug auf FL331 im Level, neu
Reiseflug nach 10 min; die Messgröße zählt ihn, weil später eine Stufe folgt.

Bewusst belassen (Codex-QS): Durchstarter ohne Bahn in der Datenbank oder im
Kreisanflug > 45° zählen nicht (von den falschen Fällen nicht zu trennen, die
Phase geht trotzdem in Climb); Holding prüft keinen Durchstarter (wie vor dem
Paket); nach Neustart im Holding bis 3 min Holding-Anzeige (Kursverlauf nicht
gespeichert).
