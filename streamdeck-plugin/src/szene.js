// Das ganze Deck als EINE Szene: eine SVG-Fläche über alle Tasten, die
// danach je Taste ausgeschnitten wird (viewBox). So laufen Streckenband,
// Laufschrift und große Anzeigen über mehrere Kacheln.
//
// Gliederung (XL, 8 × 4):
//   Reihe 0  Streckenband: Start ── ✈ ── Ziel, darunter Laufschrift
//   Reihe 1  Instrumente, je Phase andere Werte
//   Reihe 2  Phasen-Zone: baut sich je Phase um (Boarding / Reiseflug /
//            Anflug / nach der Landung)
//   Reihe 3  Bedienung, immer an derselben Stelle
//
// Zwischen den Tasten liegt die Blende des Geräts: Linien dürfen darüber
// laufen, Text nicht (er bleibt innerhalb einer Kachel). Die Laufschrift ist
// die einzige Ausnahme — sie wandert und ist deshalb trotzdem lesbar.

import { FARBE, text, marke, flaeche, LEUCHTEN, flugzeug, balken, bogen, pfeil, note } from "./glas.js";
import { ANSAGEN } from "./tasten.js";

export const KACHEL = 144;
export const FUGE = 36;

const kx = (c) => c * (KACHEL + FUGE);
const ky = (r) => r * (KACHEL + FUGE);
/** Breite über n Kacheln ab Spalte c (inklusive Fugen dazwischen). */
const spanne = (n) => n * KACHEL + (n - 1) * FUGE;

// ---- Daten ----------------------------------------------------------------

export function phasenGruppe(phase) {
  if (!phase) return "kein";
  if (["preflight", "boarding", "pushback", "taxi_out"].includes(phase)) return "vor";
  if (["takeoff_roll", "takeoff", "climb", "cruise", "holding"].includes(phase)) return "flug";
  if (["descent", "approach", "final", "landing"].includes(phase)) return "anflug";
  return "nach";
}

const PHASE_TEXT = {
  preflight: "VORBEREITUNG", boarding: "BOARDING", pushback: "PUSHBACK", taxi_out: "ROLLEN",
  takeoff_roll: "STARTLAUF", takeoff: "START", climb: "STEIGFLUG", cruise: "REISEFLUG", holding: "WARTESCHLEIFE",
  descent: "SINKFLUG", approach: "ANFLUG", final: "ENDANFLUG", landing: "LANDUNG",
  taxi_in: "ROLLEN", blocks_on: "AM STAND", arrived: "ANGEKOMMEN", pirep_submitted: "PIREP ABGEGEBEN",
};

const zahl = (v) => (v == null || !Number.isFinite(Number(v)) ? null : Number(v));
const zwei = (n) => String(n).padStart(2, "0");
const uhrZ = (ms) => (ms == null ? "--:--" : `${zwei(new Date(ms).getUTCHours())}:${zwei(new Date(ms).getUTCMinutes())}`);
const iso = (v) => (v ? Date.parse(v) : null);
const dauer = (abMs, bisMs) => {
  if (abMs == null || bisMs == null || bisMs < abMs) return "--:--";
  const m = Math.floor((bisMs - abMs) / 60000);
  return `${Math.floor(m / 60)}:${zwei(m % 60)}`;
};
const tonnen = (kg) => (kg == null ? "–" : (kg / 1000).toFixed(1).replace(".", ","));
const hoehe = (ft) => (ft == null ? "–" : ft >= 10000 ? `FL${zwei(Math.round(ft / 100)).padStart(3, "0")}` : `${Math.round(ft / 100) * 100}`);

/** Fortschritt 0..1 für das Flugzeug auf dem Band. */
export function fortschritt(st, jetzt) {
  const g = phasenGruppe(st?.phase);
  if (g === "vor" || g === "kein") return 0;
  if (g === "nach") return 1;
  const ab = iso(st.takeoff_at);
  const ete = zahl(st.live?.ete_min);
  if (ab != null && ete != null) {
    const vorbei = (jetzt - ab) / 60000;
    return Math.min(0.98, Math.max(0.02, vorbei / (vorbei + ete)));
  }
  // Ohne Restflugzeit: nach Phase schätzen.
  return { takeoff_roll: 0.01, takeoff: 0.02, climb: 0.12, cruise: 0.5, holding: 0.9, descent: 0.8, approach: 0.9, final: 0.95, landing: 0.98 }[st.phase] ?? 0.5;
}

// ---- Bausteine ------------------------------------------------------------

/** Dunkle Kachel mit dezenter Kante — der Grund jeder Taste. */
function kachelGrund(c, r, farbe = FARBE.flaeche) {
  return flaeche(kx(c), ky(r), KACHEL, KACHEL, farbe, 18);
}

/** Wert-Kachel: Marke oben, großer Wert, Unterzeile. */
function wertKachel(c, r, { marke: m, wert, unter, farbe = FARBE.text, markeFarbe }) {
  const x = kx(c) + 14, y = ky(r);
  const w = String(wert ?? "–");
  const g = w.length > 6 ? 30 : w.length > 4 ? 38 : 46;
  return [
    kachelGrund(c, r),
    marke(x, y + 30, m, markeFarbe),
    text(x, y + 88, w, { groesse: g, farbe, fett: true }),
    unter ? text(x, y + 124, unter, { groesse: 17, farbe: FARBE.leise }) : "",
  ].join("");
}

