# ADR-0004: Eigenes X-Plane-Plugin als Dataref-Server (Protokoll 2)

- **Status:** Angenommen (ersetzt die Fassung vom 2026-05-01)
- **Datum:** 2026-09-29
- **Entschieden von:** Thomas Kant (Projektleitung), umgesetzt im Lernpaket AP7

## Kontext

Die erste Fassung dieser ADR (Mai 2026) beschrieb ein Rust-Plugin mit CBOR auf
Port 49021. Gebaut wurde etwas anderes: ein C++-Plugin (SDK 4.3), das 14 feste
Datarefs als JSON an `127.0.0.1:52000` schickt. Der Client wertet davon nur das
`touchdown`-Paket aus; alle übrigen Werte holt er über zwei Wege, die beide
nachweislich lügen oder schweigen:

| Weg | Problem (gemessen) |
|---|---|
| **RREF** (UDP 49000) | liefert für **jeden** Namen einen Wert, auch für nicht existierende → 0. Folge bis v1.9.3: jedes Flugzeug galt als CL650, Add-on-Felder standen auf `Some(0)`. Alle Werte kommen als `float32` (Breite/Länge ≈ 0,5 m Rundung). |
| **Web-API** (12.1+, Port 8086) | muss vom Piloten eingeschaltet werden; lehnt ein Abo-Paket **ganz** ab, wenn ein Name darin nicht geht (ToLiss-Messung verlor so still 500 Werte); fehlt bei X-Plane 11. |

Existenz eines Datarefs lässt sich heute nur über den Umweg „Wert per Web-API
lesbar“ feststellen. Ohne Web-API gilt eine Add-on-Quelle erst nach einem Wert
≠ 0 — ein Schalter, der den ganzen Flug auf 0 steht, bleibt unsichtbar.

### Vergleich: AcarsConnect (vmsACARS 3, Plugin 2.2.0)

Analysiert am 29.09.2026 (Binärdatei + dekompilierter Client):

| AcarsConnect | Bewertung für uns |
|---|---|
| Datarefs je Name registrieren (Gruppen „S“, „D“, „W“), Typ per `XPLMGetDataRefTypes`, Wert je nach Typ | **übernehmen** |
| Netz per `poll()` im Flight-Loop, alle XPLM-Aufrufe im Hauptthread | **übernehmen** |
| Versionsabfrage, Client verlangt Mindestversion | **übernehmen** |
| Bindet auf `0.0.0.0`/`::` ohne Authentifizierung — jedes Gerät im LAN kann lesen, pausieren und Flughafenabfragen im Hauptthread auslösen | **nicht übernehmen** |
| Client fragt alle 50 ms eine ganze Gruppe ab (ein Datagramm je Gruppe, Puffer 16 KiB) | **nicht übernehmen** — Plugin liefert selbst, in begrenzten Paketen |
| Szenerie (apt.dat, Custom Scenery, CIFP) wird beim Aktivieren **synchron im X-Plane-Prozess** eingelesen | **nicht übernehmen** — unser Client hat Navigraph-Daten und eigenen Szenerie-Leser |
| Client-Parser bricht bei einem unbekannten Namen ab und verwirft den Rest des Pakets | **nicht übernehmen** — jeder Name hat einen eigenen Status |
| Eigener Signal-Handler im X-Plane-Prozess für Absturzberichte | **nicht übernehmen**, siehe 6. |

## Entscheidung

Das vorhandene C++-Plugin wird zum **Dataref-Server** ausgebaut. Der Client
bleibt Herr über die Liste der Namen; das Plugin sucht, meldet, liefert.

### 1. Transport und Sicherheit

- UDP nur über **Loopback**. Das Plugin bindet einen Steuer-Socket auf
  `127.0.0.1:52001` und nimmt Anfragen **nur** von `127.0.0.1` an.
- Antworten und Lieferungen gehen an die Absenderadresse der letzten gültigen
  `HALLO`-Anfrage.
- Die bisherigen Pakete `telemetry` und `touchdown` (Protokoll 1) sendet das
  Plugin **unverändert weiter** an `127.0.0.1:52000` — ältere Clients laufen
  mit dem neuen Plugin wie bisher, der neue Client wertet `touchdown` weiter aus.
- **Kein** Schreiben von Datarefs, **keine** Kommandos. Das Plugin ist nur-lesend.
- Kein LAN-Betrieb. (Wer X-Plane auf einem anderen Rechner hat, nutzt weiter
  RREF/Web-API; das ist bewusst so.)

