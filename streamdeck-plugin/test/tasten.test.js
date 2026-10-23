import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { TASTEN, INFO_SEITEN, WETTER_SEITEN, hinweis, musikStumm, schritt, dauer, hoehe, wind, flughoehe } from "../src/tasten.js";
import { REGLER, PROFILE, uuid } from "../src/belegung.js";
import { tasteSvg, reglerSvg, passend } from "../src/bild.js";

// Feldnamen wie ActiveFlightInfo (lib.rs) sie serialisiert.
const status = {
  callsign: "GEC 8172",
  dpt_airport: "EDDF",
  arr_airport: "LEPA",
  dep_gate: "B44",
  arr_gate: null,
  phase: "cruise",
  block_off_at: "2026-10-23T12:00:00Z",
  takeoff_at: "2026-10-23T12:21:00Z",
  landing_at: null,
  block_on_at: null,
  sim_fuel_kg: 6400,
  planned_reserve_kg: 2100,
  sim_zfw_kg: 61200,
  sim_tow_kg: 70100,
  planned_max_zfw_kg: 62500,
  planned_max_tow_kg: 79000,
  planned_ldw_kg: 62800,
  planned_max_ldw_kg: 66000,
  dep_metar_decoded: { icao: "EDDF", wind_direction_deg: 250, wind_speed_kt: 12, gust_kt: null, qnh_hpa: 1018 },
  arr_metar_decoded: { icao: "LEPA", wind_direction_deg: 240, wind_speed_kt: 8, gust_kt: 18, qnh_hpa: 1016 },
  predicted_runway: "24L",
  approach_runway: null,
  planned_alternate: "LEVC",
  divert_hint: null,
  landing_rate_fpm: null,
  landing_g_force: null,
  go_around_count: 0,
  touch_and_go_count: 0,
  live: { gs_kt: 468.4, ias_kt: 271.2, altitude_msl_ft: 35180, altitude_pressure_ft: 35012, fuel_total_kg: 6390 },
  distance_nm: 512.3,
  connection_state: "live",
  paused_since: null,
  abgabe_sperre: null,
};
const kabine = {
  an: true,
  stumm: false,
  lautstaerke_live: 0.8,
  lautstaerke_gespeichert: 0.8,
  musik_live: 0.2,
  musik_gespeichert: 0.2,
  boarding_musik: true,
  flug: true,
  grund: null,
  belegt: false,
  hinweis: null,
  naechste: { slot: "kabine_sinkflug", wann: Date.parse("2026-10-23T15:05:00Z") / 1000, faellig: false, wartet: null },
};
const jetzt = new Date("2026-10-23T13:42:00Z");
const voll = { erreichbar: true, status, kabine, hoppie: { connected: true, logged_on: true, station_id: "LECB", pending_uplink_count: 1 }, debrief: null, jetzt };

test("ohne AeroACARS sind alle Tasten aus, ohne Flug die Flugtasten", () => {
  const ohne = { erreichbar: false, status: null, kabine: null, hoppie: null, debrief: null, jetzt };
  for (const [id, t] of Object.entries(TASTEN)) {
    if (id === "utc") continue;
    assert.equal(t.bild(ohne).ton, "aus", id);
  }
  const keinFlug = { ...voll, status: null };
  assert.equal(TASTEN.sprit.bild(keinFlug).unter, "kein Flug");
  // Kabine bleibt bedienbar, auch ohne Flug.
  assert.equal(TASTEN.kabine_stumm.bild(keinFlug).ton, "ctl");
  assert.equal(TASTEN.musik_pegel.bild(keinFlug).wert, "20 %");
});

test("ein Client ohne Kabinen-Route zeigt das an statt zu raten", () => {
  const alt = { ...voll, kabine: null };
  assert.equal(TASTEN.kabine_stumm.bild(alt).unter, "Client zu alt");
});

test("Beispielflug im Reiseflug", () => {
  const b = (id) => TASTEN[id].bild(voll);
  assert.equal(b("phase").wert, "Reiseflug");
  assert.equal(b("phase").unter, "FL350");
  assert.equal(b("phase").stufen, 2);
  assert.equal(b("phase").zurueck, true);
  assert.equal(b("flug").unter, "EDDF›LEPA");
  assert.equal(b("gates").wert, "B44›–");
  assert.equal(b("block").wert, "1:42");
  assert.equal(b("luft").wert, "1:21");
  assert.equal(b("sprit").wert, "6,4 t");
  assert.equal(b("sprit").ton, "neutral");
  assert.equal(b("gewicht").wert, "61,2 t");
  assert.equal(b("wetter_ziel").wert, "240/08G18");
  assert.equal(b("wetter_ziel").unter, "Q1016");
  assert.equal(b("bahn").wert, "24L");
  assert.equal(b("landung").wert, "—");
  assert.equal(b("tempo").wert, "468 kt");
  assert.equal(b("tempo").unter, "IAS 271 kt");
  assert.equal(b("strecke").wert, "512 nm");
  assert.equal(b("hoppie").unter, "1 offen");
  assert.equal(b("hoppie").ton, "warn");
  assert.equal(b("ansage").wert, "Sinkflug");
  assert.equal(b("ansage").unter, "ca. 15:05Z");
  assert.equal(b("utc").wert, "13:42Z");
  assert.equal(b("hinweis").wert, "OK");
});

