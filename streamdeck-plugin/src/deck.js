// Das Deck als eine Szene: Auf jeder Taste liegt dieselbe Aktion „Kachel“;
// ihre Position (Spalte/Zeile) bestimmt, welchen Ausschnitt der Szene sie
// zeigt und was ein Druck tut. Dazu die vier Räder des StreamDeck +, deren
// Feld im Touch-Streifen ein Viertel des Streifens zeigt.
//
// Takt: 4 Bilder pro Sekunde, solange Kacheln sichtbar sind. Gerastert wird
// je Gerät einmal pro Bild; an die Tasten geht nur, was sich geändert hat.

import path from "node:path";
import { fileURLToPath } from "node:url";
import streamDeck, { SingletonAction } from "@elgato/streamdeck";
import { zustand, ausfuehren } from "./befehle.js";
import { befehl } from "./aeroacars.js";
import { szene, streifen, kachelOrt, KACHEL, INFO_RAD } from "./szene.js";
import { vorbereiten, rastern, pngAusschnitt, alsPngUrl } from "./render.js";
import { uuid } from "./belegung.js";
import { musikStumm, schritt } from "./tasten.js";
import { druckPlan, layoutFuer } from "./druckplan.js";

const TAKT_MS = 250;
const LANG_MS = 600;
const HALTEN_MS = 1000;
const DREH_ANZEIGE_MS = 2500;
const SPEICHERN_NACH_MS = 800;

/** Plugin-Ordner (.sdPlugin) — bin/plugin.js liegt eine Ebene darunter. */
const ORDNER = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ---- Geräte ---------------------------------------------------------------

/** Je Gerät: sichtbare Kacheln und Räder, Zustand der Räder. */
const geraete = new Map();

function geraetVon(a) {
  const id = a.device.id;
  if (!geraete.has(id)) {
    geraete.set(id, { layout: layoutFuer(a.device.type), kacheln: new Map(), raeder: new Map(), dreh: null, infoSeite: 0, wetterSeite: 1, aktionen: {}, letzteSvg: "" });
  }
  return geraete.get(id);
}

/** Kachel/Rad abmelden; ein Gerät ohne beides wird vergessen. */
function abmelden(geraetId, wie) {
  const g = geraete.get(geraetId);
  if (g) {
    wie(g);
    if (!g.kacheln.size && !g.raeder.size) geraete.delete(geraetId);
  }
  laufen();
}

let takt = null;
let abo = null;

function laufen() {
  const sichtbar = [...geraete.values()].some((g) => g.kacheln.size || g.raeder.size);
  if (sichtbar && !takt) {
    abo = zustand.abonnieren(() => {}); // hält die Abfrage von AeroACARS am Laufen
    takt = setInterval(zeichnenAlle, TAKT_MS);
  } else if (!sichtbar && takt) {
    clearInterval(takt);
    takt = null;
    abo?.();
    abo = null;
  }
}

async function zeichnenAlle() {
  await vorbereiten(ORDNER);
  const t = Date.now() / 1000;
  const s = zustand.s;
  for (const g of geraete.values()) {
    try {
      if (g.layout && g.kacheln.size) zeichneKacheln(g, s, t);
      if (g.raeder.size) zeichneStreifen(g, s, t);
    } catch (e) {
      streamDeck.logger.error(`Zeichnen fehlgeschlagen: ${e.stack ?? e}`);
    }
  }
}

function zeichneKacheln(g, s, t) {
  const sz = szene(g.layout, s, t);
  g.aktionen = sz.aktionen;
  if (sz.svg === g.letzteSvg) return; // nichts bewegt sich gerade
  g.letzteSvg = sz.svg;
  const bild = rastern(sz.svg);
  for (const k of g.kacheln.values()) {
    let png;
    if (k.c < sz.spalten && k.r < sz.reihen) {
      const { x, y } = kachelOrt(k.c, k.r);
      png = pngAusschnitt(bild, x, y, KACHEL, KACHEL);
    } else {
      png = LEER;
    }
    if (k.letztes && k.letztes.equals(png)) continue;
    k.letztes = png;
    k.aktion.setImage(alsPngUrl(png));
  }
}

