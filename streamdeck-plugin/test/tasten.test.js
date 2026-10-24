import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { TASTEN, INFO_SEITEN, WETTER_SEITEN, hinweis, musikStumm, schritt, dauer, hoehe, wind, flughoehe } from "../src/tasten.js";
import { REGLER, PROFILE, uuid, profilFuer } from "../src/belegung.js";
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
    if (id === "utc" || id === "oeffnen") continue; // brauchen AeroACARS nicht
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
  // CDM nutzt dieselbe neue Befehlsroute (Feldbefund: blau „öffnen“, ging aber nicht).
  assert.equal(TASTEN.cdm.bild(alt).unter, "Client zu alt");
  assert.equal(TASTEN.cdm.bild(voll).ton, "ctl");
});

test("Beispielflug im Reiseflug", () => {
  const b = (id) => TASTEN[id].bild(voll);
  assert.equal(b("phase").wert, "Reiseflug");
  assert.equal(b("phase").unter, "FL350");
  assert.equal(b("phase").stufen, 2);
  assert.equal(b("flug").unter, "EDDF›LEPA");
  // Zielgate erst nach der Landung (Thomas 23.10.: vorher geraten).
  assert.equal(b("gates").wert, "B44");
  assert.equal(TASTEN.gates.bild({ ...voll, status: { ...status, phase: "blocks_on", arr_gate: "12" } }).wert, "12");
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

test("Ansagen-Tasten zeigen Stumm und sagen „drücken“ statt „halten“", () => {
  const stumm = { ...voll, kabine: { ...voll.kabine, stumm: true } };
  assert.equal(TASTEN.ansage_pegel.bild(stumm).wert, "stumm");
  assert.equal(TASTEN.ansage_pegel.bild(stumm).unter, "drücken = an");
  assert.equal(TASTEN.kabine_stumm.bild(voll).unter, "drücken = stumm");
  for (const t of Object.values(TASTEN)) assert.ok(!/halten/i.test(t.tip) || t === TASTEN.musik_pegel, t.name);
});

test("Profile, Regler und Manifest passen zueinander", () => {
  const bekannt = new Set([...Object.keys(TASTEN), ...Object.keys(REGLER)]);
  const mitKachel = new Set([...bekannt, "kachel"]); // Entwurf 2: Kachel-Aktion
  for (const p of PROFILE) for (const id of p.regler ?? []) assert.ok(bekannt.has(id), `${p.datei}: ${id}`);
  assert.deepEqual(PROFILE.map((p) => [p.spalten, p.reihen]), [[5, 3], [8, 4], [4, 2]]);
  assert.ok(!bekannt.has("hoppie"), "Hoppie ist entfernt");
  for (const id of [...INFO_SEITEN, ...WETTER_SEITEN]) assert.ok(TASTEN[id], id);

  const manifest = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/manifest.json"), "utf8"));
  const imManifest = new Set(manifest.Actions.map((a) => a.UUID));
  assert.deepEqual([...imManifest].sort(), [...mitKachel].map(uuid).sort(), "npm run erzeugen vergessen?");
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

test("„AeroACARS öffnen“ findet das Profil im Manifest je Gerätetyp", () => {
  const manifest = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/manifest.json"), "utf8"));
  for (const typ of [0, 2, 7]) {
    const name = profilFuer(typ);
    assert.ok(manifest.Profiles.some((p) => p.Name === name && p.DeviceType === typ), `${typ}: ${name}`);
  }
  assert.equal(profilFuer(1), null); // Mini: kein Profil
});

test("jede Ansicht jeder Taste zeichnet ohne Lücken, voll und mit fehlenden Feldern", () => {
  const leer = { ...voll, status: { phase: "boarding" }, kabine: { an: true }, hoppie: {} };
  const mitLive = { ...voll, status: { ...status, live: { gs_kt: 300, ias_kt: 250, vertical_speed_fpm: -1234, wind_dir_deg: 250, wind_speed_kt: 45 }, landing_rate_fpm: -142, landing_g_force: 1.18,
    arr_metar_decoded: { ...status.arr_metar_decoded, visibility_m: 9999, cloud_layers: [{ cover: "BKN", base_ft: 2500 }], temperature_c: 12.4, dewpoint_c: 8 } } };
  for (const [id, t] of Object.entries(TASTEN)) {
    for (let i = 0; i < (t.ansichten ?? 1); i++) {
      for (const s of [leer, mitLive]) {
        const b = t.bild(s, i);
        const text = `${b.label} ${b.wert ?? ""} ${b.unter ?? ""}`;
        assert.ok(!/undefined|NaN|null/.test(text), `${id}/${i}: ${text}`);
      }
    }
  }
  assert.equal(TASTEN.tempo.bild(mitLive, 1).wert, "-1230");
  assert.equal(TASTEN.tempo.bild(mitLive, 2).wert, "250/45");
  assert.equal(TASTEN.wetter_ziel.bild(mitLive, 1).wert, "10 km+");
  assert.equal(TASTEN.wetter_ziel.bild(mitLive, 1).unter, "BKN 2500 ft");
  assert.equal(TASTEN.wetter_ziel.bild(mitLive, 2).wert, "12°/8°");
  assert.equal(TASTEN.landung.bild(mitLive, 1).wert, "1,18 G");
  assert.equal(TASTEN.block.bild(voll, 1).wert, "12:00Z");
});

test("jede Zielseite für den langen Druck erlaubt der Client (DECK_SEITEN in panel_server.rs)", () => {
  const rust = fs.readFileSync(path.resolve(import.meta.dirname, "../../client/src-tauri/src/panel_server.rs"), "utf8");
  const liste = rust.match(/const DECK_SEITEN: &\[&str\] = &\[([^\]]*)\]/);
  assert.ok(liste, "DECK_SEITEN nicht gefunden");
  const erlaubt = new Set([...liste[1].matchAll(/"([a-z]+)"/g)].map((m) => m[1]));
  const genutzt = Object.entries(TASTEN).filter(([, t]) => t.seite);
  assert.ok(genutzt.length >= 15);
  for (const [id, t] of genutzt) assert.ok(erlaubt.has(t.seite), `${id}: ${t.seite}`);
  // Die App kennt diese Tabs (Tab-Typ in App.tsx).
  const app = fs.readFileSync(path.resolve(import.meta.dirname, "../../client/src/App.tsx"), "utf8");
  for (const seite of erlaubt) assert.ok(app.includes(`"${seite}"`), seite);
});

test("METAR ohne Windangabe: Sicht, Wolken und Temperatur bleiben sichtbar (Codex)", () => {
  const m = { icao: "YBBN", wind_speed_kt: null, qnh_hpa: 1013, visibility_m: 4000, cloud_layers: [], temperature_c: 20, dewpoint_c: 15 };
  const s = { ...voll, status: { ...status, arr_metar_decoded: m } };
  assert.equal(TASTEN.wetter_ziel.bild(s, 0).wert, "–");
  assert.equal(TASTEN.wetter_ziel.bild(s, 0).unter, "Q1013");
  assert.equal(TASTEN.wetter_ziel.bild(s, 1).wert, "4000 m");
  assert.equal(TASTEN.wetter_ziel.bild(s, 2).wert, "20°/15°");
  assert.equal(TASTEN.wetter_ziel.bild({ ...voll, status: { ...status, arr_metar_decoded: null } }, 0).unter, "kein METAR");
});

test("erzeugtes XL-Profil: Kacheln überall, Zurück als Elgato-Profilwechsel mit eigenem Bild", async () => {
  const { execFileSync } = await import("node:child_process");
  const os = await import("node:os");
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "aa-profiltest-"));
  try {
    execFileSync("unzip", ["-q", path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin/profiles/AeroACARS XL.streamDeckProfile"), "-d", tmp]);
    const seiten = [];
    const suche = (d) => { for (const f of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, f.name); if (f.isDirectory()) suche(p); else if (f.name === "manifest.json" && p.includes("/Profiles/") && p.split("/Profiles/").length > 2) seiten.push(p); } };
    suche(tmp);
    const voll = seiten.map((p) => [p, JSON.parse(fs.readFileSync(p, "utf8"))]).find(([, m]) => Object.keys(m.Controllers[0].Actions ?? {}).length);
    assert.ok(voll, "keine belegte Seite");
    const [datei, m] = voll;
    const tasten = m.Controllers[0].Actions;
    assert.equal(Object.keys(tasten).length, 32);
    const zurueck = tasten["0,3"];
    assert.equal(zurueck.UUID, "com.elgato.streamdeck.profile.rotate");
    assert.deepEqual(zurueck.Settings, { DeviceUUID: "", PageIndex: 0, ProfileUUID: "" });
    assert.ok(fs.existsSync(path.join(path.dirname(datei), zurueck.States[0].Image)), "Bild der Zurück-Taste fehlt im Profil");
    assert.equal(Object.values(tasten).filter((a) => a.UUID === uuid("kachel")).length, 31);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
