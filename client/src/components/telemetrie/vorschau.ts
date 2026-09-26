/**
 * Vorschau-Datenquelle fuer den Telemetrie-Monitor (nur Entwicklung:
 * `?vorschau=telemetrie`). Spielt einen A320-Endanflug mit Aufsetzen und
 * Ausrollen in Schleife ab, damit sich die Oberflaeche ohne Simulator
 * ansehen laesst. Nie im Flugbetrieb verwendet.
 */

import katalogJson from "./vorschauKatalog.json";
import type { Datenquelle } from "./useTelemetrie";
import type { Frame, Katalog } from "./typen";

const katalog = katalogJson as Katalog;

interface Zustand {
  t: number;
  agl: number;
  ias: number;
  vs: number;
  pitch: number;
  aoa: number;
  g: number;
  n1: number;
  boden: boolean;
  seitTd: number;
  sprit: number;
  flaps: number;
  gear: number;
}

function neu(): Zustand {
  return { t: 0, agl: 2500, ias: 160, vs: -700, pitch: 2, aoa: 5, g: 1, n1: 50, boden: false, seitTd: 0, sprit: 4200, flaps: 3, gear: 0 };
}

const r = (a = 1) => (Math.random() - 0.5) * 2 * a;
const naeher = (a: number, b: number, k: number) => a + (b - a) * k;

function schritt(z: Zustand, dt: number) {
  z.t += dt;
  if (!z.boden) {
    const flare = z.agl < 40;
    z.vs = naeher(z.vs, flare ? -140 : -720 + Math.sin(z.t / 6) * 60, flare ? 0.06 : 0.2) + r(10);
    z.agl = Math.max(0, z.agl + (z.vs / 60) * dt);
    z.ias = naeher(z.ias, flare ? 131 : z.agl > 1500 ? 150 : 137 + Math.sin(z.t / 9) * 2.5, 0.03) + r(0.3);
    z.pitch = naeher(z.pitch, flare ? 5.2 : 2.4, 0.08) + r(0.05);
    z.aoa = naeher(z.aoa, flare ? 7.6 : 5.6 + Math.sin(z.t / 5) * 0.3, 0.1) + r(0.05);
    z.g = 1 + Math.sin(z.t * 1.7) * 0.03 + r(0.015);
    z.n1 = naeher(z.n1, z.agl < 25 ? 22 : 48 + Math.sin(z.t / 5) * 2, 0.06);
    if (z.agl < 2000) z.gear = Math.min(100, z.gear + 20 * dt);
    if (z.agl < 1800) z.flaps = 4;
    if (z.agl <= 0) {
      z.boden = true;
      z.g = 1.22;
    }
  } else {
    z.seitTd += dt;
    z.vs = naeher(z.vs, 0, 0.3);
    z.g = naeher(z.g, 1, 0.25) + r(0.02);
    z.ias = Math.max(0, z.ias - 5 * dt);
    z.pitch = naeher(z.pitch, -0.5, 0.1);
    z.aoa = naeher(z.aoa, 0, 0.1);
    z.n1 = naeher(z.n1, z.seitTd > 1.5 && z.seitTd < 15 ? 70 : 21, 0.05);
    if (z.ias < 25) Object.assign(z, neu());
  }
  z.sprit -= ((z.n1 * 25) / 3600) * dt;
}

