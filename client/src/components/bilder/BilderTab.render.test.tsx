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
  tablet: false,
  taste: null as null | { simulator: string; text: string },
  kuerzel: null as null | { text: string; verfuegbar: boolean; aktiv: boolean; fehler: null },
  lerne: null as unknown,
  senden: null as null | ((a: Record<string, unknown>) => unknown),
  status: null as null | (() => unknown),
  fluege: [] as unknown[],
  gekuerzt: false,
  unvollstaendig: true,
  hoerer: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));

vi.mock("../../lib/ipc", () => ({
  get isTauri() { return !h.tablet; },
  formatIpcError: (e: unknown) => String(e),
  listen: async (n: string, cb: (e: { payload: unknown }) => void) => { h.hoerer.set(n, [...(h.hoerer.get(n) ?? []), cb]); return () => undefined; },
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    h.aufrufe.push({ cmd, args });
    switch (cmd) {
      case "bilder_liste":
        return { eintraege: h.zeilen, belegt_bytes: 5_000_000, quellen: [], tastenkuerzel: h.kuerzel, aufnahme_verfuegbar: false, simulator_taste: h.taste, funde: [], altbestand: [] };
      case "ui_state_get_all": return h.einwilligung ? { "aeroacars.bilder.einwilligung.17": "1" } : {};
      case "bild_lesen": return new Uint8Array([0xff, 0xd8, 0xff, 0xda, 0, 2, 0xff, 0xd9]);
      case "bild_senden": return h.senden ? h.senden(args!) : { bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } };
      case "bild_status": return h.status ? h.status() : { bilder: [], kontingent: { rest: 9, grenze: 12, retry_after: 0 } };
      case "bild_kontext": return { staende: [], spur: [], fluege: h.fluege, fluege_gekuerzt: h.gekuerzt, logbuch_unvollstaendig: h.unvollstaendig };
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
  h.tablet = false;
  h.aufrufe = []; h.zeilen = []; h.einwilligung = true; h.senden = null; h.status = null; h.fluege = []; h.gekuerzt = false; h.unvollstaendig = true; h.taste = null; h.kuerzel = null; h.lerne = null;
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

  it("ein Haken zeigt dieses Bild rechts, Detail und Leiste reden vom selben Bild", async () => {
    vorbereiten();
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await waitFor(() => expect(document.querySelector(".bt-detail__zusatz")?.textContent).toBe("nicht ausgewählt"));
    const frei = boxen.findIndex((_, i) => screen.getAllByRole("button", { name: /Details:/ })[i].getAttribute("aria-pressed") !== "true");
    await userEvent.click(boxen[frei]);
    await waitFor(() => expect(document.querySelector(".bt-detail__zusatz")?.textContent).toBe("ausgewählt"));
    expect(screen.getAllByRole("button", { name: /Details:/ })[frei].getAttribute("aria-pressed")).toBe("true");
  });

  it("am Blatt ueber der Liste oeffnet ein Haken nichts, damit die naechsten Kacheln frei bleiben", async () => {
    vorbereiten();
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(boxen[0]);
    await userEvent.click(boxen[1]);
    expect(await screen.findByText("2 ausgewählt")).toBeInTheDocument();
    expect(document.querySelector(".bt-detail__zusatz")).toBeNull();
    vi.unstubAllGlobals();
  });

  it("Abbrechen nach „Senden“ in der Detailspalte laesst keinen Haken zurueck, die Leiste behaelt ihre Auswahl", async () => {
    vorbereiten();
    zeigen();
    await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(within(document.querySelector(".bt-detail") as HTMLElement).getByRole("button", { name: /senden/i }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.queryByText("1 ausgewählt")).toBeNull();
    await userEvent.click((await screen.findAllByRole("checkbox", { name: /Auswählen/ }))[0]);
    await userEvent.click(screen.getByRole("button", { name: "1 Bild senden" }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByText("1 ausgewählt")).toBeInTheDocument();
  });

  it("die Leiste sendet die AUSWAHL, auch wenn ein anderes Bild in der Detailspalte offen ist", async () => {
    vorbereiten();
    h.senden = (a) => ({ bilder: (a.bilder as Array<{ bild_id: string }>).map((b) => ({ bild_id: b.bild_id, status: "gesendet" })), kontingent: null });
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    for (const b of [boxen[0], boxen[2]]) await userEvent.click(b);
    await userEvent.click(screen.getAllByRole("button", { name: /Details:/ })[1]); // ein Bild ohne Haken ansehen
    expect(document.querySelector(".bt-detail__zusatz")?.textContent).toBe("nicht ausgewählt");
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

  it("Server aus: schon vor dem ersten Senden in Leiste und Dialog sichtbar, Senden gesperrt", async () => {
    vorbereiten();
    h.status = () => { throw { code: "live_nicht_erreichbar", message: "x" }; };
    zeigen();
    await userEvent.click((await screen.findAllByRole("checkbox", { name: /Auswählen/ }))[0]);
    expect(await screen.findByText("Server nicht erreichbar")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "1 Bild senden" }));
    const d = within(await screen.findByRole("dialog"));
    expect(d.getByText(/Deine Auswahl bleibt/)).toBeInTheDocument();
    expect(d.getByRole("button", { name: "Jetzt senden" })).toBeDisabled();
    expect(h.aufrufe.some((a) => a.cmd === "bild_senden")).toBe(false);
  });

  it("Flug aendern bietet einen frueheren Flug ohne Galeriebild an und schreibt seine Daten", async () => {
    vorbereiten();
    h.fluege = [{ pirep_id: "ALT7", rufzeichen: "GSG700", flugnummer: "700", abflug: "LOWI", ziel: "EDDM", flugzeug_icao: "A320", start_ms: Date.UTC(2026, 9, 2), ende_ms: null }];
    zeigen(false);
    await userEvent.click(await screen.findByRole("button", { name: "Flug zuordnen" }));
    const d = within(await screen.findByRole("dialog"));
    const wahl = d.getByRole("combobox");
    await within(wahl).findByRole("option", { name: /GSG700 LOWI–EDDM/ });
    expect(d.getByText(/Logbuch nicht vollständig geladen/)).toBeInTheDocument();
    expect(h.aufrufe.find((a) => a.cmd === "bild_kontext" && a.args?.fluege)?.args).toMatchObject({ fluege: true, zeiten: ["2026-11-03T10:02:00.000Z"] });
    await userEvent.selectOptions(wahl, "ALT7");
    await userEvent.click(d.getByRole("button", { name: "Fertig" }));
    await waitFor(() => {
      const s = h.aufrufe.filter((a) => a.cmd === "bild_schreiben").pop();
      expect((s?.args?.sidecar as { meta: Record<string, unknown> } | undefined)?.meta).toMatchObject({ pirep_id: "ALT7", rufzeichen: "GSG700", abflug: "LOWI", flugzeug_icao: "A320", breite: null });
    });
  });

  it("vollstaendige Flugliste: kein Hinweis auf fehlende Fluege (Gegenprobe)", async () => {
    vorbereiten();
    h.gekuerzt = false;
    h.unvollstaendig = false;
    h.fluege = [{ pirep_id: "ALT7", rufzeichen: "GSG700", flugnummer: "700", abflug: "LOWI", ziel: "EDDM", flugzeug_icao: null, start_ms: Date.UTC(2026, 9, 2), ende_ms: null }];
    zeigen(false);
    await userEvent.click(await screen.findByRole("button", { name: "Flug zuordnen" }));
    const d = within(await screen.findByRole("dialog"));
    await within(d.getByRole("combobox")).findByRole("option", { name: /GSG700/ });
    expect(d.queryByText(/Logbuch nicht vollständig|zeitlich am nächsten/)).toBeNull();
  });

  it("gekuerzte Liste: sagt, dass nur die naechsten Fluege stehen, nicht dass das Logbuch fehlt", async () => {
    vorbereiten();
    h.gekuerzt = true;
    h.unvollstaendig = false;
    h.fluege = [{ pirep_id: "ALT7", rufzeichen: "GSG700", flugnummer: "700", abflug: "LOWI", ziel: "EDDM", flugzeug_icao: null, start_ms: Date.UTC(2026, 9, 2), ende_ms: null }];
    zeigen(false);
    await userEvent.click(await screen.findByRole("button", { name: "Flug zuordnen" }));
    const d = within(await screen.findByRole("dialog"));
    expect(await d.findByText(/nur die Flüge, die zeitlich am nächsten/)).toBeInTheDocument();
    expect(d.queryByText(/Logbuch nicht vollständig/)).toBeNull();
  });

  it("eine geschlossene Serie laesst sich als Ganzes waehlen, mit eindeutigem Teilzustand", async () => {
    h.zeilen = [0, 1, 2].map((i) => eintrag({ id: `s${i}`, meta: { zeit_utc: `2026-11-07T10:00:0${i}.000Z` } }));
    zeigen();
    const haken = await screen.findByRole("checkbox", { name: "Serie mit 3 Bildern auswählen" }) as HTMLInputElement;
    await userEvent.click(haken);
    expect(screen.getByText("3 ausgewählt")).toBeInTheDocument();
    expect(haken.checked).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: /Serie mit 3 Bildern, / }));
    await userEvent.click(screen.getAllByRole("checkbox", { name: /Auswählen/ })[0]);
    expect(screen.getByText("2 ausgewählt")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Serie schließen" }));
    const teil = await screen.findByRole("checkbox", { name: "Serie mit 3 Bildern auswählen" }) as HTMLInputElement;
    expect(teil.indeterminate).toBe(true);
    expect(teil.checked).toBe(false);
    await userEvent.click(teil);
    expect(screen.getByText("3 ausgewählt")).toBeInTheDocument();
    await userEvent.click(teil);
    expect(screen.queryByText(/ausgewählt$/, { selector: ".bt-leiste__fett" })).toBeNull();
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

  it("Papierkorb in der Leiste loescht alle gewaehlten Bilder in EINEM Aufruf, das Detail bietet dann kein Einzel-Loeschen", async () => {
    vorbereiten();
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    for (const b of [boxen[0], boxen[1], boxen[2]]) await userEvent.click(b);
    expect(within(await screen.findByRole("complementary")).queryByRole("button", { name: "Löschen" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "In den Papierkorb" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_loeschen")).toBe(true));
    const aufrufe = h.aufrufe.filter((a) => a.cmd === "bild_loeschen");
    expect(aufrufe).toHaveLength(1);
    expect((aufrufe[0].args as { bilder: unknown[] }).bilder).toHaveLength(3);
  });

  it("Loeschen geht je Bild ueber bild_loeschen mit Bezeichnung", async () => {
    vorbereiten();
    zeigen();
    const detail = await screen.findByRole("complementary");
    await userEvent.click(within(detail).getByRole("button", { name: "Löschen" }));
    await waitFor(() => expect(h.aufrufe.some((a) => a.cmd === "bild_loeschen")).toBe(true));
    const a = h.aufrufe.find((x) => x.cmd === "bild_loeschen")!.args as { bilder: Array<{ gruppe: string; id: string }>; bezeichnung: string };
    expect(a.bilder).toHaveLength(1);
    expect(a.bilder[0].id).toMatch(/^r\d$/);
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

  it("Kopf nennt die Zahlen ohne doppelte Woerter, ein einzeln gewaehltes Bild zeigt seine Details", async () => {
    vorbereiten();
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    expect(document.querySelector(".bt-kopf__zahlen")?.textContent).not.toMatch(/Bilder Bilder|Flüge Flüge|Flug Flüge/);
    await userEvent.click(screen.getByRole("button", { name: "Detail schließen", hidden: true }));
    expect(await screen.findByText(/Wähle ein Bild/)).toBeInTheDocument();
    await userEvent.click(boxen[0]);
    await waitFor(() => expect(document.querySelector(".bt-detail__zusatz")?.textContent).toBe("ausgewählt"));
    await userEvent.click(screen.getByRole("button", { name: "Detail schließen", hidden: true }));
    expect(await screen.findByText(/Wähle ein Bild/)).toBeInTheDocument();
    expect(screen.getByText("1 ausgewählt")).toBeInTheDocument();
  });
  it("am Tablet fehlen die Aufgaben des Sim-PCs: Hinzufuegen, Quellen, Ordner, Export", async () => {
    vorbereiten();
    h.tablet = true;
    zeigen();
    const boxen = await screen.findAllByRole("checkbox", { name: /Auswählen/ });
    await userEvent.click(boxen[0]);
    expect(screen.getByRole("button", { name: "1 Bild senden" })).toBeInTheDocument();
    for (const name of [/Bild hinzufügen/, /Bildquellen/, /Ordner/, /Als ZIP/]) expect(screen.queryByRole("button", { name })).toBeNull();
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
