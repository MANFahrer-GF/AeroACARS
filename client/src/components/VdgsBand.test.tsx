// Das VDGS-Band.
//
// Drei Dinge sind hier nicht Kosmetik:
//   1. Der Tageswechsel. Eine TSAT um 23:58 liegt um 00:03 fünf Minuten
//      ZURÜCK — nicht 1435 Minuten voraus. Wer das falsch rechnet, zeigt
//      im Cockpit einen Countdown über einen ganzen Tag.
//   2. Das Band erscheint nur mit Eintrag. Kein leeres Gerät im Cockpit.
//   3. Ohne laufende Abflugfolge wird die fremde Seite gar nicht erst
//      gefragt — Etikette gegenüber einem Dienst, den wir nicht bezahlen.

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_k: string, fallback?: unknown, opts?: { n?: number }) =>
      typeof fallback === "string"
        ? fallback.replace("{{n}}", String(opts?.n ?? ""))
        : _k,
  }),
}));

const invoke = vi.fn();
vi.mock("../lib/ipc", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import {
  VdgsPlatte,
  ampel,
  minutenBis,
  useVdgsStand,
  type VdgsStand,
} from "./VdgsBand";

const STAND: VdgsStand = {
  callsign: "GSG421",
  departure: "LEBL",
  eobt: "15:50",
  tobt: "15:44",
  tsat: "15:46",
  ctot: "",
  taxi_min: 9,
  cdm_sts: "COMPLY",
  regulierung: "",
  rwy_sid: "24L/OLOXO3Q",
};

const um = (hhmm: string) => new Date(`2026-09-20T${hhmm}:00Z`);

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("minutenBis", () => {
  it("rechnet innerhalb desselben Tages", () => {
    expect(minutenBis("15:46", um("15:42"))).toBe(4);
    expect(minutenBis("15:42", um("15:46"))).toBe(-4);
  });

  it("dreht über Mitternacht auf die kurze Seite", () => {
    // Ohne die Drehung käme hier 1435 heraus.
    expect(minutenBis("23:58", um("00:03"))).toBe(-5);
    expect(minutenBis("00:03", um("23:58"))).toBe(5);
  });

  it("verschweigt, was keine Uhrzeit ist", () => {
    expect(minutenBis("", um("12:00"))).toBeNull();
    expect(minutenBis("--:--", um("12:00"))).toBeNull();
    expect(minutenBis("25:00", um("12:00"))).toBeNull();
    expect(minutenBis("1546", um("12:00"))).toBeNull();
  });
});

describe("ampel", () => {
  it("wird grün, wenn die TSAT in Reichweite ist", () => {
    expect(ampel(STAND, um("15:43"))).toBe("frei");
  });

  it("bleibt bernstein, solange die TSAT weit weg ist", () => {
    // Gegenprobe zur Zeile darüber: gleiche Daten, nur andere Uhrzeit.
    expect(ampel(STAND, um("15:00"))).toBe("warten");
  });

  it("wird rot bei Suspend und bei FLS-NRA", () => {
    expect(ampel({ ...STAND, cdm_sts: "SUSPENDED" }, um("15:43"))).toBe(
      "achtung",
    );
    expect(ampel({ ...STAND, cdm_sts: "FLS-NRA" }, um("15:43"))).toBe(
      "achtung",
    );
  });

  it("wird ohne bestätigte TOBT nicht grün", () => {
    expect(ampel({ ...STAND, tobt: "" }, um("15:43"))).toBe("warten");
  });
});

describe("VdgsPlatte", () => {
  it("zeigt nichts ohne Eintrag", () => {
    const { container } = render(<VdgsPlatte stand={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("zeigt Rufzeichen, Platz, Bahn und die Zeiten", () => {
    render(<VdgsPlatte stand={STAND} />);
    const band = screen.getByTestId("vdgs-band");
    expect(band.textContent).toContain("GSG421");
    expect(band.textContent).toContain("LEBL");
    expect(band.textContent).toContain("24L/OLOXO3Q");
    expect(band.textContent).toContain("15:50");
    expect(band.textContent).toContain("15:44");
    expect(band.textContent).toContain("15:46");
    expect(band.textContent).toContain("COMPLY");
  });

  it("fordert zum Setzen auf, wenn keine TOBT gesetzt ist", async () => {
    // Ohne TOBT gibt es keine TSAT und kein Sequencing — der Flug steht
    // nicht in der Folge. Das ist kein Leerwert, sondern eine offene
    // Aufgabe (Thomas, 20.09.2026: "die muss doch gesetzt werden").
    render(<VdgsPlatte stand={{ ...STAND, tobt: "", tsat: "" }} />);
    expect(screen.getByText("TOBT SETZEN")).toBeTruthy();
    // Und keine grosse Leerzahl mehr an der Stelle. (In der kleinen
    // Feldreihe darunter steht weiter "--:--" fuer CTOT usw. — geprueft
    // wird die HAUPTZAHL.)
    expect(document.querySelector(".vdgs__gross")).toBeNull();
    // Gesetzt wird sie auf vats.im/vdgs — der Klick fuehrt dorthin.
    invoke.mockClear();
    screen.getByText("TOBT SETZEN").closest("button")!.click();
    expect(invoke).toHaveBeenCalledWith("vdgs_fenster_oeffnen");
  });

  it("zeigt die Restzeit nicht, solange keine TOBT steht", () => {
    // Sie haette keinen Bezug: Ohne TOBT gibt es nichts, worauf man
    // wartet.
    render(<VdgsPlatte stand={{ ...STAND, tobt: "", tsat: "" }} />);
    expect(screen.queryByText(/in \d+ min|vor \d+ min/)).toBeNull();
  });

  it("nimmt die TOBT als Hauptzahl, wo es keine TSAT gibt", () => {
    // Plätze ohne CDM-Sequenzierung (EDDF, EGLL) liefern keine TSAT —
    // dort stünde sonst dauerhaft „--:--" als größte Zahl im Bild.
    render(<VdgsPlatte stand={{ ...STAND, tsat: "" }} />);
    const gross = screen.getByTestId("vdgs-band").querySelector(".vdgs__gross");
    expect(gross?.textContent).toContain("TOBT");
    expect(gross?.textContent).toContain("15:44");
    expect(gross?.textContent).not.toContain("TSAT");
  });

  it("nimmt die TSAT, sobald es eine gibt", () => {
    render(<VdgsPlatte stand={STAND} />);
    const gross = screen.getByTestId("vdgs-band").querySelector(".vdgs__gross");
    expect(gross?.textContent).toContain("TSAT");
    expect(gross?.textContent).toContain("15:46");
  });
});

/** Probe-Komponente: ruft nur den Haken auf, damit sein Verhalten
 *  prüfbar ist, ohne das ganze Cockpit zu rendern. */
function Probe({ aktiv }: { aktiv: boolean }) {
  const stand = useVdgsStand(aktiv);
  return <span data-testid="probe">{stand ? stand.callsign : "leer"}</span>;
}

describe("useVdgsStand", () => {
  it("fragt die fremde Seite gar nicht, solange nichts läuft", () => {
    invoke.mockResolvedValue(STAND);
    render(<Probe aktiv={false} />);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("fragt genau einmal, sobald die Abflugfolge läuft", async () => {
    // Gegenprobe zur Zeile darüber — sonst wäre „nicht gefragt" auch
    // dann grün, wenn der Haken überhaupt nie abriefe.
    invoke.mockResolvedValue(STAND);
    render(<Probe aktiv={true} />);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("vdgs_stand");
    expect(await screen.findByText("GSG421")).toBeTruthy();
  });

  it("bleibt still, wenn der Dienst nicht antwortet", async () => {
    invoke.mockRejectedValue(new Error("offline"));
    render(<Probe aktiv={true} />);
    expect(await screen.findByText("leer")).toBeTruthy();
  });
});
