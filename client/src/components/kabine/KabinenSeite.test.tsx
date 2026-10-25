// Seite „Kabine“ (beta.5): Thomas im Flug AIB 436 — „die anderen stehen
// immer noch auf wird vorbereitet. Man kann sich die auch nicht mehr
// anklicken oder ansehen oder nochmal abspielen.“ Festgehalten am
// gerenderten Bild: jede Zeile nennt Zeitpunkt und echten Stand, klappt mit
// allen Sprachen auf und lässt sich anhören.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";

const jetzt = 1_800_000_000;
const ueberblick = {
  verlauf: [
    { id: 4, slot: "anschnallzeichen_aus", rolle: "kabine", zeit: jetzt - 3600, vorschau: false, ergebnis: "abgebrochen", gestartet: true, grund: null,
      teile: [
        { sprache: "fr-FR", text: "Le signal est éteint.", datei: "f/a.wav", dauer_s: 3 },
        { sprache: "de-DE", text: "Das Anschnallzeichen ist aus.", datei: "f/b.wav", dauer_s: 3 },
      ] },
    { id: 5, slot: "reiseflug25", rolle: "kabine", zeit: jetzt - 1800, vorschau: false, ergebnis: "entfallen", gestartet: false, grund: "ton_nicht_fertig", teile: [] },
    { id: 6, slot: "kapitaen_begruessung", rolle: "kapitaen", zeit: jetzt - 10, vorschau: false, ergebnis: null, gestartet: true, grund: null,
      teile: [{ sprache: "de-DE", text: "Hier spricht Ihr Kapitän.", datei: "f/c.wav", dauer_s: 3 }] },
  ],
  geplant: [
    { slot: "reiseflug50", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 600, stand: { art: "bereit" },
      teile: [
        { sprache: "fr-FR", text: "Nous avons passé la moitié du vol.", ton: true, keine_stimme: false },
        { sprache: "de-DE", text: "Wir haben die Hälfte geschafft.", ton: true, keine_stimme: false },
      ] },
    { slot: "kapitaen_sinkflug", rolle: "kapitaen", faellig: false, wartet: null, wann: jetzt + 5000, stand: { art: "text_ab", um: jetzt + 4000 },
      teile: [
        { sprache: "fr-FR", text: null, ton: false, keine_stimme: false },
        { sprache: "de-DE", text: null, ton: false, keine_stimme: false },
      ] },
    { slot: "anschnallzeichen_aus", rolle: "kabine", faellig: false, wartet: "anschnallzeichen", wann: null, stand: { art: "bereit" },
      teile: [{ sprache: "de-DE", text: "Das Anschnallzeichen ist aus.", ton: true, keine_stimme: false }] },
    { slot: "kommando_plaetze_landung", rolle: "kommando", faellig: false, wartet: null, wann: null,
      stand: { art: "ton_wartet", um: jetzt + 30, meldung: "Piper-Server antwortet 503" },
      teile: [{ sprache: "en-GB", text: "Cabin crew, take your seats for landing.", ton: false, keine_stimme: false }] },
  ],
  sprachen: ["fr-FR", "de-DE"],
  beta: true,
  grund: null,
  ablauf: { freigabe: { art: "ja" }, abflug: { art: "fertig" }, halbzeit: { art: "offen", um: jetzt + 600 }, ankunft: { art: "offen", um: jetzt + 4000 }, ton_fertig: 1, ton_gesamt: 3 },
  profil: { flugart: "linie", muster: "A388", flugzeit_min: 304, kurzstrecke: false, flugzeit_bekannt: true, sprachen_ohne_stimme: [] },
  belegt: false,
  stumm: false,
  lautstaerke: 0.8,
};

