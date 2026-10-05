// (i)-Knopf mit Erklärung zum Antippen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN) — die
// gespiegelten Abschnitte der Landungsanzeige erklären ihre Werte auf beiden
// Seiten gleich. Bis 05.10.2026 in LandingPanel.tsx.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
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

/** Abstand zum Fensterrand und zum Knopf, in px. */
const RAND = 8;
const BREITE = 280;

interface Lage {
  top: number;
  left: number;
  /** Abstand der Pfeilspitze vom linken Fensterrand. */
  pfeil: number;
  /** Fenster über dem Knopf (unten war kein Platz). */
  oben: boolean;
}

export function InfoBadge({ explanation }: { explanation: string }) {
  const [open, setOpen] = useState(false);
  const [lage, setLage] = useState<Lage | null>(null);
  const wrap = useRef<HTMLSpanElement>(null);
  const knopf = useRef<HTMLButtonElement>(null);
  const fenster = useRef<HTMLSpanElement>(null);

  // Unter dem Knopf, links bündig; am Fensterrand eingerückt. Passt es
  // unten nicht hin, über den Knopf.
  const ausrichten = () => {
    if (!knopf.current) return;
    const r = knopf.current.getBoundingClientRect();
    const breite = Math.min(BREITE, window.innerWidth - 2 * RAND);
    let left = r.left - RAND;
    if (left + breite > window.innerWidth - RAND) left = window.innerWidth - RAND - breite;
    if (left < RAND) left = RAND;
    const hoehe = fenster.current?.getBoundingClientRect().height ?? 0;
    const unten = r.bottom + RAND;
    const oben = unten + hoehe > window.innerHeight - RAND && r.top - RAND - hoehe >= RAND;
    setLage({
      top: oben ? r.top - RAND - hoehe : unten,
      left,
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
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    // Festes Fenster: beim Rollen und bei Größenänderung folgt es dem
    // Knopf. Schließen wäre auf dem iPhone falsch — dort rollt schon das
    // Ein- und Ausblenden der Adressleiste.
    document.addEventListener("pointerdown", daneben);
    document.addEventListener("keydown", taste);
    window.addEventListener("scroll", ausrichten, true);
    window.addEventListener("resize", ausrichten);
    return () => {
      document.removeEventListener("pointerdown", daneben);
      document.removeEventListener("keydown", taste);
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
          setOpen((v) => !v);
        }}
        aria-label="info"
        aria-expanded={open}
      >
        i
      </button>
      {open && (
        <span
          ref={fenster}
          className={`info-badge__popover${lage?.oben ? " info-badge__popover--oben" : ""}`}
          role="tooltip"
          style={{
            top: lage?.top ?? 0,
            left: lage?.left ?? 0,
            // Erst messen, dann zeigen — sonst blitzt es kurz oben links.
            visibility: lage ? "visible" : "hidden",
            ["--pfeil" as string]: `${lage?.pfeil ?? 12}px`,
          }}
        >
          {explanation}
          <button
            type="button"
            className="info-badge__close"
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
            aria-label="close"
          >
            ×
          </button>
        </span>
      )}
    </span>
  );
}
