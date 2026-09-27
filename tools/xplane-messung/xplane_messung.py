#!/usr/bin/env python3
"""AeroACARS X-Plane-Messwerkzeug — findet heraus, wo ein Flugzeug seine
Cockpitschalter ablegt.

Warum: X-Plane-Add-ons (FlightFactor, ToLiss, Zibo, Felis …) schreiben
Strobes, Transponder, Anschnallzeichen, Klappen, Spoiler oder Autobrake oft
NICHT in die X-Plane-Standardwerte, sondern in eigene „Datarefs". Welche das
sind, weiß man nur, wenn man im Cockpit schaltet und zuschaut.

So geht es: Das Werkzeug liest über die Web-API von X-Plane ALLE Werte des
geladenen Flugzeugs. Dann sagt es dir Schritt für Schritt, welchen Schalter
du auf welche Stellung stellen sollst, und vergleicht nach jedem Schritt.
Was sich genau mit dem Schalter ändert, landet in der Ergebnisdatei.

Starten:   python3 xplane_messung.py
Ergebnis:  eine Datei je Flugzeug auf dem Schreibtisch
           (AeroACARS-Messung_<Typ>_<Datum>.json) — bitte an Thomas schicken.

Nur Python-Standardbibliothek, keine Installation nötig. Liest nur, ändert
in X-Plane nichts.
"""

from __future__ import annotations

import base64
import concurrent.futures as cf
import json
import os
import platform
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

VERSION = "1.0 (27.09.2026)"
BASIS = os.environ.get("XPLANE_WEBAPI", "http://127.0.0.1:8086/api/v1")
MAX_ARRAY = 48  # Array-Datarefs bis zu dieser Länge Element für Element

# ── Die Schritte ───────────────────────────────────────────────────────────
# (Schlüssel, Überschrift, Hinweis, Stellungen). Jede Stellung ist eine
# Anweisung; nach jeder drückst du Enter.
SCHRITTE = [
    ("beacon", "Beacon / Anti-Collision (rotes Blinklicht)", "",
     ["AUS", "AN"]),
    ("strobe", "Strobes (weiße Blitzlichter)",
     "Hat der Schalter nur zwei Stellungen, bei „AUTO“ einfach die Stellung lassen und Enter drücken.",
     ["OFF", "AUTO", "ON"]),
    ("nav", "Navigationslichter (NAV / POSITION)", "", ["AUS", "AN"]),
    ("landelicht", "Landelicht (LANDING)",
     "Bei mehreren Schaltern (links/rechts/Nase): alle zusammen.",
     ["AUS", "AN"]),
    ("taxilicht", "Taxi-Licht (TAXI / NOSE)", "", ["AUS", "AN"]),
    ("anschnall", "Anschnallzeichen (SEAT BELTS / FASTEN BELTS)",
     "Hat der Schalter kein AUTO, bei „AUTO“ einfach Enter drücken.",
     ["OFF", "AUTO", "ON"]),
    ("transponder", "Transponder-Modus",
     "Die Namen heißen je Flugzeug anders (STBY / ALT / XPNDR / AUTO / ON / TA ONLY / TA-RA). "
     "Gibt es eine Stellung nicht, einfach Enter drücken.",
     ["STBY", "ALT oder XPNDR (mit Höhe)", "AUTO (falls vorhanden)", "TA ONLY", "TA/RA"]),
    ("klappen", "Klappenhebel (FLAPS)",
     "Bitte jede Raste einzeln, von eingefahren bis voll. Hat das Flugzeug weniger Rasten, "
     "bleib bei den übrigen Anweisungen einfach auf voll und drück Enter.",
     ["UP / 0", "1. Raste", "2. Raste", "3. Raste", "4. Raste", "5. Raste", "6. Raste", "VOLL"]),
    ("spoiler", "Spoiler / Speedbrake",
     "ARMED = Hebel in die Arm-Raste (Landevorbereitung).",
     ["EINGEFAHREN (nicht armed)", "ARMED", "EINGEFAHREN (nicht armed)"]),
    ("autobrake", "Autobrake",
     "Alle Stufen, die das Flugzeug hat. Nicht vorhandene: Enter.",
     ["OFF / DISARM", "RTO", "1 / LO", "2 / MED", "3", "MAX / HI", "OFF / DISARM"]),
    ("apu", "APU Master (Hilfstriebwerk)",
     "Nur den Master-Schalter, die APU muss nicht anlaufen.",
     ["AUS", "AN", "AUS"]),
    ("parkbremse", "Parkbremse", "", ["GESETZT", "GELÖST", "GESETZT"]),
]

