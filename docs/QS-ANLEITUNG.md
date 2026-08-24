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
der Client tut. Genau das war Befund 19, und in Runde 28/29 gleich
zweimal wieder.

`spuren_export.py` hat deshalb eine Gegenprobe, die **ohne Datenbank**
läuft und damit auch auf dem Mac:

```bash
python3 tools/korpus/spuren_export.py --selbsttest
```

Sie prüft die Regeln, die das Werkzeug mit `bahndisziplin_tick` teilt.
Beim ersten Lauf hat sie sofort einen Denkfehler in ihrer eigenen
Erwartung gefunden — genau ihr Zweck. Der Kopf des Werkzeugs führt Liste,
was zuletzt angeglichen wurde.

> Ein Prüfwerkzeug ohne eigene Gegenprobe fällt erst auf, wenn jemand die
> Zahlen anzweifelt, die man damit begründet hat.

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

---

## 9. Nachtrag aus Runde 20: Prüfungen, die nichts prüfen

Thomas' Gegenprüfung von `75abdc6` fand zwei fachliche Fehler in grünem
Code. Beide Male lag es nicht an einem falschen Test, sondern an einem
**blinden**.

**Ein Test mit unvollständigem Ausgangszustand prüft nur die Hälfte, die
er kennt.** Der Test zum stillen Rückfall setzte `bahn_raeum_gs_kt` nicht
— und genau über dieses Feld lief der Rückfall. Ergebnis `None`, Test
grün, Fehler unentdeckt.

> Deshalb: Bei Zuständen mit mehreren optionalen Eingängen **alle
> Kombinationen** durchgehen, nicht die drei, an die man beim Bauen
> gedacht hat. Für `bahn_felder` sind das acht — der Test dazu steht in
> `src/lib.rs` und prüft je Kombination drei Regeln, die immer gelten
> müssen.

**Ein Riegel gilt nur auf dem Weg, auf dem er steht.** Die Invariante
„Ausschwenken vor Kante" war im Live-Pfad abgesichert; die Nachrechnung
umging ihn. Der Test von damals lief durch den Live-Pfad und sah nichts.

> Wer eine Regel auf einem zweiten Weg noch einmal braucht, hat eine
> Zweitimplementierung gebaut — auch wenn es nur drei Zeilen sind.

**Und die Auswertung des Testlaufs kann selbst lügen.** Dieser Befehl
meldete „grün", obwohl der Build rot war:

```bash
cargo test --workspace 2>&1 | grep -E "test result:" \
  | awk '{p+=$4} END{print "gruen:",p}'
```

Bricht die Übersetzung ab, gibt es **keine** `test result`-Zeile — `awk`
summiert nichts und druckt trotzdem sein Ergebnis. Also immer die **Zahl**
prüfen, nicht nur, dass ein Kommando durchgelaufen ist:

```bash
cargo test --workspace 2>&1 | grep -E "test result|^error" \
  | sed 's/.*ok\. \([0-9]*\) passed.*/GRUEN \1/' \
  | awk '/GRUEN/{s+=$2} /error|FAILED/{print} END{print "Rust gruen:", s}'
```

**Und: `git checkout <datei>` ist keine Gegenprobe-Rücknahme.** Bei einer
Gegenprobe wird eine Datei verändert und danach zurückgestellt — dafür
gehört eine Kopie angelegt (`cp datei /tmp/x.bak`) und zurückgespielt.
`git checkout` setzt auf den letzten Commit zurück und nimmt jede
uncommittete Arbeit mit. Genau das ist in Runde 20 passiert; gerettet hat
nur eine Sicherung von zehn Minuten vorher.

---

## 10. Nachtrag aus Runde 24: die Systemgrenze

**Eine Behauptung im Kommentar ist keine Prüfung.**

In Runde 21 stand im Webapp-Mapper:

> „Beim Touchdown-Publish ist er noch nicht bekannt. Der Wert kommt dann
> aus den `sub_scores` des PIREP, die `landingScoring.ts` ohnehin liest."

Der erste Satz stimmte. Der zweite war eine Annahme — der Mapper las das
Direktfeld, das nie ankommt. Die Korrektur wirkte im Pilot-Client und in
der Webapp **gar nicht**.

Die Kette, die das erzwingt:

1. Der Client publiziert den Touchdown, **bevor** die Bewertung läuft.
2. Der Recorder ergänzt später ausschliesslich `sub_scores`.
3. `LandingAnalysis` besitzt diese `sub_scores`.
4. Der Mapper ignorierte sie und las das nicht vorhandene Direktfeld.

> **Winkel 13: die Systemgrenze.** Für jeden Wert, der über eine
> Prozessgrenze geht — prüfen, ob er zu dem Zeitpunkt, an dem er
> übertragen wird, überhaupt existiert. Was erst nach dem Senden
> entsteht, kommt nie an.

Der Feldketten-Test war grün, weil er nur prüfte, **dass der Name
vorkommt** — nicht, aus welcher Quelle. Er prüft jetzt beides
(`liest Felder, die erst nach dem Publish entstehen, aus der richtigen
Quelle`), mit einer gepflegten Liste solcher Felder.

**Und wieder falscher Alarm beim ersten Anlauf:** Das Suchmuster für die
Publish-Stelle im Rust-Code hatte ein Fenster von 400 Zeichen — die
Begründung dazwischen ist länger. Der Test schlug an, obwohl der Code
richtig war. *Falscher Alarm ist so schädlich wie keiner.*

---

## 11. Nachtrag aus Runde 26: der Weg, den der Betrieb nimmt

**Ein Test, der einen anderen Weg nimmt als der Betrieb, prüft den
Betrieb nicht.**

Beim Herausziehen von `bahndisziplin_tick` aus `rollout_tick` landete der
Aufruf nur im Sampler-Pfad — und der kehrt bei `FlightPhase::Landing`
ausdrücklich zurück. Bei **jeder normalen Landung** wurde damit gar keine
Spur mehr aufgezeichnet, schlimmer als der Zustand davor.

Die Gegenprobe dazu lief über `TaxiIn` und war grün.

> **Winkel 14: der Hauptweg.** Wenn eine Funktion aus mehreren Pfaden
> gerufen wird, muss der Test den nehmen, den der Betrieb nimmt — nicht
> den, der sich am leichtesten aufbauen lässt. Im Zweifel den echten
> Einstieg fahren (`step_flight_at`, `handle`, `actions.*`), auch wenn
> dafür eine Fixture nötig ist.

Der Test, der über den echten Einstieg lief, fand sofort einen zweiten
Fehler: Der Tick, in dem das Messfenster schliesst, verwarf seine eigene
Position — `return` stand vor `spur_fortschreiben`.

**Das ist das Muster dieser ganzen QS:** Fast jeder Test, der eine Ebene
näher an den Betrieb rückte, fand etwas Neues.
