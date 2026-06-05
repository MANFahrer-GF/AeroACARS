// Flugzeug-Marker-Icons nach ICAO-Kategorie — 1:1 portiert aus der VPS-Live-Map
// (aeroacars-live `src/data/icaoCategory.ts`), damit die In-App-Karte denselben
// Look hat wie live.kant.ovh / Stratos.
//
// Kein 1:1-Realismus — 5 Silhouetten (Heavy / Medium / Light / Turboprop / Heli).
// Unbekannte Codes → "medium". Quelle der Heuristik: ICAO Doc 8643.

export type AircraftCategory = "heavy" | "medium" | "light" | "turboprop" | "heli";

const HEAVY_ICAOS = new Set([
  "A332", "A333", "A338", "A339",
  "A342", "A343", "A345", "A346",
  "A359", "A35K",
  "A388",
  "B741", "B742", "B743", "B744", "B748", "B74S", "B74F", "B74R",
  "B752", "B753", "B762", "B763", "B764", "B772", "B773", "B77L", "B77W", "B778", "B779",
  "B788", "B789", "B78X",
  "MD11", "DC10", "L101", "IL96", "A124",
]);

const TURBOPROP_ICAOS = new Set([
  "AT43", "AT45", "AT46", "AT72", "AT75", "AT76",
  "DH8A", "DH8B", "DH8C", "DH8D",
  "JS31", "JS32", "JS41",
  "SF34", "SF50", "SB20",
  "BE20", "BE9L", "BE99", "BE10", "BE40",
  "C208", "C20T", "C30J", "C295", "C212",
  "DHC6", "DHC7",
  "PC12", "PC6T",
  "TBM7", "TBM8", "TBM9", "TBM10",
  "AN24", "AN26", "AN30", "AN32",
  "L410",
  "DA42", "DA62",
]);

const HELI_ICAOS = new Set([
  "EC20", "EC30", "EC35", "EC45", "EC55", "EC75",
  "AS50", "AS55", "AS65", "AS32", "AS3B",
  "B06", "B06T", "B212", "B412", "B429", "B407", "B505",
  "R22", "R44", "R66",
  "S61", "S70", "S76", "S92",
  "H125", "H135", "H145", "H155", "H160", "H175", "H215", "H225",
  "MI8", "MI17", "MI24", "MI26", "MI28",
  "CH47", "CH53",
  "UH60", "UH72",
  "A109", "A119", "A139", "A149", "A169", "A189",
]);

const LIGHT_ICAOS = new Set([
  "C150", "C152", "C162", "C172", "C175", "C177", "C180", "C182", "C185", "C188",
  "C205", "C206", "C207", "C210", "C337", "C310", "C320", "C340", "C402", "C404",
  "C414", "C421", "C425", "C441",
  "P28A", "P28R", "P28T", "P32R", "P32T", "P46T", "PA46",
  "BE33", "BE35", "BE36", "BE55", "BE58", "BE60",
  "BE76", "BE77", "BE19", "BE23", "BE24",
  "DA20", "DA40", "M20P", "M20T", "M20J", "M20K", "M20R",
  "GLAS", "SR20", "SR22", "SR2T",
  "DR40",
  "TB10", "TB20", "TB21",
  "PA22", "PA23", "PA24", "PA25", "PA28", "PA30", "PA31", "PA32", "PA34", "PA38", "PA42", "PA44",
  "GP4",
  "RV4", "RV6", "RV7", "RV8", "RV9", "RV10", "RV12", "RV14",
]);

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
