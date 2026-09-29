#!/usr/bin/env python3
"""AeroACARS-Plugin-Sonde: spricht Protokoll 2 mit dem X-Plane-Plugin.

Schickt HALLO/ABO/LISTE/PING an 127.0.0.1:52001 und druckt die Antworten.
Nur Python-Standardbibliothek. Zum Prüfen am echten X-Plane (Plugin 1.0.0+).

Beispiele:
  plugin_sonde.py hallo
  plugin_sonde.py ping
  plugin_sonde.py abo sim/flightmodel/position/latitude sim/aircraft/view/acf_ICAO --rate 5 --dauer 5
  plugin_sonde.py abo "sim/flightmodel/engine/ENGN_N1_[0]" --rate 20
  plugin_sonde.py liste            # Anzahl + erste 20 Namen
  plugin_sonde.py liste --alle > datarefs.txt
  plugin_sonde.py pruefung         # automatische Abnahme, Rückgabecode 0 = alles gut
  plugin_sonde.py roh 'ABO 1 10\\nsim/x[abc]'   # beliebigen Text schicken

Die Sonde meldet sich wie der Client mit HALLO an und schickt jede Sekunde
PING (sonst verwirft das Plugin nach 5 s alle Abos).
"""

from __future__ import annotations

import argparse
import json
import socket
import sys
import time

PLUGIN_ADRESSE = ("127.0.0.1", 52001)
MAX_PAKET = 8192
MAX_DATAGRAMM = 65536


class Sonde:
    def __init__(self, port: int, zeige_roh: bool = False) -> None:
        self.ziel = ("127.0.0.1", port)
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        # Großer Empfangspuffer: LISTE und große Abos kommen in Schüben.
        try:
            self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        except OSError:
            pass
        self.sock.bind(("127.0.0.1", 0))
        self.zeige_roh = zeige_roh
        self.letzter_ping = 0.0
        self.probleme: list[str] = []

    def sende(self, text: str) -> None:
        daten = text.encode("utf-8")
        if len(daten) > MAX_DATAGRAMM:
            raise ValueError(f"Datagramm zu groß ({len(daten)} Byte)")
        self.sock.sendto(daten, self.ziel)

    def empfange(self, dauer: float, ping: bool = True):
        """Liefert (roh, json) für jedes Paket innerhalb von `dauer` Sekunden."""
        ende = time.monotonic() + dauer
        while True:
            jetzt = time.monotonic()
            if jetzt >= ende:
                return
            if ping and jetzt - self.letzter_ping >= 1.0:
                self.sende("PING\n")
                self.letzter_ping = jetzt
            self.sock.settimeout(max(0.01, min(0.25, ende - jetzt)))
            try:
                daten, von = self.sock.recvfrom(1 << 17)
            except socket.timeout:
                continue
            if von[0] != "127.0.0.1" or von[1] != self.ziel[1]:
                continue
            yield daten, self.pruefe(daten)

    def pruefe(self, daten: bytes):
        """Prüft ein Paket gegen die Protokollzusagen; gibt das JSON zurück."""
        if self.zeige_roh:
            print("  <-", daten[:400])
        if len(daten) > MAX_PAKET:
            self.probleme.append(f"Paket größer als 8192 Byte: {len(daten)}")
        if not daten.endswith(b"\n") or daten.count(b"\n") != 1:
            self.probleme.append("Paket ist nicht genau eine Zeile mit \\n am Ende")
        try:
            text = daten.decode("utf-8")
        except UnicodeDecodeError:
            self.probleme.append("Paket ist kein UTF-8")
            return None
        try:
            j = json.loads(text)
        except json.JSONDecodeError as e:
            self.probleme.append(f"kein gültiges JSON: {e}: {text[:200]}")
            return None
        if not isinstance(j, dict) or j.get("p") != 2 or "t" not in j:
            self.probleme.append(f"Paket ohne p=2/t: {text[:200]}")
        return j

    def warte_auf(self, typ: str, dauer: float = 2.0):
        for _, j in self.empfange(dauer):
            if j and j.get("t") == typ:
                return j
        return None

    def hallo(self):
        self.sende("HALLO 2 sonde-1.0\n")
        self.letzter_ping = time.monotonic()
        return self.warte_auf("hallo", 2.0)

    def abo_senden(self, abo_id: int, rate: int, namen: list[str]) -> None:
        """Schickt ein ABO, bei Bedarf mehrteilig (Datagramme ≤ 60 KiB)."""
        teile: list[list[str]] = [[]]
        groesse = 0
        for n in namen:
            if groesse + len(n) + 1 > 60000 and teile[-1]:
                teile.append([])
                groesse = 0
            teile[-1].append(n)
            groesse += len(n) + 1
        if len(teile) == 1:
            self.sende(f"ABO {abo_id} {rate}\n" + "".join(n + "\n" for n in namen))
            return
        for k, t in enumerate(teile, start=1):
            self.sende(f"ABO {abo_id} {rate} {k} {len(teile)}\n" + "".join(n + "\n" for n in t))


