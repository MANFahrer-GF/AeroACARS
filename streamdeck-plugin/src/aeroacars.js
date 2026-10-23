// Verbindung zum lokalen Panel-Server von AeroACARS (nur 127.0.0.1, ohne PIN).
// Lesen: /panel/status, /panel/kabine, /panel/hoppie, /panel/debrief.
// Bedienen: POST /panel/cmd/{name} mit Pflichtkopf X-AeroACARS-Deck: 1.

export const BASIS = "http://127.0.0.1:47847";
/** Abfragen laufen jede Sekunde: kurze Frist, damit ein hängendes AeroACARS nichts stapelt. */
const ZEITLIMIT_MS = 1500;
/**
 * Befehle dürfen länger dauern. METAR geht übers Internet zu aviationweather.gov
 * — Feldbefund 23.10.: nach 1,5 s brach das Plugin ab (gelbes Dreieck), das
 * Wetter kam trotzdem an.
 */
const BEFEHL_FRIST_MS = { metar_laden: 12000 };
const BEFEHL_STANDARD_MS = 5000;

async function holen(pfad, init = {}, frist = ZEITLIMIT_MS) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), frist);
  try {
    const res = await fetch(BASIS + pfad, { ...init, signal: ctl.signal });
    // Fehltext von AeroACARS mitnehmen (z. B. {"code":"metar_upstream",…}) — fürs Plugin-Log.
    if (!res.ok) throw new Error(`${pfad}: HTTP ${res.status} ${(await res.text().catch(() => "")).slice(0, 200)}`);
    return await res.json();
  } finally {
    clearTimeout(t);
  }
}

export const lesen = (pfad) => holen(pfad);

/** Löst einen Befehl aus; Antwort ist der frische Kabinenzustand. */
export function befehl(name, args = {}) {
  return holen(`/panel/cmd/${name}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-AeroACARS-Deck": "1" },
    body: JSON.stringify(args),
  }, BEFEHL_FRIST_MS[name] ?? BEFEHL_STANDARD_MS);
}
