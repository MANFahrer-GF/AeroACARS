#!/usr/bin/env python3
"""Ende-zu-Ende-Test: echte .xpl in der XPLM-Attrappe + plugin_sonde.py.

  ende_zu_ende.py <aeroacars_attrappe> <plugin.xpl> <plugin_sonde.py>

Drei Durchläufe:
  1. "X-Plane 12": sonde pruefung muss grün sein (inkl. LISTE), Protokoll 1
     läuft währenddessen weiter mit ~20 Hz.
  2. "X-Plane 11": LISTE meldet liste_nicht_verfuegbar, sonst alles gleich.
  3. Port 52001 belegt: Plugin lädt trotzdem, meldet es im Log, Protokoll 1
     läuft weiter.

Rückgabe 0 = grün, 1 = rot, 77 = übersprungen (Port 52001 schon belegt, z. B.
weil X-Plane mit dem Plugin gerade läuft).
"""

from __future__ import annotations

import socket
import subprocess
import sys
import time


def port_frei(port: int) -> bool:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.bind(("127.0.0.1", port))
        return True
    except OSError:
        return False
    finally:
        s.close()


def lauf(attrappe: str, plugin: str, sonde: str, modus: str) -> bool:
    print(f"=== {modus} ===", flush=True)
    args = [attrappe, plugin, "12"] + (["xp11"] if modus == "xp11" else [])
    blocker = None
    if modus == "port_belegt":
        blocker = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        blocker.bind(("127.0.0.1", 52001))
    host = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    ok = True
    try:
        time.sleep(1.0)
        if modus != "port_belegt":
            s = subprocess.run([sys.executable, sonde, "pruefung"], capture_output=True, text=True, timeout=60)
            print(s.stdout, s.stderr, flush=True)
            if s.returncode != 0:
                print("FEHLER: sonde pruefung rot")
                ok = False
        log, _ = host.communicate(timeout=60)
    finally:
        if host.poll() is None:
            host.kill()
        if blocker:
            blocker.close()
    print(log, flush=True)
    if host.returncode != 0:
        print(f"FEHLER: Attrappe endete mit {host.returncode} (Protokoll-1-Rate?)")
        ok = False
    erwartet = {
        "xp12": ["Protokoll 2 bereit", "LISTE ja", "Client angemeldet"],
        "xp11": ["Protokoll 2 bereit", "LISTE nein", "Client angemeldet"],
        "port_belegt": ["belegt", "Protokoll 1 laeuft weiter"],
    }[modus]
    for text in erwartet:
        if text not in log:
            print(f"FEHLER: im Log fehlt {text!r}")
            ok = False
    if modus == "port_belegt" and "Protokoll 2 bereit" in log:
        print("FEHLER: Protokoll 2 trotz belegtem Port bereit")
        ok = False
    return ok


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__)
        return 2
    attrappe, plugin, sonde = sys.argv[1:]
    if not port_frei(52001):
        print("Port 52001 ist belegt (läuft X-Plane mit dem Plugin?) - übersprungen")
        return 77
    ergebnisse = [lauf(attrappe, plugin, sonde, m) for m in ("xp12", "xp11", "port_belegt")]
    print("Ergebnis:", "gruen" if all(ergebnisse) else "rot")
    return 0 if all(ergebnisse) else 1


if __name__ == "__main__":
    sys.exit(main())
