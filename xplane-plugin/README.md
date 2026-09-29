# AeroACARS X-Plane Plugin

Native X-Plane plugin (XPLM SDK 4.3.0, C++17) that pairs with the
**AeroACARS** desktop ACARS client (v0.5.0+) to provide frame-perfect
telemetry — most importantly, frame-perfect **touchdown landing-rate
capture**.

| Field          | Value                                                   |
|----------------|---------------------------------------------------------|
| Plugin name    | `AeroACARS Premium`                                     |
| Version        | 1.0.0 (Protokoll 2 = Dataref-Server, ADR-0004)          |
| Signature      | `com.aeroacars.xplane.premium`                          |
| Wire format    | Line-delimited JSON over UDP loopback                   |
| Loopback port  | `127.0.0.1:52000` Protokoll 1 (Plugin → Client)         |
| Steuer-Port    | `127.0.0.1:52001` Protokoll 2 (Client ↔ Plugin)         |
| Min X-Plane    | 11.50 (XPLM303); `LISTE` ab X-Plane 12 (XPLM 4.0)       |
| Min macOS      | 11.0 (Big Sur)                                          |
| Platforms      | Windows x64 · macOS universal (x86_64+arm64) · Linux x64 |
| License        | Same as the SDK (BSD) — free for commercial use         |

The plugin is **strictly optional**. AeroACARS without it works exactly
as before — the standard X-Plane RREF UDP integration on port 49000
covers every flight. Installing the plugin upgrades the touchdown
detection from "polled at 5 s cadence" to "captured in the exact frame
of wheel contact, with 500 ms lookback for peak descent VS".

## Why a plugin

The RREF UDP protocol is excellent for general telemetry but has two
characteristics that make it unsuitable for landing-rate capture:

1. **Cadence:** the AeroACARS streamer ticks every 5 s, so by the time
   we detect the on-ground edge the buffer of pre-touchdown samples
   has already been evicted.
2. **Smoothing:** `vh_ind_fpm` is the cockpit-display VS, smoothed for
   readability — its value at touchdown is closer to "what the
   pilot's eyes see" than to "what the airframe actually did".

The plugin runs *inside* X-Plane's flight loop. It reads
`fnrml_gear` (gear normal force, in Newtons) every frame, captures
the touchdown edge with frame-perfect timing, and back-references a
500 ms lookback ring buffer to find the peak descent VS — pitch-
corrected to the body axis, just like the established `xgs` plugin.

The result is then fired off as a one-shot UDP "touchdown" packet to
the desktop client, which uses it in preference to its own
RREF-derived edge detection.

## Protokoll 2 — Dataref-Server (ab Plugin 1.0.0)

Verbindliche Spezifikation: `docs/decisions/0004-xplane-bundled-xplm-plugin.md`.
Hier die Kurzfassung plus alles, was die Umsetzung genauer festlegt.

Der Client bleibt Herr über die Namensliste; das Plugin sucht, meldet den
Status je Name und liefert. **Ein fehlender Name liefert nie einen Wert** —
„fehlt" ist eine Tatsache, kein Schein-Nullwert wie bei RREF.

### Transport

* UDP, nur Loopback. Steuer-Socket `127.0.0.1:52001`; Anfragen von anderen
  Adressen als `127.0.0.1` werden verworfen. Antworten kommen von Port 52001.
* Antworten und Lieferungen gehen an die Adresse (IP + Port) des letzten
  gültigen `HALLO 2 …`. Ein `HALLO` von einem anderen Port (Client neu
  gestartet) verwirft alle Abos des alten.
* Nur lesend: kein Schreiben von Datarefs, keine Kommandos.

### Anfragen (Textzeilen)

```
HALLO <protokoll> <client-version>
ABO <abo-id> <rate-hz> [<teil> <teile>] [g<generation>]
<name>
<name>[<index>]
…
ENDE-ABO <abo-id>
LISTE <anfrage-id>
PING
```

| Regel | Wert |
|---|---|
| Befehle je Datagramm | genau einer; nur `ABO` hat Folgezeilen |
| Datagramm | ≤ 65536 Byte, sonst `datagramm_zu_gross` |
| Zeile | ≤ 512 Byte ohne Zeilenende; `\n`, ein `\r` davor wird toleriert |
| Trennung | genau ein Leerzeichen zwischen Argumenten |
| Zahlen | nur Dezimalziffern, höchstens 10 Stellen |
| Namen | 1–512 Byte druckbares ASCII `0x21–0x7E` (kein Leerzeichen, kein UTF-8) |
| Index | Name endet auf `]` → `name[<ziffern>]`, 0 … 2147483647 |
| ungültiger Name (leer, zu lang, Nicht-ASCII, kaputter Index) | zählt mit, Status `fehlt` — das Abo bleibt |
| Generation `g<n>` | optional, LETZTES Wort der ABO-Zeile, 1 … 2147483647; ohne = 0 |
| `abo-id` / `rate-hz` | 1–16 / 1–50 |
| Namen je Abo | ≤ 8192 (über alle Teile) |
| `teil` / `teile` | 1 ≤ teil ≤ teile ≤ 8192; Teile in Reihenfolge 1, 2, 3 … |
| `anfrage-id` (LISTE) | 0 … 2147483647 |

