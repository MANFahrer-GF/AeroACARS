// Entscheidung, ob ein Fund aus dem Eingang uebernommen wird (P-11.30).
// Rust legt nur Funde aus aktivem Flug an; hier gelten die Regeln der Quelle:
//   standard    X-Plane `Output/screenshots`: wird verschoben, keine weitere Regel
//   aufgespuert Ordner aus `bild_quellen_finden`: nur kopiert, Simulator im Vordergrund
//               UND Pixelmasse passen zur Simulatoraufloesung
//   hand        vom Piloten gewaehlter Ordner: nur kopiert, keine Vordergrund-Regel
// Meldet das Programm das Simulatorfenster nicht (`sim_fenster` null), entfaellt die
// Massregel, sie lehnt nie ab (Luecke bis das Programm es liefert).
import type { Fund, Quelle } from "./typen";

export type VerwerfGrund = "format" | "vordergrund" | "masse";

export type Entscheidung =
  | { tat: "uebernehmen"; modus: "kopieren" | "verschieben" }
  | { tat: "verwerfen"; grund: VerwerfGrund };

/** Toleranz fuer die Pixelmasse (Rahmen, DPI-Rundung). */
export const MASS_TOLERANZ_PX = 4;

export function massePassen(fund: Pick<Fund, "px_breite" | "px_hoehe" | "sim_fenster">): boolean {
  const f = fund.sim_fenster;
  if (!f || fund.px_breite === null || fund.px_hoehe === null) return true;
  return Math.abs(fund.px_breite - f.breite) <= MASS_TOLERANZ_PX && Math.abs(fund.px_hoehe - f.hoehe) <= MASS_TOLERANZ_PX;
}

export function entscheide(fund: Fund, quellen: Quelle[]): Entscheidung {
  if (!fund.uebernehmbar) return { tat: "verwerfen", grund: "format" };
  if (fund.art === "aufgespuert") {
    // Nur ein belegtes „nein“ verwirft: Der Mac kann den Vordergrund nicht ermitteln (Wert leer).
    if (fund.sim_vordergrund === false) return { tat: "verwerfen", grund: "vordergrund" };
    if (!massePassen(fund)) return { tat: "verwerfen", grund: "masse" };
  }
  if (fund.art === "standard") return { tat: "uebernehmen", modus: "verschieben" };
  const q = quellen.find((x) => x.pfad === fund.quelle);
  return { tat: "uebernehmen", modus: q?.original_entfernen === true && fund.art !== "aufgespuert" ? "verschieben" : "kopieren" };
}

/** Ordner (Gruppe) im Bilderordner: PIREP, sonst der Ordner fuer Bilder ohne Flug. */
export function gruppeFuer(pirepId: string | null | undefined, ohneFlug: string): string {
  const p = (pirepId ?? "").trim();
  // Nur Zeichen, die als Ordnername unbedenklich sind; alles andere landet unter „ohne Flug“.
  return /^[A-Za-z0-9_-]{1,64}$/.test(p) ? p : ohneFlug;
}