### 2. Format

Anfragen des Clients sind **Textzeilen** (kleiner, streng prüfbarer Parser im
Plugin, kein JSON-Parser im X-Plane-Prozess). Antworten sind **eine JSON-Zeile je
Datagramm**, höchstens **8 KiB**; größere Antworten werden geteilt und tragen
`teil`/`teile`.

Anfragen (UTF-8, Zeilenende `\n`, höchstens 64 KiB je Datagramm, jede Zeile
höchstens 512 Byte, Namen nur aus druckbarem ASCII ohne Leerzeichen):

```
HALLO <protokoll> <client-version>
ABO <abo-id> <rate-hz>
<name>
<name>[<index>]
…
ENDE-ABO <abo-id>
LISTE <anfrage-id>
PING
```

- `abo-id`: 1–16, `rate-hz`: 1–50. Ein neues `ABO` mit gleicher ID ersetzt das alte.
- Ein `ABO` darf sich über mehrere Datagramme erstrecken:
  `ABO <abo-id> <rate-hz> <teil> <teile>`; erst mit dem letzten Teil gilt es.
- Höchstens 8192 Namen je Abo, 16 Abos gleichzeitig.
- Array-Elemente als `name[i]`; der Index wird gegen die Array-Länge geprüft.

Antworten (Auswahl, alle mit `"p":2`):

```json
{"p":2,"t":"hallo","plugin":"1.0.0","xplane":12100,"xplm":430}
{"p":2,"t":"abo","abo":1,"teil":1,"teile":1,
 "st":[[0,"f",1],[1,"fehlt"],[2,"d",1],[3,"b",40],[4,"vf",8]]}
{"p":2,"t":"w","abo":1,"seq":812,"teil":1,"teile":1,"v":[[0,51.2345678],[2,8.5],[3,"A20N"],[4,[0,0,1]]]}
{"p":2,"t":"flugzeug","icao":"A333","titel":"Airbus long range widebody twin","ui_name":"Airbus A330-300","pfad":"Aircraft/Laminar Research/Airbus A330-300/A330.acf"}
{"p":2,"t":"liste","id":7,"teil":3,"teile":40,"n":["sim/…","…"]}
{"p":2,"t":"fehler","grund":"zeile_zu_lang"}
```

- Status je Name in Reihenfolge der Anmeldung: Typ (`i`, `f`, `d`, `vi`, `vf`,
  `b`) und Länge, oder `"fehlt"`. **Ein fehlender Name liefert nie einen Wert.**
- Werte als `[index, wert]`; `double` mit voller Genauigkeit, Byte-Arrays als
  JSON-String (bis zum ersten NUL, Steuerzeichen escaped).
- Bei mehreren Typen gilt: `double` > `float` > `int` > Arrays > Bytes.

### 3. Lebenszyklus

- Namen werden **beim Anmelden** und bei `XPLM_MSG_PLANE_LOADED` (Flugzeug 0)
  sowie `XPLM_MSG_AIRPORT_LOADED` neu gesucht. Ein bisher fehlender Name, den
  ein Flugzeug erst später registriert, wird **alle 2 s** erneut gesucht
  (höchstens 64 Suchen je Frame). Ändert sich ein Status, schickt das Plugin
  eine neue `abo`-Antwort.
- Beim Flugzeugwechsel schickt das Plugin `flugzeug` (aus
  `sim/aircraft/view/acf_ICAO`, `acf_descrip`, `acf_ui_name`,
  `acf_relative_path`; siehe §9).
- Das Plugin liefert nur, solange der Client lebt: ohne `PING`/Anfrage für 5 s
  werden alle Abos verworfen (ein abgestürzter Client lässt keine Last im Sim).
- Pause und Replay: Lieferung läuft weiter, `sim/time/paused` und
  `is_in_replay` stehen dem Client als normale Namen zur Verfügung. (Protokoll 1
  schweigt in der Pause wie bisher.)

### 4. Leistung

- Alles im Flight-Loop (Hauptthread). Netz nicht blockierend; höchstens
  64 eingehende Datagramme je Frame.
- **Zeitbudget:** Lesen und Senden höchstens **1 ms je Frame**. Ein großes Abo
  (Flugzeug vermessen, tausende Namen) wird über mehrere Frames verteilt.
  Das ist ein **weiches** Budget (siehe §9, Nachtrag Codex-Abnahme): einen
  fremden Accessor kann das Plugin nicht unterbrechen.
