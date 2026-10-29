// Flugkarte — zeigt eine Flugakte (Recorder: recorder/src/flugakte.ts) auf einer Karte.
//
// Derselbe Baustein laeuft in der Admin-Webapp und im AeroACARS-Client: Pilot
// und Admin sehen exakt dasselbe (Thomas, 28.10.2026). Darum ohne React, ohne
// Abhaengigkeit von der umgebenden Seite: `flugkarteAnzeigen(element, akte, …)`
// baut alles in `element` auf und liefert eine Aufraeumfunktion zurueck.
//
// Entstanden aus dem abgenommenen Entwurf (GSG-Repo _entwurf/quelle/vorlage2.html):
// 2D/3D, Ereignis-Marker mit Haufen (Ring + Zahl; Kreis bis 8, sonst Liste),
// Ereignisliste nach Phasen, Datenstreifen mit Konfigurationsbaendern,
// Wiedergabe 1x-120x, Verfolgen (Zoom/Drehen/Neigen bleiben beim Nutzer).
import maplibregl, { type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { MapboxOverlay } from "@deck.gl/mapbox";
import { IconLayer, LineLayer, PathLayer, ScatterplotLayer, SolidPolygonLayer, TextLayer } from "@deck.gl/layers";
import { WebMercatorViewport } from "@deck.gl/core";
import "./flugkarte.css";
import { FARBEN_DUNKEL, KATEGORIEN, type Flugakte, type Kategorie } from "./flugakteTypen";

/** Werte fuer einen Text; Zahlen formatiert der Baustein in der Sprache, bevor sie ankommen */
export type TextWerte = Record<string, string | number>;

export interface FlugkarteOptionen {
  /** Adresse des dunklen Kartenstils (Webapp: /admin/map-style-dark.json) */
  stilUrl: string;
  /**
   * Texte: Schluessel unter `flugkarte.*` in common.json (de/en/it, in Webapp und
   * AeroACARS gleich). Liefert null, wenn es den Schluessel nicht gibt — dann bleibt
   * der deutsche Text aus der Akte stehen.
   */
  t: (schluessel: string, werte?: TextWerte) => string | null;
  /** Sprache fuer Zahlen, z. B. „de“, „en“, „it“ */
  sprache: string;
  /** Datum fuer die Kopfzeile, z. B. „26.10.2026“ */
  datum?: string;
  /** Nur fuer Tests: Zugriff auf die MapLibre-Karte */
  beiKarte?: (karte: maplibregl.Map) => void;
}

const FT = 0.3048;
const SAT_URL = "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}";
const DEM_URL = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
/** Stiel vom Gate-Marker zum Schild (sonst schwebte das Schild in 3D frei in der Luft; Thomas 28.10.) */
const STIEL_SVG = "data:image/svg+xml;base64," + btoa('<svg xmlns="http://www.w3.org/2000/svg" width="4" height="18" viewBox="0 0 4 18"><rect x="1" y="0" width="2" height="18" fill="#ebf1f7" fill-opacity="0.75"/></svg>');
const PLANE_SVG = "data:image/svg+xml;base64," + btoa('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><path fill="#fff" d="M32 2c2.2 0 3.6 2.4 3.6 6v14.2l22.4 13.2v5.4l-22.4-7v13.4l7 5.2v4.4L32 54.6 21.4 56.8v-4.4l7-5.2V33.8l-22.4 7v-5.4l22.4-13.2V8c0-3.6 1.4-6 3.6-6z"/></svg>');

/* eslint-disable @typescript-eslint/no-explicit-any */
type R = Array<number | string | null>;

const hms = (t: string) => { const [h, m, s] = t.split(":").map(Number); return h * 3600 + m * 60 + s; };
const isoS = (t: string) => hms(t.slice(11, 19)) + Number("0." + (t.slice(20).replace("Z", "") || "0"));
const esc = (s: unknown) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]!));
const zahl = (v: unknown, ersatz = 0) => (typeof v === "number" && Number.isFinite(v) ? v : ersatz);
const reduziert = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Geruest der Karte; `u` liefert einen fertig escapten Text (flugkarte.ui.*) */
const geruest = (u: (k: string) => string) => `
<div class="fk-bar">
  <div class="fk-titel"><span data-fk="titel"></span><small data-fk="unter"></small></div>
  <div class="fk-grp fk-seg" role="group" aria-label="${u("ausschnitt")}">
    <button data-fit="all" aria-pressed="true">${u("fit_all")}</button><button data-fit="dep" aria-pressed="false">${u("fit_dep")}</button>
    <button data-fit="arr" aria-pressed="false">${u("fit_arr")}</button><button data-fit="gnd" aria-pressed="false">${u("fit_gnd")}</button></div>
  <div class="fk-grp fk-seg" role="group" aria-label="${u("ansicht")}">
    <button data-fk="v2d" aria-pressed="true">2D</button><button data-fk="v3d" aria-pressed="false">3D</button></div>
  <div class="fk-grp fk-seg"><button data-fk="seiteKnopf" aria-pressed="true" title="${u("seite_titel")}">${u("ereignisse")}</button></div>
</div>
<div class="fk-main">
  <div class="fk-kartenfeld">
    <div class="fk-karte" data-fk="karte"></div>
    <!-- Kartenebenen: selten gebraucht, darum als Knopf auf der Karte (wie in Karten-Apps) statt einer zweiten Leiste -->
    <div class="maplibregl-ctrl maplibregl-ctrl-group fk-ebenen" data-fk="ebenen">
      <button type="button" data-fk="ebenenKnopf" aria-expanded="false" aria-label="${u("ebenen")}" title="${u("ebenen")}"><svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><path d="M10 3 2 7l8 4 8-4-8-4Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="m2 10.5 8 4 8-4M2 14l8 4 8-4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg></button>
      <div class="fk-ebenen-feld" data-fk="ebenenFeld" hidden>
        <div class="fk-grp fk-seg" role="group" aria-label="${u("karte")}"><span class="fk-lbl">${u("karte")}</span>
          <button data-base="dark" aria-pressed="true">${u("karte")}</button><button data-base="sat" aria-pressed="false">${u("satellit")}</button><button data-fk="relief" aria-pressed="false">${u("relief")}</button></div>
        <div class="fk-grp fk-seg" role="group" aria-label="${u("linienfarbe")}"><span class="fk-lbl">${u("linie")}</span>
          <button data-col="alt" aria-pressed="true">${u("linie_hoehe")}</button><button data-col="gs" aria-pressed="false">${u("linie_tempo")}</button><button data-col="vs" aria-pressed="false">${u("linie_vs")}</button></div>
      </div>
    </div>
    <div class="fk-laden" data-fk="laden">${u("karte_laedt")}</div>
    <div class="fk-legende"><span data-fk="legT"></span><div class="fk-rampe" data-fk="legRampe"></div><div class="fk-enden"><span data-fk="legLo"></span><span data-fk="legHi"></span></div></div>
    <div class="fk-letztes" data-fk="letztes" aria-live="polite" hidden></div>
    <div class="fk-hud" data-fk="hud"></div>
    <div class="fk-flugzeug" data-fk="flugzeug" aria-hidden="true"><img alt="" src="${PLANE_SVG}"></div>
    <div class="fk-spinne" data-fk="spinne" hidden><svg aria-hidden="true"></svg></div>
    <div class="fk-pop" data-fk="pop" hidden></div>
  </div>
  <aside class="fk-seite" aria-label="${u("ereignisse")}">
    <div class="fk-filter">
      <div class="fk-zeile"><span class="fk-lbl">${u("ereignisse")} <span data-fk="anzahl"></span></span><span><button data-fk="alle">${u("alle")}</button> · <button data-fk="wichtig">${u("wichtigste")}</button> · <button data-fk="filterKnopf" aria-expanded="true">${u("filter")} <span aria-hidden="true">▾</span></button></span></div>
      <div class="fk-chips" data-fk="chips"></div>
      <p class="fk-nf-hinweis">${u("filter_hinweis")}</p>
    </div>
    <div class="fk-liste"><div class="fk-hinweis" data-fk="hinweis" hidden></div><ol data-fk="liste"></ol></div>
  </aside>
</div>
<div class="fk-streifen">
  <div class="fk-steuer">
    <button class="fk-play" data-fk="play">▶ ${u("abspielen")}</button>
    <div class="fk-grp fk-seg" role="group" aria-label="${u("tempo")}"><span class="fk-lbl">${u("tempo")}</span>
      <button data-spd="1" aria-pressed="false">1×</button><button data-spd="3" aria-pressed="false">3×</button><button data-spd="5" aria-pressed="false">5×</button><button data-spd="10" aria-pressed="true">10×</button><button data-spd="30" aria-pressed="false">30×</button><button data-spd="60" aria-pressed="false">60×</button><button data-spd="120" aria-pressed="false">120×</button></div>
    <label><input type="checkbox" data-fk="folgen"> ${u("kamera_folgt")}</label>
    <span class="fk-jetzt" data-fk="jetzt"></span>
    <button class="fk-klapp" data-fk="streifenKnopf" aria-expanded="true">${u("datenstreifen")} <span aria-hidden="true">▾</span></button>
  </div>
  <canvas class="fk-diagramm" data-fk="diagramm" aria-label="${u("diagramm")}"></canvas>
  <div class="fk-bandtipp" data-fk="bandtipp" hidden></div>
</div>`;

/**
 * 3D-Hoehe ueber Grund (ft) je Spurpunkt [lat, lon, alt, agl, …]. Fehlt die Hoehe ueber Grund, gilt das zuletzt
 * gemessene Gelaende — und VOR dem ersten Messwert das erste gemessene (Codex 28.10.: sonst lag ein fortgesetzter
 * Flug, der in 5 000 ft ohne AGL beginnt, erst am Boden und sprang dann hoch). Ganz ohne AGL: Gelaende aus dem
 * ersten Punkt am Boden, sonst Meereshoehe.
 */
export function hoeheUeberGrund(track: Array<Array<number | string | null>>): number[] {
  const z = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : 0);
  const erster = track.find((r) => r[2] != null && r[3] != null);
  const amBoden = track.find((r) => r[12] === 1 && r[2] != null);
  let gelaende = erster ? z(erster[2]) - z(erster[3]) : amBoden ? z(amBoden[2]) : 0;
  return track.map((r) => { if (r[2] != null && r[3] != null) gelaende = z(r[2]) - z(r[3]); return Math.max(0, z(r[2]) - gelaende); });
}

