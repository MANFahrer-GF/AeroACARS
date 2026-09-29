# Arbeitspaket: Lernpaket aus vmsACARS 3 (Stand 29.09.2026)

Anlass: Analyse von vmsACARS 3.0.0-nightly.519 (Mac + Windows-Portable, dekompiliert).
Beschluss Thomas 29.09.2026. Basis: `origin/main` v1.9.10 (5b573d8f), Zweig
`feat/lernpaket-vmsacars`, Worktree `~/Claude/aeroacars-lernpaket`.

Reihenfolge nach Nutzen und Risiko. Jede Stufe endet mit einem prüfbaren Zustand.
Kein Release ohne Freigabe von Thomas. Rust-Tests laufen in der CI, lokal nur
`rustfmt`/gezielte Einzeltests (siehe Gedächtnis `macos-syspolicyd-bremst-cargo`).

---

## AP1 — MSFS-G-Kanal `SEMIBODY LOADFACTOR Y` messen (Priorität von Thomas)

**Warum:** vmsACARS benotet G unter MSFS aus `SEMIBODY LOADFACTOR Y`
(`libclient-win.dll`, `AtmoDynTelemetryGroup` → `LoadFactorNormal`), nicht aus
`G FORCE`. Unser `G FORCE` läuft laut Messung rund 614 ms hinter der Sinkrate her
(Gedächtnis `aeroacars-sim-fairness-g-kanal`). Ist der andere Kanal verzögerungsärmer,
wäre er der bessere G-Wert für die Landenote.

**Heute:** Nur `G FORCE` (`telemetry.rs` Feld 19, VISUAL_FRAME). `SEMIBODY LOADFACTOR Y`
und `ACCELERATION BODY Y` werden nirgends gelesen.

**Stufe 1a — nur mitschreiben (keine Wirkung auf die Note):**
- `SEMIBODY LOADFACTOR Y` am Ende von `TELEMETRY_FIELDS` anhängen (Schwanz-Muster;
  Prüfwerte 3624 / 379 / 464 und Stichprobenkette nachziehen).
- `SimSnapshot` bekommt ein optionales Feld; 50-Hz-Sampler (`TelemetrySample`) und
  Flug-Log (`TouchdownWindowSample`, `serde(default)`) schreiben es mit.
- `landing_analysis` bekommt die Gegenstücke zu den G-Kennzahlen
  (`peak_…_post_500ms`, Wert an der Kante) für den zweiten Kanal.
- Anzeige: keine. Note: unverändert.

**Stufe 1b — auswerten (nach ≥ 10 echten MSFS-Landungen mit 1a):**
- Je Landung: Zeitversatz der G-Spitze gegen die Aufsetzkante für beide Kanäle,
  Spitzenhöhe, Korrelation zur Höhenkurven-Sinkrate.
- Entscheidung: übernehmen / verwerfen. Erst dann Stufe 1c.

**Stufe 1c — ggf. nutzen:** G-Wert der Note unter MSFS auf den neuen Kanal
umstellen, `g_auf_referenzkette` neu kalibrieren (MSFS ist dort Referenz),
Goldenset + Korpus nachmessen. **Keine Altbuchungen neu rechnen.**

**Abnahme 1a:** CI grün; im Flug-Log eines MSFS-Fluges steht der neue Kanal je
Sample; Note eines Replays bytegleich zu vorher.

---

## AP2 — Deckel bei harter Landung

**Warum:** Gute Nebenachsen gleichen heute eine harte Landung aus;
`aggregate_master_score` (`crates/landing-scoring/src/lib.rs`) ist ein reiner
gewichteter Mittelwert. vmsACARS deckelt: ab 1,75 g höchstens 40, ab 2,6 g höchstens 15.
Thomas: „ganz nice“.

**Umsetzung:**
- Deckel auf die **Gesamtnote**, G-Grenzen auf den Wert der Referenzkette
  (`scored_g_fuer_punkte`), nicht auf Roh-G.