- Keine Allokation im heißen Pfad außer beim (Neu-)Anmelden.

### 5. Aufzählung für „Flugzeug vermessen“

`LISTE` liefert alle Dataref-Namen über `XPLMCountDataRefs` /
`XPLMGetDataRefsByIndex` / `XPLMGetDataRefInfo` (XPLM 4.0+, X-Plane 12). Bei
älterem X-Plane antwortet das Plugin mit `fehler/liste_nicht_verfuegbar`, der
Client nimmt dann wie bisher die Web-API.

### 6. Absturzschutz

- **Kein eigener Signal-Handler.** X-Plane hat einen eigenen Crash-Reporter, der
  das schuldige Modul nennt; ein zusätzlicher Handler eines Plugins kann den der
  anderen Plugins und X-Planes stören. Stattdessen:
  - `-fno-exceptions`, jede XPLM-Rückgabe geprüft, keine rohen Indizes;
  - der Anfrage-Parser und der JSON-Schreiber sind reine Funktionen mit
    **eigenen Unit-Tests und einem Fuzz-Test** (Plugin-CI, alle drei Plattformen);
  - harte Obergrenzen für Zeilen, Namen, Abos, Paketgrößen und — seit der
    Codex-Abnahme — Bytes (§9).
- **Startschutz:** Misslingt das Binden des Steuer-Sockets (Port belegt), bleibt
  das Plugin beim Protokoll-1-Senden und schreibt eine Zeile ins X-Plane-Log.

### 7. Client

- Neuer Transport im `sim-xplane`-Adapter. **Rangfolge:** Plugin (Protokoll 2)
  → Web-API (Existenz, Flugzeug, Vermessung) → RREF (Werte).
- Antwortet das Plugin auf `HALLO` mit `p=2`, übernimmt es **Werte und
  Existenz**: RREF-Abos werden abbestellt, die Existenzprüfung per Web-API
  entfällt (`addon_vorhanden` kommt aus dem Status je Name).
- Bleibt das Plugin 3 s stumm, fällt der Client ohne Zutun auf Web-API/RREF
  zurück und meldet sich alle 5 s erneut mit `HALLO`.
- Unpassende Protokollversion → Client nutzt das Plugin nur für Protokoll 1 und
  zeigt „Plugin veraltet“ an.

### 8. Verteilung

- Die Release-Pipeline baut das Plugin **vor** der App, berechnet die
  SHA-256-Prüfsumme des Plugin-Pakets und gibt sie der App beim Bauen mit. Der
  Client prüft das heruntergeladene Paket gegen diese Prüfsumme und installiert
  nur bei Übereinstimmung.
- macOS: `mac.xpl` wird ad-hoc signiert (`codesign -s -`); der Client entfernt
  nach dem Entpacken `com.apple.quarantine`. Ob X-Plane das ohne
  Apple-Developer-ID ohne Warnung lädt, wird am Mac mit X-Plane 12 geprüft.

### 9. Umsetzung im Plugin 1.0.0 — Präzisierungen (AP7, 29.09.2026)

Die Punkte 1–6 sind wie beschrieben umgesetzt. Wo die ADR offen war, gilt
(ausführlich in `xplane-plugin/README.md`, „Protokoll 2“):

- **`PING` wird beantwortet** mit `{"p":2,"t":"pong"}` — sonst hätte ein
  Client ohne Abo kein Lebenszeichen des Plugins für seine 3-s-Regel.
- **Genau ein Befehl je Datagramm**; nur `ABO` hat Folgezeilen. `\r\n` wird
  toleriert.
- **Fehler** tragen optional `zeile` (1-basiert), `abo` + `gen` (immer
  zusammen), `id`. Gründe: `leere_anfrage`, `datagramm_zu_gross`,
  `zeile_zu_lang` (Befehlszeile), `unbekannter_befehl`, `falsche_argumente`,
  `protokoll_ungueltig`, `abo_id_ungueltig`, `rate_ungueltig`,
  `teil_ungueltig`, `generation_ungueltig`, `zu_viele_namen`,
  `ueberzaehlige_zeilen`, `kein_hallo`, `abo_teil_reihenfolge`,
  `abo_teile_widerspruch`, `keine_namen`, `liste_nicht_verfuegbar`,
  `speicher`.
