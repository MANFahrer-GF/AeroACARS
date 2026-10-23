// Tastenbilder als SVG (144 × 144, die Auflösung der hochauflösenden
// Tasten). Linien-Icons im 24er-Raster, wie auf der Konzeptseite.

export const ICONS = {
  plane: '<path fill="currentColor" stroke="none" d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z"/>',
  tag: '<path d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="7.5" r="1.5"/>',
  gate: '<path d="M6 21V3h12v18M3 21h18M14 12h.5"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  watch: '<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2 2M9 2h6M12 2v3"/>',
  fuel: '<path d="M4 21V5a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v16M3 21h12M7 8h4M14 9h2a2 2 0 0 1 2 2v6a1.5 1.5 0 0 0 3 0V8l-3-3"/>',
  weight: '<path d="M6 9h12l2 12H4z"/><circle cx="12" cy="6" r="2.5"/>',
  wind: '<path d="M3 8h11a3 3 0 1 0-3-3M3 12h16a3 3 0 1 1-3 3M3 16h8"/>',
  runway: '<path d="M8 21l2-18M16 21l-2-18M12 5v2M12 10v2M12 15v2M12 20v1"/>',
  divert: '<path d="M6 21V11a5 5 0 0 1 5-5h8M16 3l3 3-3 3"/>',
  land: '<path d="M12 3v11M7 9l5 5 5-5M4 20h16"/>',
  ga: '<path d="M5 19v-6a6 6 0 0 1 6-6h8M16 4l3 3-3 3"/>',
  ok: '<circle cx="12" cy="12" r="9"/><path d="M8 12.5l3 3 5-6"/>',
  warn: '<path d="M12 3l9.5 17h-19z"/><path d="M12 10v4.5M12 17.2v.3"/>',
  pause: '<circle cx="12" cy="12" r="9"/><path d="M10 9v6M14 9v6"/>',
  signal: '<path d="M5 20v-3M10 20v-7M15 20V9M20 20V4"/>',
  msg: '<path d="M4 5h16v11H9l-5 4z"/><path d="M8 9h8M8 12h5"/>',
  spk: '<path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M16.5 9a4 4 0 0 1 0 6M19 6.5a7.5 7.5 0 0 1 0 11"/>',
  mute: '<path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>',
  down: '<path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M16 12h5"/>',
  up: '<path d="M4 10h4l5-4v12l-5-4H4z"/><path d="M16 12h5M18.5 9.5v5"/>',
  music: '<path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="17" cy="16" r="3"/>',
  musicOff: '<path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="17" cy="16" r="3"/><path d="M3 3l18 18"/>',
  back: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  gauge: '<path d="M4 17a8 8 0 1 1 16 0"/><path d="M12 17l4-5"/><circle cx="12" cy="17" r="1"/>',
  route: '<circle cx="6" cy="18" r="2"/><circle cx="18" cy="6" r="2"/><path d="M8 18h7a3 3 0 0 0 0-6H9a3 3 0 0 1 0-6h7"/>',
  mega: '<path d="M4 10v4h3l8 4V6L7 10z"/><path d="M18 9.5a3.5 3.5 0 0 1 0 5M7 14l1 5h2.5l-1-4.5"/>',
  belt: '<path d="M5 4v8a7 7 0 0 0 14 0V4"/><path d="M9 13h6v4H9z"/>',
  medal: '<circle cx="12" cy="15" r="5"/><path d="M8.5 11.2L6 3h4l2 5 2-5h4l-2.5 8.2"/><path d="M12 13v4"/>',
  win: '<path d="M4 5h16v14H4zM4 9h16"/><path d="M13 13h4v4M17 13l-5 5"/>',
};

// Grund der Taste und Farbe von Icon/Balken je Ton.
const TOENE = {
  aus: ["#1a1d22", "#5d6772"],
  neutral: ["#20252c", "#b9c4cf"],
  ok: ["#17402b", "#6fe0a0"],
  warn: ["#4a3712", "#f2c062"],
  crit: ["#4d1a16", "#ff8a7a"],
  ctl: ["#1b2f4d", "#8fbaff"],
};
// Bahnschrift (Windows 10+) zuerst, Arial Narrow auf dem Mac. Fehlt beides,
// nimmt die Software Arial — deshalb rechnet `passend` mit Arials Breiten.
// Feldbefund 23.10.: mit „Arial Narrow“ allein liefen Texte auf Windows rechts
// aus der Taste („Reiseflu…“, „19:09…“).
const SCHRIFT = "Bahnschrift SemiBold Condensed, Bahnschrift Condensed, Bahnschrift, Arial Narrow, Arial, sans-serif";

