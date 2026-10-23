// Erzeugt aus src/tasten.js alles, was die StreamDeck-Software statisch
// braucht: manifest.json, die Bilder und die drei mitgelieferten Profile.
// Eine Quelle für Namen und Icons — wer eine Taste hinzufügt, ruft
// `npm run erzeugen` und muss nichts von Hand nachziehen.

import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { TASTEN } from "../src/tasten.js";
import { listenIconSvg, icon } from "../src/bild.js";
import { einzelSvg } from "../src/einzel.js";
import { REGLER, PROFILE, PRAEFIX, uuid } from "../src/belegung.js";
import { szene, kachelOrt, KACHEL } from "../src/szene.js";
import { vorbereiten, rastern, pngAusschnitt } from "../src/render.js";
const ORDNER = path.resolve(import.meta.dirname, `../${PRAEFIX}.sdPlugin`);
const VERSION = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../package.json"), "utf8")).version + ".0";

const schreiben = (datei, inhalt) => {
  fs.mkdirSync(path.dirname(datei), { recursive: true });
  fs.writeFileSync(datei, inhalt);
};

// ---- Bilder ---------------------------------------------------------------

await vorbereiten(ORDNER);

const ruhe = { erreichbar: false, status: null, kabine: null, hoppie: null, debrief: null, jetzt: new Date(0) };
for (const [id, t] of Object.entries(TASTEN)) {
  schreiben(`${ORDNER}/imgs/actions/${id}.svg`, listenIconSvg(t.icon));
  // Standardbild (bevor das Plugin zeichnet) im neuen Stil, als PNG mit B612.
  fs.rmSync(`${ORDNER}/imgs/actions/${id}_taste.svg`, { force: true });
  {
    const bild = rastern(einzelSvg(t.bild(ruhe)));
    schreiben(`${ORDNER}/imgs/actions/${id}_taste.png`, pngAusschnitt(bild, 0, 0, bild.breite, bild.hoehe));
  }
}
for (const [id, r] of Object.entries(REGLER)) schreiben(`${ORDNER}/imgs/actions/${id}.svg`, listenIconSvg(r.icon));
schreiben(`${ORDNER}/imgs/actions/kachel.svg`, listenIconSvg("route"));
// Rasterer (resvg als WebAssembly) ins Paket — wird nicht eingecheckt.
fs.mkdirSync(`${ORDNER}/wasm`, { recursive: true });
fs.copyFileSync(path.resolve(import.meta.dirname, "../node_modules/@resvg/resvg-wasm/index_bg.wasm"), `${ORDNER}/wasm/resvg.wasm`);
schreiben(`${ORDNER}/imgs/plugin/kategorie.svg`, listenIconSvg("plane", 28));
// Das Plugin-Symbol (Marketplace, Plugin-Liste) muss PNG sein. macOS'
// `sips` rastert SVG; die Dateien liegen im Repo, nur wer das Symbol
// ändert, braucht dafür einen Mac.
const symbolSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="288" height="288" viewBox="0 0 144 144"><rect width="144" height="144" rx="28" fill="#1b2f4d"/><rect width="144" height="72" rx="28" fill="#ffffff" opacity="0.06"/>${icon("plane", 30, 22, 84, "#8fbaff")}<text x="72" y="128" text-anchor="middle" font-family="Arial Narrow, Arial, sans-serif" font-size="22" font-weight="700" fill="#e9edf2" letter-spacing="1">AeroACARS</text></svg>`;
const symbolDatei = `${ORDNER}/imgs/plugin/symbol.svg`;
schreiben(symbolDatei, symbolSvg);
if (process.platform === "darwin") {
  for (const [name, px] of [["symbol.png", 256], ["symbol@2x.png", 512]]) {
    execFileSync("sips", ["-s", "format", "png", "-z", String(px), String(px), symbolDatei, "--out", `${ORDNER}/imgs/plugin/${name}`], { stdio: "ignore" });
  }
}
fs.rmSync(symbolDatei);

// Eigenes Regler-Layout: ein Bild über das ganze Feld (200 × 100), gezeichnet
// von bild.js/reglerSvg. Elgatos `$B1` schnitt Werte ab und war grau.
schreiben(`${ORDNER}/layouts/regler.json`, JSON.stringify({ id: `${PRAEFIX}.regler`, items: [{ key: "bild", type: "pixmap", rect: [0, 0, 200, 100] }] }, null, 2) + "\n");

// ---- Manifest -------------------------------------------------------------

const aktionen = [
  {
    Name: "AeroACARS-Kachel",
    UUID: uuid("kachel"),
    Icon: "imgs/actions/kachel",
    Tooltip: "Ein Stück des großen AeroACARS-Bildes. Auf jede Taste des Decks legen — die Position bestimmt Inhalt und Bedienung.",
    Controllers: ["Keypad"],
    States: [{ Image: "imgs/actions/kachel", ShowTitle: false }],
    UserTitleEnabled: false,
  },
  ...Object.entries(TASTEN).map(([id, t]) => ({
    Name: t.name,
    UUID: uuid(id),
    Icon: `imgs/actions/${id}`,
    Tooltip: t.tip,
    Controllers: ["Keypad"],
    States: [{ Image: `imgs/actions/${id}_taste`, ShowTitle: false }],
    UserTitleEnabled: false,
  })),
  ...Object.entries(REGLER).map(([id, r]) => ({
    Name: r.name,
    UUID: uuid(id),
    Icon: `imgs/actions/${id}`,
    Tooltip: r.tip,
    Controllers: ["Encoder"],
    Encoder: { layout: "layouts/regler.json", TriggerDescription: r.ausloeser },
    States: [{ Image: `imgs/actions/${id}`, ShowTitle: false }],
    UserTitleEnabled: false,
  })),
];

const manifest = {
  Name: "AeroACARS",
  Version: VERSION,
  Author: "German Sky Group",
  Description: "Flugstatus, Kabinenansagen und Boarding-Musik von AeroACARS auf dem StreamDeck.",
  Category: "AeroACARS",
  CategoryIcon: "imgs/plugin/kategorie",
  Icon: "imgs/plugin/symbol",
  CodePath: "bin/plugin.js",
  SDKVersion: 3,
  Software: { MinimumVersion: "7.1" },
  OS: [
    { Platform: "mac", MinimumVersion: "12" },
    { Platform: "windows", MinimumVersion: "10" },
  ],
  Nodejs: { Version: "24", Debug: "disabled" },
  UUID: PRAEFIX,
  Actions: aktionen,
  Profiles: PROFILE.map((p) => ({ Name: `profiles/${p.datei}`, DeviceType: p.geraetetyp, Readonly: false, DontAutoSwitchWhenInstalled: true })),
};
schreiben(`${ORDNER}/manifest.json`, JSON.stringify(manifest, null, "\t") + "\n");

// ---- Profile --------------------------------------------------------------
// Format wie ein Export aus der StreamDeck-Software 7.x (Version 3.0): eine
// leere Standardseite plus unsere Seite, je Taste "Spalte,Zeile".

function aktion(id, name) {
  return {
    ActionID: randomUUID(),
    LinkedTitle: true,
    Name: name,
    Plugin: { Name: "AeroACARS", UUID: PRAEFIX, Version: VERSION },
    Resources: null,
    Settings: {},
    State: 0,
    States: [{ ShowTitle: false, Title: "", TitleAlignment: "middle" }],
    UUID: uuid(id),
  };
}

// Zurück-Taste: Elgatos eigene Aktion „Profil wechseln“ mit unserem Bild.
// Thomas 23.10.: das Plugin kennt kein fremdes Profil (switchToProfile ohne
// Namen griff nicht); so wählt der Pilot das Ziel einmal in der StreamDeck-
// Software, und das Bild der Szene bleibt erhalten.
const LEER_ZUSTAND = { erreichbar: false, status: null, kabine: null, hoppie: null, debrief: null, jetzt: 0 };
const LAYOUT = { 0: "15", 2: "xl", 7: "plus" };
function zurueckTaste(p) {
  const sz = szene(LAYOUT[p.geraetetyp], LEER_ZUSTAND, 0);
  const ort = Object.entries(sz.aktionen).find(([, a]) => a.bedienung === "zurueck")?.[0];
  if (!ort) return null;
  const [c, r] = ort.split(",").map(Number);
  const { x, y } = kachelOrt(c, r);
  const png = pngAusschnitt(rastern(sz.svg), x, y, KACHEL, KACHEL);
  return { ort, png };
}

function profilWechsel() {
  return {
    ActionID: randomUUID(),
    LinkedTitle: false,
    Name: "Profil wechseln",
    Plugin: { Name: "Switch Profile", UUID: "com.elgato.streamdeck.profile.rotate", Version: "1.0" },
    Resources: null,
    Settings: { DeviceUUID: "", PageIndex: 0, ProfileUUID: "" },
    State: 0,
    States: [{ Image: "Images/zurueck.png", ShowTitle: false, Title: "", TitleAlignment: "middle" }],
    UUID: "com.elgato.streamdeck.profile.rotate",
  };
}

function profilBauen(p) {
  const arbeit = fs.mkdtempSync(path.join(fs.realpathSync(process.env.TMPDIR || "/tmp"), "aa-profil-"));
  const profilId = randomUUID();
  const leer = randomUUID();
  const seite = randomUUID();
  const wurzel = `${arbeit}/Profiles/${profilId}.sdProfile`;
  const tasten = {};
  // Entwurf 2: auf jeder Taste die Kachel — die Szene verteilt die Inhalte.
  for (let r = 0; r < p.reihen; r++) for (let c = 0; c < p.spalten; c++) tasten[`${c},${r}`] = aktion("kachel", "AeroACARS-Kachel");
  const zurueck = zurueckTaste(p);
  if (zurueck) tasten[zurueck.ort] = profilWechsel();
  const regler = {};
  (p.regler ?? []).forEach((id, c) => (regler[`${c},0`] = aktion(id, REGLER[id].name)));
  const steuer = (acts) => [{ Type: "Keypad", Actions: acts }, ...(p.regler ? [{ Type: "Encoder", Actions: regler }] : [])];

  schreiben(`${arbeit}/package.json`, JSON.stringify({ AppVersion: "7.1.0", DeviceModel: p.modell, DeviceSettings: null, FormatVersion: 1, OSType: "Windows", OSVersion: "10.0", RequiredPlugins: [PRAEFIX, "com.elgato.streamdeck.profile.rotate"] }, null, 2));
  schreiben(`${wurzel}/manifest.json`, JSON.stringify({ Device: { Model: p.modell, UUID: "" }, Name: p.name, Pages: { Current: seite, Default: leer, Pages: [seite] }, Version: "3.0" }, null, 2));
  schreiben(`${wurzel}/Profiles/${leer.toUpperCase()}/manifest.json`, JSON.stringify({ Controllers: steuer({}), Icon: "", Name: "" }, null, 2));
  schreiben(`${wurzel}/Profiles/${seite.toUpperCase()}/manifest.json`, JSON.stringify({ Controllers: steuer(tasten), Icon: "", Name: p.name }, null, 2));
  if (zurueck) schreiben(`${wurzel}/Profiles/${seite.toUpperCase()}/Images/zurueck.png`, zurueck.png);

  const ziel = `${ORDNER}/profiles/${p.datei}.streamDeckProfile`;
  fs.mkdirSync(path.dirname(ziel), { recursive: true });
  fs.rmSync(ziel, { force: true });
  execFileSync("zip", ["-q", "-r", "-X", ziel, "package.json", "Profiles"], { cwd: arbeit });
  fs.rmSync(arbeit, { recursive: true, force: true });
}
PROFILE.forEach(profilBauen);

console.log(`manifest.json mit ${aktionen.length} Aktionen, ${PROFILE.length} Profile → ${ORDNER}`);
