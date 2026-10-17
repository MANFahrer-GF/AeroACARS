// Kabinenansagen (Beta): Ablauf der Wiedergabe.
//
// Rust entscheidet, WANN eine Ansage spielt (Planer + Warteschlange), und
// schickt das Ereignis `kabine-ansage` mit den fertigen Tondateien. Hier
// wird die Reihenfolge gesteuert: Sprache für Sprache nacheinander, mit
// Pegel für die Live-Anzeige der Ansage-Seite. Den Ton selbst spielt Rust
// (`kabinen_ton_spielen`) auf dem gewählten Ausgabegerät — Lautsprecher,
// Headset …; das Fenster könnte das Gerät nicht wählen. Auch der
// 10.000-ft-Chime entsteht dort aus zwei Sinustönen.

import { formatIpcError, invoke, isTauri, listen } from "./ipc";

export type Rolle = "kapitaen" | "kabine" | "kommando" | "chime";

export interface Teil {
  sprache: string;
  text: string;
  datei: string;
  dauer_s: number;
}

export interface Gespielt {
  slot: string;
  rolle: Rolle;
  zeit: number;
  teile: Teil[];
  lautstaerke: number;
}

export interface Einstellungen {
  an: boolean;
  /** Airline + Englisch oder Airline + Deutsch; Stimmen und Tagesform
   *  bestimmt der GSG-Server. */
  zweite_sprache: "airline" | "airline_de";
  echter_name: boolean;
  chimes: boolean;
  kommandos: boolean;
  lautstaerke: number;
  /** Ausgabegerät (Kennung aus `kabinen_ausgabegeraete`), `null` = Systemstandard. */
  ausgabe: string | null;
}

export interface Geraet {
  id: string;
  name: string;
}

/** Warum gerade keine Ansagen kommen (aus Rust, `Grund`). */
export type Grund =
  | { grund: "aus" | "replay" | "fracht" | "keine_pax" | "keine_passagiere" | "beta_pruefung" | "nicht_freigeschaltet" }
  | { grund: "texte_fehlen" | "ton_fehlt"; detail: string };

export interface Zustand {
  beta: boolean | null;
}

export interface Ueberblick {
  verlauf: Gespielt[];
  wartend: string[];
  sprachen: string[];
  beta: boolean | null;
  grund: Grund | null;
}

/** Was die Seite live anzeigt. */
export interface Wiedergabe {
  ansage: Gespielt | null;
  /** Index des Teils (Sprache), der gerade spricht. */
  teil: number;
  /** 0 … 1, geglätteter Pegel der laufenden Stimme. */
  pegel: number;
  /** Letzte Pegelwerte für die Wellenform, älteste zuerst. */
  verlauf: number[];
  /** Letzter Wiedergabefehler (Gerät belegt, abgezogen …), bis eine
   *  Ansage wieder klappt — die Seite nennt ihn statt still zu bleiben. */
  fehler: string | null;
}

const LEER: Wiedergabe = { ansage: null, teil: 0, pegel: 0, verlauf: [], fehler: null };
const VERLAUF_LAENGE = 64;

let zustand: Wiedergabe = LEER;
const hoerer = new Set<() => void>();

function setzen(neu: Partial<Wiedergabe>): void {
  zustand = { ...zustand, ...neu };
  hoerer.forEach((h) => h());
}

/** Für `useSyncExternalStore`. */
export function abonnieren(h: () => void): () => void {
  hoerer.add(h);
  return () => hoerer.delete(h);
}
export function aktuell(): Wiedergabe {
  return zustand;
}

/** Pegelwerte je Sekunde aus `kabinen_ton_huellkurve` (Rust `HUELLE_JE_S`). */
const HUELLE_JE_S = 30;

const schlange: Gespielt[] = [];
let laeuft = false;
/** Steigt bei jedem Stopp: eine laufende Abarbeitung bricht dann ab. */
let durchgang = 0;
/** Nach „Ausschalten“: keine Ansage aus Rust mehr annehmen, auch keine,
 *  die ein schon laufender Takt noch schickt (Codex-QS 2). Erst
 *  `freigeben()` beim Einschalten hebt das auf. */
let gesperrt = false;