// Ungefähre Zeichenbreiten von Arial Bold in em — für eine Schätzung, die
// eher zu breit als zu schmal liegt.
function breiteEm(text) {
  let em = 0;
  for (const z of String(text)) {
    if (/[0-9]/.test(z)) em += 0.56;
    else if (/[A-ZÄÖÜ]/.test(z)) em += z === "I" ? 0.28 : /[MW]/.test(z) ? 0.94 : 0.72;
    else if (/[a-zäöüß]/.test(z)) em += /[il]/.test(z) ? 0.28 : /[mw]/.test(z) ? 0.89 : /[fjrt]/.test(z) ? 0.39 : 0.61;
    else if (z === " ") em += 0.28;
    else if (/[.,:;'!|]/.test(z)) em += 0.33;
    else if (/[—%✓✗]/.test(z)) em += 0.95;
    else em += 0.6;
  }
  return em;
}

/** Größte Schriftgröße ≤ `max`, mit der `text` in `platz` Pixel passt. */
export function passend(text, max, platz, extraJeZeichen = 0) {
  const n = [...String(text ?? "")].length;
  if (!n) return max;
  const g = Math.floor((platz - extraJeZeichen * n) / breiteEm(text));
  return Math.max(10, Math.min(max, g));
}

const esc = (t) => String(t ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** Icon an Stelle x/y in Größe g (Pixel), in Farbe f. */
export function icon(name, x, y, g, f) {
  const inhalt = ICONS[name] ?? ICONS.ok;
  const k = g / 24;
  return `<g transform="translate(${x} ${y}) scale(${k})" color="${f}" fill="none" stroke="${f}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${inhalt}</g>`;
}

/** Ein Tastenbild als SVG-Text. */
export function tasteSvg(b) {
  const [grund, farbe] = TOENE[b.ton] ?? TOENE.neutral;
  const hell = b.ton === "aus" ? "#7c8792" : "#e9edf2";
  const teile = [
    `<rect width="144" height="144" rx="18" fill="${grund}"/>`,
    `<rect width="144" height="72" rx="18" fill="#ffffff" opacity="0.05"/>`,
    icon(b.icon, 12, 12, 30, farbe),
    `<text x="50" y="34" font-family="${SCHRIFT}" font-size="${passend(String(b.label ?? "").toUpperCase(), 19, 84, 1)}" font-weight="700" fill="#8a96a3" letter-spacing="1">${esc(String(b.label ?? "").toUpperCase())}</text>`,
  ];
  if (b.zurueck) teile.push(icon("back", 112, 112, 22, "#c7d0d9"));
  if (b.gross) {
    teile.push(icon(b.gross, 44, 50, 56, farbe));
  } else {
    // Rand der Taste: die Blende des Geräts schneidet ein paar Pixel ab.
    const groesse = passend(b.wert, b.klein || String(b.wert ?? "").length > 6 ? 30 : 40, 118);
    teile.push(`<text x="12" y="${groesse > 30 ? 92 : 88}" font-family="${SCHRIFT}" font-size="${groesse}" font-weight="700" fill="${hell}">${esc(b.wert)}</text>`);
  }
  if (b.balken != null) {
    const w = Math.round(120 * Math.min(1, Math.max(0, b.balken)));
    teile.push(`<rect x="12" y="116" width="120" height="8" rx="4" fill="#ffffff" opacity="0.12"/>`, `<rect x="12" y="116" width="${w}" height="8" rx="4" fill="${farbe}"/>`);
  } else if (b.stufen != null) {
    for (let i = 0; i < 6; i++) {
      const f = i === b.stufen ? farbe : i < b.stufen ? farbe : "#ffffff";
      const o = i === b.stufen ? 1 : i < b.stufen ? 0.45 : 0.12;
      teile.push(`<rect x="${12 + i * 15}" y="118" width="12" height="7" rx="3" fill="${f}" opacity="${o}"/>`);
    }
  } else if (b.unter) {
    teile.push(`<text x="12" y="126" font-family="${SCHRIFT}" font-size="${passend(b.unter, 18, 118)}" fill="#8a96a3">${esc(b.unter)}</text>`);
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">${teile.join("")}</svg>`;
}

/**
 * Ein Feld des Touch-Streifens am StreamDeck + (200 × 100 je Regler), im
 * Stil der Tasten. Ersetzt Elgatos `$B1`: grauer Grund, Werte mit „…“
 * abgeschnitten, keine eigenen Farben (Feldbefund 23.10.).
 */
export function reglerSvg(b) {
  const [grund, farbe] = TOENE[b.ton] ?? TOENE.neutral;
  const hell = b.ton === "aus" ? "#7c8792" : "#e9edf2";
  const label = String(b.label ?? "").toUpperCase();
  const teile = [
    `<rect width="200" height="100" rx="12" fill="${grund}"/>`,
    `<rect width="200" height="46" rx="12" fill="#ffffff" opacity="0.05"/>`,
    icon(b.icon, 10, 9, 22, farbe),
    `<text x="40" y="27" font-family="${SCHRIFT}" font-size="${passend(label, 15, 150, 1)}" font-weight="700" fill="#8a96a3" letter-spacing="1">${esc(label)}</text>`,
  ];
  teile.push(`<text x="10" y="64" font-family="${SCHRIFT}" font-size="${passend(b.wert, 32, 180)}" font-weight="700" fill="${hell}">${esc(b.wert)}</text>`);
  if (b.balken != null) {
    const w = Math.round(180 * Math.min(1, Math.max(0, b.balken)));
    teile.push(`<rect x="10" y="80" width="180" height="8" rx="4" fill="#ffffff" opacity="0.12"/>`, `<rect x="10" y="80" width="${w}" height="8" rx="4" fill="${farbe}"/>`);
  } else if (b.unter) {
    teile.push(`<text x="10" y="90" font-family="${SCHRIFT}" font-size="${passend(b.unter, 15, 180)}" fill="#8a96a3">${esc(b.unter)}</text>`);
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">${teile.join("")}</svg>`;
}

export const alsBase64 = (svg) => `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;

export const alsDatenUrl = (svg) => `data:image/svg+xml;charset=utf8,${encodeURIComponent(svg)}`;

/** Icon für die Aktionsliste der StreamDeck-Software (weiß auf transparent). */
export function listenIconSvg(name, groesse = 40) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${groesse}" height="${groesse}" viewBox="0 0 ${groesse} ${groesse}">${icon(name, groesse * 0.1, groesse * 0.1, groesse * 0.8, "#ffffff")}</svg>`;
}
