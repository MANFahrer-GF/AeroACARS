// Kabinenansagen (Beta): Wiedergabe im Fenster der App.
//
// Rust entscheidet, WANN eine Ansage spielt (Planer + Warteschlange), und
// schickt das Ereignis `kabine-ansage` mit den fertigen Tondateien. Hier
// wird nur abgespielt: Sprache für Sprache nacheinander, mit Pegelmessung
// für die Live-Anzeige der Ansage-Seite. Der 10.000-ft-Chime wird aus zwei
// Sinustönen erzeugt — keine fremde Tondatei, keine Lizenzfrage.

import { invoke, isTauri, listen } from "./ipc";

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
  zweite_sprache: "keine" | "airline" | "ziel" | "eigene";
  eigene_sprache: string | null;
  anbieter_kapitaen: "piper" | "azure";
  anbieter_kabine: "piper" | "azure";
  anbieter_kommando: "piper" | "azure";
  azure_region: string;
  echter_name: boolean;
  tagesform_kapitaen: string;
  tagesform_kabine: string;
  chimes: boolean;
  kommandos: boolean;
  lautstaerke: number;
}

export interface Zustand {
  beta: boolean | null;
  azure_schluessel: boolean;
}

export interface Ueberblick {
  verlauf: Gespielt[];
  wartend: string[];
  sprachen: string[];
  beta: boolean | null;
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
}

const LEER: Wiedergabe = { ansage: null, teil: 0, pegel: 0, verlauf: [] };
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

let kontext: AudioContext | null = null;
function audio(): AudioContext {
  if (!kontext) kontext = new AudioContext();
  return kontext;
}

const schlange: Gespielt[] = [];
let laeuft = false;
/** Steigt bei jedem Stopp: eine laufende Abarbeitung bricht dann ab. */
let durchgang = 0;
let aktuelleQuelle: AudioBufferSourceNode | null = null;

async function tonLaden(datei: string): Promise<AudioBuffer> {
  const daten = await invoke<ArrayBuffer>("kabinen_audio", { datei });
  return audio().decodeAudioData(daten.slice(0));
}

function pause(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Spielt einen Puffer und misst dabei den Pegel (RMS) für die Anzeige. */
function spielen(puffer: AudioBuffer, lautstaerke: number): Promise<void> {
  const ctx = audio();
  const quelle = ctx.createBufferSource();
  quelle.buffer = puffer;
  const gain = ctx.createGain();
  gain.gain.value = lautstaerke;
  const analyse = ctx.createAnalyser();
  analyse.fftSize = 1024;
  quelle.connect(gain).connect(analyse).connect(ctx.destination);
  const proben = new Float32Array(analyse.fftSize);
  let fertig = false;
  const messen = () => {
    if (fertig) return;
    analyse.getFloatTimeDomainData(proben);
    let summe = 0;
    for (const p of proben) summe += p * p;
    // Sprache liegt bei RMS ~0,05–0,3; auf 0…1 strecken, Lautstärke herausrechnen.
    const rms = Math.sqrt(summe / proben.length) / Math.max(lautstaerke, 0.05);
    const pegel = Math.min(1, rms * 4);
    const glatt = zustand.pegel * 0.6 + pegel * 0.4;
    const verlauf = [...zustand.verlauf, glatt].slice(-VERLAUF_LAENGE);
    setzen({ pegel: glatt, verlauf });
    requestAnimationFrame(messen);
  };
  return new Promise((r) => {
    quelle.onended = () => {
      fertig = true;
      if (aktuelleQuelle === quelle) aktuelleQuelle = null;
      setzen({ pegel: 0 });
      r();
    };
    aktuelleQuelle = quelle;
    quelle.start();
    requestAnimationFrame(messen);
  });
}

/** Doppel-Chime (hoch, tief) — zwei weiche Sinustöne. */
async function chime(lautstaerke: number): Promise<void> {
  const ctx = audio();
  const ton = (frequenz: number, start: number) => {
    const osz = ctx.createOscillator();
    const g = ctx.createGain();
    osz.type = "sine";
    osz.frequency.value = frequenz;
    const t0 = ctx.currentTime + start;
    g.gain.setValueAtTime(0, t0);
    g.gain.linearRampToValueAtTime(0.35 * lautstaerke, t0 + 0.02);
    g.gain.exponentialRampToValueAtTime(0.001, t0 + 1.4);
    osz.connect(g).connect(ctx.destination);
    osz.start(t0);
    osz.stop(t0 + 1.5);
  };
  ton(880, 0);
  ton(698.5, 0.55);
  await pause(2100);
}

async function abarbeiten(): Promise<void> {
  if (laeuft) return;
  laeuft = true;
  const meiner = durchgang;
  try {
    while (schlange.length > 0 && meiner === durchgang) {
      const a = schlange.shift()!;
      setzen({ ansage: a, teil: 0, verlauf: [] });
      if (audio().state === "suspended") await audio().resume();
      if (a.rolle === "chime") {
        await chime(a.lautstaerke);
      } else {
        for (let i = 0; i < a.teile.length && meiner === durchgang; i++) {
          setzen({ teil: i });
          try {
            await spielen(await tonLaden(a.teile[i].datei), a.lautstaerke);
          } catch (e) {
            console.warn("[kabine] Ton nicht abspielbar", a.slot, e);
          }
          if (i < a.teile.length - 1) await pause(700);
        }
      }
      await pause(400);
    }
  } finally {
    laeuft = false;
    setzen({ ansage: null, pegel: 0 });
  }
}

/** Sofort still: laufende Ansage abbrechen, Wartendes verwerfen. Für
 *  „Ansagen ausschalten“ — wer das nicht möchte, hört ab dem Klick nichts. */
export function stoppen(): void {
  durchgang += 1;
  schlange.length = 0;
  try {
    aktuelleQuelle?.stop();
  } catch {
    /* schon zu Ende */
  }
  aktuelleQuelle = null;
  setzen({ ansage: null, pegel: 0, verlauf: [] });
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
  listen<Gespielt>("kabine-ansage", (e) => abspielen(e.payload)).catch((e) => {
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
export const azureSchluesselSetzen = (schluessel: string | null) =>
  invoke<boolean>("kabinen_azure_schluessel_setzen", { schluessel });
export const probe = (rolle: Rolle, sprache: string) => invoke<Gespielt>("kabinen_probe", { rolle, sprache });
export const ueberblick = () => invoke<Ueberblick | null>("kabinen_ueberblick");