function frame(z: Zustand, jetzt: number): Frame {
  const w: Record<string, number | null> = {
    hoehe_msl: z.agl + 364,
    hoehe_agl: z.agl,
    radarhoehe: z.agl < 2500 ? z.agl : null,
    ias: z.ias,
    tas: z.ias * 1.02,
    gs: z.ias - 9,
    mach: z.ias / 661,
    vs: z.vs,
    pitch: z.pitch,
    bank: Math.sin(z.t / 11) * 2,
    kurs_mw: 248,
    aoa: z.aoa,
    schiebewinkel: Math.sin(z.t / 8) * 0.6,
    aoa_abriss: 14.5,
    g: z.g,
    g_laengs: z.boden ? -0.25 : r(0.01),
    g_seitlich: r(0.02),
    rate_rollen: Math.cos(z.t / 11) * 0.6,
    rate_nicken: z.agl < 40 && !z.boden ? 1 : r(0.2),
    rate_gieren: r(0.2),
    vs1: 118,
    vfe: 177,
    vmo: 350,
    gegenwind: 11,
    seitenwind: 4,
    wind_richtung: 250,
    wind_staerke: 12,
    oat: 14,
    qnh: 1016,
    triebwerke_anzahl: 2,
    n1_1: z.n1,
    n1_2: z.n1 + 0.4,
    n2_1: 60 + z.n1 * 0.45,
    n2_2: 60 + z.n1 * 0.45,
    egt_1: 380 + z.n1 * 3,
    egt_2: 386 + z.n1 * 3,
    ff_1: z.n1 * 24,
    ff_2: z.n1 * 24.3,
    oeldruck_1: 42,
    oeldruck_2: 43,
    oeltemp_1: 88,
    oeltemp_2: 90,
    schubhebel_1: z.agl < 25 ? 0 : 55,
    schubhebel_2: z.agl < 25 ? 0 : 55,
    laeuft_1: 1,
    laeuft_2: 1,
    sprit_gesamt: z.sprit,
    tank_links: z.sprit * 0.45,
    tank_mitte: z.sprit * 0.1,
    tank_rechts: z.sprit * 0.45,
    tank_links_kap: 6200,
    tank_mitte_kap: 6500,
    tank_rechts_kap: 6200,
    tank_summe: z.sprit,
    gewicht: 61800,
    schwerpunkt: 28.4,
    fahrwerk: z.gear,
    am_boden: z.boden ? 1 : 0,
    boden_bug: z.boden && z.seitTd > 1.6 ? 1 : 0,
    boden_links: z.boden ? 1 : 0,
    boden_rechts: z.boden ? 1 : 0,
    bremse_links: z.boden && z.seitTd > 2 ? 45 : 0,
    bremse_rechts: z.boden && z.seitTd > 2 ? 46 : 0,
    klappen: z.flaps * 25,
    klappen_stufe: z.flaps,
    spoiler: z.boden ? 100 : 0,
    spoiler_armed: 1,
    umkehr_1: z.boden && z.seitTd > 1.5 && z.seitTd < 15 ? 100 : 0,
    hoehenruder: z.agl < 40 && !z.boden ? -30 : r(5),
    querruder: r(8),
    seitenruder: r(3),
    trimmung: 8,
    ap: z.agl > 700 ? 1 : 0,
    athr: z.agl > 25 ? 1 : 0,
    soll_fahrt: 137,
    soll_hoehe: 3000,
    vapp: 137,
    vls: 118,
    loc_ablage: Math.sin(z.t / 13) * 0.18,
    gs_ablage: Math.sin(z.t / 10) * 0.22,
    loc_empfang: 1,
    gs_empfang: 1,
    dme: z.agl / 318,
    kabinenhoehe: 700,
    kabine_vs: -300,
    differenzdruck: 1.2,
    hydraulik: 3000,
    batteriespannung: 28.1,
    aussenstrom: 0,
    batterie: 1,
    avionik: 1,
    apu: 0,
    master_caution: 0,
    master_warning: 0,
    ueberziehwarnung: 0,
    licht_lande: 1,
    squawk: 4721,
    sim_rate: 1,
    pause: 0,
    energiehoehe: z.agl + 364 + (z.ias * 1.688) ** 2 / 64.35,
    vorhaltewinkel: 1.2,
  };
  const texte: Record<string, string> = {
    fma_lateral: "LOC",
    fma_vertikal: z.agl < 50 && !z.boden ? "FLARE" : "G/S",
    fma_schub: z.agl > 25 ? "SPEED" : "",
    autobrake: "MED",
    muster: "A320",
    kennzeichen: "D-AIUK",
    phase_acars: z.boden ? "Landing" : "Final",
    flugzeug: "FenixA320 Lufthansa",
  };
  return {
    t: jetzt,
    z: katalog.zahlen.map((k) => {
      const v = w[k.id];
      return typeof v === "number" ? v : null;
    }),
    s: katalog.texte.map((k) => texte[k.id] || null),
  };
}

/** Eine neue Vorschau-Quelle; einmal je Seite anlegen. */
export function vorschauQuelle(): Datenquelle {
  const z = neu();
  const verlauf: Frame[] = [];
  let t = Date.now() - 90_000;
  for (let i = 0; i < 900; i++) {
    schritt(z, 0.1);
    verlauf.push(frame(z, t));
    t += 100;
  }
  let timer: number | null = null;
  return {
    start: async () => ({ katalog, verlauf: [...verlauf] }),
    abonnieren: async (cb) => {
      timer = window.setInterval(() => {
        schritt(z, 0.05);
        cb(frame(z, Date.now()));
      }, 50);
      return () => {
        if (timer !== null) window.clearInterval(timer);
      };
    },
    halten: () => undefined,
    stop: () => undefined,
  };
}
