# Übergabe: Rufzeichen-Hinweis und Cockpit-Schrift fertig auf main — gemeinsam mit den Technik-Fixes als v1.7.45 veröffentlichen

Datum: 2026-09-21
Projekt: /Users/thomaskant/Claude/aeroacars-src

## Wo es anfing

Nach dem Release von v1.7.44 (Nacht zum 21.09.) wollte Thomas erklärt haben, dass das
Callsign im PDC/CPDLC-Tab auch für die VDGS-/TOBT-Abfrage gilt. Daraus wurden
mehrere QS-Runden: Ein Zeichen, das ich zur Umbruchkorrektur eingesetzt hatte, fehlte der
Cockpit-Schrift B612 Mono. Daraus entstand die Aufgabe „Fremdglyphen in der Cockpit-Schrift
beheben", samt einem Wächter dagegen. Vorgaben: keine neuen Features, nachhaltiges Bugfixing;
jede Runde extern prüfen (Codex, ersatzweise Cloud-Agent); nichts veröffentlichen ohne
Freigabe. Thomas hat am Ende entschieden: **ein gemeinsames Release mit den Fixes aus dieser
Sitzung** (`fix/technik-befunde-diagnoselog`) — diese Übergabe ist für sie gedacht.

## Entschieden und gebaut

Alles liegt auf `origin/main`, von `0a474490` bis `f4a5eced`. Nichts davon ist veröffentlicht.

- **Rufzeichen-Hinweis:** Im PDC/CPDLC-Tab steht unter dem Kopf ein Satz: Das Callsign gilt auch für die VDGS-Abfrage (TOBT/TSAT). In der schmalen VDGS-Zeile steht bei „kein CDM-Eintrag" dazu, wo man es ändert, bei einer Störung bewusst nicht. — `/Users/thomaskant/Claude/aeroacars-src/client/src/components/CpdlcPanel.tsx`, `.../VdgsBand.tsx`, `.../App.css` (`.datalink-callsign-reichweite`), Locales DE/EN/IT. Der Satz steht AUSSERHALB der Statuszeile, weil die auf 66 px festgenagelt ist und still abschneidet.
- **Deutscher Bandtext umformuliert:** „kein CDM-Eintrag — Rufzeichen im Tab PDC/CPDLC". Die Reihenfolge ist gedreht, damit kein Bindestrich vor „Tab" mitten im Wort bricht. Nachgemessen über 260–900 px: null Wortbrüche.
- **Fremdglyphen behoben:** B612 Mono fehlen u. a. → ← ≤ ≥ Δ ✓ ⚠ ⏸ ▶ U+2011. Folgendes wurde geändert:
  - Wetter „≥ 10 km" → „10+ km"
  - Aktivitätsprotokoll: Strecke „A – B", Zuweisung „MCP IAS: 250", „Fuel-Diff" statt „Fuel-Δ", ✓/⚠/⏸/▶ am Meldungsanfang gestrichen (die Stufe färbt die Zeile ohnehin)
  - X-Plane-Menüpfad mit „>"
  - OFP-Zeile „vorher X, jetzt Y"
  - FMA „HDG (arm LOC)" (Crate sim-msfs, Tests nachgezogen)
  - Loadsheet-Tabelle „!" statt „⚠"
  - Loadsheet-Zeile „Diff" statt „Δ" — **auch im ACARS-Log an phpVMS**, Entscheidung Thomas: überall gleich
  - PIREP-Texte an phpVMS (z. B. `DIVERT: X → Y`) sind unverändert, weil Betreiber sie greppen.
  
  Dateien: `/Users/thomaskant/Claude/aeroacars-src/client/src-tauri/src/lib.rs`, `.../client/src/components/WeatherBriefing.tsx`, `.../LoadsheetMonitor.tsx`, `.../client/src-tauri/crates/sim-msfs/src/adapter/telemetry.rs`.