/** Streckenband über die Spalten c0..c1 in Reihe r, mit Laufschrift. */
function band(st, s, c0, c1, r, t) {
  const y = ky(r);
  const links = kx(c0), rechts = kx(c1) + KACHEL;
  const p = fortschritt(st, s.jetzt);
  // Linie beginnt hinter dem Startcode und endet vor dem Zielcode.
  const lx0 = kx(c0 + 1) + 6, lx1 = kx(c1) - 6;
  const ly = y + 62;
  const px = lx0 + (lx1 - lx0) * p;
  const g = phasenGruppe(st?.phase);
  const phasenFarbe = g === "anflug" ? FARBE.amber : g === "flug" ? FARBE.gruen : FARBE.cyan;
  const ab = iso(st?.takeoff_at) ?? iso(st?.block_off_at);
  const eta = zahl(st?.live?.ete_min) != null ? s.jetzt + st.live.ete_min * 60000 : (st?.flugzeiten?.plan_on_block ?? null) * 1000 || null;

  const teile = [
    flaeche(links, y, rechts - links, KACHEL, FARBE.flaeche, 18),
    // Start und Ziel
    text(links + 14, y + 30, "START", { groesse: 15, farbe: FARBE.leise, fett: true, abstand: 1.5 }),
    text(links + 14, y + 76, st?.dpt_airport ?? "----", { groesse: 40, farbe: FARBE.cyan, fett: true }),
    text(links + 14, y + 108, ab ? `${uhrZ(ab)}Z` : st?.dep_gate ? `Gate ${st.dep_gate}` : "", { groesse: 18, farbe: FARBE.leise }),
    text(rechts - 14, y + 30, "ZIEL", { groesse: 15, farbe: FARBE.leise, fett: true, abstand: 1.5, anker: "end" }),
    text(rechts - 14, y + 76, st?.arr_airport ?? "----", { groesse: 40, farbe: FARBE.cyan, fett: true, anker: "end" }),
    text(rechts - 14, y + 108, eta ? `ETA ${uhrZ(eta)}Z` : "", { groesse: 18, farbe: FARBE.leise, anker: "end" }),
    // Strecke: geflogen grün, Rest magenta gestrichelt
    `<line x1="${lx0}" y1="${ly}" x2="${lx1}" y2="${ly}" stroke="${FARBE.magenta}" stroke-width="4" stroke-dasharray="14 10" opacity="0.85"/>`,
    `<line x1="${lx0}" y1="${ly}" x2="${px}" y2="${ly}" stroke="${FARBE.gruen}" stroke-width="5" stroke-linecap="round"/>`,
    `<circle cx="${lx0}" cy="${ly}" r="7" fill="${FARBE.gruen}"/>`,
    `<circle cx="${lx1}" cy="${ly}" r="7" fill="none" stroke="${FARBE.magenta}" stroke-width="3"/>`,
    flugzeug(px, ly, 40, FARBE.text),
  ];
  // Phase und Flugfläche am Flugzeug — aber nur innerhalb einer Kachel.
  const kachelDesFlugzeugs = Math.min(c1, Math.max(c0, Math.floor(px / (KACHEL + FUGE))));
  const kx0 = kx(kachelDesFlugzeugs);
  teile.push(text(kx0 + KACHEL / 2, y + 30, PHASE_TEXT[st?.phase] ?? "", { groesse: 15, farbe: phasenFarbe, fett: true, anker: "middle", abstand: 1.5 }));

  // Laufschrift unter der Linie, nur im Mittelteil.
  const lauf = laufschrift(st, s);
  const mx0 = kx(c0 + 1), mx1 = kx(c1);
  const schritt = 18; // px je Takt
  const textBreite = lauf.length * 13.2 + 200;
  const versatz = ((t * schritt) % textBreite);
  teile.push(
    `<clipPath id="lauf"><rect x="${mx0}" y="${y + 92}" width="${mx1 - mx0}" height="44"/></clipPath>`,
    `<g clip-path="url(#lauf)">${text(mx1 - versatz, y + 124, lauf, { groesse: 22, farbe: FARBE.amber, fett: true, abstand: 1 })}${text(mx1 - versatz + textBreite, y + 124, lauf, { groesse: 22, farbe: FARBE.amber, fett: true, abstand: 1 })}</g>`,
  );
  return teile.join("");
}

/** Was in der Laufschrift steht — je Phase das, was gerade zählt. */
export function laufschrift(st, s) {
  if (!st) return "KEIN AKTIVER FLUG  ·  AEROACARS BEREIT";
  const m = phasenGruppe(st.phase) === "vor" ? st.dep_metar_decoded : st.arr_metar_decoded;
  const teile = [`${st.callsign ?? ""}  ${st.dpt_airport ?? ""}–${st.arr_airport ?? ""}`];
  if (m?.raw) teile.push(m.raw);
  else if (m) teile.push(`${m.icao} WIND ${m.wind_direction_deg ?? "VRB"}/${m.wind_speed_kt ?? "–"} Q${Math.round(m.qnh_hpa ?? 0)}`);
  const n = s.kabine?.naechste;
  if (n) teile.push(`NÄCHSTE ANSAGE ${(ANSAGEN[n.slot] ?? n.slot).toUpperCase()}${n.wann ? ` ${uhrZ(n.wann * 1000)}Z` : ""}`);
  if (st.predicted_runway || st.approach_runway) teile.push(`BAHN ${st.approach_runway ?? st.predicted_runway}`);
  if (st.planned_alternate) teile.push(`ALTN ${st.planned_alternate}`);
  return teile.join("   ·   ").toUpperCase();
}

