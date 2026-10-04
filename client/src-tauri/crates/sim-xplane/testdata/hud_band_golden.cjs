#!/usr/bin/env node
/*
 * Erzeugt hud_band_golden.cjson: Golden-Datagramme fuer hud_band.rs.
 *
 * Die Erwartung stammt NICHT aus Rust, sondern aus dem echten panel.js:
 * das Skript laedt panel.js mit einem Schein-DOM, setzt Zustand und Uhr,
 * ruft zeichne()/zeichneTicker() und liest die entstandenen Texte/Klassen
 * aus. Daraus wird nach ADR-0005 das Datagramm gebaut (Abbildung
 * Klassen -> Farben unten; Art je panel.css-Element: .aa2-age t, .aa2-msg m,
 * .aa2-dot p, .aa2-ident i, .aa2-state s, Beschriftung l, Wert v, .aa2-tail x;
 * Landenote z, Etikett e). Die zwei bewussten Abweichungen (Wind als Text,
 * kein Spinner) rechnet dieses Skript selbst nach.
 *
 * Aufruf (aus diesem Ordner):  node hud_band_golden.cjs > hud_band_golden.cjson
 * Ohne Argument bleibt die Ausgabe gleich (fester Zufallssamen).
 */
const fs = require('fs');
const path = require('path');
const SRC = path.resolve(__dirname,
  '../../../../../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.js');
let src = fs.readFileSync(SRC, 'utf8');
src = src.replace(/\}\)\(\);\s*$/,
  "G.__t={Z:Z,K:K,zeichne:zeichne,zeichneTicker:zeichneTicker,nurAscii:nurAscii,kuerze:kuerze};})();");
function el(tag) {
  const e = { tag, className: '', textContent: '', style: {}, children: [], childNodes: [],
    classList: { contains: c => (e.className || '').split(' ').includes(c) },
    appendChild(c) { e.children.push(c); e.childNodes.push(c); e.firstChild = e.children[0]; return c; },
    removeChild() {}, setAttribute() {}, removeAttribute() {}, addEventListener() {},
    querySelector() { return null; }, offsetWidth: 1, firstChild: null };
  return e;
}
const strip = el('div'); strip.className = 'aa2-strip';
global.document = { createElement: el, createElementNS: (n, t) => el(t),
  querySelector: s => (s === '.aa2-strip' ? strip : null), readyState: 'complete', addEventListener() {} };
global.window = global; global.fetch = () => new Promise(() => {});
let NOW = 0; const RD = Date;
global.Date = class extends RD { constructor(...a) { if (a.length === 0) super(NOW); else super(...a); } static now() { return NOW; } };
Date.parse = RD.parse; Date.UTC = RD.UTC;
global.setInterval = () => 0;
eval(src);
const T = global.__t;

const HOST = '127.0.0.1', PORT = 47847;
function rund(x) { return Math.floor(x + 0.5); }

