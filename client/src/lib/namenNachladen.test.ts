import { describe, it, expect, vi } from "vitest";
import {
  ladeNamenGedeckelt,
  leeresGedaechtnis,
  type NachladeOptionen,
} from "./namenNachladen";

const SOFORT: NachladeOptionen = { gleichzeitig: 4, hoechstversuche: 3 };

/** Ein Abruf, den der Test von Hand auflösen kann. */
function steuerbarerAbruf() {
  const offen = new Map<string, { auf: (n: string | null) => void; ab: (e: unknown) => void }>();
  const abrufen = (kennung: string) =>
    new Promise<string | null>((auf, ab) => {
      offen.set(kennung, { auf, ab });
    });
  return { abrufen, offen };
}

/** Wartet, bis die Mikrotask-Warteschlange leer ist. */
const durchatmen = () => new Promise<void>((r) => setTimeout(r, 0));

describe("ladeNamenGedeckelt", () => {
  it("fragt hoechstens `gleichzeitig` Kennungen zur selben Zeit an", async () => {
    // Der Kern des Feldbefunds: vorher gingen 89 Abrufe gleichzeitig raus.
    const { abrufen, offen } = steuerbarerAbruf();
    const kennungen = Array.from({ length: 20 }, (_, i) => `K${i}`);

    ladeNamenGedeckelt(kennungen, leeresGedaechtnis(), abrufen, () => {}, SOFORT);
    await durchatmen();

    expect(offen.size).toBe(4);

    // Wird einer beantwortet, rueckt genau einer nach — nie mehr als vier.
    offen.get("K0")!.auf("Name 0");
    offen.delete("K0");
    await durchatmen();
    expect(offen.size).toBe(4);
  });

  it("meldet jeden gefundenen Namen genau einmal", async () => {
    const { abrufen, offen } = steuerbarerAbruf();
    const gemeldet: Array<[string, string]> = [];

    ladeNamenGedeckelt(["A", "B"], leeresGedaechtnis(), abrufen, (k, n) => gemeldet.push([k, n]), {
      gleichzeitig: 2,
      hoechstversuche: 3,
    });
    await durchatmen();

    offen.get("A")!.auf("Alpha");
    offen.get("B")!.auf(null); // kein Name hinterlegt
    await durchatmen();

    expect(gemeldet).toEqual([["A", "Alpha"]]);
  });

  it("gibt beim Abbruch alles frei, was NICHT beantwortet wurde", async () => {
    // DAS ist der schwerste Befund der QS-Runde. Ohne diese Freigabe gelten
    // abgebrochene Kennungen fuer immer als "schon angefragt" und bekommen nie
    // einen Namen.
    const { abrufen, offen } = steuerbarerAbruf();
    const g = leeresGedaechtnis();
    const kennungen = ["A", "B", "C", "D", "E", "F"];

    const abbrechen = ladeNamenGedeckelt(kennungen, g, abrufen, () => {}, {
      gleichzeitig: 2,
      hoechstversuche: 3,
    });
    await durchatmen();

    // A ist fertig, B laeuft noch, C–F wurden nie begonnen.
    offen.get("A")!.auf("Alpha");
    await durchatmen();

    abbrechen();

    expect(g.angefragt.has("A")).toBe(true); // beantwortet -> bleibt gemerkt
    for (const k of ["B", "C", "D", "E", "F"]) {
      expect(g.angefragt.has(k)).toBe(false); // freigegeben -> naechster Lauf holt sie
    }
  });

  it("holt nach einem Abbruch beim naechsten Lauf genau die Restlichen", async () => {
    // Die praktische Folge des Tests darueber, aus Sicht des Aufrufers.
    const { abrufen, offen } = steuerbarerAbruf();
    const g = leeresGedaechtnis();
    const kennungen = ["A", "B", "C", "D"];

    const abbrechen = ladeNamenGedeckelt(kennungen, g, abrufen, () => {}, {
      gleichzeitig: 1,
      hoechstversuche: 3,
    });
    await durchatmen();
    offen.get("A")!.auf("Alpha");
    await durchatmen();
    abbrechen();
    offen.clear();

    // Zweiter Lauf — wie ihn der 5-Sekunden-Takt von `records` ausloest.
    ladeNamenGedeckelt(kennungen, g, abrufen, () => {}, { gleichzeitig: 4, hoechstversuche: 3 });
    await durchatmen();

    expect([...offen.keys()].sort()).toEqual(["B", "C", "D"]);
  });

  it("wiederholt Fehlschlaege — aber nur bis zur Grenze", async () => {
    // Ohne Grenze entsteht mit einem periodisch neu laufenden Aufrufer eine
    // unbegrenzte Wiederholschleife: bei Netzausfall alle Kennungen alle 5 s.
    const g = leeresGedaechtnis();
    let versuche = 0;
    const abrufen = async () => {
      versuche++;
      throw new Error("Netz weg");
    };

    for (let lauf = 0; lauf < 5; lauf++) {
      ladeNamenGedeckelt(["X"], g, abrufen, () => {}, { gleichzeitig: 1, hoechstversuche: 3 });
      await durchatmen();
    }

    expect(versuche).toBe(3);
    expect(g.angefragt.has("X")).toBe(true); // aufgegeben, bleibt gesperrt
  });

  it("fragt bereits bekannte Kennungen nicht erneut an", async () => {
    const { abrufen, offen } = steuerbarerAbruf();
    const g = leeresGedaechtnis();

    ladeNamenGedeckelt(["A", "B"], g, abrufen, () => {}, SOFORT);
    await durchatmen();
    offen.get("A")!.auf("Alpha");
    offen.get("B")!.auf("Bravo");
    await durchatmen();
    offen.clear();

    ladeNamenGedeckelt(["A", "B"], g, abrufen, () => {}, SOFORT);
    await durchatmen();

    expect(offen.size).toBe(0);
  });

  it("meldet nach dem Abbruch nichts mehr", async () => {
    // Sonst schriebe ein spaet eintreffender Abruf in eine Komponente, die
    // laengst etwas anderes anzeigt.
    const { abrufen, offen } = steuerbarerAbruf();
    const melden = vi.fn();

    const abbrechen = ladeNamenGedeckelt(["A"], leeresGedaechtnis(), abrufen, melden, SOFORT);
    await durchatmen();
    abbrechen();
    offen.get("A")!.auf("Alpha");
    await durchatmen();

    expect(melden).not.toHaveBeenCalled();
  });

  it("kommt mit einer leeren Menge klar", async () => {
    const abrufen = vi.fn();
    const abbrechen = ladeNamenGedeckelt([], leeresGedaechtnis(), abrufen, () => {}, SOFORT);
    await durchatmen();
    expect(abrufen).not.toHaveBeenCalled();
    expect(() => abbrechen()).not.toThrow();
  });
});

