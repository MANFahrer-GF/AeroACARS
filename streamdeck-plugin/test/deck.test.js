import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { szene, streifen, kachelOrt, KACHEL, phasenGruppe, fortschritt, laufschrift } from "../src/szene.js";
import { vorbereiten, rastern, pngAusschnitt } from "../src/render.js";
import { PHASEN } from "../skripte/vorschau-daten.mjs";

const ORDNER = path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin");
const GERAETE = { xl: [8, 4], 15: [5, 3], plus: [4, 2] };

/** PNG zurück in Pixel (nur unser eigenes Format: RGBA, Filter 0). */
function entpacken(png) {
  assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  const b = png.readUInt32BE(16), h = png.readUInt32BE(20);
  const start = png.indexOf("IDAT") + 4;
  const laenge = png.readUInt32BE(start - 8);
  const roh = zlib.inflateSync(png.subarray(start, start + laenge));
  const pixel = Buffer.alloc(b * h * 4);
  for (let r = 0; r < h; r++) roh.copy(pixel, r * b * 4, r * (b * 4 + 1) + 1, (r + 1) * (b * 4 + 1));
  return { b, h, pixel };
}

test("Rasterer + eigenes PNG: Kachel entspricht exakt dem Ausschnitt der Szene", async () => {
  await vorbereiten(ORDNER);
  const sz = szene("xl", PHASEN.reiseflug, 30);
  const bild = rastern(sz.svg);
  assert.equal(bild.breite, sz.breite);
  const { x, y } = kachelOrt(2, 1);
  const kachel = entpacken(pngAusschnitt(bild, x, y, KACHEL, KACHEL));
  assert.deepEqual([kachel.b, kachel.h], [KACHEL, KACHEL]);
  for (const [px, py] of [[0, 0], [70, 70], [143, 143], [10, 120]]) {
    const i = (py * KACHEL + px) * 4, j = ((y + py) * bild.breite + x + px) * 4;
    assert.deepEqual([...kachel.pixel.subarray(i, i + 4)], [...bild.pixel.subarray(j, j + 4)], `${px},${py}`);
  }
  // Die Kachel ist nicht leer (Grund + Inhalt haben verschiedene Farben).
  const farben = new Set();
  for (let i = 0; i < kachel.pixel.length; i += 4 * 97) farben.add(kachel.pixel.readUInt32BE(i));
  assert.ok(farben.size > 3);
});

test("jede Kachel jedes Geräts hat in jeder Phase eine Belegung", () => {
  for (const [g, [sp, re]] of Object.entries(GERAETE)) {
    for (const [phase, s] of Object.entries(PHASEN)) {
      const sz = szene(g, s, 1);
      assert.deepEqual([sz.spalten, sz.reihen], [sp, re], `${g}/${phase}`);
      for (let r = 0; r < re; r++) for (let c = 0; c < sp; c++) assert.ok(sz.aktionen[`${c},${r}`], `${g}/${phase} ${c},${r}`);
      assert.ok(!/undefined|NaN|null/.test(sz.svg.replace(/<[^>]*>/g, " ")), `${g}/${phase}: Text mit undefined/NaN`);
    }
  }
});

test("Ansagen und Musik sind auf jedem Gerät leiser, stumm und lauter bedienbar", () => {
  for (const g of ["xl", "15"]) {
    const werte = Object.values(szene(g, PHASEN.reiseflug, 1).aktionen).map((a) => a.bedienung);
    for (const b of ["ans_minus", "ans_stumm", "ans_plus", "mus_minus", "mus_stumm", "mus_plus", "zurueck", "cdm"]) assert.ok(werte.includes(b), `${g}: ${b}`);
  }
});

test("ohne AeroACARS und ohne Flug zeichnet die Szene trotzdem", () => {
  const leer = { erreichbar: false, status: null, kabine: null, hoppie: null, debrief: null, jetzt: Date.now() };
  for (const g of Object.keys(GERAETE)) assert.ok(szene(g, leer, 1).svg.includes("<svg"));
  assert.match(laufschrift(null, leer), /KEIN AKTIVER FLUG/);
  assert.ok(streifen(leer, 1).includes("<svg"));
  for (const rad of ["ans", "mus", "info", "wetter"]) assert.ok(streifen(leer, 1, { rad }).includes("<svg"), rad);
});

test("Flugzeug auf dem Band: aus Restflugzeit, am Boden nach Phase", () => {
  const ab = Date.parse("2026-10-23T18:00:00Z");
  const st = { phase: "cruise", takeoff_at: "2026-10-23T18:00:00Z", live: { ete_min: 60 } };
  assert.equal(fortschritt(st, ab + 60 * 60000), 0.5);
  assert.equal(fortschritt({ phase: "boarding" }, ab), 0);
  assert.equal(fortschritt({ phase: "taxi_in" }, ab), 1);
  assert.equal(phasenGruppe("approach"), "anflug");
});

test("Zielgate erst nach der Landung", () => {
  const s = PHASEN.boarding;
  assert.ok(!szene("xl", s, 1).svg.includes(">Ziel 75<"));
  assert.ok(szene("xl", PHASEN.gelandet, 1).svg.includes("GATE ZIEL"));
});

test("Status-Taste: ACARS, SIM, SERVER in jeder Phase, mit den richtigen Zuständen", async () => {
  const { statusZeilen } = await import("../src/szene.js");
  const s = PHASEN.reiseflug;
  assert.deepEqual(statusZeilen(s.status, s).map(([, z]) => z), ["ok", "ok", "ok"]);
  assert.equal(statusZeilen({ ...s.status, paused_since: "x" }, s)[1][1], "warn");
  assert.equal(statusZeilen({ ...s.status, live: null }, s)[1][1], "aus");
  assert.equal(statusZeilen({ ...s.status, connection_state: "failing" }, s)[2][1], "warn");
  assert.equal(statusZeilen({ ...s.status, connection_state: "blocked" }, s)[2][1], "aus");
  assert.equal(statusZeilen(null, { erreichbar: false })[0][1], "aus");
  for (const g of ["xl", "15", "plus"]) for (const [phase, f] of Object.entries(PHASEN)) {
    assert.ok(szene(g, f, 1).svg.includes(">STATUS<"), `${g}/${phase}`);
  }
});