- **Mehrteiliges ABO:** Teile strikt in Reihenfolge; Teil 1 beginnt immer neu;
  Rate, Teilezahl und Generation müssen in allen Teilen gleich sein.
- **Anmeldung:** Nur `HALLO 2` meldet an (andere Nummern bekommen trotzdem die
  `hallo`-Antwort). `HALLO` von einem anderen Port verwirft die Abos des
  alten Clients. Nach 5 s Stille wird der Client auch **vergessen** —
  weitere Anfragen bekommen `kein_hallo`, damit ein nur hängender Client merkt,
  dass seine Abos weg sind. Antworten auf Anfragen Fremder (`kein_hallo`,
  Syntaxfehler) gehen an deren Absender.
- **Arrays:** ganze Arrays höchstens 256 Elemente, Byte-Arrays höchstens
  1024 Byte; die Länge im Status ist die gelieferte Länge. Unter Arrays gilt
  `vf` vor `vi`. `name[i]` liefert `f` (aus `vf`) bzw. `i` (aus `vi` oder ein
  Byte aus `b`); Index außerhalb der Länge oder Name ohne Array-Typ → `fehlt`.
- **Verwaiste Datarefs** (Plugin des Flugzeugs entladen; `XPLMFindDataRef`
  findet sie weiter, lesen ergäbe 0) gelten als `fehlt`
  (`XPLMIsDataRefGood`) — siehe Nachtrag Cloud-QS unten.
- **Statuswechsel:** Ergebnisse einer Prüfung werden erst zwischen zwei
  Lieferrunden übernommen; bei Änderung geht zuerst die vollständige neue
  `abo`-Antwort hinaus, dann wieder Werte.
- **Lieferung:** `seq` zählt je Abo. Große Abos verteilt das Zeitbudget über
  mehrere Frames; die Werte einer Runde können aus aufeinanderfolgenden Frames
  stammen. Sind alle Namen `fehlt`, kommt je Runde `"v":[]`.
  Höchstens 16 Pakete je Frame (der Client sollte `SO_RCVBUF` ≥ 1 MiB setzen).
- **`flugzeug`** kommt auch direkt nach jedem `HALLO`.
- **`flugzeug`-Namen (Nachtrag 29.09.2026, Format nach Cloud-QS):** `titel`
  bleibt in jeder Plugin-Version `acf_descrip` (Profilerkennung und
  Buchungsabgleich hängen daran). Neu ist das Feld `ui_name` =
  `sim/aircraft/view/acf_ui_name` (b[250], „ACF name as seen in the UI“, bis
  NUL, Leerraum am Rand entfernt, nur wenn nicht leer) — derselbe Name, den
  der Flugzeug-Scan des Clients nimmt. `acf_descrip` ist bei vielen Add-ons
  eine Beschreibung (ToLiss: „A320 with high fidelity system modelling“ statt
  „ToLiSs A320 Hi Def“), Messung und Scan passten so nie zusammen. Live
  gemessen (X-Plane 12.4.3, Laminar A330): `acf_ui_name` „Airbus A330-300“,
  `acf_descrip` „Airbus long range widebody twin“. Die zuerst gebaute Form
  (`titel` = UI-Name, `beschreibung` = descrip) ließ den Titel bei leerer
  Beschreibung oder mit einem älteren Client still auf den UI-Namen kippen.
  Die Kennung liest damit vier Datarefs, je eine Einheit unter dem
  Such-Budget.
- **`LISTE`** meldet nur abonnierbare Namen; eine neue `LISTE` ersetzt eine
  laufende. Die XPLM-4.0-Funktionen werden per `XPLMFindSymbol` geholt (nur
  bei XPLM ≥ 400), damit das Plugin unter X-Plane 11 weiter lädt.
- **Takt:** Solange Protokoll 2 nichts liefert, gibt der Flight-Loop wie bisher
  das Intervall von Protokoll 1 zurück; mit aktiven Abos läuft er jeden Frame,
  Protokoll 1 wird dann über die Uhr auf seinen eigenen Takt gedrosselt.
- **macOS:** Mindestversion der `mac.xpl` fest 11.0 (vorher ungesetzt = die
  SDK-Version des Build-Rechners).

#### Nachtrag nach der Cloud-QS (29.09.2026, verbindlich für Plugin und Client)

