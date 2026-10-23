// Einzeltasten (für eigene Seiten) im selben Glascockpit-Stil wie die Szene:
// dunkle Fläche, Marke in Großbuchstaben, großer Wert in B612, Farben nach
// Cockpit-Regel. Thomas 23.10.: „das alte Design auch bei denen ersetzen,
// die man sich selber zusammenbauen kann“.
//
// Eingabe ist dasselbe Tastenbild wie bisher (tasten.js):
//   { label, wert, unter, ton, icon, balken?, stufen?, gross?, zurueck?, ansichten?, ansicht? }

import { FARBE, text, marke, flaeche, balken, LEUCHTEN } from "./glas.js";

const K = 144;
const INNEN = K - 24;

/** Farbe des Werts je Ton. */
const WERTFARBE = { neutral: FARBE.text, ok: FARBE.gruen, warn: FARBE.amber, crit: FARBE.rot, ctl: FARBE.cyan, aus: FARBE.leise };

export function einzelSvg(b) {
  const ton = b.ton ?? "neutral";
  const farbe = WERTFARBE[ton] ?? FARBE.text;
  const bedienung = ton === "ctl";
  const teile = [
    `<rect width="${K}" height="${K}" fill="${FARBE.grund}"/>`,
    flaeche(0, 0, K, K, bedienung ? "#0d1622" : FARBE.flaeche, 18),
  ];
  if (bedienung) teile.push(`<rect x="1" y="1" width="${K - 2}" height="${K - 2}" rx="17" fill="none" stroke="${FARBE.cyan}" stroke-opacity="0.45" stroke-width="2"/>`);
  else if (ton === "warn" || ton === "crit") teile.push(`<rect x="1" y="1" width="${K - 2}" height="${K - 2}" rx="17" fill="none" stroke="${farbe}" stroke-opacity="0.55" stroke-width="2"/>`);

  teile.push(marke(14, 30, b.label ?? "", ton === "warn" || ton === "crit" ? farbe : FARBE.leise, "start", b.ansichten > 1 ? INNEN - 34 : INNEN));

  if (b.gross) {
    teile.push(text(K / 2, 108, b.gross === "down" ? "−" : "+", { groesse: 80, fett: true, farbe: FARBE.cyan, anker: "middle" }));
  } else {
    const wert = String(b.wert ?? "–");
    const g = b.klein || wert.length > 6 ? 32 : wert.length > 4 ? 40 : 46;
    teile.push(text(14, 88, wert, { groesse: g, fett: true, farbe, max: INNEN }));
  }

  if (b.balken != null) {
    teile.push(balken(14, 116, K - 28, b.balken, farbe === FARBE.text ? FARBE.cyan : farbe, 8));
  } else if (b.stufen != null) {
    for (let i = 0; i < 6; i++) {
      const an = i === b.stufen, vorbei = i < b.stufen;
      teile.push(`<rect x="${14 + i * 15}" y="${116}" width="11" height="7" rx="3" fill="${an || vorbei ? farbe : "#ffffff"}" opacity="${an ? 1 : vorbei ? 0.45 : 0.15}"/>`);
    }
  } else if (b.unter) {
    teile.push(text(14, 124, b.unter, { groesse: 16, farbe: FARBE.leise, max: INNEN }));
  }

  if (b.zurueck) teile.push(`<path d="M${K - 20} ${K - 34} l-12 10 12 10" stroke="${FARBE.cyan}" stroke-width="4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>`);

  // Mehrere Ansichten: Punkte oben rechts, der aktuelle hell.
  if (b.ansichten > 1) {
    for (let i = 0; i < b.ansichten; i++) {
      const x = K - 16 - (b.ansichten - 1 - i) * 11;
      teile.push(`<circle cx="${x}" cy="25" r="3.5" fill="${i === b.ansicht ? FARBE.cyan : "#ffffff"}" opacity="${i === b.ansicht ? 1 : 0.3}"/>`);
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${K}" height="${K}" viewBox="0 0 ${K} ${K}"><defs>${LEUCHTEN}</defs>${teile.join("")}</svg>`;
}
