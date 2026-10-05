// „Flugzeug vermessen" (28.09.2026) — geführte Schaltermessung.
//
// Thomas: „Wichtig ist, dass es gut durch das Programm geführt wird, genau
// erklärt wird, was zu machen ist." Deshalb je Schalter: was, wo, wie er
// heißt, und nach jeder Stellung eine Rückmeldung, ob sich etwas bewegt hat.
// Die Messung selbst läuft im Backend (`vermessung_*`), hier nur Führung.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import type { SimSnapshot, SimStatus } from "../../types";
import { SCHRITTE, schritteFuer, type Schalter, type SchrittDef, type Teil } from "./schritte";
import "./vermessen.css";

/** Ein schon vermessenes Flugzeug (vom Server, ohne Pilotenbezug). */
export interface Vermessen {
  sim: string | null;
  /** „boden" | „luft" — ältere Server/Messungen ohne Angabe = Boden. */
  teil?: string | null;
  /** MSFS: L:-Namen aus den Scans für dieses Flugzeug (null = X-Plane). */
  scan_namen?: number | null;
  /** „geprueft" | „aus_scan" | „in_arbeit" | null. */
  profil?: string | null;
  icao: string | null;
  titel: string | null;
  /** MSFS: SimObject-Ordner der Messung (Server ab 29.09.2026). */
  ordner?: string | null;
  /** Alle gemessenen Titel (Lackierungen) dieses Flugzeugs. */
  titel_liste?: string[];
  /** Geprüfte Teile („boden“, „luft“) des Flugzeugs (Server ab 30.09.2026). */
  profil_teile?: string[];
  zuletzt: number;
  anzahl: number;
}

/** SimObject-Ordner eines aircraft.cfg-Pfads — gleiche Regel wie der Server
 *  (`simObjektOrdner` im Recorder): das Segment nach `SimObjects/<Kategorie>/`,
 *  Slash/Backslash und Groß-/Kleinschreibung egal. Ohne SimObjects-Segment
 *  der Ordner über der aircraft.cfg. Lackierungen zeigen auf den Ordner des
 *  Basisflugzeugs (iFly TUI → `ifly 737-max8-189seats`). */
export function simObjektOrdner(pfad?: string | null): string | null {
  const teile = (pfad ?? "").replace(/\\/g, "/").split("/").map((t) => t.trim().toLowerCase()).filter((t) => t !== "");
  const so = teile.indexOf("simobjects");
  if (so >= 0 && teile.length > so + 3) return teile[so + 2]!;
  const n = teile.length;
  if (n >= 2 && teile[n - 1] === "aircraft.cfg") return teile[n - 2]!;
  return null;
}

/** Ordner des geladenen Flugzeugs (MSFS, aus `AircraftLoaded`); X-Plane und
 *  MSFS kurz nach dem Start: null → Abgleich über den Titel. */
export function ordnerDesGeladenen(snap?: Pick<SimSnapshot, "cockpit_rohwerte"> | null): string | null {
  return simObjektOrdner(snap?.cockpit_rohwerte?.cfg_pfad);
}

/** Titel, unter denen das geladene Flugzeug gemessen sein kann: X-Plane-
 *  Messungen tragen seit 29.09.2026 den UI-Namen (`acf_ui_name`, wie der
 *  Scan), ältere die Beschreibung (`acf_descrip` = Schnappschuss-Titel).
 *  Beide zählen. Der erste ist der Titel einer neuen Messung. */
export function titelDesGeladenen(snap?: Pick<SimSnapshot, "aircraft_title" | "aircraft_ui_name"> | null): string[] {
  const t: string[] = [];
  for (const x of [snap?.aircraft_ui_name, snap?.aircraft_title]) {
    const s = (x ?? "").trim();
    if (s && !t.some((y) => y.toLowerCase() === s.toLowerCase())) t.push(s);
  }
  return t;
}

/** Ist das geladene Flugzeug schon vermessen?
 *  1. MSFS mit Ordner (29.09.2026): gleicher SimObject-Ordner — egal welche
 *     Lackierung (iFly TUI gemessen, iFly RYR geladen = schon vermessen).
 *  2. Sonst Muster UND Titel — die ICAO allein trennt z. B. FlyByWire- und
 *     iniBuilds-A380 nicht. Eine Messung mit BEKANNTEM anderem Ordner zählt
 *     dabei nie, auch bei gleichem Titel (verschiedene Add-ons). `titel` darf
 *     mehrere Titel nennen (X-Plane: UI-Name und Beschreibung); verglichen
 *     wird mit allen gemessenen Titeln der Zeile. */
export function schonVermessen(
  liste: Vermessen[],
  titel?: string | readonly string[] | null,
  icao?: string | null,
  teil: Teil = "boden",
  ordner?: string | null,
): Vermessen | null {
  const imTeil = liste.filter((v) => (v.teil ?? "boden") === teil);
  const o = (ordner ?? "").trim().toLowerCase();
  if (o) {
    const treffer = imTeil.find((v) => klein(v.ordner) === o);
    if (treffer) return treffer;
  }
  const titelListe = typeof titel === "string" ? [titel] : (titel ?? []);
  const t = new Set(titelListe.map(klein).filter((x) => x !== ""));
  const i = gross(icao);
  if (t.size === 0) return null;
  return (
    imTeil.find(
      (v) =>
        !(o && v.ordner && klein(v.ordner) !== o) &&
        [v.titel, ...(v.titel_liste ?? [])].some((x) => t.has(klein(x))) &&
        (!i || !v.icao || gross(v.icao) === i),
    ) ?? null
  );
}

/** Ein Aircraft-Scan der VA (vom Server). */
export interface ScanEintrag {
  sim: string | null;
  icao: string | null;
  paket: string | null;
  titel_liste: string[];
  scan_namen: number | null;
  /** „geprueft" (per Messung bestätigt) | „aus_scan" (eingebaut, nur aus
   *  dem Scan abgeleitet) | „in_arbeit" | null — wie der Admin ihn setzt. */
  profil: string | null;
  zuletzt: number;
  /** „aao-profil“ / „hersteller-doku“ = hinterlegte Namensquelle, kein Scan. */
  quelle?: string | null;
  /** MSFS: SimObject-Ordner aus der Dateiliste des Scans. */
  ordner?: string[];
}

const PROFIL_RANG: Record<string, number> = { geprueft: 3, aus_scan: 2, in_arbeit: 1 };
/** Der höhere von zwei Profil-Ständen. */
const hoeher = (a: string | null | undefined, b: string | null | undefined) =>
  ((a && PROFIL_RANG[a]) ?? 0) >= ((b && PROFIL_RANG[b]) ?? 0) ? (a ?? null) : (b ?? null);