- **Generation:** Jede ABO-Kopfzeile trägt als LETZTES Wort `g<zahl>`
  (1 … 2^31 − 1), z. B. `ABO 3 5 1 2 g17`, `ABO 1 50 g4`. Das Plugin gibt
  `"gen"` in jeder `abo`-, `w`-, `abo_empfangen`- und abo-bezogenen
  `fehler`-Antwort zurück. Ohne g-Wort gilt `gen` = 0.
- **Gleiches ABO** (gleiche ID, Rate, Namensliste, Generation) setzt nichts
  zurück: eine laufende Suche läuft weiter, ein schon gesendeter Status geht
  noch einmal hinaus. (Vorher begann jedes erneute ABO die Suche bei 0 — mit
  der Client-Regel „nach 2 s neu senden“ bekamen 8192 Namen nie einen Status.)
- **`abo_empfangen`:** Jedes angenommene ABO (auch ein gleiches) wird sofort
  mit `{"p":2,"t":"abo_empfangen","abo":N,"gen":G,"namen":K}` bestätigt. Grund:
  ein einzelnes 8192er-Abo hat seinen Status zwar in ~0,6 s, sechs davon
  nacheinander aber erst nach bis zu ~3,7 s (gemessen, Schein-Welt 30 fps).
  Der Client wartet nach einer Bestätigung auf den Status, statt neu zu senden.
- **Einzelne ungültige Namen** (leer, > 512 Byte, Leerzeichen, Nicht-ASCII,
  kaputter Index) verwerfen nicht mehr das ganze Abo, sondern bekommen
  `"fehlt"`; die Zeile zählt normal mit, die Indizes bleiben stabil. Fehler
  für das ganze Abo nur bei Rahmenfehlern (Kopfzeile, zu viele Namen, Teile,
  Datagramm zu groß). `LISTE` meldet nur Namen, die als ganzer Dataref
  abonnierbar sind (dieselbe Prüfung wie der Parser, zusätzlich nicht auf `]`
  endend).
- **`ENDE-ABO`** hat nie eine Antwort, auch für eine ID ohne Abo (kein Fehler).
- **Suche mit eigenem Zeitbudget:** 0,3 ms je Frame (vorher feste 64 Suchen
  je Frame), getrennt vom Liefer-Budget (1 ms). Reihenfolge: dringende Suchen
  (Anmeldung, Flugzeug-/Flughafenwechsel) vor den periodischen, darunter das
  Abo mit dem kleinsten Rest zuerst. Der 2-s-Durchlauf prüft nur fehlende
  Namen und Arrays (Länge).
- **Lieferung:** Abo 1 (beim Client die Telemetrie) zuerst, dann Rundlauf nur
  über die belegten übrigen Abos (vorher über alle 16 Plätze; höhere IDs
  verhungerten hinter einem großen Abo).
- **Verwaist beim Lesen:** Vor jedem Lesen eines Namens, der nicht mit `sim/`
  beginnt, prüft das Plugin `XPLMIsDataRefGood`; ist er verwaist, fällt der
  Wert aus und der Status geht sofort auf `fehlt`. Nach
  `XPLM_MSG_PLANE_LOADED` pausieren Abos mit solchen Namen die Lieferung, bis
  die Neusuche übernommen ist, höchstens aber 0,5 s (Nachprüfung: viele
  8192er-Mess-Abos lagen sonst über 15 s still; kleine Abos wie Abo 1/2 sind
  durch „kürzester Rest zuerst“ vorher fertig, große schützt danach die
  Prüfung beim Lesen). Die 0,5 s zählen ab dem ersten Frame nach der
  Meldung, nicht ab der Meldung selbst — X-Plane lädt danach oft Sekunden
  ohne Frame. Die Pause ist eine Zusatzsicherung: XPLM zerstört Datarefs nie,
  `XPLMIsDataRefGood` ist genau dann wahr, wenn ein Plugin den Namen gerade
  bereitstellt. Die tatsächlichen Kosten von `XPLMFindDataRef`
  und `XPLMIsDataRefGood` misst das Plugin beim Start und schreibt sie ins
  `Log.txt`.
- **`pv` in Protokoll 1:** `telemetry` und `touchdown` tragen zusätzlich
  `"pv":"<Plugin-Version>"`. So erkennt der Client ein 1.0-Plugin auch, wenn
  dessen Protokoll 2 nicht antwortet (Port 52001 belegt), und meldet nicht
  „Plugin veraltet“. Sonst bleibt Protokoll 1 unverändert.
