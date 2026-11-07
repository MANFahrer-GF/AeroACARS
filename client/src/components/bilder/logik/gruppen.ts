// Liste nach Flug gruppiert, Suche, Filter, seitenweise, Platz je Flug (P-11.30, P-11.31).
import type { Grenzen } from "./grenzen";
import { streckeText } from "./metaFormat";
import { bildZeit, serienBilden, type Kachel } from "./serien";
import type { Eintrag, FlugEintrag } from "./typen";

export type GesendetFilter = "alle" | "gesendet" | "offen";

export interface Filter {
  suche: string;
  favoriten: boolean;
  gesendet: GesendetFilter;
  /** Nur dieser Flug (Schluessel der Gruppe, "" = ohne Flug); null = alle. */
  flug: string | null;
}

export const KEIN_FILTER: Filter = { suche: "", favoriten: false, gesendet: "alle", flug: null };

export interface Gruppe {
  /** PIREP-ID, "" fuer „Ohne Flug“. */
  schluessel: string;
  rufzeichen: string | null;
  abflug: string | null;
  ziel: string | null;
  flugzeug: string | null;
  /** Zeit des neuesten Bildes (ms). */
  zeit: number;
  anzahl: number;
  belegt_bytes: number;
  kacheln: Kachel[];
}

/** Nur vollstaendige Eintraege (JPEG und lesbares Sidecar) sind Bilder im Tab. */
export function anzeigbar(e: Eintrag): boolean {
  return e.vollstaendig && e.pfad !== null && !!e.sidecar?.meta;
}

export const flugSchluessel = (e: Eintrag): string => e.sidecar?.meta?.pirep_id ?? "";

export const wurdeGesendet = (e: Eintrag): boolean =>
  (e.sidecar?.sendungen ?? []).some((s) => s.status === "gesendet");

