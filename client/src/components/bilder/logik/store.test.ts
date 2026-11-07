import { describe, expect, it, vi } from "vitest";
import type { BildAktionen } from "./api";
import { exifGpsLesen, exifGpsSegment, exifEinsetzen } from "./exifGps";
import { erzeugeStore, hinzufuegenMeldung } from "./store";
import { leereZaehlung } from "./uebernehmer";
import { eintrag } from "./testhilfen";
import type { BilderListe, SendeAntwort } from "./typen";

function aufbau(o: { aufnehmen?: BildAktionen["aufnehmen"]; senden?: BildAktionen["senden"]; status?: BildAktionen["status"]; eintraege?: ReturnType<typeof eintrag>[]; gespeichert?: Record<string, string>; funde?: BilderListe["funde"]; ohneEingang?: boolean; kontext?: BildAktionen["kontext"]; lesen?: BildAktionen["lesen"]; warten?: () => Promise<void> } = {}) {
  const eintraege = o.eintraege ?? [eintrag({ id: "a" }), eintrag({ id: "b" }), eintrag({ id: "c" })];
  const geschrieben: Array<{ id: string; jpegBase64?: string; sidecar: { sendungen?: Array<{ status: string; code?: string; sendung_id: string; text?: string }> } }> = [];
  const gespeichert: Record<string, string> = { ...(o.gespeichert ?? {}) };
  const aufrufe: unknown[] = [];
  const api = {
    liste: async (): Promise<BilderListe> => ({ eintraege, belegt_bytes: 0, quellen: [], tastenkuerzel: null, aufnahme_verfuegbar: false, simulator_taste: null, funde: o.funde ?? [], altbestand: [] }),
    schreiben: async (a: { id: string; sidecar: never }) => {
      geschrieben.push(a as never);
      const e = eintraege.find((x) => x.id === a.id)!;
      e.sidecar = a.sidecar;
    },
    senden: o.senden ?? (async (a) => { aufrufe.push(a); return { bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "gesendet" as const })), kontingent: { rest: 7, grenze: 12, retry_after: 0 } }; }),
    status: o.status ?? (async () => ({ bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } })),
    loeschen: async () => undefined,
    aufnehmen: o.aufnehmen ?? (async () => undefined),
    kontext: o.kontext ?? (async () => ({ staende: [], spur: null })),
    lesen: o.lesen ?? (async () => Uint8Array.from([0xff, 0xd8, 0xff, 0xda, 0, 2, 0xff, 0xd9])),
  } as unknown as BildAktionen;
  let n = 0;
  const store = erzeugeStore({
    api, kodieren: async () => { throw new Error("nicht gebraucht"); },
    zustandLesen: async () => gespeichert,
    zustandSchreiben: async (k, v) => { if (v === null) delete gespeichert[k]; else gespeichert[k] = v; },
    neueId: () => `S${++n}`, warten: o.warten ?? (async () => undefined), jetzt: () => 1000, ohneEingang: o.ohneEingang,
  });
  return { store, geschrieben, gespeichert, aufrufe, api };
}

const keinePhase = () => "";

describe("Store: Tablet", () => {
  it("laedt nur neu und laesst den Eingang dem Sim-PC", async () => {
    const t = aufbau({ ohneEingang: true, funde: [{ fund_id: "f1" }] as never });
    await t.store.eingang();
    expect(t.store.lesen().geladen).toBe(true);
    expect(t.store.lesen().uebernahme.fehler).toBe(0);
  });
});

