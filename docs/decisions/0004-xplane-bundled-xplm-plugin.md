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
{"p":2,"t":"flugzeug","icao":"A20N","titel":"A320neo","pfad":"Aircraft/…/a320.acf"}
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
  `sim/aircraft/view/acf_ICAO`, `acf_descrip`, `acf_relative_path`).
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
  - harte Obergrenzen für Zeilen, Namen, Abos, Paketgrößen.
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
  die Neusuche übernommen ist. Die tatsächlichen Kosten von `XPLMFindDataRef`
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

## Folgen

- **Positiv:** Kein Schein-Nullwert mehr; „fehlt“ ist eine Tatsache. Profil-
  Erkennung ohne Web-API. Doppelte Genauigkeit für die Position. „Flugzeug
  vermessen“ ohne Web-API-Paketverluste. Nur Loopback, nur Lesen.
- **Negativ:** Ein zweites Protokoll im Plugin (1 bleibt für alte Clients).
  Mehr C++-Code im X-Plane-Prozess — deshalb Parser-Tests, Obergrenzen und
  Zeitbudget als feste Bestandteile.
- **Nicht gelöst:** X-Plane auf einem anderen Rechner (weiter RREF/Web-API).
  Die Abnahme mit ToLiss und Zibo braucht Piloten mit diesen Flugzeugen.