/** Suchtext eines Bildes (klein geschrieben). */
function haystack(e: Eintrag): string {
  const m = e.sidecar?.meta;
  const zeit = m?.zeit_utc;
  const tag = zeit ? new Date(zeit) : null;
  // Datum wie in der Anzeige und als ISO: der Platzhalter verspricht die Suche nach dem Datum.
  const datum = tag && !Number.isNaN(tag.getTime()) ? [zeit?.slice(0, 10), tag.toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit", year: "numeric" }), tag.toLocaleDateString("en-GB"), tag.toLocaleDateString("it-IT")] : [];
  return [...datum, e.sidecar?.text, m?.rufzeichen, m?.abflug, m?.ziel, m?.flugzeug_titel, m?.flugzeug_icao, m?.naechster_flugplatz, m?.pirep_id, m?.simulator, m?.phase]
    .filter((x): x is string => typeof x === "string")
    .join(" ")
    .toLowerCase();
}

export function filtern(eintraege: Eintrag[], f: Filter): Eintrag[] {
  const q = f.suche.trim().toLowerCase();
  return eintraege.filter((e) => {
    if (!anzeigbar(e)) return false;
    if (f.favoriten && !e.sidecar?.favorit) return false;
    if (f.gesendet === "gesendet" && !wurdeGesendet(e)) return false;
    if (f.gesendet === "offen" && wurdeGesendet(e)) return false;
    if (f.flug !== null && flugSchluessel(e) !== f.flug) return false;
    return q === "" || q.split(/\s+/).every((w) => haystack(e).includes(w));
  });
}

export function gruppieren(eintraege: Eintrag[], g: Pick<Grenzen, "serien_abstand_s" | "serien_min_bilder">): Gruppe[] {
  const nach = new Map<string, Eintrag[]>();
  for (const e of eintraege) {
    const k = flugSchluessel(e);
    nach.set(k, [...(nach.get(k) ?? []), e]);
  }
  const gruppen: Gruppe[] = [];
  for (const [schluessel, liste] of nach) {
    const neueste = [...liste].sort((a, b) => bildZeit(b) - bildZeit(a));
    // Kopfdaten vom neuesten Bild, das sie hat
    const mit = (f: (m: NonNullable<Eintrag["sidecar"]>["meta"] & object) => string | null | undefined) =>
      neueste.map((e) => (e.sidecar?.meta ? f(e.sidecar.meta) : null)).find((x) => typeof x === "string" && x !== "") ?? null;
    gruppen.push({
      schluessel,
      rufzeichen: mit((m) => m.rufzeichen),
      abflug: mit((m) => m.abflug),
      ziel: mit((m) => m.ziel),
      flugzeug: mit((m) => m.flugzeug_titel ?? m.flugzeug_icao),
      zeit: bildZeit(neueste[0]),
      anzahl: liste.length,
      belegt_bytes: liste.reduce((s, e) => s + e.belegt_bytes, 0),
      kacheln: serienBilden(liste, g.serien_abstand_s, g.serien_min_bilder),
    });
  }
  // Fluege: neuester zuerst; „Ohne Flug“ steht immer zuletzt.
  return gruppen.sort((a, b) => (a.schluessel === "" ? 1 : b.schluessel === "" ? -1 : b.zeit - a.zeit));
}

export interface FlugOption {
  pirep_id: string;
  rufzeichen: string | null;
  abflug: string | null;
  ziel: string | null;
  flugzeug_icao: string | null;
  label: string;
}

/**
 * Die Fluege der Zuordnung von Hand, je PIREP einmal: der laufende Flug, dann die aus Protokoll und
 * Logbuch (der Bildzeit naechster vorn), zuletzt Fluege, die nur noch ueber ihre Bilder bekannt sind.
 */
export function flugOptionenBauen(
  aktiv: { pirep_id: string; callsign: string; dpt: string; arr: string } | null,
  fluege: FlugEintrag[],
  gruppen: Gruppe[],
  sprache: string,
  laeuft: string,
): FlugOption[] {
  const o = new Map<string, FlugOption>();
  const name = (zeit: number | null, f: { rufzeichen: string | null; abflug: string | null; ziel: string | null }) =>
    [zeit ? new Date(zeit).toLocaleDateString(sprache) : null, f.rufzeichen, streckeText(f)].filter(Boolean).join(" ");
  if (aktiv) {
    o.set(aktiv.pirep_id, { pirep_id: aktiv.pirep_id, rufzeichen: aktiv.callsign, abflug: aktiv.dpt, ziel: aktiv.arr, flugzeug_icao: null, label: `${aktiv.callsign} ${aktiv.dpt}–${aktiv.arr} (${laeuft})` });
  }
  for (const f of fluege) {
    if (!o.has(f.pirep_id)) {
      const anzeige = { ...f, rufzeichen: f.rufzeichen ?? f.flugnummer };
      o.set(f.pirep_id, { pirep_id: f.pirep_id, rufzeichen: f.rufzeichen, abflug: f.abflug, ziel: f.ziel, flugzeug_icao: f.flugzeug_icao, label: name(f.start_ms ?? f.ende_ms, anzeige) || f.pirep_id });
    }
  }
  for (const g of gruppen) {
    if (g.schluessel === "" || o.has(g.schluessel)) continue;
    o.set(g.schluessel, { pirep_id: g.schluessel, rufzeichen: g.rufzeichen, abflug: g.abflug, ziel: g.ziel, flugzeug_icao: null, label: name(g.zeit, g) || g.schluessel });
  }
  return [...o.values()];
}

export interface Seite {
  gruppen: Gruppe[];
  seite: number;
  seiten: number;
  kacheln: number;
}

/** Seitenweise ueber Kacheln (eine Serie zaehlt als eine); Gruppen werden pro Seite neu geschnitten. */
export function seitenweise(gruppen: Gruppe[], seite: number, proSeite: number): Seite {
  const flach = gruppen.flatMap((g) => g.kacheln.map((k) => ({ g, k })));
  const seiten = Math.max(1, Math.ceil(flach.length / proSeite));
  const s = Math.min(Math.max(0, seite), seiten - 1);
  const ausschnitt = flach.slice(s * proSeite, (s + 1) * proSeite);
  const ergebnis: Gruppe[] = [];
  for (const { g, k } of ausschnitt) {
    const letzte = ergebnis[ergebnis.length - 1];
    if (letzte && letzte.schluessel === g.schluessel) letzte.kacheln.push(k);
    else ergebnis.push({ ...g, kacheln: [k] });
  }
  return { gruppen: ergebnis, seite: s, seiten, kacheln: flach.length };
}

/** Aufraeumen (P-11.31): gesendete Bilder, die aelter als `tage` Tage sind. Loeschen bleibt eine Handlung des Piloten (Auswahl, Papierkorb). */
export function gesendeteAelterAls(eintraege: Eintrag[], tage: number, jetzt: number): Eintrag[] {
  const grenze = jetzt - tage * 86400000;
  return eintraege.filter((e) => anzeigbar(e) && (e.sidecar?.sendungen ?? []).some((s) => s.status === "gesendet") && bildZeit(e) < grenze);
}

/** Belegter Platz je Flug (alle Teile auf der Platte). */
export function platzJeFlug(eintraege: Eintrag[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const e of eintraege) m.set(flugSchluessel(e), (m.get(flugSchluessel(e)) ?? 0) + e.belegt_bytes);
  return m;
}

/** Bytes lesbar (1024er), ohne Sprache: Einheiten sind international. */
export function bytesText(n: number, sprache = "de"): string {
  const f = (x: number, d: number) => x.toLocaleString(sprache, { maximumFractionDigits: d, minimumFractionDigits: d });
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${f(n / 1024, 0)} KB`;
  if (n < 1024 ** 3) return `${f(n / 1024 ** 2, 1)} MB`;
  return `${f(n / 1024 ** 3, 2)} GB`;
}
