# ADR-0005 — X-Plane-HUD-Band (wie das Flow-Widget)

Status: Entwurf 04.10.2026 · Zweig `feat/xplane-hud-band` · baut auf ADR-0004 (Protokoll 2)

## Ziel

Dasselbe Band wie das MSFS-Panel / Flow-Widget, in X-Plane: **nur das Band**,
ohne Fensterrahmen, überall anfassbar und ziehbar, größer/kleiner,
im Ruhezustand gedimmt, bei Mausberührung hell. Alle Zustände des Bands
(`lage()` in `msfs-panel/.../panel.js`): getrennt, bereit, pausiert, unterwegs
(Boden: Route/Fuel/ZFW/WX · Flug: Route/Höhe/Zeit/WX), anflug, auswertung,
ergebnis, rollen, amStand, eingereicht.

## Grundsatz: die App rechnet, das Plugin zeichnet

Die Formatierung (FL/ALT/AGL, ETE/FLT-Wechsel, V/S-Farbe gegen Gleitwinkel,
METAR-Kurzform, Landenote, Soll/Ist-Farben …) wird **einmal in Rust** gebaut
und als fertige Zeilen ans Plugin geschickt. Das Plugin enthält **keine
Fachlogik**, nur Parsen, Zeichnen, Ziehen, Skalieren, Merken. (Wie beim
Landeurteil: eine Rechnung, viele Anzeigen.) Die Rust-Quelle ist die
Portierung von `panel.js` (`lage`, `zeichne`, `hoehenZelle`, `zeitZelle`,
`wxAusDekodiert`/`wxText`, `vsFarbe`, `beladungsFarbe`, `istSoll`,
`nurAscii`, `kuerze`); Verhalten wie dort, Texte identisch.

## Wire-Format (Client → Plugin), ein Datagramm

Neuer Befehl in Protokoll 2 (abwärtskompatibel: alte Plugins antworten
`fehler/unbekannter_befehl`, alte Clients kennen ihn nicht):

```
BAND <seq> <ruhig> <zeilen>
<zeile 1>
<zeile 2>
…
```

| Feld | Regel |
|---|---|
| `seq` | 0 … 2147483647; Plugin verwirft `seq` ≤ letzte angenommene (Wrap: Differenz > 2^30 gilt als neu) |
| `ruhig` | `0` oder `1` → Plugin dimmt (Faktor 0,5), bei Mausberührung 1,0 |
| `zeilen` | 0 … 4. **0 = Band ausblenden** (App-Einstellung aus / Abmeldung); danach unsichtbar, bis wieder ein Band mit ≥ 1 Zeile kommt |
| Zeile | ≤ 512 Byte, nur `0x20–0x7E` und `TAB`, `\n`-getrennt |
| Zeile = Läufe | durch `TAB` getrennte **Läufe**; Lauf = 1 Farbzeichen + Text (Text darf leer sein) |
| Farben | `n` normal · `d` gedämpft (Beschriftung) · `g` gut · `w` Warnung · `b` schlecht · `a` Akzent |
| Sonderlauf | Text genau `@` → gefüllter Statuspunkt in der Farbe des Laufs statt Text |
| Abstand | Plugin setzt zwischen zwei Läufen einen festen Abstand (1 Leerzeichenbreite × 2); Rust schreibt Leerzeichen nur *innerhalb* eines Laufs |
| Zeile 1 | Ticker (Alter + Meldung); Zeile 2 = Datenzeile mit Punkt, Kennung, Lage, Zellen; weitere Zeilen frei |

Zellen wie `Route EDDF > KJFK` werden als zwei Läufe gesendet: `dRoute`,
`nEDDF > KJFK`. Keine Antwort des Plugins (fire-and-forget, UDP).

Ungültige Datagramme (Grenzen, Nicht-ASCII, Zeilenzahl ≠ Angabe) → ganze Nachricht
verworfen, Fehlerantwort `{"p":2,"t":"fehler","grund":"band_ungueltig",…}`,
bisheriges Band bleibt stehen.

## Plugin

* Eigener, vom Dataref-Dienst getrennter Zustand (`band.cpp/.h`, XPLM-frei,
  testbar wie `dienst`). Hauptthread, keine Threads, keine Ausnahmen.
