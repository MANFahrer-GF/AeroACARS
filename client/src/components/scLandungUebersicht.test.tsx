// Shared Cockpit (B4): Landungsübersicht wie die Webapp-Historie. Der
// Partnerflug trägt das Kennzeichen am Datensatz (`shared_cockpit`): Die Zeile
// zeigt die Landung des Gastgebers mit „SC“, Durchschnitt, Kurve, Kategorien,
// Fußzeile und Bestwert rechnen nur mit eigenen Landungen. Gegenprobe:
// Datensätze ohne Feld verhalten sich exakt wie bisher.
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { LandingRecord } from "../lib/landungsDatensatz";
import THY from "../lib/landungsFaelle.thy39.json";
import { eigeneLandungen, scAnzahl, scVergessen } from "../lib/scLandung";

const lage = vi.hoisted(() => ({ liste: [] as unknown[] }));

vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  invoke: (cmd: string) => {
    if (cmd === "landing_get_current") return Promise.resolve(null);
    if (cmd === "landing_list_stand") return Promise.resolve("1");
    if (cmd === "landing_list") return Promise.resolve(lage.liste);
    if (cmd === "airport_get") return Promise.resolve({ name: null });
    if (cmd === "pirep_pruefstatus") return Promise.resolve([]);
    // Ohne Fähigkeit fragt die Detailansicht live nicht (wie ein Programm ohne sc-landung-1).
    if (cmd === "paket_info") return Promise.resolve({ faehigkeiten: [] });
    return Promise.reject(new Error(`kein ${cmd} im Test`));
  },
  listen: () => Promise.resolve(() => {}),
}));

const { LandingPanel, LandingDetail } = await import("./LandingPanel");

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(() => {
  cleanup();
  lage.liste = [];
  scVergessen();
});

const EIGEN = THY as unknown as LandingRecord; // -46 fpm, 87 Punkte, 1.04 G
const GASTGEBER = {
  status: "gelandet", touchdown_id: 4711, fuehrender_pirep_id: "H1", gastgeber_name: "Anna B.",
  rolle_gastgeber: "PF", landing_rate_fpm: -213.4, landing_score: 61, g_force: 1.37, bounce_count: 0, score_text: "firm",
} as const;

/** Der Partnerflug mit eigener Messung -20 fpm / 12 Punkte / 2,05 G. */
function partnerflug(mitKennzeichen: boolean): LandingRecord {
  return {
    ...EIGEN,
    pirep_id: "SC1",
    touchdown_at: "2026-11-13T10:00:00.000Z",
    dpt_airport: "EDDF",
    arr_airport: "EDDM",
    vs_at_edge_fpm: -20,
    landing_peak_vs_fpm: -20,
    landing_rate_fpm: -20,
    score_numeric: 12,
    grade_letter: "F",
    landing_scored_g_force: 2.05,
    ...(mitKennzeichen ? { shared_cockpit: GASTGEBER } : {}),
  } as LandingRecord;
}

const ruhe = () => act(async () => { await new Promise((r) => setTimeout(r, 50)); });

async function uebersicht(liste: LandingRecord[]) {
  lage.liste = liste;
  const r = render(<LandingPanel />);
  await ruhe();
  const zeilen = [...r.container.querySelectorAll("tr.landing-ov-row")] as HTMLElement[];
  const zeile = (dpt: string) => zeilen.find((z) => z.textContent?.includes(dpt))!;
  return { ...r, zeile };
}

