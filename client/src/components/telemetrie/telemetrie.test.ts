import { describe, expect, it } from "vitest";
import de from "../../locales/de/common.json";
import en from "../../locales/en/common.json";
import it_ from "../../locales/it/common.json";
import katalogJson from "./vorschauKatalog.json";
import { ereignisseAus } from "./ereignisse";
import { csvText } from "./csv";
import { tankAnsicht, tanksAbweichend } from "./tanks";
import { nichtVerlaesslich, wertMitEinheit, wertText } from "./format";
import type { Frame, Katalog } from "./typen";
import type { Telemetrie } from "./useTelemetrie";

const katalog = katalogJson as Katalog;

describe("Telemetrie-Katalog und Texte", () => {
  const sprachen = { de, en, it: it_ } as Record<string, { telemetrie: { kanal: Record<string, string> } }>;
  for (const [sp, datei] of Object.entries(sprachen)) {
    it(`jeder Kanal hat einen Text auf ${sp}`, () => {
      const fehlend = [...katalog.zahlen, ...katalog.texte]
        .map((k) => k.id)
        .filter((id) => !datei.telemetrie.kanal[id]);
      expect(fehlend).toEqual([]);
    });
  }
});

function index(ids: string[]): Map<string, number> {
  return new Map(ids.map((id, i) => [id, i]));
}
function f(t: number, z: (number | null)[]): Frame {
  return { t, z, s: [] };
}

describe("ereignisseAus", () => {
  const idx = index(["am_boden", "vs", "g", "fahrwerk", "klappen_stufe", "ap", "ias"]);

  it("meldet das Aufsetzen mit der V/S vor dem Bodenkontakt", () => {
    const a = f(1, [0, -180, 1.0, 100, 4, 0, 130]);
    const b = f(2, [1, -20, 1.24, 100, 4, 0, 129]);
    const e = ereignisseAus(a, b, idx);
    expect(e).toHaveLength(1);
    expect(e[0]).toMatchObject({ art: "aufsetzen", wichtig: true, werte: { vs: -180, g: "1.24" } });
  });

  it("Fahrwerk, Klappen und Autopilot", () => {
    const a = f(1, [0, -700, 1, 60, 3, 1, 140]);
    const b = f(2, [0, -700, 1, 100, 4, 0, 140]);
    const arten = ereignisseAus(a, b, idx).map((e) => e.art);
    expect(arten).toEqual(["fahrwerk_unten", "klappen", "ap_aus"]);
  });

  it("fehlende Werte erzeugen keine Ereignisse", () => {
    const a = f(1, [null, null, null, null, null, null, null]);
    const b = f(2, [1, -100, 1.2, 100, 4, 1, 130]);
    expect(ereignisseAus(a, b, idx)).toEqual([]);
    expect(ereignisseAus(null, b, idx)).toEqual([]);
  });
});

describe("csvText", () => {
  it("schreibt Kopf mit Einheit und eine Zeile je Frame", () => {
    const tm = {
      zahlIndex: index(["ias", "vs"]),
      textIndex: index(["fma_lateral"]),
      kanal: (id: string) =>
        ({
          ias: { id: "ias", einheit: "kt" },
          vs: { id: "vs", einheit: "fpm" },
          fma_lateral: { id: "fma_lateral", einheit: "" },
        })[id],
      frames: [
        { t: Date.UTC(2026, 8, 25, 12, 0, 0), z: [137.5, -700], s: ['L"OC'] },
        { t: Date.UTC(2026, 8, 25, 12, 0, 1), z: [null, -690], s: [null] },
      ],
    } as unknown as Telemetrie;
    const csv = csvText(tm, ["ias", "fma_lateral", "gibt_es_nicht"]).trim().split("\n");
    expect(csv[0]).toBe("zeit_utc;ias [kt];fma_lateral");
    expect(csv[1]).toBe('2026-09-25T12:00:00.000Z;137.5;"L""OC"');
    expect(csv[2]).toBe("2026-09-25T12:00:01.000Z;;");
  });
});

function werte(w: Record<string, number>) {
  return (id: string) => (id in w ? w[id] : null);
}

