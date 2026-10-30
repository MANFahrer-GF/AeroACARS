// v3 (Etappe 5, P-8.01): Der Bestätigungsdialog der Hülle führt den Befehl
// erst nach „Ausführen“ aus und gibt genau dessen Ergebnis bzw. Fehler
// zurück; „Ablehnen“ führt nichts aus.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("../../lib/ipc", () => ipc);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k, i18n: { resolvedLanguage: "de" } }) }));

import { HuelleFenster } from "./HuelleFenster";

const AUFTRAG = { id: 7, befehl: "flight_logs_delete_all", werte: { a: 1 } };

beforeEach(() => {
  ipc.invoke.mockReset();
});

describe("Hülle-Dialog", () => {
  it("zeigt Befehl und Werte und führt erst nach Ausführen aus", async () => {
    ipc.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "shell_auftraege") return [AUFTRAG];
      if (cmd === "flight_logs_delete_all") return { deleted: 2 };
      return true;
    });
    render(<HuelleFenster />);
    expect(await screen.findByTestId("huelle-befehl")).toHaveTextContent("flight_logs_delete_all");
    expect(screen.getByTestId("huelle-satz")).toHaveTextContent("Alle Flugprotokolle auf diesem Rechner löschen");
    expect(screen.getByTestId("huelle-werte")).toHaveTextContent("a: 1");
    expect(ipc.invoke).not.toHaveBeenCalledWith("flight_logs_delete_all", expect.anything());
    fireEvent.click(screen.getByText("huelle.ausfuehren"));
    await waitFor(() =>
      expect(ipc.invoke).toHaveBeenCalledWith("shell_auftrag_beantworten", { id: 7, ok: true, ergebnis: { deleted: 2 } }),
    );
    expect(ipc.invoke).toHaveBeenCalledWith("flight_logs_delete_all", { a: 1 });
  });

  it("gibt den Fehler des Befehls zurück", async () => {
    ipc.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "shell_auftraege") return [AUFTRAG];
      if (cmd === "flight_logs_delete_all") throw { code: "x", message: "kaputt" };
      return true;
    });
    render(<HuelleFenster />);
    fireEvent.click(await screen.findByText("huelle.ausfuehren"));
    await waitFor(() =>
      expect(ipc.invoke).toHaveBeenCalledWith("shell_auftrag_beantworten", {
        id: 7, ok: false, fehler: { code: "x", message: "kaputt" },
      }),
    );
  });

  it("maskiert Geheimnisse, zeigt übrige Werte", async () => {
    ipc.invoke.mockImplementation(async () => [
      { id: 8, befehl: "phpvms_login", werte: { url: "https://german-sky-group.eu", apiKey: "geheim123" } },
    ]);
    render(<HuelleFenster />);
    const werte = await screen.findByTestId("huelle-werte");
    expect(werte).toHaveTextContent("url: https://german-sky-group.eu");
    expect(werte).toHaveTextContent("apiKey:");
    expect(werte).not.toHaveTextContent("geheim123");
    expect(screen.getByTestId("huelle-satz")).toHaveTextContent("phpVMS");
  });

  it("unbekannter Befehl: kein Satz, nur der Name", async () => {
    ipc.invoke.mockImplementation(async () => [{ id: 9, befehl: "gibt_es_nicht", werte: {} }]);
    render(<HuelleFenster />);
    expect(await screen.findByTestId("huelle-befehl")).toHaveTextContent("gibt_es_nicht");
    expect(screen.queryByTestId("huelle-satz")).toBeNull();
    expect(screen.queryByTestId("huelle-werte")).toBeNull();
  });

  it("Ablehnen führt nichts aus", async () => {
    ipc.invoke.mockImplementation(async () => []);
    ipc.invoke.mockImplementationOnce(async () => [AUFTRAG]);
    render(<HuelleFenster />);
    fireEvent.click(await screen.findByText("huelle.ablehnen"));
    await waitFor(() => expect(ipc.invoke).toHaveBeenCalledWith("shell_auftrag_beantworten", { id: 7, ok: false }));
    expect(ipc.invoke).not.toHaveBeenCalledWith("flight_logs_delete_all", expect.anything());
  });
});