- **Wächter gegen diese Fehlerklasse:** `/Users/thomaskant/Claude/aeroacars-src/scripts/pruefe-schriftzeichen.mjs`.
  - Er prüft i18n-Zweig `cdm.band`, Literale in WeatherBriefing, LoadsheetMonitor und ActivityLogPanel sowie alle Literale in `log_activity`, `log_activity_handle`, `log_activity_and_record` und `pending_acars_logs.push` über alle `.rs` unter `client/src-tauri/src/`.
  - Er liest mit einem echten Scanner, nicht mit Regex: `/Users/thomaskant/Claude/aeroacars-src/scripts/literale.mjs`, Selbsttest `scripts/pruefe-literale.mjs`.
  - Zeichenvorrat: `client/src/assets/fonts/b612mono-zeichen.json`, erneuert mit `scripts/erzeuge-schriftzeichen.py` (braucht fontTools).
  - Er läuft in der CI (`.github/workflows/ci.yml`, Linux-Job; `scripts/**` im Pfadfilter).
- **Grenzen des Wächters stehen im Kopf der Datei.** Er sieht keine Texte aus Variablen, keine aus Crates, keine vom Server, keine gespeicherten Alteinträge und keine JSX-Textknoten. Grün heißt deshalb nur: Kein direkt erkennbarer Text enthält ein fehlendes Zeichen.

## Release-Notes-Punkte (für v1.7.45, DE und EN, gleiche Punktzahl)

Format wie `/Users/thomaskant/Claude/aeroacars-src/docs/release-notes/v1.7.44.md` (Überschriften
`## 🇩🇪 Deutsch` / `## 🇬🇧 English`, Punkte `- **Titel.** Text`). Prüfen mit
`node scripts/pruefe-release-notes.mjs docs/release-notes/v1.7.45.md` — der verlangt je Sprache
genau einen Block und gleich viele Punkte.

Deutsch:

- **Wo das Rufzeichen herkommt, steht jetzt dabei.** Das Callsign im PDC/CPDLC-Tab gilt auch für die Abfrage der eigenen Abflugfolge (TOBT/TSAT) — das steht dort jetzt ausdrücklich. Findet das VDGS-Band keinen CDM-Eintrag, nennt die schmale Zeile den Weg zum Rufzeichen gleich mit. Ist nur der Dienst nicht erreichbar, bleibt dieser Hinweis weg, weil dann nicht das Rufzeichen das Problem ist.
- **Keine Fremdzeichen mehr in der Cockpit-Schrift.** Protokoll, Wetter und Loadsheet-Tabelle verwenden die Schrift B612 Mono. Einige Texte enthielten Zeichen, die diese Schrift nicht kennt: Pfeile, „≥", „Δ", Häkchen, Warn- und Pausensymbole. Der Rechner setzte sie still aus einer anderen Schrift ein. Dann passte die Breite nicht ins Raster, Spalten verrutschten, und auf manchen Systemen erschien ein Kästchen. Die Texte sind jetzt so geschrieben, dass sie ohne diese Zeichen auskommen: etwa „10+ km", „EDDM – LEBL", „MCP IAS: 250", „HDG (arm LOC)" und „Fuel-Diff".

English:

- **Where your callsign comes from is now spelled out.** The callsign in the PDC/CPDLC tab also drives the lookup of your own departure sequence (TOBT/TSAT), and the tab now says so. When the VDGS band finds no CDM entry, its slim line tells you where to change the callsign. If only the service is unreachable, that hint is left out, since the callsign isn't the problem then.
- **No more stray glyphs in the cockpit font.** The log, weather and loadsheet table use the B612 Mono font. Some texts contained characters that font does not have: arrows, “≥”, “Δ”, check marks, warning and pause symbols. The system quietly borrowed them from another font, so they broke the monospace grid, columns shifted, and on some systems a box appeared instead. The texts are now written without those characters, for example “10+ km”, “EDDM – LEBL”, “MCP IAS: 250”, “HDG (arm LOC)” and “Fuel-Diff”.

Hinweis für VA-Betreiber (in die Notes aufnehmen, DE und EN): Die Loadsheet-Zeile im
ACARS-Log eines PIREP lautet beim Block-off jetzt „(Plan … kg, Diff +…)" statt „Δ +…". Wer
im phpVMS-Log nach „Δ" sucht, findet neue Einträge nicht mehr.

