import { describe, expect, it } from "vitest";
import type { BildAktionen, Uebernahme } from "./api";
import { erzeugeUebernehmer } from "./uebernehmer";
import { sha256Hex } from "./bildverarbeitung";
import { exifGpsLesen, app1Anzahl } from "./exifGps";
import { eintrag, fund } from "./testhilfen";
import type { BilderListe, Eintrag } from "./typen";

const JPEG = Uint8Array.from([0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 9, 0, 9, 1, 1, 0x11, 0, 0xff, 0xda, 0, 4, 0, 0, 1, 0xff, 0xd9]);

function aufbau(o: { funde?: ReturnType<typeof fund>[]; eintraege?: Eintrag[]; schreibenFehler?: (n: number) => unknown; kontext?: BildAktionen["kontext"] } = {}) {
  const log: string[] = [];
  const geschrieben: Array<Parameters<BildAktionen["schreiben"]>[0]> = [];
  let imGang = 0, maxGleichzeitig = 0, schreibVersuche = 0;
  const liste: BilderListe = { eintraege: o.eintraege ?? [], belegt_bytes: 0, quellen: [], tastenkuerzel: null, aufnahme_verfuegbar: false, funde: o.funde ?? [], altbestand: [] };
  const api = {
    liste: async () => liste,
    lesen: async () => {
      imGang++; maxGleichzeitig = Math.max(maxGleichzeitig, imGang);
      await new Promise((r) => setTimeout(r, 2));
      imGang--;
      return new Uint8Array([1, 2, 3]);
    },
    uebernehmen: async (a: { fundId: string; modus: string; gruppe: string }) => {
      log.push(`${a.modus}:${a.fundId}:${a.gruppe}`);
      return a.modus === "verwerfen" ? null : ({ id: `id-${a.fundId}`, gruppe: a.gruppe, quelle_pfad: `${a.gruppe}/id-${a.fundId}.quelle.png`, format: "png", dateizeit_ms: Date.UTC(2026, 10, 7, 10) } as Uebernahme);
    },
    schreiben: async (a: Parameters<BildAktionen["schreiben"]>[0]) => {
      const e = o.schreibenFehler?.(++schreibVersuche);
      if (e) throw e;
      geschrieben.push(a);
    },
    kontext: o.kontext ?? (async () => ({ staende: [], spur: null })),
    hinzufuegen: async (a: { gruppe: string; pfade?: string[] }) => ({
      abgebrochen: false,
      ergebnisse: (a.pfade ?? []).map((p, i) => ({ pfad: p, ok: true, uebernahme: { id: `h${i}`, gruppe: a.gruppe, quelle_pfad: `x/h${i}.quelle.png`, format: "png", dateizeit_ms: Date.UTC(2026, 9, 1, 8) } })),
    }),
  } as unknown as BildAktionen;
  const kodieren = async () => ({ jpeg: JPEG, vorschau: JPEG, breite: 9, hoehe: 9 });
  return { api, log, geschrieben, uebernehmer: erzeugeUebernehmer({ api, kodieren, jetzt: () => 5 }), maxGleichzeitig: () => maxGleichzeitig, liste };
}

