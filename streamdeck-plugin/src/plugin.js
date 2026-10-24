// AeroACARS am StreamDeck: jede Taste eine Aktion, Bild aus tasten.js,
// Verhalten hier. Läuft auf demselben PC wie AeroACARS und spricht nur mit
// dessen lokalem Panel-Server.

import streamDeck, { SingletonAction } from "@elgato/streamdeck";
import { TASTEN, musikStumm, schritt } from "./tasten.js";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { einzelSvg } from "./einzel.js";
import { tasteAlsUrl } from "./render.js";
import { zustand, ausfuehren } from "./befehle.js";
import { registrieren as deckRegistrieren } from "./deck.js";
import { uuid, profilFuer } from "./belegung.js";

/** Plugin-Ordner (.sdPlugin) — bin/plugin.js liegt eine Ebene darunter. */
const ORDNER = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Tastenbild im Glascockpit-Stil, im Plugin gerastert (B612). */
const malen = (b) => tasteAlsUrl(einzelSvg(b), ORDNER);
/** Ab hier gilt ein Druck auf eine Anzeigetaste als „lang“ (App öffnen). */
const LANG_MS = 600;

/** Gemeinsame Grundlage: Bild bei jeder Abfragerunde neu zeichnen. */
class Taste extends SingletonAction {
  constructor(id) {
    super();
    this.id = id;
    this.manifestId = uuid(id);
    this.abos = new Map();
    this.letztes = new Map();
    this.laufnummer = new Map();
  }

  onWillAppear(ev) {
    const a = ev.action;
    this.abos.set(a.id, zustand.abonnieren((s) => this.zeichnen(a, s)));
  }

  onWillDisappear(ev) {
    this.abos.get(ev.action.id)?.();
    this.abos.delete(ev.action.id);
    this.letztes.delete(ev.action.id);
  }

  zeichnen(a, s) {
    if (!a.isKey?.()) return;
    const b = TASTEN[this.id].bild(s);
    this.senden(a, b);
  }

  /** Nur senden, was sich geändert hat — die Software zeichnet sonst jede Sekunde alles neu. */
  senden(a, b) {
    const schluessel = JSON.stringify(b);
    if (this.letztes.get(a.id) === schluessel) return;
    this.letztes.set(a.id, schluessel);
    // Laufnummer: nur das zuletzt angeforderte Bild darf auf die Taste —
    // ein langsamer älterer Render käme sonst danach an (Codex/Advisor 23.10.).
    const nr = (this.laufnummer.get(a.id) ?? 0) + 1;
    this.laufnummer.set(a.id, nr);
    malen(b)
      .then((url) => this.laufnummer.get(a.id) === nr && a.setImage(url))
      .catch((e) => streamDeck.logger.error(`Taste ${this.id}: ${e.stack ?? e}`));
  }

  async onKeyDown(ev) {
    await this.druecken(ev.action, zustand.s);
  }

  async druecken() {}
}

/**
 * Anzeigetaste: kurz drücken blättert durch ihre Ansichten, lang drücken
 * holt AeroACARS auf der passenden Seite nach vorn (Thomas 23.10.: „Beides“).
 */
class AnzeigeTaste extends Taste {
  constructor(id) {
    super(id);
    this.ansicht = new Map(); // je Taste die gezeigte Ansicht
    this.uhren = new Map(); // je Taste die Lang-Uhr des laufenden Drucks
  }

  zeichnen(a, s) {
    if (!a.isKey?.()) return;
    const def = TASTEN[this.id];
    const ansichten = def.ansichten ?? 1;
    const ansicht = Math.min(this.ansicht.get(a.id) ?? 0, ansichten - 1);
    this.senden(a, { ...def.bild(s, ansicht), ansichten, ansicht });
  }

  onKeyDown(ev) {
    const a = ev.action;
    clearTimeout(this.uhren.get(a.id));
    this.uhren.set(
      a.id,
      setTimeout(() => {
        this.uhren.set(a.id, null); // ausgelöst: Loslassen tut nichts mehr
        this.lang(a);
      }, LANG_MS),
    );
  }

