"""Hinweis „Neu in Generation N“ darf nicht vergessen werden (Thomas 30.10.2026).

Die Piloten sehen nach dem Neustart nur `paket.neu_text` (PaketNeu.tsx). Bei
Generation 58 und beim ersten Paket mit „Meine Anfragen“ blieb der Text stehen
— die Piloten erfuhren nichts. Deshalb:

- Aendert sich seit der zuletzt freigegebenen Generation etwas, das Piloten
  sehen (Oberflaeche, Texte, Tabellen, Profile …), aber nicht `paket.neu_text`
  (DE), bricht der Paketbau ab. Bewusste Ausnahme: `[kein-hinweis]` in einer
  Commit-Nachricht seit der Basis (reine Interna).
- Immer: Hinweistext und Commits als Zusammenfassung des Laufs — die Seite, auf
  der Thomas freigibt.

python3 -I scripts/bundle/hinweis.py paket/notiz.json [--zusammenfassung DATEI]
"""
import argparse
import json
import subprocess
import sys

TEXT = "bundle/texte/de.json"
AUSNAHME = "[kein-hinweis]"


def sichtbar(pfad: str) -> bool:
    """Geht die Datei die Piloten etwas an? Tests, Demo-Seiten, OurAirports nicht."""
    if pfad.startswith("bundle/"):
        return not pfad.startswith(("bundle/ourairports/", "bundle/ui/"))
    if pfad == "client/index.html":
        return True
    if pfad.startswith("client/src/") and not pfad.startswith("client/src-tauri/"):
        return not (pfad.startswith("client/src/dev/") or "/__tests__/" in pfad
                    or ".test." in pfad.rsplit("/", 1)[-1])
    return False


def neu_text(json_text: str) -> str:
    try:
        return (json.loads(json_text).get("paket", {}).get("neu_text") or "").strip()
    except ValueError:
        return ""


def pruefen(dateien, text_basis, text_jetzt, nachrichten):
    """(ok, grund)."""
    relevant = [d for d in dateien if sichtbar(d)]
    if not relevant:
        return True, "keine fuer Piloten sichtbare Aenderung"
    if text_jetzt and text_jetzt != text_basis:
        return True, "Hinweistext neu"
    if any(AUSNAHME in n for n in nachrichten):
        return True, f"bewusst ohne Hinweis ({AUSNAHME})"
    return False, ("Piloten-sichtbare Aenderung ohne neuen Hinweistext: paket.neu_text in "
                   "bundle/texte/{de,en,it}.json anpassen (oder " + AUSNAHME + " in die Commit-Nachricht). "
                   "Betroffen: " + ", ".join(relevant[:8]) + (" …" if len(relevant) > 8 else ""))


def git(*args) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def main(argv=None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("notiz")
    p.add_argument("--zusammenfassung", default="")
    a = p.parse_args(argv)
    with open(a.notiz, encoding="utf-8") as f:
        n = json.load(f)
    basis = n.get("basis_commit")
    jetzt = neu_text(open(TEXT, encoding="utf-8").read())

    if basis:
        dateien = git("diff", "--name-only", f"{basis}..HEAD").split()
        nachrichten = git("log", "--format=%B%x00", f"{basis}..HEAD").split("\x00")
        try:
            alt = neu_text(git("show", f"{basis}:{TEXT}"))
        except subprocess.CalledProcessError:
            alt = ""
        ok, grund = pruefen(dateien, alt, jetzt, nachrichten)
    else:
        ok, grund = True, "keine freigegebene Basis bekannt — Pruefung entfaellt"

    zeilen = [
        f"## Generation {n.get('generation')} — was die Piloten sehen",
        "",
        f"> **Neu in Generation {n.get('generation')}:** {jetzt or '(kein Hinweistext)'}",
        "",
        f"Pruefung Hinweistext: **{'ok' if ok else 'FEHLT'}** — {grund}",
        "",
        f"### Aenderungen seit Generation {n.get('basis_generation') or '?'}",
    ] + [f"- `{c['sha']}` {c['titel']}" for c in n.get("commits", [])] + [""]
    text = "\n".join(zeilen)
    print(text)
    if a.zusammenfassung:
        with open(a.zusammenfassung, "a", encoding="utf-8") as f:
            f.write(text + "\n")
    if not ok:
        print(f"::error::{grund}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
