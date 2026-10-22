// Boarding-Musik (2.1.1, Design Thomas 19.10.2026): Kasten auf der Seite
// „Kabine“ mit Zustand, Schalter und Dialog. Festgehalten am gerenderten
// Bild: Der Schalter schaltet Musik und Lied ab, der Dialog zeigt nur
// Airlines mit Musik (plus die des Flugs), „Hinzufügen“ kopiert in den
// Ordner der gewählten Airline, „Entfernen“ fragt nach.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { act, render, screen, cleanup, fireEvent, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";

const aufrufe: { cmd: string; args?: Record<string, unknown> }[] = [];
const anfang = [
  { airline: "DLH", datei: "Theme.mp3", dauer_s: 184.2 },
  { airline: "KLM", datei: "Blue.mp3", dauer_s: 158 },
];
let stuecke = [...anfang];
vi.mock("../../lib/ipc", () => ({
  isTauri: true,
  formatIpcError: (e: unknown) => String(e),
  listen: vi.fn(async () => () => undefined),
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    aufrufe.push({ cmd, args });
    if (cmd === "kabinen_airlines") return [{ icao: "DLH", name: "Lufthansa" }, { icao: "KLM", name: "KLM" }, { icao: "SWR", name: "Swiss" }];
    if (cmd === "kabinen_musik_ablage") return { ordner: "/x", flug_airline: "SWR", stuecke };
    if (cmd === "kabinen_musik_pruefen") return 20;
    if (cmd === "kabinen_musik_hinzufuegen") {
      const s = { airline: String(args?.airline), datei: "neu.mp3", dauer_s: 20 };
      stuecke = [...stuecke, s];
      return s;
    }
    return null;
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => "/home/neu.mp3") }));

import { MusikKasten, MusikRegler } from "./BoardingMusik";
import type { MusikZustand } from "../../lib/kabinenansagen";

const t = (k: string, o?: Record<string, unknown>) => i18next.t(k, o) as string;
const zustand = (z: Partial<MusikZustand>): MusikZustand => ({
  an: true, lautstaerke: 0.2, stand: "laeuft", quelle: "lied", titel: "Fly with Swiss", strecke: "Zürich → Budapest",
  ordner: null, stuecke: 0, airline: "SWR", ...z,
});

beforeAll(async () => {
  await i18next.use(initReactI18next).init({ lng: "de", resources: { de: { common: deCommon } }, ns: ["common"], defaultNS: "common" });
});
afterEach(() => {
  cleanup();
  aufrufe.length = 0;
  stuecke = [...anfang];
});