  async onKeyUp(ev) {
    const a = ev.action;
    const uhr = this.uhren.get(a.id);
    this.uhren.delete(a.id);
    if (!uhr) return;
    clearTimeout(uhr);
    await this.kurz(a);
  }

  onWillDisappear(ev) {
    clearTimeout(this.uhren.get(ev.action.id));
    this.uhren.delete(ev.action.id);
    super.onWillDisappear(ev);
  }

  async kurz(a) {
    const n = TASTEN[this.id].ansichten ?? 1;
    if (n < 2) return;
    this.ansicht.set(a.id, ((this.ansicht.get(a.id) ?? 0) + 1) % n);
    this.zeichnen(a, zustand.s);
  }

  async lang(a) {
    const seite = TASTEN[this.id].seite;
    if (!seite) return;
    // Ohne die neue Befehlsroute (Client vor dem StreamDeck-Release) geht es nicht.
    if (!zustand.s.kabine) return a.showAlert();
    await ausfuehren(a, "fenster_zeigen", { seite });
  }
}

/** Wechselt ins mitgelieferte Profil dieses Gerätetyps. */
class OeffnenTaste extends Taste {
  async onKeyDown(ev) {
    const a = ev.action;
    const profil = profilFuer(a.device.type);
    if (!profil) return a.showAlert(); // Mini, Neo …: kein mitgeliefertes Profil
    await streamDeck.profiles.switchToProfile(a.device.id, profil);
  }
}

/** Ansagen stumm/an per kurzem Druck (Thomas 24.10.: nicht nur per Halten). */
class StummTaste extends Taste {
  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    await ausfuehren(a, "kabinen_stumm", (k) => (k ? { stumm: !k.stumm } : null));
  }
}

class SchrittTaste extends Taste {
  constructor(id, befehlName, feld, richtung) {
    super(id);
    Object.assign(this, { befehlName, feld, richtung });
  }

  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    await ausfuehren(a, this.befehlName, (k) => (k ? { lautstaerke: schritt(k[this.feld], this.richtung), speichern: true } : null));
  }
}

class MusikPegelTaste extends Taste {
  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    // Stumm = live auf 0, ohne zu speichern; laut = zurück auf die Einstellung.
    await ausfuehren(a, "kabinen_musik_lautstaerke", (k) =>
      k ? { lautstaerke: musikStumm(k) ? k.musik_gespeichert : 0, speichern: false } : null,
    );
  }
}

class BoardingMusikTaste extends Taste {
  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    await ausfuehren(a, "kabinen_boarding_musik", (k) => (k ? { an: !k.boarding_musik } : null));
  }
}

class CdmTaste extends Taste {
  async druecken(a, s) {
    if (!s.kabine) return a.showAlert(); // Client ohne Befehlsroute
    await ausfuehren(a, "vdgs_fenster_oeffnen", {});
  }
}

// ---- Anmelden -------------------------------------------------------------

const besondere = {
  oeffnen: () => new OeffnenTaste("oeffnen"),
  kabine_stumm: () => new StummTaste("kabine_stumm"),
  ansage_pegel: () => new StummTaste("ansage_pegel"),
  ansage_leiser: () => new SchrittTaste("ansage_leiser", "kabinen_lautstaerke", "lautstaerke_gespeichert", -1),
  ansage_lauter: () => new SchrittTaste("ansage_lauter", "kabinen_lautstaerke", "lautstaerke_gespeichert", +1),
  musik_leiser: () => new SchrittTaste("musik_leiser", "kabinen_musik_lautstaerke", "musik_gespeichert", -1),
  musik_lauter: () => new SchrittTaste("musik_lauter", "kabinen_musik_lautstaerke", "musik_gespeichert", +1),
  musik_pegel: () => new MusikPegelTaste("musik_pegel"),
  boarding_musik: () => new BoardingMusikTaste("boarding_musik"),
  cdm: () => new CdmTaste("cdm"),
};

for (const id of Object.keys(TASTEN)) {
  streamDeck.actions.registerAction((besondere[id] ?? (() => new AnzeigeTaste(id)))());
}

// Kacheln (ganzes Deck als eine Szene) und die Räder des StreamDeck +.
deckRegistrieren();

streamDeck.connect();
