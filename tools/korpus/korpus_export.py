#!/usr/bin/env python3
"""Exportiert je Landung die Eingangsgroessen der NEUEN Achsen als CSV.

Die Projektion nutzt dieselbe Kugelmathematik wie `runway::projiziere_auf_bahn`
(Cross-Track ueber asin, Along-Track ueber acos) — nicht die ebene Naeherung.
Gegenprobe: der Aufsetzpunkt muss den vom Client gemeldeten Wert treffen.

Das Messfenster ist die Spec §5.2: Start bei on_ground + >=40 kt auf der Bahn,
Ende bei >10 Grad Kursabweichung, <60 kt, hinter dem Bahnende oder rueckwaerts.
"""
import gzip, json, glob, math, os, csv, sys, sqlite3

R = 6371008.8  # EARTH_RADIUS_M wie im Rust-Code

def bearing(la1, lo1, la2, lo2):
    p1, p2 = math.radians(la1), math.radians(la2)
    dl = math.radians(lo2 - lo1)
    y = math.sin(dl) * math.cos(p2)
    x = math.cos(p1) * math.sin(p2) - math.sin(p1) * math.cos(p2) * math.cos(dl)
    return math.atan2(y, x)

def hav(la1, lo1, la2, lo2):
    p1, p2 = math.radians(la1), math.radians(la2)
    dp = p2 - p1
    dl = math.radians(lo2 - lo1)
    a = math.sin(dp/2)**2 + math.cos(p1)*math.cos(p2)*math.sin(dl/2)**2
    return 2 * R * math.asin(math.sqrt(a))

def projiziere(tla, tlo, ela, elo, la, lo):
    """1:1 wie runway::projiziere_auf_bahn."""
    t_ab = bearing(tla, tlo, ela, elo)
    t_ac = bearing(tla, tlo, la, lo)
    d_ab = hav(tla, tlo, la, lo)
    xtd = math.sin(d_ab / R) * math.sin(t_ac - t_ab)
    quer = math.asin(max(-1.0, min(1.0, xtd))) * R
    cos_arg = max(-1.0, min(1.0, math.cos(d_ab / R) / math.cos(quer / R)))
    laengs = math.acos(cos_arg) * R
    diff = t_ac - t_ab
    while diff > math.pi:  diff -= 2*math.pi
    while diff <= -math.pi: diff += 2*math.pi
    if abs(diff) > math.pi/2:
        laengs = -laengs
    return laengs, quer

con = sqlite3.connect("/var/lib/aeroacars-recorder/aeroacars-live.db")


def versteckter_versatz_ft(icao, ident, len_ft, geo_m):
    """Spiegel von runway::geometry_hidden_displacement_ft.

    Der DFD-Export liefert die Schwelle seit AIRAC 2608 bereits versetzt und
    setzt das Zahlenfeld auf 0. Ohne diese Funktion rechnet der Export mit der
    vollen Bahnlaenge statt mit der nutzbaren - bei EDDH 23 sind das 3250 statt
    3094 m, und `overrun_m` misst dann 156 m zu spaet.
    """
    if not len_ft or len_ft <= 0 or not geo_m:
        return 0
    treffer = []
    sql = ("SELECT le_ident, le_displaced_threshold_ft, he_ident, "
           "he_displaced_threshold_ft FROM runways WHERE airport_icao=?")
    for le, lef, he, hef in con.execute(sql, (icao.upper(),)):
        if (le or "").strip().upper() == ident:
            treffer.append((lef or 0, hef or 0))
        elif (he or "").strip().upper() == ident:
            treffer.append((hef or 0, lef or 0))
    # Widerspruechliche Doppeleintraege: lieber gar nichts.
    if len(treffer) > 1 and any(t != treffer[0] for t in treffer):
        return 0
    if not treffer:
        return 0
    eigen, gegen = treffer[0]
    if eigen <= 0 or eigen >= len_ft * 0.5:
        return 0
    # Die Probe: Laenge minus BEIDE Versaetze muss die Geometrie ergeben.
    erwartet_m = (len_ft - eigen - max(gegen, 0)) * 0.3048
    if abs(erwartet_m - geo_m) > 40.0:
        return 0
    return eigen


def bahn_geometrie(icao, ident):
    """Navigraph zuerst (Spec §5.1), Breite und Belag aus OurAirports."""
    if not icao or not ident:
        return None
    d = str(ident).upper().strip().lstrip("0")
    for kand in (str(ident).upper().strip(), d, d.zfill(2)):
        r = con.execute(
            """SELECT threshold_latitude, threshold_longitude, end_latitude,
                      end_longitude, length_ft, width_ft, displaced_threshold_ft
               FROM nav_runways WHERE airport_icao=? AND
                     UPPER(REPLACE(designator,'RW',''))=? LIMIT 1""",
            (icao.upper(), kand)).fetchone()
        if r and r[0] is not None:
            surf = con.execute(
                """SELECT surface FROM runways WHERE airport_icao=?
                   AND (le_ident=? OR he_ident=?) LIMIT 1""",
                (icao.upper(), kand, kand)).fetchone()
            geo_m = hav(r[0], r[1], r[2], r[3])
            dds = max(r[6] or 0, versteckter_versatz_ft(icao, kand, r[4], geo_m))
            return dict(tla=r[0], tlo=r[1], ela=r[2], elo=r[3],
                        len_ft=r[4], width_ft=r[5], dds_ft=dds,
                        surface=(surf[0] if surf else None))
    return None

