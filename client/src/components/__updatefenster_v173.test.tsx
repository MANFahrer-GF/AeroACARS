/**
 * Stufe 4 der Release-Checkliste, messbar statt angeschaut.
 *
 * Die Checkliste verlangt einen Sichttest des Update-Fensters MIT DEM
 * AKTUELLEN Release-Text — genau der Schritt, der den Svenny-Fehler
 * gefangen hätte. Ein Blick auf den Bildschirm ist dabei nicht
 * reproduzierbar; hier steht dieselbe Frage als Prüfung.
 *
 * Diese Datei ist bewusst an die Fassung gebunden: Sie liest die echte
 * `docs/release-notes/v1.7.3.md`. Beim nächsten Release wird sie
 * umbenannt oder gelöscht — sie soll NICHT stillschweigend gegen einen
 * alten Text weiterlaufen und Sicherheit vortäuschen.
 */
import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { UpdateButton } from "./UpdateButton";

// Von `client/` aus — vitest laeuft dort, nicht in diesem Ordner.
const NOTES = readFileSync(
  resolve(process.cwd(), "../docs/release-notes/v1.7.3.md"),
  "utf8",
);

function checker(body: string) {
  return {
    update: { version: "1.7.3", body, date: null },
    stage: "fresh" as const,
    installing: false,
    progress: null,
    installAndRelaunch: async () => {},
    snooze: () => {},
  };
}

function oeffnen() {
  render(<UpdateButton checker={checker(NOTES) as never} />);
  fireEvent.click(screen.getByRole("button", { name: /update/i }));
  return screen.getByRole("dialog");
}

describe("Update-Fenster mit den echten v1.7.3-Notes", () => {
  it("die Notes sind ueberhaupt zweisprachig und nicht leer", () => {
    expect(NOTES.length).toBeGreaterThan(1000);
    expect(NOTES).toContain("🇩🇪");
    expect(NOTES).toContain("🇬🇧");
  });

  it("das Fenster oeffnet und traegt beide Knoepfe", () => {
    oeffnen();
    expect(
      screen.getByRole("button", { name: /installieren|jetzt/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /später|spaeter/i }),
    ).toBeInTheDocument();
  });

  it("kein Roh-Markdown im sichtbaren Text", () => {
    // Der Svenny-Fehler: `###` und `**fett**` standen als Zeichen da.
    // Diese Notes tragen zusaetzlich eine TABELLE — die Pipes waeren
    // die naechste Stelle, an der es auffaellt.
    const text = oeffnen().textContent ?? "";
    for (const marker of ["###", "**", "|---|", "---|"]) {
      expect(text, `„${marker}" steht als Zeichen im Fenster`).not.toContain(
        marker,
      );
    }
  });

  it("die Ueberschriften kommen als Ueberschriften an", () => {
    // ⚠ NICHT nach <h2>/<h3> suchen. Das Fenster rendert Ueberschriften
    // bewusst als gestaltete <div>-Elemente (`update-modal__notes-h2`
    // bzw. `-h3`) — kein react-markdown, um 90 KB zu sparen. Sichtbar ist
    // das dasselbe; meine erste Fassung dieser Pruefung suchte nach
    // echten Ueberschriften-Elementen und meldete prompt einen Fehler,
    // den es nicht gibt.
    const dialog = oeffnen();
    const h2 = dialog.querySelectorAll(".update-modal__notes-h2");
    const h3 = dialog.querySelectorAll(".update-modal__notes-h3");
    expect(
      h2.length + h3.length,
      "keine Ueberschrift gedeutet — das Markdown kam als Fliesstext an",
    ).toBeGreaterThan(6);
    // Und die Sprachbloecke sind die beiden ##-Ueberschriften.
    expect(h2.length).toBe(2);
  });

  it("die Tabelle wird nicht zu Kauderwelsch", () => {
    // Diese Notes tragen zwei Tabellen — die erste seit dem Svenny-Fehler.
    // Der Umsetzer kennt keine Tabellen (bewusst, siehe Kommentar dort),
    // also muessen die Zeilen wenigstens LESBAR ankommen und nicht als
    // Pipe-Salat.
    const text = oeffnen().textContent ?? "";
    expect(text).toContain("erste 300 m nach dem Aufsetzen");
    expect(text).toContain("42,6 m");
    expect(text).not.toContain("|---");
  });

  it("beide Sprachbloecke stehen wirklich drin", () => {
    const text = oeffnen().textContent ?? "";
    expect(text).toContain("Bahnbelag fehlte");
    expect(text).toContain("Runway surface was missing");
  });
});