def sammle_teile(pakete: list[dict], feld: str) -> list:
    """Setzt teil/teile-Pakete in Reihenfolge zusammen."""
    pakete = sorted(pakete, key=lambda p: p["teil"])
    aus: list = []
    for p in pakete:
        aus.extend(p[feld])
    return aus


def lies_status(sonde: Sonde, abo_id: int, dauer: float = 3.0):
    """Wartet auf eine vollständige abo-Antwort. Werte davor werden ignoriert."""
    teile: dict[int, dict] = {}
    for _, j in sonde.empfange(dauer):
        if not j:
            continue
        if j.get("t") == "fehler":
            print("  fehler:", j)
            return None
        if j.get("t") == "abo" and j.get("abo") == abo_id:
            teile[j["teil"]] = j
            if len(teile) == j["teile"]:
                return sammle_teile(list(teile.values()), "st")
    return None


def status_text(s: list) -> str:
    return s[1] if len(s) == 2 else f"{s[1]}[{s[2]}]" if s[1] in ("vf", "vi", "b") else s[1]


# ---------------------------------------------------------------------------
# Befehle
# ---------------------------------------------------------------------------

def befehl_hallo(sonde: Sonde, args) -> int:
    h = sonde.hallo()
    if not h:
        print("Keine Antwort auf HALLO — läuft X-Plane mit dem Plugin 1.0.0+?")
        return 1
    print("hallo:", json.dumps(h, ensure_ascii=False))
    f = sonde.warte_auf("flugzeug", 1.0)
    if f:
        print("flugzeug:", json.dumps(f, ensure_ascii=False))
    return 0


def befehl_ping(sonde: Sonde, args) -> int:
    t0 = time.monotonic()
    sonde.sende("PING\n")
    for _, j in sonde.empfange(2.0, ping=False):
        if j:
            print(f"{(time.monotonic() - t0) * 1000:.1f} ms:", json.dumps(j, ensure_ascii=False))
            return 0
    print("Keine Antwort.")
    return 1


def befehl_abo(sonde: Sonde, args) -> int:
    if not sonde.hallo():
        print("Keine Antwort auf HALLO.")
        return 1
    namen = args.namen
    sonde.abo_senden(args.id, args.rate, namen)
    st = lies_status(sonde, args.id)
    if st is None:
        print("Keine abo-Antwort.")
        return 1
    print(f"Status ({len(st)} Namen):")
    for s in st[:200]:
        print(f"  [{s[0]:>4}] {status_text(s):<8} {namen[s[0]]}")
    lieferungen = 0
    letzte: dict[int, object] = {}
    seq_teile: dict[int, int] = {}
    t0 = time.monotonic()
    for _, j in sonde.empfange(args.dauer):
        if not j:
            continue
        t = j.get("t")
        if t == "w" and j.get("abo") == args.id:
            seq_teile[j["seq"]] = seq_teile.get(j["seq"], 0) + 1
            for idx, wert in j["v"]:
                letzte[idx] = wert
            if j["teil"] == j["teile"]:
                lieferungen += 1
                if not args.still:
                    kurz = {namen[i].split("/")[-1]: letzte[i] for i in list(letzte)[:6]}
                    print(f"  seq {j['seq']:>5} ({j['teile']} Teil(e)): {json.dumps(kurz, ensure_ascii=False)[:160]}")
        elif t == "abo":
            print("  NEUE abo-Antwort (Status geändert):", j.get("st", [])[:10])
        elif t in ("flugzeug", "fehler"):
            print(f"  {t}:", json.dumps(j, ensure_ascii=False))
    dauer = time.monotonic() - t0
    print(f"{lieferungen} Lieferungen in {dauer:.1f} s = {lieferungen / dauer:.1f} Hz (Soll {args.rate} Hz)")
    print("Letzte Werte:")
    for i in sorted(letzte)[:200]:
        print(f"  {namen[i]} = {json.dumps(letzte[i], ensure_ascii=False)[:120]}")
    sonde.sende(f"ENDE-ABO {args.id}\n")
    return 0