test("Hinweis zeigt immer nur die wichtigste Meldung", () => {
  const alles = { ...status, live: null, paused_since: "x", abgabe_sperre: "y", connection_state: "failing", divert_hint: { divert_to: "LEVC" } };
  assert.equal(hinweis(alles).wert, "Sim weg");
  assert.equal(hinweis({ ...alles, live: {} }).wert, "Pause");
  assert.equal(hinweis({ ...alles, live: {}, paused_since: null }).wert, "Abgabe");
  assert.equal(hinweis({ ...alles, live: {}, paused_since: null, abgabe_sperre: null }).wert, "Server");
  assert.equal(hinweis({ ...status, divert_hint: { divert_to: "LEVC" } }).wert, "LEVC");
  assert.equal(hinweis(status).ton, "ok");
});

test("Sprit wird gelb nahe der Reserve und rot darunter", () => {
  const mit = (kg) => TASTEN.sprit.bild({ ...voll, status: { ...status, sim_fuel_kg: kg } }).ton;
  assert.equal(mit(2400), "warn");
  assert.equal(mit(2000), "crit");
});

test("Musik stumm ist verlustfrei aus live/gespeichert ablesbar", () => {
  assert.equal(musikStumm(kabine), false);
  assert.equal(musikStumm({ ...kabine, musik_live: 0 }), true);
  // Steht die Einstellung selbst auf 0, ist das nicht „stumm“, sondern leise.
  assert.equal(musikStumm({ ...kabine, musik_live: 0, musik_gespeichert: 0 }), false);
  assert.equal(TASTEN.musik_pegel.bild({ ...voll, kabine: { ...kabine, musik_live: 0 } }).wert, "stumm");
});

test("Lautstärke-Schritte bleiben in 0..1 und auf Zehnteln", () => {
  assert.equal(schritt(0.8, +1), 0.9);
  assert.equal(schritt(0.95, +1), 1);
  assert.equal(schritt(0.05, -1), 0);
  assert.equal(schritt(0.2, -1), 0.1);
  assert.equal(schritt(0.30000000000000004, +1), 0.4);
});

test("nächste Ansage wartet aufs Anschnallzeichen", () => {
  const b = TASTEN.ansage.bild({ ...voll, kabine: { ...kabine, hinweis: "anschnallzeichen" } });
  assert.equal(b.ton, "warn");
  assert.equal(b.icon, "belt");
  assert.equal(TASTEN.ansage.bild({ ...voll, kabine: { ...kabine, stumm: true } }).wert, "stumm");
});

test("Landebewertung erst nach der Landung, Farbe wie im MSFS-Panel", () => {
  assert.equal(TASTEN.bewertung.bild(voll).wert, "—");
  const gelandet = { ...voll, status: { ...status, landing_at: "2026-10-23T15:30:00Z" } };
  assert.equal(TASTEN.bewertung.bild(gelandet).unter, "wird bewertet");
  const d = (score_label) => TASTEN.bewertung.bild({ ...gelandet, debrief: { score_numeric: 92, grade_letter: "A", score_label } });
  assert.equal(d("smooth").wert, "A");
  assert.equal(d("smooth").unter, "92/100");
  assert.equal(d("smooth").ton, "ok");
  assert.equal(d("firm").ton, "warn");
  assert.equal(d("hard").ton, "crit");
});

test("Höhe kommt im normalen Flug aus live, nicht aus last_known", () => {
  // Wie der Client es ohne Wiederaufnahme liefert: last_known_* fehlt ganz.
  assert.equal(flughoehe({ live: { altitude_msl_ft: 35180, altitude_pressure_ft: 35012 } }), 35012);
  assert.equal(flughoehe({ live: { altitude_msl_ft: 3215, altitude_pressure_ft: 3100 } }), 3215);
  assert.equal(flughoehe({ live: null, last_known_alt_ft: 12000 }), 12000);
  assert.equal(flughoehe({ live: null }), null);
  const sprit = TASTEN.sprit.bild({ ...voll, status: { ...status, sim_fuel_kg: null } });
  assert.equal(sprit.wert, "6,4 t");
});

