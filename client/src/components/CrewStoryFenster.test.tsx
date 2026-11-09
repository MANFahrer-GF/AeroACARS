// Crew-Story-Fenster: Serverstand laden, per Knopf speichern, die Fehlerfälle des
// Moduls (423/409/403) und der Rückweg auf einem Programm ohne `modul-api-1`.
import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { Flight } from "../types";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
  openExternal: vi.fn(),
  isTauri: true,
}));

import { CrewStoryFenster } from "./CrewStoryFenster";
import { FlugBuchenMenue } from "./FlugBuchenMenue";

const FLUG = { id: "F1", flight_number: "123", callsign: null, dpt_airport_id: "EDDF", arr_airport_id: "LSZH" } as unknown as Flight;
const HASH_A = "a".repeat(40);
const HASH_B = "b".repeat(40);
const TEXTE = deCommon.bids.crew_story;

type Antwort = { status: number; daten: unknown };

/** Antworten je Aufruf in Reihenfolge; die Prüfung antwortet immer gleich. */
function server(...speichern: Antwort[]) {
  return (laden: Antwort, pruefen: Antwort = { status: 200, daten: { status: "uebernommen", gruende: [] } }) => {
    invokeMock.mockImplementation(async (cmd: string, a: { methode: string; pfad: string }) => {
      if (cmd !== "modul_api") return null;
      if (a.methode === "GET") return laden;
      if (a.pfad.endsWith("/pruefen")) return pruefen;
      const n = speichern.shift();
      if (!n) throw new Error("kein weiteres Speichern erwartet");
      return n;
    });
  };
}

const speicherAufrufe = () =>
  invokeMock.mock.calls
    .filter((c) => c[0] === "modul_api")
    .map((c) => c[1] as { methode: string; pfad: string; koerper?: Record<string, unknown> })
    .filter((a) => a.methode === "POST" && !a.pfad.endsWith("/pruefen"));

const stand = (text: string, extra: object = {}) => ({
  status: 200,
  daten: { text, hash: text ? HASH_A : null, pruefung: text ? { status: "uebernommen", gruende: [] } : null, gesperrt: false, max: 500, ...extra },
});

async function feld() {
  return (await screen.findByRole("textbox")) as HTMLTextAreaElement;
}

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

beforeEach(() => invokeMock.mockReset());