describe("Uebernehmer", () => {
  it("uebernimmt strikt nacheinander und schreibt kanonischen Hash + Metadaten aus der Fund-Basis", async () => {
    const a = aufbau({
      funde: [
        fund({ fund_id: "f1", basis: { breite: 50.1, laenge: 8.2, hoehe_ft: 3000, pirep_id: "P9", kurs: 90 } }),
        fund({ fund_id: "f2", basis: { pirep_id: "P9" } }),
      ],
    });
    const z = await a.uebernehmer.eingangAbarbeiten();
    expect(z).toMatchObject({ neu: 2, mit_daten: 1, nur_flug: 1, fehler: 0 });
    expect(a.maxGleichzeitig()).toBe(1);
    expect(a.log).toEqual(["verschieben:f1:P9", "verschieben:f2:P9"]);
    const s = a.geschrieben[0];
    expect(s.sha256).toBe(await sha256Hex(JPEG));
    expect(s.quelleEntfernen).toBe(true);
    const sc = s.sidecar as { meta: { quelle: string; hoehe_ft: number | null }; sha256: string };
    expect(sc.meta.quelle).toBe("sim");
    // Die Datei traegt EXIF-GPS; der Hash gilt den Bytes ohne EXIF.
    const datei = Uint8Array.from(atob(s.jpegBase64!), (c) => c.charCodeAt(0));
    expect(app1Anzahl(datei)).toBe(1);
    expect(exifGpsLesen(datei)!.breite).toBeCloseTo(50.1, 3);
    expect(sc.sha256).toBe(s.sha256);
  });
  it("ohne Position: kein EXIF, keine Ersatzwerte", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "f", basis: { pirep_id: "P" } })] });
    await a.uebernehmer.eingangAbarbeiten();
    const datei = Uint8Array.from(atob(a.geschrieben[0].jpegBase64!), (c) => c.charCodeAt(0));
    expect(app1Anzahl(datei)).toBe(0);
    const m = (a.geschrieben[0].sidecar as { meta: Record<string, unknown> }).meta;
    expect([m.hoehe_ft, m.gs_kt, m.breite]).toEqual([null, null, null]);
  });
  it("verwirft nach Regel, ohne die Quelldatei anzufassen (modus verwerfen)", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "x", art: "aufgespuert", sim_vordergrund: false })] });
    const z = await a.uebernehmer.eingangAbarbeiten();
    expect(z).toMatchObject({ verworfen: 1, neu: 0 });
    expect(a.log).toEqual(["verwerfen:x:ohne-flug"]);
    expect(a.geschrieben).toEqual([]);
  });
  it("Duplikat wird gezaehlt, nicht als Fehler", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "d" })], schreibenFehler: () => ({ code: "duplikat", message: "da" }) });
    expect(await a.uebernehmer.eingangAbarbeiten()).toMatchObject({ duplikate: 1, fehler: 0, neu: 0 });
  });
  it("ein Fehler stoppt die Reihe nicht und der Fund wird nach drei Versuchen liegen gelassen", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "k" }), fund({ fund_id: "g" })], schreibenFehler: (n) => (n % 2 === 1 ? { code: "bild_io", message: "voll" } : null) });
    const z = await a.uebernehmer.eingangAbarbeiten();
    expect(z).toMatchObject({ fehler: 1, neu: 1 });
    expect(z.letzterFehler).toContain("bild_io");
    await a.uebernehmer.eingangAbarbeiten();
    await a.uebernehmer.eingangAbarbeiten();
    const log0 = a.log.length;
    await a.uebernehmer.eingangAbarbeiten();
    expect(a.log.slice(log0).some((l) => l.includes(":k:"))).toBe(false);
  });
  it("Verschieben verweigert: faellt auf Kopieren zurueck", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "v", art: "standard" })] });
    let erster = true;
    const orig = a.api.uebernehmen;
    a.api.uebernehmen = async (x) => { if (erster && x.modus === "verschieben") { erster = false; throw { code: "bild_quelle_gesperrt", message: "x" }; } return orig(x); };
    expect((await a.uebernehmer.eingangAbarbeiten()).neu).toBe(1);
    expect(a.log).toEqual(["kopieren:v:ohne-flug"]);
  });
  it("gleichzeitiger Weckruf startet keinen zweiten Lauf, holt aber nach", async () => {
    const a = aufbau({ funde: [fund({ fund_id: "w" })] });
    const [z1, z2] = await Promise.all([a.uebernehmer.eingangAbarbeiten(), a.uebernehmer.eingangAbarbeiten()]);
    expect(z2.neu).toBe(0);
    expect(z1.neu).toBeGreaterThanOrEqual(1);
    expect(a.maxGleichzeitig()).toBe(1);
  });
  it("Eingangskopie ohne Sidecar wird aus dem Kontext der Dateizeit fertiggestellt", async () => {
    const rest = { ...eintrag({ id: "r1", fertig: false }), quelle_pfad: "eingang/r1.quelle.png", gruppe: "eingang", dateizeit_ms: Date.UTC(2026, 10, 7, 10) };
    const a = aufbau({
      eintraege: [rest],
      kontext: async () => ({ staende: [{ zeit: null, quelle: "protokoll", basis: { breite: 10, laenge: 20, pirep_id: "PX" } }], spur: null }),
    });
    const z = await a.uebernehmer.eingangAbarbeiten();
    expect(z.neu).toBe(1);
    expect((a.geschrieben[0].sidecar as { meta: { quelle: string } }).meta.quelle).toBe("protokoll");
    expect(a.geschrieben[0].id).toBe("r1");
  });
  it("Altbestand: Stufen protokoll, logbuch und keine werden gezaehlt; Kontextfehler ergibt keine Daten", async () => {
    const staende = [
      { zeit: null, quelle: "protokoll" as const, basis: { breite: 1, laenge: 2, pirep_id: "A" } },
      { zeit: null, quelle: "logbuch" as const, basis: { pirep_id: "B", abflug: "EDDF" } },
    ];
    let n = 0;
    const a = aufbau({ kontext: async () => { const i = n++; if (i === 2) throw new Error("offline"); return { staende: staende[i] ? [staende[i]] : [], spur: null }; } });
    const r = await a.uebernehmer.hinzufuegen(["/a.png", "/b.png", "/c.png", "/d.png"]);
    expect(r.zaehlung).toMatchObject({ neu: 4, mit_daten: 1, nur_flug: 1, ohne_daten: 2 });
    const q = a.geschrieben.map((g) => (g.sidecar as { meta: { quelle: string } }).meta.quelle);
    expect(q).toEqual(["protokoll", "logbuch", "keine", "keine"]);
    expect(a.geschrieben[0].gruppe).toBe("ohne-flug");
  });
  it("Altbestand: abgebrochene Dateiauswahl aendert nichts", async () => {
    const a = aufbau();
    a.api.hinzufuegen = async () => ({ abgebrochen: true, ergebnisse: [] });
    expect((await a.uebernehmer.hinzufuegen(undefined)).abgebrochen).toBe(true);
  });
});