# ── Web-API ────────────────────────────────────────────────────────────────


def hole(pfad: str, timeout: float = 4.0):
    with urllib.request.urlopen(BASIS + pfad, timeout=timeout) as r:
        return json.load(r)


def web_api_da() -> bool:
    try:
        hole("/datarefs?filter[name]=sim/aircraft/view/acf_ICAO")
        return True
    except Exception:
        return False


def text_wert(roh) -> str:
    if isinstance(roh, str):
        try:
            b = base64.b64decode(roh)
        except Exception:
            return roh.strip()
        return b.split(b"\0")[0].decode("utf-8", "replace").strip()
    if isinstance(roh, list):
        return bytes(x for x in roh if isinstance(x, int) and 0 < x < 256).decode("utf-8", "replace").strip()
    return str(roh)


def flugzeug_info(alle: dict[str, dict]) -> dict:
    info = {}
    for feld in ["acf_ICAO", "acf_descrip", "acf_author", "acf_studio", "acf_relative_path", "acf_tailnum"]:
        d = alle.get("sim/aircraft/view/" + feld)
        if not d:
            continue
        try:
            info[feld] = text_wert(hole(f"/datarefs/{d['id']}/value")["data"])
        except Exception:
            pass
    return info


def lies_einen(d: dict):
    """Wert eines Datarefs; None, wenn nicht lesbar (Add-on nicht geladen)."""
    try:
        v = hole(f"/datarefs/{d['id']}/value", timeout=3.0)["data"]
    except Exception:
        return None
    return v


def schnappschuss(liste: list[dict]) -> dict[str, float]:
    """Alle Zahlenwerte auf einmal (parallel), Arrays elementweise."""
    werte: dict[str, float] = {}
    with cf.ThreadPoolExecutor(max_workers=24) as ex:
        for d, v in zip(liste, ex.map(lies_einen, liste)):
            if v is None:
                continue
            name = d["name"]
            if isinstance(v, list):
                for i, x in enumerate(v[:MAX_ARRAY]):
                    if isinstance(x, (int, float)):
                        werte[f"{name}[{i}]"] = float(x)
            elif isinstance(v, (int, float)):
                werte[name] = float(v)
    return werte


# ── Ablauf ─────────────────────────────────────────────────────────────────


def frage(text: str) -> str:
    try:
        return input(text)
    except EOFError:
        return ""


def zeile(t: str = "") -> None:
    print(t, flush=True)


