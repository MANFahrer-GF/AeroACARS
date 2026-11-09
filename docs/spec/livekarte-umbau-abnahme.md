# Live-Karte 3.0: Abnahmeliste für den Umbau

Stand 09.11.2026. Gilt für den einen PR gegen `v3`, der die Karte von innen neu aufbaut.
Regel: Keine Funktion fällt weg, auch nicht still. Jede Zeile braucht am Ende beide Belege:
einen Test, der bei Fehlverhalten rot wird, und einen eigenen Klick in der Vorschau oder im
echten Client. „Offen“ gibt es beim PR nicht mehr.

Vorschau: `http://localhost:5199/bilder.html?livekarte=1` (mit Flug),
`&fall=ohneflug` (ohne Flug), `&theme=light|dark`.
Fenstergrößen für jede Klickrunde: 1490×720 (Thomas), 1920×1080, 1280×800, 1024×768, 800×600,
711×800 (schmales Pane).

## A. Grundkarte

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| A1 | Dunkel/Hell folgt dem App-Theme | Theme-Wechsel tauscht nur die Grundkarte, Overlays und Kamera bleiben | ☑ | ☑ |
| A2 | Satellit | Bild plus Beschriftung; Wahl bleibt nach Neustart (`aaLivemapBasemap`) | ☑ | ☑ |
| A3 | Kein Stiltausch | `setStyle` wird nie gerufen, kein `styledata`-Handler, keine 120-ms-Schleife | ☑ | – |
| A4 | Overlays überleben jeden Wechsel | Route, Track, Taxi, Netz, Kollegen-Routen behalten Daten und Schalterstand, auch nach 4× Umschalten | ☑ | ☑ |
| A5 | Farben je Grundkarte | Auf Satellit: heller Track, dunkle Halos, Routen-Unterlage; Kollegen-Route bleibt unter der eigenen | ☑ | ☑ |
| A6 | Weltkugel | Bleibt nach Satellit-/Theme-Wechsel, Reiterwechsel, Schließen/Öffnen (`aaLivemapKugel`); Reiterwechsel im echten Client belegt | ☑ | ☐ |
| A7 | Schlüssel an jeder Anfrage | Stil, Kacheln, Schriften, Sprites laufen über `kartenAnfrage`; auch ein später eintreffender Schlüssel wirkt | ☑ | – |
| A8 | Serverseitig getauschte Adressen | Neue Stil-Adressen aus `/api/basemap` wirken ohne Neustart | ☑ | – |
| A9 | Ohne Netz oder Stilfehler | Karte, Overlays und Satellit bleiben bedienbar, Stil wird nachgeholt | ☑ | ☑ |
| A10 | Namensnennung | CARTO/OSM, Esri, ODbL-Hinweis der Bodendaten vorhanden; startet eingeklappt | ☑ | ☑ |
| A11 | Verdeckter Reiter | Beim Zurückkommen neu vermessen (`resize`), verdeckt keine Takte | ☑ | ☑ |

## B. Eigener Flug

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| B1 | Flugzeug-Marker | Musterabhängiges Symbol, Farbe nach Phase, kursgedreht | ☑ | ☑ |
| B2 | Folgen | Zentriert, Phasen-Zoom nur beim bewussten Einschalten, manueller Zoom bleibt, Pan schaltet sichtbar aus | ☑ | ☑ |
| B3 | Zentrieren-Knopf | Nur mit laufendem Flug; zentriert und schaltet Folgen an | ☑ | ☑ |
| B4 | Folgen abgehakt | Einmal auf die Route einpassen, nach Pan nicht | ☑ | ☑ |
| B5 | Kurs oben | Standard aus, gemerkt (`aaLivemapTrackUp`), kein Zittern, zurück auf Norden | ☑ | ☑ |
| B6 | Spur | Geflogen durchgezogen plus Punkte, bis an den Marker gezogen, Schalter „Track“ | ☑ | ☑ |
| B7 | Route/OFP | Gestrichelt, Wegpunkte mit Namen ab Zoom 6,5, nächster Wegpunkt hervorgehoben, spät geladene OFP erscheint | ☑ | ☑ |
| B8 | TOC/TOD | Eigene Farbe und Größe | ☑ | ☑ |
| B9 | Start-/Ziel-Pins | ICAO an beiden Plätzen | ☑ | ☑ |
| B10 | Datumsgrenze | Route und Spur laufen nicht quer über die Welt | ☑ | – |
| B11 | Fluginfo-Panel | Phase, Höhe, GS, Rest, ETA, Fortschritt, Muster/Registrierung; einklappbar | ☑ | ☑ |
| B12 | Ereignisliste | Einträge; trägt ein Eintrag eine Position, fliegt der Klick dorthin (der heutige Protokollstrom liefert keine Position) | ☑ | ☑ |
| B13 | Resume-Schutz | Während `was_just_resumed` kein Folgen auf die Ladeposition | ☑ | – |

