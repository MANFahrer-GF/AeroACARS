// Der Grundzustand des Pilotenchats: an, abschaltbar.
//
// Festgenagelt, weil das eine Produktentscheidung ist, die im Code wie ein
// Detail aussieht. Ein `=== "1"` statt `!== "0"` an dieser Stelle würde den
// Chat nach jeder Neuinstallation stumm schalten, ohne dass jemand einen
// Fehler sieht — man wartet nur auf Antworten, die nie kommen.

import { describe, it, expect, beforeEach } from "vitest";
import {
  CHAT_AN_STORAGE_KEY,
  CHAT_TON_STORAGE_KEY,
  chatAnGeladen,
  chatAnSpeichern,
  chatTonGeladen,
  chatTonSpeichern,
} from "./chatEinstellungen";

describe("Pilotenchat — Grundzustand", () => {
  beforeEach(() => localStorage.clear());

  it("ist beim ersten Start an, ebenso der Ton", () => {
    expect(chatAnGeladen()).toBe(true);
    expect(chatTonGeladen()).toBe(true);
  });

  it("bleibt aus, wenn der Pilot ihn abgeschaltet hat", () => {
    chatAnSpeichern(false);
    expect(chatAnGeladen()).toBe(false);
    chatAnSpeichern(true);
    expect(chatAnGeladen()).toBe(true);
  });

  it("schaltet den Ton getrennt vom Chat", () => {
    chatTonSpeichern(false);
    expect(chatTonGeladen()).toBe(false);
    expect(chatAnGeladen()).toBe(true);
  });

  it("kennt nur die eigene 0 als Aus — nicht irgendeinen Fremdwert", () => {
    localStorage.setItem(CHAT_AN_STORAGE_KEY, "false");
    localStorage.setItem(CHAT_TON_STORAGE_KEY, "nein");
    expect(chatAnGeladen()).toBe(true);
    expect(chatTonGeladen()).toBe(true);
  });
});
