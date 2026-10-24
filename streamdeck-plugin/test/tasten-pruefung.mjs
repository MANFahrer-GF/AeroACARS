// Prüflauf aller Tasten: jede Einzeltaste und jede Kachel der drei Profile
// wird mit dem gebündelten Plugin kurz und lang gedrückt (Thomas 24.10.:
// „alle Tasten im Plugin auf Funktion und Vollständigkeit prüfen“).
//   node test/tasten-pruefung.mjs   → Tabelle je Taste; Exit 1 bei Fehler.
import { spawn } from "node:child_process";
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { WebSocketServer } from "ws";
import { PHASEN } from "../skripte/vorschau-daten.mjs";

const ORDNER = path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin");
const manifest = JSON.parse(fs.readFileSync(`${ORDNER}/manifest.json`, "utf8"));
let phase = "reiseflug";
const befehle = [];
const aeroacars = http.createServer((req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", () => {
    const s = PHASEN[phase];
    if (req.method === "POST") befehle.push(`${req.url.replace("/panel/cmd/", "")} ${body}`);
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify(req.url === "/panel/status" ? s.status : s.kabine));
  });
});
await new Promise((ok, f) => aeroacars.once("error", f).listen(47847, "127.0.0.1", ok));

const meldungen = []; // showAlert / showOk / switchToProfile
const bilder = new Map();
let ws;
const wss = new WebSocketServer({ port: 0 });
const bereit = new Promise((ok) => wss.on("connection", (w) => {
  ws = w;
  w.on("message", (m) => {
    const d = JSON.parse(m);
    if (d.event === "registerPlugin") ok();
    if (d.event === "setImage") bilder.set(d.context, d.payload.image);
    if (["showAlert", "showOk", "switchToProfile"].includes(d.event)) meldungen.push(`${d.event}${d.payload?.profile ? `(${d.payload.profile})` : ""}`);
  });
}));
const GERAETE = [
  { id: "xl1", name: "XL", size: { columns: 8, rows: 4 }, type: 2 },
  { id: "d15", name: "15", size: { columns: 5, rows: 3 }, type: 0 },
  { id: "plus1", name: "Plus", size: { columns: 4, rows: 2 }, type: 7 },
];
const info = { application: { font: "", language: "de", platform: "mac", platformVersion: "15", version: "7.1.0.0" }, plugin: { uuid: "eu.german-sky-group.aeroacars", version: manifest.Version }, devicePixelRatio: 2, colors: {}, devices: GERAETE };
const kind = spawn(process.execPath, ["bin/plugin.js", "-port", String(wss.address().port), "-pluginUUID", "eu.german-sky-group.aeroacars", "-registerEvent", "registerPlugin", "-info", JSON.stringify(info)], { cwd: ORDNER, stdio: "ignore" });
await bereit;
const warte = (ms) => new Promise((ok) => setTimeout(ok, ms));
const senden = (event, action, context, device, c, r) =>
  ws.send(JSON.stringify({ event, action, context, device, payload: { controller: "Keypad", coordinates: { column: c, row: r }, isInMultiAction: false, settings: {}, state: 0 } }));

async function druecken(action, context, device, c, r, halten) {
  befehle.length = 0;
  meldungen.length = 0;
  const vorher = bilder.get(context);
  senden("keyDown", action, context, device, c, r);
  await warte(halten);
  senden("keyUp", action, context, device, c, r);
  await warte(500);
  const neuesBild = action.endsWith(".kachel") || bilder.get(context) === vorher ? [] : ["Bild wechselt"];
  return [...befehle, ...meldungen, ...neuesBild].join(" | ") || "—";
}

const fehler = [];
const zeile = (name, kurz, lang) => console.log(`${name.padEnd(34)} kurz: ${kurz}\n${"".padEnd(34)} lang: ${lang}`);

// ---- Einzeltasten ---------------------------------------------------------
const einzel = manifest.Actions.filter((a) => a.Controllers.includes("Keypad") && !a.UUID.endsWith(".kachel"));
einzel.forEach((a, i) => senden("willAppear", a.UUID, `e${i}`, "xl1", i % 8, Math.floor(i / 8)));
await warte(2500);
console.log(`== Einzeltasten (${einzel.length}) – Reiseflug`);
for (const [i, a] of einzel.entries()) {
  if (!bilder.get(`e${i}`)) fehler.push(`${a.Name}: kein Bild`);
  const kurz = await druecken(a.UUID, `e${i}`, "xl1", i % 8, Math.floor(i / 8), 80);
  const lang = await druecken(a.UUID, `e${i}`, "xl1", i % 8, Math.floor(i / 8), 1300);
  zeile(a.Name, kurz, lang);
  if (kurz === "—" && lang === "—") fehler.push(`${a.Name}: tut nichts`);
  if (/showAlert/.test(kurz + lang)) fehler.push(`${a.Name}: Warnzeichen`);
}
einzel.forEach((a, i) => senden("willDisappear", a.UUID, `e${i}`, "xl1", i % 8, Math.floor(i / 8)));

// ---- Kacheln je Gerät und Phase ------------------------------------------
const KACHEL = "eu.german-sky-group.aeroacars.kachel";
for (const p of ["boarding", "reiseflug", "anflug", "gelandet"]) {
  phase = p;
  for (const g of GERAETE) {
    const { columns, rows } = g.size;
    for (let r = 0; r < rows; r++) for (let c = 0; c < columns; c++) senden("willAppear", KACHEL, `${g.id}-${c}-${r}`, g.id, c, r);
    await warte(2500);
    console.log(`\n== Kacheln ${g.name} – ${p}`);
    for (let r = 0; r < rows; r++) for (let c = 0; c < columns; c++) {
      const ctx = `${g.id}-${c}-${r}`;
      if (!bilder.get(ctx)) fehler.push(`${g.name} ${p} ${c},${r}: kein Bild`);
      const kurz = await druecken(KACHEL, ctx, g.id, c, r, 80);
      const lang = await druecken(KACHEL, ctx, g.id, c, r, 1300);
      zeile(`${c},${r}`, kurz, lang);
      if (/showAlert/.test(kurz + lang)) fehler.push(`${g.name} ${p} ${c},${r}: Warnzeichen`);
    }
    for (let r = 0; r < rows; r++) for (let c = 0; c < columns; c++) senden("willDisappear", KACHEL, `${g.id}-${c}-${r}`, g.id, c, r);
  }
}

kind.kill();
wss.close();
aeroacars.close();
console.log(fehler.length ? `\n${fehler.length} Fehler:\n${fehler.join("\n")}` : "\nalle Tasten in Ordnung");
process.exit(fehler.length ? 1 : 0);
