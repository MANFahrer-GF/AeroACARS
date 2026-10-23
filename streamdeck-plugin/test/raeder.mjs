// Prüflauf der vier Räder des StreamDeck + mit dem gebündelten Plugin gegen
// nachgebaute StreamDeck-Software und AeroACARS (Thomas 23.10.: „bei den
// Drehreglern noch irgendwelche Probleme? Bitte prüfen“).
//   node test/raeder.mjs   → Tabelle je Schritt; Exit 1 bei Fehler.
import { spawn } from "node:child_process";
import http from "node:http";
import path from "node:path";
import zlib from "node:zlib";
import { WebSocketServer } from "ws";
import { PHASEN } from "../skripte/vorschau-daten.mjs";

const ORDNER = path.resolve(import.meta.dirname, "../eu.german-sky-group.aeroacars.sdPlugin");
const s = PHASEN.reiseflug;
const befehle = [];
const aeroacars = http.createServer((req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", () => {
    if (req.method === "POST") befehle.push([req.url.replace("/panel/cmd/", ""), body]);
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify(req.url === "/panel/status" ? s.status : s.kabine));
  });
});
await new Promise((ok, f) => aeroacars.once("error", f).listen(47847, "127.0.0.1", ok));

/** Pixel (x, y) eines PNG aus unserem eigenen Kodierer (RGBA, Filter 0). */
function pixel(url, x, y) {
  const png = Buffer.from(url.split(",")[1], "base64");
  const b = png.readUInt32BE(16);
  const start = png.indexOf("IDAT") + 4;
  const roh = zlib.inflateSync(png.subarray(start, start + png.readUInt32BE(start - 8)));
  const i = y * (b * 4 + 1) + 1 + x * 4;
  return [...roh.subarray(i, i + 3)];
}
/** Hat das Feld den cyanfarbenen Rahmen der Rad-Anzeige? */
// Rahmen der Anzeige liegt bei x = 4 (cyan, halb deckend); das Band ist dort dunkel.
const hatAnzeige = (url) => { const [r, g, bl] = pixel(url, 4, 50); return bl > 80 && g > 60 && r < 90; };

const RAEDER = [["ansage", "regler-ansage"], ["musik", "regler-musik"], ["info", "regler-info"], ["wetter", "regler-wetter"]];
const letztes = {};
let ws;
const wss = new WebSocketServer({ port: 0 });
const bereit = new Promise((ok) => wss.on("connection", (w) => {
  ws = w;
  w.on("message", (m) => {
    const d = JSON.parse(m);
    if (d.event === "registerPlugin") {
      RAEDER.forEach(([, id], c) => w.send(JSON.stringify({ event: "willAppear", action: `eu.german-sky-group.aeroacars.${id}`, context: `rad${c}`, device: "plus1", payload: { controller: "Encoder", coordinates: { column: c, row: 0 }, settings: {} } })));
      ok();
    }
    if (d.event === "setFeedback") letztes[d.context] = d.payload.bild;
  });
}));
const info = { application: { font: "", language: "de", platform: "mac", platformVersion: "15", version: "7.1.0.0" }, plugin: { uuid: "eu.german-sky-group.aeroacars", version: "2.1.3.0" }, devicePixelRatio: 2, colors: {}, devices: [{ id: "plus1", name: "Plus", size: { columns: 4, rows: 2 }, type: 7 }] };
const kind = spawn(process.execPath, ["bin/plugin.js", "-port", String(wss.address().port), "-pluginUUID", "eu.german-sky-group.aeroacars", "-registerEvent", "registerPlugin", "-info", JSON.stringify(info)], { cwd: ORDNER, stdio: "ignore" });
await bereit;
const warte = (ms) => new Promise((ok) => setTimeout(ok, ms));
const senden = (event, c, extra = {}) => ws.send(JSON.stringify({ event, action: `eu.german-sky-group.aeroacars.${RAEDER[c][1]}`, context: `rad${c}`, device: "plus1", payload: { controller: "Encoder", coordinates: { column: c, row: 0 }, settings: {}, ...extra } }));
await warte(1500);

