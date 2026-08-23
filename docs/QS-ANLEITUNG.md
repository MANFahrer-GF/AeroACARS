# QS-Runde selbst durchführen

Diese Anleitung reicht aus, um eine QS-Runde ohne Rückfragen zu fahren.
Sie ist aus achtzehn Runden an der Bahndisziplin (v1.7.0) entstanden, in
denen jede Runde etwas fand — auch die, die grün begann.

---

## 1. Das Prinzip

**Eine Runde prüft, was man beim Bauen im Kopf hatte. Der Rest liegt
ausserhalb dieses Kopfes und kommt nur heraus, wenn man den Winkel
wechselt.**

Deshalb: nie dieselbe Prüfung wiederholen. Pro Runde eine andere Ebene.
Weiter, bis eine Runde nichts mehr findet — und dann noch eine.

**Jede neue Prüfung braucht ihre Gegenprobe.** Alten Zustand
wiederherstellen, Test *muss* rot werden. Ein Test ohne Gegenprobe ist
eine Behauptung, kein Nachweis. Zwei Mal ist genau das schiefgegangen:

- Eine Gegenprobe baute an der **falschen Stelle** zurück (`props.clearance_point_m != null`
  steht dreimal in derselben Datei) und blieb grün.
- Ein Test zählte Aufrufe **im Testcode** als Beleg und blieb ebenfalls grün.

---

## 2. Die Winkel, die sich bewährt haben

| # | Winkel | Was er findet |
|---|---|---|
| 1 | **Konsumenten** des geänderten Werts — wer liest ihn, rechnet jemand nach? | Zweitimplementierungen |
| 2 | **Ränder der Daten** (0, negativ, NaN, extrem kurz/lang) über den ECHTEN Bestand | geratene Zahlen neben echten |
| 3 | **Gerendertes Ergebnis** statt reiner Funktionen | Überlappungen, Überläufe |
| 4 | **Zukunftsfestigkeit** — was, wenn die Datenquelle ihre Konvention ändert | stille Formatwechsel |
| 5 | **Altdaten und andere Clients** (Stratos), Deploy-Reihenfolge | zerstörte Historie |
| 6 | **Zweitimplementierungen** systematisch (§9 der Spec) | Drift |
| 7 | **Feldkette** nebeneinander legen: Rechnung → Platte → Leitung → Mapper → Anzeige | abgehängter Code |
| 8 | **Totes Zeug**: `never used`, ungenutzte `pub fn`, tote i18n-Schlüssel | Verträge, die niemand einhält |
| 9 | **Invarianten** zwischen zusammengehörigen Werten | verdrehte Reihenfolgen |
| 10 | **Nutzlast und Grenzen** — Paketgrössen, Broker-Limits, Speicher | stille Ausfälle |
| 11 | **Das Prüfwerkzeug selbst** — rechnet es wie der Client? | schiefe Korpus-Zahlen |
| 12 | **Die eigenen neuen Tests** — sind sie scharf? | falsch-grüne Prüfungen |

### Winkel 7 und 8 sind die ergiebigsten

**Dreimal in einer QS** fand sich Code, der gebaut, getestet und
dokumentiert war — und nirgends aufgerufen wurde:
`ausfahrten_fuer_bahn`, `aussenkante_halb_aus_spur`, `aussenkante_halb_m`.

**Der Compiler kann das nicht melden.** `never used` gilt nur für private
Elemente; ein `pub fn` im Crate könnte von aussen benutzt werden. Die
eigenen Tests sind grün, die Doku liest sich sauber, und niemand ruft an.

> **Wer einmal abgehängten Code findet, sucht weiter.** Dieselbe Bauphase
> produziert dieselbe Lücke mehrfach.

---

## 3. Die Wächter, die das jetzt automatisch prüfen

Diese laufen bei jedem Testlauf mit. Wird einer rot, ist das ein echter
Befund — nicht ein zu strenger Test.