function zeichneStreifen(g, s, t) {
  if (g.dreh && Date.now() > g.dreh.bis) g.dreh = null;
  const svg = streifen(s, t, g.dreh);
  if (svg === g.letzterStreifen) return;
  g.letzterStreifen = svg;
  const bild = rastern(svg);
  for (const r of g.raeder.values()) {
    const png = pngAusschnitt(bild, Math.min(3, r.spalte) * 200, 0, 200, 100);
    if (r.letztes && r.letztes.equals(png)) continue;
    r.letztes = png;
    r.aktion.setFeedback({ bild: alsPngUrl(png) });
  }
}

/** Dunkle Kachel für Positionen außerhalb der Szene. */
const LEER = (() => {
  const pixel = new Uint8Array(KACHEL * KACHEL * 4);
  for (let i = 0; i < pixel.length; i += 4) pixel.set([4, 7, 11, 255], i);
  return pngAusschnitt({ breite: KACHEL, hoehe: KACHEL, pixel }, 0, 0, KACHEL, KACHEL);
})();

// ---- Was ein Druck tut ----------------------------------------------------

/** Führt einen Schritt aus; `aktion` für das Warnzeichen, `geraet` für Zurück. */
async function tun(was, aktion) {
  const k = zustand.s.kabine;
  // Alles außer „zurück“ braucht die neue Befehlsroute im Client.
  if (was !== "zurueck" && !k) return aktion.showAlert();
  switch (was) {
    case "zurueck": return streamDeck.profiles.switchToProfile(aktion.device.id);
    case "cdm": return ausfuehren(aktion, "vdgs_fenster_oeffnen", {});
    case "ans_leiser": return ausfuehren(aktion, "kabinen_lautstaerke", (j) => j && { lautstaerke: schritt(j.lautstaerke_gespeichert, -1), speichern: true });
    case "ans_lauter": return ausfuehren(aktion, "kabinen_lautstaerke", (j) => j && { lautstaerke: schritt(j.lautstaerke_gespeichert, +1), speichern: true });
    case "mus_leiser": return ausfuehren(aktion, "kabinen_musik_lautstaerke", (j) => j && { lautstaerke: schritt(j.musik_gespeichert, -1), speichern: true });
    case "mus_lauter": return ausfuehren(aktion, "kabinen_musik_lautstaerke", (j) => j && { lautstaerke: schritt(j.musik_gespeichert, +1), speichern: true });
    case "ans_aus": return ausfuehren(aktion, "kabinen_stumm", { stumm: true }).then(() => aktion.showOk?.()); // Rad hat kein showOk
    case "ans_an": return ausfuehren(aktion, "kabinen_stumm", { stumm: false });
    case "mus_stumm":
    case "musik_stumm":
      // Verlustfrei: live auf 0 ohne Speichern; laut = zurück auf die Einstellung.
      return ausfuehren(aktion, "kabinen_musik_lautstaerke", (j) => j && { lautstaerke: musikStumm(j) ? j.musik_gespeichert : 0, speichern: false });
    case "boarding": return ausfuehren(aktion, "kabinen_boarding_musik", (j) => j && { an: !j.boarding_musik });
    case "metar_start":
    case "metar_ziel":
      // An der Befehlskette vorbei: der Abruf geht übers Internet und darf
      // Lautstärke-Drücke nicht aufhalten. Haken bei Erfolg, Dreieck nur bei echtem Fehler.
      return befehl("metar_laden", { ort: was === "metar_ziel" ? "ziel" : "start" })
        .then(() => aktion.showOk?.())
        .catch((e) => {
          streamDeck.logger.warn(`metar_laden fehlgeschlagen: ${e.message}`);
          return aktion.showAlert();
        });
    default:
      if (was) return ausfuehren(aktion, "fenster_zeigen", { seite: was });
  }
}

