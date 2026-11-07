// Uebernahme des Eingangs (P-11.30, Vertrag 07.11.): Rust legt jeden Fund eines
// aktiven Flugs persistent im Eingang ab (`bilder_liste().funde`); das Paket arbeitet
// ihn STRIKT NACHEINANDER ab. Das Ereignis `bild_quelle_neu` ist nur ein Wecksignal.
// Ablauf je Fund: Regel (verwerfen oder uebernehmen) -> Eingangskopie lesen ->
// verkleinern + JPEG neu kodieren (Canvas, EXIF-frei) -> SHA-256 ueber diese Bytes
// (kanonisch) -> EXIF-GPS fuer die Datei auf dem Rechner -> `bild_schreiben` mit Hash.
// Ein Duplikat lehnt Rust atomar ab (Code `duplikat`).
import type { BildAktionen, Uebernahme } from "./api";
import { alsBase64, sha256Hex } from "./bildverarbeitung";
import { exifEinsetzen, exifGpsSegment } from "./exifGps";
import { fehlerCode, fehlerText } from "./normalisieren";
import { TABELLE } from "./grenzen";
import { bildMetaBauen, datenStufe, hatPosition, isoVonMs, type DatenStufe } from "./meta";
import { entscheide, gruppeFuer } from "./uebernahmeRegel";
import type { BilderListe, Eintrag, Fund, MetaBasis, MetaQuelle, Sidecar } from "./typen";

export interface Kodierer {
  (bytes: Uint8Array, mime: string): Promise<{ jpeg: Uint8Array; vorschau: Uint8Array; breite: number; hoehe: number }>;
}

export interface UebernehmerAbhaengig {
  api: BildAktionen;
  kodieren: Kodierer;
  jetzt?: () => number;
}

export interface Zaehlung {
  neu: number;
  verworfen: number;
  duplikate: number;
  fehler: number;
  mit_daten: number;
  nur_flug: number;
  ohne_daten: number;
  /** Datei wurde waehrend der Uebernahme ueberschrieben (`bild_veraltet`): Fund verworfen, die Datei kommt als neuer Fund. */
  veraltet: number;
  letzterFehler: string | null;
}

export const leereZaehlung = (): Zaehlung => ({
  neu: 0, verworfen: 0, duplikate: 0, fehler: 0, mit_daten: 0, nur_flug: 0, ohne_daten: 0, veraltet: 0, letzterFehler: null,
});

const mimeVon = (format: string | null | undefined, pfad: string): string =>
  (format ?? "").toLowerCase().startsWith("jp") || /\.jpe?g$/i.test(pfad) ? "image/jpeg" : "image/png";

/** Wie oft ein Fund scheitern darf, bevor er in dieser Sitzung liegen bleibt (Anzeige, kein Dauerlauf). */
export const MAX_VERSUCHE = 3;

/** Ordnername einer Quelle fuer die Gruppenzeile („screenshots“, „Output/screenshots“). */
export function herkunftVon(pfad: string | null): string | null {
  if (!pfad) return null;
  const teile = pfad.split(/[\\/]+/).filter(Boolean);
  return teile.length === 0 ? null : teile.slice(-2).join("/");
}