/** Eine Zeile der Übersichtstabelle. */
export type Zeile = {
  titel: string | null;
  icao: string | null;
  sim: string | null;
  /** Alle Titel, unter denen der Simulator dieses Flugzeug meldet
   *  (Lackierungen) — für den Abgleich mit dem geladenen Flugzeug. */
  titel_liste: string[];
  /** MSFS: SimObject-Ordner dieses Flugzeugs (aus Messungen und Scans) —
   *  der eindeutige Abgleich; leer = unbekannt, dann zählt der Titel. */
  ordner: string[];
  /** MSFS: L:-Namen aus den Scans (null = X-Plane/unbekannt). */
  scan_namen: number | null;
  profil: string | null;
  /** Geprüfte Teile — nur einer = Zwischenstufe „Boden/Luft geprüft“. */
  profil_teile: string[];
  boden: Vermessen | null;
  luft: Vermessen | null;
};

const klein = (x: string | null | undefined) => (x ?? "").trim().toLowerCase();
const gross = (x: string | null | undefined) => (x ?? "").trim().toUpperCase();

/** Geprüfte Teile vereinen (Reihenfolge boden, luft). */
const teileDazu = (ziel: string[], neu: readonly string[] | null | undefined) => {
  for (const t of neu ?? []) if ((t === "boden" || t === "luft") && !ziel.includes(t)) ziel.push(t);
  ziel.sort((x, y) => (x === "boden" ? -1 : 1) - (y === "boden" ? -1 : 1));
};

/** Titel ohne Dubletten (Groß/Klein egal) anhängen. */
const titelDazu = (ziel: string[], neu: Array<string | null | undefined>) => {
  for (const t of neu) if (t && t.trim() && !ziel.some((x) => klein(x) === klein(t))) ziel.push(t);
};

/** Bestand je Flugzeug: Boden und Luft nebeneinander. MSFS-Messungen mit
 *  Ordner gehören über den Ordner zusammen, alle anderen über Titel+ICAO. */
export function jeFlugzeug(liste: Vermessen[]): Zeile[] {
  const aus = new Map<string, Zeile>();
  for (const v of liste) {
    const k = v.ordner ? `${v.sim ?? ""}|o:${klein(v.ordner)}` : `${v.sim ?? ""}|${gross(v.icao)}|${klein(v.titel)}`;
    const e =
      aus.get(k) ??
      { titel: v.titel, icao: v.icao, sim: v.sim, titel_liste: [], ordner: v.ordner ? [klein(v.ordner)] : [], scan_namen: null, profil: null, profil_teile: [], boden: null, luft: null };
    titelDazu(e.titel_liste, [v.titel, ...(v.titel_liste ?? [])]);
    if (typeof v.scan_namen === "number") e.scan_namen = v.scan_namen;
    e.profil = hoeher(e.profil, v.profil);
    teileDazu(e.profil_teile, v.profil_teile);
    if ((v.teil ?? "boden") === "luft") e.luft = v;
    else e.boden = v;
    aus.set(k, e);
  }
  return [...aus.values()];
}

/** Übersicht: Messungen und Aircraft-Scans in einer Tabelle. Ein Scan
 *  gehört zu einer Messung, wenn Simulator passt und
 *  - beide Ordner kennen und einer übereinstimmt (MSFS, eindeutig), oder
 *  - (Ordner fehlt auf einer Seite) das Muster passt und der gemessene
 *    Titel unter den Titeln des Scans ist.
 *  Kennen beide Ordner und keiner stimmt überein, sind es verschiedene
 *  Flugzeuge, auch bei gleichem Titel. Sonst eigene Zeile (gescannt, aber
 *  noch nicht vermessen). */
export function uebersicht(liste: Vermessen[], scans: ScanEintrag[]): Zeile[] {
  const zeilen = jeFlugzeug(liste);
  for (const sc of scans) {
    const titel = sc.titel_liste.map(klein);
    const scOrdner = (sc.ordner ?? []).map(klein);
    const gleicherSim = (z: Zeile) => (z.sim ?? "msfs") === (sc.sim ?? "msfs");
    const treffer =
      zeilen.find((z) => gleicherSim(z) && z.ordner.some((o) => scOrdner.includes(o))) ??
      zeilen.find(
        (z) =>
          gleicherSim(z) &&
          !(z.ordner.length > 0 && scOrdner.length > 0) &&
          (!gross(z.icao) || !gross(sc.icao) || gross(z.icao) === gross(sc.icao)) &&
          z.titel_liste.some((t) => titel.includes(klein(t))),
      );
    if (treffer) {
      if (treffer.scan_namen === null && sc.scan_namen !== null) treffer.scan_namen = sc.scan_namen;
      treffer.profil = hoeher(treffer.profil, sc.profil);
      titelDazu(treffer.titel_liste, sc.titel_liste);
      for (const o of scOrdner) if (!treffer.ordner.includes(o)) treffer.ordner.push(o);
    } else {
      // Eine hinterlegte Namensquelle trägt einen Beschreibungstext als
      // Paketnamen („… – L:-Namen aus HubHop“) — die Zeile heißt dann wie
      // das Flugzeug.
      const nurNamen = sc.quelle === "aao-profil" || sc.quelle === "hersteller-doku";
      zeilen.push({
        titel: (nurNamen ? sc.titel_liste[0] : sc.paket) ?? sc.titel_liste[0] ?? sc.paket ?? null,
        icao: sc.icao,
        sim: sc.sim,
        titel_liste: [...sc.titel_liste],
        ordner: [...scOrdner],
        scan_namen: sc.scan_namen,
        profil: sc.profil,
        profil_teile: [],
        boden: null,
        luft: null,
      });
    }
  }
  return familienZusammenfassen(zeilen);
}

/** Muster, die AeroACARS mit EINEM Profil liest, weil Cockpit und Variablen
 *  gleich sind — in der Übersicht eine Zeile statt einer je Paket
 *  (28.09.2026: Fenix A320 und das Zusatzpaket A319/A321 standen doppelt). */
// Ohne Muster (ICAO): die Typen stehen im Namen, und ein geladener A321
// muss die Zeile über den Titel treffen, nicht über „A320“.
const FAMILIEN: Array<{ name: string; gehoert: (titel: string) => boolean }> = [
  { name: "Fenix A319 / A320 / A321", gehoert: (x) => /^fenixa3(19|20|21)\b/i.test(x.trim()) },
];

const juenger = (a: Vermessen | null, b: Vermessen | null): Vermessen | null =>
  !a ? b : !b ? a : { ...(a.zuletzt >= b.zuletzt ? a : b), anzahl: a.anzahl + b.anzahl };