// ---- Instrumente ----------------------------------------------------------

function instrument(art, c, r, st, s) {
  const live = st?.live ?? {};
  const x = kx(c), y = ky(r), mx = x + KACHEL / 2;
  switch (art) {
    case "hoehe": {
      const ft = zahl(live.altitude_pressure_ft) ?? zahl(live.altitude_msl_ft);
      const vs = zahl(live.vertical_speed_fpm) ?? 0;
      return kachelGrund(c, r) + marke(x + 14, y + 30, "Höhe") + text(x + 14, y + 86, hoehe(ft), { groesse: 40, fett: true, farbe: FARBE.gruen }) +
        (Math.abs(vs) > 200 ? pfeil(x + 118, y + 112, 34, vs > 0 ? 0 : 180, FARBE.cyan, 4) : "") +
        text(x + 14, y + 124, ft != null && ft < 10000 ? "ft" : "", { groesse: 17, farbe: FARBE.leise });
    }
    case "vs": {
      const vs = zahl(live.vertical_speed_fpm);
      const farbe = vs == null ? FARBE.leise : Math.abs(vs) < 200 ? FARBE.text : FARBE.cyan;
      return kachelGrund(c, r) + marke(x + 14, y + 30, "V/S") + text(x + 14, y + 86, vs == null ? "–" : `${vs > 0 ? "+" : ""}${Math.round(vs / 10) * 10}`, { groesse: 36, fett: true, farbe }) + text(x + 14, y + 124, "ft/min", { groesse: 17, farbe: FARBE.leise });
    }
    case "gs":
      return wertKachel(c, r, { marke: "GS", wert: zahl(live.gs_kt) == null ? "–" : Math.round(live.gs_kt), unter: zahl(live.ias_kt) == null ? "kt" : `IAS ${Math.round(live.ias_kt)} kt`, farbe: FARBE.gruen });
    case "wind": {
      const dir = zahl(live.wind_dir_deg), kt = zahl(live.wind_speed_kt);
      return kachelGrund(c, r) + marke(x + 14, y + 30, "Wind") +
        (dir != null && kt > 1 ? pfeil(x + 104, y + 82, 50, dir + 180, FARBE.cyan, 5) : "") +
        text(x + 14, y + 86, kt == null ? "–" : `${Math.round(kt)}`, { groesse: 40, fett: true }) +
        text(x + 14, y + 124, dir == null ? "kt" : `${zwei(Math.round(dir)).padStart(3, "0")}° kt`, { groesse: 17, farbe: FARBE.leise });
    }
    case "sprit": {
      const kg = zahl(st?.sim_fuel_kg ?? live.fuel_total_kg);
      const plan = zahl(st?.planned_block_fuel_kg);
      const res = zahl(st?.planned_reserve_kg);
      const farbe = kg != null && res && kg <= res * 1.2 ? FARBE.amber : FARBE.gruen;
      return kachelGrund(c, r) + bogen(mx, y + 80, 46, plan ? kg / plan : 0, farbe, 9) + text(mx, y + 88, tonnen(kg), { groesse: 30, fett: true, anker: "middle" }) +
        text(mx, y + 112, "t FUEL", { groesse: 15, farbe: FARBE.leise, anker: "middle", fett: true }) ;
    }
    case "eta": {
      const ete = zahl(live.ete_min);
      return wertKachel(c, r, { marke: "ETA", wert: ete == null ? "--:--" : uhrZ(s.jetzt + ete * 60000), unter: ete == null ? "" : `noch ${Math.floor(ete / 60)}:${zwei(Math.round(ete % 60))}`, farbe: FARBE.cyan });
    }
    case "luft":
      return wertKachel(c, r, { marke: "In Luft", wert: dauer(iso(st?.takeoff_at), iso(st?.landing_at) ?? s.jetzt), unter: st?.takeoff_at ? `ab ${uhrZ(iso(st.takeoff_at))}Z` : "" });
    case "block":
      return wertKachel(c, r, { marke: "Block", wert: dauer(iso(st?.block_off_at), iso(st?.block_on_at) ?? s.jetzt), unter: st?.block_off_at ? `ab ${uhrZ(iso(st.block_off_at))}Z` : "am Stand" });
    case "utc":
      return wertKachel(c, r, { marke: "UTC", wert: uhrZ(s.jetzt), unter: `lokal ${zwei(new Date(s.jetzt).getHours())}:${zwei(new Date(s.jetzt).getMinutes())}` });
    case "gate":
      return wertKachel(c, r, { marke: "Gate", wert: st?.dep_gate ?? "–", unter: st?.arr_gate ? `Ziel ${st.arr_gate}` : "", farbe: FARBE.cyan });
    case "std":
      return wertKachel(c, r, { marke: "STD", wert: uhrZ((st?.flugzeiten?.plan_off_block ?? null) * 1000 || null), unter: "Off-Block Plan" });
    case "sta":
      return wertKachel(c, r, { marke: "STA", wert: uhrZ((st?.flugzeiten?.plan_on_block ?? null) * 1000 || null), unter: "On-Block Plan" });
    case "blockfuel":
      return wertKachel(c, r, { marke: "Block Fuel", wert: tonnen(zahl(st?.planned_block_fuel_kg)), unter: "t laut OFP" });
    case "zfw": {
      const zfw = zahl(st?.sim_zfw_kg), max = zahl(st?.planned_max_zfw_kg);
      return kachelGrund(c, r) + marke(x + 14, y + 30, "ZFW") + text(x + 14, y + 84, tonnen(zfw), { groesse: 36, fett: true, farbe: zfw && max && zfw > max ? FARBE.rot : FARBE.text }) +
        balken(x + 14, y + 104, 116, max ? zfw / max : 0, zfw && max && zfw > max ? FARBE.rot : FARBE.gruen, 7) + text(x + 14, y + 130, max ? `max ${tonnen(max)} t` : "", { groesse: 15, farbe: FARBE.leise });
    }
    case "tow": {
      const tow = zahl(st?.sim_tow_kg), max = zahl(st?.planned_max_tow_kg);
      return kachelGrund(c, r) + marke(x + 14, y + 30, "TOW") + text(x + 14, y + 84, tonnen(tow), { groesse: 36, fett: true }) +
        balken(x + 14, y + 104, 116, max ? tow / max : 0, tow && max && tow > max ? FARBE.rot : FARBE.gruen, 7) + text(x + 14, y + 130, max ? `max ${tonnen(max)} t` : "", { groesse: 15, farbe: FARBE.leise });
    }
    case "netz": {
      const sim = !!st?.live, srv = st?.connection_state === "live";
      const ok = sim && srv;
      return kachelGrund(c, r) + marke(x + 14, y + 30, "Status") + text(x + 14, y + 80, ok ? "OK" : !sim ? "SIM" : "SRV", { groesse: 40, fett: true, farbe: ok ? FARBE.gruen : FARBE.rot }) +
        text(x + 14, y + 112, `Sim ${sim ? "●" : "○"}  Server ${srv ? "●" : "○"}`, { groesse: 15, farbe: FARBE.leise });
    }
    case "bahn":
      return wertKachel(c, r, { marke: "Bahn", wert: st?.approach_runway ?? st?.predicted_runway ?? "–", unter: st?.approach_runway ? "im Anflug" : "erwartet", farbe: FARBE.cyan });
    case "strecke":
      return wertKachel(c, r, { marke: "Strecke", wert: Math.round(zahl(st?.distance_nm) ?? 0), unter: "nm geflogen" });
    default:
      return kachelGrund(c, r);
  }
}