## Was nicht funktioniert hat

- **Geschützter Bindestrich (U+2011) gegen den Wortbruch.** B612 Mono kennt das Zeichen nicht. Das Ergebnis wäre eine Fremdglyphe mitten im Wort gewesen, schlimmer als der Umbruch. Die Lösung war, den Satz umzuformulieren.
- **Screenshot als Nachweis für die Schrift.** Auf dem Mac haben SF Mono und Menlo die fehlenden Zeichen. Die Vorschau sieht deshalb richtig aus und bestätigt den Fehler, statt ihn zu zeigen. Tragfähig sind nur:
  - `document.fonts.check('11px "B612 Mono"')` für die Frage, ob die Schrift geladen ist
  - eine Breitenmessung per Canvas: Die Zelle ist 7,15 px breit, „+" und „–" treffen sie genau, „≥" misst 6,04, „→" 6,62, „Δ" 7,94
  - die cmap der Schriftdatei selbst
- **Literale per Regex suchen.** Der Regex verlor Rust-Zeilenfortsetzungen mit `\` am Zeilenende und blieb bei genau den korrigierten Texten grün. Er zählte Klammern in Strings und Kommentaren mit, überging Escapes wie `"\u{2192}"` und übersprang Strings in `${…}`. Alle diese Fälle stehen jetzt als Testfall im Scanner-Selbsttest.
- **Emoji-Grenze `\p{Extended_Pictographic}`.** Die Grenze war zu weit, ⚠ ⏸ ☀ ✈ haben Text-Darstellung. Auch „jedes Graphem mit FE0F überspringen" war zu weit, dann rutscht „→️" durch. Jetzt zählt eine Folge nur als Bild, wenn ihr Grundzeichen `\p{Emoji}` ist.
- **Pauschale Ersetzung über ganz lib.rs** (das ⚠ am Meldungsanfang). Sie traf drei Stellen außerhalb der Protokollaufrufe, eine davon war zufällig richtig. Künftig wird jede Stelle einzeln beurteilt.
- **Zwei `cargo test --workspace` gleichzeitig.** Die Läufe haben sich gegenseitig blockiert, rund 30 Minuten waren verloren. Rust-Tests laufen über die CI, lokal immer nur ein cargo-Prozess (Gedächtnis: `macos-syspolicyd-bremst-cargo`).
- **Codex:** Heute ist das Nutzungslimit bis ca. 14:15 erreicht, und `codex exec` ohne `< /dev/null` hängt. Als Ersatz dient ein Cloud-Agent (`Agent` mit `isolation: "remote"`). Dessen Arbeitsverzeichnis war der GSG-Baum, nicht aeroacars-src, deshalb muss der absolute Pfad im Auftrag stehen.
- **Test-Attrappen:** `react-i18next` gemockt liefert den Ersatztext aus dem Quelltext, NICHT die Sprachdatei. Solche Tests sind blind für den Text, den der Pilot sieht, deshalb wird die JSON jetzt direkt mitgeprüft. Ein Mock, der für `hoppie_list_elements` `undefined` liefert, erzeugt unbehandelte Fehler bei grünen Tests.
- **Anführungszeichen „…" in JS-/TS-Strings mit geradem `"` am Ende** beenden den String. Das ist dreimal passiert (Testname, Grund-Text, Assertion).

## Wichtige Dateien für den nächsten Chat

- `/Users/thomaskant/Claude/aeroacars-src/handoff/2026-09-21-v1745-teil-schrift-callsign.md` — diese Datei, Release-Notes-Punkte oben
- `/Users/thomaskant/Claude/aeroacars-src/docs/release-notes/v1.7.44.md` — Format der Notes
- `/Users/thomaskant/Claude/aeroacars-src/scripts/pruefe-schriftzeichen.mjs` — Kopf nennt Abdeckung und Grenzen
- `/Users/thomaskant/Claude/aeroacars-src/.github/workflows/ci.yml` — Wächter-Schritt im Linux-Job

## Was gerade läuft

