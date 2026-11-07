// Antworten des Programms in feste Formen bringen. Der Rust-Teil darf Felder
// hinzufuegen und Namen leicht abweichen lassen; das Paket liest hier an EINER
// Stelle. Fehlendes bleibt null, nie ein Ersatzwert (P-11.30: kein 0 ft).
import type {
  AltEintrag, BilderListe, Eintrag, Fund, Kontext, KontextStand, MetaBasis, MetaQuelle, QuellenStatus, FlugEintrag } from "./typen";

type Roh = Record<string, unknown>;

const obj = (v: unknown): Roh => (v && typeof v === "object" && !Array.isArray(v) ? (v as Roh) : {});
const zahl = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const text = (v: unknown): string | null => (typeof v === "string" && v.trim() !== "" ? v : null);
const feld = (v: unknown, n = 0): number => zahl(v) ?? n;

/** Felder von `bild_meta`, die aus einem Stand gelesen werden. */
export function basisLesen(roh: unknown): MetaBasis {
  const r = obj(roh);
  const b: MetaBasis = {};
  const setzen = <K extends keyof MetaBasis>(k: K, w: MetaBasis[K] | null) => {
    if (w !== null) b[k] = w;
  };
  setzen("zeit_utc", text(r.zeit_utc));
  setzen("breite", zahl(r.breite));
  setzen("laenge", zahl(r.laenge));
  setzen("hoehe_ft", zahl(r.hoehe_ft));
  setzen("gs_kt", zahl(r.gs_kt));
  setzen("kurs", zahl(r.kurs));
  setzen("flugzeug_titel", text(r.flugzeug_titel));
  setzen("flugzeug_icao", text(r.flugzeug_icao));
  setzen("phase", text(r.phase));
  setzen("pirep_id", text(r.pirep_id));
  setzen("rufzeichen", text(r.rufzeichen));
  setzen("abflug", text(r.abflug));
  setzen("ziel", text(r.ziel));
  setzen("simulator", text(r.simulator));
  setzen("naechster_flugplatz", text(r.naechster_flugplatz) ?? text(obj(r.naechster_flugplatz).icao)); // Rust: {icao, abstand_m}
  return b;
}

export function fundLesen(roh: unknown): Fund | null {
  const r = obj(roh);
  const id = text(r.fund_id) ?? text(r.id);
  if (!id) return null;
  const verschachtelt = r.meta_basis ?? r.basis ?? r.meta;
  // Flach: `breite` ist dann die geografische Breite, wenn `laenge` daneben steht.
  const flachGeo = verschachtelt === undefined && r.laenge !== undefined;
  const basis = basisLesen(verschachtelt ?? r);
  const pxB = zahl(r.px_breite) ?? zahl(obj(r.pixel).breite) ?? (flachGeo ? null : zahl(r.breite));
  const pxH = zahl(r.px_hoehe) ?? zahl(obj(r.pixel).hoehe) ?? (flachGeo ? null : zahl(r.hoehe));
  const fenster = obj(r.sim_fenster);
  const fb = zahl(fenster.breite);
  const fh = zahl(fenster.hoehe);
  return {
    fund_id: id,
    pfad: text(r.pfad) ?? text(r.quelle_pfad),
    dateizeit_ms: zahl(r.dateizeit_ms),
    groesse: feld(r.groesse),
    format: text(r.format),
    uebernehmbar: r.uebernehmbar !== false,
    px_breite: pxB,
    px_hoehe: pxH,
    quelle: text(r.quelle) ?? text(r.quelle_ordner),
    art: text(r.art) ?? text(r.quelle_art),
    sim_vordergrund: typeof r.sim_vordergrund === "boolean" ? r.sim_vordergrund : null,
    sim_fenster: fb !== null && fh !== null ? { breite: fb, hoehe: fh } : null,
    basis,
  };
}

function altLesen(roh: unknown): AltEintrag | null {
  const r = obj(roh);
  const pfad = text(r.pfad);
  if (!pfad) return null;
  return { pfad, dateizeit_ms: zahl(r.dateizeit_ms), groesse: feld(r.groesse), format: text(r.format) ?? "", quelle: text(r.quelle) ?? "" };
}

function eintragLesen(roh: unknown): Eintrag | null {
  const r = obj(roh);
  const id = text(r.id);
  if (!id) return null;
  return {
    id,
    gruppe: text(r.gruppe) ?? "",
    pfad: text(r.pfad),
    vorschau_pfad: text(r.vorschau_pfad),
    quelle_pfad: text(r.quelle_pfad),
    groesse: feld(r.groesse),
    vorschau_groesse: feld(r.vorschau_groesse),
    quelle_groesse: feld(r.quelle_groesse),
    belegt_bytes: feld(r.belegt_bytes),
    dateizeit_ms: zahl(r.dateizeit_ms),
    sidecar: r.sidecar && typeof r.sidecar === "object" ? (r.sidecar as Eintrag["sidecar"]) : null,
    sidecar_fehler: text(r.sidecar_fehler),
    vollstaendig: r.vollstaendig === true,
  };
}

