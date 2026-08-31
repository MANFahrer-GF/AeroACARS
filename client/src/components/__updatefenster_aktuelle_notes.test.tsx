/**
 * Stufe 4 der Release-Checkliste, messbar statt angeschaut.
 *
 * Die Checkliste verlangt einen Sichttest des Update-Fensters MIT DEM
 * AKTUELLEN Release-Text — genau der Schritt, der den Svenny-Fehler
 * gefangen hätte. Ein Blick auf den Bildschirm ist nicht reproduzierbar;
 * hier steht dieselbe Frage als Prüfung.
 *
 * ⚠ Die Vorfassung dieser Datei war an `v1.7.3.md` festgenagelt und trug
 * im Kopf die Anweisung, sie beim nächsten Release umzubenennen oder zu
 * löschen — „sie soll NICHT stillschweigend gegen einen alten Text
 * weiterlaufen und Sicherheit vortäuschen". Genau das ist dann über
 * ZEHN Releases passiert (v1.7.4 bis v1.7.13): Sie lief grün gegen einen
 * ein halbes Jahr alten Text, während jede neue Fassung ungeprüft
 * ausging. Ein Wächter, der an eine Handbewegung gebunden ist, ist
 * keiner.
 *
 * Deshalb liest sie die Datei jetzt aus der **Version in
 * `package.json`**. Fehlt sie, wird der Test rot und nennt den erwarteten
 * Pfad — Vergessen ist damit nicht mehr still möglich.
 */
import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { UpdateButton } from "./UpdateButton";

// Von `client/` aus — vitest laeuft dort, nicht in diesem Ordner.
const VERSION = JSON.parse(
  readFileSync(resolve(process.cwd(), "package.json"), "utf8"),
).version as string;

const NOTES_PFAD = resolve(
  process.cwd(),
  `../docs/release-notes/v${VERSION}.md`,
);

if (!existsSync(NOTES_PFAD)) {
  throw new Error(
    `Release-Notes fuer die Fassung in package.json (${VERSION}) fehlen.\n` +
      `Erwartet: ${NOTES_PFAD}\n` +
      `Stufe 2 der Release-Checkliste — ohne die Datei baut die CI einen ` +
      `Release ohne Text, und das Update-Fenster der Piloten bleibt leer.`,
  );
}

const NOTES = readFileSync(NOTES_PFAD, "utf8");

function checker(body: string) {
  return {
    update: { version: VERSION, body, date: null },
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

describe(`Update-Fenster mit den echten v${VERSION}-Notes`, () => {
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
    // bzw. `-h3`) — kein react-markdown, um 90 KB zu sparen.
    const dialog = oeffnen();
    const h2 = dialog.querySelectorAll(".update-modal__notes-h2");
    const h3 = dialog.querySelectorAll(".update-modal__notes-h3");
    // So viele ##-Ueberschriften, wie die Datei hat.
    const erwartet_h2 = NOTES.split("\n").filter((z) =>
      /^## /.test(z),
    ).length;
    expect(h2.length).toBe(erwartet_h2);
    expect(
      h2.length + h3.length,
      "keine Ueberschrift gedeutet — das Markdown kam als Fliesstext an",
    ).toBeGreaterThan(2);
  });

  it("beide Sprachbloecke stehen wirklich drin", () => {
    // Version-unabhaengig: die beiden Sprach-Ueberschriften kommen an,
    // UND der sichtbare Text ist nicht auf einen Bruchteil geschrumpft.
    // Faellt ein Block beim Umsetzen weg, reisst die Laengenpruefung.
    const dialog = oeffnen();
    const text = dialog.textContent ?? "";
    expect(text).toContain("Deutsch");
    expect(text).toContain("English");
    expect(
      text.length,
      `sichtbarer Text ${text.length} Zeichen gegen ${NOTES.length} in der Datei — ` +
        "da ist mehr als Markdown-Auszeichnung verlorengegangen",
    ).toBeGreaterThan(NOTES.length * 0.7);
  });

  it("eine Tabelle wird nicht zu Kauderwelsch", () => {
    // Der Umsetzer kennt keine Tabellen (bewusst, siehe Kommentar dort).
    // Traegt diese Fassung eine, muessen die Zeilen wenigstens LESBAR
    // ankommen und nicht als Pipe-Salat. Traegt sie keine, ist hier
    // nichts zu pruefen — das sagt der Test dann auch.
    const hat_tabelle = NOTES.split("\n").some((z) => /^\|.*\|/.test(z));
    if (!hat_tabelle) {
      expect(hat_tabelle).toBe(false);
      return;
    }
    const text = oeffnen().textContent ?? "";
    expect(text).not.toContain("|---");
  });
});