def befehl_liste(sonde: Sonde, args) -> int:
    if not sonde.hallo():
        print("Keine Antwort auf HALLO.")
        return 1
    t0 = time.monotonic()
    sonde.sende("LISTE 1\n")
    teile: dict[int, dict] = {}
    for _, j in sonde.empfange(30.0):
        if not j:
            continue
        if j.get("t") == "fehler":
            print("fehler:", j)
            return 1
        if j.get("t") == "liste" and j.get("id") == 1:
            teile[j["teil"]] = j
            if len(teile) == j["teile"]:
                break
    if not teile:
        print("Keine LISTE-Antwort.")
        return 1
    erwartet = next(iter(teile.values()))["teile"]
    namen = sammle_teile(list(teile.values()), "n")
    print(f"{len(namen)} Namen in {len(teile)}/{erwartet} Paketen, {time.monotonic() - t0:.2f} s",
          file=sys.stderr)
    for n in (namen if args.alle else namen[:20]):
        print(n)
    return 0 if len(teile) == erwartet else 1


def befehl_roh(sonde: Sonde, args) -> int:
    text = args.text.encode("utf-8").decode("unicode_escape")
    sonde.zeige_roh = True
    if not args.ohne_hallo:
        sonde.hallo()
    print("  ->", text.encode("utf-8")[:400])
    sonde.sende(text)
    for _ in sonde.empfange(args.dauer):
        pass
    return 0