function familienZusammenfassen(zeilen: Zeile[]): Zeile[] {
  const aus: Zeile[] = [];
  const familie = new Map<string, Zeile>();
  for (const z of zeilen) {
    const f = FAMILIEN.find((f) => (z.sim ?? "msfs") === "msfs" && z.titel_liste.some(f.gehoert));
    if (!f) {
      aus.push(z);
      continue;
    }
    const e = familie.get(f.name);
    if (!e) {
      const neu: Zeile = { ...z, titel: f.name, icao: null, sim: "msfs", titel_liste: [...z.titel_liste], ordner: [...z.ordner], profil_teile: [...z.profil_teile] };
      familie.set(f.name, neu);
      aus.push(neu);
      continue;
    }
    e.boden = juenger(e.boden, z.boden);
    e.luft = juenger(e.luft, z.luft);
    if (z.scan_namen !== null && (e.scan_namen ?? -1) < z.scan_namen) e.scan_namen = z.scan_namen;
    e.profil = hoeher(e.profil, z.profil);
    teileDazu(e.profil_teile, z.profil_teile);
    titelDazu(e.titel_liste, z.titel_liste);
    for (const o of z.ordner) if (!e.ordner.includes(o)) e.ordner.push(o);
  }
  return aus;
}

interface StartAntwort {
  sim: "msfs" | "xplane";
  flugzeug: { titel?: string | null; icao?: string | null; autor?: string | null };
  anzahl_werte: number;
  l_namen: number;
  /** Sitzungsnummer — geht bei jedem Befehl mit. */
  sitzung: number;
}
interface StellungAntwort {
  mitgegangen: number;
  erste: boolean;
}
interface SchrittAntwort {
  kandidaten: number;
  // `texte`: Text-Datarefs (X-Plane) — `werte` sind dann nur Kennzahlen.
  beispiele: Array<{ variable: string; werte: Array<number | null>; texte?: Array<string | null> }>;
}

type Ergebnis = { uebersprungen: boolean; kandidaten: number };

type Phase =
  | { art: "start" }
  | { art: "verbinden" }
  | { art: "ruhe"; laeuft: boolean; rauschen: number | null }
  | {
      art: "schritt";
      nr: number;
      begonnen: boolean;
      stellung: number;
      rueckmeldungen: Array<{ stellung: string; antwort: StellungAntwort | null }>;
      misst: boolean;
      abschluss: SchrittAntwort | null;
    }
  | { art: "fertig"; sendet: boolean }
  | { art: "gesendet"; id: string };

function fehlerText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

