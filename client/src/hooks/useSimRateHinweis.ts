// Sim-Rate-Hinweis (03.10.2026, Anlass TGW 882): Ein Pilot flog drei Stunden
// mit 0,25× und merkte es nicht. Der Hinweis erscheint, sobald die Rate
// laenger als `HINWEIS_NACH_MS` ununterbrochen von 1× abweicht — Zeitlupe
// zaehlt genauso wie Zeitraffer.
import { useEffect, useRef, useState } from "react";

export const HINWEIS_NACH_MS = 2 * 60 * 1000;
const TOLERANZ = 0.01;

/** Weicht die Rate spuerbar von 1× ab? Unbekannt/0/NaN zaehlt als 1×
 *  (der MSFS-Adapter hebt 0 ebenfalls auf 1 — „Zeit steht" ist keine Rate). */
export function simRateWeichtAb(rate: number | null | undefined): boolean {
  if (rate == null || !Number.isFinite(rate) || rate <= 0) return false;
  return Math.abs(rate - 1) > TOLERANZ;
}

/** Rate fuer die Anzeige: ganze Zahl ohne Nachkomma, sonst hoechstens 2 Stellen,
 *  mit dem Dezimalzeichen der Anzeigesprache (de/it „0,25“, en „0.25“).
 *  Abnahme 03.10.2026: das Komma war fest eingebaut, Englisch zeigte „0,25×“. */
export function simRateText(rate: number, sprache = "de"): string {
  return new Intl.NumberFormat(sprache, { maximumFractionDigits: 2 }).format(rate);
}

/** Liefert die Rate, wenn sie seit mindestens `HINWEIS_NACH_MS` abweicht,
 *  sonst `null`. Die Uhr startet bei jeder Rueckkehr auf 1× neu. */
export function useSimRateHinweis(
  rate: number | null | undefined,
  aktiv: boolean,
): number | null {
  const seit = useRef<number | null>(null);
  const [, setTick] = useState(0);
  const abweichend = aktiv && simRateWeichtAb(rate);

  if (!abweichend) {
    seit.current = null;
  } else if (seit.current === null) {
    seit.current = Date.now();
  }

  useEffect(() => {
    if (!abweichend) return;
    const id = window.setInterval(() => setTick((n) => n + 1), 5000);
    return () => window.clearInterval(id);
  }, [abweichend]);

  if (!abweichend || seit.current === null) return null;
  return Date.now() - seit.current >= HINWEIS_NACH_MS ? (rate as number) : null;
}