- **Client-Regel `laminar/*`:** X-Plane registriert `laminar/B738/*` (und
  andere `laminar/…`-Namen) bei jedem Flugzeug. Ein Status „da“ ist dort
  also kein Beleg für das Flugzeug; der Client braucht einen Zweitbeleg
  (Wert ≠ 0, Web-API oder RREF-Bestätigung), bevor er daraus ein Profil
  ableitet.

#### Nachtrag nach der Codex-Abnahme (29.09.2026, verbindlich)

Unabhängige Abnahme durch Codex („nicht freigabefähig“), Befunde H2–H5,
M1–M3, N1 im Plugin behoben (H1 betrifft den Client):

- **Flugzeugwechsel mitten in einer Runde (H2):** `XPLM_MSG_PLANE_LOADED`
  verwirft eine laufende Runde (seit der dritten Nachprüfung bei allen Abos): `LESEN`/`SENDEN`
  → `BEREIT` (Stapel und Paketcursor zurück, schon geplante Pakete gehen
  nicht mehr hinaus), `ANTWORT` → `NEU` mit erzwungener vollständiger neuer
  Status-Antwort nach der Neusuche; in `NEU` gibt es keine Werte. Danach gilt
  die Pause wie bisher (bis zur Übernahme der Neusuche, höchstens 0,5 s).
  Werte des alten Flugzeugs gehen nach dem Wechsel nie mehr hinaus, und eine
  Runde mischt nie beide. Seit der dritten Nachprüfung gilt das Verwerfen
  für ALLE Abos; Abos nur mit `sim/…`-Namen pausieren nicht, sondern
  beginnen sofort eine neue Runde.
- **Speicher (H3):** harte Bytebudgets — je Abo 16 MiB, alle Abos samt
  Teil-Abos 64 MiB, LISTE 16 MiB — gezählt als reservierte Kapazität mit dem
  schlimmsten Fall je Wert und je Eintrag (Duplikate zählen einzeln).
  Überschreitung → `fehler`/`speicher_limit` (mit `abo`+`gen` bzw. `id`);
  ein fertiges ABO, das scheitert, hinterlässt die ID leer (ein Aufbaufehler
  beim Empfang eines Teils lässt wie immer das bisherige Abo stehen). Warum 16/64 statt der zuerst
  erwogenen 8/32: Der Client vermisst mit bis zu 14 Abos × 8192 Namen, und
  LISTE liefert Namen gruppiert — ein Abo kann Tausende ganzer 256er-Arrays
  enthalten (3400 davon ≈ 15 MiB). Gemessen in der Schein-Welt: Abo 1 +
  14 × 8192 typische Namen ≈ 47 MiB. Vorher war der schlimmste Fall
  unbegrenzt (16 × 48 MiB).
- **Zeit (H4):** Die Uhr wird nach **jedem einzelnen** XPLM-Aufruf gelesen
  (Find, IsDataRefGood, Types, Array-Länge, Getter; LISTE je Name statt je
  256er-Block). Ist das Budget erschöpft, endet die Arbeit vor dem nächsten
  Aufruf; ein angefangener Eintrag beginnt im nächsten Frame neu (auch die
  Verwaist-Prüfung gilt nur im Moment ihres Aufrufs). Frei ist je Budget nur
  die erste Einheit bis einschließlich ihres einen fremden Accessors.
  **Ehrliche Einordnung:** Das bleibt ein weiches Budget. XPLM ruft fremde
  Accessoren synchron im Hauptthread auf; eine harte Grenze wäre nur mit
  Isolation (eigener Prozess) möglich, die XPLM nicht bietet. Ein Dataref,
  dessen Accessor dreimal **in Folge** länger als 2 ms braucht, wird
  gedrosselt: höchstens einmal je Sekunde gelesen, über alle Abos höchstens
  ein gedrosselter Zugriff je 0,2 s, im 2-s-Nachsuchlauf übersprungen; eine
  Zeile je Name ins `Log.txt` (höchstens 32). Entschieden gegen „aus der
  Lieferung nehmen mit `fehlt`“: `fehlt` heißt „existiert nicht“, der Client
  würde daraus falsche Profilschlüsse ziehen. Der Client behält bei einer
  Runde ohne diesen Index den letzten Wert. Nach einem Flugzeugwechsel wird
  neu bewertet.
