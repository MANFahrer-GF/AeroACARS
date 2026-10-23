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

async function ausfuehren(aktion, name, args) {
  try {
    zustand.kabineSetzen(await befehl(name, args));
  } catch (e) {
    streamDeck.logger.warn(`${name} fehlgeschlagen: ${e.message}`);
    await aktion.showAlert();
  }
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
    this.uhren = new Map();
  }

  onKeyDown(ev) {
    const a = ev.action;
    if (zustand.s.kabine?.stumm) return; // Aufheben beim Loslassen.
    this.uhren.set(
      a.id,
      setTimeout(() => {
        this.uhren.delete(a.id);
        ausfuehren(a, "kabinen_stumm", { stumm: true }).then(() => a.showOk());
      }, HALTEN_MS),
    );
  }

  async onKeyUp(ev) {
    const a = ev.action;
    const uhr = this.uhren.get(a.id);
    if (uhr) {
      clearTimeout(uhr);
      this.uhren.delete(a.id);
      return; // Zu kurz gehalten: nichts.
    }
    if (zustand.s.kabine?.stumm) await ausfuehren(a, "kabinen_stumm", { stumm: false });
  }
}

class SchrittTaste extends Taste {
  constructor(id, befehlName, feld, richtung) {
    super(id);
    Object.assign(this, { befehlName, feld, richtung });
  }

  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    await ausfuehren(a, this.befehlName, { lautstaerke: schritt(s.kabine[this.feld], this.richtung), speichern: true });
  }
}

class MusikPegelTaste extends Taste {
  async druecken(a, s) {
    const k = s.kabine;
    if (!k) return a.showAlert();
    // Stumm = live auf 0, ohne zu speichern; laut = zurück auf die Einstellung.
    const wert = musikStumm(k) ? k.musik_gespeichert : 0;
    await ausfuehren(a, "kabinen_musik_lautstaerke", { lautstaerke: wert, speichern: false });
  }
}

class BoardingMusikTaste extends Taste {
  async druecken(a, s) {
    if (!s.kabine) return a.showAlert();
    await ausfuehren(a, "kabinen_boarding_musik", { an: !s.kabine.boarding_musik });
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
  constructor(id, { befehlName, live, gespeichert, titel, symbol, druecken }) {
    super(id);
    Object.assign(this, { befehlName, live, gespeichert, titel, symbol, druckFn: druecken });
    this.entwurf = null;
    this.speicherUhr = null;
    this.halteUhr = null;
  }

  zeichnen(a, s) {
    if (!a.isDial?.()) return;
    const k = s.kabine;
    const wert = this.entwurf ?? (k ? k[this.live] : null);
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
    const basis = this.entwurf ?? k[this.live];
    this.entwurf = Math.min(1, Math.max(0, Math.round((basis + ev.payload.ticks * 0.02) * 100) / 100));
    clearTimeout(this.speicherUhr);
    await ausfuehren(ev.action, this.befehlName, { lautstaerke: this.entwurf, speichern: false });
    this.speicherUhr = setTimeout(async () => {
      const wert = this.entwurf;
      this.entwurf = null;
      await ausfuehren(ev.action, this.befehlName, { lautstaerke: wert, speichern: true });
    }, SPEICHERN_NACH_MS);
  }

  onDialDown(ev) {
    this.gehalten = false;
    this.halteUhr = setTimeout(() => {
      this.gehalten = true;
      this.druckFn(ev.action, zustand.s.kabine, true);
    }, HALTEN_MS);
  }

  onDialUp(ev) {
    clearTimeout(this.halteUhr);
    if (!this.gehalten) this.druckFn(ev.action, zustand.s.kabine, false);
  }
}

/** Blätter-Regler: drehen wechselt die Seite, das Bild ist das der Taste. */
class BlaetterRegler extends Taste {
  constructor(id, titel, seiten) {
    super(id);
    Object.assign(this, { titel, seiten });
    this.seite = 0;
  }

  zeichnen(a, s) {
    if (!a.isDial?.()) return;
    const b = TASTEN[this.seiten[this.seite]].bild(s);
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
    this.seite = (((this.seite + Math.sign(ev.payload.ticks)) % n) + n) % n;
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
    gespeichert: "lautstaerke_gespeichert",
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
    gespeichert: "musik_gespeichert",
    titel: "Musik",
    symbol: "music",
    // Kurz = Musik stumm/laut, halten = Boarding-Musik an/aus.
    druecken: (a, k, gehalten) => {
      if (!k) return;
      if (gehalten) ausfuehren(a, "kabinen_boarding_musik", { an: !k.boarding_musik });
      else ausfuehren(a, "kabinen_musik_lautstaerke", { lautstaerke: musikStumm(k) ? k.musik_gespeichert : 0, speichern: false });
    },
  }),
);
streamDeck.actions.registerAction(new BlaetterRegler("regler_info", "Flug", INFO_SEITEN));
streamDeck.actions.registerAction(new BlaetterRegler("regler_wetter", "Wetter", WETTER_SEITEN));

streamDeck.connect();