def befehl_pruefung(sonde: Sonde, args) -> int:
    """Automatische Abnahme am laufenden X-Plane."""
    ergebnisse: list[tuple[bool, str]] = []

    def pruefe(ok: bool, text: str) -> None:
        ergebnisse.append((ok, text))
        print(("  ok    " if ok else "  FEHLER ") + text)

    print("1. HALLO")
    h = sonde.hallo()
    pruefe(h is not None, f"hallo-Antwort: {h}")
    if not h:
        return 1
    pruefe(h.get("plugin", "").startswith("1."), f"Plugin-Version {h.get('plugin')}")
    xplm = h.get("xplm", 0)
    f = sonde.warte_auf("flugzeug", 1.5)
    pruefe(f is not None, f"flugzeug-Meldung: {f}")

    print("2. ABO mit bekannten, fehlenden und Array-Namen")
    namen = [
        "sim/flightmodel/position/latitude",       # d
        "sim/flightmodel/position/longitude",      # d
        "sim/aircraft/view/acf_ICAO",              # b
        "sim/time/paused",                         # i
        "sim/time/is_in_replay",                   # i
        "sim/flightmodel/engine/ENGN_N1_",         # vf
        "sim/flightmodel/engine/ENGN_N1_[0]",      # f
        "sim/flightmodel/engine/ENGN_N1_[999999]", # fehlt (Index zu groß)
        "sim/time/paused[0]",                      # fehlt (Skalar mit Index)
        "gibt/es/nicht/aeroacars_sonde",           # fehlt
    ]
    sonde.abo_senden(1, 10, namen)
    st = lies_status(sonde, 1)
    pruefe(st is not None and len(st) == len(namen), f"Status für {len(namen)} Namen")
    if st:
        erwartung = ["d", "d", "b", "i", "i", "vf", "f", "fehlt", "fehlt", "fehlt"]
        for s, soll in zip(st, erwartung):
            pruefe(s[1] == soll, f"{namen[s[0]]}: {status_text(s)} (erwartet {soll})")
    print("3. Werte 3 s lang bei 10 Hz")
    lieferungen, gesehen = 0, {}
    t0 = time.monotonic()
    for _, j in sonde.empfange(3.0):
        if j and j.get("t") == "w" and j.get("abo") == 1:
            for idx, wert in j["v"]:
                gesehen[idx] = wert
            if j["teil"] == j["teile"]:
                lieferungen += 1
    hz = lieferungen / (time.monotonic() - t0)
    pruefe(7.0 <= hz <= 11.0, f"Rate {hz:.1f} Hz (Soll 10)")
    pruefe(0 in gesehen and isinstance(gesehen[0], float), f"Breite {gesehen.get(0)}")
    pruefe(2 in gesehen and isinstance(gesehen[2], str), f"ICAO {gesehen.get(2)!r}")
    pruefe(not any(i in gesehen for i in (7, 8, 9)), "fehlende Namen liefern keinen Wert")
    sonde.sende("ENDE-ABO 1\n")

    print("4. Fehlerbehandlung")
    sonde.sende("ABO 1 10\nname mit leerzeichen\n")
    fj = sonde.warte_auf("fehler", 1.0)
    pruefe(fj is not None and fj.get("grund") == "name_ungueltig", f"fehler-Antwort: {fj}")

    print("5. LISTE")
    sonde.sende("LISTE 5\n")
    teile: dict[int, dict] = {}
    fehler = None
    for _, j in sonde.empfange(20.0):
        if not j:
            continue
        if j.get("t") == "fehler":
            fehler = j
            break
        if j.get("t") == "liste" and j.get("id") == 5:
            teile[j["teil"]] = j
            if len(teile) == j["teile"]:
                break
    if xplm >= 400:
        vollst = bool(teile) and len(teile) == next(iter(teile.values()))["teile"]
        anzahl = len(sammle_teile(list(teile.values()), "n")) if teile else 0
        pruefe(vollst and anzahl > 1000, f"LISTE vollständig: {anzahl} Namen in {len(teile)} Paketen")
    else:
        pruefe(fehler is not None and fehler.get("grund") == "liste_nicht_verfuegbar",
               f"X-Plane 11: liste_nicht_verfuegbar ({fehler})")

    print("6. Paketprüfung")
    pruefe(not sonde.probleme, "alle Pakete gültig" if not sonde.probleme else "; ".join(sonde.probleme[:5]))

    rote = [t for ok, t in ergebnisse if not ok]
    print(f"\n{len(ergebnisse) - len(rote)}/{len(ergebnisse)} Prüfungen ok")
    return 0 if not rote else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", type=int, default=PLUGIN_ADRESSE[1], help="Steuer-Port des Plugins (52001)")
    ap.add_argument("--roh", action="store_true", help="jedes empfangene Paket roh zeigen")
    sub = ap.add_subparsers(dest="befehl", required=True)

    sub.add_parser("hallo", help="HALLO schicken, Antwort + Flugzeug zeigen")
    sub.add_parser("ping", help="PING schicken, Antwortzeit zeigen")
    p = sub.add_parser("abo", help="Namen abonnieren und Werte zeigen")
    p.add_argument("namen", nargs="+")
    p.add_argument("--rate", type=int, default=5)
    p.add_argument("--id", type=int, default=1)
    p.add_argument("--dauer", type=float, default=5.0)
    p.add_argument("--still", action="store_true", help="nur Zusammenfassung")
    p = sub.add_parser("liste", help="alle Dataref-Namen holen (X-Plane 12)")
    p.add_argument("--alle", action="store_true", help="alle Namen drucken, nicht nur 20")
    p = sub.add_parser("roh", help="beliebigen Text schicken (\\n als Escape)")
    p.add_argument("text")
    p.add_argument("--dauer", type=float, default=2.0)
    p.add_argument("--ohne-hallo", action="store_true")
    sub.add_parser("pruefung", help="automatische Abnahme")

    args = ap.parse_args()
    sonde = Sonde(args.port, zeige_roh=args.roh)
    befehle = {
        "hallo": befehl_hallo, "ping": befehl_ping, "abo": befehl_abo,
        "liste": befehl_liste, "roh": befehl_roh, "pruefung": befehl_pruefung,
    }
    try:
        return befehle[args.befehl](sonde, args)
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
