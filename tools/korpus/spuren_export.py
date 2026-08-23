# -*- coding: utf-8 -*-
"""Echte Rollspuren — mit der Regel des Clients ab v1.7.0:

Die Aufzeichnung laeuft WEITER, wenn das Messfenster schliesst. Sie endet
erst, wenn das Flugzeug die Bahn seitlich verlassen hat (25 m jenseits der
Kante), steht (unter 5 kt) oder die Ablage voll ist. Der Raeumpunkt wird
markiert; ab dort wird nicht mehr gewertet, aber weiter gezeichnet.
"""
import gzip, json, glob, math, sqlite3, sys
R = 6371008.8
def hav(a,b,c,d):
    p1,p2=math.radians(a),math.radians(c)
    x=math.sin((p2-p1)/2)**2+math.cos(p1)*math.cos(p2)*math.sin(math.radians(d-b)/2)**2
    return 2*R*math.asin(math.sqrt(x))
def kurs(a,b,c,d):
    p1,p2=math.radians(a),math.radians(c); dl=math.radians(d-b)
    y=math.sin(dl)*math.cos(p2); x=math.cos(p1)*math.sin(p2)-math.sin(p1)*math.cos(p2)*math.cos(dl)
    return math.atan2(y,x)
con = sqlite3.connect("/var/lib/aeroacars-recorder/aeroacars-live.db")

MIN_ABSTAND_M = 10.0
MAX_PUNKTE    = 400
RAND_M        = 80.0
STOP_GS_KT    = 5.0
MESS_MIN_GS   = 60.0
KURS_AUSFAHRT = 10.0

def bahn(icao, ident):
    d = str(ident).upper().strip().lstrip("0")
    for kand in (str(ident).upper().strip(), d, d.zfill(2)):
        r = con.execute("SELECT threshold_latitude,threshold_longitude,end_latitude,"
                        "end_longitude,length_ft,width_ft FROM nav_runways WHERE "
                        "airport_icao=? AND UPPER(REPLACE(designator,'RW',''))=? LIMIT 1",
                        (icao.upper(), kand)).fetchone()
        if r and r[0] is not None:
            surf = con.execute("SELECT surface,le_ident,le_displaced_threshold_ft,he_ident,"
                               "he_displaced_threshold_ft FROM runways WHERE airport_icao=? "
                               "AND (le_ident=? OR he_ident=?) LIMIT 1",
                               (icao.upper(), kand, kand)).fetchone()
            dds = 0
            if surf:
                dds = (surf[2] or 0) if (surf[1] or "").strip().upper()==kand else (surf[4] or 0)
            return dict(T=(r[0],r[1],r[2],r[3]), len_ft=r[4], width_ft=r[5],
                        surface=(surf[0] if surf else None), dds_ft=dds)
    return None

