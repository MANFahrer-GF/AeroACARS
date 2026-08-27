#!/usr/bin/env python3
"""Zieht den Achsen-Korpus aus der Recorder-Datenbank.

Hintergrund: Am 27.08.2026 meldete die Bahndisziplin bei 9 von 46
Landungen „Szenerie-Versatz" — an Plaetzen wie EDDK, EDDM und EGBB
nachweislich zu Unrecht. Ursache war das Fenster der Ausgleichsgeraden
(sie lief bis zur Bahnkante, also ueber das Ausschwenken hinweg).

Damit die Regel nicht wieder still verrutscht, liegt der Korpus als
Vorrat IM REPO und wird bei jedem Testlauf durchgerechnet — nicht nur,
wenn jemand an den VPS denkt.

Aufruf auf dem VPS:

    ssh live python3 /tmp/achsen_export.py > achsen_korpus.jsonl

⚠ Das Werkzeug gehoert ins Repo, nicht nach /tmp. Zwei Fehler in einem
frueheren Exportskript haben je ein Drittel der Landungen falsch
gemessen und dabei gruene Tests erzeugt (siehe korpus_v170.rs).
"""
import json
import sqlite3
import sys

DB = "/var/lib/aeroacars-recorder/aeroacars-live.db"
MINDEST_PROBEN = 10

def main() -> int:
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows = con.execute(
        """
        select payload_json from touchdowns
        where json_extract(payload_json,'$.lateral_samples') is not null
          and json_array_length(json_extract(payload_json,'$.lateral_samples')) >= ?
        order by ts desc
        """,
        (MINDEST_PROBEN,),
    ).fetchall()

    for (roh,) in rows:
        d = json.loads(roh)
        proben = [
            [round(s["laengs_m"], 1), round(s["quer_m"], 2)]
            for s in d.get("lateral_samples") or []
            if s.get("laengs_m") is not None and s.get("quer_m") is not None
        ]
        if len(proben) < MINDEST_PROBEN:
            continue
        # Nur was die Rechnung braucht — keine Pilotenkennung, keine Zeiten.
        print(json.dumps({
            "platz": d.get("airport"),
            "bahn": d.get("runway"),
            "version": d.get("client_version"),
            "breite_m": d.get("runway_width_m"),
            "mess_ende_m": d.get("mess_ende_laengs_m"),
            "raeum_m": d.get("scoring_cutoff_m"),
            "proben": proben,
        }, ensure_ascii=False, separators=(",", ":")))
    return 0

if __name__ == "__main__":
    sys.exit(main())
