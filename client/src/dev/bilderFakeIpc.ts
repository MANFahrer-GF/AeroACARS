// Attrappe der IPC-Schicht fuer die Vorschau des Bilder-Tabs (nur vite.bilder.config.mjs).
// `?fall=` normal (Entwurf: vier Gruppen), fehler, leer (erster Start, ungesendet), leerein,
// ohneflug, kontingent0, unklar, offline, viele (60 Bilder), fund (neuer Fund im Eingang).
export const isTauri = !window.location.search.includes("tablet=1");
export const formatIpcError = (e: unknown) => String(e);
export async function openExternal(url: string) { window.open(url, "_blank"); }

type Cb = (e: { event: string; payload: unknown }) => void;
const hoerer = new Map<string, Cb[]>();
export async function listen(event: string, cb: Cb) {
  hoerer.set(event, [...(hoerer.get(event) ?? []), cb]);
  return () => undefined;
}
export function senden(event: string, payload: unknown) { (hoerer.get(event) ?? []).forEach((cb) => cb({ event, payload })); }

const q = new URLSearchParams(window.location.search);
const fall = q.get("fall") ?? "normal";
const w = window as unknown as Record<string, unknown>;
w.__fakeLog = [] as unknown[];
const log = (c: string, a: unknown) => (w.__fakeLog as unknown[]).push({ c, a });

// ---------- Bilder ----------
interface Szene { sky: string; ground: string; horizon: number; cloud?: boolean; runway?: boolean; wing?: boolean }
const SZENEN: Szene[] = [
  { sky: "#9fb9cf", ground: "#4a5560", horizon: 70, wing: true },
  { sky: "#86afd3", ground: "#5c7452", horizon: 84, wing: true },
  { sky: "#3f6fa3", ground: "#dfe8f0", horizon: 92, cloud: true, wing: true },
  { sky: "#9ab8d2", ground: "#6a7f5a", horizon: 66, cloud: true },
  { sky: "#8db4d6", ground: "#5f7a55", horizon: 74, cloud: true, runway: true, wing: true },
  { sky: "#b7c6d2", ground: "#4e5a63", horizon: 60, runway: true },
  { sky: "#2b3a4a", ground: "#1e262d", horizon: 64, wing: true },
  { sky: "#1d2d44", ground: "#0e1420", horizon: 96 },
  { sky: "#92b7d8", ground: "#3f6f8f", horizon: 80 },
  { sky: "#9cc0dc", ground: "#4d7d95", horizon: 76, runway: true },
];
async function szeneAlsBytes(i: number, breite: number, hoehe: number, mime: "image/jpeg" | "image/png"): Promise<Uint8Array> {
  const s = SZENEN[i % SZENEN.length];
  const c = new OffscreenCanvas(breite, hoehe);
  const g = c.getContext("2d")!;
  const k = (y: number) => (y / 124) * hoehe;
  g.fillStyle = s.sky; g.fillRect(0, 0, breite, hoehe);
  if (s.cloud) { g.fillStyle = "#e6eef5"; g.fillRect(0, k(52), breite, k(14)); }
  g.fillStyle = s.ground; g.fillRect(0, k(s.horizon), breite, hoehe);
  if (s.runway) { g.fillStyle = "#43484d"; g.beginPath(); g.moveTo(breite * 0.43, hoehe); g.lineTo(breite * 0.57, hoehe); g.lineTo(breite * 0.515, k(s.horizon)); g.lineTo(breite * 0.485, k(s.horizon)); g.fill(); }
  if (s.wing) { g.fillStyle = "#2d3741"; g.beginPath(); g.moveTo(0, hoehe); g.lineTo(0, k(96)); g.lineTo(breite * 0.6, k(110)); g.lineTo(breite * 0.6, hoehe); g.fill(); }
  return new Uint8Array(await (await c.convertToBlob({ type: mime, quality: 0.8 })).arrayBuffer());
}

interface Zeile { id: string; gruppe: string; szene: number; iso: string | null; meta: Record<string, unknown>; text?: string; favorit?: boolean; sendungen?: unknown[]; bytes?: number }
const heute = new Date();
const tag = (offset: number, hm: string) => {
  const d = new Date(Date.UTC(heute.getUTCFullYear(), heute.getUTCMonth(), heute.getUTCDate() - offset));
  return `${d.toISOString().slice(0, 10)}T${hm}:00.000Z`;
};
const flug = (pirep: string | null, rz: string | null, a: string | null, z: string | null, ac = "A320-251N", icao = "A20N") => ({ pirep_id: pirep, rufzeichen: rz, abflug: a, ziel: z, flugzeug_titel: ac, flugzeug_icao: icao, simulator: "xplane12" });
const pos = (la: number, lo: number, h: number, gs: number, kurs: number, plat: string) => ({ breite: la, laenge: lo, hoehe_ft: h, gs_kt: gs, kurs, naechster_flugplatz: plat });
const gesendet = [{ sendung_id: "S0", zeit_ms: 1, geo_stufe: "verdeckt", status: "gesendet" }];
const fehlschlag = [{ sendung_id: "S1", zeit_ms: 1, geo_stufe: "verdeckt", status: "abgelehnt", code: "server_nicht_erreichbar" }];