- Grenzen: ≥ 1,75 g → max. 40; ≥ 2,6 g → max. **14** (Codex-QS 29.09.: mit 15 hieße der
  Überlast-Fall in der Klassenleiter „hard“, „severe“ beginnt erst unter 15). Der Grund
  (`score_deckel`) steht nur im Datensatz, wenn der Deckel die Note wirklich senkt.
  Zusätzlich prüfen, ob ein Sinkraten-Gegenstück nötig ist (≥ 600 fpm), damit der
  Deckel nicht allein am G-Kanal hängt.
- Grund des Deckels als Kennung am Ergebnis (`deckel: "hard_landing" | "overstress"`),
  sichtbar im Landungs-Tab.
- TS-Zweitimplementierung `client/src/lib/landingScoring.ts` ist veraltet (nur
  Legacy-Pfad) — nicht mitziehen, aber im Code-Kommentar vermerken; aeroacars-live-
  Webapp prüfen, ob sie die Note selbst rechnet oder übernimmt.

**Abnahme:** Unit-Tests für beide Stufen + Grenzfälle; Goldenset zeigt, welche
Landungen sich ändern (Liste an Thomas); neue Flüge only.

---

## AP3 — Hopser-Regel gegenprüfen

**Heute:** Hopser zählt bei Luftphase ≥ 300 ms **UND** Höhe ≥ 5 ft (Forensik) bzw.
≥ 15 ft (Wertung). vmsACARS: ≥ 2 ft Radiohöhe **ODER** ≥ 1 s ohne Bodenkontakt.

**Umsetzung:** Nur Gegenprobe, keine Regeländerung ohne Befund:
- Beide Regeln über alle Fixtures (`client/src-tauri/tests/fixtures/*.jsonl.gz`,
  u. a. `da40_gsg2056_bounce`) laufen lassen, Abweichungen tabellieren.
- Wo vmsACARS zählt und wir nicht (oder umgekehrt): Einzelfall ansehen.

**Abnahme:** Tabelle + Empfehlung an Thomas.

**Ergebnis 29.09.2026 (15 Fixture-Flüge mit Aufsetzfenster, 10 s nach der Kante):**

| Flug | Luftphasen nach Aufsetzen (Dauer/Höhe) | wir Forensik | wir Note | vmsACARS |
|---|---|---|---|---|
| dah3181 (X-Plane) | 3766 ms / 8,1 ft · 684 ms / 0,0 ft | 1 | 0 | 1 |
| pto705 | 2275 ms / 1,9 ft | 0 | 0 | **1** |
| da40_gsg2056_bounce | 958 ms / 0,9 ft · 154 ms / −0,3 ft | 0 | 0 | 0 |
| pto105, jbu322, jbu323, ity324 | je eine Phase 0,3–0,9 s, < 0,3 ft | 0 | 0 | 0 |
| 8 weitere | keine | 0 | 0 | 0 |

- Kurze Bodenflag-Flackerer (< 1 s, < 1 ft) verwerfen beide Regeln richtig.
- **pto705:** 2,3 s „in der Luft“ bei 1,9 ft — das ist Flackern des Bodenflags beim
  Ausrollen, kein Hopser. vmsACARS zählt ihn (ODER ≥ 1 s), wir nicht (UND ≥ 5 ft).
  Unsere UND-Regel ist hier robuster.
- **dah3181:** 3,8 s und 8,1 ft — ein echter kleiner Hopser. Wir erfassen ihn forensisch,
  benoten ihn aber nicht (Wertungsschwelle 15 ft); vmsACARS würde ihn benoten.

**Empfehlung (Fixtures):** vmsACARS-Regel nicht übernehmen.

**Nachmessung am Live-Recorder (29.09.2026, `touchdowns.window_json`, 1344 Landungen,
nur lesend):** 306 Luftphasen nach dem ersten Bodenkontakt.

| Höhe \ Dauer | < 0,3 s | 0,3–1 s | 1–3 s | ≥ 3 s |
|---|---|---|---|---|
| < 2 ft | 65 | 103 | 85 | 2 |
| 2–5 ft | 0 | 0 | 30 | 5 |
| 5–15 ft | 0 | 0 | 2 | 8 |
| ≥ 15 ft | 0 | 0 | 0 | 3 |