export function erzeugeUebernehmer(dep: UebernehmerAbhaengig) {
  const { api, kodieren } = dep;
  const jetzt = dep.jetzt ?? Date.now;
  const versuche = new Map<string, number>();
  let laeuft = false;
  let nochmal = false;

  /** Eine uebernommene Eingangskopie als fertiges Bild ablegen. */
  async function aufnehmen(u: Uebernahme, basis: MetaBasis, quelle: MetaQuelle, z: Zaehlung, herkunft: string | null = null): Promise<void> {
    const bytes = await api.lesen(u.quelle_pfad);
    const k = await kodieren(bytes, mimeVon(u.format, u.quelle_pfad));
    // Kanonischer Hash: nach Verkleinern und Neukodierung, ohne EXIF (P-7.25).
    const sha = await sha256Hex(k.jpeg);
    const meta = bildMetaBauen({ ...basis, zeit_utc: basis.zeit_utc ?? isoVonMs(u.dateizeit_ms) ?? undefined }, quelle, u.dateizeit_ms);
    const datei = hatPosition(meta)
      ? exifEinsetzen(k.jpeg, exifGpsSegment({ breite: meta.breite!, laenge: meta.laenge!, hoehe_ft: meta.hoehe_ft, kurs: meta.kurs, zeit_utc: meta.zeit_utc }))
      : k.jpeg;
    const sidecar: Sidecar = {
      fassung: 1, bild_id: u.id, text: "", favorit: false, meta, breite_px: k.breite, hoehe_px: k.hoehe,
      sha256: sha, erstellt_ms: jetzt(), sendungen: [], herkunft,
    };
    try {
      await api.schreiben({ gruppe: u.gruppe, id: u.id, jpegBase64: alsBase64(datei), vorschauBase64: alsBase64(k.vorschau), sidecar, sha256: sha, quelleEntfernen: true });
    } catch (e) {
      if (fehlerCode(e) === "duplikat") { z.duplikate++; return; }
      throw e;
    }
    z.neu++;
    z[datenStufe(meta)]++;
  }

  async function einFund(f: Fund, liste: BilderListe, z: Zaehlung): Promise<void> {
    const gruppe = gruppeFuer(f.basis.pirep_id, TABELLE.ohneFlugOrdner);
    const d = entscheide(f, liste.quellen);
    if (d.tat === "verwerfen") {
      await api.uebernehmen({ fundId: f.fund_id, modus: "verwerfen", gruppe });
      z.verworfen++;
      return;
    }
    let u: Uebernahme | null;
    try {
      try {
        u = await api.uebernehmen({ fundId: f.fund_id, modus: d.modus, gruppe });
      } catch (e) {
        // „Verschieben“ nur mit Freigabe der Quelle: ohne sie lieber kopieren als verlieren.
        if (d.modus !== "verschieben" || fehlerCode(e) === "bild_veraltet") throw e;
        u = await api.uebernehmen({ fundId: f.fund_id, modus: "kopieren", gruppe });
      }
    } catch (e) {
      if (fehlerCode(e) !== "bild_veraltet") throw e;
      // Die Datei wurde unter demselben Namen ueberschrieben: nichts wurde kopiert. Den alten Fund verwerfen,
      // die Datei meldet der Kern danach als neuen Fund (sonst bliebe er mit altem Hash fuer immer liegen).
      await api.uebernehmen({ fundId: f.fund_id, modus: "verwerfen", gruppe });
      z.veraltet++;
      return;
    }
    if (!u) throw new Error("bild_uebernehmen lieferte nichts");
    await aufnehmen(u, f.basis, "sim", z, herkunftVon(f.quelle));
  }

  /** Eingangskopie ohne Sidecar (Absturz zwischen Uebernahme und Ablage): Daten aus dem Kontext der Dateizeit. */
  async function einRest(e: Eintrag, z: Zaehlung): Promise<void> {
    const u: Uebernahme = { id: e.id, gruppe: e.gruppe, quelle_pfad: e.quelle_pfad!, format: e.quelle_pfad!.split(".").pop() ?? "png", dateizeit_ms: e.dateizeit_ms };
    const k = await kontextZuZeit(u.dateizeit_ms);
    await aufnehmen(u, k.basis, k.quelle, z);
  }

  async function kontextZuZeit(ms: number | null): Promise<{ basis: MetaBasis; quelle: MetaQuelle }> {
    const iso = isoVonMs(ms);
    if (!iso) return { basis: {}, quelle: "keine" };
    try {
      const k = await api.kontext({ zeiten: [iso] });
      const s = k.staende[0];
      return s ? { basis: s.basis, quelle: s.quelle } : { basis: {}, quelle: "keine" };
    } catch {
      return { basis: {}, quelle: "keine" };
    }
  }

  function melden(z: Zaehlung, id: string, e: unknown) {
    z.fehler++;
    z.letzterFehler = `${fehlerCode(e)}: ${fehlerText(e)}`;
    versuche.set(id, (versuche.get(id) ?? 0) + 1);
  }

  /** Eingang abarbeiten, ein Eintrag nach dem anderen. Weckt ein zweiter Aufruf, laeuft ein weiterer Durchgang. */
  async function eingangAbarbeiten(): Promise<Zaehlung> {
    const z = leereZaehlung();
    if (laeuft) {
      nochmal = true;
      return z;
    }
    laeuft = true;
    try {
      do {
        nochmal = false;
        const liste = await api.liste(false);
        for (const f of liste.funde) {
          if ((versuche.get(f.fund_id) ?? 0) >= MAX_VERSUCHE) continue;
          try {
            await einFund(f, liste, z);
          } catch (e) {
            melden(z, f.fund_id, e);
          }
        }
        // Die Eingangskopie einer Aufnahme steht auch in `eintraege`; sie gehoert dem Fund und ist nach ihm umgezogen.
        const vonFunden = new Set(liste.funde.map((f) => f.pfad));
        for (const e of liste.eintraege) {
          if (e.vollstaendig || !e.quelle_pfad || vonFunden.has(e.quelle_pfad) || (versuche.get(e.id) ?? 0) >= MAX_VERSUCHE) continue;
          try {
            await einRest(e, z);
          } catch (err) {
            melden(z, e.id, err);
          }
        }
      } while (nochmal);
    } finally {
      laeuft = false;
    }
    return z;
  }

  /** Vom Piloten gewaehlte oder gezogene Dateien, Altbestand: kopiert, Daten aus dem Kontext der Dateizeit. */
  async function hinzufuegen(pfade: string[] | undefined, fortschritt?: (z: Zaehlung) => void): Promise<{ abgebrochen: boolean; zaehlung: Zaehlung }> {
    const z = leereZaehlung();
    const r = await api.hinzufuegen({ gruppe: TABELLE.ohneFlugOrdner, pfade });
    if (r.abgebrochen) return { abgebrochen: true, zaehlung: z };
    for (const e of r.ergebnisse) {
      if (!e.ok || !e.uebernahme) {
        z.fehler++;
        z.letzterFehler = `${e.code ?? "unbekannt"}: ${e.meldung ?? e.pfad}`;
        continue;
      }
      try {
        const k = await kontextZuZeit(e.uebernahme.dateizeit_ms);
        await aufnehmen(e.uebernahme, k.basis, k.quelle, z);
      } catch (err) {
        melden(z, e.uebernahme.id, err);
      }
      fortschritt?.({ ...z });
    }
    return { abgebrochen: false, zaehlung: z };
  }

  return { eingangAbarbeiten, hinzufuegen };
}

export type Uebernehmer = ReturnType<typeof erzeugeUebernehmer>;
export type { DatenStufe };