function datensatz(): Zeile[] {
  if (fall === "leer" || fall === "leerein") return [];
  if (fall === "viele") {
    return Array.from({ length: 60 }, (_, i) => ({ id: `v${i}`, gruppe: "P412", szene: i, iso: tag(0, "10:00").replace("10:00", `${String(10 + Math.floor(i / 6)).padStart(2, "0")}:${String((i % 6) * 9).padStart(2, "0")}`), meta: { ...flug("P412", "GSG412", "EDDF", "LSZH"), ...pos(47 + i / 100, 8 + i / 50, 3000 + i * 400, 220, 140, "LSZH"), phase: "cruise", quelle: "sim" } }));
  }
  const a = flug("P412", "GSG412", "EDDF", "LSZH");
  const b = flug("P118", "GSG118", "LSZH", "EDDH");
  const c = flug("P9", "GSG9", "EDDS", "LFMN", "Falcon 50EX", "FA50");
  const z: Zeile[] = [
    { id: "a1", gruppe: "P412", szene: 0, iso: tag(0, "13:04"), meta: { ...a, ...pos(50.04, 8.56, 0, 12, 90, "EDDF"), phase: "taxi_out", quelle: "sim" }, sendungen: gesendet },
    { id: "a2", gruppe: "P412", szene: 1, iso: tag(0, "13:11"), meta: { ...a, ...pos(50.2, 8.9, 1850, 210, 40, "EDDF"), phase: "climb", quelle: "sim" } },
    { id: "a3", gruppe: "P412", szene: 2, iso: tag(0, "13:38"), meta: { ...a, ...pos(49.1, 8.7, 36000, 450, 190, "LSZH"), phase: "cruise", quelle: "sim" } },
    { id: "a4", gruppe: "P412", szene: 3, iso: tag(0, "14:21"), meta: { ...a, ...pos(47.9, 8.4, 11200, 290, 160, "LSZH"), phase: "descent", quelle: "sim" } },
    { id: "a5", gruppe: "P412", szene: 4, iso: tag(0, "14:32"), meta: { ...a, ...pos(47.4581, 8.549, 2410, 148, 142, "LSZH"), phase: "approach", quelle: "sim" }, text: "Anflug auf Zürich bei klarer Sicht, Piste 14." },
    { id: "a6", gruppe: "P412", szene: 5, iso: tag(0, "14:35"), meta: { ...a, ...pos(47.46, 8.55, 1416, 80, 140, "LSZH"), phase: "landing", quelle: "sim" } },
    { id: "b1", gruppe: "P118", szene: 6, iso: tag(1, "18:02"), meta: { ...b, ...pos(47.46, 8.55, 0, 0, 0, "LSZH"), phase: "boarding", quelle: "sim" }, sendungen: gesendet },
    { id: "b2", gruppe: "P118", szene: 7, iso: tag(1, "18:49"), meta: { ...b, ...pos(48.9, 9.2, 37000, 470, 10, "EDDS"), phase: "cruise", quelle: "sim" }, sendungen: gesendet },
    { id: "b3", gruppe: "P118", szene: 3, iso: tag(1, "19:27"), meta: { ...b, ...pos(53.5, 9.9, 3100, 160, 230, "EDDH"), phase: "approach", quelle: "sim" }, sendungen: fall === "fehler" ? fehlschlag : undefined },
    { id: "b4", gruppe: "P118", szene: 5, iso: tag(1, "19:31"), meta: { ...b, ...pos(53.6, 10.0, 53, 120, 230, "EDDH"), phase: "landing", quelle: "sim" } },
    { id: "c1", gruppe: "P9", szene: 0, iso: tag(5, "09:14"), meta: { ...c, ...pos(48.69, 9.22, 0, 14, 0, "EDDS"), phase: "taxi_out", quelle: "sim" }, sendungen: gesendet },
    { id: "c2", gruppe: "P9", szene: 2, iso: tag(5, "09:52"), meta: { ...c, ...pos(46, 7, 41000, 430, 200, "LSGG"), phase: "cruise", quelle: "sim" }, sendungen: gesendet },
    { id: "c3", gruppe: "P9", szene: 8, iso: tag(5, "10:33"), meta: { ...c, ...pos(43.9, 7.5, 6800, 250, 210, "LFMN"), phase: "descent", quelle: "sim" }, sendungen: gesendet },
    { id: "c4", gruppe: "P9", szene: 9, iso: tag(5, "10:41"), meta: { ...c, ...pos(43.7, 7.3, 1900, 150, 40, "LFMN"), phase: "approach", quelle: "sim" }, sendungen: gesendet },
    { id: "c5", gruppe: "P9", szene: 5, iso: tag(5, "10:44"), meta: { ...c, ...pos(43.66, 7.21, 12, 130, 40, "LFMN"), phase: "landing", quelle: "sim" }, sendungen: gesendet },
    { id: "o1", gruppe: "ohne-flug", szene: 6, iso: tag(8, "21:10"), meta: { quelle: "keine", ...flug(null, null, null, null, "", ""), flugzeug_titel: null, flugzeug_icao: null, simulator: null, breite: null, laenge: null, hoehe_ft: null, gs_kt: null, kurs: null, phase: null, naechster_flugplatz: null } },
    { id: "o2", gruppe: "ohne-flug", szene: 7, iso: tag(8, "21:12"), meta: { quelle: "keine", ...flug(null, null, null, null, "", ""), flugzeug_titel: null, flugzeug_icao: null, simulator: null, breite: null, laenge: null, hoehe_ft: null, gs_kt: null, kurs: null, phase: null, naechster_flugplatz: null } },
  ];
  return z;
}

