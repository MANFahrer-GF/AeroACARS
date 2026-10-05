// Bordbuch im Landungs-Tab und im PDF-Bericht (27.09.2026, Thomas: „wo sieht
// der Pilot das nach der Landung — und kommt es aufs PDF?"). Dieselbe
// Checkliste wie im Reiter „Bordbuch", an die Landung gehängt.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/ipc";
import type { Eintrag, Regel } from "../../lib/bordbuch";
import "./bordbuch.css";

/** Lädt das Bordbuch einer PIREP und hält es aktuell (ATC-Markierung,
 *  Server-Abgleich). `null` = zu diesem Flug gibt es keins (ältere Flüge). */
export function useBordbuchEintrag(pirepId: string | null | undefined) {
  const [eintrag, setEintrag] = useState<Eintrag | null>(null);
  // Zu welcher Landung der Stand gehört — `bereit` heisst: für DIESE Landung
  // ist die erste Antwort da (der PDF-Export wartet darauf).
  const [geladenFuer, setGeladenFuer] = useState<string | null>(null);
  // Jede Anfrage bekommt eine Nummer; nur die jüngste darf schreiben. Sonst
  // überschreibt eine späte Antwort (vorige Landung, oder ein Laden, das vor
  // der ATC-Markierung losging) den neueren Stand (Codex-QS).
  const aktuell = useRef(pirepId);
  aktuell.current = pirepId;
  const nummer = useRef(0);
  const laden = useCallback(async () => {
    if (!pirepId) return;
    const meine = ++nummer.current;
    let neu: Eintrag | null = null;
    try {
      const e = await invoke<Eintrag | null>("bordbuch_eintrag", { pirepId });
      // Nur echte Einträge übernehmen (Tests/ältere Brücken liefern evtl. Unsinn).
      neu = e && Array.isArray((e as Eintrag).punkte) ? e : null;
    } catch {
      neu = null;
    }
    if (meine !== nummer.current || aktuell.current !== pirepId) return;
    setEintrag(neu);
    setGeladenFuer(pirepId);
  }, [pirepId]);
  useEffect(() => {
    setEintrag(null);
    void laden();
    let aktiv = true;
    let weg: (() => void) | undefined;
    // Ein Fehler hier darf die Landungsseite nie mitreissen — das Bordbuch
    // ist Beiwerk (ohne Aktualisierung bleibt es beim geladenen Stand).
    try {
      Promise.resolve(listen("bordbuch_geaendert", () => void laden()))
        .then((u) => {
          if (typeof u !== "function") return;
          if (aktiv) weg = u;
          else u();
        })
        .catch(() => undefined);
    } catch {
      /* ohne Ereigniskanal */
    }
    return () => {
      aktiv = false;
      weg?.();
    };
  }, [laden]);
  const markieren = useCallback(
    async (regel: Regel, nachAtc: boolean) => {
      if (!pirepId) return;
      const meine = ++nummer.current;
      const neu = await invoke<Eintrag | null>("bordbuch_markieren", { pirepId, regel, nachAtc });
      if (aktuell.current !== pirepId) return;
      if (neu && meine === nummer.current) {
        setEintrag(neu);
      } else {
        // Ein Laden lief dazwischen und hat evtl. den Stand VOR der Markierung
        // gelesen. Die Markierung ist gespeichert — neu laden, das gewinnt.
        void laden();
      }
    },
    [pirepId, laden],
  );
  const bereit = !pirepId || geladenFuer === pirepId;
  return { eintrag: bereit ? eintrag : null, markieren, bereit };
}

// Anzeige (Kopf, Abschnitt, PDF-Fassung): BordbuchLandungsAnzeige.tsx
// (gespiegelt in die Webapp).
export { BordbuchBericht, BordbuchLandungsAbschnitt } from "./BordbuchLandungsAnzeige";