## C. Taxi

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| C1 | Bodendaten nach Sichtbereich | Lädt ab Zoom 11, was im Bild ist, ohne Flug; auch jenseits der Datumsgrenze | ☑ | ☑ |
| C2 | Darstellung | Rollwege grün/blau/orange, Stände, Haltepunkte, Bahnen, Terminals, Beschriftung je Zoom; unter Route und Track | ☑ | ☑ |
| C3 | Schalter „Taxiweg“ | Blendet alle Ebenen, Tooltip nennt geladene Plätze | ☑ | ☑ |

## D. VA-Verkehr

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| D1 | Kollegen-Marker | 8-s-Abruf, eigener Flug nicht doppelt, Koppelnavigation zwischen den Abrufen | ☑ | ☑ |
| D2 | Infokarte | Rufzeichen, Pilot, Route, Phase, Höhe, GS, ETA; folgt beim Neuaufbau; zweiter Klick schließt | ☑ | ☑ |
| D3 | Fremde Routen | Ein/Aus per Klick, mehrere zugleich, Punkte und Namen, Hinweis bei fehlender Route | ☑ | ☑ |
| D4 | Aufräumen | Gelandete Kollegen verlieren die Route, Netzfehler und Ausschalten nicht | ☑ | – |
| D5 | Schalter „VA-Verkehr“ | Marker und Routen weg, beim Einschalten wieder da | ☑ | ☑ |
| D6 | Einpassen | Ohne Flug einmal auf die Kollegen, eigene Bildpunkte gehen vor | ☑ | ☑ |
| D7 | Zähler im Kopf | Anzahl Kollegen | ☑ | ☑ |

## E. Online-Netz

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| E1 | Aus/VATSIM/IVAO | Nie zwei zugleich, Start immer Aus, Wechsel ohne Umweg über Aus | ☑ | ☑ |
| E2 | Sektoren | Farben je Station, Beschriftung, ohne Naht an ±180° | ☑ | ☑ |
| E3 | FL-Regler | Nur VATSIM, wirkt sofort, „eigene Höhe“ folgt dem Flug | ☑ | ☑ |
| E4 | Lotsen und Plätze | Farben nach Station, App-Ring, ATIS-Buchstaben | ☑ | ☑ |
| E5 | Fremde Piloten | Ab Zoom 4,5, gedreht | ☑ | ☑ |
| E6 | Klickfenster | Pilot, Platz, Sektor; nur eines zugleich, Vorrang bei Überlappung | ☑ | ☑ |
| E7 | Zeit bis sichtbar | Ursache der 20–25 s beim ersten Zuschalten gemessen und behoben; Messcode raus | ☐ | ☐ |

## F. Bilder

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| F1 | Punkte ohne Flug | Alle anzeigbaren Bilder aller Flüge, einmal eingepasst, Schalter „Bilder“ sichtbar | ☑ | ☑ |
| F2 | Punkte mit Flug | Bilder des Flugs plus Fokus | ☑ | ☑ |
| F3 | Marke öffnen | Großansicht mit den Bildern desselben Flugs, Vorschau beim Überfahren | ☑ | ☑ |
| F4 | Fokus aus dem Bilder-Tab | Karte fliegt hin, Schalter geht an | ☑ | ☑ |
| F5 | Leer-Kasten | Verdeckt keine Punkte oder Kollegen | ☑ | ☑ |
| F6 | Punkte gleich beim Öffnen | Ohne Umweg über den Bilder-Tab da, auch nach Neustart, ohne Flug und während Kollegen fliegen; im echten Client belegt | ☑ | ☐ |

## G. Oberfläche und Einbindung

| Nr | Funktion | Erwartung | Test | Klick |
|----|----------|-----------|------|-------|
| G1 | Leiste | Wenige Gruppen, feste Knopfmaße: an/aus gleich breit, kein Springen | ☑ | ☑ |
| G2 | Kein gemessenes Layout | Kein `--aa-panel-oben`, kein Inline-Pflaster; Hinweis nie unter der Leiste | ☑ | ☑ |
| G3 | Jede Fenstergröße | Sechs Größen oben: nichts überdeckt, nichts abgeschnitten, alle Knöpfe erreichbar | – | ☑ |
| G4 | Kopfzeile | Titel, Flug oder nächster Bid, Uhr | ☑ | ☑ |
| G5 | Systemzeile | Recorder-Zustand, letzter Versand | ☑ | ☑ |
| G6 | Tooltips, Tastatur, Screenreader | `aria-pressed`, `aria-label`, `title` an jedem Schalter | ☑ | ☑ |
| G7 | Tablet/LAN-Brücke | Karte läuft über die Brücke, Tippen trifft (Marken nicht ständig neu gebaut) | ☑ | ☑ |
| G8 | Drei Sprachen | de/en/it ohne fehlende Schlüssel | ☑ | ☑ |
| G9 | Paket-Hinweis | `paket.neu_text` beschreibt den Gesamtumfang | ☑ | – |