// ---- Aktion „Kachel“ ------------------------------------------------------

class Kachel extends SingletonAction {
  constructor() {
    super();
    this.manifestId = uuid("kachel");
    this.druecke = new Map();
  }

  onWillAppear(ev) {
    const a = ev.action;
    if (!a.isKey?.()) return;
    const { column: c = 0, row: r = 0 } = a.coordinates ?? {};
    const g = geraetVon(a);
    g.kacheln.set(a.id, { aktion: a, c, r, letztes: null });
    g.letzteSvg = ""; // neue Kachel: beim nächsten Takt zeichnen, auch wenn sich nichts bewegt
    laufen();
  }

  onWillDisappear(ev) {
    clearTimeout(this.druecke.get(ev.action.id)?.uhr);
    this.druecke.delete(ev.action.id);
    abmelden(ev.action.device.id, (g) => g.kacheln.delete(ev.action.id));
  }

  onKeyDown(ev) {
    const a = ev.action;
    const g = geraetVon(a);
    const { column: c = 0, row: r = 0 } = a.coordinates ?? {};
    const plan = druckPlan(g.aktionen[`${c},${r}`], zustand.s.kabine);
    const druck = { plan, uhr: null, erledigt: false };
    const spaet = plan.halten ? [HALTEN_MS, plan.halten] : plan.lang ? [LANG_MS, plan.lang] : null;
    if (spaet) {
      druck.uhr = setTimeout(() => {
        druck.erledigt = true;
        tun(spaet[1], a);
      }, spaet[0]);
    }
    clearTimeout(this.druecke.get(a.id)?.uhr);
    this.druecke.set(a.id, druck);
  }

  async onKeyUp(ev) {
    const a = ev.action;
    const druck = this.druecke.get(a.id);
    this.druecke.delete(a.id);
    if (!druck || druck.erledigt) return;
    clearTimeout(druck.uhr);
    if (druck.plan.kurz) await tun(druck.plan.kurz, a);
  }
}

// ---- Räder des StreamDeck + -----------------------------------------------

class Rad extends SingletonAction {
  /** `art`: "ans" | "mus" | "info" | "wetter" */
  constructor(id, art) {
    super();
    this.manifestId = uuid(id);
    this.art = art;
    this.lage = new Map(); // je Rad: Entwurf, Uhren
  }

  lageVon(a) {
    if (!this.lage.has(a.id)) this.lage.set(a.id, { entwurf: null, speicherUhr: null, halteUhr: null, gehalten: false, warStumm: false });
    return this.lage.get(a.id);
  }

  onWillAppear(ev) {
    const a = ev.action;
    if (!a.isDial?.()) return;
    a.setFeedbackLayout?.("layouts/regler.json");
    const g = geraetVon(a);
    g.raeder.set(a.id, { aktion: a, spalte: a.coordinates?.column ?? 0, letztes: null });
    g.letzterStreifen = null;
    laufen();
  }

  onWillDisappear(ev) {
    const a = ev.action;
    const l = this.lage.get(a.id);
    if (l) {
      clearTimeout(l.halteUhr);
      // Gedrehte, noch nicht gespeicherte Lautstärke jetzt speichern statt
      // später aus einem verschwundenen Rad heraus.
      if (l.speicherUhr) {
        clearTimeout(l.speicherUhr);
        if (l.entwurf != null) ausfuehren(a, this.art === "ans" ? "kabinen_lautstaerke" : "kabinen_musik_lautstaerke", { lautstaerke: l.entwurf, speichern: true });
      }
      this.lage.delete(a.id);
    }
    abmelden(a.device.id, (g) => g.raeder.delete(a.id));
  }

  zeigen(a, extra = {}) {
    const g = geraetVon(a);
    g.dreh = { rad: this.art, spalte: a.coordinates?.column ?? 0, bis: Date.now() + DREH_ANZEIGE_MS, seite: this.art === "info" ? g.infoSeite : g.wetterSeite, ...extra };
    g.letzterStreifen = null;
    zeichnenAlle();
  }

