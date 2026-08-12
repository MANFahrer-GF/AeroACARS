// Die Integritätsmeldung im Cockpit.
//
// Feldbefund vom 12.08.2026 (Screenshot aus einem laufenden Flug): Im roten
// Kasten stand "FUEL_RATE_IMPOSSIBLE in Phase BOARDING" — ein harmloser
// Hinweis — obwohl die Sitzung wegen 81 kritischer Höhenmeldungen rot war.
// Darunter die Drohung, der Flugbericht werde "wahrscheinlich als
// 'untrusted' eingestuft und für VA-Admin-Review markiert".
//
// Beides falsch, und beides festgenagelt:
//   1. Angezeigt wird der SCHWERSTE Fall, nicht der zuletzt eingegangene.
//   2. Der Text droht keine Folge an, die es nicht gibt. Seit v0.13.4 des
//      Servers führt kein Integritäts-Merkmal automatisch zu einer Prüfung
//      (scoreTrust.ts: nur fehlende Landung oder Sim-Absturz-Signatur).
//      Eine Warnung, die sich als falsch erweist, wird beim nächsten Mal
//      nicht mehr gelesen.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

type Melder = (e: { payload: unknown }) => void;
let melde: Melder | null = null;

vi.mock("../lib/ipc", () => ({
  listen: async (_name: string, cb: Melder) => {
    melde = cb;
    return () => { melde = null; };
  },
}));

import { IntegrityBanner } from "./IntegrityBanner";

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

afterEach(() => { cleanup(); melde = null; });

function flag(art: string, phase: string, schwere: string, sitzung = schwere, anzahl?: number, sitzungId = 1) {
  return {
    session_id: sitzungId,
    session_effective_severity: sitzung,
    flag: {
      type: art, phase, base_severity: schwere, effective_severity: schwere,
      ts: Date.now(), detail: {}, mode: "continuous", detector: "test",
      ...(anzahl != null ? { anzahl } : {}),
    },
  };
}

async function melden(...pakete: unknown[]) {
  for (const p of pakete) {
    await act(async () => { melde?.({ payload: p }); });
  }
}

describe("Integritätsmeldung", () => {
  it("zeigt den schwersten Fall, nicht den zuletzt eingegangenen", async () => {
    render(<IntegrityBanner />);
    await melden(
      flag("GROUND_ELEVATION_MISMATCH", "BOARDING", "critical"),
      flag("FUEL_RATE_IMPOSSIBLE", "BOARDING", "info", "critical"),
    );
    const text = document.body.textContent ?? "";
    expect(text).toContain(deCommon.integrity.flag_type.GROUND_ELEVATION_MISMATCH);
    expect(text).not.toContain(deCommon.integrity.flag_type.FUEL_RATE_IMPOSSIBLE);
  });

  it("droht nicht mehr mit einer Prüfung, die gar nicht kommt", async () => {
    render(<IntegrityBanner />);
    await melden(flag("SIM_STATE_RESET_SIGNATURE", "CRUISE", "critical"));
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/untrusted/i);
    expect(text).not.toMatch(/Admin-Review/i);
    expect(text).toContain("wird normal gewertet");
  });

  it("spricht Klartext statt Maschinenbezeichner", async () => {
    render(<IntegrityBanner />);
    await melden(flag("POSITION_DELTA_EXCESSIVE", "CLIMB", "anomaly"));
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("POSITION_DELTA_EXCESSIVE");
    expect(text).not.toContain("CLIMB");
    expect(text).toContain(deCommon.integrity.flag_type.POSITION_DELTA_EXCESSIVE);
  });

  it("zählt, wie oft es auftrat — nicht wie viele Ereignisse ankamen", async () => {
    render(<IntegrityBanner />);
    // Ein Ereignis, das der Server bereits siebenfach zusammengefasst hat.
    await melden(flag("TELEMETRY_GAP_SHORT", "CRUISE", "anomaly", "anomaly", 7));
    expect(document.body.textContent).toContain("7-mal");
  });

  // Der Hook sammelt über die Laufzeit der App, nicht über den Flug. Ohne
  // Reset stünde beim zweiten Flug die Meldung des ersten noch da — und
  // seit dem Umbau auf "zeige den schwersten Fall" hätte der alte Fall den
  // aktuellen überstrahlt.
  it("fängt bei einem neuen Flug von vorn an", async () => {
    render(<IntegrityBanner />);
    await melden(flag("SIM_STATE_RESET_SIGNATURE", "CRUISE", "critical", "critical", undefined, 1));
    expect(document.body.textContent).toContain(
      deCommon.integrity.flag_type.SIM_STATE_RESET_SIGNATURE,
    );

    // Neuer Flug, neue Sitzungskennung, harmlosere Meldung.
    await melden(flag("TELEMETRY_GAP_SHORT", "CLIMB", "anomaly", "anomaly", undefined, 2));
    const text = document.body.textContent ?? "";
    expect(text).not.toContain(deCommon.integrity.flag_type.SIM_STATE_RESET_SIGNATURE);
    expect(text).toContain(deCommon.integrity.flag_type.TELEMETRY_GAP_SHORT);
  });

  it("zählt den Zähler beim neuen Flug ebenfalls zurück", async () => {
    render(<IntegrityBanner />);
    await melden(
      flag("TELEMETRY_GAP_SHORT", "CRUISE", "anomaly", "anomaly", 5, 1),
      flag("TELEMETRY_GAP_SHORT", "CRUISE", "anomaly", "anomaly", 2, 2),
    );
    expect(document.body.textContent).not.toContain("7-mal");
    expect(document.body.textContent).toContain("2-mal");
  });

  it("bleibt bei einem reinen Hinweis ganz weg", async () => {
    render(<IntegrityBanner />);
    await melden(flag("FUEL_RATE_IMPOSSIBLE", "BOARDING", "info"));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