* Ein `ABO` mit gleicher ID ersetzt das alte — erst mit dem letzten Teil.
  Ein Teil 1 beginnt immer neu. Fehlender, doppelter oder vertauschter Teil →
  `abo_teil_reihenfolge`, andere Rate/Teilezahl/Generation als in Teil 1 →
  `abo_teile_widerspruch`; der ganze Aufbau ist dann verworfen.
* **Gleiches ABO** (gleiche ID, Rate, Generation und Namensliste) setzt
  **nichts** zurück: eine laufende Suche läuft weiter, ein schon gesendeter
  Status geht noch einmal hinaus. Neue Generation = neues Abo.
* Jedes angenommene ABO (auch ein gleiches) wird sofort mit `abo_empfangen`
  bestätigt; der Status folgt, sobald alle Namen gesucht sind.
* `ENDE-ABO` hat keine Antwort — auch nicht für eine ID ohne Abo (kein Fehler).
* Jede Anfrage außer `HALLO` braucht vorher ein `HALLO 2 …` vom selben
  Absender, sonst `kein_hallo`. Ein `HALLO` mit anderer Protokollnummer wird
  beantwortet (`"p":2`), meldet den Client aber nicht an.
* **Lebenszeichen:** Ohne Datagramm des Clients für 5 s verwirft das Plugin
  alle Abos und vergisst den Client (weitere Anfragen → `kein_hallo`). Der
  Client sollte also etwa jede Sekunde `PING` schicken.

### Antworten (eine JSON-Zeile je Datagramm, ≤ 8192 Byte inkl. `\n`)

```json
{"p":2,"t":"hallo","plugin":"1.0.0","xplane":12100,"xplm":430}
{"p":2,"t":"pong"}
{"p":2,"t":"abo_empfangen","abo":1,"gen":4,"namen":4}
{"p":2,"t":"abo","abo":1,"gen":4,"teil":1,"teile":1,"st":[[0,"d",1],[1,"fehlt"],[2,"vf",8],[3,"b",40]]}
{"p":2,"t":"w","abo":1,"gen":4,"seq":812,"teil":1,"teile":1,"v":[[0,51.234567890123449],[2,[0,0,1]],[3,"A20N"]]}
{"p":2,"t":"flugzeug","icao":"A20N","titel":"A320neo","pfad":"Aircraft/…/a320.acf"}
{"p":2,"t":"liste","id":7,"teil":3,"teile":40,"n":["sim/…","…"]}
{"p":2,"t":"fehler","grund":"rate_ungueltig","zeile":1,"abo":1,"gen":4}
```

* **Status** (`st`) je Name in Anmeldereihenfolge: `[k,"i"|"f"|"d",1]`,
  `[k,"vi"|"vf"|"b",länge]` oder `[k,"fehlt"]`. Bei mehreren Typen:
  `d` > `f` > `i` > `vf` > `vi` > `b`.
* **Array-Element** `name[i]`: Status `f` (aus `vf`), `i` (aus `vi`) oder `i`
  (ein Byte aus `b`, 0–255). Index ≥ aktuelle Länge oder Name ohne Array-Typ →
  `fehlt` (wird alle 2 s neu geprüft).
* **Ganze Arrays** werden mit höchstens **256** Elementen, Byte-Arrays mit
  höchstens **1024** Byte geliefert; die Länge im Status ist die gelieferte
  Länge (für längere Arrays einzelne Elemente abonnieren). Byte-Arrays kommen
  als JSON-Text bis zum ersten NUL.
* **Werte** (`v`): `[k, wert]`; `double` mit `%.17g` (verlustfrei), `float`
  mit `%.9g` (verlustfrei), NaN/±Inf → `null`. Zahlen sind locale-fest
  (immer `.`). Texte sind immer gültiges UTF-8: gültige Folgen unverändert,
  jedes andere Byte als `\u00XX` (Latin-1 gelesen), Steuerzeichen maskiert.
