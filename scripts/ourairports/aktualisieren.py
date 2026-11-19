#!/usr/bin/env python3
"""OurAirports-Wochenlauf (AeroACARS 3.0, Pflichtenheft P-9.40 bis P-9.43).

Holt airports.csv und runways.csv von github.com/davidmegginson/ourairports-data
zu einem festen Commit, setzt sie in das Format unter bundle/ourairports/ um
(runways.csv 1:1 bis auf die Kennung, airports.csv nur ident,type,latitude_deg,longitude_deg;
ident = icao_code, wo er abweicht) und
prueft den neuen Stand gegen den alten. Schwellen am 29.10.2026 an drei echten
Staenden geeicht (Pflichtenheft P-9.42).

Rueckgabe: 0 = kein Unterschied oder neuer Stand geschrieben; 2 = Halt (nichts
geschrieben, Bericht nennt die Gruende); 1 = Fehler.

GSG-Flughaefen: mit --gsg-url wird die aktuelle Liste (Plaetze aus Flugplaenen
und PIREPs der letzten 90 Tage) von live geholt; schlaegt der Abruf fehl oder
kommt kein gueltiger Code, gilt die feste Liste aus --gsg. Der Bericht nennt
die Quelle.

Aufruf: python3 -I aktualisieren.py --ziel bundle/ourairports
          [--commit SHA] [--gsg scripts/ourairports/gsg-flughaefen.txt]
          [--gsg-url https://live.kant.ovh/api/oeffentlich/gsg-flughaefen]
          [--bericht bericht.md] [--stand scripts/ourairports/stand.txt]
          [--roh-ordner DIR]
"""
import argparse
import csv
import io
import json
import math
import os
import re
import sys
import urllib.request

QUELLE = "https://raw.githubusercontent.com/davidmegginson/ourairports-data/{commit}/{datei}"
AIRPORTS_SPALTEN = ["ident", "type", "latitude_deg", "longitude_deg"]
# Kopfzeile des rohen airports.csv (Staende 27.10. und 29.10.2026); die
# umgesetzte Fassung hat immer dieselbe Kopfzeile, deshalb wird hier das
# Original geprueft (P-9.42 „Kopfzeilen nicht exakt wie erwartet“).
AIRPORTS_KOPF_ROH = ('"id","ident","type","name","latitude_deg","longitude_deg","elevation_ft","continent",'
                     '"iso_country","iso_region","municipality","scheduled_service","icao_code","iata_code",'
                     '"gps_code","local_code","home_link","wikipedia_link","keywords"')
RUNWAYS_KOPF = ('"id","airport_ref","airport_ident","length_ft","width_ft","surface","lighted","closed","le_ident",'
                '"le_latitude_deg","le_longitude_deg","le_elevation_ft","le_heading_degT","le_displaced_threshold_ft",'
                '"he_ident","he_latitude_deg","he_longitude_deg","he_elevation_ft","he_heading_degT","he_displaced_threshold_ft"')

# Geeicht 29.10.2026 (Woche: +0,05 % Plaetze, 28 Verschiebungen > 1 km;
# Monat: +0,17 %, 31 Verschiebungen).
ANZAHL_MINUS = -0.005
ANZAHL_PLUS = 0.02
VERSCHOBEN_KM = 1.0
VERSCHOBEN_HOECHSTENS = 100
BAHN_LAENGE_ANTEIL = 0.10
BAHN_KURS_GRAD = 5.0