TITEL_MUSTER = [
    ("A220-300","BCS3"),("A220-100","BCS1"),("A330-300","A333"),("A330-200","A332"),
    ("A340-300","A343"),("A340-600","A346"),("MD-11F","MD11"),("MD-11","MD11"),
    ("L1011","L101"),("ATR 72-600","AT76"),("ATR 72","AT72"),("ATR 42","AT43"),
    ("P180","P180"),("FA50","FA50"),("C680","C680"),("VISION JET","SF50"),
]

MODELL_MAP = {
    "PHENOM 300E":"E55P","PHENOM 300":"E55P","PHENOM 100":"E50P",
    "A350-900":"A359","A350-1000":"A35K","A340-300":"A343","A340-600":"A346",
    "A330-300":"A333","A330-200":"A332","A380":"A388","A300":"A306","A400M":"A400",
    "HA420":"HA4T","FALCON 50":"FA50","C680+":"C680","C750":"C750","C182Q":"C182",
    "AC11":"AC11","BE36":"BE36","BE24":"BE24","BE58":"BE58","PA24":"PA24","PA34":"PA34",
    "F28":"F28","AEST":"AEST","SF50":"SF50",
}

def normalisiere_icao(roh):
    """Spiegel von sim_core::normalize_icao_type — MSFS liefert reihenweise
    Rohstrings wie `ATCCOM.AC_MODEL A320.0.text` statt sauberer Typcodes."""
    t = (roh or "").strip().upper()
    if not t: return ""
    # ATCCOM/AIRCRAFT-Praefixe und $$: entfernen
    for pre in ("ATCCOM.AC_MODEL_", "ATCCOM.AC_MODEL ", "AIRCRAFT.ATC_MODEL_", "$$:"):
        if t.startswith(pre): t = t[len(pre):]
    if t.endswith(".0.TEXT"): t = t[:-7]
    t = t.strip()
    if t in MODELL_MAP: return MODELL_MAP[t]
    if 2 <= len(t) <= 4 and t.isalnum(): return t
    return ""

def icao_aus_titel(titel):
    """Spiegel von sim_core::icao_aus_titel — Reihenfolge = Spezifitaet."""
    t = (titel or "").strip().upper()
    for muster, icao in TITEL_MUSTER:
        if muster in t:
            return icao
    return ""

KURS_AUSFAHRT = 10.0
MESS_MIN_GS = 60.0

