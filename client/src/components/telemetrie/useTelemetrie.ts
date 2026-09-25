/**
 * Telemetrie-Monitor (v1.8) — der Datenstrom fuer die Oberflaeche.
 *
 * Beim Oeffnen: `telemetrie_start` liefert Katalog und Verlauf (bis zu fuenf
 * Minuten), danach kommen 20 Frames je Sekunde als Ereignis
 * `telemetrie-frame`. Alle 5 s meldet sich die Oberflaeche mit
 * `telemetrie_halten`, sonst beendet das Backend den Strom nach 15 s von
 * selbst (abgestuerztes Fenster). Beim Schliessen `telemetrie_stop`.
 *
 * Die Frames liegen in einem Ref, nicht im React-State: 20 Aenderungen je
 * Sekunde wuerden sonst jedes Mal den ganzen Monitor neu aufbauen. Neu
 * gezeichnet wird hoechstens 10× je Sekunde (`version`).
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, listen } from "../../lib/ipc";
import { ereignisseAus, type Ereignis } from "./ereignisse";
import type { Frame, Kanal, Katalog, StartAntwort } from "./typen";

/** So weit reicht der Puffer zurueck (ms). */
export const PUFFER_MS = 5 * 60 * 1000;
const HALTEN_MS = 5000;
const ZEICHNEN_MS = 100;

export type Zustand = "laedt" | "bereit" | "fehler";

export interface Marker {
  t: number;
  text: string;
}

export interface Telemetrie {
  zustand: Zustand;
  fehler: string | null;
  katalog: Katalog | null;
  /** Kanal-ID → Position in `Frame.z` bzw. `Frame.s`. */
  zahlIndex: Map<string, number>;
  textIndex: Map<string, number>;
  kanal: (id: string) => Kanal | undefined;
  /** Alle Frames im Puffer, aelteste zuerst. Nicht veraendern. */
  frames: Frame[];
  letzter: Frame | null;
  /** Steigt bei jedem Neuzeichnen — fuer `useMemo`-Abhaengigkeiten. */
  version: number;
  angehalten: boolean;
  setAngehalten: (v: boolean) => void;
  ereignisse: Ereignis[];
  marker: Marker[];
  markerSetzen: (text: string) => void;
  /** Letzter Wert eines Zahlkanals, `null` wenn gerade keiner da ist. */
  wert: (id: string, f?: Frame | null) => number | null;
  text: (id: string, f?: Frame | null) => string | null;
}

export interface Datenquelle {
  start: () => Promise<StartAntwort>;
  abonnieren: (cb: (f: Frame) => void) => Promise<() => void>;
  halten: () => void;
  stop: () => void;
}

/** Geraetekennung dieses Tabs: Auf dem Tablet meldet sich jedes Geraet
 *  einzeln an, damit das Schliessen auf einem den Strom der anderen nicht
 *  unterbricht. In der App ignoriert das Backend sie (dort zaehlt das
 *  Fenster). */
const GERAET = Math.random().toString(36).slice(2, 12);

/** Ueber `lib/ipc` — in der App direkt, auf dem Tablet ueber die
 *  LAN-Bruecke (seit v1.8.1, eigener Telemetrie-Kanal, 10 Frames/s). */
const tauriQuelle: Datenquelle = {
  start: () => invoke<StartAntwort>("telemetrie_start", { geraet: GERAET }),
  abonnieren: (cb) => listen<Frame>("telemetrie-frame", (e) => cb(e.payload)),
  halten: () => {
    void invoke("telemetrie_halten", { geraet: GERAET }).catch(() => undefined);
  },
  stop: () => {
    void invoke("telemetrie_stop", { geraet: GERAET }).catch(() => undefined);
  },
};

