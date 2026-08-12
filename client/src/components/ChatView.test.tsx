// Pilotenchat — die Regeln, die im Cockpit zählen.
//
// Geprüft wird die gerenderte Ansicht, nicht eine nachgebaute Logik. Die
// Lehre des Tages: was nachgebaut wird, bemerkt keine Änderung.
//
// Drei Dinge sind hier sicherheitsrelevant und nicht bloß Kosmetik:
//   1. Der Fokus wird nie von selbst geholt — sonst gingen Tastendrücke des
//      Piloten in den Chat statt in den Sim.
//   2. Im Endanflug verschwindet das Eingabefeld.
//   3. Eine Direktnachricht muss sichtbar adressiert sein, bevor sie rausgeht.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const tauriInvoke = vi.hoisted(() => {
  (globalThis as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  if (typeof window !== "undefined") {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  }
  return vi.fn();
});
const hoerer = vi.hoisted(() => ({ fn: null as null | ((e: { payload: unknown }) => void) }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: unknown }) => void) => {
    hoerer.fn = cb;
    return Promise.resolve(() => { hoerer.fn = null; });
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_k: string, arg?: unknown) =>
      typeof arg === "string" ? arg
        : (arg as { defaultValue?: string })?.defaultValue ?? _k,
  }),
}));

import { ChatView } from "./ChatView";

// Reihenfolge BEWUSST verkehrt: Sven (anderes Ziel) steht vor Michel
// (gleiches Ziel wie ich). Stünde die Vorlage schon richtig, würde der
// Sortier-Test nichts beweisen — genau daran ist die erste Fassung
// vorbeigelaufen.
const TEILNEHMER = [
  { pilot_id: "1", callsign: "ITY 4532", dep: "EDDB", arr: "LICC", anzeigename: "Thomas K" },
  { pilot_id: "9", callsign: "GEC 1306", dep: "LEIB", arr: "LPFR", anzeigename: "Sven M" },
  { pilot_id: "2", callsign: "EZY 5077", dep: "EDDB", arr: "LICC", anzeigename: "Michel D" },
];

