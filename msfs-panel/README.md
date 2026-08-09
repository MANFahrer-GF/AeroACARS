# AeroACARS — natives MSFS-2024-Panel

Der gleiche HUD-Streifen wie das Flow-Pro-Widget, nur als eigenes
Toolbar-Panel im Simulator. **Der Pilot entscheidet, welchen Weg er
nimmt** — beide funktionieren eigenstaendig, keiner braucht den anderen.

## Eine Quelle, zwei Ziele

| Datei hier | Herkunft |
|---|---|
| `panel.js` | **byte-identisch** mit `code.js` des Flow-Widgets |
| `panel.css` | dessen `code.css` **plus** einem nativen Rahmen-Block am Ende |
| `AeroACARSPanel.html` | eigen (Framework-Einbindungen + `<ingame-ui>`) |

Das geht, weil **beide Umgebungen derselbe Renderer sind** (Coherent GT).
Alle im Feld erkaempften Regeln gelten hier genauso: kein `gap` auf
Flex-Containern, nur ASCII in der Ausgabe, kein WebSocket. Wer `panel.js`
aendert, aendert das Flow-Widget mit — genau so ist es gewollt.

`panel.js` waehlt seinen Weg selbst:

* **Flow Pro** — `run()`/`exit()` vorhanden → Wheel-Kachel schaltet um,
  Flows `exit()` raeumt vor jedem Neuladen auf.
* **Nativ** — die Haken fehlen → sofort aufbauen und abfragen. Sichtbar-
  keit macht das Toolbar-Symbol, Ziehen und Groesse der Fensterrahmen.

Beide legen ihren Lebenszyklus unter **getrennten** globalen Schluesseln
ab (`__aeroacarsHudV2_flow` / `_nativ`). Sollten sich die Ansichten wider
Erwarten doch einen JS-Kontext teilen, legt die eine Kopie die andere
damit **nicht** still — im Testfall nachgestellt und bestaetigt.

## Die Fensterdekoration: KEIN `<ingame-ui>` (Variante A)

**Das ist der Kern, und er wurde teuer gelernt.** Das Panel enthaelt
bewusst kein `<ingame-ui>`, keine Framework-Importe und keine eigene
Kopfzeile — es ist schlichtes HTML, so wie Runde 1 (Commit `37266dd`).

Grund: Thomas' Feldbefund vom 09.08.2026 nach dem Test von v0.3.0 —
*"Das Fenster laesst sich nicht verschieben. Wir hatten eine Version, das
war die letzte, die keine Kopfzeile oben hatte. Die hatte sich
verschieben lassen."* Verschiebbar war also die Fassung OHNE eigene
Fensterdekoration. Ohne `<ingame-ui>` legt der Sim seinen **eigenen**
Fensterrahmen um die Seite, und der funktioniert.

Das deckt sich mit dem bestaetigten Asobo-Bug (MSFS-DevSupport, mehrere
Addon-Entwickler seit SU2): der Sim haengt `<ingame-ui>`-Elementen selbst
die Klassen `hide` und `panelInvisible` an und bricht damit Titelleiste
und Ziehen.

### Variante B — nur falls A im Sim doch nicht zieht

Das oeffentliche, laufende Referenzprojekt
`github.com/jopeek/msfs-panel-simaware` benutzt `<ingame-ui>`, aber
anders als unser gescheiterter Versuch: in einen **`<ingamepanel-custom>`**
gewickelt, mit **leerem** `title=""` und der Klasse `panelInvisible` —
also ebenfalls ohne sichtbare Kopfzeile. Wer A verwerfen muss, nimmt
genau dieses Muster, nicht das aus v0.3.0.

## Groessen: gemessen, nicht geschaetzt

Am 09.08.2026 mit der echten `code.css` ueber 12 Zustaende und 7
Rahmenbreiten im Browser gemessen:

| Groesse | Messwert | im Paket gesetzt |
|---|---|---|
| Hoehe mit Ticker | **63 px** (Ticker + Trennlinie + Datenzeile) | `min-height: 84 px` |
| Hoehe ohne Ticker | 38 px | — |
| Breiteste Zeile | **627 px** (Boarding: Fuel *und* ZFW je Ist/Soll) | `min-width: 680 px` |

Zwei Erkenntnisse daraus:

1. **Der Streifen bricht nicht um, er schneidet seitlich ab.** Die Hoehe
   ist ueber alle Breiten konstant — der Engpass ist die BREITE. Zu
   schmal heisst: rechts faellt Information weg, lautlos.
2. **v0.3.0 war zu klein, und die Kopfzeile war schuld.** 92 px Rahmen
   minus ~40 px Kopfzeile = ~50 px fuer einen Streifen, der 63 px
   braucht — deshalb war immer nur EINE Zeile zu sehen. Ohne Kopfzeile
   ist das an der Wurzel weg.

`InGamePanelDefinition` steht auf `defaultWidth="38" defaultHeight="10"`
(Prozent der Bildschirmflaeche; auf 1920x1080 rund 730 x 108 px) mit
`minWidth="36" minHeight="8"`. Bei anderer Bildschirmaufloesung darf
nachgezogen werden — Hauptsache das Fenster bleibt breiter als 680 px.

## Die zwei verbleibenden Fallen

1. **`content-fit="true"` ist in dieser SDK-Fassung kaputt** (offenes
   Asobo-Ticket 18144): der Rahmen faellt auf die Titelleiste zusammen
   oder waechst auf volle Bildschirmhoehe. Betrifft nur Variante B.

2. **Die X- und Pin-Knoepfe der nativen Titelleiste stehen im
   MSFS-Devsupport-Forum als Absturzausloeser.** Zum Schliessen das
   Toolbar-Symbol benutzen. In Variante A gibt es sie ohnehin nicht.

## Bauen

Braucht den MSFS-SDK (`fspackagetool.exe`) auf einem Windows-Rechner:

```
fspackagetool.exe Build\PackageDefinitions\aeroacars-panel.xml
```

Ergebnis nach `Community\aeroacars-panel\` kopieren, Sim neu starten.
Das Panel erscheint als **AeroACARS** in der Toolbar.

## Voraussetzung

AeroACARS muss laufen — das Panel holt seine Daten ueber
`http://127.0.0.1:47847` vom Panel-Server der App (nur Loopback, keine
Weitergabe nach aussen). Abschaltbar in den App-Einstellungen unter
*MSFS-In-Sim-HUD*; ist der Server aus, zeigt der Streifen genau das an.
