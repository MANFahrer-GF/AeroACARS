import { test } from "node:test";
import assert from "node:assert/strict";

// deck.js zieht das Elgato-SDK; für die reine Druck-Logik genügt eine Attrappe.
const { druckPlan, layoutFuer } = await import("../src/druckplan.js");

// Umgedreht am 24.10. auf Thomas' Wunsch: vorher nur durch 1 s Halten.
test("Ansagen stumm und wieder an durch kurzen Druck", () => {
  assert.deepEqual(druckPlan({ bedienung: "ans_stumm" }, { stumm: false }), { kurz: "ans_umschalten" });
  assert.deepEqual(druckPlan({ bedienung: "ans_stumm" }, { stumm: true }), { kurz: "ans_umschalten" });
});

test("Musik: kurz stumm/laut, halten Boarding-Musik an/aus", () => {
  assert.deepEqual(druckPlan({ bedienung: "mus_stumm" }, {}), { kurz: "mus_stumm", halten: "boarding" });
  assert.deepEqual(druckPlan({ kurz: "musik_stumm", halten: "boarding" }, {}), { kurz: "musik_stumm", halten: "boarding", lang: undefined });
});

test("Anzeigen: kurz METAR laden, lang Seite öffnen", () => {
  assert.deepEqual(druckPlan({ kurz: "metar_ziel", lang: "briefing" }, {}), { kurz: "metar_ziel", halten: undefined, lang: "briefing" });
  assert.deepEqual(druckPlan({ lang: "map" }, {}), { kurz: undefined, halten: undefined, lang: "map" });
  assert.deepEqual(druckPlan(undefined, {}), {});
});

test("Layout je Gerätetyp", () => {
  assert.equal(layoutFuer(0), "15");
  assert.equal(layoutFuer(2), "xl");
  assert.equal(layoutFuer(7), "plus");
  assert.equal(layoutFuer(1), null); // Mini
});