* **Sichtbar erst nach dem ersten gültigen BAND mit ≥ 1 Zeile** (sonst: nichts zeichnen — wer die App-Einstellung aus hat oder einen alten Client benutzt, sieht kein Fantom-Band).
* **Kein Band seit 5 s** (Client weg, nachdem schon eines kam; Band nicht per `zeilen=0` ausgeblendet): Plugin zeigt selbst eine Zeile
  `AeroACARS nicht erreichbar - laeuft die App?` mit rotem Punkt (wie
  `getrennt` im MSFS-Band), Rest bleibt.
* **Fenster** (`XPLMCreateWindowEx`): ohne Dekoration, frei positionierbar,
  eigene Hintergrundbox (dunkel, abgerundet) und Text (`XPLMDrawString`,
  proportional, ASCII). Funktioniert 2D und VR (`XPLMSetWindowPositioningMode`
  für VR möglich, erste Fassung: nur 2D).
* **Ziehen:** linke Maustaste irgendwo auf dem Band, Verschieben um die
  Mausdelta. Keine Klicks im Spiel durchlassen, solange das Band getroffen wird.
* **Größe:** Mausrad über dem Band, Stufen 0,6 · 0,8 · 1,0 · 1,25 · 1,5 · 2,0 · 3,0.
  Skalierung der Zeichnung; Fenstergröße folgt dem Inhalt.
* **Dimmen:** `ruhig=1` → Farben × 0,5 (X-Plane-Text kann kein Alpha), Maus
  darüber → ×1,0.
* **Menü** unter „Plugins → AeroACARS": Band ein/aus · Größer · Kleiner ·
  Zurücksetzen (Position + Größe).
* **Merken:** Position, Stufe, an/aus in `Output/preferences/AeroACARS-Band.txt`
  (einfaches `schlüssel=wert`), geschrieben nur bei Änderung (Mausloslassen /
  Menü), gelesen beim Start, Werte auf den Bildschirm geklemmt.
* Standard: eingeschaltet, oben mittig.

## Client (Rust)

* `hud_band.rs` (in `sim-xplane` oder `client`): `fn band(…) -> BandFrame`
  aus denselben Eingaben wie `/panel/status` + Debrief + Aktivität
  (`PanelLiveSnapshot`, `ActiveFlightInfo`, Landebewertung). Ersetzt nichts am
  MSFS-Weg; `panel.js` bleibt vorerst eigene Quelle.
* Versand 1×/s (und sofort bei Lagewechsel) über den vorhandenen Plugin-2-Socket,
  nur wenn das Plugin angemeldet ist (`HALLO` ok) und BAND unterstützt
  (`hallo`-Antwort `plugin` ≥ 1.1.0). Einstellung „X-Plane-Band senden" (Standard an); beim Abschalten einmal `BAND <seq> 0 0` senden.
* Die Zeitzelle (ETE/ETA/FLT) rotiert in Rust (8 s, wie Widget v3.9); kein
  Klick im Plugin nötig.

## Nicht-Ziele (erste Fassung)

VR-Positionierung · Klick-Interaktion · Farbverläufe/Alpha · eigene Schrift ·
Rückkanal Plugin→Client · Schreiben von Datarefs (bleibt ausgeschlossen).

## Abnahme

1. XPLM-freie Tests: Parser (Grenzen, Reihenfolge, Verwerfen), Ablauf (5-s-Frist),
   Skalierung/Klemmung, Prefs.
2. Rust-Tests: je Lage ein Fall (alle 10) + Gleichheit zu `panel.js`-Texten
   (Golden-Texte aus dem Widget), ASCII-Garantie, Zeilenlängen.
3. Feldtest in der X-Plane-Demo (Windows): Band erscheint, ziehbar, Rad skaliert,
   dimmt, Menü, Position bleibt nach Neustart, kein Einfluss auf Frame-Zeit.
4. Plugin-Version → 1.1.0; manuelles Update für X-Plane-Piloten (Plugin ist
   nicht auto-updatefähig) — Release erst nach Abnahme.

## Client-Umsetzung und Pflege (Stand 04.10.2026)

**Wo die Quelle liegt**