export const INSTRUMENTE = {
  kein: ["utc", "netz", "gate", "std", "sta", "blockfuel", "zfw", "tow"],
  vor: ["gate", "std", "sta", "blockfuel", "zfw", "tow", "utc", "netz"],
  flug: ["hoehe", "vs", "gs", "wind", "sprit", "eta", "luft", "utc"],
  anflug: ["hoehe", "vs", "gs", "wind", "bahn", "eta", "sprit", "utc"],
  nach: ["block", "luft", "strecke", "sprit", "gate", "sta", "utc", "netz"],
};

// ---- Phasen-Zone ----------------------------------------------------------

/** Boarding-Musik als große Fläche über n Kacheln, mit Equalizer. */
function musikFlaeche(c, r, n, s, t) {
  const k = s.kabine;
  const x = kx(c), y = ky(r), b = spanne(n);
  const an = !!k?.boarding_musik;
  const stumm = k && k.musik_live <= 0.001 && k.musik_gespeichert > 0.001;
  const farbe = !k ? FARBE.leise : !an ? FARBE.leise : stumm ? FARBE.amber : FARBE.cyan;
  const saeulen = [];
  for (let i = 0; i < 14; i++) {
    const h = an && !stumm ? 10 + Math.abs(Math.sin(t * 1.7 + i * 0.9)) * 38 + (i % 3) * 4 : 6;
    saeulen.push(`<rect x="${x + 14 + i * ((b - 28) / 14)}" y="${y + 112 - h}" width="${(b - 28) / 14 - 6}" height="${h}" rx="3" fill="${farbe}" opacity="${an ? 0.9 : 0.35}"/>`);
  }
  return flaeche(x, y, b, KACHEL, FARBE.flaeche, 18) + marke(x + 14, y + 30, "Boarding-Musik", farbe) +
    text(x + b - 14, y + 30, !k ? "CLIENT ZU ALT" : !an ? "AUS" : stumm ? "STUMM" : `${Math.round(k.musik_gespeichert * 100)} %`, { groesse: 15, fett: true, farbe, anker: "end", abstand: 1.5 }) +
    saeulen.join("") + text(x + 14, y + 134, an ? "kurz: stumm · halten: aus" : "halten: einschalten", { groesse: 14, farbe: FARBE.leise });
}

