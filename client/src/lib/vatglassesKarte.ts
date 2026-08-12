// VATGlasses-Sektoren für die Client-Karte — Zuständigkeit nach HÖHE.
//
// Feldbefund Thomas (12.08.2026, deutlich): flache FIR-Flächen sind für
// Deutschland schlicht falsch — über demselben Punkt sind je nach Höhe
// verschiedene Stationen zuständig, und wer gerade wen mit abdeckt, hängt
// davon ab, wer online ist. Genau das tragen die Daten des
// VATGlasses-Projekts: je Luftraumblock ein Höhenband (min/max als
// Flugfläche), eine Besitzerkette und je Station eine eigene Farbe.
//
// Dieses Modul ist die Browser-Fassung des Rechenkerns aus
// aeroacars-live/recorder/src/vatglasses.ts (dort mit Tests gegen die
// echten deutschen Daten). Unterschiede: kein Dateisystem (die Länder
// kommen per fetch über jsDelivr — der Host steht in der CSP), und statt
// einer Einzelabfrage baut es GeoJSON für die Karte: alle Blöcke einer
// Flugfläche, gefärbt nach der Station, die sie GERADE besitzt.
//
// Datenquelle: github.com/lennycolton/vatglasses-data, CC BY-NC-SA 4.0.
// Namensnennung steht am Umschalter in der Karte.

// ─── Rohformat (nur was gebraucht wird) ──────────────────────────────────

interface RohPosition {
  frequency?: string;
  callsign?: string;
  type?: string;
  pre?: string[];
  colours?: Array<{ hex?: string }>;
}

interface RohSektor {
  min?: number | null;
  max?: number | null;
  points: Array<[string, string]>;
}

interface RohLuftraum {
  id: string;
  group?: string;
  owner: string[];
  sectors: RohSektor[];
}

interface RohDatei {
  positions: Record<string, RohPosition>;
  airspace: RohLuftraum[];
  callsigns?: Record<string, unknown>;
}

// ─── Geladener Bestand ───────────────────────────────────────────────────

interface Station {
  kuerzel: string;
  land: string;
  frequenz: string | null;
  gesprochen: string | null;
  farbe: string | null;
  praefixe: string[];
}

interface Block {
  id: string;
  land: string;
  owner: string[];
  sektoren: Array<{
    flVon: number;
    flBis: number;
    /** [lon, lat] — GeoJSON-Reihenfolge. */
    umriss: Array<[number, number]>;
  }>;
}

/** "491000" → 49.1667 · Süd/West mit Minus. DDMMSS bzw. DDDMMSS. */
export function gradAusDdmmss(t: string): number {
  const neg = t.startsWith("-");
  const z = neg ? t.slice(1) : t;
  const dLen = z.length - 4;
  const d = Number(z.slice(0, dLen));
  const m = Number(z.slice(dLen, dLen + 2));
  const s = Number(z.slice(dLen + 2, dLen + 4));
  const v = d + m / 60 + s / 3600;
  return neg ? -v : v;
}

/**
 * Welche Länderdateien geladen werden. Bewusst eine feste, kleine Liste:
 * das deckt Mitteleuropa — den Raum, in dem die GSG fliegt — mit rund
 * 2 MB einmaligem Abruf. Wer weiter fliegt, sieht dort schlicht keine
 * Sektoren (und weiterhin Lotsen + Piloten); Nachladen nach Sichtfenster
 * wäre der nächste Ausbau, nicht der erste.
 */
export const LAENDER = ["ed", "lo", "ls", "li", "lf", "eb", "eh", "ek", "ep", "lk", "eg", "lh"] as const;

const BASIS = "https://cdn.jsdelivr.net/gh/lennycolton/vatglasses-data@main/data";

let bestandCache: VatglassesBestand | null = null;
let bestandPromise: Promise<VatglassesBestand> | null = null;

export class VatglassesBestand {
  readonly stationen = new Map<string, Station>();
  readonly bloecke: Block[] = [];

  aufnehmen(land: string, roh: RohDatei): void {
    for (const [k, p] of Object.entries(roh.positions ?? {})) {
      // Schlüssel je Land eindeutig — dasselbe Kürzel kann in zwei Ländern
      // vorkommen und verschiedene Stationen meinen.
      this.stationen.set(`${land}/${k}`, {
        kuerzel: k,
        land,
        frequenz: p.frequency ?? null,
        gesprochen: p.callsign ?? null,
        farbe: p.colours?.[0]?.hex ?? null,
        praefixe: p.pre ?? [],
      });
    }
    for (const b of roh.airspace ?? []) {
      const sektoren: Block["sektoren"] = [];
      for (const s of b.sectors ?? []) {
        const umriss = (s.points ?? []).map(
          ([la, lo]) => [gradAusDdmmss(lo), gradAusDdmmss(la)] as [number, number],
        );
        if (umriss.length < 3) continue;
        sektoren.push({ flVon: s.min ?? 0, flBis: s.max ?? 999, umriss });
      }
      if (sektoren.length) {
        this.bloecke.push({ id: b.id, land, owner: b.owner ?? [], sektoren });
      }
    }
  }

