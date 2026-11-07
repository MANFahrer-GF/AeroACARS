// Zustand des Bilder-Tabs ausserhalb von React: der Eingang laeuft auch, wenn der
// Tab nicht offen ist (`BilderHintergrund` in App.tsx). Die Oberflaeche liest ueber
// `useBilder()`. Alle Schreibzugriffe aufs Sidecar laufen nacheinander.
import type { BildAktionen } from "./api";
import { alsBase64 } from "./bildverarbeitung";
import { GRENZEN, TABELLE } from "./grenzen";
import { zipBauen } from "./zip";
import { fehlerCode, fehlerText } from "./normalisieren";
import { sendeMeta } from "./nachrichtVorschau";
import {
  antwortEinarbeiten, aufrufFehler, auswahlNachSenden, offeneIds, sendeStart, sendungIdWaehlen, standNachLaden, type Sendestand,
} from "./sendezustand";
import { erzeugeUebernehmer, leereZaehlung, type Kodierer, type Zaehlung } from "./uebernehmer";
import type { BilderListe, Eintrag, Fundort, GeoStufe, Kontingent, Quelle, SendeAntwort, SendeVermerk, Sidecar } from "./typen";

export interface FlugAuswahl {
  pirep_id: string | null;
  rufzeichen: string | null;
  abflug: string | null;
  ziel: string | null;
}

export interface BilderZustand {
  geladen: boolean;
  liste: BilderListe | null;
  fehler: string | null;
  sendestand: Sendestand;
  kontingent: Kontingent | null;
  uebernahme: Zaehlung;
  arbeitet: boolean;
  /** Datenschutz-Einwilligung dieses Piloten; null = noch nicht gelesen. */
  eingeschaltet: boolean | null;
  geoStufe: GeoStufe;
  vorschlaege: Fundort[] | null;
  /** Altbestand wurde angefordert (Liste in `liste.altbestand`). */
  altbestandGeladen: boolean;
  altbestandErgebnis: Zaehlung | null;
  /** Die Yoke-Taste wird gerade gelernt (Hinweis „Jetzt den Knopf druecken“). */
  yokeLernt: boolean;
  meldung: { art: "ok" | "fehler"; schluessel: string; werte?: Record<string, string | number> } | null;
}

export interface StoreAbhaengig {
  api: BildAktionen;
  kodieren: Kodierer;
  zustandLesen: () => Promise<Record<string, string>>;
  zustandSchreiben: (schluessel: string, wert: string | null) => Promise<void>;
  neueId: () => string;
  warten: (ms: number) => Promise<void>;
  jetzt: () => number;
  /** Am Tablet nur neu laden: Den Eingang arbeitet der Sim-PC ab. */
  ohneEingang?: boolean;
}

const ANFANG: BilderZustand = {
  geladen: false, liste: null, fehler: null, sendestand: {}, kontingent: null, uebernahme: leereZaehlung(), arbeitet: false,
  eingeschaltet: null, geoStufe: TABELLE.geoVorgabe, vorschlaege: null, altbestandGeladen: false, altbestandErgebnis: null, yokeLernt: false, meldung: null,
};