- vmsACARS-Regel (≥ 2 ft ODER ≥ 1 s) hätte **127** Landungen als Hopser gezählt — fast
  alles Bodenflag-Flackern. td 1492: 7,1 s „in der Luft“ bei 0,1 ft. Eine reine
  Zeitregel ist damit widerlegt.
- Heute benotet (≥ 15 ft): 3 Landungen. Nur Forensik (5–15 ft): **9** Landungen.
- Die 5–15-ft-Fälle sind echte Hopser: td 1403 (142 kt, +384 fpm, 12,6 ft, 3,6 s),
  td 603 (9,8 ft, 5,6 s), td 841 (63 kt, mehrfaches Springen bis 10,5 ft).
- Unter 5 ft liegt kein einziger Fall, der eindeutig ein Hopser ist; das Flackern
  reicht bis knapp 5 ft (td 1474: 4,8 ft, 3,1 s).
- Begründung der 15 ft (v0.7.6/0.7.7, SAS9987 13,6 ft als „Federwerk-Hopser“) ist
  physikalisch nicht haltbar: Federweg des Fahrwerks liegt weit unter 1 m. Die Schwelle
  stammt aus der Zeit, bevor die Höhe relativ zur Bodenhöhe beim Aufsetzen gemessen wurde
  (Fix THY42).

**Realität:** Airbus FCTM unterscheidet „light bounce“ (Fluglage halten, Landung
fortsetzen) und „high bounce“ (Fluglage halten, Durchstart); Airline-SOPs setzen die
Grenze bei 5 ft (Airbus Safety First, „A Focus on the Landing Flare“). FDM-Programme
erkennen einen Hopser an Boden→Luft→Boden am Luft/Boden-Schalter plus Anstieg der
Radiohöhe; eine veröffentlichte Standard-Schwelle haben wir nicht belegt.

**Beschlossen und umgesetzt (Thomas, 29.09.2026):** 5 ft mit Hinweistext.
`BOUNCE_SCORED_MIN_AGL_FT` 15 → 5 ft; Streamer-Rückfall misst jetzt relativ zur
Bodenhöhe beim ersten Bodenkontakt (Scharf > 5 ft, zurück bei Bodenkontakt oder < 2 ft),
vorher absolut 15/5 ft; Tipp-Texte der Hopser-Teilnote (DE/EN/IT) nennen das
Airbus-Verfahren (Fluglage halten, durchstarten, nicht nachdrücken). Offen: Texte der
aeroacars-live-Webapp prüfen, falls sie eigene Übersetzungen hat.

**Empfehlung an Thomas (ursprünglich):** Wertungsschwelle von 15 ft auf **5 ft** senken (= Forensik-
schwelle = „high bounce“-Grenze der SOPs), UND-Regel mit ≥ 0,3 s bleibt. Wirkung: +9 von
1344 Landungen (0,7 %) bekommen einen Hopser in die Note. Keine Zeitregel. Keine
Neuberechnung alter Landungen. Optional ein Hinweis im Bericht: „Hopser über 5 ft —
nach Airbus-Verfahren wäre ein Durchstart vorgesehen“ (nur Text, keine Zusatzstrafe).

---

## AP4 — ILS-Gleitpfad mit Quellen-Kennung

**Heute:** Gleitwinkel kommt vom Live-Server (`NavRunway.glideslope_angle`, Standard 3°,
`ils: Option<NavIls>`); fehlt er, gilt stillschweigend 3°. Die Abweichung ist nur
Soll-V/S gegen Ist-V/S, keine geometrische Höhe über dem Pfad; nur informativ.

**Hauptquelle: unsere Navigraph-Daten** (Hinweis Thomas 29.09.): Der Live-Server
liefert sie schon (`/api/navdata/airport/<ICAO>`: `ils`, `glideslope_angle`, `tch_ft`,
Navigraph-DFD-Vollbestand). vmsACARS muss dafür die Sim-Facility-Daten abfragen, wir haben
die bessere Quelle bereits im Client. MSFS-Facility-ILS höchstens als Rückfall, wenn der
Server nicht erreichbar ist (Navdaten-Zwischenspeicher im Client prüfen).

**Umsetzung:**
- Quellen-Kennung am Flug: `navigraph_ils` (ILS-Gleitweg aus Navigraph) / `navigraph_bahn`
  (Gleitwinkel der Bahn) / `angenommen_3grad`.