* **`seq`** zählt je Abo die Lieferrunden. Eine Runde kann mehrere Pakete
  haben (`teil`/`teile`); jedes Element steht ganz in genau einem Paket. Bei
  großen Abos verteilt das Zeitbudget das Lesen auf mehrere Frames — die
  Werte einer Runde stammen dann aus aufeinanderfolgenden Frames.
* Sind alle Namen eines Abos `fehlt`, kommt trotzdem je Runde ein Paket mit
  `"v":[]` (Lebenszeichen).
* **Flugzeugwechsel mitten in einer Runde:** `XPLM_MSG_PLANE_LOADED` verwirft
  bei jedem Abo eine laufende Lese- oder Senderunde sofort (Abos nur mit
  `sim/…`-Namen beginnen gleich eine neue, Abos mit Plugin-Namen pausieren
  bis zur Neusuche, höchstens 0,5 s) — Werte, die vor dem Wechsel gelesen wurden, gehen nie
  mehr hinaus, und eine Runde mischt nie altes und neues Flugzeug. Der Client
  sieht dann höchstens eine unvollständige Runde (wie bei UDP-Verlust). War
  gerade eine `abo`-Antwort unterwegs, kommt nach der Neusuche eine
  vollständige neue, und bis dahin keine Werte.
* **Suche und Statuswechsel:** Alle Namen werden beim Anmelden, bei
  `XPLM_MSG_PLANE_LOADED` (Flugzeug 0) und bei `XPLM_MSG_AIRPORT_LOADED`
  gesucht; alle 2 s werden fehlende Namen und Arrays (Länge) nachgeprüft.
  Gesucht wird in einem eigenen Zeitbudget (0,3 ms je Frame), dringende
  Suchen (Anmeldung, Wechsel) vor periodischen, darunter das Abo mit dem
  kleinsten Rest zuerst — ein kleines Abo hat seinen Status nach einem Frame,
  8192 Namen brauchen gemessen ~0,6 s bei 30 fps (Schein-Welt mit 0,5 µs je
  Suche). Ändert sich ein Status, kommt zuerst eine neue vollständige
  `abo`-Antwort und erst danach wieder Werte.
* **Verwaiste Datarefs** (Plugin entladen oder abgeschaltet; `XPLMFindDataRef`
  findet sie weiter, lesen ergäbe 0): Vor jedem Lesen eines Namens, der nicht
  mit `sim/` beginnt, prüft das Plugin `XPLMIsDataRefGood`. Ist er verwaist,
  fällt der Wert aus und der Status geht **sofort** auf `fehlt` (neue
  `abo`-Antwort nach der Runde). Nach `XPLM_MSG_PLANE_LOADED` pausieren Abos
  mit solchen Namen die Lieferung, bis die Neusuche übernommen ist (höchstens
  0,5 s — danach schützt die Prüfung beim Lesen); Abos nur mit
  `sim/…`-Namen liefern weiter.
* **`flugzeug`** nach jedem `HALLO`, nach `XPLM_MSG_PLANE_LOADED` und wenn sich
  ICAO/Titel/Pfad ändern (Prüfung alle 2 s). Fehlt ein Dataref → `null`.
* **`LISTE`** meldet nur abonnierbare Namen (druckbares ASCII, ≤ 512 Byte,
  nicht auf `]` endend — sonst würde das ABO ihn als Array-Element lesen).
  Eine neue `LISTE` ersetzt eine laufende. Ohne XPLM 4.0 (X-Plane 11):
  `{"p":2,"t":"fehler","grund":"liste_nicht_verfuegbar","id":…}`.
* **Fehlergründe:** `leere_anfrage`, `datagramm_zu_gross`, `zeile_zu_lang`
  (Befehlszeile), `unbekannter_befehl`, `falsche_argumente`,
  `protokoll_ungueltig`, `abo_id_ungueltig`, `rate_ungueltig`,
  `teil_ungueltig`, `generation_ungueltig`, `zu_viele_namen`,
  `ueberzaehlige_zeilen`, `kein_hallo`, `abo_teil_reihenfolge`,
  `abo_teile_widerspruch`, `keine_namen`, `liste_nicht_verfuegbar`,
  `speicher`, `speicher_limit`. Optional mit `zeile` (1-basiert), `abo` +
  `gen` (immer zusammen) und `id`. Ein ABO wird nur bei Rahmenfehlern (Kopfzeile, zu viele
  Namen, Teile, Datagramm zu groß) ganz verworfen; einzelne ungültige Namen
  bekommen `fehlt`.

### Leistung und Sicherheit

* Alles im Flight-Loop (Hauptthread), Sockets nicht blockierend. Höchstens 64
  eingehende Datagramme **und höchstens 0,5 ms Empfang** je Frame (Uhr nach
  jedem Datagramm samt seiner Antwort; der Rest bleibt bis zum nächsten Frame
  im Socket, ein Datagramm wird immer ganz bearbeitet).
