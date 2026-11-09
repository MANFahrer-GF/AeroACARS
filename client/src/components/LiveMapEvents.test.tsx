// Die Ereignisliste der Live-Karte: Eintraege aus dem Strom und der Klick auf einen Eintrag mit Position.
//
// Abnahmeliste Live-Karte, Zeile B12 (docs/spec/livekarte-umbau-abnahme.md): Eintraege, Klick fliegt zur Position.
//
// `useMapEvents` setzt heute an keinem Eintrag eine `position` (siehe Kopfkommentar in LiveMapEvents.tsx).
// "Klick fliegt zur Position" ist deshalb im laufenden Betrieb nicht erreichbar. Dieser Test belegt
// die KOMPONENTE: Sie zeigt Eintraege aus dem Strom an, und ein Eintrag, der eine Position traegt,
// ist anklickbar und gibt sie weiter. Er behauptet nicht, dass der Strom je eine Position liefert.

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const h = vi.hoisted(() => ({
  aktivitaet: [] as unknown[],
  faden: [] as unknown[],
  rufe: [] as string[],
}));
vi.mock("../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string) => {
    h.rufe.push(cmd);
    if (cmd === "activity_log_get") return h.aktivitaet;
    if (cmd === "hoppie_get_thread") return h.faden;
    return null;
  }),
  listen: vi.fn(async () => () => {}),
}));

