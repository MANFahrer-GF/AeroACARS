import { describe, expect, it } from "vitest";
import type { BildAktionen } from "./api";
import { erzeugeStore } from "./store";
import { eintrag } from "./testhilfen";
import type { BilderListe, SendeAntwort } from "./typen";

function aufbau(o: { senden?: BildAktionen["senden"]; status?: BildAktionen["status"]; eintraege?: ReturnType<typeof eintrag>[]; gespeichert?: Record<string, string> } = {}) {
  const eintraege = o.eintraege ?? [eintrag({ id: "a" }), eintrag({ id: "b" }), eintrag({ id: "c" })];
  const geschrieben: Array<{ id: string; sidecar: { sendungen?: Array<{ status: string; code?: string; sendung_id: string }> } }> = [];
  const gespeichert: Record<string, string> = { ...(o.gespeichert ?? {}) };
  const aufrufe: unknown[] = [];
  const api = {
    liste: async (): Promise<BilderListe> => ({ eintraege, belegt_bytes: 0, quellen: [], tastenkuerzel: null, aufnahme_verfuegbar: false, funde: [], altbestand: [] }),
    schreiben: async (a: { id: string; sidecar: never }) => {
      geschrieben.push(a as never);
      const e = eintraege.find((x) => x.id === a.id)!;
      e.sidecar = a.sidecar;
    },
    senden: o.senden ?? (async (a) => { aufrufe.push(a); return { bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "gesendet" as const })), kontingent: { rest: 7, grenze: 12, retry_after: 0 } }; }),
    status: o.status ?? (async () => ({ bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } })),
    loeschen: async () => undefined,
  } as unknown as BildAktionen;
  let n = 0;
  const store = erzeugeStore({
    api, kodieren: async () => { throw new Error("nicht gebraucht"); },
    zustandLesen: async () => gespeichert,
    zustandSchreiben: async (k, v) => { if (v === null) delete gespeichert[k]; else gespeichert[k] = v; },
    neueId: () => `S${++n}`, warten: async () => undefined, jetzt: () => 1000,
  });
  return { store, geschrieben, gespeichert, aufrufe };
}

describe("Store: Senden", () => {
  it("Erfolg: gesendet, Auswahl leer, Kontingent aus der Antwort, Vermerk im Sidecar", async () => {
    const t = aufbau();
    await t.store.laden();
    const rest = await t.store.senden(["a", "b"], "Hallo", "verdeckt");
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
    expect([...(await t.store.senden(ids, "", "aus"))]).toEqual(ids);
    expect(t.aufrufe).toEqual([]);
  });
  it("Server nicht erreichbar: Bilder bleiben ausgewaehlt, nichts geht verloren", async () => {
    const t = aufbau({ senden: async () => { throw { code: "live_nicht_erreichbar", message: "x" }; } });
    await t.store.laden();
    const rest = await t.store.senden(["a"], "", "verdeckt");
    expect([...rest]).toEqual(["a"]);
    expect(t.store.lesen().sendestand.a).toMatchObject({ phase: "fehlgeschlagen", code: "server_nicht_erreichbar" });
  });
  it("Antwort bleibt aus: erst bild_status fragen; bleibt es leer, gilt Zustellung unklar und das Bild bleibt ausgewaehlt", async () => {
    const t = aufbau({ senden: async () => { throw { code: "live_antwort_fehlt", message: "x" }; } });
    await t.store.laden();
    const rest = await t.store.senden(["a"], "", "verdeckt");
    expect([...rest]).toEqual(["a"]);
    expect(t.store.lesen().sendestand.a.code).toBe("discord_unklar");
  });
  it("Antwort bleibt aus, Status kennt das Bild als angenommen: kein Fehler, kein zweiter Versand", async () => {
    const t = aufbau({
      senden: async () => { throw { code: "live_antwort_fehlt", message: "x" }; },
      status: async () => ({ bilder: [{ bild_id: "a", status: "gesendet" }], kontingent: null }),
    });
    await t.store.laden();
    expect([...(await t.store.senden(["a"], "", "verdeckt"))]).toEqual([]);
    expect(t.store.lesen().sendestand.a.phase).toBe("gesendet");
  });
  it("Kontingent ausgeschoepft: Code und retry_after je Bild", async () => {
    const antwort: SendeAntwort = { bilder: [{ bild_id: "a", status: "abgelehnt", code: "kontingent", retry_after: 420 }], kontingent: { rest: 0, grenze: 12, retry_after: 420 } };
    const t = aufbau({ senden: async () => antwort });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt");
    expect(t.store.lesen().sendestand.a).toMatchObject({ code: "kontingent", retryAfter: 420 });
    expect(t.store.lesen().kontingent?.rest).toBe(0);
  });
  it("Wiederholung der fehlgeschlagenen Bilder behaelt die sendung_id (gleiche Bytes, kein konflikt)", async () => {
    let n = 0;
    const t = aufbau({ senden: async (a) => { if (++n === 1) throw { code: "live_nicht_erreichbar", message: "x" }; return { bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "gesendet" as const })), kontingent: null }; } });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt");
    await t.store.senden(["a"], "", "verdeckt");
    expect(t.store.lesen().sendestand.a).toMatchObject({ phase: "gesendet", sendungId: "S1" });
  });
  it("angenommen: wird nachgefragt, bis es gesendet ist", async () => {
    let n = 0;
    const t = aufbau({
      senden: async (a) => ({ bilder: a.bilder.map((b) => ({ bild_id: b.bild_id, status: "angenommen" as const })), kontingent: null }),
      status: async () => ({ bilder: [{ bild_id: "a", status: ++n < 2 ? "angenommen" : "gesendet" }], kontingent: null }),
    });
    await t.store.laden();
    await t.store.senden(["a"], "", "verdeckt");
    await new Promise((r) => setTimeout(r, 20));
    expect(t.store.lesen().sendestand.a.phase).toBe("gesendet");
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
    expect(s.meta!.breite).toBe(50.03); // Geo-Daten bleiben, nur die Zuordnung aendert sich
  });
});
