// (i)-Knopf mit Erklärung zum Antippen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN) — die
// gespiegelten Abschnitte der Landungsanzeige erklären ihre Werte auf beiden
// Seiten gleich. Bis 05.10.2026 in LandingPanel.tsx.

import { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import "./infoBadge.css";

// ---- (i) info badge — small click-to-toggle popover --------------------
//
// Pilots may not know what "V/S σ" or "Bahn-Auslastung" means precisely.
// Each value carries a small (i) icon that, when clicked, reveals an
// explanation. We use a click instead of hover so it's tappable on touch
// devices and stays open while the pilot reads it.
//
// 05.10.2026 (Thomas: eine Variante — der (i)-Knopf): offen bleibt die
// Erklärung, bis man den Knopf noch einmal, das × oder daneben antippt oder
// Escape drückt. Kein Hover — auf dem iPhone gibt es keins.
//
// Das Fenster liegt FEST über der Seite (position: fixed), am Knopf
// ausgerichtet. Vorher hing es absolut unter dem Knopf und wurde in den
// Wertzeilen abgeschnitten — die schneiden Überstand für „…" ab
// (overflow: hidden), im Client wie in der Webapp.

/** Öffnet sich ein (i), schließen alle anderen — sonst blieben per Tastatur
 *  beliebig viele offen (geschlossen wird sonst nur durch Tippen daneben).
 *  QS 06.10.2026. */
const EREIGNIS_AUF = "info-badge-auf";

/** Abstand zum Fensterrand und zum Knopf, in px. */
const RAND = 8;
const BREITE = 280;

/**
 * Der Bezugsrahmen eines `position: fixed`-Elements: normalerweise das
 * Fenster — aber ein Vorfahr mit transform, filter, backdrop-filter,
 * perspective oder contain übernimmt die Rolle (QS 06.10.2026: das Modal der
 * Webapp hat `backdrop-filter: blur()` und `overflow: hidden`; das Fenster
 * saß um dessen Rand versetzt und wurde dort abgeschnitten).
 */
function bezugsrahmen(el: Element | null): Element | null {
  for (let e: Element | null = el; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
    const cs = getComputedStyle(e) as CSSStyleDeclaration & { webkitBackdropFilter?: string };
    const gesetzt = (v: string | undefined) => v != null && v !== "" && v !== "none";
    if (
      gesetzt(cs.transform) ||
      gesetzt(cs.filter) ||
      gesetzt(cs.backdropFilter) ||
      gesetzt(cs.webkitBackdropFilter) ||
      gesetzt(cs.perspective) ||
      gesetzt(cs.translate) ||
      gesetzt(cs.scale) ||
      gesetzt(cs.rotate) ||
      /paint|layout|strict|content/.test(cs.contain ?? "") ||
      /transform|filter|perspective/.test(cs.willChange ?? "")
    ) {
      return e;
    }
  }
  return null;
}

interface Lage {
  top: number;
  left: number;
  /** Abstand der Pfeilspitze vom linken Fensterrand. */
  pfeil: number;
  /** Fenster über dem Knopf (unten war kein Platz). */
  oben: boolean;
  /** Breite in px (schmaler, wenn der sichtbare Bereich schmal ist). */
  breite: number;
}

/**
 * QS 06.10.2026 (Runde 8): Die Erklärungen beschreiben die heutigen Regeln
 * (Score-Version 19). Altbestand wird nie neu gerechnet — an einer Landung
 * vor Version 19 hängt jedes Fenster deshalb denselben Hinweis an, statt dass
 * jeder Text eine eigene Altfassung braucht. Gesetzt an der Wurzel der
 * Landungsanzeige (Client `LandungsAbschnitte`, Webapp `LandingAnalysis`).
 */
export const AltbestandKontext = createContext(false);

export function InfoBadge({ explanation }: { explanation: string }) {
  const { t } = useTranslation();
  const altbestand = useContext(AltbestandKontext);
  const text = altbestand ? `${explanation} ${t("landing.erklaer.alt_hinweis")}` : explanation;
  const [open, setOpen] = useState(false);
  const id = useId();
  const [lage, setLage] = useState<Lage | null>(null);
  const wrap = useRef<HTMLSpanElement>(null);
  const knopf = useRef<HTMLButtonElement>(null);
  const fenster = useRef<HTMLSpanElement>(null);
  // Das Fenster hängt NICHT am Knopf (QS 06.10.2026): in einer Kachel mit
  // `opacity` (Anflugkarte, nicht bewertete Teilnote) war es durchsichtig,
  // und Nachbarkacheln malten darüber. Ziel ist die Hülle der Webapp-
  // Landungsansicht (dort hängen die Farbvariablen), sonst `body`.
  const ziel = useRef<Element | null>(null);

  // Unter dem Knopf, links bündig; am Fensterrand eingerückt. Passt es
  // unten nicht hin, über den Knopf.
  const ausrichten = () => {
    if (!knopf.current) return;
    const r = knopf.current.getBoundingClientRect();
    // Sichtbarer Bereich: Fenster, geschnitten mit dem Bezugsrahmen (siehe
    // `bezugsrahmen`) — in dessen Koordinaten wird `top/left` gesetzt.
    const rahmen = bezugsrahmen(ziel.current);
    const b = rahmen?.getBoundingClientRect();
    const x0 = Math.max(0, b ? b.left + (rahmen as HTMLElement).clientLeft : 0);
    const y0 = Math.max(0, b ? b.top + (rahmen as HTMLElement).clientTop : 0);
    const x1 = Math.min(window.innerWidth, b ? b.right : window.innerWidth);
    const y1 = Math.min(window.innerHeight, b ? b.bottom : window.innerHeight);
    const breite = Math.min(BREITE, x1 - x0 - 2 * RAND);
    let left = r.left - RAND;
    if (left + breite > x1 - RAND) left = x1 - RAND - breite;
    if (left < x0 + RAND) left = x0 + RAND;
    const hoehe = fenster.current?.getBoundingClientRect().height ?? 0;
    const unten = r.bottom + RAND;
    const oben = unten + hoehe > y1 - RAND && r.top - RAND - hoehe >= y0 + RAND;
    const top = oben ? r.top - RAND - hoehe : unten;
    // Bezugsrahmen: Koordinaten relativ zu seiner Innenkante.
    const dx = b ? b.left + (rahmen as HTMLElement).clientLeft : 0;
    const dy = b ? b.top + (rahmen as HTMLElement).clientTop : 0;
    setLage({
      top: top - dy,
      left: left - dx,
      breite,
      pfeil: r.left + r.width / 2 - left - 6,
      oben,
    });
  };
  useLayoutEffect(() => {
    if (open) ausrichten();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const daneben = (e: PointerEvent) => {
      const n = e.target as Node;
      if (wrap.current?.contains(n) || fenster.current?.contains(n)) return;
      setOpen(false);
    };
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        knopf.current?.focus();
      }
    };
    const anderesAuf = (e: Event) => {
      if ((e as CustomEvent<string>).detail !== id) setOpen(false);
    };
    // Festes Fenster: beim Rollen und bei Größenänderung folgt es dem
    // Knopf. Schließen wäre auf dem iPhone falsch — dort rollt schon das
    // Ein- und Ausblenden der Adressleiste.
    document.addEventListener("pointerdown", daneben);
    document.addEventListener("keydown", taste);
    window.addEventListener(EREIGNIS_AUF, anderesAuf);
    window.addEventListener("scroll", ausrichten, true);
    window.addEventListener("resize", ausrichten);
    return () => {
      document.removeEventListener("pointerdown", daneben);
      document.removeEventListener("keydown", taste);
      window.removeEventListener(EREIGNIS_AUF, anderesAuf);
      window.removeEventListener("scroll", ausrichten, true);
      window.removeEventListener("resize", ausrichten);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    <span className="info-badge-wrap" ref={wrap}>
      <button
        ref={knopf}
        type="button"
        className={`info-badge ${open ? "info-badge--open" : ""}`}
        onClick={(e) => {
          e.stopPropagation();
          setLage(null);
          ziel.current = knopf.current?.closest(".la-shell") ?? document.body;
          if (!open) window.dispatchEvent(new CustomEvent(EREIGNIS_AUF, { detail: id }));
          setOpen(!open);
        }}
        aria-label={t("landing.info_badge.oeffnen")}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
      >
        i
      </button>
      {open && ziel.current && createPortal(
        <span
          ref={fenster}
          className={`info-badge__popover${lage?.oben ? " info-badge__popover--oben" : ""}`}
          id={id}
          // Ein Fenster mit Schließen-Knopf ist kein Tooltip (der darf nichts
          // Bedienbares enthalten).
          role="dialog"
          aria-label={t("landing.info_badge.oeffnen")}
          style={{
            top: lage?.top ?? 0,
            left: lage?.left ?? 0,
            ...(lage ? { width: lage.breite } : {}),
            // Erst messen, dann zeigen — sonst blitzt es kurz oben links.
            visibility: lage ? "visible" : "hidden",
            ["--pfeil" as string]: `${lage?.pfeil ?? 12}px`,
          }}
        >
          {text}
          <button
            type="button"
            className="info-badge__close"
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
            aria-label={t("landing.info_badge.schliessen")}
          >
            ×
          </button>
        </span>,
        ziel.current,
      )}
    </span>
  );
}
