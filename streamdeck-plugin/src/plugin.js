// AeroACARS am StreamDeck: jede Taste eine Aktion, Bild aus tasten.js,
// Verhalten hier. Läuft auf demselben PC wie AeroACARS und spricht nur mit
// dessen lokalem Panel-Server.

import streamDeck, { SingletonAction } from "@elgato/streamdeck";
import { TASTEN, musikStumm, schritt } from "./tasten.js";
import { tasteSvg, alsDatenUrl } from "./bild.js";
import { zustand, ausfuehren } from "./befehle.js";
import { registrieren as deckRegistrieren } from "./deck.js";
import { uuid, profilFuer } from "./belegung.js";

const HALTEN_MS = 1000;
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
    const bild = alsDatenUrl(tasteSvg(TASTEN[this.id].bild(s)));
    // Nur senden, was sich geändert hat — die Software zeichnet sonst jede Sekunde alles neu.
    if (this.letztes.get(a.id) === bild) return;
    this.letztes.set(a.id, bild);
    a.setImage(bild);
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
    const bild = alsDatenUrl(tasteSvg({ ...def.bild(s, ansicht), ansichten, ansicht }));
    if (this.letztes.get(a.id) === bild) return;
    this.letztes.set(a.id, bild);
    a.setImage(bild);
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

class PhaseTaste extends AnzeigeTaste {
  async kurz(a) {
    // Ohne Profilnamen springt das StreamDeck in das vorherige Profil zurück.
    // Feldbefund 23.10.: Das klappt nur, wenn das Plugin selbst hergewechselt
    // hat (Taste „AeroACARS öffnen“), nicht nach Auswahl von Hand.
    await streamDeck.profiles.switchToProfile(a.device.id);
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

/** Stumm nur durch Halten: Stumm verwirft laufende und wartende Ansagen. */
class StummTaste extends Taste {
  constructor(id) {
    super(id);
    // Je Taste der laufende Druck: ob die Kabine bei Druckbeginn schon stumm
    // war, und die Halte-Uhr (null, sobald sie ausgelöst hat).
    this.druecke = new Map();
  }

  onKeyDown(ev) {
    const a = ev.action;
    const warStumm = !!zustand.s.kabine?.stumm;
    const druck = { warStumm, uhr: null };
    if (!warStumm) {
      druck.uhr = setTimeout(() => {
        druck.uhr = null;
        ausfuehren(a, "kabinen_stumm", { stumm: true }).then(() => a.showOk());
      }, HALTEN_MS);
    }
    this.druecke.set(a.id, druck);
  }

  async onKeyUp(ev) {
    const a = ev.action;
    const druck = this.druecke.get(a.id);
    this.druecke.delete(a.id);
    if (!druck) return;
    clearTimeout(druck.uhr);
    // Nur ein Druck, der auf einer schon stummen Kabine BEGANN, hebt auf.
    // Das Loslassen nach dem Halten, das gerade stumm geschaltet hat, nicht.
    if (druck.warStumm) await ausfuehren(a, "kabinen_stumm", { stumm: false });
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
  phase: () => new PhaseTaste("phase"),
  oeffnen: () => new OeffnenTaste("oeffnen"),
  kabine_stumm: () => new StummTaste("kabine_stumm"),
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
