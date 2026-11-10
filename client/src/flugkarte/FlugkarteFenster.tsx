// Flugkarte als Vollbild-Fenster in AeroACARS (Logbuch, Landungs-Tab).
//
// Holt die Flugakte über den Tauri-Befehl `flugakte_laden` vom Live-Server
// (Pilot-Route, nur eigene Flüge) und zeigt sie mit dem Baustein flugkarte.ts —
// demselben, den der Admin in der Webapp sieht (Thomas 28.10.2026: „eine Sicht“).
// v3 (Etappe 5, P-8.21): Der Baustein lebt nur hier und kommt mit der Oberfläche
// ins Laufzeit-Paket; die Admin-Webapp lädt ihn als Seite ui/flugkarte/ aus dem
// freigegebenen Paket (seite/). Keine Kopie in aeroacars-live mehr.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import FlugkarteFlaeche from "./FlugkarteFlaeche";
import type { FlugkarteOptionen } from "./flugkarte";
import "./fenster.css";

interface Props {
  pirepId: string;
  /** Vom Logbuch: dieselben Bordbuch-Punkte wie auf der eingebetteten Karte */
  zusatz?: Pick<FlugkarteOptionen, "zusatzEreignisse" | "zusatzKategorien">;
  onClose: () => void;
}

export default function FlugkarteFenster({ pirepId, zusatz, onClose }: Props) {
  const fenster = useRef<HTMLDivElement>(null);
  const zuKnopf = useRef<HTMLButtonElement>(null);
  const { t } = useTranslation();
  const [bildZiel, setBildZiel] = useState<HTMLElement | null>(null);

  // Fokus: beim Oeffnen auf „Schliessen“, im Fenster halten, danach zurueck zum Ausloeser
  useEffect(() => {
    setBildZiel(fenster.current);
    const vorher = document.activeElement as HTMLElement | null;
    zuKnopf.current?.focus();
    let zuletzt: Element | null = document.activeElement;
    const halten = (e: FocusEvent) => {
      const f = fenster.current;
      if (!f) return;
      if (f.contains(e.target as Node)) { zuletzt = e.target as Element; return; }
      const ziele = Array.from(f.querySelectorAll<HTMLElement>("button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex='-1'])"));
      (zuletzt === ziele[0] ? ziele[ziele.length - 1] : ziele[0])?.focus();
    };
    document.addEventListener("focusin", halten);
    return () => { document.removeEventListener("focusin", halten); vorher?.focus?.(); };
  }, []);

  useEffect(() => {
    const taste = (e: KeyboardEvent) => { if (e.key === "Escape" && !e.defaultPrevented) onClose(); }; // Esc in einem Kartenmenue schliesst nur das Menue
    window.addEventListener("keydown", taste);
    return () => window.removeEventListener("keydown", taste);
  }, [onClose]);

  return (
    <div className="flugkarte-fenster" ref={fenster} role="dialog" aria-modal="true" aria-label={t("flugkarte.fenster.titel")}>
      <button ref={zuKnopf} className="flugkarte-zu" onClick={onClose} aria-label={t("flugkarte.fenster.zu")} title={`PIREP ${pirepId}`}>✕</button>
      <FlugkarteFlaeche pirepId={pirepId} bildZiel={bildZiel} zusatz={zusatz} />
    </div>
  );
}