/** Kabinen-Zeitstrahl: kommende Ansagen als Stationen, Lichtpunkt zur nächsten. */
function kabinenStrahl(c, r, n, s, t) {
  const k = s.kabine;
  const x = kx(c), y = ky(r), b = spanne(n);
  const folge = k?.folgende ?? (k?.naechste ? [k.naechste] : []);
  const teile = [flaeche(x, y, b, KACHEL, FARBE.flaeche, 18), marke(x + 14, y + 30, "Kabine", FARBE.cyan)];
  if (!k) return teile.join("") + text(x + 14, y + 90, "Client zu alt", { groesse: 22, farbe: FARBE.leise });
  if (k.stumm) teile.push(text(x + b - 14, y + 30, "STUMM", { groesse: 15, fett: true, farbe: FARBE.rot, anker: "end", abstand: 1.5 }));
  const ly = y + 74;
  const sx = (i) => x + 60 + i * ((b - 120) / Math.max(1, Math.min(folge.length, 4) - 1 || 1));
  teile.push(`<line x1="${x + 30}" y1="${ly}" x2="${x + b - 30}" y2="${ly}" stroke="${FARBE.linie}" stroke-width="4"/>`);
  folge.slice(0, 4).forEach((f, i) => {
    const px = folge.length === 1 ? x + b / 2 : sx(i);
    const wartet = i === 0 && k.hinweis === "anschnallzeichen";
    const farbe = i === 0 ? (wartet ? FARBE.amber : FARBE.cyan) : FARBE.leise;
    teile.push(`<circle cx="${px}" cy="${ly}" r="${i === 0 ? 9 : 6}" fill="${farbe}"/>`);
    // Text innerhalb der Kachel, in der die Station liegt.
    const kc = Math.floor(px / (KACHEL + FUGE));
    const tx = Math.min(Math.max(px, kx(kc) + 10), kx(kc) + KACHEL - 10);
    teile.push(text(tx, ly + 34, (ANSAGEN[f.slot] ?? f.slot).toUpperCase(), { groesse: 15, fett: true, farbe, anker: "middle" }));
    if (f.wann) teile.push(text(tx, ly - 18, `${uhrZ(f.wann * 1000)}Z`, { groesse: 15, farbe: FARBE.leise, anker: "middle" }));
    if (wartet) teile.push(`<rect x="${px - 30}" y="${ly - 14}" width="6" height="28" rx="2" fill="${FARBE.amber}"/>`);
  });
  // Lichtpunkt wandert auf die nächste Station zu (oder steht an der Schranke).
  if (folge.length) {
    const ziel = folge.length === 1 ? x + b / 2 : sx(0);
    const start = x + 30;
    const wartet = k.hinweis === "anschnallzeichen";
    const pos = wartet ? ziel - 44 : start + ((t * 30) % Math.max(1, ziel - start - 12));
    teile.push(`<circle cx="${pos}" cy="${ly}" r="6" fill="${wartet ? FARBE.amber : FARBE.cyan}" filter="url(#leuchten)"/>`);
  }
  return teile.join("");
}

/** Landebahn mit Gegen-/Seitenwind über n Kacheln. */
function landebahn(c, r, n, st) {
  const x = kx(c), y = ky(r), b = spanne(n);
  const bahn = st?.approach_runway ?? st?.predicted_runway ?? "–";
  const gw = zahl(st?.live?.headwind_kt), sw = zahl(st?.live?.crosswind_kt);
  const by = y + 64;
  const teile = [
    flaeche(x, y, b, KACHEL, FARBE.flaeche, 18),
    marke(x + 14, y + 30, "Landebahn", FARBE.cyan),
    // Bahn als Draufsicht, Anflug von links
    `<rect x="${x + 70}" y="${by}" width="${b - 140}" height="34" rx="4" fill="#2a3440"/>`,
    ...Array.from({ length: Math.floor((b - 200) / 46) }, (_, i) => `<rect x="${x + 100 + i * 46}" y="${by + 15}" width="24" height="4" fill="#d7dee5"/>`),
    `<line x1="${x + 30}" y1="${by + 17}" x2="${x + 70}" y2="${by + 17}" stroke="${FARBE.magenta}" stroke-width="3" stroke-dasharray="6 6"/>`,
    text(x + 14, y + 134, `RWY ${bahn}`, { groesse: 22, fett: true, farbe: FARBE.cyan }),
  ];
  // Werte in der rechten Kachel
  const rx = kx(c + n - 1) + 14;
  teile.push(
    text(rx, y + 30, gw == null ? "" : gw >= 0 ? "GEGENWIND" : "RÜCKENWIND", { groesse: 14, fett: true, farbe: gw != null && gw < 0 ? FARBE.amber : FARBE.leise, abstand: 1 }),
    text(rx, y + 134, sw == null ? "" : `X ${Math.abs(Math.round(sw))} kt`, { groesse: 20, fett: true, farbe: Math.abs(sw ?? 0) > 20 ? FARBE.amber : FARBE.text }),
    gw == null ? "" : text(kx(c + n - 2) + KACHEL - 14, y + 134, `${Math.abs(Math.round(gw))} kt`, { groesse: 20, fett: true, anker: "end" }),
  );
  if (sw != null && Math.abs(sw) > 1) teile.push(pfeil(x + b / 2, by + 17, 54, sw > 0 ? 0 : 180, FARBE.amber, 5));
  return teile.join("");
}