function pause(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Spielt eine Tondatei auf dem Ausgabegerät; der Pegel für die Anzeige
 *  läuft aus der vorab gelesenen Hüllkurve mit der Uhr mit. */
async function spielen(datei: string, lautstaerke: number, meiner: number): Promise<void> {
  const huelle = await invoke<number[]>("kabinen_ton_huellkurve", { datei }).catch(() => [] as number[]);
  // Kam „Ausschalten“, während die Hüllkurve lud, nicht mehr starten: Rust
  // sähe den Stopp nicht mehr, er war vor diesem Aufruf (Codex-QS beta.2).
  if (meiner !== durchgang) return;
  const start = performance.now();
  let fertig = false;
  const messen = () => {
    if (fertig) return;
    const pegel = huelle[Math.floor(((performance.now() - start) / 1000) * HUELLE_JE_S)] ?? 0;
    const glatt = zustand.pegel * 0.6 + pegel * 0.4;
    setzen({ pegel: glatt, verlauf: [...zustand.verlauf, glatt].slice(-VERLAUF_LAENGE) });
    requestAnimationFrame(messen);
  };
  requestAnimationFrame(messen);
  try {
    await invoke("kabinen_ton_spielen", { datei, lautstaerke });
  } finally {
    fertig = true;
    setzen({ pegel: 0 });
  }
}

/** Doppel-Chime (hoch, tief) — erzeugt in Rust. */
async function chime(lautstaerke: number): Promise<void> {
  await invoke("kabinen_ton_spielen", { datei: null, lautstaerke });
}

async function abarbeiten(): Promise<void> {
  if (laeuft) return;
  laeuft = true;
  const meiner = durchgang;
  try {
    while (schlange.length > 0 && meiner === durchgang) {
      const a = schlange.shift()!;
      setzen({ ansage: a, teil: 0, verlauf: [] });
      if (a.rolle === "chime") {
        try {
          await chime(a.lautstaerke);
          setzen({ fehler: null });
        } catch (e) {
          console.warn("[kabine] Chime nicht abspielbar", e);
          setzen({ fehler: formatIpcError(e) });
        }
      } else {
        for (let i = 0; i < a.teile.length && meiner === durchgang; i++) {
          setzen({ teil: i });
          try {
            await spielen(a.teile[i].datei, a.lautstaerke, meiner);
            setzen({ fehler: null });
          } catch (e) {
            console.warn("[kabine] Ton nicht abspielbar", a.slot, e);
            setzen({ fehler: formatIpcError(e) });
          }
          if (i < a.teile.length - 1) await pause(700);
        }
      }
      await pause(400);
    }
  } finally {
    laeuft = false;
    setzen({ ansage: null, pegel: 0 });
    // Kam während eines Stopps schon eine neue Ansage (aus- und gleich
    // wieder eingeschaltet), bliebe sie sonst liegen (Codex-QS 3).
    if (schlange.length > 0) void abarbeiten();
  }
}

/** Sofort still: laufende Ansage abbrechen, Wartendes verwerfen. Für
 *  „Ansagen ausschalten“ — wer das nicht möchte, hört ab dem Klick nichts. */
export function stoppen(): void {
  gesperrt = true;
  durchgang += 1;
  schlange.length = 0;
  if (isTauri) void invoke("kabinen_ton_stoppen").catch(() => undefined);
  setzen({ ansage: null, pegel: 0, verlauf: [] });
}

/** Nach dem Einschalten: Ansagen aus Rust wieder annehmen. */
export function freigeben(): void {
  gesperrt = false;
}

/** Eine Ansage einreihen (Ereignis oder Hörprobe). */
export function abspielen(a: Gespielt): void {
  schlange.push(a);
  void abarbeiten();
}

let gestartet = false;
/** Einmal beim Start der App: auf Ansagen aus Rust hören. Nur im Fenster
 *  des Sim-PCs — auf dem Tablet (LAN-Fernbedienung) spielt nichts. */
export function wiedergabeStarten(): void {
  if (gestartet || !isTauri) return;
  gestartet = true;
  listen<Gespielt>("kabine-ansage", (e) => {
    if (!gesperrt) abspielen(e.payload);
  }).catch((e) => {
    // Sonst bliebe die Wiedergabe bis zum Neustart stumm: nach kurzer
    // Pause erneut anmelden.
    console.warn("[kabine] Anmelden fehlgeschlagen, neuer Versuch", e);
    gestartet = false;
    setTimeout(wiedergabeStarten, 5000);
  });
}

export const einstellungenLesen = () => invoke<Einstellungen>("kabinen_einstellungen_lesen");
export const einstellungenSetzen = (einstellungen: Einstellungen) =>
  invoke<Einstellungen>("kabinen_einstellungen_setzen", { einstellungen });
export const zustandLesen = () => invoke<Zustand>("kabinen_zustand");
export const probe = (rolle: Rolle, sprache: string) => invoke<Gespielt>("kabinen_probe", { rolle, sprache });
export const ueberblick = () => invoke<Ueberblick | null>("kabinen_ueberblick");
export const ausgabegeraete = () => invoke<Geraet[]>("kabinen_ausgabegeraete");
