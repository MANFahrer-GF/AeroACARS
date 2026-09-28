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
    case "vermessung_profile": {
      // grob wie AircraftProfile::detect im Client
      const f = ((_args?.flugzeuge ?? []) as Array<{ titel: string[] }>);
      const erkenne = (t: string) =>
        /fenix/i.test(t) ? "FenixA320" : /flybywire|a32nx|a380x/i.test(t) ? "FbwA32nx" : /^A350|A380-800|A330/i.test(t) ? "IniA350"
        : /synaptic|A220-300/i.test(t) ? "SynapticA220" : /ifly/i.test(t) ? "IFly737Max" : /falcon 50/i.test(t) ? "ContrailFa50" : null;
      return f.map((x) => x.titel.map(erkenne).find(Boolean) ?? null) as T;
    }
    case "vermessung_liste": {
      // Stand wie auf live.kant.ovh am 28.09.2026 (zwei A380-Messungen,
      // die Aircraft-Scans der VA). ?neu = A380 noch nicht vermessen.
      const tag = (d: number) => Date.UTC(2026, 8, d);
      const flugzeuge = q.has("neu") ? [] : [
        { sim: "msfs", teil: "boden", icao: "A388", titel: "A380-800 RR Basic", zuletzt: tag(28), anzahl: 2, scan_namen: 1588, profil: "geprueft" },
        { sim: "msfs", teil: "boden", icao: "A35K", titel: "A350-1000 (No Cabin)", zuletzt: tag(28), anzahl: 3, scan_namen: 1500, profil: "geprueft" },
        { sim: "msfs", teil: "luft", icao: "A35K", titel: "A350-1000 (No Cabin)", zuletzt: tag(28), anzahl: 1, scan_namen: 1500, profil: "geprueft" },
        { sim: "msfs", teil: "boden", icao: "A20N", titel: "Airbus A320neo FlyByWire", zuletzt: tag(28), anzahl: 1, scan_namen: 896, profil: "geprueft" },
      ];
      const scans = [
        { sim: "msfs", icao: "BCS3", paket: "Synaptic A220", titel_liste: ["Synaptic Simulations A220-300", "A220-300"], scan_namen: 156, profil: "in_arbeit", zuletzt: tag(28) },
        { sim: "msfs", icao: "A388", paket: "iniBuilds A380 – L:-Namen aus AAO-Profil", titel_liste: ["A380-800 RR Basic"], scan_namen: 235, profil: null, zuletzt: tag(28) },
        { sim: "msfs", icao: "A35K", paket: "A350 Airliner", titel_liste: ["A350-1000 (Default Cabin)"], scan_namen: 1500, profil: null, zuletzt: tag(14) },
        { sim: "msfs", icao: "A388", paket: "A380X (Development)", titel_liste: ["FlyByWire A380X (A380-842)"], scan_namen: 1017, profil: null, zuletzt: tag(14) },
        { sim: "msfs", icao: "A20N", paket: "A32NX (Development)", titel_liste: ["Airbus A320 Neo FlyByWire"], scan_namen: 896, profil: null, zuletzt: tag(14) },
        { sim: "msfs", icao: "B38M", paket: "737MAX", titel_liste: ["iFly 737-MAX8 (166Seats)"], scan_namen: 1500, profil: null, zuletzt: tag(14) },
        { sim: "msfs", icao: "BE24", paket: "Sierra-C24R", titel_liste: ["Flysimware Sierra C24R G3X GNS530 C-GMTT"], scan_namen: 299, profil: "aus_scan", zuletzt: tag(6) },
        { sim: "msfs", icao: "A320", paket: "Fenix A320 – L:-Namen aus HubHop", titel_liste: ["FenixA320", "FenixA320 CFM SL"], scan_namen: 1921, profil: null, zuletzt: tag(28) },
        { sim: "msfs", icao: "A20N", paket: "Asobo A320neo", titel_liste: ["Asobo A320 Neo"], scan_namen: 0, profil: null, zuletzt: tag(3) },
        { sim: "xplane", icao: "A20N", paket: "ToLiSs A320 Hi Def", titel_liste: ["ToLiSs A320 Hi Def"], scan_namen: null, profil: "aus_scan", zuletzt: tag(5) },
        { sim: "xplane", icao: "B738", paket: "Boeing 737-800", titel_liste: ["Boeing 737-800"], scan_namen: null, profil: "aus_scan", zuletzt: tag(5) },
        { sim: "xplane", icao: "B738", paket: "Boeing 737-800X", titel_liste: ["Boeing 737-800X"], scan_namen: null, profil: "aus_scan", zuletzt: tag(5) },
        { sim: "msfs", icao: "FA50", paket: "Dassault Falcon 50", titel_liste: ["Contrail Falcon 50"], scan_namen: 697, profil: "aus_scan", zuletzt: tag(5) },
      ];
      return { flugzeuge, scans } as T;
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
