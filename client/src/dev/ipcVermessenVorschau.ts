// Ersatz-IPC für die Vorschau „Flugzeug vermessen" (vermessen.html):
// spielt einen FF-777-Durchlauf nach, damit die Führung ohne Simulator
// sichtbar ist. `?leer` = Schalter meldet nichts, `?luft` = in der Luft.
const q = new URLSearchParams(window.location.search);
let stellungenImSchritt = 0;
const warte = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function invoke<T = unknown>(cmd: string, _args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "sim_status":
      return {
        // ?msfs = iniBuilds-A380 in MSFS 2024 statt 777 in X-Plane.
        state: "connected", kind: q.has("msfs") ? "msfs2024" : "xplane12", available: true, last_error: null,
        snapshot: q.has("msfs")
          ? { on_ground: !q.has("luft"), aircraft_title: "A380-800 RR Basic", aircraft_icao: "A388" }
          : { on_ground: !q.has("luft"), aircraft_title: "Boeing 777-300ER", aircraft_icao: "B77W" },
      } as T;
    case "vermessung_starten":
      await warte(1500);
      return { sim: "xplane", flugzeug: { titel: "Boeing 777-300ER", icao: "B77W", autor: "FlightFactor" }, anzahl_werte: 9312, l_namen: 0, sitzung: 1 } as T;
    case "vermessung_ruhe":
      await warte(8000);
      return { rauschen: 214 } as T;
    case "vermessung_stellung": {
      await warte(900);
      const erste = stellungenImSchritt === 0;
      stellungenImSchritt++;
      return { erste, mitgegangen: erste || q.has("leer") ? 0 : 3 } as T;
    }
    case "vermessung_schritt_abschliessen":
      stellungenImSchritt = 0;
      return (q.has("leer")
        ? { kandidaten: 0, beispiele: [] }
        : { kandidaten: 4, beispiele: [
            { variable: "1-sim/ckpt/lights/strobe", werte: [0, 1, 2] },
            { variable: "1-sim/lights/strobe/anim", werte: [0, 0, 1] },
            { variable: "sim/cockpit2/switches/strobe_lights_on", werte: [0, 0, 1] },
          ] }) as T;
    case "vermessung_schritt_neu":
      stellungenImSchritt = 0;
      return undefined as T;
    case "vermessung_scan_namen":
      // ?scan = für das geladene Flugzeug gibt es einen Scan.
      return (q.has("scan") ? 235 : 0) as T;
    case "vermessung_liste": {
      // ?neu = geladenes Flugzeug ist noch nicht vermessen.
      const tag = (d: number) => Date.UTC(2026, 8, d);
      const liste = [
        { sim: "msfs", teil: "boden", icao: "A388", titel: "A380-800 RR Basic", zuletzt: tag(28), anzahl: 2, scan_namen: 235 },
        { sim: "xplane", teil: "luft", icao: "A333", titel: "Airbus A330-300", zuletzt: tag(27), anzahl: 1 },
        { sim: "xplane", icao: "A333", titel: "Airbus A330-300", zuletzt: tag(27), anzahl: 1 },
        { sim: "msfs", icao: "A20N", titel: "FenixA320 IAE", zuletzt: tag(24), anzahl: 1, scan_namen: 0 },
      ];
      if (!q.has("neu")) liste.unshift({ sim: "xplane", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: tag(28), anzahl: 1 });
      return (q.has("neu") && q.has("msfs") ? liste.filter((v) => v.icao !== "A388") : liste) as T;
    }
    case "vermessung_senden":
      await warte(1200);
      return { id: "c12851fa8f8ef3b7d9493cb723d337b2" } as T;
    default:
      return undefined as T;
  }
}

export async function listen(): Promise<() => void> {
  return () => undefined;
}