const fehler = [];
const pruefe = (name, bedingung, detail = "") => { console.log(`${bedingung ? "ok  " : "FEHLER"} ${name}${detail ? ` — ${detail}` : ""}`); if (!bedingung) fehler.push(name); };
const anzeigen = () => [0, 1, 2, 3].map((c) => (letztes[`rad${c}`] ? hatAnzeige(letztes[`rad${c}`]) : null));
const nurBei = (c) => anzeigen().every((a, i) => a === (i === c));

pruefe("Ruhe: kein Feld zeigt eine Rad-Anzeige", anzeigen().every((a) => a === false), JSON.stringify(anzeigen()));

for (const c of [0, 1, 2, 3]) {
  befehle.length = 0;
  senden("dialRotate", c, { ticks: 2, pressed: false });
  await warte(600);
  pruefe(`Rad ${c + 1} (${RAEDER[c][0]}) drehen: Anzeige nur über Rad ${c + 1}`, nurBei(c), JSON.stringify(anzeigen()));
  await warte(900);
  const erwartet = c === 0 ? "kabinen_lautstaerke" : c === 1 ? "kabinen_musik_lautstaerke" : null;
  if (erwartet) {
    const live = befehle.find(([n, b]) => n === erwartet && b.includes('"speichern":false'));
    const gesp = befehle.find(([n, b]) => n === erwartet && b.includes('"speichern":true'));
    pruefe(`Rad ${c + 1} drehen: Lautstärke live gesetzt und gespeichert`, !!live && !!gesp, JSON.stringify(befehle));
    const wert = live && JSON.parse(live[1]).lautstaerke;
    const basis = c === 0 ? s.kabine.lautstaerke_live : s.kabine.musik_live;
    pruefe(`Rad ${c + 1} drehen: +2 Rasten = +4 %`, Math.abs(wert - (basis + 0.04)) < 0.001, `${basis} → ${wert}`);
  } else pruefe(`Rad ${c + 1} drehen: kein Befehl an AeroACARS`, befehle.length === 0, JSON.stringify(befehle));
  await warte(2600); // Anzeige blendet zurück
  pruefe(`Rad ${c + 1}: nach 2,5 s wieder Streckenband`, anzeigen().every((a) => a === false), JSON.stringify(anzeigen()));
}

// Drücken
const druck = async (c, halten = 0) => { befehle.length = 0; senden("dialDown", c); await warte(halten || 80); senden("dialUp", c); await warte(700); return [...befehle]; };
let b = await druck(0);
pruefe("Rad 1 kurz drücken: Kabine stumm", b.some(([n, x]) => n === "kabinen_stumm" && x.includes('"stumm":true')), JSON.stringify(b));
pruefe("Rad 1 drücken: Anzeige über Rad 1", nurBei(0), JSON.stringify(anzeigen()));
b = await druck(1);
pruefe("Rad 2 kurz drücken: Musik stumm (live 0, nicht gespeichert)", b.some(([n, x]) => n === "kabinen_musik_lautstaerke" && x.includes('"lautstaerke":0,') && x.includes('"speichern":false')), JSON.stringify(b));
pruefe("Rad 2 drücken: Anzeige über Rad 2", nurBei(1), JSON.stringify(anzeigen()));
b = await druck(1, 1200);
pruefe("Rad 2 halten: Boarding-Musik umschalten, kein Musik-Stumm", b.some(([n]) => n === "kabinen_boarding_musik") && !b.some(([n]) => n === "kabinen_musik_lautstaerke"), JSON.stringify(b));
b = await druck(3);
pruefe("Rad 4 drücken: METAR laden (Ziel)", b.some(([n, x]) => n === "metar_laden" && x.includes('"ort":"ziel"')), JSON.stringify(b));
b = await druck(2);
pruefe("Rad 3 drücken: AeroACARS-Seite öffnen", b.some(([n]) => n === "fenster_zeigen"), JSON.stringify(b));

kind.kill();
wss.close();
aeroacars.close();
console.log(fehler.length ? `\n${fehler.length} Fehler` : "\nalle Räder in Ordnung");
process.exit(fehler.length ? 1 : 0);