def icao_zuordnung(roh: str) -> dict:
    """id -> ICAO: Plaetze, deren Kennung nicht ihr ICAO-Code ist.

    OurAirports fuehrt umbenannte Plaetze unter der alten Kennung (Lima: ident
    SPIM, icao_code SPJC; 845 Plaetze am 19.11.2026). Der Client sucht nach
    ICAO, fand SPJC nicht und meldete im Anflug einen Divert nach „SPIM“.
    Ist der Code schon eine Kennung, bleibt alles wie geliefert, auch bei
    einem geschlossenen Platz (FAHS: die richtigen Bahnen haengen an der
    geschlossenen Zeile, Test `a_wrong_reference_point_does_not_cost_an_airport_its_runways`).
    Den Code bekommt nur der erste Platz, der ihn traegt. Die neue Zeile steht
    HINTER der alten (gleiche Bahnen): Der Client entdoppelt Bahnen ueber die
    Geometrie und behaelt die erste Zeile in ICAO-Form, so bleiben aeltere
    Clients bei SPIM/MHSC (23 GSG-Fluege) wie bisher; ab dem Client mit
    `ZWILLINGE` (runway.rs) findet die zweite Kennung dieselben Bahnen. Die
    doppelte Bahn-id stoert nicht, der Client ordnet ueber airport_ident zu.
    """
    zeilen = list(csv.DictReader(io.StringIO(roh)))
    if not zeilen or "icao_code" not in zeilen[0] or "id" not in zeilen[0]:
        return {}
    nach_kennung = {z["ident"]: z for z in zeilen}
    um, vergeben = {}, set()
    for z in zeilen:
        icao = (z.get("icao_code") or "").strip()
        if not re.fullmatch(r"[A-Z0-9]{4}", icao) or icao == z["ident"] or z["type"] == "closed":
            continue
        if icao in vergeben:
            continue
        if icao in nach_kennung:
            continue
        um[z["id"]] = icao
        vergeben.add(icao)
    return um



def airports_umsetzen(roh: str) -> str:
    """Rohes airports.csv -> 4-Spalten-Fassung, Werte unveraendert als Text,
    Kennung = ICAO-Code, wo er abweicht ([`icao_zuordnung`])."""
    leser = csv.DictReader(io.StringIO(roh))
    fehlend = [s for s in AIRPORTS_SPALTEN if s not in (leser.fieldnames or [])]
    if fehlend:
        raise ValueError(f"airports.csv: Spalten fehlen: {fehlend}")
    um = icao_zuordnung(roh)
    aus = io.StringIO()
    schreiber = csv.writer(aus, lineterminator="\n")
    schreiber.writerow(AIRPORTS_SPALTEN)
    for z in leser:
        werte = [z[s] for s in AIRPORTS_SPALTEN]
        schreiber.writerow(werte)
        neu = um.get(z.get("id"))
        if neu is not None:
            schreiber.writerow([neu] + werte[1:])
    return aus.getvalue()


def runways_umsetzen(roh_rw: str, roh_ap: str) -> str:
    """Rohes runways.csv mit denselben Kennungen wie `airports_umsetzen`.
    Nur betroffene Zeilen aendern sich (airport_ident ueber airport_ref),
    alle anderen bleiben Byte fuer Byte wie geliefert."""
    um = icao_zuordnung(roh_ap)
    if not um:
        return roh_rw
    aus = []
    for i, zeile in enumerate(roh_rw.split("\n")):
        m = re.match(r'^("?\d+"?),("?)(\d+)\2,"([^"]*)",', zeile) if i else None
        if m is not None:
            neu = um.get(m.group(3))
            if neu is not None:
                aus.append(zeile)
                zeile = zeile[: m.start(4)] + neu + zeile[m.end(4):]
        aus.append(zeile)
    return "\n".join(aus)


def km(a, b):
    p = math.pi / 180
    la1, lo1, la2, lo2 = a[0] * p, a[1] * p, b[0] * p, b[1] * p
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 6371 * 2 * math.asin(min(1.0, math.sqrt(h)))


def plaetze(text: str) -> dict:
    out = {}
    for z in csv.DictReader(io.StringIO(text)):
        try:
            pos = (float(z["latitude_deg"]), float(z["longitude_deg"]))
        except (ValueError, KeyError):
            pos = None
        out[z["ident"]] = (z["type"], pos)
    return out


def bahnen(text: str) -> dict:
    out = {}
    for z in csv.DictReader(io.StringIO(text)):
        out[(z["airport_ident"], z["le_ident"], z["he_ident"])] = z
    return out