- Vorab prüfen: Wie viele Bahnen liefert der Server mit `ils` und mit echtem
  `glideslope_angle` (≠ Standard 3,0)? Und liefert er die Gleitweg-Antennenposition bzw.
  TCH, die für die geometrische Abweichung nötig ist?
- Geometrische Pfadabweichung je Tick (Höhe über Schwelle + TCH gegen Distanz ×
  tan(Winkel)), in Grad und Punkten (Dots) wie vmsACARS: voll 0,35°, 0 bei 0,7°.
- Zunächst **nur Forensik**, keine Note.

**Abnahme:** Wert + Quelle im Analyse-JSON und im Landungs-Tab; Korpus-Plausibilität.

**Ergebnis (29.09.2026, Zweig `feat/lernpaket-ap4-6`):**
- Neues Modul `client/src-tauri/src/anflug_forensik.rs` (reine Funktionen). Je Probe des
  Anflug-Puffers: Entfernung vor der Landeschwelle über `runway::projiziere_auf_bahn`
  (Achse Schwelle→Gegenende der Navdaten), versetzte Schwelle über
  `displacement_not_in_geometry_ft` (dieselbe Größe wie in `assess_touchdown`, kein doppelter
  Abzug seit AIRAC 2608). Sollhöhe = Schwellenhöhe + TCH + d·tan θ. Winkel wird **vom
  Gleitweg-Bezugspunkt (GPI = TCH/tan θ hinter der Schwelle)** gemessen:
  `atan(h/(d+GPI)) − θ` — so misst auch ein echtes ILS, und auf der Pfadgeraden ist die
  Abweichung in jeder Entfernung exakt 0. 1 Dot = 0,35° × θ/3 (QS-Korrektur 6), + = über dem Pfad.
- Nur Proben vor der Schwelle, im Gleitwegsektor ±10° (QS-Korrektur 5), Steuerkurs
  ≤ 90° zur Bahn, letzte 5 min vor dem Aufsetzen, 1000–200 ft über der Schwelle; Tore
  1000–500 / 500–200; je Tor mind. 5 Proben. Ausgabe: mittlere |Abw.| in Dots, größte
  Abweichung in Dots und ft mit Vorzeichen, Probenzahl.
- Quelle: `navigraph_ils` (`ils.is_some()`), `navigraph_bahn` (Navigraph-Bahn ohne ILS),
  `angenommen_3grad` (keine Navdaten-Bahn → **keine Werte**, Grund `keine_bahn`). Am
  Wert von `glideslope_angle`/`tch_ft` ist die Echtheit nicht ablesbar (serde-Standard
  3,0/50, Server schreibt fehlende TCH als **0** und Nicht-ILS-Winkel als 3,0) — deshalb
  entscheidet `ils`, und TCH 0 gilt als unbekannt → 50 ft mit `tch_angenommen: true`.
  Fehlt die Schwellenhöhe → Grund `schwellenhoehe_fehlt`, keine Werte.
- `ApproachBufferSample` hat jetzt `lat`/`lon` (Option, nie Grund zum Verwerfen einer Probe).
- Speicherung: `LandingRecord.anflug_gleitpfad` (serde default, alte Datensätze lesbar),
  Analyse-JSON `landing_analysis.anflug_gleitpfad` (damit im Flug-Log-Ereignis
  `landing_analysis`), Landungs-Tab: Info-Zeilen unter der Approach-Stability-Card
  (`AnflugForensikInfo.tsx`), Texte DE/EN/IT. **Keine** Änderung an MQTT/PIREP, Noten,
  Gate oder Deckel; keine Neuberechnung alter Landungen.
- Grenzen: Im Flug-Log steht der Wert nur, wenn der Stempel vor dem Touchdown-Dump lief
  (Normalfall), im Datensatz immer. Korpus-Plausibilität steht noch aus (braucht Flüge mit
  dieser Fassung).

**QS-Korrekturen (Cloud-Prüfer, 29.09.2026, „Freigabe: nein“ → behoben):**
1. Größte Abweichung: Dots und ft kommen jetzt von DERSELBEN Probe (vorher zwei getrennte
   Maxima, „−1,2 Dots (+60 ft)“).
