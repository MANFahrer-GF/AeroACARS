# AeroACARS Flow-Pro-Widget — Quelldateien

Die drei Quelldateien des In-Sim-HUD fuer **Flow Pro** (parallel42).
Stand: **v3.8** (dekodiertes METAR vom Server, v1.5.2+).

| Datei | Zweck |
|---|---|
| `code.js` | die gesamte Logik — **byte-identisch** mit `../Build/.../panel.js` |
| `code.css` | Darstellung (Coherent-GT-Regeln: kein flex-gap, nur ASCII) |
| `code.html` | die fast leere Buehne (`aa2-root`/`aa2-strip` + Flow-Drag) |

## Die eine Regel

**`code.js` und `panel.js` sind DIESELBE Datei** (eine Quelle, drei
Ziele: Flow, /hud-Browserseite via include_str!, natives Panel als
Referenz). Wer eine aendert, kopiert sie auf die andere und prueft:

```
cmp msfs-panel/flow-widget/code.js \
    msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.js
```

## Verteilweg an Piloten

Thomas spielt die Dateien in seinen Flow-Skripteintrag ein und
exportiert in Flow ein Community-Paket (`p42-util-flow-Thomas-
aeroacars-vX-Y.zip`). Das Zip wird nach Byte-Abgleich (code_js/
code_css/code_html in `Flow/templates/*.json` → `params`) als Asset an
das aktuelle GitHub-Release gehaengt; die GSG-Seite `/page/acars-tools`
verlinkt darauf.