describe("ladeNamenGedeckelt — Raender (QS-Runde 3)", () => {
  it("fragt eine doppelt uebergebene Kennung nur einmal an", async () => {
    // `filter` prueft die Merkliste, eingetragen wird erst danach — ohne
    // Entdoppelung gingen Dubletten zweimal raus.
    const { abrufen, offen } = steuerbarerAbruf();
    const rufe: string[] = [];
    const zaehlend = (k: string) => {
      rufe.push(k);
      return abrufen(k);
    };

    ladeNamenGedeckelt(["A", "A", "A"], leeresGedaechtnis(), zaehlend, () => {}, SOFORT);
    await durchatmen();

    expect(rufe).toEqual(["A"]);
    expect(offen.size).toBe(1);
  });

  it("ignoriert leere Kennungen", async () => {
    const abrufen = vi.fn(async () => null);
    ladeNamenGedeckelt(["", "A", ""], leeresGedaechtnis(), abrufen, () => {}, SOFORT);
    await durchatmen();
    expect(abrufen).toHaveBeenCalledTimes(1);
    expect(abrufen).toHaveBeenCalledWith("A");
  });

  it("arbeitet auch bei `gleichzeitig: 0` weiter statt haengenzubleiben", async () => {
    // Sonst starteten null Arbeiter, die Kennungen blieben als "angefragt"
    // stehen und niemand fragte je nach.
    const abrufen = vi.fn(async () => "Name");
    const g = leeresGedaechtnis();
    ladeNamenGedeckelt(["A"], g, abrufen, () => {}, { gleichzeitig: 0, hoechstversuche: 3 });
    await durchatmen();
    expect(abrufen).toHaveBeenCalledTimes(1);
  });
});