const aufrufe: { cmd: string; args?: Record<string, unknown> }[] = [];
// Abweichungen vom Überblick für einen einzelnen Test.
let anders: Record<string, unknown> = {};
vi.mock("../../lib/ipc", () => ({
  isTauri: true,
  formatIpcError: (e: unknown) => String(e),
  listen: vi.fn(async () => () => undefined),
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    aufrufe.push({ cmd, args });
    if (cmd === "kabinen_ueberblick") return { ...ueberblick, ...anders };
    if (cmd === "kabinen_stumm") ueberblick.stumm = Boolean(args?.stumm);
    if (cmd === "kabinen_anhoeren")
      return { id: 99, slot: args?.slot, rolle: "kapitaen", zeit: jetzt, vorschau: true, ergebnis: null,
        teile: [{ sprache: "de-DE", text: "Wir haben die Hälfte geschafft.", datei: "f/b.wav", dauer_s: 1 }] };
    if (cmd === "kabinen_ton_huellkurve") return [];
    return null;
  }),
}));
vi.stubGlobal("requestAnimationFrame", (f: () => void) => setTimeout(f, 5));

import { KabinenSeite } from "./KabinenSeite";

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "de",
    resources: { de: { common: deCommon } },
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
});
afterEach(() => {
  cleanup();
  aufrufe.length = 0;
  anders = {};
});