describe("Store: Senden", () => {
  it("Erfolg: gesendet, Auswahl leer, Kontingent aus der Antwort, Vermerk im Sidecar", async () => {
    const t = aufbau();
    await t.store.laden();
    const rest = await t.store.senden(["a", "b"], "Hallo", "verdeckt", keinePhase);
    expect([...rest]).toEqual([]);
    const z = t.store.lesen();
    expect(z.sendestand.a.phase).toBe("gesendet");
    expect(z.kontingent).toEqual({ rest: 7, grenze: 12, retry_after: 0 });
    const letzte = t.geschrieben.filter((g) => g.id === "a").pop()!;
    expect(letzte.sidecar.sendungen).toEqual([expect.objectContaining({ sendung_id: "S1", status: "gesendet" })]);
    expect(t.aufrufe[0]).toMatchObject({ sendungId: "S1", text: "Hallo", geoStufe: "verdeckt", bilder: [{ gruppe: "g", bild_id: "a" }, { bild_id: "b" }] });
  });
  it("mehr als die Grenze aus dem Paket wird nicht gesendet", async () => {
    const t = aufbau({ eintraege: Array.from({ length: 6 }, (_, i) => eintrag({ id: `x${i}` })) });
    await t.store.laden();
    const ids = Array.from({ length: 6 }, (_, i) => `x${i}`);
    expect([...(await t.store.senden(ids, "", "aus", keinePhase))]).toEqual(ids);
    expect(t.aufrufe).toEqual([]);
  });
  it("Server nicht erreichbar: Bilder bleiben ausgewaehlt, nichts geht verloren", async () => {
    const t = aufbau({ senden: async () => { throw { code: "live_nicht_erreichbar", message: "x" }; } });
    await t.store.laden();
    const rest = await t.store.senden(["a"], "", "verdeckt", keinePhase);
    expect([...rest]).toEqual(["a"]);
    expect(t.store.lesen().sendestand.a).toMatchObject({ phase: "fehlgeschlagen", code: "server_nicht_erreichbar" });
  });
  it("Antwort bleibt aus: erst bild_status fragen; bleibt es leer, gilt Zustellung unklar und das Bild bleibt ausgewaehlt", async () => {
    const t = aufbau({ senden: async () => { throw { code: "live_antwort_fehlt", message: "x" }; } });
    await t.store.laden();
    const rest = await t.store.senden(["a"], "", "verdeckt", keinePhase);
    expect([...rest]).toEqual(["a"]);
    expect(t.store.lesen().sendestand.a.code).toBe("discord_unklar");
  });
  it("Antwort bleibt aus, Status kennt das Bild als angenommen: kein Fehler, kein zweiter Versand", async () => {
    const t = aufbau({
      senden: async () => { throw { code: "live_antwort_fehlt", message: "x" }; },
      status: async () => ({ bilder: [{ bild_id: "a", status: "gesendet" }], kontingent: null }),
    });
    await t.store.laden();
    expect([...(await t.store.senden(["a"], "", "verdeckt", keinePhase))]).toEqual([]);
    expect(t.store.lesen().sendestand.a.phase).toBe("gesendet");
  });
  it("Kontingent ausgeschoepft: Code und retry_after je Bild", async () => {
    const antwort: SendeAntwort = { bilder: [{ bild_id: "a", status: "abgelehnt", code: "kontingent", retry_after: 420 }], kontingent: { rest: 0, grenze: 12, retry_after: 420 } };
    const t = aufbau({ senden: async () => antwort });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt", keinePhase);
    expect(t.store.lesen().sendestand.a).toMatchObject({ code: "kontingent", retryAfter: 420 });
    expect(t.store.lesen().kontingent?.rest).toBe(0);
  });
  it("Wiederholung der fehlgeschlagenen Bilder behaelt die sendung_id (gleiche Bytes, kein konflikt)", async () => {
    let n = 0;
    const t = aufbau({ senden: async (a) => { if (++n === 1) throw { code: "live_nicht_erreichbar", message: "x" }; return { bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "gesendet" as const })), kontingent: null }; } });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt", keinePhase);
    await t.store.senden(["a"], "", "verdeckt", keinePhase);
    expect(t.store.lesen().sendestand.a).toMatchObject({ phase: "gesendet", sendungId: "S1" });
  });
  it("angenommen: wird nachgefragt, bis es gesendet ist", async () => {
    let n = 0;
    const t = aufbau({
      senden: async (a) => ({ bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "angenommen" as const })), kontingent: null }),
      status: async () => ({ bilder: [{ bild_id: "a", status: ++n < 2 ? "angenommen" : "gesendet" }], kontingent: null }),
    });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt", keinePhase);
    await new Promise((r) => setTimeout(r, 20));
    expect(t.store.lesen().sendestand.a.phase).toBe("gesendet");
  });
});