export function erzeugeStore(dep: StoreAbhaengig) {
  const { api } = dep;
  let z: BilderZustand = ANFANG;
  const hoerer = new Set<() => void>();
  let pilot: string | null = null;
  let schreibKette: Promise<unknown> = Promise.resolve();
  const uebernehmer = erzeugeUebernehmer({ api, kodieren: dep.kodieren, jetzt: dep.jetzt });

  const setzen = (teil: Partial<BilderZustand>) => {
    z = { ...z, ...teil };
    hoerer.forEach((h) => h());
  };
  const reihe = <T>(f: () => Promise<T>): Promise<T> => {
    const lauf = schreibKette.then(f, f);
    schreibKette = lauf.catch(() => undefined);
    return lauf;
  };
  const meldung = (art: "ok" | "fehler", schluessel: string, werte?: Record<string, string | number>) => setzen({ meldung: { art, schluessel, werte } });
  const schluesselE = () => `aeroacars.bilder.einwilligung.${pilot}`;
  const schluesselG = () => `aeroacars.bilder.geo.${pilot}`;

  async function laden(mitAlt = z.altbestandGeladen): Promise<void> {
    try {
      const liste = await api.liste(mitAlt);
      setzen({ liste, geladen: true, fehler: null, sendestand: standNachLaden(z.sendestand, liste.eintraege) });
    } catch (e) {
      setzen({ fehler: fehlerText(e), geladen: true });
    }
  }

  async function pilotSetzen(id: string | null): Promise<void> {
    pilot = id;
    if (!id) return setzen({ eingeschaltet: null, geoStufe: TABELLE.geoVorgabe });
    try {
      const alle = await dep.zustandLesen();
      const g = alle[schluesselG()];
      setzen({
        eingeschaltet: alle[schluesselE()] === "1",
        geoStufe: (TABELLE.geoStufen as string[]).includes(g) ? (g as GeoStufe) : TABELLE.geoVorgabe,
      });
    } catch {
      setzen({ eingeschaltet: false });
    }
  }

  async function einschalten(an: boolean): Promise<void> {
    if (!pilot) return;
    await dep.zustandSchreiben(schluesselE(), an ? "1" : null);
    setzen({ eingeschaltet: an });
  }

  async function geoStufeSetzen(s: GeoStufe): Promise<void> {
    setzen({ geoStufe: s });
    if (pilot) await dep.zustandSchreiben(schluesselG(), s);
  }

  let eingangLaeufe = 0;
  async function eingang(): Promise<void> {
    if (dep.ohneEingang) return laden();
    eingangLaeufe++;
    setzen({ arbeitet: true });
    try {
      const r = await uebernehmer.eingangAbarbeiten();
      const u = z.uebernahme;
      setzen({
        uebernahme: {
          neu: u.neu + r.neu, verworfen: u.verworfen + r.verworfen, duplikate: u.duplikate + r.duplikate, fehler: u.fehler + r.fehler,
          mit_daten: u.mit_daten + r.mit_daten, nur_flug: u.nur_flug + r.nur_flug, ohne_daten: u.ohne_daten + r.ohne_daten, veraltet: u.veraltet + r.veraltet,
          letzterFehler: r.letzterFehler ?? u.letzterFehler,
        },
      });
    } finally {
      eingangLaeufe--;
      if (eingangLaeufe === 0) setzen({ arbeitet: false });
    }
    await laden();
  }

  /** Sidecar eines Bildes aendern (voller Stand, `bild_schreiben` ersetzt das Dokument). */
  function sidecarAendern(e: Eintrag, f: (s: Sidecar) => Sidecar): Promise<void> {
    return reihe(async () => {
      // Tablet und Sim-PC schreiben dasselbe Dokument: vom frischen Stand ausgehen, nicht vom Stand der Anzeige.
      await laden();
      const alt = (z.liste?.eintraege.find((x) => x.id === e.id)?.sidecar ?? e.sidecar) as Sidecar;
      await api.schreiben({ gruppe: e.gruppe, id: e.id, sidecar: f(alt) });
      await laden();
    });
  }

  const textSetzen = (e: Eintrag, text: string) => sidecarAendern(e, (s) => ({ ...s, text: text.slice(0, GRENZEN.text_max_zeichen) }));
  const favoritSetzen = (e: Eintrag, favorit: boolean) => sidecarAendern(e, (s) => ({ ...s, favorit }));
  const flugSetzen = (e: Eintrag, f: FlugAuswahl) =>
    sidecarAendern(e, (s) => ({ ...s, flug_von_hand: true, meta: { ...s.meta, pirep_id: f.pirep_id, rufzeichen: f.rufzeichen, abflug: f.abflug, ziel: f.ziel } }));

  async function loeschen(e: Eintrag, bezeichnung: string): Promise<boolean> {
    try {
      await api.loeschen({ gruppe: e.gruppe, id: e.id, bezeichnung });
      await laden();
      return true;
    } catch (err) {
      if (fehlerCode(err) !== "abgelehnt" && fehlerCode(err) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.loeschen_fehler", { fehler: fehlerText(err) });
      return false;
    }
  }

  function vermerk(e: Eintrag, v: SendeVermerk): Sidecar["sendungen"] {
    const alt = (e.sidecar?.sendungen ?? []).filter((x) => x.sendung_id !== v.sendung_id);
    return [...alt, v];
  }

  /** `stufe` nur beim Senden selbst; Nachfragen und Fortsetzen behalten die Stufe des vorhandenen Vermerks. */
  async function vermerkeSchreiben(ids: string[], sendungId: string, stufe: GeoStufe | null, stand: Sendestand): Promise<void> {
    for (const id of ids) {
      const e = z.liste?.eintraege.find((x) => x.id === id);
      const s = stand[id];
      if (!e || !s) continue;
      // Vor dem Aufruf („sendet“) ist nichts bestaetigt: Der Vermerk steht auf „unklar“, nie auf „angenommen“.
      // Bleibt er nach einem Absturz so stehen, zeigt der Tab „Zustellung unklar“ und „Erneut senden“
      // wiederholt mit derselben `sendung_id` (der Server ist darueber idempotent).
      const vorAufruf = s.phase === "sendet";
      const status: SendeVermerk["status"] = s.phase === "gesendet" ? "gesendet" : s.phase === "fehlgeschlagen" || vorAufruf ? "abgelehnt" : "angenommen";
      const code = vorAufruf ? "discord_unklar" : s.code;
      await sidecarAendern(e, (sc) => ({ ...sc, sendungen: vermerk(e, { sendung_id: sendungId, zeit_ms: dep.jetzt(), geo_stufe: stufe ?? sc.sendungen.find((x) => x.sendung_id === sendungId)?.geo_stufe ?? z.geoStufe, status, code }) }));
    }
  }

  /** Senden (hoechstens `max_je_sendung`). Rueckgabe: neue Auswahl (Fehlgeschlagene bleiben). */
  async function senden(ids: string[], text: string, stufe: GeoStufe, phaseText: (e: Eintrag) => string): Promise<Set<string>> {
    const liste = z.liste?.eintraege ?? [];
    const gewaehlt = ids.map((id) => liste.find((e) => e.id === id)).filter((e): e is Eintrag => !!e && !!e.sidecar?.meta);
    if (gewaehlt.length === 0 || gewaehlt.length > GRENZEN.max_je_sendung) return new Set(ids);
    const idListe = gewaehlt.map((e) => e.id);
    const sendungId = sendungIdWaehlen(z.sendestand, idListe, dep.neueId);
    setzen({ sendestand: sendeStart(z.sendestand, idListe, sendungId) });
    // Sendung zuerst festhalten, damit ein Neustart mitten im Aufruf fortsetzen kann
    await vermerkeSchreiben(idListe, sendungId, stufe, z.sendestand);
    let stand = z.sendestand;
    const anfrage = { sendungId, text, geoStufe: stufe, bilder: gewaehlt.map((e) => ({ gruppe: e.gruppe, bild_id: e.id, meta: sendeMeta(e.sidecar!.meta!, stufe, phaseText(e)) })) };
    try {
      const a = await api.senden(anfrage);
      stand = antwortEinarbeiten(stand, idListe, sendungId, a);
      setzen({ sendestand: stand, kontingent: a.kontingent ?? z.kontingent });
    } catch (err) {
      const code = fehlerCode(err);
      let a: SendeAntwort | null = null;
      if (code === "live_antwort_fehlt") {
        // Verbindung stand, Antwort blieb aus: erst nachfragen, nicht blind wiederholen
        try { a = await api.status(sendungId); } catch { a = null; }
      }
      if (a && a.bilder.length > 0) {
        stand = antwortEinarbeiten(stand, idListe, sendungId, a);
        setzen({ sendestand: stand, kontingent: a.kontingent ?? z.kontingent });
      } else {
        const details = (err as { details?: { retry_after?: unknown } })?.details;
        stand = aufrufFehler(stand, idListe, sendungId, code, details?.retry_after);
        setzen({ sendestand: stand });
      }
    }
    await vermerkeSchreiben(idListe, sendungId, stufe, stand);
    void nachfragen(sendungId);
    return auswahlNachSenden(new Set(ids), stand);
  }

  /** Fragt den Stand angenommener Bilder ab, bis alle gesendet oder abgelehnt sind. */
  async function nachfragen(sendungId: string): Promise<void> {
    for (let i = 0; i < GRENZEN.sende_abfrage_max; i++) {
      const offen = offeneIds(z.sendestand).filter((o) => o.sendungId === sendungId);
      if (offen.length === 0) return;
      await dep.warten(GRENZEN.sende_abfrage_s * 1000);
      try {
        const a = await api.status(sendungId);
        const ids = offen.map((o) => o.id);
        const stand = antwortEinarbeiten(z.sendestand, ids, sendungId, a);
        setzen({ sendestand: stand, kontingent: a.kontingent ?? z.kontingent });
        await vermerkeSchreiben(ids, sendungId, null, stand);
      } catch {
        return;
      }
    }
  }

  /** Nach einem Neustart: Sendungen, die im Sidecar noch „unterwegs“ stehen, beim Server erfragen. */
  async function fortsetzen(): Promise<void> {
    const offen = new Map<string, string[]>();
    for (const e of z.liste?.eintraege ?? []) {
      const v = [...(e.sidecar?.sendungen ?? [])].reverse().find((x) => x.status === "angenommen");
      if (v && !z.sendestand[e.id]) offen.set(v.sendung_id, [...(offen.get(v.sendung_id) ?? []), e.id]);
    }
    for (const [sendungId, ids] of offen) {
      try {
        const a = await api.status(sendungId);
        const stand = antwortEinarbeiten(z.sendestand, ids, sendungId, a);
        setzen({ sendestand: stand, kontingent: a.kontingent ?? z.kontingent });
        await vermerkeSchreiben(ids, sendungId, null, stand);
        void nachfragen(sendungId);
      } catch { /* kein Netz: bleibt „unterwegs“, naechster Start fragt wieder */ }
    }
  }

  async function kontingentHolen(): Promise<void> {
    try {
      const a = await api.status();
      if (a.kontingent) setzen({ kontingent: a.kontingent });
    } catch { /* ohne Verbindung bleibt das Kontingent unbekannt */ }
  }

  // ---- Bildquellen ----
  async function quellenFinden(): Promise<void> {
    try { setzen({ vorschlaege: await api.quellenFinden() }); } catch { setzen({ vorschlaege: [] }); }
  }
  async function quellenSetzen(quellen: Quelle[]): Promise<void> {
    try {
      await api.quellenSetzen({ quellen });
      await laden();
    } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.quellen_fehler", { fehler: fehlerText(e) });
    }
  }
  const aktuelleQuellen = (): Quelle[] => (z.liste?.quellen ?? []).map((q) => ({ pfad: q.pfad, aktiv: q.aktiv, original_entfernen: q.original_entfernen, art: q.art }));
  const quelleUmschalten = (pfad: string, aendern: Partial<Quelle>) =>
    quellenSetzen(aktuelleQuellen().map((q) => (q.pfad === pfad ? { ...q, ...aendern } : q)));
  const quelleEntfernen = (pfad: string) => quellenSetzen(aktuelleQuellen().filter((q) => q.pfad !== pfad));
  const quelleVorschlagen = (f: Fundort) =>
    quellenSetzen([...aktuelleQuellen().filter((q) => q.pfad !== f.pfad), { pfad: f.pfad, aktiv: true, original_entfernen: false, art: "aufgespuert" }]);
  async function ordnerWaehlen(): Promise<void> {
    try {
      const r = await api.quellenSetzen({ ordnerWaehlen: true });
      if (!r.abgebrochen) await laden();
    } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.quellen_fehler", { fehler: fehlerText(e) });
    }
  }
  async function yokeFestlegen(): Promise<void> {
    if (z.yokeLernt) return;
    setzen({ yokeLernt: true, meldung: null }); // eine alte Meldung gilt fuer den neuen Versuch nicht mehr
    try {
      let t: Awaited<ReturnType<typeof api.tasteLernen>>;
      try {
        t = await api.tasteLernen();
      } finally {
        setzen({ yokeLernt: false }); // Das Lernen endet hier, nicht erst mit der Bestaetigung
      }
      if (t === null) { meldung("fehler", "bilder.meldung.yoke_abgelaufen"); return; }
      await api.quellenSetzen({ yokeTaste: t });
      await laden();
    } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.yoke_fehler", { fehler: fehlerText(e) });
    }
  }
  async function yokeEntfernen(): Promise<void> {
    try { await api.quellenSetzen({ yokeTaste: null }); await laden(); } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.yoke_fehler", { fehler: fehlerText(e) });
    }
  }
  async function tastenkuerzelSetzen(text: string): Promise<void> {
    try { await api.quellenSetzen({ tastenkuerzel: text }); await laden(); } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.kuerzel_fehler", { fehler: fehlerText(e) });
    }
  }

  async function hinzufuegen(pfade?: string[]): Promise<void> {
    setzen({ arbeitet: true });
    try {
      const r = await uebernehmer.hinzufuegen(pfade);
      if (!r.abgebrochen) {
        setzen({ altbestandErgebnis: r.zaehlung });
        meldung(r.zaehlung.fehler > 0 && r.zaehlung.neu === 0 ? "fehler" : "ok", "bilder.meldung.hinzugefuegt", { n: r.zaehlung.neu });
      }
    } catch (e) {
      if (fehlerCode(e) !== "huelle_abgelehnt") meldung("fehler", "bilder.meldung.hinzufuegen_fehler", { fehler: fehlerText(e) });
    } finally {
      setzen({ arbeitet: false });
    }
    await laden();
  }

  async function altbestandAnzeigen(): Promise<void> {
    setzen({ altbestandGeladen: true });
    await laden(true);
  }
  async function altbestandUebernehmen(): Promise<void> {
    const pfade = (z.liste?.altbestand ?? []).map((a) => a.pfad);
    if (pfade.length === 0) return;
    await hinzufuegen(pfade);
    setzen({ altbestandGeladen: false });
    await laden(false);
  }

  /** `bild_io`: Der Simulator hat nicht innerhalb der Zeitgrenze (8 s) geantwortet oder die vorige Aufnahme haengt noch. */
  function aufnahmeGescheitert(code: string, text: string): void {
    const art = code === "kein_flug" ? "kein_flug" : code === "bild_io" ? "aufnahme_zeit" : "aufnahme_fehler";
    meldung("fehler", `bilder.meldung.${art}`, { fehler: text });
  }

  async function aufnehmen(): Promise<void> {
    setzen({ meldung: null });
    try {
      await api.aufnehmen();
      await eingang();
    } catch (e) {
      aufnahmeGescheitert(fehlerCode(e), fehlerText(e));
    }
  }

  async function ordnerZeigen(e: Eintrag): Promise<void> {
    try {
      await api.ordnerZeigen({ gruppe: e.gruppe, id: e.id });
    } catch (err) {
      meldung("fehler", "bilder.meldung.ordner_fehler", { fehler: fehlerText(err) });
    }
  }

  /** Auswahl als ZIP (JPEG und Sidecar je Bild) an einen vom Piloten gewaehlten Ort speichern. */
  async function exportieren(ids: string[]): Promise<void> {
    try {
      const dateien = [];
      for (const id of ids) {
        const e = z.liste?.eintraege.find((x) => x.id === id);
        if (!e?.pfad) continue;
        const zeit = e.sidecar?.meta?.zeit_utc?.replace(/[:.]/g, "-") ?? e.id;
        dateien.push({ name: `${zeit}-${e.id.slice(0, 8)}.jpg`, daten: await api.lesen(e.pfad), zeit_ms: e.dateizeit_ms ?? undefined });
        dateien.push({ name: `${zeit}-${e.id.slice(0, 8)}.json`, daten: new TextEncoder().encode(JSON.stringify(e.sidecar, null, 2)) });
      }
      if (dateien.length === 0) return;
      await api.exportieren({ zipBase64: alsBase64(zipBauen(dateien)), dateiname: "AeroACARS-Bilder.zip" });
    } catch (err) {
      meldung("fehler", "bilder.meldung.export_fehler", { fehler: fehlerText(err) });
    }
  }

  return {
    lesen: () => z,
    abonnieren: (h: () => void) => { hoerer.add(h); return () => { hoerer.delete(h); }; },
    laden, pilotSetzen, einschalten, geoStufeSetzen, eingang, textSetzen, favoritSetzen, flugSetzen, loeschen, senden, fortsetzen, kontingentHolen,
    yokeFestlegen, yokeEntfernen, quellenFinden, quelleUmschalten, quelleEntfernen, quelleVorschlagen, ordnerWaehlen, tastenkuerzelSetzen, hinzufuegen,
    altbestandAnzeigen, altbestandUebernehmen, aufnehmen, aufnahmeGescheitert, ordnerZeigen, exportieren,
    meldungLoeschen: () => setzen({ meldung: null }),
  };
}

export type BilderStore = ReturnType<typeof erzeugeStore>;
