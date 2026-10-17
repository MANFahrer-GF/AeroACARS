// Attrappe der IPC-Schicht für die Kabinen-Demo (nur vite.kabine.config.mjs).
// Liefert einen laufenden Flug mit Verlauf und eine sprachähnliche
// Pegelkurve; „Wiedergabe“ wartet nur die Dauer ab (Ton spielt Rust).
export const isTauri = true;
export const formatIpcError = (e: unknown) => String(e);
export async function openExternal(url: string) {
  window.open(url, "_blank");
}

type Cb = (e: { event: string; payload: unknown }) => void;
const hoerer = new Map<string, Cb[]>();
export async function listen(event: string, cb: Cb) {
  hoerer.set(event, [...(hoerer.get(event) ?? []), cb]);
  return () => undefined;
}
export function senden(event: string, payload: unknown) {
  (hoerer.get(event) ?? []).forEach((cb) => cb({ event, payload }));
}

/** Sprachähnliches Signal: Rauschen + Grundton mit Silbenhüllkurve. */
function wav(sekunden: number): ArrayBuffer {
  const rate = 24000;
  const n = Math.floor(rate * sekunden);
  const buf = new ArrayBuffer(44 + n * 2);
  const v = new DataView(buf);
  const str = (o: number, s: string) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  str(0, "RIFF"); v.setUint32(4, 36 + n * 2, true); str(8, "WAVE"); str(12, "fmt ");
  v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, rate, true); v.setUint32(28, rate * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  str(36, "data"); v.setUint32(40, n * 2, true);
  for (let i = 0; i < n; i++) {
    const t = i / rate;
    const silbe = Math.max(0, Math.sin(t * Math.PI * 4.2)) * (0.6 + 0.4 * Math.sin(t * 1.3));
    const s = (Math.sin(2 * Math.PI * 140 * t) * 0.5 + (Math.random() - 0.5) * 0.5) * silbe * 0.6;
    v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, s)) * 32767, true);
  }
  return buf;
}

export const demoUeberblick = {
  verlauf: [
    { slot: "boarding", rolle: "kabine", zeit: Date.now() / 1000 - 2400, lautstaerke: 0.8,
      teile: [{ sprache: "de-DE", text: "Liebe Gäste, herzlich willkommen an Bord. Bitte verstauen Sie Ihr Handgepäck in den Fächern über Ihnen und nehmen Sie zügig Platz.", datei: "a.wav", dauer_s: 6 }] },
    { slot: "kapitaen_begruessung", rolle: "kapitaen", zeit: Date.now() / 1000 - 1500, lautstaerke: 0.8,
      teile: [{ sprache: "de-DE", text: "Guten Abend, liebe Gäste, hier spricht Ihr Kapitän Markus Brandt. Wir werden etwa eine Stunde und fünfundfünfzig Minuten unterwegs sein.", datei: "b.wav", dauer_s: 8 }] },
  ],
  wartend: ["reiseflug50"],
  geplant: [
    { slot: "reiseflug80", bereit: true },
    { slot: "kapitaen_sinkflug", bereit: true },
    { slot: "kabine_sinkflug", bereit: false },
    { slot: "nach_der_landung", bereit: false },
    { slot: "aussteigen", bereit: false },
  ],
  sprachen: ["de-DE", "en-GB"],
  beta: true,
  // `?grund=keine_pax` zeigt den Hinweis, warum nichts kommt.
  grund: (() => {
    const g = new URLSearchParams(window.location.search).get("grund");
    return g ? (g === "texte_fehlen" || g === "ton_fehlt" ? { grund: g, detail: "api 503: ausgelastet" } : { grund: g }) : null;
  })(),
};

/** Silbenhüllkurve wie `wav()`, 30 Werte je Sekunde (Rust `huellkurve`). */
function huelle(sekunden: number): number[] {
  return Array.from({ length: Math.floor(sekunden * 30) }, (_, i) => {
    const t = i / 30;
    return Math.min(1, Math.max(0, Math.sin(t * Math.PI * 4.2)) * (0.6 + 0.4 * Math.sin(t * 1.3)) * 0.9);
  });
}
let gestoppt = 0;

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "kabinen_ueberblick":
      return demoUeberblick as T;
    case "kabinen_audio":
      return wav(String(args?.datei).startsWith("lang") ? 6 : 3.5) as T;
    case "kabinen_einstellungen_lesen":
    case "kabinen_einstellungen_setzen":
      return (args?.einstellungen ?? {
        an: true, zweite_sprache: "airline", echter_name: false,
        chimes: true, kommandos: true, lautstaerke: 0.8,
      }) as T;
    case "kabinen_zustand":
      return { beta: true } as T;
    case "kabinen_ausgabegeraete":
      return [
        { id: "coreaudio:BuiltInSpeakerDevice", name: "MacBook-Lautsprecher" },
        { id: "coreaudio:Headset", name: "Jabra Evolve2 65" },
      ] as T;
    case "kabinen_ton_huellkurve":
      return huelle(String(args?.datei).startsWith("lang") ? 6 : 3.5) as T;
    case "kabinen_ton_stoppen":
      gestoppt += 1;
      return null as T;
    case "kabinen_ton_spielen": {
      const stand = gestoppt;
      const ms = args?.datei == null ? 2100 : String(args.datei).startsWith("lang") ? 6000 : 3500;
      const ende = Date.now() + ms;
      while (Date.now() < ende && gestoppt === stand) await new Promise((r) => setTimeout(r, 50));
      return null as T;
    }
    default:
      return null as T;
  }
}