/** Ergebnis der Landung, ruhig gehalten (keine Rangliste, kein Jubel). */
function landungFlaeche(c, r, n, st, s) {
  const x = kx(c), y = ky(r), b = spanne(n);
  const d = s.debrief;
  const fpm = zahl(st?.landing_rate_fpm), g = zahl(st?.landing_g_force);
  const farbe = { smooth: FARBE.gruen, acceptable: FARBE.gruen, firm: FARBE.amber, hard: FARBE.rot, severe: FARBE.rot }[d?.score_label] ?? FARBE.text;
  return flaeche(x, y, b, KACHEL, FARBE.flaeche, 18) + marke(x + 14, y + 30, "Landung", FARBE.cyan) +
    text(x + 14, y + 100, fpm == null ? "—" : `${Math.round(fpm)}`, { groesse: 56, fett: true }) + text(x + 14, y + 132, "ft/min", { groesse: 17, farbe: FARBE.leise }) +
    text(kx(c + 1) + 14, y + 100, g == null ? "" : `${g.toFixed(2).replace(".", ",")}`, { groesse: 44, fett: true }) + text(kx(c + 1) + 14, y + 132, g == null ? "" : "G", { groesse: 17, farbe: FARBE.leise }) +
    (n > 2 ? marke(kx(c + 2) + 14, y + 30, "Bewertung") + text(kx(c + 2) + 14, y + 100, d?.grade_letter ?? "…", { groesse: 56, fett: true, farbe }) + text(kx(c + 2) + 14, y + 132, d?.score_numeric != null ? `${d.score_numeric}/100` : "wird bewertet", { groesse: 17, farbe: FARBE.leise }) : "") +
    (n > 3 ? marke(kx(c + 3) + 14, y + 30, "Bahn") + text(kx(c + 3) + 14, y + 100, st?.approach_runway ?? "–", { groesse: 44, fett: true, farbe: FARBE.cyan }) : "");
}

/** Wetter am Flughafen über zwei Kacheln: Windpfeil links, Werte rechts. */
function wetterFlaeche(c, r, m, icao) {
  const x = kx(c), y = ky(r), b = spanne(2);
  if (!m) return flaeche(x, y, b, KACHEL, FARBE.flaeche, 18) + marke(x + 14, y + 30, `Wetter ${icao ?? ""}`) + text(x + 14, y + 90, "kein METAR", { groesse: 24, farbe: FARBE.leise });
  const dir = zahl(m.wind_direction_deg), kt = zahl(m.wind_speed_kt);
  const sicht = m.visibility_m == null ? "–" : m.visibility_m >= 9999 ? "10km+" : `${Math.round(m.visibility_m / 100) / 10}km`;
  const wolke = (m.cloud_layers ?? [])[0];
  const rx = kx(c + 1) + 14;
  return flaeche(x, y, b, KACHEL, FARBE.flaeche, 18) + marke(x + 14, y + 30, `Wetter ${m.icao ?? icao ?? ""}`, FARBE.cyan) +
    `<circle cx="${x + 72}" cy="${y + 86}" r="40" fill="none" stroke="${FARBE.linie}" stroke-width="3"/>` +
    (dir != null && kt > 0 ? pfeil(x + 72, y + 86, 66, dir + 180, FARBE.cyan, 5) : text(x + 72, y + 94, "CALM", { groesse: 18, anker: "middle", fett: true })) +
    text(rx, y + 52, `${dir == null ? "VRB" : String(Math.round(dir)).padStart(3, "0")}/${kt == null ? "–" : Math.round(kt)}${m.gust_kt ? `G${Math.round(m.gust_kt)}` : ""}`, { groesse: 22, fett: true }) +
    text(rx, y + 82, `Q${Math.round(m.qnh_hpa ?? 0)}`, { groesse: 22, fett: true, farbe: FARBE.cyan }) +
    text(rx, y + 110, sicht, { groesse: 19 }) +
    text(rx, y + 134, wolke ? `${wolke.cover} ${wolke.base_ft ?? ""}` : `${m.temperature_c != null ? Math.round(m.temperature_c) + "°" : ""}`, { groesse: 17, farbe: FARBE.leise });
}

/** Nächste Ansage als Karte über zwei Kacheln. */
function ansageKarte(c, r, s) {
  const x = kx(c), y = ky(r), b = spanne(2);
  const k = s.kabine, n = k?.naechste;
  const wartet = k?.hinweis === "anschnallzeichen";
  return flaeche(x, y, b, KACHEL, FARBE.flaeche, 18) + marke(x + 14, y + 30, "Nächste Ansage", wartet ? FARBE.amber : FARBE.cyan) +
    text(x + 14, y + 86, !k ? "–" : k.stumm ? "KABINE STUMM" : n ? (ANSAGEN[n.slot] ?? n.slot).toUpperCase() : "KEINE", { groesse: 28, fett: true, farbe: k?.stumm ? FARBE.rot : FARBE.text }) +
    text(x + 14, y + 124, wartet ? "wartet: Anschnallzeichen aus" : n?.wann ? `ca. ${uhrZ(n.wann * 1000)}Z` : "", { groesse: 17, farbe: wartet ? FARBE.amber : FARBE.leise });
}

// ---- Bedienreihe ----------------------------------------------------------