describe("Store: Stufe im Vermerk und Yoke-Taste", () => {
  it("die Status-Nachfrage aendert die Geo-Stufe der Sendung nicht (Vorgabe im Store ist eine andere)", async () => {
    let n = 0;
    const t = aufbau({
      senden: async (a) => ({ bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "angenommen" as const })), kontingent: null }),
      status: async () => ({ bilder: [{ bild_id: "a", status: ++n < 1 ? "angenommen" : "gesendet" }], kontingent: null }),
    });
    await t.store.laden();
    await t.store.senden(["a"], "", "sichtbar", keinePhase);
    await new Promise((r) => setTimeout(r, 20));
    await t.store.geoStufeSetzen("aus");
    const v = t.store.lesen().liste!.eintraege.find((e) => e.id === "a")!.sidecar!.sendungen!;
    expect(v).toHaveLength(1);
    expect(v[0]).toMatchObject({ status: "gesendet", geo_stufe: "sichtbar" });
  });
  it("Yoke-Taste: das Lernen endet vor der Bestaetigung, eine alte Meldung verschwindet beim neuen Versuch", async () => {
    const t = aufbau();
    let waehrendBestaetigung: boolean | null = null;
    let meldungDabei: unknown = "offen";
    let gelernt: unknown = null;
    t.api.tasteLernen = async () => gelernt as never;
    t.api.quellenSetzen = async () => { waehrendBestaetigung = t.store.lesen().yokeLernt; meldungDabei = t.store.lesen().meldung; return { quellen: [], abgebrochen: false }; };
    await t.store.laden();
    await t.store.yokeFestlegen();
    expect(t.store.lesen().meldung?.schluessel).toBe("bilder.meldung.yoke_abgelaufen");
    gelernt = { geraet_name: "Yoke", hersteller_id: 1, produkt_id: 2, knopf: 7 };
    await t.store.yokeFestlegen();
    expect(waehrendBestaetigung).toBe(false);
    expect(meldungDabei).toBeNull();
  });

  it("Yoke-Taste: lernen, speichern mit yokeTaste, Zeit abgelaufen meldet, entfernen mit null", async () => {
    const t = aufbau();
    const gesetzt: unknown[] = [];
    let gelernt: unknown = { geraet_name: "Yoke", hersteller_id: 1, produkt_id: 2, knopf: 7 };
    t.api.tasteLernen = async () => gelernt as never;
    t.api.quellenSetzen = async (a) => { gesetzt.push(a); return { quellen: [], abgebrochen: false }; };
    await t.store.laden();
    await t.store.yokeFestlegen();
    expect(gesetzt).toEqual([{ yokeTaste: { geraet_name: "Yoke", hersteller_id: 1, produkt_id: 2, knopf: 7 } }]);
    expect(t.store.lesen().yokeLernt).toBe(false);
    gelernt = null;
    await t.store.yokeFestlegen();
    expect(gesetzt).toHaveLength(1);
    expect(t.store.lesen().meldung?.schluessel).toBe("bilder.meldung.yoke_abgelaufen");
    await t.store.yokeEntfernen();
    expect(gesetzt.at(-1)).toEqual({ yokeTaste: null });
  });
});

