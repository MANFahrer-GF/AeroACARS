// Prüfungen für den Pflicht-Riegel (v1.7.6).
//
// Der Riegel sperrt die ganze Oberfläche. Das Risiko liegt deshalb
// nicht darin, dass er ausbleibt — sondern darin, dass er im falschen
// Moment zufällt. Entsprechend prüft die Mehrzahl der Fälle, wann er
// NICHT erscheinen darf:
//
//   * kein Netz / kein Update      → keine Sperre
//   * Update erst mitten drin      → keine Sperre (Startfenster)
//   * laufender Flug               → keine Sperre
//   * abgeschaltet über den Notaus → keine Sperre
//
// Dazu die beiden Fälle, in denen er etwas tun MUSS: sperren, wenn er
// scharf ist, und einen Ausweg zeigen, sobald die Installation
// scheitert. Ohne diesen Ausweg wäre ein Client mit klemmendem Updater
// dauerhaft unbenutzbar.

import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { UpdateGate } from "./UpdateGate";
import type { UseUpdateCheckerResult } from "../hooks/useUpdateChecker";
import type { FlightPhase } from "../types";
import deCommon from "../locales/de/common.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      fallbackLng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

beforeEach(() => {
  localStorage.clear();
});

const installieren = vi.fn();

function checker(
  ueber: Partial<UseUpdateCheckerResult> = {},
): UseUpdateCheckerResult {
  return {
    update: { version: "1.7.7", body: "" } as UseUpdateCheckerResult["update"],
    stage: "fresh",
    installing: false,
    progress: null,
    snoozeBanner: vi.fn(),
    bannerSnoozed: false,
    installAndRelaunch: installieren,
    pflichtUpdate: true,
    installationGescheitert: false,
    ...ueber,
  };
}

const riegel = () => document.querySelector(".update-gate");

describe("Pflicht-Riegel: wann er sperrt", () => {
  it("sperrt, wenn beim Start eine neuere Version vorlag", () => {
    render(<UpdateGate checker={checker()} activePhase={null} />);
    expect(riegel()).not.toBeNull();
    expect(screen.getByText(/Update durchführen/)).toBeTruthy();
    // Die Version gehört in die Ansage — sonst weiss der Pilot nicht,
    // worauf er aktualisiert.
    expect(document.body.textContent).toContain("1.7.7");
  });

  it("löst die Installation aus", () => {
    installieren.mockClear();
    render(<UpdateGate checker={checker()} activePhase={null} />);
    fireEvent.click(screen.getByText(/Update durchführen/));
    expect(installieren).toHaveBeenCalledTimes(1);
  });
});

describe("Pflicht-Riegel: wann er sich zurückhält", () => {
  it("bleibt aus, wenn gar kein Update vorliegt", () => {
    render(
      <UpdateGate checker={checker({ update: null })} activePhase={null} />,
    );
    expect(riegel()).toBeNull();
  });

  it("bleibt aus, wenn das Update erst mitten in der Sitzung kam", () => {
    // `update` ist da, aber nicht startbezogen — der Vier-Stunden-Turnus
    // darf niemanden aussperren.
    render(
      <UpdateGate
        checker={checker({ pflichtUpdate: false })}
        activePhase={null}
      />,
    );
    expect(riegel()).toBeNull();
  });

  it("bleibt in JEDER aktiven Flugphase aus", () => {
    const phasen: FlightPhase[] = [
      "pushback",
      "taxi_out",
      "takeoff_roll",
      "takeoff",
      "climb",
      "cruise",
      "holding",
      "descent",
      "approach",
      "final",
      "landing",
      "taxi_in",
      "blocks_on",
    ] as FlightPhase[];
    for (const p of phasen) {
      const { unmount } = render(
        <UpdateGate checker={checker()} activePhase={p} />,
      );
      expect(riegel(), `Riegel sperrte in Phase ${p}`).toBeNull();
      unmount();
    }
  });

  it("bleibt aus, wenn der Betreiber-Notaus gesetzt ist", () => {
    localStorage.setItem("aeroacars.update.gate_off", "1");
    render(<UpdateGate checker={checker()} activePhase={null} />);
    expect(riegel()).toBeNull();
  });

  it("bleibt aus für die Version, an der die Installation scheiterte", () => {
    localStorage.setItem("aeroacars.update.gate_skip_version", "1.7.7");
    render(<UpdateGate checker={checker()} activePhase={null} />);
    expect(riegel()).toBeNull();
  });

  it("greift wieder, sobald eine NEUERE Version erscheint", () => {
    // Der Kern der Sache: Ein Fehlschlag darf den Riegel nicht dauerhaft
    // ausbauen. Der Ausweg gilt nur fuer die Version, die nicht durchkam.
    localStorage.setItem("aeroacars.update.gate_skip_version", "1.7.7");
    render(
      <UpdateGate
        checker={checker({
          update: { version: "1.7.8", body: "" } as UseUpdateCheckerResult["update"],
        })}
        activePhase={null}
      />,
    );
    expect(riegel()).not.toBeNull();
  });
});