test("Formatierungen", () => {
  assert.equal(hoehe(3215), "3.200 ft");
  assert.equal(hoehe(9960), "10.000 ft");
  assert.equal(hoehe(35012), "FL350");
  assert.equal(wind({ wind_speed_kt: 0 }), "still");
  assert.equal(wind({ wind_direction_deg: 0, wind_speed_kt: 5 }), "360/05");
  assert.equal(wind({ wind_direction_deg: null, wind_speed_kt: 3 }), "VRB/03");
  assert.equal(dauer(null, null, jetzt), null);
  assert.equal(dauer("2026-10-23T13:00:00Z", "2026-10-23T12:00:00Z", jetzt), null);
});

test("jede Taste übersteht fehlende Felder", () => {
  const leer = { ...voll, status: { phase: "boarding" }, kabine: { an: true }, hoppie: {} };
  for (const [id, t] of Object.entries(TASTEN)) {
    const b = t.bild(leer);
    assert.ok(typeof b.label === "string", id);
    assert.ok(!String(b.wert ?? "").includes("undefined") && !String(b.unter ?? "").includes("undefined"), `${id}: ${b.wert} ${b.unter}`);
    assert.ok(!String(b.wert ?? "").includes("NaN") && !String(b.unter ?? "").includes("NaN"), `${id}: ${b.wert} ${b.unter}`);
  }
});

test("Tastenbilder sind sauberes SVG, auch mit Sonderzeichen", () => {
  const svg = tasteSvg({ label: "A<B", wert: "x&y", unter: '"z"', ton: "warn", icon: "warn" });
  assert.match(svg, /^<svg [^>]*>.*<\/svg>$/);
  assert.ok(!svg.includes("A<B") && svg.includes("A&lt;B") && svg.includes("x&amp;y"));
});

test("Profile, Regler und Manifest passen zueinander", () => {
  const bekannt = new Set([...Object.keys(TASTEN), ...Object.keys(REGLER)]);
  for (const p of PROFILE) {
    for (const id of [...p.tasten.flat(), ...(p.regler ?? [])]) if (id) assert.ok(bekannt.has(id), `${p.datei}: ${id}`);
  }
  assert.deepEqual(PROFILE.map((p) => p.tasten.map((z) => z.length)), [[5, 5, 5], [8, 8, 8, 8], [4, 4]]);
  for (const id of [...INFO_SEITEN, ...WETTER_SEITEN]) assert.ok(TASTEN[id], id);

  const manifest = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/manifest.json"), "utf8"));
  const imManifest = new Set(manifest.Actions.map((a) => a.UUID));
  assert.deepEqual([...imManifest].sort(), [...bekannt].map(uuid).sort(), "npm run erzeugen vergessen?");
});

test("lange Texte werden kleiner statt aus der Taste zu laufen (Feldbefund Windows)", () => {
  // Kurze Werte behalten die volle Größe …
  assert.equal(passend("0:37", 40, 118), 40);
  // … die abgeschnittenen vom Foto werden so klein, dass sie mit Arial passen.
  for (const [text, max] of [["Reiseflug", 30], ["19:09Z", 40], ["360/03G18", 30], ["AIB 4417", 30]]) {
    const g = passend(text, max, 118);
    assert.ok(g < max, `${text}: ${g}`);
    assert.ok(g >= 18, `${text} unlesbar klein: ${g}`);
  }
  const svg = tasteSvg({ label: "Phase", wert: "Reiseflug", unter: "FL380", ton: "ok", icon: "plane" });
  assert.match(svg, /font-size="2\d"[^>]*>Reiseflug</);
});

test("Regler-Feld am StreamDeck +: eigenes Bild statt $B1, Werte passen hinein", () => {
  const svg = reglerSvg({ label: "Wetter 2/3 · YBBN", icon: "wind", wert: "160/12G22", unter: "Q1013" });
  assert.match(svg, /^<svg [^>]*width="200" height="100"/);
  assert.ok(!svg.includes("…"));
  const manifest = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/manifest.json"), "utf8"));
  for (const a of manifest.Actions.filter((x) => x.Controllers.includes("Encoder"))) assert.equal(a.Encoder.layout, "layouts/regler.json");
  const layout = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/layouts/regler.json"), "utf8"));
  assert.deepEqual(layout.items, [{ key: "bild", type: "pixmap", rect: [0, 0, 200, 100] }]);
});