describe("Store: Einwilligung, Fortsetzen, Sidecar", () => {
  it("Einwilligung und Geo-Stufe gelten je Pilot", async () => {
    const t = aufbau();
    await t.store.pilotSetzen("17");
    expect(t.store.lesen().eingeschaltet).toBe(false);
    expect(t.store.lesen().geoStufe).toBe("verdeckt");
    await t.store.einschalten(true);
    await t.store.geoStufeSetzen("aus");
    expect(Object.keys(t.gespeichert).sort()).toEqual(["aeroacars.bilder.einwilligung.17", "aeroacars.bilder.geo.17"]);
    await t.store.pilotSetzen("18");
    expect(t.store.lesen().eingeschaltet).toBe(false);
    expect(t.store.lesen().geoStufe).toBe("verdeckt");
    await t.store.pilotSetzen("17");
    expect(t.store.lesen().eingeschaltet).toBe(true);
    expect(t.store.lesen().geoStufe).toBe("aus");
  });
  it("Neustart mitten in der Sendung: angenommen im Sidecar wird beim Server erfragt", async () => {
    const e = eintrag({ id: "a", sidecar: { sendungen: [{ sendung_id: "ALT", zeit_ms: 1, geo_stufe: "verdeckt", status: "angenommen" }] } });
    const t = aufbau({ eintraege: [e], status: async (id) => ({ bilder: id === "ALT" ? [{ bild_id: "a", status: "gesendet" }] : [], kontingent: null }) });
    await t.store.laden();
    await t.store.fortsetzen();
    expect(t.store.lesen().sendestand.a.phase).toBe("gesendet");
    expect(t.geschrieben.pop()!.sidecar.sendungen![0].status).toBe("gesendet");
  });
  it("Absturz vor dem HTTP-Aufruf: nach dem Neustart gilt die Sendung als unklar, nicht als angenommen, und Wiederholen nutzt dieselbe sendung_id", async () => {
    const eintraege = [eintrag({ id: "a" })];
    const haengt = aufbau({ eintraege, senden: () => new Promise(() => undefined) });
    await haengt.store.laden();
    void haengt.store.senden(["a"], "Hallo", "verdeckt", keinePhase);
    await new Promise((r) => setTimeout(r, 0));
    expect(eintraege[0].sidecar!.sendungen).toEqual([expect.objectContaining({ sendung_id: "S1", status: "abgelehnt", code: "discord_unklar" })]);
    // Neustart: neuer Store auf demselben Sidecar
    let statusAufrufe = 0;
    const neu = aufbau({ eintraege, status: async () => { statusAufrufe++; return { bilder: [], kontingent: null }; } });
    await neu.store.laden();
    await neu.store.fortsetzen();
    expect(statusAufrufe).toBe(0);
    expect(neu.store.lesen().sendestand.a).toMatchObject({ phase: "fehlgeschlagen", sendungId: "S1", code: "discord_unklar" });
    await neu.store.senden(["a"], "Hallo", "verdeckt", keinePhase);
    expect(neu.aufrufe[0]).toMatchObject({ sendungId: "S1" });
  });
  it("Aufnahme: bild_io (Simulator antwortet nicht innerhalb von 5 s) hat einen eigenen Text, kein_flug und Sonstiges behalten ihren", async () => {
    const art = async (code: string) => {
      const t = aufbau({ aufnehmen: async () => { throw { code, message: "x" }; } });
      await t.store.aufnehmen();
      return t.store.lesen().meldung?.schluessel;
    };
    expect(await art("bild_io")).toBe("bilder.meldung.aufnahme_zeit");
    expect(await art("kein_flug")).toBe("bilder.meldung.kein_flug");
    expect(await art("bild_name")).toBe("bilder.meldung.aufnahme_fehler");
  });
  it("Text, Favorit und Flug aendern schreiben das ganze Sidecar", async () => {
    const t = aufbau();
    await t.store.laden();
    const e = t.store.lesen().liste!.eintraege[0];
    await t.store.textSetzen(e, "x".repeat(900));
    await t.store.favoritSetzen(t.store.lesen().liste!.eintraege[0], true);
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: null, rufzeichen: null, abflug: null, ziel: null });
    const s = t.store.lesen().liste!.eintraege[0].sidecar!;
    expect(s.text!.length).toBe(500);
    expect(s.favorit).toBe(true);
    expect(s.meta!.pirep_id).toBeNull();
    expect(s.flug_von_hand).toBe(true);
  });
});

