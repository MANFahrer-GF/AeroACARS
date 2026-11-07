// Die Befehle des Bilder-Tabs (Faehigkeit `bilder-1`, P-11.33) als getippte Aufrufe.
// Systembefehle (bild_senden, bild_hinzufuegen, bild_loeschen, bild_quellen_setzen,
// bild_export) geht `invoke` selbst ueber `shell_request` an die Huelle (lib/ipc.ts
// `aufrufZiel`); hier steht nie ein eigener shell_request. Oberste Argumente camelCase,
// Felder verschachtelter Objekte snake_case.
import { invoke } from "../../../lib/ipc";
import { fundLesen, kontextLesen, listeLesen, quelleLesen, yokeLesen } from "./normalisieren";
import type { YokeTaste, BildMeta, BilderListe, Fundort, Kontext, QuellenStatus, SendeAntwort, Sidecar, Quelle, Kontingent } from "./typen";

/** Ergebnis von `bild_uebernehmen` / Eintrag in `bild_hinzufuegen`. */
export interface Uebernahme {
  id: string;
  gruppe: string;
  quelle_pfad: string;
  format: string;
  dateizeit_ms: number | null;
}

export interface HinzufuegenErgebnis {
  pfad: string;
  ok: boolean;
  uebernahme?: Uebernahme;
  code?: string;
  meldung?: string;
}

export interface BildAktionen {
  liste(mitAltbestand?: boolean): Promise<BilderListe>;
  lesen(pfad: string): Promise<Uint8Array>;
  quellenFinden(): Promise<Fundort[]>;
  quellenSetzen(a: { quellen?: Quelle[]; tastenkuerzel?: string; ordnerWaehlen?: boolean; yokeTaste?: YokeTaste | null }): Promise<{ quellen: QuellenStatus[]; abgebrochen: boolean }>;
  uebernehmen(a: { fundId: string; modus: "kopieren" | "verschieben" | "verwerfen"; gruppe: string }): Promise<Uebernahme | null>;
  schreiben(a: {
    gruppe: string; id: string; jpegBase64?: string; vorschauBase64?: string; sidecar?: Sidecar | Partial<Sidecar>; sha256?: string; quelleEntfernen?: boolean;
  }): Promise<void>;
  hinzufuegen(a: { gruppe: string; pfade?: string[] }): Promise<{ abgebrochen: boolean; ergebnisse: HinzufuegenErgebnis[] }>;
  loeschen(a: { gruppe: string; id: string; bezeichnung?: string }): Promise<void>;
  senden(a: { sendungId: string; text: string; geoStufe: string; bilder: Array<{ gruppe: string; bild_id: string; meta: BildMeta }> }): Promise<SendeAntwort>;
  status(sendungId?: string): Promise<SendeAntwort>;
  aufnehmen(): Promise<void>;
  /** Wartet bis zu 10 s auf einen Knopfdruck; null = Zeit abgelaufen. */
  tasteLernen(): Promise<YokeTaste | null>;
  ordnerZeigen(a: { gruppe: string; id: string }): Promise<void>;
  exportieren(a: { zipBase64: string; dateiname: string }): Promise<string | null>;
  kontext(a: { zeiten?: string[]; pirepId?: string; mitSpur?: boolean }): Promise<Kontext>;
}

export function alsBytes(v: unknown): Uint8Array {
  if (v instanceof Uint8Array) return v;
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (Array.isArray(v)) return Uint8Array.from(v as number[]);
  throw new Error("bild_lesen lieferte keine Bytes");
}

function kontingentLesen(v: unknown): Kontingent | null {
  const k = (v ?? {}) as Record<string, unknown>;
  return typeof k.rest === "number" && typeof k.grenze === "number"
    ? { rest: k.rest, grenze: k.grenze, retry_after: typeof k.retry_after === "number" ? k.retry_after : 0 }
    : null;
}

export function sendeAntwortLesen(roh: unknown): SendeAntwort {
  const r = (roh ?? {}) as Record<string, unknown>;
  return {
    sendung_id: typeof r.sendung_id === "string" ? r.sendung_id : undefined,
    bilder: Array.isArray(r.bilder) ? (r.bilder as SendeAntwort["bilder"]) : [],
    kontingent: kontingentLesen(r.kontingent),
  };
}

export const bildAktionen: BildAktionen = {
  async liste(mitAltbestand = false) {
    return listeLesen(await invoke("bilder_liste", { mitAltbestand }));
  },
  async lesen(pfad) {
    return alsBytes(await invoke("bild_lesen", { pfad }));
  },
  quellenFinden: () => invoke<Fundort[]>("bild_quellen_finden"),
  async quellenSetzen(a) {
    const r = (await invoke("bild_quellen_setzen", a)) as { quellen?: unknown[]; abgebrochen?: boolean };
    return { quellen: (r.quellen ?? []).map(quelleLesen).filter((q): q is QuellenStatus => q !== null), abgebrochen: r.abgebrochen === true };
  },
  async uebernehmen(a) {
    const r = (await invoke("bild_uebernehmen", a)) as Uebernahme | null;
    return a.modus === "verwerfen" ? null : r;
  },
  async schreiben(a) {
    await invoke("bild_schreiben", a);
  },
  async hinzufuegen(a) {
    const r = (await invoke("bild_hinzufuegen", a)) as { abgebrochen?: boolean; ergebnisse?: HinzufuegenErgebnis[] };
    return { abgebrochen: r.abgebrochen === true, ergebnisse: r.ergebnisse ?? [] };
  },
  async loeschen(a) {
    await invoke("bild_loeschen", a);
  },
  async senden(a) {
    return sendeAntwortLesen(await invoke("bild_senden", a));
  },
  async status(sendungId) {
    return sendeAntwortLesen(await invoke("bild_status", sendungId ? { sendungId } : {}));
  },
  async tasteLernen() {
    return yokeLesen(await invoke("bild_taste_lernen", {}));
  },
  async aufnehmen() {
    await invoke("bild_aufnehmen", {});
  },
  async ordnerZeigen(a) {
    await invoke("bild_ordner_zeigen", a);
  },
  async exportieren(a) {
    return ((await invoke("bild_export", a)) as string | null) ?? null;
  },
  async kontext(a) {
    return kontextLesen(await invoke("bild_kontext", a));
  },
};

export { fundLesen };