export function quelleLesen(roh: unknown): QuellenStatus | null {
  const r = obj(roh);
  const pfad = text(r.pfad);
  if (!pfad) return null;
  return {
    pfad,
    aktiv: r.aktiv !== false,
    original_entfernen: r.original_entfernen === true,
    art: text(r.art),
    beobachtet: r.beobachtet === true,
    wird_geprueft: r.wird_geprueft === true,
    fehler: text(r.fehler),
    altbestand: feld(r.altbestand),
  };
}

const liste = <T>(v: unknown, f: (x: unknown) => T | null): T[] =>
  (Array.isArray(v) ? v : []).map(f).filter((x): x is T => x !== null);

export function yokeLesen(roh: unknown): import("./typen").YokeTaste | null {
  const y = obj(roh);
  const id = (v: unknown): number | string | null => (typeof v === "number" || (typeof v === "string" && v !== "") ? v : null);
  const knopf = id(y.knopf), hersteller = id(y.hersteller_id), produkt = id(y.produkt_id);
  return knopf !== null && hersteller !== null && produkt !== null ? { geraet_name: text(y.geraet_name) ?? "", hersteller_id: hersteller, produkt_id: produkt, knopf } : null;
}

export function listeLesen(roh: unknown): BilderListe {
  const r = obj(roh);
  const k = obj(r.tastenkuerzel);
  return {
    eintraege: liste(r.eintraege, eintragLesen),
    belegt_bytes: feld(r.belegt_bytes),
    quellen: liste(r.quellen, quelleLesen),
    tastenkuerzel: text(k.text)
      ? { text: String(k.text), verfuegbar: k.verfuegbar === true, aktiv: k.aktiv === true, fehler: text(k.fehler), yoke_verfuegbar: k.yoke_verfuegbar === true, yoke: yokeLesen(k.yoke) }
      : null,
    aufnahme_verfuegbar: r.aufnahme_verfuegbar === true,
    simulator_taste: text(obj(r.simulator_taste).text) ? { simulator: text(obj(r.simulator_taste).simulator) ?? "", text: String(obj(r.simulator_taste).text) } : null,
    funde: liste(r.funde, fundLesen),
    altbestand: liste(r.altbestand, altLesen),
  };
}

function quelleVon(v: unknown): MetaQuelle {
  return v === "protokoll" || v === "logbuch" || v === "sim" ? v : "keine";
}

function standLesen(roh: unknown): KontextStand {
  const r = obj(roh);
  // Die Angaben koennen flach oder unter `stand`/`basis` liegen.
  const innen = r.stand ?? r.basis ?? r.meta ?? r;
  return { zeit: text(r.zeit), quelle: quelleVon(r.quelle ?? obj(innen).quelle), basis: basisLesen(innen) };
}

export function kontextLesen(roh: unknown): Kontext {
  const r = obj(roh);
  const staende = Array.isArray(roh) ? roh : Array.isArray(r.staende) ? r.staende : Array.isArray(r.stand) ? r.stand : [];
  const spurRoh = r.spur;
  const spur = Array.isArray(spurRoh)
    ? spurRoh
        .map((p): [number, number] | null => {
          if (Array.isArray(p) && zahl(p[0]) !== null && zahl(p[1]) !== null) return [p[0] as number, p[1] as number];
          const o = obj(p);
          return zahl(o.lat) !== null && zahl(o.lon) !== null ? [o.lat as number, o.lon as number] : null;
        })
        .filter((p): p is [number, number] => p !== null)
    : null;
  const fluege = Array.isArray(r.fluege) ? r.fluege.map(flugLesen).filter((f): f is FlugEintrag => f !== null) : undefined;
  return { staende: staende.map(standLesen), spur, ...(fluege ? { fluege, fluegeGekuerzt: r.fluege_gekuerzt === true, logbuchUnvollstaendig: r.logbuch_unvollstaendig === true } : {}) };
}

function flugLesen(roh: unknown): FlugEintrag | null {
  const r = obj(roh);
  const pirep_id = text(r.pirep_id);
  return pirep_id === null ? null : {
    pirep_id, rufzeichen: text(r.rufzeichen), flugnummer: text(r.flugnummer), abflug: text(r.abflug), ziel: text(r.ziel), flugzeug_icao: text(r.flugzeug_icao), start_ms: zahl(r.start_ms), ende_ms: zahl(r.ende_ms),
  };
}

/** Fehlerobjekt von `invoke`: Code und Meldung, sonst ein Text. */
export function fehlerCode(e: unknown): string {
  if (e && typeof e === "object") {
    const c = (e as { code?: unknown }).code;
    if (typeof c === "string") return c;
  }
  return "unbekannt";
}

export function fehlerText(e: unknown): string {
  if (e && typeof e === "object") {
    const m = (e as { message?: unknown }).message;
    if (typeof m === "string") return m;
  }
  return String(e);
}