describe("Crew-Story-Fenster", () => {
  it("lädt den Serverstand über modul_api und zeigt die Prüfung", async () => {
    server()(stand("Hochzeit an Bord"));
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    expect((await feld()).value).toBe("Hochzeit an Bord");
    expect(invokeMock).toHaveBeenCalledWith("modul_api", { methode: "GET", pfad: "/api/kabinenansagen/briefing", abfrage: { flight_id: "F1" } });
    expect(screen.getByText(TEXTE.pruefung.uebernommen)).toBeTruthy();
    expect(screen.getByText("16/500")).toBeTruthy();
  });

  it("speichert per Knopf mit quelle, rev, basis und prüft danach den neuen Hash", async () => {
    server({ status: 200, daten: { text: "Neu bereinigt", hash: HASH_B } })(stand("Alt"));
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    fireEvent.change(await feld(), { target: { value: "Neu" } });
    fireEvent.click(screen.getByRole("button", { name: TEXTE.speichern }));
    await screen.findByText(TEXTE.meldung.gespeichert);
    const k = speicherAufrufe()[0]!.koerper!;
    expect(k).toMatchObject({ flight_id: "F1", text: "Neu", rev: 1, anlass: "knopf", basis: HASH_A });
    expect(String(k.quelle)).toMatch(/^[a-z0-9]{16}$/);
    expect((await feld()).value).toBe("Neu bereinigt");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("modul_api", { methode: "POST", pfad: "/api/kabinenansagen/briefing/pruefen", koerper: { flight_id: "F1", hash: HASH_B } }),
    );
    expect((screen.getByRole("button", { name: TEXTE.speichern }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("423: Text bleibt stehen, Fenster sperrt, kein Speichern mehr", async () => {
    server({ status: 423, daten: { fehler: "gesperrt", text: "Alt", hash: HASH_A } })(stand("Alt"));
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    fireEvent.change(await feld(), { target: { value: "Noch nicht gespeichert" } });
    fireEvent.click(screen.getByRole("button", { name: TEXTE.speichern }));
    await screen.findByText(TEXTE.meldung.gesperrt);
    const f = await feld();
    expect(f.value).toBe("Noch nicht gespeichert");
    expect(f.readOnly).toBe(true);
    expect(screen.queryByRole("button", { name: TEXTE.speichern })).toBeNull();
  });

  it("409: zeigt den Serverstand, das zweite Speichern schickt den neuen Hash als Basis und gewinnt", async () => {
    server(
      { status: 409, daten: { fehler: "geaendert", text: "Vom Loadsheet", hash: HASH_B } },
      { status: 200, daten: { text: "Meins", hash: "c".repeat(40) } },
    )(stand("Alt"));
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    fireEvent.change(await feld(), { target: { value: "Meins" } });
    fireEvent.click(screen.getByRole("button", { name: TEXTE.speichern }));
    await screen.findByText(TEXTE.meldung.geaendert);
    expect(screen.getByTestId("crew-story-server").textContent).toContain("Vom Loadsheet");
    expect((await feld()).value).toBe("Meins");
    fireEvent.click(screen.getByRole("button", { name: TEXTE.speichern }));
    await screen.findByText(TEXTE.meldung.gespeichert);
    expect(speicherAufrufe()[1]!.koerper).toMatchObject({ text: "Meins", rev: 2, basis: HASH_B });
    expect(screen.queryByTestId("crew-story-server")).toBeNull();
  });

  it("403 beim Laden: für das Konto aus, kein Netzfehler", async () => {
    server()({ status: 403, daten: { fehler: "nicht_freigeschaltet" } });
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    await screen.findByText(TEXTE.meldung.kein_zugang);
    expect(screen.queryByText(TEXTE.meldung.netz)).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("gesperrter Flug: nur lesen, nie geprüfte Story zeigt „konnte nicht geprüft werden“ ohne Prüfaufruf", async () => {
    server()(stand("Story", { gesperrt: true, pruefung: null }));
    render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    const f = await feld();
    expect(f.readOnly).toBe(true);
    expect(screen.getByText(TEXTE.gesperrt_hinweis)).toBeTruthy();
    expect(screen.getByText(TEXTE.pruefung.nicht_geprueft)).toBeTruthy();
    expect(invokeMock.mock.calls.some((c) => c[0] === "modul_api" && String((c[1] as { pfad: string }).pfad).endsWith("/pruefen"))).toBe(false);
  });

  it("laufender Flug (nurLesen) bleibt gesperrt, auch wenn der Server offen meldet", async () => {
    server()(stand("Story"));
    render(<CrewStoryFenster flight={FLUG} nurLesen onClose={() => {}} />);
    expect((await feld()).readOnly).toBe(true);
  });

  it("ungespeicherter Text: Klick daneben schließt nicht, Schließen fragt einmal nach", async () => {
    server()(stand("Alt"));
    const zu = vi.fn();
    render(<CrewStoryFenster flight={FLUG} onClose={zu} />);
    fireEvent.change(await feld(), { target: { value: "Neu" } });
    fireEvent.click(screen.getByTestId("crew-story-hintergrund"));
    expect(zu).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: TEXTE.schliessen }));
    expect(zu).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: TEXTE.trotzdem_schliessen }));
    expect(zu).toHaveBeenCalledOnce();
  });

  it("ohne neuen Kern: nur der Hinweis, kein Aufruf", () => {
    render(<CrewStoryFenster flight={FLUG} ohneKern onClose={() => {}} />);
    expect(screen.getByText(TEXTE.neues_aeroacars)).toBeTruthy();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("Text kommt nie als HTML: Gründe des Modells bleiben Text", async () => {
    server()(stand("Story", { pruefung: { status: "teilweise", gruende: ["<img src=x onerror=1>"] } }));
    const { container } = render(<CrewStoryFenster flight={FLUG} onClose={() => {}} />);
    await screen.findByText("<img src=x onerror=1>");
    expect(container.querySelector("img")).toBeNull();
  });
});

describe("Flug buchen", () => {
  it("öffnet genau die vier Buchungswege auf der Webseite", () => {
    const oeffnen = vi.fn();
    render(<FlugBuchenMenue baseUrl="https://gsg.example/" onOeffnen={oeffnen} />);
    expect(screen.queryByRole("menu")).toBeNull();
    const knopf = screen.getByRole("button", { name: deCommon.bids.buchen });
    for (const [name, pfad] of [
      [deCommon.bids.buchen_flugplan, "/flights"],
      [deCommon.bids.buchen_live, "/live-schedules"],
      [deCommon.bids.buchen_freiflug, "/dfreeflight"],
      [deCommon.bids.buchen_adventures, "/adventures"],
    ] as const) {
      fireEvent.click(knopf);
      fireEvent.click(screen.getByRole("menuitem", { name }));
      expect(oeffnen).toHaveBeenLastCalledWith(`https://gsg.example${pfad}`);
      expect(screen.queryByRole("menu")).toBeNull();
    }
    expect(oeffnen).toHaveBeenCalledTimes(4);
  });
});