  /**
   * Rufzeichen eines Online-Lotsen → Station. Schema wie im Recorder-Kern:
   * Mittelteil des Rufzeichens gegen die Präfixliste der Station geprüft.
   */
  stationFuerRufzeichen(rufzeichen: string): Station | null {
    const teile = rufzeichen.toUpperCase().split("_");
    if (teile.length < 2) return null;
    const kandidat = teile.length >= 3 ? teile[1] : teile[0];
    const praefix = teile[0];
    for (const land of LAENDER) {
      const st = this.stationen.get(`${land}/${kandidat}`);
      if (!st) continue;
      if (st.praefixe.length > 0 && !st.praefixe.some((p) => praefix.startsWith(p))) continue;
      return st;
    }
    return null;
  }
}

/** Länder einmal laden und für die Sitzung behalten. Fehlende Dateien
 *  werden übersprungen — ein Land, das nicht lädt, darf nicht alle
 *  anderen mitreißen. */
export async function ladeBestand(signal?: AbortSignal): Promise<VatglassesBestand> {
  if (bestandCache) return bestandCache;
  if (bestandPromise) return bestandPromise;
  bestandPromise = (async () => {
    const bestand = new VatglassesBestand();
    const ergebnisse = await Promise.allSettled(
      LAENDER.map(async (land) => {
        const res = await fetch(`${BASIS}/${land}.json`, { signal });
        if (!res.ok) throw new Error(`${land}: HTTP ${res.status}`);
        return [land, (await res.json()) as RohDatei] as const;
      }),
    );
    for (const e of ergebnisse) {
      if (e.status === "fulfilled") bestand.aufnehmen(e.value[0], e.value[1]);
    }
    bestandCache = bestand;
    return bestand;
  })();
  return bestandPromise;
}

// ─── GeoJSON für die Karte ───────────────────────────────────────────────

export interface SektorenFuerKarte {
  flaechen: GeoJSON.FeatureCollection;
  marken: GeoJSON.FeatureCollection;
}

/**
 * Alle Blöcke, die auf der gewählten Flugfläche von einer ONLINE-Station
 * besessen werden — gefärbt nach dieser Station, beschriftet EINMAL je
 * Station (am größten Block), nicht je Block: sonst steht dieselbe
 * Frequenz zwölfmal auf der Karte, und genau diese Überlagerung war der
 * Befund.
 */
export function baueSektoren(
  bestand: VatglassesBestand,
  fl: number,
  onlineRufzeichen: string[],
): SektorenFuerKarte {
  // Online-Stationen einmal auflösen: "land/kuerzel" → echtes Rufzeichen.
  const online = new Map<string, string>();
  for (const ruf of onlineRufzeichen) {
    const st = bestand.stationFuerRufzeichen(ruf);
    if (st) {
      const schluessel = `${st.land}/${st.kuerzel}`;
      if (!online.has(schluessel)) online.set(schluessel, ruf);
    }
  }

  const flaechen: GeoJSON.Feature[] = [];
  interface MarkenLage { st: Station; ruf: string; groesste: number; lon: number; lat: number; flVon: number; flBis: number }
  const marken = new Map<string, MarkenLage>();

  for (const block of bestand.bloecke) {
    for (const s of block.sektoren) {
      if (fl < s.flVon || fl > s.flBis) continue;
      // Besitzerkette gegen die Online-Liste — erste online Station gewinnt.
      let besitzer: Station | null = null;
      let ruf = "";
      let stufe = -1;
      for (let i = 0; i < block.owner.length; i++) {
        const schluessel = `${block.land}/${block.owner[i]}`;
        const r = online.get(schluessel);
        if (r) {
          besitzer = bestand.stationen.get(schluessel) ?? null;
          ruf = r;
          stufe = i;
          break;
        }
      }
      if (!besitzer) continue; // Unicom → keine Fläche, Stille ist Information.

      flaechen.push({
        type: "Feature",
        geometry: { type: "Polygon", coordinates: [s.umriss] },
        properties: {
          station: `${besitzer.land}/${besitzer.kuerzel}`,
          ruf,
          gesprochen: besitzer.gesprochen ?? "",
          frequenz: besitzer.frequenz ?? "",
          farbe: besitzer.farbe ?? "#22d3ee",
          block: block.id,
          fl_von: s.flVon,
          fl_bis: s.flBis,
          vertretung: stufe,
        },
      });

      // Markenlage: größter Block je Station trägt das eine Label.
      const schluessel = `${besitzer.land}/${besitzer.kuerzel}`;
      const groesse = s.umriss.length;
      const lon = s.umriss.reduce((a, p) => a + p[0], 0) / s.umriss.length;
      const lat = s.umriss.reduce((a, p) => a + p[1], 0) / s.umriss.length;
      const bisher = marken.get(schluessel);
      if (!bisher || groesse > bisher.groesste) {
        marken.set(schluessel, { st: besitzer, ruf, groesste: groesse, lon, lat, flVon: s.flVon, flBis: s.flBis });
      }
    }
  }

  return {
    flaechen: { type: "FeatureCollection", features: flaechen },
    marken: {
      type: "FeatureCollection",
      features: [...marken.values()].map((m) => ({
        type: "Feature",
        geometry: { type: "Point", coordinates: [m.lon, m.lat] },
        properties: {
          ruf: m.ruf,
          frequenz: m.st.frequenz ?? "",
          gesprochen: m.st.gesprochen ?? "",
          farbe: m.st.farbe ?? "#22d3ee",
        },
      })),
    },
  };
}