const uhr = (e: number) => new Date(e * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

describe("Seite Kabine", () => {
  // Haken „keine Kabinenansagen für diesen Flug“ im Loadsheet: eigener
  // Hinweis, nicht „Für dein Konto gesperrt“ (der Server sendet beta: false).
  it("nennt den im Loadsheet abgeschalteten Flug, nicht ein gesperrtes Konto", async () => {
    anders = {
      beta: null,
      sprachen: [],
      grund: { grund: "flug_aus" },
      ablauf: { ...ueberblick.ablauf, freigabe: { art: "flug_aus" } },
    };
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const grund = await screen.findByTestId("kabine-grund");
    expect(grund.textContent).toContain("Für diesen Flug im Loadsheet abgeschaltet.");
    expect(screen.getByTestId("kabine-ablauf").textContent).toContain("für diesen Flug im Loadsheet abgeschaltet");
    expect(document.body.textContent).not.toMatch(/Für dein Konto gesperrt|nicht freigeschaltet|kabine\./);
  });

  it("Gegenprobe: ein gesperrtes Konto heißt weiter „gesperrt“", async () => {
    anders = {
      beta: false,
      sprachen: [],
      grund: { grund: "nicht_freigeschaltet" },
      ablauf: { ...ueberblick.ablauf, freigabe: { art: "nein" } },
    };
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const grund = await screen.findByTestId("kabine-grund");
    expect(grund.textContent).toContain("für dein Konto gesperrt");
    expect(document.body.textContent).not.toMatch(/Loadsheet/);
  });

  it("nennt je Ansage Zeitpunkt und echten Stand statt „wird vorbereitet“", async () => {
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const liste = await screen.findByTestId("kabine-naechste");
    expect(liste.textContent).not.toMatch(/wird vorbereitet/);
    expect(within(liste).getByText(`Text ab ca. ${uhr(jetzt + 4000)}`)).toBeTruthy();
    // Deutlich (Thomas 18.10.2026): wann sie spielt, und rechts, ob sie fertig ist.
    expect(within(liste).getByText(`Spielt ca. ${uhr(jetzt + 600)}`)).toBeTruthy();
    expect(within(liste).getAllByText("Text und Ton fertig").length).toBeGreaterThan(0);
    // Wartet auf den Piloten (Anschnallzeichen): kein „Spielt …“ davor (Advisor 2.0.27).
    expect(within(liste).getByText("wartet, bis du das Anschnallzeichen ausschaltest")).toBeTruthy();
    expect(liste.textContent).not.toMatch(/Spielt wartet/);
    expect(within(liste).getByText(/Ton wartet \(Server ausgelastet\), neuer Versuch/)).toBeTruthy();
    // Ablauf oben und „Warum diese Ansagen“.
    expect(screen.getByTestId("kabine-ablauf").textContent).toContain("1 von 3 fertig");
    // 2.1.1 Zusatz H: Die Halbzeit hat ihren eigenen Teil.
    expect(screen.getByTestId("kabine-ablauf").textContent).toContain("Halbzeit:");
    expect(screen.getByTestId("kabine-profil").textContent).toContain("A388");
  });

  it("klappt mit allen Sprachen auf und spielt zum Anhören, ohne es als gespielt zu zählen", async () => {
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: /Halbzeit/ }));
    expect(screen.getByText("Nous avons passé la moitié du vol.")).toBeTruthy();
    expect(screen.getByText("Wir haben die Hälfte geschafft.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Anhören" }));
    await vi.waitFor(() => expect(aufrufe.some((a) => a.cmd === "kabinen_ton_spielen")).toBe(true));
    expect(aufrufe.find((a) => a.cmd === "kabinen_anhoeren")?.args).toEqual({ slot: "reiseflug50", id: null });
    // Ende des Vorhörens geht an Rust (gibt die Warteschlange frei).
    await vi.waitFor(() => expect(aufrufe.find((a) => a.cmd === "kabinen_wiedergabe_ende")?.args).toEqual({ id: 99, ergebnis: "gespielt" }));
  });

  it("„Bisher“ zeigt den Ausgang, alle Sprachen und „Nochmal anhören“", async () => {
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const bisher = await screen.findByTestId("kabine-bisher");
    expect(within(bisher).getByText("abgebrochen")).toBeTruthy();
    fireEvent.click(within(bisher).getByRole("button", { name: /Anschnallzeichen aus/ }));
    expect(within(bisher).getByText("Le signal est éteint.")).toBeTruthy();
    expect(within(bisher).getByText("Das Anschnallzeichen ist aus.")).toBeTruthy();
    // Läuft noch etwas (Vorhören aus dem Test davor), ist der Knopf gesperrt.
    const nochmal = within(bisher).getByRole("button", { name: "Nochmal anhören" }) as HTMLButtonElement;
    await vi.waitFor(() => expect(nochmal.disabled).toBe(false));
    fireEvent.click(nochmal);
    await vi.waitFor(() => expect(aufrufe.find((a) => a.cmd === "kabinen_anhoeren")?.args).toEqual({ slot: "anschnallzeichen_aus", id: 4 }));
  });

  it("„Bisher“: läuft erst nach dem Start, Entfallenes mit Grund und ohne Anhören", async () => {
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const bisher = await screen.findByTestId("kabine-bisher");
    expect(within(bisher).getByText("läuft")).toBeTruthy();
    expect(within(bisher).getByText("entfallen: Ton nicht rechtzeitig fertig")).toBeTruthy();
    fireEvent.click(within(bisher).getByRole("button", { name: /Service beginnt/ }));
    expect(within(bisher).queryByRole("button", { name: "Nochmal anhören" })).toBeNull();
  });

  it("Ton oben: Stumm schaltet sofort und sagt es, der Regler setzt die Lautstärke", async () => {
    ueberblick.stumm = false;
    render(<KabinenSeite onOpenSettings={() => undefined} />);
    const ton = await screen.findByTestId("kabine-ton");
    fireEvent.click(within(ton).getByRole("button", { name: /Ansagen/ }));
    await vi.waitFor(() => expect(aufrufe.find((a) => a.cmd === "kabinen_stumm")?.args).toEqual({ stumm: true }));
    expect(await screen.findByTestId("kabine-stumm-hinweis")).toBeTruthy();
    expect(within(ton).getByRole("button", { name: /Stumm/ }).getAttribute("aria-pressed")).toBe("true");
    const regler = within(ton).getByRole("slider");
    fireEvent.change(regler, { target: { value: "40" } });
    fireEvent.pointerUp(regler);
    await vi.waitFor(() => expect(aufrufe.filter((a) => a.cmd === "kabinen_lautstaerke").map((a) => a.args)).toContainEqual({ lautstaerke: 0.4, speichern: true }));
    // Aufheben: Hinweis weg.
    fireEvent.click(within(ton).getByRole("button", { name: /Stumm/ }));
    await vi.waitFor(() => expect(screen.queryByTestId("kabine-stumm-hinweis")).toBeNull());
    ueberblick.stumm = false;
  });
});
