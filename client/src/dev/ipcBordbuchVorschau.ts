// Ersatz-IPC nur für die Landungs-Vorschau (landung.html): liefert das
// Beispiel-Bordbuch, damit der Abschnitt ohne laufenden Client sichtbar wird.
import { BEISPIEL_BORDBUCH } from "./mockBordbuch";

let eintrag = BEISPIEL_BORDBUCH;

export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (cmd === "bordbuch_eintrag") return eintrag as T;
  if (cmd === "bordbuch_markieren") {
    const { regel, nachAtc } = args as { regel: string; nachAtc: boolean };
    eintrag = {
      ...eintrag,
      punkte: eintrag.punkte.map((p) =>
        p.regel === regel ? { ...p, status: nachAtc ? "nach_atc" : p.auto_status } : p,
      ),
    };
    return eintrag as T;
  }
  throw new Error(`Vorschau: ${cmd} nicht verfügbar`);
}

export async function listen(): Promise<() => void> {
  return () => undefined;
}