function bedienKachel(art, c, r, s) {
  const k = s.kabine, x = kx(c), y = ky(r), mx = x + KACHEL / 2;
  const grund = flaeche(x, y, KACHEL, KACHEL, "#0d1622", 18) + `<rect x="${x + 1}" y="${y + 1}" width="${KACHEL - 2}" height="${KACHEL - 2}" rx="17" fill="none" stroke="${FARBE.cyan}" stroke-opacity="0.35" stroke-width="2"/>`;
  const knopf = (zeile1, zeile2, farbe = FARBE.cyan, unter = "") =>
    grund + text(mx, y + 70, zeile1, { groesse: 26, fett: true, farbe, anker: "middle" }) + (zeile2 ? text(mx, y + 100, zeile2, { groesse: 20, fett: true, farbe, anker: "middle" }) : "") +
    (unter ? text(mx, y + 130, unter, { groesse: 14, farbe: FARBE.leise, anker: "middle" }) : "");
  switch (art) {
    case "zurueck":
      return grund + `<path d="M${mx + 6} ${y + 46} l-26 24 26 24" stroke="${FARBE.cyan}" stroke-width="7" fill="none" stroke-linecap="round" stroke-linejoin="round"/>` + text(mx, y + 128, "ZURÜCK", { groesse: 15, fett: true, farbe: FARBE.leise, anker: "middle", abstand: 1.5 });
    case "pa":
      return !k ? knopf("PA", "", FARBE.leise, "Client zu alt") : k.stumm ? knopf("PA", "STUMM", FARBE.rot, "drücken: an") : knopf("PA", "AN", FARBE.gruen, "1 s halten: stumm");
    case "ans_minus":
    case "ans_plus":
    case "mus_minus":
    case "mus_plus": {
      const ansage = art.startsWith("ans");
      const plus = art.endsWith("plus");
      const wert = k ? (ansage ? k.lautstaerke_gespeichert : k.musik_gespeichert) : null;
      return grund + text(mx, y + 34, ansage ? "ANSAGEN" : "MUSIK", { groesse: 14, fett: true, farbe: FARBE.leise, anker: "middle", abstand: 1.5 }) +
        text(mx, y + 92, plus ? "+" : "−", { groesse: 64, fett: true, farbe: FARBE.cyan, anker: "middle" }) +
        balken(x + 18, y + 116, 108, wert ?? 0, FARBE.cyan, 8);
    }
    case "boarding":
      if (!k) return knopf("MUSIK", "", FARBE.leise, "Client zu alt");
      // B612 hat keine Musiknote: als Pfad gezeichnet.
      return grund + note(mx, y + 52, 40, k.boarding_musik ? FARBE.gruen : FARBE.leise) +
        text(mx, y + 104, k.boarding_musik ? "AN" : "AUS", { groesse: 22, fett: true, farbe: k.boarding_musik ? FARBE.gruen : FARBE.leise, anker: "middle" }) +
        text(mx, y + 130, k.boarding_musik ? "halten: aus" : "halten: an", { groesse: 14, farbe: FARBE.leise, anker: "middle" });
    case "cdm":
      return knopf("CDM", "", k ? FARBE.cyan : FARBE.leise, k ? "VATSIM" : "Client zu alt");
    default:
      return kachelGrund(c, r);
  }
}

// ---- Geräte ---------------------------------------------------------------

/**
 * Die Szene für ein Gerät. `t` ist die Zeit in Takten (für Bewegung).
 * Ergebnis: { breite, hoehe, svg, aktionen: { "c,r": {kurz, lang, halten} } }
 */
export function szene(geraet, s, t = 0) {
  const st = s.erreichbar ? s.status : null;
  const g = phasenGruppe(st?.phase);
  const teile = [];
  const aktionen = {};
  const setze = (c, r, a) => (aktionen[`${c},${r}`] = a);

  if (geraet === "xl") {
    teile.push(band(st, s, 0, 7, 0, t));
    for (let c = 0; c < 8; c++) setze(c, 0, { lang: "map" });
    INSTRUMENTE[g === "kein" ? "kein" : g].forEach((art, c) => {
      teile.push(instrument(art, c, 1, st, s));
      setze(c, 1, { lang: ["sprit", "blockfuel", "zfw", "tow"].includes(art) ? "briefing" : "cockpit" });
    });
    if (g === "vor" || g === "kein") {
      teile.push(musikFlaeche(0, 2, 2, s, t), wetterFlaeche(2, 2, st?.dep_metar_decoded, st?.dpt_airport), wetterFlaeche(4, 2, st?.arr_metar_decoded, st?.arr_airport), ansageKarte(6, 2, s));
      setze(0, 2, { kurz: "musik_stumm", halten: "boarding" }); setze(1, 2, { kurz: "musik_stumm", halten: "boarding" });
      [2, 3, 4, 5].forEach((c) => setze(c, 2, { lang: "briefing" })); [6, 7].forEach((c) => setze(c, 2, { lang: "kabine" }));
    } else if (g === "flug") {
      teile.push(kabinenStrahl(0, 2, 6, s, t), wetterFlaeche(6, 2, st?.arr_metar_decoded, st?.arr_airport));
      for (let c = 0; c < 6; c++) setze(c, 2, { lang: "kabine" }); [6, 7].forEach((c) => setze(c, 2, { lang: "briefing" }));
    } else if (g === "anflug") {
      teile.push(landebahn(0, 2, 4, st), wetterFlaeche(4, 2, st?.arr_metar_decoded, st?.arr_airport), ansageKarte(6, 2, s));
      for (let c = 0; c < 6; c++) setze(c, 2, { lang: c < 4 ? "landing" : "briefing" }); [6, 7].forEach((c) => setze(c, 2, { lang: "kabine" }));
    } else {
      teile.push(landungFlaeche(0, 2, 4, st, s), musikFlaeche(4, 2, 2, s, t), ansageKarte(6, 2, s));
      for (let c = 0; c < 4; c++) setze(c, 2, { lang: "landing" });
      setze(4, 2, { kurz: "musik_stumm", halten: "boarding" }); setze(5, 2, { kurz: "musik_stumm", halten: "boarding" }); [6, 7].forEach((c) => setze(c, 2, { lang: "kabine" }));
    }
    ["zurueck", "pa", "ans_minus", "ans_plus", "mus_minus", "mus_plus", "boarding", "cdm"].forEach((art, c) => {
      teile.push(bedienKachel(art, c, 3, s));
      setze(c, 3, { bedienung: art });
    });
    return fertig(8, 4, teile, aktionen);
  }

  if (geraet === "15") {
    teile.push(band(st, s, 0, 4, 0, t));
    for (let c = 0; c < 5; c++) setze(c, 0, { lang: "map" });
    if (g === "vor" || g === "kein") {
      teile.push(musikFlaeche(0, 1, 2, s, t), instrument("std", 2, 1, st, s), instrument("gate", 3, 1, st, s), instrument("utc", 4, 1, st, s));
      setze(0, 1, { kurz: "musik_stumm", halten: "boarding" }); setze(1, 1, { kurz: "musik_stumm", halten: "boarding" });
    } else if (g === "flug") {
      ["hoehe", "gs", "wind", "eta", "sprit"].forEach((art, c) => { teile.push(instrument(art, c, 1, st, s)); setze(c, 1, { lang: "cockpit" }); });
    } else if (g === "anflug") {
      teile.push(landebahn(0, 1, 3, st), instrument("hoehe", 3, 1, st, s), instrument("eta", 4, 1, st, s));
      for (let c = 0; c < 3; c++) setze(c, 1, { lang: "landing" });
    } else {
      teile.push(landungFlaeche(0, 1, 3, st, s), instrument("block", 3, 1, st, s), instrument("utc", 4, 1, st, s));
      for (let c = 0; c < 3; c++) setze(c, 1, { lang: "landing" });
    }
    ["zurueck", "pa", "ans_minus", "ans_plus", "boarding"].forEach((art, c) => { teile.push(bedienKachel(art, c, 2, s)); setze(c, 2, { bedienung: art }); });
    return fertig(5, 3, teile, aktionen);
  }

  // StreamDeck +: 4 × 2 Tasten; der Touch-Streifen bekommt das Band eigens (streifen()).
  const instr = INSTRUMENTE[g === "kein" ? "kein" : g];
  instr.slice(0, 4).forEach((art, c) => { teile.push(instrument(art, c, 0, st, s)); setze(c, 0, { lang: "cockpit" }); });
  ["zurueck", "pa", "boarding", "cdm"].forEach((art, c) => { teile.push(bedienKachel(art, c, 1, s)); setze(c, 1, { bedienung: art }); });
  return fertig(4, 2, teile, aktionen);
}

