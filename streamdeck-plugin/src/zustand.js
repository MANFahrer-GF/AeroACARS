// Fragt AeroACARS regelmäßig ab, solange mindestens eine Taste sichtbar ist.
// Status und Kabine jede Sekunde, die Landung
// nur nach dem Aufsetzen und dann alle fünf Sekunden (der Panel-Server
// protokolliert gehäufte Landungsabfragen).

import { lesen, befehl } from "./aeroacars.js";

const TAKT_MS = 1000;
/** Fehlendes METAR höchstens so oft selbst nachladen. */
const METAR_ABSTAND_MS = 5 * 60 * 1000;

export class Zustand {
  constructor() {
    this.s = { erreichbar: false, status: null, kabine: null, debrief: null, jetzt: new Date() };
    this.hoerer = new Set();
    this.takt = null;
    this.runde = 0;
    this.metarVersuch = { start: 0, ziel: 0 };
  }

  /** `fn(s)` bei jeder Runde; liefert eine Abmelde-Funktion. */
  abonnieren(fn) {
    this.hoerer.add(fn);
    if (!this.takt) this.starten();
    fn(this.s);
    return () => {
      this.hoerer.delete(fn);
      if (this.hoerer.size === 0) this.stoppen();
    };
  }

  starten() {
    this.takt = setInterval(() => this.abfragen(), TAKT_MS);
    this.abfragen();
  }

  stoppen() {
    clearInterval(this.takt);
    this.takt = null;
  }

  /** Kabine sofort übernehmen (Antwort eines Befehls). */
  kabineSetzen(kabine) {
    this.s = { ...this.s, kabine };
    this.melden();
  }

  async abfragen() {
    // Hängt AeroACARS, dauert eine Runde länger als der Takt: nicht stapeln.
    if (this.laeuft) return;
    this.laeuft = true;
    try {
      await this.runde_();
    } finally {
      this.laeuft = false;
    }
  }

  async runde_() {
    const runde = this.runde++;
    const s = { ...this.s, jetzt: new Date() };
    try {
      s.status = await lesen("/panel/status");
      s.erreichbar = true;
    } catch {
      this.s = { erreichbar: false, status: null, kabine: null, debrief: null, jetzt: s.jetzt };
      this.melden();
      return;
    }
    // Ältere Clients kennen die Kabinen-Route nicht: dann bleibt sie null.
    s.kabine = await lesen("/panel/kabine").then(
      (k) => ((this.kabinenFehler = null), k),
      (e) => {
        // Ins Plugin-Log, einmal je neuem Fehler (Feldbefund 24.10.: ohne Log war
        // „Client zu alt“ am Gerät nicht von „keine Antwort“ zu unterscheiden).
        if (e.message !== this.kabinenFehler) this.protokoll?.(`/panel/kabine: ${e.message}`);
        this.kabinenFehler = e.message;
        return null;
      },
    );
    if (!s.status?.landing_at) s.debrief = null;
    else if (runde % 5 === 0 || !s.debrief) s.debrief = await lesen("/panel/debrief").catch(() => s.debrief);
    this.s = s;
    this.melden();
    this.metarNachladen(s);
  }

  /**
   * Fehlt Start- oder Zielwetter, holt das Plugin es selbst (Thomas 23.10.:
   * „wir haben immer noch kein Wetter“). AeroACARS lädt das Ziel-METAR von
   * sich aus erst im Sinkflug; nach dem Laden ist es nur Anzeige, PIREP und
   * Landung bekommen weiter das reguläre (siehe metar_nachladen im Client).
   * Nur mit Client, der die Befehlsroute kennt (dann gibt es `kabine`).
   */
  metarNachladen(s) {
    if (!s.status || !s.kabine) return;
    // Neuer Flug: Sperre zurücksetzen (Codex 23.10.: sonst bis zu 5 min kein Wetter).
    const flug = s.status.pirep_id ?? `${s.status.callsign}|${s.status.dpt_airport}|${s.status.arr_airport}`;
    if (flug !== this.metarFlug) {
      this.metarFlug = flug;
      this.metarVersuch = { start: 0, ziel: 0 };
    }
    const jetzt = Date.now();
    for (const [ort, feld] of [["start", "dep_metar_decoded"], ["ziel", "arr_metar_decoded"]]) {
      if (s.status[feld] || jetzt - this.metarVersuch[ort] < METAR_ABSTAND_MS) continue;
      this.metarVersuch[ort] = jetzt;
      befehl("metar_laden", { ort }).catch(() => {});
    }
  }

  melden() {
    for (const fn of this.hoerer) {
      try {
        fn(this.s);
      } catch {
        // Eine kaputte Taste darf die anderen nicht anhalten.
      }
    }
  }
}