def spur(pid, icao, ident):
    g = bahn(icao, ident)
    if not g: return None
    T = g["T"]; halbe = (g["width_ft"] or 148)*0.3048/2
    lda = (g["len_ft"] - g["dds_ft"]) / 3.280839895
    def proj(la, lo):
        tla,tlo,ela,elo = T
        ab=kurs(tla,tlo,ela,elo); ac=kurs(tla,tlo,la,lo); dd=hav(tla,tlo,la,lo)
        q=math.asin(math.sin(dd/R)*math.sin(ac-ab))*R
        lg=math.acos(max(-1,min(1,math.cos(dd/R)/math.cos(q/R))))*R
        v=(math.degrees(ac-ab)+540)%360-180
        return (lg if abs(v)<90 else -lg), q

    f = glob.glob("/var/lib/aeroacars-recorder/flight-logs/*/*/%s.jsonl.gz" % pid)
    if not f: return None
    pos=[]; td_ts=None; kurs_td=None; titel=None
    for line in gzip.open(f[0], "rt"):
        try: e=json.loads(line)
        except Exception: continue
        t=e.get("type")
        if t=="position":
            s=e.get("snapshot") or {}
            if s.get("lat") is not None:
                pos.append((e.get("timestamp",""), s))
                if titel is None and s.get("aircraft_title"): titel=s["aircraft_title"]
        elif t=="touchdown_detected":
            td_ts=e.get("timestamp") or ""; kurs_td=(e.get("payload") or e).get("heading_true_deg")
    pos.sort()
    if td_ts is None: return None

    punkte=[]; laeuft=False; fenster_zu=False; letzte=-1e9; raeum=None
    for ts, s in pos:
        if ts < td_ts: continue
        gs=s.get("groundspeed_kt") or 0.0
        hd=s.get("heading_deg_true"); og=bool(s.get("on_ground"))
        lg,q = proj(s["lat"], s["lon"])
        if not laeuft:
            if not og or gs < 40.0 or lg < 0 or lg > lda or abs(q) > 30: continue
            laeuft=True
        elif not fenster_zu:
            # Messfenster: hier wird BEWERTET.
            if lg < letzte - 60: fenster_zu = True
            elif gs < MESS_MIN_GS: fenster_zu = True
            elif hd is not None and kurs_td is not None and \
                 abs((hd - kurs_td + 180) % 360 - 180) > KURS_AUSFAHRT:
                diff = (hd - kurs_td + 180) % 360 - 180
                fenster_zu = True
                raeum = dict(m=round(lg,1), kt=round(gs,1),
                             seite=("right" if diff > 0 else "left"))
            elif lg < 0 or lg > lda: fenster_zu = True
        letzte = max(letzte, lg)
        # AUFZEICHNEN laeuft weiter, auch nach dem Schliessen.
        if gs < STOP_GS_KT: break
        if abs(q) > halbe + RAND_M: break
        if lg < -50: break
        if len(punkte) >= MAX_PUNKTE: break
        if not punkte or abs(lg - punkte[-1]["laengs_m"]) >= MIN_ABSTAND_M:
            punkte.append(dict(laengs_m=round(lg,1), quer_m=round(q,2)))
    # ── Raeumpunkt und Bewertungsgrenze ──────────────────────────────
    #
    # Zwei verschiedene Stellen, und sie duerfen nicht verwechselt werden:
    #
    #   kante_m  Wo die Spur die Bahnkante ueberschreitet und NICHT
    #            zurueckkommt. Das ist „Bahn geraeumt". Die blosse
    #            Ueberschreitung reicht nicht: Bei raKOnJD1XgNbP06q
    #            (EDDH 23) brach das Flugzeug mitten auf der Bahn auf
    #            26,9 m aus -- jenseits der 23-m-Kante -- und kam zurueck.
    #            Das ist der Befund, um den es geht, aber keine Ausfahrt.
    #
    #   m        Wo das Ausschwenken BEGANN. Ab hier wird nicht mehr
    #            bewertet, denn ein Flugzeug zieht Hunderte Meter vor der
    #            Ausfahrt nach aussen. Gemessen an 0Ab3v9EvNN1LKZ8z
    #            (EDDH 05): Mit der Kante als Grenze wurden 21,95 m
    #            gemeldet, direkt vor dem Raeumpunkt, auf einer Bahn mit
    #            23 m Halbbreite -- das war schon das Abrollen.
    if raeum is None:
        for i in range(1, len(punkte)):
            if abs(punkte[i]["quer_m"]) > halbe and abs(punkte[i-1]["quer_m"]) <= halbe:
                if all(abs(x["quer_m"]) > halbe for x in punkte[i:]):
                    # Rueckwaerts, solange der Betrag monoton faellt: Der
                    # erste Punkt, an dem er wieder steigt, ist der
                    # Umkehrpunkt -- dort begann das Ausschwenken.
                    j = i
                    while j > 0 and abs(punkte[j-1]["quer_m"]) < abs(punkte[j]["quer_m"]):
                        j -= 1
                    raeum = dict(m=punkte[j]["laengs_m"], kt=None,
                                 kante_m=punkte[i]["laengs_m"],
                                 seite=("right" if punkte[i]["quer_m"] > 0 else "left"))
                    break

    return dict(pirep=pid, icao=icao, rwy=ident, titel=titel, lda_m=round(lda,1),
                breite_m=round((g["width_ft"] or 0)*0.3048,1), belag=g["surface"],
                dds_ft=g["dds_ft"], raeum=raeum, punkte=punkte)

FAELLE = [("9K7B0OooywyjJ5jE","EDDH","23"),("raKOnJD1XgNbP06q","EDDH","23"),
          ("a3V0DXnWr6054VO6","EDDH","23"),("y75RLelRGWq7ogA3","EDDH","23"),
          ("G5K1Wb9DoWNLGme3","LGKR","34"),("85g91JXoQ0lDxgnX","KORD","27C"),
          ("zR4a18JGxVKZ84de","EDDL","05R"),("0Ab3v9EvNN1LKZ8z","EDDH","05"),
          ("qZozxjvMKd6lDQj6","EDDH","15")]
out=[]
for pid, ic, rw in FAELLE:
    r = spur(pid, ic, rw)
    if r and len(r["punkte"]) >= 5:
        out.append(r)
        p=r["punkte"]
        print("%-18s %-5s %-4s %3d Pkt  %5.0f..%5.0f m  quer %+6.1f..%+6.1f  raeum=%s"
              % (pid, ic, rw, len(p), p[0]["laengs_m"], p[-1]["laengs_m"],
                 min(x["quer_m"] for x in p), max(x["quer_m"] for x in p),
                 (r["raeum"] or {}).get("m","-")), file=sys.stderr)
json.dump(out, open("/tmp/echte_spuren.json","w"), ensure_ascii=False, indent=1)