def auswerten(pfad):
    td = None; subs = None; pos = []; titel = None; sim_icao = None
    td_ts = None
    try:
        with gzip.open(pfad, "rt") as fh:
            for line in fh:
                try: d = json.loads(line)
                except Exception: continue
                t = d.get("type")
                # Immer das LETZTE Aufsetzen, nicht das erste: Bei Platzrunden
                # mit Touch-and-Go setzt ein Flug mehrfach auf (gemessen: bis zu
                # dreimal, 7op4EybywvaWVnLr/EDHL). Bewertet wird die finale
                # Landung -- so macht es auch der Client, der seine Rollout-Werte
                # beim Steigflug nach einem Touch-and-Go zurücksetzt.
                if t == "touchdown_complete":
                    td = d.get("payload", {})
                elif t == "touchdown_detected":
                    # Der Zeitstempel des Aufsetzens. OHNE ihn beginnt das
                    # Messfenster bei der ersten Bodenposition ueber 40 kt --
                    # und das ist bei Fluegen, die am selben Platz starten und
                    # landen, der START. Gemessen an jBr89KbgmY4P8WKG (EDDL):
                    # dort lief das Fenster ueber den Startlauf, gs stieg von
                    # 128 auf 154 kt, die Spur bis 3769 m auf einer 2394-m-Bahn
                    # und der Querversatz auf 52,6 m -- ein reiner Messfehler,
                    # der als "Rad neben der Bahn" gezaehlt haette.
                    td_ts = d.get("timestamp") or ""

                elif t == "pirep_filed" and subs is None:
                    subs = d.get("payload", {}).get("sub_scores")
                elif t == "position":
                    s = d.get("snapshot", {})
                    if titel is None and s.get("aircraft_title"):
                        titel = s["aircraft_title"]
                    # aircraft_icao steht NUR in den Positions-Snapshots,
                    # nicht im Touchdown-Ereignis.
                    if sim_icao is None and s.get("aircraft_icao"):
                        sim_icao = s["aircraft_icao"]
                    if s.get("lat") is not None:
                        pos.append((d.get("timestamp",""), s["lat"], s["lon"],
                                    s.get("groundspeed_kt") or 0.0,
                                    s.get("heading_deg_true"),
                                    bool(s.get("on_ground"))))
    except Exception:
        return None
    if not td or td.get("airport_source") != "runway_match":
        return None
    if td_ts is None:
        td_ts = td.get("touchdown_at") or td.get("timestamp") or ""

    g = bahn_geometrie(td.get("runway_match_icao"), td.get("runway_match_ident") or td.get("runway"))
    if not g:
        return None

    lda_m = (g["len_ft"] - g["dds_ft"]) / 3.280839895
    if lda_m < 300:
        return None
    pos.sort()

    laeuft = False; max_quer = None; overrun = 0.0; proben = 0; letzte = -1e9
    fenster_zu = False
    # Die Geschwindigkeit am Anfang und am Ende des Messfensters. Beim
    # Ausrollen faellt sie -- immer. Steigt sie, misst das Fenster etwas
    # anderes als eine Landung (siehe Spec §12.6, Fehler 1).
    gs_start = None; gs_ende = None
    kurs_td = td.get("heading_true_deg")
    for ts, la, lo, gs, hdg, og in pos:
        # Alles vor dem Aufsetzen geht die Landung nichts an -- siehe oben.
        if td_ts and ts < td_ts:
            continue
        lg, qr = projiziere(g["tla"], g["tlo"], g["ela"], g["elo"], la, lo)
        halbe = (g["width_ft"] or 45.0) * 0.3048 / 2.0
        # Overrun heisst: beim AUSROLLEN geradeaus ueber das Ende geschossen.
        # Nicht: irgendwann spaeter hinter dem Bahnende gerollt. Deshalb nur
        # solange das Messfenster offen und die Spur noch auf der Bahnachse ist.
        if (laeuft and not fenster_zu and lg > lda_m and gs > 15.0
                and abs(qr) <= halbe + 10.0):
            overrun = max(overrun, lg - lda_m)
        if fenster_zu:
            continue
        if not laeuft:
            if not og or gs < 40.0 or lg < 0 or lg > lda_m or abs(qr) > 30:
                continue
            laeuft = True
        else:
            if lg < letzte - 60: fenster_zu = True; continue
            if gs < MESS_MIN_GS: fenster_zu = True; continue
            if hdg is not None and kurs_td is not None:
                diff = (hdg - kurs_td + 180) % 360 - 180
                if abs(diff) > KURS_AUSFAHRT: fenster_zu = True; continue
            if lg < 0 or lg > lda_m: fenster_zu = True; continue
        letzte = max(letzte, lg)
        if gs_start is None:
            gs_start = gs
        gs_ende = gs
        if max_quer is None or abs(qr) > abs(max_quer):
            max_quer = qr
        proben += 1

    alt_pts = ""; alt_val = ""
    for s in (subs or []):
        if s.get("key") == "rollout":
            alt_pts = s.get("score", "")
            alt_val = s.get("value", "")

    return dict(
        pirep=os.path.basename(pfad).split(".")[0],
        icao=td.get("runway_match_icao") or "", rwy=td.get("runway") or "",
        muster=normalisiere_icao(sim_icao) or icao_aus_titel(titel or ""),
        titel=(titel or "")[:40].replace(",", " "),
        td_m=round(td.get("td_distance_from_threshold_m") or 0.0, 1),
        lda_m=round(lda_m, 1),
        breite_m=round((g["width_ft"] or 0) * 0.3048, 1),
        belag=g["surface"] or "",
        max_quer_m=round(max_quer, 2) if max_quer is not None else "",
        overrun_m=round(overrun, 1) if overrun > 0 else "",
        proben=proben,
        gs_start=round(gs_start, 1) if gs_start is not None else "",
        gs_ende=round(gs_ende, 1) if gs_ende is not None else "",
        alt_punkte=alt_pts, alt_wert=alt_val,
    )

def main():
    files = sorted(glob.glob("/var/lib/aeroacars-recorder/flight-logs/*/*/*.jsonl.gz"))
    out = []
    for i, f in enumerate(files):
        r = auswerten(f)
        if r: out.append(r)
        if (i+1) % 200 == 0:
            print(f"  {i+1}/{len(files)}", file=sys.stderr, flush=True)
    with open("/tmp/korpus_v170.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(out[0].keys()))
        w.writeheader()
        for r in out: w.writerow(r)
    print(f"EXPORTIERT: {len(out)} von {len(files)}")
    mit_quer = sum(1 for r in out if r["max_quer_m"] != "")
    print(f"  mit seitlicher Messung: {mit_quer}")
    print(f"  mit Overrun:            {sum(1 for r in out if r['overrun_m'] != '')}")
    print(f"  ohne Muster:            {sum(1 for r in out if not r['muster'])}")

main()
