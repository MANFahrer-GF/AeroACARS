// Glascockpit-Gestaltung für das Deck: Farben wie echte Airbus-/Boeing-
// Anzeigen, Schrift B612 (Airbus, OFL), Bausteine für SVG-Szenen.
//
// Farbregeln wie im Cockpit:
//   grün   = normal / aktiv / geflogen
//   cyan   = Auswahl, Ziel, bedienbar
//   magenta= Route, geplant
//   amber  = Aufmerksamkeit
//   rot    = Störung
//   weiß   = Messwert

export const FARBE = {
  grund: "#04070b",
  flaeche: "#0b1118",
  flaecheHell: "#121b25",
  linie: "#1d2a37",
  text: "#eef3f7",
  leise: "#7f93a6",
  gruen: "#3fe08a",
  cyan: "#45d4ff",
  magenta: "#ff4fd8",
  amber: "#ffb000",
  rot: "#ff4d4d",
};

export const SCHRIFT = "B612";

const esc = (t) => String(t ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** Text; `opt`: { groesse, farbe, fett, anker: start|middle|end, abstand } */
export function text(x, y, inhalt, opt = {}) {
  const { groesse = 20, farbe = FARBE.text, fett = false, anker = "start", abstand = 0, deckkraft = 1 } = opt;
  return `<text x="${x}" y="${y}" font-family="${SCHRIFT}" font-size="${groesse}" font-weight="${fett ? 700 : 400}" fill="${farbe}" text-anchor="${anker}" letter-spacing="${abstand}" opacity="${deckkraft}">${esc(inhalt)}</text>`;
}

/** Kleine Großbuchstaben-Beschriftung über einem Wert. */
export const marke = (x, y, inhalt, farbe = FARBE.leise, anker = "start") => text(x, y, String(inhalt).toUpperCase(), { groesse: 15, farbe, fett: true, anker, abstand: 1.5 });

/** Abgerundete Fläche als Kachelgrund. */
export const flaeche = (x, y, b, h, farbe = FARBE.flaeche, r = 16) => `<rect x="${x}" y="${y}" width="${b}" height="${h}" rx="${r}" fill="${farbe}"/>`;

/** Leuchten um ein Element (für das Flugzeug, aktive Punkte). */
export const LEUCHTEN = `<filter id="leuchten" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="4" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>`;

/** Flugzeug von oben, Spitze nach rechts, Mitte bei x/y. */
export function flugzeug(x, y, groesse, farbe = FARBE.text, winkel = 0) {
  const k = groesse / 24;
  return `<g transform="translate(${x} ${y}) rotate(${winkel}) scale(${k}) translate(-12 -12)" filter="url(#leuchten)"><path fill="${farbe}" d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z" transform="rotate(90 12 12)"/></g>`;
}

/** Waagerechter Füllbalken mit Teilstrichen. */
export function balken(x, y, b, anteil, farbe, h = 8) {
  const w = Math.round(b * Math.min(1, Math.max(0, anteil ?? 0)));
  return `<rect x="${x}" y="${y}" width="${b}" height="${h}" rx="${h / 2}" fill="#ffffff" opacity="0.10"/><rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${h / 2}" fill="${farbe}"/>`;
}

/** Bogen-Anzeige (wie ein Rundinstrument), Anteil 0..1 auf 240°. */
export function bogen(cx, cy, r, anteil, farbe, dicke = 8) {
  const a0 = 150, spanne = 240;
  const punkt = (grad) => [cx + r * Math.cos((grad * Math.PI) / 180), cy + r * Math.sin((grad * Math.PI) / 180)];
  const pfad = (von, bis) => {
    const [x0, y0] = punkt(von), [x1, y1] = punkt(bis);
    return `M${x0.toFixed(1)} ${y0.toFixed(1)} A${r} ${r} 0 ${bis - von > 180 ? 1 : 0} 1 ${x1.toFixed(1)} ${y1.toFixed(1)}`;
  };
  const ende = a0 + spanne * Math.min(1, Math.max(0, anteil ?? 0));
  return `<path d="${pfad(a0, a0 + spanne)}" stroke="#ffffff" stroke-opacity="0.12" stroke-width="${dicke}" fill="none" stroke-linecap="round"/>${anteil > 0.005 ? `<path d="${pfad(a0, ende)}" stroke="${farbe}" stroke-width="${dicke}" fill="none" stroke-linecap="round"/>` : ""}`;
}

/** Pfeil (für Wind, V/S), Spitze in Richtung `winkel` (0 = nach oben). */
export function pfeil(cx, cy, laenge, winkel, farbe, dicke = 5) {
  const h = laenge / 2;
  return `<g transform="translate(${cx} ${cy}) rotate(${winkel})"><line x1="0" y1="${h}" x2="0" y2="${-h + 8}" stroke="${farbe}" stroke-width="${dicke}" stroke-linecap="round"/><path d="M0 ${-h} L${dicke * 2} ${-h + 14} L${-dicke * 2} ${-h + 14} Z" fill="${farbe}"/></g>`;
}

/** Musiknote (B612 kennt ♪ nicht), Mitte bei x/y. */
export function note(x, y, groesse, farbe) {
  const k = groesse / 24;
  return `<g transform="translate(${x} ${y}) scale(${k}) translate(-12 -12)" fill="none" stroke="${farbe}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3" fill="${farbe}"/><circle cx="17" cy="16" r="3" fill="${farbe}"/></g>`;
}