* **Ein gemeinsamer Ausgang:** höchstens **16 Pakete je Frame über alle
  Sendewege** (Antworten, `flugzeug`, Status, Werte, LISTE). Gezählt wird ein
  ganzer Flight-Loop-Aufruf: erst die Antworten beim Empfang, dann die
  Lieferung. Kleine Einzelantworten (`hallo`, `pong`, `fehler`,
  `abo_empfangen`) haben Vorrang, aber höchstens **8 je Frame**; der Rest
  wird verworfen und alle 10 s als Summe ins Log geschrieben — eine
  PING-Flut wird nicht zur Antwortflut, und der Lieferung bleiben immer
  mindestens 8 Pakete. Fehler, die erst während der Lieferung entstehen (Abo
  oder LISTE am Budget verworfen), gehen nie verloren: sie werden vorgemerkt
  und im nächsten Frame vor allem anderen über denselben Ausgang gesendet;
  ist er voll, bleiben sie vorgemerkt.
* **Zeitbudgets je Frame:** 0,3 ms für Flugzeugkennung und Suchen, danach
  1 ms für Lesen und Senden. Die Uhr wird **nach jedem einzelnen
  XPLM-Aufruf** gelesen (Find, IsDataRefGood, Types, Array-Länge, Getter,
  LISTE je Name und `XPLMCountDataRefs`, die drei Kennungs-Datarefs für
  `flugzeug` je einzeln, jedes Senden); ist das Budget
  erschöpft, endet die Arbeit vor dem nächsten Aufruf, ein angefangener
  Eintrag beginnt im nächsten Frame neu. Frei ist je Budget nur die erste
  Einheit, und die nur bis einschließlich ihres **einen** fremden Accessors —
  sonst könnte ein einziges teures Dataref jeden Fortschritt verhindern.
* **Das ist ein weiches Budget.** Einen fremden Accessor (Getter oder
  Array-Länge eines anderen Plugins) ruft XPLM synchron im Hauptthread auf;
  braucht er 20 ms, steht X-Plane 20 ms — das kann kein Plugin unterbrechen,
  eine harte Grenze ginge nur mit Isolation fremder Accessoren (eigener
  Prozess), die XPLM nicht erlaubt. Das Plugin tut, was geht: danach sofort
  aufhören, und einen Dataref, dessen Accessor **dreimal hintereinander**
  länger als **2 ms** brauchte, **drosseln** — der Zustand gilt **je
  Dataref** (Handle), nicht je Eintrag: Duplikate desselben Namens, auch in
  verschiedenen Abos, zählen und drosseln gemeinsam (feste Tabelle, 1024
  Plätze, höchstens 768 gleichzeitig verfolgte langsame Datarefs; ein
  schneller Aufruf räumt den Platz eines noch nicht gedrosselten wieder).
  Ist ein gedrosselter Dataref an der Reihe, bedient **ein** Getter-Aufruf
  **alle** seine Einträge der Runde (bei Array-Elementen wird das Array
  einmal über ein Fenster bis 256 Elemente gelesen, weiter auseinander
  liegende Indizes kommen reihum dran); die 0,2-s-Sperre geht an den am
  **längsten wartenden** gedrosselten Dataref — kein Wert verhungert. Die
  Suche fragt die Array-Länge je Dataref einmal je Durchlauf, bei
  gedrosselten nur, wenn sie ohnehin dran sind. **Fünf schnelle** gedrosselte
  Lesungen in Folge heben die Drosselung wieder auf (Log einmal). Gedrosselt
  heißt: höchstens einmal je Sekunde
  gelesen, über alle Abos höchstens ein gedrosselter Lesezugriff je 0,2 s,
  im 2-s-Nachsuchlauf übersprungen. Status und Werte bleiben wahr (kein
  `fehlt`), der Wert kommt nur seltener (der Client behält den letzten). Eine
  Zeile je Name im `Log.txt` (`… antwortet langsam …`, höchstens 32). Nach
  einem Flugzeugwechsel wird neu bewertet. Ein einzelner Ausreißer (der
  Thread wurde vom Betriebssystem unterbrochen) drosselt nichts.
* Geliefert wird zuerst Abo 1 (beim Client die Telemetrie) mit einem
  **Teilbudget von 0,5 ms**, dann im Rundlauf über die **belegten** übrigen
  Abos **und LISTE** mit dem Rest des Budgets — jeder Teilnehmer ist
  regelmäßig als erster dran. Weder ein großes Abo 1 noch ein Dauer-Abo kann
  LISTE oder andere Abos aushungern.