function fertig(spalten, reihen, teile, aktionen) {
  const breite = spanne(spalten), hoehe = reihen * KACHEL + (reihen - 1) * FUGE;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${breite}" height="${hoehe}" viewBox="0 0 ${breite} ${hoehe}"><defs>${LEUCHTEN}</defs><rect width="${breite}" height="${hoehe}" fill="${FARBE.grund}"/>${teile.join("")}</svg>`;
  return { spalten, reihen, breite, hoehe, svg, aktionen };
}

/** Eine Kachel aus der Szene: dieselbe SVG, Ausschnitt per viewBox. */
export function kachelSvg(sz, c, r) {
  const inhalt = sz.svg.replace(/^<svg[^>]*>/, "").replace(/<\/svg>$/, "");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${KACHEL}" height="${KACHEL}" viewBox="${kx(c)} ${ky(r)} ${KACHEL} ${KACHEL}">${inhalt}</svg>`;
}

/** Touch-Streifen des StreamDeck + (800 × 100, ohne Blenden): das Band am Stück. */
export function streifen(s, t = 0) {
  const st = s.erreichbar ? s.status : null;
  const p = fortschritt(st, s.jetzt);
  const x0 = 120, x1 = 680, ly = 44;
  const px = x0 + (x1 - x0) * p;
  const lauf = laufschrift(st, s);
  const textBreite = lauf.length * 11 + 160;
  const v = (t * 16) % textBreite;
  const g = phasenGruppe(st?.phase);
  const eta = zahl(st?.live?.ete_min) != null ? s.jetzt + st.live.ete_min * 60000 : null;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="100" viewBox="0 0 800 100"><defs>${LEUCHTEN}<clipPath id="ls"><rect x="110" y="66" width="580" height="34"/></clipPath></defs>
<rect width="800" height="100" fill="${FARBE.grund}"/>
${text(16, 44, st?.dpt_airport ?? "----", { groesse: 34, fett: true, farbe: FARBE.cyan })}${text(16, 86, PHASE_TEXT[st?.phase] ?? "", { groesse: 13, fett: true, farbe: g === "anflug" ? FARBE.amber : FARBE.gruen, abstand: 1 })}
${text(784, 44, st?.arr_airport ?? "----", { groesse: 34, fett: true, farbe: FARBE.cyan, anker: "end" })}${text(784, 86, eta ? `ETA ${uhrZ(eta)}Z` : "", { groesse: 13, fett: true, farbe: FARBE.leise, anker: "end" })}
<line x1="${x0}" y1="${ly}" x2="${x1}" y2="${ly}" stroke="${FARBE.magenta}" stroke-width="3" stroke-dasharray="12 9"/>
<line x1="${x0}" y1="${ly}" x2="${px}" y2="${ly}" stroke="${FARBE.gruen}" stroke-width="4" stroke-linecap="round"/>
${flugzeug(px, ly, 30, FARBE.text)}
<g clip-path="url(#ls)">${text(690 - v, 90, lauf, { groesse: 18, fett: true, farbe: FARBE.amber, abstand: 1 })}${text(690 - v + textBreite, 90, lauf, { groesse: 18, fett: true, farbe: FARBE.amber, abstand: 1 })}</g></svg>`;
}
