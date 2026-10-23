// Rendert die Szenen mit dem echten Renderer (resvg + B612) als PNG.
//   node skripte/vorschau.mjs <ausgabeordner>
import fs from "node:fs";
import path from "node:path";
import { initWasm, Resvg } from "@resvg/resvg-wasm";
import { szene, streifen } from "../src/szene.js";
import { PHASEN } from "./vorschau-daten.mjs";

const wurzel = path.resolve(import.meta.dirname, "..");
await initWasm(fs.readFileSync(`${wurzel}/node_modules/@resvg/resvg-wasm/index_bg.wasm`));
const fonts = ["B612-Regular", "B612-Bold"].map((f) => fs.readFileSync(`${wurzel}/eu.german-sky-group.aeroacars.sdPlugin/fonts/${f}.ttf`));
const png = (svg, breite) => new Resvg(svg, { font: { fontBuffers: fonts, defaultFontFamily: "B612", loadSystemFonts: false }, fitTo: breite ? { mode: "width", value: breite } : undefined }).render().asPng();
const ziel = process.argv[2];
fs.mkdirSync(ziel, { recursive: true });
const t = Number(process.argv[3] ?? 6);
for (const [name, s] of Object.entries(PHASEN)) {
  for (const g of ["xl", "15", "plus"]) fs.writeFileSync(`${ziel}/${g}-${name}.png`, png(szene(g, s, t).svg));
  fs.writeFileSync(`${ziel}/streifen-${name}.png`, png(streifen(s, t)));
}
console.log("ok", fs.readdirSync(ziel).length);