describe("Shared Cockpit in der Landungsübersicht", () => {
  it("zeigt beim Partnerflug die Landung des Gastgebers mit SC und zählt ihn nirgends", async () => {
    const { container, zeile } = await uebersicht([EIGEN, partnerflug(true)]);
    const sc = zeile("EDDF");
    expect(sc.textContent).toContain("-213");
    expect(sc.textContent).toMatch(/1[.,]37/);
    expect(sc.textContent).toContain("61");
    expect(sc.textContent).not.toContain("-20");
    expect(sc.textContent).not.toMatch(/2[.,]05/);
    const marke = within(sc).getByTestId("sc-kennzeichen");
    expect(marke.textContent).toBe("SC");
    expect(marke.getAttribute("title")).toBe("Shared Cockpit · Landung von Anna B.");
    expect(sc.querySelector(".landing-ov-grade--firm")).not.toBeNull();
    expect(sc.querySelector(".landing-ov-grade")!.textContent).toBe("SC"); // kein eigener Buchstabe
    // Kopf: nur die eigene Landung (87 Punkte, -46 fpm), getrennt gezählt
    expect(screen.getByText("letzte 1 + 1 Shared Cockpit · aus dem Recorder")).toBeTruthy();
    const kopf = container.querySelector(".landing-ov-stats")!.textContent!;
    expect(kopf).toContain("87,0");
    expect(kopf).toContain("-46");
    expect(kopf).not.toContain("49,5"); // (87 + 12) / 2 — die eigene Messung des Partners
    expect(kopf).not.toContain("74,0"); // (87 + 61) / 2 — die Gastgeber-Landung
    // Kurve braucht zwei eigene Landungen: es gibt nur eine
    expect(container.querySelector(".landing-ov-chart")).toBeNull();
    // Fußzeile: weichste Landung ist die eigene, nicht die -20 des Partnerflugs
    const fuss = container.querySelector(".landing-ov-footer")!.textContent!;
    expect(fuss).toContain("-46 fpm");
    expect(fuss).not.toContain("-20 fpm");
  });

  it("ausstehend: SC-Zeile ohne Werte, auch die eigene Messung nicht", async () => {
    const aus = { ...partnerflug(true), shared_cockpit: { status: "ausstehend", fuehrender_pirep_id: null, gastgeber_name: null, rolle_gastgeber: null } } as LandingRecord;
    const { zeile } = await uebersicht([EIGEN, aus]);
    const sc = zeile("EDDF");
    expect(sc.textContent).not.toContain("-20");
    expect(sc.textContent).not.toContain("12");
    expect(within(sc).getByTestId("sc-kennzeichen").getAttribute("title")).toBe("Shared Cockpit · Landung des Gastgebers");
  });

  it("Gegenprobe: ohne Kennzeichen exakt wie bisher (eigene Messung, zählt mit)", async () => {
    const { container, zeile } = await uebersicht([EIGEN, partnerflug(false)]);
    const z = zeile("EDDF");
    expect(z.textContent).toContain("-20");
    expect(z.textContent).toContain("12");
    expect(z.querySelector("[data-testid=sc-kennzeichen]")).toBeNull();
    expect(screen.getByText("letzte 2 Aufsetzer · aus dem Recorder")).toBeTruthy();
    expect(container.querySelector(".landing-ov-stats")!.textContent).toContain("49,5");
    expect(container.querySelector(".landing-ov-chart")).not.toBeNull();
  });

  it("Bestwert in der Detailansicht: der Partnerflug ist keine eigene Bestleistung", async () => {
    const zeige = async (mit: boolean) => {
      const r = render(<LandingDetail record={EIGEN} allRecords={[EIGEN, partnerflug(mit)]} onBack={() => {}} isPreview={false} />);
      await ruhe();
      const text = r.container.textContent ?? "";
      r.unmount();
      return text;
    };
    expect(await zeige(false)).toContain("-20 fpm (EDDF → EDDM)");
    expect(await zeige(true)).not.toContain("EDDF → EDDM");
  });

  it("Helfer: nur Partnerflüge mit gültigem Kennzeichen fallen heraus", () => {
    const liste = [EIGEN, partnerflug(true), { ...EIGEN, pirep_id: "X", shared_cockpit: null } as LandingRecord];
    expect(eigeneLandungen(liste).map((r) => r.pirep_id)).toEqual([EIGEN.pirep_id, "X"]);
    expect(scAnzahl(liste)).toBe(1);
  });
});
