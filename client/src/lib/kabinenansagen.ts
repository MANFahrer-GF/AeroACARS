// Kabinenansagen (Beta): Ablauf der Wiedergabe.
//
// Rust entscheidet, WANN eine Ansage spielt (Planer + Warteschlange), und
// spielt sie seit 1b (Rundumschlag 22.10.2026) auch selbst, Sprache für
// Sprache, samt Ende. Die Seite zeigt sie über `kabine-ansage`,
// `kabine-teil` und `kabine-ende` an (Untertitel, Pegel). Selbst gesteuert
// werden hier nur Vorhören und Hörprobe. Den Ton selbst spielt Rust
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
  /** Gesprochene Stücke mit Startzeit (s) vom Ton-Server; fehlen sie,
   *  schätzt die Seite die Satzgrenzen aus der Textlänge. */
  stuecke?: { ab_s: number; text: string }[];
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
   *  keine_stimme, kein_kurztext, flug_beendet). */
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
  /** Klang: klar | lautsprecher (Bordlautsprecher, ab Werk) | tuer (dumpf). */
  klang: "klar" | "lautsprecher" | "tuer";
  /** Abgewählte Ansagen (Slot-Namen), z. B. weil GSX oder das Flugzeug sie spielt. */
  aus: string[];
  /** Kabinen-Reaktionen nach der Landung (Applaus, Murmeln, Raunen); ab Werk an.
   *  Lautstärke folgt den Ansagen. */
  reaktionen: boolean;
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

export type Wartet = "andere_ansage" | "pause" | "ruhephase" | "ton" | "sim_angehalten" | "anschnallzeichen";