2. Nachziehen nach Bahnwechsel: `anflug_forensik_nachziehen` läuft in
   `bahn_upgrade_anwenden` (Navigraph-Upgrade beim Einreichen) und in
   `bahn_am_aufsetzpunkt_nachholen` (Szenerie ersetzt Schwelle/Ende/Versatz), wenn sich die
   Forensik-Bahn geändert hat — nur für eine schon gestempelte Landung (kein
   Durchstart-Reset dazwischen) und nur mit Proben vor dem Aufsetzen im Puffer; gleicher
   Zeitpunkt/gleiches Fenster/gleiche Platzhöhe wie beim Stempel.
3. Eigener Forensik-Ringpuffer (`anflug_forensik_puffer`, 400 Proben ≈ 5 min), an derselben
   Stelle und mit demselben Plausibilitätsfilter befüllt wie `approach_buffer`; die
   120er-Kappe bleibt (trägt über `compute_approach_stddev` die Note). Je Tor
   `oberste_hoehe_ft`, Anzeige „erfasst ab X ft“, wenn ein Tor nicht von oben an erfasst ist.
   Veralteter Kommentar an `APPROACH_BUFFER_MAX` berichtigt.
4. Sim-Boden: `msl − agl` der (bis zu 3) schwellennächsten Proben (±600 m längs, ±100 m quer),
   Median. Weicht er > 20 ft von der Navigraph-Schwellenhöhe ab (oder fehlt diese), gilt der
   Sim-Boden: `hoehenbezug` = `sim_boden`, dazu `schwellenhoehe_navigraph_ft` und
   `sim_boden_ft`; neutraler Hinweis im Landungs-Tab.
5. Gleitpfad-Sektor ±10° vom GPI aus (echter Gleitweg ≈ ±8°), Abstand `hypot(d+GPI, quer)`.
   Die Anflugruhe ist an keinen Sektor gebunden.
6. Dot = 0,35° × θ/3 (bei 3° unverändert, EGLC 5,5° ≈ 0,64°), `grad_je_dot` gespeichert.
7. `schub_grund`: `kein_n1` / `zu_kurz` getrennt, eigener Text je Grund.
- Außerdem: N1-Mittel nur über Triebwerke mit N1 ≥ 5 % (ein stehendes Triebwerk halbierte
  sonst jede Schubänderung); ohne Bahn werden `winkel_deg`/`tch_ft` nicht mehr gespeichert.
  Neue Tests u. a. für Querversatz, Gegenkurs, Ost-West-Bahn, handgerechnete Punkte
  (h = 50 + d·tan 3°, bis 2,75 NM), Versatz über echten Bahntreffer, Nachziehen nach Upgrade.

**Zusatz (Entscheid Thomas 29.09.2026): PIREP, Flug-Log, eine Anzeige für Client und Webapp**
- Die Structs liegen jetzt in `landing-scoring` (`anflug_forensik.rs`), `storage`
  re-exportiert sie — eine Definition für LandingRecord und PIREP.
- `PirepPayload.anflug_gleitpfad` / `.anflug_ruhe` (additiv, fehlen ohne Befund), befüllt
  in `build_pirep_payload` aus dem finalen Stand. NICHT im Touchdown-Payload (10-KB-Grenze).
  Größe: voll befüllt (drei Tore, Sim-Boden, Schub) 557 + 412 = 969 Bytes; dafür legt der
  Client die Werte gerundet ab (Dots/Raten 0,01, ft/s 0,1, Höhen ganze Fuß) —
  ungerundete f32 waren 1074 Bytes.
- Flug-Log: Ändert `anflug_forensik_nachziehen` die Werte, schreibt `emit_landing_finalized`
  einmal ein Ereignis `landing_analysis_nachtrag` (`payload`: `edge_at`,
  `anflug_gleitpfad`, `anflug_ruhe`) vor `landing_finalized`.