- **Sockets (H5):** beide Sockets richtet eine Funktion ein (`netz::oeffne`),
  jede Rückgabe geprüft. „Nicht blockierend“ gescheitert → Socket zu,
  Protokoll aus, Log. Windows: `SO_EXCLUSIVEADDRUSE` gescheitert → Protokoll
  2 aus (sonst könnte ein anderes Programm mitbinden); `SIO_UDP_CONNRESET`
  gescheitert → nur Warnung. Die Betriebssystemaufrufe liegen hinter einer
  Schnittstelle; alle Fehlerpfade sind auf jeder Plattform getestet, die
  echten Aufrufe zusätzlich auf allen drei (auch Windows).
- **Ausgang (M1):** Alle Sendewege teilen sich die 16 Pakete je Frame
  (gezählt über einen Flight-Loop-Aufruf: Empfang + Lieferung). Kleine
  Einzelantworten haben Vorrang, höchstens 8 je Frame; der Überschuss wird
  verworfen und alle 10 s summiert protokolliert. `flugzeug` zählt mit.
  Fehler aus der Lieferung werden vorgemerkt und im nächsten Frame zuerst
  gesendet — nie verworfen.
- **LISTE (M2):** Teilnehmer im Rundlauf der Lieferung wie ein Abo (nach
  Abo 1), bekommt also regelmäßig das ganze Budget.
- **Protokoll 1 (M3):** `telemetry` und `touchdown` schreibt der locale-feste
  JSON-Schreiber; Feldschema, Reihenfolge und Nachkommastellen unverändert
  (für endliche Werte Byte für Byte die alte Ausgabe), NaN/±Inf → `null`.
  Hinweis für den Client: seine Protokoll-1-Felder sind `f32` mit
  `serde(default)` — `null` lässt dort das Paket scheitern (wie vorher `nan`,
  nur jetzt als gültiges JSON erkennbar); robuster wäre `Option`/NaN-Default
  im Client.
- **Aktivieren/Deaktivieren (N1):** Netz nur im aktivierten Zustand.
  `XPluginDisable` verwirft Client/Abos/LISTE und schließt beide Sockets
  (Port frei, Warteschlange weg), `XPluginEnable` bindet neu. Die
  Aufsetz-Erkennung von Protokoll 1 synchronisiert sich im ersten Tick nach
  Enable (siehe „Zweite Nachprüfung“).
- **Neuer Fehlergrund:** `speicher_limit`.
- **Zweite Nachprüfung (Codex, 29.09.2026):**
  - *Drosselung je Dataref:* Der Langsam-Zustand gehört dem Handle, nicht
    dem Eintrag (vorher begannen 8192 Duplikate je bei 0, die Drosselung
    griff nie). Feste Tabelle mit offener Adressierung, 1024 Plätze, höchstens
    768 verfolgt (danach eine Log-Zeile, weitere werden nicht gedrosselt);
    ein schneller Aufruf räumt den Platz eines nicht gedrosselten Handles
    (Rückwärtsverschiebung, keine Grabsteine). Keine Allokation.
  - *Keine ungebudgetierten XPLM-Wege mehr im Frame:* `XPLMCountDataRefs`
    nicht mehr beim Empfang, sondern als erster budgetierter LISTE-Schritt;
    die `flugzeug`-Kennung fortsetzbar (eine Einheit je Dataref, Senden
    eigene Einheit) unter dem Such-Budget und vor der Suche; Empfang mit
    eigenem Zeitbudget 0,5 ms je Frame (Uhr nach jedem Datagramm samt
    Antwort). Bewusst ausgenommen bleiben nur: die Kostenmessung beim Start
    (einmalig ≈ 1–3 ms, Log.txt) und die Bearbeitung EINES Datagramms
    (Parser ≤ 64 KiB, ggf. Speicher für einen ABO-Teil) — sie wird nie
    mittendrin abgebrochen.
  - *Abo 1 mit Teilbudget* (0,5 ms); der Rundlauf bekommt den Rest samt
    eigener freier Einheit — ein großes Abo 1 hungert LISTE und Mess-Abos
    nicht mehr aus. Überschreitung je Frame damit höchstens zwei fremde
    Aufrufe (je eine freie Einheit).
  - *Vorgemerkte Fehler* über den gemeinsamen Ausgang; bei VOLL bleiben sie
    stehen.
  - *Protokoll 1 nach Enable:* Der erste Tick synchronisiert (Bodenzustand
    übernehmen, Ringpuffer und Tracker neu, keine Kante). Das ersetzt die
    Regel „Aufsetz-Erkennung bleibt über Disable/Enable stehen“ oben.
    Grenzen siehe dritte Nachprüfung.
