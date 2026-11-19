#!/usr/bin/env python3
"""Freigabeweg eines Laufzeit-Pakets (AeroACARS 3.0, P-3.12, P-9.43).

Rueckgabe 0: das neue Paket unterscheidet sich vom veroeffentlichten Stand auf
live ausschliesslich in ourairports/ -> darf ohne Einzelfreigabe hinaus.
Rueckgabe 1: alles andere (auch: kein veroeffentlichter Stand lesbar) ->
Freigabe durch Thomas.

Mit --unveraendert (19.11.2026): Rueckgabe 0, wenn KEINE Datei abweicht — der
Lauf erzeugt dann keine Generation (ein Versions-Commit baute vorher eine neue
Generation ohne neuen Inhalt). Kein Stand lesbar -> 1, der Lauf geht weiter.

Aufruf: python3 -I freigabeweg.py [--unveraendert] <paket/manifest.json> <live-basis | Manifest-Datei>
Ist das zweite Argument eine Datei, wird sie als veroeffentlichtes Manifest
gelesen (Tests); sonst wird es von <basis>/api/bundle/current geholt.
"""
import json
import os
import sys
import urllib.request


def dateien(manifest: dict) -> dict:
    return {d["path"]: d["sha256"] for d in manifest["files"]}


def nur_ourairports(neu: dict, alt: dict) -> list:
    """Pfade ausserhalb von ourairports/, die sich unterscheiden."""
    n, a = dateien(neu), dateien(alt)
    return sorted(p for p in set(n) | set(a) if not p.startswith("ourairports/") and n.get(p) != a.get(p))


def abweichend(neu: dict, alt: dict) -> list:
    """Alle Pfade, die sich unterscheiden (auch neue oder fehlende)."""
    n, a = dateien(neu), dateien(alt)
    return sorted(p for p in set(n) | set(a) if n.get(p) != a.get(p))


def veroeffentlicht(quelle: str) -> dict:
    if os.path.isfile(quelle):
        with open(quelle, encoding="utf-8") as f:
            return json.load(f)
    with urllib.request.urlopen(quelle + "/api/bundle/current", timeout=30) as r:
        aktuell = json.load(r)
    with urllib.request.urlopen(quelle + aktuell["manifest"], timeout=30) as r:
        return json.load(r)


def main(argv) -> int:
    nur_gleich = len(argv) > 1 and argv[1] == "--unveraendert"
    if nur_gleich:
        argv = argv[:1] + argv[2:]
    with open(argv[1], encoding="utf-8") as f:
        neu = json.load(f)
    try:
        alt = veroeffentlicht(argv[2])
    except Exception as e:  # kein Stand lesbar -> immer Freigabe
        print("kein veroeffentlichter Stand lesbar:", e)
        return 1
    if nur_gleich:
        anders = abweichend(neu, alt)
        print("abweichend vom veroeffentlichten Stand:", anders or "nichts")
        return 1 if anders else 0
    andere = nur_ourairports(neu, alt)
    print("ausserhalb ourairports/ geaendert:", andere or "nichts")
    return 1 if andere else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
