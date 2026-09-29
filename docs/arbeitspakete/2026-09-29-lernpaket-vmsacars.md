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
- Vorschlag Grenzen (vor Einbau am Korpus prüfen): ≥ 1,75 g → max. 40; ≥ 2,6 g → max. 15.
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

---

## AP4 — ILS-Gleitpfad mit Quellen-Kennung

**Heute:** Gleitwinkel kommt vom Live-Server (`NavRunway.glideslope_angle`, Standard 3°,
`ils: Option<NavIls>`); fehlt er, gilt stillschweigend 3°. Die Abweichung ist nur
Soll-V/S gegen Ist-V/S, keine geometrische Höhe über dem Pfad; nur informativ.

**Umsetzung:**
- Quellen-Kennung am Flug: `ils` (Navdaten-ILS) / `bahn` (Gleitwinkel der Bahn) /
  `angenommen_3grad`.
- Geometrische Pfadabweichung je Tick (Höhe über Schwelle + TCH gegen Distanz ×
  tan(Winkel)), in Grad und Punkten (Dots) wie vmsACARS: voll 0,35°, 0 bei 0,7°.
- Optional später: MSFS-Facility-ILS als zweite Quelle.
- Zunächst **nur Forensik**, keine Note.

**Abnahme:** Wert + Quelle im Analyse-JSON und im Landungs-Tab; Korpus-Plausibilität.

---

## AP5 — Anflugruhe (Forensik ohne Note)

**Heute:** 1000-ft-Tor mit V/S-Jerk, σIAS, σBank; kein Pitch, kein Schub im
`ApproachBufferSample`; Abtastung unter 1500 ft nur 1 s / 0,75 s.

**Umsetzung:** `ApproachBufferSample` um Pitch + Schubhebel/N1 erweitern; Maße:
Vorzeichenwechsel der Pfadabweichung (braucht AP4), Nick-/Roll-Ruckeln,
Schub-Umkehrungen je Minute. Tore 1000 / 500 ft. Anzeige als Hinweis, **keine Note**.

**Abhängigkeit:** AP4.

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

1. AP1a (G-Kanal mitschreiben) — klein, sofort Messdaten.
2. AP2 (Deckel) — klein, klare Wirkung.
3. AP3 (Hopser-Gegenprobe) — Analyse.
4. AP7 (Plugin) — größter Brocken, eigener Zweig.
5. AP4 → AP5 (Gleitpfad, dann Anflugruhe).
6. AP6 (Profil je Funktion).
7. AP1b/1c, sobald genug MSFS-Landungen mit dem neuen Kanal vorliegen.