describe("Sprit je Tank", () => {
  it("A380 (iniBuilds, ECAM 26.09.2026): alle elf Tanks, Säule nach Fassungsvermögen", () => {
    // Werte, wie sie der Katalog nach der Umrechnung aus Gallonen liefert.
    const w = werte({
      sprit_gesamt: 18481,
      tank_summe: 18480,
      tank_links_tip: 980,
      tank_links_tip_kap: 3500,
      tank_links_aux: 4080,
      tank_links_aux_kap: 17000,
      tank_links: 0,
      tank_links_kap: 30000,
      tank_mitte: 4160,
      tank_mitte_kap: 17000,
      tank_mitte_2: 4260,
      tank_mitte_2_kap: 17000,
      tank_mitte_3: 0,
      tank_mitte_3_kap: 18000,
      tank_rechts: 0,
      tank_rechts_kap: 30000,
      tank_rechts_aux: 4020,
      tank_rechts_aux_kap: 17000,
      tank_rechts_tip: 980,
      tank_rechts_tip_kap: 3500,
      tank_extern_1: 0,
      tank_extern_1_kap: 29000,
      tank_extern_2: 0,
      tank_extern_2_kap: 29000,
    });
    const a = tankAnsicht(w);
    expect(a.saeulen).toHaveLength(11);
    expect(a.nachKapazitaet).toBe(true);
    const summe = a.saeulen.reduce((s, x) => s + x.kg, 0);
    expect(summe).toBe(18480);
    const mitte = a.saeulen.find((x) => x.id === "tank_mitte");
    expect(mitte?.anteil).toBeCloseTo(4160 / 17000, 5);
    // Kein Hinweis: Summe und Sprit an Bord passen.
    expect(tanksAbweichend(w("tank_summe"), w("sprit_gesamt"))).toBe(false);
  });

  it("Tanks mit Fassungsvermögen 0 fehlen", () => {
    const a = tankAnsicht(
      werte({
        sprit_gesamt: 9000,
        tank_links: 3000,
        tank_links_kap: 6200,
        tank_mitte: 3000,
        tank_mitte_kap: 6500,
        tank_rechts: 3000,
        tank_rechts_kap: 6200,
        tank_links_aux: 0,
        tank_links_aux_kap: 0,
      }),
    );
    expect(a.saeulen.map((x) => x.id)).toEqual(["tank_links", "tank_mitte", "tank_rechts"]);
  });

  it("X-Plane mit fünf Tanks: Säule = Anteil am Sprit an Bord", () => {
    const a = tankAnsicht(
      werte({ sprit_gesamt: 10200, xp_tank_1: 2100, xp_tank_2: 2100, xp_tank_3: 5200, xp_tank_4: 800, xp_tank_5: 0 }),
    );
    expect(a.saeulen.map((x) => x.id)).toEqual(["xp_tank_1", "xp_tank_2", "xp_tank_3", "xp_tank_4", "xp_tank_5"]);
    expect(a.nachKapazitaet).toBe(false);
    expect(a.saeulen[2].anteil).toBeCloseTo(5200 / 10200, 5);
  });

  it("Hinweis erst ab mehr als 2 % Abweichung", () => {
    // Der A380-Fall vor der Korrektur: drei Tanks, 4174 von 18481 kg.
    expect(tanksAbweichend(4174, 18481)).toBe(true);
    expect(tanksAbweichend(18480, 18481)).toBe(false);
    expect(tanksAbweichend(18100, 18481)).toBe(true);
    expect(tanksAbweichend(18200, 18481)).toBe(false);
    expect(tanksAbweichend(null, 18481)).toBe(false);
  });
});

describe("Prüfbericht 26.09.2026", () => {
  const tStub = ((key: string, o?: { defaultValue?: string }) => {
    const pfad = key.split(".");
    let v: unknown = de;
    for (const p of pfad) v = (v as Record<string, unknown> | undefined)?.[p];
    return typeof v === "string" ? v : (o?.defaultValue ?? key);
  }) as unknown as Parameters<typeof wertText>[2];
  const kanal = (id: string) => {
    const k = [...katalog.zahlen, ...katalog.texte].find((x) => x.id === id);
    if (!k) throw new Error(id);
    return k;
  };

  it("Anschnallzeichen zeigt OFF/AUTO/ON statt an/aus", () => {
    const k = kanal("anschnallzeichen");
    expect(k.art).toBe("zahl");
    expect(wertText(k, 0, tStub, "de")).toBe("OFF");
    expect(wertText(k, 1, tStub, "de")).toBe("AUTO");
    expect(wertText(k, 2, tStub, "de")).toBe("ON");
    expect(wertMitEinheit(k, 1, tStub, "de")).toBe("AUTO");
  });

  it("nicht verlässliche Kanäle aus der Liste des Katalogs", () => {
    const s = nichtVerlaesslich("n1_1 egt_1  schubhebel_1");
    expect([...s]).toEqual(["n1_1", "egt_1", "schubhebel_1"]);
    expect(nichtVerlaesslich(null).size).toBe(0);
    expect(katalog.texte.some((k) => k.id === "nicht_verlaesslich")).toBe(true);
  });

  it("MSFS 2024 neues Treibstoffsystem: Tanks 1–5, Tank 6 gibt es nicht", () => {
    const a = tankAnsicht(
      werte({
        sprit_gesamt: 19600,
        fs_tank_1: 4500,
        fs_tank_1_kap: 12000,
        fs_tank_2: 9800,
        fs_tank_2_kap: 12000,
        fs_tank_3: 4500,
        fs_tank_3_kap: 12000,
        fs_tank_4: 400,
        fs_tank_4_kap: 12000,
        fs_tank_5: 400,
        fs_tank_5_kap: 12000,
      }),
    );
    expect(a.saeulen.map((x) => x.id)).toEqual(["fs_tank_1", "fs_tank_2", "fs_tank_3", "fs_tank_4", "fs_tank_5"]);
    expect(a.nachKapazitaet).toBe(true);
  });

  it("Schubumkehr, Soll-Mach, Vne und volle Klappen stehen im Katalog", () => {
    for (const id of ["umkehr_3", "umkehr_4", "soll_mach", "vne", "vfe_voll", "hydraulik_xp_2"]) kanal(id);
    expect(kanal("hydraulik_xp_1").einheit).toBe("");
    expect(kanal("hydraulik").einheit).toBe("psi");
  });
});