- `AnflugForensikInfo.tsx` steht in `scripts/anzeige-sync.mjs` (DATEIEN). Nur React +
  react-i18next, Schlüssel als Literale bzw. Vorspann (`quelle.`, `grund.`,
  `schub_grund.`), Props = die beiden Blöcke. Der Abgleich in aeroacars-live
  (`node scripts/anzeige-sync.mjs --schreiben`) steht noch aus: bis dahin sind die zwei
  Tests in `AnzeigeSync.test.tsx` rot (Trockenlauf gegen eine Kopie: 1 Datei, 75
  Beschriftungen, danach „auf beiden Seiten gleich“).

**Nachprüfung (Cloud-Prüfer auf 105f3264, 29.09.2026) — zwei neue Fehler aus den Korrekturen:**
- A: Der Sim-Boden wurde auch VOR der Schwelle gemessen (Wasser/Klippe/Senke: KLGA, LPMA,
  TNCM, LXGB) und schaltete den Bezug grundlos um. Jetzt nur Proben über der Bahn:
  0–600 m hinter der Landeschwelle, |quer| ≤ max(halbe Bahnbreite, 30 m), mind. 2 Proben;
  ersatzweise die letzte Probe über der Bahn vor dem Aufsetzen.
- B: Der 400er-Puffer ließ bei Touch-and-Go/Platzrunde und nach einem Durchstart den
  vorigen Anflug einfließen. Jetzt Schnittzeitpunkt `anflug_forensik_ab`, gesetzt beim
  Touch-and-Go-Reset, in `check_go_around` und im FSM-Touch-and-Go; Proben davor zählen
  nicht (Gleitpfad, Ruhe, Sim-Boden). Das Nachziehen nutzt den Schnitt, der beim Stempel
  galt. Zeitpunkt statt Leeren, damit der Puffer für die Diagnose bleibt und ein
  späterer Schnitt einen gemachten Stempel nicht verändert.
- C: Der Nachtrag `landing_analysis_nachtrag` wird jetzt auch im Warteschlangen-Zweig
  vor `FlightEnded` geschrieben (einmal, dieselbe Fahne).

---

## AP5 — Anflugruhe (Forensik ohne Note)

**Heute:** 1000-ft-Tor mit V/S-Jerk, σIAS, σBank; kein Pitch, kein Schub im
`ApproachBufferSample`; Abtastung unter 1500 ft nur 1 s / 0,75 s.

**Umsetzung:** `ApproachBufferSample` um Pitch + Schubhebel/N1 erweitern; Maße:
Vorzeichenwechsel der Pfadabweichung (braucht AP4), Nick-/Roll-Ruckeln,
Schub-Umkehrungen je Minute. Tore 1000 / 500 ft. Anzeige als Hinweis, **keine Note**.

**Abhängigkeit:** AP4.

**Ergebnis (29.09.2026, Zweig `feat/lernpaket-ap4-6`):**
- `ApproachBufferSample` um `pitch_deg` und `n1_mittel_pct` (Mittel aus `eng_n1_pct`)
  erweitert. Je Tor 1000–500 / 500–200 ft (Bezug Schwellenhöhe, sonst Platzhöhe):
  - Seitenwechsel der Pfadabweichung aus AP4, Totband ±0,1 Dot (≈ 7 ft bei 2 NM —
    Probenrauschen zählt nicht als Korrektur); ohne AP4-Pfad `None`.
  - Nick-/Roll-Unruhe = Standardabweichung der Nick-/Rollrate in °/s (Paare mit
    0,2–5 s Abstand, mind. 4 Raten). Eine konstante Rate ergibt 0; Ein- und Ausleiten
    einer Kurve oder das Abfangen ändern die Rate und zählen mit — der Wert misst
    Bewegung um die Achse, nicht nur Pendeln.
  - Schub-Umkehrungen je Minute aus mittlerem N1 mit Hysterese 2 % N1 (A/THR- und
    Hebelkorrekturen liegen bei 3–10 %, darunter Regelrauschen); mind. 10 s Dauer.
- **Weggelassen:** Schub bei X-Plane (der Adapter liest weder N1 noch Hebelstellung,
  `eng_n1_pct` ist dort immer `None`) und bei Kolbenmotoren/MSFS-Add-ons ohne lebendes
  N1 — dort steht `None` und im Landungs-Tab „ohne N1-Daten nicht erfasst", kein
  erfundener Wert. Eine stetige Schubhebel-Stellung führt der SimSnapshot für keinen der
  beiden Sims (nur das Rasten-Label `thrust_gate` einzelner Add-on-Profile).
