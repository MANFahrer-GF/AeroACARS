import { describe, it, expect, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { fmtNm } from "./LogbookView";
import { FlightProfile } from "./FlightProfile";

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

// Live-Befund Thomas K.: AGL- und Gelände-Linie im Höhenprofil "fehlen".
//
// Zwei Fehldiagnosen liegen hinter uns, beide festgehalten, damit sie nicht
// ein drittes Mal passieren:
//
//  1. "Reine SVG-Zeichenreihenfolge, MSL überdeckt die anderen." Falsch — die
//     damalige StratosLogbook-API lieferte pro Trackpunkt nur EIN Höhenfeld
//     und nie `agl_ft`. AGL fiel im Client auf `?? 0` (flache Linie am Boden),
//     Gelände auf `alt - 0 = alt` (deckungsgleich mit MSL). Behoben durch die
//     eigene GsgLogbook-API.
//  2. Danach lagen die echten Kurven zwar vor, aber alle drei in EINER
//     40.000-ft-Skala. Über flachem Land trennen MSL und AGL nur ein paar
//     hundert Fuß — unter einem Pixel. Wieder sah man eine Linie. Das ist der
//     Doppelachsen-Fehler; behoben durch getrennte Bahnen mit gemeinsamer
//     Zeitachse.
describe("FlightProfile", () => {
  const flight = [
    { t: 0, alt_ft: 500, agl_ft: 20, gnd_ft: 480, spd_kt: 0, ias_kt: 0, vs_fpm: 0, fuel: 8000 },
    { t: 600000, alt_ft: 20000, agl_ft: 19000, gnd_ft: 1000, spd_kt: 320, ias_kt: 280, vs_fpm: 2200, fuel: 7200 },
    { t: 1200000, alt_ft: 36000, agl_ft: 34000, gnd_ft: 2000, spd_kt: 460, ias_kt: 270, vs_fpm: 0, fuel: 6000 },
    { t: 1800000, alt_ft: 3000, agl_ft: 900, gnd_ft: 2100, spd_kt: 210, ias_kt: 180, vs_fpm: -1400, fuel: 5200 },
  ];

  it("gives every measure its own band instead of one shared scale", () => {
    render(<FlightProfile route={flight} />);
    for (const title of [
      "Höhe über Meer",
      "Höhe über Grund",
      "Geschwindigkeit",
      "Steig- und Sinkrate",
      "Treibstoff an Bord",
    ]) {
      expect(screen.getByText(title), `Bahn "${title}" fehlt`).toBeInTheDocument();
    }
  });

  // Der eigentliche Regressionsschutz: ohne AGL darf keine Bahn erscheinen,
  // die eine Höhe über Grund behauptet, die wir nicht kennen.
  it("omits the AGL band entirely when no AGL was delivered", () => {
    render(<FlightProfile route={[{ t: 0, alt_ft: 1000 }, { t: 1000, alt_ft: 30000 }]} />);
    expect(screen.getByText("Höhe über Meer")).toBeInTheDocument();
    expect(screen.queryByText("Höhe über Grund")).not.toBeInTheDocument();
  });

  // Genauso für alles andere — eine leere Achse mit Titel "Treibstoff"
  // behauptet, es gäbe nichts zu tanken.
  it("omits speed, vertical-speed and fuel bands when those are absent", () => {
    render(<FlightProfile route={[{ t: 0, alt_ft: 1000, agl_ft: 500 }, { t: 1000, alt_ft: 3000, agl_ft: 900 }]} />);
    expect(screen.queryByText("Geschwindigkeit")).not.toBeInTheDocument();
    expect(screen.queryByText("Steig- und Sinkrate")).not.toBeInTheDocument();
    expect(screen.queryByText("Treibstoff an Bord")).not.toBeInTheDocument();
  });

  it("shows a placeholder when there is nothing to plot", () => {
    const { container } = render(<FlightProfile route={[{ alt_ft: 100 }]} />);
    expect(container.textContent).toContain("Keine Höhendaten");
  });

  // Ein Buschflug auf 3000 ft klebte vorher als flacher Strich am unteren
  // Rand einer festen 10.000-ft-Achse.
  it("scales each band to its own data", () => {
    const { container } = render(
      <FlightProfile route={[
        { t: 0, alt_ft: 500, agl_ft: 100 },
        { t: 60000, alt_ft: 2800, agl_ft: 900 },
      ]} />,
    );
    // Feine Achse statt 10k-Sprüngen: irgendeine Beschriftung unter 1000 ft.
    const labels = [...container.querySelectorAll(".aa-fp-ylabs span")].map((e) => e.textContent);
    expect(labels.some((l) => l === "500" || l === "1k"), `Achsenwerte: ${labels.join(",")}`).toBe(true);
  });

  it("breaks a trace at gaps rather than dragging it through the floor", () => {
    const { container } = render(
      <FlightProfile route={[
        { t: 0, alt_ft: 10000, agl_ft: 9000 },
        { t: 1000, alt_ft: 11000, agl_ft: 10000 },
        { t: 2000, alt_ft: 12000 },
        { t: 3000, alt_ft: 13000, agl_ft: 12000 },
        { t: 4000, alt_ft: 14000, agl_ft: 13000 },
      ]} />,
    );
    // Die AGL-Bahn muss aus zwei Teilstücken bestehen, nicht aus einem.
    const bands = [...container.querySelectorAll(".aa-fp-band")];
    const aglBand = bands.find((b) => b.textContent?.includes("Höhe über Grund"))!;
    expect(aglBand.querySelectorAll("polyline").length).toBe(2);
  });

  // Die Geländesilhouette ist Kontext, keine Serie — sie darf die MSL-Linie
  // nicht verdecken, deshalb Fläche statt Linie und deutlich transparenter.
  it("draws terrain as a filled silhouette under the MSL trace", () => {
    const { container } = render(<FlightProfile route={flight} />);
    const altBand = [...container.querySelectorAll(".aa-fp-band")]
      .find((b) => b.textContent?.includes("Höhe über Meer"))!;
    expect(altBand.querySelector("polygon"), "Gelände fehlt als Fläche").toBeTruthy();
  });
});

// Ein frischer Pilot mit 300 geflogenen Meilen bekam "0k nm" auf die Kachel.
describe("fmtNm", () => {
  it("does not round a real distance down to zero", () => {
    expect(fmtNm(300)).toBe("300");
    expect(fmtNm(999)).toBe("999");
  });

  it("switches to thousands only once that reads better", () => {
    expect(fmtNm(1500)).toBe("1.5k");
    expect(fmtNm(42000)).toBe("42k");
  });

  it("shows a dash when there is nothing to show", () => {
    expect(fmtNm(undefined)).toBe("—");
  });
});
