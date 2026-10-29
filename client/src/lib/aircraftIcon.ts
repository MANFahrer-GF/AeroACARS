// Flugzeug-Marker-Icons nach ICAO-Kategorie — 1:1 portiert aus der VPS-Live-Map
// (aeroacars-live `src/data/icaoCategory.ts`), damit die In-App-Karte denselben
// Look hat wie live.kant.ovh / Stratos.
//
// Kein 1:1-Realismus — 5 Silhouetten (Heavy / Medium / Light / Turboprop / Heli).
// Unbekannte Codes → "medium". Quelle der Heuristik: ICAO Doc 8643.

import flotteEingebaut from "../../../bundle/tabellen/flotte.json";
import { invoke } from "./ipc";

export type AircraftCategory = "heavy" | "medium" | "light" | "turboprop" | "heli";

// Typlisten aus der Flottentabelle des Laufzeit-Pakets (P-3.53, eine Tabelle
// fuer Rust und Oberflaeche). Eingebaut ist die Datei aus `bundle/`; beim Start
// ersetzt `flotteAusPaketLaden()` sie durch den geltenden Stand.
type Kategorien = Record<"heavy" | "heli" | "turboprop" | "light", string[]>;

let HEAVY_ICAOS = new Set<string>();
let TURBOPROP_ICAOS = new Set<string>();
let HELI_ICAOS = new Set<string>();
let LIGHT_ICAOS = new Set<string>();

/** Uebernimmt `tabellen/flotte.json`; `false` (und nichts geaendert), wenn die
 *  Form nicht stimmt. */
export function flotteUebernehmen(datei: unknown): boolean {
  const k = (datei as { kategorien?: Partial<Kategorien> } | null)?.kategorien;
  const listen = ["heavy", "heli", "turboprop", "light"] as const;
  if (!k || !listen.every((n) => Array.isArray(k[n]) && k[n]!.every((c) => typeof c === "string"))) {
    return false;
  }
  HEAVY_ICAOS = new Set(k.heavy);
  HELI_ICAOS = new Set(k.heli);
  TURBOPROP_ICAOS = new Set(k.turboprop);
  LIGHT_ICAOS = new Set(k.light);
  return true;
}

flotteUebernehmen(flotteEingebaut);

/** Holt die Flottentabelle des geltenden Paketstands; bleibt sonst beim
 *  eingebauten Stand (z. B. Fernbedienung ohne Paketbefehl). */
export async function flotteAusPaketLaden(): Promise<void> {
  try {
    const text = await invoke<string>("paket_datei", { pfad: "tabellen/flotte.json" });
    flotteUebernehmen(JSON.parse(text));
  } catch {
    /* eingebauter Stand bleibt */
  }
}

export function icaoCategory(icao: string | null | undefined): AircraftCategory {
  if (!icao) return "medium";
  const code = icao.toUpperCase().trim();
  if (HEAVY_ICAOS.has(code)) return "heavy";
  if (HELI_ICAOS.has(code)) return "heli";
  if (TURBOPROP_ICAOS.has(code)) return "turboprop";
  if (LIGHT_ICAOS.has(code)) return "light";
  // alles andere (A320, B737, E190, CRJ, …) → medium
  return "medium";
}

// SVG-Pfad pro Kategorie. ViewBox -12..12 quadratisch. Nase zeigt nach oben (-Y).
const SVG_HEAVY = `<path d="M 0 -11 L 0.8 -3 L 11 1.5 L 11 3.5 L 0.8 2.5 L 0.8 7 L 4 8.8 L 4 9.8 L -4 9.8 L -4 8.8 L -0.8 7 L -0.8 2.5 L -11 3.5 L -11 1.5 L -0.8 -3 Z"
  stroke="rgba(0,0,0,0.85)" stroke-width="1.2" stroke-linejoin="round"/>`;

const SVG_MEDIUM = `<path d="M 0 -10 L 0.7 -2.5 L 9.5 2 L 9.5 3.5 L 0.7 2.2 L 0.7 6.5 L 3 8.2 L 3 9.2 L -3 9.2 L -3 8.2 L -0.7 6.5 L -0.7 2.2 L -9.5 3.5 L -9.5 2 L -0.7 -2.5 Z"
  stroke="rgba(0,0,0,0.85)" stroke-width="1.2" stroke-linejoin="round"/>`;

const SVG_LIGHT = `<path d="M 0 -8 L 0.5 -2 L 7.5 1 L 7.5 2.4 L 0.5 1.7 L 0.5 5 L 2.2 6.5 L 2.2 7.4 L -2.2 7.4 L -2.2 6.5 L -0.5 5 L -0.5 1.7 L -7.5 2.4 L -7.5 1 L -0.5 -2 Z"
  stroke="rgba(0,0,0,0.85)" stroke-width="1.0" stroke-linejoin="round"/>`;

const SVG_TURBOPROP = `<path d="M 0 -9 L 0.6 -3 L 8.5 1 L 8.5 2.5 L 0.6 1.7 L 0.6 6 L 2.6 7.5 L 2.6 8.4 L -2.6 8.4 L -2.6 7.5 L -0.6 6 L -0.6 1.7 L -8.5 2.5 L -8.5 1 L -0.6 -3 Z"
  stroke="rgba(0,0,0,0.85)" stroke-width="1.1" stroke-linejoin="round"/>
<circle cx="0" cy="-9.5" r="1.2" fill="rgba(0,0,0,0.5)"/>`;

const SVG_HELI = `<rect x="-2.5" y="-5" width="5" height="9" rx="2.5" stroke="rgba(0,0,0,0.85)" stroke-width="1"/>
<rect x="-0.6" y="3.5" width="1.2" height="6" stroke="rgba(0,0,0,0.85)" stroke-width="0.6"/>
<rect x="-1.6" y="8.5" width="3.2" height="1" rx="0.4" stroke="rgba(0,0,0,0.85)" stroke-width="0.6"/>
<line x1="-9" y1="-3" x2="9" y2="-3" stroke-width="1.4" stroke="currentColor" opacity="0.85"/>
<line x1="-7" y1="-7" x2="7" y2="1" stroke-width="1.2" stroke="currentColor" opacity="0.55"/>`;

const SVG_BY_CAT: Record<AircraftCategory, string> = {
  heavy: SVG_HEAVY,
  medium: SVG_MEDIUM,
  light: SVG_LIGHT,
  turboprop: SVG_TURBOPROP,
  heli: SVG_HELI,
};

export function aircraftSvg(icao: string | null | undefined): string {
  const cat = icaoCategory(icao);
  const inner = SVG_BY_CAT[cat];
  return `<svg viewBox="-12 -12 24 24" xmlns="http://www.w3.org/2000/svg" class="aircraft-svg" data-cat="${cat}">${inner}</svg>`;
}

export function categoryLabel(cat: AircraftCategory): string {
  switch (cat) {
    case "heavy": return "Heavy Jet";
    case "medium": return "Jet";
    case "light": return "Light";
    case "turboprop": return "Turboprop";
    case "heli": return "Helicopter";
  }
}