function render(e) {
  NOW = e.jetzt_ms;
  T.Z.verbunden = e.verbunden; T.Z.status = e.status; T.Z.debrief = e.debrief; T.Z.aktivitaet = e.aktivitaet;
  T.Z.fehlerText = e.fehler_text; T.Z.fehlerSeit = e.fehler_seit_ms;
  T.zeichne(); T.zeichneTicker();
  const K = T.K, vis = n => n.style.display !== 'none';
  const wertFarbe = c => (/aa2-g\b/.test(c) ? 'g' : /aa2-w\b/.test(c) ? 'w' : /aa2-b\b/.test(c) ? 'b' : 'n');
  const dot = /aa2-live/.test(K.dot.className) ? 'g' : /aa2-sync/.test(K.dot.className) ? 'a'
    : /aa2-off/.test(K.dot.className) ? 'b' : 'd';
  const t1 = [];
  if (vis(K.ticker)) {
    if (K.age.textContent) t1.push('nt' + T.nurAscii(K.age.textContent));
    const c = / aa2-w/.test(K.msg.className) ? 'w' : / aa2-e/.test(K.msg.className) ? 'b' : 'd';
    t1.push(c + 'm' + K.msg.textContent);
  }
  const z2 = [dot + 'p', 'ni' + T.nurAscii(K.ident.textContent)];
  if (vis(K.state)) z2.push(wertFarbe(K.state.className) + 's' + T.nurAscii(K.state.textContent));
  if (vis(K.score)) {
    z2.push('nz' + K.scoreVal.textContent);
    const bc = / aa2-w/.test(K.scoreBand.className) ? 'w' : / aa2-b/.test(K.scoreBand.className) ? 'b' : 'g';
    z2.push(bc + 'e' + T.nurAscii(K.scoreBand.textContent));
  }
  for (const z of K.zellen) {
    if (!vis(z.wurzel)) continue;
    z2.push('dl' + z.lbl.textContent);
    z2.push(wertFarbe(z.val.className) + 'v' + T.nurAscii(z.val.textContent));
  }
  if (vis(K.wind)) {
    // Abweichung 1: Wind als Text statt Pfeil (HW/TW, XW, R/L)
    const live = e.status.live, g = live.headwind_kt, q = live.crosswind_kt, a = Math.abs(q);
    z2.push('dlWind');
    z2.push('nv' + (g >= 0 ? 'HW ' : 'TW ') + rund(Math.abs(g)));
    z2.push((a >= 25 ? 'b' : a >= 15 ? 'w' : 'n') + 'vXW ' + rund(a) + (rund(a) === 0 ? '' : q > 0 ? ' R' : ' L'));
  }
  if (vis(K.tail)) z2.push('dx' + T.nurAscii(K.tail.textContent));
  const l1 = t1.length ? t1.join('\t') : '';
  return { ruhig: /aa2-quiet/.test(K.streifen.className),
    text: 'BAND 1 ' + (/aa2-quiet/.test(K.streifen.className) ? 1 : 0) + ' 2\n' + l1 + '\n' + z2.join('\t') + '\n' };
}

