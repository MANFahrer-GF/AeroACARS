// Ersatz-IPC nur für die Landungs-Vorschau (landung.html): liefert das
// Beispiel-Bordbuch, damit der Abschnitt ohne laufenden Client sichtbar wird.
import { BEISPIEL_BORDBUCH } from "./mockBordbuch";

let eintrag = BEISPIEL_BORDBUCH;

export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Wiederaufnahme-Vorschau: `?sim=1` = Simulator verbunden (an WSSS).
  if (cmd === "sim_status") {
    const sim = new URLSearchParams(window.location.search).has("sim");
    return { state: "connected", kind: "xplane12", available: true, last_error: null,
      snapshot: sim ? { lat: 1.3502, lon: 103.9840, altitude_msl_ft: 139, heading_deg_true: 114, fuel_total_kg: 90594, zfw_kg: 224885, total_weight_kg: 315480, aircraft_icao: "B77W" } : null } as T;
  }
  if (cmd === "bordbuch_eintrag") return eintrag as T;
  if (cmd === "bordbuch_markieren") {
    const { regel, nachAtc } = args as { regel: string; nachAtc: boolean };
    eintrag = {
      ...eintrag,
      punkte: eintrag.punkte.map((p) =>
        p.regel === regel ? { ...p, status: nachAtc ? "nach_atc" : p.auto_status } : p,
      ),
    };
    return eintrag as T;
  }
  throw new Error(`Vorschau: ${cmd} nicht verfügbar`);
}

export async function listen(): Promise<() => void> {
  return () => undefined;
}
