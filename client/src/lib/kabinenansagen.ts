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
  /** Startzeit jedes Satzes in s (vom Ton-Server); fehlt sie, schätzt die
   *  Seite die Satzgrenzen aus der Textlänge. */
  saetze?: number[] | null;
}

export interface Gespielt {
  /** Kennung für die Rückmeldung (`kabinen_wiedergabe_ende`); 0 = Hörprobe. */
  id: number;
  slot: string;
  rolle: Rolle;
  zeit: number;
  teile: Teil[];
  /** Nur angehört (Vorhören, Nochmal, Hörprobe): zählt nicht als gespielt. */
  vorschau: boolean;
  /** Ausgang: gespielt | abgebrochen | fehler | unbekannt | stumm | entfallen;
   *  null = ausgegeben bzw. läuft. */
  ergebnis: string | null;
  /** Die Wiedergabe hat begonnen (erst dann „läuft“). */
  gestartet?: boolean;
  /** Warum sie entfiel (zu_wenig_zeit, uebersprungen, ton_nicht_fertig, zu_spaet,
   *  keine_stimme, kein_kurztext). */
  grund?: string | null;
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
  /** Kapitän männlich/weiblich; „auto“ = der Server würfelt (bei echtem
   *  Namen: „Kapitän“). Die Stimme folgt dem Geschlecht der Crew. */
  kapitaen_geschlecht: "auto" | "m" | "w";
  /** Klang wie aus dem Bordlautsprecher (ab Werk an). */
  lautsprecher_klang: boolean;
  /** Abgewählte Ansagen (Slot-Namen), z. B. weil GSX oder das Flugzeug sie spielt. */
  aus: string[];
}

export interface Geraet {
  id: string;
  name: string;
}

/** Warum gerade gar keine Ansagen kommen (aus Rust, `Grund`). */
export type Grund =
  | {
      grund:
        | "aus"
        | "replay"
        | "fracht"
        | "keine_pax"
        | "keine_passagiere"
        | "beta_pruefung"
        | "nicht_freigeschaltet"
        | "kontingent"
        | "sim_angehalten";
    }
  | { grund: "beta_nicht_erreichbar"; detail: number };

export interface Zustand {
  beta: boolean | null;
  kontingent_erschoepft: boolean | null;
}

export interface Speicher {
  bytes: number;
  ordner: string;
}

/** Wo eine Ansage steht (Rust `Stand`); `um` = UTC-Epoch-Sekunden. */
export type Stand =
  | { art: "text_ab"; um: number | null; nach_abheben?: boolean }
  | { art: "text_wird_geschrieben" }
  | { art: "text_fehler"; um: number; meldung: string }
  | { art: "text_aufgegeben"; meldung: string }
  | { art: "ton_wird_erzeugt"; fertig: number; von: number }
  | { art: "ton_wartet"; um: number; meldung: string }
  | { art: "ton_aufgegeben"; meldung: string }
  | { art: "bereit" };

export type Wartet = "andere_ansage" | "pause" | "ruhephase" | "ton" | "sim_angehalten";

export interface TeilAnsicht {
  sprache: string;
  text: string | null;
  ton: boolean;
  keine_stimme: boolean;
}

/** Eine fällige oder noch kommende Ansage. */
export interface Geplant {
  slot: string;
  rolle: Rolle;
  faellig: boolean;
  wartet: Wartet | null;
  /** Voraussichtliche Uhrzeit (UTC-Epoch), nur wo es eine Quelle gibt. */
  wann: number | null;
  stand: Stand;
  teile: TeilAnsicht[];
  /** Nur Reiseflug 80: zweiter_service | ende_bordverkauf (vom GSG-Server). */
  art?: string | null;
}

export type TeilStand =
  | { art: "entfaellt" | "laeuft" | "fertig" }
  | { art: "offen"; um: number | null }
  | { art: "fehler"; um: number; meldung: string }
  | { art: "aufgegeben"; meldung: string };

export type Freigabe =
  | { art: "ja" | "nein" | "pruefung" | "kontingent" }
  | { art: "nicht_erreichbar"; um: number };

export interface Ablauf {
  freigabe: Freigabe;
  abflug: TeilStand;
  ankunft: TeilStand;
  ton_fertig: number;
  ton_gesamt: number;
}