describe("Pflicht-Riegel: der Ausweg", () => {
  it("zeigt VOR einem Fehlschlag keinen Weg vorbei", () => {
    render(<UpdateGate checker={checker()} activePhase={null} />);
    expect(document.querySelector(".update-gate__continue")).toBeNull();
    // Und auch kein „Später" aus dem Banner-Wortschatz.
    expect(document.body.textContent).not.toContain("Später");
  });

  it("zeigt NACH einem Fehlschlag einen Weg vorbei", () => {
    render(
      <UpdateGate
        checker={checker({ installationGescheitert: true })}
        activePhase={null}
      />,
    );
    expect(document.querySelector(".update-gate__continue")).not.toBeNull();
    expect(screen.getByText(/Trotzdem fortfahren/)).toBeTruthy();
  });

  it("merkt sich beim Ausweg die VERSION, nicht ein pauschales Aus", () => {
    // Sonst schaltet ein einziger Fehlschlag den Riegel fuer immer ab.
    const nachladen = vi.fn();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...window.location, reload: nachladen },
    });
    render(
      <UpdateGate
        checker={checker({ installationGescheitert: true })}
        activePhase={null}
      />,
    );
    fireEvent.click(screen.getByText(/Trotzdem fortfahren/));
    expect(localStorage.getItem("aeroacars.update.gate_skip_version")).toBe("1.7.7");
    expect(localStorage.getItem("aeroacars.update.gate_off")).toBeNull();
    expect(nachladen).toHaveBeenCalled();
  });
});

describe("Pflicht-Riegel: Verdrahtung", () => {
  // Diese beiden prüfen NICHT das Verhalten, sondern dass die Teile
  // überhaupt zusammenhängen. Genau solche Lücken sind heute mehrfach
  // aufgefallen: ein Feld, das gebaut wird und nirgends ankommt.

  it("ist in App.tsx eingehängt", () => {
    const app = readFileSync(resolve(__dirname, "../App.tsx"), "utf-8");
    expect(app).toContain("UpdateGate");
    expect(app).toMatch(/<UpdateGate[\s\S]{0,200}activePhase=/);
  });

  it("kennt dieselben Flugphasen wie das Banner", () => {
    // Die Liste steht bewusst zweimal (siehe UpdateGate.tsx). Wer eine
    // Phase nur an einer Stelle ergänzt, würde den Riegel dort sperren
    // lassen, wo das Banner schon schweigt — und das fiele erst einem
    // Piloten im Cockpit auf.
    const phasen = (datei: string) => {
      const t = readFileSync(resolve(__dirname, datei), "utf-8");
      const block = t.slice(t.indexOf("Set(["), t.indexOf("] as FlightPhase[])"));
      return [...block.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]).sort();
    };
    expect(phasen("./UpdateGate.tsx")).toEqual(phasen("./UpdateBanner.tsx"));
  });
});
