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
  /**
   * An das `ref` des SVG hängen — **nicht** an `onWheel`.
   *
   * React registriert Rad-Ereignisse als *passive* Zuhörer. In einem
   * passiven Zuhörer ist `preventDefault()` wirkungslos: Der Zoom griff,
   * und der Browser zoomte die ganze Seite gleich mit, weil Strg + Rad
   * seine eigene Bedeutung hat. Gemessen am 23.08.2026 — der Aufruf lief,
   * das Ereignis blieb trotzdem unverhindert.
   *
   * Deshalb hängt der Zuhörer hier von Hand am Element, mit
   * `{ passive: false }`.
   */
  radAnschluss: (el: SVGSVGElement | null) => (() => void) | undefined;
  /** Ziehen zum Verschieben. */
  aufZiehStart: (e: React.MouseEvent<SVGSVGElement>) => void;
  aufZiehen: (e: React.MouseEvent<SVGSVGElement>) => void;
  aufZiehEnde: () => void;
  /** Zurück auf die ganze Bahn. */
  zuruecksetzen: () => void;
  /** Eine Stufe näher (1) oder weiter weg (−1) — für Knöpfe. */
  stufe: (richtung: 1 | -1) => void;
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
    (e: WheelEvent) => {
      // NUR mit Strg oder Cmd. Ohne diese Bedingung verschluckt die Grafik
      // jedes Mausrad-Ereignis, das über ihr auftritt — und die Seite lässt
      // sich nicht mehr scrollen, sobald der Zeiger darüber steht. Genau
      // das ist beim ersten Anlauf passiert.
      //
      // Dieselbe Regel benutzen Karten in Dokumenten, und sie ist aus
      // gutem Grund verbreitet: Scrollen ist die häufigere Absicht.
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const el = e.currentTarget as SVGSVGElement;
      const kasten = el.getBoundingClientRect();
      // Wo unter dem Zeiger liegt der Ausschnitt? Dorthin wird gezoomt —
      // sonst wandert der Punkt, den man sich ansehen will, aus dem Bild.
      const anteil = kasten.width > 0 ? (e.clientX - kasten.left) / kasten.width : 0.5;

      // Wie bei `stufe`: aus dem vorigen Zustand rechnen. Ein Mausrad
      // liefert mehrere Ereignisse pro Umdrehung, und sie kommen schneller
      // als React rendert.
      setSicht((vorher) => {
        const aktuell = vorher ?? { von: ganzVonM, bis: ganzBisM };
        const breite = aktuell.bis - aktuell.von;
        const unterZeiger = aktuell.von + anteil * breite;
        const neueBreite = e.deltaY < 0 ? breite / SCHRITT : breite * SCHRITT;
        const von = unterZeiger - anteil * neueBreite;
        const g = grenzen(von, von + neueBreite);
        // Auf die ganze Bahn herausgezoomt: den Zustand loswerden, damit
        // die Ansicht wieder ohne Ausschnitt rechnet.
        return g.bis - g.von >= ganzBisM - ganzVonM - 0.5 ? null : g;
      });
    },
    [ganzVonM, ganzBisM, grenzen],
  );

  // Der Zuhörer hängt von Hand am Element, nicht über `onWheel` — siehe
  // `radAnschluss` oben.
  //
  // Er ruft über `radRef` auf und bleibt dadurch selbst unverändert: Ein
  // Zuhörer, der bei jedem Zoomschritt ab- und wieder angemeldet wird,
  // verliert die Ereignisse, die genau dazwischen eintreffen — und ein
  // Mausrad liefert mehrere pro Umdrehung.
  const radRef = useRef(aufRad);
  radRef.current = aufRad;

  // Ein Anschluss für BEIDE Ansichten. Längs- und Queransicht teilen sich
  // den Zoomzustand (sonst fluchten sie nicht mehr), also hängen beide
  // hier — die Aufräumfunktion aus React 19 hält auseinander, welches
  // Element gerade geht.
  const radAnschluss = useCallback((el: SVGSVGElement | null) => {
    if (!el) return;
    const zuhoerer = (e: WheelEvent) => radRef.current(e);
    el.addEventListener("wheel", zuhoerer, { passive: false });
    return () => el.removeEventListener("wheel", zuhoerer);
  }, []);

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

  /**
   * Zoomen ohne Tastatur — für die Knöpfe neben der Grafik.
   *
   * Zoomt auf die Mitte des aktuellen Ausschnitts. Wer eine bestimmte
   * Stelle vergrössern will, nimmt Strg und das Rad; die Knöpfe sind für
   * den Fall, dass man nur schnell näher heran möchte.
   */
  const stufe = useCallback(
    (richtung: 1 | -1) => {
      // Aus dem VORIGEN Zustand rechnen, nicht aus `sicht`.
      //
      // `sicht` stammt aus dem Render, in dem der Knopf gezeichnet wurde.
      // Wer zweimal schnell drückt, löst beide Klicks vor dem nächsten
      // Render aus — beide lesen denselben Ausgangswert, und der zweite
      // Schritt geht verloren. Beim Prüfen sah das aus, als reagiere der
      // Knopf überhaupt nicht.
      setSicht((vorher) => {
        const aktuell = vorher ?? { von: ganzVonM, bis: ganzBisM };
        const breite = aktuell.bis - aktuell.von;
        const mitte = (aktuell.von + aktuell.bis) / 2;
        const neueBreite = richtung > 0 ? breite / SCHRITT : breite * SCHRITT;
        const g = grenzen(mitte - neueBreite / 2, mitte + neueBreite / 2);
        // Ganz herausgezoomt: den Zustand loswerden, damit die Ansicht
        // wieder ohne Ausschnitt rechnet.
        return g.bis - g.von >= ganzBisM - ganzVonM - 0.5 ? null : g;
      });
    },
    [ganzVonM, ganzBisM, grenzen],
  );

  return {
    vonM: sicht?.von,
    bisM: sicht?.bis,
    gezoomt: sicht != null,
    radAnschluss,
    aufZiehStart,
    aufZiehen,
    aufZiehEnde,
    zuruecksetzen,
    stufe,
    zieht,
  };
}
