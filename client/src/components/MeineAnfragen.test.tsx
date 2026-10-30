// „Meine Anfragen“ (30.10.2026, Thomas): alte Anfragen bleiben nach einer
// neuen sichtbar, mit Nummer, Betreff und Status; Klick öffnet das Ticket.
import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
  isTauri: true,
}));

import { MeineAnfragen, type TicketEintrag } from "./MeineAnfragen";

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

const status = (label: string, x: Partial<NonNullable<TicketEintrag["status"]>> = {}) => ({
  label, offen: false, geschlossen: false, geloest: false, wartet_auf_pilot: false, ...x,
});
const neu: TicketEintrag = {
  ticket_number: "TKT-2026-00014", betreff: "Landung nicht bewertet", status: status("Offen", { offen: true }),
  letzte_aktivitaet: "2026-10-30T13:00:00+00:00", neue_antwort: true, url: "https://german-sky-group.eu/helpdesk/tickets/14",
};
const alt: TicketEintrag = {
  ticket_number: "TKT-2026-00013", betreff: "Paket lädt nicht", status: status("Geschlossen", { geschlossen: true }),
  letzte_aktivitaet: "2026-10-29T10:00:00+00:00", neue_antwort: false, url: null,
};

beforeEach(() => invokeMock.mockReset());

describe("MeineAnfragen", () => {
  it("zeigt alle Tickets mit Nummer, Betreff, Status; geschlossene bleiben", async () => {
    invokeMock.mockResolvedValue([neu, alt]);
    await act(async () => {
      render(<MeineAnfragen sichtbar neuLaden={null} oeffnen={() => {}} />);
    });
    expect(invokeMock).toHaveBeenCalledWith("hilfe_tickets");
    for (const t of ["TKT-2026-00014", "Landung nicht bewertet", "Offen", "Neue Antwort", "TKT-2026-00013", "Paket lädt nicht", "Geschlossen"]) {
      expect(screen.getByText(t)).toBeTruthy();
    }
    // Ohne Link (fremder Host verworfen) lässt sich das Ticket nicht öffnen.
    expect((screen.getByText("Paket lädt nicht").closest("button") as HTMLButtonElement).disabled).toBe(true);
  });

  it("öffnet das Ticket per Klick", async () => {
    invokeMock.mockResolvedValue([neu]);
    const oeffnen = vi.fn();
    await act(async () => {
      render(<MeineAnfragen sichtbar neuLaden={null} oeffnen={oeffnen} />);
    });
    fireEvent.click(screen.getByText("Landung nicht bewertet"));
    expect(oeffnen).toHaveBeenCalledWith(neu.url);
  });

  it("lädt nach dem Senden neu (neuLaden ändert sich) und behält die alte Anfrage", async () => {
    invokeMock.mockResolvedValueOnce([alt]).mockResolvedValueOnce([neu, alt]);
    let r!: ReturnType<typeof render>;
    await act(async () => {
      r = render(<MeineAnfragen sichtbar neuLaden={null} oeffnen={() => {}} />);
    });
    expect(screen.queryByText("TKT-2026-00014")).toBeNull();
    await act(async () => {
      r.rerender(<MeineAnfragen sichtbar neuLaden={{ ticket: "neu" }} oeffnen={() => {}} />);
    });
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText("TKT-2026-00014")).toBeTruthy();
    expect(screen.getByText("TKT-2026-00013")).toBeTruthy();
  });

  it("lädt nicht, solange der Reiter verborgen ist", async () => {
    invokeMock.mockResolvedValue([neu]);
    await act(async () => {
      render(<MeineAnfragen sichtbar={false} neuLaden={null} oeffnen={() => {}} />);
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("leere Liste und Fehler (altes HelpDesk) als Hinweis", async () => {
    invokeMock.mockResolvedValueOnce([]);
    await act(async () => {
      render(<MeineAnfragen sichtbar neuLaden={null} oeffnen={() => {}} />);
    });
    expect(screen.getByText("Du hast noch keine Anfrage gestellt.")).toBeTruthy();
    invokeMock.mockRejectedValueOnce({ code: "ticket_liste_fehlt" });
    await act(async () => {
      fireEvent.click(screen.getByText("Aktualisieren"));
    });
    expect(screen.getByText(/neueren HelpDesk/)).toBeTruthy();
  });
});