let zeilen = datensatz();
const eintrag = (r: Zeile) => ({
  id: r.id, gruppe: r.gruppe, pfad: `${r.gruppe}/${r.id}.jpg`, vorschau_pfad: `${r.gruppe}/${r.id}.vorschau.jpg`, quelle_pfad: null,
  groesse: r.bytes ?? 1_800_000, vorschau_groesse: 30_000, quelle_groesse: 0, belegt_bytes: (r.bytes ?? 1_800_000) + 30_000 + 2_000,
  dateizeit_ms: r.iso ? Date.parse(r.iso) : null,
  sidecar: { fassung: 1, bild_id: r.id, text: r.text ?? "", favorit: r.favorit ?? false, meta: { fassung: 1, zeit_utc: r.iso, ...r.meta }, breite_px: 1600, hoehe_px: 1000, sha256: r.id, erstellt_ms: 1, sendungen: r.sendungen ?? [] },
  sidecar_fehler: null, vollstaendig: true,
});
const eingangFund = () => fall === "fund" && !w.__fundErledigt
  ? [{ fund_id: "f1", pfad: "/Users/t/X-Plane 12/Output/screenshots/A320 - 2026-11-07 14.40.00.png", dateizeit_ms: Date.now(), groesse: 2_400_000, format: "png", uebernehmbar: true, px_breite: 2560, px_hoehe: 1440, quelle: "/Users/t/X-Plane 12/Output/screenshots", art: "standard", sim_vordergrund: null, meta_basis: { ...pos(47.3, 8.5, 5200, 210, 150, "LSZH"), ...flug("P412", "GSG412", "EDDF", "LSZH"), phase: "descent", zeit_utc: new Date().toISOString() } }]
  : [];

const quellen = () => fall === "leer" || fall === "leerein"
  ? [{ pfad: "/Users/t/X-Plane 12/Output/screenshots", aktiv: true, original_entfernen: false, art: "standard", beobachtet: true, wird_geprueft: false, fehler: null, altbestand: fall === "leer" ? 142 : 0 }]
  : [{ pfad: "/Users/t/X-Plane 12/Output/screenshots", aktiv: true, original_entfernen: false, art: "standard", beobachtet: true, wird_geprueft: false, fehler: null, altbestand: 0 },
     { pfad: "D:\\Captures", aktiv: false, original_entfernen: true, art: "hand", beobachtet: false, wird_geprueft: false, fehler: null, altbestand: 0 }];

const einstellungen: Record<string, string> = fall === "leer" ? {} : { "aeroacars.bilder.einwilligung.17": "1" };
let kontingent = { rest: fall === "kontingent0" ? 0 : 9, grenze: 12, retry_after: fall === "kontingent0" ? 1500 : 0 };
let nachfrage = 0;

