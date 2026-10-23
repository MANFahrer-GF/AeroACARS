// AeroACARS am StreamDeck: jede Taste eine Aktion, Bild aus tasten.js,
// Verhalten hier. Läuft auf demselben PC wie AeroACARS und spricht nur mit
// dessen lokalem Panel-Server.

import streamDeck, { SingletonAction } from "@elgato/streamdeck";
import { befehl } from "./aeroacars.js";
import { TASTEN, INFO_SEITEN, WETTER_SEITEN, musikStumm, schritt } from "./tasten.js";
import { tasteSvg, alsDatenUrl, icon } from "./bild.js";
import { Zustand } from "./zustand.js";
import { uuid } from "./belegung.js";

const HALTEN_MS = 1000;
const SPEICHERN_NACH_MS = 800;

const zustand = new Zustand();

// Befehle strikt nacheinander: so kommen sie in der Reihenfolge beim Server
// an, in der sie ausgelöst wurden. Sonst könnte ein langsamer Live-Pegel vom
// Drehen nach dem Speichern eintreffen und den Klang wieder zurückstellen.
let kette = Promise.resolve();

/**
 * `args` darf eine Funktion des Kabinenzustands sein: Sie wird erst
 * ausgewertet, wenn der Befehl an der Reihe ist — dann rechnen zwei schnelle
 * Drücke auf „lauter“ vom Ergebnis des ersten aus, nicht beide vom selben Wert.
 */
function ausfuehren(aktion, name, args) {
  const lauf = kette.then(async () => {
    try {
      const werte = typeof args === "function" ? args(zustand.s.kabine) : args;
      if (werte == null) return;
      zustand.kabineSetzen(await befehl(name, werte));
    } catch (e) {
      streamDeck.logger.warn(`${name} fehlgeschlagen: ${e.message}`);
      await aktion.showAlert();
    }
  });
  kette = lauf;
  return lauf;
}

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