- **Dritte Nachprüfung (Claude, 29.09.2026; Live-Messung X-Plane 12.4.3:
  XPLMFindDataRef 0,12 µs, IsDataRefGood 0,01 µs — langsame Accessoren sind
  die Ausnahme fehlerhafter Add-ons; die Drosselung ist ein Schutz und darf
  im Normalfall nichts verschlechtern und nie Werte verhungern lassen):**
  - *Kein Verhungern:* Ein fälliger gedrosselter Handle wird mit EINEM
    Getter-Aufruf gelesen, der ALLE seine Einträge der Runde bedient (Arrays
    über ein Fenster ≤ 256 Elemente bzw. 1024 Byte, weiter auseinander
    liegende Indizes reihum; Skalar- und Array-Zugriffe desselben Handles
    abwechselnd). Die 0,2-s-Sperre geht an den am längsten wartenden
    gedrosselten Handle (kleinstes `faellig` unter denen, die eine Runde in
    den letzten 2 s fällig angetroffen hat).
  - *Suche sparsam:* Array-Länge je Handle einmal je Prüfdurchlauf;
    gedrosselte Handles mit bekannter Länge werden auch in dringenden Läufen
    nur gefragt, wenn sie ohnehin dran sind.
  - *Höchstens zwei langsame fremde Aufrufe je Frame:* Jede Phase (Suche,
    Abo 1, Rundlauf) behält Budget und freie erste Einheit; nach zwei
    fremden Aufrufen > 2 ms beginnt im Frame kein fremder Accessor mehr
    (vorher bis zu drei). Eine Zwischenfassung mit Frame-Grenze nach der Uhr
    (Frame-Start + 1,3 ms) ließ einen langsamen Such-Aufruf die Telemetrie
    von Abo 1 verdrängen (Nachweis H: 5–11 Hz statt 20 Hz) und wurde
    ersetzt.
  - *Kein doppelter Index:* Ein regulär gelesener Eintrag gilt für die Runde
    als bedient; wird sein Handle später in derselben Runde gedrosselt und
    fällig, bedient ihn der gedrosselte Aufruf nicht noch einmal.
  - *Hysterese:* Fünf schnelle gedrosselte Lesungen in Folge heben die
    Drosselung auf (Log einmal).
  - *Kennung:* zwischen `XPLMFindDataRef` und dem Lesen noch einmal gegen das
    Budget.
  - *Ersatz-ABO:* Das alte Abo derselben ID zählt beim Aufbau des Ersatzes
    nicht mit (es wird ersetzt). Bis zum letzten Teil belegen altes Abo und
    Aufbau zusammen vorübergehend höchstens 16 MiB über dem Gesamtbudget.
  - *Protokoll-1-Synchronisation, Grenzen:* Ein Aufsetzen genau im
    Synchronisations-Tick wird nicht gemeldet (ein Tick; der Ringpuffer wäre
    leer). Der Wegfall des Schein-Touchdowns beim Laden am Boden gilt nur,
    wenn `fnrml_gear` schon im ersten Tick ≥ 1 N ist; meldet X-Plane dort
    noch 0 N, bleibt es beim bisherigen Verhalten (Kante im nächsten Tick).
- **H1 (Plugin-Seite geprüft):** `abo`-Antworten tragen `teil`/`teile`
  konsistent (gleiches `teile` in allen Teilen), und ein identisches `ABO`
  liefert den Status vollständig erneut (alle Teile) — das braucht der
  Client, um ein verlorenes Fragment nachzufordern.

## Folgen

- **Positiv:** Kein Schein-Nullwert mehr; „fehlt“ ist eine Tatsache. Profil-
  Erkennung ohne Web-API. Doppelte Genauigkeit für die Position. „Flugzeug
  vermessen“ ohne Web-API-Paketverluste. Nur Loopback, nur Lesen.
- **Negativ:** Ein zweites Protokoll im Plugin (1 bleibt für alte Clients).
  Mehr C++-Code im X-Plane-Prozess — deshalb Parser-Tests, Obergrenzen und
  Zeitbudget als feste Bestandteile.
- **Nicht gelöst:** X-Plane auf einem anderen Rechner (weiter RREF/Web-API).
  Die Abnahme mit ToLiss und Zibo braucht Piloten mit diesen Flugzeugen.
