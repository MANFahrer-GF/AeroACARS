// Durchlauf: das gebündelte Plugin gegen eine nachgebaute StreamDeck-Software
// (WebSocket) und ein nachgebautes AeroACARS (Panel-Server auf 47847).
//   node test/durchlauf.mjs   → druckt, was zurückkam; Exit 1 bei Fehler.
import { spawn } from "node:child_process";
import http from "node:http";
import path from "node:path";
import fs from "node:fs";
import { WebSocketServer } from "ws";
import { PHASEN } from "../skripte/vorschau-daten.mjs";

const ORDNER = path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin");
const s = PHASEN.reiseflug;
const befehle = [];
const aeroacars = http.createServer((req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", () => {
    if (req.method === "POST") befehle.push([req.url, req.headers["x-aeroacars-deck"], req.headers.origin ?? null, body]);
    const antw = req.url === "/panel/status" ? s.status : req.url === "/panel/kabine" ? s.kabine : req.url.startsWith("/panel/cmd/") ? s.kabine : null;
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify(antw));
  });
});
await new Promise((ok, fehler) => aeroacars.once("error", fehler).listen(47847, "127.0.0.1", ok));

const wss = new WebSocketServer({ port: 0 });
const port = wss.address().port;
const bilder = new Map();
let feedback = 0;
wss.on("connection", (ws) => {
  ws.on("message", (m) => {
    const d = JSON.parse(m);
    if (d.event === "registerPlugin") {
      const UUID = "eu.german-sky-group.aeroacars.kachel";
      for (let r = 0; r < 4; r++) for (let c = 0; c < 8; c++)
        ws.send(JSON.stringify({ event: "willAppear", action: UUID, context: `k${c}-${r}`, device: "xl1", payload: { controller: "Keypad", coordinates: { column: c, row: r }, isInMultiAction: false, settings: {}, state: 0 } }));
      ws.send(JSON.stringify({ event: "willAppear", action: "eu.german-sky-group.aeroacars.regler-ansage", context: "rad0", device: "plus1", payload: { controller: "Encoder", coordinates: { column: 0, row: 0 }, settings: {} } }));
      // Druck auf „Ansagen +“ (XL Spalte 3, Zeile 3)
      setTimeout(() => {
        ws.send(JSON.stringify({ event: "keyDown", action: UUID, context: "k3-3", device: "xl1", payload: { coordinates: { column: 3, row: 3 }, settings: {}, state: 0, isInMultiAction: false } }));
        ws.send(JSON.stringify({ event: "keyUp", action: UUID, context: "k3-3", device: "xl1", payload: { coordinates: { column: 3, row: 3 }, settings: {}, state: 0, isInMultiAction: false } }));
        // Kurzer Druck auf das Ansagen-Rad des + → Kabine stumm.
        const RAD = "eu.german-sky-group.aeroacars.regler-ansage";
        ws.send(JSON.stringify({ event: "dialDown", action: RAD, context: "rad0", device: "plus1", payload: { controller: "Encoder", coordinates: { column: 0, row: 0 }, settings: {} } }));
        ws.send(JSON.stringify({ event: "dialUp", action: RAD, context: "rad0", device: "plus1", payload: { controller: "Encoder", coordinates: { column: 0, row: 0 }, settings: {} } }));
      }, 2500);
    }
    if (d.event === "setImage") bilder.set(d.context, d.payload.image);
    if (d.event === "setFeedback") feedback++;
  });
});
const info = { application: { font: "", language: "de", platform: "mac", platformVersion: "15", version: "7.1.0.0" }, plugin: { uuid: "eu.german-sky-group.aeroacars", version: "2.0.0.0" }, devicePixelRatio: 2, colors: {},
  devices: [{ id: "xl1", name: "XL", size: { columns: 8, rows: 4 }, type: 2 }, { id: "plus1", name: "Plus", size: { columns: 4, rows: 2 }, type: 7 }] };
const kind = spawn(process.execPath, ["bin/plugin.js", "-port", String(port), "-pluginUUID", "eu.german-sky-group.aeroacars", "-registerEvent", "registerPlugin", "-info", JSON.stringify(info)], { cwd: ORDNER, stdio: ["ignore", "pipe", "pipe"] });
let log = "";
kind.stdout.on("data", (d) => (log += d));
kind.stderr.on("data", (d) => (log += d));
await new Promise((ok) => setTimeout(ok, 4500));
kind.kill();
wss.close();
aeroacars.close();
const pngs = [...bilder.values()].filter((v) => v.startsWith("data:image/png;base64,"));
const verschieden = new Set(pngs).size;
const probe = pngs[0] ? Buffer.from(pngs[0].split(",")[1], "base64") : null;
console.log(`Kacheln mit Bild: ${pngs.length}/32, verschiedene Bilder: ${verschieden}, Streifen-Updates: ${feedback}`);
console.log(`erste Kachel: ${probe ? `${probe.readUInt32BE(16)}x${probe.readUInt32BE(20)} PNG` : "keine"}`);
console.log("Befehle:", JSON.stringify(befehle));
if (pngs[0]) fs.writeFileSync(process.argv[2] ?? "/dev/null", probe);
const ok = pngs.length === 32 && verschieden > 20 && feedback > 0 && befehle.some(([u, k, o]) => u === "/panel/cmd/kabinen_lautstaerke" && k === "1" && o === null) && befehle.some(([u, , , b]) => u === "/panel/cmd/kabinen_stumm" && b.includes('"stumm":true'));
if (!ok || process.env.LOG) console.log("LOG:\n" + log.slice(-3000));
process.exit(ok ? 0 : 1);