* **Höchstens zwei fremde Aufrufe über dem Budget je Frame:** Such- und
  Liefer-Budget bilden zusammen eine Frame-Grenze (1,3 ms). Danach beginnt
  je Frame höchstens **ein** weiterer fremder Accessor (die freie Einheit),
  reihum vergeben an Suche, Abo 1 und Rundlauf (jeder ist spätestens jeden
  dritten Frame dran). Der schlimmste Frame ist also 1,3 ms + der eine
  Aufruf, der im Budget begann und sich als langsam herausstellte, + die
  freie Einheit.
* **Speicherbudgets (hart):** je Abo **16 MiB**, alle Abos samt Teil-Abos
  zusammen **64 MiB**, LISTE **16 MiB**. Gezählt wird die reservierte
  Kapazität, mit dem schlimmsten Fall je Wert (der Ausgabestapel wird beim
  Anmelden für die längste mögliche Zahl reserviert, damit beim Lesen nichts
  mehr allokiert wird) — und je Eintrag: 8192-mal derselbe 1024-Byte-Dataref
  kostet 8192 × 6153 Byte ≈ 48 MiB und wird abgelehnt. Überschreitung →
  `{"p":2,"t":"fehler","grund":"speicher_limit","abo":…,"gen":…}` (bzw. mit
  `"id"` bei LISTE) statt Allokation. Scheitert ein fertiges ABO (oder später
  seine Werte-Reserve nach einer Nachsuche), ist die ID danach leer; reißt
  das Budget schon beim Empfang eines Teils, bleibt — wie bei jedem
  Aufbaufehler — das bisherige Abo dieser ID unverändert.
  Größenordnung legitimer Fälle: Abo 1 (≈ 200 Namen) ≈ 30 KiB; Vermessung mit
  14 Abos × 8192 typischen Namen (5 % ganze 256er-Arrays) ≈ 47 MiB gemessen;
  ein einzelnes Abo mit 3400 ganzen 256er-Arrays ≈ 15 MiB; LISTE mit 40 000
  Namen ≈ 2,5 MiB.
* Beim Start misst das Plugin einmal die Kosten von `XPLMFindDataRef` und
  `XPLMIsDataRefGood` und schreibt sie ins `Log.txt`
  (`Protokoll 2: Kosten je Aufruf - …`).
* Solange Protokoll 2 nichts zu liefern hat, läuft der Flight-Loop im Takt von
  Protokoll 1 (20 Hz, unter 200 ft AGL jeden Frame); mit aktiven Abos jeden
  Frame, Protokoll 1 wird dann über die Uhr auf seinen Takt gedrosselt.
* Protokoll 2 schweigt **nicht** in Pause/Replay; `sim/time/paused` und
  `sim/time/is_in_replay` kann der Client wie jeden Namen abonnieren.
* Speicher nur beim (Neu-)Anmelden (Abo, Statuswechsel, LISTE), per
  `malloc` mit Fehlerprüfung → `fehler/speicher` statt Absturz.
* Die XPLM-4.0-Funktionen für `LISTE` werden per `XPLMFindSymbol` geholt
  (nur bei XPLM ≥ 400) — das Plugin lädt deshalb weiter unter X-Plane 11.
* **Sockets** (beide Protokolle, `src/netz.cpp`): jede Rückgabe wird
  geprüft. Scheitert „nicht blockierend“ (`fcntl`/`ioctlsocket(FIONBIO)`),
  wird der Socket geschlossen und das jeweilige Protokoll bleibt aus — ein
  blockierender Socket im Flight-Loop könnte X-Plane anhalten. Unter Windows
  ebenso bei `SO_EXCLUSIVEADDRUSE` (sonst könnte ein anderes Programm den
  Steuer-Port mitbinden); scheitert nur `SIO_UDP_CONNRESET`, gibt es eine
  Warnung und Protokoll 2 läuft (Empfangsfehler nach einem beendeten Client
  werden übersprungen).
* Ist Port 52001 belegt, schreibt das Plugin eine Zeile ins `Log.txt` und
  läuft nur mit Protokoll 1 weiter.
* **Aktivieren/Deaktivieren** (Plugin-Admin): Netz nur, solange das Plugin
  aktiviert ist. `XPluginDisable` verwirft Client, Abos und LISTE und
  schließt beide Sockets (Port 52001 frei, ungelesene Datagramme weg);
  `XPluginEnable` bindet neu. Der Client bekommt danach `kein_hallo` und
  meldet sich neu an.
* **Empfehlung Client:** Empfangspuffer (`SO_RCVBUF`) ≥ 1 MiB — große Abos
  und `LISTE` kommen mit bis zu 128 KiB je Frame; Windows hat ab Werk 64 KiB.