- Speicherung wie AP4: `LandingRecord.anflug_ruhe`, `landing_analysis.anflug_ruhe`,
  Hinweiszeile im Landungs-Tab ohne Farbe und ohne Wertung. Keine Note.

---

## AP6 — Profil je Funktion (MSFS)

**Heute:** X-Plane fällt je Funktion auf den Standard zurück (`build_active_catalog`).
MSFS nur per Freigabeliste (`standard_seatbelts_bedient`, `standard_transponder_bedient`),
sonst `None`.

**Umsetzung:** Freigabeliste zu einer Tabelle „Funktion × Profil → Standard erlaubt?“
verallgemeinern. **Nicht** blind überall zurückfallen: bei vielen Add-ons lügt der
Standard-SimVar (Parkbremse iFly, Transponder A220). Nur wo gemessen.

**Abnahme:** Tabelle im Code, Tests je Profil, keine Verhaltensänderung bei den
gemessenen Profilen.

---

## AP7 — X-Plane-Plugin zum Dataref-Server ausbauen (beschlossen)

**Heute:** `xplane-plugin/src/plugin.cpp` (C++17, SDK 4.3, v0.5.13), 14 feste Datarefs,
JSON über UDP 127.0.0.1:52000, 20 Hz / jeder Frame unter 200 ft. Client wertet nur das
`touchdown`-Paket aus; `telemetry` ist bloß Lebenszeichen. Keine Signatur der `.xpl`,
keine Prüfsumme beim Download. ADR 0004 veraltet.

**Ziel (Muster AcarsConnect, aber schlanker):**
1. Anmeldung: Client schickt Namensliste (Gruppe), Plugin sucht jeden Namen einzeln
   (`XPLMFindDataRef` + Typ-Cache), antwortet mit „gefunden / fehlt“ je Name.
2. Lieferung: Plugin schiebt die Gruppe je Flight-Loop mit fester Rate; fehlende Namen
   liefern **keinen Wert** (nie 0).
3. Neu-Suche bei `XPLM_MSG_PLANE_LOADED`.
4. Aufzählung aller Datarefs (XPLM 4 `XPLMCountDataRefs`/`XPLMGetDataRefsByIndex`)
   für „Flugzeug vermessen“.
5. Startschutz + Crash-Handler; **kein** Szenerie-Parser im X-Plane-Prozess.
6. Client: neuer Transport im sim-xplane-Adapter mit Rangfolge Plugin → Web-API → RREF.
7. Protokoll versioniert; Client lehnt unpassende Plugin-Versionen ab.
8. Prüfsumme beim Plugin-Download; Mac-Signierung klären (Apple-Developer-Account?).
9. ADR 0004 neu schreiben.

**Abnahme:** Mit ToLiss und Zibo: kein Flugzeug gilt mehr als CL650, fehlende Datarefs
erscheinen als „fehlt“ statt 0; Rückfall ohne Plugin funktioniert wie bisher.

---

## Reihenfolge

Korrigiert am 29.09.2026 (Thomas: „AP7 ist das letzte“):

1. AP1a, AP2, AP3 — veröffentlicht mit v1.9.11 (29.09.2026).
2. AP4 → AP5 (Gleitpfad, Anflugruhe) — gebaut, Cloud-QS „Freigabe: ja“,
   Live-Seite (Recorder + Landungsanalyse) ebenso; wird mit AP7 ausgerollt.
3. AP6 — als Code-Umbau verworfen (die Tabelle „Funktion × Profil“ gibt es
   praktisch schon; ein Umbau änderte nichts). Stattdessen Messaufruf im Forum
   (#44, 29.09.2026): Fenix- und PMDG-Autopilot sind durchgehend „aus“, belegt
   an Flug-Logs; Anbindung je Add-on, sobald Luftmessungen vorliegen.
4. AP7 (Plugin) — zuletzt, eigener Zweig `feat/ap7-xplane-plugin`, ADR-0004 neu.
5. AP1b/1c, sobald genug MSFS-Landungen mit dem neuen G-Kanal vorliegen.