| Was | Datei |
|---|---|
| Port von `panel.js`, Wire-Format, Pruefer | `client/src-tauri/crates/sim-xplane/src/hud_band.rs` |
| Versand (1x/s, sofort bei Lagewechsel, `BAND … 0 0`) | `…/sim-xplane/src/plugin2.rs` (`Sitzung::band_takt`, `beenden`) |
| Takt der App (500 ms, baut nur bei Bedarf) + Einstellung | `client/src-tauri/src/xplane_band.rs` (`xplane_band.json`, Standard an) |
| Schalter in den Einstellungen | `client/src/components/XplaneBandPanel.tsx` |
| Golden-Tests | `…/sim-xplane/testdata/hud_band_golden.{cjs,json}`, `hud_band_tests.rs` |

**Pflege: zwei Quellen derselben Anzeige.** `panel.js` (MSFS-Panel, Flow-Widget,
Browser-HUD `/hud`) und `hud_band.rs` (X-Plane) sind getrennte Umsetzungen
derselben Anzeige. Jede Aenderung am Band (Texte, Zellen, Farben, Schwellen,
Rotation) gehoert in BEIDE. Das Skript `hud_band_golden.cjs` laesst das echte
`panel.js` mit Schein-DOM laufen und schreibt `hud_band_golden.json`; der Test
`alle_golden_faelle_stimmen_mit_panel_js_ueberein` vergleicht 300+ Faelle mit
dem Rust-Ergebnis. Nach einer Aenderung an `panel.js`: `node hud_band_golden.cjs
> hud_band_golden.json` (im Ordner `testdata`), dann Rust anpassen, bis der Test
gruen ist. (Aenderung nur in `panel.js` -> der Test wird rot, sobald die
Fixture neu erzeugt ist.)

**Bewusste Abweichungen von `panel.js`**

1. Wind: Text statt Pfeil — `Wind`, `HW 12` (Gegenwind; `TW` bei Rueckenwind),
   `XW 18 R` (Querwind, `R`/`L` = von rechts/links; Farbe wie dort ab 15/25 kt).
   Die Gesamtwindzahl entfaellt.
2. Kein Spinner bei `auswertung` (nur der Text).
3. Keine Trennstriche; der Abstand zwischen Laeufen kommt vom Plugin.
4. Alle Server-Texte (auch die Kennung) laufen durch `nurAscii`. Art je Element
   wie im Nachtrag; Landenote als Art `z`, Etikett als Art `e`
   (Farbe g/w/b wie die BAND-Klasse).
5. Zeitzelle: 30-s-Takt wie `panel.js` v3.10 (der ADR/Widget v3.9 nannte 8 s;
   v3.10 hat auf 30 s beruhigt — Pilotenbefund 11.08.2026).
6. Zeile 1 (Ticker) ist ohne Meldung eine leere Zeile (nur `\n`), damit Zeile 2
   immer die Datenzeile bleibt.
7. Ticker-Meldung holt die App jeden Takt (panel.js: jeden 5. Status).
8. Zeilenende: jede Zeile, auch die letzte, endet mit `\n` (wie die ABO-Datagramme).

## Nachtrag 04.10.2026 — Feldbefunde (X-Plane 12.4.3 Demo, macOS, Metal) und Flow-Optik

### Was im Sim gemessen wurde
* `XPLMDrawString` und `XPLMDrawTranslucentDarkBox` erscheinen; `XPLMDrawString`
  folgt der GL-Modelview (Verschiebung sichtbar wirksam).
* **Eigene GL-Flächen OHNE Textur erscheinen nicht** — vier Wege getestet
  (Sofortmodus mit/ohne Blending, Vertex-Array, absolute Koordinaten), alle
  unsichtbar, auch ohne X-Plane-Box darüber.
* Hypothese (begründet, nicht bewiesen): X-Planes eigene Zeichnungen und alle
  bekannten funktionierenden Plugins auf XP12/Mac (AviTab, ImGui-Fenster)
  zeichnen **texturiert** (`XPLMSetGraphicsState(0,1,0,0,1,0,0)` +
  `XPLMBindTexture2d`). Darum zeichnet das Band ab jetzt **ausschließlich
  texturiert**, auch Flächen (weißer Texel / Kreistextur).