class PhaseTaste extends Taste {
  async onKeyDown(ev) {
    // Ohne Profilnamen springt das StreamDeck in das vorherige Profil zurück.
    await streamDeck.profiles.switchToProfile(ev.action.device.id);
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
  async druecken(a) {
    await ausfuehren(a, "vdgs_fenster_oeffnen", {});
  }
}

// ---- Drehregler (StreamDeck +) --------------------------------------------

const pixmap = (name, farbe) => `data:image/svg+xml;base64,${Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 48 48">${icon(name, 6, 6, 36, farbe)}</svg>`).toString("base64")}`;

/** Lautstärke-Regler: drehen = live ändern, kurz danach speichern. */
class PegelRegler extends Taste {
  constructor(id, { befehlName, live, titel, symbol, druecken }) {
    super(id);
    Object.assign(this, { befehlName, live, titel, symbol, druckFn: druecken });
    // Je Regler-Instanz (eine Aktion kann auf mehreren Geräten liegen).
    this.lage = new Map();
  }

  lageVon(a) {
    if (!this.lage.has(a.id)) this.lage.set(a.id, { entwurf: null, speicherUhr: null, halteUhr: null, gehalten: false });
    return this.lage.get(a.id);
  }

  onWillDisappear(ev) {
    const l = this.lage.get(ev.action.id);
    if (l) clearTimeout(l.halteUhr);
    super.onWillDisappear(ev);
  }

  zeichnen(a, s) {
    if (!a.isDial?.()) return;
    const k = s.kabine;
    const wert = this.lageVon(a).entwurf ?? (k ? k[this.live] : null);
    const stumm = k && (this.id === "regler_ansage" ? k.stumm : musikStumm(k));
    const text = !s.erreichbar ? "aus" : !k ? "–" : stumm ? "stumm" : `${Math.round((wert ?? 0) * 100)} %`;
    const fb = JSON.stringify([text, wert, stumm]);
    if (this.letztes.get(a.id) === fb) return;
    this.letztes.set(a.id, fb);
    a.setFeedback({
      title: this.titel,
      icon: pixmap(stumm ? (this.id === "regler_ansage" ? "mute" : "musicOff") : this.symbol, stumm ? "#ff8a7a" : "#8fbaff"),
      value: text,
      indicator: { value: Math.round((wert ?? 0) * 100) },
    });
  }

  async onDialRotate(ev) {
    const k = zustand.s.kabine;
    if (!k) return;
    const l = this.lageVon(ev.action);
    const basis = l.entwurf ?? k[this.live] ?? 0;
    const wert = Math.min(1, Math.max(0, Math.round((basis + ev.payload.ticks * 0.02) * 100) / 100));
    l.entwurf = wert;
    // Speicher-Uhr VOR dem Netzaufruf neu stellen: so gibt es immer genau
    // eine, und sie speichert den zuletzt gedrehten Wert.
    clearTimeout(l.speicherUhr);
    l.speicherUhr = setTimeout(async () => {
      l.speicherUhr = null;
      const letzter = l.entwurf;
      if (letzter == null) return;
      await ausfuehren(ev.action, this.befehlName, { lautstaerke: letzter, speichern: true });
      if (l.entwurf === letzter && !l.speicherUhr) l.entwurf = null;
    }, SPEICHERN_NACH_MS);
    await ausfuehren(ev.action, this.befehlName, { lautstaerke: wert, speichern: false });
  }

  onDialDown(ev) {
    const l = this.lageVon(ev.action);
    l.gehalten = false;
    l.halteUhr = setTimeout(() => {
      l.gehalten = true;
      this.druckFn(ev.action, zustand.s.kabine, true);
    }, HALTEN_MS);
  }

  onDialUp(ev) {
    const l = this.lageVon(ev.action);
    clearTimeout(l.halteUhr);
    if (!l.gehalten) this.druckFn(ev.action, zustand.s.kabine, false);
  }
}

/** Blätter-Regler: drehen wechselt die Seite, das Bild ist das der Taste. */
class BlaetterRegler extends Taste {
  constructor(id, titel, seiten) {
    super(id);
    Object.assign(this, { titel, seiten });
    this.seiten_ = new Map(); // Seite je Regler-Instanz
  }

  zeichnen(a, s) {
    if (!a.isDial?.()) return;
    const b = TASTEN[this.seiten[this.seiten_.get(a.id) ?? 0]].bild(s);
    const fb = JSON.stringify([b.label, b.wert, b.unter]);
    if (this.letztes.get(a.id) === fb) return;
    this.letztes.set(a.id, fb);
    a.setFeedback({
      title: `${this.titel} · ${b.label}`,
      icon: pixmap(b.icon, "#8fbaff"),
      value: `${b.wert ?? ""} ${b.unter ?? ""}`.trim(),
      indicator: { enabled: false },
    });
  }

  onDialRotate(ev) {
    const n = this.seiten.length;
    const alt = this.seiten_.get(ev.action.id) ?? 0;
    this.seiten_.set(ev.action.id, (((alt + Math.sign(ev.payload.ticks)) % n) + n) % n);
    this.letztes.delete(ev.action.id);
    this.zeichnen(ev.action, zustand.s);
  }
}

// ---- Anmelden -------------------------------------------------------------

const besondere = {
  phase: () => new PhaseTaste("phase"),
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
  streamDeck.actions.registerAction((besondere[id] ?? (() => new Taste(id)))());
}

streamDeck.actions.registerAction(
  new PegelRegler("regler_ansage", {
    befehlName: "kabinen_lautstaerke",
    live: "lautstaerke_live",
    titel: "Ansagen",
    symbol: "spk",
    // Halten = stumm, kurz = Stumm aufheben (wie die Taste).
    druecken: (a, k, gehalten) => {
      if (!k) return;
      if (gehalten && !k.stumm) ausfuehren(a, "kabinen_stumm", { stumm: true });
      else if (!gehalten && k.stumm) ausfuehren(a, "kabinen_stumm", { stumm: false });
    },
  }),
);
streamDeck.actions.registerAction(
  new PegelRegler("regler_musik", {
    befehlName: "kabinen_musik_lautstaerke",
    live: "musik_live",
    titel: "Musik",
    symbol: "music",
    // Kurz = Musik stumm/laut, halten = Boarding-Musik an/aus.
    druecken: (a, k, gehalten) => {
      if (!k) return;
      if (gehalten) ausfuehren(a, "kabinen_boarding_musik", (j) => (j ? { an: !j.boarding_musik } : null));
      else ausfuehren(a, "kabinen_musik_lautstaerke", (j) => (j ? { lautstaerke: musikStumm(j) ? j.musik_gespeichert : 0, speichern: false } : null));
    },
  }),
);
streamDeck.actions.registerAction(new BlaetterRegler("regler_info", "Flug", INFO_SEITEN));
streamDeck.actions.registerAction(new BlaetterRegler("regler_wetter", "Wetter", WETTER_SEITEN));

streamDeck.connect();
