// Zoom und Verschieben für die Bahn-Ansichten.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.4.
//
// # Warum ein gemeinsamer Zustand
//
// Längs- und Queransicht müssen fluchten — der Aufsetzpunkt oben senkrecht
// über der Marke unten. Mit zwei getrennten Zoomzuständen wäre das nach der
// ersten Mausbewegung vorbei, und niemand hätte es bemerkt, weil jede
// Ansicht für sich plausibel aussieht.
//
// Deshalb hält dieser Haken **einen** Ausschnitt in Metern, und beide
// Ansichten bekommen dieselbe Projektion daraus.
//
// # Warum in Metern und nicht in Pixeln
//
// Ein Pixelversatz müsste bei jeder Grössenänderung des Fensters
// umgerechnet werden. Der Ausschnitt in Metern ist von der Darstellung
// unabhängig: Wer auf den Aufsetzpunkt gezoomt hat, sieht ihn auch nach dem
// Umschalten auf ein schmales Fenster.

import { useCallback, useRef, useState } from "react";

export interface BahnZoom {
  /** Sichtbarer Bereich in Metern ab der Landeschwelle. */
  vonM: number | undefined;
  bisM: number | undefined;
  /** Ist hineingezoomt? Steuert den Zurücksetzen-Knopf. */
  gezoomt: boolean;
  /** Auf das Mausrad reagieren — an `onWheel` beider Ansichten hängen. */
  aufRad: (e: React.WheelEvent<SVGSVGElement>) => void;
  /** Ziehen zum Verschieben. */
  aufZiehStart: (e: React.MouseEvent<SVGSVGElement>) => void;
  aufZiehen: (e: React.MouseEvent<SVGSVGElement>) => void;
  aufZiehEnde: () => void;
  /** Zurück auf die ganze Bahn. */
  zuruecksetzen: () => void;
  /** Wird gerade gezogen? Für den Mauszeiger. */
  zieht: boolean;
}

/** Wie stark ein Radschritt vergrössert. */
const SCHRITT = 1.25;
/** Kleinster Ausschnitt — darunter wird die Projektion sinnlos. */
const MIN_SICHT_M = 50;

/**
 * @param ganzVonM  Anfang der Bahn in Metern (negativ bei versetzter Schwelle)
 * @param ganzBisM  Ende der Bahn in Metern
 */
export function useBahnZoom(ganzVonM: number, ganzBisM: number): BahnZoom {
  const [sicht, setSicht] = useState<{ von: number; bis: number } | null>(null);
  const [zieht, setZieht] = useState(false);
  const ziehStart = useRef<{ x: number; von: number; bis: number } | null>(null);

  const grenzen = useCallback(
    (von: number, bis: number) => {
      const breite = Math.max(MIN_SICHT_M, Math.min(bis - von, ganzBisM - ganzVonM));
      let v = von;
      if (v < ganzVonM) v = ganzVonM;
      if (v + breite > ganzBisM) v = ganzBisM - breite;
      return { von: v, bis: v + breite };
    },
    [ganzVonM, ganzBisM],
  );

  const aufRad = useCallback(
    (e: React.WheelEvent<SVGSVGElement>) => {
      e.preventDefault();
      const el = e.currentTarget;
      const kasten = el.getBoundingClientRect();
      // Wo unter dem Zeiger liegt der Ausschnitt? Dorthin wird gezoomt —
      // sonst wandert der Punkt, den man sich ansehen will, aus dem Bild.
      const anteil = kasten.width > 0 ? (e.clientX - kasten.left) / kasten.width : 0.5;

      const aktuell = sicht ?? { von: ganzVonM, bis: ganzBisM };
      const breite = aktuell.bis - aktuell.von;
      const unterZeiger = aktuell.von + anteil * breite;
      const neueBreite =
        e.deltaY < 0 ? breite / SCHRITT : breite * SCHRITT;
      const von = unterZeiger - anteil * neueBreite;
      const g = grenzen(von, von + neueBreite);
      // Auf die ganze Bahn herausgezoomt: den Zustand loswerden, damit die
      // Ansicht wieder ohne Ausschnitt rechnet.
      if (g.bis - g.von >= ganzBisM - ganzVonM - 0.5) {
        setSicht(null);
      } else {
        setSicht(g);
      }
    },
    [sicht, ganzVonM, ganzBisM, grenzen],
  );

  const aufZiehStart = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (!sicht) return; // ohne Zoom gibt es nichts zu verschieben
      ziehStart.current = { x: e.clientX, von: sicht.von, bis: sicht.bis };
      setZieht(true);
    },
    [sicht],
  );

  const aufZiehen = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      const start = ziehStart.current;
      if (!start) return;
      const kasten = e.currentTarget.getBoundingClientRect();
      if (kasten.width <= 0) return;
      const breite = start.bis - start.von;
      const meterJePixel = breite / kasten.width;
      const versatz = (e.clientX - start.x) * meterJePixel;
      setSicht(grenzen(start.von - versatz, start.bis - versatz));
    },
    [grenzen],
  );

  const aufZiehEnde = useCallback(() => {
    ziehStart.current = null;
    setZieht(false);
  }, []);

  const zuruecksetzen = useCallback(() => setSicht(null), []);

  return {
    vonM: sicht?.von,
    bisM: sicht?.bis,
    gezoomt: sicht != null,
    aufRad,
    aufZiehStart,
    aufZiehen,
    aufZiehEnde,
    zuruecksetzen,
    zieht,
  };
}