// ---- Szenarien --------------------------------------------------------
let seed = 20261004;
function rnd() { seed = (seed * 1664525 + 1013904223) % 4294967296; return seed / 4294967296; }
const pick = a => a[Math.floor(rnd() * a.length)];
const maybe = (p, v) => (rnd() < p ? v : undefined);
const num = (lo, hi, d) => { const x = lo + rnd() * (hi - lo); return d === undefined ? Math.round(x) : Math.round(x * 10 ** d) / 10 ** d; };
const PHASEN = ['preflight','boarding','pushback','taxi_out','takeoff_roll','takeoff','climb','cruise','holding','descent','approach','final','landing','taxi_in','blocks_on','arrived','pirep_submitted','xyz_neu'];
const METARS = [
  'METAR EDDB AUTO 26012KT 240V300 9999 -SHRA FEW///TCU 30/13 Q1011',
  'EDDF 041050Z 24015G25KT 9999 SCT040 BKN080 12/05 Q1018 NOSIG',
  'KJFK 041051Z 18010KT 10SM FEW250 22/10 A3002 RMK AO2 SLP165',
  'LFPG 041000Z VRB02KT CAVOK 15/09 Q1020 TEMPO 4000 RA',
  'EGLL 3000 BR OVC003 M02/M03 Q1030', 'garbage text without tokens at all that goes on and on and on for quite a while longer than forty eight chars',
  'METAR LOWW 041020Z AUTO 31008KT 5000 +TSRA BKN020CB OVC040 18/17 Q0998 BECMG 3000',
  'SPECI EDDM 041030Z 00000KT 0400 R26L/0600 FG VV001 M01/M01 Q1025', '', 'XX',
  'KSFO 1/2SM FZFG VV002 A2992', 'ETAD 35004KT 9999 VCSH FEW030 BKN///', 'EDDH 270V310 NDV 9999NDV 20015MPS',
];
function decoded() {
  const d = {};
  Object.assign(d, maybe(0.8, { icao: pick(['EDDF','KJFK','LFPG','EGLL','Ä-Üx']) }), maybe(0.4, { weather: pick(['-SHRA','TS','FG','+RA Böen']) }),
    maybe(0.8, { wind_speed_kt: pick([0, 5, 12.5, 24.5, 40]) }), maybe(0.6, { wind_direction_deg: pick([0, 5, 90.5, 359.6, 360]) }),
    maybe(0.3, { gust_kt: pick([18, 30.4]) }), maybe(0.7, { visibility_m: pick([200, 800, 1500, 4999, 5000, 7400, 9999, 10000]) }),
    maybe(0.7, { cloud_layers: [pick([{cover:'FEW',base_ft:2500},{cover:'BKN',base_ft:1200},{cover:'OVC',base_ft:400},{cover:'SCT',base_ft:900},{cover:'BKNCB',base_ft:3000},{cover:'VV',base_ft:100},{base_ft:2000},{cover:'BKN'}]),
      pick([{cover:'OVC',base_ft:3000},{cover:'FEW',base_ft:300},{cover:'BKN',base_ft:1200}])] }),
    maybe(0.7, { temperature_c: pick([30, 12.4, 0, -0.4, -5.5, 101]), dewpoint_c: pick([13, 5, -3.6, 0, 99]) }),
    maybe(0.8, { qnh_hpa: pick([1013, 1011.4, 998, 1030.5, 995.5]) }));
  return d;
}
function live() {
  const l = {};
  Object.assign(l, maybe(0.9, { altitude_msl_ft: pick([-150, 0, 450, 3949, 3950, 9999.5, 10000.4, 10001, 25000, 36000.4]) }),
    maybe(0.8, { altitude_agl_ft: pick([0, 49, 50, 149.5, 1000, 2500.5]) }),
    maybe(0.7, { altitude_pressure_ft: pick([-300, 5000, 17499, 17500, 17500.4, 36000, 4050]) }),
    maybe(0.5, { qnh_hpa: pick([1013.25, 1013, 1012.6, 1012.7, 1020]) }),
    maybe(0.8, { vertical_speed_fpm: pick([-1200, -700, -300, 0, 150, -0.4, -2000, -1000.5, -450.5, -1500]) }),
    maybe(0.8, { gs_kt: pick([0, 39, 40, 140, 160, 250]) }), maybe(0.8, { ias_kt: pick([0, 135.5, 250, 140.4]) }),
    maybe(0.8, { bank_deg: pick([0, -0.04, 2.25, -12.35, 0.05, 1.005]) }),
    maybe(0.7, { headwind_kt: pick([12, -6.5, 0, 0.4, 24.5, -0.5, 30]), crosswind_kt: pick([3, -8, 0, 14.5, -15, 25, -26.4, 0.4, -0.5]) }),
    maybe(0.8, { ete_min: pick([0, 5, 45, 59, 60, 125, 600, 1500]) }));
  return l;
}
function status() {
  const s = {};
  const ph = pick(PHASEN);
  s.phase = ph;
  Object.assign(s, maybe(0.7, { callsign: pick(['DLH 400', 'GAF 12Ä', 'N123AB', '', 'X\tY'] ) }),
    maybe(0.5, { airline_icao: pick(['DLH', 'GAF', '']), flight_number: pick(['400', '12', '']) }),
    maybe(0.9, { dpt_airport: pick(['EDDF', 'EDDB', '']), arr_airport: pick(['KJFK', 'EGLL', '']) }),
    maybe(0.6, { takeoff_at: pick(['2026-10-04T10:30:00Z', '2026-10-04T11:59:30Z', '2026-10-03T03:00:00.123Z', '2026-10-04T13:00:00Z', 'kaputt']) }),
    maybe(0.15, { paused_since: pick(['2026-10-04T11:40:00Z', '2026-10-04T11:59:59Z']) }),
    maybe(0.7, { landing_score_finalized: pick([true, false]) }),
    maybe(0.5, { connection_state: pick(['ok', 'failing', 'connecting']) }),
    maybe(0.5, { queued_position_count: pick([0, 0, 3, 12]) }),
    maybe(0.5, { sim_fuel_kg: pick([1000, 5120.5, 0, 6300]), planned_block_fuel_kg: pick([5000, 0, 6000, 6250.5]) }),
    maybe(0.5, { sim_zfw_kg: pick([60000, 61234.5]), planned_zfw_kg: pick([60000, 62000, 0]) }),
    maybe(0.5, { dep_metar: pick(METARS) }), maybe(0.4, { arr_metar: pick(METARS) }),
    maybe(0.4, { dep_metar_decoded: decoded() }), maybe(0.4, { arr_metar_decoded: decoded() }),
    maybe(0.4, { approach_glideslope_angle: pick([3, 2.5, 1.5, 8, 5.5]) }),
    maybe(0.5, { predicted_runway: pick(['25C', '07L', '']) }),
    maybe(0.5, { block_on_at: '2026-10-04T11:55:12Z', block_off_at: pick(['2026-10-04T08:00:00Z', '2026-10-04T11:30:00Z', '2026-10-05T01:00:00Z']) }),
    maybe(0.85, { live: live() }));
  return s;
}
function debrief() {
  if (rnd() < 0.15) return null;
  return Object.assign({}, maybe(0.9, { score_numeric: pick([0, 55, 87, 100, 93.5]) }), maybe(0.9, { score_label: pick(['smooth','acceptable','firm','hard','severe','Weird','']) }),
    maybe(0.9, { landing_rate_fpm: pick([-125, -455.5, -770, 0, -0.4]) }), maybe(0.6, { landing_scored_g_force: pick([1.05, 1.125, 1.8]) }),
    maybe(0.5, { landing_g_force: pick([1.3, 2.455]) }), maybe(0.7, { bounce_count: pick([0, 1, 3]) }),
    maybe(0.6, { runway_match: { runway_ident: pick(['25C', '', '07L']) } }));
}
function akt() {
  const r = rnd(); if (r < 0.25) return null;
  return Object.assign({ timestamp: pick(['2026-10-04T11:59:57Z', '2026-10-04T11:50:00Z', '2026-10-04T08:00:00Z', '2026-10-04T12:00:05Z', 'kaputt']),
    level: pick(['info', 'warn', 'error', 'WARN', '']),
    message: pick(['PIREP prefiled', 'Sim pausiert ▶ Fortsetzen', 'Verzögerung → Gate ✓ ° … 😀', 'x'.repeat(250), 'word '.repeat(60), 'Überlänge — ünïcode „Zitat“']) },
    maybe(0.5, { detail: pick(['EDDF > KJFK', 'd'.repeat(300), 'Größe']) }));
}
const out = [];
const T0 = Date.parse('2026-10-04T12:00:00Z');
const AKT = { timestamp: '2026-10-04T11:59:57Z', level: 'info', message: 'PIREP prefiled', detail: 'EDDF > KJFK' };
const LIVE_FL = { altitude_pressure_ft: 36000, altitude_msl_ft: 36100, altitude_agl_ft: 35000, ete_min: 125, vertical_speed_fpm: 0, gs_kt: 450 };
const BASE = { callsign: 'DLH 400', dpt_airport: 'EDDF', arr_airport: 'KJFK', connection_state: 'ok', queued_position_count: 0 };
const DEB = { score_numeric: 87, score_label: 'firm', landing_rate_fpm: -455.4, landing_scored_g_force: 1.234, bounce_count: 1, runway_match: { runway_ident: '25C' } };
const KURATIERT = [
  ['lage-getrennt', { verbunden: false, status: null, debrief: null, aktivitaet: AKT, jetzt_ms: T0, fehler_text: 'Zeitueberschreitung nach 3 s', fehler_seit_ms: T0 - 12400 }],
  ['lage-bereit', { verbunden: true, status: null, debrief: null, aktivitaet: AKT, jetzt_ms: T0 }],
  ['lage-pausiert', { verbunden: true, status: Object.assign({}, BASE, { phase: 'cruise', paused_since: '2026-10-04T11:55:00Z' }), jetzt_ms: T0 }],
  ['lage-unterwegs-boden', { verbunden: true, status: Object.assign({}, BASE, { phase: 'boarding', sim_fuel_kg: 5120.4, planned_block_fuel_kg: 5200, sim_zfw_kg: 60000, planned_zfw_kg: 70000,
      dep_metar_decoded: { icao: 'EDDF', wind_speed_kt: 12, wind_direction_deg: 240, visibility_m: 9999, cloud_layers: [{ cover: 'BKN', base_ft: 2500 }], temperature_c: 12, dewpoint_c: 5, qnh_hpa: 1018 } }), jetzt_ms: T0 }],
  ['lage-unterwegs-flug-ete', { verbunden: true, status: Object.assign({}, BASE, { phase: 'cruise', takeoff_at: '2026-10-04T10:30:00Z', live: LIVE_FL }), aktivitaet: AKT, jetzt_ms: T0 }],
  ['lage-unterwegs-flug-eta', { verbunden: true, status: Object.assign({}, BASE, { phase: 'cruise', takeoff_at: '2026-10-04T10:30:00Z', live: LIVE_FL }), jetzt_ms: T0 + 30000 }],
  ['lage-unterwegs-flug-flt', { verbunden: true, status: Object.assign({}, BASE, { phase: 'descent', takeoff_at: '2026-10-04T10:30:00Z', live: Object.assign({}, LIVE_FL, { altitude_msl_ft: 14000 }), arr_metar: 'KJFK 041051Z 18010KT 10SM FEW250 22/10 A3002 RMK AO2' }), jetzt_ms: T0 + 60000 }],
  ['lage-anflug', { verbunden: true, status: Object.assign({}, BASE, { phase: 'final', predicted_runway: '25C', live: { vertical_speed_fpm: -700, gs_kt: 140, ias_kt: 135.4, altitude_agl_ft: 1020.4, bank_deg: -1.25, headwind_kt: 12, crosswind_kt: 17.6 } }), jetzt_ms: T0 }],
  ['lage-auswertung', { verbunden: true, status: Object.assign({}, BASE, { phase: 'landing', landing_score_finalized: false }), jetzt_ms: T0 }],
  ['lage-ergebnis', { verbunden: true, status: Object.assign({}, BASE, { phase: 'landing', landing_score_finalized: true }), debrief: DEB, jetzt_ms: T0 }],
  ['lage-rollen', { verbunden: true, status: Object.assign({}, BASE, { phase: 'taxi_in', landing_score_finalized: true }), debrief: DEB, jetzt_ms: T0 }],
  ['lage-amstand', { verbunden: true, status: Object.assign({}, BASE, { phase: 'blocks_on', landing_score_finalized: true, block_on_at: '2026-10-04T11:55:12Z', block_off_at: '2026-10-04T08:50:00Z' }), debrief: DEB, jetzt_ms: T0 }],
  ['lage-eingereicht', { verbunden: true, status: Object.assign({}, BASE, { phase: 'pirep_submitted', landing_score_finalized: true }), debrief: DEB, jetzt_ms: T0 }],
  ['lage-eingereicht-wartet', { verbunden: true, status: Object.assign({}, BASE, { phase: 'pirep_submitted', landing_score_finalized: true, queued_position_count: 4 }), debrief: DEB, jetzt_ms: T0 }],
];
for (const [name, e] of KURATIERT) {
  const eg = Object.assign({ name, debrief: null, aktivitaet: null, host: HOST, port: PORT, fehler_text: null, fehler_seit_ms: null }, e);
  const rt = JSON.parse(JSON.stringify(eg));
  out.push({ name, eingabe: rt, erwartet: render(rt).text });
}
const BASIS = Date.parse('2026-10-04T12:00:00Z');
for (let i = 0; i < 300; i++) {
  const verbunden = rnd() < 0.93;
  const st = rnd() < 0.1 ? null : status();
  const e = { name: 'fuzz-' + i, verbunden, status: st, debrief: debrief(), aktivitaet: akt(),
    jetzt_ms: BASIS + Math.floor(rnd() * 3e6) - 5e5, host: HOST, port: PORT,
    fehler_text: maybe(0.7, pick(['Zeitueberschreitung nach 3 s', 'HTTP 500', 'Fehler äöü mit sehr langem Text ' + 'z'.repeat(60)])) || null,
    fehler_seit_ms: maybe(0.7, BASIS - Math.floor(rnd() * 600000)) || null };
  // JSON-Rundlauf: undefined-Felder verschwinden wie auf dem Draht
  const rt = JSON.parse(JSON.stringify(e));
  const r = render(rt);
  out.push({ name: e.name, eingabe: rt, erwartet: r.text });
}
process.stdout.write(JSON.stringify(out));