describe("Store: Flug zuordnen", () => {
  // Kleinstes JPEG mit unserem EXIF-Segment: SOI, APP1, SOS, EOI
  const jpegMitOrt = () => exifEinsetzen(Uint8Array.from([0xff, 0xd8, 0xff, 0xda, 0, 2, 0xff, 0xd9]), exifGpsSegment({ breite: 50.03, laenge: 8.57, hoehe_ft: 12000, kurs: 270, zeit_utc: "2026-11-07T10:00:00.000Z" }));
  const basis = { breite: 41, laenge: 2, hoehe_ft: 35000, gs_kt: 450, kurs: 90, flugzeug_titel: "B738", flugzeug_icao: "B738", phase: "cruise", simulator: "xplane12", naechster_flugplatz: "LEBL", pirep_id: "P2", rufzeichen: "GSG202", abflug: "LEBL", ziel: "EDDM" };
  const ausB64 = (b: string) => Uint8Array.from(atob(b), (c) => c.charCodeAt(0));

  it("anderer Flug: alle Aufnahmedaten kommen neu aus dessen Protokoll zur Bildzeit, das JPEG bekommt die neue Position", async () => {
    let frage: unknown;
    const t = aufbau({ lesen: async () => jpegMitOrt(), kontext: async (a) => { frage = a; return { staende: [{ zeit: null, quelle: "protokoll", basis }], spur: null }; } });
    await t.store.laden();
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: "P2", rufzeichen: "GSG202", abflug: "LEBL", ziel: "EDDM" });
    expect(frage).toEqual({ zeiten: ["2026-11-07T10:00:00.000Z"], pirepId: "P2" });
    const s = t.store.lesen().liste!.eintraege[0].sidecar!;
    expect(s.meta).toMatchObject({ pirep_id: "P2", breite: 41, laenge: 2, hoehe_ft: 35000, flugzeug_icao: "B738", simulator: "xplane12", naechster_flugplatz: "LEBL", phase: "cruise", quelle: "protokoll", zeit_utc: "2026-11-07T10:00:00.000Z" });
    const jpeg = t.geschrieben.pop()!.jpegBase64!;
    expect(exifGpsLesen(ausB64(jpeg))).toMatchObject({ breite: 41, laenge: 2 });
  });

  it("Ohne Flug: nichts vom alten Flug bleibt stehen, auch nicht die Position im JPEG", async () => {
    const t = aufbau({ lesen: async () => jpegMitOrt() });
    await t.store.laden();
    expect(exifGpsLesen(jpegMitOrt())).not.toBeNull();
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: null, rufzeichen: null, abflug: null, ziel: null });
    const m = t.store.lesen().liste!.eintraege[0].sidecar!.meta!;
    expect(m).toMatchObject({ pirep_id: null, rufzeichen: null, abflug: null, ziel: null, breite: null, laenge: null, hoehe_ft: null, gs_kt: null, kurs: null, flugzeug_titel: null, flugzeug_icao: null, phase: null, simulator: null, naechster_flugplatz: null, quelle: "keine" });
    expect(m.zeit_utc).toBe("2026-11-07T10:00:00.000Z");
    expect(exifGpsLesen(ausB64(t.geschrieben.pop()!.jpegBase64!))).toBeNull();
  });

  it("Der Logbuch-Rueckfall nennt einen anderen Flug: dessen Daten gelten nicht, der gewaehlte Flug bleibt", async () => {
    const t = aufbau({ kontext: async () => ({ staende: [{ zeit: null, quelle: "logbuch", basis: { pirep_id: "FREMD", abflug: "EGLL" } }], spur: null }) });
    await t.store.laden();
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: "P2", rufzeichen: "77", abflug: "LEBL", ziel: null, flugzeug_icao: "A20N" });
    expect(t.store.lesen().liste!.eintraege[0].sidecar!.meta).toMatchObject({ pirep_id: "P2", rufzeichen: "77", abflug: "LEBL", ziel: null, breite: null, flugzeug_icao: "A20N", quelle: "logbuch" });
  });

  it("am Tablet schreibt der Tab keine Bytes (das Programm nimmt die alte Position aus dem JPEG)", async () => {
    const t = aufbau({ ohneEingang: true });
    await t.store.laden();
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: null, rufzeichen: null, abflug: null, ziel: null });
    expect(t.geschrieben.pop()!.jpegBase64).toBeUndefined();
  });

  it("gleiche Position: das JPEG wird nicht neu geschrieben", async () => {
    const t = aufbau({ kontext: async () => ({ staende: [{ zeit: null, quelle: "protokoll", basis: { pirep_id: "P2", breite: 50.03, laenge: 8.57 } }], spur: null }) });
    await t.store.laden();
    await t.store.flugSetzen(t.store.lesen().liste!.eintraege[0], { pirep_id: "P2", rufzeichen: null, abflug: null, ziel: null });
    expect(t.geschrieben.pop()!.jpegBase64).toBeUndefined();
  });

  it("Kandidaten kommen mit der Bildzeit, ohne Antwort bleibt die Liste leer statt zu scheitern", async () => {
    let frage: unknown;
    const t = aufbau({ kontext: async (a) => { frage = a; return { staende: [], spur: null, fluege: [{ pirep_id: "P9", rufzeichen: null, abflug: null, ziel: null, flugzeug_icao: null, start_ms: 1, ende_ms: 2 }] }; } });
    expect((await t.store.flugKandidaten("2026-11-07T10:00:00Z")).map((f) => f.pirep_id)).toEqual(["P9"]);
    expect(frage).toEqual({ zeiten: ["2026-11-07T10:00:00Z"], fluege: true });
    const aus = aufbau({ kontext: async () => { throw new Error("offline"); } });
    expect(await aus.store.flugKandidaten(null)).toEqual([]);
  });
});