export interface Profil {
  flugart: "linie" | "bizjet" | "aus" | null;
  muster: string;
  flugzeit_min: number | null;
  kurzstrecke: boolean;
  flugzeit_bekannt: boolean;
  sprachen_ohne_stimme: string[];
}

export interface Ueberblick {
  verlauf: Gespielt[];
  geplant: Geplant[];
  sprachen: string[];
  beta: boolean | null;
  grund: Grund | null;
  ablauf: Ablauf;
  profil: Profil;
  /** Eine Ansage oder ein Vorhören läuft: Anhören-Knöpfe sperren. */
  belegt: boolean;
  /** Stumm (nur bis zum Neustart): Fälliges wird übersprungen, nicht nachgeholt. */
  stumm: boolean;
  /** 0 … 1, wirkt sofort, auch auf eine laufende Ansage. */
  lautstaerke: number;
  /** Name des Kapitäns auf diesem Flug (vom GSG-Server), sonst null. */
  kapitaen?: string | null;
  /** Hinweis an den Piloten, z. B. „anschnallzeichen“ (Zeichen im Reiseflug noch an). */
  hinweis?: string | null;
}

/** Was die Seite live anzeigt. */
export interface Wiedergabe {
  ansage: Gespielt | null;
  /** Index des Teils (Sprache), der gerade spricht. */
  teil: number;
  /** Seit wann dieser Teil spielt (performance.now()), für die Untertitel. */
  teilStart: number;
  /** 0 … 1, geglätteter Pegel der laufenden Stimme. */
  pegel: number;
  /** Letzte Pegelwerte für die Wellenform, älteste zuerst. */
  verlauf: number[];
  /** Letzter Wiedergabefehler (Gerät belegt, abgezogen …), bis eine
   *  Ansage wieder klappt — die Seite nennt ihn statt still zu bleiben. */
  fehler: string | null;
}

const LEER: Wiedergabe = { ansage: null, teil: 0, teilStart: 0, pegel: 0, verlauf: [], fehler: null };
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
/** Stumm (Knopf oben auf der Seite): Was Rust kurz vor dem Stummschalten
 *  noch geschickt hat, wird nicht gespielt, sondern als „stumm“ gemeldet
 *  (Codex beta.6: Fenster zwischen Ausgeben und Ereignis). */
let stummLokal = false;

function pause(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Spielt eine Tondatei auf dem Ausgabegerät; der Pegel für die Anzeige
 *  läuft aus der vorab gelesenen Hüllkurve mit der Uhr mit. */
async function spielen(datei: string, meiner: number): Promise<void> {
  const huelle = await invoke<number[]>("kabinen_ton_huellkurve", { datei }).catch(() => [] as number[]);
  // Kam „Ausschalten“, während die Hüllkurve lud, nicht mehr starten: Rust
  // sähe den Stopp nicht mehr, er war vor diesem Aufruf (Codex-QS beta.2).
  if (meiner !== durchgang) return;
  const start = performance.now();
  setzen({ teilStart: start });
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
    // Die Lautstärke setzt Rust selbst (eine Quelle, wirkt auch mitten in
    // der Ansage).
    await invoke("kabinen_ton_spielen", { datei });
  } finally {
    fertig = true;
    setzen({ pegel: 0 });
  }
}

/** Doppel-Chime (hoch, tief) — erzeugt in Rust. */
async function chime(): Promise<void> {
  await invoke("kabinen_ton_spielen", { datei: null });
}

/** Meldet Rust, wie eine Wiedergabe endete: Erst dann steht die Ansage
 *  in „Bisher“ als gespielt, und die nächste darf sofort beginnen. */
function melden(a: Gespielt, ergebnis: "gespielt" | "abgebrochen" | "fehler" | "stumm", versuch = 0): void {
  if (!isTauri || a.id === 0) return;
  // Geht die Rückmeldung verloren, bliebe die Ansage ohne Ausgang und ein
  // Vorhören sperrte bis zur geschätzten Dauer: kurz wiederholen (Rust
  // nimmt Doppelte nicht zweimal, Codex-QS beta.5).
  void invoke("kabinen_wiedergabe_ende", { id: a.id, ergebnis }).catch(() => {
    if (versuch < 3) setTimeout(() => melden(a, ergebnis, versuch + 1), 1000);
  });
}