export interface TeilAnsicht {
  sprache: string;
  text: string | null;
  ton: boolean;
  keine_stimme: boolean;
  /** Text dieser Sprache auch nach der Nachholung ausgeblieben (1f), mit Grund. */
  fehlt?: string | null;
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
  /** Halbzeit-Ansage (2.1.1 Zusatz H), im Reiseflug geschrieben; fehlt bei älterem Stand. */
  halbzeit?: TeilStand;
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
  /** Sprachen ohne Sicherheitsvorlage (Server `entfaellt`), einmal je Flug genannt. */
  sicherheit_ohne_vorlage?: string[];
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

/** Hier abgespielt werden nur noch Vorhören und Hörprobe (Knopf auf der
 *  Seite, das Fenster ist dann vorne). Ansagen des Fluges spielt seit 1b
 *  das Backend selbst und trägt ihr Ende ein — die Oberfläche zeigt sie nur
 *  an, damit eine im Hintergrund schlafende WebView nichts mehr aufhält. */
const schlange: Gespielt[] = [];
let laeuft = false;
/** Steigt bei jedem Stopp: eine laufende Abarbeitung bricht dann ab. */
let durchgang = 0;

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
 *  „Ansagen ausschalten“ — wer das nicht möchte, hört ab dem Klick nichts.
 *  Die Ansage, die das Backend gerade spielt, endet über denselben Stopp. */
export function stoppen(): void {
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
  if (stumm) abbrechen();
  await invoke("kabinen_stumm", { stumm });
}

/** Lautstärke 0 … 1: wirkt sofort, auch mitten in einer Ansage, und gilt
 *  dauerhaft (derselbe Wert wie in den Einstellungen). Beim Ziehen nur
 *  der Wert, beim Loslassen auch die Einstellungsdatei (`speichern`). */
export const lautstaerkeSetzen = (lautstaerke: number, speichern: boolean) =>
  invoke<void>("kabinen_lautstaerke", { lautstaerke, speichern });

/** Eine Ansage einreihen (Ereignis oder Hörprobe). */
export function abspielen(a: Gespielt): void {
  schlange.push(a);
  void abarbeiten();
}

/** Steigt mit jedem angezeigten Teil: beendet den Pegel des vorigen. */
let anzeigeTeil = 0;
/** Die Ansage, die das Backend gerade spielt (nur Anzeige). */
let backendAnsage: Gespielt | null = null;

/** Teil `teil` einer Ansage des Backends beginnt: Untertitel und Pegel aus
 *  der Hüllkurve, mit der Uhr — kein Ton aus dem Fenster. */
function teilAnzeigen(a: Gespielt, teil: number): void {
  const meiner = ++anzeigeTeil;
  const start = performance.now();
  setzen({ ansage: a, teil, teilStart: start, ...(teil === 0 ? { verlauf: [] } : {}) });
  const datei = a.teile[teil]?.datei;
  if (!datei) return;
  void invoke<number[]>("kabinen_ton_huellkurve", { datei })
    .catch(() => [] as number[])
    .then((huelle) => {
      const messen = () => {
        if (meiner !== anzeigeTeil) return;
        const i = Math.floor(((performance.now() - start) / 1000) * HUELLE_JE_S);
        if (i >= huelle.length) {
          setzen({ pegel: 0 });
          return;
        }
        const glatt = zustand.pegel * 0.6 + (huelle[i] ?? 0) * 0.4;
        setzen({ pegel: glatt, verlauf: [...zustand.verlauf, glatt].slice(-VERLAUF_LAENGE) });
        requestAnimationFrame(messen);
      };
      requestAnimationFrame(messen);
    });
}

interface TeilEreignis {
  id: number;
  teil: number;
}
interface EndeEreignis {
  id: number;
  ergebnis: string;
  fehler: string | null;
}

let gestartet = false;
/** Einmal beim Start der App: anzeigen, was das Backend spielt. Nur im
 *  Fenster des Sim-PCs — auf dem Tablet (LAN-Fernbedienung) nichts. */
export function wiedergabeStarten(): void {
  if (gestartet || !isTauri) return;
  gestartet = true;
  Promise.all([
    listen<Gespielt>("kabine-ansage", (e) => {
      backendAnsage = e.payload;
    }),
    listen<TeilEreignis>("kabine-teil", (e) => {
      if (backendAnsage?.id === e.payload.id) teilAnzeigen(backendAnsage, e.payload.teil);
    }),
    listen<EndeEreignis>("kabine-ende", (e) => {
      if (backendAnsage?.id !== e.payload.id) return;
      backendAnsage = null;
      anzeigeTeil += 1;
      setzen({ ansage: null, pegel: 0 });
      if (e.payload.fehler) setzen({ fehler: e.payload.fehler });
      else if (e.payload.ergebnis === "gespielt") setzen({ fehler: null });
    }),
  ]).catch((e) => {
    // Sonst bliebe die Anzeige bis zum Neustart leer: nach kurzer Pause
    // erneut anmelden.
    console.warn("[kabine] Anmelden fehlgeschlagen, neuer Versuch", e);
    gestartet = false;
    setTimeout(wiedergabeStarten, 5000);
  });
  // 1b: Sichtbarkeit ins Flugprotokoll (macOS drosselt verdeckte WebViews).
  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", () => {
      void invoke("kabinen_fenster", { sichtbar: document.visibilityState === "visible" }).catch(() => undefined);
    });
  }
}

export const einstellungenLesen = () => invoke<Einstellungen>("kabinen_einstellungen_lesen");
/** `herkunft` steht beim Ein-/Ausschalten im Flugprotokoll (1d). */
export const einstellungenSetzen = (einstellungen: Einstellungen, herkunft?: "einstellungen" | "seite") =>
  invoke<Einstellungen>("kabinen_einstellungen_setzen", { einstellungen, herkunft: herkunft ?? null });
export const zustandLesen = () => invoke<Zustand>("kabinen_zustand");
export const probe = (rolle: Rolle, sprache: string) => invoke<Gespielt>("kabinen_probe", { rolle, sprache });
export const ueberblick = () => invoke<Ueberblick | null>("kabinen_ueberblick");
export const erneut = () => invoke<void>("kabinen_erneut");
export const speicherLesen = () => invoke<Speicher>("kabinen_speicher");
export const ausgabegeraete = () => invoke<Geraet[]>("kabinen_ausgabegeraete");