## Protokoll 1 (bleibt für ältere Clients)

Unverändert seit Plugin 0.5.13: `telemetry` und `touchdown` an
`127.0.0.1:52000`, im selben Takt wie bisher, schweigt in Pause/Replay. Der
neue Client wertet weiter `touchdown` aus. Einzige Ergänzung ab 1.0.0: das
Feld `"pv":"1.0.0"` (Plugin-Version) in beiden Paketen — so erkennt der
Client ein aktuelles Plugin auch, wenn dessen Protokoll 2 nicht antwortet
(Port 52001 belegt). Alte Clients ignorieren das Feld.

Seit 1.0.0 schreibt der locale-feste JSON-Schreiber die beiden Pakete
(`src/protokoll1.cpp`) statt `snprintf`: Stellt ein anderes Plugin
`LC_NUMERIC` auf Dezimalkomma, bleibt es beim Punkt. Feldschema,
Reihenfolge und Nachkommastellen sind unverändert (für endliche Werte Byte
für Byte die alte Ausgabe, Test `protokoll1_gleich_wie_bisher`); ein nicht
endlicher Wert (NaN/±Inf) wird `null` statt des ungültigen `nan`.

Der erste Protokoll-1-Tick nach `XPluginEnable` (auch beim Laden)
synchronisiert nur: aktueller Bodenzustand (`fnrml_gear` ≥ 1 N) wird
übernommen, Ringpuffer und Sinkraten-Tracker neu, **keine Aufsetz-Kante in
diesem Tick**. Sonst meldete ein Plugin, das in der Luft abgeschaltet und am
Boden wieder eingeschaltet wird, einen Schein-Touchdown mit alten Werten.
Grenzen: Ein Aufsetzen genau in diesem einen Tick würde nicht gemeldet
(Fenster: ein Tick; der Ringpuffer wäre ohnehin leer). Beim Laden am Boden
entfällt der alte Schein-Touchdown nur, wenn X-Plane `fnrml_gear` schon im
ersten Tick mit der Last meldet — zeigt der erste Tick noch 0 N, gilt das
Flugzeug als in der Luft und die folgende Kante wird wie bisher gemeldet.

Every packet is a single line of JSON terminated with `\n`. The
schema is versioned via `"v":1`. Two packet types:

### `telemetry` — every flight-loop tick

Sent on every flight loop tick (~20 Hz cruise / per-frame near the
ground). Used as a heartbeat — the client uses it to know the plugin
is alive but trusts the standard RREF stream for the live values.

```json
{"v":1,"pv":"1.0.0","type":"telemetry","seq":12345,"ts":1234.567890,
 "lat":50.0345678,"lon":8.5712345,
 "agl_ft":2150.40,"vs_fpm_raw":-285.40,"vs_fpm":-285.10,
 "fnrml_gear_n":0.00,"on_ground":false,"g_normal":0.9970,
 "pitch_deg":3.420,"bank_deg":0.150,"hdg_true":253.117,
 "ias_kt":138.40,"gs_kt":134.50}
```

### `touchdown` — one-shot at wheel contact

Fires exactly once per landing, the instant `fnrml_gear` crosses
the touchdown threshold (1 N — far below any physically plausible
contact). The `captured_*` fields hold the values we want to
record for the PIREP: peak descent VS pulled from a 500 ms
lookback, pitch- and bank-attitude at the edge, etc. Re-arms when
AGL climbs back above 50 ft so a touch-and-go gets two events.

```json
{"v":1,"pv":"1.0.0","type":"touchdown","seq":12450,"ts":1289.012345,
 "lat":50.0411111,"lon":8.5811111,
 "captured_vs_fpm":-285.4,"captured_g_normal":1.18,
 "captured_pitch_deg":3.4,"captured_bank_deg":0.2,
 "captured_ias_kt":138.0,"captured_gs_kt":134.5,
 "captured_heading_deg":253.1,
 "fnrml_gear_n":52312.0,"agl_ft":0.4}
```

## Installation (pilot-facing)

The AeroACARS desktop installer copies the plugin automatically. If
you're installing manually:

1. Download the `AeroACARS-XPlane-Plugin-vX.Y.Z.zip` artifact from
   the GitHub Release page.
2. Extract it into your X-Plane plugins folder so you end up with:
   ```
   <X-Plane>/Resources/plugins/AeroACARS/
       64/
           win.xpl   (Windows pilots)
           mac.xpl   (macOS pilots)
           lin.xpl   (Linux pilots)
       README.md
       XPLM_SDK_LICENSE.txt
   ```
   You can drop the *whole* folder in — X-Plane picks the matching
   `.xpl` for your platform automatically.