export function FlugzeugVermessen() {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>({ art: "start" });
  const [start, setStart] = useState<StartAntwort | null>(null);
  const [ergebnisse, setErgebnisse] = useState<Record<string, Ergebnis>>({});
  const [fehler, setFehler] = useState<string | null>(null);
  const [sim, setSim] = useState<SimStatus | null>(null);
  const [vermessen, setVermessen] = useState<Vermessen[]>([]);
  const [scans, setScans] = useState<ScanEintrag[]>([]);
  // Welche Schalter gemessen werden (Startseite, voreingestellt alle) und
  // der beim Start festgehaltene Plan — z. B. nur die Autobrake nachmessen.
  const [auswahl, setAuswahl] = useState<Set<Schalter>>(() => new Set(SCHRITTE.map((x) => x.schalter)));
  const [plan, setPlan] = useState<SchrittDef[]>(SCHRITTE);
  const [teil, setTeil] = useState<Teil>("boden");

  // „Schon vermessen" — bei jedem Zurück auf die Startseite neu holen.
  useEffect(() => {
    if (phase.art !== "start") return;
    let lebt = true;
    invoke<{ flugzeuge?: Vermessen[]; scans?: ScanEintrag[] } | Vermessen[]>("vermessung_liste")
      .then((l) => {
        if (!lebt) return;
        // Ältere Form: nur die Liste der Messungen.
        if (Array.isArray(l)) {
          setVermessen(l);
          setScans([]);
        } else {
          setVermessen(Array.isArray(l?.flugzeuge) ? l.flugzeuge : []);
          setScans(Array.isArray(l?.scans) ? l.scans : []);
        }
      })
      .catch(() => undefined);
    return () => {
      lebt = false;
    };
  }, [phase.art]);
  // Laufnummer: Abbrechen/Verlassen zählt hoch, eine danach noch
  // eintreffende Antwort ändert die Ansicht nicht mehr.
  const lauf = useRef(0);
  // Ein Befehl läuft — Doppelklicks nicht zweimal senden.
  const belegt = useRef(false);
  // Sitzung im Backend (ab erfolgreichem Start).
  const sitzung = useRef<number | null>(null);
  // Kennung des eigenen Starts — damit trifft ein Abbruch mitten im
  // Verbinden nur diesen Start, nie einen gleichzeitig vom iPad gestarteten.
  const startNr = useRef<number | null>(null);
  const beendenArgs = () => ({
    ...(startNr.current === null ? {} : { start: startNr.current }),
    ...(sitzung.current === null ? {} : { sitzung: sitzung.current }),
  });

  // Simulatorstatus für den Startbildschirm.
  useEffect(() => {
    if (phase.art !== "start") return;
    let lebt = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const frage = async () => {
      try {
        const s = await invoke<SimStatus>("sim_status");
        if (lebt) setSim(s);
      } catch {
        if (lebt) setSim(null);
      }
      if (lebt) timer = setTimeout(() => void frage(), 2000);
    };
    void frage();
    return () => {
      lebt = false;
      if (timer) clearTimeout(timer);
    };
  }, [phase.art]);

  // Verlässt der Pilot die Seite, Messung sauber beenden — auch mitten im
  // Verbinden (der Start bricht dann im Backend selbst ab).
  useEffect(
    () => () => {
      lauf.current++;
      // Nie gestartet: nichts zu beenden.
      if (startNr.current !== null) void invoke("vermessung_beenden", beendenArgs()).catch(() => undefined);
    },
    [],
  );

  const beenden = useCallback(async () => {
    lauf.current++;
    belegt.current = false;
    const args = beendenArgs();
    sitzung.current = null;
    startNr.current = null;
    if (args.start !== undefined || args.sitzung !== undefined) {
      await invoke("vermessung_beenden", args).catch(() => undefined);
    }
  }, []);

  /** Einen Befehl ausführen; `null`, wenn inzwischen abgebrochen wurde. */
  const ausfuehren = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T | null> => {
    const n = lauf.current;
    const mitNr = sitzung.current === null ? args : { ...args, sitzung: sitzung.current };
    const a = await invoke<T>(cmd, mitNr);
    return lauf.current === n ? a : null;
  };
  const fehlerWennAktuell = (n: number, e: unknown) => {
    if (lauf.current === n) setFehler(fehlerText(e));
  };

  const abbrechen = async () => {
    if (!window.confirm(t("vermessen.abbrechen_frage"))) return;
    await beenden();
    setErgebnisse({});
    setFehler(null);
    setPhase({ art: "start" });
  };

  const starten = async () => {
    setFehler(null);
    setPhase({ art: "verbinden" });
    const n = lauf.current;
    // Boden oder Luft entscheidet der Simulator: am Boden die Schalter, in der
    // Luft der Autopilot (rastet am Boden nicht ein).
    const neuerTeil: Teil = sim?.snapshot?.on_ground === false ? "luft" : "boden";
    setTeil(neuerTeil);
    setPlan(schritteFuer(neuerTeil).filter((x) => auswahl.has(x.schalter)));
    startNr.current = Math.floor(Math.random() * 2 ** 50);
    sitzung.current = null;
    try {
      const a = await ausfuehren<StartAntwort>("vermessung_starten", { start: startNr.current, teil: neuerTeil });
      if (!a) return;
      sitzung.current = a.sitzung;
      setStart(a);
      setErgebnisse({});
      setPhase({ art: "ruhe", laeuft: false, rauschen: null });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "start" });
    }
  };

  const ruhe = async () => {
    setPhase({ art: "ruhe", laeuft: true, rauschen: null });
    const n = lauf.current;
    try {
      const r = await ausfuehren<{ rauschen: number }>("vermessung_ruhe");
      if (!r) return;
      setPhase({ art: "ruhe", laeuft: false, rauschen: r.rauschen });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "ruhe", laeuft: false, rauschen: null });
    }
  };

  const schrittBeginnen = (nr: number) =>
    setPhase({ art: "schritt", nr, begonnen: false, stellung: 0, rueckmeldungen: [], misst: false, abschluss: null });

  const abschliessen = async (def: SchrittDef, uebersprungen: boolean, rueck: Array<{ stellung: string; antwort: StellungAntwort | null }>, nr: number) => {
    const n = lauf.current;
    try {
      const a = await ausfuehren<SchrittAntwort>("vermessung_schritt_abschliessen", {
        schalter: def.schalter,
        uebersprungen,
      });
      if (!a) return;
      setErgebnisse((e) => ({ ...e, [def.schalter]: { uebersprungen, kandidaten: a.kandidaten } }));
      if (uebersprungen) {
        weiterNach(nr);
      } else {
        setPhase({ art: "schritt", nr, begonnen: true, stellung: def.stellungen.length, rueckmeldungen: rueck, misst: false, abschluss: a });
      }
    } catch (e) {
      fehlerWennAktuell(n, e);
    }
  };

  const weiterNach = (nr: number) => {
    if (nr + 1 < plan.length) schrittBeginnen(nr + 1);
    else setPhase({ art: "fertig", sendet: false });
  };

  const stellungMessen = async (vorhanden: boolean) => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      await stellungMessenInnen(vorhanden);
    } finally {
      belegt.current = false;
    }
  };

  const stellungMessenInnen = async (vorhanden: boolean) => {
    if (phase.art !== "schritt") return;
    const n = lauf.current;
    const def = plan[phase.nr]!;
    const name = def.stellungen[phase.stellung]!;
    let rueck = phase.rueckmeldungen;
    if (vorhanden) {
      setPhase({ ...phase, misst: true });
      try {
        const a = await ausfuehren<StellungAntwort>("vermessung_stellung", { stellung: t(`vermessen.stellung.${name}`) });
        if (!a) return;
        rueck = [...rueck, { stellung: name, antwort: a }];
      } catch (e) {
        if (lauf.current !== n) return;
        setFehler(fehlerText(e));
        setPhase({ ...phase, misst: false });
        return;
      }
    } else {
      rueck = [...rueck, { stellung: name, antwort: null }];
    }
    const naechste = phase.stellung + 1;
    if (naechste >= def.stellungen.length) {
      setPhase({ ...phase, misst: false, rueckmeldungen: rueck, stellung: naechste });
      await abschliessen(def, false, rueck, phase.nr);
    } else {
      setPhase({ ...phase, misst: false, rueckmeldungen: rueck, stellung: naechste });
    }
  };

  const ueberspringen = async () => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      await abschliessen(plan[phase.nr]!, true, [], phase.nr);
    } finally {
      belegt.current = false;
    }
  };

  const letzteStellung = async () => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      const def = plan[phase.nr]!;
      await abschliessen(def, false, phase.rueckmeldungen, phase.nr);
    } finally {
      belegt.current = false;
    }
  };

  const schrittNeu = async () => {
    if (phase.art !== "schritt") return;
    await invoke("vermessung_schritt_neu", sitzung.current === null ? undefined : { sitzung: sitzung.current }).catch(
      () => undefined,
    );
    setPhase({ ...phase, stellung: 0, rueckmeldungen: [], abschluss: null, begonnen: true });
  };

  const senden = async () => {
    setFehler(null);
    setPhase({ art: "fertig", sendet: true });
    const n = lauf.current;
    try {
      const a = await ausfuehren<{ id: string }>("vermessung_senden");
      if (!a) return;
      await beenden();
      setPhase({ art: "gesendet", id: a.id });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "fertig", sendet: false });
    }
  };

  const flugzeugName = start ? [start.flugzeug.titel, start.flugzeug.icao].filter(Boolean).join(" · ") : "";

  return (
    <div className="vm">
      {fehler && (
        <div className="vm-fehler" role="alert">
          {t("vermessen.fehler", { fehler })}
        </div>
      )}

      {phase.art === "start" && <StartSeite
          sim={sim}
          vermessen={vermessen}
          scans={scans}
          auswahl={auswahl}
          onAuswahl={setAuswahl}
          onStart={() => void starten()}
        />}

      {phase.art === "verbinden" && (
        <section className="vm-karte vm-mitte">
          <div className="vm-drehen" aria-hidden />
          <h3>{t("vermessen.verbinden_titel")}</h3>
          <p className="vm-dim">{t("vermessen.verbinden_text")}</p>
        </section>
      )}

      {phase.art === "ruhe" && start && (
        <section className="vm-karte">
          <Kopf flugzeug={flugzeugName} text={t("vermessen.verbunden", { anzahl: start.anzahl_werte })} />
          {start.sim === "msfs" && (
            <p className={start.l_namen > 0 ? "vm-hinweis" : "vm-hinweis vm-hinweis--warn"}>
              {start.l_namen > 0 ? t("vermessen.mit_scan", { anzahl: start.l_namen }) : t("vermessen.ohne_scan")}
            </p>
          )}
          <h3>{t("vermessen.ruhe_titel")}</h3>
          <p>{teil === "luft" ? t("vermessen.ruhe_text_luft") : t("vermessen.ruhe_text")}</p>
          {phase.laeuft && <Fortschritt sekunden={8} text={t("vermessen.ruhe_laeuft")} />}
          {phase.rauschen !== null && <p className="vm-ok">✓ {t("vermessen.ruhe_fertig", { anzahl: phase.rauschen })}</p>}
          <div className="vm-knoepfe">
            {phase.rauschen === null ? (
              <button type="button" className="button button--primary" disabled={phase.laeuft} onClick={() => void ruhe()}>
                {t("vermessen.ruhe_start")}
              </button>
            ) : (
              <button type="button" className="button button--primary" onClick={() => schrittBeginnen(0)}>
                {t("vermessen.weiter")}
              </button>
            )}
            <button type="button" className="button vm-leise" onClick={() => void abbrechen()}>
              {t("vermessen.abbrechen")}
            </button>
          </div>
        </section>
      )}

      {phase.art === "schritt" && (
        <SchrittKarte
          plan={plan}
          phase={phase}
          ergebnisse={ergebnisse}
          onBeginnen={() => setPhase({ ...phase, begonnen: true })}
          onUeberspringen={() => void ueberspringen()}
          onErledigt={() => void stellungMessen(true)}
          onGibtEsNicht={() => void stellungMessen(false)}
          onLetzte={() => void letzteStellung()}
          onNeu={() => void schrittNeu()}
          onWeiter={() => weiterNach(phase.nr)}
          onAbbrechen={() => void abbrechen()}
        />
      )}

      {phase.art === "fertig" && (
        <section className="vm-karte">
          <Kopf flugzeug={flugzeugName} />
          <h3>{t("vermessen.fertig_titel")}</h3>
          <p>{t("vermessen.fertig_text")}</p>
          <ul className="vm-uebersicht">
            {plan.map((s) => {
              const e = ergebnisse[s.schalter];
              return (
                <li key={s.schalter}>
                  <span>{t(`vermessen.schritt.${s.schalter}.titel`)}</span>
                  <span className={!e || e.uebersprungen ? "vm-dim" : e.kandidaten > 0 ? "vm-ok" : "vm-warn"}>
                    {!e || e.uebersprungen
                      ? t("vermessen.uebersprungen")
                      : e.kandidaten > 0
                        ? t("vermessen.werte", { anzahl: e.kandidaten })
                        : t("vermessen.keine_werte")}
                  </span>
                </li>
              );
            })}
          </ul>
          <div className="vm-knoepfe">
            <button type="button" className="button button--primary" disabled={phase.sendet} onClick={() => void senden()}>
              {phase.sendet ? t("vermessen.sendet") : t("vermessen.senden")}
            </button>
            <button type="button" className="button vm-leise" disabled={phase.sendet} onClick={() => void abbrechen()}>
              {t("vermessen.abbrechen")}
            </button>
          </div>
        </section>
      )}

      {phase.art === "gesendet" && (
        <section className="vm-karte vm-mitte">
          <div className="vm-haken" aria-hidden>
            ✓
          </div>
          <h3>{t("vermessen.gesendet_titel")}</h3>
          <p>{t("vermessen.gesendet_text")}</p>
          <button type="button" className="button" onClick={() => setPhase({ art: "start" })}>
            {t("vermessen.neue_messung")}
          </button>
        </section>
      )}
    </div>
  );
}

