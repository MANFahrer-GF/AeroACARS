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

## Die drei Fallen (alle schon hineingetreten)

1. **`<ingame-ui>` ohne Framework-Einbindungen ist ein totes Tag.**
   Runde 2 hatte den Wrapper gesetzt und trotzdem keinen Rahmen, keine
   Titelleiste, kein Ziehen. Es fehlten die vier `/JS/*.js`-Skripte und
   die zwei `/templates/ingameUi*`-Importe im `<head>`. Mit ihnen war das
   Panel im Feld verschiebbar (Runde-3-Stand, 09.08.2026).

2. **`content-fit="true"` ist in dieser SDK-Fassung kaputt** (offenes
   Asobo-Ticket 18144): der Rahmen faellt auf die Titelleiste zusammen
   oder waechst auf volle Bildschirmhoehe. Ausweg ist feste Rahmengroesse
   plus erzwungene Fuellung der Framework-Wrapper — steht unten in
   `panel.css`. Nicht erneut mit `content-fit` versuchen.

3. **Die X- und Pin-Knoepfe der nativen Titelleiste stehen im
   MSFS-Devsupport-Forum als Absturzausloeser.** Zum Schliessen das
   Toolbar-Symbol benutzen.

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
