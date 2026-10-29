// v3 (Etappe 5, P-8.21): Texte der eigenständigen Flugkarten-Seite.
// Dieselben Schlüssel `flugkarte.*` wie im Client (bundle/texte), beim Bau in
// die Seite gelegt (vite.flugkarte.config.mjs). Fehlt ein Schlüssel, liefert
// `uebersetzer` null — dann bleibt der deutsche Text aus der Akte stehen.

export type Texte = Record<string, unknown>;

/** `t` für den Baustein: Schlüssel wie „flugkarte.ui.titel“, Werte als {{name}}. */
export function uebersetzer(alle: Record<string, Texte>, sprache: string) {
  const texte = alle[sprache] ?? alle.en ?? {};
  return (schluessel: string, werte?: Record<string, string | number>): string | null => {
    const teile = schluessel.split(".");
    if (teile[0] !== "flugkarte") return null;
    let knoten: unknown = texte;
    for (const t of teile.slice(1)) knoten = (knoten as Texte | undefined)?.[t];
    if (typeof knoten !== "string") return null;
    return knoten.replace(/\{\{\s*(\w+)\s*\}\}/g, (_, n: string) => String(werte?.[n] ?? ""));
  };
}

/** Sprache der Seite: ?sprache=…, sonst die des Browsers, sonst Englisch. */
export function seitenSprache(suche: string, browser: string, vorhanden: string[]): string {
  const gewuenscht = [new URLSearchParams(suche).get("sprache"), browser.slice(0, 2)];
  return gewuenscht.find((s): s is string => !!s && vorhanden.includes(s)) ?? "en";
}

/** Nachricht der einbettenden Seite (Admin-Webapp): nur vom direkten Elternfenster
 *  und nur in dieser Form. */
export function akteAusNachricht(
  daten: unknown,
  quelleIstEltern: boolean,
): { akte: unknown; sprache?: string } | null {
  if (!quelleIstEltern || typeof daten !== "object" || daten === null) return null;
  const d = daten as { typ?: unknown; akte?: unknown; sprache?: unknown };
  if (d.typ !== "flugkarte-akte" || typeof d.akte !== "object" || d.akte === null) return null;
  return { akte: d.akte, sprache: typeof d.sprache === "string" ? d.sprache : undefined };
}
