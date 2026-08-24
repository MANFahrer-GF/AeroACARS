#!/usr/bin/env python3
"""Vermisst ein gedrucktes PDF: Seiten, Schriftgrössen, zu kleine Stellen.

    python3 scripts/bericht-messen.py [bericht.pdf]

Wie gross eine Schrift auf dem Papier wird, ergibt sich erst beim Drucken.
Vorher wurde es gerechnet — 3,6 pt für die Bahn-Grafik, aus Seitenrand,
Spaltenbreite und viewBox. Diese Messung liest die Werte aus dem fertigen
PDF und braucht keine Annahme mehr.

Schwelle: 6 pt. Darunter ist Kleindruck auf Papier nicht mehr zu
entziffern; 7 bis 8 pt liest sich bequem.
"""
import sys, collections, pathlib
import fitz

SCHWELLE_PT = 6.0
# Der Vorgabepfad haengt am SKRIPT, nicht am Aufrufort. Aus `client/`
# gestartet (so laeuft `npm run bericht:pdf`) ging der relative Pfad ins
# Leere, und die Messung brach ab, statt zu messen.
STANDARD = pathlib.Path(__file__).resolve().parent.parent / "client" / "bericht-dist" / "bericht.pdf"
pfad = sys.argv[1] if len(sys.argv) > 1 else str(STANDARD)
doc = fitz.open(pfad)

print(f"{pfad}")
print(f"  {doc.page_count} Seite(n), {doc[0].rect.width:.0f}×{doc[0].rect.height:.0f} pt "
      f"({doc[0].rect.width/72*25.4:.0f}×{doc[0].rect.height/72*25.4:.0f} mm)")

groessen = collections.Counter()
zu_klein = []
for nr, seite in enumerate(doc, 1):
    for block in seite.get_text("dict")["blocks"]:
        for zeile in block.get("lines", []):
            for span in zeile["spans"]:
                t = span["text"].strip()
                if not t:
                    continue
                g = round(span["size"], 1)
                groessen[g] += len(t)
                if g < SCHWELLE_PT:
                    zu_klein.append((nr, g, t[:38]))

gesamt = sum(groessen.values())
print(f"\n  Schriftgrössen (nach Zeichenzahl):")
for g, n in sorted(groessen.items()):
    marke = "  ← unter der Schwelle" if g < SCHWELLE_PT else ""
    print(f"    {g:5.1f} pt   {n:6} Zeichen  {n/gesamt*100:5.1f} %{marke}")

if zu_klein:
    anteil = sum(len(t) for _, _, t in zu_klein) / gesamt * 100
    print(f"\n  ⚠ {len(zu_klein)} Textstellen unter {SCHWELLE_PT} pt ({anteil:.1f} % der Zeichen). Beispiele:")
    for nr, g, t in zu_klein[:12]:
        print(f"    S.{nr}  {g:4.1f} pt  „{t}“")
    sys.exit(1)

print(f"\n  ✓ Keine Textstelle unter {SCHWELLE_PT} pt.")