function StartSeite({
  sim,
  vermessen,
  scans,
  auswahl,
  onAuswahl,
  onStart,
}: {
  sim: SimStatus | null;
  vermessen: Vermessen[];
  scans: ScanEintrag[];
  auswahl: Set<Schalter>;
  onAuswahl: (a: Set<Schalter>) => void;
  onStart: () => void;
}) {
  const { t, i18n } = useTranslation();
  const snap = sim?.snapshot ?? null;
  const verbunden = !!snap;
  // In der Luft gibt es den Autopilot-Teil, am Boden die Schalter.
  const teil: Teil = snap?.on_ground === false ? "luft" : "boden";
  const luft = teil === "luft";
  // X-Plane: UI-Name zuerst (so heißt die Messung), dann die Beschreibung.
  const titelGeladen = titelDesGeladenen(snap);
  const messTitel = titelGeladen[0] ?? null;
  const flugzeug = [messTitel, snap?.aircraft_icao].filter(Boolean).join(" · ");
  const datum = (ms: number) => new Date(ms).toLocaleDateString(i18n.language || "de");
  const ordnerGeladen = ordnerDesGeladenen(snap);
  const schon = verbunden ? schonVermessen(vermessen, titelGeladen, snap?.aircraft_icao, teil, ordnerGeladen) : null;
  const scanNamen = useScanNamen(sim);
  const moeglich = schritteFuer(teil);
  const gewaehlt = moeglich.filter((x) => auswahl.has(x.schalter)).length;
  const bestand = uebersicht(vermessen, scans);
  const eigeneProfile = useEigeneProfile(bestand);
  // Tabelle: das geladene Flugzeug immer oben — auch, wenn es noch gar nicht
  // vermessen ist (dann „fehlt“ in beiden Spalten).
  const titelJetzt = new Set(titelGeladen.map(klein));
  const icaoJetzt = (snap?.aircraft_icao ?? "").trim().toUpperCase();
  // Zuerst über den Ordner (MSFS, jede Lackierung); eine Zeile mit anderem
  // bekannten Ordner ist ein anderes Flugzeug, auch bei gleichem Titel.
  const istGeladen = (v: Zeile) =>
    ordnerGeladen && v.ordner.length > 0
      ? v.ordner.includes(ordnerGeladen)
      : titelJetzt.size > 0 &&
    v.titel_liste.some((x) => titelJetzt.has(klein(x))) &&
    (!icaoJetzt || !v.icao || gross(v.icao) === icaoJetzt);
  const geladenImBestand = bestand.filter(istGeladen);
  const zeilen = [
    ...(verbunden && titelJetzt.size > 0
      ? geladenImBestand.length > 0
        ? geladenImBestand.map((v) => ({ ...v, geladen: true }))
        : [
            {
              titel: messTitel,
              icao: snap?.aircraft_icao ?? null,
              sim: sim?.kind?.startsWith("msfs") ? "msfs" : sim?.kind?.startsWith("xplane") ? "xplane" : null,
              titel_liste: titelGeladen,
              ordner: ordnerGeladen ? [ordnerGeladen] : [],
              scan_namen: null,
              profil: null,
              profil_teile: [],
              boden: null,
              luft: null,
              geladen: true,
            },
          ]
      : []),
    ...bestand.filter((v) => !(verbunden && istGeladen(v))).map((v) => ({ ...v, geladen: false })),
  ];
  const vorher = luft
    ? ["luft_vorher_1", "luft_vorher_2", "luft_vorher_3", "luft_vorher_4"]
    : ["vorher_sim", "vorher_boden", "vorher_strom", "vorher_pause"];
  return (
    <section className="vm-karte">
      <h3 className="vm-titel">{luft ? t("vermessen.luft_titel") : t("vermessen.titel")}</h3>
      <p>{luft ? t("vermessen.luft_einleitung") : t("vermessen.einleitung")}</p>
      <p className="vm-dim">{luft ? t("vermessen.luft_dauer") : t("vermessen.dauer")}</p>
      <div className="vm-liste">
        <div className="vm-liste-titel">{t("vermessen.vorher_titel")}</div>
        <ol>
          {vorher.map((k) => (
            <li key={k}>{t(`vermessen.${k}`)}</li>
          ))}
        </ol>
      </div>
      <div className={`vm-status ${verbunden ? "vm-status--ok" : "vm-status--warn"}`}>
        {!verbunden
          ? t("vermessen.sim_fehlt")
          : t(luft ? "vermessen.sim_ok_luft" : "vermessen.sim_ok", { flugzeug: flugzeug || "—" })}
      </div>
      <div className="vm-tabelle-rahmen">
        <div className="vm-liste-titel">{t("vermessen.tabelle_titel")}</div>
        <p className="vm-klein vm-dim">{t("vermessen.liste_hinweis")}</p>
        <table className="vm-tabelle">
          <thead>
            <tr>
              <th>{t("vermessen.spalte_flugzeug")}</th>
              <th>{t("vermessen.stand_boden")}</th>
              <th>{t("vermessen.stand_luft")}</th>
              <th>{t("vermessen.spalte_scan")}</th>
              <th>{t("vermessen.spalte_profil")}</th>
            </tr>
          </thead>
          <tbody>
            {zeilen.map((v, n) => (
              <tr key={`${v.sim}|${v.icao}|${v.titel}|${n}`} className={v.geladen ? "vm-zeile--geladen" : undefined}>
                <td>
                  <div className="vm-bestand-name">
                    {v.titel || "—"}
                    {v.geladen && <span className="vm-geladen">{t("vermessen.geladen")}</span>}
                  </div>
                  <div className="vm-dim vm-klein">
                    {[v.icao, v.sim === "xplane" ? "X-Plane" : v.sim === "msfs" ? "MSFS" : null].filter(Boolean).join(" · ")}
                  </div>
                </td>
                {(["boden", "luft"] as const).map((tl) => {
                  const e = v[tl];
                  return (
                    <td key={tl}>
                      {e ? (
                        <span className="vm-zelle vm-zelle--ok">
                          ✓ {datum(e.zuletzt)}
                          {e.anzahl > 1 ? ` (${e.anzahl}×)` : ""}
                        </span>
                      ) : (
                        <span className="vm-zelle vm-zelle--fehlt">{t("vermessen.fehlt")}</span>
                      )}
                    </td>
                  );
                })}
                <td>
                  <ScanZelle
                    sim={v.sim}
                    // Beim geladenen Flugzeug die frische Abfrage, sonst die Serverzahl.
                    anzahl={v.geladen && scanNamen !== null ? scanNamen : v.scan_namen}
                  />
                </td>
                <td>
                  {v.profil === "geprueft" ? (
                    <span className="vm-zelle vm-zelle--ok" title={t("vermessen.profil_geprueft_hilfe")}>
                      ✓ {t("vermessen.profil_geprueft")}
                    </span>
                  ) : v.profil_teile.length === 1 ? (
                    // Zwischenstufe (30.09.2026): ein Teil geprüft, der andere
                    // fehlt noch — „geprüft“ gilt erst mit Boden UND Luft.
                    <span
                      className="vm-zelle vm-zelle--teil"
                      title={t("vermessen.profil_teil_hilfe", {
                        fehlt: t(v.profil_teile[0] === "boden" ? "vermessen.teil_luft" : "vermessen.teil_boden"),
                      })}
                    >
                      ✓ {t(v.profil_teile[0] === "boden" ? "vermessen.profil_boden_geprueft" : "vermessen.profil_luft_geprueft")}
                    </span>
                  ) : eigeneProfile.get(zeilenSchluessel(v)) || v.profil === "aus_scan" ? (
                    <span
                      className="vm-zelle vm-zelle--fehlt"
                      title={t("vermessen.profil_eigen_hilfe", { name: eigeneProfile.get(zeilenSchluessel(v)) ?? "" })}
                    >
                      {t("vermessen.profil_eigen")}
                    </span>
                  ) : v.profil === "in_arbeit" ? (
                    <span className="vm-zelle vm-zelle--arbeit">{t("vermessen.profil_in_arbeit")}</span>
                  ) : v.sim === "msfs" ? (
                    <span className="vm-zelle vm-zelle--leise" title={t("vermessen.profil_standard_hilfe")}>
                      {t("vermessen.profil_standard")}
                    </span>
                  ) : (
                    <span className="vm-dim">–</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {schon && <p className="vm-hinweis">{t(luft ? "vermessen.stand_nochmal_luft" : "vermessen.stand_nochmal_boden")}</p>}
      {!schon && scanNamen === 0 && <p className="vm-hinweis vm-hinweis--warn">{t("vermessen.erst_scan")}</p>}
      <p className="vm-klein vm-dim">{t("vermessen.was_gesendet")}</p>
      <div className="vm-knoepfe">
        <button
          type="button"
          className={schon ? "button" : "button button--primary"}
          disabled={!verbunden || gewaehlt === 0}
          onClick={onStart}
        >
          {schon ? t("vermessen.trotzdem") : t(luft ? "vermessen.luft_starten" : "vermessen.starten")}
        </button>
      </div>
      <details className="vm-liste vm-auswahl">
        <summary className="vm-liste-titel">
          {t("vermessen.auswahl_titel", { anzahl: gewaehlt, von: moeglich.length })}
        </summary>
        <p className="vm-klein vm-dim">{t(luft ? "vermessen.auswahl_hinweis_luft" : "vermessen.auswahl_hinweis")}</p>
        <div className="vm-auswahl-gitter">
          {moeglich.map((x) => (
            <label key={x.schalter} className="vm-auswahl-punkt">
              <input
                type="checkbox"
                checked={auswahl.has(x.schalter)}
                onChange={(e) => {
                  const neu = new Set(auswahl);
                  if (e.target.checked) neu.add(x.schalter);
                  else neu.delete(x.schalter);
                  onAuswahl(neu);
                }}
              />
              {t(`vermessen.schritt.${x.schalter}.titel`)}
            </label>
          ))}
        </div>
        <div className="vm-knoepfe">
          <button
            type="button"
            className="vm-link"
            onClick={() => onAuswahl(new Set([...auswahl, ...moeglich.map((x) => x.schalter)]))}
          >
            {t("vermessen.auswahl_alle")}
          </button>
          <button
            type="button"
            className="vm-link"
            onClick={() => onAuswahl(new Set([...auswahl].filter((x) => !moeglich.some((m) => m.schalter === x))))}
          >
            {t("vermessen.auswahl_keine")}
          </button>
        </div>
      </details>
    </section>
  );
}

const zeilenSchluessel = (v: { sim: string | null; icao: string | null; titel: string | null }) =>
  `${v.sim ?? ""}|${gross(v.icao)}|${klein(v.titel)}`;

/** MSFS: hat AeroACARS für diese Zeilen ein eigenes Profil? (gleiche
 *  Erkennung wie im Flug, im Client) — Schlüssel → Profilname. */
function useEigeneProfile(zeilen: Zeile[]): Map<string, string> {
  const msfs = zeilen.filter((z) => (z.sim ?? "msfs") === "msfs" && z.titel_liste.length > 0);
  const schluessel = msfs.map(zeilenSchluessel).join("\n");
  const [karte, setKarte] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    if (!msfs.length) return;
    let lebt = true;
    invoke<Array<string | null>>("vermessung_profile", {
      flugzeuge: msfs.map((z) => ({ titel: z.titel_liste, icao: z.icao })),
    })
      .then((r) => {
        if (!lebt || !Array.isArray(r)) return;
        const m = new Map<string, string>();
        msfs.forEach((z, i) => {
          const p = r[i];
          if (p) m.set(zeilenSchluessel(z), p);
        });
        setKarte(m);
      })
      .catch(() => undefined);
    return () => {
      lebt = false;
    };
    // Nur neu fragen, wenn sich die Zeilen ändern.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [schluessel]);
  return karte;
}

/** Spalte „Variablen (Scan)": ob für das Muster L:-Namen vorliegen. */
function ScanZelle({ sim, anzahl }: { sim: string | null; anzahl: number | null }) {
  const { t } = useTranslation();
  if (sim === "xplane") return <span className="vm-zelle vm-zelle--leise">{t("vermessen.scan_unnoetig")}</span>;
  if (anzahl === null) return <span className="vm-dim">–</span>;
  return anzahl > 0 ? (
    <span className="vm-zelle vm-zelle--ok">✓ {t("vermessen.scan_anzahl", { anzahl })}</span>
  ) : (
    <span className="vm-zelle vm-zelle--fehlt">{t("vermessen.scan_keiner")}</span>
  );
}

/** MSFS: wie viele L:-Namen die Aircraft-Scans für das geladene Flugzeug
 *  liefern. `null` = unbekannt/nicht MSFS (dann kein Hinweis). */
function useScanNamen(sim: SimStatus | null): number | null {
  const istMsfs = sim?.kind === "msfs2020" || sim?.kind === "msfs2024";
  const titel = sim?.snapshot?.aircraft_title ?? "";
  const icao = sim?.snapshot?.aircraft_icao ?? "";
  const [stand, setStand] = useState<{ schluessel: string; anzahl: number | null }>({ schluessel: "", anzahl: null });
  // Cloud-QS 29.09.2026 (P3): der aircraft.cfg-Pfad kommt mit `AircraftLoaded`
  // oft erst nach dem ersten Statustakt. Gehört er zum Schlüssel, fragt die
  // Startseite neu, sobald er da ist — sonst blieb die Anzahl ohne
  // Ordner-Zuordnung stehen (auch 0), obwohl die Messung den Pfad nutzt.
  // Den Pfad selbst holt der Befehl aus dem Snapshot.
  const pfad = sim?.snapshot?.cockpit_rohwerte?.cfg_pfad ?? "";
  const schluessel = istMsfs && titel ? `${icao}|${titel}|${pfad}` : "";
  useEffect(() => {
    if (!schluessel) return;
    let lebt = true;
    invoke<number>("vermessung_scan_namen", { icao, titel })
      .then((n) => lebt && setStand({ schluessel, anzahl: typeof n === "number" ? n : null }))
      .catch(() => lebt && setStand({ schluessel, anzahl: null }));
    return () => {
      lebt = false;
    };
    // Nur bei Flugzeugwechsel neu fragen, nicht bei jedem Statustakt.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [schluessel]);
  return schluessel && stand.schluessel === schluessel ? stand.anzahl : null;
}

function Kopf({ flugzeug, text }: { flugzeug: string; text?: string }) {
  return (
    <div className="vm-kopf">
      <span className="vm-flugzeug">{flugzeug || "—"}</span>
      {text && <span className="vm-dim">{text}</span>}
    </div>
  );
}

function Fortschritt({ sekunden, text }: { sekunden: number; text: string }) {
  const [rest, setRest] = useState(sekunden);
  useEffect(() => {
    const iv = setInterval(() => setRest((r) => Math.max(0, r - 1)), 1000);
    return () => clearInterval(iv);
  }, []);
  return (
    <div className="vm-fortschritt" aria-live="polite">
      <div className="vm-balken">
        <div className="vm-balken-fuellung" style={{ animationDuration: `${sekunden}s` }} />
      </div>
      <span>
        {text}
        {rest > 0 ? `${rest} s` : ""}
      </span>
    </div>
  );
}

function SchrittKarte({
  plan,
  phase,
  ergebnisse,
  onBeginnen,
  onUeberspringen,
  onErledigt,
  onGibtEsNicht,
  onLetzte,
  onNeu,
  onWeiter,
  onAbbrechen,
}: {
  plan: SchrittDef[];
  phase: Extract<Phase, { art: "schritt" }>;
  ergebnisse: Record<string, Ergebnis>;
  onBeginnen: () => void;
  onUeberspringen: () => void;
  onErledigt: () => void;
  onGibtEsNicht: () => void;
  onLetzte: () => void;
  onNeu: () => void;
  onWeiter: () => void;
  onAbbrechen: () => void;
}) {
  const { t, i18n } = useTranslation();
  const def = plan[phase.nr]!;
  const k = `vermessen.schritt.${def.schalter}`;
  const hinweis = i18n.exists(`${k}.hinweis`) ? t(`${k}.hinweis`) : null;
  // Luft-Schritte: wann, eigene Einstiegsfrage und je Stellung eine genaue
  // Anweisung (was drücken, woran man im FMA sieht, dass es geklappt hat).
  const wann = i18n.exists(`${k}.wann`) ? t(`${k}.wann`) : null;
  const frage = i18n.exists(`${k}.frage`) ? t(`${k}.frage`) : t("vermessen.hat_schalter");
  const tun = i18n.exists(`${k}.tun.${phase.stellung}`) ? t(`${k}.tun.${phase.stellung}`) : null;
  const letzteRueck = phase.rueckmeldungen[phase.rueckmeldungen.length - 1];
  const nichtsBewegt = !!letzteRueck?.antwort && !letzteRueck.antwort.erste && letzteRueck.antwort.mitgegangen === 0;
  const fertig = phase.abschluss !== null;
  const gemessen = phase.rueckmeldungen.filter((r) => r.antwort).length;

  return (
    <section className="vm-karte">
      <div className="vm-schritte" aria-hidden>
        {plan.map((s, i) => (
          <span
            key={s.schalter}
            className={`vm-punkt${i === phase.nr ? " vm-punkt--jetzt" : ""}${
              ergebnisse[s.schalter]?.uebersprungen
                ? " vm-punkt--weg"
                : ergebnisse[s.schalter]
                  ? ergebnisse[s.schalter]!.kandidaten > 0
                    ? " vm-punkt--fertig"
                    : " vm-punkt--leer"
                  : ""
            }`}
          />
        ))}
      </div>
      <div className="vm-dim vm-klein">{t("vermessen.schritt_von", { nr: phase.nr + 1, von: plan.length })}</div>
      <h3 className="vm-titel">{t(`${k}.titel`)}</h3>

      <div className="vm-info">
        {wann && (
          <div className="vm-info-wann">
            <div className="vm-info-titel">{t("vermessen.wann")}</div>
            <div>{wann}</div>
          </div>
        )}
        <div>
          <div className="vm-info-titel">{t("vermessen.wo")}</div>
          <div>{t(`${k}.wo`)}</div>
        </div>
        <div>
          <div className="vm-info-titel">{t("vermessen.heisst")}</div>
          <div className="vm-mono">{t(`${k}.namen`)}</div>
        </div>
      </div>
      {hinweis && <p className="vm-hinweis">{hinweis}</p>}

      {!phase.begonnen ? (
        <>
          <p className="vm-frage">{frage}</p>
          <div className="vm-knoepfe">
            <button type="button" className="button button--primary" onClick={onBeginnen}>
              {t("vermessen.ja_starten")}
            </button>
            <button type="button" className="button" onClick={onUeberspringen}>
              {t("vermessen.nein_ueberspringen")}
            </button>
          </div>
        </>
      ) : (
        <>
          <ol className="vm-stellungen">
            {def.stellungen.map((s, i) => {
              const r = phase.rueckmeldungen[i];
              const jetzt = !fertig && i === phase.stellung;
              const leer = !!r?.antwort && !r.antwort.erste && r.antwort.mitgegangen === 0;
              const zustand = r ? (r.antwort ? (leer ? "leer" : "fertig") : "fehlt") : jetzt ? "jetzt" : i < phase.stellung ? "fehlt" : "offen";
              if (fertig && !r) return null;
              return (
                <li key={`${s}-${i}`} className={`vm-stellung vm-stellung--${zustand}`}>
                  <span className="vm-stellung-marke" aria-hidden>
                    {zustand === "fertig" ? "✓" : zustand === "leer" ? "!" : zustand === "fehlt" ? "–" : i + 1}
                  </span>
                  <span className="vm-stellung-name">{t(`vermessen.stellung.${s}`)}</span>
                  <span className="vm-stellung-rueck">
                    {r?.antwort
                      ? r.antwort.erste
                        ? t("vermessen.ausgang")
                        : r.antwort.mitgegangen === 1
                          ? t("vermessen.bewegt_eins")
                          : r.antwort.mitgegangen > 0
                            ? t("vermessen.bewegt", { anzahl: r.antwort.mitgegangen })
                            : t("vermessen.keine_bewegung")
                      : r
                        ? t("vermessen.gibt_es_nicht")
                        : ""}
                  </span>
                </li>
              );
            })}
          </ol>

          {!fertig && (
            <div className="vm-aufgabe">
              {tun ? (
                <>
                  <div className="vm-aufgabe-text">
                    <strong>{t(`vermessen.stellung.${def.stellungen[phase.stellung]}`)}</strong>
                  </div>
                  <p className="vm-aufgabe-tun">{tun}</p>
                </>
              ) : (
                <div className="vm-aufgabe-text">
                  {t("vermessen.stelle_auf")} <strong>{t(`vermessen.stellung.${def.stellungen[phase.stellung]}`)}</strong>
                </div>
              )}
              <div className="vm-knoepfe">
                <button type="button" className="button button--primary" disabled={phase.misst} onClick={onErledigt}>
                  {phase.misst ? t("vermessen.messe") : t("vermessen.erledigt")}
                </button>
                {phase.stellung > 0 && (
                  <button type="button" className="button" disabled={phase.misst} onClick={onGibtEsNicht}>
                    {t("vermessen.gibt_es_nicht")}
                  </button>
                )}
                {def.offenesEnde && gemessen >= 2 && (
                  <button type="button" className="button" disabled={phase.misst} onClick={onLetzte}>
                    {t("vermessen.letzte_raste")}
                  </button>
                )}
              </div>
            </div>
          )}

          {nichtsBewegt && !fertig && (
            <div className="vm-hinweis vm-hinweis--warn">
              {t("vermessen.nichts_bewegt")}{" "}
              <button type="button" className="vm-link" onClick={onNeu}>
                {t("vermessen.neu_beginnen")}
              </button>
            </div>
          )}

          {fertig && phase.abschluss && (
            <div className={phase.abschluss.kandidaten > 0 ? "vm-ergebnis vm-ergebnis--ok" : "vm-ergebnis vm-ergebnis--warn"}>
              {phase.abschluss.kandidaten > 0
                ? t("vermessen.gefunden", { anzahl: phase.abschluss.kandidaten })
                : t("vermessen.nichts_gefunden")}
              {phase.abschluss.beispiele.length > 0 && (
                <ul className="vm-beispiele vm-mono">
                  {phase.abschluss.beispiele.map((b) => (
                    <li key={b.variable}>
                      {b.variable}:{" "}
                      {b.texte
                        ? b.texte.map((x) => (x == null ? "—" : `"${x}"`)).join(" / ")
                        : b.werte.map((w) => (w == null ? "—" : String(Math.round(w * 1000) / 1000))).join(" / ")}
                    </li>
                  ))}
                </ul>
              )}
              <div className="vm-knoepfe">
                <button type="button" className="button button--primary" onClick={onWeiter}>
                  {phase.nr + 1 < plan.length ? t("vermessen.naechster") : t("vermessen.zur_uebersicht")}
                </button>
                <button type="button" className="button" onClick={onNeu}>
                  {t("vermessen.neu_beginnen")}
                </button>
              </div>
            </div>
          )}
        </>
      )}

      <div className="vm-fuss">
        <button type="button" className="vm-link vm-dim" onClick={onAbbrechen}>
          {t("vermessen.abbrechen")}
        </button>
      </div>
    </section>
  );
}
