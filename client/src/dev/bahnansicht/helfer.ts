// PROTOTYP: Auszug aus aeroacars-live/webapp/src/lib/landungsDatensatzAusTouchdown.ts (nur was der Payload-Mapper braucht).
type Roh = Record<string, unknown>;
const zahl = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const text = (v: unknown): string | null => (typeof v === "string" && v !== "" ? v : null);
function versionAb(v: string | null, ab: string): boolean {
  if (!v) return false;
  const a = v.split(/[.-]/).map((x) => parseInt(x, 10));
  const b = ab.split(".").map((x) => parseInt(x, 10));
  for (let i = 0; i < b.length; i++) {
    const x = Number.isFinite(a[i]) ? a[i]! : 0;
    if (x !== b[i]) return x > b[i]!;
  }
  return true;
}
export function bewerteteGAusPayload(pl: Roh): number | null {
  if (text(pl.simulator) === "xplane" && versionAb(text(pl.client_version), "1.6.2")) return null;
  return zahl(pl.scored_g_load);
}

/** IAS beim Aufsetzen: Der Client sendet 0, wenn er keine hat
 *  (`landing_speed_kt.unwrap_or(0)`) — dann leer, nicht „0 kt". */
export function aufsetzIasAusPayload(pl: Roh, spalte?: number | null): number | null {
  const v = zahl(pl.ias_kt) ?? zahl(spalte);
  return v != null && v > 0 ? v : null;
}