export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  log(cmd, args);
  const a = (args ?? {}) as Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
  switch (cmd) {
    case "paket_info": return { faehigkeiten: ["bilder-1"] } as T;
    case "ui_state_get_all": return { ...einstellungen } as T;
    case "ui_state_set": if (a.value === null) delete einstellungen[a.key]; else einstellungen[a.key] = a.value; return undefined as T;
    case "bilder_liste":
      return {
        eintraege: zeilen.map(eintrag), belegt_bytes: zeilen.length * 1_832_000,
        quellen: quellen(), tastenkuerzel: { text: "Strg+Shift+S", verfuegbar: q.get("sim") === "msfs", aktiv: true, fehler: null, yoke_verfuegbar: q.get("sim") === "msfs", yoke: w.__yoke ?? null },
        aufnahme_verfuegbar: q.get("sim") === "msfs", simulator_taste: q.get("taste") === "keine" ? null : { simulator: "xplane", text: "Shift+Leertaste" }, funde: eingangFund(),
        altbestand: a.mitAltbestand ? Array.from({ length: fall === "leer" ? 142 : 0 }, (_, i) => ({ pfad: `/x/${i}.png`, dateizeit_ms: Date.now() - i * 1000, groesse: 1, format: "png", quelle: "/x" })) : [],
      } as T;
    case "bild_lesen": {
      const pfad = String(a.pfad);
      if (pfad.includes(".quelle.")) return await szeneAlsBytes(2, 2560, 1440, "image/png") as T;
      const id = pfad.split("/").pop()!.split(".")[0];
      const z = zeilen.find((r) => r.id === id);
      const klein = pfad.includes("vorschau");
      return await szeneAlsBytes(z?.szene ?? 0, klein ? 320 : 640, klein ? 200 : 400, "image/jpeg") as T;
    }
    case "bild_uebernehmen":
      if (a.modus === "verwerfen") { w.__fundErledigt = true; return null as T; }
      w.__fundErledigt = true;
      return { id: "neu1", gruppe: a.gruppe, quelle_pfad: `${a.gruppe}/neu1.quelle.png`, format: "png", groesse: 1, dateizeit_ms: Date.now(), modus: a.modus, original_entfernt: a.modus === "verschieben" } as T;
    case "bild_schreiben": {
      w.__geschrieben = a;
      const r = zeilen.find((x) => x.id === a.id);
      const sc = a.sidecar as Record<string, any> | undefined; // eslint-disable-line @typescript-eslint/no-explicit-any
      if (r && sc) { r.text = sc.text; r.favorit = sc.favorit; r.meta = { ...sc.meta }; r.iso = sc.meta?.zeit_utc ?? r.iso; r.sendungen = sc.sendungen; }
      else if (sc) zeilen.push({ id: a.id, gruppe: a.gruppe, szene: 4, iso: sc.meta?.zeit_utc ?? null, meta: sc.meta, text: sc.text, sendungen: sc.sendungen });
      return undefined as T;
    }
    case "bild_senden": {
      await new Promise((ok) => setTimeout(ok, 400));
      if (fall === "offline") throw { code: "live_nicht_erreichbar", message: "offline" };
      if (fall === "unklar") throw { code: "live_antwort_fehlt", message: "timeout" };
      if (fall === "kontingent0") throw { code: "live_kontingent", message: "429", details: { retry_after: "1500" } };
      kontingent = { ...kontingent, rest: Math.max(0, kontingent.rest - a.bilder.length) };
      return { sendung_id: a.sendungId, bilder: a.bilder.map((b: any) => ({ bild_id: b.bild_id, status: "angenommen" })), kontingent } as T; // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    case "bild_status": {
      nachfrage++;
      return { bilder: (a.sendungId ? zeilen.flatMap((r) => (r.sendungen ?? []).filter((s: any) => s.sendung_id === a.sendungId).map(() => ({ bild_id: r.id, status: nachfrage > 1 ? "gesendet" : "angenommen" }))) : []), kontingent } as T; // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    case "bild_quellen_finden": return [
      { pfad: "/Users/t/X-Plane 12/Output/screenshots", art: "xplane_standard", simulator: "xplane", existiert: true, vorschlag_modus: "verschieben" },
      { pfad: "C:\\Users\\t\\Videos\\Captures", art: "captures", simulator: "msfs", existiert: true, vorschlag_modus: "kopieren" },
    ] as T;
    case "bild_quellen_setzen": if ("yokeTaste" in a) w.__yoke = a.yokeTaste; return { quellen: quellen(), abgebrochen: !!a.ordnerWaehlen } as T;
    case "bild_loeschen": { const weg = new Set((a.bilder as Array<{ id: string }>).map((b) => b.id)); zeilen = zeilen.filter((r) => !weg.has(r.id)); return [] as T; }
    case "bild_hinzufuegen": return { abgebrochen: true, ergebnisse: [] } as T;
    case "bild_taste_lernen": return fall === "yokezeit" ? (null as T) : { geraet_name: "Honeycomb Alpha Yoke", hersteller_id: 1, produkt_id: 2, knopf: 7 } as T;
    case "bild_ordner_zeigen": case "bild_export": case "bild_aufnehmen": return undefined as T;
    case "flight_get_track": return Array.from({ length: 40 }, (_, i) => [8.56 + i * 0.012, 50.03 - i * 0.065]) as T;
    case "bild_kontext": return { staende: [], spur: [] } as T;
    default: return undefined as T;
  }
}
export const bildBytesHolen = async (pfad: string): Promise<Uint8Array> => new Uint8Array(await invoke<ArrayBuffer>("bild_lesen", { pfad }));
