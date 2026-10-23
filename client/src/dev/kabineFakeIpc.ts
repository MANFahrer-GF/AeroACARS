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

const jetzt = Math.floor(Date.now() / 1000);
const q = new URLSearchParams(window.location.search);
/** `?fall=` reiseflug (Standard, Flug AIB 436 wie bei Thomas), fehler, boden. */
const fall = q.get("fall") ?? "reiseflug";
const sprachen = ["fr-FR", "de-DE"];
const teile = (fr: string | null, de: string | null, ton = true) => [
  { sprache: "fr-FR", text: fr, ton: ton && fr !== null, keine_stimme: false },
  { sprache: "de-DE", text: de, ton: ton && de !== null, keine_stimme: false },
];
const gespielt = (id: number, slot: string, rolle: string, vor: number, de: string, ergebnis: string | null = "gespielt") => ({
  id, slot, rolle, zeit: jetzt - vor, vorschau: false, ergebnis, gestartet: true, grund: null as string | null,
  teile: [
    { sprache: "fr-FR", text: "Bonsoir à tous. " + de, datei: "f/lang-a.wav", dauer_s: 6 },
    { sprache: "de-DE", text: de, datei: "f/b.wav", dauer_s: 3.5 },
  ],
});

const plan: Record<string, unknown[]> = {
  reiseflug: [
    { slot: "reiseflug50", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 32 * 60, stand: { art: "bereit" },
      teile: teile("Ici le commandant. Nous avons passé la moitié du vol.", "Hier der Kapitän. Wir haben die Hälfte geschafft, alles läuft wie geplant.") },
    { slot: "reiseflug80", rolle: "kabine", faellig: false, wartet: null, wann: jetzt + 95 * 60, stand: { art: "bereit" }, art: "ende_bordverkauf",
      teile: teile("Nous servons maintenant le petit-déjeuner.", "Wir servieren jetzt das Frühstück.") },
    { slot: "kapitaen_sinkflug", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 140 * 60, stand: { art: "text_ab", um: jetzt + 120 * 60 }, teile: teile(null, null) },
    { slot: "kabine_sinkflug", rolle: "kabine", faellig: false, wartet: null, wann: jetzt + 140 * 60, stand: { art: "text_ab", um: jetzt + 120 * 60 }, teile: teile(null, null) },
    { slot: "chime10000_sinkflug", rolle: "chime", faellig: false, wartet: null, wann: null, stand: { art: "bereit" }, teile: [] },
    { slot: "kommando_landung_vorbereiten", rolle: "kommando", faellig: false, wartet: null, wann: null, stand: { art: "bereit" },
      teile: [{ sprache: "en-GB", text: "Cabin crew, prepare the cabin for landing.", ton: true, keine_stimme: false }] },
    { slot: "kommando_plaetze_landung", rolle: "kommando", faellig: false, wartet: null, wann: null, stand: { art: "ton_wartet", um: jetzt + 25, meldung: "Piper-Server antwortet 503 Service Unavailable" },
      teile: [{ sprache: "en-GB", text: "Cabin crew, take your seats for landing.", ton: false, keine_stimme: false }] },
    { slot: "nach_der_landung", rolle: "kabine", faellig: false, wartet: null, wann: null, stand: { art: "text_ab", um: jetzt + 120 * 60 }, teile: teile(null, null) },
    { slot: "kommando_tueren_entschaerfen", rolle: "kommando", faellig: false, wartet: null, wann: null, stand: { art: "ton_wird_erzeugt", fertig: 0, von: 1 },
      teile: [{ sprache: "en-GB", text: "Cabin crew, disarm doors and cross-check.", ton: false, keine_stimme: false }] },
    { slot: "aussteigen", rolle: "kabine", faellig: false, wartet: null, wann: null, stand: { art: "text_ab", um: jetzt + 120 * 60 }, teile: teile(null, null) },
  ],
  fehler: [
    { slot: "reiseflug25", rolle: "kabine", faellig: true, wartet: "ton", wann: null, stand: { art: "ton_wartet", um: jetzt + 30, meldung: "Piper-Server antwortet 429 Too Many Requests" },
      teile: teile("Le service commence.", "Der Service beginnt.") },
    { slot: "reiseflug50", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 20 * 60, stand: { art: "ton_aufgegeben", meldung: "Piper-Server antwortet 400 Bad Request" },
      teile: teile("Ici le commandant.", "Hier der Kapitän.") },
    { slot: "kapitaen_sinkflug", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 70 * 60, stand: { art: "text_aufgegeben", meldung: "api 402: budget" }, teile: teile(null, null) },
  ],
  boden: [
    { slot: "boarding", rolle: "kabine", faellig: true, wartet: "andere_ansage", wann: null, stand: { art: "bereit" }, teile: teile("Bienvenue à bord.", "Willkommen an Bord.") },
    { slot: "begruessung_tueren_zu", rolle: "kabine", faellig: false, wartet: null, wann: null, stand: { art: "ton_wird_erzeugt", fertig: 1, von: 2 }, teile: teile("Bienvenue.", "Willkommen.") },
    { slot: "sicherheit", rolle: "kabine", faellig: false, wartet: null, wann: null, stand: { art: "text_wird_geschrieben" }, teile: teile(null, null) },
    { slot: "chime10000_steigflug", rolle: "chime", faellig: false, wartet: null, wann: null, stand: { art: "bereit" }, teile: [] },
  ],
};