describe("Store: Export", () => {
  it("das JPEG im ZIP traegt die Position aus dem Sidecar, auch wenn die Datei keine mehr hat", async () => {
    let zip = new Uint8Array();
    const t = aufbau();
    (t.api as unknown as { exportieren: BildAktionen["exportieren"] }).exportieren = async (a) => { zip = Uint8Array.from(atob(a.zipBase64), (c) => c.charCodeAt(0)); return null; };
    await t.store.laden();
    await t.store.exportieren(["a"]);
    const text = new TextDecoder("latin1").decode(zip);
    expect(text).toContain("Exif");
  });
});

describe("Store: Sendung bis zum Ende verfolgen", () => {
  const bild = (status: "angenommen" | "gesendet" | "abgelehnt", id = "a") => ({ bild_id: id, status, code: status === "abgelehnt" ? "zu_gross" : undefined });
  it("spaet abgelehnt: das Bild kommt in die Auswahl zurueck, gesendete nicht", async () => {
    const t = aufbau({
      senden: async () => ({ bilder: [bild("angenommen", "a"), bild("angenommen", "b")], kontingent: null }),
      status: async () => ({ bilder: [bild("abgelehnt", "a"), bild("gesendet", "b")], kontingent: null }),
    });
    await t.store.laden();
    const rest = await t.store.senden(["a", "b"], "", "aus", keinePhase);
    expect([...rest]).toEqual([]);
    await vi.waitFor(() => expect(t.store.lesen().wiederAuswaehlen).toEqual(["a"]));
    t.store.wiederAuswaehlenLoeschen();
    expect(t.store.lesen().wiederAuswaehlen).toEqual([]);
  });
  it("ein Aussetzer beim Nachfragen beendet die Verfolgung nicht", async () => {
    let n = 0;
    const t = aufbau({
      senden: async () => ({ bilder: [bild("angenommen")], kontingent: null }),
      status: async () => { if (++n === 1) throw { code: "live_nicht_erreichbar", message: "x" }; return { bilder: [bild("gesendet")], kontingent: null }; },
    });
    await t.store.laden();
    await t.store.senden(["a"], "", "aus", keinePhase);
    await vi.waitFor(() => expect(t.store.lesen().sendestand.a.phase).toBe("gesendet"));
    expect(t.store.lesen().serverErreichbar).toBe(true);
  });
  it("Fortsetzen nach Neustart: spaet abgelehnt kommt ebenfalls zurueck", async () => {
    const t = aufbau({
      eintraege: [eintrag({ id: "a", sidecar: { sendungen: [{ sendung_id: "S9", zeit_ms: 1, geo_stufe: "aus", status: "angenommen" }] } })],
      status: async () => ({ bilder: [bild("abgelehnt")], kontingent: null }),
    });
    await t.store.laden();
    await t.store.fortsetzen();
    expect(t.store.lesen().wiederAuswaehlen).toEqual(["a"]);
  });
  it("der Text der Sendung steht im Vermerk, fuer die Wiederholung", async () => {
    const t = aufbau({ senden: async () => { throw { code: "live_nicht_erreichbar", message: "x" }; } });
    await t.store.laden();
    await t.store.senden(["a"], "Mein Text", "aus", keinePhase);
    expect(t.geschrieben.pop()!.sidecar.sendungen![0].text).toBe("Mein Text");
  });
});