export function useTelemetrie(quelle?: Datenquelle): Telemetrie {
  const q = quelle ?? tauriQuelle;
  const [zustand, setZustand] = useState<Zustand>("laedt");
  const [fehler, setFehler] = useState<string | null>(null);
  const [katalog, setKatalog] = useState<Katalog | null>(null);
  const [version, setVersion] = useState(0);
  const [angehalten, setAngehaltenState] = useState(false);
  const [marker, setMarker] = useState<Marker[]>([]);

  const frames = useRef<Frame[]>([]);
  const ereignisse = useRef<Ereignis[]>([]);
  const angehaltenRef = useRef(false);
  const zeichnenGeplant = useRef<number | null>(null);
  const zuletztGezeichnet = useRef(0);
  const indexRef = useRef<Map<string, number>>(new Map());

  const neuZeichnen = useCallback(() => {
    if (angehaltenRef.current || zeichnenGeplant.current !== null) return;
    const warten = Math.max(0, ZEICHNEN_MS - (Date.now() - zuletztGezeichnet.current));
    zeichnenGeplant.current = window.setTimeout(() => {
      zeichnenGeplant.current = null;
      zuletztGezeichnet.current = Date.now();
      setVersion((v) => v + 1);
    }, warten);
  }, []);

  useEffect(() => {
    if (!q) return;
    let aus = false;
    let abbestellen: (() => void) | null = null;

    const aufnehmen = (f: Frame) => {
      const liste = frames.current;
      const vorher = liste.length > 0 ? liste[liste.length - 1] : null;
      // Zeitlich rueckwaerts (Verlauf und Strom ueberlappen) → verwerfen.
      if (vorher && f.t <= vorher.t) return;
      liste.push(f);
      const neu = ereignisseAus(vorher, f, indexRef.current);
      if (neu.length) {
        ereignisse.current = [...ereignisse.current, ...neu].slice(-200);
      }
      const grenze = f.t - PUFFER_MS;
      let weg = 0;
      while (weg < liste.length && liste[weg].t < grenze) weg++;
      if (weg > 0) liste.splice(0, weg);
    };

    (async () => {
      try {
        // Zuerst abonnieren, dann starten: sonst fehlen die Frames zwischen
        // Verlaufsabzug und erstem Ereignis.
        const puffer: Frame[] = [];
        let gestartet = false;
        abbestellen = await q.abonnieren((f) => {
          if (aus) return;
          if (!gestartet) {
            puffer.push(f);
            return;
          }
          aufnehmen(f);
          neuZeichnen();
        });
        // Waehrend des Anmeldens schon wieder geschlossen: sofort abmelden,
        // sonst bliebe der Listener haengen (Codex-Befund 4).
        if (aus) {
          abbestellen();
          return;
        }
        const antwort = await q.start();
        if (aus) return;
        const idx = new Map<string, number>();
        antwort.katalog.zahlen.forEach((k, i) => idx.set(k.id, i));
        indexRef.current = idx;
        setKatalog(antwort.katalog);
        frames.current = [];
        ereignisse.current = [];
        for (const f of antwort.verlauf) aufnehmen(f);
        for (const f of puffer) aufnehmen(f);
        gestartet = true;
        setZustand("bereit");
        setVersion((v) => v + 1);
      } catch (e) {
        if (aus) return;
        setFehler(e instanceof Error ? e.message : String(e));
        setZustand("fehler");
      }
    })();

    const halten = window.setInterval(() => q.halten(), HALTEN_MS);
    return () => {
      aus = true;
      window.clearInterval(halten);
      if (zeichnenGeplant.current !== null) {
        window.clearTimeout(zeichnenGeplant.current);
        zeichnenGeplant.current = null;
      }
      abbestellen?.();
      q.stop();
    };
  }, [q, neuZeichnen]);

  const setAngehalten = useCallback(
    (v: boolean) => {
      angehaltenRef.current = v;
      setAngehaltenState(v);
      if (!v) neuZeichnen();
    },
    [neuZeichnen],
  );

  const markerSetzen = useCallback((text: string) => {
    const liste = frames.current;
    const t = liste.length ? liste[liste.length - 1].t : Date.now();
    setMarker((m) => [...m, { t, text }]);
  }, []);

  const zahlIndex = useMemo(() => {
    const m = new Map<string, number>();
    katalog?.zahlen.forEach((k, i) => m.set(k.id, i));
    return m;
  }, [katalog]);
  const textIndex = useMemo(() => {
    const m = new Map<string, number>();
    katalog?.texte.forEach((k, i) => m.set(k.id, i));
    return m;
  }, [katalog]);
  const kanalMap = useMemo(() => {
    const m = new Map<string, Kanal>();
    katalog?.zahlen.forEach((k) => m.set(k.id, k));
    katalog?.texte.forEach((k) => m.set(k.id, k));
    return m;
  }, [katalog]);

  const liste = frames.current;
  const letzter = liste.length ? liste[liste.length - 1] : null;

  const wert = useCallback(
    (id: string, f?: Frame | null) => {
      const i = zahlIndex.get(id);
      const fr = f === undefined ? letzter : f;
      if (i === undefined || !fr) return null;
      const v = fr.z[i];
      return typeof v === "number" && Number.isFinite(v) ? v : null;
    },
    [zahlIndex, letzter],
  );
  const text = useCallback(
    (id: string, f?: Frame | null) => {
      const i = textIndex.get(id);
      const fr = f === undefined ? letzter : f;
      if (i === undefined || !fr) return null;
      return fr.s[i] ?? null;
    },
    [textIndex, letzter],
  );

  return {
    zustand,
    fehler,
    katalog,
    zahlIndex,
    textIndex,
    kanal: (id) => kanalMap.get(id),
    frames: liste,
    letzter,
    version,
    angehalten,
    setAngehalten,
    ereignisse: ereignisse.current,
    marker,
    markerSetzen,
    wert,
    text,
  };
}
