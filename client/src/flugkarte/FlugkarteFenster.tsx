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
import { invoke } from "../lib/ipc";
import { flugkarteAnzeigen } from "./flugkarte";
import type { Flugakte } from "./flugakteTypen";
import stilUrl from "./stil-dunkel.json?url";
import "./fenster.css";

export default function FlugkarteFenster({ pirepId, onClose }: { pirepId: string; onClose: () => void }) {
  const feld = useRef<HTMLDivElement>(null);
  const fenster = useRef<HTMLDivElement>(null);
  const zuKnopf = useRef<HTMLButtonElement>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [laedt, setLaedt] = useState(true);
  const { t, i18n } = useTranslation();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "en";

  useEffect(() => {
    let weg: (() => void) | null = null, abgebrochen = false;
    setFehler(null); setLaedt(true);
    invoke<string | null>("flugakte_laden", { pirepId })
      .then((text) => {
        if (abgebrochen || !feld.current) return;
        if (text == null) { setFehler(t("flugkarte.fenster.kein_protokoll")); return; }
        const akte = JSON.parse(text) as Flugakte;
        const start = akte.kopf?.start ? new Date(akte.kopf.start) : null;
        weg = flugkarteAnzeigen(feld.current, akte, {
          stilUrl,
          // Texte aus common.json (flugkarte.*), dieselben wie in der Webapp; fehlt einer, bleibt der deutsche aus der Akte
          t: (k, w) => (i18n.exists(k) ? i18n.t(k, w) : null),
          sprache,
          datum: start ? start.toLocaleDateString(sprache, { timeZone: "UTC" }) : undefined,
          // nur im Entwicklungsmodus: Karte fuer Browser-Tests greifbar (wie in der Webapp)
          beiKarte: import.meta.env.DEV ? (karte) => { (window as unknown as { __flugkarte: unknown }).__flugkarte = karte; } : undefined,
        });
      })
      .catch((e: unknown) => {
        if (abgebrochen) return;
        const f = e as { code?: string; message?: string };
        setFehler(f?.code === "flugakte_kein_zugang"
          ? t("flugkarte.fenster.kein_zugang")
          : t("flugkarte.fenster.fehler", { fehler: f?.message ?? String(e) }));
      })
      .finally(() => { if (!abgebrochen) setLaedt(false); });
    return () => { abgebrochen = true; weg?.(); };
  }, [pirepId, sprache]); // neue Sprache: neu aufbauen (der Baustein holt seine Texte einmal)

  // Fokus: beim Oeffnen auf „Schliessen“, im Fenster halten, danach zurueck zum Ausloeser
  useEffect(() => {
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
      {laedt && <div className="flugkarte-hinweis">{t("flugkarte.fenster.laedt")}</div>}
      {fehler && <div className="flugkarte-hinweis">{fehler}</div>}
      <div className="flugkarte-inhalt" ref={feld} />
    </div>
  );
}
