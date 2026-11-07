// Rendertests des Bilder-Tabs: echte Komponenten, gestellte `invoke`-Antworten.
import fs from "node:fs";
import path from "node:path";
import { render, screen, waitFor, within, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  aufrufe: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
  zeilen: [] as unknown[],
  einwilligung: true,
  taste: null as null | { simulator: string; text: string },
  kuerzel: null as null | { text: string; verfuegbar: boolean; aktiv: boolean; fehler: null },
  lerne: null as unknown,
  senden: null as null | ((a: Record<string, unknown>) => unknown),
  hoerer: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));

vi.mock("../../lib/ipc", () => ({
  isTauri: true,
  formatIpcError: (e: unknown) => String(e),
  listen: async (n: string, cb: (e: { payload: unknown }) => void) => { h.hoerer.set(n, [...(h.hoerer.get(n) ?? []), cb]); return () => undefined; },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    h.aufrufe.push({ cmd, args });
    switch (cmd) {
      case "bilder_liste":
        return { eintraege: h.zeilen, belegt_bytes: 5_000_000, quellen: [], tastenkuerzel: h.kuerzel, aufnahme_verfuegbar: false, simulator_taste: h.taste, funde: [], altbestand: [] };
      case "ui_state_get_all": return h.einwilligung ? { "aeroacars.bilder.einwilligung.17": "1" } : {};
      case "bild_lesen": return new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
      case "bild_senden": return h.senden ? h.senden(args!) : { bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } };
      case "bild_status": return { bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } };
      case "bild_schreiben": return undefined;
      case "bild_taste_lernen": return h.lerne;
      case "bild_quellen_setzen": return { quellen: [], abgebrochen: false };
      case "flight_get_track": return [];
      default: return undefined;
    }
  },
}));
vi.mock("../../flugkarte/ausschnitt", () => ({ bildKarteAnzeigen: (_r: unknown, o: { beiFehler?: (e: unknown) => void }) => { o.beiFehler?.(new Error("kein WebGL")); return { aktualisieren: () => undefined, aufraeumen: () => undefined }; } }));

import { BilderTab } from "./BilderTab";
import { bilderStore } from "./bilderStore";
import { eintrag } from "./logik/testhilfen";

beforeEach(() => {
  h.aufrufe = []; h.zeilen = []; h.einwilligung = true; h.senden = null; h.taste = null; h.kuerzel = null; h.lerne = null;
  URL.createObjectURL = vi.fn(() => "blob:x");
  URL.revokeObjectURL = vi.fn();
  void bilderStore.pilotSetzen(null);
});

const vorbereiten = (n = 3) => {
  h.zeilen = Array.from({ length: n }, (_, i) => eintrag({ id: `r${i}`, meta: { zeit_utc: `2026-11-0${i + 1}T10:0${i}:00.000Z`, pirep_id: i < 2 ? "P1" : null, hoehe_ft: i === 2 ? null : 1000 + i }, bytes: 1_000_000 }));
};
const zeigen = (flug = true) =>
  render(<BilderTab pilotId="17" pilotName="Thomas K." aktiverFlug={flug ? { pirep_id: "P1", callsign: "GSG412", dpt: "EDDF", arr: "LSZH" } : null} simKind="xplane12" onKarteOeffnen={vi.fn()} />);