export const demoUeberblick = {
  verlauf:
    fall === "boden"
      ? []
      : [
          gespielt(1, "kapitaen_begruessung", "kapitaen", 5400, "Willkommen an Bord, hier spricht Ihr Kapitän Thomas Kant. Wir fliegen fünf Stunden und vier Minuten nach Perth."),
          gespielt(2, "anschnallzeichen_aus", "kabine", 4200, "Das Anschnallzeichen ist aus. Gleich kommen wir mit Getränken zu Ihnen.", "abgebrochen"),
          gespielt(3, "reiseflug25", "kabine", 600, "Der Service beginnt. Wir reichen Ihnen ein Menü und Getränke."),
          ...(fall === "fehler" ? [{ ...gespielt(5, "reiseflug50", "kapitaen", 300, "", "entfallen"), teile: [], grund: "ton_nicht_fertig" }] : []),
          ...(fall === "stumm" ? [gespielt(4, "reiseflug50", "kapitaen", 120, "Hier der Kapitän. Wir haben die Hälfte geschafft.", "stumm")] : []),
        ],
  geplant: (plan[fall] ?? plan.reiseflug).filter((g) => fall !== "stumm" || (g as { slot: string }).slot !== "reiseflug50"),
  sprachen,
  beta: true,
  grund: (() => {
    const g = q.get("grund");
    return g ? (g === "beta_nicht_erreichbar" ? { grund: g, detail: jetzt + 240 } : { grund: g }) : null;
  })(),
  ablauf: {
    freigabe: fall === "fehler" ? { art: "kontingent" } : { art: "ja" },
    abflug: fall === "boden" ? { art: "laeuft" } : { art: "fertig" },
    ankunft: fall === "fehler" ? { art: "aufgegeben", meldung: "api 402: budget" } : { art: "offen", um: jetzt + 120 * 60 },
    ton_fertig: fall === "reiseflug" ? 5 : 1,
    ton_gesamt: fall === "reiseflug" ? 10 : 3,
  },
  profil: {
    flugart: "linie", muster: "A388", flugzeit_min: 304, kurzstrecke: false, flugzeit_bekannt: true,
    sprachen_ohne_stimme: q.get("ohne_stimme") ? ["lv-LV"] : [],
  },
  belegt: false,
  kapitaen: "Tobias Berger",
  hinweis: q.get("hinweis") ? "anschnallzeichen" : null,
  stumm: fall === "stumm",
  lautstaerke: 0.8,
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
      return { ...demoUeberblick } as T;
    case "kabinen_stumm":
      demoUeberblick.stumm = Boolean(args?.stumm);
      return null as T;
    case "kabinen_lautstaerke":
      demoUeberblick.lautstaerke = Number(args?.lautstaerke);
      return null as T;
    case "kabinen_audio":
      return wav(String(args?.datei).startsWith("lang") ? 6 : 3.5) as T;
    case "kabinen_einstellungen_lesen":
    case "kabinen_einstellungen_setzen":
      return (args?.einstellungen ?? {
        an: true, zweite_sprache: "airline", echter_name: false,
        chimes: true, kommandos: true, lautstaerke: 0.8, ausgabe: null,
        kapitaen_geschlecht: "auto", klang: "lautsprecher", aus: ["boarding"], reaktionen: true,
      }) as T;
    case "kabinen_zustand":
      return { beta: true, kontingent_erschoepft: false } as T;
    case "kabinen_speicher":
      return { bytes: 7_400_000, ordner: "~/Library/Caches/aeroacars/kabinenansagen/ton" } as T;
    case "kabinen_anhoeren": {
      const id = args?.id as number | null;
      const g = id ? demoUeberblick.verlauf.find((v) => v.id === id) : null;
      const p = (demoUeberblick.geplant as { slot: string; rolle: string; teile: { sprache: string; text: string | null }[] }[]).find((x) => x.slot === args?.slot);
      return (g
        ? { ...g, id: 900, vorschau: true, ergebnis: null }
        : { id: 901, slot: args?.slot, rolle: p?.rolle ?? "kabine", zeit: jetzt, vorschau: true, ergebnis: null,
            teile: (p?.teile ?? []).map((t, i) => ({ sprache: t.sprache, text: t.text ?? "", datei: i ? "f/b.wav" : "f/lang-a.wav", dauer_s: i ? 3.5 : 6 })) }) as T;
    }
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
    case "kabinen_musik_zustand":
      return { ...demoMusik } as T;
    case "kabinen_musik_lautstaerke":
      demoMusik.lautstaerke = Number(args?.lautstaerke);
      return null as T;
    case "kabinen_boarding_musik":
      demoMusik.an = Boolean(args?.an);
      demoMusik.stand = demoMusik.an ? "laeuft" : "aus";
      return null as T;
    case "kabinen_airlines":
      return [
        { icao: "AFR", name: "Air France" }, { icao: "AUA", name: "Austrian Airlines" }, { icao: "DLH", name: "Lufthansa" },
        { icao: "EWG", name: "Eurowings" }, { icao: "ITY", name: "ITA Airways" }, { icao: "KLM", name: "KLM" }, { icao: "SWR", name: "Swiss" },
      ] as T;
    case "kabinen_musik_ablage":
      return { ordner: "~/Documents/AeroACARS/Boarding-Musik", flug_airline: "DLH", stuecke: demoStuecke } as T;
    case "kabinen_musik_pruefen":
      return 212.4 as T;
    case "kabinen_musik_hinzufuegen": {
      const st = { airline: String(args?.airline), datei: String(args?.quelle).split("/").pop() ?? "", dauer_s: 212.4 };
      demoStuecke.push(st);
      return st as T;
    }
    case "kabinen_musik_entfernen":
      demoStuecke.splice(demoStuecke.findIndex((x) => x.airline === args?.airline && x.datei === args?.datei), 1);
      return null as T;
    default:
      return null as T;
  }
}

/** Boarding-Musik der Demo: `?musik=` laeuft (Standard), lied, kein_lied, eigene, aus. */
const musikFall = q.get("musik") ?? "laeuft";
const demoMusik = {
  an: musikFall !== "aus",
  lautstaerke: 0.2,
  stand: musikFall === "lied" ? "wird_erzeugt" : musikFall === "eigene" ? "bereit" : musikFall,
  quelle: musikFall === "eigene" ? "eigene" : "lied",
  titel: musikFall === "eigene" ? null : "Fly with Lufthansa",
  strecke: "Frankfurt → Hamburg",
  ordner: musikFall === "eigene" ? "DLH" : null,
  stuecke: musikFall === "eigene" ? 2 : 0,
  airline: "DLH",
};
const demoStuecke = [
  { airline: "DLH", datei: "Lufthansa Boarding Theme.mp3", dauer_s: 184.2 },
  { airline: "DLH", datei: "Piano Lounge.m4a", dauer_s: 241 },
  { airline: "KLM", datei: "Blue Skies.mp3", dauer_s: 158.7 },
  { airline: "Alle", datei: "Chill Jazz.flac", dauer_s: 27.5 },
];
