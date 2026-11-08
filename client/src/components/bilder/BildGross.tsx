// Grossansicht eines Bildes (Lupe auf der Kachel, Bild in der Detailspalte, Punkte auf den Karten).
// Eigene Ebene ueber allem: `ziel` ist das Element, in das sie gehaengt wird; die Flugkarte gibt ihr
// Fenster an, weil dessen Fokusfalle Fokus von ausserhalb zurueckholt. Keine Secure-Context-APIs:
// am Tablet laeuft das ueber http://<LAN-IP>.
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { langZeit } from "./logik/metaFormat";
import type { Eintrag } from "./logik/typen";
import { bildZeitIso, useKachelTexte } from "./Kacheln";
import { Symbol } from "./Symbole";
import { useBildUrl } from "./useBildUrl";
import "./bilder.css";

export function BildGross({ bilder, startId, onZu, ziel }: { bilder: Eintrag[]; startId: string; onZu: () => void; ziel?: HTMLElement | null }) {
  const { t, phase, hoehe } = useKachelTexte();
  const [index, setIndex] = useState(() => Math.max(0, bilder.findIndex((b) => b.id === startId)));
  const [voll, setVoll] = useState(false);
  const e = bilder[index];
  const bild = useBildUrl(e.pfad);
  const vorschau = useBildUrl(e.vorschau_pfad ?? e.pfad);
  const flaeche = useRef<HTMLDivElement>(null);
  const buehne = useRef<HTMLDivElement>(null);
  const element = useRef<HTMLImageElement>(null);
  const zuKnopf = useRef<HTMLButtonElement>(null);
  /** Stelle im Bild (Anteile), die nach dem Umschalten auf volle Groesse in der Mitte stehen soll. */
  const mitte = useRef({ x: 0.5, y: 0.5 });
  const mehrere = bilder.length > 1;

  const blaettern = (schritt: number) => {
    setVoll(false);
    setIndex((i) => (i + schritt + bilder.length) % bilder.length);
  };

  // Der Tastengriff laeuft in der Capture-Phase: Esc schliesst sonst auch das Flugkarten-Fenster darunter.
  const griffe = useRef({ onZu, blaettern });
  griffe.current = { onZu, blaettern };
  useEffect(() => {
    const taste = (ev: KeyboardEvent) => {
      if (!["Escape", "ArrowLeft", "ArrowRight", "Tab"].includes(ev.key)) return;
      ev.preventDefault();
      ev.stopPropagation();
      if (ev.key === "Escape") griffe.current.onZu();
      else if (ev.key === "ArrowLeft") { if (bilder.length > 1) griffe.current.blaettern(-1); }
      else if (ev.key === "ArrowRight") { if (bilder.length > 1) griffe.current.blaettern(1); }
      else {
        const ziele = Array.from(flaeche.current?.querySelectorAll<HTMLElement>("button") ?? []);
        const jetzt = ziele.indexOf(document.activeElement as HTMLElement);
        const neu = jetzt < 0 ? 0 : (jetzt + (ev.shiftKey ? -1 : 1) + ziele.length) % ziele.length;
        ziele[neu]?.focus();
      }
    };
    window.addEventListener("keydown", taste, true);
    return () => window.removeEventListener("keydown", taste, true);
  }, [bilder.length]);

  useEffect(() => {
    const vorher = document.activeElement as HTMLElement | null;
    zuKnopf.current?.focus();
    return () => vorher?.focus?.();
  }, []);

  useLayoutEffect(() => {
    const b = buehne.current, i = element.current;
    if (!voll || !b || !i) return;
    b.scrollLeft = i.offsetLeft + mitte.current.x * i.offsetWidth - b.clientWidth / 2;
    b.scrollTop = i.offsetTop + mitte.current.y * i.offsetHeight - b.clientHeight / 2;
  }, [voll]);

  function bildKlick(ev: MouseEvent<HTMLImageElement>) {
    ev.stopPropagation();
    const r = ev.currentTarget.getBoundingClientRect();
    mitte.current = { x: (ev.clientX - r.left) / r.width, y: (ev.clientY - r.top) / r.height };
    setVoll((v) => !v);
  }

  const ph = phase(e);
  const url = bild ?? vorschau;
  return createPortal(
    <div className="bt-gross" ref={flaeche} role="dialog" aria-modal="true" aria-label={t("bilder.gross.titel")}>
      <div className="bt-gross__kopf">
        {mehrere && <span className="bt-gross__zahl">{t("bilder.gross.zaehler", { n: index + 1, m: bilder.length })}</span>}
        <span className="bt-gross__zeit">{langZeit(bildZeitIso(e))}</span>
        {ph && <span className="bt-gross__phase">{ph}</span>}
        <span className="bt-gross__hoehe">{hoehe(e)}</span>
        <button type="button" className="bt-gross__umschalten" aria-pressed={voll} onClick={() => { mitte.current = { x: 0.5, y: 0.5 }; setVoll((v) => !v); }}>
          {t(voll ? "bilder.gross.einpassen" : "bilder.gross.volle_groesse")}
        </button>
        <button type="button" ref={zuKnopf} className="bt-gross__knopf" aria-label={t("bilder.gross.zu")} title={t("bilder.gross.zu")} onClick={onZu}>{Symbol.zu}</button>
      </div>
      <div className="bt-gross__buehne" ref={buehne} data-voll={voll ? "an" : undefined} onClick={(ev) => { if (ev.target === ev.currentTarget) onZu(); }}>
        {url
          ? <img ref={element} src={url} alt="" data-vorschau={bild ? undefined : "an"} onClick={bildKlick} />
          : <span className="bt-gross__laedt">{t("bilder.kachel.laedt")}</span>}
      </div>
      {mehrere && <button type="button" className="bt-gross__knopf bt-gross__blatt bt-gross__blatt--zurueck" aria-label={t("bilder.gross.zurueck")} title={t("bilder.gross.zurueck")} onClick={() => blaettern(-1)}>{Symbol.links}</button>}
      {mehrere && <button type="button" className="bt-gross__knopf bt-gross__blatt bt-gross__blatt--vor" aria-label={t("bilder.gross.vor")} title={t("bilder.gross.vor")} onClick={() => blaettern(1)}>{Symbol.rechts}</button>}
    </div>,
    ziel ?? document.body,
  );
}
