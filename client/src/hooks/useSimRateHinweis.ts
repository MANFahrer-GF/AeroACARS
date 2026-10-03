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

/** Rate fuer die Anzeige: ganze Zahl ohne Komma, sonst hoechstens 2 Stellen. */
export function simRateText(rate: number): string {
  const gerundet = Math.round(rate * 100) / 100;
  return String(gerundet).replace(".", ",");
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