- Hintergrundprozesse: keine (beide cargo-Läufe beendet, alle Vorschau-Server gestoppt, temporäre `src/dev/_*`-Vorschauen und Einträge in `/Users/thomaskant/Claude/GSG/.claude/launch.json` entfernt)
- Server und Ports: keine
- Branches und Arbeitskopien:
  - `/Users/thomaskant/Claude/aeroacars-src` steht auf dem lokalen Zweig `fix/sprit-verbrauch-spalte`, dessen HEAD = `origin/main` = `f4a5eced` (in dieser Sitzung immer `git push origin HEAD:main`). Arbeitsbaum sauber.
  - `/Users/thomaskant/Claude/aeroacars-live` steht auf `fix/verbindung-nachhaltig` (Zweig der anderen Sitzung) — von hier nicht angefasst.
  - Probe-Merge `origin/fix/technik-befunde-diagnoselog` auf `origin/main`: konfliktfrei (`git merge-tree` exit 0, Baum `bebd2ac…`). Der Zweig zweigt bei `203cef0e` ab; `main` ist seitdem 5 Commits weiter. Wächter über eine Kopie des Merge-Ergebnisses: grün, 262 Texte. Der Zweig bringt keine neuen Protokollaufrufe mit, nur `tracing`.

## Prüfen, ob noch alles steht

- `git -C /Users/thomaskant/Claude/aeroacars-src rev-parse --short origin/main` — `f4a5eced` (oder später, wenn inzwischen gemergt)
- `cd /Users/thomaskant/Claude/aeroacars-src && node scripts/pruefe-literale.mjs | tail -1` — „17 Fälle"
- `cd /Users/thomaskant/Claude/aeroacars-src && node scripts/pruefe-schriftzeichen.mjs --selbsttest && node scripts/pruefe-schriftzeichen.mjs` — „23 Fälle", dann „262 Texte … alle in B612 Mono vorhanden"
- `git -C /Users/thomaskant/Claude/aeroacars-src merge-tree --write-tree origin/main origin/fix/technik-befunde-diagnoselog >/dev/null; echo $?` — `0` (kein Konflikt)
- `gh run list --limit 1` im Repo — Lauf für `f4a5eced` `success` (war Lauf 35587902902)

## Offen und verschoben

- Verschoben: **Texte über Variablen, aus Crates und vom Server** erfasst der Wächter nicht. Ein statischer Prüfer kann das grundsätzlich nicht, deshalb steht es im Kopf der Datei. Wer eine neue Protokollmeldung über eine Variable baut, prüft sie selbst.
- Verschoben: In vier TS-Dateien weicht der Scanner vom TypeScript-Compiler ab: AboutPanel, LiveMapView, UpdateButton, `lib/vatsimKarte.ts`. Keine davon steht in der Prüfliste, deshalb ist die Abweichung heute folgenlos.
- Verschoben: Die Warn- und Error-Meldungen mit geändertem Text beginnen in GlitchTip neue Issue-Gruppen. Ein Filter auf die alten Texte wurde nicht gefunden.
- Offen: **Die Freigabe für das Release v1.7.45** fehlt noch. QS erteilt keine Veröffentlichungserlaubnis.
- Offen (aus der anderen Sitzung): Die **Prüflisten-Regel** für Flüge ohne Live-Sitzung muss entschieden sein, bevor der Recorder ausgerollt wird. Reihenfolge laut der anderen Sitzung: Recorder, dann Gate-Modul, dann Client.
- Hinweis: Die fachlichen Änderungen des Zweigs `fix/technik-befunde-diagnoselog` wurden von hier NICHT inhaltlich geprüft, nur auf Konflikte und Schriftzeichen. Ob dort eine externe Abnahme gelaufen ist, war aus dem Verlauf nicht zu erkennen.

## Hier weitermachen

`origin/main` (mit `f4a5eced`) in `fix/technik-befunde-diagnoselog` holen, auf dem kombinierten Stand einmal CI und die Wächter laufen lassen und die Release-Notes-Punkte oben in `docs/release-notes/v1.7.45.md` übernehmen. Danach geht es mit der Recorder-Entscheidung und der vereinbarten Reihenfolge weiter.
