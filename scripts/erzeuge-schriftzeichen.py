#!/usr/bin/env python3
"""Erneuert den Zeichenvorrat von B612 Mono für den Wächter.

Aufruf aus dem Repo-Wurzelverzeichnis:

    python3 scripts/erzeuge-schriftzeichen.py

Braucht `fontTools` (`pip install fonttools brotli`). Das Ergebnis liegt
in client/src/assets/fonts/b612mono-zeichen.json und wird von
scripts/pruefe-schriftzeichen.mjs gelesen.

Warum nicht direkt in Node: woff2 ist brotli-komprimiert, und eine
Schrift-Bibliothek nur für diese eine Prüfung ins Paket zu holen wäre
teurer als eine Datei, die sich selten ändert. Ändert sich die Schrift
doch, fällt es auf — der Wächter vergleicht die Prüfsummen mit.
"""

import hashlib
import json
import pathlib
import sys

try:
    from fontTools.ttLib import TTFont
except ImportError:
    sys.exit("fontTools fehlt: pip install fonttools brotli")

BASIS = pathlib.Path(__file__).resolve().parent.parent / "client/src/assets/fonts"
DATEIEN = ["B612Mono-Regular.woff2", "B612Mono-Bold.woff2"]

gemeinsam: set[int] | None = None
summen: dict[str, str] = {}
for name in DATEIEN:
    pfad = BASIS / name
    summen[name] = hashlib.sha256(pfad.read_bytes()).hexdigest()[:16]
    schrift = TTFont(pfad)
    vorhanden: set[int] = set()
    for tabelle in schrift["cmap"].tables:
        vorhanden |= set(tabelle.cmap.keys())
    # Schnittmenge: Ein Zeichen, das nur die fette Schnittform kennt,
    # fehlt in der normalen — und die Zeile steht mal so, mal so da.
    gemeinsam = vorhanden if gemeinsam is None else (gemeinsam & vorhanden)

assert gemeinsam is not None
# Nur der Bereich, der in Texten vorkommt. Die ganze cmap als Liste wäre
# unnötig groß und sägt am Nutzen der Datei.
relevant = sorted(cp for cp in gemeinsam if 0x20 <= cp <= 0x2FFF)

ZIEL = BASIS / "b612mono-zeichen.json"
ZIEL.write_text(
    json.dumps(
        {
            "_hinweis": "Erzeugt aus den B612-Mono-Dateien. "
            "Erneuern mit scripts/erzeuge-schriftzeichen.py",
            "schriften": summen,
            "zeichen": relevant,
        },
        ensure_ascii=False,
        indent=2,
    )
    + "\n",
    encoding="utf8",
)
print(f"{ZIEL}: {len(relevant)} Zeichen, Schriften {summen}")
