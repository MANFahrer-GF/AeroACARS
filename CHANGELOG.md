# Changelog

Alle nennenswerten Änderungen an AeroACARS. Format: lose an [Keep a Changelog](https://keepachangelog.com/) angelehnt; Versionsnummern folgen [Semantic Versioning](https://semver.org/) (Patch: Bugfix, Minor: Feature, Major: Breaking).

---

## [v0.12.4] — 2026-05-21 · Score-Konsistenz Pilot-App ↔ VPS-Web

Spec: `docs/spec/v0.12.4-score-consistency.md` (QS R0→R10).

Derselbe Flug zeigte im VPS-Web unter „Reports" und in der „Landung-Analyse" **unterschiedliche Landungs-Scores**. Ursache: die Webapp rechnete die Sub-Scores server-seitig aus den Touchdown-Roh-Feldern **neu**, statt die vom Pilot-Client gelieferten PIREP-Werte zu nutzen — und einige dieser Roh-Felder waren stale oder falsch berechnet. Ab v0.12.4 ist der Pilot-Client die **einzige Score-Wahrheit**: er scort beim Flug, der Recorder propagiert `landing_score` + `sub_scores` von der PIREP- auf die verknüpfte Touchdown-Zeile, und die Webapp **rendert nur noch** — kein Recompute mehr in fünf Ansichten. Damit zeigen Pilot-App und VA-Web garantiert denselben Score.

Begleitend zwei Touchdown-Roh-Feld-Bugs gefixt: (1) `rollout_distance_m` wurde im `touchdown_complete`-Event ~9 s nach dem Aufsetzen als Mitten-im-Ausrollen-Snapshot eingefroren — der Client schickt jetzt nach der Rollout-Finalisierung (~40 kt / Heading-Turn-off) ein nachgelagertes `touchdown_rollout_finalized`-Event mit dem finalen Wert; (2) `fuel_efficiency_pct` im MQTT-Touchdown-Payload rechnete fälschlich aus Block-Fuel (inkl. Taxi-out) statt aus dem Trip-Burn (`takeoff − landing`). Die Rollout-Finalisierungs-Logik selbst (40 kt / 30° Turn-off / 5 kt Floor) war bereits korrekt — nur veraltete Code-Kommentare wurden mitkorrigiert. Neu außerdem: die diskrete Landungs-Kategorie (Smooth/Acceptable/Firm/Hard/Severe) erscheint in den Webapp-Listen wieder als kleine Wort-Pill. Für Piloten ändert sich an der Bewertung **nichts** — der Score-Algorithmus ist unverändert; v0.12.4 stellt nur die Konsistenz zwischen Pilot-App und VA-Web her.

## [v0.12.3] — 2026-05-21 · Landing-G FOQA-konforme Messung

Spec: `docs/spec/v0.12.3-landing-g-foqa-measurement.md` (SPEC ACCEPTED, QS R0→R5).

Die Aufsetz-G-Kraft wurde bisher als **roher 50-Hz-Einzelframe-Peak** gescort — schärfer, als ihn ein echter Flugschreiber je aufzeichnen würde (Befund TAP533: 1.95 g für eine an sich saubere Landung). Ab v0.12.3 wird die G-Kraft wie eine echte Flugdaten-Überwachung (FOQA/FDM) gemessen: ein framerate-unabhängiger EMA-Filter (τ ≈ 100 ms) glättet das G-Signal leicht, dann wird der Spitzenwert über das Touchdown-Fenster genommen. Der so gescorte Wert (`scored_g`) speist konsistent **alle** Konsumenten — `sub_g_force`-Score, Touchdown-Klassifikation, G-Force-Forensik-Card, Activity-/ACARS-Text, das phpVMS-PIREP-Feld „Landing G-Force", QuickFlags, das RunwayDiagram, das `LandingScored`-Event und die VPS-Webapp. Der rohe 50-Hz-Peak (`peak_g_load`) bleibt überall als Forensik-Detail erhalten — backward-kompatibel, alte PIREPs/JSONL-Logs deserialisieren unverändert. Die Accident-/Crash-Erkennung nutzt bewusst weiter den rohen Extremwert (Extremwert-Detektion, kein faires Scoring). Methode ist sim-agnostisch — identisch für MSFS und X-Plane. Begleitend: die G-Force-Forensik-Card (Pilot-Client + VPS-Webapp) wurde inhaltlich überarbeitet (gescorter Wert als Headline, Roh-Werte als Forensik-Detail), das `aeroacars-live`-Repo zog parallel nach.

## [v0.12.2] — 2026-05-20 · X-Plane Aircraft-DataRef-Profile (CL650) · Discord-Push-Diagnose

Zwei Streams. Spec: `docs/spec/v0.12.2-xplane-aircraft-dataref-profiles.md`.

**Stream 1 — X-Plane Aircraft-DataRef-Profile (echte Lösung für Study-Level-Add-ons).** Study-Level-X-Plane-Add-ons wie die Hot-Start Challenger 650 fahren Cockpit-/Systemfunktionen über eigene DataRefs und bedienen die Standard-`sim/...`-DataRefs nicht — AeroACARS sah die CL650-Flaps nie (GSG225-Befund). v0.12.1 hat das fail-soft abgefangen; v0.12.2 liefert die echte Lösung. Ein Profil-System erkennt ein bekanntes Study-Level-Flugzeug über den Web-API-Flugzeugtitel **oder** eine RREF-Probe auf einen Signatur-DataRef und abonniert dann dessen add-on-eigene DataRefs. Erstes Profil: die Hot-Start Challenger 650 — Flaps (Hebel 0/20/30/45, gegen die Hersteller-Doku `Wires.txt` verifiziert), Battery-Master, Beacon- und Taxi-Light. LANDING-CONFIG-Prüfung und Approach-Stability-Scoring funktionieren damit für die CL650 wieder voll. Bei einem Flugzeugwechsel zur Laufzeit fällt der Adapter automatisch auf den Standard-Katalog zurück; Flugzeuge ohne Profil bleiben unverändert. Code-QS-Findings R4 (P1: Reset ohne Web-API-Titel) + R5 (P2: stale Titel überstimmt retirete Probe nicht) eingearbeitet.

**Stream 2 — Discord-Push-Diagnose.** Piloten meldeten, die Discord-Live-Presence verschwand nach den Updates, während der Test-Button weiter funktioniert (= Verbindung ok, aber der Flug-Push landet nicht). Der Push-Pfad verschluckte jede Fehlermeldung still. Das Discord-Panel (Einstellungen → Discord) zeigt jetzt „Letzter Discord-Push: vor X s" plus die letzte Fehlermeldung — bleibt die Zeit während eines Flugs stehen, kommt der Push nicht durch. Reine Anzeige/Logging-Erweiterung, kein Wire-Format-Wechsel.

## [v0.12.1] — 2026-05-20 · Pilot-Befunde · Phase-FSM, VA-Härtung, Approach-Stability, Autostart, Resume

Fünf unabhängige Befunde aus dem Piloten-Feedback eines Tages, in einer Release zusammengefasst. Spec: `docs/spec/v0.12.1-phase-fsm-fix-and-va-hardening.md`.

**Stream A — Phasenwechsel Boarding→Pushback gegen Sim-Reload-Glitches gehärtet.** Am Flug BTX8815 sprang die Phase 65 s nach Flugstart auf „Pushback" — ausgelöst von einem einzelnen Telemetrie-Glitch direkt nach einem Sim-Reload (kurzer Geschwindigkeits-Spike, obwohl das Flugzeug mit gesetzter Parkbremse stillstand). Der Übergang verlangt jetzt **echte Bewegung**: Geschwindigkeit über zwei aufeinanderfolgende Messungen plus eine echte Positionsverschiebung von mindestens 5 m. Zusätzlich erkennt der Recorder eine Reload-Lücke (> 10 s ohne Telemetrie) und verwirft das erste, glitch-anfällige Sample danach. Folgefehler behoben: die Off-Block-Zeit (und damit die Blockzeit im PIREP) stimmt wieder.

**Stream B — VA-Härtung: nur aktive GSG-Piloten.** Login, Live-Tracking und PIREP-Provisioning prüfen jetzt den Piloten-Status aus phpVMS. Nur ein **aktiver** Account kommt durch — wartende, abgelehnte, beurlaubte, gesperrte oder gelöschte Accounts werden mit einer status-spezifischen Meldung abgewiesen. Die Durchsetzung läuft client- **und** server-seitig (Recorder). Hintergrund: GitHub-Issue #16.

**Stream C — Approach-Stability fairer bei Study-Level-Add-ons.** Auf Flug GSG225 (Hot-Start Challenger 650, X-Plane) zeigte die Approach-Stability-Card „LANDING CONFIG: INCOMPLETE", obwohl Gear und Flaps gesetzt waren — das Study-Add-on bedient die Standard-Flaps-DataRef nicht. Lässt sich die Flaps-Stellung nicht zuverlässig lesen, wird die Landing-Config jetzt als „nicht bewertbar" angezeigt statt fälschlich rot — und fließt nicht als Strafe in den Score. Außerdem: der Kennwert „max. V/S-Abweichung < 500 ft" ignoriert jetzt Werte unter 50 ft AGL — ein kurzer Ballon im Flare ist kein Anflug-Stabilitätsproblem mehr.

**Stream D — Auto-Start nach Update repariert (macOS).** Nach einem App-Update griff der Auto-Start nicht mehr, bis man ihn einmal aus- und wieder einschaltete. Ursache: ein Frontend-Timing-Fehler überschrieb den gespeicherten Auto-Start-Status beim Start. Behoben — der gespeicherte Stand bleibt jetzt erhalten.

**Stream E — Resume nach Crash sicherer.** Nach einem Sim-Crash lädt das Flugzeug oft an einer beliebigen Boden-Position neu. Bisher übernahm der Resume diese Glitch-Position nach einem 10-Sekunden-Countdown automatisch in den laufenden Flug. Jetzt: passt die Sim-Position nicht zum gespeicherten Flug (Flug war in der Luft, Sim meldet Boden — oder große Distanz-Abweichung), wird **nicht automatisch** fortgesetzt — der Pilot muss aktiv „Flug fortsetzen" bestätigen.

Webapp-Mirror (`aeroacars-live`) zieht die Stream-B-Recorder-Härtung nach.

---

## [v0.12.0] — 2026-05-20 · Bahn-Auslastung · Float-Toleranz fürs faire Brems-Scoring

**Pilot-Beschwerde (BTX8815, Fenix A319, EDVE→LOWS):** exzellent gebremst — kurze Ausrollstrecke (442 m) — und trotzdem nur 80 PT „Bahn-Auslastung". Nachgerechnet: der Score war nach der v0.10.0-Formel korrekt, ABER die Formel gewichtete die Float-Strecke (zu spätes Aufsetzen) 1:1 wie die Ausrollstrecke (das eigentliche Bremsen). Bei diesem Flug waren 55 % der „Bahn-Auslastung" Float — die fehlenden 20 PT kamen vom späten Aufsetzen, nicht vom Bremsen.

**Float-Toleranz (15 % der LDA):** Float innerhalb der ersten 15 % der nutzbaren Bahnlänge gilt jetzt als normal und kostet keine Punkte — nur der Float-Überschuss darüber fließt ins Punkte-Banding ein. Die Ausrollstrecke zählt weiterhin voll. Damit trennt der Score faire Aufsetz-Toleranz von echter Brems-Disziplin. Der genannte Flug springt von 80 auf 100 PT.

**Neue Begründung „long_float":** wenn der Punktverlust ausschließlich vom späten Aufsetzen kam (die Ausrollstrecke allein wäre exzellent gewesen), zeigt die Card jetzt „Bremsweg top — aber spät aufgesetzt" statt einer brems-kritischen Begründung — mit Coach-Tipp Richtung Aim-Point.

**UI:** die Bahn-Auslastungs-Card rendert ihre Info-Zeilen (Aufsetzpunkt, Ausrollstrecke, Bahn) jetzt sprach-lokalisiert (DE/EN/IT). Das „🛬 Wie wird das berechnet?"-Modal erklärt die Float-Toleranz mit einer konkreten Beispiel-Rechnung. Terminologie bereinigt: die Ausrollstrecke endet beim Abrollen von der Bahn (~40 kt), nicht beim Stillstand.

**Sicherheit unverändert:** das Overrun-Risiko (zu nah am Bahnende) wird weiterhin auf der echten, ungekürzten Gesamtdistanz geprüft — die Float-Toleranz versteckt kein echtes Overrun.

**Score-Versionierung:** `score_algorithm_version` 2 → 3. Forward-only — alte PIREPs behalten ihren gespeicherten Score, es wird nichts rückwirkend neu gerechnet. In der „Meine Flüge"-Historie kann dadurch zwischen v2- und v3-Flügen ein kleiner Knick bei der Bahn-Auslastung sichtbar werden; das ist erwartet.

Spec: `docs/spec/v0.12.0-runway-utilization-refinement.md`. Webapp-Mirror (`aeroacars-live`) zieht die identische Score-Logik nach.

---

## [v0.11.2] — 2026-05-18 · Pilotenwunsch · Touchdown-Chart auch 3 s nach TD

**Pilotenwunsch (ViolonC im Discord):** das Sinkrate-Chart im Landing-Detail soll nicht hart am TD-Punkt enden, sondern auch die ersten 3 Sekunden nach dem Aufsetzen zeigen (Strut-Compression + Rebound + Stabilisierung).

Fix: `build_landing_record` hängt jetzt zusätzlich zu den Pre-TD-Samples (aus `snapshot_buffer`) auch die ersten 3000 ms aus `post_touchdown_buffer` an das `touchdown_profile`-Array. Backend sampled post-TD bereits seit v0.5.39 mit 50 Hz — die Daten lagen bisher nur als lokale JSONL-Forensik-Datei vor, jetzt erscheinen sie im sichtbaren Chart.

Chart-Code unverändert (`VsCurveChart` in `LandingPanel.tsx` rendert dynamisch die gesamte Profile-Range — sobald das Array t_ms > 0 enthält, wird die x-Achse automatisch erweitert).

---

## [v0.11.1] — 2026-05-18 · Hotfix · client_version im Touchdown-/PIREP-Payload

**Hotfix für v0.11.0.** UI-Render-Code für die Pilot-Client-Version-Pill war korrekt, aber das `client_version`-Feld wurde nie im Touchdown-Payload oder PIREP-Payload mit-gesendet — es lag nur im `FlightMeta` der Connect-Message und landete deshalb nie in der DB-Row. Resultat: leere Pill für alle Flüge.

Fix: `client_version: Some(env!("CARGO_PKG_VERSION"))` zu `TouchdownPayload` und `PirepPayload` hinzugefügt + an den jeweiligen Build-Stellen gesetzt. Pill wird für alle Flüge die **nach** dem v0.11.1-Update eingereicht werden korrekt befüllt.

Plus: Webapp zeigt die Pill jetzt auch in der **Reports-Übersicht** auf jeder PIREP-Karte (vorher nur im Detail-Modal beim Klick auf eine Karte) — VA-Owner kann die Version-Verteilung der Piloten direkt aus der Liste scannen.

---

## [v0.11.0] — 2026-05-18 · Pilot-Hilfen, Approach-Stability-Card, Loadsheet-Polish, Settings-Tabs

**Public Release** für alle Piloten. Stable-Updater greift das automatisch. Sammelt drei größere UI-Erweiterungen, einen wichtigen Bug-Fix und eine neue VPS-Diagnose-Anzeige in einer Release.

### Neu — Pilot-Hilfen für die Landeauswertung

- **🛬 Bahn-Auslastung — In-App-Erklärung.** Neuer "🛬 Wie wird das berechnet?"-Button am Boden der rollout-Sub-Score-Card öffnet ein Modal mit Formel, allen fünf Punkte-Bändern (excellent_margin … overrun_risk) inkl. Punktzahlen + Farben, Heavy-Bonus (5 pp für Wide-Bodies), Pre-Displaced-Cap (max 55 PTS bei zu frühem Touchdown), sechs Skip-Reasons und der Erklärung was die Card-Felder bedeuten. Pilot kann selbständig nachvollziehen warum sein Score so ist — kein Discord-Nachfragen mehr. (`client/src/components/RunwayUtilizationHelpModal.tsx`)
- **🛬 Glossar-Modal jetzt mehrsprachig (DE/EN/IT).** Bislang war das Glossar im Runway-Diagramm hartcodiert deutsch. Jetzt komplett über i18n — 18 Begriffe (Threshold, TDZ, Aim Point, TCH, DDS, Glideslope, Rollout, AIRAC, AGL/fpm/kt etc.) in allen drei Sprachen. (`client/src/components/RunwayGlossaryModal.tsx`)

### Neu — Approach-Stability-Card (analog zur aeroacars-live-Webapp)

- **7-Kacheln-Approach-Stability statt schmaler 2-Wert-Indicator.** Im LandingPanel wird der bisherige `StabilityIndicator` (nur σ-V/S + σ-Bank) durch eine vollwertige Card abgelöst:
  - **STABLE-GATE-Pill** rechts oben (✓ STABLE / ⚠ PARTIAL / ✗ UNSTABLE — Schwellen: 0 Verletzungen = stable, ≥2 hart oder ≥3 gesamt = unstable, sonst partial)
  - Sub-Line mit HAT/AGL-Filter + Sample-Count
  - Datenquellen-Zeile (MSFS / X-Plane)
  - **7 Kacheln**: V/S-Jerk · Bank σ (filtered) · IAS σ · Sink Rate · Landing-Config · V/S vs. 3°-ILS · Max V/S-Dev <500ft — jede Kachel hat farbiges Band (good/ok/bad/missing) basierend auf den FAA-AC-120-71B-Toleranzen aus dem Backend (`compute_approach_stability_v2`)
  - **Coaching-Banner** am Boden, dynamisch (sauber / partial / unstable)
  - **ⓘ Hilfe-Modal** "Was bedeuten die Werte?" erklärt das Stable-Gate-Konzept, die Pill-Logik und jede einzelne Kennzahl mit Schwellwert-Bändern (DE/EN/IT). (`client/src/components/ApproachStabilityCard.tsx`, `ApproachStabilityHelpModal.tsx`)
- **Backend-Storage erweitert (forward-only).** Drei bereits gerechnete Werte (`approach_vs_deviation_fpm`, `approach_max_vs_deviation_below_500_fpm`, `approach_used_hat`) waren bisher nur im MQTT-Payload — jetzt auch im lokal persistierten `LandingRecord`. Alte `landing_history.json`-Einträge bleiben via `serde(default)` lesbar; ihre Stability-Card zeigt einen Legacy-Hinweis statt der Kacheln. (`client/src-tauri/crates/storage/src/lib.rs`, `client/src-tauri/src/lib.rs`)

### Neu — Loadsheet-Bereich modernisiert ("Plan-Treue"-Score)

- **Treibstoff- und Gewichts-Tabellen nebeneinander** als 2-Spalten-Grid (auto-fit, stapelt auf schmalen Screens). Pro Zeile: IST groß und prominent, Plan-Wert als dezente Sub-Zeile darunter (`"Plan 13884 kg"`), Δ als runde Pill mit Trend-Icon (`✓` / `≈` / `▲` / `▼`).
- **Loadsheet-Score-Footer wurde zu einer Hero-Card** mit SVG-Donut-Ring (Mount-Animation, Gradient, Glow), die fünf Plan-vs-Real-Pills (Block-Fuel · TOW · LDW · ZFW) rechts daneben.
- **Section-Naming aufgeräumt** — die übergeordnete Sektion heißt jetzt **"Loadsheet"** (statt zwei mal "TREIBSTOFF" verschachtelt), die Sub-Cards bleiben "⛽ TREIBSTOFF" und "⚖️ GEWICHT", die Footer-Card "📋 PLAN-TREUE" (vorher "Loadsheet-Bewertung").
- **Plan/Real-Treibstoff-Balken** bekommt eine echte farbige Δ-Pill statt des bisherigen Mini-Texts.

### Neu — Settings in 5 Tabs sortiert (statt einer endlos langen Liste)

Settings-Panel wurde komplett reorganisiert in:
- **🎮 Simulator** — nur die Sim-Auswahl, prominent und isoliert (Default-Tab für Erst-Nutzer)
- **🛫 Benötigt** — SimBrief Integration, Flug-Aufzeichnungs-Verhalten (Auto-File, Auto-Start, Approach-Advisories)
- **🎨 Komfort** — Sprache & Darstellung, Verhalten (Minimize-to-Tray), Speicher (Flight-Logs), Fehler-Reporting, Discord-RPC
- **✈️ Plugins** — PMDG Premium Telemetry, X-Plane Premium Plugin (vorher unter Debug-Mode versteckt, sind aber keine Debug-Tools)
- **🛠 Technik** — Debug-Mode + Sim-Debug-Panel + phpVMS-Heartbeat + Orphan-Flights-Cleanup

Tab-Wahl wird in localStorage gemerkt. UI bekommt Hint-Text unter der Tab-Bar der jeweils erklärt was der Tab umfasst. Alle Labels in DE/EN/IT.

### Bug-Fix — VFR-Eingabe-Maske verschwindet bei Resize

VFR-Modal (ManualFlightModal) verlor alle eingetippten Werte (Block-Fuel, Flight-Time, Cruise-Level, Route, ZFW etc.) wenn der Pilot mitten in der Eingabe das Fenster verkleinerte/vergrößerte. Zwei defensive Fixes:
- **Form-State wird in localStorage persistiert** (Key `aeroacars.vfr.<bid_id>`) — überlebt jeden React-Remount, Resize, oder kurzen App-Reload. Werte werden nach erfolgreichem Flug-Start oder explizitem Cancel gelöscht.
- **Backdrop-Click härter** — schließt das Modal nur noch im `aircraft`-Stage (vor Aircraft-Auswahl). Im `plan`-Stage (Pilot tippt Werte) wird ein versehentlicher Backdrop-Touch/Klick ignoriert; nur der explizite Cancel-Button schließt. (`client/src/components/ManualFlightModal.tsx`)

### Neu — Pilot-Client-Version im VPS-PIREP-Report

- `client_version` aus dem MQTT-Payload (= `CARGO_PKG_VERSION` des Pilot-Clients, schon seit v0.7.x mitgesendet) wird jetzt im Webapp-PIREP-Detail-Header als kleine Pill angezeigt — `📱 AeroACARS v0.11.0`. VA-Owner sieht auf einen Blick mit welcher Client-Version ein PIREP produziert wurde — wichtig wenn Anomalien diagnostiziert werden müssen. (`aeroacars-live/webapp/src/components/LandingAnalysis.tsx`)

### Tools / QS

- **Neues i18n-Audit-Script `scripts/i18n-audit.mjs`.** Drei Checks: (1) PARITY zwischen DE/EN/IT, (2) alle im Code referenzierten `t("…")`-Keys existieren im EN-Master, (3) DEAD-Keys-Warnung. Plural-aware (`_one`/`_other`), Template-aware (`\`prefix.${...}\``), String-Literal-aware (`"foo.bar.baz"` als Konstante), Underscore-Suffix-aware (`\`prefix_${...}\``). Aufruf: `node scripts/i18n-audit.mjs [--strict]`.
- **3 i18n-Bugs gefunden + gefixt:** `sim.xplane_phase2` fehlte in allen Locales (Pilot sah rohen Key auf X-Plane-Panel); `settings.delete_all_logs_done` nutzte einen `defaultValue`-Hack statt sauberer i18next-V4-Plural-Resolution; `actions.language_it` war nur in IT vorhanden (Orphan).
- **12 echte Altlasten gelöscht:** `phase_timeline.*` (Feature seit Längerem entfernt, Locale-Keys standen noch da).

### Nicht in dieser Release — folgt in v0.12.0

- **Push-Nachrichten-System** (Admin-zu-Pilot-Inbox mit Bestätigung + Targeting) — bekommt eigene Spec-Runde, ist scope-mäßig ein eigenständiges Feature mit Recorder-DB-Schema, Webapp-Admin-UI und Client-Polling + Inbox-Component.
- **Loadsheet-Score-Algorithmus-Politur** (z.B. Penalty-Schwellen-Refinement) — der Score selbst ist v0.3.0-Stand; nur die Visualisierung wurde modernisiert.

---

---

## [v0.9.2] — 2026-05-18 · GlitchTip + Discord Rich Presence (Public-Release)

**Inhaltlich identisch mit v0.9.1.** Nochmal versioniert weil v0.9.1 selbst nach 6 QS-Runden noch einen Bug hatte (F18, siehe Runde 7 unten) und v0.9.0/v0.9.1 schon mehrfach als Draft existierten. Sauberer Cut.

**Runde 7 (F18) nach 7. QS-Pass:**
- **F18 (P1):** Discord-RPC-Toggle blieb nach AeroACARS-Neustart auf „Deaktiviert" obwohl Checkbox in den Settings AN war. Ursache: `init()` uebergab persistierte Settings an `Manager::new()` (= setzte internen Zustand bereits auf `enabled=true`), danach `apply_settings(same)` matchte den `(was_enabled=true, new=true)`-No-Op-Branch und rief nie `enable()` → Pipe nie geoeffnet → Status blieb Disabled obwohl UI an war. Stiller Feature-Verlust.
  - **Fix (Primaer):** Manager mit `DiscordPresenceSettings::default()` (= `enabled=false`) starten, dann persistierte Settings via `apply_settings()` nachschieben → triggert sauberen `(false → true)`-Transition → `enable()` → Discord-Pipe wird wirklich geoeffnet.
  - **Fix (Defense-in-Depth, neu in v0.9.2):** `apply_settings` prueft jetzt zusaetzlich `client_bound` und schiesst bei `enabled=true && !client_bound` ein `enable()` ab — egal welcher Match-Arm sonst greifen wuerde. Damit ist die Funktion robust gegen alle Init-Races, transienten Connect-Fails oder zukuenftige Code-Pfade die Settings ohne saubere State-Sync setzen. Macht F18 nicht reproduzierbar, selbst wenn jemand spaeter die init-Logik versehentlich wieder bricht.

## [v0.9.1] — 2026-05-18 · INTERN (Draft, nicht publiziert wegen F18)

7-runden-QS-Cycle. Vor Publish-Klick wurde F18 entdeckt → eskaliert auf v0.9.2.

## [v0.9.1-original] — 2026-05-18 · GlitchTip + Discord Rich Presence (initiales Public-Release nach QS)

**Inhaltlich identisch mit dem internen v0.9.0 das nie produktiv ging.** Versionsnummer-Sprung auf 0.9.1 weil ein interner v0.9.0-Build kurzzeitig (~15 min, 0 Downloads) als `releases/latest` sichtbar war — die Nummer brennt damit, fresher Public-Release startet bei 0.9.1.

### Zusaetzlich zu v0.9.0-Inhalten — QS-Hotfix-Findings F1-F11 bereinigt:

**Runde 1 (F1-F8):**
- **F1:** Release-Workflow auto-publishte den Draft sofort nach Build → jetzt `if: false`-Gate, Publish muss manuell in der UI geklickt werden
- **F2:** Discord-RPC zeigte fuer Phasen `HOLDING` + `PIREP FILED` faelschlich „PREFLIGHT" → 20 Phasen jetzt komplett gemappt + Regression-Tests
- **F3:** Sentry-Opt-Out rief `flush()` (= pushed Events AKTIV raus statt zu verwerfen) → entfernt
- **F4:** Tag `route` war in 4 Allowlists aber UI sagte „Route NICHT gesendet" → konsistent geloescht
- **F5:** UI-Text sagte `live.kant.ovh` statt korrekt `tip.kant.ovh` (DSGVO-Consent-Konsistenz)
- **F6:** Frontend-Sentry hatte `integrations: []` → deaktivierte alle Default-Integrations (BrowserApiErrors, GlobalHandlers, Breadcrumbs); jetzt aktiv
- **F7:** `set_sim_lost`-Code existiert aber kein Caller → in Runde 5 dann doch verdrahtet (siehe F16)
- **F8:** CHANGELOG-Behauptung „18 Phasen" → korrigiert auf 20 Phasen (17 FSM-aktiv + 3 v0.10.0-ready)

**Runde 2 (F9-F11) nach 2. QS-Pass:**
- **F9:** Sentry-Opt-Out-Rest-Risiko — Atomic-Gate verhinderte zwar kuenftige Events, aber im Transport-Buffer pending events haetten noch beim naechsten Tick rausgehen koennen → jetzt `Hub::current().bind_client(None)` droppt den Transport hart, Buffer-Inhalt geht verloren statt gesendet zu werden. **DS7 hart erfuellt: „ab Klick geht nichts mehr raus".**
- **F10:** Webapp-Allowlist hatte noch `ui.route` als Backdoor fuer spaeter-versehentliches Route-Tag-Setting → entfernt
- **F11:** Code-Kommentar-Drift in `sentry_init.rs` (Kommentar referenzierte `Hub::end_session()`, Code rief es nicht) → Kommentar synchronisiert auf tatsaechliche `bind_client(None)`-Implementierung

**Runde 3 (F12) nach 3. QS-Pass:**
- **F12:** Asymmetrischer Opt-In nach Opt-Out — F9 droppte den Client hart bei Opt-Out, aber Opt-In schaltete nur die Atomic wieder an ohne Client neu zu binden. Pilot der "aus → an" klickte hatte native Crash-Reports kaputt bis App-Neustart. Settings-Hint sagte aber "wirkt sofort, kein Neustart noetig". Fix: `build_options()` aus `init()` extrahiert, `set_consent(true)` baut bei fehlendem Client einen neuen via `Client::from(options)` + `Hub::bind_client(Some(...))` + Initial-Scope. Aus-an-aus-an funktioniert jetzt symmetrisch in einem App-Run, ohne Neustart.

**Runde 4 (F13-F15) nach 4. QS-Pass:**
- **F13 (P1):** F9-Implementierung war architektonisch unvollstaendig — der `SENTRY_GUARD: OnceLock<Option<ClientInitGuard>>` hielt eine `Arc<Client>`-Referenz dauerhaft am Leben. `Hub::bind_client(None)` unbindet nur vom Hub, aber der Guard hielt weiter. Pending Events im Transport-Buffer haetten beim Guard-Drop ueber `Client::close()` drainen koennen. DS7-Garantie nicht belastbar. **Fix:** Kompletter Lifecycle-Umbau:
  - `SENTRY_GUARD` (=`ClientInitGuard` im `OnceLock`) entfernt
  - Neuer `SENTRY_CLIENT: OnceLock<Mutex<Option<Arc<Client>>>>` — wir kontrollieren die Arc-Referenz selbst
  - `init()` ruft `create_and_bind()`: baut `Client::from(options)`, bindet via `Hub::bind_client(Some(Arc::clone))`, speichert Arc im Slot
  - `set_consent(false)`: `slot.take()` → unsere Arc weg + `client.close(Some(ZERO))` → Transport-Worker signalisiert Pending-Queue zu verwerfen + `Hub::bind_client(None)` → Hub-Referenz weg. Arc-Refcount geht auf 0, Drop laeuft, Transport-Worker-Thread terminiert sauber.
  - `set_consent(true)`: `create_and_bind()` — no-op wenn Client schon da, sonst Neu-Bau.
- **F14 (P2):** Release-Notes verlinkten `docs/spec/v0.9.1-*` (existiert nicht — Specs heissen `v0.9.0-*`). Fix: alle 4 Spec-Links pro Sprache zurueck auf `v0.9.0-*`.
- **F15 (P2):** Known-Issue-Block sagte „sim-lost-Suffix kommt in v0.9.1" — aber v0.9.1 ist genau dieses Release. Fix: auf „kommt in einem spaeteren v0.9.x-Release (vorgesehen v0.9.2)" korrigiert. Inzwischen ueberholt durch F16-Implementierung.

**Runde 5 (F16) nach 5. QS-Pass:**
- **F16 (F7-Auflösung, P2 → done):** statt F7 als Known-Issue auf v0.9.2 zu verschieben, doch noch in v0.9.1 verdrahtet. Spec-LE8 ist damit voll implementiert.

**Runde 6 (F17) nach 6. QS-Pass:**
- **F17 (P0):** F13-Refactor hatte `Client::from(options)` direkt benutzt, dabei aber `sentry::apply_defaults(options)` ausgelassen. `apply_defaults` setzt den Default-Transport (reqwest-basiert), die Default-Integrations (Panic, Backtrace, Contexts) und Env/Proxy-Defaults. **Ohne diesen Call hatte der Client KEINEN Transport — Events gingen nicht raus, native GlitchTip war faktisch tot.** Lifecycle-Fix von F13 war OK, aber halbfertig. Fix: `let options = sentry::apply_defaults(options)` vor `Client::from()`. Zusaetzlich `client.is_enabled()` als Boot-Log + Warn-Log wenn false (= sichtbar in journalctl/console wenn die Build-Konfig was vermisst).
  - Neuer Tauri-Command `discord_rpc_set_sim_lost(lost: bool)` → ruft `Manager::set_sim_lost`
  - Frontend-Hook `useDiscordRpcPush` hat zweiten useEffect der auf `simStatus.state`-Aenderungen reagiert: `lost = simStatus.state !== "connected"`, ruft Command. Backend-Manager dedupliziert intern.
  - Wirkt waehrend aktivem Flug — kein Flug = kein Suffix sinnvoll
  - Known-Issue-Block dazu entfernt (im aktuellen Release keine offenen Spec-Punkte mehr)

## [v0.9.0] — 2026-05-18 · INTERN (nie publiziert, kurz als latest sichtbar)

Versionsnummer **verbrannt** wegen ~15-min-Sichtbarkeits-Fenster im `releases/latest` waehrend QS noch lief. Inhalt vollstaendig in v0.9.1 enthalten. Tag bleibt im Git-Log fuer Audit-Trail, keine Pilot-Distribution.

🚀 **Doppel-Feature-Release: Anonyme Fehler-Telemetrie an self-hosted GlitchTip + Pilot-Flugstatus live im Discord-Profil. Beide Features sind Opt-In, Default = aus, jederzeit per Toggle abschaltbar.**

### F-001 · Discord Rich Presence

Pilots können ihren aktuellen Flugstatus live ins Discord-Profil spiegeln. Andere VA-Mitglieder sehen so „Pilot X fliegt GSG3184 EDDB→KMRH CRUISE" direkt in der Mitglieder-Liste — ohne dass irgendjemand den Pilot-Client öffnen oder ins Webapp-Dashboard schauen muss.

- **Settings → Discord Rich Presence**: 3 Toggles
  - **Master-Toggle** (Default OFF, DSGVO-Opt-In)
  - **Callsign anonymisieren** ("GSG3184" → "GSG-Flight", Route bleibt sichtbar)
  - **"Profil öffnen"-Button anzeigen** (= phpVMS-Profil-Link in der Presence)
- **Live-Status**: grüner/grauer/roter Dot zeigt Verbindung zu Discord, alle 5s aktualisiert
- **Test-Presence-Button**: sendet 15s eine Dummy-Presence — Pilot kann verifizieren ohne echten Flug
- **20 Phasen** korrekt gemappt (kein UNKNOWN-Fallback): Preflight, Boarding, Pushback, Taxi-Out, Takeoff-Roll, Takeoff, **REJECTED-TAKE-OFF** ⚠ (v0.10.0-ready), Climb, Cruise, **Holding** (v0.5.11), Descent, Approach, Final, Landing, **GO-AROUND** ⚠ (v0.10.0-ready), Taxi-In, Arrived, Shutdown, **DEBOARDING** (v0.10.0-ready), PIREP-Filed. — Heutiger Rust-FSM emittiert 17 davon, die 3 v0.10.0-Phasen sind vorbereitet aber feuern erst mit dem Phase-Expansion-Release.
- **60s Heartbeat + sofortiger Update bei Phase-Wechsel**
- **Graceful Fallback**: wenn Discord nicht installiert oder offen → Status "NotFound", kein Crash, kein Toast-Spam
- **Wirkt sofort**: Toggle aus = Pipe wird geschlossen + Activity gecleart binnen 5s

#### Asset-Layout

- `large_image` = AeroACARS-Logo (Brand-Konsistenz)
- `small_image` = Sim-Badge unten-rechts (MSFS 2024/2020, X-Plane 11/12 — vier eigene Designs mit Aviation-Top-Down-Jet)
- Phase wird als Text in der Status-Zeile angezeigt, nicht als Icon (bei 30×30 px lesbar)

#### Architektur

- **Discord-App-ID NICHT im Client-Binary** — der VA-Owner pflegt sie einmal im Webapp-Admin (Settings → Discord → "Discord-Application-ID"), Pilot-Client zieht sie zur Laufzeit via Public-Endpoint nach. Vorteil: kein Re-Release wenn die VA die Discord-App wechselt, Forks funktionieren automatisch gegen die eigene VPS.
- Neuer Rust-Workspace-Crate `discord-presence` (~600 LOC + 24 pure-fn Tests)
- Settings persistieren in `<app_data_dir>/discord_rpc_settings.json` über App-Restarts

### F-002 · GlitchTip — anonyme Crash-Telemetrie

Self-hosted Sentry-kompatible Fehler-Sammelstelle. AeroACARS (Client) + Recorder (VPS) + Webapp (Admin-UI) senden anonymisierte Crash- und Error-Events automatisch hin, sodass der VA-Owner Bugs sieht **bevor** Pilots im Discord klagen.

- **Settings → Fehler-Telemetrie (anonym)**: 1 Toggle (Default OFF, DSGVO-Opt-In)
- **First-Run-Banner** beim ersten v0.9.0-Start mit klarer Erklärung was gesendet wird / was nicht
- **Privacy-Guarantien** (DSGVO Art. 6 (1) a):
  - **Was wird gesendet**: Crash-Stack-Traces, Sim-Name, Aircraft-ICAO, App-Version, OS
  - **Was wird NICHT gesendet**: Position, Route, Login, IP-Adresse, Passwörter, E-Mail
  - **Wohin**: VA-eigener self-hosted GlitchTip (`tip.kant.ovh`), kein 3rd-Party
- **Tag-Allowlist + Redaction** beidseitig (Rust + TS): selbst wenn anderer Code versehentlich PII setzt, wird es im `beforeSend`/`before_send`-Hook gestrippt
- **Self-hosted GlitchTip-Stack** auf der VPS: Docker-Compose (postgres + redis + web + worker), Caddy mit auto Let's-Encrypt-Cert, 4 Uptime-Monitore eingerichtet (Recorder + GlitchTip self + GSG-phpVMS + GSG-API)

### Telemetry-Contract

Beide Features halten sich an `docs/spec/v0.9.0-telemetry-contract.md` (Sektion 1.3 für die kanonischen Phasen, Sektion 9 für Datenschutz-Gates).

### Bekannte Einschränkungen v0.9.1

- **Phase-Expansion (REJECTED-TAKEOFF, GO-AROUND, DEBOARDING):** Spec definiert + Discord-Mapping vorhanden, aber Rust-FSM emittiert diese Phasen noch nicht — kommt mit v0.10.0 (Phase-Expansion-Release laut Roadmap). Heute werden 17 von 20 mappable-Phasen emittiert.

*(Der „Sim getrennt"-Suffix-Hinweis von vorher wurde mit F7/F16 in Runde 5 dieser Releases doch noch verdrahtet — keine known-issue mehr.)*

### Sonstiges

- **Webapp**: JSONL-Forensik-Importer lädt jetzt **lazy** statt automatisch — Settings-Tab öffnet sofort, Import-Section zeigt "📂 Dateien jetzt laden"-Button
- **i18n**: alle neuen UI-Strings DE/EN/IT
- **CI**: GH-Actions-Release-Workflow leitet `AEROACARS_SENTRY_DSN` und `VITE_SENTRY_DSN_CLIENT` an `tauri-action` weiter (signed Builds haben die GlitchTip-DSN eingebacken)
- **VPS-Deploy**: `deploy-recorder.sh` reicht `VITE_SENTRY_DSN_WEBAPP` vom env-File zum Vite-Build durch
- **Recorder fix**: `package.json`-Import in `src/index.ts` per `fs.readFileSync` statt static-`import-with-json` (vorher: tsc warf rootDir auf Repo-Root → dist landete unter `dist/src/`, systemd brach)

---

## [v0.7.17] — 2026-05-12 · Fenix-Polish + Bug-Bündel

🛠️ **Bug-Sammel-Release nach Tester-Feedback zu v0.7.16. Fenix-Profil ist jetzt default-on (kein Toggle mehr), Squawk + Aircraft-Type bei Fenix bereinigt, SimBrief-Refresh greift beim Flug-Start, Bahn-Auslastung-Score endlich aircraft-aware, Auto-Start sagt jetzt warum er nicht feuert.**

### F-001 · Fenix-Profil von Opt-in zu Default-on

- Beta-Toggle in Settings ist entfernt (Backend-Flag `fenix_beta_enabled` weg, Tauri-Commands raus, i18n-Strings raus)
- Bei erkanntem Fenix-Profil greifen die LVAR-Overrides automatisch (Landing-/Nose-/Wing-Light, Park-Brake etc.)
- localStorage-Key `fenix_beta_enabled` wird beim ersten Start aufgeräumt — keine Pilot-Aktion nötig

### B-001 · Aircraft-Type-Fallback bei Fenix

- Vorher: Activity-Log zeigte „Type ?" weil Fenix den Standard-`ATC MODEL`-SimVar nicht zuverlässig füllt
- Jetzt: `AircraftProfile::icao_fallback()` setzt bei FenixA319/A320/A321 den ICAO-Code aus dem Profile-Match (`A319`/`A320`/`A321`)
- Profile ohne eindeutigen Variant (Default, FBW, PMDG) behalten `None` — keine Phantasie-ICAOs

### B-002 · Squawk-Logging bei Fenix unterdrückt

- Standard-`TRANSPONDER CODE:1`-SimVar ist bei Fenix nicht mit dem cockpit-seitigen RMP synchronisiert (= zeigte falsche / eingefrorene Codes)
- Bis ein Fenix-eigener LVAR identifiziert ist, gibt der Snapshot bei `is_fenix()` jetzt `transponder_code: None` → keine falschen Squawk-Einträge mehr im Activity-Log und PIREP

### N-001 · SimBrief-Refresh greift jetzt beim Flug-Start

- Vorher: Pilot drückt im Bid-Tab „Aktualisieren" (zeigt frische SimBrief-Daten), klickt Flug-Start → `flight_start` ignoriert das und holt den alten OFP aus dem phpVMS-Bid-Pointer
- Jetzt: wenn der Pilot in Settings → SimBrief einen Identifier (User-ID oder Username) gesetzt hat, holt `flight_start` **zuerst** den aktuellsten OFP direkt von simbrief.com (mit DEP/ARR-Match-Verifikation); Fallback auf den Bid-Pointer nur wenn Direct fehlschlägt
- Identisches Verhalten wie der `flight_refresh_simbrief`-Pfad — Direct-First mit Pointer-Fallback

### N-002 · Bahn-Auslastung-Score aircraft-aware

- Vorher: Rollout-Schwellen waren absolute Meter (800/1200/1800/2500) → jeder Airliner mit 2 km Rollout bekam „long_rollout" / 25 Pkt, obwohl 2 km für einen A320 völlig normal sind
- Jetzt: 3 Aircraft-Kategorien (Light / Medium / Heavy) mit angepassten Schwellen:
  - Light (Default): unverändert
  - Medium (A32x-Family, B737, E170/190, CRJ, ATR, Dash-8 etc.): 1200/1800/2400/3000 m
  - Heavy (A330/340/350/380, B747/767/777/787, MD11): 1500/2300/3000/3800 m
- Aircraft-Klassifizierung via ICAO-Type-Designator-Lookup, robust gegen Whitespace / Groß-Kleinschreibung
- Beide Stellen (Pilot-Client Rust-Crate UND aeroacars-live Webapp) sind in Sync zu fixen — diese Version fixt den Pilot-Client; das Webapp-Repo bekommt einen separaten Patch

### N-003 · Auto-Start sagt jetzt warum er nicht feuert

- Vorher: 3 stille Skip-Pfade (`sim_data_warm`, `bids empty`, `no_bid_match`) → Pilot saß ratlos da, Watcher loggte nur Debug
- Jetzt: alle Skip-Pfade haben einen Activity-Log-Hint mit 60-Sekunden-Throttle:
  - **Aircraft-Titel fehlt** (X-Plane-spezifisch: „Web-API in Settings → Network einschalten"; MSFS: „Sim noch im Boot")
  - **Fuel = 0** (Sanity-Schwelle von 100 kg → 1 kg gelockert, damit Light-GA mit halbvollem Tank nicht ausgeschlossen wird)
  - **Keine Bids verfügbar** („eingeloggt?")
  - **Kein Bid matched aktuelle Position** (mit Entfernung zum nächsten Departure)
  - **`flight_start` failed** (Bid + Error-Code als Warn-Eintrag)

### N-004 · X-Plane Plugin-Version-Sync

- `xplane-plugin/CMakeLists.txt` hat seit Initial-Commit `VERSION 0.5.0` getragen, während `plugin.cpp` über 6 Patches (v0.5.3/.5.6/.5.8/.5.11/.5.13) ging
- Plugin loggte deshalb fälschlicherweise „v0.5.0" in X-Plane `Log.txt` — bei Bug-Reports verwirrend
- Jetzt: `VERSION 0.5.13` (= echter Code-Stand), per `target_compile_definitions` als Macro `AEROACARS_PLUGIN_VERSION` ins Plugin gezogen → Log meldet die Wahrheit
- Künftig synchron mit Code-Changes hochziehen

### Tests

- 15 neue Rust-Unit-Tests (sim-core: 1 icao_fallback, sim-msfs: 5 Fenix-Mapping + 3 ICAO-Fallback + 2 Squawk-Suppression; landing-scoring: 4 Bahn-Auslastung-Cases)
- `cargo test --workspace --lib`: alle grün
- `tsc -b` clean, `npm test` grün

### Garantien

- F-001: Nicht-Fenix-Aircraft sind unverändert (nur Profile-Check leitet ins Override)
- B-001/B-002: greifen nur bei `is_fenix()` Profile-Match
- N-001: SimBrief-direct nur wenn Identifier gesetzt; sonst Bid-Pointer-Pfad wie vorher
- N-002: Light-Schwellen identisch zu v0.7.16 → keine Regression für GA-Piloten
- N-003: Activity-Log-Spam verhindert durch existierenden 60-Sekunden-Throttle pro Reason-Code

### Tracker

Siehe [docs/qs/v0.7.16-fenix-beta-bugs.md](docs/qs/v0.7.16-fenix-beta-bugs.md) für die vollständige Bug-Sammlung und Diagnose-Spuren aus Tester-Feedback.

---

## [v0.7.16] — 2026-05-12 · Fenix A32x Cockpit-State (Opt-in Beta)

🧪 **Stable-Release mit neuem Opt-in Beta-Feature für Fenix A32x. Standardmäßig deaktiviert. Read-only, kein FSUIPC, keine MSFS Community-Folder-Änderungen. Wer ihn nicht einschaltet, fliegt bit-identisch zu v0.7.15.**

### Was die Version liefert

#### Fenix A319 / A320 / A321 Variant-Erkennung
- `AircraftProfile` erweitert um `FenixA319` und `FenixA321` (vorher liefen beide als `FenixA320`)
- Detection per Title-Substring + ICAO-Fallback (für Repaints ohne Variant-Suffix)
- Helper `AircraftProfile::is_fenix()` für alle drei Varianten
- Label-Differenzierung: „Fenix A319" / „Fenix A320" / „Fenix A321"

#### Additive Fenix-LVAR-Mappings (Opt-in)
Neu unter dem `fenix_beta_enabled`-Flag (Default off):
- `L:S_OH_EXT_LT_LANDING_L` + `_R` (3-Pos-Selektor: retracted/off/on) → `light_landing`
- `L:S_OH_EXT_LT_NOSE` (3-Pos: off/taxi/T.O.) → `light_taxi`
- `L:S_OH_EXT_LT_WING` (Wing-Inspection) → neu: `light_wing` für Fenix-Beta-User
- `L:S_OH_EXT_LT_RWY_TURNOFF` (Runway-Turnoff, read-only QS)
- `L:S_OH_EXT_LT_LANDING_BOTH` (Composite, Verifikation gegen L/R)
- `L:S_FC_FLAPS` (Flaps-Lever-Detent, read-only QS)

LVAR-Namen verifiziert gegen den **echten Fenix-Install** auf der Dev-Maschine — Quelle:
`SimObjects\Airplanes\FNX_32X\model\FNX32X_Interior.xml` in
`fnx-aircraft-320` / `fnx-aircraft-319-321`.

#### Feature-Flag-Infrastruktur
- `fenix_beta_enabled: AtomicBool` auf `MsfsAdapter::Shared`
- Tauri-Commands `set_fenix_beta_enabled` / `get_fenix_beta_enabled`
- Frontend-Toggle in Settings → Beta (DE / EN / IT lokalisiert)
- localStorage-Persistenz + Backend-Sync beim App-Mount
- Default off → bit-identisches Verhalten zu v0.7.15 Stable für alle Nicht-Beta-User

### Spec-Garantien

- ❌ Keine Writes, keine Steuerung, kein FMC-Zugriff
- ❌ Keine FSUIPC-Abhängigkeit
- ❌ Keine MSFS Community-Folder-Additionen (kein WASM-Modul, keine DLL)
- ✅ Read-only via plain SimConnect mit `L:`-Prefix
- ✅ Pilot muss nur AeroACARS-Update installieren + Schalter umlegen
- ✅ Bei fehlender LVAR: leise auf Standard-MSFS-SimVar zurückfallen, kein Crash

### Tests

- 12 neue Rust-Unit-Tests (7 in sim-core für A319/A320/A321-Detection + is_fenix-Helper; 5 in sim-msfs für Beta-On/Off-Mapping + Layout-Smoke-Test)
- `cargo test --workspace --lib`: 224/224 passed

### Verifikation

| Check | Status |
|---|---|
| `cargo check` (client/src-tauri) | ✅ |
| `cargo test --workspace --lib` | ✅ 224/224 |
| Spec-Pfad-Aktualisierung (`Cockpit_Behavior.xml` → `FNX32X_Interior.xml`) | ✅ |
| LVAR-Namen vs. echter Fenix-Install | ✅ |
| Stable-Verhalten (Beta aus) bit-identisch zu v0.7.15 | ✅ |

### Release-Regeln

Diese Version geht **als normales Stable-Release** über den Auto-Updater an alle Piloten. Das Fenix-Profil ist Opt-in und Default off, deshalb kein Risiko für die breite Nutzerbasis.

Stable-Übernahme der Fenix-LVAR-Mappings in den **Default-Path** (= ohne Opt-in) folgt frühestens, wenn das Beta-Feedback grün ist.

### Dokumente

- Spec: [docs/spec/fenix-a32x-cockpit-state-beta.md](docs/spec/fenix-a32x-cockpit-state-beta.md)
- QS-Guide: [docs/spec/fenix-a32x-beta-qs-guide.md](docs/spec/fenix-a32x-beta-qs-guide.md)

---

## [v0.7.15] — 2026-05-12 · Sim-Recovery Release

🎯 **Laufende Flüge überleben Simulator-Crash, Pause, Neustart oder kurze Rechner-Unterbrechungen sauber — ohne Datenloch, ohne Session-Split, mit korrekter Flight-Time.**

### Trigger

Real-Pilot-Incident **AUA 323 LOWW→ESGG am 2026-05-11** (PIREP `J2VoaZmoD6LQGpMg`): MSFS friert im Descent ein, ACARS pausiert nach 30 s, Pilot bemerkt es erst nach manueller Landung am Boden in ESGG. Resultat: zwei Sessions im History-Tab, Block-Time-Drift, AeroACARS-Recorder hat eine 23-min Lücke nicht überbrückt.

Diese Version ist ein **kombiniertes Sim-Recovery-Release** das diese Wurzel an drei Stellen anpackt: Pause-Handling, Session-Identität, Sim-Awareness.

### Was die Version liefert

#### Phase 1 (Client) — Auto-Resume + Pause-Akkumulator + `pirep_id`-Payload
- Streamer-Loop resumed automatisch sobald wieder Sim-Daten kommen — kein manueller „Flug fortsetzen"-Klick mehr nötig
- Manueller Resume-Button bleibt als Fallback
- Pause-Dauern werden in `pause_total_duration_secs` akkumuliert + an `pause_segments` angehängt (Audit-Log)
- Block-/Flight-Time im PIREP zieht akkumulierte Pause-Zeit ab (Heartbeat + File + Manual-Edit)
- Reposition-Distanz beim Resume wird NICHT in `distance_nm` addiert (`last_lat`/`last_lon` reset)
- `pirep_id` wird in jedem Position-MQTT-Payload mitgesendet
- **Heartbeat-Fix**: bei Sim-Disconnect ohne Snapshot wird trotzdem alle 30 s ein Heartbeat mit `last_good_snap` an phpVMS gesendet → PIREP bleibt unbegrenzt am Leben

#### Phase 2 (Server) — `pirep_id`-Join im `ensureSession`
- Recorder priorisiert `pirep_id` aus dem Payload VOR der Standard-Heuristik (callsign/dep/arr + Zeitfenster) → 23-min-Positions-Lücken erzeugen keine neue Session mehr
- 6h-Cutoff: ENDED-Sessions können nur innerhalb von 6h nach `last_seen` reopened werden
- Terminal-Schutz: ARRIVED/PIREP_SUBMITTED-Sessions werden NIE wiedereröffnet
- Backfill: ACTIVE-Sessions ohne `pirep_id` bekommen sie nachträglich angeheftet
- Frisch erstellte Sessions bekommen `pirep_id` direkt mit gesetzt

#### F5 — MSFS-Pause via SimConnect `Pause_EX1`
- SimConnect-System-Event `Pause_EX1` mit `dwData`-Flag-Set wird abonniert
- Aktive MSFS-Esc-Pause / Active-Pause / Sim-Pause werden sofort erkannt — kein 30 s-Warten auf Disconnect-Threshold
- Initial-State zuverlässig: wenn AeroACARS connectet während MSFS schon pausiert ist, kommt sofort ein initialer Pause_EX1-Event
- `SimSnapshot.paused` wird durchgereicht, der Streamer pausiert + akkumuliert
- Auto-Resume bei Pause_EX1-Event mit `dwData=0` ohne Pilot-Klick

#### F6 — X-Plane Pause + Replay-Modus
- Neue RREF-Subscriptions auf `sim/time/paused` + `sim/time/is_in_replay`
- `SimSnapshot.paused` wird aus beiden gespeist (Replay-Modus zählt als Pause-äquivalent)
- Funktioniert ohne X-Plane-Plugin-Update — RREF ist nativ, kein Protokoll-Bump nötig

#### F7 — Aircraft-Change-Warnung nach Recovery (MSFS + X-Plane ≥12.1)
- Beim Resume Vergleich `snap.aircraft_icao` vs. `flight.aircraft_icao` (Bid-Wert)
- Bei Mismatch: Activity-Log-Warn mit konkretem Hinweis („Sim meldet A320, Bid erwartet B738")
- Resume wird NICHT blockiert (Spec-Prinzip P2: informieren statt blockieren) — Pilot kann via PIREP-Cancel-UI korrigieren
- **Sim-Coverage:** MSFS via SimConnect (ATC MODEL); X-Plane via Web-API ab v12.1 (`sim/aircraft/view/acf_ICAO`). X-Plane <12.1 oder mit deaktivierter Web-API überspringt F7 still (keine falsch-positiven Warnungen)

### Datenmodell

`FlightStats` + `PersistedFlightStats` erweitert um (alle `#[serde(default)]`, forward-only):
- `pause_total_duration_secs: i64` — Summe aller Pause-Sekunden
- `pause_segments: Vec<PauseSegment>` — Audit-Daten pro Pause-Block (Start, Ende, Reason, Drift)
- `current_pause_reason: Option<PauseReason>` — aktive Reason für Resume-Helper
- `PauseReason` enum: `SimDisconnect` | `SimPause` | `ManualResume`

Pre-v0.7.15 `active_flight.json` lädt weiter — fehlende Felder defaulten auf `0` / leeren Vec / `None`.

### Tests

- 16 neue Rust-Unit-Tests (Block-Time-Saturating-Arithmetik, Drift-Schwellen-Monotonie, PauseSegment serde-Roundtrip, PausedFlightStats Backward-Compat, SimPause-Reason persistence)
- 5 Node-Test-Driver-Tests im Recorder (25-min-Gap, Terminal-Schutz, 6h-Cutoff, Legacy-Backfill, brand-new Session)
- `cargo test --lib`: 115/115 passed
- `npm test` im Recorder: 5/5 passed

### Verifikation

| Check | Status |
|---|---|
| `cargo check` (client/src-tauri) | ✅ |
| `cargo test --lib` | ✅ 115/115 |
| `npx tsc --noEmit` (recorder) | ✅ |
| `npm test` (recorder) | ✅ 5/5 |
| forward-only: pre-v0.7.15 `active_flight.json` lädt | ✅ via `serde(default)` |

### Companion Server-Deploy

Server-Patches sind in `aeroacars-live` (commits `92b22c6` + `0ffceca`) gepusht. Auf `live.kant.ovh` deployen via `deploy-recorder.sh`.

### Aus Scope BEWUSST raus (kommt später)

- F8 Bid-Change-Detection (nur Light-Check geplant gewesen, nicht fertig)
- Neue Toast-/Banner-UI-Architektur
- Drift-Linie auf Karte
- Vollständige 29-Szenarien-Pilot-QS-Matrix

### Spec-Referenz

Komplette Anforderungen + Akzeptanzkriterien: [`docs/spec/sim-disconnect-auto-resume.md`](docs/spec/sim-disconnect-auto-resume.md)

Trigger-Incident-Daten (PIREP `J2VoaZmoD6LQGpMg`) für späteren Forensik-Review.

---

## [v0.7.14] — 2026-05-12

🎯 **Discord-Posts laufen jetzt zentral vom VPS — Pilot-Client postet nichts mehr.**

### Warum

v0.7.13 hatte Pilot-Local-Webhook-URL eingeführt — Pilot pasted die URL in Settings. Problem: bei N Piloten = N Stellen wo das Token leakt + jeder Pilot konnte eine andere URL setzen (= Discord-Spam-Risiko).

Außerdem hat der Recorder auf live.kant.ovh **bereits seit Monaten** seine eigene Discord-Integration (Webapp-Admin → Settings → Discord-Webhook für Touchdown + PIREP-Posts). Pilot-Client postete zusätzlich → doppelte Posts für Landing + PIREP, plus zwei einzelne Events (Takeoff, Divert) die nur der Pilot-Client gepostet hat.

v0.7.14 räumt das auf: **Pilot-Client-Discord-Code komplett raus, Recorder ist die einzige Quelle**.

### Pilot-Client (~250 LOC raus)

- `client/src-tauri/src/discord.rs` komplett gelöscht
- `mod discord;` aus `lib.rs` raus
- 4 `discord::post_event(...)`-Aufrufe in `lib.rs` raus (Takeoff, Landing, PirepFiled, Divert)
- `discord_webhook_get` + `discord_webhook_set` Tauri-Commands raus
- Settings-Sektion „Discord-Integration" + i18n-Keys (DE/EN/IT) raus
- Migration: alte `<app_data_dir>/discord-webhook.txt` aus v0.7.13 wird beim ersten Start unter v0.7.14 automatisch gelöscht
- Cargo `discord-rich-presence` Dep blieb schon raus (v0.7.13)

### Recorder (~80 LOC neu)

- `postTakeoff(db, ev)` — neuer Discord-Poster für Takeoff-Events. Triggered vom MQTT-`takeoff`-Channel den der Recorder schon empfängt.
- `postPirep` erweitert um Divert-Detection: bei `payload.divert === true` zeigt das Embed `🔀 DIVERT filed` (orange) mit klarer Vorher→Nachher-Route (`EDDF → ~~MDPC~~ ➜ MDST`)
- `mqttSubscriber` neuer `onTakeoff`-Hook
- `index.ts` wired `void postTakeoff(db, row)` an den Subscriber
- Neue Setting `enable_takeoffs` (Default: true) — gleicher Pattern wie `enable_touchdowns`/`enable_pireps`

### Webapp (Admin)

- Settings → Discord-Webhook: zusätzlicher Toggle „Takeoffs posten" zwischen Webhook-URL und „Touchdowns posten"
- PIREPs-Label ergänzt: „PIREPs posten (inkl. Divert-Embed bei Umleitungen)"

### Was VA-Owner macht (einmalig)

1. Browser: https://live.kant.ovh/admin/ → Settings → Discord-Webhook
2. **Webhook-URL** einfügen (aus dem Discord-Server, falls noch nicht da)
3. **Toggles** prüfen (Takeoffs/Touchdowns/PIREPs alle ✓ ist Default)
4. **Test**-Button → grünes „✓ Webhook OK"
5. **Speichern**

Fertig. **Kein Pilot muss irgendwas machen.** Beim nächsten Flug postet der Recorder Takeoff + Touchdown + PIREP (+ ggf. Divert) automatisch.

### Sicherheits-Properties

- URL liegt **nur** in SQLite-DB des Recorders (`/var/lib/aeroacars-recorder/`)
- URL geht **nie** an einen Pilot-Client
- Pilot kann URL nicht sehen, ändern, oder missbrauchen
- Rotation: VA-Owner ändert die URL in 30 Sek im Webapp-Admin, fertig

---

## [v0.7.13] — 2026-05-12

🧹 **Codebase-Audit + Security-Cleanup — kein hardcoded Token mehr, ~700 LOC tot raus.**

### Hintergrund

Komplettes QS-Audit über beide Codebases (Pilot-Client + aeroacars-live + Cross-Cutting-Security). 3 parallele Auditor-Agenten haben **18 Punkte** identifiziert. Dieses Release pickt alle **Pilot-Client-relevanten** Punkte raus (= SSH/VPS-Changes wurden bewusst ausgeklammert für einen späteren Release).

### Critical-Fix

**A1 — Discord-Webhook-Token nicht mehr hardcoded** (Audit C1)
- `discord.rs:27` hatte den GSG-Discord-Webhook-Token im Klartext drin. Repo ist public auf github.com/MANFahrer-GF/AeroACARS → das Token war effektiv öffentlich.
- v0.7.13 liest die Webhook-URL jetzt aus 3 Quellen (mit Priorität): Env `AEROACARS_DISCORD_WEBHOOK` > `<app_data_dir>/discord-webhook.txt` (chmod 0600) > None.
- Settings → Discord-Integration: neues Feld "Webhook-URL" wo Pilot/VA-Owner die URL einfügt.
- Default = leer = keine Posts. Pilot muss aktiv konfigurieren.
- **Wichtig für VA-Owner:** alten Webhook in Discord **rotieren**, neuen erstellen, URL an Piloten verteilen (Pinned-Post im Discord oder PM).

### Cleanup (~700 LOC tot raus)

| # | Was | Stelle |
|---|---|---|
| B7 | Discord `EventContext.airline_icao` + `fuel_used_kg` Felder + 4 Setter | `discord.rs:52` + `lib.rs` × 4 |
| B6 | `current_premium_status` + `pirep_queue::count` Cargo-Warnings | `lib.rs:7230, 12241` |
| B5 | `fcu_debounce()` + 8 Fenix-FCU-State-Felder (Plan verworfen) | `lib.rs:2198, 16331` |
| B4 | 6 orphan Tauri-Commands ohne Frontend-Caller | `landing_get`, `get_minimize_to_tray`, `get_simbrief_settings`, `ofp_callsign_warning_dismiss`, `xplane_uninstall_plugin`, `detect_running_sim` |
| B3 | 4 verwaiste React-Components | `Dashboard.tsx`, `FlightInfoPanel.tsx`, `MassPanel.tsx`, `PhaseTimeline.tsx` |
| B2 | Discord-Rich-Presence-Block (~170 LOC dead seit v0.4.0) | `discord.rs:485-659` + `discord-rich-presence` Cargo-Dep |
| C1 | `secrets::migrate_from_keyring()` + `keyring` Cargo-Dep (v0.5.15-Migration, 30+ Releases her) | `crates/secrets/src/lib.rs:183` |
| C2 | 4 tote i18n-Keys + ganzer `dashboard:`-Locale-Block (DE/EN/IT) | `tabs.dashboard`, `landing.peak_vs`, `landing.plan_tow`, `landing.plan_ldw` |
| C3 | Workspace-Dep `schemars = "0.8"` ohne Code-Pfad | `Cargo.toml` |
| C4 | Stale "Wiring kommt in v0.4.5" + "Patch in v0.7.7" Comments | diverse |

### Security-Härtung

| # | Was |
|---|---|
| B1 | Tauri-Updater-Private-Key aus `client/aeroacars-updater.key` nach `~/.aeroacars-keys/` verschoben (war in `.gitignore`, aber 1× `git add -f` = catastrophic). GitHub-Actions-Secrets nicht betroffen. |
| C5 | Tauri-Webview-CSP von `null` auf strict gesetzt (Audit M-Sec-7). `default-src 'self'` + explizit erlaubte `connect-src` (phpVMS via `https:`, MQTT-WS `wss://live.kant.ovh`, SimBrief). XSS-Defense-in-Depth. |

### Doku + Audit-Spuren

| # | Was |
|---|---|
| D1 | `cargo audit` lokal ausgeführt → 4 Vulnerabilities in `rustls-webpki@0.102.8` via `rumqttc 0.24 → rustls 0.22` → in Audit-Report dokumentiert, Dep-Tree-Update für späteren Release |
| D2 | MEMORY.md korrigiert: Secrets-Storage ist **file-based** (`<app_data_dir>/secrets.json`, chmod 0600), NICHT OS-Keyring (Doku war 30 Releases falsch) |
| D3 | 8 stale Specs (alle als "Approved" / "CODE-READY" markiert) nach `docs/spec/historical/` archiviert. Nur `requirements.md` bleibt aktiv |
| D4 | Audit-Reports (`pilot-client-audit.md`, `aeroacars-live-audit.md`, `security-audit.md`, `MASTER-AUDIT-REPORT.md`) im Repo committet |

### Was NICHT in v0.7.13 ist (bewusst, für separates Release)

- **C2** (`NOPASSWD:ALL` auf VPS) → braucht SSH-Patch + Test der Admin-Endpoints
- **H1+H2** Rate-Limit auf `/api/login` + Re-Auth auf Admin-Endpoints → Recorder-Code-Change
- **H3** `@fastify/static` Major-Bump auf 9.x → Recorder-Test
- **H5** `bcrypt → bcryptjs` → Recorder-Migration
- **Caddy Security-Headers** (CSP, HSTS-explicit, X-Frame-Options)

→ Diese 5 Punkte landen im nächsten VPS-Side-Release wenn du SSH-Window für Tests freigibst.

### Cargo-Audit-Finding (Pending)

4 Vulns in `rustls-webpki@0.102.8` (transitive via `rumqttc`). Solution = `rustls-webpki >=0.103.12` aber das verlangt `rumqttc`-Upgrade auf eine Version die `rustls 0.23+` nutzt → API-Breaking-Check needed. Notiert in `docs/audit/MASTER-AUDIT-REPORT.md` Q1. Geplant für v0.7.14.

---

## [v0.7.12] — 2026-05-12

🐛 **Bid-Card: Pax + Cargo erscheinen wieder — auch ohne phpVMS-Bid-Pointer.**

### Hintergrund

Direkt nach v0.7.11-Release gemeldet: bei CFG2228 (Condor-Bid) war zwischen
der Aircraft-Zeile und dem SimBrief-Plan-Block ein leerer Bereich — die
Pax/Cargo-Chips fehlten komplett. Ursache: v0.7.10 Pre-Flight-SimBrief-direct
holt OFP direkt von simbrief.com ohne die phpVMS-Bid-Pointer-Subfleet-Fares
zu fuellen. Wenn die Bid noch keinen OFP via phpVMS gebunden hat (oder
phpVMS bei diesem Subfleet keine fare-entries hat), waren `paxCount` und
`cargoKg` 0 → Chips ausgeblendet.

### Fix

- `api-client/lib.rs` SimBrief-XML-Parser: extrahiert jetzt `<weights><pax_count>`
  + `<weights><pax_count_actual>` (Fallback) + `<weights><cargo>` (kg).
- `SimBriefOfp` struct: neue Felder `pax_count: i32` + `cargo_kg: f32`.
- `BidSimBriefPreview` (Tauri-Response): gleichen Felder ergaenzt.
- `bid_simbrief_preview` Command: populiert die Werte aus dem geparsten OFP.
- `BidsList.tsx` BidDetails: `paxCount`/`cargoKg` bevorzugen jetzt
  Preview-Werte (> 0) vor den Bid-Subfleet-Fares. Fallback bleibt damit
  identisch zum v0.3.0-Verhalten fuer Pilots ohne SimBrief-Settings.

---

## [v0.7.11] — 2026-05-12

🎯 **Eine Sinkrate, eine Wahrheit — Schluss mit dem Werte-Dschungel.**

### Was

Realer Pilot-Fall: DAL804 zeigte in phpVMS/AeroSore `-407 fpm` aber in der AeroACARS-UI `-364 fpm`. Beide Werte wurden vom Pilot-Client erzeugt — der eine aus dem MSFS-SimVar `PLANE TOUCHDOWN NORMAL VELOCITY` (latched), der andere aus dem 50-Hz-Buffer-Edge (interpoliert). Pilot war verständlicherweise irritiert ("der Pilot hat mich schon für doof erklärt").

v0.7.11 macht den 50-Hz-Buffer-Edge-Wert (`vs_at_edge_fpm`) zur **einzigen kanonischen Score-Basis** und räumt die UI auf, sodass nirgendwo mehr verwirrende Parallel-Werte stehen.

### Backend (Rust)

- `lib.rs` Buffer-Dump-Hook: wenn die v2-Touchdown-Forensik-Kaskade (`vs_at_impact → smoothed_500ms → smoothed_1000ms → pre_flare_peak`) den primären VS-Wert REJECTet, wird jetzt **`vs_at_edge_fpm`** (50-Hz on-ground-Edge-Interpolation) statt MSFS-SimVar verwendet. SimVar-latched fällt damit raus aus dem Score, dem MQTT-Payload und dem phpVMS-PIREP.
- `finalize_landing_rate(stats, vs_fpm, confidence, source)` Atomic-Write-Helper sorgt dafür dass `landing_rate_fpm`, `landing_rate_confidence` und `landing_rate_source` IMMER gemeinsam gesetzt werden.

### Pilot-Client (Frontend)

- `LandingPanel.tsx` Touchdown-Card aufgeräumt: alle smoothed-VS-Varianten (250/500/1000/1500 ms), `vs_at_edge_fpm`, `landing_peak_vs_fpm`, `peak_g_post_500ms/1000ms` aus der Touchdown-Card entfernt. Pilot sieht hier nur noch EINE Sinkrate (= Score-Basis), Touchdown-G, Peak-G, Pitch/Bank/Speed/Sideslip, Bounces, Heading. Die smoothed-Werte leben weiterhin in der **Sinkrate-Forensik-Sektion** (v0.7.8) — dort gehören sie hin.

### VPS-Webapp

- "Algorithmen-Forensik"-Sub-Section in der Diagnostik-Card entfernt. Die zeigte VS-Schätzer-Vergleiche (Lua-30 / Time-Tier-MSFS / SimVar-Final-VS) — gehört in interne Backend-Logs, nicht in die Pilot-UI. Der 50-Hz-TouchdownWindow-Card unten zeigt weiterhin alle relevanten Forensik-Werte für VA-Owner.

### Vorher/Nachher

- v0.7.10: DAL804 → phpVMS `-407 fpm` (SimVar) / AeroACARS-UI `-364 fpm` (50-Hz-Edge) → Pilot verwirrt
- v0.7.11: DAL804 → phpVMS `-364 fpm` / AeroACARS-UI `-364 fpm` → konsistent

---

## [v0.7.10] — 2026-05-11

✨ **Pre-Flight-SimBrief-direct: frische OFP-Werte schon in der Bid-Liste — vor dem IFR-Start.**

### Was

Bisher konnte der Pilot SimBrief-OFP-Daten erst nach dem IFR-Start via "Aktualisieren" laden — vorher zeigte die Bid-Liste nur die phpVMS-Pointer-Werte (oft veraltet). Pilot-Wunsch: *"warum kann ich die Daten nicht schon vor dem IFR-Start bekommen?"*

v0.7.10 holt SimBrief-Daten direkt von simbrief.com (via `bid_simbrief_preview` Tauri-Command), sobald die Bid-Liste geladen wird.

### Backend (Rust)

- Neuer `#[tauri::command] bid_simbrief_preview(bid_id: i64)` — holt die SimBrief-OFP für eine Bid direkt von simbrief.com, ohne `phase_locked`/`active_flight`-Gate
- Reuse von `try_simbrief_direct_with_match` Logik (gleicher Code-Path wie der Refresh-Button)

### Frontend

- `BidsList.tsx` `fetchPreviewsForBids()` ruft beim Bid-List-Load alle Previews parallel ab
- Grünes "✓ Frische SimBrief-Werte geladen"-Banner bei Erfolg
- Neuer Notice-Tone `"ok"` (zusätzlich zu `info/warn/err`) — grünes CSS

---

## [v0.7.9] — 2026-05-11

🔄 **SimBrief-OFP-Refresh: Callsign-Match auf SOFT-Warning umgestellt — DEP/ARR sind der echte Anker.**

### Was

Realer Pilot-Fall: phpVMS-Bid `EWL9725` (Eurowings Europe Operator-Code EWL), SimBrief-OFP-Callsign `EWG4PY` (Eurowings ICAO + Personal-Callsign). DEP/ARR identisch (LOWG → EDDL), aber das v0.7.7-Match-Verify rejectet den OFP als "Mismatch" → der Refresh-Button bekam keine neuen OFP-Daten, der Pilot sah nichts und war frustriert.

v0.7.9 macht den Callsign-Check zu einem SOFT-Warning statt Hard-Block:

- **DEP+ARR matchen** → OFP wird **geladen** (war vorher Hard-Block bei Callsign-Diff)
- **Callsign weicht ab** → OFP wird trotzdem geladen, dazu gelber Warning-Notice mit konkreten Werten ("OFP-Callsign ist EWG4PY, dein aktiver Flug nutzt EWL9725 / 9725 — DEP/ARR stimmen, OFP wurde geladen")
- **DEP oder ARR weichen ab** → Hard-Block bleibt (= klar ein anderer Flug)

### Backend (Rust)

- Neuer `DirectOutcome::MatchWithCallsignWarning { ofp, simbrief_callsign }` Variante zwischen `Match` und `Mismatch`
- `try_simbrief_direct_with_match` 2-stufige Logik: erst DEP/ARR (Hard), dann Callsign (Soft)
- Neuer `AppState::ofp_callsign_warning: Mutex<Option<OfpCallsignWarning>>` — gespiegelt nach Frontend via:
  - `#[tauri::command] ofp_callsign_warning_get()` — Frontend liest nach Refresh
  - `#[tauri::command] ofp_callsign_warning_dismiss()` — X-Button löscht
- Clean-Match löscht das Warning automatisch (sonst hängt es nach neuem Refresh)

### Frontend

- `BidsList.tsx` `refreshAll()` pollt nach `flight_refresh_simbrief` das Warning und rendert gelben Notice
- Neuer i18n-Key `flight.ofp_callsign_warning` in DE/EN/IT mit Placeholders `{{sb_callsign}}` + `{{active_callsigns}}`

### Was bleibt unverändert

- v0.7.7-Pointer-Pfad-Fallback (bei `bid_not_found`)
- DEP+ARR-Hard-Match (nur Callsign wurde von Hard auf Soft umgestellt)
- SimBrief-Settings-Konfiguration (Settings → SimBrief Integration)

---

## [v0.7.8] — 2026-05-11

🎯 **Sinkrate-Forensik im Landung-Tab — Pilot versteht warum seine Landerate so ist wie sie ist.**

### Was

Reine UI-Erweiterung, keine neue Datenerhebung. Adressiert wiederkehrende Pilot-Beschwerden vom Typ *"Volanta zeigt mir 232 fpm aber AeroACARS scort 357 — wer hat Recht?"* — beide Werte sind physikalisch korrekt, sie messen unterschiedliche Sachen. Die neue Sektion erklaert das transparent.

Spec: `docs/spec/v0.7.8-landing-rate-explainability.md` v1.8 APPROVED (8 QS-Iterationen).

### Was sich aendert (im Landung-Tab)

Neue Sektion **🎯 Sinkrate-Forensik** zwischen Approach-Stability und Flare-Quality, mit 6 Bloecken:

1. **Aufklaerungs-Block** (cyan-Akzent): "Welche Sinkrate ist die richtige?" — erklaert dass Volanta/Cockpit-VSI Mittel ueber 0.5-1.5 s zeigen, AeroACARS aber den Cascade-Wert direkt am Aufsetz-Moment scort (FAR 25.473 Engineering-Standard)
2. **Tool-Mittel-Tiles** (4 Tiles): 1.5 s / 1.0 s / 0.5 s / 0.25 s aus `vs_smoothed_*_fpm` — was dein Cockpit/Volanta typischerweise anzeigt
3. **Bucket-Aufschluesselung**: disjoint-Bucket-Differenz aus den 4 kumulativen Mittelwerten — zeigt wie sich die Sinkrate in jeder Phase entwickelt hat. Bei monotonem Anstieg ueber Betrag (|Delta| > 20 fpm in allen 3 Inter-Bucket-Schritten): Auto-Trend-Note "Flare nicht gehalten / durchgesackt"
4. **Score-Basis-Tile** (gross + prominent): `landing_peak_vs_fpm ?? landing_rate_fpm` mit Tone-Farbe nach `T_VS_*`-Bands (200/400/600/1000 fpm) + `landing_source` als Quellen-Pill
5. **Coaching-Tipp** (ein Satz nach Prioritaet): flare_lost / hard_g / no_flare / late_drop / clean
6. **Mehr Details** (collapsible, default zu): Position-Trace letzte 3 s aus `touchdown_profile` (NICHT `approach_samples` — das hat nur vs_fpm/bank_deg) + Aufprall-Belastung (Peak-G post-TD 500ms/1s)

### Backwards-Compat

- Sektion rendert wenn `hasForensics(record) === true` — mindestens eines von `forensic_sample_count`, `vs_smoothed_*_fpm`, `vs_at_edge_fpm` gesetzt
- Eintraege ohne 50-Hz-Forensik-Daten zeigen einen kompakten Legacy-Hinweis ("Fuer diesen aelteren Flug wurden die Forensik-Daten noch nicht gespeichert")
- Tiles mit `null`-Wert zeigen `—` (Em-Dash), Grid bleibt 2x2 stabil
- Score-Basis-Source-Pill nur wenn `landing_source != null && !== ""` (pre-v0.7.1: kein Pill, kein Error)

### Design-Konsistenz mit AeroACARS-Look (§4.5)

Spec verbietet "Stein im AeroACARS-Design". Implementiert mit:
- `landing-section` / `landing-stability` / `landing-stability__row` Pattern (App.css:4841, 5172, 5180)
- Lokale Sub-Komponenten (`SmoothedVsTile`, `ScoreBasisTile`, `VsBucketBreakdown`, `PositionTrace`, `ImpactTiles`) im selben File — kein Import nicht-existenter UI-Bibliothek
- CSS-Variablen + Tone-Farb-Set, keine harten Borders, keine Box-Shadows, nur Volanta als externer Tool-Anker (kein DLHv/SmartCARS)

### Out-of-Scope

- Score-Engine bleibt unveraendert (Cascade-Chain `landing_peak_vs_fpm ?? landing_rate_fpm` in `LandingPanel.tsx:257, 1116` nicht angefasst)
- Keine Backend-Aenderungen, keine VPS-Web-Aenderungen (dort schon in v0.7.7-Pipeline umgesetzt)
- Sub-Score-Tabelle bleibt in `SubScoreGrid` weiter oben (kein Doppeln)

### Tests

- **31 Vitest-Tests** in `SinkrateForensik.test.tsx` (neu) — Bucket-Math, Trend-Detection, Coaching-Selector, Score-Basis-Cascade, Tone-Bands, Trace-Filter, GSG-218-End-to-End
- Neue Test-Infrastruktur: `vitest`, `@testing-library/react`, `jsdom` (siehe `vitest.config.ts` + `src/test/setup.ts`)
- `npm test` gruen, `npm run build` gruen
- TypeScript exclude fuer Test-Files (App-Build umgeht Tests)

### Files

- `client/src/components/SinkrateForensik.tsx` (neu, ~370 Zeilen mit allen Sub-Komponenten)
- `client/src/components/SinkrateForensik.test.tsx` (neu, 31 Tests)
- `client/src/components/LandingPanel.tsx` (Import + Render-Stelle)
- `client/src/locales/{de,en,it}/common.json` (je 29 neue Keys unter `landing.sinkrate_forensik.*`)
- `client/src/App.css` (~320 Zeilen neue CSS-Klassen am Ende)
- `client/vitest.config.ts` + `client/src/test/setup.ts` (Test-Setup)
- `client/package.json` + `client/package-lock.json` (devDependencies)
- `client/tsconfig.json` (exclude Test-Dateien aus App-Build)

---

## [v0.7.7] — 2026-05-11

🛫 **OFP-Refresh waehrend Boarding endlich nutzbar — SimBrief-direct macht den Bid-Pointer-Pfad obsolet. Real-Pilot-Frust beseitigt, Pilot-Callsign-Cases unterstuetzt.**

### Was

Real-Pilot-Frust nach v0.7.5/v0.7.6: Pilot regeneriert OFP auf simbrief.com, klickt "Aktualisieren" — und die Plan-Werte blieben alt. Wurzel-Analyse zeigte zwei verschachtelte Probleme:

1. **W1 Discoverability** — Der prominente "Aktualisieren"-Button im Bid-Tab rief gar nicht den OFP-Refresh fuer den aktiven Flug auf. Pilot musste den versteckten Cockpit-Refresh-Button finden.
2. **W5 Bid weg nach Prefile** — phpVMS-7 entfernt den Bid sofort wenn AeroACARS prefiled. Damit ist der gesamte phpVMS-Pointer-Pfad fuer OFP-Refresh **tot** sobald der Pilot in Boarding ist. Cockpit-Button auch.

v0.7.7 loest beide gemeinsam — UX-Schicht + echter Daten-Pfad.

Spec: `docs/spec/ofp-refresh-during-boarding.md` v1.4 + `docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md` v1.5.

### Schicht 1 — UX-Discoverability

- **Bid-Tab-Refresh ruft jetzt auch `flight_refresh_simbrief`** — der prominente Button macht endlich was der Pilot erwartet
- **Phase-Gate** `Preflight | Boarding | Pushback | TaxiOut` (inkl. Pushback — Plan-Werte sind dort noch nutzbar)
- **`flight_id` persistiert** vor `prefile_pirep` aus dem Bid — sonst nach Prefile fuer immer weg (W5-Foundation)
- **`simbrief_ofp_id` + `_generated_at`** in FlightStats fuer "OFP unveraendert"-Erkennung
- **Notice-Infrastruktur** mit Auto-Clear + UI-Refresh-Trigger
- Pilot-Client-Banner ist seit v0.7.1 schon master-score-derived — keine Aenderung noetig

### Schicht 2 — Daten-Pfad SimBrief-direct

- **`fetch_simbrief_direct()`** via `xml.fetcher.php?userid=X` oder `?username=X` — bypasst den phpVMS-Bid-Pointer komplett. Funktioniert auch wenn der Bid weg ist (W5-Loesung)
- **Settings-Section "SimBrief Integration"** — eigene Tab-Sektion mit zwei Feldern (Username + User-ID), `Verbindung pruefen`-Button mit OFP-Vorschau bei Erfolg
- **localStorage-Sync beim Login-Mount** — Settings sind nach App-Restart sofort verfuegbar, kein Pilot-Doppelklick noetig
- **Robust-Error-Detection** — HTTP 400 (Navigraph-Doku) UND `<fetch><status>Error</status>` (Live-Probe) werden beide als `UserNotFound` gemapped
- **`SimBriefDirectError`-Enum** (UserNotFound / Unavailable / Network / ParseFailed) mit spezifischen i18n-Notice-Texten

### Match-Verifikation (Pilot-Callsign-Cases)

Real-Beispiel: Pilot fliegt Bid `CFG1504` aber mit persoenlichem Callsign `4TK`. SimBrief-OFP traegt Callsign `CFG4TK`. Match-Logik akzeptiert eine **Kandidaten-Liste** valider Formen:

1. `airline_icao + flight_number` (`CFG1504`)
2. `flight_number` allein (`1504`)
3. `Bid.flight.callsign` direkt (wenn phpVMS-VA das fuellt)
4. `airline_icao + Profile.callsign` (`CFG4TK`)
5. `Profile.callsign` allein (`4TK`)

dpt/arr bleiben Pflicht-Anker. Kein blinder Suffix-Match (= `DLH1100` vs `100` MISMATCH, v1.1-Regression-Guard).

### Mismatch-Handling

Wenn der Pilot zwischen Bid-Start und Refresh einen OFP fuer einen **anderen Flug** generiert hat: **HARD-Block** mit reicher Notice. Pilot sieht:

> ⚠ Dein letzter SimBrief-OFP gehoert zu Flug **CFG2000** (EDDF → GCTS). Erwartet waere **CFG1504 / 1504 / CFG4TK / 4TK** (EDDF → GCTS). Bitte auf simbrief.com einen OFP fuer den aktiven Flug generieren.

Alle 3 Refresh-Buttons (Bid-Tab + Cockpit-Tab + Loadsheet-Inline) zeigen identische, lokalisierte Notice via shared `formatRefreshError`-Helper. Cockpit-Context zeigt zusaetzlich `phase_locked` + `no_simbrief_link` als lesbare Hinweise — keine `[object Object]`-Falle mehr.

### Audit-Trail

`flight_refresh_simbrief` loggt jetzt im Activity-Log:
- `OFP refreshed` (alte ID → neue ID, neu)
- `OFP unchanged` (gleiche ID, nichts ueberschrieben)

Sichtbar im Pilot-Activity-Log + im JSONL-Flugprotokoll fuer Re-Analyse.

### Composite-Failure-Priorisierung

Wenn beide Pfade scheitern (SimBrief offline UND Bid weg), priorisiert die Notice den **Direct-Fehler** — Pilot weiss damit dass das Problem bei SimBrief-Konfiguration sitzt, nicht beim Bid. Falsche Diagnose ("Bid weg" als Sekundaer-Symptom) wird vermieden.

### Backward-Compat

- Pilot ohne SimBrief-Username/User-ID in Settings: faellt auf v0.7.6-Verhalten zurueck (Pointer-Pfad mit `bid_not_found`-Hinweis, jetzt mit Pointer auf Settings)
- Alte v0.7.6-PIREPs werden nicht beruehrt
- Alte landing_history.json + PersistedFlight-Snapshots bleiben lesbar (alle neuen Felder `Option<T>` + `#[serde(default)]`)

### Tests

- **179/179 gruen** (vorher 169)
- 99 lib unit (+27: 8 Persistenz/Phase-Gate + 19 Match-Verifikation inkl. CFG4TK + Audit-Log)
- 11 api-client (+2 SimBrief request_id-Parser)
- 13 phase_fsm_replay (v0.7.5)
- 8 touchdown_v2_replay
- 30 sim_core unit + 9 sim_core lib
- 8 goldenset (landing-scoring)

### Update-Empfehlung

Auto-Updater zieht v0.7.7 automatisch fuer alle bestehenden v0.7.6-Pilot-Clients. Pilot kann nach Update einmalig SimBrief-Username/User-ID in Settings eintragen (oder ohne SimBrief-direct weiter Pointer-Pfad nutzen). Ohne Settings-Eintrag funktioniert AeroACARS exakt wie v0.7.6.

---

## [v0.7.6] — 2026-05-11

🧮 **Landing Payload & UI Consistency — Score, Payload, Forensik und Web-Anzeige zeigen jetzt dieselbe Wahrheit. Drei reale Datenkonsistenz-Bugs beseitigt, durch zwei v0.7.5-Pilot-Logs belegt.**

### Was

QS-Sichtung von zwei realen v0.7.5-Pilot-Logs (SAS9987 EDDH→ENSB, GSG303 2OR3→OR66) hat drei Datenkonsistenz-Bugs aufgedeckt, bei denen Score, PIREP-Payload und Web-Dashboard sich gegenseitig widersprachen. Score-Algorithmus selbst war NICHT kaputt — alle Bugs sassen im Payload-Contract, im Web-Frontend, oder in fehlender Sicherheitsnetz-Logik. v0.7.6 ist daher ein reiner Konsistenz-Schnitt.

Spec: `docs/spec/v0.7.6-landing-payload-consistency.md` (v1.2 final).

### P1-1 — Fuel-Contract sauberziehen

PIREP-Payload bekommt das neue Feld `actual_trip_burn_kg` = `takeoff_fuel_kg − landing_fuel_kg`. Single Source of Truth fuer OFP-Vergleich zwischen Pilot-Client, Web-Dashboard, Discord-Embed und phpVMS-Modul.

`fuel_used_kg` bleibt im Payload, ist aber **explizit als raw/sim-cumulative** markiert — bei MSFS oft Cumulative-Counter seit Sim-Start. Real-Beleg SAS9987: 19984 kg gemeldet bei tatsaechlich 8762 kg Trip-Burn → +117% Phantom-Abweichung im Web-Dashboard.

**Web-Dashboard Fallback-Kette** fuer Backward-Compat:

```
actualBurn = actual_trip_burn_kg                    (v0.7.6+ PIREPs)
          ?? takeoff_fuel_kg - landing_fuel_kg       (v0.7.5 Backward-Compat)
          ?? null                                     (Fuel-Zeile ausgeblendet)
```

`pl.fuel_used_kg` darf fuer den OFP-Vergleich **niemals direkt** genutzt werden.

Auch im Recorder (`recorder/src/db.ts`) wurde der SessionStats-Fuel-Compute auf die Trip-Burn-Reihenfolge umgestellt: Position-Stream-Delta hat Vorrang vor Raw-PIREP-`fuel_used_kg`.

### P1-2 — Bounce-Quelle synchronisieren

Bei SAS9987 zeigte v0.7.5 gleichzeitig `landing_analysis.bounce_count = 1` (max AGL 13.6 ft Wiederabheben) UND `payload.bounce_count = 0` UND Sub-Score `bounces = 100 (clean)`. Drei Quellen, zwei Wahrheiten.

Fix in der **Forensik-Schicht** (touchdown_v2 — nicht landing-scoring, weil nur die Forensik AGL-Verlauf kennt):

```rust
pub const BOUNCE_FORENSIC_MIN_AGL_FT: f32 = 5.0;   // sichtbar im Replay
pub const BOUNCE_SCORED_MIN_AGL_FT:   f32 = 15.0;  // bestraft im Sub-Score
```

`landing_analysis` emittiert jetzt **drei** Counts:
- `forensic_bounce_count` (≥ 5 ft) — kleine Hopser im Replay sichtbar
- `scored_bounce_count` (≥ 15 ft) — was wirklich im Sub-Score zaehlt
- `bounce_count` = `forensic_bounce_count` (Backward-Compat fuer alte Reader)

Die Override-Logik nach dem 50-Hz-Sampler-Dump schreibt jetzt `scored_bounce_count` zurueck in `s.bounce_count`, sodass alle 5 Score-Pfade konsistent sind. Zentraler `scored_bounce_count_for_score(stats)`-Helper macht die Semantik im Code explizit.

SAS9987-Klasse (13.6 ft) → `forensic_bounce_count: 1, scored_bounce_count: 0`, Sub-Score bleibt 100 (clean) — alle drei Quellen erzaehlen jetzt die gleiche Geschichte.

### P1-3 — Runway-Geometry-Trust-Check

GSG303 v0.7.5: `arr_airport=OR66` aber `runway_match_icao=K5S9` (3.5 km Centerline-Offset, Float-Distance −613 m). Score behandelte das trotzdem als "TD Zone 1, excellent stop".

Neue **pure-function** `runway_geometry_trust_check()` mit 4 Reasons:

```rust
pub const RUNWAY_TRUST_MAX_CENTERLINE_OFFSET_M: f32 = 200.0;
pub const RUNWAY_TRUST_MIN_FLOAT_DISTANCE_M:    f32 = -100.0;

// Returns (trusted, reason):
//   "no_runway_match"             — None matched_icao → silent in UI
//   "icao_mismatch"               — Match != arr/divert → Alarm-Pill
//   "centerline_offset_too_large" — > 200 m → Alarm-Pill
//   "negative_float_distance"     — < -100 m → Alarm-Pill
```

ICAO-Vergleich ist `eq_ignore_ascii_case` — robust gegen Mixed-Case aus externen Quellen.

PIREP-Payload + TouchdownPayload + LandingRecord (storage) bekommen `runway_geometry_trusted` + `runway_geometry_reason`-Felder. Bei `trusted=false` wird `landing_touchdown_zone` auf `None` gesetzt. `landing_float_distance_m` bleibt als Raw-Wert (Diagnostik).

**Web-Dashboard + Pilot-Client** blenden bei untrusted geometry komplett aus:
- Touchdown-Zone, Float-Distance, Centerline-Offset, Past-Threshold
- Runway-ID + Runway-Length (waeren bei GSG303 sonst "K5S9/16 (asphalt) · 1152 m" = irrefuehrend)
- RunwayDiagram

Sichtbar bleibt nur ein lokalisierter Hint-Pill mit dem Reason. **Rollout-Sub-Score bleibt valide** (kommt aus GPS-Track, nicht aus Runway-DB).

`no_runway_match` (Privatplaetze ohne DB-Eintrag) zeigt **kein** Alarm-Pill — silent Suppression der Geometry-Tiles.

### P2 — Render-Artefakte + Legacy-Felder

- React `&&`-mit-Zahl-Bug in `PirepFeed.tsx` gefixt (`{count && ...}` rendert "0" wenn count exakt 0). `(count ?? 0) > 0` Pattern statt truthy-Check. Verhindert die `00`/`0`-Artefakte aus dem v0.7.5-Screenshot.
- `Stat label="Fuel"` in PilotHistory: `!= null` statt truthy → 0 kg (Glider-Sessions) wird jetzt korrekt angezeigt.
- Monitor PirepFeed bekommt gleiche Fuel-Fallback-Kette wie Webapp.
- `fuel_efficiency_pct` (alter, abweichender Berechnungs-Wert) ist `@deprecated since v0.7.6` markiert — Web rendert nicht mehr, Feld bleibt im Payload fuer externe Discord-Embeds / Custom-Dashboards. Single Source of Truth: `sub_scores[fuel].value`.

### Backward-Compat

- **Score-Algorithmus** unveraendert. Bei Re-Anzeige der zwei Real-Logs:
  - SAS9987: Score bleibt **67**, OFP-Treue bleibt **95**
  - GSG303: Score bleibt **49**, Fuel-Skip bleibt
- Alte v0.7.5-PIREPs ohne `actual_trip_burn_kg` / `runway_geometry_trusted` werden via Fallback-Ketten korrekt angezeigt.
- Alte landing_history.json-Eintraege im Pilot-Client bleiben deserialisierbar (alle neuen Felder `Option<...>` mit `serde(default)`).
- Banner-Anzeige im Pilot-Client (Headline) war seit v0.7.1 schon master-score-derived → kein Change.

### Tests

- **131/131 gruen** (vorher 116)
- 72 lib unit (+15 neue v0.7.6-Tests: 11 runway-trust + 3 bounce-threshold + 1 case-insensitive)
- 30 sim_core unit
- 8 goldenset (landing-scoring)
- 13 phase_fsm_replay (v0.7.5)
- 8 touchdown_v2_replay

### Update-Empfehlung

Auto-Updater zieht v0.7.6 automatisch fuer alle bestehenden v0.7.5-Pilot-Clients. Web-Dashboard auf live.kant.ovh nutzt ab v0.7.6 die neuen Felder bevorzugt, mit Fallback fuer alle vorhandenen v0.7.5-PIREPs in der DB.

---

## [v0.7.5] — 2026-05-10

🛡️ **Phase-Safety Hotfix — zwei reale State-Machine-Bugs beseitigt, durch echte VPS-Pilot-Daten belegt + replay-getestet.**

### Was

VPS-Datenanalyse von 29 realen JSONL-Pilot-Logs hat zwei eigenstaendige Phase-FSM-Bugs aufgedeckt, die in Spec v1.0 als "theoretisch moeglich" markiert waren — aber in Real-Logs **belegt** sind:

1. **URO913** — Universal Arrived-Fallback feuerte waehrend des Rolls (engines=0 + groundspeed > 1) und schaltete den Flieger faelschlich auf Arrived, obwohl der Pilot noch nicht stand.
2. **PTO105** — `holding_pending_since` leakte phasenuebergreifend, sodass eine `Approach → Final → Approach`-Sequenz innerhalb von 5.2 s als "Holding" missdetektiert wurde, statt der spec-gemaessen 90 s Dwell.

### Fix 1 — Arrived-Fallback verlangt echten Stillstand

```rust
// NEU: pub fn arrived_fallback_conditions_basic(...)
on_ground && engines_running == 0 && groundspeed_kt < 1.0
```

Vorher fehlte die `groundspeed_kt < 1.0`-Bedingung — der Fallback feuerte bei `engines=0` selbst wenn der Flieger noch mit 42 kt rollte (URO913 Real-Log: 4 Snapshots mit gs > 1 + engines=0 + on_ground). Mit Fix bleibt der Fallback aus, bis der Flieger wirklich steht.

### Fix 2 — `holding_pending_since` reset bei Phase-Wechsel ≠ Holding

```rust
// NEU: pub fn should_reset_holding_pending(prev, next) -> bool
next != prev && next != FlightPhase::Holding
```

Im Phase-Wechsel-Block wird der Pending-Counter jetzt explizit zurueckgesetzt, wenn die naechste Phase **nicht** Holding ist. Vorher konnte ein leakender Counter dazu fuehren, dass eine kurze `Approach → Final → Approach`-Schwankung (5.2 s in PTO105) die 90 s Dwell-Pruefung umging.

### Tests — 3-Layer Replay-Suite (13 neue Tests)

**`tests/phase_fsm_replay.rs`** — 13/13 gruen:

- **7 Helper-Tests** verifizieren beide Helper-Funktionen direkt (Wahrheitstabelle pro Bedingung).
- **3 Fixture-Replay-Tests** laden anonymisierte Real-Daten und beweisen dass das Bug-Symptom in den Daten steckt + dass die Helper sie jetzt korrekt blockieren.
- **3 PII-Schutz-Tests** verhindern dass anonymisierte Fixtures jemals echte PIREP-IDs / Airlines / Routen / Flugnummern ins Repo holen.

### Anonymisierte Fixtures (PII-frei)

```
client/src-tauri/tests/fixtures/
  phase_arrived_fallback_rolling.jsonl.gz  (TEST001 — URO913-Klasse)
  phase_holding_pending_leak.jsonl.gz      (TEST002 — PTO105-Klasse)
  phase_valid_holding.jsonl.gz             (TEST003 — DLH742 positiv-Beleg)
```

`pirep_id = TEST_FIXTURE`, `airline_icao = TEST`, `flight_number = TEST00X`, `dpt_airport = XXXX`, `arr_airport = YYYY`. Dateinamen tragen bewusst keine Real-Callsigns mehr.

### Spec-Update

`docs/spec/flight-phase-state-machine.md` v1.5:

- §13.8 🔴 BELEGT (URO913)
- §13.9 🔴 BELEGT (PTO105 — neu)
- §15 VPS-Daten-Coverage (29 Logs analysiert)
- §16 Reale Regression-Kandidaten

### Backward-Compat

- **Keine API-Aenderung** — beide Helper sind neu (`pub`) und werden intern aufgerufen.
- **Pilot-Verhalten** wird strikter, aber korrekter:
  - Rollende Flieger mit abgestellten Engines werden nicht mehr stillschweigend auf Arrived gesetzt (URO913-Klasse).
  - Kurze Approach-Schwankungen unter 90 s werden nicht mehr als Holding missdetektiert (PTO105-Klasse).
- Echte Holding-Episoden (>= 90 s Dwell, DLH742-Klasse) bleiben unveraendert erkannt — durch positiv-Beleg-Replay-Test abgesichert.

### Tests gesamt

- **116/116 gruen**
- 57 lib unit
- 30 sim_core unit
- 8 goldenset (landing-scoring)
- 8 touchdown_v2_replay
- **13 phase_fsm_replay (NEU)**

### Update-Empfehlung

Auto-Updater zieht v0.7.5 automatisch fuer alle bestehenden v0.7.4-Pilot-Clients.

---

## [v0.7.4] — 2026-05-10

🧹 **Polish ueber v0.7.3 — Cargo-Aliase praeziser, A359-Edge geloest, Strict-Tests pro Familie.**

### Was

QS-Review nach v0.7.3 hat 1 P1 + 3 P2 + 3 P3 aufgedeckt — alles in v0.7.4 abgearbeitet.

### P1 — `FREIGHTER`-Long-Form fuer alle Cargo-Aliase

v0.7.3 hatte das Long-Form `"X-X FREIGHTER"` nur fuer `B748F` eingebaut. Ein Sim-Addon das `"Boeing 757-200 Freighter"` als Title meldet hatte mit `B752F`-Bid weiter geblockt. v0.7.4 zieht das fuer alle Frachter nach:

```rust
"B74F"  => &["747-400F", "747-400 FREIGHTER", "B74F"],
"B752F" => &["757-200F", "757-200 FREIGHTER", "B752F"],
"B763F" => &["767-300F", "767-300 FREIGHTER", "B763F"],
"B762F" => &["767-200F", "767-200 FREIGHTER", "B762F"],
"A332F" => &["A330-200F", "A330-200 FREIGHTER", "A332F"],
```

### P2 — A359-Alias narrowed

`A359 => &["A350-900", "A350"]` matchte faelschlich auch `A350-1000` weil `"A350-1000".contains("A350")` true ist. Substring-Match ist sensitiv — der `"A350"`-Alias war zu breit und kollidierte mit der A35K-Familie. Fix:

```rust
"A359" => &["A350-900"],  // "A350" entfernt
```

Alle bekannten Sim-Adapter (Asobo, iniBuilds, Aerosoft) liefern den Variant-Suffix immer mit. Der `"A350"`-Alias war redundant + gefaehrlich.

### P2 — Strict-Cargo-Grenze testgesichert

Spec §7.3 sagt explizit "Cargo-Bid + Pax-Sim = strict geblockt" (Pax-Compartment hat keine Cargo-Lasten-Verteilung). v0.7.3 hatte aber nur Mismatch-Tests gegen unverwandte Familien — die Strict-Grenze pro Familie war nicht getestet. v0.7.4 fuegt drei explizite Tests hinzu:

```rust
fn cargo_bid_strict_against_pax_sim() {
    assert!(!aircraft_types_match("B752F", "757-200"));   // BLOCKIERT
    assert!(!aircraft_types_match("B763F", "767-300"));   // BLOCKIERT
    // ... pro Familie
}

fn pax_bid_accepts_cargo_sim_pragmatism() {
    assert!(aircraft_types_match("B752", "757-200F"));    // erlaubt
    // ... umgekehrte Richtung okay (Cargo-Pragmatismus)
}

fn cargo_aliases_match_freighter_long_form() {
    assert!(aircraft_types_match("B752F", "757-200 Freighter"));
    // ... P1-Verifikation
}

fn a359_does_not_match_a350_1000() {
    assert!(!aircraft_types_match("A359", "A350-1000"));  // BLOCKIERT
}
```

### Spec-Pflege (3 P3)

- Spec-Status-Texte alle auf v0.7.4-Pending aktualisiert (52 Aliases, 21 Tests, 17 Familien)
- B748F-Edge-Case-Eintrag uebersetzt vom "wird kommen" auf "ist da seit v0.7.3" + neuer A359-A350-1000-Eintrag
- Code-Tippfehler `"Quatar Cargo"` → `"Qatar Cargo"`

### Tests

- **57/57 lib** (vorher 53 — 4 neue Tests)
- **21/21 aircraft_alias_tests** (vorher 17)
- 30/30 landing-scoring + 8/8 goldenset
- 8/8 touchdown_v2_replay
- **Gesamt 103/103 Tests grün**

### Backward-Compat

Aliases sind additiv (Spec §8). Eine bestehende Pilot-PIREP die unter v0.7.3 ging, geht auch unter v0.7.4. Die Aenderungen unter v0.7.4 sind:
- 5 Cargo-Familien akzeptieren ZUSAETZLICHE Long-Form-Strings (`" FREIGHTER"`)
- A359 akzeptiert KEIN A350-1000 mehr — das war vorher faelschlich akzeptiert. Wer in v0.7.3 mit A359-Bid + A350-1000-Sim flog, wird ab v0.7.4 mit aircraft_mismatch geblockt. Das ist die korrekte Strenge — ein A350-1000 ist kein A350-900 (33 Pax mehr, andere Performance).

---

## [v0.7.3] — 2026-05-10

🛬 **Aircraft-Type-Match: Cargo-Frachter HOHE-Prio Aliase + Spec-Pflege.**

### Was

Proaktive Erweiterung der `aircraft_aliases`-Tabelle um die wahrscheinlichsten Cargo-Frachter, basierend auf Spec §4 Arbeitsliste:

- **`B748F`** — Boeing 747-8 Freighter (Lufthansa Cargo, Atlas Air, Cargolux, Polar Air Cargo)
- **`B74F`** — Boeing 747-400 Freighter (Klassiker)
- **`B752F`** — Boeing 757-200 Freighter (DHL/UPS/FedEx)
- **`B763F`** — Boeing 767-300 Freighter (FedEx-Klassiker)
- **`B762F`** — Boeing 767-200 Freighter
- **`A332F`** — Airbus A330-200 Freighter (Qatar Cargo, Turkish Cargo, Etihad Cargo)

Plus **6 neue Tests** in `aircraft_alias_tests` — pro Familie 1 Match + 1 offensichtlicher Mismatch (laut Spec Leitprinzip).

### Cargo-Pragmatismus (Spec §7.3)

- **Pax-Bid + Cargo-Sim** (z.B. `B752` Bid + `757-200F` Sim): wird akzeptiert via Long-Form-Substring. Begruendung: Cargo-Variante kann problemlos eine Pax-Strecke fliegen.
- **Cargo-Bid + Cargo-Sim** (z.B. `B752F` Bid + `757-200F` Sim): matched ueber den neuen expliziten Alias.
- **Cargo-Bid + Pax-Sim** (z.B. `B763F` Bid + `767-300` Pax-Sim): bleibt strict geblockt (Pax-Compartment hat keine Cargo-Lasten-Verteilung).

### Spec aktualisiert

`docs/spec/aircraft-type-match.md` v1.1 — neues **Leitprinzip** + **3 harte Regeln** (statt strenges Regelwerk):

1. Keine extrem breiten Aliases wie `A3`, `747`, `MD`, `AIRBUS`
2. Jeder neue Alias bekommt mindestens einen Match-Test
3. Jeder neue Alias bekommt mindestens einen offensichtlichen Mismatch-Test

Test-Matrix §5 von "Pflicht" auf "Empfehlung" umgestellt. §4 Arbeitsliste statt Lueckenanalyse.

### Tests

- **53/53 lib** (vorher 46 — 6 neu fuer Cargo + 1 Bug-Fix)
- **17/17 aircraft_alias_tests** (vorher 10)
- 30/30 landing-scoring + 8/8 goldenset
- 8/8 touchdown_v2_replay
- **Gesamt 99/99 Tests grün**

### Nicht in v0.7.3

Verbleibende Arbeitsliste fuer spaeter (proaktiv NICHT noetig — nur bei echtem Pilot-Bug):

- ATR/CRJ/Q400 Familien (Regional)
- MD-80/MD-90/Fokker Familien (selten)
- Sukhoi SU95
- A338F / A33F generisch

---

## [v0.7.2] — 2026-05-10

🔧 **Hotfix: MD-11 / MD-11F Aircraft-Type-Match.**

### Live-Bug

Pilot Sven (German Sky Group) konnte die Martinair-Cargo-Bid **MPH62** (SKBO → TJBQ, MD11/PH-MCU, 78.1 t Cargo) nicht starten. AeroACARS blockierte mit `aircraft_mismatch`:

```
Aircraft mismatch: bid wants MD11 (PH-MCU), sim has MD11F
(title "TFDi Design MD-11F PW4462 (Low Poly Cabin)").
Load the correct aircraft type in the sim or pick a matching bid.
```

### Ursache

Die `aircraft_aliases`-Tabelle (`lib.rs:408-487`) hatte keinen Eintrag fuer die MD-11-Familie — Vergessen seit Initial-Implementation. Boeing 777F hatte einen Alias, MD-11F nicht. Strict-equality `MD11 != MD11F` blockierte den Cargo-Pilot, obwohl Frachter-Variante derselben Familie.

### Fix

```rust
// ---- McDonnell Douglas ----
"MD11"  => &["MD-11", "MD11"],   // matched MD-11 + MD-11F
"MD11F" => &["MD-11F", "MD11F"], // strikt fuer Frachter-only Bids
```

Plus 2 Unit-Tests:
- `md11_matches_md_11f_long_form` — alle MD11/MD11F/MD-11/MD-11F Kombinationen
- `md11_does_not_match_unrelated_widebodies` — MD11 darf nicht mit B77W/A359/B748 matchen

### Effekt

Cargo-Bid mit MD11-ICAO + Sim mit MD-11F → Start funktioniert. Pure-Frachter-Bid mit MD11F-ICAO bleibt strict (TFDi-Design "MD-11F"-Title matched). Andere Widebodies bleiben blockiert wenn falscher Typ geladen.

### Tests

- 46/46 lib (vorher 44 — 2 neu fuer MD11)
- 30/30 landing-scoring + 8/8 goldenset
- 8/8 touchdown_v2_replay
- **Gesamt 92/92 Tests grün**

### Pilot-Workaround vor v0.7.2

Wer schon v0.7.1 hat: `VFR Start (manuell)` umgeht die Aircraft-Verifikation komplett. Nicht ideal weil Sim-Mismatch dann unbemerkt bleibt — aber funktioniert.

---

## [v0.7.1] — 2026-05-10

🎯 **Landing UX & Fairness — Score wird verstaendlich, fair und konsistent ueberall.**

### Warum

v0.7.0 hat die Landerate-Messung strukturell saniert (Touchdown-Forensik v2). Pilot-Feedback zeigte aber: Pilot versteht den Score noch nicht gut, VFR-Modus wird vom Modal blockiert, sparsame Piloten werden bestraft, App und phpVMS zeigen unterschiedliche Zahlen, der Anflug-Chart erklaert nicht was bewertet wird.

v0.7.1 schliesst diese UX-Luecke ohne den Touchdown-Core anzufassen.

### Was sich aendert

**Spec:** [docs/spec/v0.7.1-landing-ux-fairness.md](docs/spec/v0.7.1-landing-ux-fairness.md) (v1.6 approved nach 5 Review-Runden + 3 Score-Contract-Patches)

**Neue Crate:** `client/src-tauri/crates/landing-scoring/` (~700 Zeilen, 38 Tests)
- Single-Source-of-Truth fuer alle Sub-Score-Algorithmen
- Backend, Frontend, Webapp, Monitor + phpVMS sehen IDENTISCHE Werte fuer denselben PIREP
- Spec §3.1 SSoT — KEIN Recompute in irgendeinem Konsumenten

**Sub-Scores im PIREP-Payload + landing_history.json:** Voll ausgebautes `SubScoreEntry`-Wire-Format mit `score`, `points`, `band`, `label_key`, `value`, `rationale_key`, `tip_key`, `skipped`, `reason`, `warning`. UI rendert direkt aus diesen Felder ohne nachzurechnen.

**Master-Score = gewichteter Aggregate aus allen Sub-Scores** (vorher: Touchdown-Klassifikation aus VS+G+Bounces only). Fuel/Loadsheet/Stability/Rollout fliessen jetzt sichtbar in den Hauptscore. Gewichte 1:1 aus v0.7.0: landing_rate=3, g_force=3, bounces=2, stability=2, rollout=1, fuel=1, loadsheet=1 (NEU).

### Sichtbare Fairness-Aenderungen

**F1 — VFR/Manual-Mode: Start ohne ZFW funktioniert jetzt wirklich.** Modal-ZFW-Feld ist optional, leer = "VFR ohne Loadsheet-Wertung". Backend-Gate gelockert. Loadsheet-Sub-Score wird sauber als "nicht bewertet" markiert (kein 0-Penalty). Bild2-Bug fuer VFR-Piloten geloest.

**F2 — Fuel-Score nur bei echtem `planned_burn`.** Backend-Fallback `planned_block_fuel * 0.9` entfernt. Pilot wird nicht mehr fuer eine Annahme bewertet die er nie selbst geplant hat. Ohne OFP-Trip-Burn → Sub-Score skipped.

**F3 — Asymmetrie: Minderverbrauch wird nicht mehr bestraft.** Bisher zaehlte `-5%` genauso schlecht wie `+5%`. Jetzt:
- Mehrverbrauch (>0%): score-relevant wie v0.7.0 (off_plan=55, very_off=25, way_off=5)
- Minderverbrauch (-5..-15%): Score 95 "Effizient" — KEIN Penalty
- Starker Minderverbrauch (>15% under): Score 85 mit Warning "planned_burn_may_be_off"

Label-Wechsel: "Spritverbrauch" → **"OFP-Treue"** / "OFP compliance" / "Aderenza OFP" (DE/EN/IT).

### Sichtbare Forensik-Anschluesse

**F4 — Forensik-Badge mit Confidence-Pill** im LandingPanel: gruen (High) / blau (Medium) / orange (Low) / rot (VeryLow). Zeigt Pilot wie sicher die Touchdown-Messung war. Source-Tooltip ("Impact Frame", "Smoothed 500ms" etc.) erklaert woher der Wert kommt. Bedingung: `ux_version >= 1 && forensics_version >= 2` (v0.7.0-PIREPs bekommen kein Badge weil keine Confidence-Daten vorhanden).

**F5 — ApproachChart Vorlauf/Gate/Flare-Zonen.** Chart hat jetzt drei farbige Hintergrund-Bands:
- Grau = Vorlauf (>1000 ft AGL — nicht bewertet)
- Blau = Bewertetes Gate (0-1000 ft AGL minus letzte 3 Sekunden vor TD)
- Gelb = Flare-Zone (letzte 3 Sekunden vor TD — separat bewertet)

Plus Legende + Tooltip "Bewertet werden Anflug-Samples zwischen 0 und 1000 ft AGL. Die letzten 3 Sekunden vor Touchdown (Flare-Manoever) sind ausgeschlossen — der Flare wird im separaten Flare-Block bewertet." Adrian-Punkt aus Pilot-Feedback geloest.

**F6 — Flare als eigene Zone** (war schon ab v0.5.43 als post-flight-Block da, jetzt explizit zeitbasiert vom Stability-Gate getrennt).

**F7 — Stability-v2-Felder im PIREP** (in dieser Release nur in PirepPayload exponiert, UI-Detail-Panel kommt v0.7.2): `approach_vs_jerk_fpm` mean, `approach_ias_stddev_kt`, `approach_stable_config: bool`, `approach_excessive_sink: bool`. Webapp/Monitor koennen die Werte ab jetzt lesen.

**F8 — i18n-Audit (DE/EN/IT):**
- "Spritverbrauch" → "OFP-Treue"
- "stability" → "Anflug-Stabilitaet"
- "Loadsheet" + "Flare" als neue Sub-Score-Labels
- Neue Rationales: `efficient`, `very_efficient`, `loadsheet_present`
- Neue Skip-Reason-Strings: `landing.skipped_reason.*` mit "(kein Penalty)"-Hinweis
- Forensik-Block + Confidence-Labels

**F9 — Web/Monitor-Parity:** webapp + monitor lesen jetzt `sub_scores` direkt aus dem PIREP-Payload. Identische Pills (mit deutschen Labels statt rohen Keys), identische Score-Werte. App, Web, Monitor und phpVMS zeigen fuer denselben PIREP IDENTISCHE Zahlen.

### Score-Contract einheitlich (nach 3 Review-Runden)

| Pfad | Score-Wert |
|---|---|
| `body.score` (phpVMS native /file) | Aggregate-Master |
| `build_pirep_fields` "Landing Score" | Aggregate-Master |
| `PirepPayload.landing_score` (MQTT) | Aggregate-Master |
| `LandingRecord.score_numeric` (UI) | Aggregate-Master |
| `LandingRecord.score_label` (UI) | Aggregate-Label (smooth/firm/...) |
| Discord-Embed PIREP-Filed | Aggregate-Master |
| Discord-Embed Divert | Aggregate-Master |
| Activity-Log Title | Touchdown-Klasse (bewusst, mit Label) |
| Activity-Log Detail | "Master 77/100" |

### Skipped Sub-Scores sichtbar

VFR/Manual-Pilot ohne ZFW + Trip-Burn-Plan: Sub-Scores "Loadsheet" und "OFP-Treue" werden als graue dashed-Pills mit Tooltip ("Kein OFP-Trip-Burn — nicht bewertet, kein Penalty") angezeigt — vorher verschwanden sie einfach. Pilot sieht jetzt warum keine Wertung erfolgt. Auch in webapp + monitor.

### Backward-Compat (Spec §3.5 Legacy-Schutz)

Pre-v0.7.1-PIREPs (`ux_version < 1`) zeigen den alten Master-Score wie zum Aufzeichnungszeitpunkt — **keine Re-Score-Verwirrung**. UI rechnet alte Records nicht mit neuer Logik nach. Marker-System: `forensics_version: 2` (aus v0.7.0) + `ux_version: 1` (NEU v0.7.1).

### Score-Drift-Tabelle (Phase-2 erwartete Aenderungen)

| Flug | v0.7.0 | v0.7.1 | Grund |
|---|---|---|---|
| PTO 105 GA (smooth) | 95 | 95 | unveraendert |
| DLH 304 (-3.5% Fuel) | 74 | 77 | F3: Minderverbrauch nicht bestraft |
| CFG 785 (smooth) | 94 | 95 | loadsheet=100 mit drin |
| DAH 3181 (firm + Mehrverbrauch) | 65 | 68 | Mehrverbrauch unveraendert, loadsheet=100 |

### Neue Datenfelder

**Storage (`LandingRecord` + `ApproachSample` Erweiterung):**
- `ux_version`, `forensics_version`, `landing_confidence`, `landing_source`
- `approach_vs_jerk_fpm`, `approach_ias_stddev_kt`, `approach_stable_config`, `approach_excessive_sink`
- `gate_window` (start/end ms + heights + count)
- `sub_scores: Vec<SubScoreEntry>`
- `ApproachSample.t_ms`, `agl_ft`, `is_scored_gate`, `is_flare`

Alle neuen Felder mit `#[serde(default)]` — alte `landing_history.json`-Dateien lesen weiter ohne Crash.

**MQTT (`PirepPayload`):** identische Erweiterung. v0.7.1+ Web/Monitor sehen die neuen Felder, alte VPS-Versionen ignorieren sie.

### Was NICHT in v0.7.1

- F7-B `sub_stability` 4-Faktor-Voting (bleibt 2-Faktor wie v0.7.0 fuer Backward-Compat — die neuen Felder sind in PirepPayload aber Score-Algorithmus unveraendert)
- StabilityDetailPanel + FlareDetailPanel UI (Felder durchgereicht, dedizierte UI kommt v0.7.2)
- Re-Score historischer PIREPs (forward-only)
- Mobile Frontend
- WASM-Live-Score-Vorschau

### Tests

- `cargo test --lib`: 44/44 GREEN (v0.7.0 Backbone unangetastet)
- `cargo test -p landing-scoring`: 30 unit + 8 goldenset = 38/38 GREEN
- `cargo test --test touchdown_v2_replay`: 8/8 GREEN
- `tsc --noEmit` (client + webapp + monitor): clean

**Gesamt 90/90 Tests grün** ueber alle Crates und Frontends.

---

## [v0.7.0] — 2026-05-10

🏗 **Touchdown-Forensik v2 — strukturelles Redesign der Landerate-Berechnung.**

### Warum

Pilot-Bug-Report 2026-05-10: X-Plane DAH 3181 zeigte +104 fpm POSITIVE Landerate (physikalisch unmöglich). Tiefe Ursachen-Analyse + 3 Review-Runden mit dem VA-Owner ergaben **9 zusammenhängende Bugs** gleicher architektonischer Wurzel:

- vs_at_edge unconditional-override ohne Plausibilitäts-Prüfung → positive Landerate möglich
- Single-shot TD-Detection (`is_none()`-Guard) verhindert zweiten TD bei T&G/Go-Around
- X-Plane on_ground edge ist trigger-happy → 44ms Float-Streifschuss wird als TD erkannt
- bounce_count Inkonsistenz zwischen 50Hz Sampler und Streamer-Counter
- Keine Confidence-Tagging
- ...

### Spec

`docs/spec/touchdown-forensics-v2.md` (v2.3, approved nach 3 Review-Runden). 4-Layer-Architektur:

1. **TD-Candidate Detection** (sim-spezifisch): X-Plane mit gear_force-edge ODER on_ground; MSFS nur on_ground
2. **TD-Validation** (sim-spezifisch): X-Plane MUST-PASS gear_force (mass-aware Threshold) + 2 Plausibilitäts-Tests; MSFS 3-of-4 Voting; Fallback auf 4-of-4 für legacy-X-Plane ohne gear_force
3. **VS-Calculation am IMPACT-Frame** (sim-agnostic): contact_frame ≠ impact_frame ≠ load_peak_frame. VS-Cascade: vs_at_impact → smoothed_500ms → smoothed_1000ms → pre_flare_peak → REJECT mit HARD GUARDS (niemals positiv, niemals < -3000 fpm)
4. **LandingEpisode-Aggregation**: false_edges + contact + low_level_touches (Bounces) + settle. Multi-TD-Lifecycle für T&G/Go-Around. Härtester Impact in Episode = Bounce-Score-Regel.

### Was sich ändert

**Neue Module:**
- `client/src-tauri/src/touchdown_v2.rs` (~700 Zeilen, 15 unit-tests)
- `client/src-tauri/tests/touchdown_v2_replay.rs` (6 Replay-Tests gegen echte Pilot-JSONLs)

**Schema-Erweiterung (backward-compatible):**
- `TouchdownWindowSample` bekommt `gear_normal_force_n` + `total_weight_kg` als Optional fields
- Sampler füllt sie aus dem Sim-Snapshot
- Alte JSONLs deserialisieren weiter via serde-default

**Sampler-Refactor (minimal-invasiv):**
- vs_at_edge unconditional-override ersetzt durch `touchdown_v2::compute_landing_rate` cascade
- Multi-TD via Climb-out-Reset: nach Dump + agl > 100ft AGL werden TD-state-fields zurückgesetzt → nächster TD wird erfasst
- bounce_count aus 50Hz `analysis` (Wahrheit) statt Streamer-Counter (Inkonsistenz-Bug fix)
- forensics_version = 2 als Footer in PIREP-notes

**Verifiziert gegen 6 echte Test-Flüge (Replay-Acceptance-Tests):**

| Flug | Sim | Vorher | Nachher | Status |
|---|---|---|---|---|
| PTO 105 GA | MSFS | -55/100 | -55/smooth | ✓ unverändert |
| DLH 304 | MSFS | -357/80 | -357/acceptable | ✓ unverändert |
| CFG 785 (EDDV-EDDB) | MSFS | -142/100 | -142/smooth | ✓ unverändert |
| DLH 742 (EDDM-RJBB) | MSFS | -191/100 | -191/smooth | ✓ unverändert |
| **DAH 3181 (ZGGG-DAAG)** | **X-Plane** | **+104/80 ❌** | **-414/firm + Float false_edge** | **✓ FIX** |
| PTO 705 T&G | MSFS | -182 vom Streifschuss | erste Episode -182 + low_level_touches | ✓ |

**Plus:** 4/4 MSFS-Flüge bit-identisch zu vorher (= Spec-Acceptance Sektion 10 erfüllt, keine Regression).

### HARD GUARDS strukturell

```rust
fn finalize_vs(candidate_fpm: f32) -> Result<f32, RejectionReason> {
    if !candidate_fpm.is_finite() { return Err(EmptyWindow); }
    if candidate_fpm > 0.0       { return Err(PositiveVs); }
    if candidate_fpm < -3000.0   { return Err(ImplausiblyHigh); }
    Ok(candidate_fpm)
}
```

**Niemals positive Landerate möglich.** Bei REJECT bleibt Score auf primary-chain Wert (kein automatischer Override mit unsicherem Wert).

### Was BEWUSST NICHT in v0.7.0

- Frontend-Confidence-Badge (kommt v0.7.1)
- Episode-Anzeige im Cockpit (kommt v0.7.1)
- Re-Score alter PIREPs (forward-only)
- Per-gear contact / Throttle / N1 / Spoilers / Autobrake (addon-unzuverlässig)
- Synthetic-TD Auto-Score (nur Review-Banner)

### Geänderte Dateien

- `client/src-tauri/Cargo.toml` — neue dev-dependency `flate2` für Replay-Tests
- `client/src-tauri/crates/recorder/src/lib.rs` — TouchdownWindowSample Schema-Erweiterung
- `client/src-tauri/src/lib.rs` — TelemetrySample-Felder, Sampler-Capture, Sampler-Loop (vs_at_edge → v2 cascade + Multi-TD-Reset + bounce_count fix), PIREP-notes-Footer
- `client/src-tauri/src/touchdown_v2.rs` — neues Modul (~700 Zeilen)
- `client/src-tauri/tests/touchdown_v2_replay.rs` — neue Replay-Acceptance-Tests
- `client/src-tauri/tests/fixtures/*.jsonl.gz` — 6 echte Pilot-JSONLs für CI-Tests
- `docs/spec/touchdown-forensics-v2.md` — vollständige Architektur-Spec (v2.3)
- Versionen → 0.7.0

---

## [v0.6.2] — 2026-05-10

🩹 **Hotfix v0.6.1 → v0.6.2 — drei Bugs aus dem Pilot-Test-Flight CFG 785 EDDV→EDDB gefixt.**

### Test-Flight-Befund

Pilot Test-Flight komplett analysiert (JSONL: 1375 events, 0 unerwartete Lücken, Touchdown-Score 100/100 auf EDDB 06R, 591m past threshold, 2.2m left of centerline). 96-Sekunden Resume-Lücke (= App-Restart-Test) sauber recovered. Aber drei UX/Korrekturheft-Bugs gefunden:

### 🟡 Bug #1 — Indikator-Wackler „1 Position offline ↔ live"

### Pilot-Report (Test-Flight CFG 785 EDDV→EDDB im Pushback)

Indikator zeigte abwechselnd „OFFLINE 1 Position offline · Σ 251" und kurz „live", obwohl alles funktionierte (POSTs gingen raus, JSONL komplett, Live-Map auf VPS aktiv).

### Root-Cause

Der v0.6.1-Fix für Bug #7 hatte den UX-Indikator nicht KOMPLETT gefixt. Die Worker-Loop hatte nach dem `match` (Erfolg/Failure-Branches) noch einen **unconditional queued_position_count update** der den korrekt gesetzten 0-Wert aus dem success-Branch überschrieb mit dem race-condition-Wert:

```rust
match post_fut.await {
    Ok(Ok(())) => { stats.queued_position_count = 0; }  // ← korrekt!
    ...
}
// Außerhalb des match — race window:
let total_after = outbox.lock().len();
stats.queued_position_count = total_after as u32;  // ← ÜBERSCHREIBT mit 1!
```

**Sequenz:**
1. t=0: Worker drained outbox (z.B. 1 Item) → POST success → field=0 ✓
2. **Zwischen Zeile 6820 und 6856: Streamer pusht 1 fresh item** (Pushback-Phase, Streamer pusht alle ~3s)
3. t=0 unconditional update: `field = outbox.len() = 1` ❌
4. t=1, t=2, t=3: Worker tick `if !due continue` (Pushback interval=4s) → field bleibt 1 → UI zeigt „1 Position offline"
5. t=4: Worker postet erneut → field=0 → kurz später Race → 1 → ...

### Fix

Unconditional update raus. queued_position_count wird jetzt **NUR im match-arm** gesetzt mit korrekter Semantik:

- **success-arm:** `queued_position_count = 0` (egal was outbox.len() ist — der nächste POST nimmt es mit raus)
- **failure-arm + timeout-arm:** `queued_position_count = outbox.len()` nach `requeue_batch` (= echter „stuck items" Backlog)
- **404-arm:** Worker terminiert sauber (kein Update nötig)

Damit matcht die Semantik jetzt v0.5.x: field = „stuck items wegen Network-Problem", 0 sonst. UI zeigt durchgehend „live" im normalen Betrieb, „queued" nur bei echten Connection-Issues.

### 🟡 Bug #2 — MQTT-Initial-Phase-Publish überschreibt echte Phase nach Resume

### Pilot-Report

Beim App-Restart mid-flight (CFG 785 im Climb auf 12k ft) zeigte die Live-Map auf live.kant.ovh für ~5 Sekunden „PREFLIGHT" — obwohl die App-State (FlightStats.phase) korrekt CLIMB war.

### Root-Cause

Im `MqttHandle::new()` (Login-Zeit) gab es einen unconditional retained Phase-Publish:

```rust
// crates/aeroacars-mqtt/src/lib.rs:763
let initial_phase = PhasePayload {
    ts: chrono::Utc::now().timestamp_millis(),
    phase: phase_label(FlightPhase::Preflight),  // ← FALSCH!
};
publish_json(..., topic("phase"), &initial_phase, QoS::AtLeastOnce, true).await;
```

**Sequenz beim Resume:**

1. App startet → MQTT-Handle init → publisht `phase=PREFLIGHT` retained
2. VPS-Subscriber bekommt PREFLIGHT → DB `current_phase=PREFLIGHT`
3. Streamer startet später (nach `flight_resume_confirm` button click)
4. Erste Position-Payload hat `phase=CLIMB` → DB wird korrigiert
5. Race-Window zwischen 1 und 4 = ~3-5s sichtbar als „PREFLIGHT" auf der Live-Map

### Fix

Initial-Phase-Publish komplett entfernt. Wenn ein Flug aktiv ist, sendet der Streamer beim ersten Tick die echte Phase im position-payload (das embed wurde in v0.5.14 nachgezogen). Wenn kein Flug aktiv → Monitor zeigt „—" (korrekt, kein Flug = keine Phase).

Der retained-message vom letzten Flug bleibt im Broker bis der nächste Streamer-Tick eine neue Phase sendet — das ist OK weil der Subscriber den position-payload schneller sieht als ein Monitor connected.

### 🟡 Bug #3 — Indikator-Status-Semantik „offline" für jeden Backlog (UX-Verwirrung)

### Pilot-Frage

> „Wie wir vorgehen bei der Anzeige offline → das verwirrt den Piloten. Offline heißt offline — aber er ist doch nicht offline oder?"

### Root-Cause

Vor v0.6.2 hatte der Indikator vier Status: `live` / `queued` / `stale` / `idle`. „queued" wurde gerendert mit dem Label „X Positionen offline" — aber das deckte zwei verschiedene Fälle ab:

| Was tatsächlich ist | Was angezeigt wurde | Was Pilot dachte |
|---|---|---|
| Cruise, 5 items warten auf nächsten 30s-POST (= NORMAL) | „5 Positionen offline" | „Mist, Connection weg!" |
| Echte Connection weg, POST scheiterte | „5 Positionen offline" | „Mist, Connection weg!" |

→ Beide Fälle sahen IDENTISCH aus, aber nur einer war ein Problem.

### Fix

Drei klar getrennte Status statt zwei:

| Status | Wann | Farbe | Label DE | Label EN |
|---|---|---|---|---|
| **Live** | queued=0, letzter POST ✓ | 🟢 grün, Pulse | „LIVE" | „LIVE" |
| **Sync** | queued>0, letzter POST ✓ | 🔵 blau, soft Pulse | „SYNC · X Positionen werden gesendet" | „SYNC · X positions syncing" |
| **Offline** | letzter POST ✗ (echter Connection-Loss) | 🔴 rot, kein Pulse | „OFFLINE · Verbindung verloren — X Positionen warten" | „OFFLINE · Connection lost — X positions waiting" |
| **Stale** | seit 3 min nichts gepostet | ⚪ grau | „FEHLER" | „STALLED" |

**Implementation:**

- Backend: neues field `ActiveFlight.connection_state: AtomicU8` (0=Live, 1=Failing). Worker setzt es nach jedem POST-Versuch.
- IPC: `flight_status` exposed das field als `connection_state: "live" | "failing"`.
- Frontend: `LiveRecordingIndicator` priorisiert Status: Stale > Offline > Sync > Live.
- i18n: neue keys in DE/EN/IT für `recording.status.sync`, `recording.status.offline`, `recording.sync_pending`, `recording.offline_pending`. Alte „queued" keys bleiben als legacy für Backward-Compat.
- CSS: neue Klassen `.live-rec--sync` (blau) und `.live-rec--offline` (rot).

Plus `SettingsPanel` updated — Position-Queue-Row zeigt jetzt „X · wird gesendet" oder „X · ausstehend (offline)" je nach connection_state.

### Geänderte Dateien

- `client/src-tauri/src/lib.rs` — Worker-Loop unconditional queued_count-Update raus, success/failure match-arms setzen field selbst mit korrekter Semantik. Plus neues `ActiveFlight.connection_state: AtomicU8` field, Worker setzt es bei success/failure. Plus `ActiveFlightInfo.connection_state` für IPC.
- `client/src-tauri/crates/aeroacars-mqtt/src/lib.rs` — Initial-Phase-Publish entfernt.
- `client/src/types.ts` — neuer `connection_state: "live" | "failing"` field auf `ActiveFlightInfo`.
- `client/src/components/LiveRecordingIndicator.tsx` — 3 Status (live/sync/offline) statt 2 (live/queued).
- `client/src/components/SettingsPanel.tsx` — Position-Queue-Label hängt am `connection_state`.
- `client/src/App.tsx` — `connectionState` prop an `LiveRecordingIndicator` durchreichen.
- `client/src/App.css` — neue Klassen `.live-rec--sync` (blau) und `.live-rec--offline` (rot).
- `client/src/locales/{de,en,it}/common.json` — neue i18n keys.
- Versionen → 0.6.2

---

## [v0.6.1] — 2026-05-10

🩹 **Audit-Fixes vor v0.6.0-Rollout — der phpVMS-Worker batched jetzt wirklich.**

### Hintergrund

v0.6.0 wurde gebaut + auf GitHub-Releases gepusht, aber NICHT als „Latest" markiert (Pilot-Schutz). Independent-Code-Review hat Bugs gefunden, die in v0.6.0 selbst noch drin waren. Statt v0.6.0 mit Bugs als Latest zu setzen, wurde v0.6.0 zum Draft demoted und v0.5.51 blieb Latest, bis v0.6.1 mit den Fixes raus ist. **v0.6.0 wird nie als Latest released — Piloten gehen direkt von v0.5.51 auf v0.6.1.**

### 🔴 Bug #1 — phpVMS-Worker postete Items SINGLE-FILE statt batched

In v0.6.0 initial: `MAX_BATCH=50` zog 50 Items aus der Outbox, aber dann lief ein `for position in batch { client.post_positions(&[position.clone()]).await }` — also 50 separate HTTP-Calls. Bei 50ms RTT = 2.5 s pro Drain statt einer 70ms-Anfrage. Bei 5-sec Per-Item-Timeout auf einer flaky Verbindung: bis zu 250 s pro Drain. Hätte den ganzen Sinn des Refactors halb umsonst gemacht.

**Fix:** Echter Batch-POST — `client.post_positions(&flight.pirep_id, &batch)` als ein einziger HTTP-Call. Per-Item-Timeout (5s) auf Per-Batch-Timeout (15s) umgestellt. Bei Failure geht der KOMPLETTE Batch zurück in die Outbox via neue `requeue_batch`-Helper (push_front in reverse-iter erhält chronologische Reihenfolge).

### 🟡 Bug #2 — `position_queue.json`-Read-Errors silent geswallowed

`if let Ok(items) = q.read_all()` im Worker-Init verschluckte File-Read-Errors. Wenn die queue.json nach einem Power-Cut korrupt ist, sind alle persistierten Positions stillschweigend weg — kein Log, kein Indikator.

**Fix:** Explizites `match` mit `tracing::warn!` bei Read-Failure und `deserialize_failed`-Counter im Success-Log.

### 🟡 Bug #3 — Outbox-Cap-Drop war silent

Wenn die Outbox > 5000 Items wuchs, wurden ältere Positions still gedroppt — kein Log, kein Activity-Feed-Eintrag. Nach 8h Netz-Outage auf einem Long-Haul: die Start-of-Flight Punkte (Departure-Climb, TOC) verschwinden aus dem Live-Map ohne Warnung. JSONL-Forensik bleibt komplett, aber der Pilot hat kein Signal warum sein Track kürzer wird.

**Fix:** `tracing::warn!` pro Tick mit `dropped_this_tick`-Counter und expliziter Klarstellung dass JSONL-Forensik noch komplett ist.

### 🟡 Bug #4 — `persist_outbox` löschte Items von anderen pireps

Erste persist_outbox-Implementierung machte `queue.replace(&items)` mit nur den aktuellen pirep-items → wenn queue.json items von einem anderen pirep_id hatte (App-Crash mid-flight eines prior flights), wurden die zerstört. Plus: bei leerer Outbox returned die Funktion ohne write → ältere Items des aktuellen pireps blieben in queue.json und wurden beim nächsten Start als Duplikate re-posted.

**Fix:** Read-modify-write Pattern — read all, filter aktuellen pirep raus, append outbox snapshot, write combined back. Auch leere Outbox triggert write (= file gelöscht wenn nichts mehr da).

### 🔴 Bug #5 — `position_interval(phase)` faelschlich entfernt → fix 3s im Worker statt phase-aware

In meinem ersten v0.6.1-Pass hatte ich die `position_interval(phase)`-Funktion gelöscht und den Worker auf fix 3s-Cadence umgestellt — mit der Begründung „eine fixe Cadence + Batching von 50 Items effektiver als Phase-aware". **Das war Quatsch.** Der Pilot hat mich darauf gestoßen.

`position_interval(phase)` hatte einen realen Sinn: im Cruise muss phpVMS nur alle **30s** ein POST sehen (langer gerader Leg, sparse samples reichen für die Live-Map), im **Pushback nur alle 4s** (sonst wird die Phase verpasst, weil sie in 8-15s vorbei sein kann), im **Approach 8s** (präziser inbound Track). Mit fix 3s hätte der Worker 10× mehr POSTs im Cruise produziert als nötig — Bandbreite, phpVMS-Server-Load, DB-Bloat.

**Fix:** Funktion `position_interval(phase)` ist wieder zurück. Worker-Loop tickt jetzt mit kurzer **TICK=1s** (responsive Stop-Check + Backoff-Aufloesung), aber die ECHTE POST-Cadence kommt aus `position_interval(phase)`. Tracking via `last_post_at: Option<Instant>` — gepostet wird nur wenn `last_post_at.elapsed() >= interval`. Resultat:

- **Cruise:** Worker tickt 1s, postet aber alle 30s — 30 Items im Batch (Streamer pusht alle ~3s). Eine HTTP-Anfrage pro halbe Minute statt 10.
- **Pushback:** Worker postet alle 4s mit dem aktuellen Item.
- **Approach/Final:** alle 8s, im Touchdown-Frame (sampler 50Hz) sind alle ~16 frames in der Outbox.

Plus: **Exponential Backoff non-blocking umgebaut.** Vorher war's `tokio::time::sleep(extra_secs).await` im Loop — blockte den responsive Stop-Check. Jetzt: `backoff_until: Option<Instant>` wird gesetzt, der Loop-Top-Check skipped bis dahin. Stop-Signal wird in jedem TICK=1s erkannt selbst während Backoff läuft.

### 🔴 Bug #6 — Orphan-Persist-Race im Stop-Pfad

Audit-Pass nach Bug #5 hat einen weiteren echten Bug gefunden: Stop-Pfade machen `outbox.clear()` gefolgt von `stop=true`. Zwischen diesen zwei Aufrufen kann der Streamer-Tick noch 1+ Items in die Outbox geschoben haben (Race-Window klein aber real). Worker sieht im nächsten Tick `stop=true` → ruft `persist_outbox` → die orphan items des cancelled/filed pireps landen in `position_queue.json` und rotten dort für immer (Worker-Init-Load filtert sie raus aber löscht sie nicht).

**Fix (initial):** Worker im stop-branch macht selbst nochmal `outbox.clear()` BEVOR er persist_outbox aufruft. **3rd-Audit hat das aber als nicht-atomar erkannt** — clear() droppt den Lock am Semicolon, persist_outbox acquired ihn wieder = Microsekunden-Race-Window bleibt. **Echter Fix:** neue Funktion `persist_outbox_clearing()` die clear+snapshot ATOMAR unter dem GLEICHEN Lock-Hold macht. Race-Window strukturell zu.

Semantisch korrekt für alle 5 Stop-Pfade: bei Cancel/Forget will der User explizit nichts mehr; bei Filing hat die JSONL alle Position-Events für Forensik-Upload; bei remote_cancellation würde der POST eh 404 zurückgeben.

### 🟡 Bug #7 — UX-Regression: queued_position_count bedeutete in v0.5.x „echter Backlog", in v0.6.0/v0.6.1 anfangs „alles in der Outbox"

In v0.5.51: `queued_position_count` zeigte nur stuck items (failed POSTs in der file-queue) → 0 unter normalen Bedingungen → Indikator zeigte „live" im Cruise.

In v0.6.0/v0.6.1 anfangs: Streamer pusht alle ~3s in die Memory-Outbox UND setzte sofort `queued_position_count = outbox.len()`. Worker drained nur alle 30s im Cruise. Resultat: Outbox 29 von 30 Sekunden > 0 → Indikator zeigt durchgehend „queued (offline)" obwohl alles funktioniert.

**Fix:** Streamer-Tick setzt `queued_position_count` NICHT mehr. Nur der Worker setzt es nach jedem POST (success → outbox.len() = was nach Drain übrig ist; failure → outbox.len() = was nach Requeue drin steht). Im Cruise: nach success ist outbox leer → count=0 → Indikator zeigt korrekt „live".

### 🔴 Bug #8 — Hardkill-Datenverlust für phpVMS Live-Map (v0.6.0/v0.6.1 anfangs: bis zu 499 Items weg)

In v0.5.51 lief der phpVMS-POST inline im Streamer-Tick → bei Hardkill mid-flight verlor man max 1-2 Positions für die Live-Map (rest war eh schon gepostet).

In v0.6.0/v0.6.1 anfangs: Persist nur bei Outbox >= 500 mit 30s-Cooldown. Bei Hardkill mit Backlog=499 und letzter persist 29s her: bis zu **499 positions verloren** für phpVMS Live-Map (JSONL-Forensik bleibt komplett, aber phpVMS hat sie nicht). Realistisch im Cruise: 30-60 Items pro Hardkill-Event verloren.

**Fix:** Persist-Trigger drastisch verschärft. Statt „nur bei Backlog >= 500" jetzt: **alle 30s wenn Outbox nicht leer**. Begrenzt Crash-Verlust auf ~30s an positions (= 1-10 Items je nach Phase) statt potenziell 499. Im Stop-Branch wird weiterhin IMMER persistiert (atomar via `persist_outbox_clearing`).

### 🟡 Bug #9 — `persist_outbox` Hysteresis bei steady-state Backlog (subsumed in #8)

Bei outbox.len() >= 500 wurde `persist_outbox` jeden TICK=1s aufgerufen. Bei steady-state outbox≈500: full file rewrite jede Sekunde, ~100KB+ pro Write.

**Fix:** `last_persist_at: Option<Instant>` mit `PERSIST_INTERVAL=30s`. Mit dem #8-Fix automatisch behoben weil dieselbe 30s-Cadence jetzt für ALLE Persists gilt (nicht nur backlog-getriggerte).

### Spawn-Order konsistent

In allen 3 Spawn-Sites (flight_start / flight_resume_after_disconnect / flight_resume_confirm) wird jetzt `spawn_phpvms_position_worker` ZUERST aufgerufen, dann der Streamer + Sampler. Hint an den Scheduler dass der Worker-Init-Load aus queue.json fertig sein soll bevor der Streamer fresh items pusht (chronologische Reihenfolge in der Outbox).

### Geänderte Dateien

- `client/src-tauri/src/lib.rs` — Worker-Loop komplett überarbeitet (echtes Batch-POST mit BATCH_TIMEOUT=15s, requeue_batch-Helper mit reverse-iter push_front, non-blocking exponential backoff via backoff_until: Option<Instant>, queue-read-error logging, deserialize-counter, persist_outbox read-modify-write mit other-pirep-preservation, persist-hysteresis 30s cooldown bei steady-state backlog, orphan-persist-race fix via clear-before-persist im stop-branch, phase-aware POST-Cadence über position_interval(phase) + last_post_at-Tracking statt fix 3s); Streamer-Tick: outbox-cap-drop logging; spawn-order in flight_start + flight_resume_after_disconnect umgedreht
- Versionen → 0.6.1

---

## [v0.6.0] — 2026-05-10 (DRAFT — never released)

> **Note:** v0.6.0 wurde gebaut, aber wegen den in v0.6.1 gefixten Bugs nie als „Latest" promoted. Piloten ziehen direkt v0.5.51 → v0.6.1. Der Architektur-Beschrieb unten ist der Stand wie er in v0.6.1 ausgeliefert wird.

🏗 **Strukturelles Redesign: Streamer-Tick komplett vom phpVMS-IO entkoppelt.**

### Warum

Wir hatten in v0.5.x eine **Klasse von Bugs** angesammelt, die immer wieder dasselbe Symptom hatte: irgendwas im Streamer-Tick blockierte → Live-Map einfriert, JSONL-Loch, MQTT-Stille, im Extremfall Sim-Disconnect-Annahme weil die Heartbeats stalled. Jeder Hotfix hat den jeweils konkreten Hänger entkoppelt (v0.5.49: POST in `tokio::spawn`; v0.5.51: Drain in `tokio::spawn` mit Cap+Timeout). Aber die **Architektur selbst** — „der Streamer-Tick macht alles" — produzierte garantiert den nächsten Bug derselben Klasse.

User-Wunsch nach v0.5.51: *„wir haben die ganze Nacht Zeit — neu denken kein bugfixing mehr — wie können wir das besser machen so das wir aber alle Daten behalten — hart denken !! Komplettes Redesign"*. Plus klare Ansage: keine Feature-Flag-Fallbacks, weil *„wenn der alte misst drin ist haben wir doch wieder das gleich Problem"*.

### Was sich strukturell ändert

**Vorher (v0.5.x):** Streamer-Tick (1 Loop, alle 0.5–3 s) machte Snapshot-Read **+** FSM-Step **+** JSONL-Write **+** MQTT-Publish **+** phpVMS-POST **+** Queue-Drain **+** Heartbeat **+** Persist-Stats. Jedes Sub-Step konnte den ganzen Tick blockieren. Workarounds: „Critical-Window" (AGL <1500 ft → POST pausieren), file-backed `position_queue.json` als Failover, jeder Failure-Path eigene Spawn-Logik.

**Nachher (v0.6.0):**

- **Streamer-Tick** macht nur noch: Snapshot lesen → FSM-Step → JSONL-Write → MQTT-Publish (non-blocking) → push in **Memory-Outbox**. Pures CPU + lokales File-IO. Blockiert *strukturell* nicht mehr auf phpVMS.
- **`spawn_phpvms_position_worker`** ist ein eigener async Task pro Flug. Tickt mit `TICK=1s` (responsive Stop-Check), POST-Cadence kommt aus `position_interval(phase)` (4-30s je nach Flugphase). Bis 50 Items pro Batch in einem HTTP-POST mit `BATCH_TIMEOUT=15s`. Bei Failure: KOMPLETTER Batch zurück in die Outbox + non-blocking exponential Backoff (3,6,12,24,48,60s gecapped). Bei 404: PIREP wurde server-seitig gelöscht → Worker terminiert sauber.
- **Memory-Outbox** (5000 Items max ≈ 8 h Cruise-Daten) ist die Single-Source-of-Truth für ungesendete Positions. Persistierung in `position_queue.json` nur noch lazy (Worker-Stop oder Backlog ≥ 500 mit 30s-Hysteresis) für App-Restart-Recovery.
- **50-Hz Touchdown-Sampler** (eigener Task seit v0.5.39) bleibt unverändert — der war nie das Problem.

### Was raus ist

- **Critical-Window-Pausierung im Streamer-Tick** — nicht mehr nötig, der Streamer macht eh kein phpVMS-IO mehr
- **`drain_position_queue` + `spawn_position_queue_drain` + `enqueue_position_offline`** — die ganze File-Queue-Drain-Logik im Tick. Worker liest die Outbox direkt
- **`queue_drain_in_flight: AtomicBool`** — Guard gegen parallele Drains, nicht mehr nötig
- **`last_phpvms_post_at`-Tracking im Streamer** — Cadence-Steuerung sitzt jetzt im Worker
- **`recorder_core.rs`-Skeleton** — initial als Komplett-Refactor-Modul angelegt, aber Targeted-Refactor (alles in `lib.rs`, nur die Workarounds raus, bewährter `step_flight` bleibt 1:1) hat sich als pragmatischer ohne Test-Suite herausgestellt

### Datenintegrität

JSONL ist wie vorher die Single-Source-of-Truth. Jede Position wird **vor** dem Outbox-Push in die JSONL geschrieben. Wenn der phpVMS-Worker stundenlang kein Netz hat: Outbox füllt bis 5000, ältere Items werden gedroppt aus dem Live-Stream — aber die JSONL hat sie alle, und der Forensik-Upload nach PIREP-Filing zieht sie nach.

### Verhalten beim Cancel/Forget/Filing

- **`flight_end` (Filing):** Outbox wird vor `stop=true` geleert. PIREP ist serverseitig akkzeptiert, weiteres POSTen ist sinnlos. Forensik-Upload (file_pirep-Anhang) enthält die JSONL mit allen Position-Events.
- **`flight_cancel`:** Outbox wird vor `stop=true` geleert. Pilot will *explizit* nichts mehr senden.
- **`flight_forget`:** Outbox geleert.
- **`handle_remote_cancellation` (PIREP serverseitig weg):** Outbox geleert, Worker terminiert.

### Geänderte Dateien

- `client/src-tauri/src/lib.rs` — neue `ActiveFlight.position_outbox` (`Mutex<VecDeque<PositionEntry>>`) + `phpvms_worker_spawned`-Guard, neue `spawn_phpvms_position_worker` + `persist_outbox`-Helper, Streamer-Tick pusht in Outbox statt direktem POST, alle 3 Spawn-Sites (flight_start / flight_resume / flight_resume_after_disconnect) wired, alle 5 stop-Pfade (cancel/forget/end/end_with_overrides/remote_cancellation) clearen die Outbox vor stop=true
- `client/src-tauri/src/recorder_core.rs` — gelöscht (Komplett-Refactor-Skeleton wurde nicht weiter verfolgt)
- `client/src/components/LiveRecordingIndicator.tsx` — Stale-Threshold-Kommentar auf v0.6.0 aktualisiert (180 sec ist großzügig genug für die phase-aware POST-Cadence bis 30s im Cruise plus 1-2 Backoff-Cycles bei Connection-Issues)
- Versionen → **0.6.0** (Minor-Bump, weil internes Daten-/Worker-Modell sich ändert; UI + persistente Files bleiben backward-compatible)

### Risiko

Erstes Major-Refactor seit v0.5.0 ohne Test-Suite. Strategie: **Forward-only**, kein Feature-Flag-Fallback. Wenn ein Showstopper-Bug auftaucht, wird die v0.5.51-Tag als Hotfix-Basis genutzt.

---

## [v0.5.51] — 2026-05-09

🩹 **Hotfix: Live-Map endete am Touchdown statt am Gate (v0.5.45-Regression).**

### Hintergrund

User-Report: Pilot 22 (Michael, PTO 705 LICR→LICC) — Live-Map zeigte den Track bis zum Touchdown, dann **5-Min-Stille** bis zum Block-On. Im JSONL: 295 Sekunden komplett kein Event (kein MQTT-Publish, kein JSONL-Append, kein Activity-Log). Symptom war reproduzierbar: vor v0.5.45 lief das einwandfrei, ab v0.5.45 brach der Stream nach Touchdown ab.

### Root-Cause

Klassischer **Sequential-Await-Block** im Streamer-Tick:

```rust
if !in_critical_window {
    drain_position_queue(q, &client, &flight.pirep_id).await;  // ← BLOCKIERT
}

// drain_position_queue itself:
for q in items {
    client.post_positions(...).await   // ← await pro Item, kein Timeout
}
```

**Die v0.5.45-Verkettung:**

1. v0.5.45 erhöhte die Critical-Window-AGL-Schwelle von 300 → **1500 ft** (User-Wunsch: dichtere Sample-Cadence im Final). Plus 60-sec-Extension nach jedem agl_low-Sample.
2. Während Critical-Window werden Position-POSTs **gequeued** statt gesendet (existiert seit v0.5.39).
3. Bei adaptive 500-1000 ms Tick + 5+ min Final-Approach sammelten sich **300-600 Items** in der Queue.
4. Nach Touchdown endet das Critical-Window → Drain feuert sequentiell mit `.await` pro Item.
5. Bei NAT-Eviction (Fehler 1236) hängt jeder POST bis zum 10s HTTP-Timeout → 400 × 10s = **67 Min Drain-Zeit**.
6. Während des Drains blockt der **Streamer-Tick komplett** — kein MQTT-Publish, kein JSONL-Append.

In v0.5.49 hatte ich den **POST entkoppelt** (`tokio::spawn` für `post_positions`), aber den **Drain übersehen** — das `drain_position_queue` blieb `await`-blockiert im Tick.

### Fix

**Drain läuft jetzt in `tokio::spawn`**, mit Per-Item-Timeout + Drain-Cap:

- Neue Funktion `spawn_position_queue_drain()` — fire-and-forget aus dem Streamer-Tick
- `Per-Item tokio::time::timeout(5s)` — ein hängender POST stalled nicht den ganzen Drain
- `MAX_DRAIN_PER_TICK = 50` Items — Drain dauert nie länger als ~5 Min selbst im Worst-Case
- `Per-Flight queue_drain_in_flight: AtomicBool` — verhindert parallele Drains

**Streamer-Tick blockiert nie wieder auf phpVMS.** MQTT-Publish + JSONL-Append + Sampler laufen kontinuierlich auch wenn 1000+ Items in der Queue stehen.

### Was Piloten merken

- **Live-Map-Track läuft kontinuierlich bis zum Gate** — keine Stille mehr nach Touchdown
- **Indikator-Count "X Positionen offline"** geht jetzt langsam runter (bis zu 50 pro 3-sec-Tick = ~1000 Items in 1 Min) statt minutenlangem Drain-Hang
- **PIREP-Filing** läuft genauso wie vorher (separater Code-Pfad)

### Geänderte Dateien

- `client/src-tauri/src/lib.rs` — `drain_position_queue` mit Per-Item-Timeout + Cap, neue `spawn_position_queue_drain`-Wrapper, Streamer-Tick ruft Spawn statt direktem `.await`, `ActiveFlight.queue_drain_in_flight` AtomicBool
- Versionen → 0.5.51

---

## [v0.5.50] — 2026-05-09

🚨 **Hotfix: macOS-Crash beim Startup nach v0.5.49-Update.**

### Hintergrund

Pilot-Report direkt nach v0.5.49-Release: „Auf Mac geht die Version sofort wieder zu nach dem Update — öffnet nicht mehr." App crashed unmittelbar beim Startup mit „no reactor running" panic.

### Root-Cause

`spawn_pirep_queue_worker` (neu in v0.5.49) nutzte `tokio::spawn` direkt — diese Funktion wird aber aus dem **synchronen `.setup()`-Closure** aufgerufen, wo auf macOS noch kein tokio-Runtime-Context aktiv ist. Auf Windows toleriert Tauri das (Runtime ist da früher initialisiert), auf macOS panic'd der Aufruf sofort.

### 🆕 Fix

- `spawn_pirep_queue_worker` nutzt jetzt `tauri::async_runtime::spawn` statt `tokio::spawn` — explizit Tauris managed Runtime, funktioniert in jedem Kontext auf allen Plattformen
- Alle anderen `tokio::spawn`-Sites bleiben unverändert (sind in async fn-Kontexten, da gibt's keinen Bug)

### Sofort-Maßnahmen

- v0.5.49 zu Draft demoted, v0.5.48 wurde zwischenzeitlich wieder Latest
- Mac-Piloten die schon v0.5.49 installiert hatten und nicht mehr starten können: v0.5.48-DMG manuell drüberinstallieren, dann auf v0.5.50-Auto-Update warten
- Windows-Piloten waren NICHT betroffen — der Bug war macOS-spezifisch

---

## [v0.5.49] — 2026-05-09

🛡 **„Fehler 1236"-Fix — HTTP-Härtung + entkoppelter Streamer + PIREP-Offline-Queue.**

### Hintergrund

User-Report PTO 705 (PFE-Pilot, EDLN→EDDL): Direkt nach der Landung kam Windows-Socket-Error 1236 (`ERROR_CONNECTION_INVALID`). App erstarrte, kein Position-Update mehr, kein UI-Refresh. Pilot musste die App neu starten und den PIREP manuell einreichen. Im JSONL: Position-Stream endet exakt am Touchdown, dann 2 min 36 sec Stille bis `flight_resumed`. **Nicht der Pilot hat verworfen — die App ist gehängt.**

Root-Cause-Analyse:

- `reqwest`-Client hatte `DEFAULT_TIMEOUT=20s`, KEIN `connect_timeout`, KEIN `tcp_keepalive`. Eine vom Router gekillte TCP-Verbindung (NAT-Eviction, ISP-RST) führte zu 20 sec hängendem `await` im Streamer-Tick
- Tick-Loop blockiert → keine Snapshots, kein JSONL-Append, kein MQTT-Publish, UI eingefroren
- Pilot dachte App ist tot, force-close

### 🆕 Fünf zusammenhängige Fixes

**1. HTTP-Client-Hardening** (`api-client/src/lib.rs`)
- `tcp_keepalive(30s)` — OS schickt regelmäßig Keep-Alive-Pakete, hält NAT-Einträge im Router warm und phpVMS-Server-keep-alive aktiv
- `connect_timeout(5s)` — TCP-Handshake gibt schnell auf statt 20s zu warten
- `pool_idle_timeout(60s)` — idle Verbindungen werden vor dem nginx-`keepalive_timeout` gerecycelt
- `DEFAULT_TIMEOUT 20→10s` — wenn ein Call so lange hängt, ist die Verbindung eh tot

**2. Streamer-Tick komplett vom Position-POST entkoppelt** (`lib.rs:8999`)
- `client.post_positions().await` läuft jetzt in `tokio::spawn` mit hartem 8s `tokio::timeout`
- Tick-Loop läuft IMMER weiter — JSONL/MQTT/Sampler werden nie blockiert
- Bei Timeout/Error: Position landet im persistenten `position_queue` (existierende Logik)
- Pilot-erkennbarer Effekt: bei Verbindungs-Glitch friert die App nicht mehr ein, Live-Tracking läuft weiter, Recovery beim nächsten erfolgreichen POST

**3. PIREP-File mit Auto-Retry + persistente Queue** (`lib.rs:7030`)
- Neuer `file_pirep_with_retry()`: 3 Versuche mit 5s/30s exponentiellem Backoff bei TRANSIENTEM Fehler (Netz, Timeout, 5xx, 429, 408)
- Hard-Fehler (Validation, Auth) brechen sofort ab — Pilot muss korrigieren
- Bei 3× Fail: PIREP wandert als `<app_data_dir>/pending_pireps/<pirep_id>.json` in den persistenten Queue
- `record_landing_for_filed_flight` + `clear_persisted_flight` laufen — Pilot kann sofort den nächsten Flug starten

**3b. Background-Worker** (`lib.rs:6238`)
- Neuer `spawn_pirep_queue_worker`: tickt alle 60 Sekunden
- Scannt `pending_pireps/`, versucht jeden PIREP einzureichen
- Bei Erfolg: löschen + `consume_bid_best_effort` + `spawn_flight_log_upload` + Activity-Log „Gequeueter PIREP nachträglich eingereicht"
- Bei Failure: `attempt_count` + `last_error` werden zurückgeschrieben (Pilot kann im Verzeichnis sehen wie oft retried wurde)
- Skip nach 50 Versuchen (= circular failure, manuell nötig)

**4. Windows-Socket-Codes übersetzen** (`lib.rs:6280`)
- Neuer `friendly_net_error()`: mappt `1236` → „Verbindung wurde unterbrochen (Router-NAT-Eviction o.ä.). Wiederversuch automatisch."
- Plus 10053/10054/10060 (Connection abort/reset/timeout), DNS-Failures, Connect-Failures
- Pilot sieht im Activity-Log lesbare Texte statt kryptischer Codes

**5. Doppel-Touchdown-Window-Dump-Fix** (`lib.rs:8544`)
- Aus dem PTO-705-Log: nach `flight_resumed` wurde der TouchdownWindow-Buffer ein zweites Mal gedumpt (~80 KB Overhead)
- Root-Cause: `touchdown_window_dumped_at` wurde in `stats` gesetzt, aber `save_active_flight` lief erst beim nächsten Periodic-Tick → wenn die App dazwischen quitted, war die Disk-Kopie noch `None`
- Fix: explizites `save_active_flight(&app, &flight)` direkt nach dem Setzen, vor dem `record_event`

### Was Piloten merken

- **Kein App-Hang mehr bei Netzwerk-Glitches** — Streamer läuft kontinuierlich weiter, UI bleibt responsive
- **PIREP-Submit nie wieder verloren** — auch wenn das Netz beim End-Flight komplett weg ist, wird der PIREP automatisch eingereicht sobald die Verbindung wieder steht. Pilot kann SOFORT den nächsten Flug starten
- **Verständliche Fehler-Meldungen** — „Verbindung wurde unterbrochen, Wiederversuch automatisch" statt „Fehler 1236"
- **Saubere Touchdown-Forensik** — kein doppelter 80-KB-Buffer-Dump mehr nach Resume

### Geänderte Dateien

- `client/src-tauri/crates/api-client/src/lib.rs` — `Client::new()` mit Keep-Alive + connect_timeout + pool-Hardening; `FileBody`/`FareEntry` Deserialize hinzu
- `client/src-tauri/src/lib.rs` — Streamer-Tick spawnt POST + Timeout, neuer `pirep_queue` Modul, `file_pirep_with_retry`, `spawn_pirep_queue_worker`, `friendly_net_error`, `enqueue_position_offline`, `is_transient_pirep_error`, immediate `save_active_flight` nach TD-Window-Dump
- Versionen → 0.5.49

---

## [v0.5.48] — 2026-05-09

🔔 **Update-Banner mit Eskalations-Stufen + 4 h Re-Check während die App läuft.**

### Hintergrund

User-Report: Pilot hängt seit Tagen auf v0.5.22 und bekommt keinen Update-Hinweis. Root-Cause-Analyse: der Tauri-Updater hat einen Check beim App-Start gemacht, das war's. Pilot der die App 8 h fürs Cruise offen hatte, sah nichts. Plus der Header-Button war zu dezent — leicht zu übersehen wenn man ihn beim Start nicht sofort registriert hat.

### 🆕 Neuer `useUpdateChecker`-Hook + Eskalations-Logik

**Polling-Strategie:**
- **Beim App-Start** wie bisher (1× sofort)
- **Während App läuft** alle **4 Stunden** ein leiser Re-Check (lange Cruise-Sessions)
- **Bei Window-Focus** Re-Check wenn letzter Check > 30 min her (Pilot wechselt vom Sim zurück zur App)
- **Nie öfter** — GitHub-Rate-Limit + Sim-FPS schonen

**Eskalations-Stufen am UI:**

| Update-Alter | Anzeige |
|---|---|
| `fresh` (< 24 h) | Header-Button wie bisher (dezent) |
| `pulse` (≥ 24 h ignoriert) | Button bekommt sanfte Pulse-Animation |
| `banner` (≥ 72 h ignoriert) | Großes Banner oben in der App + Button glüht cyan |

**Neuer `UpdateBanner`-Component:** voll-breit oben in der App, **drei Bedingungen** müssen ALLE für die Anzeige stimmen:
1. Stage = `banner` (3+ Tage alt)
2. Pilot ist NICHT in einer aktiv-fliegenden Phase (Pushback / Taxi / Cruise / Approach / Landing / Taxi-In / Blocks-On werden alle ausgeschlossen — niemals einen Pilot mid-flight stören)
3. Pilot hat das Banner nicht mit „Später" weggeklickt (4 h Snooze, danach kommt es wieder)

**localStorage-State:**
- `aeroacars.update.first_seen.{version}` — wann das Update zuerst erkannt wurde (für Stage-Berechnung)
- `aeroacars.update.dismissed_until` — Snooze-Ablauf-Timestamp
- `aeroacars.update.last_check_at` — letzter erfolgreicher Check (für Focus-Re-Check-Throttle)

Alte first-seen-Einträge anderer Versionen werden automatisch aufgeräumt damit localStorage nicht voll läuft.

### Was Piloten merken

- **Lange Sessions:** Update das während des Cruise erscheint, wird ohne App-Restart erkannt — beim nächsten Tab-Switch zur App-Fenster gleich angezeigt
- **Ignorierte Updates:** Button glüht nach 24 h sanft, nach 72 h zusätzlich großes Banner — schwer zu übersehen aber nicht penetrant
- **Mid-Flight-Schutz:** Banner wird NIE während Pushback/Taxi/Cruise/Approach/Landing eingeblendet. Nur Header-Button bleibt — Pilot bestimmt selbst wann er installiert
- **Snooze:** „Später" am Banner versteckt es für 4 h. Pilot wird danach noch einmal erinnert. Header-Button bleibt sichtbar
- **DE/EN/IT** vollständig

### Geänderte Dateien

- `client/src/hooks/useUpdateChecker.ts` — neu, zentrale Quelle für Update-State
- `client/src/components/UpdateButton.tsx` — konsumiert jetzt den Hook + Stage-Aware-CSS-Klassen
- `client/src/components/UpdateBanner.tsx` — neu, große Eskalation
- `client/src/App.tsx` — Hook gemountet, Banner gerendert mit Phase-Awareness
- `client/src/App.css` — `.update-button--pulse`, `.update-button--escalated`, `.update-banner*`
- `client/src/locales/{de,en,it}/common.json` — neuer `update`-Namespace
- Versionen: `package.json`, `tauri.conf.json`, `Cargo.toml` → 0.5.48

---

## [v0.5.47] — 2026-05-09

🎯 **Web/Client-Parität — eine Wahrheit für Sub-Scores, Labels und Einheiten.**

### Hintergrund

User-Feedback: „die beiden Berechnungen im Web und in AeroACARS müssen gleich sein". Audit hat starke Drift aufgedeckt — Pilot-App (`LandingPanel.tsx`) und Live-Monitor (`LandingAnalysis.tsx`) hatten zwei separate Sub-Score-Tabellen mit unterschiedlichen Schwellen, Bands, Coach-Tipps und Rollout-Metriken. Derselbe Flug bekam je nach Plattform andere Teilnoten.

### 🆕 Vier zusammengehörige Fixes

**1. Score-Modul `client/src/lib/landingScoring.ts` portiert (1:1 vom Webapp):**
- `computeSubScores()`, `aggregateSubScores()`, `classifyLanding()`, `band()`, `RATIONALE_LABELS`, `TIP_LABELS`, `SUB_LABELS` — alles aus einer Datei
- `LandingPanel.tsx` löscht 7 lokale `score*`-Funktionen + lokales `band()` und delegiert an die Lib
- Schwellwerte für V/S, G, Bounces, Stability, Rollout (jetzt absolute Meter wie Webapp), Fuel sind identisch
- Coach-Tip-Logik nutzt den schwächsten Sub-Score wie im Webapp
- Datei ist Quelle der Wahrheit — Änderungen MÜSSEN in beiden Repos parallel passieren

**2. Label-Drift eliminiert (Webapp):**
- `LandingAnalysis.tsx`: Touchdown-Tile „V/S" → „Sinkrate", „Peak G" → „Peak-G"
- 50-Hz-Forensik-Card: „V/S am Edge", „V/S 250/500/1000/1500 ms-Mean", „Peak-G post-TD …" — alle V/S-Labels auf „Sinkrate" + Bindestrich-Konsistenz mit Pilot-App
- Flare-Card: „V/S-Reduktion" → „Sinkraten-Reduktion", „dV/S/dt" → „dSinkrate/dt", „V/S End-of-Flare" → „Sinkrate End-of-Flare"
- G-Tone-Schwellen folgen jetzt den `landingScoring.ts`-Bands (1.40 firm, 1.70 hard, 2.10 severe)

**3. Einheiten-Konsistenz kg statt t (Webapp):**
- LDW + Fuel @ Landing: vorher in `t` mit `/1000`-Trick, jetzt in `kg` mit Tausender-Trennzeichen — gleich zur Client-`ComparisonTable` im Reports-Tab

**4. Fehlende 50-Hz-Felder im Client + Typo-Fix:**
- Client zeigt jetzt zusätzlich `vs_smoothed_250ms_fpm`, `vs_smoothed_1500ms_fpm`, `peak_g_post_1000ms` (waren in `LandingRecord` vorhanden, aber nie gerendert)
- DE-i18n-Typo `Flare-Qualitaet` → `Flare-Qualität`, `verfuegbar` → `verfügbar`, `fuer` → `für`
- Alle Forensik-Labels in DE/EN/IT von „V/S" auf „Sinkrate" / „Sink rate" / „Rateo discesa" angeglichen

**5. Quick-Flag-Chips auch im Client:**
- Neuer `QuickFlags`-Component direkt unter dem Headline-Block: HARTE LANDUNG (≥600 fpm oder ≥1.7 G), SCHWERE LANDUNG (≥1000 fpm oder ≥2.1 G), BOUNCE × n, ABSEITS DER MITTELLINIE (>5 m), UNSTABILER ANFLUG (σ V/S > 400 fpm)
- Spiegelt die Chip-Row aus dem Webapp-Header — Pilot sieht in beiden Plattformen dieselben Auffälligkeiten als erstes
- DE/EN/IT i18n vollständig + neue CSS-Klassen

### Was Piloten merken

- **Sub-Score-Breakdown** im Client und Web zeigen jetzt für denselben Flug exakt dieselben Punkte — keine "Welcher Wert stimmt jetzt?"-Diskussionen mehr
- **Labels** sind durchgängig „Sinkrate" (DE) / „Sink rate" (EN) / „Rateo discesa" (IT) statt mal „V/S" mal „Sinkrate"
- **Einheiten** für LDW + Fuel-at-Landing sind in beiden Plattformen kg
- **Auffälligkeiten** als Chip-Row direkt unter dem Headline auch in der Pilot-App
- Touchdown-Tile-Färbung (Webapp) folgt jetzt den offiziellen Score-Bands

### Geänderte Dateien

- `client/src/lib/landingScoring.ts` — neu, Source-of-Truth für beide Plattformen
- `client/src/components/LandingPanel.tsx` — `computeSubScores` delegiert an Lib, neue `QuickFlags`-Component, fehlende Forensik-Felder gerendert
- `client/src/locales/{de,en,it}/common.json` — Typo-Fix, V/S → Sinkrate, neue 250/1500/1000ms-Keys, neue `landing.flag.*`-Keys
- `client/src/App.css` — `.landing-flags`, `.landing-flag--warn`, `.landing-flag--err`
- `aeroacars-live/webapp/src/components/LandingAnalysis.tsx` — Label-Drift, kg-Einheit, G-Schwellen
- Versionen: `package.json`, `tauri.conf.json`, `Cargo.toml` → 0.5.47

---

## [v0.5.46] — 2026-05-09

🎯 **Adrian-Feedback umgesetzt — Approach-Stability-Filter + OFP-Refresh im Loadsheet-Card.**

### Hintergrund

Adrian (GSG-Pilot) hat zwei konkrete Pain-Points gemeldet:

1. **Approach-Stability-Wert „V/S-Streuung 320 fpm"** — wird durch das Flare-Manöver in den letzten 3 Sekunden kaputtgemessen, weil dort die Sinkrate absichtlich aktiv reduziert wird. Plus alte Samples >1.500 ft AGL (Localizer-Intercept-Höhe) verfälschen die Statistik.
2. **PasStudio-Loadsheet wird nicht erkannt** — wenn der Pilot in PasStudio neu plant und sich der Block-Fuel ändert, hält AeroACARS noch den alten OFP. Der Refresh-Button existiert zwar im ActiveFlight-Header, war aber nicht prominent genug.

### 🆕 Zwei zusammengehörige Fixes

**1. Approach-Stability-Filter (lib.rs `compute_approach_stddev` + `compute_approach_stability_v2`):**

- AGL-Window: nur Samples > 0 ft und ≤ **1.500 ft AGL** (war zuvor unbegrenzt — alte Cruise-Samples wurden mitgezählt)
- Flare-Cutoff: alle Samples in den **letzten 3 Sekunden vor Touchdown** werden ausgeschlossen
- Konstanten neu: `APPROACH_STABILITY_AGL_CAP_FT = 1500.0`, `APPROACH_FLARE_CUTOFF_MS = 3000`
- Greift in beiden Metriken: V/S-Stddev, Bank-Stddev, Stability-V2-Gate-Bewertung

Effekt: Adrian's „320 fpm V/S-Streuung" wird realistischer (~80-150 fpm wie Volanta) — der Wert reflektiert jetzt die echte Anflug-Stabilität, nicht das Flare-Manöver.

**2. OFP-Refresh-Button im Loadsheet-Card (LoadsheetMonitor.tsx):**

- Heuristik „OFP veraltet": Block-Fuel-Delta ≥ 400 kg (oder ≥ 5 % vom Plan) UND ZFW-Delta < 200 kg → klassisches PasStudio-Update-Muster
- Bei Treffer wird der normale Hint durch `📋 Block-Abweichung sieht nach OFP-Update in PasStudio/SimBrief aus — OFP neu laden?` übersteuert
- Inline-Button **„OFP neu laden"** ruft das bestehende `flight_refresh_simbrief`-Command auf — zieht den frischesten Bid + OFP von SimBrief und überschreibt alle `planned_*`-Felder im aktiven Flug
- Status-Feedback inline: Lade-Spinner, ✓-Bestätigung (Auto-clear nach 4 s), Fehler-Tooltip
- DE/EN/IT i18n vollständig

### Was Piloten merken

- **Approach-Stability-Werte** beim Touchdown sind jetzt deutlich realistischer (Volanta-vergleichbar)
- **Loadsheet-Card** während Boarding zeigt einen klaren Refresh-Button wenn der Plan veraltet aussieht — keine Diskussion mehr ob „PasStudio-Werte ankommen"

### Geänderte Dateien

- `client/src-tauri/src/lib.rs` — `compute_approach_stddev`, `compute_approach_stability_v2`, neue Konstanten + Call-Site
- `client/src/components/LoadsheetMonitor.tsx` — OFP-Outdated-Heuristik + Inline-Refresh-Button
- `client/src/locales/{de,en,it}/common.json` — 5 neue Keys unter `cockpit.loadsheet`
- `client/src/App.css` — `.loadsheet__refresh-btn`, `.loadsheet__refresh-done`, `.loadsheet__refresh-err`
- Versionen: `package.json`, `tauri.conf.json`, `Cargo.toml` → 0.5.46

---

## [v0.5.45] — 2026-05-09

🔧 **Sampler-Hardening: dichte Approach-Cadence + Phantom-TD-Fix + Resume-Schutz.**

### Hintergrund

User-Reports DLH 1731, CFG 9746 LDZA→EDDM (MSFS Fenix) sowie GSG 302 X-Plane DA40 Bush-Strip — drei Probleme im Anflug-/Touchdown-Bereich:

1. **Sample-Cadence im Final-Approach 3.5 sec** statt der geplanten 1-2 sec
2. **Phantom-Touchdown beim Taxi auf unebenem Bush-Strip** (gear_normal_force_n schwankte)
3. **Doppel-TD nach App-Resume** weil Sampler-Guard zurückgesetzt wurde

### 🆕 Vier zusammengehörige Fixes

**1. `adaptive_tick_interval` enger gestaffelt (Option B aus User-Vorschlag):**

| AGL | vorher | jetzt |
|---|---|---|
| < 100 ft | 500 ms | 500 ms |
| < 500 ft | 1000 ms | **750 ms** |
| < 1000 ft | 2000 ms | **1000 ms** |
| < 1500 ft | (default 3000 ms) | **1000 ms** |
| < 2000 ft | (default 3000 ms) | **1500 ms** |

**2. Critical-Window AGL-Trigger 300 → 1500 ft (Option A):** phpVMS-POST pausiert ab Final-Approach. JSONL/MQTT-Cadence wird nicht mehr durch HTTP-Latency gestretcht.

**3. Phase-Guard gegen Phantom-Touchdowns:** TD-Edge wird nur akzeptiert wenn `FlightStats.phase` ∈ {Approach, Final, Landing}. Schließt Bush-Strip-Bumps in TaxiOut/TakeoffRoll als False-Positive aus. Greift in beiden Edge-Detection-Pfaden (RREF on_ground + X-Plane-Premium-Plugin-Touchdown-Event).

**4. Resume-Hardening:** `PersistedFlightStats` bekommt 4 neue Felder die jetzt mit-persistiert werden:

- `sampler_touchdown_at`
- `sampler_takeoff_at`
- `touchdown_window_dumped_at`
- `landing_score_finalized`

Verhindert Re-Capture nach App-Resume wenn der TD vor dem Quit/Restart bereits gefeuert hat. War das Root-Cause beim X-Plane-Bush-Strip-Doppel-TD: Phantom-Edge → flight_resumed → Guards waren None → echter Landing-Edge wurde als zweites Capture aufgezeichnet.

### Was Piloten merken

- **Approach-Stabilitäts-Analyse beim Touchdown** sieht jetzt jeden V/S-Spike (4-5x dichtere Sample-Cadence im Final-Approach)
- **GA-Flieger auf unebenen Bush-Strips** (DA40, Cessna mit High-Float-Gear) bekommen keine Phantom-TDs mehr während Taxi
- **App-Restart mid-flight** (Sim-Crash, geplanter Reboot) verliert keine Sampler-State mehr
- phpVMS sieht Position-Punkte im Final-Approach ein paar Sekunden verzögert (akzeptabel — Live-Map via MQTT bleibt live)

---

## [v0.5.44] — 2026-05-09

🛩 **Aircraft-Type-Fallback aus Sim-Snapshot — auch ohne SimBrief OFP gesetzt.**

### Hintergrund

User-Report: bei DLH 1731 (Lufthansa A320, D-AIUM) wurde im Live-Monitor nur die Registration „D-AIUM" angezeigt, der Aircraft-Type („A320") fehlte. Pattern bei mehreren Flügen ohne SimBrief OFP.

### Root Cause

`flight.aircraft_icao` wird in `lib.rs:4835` gesetzt aus:
```rust
let aircraft_icao = aircraft_details
    .as_ref()
    .and_then(|a| a.icao.clone())
    .unwrap_or_default()  // ← "" wenn aircraft_details None
```

`aircraft_details` kommt aus `phpVMS.get_aircraft(simbrief.aircraft_id)`. Wenn der Pilot **kein SimBrief OFP** generiert hat (oder das OFP keinen `aircraft_id` enthielt), bleibt `aircraft_icao` leer. Der MQTT Position-Payload sendet dann `aircraft_icao: ""`.

### Fix

**Client (v0.5.44):** im Streamer-Tick wenn `flight.aircraft_icao` leer ist, fallback auf `snap.aircraft_icao` mit Regex-Extraktion. MSFS liefert oft kuriose Strings wie `"ATCCOM.AC_MODEL A321.0.text"` — der neue `extract_icao_code()` Helper extrahiert daraus `"A321"` per Regex `\b([A-Z]\d{2,3}|[A-Z]{2,4}\d{0,3})\b`.

**Recorder (separater Fix, schon deployed):** `upsertFlightPosition` behandelt empty-Strings als NULL. Greift für **alle** pre-v0.5.44 Pilot-Clients sofort — das vorhandene Spalten-Wert wird nicht mehr durch leere Payloads überschrieben.

### Was Piloten merken

- **VAs ohne SimBrief-Setup** sehen jetzt den richtigen Aircraft-Type im Live-Monitor + auf der Karte (vorher nur Registration)
- **Marker-Icon** auf der Live-Map zeigt das korrekte Flugzeug-SVG (vorher Default)
- **PIREP Custom-Field „Aircraft Type"** wird gefüllt auch ohne SimBrief

---

## [v0.5.43] — 2026-05-09

🎯 **50-Hz-Forensik in der LandingPanel — Pilot sieht alles direkt in der App.**

### Hintergrund

Bisher waren die v0.5.39+ TouchdownWindow-Forensik-Felder (`vs_at_edge`, Multi-Window-VS, Peak-G post-TD, Flare-Quality-Score) nur in der aeroacars-live Webapp sichtbar. Pilot musste nach dem Flug ins Webportal wechseln um den Volanta-/DLHv-Vergleich zu sehen.

### 🆕 Was neu ist

**Touchdown-Section** in der Cockpit-LandingPanel zeigt jetzt direkt:
- `V/S am Edge` (interpoliert zwischen 30-ms-Samples = Volanta-equivalent)
- `500-ms-Mean (Volanta)` und `1-s-Mean (DLHv)` als zusätzliche Zeilen
- `Peak-G nach TD` separat vom `Peak-G` (= echter Gear-Compression-Spike, oft 100-300 ms nach Bodenkontakt)

Alle vier zusätzlichen KV-Zeilen erscheinen nahtlos in der bestehenden 2-Spalten-Grid neben den klassischen Touchdown-Werten — keine Stein-daneben-Optik.

**Flare-Quality** als eigene Section nach Approach-Stability:
- Großer Score 0..100 (links, farbig je band)
- KV-Liste rechts (rechts): Pre-Flare-VS, End-of-Flare-VS, Reduktion, dV/S/dt
- Status-Chip im Header: ✈ FLARE / KEIN FLARE
- Gleicher visueller Stil wie StabilityIndicator damit's harmonisch in den Tab integriert

**i18n** komplett — DE/EN/IT (23 Keys × 3 Sprachen).

### Backend

`LandingRecord`-Struct in `crates/storage` um 14 optionale Forensik-Felder erweitert (alle `#[serde(default)]` für Backwards-Compat mit alten landing_history.json-Einträgen). `build_landing_record` liest aus `stats.landing_analysis` über die `ana_f32/i32/u32/bool`-Helper.

### Was wenn die Felder None sind?

Pre-v0.5.39 Landungen aus dem History-Store oder Sample-Loch-Fälle: die zusätzlichen KV-Zeilen erscheinen einfach nicht (conditional render). Die Flare-Section erscheint gar nicht. Keine UI-Brüche.

---

## [v0.5.42] — 2026-05-09

🧹 **Smoothed VS filtert positive Werte raus — reine Sinkrate als Maß.**

### Hintergrund

Direkt nach v0.5.41 Feedback: in `compute_landing_analysis()` und im aeroacars-live FSM-Replay-Importer wurden ALLE airborne-Samples im Smoothing-Window gemittelt — auch solche mit positiver V/S (= Float-Effekt, Ground-Effect-Bumps, Ballooning kurz vor TD). Diese verfälschen den Mittelwert Richtung 0 und täuschen einen sanfteren Touchdown vor als physikalisch passiert ist.

Volanta und DLHv filtern ähnlich — die zeigen die „reine Sinkrate" beim Touchdown, nicht den durchgemischten Mittelwert mit Float-Bumps.

### Fix

`mean_vs_window()` nimmt jetzt nur noch Samples mit `vs_fpm < 0` (= echte Sinkrate). Greift in:
- `vs_smoothed_250ms_fpm` / 500ms / 1000ms / 1500ms im 50-Hz-Buffer-Analyzer
- gleiches im aeroacars-live `importer.ts` für FSM-Replay von pre-v0.5.40 historische Logs

`vs_at_edge_fpm` (linear interpoliert auf den exakten on_ground-Edge) bleibt unangetastet — das ist ein direkter Mess-Wert, kein Mittel.

### Was sich für Piloten ändert

Bei Landungen mit Float / Ballooning kurz vor TD wird der `vs_smoothed_500ms_fpm`-Wert jetzt etwas pessimistischer (= ehrlicher). Bei sauberen Approaches ohne Float-Bumps unverändert.

---

## [v0.5.41] — 2026-05-09

🎯 **Touchdown-Score nutzt jetzt 50-Hz `vs_at_edge` (= Volanta-equivalent), nicht mehr MSFS-SimVar.**

### Hintergrund

Vergleichs-Test mit DLH 1404 EDDF→LDZA (Fenix A320 SL, MSFS 2024):

| Tool | VS |
|---|---|
| Volanta | 66 fpm |
| DLHv-Tool | 62 fpm |
| AeroACARS v0.5.40 (msfs_simvar_latched) | **-101 fpm** ❌ |
| AeroACARS v0.5.41 `vs_at_edge` | **-66 fpm** ✅ exakt Volanta |

Der MSFS-SimVar `TOUCHDOWN_VELOCITY` liefert beim Fenix A320 SL deutlich pessimistischere Werte als die echte (smoothed) Sinkrate beim Bodenkontakt. Volanta und DLHv messen smoothed über 250–500 ms — exakt was unser v0.5.39-Patch im 50-Hz-Buffer berechnet (`vs_at_edge_fpm` = linear interpoliert auf den exakten on_ground-Edge zwischen zwei 30-ms-Samples).

### Fix: Score-Recompute aus dem Buffer

Nach dem 10-s-Sampler-Dump wird der Score mit den high-res-Werten neu berechnet:
- `landing_peak_vs_fpm` ← `vs_at_edge_fpm` aus dem 50-Hz-Buffer
- `landing_peak_g_force` ← `peak_g_post_500ms` (echter Gear-Compression-Spike, oft 50–100 ms NACH TD-Edge — der bisherige Wert traf den Free-Float-Frame VOR dem Spike)
- `LandingScore::classify()` neu mit den Werten

### Touchdown-MQTT-Event jetzt verzögert (10 s post-TD)

`announce_landing_score` blockiert die Touchdown-Emission bis der Sampler fertig ist (`landing_score_finalized=true`). Vorher hätte der Live-Monitor den überholten msfs_simvar_latched-Wert gesehen, dann 10 s später müsste man das nochmal korrigieren — was Duplikate erzeugt. Jetzt: ein Touchdown-Event, mit den finalen Werten.

**Fallback-Timeout: 12 s** — wenn der Sampler-Dump aus irgendeinem Grund nicht durchgeht (Sample-Loch, Sampler-Crash), wird der Touchdown trotzdem mit den vorhandenen Werten emittiert. Verhindert dass Touchdowns bei Buffer-Path-Fehlern nie gemeldet werden.

### Flare-Score-Skala neu balanciert

Vorherige Skala bestrafte Piloten die mit bereits niedriger VS reinkamen (= eigentlich gute Approaches) zu hart. „Reduktion >0 fpm" gab pauschal nur 20 Punkte.

**Neu:** Endpoint-Score dominiert (= was kommt am TD raus, der eigentliche Touchdown-Indikator), Reduktion gibt Bonus-Punkte (= Flare hat eine harte Landung gerettet wenn aus hohem VS reduziert wurde):

| `vs_at_flare_end` | Endpoint |
|---|---|
| > -75 fpm | 100 (butter) |
| > -150 fpm | 80 (smooth) |
| > -300 fpm | 60 (acceptable) |
| > -500 fpm | 40 (firm) |
| sonst | 20 |

| `flare_reduction` | Bonus |
|---|---|
| > 400 fpm | +20 |
| > 200 fpm | +15 |
| > 100 fpm | +10 |
| > 50 fpm | +5 |
| sonst | 0 |

Endpoint + Bonus, gecappt [0..100].

**Beispiele:**
- DLH 1404 (Peter, vs_end=-61, red=59): 100 + 5 = **100** ✓ (vorher 20)
- B738 hypothetisch (vs_end=-100, red=600): 80 + 20 = **100** ✓
- URO 913 (vs_end=-606 estimated, red=315): 20 + 15 = **35**
- Bad Pilot (vs_end=-800, red=0): 20 + 0 = **20**

---

## [v0.5.40] — 2026-05-09

🐞 **Fix: Phase-FSM hing 9 h in Pushback** bei Aerosoft A340-600 Pro (URO 913 ZWWW→EHBK).

### Hintergrund

Pilot meldete: nur Boarding→Pushback und Pushback→Arrived in der Phase-Historie. Die kompletten 9 h dazwischen (TaxiOut, TakeoffRoll, Takeoff, Climb, Cruise, Descent, Approach, Final, Landing, TaxiIn) wurden übersprungen — obwohl der Flug echt war (max IAS 331 kt, max ALT 36340 ft, 7173 Position-Snaps, 7069 davon airborne).

### Zwei Bugs

**Bug 1 — `pushback_state == 3` falsch interpretiert:**
MSFS PUSHBACK STATE = 3 ist der **Default-Wert** ("kein Pushback aktiv"), nicht "Pushback gerade abgeschlossen". Werte 0/1/2 = Push aktiv (forward/back/slow), 3 = idle. Der Pilot pushed mit GSX (oder manuell), wodurch der MSFS-State NIE auf 0/1/2 wechselte — nur 3 die ganze Zeit. Die FSM las das als „Tug ist gerade fertig" und wartete auf 10 s Stillstand vor TaxiOut. Pilot rollte aber schon mit 14 kt, also kam nie ein Stillstand → Phase blieb hängen.

**Bug 2 — Aerosoft A340-600 Pro flickert `engines_running`:**
Der Aerosoft-A346 zappelt die `GENERAL ENG COMBUSTION` SimVar zwischen 0 und 4 — 27 Wechsel in 7 min Pushback-Phase observed. Die FSM-Bedingung `snap.engines_running > 0` lieferte zufällig true/false. Selbst wenn die Stillstand-Logik nicht blockiert hätte, wäre die Engines-Bedingung nicht stabil getriggert.

### Fix

- **`saw_pushback_state_active`** Track-State: nur wenn `pushback_state` jemals 0/1/2 war seit Flight-Start, gilt der spätere 3-Wert als „Tug detached". Sonst Fall-back auf alte Heuristik (engines + gs > 3 kt = TaxiOut)
- **`engines_effectively_running()`** Helper: Anti-Flicker mit 2-s-Grace-Window. Wenn `engines_running > 0` zuletzt < 2 s zurück, gilt als laufend. Verwendet in Pushback→TaxiOut + TaxiOut→TakeoffRoll
- Existierende 5-s-Debounce für Activity-Log bleibt unangetastet (nur FSM-Pfad gefixt)

### Was Piloten merken

- Aerosoft A340-600 Pro + andere Aircraft mit GSX-Pushback / flickerigem `engines_running`-SimVar tracken jetzt alle Phasen sauber
- Default-MSFS-Pushback (Tug-Animation) funktioniert weiter wie vorher (saw_pushback_state_active wird true → alte Logik greift)

---

## [v0.5.39] — 2026-05-09

🎯 **50-Hz-Touchdown-Forensik + Flare-Quality + Critical-Window-Priority.**

### Hintergrund

User-Vergleich vom DLH-1331-Flug (GMMN→EDDF, Fenix A321): AeroACARS meldete -205 fpm / 0.99G, Volanta -87 fpm / 1.14G, DLHv-Tool -96 fpm / 1.18G. Root-Cause-Analyse zeigte: ein 1.86-s-Loch im JSONL-Position-Stream genau im Touchdown-Moment, weil der Streamer-Tick im selben Loop phpVMS-POSTs ausführt (200-1500 ms HTTP-Latenz) und die adaptive 500-ms-Cadence stretcht. AeroACARS griff daher auf MSFS's instantaneous `TOUCHDOWN_VELOCITY` SimVar zurück, während Volanta/DLHv smoothed VS-Mittel über ~500-1000 ms verwenden — physikalisch repräsentativer für das was der Pilot fühlt.

### 🆕 50-Hz-TouchdownWindow-Buffer-Dump

`spawn_touchdown_sampler` läuft schon bei 50 Hz im RAM, puffert die letzten 5 s. Beim TD-Edge:

1. Pre-TD-Buffer wird in einen separaten Post-Buffer kopiert (= vor Eviction geschützt)
2. Sampler sammelt für TOUCHDOWN_POST_WINDOW_MS (10 s) weiter Post-TD-Samples
3. Nach 10 s flusht der Sampler den gesamten Buffer (~750 Samples ≈ 40 KB) als ein einzelnes `TouchdownWindow`-Event in die JSONL — Lock wird vor dem File-IO released damit der Streamer-Tick nicht wartet

Damit ist die Datenlücke geschlossen: 50 Hz-Auflösung über das gesamte ±10-s-Fenster um den TD.

### 🎯 Landing-Critical-Window pausiert blockierende Network-IO

Streamer-Tick checkt jetzt `landing_critical_until`:

- Proaktiv gesetzt bei AGL <300 ft + Approach/Final/Landing-Phase (Window auf now+60 s, refreshed jeden Tick)
- Sampler refresht beim TD-Edge auf TD+10 s

Während dem Window:
- phpVMS-POST übersprungen, Position direkt in die Offline-Queue
- Queue-Drain übersprungen (mehrere POSTs auf einmal würden Tick blockieren)
- MQTT-Publish (try_send, non-blocking) + JSONL-Append (lokales File-IO, ~ms) laufen normal weiter

Beim ersten Tick außerhalb des Windows wird die Queue normal gedrained → phpVMS bekommt die Punkte mit ein paar Sekunden Verzögerung, dafür ist der Live-Track + Forensik-Log lückenlos.

### 📊 Forensik-Analyzer auf dem Buffer

Neue `compute_landing_analysis(samples, edge_at)` Funktion liefert:

- **Multi-Window VS-Mittel** über 250/500/1000/1500 ms vor TD — 500 ms ≈ Volanta-Style, 1000 ms ≈ DLHv-Style
- **VS am Edge** linear interpoliert auf den exakten on_ground-Edge zwischen zwei 20-ms-Samples
- **Peak G post-TD** über 500 ms + 1000 ms = der echte Gear-Compression-Spike (löst das alte Problem dass `snap.g_force` im TD-Frame oft <1G liefert)
- **Flare-Qualität** im 1900-ms-Window vor TD:
  - `peak_vs_pre_flare_fpm`: steepste Sinkrate
  - `vs_at_flare_end_fpm`: VS unmittelbar vor TD
  - `flare_reduction_fpm`: Reduktion durch Flare (positiv = sanfter geworden)
  - `flare_dvs_dt_fpm_per_sec`: Steigungs-Rate
  - `flare_quality_score` 0..100: 100 = >400 fpm Reduktion + sanfter Endwert, 0 = keine Reduktion (Pilot zog zu spät oder gar nicht)
  - `flare_detected`: bool, true wenn Reduktion >50 fpm
- **Bounce-Profil**: Anzahl + Peak-AGL pro Excursion (>5 ft Mikro-Hopper-Filter)

Wird als zweites Event `LandingAnalysis` direkt nach dem `TouchdownWindow` in die JSONL geschrieben.

### 🔌 Live-Pfad: TouchdownPayload um 14 Forensik-Felder erweitert

`aeroacars-mqtt::TouchdownPayload` bekommt alle Analyzer-Felder als Optional mit `skip_serializing_if = "Option::is_none"` damit alte Pilot-Clients (v0.5.38-) beim Live-Monitor weiter funktionieren. Werte werden vom Streamer-Tick aus `FlightStats.landing_analysis` (vom Sampler gesetzt) gelesen via `ana_f32/i32/u32/bool`-Helpers.

Race-Case: Sampler braucht 10 s post-TD bis er fertig ist; wenn der Streamer-Tick vorher bereits TouchdownComplete sendet, sind die Felder None. Der nächste Refinement-Tick im Streamer-Loop bekommt die fertigen Daten, und der JSONL-Re-Importer im aeroacars-live Recorder backfillt fehlende Felder beim späteren Log-Upload (Match per `edge_at` ±15 s).

### 📺 Live-Monitor zeigt die neue Forensik

aeroacars-live Webapp `LandingAnalysis.tsx` bekommt eine neue cyan-Card **🎯 50-Hz-TouchdownWindow** die nur erscheint wenn der Pilot-Client v0.5.39+ liefert (`forensic_sample_count != null` als Feature-Detect):

- Tabelle mit allen 5 V/S-Werten, jeweils gelabelt welcher dem Volanta-/DLHv-Display entspricht
- Peak G post-TD 500 ms + 1000 ms separat
- Eigener Flare-Block mit Score, Reduktion, dV/S/dt + FLARE/KEIN-FLARE Status-Flag
- Bounce-Max-Höhe wenn Bounces

### Was nicht geht

Pre-v0.5.39-Logs bekommen die Forensik nicht — der Sampler emittierte `TouchdownWindow`/`LandingAnalysis` damals nicht. Für historische Landungen bleibt die alte Algorithmen-Forensik-Card (mit `vs_estimate_msfs`/`vs_estimate_xp`) bestehen.

### Files

- `client/src-tauri/src/lib.rs`: +492 Zeilen (Sampler-Erweiterung, Analyzer, Helpers, Streamer-Tick-Pause-Logik)
- `client/src-tauri/crates/recorder/src/lib.rs`: +60 Zeilen (TouchdownWindow + LandingAnalysis Event-Varianten + TouchdownWindowSample Struct)
- `client/src-tauri/crates/aeroacars-mqtt/src/lib.rs`: +57 Zeilen (TouchdownPayload-Erweiterung)
- aeroacars-live: webapp `LandingAnalysis.tsx` neue Card + recorder `importer.ts` landing_analysis-Backfill

---

## [v0.5.38] — 2026-05-09

🟡🟠🔴 **Visual Stable-Approach-Advisory Banner im Cockpit-Tab.**

### Hintergrund

User-Report aus dem GSG-301 GA-Flug: Pilot hatte instabilen Anflug (Bank ±7° unter 200 ft AGL, V/S -625 fpm bei 330 ft AGL), hätte durchstarten sollen, hat aber durchgezogen → -900 fpm Hard Landing. AeroACARS hat das **nicht** in real-time geflagged — Pilot bekam keine Warnung dass die Approach-Kriterien verletzt wurden.

### 🆕 Visual Banner

Neue `<StableApproachBanner>` Komponente im Cockpit-Tab. Zeigt während Approach/Final/Landing eine farbige Warnung wenn FAA-Stable-Approach-Kriterien (AC 120-71B) verletzt sind:

| Phase | Schwelle | Severity |
|---|---|---|
| 1000 ft AAL | Bank > 5° **oder** V/S außerhalb [-1100,-300] **oder** Konfig nicht gesetzt | 🟡 Warn |
| 500 ft AAL | Bank > 5° **oder** V/S < -1000 | 🟠 Alert |
| 200 ft AAL | Bank > 5° **oder** V/S < -800 | 🔴 Crit (mit Pulse-Animation) |
| Sub-100 ft V/S<-700 | Hard Landing imminent | 🔴 Crit |
| Post-TD V/S<-600 | Hard Landing detected | 🔴 8s sichtbar |

Banner blendet sich automatisch ein/aus wenn Kriterium wechselt. Kein Sound (User-Wunsch — Voice-Advisory wurde verworfen).

### ⚙ Settings-Toggle

`Settings → PIREP-Filing → Anflug-Warnungen anzeigen` (Default: **ON**). Kann pro Pilot deaktiviert werden falls die Banner stören.

### 🌍 i18n

Banner-Texte voll lokalisiert in DE/EN/IT.

Versions-Bump 0.5.37 → 0.5.38.

---

## [v0.5.37] — 2026-05-08

🇮🇹 **Italienische Übersetzung + Fix für Sprach-Reset nach Update.**

### 🇮🇹 Italiano (für Marco)

- Volle Übersetzung des UI in Italienisch (`locales/it/common.json`, ~250 Keys)
- Aviation-Begriffe korrekt: crociera, discesa, decollo, atterraggio, riattaccata, etc.
- Standard-ICAO-Abkürzungen behalten (IAS, GS, AGL, MSL, V/S, kt, ft, fpm)
- `LANGUAGE_LABELS`-Map exportiert für saubere Anzeige im Switcher

### 🐞 Sprach-Reset-Bug

User-Report: nach jedem App-Update fiel die Sprache auf Englisch zurück, obwohl Browser-Locale Deutsch war.

**Root-Cause:** `i18next-browser-languagedetector` mit `caches: ["localStorage"]` schreibt nur dann nach localStorage wenn `i18n.changeLanguage()` explizit gerufen wird. Bei reiner Auto-Detection (Browser-Locale) bleibt der localStorage-Key leer → nach Update fängt die Detection wieder bei Null an, und WebView2 könnte die Locale anders berichten.

**Fix:**
- Beim Ersten-Run nach Auto-Detection: erkannte Sprache EXPLIZIT in `localStorage["aeroacars.lang"]` schreiben
- Neue helper-Funktion `setLanguage(lang)` die `i18n.changeLanguage()` + `localStorage.setItem()` koppelt
- SettingsPanel nutzt `setLanguage()` statt `changeLanguage()` direkt

### 🆕 Sprach-Switcher

SettingsPanel-Dropdown zeigt jetzt alle 3 Sprachen (DE, EN, IT) dynamisch aus `SUPPORTED_LANGUAGES`. Marco kann manuell auf Italienisch umschalten — Auswahl persistiert über App-Updates.

Versions-Bump 0.5.36 → 0.5.37.

---

## [v0.5.36] — 2026-05-08

🛩 **VFR/Manual-Mode: Aircraft-Mismatch wird Warnung statt Block.**

User-Stance: „wir sollten dem Piloten vertrauen". Der bisherige Hard-Block beim Aircraft-Type-Mismatch im VFR/Manual-Modus widersprach dem — Pilot hat im Picker bewusst eine Aircraft gewählt, aber falls X-Plane die ICAO als `ATCCOM.AC_MODEL XXX.0.text` meldet oder Custom-Liveries den Title verändern, fiel der Loose-Match durch und der Start wurde geblockt.

### 🆕 Was sich ändert

- Backend: neuer Error-Code `aircraft_mismatch_warning` (statt `aircraft_mismatch`) im VFR-Pfad
- ManualFlightPlan bekommt `acknowledge_aircraft_mismatch: bool` Feld
- Erst-Versuch ohne Flag → liefert Warnung zurück
- Modal zeigt **gelbes Warn-Banner** + **„Trotzdem starten"-Button**
- Klick → Re-Submit mit `acknowledge=true` → Check wird übersprungen
- Mismatch wird im Recorder weiter geloggt (für Forensik) aber blockt nicht

### IFR-Mode unberührt

`flight_start` (IFR mit SimBrief-OFP) liefert weiter den harten `aircraft_mismatch`-Error. Begründung: bei IFR ist der OFP die Source-of-Truth und ein Mismatch deutet auf einen Buchungs-Fehler hin.

Versions-Bump 0.5.35 → 0.5.36.

---

## [v0.5.35] — 2026-05-08

🐞 **Touchdown-V/S Capture für GA + sparse-DataRef-Cases gefixt — Position-Sampling adaptiv unter 1000ft AGL.**

### Hintergrund

User-Report aus dem GSG-301 GA-Flug (Cessna 152 in X-Plane 12, Forensik-Log analysiert): peak_vs_fpm=-33 fpm gemeldet, peak_g_force=1.36 — passte nicht zueinander. Die echte TD-V/S war vermutlich -300 bis -400 fpm.

**Root-Cause-Analyse aus dem JSONL:**
- Position-Sampling auf **0.1 Hz** (= 10.4s Mean-Interval, 91% der Frames mit >10s Lücken)
- 10s vor TD: AGL 145 ft, V/S **-360 fpm** (letzter airborne Sample)
- TD-Frame: Lücke von 10.44s → kompletter Touchdown-Moment fiel durch
- Lua-30-Sample-Estimator spannte das Fenster über den ganzen Approach (statt Flare) und gab geglätteten Mittelwert zurück

### 🆕 Fix 1 — Adaptive Position-Rate

`adaptive_tick_interval()`: Tick-Cadence je nach Phase + AGL:
- Cruise/Climb/Descent: 3s (unverändert)
- Approach/Final/Takeoff bei AGL <1000ft: **2s** (= 0.5 Hz)
- AGL <500ft: **1s** (= 1 Hz)
- AGL <100ft (Flare/Wheels-Up): **500ms** (= 2 Hz)

### 🆕 Fix 2 — JSONL-Append pro Tick

Vorher: JSONL-Append war IM phpVMS-OK-Branch → wurde nur bei erfolgreichem phpVMS-POST geschrieben (8-30s Cadence). Jetzt: nach MQTT-Publish, vor phpVMS-POST → jeder Tick im Log.

### 🆕 Fix 3 — V/S-Estimator Sparse-Sampling-Fallback

Neuer `last_low_agl_vs_fpm`-Tracker in FlightStats: speichert die letzte airborne V/S unter 500ft AGL mit Timestamp. Wird kontinuierlich pro Tick upgedated.

X-Plane Priority-Chain neu:
- Bevorzugt `agl_estimate_xp` falls Fenster <3s (= echte Flare)
- Falls Fenster ≥3s = unplausible (= sparse-Sampling-Spread): verwendet `last_low_agl_vs_fpm` falls innerhalb 15s
- Bei beiden vorhanden: nimmt den deeperen (= numerisch kleineren)

Neue `vs_source`-Labels:
- `agl_estimate_xp_or_last_low` (beide vorhanden, deeper gewählt)
- `last_low_agl_vs` (Estimator implausibel, last_low gerettet)
- `agl_estimate_xp_implausible_window` (last resort)

### 🆕 Fix 4 — Go-Around-Detector empfindlicher

- `GO_AROUND_AGL_RECOVERY_FT`: 200 → **150 ft** (sparse Sampling)
- `GO_AROUND_MIN_VS_FPM`: 500 → **300 fpm** (slow GA Aircraft klettern selten >500fpm)

### Erwartung für GA-Flüge ab v0.5.35

Bei Cessna 152 in X-Plane mit Standard-DataRef-Rate:
- Position-Frames im Final: 1 alle Sekunde (statt 1 alle 10s)
- Touchdown-V/S richtig gefangen via `last_low_agl_vs_fpm` falls Lua-Estimator wegen Sim-FPS sparse läuft
- Go-Around bei Cub/C152-Style climb-out korrekt detektiert

Versions-Bump 0.5.34 → 0.5.35.

---

## [v0.5.34] — 2026-05-08

🛡 **JSONL-Forensik-Logs jetzt vollstaendig — alles was MQTT publiziert landet auch im Log.**

### Hintergrund

Beim Recovery-Vorfall heute hatten wir versucht aus den JSONL-Forensik-Logs die verlorenen Touchdown-Daten zu rekonstruieren. Problem: das `landing_scored`-Event im JSONL hatte nur 4 Felder (`score`, `peak_vs_fpm`, `peak_g_force`, `bounce_count`) — die ~50 detaillierten Forensik-Felder die der Live-MQTT-Touchdown-Topic publiziert (Approach-Stability v2, Landing-Quality, Wind-Komponenten, Runway-Match, V/S-Estimator-Vergleiche, etc.) fehlten komplett.

### 🆕 Neue JSONL-Events

`recorder::FlightLogEvent` bekommt 4 neue Variants — alle parallel zum jeweiligen MQTT-Topic geschrieben:

- **`TouchdownComplete`** — kompletter `TouchdownPayload` (= alle ~50 Felder die der Live-Recorder bekommt)
- **`PirepFiled`** — kompletter `PirepPayload` (Block/Flight-Time, Fuel-Aggregate, Distance, Peak-Altitude, Landing-Score, Go-Arounds, Touchdown-Count, Gates, Approach-Runway, Divert)
- **`BlockSnapshot`** — Out-Of-Block Pre-Flight-Snapshot
- **`TakeoffSnapshot`** — Wheels-Up-Snapshot

Format: `{ "type": "...", "timestamp": "...", "payload": {...} }` — `payload` ist `serde_json::Value` damit das Schema mitwachsen kann ohne dass alte Logs unparsbar werden.

### Was das ermoeglicht

Falls die Server-DB jemals wieder Daten verliert, kann ein offline Recovery-Tool die Touchdown-/PIREP-Rows **1:1** aus dem JSONL rekonstruieren — keine Approximationen mehr, keine fehlenden Felder.

### Backwards-Compat

`LandingScored` (v0.5.0+) bleibt erhalten als kleinerer Event fuer Tools die nur den Score-Indikator brauchen. `TouchdownComplete` wird zusaetzlich geschrieben.

Versions-Bump 0.5.33 → 0.5.34.

---

## [v0.5.33] — 2026-05-08

🐞 **Aircraft-Picker funktioniert jetzt richtig: alle Flugzeuge, nur Ground+Active, voll DE+EN.**

### 🐞 Behoben

**Problem (v0.5.32):**
v0.5.32 versuchte `/api/fleet/{id}/aircraft` aufzurufen — diesen Endpoint gibt es in phpVMS-V7 **nicht** (nur `/api/fleet/aircraft/{id}` für ein einzelnes Aircraft per ID). Resultat: alle per-Subfleet-Calls liefen ins 404, wurden „graceful skipped", Picker zeigte „Keine Aircraft in deiner Fleet verfügbar" trotz vorhandener Flugzeuge.

**Fix in v0.5.33:**
- `GET /api/fleet?limit=100&page=N` paginiert (verifiziert via offizielle phpVMS-Docs + Source-Code)
- `SubfleetResource` enthaelt `aircraft`-Array bereits inline → kein N+1
- Pages-Loop bis non-volle Page (Cap 50 Pages)
- Neuer `SubfleetWithAircraft`-Typ mit `#[serde(default)] aircraft: Vec<AircraftDetails>`
- `get_all_aircraft()` flatten ueber alle Subfleets

### 🆕 Filter (Pilot-Wunsch)

**Nur tatsächlich verfügbare Flugzeuge im Picker:**
- `state == 0` (PARKED — nicht IN_USE / IN_AIR)
- `status == "A"` (ACTIVE — nicht MAINTENANCE / STORED / RETIRED / SCRAPPED / WRITTEN_OFF)
- Tracing-Log: `before=N after=M` für Diagnose

### 🌍 Vollständige DE+EN-Lokalisierung

**Neue i18n-Keys (35+):**
- `manual_flight.*` — Header, Step-Titles, Loading, Empty, Search, List-Total, No-Match, Submit-Buttons, **alle 6 Form-Felder** (Block-Fuel, Flight-Time, Cruise-Level, Route, Alternate, ZFW) je mit Label + Placeholder + Help-Text
- `bid_card.*` — VFR-Start-Button + Tooltip, komplette Hint-Box (Title + IFR/VFR-Zeilen)
- `flight.error.*` (10 Codes) jetzt **auch im Manual-Modal** lokalisiert (war vorher roher Code wie `no_sim_snapshot: ...`)

### ✏️ Sprache

- „Aircraft" → „Flugzeug" überall im UI (DE)
- „Aircraft" → „Aircraft" (EN, weil das im Englischen korrekt ist)
- Empty-State-Meldung neu: nennt konkret die Filter-Gründe (Einsatz/Luft/Wartung)

Versions-Bump 0.5.32 → 0.5.33.

---

## [v0.5.32] — 2026-05-08

🐞 **Aircraft-Picker zeigt jetzt einzelne Aircraft, nicht Subfleets.**

### 🐞 Behoben

**Problem (User-Feedback aus v0.5.30/31):**
Im VFR/Manual-Mode-Aircraft-Picker tauchten Einträge wie „DLH-A319-CFM-SL", „BAW-A319-IAE-WTF" auf — das sind **Subfleet-Namen, keine Aircraft-Registrations**. Pilot konnte daraus keinen einzelnen Flieger auswählen („mit einem Subfleet kann ich nicht fliegen").

**Root-Cause:**
phpVMS-V7-Endpoint `GET /api/fleet` liefert **Subfleets** (= Sammlung von Aircraft eines Typs), nicht einzelne Aircraft. Unsere v0.5.27-Implementation hat den Response naiv in `AircraftDetails` deserialisiert — das hat zwar deserialisiert (alle Felder sind `Option`), aber `registration`/`icao` der Subfleet-Liste sind eben Subfleet-Felder, nicht Aircraft-Felder.

**Fix in v0.5.32:**
- Neuer `SubfleetSummary`-Typ in `api-client` für korrekte Subfleet-Deserialisierung (`id`, `name`, `icao`, `type`)
- Neue Methode `Client::get_all_aircraft()`: aggregiert über alle Subfleets via N+1-Pattern (`GET /api/fleet/{id}/aircraft` pro Subfleet)
- Per-Subfleet-Failures werden geloggt aber nicht propagiert — ein einzelner kaputter Subfleet crashed nicht den Picker
- `fleet_list_at_airport` ruft jetzt `get_all_aircraft()` statt `get_fleet()` auf
- phpVMS-Subfleet-Rank-Restriktion wirkt weiter server-seitig (= Pilot sieht nur was er fliegen darf)

Versions-Bump 0.5.31 → 0.5.32.

---

## [v0.5.31] — 2026-05-08

🎯 **Mode-Hint-Box deutlicher: klare Regel statt Marketing-Text.**

### 🔧 Geändert

User-Feedback: der v0.5.29-Hinweis war zu unscheinbar/unklar. „IFR Start: nutzt SimBrief-OFP" sagt nicht eindeutig dass es **PFLICHT** ist. „VFR Start funktioniert auch ohne SB" sagt nicht eindeutig dass es **OPTIONAL** ist.

**v0.5.31 — neue Hint-Box mit klarer Regel-Struktur:**

```
┌─────────────────────────────────────────────────────────────┐
│ 💡 Welchen Button nutzen?                                   │
│                                                             │
│ 🛫 IFR Start    NUR mit SimBrief-OFP (Plan-Daten kommen    │
│                  aus dem OFP).                              │
│                                                             │
│ 🛩 VFR Start    AUCH OHNE SimBrief-OFP — du gibst Aircraft  │
│                  + Block-Fuel selbst ein.                   │
└─────────────────────────────────────────────────────────────┘
```

- **Titel** „Welchen Button nutzen?" macht Frage explizit
- **Zwei klare Zeilen** mit Icon + Button-Name (color-coded blau/gelb) + Regel
- **Bold-Highlights** auf dem entscheidenden Wort: „NUR mit" vs „AUCH OHNE"
- **Karten-Border** statt linker Border-Strich — visuell prominenter

Versions-Bump 0.5.30 → 0.5.31.

---

## [v0.5.30] — 2026-05-08

🎯 **Aircraft-Picker zeigt jetzt die GESAMTE Fleet — keine Airport-/State-Einschränkung.**

### 🔧 Geändert

**Problem (User-Feedback aus v0.5.27/28-Tests):**
Beim VFR/Manual-Mode-Aircraft-Picker für einen LEPA-Bid:

> „Keine Aircraft am LEPA verfügbar (alle in use, in Maintenance, oder phpVMS-Endpoint nicht eingerichtet)."

Pilot konnte keinen Flug starten obwohl Aircraft in der Fleet existieren — sie standen aber an anderen Airports.

**v0.5.30 Lösung:**
- **Kein Airport-Filter mehr** — alle Aircraft die der Pilot fliegen darf werden angezeigt (= /api/fleet, Subfleet-Rank-Restriktion bleibt server-seitig)
- **Kein State-Filter mehr** — auch in-use / in-flight / Maintenance Aircraft werden angezeigt mit visuellem Indikator
- **Smart-Sort**: Aircraft am Departure-Airport stehen oben in der Liste, dann nach State (parked vor in-use), dann alphabetisch
- **Visuelle Indikatoren** in der Liste:
  - Grün-fettes `@LEPA`-Tag wenn Aircraft am Dep-Airport steht
  - Status-Pill: `🔒 in Use` (gelb) / `✈ in Flight` (cyan) / `🔧 Maintenance` (rot) bei nicht-parked Aircraft
- **Header zeigt Count**: "12 Aircraft gesamt · Aircraft am LEPA stehen oben"

**Falls Pilot ein in-use/Maintenance-Aircraft auswählt:** phpVMS-Prefile lehnt mit klarer Fehlermeldung ab — Pilot kann dann anderes wählen.

### 🔧 Implementation

- **Rust**: `fleet_list_at_airport()` ruft jetzt nur `client.get_fleet()` (= alle Aircraft), nicht mehr `/api/airports/{icao}/aircraft`. icao-Parameter bleibt für Sort-Priority. State-Filter (`state == 0`) entfernt.
- **Frontend**: Aircraft-List-Item zeigt Airport + State-Pill. Empty-State-Message angepasst.

Versions-Bump 0.5.29 → 0.5.30.

---

## [v0.5.29] — 2026-05-08

🎯 **Pilot entscheidet komplett selbst — Auto-IFR/VFR-Kategorisierung entfernt, durch klaren Hinweis-Text ersetzt.**

### 🔧 Geändert

**v0.5.28 hatte automatische IFR/VFR-Pills** auf jeder Bid-Card (gruen/gelb basierend auf `flight_type`-Code). Das war zwar "nur Hinweis", fühlte sich aber wie eine Kategorisierung an — Bids wurden mit einem Label versehen.

**v0.5.29: Pills entfernt, statt dessen klare Text-Box** unter den Action-Buttons:

> 💡 **IFR Start**: nutzt SimBrief-OFP (Block-Fuel/Route/Weights aus dem Plan).
> **VFR Start**: funktioniert auch ohne SB — du wählst Aircraft + Block-Fuel selbst.
> Pilot entscheidet je nach Flug.

**Konsequenz:**
- Keine Auto-Detection mehr nach `flight_type` (= keine Annahme „dieser Bid ist IFR")
- Keine farblichen Kategorien-Pills
- Hinweis-Text steht IMMER da (nicht conditional)
- Beide Buttons immer sichtbar wenn kein aktiver Flug läuft
- Trust-the-Pilot in Reinform

### 🔧 Implementation

- `flightRulesHint()`-Helper entfernt (= Auto-Detection-Logik)
- IFR/VFR-Pill-JSX in BidsList aus dem Header entfernt
- Neuer `.bid-card__mode-hint`-Block unter den Buttons mit kompaktem Erklärungs-Text
- CSS umgebaut: `.bid-card__rules-badge--*` entfernt, neuer `.bid-card__mode-hint` Style (subtle grau-bordered)

Versions-Bump 0.5.28 → 0.5.29.

---

## [v0.5.28] — 2026-05-08

🎯 **UX-Polish für VFR/Manual-Mode: klarere Button-Labels + IFR/VFR-Hinweis-Pill auf Bid-Cards.**

Folgepatch zu v0.5.27. Funktionalitaet identisch, nur bessere Lesbarkeit + Hinweise. Kein Verhaltens-Aenderung — Pilot entscheidet weiter selbst (= keine harte Enforcement nach flight_type).

### ✨ Neu

**1. Button-Labels eindeutig:**
- "Start Flight" → **"🛫 IFR Start (SimBrief)"**
- "🛩 VFR/Manual-Mode" / "🛩 Manual-Override" → einheitlich **"🛩 VFR Start (manuell)"**

**2. Hover-Tooltips erklaeren wann zu nutzen:**
- IFR-Button: „Standard-Flug nach IFR-Regeln, basiert auf deinem SimBrief-OFP. Block-Fuel, Route, Weights und Alternates kommen aus dem OFP."
- VFR-Button: „Manueller Flug-Start ohne SimBrief-OFP — z.B. fuer VFR, kleine Pisten oder Pattern-Training. Du waehlst Aircraft + Block-Fuel selbst. Auch nutzbar als Aircraft-Override fuer Bids mit SimBrief-OFP."

**3. IFR/VFR-Hinweis-Pill auf jeder Bid-Card** (Header-Meta-Row):
- **IFR-Pill** (gruen): bei flight_type ∈ {J, F, C, M, I, V, S, R} — Scheduled, Charter, Mil, Special
- **VFR-Pill** (gelb): bei flight_type containing "VFR", oder ∈ {G, T, X} — General Aviation, Training, Test
- Kein Pill: bei unbekanntem oder leerem flight_type

Reine **Information** — KEINE Filter, KEINE Pflicht. Pilot kann auch IFR-Bid mit VFR-Manual-Mode fliegen wenn er will, oder VFR-Bid mit SimBrief-OFP. Trust-the-Pilot-Design.

**4. Tooltip-Hint auf der Pill:**
- IFR-Pill: „IFR-typischer Bid (Scheduled / Charter). Empfohlener Flow: SimBrief-OFP + 'IFR Start'-Button. Du kannst aber auch VFR/Manual fliegen."
- VFR-Pill: „VFR-typischer Bid (GA / Training / Test). Empfohlener Flow: 'VFR Start (manuell)'-Button. Du kannst aber auch SimBrief nutzen falls vorhanden."

### 🔧 Implementation

- **BidsList.tsx**: neuer Helper `flightRulesHint(type)` mit Detection-Logik. Pill rendert nur wenn Hint != null. Button-Labels in JSX angepasst.
- **App.css**: `.bid-card__rules-badge--ifr` (gruen) + `--vfr` (gelb) parallel zu existierendem type-badge.

### ⚠ Hinweise

- Wenn dein VA-flight_type-Schema nicht in {J,F,C,M,I,V,S,R,G,T,X} fällt: kein Hinweis-Pill. Zwei Optionen: phpVMS-Admin → Flight-Type-Codes auf ICAO-Standard setzen, ODER `flight_type` mit "VFR" / "IFR" als Substring (z.B. "VFR-Pattern" oder "IFR-Charter").
- Detection-Pattern ist in `flightRulesHint()` lokalisiert — bei VA-spezifischen Konventionen einfach die Switch-Case erweitern.

---

## [v0.5.27] — 2026-05-08

🎯 **VFR/Manual-Flight-Mode — Flug-Start ohne SimBrief-OFP für VFR-Flüge, kleine Pisten, GA.**

### ✨ Neu

**Problem:** AeroACARS hat bisher SimBrief-OFP für jeden Bid verlangt (siehe `lib.rs` Z.4848: `"no aircraft on this bid — please prepare a SimBrief OFP first"`). Für VFR-Flüge unterstützt SimBrief aber kein OFP-Routing — Pilot konnte zwar Bid in phpVMS erstellen, AeroACARS verweigerte aber den Start.

**Lösung:** Neuer „🛩 VFR/Manual-Mode" Button auf jeder Bid-Card. Pilot wählt:

1. **Aircraft-Picker** mit Suche
   - phpVMS-API `GET /api/airports/{icao}/aircraft` (mit Fallback auf `/api/fleet`)
   - Filter: nur Aircraft im State `parked` (= verfügbar)
   - Sim-Default-Auswahl: AeroACARS sieht den im Sim geladenen Aircraft → vorausgewählt mit Match-Erkennung über Registration ODER ICAO
   - Volltext-Suche über ICAO / Registration / Name

2. **Manual-Flight-Plan-Form**
   - **Pflicht-Felder**: Block-Fuel (kg), erwartete Flugzeit (min) — sonst keine Fuel-Score / ETA möglich
   - **Optional**: Cruise-Level (ft), Route (free-text), Alternate (ICAO), ZFW (kg)

3. Klick „🛩 Flug starten" → identischer Flow wie Standard-`flight_start` aber ohne SimBrief-Pflicht.

### 🔧 Implementation

**Client (lib.rs):**
- Neue Tauri-Commands `fleet_list_at_airport(icao)` + `flight_start_manual(bid_id, plan)`
- `ManualFlightPlan` Deserialize-Struct mit Pflicht-Feldern + Optionals
- Identischer Pre-Flight-Gate (ground + dpt-distance), Aircraft-Mismatch-Check, PIREP-Prefile, Streamer-Spawn
- `FlightStats.flight_plan_source: "simbrief" / "manual" / None` (carry-through im PIREP-Body als notes-Prefix)
- `planned_burn_kg` Fallback: 90% des block_fuel falls Pilot's planned_burn nicht angibt

**API-Client (api-client/src/lib.rs):**
- Neue Methoden `client.get_aircraft_at_airport(icao)` + `client.get_fleet()`

**Frontend (TS/React):**
- Neue Komponente `<ManualFlightModal>` mit 2-Stage-Workflow (Aircraft → Plan)
- 130+ Zeilen CSS für das Modal (matching dark theme)
- Manual-Mode-Button in BidsList:
  - Bei Bid OHNE simbrief: „🛩 VFR/Manual-Mode" als gleichwertige Action
  - Bei Bid MIT simbrief: „🛩 Manual-Override" (= falls Pilot anderes Aircraft fliegen will)
- Sim-Snapshot wird als simHint übergeben für Aircraft-Default-Auswahl + Block-Fuel-Default

**Backward-kompatibel:** existierender `flight_start`-Flow bleibt unverändert. SimBrief-Bids gehen weiter den OFP-Path, Manual-Mode ist additiv.

### ⚠ Hinweise

- **Aircraft-Subfleet-Validation**: phpVMS enforced server-side — Pilot mit Rank-N kann keine Aircraft fliegen die Rank N+1 brauchen. Manual-Picker zeigt aber alle Aircraft am Departure-Airport.
- **Fuel-Planung**: ohne explicit `planned_burn_kg` nehmen wir 90% des Block-Fuel als Trip-Schätzung. Realistischer wäre 75% (= mit Reserve), aber 90% ist bei VFR/GA üblicher.
- **PIREP-Notes**: bei Manual-Mode wird automatisch `Manual/VFR-Mode (kein SimBrief-OFP). Block: XXX kg, ETA: YY min` in den PIREP-Notes-Block geschrieben damit VA-Owner sieht dass es ein Manual-Flug war.

---

## [v0.5.26] — 2026-05-08

🎯 **9 neue Landung-Sicherheits-Indikatoren + DA-Gate (200 ft) + sim-/aircraft-spezifische Limits.**

Folgepatch zu v0.5.25 — die Approach-Stability-v2 deckte den **Anflug-Pfad** korrekt ab. v0.5.26 ergänzt **per-Touchdown-Sicherheits-Metriken** und einen strengeren **Decision-Altitude-Gate-Check**.

### ✨ Neu — Sicherheits-Indikatoren am Touchdown

**1. Wing-Strike-Severity (%)**
Bank am TD relativ zum aircraft-spezifischen Wing-Strike-Limit. 0% = wings level, 100% = am Limit. Conservative-Defaults pro ICAO (CL60: 6°, A321: 7°, B737: 8°, C172: 15°, etc.). Über 60% gibt Coaching-Hinweis, über 80% = Alert.

**2. Float-Distance (m)**
Distanz Threshold-Crossing → Touchdown. Long-Landing-Indikator. Standard 300-400 m. > 1000 m = Runway-Overrun-Risk auf kurzen Bahnen.

**3. Touchdown-Zone (1/2/3)**
FAA-Drittel-Klassifikation: Zone 1 = erstes Drittel (correct), Zone 2 = mittleres (long), Zone 3 = letztes (overshoot). Aircraft-Type-unabhängig.

**4. Vref-Deviation (kt)**
IAS am TD vs. Vref. **Source-Chain**: PMDG-FMC (MSFS-only) → ICAO-Kategorie-Default → unbekannt. Vref-DB enthält 30+ Aircraft-Types von B748 bis C172.

**5. Stable-At-DA (200 ft AGL/HAT)**
Strengerer 200-ft-Sub-Gate-Check (= ICAO Decision-Altitude-Standard für CAT-I-ILS). Tighter Cutoffs als beim 1000-ft-Gate: jerk < 80, bank < 3°, ias < 8 kt.

### ✨ Neu — Aggregat-Metriken

**6. Stall-Warning-Counter** — Anzahl `stall_warning=true`-Samples im gesamten Approach-Buffer. Indiziert ob Pilot Speed-Margin zu eng hatte.

**7. Yaw-Rate am TD (°/s)** — heading-Änderung im 1-sec-Window vor TD. Hoch = Ground-Loop-Risk bei Crosswind-Landing.

**8. Brake-Energy-Proxy (kJ/m)** — `(½ × Mass × IAS²) / Rollout`. Indiziert Brake-Pack-Thermal-Stress.

**9. Aircraft-spezifische Limits-DB** (ICAO-basiert)
Hardcoded `aircraft_limits_for(icao)` mit `max_bank_landing_deg` + `typical_vref_kt` für 30+ Standard-Types. Fallback `8°/None` für unbekannte ICAO. Pilot/VA-Override via DBasic Tech-Limits weiter erste Priorität.

### ✨ Neu — UX

**Neue „🎯 Landing-Quality"-Card im LandingAnalysis-Modal** zusätzlich zur Approach-Stability-Card. Zeigt 6 MetricTiles (Wing-Strike-Risk / TD-Zone / Float-Distance / Vref-Dev / Yaw-Rate / Brake-Energy) mit Tone-Coding und ausführlichen Hover-Tooltips.

**Erweiterte Coaching-Texte** in der Approach-Stability-Card:
- „Wing-Strike-Risk 85% — Bank am TD nahe Aircraft-Limit. Crosswind-Korrektur über Sideslip (Wing-Down + Rudder), nicht über Crab-into-flare-only."
- „Touchdown im letzten Drittel der Bahn (Zone 3) — Runway-Overrun-Risk auf kurzen Bahnen. Pre-flare nicht zu lang, früher abfangen."
- „IAS am TD -8 kt unter Vref — Stall-Risiko."
- „Stabil bei 1000 ft, aber NICHT mehr bei 200 ft (DA). Final-Phase wackelig."
- „⚠ 3 Stall-Warning-Events im Approach detektiert. Speed-Margin zu eng."

### 🔧 Implementation

- **Client (`lib.rs`)**: `aircraft_limits_for(icao)` Lookup-DB mit 30+ Types. `compute_approach_stability_v2` erweitert um DA-Gate (200 ft Filter) + Stall-Counter. Per-Touchdown-Section im File-PIREP-Path: Wing-Strike-Severity, Float-Distance + TD-Zone aus runway_match, Vref-Deviation mit Source-Chain, Yaw-Rate aus 1-sec-snapshot_buffer-Lookback, Brake-Energy-Formel.
- **MQTT-Payload**: 9 neue Felder (alle `Option<>`, `skip_serializing_if`).
- **Server (`recorder`)**: 9 neue Spalten in `touchdowns`-Tabelle (idempotente ALTER), insertTouchdown extrahiert, /api/touchdowns liefert sie typed.
- **Webapp**: Neue `_LandingQualityCard.tsx` mit 6 MetricTiles. ApproachStabilityCard um Coaching-Texte erweitert. TouchdownDto um 9 Felder.
- DB-Backup pre-deploy: `aeroacars-live.db.backup-pre-landing-quality`.

### ⚠ Hinweise

- **MSFS-Bank**: Sign noch nicht geflippt (im Gegensatz zu Pitch in v0.5.24). Wenn Wing-Strike-Severity-Daten nach Real-World-Tests komisch aussehen → Patch nachschieben.
- **Vref-Quelle "icao_default"**: konservativ pro Aircraft-Type, Pilot-Vref-Addends (Wind/Gust/Ice) NICHT berücksichtigt → Deviation-Werte nur als grobes Indiz, PMDG-FMC-Vref ist autoritativ wenn verfügbar.
- **Brake-Energy-Proxy**: ohne `landing_weight_kg` aus PMDG/Sim wird Default 50.000 kg verwendet — Werte ohne LDW-SimVar relativ.

---

## [v0.5.25] — 2026-05-08

🎯 **Approach-Stability v2: Stable-Approach-Gate-konformes Stabilitäts-Maß. Pilot versteht endlich was der Score bedeutet.**

### 🐛 Behoben

**Approach-Stability-Algorithmus war inkorrekt für Real-World-Cases.**

Pre-v0.5.25-Algorithmus berechnete `σ V/S` und `σ Bank` über das gesamte Approach+Final-Buffer-Window (= 5000 ft AGL bis Touchdown). Probleme:

- **ATC-Vectoring-Turns** (Bank 20-30° auf Anweisung) wurden als Pilot-Instabilität bestraft
- **Initial-Descent-Step-Downs** (Flaps-Stages, Speed-Down) erhöhten σ V/S obwohl Flugverhalten korrekt
- **σ um Mittelwert** misst NICHT Glide-Slope-Abweichung — ein Pilot der konstant -1100 fpm hält (über Glide-Slope) bekommt perfekten σ-Score
- **Mountain-Airports** (LSGS, LFKB) — AGL fluktuiert über Bergkämmen, Window-Filter falsch
- **GA-Anflüge** wurden mit 3°-ILS-Schwellwerten verglichen — C172 auf 5° Visual-Approach falsch bewertet
- **Späte RWY-Wechsel** (ATC ändert von 09L auf 09R bei 1200 ft AGL) bestraften Pilot für die ausgeführte Anweisung

### ✨ Neu — Approach-Stability v2

**HAT statt AGL als Window-Filter** (Mountain-Airport-tauglich)
Höhenfilter über `MSL_altitude − arr_airport_elevation` statt `AGL`. AeroACARS-Client sucht arr-Airport-Elevation aus dem phpVMS-API-Cache (state.airports.elevation). Fallback auf AGL wenn unbekannt — `approach_used_hat`-Flag in PIREP zeigt welche Methode genutzt wurde.

**5 Primär-Metriken (Score-relevant) im 1000-ft-Gate:**

1. **V/S-Jerk** — mean `|Δvs|` sample-to-sample. **Sim/Aircraft-agnostisch** (Jet, Turboprop, GA gleichermaßen). Schwellwerte: < 100 fpm/tick = sehr stabil, > 300 fpm/tick = unstabil.

2. **Bank σ (filtered)** — Standard-Deviation Bank, **Vector-Windows ausgenommen** (5 sec vor/nach RWY-Change). Pilot wird nicht für ATC-Turn bestraft.

3. **IAS σ** — Speed-Stabilität. < 5 kt = on-target, > 15 kt = große Schwankungen.

4. **Excessive-Sink-Flag** — `True` wenn ein Sample im Gate `V/S < -1000 fpm`. FAA-Limit-Verletzung (Pflicht-Go-Around).

5. **Stable-Config-Flag** — Gear ≥ 99% AND Flaps ≥ 70% am Gate-Eintritt.

**Composite Stable-At-Gate-Indikator:** `stable = jerk_ok AND bank_ok AND ias_ok AND !excessive_sink AND config_ok`. Pilot kriegt klares Boolean: ✓ STABLE GATE oder ⚠ UNSTABLE GATE.

**Sekundär (informativ, nicht Score-relevant):**
- V/S-Deviation vs 3°-ILS-Profil — relevant für ILS-Anflüge, bei GA/Visual nur informativ
- Max V/S-Deviation unter 500 ft AGL — kritischste Phase
- Late-RWY-Change-Detection — bestraft Pilot nicht, zeigt Hinweis-Pill

### ✨ Neu — UX-Transparenz für Piloten

**Neue ApproachStabilityCard im LandingAnalysis-Modal:**
- Composite-Indicator-Pill ✓ STABLE oder ⚠ UNSTABLE direkt im Card-Header
- Confidence-Hinweis (HAT vs. AGL, Sample-Count)
- 7 MetricTiles mit individuellen Tone-Bewertungen + ausführliche Hover-Tooltips erklären was jede σ bedeutet + Schwellwerte
- **Coaching-Section** mit konkreten Tipps wenn Score schlecht:
  - „V/S-Jerk 350 fpm/tick — du hast die Sinkrate stark verändert. Stabiles Sinkprofil halten: kleine Korrekturen früh, nicht große Korrekturen spät."
  - „Bank-σ 6.2° über 5° — späte Lineup-Korrekturen vermeiden, früh auf den Localizer einschneiden."
  - „RWY-Wechsel unter 1500 ft AGL detektiert — wurdest nicht bestraft, aber im Real-Op: stabil-prüfen oder go-around."
- Bei sauberem Anflug: ✨-Lob-Box

### 🔧 Implementation

- **Client (`lib.rs`)**: `ApproachBufferSample`-Struct erweitert um msl_ft / ias_kt / heading_true_deg / gear_position / flaps_position / selected_runway. `compute_approach_stability_v2(buf, arr_elevation)` implementiert HAT-Window + V/S-Jerk + IAS-σ + Excessive-Sink + Stable-Config. arr_airport_elevation_ft wird beim ersten Streamer-Tick aus dem state.airports-Cache (phpVMS-API) gelesen.
- **MQTT-Payload (`aeroacars-mqtt`)**: TouchdownPayload um 5 Felder erweitert (`approach_vs_jerk_fpm`, `approach_ias_stddev_kt`, `approach_excessive_sink`, `approach_stable_config`, `approach_used_hat`).
- **Server (`recorder`)**: 5 neue Spalten in `touchdowns`-Tabelle (idempotente ALTER), insertTouchdown extrahiert mit boolToInt-Helper, /api/touchdowns liefert sie typed.
- **Webapp**: TouchdownDto erweitert. Neue ApproachStabilityCard als eigene Datei (`_ApproachStabilityCard.tsx`) mit responsivem 4-tile-Grid, Coaching-Texten, Late-RWY-Change-Pill.
- DB-Backup pre-deploy: `aeroacars-live.db.backup-pre-approach-stability-v2`.

### ⚠ Hinweise

- Pre-v0.5.25-Touchdowns zeigen die alte σ-Auswertung als Fallback in der Card (mit Hinweis).
- HAT-Window erfordert dass arr_airport_elevation_ft im phpVMS-API-Cache landete — passiert beim Bid-Pickup ohnehin via `airport_get`-Command. Wenn nicht: Fallback auf AGL mit Confidence-Warnung.
- 3°-Glide-Slope-Target ist nur sekundär — der primäre Score (V/S-Jerk) funktioniert sim/aircraft-agnostisch und ist NICHT auf 3°-ILS-Profile zugeschnitten.

---

## [v0.5.24] — 2026-05-08

🎯 **Pitch-Sign-Fix für MSFS + frame-genaues Wheels-Up-Capture für Tail-Strike-Detection.** Plus Client-Version-Tag im MQTT-Stream für Version-Compliance-Tracking.

### 🐛 Behoben

**1. MSFS pitch-sign invertiert (= alle MSFS-Pilot-PIREPs hatten falsches Vorzeichen)**

MSFS-SimConnect hat eine inverse Konvention: `PLANE PITCH DEGREES` reportet **positive Werte wenn die Nase UNTER dem Horizont** ist (= Universal-Aviation-Konvention macht es umgekehrt: positiv = nose-up). AeroACARS las den Wert ohne Sign-Flip und schrieb daher invertierte Werte in alle MSFS-Pilot-PIREPs:

- A321-Flare bei +5° real → gespeichert als -5° (Pilot sieht „Nose-down landing" obwohl er normal flared)
- A321-Rotation bei +11° real → gespeichert als -11°
- Alle `Touchdown Sideslip`, `Landing Pitch`, `Takeoff Pitch` Custom-Fields betroffen

phpVMS DisposableSpecial-Triggers nutzten `abs()` und maskierten den Bug an der Trigger-Stelle. Aber Pilot-PIREP-Detail-Views zeigten die unsinnigen negativen Werte. **Fix:** sign-flip im MSFS-Adapter-Boundary (`telemetry.rs::SimSnapshot{}`-Builder + `adapter.rs` Touchdown-Block-Reader). X-Plane bleibt unangetastet — dortige `sim/flightmodel/position/theta`-DataRef ist konventions-konform.

**2. Takeoff-Pitch-Capture frame-genau (= Tail-Strike-Check präziser)**

Bisher wurden `takeoff_pitch_deg` / `takeoff_bank_deg` im Streamer-Tick gestempelt — das ist 3-30s Cadence je nach Phase, also potenziell mehrere Sekunden NACH dem echten Wheels-Up-Frame. Bei diesen Sekunden hat das Aircraft schon weiter pitch-up rotiert (Initial-Climb), der gestempelte Wert war oft 2-3° höher als der eigentliche Rotations-Pitch.

**Fix:** der bestehende 50Hz-Touchdown-Sampler-Task fängt jetzt auch den umgekehrten Edge ab (`prev_in_air=false → in_air_now=true` = Wheels-Up). Capture innerhalb 20ms im physischen Lift-Off-Frame. Phase-Transition-Code in `step_flight` verwendet `sampler_takeoff_pitch_deg.or(snap.pitch_deg)` als Priority-Chain — Sampler-Wert wins wenn vorhanden, sonst Streamer-Tick als Fallback. Wirkt beide Sims (X-Plane via `gear_normal_force_n` Edge, MSFS via `on_ground` Edge).

Bei tail-strike-empfindlichen Aircraft wie der A321 (~9.7° max safe pitch) erspart das 2-3° False-Positive-Drift im phpVMS DisposableSpecial Tail-Strike-Check.

### ✨ Neu

**3. `client_version`-Field im MQTT-PositionPayload**

Pro Position-Tick schickt der Client jetzt `client_version: "0.5.24"`. Der aeroacars-live-Monitor sieht damit pro Pilot welche Build-Version sendet — nützlich für:

- Version-Compliance-Tracking („Pilot X läuft noch v0.5.15, hat den Numeric-Fix nicht")
- Bug-Korrelation („alle disagreement-Touchdowns kommen von v0.5.18-")
- Updater-Monitoring („wieviele % der Piloten sind auf der neuesten Version?")

Server-seitig: das Field landet in `flights.last_position_json` (= im rohen JSON-Snapshot pro Pilot) und kann dort per `json_extract()` aggregiert werden. Native Tabellen-Spalte folgt in einem Server-Patch falls nötig.

### ⚠ Hinweise

- Existierende PIREPs werden NICHT retroaktiv korrigiert — nur neu eingehende MSFS-Touchdowns ab v0.5.24-Pilot-Version haben korrektes Pitch-Vorzeichen
- DisposableSpecial-Tail-Strike-Triggers funktionieren weiterhin via `abs()` — Pre-v0.5.24-Daten triggern korrekt, Post-v0.5.24-Daten ebenfalls (MSFS-Werte jetzt mit positivem Vorzeichen)
- Update via Auto-Updater empfohlen damit MSFS-Pilot-PIREPs ab sofort intuitive Pitch-Werte zeigen

---

## [v0.5.23] — 2026-05-08

🎯 **Forensik-Werkzeuge: aeroacars-live-Monitor sieht jetzt alles was der Client sieht.** Plus harte Fixes für Session-Splitting bei Hin/Rück-Flügen und leere ICAO-Felder.

### ✨ Neu

**1. Auto-Upload des kompletten Flight-Logs nach PIREP-File**

Nach erfolgreichem `file_pirep` lädt der Client das komplette JSONL-Logfile automatisch als gzip an aeroacars-live (`POST /api/flight-logs/upload`). Der VA-Owner kann es dann über den **„📥 Client-Log"-Button** in der History-Detail-View herunterladen — ohne den Piloten kontaktieren zu müssen.

- Auth via dieselbe MQTT-Cred-Pair die schon in der Provisioning-Phase im OS-Keyring liegt — keine zusätzliche Konfiguration
- Fire-and-forget — Failure ist non-fatal, JSONL bleibt lokal verfügbar
- Pilot kriegt Activity-Log-Eintrag mit Größen-Statistik (z.B. „2342 KB raw → 412 KB gzip (18% Kompression)")
- Bandwidth: typischer 2h-Flug ≈ 200-800 KB komprimiert
- Server-Storage: Auto-Purge auf VPS nach 90 Tagen vorgesehen

**Forensik-Wert:** der JSONL-Stream hat **mehr** als der MQTT-Server-Stream:
- ≈80 SimSnapshot-Felder pro Position-Tick (statt ≈35 via MQTT)
- Vollständiger User-Activity-Log
- PhaseChanged-Events mit altitude/groundspeed-Kontext bei jeder FSM-Transition
- Velocity-Body-Achsen, FCU-Setpoints, alle Lichter, COM/NAV-Radios, Autobrake, APU-State, Pushback-State, Seatbelts-/No-Smoking-Sign

**2. Touchdown-Algorithmen-Forensik im aeroacars-live-Monitor**

Bei jedem Touchdown laufen MSFS-Time-Tier- und X-Plane-Lua-30-Sample-Schätzer schon parallel — jetzt kriegt aeroacars-live alle Zwischenergebnisse und kann **Algorithmen-Disagreements** (= |xp_estimate − msfs_estimate| > 50 fpm) sichtbar machen für FSM-Edge-Case-Analyse.

Neue Felder in TouchdownPayload:
- `simulator`: „msfs" / „xplane" / „other"
- `vs_estimate_xp_fpm`: Lua-30-Sample-Schätzung
- `vs_estimate_msfs_fpm`: Time-Tier-Schätzung
- `vs_source`: welcher Pfad gewann („msfs_simvar_latched" / „agl_estimate_msfs" / „agl_estimate_xp" / „sampler_gear_force" / „buffer_min" / „low_agl_vs_min")
- `gear_force_peak_n`: X-Plane-Sampler-Wert
- `estimate_window_ms`: Window-Größe des gewinnenden Schätzers
- `estimate_sample_count`: Samples im Berechnungs-Fenster

Webapp-seitig: Touchdowns-Tab kriegt **„🔬 Touchdown-Forensik nach Simulator"-Card** + Sim-Filter („MSFS / X-Plane / Alle") + **„⚠ Disagreement"**-Filter + LandingAnalysis-Modal kriegt **„🔬 Algorithmen-Forensik"-Card** mit beiden Schätzern nebeneinander.

### 🐛 Behoben

**3. MQTT-Identity-Felder nicht mehr als leere Strings serialisiert**

PositionPayload-Felder `callsign` / `aircraft_icao` / `dep` / `arr` / `aircraft_registration` sind jetzt `Option<String>` mit `#[serde(skip_serializing_if = "Option::is_none")]`. Empty/whitespace-only Werte verschwinden komplett aus dem JSON statt als `""` gesendet zu werden.

**Hintergrund:** phpVMS-API liefert manchmal leere ICAO-Codes (Aircraft ohne `icao_code`-Feld in der VA-DB). Wenn der Client diese als `""` serialisierte, überschrieb der Server-COALESCE-UPSERT den vorher korrekt akkumulierten Wert in `flights.aircraft_icao` mit `""`. Resultat: Sessions starteten mit `aircraft_icao = NULL` obwohl der Pilot tatsächlich einen ICAO-getaggten Flieger flog.

### 🔬 Forensik-Workflow für VA-Owner (neu möglich ab v0.5.23)

1. Webapp Touchdowns-Tab → Filter „⚠ Disagreement" zeigt alle Landungen wo MSFS- und X-Plane-Schätzer auseinanderlagen
2. Klick auf Touchdown → 🔬-Card zeigt beide Werte + welcher gewann + Window-Konfidenz
3. Wenn |Δ| > 100 fpm und beide plausibel → Edge-Case lohnt anzuschauen
4. PilotHistory → Session-Detail → 📥 Client-Log → JSONL für rohe AGL-Samples + Activity-Log
5. Patch in `lib.rs` + Test-Cases mit gespeicherten JSONLs validieren

Vollständige Algorithmus-Referenz: [`docs/client-log-format.md`](docs/client-log-format.md).

### 📊 Server-Side (aeroacars-live, parallel deployed)

- DB-Schema: 3 neue Spalten in `flight_sessions` (client_log_path/size/uploaded_at) + 7 neue Spalten in `touchdowns` (simulator + 6 Forensik-Felder)
- Migration v1: Backfill aircraft_icao aus flights-Tabelle für historische Sessions
- Sustainable Session-Splitter mit drei orthogonalen Detektoren: Metadata-Mismatch / PIREP-Terminator / Phase-Regression — verhindert „Hin+Rück landet in einer Session"-Bug
- Defense-in-depth: Server-seitiger `sanitizeStr` im flights-UPSERT als Fallback für alte Clients ohne v0.5.23-Fix

### ⚠ Hinweise zum Update

- Backward-kompatibel: alte Server (ohne neue Forensik-Felder) ignorieren die neuen Optional-Payload-Felder. Neue Server (= aeroacars-live ab heute) extrahieren typed.
- Pilot-PCs ohne v0.5.23 schicken weiter PositionPayload mit `""` als Empty-Marker — Server-Defense fängt das ab.
- Bestehende Sessions in der DB bleiben unverändert (Migration v1 fixt nur was sicher fixbar ist).

---

## [v0.5.13] — 2026-05-07

🎯 **X-Plane Touchdown jetzt bit-genau LandingRate-1-aligned (Lua adaptive 30-sample method).**

Pilot-Bericht 2026-05-07: X-Plane-Flug MYNN→MBGT auf v0.5.11/v0.5.12 zeigte -394 fpm Touchdown — LandingRate-1.lua-Tool im selben Sim sagte 273 fpm. ~44% zu hoch.

**Ursache:** Mein bisheriger Time-Tier-Estimator (750ms / 1s / 1.5s / 2s / 3s / 12s mit fixen Min-Sample-Counts) ist zu starr. Bei niedriger X-Plane-RREF-Rate fallen kurze Tiers wegen Sample-Underflow durch, längere Tiers gewinnen — und ziehen Pre-Flare-Sinkraten in die Touchdown-Berechnung mit rein.

**Lua's Methode** (LandingRate-1, Dan Berry 2014+) macht's anders:
```lua
new_table("lrl_agl", 30)  -- 30 Samples, NICHT 1 Sekunde fix
```
Adaptives Fenster — bei 60 fps Render = ~0.5s, bei 30 fps = ~1s, bei 10 fps = ~3s. Selbstkalibrierend, robust gegen Framerate-Schwankungen.

### 🐛 Behoben

**X-Plane Touchdown-Capture komplett auf Lua-Style umgestellt.**

| Sim | Algorithmus | Datei |
|---|---|---|
| **X-Plane** (NEU) | Lua-style 30-sample adaptive AGL-Δ | `lib.rs` + `plugin.cpp` |
| MSFS (UNVERÄNDERT) | Time-tier estimator als Fallback nach latched SimVar | `lib.rs` |

**Schlüssel-Änderungen für X-Plane:**
- Neue Funktion `estimate_xplane_touchdown_vs_lua_style` — nimmt die letzten 30 Samples aus dem Sampler-Buffer, berechnet AGL-Avg-Midpoint-Rate exakt wie Lua's `lrl_agl` table
- Plugin (`plugin.cpp`): Time-Tier-Loop entfernt, durch 30-Sample-Method ersetzt
- AGL-Guards bleiben unverändert (TD ≤ 5 ft / on_ground=true, Window-Start ≤ 250 ft)
- Plugin sendet weiter `captured_vs_source` Diagnose-Metadata, jetzt `"lua_30_sample"`

### MSFS unverändert

**Nichts MSFS-relevantes wurde angefasst.** v0.5.12-Behavior für MSFS bleibt 1:1:
1. Latched SimVar (PLANE TOUCHDOWN NORMAL VELOCITY) — primary, GEES-aligned
2. Time-tier AGL-Δ — fallback (separate Funktion `estimate_xplane_touchdown_vs_from_agl` bleibt erhalten)
3. Buffer-Min — last resort

**Sampler bleibt für MSFS explizit aus** (war v0.5.12-Fix gegen Spike-Contamination).

### Validation

| Flug | Sim | v0.5.12 | v0.5.13 (erwartet) |
|---|---|---|---|
| MYNN→MBGT (Pilot) | X-Plane 12 | -394 fpm | ~-273 fpm (matcht LandingRate-1.lua) |
| 11 Flüge (Pete + Michael) | MSFS | korrekt seit v0.5.12 | unverändert korrekt |

### 🛠 Intern

- Tests: 87 grün
- `agl_estimate_xp` (Lua-style) und `agl_estimate_msfs` (time-tier) koexistieren als getrennte Funktionen
- Plugin baut clean auf Win/Mac/Linux via CI
- Frontend: 0 Änderungen

---

## [v0.5.12] — 2026-05-07

🚨 **KRITISCHER MSFS-Hotfix — Touchdown-Capture wieder GEES-aligned wie pre-v0.5.x.**

Pilot-Bericht: MSFS-Flug Lufthansa LH595 DNAA→EDDF zeigte -1173 fpm Touchdown bei G 1.12 — physikalisch widersprüchlich. Volanta + LHA-Tools sagten -560 fpm, MSFS-internal latched SimVar -419 fpm. Plus 11 weitere MSFS-Flüge analysiert (Pilot „Pete"): bei 90% war die latched SimVar `null`, Werte kamen aus Fallback-Pfaden — manche kontaminiert durch Sampler-Spike-Artefakte.

### 🐛 Behoben

**Bug-Klasse:** v0.5.0+ hat den X-Plane-Style Sampler (`sampler_touchdown_vs_fpm` via fnrml_gear bzw. on_ground-Edge-Fallback) in `step_flight()` auch für MSFS-Flüge einreihen lassen. MSFS hatte vorher (v0.3.5–v0.4.3) eine saubere zweistufige Logik: `latched MSFS SimVar → buffer-min`. Mit v0.5.0 schob sich der Sampler **vor** den latched-Wert in der Priority-Chain — und bei MSFS-Touchdown-Frames liefert der Sampler oft eine Spike-Reading durch Gear-Contact-Rebound-Oszillation.

**Fix — Sim-aware Capture-Trennung:**

```
MSFS-Pfad:
  1. snap.touchdown_vs_fpm  ← MSFS-latched SimVar (PLANE TOUCHDOWN
                              NORMAL VELOCITY — frame-genau, vom Sim
                              selbst gemessen, GEES-aligned)
  2. AGL-Δ Estimator        ← Geometrische Wahrheit als Fallback
                              für die ~90% der Flüge wo MSFS die
                              latched SimVar nicht setzt
  3. Buffer-Min (AGL≤250)   ← Last-resort

X-Plane-Pfad (unverändert seit v0.5.11):
  1. AGL-Δ Estimator (LandingRate-1)
  2. sampler_touchdown_vs_fpm (fnrml_gear)
  3. Buffer-Min
  4. low_agl_vs_min_fpm
```

**Schlüssel-Änderungen:**

- **Sampler-Pfad explizit AUS für MSFS** — der Sampler-Capture wird bei `is_msfs == true` gar nicht mehr konsultiert
- **AGL-Guard relaxed:** Touchdown-Sample wird akzeptiert wenn `on_ground=true` ODER `AGL ≤ 5 ft` (vorher nur strict AGL≤5). MSFS reportet AGL ≈ 9-14 ft auch bei `on_ground=true` — sim-quirk, nicht pre-touchdown
- **`negative_only` Filter** auf alle Quellen — physikalisch unmögliche positive „Landing-Rates" werden geblockt

**Validation:**

| Flug | Pilot | v0.5.11 (kaputt) | v0.5.12 (Fix) |
|---|---|---|---|
| LH595 DNAA→EDDF (B738) | Michael | -1173 fpm phantom | ~-419 fpm (matcht MSFS-internal) |
| 11 MSFS-Flüge (EDDF-Routen) | Pete | -132 bis -346 (zufällig OK) | konsistent über AGL-Δ-Pfad |
| Pre-v0.5.x Verhalten | (jede Pilot) | n/a | **wiederhergestellt + besser** |

### 🛠 Intern

- `step_flight()` enthält jetzt `match snap.simulator { ... }`-Branch
- AGL-Δ Estimator akzeptiert MSFS-AGL-Quirk (on_ground=true override)
- 87 Tests grün (alle 5 X-Plane-Touchdown-Regression-Tests bleiben gültig)
- Wirkt für alle MSFS-Versionen (Msfs2020 + Msfs2024)

---

## [v0.5.11] — 2026-05-07

🚀 **Großes Release — drei zusammenhängende Themen:**
1. **FSM-Audit** für alle Flugzeug-Klassen (Airliner / GA / Heli / Glider / Seaplane) inkl. Touch-and-Go, Go-Around, Holding-Pattern, Pause/Slew-Robustheit
2. **X-Plane Touchdown-Erfassung neu architektiert** nach LandingRate-1-Methode (AGL-Δ statt VSI), Plugin entmachtet
3. **MQTT Live-Tracking** zur aeroacars-live VPS — komplett unsichtbar im Hintergrund

87 Tests grün. Frontend ohne Änderungen.

---

### 🛩 Teil 1: FSM-Phasen-Audit (alle Aircraft-Klassen)

Pilot-Frage: „können wir alle Flugphasen für GA / Airliner / Heli prüfen?" — ja. v0.5.11 ist das Ergebnis einer vollständigen FSM-Audit mit Tiefen-Analyse der False-Positive-Risiken **bevor** gepusht wurde.

**🚁 Helikopter vertikaler Start aus Taxi**
TaxiOut→TakeoffRoll erwartet GS>30 kt am Boden, Helis erreichen das nie. Vorher: FSM hängt für ganzen Flug in TaxiOut.
→ Fix: TaxiOut → Takeoff direkt wenn `on_ground` true→false + AGL>5 ft + VS>100 fpm (Hardening gegen on_ground-Flicker).

**🚁 Helikopter pure-hover Departure aus Boarding**
Heli die direkt vom Gate vertikal abheben gehen nie auf TaxiOut → stuck in Boarding.
→ Fix: Boarding → Takeoff direkt + AGL>3 ft + VS>100 fpm.

**✈ Glider (Tow + Winch)**
engines>0 Anforderung in Heli-Pfaden gedroppt → Glider-Tow funktioniert (Glider ist airborne mit GS>0 aber engines=0).

**🛟 Seaplane Wasser-Operationen**
Boarding→TaxiOut akzeptiert jetzt Wasser-Oberfläche (`AGL<5 + |VS|<50` ≈ ground-equivalent). TaxiOut→Takeoff Catchall für Seaplanes wo on_ground=false bleibt: `!on_ground + AGL>50 + VS>100 + !slew + !paused`.

**🛩 GA Niedrigflug-Sackgasse**
Cessna mit Cruise auf 3000 ft AGL erreichte vorher nie Climb→Descent (braucht VS<-500). Climb→Descent triggert jetzt in DREI Szenarien:
- Standard TOD (Airliner): vs<-500 + lost>200 ft
- Low-altitude approach: vs<-100 + AGL<3000 + lost>500 ft
- Near-ground catchall: AGL<2000 + lost>800 ft + vs<0

**🔄 Touch-and-Go + Go-Around: climb_peak_msl Reset**
Beide Handler springen zurück zu Climb, aber der climb_peak_msl-Tracker wurde vorher nur bei Takeoff→Climb zurückgesetzt → Stale-Peak nach T&G/GA hätte mein neuen Low-Altitude-Trigger fälschlich feuern lassen.

**⏸ Pause + Slew Guard**
Während sim-pause oder slew-mode friert die FSM-Logik ein (kein Phasenwechsel), aber Position-Recording, Distanz-Tracking, Heartbeat laufen weiter. Verhindert dass eingefrorene snapshots Holding-Detektor-Timer fälschlich ablaufen lassen.

**🎯 NEUE Phase: Holding**
ICAO-konforme Holding-Pattern-Erkennung (sustained turn 90s + level flight). Triggert aus Cruise (high-altitude hold) oder Approach (low-altitude approach hold). Exit über bank<5° für 30s ODER aktiver Sinkflug → Approach.

**Audit-Endergebnis:**

| Aircraft | Vorher | Nach v0.5.11 |
|---|---|---|
| Airliner FL340 | ✅ | ✅ unverändert |
| Cessna 172 @ 3000 ft | ❌ stuck in Climb | ✅ alle Phasen |
| Bell 407 vertikal | ❌ stuck in TaxiOut | ✅ alle Phasen |
| EC135 pure-hover | ❌ stuck in Boarding | ✅ alle Phasen |
| Glider Aerotow / Winch | ❌ engines>0 lockt aus | ✅ alle Phasen |
| Seaplane (Wasser) | ❌ stuck in Boarding | ✅ alle Phasen |
| Pattern + Touch-and-Go | ⚠️ stuck nach 2. Anflug | ✅ Multi-T&G stabil |
| Missed Approach + GA | ⚠️ 2. Approach instabil | ✅ stabil |
| ATC Holding-Pattern | (nicht erkannt) | ✅ neue HOLDING-Phase |

**⚠️ Verworfen aus pre-release v0.5.10:** Der dortige Climb→Cruise low-altitude-Pfad (vs.abs()<100 + lost.abs()<100) wäre während aktivem Climb fälschlich gefeuert (lost-from-peak ist immer ~0 beim aktiven Climb). Komplett rausgenommen — GA bleibt in Climb bis Descent.

---

### 🎯 Teil 2: X-Plane Touchdown-Erfassung — Architektur-Refactor

Pilot-Analyse 2026-05-07: „warum kriegen LandingRate.lua und Volanta plausible Werte und wir nicht? Plus: most-negative-anywhere-in-approach kann pre-flare-Sinkraten als Touchdown ausgeben."

**Bug-Klasse:** v0.5.5+ trackte den negativsten VS-Wert über den GANZEN Approach. Ein steiler Pre-Flare-Sinkflug bei 943 ft AGL (z.B. -1346 fpm) hätte den echten gentle Touchdown bei 0 ft AGL überschrieben → Phantom-Hard-Landing-Reports.

**Fix — neue clean Hierarchie für Touchdown-VS-Erfassung:**

1. **PRIMÄR: AGL-Δ Estimator** mit Window-Tiers (750 ms / 1 s / 1.5 s / 2 s / 3 s / 12 s sparse-fallback)
   - LandingRate-1-Algorithmus (etabliert seit ~2014, gleiche Methode wie Volanta)
   - **Strikte Guards:** AGL ≤ 5 ft am Touchdown-Frame, AGL ≤ 250 ft am Window-Start
   - Pre-flare-Höhen können physisch nicht in die Berechnung kommen
2. Sampler-Edge-Capture (negative_only filtered)
3. MSFS-latched Touchdown-SimVar (negative_only)
4. Tighter buffer-window-scan + AGL≤250 Filter (negative_only)
5. `low_agl_vs_min_fpm` (umbenannt von `approach_vs_min_fpm`, jetzt nur AGL≤250 trackend)

**`negative_only` Filter:** alle Fallback-Quellen werden gefiltert — eine positive Landing-Rate ist physikalisch unmöglich.

**Plugin entmachtet:**
- Plugin-Code spiegelt gleichen Algorithmus + AGL≤250-Limit
- Plugin-Buffer hat 128 Samples (~3.8 s history)
- Plugin sendet Diagnose-Metadaten (`captured_vs_source`, `captured_vs_window_ms`, `captured_vs_samples`) im Touchdown-Paket
- Plugin liefert weiterhin `captured_vs_fpm` aber Client kann mit eigener AGL-Estimate **überschreiben** wenn er bessere Samples hat
- Plugin-Reinstall **nicht zwingend** — alte Plugin-Versionen werden durch Client-Logik korrekt gefiltert

**5 Regression-Tests** (alle grün): rebound-VSI / pre-flare-spike / butter-landing / all-positive-VS / negative_only-Filter.

---

### 📡 Teil 3: MQTT Live-Tracking zur aeroacars-live VPS

**NEUE Crate** `client/src-tauri/crates/aeroacars-mqtt/` integriert. Komplett unsichtbares Hintergrund-Feature (KEINE UI, KEIN Settings-Tab, KEIN Toggle).

**Auto-Provisioning** beim Login:
- Client ruft `https://live.kant.ovh/api/provision` mit phpVMS-API-Key auf
- Server validiert API-Key gegen phpVMS-Backend, liefert MQTT-Credentials zurück
- Credentials werden im OS-Keyring gecacht — Re-Install = same credentials (idempotent)
- Logout flusht Cache + sauberer Shutdown

**5 Hook-Points im Streamer:**
- **Position** (high-frequency, retained, QoS 0) — bei jedem position-tick
- **Phase** (low-frequency, retained, QoS 1) — bei FSM-Phasenwechsel inkl. neue HOLDING-Phase
- **Touchdown** (event, QoS 1) — wenn `announce_landing_score` ein Score-Message generiert
- **PIREP** (event, QoS 1) — nach `file_pirep` success
- **Shutdown** (clean OFFLINE flush mit 200ms-Pause) — auf RunEvent::ExitRequested

**Sicherheitseigenschaften:**
- MQTT-Connect über `wss://live.kant.ovh/mqtt` (TLS via rustls, kein OpenSSL-dep)
- `try_send` mit bounded queue → broker stall kann NIE den Streamer hot-path blocken
- Provision-Failure ist non-fatal — AeroACARS funktioniert exakt wie ohne MQTT
- LWT (last-will-and-testament) sorgt dafür dass beim Crash der OFFLINE-Status kommt
- Topic-ACL: jeder Pilot kann nur in `aeroacars/<va>/<seine-id>/#` publishen

**Wichtig für VA-Admins:** der Server-seitige Monitor-Frontend muss noch um die neue `HOLDING`-Phase erweitert werden — bis dahin fällt das Frontend auf den raw-String zurück (kein Funktionsverlust, nur Cosmetics).

---

### 🛠 Intern

- Tests: 87 grün (82 vorher + 5 neue Regression-Tests)
- Backend kompiliert cross-platform clean
- Plugin baut auf Windows (Mac/Linux via CI)
- Frontend: 0 Änderungen
- pre-release v0.5.10 wurde verworfen (Climb→Cruise alt-path zu riskant, T&G/GA-Reset fehlte)

---

## [v0.5.9] — 2026-05-07

🩹 **Climb→Descent FSM-Bug: ein einzelner VS-Spike beendete den Steigflug.**

Pilot Michael (MSFS, EGPH→HEGN B738): bei Climb auf FL050 hat ein einzelner -742 fpm-Spike (Level-Off-Maneuver) die FSM auf Descent geflippt. Aircraft stieg weiter durch FL340 und cruiste, aber FSM blieb 50+ Min in Descent hängen weil es keinen Descent→Climb Rücktransitionspfad gibt.

### 🐛 Behoben

Climb→Descent verlangt jetzt **zusätzlich** dass das Aircraft **echten Höhenverlust** vom Climb-Peak hat (>200 ft MSL).

```
Vorher: vs < -500 fpm                                    → Descent
Jetzt:  vs < -500 fpm AND lost_from_climb_peak > 200 ft → Descent
```

Single-Sample-Spikes (Turbulenz, Auto-Pilot-Trim, ATC-Level-Off) werden gefiltert. Erst wenn das Aircraft tatsächlich >200 ft Höhe verliert, gilt's als Descent. Echter Top-of-Descent verliert sofort tausende Fuß → triggert problemlos.

### 🛠 Intern
- Neues Feld `climb_peak_msl` in FlightStats (persistiert)
- Reset bei Takeoff→Climb (Re-Takeoff nach Divert)
- Wirkt für **MSFS und X-Plane** (FSM ist sim-agnostisch)
- Tests: 82 grün

---

## [v0.5.8] — 2026-05-07

🎯 **Multi-Window AGL-Δ + Plugin-Update — komplette Algorithmus-Konvergenz mit Volanta-Niveau.**

Pilot-Hinweis: „Volanta nutzt kein Plugin mehr und kriegt trotzdem korrekte Werte." Bestätigt unsere Strategie — der AGL-Δ-Algorithmus aus v0.5.7 ist self-sufficient ohne Plugin. v0.5.8 robustifiziert ihn weiter.

### 🆕 Multi-Window AGL-Derivative

Statt nur 2 s evaluiert der Client/Plugin jetzt **drei Fenster gleichzeitig** (1 s, 2 s, 3 s) und nimmt das negativste:
- **Hard Landing** (kein Flare): alle drei Fenster geben gleiche Werte
- **Airliner-Standard-Flare** (~3 s): 2 s-Fenster fängt den Pre-Flare-Sinkflug
- **GA Long-Flare** (~5 s): 3 s-Fenster deckt den relevanten Slice ab
- **Floater** (lange flache Approach): 1 s-Fenster misst nur die letzten Sekunden = sanfte Butter-Rate

### 🆕 Plugin (v0.5.8) — gleiche Methode

Plugin's Ring-Buffer hat jetzt auch AGL-Werte (war vorher nur VS+Pitch). Multi-Window-AGL-Δ läuft im Plugin self-sufficient. Kombiniert mit running airborne-VS-min als Backup.

**Aber wichtig:** Plugin ist optional. Volanta beweist dass die UDP-RREF-Daten von X-Plane (Port 49000) reichen — der Algorithmus macht den Unterschied, nicht der Plugin.

### 🛠 Intern
- Client: drei parallele AGL-Fenster, most-negative wins
- Plugin: VS-Buffer von 64 → 128 Samples (~3.8 s history bei 30 fps)
- Tests: 82 grün

---

## [v0.5.7] — 2026-05-07

🎯 **Methoden-Wechsel: VS wird jetzt aus AGL-Δ berechnet (LandingRate-1-Algorithmus, seit ~10 Jahren in der X-Plane-Welt erprobt).**

Pilot-Frage „warum kommen LandingRate.lua und Volanta immer auf richtige Werte und wir nicht?" — weil die einen fundamental anderen Ansatz nutzen den wir bisher nicht hatten.

### 🐛 Behoben

**Vorher** lasen wir die Sinkrate direkt aus `local_vy` / `vh_ind_fpm` (Flight-Model-Output). Beim Flare reduziert das Flight-Model die VSI absichtlich auf nahe 0 für gutes Stick-Feel — der Flieger sinkt physikalisch noch weiter, aber die VSI-Anzeige lügt schon. Egal wie clever wir Buffer-Min-Suche oder Running-Min nutzen, die Quelldaten sind kompromittiert.

**Jetzt** nutzen wir denselben Algorithmus wie LandingRate-1.lua (Dan Berry, 2014+) und Volanta:

```
gVS = (current_AGL - avg_AGL_letzte_2s) / (Zeitspanne / 2) * 60
```

Statt VSI lesen wir die **tatsächliche AGL-Differenz** über ein 2-Sekunden-Fenster. Das ist reine Geometrie — die Geometrie kann nicht durch Flight-Model-Tricks verfälscht werden. Bei einem Anflug von 81 ft AGL → 0 ft in 2 Sekunden gibt das exakt den echten Sinkflug, unabhängig von dem was VSI behauptet.

**Most-negative-wins** Hierarchie beim Final → Landing:
1. **AGL-Differential** (PRIMÄR — geometrische Wahrheit, wenn Sample-Density ausreicht)
2. Running Approach-Min (v0.5.5 Fallback)
3. Sampler-Edge-Capture (v0.4.4 Edge-Detection)
4. Buffer-Window-Scan (Legacy)
5. Live snap.vs (Last resort)

### 🛠 Intern
- Tests: 82 grün
- AGL-Daten waren schon im snapshot_buffer, kein neues Tracking nötig
- Wirkt mit ODER ohne Plugin (rein client-seitig)
- Plugin-Algorithmus folgt in v0.5.8 (gleicher Ansatz im C++)

---

## [v0.5.6] — 2026-05-06

🩹 **Plugin-Pendant zur v0.5.5-Touchdown-Logik.**

v0.5.5 hat den Bug im Tauri-Client gefixt; v0.5.6 fixt jetzt auch den Plugin-Code damit beide Schichten konsistent korrekt sind. Plugin sendet jetzt von sich aus den richtigen Wert.

### 🐛 Behoben

Plugin trackt jetzt auch eine **`g_airborne_vs_min`** — den negativsten pitch-korrigierten VS-Wert über den GESAMTEN airborne Segment (ground→air bis air→ground). Beim Touchdown-Edge wird der Wert mit dem Lookback-Window-Min und dem Live-VS verglichen — most-negative wins.

Zusammen mit der v0.5.5-Client-Logik gibt es jetzt **doppelte Korrektheit**:
- Plugin liefert von sich aus richtige `captured_vs_fpm` aus dem ganzen Anflug
- Client überschreibt nochmal mit dem eigenen Tracker falls Plugin doch falsch liegt

Reset-Logik im Plugin:
- Bei jedem ground→air Edge (Takeoff, Go-Around-Lift-off): Tracker = 0
- Nach erfolgreichem Touchdown-Capture: Tracker = 0 (Touch-and-Go bereit)
- Bei Plugin-Reload (`XPluginStop`): Tracker = 0

### ⚠️ Pilot-Aktion

1. v0.5.6 Auto-Update annehmen (Tauri-Client)
2. Settings → Debug → **„Plugin installieren"** klicken (lädt v0.5.6-Plugin)
3. **X-Plane neu starten** — neuer Plugin lädt erst beim X-Plane-Start

Dann ist das Plugin self-sufficient korrekt, auch ohne Client-Tracker-Override.

---

## [v0.5.5] — 2026-05-06

🩹 **Hotfix: Touchdown-VS bei aggressivem Flare wird endlich richtig erfasst.**

Pilot-Test (B738, MWCR Pattern, score 60/100 „firm" mit absurden Werten **VS +57 fpm bei G 1.52**): die Werte sind physikalisch widersprüchlich — 57 fpm = unmerklich, G 1.52 = harte Landung. Echte Sinkrate war ca. -500 fpm während des Anflugs (sichtbar im JSONL bei AGL 81 ft).

### 🐛 Behoben

Der 50-Hz-Sampler hatte ein zu schmales Lookback-Fenster (500 ms) und konnte bei aggressivem Flare nur **Post-Touchdown-Rebound-Samples** im Buffer finden — alle mit positivem VS. Resultat: das Min-Search fand keinen Sinkflug, gab den Rebound-Wert zurück.

**Doppelte Verteidigung in v0.5.5:**

1. **Running Peak-Descent-Tracker (`approach_vs_min_fpm`).** Ab Approach-Entry wird jeden 20-ms-Tick der **kleinste pitch-korrigierte VS-Wert** über die gesamte Approach + Final-Phase getrackt — unabhängig vom Sampler-Buffer. Selbst wenn X-Plane nur 1-2 Hz RREF liefert, fängt das den echten Peak-Sinkflug ein. Reset bei jedem neuen Approach (Go-Around-sicher).

2. **Sampler-Lookback erweitert von 500 ms auf 2 s.** Belt-and-suspenders gegen Buffer-Race-Bedingungen bei niedrigen RREF-Raten.

Beim Final → Landing wird nun der **negativste der drei Werte** genommen: Sampler-Edge-Capture vs. Buffer-Window-Scan vs. Running-Approach-Min. Most-negative wins.

### 🛠 Intern
- Tests: 82 grün
- Patch wirkt **mit oder ohne** installiertes X-Plane-Premium-Plugin — Plugin gibt frame-genaue Werte direkt vom flight-loop, der Tracker ist Backup für Plugin-lose Setups
- Persistierung des Trackers nicht nötig — er lebt nur innerhalb einer einzigen Approach-Phase

---

## [v0.5.4] — 2026-05-06

🩹 **Hotfix: Pattern-Flüge auf niedriger Höhe bleiben in Cruise hängen.**

Pilot-JSONL-Log: kurzer MWCR → MWCR Pattern-Test (B738), Cruise-Höhe 5000 ft AGL, 16 Min Flugdauer, normale Landung mit Aufsetzen — Ergebnis: keine Landing-Rate erfasst, Phase ging direkt von Cruise → Arrived.

### 🐛 Behoben

**Bug 1: Cruise→Descent forderte > 5000 ft Höhenverlust.** Der Cruise-Peak war bei 5002 ft MSL, beim Aufsetzen MSL 29 ft → Höhenverlust 4973 ft, **knapp unter** der 5000-ft-Schwelle. FSM blieb in Cruise, der Universal-Arrived-Fallback hat dann am Ende stumm direkt nach Arrived gesprungen — ohne durch Final→Landing zu gehen, also keine Touchdown-Erfassung.

Fix: Eskape-Klausel — Cruise→Descent triggert jetzt entweder bei (a) > 5000 ft Höhenverlust (wie bisher, für Airliner-TOD) **oder** (b) AGL < 3000 ft + Sinkflug (Pattern/GA-Bereich). Step-Downs bei FL360 lösen weiterhin keinen falschen Phasenwechsel aus.

**Bug 2: Universal-Arrived-Fallback verlor Touchdown-Daten.** Selbst wenn der 50-Hz-Sampler den Edge intern erfasst hatte, wurden VS/G nicht in den PIREP geschrieben weil der Code-Pfad „Final→Landing" der einzige war der das tat.

Fix: Rescue-Pfad — wenn Arrived-Fallback feuert UND der Sampler einen Touchdown gespeichert hat, werden `landing_rate_fpm`, `landing_peak_vs_fpm`, `landing_g_force`, `landing_peak_g_force` aus den Sampler-Werten gefüllt. Zweite Verteidigungslinie selbst wenn die FSM-Hauptkette ausfällt.

### 🛠 Intern
- Tests: 82 grün
- Beide Fixes wirken auch ohne installiertes X-Plane-Premium-Plugin (Sampler ist nativer Teil des Tauri-Clients)

---

## [v0.5.3] — 2026-05-06

🚨 **KRITISCHER Hotfix — Port-Konflikt mit X-Plane behoben.**

Pilot-Bericht mit Screenshot der X-Plane-Netzwerkeinstellungen zeigte: „Fehler bei der Initialisierung des UDP-Netzwerkausgangs (Port 49001). Lokales Netzwerk wird deaktiviert." Mein Plugin hatte 49001 für die Loopback-Kommunikation gewählt — **das ist aber X-Planes eigener Sende-Port**. Beide Apps stritten um denselben Socket → X-Plane konnte sein UDP-Netzwerk nicht initialisieren.

### 🐛 Behoben

- **Port von 49001 → 52000** in Plugin (`AEROACARS_UDP_PORT`) und Client (`PREMIUM_UDP_PORT`). 52000 ist:
  - **Weit außerhalb** X-Planes 49000-49003 Bereich (Send/Receive)
  - **Nicht** der X-Plane-Connect-Port (49520, NASA-Research-Tool)
  - In IANA Dynamic-Range, kein bekannter Service
  - Komplett konfliktfrei für 99,9% der Setups

### ⚠️ Pilot-Aktion erforderlich

1. AeroACARS-Update auf v0.5.3 installieren (auto-update)
2. Settings → Debug → Plugin **neu installieren** (lädt v0.5.3-Plugin von GitHub)
3. **X-Plane neu starten** — die Fehlermeldung über deaktiviertes lokales Netzwerk verschwindet, X-Planes UDP-Netzwerk arbeitet wieder normal

Plugin- und Client-Port müssen synchron sein — die v0.5.3-Auto-Install-Funktion zieht automatisch das passende Plugin-ZIP, daher reicht ein Klick auf „Plugin installieren" nach dem Client-Update.

### 🛠 Intern

- Neuer Defensive-Comment-Block in beiden Source-of-Truth-Konstanten warnt explizit vor X-Planes 49000-49003 Range
- Tests: 82 grün (unverändert)
- Plugin-Source ist nur an einer Konstante geändert, alle anderen Logiken stabil

---

## [v0.5.2] — 2026-05-06

🩹 **Hotfix: kein flackerndes Konsolen-Fenster mehr beim Settings-Tab-Klick.**

Pilot-Feedback nach v0.5.1: „beim Klick auf den Tab Einstellungen öffnet sich ein unsichtbares Fenster". Das war eine echte (leere) `cmd.exe`-Konsole, die kurz aufflackerte und den Fokus stahl — verursacht durch den `reg.exe query` aus der X-Plane-Pfad-Auto-Erkennung.

### 🐛 Behoben
- **`CREATE_NO_WINDOW`-Flag** für den `reg.exe`-Subprocess. Windows zeigt jetzt keine Konsole mehr an, kein Fokus-Stehlen, kein Flackern.

Patch nur Windows-relevant. Mac/Linux unverändert.

---

## [v0.5.1] — 2026-05-06

🩹 **Hotfix für v0.5.0-Regression — Settings-Tab hängt beim ersten Öffnen.**

Pilot-Feedback nach v0.5.0-Install: „Einstellungsseite ist hakelig beim Scrollen, Sprache konnte erst nicht verstellt werden." Klassischer Synchronization-Bug — der neue X-Plane-Premium-Panel rief auf seinem ersten Render einen synchronen Tauri-Command (`xplane_detect_install_path`) auf, der intern `reg.exe query` als Subprocess startete. Auf dem Main-Thread = blockiert den ganzen IPC-Kanal für ~200-800 ms, während dem **kein einziger anderer Command** durchkommt — daher Sprachwechsel-Hang + Scroll-Lag.

### 🐛 Behoben

- **`xplane_detect_install_path` ist jetzt async + `spawn_blocking`** — der `reg.exe`-Query läuft auf einem Worker-Thread, IPC bleibt frei, Settings-Panel reagiert sofort.
- **`xplane_uninstall_plugin` ebenfalls async** — beugt potenziellem Stall bei langsamen `remove_dir_all` (Windows Defender, Netzlaufwerke) vor.

### 🛠 Intern

- Selbe Pattern wie `detect_running_sim` (das schon seit v0.3.0 async ist).
- Tests: 82 grün (unverändert).

---

## [v0.5.0] — 2026-05-06

🚀 **„X-Plane Premium" — Frame-genaue Touchdown-Erfassung via nativem Plugin.**

Größtes Feature seit Release: ein optionaler nativer X-Plane-Plugin (XPLM SDK 4.3.0, C++17), der die Touchdown-Edge **innerhalb** des X-Plane-Flight-Loops erfasst — frame-genau, mit 500 ms Lookback-Buffer für die Peak-Sinkrate. Löst endgültig die seit v0.4.2 jagende „6 fpm Landing Rate trotz harter Landung"-Klasse von Bugs.

### 🆕 X-Plane Premium Plugin

**Was es tut:**
- Liest `fnrml_gear` (Gear-Normalkraft) jeden Frame und erkennt den exakten Frame des Aufsetzens (xgs-Methode, etablierte X-Plane-Konvention seit ~10 Jahren).
- Ermittelt die Peak-Sinkrate aus einem 500 ms-Lookback-Ring-Buffer **vor** dem Edge — so dass das gemessene VS dem tatsächlichen Anflug entspricht, nicht dem schon ausgependelten Wert nach Bodenkontakt.
- Pitch-Korrektur: `vs × cos(pitch)` (xgs-Konvention) — projiziert Welt-Y-Geschwindigkeit auf die Body-Achse.
- Sendet einen einmaligen JSON-„touchdown"-Paket über UDP an die AeroACARS-App auf `127.0.0.1:49001`.
- Re-armiert sich bei AGL > 50 ft, Touch-and-Go funktioniert also korrekt.

**Cross-Platform:**
- Windows x64 (`win.xpl`, MSVC, statisches CRT — keine DLL-Abhängigkeiten beim Piloten)
- macOS Universal (`mac.xpl`, x86_64 + arm64 in einer Datei)
- Linux x64 (`lin.xpl`, GCC)

**Sicherheit (NIE den Sim crashen):**
- Alle DataRef-Handles NULL-geprüft, alle Errors via `XPLMDebugString` geloggt, nie propagiert.
- Compile mit `-fno-exceptions -fno-rtti` (keine C++-Exceptions über die C-ABI-Plugin-Boundary).
- Non-blocking UDP `sendto()` — kein Stallen des Flight-Loops, auch nicht wenn der Client offline ist.
- Keinerlei Filesystem-Writes, keine Registry-Edits — Plugin ist read-only gegen X-Plane-State.
- Sauberes Reverse-Order-Cleanup in `XPluginStop`.

**Wire Format:** Versionierte Line-delimited-JSON über UDP-Loopback. Schema-`v:1`, zwei Pakettypen: `telemetry` (jeden Tick) + `touchdown` (one-shot pro Landung).

### 🆕 Auto-Install im AeroACARS-Client

Settings → Debug → „X-Plane Premium Plugin"-Karte:
- **Auto-Erkennung** des X-Plane-Hauptordners (Windows-Registry · macOS Standard-Pfade · Linux Standard-Pfade)
- **Manueller Pfad-Override** wenn die Auto-Erkennung nichts findet
- **„Plugin installieren"-Button** lädt die zur installierten Client-Version passende Plugin-Zip von GitHub und entpackt nach `<X-Plane>/Resources/plugins/AeroACARS/`
- **Status-Badge** „📡 live" sobald das Plugin Pakete sendet

### 🆕 Listener im Tauri-Client

- Neuer UDP-Listener (`crates/sim-xplane/src/premium.rs`) bindet `127.0.0.1:49001`, parst JSON-Pakete, surft Status + Touchdown-Events nach lib.rs.
- Touchdown-Sampler: wenn ein Premium-Paket eintrifft, **überschreibt** dessen `captured_vs_fpm` / `captured_g_normal` die RREF-basierte Edge-Detection — Frame-Genauigkeit, kein UDP-Eviction-Race mehr.
- RREF-Pfad bleibt voll funktional: Piloten ohne Plugin merken keinen Unterschied, ihre Flüge laufen wie vorher.

### 🛠 Intern

- Neuer Workspace-Member `xplane-plugin/` mit Cross-Platform-CMake-Build
- X-Plane SDK 4.3.0 vendored unter `xplane-plugin/third_party/XPSDK430/` (BSD-Lizenz, freie Commercial-Use)
- 6 neue Unit-Tests für den Premium-Packet-Parser
- 3 neue Tauri-Commands: `xplane_premium_status`, `xplane_detect_install_path`, `xplane_install_plugin`, `xplane_uninstall_plugin`
- GitHub-Actions-Pipeline erweitert: Plugin-Build-Matrix (Win/Mac/Linux) + Plugin-Package-Job, der die drei `.xpl` zu `AeroACARS-XPlane-Plugin-vX.Y.Z.zip` zusammenfasst und ans Release uploaded
- Bilingual i18n (DE+EN) für alle neuen Strings

### 🐛 Behoben (X-Plane only)

- **Landing-Rate-Bug aus v0.4.2/v0.4.3 final beseitigt:** Sampler-side Edge-Detection auf `fnrml_gear` (statt nur Streamer-side `on_ground`-Flag). Funktioniert sowohl mit als auch ohne Premium-Plugin — ohne Plugin macht der Sampler die Edge-Detection auf seinen 50-Hz-Snapshots, mit Plugin übernimmt das Plugin frame-genau.
- **Pitch-Korrektur bei VS-Capture:** Konsistent mit xgs (`vs × cos(theta_rad)`) im Sampler und im Plugin.

---

## [v0.4.3] — 2026-05-05

X-Plane-spezifischer Touchdown-VS-Fix nach Pilot-Live-Test heute Abend.

### 🐛 Behoben (X-Plane only)
- **Landing-Rate / peak_vs_fpm war bei X-Plane immer ~0** auch bei klar härteren Landungen. Pilot-Log heute (EWL6822 LEPA→EDDG, A320, sichtbare Sinkrate -350 fpm beim Aufsetzen): AeroACARS scorete „smooth, peak_vs_fpm: +5.7" — Touchdown-Window enthielt nur Post-Rollout-Daten.

  **Ursache:** Wir lasen `sim/flightmodel/position/vh_ind_fpm` — das ist die **VSI-Anzeige** wie im echten Cockpit, mit absichtlichem Damping (mehrere Sekunden Smoothing). Beim physischen Touchdown ist der gesmoothte Wert schon nahe 0, der echte Sinkflug ist als langsamer „Decay" über die letzten Sekunden verteilt — im 500ms-Touchdown-Window nicht mehr als Peak erkennbar.

  **Fix:** Switch auf `sim/flightmodel/position/local_vy` — die rohe vertikale Y-Achsen-Geschwindigkeit (m/s, real-time, kein Smoothing). Konvertierung im Setter: `value * 196.8504` (= 3.28084 ft/m × 60 sec/min). Das ist der gleiche DataRef den die etablierten X-Plane-Landing-Rate-Plugins (xgs, LRM, „A New Landing Rate Display") seit ~10 Jahren verwenden.

  Bei MSFS unverändert (ist ohnehin ein anderer Code-Pfad mit SimConnect-`PLANE TOUCHDOWN NORMAL VELOCITY`).

### 🛠 Intern
- DataRef-Switch in `client/src-tauri/crates/sim-xplane/src/dataref.rs`
- Verifiziert gegen X-Plane Developer-Doku + Production-Plugins (xgs, LRM)
- Tests: 76 grün

---

## [v0.4.2] — 2026-05-05

UX-Polish nach Pilot-Feedback heute Abend.

### 🆕 Neu
- **PIREP-Erfolgs-Banner** im Cockpit-Tab nach erfolgreichem File. Grüner Banner mit Callsign + Route bleibt 8 s sichtbar, manuell schließbar via X. Vorher verschwand das ActiveFlightPanel still — Pilot wusste nicht ob's wirklich gefilt wurde oder hängengeblieben ist.
- **Hint-Banner im Landung-Tab** wenn keine SimBrief-Plan-Werte vorhanden sind (alle SOLL-Spalten leer wären). Erklärt warum statt nur stille Striche.
- **SimBrief-OFP-Status im Activity-Log** beim Flight-Start. Vorher: Fetch-Fehler nur in Tracing-Logs (unsichtbar für Pilot). Jetzt drei Activity-Log-Einträge je nach Outcome:
  - ✅ „SimBrief OFP geladen" mit Plan-Block / Trip / TOW
  - ⚠️ „SimBrief-OFP konnte nicht geladen werden" mit OFP-ID + Erklärung
  - ⚠️ „SimBrief-OFP-Fetch fehlgeschlagen" mit Error-Detail
  - ℹ️ „Kein SimBrief-OFP für diesen Flug" wenn der Bid gar keinen hatte

So sehen Piloten beim nächsten Mal sofort wenn der OFP-Fetch silently scheitert (was zum heutigen „Landung-Tab hat keine SOLL-Werte"-Bug geführt hat).

### 🛠 Intern
- Neue UI-Component für PIREP-Success-Banner in `CockpitView.tsx` mit 8s Auto-Dismiss + Manual-Close
- i18n DE+EN für alle neuen Texte
- Tests: 76 grün

---

## [v0.4.1] — 2026-05-05

Real-Pilot-Workflow: was tun wenn der Sim mid-flight wegbricht?

### 🆕 Neu: Sim-Disconnect-Handling

Wenn der Streamer länger als 30 s keine brauchbaren Sim-Daten mehr bekommt (Sim-Crash, Quit, Window-Switch-Glitch), passiert jetzt:

1. **Flug wird automatisch in den Pause-Status gesetzt** — keine Position-Updates mehr an phpVMS, kein Phase-FSM, kein Activity-Log-Spam
2. **Die letzten bekannten Werte werden eingefroren** und sowohl im **Activity-Log** als auch in einem **gelben Banner im Cockpit-Tab** angezeigt:
   - Latitude / Longitude
   - Heading + Altitude
   - Fuel on Board
   - ZFW (Leergewicht)
3. **Heartbeat läuft weiter** — phpVMS' Live-Tracking-Cron killt den PIREP NICHT während der Pause (sonst wäre nach 2 h Schluss)
4. **„🔄 Flug wiederaufnehmen"-Button** im Banner — Pilot startet den Sim neu, lädt das Flugzeug an die richtige Position (oder bewusst andere — kein 5-NM-Restriction wie bei smartCARS), klickt den Button → Streamer macht weiter
5. **KEIN Auto-Resume:** auch wenn der Sim plötzlich wieder Daten liefert wartet das Backend auf den manuellen Klick (sonst würden Mid-Air-Position-Sprünge wild ins PIREP gehen)
6. **Reposition-Audit-Log:** beim Resume wird die Distanz zwischen alter und neuer Position berechnet. Bei großen Sprüngen (> 500 nm) als WARN-Level damit's für VA-Audits sichtbar ist
7. **Distance-Reset bei Resume:** Reposition-Sprung fließt **nicht** in die geloggte Flugdistanz ein. PIREP `distance_nm` zeigt nur tatsächlich geflogene Distanz, der Reposition-Δ wird separat als Activity-Log-Zeile festgehalten

Bewusst KEINE 5-NM/2000-ft-Restriktion wie bei smartCARS — der Pilot entscheidet wo er weitermacht, der Audit-Log macht's nachvollziehbar.

### 🛠 Intern
- Neuer Tauri-Command `flight_resume_after_disconnect` mit Δ-Distanz-Audit
- `FlightStats` erweitert um `paused_since` + `paused_last_known: PausedSnapshot`
- `ActiveFlightInfo` flow-through dieser Felder ans Frontend
- Neue Cockpit-Component `<DisconnectBanner>` (i18n DE+EN)
- Konstanten: `SIM_DISCONNECT_THRESHOLD_S=30`, `REPOSITION_WARN_DELTA_NM=500.0`
- Tests: 76 grün

---

## [v0.4.0] — 2026-05-05

Erstes Minor-Release der 0.4er-Reihe. Hauptthema: **Discord-Integration**.

### 🎉 Neu — Discord-Webhook

Vier Lifecycle-Events werden jetzt automatisch in den GSG-Discord-Channel gepostet, im Stil etablierter VA-Bots:
- ✈️ **Takeoff** (grün) — mit Block-Fuel + Plan-Δ + TOW
- 🛬 **Landung** (orange) — mit Landing-Rate + Score + Distance
- 📋 **PIREP filed** (violett) — kompletter Flugbericht
- ⚠️ **Divert** (amber) — mit Geplant/Tatsächlich-Vergleich

Layout angelehnt an den GSG-Bot-Stil:
- Author-Bar oben mit phpVMS-Pilot-ID + Name (z.B. „GSG0001 - Thomas K")
- Title als „Flight CHH3184/C.PF has landed"
- 3-Spalten-Felder: Dep.Airport / Arr.Airport / Equipment
- 2-Spalten-Felder: Flight Time / Distance
- **Großes Airline-Logo unten** — kommt direkt aus phpVMS (`bid.flight.airline.logo`), keine externe Hosting-Abhängigkeit. Wenn die VA das Logo-Feld in phpVMS pflegt, erscheint es automatisch.

Webhook-URL ist hardcoded für GSG (`#flights`-Channel). Posts laufen fire-and-forget (`tokio::spawn`) — Discord-Latenz blockt nie den Flugverlauf.

### 🛠 Intern
- Neues Modul `client/src-tauri/src/discord.rs` mit Embed-Builder + HTTP-Helper
- `ActiveFlight`/`PersistedFlight` erweitert um `airline_logo_url: Option<String>` (aus Bid-Relation; persistiert für Resume)
- `AppState.cached_pilot: Mutex<Option<(String, String)>>` — wird beim Login + Refresh aus dem phpVMS-Profile gefüllt, für die „GSG0001 - Pilot Name"-Zeile
- Discord Rich Presence Service (Crate `discord-rich-presence v1`) eingebaut aber noch nicht gewired — kommt in v0.4.1
- Tests: 76 grün

---

## [v0.3.5] — 2026-05-05

Drei X-Plane / phpVMS-Bugs nach Pilot-Test heute morgen.

### Behoben
- **MSL-Höhe weicht im Cruise um ~1.000 ft ab.** Wir lasen `sim/flightmodel/position/elevation` (= TRUE MSL, geographische Höhe über Sea Level), das aber bei nicht-ISA-Atmosphäre vom Indicated-Altitude abweicht. Pilot Michel D. sah heute auf YBBN→NWWW bei FL390 / OAT −46 °C → AeroACARS meldete 40.009 ft, PFD korrekt 39.000 ft (Differenz exakt die ISA-Deviation × 4 ft/°C). Im Sinkflug konvergierten die Werte wieder. Jetzt: `sim/cockpit2/gauges/indicators/altitude_ft_pilot` — Indicated, exakt was der Pilot sieht.
- **QNH-Anzeige zeigte unmögliche Werte (z.B. 198 hPa).** Der gelesene DataRef `sim/weather/barometer_current_inhg` ist der **Umgebungsdruck am Flugzeug**, nicht die Kollsman-Einstellung. Bei FL390 sind ~187 hPa Außendruck korrekt — aber das ist nicht was im Höhenmesser-Fenster steht. Jetzt: `sim/cockpit2/gauges/actuators/barometer_setting_in_hg_pilot` — die tatsächliche Altimeter-Setting (1013 hPa bei STD, real QNH bei lokal). Achtung: heißt `barometer_*` nicht `altimeter_*` (X-Plane-Naming-Inkonsistenz, gegen FlyWithLua + X-RAAS-Plugin verifiziert).
- **„Geflogene Route: 100%" während Boarding** auf der phpVMS-Live-Seite. v0.3.0 versuchte das durch Senden von `None` als `distance` während der Pre-Flight-Phase zu beheben — funktionierte nicht weil PHP's `empty()` sowohl `null` als auch `0` als „empty" erkennt und den 100%-Fallback triggert (1/1 = 100). Jetzt: minimaler Floor von 0.001 nm bis echte Distanz akkumuliert ist → `empty(0.001)` = false → Division läuft real → 0.001 / Plan-Distanz ≈ 0% bis Pushback.

---

## [v0.3.4] — 2026-05-04

Hot-Patch: v0.3.3 hatte einen TypeScript-Build-Fehler im CI (`'fnumMismatch' is declared but its value is never read`) — die Build-Jobs für Windows + macOS schlugen fehl, der `publish`-Step wurde geskipped, also kamen keine Installer am Release an. Inhaltlich = v0.3.3, nur sauber gebaut.

### Behoben
- **TS6133-Fehler in `BidsList.tsx`** — Cross-Product-Match-Logik entfernt nachdem v0.3.3 sie aus `ofpMismatch` rausgenommen hatte; die Variablen waren danach unused. Strict-Mode tot.

---

## [v0.3.3] — 2026-05-04 *(broken release — keine Build-Artefakte)*

Patch nach v0.3.2 — drei kleine UX-Fixes rund um die OFP-Mismatch-Detection.

### Behoben
- **Falscher OFP-Mismatch-Banner bei legitimen Plan-Varianten.** Der Match zwischen Bid-Flugnummer und SimBrief-OFP-Callsign war zu strikt. Beispiel: Bid „EWL 4368", OFP-Callsign „EWL4TK" (Pilot nutzt persönlichen ATC-Callsign in SimBrief). Der Banner feuerte fälschlich „SimBrief-OFP passt nicht zur Buchung", obwohl Aircraft + Origin + Destination alle übereinstimmten. Match-Logik jetzt bidirektional als Cross-Product aller Bid-Variants (Flight-Number + Callsign, mit/ohne Airline-ICAO-Prefix) gegen alle OFP-Variants. Plus: Flight-Number-Diff alleine triggert NICHT mehr den Banner — Aircraft / Origin / Destination sind die einzigen Signale stark genug für einen „altes OFP"-Befund. Eine Callsign-Differenz bei sonst stimmiger Route + Aircraft ist fast immer ein legitimer persönlicher ATC-Callsign.
- **Kein Hinweis wenn überhaupt kein OFP an die Buchung gebunden ist.** Vorher rätselte der Pilot warum die Plan-Cards leer sind. Jetzt blauer Info-Banner: „Kein SimBrief-OFP für diese Buchung — erstelle einen auf simbrief.com".

---

## [v0.3.2] — 2026-05-04

Patch-Release direkt nach v0.3.1. Zwei Pilot-Reports vom Live-Test:

### 🐛 Behoben
- **„Discard flight" / „Forget locally" / „Logs löschen" funktionierten auf macOS nicht.** Tauri auf macOS nutzt WKWebView, und WKWebView droppt `window.confirm()` und `window.alert()`-Aufrufe stillschweigend — der Dialog kommt nie, der Aufruf returnt sofort `false`/`undefined`, der Button-Handler springt raus. Auf Windows (WebView2) hat's funktioniert, daher fiel's vorher nicht auf. Alle 6 betroffenen Stellen (`ActiveFlightPanel`, `LandingPanel`, `ActivityLogPanel`, `SettingsPanel`, `ResumeFlightBanner`) nutzen jetzt eine neue In-App-`<ConfirmDialog>`-Component (kein Native-Dialog, kein Plugin, garantiert cross-platform).
- **Loadsheet im Cockpit verglich gegen einen veralteten OFP-Stand.** Real-Pilot-Workflow: Pilot regeneriert auf simbrief.com einen neuen OFP nachdem der Flug schon gestartet ist (Pax/Cargo/Reserve geändert). AeroACARS hatte die Plan-Werte beim Flight-Start eingefroren — der „Refresh"-Button im My-Flights-Tab refreshte nur die Bid-Card-Vorschau, nicht den aktiven Flug-Snapshot. Resultat: Loadsheet zeigte falsche Δ-Werte gegen die Plan-Variante, die der Pilot gar nicht mehr nutzt.

### ✨ Neu
- **OFP-Refresh-Button im Cockpit-Tab** (sichtbar in den Phasen Preflight / Boarding / TaxiOut). Klick → Backend zieht den aktuellen Bid + frische SimBrief-OFP, überschreibt `planned_block` / `planned_tow` / `planned_zfw` / `planned_route` / `planned_alternate` / `max_*` und persistiert sofort. Das Loadsheet vergleicht ab dem Klick gegen den neuen Plan. Activity-Log-Eintrag „OFP refreshed" mit den drei Hauptwerten als Audit-Trail.
- **`<ConfirmDialog>` + `useConfirm()`-Hook** als neue UI-Primitive. Kann von künftigen Components mitgenutzt werden — Esc cancelt, Enter confirmt, Backdrop-Click cancelt, optionaler `destructive`-Mode (rot statt blau). i18n-Keys: `confirm_dialog.default_title` / `confirm` / `cancel`.

### 🛠 Intern
- Neuer Tauri-Command `flight_refresh_simbrief()` — pullt Bid → SimBrief-OFP → mass-assigned planned_*-Felder unter dem `active_flight`-Lock. Verifiziert Bid-ID nach dem Await damit ein parallel-discarded Flight nicht überschrieben wird.

---

## [v0.3.1] — 2026-05-04

Konsolidierter 0.3.x-Release. Bündelt das komplette SimBrief-Integration-Paket (Phase H.7), erweiterte X-Plane-Telemetrie, Live-Block-Fuel-Fix, das Loadsheet-Feature, OFP-Mismatch-Detection, UX-Polish nach dem GSG-Live-Test sowie das **neue Divert-Manual-PIREP-Routing**.

### 🌟 Highlights
- **Divert-Manual-PIREP** — landet jetzt sauber im PENDING-Bucket des VA-Admins statt fälschlich auto-akzeptiert zu werden. Pilot klickt „Divert nach XXX" → PIREP wird als manueller Eintrag mit dem tatsächlichen Landing-Airport für Admin-Review markiert.
- **Loadsheet-Feature** — Live-Anzeige Block-Fuel / ZFW / TOW während Boarding plus Score-Bewertung im Landung-Tab.
- **SimBrief Soll/Ist-Vergleich** — kompletter Plan-vs-Actual-Block im Landung-Tab, farbcodiert mit aviation-tauglichen Schwellen (5/10 %).
- **OFP-Mismatch-Detection** — erkennt wenn der zuletzt von SimBrief geladene OFP nicht zur aktuellen Buchung passt.
- **X-Plane Auto-Reconnect + neue Telemetrie** — startet sich selbst neu, liefert Wing-/Wheel-Well-Lights + TO-Config-Warning für 737 Zibo/LevelUp + universelle Autobrake/XPDR-Labels.

> Hintergrund: v0.3.0 war als Tag bereits gesetzt, aber ohne Release-Notes. Statt rückwirkend zu rekonstruieren bündeln wir alles unter v0.3.1 — alles, was seit v0.2.4 reingegangen ist.

### 🐛 X-Plane Bug-Fixes
- **Gear-DataRef [0]-Index-Fix.** `sim/flightmodel2/gear/deploy_ratio[0]` mit explizitem Index — fixt „Gear UP am Boden" bei LevelUp 737 (gleiches RREF-Pattern wie der Engine-Bug damals).
- **Auto-Reconnect hart abgesichert.** Re-Subscribe-Loop alle 5 s wenn State ≠ Connected. Funktioniert in allen Szenarien: AeroACARS startet vor X-Plane, X-Plane Restart, X-Plane Crash, Aircraft-Wechsel mit Daten-Stillstand.

### ✨ X-Plane Erweiterungen
- **Autobrake-Stufe als Label** (universell, alle Aircraft) — `RTO/OFF/1/2/3/MAX`.
- **XPDR-Mode als Label** (universell) — `OFF/STBY/XPNDR/TEST/ALT/TA/TA-RA`.
- **Wing-Lights** (Boeing 737 Zibo / LevelUp).
- **Wheel-Well-Lights** (737 Zibo / LevelUp).
- **Takeoff-Config-Warning** (737 Zibo / LevelUp) — Warnung im Cockpit-Status wenn Flaps / Trim / Spoiler nicht für TO konfiguriert.

### 📡 phpVMS Live-Display
- **Live-Block-Fuel im `UpdateBody`** wird bei jedem Heartbeat mitgeschickt. phpVMS leitet „Verbleibender Treibstoff = block_fuel − fuel_used" daraus ab; ohne Feld defaultete block_fuel auf 0, Anzeige zeigte „−<fuel_used>" für den ganzen Flug („−17008 kg"-Bug).
- **Bid-Card erweitert** um Aircraft-Type + Marketing-Name + Load-Chips (Pax blau, Cargo orange) + Flight-Type-Badge (PAX/CARGO/CHARTER/REPO). Reihenfolge der Plan-Cards aviation-korrekt: Block → Trip → Reserve | ZFW → TOW → LDW | Alt.
- **SimBrief-Plan-Vorschau** im Briefing per `fetch_simbrief_preview` direkt auf der Bid-Card — Pilot sieht Block / Trip / Reserve / TOW / LDW / ZFW / Alternate VOR dem Tanken, ohne den OFP-Link zu öffnen.

### 🛫 SimBrief-Integration (Phase H.7)
- **API-Client für SimBrief XML-Fetcher** (`xml.fetcher.php`, beide ID-Varianten — numerische SimBrief-ID und Username). Backend-Anbindung läuft automatisch über die phpVMS-Bid-Relation, kein explizites Setup im Settings-Tab nötig.
- **`FlightStats` erweitert** um Plan-Felder: `planned_block_fuel_kg` / `planned_burn_kg` / `planned_reserve_kg` / `planned_zfw_kg` / `planned_tow_kg` / `planned_ldw_kg` / `planned_taxi_kg` / `max_zfw_kg` / `max_tow_kg` / `max_ldw_kg` + Aircraft-Reg + Plan-Route + Plan-Alternate.
- **Landung-Tab** mit komplettem Fuel + Weight + ZFW Soll/Ist/Δ — farbcodiert grün/gelb/rot. Schwellen praxisnah: <5 % grün, 5-10 % gelb, >10 % rot (vorher 1/3 % — viel zu eng für realen Flugbetrieb).
- **Overweight-Warnungen** wenn IST > MAX bei TOW / LDW / ZFW (`LoadsheetMonitor.tsx`).
- **OFP-Mismatch-Detection.** Vergleicht 4 Signale zwischen SimBrief-OFP und phpVMS-Buchung: Aircraft-Type, Origin, Destination, Flight-Number / ATC-Callsign (mit 4 Match-Formaten: direkt, ICAO-Prefix, ATC-Callsign, ATC mit Airline-Prefix). Bei Mismatch werden OFP-Werte komplett ausgeblendet damit keine falschen Daten angezeigt werden — Pilot sieht klaren Banner und weiß: neuen OFP generieren.

### 📋 Loadsheet-Feature (neu in 0.3.x)
- **`LoadsheetMonitor` im Cockpit-Tab** — sichtbar nur in Phase Preflight / Boarding (verschwindet ab TaxiOut). 3 Zeilen mit IST / SOLL / Δ / MAX für Block-Fuel / ZFW / TOW. Inline-Hints: „✓ Bereit für Pushback" / „🛢 Tankvorgang läuft — noch X kg fehlen" / „👥 Boarding läuft — noch X kg fehlen" / „💡 +X kg über Plan".
- **`LoadsheetScore` im Landung-Tab.** Score 0-100 basierend auf Δ% pro Wert (Block/TOW/LDW/ZFW): >5 % = -5 Punkte, >10 % = -15 Punkte. Score-Farbe ≥90 grün, ≥70 gelb, sonst rot. Plus Breakdown-Liste mit ✓/⚠/✕ pro Wert.
- **„Über-Tankt"-Hint im Activity-Log** beim Block-Off-Trigger wenn Block-IST > Plan + Reserve + 500 kg Toleranz. Sanft formuliert („Sehr viel Sprit an Bord, höherer Burn unterwegs zu erwarten") — keine Warnung, nur Cost-Index-Bewusstsein.
- **Loadsheet-Activity-Log @ Block-Off** einmalig „📋 Loadsheet @ Block-off" + Detailzeile (Block / ZFW / TOW). Wandert sowohl in den Cockpit-Activity-Log als auch in den phpVMS-PIREP-ACARS-Log. Dedup über `loadsheet_logged_at_blockoff` Flag (überlebt Resume-after-Crash).

### 🎨 UX-Polish nach GSG-Live-Test
- **Loadsheet im InfoStrip-Stil** (gleiche Optik wie der MASSE/FLUG/TRIP-Strip oben). Keine eigene Box — gehört visuell zum aktiven Flug-Block. Inline-Δ-Suffix statt eigener Spalte: „BLOCK 6.334 kg +0", „TOW 64.544 kg +227". Toggle-Button [▾]/[▸] zum Ein-/Ausklappen.
- **Wetter-Briefing 1-Zeilen-Format** ersetzt die alten 2 Cards: `ABFLUG EDDW 010°/6 kt · 👁 ≥ 10 km · 18°/12° · 1013 hPa  🌦 -SHRA  [▸ METAR]`. METAR-Text aufklappbar. Spart ~200 px Höhe.
- **Wetter-Phänomen-Pills** mit Icon + Code (🌦 SHRA / ⛈ TSRA / ☁ OVC / 🌫 FG) parsed aus dem METAR-Rawtext + Bewölkungs-Fallback.
- **Sicht-Fallback** aus Raw-METAR (`9999` → „≥ 10 km", `CAVOK` → ☀) wenn der Backend-Parser nichts liefert.
- **Visibility-Threshold 9.5 km** statt 10.0 km für die ≥10 km-Anzeige (Aviation-Konvention `9999 m = "10 km oder mehr"`).
- **Cockpit-Tab kompakter:** LiveTapes ~10 % schmaler (Padding 10/14 → 8/12, Schrift 22 → 20 px). RouteMap erst ab Pushback einblenden — vor Pushback ist 0 % Strecke logisch unsinnig.
- **PMDG-Status False-Positive-Fix.** SDK-Warnung wurde fälschlich gefired wenn Sim noch nicht connected, Aircraft im Loading, oder PMDG NG3 in der 20-60s Init-Phase. Jetzt 4-stufiger Check: simState=connected + aircraft_loaded + 20 s Geduld nach Subscribe + ever_received=false.

### ⚙️ Auto-Start-UX
- **Activity-Log-Hint wenn Auto-Start nicht greifen kann.** Drei spezifische Reasons mit jeweils eigener Meldung, throttled 1×/60 s pro Reason: Triebwerke an / Flugzeug rollt / in der Luft.
- **Auto-Start-Skip-Banner im Briefing-Tab.** Gelber Banner mit Begründung im Briefing-Tab — vorher musste der Pilot im Settings-Activity-Log nachschauen oder rätseln warum nichts passiert.
- **Auto-Start-State im Backend persistiert** (`app_config_dir/auto_start.json`). Bisher war `localStorage` die Source of Truth — nach Force-Kill / Hot-Reload im Tauri-Dev-Mode gelegentlich inkonsistent zum Watcher. Frontend zieht beim Mount den Backend-Wert und syncht localStorage als reinen Cache.

### 🛬 Divert-PIREP-Routing (Fix vom 2026-05-04)
- **Diverts werden nicht mehr fälschlich auto-akzeptiert.** phpVMS' `Acars\PirepController::file()` prüft beim Submit nur die Rang-Regel `auto_approve_acars` und ignoriert ein vorher per Smuggle gesetztes `source=MANUAL`. Sobald der PIREP danach `ACCEPTED` ist, blockt `checkReadOnly()` jeden weiteren State-Update — `state→PENDING` schlug mit „PIREP is read-only" fehl.

  **Neuer Pfad:** Bei Divert wird `/file` komplett übersprungen. Stattdessen ein einziger `update_pirep`-Call der `state=PENDING`, `source=MANUAL`, `arr_airport_id`, alle Stats und Timestamps mass-assigned **solange der PIREP noch IN_PROGRESS ist**. Verifiziert gegen phpvms@dev: `PirepController::update` + `parsePirep()` schieben alles per Mass-Assign auf den Pirep-Record, alle nötigen Felder sind in `$fillable`. Der PIREP landet sauber im PENDING-Bucket des VA-Admins ohne Auto-Approve-Trigger.
- **Activity-Log-Display-Fix.** Zeigt bei Divert die echte Arrival-ICAO mit „(DIVERT, planned X)" Suffix statt der alten Plan-Destination — sowohl im Auto-Path als auch im Manual-Path.

### 🛠 Intern
- `UpdateBody` (api-client) erweitert um `arr_airport_id`, `landing_rate`, `score`, `submitted_at`, `block_on_time` für den Divert-Mass-Assign-Pfad.
- `PirepFull.distance` entfernt — phpVMS gibt das Feld inkonsistent als Objekt oder Zahl zurück, wir brauchen's für den State-Check eh nicht.
- `SimSnapshot` erweitert um `light_wing`, `light_wheel_well`, `xpdr_mode_label`, `takeoff_config_warning` als universelle Felder. PMDG-Adapter füllt sie via `snapshot()`-merge, X-Plane-Adapter via DataRefs. Activity-Log liest direkt aus `snap.*` statt aus `snap.pmdg.*` → einheitlicher Pfad.
- Tests: 76 grün (unverändert).

### 📭 Bewusst nicht in 0.3.x
Diese Punkte standen mal auf dem Master-Plan, sind aber nicht enthalten — Code-Verifikation per Grep:
- **Aircraft-Reg-Verifikation (SimBrief vs. Sim).** War in v0.1.x drin, wegen MSFS-2024 Pilot-Profil-Override mit False-Positives wieder ausgebaut. Bleibt skipped bis ein WASM-Livery-Reader steht.
- **Settings-Tab SimBrief-ID/Username-Eingabefeld + Test-Button + Status-Pill.** SimBrief-Anbindung läuft automatisch über die phpVMS-Bid-Relation, daher kein expliziter Setup-Schritt nötig.
- **One-Time Update-Banner im Cockpit-Tab nach erstem Start.** Aus dem gleichen Grund nicht implementiert.
- **„Tipp"-Hinweise im Activity-Log wenn ohne SB-ID gestartet.** Same.

---

## Frühere Versionen

Notes für v0.2.x und v0.1.x liegen in den jeweiligen Release-Commit-Messages (`git log --oneline | grep "release:"`). Die Tags `v0.3.0` (Dev-Build, 2026-05-03) und v0.3.1 markieren denselben funktionalen Release-Zweig — alles, was zwischen v0.2.4 und v0.3.1 reingewachsen ist, steht oben unter `[v0.3.1]`.