// ---------------------------------------------------------------------------
// Boarding-Musik (2.1.1 G, Design 19.10.2026): Regler, Kasten, Dialog.
// Abspielen tut Rust (eigener Pegel, Pause bei jeder Ansage).
// ---------------------------------------------------------------------------

/** Zustand des Kastens „Boarding-Musik“ (Rust `MusikAnsicht`). */
export interface MusikZustand {
  /** Schalter „Boarding-Musik“; aus = keine Musik, kein Lied. */
  an: boolean;
  lautstaerke: number;
  stand: "aus" | "ohne_flug" | "keine" | "wird_erzeugt" | "kein_lied" | "bereit" | "laeuft" | "pausiert" | "beendet" | "probe";
  quelle: "eigene" | "lied" | null;
  titel: string | null;
  strecke: string | null;
  ordner: string | null;
  stuecke: number;
  airline: string | null;
  /** Abschnitt, solange sie läuft: Boarding oder nach der Landung (Zusatz I). */
  abschnitt?: "boarding" | "ankunft" | null;
}

/** Ein Stück der eigenen Ablage. */
export interface MusikStueck {
  airline: string;
  datei: string;
  dauer_s: number | null;
}

export interface MusikAblage {
  ordner: string;
  stuecke: MusikStueck[];
  flug_airline: string | null;
}

export interface AirlineEintrag {
  icao: string;
  name: string;
}

/** Ordnername für „Alle Airlines“ (Rust `musik::ALLE`). */
export const ALLE_AIRLINES = "Alle";
/** Erlaubte Formate (Design §4). */
export const MUSIK_FORMATE = ["mp3", "m4a", "aac", "wav", "flac", "ogg"];

export const musikZustand = () => invoke<MusikZustand>("kabinen_musik_zustand");
export const musikLautstaerkeSetzen = (lautstaerke: number, speichern: boolean) =>
  invoke<void>("kabinen_musik_lautstaerke", { lautstaerke, speichern });
export const boardingMusikSetzen = (an: boolean) => invoke<void>("kabinen_boarding_musik", { an });
export const musikAblage = () => invoke<MusikAblage>("kabinen_musik_ablage");
export const musikPruefen = (quelle: string) => invoke<number | null>("kabinen_musik_pruefen", { quelle });
export const musikHinzufuegen = (quelle: string, airline: string) =>
  invoke<MusikStueck>("kabinen_musik_hinzufuegen", { quelle, airline });
export const musikEntfernen = (airline: string, datei: string) => invoke<void>("kabinen_musik_entfernen", { airline, datei });
export const musikOrdnerOeffnen = () => invoke<void>("kabinen_musik_ordner_oeffnen");
export const musikProbe = (airline: string, datei: string) => invoke<void>("kabinen_musik_probe", { airline, datei });
export const musikProbeFlug = () => invoke<void>("kabinen_musik_probe_flug");
/** „Boarding-Musik laden“: Ist keine da, das Laden neu anstoßen (Thomas 19.10.2026). */
export const musikLaden = () => invoke<void>("kabinen_musik_laden");
export const musikProbeStopp = () => invoke<void>("kabinen_musik_probe_stopp");
export const airlinesLesen = () => invoke<AirlineEintrag[]>("kabinen_airlines");

/** Datei-Fenster mit den erlaubten Formaten; `null` = abgebrochen. */
export async function musikDateiWaehlen(titel: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const wahl = await open({ multiple: false, directory: false, title: titel, filters: [{ name: "Audio", extensions: MUSIK_FORMATE }] });
  return typeof wahl === "string" ? wahl : null;
}

/** m:ss für Längen. */
export function laenge(s: number | null | undefined): string {
  if (s == null || !Number.isFinite(s)) return "–";
  const g = Math.round(s);
  return `${Math.floor(g / 60)}:${String(g % 60).padStart(2, "0")}`;
}