import { LiveMapEventList, useMapEvents, type MapEvent } from "./LiveMapEvents";

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
beforeEach(() => {
  h.aktivitaet = [];
  h.faden = [];
  h.rufe = [];
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const telex = (text: string, at: string, direction: "sent" | "received" = "received") => ({
  kind: "telex", direction, text, at, min: null, mrn: null, response: null, element_id: null,
  closed: null, deferred: null, superseded: null, station: "EDDF",
});

/** Hook und Liste zusammen, so wie die Karte sie verdrahtet. */
function Strom({ aktiv, extra = [], onJumpTo = () => {} }: { aktiv: boolean; extra?: MapEvent[]; onJumpTo?: (p: [number, number]) => void }) {
  const { events } = useMapEvents(aktiv);
  return <LiveMapEventList events={[...events, ...extra]} callsign="DLH400" onJumpTo={onJumpTo} />;
}
const zeilen = () => [...document.querySelectorAll(".aa-livemap-events__row")];
const titel = () => zeilen().map((z) => z.querySelector(".aa-livemap-events__title-text")?.textContent);

describe("Ereignisliste (B12)", () => {
  it("zeigt Eintraege aus dem Strom: Systemmeldungen und ACARS, neueste zuerst, mit Uhrzeit in UTC", async () => {
    h.aktivitaet = [
      { timestamp: "2026-11-09T10:00:01Z", level: "info", message: "Simulator verbunden", detail: "MSFS 2024" },
      { timestamp: "2026-11-09T10:05:30Z", level: "warn", message: "Position nachgeholt ❤️", detail: undefined },
    ];
    h.faden = [telex("CLEARED TO KJFK VIA FRANKFURT 4A", "2026-11-09T10:03:00Z")];
    render(<Strom aktiv />);
    await waitFor(() => expect(zeilen()).toHaveLength(3));
    expect(titel()).toEqual(["Position nachgeholt", "PDC erhalten", "Simulator verbunden"]);
    const uhr = zeilen().map((z) => z.querySelector(".aa-livemap-events__time")?.textContent);
    expect(uhr).toEqual(["10:05:30z", "10:03:00z", "10:00:01z"]);
    expect(zeilen()[2].querySelector(".aa-livemap-events__sub")?.textContent).toBe("MSFS 2024");
    expect(zeilen()[1].querySelector(".aa-livemap-events__sub")?.textContent).toBe("CLEARED TO KJFK VIA FRANKFURT 4A");
    // Eintraege ohne Position sind keine Knoepfe.
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("benennt ACARS-Meldungen nach Art und Richtung", async () => {
    h.faden = [
      telex("OUT 1042 EDDF", "2026-11-09T10:01:00Z", "sent"),
      telex("POS N50 E008", "2026-11-09T10:02:00Z", "sent"),
    ];
    render(<Strom aktiv />);
    await waitFor(() => expect(zeilen()).toHaveLength(2));
    expect(titel()).toEqual(["POS REPORT gesendet", "OUT REPORT gesendet"]);
  });

  it("sagt, dass noch nichts vorliegt, und nennt das Rufzeichen im Kopf", async () => {
    render(<Strom aktiv />);
    await waitFor(() => expect(screen.getByText("Noch keine Ereignisse.")).toBeTruthy());
    expect(screen.getByText("EREIGNISSE · DLH400")).toBeTruthy();
    render(<LiveMapEventList events={[]} callsign={null} onJumpTo={() => {}} />);
    expect(screen.getByText("EREIGNISSE · —")).toBeTruthy();
  });

  it("holt nur, solange die Karte vorn liegt, und die Systemmeldungen im 2-s-Takt", async () => {
    render(<Strom aktiv={false} />);
    await act(async () => { await new Promise((r) => setTimeout(r, 30)); });
    expect(h.rufe).toEqual([]);
    cleanup();

    vi.useFakeTimers();
    render(<Strom aktiv />);
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    const zuerst = h.rufe.filter((c) => c === "activity_log_get").length;
    expect(zuerst).toBe(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(h.rufe.filter((c) => c === "activity_log_get").length).toBe(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(h.rufe.filter((c) => c === "activity_log_get").length).toBe(4);
  });

  it("macht einen Eintrag mit Position zum Knopf, der die Position weitergibt", () => {
    const abflug: MapEvent = {
      key: "flug-1", at: "2026-11-09T10:07:09Z", category: "flug", title: "Abheben", sub: "EDDF 25C", position: [8.57, 50.03],
    };
    const ohne: MapEvent = { key: "sys-1", at: "2026-11-09T10:00:00Z", category: "system", title: "Simulator verbunden" };
    const sprung = vi.fn();
    render(<LiveMapEventList events={[abflug, ohne]} callsign="DLH400" onJumpTo={sprung} />);
    const knoepfe = screen.getAllByRole("button");
    expect(knoepfe).toHaveLength(1);
    expect(knoepfe[0].getAttribute("aria-label")).toBe("10:07:09z Abheben, Position auf Karte zeigen");
    expect(knoepfe[0].textContent).toContain("EDDF 25C");
    fireEvent.click(knoepfe[0]);
    expect(sprung).toHaveBeenCalledTimes(1);
    expect(sprung).toHaveBeenCalledWith([8.57, 50.03]);
    // Der Eintrag ohne Position tut beim Klick nichts.
    fireEvent.click(screen.getByText("Simulator verbunden"));
    expect(sprung).toHaveBeenCalledTimes(1);
  });

  it("zeigt Positionseintraege aus dem Strom neben denen ohne Position, in Zeitfolge", async () => {
    h.aktivitaet = [{ timestamp: "2026-11-09T10:00:01Z", level: "info", message: "Simulator verbunden" }];
    const sprung = vi.fn();
    const mitPosition: MapEvent = { key: "flug-1", at: "2026-11-09T10:07:09Z", category: "flug", title: "Fahrwerk eingefahren", position: [8.9, 50.2] };
    render(<Strom aktiv extra={[mitPosition]} onJumpTo={sprung} />);
    await waitFor(() => expect(titel()).toContain("Simulator verbunden"));
    expect(titel()).toContain("Fahrwerk eingefahren");
    fireEvent.click(screen.getByRole("button", { name: /Fahrwerk eingefahren/ }));
    expect(sprung).toHaveBeenCalledWith([8.9, 50.2]);
  });

  it("blendet nur neu eingetroffene Eintraege ein, nicht die ganze Liste beim Oeffnen", () => {
    const alt: MapEvent = { key: "a", at: "2026-11-09T10:00:00Z", category: "system", title: "Alt" };
    const neu: MapEvent = { key: "b", at: "2026-11-09T10:01:00Z", category: "system", title: "Neu" };
    const { rerender } = render(<LiveMapEventList events={[alt]} callsign="X" onJumpTo={() => {}} />);
    expect(document.querySelectorAll(".aa-livemap-events__row--fresh")).toHaveLength(0);
    rerender(<LiveMapEventList events={[neu, alt]} callsign="X" onJumpTo={() => {}} />);
    const frisch = [...document.querySelectorAll(".aa-livemap-events__row--fresh")];
    expect(frisch).toHaveLength(1);
    expect(frisch[0].textContent).toContain("Neu");
    rerender(<LiveMapEventList events={[neu, alt]} callsign="X" onJumpTo={() => {}} />);
    expect(document.querySelectorAll(".aa-livemap-events__row--fresh")).toHaveLength(0);
  });
});