function antworten() {
  tauriInvoke.mockImplementation((cmd: string) => {
    if (cmd === "chat_verlauf") return Promise.resolve({ nachrichten: [], fenster_stunden: 12 });
    if (cmd === "chat_teilnehmer") return Promise.resolve({ teilnehmer: TEILNEHMER });
    if (cmd === "chat_senden") return Promise.resolve(true);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  tauriInvoke.mockReset();
  antworten();
  hoerer.fn = null;
});
afterEach(() => vi.restoreAllMocks());

describe("Pilotenchat", () => {
  it("holt Verlauf und Teilnehmer beim Öffnen", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await waitFor(() => {
      expect(tauriInvoke).toHaveBeenCalledWith("chat_verlauf", undefined);
      expect(tauriInvoke).toHaveBeenCalledWith("chat_teilnehmer", undefined);
    });
  });

  // Der Wettlauf beim Öffnen: der Verlauf-Abruf braucht einen Moment, und
  // in genau diesem Moment kann über MQTT ein Zuruf eintreffen. Vorher
  // setzte der Abruf die Liste hart und der Zuruf war weg — ohne jede
  // Spur, denn technisch war nichts schiefgegangen.
  it("verliert keinen Zuruf, der während des Abrufs eintrifft", async () => {
    let verlaufAufloesen: (w: unknown) => void = () => {};
    tauriInvoke.mockImplementation((cmd: string) => {
      if (cmd === "chat_verlauf") return new Promise((res) => { verlaufAufloesen = res; });
      if (cmd === "chat_teilnehmer") return Promise.resolve({ teilnehmer: TEILNEHMER });
      return Promise.resolve(null);
    });
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);

    // Der Zuruf kommt an, bevor der Verlauf da ist.
    await waitFor(() => expect(hoerer.fn).toBeTruthy());
    hoerer.fn!({ payload: {
      id: 77, va_prefix: "gsg", von_pilot_id: "2", an_pilot_id: null,
      ts: Date.now(), text: "Bin schon im Steigflug", callsign: "EZY 5077",
      anzeigename: "Michel D",
    } });
    expect(await screen.findByText("Bin schon im Steigflug")).toBeTruthy();

    // Jetzt trifft der Verlauf ein — mit einer ÄLTEREN Zeile, die den
    // frischen Zuruf nicht verdrängen darf.
    verlaufAufloesen({ nachrichten: [{
      id: 12, va_prefix: "gsg", von_pilot_id: "9", an_pilot_id: null,
      ts: Date.now() - 60_000, text: "Guten Morgen zusammen", callsign: "GEC 1306",
      anzeigename: "Sven M",
    }], fenster_stunden: 12 });

    expect(await screen.findByText("Guten Morgen zusammen")).toBeTruthy();
    expect(screen.getByText("Bin schon im Steigflug")).toBeTruthy();
  });

  // Wer im Funkschatten war (Tablet aus, WLAN weg, Rechner im Schlaf), hat
  // die Zurufe dieser Zeit nie bekommen. Beim Zurückkommen wird abgeglichen.
  it("holt Verpasstes nach, wenn das Fenster wieder sichtbar wird", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith("chat_verlauf", undefined));
    const vorher = tauriInvoke.mock.calls.filter((c) => c[0] === "chat_verlauf").length;

    tauriInvoke.mockImplementation((cmd: string) => {
      if (cmd === "chat_verlauf") return Promise.resolve({ nachrichten: [{
        id: 99, va_prefix: "gsg", von_pilot_id: "2", an_pilot_id: null,
        ts: Date.now(), text: "Warst du weg?", callsign: "EZY 5077", anzeigename: "Michel D",
      }], fenster_stunden: 12 });
      if (cmd === "chat_teilnehmer") return Promise.resolve({ teilnehmer: TEILNEHMER });
      return Promise.resolve(null);
    });

    // Zwei Wechsel kurz hintereinander — die Drossel darf daraus nur eine
    // Anfrage machen, sonst häufen sich beim Hin- und Herklicken zwischen
    // Sim und Client die Rundläufe über die LAN-Brücke.
    // Eine echte Abwesenheit dauert länger als ein Wimpernschlag — die Uhr
    // eine Minute vorstellen, sonst prüft der Test nur die Drossel.
    const echt = Date.now();
    const uhr = vi.spyOn(Date, "now").mockReturnValue(echt + 60_000);

    // Zwei Wechsel kurz hintereinander — die Drossel darf daraus nur eine
    // Anfrage machen, sonst häufen sich beim Hin- und Herklicken zwischen
    // Sim und Client die Rundläufe über die LAN-Brücke.
    document.dispatchEvent(new Event("visibilitychange"));
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => {
      expect(tauriInvoke.mock.calls.filter((c) => c[0] === "chat_verlauf").length).toBeGreaterThan(vorher);
    });
    expect(await screen.findByText("Warst du weg?")).toBeTruthy();
    const nachher = tauriInvoke.mock.calls.filter((c) => c[0] === "chat_verlauf").length;
    expect(nachher - vorher).toBe(1);
    uhr.mockRestore();
  });

  it("zeigt Namen UND Rufzeichen UND Strecke — Rufzeichen allein nutzt nichts", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    expect(await screen.findByText("Michel D")).toBeTruthy();
    expect(screen.getByText("EZY 5077")).toBeTruthy();
    expect(screen.getAllByText(/EDDB → LICC/).length).toBeGreaterThan(0);
  });

  it("führt sich selbst NICHT in der Teilnehmerliste auf", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await screen.findByText("Michel D");
    expect(screen.queryByText("Thomas K")).toBeNull();
  });

  it("stellt gleiches Ziel nach vorn", async () => {
    // Im Flug lautet die Frage "wer ist auch nach Catania unterwegs" —
    // die soll man nicht suchen müssen.
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await screen.findByText("Michel D");
    const namen = Array.from(document.querySelectorAll(".chat__pilot-name"))
      .map((e) => e.textContent);
    expect(namen[0]).toBe("Michel D");   // LICC, wie ich
    expect(namen[1]).toBe("Sven M");     // LPFR
  });

  it("SICHERHEIT: holt den Tastaturfokus nicht von selbst", async () => {
    // Eine eingehende Nachricht darf dem Piloten nie die Tastatur wegnehmen.
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await screen.findByText("Michel D");
    const vorher = document.activeElement;
    hoerer.fn?.({ payload: {
      id: 1, va_prefix: "gsg", von_pilot_id: "2", an_pilot_id: null,
      ts: Date.now(), text: "Hallo", callsign: "EZY 5077", anzeigename: "Michel D",
    } });
    await screen.findByText(/Hallo/);
    expect(document.activeElement).toBe(vorher);
  });

  it("warnt sichtbar, sobald die Tastatur im Chat liegt", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    const feld = await screen.findByLabelText("Nachricht schreiben");
    expect(document.querySelector(".chat__fokus")).toBeNull();
    fireEvent.focus(feld);
    expect(document.querySelector(".chat__fokus")).toBeTruthy();
    fireEvent.blur(feld);
    expect(document.querySelector(".chat__fokus")).toBeNull();
  });

  it("SICHERHEIT: im Endanflug ist das Eingabefeld weg, Zurufe bleiben", async () => {
    render(<ChatView eigenePilotId="1" phase="FINAL" tonAn={false} />);
    await screen.findByText("Michel D");
    expect(screen.queryByLabelText("Nachricht schreiben")).toBeNull();
    expect(screen.getByText("Bin gleich am Gate")).toBeTruthy();
    expect(document.querySelector(".chat__gesperrt")).toBeTruthy();
  });

  it("im Reiseflug ist das Eingabefeld da", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    expect(await screen.findByLabelText("Nachricht schreiben")).toBeTruthy();
  });

  it("ein Schnellzuruf geht ohne Tastatur raus", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    fireEvent.click(await screen.findByText("Rolle raus"));
    await waitFor(() => {
      expect(tauriInvoke).toHaveBeenCalledWith("chat_senden", {
        text: "Rolle raus", anPilotId: null,
      });
    });
  });

  it("Klick auf einen Namen adressiert eine Direktnachricht sichtbar", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    fireEvent.click(await screen.findByText("Michel D"));
    const zeile = document.querySelector(".chat__an-wen");
    expect(zeile?.textContent).toContain("Michel D");

    const feld = screen.getByLabelText("Nachricht schreiben");
    fireEvent.change(feld, { target: { value: "Nur für dich" } });
    fireEvent.submit(feld.closest("form")!);
    await waitFor(() => {
      expect(tauriInvoke).toHaveBeenCalledWith("chat_senden", {
        text: "Nur für dich", anPilotId: "2",
      });
    });
  });

  it("nach dem Senden ist die Adressierung wieder aufgehoben", async () => {
    // Sonst ginge die nächste Nachricht versehentlich auch direkt raus.
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    fireEvent.click(await screen.findByText("Michel D"));
    const feld = screen.getByLabelText("Nachricht schreiben");
    fireEvent.change(feld, { target: { value: "eins" } });
    fireEvent.submit(feld.closest("form")!);
    await waitFor(() => expect(document.querySelector(".chat__an-wen")).toBeNull());
  });

  it("derselbe Zuruf erscheint nicht doppelt", async () => {
    // Er kann über den Verlauf UND über MQTT hereinkommen.
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await screen.findByText("Michel D");
    const n = {
      id: 7, va_prefix: "gsg", von_pilot_id: "2", an_pilot_id: null,
      ts: Date.now(), text: "Doppelt?", callsign: "EZY 5077", anzeigename: "Michel D",
    };
    hoerer.fn?.({ payload: n });
    hoerer.fn?.({ payload: n });
    await screen.findByText(/Doppelt\?/);
    expect(screen.getAllByText(/Doppelt\?/).length).toBe(1);
  });

  it("leere Nachrichten gehen nicht raus", async () => {
    // Erst Leerzeichen, dann etwas Echtes. Am Ende darf GENAU EIN Zuruf
    // rausgegangen sein — und zwar der echte. Nur auf "noch nichts gesendet"
    // zu prüfen ginge zum Zeitpunkt null schon durch, bevor der Aufruf
    // überhaupt hätte stattfinden können.
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    const feld = await screen.findByLabelText("Nachricht schreiben");
    fireEvent.change(feld, { target: { value: "   " } });
    fireEvent.submit(feld.closest("form")!);
    fireEvent.change(feld, { target: { value: "echt" } });
    fireEvent.submit(feld.closest("form")!);
    await waitFor(() => {
      const gesendet = tauriInvoke.mock.calls.filter((c) => c[0] === "chat_senden");
      expect(gesendet.length).toBe(1);
      expect((gesendet[0][1] as { text: string }).text).toBe("echt");
    });
  });

  it("eine Direktnachricht an dich ist als solche markiert", async () => {
    render(<ChatView eigenePilotId="1" phase="CRUISE" tonAn={false} />);
    await screen.findByText("Michel D");
    hoerer.fn?.({ payload: {
      id: 9, va_prefix: "gsg", von_pilot_id: "2", an_pilot_id: "1",
      ts: Date.now(), text: "geheim", callsign: "EZY 5077", anzeigename: "Michel D",
    } });
    await screen.findByText(/geheim/);
    expect(document.querySelector(".chat__msg--direkt")).toBeTruthy();
    expect(screen.getByText("NUR AN DICH")).toBeTruthy();
  });
});
