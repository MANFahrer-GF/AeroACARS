# AeroACARS am StreamDeck

Plugin für die Elgato-StreamDeck-Software (Windows und Mac). Es zeigt den
Flugstatus aus AeroACARS auf den Tasten und bedient Kabinenansagen,
Boarding-Musik und das VATSIM-CDM-Fenster. Mitgeliefert sind fertige
Profile für StreamDeck 15 (MK.2), XL und +.

## Wie es mit AeroACARS spricht

Nur über den lokalen Panel-Server von AeroACARS (`127.0.0.1:47847`), den
auch das MSFS-Panel nutzt. Keine PIN, kein „Fernzugriff“, nichts zu
koppeln. Das StreamDeck muss am selben PC hängen wie AeroACARS.

| Route | Zweck | ab Client |
|---|---|---|
| `GET /panel/status` | Flug, Phase, Zeiten, Sprit, Wetter … | alt |
| `GET /panel/debrief` | Landung mit Note | alt |
| `GET /panel/kabine` | Stumm, Lautstärken, Boarding-Musik, nächste Ansage | neu |
| `GET /panel/hoppie` | Station, offene Freigaben | neu |
| `POST /panel/cmd/{name}` | Kabine stumm, Lautstärken, Boarding-Musik, CDM-Fenster | neu |

Mit einem älteren Client gehen alle Flugtasten; die Kabinentasten zeigen
„Client zu alt“.

Befehle brauchen den Kopf `X-AeroACARS-Deck: 1` und dürfen keinen
`Origin`-Kopf tragen — so kann keine Webseite im Browser sie auslösen.
Was einen Flug beendet oder abbricht, gibt es am Deck absichtlich nicht.

## Bauen

```bash
npm ci
npm test
npm run pack
```

`npm run pack` erzeugt Manifest, Bilder und Profile aus `src/` neu
(`npm run erzeugen`, Plugin-Symbol braucht macOS' `sips`), bündelt
`src/plugin.js`, prüft mit `streamdeck validate` und legt
`dist/eu.german-sky-group.aeroacars.streamDeckPlugin` ab — die Datei, die
Piloten doppelklicken.

## Aufbau

| Datei | Inhalt |
|---|---|
| `src/tasten.js` | Was jede Taste zeigt (reine Funktionen, getestet) |
| `src/bild.js` | Tastenbilder als SVG, Icons |
| `src/belegung.js` | Drehregler und Belegung der drei Profile |
| `src/zustand.js` | Abfrage von AeroACARS, solange eine Taste sichtbar ist |
| `src/plugin.js` | Verhalten: Zurück, Halten für Stumm, Regler |
| `skripte/erzeugen.mjs` | Manifest, Bilder, Profile aus den Quellen oben |

## Noch am Gerät zu prüfen

- Profile installieren und öffnen (auf 15er, XL, +). Die Modellkennung
  des 15er-Profils (`20GBA9901`) ist nicht aus einem echten Export belegt.
- Zurück über die Phasen-Taste (`switchToProfile` ohne Namen).
- Drehregler-Anzeige (`$B1`) auf dem +.