describe("Store: Erreichbarkeit", () => {
  it("ist schon vor dem ersten Senden bekannt und kommt mit dem Server zurueck", async () => {
    let aus = true;
    const t = aufbau({ status: async () => { if (aus) throw { code: "live_nicht_erreichbar", message: "x" }; return { bilder: [], kontingent: { rest: 3, grenze: 12, retry_after: 0 } }; } });
    expect(t.store.lesen().serverErreichbar).toBeNull();
    await t.store.kontingentHolen();
    expect(t.store.lesen().serverErreichbar).toBe(false);
    aus = false;
    await t.store.kontingentHolen();
    expect(t.store.lesen().serverErreichbar).toBe(true);
  });
  it("„nicht angemeldet“ heisst nicht „Server aus“ (Gegenprobe)", async () => {
    const t = aufbau({ status: async () => { throw { code: "live_nicht_angemeldet", message: "x" }; } });
    await t.store.kontingentHolen();
    expect(t.store.lesen().serverErreichbar).toBeNull();
  });
});

describe("Hinzufuegen: Meldung", () => {
  const z = (w: Partial<ReturnType<typeof leereZaehlung>>) => ({ ...leereZaehlung(), ...w });
  it("nennt Erfolg, Teilfehler mit Anzahl und Grund, Totalausfall und Duplikate", () => {
    expect(hinzufuegenMeldung(z({ neu: 1 }))).toMatchObject({ art: "ok", schluessel: "bilder.meldung.hinzugefuegt_eins" });
    expect(hinzufuegenMeldung(z({ neu: 3 }))).toMatchObject({ art: "ok", schluessel: "bilder.meldung.hinzugefuegt", werte: { n: 3 } });
    expect(hinzufuegenMeldung(z({ neu: 2, fehler: 1, letzterFehler: "bild_format: kaputt.png" }))).toEqual({ art: "ok", schluessel: "bilder.meldung.hinzugefuegt_teil", werte: { n: 2, gesamt: 3, fehler: 1, grund: "kaputt.png" } });
    expect(hinzufuegenMeldung(z({ fehler: 2, letzterFehler: "x: y", letzterCode: "bild_unlesbar" }))).toMatchObject({ art: "fehler", schluessel: "bilder.meldung.hinzufuegen_keins", werte: { gesamt: 2, fehler: 2, grundText: "bilder.meldung.grund_unlesbar" } });
    expect(hinzufuegenMeldung(z({ neu: 1, duplikate: 2 }))).toMatchObject({ schluessel: "bilder.meldung.hinzugefuegt_mit_duplikaten", werte: { n: 1, dup: 2 } });
    expect(hinzufuegenMeldung(z({ duplikate: 2 }))).toMatchObject({ schluessel: "bilder.meldung.hinzugefuegt_nur_duplikate" });
  });
});
