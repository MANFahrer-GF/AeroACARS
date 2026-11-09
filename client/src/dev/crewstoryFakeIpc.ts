// Attrappe der IPC-Schicht fuer die Vorschau des Crew-Story-Fensters.
// `?fall=` normal (200), leer (keine Story), gesperrt (Flug laeuft, nie geprueft), sperre423
// (beim Speichern laeuft der Flug schon), konflikt (erstes Speichern 409), aus (403), kaputt
// (Netzfehler), altclient (Programm ohne modul-api-1), altohneofp (dazu ohne OFP).
export const isTauri = true;
export const formatIpcError = (e: unknown) => String(e);
const w = window as unknown as { __fakeLog: unknown[]; __geoeffnet: string[] };
w.__fakeLog = [];
w.__geoeffnet = [];
export async function openExternal(url: string) { w.__geoeffnet.push(url); }
export async function listen() { return () => undefined; }

const q = new URLSearchParams(window.location.search);
const fall = q.get("fall") ?? "normal";
const HASH = (s: string) => (s.length.toString(16).padStart(8, "0") + "a".repeat(32)).slice(0, 40);
let serverText = fall === "leer" ? "" : "Hochzeitsgesellschaft an Bord, die Braut sitzt vorne rechts.";
let konflikte = fall === "konflikt" ? 1 : 0;

const flight = (id: string, nr: string, von: string, nach: string, ofp: boolean) => ({
  id, flight_number: nr, route_code: null, route_leg: null, callsign: null, dpt_airport_id: von, arr_airport_id: nach, alt_airport_id: null,
  flight_time: 75, dpt_time: "10:15", arr_time: "11:30", level: 360, route: "SOBRA Y163 NATOR", flight_type: "J", distance: { mi: null, km: null, nmi: 330 },
  airline: { id: 1, icao: "GSG", iata: "GS", name: "German Sky Group", logo: null }, dpt_airport: null, arr_airport: null,
  simbrief: ofp ? { id: `sb${id}`, url: null, aircraft_id: 7, subfleet: null } : null,
});
const BIDS = [
  { id: 11, user_id: 1, flight_id: "F11", flight: flight("F11", "421", "EDDF", "LSZH", fall !== "altohneofp") },
  { id: 12, user_id: 1, flight_id: "F12", flight: flight("F12", "422", "LSZH", "EDDF", true) },
];

function stand(extra: object = {}) {
  const gesperrt = fall === "gesperrt";
  return { text: serverText, hash: serverText ? HASH(serverText) : null, pruefung: serverText && !gesperrt ? { status: "teilweise", gruende: ["Die Uhrzeit fällt weg, sie kommt aus dem Flugplan."] } : null, ansagen_aus: false, gesperrt, max: 500, ...extra };
}

function modulApi(a: { methode: string; pfad: string; koerper?: { text?: string } }) {
  w.__fakeLog.push(a);
  if (fall === "kaputt") throw { code: "modul_api_netz", message: "not reachable" };
  if (fall === "aus") return { status: 403, daten: { fehler: "nicht_freigeschaltet" } };
  if (a.methode === "GET") return { status: 200, daten: stand() };
  if (a.pfad.endsWith("/pruefen")) return { status: 200, daten: { status: "uebernommen", gruende: [] } };
  if (fall === "sperre423") return { status: 423, daten: { fehler: "gesperrt", ...stand({ gesperrt: true }) } };
  if (konflikte > 0) {
    konflikte--;
    serverText = "Letzter Flug unserer Purserin vor der Rente.";
    return { status: 409, daten: { fehler: "geaendert", ...stand() } };
  }
  serverText = a.koerper?.text ?? "";
  return { status: 200, daten: { text: serverText, hash: serverText ? HASH(serverText) : null } };
}

export async function invoke<T>(cmd: string, a?: unknown): Promise<T> {
  await new Promise((r) => setTimeout(r, 150));
  const antwort = (): unknown => {
    switch (cmd) {
      case "phpvms_get_bids": return BIDS;
      case "paket_info": return { faehigkeiten: fall.startsWith("alt") ? ["bilder-1"] : ["bilder-1", "modul-api-1"] };
      case "modul_api": return modulApi(a as { methode: string; pfad: string });
      default: return null;
    }
  };
  return antwort() as T;
}