3. Restart X-Plane. Open `Plugins → Plugin Admin` — you should see
   "AeroACARS Premium" listed as enabled.
4. Open the AeroACARS desktop client → Settings → Debug. The
   "X-Plane Premium Plugin" panel should turn green within a few
   seconds.

If the panel doesn't turn green:

* Open `<X-Plane>/Log.txt` and grep for `[AeroACARS]` — every
  log line from the plugin is prefixed with that.
* Check that no other AeroACARS instance is running (port 52000
  is held by exactly one app at a time).
* Protokoll 2 prüfen (Plugin 1.0.0+, X-Plane läuft):
  `python3 werkzeuge/plugin_sonde.py pruefung` — oder einzeln
  `… hallo`, `… abo sim/flightmodel/position/latitude --rate 5`,
  `… liste`. Im `Log.txt` steht beim Start
  `Protokoll 2 bereit auf 127.0.0.1:52001 (X-Plane …, XPLM …, LISTE ja|nein)`.

## Building from source

The plugin is in **C++17**, no exceptions, no RTTI, hidden symbol
visibility — same conventions every other open-source XPLM plugin
uses. Cross-platform build via CMake.

### Prerequisites

* CMake ≥ 3.20 (Visual Studio 17 2022 ships one bundled at
  `BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe`)
* Windows: Visual Studio 2022 (Build Tools edition is fine)
* macOS: Xcode 15+ command-line tools
* Linux: GCC 11+ or Clang 14+
* The X-Plane SDK is **vendored** under
  `third_party/XPSDK430/` — no separate download needed.

### Windows

```sh
cd xplane-plugin
cmake -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release --target AeroACARS
# Output: build/AeroACARS/64/win.xpl
```

### macOS (universal — Apple Silicon + Intel in one .xpl)

```sh
cd xplane-plugin
cmake -B build -G "Unix Makefiles" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_OSX_ARCHITECTURES="x86_64;arm64"
cmake --build build --target AeroACARS
# Output: build/AeroACARS/64/mac.xpl
```

### Linux

```sh
cd xplane-plugin
cmake -B build -G "Unix Makefiles" -DCMAKE_BUILD_TYPE=Release
cmake --build build --target AeroACARS
# Output: build/AeroACARS/64/lin.xpl
```

### Tests (ohne X-Plane)

```sh
cd xplane-plugin
cmake -B build-tests -DAEROACARS_PLUGIN_TESTS=ON -DCMAKE_BUILD_TYPE=Debug
cmake --build build-tests --target aeroacars_alle_tests
ctest --test-dir build-tests --output-on-failure
```

* `einheitstests` — Parser, JSON-Schreiber, Paketaufteilung und der ganze
  Dienst (`src/dienst.cpp`) gegen eine Schein-X-Plane-Welt
  (`tests/schein_welt.h`): Status, Raten, Zeitbudget, Nachsuche, verwaiste
  Datarefs, Zeitüberschreitung, mehrteilige Abos, LISTE, voller Socket.
  Dazu Protokoll 1 (Format gegen das alte `snprintf`, NaN, Locale) und die
  Socket-Einrichtung mit einer Attrappe, die jeden Betriebssystemaufruf
  scheitern lässt. `aeroacars_tests <teilname>` führt nur passende Tests aus.
* `fuzz` — 5 s zufällige und verstümmelte Eingaben gegen Parser und Dienst,
  dazu PING-Fluten, langsame Accessoren, Abos am Speicherbudget; geprüft
  werden auch die Bytebudgets und die 16 Pakete je Frame (fester Startwert;
  `aeroacars_fuzz <sekunden> <startwert>` für längere Läufe).
* `netz_echt` (alle drei Plattformen, auch Windows) — die echten
  Socket-Aufrufe: Steuer-Socket wirklich nicht blockierend, zweiter Socket
  auf demselben Port scheitert, Sender erreicht den Steuer-Socket.
* `ende_zu_ende` (macOS/Linux) — die echte `.xpl` in einer XPLM-Attrappe
  (`tests/attrappe/`), abgefragt mit `werkzeuge/plugin_sonde.py pruefung`;
  dazu X-Plane-11-Verhalten, belegter Port 52001 und ein Zyklus
  Deaktivieren/Aktivieren. Wird übersprungen (77), wenn Port 52001 belegt
  ist (X-Plane mit dem Plugin läuft).
* Unter macOS/Linux mit AddressSanitizer + UBSan
  (`-DAEROACARS_SANITIZER=OFF` schaltet ab), unter Windows ohne.

### Output layout

CMake writes files into the X-Plane "fat plugin" layout straight away:

```
build/
  AeroACARS/
    README.md
    XPLM_SDK_LICENSE.txt
    64/
      win.xpl   (or mac.xpl, lin.xpl)
```

Drop the whole `build/AeroACARS/` folder into
`<X-Plane>/Resources/plugins/` to install.

## Architecture & safety

The plugin runs in X-Plane's render thread via the flight-loop
callback. It is constrained by four non-negotiable rules:

1. **Never crash X-Plane.** Every `XPLMFindDataRef` result is
   NULL-checked before use. All errors are caught and logged via
   `XPLMDebugString`, never propagated. C++ exceptions are
   compiled out (`-fno-exceptions`). Protokoll 2: harte Obergrenzen
   (`src/grenzen.h`), kein eigener Signal-Handler, Parser und
   JSON-Schreiber mit Unit- und Fuzz-Tests unter ASan/UBSan, Arbeitspuffer
   mit Reserve gegen Plugins, die mehr Array-Werte schreiben als erbeten.
2. **Never stall the flight loop.** The callback reads ~15
   DataRefs (microseconds), builds a small JSON string
   (microseconds), and calls a non-blocking `sendto()` on a UDP
   socket (microseconds when the buffer is empty,
   `ECONNREFUSED`-ignored when the client isn't listening).
   No filesystem I/O, no malloc inside the hot path.
3. **Never persist state outside the plugin's address space.**
   No file writes, no registry edits, no env-var tweaks. The
   plugin is purely read-only against X-Plane state.
4. **Clean shutdown on plugin reload.** `XPluginStop` unregisters
   the flight loop, closes the socket, and zeros every DataRef
   handle, so a subsequent `XPluginStart` starts from a known-
   good slate.

Quelltexte:

| Datei | Inhalt |
|---|---|
| `src/plugin.cpp` | Einstiegspunkte, Protokoll 1, gemeinsamer Flight-Loop |
| `src/dienst_xplm.cpp` | Protokoll 2 an XPLM + Socket (einzige XPLM-Stelle für P2) |
| `src/dienst.cpp` | Protokoll 2: Abos, Lieferung, LISTE, Flugzeug (XPLM-frei) |
| `src/anfrage.cpp` | Anfrage-Parser (rein, XPLM-frei) |
| `src/json_schreiber.cpp` | JSON in festen Puffer (rein, XPLM-frei) |
| `src/pakete.cpp` | Aufteilung in Pakete ≤ 8 KiB (rein, XPLM-frei) |
| `src/protokoll1.cpp` | Pakete `telemetry`/`touchdown` (rein, XPLM-frei) |
| `src/netz.cpp` | Regel zum Einrichten beider Sockets (rein, prüfbar) |
| `src/netz_os.cpp` | die echten Socket-Aufrufe (Winsock/POSIX) |
| `src/grenzen.h` | alle Obergrenzen an einer Stelle |

## DataRefs read (Protokoll 1)

Protokoll 2 liest genau die Namen, die der Client abonniert, plus für die
`flugzeug`-Meldung `sim/aircraft/view/acf_ICAO`, `acf_descrip` und
`acf_relative_path`.

| DataRef                                                    | Used for                          |
|------------------------------------------------------------|-----------------------------------|
| `sim/flightmodel/position/latitude`                        | telemetry, touchdown lat          |
| `sim/flightmodel/position/longitude`                       | telemetry, touchdown lon          |
| `sim/flightmodel/position/y_agl`                           | adaptive-rate trigger, edge guard |
| `sim/flightmodel/position/local_vy`                        | VS (raw, m/s, no smoothing)       |
| `sim/flightmodel/forces/fnrml_gear`                        | touchdown edge detection          |
| `sim/flightmodel/failures/onground_any`                    | reported in telemetry             |
| `sim/flightmodel2/misc/gforce_normal`                      | g-force capture                   |
| `sim/flightmodel/position/{theta,phi,psi}`                 | pitch / bank / true heading       |
| `sim/cockpit2/gauges/indicators/airspeed_kts_pilot`        | IAS                               |
| `sim/flightmodel/position/groundspeed`                     | GS                                |
| `sim/time/{paused,is_in_replay}`                           | suppress packets while paused     |

All names are read-only. The plugin never writes to a DataRef.

## License & attribution

The plugin source is licensed identically to AeroACARS itself — see
the project root `LICENSE`. The X-Plane SDK headers under
`third_party/XPSDK430/` are licensed under the Laminar Research /
X-Plane Plugin SDK BSD license — see `third_party/XPSDK430/license.txt`.
The license file is copied into the released plugin folder as
`XPLM_SDK_LICENSE.txt` to comply with the attribution clause.