describe("Bilder-Tab", () => {
  it("zeigt Gruppen nach Flug, die Gruppe „Ohne Flug“ und kein 0 ft fuer fehlende Hoehe", async () => {
    vorbereiten();
    zeigen();
    expect(await screen.findByText("läuft gerade")).toBeInTheDocument();
    expect(screen.getByText(/Ohne Flug ·/)).toBeInTheDocument();
    expect(screen.getAllByText("Keine Flugdaten").length).toBeGreaterThan(0);
    expect(screen.queryByText(/^0 ft$/)).toBeNull();
    expect(screen.getByText(/3 Bilder · 1 Flug ·/)).toBeInTheDocument();
  });

  it("Auswahl-Checkbox ist vom Oeffnen getrennt (Gegenprobe: Oeffnen waehlt nicht aus)", async () => {
    vorbereiten();
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(boxen[0]);
    expect(await screen.findByText("1 ausgewählt")).toBeInTheDocument();
    const oeffnen = screen.getAllByRole("button", { name: /Details:/ });
    const vorher = oeffnen[1].getAttribute("aria-pressed");
    await userEvent.click(oeffnen[1]);
    expect(screen.getByText("1 ausgewählt")).toBeInTheDocument();
    expect(oeffnen[1].getAttribute("aria-pressed")).not.toBe(vorher);
  });

  it("Senden: Vorschau der Nachricht, Vorgabe verdeckt, ruft bild_senden mit Kennungen und ohne Base64", async () => {
    vorbereiten();
    h.senden = (a) => ({ sendung_id: a.sendungId, bilder: (a.bilder as Array<{ bild_id: string }>).map((b) => ({ bild_id: b.bild_id, status: "gesendet" })), kontingent: { rest: 7, grenze: 12, retry_after: 0 } });
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(boxen[0]);
    await userEvent.click(boxen[1]);
    await userEvent.click(screen.getByRole("button", { name: "2 Bilder senden" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("So erscheint die Nachricht")).toBeInTheDocument();
    expect(within(dialog).getByRole("radio", { name: /Verdeckt/ })).toBeChecked();
    expect(within(dialog).getAllByText(/Geo-Daten verdeckt/)).toHaveLength(2);
    await userEvent.type(within(dialog).getByLabelText(/Text für alle 2 Bilder/), "Hallo");
    await userEvent.click(within(dialog).getByRole("button", { name: "Jetzt senden" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_senden")).toBe(true));
    const a = h.aufrufe.find((x) => x.cmd === "bild_senden")!.args as { text: string; geoStufe: string; bilder: Array<Record<string, unknown>> };
    expect(a.text).toBe("Hallo");
    expect(a.geoStufe).toBe("verdeckt");
    expect(a.bilder).toHaveLength(2);
    expect(Object.keys(a.bilder[0]).sort()).toEqual(["bild_id", "gruppe", "meta"]);
    await waitFor(() => expect(screen.queryByText(/^\d+ ausgewählt$/)).toBeNull());
  });

  it("die Leiste sendet die AUSWAHL, auch wenn ein anderes Bild in der Detailspalte offen ist", async () => {
    vorbereiten();
    h.senden = (a) => ({ bilder: (a.bilder as Array<{ bild_id: string }>).map((b) => ({ bild_id: b.bild_id, status: "gesendet" })), kontingent: null });
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    const offen = screen.getAllByRole("button", { name: /Details:/ }).find((b) => b.getAttribute("aria-pressed") === "true");
    expect(offen).toBeTruthy();
    for (const b of [boxen[0], boxen[2]]) await userEvent.click(b); // das offene Bild (neuestes im Flug) ist NICHT dabei
    await userEvent.click(screen.getByRole("button", { name: "2 Bilder senden" }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Jetzt senden" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_senden")).toBe(true));
    const a = h.aufrufe.find((x) => x.cmd === "bild_senden")!.args as { bilder: Array<{ bild_id: string }> };
    expect(a.bilder).toHaveLength(2);
  });

  it("fehlgeschlagene Bilder bleiben ausgewaehlt und zeigen Grund und „Erneut versuchen“ als Textzeile", async () => {
    vorbereiten();
    h.senden = () => { throw { code: "live_nicht_erreichbar", message: "x" }; };
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(boxen[0]);
    await userEvent.click(screen.getByRole("button", { name: "1 Bild senden" }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Jetzt senden" }));
    expect(await screen.findByText(/Nicht gesendet: Server nicht erreichbar/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeInTheDocument();
    expect(screen.getByText("1 ausgewählt")).toBeInTheDocument();
    expect(screen.getByText("Server nicht erreichbar")).toBeInTheDocument();
  });

  it("Zustellung unklar: eigener Text, Bild bleibt ausgewaehlt", async () => {
    vorbereiten();
    h.senden = () => { throw { code: "live_antwort_fehlt", message: "x" }; };
    zeigen();
    await userEvent.click((await screen.findAllByRole("checkbox", { name: /Auswählen/ }))[0]);
    await userEvent.click(screen.getByRole("button", { name: "1 Bild senden" }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Jetzt senden" }));
    expect(await screen.findByText(/Zustellung unklar, bitte im Kanal prüfen/, { selector: ".bt-notiz" })).toBeInTheDocument();
  });

  it("ohne Einschalten fragt der Tab zuerst (Datenschutz-Hinweis), gesendet wird erst danach", async () => {
    vorbereiten();
    h.einwilligung = false;
    zeigen();
    await userEvent.click((await screen.findAllByRole("checkbox", { name: /Auswählen/ }))[0]);
    await userEvent.click(screen.getByRole("button", { name: "1 Bild senden" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Bilder sammeln und nach Discord teilen")).toBeInTheDocument();
    expect(h.aufrufe.some((a) => a.cmd === "bild_senden")).toBe(false);
    await userEvent.click(within(dialog).getByRole("button", { name: "Bilder einschalten" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "ui_state_set" && a.args?.key === "aeroacars.bilder.einwilligung.17")).toBe(true));
  });

  it("erster Start ohne Bilder: Einschalten und Anleitung wie im Entwurf", async () => {
    h.einwilligung = false;
    zeigen(false);
    expect(await screen.findByText("So kommen Bilder hierher")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bilder einschalten" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Später" })).toBeInTheDocument();
    expect(screen.getByText("Noch keine Bilder auf diesem Rechner")).toBeInTheDocument();
  });

  it("Suche und Filter: Treffer ohne Ergebnis nennt den Grund", async () => {
    vorbereiten();
    zeigen();
    await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "gibtesnicht" } });
    expect(await screen.findByText("Kein Bild passt zu Suche und Filter.")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
    await userEvent.click(screen.getByRole("button", { name: "Gesendet" }));
    expect(await screen.findByText("Kein Bild passt zu Suche und Filter.")).toBeInTheDocument();
  });

  it("Hauptaktionen stehen in der Detailspalte vor den Metadaten; ohne Kartenfaehigkeit kein Absturz", async () => {
    vorbereiten();
    zeigen();
    const detail = await screen.findByRole("complementary");
    const text = detail.textContent ?? "";
    expect(text.indexOf("An Discord senden")).toBeLessThan(text.indexOf("Position"));
    expect(text.indexOf("Löschen")).toBeLessThan(text.indexOf("Position"));
    expect(await within(detail).findByText("Die Karte kann hier nicht gezeichnet werden (WebGL fehlt).")).toBeInTheDocument();
  });

  it("Loeschen geht je Bild ueber bild_loeschen mit Bezeichnung", async () => {
    vorbereiten();
    zeigen();
    const detail = await screen.findByRole("complementary");
    await userEvent.click(within(detail).getByRole("button", { name: "Löschen" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_loeschen")).toBe(true));
    const a = h.aufrufe.find((x) => x.cmd === "bild_loeschen")!.args as { gruppe: string; id: string; bezeichnung: string };
    expect(a.id).toMatch(/^r\d$/);
    expect(a.bezeichnung).toMatch(/^Bild vom/);
  });

  it("Aufnahmehinweis: Taste aus den Einstellungen des Piloten, sonst neutral, nie eine feste Taste", async () => {
    vorbereiten();
    h.taste = { simulator: "xplane", text: "F12" };
    const { unmount } = zeigen();
    expect(await screen.findByTitle("Aufnahme: F12 in X-Plane")).toHaveTextContent("F12 in X-Plane");
    unmount();
    h.taste = null;
    zeigen();
    expect(await screen.findByText("Screenshot-Taste des Simulators")).toBeInTheDocument();
    expect(screen.queryByText(/Shift/)).toBeNull();
  });

  it("MSFS nennt die AeroACARS-Taste aus tastenkuerzel.text", async () => {
    vorbereiten();
    h.kuerzel = { text: "Alt+P", verfuegbar: true, aktiv: true, fehler: null };
    render(<BilderTab pilotId="17" pilotName="T" aktiverFlug={null} simKind="msfs2024" onKarteOeffnen={vi.fn()} />);
    expect(await screen.findByText("Aufnahme: Alt+P in MSFS")).toBeInTheDocument();
  });

  it("erster Start: Taste neutral ohne Meldung, Hinweis auf eigene Werkzeuge mit Bildquelle", async () => {
    h.einwilligung = false;
    zeigen(false);
    expect(await screen.findByText(/Eigene Werkzeuge wie Game Bar, NVIDIA oder Steam funktionieren mit jeder Taste/)).toBeInTheDocument();
    expect(screen.getByText("Im Simulator die Screenshot-Taste drücken, wie gewohnt.")).toBeInTheDocument();
  });

  it("Platz je Flug steht im Gruppenkopf (P-11.31)", async () => {
    vorbereiten();
    zeigen();
    await screen.findByText("läuft gerade");
    // zwei Bilder zu je 1 000 000 Byte im Flug P1: 2 Bilder = 1,9 MB (Gruppenkopf), 1 Bild = 977 KB (Ohne Flug)
    expect(screen.getByText("1,9 MB")).toBeInTheDocument();
    expect(screen.getByText("977 KB")).toBeInTheDocument();
  });

  it("Yoke-Taste: nur bei yoke_verfuegbar, Anzeige der Belegung, Festlegen ruft bild_taste_lernen und speichert mit yokeTaste", async () => {
    vorbereiten();
    h.kuerzel = { text: "Alt+P", verfuegbar: true, aktiv: true, fehler: null, yoke_verfuegbar: true, yoke: null } as never;
    h.lerne = { geraet_name: "Honeycomb Alpha", hersteller_id: 1, produkt_id: 2, knopf: 7 };
    render(<BilderTab pilotId="17" pilotName="T" aktiverFlug={null} simKind="msfs2024" onKarteOeffnen={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Bildquellen" }));
    expect(await screen.findByText("kein Knopf belegt")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Yoke-Taste festlegen" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_taste_lernen")).toBe(true));
    await waitFor(() => expect(h.aufrufe.find((a) => a.cmd === "bild_quellen_setzen" && a.args?.yokeTaste)).toBeTruthy());
  });

  it("Yoke-Bereich fehlt ohne yoke_verfuegbar; X-Plane bekommt den Hinweis auf sim/operation/screenshot", async () => {
    vorbereiten();
    h.kuerzel = { text: "Alt+P", verfuegbar: true, aktiv: true, fehler: null } as never;
    zeigen();
    await userEvent.click(await screen.findByRole("button", { name: "Bildquellen" }));
    expect(screen.queryByRole("button", { name: "Yoke-Taste festlegen" })).toBeNull();
    expect(await screen.findByText(/sim\/operation\/screenshot/)).toBeInTheDocument();
  });

  it("Aufraeumen: alte gesendete Bilder lassen sich zum Aufraeumen auswaehlen", async () => {
    h.zeilen = [eintrag({ id: "alt", meta: { zeit_utc: "2020-01-01T10:00:00Z", pirep_id: "P1" }, sidecar: { sendungen: [{ sendung_id: "s", zeit_ms: 1, geo_stufe: "verdeckt", status: "gesendet" }] } }), eintrag({ id: "neu", meta: { pirep_id: "P1", zeit_utc: new Date().toISOString() } })];
    zeigen();
    await userEvent.click(await screen.findByRole("button", { name: "Zum Aufräumen auswählen" }));
    expect(await screen.findByText("1 ausgewählt")).toBeInTheDocument();
  });
});

// jsdom rechnet kein CSS: die Regel selbst ist hier der Beleg, gemessen im Browser (Kachel wurde beim Ueberfahren deckend).
it("Ueberfahren einer Kachel deckt das Bild nicht ab", () => {
  const css = fs.readFileSync(path.join(__dirname, "bilder.css"), "utf-8");
  expect(css).toMatch(/\.bt-kachel__oeffnen:hover:not\(:disabled\) \{ background: rgba\(255, 255, 255, 0\.06\); \}/);
});