def messe_flugzeug() -> str | None:
    zeile("\nLese die Liste aller Werte aus X-Plane …")
    try:
        alle_roh = hole("/datarefs", timeout=30)["data"]
    except Exception as e:
        zeile(f"  ✗ X-Plane antwortet nicht: {e}")
        return None
    alle = {d["name"]: d for d in alle_roh}
    info = flugzeug_info(alle)
    zeile(f"  Flugzeug: {info.get('acf_descrip', '?')}  ·  Typ {info.get('acf_ICAO', '?')}  ·  von {info.get('acf_author', '?')}")
    zeile(f"  Pfad:     {info.get('acf_relative_path', '?')}")
    zahlen = [d for d in alle_roh if d.get("value_type") in ("int", "float", "double", "int_array", "float_array")]
    zeile(f"  {len(zahlen)} Zahlenwerte gefunden.")

    zeile("\nSchritt 0 — Ruhemessung. Bitte 10 Sekunden NICHTS im Cockpit anfassen.")
    zeile("(Flugzeug am Boden, Parkbremse gesetzt, Simulator NICHT pausiert.)")
    frage("Enter drücken, wenn bereit … ")
    zeile("  messe …")
    t0 = time.time()
    a = schnappschuss(zahlen)
    zeile(f"  (ein Durchgang dauert {time.time() - t0:.1f} s)")
    unruhig: set[str] = set()
    for _ in range(3):
        time.sleep(2)
        b = schnappschuss(zahlen)
        unruhig |= {k for k in set(a) | set(b) if a.get(k) != b.get(k)}
        a = b
    zeile(f"  {len(unruhig)} Werte ändern sich von selbst — die werden ignoriert.")

    ergebnis = {
        "werkzeug": VERSION,
        "zeit_utc": datetime.utcnow().isoformat(timespec="seconds") + "Z",
        "rechner": platform.platform(),
        "flugzeug": info,
        "anzahl_werte": len(a),
        "unruhig": sorted(unruhig),
        "schritte": [],
    }

    for schluessel, titel, hinweis, stellungen in SCHRITTE:
        zeile("\n" + "─" * 70)
        zeile(f"{titel}")
        if hinweis:
            zeile(f"  Hinweis: {hinweis}")
        antw = frage("  Hat das Flugzeug diesen Schalter? Enter = ja, „n“ + Enter = überspringen … ").strip().lower()
        if antw.startswith("n"):
            ergebnis["schritte"].append({"schalter": schluessel, "uebersprungen": True})
            continue
        stand: list[dict[str, float]] = []
        namen: list[str] = []
        for s in stellungen:
            antw = frage(f"  → Stelle auf  [{s}]  und drück Enter  (s = gibt es nicht) … ").strip().lower()
            if antw == "s":
                continue
            time.sleep(1.0)  # Animation/Logik des Add-ons nachlaufen lassen
            stand.append(schnappschuss(zahlen))
            namen.append(s)
        # Kandidaten: Werte, die sich über die Stellungen ändern und nicht
        # von selbst unruhig sind.
        kand = []
        if len(stand) >= 2:
            schluessel_alle = set().union(*stand) - unruhig
            for k in schluessel_alle:
                reihe = [st.get(k) for st in stand]
                if len({round(x, 4) if x is not None else None for x in reihe}) > 1:
                    kand.append({"dataref": k, "werte": reihe})
        # Eigene Add-on-Werte zuerst, dann nach Anzahl verschiedener Werte.
        kand.sort(key=lambda c: (c["dataref"].startswith("sim/"), -len(set(map(str, c["werte"])))))
        ergebnis["schritte"].append({"schalter": schluessel, "stellungen": namen, "kandidaten": kand[:80]})
        zeile(f"  ✓ {len(kand)} Werte sind mitgegangen.")
        for c in kand[:5]:
            werte = " / ".join("—" if x is None else f"{x:g}" for x in c["werte"])
            zeile(f"     {c['dataref']}:  {werte}")

    typ = "".join(ch for ch in (info.get("acf_ICAO") or "Flugzeug") if ch.isalnum()) or "Flugzeug"
    ziel = os.path.join(os.path.expanduser("~/Desktop"), f"AeroACARS-Messung_{typ}_{datetime.now():%Y%m%d-%H%M}.json")
    with open(ziel, "w", encoding="utf-8") as f:
        json.dump(ergebnis, f, ensure_ascii=False, indent=1)
    zeile("\n" + "═" * 70)
    zeile(f"✓ Fertig. Gespeichert auf dem Schreibtisch:\n  {ziel}")
    return ziel


def main() -> None:
    zeile("═" * 70)
    zeile(f"AeroACARS X-Plane-Messwerkzeug {VERSION}")
    zeile("═" * 70)
    if not web_api_da():
        zeile("✗ X-Plane antwortet nicht auf der Web-API (Port 8086).")
        zeile("  1. Läuft X-Plane 12 und ist ein Flugzeug geladen?")
        zeile("  2. X-Plane → Einstellungen → Netzwerk → Web Server / Web API einschalten (Port 8086).")
        zeile("  Dann dieses Werkzeug neu starten.")
        frage("\nEnter zum Beenden … ")
        sys.exit(1)
    dateien = []
    while True:
        frage("\nLade das Flugzeug in X-Plane (am Boden, Triebwerke egal) und drück dann Enter … ")
        d = messe_flugzeug()
        if d:
            dateien.append(d)
        weiter = frage("\nNoch ein Flugzeug messen? Dann erst in X-Plane laden und „j“ + Enter, sonst nur Enter … ")
        if not weiter.strip().lower().startswith("j"):
            break
    zeile("\nDanke! Bitte diese Datei(en) an Thomas schicken (z. B. per Discord):")
    for d in dateien:
        zeile(f"  • {os.path.basename(d)}")
    frage("\nEnter zum Beenden … ")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        zeile("\nAbgebrochen.")