* Die Prefs-Datei wurde auf dem Mac nie geschrieben (HFS-Pfad) — behoben über
  `XPLM_USE_NATIVE_PATHS`.

### Flow-Optik (Ziel: wie `msfs-panel/.../panel.css`)
Box `rgba(13,17,24,0.88)`, Rand 1 px `rgba(255,255,255,0.13)`, Radius 7,
Innenabstand 9/15 px, Zellabstand 13 px, eng 5 px. Farben: ink `#f2f5fa`,
muted `#aeb8c9`, good `#3ecf6a`, warn `#f0a83f`, bad `#ef5464`, accent `#4c8dff`.
Schrift: **Open Sans** (OFL, im Plugin eingebettet, per `stb_truetype` in eine
Atlas-Textur gebacken) statt X-Planes Schrift. Größen/Gewichte wie panel.css:
Ticker-Alter 11 px, Ticker-Meldung 11 px, Punkt 8 px Kreis, Kennung 13 bold,
Lage 13 semibold, Beschriftung 9 px GROSS gesperrt muted, Wert 15 bold,
Anhang 13 semibold muted.

### Wire-Format, Lauf mit **Art** (ersetzt §Läufe; beide Seiten noch unveröffentlicht)
Lauf = `<farbe><art><text>`. Farben wie bisher (`n d g w b a`). Arten:

| Art | Bedeutung (panel.css) |
|---|---|
| `t` | Ticker-Alter (`.aa2-age`) |
| `m` | Ticker-Meldung (`.aa2-msg`) |
| `p` | Statuspunkt (`.aa2-dot`), Text muss leer sein |
| `i` | Kennung (`.aa2-ident`) |
| `s` | Lage-Text (`.aa2-state`) |
| `l` | Zellen-Beschriftung (`.aa2-lbl`) — Plugin zeichnet sie in GROSSBUCHSTABEN |
| `v` | Zellen-Wert (`.aa2-val`) |
| `x` | Anhang (`.aa2-tail`) |
| `z` | Landenote (`.aa2-val.aa2-xl`, 24 px bold) |
| `e` | Noten-Etikett als Pille (`.aa2-band`: 10 px, 800, gesperrt, GROSS, Hintergrund = Farbe mit 24 % Deckkraft, voll gerundet) |

Der Sonderlauf `@` entfällt (ersetzt durch Art `p`). Ein Lauf braucht mindestens
2 Zeichen (Farbe + Art). Leere Zeile = Zeile ohne Läufe (nur `\n`) und belegt keine Höhe.
Abstände setzt das Plugin nach Art: zwischen Beschriftung und Wert 5 px, zwischen
Zellen 13 px, Ticker-Alter→Meldung 5 px.

### Zeichenweg mit Selbstprüfung und Rückfall
Das Plugin zeichnet texturiert. In den ersten Frames prüft es einmalig per
`glReadPixels` an der Boxmitte, ob die eigene Fläche ankam, und schreibt das
Ergebnis ins Log (`Band: Zeichenweg texturiert OK` bzw. `... RUECKFALL`). Bei
GL-Fehler oder fehlender Fläche schaltet es auf X-Plane-Box + `XPLMDrawString`
zurück — das Band bleibt immer lesbar. Texturen werden nur außerhalb des
Zeichen-Callbacks erzeugt (Start/Flight-Loop).

### Nachtrag QS 04.10.2026 (Cloud-Prüfung)
* Einstellung „aus": `BAND <seq> 0 0` geht sofort und danach **alle 5 s erneut**
  (`BAND_AUS_ABSTAND`), nicht nur einmal — ein verlorenes UDP-Datagramm oder ein
  App-Neustart mit „aus" ließ sonst ein altes Band stehen.
* Endet der X-Plane-Listener (Sim-Wechsel), meldet er `band_bereit(false)`;
  die App baut dann kein Band mehr.
* Die App zeigt einen Hinweis, wenn ein Plugin mit Protokoll 2, aber < 1.1.0
  verbunden ist (Plugins-Tab mit Installationsknopf und beim Band-Schalter).
* CI/Release: `libgl-dev` für den Linux-Build; Lizenzen OpenSans OFL und stb im Plugin-Zip.
