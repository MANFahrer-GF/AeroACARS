// Gemeinsame Bausteine fuer die Tests des Bilder-Tabs.
import type { BildMeta, Eintrag, Fund, Sidecar } from "./typen";

export function meta(m: Partial<BildMeta> = {}): BildMeta {
  return {
    fassung: 1, zeit_utc: "2026-11-07T10:00:00.000Z", breite: 50.03, laenge: 8.57, hoehe_ft: 12000, gs_kt: 280, kurs: 270,
    flugzeug_titel: "Airbus A320", flugzeug_icao: "A320", phase: "cruise", pirep_id: "P1", rufzeichen: "GSG101",
    abflug: "EDDF", ziel: "LEPA", simulator: "msfs", naechster_flugplatz: "EDDF", quelle: "sim", ...m,
  };
}

let zaehler = 0;
export function eintrag(o: { id?: string; meta?: Partial<BildMeta>; sidecar?: Partial<Sidecar>; bytes?: number; fertig?: boolean } = {}): Eintrag {
  const id = o.id ?? `b${++zaehler}`;
  return {
    id, gruppe: "g", pfad: `g/${id}.jpg`, vorschau_pfad: `g/${id}.vorschau.jpg`, quelle_pfad: null,
    groesse: o.bytes ?? 1000, vorschau_groesse: 100, quelle_groesse: 0, belegt_bytes: o.bytes ?? 1000,
    dateizeit_ms: Date.parse((o.meta?.zeit_utc ?? "2026-11-07T10:00:00.000Z")), sidecar_fehler: null, vollstaendig: o.fertig ?? true,
    sidecar: { fassung: 1, bild_id: id, text: "", favorit: false, meta: meta(o.meta), sendungen: [], ...o.sidecar },
  };
}

export function fund(f: Partial<Fund> = {}): Fund {
  return {
    fund_id: "f1", pfad: "/x/a.png", dateizeit_ms: 1, groesse: 10, format: "png", uebernehmbar: true, px_breite: 2560, px_hoehe: 1440,
    quelle: "/x", art: "standard", sim_vordergrund: null, sim_fenster: null, basis: {}, ...f,
  };
}