def zahl(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def pruefen(alt_ap: str, neu_ap: str, alt_rw: str, neu_rw: str, gsg: set, roh_ap_kopf=None, rw_kopf=None) -> list:
    """Liste der Haltegruende; leer = Stand darf hinaus."""
    gruende = []
    if roh_ap_kopf is not None and roh_ap_kopf.rstrip("\r") != AIRPORTS_KOPF_ROH:
        gruende.append("airports.csv (roh): Kopfzeile nicht wie erwartet")
    if rw_kopf is not None and neu_rw.split("\n", 1)[0].rstrip("\r") != rw_kopf:
        gruende.append("runways.csv: Kopfzeile nicht wie erwartet")
    if neu_rw.split("\n", 1)[0] != alt_rw.split("\n", 1)[0]:
        gruende.append("runways.csv: Kopfzeile geaendert")
    if neu_ap.split("\n", 1)[0] != alt_ap.split("\n", 1)[0]:
        gruende.append("airports.csv: Kopfzeile geaendert")
    a0, a1 = plaetze(alt_ap), plaetze(neu_ap)
    r0, r1 = bahnen(alt_rw), bahnen(neu_rw)
    for name, n0, n1 in (("Flughaefen", len(a0), len(a1)), ("Bahnen", len(r0), len(r1))):
        anteil = (n1 - n0) / n0 if n0 else 1.0
        if not (ANZAHL_MINUS <= anteil <= ANZAHL_PLUS):
            gruende.append(f"{name}: {n0} -> {n1} ({anteil:+.2%}) ausserhalb {ANZAHL_MINUS:+.1%}/{ANZAHL_PLUS:+.0%}")
    verschoben = [i for i in a0 if i in a1 and a0[i][1] and a1[i][1] and km(a0[i][1], a1[i][1]) > VERSCHOBEN_KM]
    if len(verschoben) > VERSCHOBEN_HOECHSTENS:
        gruende.append(f"{len(verschoben)} Flughaefen um > {VERSCHOBEN_KM} km verschoben (hoechstens {VERSCHOBEN_HOECHSTENS})")
    for i in sorted(gsg):
        if i in a0 and i not in a1:
            gruende.append(f"GSG-Flughafen {i} entfernt")
        elif i in a0 and a0[i][0] != a1[i][0]:
            gruende.append(f"GSG-Flughafen {i}: Typ {a0[i][0]} -> {a1[i][0]}")
        elif i in verschoben:
            gruende.append(f"GSG-Flughafen {i} um > {VERSCHOBEN_KM} km verschoben")
    for k, z0 in r0.items():
        if k[0] not in gsg:
            continue
        z1 = r1.get(k)
        if z1 is None:
            gruende.append(f"GSG-Bahn {k[0]} {k[1]}/{k[2]} entfernt oder umbenannt")
            continue
        l0, l1 = zahl(z0.get("length_ft")), zahl(z1.get("length_ft"))
        if l0 and l1 and abs(l1 - l0) / l0 > BAHN_LAENGE_ANTEIL:
            gruende.append(f"GSG-Bahn {k[0]} {k[1]}/{k[2]}: Laenge {l0:.0f} -> {l1:.0f} ft")
        h0, h1 = zahl(z0.get("le_heading_degT")), zahl(z1.get("le_heading_degT"))
        if h0 is not None and h1 is not None and abs((h1 - h0 + 180) % 360 - 180) > BAHN_KURS_GRAD:
            gruende.append(f"GSG-Bahn {k[0]} {k[1]}/{k[2]}: Kurs {h0:.1f} -> {h1:.1f}")
    return gruende


def holen(commit: str, datei: str, roh_ordner) -> str:
    if roh_ordner:
        with open(os.path.join(roh_ordner, datei), encoding="utf-8") as f:
            return f.read()
    with urllib.request.urlopen(QUELLE.format(commit=commit, datei=datei), timeout=120) as r:
        return r.read().decode("utf-8")


def abrufen(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "aeroacars-ourairports"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8")


def gsg_flughaefen(url, datei):
    """(Menge, Vermerk): Liste von live, sonst die feste Datei."""
    fest = set()
    if datei:
        with open(datei, encoding="utf-8") as f:
            fest = {z.strip().upper() for z in f if z.strip()}
    if not url:
        return fest, f"GSG-Flughaefen: feste Liste ({len(fest)})"
    try:
        codes = json.loads(abrufen(url)).get("icao")
        neu = {c.strip().upper() for c in codes if isinstance(c, str)} if isinstance(codes, list) else set()
        neu = {c for c in neu if re.fullmatch(r"[A-Z0-9]{4}", c)}
        if not neu:
            raise ValueError("keine gueltigen ICAO-Codes")
        # Eine unvollstaendige Antwort darf die GSG-Schwellen nicht still
        # abschwaechen: weniger als die Haelfte der festen Liste gilt als Ausfall.
        if len(neu) * 2 < len(fest):
            raise ValueError(f"nur {len(neu)} Codes, feste Liste hat {len(fest)}")
        return neu, f"GSG-Flughaefen: aktuelle Liste von live ({len(neu)})"
    except Exception as e:  # noqa: BLE001 - jeder Ausfall faellt auf die feste Liste zurueck
        return fest, (f"GSG-Flughaefen: Abruf von live fehlgeschlagen ({type(e).__name__}: {e}), "
                      f"feste Liste genutzt ({len(fest)})")


def main(argv=None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--ziel", required=True)
    p.add_argument("--commit", default="main")
    p.add_argument("--gsg", default=None)
    p.add_argument("--gsg-url", default=None, help="aktuelle GSG-Flughafenliste von live; Rueckfall auf --gsg")
    p.add_argument("--bericht", default=None)
    p.add_argument("--stand", default=None, help="Datei fuer den OurAirports-Commit des geschriebenen Stands (P-9.44)")
    p.add_argument("--roh-ordner", default=None, help="Rohdateien lokal statt von GitHub (Tests)")
    a = p.parse_args(argv)
    gsg, gsg_vermerk = gsg_flughaefen(a.gsg_url, a.gsg)
    if "fehlgeschlagen" in gsg_vermerk:
        print(f"::warning::{gsg_vermerk}")
    pfad_ap, pfad_rw = os.path.join(a.ziel, "airports.csv"), os.path.join(a.ziel, "runways.csv")
    with open(pfad_ap, encoding="utf-8") as f:
        alt_ap = f.read()
    with open(pfad_rw, encoding="utf-8") as f:
        alt_rw = f.read()
    roh_ap = holen(a.commit, "airports.csv", a.roh_ordner)
    neu_ap = airports_umsetzen(roh_ap)
    neu_rw = runways_umsetzen(holen(a.commit, "runways.csv", a.roh_ordner), roh_ap)
    bericht = [f"# OurAirports {a.commit}", "", gsg_vermerk, ""]
    if neu_ap == alt_ap and neu_rw == alt_rw:
        print("OurAirports: kein Unterschied")
        if a.bericht:
            with open(a.bericht, "w", encoding="utf-8") as f:
                f.write("\n".join(bericht + ["Kein Unterschied zum vorhandenen Stand."]) + "\n")
        return 0
    gruende = pruefen(alt_ap, neu_ap, alt_rw, neu_rw, gsg, roh_ap.split("\n", 1)[0], RUNWAYS_KOPF)
    if gruende:
        bericht += ["**Halt** — nichts geschrieben. Gruende:", ""] + [f"- {g}" for g in gruende]
    else:
        bericht += ["Alle Pruefungen bestanden, neuer Stand geschrieben."]
    text = "\n".join(bericht) + "\n"
    print(text)
    if a.bericht:
        with open(a.bericht, "w", encoding="utf-8") as f:
            f.write(text)
    if gruende:
        return 2
    with open(pfad_ap, "w", encoding="utf-8", newline="") as f:
        f.write(neu_ap)
    with open(pfad_rw, "w", encoding="utf-8", newline="") as f:
        f.write(neu_rw)
    if a.stand:
        with open(a.stand, "w", encoding="utf-8") as f:
            f.write(a.commit + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