| Prüfung | Datei | Was sie hält |
|---|---|---|
| Feldkette | `client/src/dev/Feldkette.test.ts` | Jedes Bahndisziplin-Feld durchläuft alle sechs Glieder |
| Skip-Gründe | `client/src/dev/SkipGruende.test.ts` | Jeder `skipped(...)`-Grund hat einen Text in Webapp **und** Monitor |
| Anzeige-Gleichheit | `client/src/components/AnzeigeSync.test.tsx` | Client und Webapp zeigen dieselbe Grafik; rechnet den Importbaum nach |
| Angeschlossen | `client/src-tauri/tests/angeschlossen.rs` | Jede `pub fn` der Bewertungsmodule wird produktiv gerufen |
| Tote Schlüssel | `client/src/i18n/vollstaendigkeit.test.ts` | Kein `runway_v2`-Schlüssel ohne Verwendung, keine Sprache ohne Eintrag |
| Anzeige-QS | `client/src/components/RunwayQS.test.tsx` | 22 Regeln über alle Varianten und beide Ansichten |
| Lesbarkeit | `client/src/components/RunwayLesbarkeit.test.tsx` | Kein Text überlappt, nichts läuft aus dem Bild |

---

## 4. Wo alles liegt

### Die zwei Repos

| Was | Wo |
|---|---|
| Client (kanonisch für die Anzeige) | `~/Claude/aeroacars-src` |
| Server, Webapp, Monitor, Recorder | `~/Claude/aeroacars-live` |

**`client/src` ist die eine Quelle der Landebahn-Anzeige.** Die Webapp
bekommt eine Kopie:

```bash
node scripts/anzeige-sync.mjs              # prüfen
node scripts/anzeige-sync.mjs --schreiben  # abgleichen
```

Nicht abgeglichen werden die **Mapper** (verschiedene Quellen, gleiches
Ergebnis) und das **Glossar-Modal** (repo-eigener Dialog-Baustein). Die
Begründungen stehen im Kopf des Skripts.

### Die Spezifikationen

| Datei | Inhalt |
|---|---|
| `docs/spec/v1.7.0-bahndisziplin.md` | Die Bewertung: Achsen, Bänder, Korpus-Zahlen, Bau-Reihenfolge |
| `docs/spec/runway-diagram-v2.contract.md` | Die Anzeige: Props, beide Mapping-Wege, Wire-Felder |
| `docs/spec/assets/v1.7.0-bahndisziplin-referenz.html` | **Die Referenzgrafik** |

> **Wer die Anzeige ändert, vergleicht gegen die Datei — nicht gegen die
> Beschreibung der Datei.** Der erste Bau entstand aus dem Text und war
> strukturell richtig, in den Einzelheiten falsch.

### Die Prüfwerkzeuge (laufen auf dem Live-Server)

| Werkzeug | Was es tut |
|---|---|
| `tools/korpus/korpus_export.py` | Alle Landungen als CSV → `/tmp/korpus_v170.csv` |
| `tools/korpus/spuren_export.py` | Neun echte Rollspuren für die Demo |
| `tools/korpus/ausfahrten_export.py` | Ausfahrten aus den OSM-Bodenkarten |

**Diese Werkzeuge sind selbst Zweitimplementierungen.** Ändert sich die
Client-Logik, müssen sie mit — sonst misst der Korpus etwas anderes als
der Client tut. Genau das war Befund 19.

---

## 5. Der Ablauf einer Runde

```bash
# 1. Alles grün? (Ausgangspunkt festhalten)
cd ~/Claude/aeroacars-src/client/src-tauri && cargo test --workspace
cd ~/Claude/aeroacars-src/client && npx vitest run
cd ~/Claude/aeroacars-src && node scripts/anzeige-sync.mjs
```

```bash
# 2. Winkel wählen (einen, der noch nicht dran war) und prüfen
```

```bash
# 3. Für jeden Befund: erst messen, dann bauen
#    Bei Bewertungsänderungen IMMER am Korpus nachrechnen:
ssh live 'cat > /tmp/korpus_export.py' < tools/korpus/korpus_export.py
ssh live 'cd /tmp && python3 korpus_export.py'
scp live:/tmp/korpus_v170.csv /tmp/
cd client/src-tauri && KORPUS=/tmp/korpus_v170.csv \
  cargo test -p landing-scoring --test korpus_v170 -- --ignored --nocapture
```

```bash
# 4. Test schreiben, Gegenprobe fahren (alter Stand MUSS rot werden)
# 5. Anzeige abgleichen, alles laufen lassen
node scripts/anzeige-sync.mjs --schreiben
```