/** Baut die Flugkarte in `root` auf. Rueckgabe: Aufraeumen (Karte, Zeitgeber, Ereignisse). */
export function flugkarteAnzeigen(root: HTMLElement, akte: Flugakte, opt: FlugkarteOptionen): () => void {
  // ---------- Sprache ----------
  const LOC = ({ de: "de-DE", en: "en-GB", it: "it-IT" } as Record<string, string>)[opt.sprache.slice(0, 2)] ?? "de-DE";
  const fmt = (n: unknown, d = 0) => (n == null || typeof n !== "number" || isNaN(n) ? "–" : n.toLocaleString(LOC, { maximumFractionDigits: d, minimumFractionDigits: d }));
  const zahlenFormatiert = (w: TextWerte) => Object.fromEntries(Object.entries(w).map(([k, v]) => [k, typeof v === "number" ? v.toLocaleString(LOC, { maximumFractionDigits: 2 }) : v]));
  /** Text unter flugkarte.<k>; fehlt er, steht der Schluessel da (der Paritaetstest verhindert das) */
  const TX = (k: string, w?: TextWerte) => opt.t("flugkarte." + k, w && zahlenFormatiert(w)) ?? k;
  const U = (k: string, w?: TextWerte) => TX("ui." + k, w);
  /** Text eines Ereignisses in der Sprache der Anzeige; ohne passenden Text bleibt der deutsche aus der Akte */
  const ereignisText = (e: Flugakte["events"][number]) => {
    const w = (e.werte ?? {}) as TextWerte;
    if (e.code === "ansage") return opt.t(`flugkarte.slot.${w.slot}`) ?? e.txt;
    if (e.code === "phase") return opt.t(`flugkarte.phase.${w.phase}`) ?? e.txt;
    if (!e.code) return e.txt;
    const v = { ...w };
    if (typeof v.regel === "string") v.was = opt.t(`flugkarte.regel.${v.regel}`) ?? v.regel;
    return opt.t(`flugkarte.ev.${e.code}`, zahlenFormatiert(v)) ?? e.txt;
  };

  root.classList.add("fk");
  root.innerHTML = geruest((k) => esc(U(k)));
  const $ = <T extends HTMLElement = HTMLElement>(n: string) => root.querySelector(`[data-fk="${n}"]`) as T;
  const alle = (q: string) => Array.from(root.querySelectorAll<HTMLElement>(q));
  let beendet = false;
  const aufraeumen: Array<() => void> = [];

  const T: R[] = akte.track;
  if (!T.length) {
    root.innerHTML = `<div class="fk-laden">${esc(U("keine_positionen"))}</div>`;
    return () => { root.innerHTML = ""; };
  }
  // Landeauswertung kommt mit roher Laenge (168°), die Spur ueber die Datumsgrenze fortlaufend (−191°):
  // auf dieselbe Weltkopie wie das Spurende legen, sonst liegt die Bahn eine Welt daneben
  const L = akte.land && typeof akte.land.lon === "number"
    ? { ...akte.land, lon: akte.land.lon + 360 * Math.round(((T[T.length - 1][1] as number) - akte.land.lon) / 360) }
    : akte.land;
  const MESSBAR = akte.messbar ?? {};
  const KLAPPEN_PROZENT = akte.klappenArt === "stellung";
  const kopf = akte.kopf ?? ({} as Flugakte["kopf"]);
  const alt = (r: R) => zahl(r[2]), agl = (r: R) => zahl(r[3]);
  // 3D-Hoehe ueber Grund (ft): die Grundkarte liegt flach auf Meereshoehe. Mit der Hoehe ueber dem Meer schwebte am
  // Boden alles um die Platzhoehe ueber der Karte (Vilnius 636 ft; Thomas 28.10.) — und manche Add-ons melden am
  // Boden sogar Unsinn (X-Plane in Helsinki −21 ft). Fehlt die Hoehe ueber Grund, gilt das zuletzt gemessene Gelaende.
  const HOEHE = hoeheUeberGrund(T);

  // Zeitachse mit Datumswechsel
  const S: number[] = [];
  { let add = 0, prev = -1; T.forEach((r) => { let s = hms(String(r[11])) + add; if (s < prev - 3600) { add += 86400; s += 86400; } S.push(s); prev = s; }); }
  const t0 = S[0];
  const fixDay = (s: number) => (s < t0 - 3600 ? s + 86400 : s);
  const idxAt = (s: number) => { let lo = 0, hi = S.length - 1; while (lo < hi) { const m = (lo + hi + 1) >> 1; if (S[m] <= s) lo = m; else hi = m - 1; } return lo; };
  const clock = (s: number) => { s = ((Math.round(s) % 86400) + 86400) % 86400; return [(s / 3600) | 0, ((s % 3600) / 60) | 0, s % 60].map((v) => String(v).padStart(2, "0")).join(":"); };

  // Kategorien je Karte (Filterzustand gehoert dieser Karte)
  const CATS: Record<string, Kategorie & { on: boolean; c: string; n: string; how: string }> = Object.fromEntries(Object.entries(KATEGORIEN).map(([k, c]) =>
    [k, { ...c, on: c.an, c: TX(`kat.${k}.c`), n: TX(`kat.${k}.n`), how: TX(`kat.${k}.how`) }]));
  if (KLAPPEN_PROZENT) CATS.flaps.how = TX("kat.flaps.how_stellung");

  // altFt: Hoehe ueber dem Meer (Anzeige); h: Hoehe ueber Grund (3D-Lage)
  type Ev = { n: number; s: number; i: number; lat: number; lon: number; altFt: number; h: number; txt: string; k: string; t: string; ph?: string; [k: string]: any };
  const E: Ev[] = akte.events.filter((e) => CATS[e.k]).map((e, n) => {
    const s = fixDay(isoS(e.t));
    const i = Math.min(e.i ?? idxAt(s), T.length - 1);
    const r = T[i];
    const altFt = e.k === "td" ? alt(r) - agl(r) : alt(r);
    // ph: Phasen-Code (Pushback, Approach …) — die Ausschnitte suchen danach, nicht nach dem uebersetzten Namen
    const ph = e.k === "phase" ? String((e.werte as TextWerte | undefined)?.phase ?? e.txt) : undefined;
    return { ...e, n, s, i, lat: (e.lat as number) ?? (r[0] as number), lon: (e.lon as number) ?? (r[1] as number), altFt, h: e.k === "td" ? 0 : HOEHE[i], txt: ereignisText(e), ph };
  });

  // Bahn aus den Aufsetzdaten (fehlt die Landeauswertung, entfaellt sie)
  const mPerLat = 110574, mPerLon = (lat: number) => 111320 * Math.cos((lat * Math.PI) / 180);
  const off = (lat: number, lon: number, east: number, north: number) => [lon + east / mPerLon(lat), lat + north / mPerLat];
  const HAT_BAHN = !!(L && L.lat != null && L.runway_true_course_deg != null && L.runway_length_m && L.td_distance_from_threshold_m != null);
  const endR = T[T.length - 1];
  let thrLL = [endR[1] as number, endR[0] as number];
  let RWY: number[][] = [];
  if (HAT_BAHN && L) {
    const crs = ((L.runway_true_course_deg as number) * Math.PI) / 180, ux = Math.sin(crs), uy = Math.cos(crs);
    const d = L.td_distance_from_threshold_m as number;
    thrLL = off(L.lat as number, L.lon as number, -ux * d, -uy * d);
    const endLL = off(thrLL[1], thrLL[0], ux * (L.runway_length_m as number), uy * (L.runway_length_m as number));
    const w2 = ((L.runway_width_m as number) || 45) / 2;
    const corner = (p: number[], sg: number) => { const q = off(p[1], p[0], -uy * w2 * sg, ux * w2 * sg); return [q[0], q[1], 0]; }; // Bahn liegt auf der Karte (3D-Hoehe ueber Grund)
    RWY = [corner(thrLL, 1), corner(endLL, 1), corner(endLL, -1), corner(thrLL, -1)];
  }
  const RWY_DATA = HAT_BAHN ? [RWY] : [], RWY_RAND = HAT_BAHN ? [[...RWY, RWY[0]]] : [];
  const BAHN_SCHILD = HAT_BAHN && L ? [{ lon: thrLL[0], lat: thrLL[1], t: `${L.runway_match_icao ?? kopf.arr_airport ?? ""} ${L.runway ?? ""}` }] : [];

  // Kopf
  $("titel").textContent = `${kopf.airline_icao ?? ""} ${kopf.flight_number ?? ""} · ${kopf.dpt_airport ?? "?"} → ${kopf.arr_airport ?? "?"}`;
  const sim = akte.flugzeug?.sim === "Msfs2024" ? "MSFS 2024" : akte.flugzeug?.sim === "Msfs2020" ? "MSFS 2020" : akte.flugzeug?.sim ?? "";
  // X-Plane liefert als Titel oft einen Beschreibungssatz — dann lieber den ICAO-Typ
  const titel = akte.flugzeug?.titel && akte.flugzeug.titel.length <= 40 ? akte.flugzeug.titel : akte.flugzeug?.icao ?? akte.flugzeug?.titel;
  $("unter").textContent = [titel, sim, opt.datum, akte.dep_gate ? U("gate", { gate: akte.dep_gate }) : null].filter(Boolean).join(" · ");

  // Farben
  const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const mix = (stops: string[], f: number) => { f = Math.max(0, Math.min(1, f)); const p = f * (stops.length - 1), i = Math.min(Math.floor(p), stops.length - 2), u = p - i; const a = hex(stops[i]), b = hex(stops[i + 1]); return a.map((v, k) => Math.round(v + (b[k] - v) * u)); };
  const RAMP = ["#22d3ee", "#3b82f6", "#8b5cf6", "#ec4899", "#f97316", "#facc15"];
  const VSR = ["#38bdf8", "#7dd3fc", "#e2e8f0", "#fdba74", "#f97316"];
  const maxAlt = Math.max(1, ...T.map(alt)), maxGs = Math.max(1, ...T.map((r) => zahl(r[4])));
  const COLOR: Record<string, { f: (r: R) => number[]; t: string; lo: string; hi: string; stops: string[] }> = {
    alt: { f: (r) => mix(RAMP, alt(r) / maxAlt), t: U("leg_hoehe"), lo: "0", hi: fmt(Math.round(maxAlt / 1000) * 1000), stops: RAMP },
    gs: { f: (r) => mix(RAMP, zahl(r[4]) / maxGs), t: U("leg_gs"), lo: "0", hi: fmt(maxGs), stops: RAMP },
    vs: { f: (r) => mix(VSR, (zahl(r[6]) + 2500) / 5000), t: U("leg_vs"), lo: "−" + fmt(2500), hi: "+" + fmt(2500), stops: VSR },
  };
  let colMode = "alt";
  const rgb = (a: number[]) => `rgb(${a.join(",")})`;
  function legend() {
    const c = COLOR[colMode];
    $("legT").textContent = c.t; $("legLo").textContent = c.lo; $("legHi").textContent = c.hi;
    $("legRampe").style.background = `linear-gradient(90deg,${c.stops.join(",")})`;
  }

  // Zustand
  const TD_S = L?.t ? fixDay(isoS(String(L.t))) : null;
  let scrubS = S[0], sel: number | null = null, playing = false, speed = 10, follow = false, is3d = false, base = "dark", relief = false;
  let range = [S[0], S[S.length - 1]];

  function stateAt(s: number) {
    const i = idxAt(s), j = Math.min(i + 1, T.length - 1);
    const u = S[j] > S[i] ? Math.max(0, Math.min(1, (s - S[i]) / (S[j] - S[i]))) : 0;
    const a = T[i], b = T[j], lerp = (k: number) => zahl(a[k]) + (zahl(b[k], zahl(a[k])) - zahl(a[k])) * u;
    const dh = b[7] == null ? 0 : ((zahl(b[7]) - zahl(a[7]) + 540) % 360) - 180; // fehlt der naechste Kurs: den letzten halten, nicht gegen 0° drehen
    const h = HOEHE[i] + (HOEHE[j] - HOEHE[i]) * u;
    return { i, lat: lerp(0), lon: lerp(1), alt: lerp(2), agl: lerp(3), h, gs: lerp(4), ias: lerp(5), vs: lerp(6), hdg: (zahl(a[7]) + dh * u + 360) % 360, fuel: lerp(10), r: a, fehlt: (k: number) => a[k] == null };
  }

  // ---------- Karte ----------
  const allB = T.reduce<number[]>((b, r) => [Math.min(b[0], r[1] as number), Math.min(b[1], r[0] as number), Math.max(b[2], r[1] as number), Math.max(b[3], r[0] as number)], [Infinity, Infinity, -Infinity, -Infinity]); // nicht ±180: fortlaufende Laengen ueber die Datumsgrenze
  // renderWorldCopies: false sperrt die Mitte am Weltrand — Fluege in Ostasien/Pazifik lagen dann am Kartenrand (CES 442, 28.10.)
  const map = new maplibregl.Map({
    container: $("karte"), attributionControl: false, renderWorldCopies: true, maxPitch: 78, minZoom: 0.5, maxZoom: 19, dragRotate: true,
    style: { version: 8, sources: {}, layers: [{ id: "bg", type: "background", paint: { "background-color": "#0b121b" } }] },
    bounds: [[allB[0], allB[1]], [allB[2], allB[3]]], fitBoundsOptions: { padding: 70 },
    // MapLibres eigene Knopftexte, sonst englisch auch auf Deutsch und Italienisch (Knopf-Pruefung 28.10.)
    locale: { "NavigationControl.ZoomIn": U("zoom_rein"), "NavigationControl.ZoomOut": U("zoom_raus"), "NavigationControl.ResetBearing": U("norden"),
      "AttributionControl.ToggleAttribution": U("quellen"), "Map.Title": U("karte") },
  });
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "top-right");
  const ebenenEl = $("ebenen");
  map.addControl({ onAdd: () => ebenenEl, onRemove: () => ebenenEl.remove() }, "top-right");
  map.addControl(new maplibregl.AttributionControl({ compact: true }), "bottom-right");
  opt.beiKarte?.(map);

  let overlay: MapboxOverlay | null = null;
  const VEC: Array<[string, string]> = [];
  map.on("error", (e: any) => console.warn("Flugkarte:", e?.error?.message ?? e));
  // Frist ab Kartenstart (nicht erst nach dem Stilabruf): kommt die Grundkarte nicht, sagt der Hinweis das
  const ladeUhr = setTimeout(() => { const l = root.querySelector('[data-fk="laden"]'); if (l) l.textContent = U("karte_laedt_nicht"); }, 12000);
  aufraeumen.push(() => clearTimeout(ladeUhr));
  map.on("load", async () => {
    // Kartenstil der Seite (dunkle OpenFreeMap-Karte) mit etwas mehr Kontrast fuer die Flugspur
    let stil: StyleSpecification | null = null;
    try { stil = await (await fetch(opt.stilUrl)).json(); } catch { /* ohne Grundkarte weiter */ }
    if (beendet) return;
    let ersteSchrift: string | undefined;
    if (stil) {
      if (stil.glyphs) map.setGlyphs(stil.glyphs);
      if (stil.sprite && typeof stil.sprite === "string") map.setSprite(stil.sprite);
      for (const [id, q] of Object.entries(stil.sources)) if (!map.getSource(id)) map.addSource(id, q as any);
      for (const l of stil.layers) {
        if (l.type === "background") continue;
        const lay: any = { ...l, id: "v:" + l.id };
        const f = FARBEN_DUNKEL[l.id];
        if (f) lay.paint = { ...(l as any).paint, ...f };
        if (l.id.startsWith("place_")) lay.paint = { ...(lay.paint ?? {}), "text-color": "#c3cfdb", "text-halo-color": "rgba(10,16,24,0.92)", "text-halo-width": 1.4 };
        if (lay.paint) for (const k of Object.keys(lay.paint)) if (k.endsWith("-pattern")) delete lay.paint[k];
        map.addLayer(lay);
        VEC.push([lay.id, l.type]);
        if (!ersteSchrift && l.type === "symbol") ersteSchrift = lay.id;
      }
    }
    map.addSource("sat", { type: "raster", tiles: [SAT_URL], tileSize: 256, maxzoom: 19, attribution: "Esri World Imagery (Esri, Maxar, Earthstar Geographics)" });
    map.addLayer({ id: "sat", type: "raster", source: "sat", layout: { visibility: "none" }, paint: { "raster-fade-duration": 150, "raster-saturation": -0.15 } }, ersteSchrift);
    map.addSource("dem", { type: "raster-dem", tiles: [DEM_URL], tileSize: 256, encoding: "terrarium", maxzoom: 12, attribution: "Relief: Mapzen Terrain Tiles, AWS Open Data" });
    map.addLayer({ id: "relief", type: "hillshade", source: "dem", layout: { visibility: "none" }, paint: { "hillshade-exaggeration": 0.55, "hillshade-shadow-color": "#000000", "hillshade-highlight-color": "rgba(255,255,255,.35)", "hillshade-accent-color": "#000000" } }, ersteSchrift);
    // eigene Zeichenflaeche statt mit MapLibre verschraenkt: verschraenkt zeichnet deck die Spur mit MapLibres
    // Kameramatrix, Flugzeugsymbol, Kreise und Klicks rechnen aber mit deck — in 3D lag das Flugzeug dann neben
    // bzw. unter der Spur (bei 5 000 ft und naher Kamera ~20 px; Thomas 28.10.). So rechnet alles mit einer Projektion.
    overlay = new MapboxOverlay({ interleaved: false, layers: layers(), getTooltip: tip as any, onClick: (info: any) => { if (!info.object) { haufenZu(); select(null); } } });
    map.addControl(overlay as any);
    // Hinweis weg, sobald die sichtbaren Kacheln da sind (nicht „idle“: das kommt bei laufender Wiedergabe nie);
    // kommt die Grundkarte nicht, sagt der Hinweis das — die Flugspur bleibt nutzbar
    const laden = $("laden");
    const fertig = () => { laden?.remove(); clearTimeout(ladeUhr); map.off("data", beiDaten); };
    const beiDaten = () => { if (map.areTilesLoaded()) fertig(); };
    map.on("data", beiDaten);
    map.once("idle", fertig);
    beiDaten();
    // den schon gewaehlten Ausschnitt behalten (Klick waehrend „Karte wird geladen“, Pruefung 29.10.)
    map.resize(); fit(fitKey, false);
    map.getContainer().querySelector(".maplibregl-ctrl-attrib")?.classList.remove("maplibregl-compact-show");
  });
  map.on("rotate", () => refresh());
  map.on("moveend", () => {
    // Auswahl, die erst nach dem Hinfliegen in einem Haufen liegt: Haufen oeffnen, damit sie sichtbar ist
    if (sel != null && !offen && !playing && overlay) {
      haufenBilden();
      const g = grpOf.get(sel);
      if (g && g.m.length > 1 && g.art !== "zoom") { haufenAuf(g); if (g.art === "liste") select(sel); } // Liste: Popup mit Rueckweg neu aufbauen
      else if (g && g.art === "zoom" && nachZoomen < 3) {
        // liegt noch mit weiter entfernten Ereignissen zusammen: naeher heran, bis der Haufen allein steht
        nachZoomen++;
        const b = g.m.reduce((b, e) => b.extend([e.lon, e.lat]), new maplibregl.LngLatBounds([g.lead.lon, g.lead.lat], [g.lead.lon, g.lead.lat]));
        map.fitBounds(b, { padding: 120, maxZoom: 18, bearing: map.getBearing(), pitch: map.getPitch(), duration: reduziert() ? 0 : 500 });
      }
    }
    refresh();
  });
  map.on("zoom", () => refresh());
  map.on("pitch", () => refresh());
  map.on("zoomstart", () => haufenZu());
  map.on("move", () => { if (is3d || map.getPitch() > 1 || welt() !== weltGezeichnet) refresh(); placePop(); placePlane(); });

  // ---------- deck.gl-Ebenen ----------
  const SEG: number[] = []; for (let i = 1; i < T.length; i++) SEG.push(i);
  const Z = (m: number) => (is3d ? m : 0); // 2D flach: sonst verschiebt die Perspektive hohe Punkte beim Zoomen
  const pathPos = (i: number) => [T[i][1] as number, T[i][0] as number, Z(HOEHE[i] * FT)];
  const groundPos = (i: number) => [T[i][1] as number, T[i][0] as number, 0];
  const WALL = T.map((_r, i) => i).filter((i) => agl(T[i]) > 30);
  const SCHATTEN = [T.map((_r, i) => i)];
  const visible = () => E.filter((e) => CATS[e.k].on);
  const depthOff = { depthCompare: "always" } as any;

  // Haufen: Marker naeher als ein Durchmesser werden einer (Ring + Zahl). Klick: verteilt → zoomen,
  // bis 8 an einer Stelle → Kreis, mehr → Liste.
  const HAUFEN_PX = 22, SPINNE_MAX = 8, VERTEILT_M = 40;
  const PRIO = ["gate", "td", "to", "ga", "exit", "warn", "sim"];
  const SONDER = ["td", "to", "ga", "exit", "gate"];
  const prio = (e: Ev) => { const i = PRIO.indexOf(e.k); return i < 0 ? PRIO.length : i; };
  const geoM = (a: Ev, b: Ev) => Math.hypot((a.lat - b.lat) * 110574, (a.lon - b.lon) * mPerLon(a.lat));
  type Haufen = { x: number; y: number; lead: Ev; m: Ev[]; art?: string; id?: number; sig?: string };
  let zStufe: number | null = null, haufenKey: string | null = null, haufenKamera: string | null = null, haufen: Haufen[] = [];
  let grpOf = new Map<number, Haufen>(), offen: Haufen | null = null, markerCache: any = { key: null };
  let gesteAktiv = false, gesteEnde = 0, userRotated = false, userZoomed = false;
  function haufenBilden() {
    const vis = visible();
    if (zStufe == null || Math.abs(map.getZoom() - zStufe) >= 0.15) zStufe = map.getZoom(); // Hysterese, kein Springen
    const c = map.getCenter(), zq = zStufe.toFixed(3);
    const kamera = is3d || map.getPitch() > 1 ? [zq, Math.round(map.getBearing() / 3), Math.round(map.getPitch() / 3), c.lng.toFixed(3), c.lat.toFixed(3)].join(",") : zq;
    if (!(playing && follow) || gesteAktiv || !haufenKamera) haufenKamera = kamera; // nur die Kamerafahrt des Verfolgens friert ein
    const key = [haufenKamera, is3d, Object.values(CATS).map((c) => +c.on).join("")].join("|");
    if (key === haufenKey) return;
    haufenKey = key;
    const vp = viewport();
    const gruppen: Haufen[] = [];
    for (const e of [...vis].sort((a, b) => prio(a) - prio(b) || a.s - b.s)) {
      const [x, y] = vp.project([e.lon, e.lat, Z(e.h * FT)]);
      const g = gruppen.find((g) => Math.hypot(g.x - x, g.y - y) < HAUFEN_PX);
      if (g) g.m.push(e); else gruppen.push({ x, y, lead: e, m: [e] });
    }
    grpOf = new Map();
    for (const g of gruppen) {
      g.m.sort((a, b) => a.s - b.s);
      const weite = Math.max(...g.m.map((e) => geoM(e, g.lead)));
      g.art = g.m.length === 1 ? "einzeln" : weite > VERTEILT_M ? "zoom" : g.m.length <= SPINNE_MAX ? "spinne" : "liste";
      g.id = g.lead.n; g.sig = g.m.map((e) => e.n).join(",");
      g.m.forEach((e) => grpOf.set(e.n, g));
    }
    haufen = gruppen;
    if (offen) { const neu = haufen.find((g) => g.sig === offen!.sig && g.art === offen!.art); if (neu) offen = neu; else haufenZu(); }
  }

  // Weltkopie: MapLibre dreht die Kartenmitte immer auf ±180° zurück, die Spur ueber die Datumsgrenze
  // liegt aber fortlaufend (z. B. −191°). deck.gl zeichnet im Verbund keine Kopien — also die Daten in
  // die Weltkopie verschieben, in der die Kartenmitte liegt (sonst fehlte die Spur in der Landeansicht).
  const datenMitte = (allB[0] + allB[2]) / 2;
  const welt = () => 360 * Math.round((map.getCenter().lng - datenMitte) / 360);
  let weltGezeichnet = 0;
  let weltMatrix: number[] | null = null;
  function weltProps() {
    const w = welt();
    if (w !== weltGezeichnet || (w && !weltMatrix)) { weltGezeichnet = w; weltMatrix = w ? [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, w, 0, 0, 1] : null; }
    return { modelMatrix: weltMatrix as any }; // deck nimmt ein Zahlenfeld, der Typ verlangt Matrix4Like
  }
  function layers() {
    const st = stateAt(scrubS);
    const ev = visible();
    haufenBilden();
    const mk = [haufenKey, offen?.sig ?? "", offen?.art ?? "", map.getZoom() >= 9].join("#");
    if (markerCache.key !== mk) {
      const zu = haufen.filter((g) => !(offen && offen.art === "spinne" && g.id === offen.id));
      const evDraw = zu.map((g) => g.lead).sort((a, b) => +SONDER.includes(a.k) - +SONDER.includes(b.k));
      const mehrere = zu.filter((g) => g.m.length > 1);
      markerCache = {
        key: mk, evDraw, mehrere, ringe: mehrere.filter((g) => g.lead.k !== "td"), mitte: offen && offen.art === "spinne" ? [offen.lead] : [],
        tdDraw: evDraw.filter((e) => e.k === "td"),
        gates: map.getZoom() >= 9 ? ev.filter((e) => e.k === "gate" && !(offen && offen.art === "spinne" && offen.m.includes(e))) : [],
      };
    }
    const { evDraw, mehrere, ringe, mitte, tdDraw, gates } = markerCache;
    const fruehest = (e: Ev) => grpOf.get(e.n)?.m[0].s ?? e.s;
    const vorbei = E.reduce((n, e) => n + +(e.s <= scrubS + 0.5), 0);
    const L3 = is3d;
    const groesse = (k: string) => (SONDER.includes(k) ? 13 : 10.5);
    const W = weltProps();
    return [
      new SolidPolygonLayer({ ...W, id: "rwy", data: RWY_DATA, getPolygon: (d: any) => d.map((p: number[]) => [p[0], p[1], Z(p[2])]), updateTriggers: { getPolygon: is3d }, getFillColor: base === "sat" ? [255, 255, 255, 40] : [120, 140, 165, 120], _full3d: true }),
      new PathLayer({ ...W, id: "rwy-edge", data: RWY_RAND, getPath: (d: any) => d.map((p: number[]) => [p[0], p[1], Z(p[2])]), updateTriggers: { getPath: is3d }, getColor: [200, 215, 230, 200], getWidth: 1.5, widthUnits: "pixels" }),
      // Spur, Rand, Schatten und Vorhang ohne Tiefenvergleich, die Reihenfolge entscheidet: Rand und Spur liegen auf
      // derselben Hoehe, mit Tiefentest gewann je Bild mal der eine, mal der andere — die Spur flimmerte und zerfiel
      // in Striche, sobald sich die Kamera bewegte (Kamera folgt, 3D; Thomas 28.10.)
      new LineLayer({ ...W, id: "wall", data: L3 ? WALL : [], getSourcePosition: (i: number) => groundPos(i) as any, getTargetPosition: (i: number) => pathPos(i) as any, getColor: (i: number) => [...COLOR[colMode].f(T[i]), 46] as any, getWidth: 1.4, widthUnits: "pixels", updateTriggers: { getColor: colMode }, parameters: depthOff }),
      new PathLayer({ ...W, id: "shadow", data: L3 ? SCHATTEN : [], getPath: (d: number[]) => d.map(groundPos) as any, getColor: [0, 0, 0, 120], getWidth: 3, widthUnits: "pixels", jointRounded: true, parameters: depthOff }),
      new PathLayer({ ...W, id: "track-halo", data: SEG, getPath: (i: number) => [pathPos(i - 1), pathPos(i)] as any, getColor: [6, 10, 16, 200], getWidth: 7, widthUnits: "pixels", capRounded: true, jointRounded: true, updateTriggers: { getPath: is3d }, parameters: depthOff }),
      new PathLayer({ ...W, id: "track", data: SEG, getPath: (i: number) => [pathPos(i - 1), pathPos(i)] as any, getColor: (i: number) => COLOR[colMode].f(T[i]) as any, getWidth: 4, widthUnits: "pixels", capRounded: true, jointRounded: true, updateTriggers: { getColor: colMode, getPath: is3d }, parameters: depthOff }),
      new LineLayer({ ...W, id: "plane-drop", data: L3 && st.h > 30 ? [st] : [], getSourcePosition: (d: any) => [d.lon, d.lat, 0], getTargetPosition: (d: any) => [d.lon, d.lat, d.h * FT], getColor: [255, 255, 255, 150], getWidth: 1.5, widthUnits: "pixels" }),
      new ScatterplotLayer({ ...W, id: "td-ring", data: tdDraw, getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], updateTriggers: { getPosition: is3d }, getRadius: 20, radiusUnits: "pixels", stroked: true, filled: false, getLineColor: [255, 255, 255, 230], lineWidthUnits: "pixels", getLineWidth: 2, billboard: true, parameters: depthOff }),
      // weicher Ring unter gestapelten Markern (der TD-Marker hat seinen eigenen)
      new ScatterplotLayer({ ...W, id: "haufen-ring", data: ringe, getPosition: (g: Haufen) => [g.lead.lon, g.lead.lat, Z(g.lead.h * FT)], updateTriggers: { getPosition: is3d },
        getRadius: (g: Haufen) => (SONDER.includes(g.lead.k) ? 17 : 14.5), radiusUnits: "pixels", filled: true, getFillColor: [230, 238, 245, 40], stroked: true,
        getLineColor: [230, 238, 245, 150], getLineWidth: 1.5, lineWidthUnits: "pixels", billboard: true, parameters: depthOff }),
      new ScatterplotLayer({ ...W, id: "marks", data: evDraw, pickable: true, autoHighlight: true, highlightColor: [255, 255, 255, 90],
        getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], getRadius: (e: Ev) => groesse(e.k), radiusUnits: "pixels",
        getFillColor: (e: Ev) => [...hex(CATS[e.k].col), fruehest(e) > scrubS + 0.5 ? 120 : 255] as any, stroked: true,
        getLineColor: (e: Ev) => (sel === e.n ? [255, 255, 255, 255] : [8, 13, 20, 255]), getLineWidth: (e: Ev) => (sel === e.n ? 3 : 1.5), lineWidthUnits: "pixels",
        billboard: true, parameters: depthOff, onClick: (info: any) => { markerKlick(info.object.n); return true; },
        updateTriggers: { getLineColor: sel, getLineWidth: sel, getPosition: is3d, getFillColor: vorbei } }),
      new TextLayer({ ...W, id: "codes", data: evDraw, getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], getText: (e: Ev) => CATS[e.k].c, getSize: (e: Ev) => (SONDER.includes(e.k) ? 10 : 9),
        getColor: (e: Ev) => (e.k === "warn" ? [255, 255, 255] : [8, 13, 20]).concat(fruehest(e) > scrubS + 0.5 ? 150 : 255) as any, updateTriggers: { getPosition: is3d, getColor: vorbei },
        fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontWeight: 700, characterSet: "auto", billboard: true, getTextAnchor: "middle", getAlignmentBaseline: "center", parameters: depthOff }),
      new ScatterplotLayer({ ...W, id: "haufen-mitte", data: mitte, getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], updateTriggers: { getPosition: is3d },
        getRadius: 4, radiusUnits: "pixels", getFillColor: [230, 238, 245], stroked: true, getLineColor: [8, 13, 20], getLineWidth: 1.5, lineWidthUnits: "pixels", billboard: true, parameters: depthOff }),
      new TextLayer({ ...W, id: "haufen-zahl", data: mehrere, getPosition: (g: Haufen) => [g.lead.lon, g.lead.lat, Z(g.lead.h * FT)], updateTriggers: { getPosition: is3d },
        getText: (g: Haufen) => String(g.m.length), getSize: 10, getColor: [255, 255, 255], getPixelOffset: [12, -11], getTextAnchor: "middle", getAlignmentBaseline: "center",
        background: true, getBackgroundColor: [37, 99, 235, 255], backgroundPadding: [4, 2, 4, 2], backgroundBorderRadius: 8, getBorderColor: [11, 18, 27, 255], getBorderWidth: 2,
        fontFamily: '"JetBrains Mono", monospace', fontWeight: 700, characterSet: "auto", parameters: depthOff } as any),
      // Stiel: vom Rand des Gate-Markers (13 px) bis zur Unterkante des Schilds (30 px), in Bildpunkten — steht in 2D und 3D
      new IconLayer({ ...W, id: "gate-stiele", data: gates, getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], updateTriggers: { getPosition: is3d },
        getIcon: () => ({ url: STIEL_SVG, width: 4, height: 18, anchorY: 18 }), getSize: 18, sizeUnits: "pixels", getPixelOffset: [0, -12], billboard: true, parameters: depthOff }),
      new TextLayer({ ...W, id: "gate-schilder", data: gates, getPosition: (e: Ev) => [e.lon, e.lat, Z(e.h * FT)], updateTriggers: { getPosition: is3d },
        getText: (e: Ev) => `${e.dir === "dep" ? kopf.dpt_airport ?? "" : kopf.arr_airport ?? ""} · ${U("gate", { gate: e.gate })}  ${e.t.slice(11, 16)}`, getSize: 12, getColor: [235, 241, 247],
        getTextAnchor: "middle", getAlignmentBaseline: "bottom", getPixelOffset: [0, -30], background: true, getBackgroundColor: [11, 18, 27, 230], backgroundPadding: [6, 3],
        getBorderColor: [255, 255, 255, 90], getBorderWidth: 1, fontFamily: '"JetBrains Mono", monospace', fontWeight: 700, characterSet: "auto", parameters: depthOff }),
      new TextLayer({ ...W, id: "labels", data: BAHN_SCHILD, getPosition: (d: any) => [d.lon, d.lat, 0], updateTriggers: { getPosition: is3d }, getText: (d: any) => d.t, getSize: 12,
        getColor: [230, 238, 245], getPixelOffset: [0, 22], fontFamily: '"JetBrains Mono", monospace', fontWeight: 700, outlineWidth: 4, outlineColor: [8, 13, 20, 230], fontSettings: { sdf: true }, characterSet: "auto", parameters: depthOff }),
    ];
  }
  const TIP_STIL = { background: "#142131", color: "#e6eef6", fontFamily: "system-ui, sans-serif", fontSize: "12px", padding: "5px 8px", borderRadius: "6px", border: "1px solid #2e4156" };
  function tip({ object, layer }: any) {
    if (!object || layer?.id !== "marks") return null;
    const g = grpOf.get(object.n);
    if (g && g.m.length > 1 && offen?.id !== g.id) return { html: `<b>${esc(U("tip_haufen", { n: g.m.length }))}</b> · ${esc(U(g.art === "zoom" ? "tip_zoom" : "tip_auf"))}`, style: TIP_STIL };
    return { html: `<b>${esc(object.t.slice(11, 19))}</b> · ${esc(object.txt)}`, style: TIP_STIL };
  }

  // Flugzeugsymbol: die Nase zeigt in den gemessenen Kurs, auf den Bildschirm projiziert (auch in 3D richtig).
  // Nicht die Bewegungsrichtung: beim Pushback rollt das Flugzeug rueckwaerts (Thomas 28.10.).
  // Nur ohne gemessenen Kurs ersatzweise die Bewegungsrichtung.
  const planeEl = $("flugzeug");
  function placePlane() {
    const vp = viewport(), a = stateAt(scrubS);
    const p0 = vp.project([a.lon, a.lat, Z(a.h * FT)]);
    let deg = a.hdg - map.getBearing();
    if (!a.fehlt(7)) {
      const h = (a.hdg * Math.PI) / 180, [qLon, qLat] = off(a.lat, a.lon, Math.sin(h) * 50, Math.cos(h) * 50);
      const p1 = vp.project([qLon, qLat, Z(a.h * FT)]);
      deg = (Math.atan2(p1[0] - p0[0], -(p1[1] - p0[1])) * 180) / Math.PI;
    } else {
      // erst voraus schauen, am Spurende (nichts mehr voraus) die Bewegung davor
      for (const dt of [4, 10, 25, -4, -10, -25]) {
        const b = stateAt(Math.max(S[0], Math.min(scrubS + dt, S[S.length - 1])));
        const p1 = vp.project([b.lon, b.lat, Z(b.h * FT)]), dx = (p1[0] - p0[0]) * Math.sign(dt), dy = (p1[1] - p0[1]) * Math.sign(dt);
        if (Math.hypot(dx, dy) > 3) { deg = (Math.atan2(dx, -dy) * 180) / Math.PI; break; }
      }
    }
    planeEl.style.transform = `translate(${p0[0]}px, ${p0[1]}px) translate(-50%, -50%) rotate(${deg}deg)`;
  }
  let rq = 0;
  function refresh() {
    if (rq || beendet) return;
    rq = requestAnimationFrame(() => { rq = 0; if (beendet) return; overlay?.setProps({ layers: layers() }); placePlane(); hud(); lastEvent(); drawChart(); placePop(); listMark(); });
  }

  // ---------- Haufen oeffnen ----------
  const spinneEl = $("spinne"), pop = $("pop");
  let popArt: string | null = null;
  function markerKlick(n: number) {
    const g = grpOf.get(n);
    if (!g || g.m.length === 1) { select(n, false); return; }
    if (offen?.id === g.id) { haufenZu(); select(null); return; }
    if (g.art === "zoom") {
      haufenZu();
      const b = g.m.reduce((b, e) => b.extend([e.lon, e.lat]), new maplibregl.LngLatBounds([g.lead.lon, g.lead.lat], [g.lead.lon, g.lead.lat]));
      map.fitBounds(b, { padding: 120, maxZoom: 18, bearing: map.getBearing(), pitch: map.getPitch(), duration: reduziert() ? 0 : 600 });
      return;
    }
    haufenAuf(g);
    if (g.art === "liste") listeZeigen(); else select(null);
  }
  // Kaesten ueber der Karte (Datenanzeige, letztes Ereignis, Legende, Kartenknoepfe) in Kartenkoordinaten
  function kastenFlaechen() {
    const cv = map.getCanvas().getBoundingClientRect();
    return [hudEl, lastEl, root.querySelector<HTMLElement>(".fk-legende"), root.querySelector<HTMLElement>(".maplibregl-ctrl-top-right"), root.querySelector<HTMLElement>(".maplibregl-ctrl-bottom-right")]
      // auf dem Handy stehen Datenanzeige und letztes Ereignis unter der Karte (nicht absolut) und zaehlen nicht
      .filter((k): k is HTMLElement => !!k && !k.hidden && k.offsetHeight > 0 && (k.className.includes("maplibregl-ctrl") || getComputedStyle(k).position === "absolute"))
      .map((k) => { const r = k.getBoundingClientRect(); return [r.left - cv.left, r.top - cv.top, r.right - cv.left, r.bottom - cv.top]; });
  }
  const ueberdeckung = (kaesten: number[][], l: number, t: number, w: number, h: number) =>
    kaesten.reduce((k, [a, b, c, d]) => k + Math.max(0, Math.min(l + w, c) - Math.max(l, a)) * Math.max(0, Math.min(t + h, d) - Math.max(t, b)), 0);
  // Ein aufgefaecherter Kreis darf keinen Kasten ueberdecken: liegt er am Rand oder unter einem Kasten,
  // fahren wir die Karte so, dass er an einer freien Stelle steht
  function kreisFreiStellen(g: Haufen) {
    if (follow) return;
    const W = map.getCanvas().clientWidth, H = map.getCanvas().clientHeight;
    // die Ecke oben links bleibt frei: dort weicht das Popup aus, wenn neben dem Kreis kein Platz ist
    const kaesten = [...kastenFlaechen(), [0, 0, (pop.offsetWidth || 270) + 16, (pop.offsetHeight || 240) + 16]];
    const r = (g.m.length <= 4 ? 44 : 56) + 22;
    const frei = (x: number, y: number) => x - r >= 0 && y - r >= 0 && x + r <= W && y + r <= H && ueberdeckung(kaesten, x - r, y - r, 2 * r, 2 * r) === 0;
    const [cx, cy] = viewport().project([g.lead.lon, g.lead.lat, Z(g.lead.h * FT)]);
    if (frei(cx, cy)) return;
    for (const fy of [0.5, 0.35, 0.65, 0.25, 0.75, 0.2, 0.8]) for (const fx of [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8, 0.1, 0.9]) {
      const x = W * fx, y = H * fy;
      if (frei(x, y)) { map.easeTo({ center: [g.lead.lon, g.lead.lat], offset: [x - W / 2, y - H / 2], duration: reduziert() ? 0 : 500 }); return; }
    }
  }
  function haufenAuf(g: Haufen) {
    offen = g;
    if (g.art === "spinne") {
      spinneEl.innerHTML = '<svg aria-hidden="true"></svg>' + g.m.map((e) => `<button class="fk-smark${sel === e.n ? " fk-sel" : ""}" data-n="${e.n}" style="background:${CATS[e.k].col};${e.k === "warn" ? "color:#fff" : ""}" title="${esc(e.t.slice(11, 19))} · ${esc(e.txt)}" aria-label="${esc(e.t.slice(11, 19))} ${esc(e.txt)}">${esc(CATS[e.k].c)}</button>`).join("");
      spinneEl.hidden = false;
      kreisFreiStellen(g);
    }
    spinneSetzen(); refresh();
  }
  function haufenZu() {
    if (!offen) return;
    offen = null; spinneEl.hidden = true; spinneEl.innerHTML = '<svg aria-hidden="true"></svg>';
    if (popArt === "liste") { pop.hidden = true; popArt = null; }
    refresh();
  }
  spinneEl.addEventListener("click", (ev) => { const b = (ev.target as HTMLElement).closest<HTMLElement>(".fk-smark"); if (b) select(+b.dataset.n!, false); });
  function spinneOrt(e: Ev) {
    const g = offen!, i = g.m.indexOf(e), n = g.m.length;
    const [cx, cy] = viewport().project([g.lead.lon, g.lead.lat, Z(g.lead.h * FT)]);
    const r = n <= 4 ? 44 : 56, w = -Math.PI / 2 + (i * 2 * Math.PI) / n, rand = r + 22;
    const W = map.getCanvas().clientWidth, H = map.getCanvas().clientHeight;
    const mx = Math.max(rand, Math.min(W - rand, cx)), my = Math.max(rand, Math.min(H - rand, cy)); // Kreis nach innen schieben
    return [mx + r * Math.cos(w), my + r * Math.sin(w), cx, cy, mx, my];
  }
  function spinneSetzen() {
    if (!offen || offen.art !== "spinne") return;
    // liegt der Haufen ausserhalb des Bildes, nicht an den Rand gedrueckt zeigen
    const [hx, hy] = viewport().project([offen.lead.lon, offen.lead.lat, Z(offen.lead.h * FT)]);
    spinneEl.style.visibility = hx < 0 || hy < 0 || hx > map.getCanvas().clientWidth || hy > map.getCanvas().clientHeight ? "hidden" : "";
    let linien = "";
    for (const e of offen.m) {
      const [x, y, cx, cy, mx, my] = spinneOrt(e);
      const b = spinneEl.querySelector<HTMLElement>(`[data-n="${e.n}"]`);
      if (b) { b.style.left = x + "px"; b.style.top = y + "px"; b.classList.toggle("fk-sel", sel === e.n); }
      linien += `<line x1="${mx}" y1="${my}" x2="${x}" y2="${y}"/>`;
      if (mx !== cx || my !== cy) linien += `<line class="fk-zum-ort" x1="${cx}" y1="${cy}" x2="${mx}" y2="${my}"/>`;
    }
    spinneEl.querySelector("svg")!.innerHTML = linien;
  }
  function listeZeigen() {
    const g = offen!; sel = null; popArt = "liste";
    pop.innerHTML = `<h4><span>${esc(U("liste_titel", { n: g.m.length }))}</span><button aria-label="${esc(U("schliessen"))}">×</button></h4><ol class="fk-poplist">${g.m.map((e) => `<li><button data-n="${e.n}"><span class="fk-t">${esc(e.t.slice(11, 19))}</span><span class="fk-punkt" style="background:${CATS[e.k].col};${e.k === "warn" ? "color:#fff" : ""}">${esc(CATS[e.k].c)}</span><span>${esc(e.txt)}</span></button></li>`).join("")}</ol>`;
    pop.querySelector<HTMLElement>("h4 button")!.onclick = () => { haufenZu(); select(null); };
    pop.querySelectorAll<HTMLElement>(".fk-poplist button").forEach((b) => (b.onclick = () => select(+b.dataset.n!, false)));
    pop.hidden = false; refresh(); renderList();
  }

  // ---------- Popup ----------
  function viewport() {
    const c = map.getCenter();
    // Mitte in die Welt der Daten zurueckrechnen (siehe welt()), sonst stehen Flugzeug und Marker eine Welt daneben
    return new WebMercatorViewport({ width: map.getCanvas().clientWidth, height: map.getCanvas().clientHeight, longitude: c.lng - welt(), latitude: c.lat, zoom: map.getZoom(), pitch: map.getPitch(), bearing: map.getBearing() });
  }
  function placePop() {
    spinneSetzen();
    if (pop.hidden || (sel == null && popArt !== "liste")) return;
    let x: number, y: number;
    const aufHaufen = () => viewport().project([offen!.lead.lon, offen!.lead.lat, Z(offen!.lead.h * FT)]);
    if (popArt === "liste") [x, y] = aufHaufen();
    else {
      const e = E[sel!];
      if (offen && offen.m.includes(e)) [x, y] = offen.art === "spinne" ? spinneOrt(e) : aufHaufen();
      else [x, y] = viewport().project([e.lon, e.lat, Z(e.h * FT)]);
    }
    if (matchMedia("(max-width: 640px)").matches) { pop.style.visibility = "visible"; return; } // Handy: steht im Fluss unter der Karte
    const W = map.getCanvas().clientWidth, H = map.getCanvas().clientHeight, pw = pop.offsetWidth, ph = pop.offsetHeight;
    // Popup nie ueber einen anderen Kasten (Datenanzeige, letztes Ereignis, Legende, Kartenknoepfe):
    // oben, unten, links, rechts vom Marker probieren und die Stelle mit der kleinsten Ueberdeckung nehmen
    // dazu die aufgefaecherten Marker: das Popup soll den Kreis nicht zudecken
    const cvR = map.getCanvas().getBoundingClientRect();
    const marker = spinneEl.hidden ? [] : Array.from(spinneEl.querySelectorAll<HTMLElement>(".fk-smark")).map((m) => { const r = m.getBoundingClientRect(); return [r.left - cvR.left, r.top - cvR.top, r.right - cvR.left, r.bottom - cvR.top]; });
    const kaesten = [...kastenFlaechen(), ...marker];
    const klemm = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
    const lagen: Array<[number, number, string]> = [
      [klemm(x - pw / 2, 8, W - pw - 8), y - ph - 18, "oben"],
      [klemm(x - pw / 2, 8, W - pw - 8), y + 18, "unten"],
      [x - pw - 18, klemm(y - ph / 2, 8, H - ph - 8), "links"],
      [x + 18, klemm(y - ph / 2, 8, H - ph - 8), "rechts"],
      [8, 8, "ecke"], // Ausweichplatz: oben links ist sonst nichts — lieber ohne Pfeil als ueber einem Kasten
    ];
    const kosten = ([l, t]: [number, number, string]) => {
      const k = ueberdeckung(kaesten, l, t, pw, ph);
      const raus = Math.max(0, 8 - l) + Math.max(0, l + pw - W + 8) + Math.max(0, 8 - t) + Math.max(0, t + ph - H + 8);
      return k + raus * 1000;
    };
    const [l, t, wo] = lagen.find((lage) => kosten(lage) === 0) ?? lagen.reduce((best, lage) => (kosten(lage) < kosten(best) ? lage : best));
    pop.dataset.lage = wo;
    pop.style.left = l + "px"; pop.style.top = t + "px";
    pop.style.visibility = y < -10 || y > H + 10 || x < -10 || x > W + 10 ? "hidden" : "visible"; // Ort ausserhalb des Bildes
  }
  let nachZoomen = 0;
  function select(n: number | null, fly = false) {
    sel = n; popArt = n == null ? null : "ereignis";
    if (fly) nachZoomen = 0;
    if (n == null) { pop.hidden = true; refresh(); renderList(); return; }
    const g = grpOf.get(n);
    if (g && g.m.length > 1 && g.art !== "zoom" && offen?.id !== g.id) haufenAuf(g);
    else if (offen && !offen.m.includes(E[n])) haufenZu();
    const e = E[n], r = T[e.i];
    scrubS = e.s;
    // Beschriftungen (links) sind rohe Texte und werden unten escapt, die Werte (rechts) sind schon fertiges HTML
    let rows: Array<[string, string]> = [[U("zeit_utc"), e.t.slice(11, 19)], [U("hoehe"), fmt(e.altFt) + " ft"], [U("ueber_grund"), fmt(r[3]) + " ft"], ["IAS", fmt(r[5]) + " kt"], [U("gs_lang"), fmt(r[4]) + " kt"], ["V/S", fmt(r[6]) + " fpm"]];
    if (e.k === "pa") rows.splice(1, 0, [U("dauer"), fmt(e.dauer) + " s"], [U("sprachen"), esc((e.sprachen ?? []).join(", "))]);
    if (e.k === "td" && L && e.lat === L.lat) rows = [[U("zeit_utc"), String(L.t ?? e.t).slice(11, 22)], [U("bahn"), esc(L.runway ?? "–")], [U("hinter_schwelle"), fmt(L.td_distance_from_threshold_m) + " m"], [U("mittellinie"), fmt(L.runway_match_centerline_offset_m, 1) + " m"], [U("sinkrate"), fmt(L.vs) + " fpm"], ["G", fmt(L.g, 2)], [U("note"), esc(L.score_grade ?? "–")]];
    if (e.k === "exit" && L) rows.push([U("ausrollstrecke"), fmt(L.rollout_distance_m) + " m"]);
    if (e.k === "gate") rows = [[U(e.dir === "dep" ? "off_block" : "on_block"), e.t.slice(11, 19)], [U("flughafen"), esc(e.dir === "dep" ? kopf.dpt_airport : kopf.arr_airport)], [U("gate_feld"), esc(e.gate)]];
    if (e.anzahl) rows.push([U("wechsel"), esc(U("wechsel_wert", { n: Number(e.anzahl), bis: String(e.bis ?? "").slice(11, 16) }))]);
    const zurueck = offen?.art === "liste" && offen.m.includes(e) ? `<button class="fk-zurueck" type="button">${esc(U("zurueck", { n: offen.m.length }))}</button>` : "";
    pop.innerHTML = `${zurueck}<h4><span>${esc(CATS[e.k].n)}</span><button aria-label="${esc(U("schliessen"))}">×</button></h4><p class="fk-was">${esc(e.txt)}</p><dl>${rows.map((q) => `<dt>${esc(q[0])}</dt><dd>${q[1]}</dd>`).join("")}</dl>`;
    pop.querySelector<HTMLElement>("h4 button")!.onclick = () => (zurueck ? listeZeigen() : select(null));
    pop.querySelector(".fk-zurueck")?.addEventListener("click", listeZeigen);
    pop.hidden = false;
    if (follow) chase();
    else if (fly && !(offen && offen.m.includes(e))) map.easeTo({ center: [e.lon, e.lat], zoom: Math.max(map.getZoom(), e.altFt < 4000 ? 12.5 : 9.5), duration: reduziert() ? 0 : 700 });
    refresh(); renderList();
    // Handy: das Popup steht unter der Karte und muss ins Bild — die Liste danach nicht wieder hinscrollen
    if (matchMedia("(max-width: 640px)").matches) pop.scrollIntoView({ block: "nearest" });
    else root.querySelector(`.fk-liste [data-n="${n}"]`)?.scrollIntoView({ block: "nearest" });
  }

  // ---------- Anzeige ----------
  const hudEl = $("hud");
  let hudKey: number | null = null;
  // HUD-Beschriftungen einmal holen (das HUD wird jede Sekunde neu gesetzt); Werte fuer innerHTML schon escapt
  const HUD = { nm: esc(U("nicht_messbar_flugzeug")), hoehe: esc(U("hoehe")), agl: esc(U("ueber_grund")), kurs: esc(U("kurs")), sprit: esc(U("sprit")), klappen: U("hud_klappen"), fahrwerk: U("hud_fahrwerk") };
  function hud() {
    const k = Math.floor(scrubS); if (k === hudKey) return; hudKey = k;
    const st = stateAt(scrubS), r = st.r;
    const b = (lab: string, val: string) => `<div><small>${lab}</small><b>${val}</b></div>`;
    const wert = (k2: number, txt: string) => (st.fehlt(k2) ? "–" : txt);
    // Konfiguration: an / aus / nicht messbar (gestrichelt)
    const c = (k2: number, an: boolean, txt: string, col: string) => r[k2] == null ? `<span class="fk-nm" title="${HUD.nm}">${esc(txt)}</span>` : `<span class="${an ? "fk-an" : ""}" style="${an ? `background:${col}` : ""}">${esc(txt)}</span>`;
    const kl = r[13] as number | null;
    hudEl.innerHTML = b("UTC", clock(scrubS)) + b(HUD.hoehe, wert(2, fmt(st.alt) + " ft")) + b("IAS", wert(5, fmt(st.ias) + " kt")) + b("V/S", wert(6, fmt(st.vs) + " fpm"))
      + b(HUD.agl, wert(3, fmt(st.agl) + " ft")) + b("GS", wert(4, fmt(st.gs) + " kt")) + b(HUD.kurs, wert(7, fmt(st.hdg) + "°")) + b(HUD.sprit, wert(10, fmt(st.fuel) + " kg"))
      + `<div class="fk-cfg">${c(13, (kl ?? 0) > 0, KLAPPEN_PROZENT ? `${HUD.klappen} ${kl ?? "–"} %` : `${HUD.klappen} ${kl ?? "–"}`, CATS.flaps.col)}${c(14, r[14] === 1, HUD.fahrwerk, CATS.gear.col)}${c(15, zahl(r[15]) > SB_RUHE + 10, `SPD BRK ${r[15] ?? "–"} %`, CATS.spoiler.col)}${c(16, r[16] === 1, "AP", CATS.ap.col)}${c(17, r[17] === 1, "A/THR", CATS.ap.col)}${c(18, r[18] === 1, "LDG LT", CATS.light.col)}</div>`;
    $("jetzt").textContent = U("jetzt", { zeit: clock(scrubS), min: Math.round((scrubS - S[0]) / 60) });
  }

  // ---------- Letztes Ereignis ----------
  const lastEl = $("letztes");
  let lastShown: number | null = null;
  function lastEvent() {
    let best: Ev | null = null;
    for (const e of E) { if (e.s > scrubS + 0.5) break; if (CATS[e.k].on && e.k !== "phase") best = e; }
    if (!best) { lastEl.hidden = true; lastShown = null; return; }
    if (best.n !== lastShown) {
      lastShown = best.n;
      lastEl.innerHTML = `<span class="fk-punkt" style="background:${CATS[best.k].col};${best.k === "warn" ? "color:#fff" : ""}">${esc(CATS[best.k].c)}</span><b>${esc(best.txt)}</b><small>${esc(best.t.slice(11, 19))} UTC · ${fmt(best.altFt)} ft</small>`;
      lastEl.classList.remove("fk-neu"); void lastEl.offsetWidth; lastEl.classList.add("fk-neu");
    }
    lastEl.hidden = false;
    const hs = getComputedStyle(hudEl);
    if (hs.position === "absolute") {
      lastEl.style.bottom = hudEl.offsetHeight + parseFloat(hs.bottom) + 8 + "px";
      // niedrige Karte: stoesst der Kasten an die Kartenknoepfe oben rechts, rueckt er links daneben
      lastEl.style.right = "";
      const k = root.querySelector<HTMLElement>(".maplibregl-ctrl-top-right")?.getBoundingClientRect(), l = lastEl.getBoundingClientRect();
      if (k && l.top < k.bottom + 6 && l.right > k.left - 6) lastEl.style.right = 12 + k.width + 10 + "px";
    }
  }

  // ---------- Ereignisliste ----------
  const list = $("liste");
  let listKey: number | null = null;
  function renderList() {
    const out: string[] = [];
    E.forEach((e) => {
      if (e.k === "phase") { out.push(`<li class="fk-ph"><span>${esc(e.txt)}</span><span>${esc(e.t.slice(11, 16))}</span></li>`); return; }
      if (!CATS[e.k].on) return;
      const r = T[e.i];
      const sub = e.k === "pa" ? `${fmt(e.dauer)} s · ${fmt(e.altFt)} ft` : `${fmt(e.altFt)} ft · ${fmt(r[5])} kt`; // nur Zahlen und Einheiten
      out.push(`<li><button data-n="${e.n}" class="${sel === e.n ? "fk-sel" : ""}"><span class="fk-t">${esc(e.t.slice(11, 19))}</span><span class="fk-punkt" style="background:${CATS[e.k].col};${e.k === "warn" ? "color:#fff" : ""}">${esc(CATS[e.k].c)}</span><span>${esc(e.txt)}<small>${sub}</small></span></button></li>`);
    });
    list.innerHTML = out.join(""); listKey = null;
    $("anzahl").textContent = U("anzahl", { n: visible().filter((e) => e.k !== "phase").length, m: E.filter((e) => e.k !== "phase").length });
    listMark();
  }
  list.addEventListener("click", (ev) => { const b = (ev.target as HTMLElement).closest<HTMLElement>("button[data-n]"); if (b) select(+b.dataset.n!, true); });
  function listMark() {
    const k = E.reduce((n, e) => n + +(e.s <= scrubS + 0.5), 0); if (k === listKey) return; listKey = k;
    list.querySelectorAll<HTMLElement>("button[data-n]").forEach((b) => b.classList.toggle("fk-fut", E[+b.dataset.n!].s > scrubS + 0.5));
  }
  // Hinweis, was dieses Flugzeug nicht liefert (statt einer stummen Null)
  const nichtMessbar = Object.entries(MESSBAR).filter(([k, v]) => v === false && CATS[k]).map(([k]) => CATS[k].n);
  const hinweis: string[] = [];
  if (nichtMessbar.length) hinweis.push(U("hinweis_nm", { liste: nichtMessbar.join(", ") }));
  if (KLAPPEN_PROZENT) hinweis.push(U("hinweis_klappen_prozent"));
  if (hinweis.length) { $("hinweis").textContent = hinweis.join(" "); $("hinweis").hidden = false; }

  // ---------- Filter ----------
  const DEF = Object.fromEntries(Object.entries(CATS).map(([k, c]) => [k, c.an]));
  const chipsEl = $("chips");
  function renderChips() {
    chipsEl.innerHTML = Object.entries(CATS).map(([k, c]) => {
      const n = E.filter((e) => e.k === k).length;
      if (MESSBAR[k] === false) return `<span class="fk-chip fk-nm" title="${esc(U("nicht_messbar_flugzeug"))}"><i>${esc(c.c)}</i>${esc(c.n)} <em>–</em></span>`;
      if (!n) return "";
      return `<button class="fk-chip" data-k="${k}" aria-pressed="${c.on}" title="${esc(c.how)}"><i style="background:${c.col};${k === "warn" ? "color:#fff" : ""}">${esc(c.c)}</i>${esc(c.n)} <em>${n}</em></button>`;
    }).join("");
  }
  chipsEl.addEventListener("click", (ev) => {
    const b = (ev.target as HTMLElement).closest<HTMLElement>("button.fk-chip"); if (!b) return;
    haufenZu(); const c = CATS[b.dataset.k!]; c.on = !c.on;
    if (sel != null && E[sel].k === b.dataset.k && !c.on) select(null);
    renderChips(); renderList(); refresh();
  });
  $("alle").onclick = () => { haufenZu(); Object.values(CATS).forEach((c) => (c.on = true)); renderChips(); renderList(); refresh(); };
  $("wichtig").onclick = () => { haufenZu(); Object.entries(CATS).forEach(([k, c]) => (c.on = DEF[k])); renderChips(); renderList(); refresh(); };

  // ---------- Datenstreifen ----------
  const cv = $<HTMLCanvasElement>("diagramm"), cx = cv.getContext("2d")!;
  const klBreite = KLAPPEN_PROZENT ? 100 : Math.max(1, ...T.map((r) => zahl(r[13])));
  // Speedbrake-Ruhestellung kommt vom Recorder (eine Quelle mit den Ereignissen; 777 in X-Plane: 60 %) —
  // sonst waere das Band den ganzen Flug rot. Nur fuer alte Akten ohne Feld: dieselbe Regel auf der Spur.
  const SB_RUHE = typeof akte.sbRuhe === "number" ? akte.sbRuhe : (() => {
    const z = new Map<number, number>(); let n = 0;
    for (const r of T) { if (r[12] !== 0 || r[15] == null) continue; const v = Math.round(zahl(r[15]) / 5) * 5; z.set(v, (z.get(v) ?? 0) + 1); n++; }
    const [wert, anzahl] = [...z.entries()].sort((a, b) => b[1] - a[1])[0] ?? [0, 0];
    return n && anzahl / n >= 0.8 ? wert : 0;
  })();
  const STRIPS: Array<[string, (r: R) => boolean, string, (r: R) => number]> = [
    [U(KLAPPEN_PROZENT ? "band_klappen_prozent" : "band_klappen"), (r) => zahl(r[13]) > 0, CATS.flaps.col, (r) => Math.min(1, 0.25 + zahl(r[13]) / klBreite)],
    [U("band_fahrwerk"), (r) => r[14] === 1, CATS.gear.col, () => 1],
    [U("band_speedbrakes"), (r) => zahl(r[15]) > SB_RUHE + 10, CATS.spoiler.col, (r) => Math.min(1, 0.35 + (zahl(r[15]) - SB_RUHE) / 100)],
    [U("band_autopilot"), (r) => r[16] === 1, CATS.ap.col, () => 1],
    [U("band_autothrust"), (r) => r[17] === 1, "#c4b5fd", () => 1],
    [U("band_landelicht"), (r) => r[18] === 1, CATS.light.col, () => 1],
  ];
  const STRIP_FELD = [13, 14, 15, 16, 17, 18];
  const G = { L: 92, R: 46, top: 20, altH: 104, sH: 15, gap: 3 };
  // je Band die zusammenhaengenden Abschnitte des gezeichneten Ausschnitts (fuer Zeichnen und Maus-Hinweis)
  let baender: Array<Array<{ s0: number; s1: number; wert: string }>> = [];
  let CW = 0, CH = 0;
  // Zeitachse mit Stauchung: lange ruhige Abschnitte (kein Band aendert sich, z. B. Reiseflug) werden schmal,
  // damit Abflug und Landung mit ihren kurzen Klappen-/Fahrwerk-/Speedbrake-Phasen lesbar breit sind.
  // STUFEN: Stuetzpunkte [Zeit, x]; dazwischen linear. LUECKEN: gestauchte Abschnitte fuer das Bruchzeichen.
  let STUFEN: number[][] = [], LUECKEN: number[][] = [];
  function zeitachse() {
    const w0 = G.L, W = CW - G.L - G.R, [r0, r1] = range, span = r1 - r0;
    STUFEN = [[r0, w0], [r1, w0 + W]]; LUECKEN = [];
    if (span < 2 * 3600 || W < 200) return;
    const wechsel: number[] = [r0];
    let vorher = "";
    for (let i = idxAt(r0); i <= idxAt(r1); i++) {
      const q = T[i], key = STRIPS.map(([, on], k) => (q[STRIP_FELD[k]] == null ? "n" : on(q) ? (k === 0 ? String(q[13]) : "1") : "0")).join("|") + q[12];
      if (key !== vorher) { wechsel.push(S[i]); vorher = key; }
    }
    wechsel.push(r1);
    const RAND = 300, MIN = Math.max(1200, span * 0.1), BREIT = 56; // 5 min Kontext an jedem Rand, nur Ruhe ab 20 min
    const luecken: number[][] = [];
    for (let j = 1; j < wechsel.length; j++) if (wechsel[j] - wechsel[j - 1] > MIN) luecken.push([wechsel[j - 1] + RAND, wechsel[j] - RAND]);
    if (!luecken.length) return;
    const gestaucht = luecken.reduce((a, [a0, a1]) => a + a1 - a0, 0), breiteL = Math.min(BREIT, (W * 0.25) / luecken.length);
    const pxProS = (W - luecken.length * breiteL) / (span - gestaucht);
    let s = r0, x = w0; STUFEN = [[s, x]];
    for (const [a0, a1] of luecken) { x += (a0 - s) * pxProS; STUFEN.push([a0, x]); x += breiteL; STUFEN.push([a1, x]); s = a1; }
    STUFEN.push([r1, x + (r1 - s) * pxProS]);
    LUECKEN = luecken;
  }
  const abbilden = (v: number, von: 0 | 1) => {
    const nach = 1 - von, P = STUFEN;
    if (P.length < 2) return 0;
    let j = 1; while (j < P.length - 1 && v > P[j][von]) j++;
    const a = P[j - 1], b = P[j], d = b[von] - a[von] || 1;
    return a[nach] + ((v - a[von]) / d) * (b[nach] - a[nach]);
  };
  const xOf = (s: number) => abbilden(s, 0);
  const sOf = (x: number) => abbilden(x, 1);
  const puffer = document.createElement("canvas"), px = puffer.getContext("2d")!;
  let chartKey: string | null = null, chartYA: ((a: number) => number) | null = null, chartYAx = 0;
  function drawChart() {
    const r = cv.getBoundingClientRect(), d = devicePixelRatio || 1;
    if (!r.width) return;
    if (cv.width !== Math.round(r.width * d) || cv.height !== Math.round(r.height * d)) { cv.width = Math.round(r.width * d); cv.height = Math.round(r.height * d); }
    CW = r.width; CH = r.height;
    G.altH = Math.max(40, CH - (G.top + 10 + STRIPS.length * (G.sH + G.gap) + 22)); // Baender und Zeitachse fest, der Hoehenteil nimmt den Rest
    // linker Rand so breit wie die laengste Bandbeschriftung (englisch/italienisch laenger als deutsch), mindestens 92 px
    cx.font = '11px "JetBrains Mono", monospace';
    G.L = Math.max(92, Math.ceil(Math.max(...STRIPS.map(([name]) => cx.measureText(name).width))) + 12);
    const key = [range[0], range[1], cv.width, cv.height, colMode, Object.values(CATS).map((c) => +c.on).join(""), sel].join("|");
    if (key === chartKey && chartYA) {
      cx.setTransform(1, 0, 0, 1, 0, 0); cx.clearRect(0, 0, cv.width, cv.height); cx.drawImage(puffer, 0, 0);
      cx.setTransform(d, 0, 0, d, 0, 0); zeitregler(chartYA, chartYAx); return;
    }
    cx.setTransform(d, 0, 0, d, 0, 0); cx.clearRect(0, 0, CW, CH);
    zeitachse();
    const a0 = idxAt(range[0]), a1 = Math.min(idxAt(range[1]) + 1, T.length - 1);
    const top = Math.max(...T.slice(a0, a1 + 1).map(alt)) * 1.08 || 1000;
    const iTop = Math.max(...T.slice(a0, a1 + 1).map((q) => zahl(q[5]))) * 1.1 || 100;
    const yA = (a: number) => G.top + (1 - a / top) * G.altH, yI = (v: number) => G.top + (1 - v / iTop) * G.altH;
    cx.font = '11px "JetBrains Mono", monospace'; cx.textBaseline = "middle";
    // Beschriftung nie enger als 14 px — sonst liegen die Zahlen bei flachem Streifen uebereinander
    const stepA = [500, 1000, 2000, 5000, 10000, 20000].find((st) => (st / top) * G.altH >= 14) ?? 50000;
    const stepI = [100, 200, 500].find((st) => (st / iTop) * G.altH >= 14) ?? 1000;
    cx.strokeStyle = "#1b2939"; cx.lineWidth = 1; cx.fillStyle = "#9fb0c2"; cx.textAlign = "right";
    for (let a = 0; a <= top; a += stepA) { const y = yA(a); cx.beginPath(); cx.moveTo(G.L, y); cx.lineTo(CW - G.R, y); cx.stroke(); cx.fillText(a >= 1000 ? fmt(a / 1000) + "k ft" : fmt(a) + " ft", G.L - 6, y); }
    cx.textAlign = "left"; cx.fillStyle = "#c9d4df";
    for (let v = stepI; v <= iTop; v += stepI) cx.fillText(fmt(v) + " kt", CW - G.R + 6, yI(v));
    cx.save(); cx.beginPath(); cx.rect(G.L, 0, CW - G.L - G.R, CH); cx.clip();
    // Hoehe: fehlende Werte unterbrechen die Linie statt eine 0 zu zeichnen
    cx.beginPath(); cx.moveTo(xOf(S[a0]), yA(0));
    for (let i = a0; i <= a1; i++) if (T[i][2] != null) cx.lineTo(xOf(S[i]), yA(alt(T[i])));
    cx.lineTo(xOf(S[a1]), yA(0)); cx.closePath();
    const gr = cx.createLinearGradient(0, G.top, 0, G.top + G.altH); gr.addColorStop(0, "rgba(139,92,246,.35)"); gr.addColorStop(1, "rgba(34,211,238,.06)"); cx.fillStyle = gr; cx.fill();
    cx.lineWidth = 2.2;
    for (let i = a0 + 1; i <= a1; i++) { if (T[i][2] == null || T[i - 1][2] == null) continue; cx.strokeStyle = rgb(COLOR[colMode].f(T[i])); cx.beginPath(); cx.moveTo(xOf(S[i - 1]), yA(alt(T[i - 1]))); cx.lineTo(xOf(S[i]), yA(alt(T[i]))); cx.stroke(); }
    cx.strokeStyle = "rgba(226,232,240,.75)"; cx.lineWidth = 1.2; cx.setLineDash([4, 3]); cx.beginPath();
    let offenePfad = false;
    for (let i = a0; i <= a1; i++) { if (T[i][5] == null) { offenePfad = false; continue; } const x = xOf(S[i]), y = yI(zahl(T[i][5])); if (offenePfad) cx.lineTo(x, y); else { cx.moveTo(x, y); offenePfad = true; } }
    cx.stroke(); cx.setLineDash([]);
    // Abschnitte statt Einzelmesspunkte: jeder mindestens 3 px breit (kurze Speedbrake-/Lichtphasen bleiben sichtbar),
    // Zahlen nur, wo sie ganz hineinpassen — sonst nennt der Maus-Hinweis den Wert
    cx.font = '700 10px "JetBrains Mono", monospace'; cx.textAlign = "left";
    baender = STRIPS.map(([, on, , al], k) => {
      const feld = STRIP_FELD[k], laeufe: Array<{ s0: number; s1: number; wert: string; i: number }> = [];
      for (let i = a0; i < a1; i++) {
        const q = T[i];
        const wert = q[feld] == null ? "nm" : !on(q) ? "aus" : k === 0 ? String(q[13]) : k === 2 ? String(Math.round(zahl(q[15]))) : "an";
        const l = laeufe[laeufe.length - 1];
        if (l && l.wert === wert) l.s1 = S[i + 1]; else laeufe.push({ s0: S[i], s1: S[i + 1], wert, i });
      }
      const y = G.top + G.altH + 10 + k * (G.sH + G.gap), col = STRIPS[k][2];
      cx.fillStyle = "#152231"; cx.fillRect(G.L, y, CW - G.L - G.R, G.sH);
      let frei = -Infinity; // rechte Kante der zuletzt geschriebenen Zahl
      for (const l of laeufe) {
        const xa = xOf(l.s0), echt = xOf(l.s1) - xa, w = Math.max(3, echt), x0 = xa - (w - echt) / 2; // kurze Abschnitte mittig verbreitert
        if (l.wert === "nm") { cx.fillStyle = "rgba(131,148,166,.18)"; cx.fillRect(x0, y + G.sH / 2 - 0.5, w, 1); continue; } // nicht messbar: feine Linie
        if (l.wert === "aus") continue;
        cx.globalAlpha = al(T[l.i]); cx.fillStyle = col; cx.fillRect(x0, y, w, G.sH); cx.globalAlpha = 1;
        if (k === 0 || k === 2) {
          const txt = k === 2 ? l.wert + " %" : l.wert, tw = cx.measureText(txt).width;
          if (w >= tw + 6 && x0 + 3 > frei) { cx.fillStyle = "#0b121b"; cx.fillText(txt, x0 + 3, y + G.sH / 2 + 0.5); frei = x0 + 3 + tw + 4; }
        }
      }
      return laeufe;
    });
    cx.restore();
    cx.font = '11px "JetBrains Mono", monospace'; cx.fillStyle = "#9fb0c2"; cx.textAlign = "right";
    STRIPS.forEach(([name], k) => cx.fillText(name, G.L - 6, G.top + G.altH + 10 + k * (G.sH + G.gap) + G.sH / 2 + 0.5));
    cx.textAlign = "left"; cx.fillStyle = "#c9d4df"; cx.fillText("- - IAS", CW - G.R + 6, G.top - 10);
    const span = range[1] - range[0], stuecke = STUFEN.slice(1).map((b, j) => (b[1] - STUFEN[j][1]) / ((b[0] - STUFEN[j][0]) || 1));
    const pxProS = Math.max(...stuecke, (CW - G.L - G.R) / (span || 1));
    const stepT = [60, 120, 300, 600, 900, 1800, 3600, 7200, 10800].find((st) => st * pxProS >= 56) ?? 21600;
    const yAx = G.top + G.altH + 10 + STRIPS.length * (G.sH + G.gap) + 10;
    cx.textAlign = "center"; cx.fillStyle = "#9fb0c2";
    let letzteX = -Infinity; // Uhrzeiten nie enger als 56 px, auch nicht an gestauchten Stellen
    for (let s = Math.ceil(range[0] / stepT) * stepT; s <= range[1]; s += stepT) { const x = xOf(s); if (x - letzteX < 56 || LUECKEN.some(([a, b]) => s > a && s < b)) continue; cx.fillText(clock(s).slice(0, 5), x, yAx); letzteX = x; }
    // Bruchzeichen: hier ist ruhige Zeit gestaucht
    for (const [a, b] of LUECKEN) {
      const xa = xOf(a), xb = xOf(b), xm = (xa + xb) / 2;
      cx.fillStyle = "rgba(255,255,255,.035)"; cx.fillRect(xa, G.top, xb - xa, yAx - 8 - G.top);
      cx.strokeStyle = "rgba(159,176,194,.45)"; cx.lineWidth = 1; cx.setLineDash([2, 3]);
      for (const xx of [xa, xb]) { cx.beginPath(); cx.moveTo(xx, G.top); cx.lineTo(xx, yAx - 8); cx.stroke(); }
      cx.setLineDash([]); cx.fillStyle = "#c9d4df"; cx.fillText("≈", xm, yAx);
    }
    // Ereignis-Punkte oben, dicht beieinander liegende zusammengefasst
    const eimer = new Map<number, Ev[]>();
    visible().forEach((e) => { if (e.s < range[0] || e.s > range[1] || e.k === "phase") return; const b = Math.round(xOf(e.s) / 7); const l = eimer.get(b) ?? []; l.push(e); eimer.set(b, l); });
    for (const l of eimer.values()) {
      l.sort((a, b) => prio(a) - prio(b)); const e = l[0], x = l.reduce((s, q) => s + xOf(q.s), 0) / l.length;
      cx.fillStyle = CATS[e.k].col; cx.beginPath(); cx.arc(x, 8, l.some((q) => q.n === sel) ? 5.5 : 3.4 + Math.min(l.length - 1, 4) * 0.6, 0, Math.PI * 2); cx.fill();
    }
    puffer.width = cv.width; puffer.height = cv.height; px.drawImage(cv, 0, 0);
    chartKey = key; chartYA = yA; chartYAx = yAx;
    zeitregler(yA, yAx);
  }
  function zeitregler(yA: (a: number) => number, yAx: number) {
    if (scrubS < range[0] || scrubS > range[1]) return;
    const x = xOf(scrubS); cx.strokeStyle = "#ffffff"; cx.lineWidth = 1.5; cx.beginPath(); cx.moveTo(x, 14); cx.lineTo(x, yAx - 8); cx.stroke();
    cx.fillStyle = "#ffffff"; cx.beginPath(); cx.arc(x, yA(stateAt(scrubS).alt), 4.5, 0, Math.PI * 2); cx.fill();
  }
  let cDown = false;
  function chartPointer(ev: PointerEvent) {
    const r = cv.getBoundingClientRect(), x = ev.clientX - r.left, y = ev.clientY - r.top;
    if (ev.type === "pointerdown" && y < 18) {
      let best: number | null = null, bd = 9;
      visible().forEach((e) => { const dx = Math.abs(xOf(e.s) - x); if (e.s >= range[0] && e.s <= range[1] && dx < bd) { bd = dx; best = e.n; } });
      if (best != null) { select(best, true); return; }
    }
    scrubS = Math.max(range[0], Math.min(range[1], sOf(x)));
    if (follow) chase();
    refresh();
  }
  cv.addEventListener("pointerdown", (e) => { cDown = true; cv.setPointerCapture(e.pointerId); chartPointer(e); });
  cv.addEventListener("pointermove", (e) => { if (cDown) chartPointer(e); });
  cv.addEventListener("pointerup", () => (cDown = false));
  // Maus ueber einem Band: Wert und Zeitraum des Abschnitts (auch fuer sehr kurze Phasen)
  const tipp = $("bandtipp");
  const BAND_WERT = (k: number, w: string) => k === 0 ? (KLAPPEN_PROZENT ? w + " %" : U("band_stufe", { w })) : k === 1 ? U("band_ausgefahren") : k === 2 ? `${w} %` : U("band_an");
  cv.addEventListener("mousemove", (e) => {
    const r = cv.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
    const k = Math.floor((y - (G.top + G.altH + 10)) / (G.sH + G.gap));
    const imBand = k >= 0 && k < STRIPS.length && y - (G.top + G.altH + 10 + k * (G.sH + G.gap)) <= G.sH && x >= G.L && x <= CW - G.R;
    const s = sOf(x), l = imBand ? baender[k]?.find((b) => s >= b.s0 && s <= b.s1 + 0.5 || (xOf(b.s1) - xOf(b.s0) < 3 && Math.abs(x - xOf(b.s0)) <= 3)) : undefined;
    if (!l || l.wert === "aus") { tipp.hidden = true; return; }
    const dauer = Math.round(l.s1 - l.s0);
    tipp.textContent = `${STRIPS[k][0]}: ${l.wert === "nm" ? U("nicht_messbar") : BAND_WERT(k, l.wert)} · ${clock(l.s0)}–${clock(l.s1)} (${dauer >= 120 ? Math.round(dauer / 60) + " min" : dauer + " s"})`;
    tipp.hidden = false;
    const sr = cv.parentElement!.getBoundingClientRect(), tw = tipp.offsetWidth;
    tipp.style.left = Math.max(4, Math.min(sr.width - tw - 4, e.clientX - sr.left - tw / 2)) + "px";
    tipp.style.top = r.top - sr.top + G.top + G.altH + 10 + k * (G.sH + G.gap) - 26 + "px";
  });
  cv.addEventListener("mouseleave", () => (tipp.hidden = true));

  // ---------- Wiedergabe und Verfolgen ----------
  // Verfolgen fuehrt nur die Mitte nach; Zoom, Neigung, Drehung bleiben beim Nutzer.
  // Waehrend einer eigenen Geste (samt Nachlauf bis moveend) setzt es aus.
  let lastT = 0;
  const playBtn = $("play");
  for (const evn of ["movestart", "zoomstart", "rotatestart", "pitchstart", "move", "rotate"] as const) {
    map.on(evn, (e: any) => { if (!e.originalEvent) return; gesteAktiv = true; selbstBewegt = true; if (evn.startsWith("rotate")) userRotated = true; if (evn === "zoomstart") userZoomed = true; });
  }
  map.on("moveend", () => { if (gesteAktiv) { gesteAktiv = false; gesteEnde = performance.now(); } });
  const userBusy = () => gesteAktiv || performance.now() - gesteEnde < 300;
  /** 3D-Verfolgen: Zoom, bei dem das Flugzeug in dieser Hoehe gut im Bild steht (die Kamera weit darueber) */
  const zoomFuerHoehe = (st: { h: number; lat: number }) => {
    const h = map.getCanvas().clientHeight, altM = Math.max(st.h * FT, 150);
    return Math.max(7, Math.min(15, Math.log2((0.12 * h * 40075016 * Math.cos((st.lat * Math.PI) / 180)) / (512 * altM * Math.sin((62 * Math.PI) / 180)))));
  };
  function startFollow() {
    userRotated = false; userZoomed = false;
    const st = stateAt(scrubS);
    if (is3d) {
      map.jumpTo({ center: [st.lon, st.lat], bearing: st.hdg, pitch: 62, zoom: zoomFuerHoehe(st) });
    } else map.jumpTo({ center: [st.lon, st.lat], zoom: Math.max(map.getZoom(), 11.5) });
    chase();
  }
  function chase() {
    if (userBusy()) return;
    const st = stateAt(scrubS);
    // 3D: der Kameraabstand folgt der Hoehe (sonst flog das Flugzeug im Steigflug bei ~5 000 ft auf Kamerahoehe, die
    // Spur verschwand oder bog sich weg; im Anflug blieb die Kamera auf Reiseflug-Abstand). Hat der Nutzer selbst
    // gezoomt, gilt sein Zoom — naeher als eine Stufe ueber dem Hoehenzoom aber nie.
    const zH = is3d ? zoomFuerHoehe(st) : 0;
    const zoom = !is3d ? {} : !userZoomed ? { zoom: zH } : map.getZoom() > zH + 1 ? { zoom: zH + 1 } : {};
    map.jumpTo(is3d && !userRotated ? { center: [st.lon, st.lat], bearing: st.hdg, ...zoom } : { center: [st.lon, st.lat], ...zoom });
    if (is3d) {
      const c = map.getCanvas();
      for (let k = 0; k < 3; k++) { // das Flugzeug schwebt ueber seinem Bodenpunkt: in die Mitte nachschieben
        const [x, y] = viewport().project([st.lon, st.lat, st.h * FT]);
        const dx = x - c.clientWidth / 2, dy = y - c.clientHeight / 2;
        if (Math.abs(dx) + Math.abs(dy) < 1) break;
        map.panBy([dx, dy], { duration: 0 });
      }
    }
  }
  let spielRq = 0;
  function tick(now: number) {
    if (!playing || beendet) return;
    const dt = lastT ? (now - lastT) / 1000 : 0; lastT = now;
    scrubS += dt * speed;
    if (scrubS >= range[1]) { scrubS = range[1]; setPlay(false); }
    if (follow) chase();
    refresh();
    spielRq = playing ? requestAnimationFrame(tick) : 0;
  }
  function setPlay(on: boolean) {
    playing = on; lastT = 0;
    cancelAnimationFrame(spielRq); spielRq = 0; // Pause→Abspielen im selben Bild: sonst zwei Schleifen, doppeltes Tempo
    playBtn.textContent = on ? "❚❚ " + U("pause") : "▶ " + U("abspielen");
    if (on) { if (scrubS >= range[1] - 1) scrubS = range[0]; spielRq = requestAnimationFrame(tick); }
  }
  playBtn.onclick = () => setPlay(!playing);
  const press = (q: string, btn: Element | null) => alle(q).forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
  alle("[data-spd]").forEach((b) => (b.onclick = () => { press("[data-spd]", b); speed = +b.dataset.spd!; }));
  const followBox = $<HTMLInputElement>("folgen");
  followBox.onchange = () => { follow = followBox.checked; if (follow) startFollow(); };
  const stopFollow = () => { follow = false; followBox.checked = false; };

  // ---------- Ausschnitt, Ansicht, Karte ----------
  const tdS = TD_S ?? S[S.length - 1] - 600;
  const ersteZeit = (k: string, ph?: string) => E.find((e) => e.k === k && (ph == null || e.ph === ph))?.s;
  const RANGES: Record<string, number[]> = {
    all: [S[0], S[S.length - 1]],
    dep: [ersteZeit("phase", "Pushback") ?? S[0], (ersteZeit("to") ?? S[0]) + 150],
    arr: [ersteZeit("phase", "Approach") ?? tdS - 900, tdS + 40],
    gnd: [tdS - 70, S[S.length - 1]],
  };
  let fitKey = "all", ansichtZug = 0;
  let selbstBewegt = false; // hat der Nutzer die Karte seit dem letzten Einpassen selbst bewegt?
  // vonVorn: der Nutzer hat einen Ausschnitt gewaehlt — Zeitregler an dessen Anfang, damit „Abspielen“ den Ausschnitt
  // von vorn bis hinten zeigt (stand er noch kurz vor dem Aufsetzen, spielte „Anflug“ nur die letzten Sekunden und hielt an)
  function fit(key: string, anim = true, vonVorn = false) {
    selbstBewegt = false;
    fitKey = key; range = RANGES[key].slice();
    if (range[1] <= range[0]) range = RANGES.all.slice();
    if (vonVorn || scrubS < range[0] || scrubS > range[1]) scrubS = range[0];
    const a = idxAt(range[0]), b = idxAt(range[1]);
    let w = Infinity, s = Infinity, e = -Infinity, n = -Infinity; // nicht ±180: fortlaufende Laengen
    for (let i = a; i <= b; i++) { w = Math.min(w, T[i][1] as number); e = Math.max(e, T[i][1] as number); s = Math.min(s, T[i][0] as number); n = Math.max(n, T[i][0] as number); }
    const narrow = map.getCanvas().clientWidth < 640;
    const grund = narrow ? { top: 40, bottom: 40, left: 40, right: 70 } /* rechts die Zoomknoepfe */ : is3d ? { top: 170, bottom: 120, left: 60, right: 80 } : { top: 80, bottom: 60, left: 60, right: 60 };
    // unten links steht die Hoehenlegende: unten mindestens so viel Rand, dass Spurende und Flugzeug darueber bleiben
    const legende = root.querySelector<HTMLElement>(".fk-legende");
    if (!narrow && legende && legende.offsetHeight) grund.bottom = Math.max(grund.bottom, legende.offsetHeight + 12 + 20);
    const cw = map.getCanvas().clientWidth, ch = map.getCanvas().clientHeight;
    // Rand hoechstens 60 % der Karte, sonst passt der Flug nicht hinein und MapLibre tut gar nichts
    const begrenzt = (v: typeof grund) => {
      const kv = Math.min(1, (ch * 0.6) / (v.top + v.bottom)), kh = Math.min(1, (cw * 0.6) / (v.left + v.right));
      return { top: v.top * kv, bottom: v.bottom * kv, left: v.left * kh, right: v.right * kh };
    };
    // Die Datenanzeige (HUD) sitzt unten rechts: entweder darueber oder links daneben Platz lassen,
    // je nachdem, was den Flug groesser zeigt. Der HUD-Rand bleibt dabei ganz; gekuerzt wird nur der Gegenrand.
    let pad = begrenzt(grund);
    if (!narrow && getComputedStyle(hudEl).position === "absolute") {
      // Platz auch fuer den Kasten „letztes Ereignis“ (54 px) und ein halbes Flugzeugsymbol (20 px) ueber der Datenanzeige
      const hudH = hudEl.offsetHeight + 46 + 54 + 20, hudW = Math.max(hudEl.offsetWidth, lastEl.offsetWidth) + 24 + 20;
      const varianten: Array<typeof pad> = [];
      if (hudH + 20 < ch * 0.85) varianten.push({ ...begrenzt(grund), bottom: hudH, top: Math.min(grund.top, ch * 0.85 - hudH) });
      if (hudW + 20 < cw * 0.85) varianten.push({ ...begrenzt(grund), right: hudW, left: Math.min(grund.left, cw * 0.85 - hudW) });
      const zoomMit = (p: typeof pad) => map.cameraForBounds([[w, s], [e, n]], { padding: p, bearing: is3d ? -35 : 0 })?.zoom ?? -1;
      if (varianten.length) pad = varianten.reduce((a, b) => (zoomMit(b) > zoomMit(a) ? b : a));
    }
    map.fitBounds([[w, s], [e, n]], { padding: pad, bearing: is3d ? -35 : 0, pitch: is3d ? 60 : 0, maxZoom: 16, duration: anim && !reduziert() ? 900 : 0 });
    refresh();
  }
  // neuer Ausschnitt: Auswahl aufheben — sonst blieb das Popup des alten Ereignisses in der Ecke stehen und das
  // Einpassen oeffnete dessen Haufen wieder, am Kartenrand festgeklemmt (Thomas 28.10.)
  alle("[data-fit]").forEach((b) => (b.onclick = () => { stopFollow(); haufenZu(); select(null); press("[data-fit]", b); fit(b.dataset.fit!, true, true); }));
  // ohne gemessene Landung gibt es keinen Landeausschnitt — nichts erfinden
  if (TD_S == null) alle('[data-fit="arr"],[data-fit="gnd"]').forEach((b) => { (b as HTMLButtonElement).disabled = true; b.title = U("keine_landung"); });
  function setView(three: boolean) {
    haufenZu(); is3d = three; press('[data-fk="v2d"], [data-fk="v3d"]', $(three ? "v3d" : "v2d"));
    map.easeTo({ pitch: three ? 60 : 0, bearing: three ? -35 : 0, duration: reduziert() ? 0 : 900 });
    const zug = ++ansichtZug;
    map.once("moveend", () => { if (zug === ansichtZug && !beendet) (follow ? startFollow() : fit(fitKey)); });
    refresh();
  }
  $("v2d").onclick = () => setView(false);
  $("v3d").onclick = () => setView(true);
  alle("[data-base]").forEach((b) => (b.onclick = () => {
    base = b.dataset.base!; press("[data-base]", b);
    VEC.forEach(([id, type]) => { if (type !== "symbol") map.setLayoutProperty(id, "visibility", base === "sat" ? "none" : "visible"); });
    if (map.getLayer("sat")) map.setLayoutProperty("sat", "visibility", base === "sat" ? "visible" : "none");
    refresh();
  }));
  $("relief").onclick = () => { relief = !relief; $("relief").setAttribute("aria-pressed", String(relief)); if (map.getLayer("relief")) map.setLayoutProperty("relief", "visibility", relief ? "visible" : "none"); };
  alle("[data-col]").forEach((b) => (b.onclick = () => { press("[data-col]", b); colMode = b.dataset.col!; legend(); refresh(); }));

  // Kartenebenen auf- und zuklappen; ein Klick daneben oder Esc schliesst
  const ebenenKnopf = $("ebenenKnopf"), ebenenFeld = $("ebenenFeld");
  const kennung = "fk" + Math.random().toString(36).slice(2, 8); // mehrere Karten auf einer Seite: eindeutige IDs
  ebenenFeld.id = kennung + "-ebenen"; ebenenKnopf.setAttribute("aria-controls", ebenenFeld.id);
  const ebenenZu = () => { ebenenFeld.hidden = true; ebenenKnopf.setAttribute("aria-expanded", "false"); lastEl.style.visibility = ""; };
  ebenenKnopf.onclick = () => {
    if (!ebenenFeld.hidden) return ebenenZu();
    ebenenFeld.hidden = false; ebenenKnopf.setAttribute("aria-expanded", "true");
    // buendig zum oberen Kartenrand aufklappen (nicht auf Hoehe des Knopfs), damit es unten nichts verdeckt
    ebenenFeld.style.top = 10 - $("ebenen").offsetTop + "px";
    // stoesst es in einer niedrigen Karte trotzdem an „letztes Ereignis“, ruht dieser Kasten, solange das Feld offen ist
    const f = ebenenFeld.getBoundingClientRect(), l = lastEl.getBoundingClientRect();
    if (!lastEl.hidden && Math.min(f.right, l.right) > Math.max(f.left, l.left) && Math.min(f.bottom, l.bottom) > Math.max(f.top, l.top)) lastEl.style.visibility = "hidden";
  };
  const ebenenAussen = (e: Event) => { if (!ebenenFeld.hidden && !$("ebenen").contains(e.target as Node)) ebenenZu(); };
  const ebenenEsc = (e: KeyboardEvent) => { if (e.key === "Escape" && !ebenenFeld.hidden) { e.preventDefault(); e.stopPropagation(); ebenenZu(); ebenenKnopf.focus(); } };
  document.addEventListener("pointerdown", ebenenAussen);
  root.addEventListener("keydown", ebenenEsc);
  aufraeumen.push(() => { document.removeEventListener("pointerdown", ebenenAussen); root.removeEventListener("keydown", ebenenEsc); });

  // Filterblock einklappbar: in niedrigen Fenstern anfangs zu, damit die Ereignisliste Platz hat; Wahl wird gemerkt
  const filterKnopf = $("filterKnopf");
  chipsEl.id = kennung + "-filter"; filterKnopf.setAttribute("aria-controls", chipsEl.id);
  // je Fensterklasse gemerkt: offen im hohen Fenster soll nicht heissen „Liste weg“ im niedrigen
  const filterSchluessel = "fk-filter-offen-" + (window.innerHeight >= 900 ? "hoch" : "niedrig");
  let filterOffen = window.innerHeight >= 900;
  try { const g = localStorage.getItem(filterSchluessel); if (g != null) filterOffen = g === "1"; } catch { /* ohne Speicher */ }
  const seiteEl = root.querySelector<HTMLElement>(".fk-seite")!, filterEl = root.querySelector<HTMLElement>(".fk-filter")!;
  const filterZeigen = () => {
    chipsEl.hidden = !filterOffen; filterKnopf.setAttribute("aria-expanded", String(filterOffen)); filterKnopf.querySelector("span")!.textContent = filterOffen ? "▴" : "▾";
    seiteEl.classList.remove("fk-nur-filter");
    if (filterOffen && seiteEl.clientHeight - filterEl.scrollHeight < 140) seiteEl.classList.add("fk-nur-filter");
  };
  filterKnopf.onclick = () => { filterOffen = !filterOffen; filterZeigen(); try { localStorage.setItem(filterSchluessel, filterOffen ? "1" : "0"); } catch { /* ohne Speicher */ } };

  // Ereignisleiste und Datenstreifen wegklappbar — die Karte bekommt den Platz; Wahl wird gemerkt
  const umschalter = (knopf: HTMLElement, klasse: string, schluessel: string, attr: "aria-pressed" | "aria-expanded") => {
    let an = true;
    try { an = localStorage.getItem(schluessel) !== "0"; } catch { /* ohne Speicher */ }
    const zeigen = () => {
      root.classList.toggle(klasse, !an); knopf.setAttribute(attr, String(an));
      const pfeil = knopf.querySelector("span"); if (pfeil) pfeil.textContent = an ? "▾" : "▴";
    };
    knopf.onclick = () => { an = !an; zeigen(); tipp.hidden = true; try { localStorage.setItem(schluessel, an ? "1" : "0"); } catch { /* ohne Speicher */ } requestAnimationFrame(() => { if (!beendet) { drawChart(); filterZeigen(); } }); };
    zeigen();
  };
  root.querySelector(".fk-seite")!.id = kennung + "-seite"; $("seiteKnopf").setAttribute("aria-controls", kennung + "-seite");
  cv.id = kennung + "-streifen"; $("streifenKnopf").setAttribute("aria-controls", cv.id);
  umschalter($("seiteKnopf"), "fk-ohne-seite", "fk-seite-offen", "aria-pressed");
  umschalter($("streifenKnopf"), "fk-ohne-streifen", "fk-streifen-offen", "aria-expanded");

  legend(); renderChips(); renderList(); hud();
  filterZeigen(); // erst nach renderChips messen, sonst misst es einen leeren Filterblock
  // Groesse des Kartenfelds beobachten, nicht nur das Fenster: das Feld aendert sich auch ohne Fenster-Resize
  // andere Groesse: neu einpassen, solange der Nutzer nicht selbst verschoben/gezoomt hat — sonst lag
  // der Flug nach dem Vergroessern/Verkleinern des Fensters unter der Datenanzeige
  const beobachter = new ResizeObserver(() => { map.resize(); if (follow) chase(); else if (!selbstBewegt && overlay) fit(fitKey, false); refresh(); filterZeigen(); });
  beobachter.observe($("karte"));
  aufraeumen.push(() => beobachter.disconnect());
  const ersterRq = requestAnimationFrame(() => { if (!beendet) drawChart(); });

  return () => {
    beendet = true; playing = false;
    cancelAnimationFrame(rq); cancelAnimationFrame(spielRq); cancelAnimationFrame(ersterRq);
    aufraeumen.forEach((f) => f());
    try { map.remove(); } catch { /* schon weg */ }
    root.innerHTML = ""; root.classList.remove("fk");
  };
}