  async onDialRotate(ev) {
    const a = ev.action;
    const g = geraetVon(a);
    const schritte = ev.payload.ticks;
    if (this.art === "info") {
      // Jeder Rastschritt zählt (schnelles Drehen überspringt Seiten).
      g.infoSeite = (((g.infoSeite + schritte) % INFO_RAD.length) + INFO_RAD.length) % INFO_RAD.length;
      return this.zeigen(a);
    }
    if (this.art === "wetter") {
      g.wetterSeite = (((g.wetterSeite + schritte) % 2) + 2) % 2;
      return this.zeigen(a);
    }
    const k = zustand.s.kabine;
    if (!k) return a.showAlert();
    const l = this.lageVon(a);
    const ansage = this.art === "ans";
    const befehlName = ansage ? "kabinen_lautstaerke" : "kabinen_musik_lautstaerke";
    const basis = l.entwurf ?? (ansage ? k.lautstaerke_live : k.musik_live) ?? 0;
    const wert = Math.min(1, Math.max(0, Math.round((basis + schritte * 0.02) * 100) / 100));
    l.entwurf = wert;
    this.zeigen(a, { wert });
    // Genau eine Speicher-Uhr, gestellt vor dem Netzaufruf: speichert den letzten Wert.
    clearTimeout(l.speicherUhr);
    l.speicherUhr = setTimeout(async () => {
      l.speicherUhr = null;
      const letzter = l.entwurf;
      if (letzter == null) return;
      await ausfuehren(a, befehlName, { lautstaerke: letzter, speichern: true });
      if (l.entwurf === letzter && !l.speicherUhr) l.entwurf = null;
    }, SPEICHERN_NACH_MS);
    await ausfuehren(a, befehlName, { lautstaerke: wert, speichern: false });
  }

  onDialDown(ev) {
    const a = ev.action;
    const l = this.lageVon(a);
    l.gehalten = false;
    l.warStumm = !!zustand.s.kabine?.stumm;
    clearTimeout(l.halteUhr);
    // Musik: halten = Boarding-Musik an/aus. Ansagen: kein Halten — am Rad
    // schaltet ein kurzer Druck stumm/an (Thomas 23.10.: Halten am Plus ging
    // „nicht so richtig“; das Rad wird nicht versehentlich gedrückt wie eine Taste).
    const halten = this.art === "mus" ? "boarding" : null;
    if (halten) {
      l.halteUhr = setTimeout(() => {
        l.gehalten = true;
        Promise.resolve(tun(halten, a)).then(() => this.zeigen(a));
      }, HALTEN_MS);
    }
  }

  async onDialUp(ev) {
    const a = ev.action;
    const l = this.lageVon(a);
    clearTimeout(l.halteUhr);
    if (l.gehalten) return;
    const g = geraetVon(a);
    if (this.art === "ans") await tun(l.warStumm ? "ans_an" : "ans_aus", a);
    else if (this.art === "mus") await tun("mus_stumm", a);
    else if (this.art === "wetter") await tun(g.wetterSeite === 1 ? "metar_ziel" : "metar_start", a);
    else if (this.art === "info") await tun(INFO_LANG[INFO_RAD[g.infoSeite]] ?? "cockpit", a);
    this.zeigen(a);
  }
}

/** Druck auf Rad 3 öffnet die passende Seite der App. */
const INFO_LANG = { sprit: "briefing" };

export function registrieren() {
  streamDeck.actions.registerAction(new Kachel());
  streamDeck.actions.registerAction(new Rad("regler_ansage", "ans"));
  streamDeck.actions.registerAction(new Rad("regler_musik", "mus"));
  streamDeck.actions.registerAction(new Rad("regler_info", "info"));
  streamDeck.actions.registerAction(new Rad("regler_wetter", "wetter"));
}