describe("Kasten Boarding-Musik", () => {
  it("zeigt Lied, Strecke, Länge und den Zustand als Plakette", () => {
    render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    const k = screen.getByTestId("kabine-musik");
    expect(within(k).getByText("Boarding-Musik · Fly with Swiss")).toBeTruthy();
    expect(within(k).getByText("Für diesen Flug erzeugt · Zürich → Budapest · 3:00")).toBeTruthy();
    expect(screen.getByTestId("kabine-musik-stand").textContent).toBe("läuft");
    expect(within(k).getByText(/nach der Landung bis zum Aussteigen/)).toBeTruthy();
  });

  it("nennt den Abschnitt, solange sie läuft (Zusatz I)", () => {
    render(<MusikKasten zustand={zustand({ abschnitt: "ankunft" })} t={t} onNeu={() => undefined} />);
    expect(screen.getByTestId("kabine-musik-stand").textContent).toBe("läuft · Ankunft");
  });

  it("nennt Pause, Ausfall und Aus", () => {
    for (const [stand, text] of [["pausiert", "pausiert für Ansage"], ["kein_lied", "nicht geladen"], ["wird_erzeugt", "wird erzeugt"], ["aus", "aus"]] as const) {
      render(<MusikKasten zustand={zustand({ stand })} t={t} onNeu={() => undefined} />);
      expect(screen.getByTestId("kabine-musik-stand").textContent).toBe(text);
      cleanup();
    }
  });

  it("„Boarding-Musik laden“ nur ohne Musik, stößt das Laden an (Thomas 19.10.2026)", async () => {
    render(<MusikKasten zustand={zustand({ stand: "bereit" })} t={t} onNeu={() => undefined} />);
    expect(screen.queryByRole("button", { name: "Boarding-Musik laden" })).toBeNull();
    cleanup();
    render(<MusikKasten zustand={zustand({ stand: "kein_lied" })} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Boarding-Musik laden" }));
    await waitFor(() => expect(aufrufe.some((a) => a.cmd === "kabinen_musik_laden")).toBe(true));
  });

  it("Schalter aus schickt boarding_musik = false (Entscheidung 15)", async () => {
    render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Boarding-Musik an" }));
    await waitFor(() => expect(aufrufe.find((a) => a.cmd === "kabinen_boarding_musik")?.args).toEqual({ an: false }));
  });

  it("eigene Musik: Airline mit Namen und Zahl der Stücke", () => {
    render(<MusikKasten zustand={zustand({ quelle: "eigene", ordner: "Alle", stuecke: 2, titel: null, stand: "bereit" })} t={t} onNeu={() => undefined} />);
    expect(screen.getByText("Eigene Musik · Alle Airlines · 2 Stücke")).toBeTruthy();
  });
});

describe("Dialog Eigene Boarding-Musik", () => {
  it("zeigt nur Airlines mit Musik plus die des Flugs, die des Flugs offen", async () => {
    render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Eigene Musik …" }));
    const d = await screen.findByTestId("kabine-musik-dialog");
    await waitFor(() => expect(within(d).getByRole("button", { name: /Swiss \(SWR\)/ })).toBeTruthy());
    await waitFor(() => {
      const koepfe = Array.from(d.querySelectorAll(".kabine-musik-gruppe-kopf"));
      expect(koepfe.map((b) => `${b.getAttribute("aria-expanded")} ${b.textContent}`)).toEqual(["true ▾ Swiss (SWR)0 Stücke", "false ▸ Lufthansa (DLH)1 Stück", "false ▸ KLM (KLM)1 Stück"]);
    });
    expect(within(d).getAllByText("Keine eigene Musik").length).toBe(1);
    expect(within(d).queryByText("Alle Airlines", { selector: "button span" })).toBeNull();
    expect(within(d).queryByPlaceholderText(/Airline suchen/)).toBeNull();
  });

  it("Datei wählen, kurz-Hinweis, für diese Airline hinzufügen", async () => {
    render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Eigene Musik …" }));
    const d = await screen.findByTestId("kabine-musik-dialog");
    fireEvent.click(within(d).getByRole("button", { name: "Datei wählen" }));
    await within(d).findByText("neu.mp3 · 0:20");
    expect(within(d).getByText(/Kürzer als 30 Sekunden/)).toBeTruthy();
    await waitFor(() => expect((within(d).getByRole("radio", { name: "Diese Airline: Swiss (SWR)" }) as HTMLInputElement).checked).toBe(true));
    fireEvent.click(within(d).getByRole("button", { name: "Hinzufügen" }));
    await waitFor(() => expect(aufrufe.find((a) => a.cmd === "kabinen_musik_hinzufuegen")?.args).toEqual({ quelle: "/home/neu.mp3", airline: "SWR" }));
  });

  it("Entfernen fragt nach, Abbrechen löscht nichts", async () => {
    render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Eigene Musik …" }));
    const d = await screen.findByTestId("kabine-musik-dialog");
    fireEvent.click(await within(d).findByRole("button", { name: /Lufthansa \(DLH\)/ }));
    const zeile = within(d).getByText("Theme.mp3").closest("li") as HTMLElement;
    fireEvent.click(within(zeile).getByRole("button", { name: "Entfernen" }));
    expect(await screen.findByText("Aus der Boarding-Musik entfernen?")).toBeTruthy();
    const frage = screen.getByText("Aus der Boarding-Musik entfernen?").closest("[role=dialog]") as HTMLElement;
    fireEvent.click(within(frage).getAllByRole("button", { name: "Abbrechen" }).at(-1)!);
    await waitFor(() => expect(screen.queryByText("Aus der Boarding-Musik entfernen?")).toBeNull());
    expect(aufrufe.some((a) => a.cmd === "kabinen_musik_entfernen")).toBe(false);
  });

  it("Neuzeichnen des Kastens setzt den Fokus im Dialog nicht um (Thomas 19.10.2026)", async () => {
    const { rerender } = render(<MusikKasten zustand={zustand({})} t={t} onNeu={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Eigene Musik …" }));
    const d = await screen.findByTestId("kabine-musik-dialog");
    fireEvent.click(await within(d).findByRole("radio", { name: "Andere Airline …" }));
    await within(d).findByPlaceholderText(/Airline suchen/);
    // Offene Lesevorgänge (Ablage, Airlines) abwarten, bevor gezählt wird.
    await waitFor(() => expect(within(d).getByRole("radio", { name: "Diese Airline: Swiss (SWR)" })).toBeTruthy());
    await act(async () => { await new Promise((r) => setTimeout(r, 50)); });
    // Ab hier: Wer fokussiert etwas? Der Pilot tippt gerade ins Feld.
    const fokus = vi.spyOn(HTMLElement.prototype, "focus");
    // Wie die Seite alle 2 s: neuer Zustand, neue Rückruf-Funktion.
    rerender(<MusikKasten zustand={zustand({ stand: "pausiert" })} t={t} onNeu={() => undefined} />);
    rerender(<MusikKasten zustand={zustand({ stand: "laeuft" })} t={t} onNeu={() => undefined} />);
    // Bei Rot: wer den Fokus bekam (Hilfe zur Diagnose).
    if (fokus.mock.calls.length) console.log("Fokus an", fokus.mock.contexts.map((c) => (c as HTMLElement).outerHTML.slice(0, 120)).join(" || "));
    expect(fokus).not.toHaveBeenCalled();
    fokus.mockRestore();
  });
});

// Thomas 22.10.2026: „Wenn ich die Lautstärke verstelle, verändert sich alles.“
// Nachgestellt: Nach dem Loslassen sprang der Regler auf den zuletzt
// abgefragten alten Wert zurück, bis die nächste Abfrage kam.
describe("Lautstärke-Regler springt nach dem Loslassen nicht zurück", () => {
  it("behält den gezogenen Wert, bis der neue Stand gemeldet ist", () => {
    const t = i18next.getFixedT("de");
    const { rerender } = render(<MusikRegler lautstaerke={0.2} t={t} />);
    const regler = screen.getByRole("slider");
    fireEvent.change(regler, { target: { value: "60" } });
    fireEvent.pointerUp(regler);
    expect((regler as HTMLInputElement).value).toBe("60");
    // Abfrage liefert noch den alten Wert: bleibt bei 60.
    rerender(<MusikRegler lautstaerke={0.2} t={t} />);
    expect((regler as HTMLInputElement).value).toBe("60");
    // Neuer Stand gemeldet, danach folgt der Regler wieder von außen.
    rerender(<MusikRegler lautstaerke={0.6} t={t} />);
    rerender(<MusikRegler lautstaerke={0.3} t={t} />);
    expect((regler as HTMLInputElement).value).toBe("30");
  });
});