```bash
# 6. Demo bauen und ansehen
node scripts/demo-bauen.mjs /tmp/bahndisziplin.html
#    Zum Dranarbeiten mit laufendem Neuladen:
npx vite --config client/vite.demo.config.mjs --port 1421
#    → http://localhost:1421/demo.html
```

```bash
# 7. Webapp ausliefern (nur wenn die Anzeige betroffen ist)
cd ~/Claude/aeroacars-live/webapp && npm run build
rsync -az --delete dist/ live:/opt/aeroacars-live/webapp/dist/
curl -s -o /dev/null -w "%{http_code}\n" https://live.kant.ovh/admin/
```

---

## 6. Regeln, die sich teuer gelernt haben

**Der Client rechnet, der Server zeigt an.** Keine Bahndisziplin-Grösse
wird serverseitig hergeleitet: Sie stammen aus dem 5-Hz-Rollout-Fenster,
das nur der Client sieht. *Zwei Zahlen für dieselbe Landung sind schlimmer
als eine fehlende.*

**Nichts raten.** Fehlt eine Grösse, entfällt die Anzeige **sichtbar** mit
dem Grund. Eine leere Querachse sieht aus wie eine Messung, die nichts
gefunden hat.

**Den richtigen Grund nennen.** Bei EDDS 07 stand „Für diese Bahn ist keine
Breite hinterlegt" — die Bahn ist 45 m breit, der Flug war nur älter als
v1.7.0. Wer das liest, prüft die Navdaten und findet nichts.

**Keine stillen Auslassungen.** Wenn die Anzeige etwas weglässt (zu
gedrängt, zu viele), muss sie es zählen: „R11/M19 **+2**".

**Ein Grenzwert gehört zu der Rechnung, für die er kalibriert wurde.** Als
die Rechnung auf die Reifen-Aussenkante umgestellt wurde, musste die
Kantentoleranz mitwachsen (1,5 → 2,1 m) — sonst fiel genau der Fall wieder
auseinander, für den sie gebaut wurde.

**Nichts stillschweigend entfernen.** Beim Zusammenführen der beiden
Anzeigen wäre „↓ Soll-Aufsetz-Stelle" lautlos verschwunden. Sie wurde
zuerst gerettet — und dann *mit Begründung* entfernt, weil sie fachlich
falsch war (auf den Aim-Point wird gezielt, aufgesetzt wird dahinter).

**Ein Test, der sich am Verschwindenden festhält, verschwindet mit ihm.**
Der Bremspunkt-Test las seinen Suchtext aus der Sprachdatei. Nach dem
Entfernen des toten Schlüssels prüfte `not.toContain(undefined)` nichts
mehr.

**Falscher Alarm richtet denselben Schaden an wie gar kein Alarm.** Eine
Prüfung, die grundlos rot wird, wird abgeschaltet.

---

## 7. Das Gedächtnis

Der Hintergrund — Entscheidungen, frühere Fehler, das „Warum" — steht in
`~/.claude/projects/-Users-thomaskant-Claude-GSG/memory/`.

Für die Landebewertung besonders:

- `feedback-qs-runden-bis-die-befunde-ausgehen` — die Winkel und die Gegenprobe
- `aeroacars-landebewertung-zweitimplementierungen` — wo dieselbe Rechnung nochmal lebt
- `anzeige-eine-quelle-client-webapp` — der Sync und seine Ausnahmen
- `aeroacars-demo-bedienbar` — warum statische Demos täuschen
- `feedback-exhaustive-analysis-before-shipping` — volle Fehleroberfläche vor dem Release

Bei Codefragen **zuerst den Wissensgraphen fragen** (`graphify query`),
dann greppen.

---

## 8. Was noch offen ist

| Punkt | Stand |
|---|---|
| Client-Tag v1.7.0 | wartet auf Freigabe — Releases nie ohne Ansage |
| Echter Divert gegen den Live-Server | nie getestet |
| MSFS `flight_model.cfg` gegen eine echte Datei | X-Plane ist verifiziert, MSFS nicht |
| Drei Entscheidungen in §13 der Spec | Gewichtung der Achse, Bandgrenzen, kleine Plätze ohne Navdaten |
| **Export / PDF-Export** | von Thomas angemeldet, noch nicht angefasst |
