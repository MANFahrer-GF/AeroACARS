# -*- coding: utf-8 -*-
"""Ausfahrten einer Bahn aus der OSM-Bodenkarte.

Ein Rollweg ist eine Ausfahrt, wenn einer seiner Stuetzpunkte nahe an der
Bahnkante liegt. Der Name kommt aus `properties.r`.
"""
import sqlite3, json, math, sys
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

def ausfahrten(icao, ident):
    d = str(ident).upper().strip().lstrip("0")
    r = None
    for kand in (str(ident).upper().strip(), d, d.zfill(2)):
        r = con.execute("SELECT threshold_latitude,threshold_longitude,end_latitude,"
                        "end_longitude,width_ft FROM nav_runways WHERE airport_icao=? AND "
                        "UPPER(REPLACE(designator,'RW',''))=? LIMIT 1",(icao,kand)).fetchone()
        if r and r[0] is not None: break
    if not r or r[0] is None: return None
    T=(r[0],r[1],r[2],r[3]); halbe=(r[4] or 148)*0.3048/2
    laenge = hav(*T)
    def proj(la,lo):
        tla,tlo,ela,elo=T
        ab=kurs(tla,tlo,ela,elo); ac=kurs(tla,tlo,la,lo); dd=hav(tla,tlo,la,lo)
        q=math.asin(math.sin(dd/R)*math.sin(ac-ab))*R
        lg=math.acos(max(-1,min(1,math.cos(dd/R)/math.cos(q/R))))*R
        v=(math.degrees(ac-ab)+540)%360-180
        return (lg if abs(v)<90 else -lg), q

    row = con.execute("SELECT geojson FROM airport_ground WHERE icao=?", (icao,)).fetchone()
    if not row: return None
    gj = json.loads(row[0])
    treffer = {}
    for f in gj["features"]:
        p = f["properties"]
        if p.get("k") != "taxiway": continue
        g = f["geometry"]
        if g.get("type") != "LineString": continue
        name = (p.get("r") or "").strip()
        if not name: continue
        for lon, lat in g["coordinates"]:
            lg, q = proj(lat, lon)
            if lg < 20 or lg > laenge + 200: continue
            kant = abs(abs(q) - halbe)
            if kant > 25.0: continue
            seite = "right" if q > 0 else "left"
            key = (name, seite)
            if key not in treffer or kant < treffer[key][1]:
                treffer[key] = (round(lg,1), kant)
    return [dict(name=n, seite=s, laengs_m=lg)
            for (n,s),(lg,_) in sorted(treffer.items(), key=lambda x: x[1][0])]

alles = {}
for icao, rwy in [("EDDH","23"),("EDDH","05"),("EDDL","05R"),("EHAM","06")]:
    a = ausfahrten(icao, rwy)
    alles["%s/%s" % (icao, rwy)] = a or []
    print("%s/%s: %s" % (icao, rwy,
          ", ".join("%s@%.0fm %s" % (x["name"], x["laengs_m"], x["seite"]) for x in (a or [])[:12])),
          file=sys.stderr)
json.dump(alles, open("/tmp/ausfahrten.json","w"), ensure_ascii=False, indent=1)
