/**
 * Notizblock (v1.8.1) — Schreibflaeche fuer den Apple Pencil in der
 * Tablet-Ansicht (LAN-Fernbedienung).
 *
 * - Stift mit Andruck, drei Farben, Radierer fuer einzelne Striche,
 *   Rueckgaengig, „Alles loeschen" mit Rueckfrage.
 * - Handballen-Schutz: Sobald ein Stift erkannt wurde, malt der Finger nicht
 *   mehr (umschaltbar). Ohne Stift malt der Finger.
 * - Der Inhalt bleibt auf dem Geraet gespeichert (localStorage), er geht
 *   nicht an den Sim-PC.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui";
import {
  HANDBALLEN_NACHLAUF_MS,
  SPEICHER_SCHLUESSEL,
  Verlauf,
  darfZeichnen,
  kompakt,
  laden,
  pixelBreite,
  trifft,
  type Punkt,
  type Strich,
} from "./logik";
import "./notizblock.css";

type Werkzeug = "stift" | "radierer";
const FARBEN = ["--text", "--acc", "--sem-danger"] as const;
const BREITEN = { fein: 0.0022, mittel: 0.0035 } as const;
const RADIER_RADIUS = 0.012;

const SPEICHER_FINGER = "aeroacars.notizblock.finger";
/** Einmal einen Stift gesehen → auch nach Neuladen bleibt der Finger aus. */
const SPEICHER_STIFT = "aeroacars.notizblock.stift";

function lesen(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}
function schreiben(k: string, v: string): boolean {
  try {
    localStorage.setItem(k, v);
    return true;
  } catch {
    // Speicher voll oder privater Modus — der Aufrufer zeigt es an.
    return false;
  }
}

function farbwert(el: Element, token: string): string {
  return getComputedStyle(el).getPropertyValue(token).trim() || "#888";
}