async function abarbeiten(): Promise<void> {
  if (laeuft) return;
  laeuft = true;
  const meiner = durchgang;
  try {
    while (schlange.length > 0 && meiner === durchgang) {
      const a = schlange.shift()!;
      setzen({ ansage: a, teil: 0, verlauf: [] });
      // Erst jetzt „läuft“ sie in „Bisher“ (nicht schon beim Ausgeben in Rust);
      // abwarten, damit Rust es vor dem ersten Ton weiß (Codex beta.6).
      // Höchstens 2 s: Antwortet Rust nicht, spielt sie trotzdem (Codex-Nachprüfung).
      if (isTauri && !a.vorschau && a.id !== 0) {
        await Promise.race([invoke("kabinen_wiedergabe_start", { id: a.id }).catch(() => undefined), pause(2000)]);
      }
      // Kam währenddessen Stopp oder Stumm: nicht mehr anfangen (sonst
      // spielte ein Gong noch, Sprachteile prüfen das selbst).
      if (meiner !== durchgang) {
        melden(a, "abgebrochen");
        break;
      }
      let fehlgeschlagen = false;
      if (a.rolle === "chime") {
        try {
          await chime();
          setzen({ fehler: null });
        } catch (e) {
          console.warn("[kabine] Chime nicht abspielbar", e);
          setzen({ fehler: formatIpcError(e) });
          fehlgeschlagen = true;
        }
      } else {
        for (let i = 0; i < a.teile.length && meiner === durchgang; i++) {
          setzen({ teil: i });
          try {
            await spielen(a.teile[i].datei, meiner);
            setzen({ fehler: null });
          } catch (e) {
            console.warn("[kabine] Ton nicht abspielbar", a.slot, e);
            setzen({ fehler: formatIpcError(e) });
            fehlgeschlagen = true;
          }
          if (i < a.teile.length - 1) await pause(700);
        }
      }
      melden(a, meiner !== durchgang ? "abgebrochen" : fehlgeschlagen ? "fehler" : "gespielt");
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
  abbrechen();
}

/** Laufendes abbrechen, Wartendes verwerfen — und Rust melden, dass es
 *  nicht (zu Ende) lief. */
function abbrechen(): void {
  durchgang += 1;
  for (const a of schlange) melden(a, "abgebrochen");
  schlange.length = 0;
  if (isTauri) void invoke("kabinen_ton_stoppen").catch(() => undefined);
  setzen({ ansage: null, pegel: 0, verlauf: [] });
}

/** Vorhören oder Nochmal: eine fertige kommende Ansage (`id` leer) oder
 *  eine aus „Bisher“ (`id`). Zählt nicht als gespielt; Rust hält derweil
 *  fällige Ansagen zurück. */
export async function anhoeren(slot: string, id?: number): Promise<void> {
  abspielen(await invoke<Gespielt>("kabinen_anhoeren", { slot, id: id ?? null }));
}

/** Anhören beenden — ohne die Ansagen auszuschalten. */
export function anhoerenStoppen(): void {
  abbrechen();
}

/** Stumm schalten: sofort still (laufende Ansage bricht ab); Rust verbucht
 *  alles, was bis zum Aufheben fällig wird, als „stumm übersprungen“. */
export async function stummSetzen(stumm: boolean): Promise<void> {
  stummLokal = stumm;
  if (stumm) abbrechen();
  await invoke("kabinen_stumm", { stumm });
}

/** Lautstärke 0 … 1: wirkt sofort, auch mitten in einer Ansage, und gilt
 *  dauerhaft (derselbe Wert wie in den Einstellungen). Beim Ziehen nur
 *  der Wert, beim Loslassen auch die Einstellungsdatei (`speichern`). */
export const lautstaerkeSetzen = (lautstaerke: number, speichern: boolean) =>
  invoke<void>("kabinen_lautstaerke", { lautstaerke, speichern });

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
    // Kam sie nach „Ausschalten“ noch an: nicht spielen, aber melden.
    if (gesperrt) melden(e.payload, "abgebrochen");
    else if (stummLokal) melden(e.payload, "stumm");
    else abspielen(e.payload);
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
export const erneut = () => invoke<void>("kabinen_erneut");
export const speicherLesen = () => invoke<Speicher>("kabinen_speicher");
export const ausgabegeraete = () => invoke<Geraet[]>("kabinen_ausgabegeraete");
