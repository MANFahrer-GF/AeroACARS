#!/usr/bin/env python3
"""X-Plane-RREF-Sonde: fragt Datarefs über denselben Weg ab wie AeroACARS
(UDP 49000, RREF) und zeigt, welche X-Plane wirklich schickt.

Anlass (27.09.2026): Bordbuch meldete bei SIA 375 (FF 777, X-Plane) Strobes
„diesmal ohne", obwohl sie an waren. Frage: streamt X-Plane Datarefs, die
das geladene Flugzeug gar nicht hat (z. B. ToLiss-/A330-Schalter)?

    python3 tools/xplane-rref-sonde.py            # 20 s messen
    python3 tools/xplane-rref-sonde.py --sek 120  # länger, Schalter bedienen
"""
import argparse
import socket
import struct
import time

DATAREFS = [
    # Standard (sollten immer kommen)
    "sim/cockpit2/switches/strobe_lights_on",
    "sim/cockpit2/switches/beacon_on",
    "sim/cockpit/electrical/beacon_lights_on",
    "sim/cockpit2/switches/navigation_lights_on",
    "sim/cockpit2/switches/landing_lights_on",
    "sim/cockpit2/switches/taxi_light_on",
    "sim/cockpit2/radios/actuators/transponder_mode",
    "sim/cockpit2/radios/actuators/transponder_code",
    "sim/cockpit2/controls/flap_handle_request_ratio",
    "sim/cockpit2/controls/speedbrake_ratio",
    "sim/cockpit2/switches/fasten_seat_belts",
    # Add-on-Quellen, die AeroACARS abonniert (dürfen bei Standard-
    # flugzeugen NICHT kommen, sonst stimmt die Abwesenheitsregel nicht)
    "laminar/a333/switches/strobe_pos",
    "laminar/A333/switches/fasten_seatbelts",
    "AirbusFBW/OHPLightSwitches[7]",
    "AirbusFBW/OHPLightSwitches[0]",
    "AirbusFBW/AP1Engage",
    "laminar/B738/knob/transponder_pos",
    "laminar/B738/toggle_switch/seatbelt_sign_pos",
    "laminar/B738/annunciator/speedbrake_armed",
    "B742/ext_light/beacon_sw",
    # Gibt es garantiert nirgends — Gegenprobe
    "aeroacars/gibt/es/nicht",
]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=49000)
    ap.add_argument("--sek", type=float, default=20)
    a = ap.parse_args()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", 0))
    sock.settimeout(0.5)
    ziel = (a.host, a.port)

    def abo(idx: int, name: str, freq: int) -> None:
        name_b = name.encode()[:399].ljust(400, b"\0")
        sock.sendto(b"RREF\0" + struct.pack("<ii", freq, idx) + name_b, ziel)

    for i, n in enumerate(DATAREFS):
        abo(i, n, 5)

    werte: dict[int, list[float]] = {}
    ende = time.time() + a.sek
    letzte_ausgabe = 0.0
    try:
        while time.time() < ende:
            try:
                daten, _ = sock.recvfrom(8192)
            except socket.timeout:
                continue
            if not daten.startswith(b"RREF"):
                continue
            nutz = daten[5:]
            for off in range(0, len(nutz) - 7, 8):
                idx, wert = struct.unpack("<if", nutz[off : off + 8])
                verlauf = werte.setdefault(idx, [])
                if not verlauf or verlauf[-1] != wert:
                    verlauf.append(wert)
            if time.time() - letzte_ausgabe > 2:
                letzte_ausgabe = time.time()
                print(f"… {len(werte)}/{len(DATAREFS)} Datarefs liefern Werte")
    finally:
        for i, n in enumerate(DATAREFS):
            abo(i, n, 0)  # abbestellen

    print("\n=== Ergebnis ===")
    for i, n in enumerate(DATAREFS):
        v = werte.get(i)
        zeile = "—  kommt NICHT" if v is None else "→ " + " → ".join(f"{x:g}" for x in v[:12])
        print(f"{n:52s} {zeile}")


if __name__ == "__main__":
    main()