export function Notizblock() {
  const { t } = useTranslation();
  const rahmen = useRef<HTMLDivElement>(null);
  const leinwand = useRef<HTMLCanvasElement>(null);
  // Einmal laden, nicht bei jedem Rendern neu parsen.
  const [anfang] = useState<Strich[]>(() => laden(lesen(SPEICHER_SCHLUESSEL)));
  const striche = useRef<Strich[]>(anfang);
  const verlauf = useRef(new Verlauf());
  const aktuell = useRef<{ id: number; strich: Strich; typ: string } | null>(null);
  // Finger-Striche der letzten Sekunden: kommt kurz danach der erste Stift,
  // war es der Handballen — dann verschwinden sie wieder.
  const juengsteFingerStriche = useRef<Array<{ strich: Strich; ende: number }>>([]);
  const radiertIn = useRef<number | null>(null);
  const stiftGesehen = useRef(lesen(SPEICHER_STIFT) === "1");

  const [werkzeug, setWerkzeug] = useState<Werkzeug>("stift");
  const [farbe, setFarbe] = useState<(typeof FARBEN)[number]>("--text");
  const [breite, setBreite] = useState<keyof typeof BREITEN>("fein");
  const [finger, setFingerState] = useState(() => lesen(SPEICHER_FINGER) === "1");
  const [loeschenFragen, setLoeschenFragen] = useState(false);
  const [kannZurueck, setKannZurueck] = useState(false);
  const [hoehe, setHoehe] = useState(500);
  const [nichtGespeichert, setNichtGespeichert] = useState(false);

  const setFinger = (v: boolean) => {
    setFingerState(v);
    schreiben(SPEICHER_FINGER, v ? "1" : "0");
  };

  const speichern = useCallback(() => {
    // Schlaegt das Speichern fehl (Speicher voll, privater Modus), bleibt
    // der Inhalt sichtbar, ueberlebt aber kein Neuladen — das muss der
    // Pilot wissen, statt es spaeter zu merken.
    setNichtGespeichert(!schreiben(SPEICHER_SCHLUESSEL, JSON.stringify(kompakt(striche.current))));
    setKannZurueck(!verlauf.current.leer);
  }, []);

  /* ---------------------------------------------------------- Zeichnen */

  const zeichneStrich = useCallback((ctx: CanvasRenderingContext2D, s: Strich, w: number) => {
    const el = leinwand.current;
    if (!el || s.punkte.length === 0) return;
    ctx.strokeStyle = s.farbe.startsWith("--") ? farbwert(el, s.farbe) : s.farbe;
    ctx.fillStyle = ctx.strokeStyle;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    if (s.punkte.length === 1) {
      const q = s.punkte[0];
      ctx.beginPath();
      ctx.arc(q.x * w, q.y * w, pixelBreite(s, q.p, w) / 2, 0, Math.PI * 2);
      ctx.fill();
      return;
    }
    // Abschnittsweise, damit der Andruck die Breite unterwegs aendern kann;
    // Mittelpunkte als Stuetzen glaetten die Linie.
    for (let i = 1; i < s.punkte.length; i++) {
      const a = s.punkte[i - 1];
      const b = s.punkte[i];
      const vor = s.punkte[i - 2] ?? a;
      // y ist in Breiten-Einheiten gespeichert (siehe logik.ts).
      const mx0 = ((vor.x + a.x) / 2) * w;
      const my0 = ((vor.y + a.y) / 2) * w;
      const mx1 = ((a.x + b.x) / 2) * w;
      const my1 = ((a.y + b.y) / 2) * w;
      ctx.lineWidth = pixelBreite(s, (a.p + b.p) / 2, w);
      ctx.beginPath();
      ctx.moveTo(i === 1 ? a.x * w : mx0, i === 1 ? a.y * w : my0);
      ctx.quadraticCurveTo(a.x * w, a.y * w, mx1, my1);
      if (i === s.punkte.length - 1) ctx.lineTo(b.x * w, b.y * w);
      ctx.stroke();
    }
  }, []);

  const allesZeichnen = useCallback(() => {
    const el = leinwand.current;
    if (!el) return;
    const ctx = el.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const w = el.width / dpr;
    const h = el.height / dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    for (const s of striche.current) zeichneStrich(ctx, s, w);
    if (aktuell.current) zeichneStrich(ctx, aktuell.current.strich, w);
  }, [zeichneStrich]);

  // Groesse: Flaeche fuellt den Platz bis zum unteren Bildschirmrand.
  useEffect(() => {
    const box = rahmen.current;
    const el = leinwand.current;
    if (!box || !el) return;
    const anpassen = () => {
      const oben = box.getBoundingClientRect().top;
      const verfuegbar = Math.max(320, window.innerHeight - oben - 16);
      setHoehe(verfuegbar);
      const dpr = window.devicePixelRatio || 1;
      const b = box.clientWidth;
      el.width = Math.round(b * dpr);
      el.height = Math.round(verfuegbar * dpr);
      el.style.width = `${b}px`;
      el.style.height = `${verfuegbar}px`;
      allesZeichnen();
    };
    anpassen();
    const ro = new ResizeObserver(anpassen);
    ro.observe(box);
    window.addEventListener("resize", anpassen);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", anpassen);
    };
  }, [allesZeichnen]);

  // Themenwechsel: Farben kommen aus Tokens, also neu zeichnen.
  useEffect(() => {
    const mo = new MutationObserver(allesZeichnen);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => mo.disconnect();
  }, [allesZeichnen]);

  /* ----------------------------------------------------------- Eingabe */

  const relativ = (e: { clientX: number; clientY: number; pressure: number; pointerType: string }): Punkt => {
    const r = leinwand.current!.getBoundingClientRect();
    return {
      x: (e.clientX - r.left) / r.width,
      y: (e.clientY - r.top) / r.width,
      // Maus meldet 0.5 beim Druecken, Finger oft 0 — beides wie mittlerer Druck.
      p: e.pointerType === "pen" && e.pressure > 0 ? e.pressure : 0.5,
    };
  };

  const darf = (e: React.PointerEvent) =>
    darfZeichnen(e.pointerType, e.width, e.height, finger, stiftGesehen.current);

  /** Erster Stiftkontakt: merken und frische Finger-Striche als Handballen
   *  entfernen. */
  const stiftErkannt = () => {
    const ersterStift = !stiftGesehen.current;
    stiftGesehen.current = true;
    if (ersterStift) schreiben(SPEICHER_STIFT, "1");
    if (finger) return;
    const grenze = Date.now() - HANDBALLEN_NACHLAUF_MS;
    const weg = new Set(
      juengsteFingerStriche.current.filter((f) => f.ende >= grenze).map((f) => f.strich),
    );
    juengsteFingerStriche.current = [];
    if (weg.size === 0) return;
    verlauf.current.merken(striche.current);
    striche.current = striche.current.filter((s) => !weg.has(s));
    speichern();
  };

  // Zustand vor dem ersten Treffer dieses Radiervorgangs — gemerkt wird er
  // erst, wenn wirklich etwas verschwindet (Cloud-QS Befund 5: sonst legte
  // jedes Antippen ins Leere einen wirkungslosen Rueckgaengig-Schritt an).
  const vorRadieren = useRef<Strich[] | null>(null);
  const radieren = (q: Punkt) => {
    const vorher = striche.current;
    const bleiben = vorher.filter((s) => !trifft(s, q.x, q.y, RADIER_RADIUS));
    if (bleiben.length !== vorher.length) {
      if (vorRadieren.current) {
        verlauf.current.merken(vorRadieren.current);
        vorRadieren.current = null;
      }
      striche.current = bleiben;
      allesZeichnen();
    }
  };

  const onDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.pointerType === "pen") {
      stiftErkannt();
      // Laeuft gerade ein Finger-/Handballen-Strich, gewinnt der Stift: der
      // Handstrich wird verworfen statt den Stift zu blockieren.
      if (aktuell.current && aktuell.current.typ !== "pen" && !finger) {
        aktuell.current = null;
        allesZeichnen();
      }
    }
    if (!darf(e) || aktuell.current || radiertIn.current !== null) return;
    e.preventDefault();
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // Ohne Capture geht es auch — nur ein Strich, der den Rand verlaesst,
      // endet dann dort.
    }
    const q = relativ(e);
    setLoeschenFragen(false);
    // Radierer: auch die Radierer-Taste mancher Stifte (buttons & 32).
    if (werkzeug === "radierer" || (e.buttons & 32) !== 0) {
      vorRadieren.current = striche.current;
      radiertIn.current = e.pointerId;
      radieren(q);
      return;
    }
    aktuell.current = {
      id: e.pointerId,
      strich: { farbe, breite: BREITEN[breite], punkte: [q] },
      typ: e.pointerType,
    };
    allesZeichnen();
  };

  const onMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (radiertIn.current === e.pointerId) {
      radieren(relativ(e));
      return;
    }
    const a = aktuell.current;
    if (!a || a.id !== e.pointerId) return;
    e.preventDefault();
    // Zwischenpunkte des Stifts (bis 240 Hz) fuer glatte Linien.
    // Kann leer sein (manche Browser, synthetische Ereignisse) — dann das
    // Ereignis selbst nehmen, sonst gingen Bewegungen verloren.
    const zwischen = e.nativeEvent.getCoalescedEvents?.() ?? [];
    const ereignisse = zwischen.length ? zwischen : [e.nativeEvent];
    for (const ce of ereignisse) a.strich.punkte.push(relativ(ce));
    allesZeichnen();
  };

  const onUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (radiertIn.current === e.pointerId) {
      radiertIn.current = null;
      const nichtsGeloescht = vorRadieren.current !== null;
      vorRadieren.current = null;
      if (!nichtsGeloescht) speichern();
      return;
    }
    const a = aktuell.current;
    if (!a || a.id !== e.pointerId) return;
    aktuell.current = null;
    // iPadOS bricht erkannte Handballen-Beruehrungen mit „pointercancel" ab —
    // ein so beendeter Finger-Strich wird verworfen, nicht gespeichert.
    if (e.type === "pointercancel" && a.typ !== "pen") {
      allesZeichnen();
      return;
    }
    verlauf.current.merken(striche.current);
    striche.current = [...striche.current, a.strich];
    if (a.typ === "touch" && !stiftGesehen.current) {
      juengsteFingerStriche.current = [
        ...juengsteFingerStriche.current.slice(-9),
        { strich: a.strich, ende: Date.now() },
      ];
    }
    allesZeichnen();
    speichern();
  };

  /* ------------------------------------------------------------ Aktionen */

  const zurueck = () => {
    const z = verlauf.current.zurueck();
    if (!z) return;
    striche.current = z;
    allesZeichnen();
    speichern();
  };

  const allesLoeschen = () => {
    if (!loeschenFragen) {
      setLoeschenFragen(true);
      return;
    }
    verlauf.current.merken(striche.current);
    striche.current = [];
    setLoeschenFragen(false);
    allesZeichnen();
    speichern();
  };

  // Rueckfrage nach ein paar Sekunden zuruecknehmen.
  useEffect(() => {
    if (!loeschenFragen) return;
    const id = window.setTimeout(() => setLoeschenFragen(false), 4000);
    return () => window.clearTimeout(id);
  }, [loeschenFragen]);

  return (
    <div className="notiz">
      <div className="notiz-leiste" role="toolbar" aria-label={t("notizblock.werkzeuge")}>
        <div className="notiz-gruppe" role="group">
          <button
            type="button"
            className={werkzeug === "stift" ? "notiz-knopf notiz-knopf--an" : "notiz-knopf"}
            aria-pressed={werkzeug === "stift"}
            onClick={() => setWerkzeug("stift")}
          >
            {t("notizblock.stift")}
          </button>
          <button
            type="button"
            className={werkzeug === "radierer" ? "notiz-knopf notiz-knopf--an" : "notiz-knopf"}
            aria-pressed={werkzeug === "radierer"}
            onClick={() => setWerkzeug("radierer")}
          >
            {t("notizblock.radierer")}
          </button>
        </div>
        <div className="notiz-gruppe" role="group" aria-label={t("notizblock.farbe")}>
          {FARBEN.map((f) => (
            <button
              key={f}
              type="button"
              className={farbe === f && werkzeug === "stift" ? "notiz-farbe notiz-farbe--an" : "notiz-farbe"}
              style={{ background: `var(${f})` }}
              aria-pressed={farbe === f}
              aria-label={t(`notizblock.farbe_${f.replace(/^--/, "").replace("sem-", "")}`)}
              onClick={() => {
                setFarbe(f);
                setWerkzeug("stift");
              }}
            />
          ))}
        </div>
        <div className="notiz-gruppe" role="group" aria-label={t("notizblock.strich")}>
          {(Object.keys(BREITEN) as Array<keyof typeof BREITEN>).map((b) => (
            <button
              key={b}
              type="button"
              className={breite === b ? "notiz-knopf notiz-knopf--an" : "notiz-knopf"}
              aria-pressed={breite === b}
              onClick={() => setBreite(b)}
            >
              {t(`notizblock.${b}`)}
            </button>
          ))}
        </div>
        <label className="notiz-schalter">
          <input type="checkbox" checked={finger} onChange={(e) => setFinger(e.target.checked)} />
          {t("notizblock.finger")}
        </label>
        <span className="notiz-luecke" />
        <Button size="sm" onClick={zurueck} disabled={!kannZurueck}>
          {t("notizblock.zurueck")}
        </Button>
        <Button size="sm" variant="danger" onClick={allesLoeschen}>
          {loeschenFragen ? t("notizblock.wirklich_loeschen") : t("notizblock.alles_loeschen")}
        </Button>
      </div>

      {nichtGespeichert && (
        <p className="notiz-warnung" role="status">
          {t("notizblock.nicht_gespeichert")}
        </p>
      )}
      <div ref={rahmen} className="notiz-flaeche" style={{ height: hoehe }}>
        <canvas
          ref={leinwand}
          className={werkzeug === "radierer" ? "notiz-canvas notiz-canvas--radierer" : "notiz-canvas"}
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
          aria-label={t("notizblock.flaeche")}
          role="img"
        />
      </div>
    </div>
  );
}
